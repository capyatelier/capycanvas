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
    refused: Vec<u64>,
    peak: u64,
}

struct Memory { lane: u64, admitted: u64, strokes: u64 }

impl Canvas {
    fn photo(budget_mib: u64) -> Self { Self::photo_of(EXTENT, layer_core::color::SampleDepth::U8, budget_mib) }
    fn photo_of(extent: [u32; 2], depth: layer_core::color::SampleDepth, budget_mib: u64) -> Self {
        let photo = layer_core::color::source::rgba8_source(extent, |x, y| [
            ((3 * x + y / 8) % 251) as u8, ((5 * y + x / 8) % 241) as u8, (((x + y) / 4 + x * y % 31) % 197) as u8, 255]);
        let document = layer_color::photo_project(Arc::unwrap_or_clone(photo), Default::default(), "Photo".into(), depth).unwrap();
        let target = document.active_target().unwrap();
        let r = crate::memory::memory_tests::renderer(document.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(budget_mib << 20), ..Default::default() }).unwrap();
        let view = layer_render::ViewState { width_px: (extent[0] as f32 * FIT) as u32, height_px: (extent[1] as f32 * FIT) as u32,
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
        let mut canvas = Self { engine, input, target, presenter, shown, clock: 1_000_000_000, sequence: 0, refused: Vec::new(), peak: 0 };
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
        self.peak = self.peak.max(self.original_bytes());
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
    fn ovals(&mut self, center: [f32; 2], radius: f32, contacts: u64) -> bool {
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
                    ..crate::test_support::pen(self.sequence, phase, [view.width_px as f32 * (center[0] + radius * angle.cos()), view.height_px as f32 * (center[1] + radius * angle.sin())], SampleFlags::PRIMARY) }).unwrap();
                let refusals = self.engine.backend().stroke_memory_refusals();
                self.frame(timestamp_ns);
                for _ in 1..if held || step == STEPS { 1 } else { SAMPLE_FRAMES } {
                    if !self.engine.has_pending_input() && !self.engine.has_pending_document_edits() { break; }
                    self.frame(self.clock + FRAME_NS);
                }
                if self.engine.backend().stroke_memory_refusals() != refusals { self.refused.push(step); }
                held |= self.engine.has_pending_input() && step != STEPS;
            }
        }
        self.settle()
    }
    fn memory(&self) -> Memory {
        let r = self.engine.backend();
        let scratch = r.liquify.as_ref().map_or(0, |liquify| resident::Storage::storage_bytes(&liquify.empty)
            + liquify.scratch_map.iter().map(resident::Storage::storage_bytes).sum::<u64>() + liquify.scratch_color.iter().map(PageSurface::storage_bytes).sum::<u64>());
        let mut strokes = 0;
        for stroke in r.liquify.iter().flat_map(|liquify| liquify.strokes.values()) {
            layer_core::liquify::Field::resources(&stroke.field, &mut |_, bytes| strokes += bytes as u64);
        }
        Memory { lane: r.raster_tiers.raster.used() - scratch, admitted: r.raster_tiers.admitted.used(), strokes }
    }
    fn original_bytes(&self) -> u64 {
        let mut atlases = BTreeMap::new();
        let mut visit = |id: u64, bytes: usize| if bytes as u64 == ORIGINAL_ATLAS_BYTES { atlases.insert(id, ORIGINAL_ATLAS_BYTES); };
        for stroke in self.engine.backend().liquify.iter().flat_map(|liquify| liquify.strokes.values()) {
            layer_core::liquify::Field::resources(&stroke.field, &mut visit);
        }
        let SourceTarget::Paint(paint) = self.target else { unreachable!() };
        if let Some(state) = self.engine.document().scene().paint(paint).and_then(|source| source.raster.liquify()) {
            layer_core::liquify::Field::resources(&*state.field, &mut visit);
        }
        atlases.values().sum()
    }
    fn photo_page_bytes(&self) -> u64 {
        let data = self.engine.document().scene().raster(self.target).unwrap().wait_data().unwrap();
        self.engine.backend().paint_layers.iter().filter(|layer| layer.id == self.target).flat_map(|layer| &layer.pages)
            .filter(|page| !data.tiles.contains_key(&layer_core::raster::TileKey { plane: layer_core::raster::RasterPlane::Color, coordinate: page.coordinate }))
            .map(|page| page.primary.storage_bytes() + page.secondary.as_ref().map_or(0, |surface| surface.storage_bytes())).sum()
    }
    fn take(&mut self, bytes: u64) -> Option<crate::residency::Reservation> {
        for _ in 0..SETTLE_FRAMES {
            if let Ok(taken) = self.engine.backend_mut().admit_raster_working(bytes) { return Some(taken); }
            self.engine.backend_mut().wait_idle().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        None
    }
    fn revisions(&mut self, original: &layer_core::Artwork) -> Vec<Contents> {
        let mut revisions = vec![self.contents()];
        while self.engine.undo().unwrap() {
            assert!(self.settle());
            revisions.push(self.contents());
        }
        assert!(self.engine.document().artwork == *original, "Undo returns the photo");
        for committed in revisions.iter().rev().skip(1) {
            assert!(self.engine.redo().unwrap() && self.settle());
            assert!(self.contents() == *committed, "Redo returns each committed contact");
        }
        revisions.reverse();
        revisions
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
        assert!(canvas.ovals([0.5; 2], 1. / 3., 1), "{preset:?}: the stroke must end within {SETTLE_FRAMES} frames of its last input, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        assert_eq!(canvas.engine.backend().stroke_memory_refusals(), refusals + 1, "{preset:?}: the painter is told once that the stroke found no memory");
        let current = canvas.engine.document();
        assert!(current.revision == document.revision && current.artwork == document.artwork && current.working == document.working, "{preset:?}: the refused stroke leaves the drawing at its revision");
        let after = canvas.memory();
        assert_eq!([after.lane, after.admitted, after.strokes], [before.lane, before.admitted, before.strokes], "{preset:?}: the refused stroke returns the lane and its Liquify maps and originals");
        assert!(canvas.pixels() == shown, "{preset:?}: the canvas shows the pixels from before the refused stroke");
        canvas.brush(preset, 200.);
        assert!(canvas.ovals([0.5; 2], 1. / 8., 1), "{preset:?}: a stroke that fits must end");
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
fn back_to_back_contacts_commit_the_revisions_a_roomy_run_commits() {
    let revisions = |budget: u64| {
        let mut canvas = Canvas::photo(budget);
        canvas.brush(layer_core::DefaultBrushPreset::LiquifyPush, 740.);
        let original = canvas.engine.document().artwork.clone();
        assert!(canvas.ovals([0.5; 2], 1. / 3., 2), "{budget} MiB: both contacts must end within {SETTLE_FRAMES} frames of the last input, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        (canvas.revisions(&original), canvas.refused)
    };
    let (roomy, refused) = revisions(1024);
    assert_eq!((roomy.len(), refused), (3, Vec::new()), "a roomy run commits both contacts");
    for budget in [512, 560] {
        let (tight, refused) = revisions(budget);
        assert!(tight.len() == 2 && tight[..] == roomy[..2], "{budget} MiB: the first contact's pixels are the roomy run's");
        assert!(matches!(refused[..], [step] if step > 1), "{budget} MiB: the second contact paints past its first move before memory refuses it: {refused:?}");
    }
}

#[test]
fn a_liquify_session_holds_only_what_its_contact_reads_and_commits_what_a_roomy_run_commits() {
    const LEFT: [f32; 2] = [7. / 24., 0.5];
    const RIGHT: [f32; 2] = [17. / 24., 0.5];
    let session = |pressed: bool| {
        let mut canvas = Canvas::photo(1024);
        let original = canvas.engine.document().artwork.clone();
        canvas.brush(layer_core::DefaultBrushPreset::LoadedOil, 200.);
        assert!(canvas.ovals(LEFT, 1. / 16., 1), "the painted stroke must end");
        canvas.brush(layer_core::DefaultBrushPreset::LiquifyPush, 200.);
        let mut peaks = Vec::new();
        for center in [LEFT, RIGHT, LEFT] {
            canvas.peak = 0;
            assert!(canvas.ovals(center, 1. / 16., 1), "each Liquify contact must end");
            peaks.push(canvas.peak);
            if pressed {
                let r = canvas.engine.backend();
                let unpinned = r.raster_tiers.raster.limit() - r.raster_tiers.raster.used() + r.paint_memory_bytes() - r.pinned_paint_bytes();
                let taken = canvas.take(unpinned - (1 << 20)).expect("the lane admits the bytes no edit pins");
                assert!(canvas.photo_page_bytes() <= 1 << 20, "another edit takes the pages the session restored from the photo, short of the 1 MiB it leaves: {}", canvas.photo_page_bytes());
                canvas.engine.backend_mut().commit_raster_working(taken);
                assert!(canvas.settle());
            }
        }
        assert_eq!(canvas.refused, Vec::<u64>::new(), "every contact fits");
        (canvas.revisions(&original), peaks)
    };
    let ((roomy, kept), (pressed, held)) = (session(false), session(true));
    assert!(kept[1] > kept[0], "the right contact reads originals the left one did not: {kept:?}");
    assert!(held.iter().all(|peak| *peak <= kept[0]), "under pressure a contact holds no more originals than the first contact read, not those of the whole session: {held:?} against {kept:?}");
    assert!(roomy.len() == 5 && pressed == roomy, "contacts that rebuild their originals commit the pixels of contacts that kept them, through Undo and Redo");
}

#[test]
fn a_stroke_that_fits_keeps_its_passes_and_dispatches() {
    use crate::layer_tests::{dab, placement, preset_style};
    let frames = |scale: f32| {
        let extent = [1024, 768];
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc = placement::paint_document(extent, "Liquify passes");
        placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent, |x, y| [(x % 251) as u8, (y % 239) as u8, ((x + y) % 173) as u8, 255]));
        let target = placement::target(&doc);
        let view = layer_render::ViewState { width_px: (extent[0] as f32 * scale) as u32, height_px: (extent[1] as f32 * scale) as u32, document_to_surface: [scale, 0., 0., scale, 0., 0.] };
        crate::test_support::submit_settled(&mut r, FramePacket { view, ..crate::test_support::packet(doc.scene(), extent) });
        let mut style = preset_style(layer_core::DefaultBrushPreset::LiquifyPush);
        style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc), false)));
        const STEPS: usize = 6;
        (0..=STEPS).map(|step| {
            let mut real = dab([0.; 4]);
            real.center = layer_core::Point { x: 200. + 90. * step as f32, y: 300. + 20. * step as f32 };
            real.radii = [140.; 2];real.motion = [40., 10.];real.material[3] = 1.;
            let mut predicted = real;predicted.center.x += 45.;
            let dabs = [real, predicted];
            let mut persistent = crate::test_support::dab_batch(target, style.clone(), real.bounds());
            persistent.stroke_id = StrokeId(53);persistent.stroke_start = step == 0;persistent.stroke_end = step == STEPS;
            let mut preview = crate::test_support::dab_batch(target, style.clone(), predicted.bounds());
            preview.stroke_id = StrokeId(53);preview.kind = DabBatchKind::Preview;preview.stroke_start = false;preview.stroke_end = false;preview.first_dab = 1;
            let batches = [persistent, preview];
            if step == STEPS { placement::paint_mut(&mut doc).raster = layer_core::raster::RasterRevision::pending(); }
            let before = (r.metrics().command_passes, crate::test_support::PassCounts::snapshot());
            crate::test_support::submit_ready(&mut r, FramePacket { view, composite_all: false, dabs: &dabs[..if step == STEPS { 1 } else { 2 }], dab_batches: &batches[..if step == STEPS { 1 } else { 2 }],
                ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
            assert_eq!(r.preview_level > 0, scale < 1. && step < STEPS, "the view sets whether color waits for the stroke's end");
            [r.metrics().command_passes - before.0, crate::test_support::PassCounts::snapshot().delta(before.1).liquify_dispatches]
        }).collect::<Vec<_>>()
    };
    assert_eq!([frames(0.25), frames(1.)], [[[8, 12], [5, 12], [6, 18], [5, 16], [5, 22], [5, 26], [3, 14]], [[7, 16], [6, 16], [7, 20], [6, 16], [6, 24], [6, 20], [3, 8]]],
        "the command passes and Liquify dispatches of each frame, below full size and at it");
}

#[test]
fn a_contact_reconstructs_the_same_pixels_whatever_order_its_session_kept_its_originals_in() {
    let session = |history: bool, pressed: bool| {
        let mut canvas = Canvas::photo_of([6144, 3072], layer_core::color::SampleDepth::F32, 8192);
        canvas.brush(layer_core::DefaultBrushPreset::LiquifyPush, 740.);
        if history { assert!(canvas.ovals([1. / 24., 1. / 12.], 1. / 64., 1), "the corner contact must end, not wait on {:?}", canvas.engine.backend().outstanding_terms()); }
        assert!(canvas.ovals([0.5; 2], 1. / 4., 1), "the first wide contact must end, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        if pressed {
            let r = canvas.engine.backend();
            let unpinned = r.raster_tiers.raster.limit() - r.raster_tiers.raster.used() + r.paint_memory_bytes() - r.pinned_paint_bytes();
            let taken = canvas.take(unpinned - (1 << 20)).expect("the lane admits the bytes no edit pins");
            canvas.engine.backend_mut().commit_raster_working(taken);
            assert!(canvas.settle());
        }
        assert!(canvas.ovals([0.5 + 1. / 12., 0.5], 1. / 4., 1), "the second wide contact must end, not wait on {:?}", canvas.engine.backend().outstanding_terms());
        assert_eq!(canvas.refused, Vec::<u64>::new(), "every contact fits");
        canvas.contents()
    };
    let samples = |bytes: &[u8]| bytes.chunks_exact(4).map(|sample| f32::from_ne_bytes(sample.try_into().unwrap())).collect::<Vec<_>>();
    let roomy = session(false, false);
    for (history, pressed) in [(false, true), (true, false)] {
        let other = session(history, pressed);
        let errors = roomy.iter().flat_map(|(key, expected)| samples(expected).into_iter().zip(samples(&other[key])).map(|(a, b)| f64::from((a - b).abs()))).collect::<Vec<_>>();
        let (max, mean) = (errors.iter().copied().fold(0., f64::max), errors.iter().sum::<f64>() / errors.len() as f64);
        assert!(roomy.iter().all(|(key, expected)| other.get(key) == Some(expected)),
            "kept, rebuilt and shifted originals reconstruct the same pixels (history {history}, pressed {pressed}): max error {max:e}, mean error {mean:e} over {} samples", errors.len());
    }
}

fn fit_pen_up() -> (WgpuRasterizer, BTreeSet<[u32; 2]>, BTreeSet<[u32; 2]>) {
    use crate::layer_tests::{dab, placement, preset_style};
    let extent = [2048, 1536];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc = placement::paint_document(extent, "Liquify pen-up");
    placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent, |x, y| [(x % 251) as u8, (y % 239) as u8, ((x + y) % 173) as u8, 255]));
    let target = placement::target(&doc);
    let view = layer_render::ViewState { width_px: extent[0] / 4, height_px: extent[1] / 4, document_to_surface: [0.25, 0., 0., 0.25, 0., 0.] };
    crate::test_support::submit_settled(&mut r, FramePacket { view, ..crate::test_support::packet(doc.scene(), extent) });
    let mut style = preset_style(layer_core::DefaultBrushPreset::LiquifyPush);
    style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc), false)));
    const STEPS: usize = 5;
    let mut declared = BTreeMap::new();
    for step in 0..=STEPS {
        let mut real = dab([0.; 4]);
        real.center = layer_core::Point { x: 500. + 200. * step as f32, y: 700. + 40. * step as f32 };
        real.radii = [140.; 2];real.motion = [40., 10.];real.material[3] = 1.;
        let mut predicted = real;predicted.center.x += 45.;
        let dabs = [real, predicted];
        let mut persistent = crate::test_support::dab_batch(target, style.clone(), real.bounds());
        persistent.stroke_id = StrokeId(61);persistent.stroke_start = step == 0;persistent.stroke_end = step == STEPS;
        let mut preview = crate::test_support::dab_batch(target, style.clone(), predicted.bounds());
        preview.stroke_id = StrokeId(61);preview.kind = DabBatchKind::Preview;preview.stroke_start = false;preview.stroke_end = false;preview.first_dab = 1;
        let batches = [persistent, preview];
        if step == STEPS { placement::paint_mut(&mut doc).raster = layer_core::raster::RasterRevision::pending(); }
        let count = if step == STEPS { 1 } else { 2 };
        let packet = FramePacket { view, composite_all: false, dabs: &dabs[..count], dab_batches: &batches[..count], ..crate::test_support::packet(doc.scene(), extent) };
        let mut plans = brush_tiles::plan_packet(packet, |target| packet.scene.target_extent(target)).unwrap();
        Liquify::extend_required_plans(&r, packet, &mut plans);
        declared.clear();
        Liquify::required_sources(&r, packet, &plans, &mut declared);
        crate::test_support::submit_ready(&mut r, packet).unwrap();
    }
    let restore = r.restore_preflight.as_ref().unwrap();
    let mut read = declared.remove(&target).unwrap_or_default();
    read.extend(restore.outputs().iter().filter_map(|(at, output)| match output { raster::staging::Output::Color(c) if *at == target => Some(*c), _ => None }));
    let held = restore.dependencies().iter().filter(|(at, _)| *at == target).flat_map(|(_, coordinates)| coordinates.iter().copied()).collect();
    (r, read, held)
}

#[test]
fn a_liquify_pen_up_holds_only_the_pages_it_reads_or_writes() {
    let (_, read, held) = fit_pen_up();
    let unread = held.difference(&read).copied().collect::<Vec<_>>();
    assert!(!read.is_empty() && unread.is_empty(), "pen-up holds pages the contact neither reads nor writes: {unread:?}");
}

#[test]
fn a_finished_contact_leaves_every_original_for_other_edits_to_take() {
    let (r, ..) = fit_pen_up();
    let atlases = r.liquify.as_ref().unwrap().sessions.iter().filter_map(Weak::upgrade)
        .map(|session| session.lock().unwrap().atlases.iter().flatten().count() as u64).sum::<u64>();
    assert!(atlases > 0 && r.idle_original_bytes() == atlases * ORIGINAL_ATLAS_BYTES,
        "after pen-up no frame reads the contact's {atlases} atlases, but only {} B of them can be taken", r.idle_original_bytes());
}

fn fill_lane(r: &mut WgpuRasterizer) -> Vec<crate::residency::Reservation> {
    let mut held = Vec::new();
    loop {
        let before = held.len();
        for bytes in [64 << 20, 4 << 20, 256 << 10, 16 << 10] {
            while let Ok(taken) = r.admit_raster_working(bytes) { held.push(taken); }
        }
        r.wait_idle().unwrap();
        if held.len() == before { return held; }
    }
}

#[test]
fn a_committed_edit_captures_its_pages_while_pinned_data_fills_the_lane() {
    use crate::layer_tests::{dab, placement};
    let paint = |full: bool| {
        let extent = [2048, 1024];
        let mut doc = placement::paint_document(extent, "Capture without room");
        let mut r = crate::memory::memory_tests::renderer(doc.composition().color,
            layer_core::DeviceMemoryFacts { budget_bytes: Some(512 << 20), ..Default::default() }).unwrap();
        let target = placement::target(&doc);
        crate::test_support::submit_settled(&mut r, crate::test_support::packet(doc.scene(), extent));
        let style = crate::tests::test_style(BrushExecution::Dry);
        let stroke = |step: usize| {
            let mut stamp = dab([0.2, 0.4, 0.6, 0.8]);
            stamp.center = layer_core::Point { x: 200. + 300. * step.min(5) as f32, y: 500. };stamp.radii = [120.; 2];
            let mut batch = crate::test_support::dab_batch(target, style.clone(), stamp.bounds());
            batch.stroke_id = StrokeId(77);batch.stroke_start = step == 0;batch.stroke_end = step == 6;
            (stamp, batch)
        };
        for step in 0..6 {
            let (stamp, batch) = stroke(step);
            crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[stamp], dab_batches: &[batch], composite_all: false, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        }
        crate::test_support::submit_ready(&mut r, FramePacket { composite_all: false, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
        let held = if full { fill_lane(&mut r) } else { Vec::new() };
        placement::paint_mut(&mut doc).raster = layer_core::raster::RasterRevision::pending();
        let revision = placement::paint(&doc).raster.clone();
        let (stamp, batch) = stroke(6);
        crate::test_support::submit_ready(&mut r, FramePacket { dabs: &[stamp], dab_batches: &[batch], composite_all: false, ..crate::test_support::packet(doc.scene(), extent) })
            .expect("the committing frame submits");
        let idle = FramePacket { composite_all: false, ..crate::test_support::packet(doc.scene(), extent) };
        let captured = (0..SETTLE_FRAMES).any(|_| {
            crate::test_support::submit_ready(&mut r, idle).unwrap();
            r.wait_idle().unwrap();
            revision.try_data().is_some()
        });
        assert!(captured, "the committed pages must be captured while pinned data fills the lane (full {full})");
        drop(held);
        crate::test_support::submit_settled(&mut r, idle);
        let waits = (0..SETTLE_FRAMES).any(|_| {
            if r.next_frame() == layer_render::FrameDemand::Wait { return true; }
            crate::test_support::submit_ready(&mut r, idle).unwrap();
            r.wait_idle().unwrap();
            false
        });
        assert!(waits, "once the capture lands the renderer waits (full {full}), not on {:?}", r.outstanding_terms());
        revision.wait_data().unwrap().tiles.iter().map(|(key, tile)| (*key, tile.wait_backing().unwrap().decode_on_worker().unwrap())).collect::<Contents>()
    };
    let (pressed, roomy) = (paint(true), paint(false));
    assert!(!roomy.is_empty() && pressed == roomy, "an edit captured past a full lane stores the pixels of one captured with room");
}
