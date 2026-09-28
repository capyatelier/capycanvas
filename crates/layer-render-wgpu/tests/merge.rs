//! Merges bake their members into one layer: for Normal stacks the merged
//! composite equals the one before, in both blend spaces, and one undo step
//! restores the layers.
mod support;
use layer_core::*;
use layer_engine::{InputProducer, PenEvent};
use support::*;

/// Merged pixels are recomposited in another order than the display's, so
/// 8-bit captures may round one code apart.
const TOLERANCE: u8 = 1;

fn merge(engine: &mut Engine, kind: MergeKind) -> LayerId {
    let result = engine.allocate_layer_id();
    let coverage = engine.allocate_layer_id();
    let plan = engine.document().merge_plan(kind, result, coverage).unwrap();
    engine.insert_with_operations(plan.edits, vec![(plan.result, plan.operation)], None).unwrap();
    result
}

fn operate(engine: &mut Engine, id: LayerId, kind: LayerOperationKind) {
    let coverage = engine.allocate_layer_id();
    engine
        .append_layer_operation(id, LayerOperation {
            placement: Affine::IDENTITY,
            coverage: LayerMask::reveal_all(coverage, Point::default()),
            kind,
        })
        .unwrap();
}

fn gradient(engine: &mut Engine, name: &str, colors: [[f32; 4]; 2], vertical: bool) {
    let id = self::id(engine, name);
    let end = if vertical { Point { x: 0., y: SIZE[1] as f32 } } else { Point { x: SIZE[0] as f32, y: 0. } };
    operate(engine, id, LayerOperationKind::Gradient { start: Point::default(), end, colors, radial: false, alpha_locked: false });
}

fn document(names: &[&str], space: BlendSpace) -> Document {
    let mut doc = Document::new("merge", SIZE[0], SIZE[1]);
    doc.blend_space = space;
    doc.layers.remove(0);
    for (i, name) in names.iter().enumerate() {
        let id = doc.allocate_layer_id();
        doc.layers.insert(i, Layer::paint(id, *name));
    }
    doc.active_layer = doc.layers[0].id;
    doc
}

fn id(engine: &Engine, name: &str) -> LayerId {
    engine.document().layers.iter().find(|l| &*l.name == name).unwrap().id
}

fn edit(engine: &mut Engine, name: &str, change: impl FnOnce(&mut Layer)) {
    let mut layer = engine.document().layer(id(engine, name)).unwrap().clone();
    change(&mut layer);
    engine.apply_edit(Edit::ReplaceLayer(Box::new(layer))).unwrap();
}

fn stroke(engine: &mut Engine, input: &mut InputProducer<PenEvent>, name: &str, color: [f32; 4], from: Point, to: Point, start: u64) {
    engine.set_active_layer(id(engine, name)).unwrap();
    draw(engine, input, color, from, to, start);
}

/// Top to bottom: a stroke masked by a polygon at 60% opacity, a stroke over
/// a gradient, then the paper.
fn painted(space: BlendSpace) -> (Engine, InputProducer<PenEvent>) {
    let (mut engine, mut input) = engine(document(&["Upper", "Lower"], space));
    gradient(&mut engine, "Lower", [[0.9, 0.3, 0.1, 1.], [0.1, 0.6, 0.8, 0.4]], false);
    stroke(&mut engine, &mut input, "Lower", [0.1, 0.1, 0.8, 1.], Point { x: 20., y: 200. }, Point { x: 360., y: 40. }, 1_000_000_000);
    stroke(&mut engine, &mut input, "Upper", [0.9, 0.8, 0.1, 0.9], Point { x: 30., y: 30. }, Point { x: 350., y: 220. }, 2_000_000_000);
    let mask = engine.allocate_layer_id();
    edit(&mut engine, "Upper", |layer| {
        layer.opacity = 0.6;
        let mut coverage = LayerMask::reveal_all(mask, Point::default());
        coverage.default_coverage = 0.;
        coverage.initial = Some(Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 300., y: 20. },
            Point { x: 250., y: 256. },
        ]).unwrap());
        layer.mask = Some(coverage);
    });
    (engine, input)
}

#[test]
fn merge_down_keeps_the_composite_in_one_undo_step() {
    for space in BlendSpace::ALL {
        merge_down_in(space);
    }
}
fn merge_down_in(space: BlendSpace) {
    let (mut engine, _input) = painted(space);
    let original = image(&mut engine, 3_000_000_000);
    let result = merge(&mut engine, MergeKind::Down);
    let merged = image(&mut engine, 4_000_000_000);
    merged.assert_near(&original, TOLERANCE, "merged");
    let layers = &engine.document().layers;
    assert_eq!(layers.len(), 2, "the result replaces both layers");
    assert!(layers[0].id == result && layers[0].mask.is_none() && layers[0].opacity == 1.);
    let archive = {
        let mut bytes = Vec::new();
        Project::snapshot(engine.document()).unwrap().write(&mut bytes).unwrap();
        bytes
    };
    let (mut reopened, _) = support::engine(Project::read(archive.as_slice(), ProjectLimits::default()).unwrap().document);
    image(&mut reopened, 0).assert_near(&original, TOLERANCE, "saved and reopened");
    assert!(engine.undo().unwrap());
    image(&mut engine, 4_100_000_000).assert_eq(&original, "one undo step restores the layers");
    assert_eq!(engine.document().layers.len(), 3);
    assert!(engine.redo().unwrap());
    image(&mut engine, 4_200_000_000).assert_near(&original, TOLERANCE, "redo");
}

#[test]
fn a_merge_right_after_a_stroke_bakes_it_and_undo_restores_it() {
    let (mut engine, mut input) = engine(document(&["Upper", "Lower"], BlendSpace::Linear));
    stroke(&mut engine, &mut input, "Lower", [0.2, 0.7, 0.3, 1.], Point { x: 20., y: 40. }, Point { x: 360., y: 200. }, 1_000_000_000);
    let original = image(&mut engine, 1_500_000_000);
    stroke(&mut engine, &mut input, "Upper", [0.9, 0.1, 0.4, 0.8], Point { x: 30., y: 220. }, Point { x: 340., y: 30. }, 2_000_000_000);
    merge(&mut engine, MergeKind::Down);
    let merged = image(&mut engine, 3_000_000_000);
    assert!(engine.undo().unwrap());
    let restored = image(&mut engine, 3_100_000_000);
    merged.assert_near(&restored, TOLERANCE, "the merge keeps the stroke it followed");
    assert!(engine.undo().unwrap());
    image(&mut engine, 3_200_000_000).assert_eq(&original, "undoing the stroke too");
}

#[test]
fn clipping_stacks_and_groups_merge_to_the_same_composite() {
    for space in BlendSpace::ALL {
        clipping_stacks_and_groups_in(space);
    }
}
fn clipping_stacks_and_groups_in(space: BlendSpace) {
    let (mut engine, mut input) = engine(document(&["Group", "Front", "Back", "Shade", "Base"], space));
    let group = id(&engine, "Group");
    edit(&mut engine, "Group", |layer| {
        layer.kind = LayerKind::Group;
        layer.opacity = 0.7;
    });
    for name in ["Front", "Back"] {
        edit(&mut engine, name, |layer| layer.properties.parent = Some(group));
    }
    edit(&mut engine, "Shade", |layer| {
        layer.properties.clipped = true;
        layer.properties.blend = LayerBlend::Multiply;
    });
    gradient(&mut engine, "Base", [[0.8, 0.7, 0.2, 1.], [0.3, 0.2, 0.9, 0.7]], true);
    stroke(&mut engine, &mut input, "Shade", [0.2, 0.5, 0.9, 1.], Point { x: 10., y: 128. }, Point { x: 370., y: 128. }, 1_000_000_000);
    stroke(&mut engine, &mut input, "Back", [0.9, 0.1, 0.1, 1.], Point { x: 60., y: 20. }, Point { x: 60., y: 240. }, 2_000_000_000);
    stroke(&mut engine, &mut input, "Front", [0.1, 0.9, 0.2, 0.8], Point { x: 20., y: 60. }, Point { x: 300., y: 60. }, 3_000_000_000);
    let original = image(&mut engine, 4_000_000_000);
    engine.set_active_layer(id(&engine, "Base")).unwrap();
    assert_eq!(engine.document().merge_down(), MergeDown::ClippingStack);
    merge(&mut engine, MergeKind::Down);
    image(&mut engine, 5_000_000_000).assert_near(&original, TOLERANCE, "clipping stack");
    engine.set_active_layer(group).unwrap();
    merge(&mut engine, MergeKind::Group);
    image(&mut engine, 6_000_000_000).assert_near(&original, TOLERANCE, "group");
    assert_eq!(engine.document().layers.len(), 3);
    assert!(engine.undo().unwrap());
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_100_000_000).assert_eq(&original, "undo both merges");
}

#[test]
fn merge_visible_and_flatten_keep_the_composite() {
    for space in BlendSpace::ALL {
        merge_visible_and_flatten_in(space);
    }
}
fn merge_visible_and_flatten_in(space: BlendSpace) {
    let (mut engine, mut input) = painted(space);
    let hidden = engine.allocate_layer_id();
    engine.apply_edit(Edit::InsertLayer { index: 1, layer: Layer::paint(hidden, "Hidden") }).unwrap();
    stroke(&mut engine, &mut input, "Hidden", [0., 0., 0., 1.], Point { x: 0., y: 0. }, Point { x: 380., y: 250. }, 3_000_000_000);
    engine.apply_edit(Edit::SetLayerVisibility { id: hidden, visible: false }).unwrap();
    let original = image(&mut engine, 4_000_000_000);
    merge(&mut engine, MergeKind::Visible);
    image(&mut engine, 5_000_000_000).assert_near(&original, TOLERANCE, "merge visible");
    assert!(engine.document().layer(hidden).is_some(), "hidden layers stay");
    assert!(engine.undo().unwrap());
    merge(&mut engine, MergeKind::Flatten);
    image(&mut engine, 6_000_000_000).assert_near(&original, TOLERANCE, "flatten");
    assert_eq!(engine.document().layers.len(), 2, "one layer over the paper");
    assert!(engine.undo().unwrap());
    image(&mut engine, 6_100_000_000).assert_eq(&original, "undo");
}

#[test]
fn stamp_visible_adds_the_composite_of_every_visible_layer() {
    for space in BlendSpace::ALL {
        stamp_visible_in(space);
    }
}
fn stamp_visible_in(space: BlendSpace) {
    let (mut engine, _input) = painted(space);
    let original = image(&mut engine, 3_000_000_000);
    let stamp = merge(&mut engine, MergeKind::Stamp);
    image(&mut engine, 4_000_000_000);
    assert_eq!(engine.document().layers[0].id, stamp);
    for name in ["Upper", "Lower"] {
        engine.apply_edit(Edit::SetLayerVisibility { id: id(&engine, name), visible: false }).unwrap();
    }
    image(&mut engine, 5_000_000_000).assert_near(&original, TOLERANCE, "the stamp alone");
}

#[test]
fn an_effect_applies_to_the_layer_below() {
    let (mut engine, _input) = engine(document(&["Mono", "Photo", "Backdrop"], BlendSpace::Linear));
    gradient(&mut engine, "Photo", [[0.9, 0.3, 0.1, 1.], [0.1, 0.6, 0.8, 1.]], false);
    gradient(&mut engine, "Backdrop", [[0.2, 0.2, 0.2, 1.], [0.8, 0.8, 0.8, 1.]], true);
    let invert = bundled_effect_catalog().get("black_white").unwrap();
    edit(&mut engine, "Mono", |layer| {
        layer.kind = LayerKind::Effect;
        layer.effect = Some(std::sync::Arc::new(EffectInstance::new(invert.program())));
    });
    let original = image(&mut engine, 1_000_000_000);
    engine.set_active_layer(id(&engine, "Mono")).unwrap();
    assert_eq!(engine.document().merge_down(), MergeDown::ApplyEffect);
    merge(&mut engine, MergeKind::Down);
    image(&mut engine, 2_000_000_000).assert_near(&original, TOLERANCE, "the effect over an opaque layer");
    assert!(engine.document().layers.iter().all(|l| l.kind != LayerKind::Effect));
}

#[test]
fn pixels_outside_the_canvas_survive_a_merge() {
    let (mut engine, _input) = painted(BlendSpace::Linear);
    edit(&mut engine, "Upper", |layer| {
        layer.properties.offset = Point { x: -120., y: 40. };
        layer.mask = None;
    });
    let grow = CanvasGeometry::crop(CanvasRect { origin: [-200, -64], size: [SIZE[0] + 400, SIZE[1] + 128] });
    engine.apply_canvas_geometry(&grow).unwrap();
    let revealed = image(&mut engine, 3_000_000_000);
    assert!(engine.undo().unwrap());
    merge(&mut engine, MergeKind::Down);
    image(&mut engine, 4_000_000_000);
    engine.apply_canvas_geometry(&grow).unwrap();
    image(&mut engine, 5_000_000_000).assert_near(&revealed, TOLERANCE, "hidden pixels of both layers");
}

#[test]
fn placed_photos_and_watercolor_become_plain_pixels() {
    for space in BlendSpace::ALL {
        placed_photos_and_watercolor_in(space);
    }
}
fn placed_photos_and_watercolor_in(space: BlendSpace) {
    let mut doc = document(&["Wash", "Photo"], space);
    doc.layers[1].source = Some(color::source::rgba8_source([300, 200], |x, y| {
        [(x * 5 % 256) as u8, (y * 3 % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    doc.layers[1].properties.placement = Affine([0.9, 0.2, -0.2, 0.9, 40., 10.]);
    let (mut engine, mut input) = engine(doc);
    let mut brush = default_brush(DefaultBrushPreset::WetWatercolor);
    brush.diameter = 60.;
    brush.color_rgba_linear = [0.1, 0.3, 0.9, 1.];
    engine.set_active_layer(id(&engine, "Wash")).unwrap();
    engine.set_brush(brush).unwrap();
    draw_with_brush(&mut engine, &mut input, Point { x: 40., y: 60. }, Point { x: 330., y: 190. }, 1_000_000_000);
    let original = image(&mut engine, 2_000_000_000);
    let result = merge(&mut engine, MergeKind::Down);
    image(&mut engine, 3_000_000_000).assert_near(&original, TOLERANCE, "merged photo and wash");
    let layer = engine.document().layer(result).unwrap();
    assert!(layer.source.is_none() && layer.properties.placement == Affine::IDENTITY);
    let data = layer.raster.wait_data().unwrap();
    assert!(data.watercolor.is_none() && data.tiles.keys().all(|k| k.plane == raster::RasterPlane::Color), "no wet state remains");
}


/// Timing, not a gate: `cargo test --release -p layer-render-wgpu --test
/// merge -- --ignored --nocapture`.
#[test]
#[ignore = "24 MP timing report"]
fn merge_timing_on_a_24_megapixel_photo_with_ten_layers() {
    use layer_engine::{CanvasEngine, ViewTransform, input_queue};
    let extent = [6000, 4000];
    let mut doc = Document::new("24 MP merge", extent[0], extent[1]);
    doc.layers[0].source = Some(color::source::rgba8_source(extent, |x, y| {
        [(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8, 255]
    }));
    let bands: Vec<_> = (0..9).map(|_| doc.allocate_layer_id()).collect();
    for (i, id) in bands.iter().enumerate() {
        doc.layers.insert(0, Layer::paint(*id, format!("Band {i}")));
    }
    doc.active_layer = bands[8];
    let gpu = layer_render_wgpu::WgpuRasterizer::new_native_headless(doc.color).expect("physical GPU required");
    let (_producer, consumer) = input_queue(64);
    let view = layer_render::ViewState { width_px: 1600, height_px: 1000, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.], background_rgba_linear: [0.; 4] };
    let mut engine = CanvasEngine::new(gpu, doc, consumer, view, ViewTransform::IDENTITY).unwrap();
    for (i, id) in bands.iter().enumerate() {
        let y = 400. * i as f32 + 100.;
        let coverage = engine.allocate_layer_id();
        engine
            .append_layer_operation(*id, LayerOperation {
                placement: Affine::IDENTITY,
                coverage: LayerMask::reveal_all(coverage, Point::default()),
                kind: LayerOperationKind::Figure(Figure {
                    shape: FigureShape::Rectangle,
                    paint: FigurePaint::Fill,
                    start: Point { x: 200., y },
                    end: Point { x: 5800., y: y + 300. },
                    width: 1.,
                    colors: [[0.1 * i as f32, 0.8, 0.4, 0.6]; 2],
                    alpha_locked: false,
                    erase: false,
                }),
            })
            .unwrap();
    }
    let settle = |engine: &mut Engine| {
        let start = std::time::Instant::now();
        engine.render_frame_at(0).unwrap();
        while engine.has_pending_document_edits() {
            engine.render_frame_at(0).unwrap();
        }
        engine.backend_mut().wait_idle().unwrap();
        start.elapsed()
    };
    settle(&mut engine);
    for kind in [MergeKind::Visible, MergeKind::Flatten] {
        let start = std::time::Instant::now();
        let result = merge(&mut engine, kind);
        let apply = start.elapsed();
        let frame = settle(&mut engine);
        let pages = engine.document().layer(result).unwrap().raster.wait_data().unwrap().tiles.len();
        println!(
            "{kind:?}: apply {:.2} ms, bake frame to GPU idle {:.1} ms, {pages} pages",
            apply.as_secs_f64() * 1e3,
            frame.as_secs_f64() * 1e3
        );
        engine.undo().unwrap();
        settle(&mut engine);
    }
}
