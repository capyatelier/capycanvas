use super::*;
use crate::{READBACK_TIMEOUT, WgpuRasterizer, layer_tests::page_bytes};
use layer_core::color::RgbSpace;

fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native writeback test"),
        size: wgpu::Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, texture: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * texture.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        texture.size(),
    );
}
fn working_bytes(pixels: &[[f32; 4]]) -> Vec<u8> {
    pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect()
}
fn submit(
    r: &WgpuRasterizer,
    encoder: &NativeTileEncoder,
    status: &NativeEncodeStatus,
    batches: &[NativeTileBatch],
    reset: bool,
) {
    let mut commands = r.device.create_command_encoder(&Default::default());
    if reset {
        status.reset(&mut commands);
    }
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        for batch in batches {
            encoder.encode(&mut pass, batch);
        }
    }
    r.queue.submit([commands.finish()]);
}
fn read_status(
    r: &WgpuRasterizer,
    status: &NativeEncodeStatus,
) -> Result<NativeEncodingStats, GpuRasterError> {
    let read = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native encoding test status"),
        size: STATUS_BYTES,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut commands = r.device.create_command_encoder(&Default::default());
    commands.copy_buffer_to_buffer(status.buffer(), 0, &read, 0, STATUS_BYTES);
    r.queue.submit([commands.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    read.slice(..)
        .map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = read.get_mapped_range(..).unwrap();
    let result = NativeEncodeStatus::decode(&bytes);
    drop(bytes);
    read.unmap();
    result
}
fn format(depth: IntegerDepth) -> wgpu::TextureFormat {
    if depth == IntegerDepth::U8 {
        wgpu::TextureFormat::Rgba8Uint
    } else {
        wgpu::TextureFormat::Rgba16Uint
    }
}
fn code(bytes: &[u8], component: usize, depth: IntegerDepth) -> u32 {
    if depth == IntegerDepth::U8 {
        u32::from(bytes[component])
    } else {
        u32::from(u16::from_le_bytes(
            bytes[component * 2..component * 2 + 2].try_into().unwrap(),
        ))
    }
}
fn reference(
    pixel: [f32; 4],
    depth: IntegerDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
) -> [u32; 4] {
    let max = depth.maximum() as f64;
    let coverage = (f64::from(pixel[3]) * max).round() as u32;
    if coverage == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    out[3] = coverage;
    for c in 0..3 {
        // Reference follows the declared Float32 unassociation, then uses Float64
        // transfer/rounding independently of the GPU table and analytic estimate.
        let limit = if alpha == AlphaAssociation::Straight {
            pixel[3]
        } else {
            1.
        };
        let mut value = pixel[c].clamp(0., limit);
        if alpha == AlphaAssociation::Straight {
            value /= pixel[3];
        }
        out[c] = (space.encode(f64::from(value)).clamp(0., 1.) * max).round() as u32;
    }
    out
}
fn assert_pixels(
    actual: &[u8],
    pixels: &[[f32; 4]],
    depth: IntegerDepth,
    space: RgbSpace,
    alpha: AlphaAssociation,
    region: [u32; 4],
    sentinel: u32,
) {
    let mut errors = [0; 4];
    let mut first = None;
    for (i, (bytes, pixel)) in actual
        .chunks_exact(4 * depth.bytes())
        .zip(pixels)
        .enumerate()
    {
        let x = i as u32 % 256;
        let y = i as u32 / 256;
        let inside = x >= region[0]
            && x < region[0] + region[2]
            && y >= region[1]
            && y < region[1] + region[3];
        let expected = if inside {
            reference(*pixel, depth, space, alpha)
        } else {
            [sentinel; 4]
        };
        for c in 0..4 {
            let got = code(bytes, c, depth);
            errors[c] = errors[c].max(got.abs_diff(expected[c]));
            if got != expected[c] && first.is_none() {
                first = Some((i, c, got, expected[c], *pixel));
            }
        }
    }
    assert_eq!(
        errors, [0; 4],
        "{depth:?} {space:?} {alpha:?} first={first:?}"
    );
}

#[test]
fn native_writeback_codes_boundaries_and_partial_tiles() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let outputs = [
        texture(&r, format(IntegerDepth::U8)),
        texture(&r, format(IntegerDepth::U16)),
    ];
    let mut tables = transfer::Tables::default();
    let mut cases = 0;
    for space in RgbSpace::ALL {
        let transfer = tables.prepare(&r.device, space).unwrap();
        for (depth, encoded) in [IntegerDepth::U8, IntegerDepth::U16]
            .into_iter()
            .zip(&outputs)
        {
            let max = depth.maximum();
            for alpha in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                for mode in 0..10 {
                    let pixels: Vec<[f32; 4]> = (0..65536u32)
                        .map(|i| {
                            let coverage = match mode {
                                0..=4 => {
                                    [1., 1. / max as f32, 2. / max as f32, 17. / max as f32, 0.]
                                        [mode]
                                }
                                5..=7 => {
                                    let boundary = (f64::from(i % max) + 0.5) / f64::from(max);
                                    let rounded = boundary as f32;
                                    [rounded.next_down(), rounded, rounded.next_up()][mode - 5]
                                }
                                8 => 0.25 / max as f32,
                                _ => 1.,
                            };
                            let mut p = [0.; 4];
                            p[3] = coverage;
                            for c in 0..3 {
                                let n = i.wrapping_mul([1, 101, 237][c]) & max;
                                let linear = if mode == 9 {
                                    let boundary =
                                        space.decode((f64::from(n) + 0.5) / f64::from(max)) as f32;
                                    [boundary.next_down(), boundary, boundary.next_up()][c]
                                } else {
                                    space.decode(f64::from(n) / f64::from(max)) as f32
                                };
                                p[c] = if alpha == AlphaAssociation::Straight {
                                    linear * coverage
                                } else {
                                    linear
                                };
                            }
                            if i < 8 && mode == 8 {
                                p[0] = f32::MAX;
                            }
                            p
                        })
                        .collect();
                    let bytes = working_bytes(&pixels);
                    upload(&r, &working, &bytes);
                    upload(&r, &canonical, &bytes);
                    let seed = vec![0x39; 256 * 256 * 4 * depth.bytes()];
                    upload(&r, encoded, &seed);
                    let region = if mode == 4 {
                        [13, 7, 229, 231]
                    } else {
                        [0, 0, 256, 256]
                    };
                    let batch = encoder
                        .prepare(
                            &r.device,
                            &[NativeTileRequest {
                                working: &working,
                                canonical: &canonical,
                                encoded,
                                transfer,
                                depth,
                                alpha,
                                region,
                            }],
                            &status,
                        )
                        .unwrap();
                    submit(&r, &encoder, &status, &[batch], true);
                    read_status(&r, &status).unwrap();
                    assert_pixels(
                        &page_bytes(&r, encoded),
                        &pixels,
                        depth,
                        space,
                        alpha,
                        region,
                        if depth == IntegerDepth::U8 {
                            0x39
                        } else {
                            0x3939
                        },
                    );
                    assert!(
                        page_bytes(&r, &working) == bytes,
                        "writeback modified working source"
                    );
                    let canonical_bytes = page_bytes(&r, &canonical);
                    for (i, encoded) in canonical_bytes.chunks_exact(16).enumerate() {
                        let x = i as u32 % 256;
                        let y = i as u32 / 256;
                        if x < region[0]
                            || x >= region[0] + region[2]
                            || y < region[1]
                            || y >= region[1] + region[3]
                        {
                            assert!(
                                encoded == &bytes[i * 16..i * 16 + 16],
                                "canonical changed outside region at {i}"
                            );
                            continue;
                        }
                        let codes = reference(pixels[i], depth, space, alpha);
                        let coverage = codes[3] as f32 / max as f32;
                        for c in 0..4 {
                            let actual =
                                f32::from_le_bytes(encoded[c * 4..c * 4 + 4].try_into().unwrap());
                            let expected = if c == 3 {
                                coverage
                            } else {
                                let decoded =
                                    space.decode(f64::from(codes[c]) / f64::from(max)) as f32;
                                if alpha == AlphaAssociation::Straight {
                                    decoded * coverage
                                } else {
                                    decoded
                                }
                            };
                            assert!(
                                (actual - expected).abs()
                                    <= 0.00000024 * expected.abs().max(0.0000001),
                                "canonical {depth:?} {space:?} {alpha:?} pixel={i} component={c} actual={actual} expected={expected}"
                            );
                        }
                    }
                    cases += 1;
                }
            }
        }
    }
    eprintln!(
        "NATIVE_WRITEBACK cases={cases} pixels={} max_code_error=0",
        cases * 65536
    );
}

#[test]
fn native_writeback_batch_validation_cancellation_and_failure_status() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let transfer = NativeTransfer::new(&r.device, RgbSpace::Srgb).unwrap();
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let outputs: Vec<_> = (0..16)
        .map(|i| {
            texture(
                &r,
                format(if i % 2 == 0 {
                    IntegerDepth::U8
                } else {
                    IntegerDepth::U16
                }),
            )
        })
        .collect();
    let make_requests = || {
        outputs
            .iter()
            .enumerate()
            .map(|(i, encoded)| NativeTileRequest {
                working: &working,
                canonical: &canonical,
                encoded,
                transfer: &transfer,
                depth: if i % 2 == 0 {
                    IntegerDepth::U8
                } else {
                    IntegerDepth::U16
                },
                alpha: if i % 3 == 0 {
                    AlphaAssociation::PremultipliedLinear
                } else {
                    AlphaAssociation::Straight
                },
                region: [i as u32, 5, 239, 247],
            })
            .collect::<Vec<_>>()
    };
    let pixels = vec![[f32::MAX, -f32::MAX, 0.125, 1. / 65535.]; 65536];
    upload(&r, &working, &working_bytes(&pixels));
    for output in &outputs {
        upload(
            &r,
            output,
            &vec![0x39; 65536 * output.format().block_copy_size(None).unwrap() as usize],
        );
    }
    let requests = make_requests();
    let batch = encoder.prepare(&r.device, &requests, &status).unwrap();
    assert_eq!(
        batch.parameter_bytes(),
        16 * u64::from(
            32u32.next_multiple_of(r.device.limits().min_uniform_buffer_offset_alignment)
        )
    );
    let empty = encoder.prepare(&r.device, &[], &status).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.parameter_bytes(), 0);
    let mut commands = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = commands.begin_compute_pass(&Default::default());
        encoder.encode(&mut pass, &batch);
    }
    drop(commands);
    for output in &outputs {
        assert!(page_bytes(&r, output).iter().all(|&v| v == 0x39));
    }
    submit(&r, &encoder, &status, &[empty, batch], true);
    assert_eq!(
        read_status(&r, &status).unwrap().clipped_pixels,
        8 * 239 * 247
    );
    for request in &requests {
        assert_pixels(
            &page_bytes(&r, request.encoded),
            &pixels,
            request.depth,
            RgbSpace::Srgb,
            request.alpha,
            request.region,
            if request.depth == IntegerDepth::U8 {
                0x39
            } else {
                0x3939
            },
        );
    }
    let extra = make_requests().into_iter().next().unwrap();
    let mut invalid = make_requests();
    invalid.push(extra);
    assert!(encoder.prepare(&r.device, &invalid, &status).is_err());
    let wrong = texture(&r, wgpu::TextureFormat::Rgba16Float);
    for mode in 0..6 {
        let mut invalid = make_requests();
        match mode {
            0 => invalid[15].region = [1, 0, u32::MAX, 1],
            1 => invalid[15].region = [0, 256, 1, 1],
            2 => invalid[15].working = &wrong,
            3 => invalid[15].depth = IntegerDepth::U8,
            4 => invalid[15].alpha = AlphaAssociation::None,
            _ => invalid[15].canonical = &working,
        }
        assert!(encoder.prepare(&r.device, &invalid, &status).is_err());
    }
    for mode in 0..5 {
        let mut descriptor = wgpu::TextureDescriptor {
            label: Some("invalid writeback target"),
            size: working.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        };
        match mode {
            0 => descriptor.size.width = 255,
            1 => descriptor.size.depth_or_array_layers = 2,
            2 => descriptor.mip_level_count = 2,
            3 => descriptor.usage = wgpu::TextureUsages::COPY_SRC,
            _ => descriptor.usage = wgpu::TextureUsages::TEXTURE_BINDING,
        }
        let bad_texture = r.device.create_texture(&descriptor);
        let mut invalid = make_requests();
        if mode == 4 {
            invalid[15].canonical = &bad_texture;
        } else {
            invalid[15].working = &bad_texture;
        }
        assert!(encoder.prepare(&r.device, &invalid, &status).is_err());
    }
    let mut zero = make_requests();
    zero[0].region = [256, 256, 0, 0];
    let batch = encoder.prepare(&r.device, &zero[..1], &status).unwrap();
    let before = page_bytes(&r, &outputs[0]);
    submit(&r, &encoder, &status, &[batch], true);
    assert_eq!(read_status(&r, &status).unwrap().clipped_pixels, 0);
    assert!(page_bytes(&r, &outputs[0]) == before);

    // A later batch's error must reject the whole publication even when earlier
    // batches produced valid bytes. Resetting is a new publication, not a batch.
    for bad in [
        [f32::NAN, 0., 0., 1.],
        [0., f32::INFINITY, 0., 1.],
        [0., 0., f32::NEG_INFINITY, 1.],
        [0., 0., 0., -0.1],
        [0., 0., 0., 1.1],
        [0., 0., 0., -f32::from_bits(1)],
    ] {
        let mut one = make_requests();
        one[0].region = [0, 0, 1, 1];
        let good = encoder.prepare(&r.device, &one[..1], &status).unwrap();
        upload(
            &r,
            &working,
            &working_bytes(&vec![[0.2, 0.3, 0.4, 1.]; 65536]),
        );
        submit(&r, &encoder, &status, &[good], true);
        read_status(&r, &status).unwrap();
        upload(&r, &working, &working_bytes(&vec![bad; 65536]));
        let later = encoder.prepare(&r.device, &one[..1], &status).unwrap();
        submit(&r, &encoder, &status, &[later], false);
        assert!(read_status(&r, &status).is_err());
        upload(
            &r,
            &working,
            &working_bytes(&vec![[0.2, 0.3, 0.4, 1.]; 65536]),
        );
        let retry = encoder.prepare(&r.device, &one[..1], &status).unwrap();
        submit(&r, &encoder, &status, &[retry], true);
        read_status(&r, &status).unwrap();
    }
    assert!(NativeEncodeStatus::decode(&[0; 7]).is_err());
}

#[test]
fn canonical_native_tiles_are_stable_across_64_publications() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeTileEncoder::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = [
        texture(&r, wgpu::TextureFormat::Rgba32Float),
        texture(&r, wgpu::TextureFormat::Rgba32Float),
    ];
    let mut tables = transfer::Tables::default();
    for space in RgbSpace::ALL {
        let transfer = tables.prepare(&r.device, space).unwrap();
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let max = depth.maximum();
            let encoded = texture(&r, format(depth));
            let pixels: Vec<_> = (0..65536u32)
                .map(|i| {
                    let alpha = [0, 1, 2, 17, 257.min(max), max / 2, max][i as usize % 7] as f32
                        / max as f32;
                    let mut p = [0.; 4];
                    p[3] = alpha;
                    for c in 0..3 {
                        p[c] = space.decode(
                            f64::from(i.wrapping_mul([1, 101, 237][c]) & max) / f64::from(max),
                        ) as f32
                            * alpha;
                    }
                    p
                })
                .collect();
            upload(&r, &working[0], &working_bytes(&pixels));
            let batches: Vec<_> = (0..2)
                .map(|i| {
                    encoder
                        .prepare(
                            &r.device,
                            &[NativeTileRequest {
                                working: &working[i],
                                canonical: &working[1 - i],
                                encoded: &encoded,
                                transfer,
                                depth,
                                alpha: AlphaAssociation::Straight,
                                region: [0, 0, 256, 256],
                            }],
                            &status,
                        )
                        .unwrap()
                })
                .collect();
            let mut commands = r.device.create_command_encoder(&Default::default());
            status.reset(&mut commands);
            {
                let mut pass = commands.begin_compute_pass(&Default::default());
                for cycle in 0..64 {
                    encoder.encode(&mut pass, &batches[cycle % 2]);
                }
            }
            r.queue.submit([commands.finish()]);
            assert_eq!(read_status(&r, &status).unwrap().clipped_pixels, 0);
            let actual = page_bytes(&r, &encoded);
            let mut errors = [0; 4];
            for (i, pixel) in actual.chunks_exact(4 * depth.bytes()).enumerate() {
                let alpha = [0, 1, 2, 17, 257.min(max), max / 2, max][i % 7];
                for c in 0..4 {
                    let expected = if c == 3 {
                        alpha
                    } else if alpha == 0 {
                        0
                    } else {
                        (i as u32).wrapping_mul([1, 101, 237][c]) & max
                    };
                    errors[c] = errors[c].max(code(pixel, c, depth).abs_diff(expected));
                }
            }
            assert_eq!(
                errors, [0; 4],
                "{space:?} {depth:?} drift after 64 publications"
            );
        }
    }
}

#[test]
#[ignore = "hardware native writeback preparation and GPU completion benchmark"]
fn native_writeback_workloads() {
    use std::time::Instant;
    let r = WgpuRasterizer::new_headless().unwrap();
    let cold = Instant::now();
    let encoder = NativeTileEncoder::new(&r.device);
    eprintln!(
        "NATIVE_WRITEBACK_PREPARE pipelines_ms={:.4}",
        cold.elapsed().as_secs_f64() * 1000.
    );
    let status = NativeEncodeStatus::new(&r.device);
    let working: Vec<_> = (0..16)
        .map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float))
        .collect();
    let canonical: Vec<_> = (0..16)
        .map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float))
        .collect();
    let mut tables = transfer::Tables::default();
    for space in [RgbSpace::Srgb, RgbSpace::AdobeRgb, RgbSpace::ProPhoto] {
        let cold = Instant::now();
        let transfer = tables.prepare(&r.device, space).unwrap();
        eprintln!(
            "NATIVE_WRITEBACK_PREPARE space={space:?} table_ms={:.4} table_bytes={}",
            cold.elapsed().as_secs_f64() * 1000.,
            transfer.storage_bytes()
        );
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let encoded: Vec<_> = (0..16).map(|_| texture(&r, format(depth))).collect();
            for clipped in [false, true] {
                let pixels: Vec<_> = (0..65536u32)
                    .map(|i| {
                        let alpha = [1., 0.5, 257. / 65535., 1. / 65535.][i as usize % 4];
                        let mut value = [0.; 4];
                        value[3] = alpha;
                        for c in 0..3 {
                            let code = i.wrapping_mul([1, 101, 237][c]) & 65535;
                            value[c] = space.decode(f64::from(code) / 65535.) as f32 * alpha;
                        }
                        if clipped {
                            value[0] = 2. * alpha;
                        }
                        value
                    })
                    .collect();
                let bytes = working_bytes(&pixels);
                for tile in &working {
                    upload(&r, tile, &bytes);
                }
                r.queue.submit([]);
                r.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(READBACK_TIMEOUT),
                    })
                    .unwrap();
                for count in [1, 16] {
                    for side in [32, 256] {
                        let requests: Vec<_> = (0..count)
                            .map(|i| NativeTileRequest {
                                working: &working[i],
                                canonical: &canonical[i],
                                encoded: &encoded[i],
                                transfer,
                                depth,
                                alpha: AlphaAssociation::Straight,
                                region: [0, 0, side, side],
                            })
                            .collect();
                        for reuse in [false, true] {
                            let mut batch = encoder.prepare(&r.device, &requests, &status).unwrap();
                            let mut samples = Vec::new();
                            for iteration in 0..120 {
                                let start = Instant::now();
                                if !reuse {
                                    batch = encoder.prepare(&r.device, &requests, &status).unwrap();
                                }
                                let mut commands =
                                    r.device.create_command_encoder(&Default::default());
                                status.reset(&mut commands);
                                {
                                    let mut pass = commands.begin_compute_pass(&Default::default());
                                    encoder.encode(&mut pass, &batch);
                                }
                                r.queue.submit([commands.finish()]);
                                let cpu = start.elapsed().as_secs_f64() * 1000.;
                                r.device
                                    .poll(wgpu::PollType::Wait {
                                        submission_index: None,
                                        timeout: Some(READBACK_TIMEOUT),
                                    })
                                    .unwrap();
                                let complete = start.elapsed().as_secs_f64() * 1000.;
                                if iteration == 0 {
                                    eprintln!(
                                        "NATIVE_WRITEBACK_COLD space={space:?} depth={depth:?} clipped={clipped} tiles={count} side={side} reuse={reuse} cpu_ms={cpu:.4} complete_ms={complete:.4}"
                                    );
                                }
                                if iteration >= 20 {
                                    samples.push([cpu, complete]);
                                }
                            }
                            let mut percentiles = [[0.; 2]; 2];
                            for axis in 0..2 {
                                samples.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
                                percentiles[axis] = [samples[94][axis], samples[98][axis]];
                            }
                            let expected_clipped = if clipped {
                                count as u32
                                    * side
                                    * side
                                    * if depth == IntegerDepth::U8 { 3 } else { 4 }
                                    / 4
                            } else {
                                0
                            };
                            assert_eq!(
                                read_status(&r, &status).unwrap().clipped_pixels,
                                expected_clipped
                            );
                            eprintln!(
                                "NATIVE_WRITEBACK_BENCH space={space:?} depth={depth:?} clipped={clipped} tiles={count} side={side} reuse={reuse} samples=100 cpu_p95_p99={:.4?} complete_p95_p99={:.4?} parameter_bytes={} working_pool_bytes={} encoded_pool_bytes={} canonical_pool_bytes={} table_bytes={}",
                                percentiles[0],
                                percentiles[1],
                                batch.parameter_bytes(),
                                16 * 65536 * 16,
                                16 * 65536 * 4 * depth.bytes(),
                                16 * 65536 * 16,
                                transfer.storage_bytes()
                            );
                        }
                    }
                }
            }
        }
    }
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|l| l.starts_with("VmHWM:") || l.starts_with("VmRSS:"))
        {
            eprintln!("{line}");
        }
    }
}

#[test]
fn native_restore_writeback_capture_round_trip_preserves_committed_codes() {
    use crate::raster::TileCapture;
    use layer_core::raster::RasterTile;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let encoded8 = texture(&r, wgpu::TextureFormat::Rgba8Uint);
    let encoded16 = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    for space in RgbSpace::ALL {
        let transfer = r.prepare_native_transfer(space).unwrap();
        assert_eq!(transfer, r.prepare_native_transfer(space).unwrap());
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let maximum = depth.maximum();
            for association in [
                AlphaAssociation::Straight,
                AlphaAssociation::PremultipliedLinear,
            ] {
                let encoded = if depth == IntegerDepth::U8 {
                    &encoded8
                } else {
                    &encoded16
                };
                let pixels: Vec<_> = (0..65536u32)
                    .map(|i| {
                        let a = [0, 1, 2, 17, maximum / 2, maximum][i as usize % 6] as f32
                            / maximum as f32;
                        let rgb: [f32; 3] = std::array::from_fn(|c| {
                            space.decode(
                                f64::from(i.wrapping_mul([1, 101, 237][c]) & maximum)
                                    / f64::from(maximum),
                            ) as f32
                                * a
                        });
                        [rgb[0], rgb[1], rgb[2], a]
                    })
                    .collect();
                upload(&r, &working, &working_bytes(&pixels));
                let request = NativeTileRequest {
                    working: &working,
                    encoded,
                    canonical: &canonical,
                    transfer: &transfer,
                    depth,
                    alpha: association,
                    region: [0, 0, 256, 256],
                };
                let descriptor = request.descriptor();
                let batch = encoder.prepare(&r.device, &[request], &status).unwrap();
                let mut first = None;
                for cycle in 0..4 {
                    submit(&r, &encoder, &status, std::slice::from_ref(&batch), true);
                    let ticket = RasterTile::pending(descriptor);
                    let capture = r
                        .capture_tiles(
                            &[TileCapture {
                                source: crate::raster::CaptureSource::Texture(encoded),
                                tile: ticket.clone(),
                            }],
                            Some(&status),
                        )
                        .unwrap();
                    capture.finish().unwrap();
                    let backing = ticket.wait_backing().unwrap();
                    let bytes = backing.decode().unwrap();
                    if let Some(first) = &first {
                        assert!(
                            first == &bytes,
                            "native code drift {space:?} {depth:?} {association:?} cycle={cycle}"
                        );
                    } else {
                        first = Some(bytes);
                    }
                    r.restore_native_tiles(&[NativeTileRestore {
                        blob: &backing,
                        space,
                        destination: space,
                        working: &working,
                    }])
                    .unwrap();
                    let restored = page_bytes(&r, &working);
                    let canonical_bytes = page_bytes(&r, &canonical);
                    let mut max_error = 0f32;
                    for (a, b) in restored
                        .chunks_exact(4)
                        .zip(canonical_bytes.chunks_exact(4))
                    {
                        let a = f32::from_le_bytes(a.try_into().unwrap());
                        let b = f32::from_le_bytes(b.try_into().unwrap());
                        max_error = max_error.max((a - b).abs());
                    }
                    assert!(max_error <= 0.00000024, "canonical mismatch: {max_error}");
                }
            }
        }
    }
}

#[test]
fn native_restore_preflight_and_late_corruption_preserve_live_pixels_and_retry() {
    use layer_core::raster::TileBlob;
    use std::sync::Arc;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let working = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let other = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let wrong = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let sentinel = working_bytes(&vec![[0.12, 0.23, 0.34, 0.45]; 65536]);
    upload(&r, &working, &sentinel);
    let descriptor = PixelDescriptor {
        encoding: TransferEncoding::Profile,
        ..PixelDescriptor::SRGB8_STRAIGHT
    };
    let good = Arc::new(TileBlob::encode(descriptor, &[173; 256 * 256 * 4]).unwrap());
    let invalid =
        Arc::new(TileBlob::encode(PixelDescriptor::COVERAGE8, &[255; 256 * 256]).unwrap());
    let request = |blob, working| NativeTileRestore {
        blob,
        space: RgbSpace::Srgb,
        destination: RgbSpace::Srgb,
        working,
    };
    assert!(
        r.restore_native_tiles(&[request(&good, &working), request(&good, &working)])
            .is_err()
    );
    assert!(
        r.restore_native_tiles(&[request(&good, &working), request(&good, &wrong)])
            .is_err()
    );
    assert!(
        r.restore_native_tiles(&[request(&good, &working), request(&invalid, &other)])
            .is_err()
    );
    assert!(page_bytes(&r, &working) == sentinel);
    for (descriptor, space) in [
        (PixelDescriptor::SRGB8_PAINT, RgbSpace::DisplayP3),
        (
            PixelDescriptor {
                encoding: TransferEncoding::Linear,
                ..descriptor
            },
            RgbSpace::Srgb,
        ),
        (
            PixelDescriptor {
                alpha: AlphaAssociation::None,
                ..descriptor
            },
            RgbSpace::Srgb,
        ),
        (
            PixelDescriptor {
                bits_per_channel: 32,
                ..descriptor
            },
            RgbSpace::Srgb,
        ),
    ] {
        let mut invalid =
            TileBlob::encode(PixelDescriptor::SRGB8_STRAIGHT, &[173; 256 * 256 * 4]).unwrap();
        invalid.descriptor = descriptor;
        let invalid = Arc::new(invalid);
        assert!(
            r.restore_native_tiles(&[NativeTileRestore {
                blob: &invalid,
                space,
                destination: space,
                working: &working
            }])
            .is_err()
        );
    }
    assert!(page_bytes(&r, &working) == sentinel);
    let many: Vec<_> = (0..17).map(|_| request(&good, &working)).collect();
    assert!(r.restore_native_tiles(&many).is_err());
    let mut corrupt = TileBlob::encode(descriptor, &[17; 256 * 256 * 4]).unwrap();
    corrupt.digest[0] ^= 1;
    let corrupt = Arc::new(corrupt);
    assert!(
        r.restore_native_tiles(&[request(&good, &working), request(&corrupt, &other)])
            .is_err()
    );
    // Both are private candidates; no live revision was replaced. Abandoned
    // decode reservations must not supply stale pixels when the work retries.
    r.restore_native_tiles(&[request(&good, &working)]).unwrap();
    let actual = page_bytes(&r, &working);
    let a = 173f32 / 255.;
    let expected = RgbSpace::Srgb.decode(173. / 255.) as f32 * a;
    for pixel in actual.chunks_exact(16) {
        for c in 0..3 {
            assert!(
                (f32::from_le_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap()) - expected).abs()
                    < 0.0000002
            );
        }
    }
    r.restore_native_tiles(&[]).unwrap();
}

#[test]
#[ignore = "hardware native raster restore with shared decode residency"]
fn native_restore_workloads() {
    use layer_core::raster::TileBlob;
    use std::sync::Arc;
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let working: Vec<_> = (0..16)
        .map(|_| texture(&r, wgpu::TextureFormat::Rgba32Float))
        .collect();
    for space in [RgbSpace::Srgb, RgbSpace::AdobeRgb, RgbSpace::ProPhoto] {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let descriptor = PixelDescriptor {
                bits_per_channel: depth.bits(),
                encoding: TransferEncoding::Profile,
                ..PixelDescriptor::SRGB8_STRAIGHT
            };
            let blobs: Vec<_> = (0..20u32)
                .map(|tile| {
                    let bytes: Vec<_> = (0..65536u32)
                        .flat_map(|i| {
                            [
                                i.wrapping_mul(8191) + tile * 31,
                                i.wrapping_mul(17) + tile * 16381,
                                if (i / 173 + tile) % 2 == 0 {
                                    depth.maximum()
                                } else {
                                    0
                                },
                                depth.maximum(),
                            ]
                        })
                        .flat_map(|v| (v as u16).to_le_bytes().into_iter().take(depth.bytes()))
                        .collect();
                    Arc::new(TileBlob::encode(descriptor, &bytes).unwrap())
                })
                .collect();
            for count in [1, 16] {
                for misses in [false, true] {
                    let mut times = Vec::new();
                    for frame in 0..120 {
                        let requests: Vec<_> = (0..count)
                            .map(|i| NativeTileRestore {
                                blob: &blobs[if misses { (frame * count + i) % 20 } else { i }],
                                space,
                                destination: space,
                                working: &working[i],
                            })
                            .collect();
                        let start = std::time::Instant::now();
                        r.restore_native_tiles(&requests).unwrap();
                        let cpu = start.elapsed().as_secs_f64() * 1000.;
                        r.device
                            .poll(wgpu::PollType::Wait {
                                submission_index: None,
                                timeout: Some(READBACK_TIMEOUT),
                            })
                            .unwrap();
                        let complete = start.elapsed().as_secs_f64() * 1000.;
                        if frame == 0 {
                            println!(
                                "NATIVE_RESTORE cold space={space:?} depth={depth:?} tiles={count} misses={misses} cpu_ms={cpu:.4} complete_ms={complete:.4}"
                            );
                        }
                        if frame >= 20 {
                            times.push([cpu, complete]);
                        }
                    }
                    for axis in 0..2 {
                        times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
                        println!(
                            "NATIVE_RESTORE warm space={space:?} depth={depth:?} tiles={count} misses={misses} kind={} p95_ms={:.4} p99_ms={:.4}",
                            ["cpu", "complete"][axis],
                            times[94][axis],
                            times[98][axis]
                        );
                    }
                    println!(
                        "NATIVE_RESTORE storage space={space:?} depth={depth:?} tiles={count} misses={misses} upload_peak={}",
                        r.metrics.source_upload_peak_bytes
                    );
                }
            }
        }
    }
}
