//! Side-button transitions through Wayland, GDK and the real canvas gestures.
use super::*;

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_pen_buttons --tablet"]
fn native_canvas_pen_buttons() {
    let mut d = Driver::new("art.capycanvas.CanvasPenButtons");
    until(
        || {
            d.w.gpu.borrow().as_ref().is_some_and(|g| {
                let engine = g.session.engine();
                engine
                    .backend()
                    .paint_ready(engine.document(), engine.brush(), false)
            })
        },
        "active brush startup",
    );
    let area = d.w.resolved().work_area;
    let p = [area.x + area.width * 0.4, area.y + area.height * 0.5];
    let q = [p[0] + 50., p[1] + 20.];
    let r = [p[0] + 100., p[1]];
    let camera = state(&d.w).camera;
    let stats =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .backend()
            .stats
            .clone();
    let initial =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .metrics()
            .committed_strokes;
    let raster = |w: &Workspace| {
        let gpu = w.gpu.borrow();
        let document = gpu.as_ref().unwrap().session.engine().document();
        document
            .layers
            .iter()
            .find(|l| l.id == document.active_layer)
            .unwrap()
            .raster
            .clone()
    };
    let paper = raster(&d.w);
    let mut expected = initial;
    // BTN_STYLUS, BTN_STYLUS2 and BTN_STYLUS3 map to middle, right and back.
    for button in [331, 332, 329] {
        for held_before in [false, true] {
            stats.lock().unwrap().pen_routes.clear();
            if held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"move","point":p},
                    {"pen":"button","button":button,"down":true}
                ]));
            }
            d.input.perform(serde_json::json!([
                {"pen":"down","point":p}, {"pen":"move","point":q}
            ]));
            if !held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"button","button":button,"down":true},
                    {"pen":"move","point":r},
                    {"pen":"button","button":button,"down":false}
                ]));
            }
            assert_eq!(
                d.w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .metrics()
                    .committed_strokes,
                expected,
                "side buttons cannot finish the stroke"
            );
            d.input
                .perform(serde_json::json!([{"pen":"move","point":r}, {"pen":"up"}]));
            if held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"move","point":p},
                    {"pen":"button","button":button,"down":false}
                ]));
            }
            expected += 1;
            assert_eq!(
                d.w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .metrics()
                    .committed_strokes,
                expected
            );
            assert_eq!(state(&d.w).camera, camera, "pen buttons must not pan");
            let routes = stats.lock().unwrap().pen_routes.clone();
            let phases: Vec<_> = routes
                .iter()
                .filter(|r| r.2 == "send")
                .map(|r| r.1.as_str())
                .collect();
            assert_eq!(phases.first(), Some(&"Down"));
            assert_eq!(phases.last(), Some(&"Up"));
            assert!(
                phases[1..phases.len() - 1].iter().all(|p| *p == "Move"),
                "{phases:?}"
            );
            let painted = raster(&d.w);
            assert_ne!(painted, paper);
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            pump(180);
            assert_eq!(raster(&d.w), paper, "one Undo removes the whole stroke");
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            });
            pump(180);
            assert_eq!(raster(&d.w), painted);
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            pump(180);
        }
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts });
    for action in [
        PreferenceAction::OpenActionPicker { trigger: "pen.button.primary".into() },
        PreferenceAction::ChooseAction { id: "hold.eyedropper".into() },
    ] {
        d.w.dispatch(UiAction::Preferences { action });
    }
    d.w.dispatch(UiAction::CloseSettings);
    pump(300);
    assert_eq!(
        state(&d.w).settings.gestures.get("pen.button.primary").map(String::as_str),
        Some("hold.eyedropper")
    );
    let tool = |w: &Workspace| state(w).layer_tools.tool;
    d.input.perform(serde_json::json!([
        {"pen":"move","point":p},
        {"pen":"button","button":331,"down":true}
    ]));
    pump(120);
    assert!(tool(&d.w).picks_color(), "a bound side button samples while held");
    d.input.perform(serde_json::json!([{"pen":"button","button":331,"down":false}]));
    pump(120);
    assert_eq!(tool(&d.w), LayerCanvasTool::Paint);
    d.input.perform(serde_json::json!([
        {"pen":"down","point":p}, {"pen":"move","point":q},
        {"pen":"button","button":331,"down":true},
        {"pen":"move","point":r}
    ]));
    assert_eq!(tool(&d.w), LayerCanvasTool::Paint, "the stroke keeps its tool");
    d.input.perform(serde_json::json!([{"pen":"up"}]));
    pump(250);
    assert_eq!(
        d.w.gpu.borrow().as_ref().unwrap().session.engine().metrics().committed_strokes,
        expected + 1,
        "a bound side button cannot split the stroke"
    );
    assert!(tool(&d.w).picks_color(), "the hold applies after the stroke");
    d.input.perform(serde_json::json!([{"pen":"move","point":p}, {"pen":"button","button":331,"down":false}]));
    pump(120);
    assert_eq!(tool(&d.w), LayerCanvasTool::Paint);
    d.input.perform(serde_json::json!([{"pen":"leave"}]));
    // The actual mouse still owns middle/right navigation.
    for button in [274, 273] {
        let before = state(&d.w).camera;
        d.input.perform(serde_json::json!([
            {"point":p}, {"down":true,"button":button},
            {"point":q}, {"down":false,"button":button}
        ]));
        assert_ne!(state(&d.w).camera.translation, before.translation);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_touch_taps --tablet"]
fn native_canvas_touch_taps() {
    let mut d = Driver::new("art.capycanvas.CanvasTouchTaps");
    let area = d.w.resolved().work_area;
    let p = [area.x + area.width * 0.4, area.y + area.height * 0.5];
    let q = [p[0] + 60., p[1] + 10.];
    let history = |w: &Workspace| {
        let gpu = w.gpu.borrow();
        gpu.as_ref().unwrap().session.engine().document().revision
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while !d.w.gpu.borrow().as_ref().is_some_and(|g| {
        let engine = g.session.engine();
        engine.backend().paint_ready(engine.document(), engine.brush(), false)
    }) {
        assert!(Instant::now() < deadline, "active brush startup");
        pump(20);
    }
    d.input.perform(serde_json::json!([{"pen":"down","point":p}, {"pen":"move","point":q}, {"pen":"up"}, {"pen":"leave"}]));
    pump(300);
    assert!(d.w.gpu.borrow().as_ref().unwrap().session.command(CommandId::Undo).enabled);
    let painted = history(&d.w);
    let camera = state(&d.w).camera;
    let tap = |d: &mut Driver, fingers: usize| {
        let points: Vec<_> = (0..fingers).map(|i| [p[0] + 90. * i as f32, p[1] + 40.]).collect();
        let mut events: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(slot, point)| serde_json::json!({"touch":"down","slot":slot,"point":point}))
            .collect();
        events.extend((0..fingers).map(|slot| serde_json::json!({"touch":"up","slot":slot})));
        d.input.perform(serde_json::Value::Array(events));
        pump(250);
    };
    tap(&mut d, 2);
    assert!(!d.w.gpu.borrow().as_ref().unwrap().session.command(CommandId::Undo).enabled, "two-finger tap undoes");
    assert_ne!(history(&d.w), painted);
    tap(&mut d, 3);
    assert!(d.w.gpu.borrow().as_ref().unwrap().session.command(CommandId::Undo).enabled, "three-finger tap redoes");
    assert_eq!(state(&d.w).camera.translation, camera.translation);
    assert_eq!(state(&d.w).camera.zoom, camera.zoom);
    d.input.perform(serde_json::json!([
        {"touch":"down","slot":0,"point":p},
        {"touch":"down","slot":1,"point":[p[0] + 120., p[1]]},
        {"touch":"move","slot":1,"point":[p[0] + 220., p[1] + 40.]},
        {"touch":"up","slot":1},
        {"touch":"up","slot":0}
    ]));
    pump(250);
    assert!(d.w.gpu.borrow().as_ref().unwrap().session.command(CommandId::Undo).enabled, "a pinch is not a tap");
    assert_ne!(state(&d.w).camera.zoom, camera.zoom);
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_remote_keys"]
fn native_remote_keys() {
    let mut d = Driver::new("art.capycanvas.RemoteKeys");
    let enabled = |w: &Workspace, command| w.gpu.borrow().as_ref().unwrap().session.command(command).enabled;
    d.w.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts });
    d.w.dispatch(UiAction::Preferences {
        action: PreferenceAction::BeginShortcut { id: CommandId::Undo.shortcut_id() },
    });
    for pressed in [true, false] {
        d.w.interact(crate::input::key_input(gdk::Key::AudioPlay, pressed, gdk::ModifierType::empty(), false, None));
    }
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ConfirmShortcut { replace: true } });
    d.w.dispatch(UiAction::CloseSettings);
    pump(120);
    assert!(state(&d.w).settings.shortcuts[&CommandId::Undo.shortcut_id()].iter().any(|c| c.key == "mediaplaypause"));
    d.w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
    pump(120);
    assert!(enabled(&d.w, CommandId::Undo) && !enabled(&d.w, CommandId::Redo));
    d.input.perform(serde_json::json!([{"key":0x1008ff14,"down":true},{"key":0x1008ff14,"down":false}]));
    pump(250);
    assert!(enabled(&d.w, CommandId::Redo), "a remote's media key undoes");
    d.finish();
}

fn descendants<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<T> {
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(widget) = stack.pop() {
        if let Ok(typed) = widget.clone().downcast::<T>() {
            found.push(typed);
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            stack.push(current);
        }
    }
    found
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_shortcut_page"]
fn native_shortcut_page() {
    let mut d = Driver::new("art.capycanvas.ShortcutPage");
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| "artifacts/ui/shortcut-page".into()));
    std::fs::create_dir_all(&output).unwrap();
    let shot = |d: &Driver, name: &str| {
        pump(400);
        crate::capture(&d.w, output.join(format!("{name}.png")).to_str().unwrap());
    };
    let root = |d: &Driver| d.w.window.clone().upcast::<gtk::Widget>();
    let named = |d: &Driver, name: &str| find_named(&root(d), name).unwrap_or_else(|| panic!("{name}"));
    let page = |d: &Driver| d.w.gpu.borrow().as_ref().unwrap().session.preferences().unwrap().shortcut_page;
    for _ in 0..4 {
        d.w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
        pump(100);
        d.w.dispatch(UiAction::CloseSettings);
        pump(100);
    }
    d.w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
    pump(500);
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        shot(&d, &format!("categories-{theme:?}").to_lowercase());
    }
    d.w.dispatch(UiAction::SetTheme { theme: Some(Theme::Light) });
    let filters = named(&d, "shortcut-filters");
    let search = named(&d, "shortcuts-search").downcast::<gtk::SearchEntry>().unwrap();
    let line = filters.compute_bounds(&root(&d)).unwrap();
    let mut left = f32::MIN;
    for name in ["shortcuts-search", "shortcut-context", "shortcut-show"] {
        let b = named(&d, name).compute_bounds(&root(&d)).unwrap();
        assert!((b.y() + b.height() / 2. - (line.y() + line.height() / 2.)).abs() < 2., "{name} is on the filter line");
        assert!(b.x() > left, "{name} follows the previous control");
        left = b.x();
    }
    let keymap = named(&d, "keymap-preset").compute_bounds(&root(&d)).unwrap();
    assert!(keymap.y() < line.y(), "the keymap preset sits above the shortcut search");
    assert!(find_named(&root(&d), "shortcut-command.Undo").is_none_or(|w| !w.is_mapped()), "rows are built on demand");
    named(&d, "shortcut-category-Edit").emit_by_name::<()>("activated", &[]);
    pump(60);
    let navigation = named(&d, "shortcut-navigation").downcast::<adw::NavigationView>().unwrap();
    assert!(named(&d, "shortcut-categories").is_mapped(), "the category list slides away instead of vanishing");
    crate::capture(&d.w, output.join("category-transition.png").to_str().unwrap());
    pump(500);
    assert_eq!(navigation.visible_page_tag().as_deref(), Some("category"));
    assert!(named(&d, "shortcut-category-back").is_visible());
    assert!(named(&d, "shortcut-command.Undo").is_mapped() && !named(&d, "shortcut-categories").is_mapped());
    shot(&d, "category-edit");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    pump(500);
    assert!(named(&d, "shortcut-categories").is_mapped());
    assert_eq!(navigation.visible_page_tag().as_deref(), Some("shortcuts"));
    named(&d, "shortcut-category-Edit").emit_by_name::<()>("activated", &[]);
    pump(500);
    navigation.pop();
    pump(500);
    assert!(page(&d).category.is_none(), "native back navigation returns to the categories");
    named(&d, "shortcut-category-View").grab_focus();
    d.input.perform(serde_json::json!([{"key":0xff0d,"down":true},{"key":0xff0d,"down":false}]));
    pump(500);
    assert_eq!(page(&d).category.as_deref(), Some("View"));
    d.input.perform(serde_json::json!([{"key":0xff1b,"down":true},{"key":0xff1b,"down":false}]));
    pump(500);
    assert!(page(&d).category.is_none() && state(&d.w).settings_open, "Escape leaves the category, not Preferences");
    named(&d, "shortcut-category-Brush presets").emit_by_name::<()>("activated", &[]);
    shot(&d, "category-brush-presets");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    search.set_text("pencil");
    pump(400);
    assert!(named(&d, "shortcut-brush.2").is_mapped() && named(&d, "shortcut-command.Pencil").is_mapped());
    shot(&d, "search-pencil");
    search.set_text("z");
    pump(400);
    assert!(named(&d, "shortcut-command.Undo").is_mapped());
    assert!(find_named(&root(&d), "shortcut-command.ZoomIn").is_none_or(|w| !w.is_mapped()), "one letter finds keys, not names");
    shot(&d, "search-letter");
    search.set_text("");
    pump(300);
    search.grab_focus();
    pump(100);
    d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
    pump(400);
    assert_eq!(page(&d).key.as_deref(), Some("Ctrl+Z"), "pressing a shortcut in search looks it up");
    assert!(named(&d, "shortcut-command.Undo").is_mapped());
    assert!(find_named(&root(&d), "shortcut-command.Redo").is_none_or(|w| !w.is_mapped()));
    shot(&d, "key-search");
    let editor = |d: &Driver| d.w.gpu.borrow().as_ref().unwrap().session.preferences().unwrap().shortcut_editor;
    let recording = |d: &Driver| find_named(&root(d), "shortcut-recording").is_some_and(|w| w.is_mapped());
    let key = |d: &mut Driver, code: u32| {
        d.input.perform(serde_json::json!([{"key":code,"down":true},{"key":code,"down":false}]));
        pump(300);
    };
    named(&d, "shortcut-command.Undo").emit_by_name::<()>("activated", &[]);
    pump(400);
    assert!(named(&d, "shortcut-editor").is_mapped());
    assert!(find_named(&root(&d), "shortcut-capture").is_none(), "recording happens inside the editor");
    let before = editor(&d).unwrap().bindings;
    shot(&d, "editor");
    named(&d, "add-shortcut").emit_by_name::<()>("activated", &[]);
    pump(300);
    assert!(recording(&d));
    shot(&d, "editor-recording");
    named(&d, "cancel-shortcut").downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(300);
    assert!(!recording(&d) && named(&d, "add-shortcut").is_mapped());
    named(&d, "add-shortcut").emit_by_name::<()>("activated", &[]);
    pump(300);
    named(&d, "remove-shortcut-0").grab_focus();
    key(&mut d, 0xffc2);
    assert_eq!(state(&d.w).preferences.capture.as_ref().map(|c| c.shortcut.as_str()), Some("F5"), "recording after a cancel still hears keys, whatever has focus");
    shot(&d, "editor-captured");
    named(&d, "confirm-shortcut").downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(300);
    let mut expected = before.clone();
    expected.push("F5".into());
    assert_eq!(editor(&d).unwrap().bindings, expected, "adding keeps the existing shortcuts");
    named(&d, "add-shortcut").emit_by_name::<()>("activated", &[]);
    pump(300);
    key(&mut d, 0x65);
    let conflict = state(&d.w).preferences.capture.as_ref().and_then(|c| c.conflict.clone());
    assert_eq!(conflict.as_deref(), Some("Eraser"), "keys reach the sheet after it rebuilds");
    let confirm = named(&d, "confirm-shortcut").downcast::<gtk::Button>().unwrap();
    assert_eq!(confirm.label().as_deref(), Some(if conflict.is_some() { "Reassign" } else { "Add" }));
    shot(&d, "editor-conflict");
    key(&mut d, 0xff1b);
    assert!(!recording(&d) && named(&d, "shortcut-editor").is_mapped(), "Escape cancels recording, not the editor: capture={:?} editor={:?} error={:?}", state(&d.w).preferences.capture, editor(&d).map(|e| e.id), state(&d.w).preferences.error);
    named(&d, "remove-shortcut-1").downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(300);
    assert_eq!(editor(&d).unwrap().bindings, before);
    key(&mut d, 0xff1b);
    pump(300);
    assert!(editor(&d).is_none() && find_named(&root(&d), "shortcut-editor").is_none_or(|w| !w.is_mapped()));
    let modifier = |d: &Driver| d.w.gpu.borrow().as_ref().unwrap().session.preferences().unwrap().modifier_editor;
    search.set_text("");
    pump(300);
    named(&d, "shortcut-category-Modifier keys").emit_by_name::<()>("activated", &[]);
    pump(500);
    assert!(named(&d, "modifier-Space").is_mapped() && named(&d, "modifier-Alt").is_mapped());
    shot(&d, "modifier-keys");
    named(&d, "modifier-Alt").emit_by_name::<()>("activated", &[]);
    pump(60);
    crate::capture(&d.w, output.join("modifier-transition.png").to_str().unwrap());
    pump(500);
    let navigation = named(&d, "shortcut-navigation").downcast::<adw::NavigationView>().unwrap();
    assert_eq!(navigation.visible_page_tag().as_deref(), Some("modifier"), "a modifier key slides in instead of a popup");
    assert!(find_named(&root(&d), "modifier-key").is_none_or(|w| !w.is_mapped()));
    assert_eq!(modifier(&d).unwrap().label, "Alt");
    shot(&d, "modifier-alt");
    named(&d, "modifier-same").downcast::<adw::SwitchRow>().unwrap().set_active(false);
    pump(400);
    assert_eq!(modifier(&d).unwrap().actions.len(), 10, "one row per kind of tool");
    shot(&d, "modifier-alt-per-tool");
    named(&d, "modifier-action-selection").emit_by_name::<()>("activated", &[]);
    pump(400);
    assert!(named(&d, "action-picker").is_mapped());
    let description = named(&d, "action-picker-description").downcast::<gtk::Label>().unwrap();
    assert!(description.is_mapped() && description.text().starts_with("Holding Alt"), "the description sits below the title bar");
    shot(&d, "modifier-picker");
    key(&mut d, 0xff1b);
    pump(300);
    assert!(d.w.gpu.borrow().as_ref().unwrap().session.preferences().unwrap().shortcut_page.picker.is_none(), "Escape closes the picker");
    assert!(named(&d, "modifier-page").is_mapped(), "and stays on the key's page");
    named(&d, "modifier-action-selection").emit_by_name::<()>("activated", &[]);
    pump(400);
    named(&d, "action-command.Move").emit_by_name::<()>("activated", &[]);
    pump(400);
    let alt = state(&d.w).settings.hold_keys.clone().unwrap().into_iter().find(|h| h.key.key == "alt").unwrap();
    assert_eq!(alt.actions.get(&ToolCategory::Selection).map(String::as_str), Some("command.Move"));
    shot(&d, "modifier-alt-selection");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    pump(500);
    assert!(modifier(&d).is_none() && navigation.visible_page_tag().as_deref() == Some("category"));
    named(&d, "add-modifier-key").emit_by_name::<()>("activated", &[]);
    pump(400);
    key(&mut d, 0xffe9);
    assert!(state(&d.w).preferences.capture.as_ref().is_some_and(|c| c.existing));
    assert_eq!(named(&d, "confirm-shortcut").downcast::<gtk::Button>().unwrap().label().as_deref(), Some("Open"));
    shot(&d, "modifier-existing");
    named(&d, "confirm-shortcut").downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(500);
    assert_eq!(modifier(&d).unwrap().label, "Alt", "an existing key opens instead");
    navigation.pop();
    pump(500);
    assert!(modifier(&d).is_none(), "native back navigation closes the key");
    named(&d, "add-modifier-key").emit_by_name::<()>("activated", &[]);
    pump(400);
    assert!(recording(&d));
    d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x20,"down":true}]));
    pump(300);
    assert_eq!(state(&d.w).preferences.capture.as_ref().map(|c| c.shortcut.as_str()), Some("Ctrl+Space"), "combinations record while held");
    shot(&d, "modifier-new");
    d.input.perform(serde_json::json!([{"key":0x20,"down":false},{"key":0xffe3,"down":false}]));
    pump(300);
    named(&d, "confirm-shortcut").downcast::<gtk::Button>().unwrap().emit_clicked();
    pump(500);
    assert_eq!(modifier(&d).unwrap().label, "Ctrl+Space");
    assert_eq!(navigation.visible_page_tag().as_deref(), Some("modifier"));
    named(&d, "modifier-action-all").emit_by_name::<()>("activated", &[]);
    pump(400);
    named(&d, "action-command.Pencil").emit_by_name::<()>("activated", &[]);
    pump(400);
    shot(&d, "modifier-ctrl-space");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    pump(500);
    shot(&d, "modifier-keys-added");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    pump(500);
    search.set_text("space");
    pump(400);
    assert!(named(&d, "modifier-results").is_mapped(), "search finds modifier keys");
    shot(&d, "search-modifier");
    search.set_text("");
    pump(300);
    let show = named(&d, "shortcut-show").downcast::<gtk::DropDown>().unwrap();
    let shows: Vec<_> = (0..show.model().unwrap().n_items())
        .map(|i| show.model().unwrap().item(i).and_downcast::<gtk::StringObject>().unwrap().string().to_string())
        .collect();
    assert_eq!(shows, ["All actions", "With shortcuts", "Customized"]);
    show.set_selected(2);
    pump(300);
    assert!(named(&d, "modifier-results").is_mapped(), "customized modifier keys are listed");
    shot(&d, "customized-modifiers");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ResetAllShortcuts });
    pump(300);
    assert!(named(&d, "shortcut-empty").is_mapped());
    shot(&d, "empty-customized");
    show.set_selected(1);
    pump(300);
    assert!(named(&d, "shortcut-command.Undo").is_mapped());
    show.set_selected(0);
    let context = named(&d, "shortcut-context").downcast::<gtk::DropDown>().unwrap();
    let selection = (0..context.model().unwrap().n_items())
        .position(|i| context.model().unwrap().item(i).and_downcast::<gtk::StringObject>().unwrap().string() == "Selection tools")
        .unwrap();
    context.set_selected(selection as u32);
    pump(400);
    assert!(page(&d).context.is_some());
    shot(&d, "context-selection");
    context.set_selected(0);
    pump(200);
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Page { page: SettingsPage::Input } });
    pump(400);
    let pen = named(&d, "trigger-pen.button.primary");
    assert!(pen.is_mapped(), "touch and pen buttons live on Pen & Input");
    assert!(named(&d, "trigger-pen.button.tertiary").is_mapped() && named(&d, "setting-eraser-tool").is_mapped());
    let pen_group = named(&d, "triggers-pen-buttons").compute_bounds(&root(&d)).unwrap();
    let touch_group = named(&d, "triggers-touch-gestures").compute_bounds(&root(&d)).unwrap();
    assert!(pen_group.y() != touch_group.y(), "pen buttons and touch gestures are separate groups");
    shot(&d, "pen-and-input");
    named(&d, "trigger-touch.tap.4").grab_focus();
    shot(&d, "pen-and-input-buttons");
    let eraser = named(&d, "setting-eraser-tool").downcast::<adw::ComboRow>().unwrap();
    eraser.set_selected(1);
    pump(300);
    assert_eq!(state(&d.w).settings.eraser_end.tool, Some(CommandId::Eraser));
    assert!(!named(&d, "setting-eraser-erase").is_visible(), "the Eraser always erases");
    eraser.set_selected(0);
    pump(300);
    named(&d, "setting-eraser-tool").grab_focus();
    shot(&d, "pen-and-input-eraser");
    pen.emit_by_name::<()>("activated", &[]);
    pump(500);
    let input = named(&d, "input-navigation").downcast::<adw::NavigationView>().unwrap();
    assert_eq!(input.visible_page_tag().as_deref(), Some("pen-button"), "a pen button slides in");
    shot(&d, "pen-button");
    named(&d, "pen-button-same").downcast::<adw::SwitchRow>().unwrap().set_active(false);
    pump(400);
    named(&d, "pen-button-action-drawing").emit_by_name::<()>("activated", &[]);
    pump(400);
    assert!(named(&d, "action-picker").is_mapped());
    shot(&d, "action-picker");
    named(&d, "action-picker-search").downcast::<gtk::SearchEntry>().unwrap().set_text("pencil");
    pump(400);
    shot(&d, "action-picker-search");
    named(&d, "action-command.Pencil").emit_by_name::<()>("activated", &[]);
    pump(300);
    named(&d, "pen-button-action-selection").emit_by_name::<()>("activated", &[]);
    pump(400);
    named(&d, "action-picker-search").downcast::<gtk::SearchEntry>().unwrap().set_text("undo");
    pump(300);
    named(&d, "action-command.Undo").emit_by_name::<()>("activated", &[]);
    pump(300);
    let actions = state(&d.w).settings.pen_buttons.get("pen.button.primary").cloned().unwrap();
    assert_eq!(actions.get(&ToolCategory::Drawing).map(String::as_str), Some("command.Pencil"));
    assert_eq!(actions.get(&ToolCategory::Selection).map(String::as_str), Some("command.Undo"));
    shot(&d, "pen-button-per-tool");
    named(&d, "shortcut-category-back").emit_by_name::<()>("clicked", &[]);
    pump(500);
    assert_eq!(input.visible_page_tag().as_deref(), Some("input"));
    let action = named(&d, "trigger-pen.button.primary").downcast::<adw::ActionRow>().unwrap();
    assert!(descendants::<gtk::Label>(action.upcast_ref()).iter().any(|l| l.text() == "Depends on the tool"));
    named(&d, "trigger-pen.button.primary").grab_focus();
    shot(&d, "pen-and-input-pencil");
    assert!(find_named(&root(&d), "action-picker").is_none_or(|w| !w.is_mapped()));
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Page { page: SettingsPage::Shortcuts } });
    let combo = named(&d, "keymap-preset").downcast::<adw::ComboRow>().unwrap();
    let titles: Vec<_> = (0..combo.model().unwrap().n_items())
        .map(|i| combo.model().unwrap().item(i).and_downcast::<gtk::StringObject>().unwrap().string().to_string())
        .collect();
    combo.set_selected(titles.iter().position(|t| t == "Krita Style").unwrap() as u32);
    pump(200);
    assert_eq!(state(&d.w).settings.keymap.as_ref().map(|k| k.id.as_str()), Some("krita"));
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::KeymapDetails { open: true } });
    pump(400);
    assert!(named(&d, "keymap-details").is_mapped());
    shot(&d, "keymap-differences");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::KeymapDetails { open: false } });
    pump(300);
    let text = serde_json::json!({"format": "capycanvas-keymap", "version": 1, "keymap": {"id": "photoshop", "revision": 1}}).to_string();
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ImportKeymap { text } });
    pump(400);
    let alert = descendants::<adw::AlertDialog>(&root(&d)).into_iter().next().expect("import preview");
    alert.emit_by_name::<()>("response", &[&"import"]);
    pump(400);
    assert_eq!(state(&d.w).settings.keymap.as_ref().map(|k| k.id.as_str()), Some("photoshop"));
    combo.set_selected(0);
    pump(200);
    d.finish();
}
