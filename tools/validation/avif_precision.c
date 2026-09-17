// Independent AV1/AOM encoder fixtures for the native dav1d reader.
// Compile against the installed libavif's matching public avif.h (1.3.0).
#include "avif.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(avifResult result) {
  if (result != AVIF_RESULT_OK) { fprintf(stderr, "%s\n", avifResultToString(result)); exit(1); }
}
static void write_file(const char* directory, const char* name, const void* bytes, size_t length) {
  char path[4096];
  if (snprintf(path, sizeof(path), "%s/%s", directory, name) >= (int) sizeof(path)) exit(1);
  FILE* file = fopen(path, "wb");
  if (!file || fwrite(bytes, 1, length, file) != length || fclose(file)) exit(1);
}
static uint16_t sample(unsigned x, unsigned y, unsigned c, unsigned depth) {
  const unsigned factors[] = {37, 17, 7, 61};
  const unsigned offsets[] = {19, 301, 151, 251};
  return (((y * 64 + x) * factors[c] + offsets[c]) & ((1u << depth) - 1));
}
int main(int argc, char** argv) {
  if (argc != 2 || strcmp(avifVersion(), "1.3.0")) {
    fprintf(stderr, "Usage: avif-precision OUTPUT_DIRECTORY (requires libavif 1.3.0)\n"); return 1;
  }
  if (!avifCodecName(AVIF_CODEC_CHOICE_AOM, AVIF_CODEC_FLAG_CAN_ENCODE)) return 1;
  for (unsigned depth = 10; depth <= 12; depth += 2) {
    for (unsigned variant = 0; variant < 11; ++variant) {
      unsigned rotated = variant == 1, bitstream_color = variant == 2;
      unsigned geometry = variant >= 3;
      unsigned angle = geometry ? (variant - 3) / 2 : rotated ? 1 : 0;
      unsigned axis = geometry ? (variant - 3) % 2 : 0;
      char suffix[40] = "";
      if (geometry) snprintf(suffix, sizeof(suffix), "-crop-r%u-m%u", angle, axis);
      else if (rotated) strcpy(suffix, "-rotated");
      else if (bitstream_color) strcpy(suffix, "-bitstream");
      avifImage* image = avifImageCreate(64, 32, depth, AVIF_PIXEL_FORMAT_YUV444);
      if (!image) return 1;
      image->colorPrimaries = AVIF_COLOR_PRIMARIES_SMPTE432;
      image->transferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_SRGB;
      image->matrixCoefficients = AVIF_MATRIX_COEFFICIENTS_IDENTITY;
      image->yuvRange = AVIF_RANGE_FULL;
      // EXIF orientation intentionally repeats presentation metadata. The
      // container irot below remains authoritative and must be applied once.
      const uint8_t exif[] = {
        'I','I',42,0,8,0,0,0,4,0,
        18,1,3,0,1,0,0,0,6,0,0,0,       // orientation 6 (not applied twice)
        26,1,5,0,1,0,0,0,62,0,0,0,      // XResolution
        27,1,5,0,1,0,0,0,70,0,0,0,      // YResolution
        40,1,3,0,1,0,0,0,2,0,0,0,       // inches
        0,0,0,0,
        44,1,0,0,1,0,0,0,               // 300/1 DPI
        150,0,0,0,1,0,0,0               // 150/1 DPI; rotation swaps axes
      };
      if (rotated || geometry) check(avifImageSetMetadataExif(image, exif, sizeof(exif)));
      image->transformFlags = geometry ? AVIF_TRANSFORM_CLAP | AVIF_TRANSFORM_IROT | AVIF_TRANSFORM_IMIR
                                      : rotated ? AVIF_TRANSFORM_IROT : AVIF_TRANSFORM_NONE;
      image->irot.angle = angle;
      image->imir.axis = axis;
      avifCropRect crop = {geometry ? 8 : 0, geometry ? 4 : 0, geometry ? 48 : 64, geometry ? 24 : 32};
      avifDiagnostics diagnostics = {{0}};
      if (geometry && !avifCleanApertureBoxFromCropRect(&image->clap, &crop, 64, 32, &diagnostics)) {
        fprintf(stderr, "Invalid reference crop: %s\n", diagnostics.error); return 1;
      }
      avifRGBImage rgb;
      avifRGBImageSetDefaults(&rgb, image);
      rgb.format = AVIF_RGB_FORMAT_RGBA;
      rgb.avoidLibYUV = AVIF_TRUE;
      // libavif 1.3.0 omits irot on the alpha item (upstream's
      // abc_color_irot_alpha_NOirot regression). Use opaque rotated fixtures;
      // valid rotated-alpha coverage uses the corrected upstream sample.
      rgb.ignoreAlpha = rotated || geometry ? AVIF_TRUE : AVIF_FALSE;
      check(avifRGBImageAllocatePixels(&rgb));
      for (unsigned y = 0; y < 32; ++y) {
        uint16_t* row = (uint16_t*) (rgb.pixels + y * rgb.rowBytes);
        for (unsigned x = 0; x < 64; ++x)
          for (unsigned c = 0; c < 4; ++c) row[x * 4 + c] = sample(x, y, c, depth);
      }
      check(avifImageRGBToYUV(image, &rgb));
      avifEncoder* encoder = avifEncoderCreate();
      if (!encoder) return 1;
      encoder->codecChoice = AVIF_CODEC_CHOICE_AOM;
      encoder->maxThreads = 2;
      encoder->speed = AVIF_SPEED_FASTEST;
      encoder->quality = AVIF_QUALITY_LOSSLESS;
      encoder->qualityAlpha = AVIF_QUALITY_LOSSLESS;
      if (bitstream_color) {
        // libavif normally puts these values only in colr. Explicit AOM
        // options create a separate reference carrying color in the AV1 OBU.
        check(avifEncoderSetCodecSpecificOption(encoder, "color:color-primaries", "12"));
        check(avifEncoderSetCodecSpecificOption(encoder, "color:transfer-characteristics", "13"));
        check(avifEncoderSetCodecSpecificOption(encoder, "color:matrix-coefficients", "0"));
      }
      avifRWData output = AVIF_DATA_EMPTY;
      check(avifEncoderWrite(encoder, image, &output));
      char name[80];
      snprintf(name, sizeof(name), "p3-%ubit%s.avif", depth, suffix);
      write_file(argv[1], name, output.data, output.size);
      // Independent expected *oriented*, normalized integer source samples.
      uint8_t expected[64 * 32 * 8];
      unsigned width = angle % 2 ? crop.height : crop.width;
      unsigned height = angle % 2 ? crop.width : crop.height;
      for (unsigned y = 0; y < crop.height; ++y) {
        for (unsigned x = 0; x < crop.width; ++x) {
          unsigned dx = x, dy = y;
          if (angle == 1) { dx = y; dy = crop.width - 1 - x; }
          if (angle == 2) { dx = crop.width - 1 - x; dy = crop.height - 1 - y; }
          if (angle == 3) { dx = crop.height - 1 - y; dy = x; }
          if (geometry && axis == 0) dy = height - 1 - dy;
          if (geometry && axis == 1) dx = width - 1 - dx;
          for (unsigned c = 0; c < 4; ++c) {
            unsigned max = (1u << depth) - 1;
            unsigned value = (rotated || geometry) && c == 3 ? max : sample(x + crop.x, y + crop.y, c, depth);
            value = (value * 65535u + max / 2) / max;
            size_t at = (dy * width + dx) * 8 + c * 2;
            expected[at] = value & 255; expected[at + 1] = value >> 8;
          }
        }
      }
      snprintf(name, sizeof(name), "p3-%ubit%s.rgba16", depth, suffix);
      write_file(argv[1], name, expected, width * height * 8);
      avifRWDataFree(&output);
      avifEncoderDestroy(encoder);
      avifRGBImageFreePixels(&rgb);
      avifImageDestroy(image);
    }
  }
  printf("22 lossless 10/12-bit P3 fixtures (alpha, rotation, bitstream color, crop/mirror) written with libavif %s and AOM\n", avifVersion());
  return 0;
}
