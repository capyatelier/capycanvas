use super::*;
use crate::{
    WgpuRasterizer,
    layer_tests::page_bytes,
    raster::{CaptureSource, TileCapture},
};
use layer_core::raster::RasterTile;
mod bench;

fn texture(r: &WgpuRasterizer) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scalar native test"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, values: &[f32]) {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    r.queue.write_texture(
        t.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(1024),
            rows_per_image: None,
        },
        t.size(),
    );
}
fn buffer(r: &WgpuRasterizer, depth: IntegerDepth) -> wgpu::Buffer {
    r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("packed native scalar fixture"),
        size: 65536 * depth.bytes() as u64,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
fn submit(
    r: &WgpuRasterizer,
    encoder: &NativeScalarEncoder,
    status: &NativeEncodeStatus,
    batch: &NativeScalarBatch,
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
    status.reset(&mut commands);
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        encoder.encode(&mut pass, batch);
    }
    r.queue.submit([commands.finish()]);
}
fn capture(
    r: &WgpuRasterizer,
    buffer: &wgpu::Buffer,
    descriptor: PixelDescriptor,
    status: &NativeEncodeStatus,
) -> Vec<u8> {
    let tile = RasterTile::default();
    r.capture_tiles(
        &[TileCapture {
            descriptor,
            source: CaptureSource::Packed(buffer),
            tile: tile.clone(),
        }],
        Some(status),
    )
    .unwrap()
    .finish()
    .unwrap();
    tile.wait_backing().unwrap().decode().unwrap()
}
fn code(bytes: &[u8], i: usize, depth: IntegerDepth) -> u32 {
    if depth == IntegerDepth::U8 {
        bytes[i] as u32
    } else {
        u16::from_le_bytes(bytes[i * 2..i * 2 + 2].try_into().unwrap()) as u32
    }
}

#[test]
fn scalar_writeback_preserves_every_code_half_neighbors_and_partial_packed_words() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r);
    let canonical = texture(&r);
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        let encoded = buffer(&r, depth);
        let maximum = depth.maximum();
        for pattern in 0..4 {
            let values: Vec<f32> = (0..65536u32)
                .map(|i| {
                    let c = i % (maximum + 1);
                    if pattern == 0 {
                        c as f32 / maximum as f32
                    } else {
                        let half = ((c.min(maximum - 1) as f64 + 0.5) / maximum as f64) as f32;
                        match pattern {
                            1 => half.next_down(),
                            2 => half,
                            _ => half.next_up(),
                        }
                    }
                })
                .collect();
            upload(&r, &working, &values);
            for region in [
                [0, 0, 256, 256],
                [1, 7, 253, 243],
                [3, 255, 1, 1],
                [255, 0, 1, 256],
                [17, 31, 0, 71],
            ] {
                r.queue
                    .write_buffer(&encoded, 0, &vec![0xa5; encoded.size() as usize]);
                upload(&r, &canonical, &vec![-7.; 65536]);
                let request = NativeScalarRequest {
                    working: &working,
                    encoded: &encoded,
                    canonical: &canonical,
                    depth,
                    region,
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &status).unwrap();
                submit(&r, &encoder, &status, &batch);
                let bytes = capture(&r, &encoded, descriptor, &status);
                let canonical_bytes = page_bytes(&r, &canonical);
                for (i, value) in values.iter().enumerate() {
                    let x = i as u32 % 256;
                    let y = i as u32 / 256;
                    let inside = x >= region[0]
                        && x < region[0] + region[2]
                        && y >= region[1]
                        && y < region[1] + region[3];
                    let expected = if inside {
                        (f64::from(*value) * maximum as f64).round() as u32
                    } else if depth == IntegerDepth::U8 {
                        0xa5
                    } else {
                        0xa5a5
                    };
                    assert_eq!(
                        code(&bytes, i, depth),
                        expected,
                        "depth {depth:?}, pattern {pattern}, region {region:?}, pixel {i}"
                    );
                    let actual =
                        f32::from_le_bytes(canonical_bytes[i * 4..i * 4 + 4].try_into().unwrap());
                    let expected = if inside {
                        expected as f64 / maximum as f64
                    } else {
                        -7.
                    };
                    assert!(
                        (actual as f64 - expected).abs() < 6e-8,
                        "canonical pixel {i}: {actual} vs {expected}"
                    );
                }
            }
        }
    }
}

#[test]
fn scalar_native_capture_rejects_entire_mixed_publication_on_invalid_coverage() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r);
    let canonical = [texture(&r), texture(&r)];
    let buffers = [buffer(&r, IntegerDepth::U8), buffer(&r, IntegerDepth::U16)];
    let requests: Vec<_> = [IntegerDepth::U8, IntegerDepth::U16]
        .into_iter()
        .enumerate()
        .map(|(i, depth)| NativeScalarRequest {
            working: &working,
            encoded: &buffers[i],
            canonical: &canonical[i],
            depth,
            region: [0, 0, 256, 256],
        })
        .collect();
    let batch = encoder.prepare(&r.device, &requests, &status).unwrap();
    for invalid in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -f32::MIN_POSITIVE,
        -1.,
        1f32.next_up(),
    ] {
        let mut values = vec![0.5; 65536];
        values[65535] = invalid;
        upload(&r, &working, &values);
        submit(&r, &encoder, &status, &batch);
        let copies: Vec<_> = requests
            .iter()
            .map(|r| TileCapture {
                descriptor: r.descriptor(),
                source: CaptureSource::Packed(r.encoded),
                tile: RasterTile::default(),
            })
            .collect();
        let capture = r.capture_tiles(&copies, Some(&status)).unwrap();
        let mut commands = r.device.create_command_encoder(&Default::default());
        status.reset(&mut commands);
        r.queue.submit([commands.finish()]);
        assert!(capture.finish().is_err());
        assert!(
            copies
                .iter()
                .all(|c| matches!(c.tile.try_backing(), Some(Err(_))))
        );
    }
    upload(&r, &working, &vec![-0.; 65536]);
    submit(&r, &encoder, &status, &batch);
    for request in &requests {
        assert!(
            capture(&r, request.encoded, request.descriptor(), &status)
                .iter()
                .all(|b| *b == 0)
        );
    }
    let cancelled = RasterTile::default();
    drop(
        r.capture_tiles(
            &[TileCapture {
                descriptor: requests[0].descriptor(),
                source: CaptureSource::Packed(&buffers[0]),
                tile: cancelled.clone(),
            }],
            Some(&status),
        )
        .unwrap(),
    );
    assert!(matches!(cancelled.try_backing(), Some(Err(_))));
}

#[test]
fn scalar_native_preflights_shapes_aliases_depth_and_capture_descriptor() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r);
    let canonical = texture(&r);
    let encoded = buffer(&r, IntegerDepth::U16);
    let make = || NativeScalarRequest {
        working: &working,
        encoded: &encoded,
        canonical: &canonical,
        depth: IntegerDepth::U16,
        region: [0, 0, 256, 256],
    };
    assert!(encoder.prepare(&r.device, &[], &status).unwrap().is_empty());
    assert!(
        encoder
            .prepare(&r.device, &[make(), make()], &status)
            .is_err()
    );
    assert!(
        encoder
            .prepare(
                &r.device,
                &(0..17).map(|_| make()).collect::<Vec<_>>(),
                &status
            )
            .is_err()
    );
    for region in [[256, 0, 1, 1], [0, 0, u32::MAX, 1], [0, 255, 1, 2]] {
        let mut request = make();
        request.region = region;
        assert!(encoder.prepare(&r.device, &[request], &status).is_err());
    }
    let mut request = make();
    request.canonical = &working;
    assert!(encoder.prepare(&r.device, &[request], &status).is_err());
    let mut request = make();
    request.depth = IntegerDepth::U8;
    assert!(encoder.prepare(&r.device, &[request], &status).is_err());
    let tile = RasterTile::default();
    assert!(
        r.capture_tiles(
            &[TileCapture {
                descriptor: PixelDescriptor::COVERAGE8,
                source: CaptureSource::Packed(&encoded),
                tile: tile.clone()
            }],
            Some(&status)
        )
        .is_err()
    );
    assert!(tile.try_backing().is_none());
}

#[test]
fn scalar_native_restore_capture_cycles_preserve_codes_and_bound_uploads() {
    use layer_core::raster::TileBlob;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working: Vec<_> = (0..16).map(|_| texture(&r)).collect();
    let canonical: Vec<_> = (0..16).map(|_| texture(&r)).collect();
    let depths: Vec<_> = (0..16)
        .map(|i| {
            if i % 2 == 0 {
                IntegerDepth::U8
            } else {
                IntegerDepth::U16
            }
        })
        .collect();
    let buffers: Vec<_> = depths.iter().map(|d| buffer(&r, *d)).collect();
    let requests: Vec<_> = (0..16)
        .map(|i| NativeScalarRequest {
            working: &working[i],
            encoded: &buffers[i],
            canonical: &canonical[i],
            depth: depths[i],
            region: [0, 0, 256, 256],
        })
        .collect();
    let expected: Vec<Vec<u8>> = depths
        .iter()
        .enumerate()
        .map(|(j, d)| {
            (0..65536u32)
                .flat_map(|i| {
                    let value = (i * (j as u32 * 2 + 1)) as u16;
                    if *d == IntegerDepth::U8 {
                        vec![value as u8]
                    } else {
                        value.to_le_bytes().to_vec()
                    }
                })
                .collect()
        })
        .collect();
    let mut blobs: Vec<_> = requests
        .iter()
        .zip(&expected)
        .map(|(r, b)| std::sync::Arc::new(TileBlob::encode(r.descriptor(), b).unwrap()))
        .collect();
    let batch = encoder.prepare(&r.device, &requests, &status).unwrap();
    assert!(batch.parameter_bytes() <= 4096);
    for _cycle in 0..4 {
        for _repeat in 0..5 {
            let restores: Vec<_> = blobs
                .iter()
                .zip(&working)
                .map(|(blob, working)| NativeScalarRestore { blob, working })
                .collect();
            r.restore_native_scalars(&restores).unwrap();
        }
        submit(&r, &encoder, &status, &batch);
        let copies: Vec<_> = requests
            .iter()
            .map(|r| TileCapture {
                descriptor: r.descriptor(),
                source: CaptureSource::Packed(r.encoded),
                tile: RasterTile::default(),
            })
            .collect();
        r.capture_tiles(&copies, Some(&status))
            .unwrap()
            .finish()
            .unwrap();
        blobs = copies
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let blob = c.tile.wait_backing().unwrap();
                let actual = blob.decode().unwrap();
                assert!(actual == expected[i], "native scalar drift at tile {i}");
                blob
            })
            .collect();
    }
    assert!(r.metrics.source_upload_peak_bytes <= 16 * 256 * 256 * 4);
    assert!(r.metrics.source_upload_submissions >= 16);
    // Descriptor and late data failures leave live pages/candidates untouched
    // when no earlier bounded chunk was submitted. A following retry succeeds.
    let before = page_bytes(&r, &working[0]);
    let mut corrupt = TileBlob::encode(requests[1].descriptor(), &expected[1]).unwrap();
    corrupt.digest[0] ^= 1;
    assert!(
        r.restore_native_scalars(&[
            NativeScalarRestore {
                blob: &blobs[1],
                working: &working[0]
            },
            NativeScalarRestore {
                blob: &corrupt,
                working: &working[1]
            },
        ])
        .is_err()
    );
    assert!(page_bytes(&r, &working[0]) == before);
    assert!(
        r.restore_native_scalars(&[
            NativeScalarRestore {
                blob: &blobs[0],
                working: &working[0]
            },
            NativeScalarRestore {
                blob: &blobs[1],
                working: &working[0]
            },
        ])
        .is_err()
    );
    r.restore_native_scalars(&[NativeScalarRestore {
        blob: &blobs[0],
        working: &working[0],
    }])
    .unwrap();
    assert!(page_bytes(&r, &working[0]) == before);
}
