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
        d.perform(serde_json::json!([{ "wait_ms": 300 }, { "capture": name }]));
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
        (Theme::Dark, T::Off),
        (Theme::Dark, T::Low),
        (Theme::Dark, T::High),
        (Theme::Light, T::Medium),
        (Theme::Light, T::High),
        (Theme::Light, T::Off),
    ] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        use_transparency(&d.w, level);
        d.w.dispatch(UiAction::Invoke { command: CommandId::SearchCommands });
        pump(300);
        let name = format!("command-bar-glass-{theme:?}-{level:?}").to_lowercase();
        let body = d.w.command_bar.glass(&d.w).expect("mapped command bar").bounds;
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
        entry.set_text("zzzz");
        pump(300);
        let shrunk = d.w.command_bar.glass(&d.w).unwrap().bounds;
        assert!(shrunk[3] + 60. < body[3]);
        let fewer = capture(&mut d, &format!("{name}-empty"));
        let row = fewer(shrunk[1] + shrunk[3] - 6., span);
        assert!(close(mean(&row), expected, 0.04), "{name} resized: {:?} vs {expected:?}", mean(&row));
        let vacated = fewer(body[1] + body[3] - 6., span);
        assert!(sharpness(&vacated) > 0.05, "{name}: no stale blur below the resized bar");
        d.key(0xff1b);
        assert!(state(&d.w).command_search.is_none());
        let closed = capture(&mut d, &format!("{name}-closed"));
        for y in [body[1] + 6., body[1] + body[3] - 6.] {
            assert!(sharpness(&closed(y, span)) > 0.05, "{name} y={y}: no stale blur after closing");
        }
    }
    d.w.window.close();
}
