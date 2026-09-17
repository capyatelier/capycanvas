// Independent container + AOM decode for small AVIF qualification fixtures.
// Build against the matching public libavif 1.3.0 header. Output is oriented,
// straight RGBA16LE; no gain-map tone mapping or display conversion is applied.
#include "avif.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(avifResult result) {
  if (result != AVIF_RESULT_OK) {
    fprintf(stderr, "%s\n", avifResultToString(result)); exit(1);
  }
}
static FILE* output(const char* prefix, const char* suffix) {
  char path[4096];
  if (snprintf(path, sizeof(path), "%s%s", prefix, suffix) >= (int) sizeof(path)) exit(1);
  FILE* file = fopen(path, "wb");
  if (!file) { perror(path); exit(1); }
  return file;
}
int main(int argc, char** argv) {
  if ((argc != 3 && argc != 4) || strcmp(avifVersion(), "1.3.0")) {
    fprintf(stderr, "Usage: avif-reference INPUT OUTPUT_PREFIX [poster] (requires libavif 1.3.0)\n"); return 1;
  }
  if (!avifCodecName(AVIF_CODEC_CHOICE_AOM, AVIF_CODEC_FLAG_CAN_DECODE)) return 1;
  avifDecoder* decoder = avifDecoderCreate();
  if (!decoder) return 1;
  decoder->codecChoice = AVIF_CODEC_CHOICE_AOM;
  decoder->maxThreads = 2;
  decoder->imageSizeLimit = 4 * 1024 * 1024;
  decoder->imageDimensionLimit = 4096;
  decoder->imageCountLimit = 100;
  check(avifDecoderSetIOFile(decoder, argv[1]));
  check(avifDecoderParse(decoder));
  if (argc == 4) {
    if (strcmp(argv[3], "poster")) return 1;
    check(avifDecoderSetSource(decoder, AVIF_DECODER_SOURCE_PRIMARY_ITEM));
  } else if (decoder->imageSequenceTrackPresent) {
    check(avifDecoderSetSource(decoder, AVIF_DECODER_SOURCE_TRACKS));
  }
  check(avifDecoderNextImage(decoder));
  const avifImage* image = decoder->image;
  avifCropRect crop = {0, 0, image->width, image->height};
  if ((image->transformFlags & AVIF_TRANSFORM_CLAP) &&
      !avifCropRectConvertCleanApertureBox(&crop, &image->clap, image->width, image->height,
                                         image->yuvFormat, &decoder->diag)) return 1;
  unsigned angle = image->transformFlags & AVIF_TRANSFORM_IROT ? image->irot.angle : 0;
  unsigned mirror = image->transformFlags & AVIF_TRANSFORM_IMIR ? image->imir.axis + 1 : 0;
  unsigned width = angle % 2 ? crop.height : crop.width;
  unsigned height = angle % 2 ? crop.width : crop.height;
  avifRGBImage rgb;
  avifRGBImageSetDefaults(&rgb, image);
  rgb.format = AVIF_RGB_FORMAT_RGBA;
  rgb.avoidLibYUV = AVIF_TRUE;
  rgb.chromaUpsampling = AVIF_CHROMA_UPSAMPLING_BILINEAR;
  check(avifRGBImageAllocatePixels(&rgb));
  check(avifImageYUVToRGB(image, &rgb));
  // Forward-map source pixels, independently of the application's inverse row
  // gather. Only this small-fixture oracle allocates an oriented output plane.
  size_t output_size = (size_t) width * height * 8;
  uint8_t* oriented = calloc(1, output_size);
  if (!oriented) return 1;
  for (unsigned y = 0; y < crop.height; ++y) {
    for (unsigned x = 0; x < crop.width; ++x) {
      unsigned dx = x, dy = y;
      if (angle == 1) { dx = y; dy = crop.width - 1 - x; }
      if (angle == 2) { dx = crop.width - 1 - x; dy = crop.height - 1 - y; }
      if (angle == 3) { dx = crop.height - 1 - y; dy = x; }
      if (mirror == 1) dy = height - 1 - dy;
      if (mirror == 2) dx = width - 1 - dx;
      const uint8_t* row = rgb.pixels + (y + crop.y) * rgb.rowBytes;
      for (unsigned c = 0; c < 4; ++c) {
        unsigned value = rgb.depth == 8 ? row[(x + crop.x) * 4 + c] : ((const uint16_t*)row)[(x + crop.x) * 4 + c];
        unsigned max = (1u << rgb.depth) - 1;
        value = (value * 65535u + max / 2) / max;
        size_t at = ((size_t) dy * width + dx) * 8 + c * 2;
        oriented[at] = value & 255; oriented[at + 1] = value >> 8;
      }
    }
  }
  FILE* file = output(argv[2], ".rgba16");
  if (fwrite(oriented, 1, output_size, file) != output_size || fclose(file)) return 1;
  free(oriented);
  file = output(argv[2], ".json");
  fprintf(file, "{\"reference\":\"libavif 1.3.0 / AOM / bilinear\",\"width\":%u,\"height\":%u,"
    "\"depth\":%u,\"primaries\":%u,\"transfer\":%u,\"matrix\":%u,\"rotation_ccw\":%u,"
    "\"alpha\":%s,\"gain_map\":%s,\"frames\":%d,\"mirror\":%u,\"icc_bytes\":%zu,\"exif_bytes\":%zu}\n", width, height, image->depth,
    image->colorPrimaries, image->transferCharacteristics, image->matrixCoefficients,
    angle, image->alphaPlane ? "true" : "false", image->gainMap ? "true" : "false", decoder->imageCount,
    mirror, image->icc.size, image->exif.size);
  if (fclose(file)) return 1;
  avifRGBImageFreePixels(&rgb);
  avifDecoderDestroy(decoder);
  return 0;
}
