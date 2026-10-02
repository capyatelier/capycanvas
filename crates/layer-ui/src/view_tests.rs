fn device_pixels(s: &UiSession<Recorder>) -> bool {
    s.state.camera.translation.iter().all(|v| v.fract() == 0.0)
}

#[test]
fn actual_pixels_zooms_to_one_on_whole_device_pixels() {
    let mut s = session(Platform::Gtk);
    for turn in [None, Some(CommandId::RotateRight), Some(CommandId::RotateRight), Some(CommandId::RotateLeft)] {
        if let Some(turn) = turn {
            invoke(&mut s, turn);
        }
        s.dispatch(UiAction::SetZoom { zoom: 0.371 }).unwrap();
        s.state.camera.translation = s.state.camera.translation.map(|v| v + 0.43);
        let revision = s.state.camera.revision;
        let change = invoke(&mut s, CommandId::ActualPixels);
        assert_ne!(change.regions & regions::CAMERA, 0);
        assert_eq!(s.state.camera.zoom, 1.0);
        assert!(device_pixels(&s), "{turn:?}: {:?}", s.state.camera.translation);
        assert!(s.state.camera.revision > revision);
        assert_eq!(s.engine.view().document_to_surface, s.state.camera.document_to_surface());
    }
    assert!((s.state.camera.rotation.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    assert!(s.command(CommandId::ActualPixels).enabled);
    assert_eq!(s.command(CommandId::ActualPixels).label.as_ref(), "Actual Pixels");
    assert_ne!(CommandId::ActualPixels.label(), CommandId::PlacementOriginalSize.label());
    let view = s.application_menu(ApplicationMenu::View);
    let zoom_section = view
        .sections
        .iter()
        .find(|section| section.iter().any(|i| i.action == Some(UiAction::Invoke { command: CommandId::ZoomIn })))
        .unwrap();
    let labels: Vec<_> = zoom_section.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Zoom in", "Zoom out", "Fit canvas", "Actual Pixels"]);
    assert!(s.command_catalog().iter().any(|d| d.id == "command.actual_pixels" && d.enabled));
}

#[test]
fn set_zoom_clamps_to_the_camera_limits_and_waits_for_idle() {
    let mut s = session(Platform::Web);
    s.dispatch(UiAction::SetZoom { zoom: 100.0 }).unwrap();
    assert_eq!(s.state.camera.zoom, MAX_ZOOM);
    assert!(!s.command(CommandId::ZoomIn).enabled);
    s.dispatch(UiAction::SetZoom { zoom: 0.0001 }).unwrap();
    assert_eq!(s.state.camera.zoom, MIN_ZOOM);
    assert!(!s.command(CommandId::ZoomOut).enabled);
    s.dispatch(UiAction::SetZoom { zoom: 2.0 }).unwrap();
    assert_eq!(s.state.camera.zoom, 2.0);
    assert!(device_pixels(&s));
    let camera = s.state.camera.clone();
    assert!(s.dispatch(UiAction::SetZoom { zoom: f32::NAN }).is_err());
    assert!(s.dispatch(UiAction::SetZoom { zoom: f32::INFINITY }).is_err());
    assert_eq!(s.state.camera, camera);
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    assert!(s.dispatch(UiAction::SetZoom { zoom: 1.0 }).is_err(), "no zoom jumps under a contact");
    assert!(!s.command(CommandId::ActualPixels).enabled);
    assert_eq!(s.state.camera.zoom, 2.0);
    let action = UiAction::SetZoom { zoom: 0.5 };
    assert_eq!(serde_json::to_value(&action).unwrap(), serde_json::json!({"type": "set_zoom", "zoom": 0.5}));
}

#[test]
fn zoom_menu_offers_view_commands_then_fixed_levels() {
    let mut s = session(Platform::Gtk);
    let menu = s.zoom_menu();
    let [commands, levels] = menu.sections.as_slice() else {
        panic!("two sections: {menu:?}");
    };
    let invoked: Vec<_> = commands
        .iter()
        .map(|i| match i.action {
            Some(UiAction::Invoke { command }) => command,
            _ => panic!("{i:?}"),
        })
        .collect();
    assert_eq!(invoked, [CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas, CommandId::ActualPixels]);
    assert_eq!(commands[3].label, "Actual Pixels");
    assert_eq!(commands[3].hint, "Ctrl+1 / Ctrl+Alt+0");
    assert_eq!(levels.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["25%", "50%", "100%", "200%", "400%"]);
    for item in levels {
        assert!(item.enabled && item.selected.is_none());
        let Some(UiAction::SetZoom { zoom }) = item.action.clone() else {
            panic!("{item:?}");
        };
        s.dispatch(item.action.clone().unwrap()).unwrap();
        assert_eq!(s.state.camera.zoom, zoom);
        assert!(device_pixels(&s));
    }
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    let busy = s.zoom_menu();
    assert!(busy.sections.iter().flatten().all(|i| !i.enabled), "no zoom item runs under a contact");
    assert_eq!(serde_json::to_value(NumericControl::zoom()).unwrap()["mapping"], serde_json::json!({"type": "log"}));
}

#[test]
fn zoom_field_types_percent_on_a_logarithmic_track() {
    let zoom = NumericControl::zoom();
    let typed = |text: &str| zoom.resolve(1.0, NumericOperation::Expression { text: text.into() }).unwrap();
    assert_eq!(typed("200").value, 2.0);
    assert!((typed("33.3 %").value - 0.333).abs() < 1e-9);
    assert_eq!(typed("5000").value, f64::from(MAX_ZOOM));
    assert_eq!(typed("0").value, f64::from(MIN_ZOOM));
    assert_eq!(zoom.resolve(1.0, NumericOperation::Format).unwrap().text, "100 %");
    let halfway = zoom.resolve(1.0, NumericOperation::Position { position: 0.5 }).unwrap().value;
    assert!((halfway - (f64::from(MIN_ZOOM) * f64::from(MAX_ZOOM)).sqrt()).abs() < 1e-3, "{halfway}");
    let doubling = |v: f64| zoom.resolve(v, NumericOperation::Format).unwrap().fill;
    assert!(((doubling(2.0) - doubling(1.0)) - (doubling(1.0) - doubling(0.5))).abs() < 1e-9);
}

#[test]
fn actual_pixels_chords_follow_each_keymap_and_reach_browsers() {
    let id = CommandId::ActualPixels.shortcut_id();
    let [primary_one, primary_alt_zero, one] =
        [chord("1", true, false, false), chord("0", true, false, true), chord("1", false, false, false)];
    for key in [&primary_one, &primary_alt_zero, &one] {
        assert!(key.available(Platform::Web), "{key:?}");
    }
    let mut settings = Settings::default();
    assert_eq!(settings.keys(&id), [primary_one.clone(), primary_alt_zero.clone()]);
    assert_eq!(
        settings.shortcut_match(&primary_one, Platform::Web, Some(ToolCategory::Drawing)).map(|d| d.id),
        Some(id.clone())
    );
    for (preset, keys) in [
        ("photoshop", vec![primary_one.clone()]),
        ("affinity", vec![primary_one.clone()]),
        ("gimp", vec![one.clone()]),
    ] {
        crate::keymaps::select(&mut settings, preset).unwrap();
        assert_eq!(settings.keys(&id), keys, "{preset}");
        assert_eq!(bound(&settings, &keys[0]).as_deref(), Some(id.as_str()), "{preset}");
        let parsed = crate::keymaps::preset(preset).unwrap().preset;
        assert!(parsed.revision >= 2, "{preset} changed its rows");
        assert!(!parsed.differences.iter().any(|(_, note)| note.contains("100% zoom")), "{preset}");
    }
    assert_eq!(bound(&settings, &primary_alt_zero), None, "GIMP binds only 1");
}
