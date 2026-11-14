use super::*;
use layer_render::CanvasRenderer;
use layer_ui::{EffectAction, FilterPreviewCache, Platform, UiAction, UiSession};

fn settle_picker(session: &mut UiSession<WgpuRasterizer>) {
    static CLOCK: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let deadline = std::time::Instant::now() + READBACK_TIMEOUT;
    while !session.filter_previews_idle() || session.engine().backend().frame_demand() == layer_render::FrameDemand::Work {
        let document = session.engine().document().clone();
        let brush = session.engine().brush().clone();
        let renderer = session.renderer_mut();
        if renderer.startup_needs_update(&document, &brush, false) {
            renderer.prepare_startup(&document, &brush, false).unwrap();
        }
        renderer.poll_startup().unwrap();
        renderer.device().poll(wgpu::PollType::Poll).unwrap();
        let now = CLOCK.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64;
        session.frame(now, now).unwrap();
        session.renderer_mut().shader_idle(false, false);
        assert!(std::time::Instant::now() < deadline, "picker must finish its pending edits: {:?}", session.state().filter_picker.selected);
        std::thread::yield_now();
    }
    session.renderer_mut().shader_idle(false, false);
}

fn picker() -> UiSession<WgpuRasterizer> {
    let reference = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let renderer = crate::test_support::staged_renderer(&reference, Default::default());
    renderer.shader_idle(false, false);
    let mut document = crate::layer_tests::placement::paint_document([64; 2], "Filters");
    crate::layer_tests::placement::set_source(&mut document,
        layer_core::color::source::rgba8_source([64; 2], |_, _| [127, 63, 191, 255]));
    let mut session = UiSession::new(renderer, document, [1200, 900], Platform::Gtk).unwrap();
    session.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(layer_ui::WorkspaceState {
        layout: layer_ui::WorkspacePreset::Painter.layout(Platform::Gtk), ..Default::default()
    }) }).unwrap();
    let id = session.state().workspace.layout.header.entries().find(|entry| entry.item == layer_ui::HeaderItem::Tool {
        control: layer_ui::ToolbarControl::Panel { panel: layer_ui::Panel::Adjustments }
    }).unwrap().id;
    session.dispatch(UiAction::MeasureHeader { height: 60., items: vec![layer_ui::HeaderItemBounds {
        id, bounds: layer_ui::Bounds { x: 80., y: 0., width: 40., height: 60. }
    }] }).unwrap();
    session.dispatch(UiAction::ActivateHeaderItem { id }).unwrap();
    let document = session.engine().document().clone();
    let brush = session.engine().brush().clone();
    session.renderer_mut().prepare_startup(&document, &brush, false).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    crate::test_support::wait_startup(session.renderer_mut(), deadline, |p| p.brush_ready,
        format_args!("filter picker startup"));
    settle_picker(&mut session);
    session
}

#[test]
fn visible_filter_apply_uses_recipes_warmed_by_the_picker() {
    for (id, replacement) in [("curves", false), ("gaussian_blur", false), ("curves", true), ("gaussian_blur", true), ("gradient_fill", false)] {
        let mut session = picker();
        if id == "gradient_fill" {
            let paint = crate::layer_tests::placement::occurrence_id(session.engine().document());
            session.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: Arc::from(id) } }).unwrap();
            let handle = session.engine().document().working.occurrence.unwrap();
            session.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Blend {
                id: layer_ui::occurrence_token(handle), value: layer_core::LayerBlend::Multiply.code()
            } }).unwrap();
            session.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Select {
                id: layer_ui::occurrence_token(paint), mask: false
            } }).unwrap();
        }
        if replacement {
            session.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: Arc::from("exposure") } }).unwrap();
            let handle = session.engine().document().working.occurrence.unwrap();
            session.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Blend {
                id: layer_ui::occurrence_token(handle), value: layer_core::LayerBlend::Multiply.code()
            } }).unwrap();
        }
        settle_picker(&mut session);
        let update = session.poll_filter_previews(0, vec![Arc::from(id)], FilterPreviewCache::default()).unwrap();
        assert_eq!(update.status.requests, 0, "bundled rows keep their bundled previews");
        let recipes = compile::tests::ordered_demands(&session.renderer_mut().context_compilation, ANTICIPATED);
        assert!(!recipes.is_empty(), "the visible filter must declare cold apply recipes while optional work is parked");
        session.renderer_mut().shader_idle(true, true);
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        while compile::tests::queued_recipes(&session.renderer_mut().context_compilation) != 0 {
            session.renderer_mut().poll_startup().unwrap();
            assert!(std::time::Instant::now() < deadline, "visible filter warming timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        session.renderer_mut().shader_idle(false, false);
        session.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: Arc::from(id) } }).unwrap();
        let document = session.engine().document().clone();
        let snapshot = document.snapshot();
        let view = snapshot.view();
        assert!(view.order().iter().any(|&handle| view.effect(handle).is_some_and(|effect| effect.program.id.as_ref() == id)));
        let renderer = session.renderer_mut();
        let mut actual = renderer.scene_pipelines.effects(renderer);
        for (chain, execution) in scene::effect_chains(view, scene::EffectPreparation::Display) {
            actual.prepare(renderer, view, &chain, execution, 0., 0, view.composition().blend).unwrap();
        }
        if id == "gradient_fill" {
            let handle = document.working.occurrence.unwrap();
            actual.prepare(renderer, view, &[handle], effects::Execution::Preview, 0., 0,
                scene::preview_space(view.effect(handle).unwrap(), view.composition().blend)).unwrap();
        }
        assert!(actual.needs().ready(), "{id}: applying the visible filter must not create cold pipelines");
        settle_picker(&mut session);
    }
}

#[test]
fn closing_the_filter_picker_cancels_unstarted_apply_recipes() {
    for native_close in [false, true] {
        let mut session = picker();
        session.poll_filter_previews(0, vec![Arc::from("curves")], FilterPreviewCache::default()).unwrap();
        let queued = compile::tests::ordered_demands(&session.renderer_mut().context_compilation, ANTICIPATED);
        assert!(!queued.is_empty());
        if native_close {
            let id = session.state().workspace.layout.header.entries().find(|entry| entry.item == layer_ui::HeaderItem::Tool {
                control: layer_ui::ToolbarControl::Panel { panel: layer_ui::Panel::Adjustments }
            }).unwrap().id;
            session.dispatch(UiAction::ActivateHeaderItem { id }).unwrap();
        } else {
            session.poll_filter_previews(1, Vec::new(), FilterPreviewCache::default()).unwrap();
        }
        let remaining = compile::tests::demands(&session.renderer_mut().context_compilation, ANTICIPATED);
        assert!(queued.iter().all(|id| !remaining.contains(id)));
    }
}
