/* The libjpeg setjmp boundary stays entirely in C. Rust I/O callbacks return
 * errors normally; only then may a source/destination manager raise an error.
 * One scanline and fixed I/O buffers are used, never a full decoded image. */
#include <stdio.h>
#include <stdint.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
#include <setjmp.h>
#include <limits.h>
#include <jpeglib.h>
#include <jerror.h>

_Static_assert(JMSG_LENGTH_MAX <= 512, "Rust JPEG error buffer is too small");

typedef ptrdiff_t (*read_fn)(void *, unsigned char *, size_t);
typedef int (*write_fn)(void *, const unsigned char *, size_t);

struct error {
  struct jpeg_error_mgr base;
  jmp_buf jump;
  char *message;
};
static void fail(j_common_ptr jpeg) {
  struct error *error = (struct error *)jpeg->err;
  (*jpeg->err->format_message)(jpeg, error->message);
  longjmp(error->jump, 1);
}
static void emit(j_common_ptr jpeg, int level) {
  if (level < 0) fail(jpeg); /* Never silently return repaired/corrupt pixels. */
}
static void init_error(struct error *error, char *message) {
  jpeg_std_error(&error->base);
  error->base.error_exit = fail;
  error->base.emit_message = emit;
  error->message = message;
}

struct source {
  struct jpeg_source_mgr base;
  read_fn read;
  void *opaque;
  unsigned char buffer[65536];
};
static void source_init(j_decompress_ptr jpeg) { (void)jpeg; }
static void source_term(j_decompress_ptr jpeg) { (void)jpeg; }
static boolean source_fill(j_decompress_ptr jpeg) {
  struct source *source = (struct source *)jpeg->src;
  ptrdiff_t count = source->read(source->opaque, source->buffer, sizeof(source->buffer));
  if (count < 0) ERREXIT(jpeg, JERR_FILE_READ);
  if (count == 0) ERREXIT(jpeg, JERR_INPUT_EOF);
  if ((size_t)count > sizeof(source->buffer)) ERREXIT(jpeg, JERR_FILE_READ);
  source->base.next_input_byte = source->buffer;
  source->base.bytes_in_buffer = (size_t)count;
  return TRUE;
}
static void source_skip(j_decompress_ptr jpeg, long count) {
  if (count <= 0) return;
  while ((size_t)count > jpeg->src->bytes_in_buffer) {
    count -= (long)jpeg->src->bytes_in_buffer;
    source_fill(jpeg);
  }
  jpeg->src->next_input_byte += count;
  jpeg->src->bytes_in_buffer -= (size_t)count;
}
struct decoder {
  struct jpeg_decompress_struct jpeg;
  struct error error;
  struct source source;
};
struct info {
  uint32_t width, height, channels, precision, adobe, transform, multiple_scans;
};
void capy_jpeg_decoder_free(struct decoder *decoder) {
  if (!decoder) return;
  jpeg_destroy_decompress(&decoder->jpeg);
  free(decoder);
}
struct decoder *capy_jpeg_decoder_new(read_fn read, void *opaque, struct info *info,
                                     char *message) {
  struct decoder *volatile decoder = calloc(1, sizeof(*decoder));
  if (!decoder) { strcpy(message, "JPEG context allocation failed"); return NULL; }
  init_error(&decoder->error, message);
  decoder->jpeg.err = &decoder->error.base;
  if (setjmp(decoder->error.jump)) { capy_jpeg_decoder_free(decoder); return NULL; }
  jpeg_create_decompress(&decoder->jpeg);
  decoder->source.read = read;
  decoder->source.opaque = opaque;
  decoder->source.base.init_source = source_init;
  decoder->source.base.fill_input_buffer = source_fill;
  decoder->source.base.skip_input_data = source_skip;
  decoder->source.base.resync_to_restart = jpeg_resync_to_restart;
  decoder->source.base.term_source = source_term;
  decoder->jpeg.src = &decoder->source.base;
  jpeg_read_header(&decoder->jpeg, TRUE);
  info->width = decoder->jpeg.image_width;
  info->height = decoder->jpeg.image_height;
  info->precision = decoder->jpeg.data_precision;
  info->adobe = decoder->jpeg.saw_Adobe_marker;
  info->transform = decoder->jpeg.Adobe_transform;
  info->multiple_scans = jpeg_has_multiple_scans(&decoder->jpeg);
  switch (decoder->jpeg.jpeg_color_space) {
  case JCS_GRAYSCALE: info->channels = 1; decoder->jpeg.out_color_space = JCS_GRAYSCALE; break;
  case JCS_RGB: case JCS_YCbCr: info->channels = 3; decoder->jpeg.out_color_space = JCS_RGB; break;
  case JCS_CMYK: case JCS_YCCK: info->channels = 4; decoder->jpeg.out_color_space = JCS_CMYK; break;
  default: info->channels = 0; break;
  }
  return decoder;
}
int capy_jpeg_decoder_start(struct decoder *decoder, size_t budget, char *message) {
  decoder->error.message = message;
  if (setjmp(decoder->error.jump)) return 0;
  /* max_memory_to_use bounds virtual coefficient arrays, not every small codec
   * allocation. Reserve conservative scanline/controller/I/O scratch separately.
   * Preflight padded coefficient dimensions before asking libjpeg to allocate. */
  uint64_t h = 1, v = 1;
  for (int i = 0; i < decoder->jpeg.num_components; i++) {
    jpeg_component_info *c = &decoder->jpeg.comp_info[i];
    if (c->h_samp_factor < 1 || c->h_samp_factor > 4 || c->v_samp_factor < 1 || c->v_samp_factor > 4) {
      strcpy(message, "Unsupported JPEG sampling factors"); return 0;
    }
    if ((uint64_t)c->h_samp_factor > h) h = c->h_samp_factor;
    if ((uint64_t)c->v_samp_factor > v) v = c->v_samp_factor;
  }
  uint64_t columns = (decoder->jpeg.image_width + h * 8 - 1) / (h * 8);
  uint64_t rows = (decoder->jpeg.image_height + v * 8 - 1) / (v * 8);
  uint64_t scratch = 2 * 1024 * 1024 + (uint64_t)decoder->jpeg.image_width * 4 * 8 * v * 4;
  uint64_t coefficients = 0;
  if (jpeg_has_multiple_scans(&decoder->jpeg)) {
    for (int i = 0; i < decoder->jpeg.num_components; i++) {
      jpeg_component_info *c = &decoder->jpeg.comp_info[i];
      coefficients += columns * rows * c->h_samp_factor * c->v_samp_factor * sizeof(JBLOCK);
    }
  }
  if (scratch > budget || coefficients > budget - scratch || budget - scratch > LONG_MAX) {
    strcpy(message, "JPEG scan/coefficient buffers exceed the codec memory budget"); return 0;
  }
  decoder->jpeg.mem->max_memory_to_use = (long)(budget - scratch);
  decoder->jpeg.dct_method = JDCT_ISLOW;
  return jpeg_start_decompress(&decoder->jpeg) ? 1 : 0;
}
int capy_jpeg_decoder_row(struct decoder *decoder, unsigned char *row, size_t size, char *message) {
  decoder->error.message = message;
  if (setjmp(decoder->error.jump)) return 0;
  if (size != (size_t)decoder->jpeg.output_width * decoder->jpeg.output_components) {
    strcpy(message, "Invalid JPEG output row"); return 0;
  }
  JSAMPROW rows[1] = { row };
  return jpeg_read_scanlines(&decoder->jpeg, rows, 1) == 1;
}
int capy_jpeg_decoder_finish(struct decoder *decoder, char *message) {
  decoder->error.message = message;
  if (setjmp(decoder->error.jump)) return 0;
  return jpeg_finish_decompress(&decoder->jpeg) ? 1 : 0;
}

struct destination {
  struct jpeg_destination_mgr base;
  write_fn write;
  void *opaque;
  unsigned char buffer[65536];
};
static void destination_init(j_compress_ptr jpeg) {
  struct destination *destination = (struct destination *)jpeg->dest;
  destination->base.next_output_byte = destination->buffer;
  destination->base.free_in_buffer = sizeof(destination->buffer);
}
static boolean destination_empty(j_compress_ptr jpeg) {
  struct destination *destination = (struct destination *)jpeg->dest;
  if (!destination->write(destination->opaque, destination->buffer, sizeof(destination->buffer)))
    ERREXIT(jpeg, JERR_FILE_WRITE);
  destination_init(jpeg);
  return TRUE;
}
static void destination_term(j_compress_ptr jpeg) {
  struct destination *destination = (struct destination *)jpeg->dest;
  size_t size = sizeof(destination->buffer) - destination->base.free_in_buffer;
  if (size && !destination->write(destination->opaque, destination->buffer, size))
    ERREXIT(jpeg, JERR_FILE_WRITE);
}
struct encoder {
  struct jpeg_compress_struct jpeg;
  struct error error;
  struct destination destination;
};
void capy_jpeg_encoder_free(struct encoder *encoder) {
  if (!encoder) return;
  jpeg_destroy_compress(&encoder->jpeg);
  free(encoder);
}
struct encoder *capy_jpeg_encoder_new(write_fn write, void *opaque, uint32_t width,
                                     uint32_t height, int channels, int quality, char *message) {
  struct encoder *volatile encoder = calloc(1, sizeof(*encoder));
  if (!encoder) { strcpy(message, "JPEG context allocation failed"); return NULL; }
  init_error(&encoder->error, message);
  encoder->jpeg.err = &encoder->error.base;
  if (setjmp(encoder->error.jump)) { capy_jpeg_encoder_free(encoder); return NULL; }
  jpeg_create_compress(&encoder->jpeg);
  encoder->destination.write = write;
  encoder->destination.opaque = opaque;
  encoder->destination.base.init_destination = destination_init;
  encoder->destination.base.empty_output_buffer = destination_empty;
  encoder->destination.base.term_destination = destination_term;
  encoder->jpeg.dest = &encoder->destination.base;
  encoder->jpeg.image_width = width;
  encoder->jpeg.image_height = height;
  encoder->jpeg.input_components = channels;
  encoder->jpeg.in_color_space = channels == 1 ? JCS_GRAYSCALE : channels == 4 ? JCS_CMYK : JCS_RGB;
  jpeg_set_defaults(&encoder->jpeg);
  jpeg_set_quality(&encoder->jpeg, quality, TRUE);
  /* Full chroma resolution and baseline sequential coding. No full coefficient
   * buffer for optimization/progressive output, and no quality-dependent switch. */
  for (int i = 0; i < encoder->jpeg.num_components; i++) {
    encoder->jpeg.comp_info[i].h_samp_factor = 1;
    encoder->jpeg.comp_info[i].v_samp_factor = 1;
  }
  encoder->jpeg.optimize_coding = FALSE;
  encoder->jpeg.dct_method = JDCT_ISLOW;
  jpeg_start_compress(&encoder->jpeg, TRUE);
  return encoder;
}
int capy_jpeg_encoder_marker(struct encoder *encoder, const unsigned char *data, size_t size, char *message) {
  encoder->error.message = message;
  if (setjmp(encoder->error.jump)) return 0;
  if (size > 65533) { strcpy(message, "JPEG ICC chunk is too large"); return 0; }
  jpeg_write_marker(&encoder->jpeg, JPEG_APP0 + 2, data, (unsigned int)size);
  return 1;
}
int capy_jpeg_encoder_row(struct encoder *encoder, const unsigned char *row, size_t size, char *message) {
  encoder->error.message = message;
  if (setjmp(encoder->error.jump)) return 0;
  if (size != (size_t)encoder->jpeg.image_width * encoder->jpeg.input_components) {
    strcpy(message, "Invalid JPEG input row"); return 0;
  }
  JSAMPROW rows[1] = { (JSAMPROW)row };
  return jpeg_write_scanlines(&encoder->jpeg, rows, 1) == 1;
}
int capy_jpeg_encoder_finish(struct encoder *encoder, char *message) {
  encoder->error.message = message;
  if (setjmp(encoder->error.jump)) return 0;
  jpeg_finish_compress(&encoder->jpeg);
  return 1;
}
