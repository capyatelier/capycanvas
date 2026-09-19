// Independent lossless AV1 fixture encoder; never linked into the application.
#include "avif.h"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void check(avifResult result) {
    if (result != AVIF_RESULT_OK) {
        fprintf(stderr, "%s\n", avifResultToString(result));
        exit(1);
    }
}

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(avifVersion(), "1.3.0")) return 1;
    char versions[256];
    avifCodecVersions(versions);
    printf("libavif %s, codecs %s\n", avifVersion(), versions);
    const unsigned factors[] = {37, 17, 7};
    const unsigned offsets[] = {19, 301, 151};
    for (unsigned depth = 8; depth <= 12; depth += 2) {
        avifImage *image = avifImageCreate(64, 32, depth, AVIF_PIXEL_FORMAT_YUV444);
        if (!image) return 1;
        image->colorPrimaries = AVIF_COLOR_PRIMARIES_SMPTE432;
        image->transferCharacteristics = AVIF_TRANSFER_CHARACTERISTICS_SRGB;
        image->matrixCoefficients = AVIF_MATRIX_COEFFICIENTS_IDENTITY;
        image->yuvRange = AVIF_RANGE_FULL;
        avifRGBImage rgb;
        avifRGBImageSetDefaults(&rgb, image);
        rgb.format = AVIF_RGB_FORMAT_RGB;
        rgb.avoidLibYUV = AVIF_TRUE;
        check(avifRGBImageAllocatePixels(&rgb));
        for (unsigned y = 0; y < 32; ++y) {
            uint8_t *row = rgb.pixels + y * rgb.rowBytes;
            for (unsigned x = 0; x < 64; ++x) {
                for (unsigned c = 0; c < 3; ++c) {
                    unsigned value = (((y * 64 + x) * factors[c] + offsets[c]) & ((1u << depth) - 1));
                    if (depth == 8) row[x * 3 + c] = value;
                    else ((uint16_t *)row)[x * 3 + c] = value;
                }
            }
        }
        check(avifImageRGBToYUV(image, &rgb));
        avifEncoder *encoder = avifEncoderCreate();
        if (!encoder) return 1;
        encoder->codecChoice = AVIF_CODEC_CHOICE_AOM;
        encoder->maxThreads = 2;
        encoder->speed = AVIF_SPEED_FASTEST;
        encoder->quality = AVIF_QUALITY_LOSSLESS;
        avifRWData output = AVIF_DATA_EMPTY;
        check(avifEncoderWrite(encoder, image, &output));
        char path[4096];
        if (snprintf(path, sizeof(path), "%s/p3-%ubit.avif", argv[1], depth) >= (int)sizeof(path)) return 1;
        FILE *file = fopen(path, "wb");
        if (!file || fwrite(output.data, 1, output.size, file) != output.size || fclose(file)) return 1;
        avifRWDataFree(&output);
        avifEncoderDestroy(encoder);
        avifRGBImageFreePixels(&rgb);
        avifImageDestroy(image);
    }
    return 0;
}
