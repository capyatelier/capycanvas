use super::*;

fn picker() -> layer_render::ColorPickerOverlay {
    layer_render::ColorPickerOverlay {sample_color_only: false, center: [32.; 2], sample: [32.; 2], scale: 1., classic: false,
        layer: false, original: [0., 0., 0., 1.], candidate: [1.; 4]}
}

#[test]
fn first_ready_frame_requires_only_its_viewport_variant() {
    let extent = [64; 2];
    let document = crate::scene::scale::tests::document_at(extent);
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let all = layer_core::SceneScope::All;
    crate::test_support::submit_settled(&mut r, crate::test_support::packet(document.scene().with_scope(&all), extent));
    assert!(r.presentation_page().is_some(), "the first ready frame must have a committed keyed projection");
    assert!(r.selected_display_cache().unwrap().presentable_pages() > 0);
    let batch = r.compilation.batch();
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8Unorm, SdrSurfaceColor::Srgb).unwrap();
    assert!(presenter.recipes.paper.get().is_none(), "an unlit canvas needs no lit viewport recipes");
    let unused_required = presenter.pipeline[1..].iter().any(|pipeline|
        crate::compile::tests::requested(&r.compilation, pipeline, crate::startup::DOCUMENT));
    pollster::block_on(r.compilation.prepare_all([&presenter.pipeline[0], &presenter.surround_pipeline,
        &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline])).unwrap();
    let ready = presenter.pipelines_ready(&r);
    let (_, target) = crate::create_target(r.device(), extent, wgpu::TextureFormat::Rgba8Unorm, "first viewport variant");
    let mut encoder = r.device().create_command_encoder(&Default::default());
    presenter.encode(&r, &mut encoder, &target, crate::test_support::view(extent), [0.; 4]).unwrap();
    assert!(presenter.recipes.paper.get().is_none(), "an unlit frame must not request lit viewport recipes");
    let encoded = presenter.history.valid;
    let anticipated = presenter.pipeline[1..].iter().all(|pipeline|
        crate::compile::tests::requested(&r.compilation, pipeline, crate::startup::VALIDATION)
            && !crate::compile::tests::requested(&r.compilation, pipeline, crate::startup::DOCUMENT));
    drop(encoder);
    drop(presenter);
    drop(batch);
    drop(r);
    crate::finish_shader_compiler_shutdown();
    assert!(!unused_required, "unused viewport variants do not enter the first frame gate");
    assert!(ready && encoded, "the ready source draws while the other variants remain cold");
    assert!(anticipated, "after the first ready frame, remaining variants run without idle admission");
}

#[test]
fn first_lit_frame_gates_its_variant_without_idle_frames_from_any_warm_class() {
    use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, Instant}};
    let mut document = crate::layer_tests::paper_shade::rough_gray_paper();
    crate::layer_tests::paper_shade::set_paper(&mut document, &[(layer_core::paper::RELIEF, layer_core::EffectValue::Number(150.))]);
    let reference = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let mut renderer = crate::test_support::staged_renderer(&reference, document.composition().color);
    let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
    renderer.shader_idle(false, false);
    renderer.prepare_startup(&document, &brush, false).unwrap();
    crate::test_support::submit_settled(&mut renderer, crate::test_support::packet(document.scene(), [128; 2]));
    let deadline = Instant::now() + Duration::from_secs(60);
    while !renderer.poll_startup().unwrap().complete || renderer.paper_shade().is_none() {
        assert!(Instant::now() < deadline, "paper startup must complete");
        std::thread::sleep(Duration::from_millis(1));
    }
    renderer.prepare_context(&document, &brush, layer_render::PreparationContext {
        next: layer_render::PreparationUses {selection_paint: true, ..Default::default()},
        brushes: vec![layer_core::default_brush(layer_core::DefaultBrushPreset::Paintbrush)],
        ..Default::default()
    });
    for class in [crate::startup::ANTICIPATED, crate::startup::BACKGROUND] {
        assert!(!crate::compile::tests::ordered_demands(&renderer.context_compilation, class).is_empty());
    }
    let batch = renderer.compilation.batch();
    let mut presenter = ViewportPresenter::for_surface(&renderer, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
    presenter.prepare_paper_lighting(&renderer);
    let paper = presenter.recipes.paper.get().unwrap().clone();
    assert!(presenter.bound_viewport(&renderer).1, "configured lighting selects the lit recipe before it is ready");
    assert!(!presenter.pipelines_ready(&renderer));
    assert!(crate::compile::tests::requested(&renderer.compilation, &paper[0], crate::startup::DOCUMENT));
    assert!(paper[1..].iter().all(|pipeline| !crate::compile::tests::requested(&renderer.compilation, pipeline, crate::startup::VALIDATION)));
    assert!(presenter.pipeline.iter().all(|pipeline| !crate::compile::tests::requested(&renderer.compilation, pipeline, crate::startup::DOCUMENT)));
    pollster::block_on(renderer.compilation.prepare_all([&paper[0], &presenter.surround_pipeline,
        &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline])).unwrap();
    assert!(presenter.pipelines_ready(&renderer));
    assert!(paper[1..].iter().all(|pipeline| !pipeline.ready()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || { count.fetch_add(1, Ordering::Relaxed); });
    renderer.set_preparation_wake(wake.clone());
    renderer.context_compilation.set_wake(wake);
    let (_, target) = crate::create_target(renderer.device(), [128; 2], wgpu::TextureFormat::Rgba8UnormSrgb, "first lit viewport variant");
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    presenter.encode(&renderer, &mut encoder, &target, crate::test_support::view([128; 2]), [0.; 4]).unwrap();
    assert!(presenter.paper_lighting_drawn());
    assert_eq!(presenter.last_artwork_pipeline.as_ref(), Some(paper[0].compile()));
    assert!(paper[1..].iter().all(|pipeline|
        crate::compile::tests::requested(&renderer.compilation, pipeline, crate::startup::VALIDATION)
            && !crate::compile::tests::requested(&renderer.compilation, pipeline, crate::startup::DOCUMENT)));
    assert!(renderer.poll_startup().unwrap().complete, "unused warming must not reopen startup readiness");
    assert!(!crate::compile::tests::ordered_demands(&renderer.compilation, crate::startup::OTHER).is_empty());
    renderer.queue.submit([encoder.finish()]);
    drop(batch);
    renderer.shader_idle(true, true);
    while crate::compile::tests::queued_recipes(&renderer.compilation)
        + crate::compile::tests::queued_recipes(&renderer.context_compilation) != 0 {
        renderer.compilation.check().unwrap();
        renderer.context_compilation.check().unwrap();
        assert!(renderer.poll_startup().unwrap().complete, "warming must keep startup complete");
        assert!(Instant::now() < deadline, "every idle family must finish warming");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(paper.iter().all(crate::Deferred::ready));
    assert!(presenter.pipeline[1..].iter().all(crate::Deferred::ready));
    assert!(renderer.poll_startup().unwrap().complete);
    assert!(renderer.frame_demand() != layer_render::FrameDemand::Work);
    assert!(!presenter.needs_present(&renderer, crate::test_support::view([128; 2]), [0.; 4]));
    drop(presenter);
    drop(renderer);
    drop(reference);
    crate::finish_shader_compiler_shutdown();
    assert_eq!(wakes.load(Ordering::Relaxed), 0, "unused warm completions must request no idle frames");
}

#[test]
fn standalone_overview_of_lit_paper_requires_no_lit_viewport_variant() {
    let mut document = crate::layer_tests::paper_shade::rough_gray_paper();
    crate::layer_tests::paper_shade::set_paper(&mut document, &[(layer_core::paper::RELIEF, layer_core::EffectValue::Number(150.))]);
    let mut renderer = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    crate::test_support::submit_settled(&mut renderer, crate::test_support::packet(document.scene(), [128; 2]));
    assert!(renderer.paper_shade().is_some());
    renderer.shader_idle(false, false);
    let _batch = renderer.compilation.batch();
    let presenter = ViewportPresenter::for_overview_surface(&renderer, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb).unwrap();
    assert!(presenter.recipes.paper.get().is_none());
    assert!(!presenter.bound_viewport(&renderer).1);
    pollster::block_on(renderer.compilation.prepare_all([&presenter.surround_pipeline,
        &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline].into_iter().chain(presenter.overview_pipeline.iter()))).unwrap();
    assert!(presenter.pipelines_ready(&renderer));
    assert!(presenter.recipes.paper.get().is_none());
}

#[test]
fn presenter_creation_declares_each_output_before_first_presentation() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let batch = r.compilation.batch();
    for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8UnormSrgb, wgpu::TextureFormat::Rgba16Float] {
        let before = r.compilation.pending();
        let presenter = ViewportPresenter::for_surface(&r, format, SdrSurfaceColor::Srgb).unwrap();
        assert!(r.compilation.pending() > before, "{format:?}: creation must declare its recipes");
        assert!(presenter.pipeline.iter().all(|pipeline| !pipeline.ready()));
        let queued = r.compilation.pending();
        for _ in 0..20 { assert!(!presenter.pipelines_ready(&r)); }
        assert_eq!(r.compilation.pending(), queued, "repeated declarations share one queue entry per recipe");
    }
    drop(batch);
}

#[test]
fn unused_picker_is_prepared_after_construction_when_idle() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let batch = r.compilation.batch();
    let presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8Unorm, SdrSurfaceColor::Srgb).unwrap();
    assert!(!presenter.picker.pipeline.ready());
    assert!(presenter.picker.bounds().is_none());
    r.shader_idle(true, true);
    drop(batch);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !presenter.picker.pipeline.ready() {
        r.compilation.check().unwrap();
        assert!(std::time::Instant::now() < deadline, "constructor-declared optional picker must run while idle");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(presenter.picker.bounds().is_none());
}

#[test]
fn hidden_overview_and_picker_stay_optional_while_visible_navigator_is_required() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let _batch = r.compilation.batch();
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8Unorm, SdrSurfaceColor::Srgb).unwrap();
    presenter.prepare_overviews(&r);
    presenter.set_color_picker(Some(picker()));
    pollster::block_on(r.compilation.prepare_all(presenter.pipeline.iter()
        .chain([&presenter.surround_pipeline, &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline]))).unwrap();
    assert!(!presenter.overview_pipeline.as_ref().unwrap().ready());
    assert!(!presenter.picker.pipeline.ready());
    assert!(presenter.pipelines_ready(&r), "hidden overview and picker must not hold the canvas gate");
    let queued = crate::compile::tests::queued_recipes(&r.compilation);
    for _ in 0..20 { assert!(presenter.pipelines_ready(&r)); }
    assert_eq!(crate::compile::tests::queued_recipes(&r.compilation), queued);
    r.presentation.navigator = true;
    assert!(presenter.pipelines_ready(&r), "an available image without a visible slot is not a visible Navigator");
    presenter.set_overviews(&r, &[crate::OverviewPlacement {bounds: [0., 0., 32., 32.], clip: None, work_area: [[0.; 2]; 4],
        outline_linear: [0.; 3], scale: 1., opacity: 1.}]);
    r.presentation.navigator = false;
    assert!(!presenter.pipelines_ready(&r), "a visible slot must prepare before its first image arrives");
    r.presentation.navigator = true;
    assert!(!presenter.pipelines_ready(&r), "a visible Navigator must be ready at the canvas gate");
    pollster::block_on(r.compilation.prepare_all(presenter.overview_pipeline.iter())).unwrap();
    assert!(presenter.pipelines_ready(&r));
    assert!(!presenter.picker.pipeline.ready());
}

#[test]
fn standalone_navigator_declares_its_output_recipe_at_creation() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let _batch = r.compilation.batch();
    let presenter = ViewportPresenter::for_overview_surface(&r, wgpu::TextureFormat::Rgba8Unorm, SdrSurfaceColor::Srgb).unwrap();
    assert_eq!(presenter.overview_pipeline.as_ref().unwrap().identity(), presenter.recipes.overviews[1].get().unwrap().identity());
    assert!(!presenter.pipelines_ready(&r));
    pollster::block_on(r.compilation.prepare_all(presenter.pipeline.iter()
        .chain([&presenter.surround_pipeline, &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline])
        .chain(presenter.overview_pipeline.iter()))).unwrap();
    assert!(presenter.pipelines_ready(&r));
    assert!(!presenter.recipes.overviews[0].get().unwrap().ready());
}

#[test]
fn optional_picker_completion_repaints_without_parking_the_canvas() {
    use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, time::{Duration, Instant}};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let batch = r.compilation.batch();
    let mut presenter = ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8Unorm, SdrSurfaceColor::Srgb).unwrap();
    presenter.set_target_retention(true);
    presenter.set_color_picker(Some(picker()));
    pollster::block_on(r.compilation.prepare_all(presenter.pipeline.iter()
        .chain([&presenter.surround_pipeline, &presenter.cursor_pipeline, &presenter.recipes.transition.pipeline]))).unwrap();
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    r.set_preparation_wake(Arc::new(move || { count.fetch_add(1, Ordering::Relaxed); }));
    let (_, target) = crate::create_target(&r.device, [64; 2], wgpu::TextureFormat::Rgba8Unorm, "optional picker");
    let view = crate::test_support::view([64; 2]);
    let mut encoder = r.device.create_command_encoder(&Default::default());
    presenter.encode(&r, &mut encoder, &target, view, [0.; 4]).unwrap();
    assert!(presenter.history.valid);
    assert!(!presenter.picker.pipeline.ready());
    assert!(!presenter.needs_present(&r, view, [0.; 4]), "a cold optional picker must allow an idle canvas");
    pollster::block_on(r.compilation.prepare_all(presenter.overview_pipeline.iter())).unwrap();
    assert!(!presenter.needs_present(&r, view, [0.; 4]), "a hidden overview does not invalidate presentation");
    drop(batch);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !presenter.picker.pipeline.ready() || wakes.load(Ordering::Relaxed) == 0 {
        r.compilation.check().unwrap();
        assert!(Instant::now() < deadline, "the waiting picker must wake when its recipe completes");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(wakes.load(Ordering::Relaxed), 1);
    assert!(presenter.needs_present(&r, view, [0.; 4]), "the newly ready picker must repaint");
    presenter.encode(&r, &mut encoder, &target, view, [0.; 4]).unwrap();
    assert!(!presenter.needs_present(&r, view, [0.; 4]));
    r.queue.submit([encoder.finish()]);
}
