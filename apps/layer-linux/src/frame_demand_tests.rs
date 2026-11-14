use super::*;

fn render_thread() -> std::path::PathBuf {
    std::fs::read_dir("/proc/self/task").unwrap().filter_map(|task| task.ok()).map(|task| task.path())
        .filter(|task| std::fs::read_to_string(task.join("comm")).is_ok_and(|name| name.trim() == "canvas-gpu"))
        .min_by_key(|task| task.file_name().unwrap().to_str().unwrap().parse::<u64>().unwrap())
        .expect("a canvas GPU owner runs")
}

fn wakeups(thread: &std::path::Path) -> u64 {
    std::fs::read_to_string(thread.join("status")).unwrap().lines()
        .find_map(|line| line.strip_prefix("voluntary_ctxt_switches:")).unwrap().trim().parse().unwrap()
}

fn ui_frames(w: &Workspace) -> usize {
    ui_session(w).engine().backend().stats.lock().unwrap().frame_handler_cpu.len()
}

fn settle(w: &Workspace) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        pump(5);
        let ready = w.gpu.borrow().as_ref().is_some_and(|g| {
            let e = g.session.engine();
            e.backend().startup.complete && e.backend().frames_idle() && !e.has_pending_input() && !e.has_pending_document_edits()
                && g.session.frame_demand() == layer_render::FrameDemand::Wait
        });
        if ready && w.frame_timer.borrow().is_none() { break; }
        assert!(Instant::now() < deadline, "the canvas must settle");
    }
    pump(2 * layer_ui::session_recovery::CHECKPOINT_INTERVAL_MS + 500);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_idle_canvas_runs_no_frames_and_wakes_only_for_the_display_watch() {
    let app = native_test_app("art.capycanvas.IdleCanvasSleeps");
    let project = new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    settle(&w);
    let thread = render_thread();
    let (woken, framed) = (wakeups(&thread), ui_frames(&w));
    pump(5000);
    let (woken, framed) = (wakeups(&thread) - woken, ui_frames(&w) - framed);
    eprintln!("idle 5 s: render thread woke {woken} times, {framed} UI frames");
    let display_watch = (5000 / crate::render_thread::SCREEN_CHECK_DELAY.as_millis() + 1) as u64;
    assert!(woken <= display_watch && framed == 0,
        "an idle canvas woke its render thread {woken} times (display watch allows {display_watch}) and ran {framed} UI frames in 5 s");
}

fn suspended(w: &Workspace) -> bool {
    w.window.surface().and_downcast::<gdk::Toplevel>().is_some_and(|toplevel| toplevel.state().contains(gdk::ToplevelState::SUSPENDED))
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hidden_window_requests_no_frames() {
    let app = native_test_app("art.capycanvas.HiddenWindowFrames");
    let project = new_drawing(640, 480, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    settle(&w);
    w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Insert { effect: "ripple".into() } });
    let shown = ui_frames(&w);
    pump(500);
    assert!(ui_frames(&w) - shown > 10, "an animated effect draws frames while the window is shown");
    w.window.minimize();
    until(|| suspended(&w), "the compositor suspends a minimized window");
    pump(200);
    let thread = render_thread();
    let (woken, framed) = (wakeups(&thread), ui_frames(&w));
    pump(2000);
    let (woken, framed) = (wakeups(&thread) - woken, ui_frames(&w) - framed);
    eprintln!("hidden 2 s: render thread woke {woken} times, {framed} UI frames");
    let display_watch = (2000 / crate::render_thread::SCREEN_CHECK_DELAY.as_millis() + 1) as u64;
    assert!(woken <= display_watch && framed == 0,
        "a hidden window woke its render thread {woken} times (display watch allows {display_watch}) and ran {framed} UI frames in 2 s");
    w.window.set_visible(false);
    w.window.present();
    until(|| w.window.is_mapped() && !suspended(&w), "the compositor resumes a shown window");
    let resumed = ui_frames(&w);
    pump(500);
    assert!(ui_frames(&w) - resumed > 10, "frames resume when the window is shown again");
}
