#!/usr/bin/env python3
"""Generate the ProPhoto RGBA16 sample used by Web photo journeys."""
import argparse
from pathlib import Path
import struct
import zlib


WIDTH, HEIGHT, TRANSPARENT_COLUMNS = 513, 257, 31


def chunk(kind, payload):
    return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind + payload))


def generate(profile):
    if len(profile) < 128 or profile[36:40] != b'acsp' or profile[16:20] != b'RGB ':
        raise ValueError('Expected the RGB prophoto.icc emitted by the layer-color proof_profiles example')
    pixels = bytearray()
    for y in range(HEIGHT):
        pixels.append(0)
        for x in range(WIDTH):
            rgba = (0, 0, 0, 0) if x < TRANSPARENT_COLUMNS else (
                (x - TRANSPARENT_COLUMNS) * 65535 // (WIDTH - TRANSPARENT_COLUMNS - 1),
                y * 65535 // (HEIGHT - 1), (257 * x + 911 * y) & 65535, 65535)
            pixels.extend(struct.pack('>4H', *rgba))
    return b'\x89PNG\r\n\x1a\n' + b''.join([
        chunk(b'IHDR', struct.pack('>2I5B', WIDTH, HEIGHT, 16, 6, 0, 0, 0)),
        chunk(b'iCCP', b'ProPhoto RGB\0\0' + zlib.compress(profile)),
        chunk(b'IDAT', zlib.compress(pixels)), chunk(b'IEND', b'')])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('profile', type=Path, help='prophoto.icc from cargo run --locked -p layer-color --example proof_profiles -- OUTPUT_DIRECTORY')
    parser.add_argument('output', type=Path, help='generated PNG path on scratch storage or in ignored apps/layer-web/pkg')
    args = parser.parse_args()
    encoded = generate(args.profile.read_bytes())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(encoded)
    print(f'{args.output}: {WIDTH}x{HEIGHT} ProPhoto RGBA16; {TRANSPARENT_COLUMNS} transparent black columns; {len(encoded)} bytes')


if __name__ == '__main__':
    main()
