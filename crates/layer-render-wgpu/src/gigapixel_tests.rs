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
