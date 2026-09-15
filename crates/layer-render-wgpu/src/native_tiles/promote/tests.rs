use super::*;
use crate::native_tiles::{
    NativeTileEncoder, NativeTileRequest,
    scalar::{NativeScalarEncoder, NativeScalarRequest},
};
use crate::raster::{CaptureSource, TileCapture};
use crate::{WgpuRasterizer, layer_tests::page_bytes};
use layer_core::{
    color::{AlphaAssociation, IntegerDepth, RgbSpace},
    raster::RasterTile,
};

fn texture(r: &WgpuRasterizer, format: wgpu::TextureFormat) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canonical promotion fixture"),
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
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}
fn upload(r: &WgpuRasterizer, t: &wgpu::Texture, bytes: &[u8]) {
    r.queue.write_texture(
        t.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(256 * t.format().block_copy_size(None).unwrap()),
            rows_per_image: None,
        },
        t.size(),
    );
}
fn floats(values: impl Iterator<Item = f32>) -> Vec<u8> {
    values.flat_map(f32::to_le_bytes).collect()
}

#[test]
fn promotion_preserves_float32_bits_and_pixels_outside_each_region() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let promoter = NativePromoter::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    for format in [
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::R32Float,
    ] {
        let channels = (format.block_copy_size(None).unwrap() / 4) as usize;
        let canonical = texture(&r, format);
        let working = texture(&r, format);
        let original = floats((0..65536 * channels).map(|i| -0.5 + (i % 65536) as f32 / 65535.));
        let replacement =
            floats((0..65536 * channels).map(|i| 0.12345 + (i % 65536) as f32 / 65535.));
        upload(&r, &canonical, &replacement);
        for region in [
            [0, 0, 256, 256],
            [3, 5, 63, 65],
            [255, 0, 1, 256],
            [0, 255, 256, 1],
            [256, 256, 0, 0],
        ] {
            for invalid in [false, true] {
                upload(&r, &working, &original);
                // Gamut clipping is reported separately and does not reject an
                // otherwise valid native publication.
                r.queue.write_buffer(
                    status.buffer(),
                    0,
                    &[u32::from(invalid).to_le_bytes(), 65536u32.to_le_bytes()].concat(),
                );
                let batch = promoter
                    .prepare(
                        &r.device,
                        &[NativePromotion {
                            canonical: &canonical,
                            working: &working,
                            region,
                        }],
                        &status,
                    )
                    .unwrap();
                assert_eq!(batch.is_empty(), region[2] == 0 || region[3] == 0);
                let mut commands = r.device.create_command_encoder(&Default::default());
                promoter.encode(&mut commands, &batch);
                r.queue.submit([commands.finish()]);
                let actual = page_bytes(&r, &working);
                let [x, y, width, height] = region;
                for py in 0..256 {
                    for px in 0..256 {
                        let begin = (py * 256 + px) as usize * channels * 4;
                        let range = begin..begin + channels * 4;
                        let changed =
                            !invalid && px >= x && py >= y && px < x + width && py < y + height;
                        let expected = if changed { &replacement } else { &original };
                        assert_eq!(
                            actual[range.clone()],
                            expected[range],
                            "{format:?} {region:?} invalid={invalid} at {px},{py}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn mixed_promotion_batch_keeps_independent_regions_across_empty_entries() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let promoter = NativePromoter::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let regions = [
        [1, 2, 3, 4],
        [256, 256, 0, 0],
        [255, 255, 1, 1],
        [250, 249, 6, 7],
    ];
    let formats = [
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::R32Float,
        wgpu::TextureFormat::Rgba32Float,
    ];
    let originals = formats.map(|format| {
        floats(
            (0..65536 * format.block_copy_size(None).unwrap() / 4)
                .map(|i| (i % 65536) as f32 / 65535.),
        )
    });
    let replacements = formats.map(|format| {
        floats(
            (0..65536 * format.block_copy_size(None).unwrap() / 4)
                .map(|i| 0.12345 + (i % 65536) as f32 / 65535.),
        )
    });
    let canonical = formats.map(|format| texture(&r, format));
    let working = formats.map(|format| texture(&r, format));
    for i in 0..4 {
        upload(&r, &canonical[i], &replacements[i]);
        upload(&r, &working[i], &originals[i]);
    }
    let requests = std::array::from_fn::<_, 4, _>(|i| NativePromotion {
        canonical: &canonical[i],
        working: &working[i],
        region: regions[i],
    });
    let batch = promoter.prepare(&r.device, &requests, &status).unwrap();
    let mut commands = r.device.create_command_encoder(&Default::default());
    status.reset(&mut commands);
    promoter.encode(&mut commands, &batch);
    r.queue.submit([commands.finish()]);
    for i in 0..4 {
        let mut expected = originals[i].clone();
        let [x, y, width, height] = regions[i];
        let pixel_bytes = formats[i].block_copy_size(None).unwrap() as usize;
        for row in y..y + height {
            let start = (row * 256 + x) as usize * pixel_bytes;
            let end = start + width as usize * pixel_bytes;
            expected[start..end].copy_from_slice(&replacements[i][start..end]);
        }
        assert!(page_bytes(&r, &working[i]) == expected, "mixed region {i}");
    }
}

#[test]
fn late_color_or_scalar_failure_rejects_every_promotion_and_capture() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let color_encoder = NativeTileEncoder::new(&r.device);
    let scalar_encoder = NativeScalarEncoder::new(&r.device);
    let promoter = NativePromoter::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let transfer = r.prepare_native_transfer(RgbSpace::Srgb).unwrap();
    let color = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let scalar = texture(&r, wgpu::TextureFormat::R32Float);
    let canonical_color = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let canonical_scalar = texture(&r, wgpu::TextureFormat::R32Float);
    let working_color = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let working_scalar = texture(&r, wgpu::TextureFormat::R32Float);
    let encoded_color = texture(&r, wgpu::TextureFormat::Rgba16Uint);
    let encoded_scalar = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native promotion scalar fixture"),
        size: 131072,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let old_color = floats((0..262144).map(|i| (i % 19) as f32 / 19.));
    let old_scalar = floats((0..65536).map(|i| (i % 23) as f32 / 23.));
    for failure in [None, Some(true), Some(false), None] {
        let mut colors = floats((0..65536).flat_map(|i| {
            let a = if i % 2 == 0 { 2. / 65535. } else { 1. };
            let rgb = RgbSpace::Srgb.decode(i as f64 / 65535.) as f32 * a;
            [rgb, rgb, rgb, a]
        }));
        let mut scalars = floats((0..65536).map(|i| i as f32 / 65535.));
        if let Some(bad_color) = failure {
            let bytes = if bad_color { &mut colors } else { &mut scalars };
            let last = bytes.len() - 4;
            bytes[last..].copy_from_slice(&f32::NAN.to_le_bytes());
        }
        upload(&r, &color, &colors);
        upload(&r, &scalar, &scalars);
        upload(&r, &working_color, &old_color);
        upload(&r, &working_scalar, &old_scalar);
        let color_request = NativeTileRequest {
            working: &color,
            canonical: &canonical_color,
            encoded: &encoded_color,
            transfer: &transfer,
            depth: IntegerDepth::U16,
            alpha: AlphaAssociation::Straight,
            region: [0, 0, 256, 256],
        };
        let color_descriptor = color_request.descriptor();
        let color_batch = color_encoder
            .prepare(&r.device, &[color_request], &status)
            .unwrap();
        let scalar_request = NativeScalarRequest {
            working: &scalar,
            canonical: &canonical_scalar,
            encoded: &encoded_scalar,
            depth: IntegerDepth::U16,
            region: [0, 0, 256, 256],
        };
        let scalar_descriptor = scalar_request.descriptor();
        let scalar_batch = scalar_encoder
            .prepare(&r.device, &[scalar_request], &status)
            .unwrap();
        let promotions = [
            (&canonical_color, &working_color),
            (&canonical_scalar, &working_scalar),
        ]
        .map(|(canonical, working)| {
            promoter
                .prepare(
                    &r.device,
                    &[NativePromotion {
                        canonical,
                        working,
                        region: [0, 0, 256, 256],
                    }],
                    &status,
                )
                .unwrap()
        });
        let mut commands = r.device.create_command_encoder(&Default::default());
        status.reset(&mut commands);
        // The bad plane is deliberately encoded last. Neither an earlier good
        // color nor an earlier good scalar may be adopted before global status.
        if failure == Some(true) {
            scalar_encoder.encode(
                &mut commands.begin_compute_pass(&Default::default()),
                &scalar_batch,
            );
            color_encoder.encode(
                &mut commands.begin_compute_pass(&Default::default()),
                &color_batch,
            );
        } else {
            color_encoder.encode(
                &mut commands.begin_compute_pass(&Default::default()),
                &color_batch,
            );
            scalar_encoder.encode(
                &mut commands.begin_compute_pass(&Default::default()),
                &scalar_batch,
            );
        }
        for batch in &promotions {
            promoter.encode(&mut commands, batch);
        }
        r.queue.submit([commands.finish()]);
        let color_tile = RasterTile::pending(color_descriptor);
        let scalar_tile = RasterTile::pending(scalar_descriptor);
        let result = r
            .capture_tiles(
                &[
                    TileCapture {
                        source: CaptureSource::Texture(&encoded_color),
                        tile: color_tile.clone(),
                    },
                    TileCapture {
                        source: CaptureSource::Packed(&encoded_scalar),
                        tile: scalar_tile.clone(),
                    },
                ],
                Some(&status),
            )
            .unwrap()
            .finish();
        if failure.is_some() {
            assert!(result.is_err());
            assert!(matches!(color_tile.try_backing(), Some(Err(_))));
            assert!(matches!(scalar_tile.try_backing(), Some(Err(_))));
            assert!(page_bytes(&r, &working_color) == old_color);
            assert!(page_bytes(&r, &working_scalar) == old_scalar);
        } else {
            result.unwrap();
            assert!(color_tile.wait_backing().is_ok());
            assert!(scalar_tile.wait_backing().is_ok());
            assert!(page_bytes(&r, &working_color) == page_bytes(&r, &canonical_color));
            assert!(page_bytes(&r, &working_scalar) == page_bytes(&r, &canonical_scalar));
        }
    }
}

#[test]
fn promotion_preflights_count_layout_regions_and_cross_request_aliases() {
    let r = WgpuRasterizer::new_headless().unwrap();
    let promoter = NativePromoter::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    let a = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let b = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let c = texture(&r, wgpu::TextureFormat::Rgba32Float);
    let scalar = texture(&r, wgpu::TextureFormat::R32Float);
    let request = |canonical, working, region| NativePromotion {
        canonical,
        working,
        region,
    };
    let full = [0, 0, 256, 256];
    for entries in [
        vec![request(&a, &a, full)],
        vec![request(&a, &b, full), request(&b, &c, full)],
        vec![request(&a, &b, full), request(&c, &a, full)],
        vec![request(&a, &b, full), request(&a, &c, full)],
        vec![request(&a, &b, full), request(&c, &b, full)],
        vec![request(&a, &scalar, full)],
        vec![request(&a, &b, [255, 0, 2, 1])],
        vec![request(&a, &b, [u32::MAX, 0, 0, 0])],
        (0..17).map(|_| request(&a, &b, full)).collect(),
    ] {
        assert!(promoter.prepare(&r.device, &entries, &status).is_err());
    }
    assert!(
        promoter
            .prepare(&r.device, &[], &status)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "physical GPU canonical promotion latency"]
fn canonical_promotion_workloads() {
    use std::time::Instant;
    let r = WgpuRasterizer::new_headless().unwrap();
    let started = Instant::now();
    let promoter = NativePromoter::new(&r.device);
    let status = NativeEncodeStatus::new(&r.device);
    eprintln!(
        "native promotion pipeline preparation {:.4} ms",
        started.elapsed().as_secs_f64() * 1000.
    );
    let wait = || {
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(crate::READBACK_TIMEOUT),
            })
            .unwrap()
    };
    for format in [
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::R32Float,
    ] {
        for count in [1usize, 16] {
            let inputs: Vec<_> = (0..count).map(|_| texture(&r, format)).collect();
            let outputs: Vec<_> = (0..count).map(|_| texture(&r, format)).collect();
            let pixel_bytes = format.block_copy_size(None).unwrap() as usize;
            let bytes = floats((0..65536 * pixel_bytes / 4).map(|i| (i % 65536) as f32 / 65535.));
            for t in inputs.iter().chain(&outputs) {
                upload(&r, t, &bytes);
            }
            let mut commands = r.device.create_command_encoder(&Default::default());
            status.reset(&mut commands);
            r.queue.submit([commands.finish()]);
            wait();
            for region in [[0, 0, 256, 256], [3, 5, 63, 65]] {
                let requests: Vec<_> = inputs
                    .iter()
                    .zip(&outputs)
                    .map(|(canonical, working)| NativePromotion {
                        canonical,
                        working,
                        region,
                    })
                    .collect();
                for reuse in [false, true] {
                    let retained = promoter.prepare(&r.device, &requests, &status).unwrap();
                    let mut cpu = Vec::new();
                    let mut completed = Vec::new();
                    let mut first_ms = 0.;
                    for sample in 0..120 {
                        let start = Instant::now();
                        let rebuilt;
                        let batch = if reuse {
                            &retained
                        } else {
                            rebuilt = promoter.prepare(&r.device, &requests, &status).unwrap();
                            &rebuilt
                        };
                        let mut commands = r.device.create_command_encoder(&Default::default());
                        promoter.encode(&mut commands, batch);
                        r.queue.submit([commands.finish()]);
                        let submitted = start.elapsed().as_secs_f64() * 1000.;
                        wait();
                        let finished = start.elapsed().as_secs_f64() * 1000.;
                        if sample == 0 {
                            first_ms = finished;
                        }
                        if sample >= 20 {
                            cpu.push(submitted);
                            completed.push(finished);
                        }
                    }
                    cpu.sort_by(f64::total_cmp);
                    completed.sort_by(f64::total_cmp);
                    eprintln!(
                        "native promotion {format:?} count={count} region={region:?} reuse={reuse} samples=100 payload_bytes={} first_ms={first_ms:.4} cpu_p95={:.4} cpu_p99={:.4} completed_p95={:.4} completed_p99={:.4}",
                        count * 2 * 65536 * pixel_bytes,
                        cpu[94],
                        cpu[98],
                        completed[94],
                        completed[98]
                    );
                }
            }
        }
    }
}
