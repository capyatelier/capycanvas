use super::*;
use layer_render::{CanvasRenderer, OperationUse, PreparationContext, PreparationUses, RegionRequest, RegionSource, RegionTask};
use std::collections::HashSet;

fn fixture(test: impl FnOnce(&mut WgpuRasterizer, &mut Document, &BrushSnapshot, compile::Batch)) {
    {
        let mut reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut document = Document::new(layer_core::PortableId::random(), 64, 64, layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        crate::test_support::submit_ready(&mut reference,
            crate::test_support::packet(document.scene(), [64; 2])).unwrap();
        let mut renderer = crate::test_support::staged_renderer(&reference, Default::default());
        renderer.artwork_frame = reference.artwork_frame.clone();
        renderer.document_extent = [64; 2];
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        brush.diameter = 9.;
        renderer.shader_idle(true, false);
        let batch = renderer.compilation.batch();
        renderer.prepare_startup(&document, &brush, false).unwrap();
        test(&mut renderer, &mut document, &brush, batch);
    }
}

fn identities(needs: &Needs) -> HashSet<usize> {
    needs.render.iter().filter(|recipe| !recipe.ready()).map(Deferred::identity)
        .chain(needs.compute.iter().filter(|recipe| !recipe.ready()).map(Deferred::identity)).collect()
}

fn demanded(renderer: &WgpuRasterizer, class: Class) -> HashSet<usize> {
    compile::tests::demands(&renderer.context_compilation, class)
}

fn request(fill: bool, refinement: layer_render::RegionRefinement) -> RegionRequest {
    RegionRequest { fill_areas: None, colour_mode: Default::default(),
        fill: fill.then(Default::default), reference_count: None, paper: None,
        enclosure: None, contiguous: true, selection: None, request_id: 1,
        source: RegionSource::Composite, position: [0; 2], tolerance: 0.,
        refinement, limit: None }
}

#[test]
fn context_fill_and_region_declarations_cover_real_requests() {
    for fill in [false, true] {
        fixture(|renderer, document, brush, batch| {
            let refinement = layer_render::RegionRefinement { close_gaps: true, ..Default::default() };
            renderer.prepare_context(document, brush, PreparationContext {
                next: PreparationUses { fill, ..Default::default() }, regions: refinement,
                ..Default::default()
            });
            let declared = demanded(renderer, BACKGROUND);
            let before = compile::tests::demands(&renderer.compilation, DOCUMENT);
            let request = request(fill, refinement);
            let task = if fill {
                RegionTask::Fill { request, target: document.drawing_content().unwrap(),
                    position: layer_core::Point { x: 0., y: 0. } }
            } else { RegionTask::Mask(request) };
            assert!(renderer.request_region(task).unwrap());
            assert!(renderer.take_region().is_none());
            let after = compile::tests::demands(&renderer.compilation, DOCUMENT);
            let required: HashSet<_> = after.difference(&before).copied().collect();
            assert!(!required.is_empty(), "cold request must declare its real dependencies");
            assert!(required.is_subset(&declared), "undeclared request recipes: {:?}",
                required.difference(&declared).collect::<Vec<_>>());
            drop(batch);
        });
    }
}

fn selection_contact() -> layer_render::SelectionPaint {
    let mut contact = crate::layer_tests::dab([1.; 4]);
    contact.radii = [12.; 2]; contact.flow = 1.; contact.hardness = 1.;
    let update = layer_render::SelectionPaint {
        id: 1, before: Arc::new(layer_core::Selection::empty()),
        mode: layer_render::SelectionPaintMode::Add, opacity: 1., gray: 0.5,
        style: crate::tests::test_style(BrushExecution::Dry), gradient: None,
        dabs: vec![contact], enclosed: None, finish: false, restart: false,
    };
    update
}

#[test]
fn context_selection_paint_declaration_covers_real_first_contact() {
    fixture(|renderer, document, brush, batch| {
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { selection_paint: true, ..Default::default() },
            ..Default::default()
        });
        let declared = demanded(renderer, ANTICIPATED);
        let before = compile::tests::demands(&renderer.compilation, DOCUMENT);
        let update = selection_contact();
        assert!(!renderer.paint_selection(&update).unwrap());
        let after = compile::tests::demands(&renderer.compilation, DOCUMENT);
        let required: HashSet<_> = after.difference(&before).copied().collect();
        assert!(!required.is_empty());
        assert!(required.is_subset(&declared));
        drop(batch);
    });
}

fn operation_batch(document: &mut Document, operation: OperationUse) -> DabBatch {
    let target = document.drawing_target().unwrap();
    crate::layer_tests::placement::paint_mut(document).operations = Arc::new(vec![layer_core::RasterOperation {
        placement: layer_core::Affine::IDENTITY,
        coverage: crate::layer_tests::placement::reveal_all([64; 2], [0; 2]),
        kind: match operation { OperationUse::Gradient => layer_core::RasterOperationKind::Gradient {
            texture: Default::default(), start: layer_core::Point { x: 0., y: 0. },
            end: layer_core::Point { x: 64., y: 0. }, gradient: Default::default(),
            shape: layer_core::GradientShape::Linear, reverse: false, opacity: 1., alpha_locked: false,
        }, OperationUse::Figure => layer_core::RasterOperationKind::Figure(layer_core::Figure {
            shape: layer_core::FigureShape::Rectangle, paint: layer_core::FigurePaint::Fill,
            start: layer_core::Point {x:0.,y:0.}, end: layer_core::Point {x:64.,y:64.},
            width: 1., colors: [[1.;4];2], alpha_locked: false, erase: false,
        }) },
    }]);
    DabBatch { kind: DabBatchKind::RasterOperation(0), dab_count: 0,
        ..crate::test_support::dab_batch(target,
            brush_style(document, &layer_core::default_brush(layer_core::DefaultBrushPreset::GPen), StrokeTool::Brush),
            layer_core::Rect::from_extent([64; 2])) }
}

#[test]
fn context_operation_declaration_covers_real_batches() {
    for operation in [OperationUse::Gradient, OperationUse::Figure] {
    fixture(|renderer, document, brush, batch| {
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { operation: Some(operation), ..Default::default() },
            ..Default::default()
        });
        let declared = demanded(renderer, BACKGROUND);
        let before = compile::tests::demands(&renderer.compilation, DOCUMENT);
        let batches = [operation_batch(document, operation)];
        assert!(!renderer.brush_batches_ready(FramePacket { dab_batches: &batches,
            ..crate::test_support::packet(document.scene(), [64; 2]) }));
        let after = compile::tests::demands(&renderer.compilation, DOCUMENT);
        let required: HashSet<_> = after.difference(&before).copied().collect();
        assert!(!required.is_empty());
        assert!(required.is_subset(&declared));
        drop(batch);
    });
    }
}
#[test]
fn context_background_is_exactly_the_idle_family_union() {
    fixture(|renderer, document, brush, batch| {
        let shown = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let context = PreparationContext { brushes: vec![shown.clone()], ..Default::default() };
        let mut families: [Needs; 6] = std::array::from_fn(|_| Needs::default());
        families[0].sizes(renderer, document, brush, None);
        families[1].brush(renderer, document, &shown, None);
        families[2].transform(renderer);
        families[3].selection_paint(renderer);
        families[4].fill(renderer, Some(document.scene()));
        families[5].regions(renderer, context.regions, false, true, true);
        let expected: HashSet<_> = families.iter().flat_map(identities).collect();
        renderer.prepare_context(document, brush, context.clone());
        assert!(demanded(renderer, ANTICIPATED).is_empty());
        assert_eq!(demanded(renderer, BACKGROUND), expected);
        let actual = compile::tests::ordered_demands(&renderer.context_compilation, BACKGROUND);
        let queued: HashSet<_> = actual.iter().copied().collect();
        let mut seen = HashSet::new();
        let order: Vec<_> = families.iter().flat_map(|needs| needs.render.iter().map(Deferred::identity)
            .chain(needs.compute.iter().map(Deferred::identity)))
            .filter(|id| queued.contains(id) && seen.insert(*id)).collect();
        assert!(!order.is_empty());
        assert_eq!(actual, order, "Background families keep the declared FIFO order");
        let count = compile::tests::queued_recipes(&renderer.context_compilation);
        renderer.prepare_context(document, brush, context);
        assert_eq!(demanded(renderer, BACKGROUND), expected);
        assert_eq!(compile::tests::queued_recipes(&renderer.context_compilation), count);
        drop(batch);
    });
}

#[test]
fn selection_publication_introduces_no_recipes_after_context_warming() {
    for presets in [
        vec![layer_core::DefaultBrushPreset::Smudge, layer_core::DefaultBrushPreset::Eraser],
        vec![layer_core::DefaultBrushPreset::PointyPencil],
    ] {
        fixture(|renderer, document, brush, batch| {
            let mut context = PreparationContext {
                next: PreparationUses { selection_strokes: true, ..Default::default() },
                brushes: presets.into_iter().map(layer_core::default_brush).collect(),
                ..Default::default()
            };
            renderer.prepare_context(document, brush, context.clone());
            drop(batch);
            renderer.shader_idle(true, true);
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            loop {
                let progress = renderer.poll_startup().unwrap();
                if progress.brush_ready
                    && compile::tests::queued_recipes(&renderer.context_compilation) == 0
                    && compile::tests::queued_recipes(&renderer.startup.as_ref().unwrap().compiler) == 0 {
                    break;
                }
                assert!(std::time::Instant::now() < deadline, "selection context did not finish warming");
                std::thread::sleep(Duration::from_millis(1));
            }
            renderer.shader_idle(true, false);
            let paused = renderer.compilation.batch();
            document.working.selection = Some(layer_core::Selection::polygon(vec![
                layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. },
                layer_core::Point { x: 64., y: 64. }, layer_core::Point { x: 0., y: 64. },
            ]).unwrap());
            document.revision += 1;
            renderer.prepare_startup(document, brush, false).unwrap();
            context.next = PreparationUses { fill: true, transform: true, ..Default::default() };
            renderer.prepare_context(document, brush, context);
            let introduced: HashSet<_> = [&renderer.compilation, &renderer.context_compilation,
                &renderer.startup.as_ref().unwrap().compiler].into_iter()
                .flat_map(|scope| compile::tests::demands(scope, BACKGROUND)).collect();
            assert!(introduced.is_empty(), "selection publication introduced recipes after warming: {introduced:?}");
            drop(paused);
        });
    }
    crate::finish_shader_compiler_shutdown();
}

#[test]
fn predicted_selection_keeps_future_sizes_and_toolbar_brushes_in_background() {
    fixture(|renderer, document, brush, batch| {
        let selected = Arc::new(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. },
            layer_core::Point { x: 64., y: 64. }, layer_core::Point { x: 0., y: 64. },
        ]).unwrap());
        let shown = layer_core::default_brush(layer_core::DefaultBrushPreset::PointyPencil);
        let mut anticipated = Needs::default();
        anticipated.brush(renderer, document, brush, Some(selected.clone()));
        anticipated.selection_paint(renderer);
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { selection_strokes: true, selection_paint: true, ..Default::default() },
            brushes: vec![shown.clone()], ..Default::default()
        });
        let actual = compile::tests::ordered_demands(&renderer.context_compilation, ANTICIPATED);
        let queued: HashSet<_> = actual.iter().copied().collect();
        let mut seen = HashSet::new();
        let expected: Vec<_> = anticipated.render.iter().map(Deferred::identity)
            .chain(anticipated.compute.iter().map(Deferred::identity))
            .filter(|id| queued.contains(id) && seen.insert(*id)).collect();
        assert_eq!(actual, expected, "selected Brush/Eraser precede selection-paint anticipation");
        document.working.selection = Some((*selected).clone());
        let mut current = Needs::default();
        current.brush(renderer, document, brush, None);
        current.sizes(renderer, document, brush, None);
        let mut toolbar = Needs::default();
        toolbar.brush(renderer, document, &shown, None);
        assert!(!identities(&toolbar).is_subset(&identities(&current)),
            "visible Pencil-family brush must contribute a distinct selected recipe");
        let mut future = Needs::default();
        future.sizes(renderer, document, brush, None);
        future.brush(renderer, document, &shown, None);
        let future: HashSet<_> = identities(&future).difference(&identities(&anticipated)).copied().collect();
        assert!(!future.is_empty());
        let background: HashSet<_> = compile::tests::ordered_demands(&renderer.context_compilation, BACKGROUND).into_iter().collect();
        assert!(future.is_subset(&background), "predicted selected future recipes must stay Background: {:?}", future.difference(&background).collect::<Vec<_>>());
        drop(batch);
    });
    crate::finish_shader_compiler_shutdown();
}

#[test]
fn context_replacement_cancels_optional_demands_but_keeps_required_promotions() {
    fixture(|renderer, document, brush, batch| {
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { operation: Some(OperationUse::Gradient), ..Default::default() },
            ..Default::default()
        });
        let anticipated = demanded(renderer, ANTICIPATED);
        assert!(!anticipated.is_empty());
        let batches = [operation_batch(document, OperationUse::Gradient)];
        assert!(!renderer.brush_batches_ready(FramePacket { dab_batches: &batches,
            ..crate::test_support::packet(document.scene(), [64; 2]) }));
        let required = compile::tests::demands(&renderer.compilation, DOCUMENT);
        assert!(anticipated.is_subset(&required));
        crate::layer_tests::placement::paint_mut(document).operations = Arc::new(Vec::new());
        renderer.prepare_context(document, brush, PreparationContext::default());
        assert!(demanded(renderer, ANTICIPATED).is_empty());
        assert!(anticipated.is_disjoint(&demanded(renderer, BACKGROUND)));
        assert!(required.is_subset(&compile::tests::demands(&renderer.compilation, DOCUMENT)));
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { selection_paint: true, ..Default::default() }, ..Default::default()
        });
        let paint = demanded(renderer, ANTICIPATED);
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { fill: true, ..Default::default() }, ..Default::default()
        });
        let fill = demanded(renderer, ANTICIPATED);
        let background: HashSet<_> = compile::tests::ordered_demands(&renderer.context_compilation, BACKGROUND).into_iter().collect();
        let paint_only: HashSet<_> = paint.difference(&fill).copied().collect();
        assert!(!paint_only.is_empty());
        assert!(paint_only.is_subset(&background), "retired anticipation keeps its shared Background demand");
        assert!(!compile::tests::ordered_demands(&renderer.context_compilation, ANTICIPATED).is_empty(), "the new family promotes ahead of Background");
        drop(batch);
    });
}

#[test]
fn context_selection_stroke_declaration_covers_selected_brush_startup() {
    fixture(|renderer, document, brush, batch| {
        renderer.prepare_context(document, brush, PreparationContext {
            next: PreparationUses { selection_strokes: true, ..Default::default() },
            ..Default::default()
        });
        let declared = demanded(renderer, ANTICIPATED);
        let before = compile::tests::demands(&renderer.startup.as_ref().unwrap().compiler, BRUSH);
        document.working.selection = Some(layer_core::Selection::polygon(vec![
            layer_core::Point { x: 0., y: 0. }, layer_core::Point { x: 64., y: 0. },
            layer_core::Point { x: 64., y: 64. }, layer_core::Point { x: 0., y: 64. },
        ]).unwrap());
        renderer.prepare_startup(document, brush, false).unwrap();
        let after = compile::tests::demands(&renderer.startup.as_ref().unwrap().compiler, BRUSH);
        let added: HashSet<_> = after.difference(&before).copied().collect();
        assert!(!added.is_empty());
        assert!(added.is_subset(&declared), "unwarmed selected-brush recipes: {:?}",
            added.difference(&declared).collect::<Vec<_>>());
        drop(batch);
    });
}

#[test]
fn context_size_warming_covers_the_real_resized_brush_startup() {
    fixture(|renderer, document, brush, batch| {
        renderer.prepare_context(document, brush, PreparationContext::default());
        let declared = demanded(renderer, BACKGROUND);
        let before = compile::tests::demands(&renderer.startup.as_ref().unwrap().compiler, BRUSH);
        let mut resized = brush.clone();
        resized.diameter = 128.;
        renderer.prepare_startup(document, &resized, false).unwrap();
        let after = compile::tests::demands(&renderer.startup.as_ref().unwrap().compiler, BRUSH);
        let added: HashSet<_> = after.difference(&before).copied().collect();
        assert!(!added.is_empty(), "the resize must cross a material-kernel size boundary");
        assert!(added.is_subset(&declared), "unwarmed resized-brush recipes: {:?}",
            added.difference(&declared).collect::<Vec<_>>());
        drop(batch);
    });
}

#[derive(Clone, Copy, Debug)]
enum FirstUse { Fill, AutoSelect, ColorSelect, SelectionPaint, Gradient, Figure, Toolbar, Resize, SelectedBrush, Transform }

fn warmed_context_needs_no_compile_at_first_use(use_: FirstUse) {
    fixture(|renderer, document, brush, batch| {
        let shown = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let context = PreparationContext {
            next: PreparationUses { selection_strokes: matches!(use_, FirstUse::AutoSelect | FirstUse::ColorSelect | FirstUse::SelectionPaint | FirstUse::SelectedBrush), selection_paint: true, fill: true, transform: true,
                operation: Some(if matches!(use_, FirstUse::Figure) {OperationUse::Figure} else {OperationUse::Gradient}) },
            brushes: vec![shown.clone()], ..Default::default()
        };
        renderer.prepare_context(document, brush, context.clone());
        drop(batch);
        renderer.shader_idle(true, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(renderer, deadline, |progress| progress.brush_ready,
            format_args!("{use_:?} startup"));
        while compile::tests::queued_recipes(&renderer.context_compilation) != 0
            || compile::tests::queued_recipes(&renderer.startup.as_ref().unwrap().compiler) != 0 {
            renderer.poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "{use_:?} warming did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
        if matches!(use_, FirstUse::Fill | FirstUse::AutoSelect | FirstUse::ColorSelect) {
            renderer.prepare_region_cache(Some(&layer_render::RegionPreparation { source: RegionSource::Composite,
                reference_count: None, paper: None, tolerance: 0., refinement: context.regions, enclosed: false }));
            let frame = renderer.artwork_frame.clone().unwrap();
                while renderer.regions.as_ref().is_none_or(|regions| regions.background_pending()) {
                let mut encoder = crate::submission::CommandEncoder::new(&renderer.device, &Default::default());
                renderer.prefetch_regions(&frame, &mut encoder, &mut crate::test_support::work(false)).unwrap();
                renderer.uploads.finish(&encoder); encoder.submit(&renderer.queue);
                crate::test_support::complete(renderer);
                assert!(std::time::Instant::now() < deadline, "{use_:?} source prewarm did not finish");
            }
        }
        renderer.shader_idle(false, false);
        let first_use = renderer.compilation.batch();
        match use_ {
            FirstUse::SelectionPaint => assert!(renderer.paint_selection(&selection_contact()).unwrap()),
            FirstUse::Fill | FirstUse::AutoSelect | FirstUse::ColorSelect => {
                let mut request = request(matches!(use_, FirstUse::Fill), context.regions);
                request.contiguous = !matches!(use_, FirstUse::ColorSelect);
                let task = if matches!(use_, FirstUse::Fill) { RegionTask::Fill { request,
                    target: document.drawing_content().unwrap(), position: layer_core::Point {x:0.,y:0.} }
                } else { RegionTask::Mask(request) };
                assert!(renderer.request_region(task).unwrap());
                while !crate::region_requests::tests::request_encoded(renderer) {
                    if let Some(result) = renderer.take_region() { result.unwrap(); break; }
                    crate::test_support::complete(renderer);
                    assert!(std::time::Instant::now() < deadline, "{use_:?} deferred an already warmed request");
                }
            }
            FirstUse::Gradient | FirstUse::Figure => {
                let operation = if matches!(use_, FirstUse::Figure) {OperationUse::Figure} else {OperationUse::Gradient};
                let batches = [operation_batch(document, operation)];
                assert!(renderer.brush_batches_ready(FramePacket {dab_batches:&batches,
                    ..crate::test_support::packet(document.scene(), [64;2])}));
            }
            FirstUse::Toolbar | FirstUse::Resize | FirstUse::SelectedBrush | FirstUse::Transform => {
                let mut next = if matches!(use_, FirstUse::Toolbar) {shown.clone()} else {brush.clone()};
                if matches!(use_, FirstUse::Resize) {next.diameter = 128.;}
                if matches!(use_, FirstUse::SelectedBrush) {
                    let mut selection = layer_core::Selection::empty();
                    selection.inverted = true;
                    document.working.selection = Some(selection);
                    document.revision += 1;
                }
                renderer.prepare_startup(document, &next, matches!(use_, FirstUse::Transform)).unwrap();
                assert!(renderer.poll_startup().unwrap().brush_ready, "{use_:?} deferred a warmed brush");
            }
        }
        assert_eq!(compile::tests::queued_recipes(&renderer.compilation), 0, "{use_:?} requested a new pipeline");
        drop(first_use);
    });
}

macro_rules! warmed_first_use_tests {
    ($($name:ident: $use:ident),+ $(,)?) => {$(
        #[test]
        fn $name() { warmed_context_needs_no_compile_at_first_use(FirstUse::$use); }
    )+};
}
warmed_first_use_tests! {
    warmed_context_needs_no_compile_at_first_use_fill: Fill,
    warmed_context_needs_no_compile_at_first_use_auto_select: AutoSelect,
    warmed_context_needs_no_compile_at_first_use_color_select: ColorSelect,
    warmed_context_needs_no_compile_at_first_use_selection_paint: SelectionPaint,
    warmed_context_needs_no_compile_at_first_use_gradient: Gradient,
    warmed_context_needs_no_compile_at_first_use_figure: Figure,
    warmed_context_needs_no_compile_at_first_use_toolbar: Toolbar,
    warmed_context_needs_no_compile_at_first_use_resize: Resize,
    warmed_context_needs_no_compile_at_first_use_selected_brush: SelectedBrush,
    warmed_context_needs_no_compile_at_first_use_transform: Transform,
}

#[test]
fn startup_brush_changes_replace_queued_context_sizes() {
    fixture(|renderer, document, brush, batch| {
        let mut old = Needs::default(); old.sizes(renderer, document, brush, None);
        let old = identities(&old);
        assert!(!old.is_empty());
        let next_brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Chalk);
        let mut next = Needs::default(); next.sizes(renderer, document, &next_brush, None);
        let next = identities(&next);
        assert!(!next.is_empty());
        renderer.prepare_startup(document, &next_brush, false).unwrap();
        let declared = demanded(renderer, BACKGROUND);
        assert!(next.is_subset(&declared));
        assert!(old.difference(&next).all(|id| !declared.contains(id)));
        drop(batch);
    });
}

#[test]
fn changing_context_does_not_cancel_an_inflight_optional_recipe() {
    fixture(|renderer, document, brush, batch| {
        drop(batch);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        crate::test_support::wait_startup(renderer, deadline, |progress| progress.brush_ready,
            format_args!("inflight context startup"));
        let (entered, started) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let recipe = Deferred::new(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
            17
        });
        renderer.context_compilation.warm(&recipe, ANTICIPATED, renderer.context_compilation.generation());
        renderer.shader_idle(true, true);
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        renderer.prepare_context(document, brush, PreparationContext::default());
        let removed = !demanded(renderer, ANTICIPATED).contains(&recipe.identity());
        let running = !recipe.ready();
        release.send(()).unwrap();
        while !recipe.ready() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(removed && running);
        assert_eq!(*recipe, 17);
    });
}
