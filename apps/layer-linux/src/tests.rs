//! Native control/lifecycle integration on a hardware desktop. Control signals
//! exercise GTK bindings; pen records exercise scheduling and GPU presentation.
//! Physical tablet/touch delivery remains a human test (not faked here).
use super::*;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use std::time::{Duration, Instant};

fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    let context = glib::MainContext::default();
    while Instant::now() < until {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn state(w: &Workspace) -> UiState {
    w.gpu.borrow().as_ref().unwrap().session.state().clone()
}

fn native_test_app(id: &str) -> adw::Application {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id(id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    app
}

fn command(w: &Workspace, id: CommandId) -> gtk::Button {
    w.commands
        .borrow()
        .iter()
        .find(|(c, _)| *c == id)
        .unwrap()
        .1
        .clone()
}
fn click(button: &gtk::Button) {
    assert!(button.is_sensitive());
    button.emit_clicked();
    pump(100);
}
fn white_pixels(w: &Workspace) -> usize {
    // Inspect the same whole-window scene as the review PNG, not a fresh
    // standalone canvas snapshot that could hide host invalidation errors.
    let texture = crate::snapshot(w);
    canvas_white(w, &texture)
}
fn canvas_white(w: &Workspace, texture: &gdk::Texture) -> usize {
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    let bounds = w.area.compute_bounds(&w.window).unwrap();
    let stride = texture.width() as usize * 4;
    (bounds.y() as usize..(bounds.y() + bounds.height()) as usize)
        .map(|y| {
            let row = &bytes[y * stride + bounds.x() as usize * 4
                ..y * stride + (bounds.x() + bounds.width()) as usize * 4];
            row.chunks_exact(4)
                .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245 && p[3] > 245)
                .count()
        })
        .sum()
}

fn capture_reference(w: &Workspace, path: &str, scale: f32) {
    // Snapshot the actual native dialog/content subtree. This reference
    // measures GTK widgets, not compositor delivery or OS window shadows.
    crate::with_canvas_snapshot(w, || {
        let snapshot = gtk::Snapshot::new();
        snapshot.scale(scale, scale);
        let content = w.window.child().unwrap();
        content.allocate(w.window.width(), w.window.height(), -1, None);
        w.window.snapshot_child(&content, &snapshot);
        let node = snapshot.to_node().expect("allocated native content");
        w.window
            .renderer()
            .unwrap()
            .render_texture(
                &node,
                Some(&gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    w.window.width() as f32 * scale,
                    w.window.height() as f32 * scale,
                )),
            )
            .save_to_png(path)
            .unwrap();
    });
}

#[test]
#[ignore = "GTK visual reference for the web: requires a Wayland display"]
fn native_web_parity_reference() {
    let app = native_test_app("dev.layer.ParityTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    std::fs::create_dir_all("../../artifacts/ui/parity").unwrap();
    fn record(widget: &gtk::Widget, root: &gtk::Widget) -> serde_json::Value {
        let b = widget.compute_bounds(root).unwrap();
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_visible() && c.is_child_visible() {
                children.push(record(&c, root));
            }
        }
        serde_json::json!({
            "type": widget.type_().name(),
            "name": widget.widget_name().to_string(),
            "css": widget.css_classes().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "bounds": [b.x(), b.y(), b.width(), b.height()],
            "font": widget.pango_context().font_description().map(|f| f.to_string()),
            "text": widget.downcast_ref::<gtk::Label>().map(|l| l.text().to_string()),
            "icon": widget.downcast_ref::<gtk::Image>().and_then(|i| i.icon_name()).map(|s| s.to_string()),
            "children": children,
        })
    }
    for (scheme, name) in [
        (adw::ColorScheme::ForceDark, "dark"),
        (adw::ColorScheme::ForceLight, "light"),
    ] {
        for modal in [false, true] {
            adw::StyleManager::default().set_color_scheme(scheme);
            adw::StyleManager::for_display(&gdk::Display::default().unwrap())
                .set_color_scheme(adw::ColorScheme::Default);
            let w = Workspace::new(&app);
            w.window.present();
            assert!(w.gpu.borrow().is_some());
            if modal {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Settings,
                });
            }
            pump(2500);
            let name = if modal {
                format!("settings-{name}")
            } else {
                name.to_string()
            };
            capture_reference(
                &w,
                &format!("../../artifacts/ui/parity/gtk-{name}.png"),
                1.0,
            );
            if !modal {
                capture_reference(
                    &w,
                    &format!("../../artifacts/ui/parity/gtk-{name}-2x.png"),
                    2.0,
                );
            }
            let mut reference = record(w.window.upcast_ref(), w.window.upcast_ref());
            reference["scale"] = serde_json::json!(w.area.scale_factor());
            reference["camera"] = serde_json::to_value(state(&w).camera).unwrap();
            std::fs::write(
                format!("../../artifacts/ui/parity/gtk-{name}.json"),
                serde_json::to_vec(&reference).unwrap(),
            )
            .unwrap();
            w.window.destroy();
            pump(100);
        }
    }
}

#[test]
#[ignore = "native GTK widgets: requires a Wayland display"]
fn native_ribbon_allocation() {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let strip = TileStrip::new();
    strip.add_css_class("toolbar-controls");
    for _ in 0..6 {
        strip.append(&gtk::Button::from_icon_name("document-edit-symbolic"));
    }
    strip.set_grip(&tiles::grip());
    for (axis, edge) in [(Axis::Horizontal, Edge::Top), (Axis::Vertical, Edge::Left)] {
        let mut layout = DockLayout::default();
        layout.bands.retain(|b| b.id == 1);
        layout.bands[0].edge = edge;
        strip.configure(axis, true);
        for length in [96.0, 500.0] {
            let viewport = if axis == Axis::Horizontal {
                [length, 800.0]
            } else {
                [800.0, length]
            };
            let g = layout.resolve(viewport[0], viewport[1]).groups.remove(0);
            strip.allocate(g.bounds.width as i32, g.bounds.height as i32, -1, None);
            let expected = tile_layout(g.bounds.width, g.bounds.height, axis, 6, true);
            let mut child = strip.first_child();
            for b in expected.tiles.into_iter().chain(expected.grip) {
                let widget = child.unwrap();
                let actual = widget.compute_bounds(&strip).unwrap();
                assert_eq!(
                    (actual.x(), actual.y(), actual.width(), actual.height()),
                    (b.x, b.y, b.width, b.height)
                );
                child = widget.next_sibling();
            }
            assert!(child.is_none());
            assert_eq!(layout.bands[0].extent, TILE_SIZE + 6.0);
        }
    }
}

#[test]
#[ignore = "native sidebar, shortcut recording and persistence: requires a Wayland display"]
fn native_preferences_and_shortcuts() {
    if let Some(path) = std::env::var_os("LAYER_SETTINGS_FILE") {
        assert!(
            !std::path::Path::new(&path).exists(),
            "Use a fresh isolated preferences path for this test"
        );
    }
    let app = native_test_app("dev.layer.PreferencesTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    let w = windows.borrow()[0].clone();
    pump(300);
    assert_eq!(w.window.title().as_deref(), Some(APP_NAME));
    click(&command(&w, CommandId::NewWindow));
    assert_eq!(windows.borrow().len(), 2);
    let second = windows.borrow()[1].clone();
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(100);
    assert_eq!(state(&second).settings, state(&w).settings);
    second.window.destroy();
    pump(100);
    assert_eq!(windows.borrow().len(), 1);
    let dir = "../../artifacts/ui/preferences";
    std::fs::create_dir_all(dir).unwrap();
    for (theme, suffix) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for page in SettingsPage::ALL {
            w.dispatch(UiAction::OpenSettings { page });
            pump(400);
            assert_eq!(
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .preferences()
                    .unwrap()
                    .page,
                page
            );
            capture_reference(&w, &format!("{dir}/gtk-{}-{suffix}.png", page.key()), 1.0);
        }
        w.dispatch(UiAction::CancelSettings);
        pump(250);
    }
    click(&command(&w, CommandId::KeyboardShortcuts));
    let search: gtk::SearchEntry = find_named(w.preferences.dialog.upcast_ref(), "settings-search")
        .unwrap()
        .downcast()
        .unwrap();
    search.set_text("pressure response");
    pump(300);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .preferences()
            .unwrap()
            .page,
        SettingsPage::Input
    );
    search.set_text("");
    pump(300);
    let feedback: adw::SwitchRow =
        find_named(w.preferences.dialog.upcast_ref(), "setting-feedback")
            .unwrap()
            .downcast()
            .unwrap();
    feedback.set_active(false);
    assert!(
        !find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-prediction-horizon"
        )
        .unwrap()
        .is_sensitive()
    );
    feedback.set_active(true);
    assert!(
        find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-platform-prediction"
        )
        .is_none()
    );
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Shortcuts,
    });
    let row: adw::ActionRow =
        find_named(w.preferences.dialog.upcast_ref(), "shortcut-command.Brush")
            .unwrap()
            .downcast()
            .unwrap();
    row.emit_by_name::<()>("activated", &[]);
    pump(200);
    let controllers = w.window.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .unwrap()
                .downcast::<gtk::EventControllerKey>()
                .ok()
        })
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()],
    );
    assert_eq!(
        state(&w).preferences.capture.unwrap().conflict.as_deref(),
        Some("Eraser")
    );
    capture_reference(&w, &format!("{dir}/gtk-shortcut-conflict.png"), 1.0);
    let confirm: gtk::Button = find_named(w.window.upcast_ref(), "confirm-shortcut")
        .unwrap()
        .downcast()
        .unwrap();
    click(&confirm);
    assert!(state(&w).preferences.capture.is_none());
    assert_eq!(
        state(&w).settings_draft.as_ref().unwrap().shortcuts["command.Brush"][0].key,
        "e"
    );
    click(&find_button(w.preferences.dialog.upcast_ref(), "Apply").unwrap());
    assert!(
        state(&w).requests.is_empty(),
        "host acknowledged the saved snapshot"
    );
    assert!(
        w.shortcut_hints
            .borrow()
            .iter()
            .filter(|(id, _)| *id == CommandId::Settings)
            .all(|(_, label)| label.text() == "Ctrl+,")
    );
    // Explicit override isolates persistence from the user's actual config.
    if std::env::var_os("LAYER_SETTINGS_FILE").is_some() {
        assert_eq!(
            crate::preferences::load().unwrap().unwrap(),
            state(&w).settings
        );
        click(&command(&w, CommandId::NewWindow));
        let next = windows.borrow().last().unwrap().clone();
        assert_eq!(state(&next).settings, state(&w).settings);
        next.window.destroy();
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Canvas,
    });
    w.window.set_default_size(640, 600);
    pump(500);
    capture_reference(&w, &format!("{dir}/gtk-narrow.png"), 1.0);
    w.window.destroy();
    pump(200);
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "cursor vectors: requires a Wayland/Vulkan display"]
fn native_cursor_vectors() {
    let app = native_test_app("dev.layer.CursorTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1800);
    let theme = gtk::IconTheme::for_display(&w.area.display());
    for &icon in ui_catalog().icons {
        for scale in [1, 2] {
            let asset = theme.lookup_icon(
                &format!("layer-{icon}-symbolic"),
                &[],
                16,
                scale,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::FORCE_SYMBOLIC,
            );
            assert!(asset.is_symbolic());
            assert!(
                asset
                    .file()
                    .unwrap()
                    .uri()
                    .starts_with("resource:///dev/layer/icons/"),
                "{icon} must come from the shared bank"
            );
        }
    }
    std::fs::create_dir_all("../../artifacts/ui/cursors").unwrap();
    let scale = w.area.scale_factor() as f32;
    assert_eq!(
        w.area.cursor().and_then(|c| c.name()).as_deref(),
        Some("none")
    );
    let mut report = Vec::new();
    for (preset, label) in [
        (layer_core::DefaultBrushPreset::GPen, "round"),
        (layer_core::DefaultBrushPreset::TexturedFlat, "flat"),
        (layer_core::DefaultBrushPreset::WatercolorWash, "watercolor"),
    ] {
        w.dispatch(UiAction::SelectBrush { id: preset as u32 });
        w.dispatch(UiAction::SetBrushSize { value: 512.0 });
        pump(150);
        w.cursor_input(Some(PenEvent {
            device_id: 123,
            sequence: 1,
            timestamp_ns: 1_000_000_000_000,
            view_revision: state(&w).camera.revision,
            surface_position: Point {
                x: 600.0 * scale,
                y: 460.0 * scale,
            },
            pressure: 0.0,
            tilt_radians: [0.3, 0.6],
            twist_radians: 0.5,
            distance: 0.0,
            phase: PenPhase::Hover,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }));
        let shape = w
            .gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .canvas_cursor()
            .unwrap();
        assert!(!shape.outline.is_empty());
        assert!(shape.segments.len() > 20);
        assert!(
            shape
                .segments
                .iter()
                .all(|s| s.from.into_iter().chain(s.to).all(f32::is_finite))
        );
        pump(50);
        capture_reference(
            &w,
            &format!("../../artifacts/ui/cursors/gtk-{label}.png"),
            1.0,
        );
        let mut times = Vec::new();
        for _ in 0..1000 {
            let start = Instant::now();
            let shape = w
                .gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .canvas_cursor()
                .unwrap();
            std::hint::black_box(&shape.segments);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        report.push(serde_json::json!({"brush":label,"median_ms":times[500],"p95_ms":times[950],"p99_ms":times[990],"segments":shape.segments.len()}));
        assert!(
            times[990] < 8.333,
            "cursor preparation must fit a 120Hz frame"
        );
    }
    std::fs::write(
        "../../artifacts/ui/cursors/native-preparation.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    w.cursor_input(None);
    assert!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .canvas_cursor()
            .is_none()
    );
    w.window.destroy();
}

#[test]
#[ignore = "workspace restore integration: requires a Wayland display"]
fn native_workspace_restore() {
    let app = native_test_app("dev.layer.RestoreTest");
    let source = Workspace::new(&app);
    source.window.present();
    pump(1000);
    source.dispatch(UiAction::MovePanel {
        panel: Panel::Toolbar,
        target: DockTarget::Tab {
            group: 5,
            index: Some(0),
        },
        viewport: [1200.0, 900.0],
    });
    source.dispatch(UiAction::ResizeDock {
        id: 3,
        position: [320.0, 450.0],
        viewport: [1200.0, 900.0],
    });
    source.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(150);
    let saved = serde_json::to_string(&state(&source).workspace).unwrap();
    let geometry = |w: &Workspace| {
        w.groups
            .borrow()
            .iter()
            .map(|g| {
                let b = g
                    .stack
                    .parent()
                    .unwrap()
                    .compute_bounds(&w.surface)
                    .unwrap();
                (
                    g.id,
                    g.panels.clone(),
                    g.stack.visible_child_name(),
                    [b.x(), b.y(), b.width(), b.height()],
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = geometry(&source);
    source.window.destroy();
    drop(source);
    pump(100);
    let fresh = Workspace::new(&app);
    fresh.window.present();
    pump(1000);
    fresh.dispatch(UiAction::RestoreWorkspace {
        workspace: serde_json::from_str(&saved).unwrap(),
    });
    pump(150);
    assert_eq!(
        serde_json::to_string(&state(&fresh).workspace).unwrap(),
        saved
    );
    assert_eq!(geometry(&fresh), expected);
    assert!(command(&fresh, CommandId::ZenMode).has_css_class("selected-tool"));
    assert_eq!(
        fresh
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .tabs
            .len(),
        2
    );
    assert!(!fresh.toolbar.last_child().unwrap().is_child_visible());
    assert!(!fresh.status.is_visible());
    fresh.window.destroy();
    pump(100);
}

#[test]
#[ignore = "hardware Wayland benchmark: run separately in release with --ignored --test-threads=1"]
fn native_frame_pacing() {
    use layer_core::DefaultBrushPreset;
    let app = native_test_app("dev.layer.FramePacingTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1500);
    assert!(
        w.gpu.borrow().is_some(),
        "hardware Vulkan canvas must initialize"
    );
    let bounds = w.area.compute_bounds(&w.surface).unwrap();
    assert_eq!(
        [bounds.x(), bounds.y(), bounds.width(), bounds.height()],
        [
            0.0,
            0.0,
            w.surface.width() as f32,
            w.surface.height() as f32
        ]
    );
    let worker_stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    let mut reports = Vec::new();
    let mut sequence = 0;
    for (name, preset) in [
        ("GPen", Some(DefaultBrushPreset::GPen)),
        ("NaturalBlender", Some(DefaultBrushPreset::NaturalBlender)),
        ("WetRound", Some(DefaultBrushPreset::WetRound)),
        ("WatercolorWash", Some(DefaultBrushPreset::WatercolorWash)),
        ("Pan", None),
    ] {
        if std::env::var("LAYER_PACING_BRUSH").is_ok_and(|s| s != name) {
            continue;
        }
        if let Some(preset) = preset {
            w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            w.dispatch(UiAction::SetBrushSize { value: 384.0 });
        }
        pump(300);
        *worker_stats.lock().unwrap() = Default::default();
        let camera = state(&w).camera;
        let start = Instant::now();
        let mut first = true;
        let mut last_event = None;
        while start.elapsed() < Duration::from_secs(6) {
            let t = start.elapsed().as_secs_f32() * 5.0;
            let event = PenEvent {
                device_id: 1,
                sequence,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: camera.viewport[0] as f32 * (0.5 + 0.23 * t.cos()),
                    y: camera.viewport[1] as f32 * (0.5 + 0.20 * (t * 1.3).sin()),
                },
                pressure: 0.8,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase: if first {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            sequence += 1;
            if preset.is_some() {
                if std::env::var("LAYER_PACING_CURSOR").as_deref() != Ok("0") {
                    w.cursor_input(Some(event));
                }
                w.input.send(&w, event);
            } else {
                w.interact(UiInput::Pointer {
                    id: 55,
                    phase: if first {
                        ContactPhase::Down
                    } else {
                        ContactPhase::Move
                    },
                    kind: PointerKind::Mouse,
                    button: PointerButton::Pan,
                    position: [event.surface_position.x, event.surface_position.y],
                });
            }
            first = false;
            last_event = Some(event);
            pump(2);
        }
        if preset.is_some() {
            w.input.send(
                &w,
                PenEvent {
                    phase: PenPhase::Up,
                    ..last_event.unwrap()
                },
            );
        } else {
            w.interact(UiInput::Pointer {
                id: 55,
                phase: ContactPhase::Up,
                kind: PointerKind::Mouse,
                button: PointerButton::Pan,
                position: [600.0, 450.0],
            });
        }
        pump(150);
        let stats = worker_stats.lock().unwrap();
        assert!(
            stats.cpu.len() > 100,
            "canvas must schedule independently of GTK painting; worker frames = {}, GPU = {}, presented = {}",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        assert!(
            stats.gpu.len() > 100,
            "hardware timestamps must cover GPU rendering"
        );
        assert!(
            stats.presented.iter().filter(|p| p[3] == 1).count() > 100,
            "measure real child-surface presentation"
        );
        let report = serde_json::json!({
            "brush": name, "viewport": camera.viewport, "brush_size": 384, "stroke_seconds": 6,
            "path": "app-owned Wayland Vulkan subsurface",
            "cursor": std::env::var("LAYER_PACING_CURSOR").as_deref() != Ok("0"),
            "input_cpu": stats.input_cpu,
            "input_handler_cpu": stats.input_handler_cpu,
            "frame_handler_cpu": stats.frame_handler_cpu,
            "wake_lateness": stats.wake_lateness,
            "worker_cpu": stats.cpu, "worker_gpu": stats.gpu, "canvas_presentation": stats.presented,
        });
        eprintln!(
            "{name}: {} canvas frames, {} GPU timings, {} presentation feedbacks",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        reports.push(report);
    }
    let path = std::env::var("LAYER_PACING_REPORT")
        .unwrap_or_else(|_| "/tmp/layer-wayland-pacing.json".into());
    std::fs::write(path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    if let Ok(path) = std::env::var("LAYER_PACING_CAPTURE") {
        crate::capture(&w, &path);
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_compositor_input() {
    let report_dir = std::path::PathBuf::from(
        std::env::var("LAYER_NATIVE_INPUT_DIR")
            .expect("run with apps/layer-linux/bench/native-input.js in isolated Mutter"),
    );
    let app = native_test_app("dev.layer.NativeInputTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1500);
    assert!(w.gpu.borrow().is_some());
    let events = Rc::new(RefCell::new(Vec::<[f64; 5]>::new()));
    let motion = gtk::EventControllerLegacy::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_event(glib::clone!(
        #[strong]
        events,
        move |_, event| {
            if event.event_type() == gdk::EventType::MotionNotify {
                let (x, y) = event.position().unwrap();
                events.borrow_mut().push([
                    glib::monotonic_time() as f64 * 1000.0,
                    event.time() as f64,
                    f64::from(
                        event
                            .modifier_state()
                            .contains(gdk::ModifierType::BUTTON2_MASK),
                    ),
                    x,
                    y,
                ]);
            }
            glib::Propagation::Proceed
        }
    ));
    w.area.add_controller(motion);
    let stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    *stats.lock().unwrap() = Default::default();
    let before = state(&w).camera;
    let loop_ = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(
        Duration::from_secs(9),
        glib::clone!(
            #[strong]
            loop_,
            move || loop_.quit()
        ),
    );
    std::fs::write(report_dir.join("ready"), b"ready").unwrap();
    loop_.run();
    let stats = stats.lock().unwrap();
    let report = serde_json::json!({
        "events": &*events.borrow(), "input_cpu": stats.input_cpu, "input_handler_cpu": stats.input_handler_cpu,
        "wake_lateness": stats.wake_lateness, "worker_cpu": stats.cpu, "worker_gpu": stats.gpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    std::fs::write(
        report_dir.join("received.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Native delivery: {} pointer events; {} rendered frames",
        events.borrow().len(),
        stats.cpu.len()
    );
    assert_ne!(
        state(&w).camera,
        before,
        "native input must actually pan the canvas"
    );
    assert!(
        events.borrow().iter().filter(|e| e[2] == 1.0).count() > 720,
        "must receive compositor-originated panning events above 120 Hz"
    );
    drop(stats);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "hardware desktop: run this test separately with --ignored --test-threads=1"]
fn native_workspace_controls_docking_and_ink() {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id("dev.layer.CopilotTest")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let w = Workspace::new(&app);
    w.window.present();
    pump(3000);
    assert!(w.gpu.borrow().is_some());
    assert_eq!(state(&w).settings.theme, None);
    assert_eq!(
        state(&w).theme == Theme::Dark,
        adw::StyleManager::default().is_dark()
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(100);
    assert!(!w.ticking.get(), "idle canvas must stop requesting frames");
    let close = find_css(w.header.upcast_ref(), "close").unwrap();
    let circle = close.first_child().unwrap().compute_bounds(&close).unwrap();
    assert_eq!((circle.width(), circle.height()), (24.0, 24.0));
    let hit = close.compute_bounds(&w.header).unwrap();
    assert!(hit.width() >= 34.0 && hit.height() >= 34.0);
    let circle = close
        .first_child()
        .unwrap()
        .compute_bounds(&w.window)
        .unwrap();
    let right = w.window.width() as f32 - circle.x() - circle.width();
    assert!(
        (10.0..=14.0).contains(&circle.y()) && (10.0..=14.0).contains(&right),
        "native close insets: top={}, right={right}",
        circle.y()
    );
    // Exercise the same native signal callbacks as drawing. Space-pan uses
    // that path instead of a second recognizer competing for left-button input.
    let controllers = w.area.observe_controllers();
    let stylus = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureStylus>())
        .unwrap();
    let before = state(&w).camera;
    w.interact(crate::input::key_input(
        gdk::Key::space,
        true,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("down", &[&600.0f64, &450.0f64]);
    stylus.emit_by_name::<()>("motion", &[&640.0f64, &470.0f64]);
    w.interact(crate::input::key_input(
        gdk::Key::space,
        false,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("motion", &[&660.0f64, &490.0f64]);
    stylus.emit_by_name::<()>("up", &[&660.0f64, &490.0f64]);
    pump(100);
    let dpi = w.area.scale_factor() as f32;
    assert_eq!(
        state(&w).camera.translation,
        [
            before.translation[0] + 60.0 * dpi,
            before.translation[1] + 40.0 * dpi
        ]
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        0
    );
    click(&command(&w, CommandId::FitCanvas));
    let initial = white_pixels(&w);
    assert!(initial > 100_000, "GPU paper should be visibly presented");
    let pencil = w
        .brush_buttons
        .borrow()
        .iter()
        .find(|(_, b)| b.widget_name() == "brush-2")
        .unwrap()
        .clone();
    click(&pencil.1);
    assert_eq!(state(&w).brush.preset, pencil.0);
    let size = w
        .size_buttons
        .borrow()
        .iter()
        .find(|(v, _)| *v == 96.0)
        .unwrap()
        .1
        .clone();
    click(&size);
    assert_eq!(state(&w).brush.diameter, 96.0);
    w.size_number.set_value(84.0);
    assert_eq!(state(&w).brush.diameter, 84.0);
    assert_eq!(w.size.value(), 84.0);
    click(&command(&w, CommandId::AddLayer));
    assert_eq!(state(&w).layers.len(), 3);
    let view = state(&w).camera;
    for i in 0..=36 {
        let t = i as f32 / 36.0;
        let phase = if i == 0 {
            PenPhase::Down
        } else if i == 36 {
            PenPhase::Up
        } else {
            PenPhase::Move
        };
        let event = PenEvent {
            device_id: 1,
            sequence: i + 1,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            view_revision: view.revision,
            surface_position: Point {
                x: view.viewport[0] as f32 * (0.2 + 0.6 * t),
                y: view.viewport[1] as f32 * (0.5 + 0.09 * (t * std::f32::consts::TAU).sin()),
            },
            pressure: 0.3 + 0.7 * (t * std::f32::consts::PI).sin(),
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .pen(event)
            .unwrap();
        w.wake();
        pump(15);
    }
    pump(300);
    let after_ink = white_pixels(&w);
    assert!(
        after_ink + 500 < initial,
        "ink must be visibly presented: {initial} -> {after_ink}"
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        1
    );
    assert_stroke_positions(&w, &crate::snapshot(&w));
    // Presentation must follow the same top-left transform as input even
    // after an asymmetric pan/zoom/rotation (a centered fit hides a Y flip).
    let center = view.viewport.map(|v| v as f32 * 0.5);
    let change = w.gpu.borrow_mut().as_mut().unwrap().session.gesture(
        center,
        [center[0] + 30.0, center[1] + 45.0],
        1.1,
        0.35,
    );
    w.changed(change);
    pump(150);
    assert_stroke_positions(&w, &crate::snapshot(&w));
    click(&command(&w, CommandId::FitCanvas));
    click(&command(&w, CommandId::Undo));
    assert!(white_pixels(&w) > after_ink + 500);
    click(&command(&w, CommandId::Redo));
    assert!(white_pixels(&w) < initial - 500);
    let visibility = || {
        w.layers
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::CheckButton>()
            .unwrap()
    };
    visibility().set_active(false);
    pump(100);
    assert!(white_pixels(&w) > after_ink + 500);
    visibility().set_active(true);
    pump(100);
    assert!(white_pixels(&w) < initial - 500);
    w.layer_opacity.set_value(0.5);
    pump(100);
    assert_eq!(state(&w).layers[0].opacity, 0.5);
    w.layer_opacity.set_value(1.0);
    pump(100);
    let painted_id = state(&w).layers[0].id;
    click(&command(&w, CommandId::LowerLayer));
    assert_eq!(state(&w).layers[1].id, painted_id);
    click(&command(&w, CommandId::RaiseLayer));
    assert_eq!(state(&w).layers[0].id, painted_id);

    click(&command(&w, CommandId::Settings));
    assert!(state(&w).settings_draft.is_some());
    assert!(w.preferences.dialog.root().is_some());
    let pressure: adw::SpinRow = find_named(w.preferences.dialog.upcast_ref(), "setting-pressure")
        .unwrap()
        .downcast()
        .unwrap();
    pressure.set_value(1.45);
    assert_eq!(state(&w).settings_draft.unwrap().pressure_gamma, 1.45);
    w.preferences.dialog.close();
    pump(300);
    assert!(state(&w).settings_draft.is_none());
    assert_eq!(state(&w).settings.pressure_gamma, 1.0);
    click(&command(&w, CommandId::Settings));
    pressure.set_value(1.5);
    let apply = find_button(w.preferences.dialog.upcast_ref(), "Apply").unwrap();
    click(&apply);
    assert_eq!(state(&w).settings.pressure_gamma, 1.5);
    assert!(state(&w).settings_draft.is_none());
    pump(300);

    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Sizes,
        target: DockTarget::Edge {
            edge: Edge::Right,
            outer: false,
        },
    });
    pump(100);
    assert_eq!(
        state(&w)
            .workspace
            .layout
            .bands
            .iter()
            .filter(|b| b.edge == Edge::Right)
            .count(),
        2
    );
    let layers = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.active == Panel::Layers)
        .unwrap()
        .bounds;
    let hint = w
        .drop_at(
            layers.x + layers.width * 0.5,
            layers.y + layers.height * 0.5,
            DockItem::Panel {
                panel: Panel::Brushes,
            },
        )
        .unwrap();
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Brushes,
        target: hint.target,
    });
    pump(100);
    assert!(
        state(&w)
            .workspace
            .layout
            .resolve(1200.0, 900.0)
            .groups
            .iter()
            .any(|g| g.panels == [Panel::Layers, Panel::Brushes])
    );
    let tab = w
        .groups
        .borrow()
        .iter()
        .flat_map(|g| g.tabs.iter())
        .find(|(p, _)| *p == Panel::Layers)
        .unwrap()
        .1
        .clone();
    click(&tab);
    assert!(
        state(&w)
            .workspace
            .layout
            .resolve(1200.0, 900.0)
            .groups
            .iter()
            .any(|g| g.panels.len() == 2 && g.active == Panel::Layers)
    );
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Sizes,
        target: DockTarget::Tab {
            group: 8,
            index: None,
        },
    });
    pump(100);
    click(&tab);
    let root = w
        .groups
        .borrow()
        .iter()
        .find(|g| g.id == 8)
        .unwrap()
        .stack
        .parent()
        .unwrap();
    let header = find_css(&root, "dock-tabs").unwrap();
    let grip = header.last_child().unwrap();
    let controllers = grip.observe_controllers();
    let source = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>())
        .unwrap();
    let item = source
        .content()
        .unwrap()
        .value(NativeDockItem::static_type())
        .unwrap()
        .get::<NativeDockItem>()
        .unwrap()
        .0;
    assert_eq!(item, DockItem::Group { group: 8 });
    let b = root.compute_bounds(&w.surface).unwrap();
    let hint = w
        .drop_at(
            b.x() + b.width() * 0.5,
            b.y() + b.height() * 0.5,
            DockItem::Panel {
                panel: Panel::Toolbar,
            },
        )
        .unwrap();
    assert_eq!(hint.bounds.width, 3.0);
    assert_eq!(hint.bounds.y, b.y());
    let grip_bounds = grip.compute_bounds(&w.surface).unwrap();
    assert!(hint.bounds.x + hint.bounds.width <= grip_bounds.x());
    *w.drop_hint.borrow_mut() = Some(hint);
    w.surface.queue_draw();
    pump(50);
    crate::capture(&w, "../../artifacts/ui/gtk-tab-insertion.png");
    w.clear_drop();
    let expected = w
        .groups
        .borrow()
        .iter()
        .find(|g| g.id == 8)
        .unwrap()
        .panels
        .clone();
    w.dispatch(item.move_action(
        DockTarget::Edge {
            edge: Edge::Bottom,
            outer: false,
        },
        [w.surface.width() as f32, w.surface.height() as f32],
    ));
    pump(100);
    assert_eq!(
        w.groups.borrow().iter().find(|g| g.id == 8).unwrap().panels,
        expected
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(150);
    // Dock resizing must retain its native drag handle and GPU session.
    let handle = w
        .surface
        .imp()
        .children
        .borrow()
        .iter()
        .find(|(s, _)| *s == Slot::Divider(3))
        .unwrap()
        .1
        .clone();
    w.dispatch(UiAction::ResizeDock {
        id: 3,
        position: [282.0, 0.0],
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
    });
    pump(100);
    assert!(
        w.surface
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(s, h)| *s == Slot::Divider(3) && *h == handle)
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(100);
    // Repeated allocations retire differently sized display images. The pool
    // must remain bounded, eventually present the latest size, then go idle.
    for position in [250.0, 310.0, 260.0, 320.0, 270.0, 300.0] {
        w.dispatch(UiAction::ResizeDock {
            id: 3,
            position: [position, 0.0],
            viewport: [w.surface.width() as f32, w.surface.height() as f32],
        });
        pump(25);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(200);
    let scale = w.area.scale_factor() as u32;
    assert_eq!(
        state(&w).camera.viewport,
        [
            w.area.width() as u32 * scale,
            w.area.height() as u32 * scale
        ]
    );
    assert!(!w.ticking.get(), "resize presentation must settle");
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        1
    );
    click(&command(&w, CommandId::FitCanvas));
    w.window.set_visible(false);
    pump(50);
    w.window.present();
    pump(200);
    std::fs::create_dir_all("../../artifacts/ui").unwrap();
    review(&w, "dark");
    assert_eq!(
        command(&w, CommandId::ZenMode).icon_name().as_deref(),
        Some("layer-zen-symbolic")
    );
    let toolbar_bounds = w.toolbar.compute_bounds(&w.surface).unwrap();
    for pair in w.brush_buttons.borrow().windows(2).take(2) {
        let a = pair[0].1.compute_bounds(&w.surface).unwrap();
        let b = pair[1].1.compute_bounds(&w.surface).unwrap();
        assert_eq!(b.y() - a.y() - a.height(), 2.0);
    }
    assert_eq!(toolbar_bounds.y(), HEADER_HEIGHT);
    let zen = command(&w, CommandId::ZenMode)
        .compute_bounds(&w.surface)
        .unwrap();
    let below = toolbar_bounds.y() - zen.y() - zen.height();
    assert!(
        (below - zen.y()).abs() <= 2.0,
        "header control margins: above={}, below={below}",
        zen.y()
    );
    assert_eq!(toolbar_bounds.x(), w.resolved().status.x);
    assert_eq!(toolbar_bounds.width(), w.resolved().status.width);
    let mut child = w.toolbar.first_child();
    let mut previous_right = None;
    while let Some(tile) = child {
        if tile.has_css_class("tile-button") {
            assert_eq!((tile.width(), tile.height()), (36, 36));
            let b = tile.compute_bounds(&w.toolbar).unwrap();
            assert_eq!(b.y(), 0.0);
            assert_eq!(b.x(), previous_right.map_or(0.0, |x| x + 2.0));
            previous_right = Some(b.x() + b.width());
        }
        child = tile.next_sibling();
    }
    let grip = w
        .toolbar
        .last_child()
        .unwrap()
        .compute_bounds(&w.toolbar)
        .unwrap();
    assert_eq!(grip.x() + grip.width(), w.toolbar.width() as f32);
    assert_eq!((grip.width(), grip.height()), (20.0, 24.0));
    assert_eq!(
        grip.y() + grip.height() * 0.5,
        w.toolbar.height() as f32 * 0.5
    );
    for group in w.groups.borrow().iter() {
        for (_, tab) in &group.tabs {
            assert_eq!(tab.compute_bounds(&w.surface).unwrap().height(), TILE_SIZE);
        }
    }
    // Window narrowing wraps the lone ribbon, without persisting its growth.
    w.window.set_default_size(680, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 74);
    let first = w.toolbar.first_child().unwrap();
    let fourth = first
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap();
    let a = first.compute_bounds(&w.toolbar).unwrap();
    let b = fourth.compute_bounds(&w.toolbar).unwrap();
    assert_eq!(a.x(), b.x());
    assert_eq!(b.y() - a.y(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-auto-wrap.png");
    w.window.set_default_size(1200, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 36);
    // One-column side ribbon, then two columns after cross-axis resizing.
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Toolbar,
        target: DockTarget::Edge {
            edge: Edge::Left,
            outer: false,
        },
    });
    pump(100);
    assert_eq!(w.toolbar.width(), 36);
    let grip = w
        .toolbar
        .last_child()
        .unwrap()
        .compute_bounds(&w.toolbar)
        .unwrap();
    assert_eq!((grip.width(), grip.height()), (24.0, 20.0));
    assert_eq!(grip.y() + grip.height(), w.toolbar.height() as f32);
    let layout = state(&w).workspace.layout;
    let band = layout.bands.last().unwrap();
    let d = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.id == band.id)
        .unwrap();
    w.dispatch(UiAction::ResizeDock {
        id: band.id,
        position: [d.bounds.x + d.bounds.width * 0.5 + 38.0, d.bounds.y],
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
    });
    pump(100);
    assert_eq!(w.toolbar.width(), 74);
    let first = w.toolbar.first_child().unwrap();
    let second = first
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap();
    let a = first.compute_bounds(&w.toolbar).unwrap();
    let b = second.compute_bounds(&w.toolbar).unwrap();
    assert_eq!(a.y(), b.y());
    assert_eq!(b.x() - a.x(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-wrapped.png");
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(100);
    let camera = state(&w).camera;
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::ZenMode));
    pump(250);
    // Native enter events from reparenting may arrive during the pump. This
    // assertion supplies its own hover location, independently of the desktop cursor.
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    w.update_zen();
    // Capture the settled 180ms fade, not the first frame after hiding chrome.
    pump(200);
    assert!(w.header.has_css_class("zen-hidden"));
    assert_eq!(state(&w).camera, camera);
    assert!(
        !w.ticking.get(),
        "Zen fade must not run the canvas frame loop"
    );
    for (slot, widget) in w.surface.imp().children.borrow().iter() {
        if *slot != Slot::Canvas {
            assert!(!widget.can_target());
        }
    }
    crate::capture(&w, "../../artifacts/ui/gtk-zen.png");
    assert!(w.reveal_chrome_at(24.0, 24.0));
    assert!(!w.header.has_css_class("zen-hidden"));
    assert!(command(&w, CommandId::ZenMode).has_css_class("selected-tool"));
    pump(200);
    crate::capture(&w, "../../artifacts/ui/gtk-zen-controls.png");
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::Settings));
    assert!(
        !w.header.has_css_class("zen-hidden"),
        "settings must pin chrome"
    );
    w.dispatch(UiAction::CancelSettings);
    pump(100);
    click(&command(&w, CommandId::ZenMode));
    assert!(!w.header.has_css_class("zen-hidden"));
    click(&command(&w, CommandId::TogglePanels));
    assert_eq!(state(&w).camera.translation, camera.translation);
    click(&command(&w, CommandId::TogglePanels));
    click(&command(&w, CommandId::ToggleTheme));
    pump(200);
    assert_eq!(state(&w).theme, Theme::Light);
    review(&w, "light");
    let center = state(&w).camera.viewport.map(|v| v as f32 * 0.5);
    let change = w
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .gesture(center, center, 4.0, 0.0);
    w.changed(change);
    pump(150);
    let texture = crate::snapshot(&w);
    texture
        .save_to_png("../../artifacts/ui/gtk-zoom.png")
        .unwrap();
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    let offset = (24 * texture.width() as usize + 300) * 4;
    assert!(
        bytes[offset..offset + 3].iter().all(|c| *c > 245),
        "zoomed paper must show through the title bar"
    );
    w.window.destroy();
    pump(100);
    assert!(w.gpu.borrow().is_none());
}

fn review(w: &Workspace, theme: &str) {
    // Save the exact texture that passed the ink assertion, without taking a
    // second scene snapshot between validation and artifact generation.
    let texture = crate::snapshot(w);
    assert_stroke_positions(w, &texture);
    // Sample the stroke itself: light-theme chrome is also almost white, so
    // whole-window white-pixel counts cannot compare across themes.
    let path = format!("../../artifacts/ui/gtk-{theme}.png");
    texture.save_to_png(&path).unwrap();
    assert_stroke_positions(w, &gdk::Texture::from_filename(&path).unwrap());
}

fn assert_stroke_positions(w: &Workspace, texture: &gdk::Texture) {
    let camera = state(w).camera;
    let gpu = w.gpu.borrow();
    let stroke = gpu
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .strokes()
        .next()
        .unwrap();
    let [a, b, c, d, tx, ty] = camera.document_to_surface();
    let origin = w.area.compute_bounds(&w.window).unwrap();
    let scale = w.area.scale_factor() as f32;
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    for fraction in 1..=3 {
        let point = stroke.points[stroke.points.len() * fraction / 4].position;
        let x = (origin.x() + (a * point.x + c * point.y + tx) / scale).round() as usize;
        let y = (origin.y() + (b * point.x + d * point.y + ty) / scale).round() as usize;
        let offset = (y * texture.width() as usize + x) * 4;
        assert!(
            bytes[offset] < 235 && bytes[offset + 1] < 235 && bytes[offset + 2] < 235,
            "committed stroke fraction {fraction}/4 must be visible at its input position: ({x}, {y})"
        );
    }
}

fn find_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(b) = root.downcast_ref::<gtk::Button>()
        && b.label().as_deref() == Some(label)
    {
        return Some(b.clone());
    }
    let mut child = root.first_child();
    while let Some(w) = child {
        if let Some(button) = find_button(&w, label) {
            return Some(button);
        }
        child = w.next_sibling();
    }
    None
}

fn find_named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_named(&widget, name) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn find_css(root: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    if root.has_css_class(class) {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_css(&widget, class) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}
