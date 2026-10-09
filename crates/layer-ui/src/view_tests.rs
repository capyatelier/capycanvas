fn device_pixels(s: &UiSession<Recorder>) -> bool {
    s.state.camera.translation.iter().all(|v| v.fract() == 0.0)
}

#[test]
fn actual_pixels_zooms_to_one_on_whole_device_pixels() {
    let mut s = session(Platform::Gtk);
    for quarter in 0..4 {
        s.dispatch(UiAction::SetRotation { rotation: quarter as f32 * std::f32::consts::FRAC_PI_2 }).unwrap();
        s.dispatch(UiAction::SetZoom { zoom: 0.371 }).unwrap();
        s.state.camera.translation = s.state.camera.translation.map(|v| v + 0.43);
        let revision = s.state.camera.revision;
        let change = invoke(&mut s, CommandId::ActualPixels);
        assert_ne!(change.regions & regions::CAMERA, 0);
        assert_eq!(s.state.camera.zoom, 1.0);
        assert!(device_pixels(&s), "{quarter}: {:?}", s.state.camera.translation);
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
fn zoom_menu_groups_zoom_and_rotation_controls() {
    let mut s = session(Platform::Gtk);
    let menu = s.zoom_menu();
    let [commands, levels, zoom_lock, rotation] = menu.menu.sections.as_slice() else {
        panic!("four sections: {menu:?}");
    };
    let invoked: Vec<_> = commands
        .iter()
        .map(|i| match i.action {
            Some(UiAction::Invoke { command }) => command,
            _ => panic!("{i:?}"),
        })
        .collect();
    assert_eq!(menu.rotation_section, 3);
    assert_eq!(zoom_lock[0].action, Some(UiAction::SetZoomLocked { locked: true }));
    assert_eq!(zoom_lock[0].selected, Some(false));
    assert_eq!(rotation[0].action, Some(UiAction::Invoke { command: CommandId::ResetRotation }));
    assert_eq!(rotation[1].action, Some(UiAction::SetRotationLocked { locked: true }));
    assert_eq!(rotation[1].selected, Some(false));
    assert_eq!(menu.buttons.iter().map(|c| c.id).collect::<Vec<_>>(), NAVIGATOR_COMMANDS);
    assert_eq!(invoked, [CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas, CommandId::ActualPixels]);
    assert_eq!(commands[3].label, "Actual Pixels");
    assert_eq!(commands[3].hint, "Ctrl+Alt+0 / Ctrl+1");
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
    assert!(busy.menu.sections.iter().flatten().all(|i| !i.enabled), "no zoom item runs under a contact");
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
fn navigation_locks_preserve_pan_and_leave_explicit_controls_usable() {
    for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
        let mut s = session(platform);
        s.dispatch(UiAction::SetZoom { zoom: 1.0 }).unwrap();
        s.dispatch(UiAction::SetRotation { rotation: 0.4 }).unwrap();
        for (zoom, rotation) in [(true, false), (false, true), (true, true), (false, false)] {
            s.dispatch(UiAction::SetZoomLocked { locked: zoom }).unwrap();
            s.dispatch(UiAction::SetRotationLocked { locked: rotation }).unwrap();
            let before = s.state.camera.clone();
            let from = before.work_area_center();
            let point = before.input_transform().map(layer_core::Point { x: from[0], y: from[1] });
            let to = [from[0] + 30.0, from[1] - 20.0];
            s.gesture(from, to, 1.2, 0.3).unwrap();
            assert!((s.state.camera.zoom - before.zoom * if zoom { 1.0 } else { 1.2 }).abs() < 1e-5);
            assert!((s.state.camera.rotation - before.rotation - if rotation { 0.0 } else { 0.3 }).abs() < 1e-5);
            let anchored = s.state.camera.input_transform().map(layer_core::Point { x: to[0], y: to[1] });
            assert!((anchored.x - point.x).abs() < 1e-3 && (anchored.y - point.y).abs() < 1e-3);
            let menu = s.zoom_menu();
            assert_eq!(menu.menu.sections[2][0].selected, Some(zoom));
            assert_eq!(menu.menu.sections[menu.rotation_section][1].selected, Some(rotation));
            assert_eq!(s.engine.view().document_to_surface, s.state.camera.document_to_surface());
        }
        s.dispatch(UiAction::SetZoomLocked { locked: true }).unwrap();
        s.dispatch(UiAction::SetRotationLocked { locked: true }).unwrap();
        let before = s.state.camera.clone();
        for (scale, rotation) in [(f32::NAN, 0.0), (0.0, 0.0), (1.0, f32::INFINITY)] {
            assert!(s.gesture([100., 100.], [100., 100.], scale, rotation).is_err());
            assert_eq!(s.state.camera, before);
        }
        for (id, phase, point) in [(1, PenPhase::Down, [100., 100.]), (2, PenPhase::Down, [300., 100.]),
            (2, PenPhase::Move, [200., 300.]), (1, PenPhase::Up, [100., 100.]), (2, PenPhase::Up, [200., 300.])] {
            s.touch(id, phase, point);
        }
        assert_eq!(s.state.camera.zoom, before.zoom);
        assert_eq!(s.state.camera.rotation, before.rotation);
        assert_ne!(s.state.camera.translation, before.translation);
        s.scroll([100., 100.], [0., -100.], 1.0, true, false).unwrap();
        assert_eq!(s.state.camera.zoom, before.zoom);
        invoke(&mut s, CommandId::ZoomIn);
        assert!(s.state.camera.zoom > before.zoom);
        invoke(&mut s, CommandId::RotateRight);
        assert_ne!(s.state.camera.rotation, before.rotation);
        s.dispatch(UiAction::SetZoom { zoom: 2.0 }).unwrap();
        s.dispatch(UiAction::SetRotation { rotation: 0.0 }).unwrap();
        assert_eq!(s.state.camera.zoom, 2.0);
        assert!(s.state.camera.rotation.abs() < 1e-6);
    }
}

#[test]
fn rotation_field_preserves_the_center_and_refuses_invalid_or_busy_edits() {
    let mut s = session(Platform::Gtk);
    let center = s.state.camera.work_area_center();
    let point = s.state.camera.input_transform().map(layer_core::Point { x: center[0], y: center[1] });
    let control = NumericControl::rotation();
    let value = control.resolve(0.0, NumericOperation::Expression { text: "45".into() }).unwrap();
    assert!((value.value - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
    s.dispatch(UiAction::SetRotation { rotation: value.value as f32 }).unwrap();
    let anchored = s.state.camera.input_transform().map(layer_core::Point { x: center[0], y: center[1] });
    assert!((anchored.x - point.x).abs() < 1e-3 && (anchored.y - point.y).abs() < 1e-3);
    let camera = s.state.camera.clone();
    for rotation in [f32::NAN, f32::INFINITY] {
        assert!(s.dispatch(UiAction::SetRotation { rotation }).is_err());
        assert_eq!(s.state.camera, camera);
    }
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    for action in [UiAction::SetRotation { rotation: 0.0 }, UiAction::SetZoomLocked { locked: true }, UiAction::SetRotationLocked { locked: true }] {
        assert!(s.dispatch(action).is_err());
        assert_eq!(s.state.camera, camera);
    }
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
    assert_eq!(settings.keys(&id), [primary_alt_zero.clone(), primary_one.clone()]);
    assert_eq!(
        settings.shortcut_match(&primary_one, Platform::Web, Some(ToolCategory::Drawing)).map(|d| d.id),
        Some(id.clone())
    );
    for (preset, keys) in [
        ("clip-studio", vec![primary_alt_zero.clone(), primary_one.clone()]),
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
