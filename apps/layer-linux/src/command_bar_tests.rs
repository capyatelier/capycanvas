use super::*;

#[test]
#[ignore = "isolated native-input.js --native-test=native_command_bar_input --native-storage"]
fn native_command_bar_input() {
    let mut d = Driver::managed("art.capycanvas.CommandBar");
    d.w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    let open = serde_json::json!([
        {"key":0xffe3,"down":true}, {"key":0xffe1,"down":true}, {"key":0x50,"down":true},
        {"key":0x50,"down":false}, {"key":0xffe1,"down":false}, {"key":0xffe3,"down":false}
    ]);
    let mut timings = Vec::new();
    for attempt in 0..3 {
        d.perform(open.clone());
        assert!(
            state(&d.w).command_search.is_some(),
            "keyboard opener {attempt}: focus={:?}, command={:?}, error={:?}",
            gtk::prelude::GtkWindowExt::focus(&d.w.window),
            d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .command(CommandId::SearchCommands),
            state(&d.w).host_error
        );
        let entry = d
            .named("command-search")
            .downcast::<gtk::SearchEntry>()
            .unwrap();
        assert!(
            gtk::prelude::GtkWindowExt::focus(&d.w.window)
                .is_some_and(|focus| focus == entry || focus.is_ancestor(&entry))
        );
        let start = Instant::now();
        entry.set_text("fit cnvs");
        assert_eq!(
            state(&d.w).command_search.as_ref().unwrap().results[0].id,
            "command.fit_canvas"
        );
        timings.push(start.elapsed());
        // Query and immediate Enter run the new result, without a debounce.
        d.key(0xff0d);
        assert!(state(&d.w).command_search.is_none());
    }
    d.perform(open.clone());
    let entry = d
        .named("command-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap();
    entry.set_text("undo");
    pump(150);
    let popup = d.named("command-bar").downcast::<gtk::Popover>().unwrap();
    let detail = d.named("command-detail").downcast::<gtk::Label>().unwrap();
    assert_eq!(detail.text(), "Nothing to undo");
    capture_popover(
        &popup,
        d.dir.join("command-bar-light.png").to_str().unwrap(),
    );
    assert_eq!(
        state(&d.w).command_search.as_ref().unwrap().results[0]
            .disabled_reason
            .as_deref(),
        Some("Nothing to undo")
    );
    d.key(0xff0d);
    assert!(state(&d.w).command_search.as_ref().unwrap().error.is_some());
    entry.set_text("brush size");
    d.key(0xff0d);
    assert!(
        state(&d.w)
            .command_search
            .as_ref()
            .unwrap()
            .parameter
            .is_some()
    );
    pump(150);
    capture_popover(
        &popup,
        d.dir.join("command-bar-value.png").to_str().unwrap(),
    );
    entry.set_text("24");
    d.key(0xff0d);
    assert_eq!(state(&d.w).brush.diameter, 24.);
    assert!(state(&d.w).command_search.is_none());
    d.w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    d.perform(open.clone());
    entry.set_text("select");
    pump(150);
    capture_popover(&popup, d.dir.join("command-bar-dark.png").to_str().unwrap());
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        d.perform(serde_json::json!([{ "capture": "command-bar-placement" }]));
    }
    d.key(0xff54); // Down
    assert_eq!(state(&d.w).command_search.as_ref().unwrap().selected, 1);
    d.key(0xff1b); // Escape
    assert!(state(&d.w).command_search.is_none());
    d.perform(open.clone());
    entry.set_text("eraser");
    let row = d.named("command-result-0");
    let point = d.point(&row);
    d.perform(serde_json::json!([{"touch":"down","point":point},{"touch":"up"}]));
    assert!(
        state(&d.w).command_search.is_none(),
        "touch invokes the selected command"
    );
    assert_eq!(state(&d.w).brush.tool, Tool::Eraser);
    d.perform(open);
    let revision =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision;
    let point = [
        d.w.window.width() as f32 / 2.,
        d.w.window.height() as f32 * 0.8,
    ];
    d.perform(serde_json::json!([{"point":point},{"down":true},{"down":false}]));
    assert!(
        state(&d.w).command_search.is_none(),
        "outside contact dismisses"
    );
    assert_eq!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        revision,
        "dismissal must not paint"
    );
    eprintln!("GTK command query/update timings: {timings:?}");
    let mut open_paint = Vec::new();
    let mut query_paint = Vec::new();
    for i in 0..20 {
        let start = Instant::now();
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::SearchCommands,
        });
        wait_for_command_paint(&popup);
        open_paint.push(start.elapsed());
        let start = Instant::now();
        entry.set_text(["select", "undo", "brush size", "pencil"][i % 4]);
        wait_for_command_paint(&popup);
        query_paint.push(start.elapsed());
        d.w.dispatch(UiAction::CommandSearch {
            action: CommandSearchAction::Close,
        });
        pump(150);
    }
    open_paint.sort();
    query_paint.sort();
    eprintln!(
        "GTK warm open to paint p95: {:?}; query to paint p95: {:?}",
        open_paint[18], query_paint[18]
    );
    d.w.window.close();
}

fn wait_for_command_paint(popup: &gtk::Popover) {
    let clock = popup.frame_clock().unwrap();
    let painted = Rc::new(Cell::new(false));
    let signal = clock.connect_after_paint(glib::clone!(
        #[strong]
        painted,
        move |_| painted.set(true)
    ));
    popup.queue_draw();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !painted.get() {
        assert!(Instant::now() < deadline, "command bar paints");
        pump(1);
    }
    clock.disconnect(signal);
}
