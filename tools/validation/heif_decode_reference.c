// Independent HEIC oracle. Build only for validation against pinned libheif
// 1.23.4 + libde265; this executable is never part of an application package.
// Usage: heif-decode-reference INPUT OUTPUT [yuv]
#include <libheif/heif.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(heif_error error) {
  if (error.code) { fprintf(stderr, "%s\n", error.message); exit(1); }
}
int main(int argc, char** argv) {
  if ((argc != 3 && argc != 4) || strcmp(heif_get_version(), "1.23.4")) return 2;
  int yuv = argc == 4 && !strcmp(argv[3], "yuv");
  check(heif_init(NULL));
  heif_context* context = heif_context_alloc();
  if (!context) return 1;
  heif_context_set_max_decoding_threads(context, 0);
  check(heif_context_read_from_file(context, argv[1], NULL));
  heif_image_handle* handle = NULL;
  check(heif_context_get_primary_image_handle(context, &handle));
  heif_decoding_options* options = heif_decoding_options_alloc();
  if (!options || options->version < 10) return 1;
  options->output_image_nclx_profile_passthrough = 1;
  options->strict_decoding = 1;
  options->decoder_id = "libde265";
  options->num_codec_threads = 1;
  options->color_conversion_options.preferred_chroma_upsampling_algorithm = heif_chroma_upsampling_bilinear;
  options->color_conversion_options.only_use_preferred_chroma_algorithm = 1;
  heif_image* image = NULL;
  check(heif_decode_image(handle, &image, yuv ? heif_colorspace_YCbCr : heif_colorspace_RGB,
                          yuv ? heif_chroma_420 : heif_chroma_interleaved_RRGGBBAA_LE, options));
  FILE* file = fopen(argv[2], "wb");
  if (!file) return 1;
  if (yuv) {
    const heif_channel channels[] = {heif_channel_Y, heif_channel_Cb, heif_channel_Cr};
    for (unsigned c = 0; c < 3; ++c) {
      int stride = 0;
      const uint8_t* data = heif_image_get_plane_readonly(image, channels[c], &stride);
      int width = heif_image_get_width(image, channels[c]);
      int height = heif_image_get_height(image, channels[c]);
      int depth = heif_image_get_bits_per_pixel_range(image, channels[c]);
      if (!data || depth < 8 || depth > 16 || width < 1 || height < 1) return 1;
      for (int y = 0; y < height; ++y) for (int x = 0; x < width; ++x) {
        const uint8_t* p = data + y * stride + x * (depth > 8 ? 2 : 1);
        uint16_t value = p[0];
        if (depth > 8) memcpy(&value, p, 2);
        uint8_t out[2] = {value & 255, value >> 8};
        if (fwrite(out, 1, 2, file) != 2) return 1;
      }
      printf("%s: plane %u %dx%d %d-bit\n", argv[1], c, width, height, depth);
    }
    if (fclose(file)) return 1;
    goto cleanup;
  }
  int stride = 0;
  const uint8_t* data = heif_image_get_plane_readonly(image, heif_channel_interleaved, &stride);
  int width = heif_image_get_width(image, heif_channel_interleaved);
  int height = heif_image_get_height(image, heif_channel_interleaved);
  int depth = heif_image_get_bits_per_pixel_range(image, heif_channel_interleaved);
  if (!data || width < 1 || height < 1 || depth < 8 || depth > 16 || stride < width * 8) return 1;
  unsigned maximum = (1u << depth) - 1;
  for (int y = 0; y < height; ++y) {
    const uint8_t* row = data + y * stride;
    for (int c = 0; c < width * 4; ++c) {
      unsigned sample = row[c*2] | ((unsigned)row[c*2+1] << 8);
      if (sample > maximum) return 1;
      unsigned normalized = (sample * 65535u + maximum / 2) / maximum;
      uint8_t bytes[2] = {normalized & 255, normalized >> 8};
      if (fwrite(bytes, 1, 2, file) != 2) return 1;
    }
  }
  if (fclose(file)) return 1;
  printf("%s: %dx%d, %d-bit, libheif %s / libde265\n", argv[1], width, height, depth, heif_get_version());
cleanup:
  heif_image_release(image);
  heif_decoding_options_free(options);
  heif_image_handle_release(handle);
  heif_context_free(context);
  heif_deinit();
  return 0;
}
