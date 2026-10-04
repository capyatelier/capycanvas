use super::*;
use layer_core::{ContentBoundsRequest, ContentScope, Document, Point, Rect, Selection};
use layer_core::authored::*;
use layer_core::color::{DocumentColor, SampleDepth, source::*};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey, TILE_SIZE};

#[path = "content_bounds_review_tests.rs"]
mod review;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } }
}

fn raster(plane: RasterPlane, color: DocumentColor, coverage: impl Fn(u32, u32) -> f32) -> RasterRevision {
    let descriptor = plane.descriptor(color);
    let stride = descriptor.bytes_per_pixel().unwrap();
    let size = usize::from(descriptor.bits_per_channel / 8);
    let mut bytes = vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let alpha = coverage(x, y);
            let value = match (descriptor.sample, size) {
                (layer_core::color::SampleType::Unsigned, 1) => vec![(alpha * 255.).round() as u8],
                (layer_core::color::SampleType::Unsigned, 2) => ((alpha * 65535.).round() as u16).to_le_bytes().to_vec(),
                (layer_core::color::SampleType::Float, 2) => layer_core::color::f16::from_f32(alpha).to_le_bytes().to_vec(),
                (layer_core::color::SampleType::Float, 4) => alpha.to_le_bytes().to_vec(),
                _ => panic!("unsupported coverage descriptor"),
            };
            for channel in 0..usize::from(descriptor.channels) {
                let offset = (y * TILE_SIZE + x) as usize * stride + channel * size;
                bytes[offset..offset + size].copy_from_slice(&value);
            }
        }
    }
    RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane, coordinate: [0, 0] }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))].into(),
        watercolor: None,
    })
}

fn document(color: DocumentColor) -> Document {
    let mut document = Document::new(PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
    let owner = document.working.occurrence.unwrap();
    let paper: Vec<_> = document.scene().order().iter().copied().filter(|h| *h != owner).collect();
    let root = document.composition().result;
    document.artwork.stacks.get_mut(root).unwrap().entries = vec![owner];
    for h in paper { let id = document.artwork.occurrences.id(h).unwrap(); document.artwork.occurrences.change(h, id, None).unwrap(); }
    reindex(&mut document);
    document
}

fn reindex(document: &mut Document) {
    let working = document.working.clone();
    *document = Document::from_artwork(document.artwork.clone()).unwrap();
    document.working = working;
}

fn captured(document: &Document) -> Arc<SceneSnapshot> {
    Arc::new(SceneSnapshot::new(document.artwork.clone(), Arc::new(SceneIndex::build(&document.artwork).unwrap()), document.owner, document.revision, Default::default()))
}

fn owner(document: &Document) -> OccurrenceHandle { document.working.occurrence.unwrap() }
fn occurrence(document: &mut Document) -> &mut Occurrence {
    let h = owner(document);
    document.artwork.occurrences.get_mut(h).unwrap()
}
fn paint_target(document: &Document) -> SourceTarget {
    match document.artwork.occurrences.get(owner(document)).unwrap().content {
        OccurrenceContent::Paint(h) => SourceTarget::Paint(h), _ => panic!("paint source"),
    }
}
fn paint(document: &mut Document) -> &mut PaintSource {
    let SourceTarget::Paint(h) = paint_target(document) else { unreachable!() };
    document.artwork.paint.get_mut(h).unwrap()
}
fn attach_mask(document: &mut Document, owner: OccurrenceHandle, translation: Point) -> CoverageHandle {
    let h = document.artwork.coverage.next_handle();
    let snapshot = layer_core::CoverageSnapshot::reveal_all(h, document.composition().size, translation);
    assert_eq!(document.artwork.coverage.insert(PortableId::random(), snapshot.source).unwrap(), h);
    document.artwork.occurrences.get_mut(owner).unwrap().mask = Some(snapshot.use_);
    h
}
fn wrap(document: &mut Document, child: OccurrenceHandle, name: &str) -> OccurrenceHandle {
    let containing = document.artwork.stacks.iter().find_map(|(h, _, s)| s.entries.contains(&child).then_some(h)).unwrap();
    let nested = document.artwork.stacks.insert(PortableId::random(), Stack { entries: vec![child] }).unwrap();
    let h = document.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Stack(nested), name)).unwrap();
    let entries = &mut document.artwork.stacks.get_mut(containing).unwrap().entries;
    let index = entries.iter().position(|entry| *entry == child).unwrap(); entries[index] = h;
    h
}
fn add_effect(document: &mut Document, effect: layer_core::EffectInstance, name: &str) -> OccurrenceHandle {
    let definition = document.artwork.definitions.insert(PortableId::random(), Definition { program: effect.program }).unwrap();
    let application = document.artwork.effects.insert(PortableId::random(), EffectApplication { definition, values: effect.values}).unwrap();
    let h = document.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), name)).unwrap();
    let root = document.composition().result; document.artwork.stacks.get_mut(root).unwrap().entries.insert(0, h);
    h
}

fn source(extent: [u32; 2], coverage: impl Fn(u32, u32) -> u8) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false,
    }, 8 * 1024 * 1024).unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0]).flat_map(|x| [180, 80, 40, coverage(x, y)]).collect();
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

fn bounds(renderer: &WgpuRasterizer, document: &Document, scope: ContentScope) -> Rect {
    pollster::block_on(renderer.snapshot_gpu().content_bounds(ContentBoundsRequest { snapshot: captured(document), scope, selection: document.working.selection.clone() }, Default::default())).unwrap()
}

#[test]
fn actual_alpha_bounds_include_small_positive_samples_at_every_depth() {
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
        let color = DocumentColor { depth, ..Default::default() };
        let renderer = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut document = document(color);
        let alpha = match depth { SampleDepth::U8 => 1. / 255., SampleDepth::U16 => 1. / 65535., _ => 1. / 65536. };
        paint(&mut document).raster = raster(RasterPlane::Color, color, |x, y| if (x == 17 && y == 23) || (x == 61 && y == 47) { alpha } else { 0. });
        assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(17., 23., 62., 48.), "{depth:?}");
        assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(17., 23., 62., 48.), "{depth:?}");
    }
}

#[test]
fn source_padding_and_fully_erased_override_do_not_count() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    let imported = source([96, 80], |x, y| if (19..41).contains(&x) && (27..53).contains(&y) { 255 } else { 0 });
    { let paint = paint(&mut document); paint.domain = imported.extent; paint.original = Some(imported); }
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(19., 27., 41., 53.));
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |_, _| 0.);
    assert!(bounds(&renderer, &document, ContentScope::All).is_empty());
    assert!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))).is_empty());

    let imported = source([320, 80], |x, y| if (19..281).contains(&x) && (27..53).contains(&y) { 255 } else { 0 });
    { let paint = paint(&mut document); paint.domain = imported.extent; paint.original = Some(imported); }
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(256., 27., 281., 53.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(256., 27., 281., 53.));
}

#[test]
fn mask_products_use_coverage_instead_of_intersecting_rectangles() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if y == 20 && (x == 20 || x == 40) { 1. } else { 0. });
    let h = owner(&document);
    let mask = attach_mask(&mut document, h, Point::default());
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
    document.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, document.composition().color, |x, y| if y == 20 && (x == 21 || x == 39) { 1. } else { 0. });
    assert!(bounds(&renderer, &document, ContentScope::Visible).is_empty());
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(20., 20., 41., 21.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(20., 20., 41., 21.));
}

#[test]
fn inverted_selection_intersects_actual_target_pixels_in_local_coordinates() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    occurrence(&mut document).translation = Point { x: 10., y: 12. };
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if y == 20 && (x == 20 || x == 40) { 1. } else { 0. });
    let mut selection = Selection::polygon(rect(29., 31., 32., 34.).corners().to_vec()).unwrap();
    selection.inverted = true;
    document.working.selection = Some(selection);
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(40., 20., 41., 21.));
    document.working.selection.as_mut().unwrap().inverted = false;
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(20., 20., 21., 21.));
}

#[test]
fn nested_group_target_selection_retains_absolute_registration() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    occurrence(&mut document).translation = Point { x: 10., y: 10. };
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if y == 20 && (x == 20 || x == 40) { 1. } else { 0. });
    let target = paint_target(&document);
    let h = owner(&document);
    let inner = wrap(&mut document, h, "inner");
    let o = document.artwork.occurrences.get_mut(inner).unwrap(); o.translation = Point { x: 20., y: 20. }; o.visible = false;
    let outer = wrap(&mut document, inner, "outer");
    document.artwork.occurrences.get_mut(outer).unwrap().translation = Point { x: 30., y: 30. };
    reindex(&mut document);
    document.working.selection = Some(Selection::polygon(rect(79., 79., 82., 82.).corners().to_vec()).unwrap());
    let request = ContentBoundsRequest::new(&document, ContentScope::Target(target));
    assert_eq!(request.scope, ContentScope::Target(target));
    assert_eq!(request.snapshot.view().source_owner(target), Some(owner(&document)));
    assert_eq!(request.snapshot.view().order().len(), 3);
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(target)), rect(20., 20., 21., 21.));
}

#[test]
fn hidden_offcanvas_content_counts_for_all_and_local_target_only() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if (20..40).contains(&x) && (30..50).contains(&y) { 1. } else { 0. });
    occurrence(&mut document).translation = Point { x: -30., y: -40. };
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(-10., -10., 10., 10.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Canvas), rect(0., 0., 10., 10.));
    occurrence(&mut document).visible = false;
    occurrence(&mut document).opacity = 0.;
    assert!(bounds(&renderer, &document, ContentScope::Visible).is_empty());
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(-10., -10., 10., 10.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))), rect(20., 30., 40., 50.));
}

#[test]
fn immutable_request_and_cancelled_capture_do_not_publish_changed_inputs() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if x == 3 && y == 7 { 1. } else { 0. });
    let request = ContentBoundsRequest::new(&document, ContentScope::All);
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |_, _| 0.);
    assert_eq!(pollster::block_on(renderer.snapshot_gpu().content_bounds(request.clone(), Default::default())).unwrap(), rect(3., 7., 4., 8.));
    let control = crate::snapshot::CaptureControl::default();
    control.cancel();
    assert!(pollster::block_on(renderer.snapshot_gpu().content_bounds(request, control)).is_err());
    assert!(bounds(&renderer, &document, ContentScope::All).is_empty());
}

#[test]
fn group_mask_products_and_paper_are_evaluated_at_document_coordinates() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if y == 20 && (x == 20 || x == 40) { 1. } else { 0. });
    let h = owner(&document);
    let group = wrap(&mut document, h, "group");
    let mask = attach_mask(&mut document, group, Point::default());
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
    document.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, document.composition().color, |x, y| if y == 20 && (x == 21 || x == 39) { 1. } else { 0. });
    assert!(bounds(&renderer, &document, ContentScope::Visible).is_empty());
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(20., 20., 41., 21.));
    let paper = Document::new(PortableId::random(), 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    assert_eq!(bounds(&renderer, &paper, ContentScope::Visible), rect(0., 0., 128., 96.));
}

#[test]
fn inverted_mask_target_uses_actual_coverage_and_finite_extent() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    let h = owner(&document);
    let mask = attach_mask(&mut document, h, Point::default());
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 1.;
    occurrence(&mut document).mask.as_mut().unwrap().inverted = true;
    document.artwork.coverage.get_mut(mask).unwrap().raster = raster(RasterPlane::Mask, document.composition().color, |x, y| if (25..35).contains(&x) && (40..60).contains(&y) { 0. } else { 1. });
    let target = SourceTarget::Coverage(mask);
    assert_eq!(bounds(&renderer, &document, ContentScope::Target(target)), rect(25., 40., 35., 60.));
}

#[test]
fn wetness_pages_without_color_do_not_create_content_bounds() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    let mut data = raster(RasterPlane::WatercolorWetness, document.composition().color, |_, _| 1.).wait_data().unwrap().as_ref().clone();
    data.watercolor = Some(layer_core::raster::RasterWatercolor { wet_edge: 0.5, burnt_edge: 0.5, edge_width: 2. });
    paint(&mut document).raster = RasterRevision::backed(data);
    assert!(bounds(&renderer, &document, ContentScope::All).is_empty());
    assert!(bounds(&renderer, &document, ContentScope::Target(paint_target(&document))).is_empty());
}

#[test]
fn transparent_offcanvas_photo_does_not_expand_paper_coverage() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = Document::new(PortableId::random(), 128, 96, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    paint(&mut document).original = Some(source([96, 80], |_, _| 0));
    occurrence(&mut document).translation = Point { x: -80., y: -70. };
    assert_eq!(bounds(&renderer, &document, ContentScope::All), rect(0., 0., 128., 96.));
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(0., 0., 128., 96.));
    let paper = document.scene().order()[1];
    crate::tests::native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
        layer_core::color::RgbColor { rgba: [1., 1., 1., 0.], ..layer_core::color::RgbColor::WHITE }));
    for scope in [ContentScope::Canvas, ContentScope::Visible, ContentScope::All] {
        assert!(bounds(&renderer, &document, scope).is_empty());
    }
    crate::tests::native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
        layer_core::color::RgbColor { rgba: [1., 1., 1., 0.001], ..layer_core::color::RgbColor::WHITE }));
    for scope in [ContentScope::Canvas, ContentScope::Visible, ContentScope::All] {
        assert_eq!(bounds(&renderer, &document, scope), rect(0., 0., 128., 96.));
    }
}

#[test]
fn renderer_cancellation_discards_the_old_result_before_replacement() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if x == 3 && y == 7 { 1. } else { 0. });
    assert!(renderer.request_content_bounds(ContentBoundsRequest::new(&document, ContentScope::All)).unwrap());
    renderer.cancel_content_bounds();
    assert!(renderer.take_content_bounds().is_none());
    paint(&mut document).raster = raster(RasterPlane::Color, document.composition().color, |x, y| if x == 55 && y == 63 { 1. } else { 0. });
    assert!(renderer.request_content_bounds(ContentBoundsRequest::new(&document, ContentScope::All)).unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let result = loop {
        if let Some(result) = renderer.take_content_bounds() { break result.unwrap(); }
        assert!(std::time::Instant::now() < deadline, "replacement bounds complete");
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    assert_eq!(result, rect(55., 63., 56., 64.));
    assert!(renderer.take_content_bounds().is_none());
}

#[test]
fn linked_and_unlinked_initial_mask_coverage_uses_the_owner_geometry_once() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).original = Some(source([96, 80], |_, _| 255));
    occurrence(&mut document).translation = Point { x: 10., y: 10. };
    occurrence(&mut document).placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([2., 0., 0., 2., 0., 0.]));
    let h = owner(&document);
    let mask = attach_mask(&mut document, h, Point { x: 10., y: 10. });
    document.artwork.coverage.get_mut(mask).unwrap().default_coverage = 0.;
    document.artwork.coverage.get_mut(mask).unwrap().initial = Some(Selection::polygon(rect(20., 20., 30., 30.).corners().to_vec()).unwrap());
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(49., 49., 71., 71.));
    let mask = occurrence(&mut document).mask.as_mut().unwrap();
    mask.linked = false;
    mask.translation = Point::default();
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), rect(20., 20., 30., 30.));
}

#[test]
fn visible_filter_halos_extend_past_the_local_source_hull() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    paint(&mut document).original = Some(source([16, 16], |_, _| 255));
    occurrence(&mut document).translation = Point { x: 20., y: 20. };
    let mut effect = layer_core::EffectInstance::new(crate::tests::fixture("gaussian_blur").program());
    effect.set("sigma", layer_core::EffectValue::Number(2.)).unwrap();
    add_effect(&mut document, effect, "blur");
    let canvas = bounds(&renderer, &document, ContentScope::Canvas);
    assert!(canvas.min.x < 20. && canvas.min.y < 20. && canvas.max.x > 36. && canvas.max.y > 36., "blur extends actual alpha outside the original source: {canvas:?}");
    assert_eq!(bounds(&renderer, &document, ContentScope::Visible), canvas);
}

#[test]
#[ignore = "reference tablet bounds timing"]
fn content_bounds_timing_on_reference_canvas() {
    let tier = std::env::var("CAPY_BOUNDS_TIMING_TIER").expect("CAPY_BOUNDS_TIMING_TIER");
    let extent = match tier.as_str() {
        "low" => [4248, 2832], "mid" => [6000, 4000], "top" => [9504, 6336],
        _ => panic!("unknown bounds timing tier"),
    };
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = document(Default::default());
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().size = extent;
    paint(&mut document).domain = extent;
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false,
    }, 512 * 1024 * 1024).unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0]).flat_map(|x| [180, 80, 40,
            if x >= 10 && y >= 10 && x < extent[0] - 10 && y < extent[1] - 10 { 255 } else { 0 }]).collect();
        builder.push_row(&row).unwrap();
    }
    paint(&mut document).original = Some(Arc::new(builder.finish().unwrap()));
    for scope in [ContentScope::Target(paint_target(&document)), ContentScope::Visible] {
        for run in 0..4 {
            let start = std::time::Instant::now();
            let actual = bounds(&renderer, &document, scope);
            let elapsed = start.elapsed();
            assert_eq!(actual, rect(10., 10., extent[0] as f32 - 10., extent[1] as f32 - 10.));
            println!("bounds tier={tier} canvas={}x{} scope={scope:?} run={run} ms={:.3}", extent[0], extent[1], elapsed.as_secs_f64() * 1000.);
        }
    }
}
