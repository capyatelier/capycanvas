use super::*;
use crate::test_support::{pen, wait_until};
use layer_core::{BrushSnapshot, Document, DocumentNames, Edit, EffectInstance, EffectValue, PortableId, raster::{RasterPlane, TILE_SIZE, TileKey}};
use layer_core::authored::{EffectApplication, Occurrence, OccurrenceContent, OccurrenceHandle, RecordChange, SourceTarget, Stack};
use layer_engine::{CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
use layer_render::ViewState;

const VIEW: ViewState = ViewState { width_px: 1024, height_px: 768, document_to_surface: [1., 0., 0., 1., 0., 0.] };

fn tap(engine: &mut CanvasEngine<WgpuRasterizer>, input: &mut InputProducer<PenEvent>, revision: u64, origin: [f32; 2], at: [f32; 2]) {
    engine.set_view(VIEW, ViewTransform { revision, surface_to_document: [1., 0., 0., 1., origin[0], origin[1]] });
    for (sequence, phase) in [(0, PenPhase::Down), (1, PenPhase::Up)] {
        input.push(PenEvent { view_revision: revision, ..pen(10 * revision + sequence, phase, at, SampleFlags::PRIMARY) }).unwrap();
    }
    wait_until("the tap must commit", || {
        engine.render_frame().unwrap();
        if engine.has_pending_input() || engine.has_pending_document_edits() { Err(()) } else { Ok(()) }
    });
}

fn painted(engine: &mut CanvasEngine<WgpuRasterizer>, target: SourceTarget, tile: [u32; 2]) -> Vec<u8> {
    let key = TileKey { plane: RasterPlane::Color, coordinate: tile };
    let blob = wait_until("the painted tile must be captured", || {
        engine.render_frame().unwrap();
        match engine.document().target_raster(target).unwrap().try_data() {
            Some(data) => data.unwrap().tiles.get(&key).unwrap_or_else(|| panic!("tile {tile:?} must hold the dab")).try_backing().ok_or(()),
            None => Err(()),
        }
    }).unwrap();
    blob.decode_on_worker().unwrap()
}

fn paints_its_far_edge_like_its_origin(extent: [u32; 2], texture: u32) {
    let document = Document::new(PortableId::random(), extent[0], extent[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() }).without_paper().unwrap();
    let target = document.working.target.unwrap();
    let (mut input, consumer) = input_queue(8);
    let mut engine = CanvasEngine::new(crate::test_support::texture_limited_renderer(document.composition().color, texture), document, consumer, VIEW, ViewTransform::IDENTITY).unwrap();
    engine.set_brush(BrushSnapshot { color_rgba_linear: [0., 0., 1., 1.], diameter: 9.5, hardness: 0.5, flow: 1., mappings: Arc::from([]), ..Default::default() }).unwrap();
    let far = [extent[0] / TILE_SIZE - 1, extent[1] / TILE_SIZE - 1];
    for (revision, tile) in [(1, [0, 0]), (2, far)] {
        let center = tile.map(|t| (t * TILE_SIZE + TILE_SIZE / 2) as f32);
        tap(&mut engine, &mut input, revision, [center[0] - 300., center[1] - 200.], [300.375, 200.6875]);
    }
    let near = painted(&mut engine, target, [0, 0]);
    assert!(near.iter().any(|v| *v != 0), "the dab near the origin must paint");
    assert_eq!(painted(&mut engine, target, far), near, "a dab at the far edge must land on the same pixels as one near the origin");
}

#[test]
fn a_panorama_wider_than_the_device_texture_paints_its_far_edge_exactly() {
    paints_its_far_edge_like_its_origin([layer_core::MAX_EXTENT, 8192], 8192);
}

#[test]
fn a_strip_at_the_largest_side_paints_its_far_edge_exactly() {
    paints_its_far_edge_like_its_origin([layer_core::MAX_EXTENT, 2 * TILE_SIZE], 16384);
}

#[test]
fn painting_past_the_save_capacity_is_refused_and_the_drawing_still_saves() {
    let extent = [65536, 16384];
    let grid = extent.map(|side| side / TILE_SIZE);
    let mut artwork = layer_core::authored::Artwork::new(extent).unwrap();
    let (owner, target) = crate::test_support::add_paint(&mut artwork, "Ink", extent);
    let color = artwork.compositions.get(artwork.root).unwrap().color;
    let descriptor = color.paint_descriptor();
    let shared = layer_core::raster::RasterTile::backed(layer_core::raster::TileBlob::encode(descriptor, &vec![96; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap());
    let capacity = layer_core::ProjectLimits::default().tile_capacity();
    let room = 2;
    let mut filled = 0;
    while filled < capacity - room {
        let count = (capacity - room - filled).min((grid[0] * grid[1]) as usize);
        let SourceTarget::Paint(handle) = crate::test_support::add_paint(&mut artwork, "Filled", extent).1 else { unreachable!() };
        artwork.paint.get_mut(handle).unwrap().raster = layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
            tiles: (0..count as u32).map(|index| (TileKey { plane: RasterPlane::Color, coordinate: [index % grid[0], index / grid[0]] }, shared.clone())).collect(),
            watercolor: None });
        filled += count;
    }
    let mut document = Document::from_artwork(artwork).unwrap();
    document.working.occurrence = Some(owner);
    document.working.target = Some(target);
    assert_eq!(document.saved_tiles(), capacity - room);
    let (mut input, consumer) = input_queue(64);
    let mut engine = CanvasEngine::new(crate::test_support::texture_limited_renderer(color, 8192), document, consumer, VIEW, ViewTransform::IDENTITY).unwrap();
    engine.set_brush(BrushSnapshot { color_rgba_linear: [0., 0., 1., 1.], diameter: 24., hardness: 1., flow: 1., mappings: Arc::from([]), ..Default::default() }).unwrap();
    engine.set_view(VIEW, ViewTransform { revision: 1, surface_to_document: [1., 0., 0., 1., 0., 0.] });
    let path: Vec<_> = (0..=32).map(|step| (step, [100. + step as f32 * 30., 100.])).collect();
    for &(step, at) in &path {
        let phase = match step { 0 => PenPhase::Down, 32 => PenPhase::Up, _ => PenPhase::Move };
        input.push(PenEvent { view_revision: 1, ..pen(10 + step as u64, phase, at, SampleFlags::PRIMARY) }).unwrap();
    }
    wait_until("the stroke must commit", || {
        engine.render_frame().unwrap();
        if engine.has_pending_input() || engine.has_pending_document_edits() { Err(()) } else { Ok(()) }
    });
    assert_eq!(engine.metrics().saved_tile_refusals, 1, "the stroke reaches past the room the drawing can save");
    let painted = wait_until("the stroke must be captured", || {
        engine.render_frame().unwrap();
        engine.document().target_raster(target).unwrap().try_data().ok_or(())
    }).unwrap();
    assert!((1..=room).contains(&painted.tiles.len()), "{} new tiles", painted.tiles.len());
    assert!(engine.document().saved_tiles() <= capacity);
    let artwork = Arc::new(engine.document().artwork.clone());
    let capture = artwork.capture(layer_core::authored::CaptureCheckpoint { owner: 1, document: artwork.id, session_generation: 1, artwork_generation: 1, working_generation: 1, edit_checkpoint: 1 }).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut saved = Vec::new();
    layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancel).unwrap().write(&mut saved, &cancel).unwrap();
    assert!(!saved.is_empty());
}

fn settle(engine: &mut CanvasEngine<WgpuRasterizer>, context: &str) {
    wait_until(format_args!("{context} must settle"), || {
        engine.render_frame().unwrap_or_else(|error| panic!("{context}: {error}"));
        engine.backend_mut().wait_idle().unwrap();
        let backend = engine.backend();
        let busy = engine.has_pending_document_edits() || backend.next_frame() == layer_render::FrameDemand::Work
            || !backend.settled() || crate::scene::scale::tests::publication_pending(backend);
        if busy { Err(()) } else { Ok(()) }
    });
}

fn add_effect(engine: &mut CanvasEngine<WgpuRasterizer>, effect: EffectInstance) -> OccurrenceHandle {
    let document = engine.document();
    let artwork = &document.artwork;
    let application = RecordChange::insert(&artwork.effects, EffectApplication::new(effect.program, effect.values, document.composition().size));
    let occurrence = RecordChange::insert(&artwork.occurrences, Occurrence::new(OccurrenceContent::Effect(application.handle), "Effect"));
    let root = document.composition().result;
    let mut entries = artwork.stacks.get(root).unwrap().entries.clone();
    entries.insert(0, occurrence.handle);
    let stack = RecordChange::replace(&artwork.stacks, root, Some(Stack { entries })).unwrap();
    let layer = occurrence.handle;
    engine.apply_edit(Edit::Batch(vec![Edit::Effect(application), Edit::Occurrence(occurrence), Edit::Stack(stack)])).unwrap();
    layer
}

fn set_effect(engine: &mut CanvasEngine<WgpuRasterizer>, layer: OccurrenceHandle, key: &str, value: f32) {
    let effects = &engine.document().artwork.effects;
    let handle = engine.document().scene().effect_handle(layer).unwrap();
    let mut application = effects.get(handle).unwrap().clone();
    let mut effect = EffectInstance { program: application.program.clone(), values: application.values };
    effect.set(key, EffectValue::Number(value)).unwrap();
    application.values = effect.values;
    let edit = Edit::Effect(RecordChange::replace(effects, handle, Some(application)).unwrap());
    engine.apply_edit(edit).unwrap();
}

fn analyze(engine: &mut CanvasEngine<WgpuRasterizer>, layer: OccurrenceHandle) {
    use layer_render::CanvasRenderer;
    if engine.document().scene().effect(layer).unwrap().program.analysis().is_none() { return; }
    let query = layer_core::ArtworkQuery::new(engine.document(), layer_core::ArtworkSource::EffectInput(layer));
    assert!(engine.backend_mut().request_effect_analysis(query).unwrap());
    wait_until("the effect analysis must finish", || engine.backend_mut().take_effect_analysis().ok_or(())).unwrap();
    engine.backend_mut().accept_effect_analysis().unwrap();
}

fn displayed(engine: &mut CanvasEngine<WgpuRasterizer>, revision: u64, scale: f32, page: u32, context: &str) -> Vec<[f32; 4]> {
    let origin = ((page - 1) * TILE_SIZE) as f32;
    engine.set_view(ViewState { document_to_surface: [scale, 0., 0., scale, -origin * scale, 0.], ..VIEW },
        ViewTransform { revision, surface_to_document: [1. / scale, 0., 0., 1. / scale, origin, 0.] });
    settle(engine, context);
    let plan = engine.backend().scale_display.as_ref().unwrap().plan;
    let pixels = crate::scene::scale::tests::display_pixels(engine.backend());
    let [left, right] = [page - 1, page + 2].map(|page| ((i64::from(page * TILE_SIZE) - plan.doc_bounds.min[0]) >> plan.level) as usize);
    assert!(right <= plan.size[0] as usize, "{context}: the view at {scale} must show page {page} and its neighbours");
    pixels.chunks_exact(plan.size[0] as usize).flat_map(|row| row[left..right].iter().copied()).collect()
}

fn filters_its_far_edge_like_its_origin(id: &str, key: &str, values: [f32; 2], tolerance: f32) {
    let extent = [layer_core::MAX_EXTENT, 2 * TILE_SIZE];
    let pages = [16, extent[0] / TILE_SIZE - 16];
    let mut document = crate::artwork_sample_tests::document_in(extent, layer_core::color::RgbSpace::Srgb,
        |x, y| [x as f32 / 2550., y as f32 / 2550., if (x / 16 + y / 16) % 2 == 0 { 0.6 } else { 0.01 }, if (x / 32 + y / 48) % 3 == 0 { 0.4 } else { 1. }]);
    let paint = crate::artwork_sample_tests::paint_mut(&mut document);
    let mut data = (*paint.raster.try_data().unwrap().unwrap()).clone();
    let tile = data.tiles.remove(&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }).unwrap();
    data.tiles.extend(pages.map(|page| (TileKey { plane: RasterPlane::Color, coordinate: [page, 0] }, tile.clone())));
    paint.raster = layer_core::raster::RasterRevision::backed(data);
    let (_input, consumer) = input_queue(8);
    let renderer = crate::test_support::texture_limited_renderer(document.composition().color, 8192);
    let mut engine = CanvasEngine::new(renderer, document, consumer, VIEW, ViewTransform::IDENTITY).unwrap();
    let scales = [1., 1. / 64.];
    let mut revision = 0;
    let mut next = || { revision += 1; revision };
    let mut shown = scales.map(|scale| displayed(&mut engine, next(), scale, pages[0], "the panorama"));
    let mut effect = EffectInstance::new(crate::tests::fixture(id).program());
    effect.set(key, EffectValue::Number(values[0])).unwrap();
    let layer = add_effect(&mut engine, effect);
    analyze(&mut engine, layer);
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 { set_effect(&mut engine, layer, key, value); }
        for (scale, before) in scales.iter().zip(&mut shown) {
            let context = format!("{id} {key}={value} at {scale}");
            let near = displayed(&mut engine, next(), *scale, pages[0], &context);
            let far = displayed(&mut engine, next(), *scale, pages[1], &context);
            assert!(crate::test_support::max_error(&near, before) > 0.01, "{context} must change the picture");
            let error = crate::test_support::max_error(&near, &far);
            assert!(error <= tolerance, "{context}: the far edge differs from the same paint near the origin by {error}");
            *before = near;
        }
    }
}

#[test]
fn a_panorama_wider_than_the_device_texture_blurs_its_far_edge_like_its_origin() {
    filters_its_far_edge_like_its_origin("gaussian_blur", "sigma", [12., 100.], 5e-4);
}

#[test]
fn a_panorama_wider_than_the_device_texture_dehazes_its_far_edge_like_its_origin() {
    filters_its_far_edge_like_its_origin("dehaze", "amount", [80., 30.], 1e-5);
}

#[test]
fn a_panorama_wider_than_the_device_texture_lifts_shadows_at_its_far_edge_like_its_origin() {
    filters_its_far_edge_like_its_origin("shadows_highlights", "shadows", [80., 30.], 1e-5);
}
