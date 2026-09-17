use super::*;
use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace, rgb, source::*};
use layer_render::ViewState;

fn codes(x: u32, y: u32) -> [u16; 4] {
    [
        ((x * 8191 + y * 31) % 65536) as u16,
        ((x * 17 + y * 16381) % 65536) as u16,
        if (x / 173 + y / 111) % 2 == 0 {
            65535
        } else {
            0
        },
        [0, 1, 257, 32768, 65535][((x + y * 3) % 5) as usize],
    ]
}
fn photo(extent: [u32; 2]) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0])
            .flat_map(|x| codes(x, y))
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}
fn read_sums(r: &WgpuRasterizer) -> Vec<[f32; 4]> {
    let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test overview sums"),
        size: OVERVIEW_BYTES,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    encoder.copy_buffer_to_buffer(
        &r.thumbnails.sources.as_ref().unwrap().working,
        0,
        &buffer,
        0,
        OVERVIEW_BYTES,
    );
    encoder.submit(&r.queue);
    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    rx.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().unwrap();
    bytes
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|i| f32::from_le_bytes(p[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect()
}
fn thumbnail(r: &mut WgpuRasterizer, id: LayerId) -> Vec<u8> {
    r.start_thumbnail(7, id).unwrap();
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    r.thumbnails.take().unwrap().unwrap().bytes
}

#[test]
fn placed_photo_thumbnail_keeps_full_source_orientation_and_off_canvas_paint() {
    let mut builder = SourceBuilder::new(
        [1024, 512],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        8 * 1024 * 1024,
    )
    .unwrap();
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
    ];
    for y in 0..512 {
        let row: Vec<u8> = (0..1024)
            .flat_map(|x| colors[(y / 256 * 2 + x / 512) as usize])
            .collect();
        builder.push_row(&row).unwrap();
    }
    let source = Arc::new(builder.finish().unwrap());
    let mut layer = Layer::paint(LayerId(1), "oversized photo");
    layer.source = Some(source.clone());
    layer.properties.placement = layer_core::Affine([0.125, 0., 0., 0.125, 17., -30.]);
    let mut sibling = layer.clone();
    sibling.id = LayerId(2);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([128, 64], &[layer.clone(), sibling.clone()])
        .unwrap();
    let pixel = |bytes: &[u8], x: usize, y: usize| {
        <[u8; 4]>::try_from(&bytes[(y * 32 + x) * 4..(y * 32 + x + 1) * 4]).unwrap()
    };
    let fitted = thumbnail(&mut r, layer.id);
    for (x, y, color) in [
        (8, 12, colors[0]),
        (24, 12, colors[1]),
        (8, 20, colors[2]),
        (24, 20, colors[3]),
    ] {
        assert_eq!(pixel(&fitted, x, y), color, "full source at {x},{y}");
    }
    assert_eq!(thumbnail(&mut r, sibling.id), fitted);
    let owners = Arc::strong_count(&source);
    let work = r.scene.as_ref().unwrap().source_cache_work()[1];
    // Content framing ignores translation/uniform zoom, including Original Size.
    layer.properties.placement = layer_core::Affine([1., 0., 0., 1., -8000., 9000.]);
    r.ensure_document([128, 64], &[layer.clone(), sibling.clone()])
        .unwrap();
    assert_eq!(thumbnail(&mut r, layer.id), fitted);
    // Rotation must change the visible content, without rebuilding the original.
    layer.properties.placement = layer_core::Affine([0., 0.25, -0.25, 0., 12., 98.]);
    r.ensure_document([128, 64], &[layer.clone(), sibling.clone()])
        .unwrap();
    let rotated = thumbnail(&mut r, layer.id);
    for (x, y, color) in [
        (12, 8, colors[2]),
        (20, 8, colors[0]),
        (12, 24, colors[3]),
        (20, 24, colors[1]),
    ] {
        assert_eq!(pixel(&rotated, x, y), color, "rotated source at {x},{y}");
    }
    assert_eq!(
        thumbnail(&mut r, sibling.id),
        fitted,
        "shared source has independent placement"
    );
    assert_eq!(
        r.scene.as_ref().unwrap().source_cache_work()[1],
        work,
        "geometry never decodes the source again"
    );
    assert_eq!(r.thumbnails.sources.as_ref().unwrap().builds, 1);
    assert_eq!(
        Arc::strong_count(&source),
        owners,
        "preview cache owns no source"
    );
    for (matrix, samples) in [
        (
            layer_core::Affine([-0.25, 0., 0., 0.25, 0., 0.]),
            [(8, 12, 1), (24, 12, 0), (8, 20, 3), (24, 20, 2)],
        ),
        (
            layer_core::Affine([0.5, 0., 0., 0.25, 0., 0.]),
            [(8, 14, 0), (24, 14, 1), (8, 18, 2), (24, 18, 3)],
        ),
        (
            layer_core::Affine([0.25, 0.25, -0.25, 0.25, 0., 0.]),
            [(13, 8, 0), (24, 18, 1), (8, 13, 2), (18, 24, 3)],
        ),
    ] {
        let mut transformed = layer.clone();
        transformed.properties.placement = matrix;
        r.ensure_document([128, 64], &[transformed, sibling.clone()])
            .unwrap();
        let image = thumbnail(&mut r, layer.id);
        for (x, y, color) in samples {
            assert_eq!(
                pixel(&image, x, y),
                colors[color],
                "flipped/asymmetric/45-degree content at {x},{y}"
            );
        }
    }
    r.ensure_document([128, 64], &[layer.clone(), sibling.clone()])
        .unwrap();

    // An edited tile beyond the canvas belongs to the local source and rotates with it.
    let mut page = r.create_page([3, 1], "off-canvas thumbnail paint");
    page.primary_needs_clear = false;
    r.queue.write_texture(
        page.primary.texture.as_image_copy(),
        &[0u8, 255, 255, 255].repeat((PAGE_SIZE * PAGE_SIZE) as usize),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(PAGE_SIZE * 4),
            rows_per_image: None,
        },
        page.primary.texture.size(),
    );
    r.paint_layers
        .iter_mut()
        .find(|p| p.id == layer.id)
        .unwrap()
        .pages
        .push(page);
    let painted = thumbnail(&mut r, layer.id);
    assert_eq!(pixel(&painted, 12, 28), [0, 255, 255, 255]);
    assert_eq!(pixel(&painted, 20, 8), colors[0]);
    assert_eq!(
        thumbnail(&mut r, sibling.id),
        fitted,
        "paint does not modify the shared original"
    );
    r.paint_layers
        .iter_mut()
        .find(|p| p.id == layer.id)
        .unwrap()
        .pages
        .clear();
    assert_eq!(
        thumbnail(&mut r, layer.id),
        rotated,
        "undo restores the original contribution"
    );
    assert_eq!(r.thumbnails.sources.as_ref().unwrap().builds, 1);
}

#[test]
fn tiny_portrait_photo_thumbnail_has_color_and_checkered_letterbox() {
    let source = photo([24, 48]);
    let mut layer = Layer::paint(LayerId(1), "tiny photo");
    layer.source = Some(source);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([24, 48], &[layer]).unwrap();
    let bytes = thumbnail(&mut r, LayerId(1));
    assert!(bytes.chunks_exact(4).all(|p| p[3] == 255));
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|p| p[0].abs_diff(p[2]) > 20)
            .count()
            > 100
    );
    assert_ne!(bytes[0], bytes[16], "letterbox checker survives");
}
fn reference(
    extent: [u32; 2],
    source: [u32; 2],
    override_color: Option<[f64; 4]>,
) -> Vec<[f64; 4]> {
    let matrix = RgbSpace::ProPhoto.linear_transform(RgbSpace::Srgb);
    let side = f64::from(extent[0].max(extent[1]));
    let step = side / 32.;
    let origin = extent.map(|v| (f64::from(v) - side) * 0.5);
    let mut result = vec![[0.; 4]; 1024];
    for oy in 0..32 {
        for ox in 0..32 {
            let low = [origin[0] + ox as f64 * step, origin[1] + oy as f64 * step];
            let high = low.map(|v| v + step);
            for y in low[1].floor().max(0.) as u32..(high[1].ceil().max(0.) as u32).min(extent[1]) {
                for x in
                    low[0].floor().max(0.) as u32..(high[0].ceil().max(0.) as u32).min(extent[0])
                {
                    let value = if x / 256 == 0 && y / 256 == 1 && override_color.is_some() {
                        override_color.unwrap()
                    } else if x < source[0] && y < source[1] {
                        let c = codes(x, y).map(|v| f64::from(v) / 65535.);
                        let rgb = rgb::apply(
                            matrix,
                            [c[0], c[1], c[2]].map(|v| RgbSpace::ProPhoto.decode(v)),
                        );
                        [rgb[0] * c[3], rgb[1] * c[3], rgb[2] * c[3], c[3]]
                    } else {
                        [0.; 4]
                    };
                    let weight = (high[0].min(f64::from(x + 1)) - low[0].max(f64::from(x)))
                        * (high[1].min(f64::from(y + 1)) - low[1].max(f64::from(y)))
                        / (step * step);
                    for channel in 0..4 {
                        result[oy * 32 + ox][channel] += value[channel] * weight;
                    }
                }
            }
        }
    }
    result
}

#[test]
fn photo_overview_matches_float64_area_integrals_and_reuses_unchanged_originals() {
    let source = photo([2305, 769]); // Forty tiles and partial right/bottom edges.
    let extent = [2401, 901]; // Letterbox and empty canvas beyond original.
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "photo");
    layer.source = Some(source.clone());
    r.submit(FramePacket {
        view: ViewState {
            width_px: 512,
            height_px: 512,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [0.; 4],
        },
        document_extent: extent,
        layers: std::slice::from_ref(&layer),
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        time_seconds: 0.,
        composite_all: true,
    })
    .unwrap();
    let owners = Arc::strong_count(&source);
    for (phase, override_color) in [None, Some([1., 0., 0., 1.]), Some([0.; 4]), None]
        .into_iter()
        .enumerate()
    {
        r.paint_layers[0].pages.clear();
        if let Some(color) = override_color {
            let mut page = r.create_page([0, 1], "test thumbnail override");
            page.primary_needs_clear = false;
            let bytes = color
                .map(|v| (v * 255.) as u8)
                .repeat((PAGE_SIZE * PAGE_SIZE) as usize);
            r.queue.write_texture(
                page.primary.texture.as_image_copy(),
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PAGE_SIZE * 4),
                    rows_per_image: None,
                },
                page.primary.texture.size(),
            );
            r.paint_layers[0].pages.push(page);
        }
        let image = thumbnail(&mut r, layer.id);
        assert!(image.chunks_exact(4).all(|p| p[3] == 255));
        let actual = read_sums(&r);
        let expected = reference(extent, source.extent, override_color);
        let error = actual
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
            .map(|(a, b)| (f64::from(*a) - b).abs())
            .fold(0., f64::max);
        assert!(
            error < 0.00002,
            "phase {phase}, maximum linear error {error}"
        );
        assert_eq!(r.thumbnails.sources.as_ref().unwrap().builds, 1);
        assert_eq!(
            Arc::strong_count(&source),
            owners,
            "thumbnail caches must not pin originals"
        );
        let work = r.scene.as_ref().unwrap().source_cache_work();
        assert_eq!(
            thumbnail(&mut r, layer.id),
            image,
            "repeat query remains exact"
        );
        assert_eq!(
            r.scene.as_ref().unwrap().source_cache_work()[1],
            work[1],
            "warm requests decode no unchanged source tiles"
        );
        assert!(r.metrics.source_upload_peak_bytes <= 16 * 1024 * 1024);
    }
    // Cache capacity, document-extent invalidation, and weak ownership are
    // independent of whether originals share their compressed tile storage.
    r.paint_layers[0].pages.clear();
    let mut retained = Vec::new();
    for _ in 0..10 {
        let next = photo([17, 13]);
        retained.push(next.clone());
        r.tiled_sources.insert(layer.id, next);
        thumbnail(&mut r, layer.id);
    }
    assert_eq!(
        r.thumbnails.sources.as_ref().unwrap().cache.len(),
        OVERVIEWS
    );
    assert!(
        r.thumbnails.storage_bytes()
            <= ROW_BYTES + 48 + 16 + OVERVIEW_BYTES + 8 * (OVERVIEW_BYTES + CONTRIBUTION_LIMIT)
    );
    let builds = r.thumbnails.sources.as_ref().unwrap().builds;
    r.document_extent = [31, 29];
    thumbnail(&mut r, layer.id);
    assert_eq!(r.thumbnails.sources.as_ref().unwrap().builds, builds + 1);
    retained.clear();
    thumbnail(&mut r, layer.id);
    assert!(r.thumbnails.sources.as_ref().unwrap().cache.len() <= 3);
}

#[test]
fn incremental_photo_thumbnails_bound_work_and_restart_discarded_batches() {
    let source = photo([1025, 513]);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "background photo thumbnail");
    layer.source = Some(source.clone());
    r.ensure_document(source.extent, &[layer]).unwrap();
    let mut gpu = SourceThumbnails::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    assert!(!gpu.prepare(&mut r, LayerId(1), &mut encoder, 4).unwrap());
    assert_eq!(gpu.builds, 0);
    r.uploads.finish(&encoder);
    drop(encoder);
    r.thumbnails.sources = Some(gpu);
    let mut previous = 0;
    let mut batches = 0;
    loop {
        let before = r.scene.as_ref().unwrap().source_cache_work()[1];
        let ready = r.prepare_thumbnail_batch(LayerId(1)).unwrap();
        let cache = r.thumbnails.sources.as_ref().unwrap();
        let completed = source.tiles.len() - cache.cache.back().unwrap().remaining.len();
        assert!(completed - previous <= 4);
        assert!(r.scene.as_ref().unwrap().source_cache_work()[1] - before <= 4);
        assert_eq!(cache.builds, usize::from(ready));
        previous = completed;
        batches += 1;
        if ready {
            break;
        }
        assert!(batches < source.tiles.len());
    }
    assert_eq!(batches, source.tiles.len().div_ceil(4));
    let before = r.scene.as_ref().unwrap().source_cache_work();
    let first = thumbnail(&mut r, LayerId(1));
    assert_eq!(thumbnail(&mut r, LayerId(1)), first);
    assert_eq!(r.scene.as_ref().unwrap().source_cache_work()[1], before[1]);
    let actual = read_sums(&r);
    let expected = reference(source.extent, source.extent, None);
    let error = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (f64::from(*a) - b).abs())
        .fold(0., f64::max);
    assert!(error < 0.00002, "incremental overview linear error {error}");
}

#[test]
fn discarded_overview_commands_never_publish_source_or_overview_cache_hits() {
    let source = photo([513, 257]);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "aborted overview");
    layer.source = Some(source.clone());
    r.ensure_document(source.extent, &[layer]).unwrap();
    let mut gpu = SourceThumbnails::new(&r);
    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
    gpu.render(&mut r, LayerId(1), &mut encoder).unwrap();
    assert_eq!(gpu.builds, 1);
    let misses = r.scene.as_ref().unwrap().source_cache_work()[1];
    // Close mapped staging before discarding, as a failed frame must do.
    r.uploads.finish(&encoder);
    drop(encoder);
    r.thumbnails.sources = Some(gpu);
    thumbnail(&mut r, LayerId(1));
    assert_eq!(r.thumbnails.sources.as_ref().unwrap().builds, 2);
    assert_eq!(r.scene.as_ref().unwrap().source_cache_work()[1], misses * 2);
    let actual = read_sums(&r);
    let expected = reference(source.extent, source.extent, None);
    let error = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (f64::from(*a) - b).abs())
        .fold(0., f64::max);
    assert!(error < 0.00002, "retried overview linear error {error}");
}

#[test]
#[ignore = "physical GPU photo thumbnail latency and residency measurement"]
fn photo_thumbnail_workloads() {
    let size = std::env::var("LAYER_PHOTO_BENCH_EXTENT").unwrap_or_else(|_| "6000x4000".into());
    let (width, height) = size.split_once('x').unwrap();
    let extent = [width.parse().unwrap(), height.parse().unwrap()];
    let source = photo(extent);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let mut layer = Layer::paint(LayerId(1), "thumbnail benchmark");
    layer.source = Some(source.clone());
    r.ensure_document(extent, &[layer]).unwrap();
    let measure = |r: &mut WgpuRasterizer| {
        let start = std::time::Instant::now();
        r.start_thumbnail(1, LayerId(1)).unwrap();
        let cpu = start.elapsed().as_secs_f64() * 1000.;
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .unwrap();
        r.thumbnails.take().unwrap().unwrap();
        [cpu, start.elapsed().as_secs_f64() * 1000.]
    };
    let cold = measure(&mut r);
    println!(
        "THUMB cold extent={extent:?} cpu_ms={:.4} complete_ms={:.4}",
        cold[0], cold[1]
    );
    for count in [0, 1, 16, 64] {
        r.paint_layers[0].pages.clear();
        for i in 0..count {
            let c = [
                i % extent[0].div_ceil(PAGE_SIZE),
                i / extent[0].div_ceil(PAGE_SIZE),
            ];
            let mut page = r.create_page(c, "thumbnail benchmark paint");
            page.primary_needs_clear = false;
            r.queue.write_texture(
                page.primary.texture.as_image_copy(),
                &[255, 0, 0, 255].repeat((PAGE_SIZE * PAGE_SIZE) as usize),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(PAGE_SIZE * 4),
                    rows_per_image: None,
                },
                page.primary.texture.size(),
            );
            r.paint_layers[0].pages.push(page);
        }
        let mut times = Vec::new();
        for i in 0..120 {
            let t = measure(&mut r);
            if i >= 20 {
                times.push(t);
            }
        }
        for axis in 0..2 {
            times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
            println!(
                "THUMB warm extent={extent:?} overrides={count} kind={} p95_ms={:.4} p99_ms={:.4}",
                ["cpu", "complete"][axis],
                times[94][axis],
                times[98][axis]
            );
        }
    }
    println!(
        "THUMB storage={} source_peak_upload={} source_work={:?}",
        r.thumbnails.storage_bytes(),
        r.metrics.source_upload_peak_bytes,
        r.scene.as_ref().unwrap().source_cache_work()
    );
}

#[test]
fn failed_overview_can_be_retried_without_publishing_partial_pixels() {
    let mut bad = (*photo([513, 257])).clone();
    bad.tiles.insert(
        [2, 1],
        Arc::new(
            layer_core::raster::TileBlob::encode(
                layer_core::color::PixelDescriptor::COVERAGE8,
                &vec![255; 65536],
            )
            .unwrap(),
        ),
    );
    let bad = Arc::new(bad);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    // Deliberately bypass document preflight to exercise the decoder's failure
    // boundary after several successful uploads in this same request.
    r.document_extent = bad.extent;
    r.tiled_sources.insert(LayerId(1), bad);
    for _ in 0..2 {
        assert!(r.start_thumbnail(1, LayerId(1)).is_err());
        assert!(!r.thumbnails_pending());
        assert!(r.thumbnails.sources.as_ref().unwrap().cache.is_empty());
    }
    let source = photo([513, 257]);
    r.tiled_sources.insert(LayerId(1), source.clone());
    let image = thumbnail(&mut r, LayerId(1));
    assert_eq!(image.len(), 4096);
    let actual = read_sums(&r);
    let expected = reference(source.extent, source.extent, None);
    let error = actual
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (f64::from(*a) - b).abs())
        .fold(0., f64::max);
    assert!(error < 0.00002, "recovery after failed build: {error}");
}
