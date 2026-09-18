// Isolated, memory-limited codec worker. Rust owns rendition math, staging,
// cancellation and atomic publication. No file controls executable/library paths.
#include <avif/avif.h>
#include <ultrahdr_api.h>
#include <cstdio>
#include <jpeglib.h>
#include <sys/resource.h>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

static void require(bool ok, const char* message) { if (!ok) throw std::runtime_error(message); }
static std::vector<uint8_t> load(const char* path, uint64_t limit) {
  std::ifstream f(path, std::ios::binary | std::ios::ate);
  require(bool(f), "Cannot open codec input");
  auto n = f.tellg(); require(n >= 0 && uint64_t(n) <= limit, "Codec input exceeds budget");
  std::vector<uint8_t> b(static_cast<size_t>(n)); f.seekg(0);
  require(bool(f.read(reinterpret_cast<char*>(b.data()), n)), "Incomplete codec input"); return b;
}
static void save(const char* path, const void* data, size_t length) {
  std::ofstream f(path, std::ios::binary); f.write(static_cast<const char*>(data), length);
  f.close(); require(bool(f), "Cannot write codec output");
}
static void avif_ok(avifResult r) { if (r != AVIF_RESULT_OK) throw std::runtime_error(avifResultToString(r)); }
static void uhdr_ok(uhdr_error_info_t r) { if (r.error_code != UHDR_CODEC_OK) throw std::runtime_error(r.has_detail ? r.detail : "Ultra HDR codec failed"); }
static void jpeg_error(j_common_ptr c) { char text[JMSG_LENGTH_MAX]; c->err->format_message(c, text); std::fprintf(stderr,"%s\n",text); std::exit(2); }
// Row-based JPEG work stays in this killable process, including entropy coding.
static void jpeg_encode(const char* input, const char* output, unsigned w, unsigned h, int quality, bool icc) {
  std::ifstream raw(input, std::ios::binary); require(bool(raw), "Missing RGB rows");
  FILE* f = std::fopen(output,"wb"); require(f, "Cannot create JPEG");
  jpeg_compress_struct c{}; jpeg_error_mgr errors{};
  c.err = jpeg_std_error(&errors); errors.error_exit = jpeg_error;
  jpeg_create_compress(&c); jpeg_stdio_dest(&c,f); c.image_width=w;c.image_height=h;c.input_components=3;c.in_color_space=JCS_RGB;
  jpeg_set_defaults(&c); if (!icc) jpeg_set_colorspace(&c,JCS_RGB); jpeg_set_quality(&c,quality,TRUE);
  for(int i=0;i<3;i++){c.comp_info[i].h_samp_factor=1;c.comp_info[i].v_samp_factor=1;}
  jpeg_start_compress(&c,TRUE);
  if(icc){auto p=load("profile",4*1024*1024);jpeg_write_icc_profile(&c,p.data(),p.size());
    if(std::ifstream("exif").good()){auto e=load("exif",65533);jpeg_write_marker(&c,JPEG_APP0+1,e.data(),e.size());}
  }
  std::vector<uint8_t> row(size_t(w)*3);
  while(c.next_scanline<h){require(bool(raw.read(reinterpret_cast<char*>(row.data()),row.size())),"Incomplete RGB rows");JSAMPROW p=row.data();jpeg_write_scanlines(&c,&p,1);}
  jpeg_finish_compress(&c);jpeg_destroy_compress(&c);require(std::fclose(f)==0,"Cannot finish JPEG");
}
static void jpeg_decode_base(unsigned w,unsigned h) {
  FILE* f=std::fopen("base.jpg","rb");require(f,"Missing JPEG base");
  jpeg_decompress_struct c{};jpeg_error_mgr errors{};c.err=jpeg_std_error(&errors);errors.error_exit=jpeg_error;
  jpeg_create_decompress(&c);jpeg_stdio_src(&c,f);jpeg_read_header(&c,TRUE);c.out_color_space=JCS_RGB;
  require(c.image_width==w&&c.image_height==h,"JPEG dimensions changed");jpeg_start_decompress(&c);
  std::ofstream out("base-decoded",std::ios::binary);std::vector<uint8_t> row(size_t(w)*3);
  while(c.output_scanline<h){JSAMPROW p=row.data();jpeg_read_scanlines(&c,&p,1);out.write(reinterpret_cast<char*>(row.data()),row.size());}
  jpeg_finish_decompress(&c);jpeg_destroy_decompress(&c);std::fclose(f);out.close();require(bool(out),"Cannot write decoded base");
}
struct Metadata { float low, high, offset, headroom; };
static Metadata metadata(){auto bytes=load("metadata",sizeof(Metadata));require(bytes.size()==sizeof(Metadata),"Invalid gain metadata");Metadata m;std::memcpy(&m,bytes.data(),sizeof(m));require(std::isfinite(m.low)&&std::isfinite(m.high)&&m.high>m.low&&m.offset>0&&m.headroom>0,"Invalid gain metadata");return m;}
static void jpeg_mux(unsigned w,unsigned h,uint64_t budget) {
  jpeg_encode("gain.raw","gain.jpg",w,h,100,false);
  auto base=load("base.jpg",budget/4),gain=load("gain.jpg",budget/4);auto m=metadata();
  uhdr_gainmap_metadata_t meta{};
  for(int i=0;i<3;i++){meta.min_content_boost[i]=std::exp2(m.low);meta.max_content_boost[i]=std::exp2(m.high);meta.gamma[i]=1;meta.offset_sdr[i]=meta.offset_hdr[i]=m.offset;}
  meta.hdr_capacity_min=1;meta.hdr_capacity_max=std::exp2(m.headroom);meta.use_base_cg=1;
  uhdr_compressed_image_t b{base.data(),base.size(),base.size(),UHDR_CG_BT_2100,UHDR_CT_SRGB,UHDR_CR_FULL_RANGE};
  uhdr_compressed_image_t g{gain.data(),gain.size(),gain.size(),UHDR_CG_BT_2100,UHDR_CT_UNSPECIFIED,UHDR_CR_FULL_RANGE};
  auto enc=uhdr_create_encoder();require(enc,"Cannot allocate Ultra HDR encoder");
  uhdr_ok(uhdr_enc_set_compressed_image(enc,&b,UHDR_BASE_IMG));uhdr_ok(uhdr_enc_set_gainmap_image(enc,&g,&meta));uhdr_ok(uhdr_encode(enc));
  auto output=uhdr_get_encoded_stream(enc);require(output,"Missing Ultra HDR output");save("encoded",output->data,output->data_sz);uhdr_release_encoder(enc);
}
static avifImage* avif_rgb(const char* path,unsigned w,unsigned h,bool alpha,uint64_t budget) {
  auto raw=load(path,budget/3);require(raw.size()==size_t(w)*h*(alpha?8:6),"Invalid AVIF RGB rows");
  auto image=avifImageCreate(w,h,12,AVIF_PIXEL_FORMAT_YUV444);require(image,"Cannot allocate AVIF");
  image->colorPrimaries=alpha?AVIF_COLOR_PRIMARIES_BT2020:AVIF_COLOR_PRIMARIES_UNSPECIFIED;
  image->transferCharacteristics=alpha?AVIF_TRANSFER_CHARACTERISTICS_SRGB:AVIF_TRANSFER_CHARACTERISTICS_UNSPECIFIED;
  image->matrixCoefficients=AVIF_MATRIX_COEFFICIENTS_IDENTITY;image->yuvRange=AVIF_RANGE_FULL;
  avifRGBImage rgb;avifRGBImageSetDefaults(&rgb,image);rgb.format=alpha?AVIF_RGB_FORMAT_RGBA:AVIF_RGB_FORMAT_RGB;
  rgb.depth=12;rgb.pixels=raw.data();rgb.rowBytes=w*(alpha?8:6);rgb.alphaPremultiplied=AVIF_FALSE;rgb.avoidLibYUV=AVIF_TRUE;rgb.maxThreads=2;
  avif_ok(avifImageRGBToYUV(image,&rgb));return image;
}
static void avif_encode(unsigned w,unsigned h,int quality,uint64_t budget) {
  auto image=avif_rgb("base.raw",w,h,true,budget);auto m=metadata();
  if(std::ifstream("exif").good()){auto e=load("exif",65533);require(e.size()>6,"Invalid Exif");avif_ok(avifImageSetMetadataExif(image,e.data()+6,e.size()-6));}
  auto gm=avifGainMapCreate();require(gm,"Cannot allocate gain map");image->gainMap=gm;
  gm->image=avif_rgb("gain.raw",w,h,false,budget);
  for(int c=0;c<3;c++){
    gm->gainMapMin[c]={int32_t(std::round(m.low*1000000)),1000000};
    gm->gainMapMax[c]={int32_t(std::round(m.high*1000000)),1000000};gm->gainMapGamma[c]={1,1};
    gm->baseOffset[c]=gm->alternateOffset[c]={1,64};
  }
  gm->baseHdrHeadroom={0,1};gm->alternateHdrHeadroom={uint32_t(std::round(m.headroom*1000000)),1000000};gm->useBaseColorSpace=AVIF_TRUE;
  gm->altColorPrimaries=AVIF_COLOR_PRIMARIES_BT2020;gm->altTransferCharacteristics=AVIF_TRANSFER_CHARACTERISTICS_LINEAR;
  gm->altMatrixCoefficients=AVIF_MATRIX_COEFFICIENTS_IDENTITY;gm->altYUVRange=AVIF_RANGE_FULL;gm->altDepth=16;gm->altPlaneCount=4;
  auto enc=avifEncoderCreate();require(enc,"Cannot allocate AVIF encoder");enc->codecChoice=AVIF_CODEC_CHOICE_AOM;enc->maxThreads=2;
  enc->speed=8;enc->quality=quality;enc->qualityAlpha=100;enc->qualityGainMap=100;
  avifRWData data=AVIF_DATA_EMPTY;avif_ok(avifEncoderWrite(enc,image,&data));save("encoded",data.data,data.size);
  avifRWDataFree(&data);avifEncoderDestroy(enc);avifImageDestroy(image);
}
// Header is LE u32 width,height,primaries,transfer,bytes-per-pixel. Native worker
// builds currently target little-endian Linux, checked in the build recipe.
static void raw_output(const char* path,unsigned w,unsigned h,unsigned primaries,unsigned transfer,unsigned bpp,const void* data,size_t stride){
  std::ofstream f(path,std::ios::binary);uint32_t header[]={w,h,primaries,transfer,bpp};f.write(reinterpret_cast<char*>(header),sizeof(header));
  for(unsigned y=0;y<h;y++)f.write(static_cast<const char*>(data)+size_t(y)*stride,size_t(w)*bpp);
  f.close();require(bool(f),"Cannot write decoded HDR");
}
static void jpeg_read(uint64_t budget,unsigned max_dimension) {
  auto data=load("source",budget/4);uhdr_compressed_image_t input{data.data(),data.size(),data.size(),UHDR_CG_UNSPECIFIED,UHDR_CT_UNSPECIFIED,UHDR_CR_UNSPECIFIED};
  auto dec=uhdr_create_decoder();require(dec,"Cannot allocate Ultra HDR decoder");uhdr_ok(uhdr_dec_set_image(dec,&input));uhdr_ok(uhdr_dec_set_out_img_format(dec,UHDR_IMG_FMT_64bppRGBAHalfFloat));uhdr_ok(uhdr_dec_set_out_color_transfer(dec,UHDR_CT_LINEAR));uhdr_ok(uhdr_dec_set_out_max_display_boost(dec,std::numeric_limits<float>::max()));uhdr_ok(uhdr_dec_probe(dec));
  int w=uhdr_dec_get_image_width(dec),h=uhdr_dec_get_image_height(dec);require(w>0&&h>0&&unsigned(w)<=max_dimension&&unsigned(h)<=max_dimension&&uint64_t(w)*h<=budget/64,"HDR JPEG exceeds memory or dimension budget");
  auto meta=uhdr_dec_get_gainmap_metadata(dec);require(meta,"JPEG contains no supported gain map");
  uhdr_ok(uhdr_decode(dec));
  auto image=uhdr_get_decoded_image(dec);require(image,"Missing HDR pixels");
  unsigned cg=image->cg==UHDR_CG_BT_709?1:image->cg==UHDR_CG_DISPLAY_P3?12:image->cg==UHDR_CG_BT_2100?9:0;require(cg,"Unsupported HDR JPEG primaries");
  raw_output("decoded",w,h,cg,8,8,image->planes[0],image->stride[0]*8);
  auto base=uhdr_dec_get_base_image(dec);require(base,"Missing SDR base");save("base.jpg",base->data,base->data_sz);jpeg_decode_base(w,h);uhdr_release_decoder(dec);
}
static void avif_read(uint64_t budget,unsigned max_dimension) {
  auto data=load("source",budget/4);auto dec=avifDecoderCreate();require(dec,"Cannot allocate AVIF decoder");
  dec->codecChoice=AVIF_CODEC_CHOICE_DAV1D;dec->maxThreads=2;dec->imageDimensionLimit=max_dimension;
  dec->imageSizeLimit=uint32_t(std::min<uint64_t>(budget/80,AVIF_DEFAULT_IMAGE_SIZE_LIMIT));dec->imageCountLimit=1;
  dec->imageContentToDecode=AVIF_IMAGE_CONTENT_ALL;avif_ok(avifDecoderSetIOMemory(dec,data.data(),data.size()));avif_ok(avifDecoderParse(dec));
  require(!dec->imageSequenceTrackPresent,"HDR AVIF sequences need a still-image export");
  require(dec->image->transformFlags==AVIF_TRANSFORM_NONE,"Oriented or cropped HDR AVIF is not supported yet");
  require(dec->image->gainMap,"AVIF contains no supported gain map");avif_ok(avifDecoderNextImage(dec));
  auto image=dec->image;auto gm=image->gainMap;require(gm&&gm->alternateHdrHeadroom.d,"Invalid AVIF gain map headroom");
  require(!image->icc.size && (image->colorPrimaries==1 || image->colorPrimaries==9 || image->colorPrimaries==12) && image->transferCharacteristics==13,
          "HDR AVIF requires tagged sRGB-transfer RGB without an overriding ICC profile");
  require((gm->useBaseColorSpace || gm->altColorPrimaries==image->colorPrimaries) && gm->image->width==image->width && gm->image->height==image->height,
          "This AVIF gain-map color space or resolution is not supported yet");
  require(gm->baseHdrHeadroom.d && gm->alternateHdrHeadroom.d && double(gm->alternateHdrHeadroom.n)/gm->alternateHdrHeadroom.d > double(gm->baseHdrHeadroom.n)/gm->baseHdrHeadroom.d,
          "HDR AVIF requires an SDR base and an HDR alternate");
  avifRGBImage rgb;avifRGBImageSetDefaults(&rgb,image);rgb.format=AVIF_RGB_FORMAT_RGBA;rgb.depth=16;rgb.isFloat=AVIF_FALSE;rgb.maxThreads=2;rgb.alphaPremultiplied=AVIF_FALSE;
  avif_ok(avifRGBImageAllocatePixels(&rgb));avif_ok(avifImageYUVToRGB(image,&rgb));
  raw_output("decoded",image->width,image->height,image->colorPrimaries,13,8,rgb.pixels,rgb.rowBytes);
  raw_output("fallback",image->width,image->height,image->colorPrimaries,13,8,rgb.pixels,rgb.rowBytes);
  avifRGBImageFreePixels(&rgb);
  avifRGBImageSetDefaults(&rgb,gm->image);rgb.format=AVIF_RGB_FORMAT_RGB;rgb.depth=16;rgb.maxThreads=2;
  avif_ok(avifRGBImageAllocatePixels(&rgb));avif_ok(avifImageYUVToRGB(gm->image,&rgb));
  std::ofstream gains("decoded-gain",std::ios::binary);
  for(unsigned y=0;y<image->height;y++) gains.write(reinterpret_cast<char*>(rgb.pixels)+size_t(y)*rgb.rowBytes,size_t(image->width)*6);
  gains.close();require(bool(gains),"Cannot write decoded gain map");
  float metadata[15];
  for(int c=0;c<3;c++) {
    require(gm->gainMapMin[c].d && gm->gainMapMax[c].d && gm->gainMapGamma[c].d && gm->gainMapGamma[c].n && gm->baseOffset[c].d && gm->alternateOffset[c].d,"Invalid AVIF gain metadata");
    metadata[c]=float(gm->gainMapMin[c].n)/gm->gainMapMin[c].d;
    metadata[3+c]=float(gm->gainMapMax[c].n)/gm->gainMapMax[c].d;
    metadata[6+c]=float(gm->gainMapGamma[c].n)/gm->gainMapGamma[c].d;
    metadata[9+c]=float(gm->baseOffset[c].n)/gm->baseOffset[c].d;
    metadata[12+c]=float(gm->alternateOffset[c].n)/gm->alternateOffset[c].d;
  }
  save("decoded-metadata",metadata,sizeof(metadata));
  if(dec->image->exif.size)save("decoded-exif",dec->image->exif.data,dec->image->exif.size);
  avifRGBImageFreePixels(&rgb);avifDecoderDestroy(dec);
}
int main(int argc,char** argv){
  try{
    if(argc==2&&std::string(argv[1])=="--version"){std::puts("capy-hdr-codec 1");return 0;}
    require(argc==7,"Invalid codec command");std::string mode=argv[1];
    auto number=[](const char* s){char* end=nullptr;auto n=std::strtoull(s,&end,10);require(end&&!*end,"Invalid codec argument");return n;};
    unsigned w=number(argv[2]),h=number(argv[3]),quality=number(argv[4]),dimension=number(argv[6]);uint64_t budget=number(argv[5]);
    require(budget>=32*1024*1024&&budget<=uint64_t(16)*1024*1024*1024&&dimension>0&&dimension<=32768,"Invalid codec budget");
    rlimit memory{budget,budget},cpu{600,600},core{0,0},file{budget,budget};require(!setrlimit(RLIMIT_AS,&memory)&&!setrlimit(RLIMIT_CPU,&cpu)&&!setrlimit(RLIMIT_CORE,&core)&&!setrlimit(RLIMIT_FSIZE,&file),"Cannot constrain codec process");
    if(mode=="decode-jpeg")jpeg_read(budget,dimension);
    else if(mode=="decode-avif")avif_read(budget,dimension);
    else{
      require(w&&h&&w<=dimension&&h<=dimension&&uint64_t(w)*h<=budget/80&&quality>=1&&quality<=100,"HDR export exceeds codec budget");
      if(mode=="base-jpeg"){jpeg_encode("base.raw","base.jpg",w,h,quality,true);jpeg_decode_base(w,h);}
      else if(mode=="mux-jpeg")jpeg_mux(w,h,budget);
      else if(mode=="encode-avif")avif_encode(w,h,quality,budget);
      else throw std::runtime_error("Unknown codec operation");
    }
    return 0;
  }catch(const std::exception& e){std::fprintf(stderr,"%s\n",e.what());return 2;}
}
