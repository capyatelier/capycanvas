use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};

#[test]
fn large_photo_display_plans_bound_pixels_without_changing_document_dimensions() {
    for extent in [
        [6000, 4000],
        [8192, 5464],
        [10000, 6000],
        [32768, 1],
        [513, 273],
        [1, 1],
    ] {
        let plan = Plan::new(extent).unwrap();
        assert_eq!(plan.extent, extent);
        assert!(plan.size.into_iter().all(|v| v <= MAX_SIDE));
        assert_eq!(plan.size, extent.map(|v| v.div_ceil(1 << plan.level)));
        assert!(plan.pixel_bytes() <= 6 * 1024 * 1024);
        if plan.level > 0 {
            assert!(
                extent
                    .into_iter()
                    .any(|v| v.div_ceil(1 << (plan.level - 1)) > MAX_SIDE)
            );
        }
    }
    for extent in [[0, 1], [1, 0], [u32::MAX, u32::MAX]] {
        assert!(Plan::new(extent).is_err());
    }
}

fn value(x: u32, y: u32) -> [f32; 4] {
    let alpha = ((x * 7 + y * 3) % 17) as f32 / 16.;
    [
        if (x + y) % 2 == 0 { -0.125 } else { 1.125 } * alpha,
        ((x * 13 + y * 29) % 257) as f32 / 256. * alpha,
        ((x / 256 + y / 256) % 7) as f32 / 6. * alpha,
        alpha,
    ]
}
fn upload(r: &WgpuRasterizer, extent: [u32; 2], pixels: &[[f32; 4]]) -> wgpu::Texture {
    let texture = create_color_target(&r.device, extent, "independent mip source").0;
    let bytes: Vec<_> = pixels
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    r.queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(extent[0] * 16),
            rows_per_image: Some(extent[1]),
        },
        texture.size(),
    );
    texture
}
fn pixels(r: &WgpuRasterizer, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}

#[test]
fn queued_tile_mips_match_float64_area_reference_through_partial_edges_and_updates() {
    let r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    let pipelines = Pipelines::new(&r.device);
    for extent in [[257, 3], [513, 273], [2051, 1027], [4097, 1]] {
        let plan = Plan::new(extent).unwrap();
        let expected: Vec<_> = (0..extent[1])
            .flat_map(|y| (0..extent[0]).map(move |x| value(x, y)))
            .collect();
        let source = upload(&r, extent, &expected);
        let source_before = pixels(&r, &source);
        let last = 8;
        let mut image = Image::with_mips(&r, plan, last);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        // Reuse scratch and edge records in a single queued submission. Visit
        // partial tiles first to expose stale padding and mutable-uniform bugs.
        let mut tiles: Vec<_> = page_coordinates(PixelRect::full(extent)).collect();
        tiles.reverse();
        for coordinate in tiles {
            image
                .write_tile(
                    &r.device,
                    &pipelines,
                    &mut encoder,
                    &source,
                    coordinate.map(|v| v * PAGE_SIZE),
                    coordinate,
                )
                .unwrap();
        }
        encoder.submit(&r.queue);
        let levels: Vec<_> = std::iter::once((plan.level, &image.texture))
            .chain(
                image
                    .reduced
                    .iter()
                    .enumerate()
                    .map(|(i, (texture, _))| (plan.level + i as u32 + 1, texture)),
            )
            .map(|(level, texture)| (level, pixels(&r, texture)))
            .collect();
        for (level, actual) in &levels {
            let scale = 1 << level;
            let size = extent.map(|n| n.div_ceil(scale));
            assert_eq!(image.sample(*level).0, *level);
            assert_eq!(image.sample(*level).2, size);
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let mut sum = [0f64; 4];
                    let mut count = 0;
                    for yy in y * scale..((y + 1) * scale).min(extent[1]) {
                        for xx in x * scale..((x + 1) * scale).min(extent[0]) {
                            let input = expected[(yy * extent[0] + xx) as usize];
                            for i in 0..4 {
                                sum[i] += input[i] as f64;
                            }
                            count += 1;
                        }
                    }
                    for i in 0..4 {
                        let a = actual[(y * size[0] + x) as usize][i] as f64;
                        let b = sum[i] / f64::from(count);
                        assert!(
                            (a - b).abs() < 3e-7,
                            "{extent:?} {x},{y} channel {i}: {a} != {b}"
                        );
                    }
                }
            }
        }
        assert_eq!(image.sample(0).0, plan.level);
        assert_eq!(image.sample(99).0, last);
        assert_eq!(
            source_before,
            pixels(&r, &source),
            "display reduction never writes artwork"
        );
        assert!(image.records.len() <= 4);
        assert_eq!(
            image.storage_bytes(),
            plan.pixel_bytes_through(last) + image.records.len() as u64 * u64::from(last) * 16
        );

        // A later tile update changes only that tile's derived footprint.
        let changed = [0.03125, 0.125, 0.25, 0.5];
        let tile = upload(
            &r,
            [PAGE_SIZE; 2],
            &vec![changed; PAGE_SIZE.pow(2) as usize],
        );
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        image
            .write_tile(&r.device, &pipelines, &mut encoder, &tile, [0; 2], [0; 2])
            .unwrap();
        encoder.submit(&r.queue);
        for (level, actual) in &levels {
            let texture = if *level == plan.level {
                &image.texture
            } else {
                &image.reduced[(level - plan.level - 1) as usize].0
            };
            let updated = pixels(&r, texture);
            let scale = 1 << level;
            let size = extent.map(|n| n.div_ceil(scale));
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let index = (y * size[0] + x) as usize;
                    assert_eq!(
                        updated[index],
                        if x * scale < PAGE_SIZE && y * scale < PAGE_SIZE {
                            changed
                        } else {
                            actual[index]
                        }
                    );
                }
            }
        }
    }
}
