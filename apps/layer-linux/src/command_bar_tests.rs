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
        d.input.perform(open.clone());
        assert!(
            state(&d.w).command_search.is_some(),
            "keyboard opener {attempt}: focus={:?}, command={:?}, error={:?}",
            gtk::prelude::GtkWindowExt::focus(&d.w.window),
            ui_session(&d.w)
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
        d.input.key(0xff0d);
        assert!(state(&d.w).command_search.is_none());
    }
    d.input.perform(open.clone());
    let entry = d
        .named("command-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap();
    entry.set_text("undo");
    pump(150);
    let card = d.named("command-bar").downcast::<gtk::Box>().unwrap();
    let detail = d.named("command-detail").downcast::<gtk::Label>().unwrap();
    assert_eq!(detail.text(), "Nothing to undo");
    icons::capture_widget(&d.w.window, &card)
        .save_to_png(d.input.dir.join("command-bar-light.png")).unwrap();
    d.input.key(0xff0d);
    assert!(state(&d.w).command_search.as_ref().unwrap().error.is_some());
    entry.set_text("brush size");
    d.input.key(0xff0d);
    assert!(
        state(&d.w)
            .command_search
            .as_ref()
            .unwrap()
            .parameter
            .is_some()
    );
    pump(150);
    icons::capture_widget(&d.w.window, &card)
        .save_to_png(d.input.dir.join("command-bar-value.png")).unwrap();
    entry.set_text("24");
    d.input.key(0xff0d);
    assert_eq!(state(&d.w).brush.diameter, 24.);
    assert!(state(&d.w).command_search.is_none());
    d.w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    d.input.perform(open.clone());
    entry.set_text("select");
    pump(150);
    icons::capture_widget(&d.w.window, &card)
        .save_to_png(d.input.dir.join("command-bar-dark.png")).unwrap();
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        d.input
            .perform(serde_json::json!([{ "capture": "command-bar-placement" }]));
    }
    d.input.key(0xff54); // Down
    assert_eq!(state(&d.w).command_search.as_ref().unwrap().selected, 1);
    d.input.key(0xff1b); // Escape
    assert!(state(&d.w).command_search.is_none());
    d.input.perform(open.clone());
    entry.set_text("eraser");
    let row = d.named("command-result-0");
    wait_for_command_paint(&card);
    let requested = d.w.chrome.revision();
    until(|| row.is_mapped() && row.width() > 0 && row.height() > 0
        && row.compute_bounds(&d.w.window).is_some_and(|bounds| bounds.width() > 0. && bounds.height() > 0.)
        && d.w.chrome_released.get() >= requested, "allocated eraser result chrome is selected");
    let bounds = row.compute_bounds(&d.w.window).unwrap();
    let point = d.point(&row);
    let picked = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
        .map(|widget| (widget.widget_name(), widget.type_().name()));
    d.input
        .perform(serde_json::json!([{"touch":"down","point":point},{"touch":"up"}]));
    assert!(
        state(&d.w).command_search.is_none(),
        "touch invokes the selected command: bounds={bounds:?} point={point:?} picked={picked:?} requested={requested} shown={} current={} row={}x{}",
        d.w.chrome_released.get(), d.w.chrome.revision(), row.width(), row.height()
    );
    assert_eq!(state(&d.w).brush.tool, Tool::Eraser);
    eprintln!("GTK command query/update timings: {timings:?}");
    let mut open_paint = Vec::new();
    let mut query_paint = Vec::new();
    for i in 0..20 {
        let start = Instant::now();
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::SearchCommands,
        });
        wait_for_command_paint(&card);
        open_paint.push(start.elapsed());
        let start = Instant::now();
        entry.set_text(["select", "undo", "brush size", "pencil"][i % 4]);
        wait_for_command_paint(&card);
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

#[test]
#[ignore = "isolated native-input.js --native-test=native_command_bar_capture --native-storage"]
fn native_command_bar_capture() {
    let mut d = Driver::managed("art.capycanvas.CommandBarCapture");
    command_search_compact_keyboard(&mut d);
    command_search_contacts_and_focus(&mut d);
    d.w.window.close();
    pump(160);
}

fn command_card_bounds(w: &Workspace, card: &gtk::Box) -> [f32; 4] {
    let bounds = card.compute_bounds(&w.surface).expect("mapped command card");
    [bounds.x(), bounds.y(), bounds.width(), bounds.height()]
}

fn wait_for_command_close(d: &Driver) {
    let capture = d.named("command-search-capture");
    until(|| !capture.is_visible(), "closing command chrome is retired");
    assert!(!d.w.command_bar.owns_input(), "retired command search releases input");
}

fn command_search_compact_keyboard(d: &mut Driver) {
    let maximized = d.w.window.is_maximized();
    let size = [d.w.window.width(), d.w.window.height()];
    d.w.window.unmaximize();
    d.w.window.set_default_size(640, 480);
    until(|| d.w.window.width() == 640 && d.w.window.height() == 480, "compact command window allocation");
    d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
    let card = d.named("command-bar").downcast::<gtk::Box>().unwrap();
    let entry = d.named("command-search");
    entry.clone().downcast::<gtk::SearchEntry>().unwrap().set_text("a");
    let list = d.named("command-results");
    let viewport = list.ancestor(gtk::Viewport::static_type()).and_downcast::<gtk::Viewport>().unwrap();
    wait_for_command_paint(&card);
    assert_eq!(state(&d.w).command_search.as_ref().unwrap().results.len(), 8);
    assert!(viewport.height() > 0 && viewport.height() < list.height(), "compact results require scrolling");
    let w = d.w.clone();
    let visible = |selected| {
        assert_eq!(state(&w).command_search.as_ref().unwrap().selected, selected);
        let row = find_named(&list, &format!("command-result-{selected}")).unwrap();
        until(|| row.compute_bounds(&viewport).is_some_and(|bounds| bounds.y() >= -1.
            && bounds.y() + bounds.height() <= viewport.height() as f32 + 1.), "selected command scrolls fully into view");
        let bounds = row.compute_bounds(&viewport).unwrap();
        assert!(bounds.y() >= -1. && bounds.y() + bounds.height() <= viewport.height() as f32 + 1.,
            "selected result {selected} is fully visible: {bounds:?}, viewport height {}", viewport.height());
        assert!(gtk::prelude::GtkWindowExt::focus(&w.window)
            .is_some_and(|focus| focus == entry || focus.is_ancestor(&entry)), "compact navigation keeps entry focus");
    };
    visible(0);
    for selected in 1..8 {
        d.input.key(0xff54);
        visible(selected);
    }
    d.input.key(0xff1b);
    wait_for_command_close(d);
    d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
    visible(0);
    entry.clone().downcast::<gtk::SearchEntry>().unwrap().set_text("a");
    assert_eq!(state(&d.w).command_search.as_ref().unwrap().results.len(), 8);
    for selected in 1..8 {
        d.input.key(0xff54);
        visible(selected);
    }
    for selected in (0..7).rev() {
        d.input.key(0xff52);
        visible(selected);
    }
    d.input.key(0xff1b);
    wait_for_command_close(d);
    d.w.window.set_default_size(size[0], size[1]);
    if maximized { d.w.window.maximize(); }
    until(|| d.w.window.width() == size[0] && d.w.window.height() == size[1], "restored command window allocation");
}

fn command_search_contacts_and_focus(d: &mut Driver) {
    for theme in [Theme::Light, Theme::Dark] {
        wait_for_command_close(d);
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let previous = find_named(d.w.color_panel.root.upcast_ref(), "color-readout").unwrap();
        until(|| previous.is_mapped(), "editor color readout is mapped before command search");
        assert!(previous.can_focus() && previous.is_sensitive() && previous.grab_focus(),
            "{theme:?}: color-readout accepts editor focus: {:?}",
            std::iter::successors(Some(previous.clone()), |widget| widget.parent())
                .map(|widget| (widget.widget_name(), widget.type_().name(), widget.is_mapped(), widget.is_sensitive(), widget.can_focus(), widget.is_focusable()))
                .collect::<Vec<_>>());
        let previous = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        let card = d.named("command-bar").downcast::<gtk::Box>().unwrap();
        let entry = d.named("command-search");
        let results = d.named("command-results");
        wait_for_command_paint(&card);
        until(|| entry.is_mapped() && entry.width() > 0 && entry.height() > 0
            && d.w.chrome_released.get() == d.w.chrome.revision(), "command focus traversal chrome is selected");
        let entry_focused = || gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .is_some_and(|focus| focus == entry || focus.is_ancestor(&entry));
        for reverse in [false, true] {
            assert!(entry.grab_focus());
            let mut visited = Vec::new();
            for _ in 0..state(&d.w).command_search.as_ref().unwrap().results.len() + 8 {
                if reverse {
                    d.input.perform(serde_json::json!([
                        {"key":0xffe1,"down":true}, {"key":0xff09,"down":true},
                        {"key":0xff09,"down":false}, {"key":0xffe1,"down":false}
                    ]));
                } else { d.input.key(0xff09); }
                let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).expect("command card focus");
                assert!(focus.is_ancestor(&card), "{theme:?}: focus stays in the command card");
                if !visited.contains(&focus) { visited.push(focus); }
                if entry_focused() { break; }
            }
            assert!(entry_focused() && visited.len() > 1, "{theme:?}: native focus traversal wraps reverse={reverse}");
            assert!(visited.iter().any(|focus| focus.is_ancestor(&results)), "{theme:?}: result rows remain in native focus traversal");
        }
        d.input.key(0xff1b);
        wait_for_command_close(d);
        assert_eq!(gtk::prelude::GtkWindowExt::focus(&d.w.window), Some(previous.clone()), "{theme:?}: previous focus restored");
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        d.w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close });
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        wait_for_command_paint(&card);
        assert!(state(&d.w).command_search.is_some() && card.is_mapped() && entry_focused(),
            "{theme:?}: reopening cancels the retained close");
        let close = d.point(&d.named("command-search-close"));
        d.input.click(close);
        wait_for_command_close(d);
        assert_eq!(gtk::prelude::GtkWindowExt::focus(&d.w.window), Some(previous), "{theme:?}: reopened search retains prior focus");
        let point = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.5, 0.9]);
        let end = [point[0] + 60., point[1] - 25.];
        for device in ["mouse", "touch"] {
            for drag in [false, true] {
                d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
                let bounds = card.compute_bounds(&d.w.window).unwrap();
                assert!(!bounds.contains_point(&gtk::graphene::Point::new(point[0], point[1])));
                assert!(!bounds.contains_point(&gtk::graphene::Point::new(end[0], end[1])));
                let revision = ui_session(&d.w).engine().document().revision;
                let camera = serde_json::to_value(state(&d.w).camera).unwrap();
                let mut events = vec![contact(device, "down", point)];
                if drag { events.push(contact(device, "move", end)); }
                events.push(contact(device, "up", end));
                d.input.perform(serde_json::Value::Array(events));
                assert!(state(&d.w).command_search.is_none(), "{theme:?}: outside {device} drag={drag} dismisses");
                wait_for_command_close(d);
                assert_eq!(ui_session(&d.w).engine().document().revision, revision, "outside contact never paints");
                assert_eq!(serde_json::to_value(state(&d.w).camera).unwrap(), camera, "outside contact never moves canvas");
            }
        }
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        let revision = ui_session(&d.w).engine().document().revision;
        let camera = serde_json::to_value(state(&d.w).camera).unwrap();
        d.input.perform(serde_json::json!([
            {"point":point}, {"button":272,"down":true},
            {"button":273,"down":true}, {"button":272,"down":false}
        ]));
        assert!(d.named("command-search-capture").is_visible() && d.w.command_bar.owns_input(),
            "{theme:?}: the second outside button retains capture after the first releases");
        d.input.perform(serde_json::json!([{"button":273,"down":false}]));
        wait_for_command_close(d);
        assert_eq!(ui_session(&d.w).engine().document().revision, revision, "outside mouse chord never paints");
        assert_eq!(serde_json::to_value(state(&d.w).camera).unwrap(), camera, "outside mouse chord never moves canvas");
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        let revision = ui_session(&d.w).engine().document().revision;
        let camera = serde_json::to_value(state(&d.w).camera).unwrap();
        d.input.perform(serde_json::json!([{"point":point},{"wheel":[0,1]},{"wheel":[1,-1]}]));
        assert_eq!(ui_session(&d.w).engine().document().revision, revision, "outside scroll never paints");
        assert_eq!(serde_json::to_value(state(&d.w).camera).unwrap(), camera, "outside scroll never moves canvas");
        d.w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close });
        wait_for_command_close(d);
        d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
            id: PreferenceId::ZenShowCapy, value: PreferenceValue::Bool(false),
        }});
        d.w.area.grab_focus();
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        entry.clone().downcast::<gtk::SearchEntry>().unwrap().set_text("zen mode");
        assert_eq!(state(&d.w).command_search.as_ref().unwrap().results[0].id, "command.zen_mode");
        d.input.key(0xff0d);
        wait_for_command_close(d);
        assert!(state(&d.w).workspace.zen_mode && state(&d.w).command_search.is_none(), "{theme:?}: executing Zen closes search");
        d.input.key(0xff1b);
        assert!(!state(&d.w).workspace.zen_mode && state(&d.w).command_search.is_none(), "{theme:?}: closed search releases Escape to Zen");
    }
}

fn command_search_native_composition(d: &mut Driver) {
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for key in [0xff52, 0xff54, 0xff1b, 0xff0d, 0xff09, 0xfe20] {
            d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
            let entry = d.named("command-search").downcast::<gtk::SearchEntry>().unwrap();
            entry.set_text("");
            let text = entry.delegate().and_downcast::<gtk::Text>().unwrap();
            text.set_im_module(Some("gtk-im-context-simple"));
            let marked = Rc::new(Cell::new(false));
            let signal = text.connect_preedit_changed(glib::clone!(#[strong] marked,
                move |_, preedit| if !preedit.is_empty() { marked.set(true); }));
            assert!(entry.grab_focus());
            let selected = state(&d.w).command_search.as_ref().unwrap().selected;
            d.input.perform(serde_json::json!([
                {"key":0xffe3,"down":true}, {"key":0xffe1,"down":true},
                {"key":0x55,"down":true}, {"key":0x55,"down":false},
                {"key":0xffe1,"down":false}, {"key":0xffe3,"down":false}
            ]));
            assert!(marked.get() && d.w.command_bar.composition_busy(), "{theme:?}: native simple IM preedit starts");
            for digit in "3042".chars() { d.input.key(digit as u32); }
            if key == 0xfe20 {
                d.input.perform(serde_json::json!([
                    {"key":0xffe1,"down":true}, {"key":0xff09,"down":true},
                    {"key":0xff09,"down":false}, {"key":0xffe1,"down":false}
                ]));
            } else { d.input.key(key); }
            assert!(state(&d.w).command_search.is_some(), "native IM-owned key never closes or executes search");
            assert_eq!(state(&d.w).command_search.as_ref().unwrap().selected, selected, "native IM-owned key never navigates results");
            assert!(gtk::prelude::GtkWindowExt::focus(&d.w.window)
                .is_some_and(|focus| focus == entry || focus.is_ancestor(&entry)), "native IM-owned key keeps entry focus");
            if key == 0xff0d { assert_eq!(entry.text(), "あ", "native IM commits once through Return"); }
            text.disconnect(signal);
            eprintln!("Native GtkIMContextSimple ownership: {theme:?} key={key:#x}");
            d.w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Close });
            pump(150);
        }
    }
}

fn wait_for_command_paint(widget: &impl IsA<gtk::Widget>) {
    let clock = widget.frame_clock().unwrap();
    let painted = Rc::new(Cell::new(false));
    let signal = clock.connect_after_paint(glib::clone!(
        #[strong]
        painted,
        move |_| painted.set(true)
    ));
    widget.queue_draw();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !painted.get() {
        assert!(Instant::now() < deadline, "command bar paints");
        pump(1);
    }
    clock.disconnect(signal);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_command_bar_glass with LAYER_NATIVE_CAPTURE_DIR"]
fn native_command_bar_glass() {
    use layer_core::DefaultBrushPreset;
    let captures = std::path::PathBuf::from(
        std::env::var("LAYER_NATIVE_CAPTURE_DIR").expect("compositor captures"),
    );
    let mut d = Driver::managed("art.capycanvas.CommandBarGlass");
    d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    for _ in 0..3 {
        d.w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    }
    pump(300);
    let camera = state(&d.w).camera;
    let [a, b, c, e, x0, y0] = camera.document_to_surface();
    let det = a * e - b * c;
    let document = |x: f32, y: f32| {
        let (x, y) = (x - x0, y - y0);
        [(e * x - c * y) / det, (a * y - b * x) / det]
    };
    let [width, height] = [d.w.surface.width() as f32, d.w.surface.height() as f32];
    let top = COMMAND_SEARCH_STYLE.top(height);
    d.w.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 });
    d.w.dispatch(UiAction::SetBrushSize { value: 6. / det.abs().sqrt() });
    d.w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
    for i in 0..=54 {
        let x = width / 2. - 351. + 13. * i as f32;
        native_pen_path(
            &d.w,
            &[document(x, top - 150.), document(x, top + 150.), document(x, top + 480.)],
        );
    }
    let origin = d.w.surface.compute_point(&d.w.window, &gtk::graphene::Point::zero()).unwrap();
    let (window_x, window_y) = d.w.window.surface_transform();
    let offset = [origin.x() + window_x as f32, origin.y() + window_y as f32];
    let capture = |d: &mut Driver, name: &str| {
        d.input.perform(serde_json::json!([{ "wait_ms": 300 }, { "capture": name }]));
        let texture = gdk::Texture::from_filename(captures.join(format!("{name}.png"))).unwrap();
        let stride = texture.width() as usize * 4;
        let mut pixels = vec![0; stride * texture.height() as usize];
        texture.download(&mut pixels, stride);
        move |y: f32, x: [f32; 2]| -> Vec<[f32; 3]> {
            let y = (y + offset[1]).round() as usize;
            ((x[0] + offset[0]).round() as usize..(x[1] + offset[0]).round() as usize)
                .map(|x| {
                    let p = &pixels[y * stride + x * 4..];
                    [p[2], p[1], p[0]].map(|v| v as f32 / 255.)
                })
                .collect()
        }
    };
    let mean = |row: &[[f32; 3]]| -> [f32; 3] {
        std::array::from_fn(|i| row.iter().map(|p| p[i]).sum::<f32>() / row.len() as f32)
    };
    let sharpness = |row: &[[f32; 3]]| -> f32 {
        let luma = |p: &[f32; 3]| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        row.windows(2).map(|p| (luma(&p[1]) - luma(&p[0])).abs()).sum::<f32>() / (row.len() - 1) as f32
    };
    let close = |a: [f32; 3], b: [f32; 3], tolerance: f32| a.iter().zip(b).all(|(a, b)| (a - b).abs() <= tolerance);
    let entry = d.named("command-search").downcast::<gtk::SearchEntry>().unwrap();
    use layer_ui::Transparency as T;
    for (theme, level) in [
        (Theme::Light, T::Medium), (Theme::Light, T::Off),
        (Theme::Dark, T::Medium), (Theme::Dark, T::Off),
    ] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        use_transparency(&d.w, level);
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        pump(300);
        let name = format!("command-bar-glass-{theme:?}-{level:?}").to_lowercase();
        let card = d.named("command-bar").downcast::<gtk::Box>().unwrap();
        let body = command_card_bounds(&d.w, &card);
        let span = [body[0] + 16., body[0] + body[2] - 16.];
        let open = capture(&mut d, &name);
        let palette = state(&d.w).palette;
        let behind = mean(&open(body[1] - 60., span));
        assert!(sharpness(&open(body[1] - 60., span)) > 0.05, "{name}: stripes surround the bar");
        let glass = palette.glass.panel.0;
        let expected = std::array::from_fn(|i| glass[i] * glass[3] + behind[i] * (1. - glass[3]));
        for y in [body[1] + 6., body[1] + body[3] - 6.] {
            let row = open(y, span);
            assert!(close(mean(&row), expected, 0.04), "{name} y={y}: {:?} vs {expected:?}", mean(&row));
            assert!(sharpness(&row) < 0.01, "{name} y={y}: glass must blur, not show, the stripes");
        }
        assert!(entry.text().is_empty());
        for _ in 0..4 { d.input.key(0x7a); }
        pump(300);
        let shrunk = command_card_bounds(&d.w, &card);
        assert!(shrunk[3] + 60. < body[3]);
        let fewer = capture(&mut d, &format!("{name}-empty"));
        let row = fewer(shrunk[1] + shrunk[3] - 6., span);
        assert!(close(mean(&row), expected, 0.04), "{name} resized: {:?} vs {expected:?}", mean(&row));
        let vacated = fewer(body[1] + body[3] - 6., span);
        assert!(sharpness(&vacated) > 0.05, "{name}: no stale blur below the resized bar");
        d.input.key(0xff1b);
        assert!(state(&d.w).command_search.is_none());
        let closed = capture(&mut d, &format!("{name}-closed"));
        for y in [body[1] + 6., body[1] + body[3] - 6.] {
            assert!(sharpness(&closed(y, span)) > 0.05, "{name} y={y}: no stale blur after closing");
        }
    }
    d.w.window.close();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_command_bar_preedit_guard --native-storage"]
fn native_command_bar_preedit_guard() {
    let mut d = Driver::managed("art.capycanvas.CommandPreedit");
    assert!(std::sync::Arc::ptr_eq(&d.w.localization(), crate::launch_localization()));
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        eprintln!("Synthetic GtkText preedit signal ownership: {theme:?}");
        for preedit in ["にほんご", "简体", "繁體", "한국어"] {
            d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
            let entry = d.named("command-search").downcast::<gtk::SearchEntry>().unwrap();
            entry.set_text("fit canvas");
            let text = entry.delegate().and_downcast::<gtk::Text>().unwrap();
            let capture = d.named("command-search-capture").downcast::<gtk::Overlay>().unwrap();
            let controllers = capture.observe_controllers();
            let controller = |name: &str| (0..controllers.n_items()).find_map(|i| {
                controllers.item(i).and_downcast::<gtk::EventControllerKey>()
                    .filter(|controller| controller.name().as_deref() == Some(name))
            }).unwrap();
            let keys = controller("command-search-navigation");
            let bubble = controller("command-search-composed-key");
            let before = serde_json::to_value(state(&d.w).command_search).unwrap();
            text.emit_preedit_changed(preedit);
            for (key, code) in [(gdk::Key::Up, 111u32), (gdk::Key::Down, 116), (gdk::Key::Escape, 9), (gdk::Key::Tab, 23), (gdk::Key::ISO_Left_Tab, 23)] {
                assert!(!keys.emit_by_name::<bool>("key-pressed", &[&key, &code, &gdk::ModifierType::empty()]));
                assert!(bubble.emit_by_name::<bool>("key-pressed", &[&key, &code, &gdk::ModifierType::empty()]));
                keys.emit_by_name::<()>("key-released", &[&key, &code, &gdk::ModifierType::empty()]);
                assert_eq!(serde_json::to_value(state(&d.w).command_search).unwrap(), before);
            }
            assert!(!keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            text.emit_preedit_changed("");
            entry.emit_by_name::<()>("activate", &[]);
            assert_eq!(serde_json::to_value(state(&d.w).command_search).unwrap(), before);
            assert!(!keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            entry.emit_by_name::<()>("activate", &[]);
            assert!(bubble.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            assert_eq!(serde_json::to_value(state(&d.w).command_search).unwrap(), before);
            keys.emit_by_name::<()>("key-released", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]);
            assert!(!keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Return, &36u32, &gdk::ModifierType::empty()]));
            entry.emit_by_name::<()>("activate", &[]);
            assert!(state(&d.w).command_search.is_none());
            d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
            text.emit_preedit_changed(preedit);
            assert!(!keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
            text.emit_preedit_changed("");
            assert!(bubble.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
            assert!(state(&d.w).command_search.is_some());
            keys.emit_by_name::<()>("key-released", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]);
            assert!(keys.emit_by_name::<bool>("key-pressed", &[&gdk::Key::Escape, &9u32, &gdk::ModifierType::empty()]));
            assert!(state(&d.w).command_search.is_none());
        }
    }
    command_search_native_composition(&mut d);
    d.w.window.close();
    pump(160);
}
