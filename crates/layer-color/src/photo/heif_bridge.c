// Narrow, versioned bridge to the bundled libheif/libavif APIs. Rust owns input bytes,
// admission, profile policy and tiled row publication; this library owns only
// libheif contexts, handles and decoded planes. Built by tools/build/photo-codecs.py.
#include <libheif/heif.h>
#include <libheif/heif_properties.h>
#include <libheif/heif_sequences.h>
#include <avif/avif.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
  uint32_t width, height, bits, storage_bpp, alpha, premultiplied, images, quarter_turns;
  uint32_t nclx, primaries, transfer, matrix, full_range;
  float chromaticities[8];
  uint64_t icc_bytes, exif_bytes;
  // libavif leaves display geometry unapplied. Rust gathers oriented rows into
  // source tiles without allocating another full-frame pixel buffer.
  uint32_t plane_width, plane_height, crop_x, crop_y, crop_width, crop_height;
  uint32_t plane_turns, mirror, first_frame, gain_map;
} CapyPhotoInfo;

typedef struct {
  heif_context* context;
  heif_image_handle* handle;
  heif_image* image;
  heif_item_id exif;
  CapyPhotoInfo info;
  avifDecoder* avif;
  avifRGBImage rgb;
  avifIO io;
  const uint8_t* encoded;
  size_t encoded_length;
  int (*cancel)(void*);
  void* cancel_data;
  uint32_t dimension, profile_limit;
} CapyPhoto;

uint32_t capy_photo_abi(void) { return 3; }
size_t capy_photo_info_size(void) { return sizeof(CapyPhotoInfo); }
const char* capy_photo_version(void) { return heif_get_version(); }
const char* capy_photo_avif_version(void) { return avifVersion(); }
int capy_photo_decoder(int format) {
  return format == 4 ? avifCodecName(AVIF_CODEC_CHOICE_DAV1D, AVIF_CODEC_FLAG_CAN_DECODE) != NULL
                     : heif_have_decoder_for_format(format);
}

static int fail(char* message, const char* reason) {
  snprintf(message, 512, "%s", reason ? reason : "HEIF decoding failed");
  return 0;
}
static int check(heif_error error, char* message) {
  return error.code == heif_error_Ok ? 1 : fail(message, error.message);
}

void capy_photo_close(CapyPhoto* photo) {
  if (!photo) return;
  if (photo->image) heif_image_release(photo->image);
  if (photo->handle) heif_image_handle_release(photo->handle);
  if (photo->context) heif_context_free(photo->context);
  avifRGBImageFreePixels(&photo->rgb);
  if (photo->avif) avifDecoderDestroy(photo->avif);
  free(photo);
  heif_deinit();
}

static void color_info(CapyPhotoInfo* info, const heif_color_profile_nclx* profile) {
  info->nclx = profile != NULL;
  if (!profile) return;
  info->primaries = profile->color_primaries;
  info->transfer = profile->transfer_characteristics;
  info->matrix = profile->matrix_coefficients;
  info->full_range = profile->full_range_flag;
  const float xy[] = {profile->color_primary_red_x, profile->color_primary_red_y,
    profile->color_primary_green_x, profile->color_primary_green_y,
    profile->color_primary_blue_x, profile->color_primary_blue_y,
    profile->color_primary_white_x, profile->color_primary_white_y};
  memcpy(info->chromaticities, xy, sizeof(xy));
}

static int cancelled(CapyPhoto* photo) {
  return photo->cancel && photo->cancel(photo->cancel_data);
}
static int avif_check(CapyPhoto* photo, avifResult result, char* message) {
  if (cancelled(photo)) return fail(message, "Image read cancelled");
  if (result == AVIF_RESULT_OK) return 1;
  snprintf(message, 512, "AVIF: %s (%.350s)", avifResultToString(result), photo->avif->diag.error);
  return 0;
}
static avifResult avif_read(avifIO* io, uint32_t flags, uint64_t offset, size_t size, avifROData* output) {
  CapyPhoto* photo = io->data;
  if (flags || cancelled(photo) || offset > photo->encoded_length) return AVIF_RESULT_IO_ERROR;
  size_t remaining = photo->encoded_length - (size_t) offset;
  output->data = photo->encoded + offset;
  output->size = size < remaining ? size : remaining;
  return AVIF_RESULT_OK;
}
static int avif_info(CapyPhoto* photo, char* message) {
  const avifImage* image = photo->avif->image;
  CapyPhotoInfo* info = &photo->info;
  if (!image->width || !image->height || image->width > photo->dimension || image->height > photo->dimension)
    return fail(message, "AVIF image exceeds the dimension limit");
  if (image->depth != 8 && image->depth != 10 && image->depth != 12)
    return fail(message, "Unsupported AVIF sample precision");
  if (image->icc.size > photo->profile_limit ||
      (image->exif.size && image->exif.size > photo->profile_limit - 4))
    return fail(message, "AVIF ICC or EXIF exceeds the metadata limit");
  avifCropRect crop = {0, 0, image->width, image->height};
  if ((image->transformFlags & AVIF_TRANSFORM_CLAP) &&
      !avifCropRectFromCleanApertureBox(&crop, &image->clap, image->width, image->height, &photo->avif->diag))
    return fail(message, "Invalid AVIF clean aperture");
  info->plane_width = image->width; info->plane_height = image->height;
  info->crop_x = crop.x; info->crop_y = crop.y;
  info->crop_width = crop.width; info->crop_height = crop.height;
  info->plane_turns = image->transformFlags & AVIF_TRANSFORM_IROT ? image->irot.angle : 0;
  info->quarter_turns = info->plane_turns;
  info->mirror = image->transformFlags & AVIF_TRANSFORM_IMIR ? image->imir.axis + 1 : 0;
  if (info->plane_turns > 3 || info->mirror > 2) return fail(message, "Invalid AVIF orientation");
  info->width = info->plane_turns % 2 ? crop.height : crop.width;
  info->height = info->plane_turns % 2 ? crop.width : crop.height;
  info->bits = image->depth;
  info->alpha = photo->avif->alphaPresent;
  info->gain_map = image->gainMap != NULL;
  info->premultiplied = 0; // RGB conversion explicitly requests straight alpha.
  info->images = photo->avif->imageCount;
  info->nclx = 1;
  info->primaries = image->colorPrimaries;
  info->transfer = image->transferCharacteristics;
  info->matrix = image->matrixCoefficients;
  info->full_range = image->yuvRange == AVIF_RANGE_FULL;
  // libavif's unknown-primary fallback is BT.709. Leave unknown coordinates
  // invalid so shared profile policy cannot silently assign that color space.
  memset(info->chromaticities, 0, sizeof(info->chromaticities));
  switch (info->primaries) {
    case 1: case 4: case 5: case 6: case 7: case 8: case 9: case 10: case 11: case 12: case 22:
      avifColorPrimariesGetValues(image->colorPrimaries, info->chromaticities); break;
  }
  info->icc_bytes = image->icc.size;
  info->exif_bytes = image->exif.size ? image->exif.size + 4 : 0;
  return 1;
}
static uint32_t read_be32(const uint8_t* bytes) {
  return (uint32_t) bytes[0] << 24 | (uint32_t) bytes[1] << 16 | (uint32_t) bytes[2] << 8 | bytes[3];
}
// libavif 1.4.2 skips tkhd matrices and display scaling. Until these have a
// qualified mapping, reject them explicitly instead of returning wrong pixels.
// Walk only the fixed moov/trak/tkhd hierarchy, with bounded offsets and depth.
static int avif_track_geometry(CapyPhoto* photo, const uint8_t* bytes, size_t length, unsigned depth, char* message) {
  for (size_t at = 0; at < length;) {
    if (cancelled(photo)) return fail(message, "Image read cancelled");
    if (length - at < 8) return fail(message, "Truncated AVIF sequence box");
    const uint8_t* box = bytes + at;
    uint64_t size = read_be32(box);
    size_t header = 8;
    if (size == 1) {
      if (length - at < 16) return fail(message, "Truncated AVIF sequence box");
      size = (uint64_t) read_be32(box + 8) << 32 | read_be32(box + 12);
      header = 16;
    } else if (!size) size = length - at;
    if (size < header || size > length - at) return fail(message, "Invalid AVIF sequence box size");
    const uint8_t* payload = box + header;
    size_t payload_size = (size_t) size - header;
    if ((depth == 0 && !memcmp(box + 4, "moov", 4)) || (depth == 1 && !memcmp(box + 4, "trak", 4))) {
      if (!avif_track_geometry(photo, payload, payload_size, depth + 1, message)) return 0;
    } else if (depth == 2 && !memcmp(box + 4, "tkhd", 4)) {
      if (!payload_size || payload[0] > 1) return fail(message, "Invalid AVIF track header");
      size_t offset = payload[0] == 1 ? 52 : 40;
      if (payload_size < offset + 44) return fail(message, "Truncated AVIF track header");
      const uint32_t identity[] = {0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000};
      for (unsigned i = 0; i < 9; ++i) {
        if (read_be32(payload + offset + i * 4) != identity[i])
          return fail(message, "AVIF track transformations require a still-image export");
      }
      uint32_t width = read_be32(payload + offset + 36), height = read_be32(payload + offset + 40);
      if ((width || height) && (width != photo->info.plane_width * 65536u || height != photo->info.plane_height * 65536u))
        return fail(message, "AVIF track display scaling requires a still-image export");
    }
    at += (size_t) size;
  }
  return 1;
}
static int avif_open(CapyPhoto* photo, uint64_t budget, char* message) {
  photo->avif = avifDecoderCreate();
  if (!photo->avif) return fail(message, "AVIF decoder allocation failed");
  avifDecoder* decoder = photo->avif;
  decoder->codecChoice = AVIF_CODEC_CHOICE_DAV1D;
  decoder->maxThreads = 2;
  uint64_t pixels = (uint64_t) photo->dimension * photo->dimension;
  if (pixels > budget / 24) pixels = budget / 24;
  if (!pixels) return fail(message, "AVIF codec budget is too small");
  // libavif rejects a limit above its compiled maximum even for tiny images.
  decoder->imageSizeLimit = pixels > AVIF_DEFAULT_IMAGE_SIZE_LIMIT ? AVIF_DEFAULT_IMAGE_SIZE_LIMIT : (uint32_t) pixels;
  decoder->imageDimensionLimit = photo->dimension;
  decoder->imageCountLimit = 4096;
  decoder->ignoreXMP = AVIF_TRUE;
  // Older still encoders omitted pixi. AV1 configuration/decoded precision is
  // still checked; retain strict crop and alpha geometry checks.
  decoder->strictFlags &= ~AVIF_STRICT_PIXI_REQUIRED;
  decoder->allowProgressive = AVIF_FALSE; // Decode the complete still, not its first refinement.
  decoder->imageContentToDecode = AVIF_IMAGE_CONTENT_COLOR_AND_ALPHA;
  photo->io.read = avif_read;
  photo->io.sizeHint = photo->encoded_length;
  photo->io.persistent = AVIF_TRUE;
  photo->io.data = photo;
  avifDecoderSetIO(decoder, &photo->io); // Borrowed IO; no destroy callback.
  if (!avif_check(photo, avifDecoderParse(decoder), message)) return 0;
  if (decoder->imageSequenceTrackPresent) {
    // A primary item can be an independent poster. Always select actual tracks
    // before describing the imported pixels as the first sequence frame.
    if (!avif_check(photo, avifDecoderSetSource(decoder, AVIF_DECODER_SOURCE_TRACKS), message)) return 0;
    photo->info.first_frame = 1;
  }
  if (!avif_info(photo, message)) return 0;
  return !photo->info.first_frame || avif_track_geometry(photo, photo->encoded, photo->encoded_length, 0, message);
}

// Encoded bytes must remain alive until close. All limits are nonzero; zero
// disables a libheif limit. Do not change process-global security defaults.
int capy_photo_open(const uint8_t* bytes, size_t length, uint64_t budget,
                    uint32_t dimension, uint32_t profile_limit,
                    int (*cancel)(void*), void* cancel_data,
                    CapyPhoto** output, CapyPhotoInfo* info, char* message) {
  *output = NULL;
  if (heif_get_version_number() < 0x01170400 || !budget || !dimension || profile_limit < 4)
    return fail(message, "HEIF requires libheif 1.23.4 or newer and nonzero image limits");
  if (!check(heif_init(NULL), message)) return 0;
  CapyPhoto* photo = calloc(1, sizeof(*photo));
  if (!photo) { heif_deinit(); return fail(message, "HEIF allocation failed"); }
  photo->encoded = bytes; photo->encoded_length = length;
  photo->dimension = dimension; photo->profile_limit = profile_limit;
  photo->cancel = cancel; photo->cancel_data = cancel_data;
  if (cancelled(photo)) { fail(message, "Image read cancelled"); goto error; }
  avifROData input = {bytes, length};
  if (avifPeekCompatibleFileType(&input)) {
    if (!avif_open(photo, budget, message)) goto error;
    goto success;
  }
  photo->context = heif_context_alloc();
  if (!photo->context) { fail(message, "HEIF context allocation failed"); goto error; }
  heif_security_limits* limits = heif_context_get_security_limits(photo->context);
  if (!limits || limits->version < 4) { fail(message, "HEIF memory limits unavailable"); goto error; }
  limits->max_image_size_pixels = (uint64_t) dimension * dimension;
  if (limits->max_image_size_pixels > budget / 8)
    limits->max_image_size_pixels = budget / 8;
  if (!limits->max_image_size_pixels) { fail(message, "HEIF codec budget is too small"); goto error; }
  limits->max_memory_block_size = budget;
  limits->max_total_memory = budget;
  limits->max_color_profile_size = profile_limit;
  // Parsing collections stays bounded even though only the primary image is used.
  if (limits->max_items > 4096) limits->max_items = 4096;
  if (limits->max_sequence_frames > 4096) limits->max_sequence_frames = 4096;
  heif_context_set_max_decoding_threads(photo->context, 0);
  if (!check(heif_context_read_from_memory_without_copy(photo->context, bytes, length, NULL), message)) goto error;
  if (heif_context_number_of_sequence_tracks(photo->context) > 0) {
    fail(message, "HEIF sequences require a still-image export"); goto error;
  }
  if (!check(heif_context_get_primary_image_handle(photo->context, &photo->handle), message)) goto error;
  int width = heif_image_handle_get_width(photo->handle);
  int height = heif_image_handle_get_height(photo->handle);
  if (width <= 0 || height <= 0 || (uint32_t) width > dimension || (uint32_t) height > dimension) {
    fail(message, "HEIF image exceeds the dimension limit"); goto error;
  }
  photo->info.width = width;
  photo->info.height = height;
  photo->info.plane_width = photo->info.crop_width = width;
  photo->info.plane_height = photo->info.crop_height = height;
  int bits = heif_image_handle_get_luma_bits_per_pixel(photo->handle);
  int chroma = heif_image_handle_get_chroma_bits_per_pixel(photo->handle);
  if (chroma > bits) bits = chroma;
  // Unknown bit depth requests 16-bit output to avoid an implicit reduction.
  photo->info.bits = bits < 0 ? 16 : (uint32_t) bits;
  photo->info.alpha = heif_image_handle_has_alpha_channel(photo->handle);
  photo->info.premultiplied = heif_image_handle_is_premultiplied_alpha(photo->handle);
  photo->info.images = heif_context_get_number_of_top_level_images(photo->context);
  photo->info.icc_bytes = heif_image_handle_get_raw_color_profile_size(photo->handle);
  if (photo->info.icc_bytes > profile_limit) { fail(message, "HEIF ICC profile exceeds the limit"); goto error; }
  heif_color_profile_nclx* nclx = NULL;
  heif_error error = heif_image_handle_get_nclx_color_profile(photo->handle, &nclx);
  if (error.code != heif_error_Ok && error.code != heif_error_Color_profile_does_not_exist) {
    check(error, message); goto error;
  }
  color_info(&photo->info, nclx);
  if (nclx) heif_nclx_color_profile_free(nclx);
  heif_item_id primary;
  if (!check(heif_context_get_primary_image_ID(photo->context, &primary), message)) goto error;
  int rotation = heif_item_get_property_transform_rotation_ccw(photo->context, primary, 0);
  photo->info.quarter_turns = rotation < 0 ? 0 : (uint32_t) rotation / 90;
  int exif = heif_image_handle_get_number_of_metadata_blocks(photo->handle, "Exif");
  if (exif > 1) { fail(message, "HEIF image has ambiguous EXIF blocks"); goto error; }
  if (exif == 1) {
    if (heif_image_handle_get_list_of_metadata_block_IDs(photo->handle, "Exif", &photo->exif, 1) != 1) {
      fail(message, "HEIF EXIF block is missing"); goto error;
    }
    photo->info.exif_bytes = heif_image_handle_get_metadata_size(photo->handle, photo->exif);
    if (photo->info.exif_bytes > profile_limit) { fail(message, "HEIF EXIF exceeds the metadata limit"); goto error; }
  }
success:
  if (cancelled(photo)) { fail(message, "Image read cancelled"); goto error; }
  photo->cancel = NULL; photo->cancel_data = NULL;
  *info = photo->info;
  *output = photo;
  return 1;
error:
  capy_photo_close(photo);
  return 0;
}

int capy_photo_metadata(CapyPhoto* photo, int exif, uint8_t* bytes, size_t size, char* message) {
  uint64_t expected = exif ? photo->info.exif_bytes : photo->info.icc_bytes;
  if (!expected || expected != size) return fail(message, "Invalid HEIF metadata buffer");
  if (photo->avif) {
    const avifImage* image = photo->avif->image;
    if (exif) {
      size_t offset;
      if (!avif_check(photo, avifGetExifTiffHeaderOffset(image->exif.data, image->exif.size, &offset), message)) return 0;
      // Match the HEIF ExifDataBlock contract consumed by shared Rust metadata.
      for (unsigned i = 0; i < 4; ++i) bytes[i] = (uint8_t) (offset >> ((3 - i) * 8));
      memcpy(bytes + 4, image->exif.data, image->exif.size);
    } else memcpy(bytes, image->icc.data, size);
    return 1;
  }
  return check(exif ? heif_image_handle_get_metadata(photo->handle, photo->exif, bytes)
                    : heif_image_handle_get_raw_color_profile(photo->handle, bytes), message);
}

// The cancellation callback and its data remain alive through this call; the
// callback may run on a decoder thread. Returned pixels remain valid until close.
static int decode(CapyPhoto* photo, uint64_t budget, int (*cancel)(void*), void* cancel_data,
                      CapyPhotoInfo* info, const uint8_t** pixels, size_t* stride,
                      char* message) {
  if (photo->image || photo->rgb.pixels) return fail(message, "HEIF/AVIF image was already decoded");
  if (!budget) return fail(message, "HEIF codec budget is too small");
  photo->cancel = cancel; photo->cancel_data = cancel_data;
  if (cancelled(photo)) return fail(message, "Image read cancelled");
  if (photo->avif) {
    if ((uint64_t) photo->info.plane_width * photo->info.plane_height > budget / 24)
      return fail(message, "AVIF decoded image exceeds the codec budget");
    if (!avif_check(photo, avifDecoderNextImage(photo->avif), message) || !avif_info(photo, message)) return 0;
    if (photo->info.transfer == 16 || photo->info.transfer == 18)
      return fail(message, "HDR HEIF/AVIF needs an explicit SDR conversion before import");
    if ((uint64_t) photo->info.plane_width * photo->info.plane_height > budget / 24)
      return fail(message, "AVIF decoded image exceeds the codec budget");
    avifRGBImageSetDefaults(&photo->rgb, photo->avif->image);
    photo->rgb.format = AVIF_RGB_FORMAT_RGBA;
    photo->rgb.alphaPremultiplied = AVIF_FALSE;
    photo->rgb.avoidLibYUV = AVIF_TRUE;
    photo->rgb.chromaUpsampling = AVIF_CHROMA_UPSAMPLING_BILINEAR;
    photo->rgb.maxThreads = 2;
    if (!avif_check(photo, avifRGBImageAllocatePixels(&photo->rgb), message) ||
        !avif_check(photo, avifImageYUVToRGB(photo->avif->image, &photo->rgb), message)) return 0;
    photo->info.storage_bpp = photo->rgb.depth == 8 ? 4 : 8;
#if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
    if (photo->rgb.depth > 8) {
      for (uint32_t y = 0; y < photo->rgb.height; ++y) {
        uint8_t* row = photo->rgb.pixels + (size_t) y * photo->rgb.rowBytes;
        for (uint32_t x = 0; x < photo->rgb.width * 8; x += 2) {
          uint8_t byte = row[x]; row[x] = row[x + 1]; row[x + 1] = byte;
        }
      }
    }
#endif
    *pixels = photo->rgb.pixels; *stride = photo->rgb.rowBytes; *info = photo->info;
    photo->cancel = NULL; photo->cancel_data = NULL;
    return 1;
  }
  heif_security_limits* limits = heif_context_get_security_limits(photo->context);
  limits->max_total_memory = budget;
  limits->max_memory_block_size = budget;
  heif_decoding_options* options = heif_decoding_options_alloc();
  if (!options) return fail(message, "HEIF options allocation failed");
  if (options->version < 10) {
    heif_decoding_options_free(options);
    return fail(message, "HEIF source color preservation is unavailable");
  }
  options->strict_decoding = 1;
  options->convert_hdr_to_8bit = 0;
  options->output_image_nclx_profile_passthrough = 1;
  options->cancel_decoding = cancel;
  options->progress_user_data = cancel_data;
  options->num_codec_threads = 2;
  options->color_conversion_options.preferred_chroma_upsampling_algorithm = heif_chroma_upsampling_bilinear;
  options->color_conversion_options.only_use_preferred_chroma_algorithm = 1;
  heif_chroma chroma = photo->info.bits <= 8 ? heif_chroma_interleaved_RGBA : heif_chroma_interleaved_RRGGBBAA_LE;
  heif_error error = heif_decode_image(photo->handle, &photo->image, heif_colorspace_RGB, chroma, options);
  heif_decoding_options_free(options);
  if (!check(error, message)) return 0;
  int width = heif_image_get_width(photo->image, heif_channel_interleaved);
  int height = heif_image_get_height(photo->image, heif_channel_interleaved);
  int bits = heif_image_get_bits_per_pixel_range(photo->image, heif_channel_interleaved);
  if (width <= 0 || height <= 0 || bits < 1 || bits > 16)
    return fail(message, "Invalid HEIF decoded plane dimensions or precision");
  photo->info.width = width;
  photo->info.height = height;
  photo->info.plane_width = photo->info.crop_width = width;
  photo->info.plane_height = photo->info.crop_height = height;
  photo->info.bits = bits;
  heif_chroma layout = heif_image_get_chroma_format(photo->image);
  if (layout != heif_chroma_interleaved_RGBA && layout != heif_chroma_interleaved_RRGGBBAA_LE)
    return fail(message, "Unexpected HEIF decoded pixel layout");
  photo->info.storage_bpp = layout == heif_chroma_interleaved_RGBA ? 4 : 8;
  photo->info.premultiplied = heif_image_is_premultiplied_alpha(photo->image);
  heif_color_profile_nclx* nclx = NULL;
  error = heif_image_get_nclx_color_profile(photo->image, &nclx);
  if (error.code != heif_error_Ok && error.code != heif_error_Color_profile_does_not_exist)
    return check(error, message);
  // RGB output may omit an NCLX object even in passthrough mode. The item
  // profile remains authoritative in that case; do not mark tagged pixels as
  // untagged merely because the converted plane omits redundant metadata.
  if (nclx) {
    color_info(&photo->info, nclx);
    heif_nclx_color_profile_free(nclx);
  }
  *pixels = heif_image_get_plane_readonly2(photo->image, heif_channel_interleaved, stride);
  if (!*pixels) return fail(message, "HEIF decoded plane is missing");
  *info = photo->info;
  if (cancelled(photo)) return fail(message, "Image read cancelled");
  photo->cancel = NULL; photo->cancel_data = NULL;
  return 1;
}

int capy_photo_decode(CapyPhoto* photo, uint64_t budget, int (*cancel)(void*), void* cancel_data,
                      CapyPhotoInfo* info, const uint8_t** pixels, size_t* stride, char* message) {
  int result = decode(photo, budget, cancel, cancel_data, info, pixels, stride, message);
  // Never retain a Rust callback or its borrowed token beyond the FFI call,
  // including cancellation and decoder errors.
  photo->cancel = NULL; photo->cancel_data = NULL;
  return result;
}
