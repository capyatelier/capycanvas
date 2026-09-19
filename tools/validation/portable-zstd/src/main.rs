//! Bidirectional compatibility with independently compiled C Zstd.
use layer_core::{
    Project,
    color::{DocumentColor, RgbSpace, SampleDepth},
    raster::TileBlob,
};
use sha2::{Digest, Sha256};
use std::{
    hint::black_box,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Instant,
};
fn random(seed: &mut u32) -> u32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    *seed
}
fn shuffled(bytes: &[u8], bpp: usize) -> Vec<u8> {
    let mut result = vec![0; bytes.len()];
    let count = bytes.len() / bpp;
    for (i, pixel) in bytes.chunks_exact(bpp).enumerate() {
        for c in 0..bpp {
            result[c * count + i] = pixel[c];
        }
    }
    result
}
// Force a Huffman-only block so the LZ match finder cannot hide metadata or
// length-limiting errors by replacing the test input with long matches.
fn huffman_frame(input: &[u8]) -> Vec<u8> {
    let table = zrip_core::huffman::encode::HuffmanEncodeTable::from_data(input).unwrap();
    let mut literals = table.serialize_weights();
    literals.extend(table.encode_4_streams(input));
    let header = 0x0eu64 | ((input.len() as u64) << 4) | ((literals.len() as u64) << 22);
    let mut block = header.to_le_bytes()[..5].to_vec();
    block.extend(literals);
    block.push(0); // No sequences, hence no sequence mode byte.
    assert!(block.len() <= 128 * 1024);
    // Keep the window at 128 KiB even for small inputs: a deliberately forced
    // near-uniform Huffman block can exceed its uncompressed size.
    let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x80, 0x38];
    frame.extend((input.len() as u32).to_le_bytes());
    frame.extend((((block.len() as u32) << 3) | 5).to_le_bytes()[..3].iter());
    frame.extend(block);
    frame
}
fn huffman_cases() {
    for symbols in 129..=256 {
        for heavy in [0, 100_000] {
            let mut input: Vec<_> = (0..symbols).map(|s| s as u8).cycle().take(4096).collect();
            input.extend(std::iter::repeat_n(255, heavy));
            let frame = huffman_frame(&input);
            assert_eq!(
                zstd::bulk::decompress(&frame, input.len()).unwrap_or_else(|e| panic!(
                    "Huffman symbols={symbols} heavy={heavy} frame={}: {e}",
                    frame.len()
                )),
                input
            );
            assert_eq!(
                zrip_decode::decompress_with_limit(&frame, input.len()).unwrap(),
                input
            );
        }
    }
    // Fibonacci frequencies produce a naive tree deeper than Zstd's 11 bits.
    // Put the symbols at the end of the alphabet to require FSE weights too.
    let (mut a, mut b) = (1, 1);
    let mut input = Vec::new();
    for symbol in 232..=255 {
        input.extend(std::iter::repeat_n(symbol, a));
        (a, b) = (b, a + b);
    }
    let frame = huffman_frame(&input);
    assert_eq!(zstd::bulk::decompress(&frame, input.len()).unwrap(), input);
    assert_eq!(
        zrip_decode::decompress_with_limit(&frame, input.len()).unwrap(),
        input
    );
    println!("257 forced Huffman blocks: full alphabet and length-limited trees decoded by C/Rust");
}
fn archive(path: &Path) {
    let mut file = std::fs::File::open(path).unwrap();
    let mut header = [0u8; 52];
    file.read_exact(&mut header).unwrap();
    assert!(header.starts_with(b"CAPYRASTER\x04\0") || header.starts_with(b"CAPYRASTER\x05\0"));
    let len = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let mut json = vec![0; len as usize];
    file.read_exact(&mut json).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&json).unwrap();
    let mut total = 0;
    for blob in manifest["blobs"].as_array().unwrap() {
        let offset = blob["offset"].as_u64().unwrap();
        let size = blob["size"].as_u64().unwrap();
        let descriptor: layer_core::color::PixelDescriptor =
            serde_json::from_value(blob["descriptor"].clone()).unwrap();
        let expected = descriptor.byte_len([256; 2]).unwrap();
        file.seek(SeekFrom::Start(52 + len + offset)).unwrap();
        let mut encoded = vec![0; size as usize];
        file.read_exact(&mut encoded).unwrap();
        let old = zstd::bulk::decompress(&encoded, expected).unwrap();
        assert_eq!(
            zrip_decode::decompress_with_limit(&encoded, expected).unwrap(),
            old
        );
        for level in [-8, 1] {
            let new = zrip_encode::compress(&old, level).unwrap();
            assert_eq!(zstd::bulk::decompress(&new, expected).unwrap(), old);
        }
        total += 1;
    }
    let project = Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    let mut saved = Vec::new();
    project.write(&mut saved).unwrap();
    let restored = Project::read(saved.as_slice(), Default::default()).unwrap();
    assert_eq!(restored.document.color, project.document.color);
    println!(
        "archive {}: {total} old frames decoded and recompressed in both modes; project reopened",
        path.display()
    );
}
fn main() {
    huffman_cases();
    let mut seed = 0x12345678u32;
    for case in 0..256 {
        let n = match case % 8 {
            0 => 0,
            1 => 1,
            2 => 127,
            3 => 65535,
            4 => 65536,
            5 => 65537,
            6 => 131073,
            _ => 1048576,
        };
        let palette = (case / 8 + 1).min(256);
        let input: Vec<u8> = (0..n)
            .map(|i| match case % 7 {
                0 => random(&mut seed) as u8,
                1 => i as u8,
                2 => ((random(&mut seed) % palette) as u8).wrapping_add(231),
                3 => {
                    if i % 65536 < 32768 {
                        0
                    } else {
                        random(&mut seed) as u8
                    }
                }
                4 => {
                    if i % 2048 < 1024 {
                        i as u8
                    } else {
                        random(&mut seed) as u8
                    }
                }
                5 => ((i % 4) * 53) as u8,
                _ => {
                    if random(&mut seed) % 1000 < 995 {
                        255
                    } else {
                        random(&mut seed) as u8
                    }
                }
            })
            .collect();
        for level in [-8, 1, 4] {
            let encoded = zrip_encode::compress(&input, level).unwrap();
            assert_eq!(
                zstd::bulk::decompress(&encoded, input.len()).unwrap(),
                input,
                "C decode case {case}, level {level}"
            );
            assert_eq!(
                zrip_decode::decompress_with_limit(&encoded, input.len()).unwrap(),
                input
            );
        }
        let encoded =
            zstd::bulk::compress(&input, [-20, 1, 3, 9, 19, 22][(case % 6) as usize]).unwrap();
        assert_eq!(
            zrip_decode::decompress_with_limit(&encoded, input.len()).unwrap(),
            input,
            "old frame {case}"
        );
    }
    println!("256 mixed entropy/boundary cases: C/Rust bidirectional compatibility passed");
    for depth in [
        SampleDepth::U8,
        SampleDepth::U16,
        SampleDepth::F16,
        SampleDepth::F32,
    ] {
        let color = DocumentColor {
            space: RgbSpace::Srgb,
            depth,
        };
        let descriptor = color.paint_descriptor();
        let input: Vec<u8> = (0..65536u32)
            .flat_map(|i| {
                (0..4).flat_map(move |c| {
                    let value = if c == 3 {
                        1.
                    } else {
                        (i.wrapping_mul(c * 73 + 1) % 4096) as f32 / 1024.
                    };
                    match depth {
                        SampleDepth::U8 => vec![(value * 63.) as u8],
                        SampleDepth::U16 => ((value * 16383.) as u16).to_le_bytes().to_vec(),
                        SampleDepth::F16 => layer_core::color::f16::from_f32(value)
                            .to_le_bytes()
                            .to_vec(),
                        SampleDepth::F32 => value.to_le_bytes().to_vec(),
                    }
                })
            })
            .collect();
        let coded = if depth == SampleDepth::U8 {
            input.clone()
        } else {
            shuffled(&input, 4 * depth.bytes())
        };
        if let Some(path) = std::env::var_os("CAPY_ZSTD_FIXTURES") {
            let path = std::path::PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            let name = match depth {
                SampleDepth::U8 => "u8",
                SampleDepth::U16 => "u16",
                SampleDepth::F16 => "f16",
                SampleDepth::F32 => "f32",
            };
            std::fs::write(
                path.join(format!("{name}.zst")),
                zstd::bulk::compress(&coded, 1).unwrap(),
            )
            .unwrap();
        }
        for source in [false, true] {
            let start = Instant::now();
            let mut tile = None;
            for _ in 0..32 {
                tile = Some(black_box(
                    if source {
                        TileBlob::encode_source(descriptor, &input)
                    } else {
                        TileBlob::encode(descriptor, &input)
                    }
                    .unwrap(),
                ));
            }
            let mbps = input.len() as f64 * 32. / start.elapsed().as_secs_f64() / 1e6;
            let tile = tile.unwrap();
            assert_eq!(tile.decode().unwrap(), input);
            assert_eq!(
                zstd::bulk::decompress(&tile.compressed().unwrap(), input.len()).unwrap(),
                coded
            );
            let old = zstd::bulk::compress(&coded, if source { 1 } else { -20 }).unwrap();
            // Match validation, shuffle, and hashing work to isolate the codec
            // change. Use the current Rust SHA implementation in both paths.
            let start = Instant::now();
            for _ in 0..32 {
                descriptor.validate_samples(&input).unwrap();
                let mut hash = Sha256::new();
                hash.update(serde_json::to_vec(&descriptor).unwrap());
                hash.update(&input);
                black_box(hash.finalize());
                let planes =
                    (depth != SampleDepth::U8).then(|| shuffled(&input, 4 * depth.bytes()));
                black_box(
                    zstd::bulk::compress(
                        planes.as_deref().unwrap_or(&input),
                        if source { 1 } else { -20 },
                    )
                    .unwrap(),
                );
            }
            let native_mbps = input.len() as f64 * 32. / start.elapsed().as_secs_f64() / 1e6;
            let old_tile =
                TileBlob::from_compressed(descriptor, tile.digest, Arc::from(old.clone())).unwrap();
            assert_eq!(old_tile.decode().unwrap(), input);
            println!(
                "tile {depth:?} source={source}: old={} new={} encode+shuffle+digest Rust={mbps:.1} C={native_mbps:.1} MB/s",
                old.len(),
                tile.compressed_len()
            );
        }
    }
    for path in std::env::args().skip(1) {
        archive(Path::new(&path));
    }
}
