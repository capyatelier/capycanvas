use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};

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

#[test]
fn native_preview_defers_mip_compilation_without_allocating_or_losing_the_request() {
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: IntegerDepth::U16,
    })
    .unwrap();
    let layers = [Layer::paint(LayerId(1), "deferred preview")];
    r.ensure_document([513, 17], &layers).unwrap();
    r.startup = Some(startup::Startup::new(&r.device).unwrap());
    let (release, wait) = std::sync::mpsc::channel();
    let (entered, blocked) = std::sync::mpsc::channel();
    let compiler = &r.startup.as_ref().unwrap().compiler;
    compiler.enqueue(0, move || {
        entered.send(()).map_err(|e| e.to_string())?;
        wait.recv_timeout(std::time::Duration::from_secs(20))
            .map_err(|e| e.to_string())
    });
    compiler.start();
    blocked
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    assert!(!r.request_canvas_preview(None).unwrap());
    assert!(!r.canvas_preview_pending());
    assert_eq!(r.canvas_preview.storage_bytes(), 0);
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !r.request_canvas_preview(None).unwrap() {
        assert!(
            std::time::Instant::now() < deadline,
            "mip pipeline did not complete"
        );
        std::thread::yield_now();
    }
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let image = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
    assert_eq!([image.width, image.height], [256, 8]);
    assert!(image.bytes.iter().all(|v| *v == 0));
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
        depth: IntegerDepth::U16,
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
        let mut image = Image::new(&r, &pipelines, plan);
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
        let actual = pixels(&r, &image.texture);
        let scale = 1 << plan.level;
        for y in 0..plan.size[1] {
            for x in 0..plan.size[0] {
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
                    let a = actual[(y * plan.size[0] + x) as usize][i] as f64;
                    let b = sum[i] / f64::from(count);
                    assert!(
                        (a - b).abs() < 3e-7,
                        "{extent:?} {x},{y} channel {i}: {a} != {b}"
                    );
                }
            }
        }
        assert_eq!(
            source_before,
            pixels(&r, &source),
            "display reduction never writes artwork"
        );
        assert!(image.records.len() <= 4);
        assert_eq!(
            image.storage_bytes(),
            plan.pixel_bytes() + 16 + image.records.len() as u64 * u64::from(plan.level) * 16
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
        let updated = pixels(&r, &image.texture);
        for y in 0..plan.size[1] {
            for x in 0..plan.size[0] {
                let index = (y * plan.size[0] + x) as usize;
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

#[test]
fn native_navigator_averages_fine_stripes_and_preserves_alpha_and_revision_reuse() {
    let color = DocumentColor {
        space: RgbSpace::Srgb,
        depth: IntegerDepth::U16,
    };
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let extent = [4101, 259];
    let doc = layer_core::Document::new("Navigator mip geometry", extent[0], extent[1]);
    r.submit(FramePacket {
        layers: &doc.layers,
        document_extent: extent,
        view: layer_render::ViewState {
            width_px: 640,
            height_px: 480,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [0.; 4],
        },
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: true,
    })
    .unwrap();
    // One bright column per four pixels aliases to black under the old fixed
    // sixteen samples. Full-area mip reduction must retain its quarter coverage.
    let input: Vec<_> = (0..extent[1])
        .flat_map(|_| {
            (0..extent[0]).map(|x| {
                let v = if x % 4 == 0 { 0.5 } else { 0. };
                [v, v, v, 0.5]
            })
        })
        .collect();
    let source = upload(&r, extent, &input);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        r.composite_texture.as_ref().unwrap().as_image_copy(),
        source.size(),
    );
    encoder.submit(&r.queue);
    let before = pixels(&r, r.composite_texture.as_ref().unwrap());
    assert!(r.request_canvas_preview(None).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let preview = r.take_canvas_preview().unwrap().unwrap();
    let revision = preview.revision;
    let image = preview.image.unwrap();
    assert_eq!([image.width, image.height], [256, 16]);
    for row in image.bytes.chunks_exact(image.stride as usize) {
        // The last column has the actual partial footprint and is tested below
        // with a uniform coarse-cell fixture. All interior stripe averages agree.
        for pixel in row[..(image.width as usize - 1) * 4].chunks_exact(4) {
            for channel in &pixel[..3] {
                assert!(channel.abs_diff(137) <= 1, "{pixel:?}");
            }
            assert!(pixel[3].abs_diff(128) <= 1);
        }
    }
    assert_eq!(before, pixels(&r, r.composite_texture.as_ref().unwrap()));
    let bytes = r.canvas_preview.storage_bytes();
    assert!(bytes < 7 * 1024 * 1024);
    assert!(r.request_canvas_preview(Some(revision)).unwrap());
    assert!(r.take_canvas_preview().unwrap().unwrap().image.is_none());
    assert_eq!(r.canvas_preview.storage_bytes(), bytes);

    // Piecewise-constant colors aligned with sixteen-pixel mip cells allow an
    // independent exact box integral in original document coordinates, including
    // the final five columns and three rows, with no mip implementation oracle.
    let values = |x: u32, y: u32| {
        let alpha = if (y / 16) % 3 == 0 { 0.5 } else { 1. };
        let v = ((x / 16) % 17) as f64 / 16. * alpha;
        [v, v, v, alpha]
    };
    let input: Vec<_> = (0..extent[1])
        .flat_map(|y| (0..extent[0]).map(move |x| values(x, y).map(|v| v as f32)))
        .collect();
    let source = upload(&r, extent, &input);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_texture_to_texture(
        source.as_image_copy(),
        r.composite_texture.as_ref().unwrap().as_image_copy(),
        source.size(),
    );
    encoder.submit(&r.queue);
    assert!(r.request_canvas_preview(None).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    let image = r.take_canvas_preview().unwrap().unwrap().image.unwrap();
    for y in 0..image.height {
        for x in 0..image.width {
            let low = [
                x as f64 / image.width as f64 * extent[0] as f64,
                y as f64 / image.height as f64 * extent[1] as f64,
            ];
            let high = [
                (x + 1) as f64 / image.width as f64 * extent[0] as f64,
                (y + 1) as f64 / image.height as f64 * extent[1] as f64,
            ];
            let mut sum = [0.; 4];
            let mut weight = 0.;
            for yy in low[1].floor() as u32..high[1].ceil() as u32 {
                for xx in low[0].floor() as u32..high[0].ceil() as u32 {
                    let area = (high[0].min(xx as f64 + 1.) - low[0].max(xx as f64))
                        * (high[1].min(yy as f64 + 1.) - low[1].max(yy as f64));
                    let v = values(xx, yy);
                    for i in 0..4 {
                        sum[i] += v[i] * area;
                    }
                    weight += area;
                }
            }
            let v = sum[0] / sum[3];
            let srgb = if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            };
            let expected = [
                (srgb * 255.).round() as u8,
                (sum[3] / weight * 255.).round() as u8,
            ];
            let index = (y * image.stride + x * 4) as usize;
            for i in 0..4 {
                let e = expected[usize::from(i == 3)];
                assert!(
                    image.bytes[index + i].abs_diff(e) <= 1,
                    "{x},{y} channel {i}: {} != {e}",
                    image.bytes[index + i]
                );
            }
        }
    }
}
