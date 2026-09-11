//! Test-only CPU pixel oracle; production has no CPU transform/raster fallback.
use super::*;
use crate::{WgpuRasterizer, create_color_target};
use layer_core::Point;
use wgpu::util::DeviceExt;

fn upload(r: &WgpuRasterizer, size: [u32; 2], pixels: &[u8]) -> wgpu::Texture {
    let (t, _) = create_color_target(r.device(), size, "transform fixture");
    r.queue().write_texture(
        t.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        t.size(),
    );
    t
}
fn read(r: &WgpuRasterizer, t: &wgpu::Texture) -> Vec<u8> {
    let pitch = (t.width() * 4).div_ceil(256) * 256;
    let b = r.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("test pixel readback"),
        size: u64::from(pitch) * u64::from(t.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut e = r.device().create_command_encoder(&Default::default());
    e.copy_texture_to_buffer(
        t.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &b,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(t.height()),
            },
        },
        t.size(),
    );
    r.queue().submit([e.finish()]);
    b.map_async(wgpu::MapMode::Read, .., |result| result.unwrap());
    wait(r);
    let pixels = b
        .get_mapped_range(..)
        .unwrap()
        .chunks_exact(pitch as usize)
        .flat_map(|row| row[..t.width() as usize * 4].iter().copied())
        .collect();
    b.unmap();
    pixels
}
fn wait(r: &WgpuRasterizer) {
    r.device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
}
fn mask(
    r: &WgpuRasterizer,
    size: [u32; 2],
    origin: [i32; 2],
    values: &[u32],
    inverted: bool,
) -> wgpu::Buffer {
    let mut words = vec![
        0,
        0,
        size[0],
        size[1],
        u32::from(inverted),
        1,
        (origin[0] as f32).to_bits(),
        (origin[1] as f32).to_bits(),
    ];
    let stride = size[0].div_ceil(8) as usize;
    words.resize((8 + stride * size[1] as usize).max(12), 0);
    for (i, v) in values.iter().enumerate() {
        let x = i % size[0] as usize;
        let y = i / size[0] as usize;
        words[8 + y * stride + x / 8] |= v << ((x % 8) * 4);
    }
    let bytes: Vec<_> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
    r.device()
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test packed coverage"),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE,
        })
}
fn draw(
    r: &WgpuRasterizer,
    p: &mut PixelTransform,
    source: &TransformSource,
    t: &wgpu::Texture,
    origin: [i32; 2],
    transform: ImageTransform,
) {
    let v = t.create_view(&Default::default());
    let mut e = r.device().create_command_encoder(&Default::default());
    p.encode(
        r.device(),
        r.queue(),
        &mut e,
        source,
        transform,
        &[TransformTarget {
            view: &v,
            origin,
            extent: [t.width(), t.height()],
            region: [0, 0, t.width(), t.height()],
        }],
    )
    .unwrap();
    r.queue().submit([e.finish()]);
}

#[test]
fn transforms_match_independent_premultiplied_oracle_with_coverage_and_crop() {
    let r = WgpuRasterizer::new().unwrap();
    let mut pass = PixelTransform::new(r.device());
    let size = [17, 13];
    let origin = [257, 259];
    let pixels: Vec<u8> = (0..size[0] * size[1])
        .flat_map(|i| {
            let x = i % size[0];
            let y = i / size[0];
            let alpha = if (x + y) % 5 == 0 {
                0
            } else {
                120 + (x * 7 + y * 5) % 136
            };
            [
                (alpha * x / 16) as u8,
                (alpha * y / 12) as u8,
                (alpha * ((x + y) % 7) / 6) as u8,
                alpha as u8,
            ]
        })
        .collect();
    let values: Vec<u32> = (0..size[0] * size[1])
        .map(|i| {
            let x = i % size[0];
            let y = i / size[0];
            if y == 6 {
                0
            } else if x < 8 {
                4
            } else if x == 8 {
                2
            } else {
                0
            }
        })
        .collect();
    let source_texture = upload(&r, size, &pixels);
    let (output, _) = create_color_target(r.device(), [35, 31], "transform result");
    let output_origin = [250, 252];
    let pivot = Point { x: 265.5, y: 265.5 };
    for mode in 0..3 {
        let buffer = mask(&r, size, origin, &values, mode == 2);
        let source = pass
            .source(
                r.device(),
                &source_texture,
                origin,
                (mode != 0).then_some(&buffer),
            )
            .unwrap();
        for interpolation in [Interpolation::Nearest, Interpolation::Linear] {
            for affine in [
                Affine::IDENTITY,
                Affine::translation(Point { x: 4., y: -3. }),
                Affine::translation(Point { x: 0.25, y: 0.6 }),
                Affine::around(pivot, [1.7, 0.7], 0.31, Point::default()),
                Affine::around(
                    pivot,
                    [1., 1.],
                    std::f32::consts::FRAC_PI_2,
                    Point::default(),
                ),
                Affine::around(pivot, [-1., 1.], 0., Point::default()),
                Affine::around(pivot, [0.25, 0.4], -0.7, Point::default()),
                Affine::translation(Point { x: 1e20, y: -1e20 }),
            ] {
                let transform = ImageTransform {
                    affine,
                    interpolation,
                };
                draw(&r, &mut pass, &source, &output, output_origin, transform);
                let actual = read(&r, &output);
                let coverage = |x: i32, y: i32| -> f64 {
                    if mode == 0 {
                        return 1.;
                    }
                    let x = x - origin[0];
                    let y = y - origin[1];
                    let m = if x >= 0 && y >= 0 && x < size[0] as i32 && y < size[1] as i32 {
                        values[(y * size[0] as i32 + x) as usize] as f64 / 4.
                    } else {
                        0.
                    };
                    if mode == 2 { 1. - m } else { m }
                };
                let color = |x: i32, y: i32| -> [f64; 4] {
                    let x = x - origin[0];
                    let y = y - origin[1];
                    if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
                        return [0.; 4];
                    }
                    let i = (y * size[0] as i32 + x) as usize * 4;
                    std::array::from_fn(|c| pixels[i + c] as f64 / 255.)
                };
                // Independently invert in f64; use weighted integer neighbors,
                // not the shader's nested mix or the core inverse implementation.
                let [a, b, c, d, tx, ty] = affine.0.map(f64::from);
                let det = a * d - b * c;
                for y in 0..output.height() {
                    for x in 0..output.width() {
                        let gx = x as i32 + output_origin[0];
                        let gy = y as i32 + output_origin[1];
                        let base = color(gx, gy);
                        let dx = gx as f64 + 0.5 - tx;
                        let dy = gy as f64 + 0.5 - ty;
                        let u = (d * dx - c * dy) / det;
                        let v = (-b * dx + a * dy) / det;
                        let mut moved = [0.; 4];
                        if u.abs() < 1e9 && v.abs() < 1e9 {
                            if interpolation == Interpolation::Nearest {
                                let (ix, iy) = (u.floor() as i32, v.floor() as i32);
                                let color = color(ix, iy);
                                let m = coverage(ix, iy);
                                for k in 0..4 {
                                    moved[k] = color[k] * m;
                                }
                            } else {
                                let left = (u - 0.5).floor() as i32;
                                let top = (v - 0.5).floor() as i32;
                                for iy in top..=top + 1 {
                                    for ix in left..=left + 1 {
                                        let w = (1. - (u - 0.5 - ix as f64).abs())
                                            * (1. - (v - 0.5 - iy as f64).abs())
                                            * coverage(ix, iy);
                                        let color = color(ix, iy);
                                        for k in 0..4 {
                                            moved[k] += color[k] * w;
                                        }
                                    }
                                }
                            }
                        }
                        let m = coverage(gx, gy);
                        let i = (y * output.width() + x) as usize * 4;
                        for k in 0..4 {
                            let expected = if affine == Affine::IDENTITY {
                                base[k]
                            } else {
                                moved[k] + base[k] * (1. - m) * (1. - moved[3])
                            };
                            let expected = (expected.clamp(0., 1.) * 255.).round() as u8;
                            assert!(
                                actual[i + k].abs_diff(expected) <= 2,
                                "mode {mode}, {transform:?}, {x},{y},{k}: {} != {expected}",
                                actual[i + k]
                            );
                        }
                    }
                }
            }
        }
    }
    assert_eq!(
        read(&r, &source_texture),
        pixels,
        "previews must never mutate their source"
    );
}

#[test]
fn transform_regions_are_seamless_reuse_storage_and_preserve_untouched_pixels() {
    let r = WgpuRasterizer::new().unwrap();
    let mut p = PixelTransform::new(r.device());
    let size = [520, 280];
    let pixels: Vec<_> = (0..size[0] * size[1])
        .flat_map(|i| [(i % 251) as u8, (i % 193) as u8, 64, 255])
        .collect();
    let texture = upload(&r, size, &pixels);
    let source = p.source(r.device(), &texture, [0, 0], None).unwrap();
    let mut empty = r.device().create_command_encoder(&Default::default());
    p.encode(
        r.device(),
        r.queue(),
        &mut empty,
        &source,
        ImageTransform::default(),
        &[],
    )
    .unwrap();
    assert_eq!(
        p.storage_bytes(),
        48,
        "empty work does not allocate uniforms"
    );
    assert!(p.records.is_empty());
    let transform = ImageTransform {
        affine: Affine::around(
            Point { x: 260., y: 140. },
            [0.9, 0.8],
            0.15,
            Point { x: 20., y: 8. },
        ),
        interpolation: Interpolation::Linear,
    };
    let (full, _) = create_color_target(r.device(), size, "whole transform");
    draw(&r, &mut p, &source, &full, [0, 0], transform);
    let expected = read(&r, &full);
    let tiles: Vec<_> = [
        ([0, 0], [256, 256]),
        ([256, 0], [256, 256]),
        ([512, 0], [8, 256]),
        ([0, 256], [256, 24]),
        ([256, 256], [256, 24]),
        ([512, 256], [8, 24]),
    ]
    .into_iter()
    .map(|(origin, size)| {
        let (t, v) = create_color_target(r.device(), size, "transform tile");
        (origin, t, v)
    })
    .collect();
    let targets: Vec<_> = tiles
        .iter()
        .map(|(origin, t, v)| TransformTarget {
            view: v,
            extent: [t.width(), t.height()],
            origin: *origin,
            region: [0, 0, t.width(), t.height()],
        })
        .collect();
    let mut e = r.device().create_command_encoder(&Default::default());
    p.encode(r.device(), r.queue(), &mut e, &source, transform, &targets)
        .unwrap();
    r.queue().submit([e.finish()]);
    for (origin, t, _) in &tiles {
        let actual = read(&r, t);
        for y in 0..t.height() {
            for x in 0..t.width() {
                let a = ((y * t.width() + x) * 4) as usize;
                let b = (((y + origin[1] as u32) * size[0] + x + origin[0] as u32) * 4) as usize;
                assert_eq!(
                    &actual[a..a + 4],
                    &expected[b..b + 4],
                    "tile {origin:?} pixel {x},{y}"
                );
            }
        }
    }
    let capacity = p.storage_bytes();
    let records = p.records.as_ptr();
    let sentinel = vec![93; size[0] as usize * size[1] as usize * 4];
    let partial = upload(&r, size, &sentinel);
    let view = partial.create_view(&Default::default());
    for i in 0..5 {
        let mut e = r.device().create_command_encoder(&Default::default());
        p.encode(
            r.device(),
            r.queue(),
            &mut e,
            &source,
            ImageTransform {
                affine: Affine::translation(Point { x: i as f32, y: 1. }),
                ..Default::default()
            },
            &[TransformTarget {
                view: &view,
                origin: [0, 0],
                extent: size,
                region: [200, 100, 80, 70],
            }],
        )
        .unwrap();
        r.queue().submit([e.finish()]);
        assert_eq!(p.storage_bytes(), capacity);
        assert_eq!(p.records.as_ptr(), records);
    }
    let result = read(&r, &partial);
    for y in 0..size[1] {
        for x in 0..size[0] {
            if !(200..280).contains(&x) || !(100..170).contains(&y) {
                let i = ((y * size[0] + x) * 4) as usize;
                assert_eq!(&result[i..i + 4], &sentinel[i..i + 4]);
            }
        }
    }
    let mut e = r.device().create_command_encoder(&Default::default());
    assert!(
        p.encode(
            r.device(),
            r.queue(),
            &mut e,
            &source,
            ImageTransform {
                affine: Affine([0.; 6]),
                ..Default::default()
            },
            &targets
        )
        .is_err()
    );
    assert!(
        p.encode(
            r.device(),
            r.queue(),
            &mut e,
            &source,
            transform,
            &[TransformTarget {
                view: &view,
                extent: size,
                origin: [0, 0],
                region: [u32::MAX, 0, 2, 2]
            }]
        )
        .is_err()
    );
    assert_eq!(
        p.storage_bytes(),
        capacity,
        "invalid work does not allocate"
    );
    assert!(p.source(r.device(), &texture, [i32::MAX, 0], None).is_err());
    for (size, usage) in [
        (32, wgpu::BufferUsages::STORAGE),
        (48, wgpu::BufferUsages::COPY_DST),
    ] {
        let invalid = r.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("invalid selection fixture"),
            size,
            usage,
            mapped_at_creation: false,
        });
        assert!(
            p.source(r.device(), &texture, [0, 0], Some(&invalid))
                .is_err()
        );
    }
}

#[test]
#[ignore = "hardware GPU transform benchmark; release, serial"]
fn transform_latency() {
    use std::time::Instant;
    let r = WgpuRasterizer::new().unwrap();
    let start = Instant::now();
    let mut p = PixelTransform::new(r.device());
    eprintln!(
        "transform pipeline creation: {:.3}ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    let size = [2048, 1536];
    let pixels: Vec<_> = (0..size[0] * size[1])
        .flat_map(|i| [(i % 193) as u8, (i % 211) as u8, 64, 230])
        .collect();
    let texture = upload(&r, size, &pixels);
    let values: Vec<_> = (0..size[0] * size[1])
        .map(|i| if (i % size[0]) / 128 % 2 == 0 { 4 } else { 2 })
        .collect();
    let coverage = mask(&r, size, [0, 0], &values, false);
    let plain = p.source(r.device(), &texture, [0, 0], None).unwrap();
    let selected = p
        .source(r.device(), &texture, [0, 0], Some(&coverage))
        .unwrap();
    let (_, full) = create_color_target(r.device(), size, "transform full output");
    let tile_views: Vec<_> = (0..48)
        .map(|_| create_color_target(r.device(), [256, 256], "transform tile output").1)
        .collect();
    let tile_targets: Vec<_> = tile_views
        .iter()
        .enumerate()
        .map(|(i, view)| TransformTarget {
            view,
            extent: [256, 256],
            origin: [(i % 8) as i32 * 256, (i / 8) as i32 * 256],
            region: [0, 0, 256, 256],
        })
        .collect();
    let full_target = [TransformTarget {
        view: &full,
        extent: size,
        origin: [0, 0],
        region: [0, 0, size[0], size[1]],
    }];
    let small_target = [TransformTarget {
        view: &full,
        extent: size,
        origin: [0, 0],
        region: [512, 512, 256, 256],
    }];
    let percentile = |values: &mut [f64]| {
        values.sort_by(f64::total_cmp);
        [0.5, 0.95, 0.99].map(|p| values[(values.len() as f64 * p).ceil() as usize - 1])
    };
    for (name, source, targets, identity) in [
        ("copy baseline", &plain, full_target.as_slice(), true),
        ("whole layer", &plain, full_target.as_slice(), false),
        ("selected", &selected, full_target.as_slice(), false),
        (
            "selected 48 tiles",
            &selected,
            tile_targets.as_slice(),
            false,
        ),
        (
            "selected dirty 256px region",
            &selected,
            small_target.as_slice(),
            false,
        ),
    ] {
        let mut telemetry = crate::telemetry::Telemetry::new(r.device(), r.queue());
        telemetry.enabled = true;
        let mut cpu = Vec::new();
        let mut completed = Vec::new();
        let mut capacity = 0;
        for i in 0..160 {
            let transform = ImageTransform {
                affine: if identity {
                    Affine::IDENTITY
                } else {
                    Affine::around(
                        Point { x: 1024., y: 768. },
                        [0.9, 1.1],
                        0.2 + i as f32 * 0.001,
                        Point { x: 30., y: 20. },
                    )
                },
                interpolation: Interpolation::Linear,
            };
            let start = Instant::now();
            let mut e = r.device().create_command_encoder(&Default::default());
            if i >= 40 {
                telemetry.begin(r.device(), &mut e);
            }
            p.encode(r.device(), r.queue(), &mut e, source, transform, targets)
                .unwrap();
            if i >= 40 {
                telemetry.end(&mut e);
            }
            r.queue().submit([e.finish()]);
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            if i >= 40 {
                telemetry.submitted();
            }
            wait(&r);
            if i >= 40 {
                cpu.push(elapsed);
                completed.push(start.elapsed().as_secs_f64() * 1000.);
                assert_eq!(p.storage_bytes(), capacity);
            } else {
                capacity = p.storage_bytes();
            }
        }
        let snapshot = telemetry.snapshot();
        let mut gpu: Vec<_> = snapshot.gpu.ordered().into_iter().map(f64::from).collect();
        assert_eq!(gpu.len(), 120);
        let cpu = percentile(&mut cpu);
        let gpu = percentile(&mut gpu);
        let completed = percentile(&mut completed);
        eprintln!(
            "{name}: CPU median/p95/p99 {cpu:.3?}ms, GPU {gpu:.3?}ms, completion {completed:.3?}ms; scratch {}B",
            p.storage_bytes()
        );
        assert!(
            completed[2] < 8.333,
            "transform exceeds 120Hz budget: {name}"
        );
    }
}
