use super::*;

fn handoff_themes() -> Vec<Theme> {
    match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref() {
        Ok("light") => vec![Theme::Light],
        Ok("dark") => vec![Theme::Dark],
        Err(_) => vec![Theme::Light, Theme::Dark],
        Ok(_) => panic!("CAPY_NATIVE_TEST_THEME must be light or dark"),
    }
}

fn chrome_settled(w: &Rc<Workspace>) {
    new_photo::ready(w);
    until(|| w.chrome_released.get() == w.chrome.revision() && w.frame_timer.borrow().is_none(),
        "current chrome paints and canvas scheduling becomes idle");
}

fn chrome_color_points(w: &Workspace) -> [[f32; 2]; 3] {
    let (tab_point, body_point) = {
        let groups = w.groups.borrow();
        let group = groups.iter().find(|g| g.tabs_visible && g.tabs.iter().any(|(_, tab)| tab.has_css_class("selected-tool"))).unwrap();
        let tab = group.tabs.iter().find(|(_, tab)| tab.has_css_class("selected-tool")).unwrap().1.compute_bounds(&w.surface).unwrap();
        let body = group.stack.compute_bounds(&w.surface).unwrap();
        ([tab.x() + 4., tab.y() + tab.height() - 3.], [body.x() + 3., body.y() + 3.])
    };
    let canvas = w.area.compute_bounds(&w.surface).unwrap();
    let surround = [canvas.x() + canvas.width() * 0.5, canvas.y() + canvas.height() * 0.8];
    [tab_point, body_point, surround]
}

#[test]
#[ignore = "private compositor recording; held theme changes during canvas Frames"]
fn native_held_chrome_during_canvas_frames() {
    let app = native_test_app("art.capycanvas.HeldChromeFrames");
    let w = fixture_workspace(&app);
    w.window.maximize(); w.window.present();
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(30);
    input.ready();
    chrome_settled(&w);
    use_transparency(&w, layer_ui::Transparency::Off);
    w.dispatch(UiAction::SetZoom { zoom: 0.1 });
    chrome_settled(&w);
    let points = chrome_color_points(&w);
    let mut mismatches = Vec::new();
    for theme in handoff_themes() {
        for episode in 0..3 {
            w.dispatch(UiAction::SetTheme { theme: Some(if theme == Theme::Light { Theme::Dark } else { Theme::Light }) });
            chrome_settled(&w);
            let before = ui_session(&w).engine().metrics().frames;
            let turns = Rc::new(Cell::new(0_u32));
            let target_before = Rc::new(Cell::new(None::<u64>));
            let trace = Rc::new(RefCell::new(Vec::new()));
            let clock = w.surface.frame_clock().unwrap();
            let observation = clock.connect_after_paint(glib::clone!(#[strong] w, #[strong] target_before, #[strong] trace, move |_| {
                let session = ui_session(&w);
                let target = target_before.get().is_some_and(|previous| w.chrome.revision() > previous) && session.state().theme == theme;
                trace.borrow_mut().push((target, w.chrome_hold.get()));
            }));
            let actor = RefCell::new(None);
            let name = format!("held-theme-frames-{theme:?}-{episode}").to_lowercase();
            let pixels = record_pixels(&w, &mut input, &name, points, || {
                actor.replace(Some(w.surface.add_tick_callback(glib::clone!(#[strong] w, #[strong] turns, #[strong] target_before, move |_, _| {
                    let turn = turns.get();
                    let phase = if turn == 0 { PenPhase::Down } else { PenPhase::Move };
                    let event = pen_event(&state(&w).camera, [200. + turn as f32, 200.], phase, u64::from(turn));
                    w.input.send(&w, event);
                    if turn == 3 {
                        target_before.set(Some(w.chrome.revision()));
                        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                    }
                    turns.set(turn + 1);
                    glib::ControlFlow::Continue
                }))));
            });
            actor.take().unwrap().remove();
            clock.disconnect(observation);
            let event = pen_event(&state(&w).camera, [200. + turns.get() as f32, 200.], PenPhase::Up, u64::from(turns.get()));
            w.input.send(&w, event);
            let trace = trace.borrow();
            let target_rows: Vec<_> = trace.iter().filter(|row| row.0).collect();
            assert!(!target_rows.is_empty() && target_rows.iter().all(|row| row.1),
                "{name}: target theme revision must be held while canvas Frames flow");
            assert!(turns.get() > 4 && ui_session(&w).engine().metrics().frames > before + 1,
                "{name}: discrete theme change overlaps real canvas Frames");
            transition_pixels(&name, &pixels, &mut mismatches);
            chrome_settled(&w);
        }
    }
    input.finish(); w.window.destroy();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
#[ignore = "private GTK display; discrete camera and window changes stay held"]
fn native_discrete_view_changes_keep_chrome_held() {
    let app = native_test_app("art.capycanvas.DiscreteViewChrome");
    let w = fixture_workspace(&app);
    w.window.present();
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(30);
    input.ready();
    for theme in handoff_themes() {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        chrome_settled(&w);
        for command in [CommandId::FitCanvas, CommandId::ActualPixels] {
            w.dispatch(UiAction::SetZoom { zoom: 0.25 });
            chrome_settled(&w);
            let paints = sample_after_paint(&w.surface, glib::clone!(#[strong] w, move ||
                (ui_session(&w).camera().revision, w.chrome_hold.get())
            ));
            let pacing = w.navigation_input.get();
            w.dispatch(UiAction::Invoke { command });
            assert!(w.navigation_input.get() > pacing, "{theme:?}/{command:?}: discrete camera command advances its pacing anchor");
            let camera = ui_session(&w).camera().revision;
            until(|| paints.samples.borrow().iter().any(|paint| paint.state.0 == camera), "discrete view change paints its camera");
            let rows = paints.finish();
            assert!(rows.iter().filter(|paint| paint.state.0 == camera).all(|paint| paint.state.1),
                "{theme:?}/{command:?}: an isolated camera command keeps chrome held");
            chrome_settled(&w);
        }
        let paints = sample_after_paint(&w.surface, glib::clone!(#[strong] w, move ||
            (ui_session(&w).camera().revision, w.chrome_hold.get())
        ));
        let pacing = w.navigation_input.get();
        w.dispatch(UiAction::SetZoom { zoom: 0.6 });
        assert!(w.navigation_input.get() > pacing, "{theme:?}: direct SetZoom advances its pacing anchor");
        let camera = ui_session(&w).camera().revision;
        until(|| paints.samples.borrow().iter().any(|paint| paint.state.0 == camera), "single zoom paints its camera");
        assert!(paints.finish().iter().filter(|paint| paint.state.0 == camera).all(|paint| paint.state.1),
            "{theme:?}: a direct SetZoom keeps chrome held");
        chrome_settled(&w);
        let previous = [w.window.width(), w.window.height()];
        let paints = sample_after_paint(&w.surface, glib::clone!(#[strong] w, move ||
            ([w.window.width(), w.window.height()], w.chrome_hold.get())
        ));
        w.window.set_default_size(previous[0] - 64, previous[1] - 48);
        until(|| paints.samples.borrow().iter().any(|paint| paint.state.0 != previous), "single window size change paints");
        let rows = paints.finish();
        let first = rows.iter().find(|paint| paint.state.0 != previous).unwrap();
        assert!(first.state.1, "{theme:?}: first paint of a single window size jump keeps chrome held");
        chrome_settled(&w);
        let camera = ui_session(&w).camera().revision;
        let paints = sample_after_paint(&w.surface, glib::clone!(#[strong] w, move ||
            (ui_session(&w).camera().revision, w.chrome_hold.get())
        ));
        let point = screen_point(w.area.upcast_ref(), &w.window, [0.5, 0.5]);
        let mut events = vec![serde_json::json!({"point":point})];
        events.extend((0..20).map(|_| serde_json::json!({"wheel":[0,1]})));
        let pacing = w.navigation_input.get();
        input.perform(serde_json::Value::Array(events));
        assert!(w.navigation_input.get() > pacing, "{theme:?}: native navigation advances its pacing anchor");
        until(|| paints.samples.borrow().iter().any(|paint| paint.state.0 != camera && !paint.state.1), "native continuous navigation paints unheld chrome");
        paints.finish();
        chrome_settled(&w);
    }
    input.finish(); w.window.destroy();
}

fn pending_dependency(w: &Rc<Workspace>) -> (usize, bool) {
    let receiver = ui_session(w).engine().backend().pending_dependency().unwrap();
    glib::MainContext::default().block_on(async move {
        gtk::gio::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(10)))
            .await.expect("dependency observer task").expect("dependency observer reply")
    })
}

#[test]
#[ignore = "private compositor recording; chrome paints while a front raster dependency is unpublished"]
fn native_chrome_progress_during_pending_canvas() {
    use layer_render::CanvasRenderer;
    let app = native_test_app("art.capycanvas.PendingCanvasChrome");
    let w = fixture_workspace(&app);
    w.window.maximize(); w.window.present();
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(30);
    input.ready();
    chrome_settled(&w);
    use_transparency(&w, layer_ui::Transparency::Off);
    w.dispatch(UiAction::SetZoom { zoom: 0.1 });
    chrome_settled(&w);
    let points = chrome_color_points(&w);
    let mut mismatches = Vec::new();
    for theme in handoff_themes() {
        w.dispatch(UiAction::SetTheme { theme: Some(if theme == Theme::Light { Theme::Dark } else { Theme::Light }) });
        chrome_settled(&w);
        let (mut document, view) = {
            let session = ui_session(&w);
            (session.engine().document().clone(), session.engine().view())
        };
        let paint = document.artwork.paint.iter().next().unwrap().0;
        let saved = document.artwork.paint.get(paint).unwrap().raster.try_data().unwrap().unwrap();
        let gate = layer_core::raster::RasterRevision::pending();
        document.artwork.paint.get_mut(paint).unwrap().raster = gate.clone();
        let restore = [(layer_core::SourceTarget::Paint(paint), gate.clone())];
        w.gpu.borrow_mut().as_mut().unwrap().session.renderer_mut().submit(layer_render::FramePacket {
            time_seconds: 0., view, document_extent: document.composition().size,
            scene: document.scene(), inspect_mask: None, selection_overlays: None,
            dabs: &[], dab_batches: &[], restore_rasters: &restore, reset_layers: false,
            commit_rasters: true, composite_all: true, blend_space: document.composition().blend,
        }).unwrap();
        let before = pending_dependency(&w);
        assert!(before.0 >= 1 && before.1 && gate.try_data().is_none(), "front Frame has an unpublished raster dependency: {before:?}");
        let previous = w.chrome.revision();
        let paints = sample_after_paint(&w.surface, glib::clone!(#[strong] w, move ||
            (w.chrome_released.get(), w.chrome.revision())
        ));
        let name = format!("theme-with-pending-raster-{theme:?}").to_lowercase();
        let recorded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            record_pixels(&w, &mut input, &name, points, || {
                w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                until(|| paints.samples.borrow().iter().any(|paint| paint.state.0 > previous && paint.state.0 == paint.state.1),
                    "held chrome paints while its front raster dependency remains unpublished");
            })
        }));
        let after = recorded.as_ref().ok().map(|_| pending_dependency(&w));
        let unpublished = gate.try_data().is_none();
        gate.publish(Ok(saved.as_ref().clone())).unwrap();
        let pixels = match recorded { Ok(pixels) => pixels, Err(failure) => std::panic::resume_unwind(failure) };
        assert!(after.is_some_and(|state| state.0 >= 1 && state.1) && unpublished,
            "chrome painted before the front Frame dependency was published: {after:?}");
        assert!(!paints.finish().is_empty(), "pending-raster theme transition has actual GTK paints");
        transition_pixels(&name, &pixels, &mut mismatches);
        chrome_settled(&w);
    }
    input.finish(); w.window.destroy();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
