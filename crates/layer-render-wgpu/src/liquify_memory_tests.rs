use super::*;
use layer_engine::{CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ViewTransform, input_queue};

const EXTENT: [u32; 2] = [3072, 2048];
const FIT: f32 = 0.25;
const FRAME_NS: u64 = 16_666_667;
const SAMPLE_FRAMES: usize = 240;
const SETTLE_FRAMES: usize = 600;

type Contents = BTreeMap<layer_core::raster::TileKey, Vec<u8>>;

struct Canvas {
    engine: CanvasEngine<WgpuRasterizer>,
    input: InputProducer<PenEvent>,
    target: SourceTarget,
    presenter: ViewportPresenter,
    shown: wgpu::Texture,
    clock: u64,
    sequence: u64,
}

struct Memory { lane: u64, admitted: u64, strokes: u64 }

impl Canvas {
    fn photo(budget_mib: u64) -> Self {
        let photo = layer_core::color::source::rgba8_source(EXTENT, |x, y| [
            ((3 * x + y / 8) % 251) as u8, ((5 * y + x / 8) % 241) as u8, (((x + y) / 4 + x * y % 31) % 197) as u8, 255]);
        let document = layer_color::photo_project(Arc::unwrap_or_clone(photo), Default::default(), "Photo".into(), layer_core::color::SampleDepth::U8).unwrap();
        let target = document.active_target().unwrap();
        let r = crate::memory::memory_tests::renderer(document.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(budget_mib << 20), ..Default::default() }).unwrap();
        let view = layer_render::ViewState { width_px: (EXTENT[0] as f32 * FIT) as u32, height_px: (EXTENT[1] as f32 * FIT) as u32,
            document_to_surface: [FIT, 0., 0., FIT, 0., 0.] };
        let (input, consumer) = input_queue(1024);
        let engine = CanvasEngine::new(r, document, consumer, view, ViewTransform { revision: 1, surface_to_document: [1. / FIT, 0., 0., 1. / FIT, 0., 0.] }).unwrap();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let shown = engine.backend().device().create_texture(&wgpu::TextureDescriptor {
            label: Some("liquify memory presentation"), size: wgpu::Extent3d { width: view.width_px, height: view.height_px, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, view_formats: &[],
        });
        let presenter = ViewportPresenter::for_surface(engine.backend(), format, SdrSurfaceColor::Srgb).unwrap();
        let mut canvas = Self { engine, input, target, presenter, shown, clock: 1_000_000_000, sequence: 0 };
        assert!(canvas.settle(), "the photo must open");
        canvas
    }
    fn brush(&mut self, preset: layer_core::DefaultBrushPreset, diameter: f32) {
        self.engine.set_brush(layer_core::BrushSnapshot { diameter, ..layer_core::default_brush(preset) }).unwrap();
    }
    fn frame(&mut self, at: u64) {
        self.clock = at.max(self.clock + 1);
        self.engine.render_frame_at(self.clock).unwrap();
        self.presenter.present(self.engine.backend(), &self.shown.create_view(&Default::default()), self.engine.view(), [0.12, 0.12, 0.12, 1.]).unwrap();
        self.engine.backend_mut().wait_idle().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    fn idle(&self) -> bool {
        let (engine, r) = (&self.engine, self.engine.backend());
        !engine.has_pending_input() && !engine.has_pending_document_edits() && !engine.raster_backing_pending() && !engine.has_active_stroke()
            && r.settled() && engine.document().scene().raster(self.target).unwrap().host_backed() && r.has_presentable_frame() && !crate::scene::scale::tests::publication_pending(r)
    }
    fn settle(&mut self) -> bool {
        for _ in 0..SETTLE_FRAMES {
            self.frame(self.clock + FRAME_NS);
            if self.idle() { return true; }
        }
        false
    }
    fn ovals(&mut self, radius: f32, contacts: u64) -> bool {
        const STEPS: u64 = 32;
        let view = self.engine.view();
        let start_ns = self.clock.max(60_000_000_000);
        let mut held = false;
        for contact in 0..contacts {
            for step in 0..=STEPS {
                let angle = step as f32 * std::f32::consts::TAU / STEPS as f32;
                let phase = if step == 0 { PenPhase::Down } else if step == STEPS { PenPhase::Up } else { PenPhase::Move };
                self.sequence += 1;
                let timestamp_ns = start_ns + contact * 4_001_000_000 + step * 4_000_000_000 / STEPS;
                self.input.push(PenEvent { timestamp_ns, view_revision: 1, pressure: if phase == PenPhase::Up { 0. } else { 1. },
                    ..crate::test_support::pen(self.sequence, phase, [view.width_px as f32 * (0.5 + radius * angle.cos()), view.height_px as f32 * (0.5 + radius * angle.sin())], SampleFlags::PRIMARY) }).unwrap();
                self.frame(timestamp_ns);
                for _ in 1..if held || step == STEPS { 1 } else { SAMPLE_FRAMES } {
                    if !self.engine.has_pending_input() && !self.engine.has_pending_document_edits() { break; }
                    self.frame(self.clock + FRAME_NS);
                }
                held |= self.engine.has_pending_input() && step != STEPS;
            }
        }
        self.settle()
    }
    fn memory(&self) -> Memory {
        let r = self.engine.backend();
        let scratch = r.liquify.as_ref().map_or(0, |liquify| resident::Storage::storage_bytes(&liquify.empty)
            + liquify.scratch_map.iter().map(resident::Storage::storage_bytes).sum::<u64>() + liquify.scratch_color.iter().map(PageSurface::storage_bytes).sum::<u64>());
        let strokes = r.liquify.as_ref().map_or(0, |liquify| liquify.strokes.values().map(|stroke| {
            let originals = stroke.field.originals.lock().unwrap();
            stroke.field.tiles.values().map(|tile| tile.pixels.storage_bytes()).sum::<u64>() + originals.atlases.iter().map(|atlas| texture_bytes(&atlas.texture)).sum::<u64>()
        }).sum());
        Memory { lane: r.raster_tiers.raster.used() - scratch, admitted: r.raster_tiers.admitted.used(), strokes }
    }
    fn pixels(&self) -> Vec<u8> { crate::layer_tests::page_bytes(self.engine.backend(), &self.shown) }
    fn contents(&self) -> Contents {
        self.engine.document().scene().raster(self.target).unwrap().wait_data().unwrap().tiles.iter()
            .map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect()
    }
}

#[test]
fn a_stroke_refused_its_own_memory_ends_with_the_drawing_as_it_was_and_the_next_stroke_commits() {
    for preset in [layer_core::DefaultBrushPreset::LiquifyPush, layer_core::DefaultBrushPreset::LoadedOil] {
        let mut canvas = Canvas::photo(384);
        canvas.brush(preset, 740.);
        let (document, before, shown, refusals) = (canvas.engine.document().clone(), canvas.memory(), canvas.pixels(), canvas.engine.backend().stroke_memory_refusals());
        assert!(canvas.ovals(1. / 3., 1), "{preset:?}: the stroke must end within {SETTLE_FRAMES} frames of its last input, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        assert_eq!(canvas.engine.backend().stroke_memory_refusals(), refusals + 1, "{preset:?}: the painter is told once that the stroke found no memory");
        let current = canvas.engine.document();
        assert!(current.revision == document.revision && current.artwork == document.artwork && current.working == document.working, "{preset:?}: the refused stroke leaves the drawing at its revision");
        let after = canvas.memory();
        assert_eq!([after.lane, after.admitted, after.strokes], [before.lane, before.admitted, before.strokes], "{preset:?}: the refused stroke returns the lane and its Liquify maps and originals");
        assert!(canvas.pixels() == shown, "{preset:?}: the canvas shows the pixels from before the refused stroke");
        canvas.brush(preset, 200.);
        assert!(canvas.ovals(1. / 8., 1), "{preset:?}: a stroke that fits must end");
        assert_eq!((canvas.engine.document().revision, canvas.engine.backend().stroke_memory_refusals()), (document.revision + 1, refusals + 1), "{preset:?}: the next stroke commits one revision");
        let (painted, pushed) = (canvas.contents(), canvas.pixels());
        assert!(!painted.is_empty() && pushed != shown, "{preset:?}: the committed stroke changes the photo");
        assert!(canvas.engine.undo().unwrap() && canvas.settle());
        assert!(canvas.engine.document().artwork == document.artwork && canvas.pixels() == shown, "{preset:?}: Undo returns the pixels from before both strokes");
        assert!(!canvas.engine.undo().unwrap(), "{preset:?}: the refused stroke left no history");
        assert!(canvas.engine.redo().unwrap() && canvas.settle());
        assert!(canvas.contents() == painted && canvas.pixels() == pushed, "{preset:?}: Redo returns the committed stroke");
    }
}

#[test]
fn a_contact_refused_right_after_another_keeps_the_first_revision() {
    let painted = |contacts: u64| {
        let mut canvas = Canvas::photo(512);
        canvas.brush(layer_core::DefaultBrushPreset::LiquifyPush, 740.);
        let revision = canvas.engine.document().revision;
        assert!(canvas.ovals(1. / 3., contacts), "{contacts} contacts must end within {SETTLE_FRAMES} frames of the last input, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        assert_eq!((canvas.engine.document().revision, canvas.engine.backend().stroke_memory_refusals()), (revision + 1, contacts - 1), "the first contact commits and the second is refused");
        canvas.contents()
    };
    assert!(painted(2) == painted(1), "the refused second contact leaves the first contact's pixels");
}
