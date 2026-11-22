use super::*;
use crate::test_support::{PassCounts, packet, submit_ready, submit_settled, submitted_strokes};
use layer_core::raster::{RasterPlane, TileKey};
use layer_core::{BrushExecution, DefaultBrushPreset, Document};
use layer_engine::{CanvasEngine, InstantFeedbackConfig, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};
use std::collections::BTreeMap;

type Stroke = Vec<(Vec<Dab>, Vec<DabBatch>)>;
type Engine = CanvasEngine<WgpuRasterizer>;
type Pages = BTreeMap<[u32; 2], Vec<u8>>;
type Tiles = BTreeMap<TileKey, Vec<u8>>;

const PAGE_TEXELS: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64;
const TICK: u64 = 8_333_333;

fn wet_presets() -> Vec<DefaultBrushPreset> {
    DefaultBrushPreset::ALL.into_iter().filter(|preset| layer_core::default_brush(*preset).execution_class() == BrushExecution::Wet).collect()
}

fn renderer(document: &Document) -> WgpuRasterizer {
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    layer_render::CanvasRenderer::set_working_layer(&mut r, document.working.occurrence);
    r
}

fn settle(engine: &mut Engine, clock: &mut u64) {
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    loop {
        *clock += TICK;
        engine.render_frame_at(*clock).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() && !engine.raster_backing_pending()
            && engine.backend().next_frame() != layer_render::FrameDemand::Work { break; }
        assert!(std::time::Instant::now() < deadline, "the engine settles");
    }
}

struct Painted { engine: Engine, clock: u64, unpainted: Tiles, stroke: Stroke, counts: PassCounts }

struct Ellipse { center: [f32; 2], radii: [f32; 2], samples: u64, samples_per_frame: u64 }

fn paint(document: &Document, preset: DefaultBrushPreset, diameter: f32, zoom: f32, path: Ellipse) -> Painted {
    let Ellipse { center, radii, samples, samples_per_frame } = path;
    let extent = document.composition().size;
    let view = layer_render::ViewState {
        width_px: (extent[0] as f32 * zoom).ceil() as u32, height_px: (extent[1] as f32 * zoom).ceil() as u32,
        document_to_surface: [zoom, 0., 0., zoom, 0., 0.],
    };
    let (mut input, consumer) = input_queue(1024);
    let mut engine = CanvasEngine::new(renderer(document), document.clone(), consumer, view,
        ViewTransform { revision: 1, surface_to_document: [1. / zoom, 0., 0., 1. / zoom, 0., 0.] }).unwrap();
    engine.set_brush(layer_core::BrushSnapshot { diameter, ..layer_core::default_brush(preset) }).unwrap();
    engine.set_instant_feedback(InstantFeedbackConfig { enabled: true, ..Default::default() }).unwrap();
    let mut clock = 1_000_000_000;
    settle(&mut engine, &mut clock);
    let unpainted = committed_tiles(&engine);
    let before = PassCounts::snapshot();
    let ((), stroke) = submitted_strokes(|| {
        for step in 0..=samples {
            clock += TICK;
            let angle = step as f32 / samples as f32 * 1.125 * std::f32::consts::TAU;
            let phase = if step == 0 { PenPhase::Down } else if step == samples { PenPhase::Up } else { PenPhase::Move };
            input.push(PenEvent { timestamp_ns: clock, view_revision: 1, ..crate::test_support::pen(step + 1, phase,
                [zoom * (center[0] + radii[0] * angle.cos()), zoom * (center[1] + radii[1] * angle.sin())], SampleFlags::PRIMARY) }).unwrap();
            if step % samples_per_frame != 0 || step == samples { continue; }
            let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
            loop {
                engine.render_frame_at(clock).unwrap();
                engine.backend_mut().wait_idle().unwrap();
                if !engine.has_pending_input() && !engine.has_pending_document_edits() { break; }
                assert!(std::time::Instant::now() < deadline, "{preset:?} sample {step} is consumed");
                std::thread::yield_now();
            }
        }
        settle(&mut engine, &mut clock);
    });
    Painted { engine, clock, unpainted, stroke, counts: PassCounts::snapshot().delta(before) }
}

fn target(document: &Document) -> SourceTarget { document.active_target().unwrap() }

fn committed_tiles(engine: &Engine) -> Tiles {
    let document = engine.document();
    document.scene().raster(target(document)).unwrap().wait_data().unwrap().tiles.iter().filter(|(key, _)| key.plane == RasterPlane::Color)
        .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Replay { FramesAsSubmitted, OneBatchWithoutSparePages }

fn replay(document: &Document, stroke: &Stroke, order: Replay) -> (WgpuRasterizer, PassCounts) {
    let extent = document.composition().size;
    let frame = FramePacket { composite_all: false, commit_rasters: false, blend_space: document.composition().blend, ..packet(document.scene(), extent) };
    let mut r = renderer(document);
    submit_settled(&mut r, FramePacket { commit_rasters: true, ..frame });
    let before = PassCounts::snapshot();
    for (dabs, batches) in stroke {
        if order == Replay::FramesAsSubmitted { submit_ready(&mut r, FramePacket { dabs, dab_batches: batches, ..frame }).unwrap(); continue; }
        for batch in batches {
            drop_spare_pages(&mut r);
            submit_ready(&mut r, FramePacket { dabs, dab_batches: std::slice::from_ref(batch), ..frame }).unwrap();
        }
    }
    r.wait_idle().unwrap();
    (r, PassCounts::snapshot().delta(before))
}

fn drop_spare_pages(r: &mut WgpuRasterizer) {
    for layer in &mut r.paint_layers {
        for page in &mut layer.pages {
            let Some(mut spare) = page.secondary.take() else { continue };
            if page.active_secondary { std::mem::swap(&mut page.primary, &mut spare); }
            page.active_secondary = false;
            for coverage in layer.coverage_pages.iter().filter(|coverage| coverage.coordinate == page.coordinate) { coverage.release_color_bindings(); }
        }
    }
}

struct Canvas { color: Pages, coverage: Pages, reservoir: Vec<u8> }

fn canvas(r: &WgpuRasterizer, id: SourceTarget) -> Canvas {
    let layer = r.paint_layers.iter().find(|layer| layer.id == id).unwrap();
    let bytes = |surface: &resident::Resident<PageSurface>, version: &layer_core::raster::ContentVersion|
        crate::layer_tests::page_bytes(r, &surface.read(version, None).expect("the stroke's last write is submitted").value().texture);
    let state = r.reservoir.state.as_ref().expect("a Wet stroke leaves its reservoir");
    Canvas {
        color: layer.pages.iter().map(|page| (page.coordinate, bytes(page.active(), &r.current_raster_version(id, RasterPlane::Color, page.coordinate).unwrap()))).collect(),
        coverage: layer.coverage_pages.iter().filter_map(|page| Some((page.coordinate, bytes(page.active(), &page.state.as_ref()?.version)))).collect(),
        reservoir: bytes(if r.reservoir.active_secondary { &r.reservoir.secondary } else { &r.reservoir.primary }, &state.version),
    }
}

fn assert_same_pages(actual: &Pages, expected: &Pages, what: &str, context: &str) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>(), "{context}: {what} pages");
    for (coordinate, bytes) in actual {
        let samples = bytes.len() / PAGE_TEXELS as usize / 4;
        let differing: Vec<_> = bytes.as_chunks::<4>().0.iter().zip(expected[coordinate].as_chunks::<4>().0).enumerate().filter(|(_, (a, b))| a != b)
            .map(|(index, (a, b))| ([index / samples % PAGE_SIZE as usize, index / samples / PAGE_SIZE as usize], f32::from_le_bytes(*a), f32::from_le_bytes(*b))).collect();
        assert!(differing.is_empty(), "{context}: {} {what} samples of page {coordinate:?} differ from a whole-page evaluation, first (texel, value, whole-page value) {:?}",
            differing.len(), &differing[..differing.len().min(4)]);
    }
}

fn dispatched_whole(counts: PassCounts) -> bool { counts.transport_invocations == counts.transport_pages * PAGE_TEXELS }

fn wet_strokes_commit_what_whole_pages_commit(diameter: f32, extent: [u32; 2], samples_per_frame: u64) {
    let document = crate::scene::scale::tests::document_at(extent);
    let id = target(&document);
    let center = extent.map(|side| (side / 2) as f32);
    let radii = [(diameter * 1.2).min(620.), (diameter * 0.6).min(310.)];
    let presets = wet_presets();
    assert!(presets.len() >= 7, "{presets:?}");
    let (mut clipped_strokes, mut frames_of_several_batches) = (0, 0);
    for preset in presets {
        for zoom in [1., 0.25] {
            let context = format!("{preset:?} {diameter} px at {}%", zoom * 100.);
            let mut painted = paint(&document, preset, diameter, zoom, Ellipse { center, radii, samples: 54, samples_per_frame });
            frames_of_several_batches += painted.stroke.iter().filter(|(_, batches)| batches.len() > 1).count();
            let (clipped, counts) = replay(&document, &painted.stroke, Replay::FramesAsSubmitted);
            let (whole, reference) = replay(&document, &painted.stroke, Replay::OneBatchWithoutSparePages);
            assert!(dispatched_whole(reference), "{context}: the reference dispatches whole pages: {reference:?}");
            assert_eq!(counts.transport_pages, reference.transport_pages, "{context}: both replays run the same page jobs");
            clipped_strokes += u64::from(!dispatched_whole(counts));
            let (actual, expected) = (canvas(&clipped, id), canvas(&whole, id));
            assert_same_pages(&actual.color, &expected.color, "colour", &context);
            assert_same_pages(&actual.coverage, &expected.coverage, "stroke coverage", &context);
            assert!(actual.reservoir == expected.reservoir, "{context}: the reservoir differs from a whole-page evaluation");

            let stroked = committed_tiles(&painted.engine);
            assert!(stroked != painted.unpainted, "{context}: the stroke commits paint");
            assert!(painted.engine.undo().unwrap(), "{context}: undo");
            settle(&mut painted.engine, &mut painted.clock);
            assert!(committed_tiles(&painted.engine) == painted.unpainted, "{context}: Undo returns the layer as it was");
            assert!(painted.engine.redo().unwrap(), "{context}: redo");
            settle(&mut painted.engine, &mut painted.clock);
            assert!(committed_tiles(&painted.engine) == stroked, "{context}: Redo returns the committed stroke");
        }
    }
    assert!(clipped_strokes > 0 && frames_of_several_batches > 0,
        "{diameter} px: {clipped_strokes} strokes dispatched less than whole pages and {frames_of_several_batches} frames carried several batches");
}

#[test]
fn wet_strokes_at_15_px_commit_what_whole_pages_commit() { wet_strokes_commit_what_whole_pages_commit(15., [512, 512], 6); }

#[test]
fn wet_strokes_at_100_px_commit_what_whole_pages_commit() { wet_strokes_commit_what_whole_pages_commit(100., [512, 512], 6); }

#[test]
fn wet_strokes_at_480_px_commit_what_whole_pages_commit() { wet_strokes_commit_what_whole_pages_commit(480., [2048, 1536], 6); }

#[test]
fn wet_strokes_at_2000_px_commit_what_whole_pages_commit() { wet_strokes_commit_what_whole_pages_commit(2000., [2560, 1536], 18); }

fn jobs_damage_and_invocations_allowed(stroke: &Stroke, extent: [u32; 2]) -> [u64; 3] {
    let mut pages = BTreeMap::<[u32; 2], (u32, PixelRect)>::new();
    let (mut jobs, mut damage, mut allowed) = (0, 0, 0);
    for batch in stroke.iter().flat_map(|(_, batches)| batches) {
        let rect = batch_pixel_rect(batch, extent);
        for coordinate in page_coordinates(rect) {
            let local = rect.intersect(page_rect(coordinate)).page_local(coordinate);
            let (visits, previous) = pages.entry(coordinate).or_default();
            let area = local.union(*previous);
            let rounded = PixelRect::new(area.min_x() / 8 * 8, area.min_y() / 8 * 8, area.max_x().div_ceil(8) * 8, area.max_y().div_ceil(8) * 8);
            jobs += 1;
            damage += local.area();
            allowed += if *visits < 2 { PAGE_TEXELS } else { rounded.area() };
            (*visits, *previous) = (*visits + 1, local);
        }
    }
    [jobs, damage, allowed]
}

#[test]
fn a_wide_wet_stroke_below_full_size_dispatches_its_damage_instead_of_whole_pages() {
    let extent = [4248, 2832];
    let document = crate::scene::scale::tests::document_at(extent);
    let painted = paint(&document, DefaultBrushPreset::LoadedOil, 480., 0.25, Ellipse { center: [2124., 1416.], radii: [740., 360.], samples: 240, samples_per_frame: 8 });
    let [jobs, damage, allowed] = jobs_damage_and_invocations_allowed(&painted.stroke, extent);
    let (_, counts) = replay(&document, &painted.stroke, Replay::FramesAsSubmitted);
    let whole = counts.transport_pages * PAGE_TEXELS;
    assert_eq!(counts.transport_pages, jobs, "the replay runs the stroke's page jobs");
    assert!(counts.transport_invocations <= allowed,
        "{jobs} page jobs over {damage} damaged texels dispatched {} invocations; a page's first two jobs whole and later ones over their damage, the damage before it and workgroup rounding allow {allowed}; whole pages are {whole}",
        counts.transport_invocations);
    assert!(painted.counts.transport_invocations < painted.counts.transport_pages * PAGE_TEXELS,
        "the live stroke with its prediction dispatched whole pages: {:?}", painted.counts);
}
