use super::*;
use crate::test_support::{pen, wait_until};
use layer_core::{BrushSnapshot, Document, DocumentNames, PortableId, authored::SourceTarget, raster::{RasterPlane, TILE_SIZE, TileKey}};
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
    paints_its_far_edge_like_its_origin([65536, 16384], 8192);
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
