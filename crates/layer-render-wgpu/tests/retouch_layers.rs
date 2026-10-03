//! New Dodge & Burn Layer and Frequency Separation on the GPU: the neutral
//! layer stores the middle code and leaves the image as it was, painting on
//! it dodges and burns, and Low and High recombine into the original layer.
mod support;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth};
use layer_core::*;
use std::sync::Arc;
use support::*;

const DEPTHS: [(SampleDepth, f64); 2] = [(SampleDepth::U8, 255.), (SampleDepth::U16, 65535.)];

/// A photo with smooth gradients, fine noise and hard edges, opaque.
fn photo(depth: SampleDepth) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        SIZE,
        SourceInterpretation { channels: SourceChannels::Rgba, depth, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false },
        64 * 1024 * 1024,
    )
    .unwrap();
    let noise = |x: u32, y: u32, k: u32| ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ k.wrapping_mul(83492791)) % 1000) as f32 / 1000.;
    for y in 0..SIZE[1] {
        let mut row = Vec::new();
        for x in 0..SIZE[0] {
            let (u, v) = (x as f32 / SIZE[0] as f32, y as f32 / SIZE[1] as f32);
            let edge = if (x / 48 + y / 40) % 2 == 0 { 0.25 } else { -0.2 };
            let rgb = [0.5 + 0.4 * u - 0.3 * v + edge, 0.2 + 0.6 * v + 0.1 * noise(x, y, 1), 0.9 - 0.7 * u + 0.15 * noise(x, y, 2)];
            for value in rgb.into_iter().map(|c| c.clamp(0.02, 0.98)).chain([1.]) {
                match depth {
                    SampleDepth::U16 => row.extend_from_slice(&((value * 65535.).round() as u16).to_le_bytes()),
                    _ => row.push((value * 255.).round() as u8),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

/// A photo layer inside an offset group over the paper.
fn document(depth: SampleDepth, space: BlendSpace) -> Document {
    let mut doc = Document::new("retouch", SIZE[0], SIZE[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.color = DocumentColor { space: RgbSpace::Srgb, depth };
    doc.blend_space = space;
    let group = doc.allocate_layer_id();
    let mut folder = Layer::paint(group, "Folder");
    folder.kind = LayerKind::Group;
    folder.properties.offset = Point { x: 20., y: -12. };
    doc.layers[0].properties.parent = Some(group);
    doc.layers[0].properties.offset = Point { x: -20., y: 12. };
    doc.layers[0].source = Some(photo(depth));
    doc.layers.insert(0, folder);
    doc.active_layer = doc.layers[1].id;
    doc
}

/// The exported composite as encoded codes at the document's depth.
fn codes(engine: &mut Engine, maximum: f64) -> Vec<f64> {
    image(engine, 0);
    let mut capture = engine
        .backend()
        .snapshot_gpu()
        .capture(Project { document: engine.document().clone() }, [0.; 4], 0., Default::default())
        .unwrap();
    capture
        .read_region([0, 0, SIZE[0], SIZE[1]])
        .unwrap()
        .into_iter()
        .flat_map(|p| {
            let alpha = f64::from(p[3]);
            [p[0], p[1], p[2]].map(|c| RgbSpace::Srgb.encode(f64::from(c) / alpha) * maximum).into_iter().chain([alpha * maximum])
        })
        .collect()
}

fn assert_within(actual: &[f64], expected: &[f64], tolerance: f64, what: &str) {
    let (index, difference) = actual
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    let pixel = index / 4;
    let at = [pixel as u32 % SIZE[0], pixel as u32 / SIZE[0]];
    assert!(difference <= tolerance, "{what}: {difference:.3} codes apart at {at:?}");
}

fn insert(engine: &mut Engine, plan: RetouchLayerPlan) {
    engine.insert_with_operations(plan.edits, plan.operations, None).unwrap();
}

#[test]
fn a_dodge_and_burn_layer_stores_the_middle_code_and_leaves_the_image_unchanged() {
    for (depth, maximum) in DEPTHS {
        for space in BlendSpace::ALL {
            let (mut engine, _input) = engine(document(depth, space));
            let original = codes(&mut engine, maximum);
            let before = image(&mut engine, 0);
            let ids = std::array::from_fn(|_| engine.allocate_layer_id());
            let plan = engine.document().dodge_burn_plan(ids, "Dodge & Burn").unwrap();
            let id = plan.active;
            insert(&mut engine, plan);
            let what = format!("{depth:?} {space:?}");
            assert_within(&codes(&mut engine, maximum), &original, 1., &what);
            let data = engine.document().layer(id).unwrap().raster.wait_data().unwrap();
            assert_eq!(data.tiles.len(), 2, "{what}: the canvas's pages");
            let middle = match space {
                BlendSpace::Perceptual => (maximum + 1.) / 2.,
                BlendSpace::Linear => (RgbSpace::Srgb.encode(0.5) * maximum).round(),
            };
            for (key, tile) in &data.tiles {
                let bytes = tile.wait_backing().unwrap().decode().unwrap();
                let samples: Vec<f64> = match depth {
                    SampleDepth::U16 => bytes.chunks_exact(2).map(|b| f64::from(u16::from_le_bytes([b[0], b[1]]))).collect(),
                    _ => bytes.iter().map(|b| f64::from(*b)).collect(),
                };
                let size = raster::TILE_SIZE;
                for (i, pixel) in samples.chunks_exact(4).enumerate() {
                    let [x, y] = [key.coordinate[0] * size + i as u32 % size, key.coordinate[1] * size + i as u32 / size];
                    if x < SIZE[0] && y < SIZE[1] {
                        assert_eq!(pixel, [middle, middle, middle, maximum], "{what}: stored at {x}, {y}");
                    }
                }
            }
            assert!(engine.undo().unwrap());
            image(&mut engine, 1).assert_eq(&before, &format!("{what}: one undo step"));
        }
    }
}

#[test]
fn painting_white_on_a_dodge_and_burn_layer_dodges_and_black_burns() {
    for space in BlendSpace::ALL {
        let (mut engine, mut input) = engine(document(SampleDepth::U8, space));
        let before = image(&mut engine, 0);
        let ids = std::array::from_fn(|_| engine.allocate_layer_id());
        let plan = engine.document().dodge_burn_plan(ids, "Dodge & Burn").unwrap();
        insert(&mut engine, plan);
        draw(&mut engine, &mut input, [1., 1., 1., 0.3], Point { x: 20., y: 70. }, Point { x: 360., y: 70. }, 1_000_000_000);
        draw(&mut engine, &mut input, [0., 0., 0., 0.3], Point { x: 20., y: 190. }, Point { x: 360., y: 190. }, 2_000_000_000);
        let after = image(&mut engine, 3_000_000_000);
        let luma = |image: &Image, y: u32| {
            (40..340).map(|x| image.rgba[((y * SIZE[0] + x) * 4) as usize..][..3].iter().map(|c| f64::from(*c)).sum::<f64>()).sum::<f64>()
        };
        assert!(luma(&after, 70) > luma(&before, 70) + 300., "{space:?}: white dodges");
        assert!(luma(&after, 190) < luma(&before, 190) - 300., "{space:?}: black burns");
        assert!((luma(&after, 130) - luma(&before, 130)).abs() < 300., "{space:?}: the rest stays");
    }
}

#[test]
fn a_large_separation_yields_without_publishing_partial_rasters() {
    let extent = [1280, 768];
    let mut document = Document::new("bounded separation", extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.blend_space = BlendSpace::Perceptual;
    document.layers[0].source = Some(color::source::rgba8_source(extent, |x, y| {
        [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let (mut engine, _input) = engine(document);
    let before = image(&mut engine, 0);
    let photo = engine.document().active_layer;
    let filters = SeparationFilters::new(bundled_effect_catalog(), 21.).unwrap();
    let ids = std::array::from_fn(|_| engine.allocate_layer_id());
    let plan = engine.document().separation_plan(photo, &filters, ids, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
    let targets: Vec<_> = plan.operations.iter().map(|(id, _)| *id).collect();
    insert(&mut engine, plan);
    engine.render_frame_at(1).unwrap();
    assert!(engine.has_pending_document_edits(), "a bake must yield between bounded regions");
    assert!(!engine.can_undo(), "an incomplete bake cannot enter undo");
    for id in targets {
        assert!(engine.document().layer(id).unwrap().raster.try_data().is_none(),
            "partial pixels must not become saveable");
    }
    image(&mut engine, 2).assert_near(&before, 2, "bounded separation reconstructs the photo");
    assert!(engine.undo().unwrap());
    image(&mut engine, 3).assert_eq(&before, "bounded separation is one undo step");
    assert!(engine.redo().unwrap());
    image(&mut engine, 4).assert_near(&before, 2, "redo restores the complete separation");
}

#[test]
fn frequency_separation_recombines_into_the_original_layer() {
    for (depth, maximum) in DEPTHS {
        for radius in [2., 7.5, 21.] {
            let (mut engine, _input) = engine(document(depth, BlendSpace::Perceptual));
            let original = codes(&mut engine, maximum);
            let before = image(&mut engine, 0);
            let photo = engine.document().active_layer;
            let filters = SeparationFilters::new(bundled_effect_catalog(), radius).unwrap();
            let preview = engine.allocate_layer_id();
            engine.set_layer_preview(Some(layer_engine::LayerPreview { above: photo, layer: SeparationFilters::clipped(preview, &filters.blur, filters.blur.program.id.clone()) }));
            let previewed = image(&mut engine, 0);
            engine.set_layer_preview(None);
            image(&mut engine, 0).assert_eq(&before, "the preview leaves nothing behind");
            let ids = std::array::from_fn(|_| engine.allocate_layer_id());
            let plan = engine.document().separation_plan(photo, &filters, ids, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
            let (high, low) = (plan.active, plan.operations[0].0);
            insert(&mut engine, plan);
            let what = format!("{depth:?} radius {radius}");
            assert_within(&codes(&mut engine, maximum), &original, 2., &what);
            image(&mut engine, 1).assert_near(&before, 2, &format!("{what}: live composite"));
            let visible = |engine: &mut Engine, id: LayerId, visible: bool| {
                engine.apply_edit(Edit::SetLayerVisibility { id, visible }).unwrap();
            };
            visible(&mut engine, high, false);
            let blurred = codes(&mut engine, maximum);
            image(&mut engine, 2).assert_near(&previewed, 1, &format!("{what}: the preview shows Low"));
            let detail = |codes: &[f64]| codes.windows(5).map(|w| (w[0] - w[4]).powi(2)).sum::<f64>();
            assert!(detail(&blurred) < 0.8 * detail(&original), "{what}: Low holds less detail");
            assert!(engine.document().layer(low).unwrap().visible);
            for _ in 0..2 {
                assert!(engine.undo().unwrap());
            }
            image(&mut engine, 3).assert_eq(&before, &format!("{what}: one undo step"));
        }
    }
}

/// Timing and memory, not a gate: `cargo test --release -p
/// layer-render-wgpu --test retouch_layers -- --ignored --nocapture`. Each
/// step prints its wall-clock bounds for an external GPU memory sampler.
#[test]
#[ignore = "24 MP timing report"]
fn frequency_separation_dodge_burn_and_a_filter_merge_on_a_24_megapixel_photo() {
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    let extent = [6000, 4000];
    let mut doc = Document::new("24 MP retouch", extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.blend_space = BlendSpace::Perceptual;
    doc.layers[0].source = Some(color::source::rgba8_source(extent, |x, y| {
        [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let photo = doc.layers[0].id;
    let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(doc.color).expect("physical GPU required");
    let (_producer, consumer) = input_queue(64);
    let view = layer_render::ViewState { width_px: 1600, height_px: 1000, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.], background_rgba_linear: [0.; 4] };
    let mut engine = CanvasEngine::new(gpu, doc, consumer, view, ViewTransform::IDENTITY).unwrap();
    let settle = |engine: &mut Engine| {
        let start = std::time::Instant::now();
        engine.render_frame_at(0).unwrap();
        while engine.wants_continuous_frames() {
            assert!(start.elapsed().as_secs() < 60, "retouching did not settle");
            engine.backend_mut().wait_idle().unwrap();
            engine.render_frame_at(0).unwrap();
        }
        engine.backend_mut().wait_idle().unwrap();
        start.elapsed()
    };
    let held = |engine: &Engine| engine.backend().device().generate_allocator_report()
        .map(|report| report.allocations.iter().map(|allocation| allocation.size).sum::<u64>()).unwrap();
    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1e3;
    let initial = settle(&mut engine);
    let loaded = held(&engine);
    println!("Photo settled: {:.1} ms, {:.1} MiB allocated", ms(initial), loaded as f64 / 1048576.);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let preview = engine.allocate_layer_id();
    for radius in [4., 4.5, 21.] {
        let filters = SeparationFilters::new(bundled_effect_catalog(), radius).unwrap();
        engine.set_layer_preview(Some(layer_engine::LayerPreview { above: photo, layer: SeparationFilters::clipped(preview, &filters.blur, filters.blur.program.id.clone()) }));
        let elapsed = settle(&mut engine);
        let allocated = held(&engine);
        println!("Preview radius {radius}: frame to settled GPU {:.1} ms, {:.1} MiB above the photo", ms(elapsed), (allocated as i64 - loaded as i64) as f64 / 1048576.);
        assert!(allocated <= loaded + (200 << 20), "preview retains excessive working images");
    }
    engine.set_layer_preview(None);
    settle(&mut engine);
    for radius in [4., 21.] {
        let filters = SeparationFilters::new(bundled_effect_catalog(), radius).unwrap();
        let begin = now();
        let start = std::time::Instant::now();
        let ids = std::array::from_fn(|_| engine.allocate_layer_id());
        let plan = engine.document().separation_plan(photo, &filters, ids, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
        let low = plan.operations[0].0;
        insert(&mut engine, plan);
        let apply = start.elapsed();
        let windows = engine.backend().metrics().image_window_submissions;
        let frame = settle(&mut engine);
        let metrics = engine.backend().metrics();
        println!(
            "STEP {begin} {} Frequency Separation radius {radius}: apply {:.2} ms, bakes to GPU idle {:.1} ms, {} filter windows, peak {:.0} MiB of filter images",
            now(),
            ms(apply),
            ms(frame),
            metrics.image_window_submissions - windows,
            metrics.image_window_peak_bytes as f64 / 1048576.
        );
        let allocated = held(&engine);
        println!("Frequency Separation radius {radius}: {:.1} MiB above the photo", (allocated as i64 - loaded as i64) as f64 / 1048576.);
        assert!(allocated <= loaded + (2 * 384 + 300) * (1 << 20), "separation retains more than its pages and scratch");
        if radius == 4. {
            let ids = std::array::from_fn(|_| engine.allocate_layer_id());
            let plan = engine.document().separation_plan(low, &filters, ids, ["Frequency Separation", "Low", "High"].map(std::sync::Arc::from)).unwrap();
            insert(&mut engine, plan);
            let elapsed = settle(&mut engine);
            let second = held(&engine);
            println!("Second separation: {:.1} ms, {:.1} MiB above the first", ms(elapsed), (second as i64 - allocated as i64) as f64 / 1048576.);
            assert!(second <= allocated + (2 * 384) * (1 << 20), "another separation retains more than its own pages");
            engine.undo().unwrap();
            settle(&mut engine);
        }
        engine.undo().unwrap();
        settle(&mut engine);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let blur = engine.allocate_layer_id();
    let mut effect = EffectInstance::new(bundled_effect_catalog().get(SeparationFilters::BLUR).unwrap().program());
    effect.set("sigma", EffectValue::Number(8.)).unwrap();
    let mut filter = Layer::paint(blur, "Blur");
    filter.kind = LayerKind::Effect;
    filter.effect = Some(Arc::new(effect));
    filter.properties.clipped = true;
    engine.apply_edit(Edit::Batch(vec![Edit::InsertLayer { index: 0, layer: Box::new(filter) }, Edit::SetActiveLayer { id: blur }])).unwrap();
    settle(&mut engine);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let begin = now();
    let (result, coverage) = (engine.allocate_layer_id(), engine.allocate_layer_id());
    let plan = engine.document().merge_plan(MergeKind::Down, result, coverage).unwrap();
    let windows = engine.backend().metrics().image_window_submissions;
    engine.insert_with_operations(plan.edits, vec![(result, plan.operation)], None).unwrap();
    let frame = settle(&mut engine);
    println!(
        "STEP {begin} {} Merge Down of a clipped Gaussian Blur: bake to GPU idle {:.1} ms, {} filter windows",
        now(),
        ms(frame),
        engine.backend().metrics().image_window_submissions - windows
    );
    engine.undo().unwrap();
    engine.undo().unwrap();
    settle(&mut engine);
    let ids = std::array::from_fn(|_| engine.allocate_layer_id());
    let plan = engine.document().dodge_burn_plan(ids, "Dodge & Burn").unwrap();
    let id = plan.active;
    insert(&mut engine, plan);
    let frame = settle(&mut engine);
    let data = engine.document().layer(id).unwrap().raster.wait_data().unwrap();
    let blobs: Vec<_> = data.tiles.values().map(|t| t.wait_backing().unwrap()).collect();
    let unique: std::collections::BTreeSet<_> = blobs.iter().map(|b| b.digest).collect();
    let resident: usize = blobs.iter().map(|b| b.resident_bytes()).sum();
    println!(
        "Dodge & Burn: fill to GPU idle {:.1} ms, {} pages ({} MiB of Rgba32Float working pages), {} distinct tiles, {:.1} KiB resident compressed",
        ms(frame),
        blobs.len(),
        blobs.len() * 256 * 256 * 16 / (1024 * 1024),
        unique.len(),
        resident as f64 / 1024.
    );
}
