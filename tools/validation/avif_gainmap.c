// Synthetic gain-map interchange oracle. Build with libavif 1.4.2 and AOM.
// Application builds never compile or link this validation program.
#include <avif/avif.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void checked(avifResult result, unsigned line) {
    if (result != AVIF_RESULT_OK) { fprintf(stderr, "Line %u: %s\n", line, avifResultToString(result)); exit(1); }
}
#define check(result) checked((result), __LINE__)
static void save(const char *root, const char *name, const char *ext, const void *data, size_t size) {
    char path[4096];
    if (snprintf(path, sizeof(path), "%s/%s.%s", root, name, ext) >= (int)sizeof(path)) exit(1);
    FILE *f = fopen(path, "wb");
    if (!f || fwrite(data, 1, size, f) != size || fclose(f)) exit(1);
}
static void sample(avifImage *image, unsigned channel, unsigned x, unsigned y, unsigned value) {
    uint8_t *plane = avifImagePlane(image, (avifChannelIndex)channel);
    uint32_t stride = avifImagePlaneRowBytes(image, (avifChannelIndex)channel);
    ((uint16_t *)(plane + y * stride))[x] = (uint16_t)value;
}
int main(int argc, char **argv) {
    if (argc != 2 || strcmp(avifVersion(), "1.4.2")) return 1;
    const char *names[] = {"hdr-rgb", "hdr-small-gray", "hdr-alternate"};
    for (unsigned variant = 0; variant < 3; ++variant) {
        avifImage *base = avifImageCreate(16, 12, 12, AVIF_PIXEL_FORMAT_YUV444);
        if (!base) return 1;
        base->colorPrimaries = AVIF_COLOR_PRIMARIES_SMPTE432;
        base->transferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_SRGB;
        base->matrixCoefficients = AVIF_MATRIX_COEFFICIENTS_IDENTITY;
        base->yuvRange = AVIF_RANGE_FULL;
        check(avifImageAllocatePlanes(base, AVIF_PLANES_ALL));
        for (unsigned y = 0; y < 12; ++y) for (unsigned x = 0; x < 16; ++x) {
            sample(base, AVIF_CHAN_Y, x, y, 800 + y * 71);
            sample(base, AVIF_CHAN_U, x, y, 850 + ((x + y) % 7) * 101);
            sample(base, AVIF_CHAN_V, x, y, 700 + x * 83);
            sample(base, AVIF_CHAN_A, x, y, (x * 257 + y * 61) % 4096);
        }
        avifGainMap *gm = avifGainMapCreate();
        if (!gm) return 1;
        base->gainMap = gm;
        const unsigned small = variant == 1;
        gm->image = avifImageCreate(small ? 8 : 16, small ? 6 : 12, 10,
            small ? AVIF_PIXEL_FORMAT_YUV400 : AVIF_PIXEL_FORMAT_YUV444);
        if (!gm->image) return 1;
        gm->image->matrixCoefficients = small ? AVIF_MATRIX_COEFFICIENTS_BT601 : AVIF_MATRIX_COEFFICIENTS_IDENTITY;
        gm->image->yuvRange = AVIF_RANGE_FULL;
        check(avifImageAllocatePlanes(gm->image, AVIF_PLANES_YUV));
        for (unsigned y = 0; y < gm->image->height; ++y) for (unsigned x = 0; x < gm->image->width; ++x) {
            sample(gm->image, AVIF_CHAN_Y, x, y, 300 + ((x * 19 + y * 13) % 500));
            if (!small) {
                sample(gm->image, AVIF_CHAN_U, x, y, 250 + ((x * 23 + y * 31) % 500));
                sample(gm->image, AVIF_CHAN_V, x, y, 200 + ((x * 37 + y * 17) % 500));
            }
        }
        for (unsigned c = 0; c < 3; ++c) {
            gm->gainMapMin[c] = (avifSignedFraction){0, 1};
            gm->gainMapMax[c] = (avifSignedFraction){(int32_t)c + 2, 1};
            gm->gainMapGamma[c] = (avifUnsignedFraction){c == 1 ? 2 : 1, c == 2 ? 2 : 1};
            gm->baseOffset[c] = (avifSignedFraction){(int32_t)c + 1, 64};
            gm->alternateOffset[c] = (avifSignedFraction){(int32_t)c, 128};
        }
        gm->baseHdrHeadroom = (avifUnsignedFraction){0, 1};
        gm->alternateHdrHeadroom = (avifUnsignedFraction){4, 1};
        gm->useBaseColorSpace = variant != 2;
        gm->altColorPrimaries = variant == 2 ? AVIF_COLOR_PRIMARIES_BT2020 : base->colorPrimaries;
        gm->altTransferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_LINEAR;
        gm->altMatrixCoefficients = AVIF_MATRIX_COEFFICIENTS_IDENTITY;
        gm->altYUVRange = AVIF_RANGE_FULL;
        gm->altDepth = 16;
        gm->altPlaneCount = 4;
        avifEncoder *enc = avifEncoderCreate();
        if (!enc) return 1;
        enc->codecChoice = AVIF_CODEC_CHOICE_AOM;
        enc->maxThreads = 1;
        enc->speed = AVIF_SPEED_FASTEST;
        enc->quality = enc->qualityAlpha = enc->qualityGainMap = AVIF_QUALITY_LOSSLESS;
        avifRWData encoded = AVIF_DATA_EMPTY;
        avifResult encoded_result = avifEncoderWrite(enc, base, &encoded);
        if (encoded_result != AVIF_RESULT_OK) fprintf(stderr, "%s: %s\n", names[variant], enc->diag.error);
        check(encoded_result);
        save(argv[1], names[variant], "avif", encoded.data, encoded.size);
        // Decode the actual container and compressed planes independently before
        // asking libavif to reconstruct linear sRGB half-float reference pixels.
        avifDecoder *dec = avifDecoderCreate();
        if (!dec) return 1;
        dec->codecChoice = AVIF_CODEC_CHOICE_DAV1D;
        dec->imageContentToDecode = AVIF_IMAGE_CONTENT_ALL;
        check(avifDecoderSetIOMemory(dec, encoded.data, encoded.size));
        check(avifDecoderParse(dec));
        check(avifDecoderNextImage(dec));
        avifRGBImage output;
        avifRGBImageSetDefaults(&output, dec->image);
        output.format = AVIF_RGB_FORMAT_RGBA;
        output.depth = 16;
        output.isFloat = AVIF_TRUE;
        output.avoidLibYUV = AVIF_TRUE;
        avifDiagnostics diag = {{0}};
        check(avifImageApplyGainMap(dec->image, dec->image->gainMap, 4,
            AVIF_COLOR_PRIMARIES_BT709, AVIF_TRANSFER_CHARACTERISTICS_LINEAR, &output, NULL, &diag));
        uint8_t reference[16 * 12 * 8];
        for (unsigned y = 0; y < 12; ++y) for (unsigned x = 0; x < 16; ++x) {
            size_t at = y * 16 + x;
            memcpy(reference + at * 8, output.pixels + y * output.rowBytes + x * 8, 8);
        }
        save(argv[1], names[variant], "rgba16f", reference, sizeof(reference));
        avifRGBImageFreePixels(&output);
        avifDecoderDestroy(dec);
        avifRWDataFree(&encoded);
        avifEncoderDestroy(enc);
        avifImageDestroy(base);
    }
    printf("Three lossless AVIF gain-map fixtures and native HDR references written\n");
    return 0;
}
