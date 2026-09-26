fn finger(s: &mut UiSession<Recorder>, id: u64, phase: ContactPhase, position: [f32; 2], ms: u64) -> InputReply {
    s.input(UiInput::Pointer {
        id,
        phase,
        kind: PointerKind::Touch,
        button: PointerButton::Primary,
        position,
        time_ns: 1_000_000_000 + ms * 1_000_000,
    })
    .unwrap()
}

fn tap(s: &mut UiSession<Recorder>, fingers: u64, start: u64, hold: u64) -> InputReply {
    for id in 0..fingers {
        finger(s, 10 + id, ContactPhase::Down, [300. + 80. * id as f32, 400.], start + id * 20);
    }
    for id in 0..fingers {
        finger(s, 10 + id, ContactPhase::Move, [302. + 80. * id as f32, 401.], start + 60);
    }
    let mut reply = InputReply::default();
    for id in 0..fingers {
        reply = finger(s, 10 + id, ContactPhase::Up, [302. + 80. * id as f32, 401.], start + hold + id * 10);
    }
    reply
}

fn painted_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Android);
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(4, 4).unwrap();
    assert!(s.command(CommandId::Undo).enabled);
    s
}

#[test]
fn two_and_three_finger_taps_undo_and_redo() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    let camera = s.state.camera.clone();
    tap(&mut s, 2, 0, 150);
    assert_ne!(s.engine.document().layers, painted.layers);
    let undone = s.engine.document().clone();
    assert_eq!(s.state.camera.view(), camera.view(), "tap jitter never moves the view");
    assert!(s.state.camera.revision > camera.revision);
    tap(&mut s, 3, 1000, 150);
    assert_eq!(s.engine.document().layers, painted.layers);
    tap(&mut s, 2, 2000, 150);
    tap(&mut s, 4, 3000, 150);
    assert_eq!(s.engine.document().layers, undone.layers, "four fingers are unbound by default");
}

#[test]
fn taps_fail_when_held_moved_staggered_or_untimed() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    tap(&mut s, 2, 0, 900);
    assert_eq!(s.engine.document(), &painted, "a long press is not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 2000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 2010);
    finger(&mut s, 11, ContactPhase::Move, [460., 400.], 2050);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 2100);
    finger(&mut s, 11, ContactPhase::Up, [460., 400.], 2110);
    assert_eq!(s.engine.document(), &painted, "a pinch is not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 3000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 3010);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 3050);
    finger(&mut s, 12, ContactPhase::Down, [350., 400.], 3060);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 3100);
    finger(&mut s, 12, ContactPhase::Up, [350., 400.], 3120);
    assert_eq!(s.engine.document(), &painted, "rolling contacts are not a tap");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 4000);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 4010);
    finger(&mut s, 11, ContactPhase::Cancel, [400., 400.], 4020);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 4050);
    assert_eq!(s.engine.document(), &painted, "cancellation is not a tap");
    for (id, phase) in [(10, ContactPhase::Down), (11, ContactPhase::Down), (10, ContactPhase::Up), (11, ContactPhase::Up)] {
        s.input(UiInput::Pointer {
            id,
            phase,
            kind: PointerKind::Touch,
            button: PointerButton::Primary,
            position: [300., 400.],
            time_ns: 0,
        })
        .unwrap();
    }
    assert_eq!(s.engine.document(), &painted);
}

#[test]
fn taps_yield_to_pens_strokes_and_the_picker() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 0);
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 10);
    s.input(UiInput::Pointer {
        id: 1,
        phase: ContactPhase::Down,
        kind: PointerKind::Pen,
        button: PointerButton::Primary,
        position: [500., 500.],
        time_ns: 1,
    })
    .unwrap();
    s.input(UiInput::Pointer {
        id: 1,
        phase: ContactPhase::Up,
        kind: PointerKind::Pen,
        button: PointerButton::Primary,
        position: [500., 500.],
        time_ns: 2,
    })
    .unwrap();
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 50);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 60);
    assert_eq!(s.engine.document().layers, painted.layers, "a pen contact ends the tap");
    s.pen(event(&s, 10, PenPhase::Down, 1.)).unwrap();
    tap(&mut s, 2, 1000, 100);
    s.pen(event(&s, 11, PenPhase::Up, 1.)).unwrap();
    s.frame(12, 12).unwrap();
    let stroked = s.engine.document().clone();
    assert_ne!(stroked.layers, painted.layers, "a stroke owns the canvas");
    finger(&mut s, 10, ContactPhase::Down, [300., 400.], 3000);
    s.input(UiInput::ColorPickerHold { id: 10, position: [300., 400.], offset: 40. }).unwrap();
    finger(&mut s, 11, ContactPhase::Down, [400., 400.], 3010);
    finger(&mut s, 11, ContactPhase::Up, [400., 400.], 3050);
    finger(&mut s, 10, ContactPhase::Up, [300., 400.], 3060);
    s.frame(13, 13).unwrap();
    assert_eq!(s.engine.document().layers, stroked.layers);
}

#[test]
fn tap_bindings_are_configurable_and_validated() {
    let mut s = painted_session();
    let painted = s.engine.document().clone();
    edit_preference(&mut s, PreferenceId::TwoFingerTap, PreferenceValue::Choice(0));
    assert_eq!(s.state.settings.gestures.get("touch.tap.2").map(String::as_str), Some(""));
    tap(&mut s, 2, 0, 100);
    assert_eq!(s.engine.document(), &painted);
    let row = s.state.settings.field(PreferenceId::ThreeFingerTap, Platform::Android).unwrap();
    let PreferenceKind::Choice { options, selected, .. } = row.kind else { panic!("choice") };
    assert_eq!(options[selected as usize], "Redo");
    assert!(!options.iter().any(|o| o.contains("held")), "taps cannot hold");
    let search = options.iter().position(|o| o.starts_with("Search Commands")).unwrap();
    edit_preference(&mut s, PreferenceId::ThreeFingerTap, PreferenceValue::Choice(search as u32));
    tap(&mut s, 3, 1000, 100);
    assert!(s.state.command_search.is_some());
    preference(&mut s, PreferenceAction::Reset { id: PreferenceId::TwoFingerTap });
    assert!(!s.state.settings.gestures.contains_key("touch.tap.2"));
    let mut settings = Settings::default();
    settings.gestures.insert("touch.tap.2".into(), "hold.eyedropper".into());
    assert!(settings.validate().is_err());
    settings.gestures.insert("touch.tap.2".into(), "command.Nope".into());
    assert!(settings.validate().is_err());
    settings.gestures = [("touch.tap.9".to_string(), String::new())].into();
    assert!(settings.validate().is_err());
    settings.gestures = [("pen.button.primary".to_string(), "hold.eyedropper".to_string())].into();
    settings.validate().unwrap();
    let saved = serde_json::to_value(&settings).unwrap();
    assert_eq!(serde_json::from_value::<Settings>(saved).unwrap(), settings);
    assert!(serde_json::to_value(Settings::default()).unwrap().get("gestures").is_none());
    assert!(Settings::default().field(PreferenceId::TwoFingerTap, Platform::Mac).is_err());
    assert!(Settings::default().field(PreferenceId::PenButton, Platform::Ios).is_err());
}

#[test]
fn pen_buttons_are_opt_in_and_use_the_hold_lifecycle() {
    let mut s = session(Platform::Gtk);
    let preset = s.state.brush.preset;
    let press = |s: &mut UiSession<Recorder>, button, pressed| {
        s.input(UiInput::PenButton { button, pressed }).unwrap()
    };
    assert!(!press(&mut s, PenButton::Primary, true).handled, "unbound buttons stay with the driver");
    assert!(!press(&mut s, PenButton::Primary, false).handled);
    assert!(painting_with(&s, preset));
    let options = |s: &UiSession<Recorder>, id| {
        let PreferenceKind::Choice { options, .. } = s.state.settings.field(id, Platform::Gtk).unwrap().kind else {
            panic!("choice")
        };
        options
    };
    let sample = options(&s, PreferenceId::PenButton).iter().position(|o| o == "Sample color while held").unwrap();
    edit_preference(&mut s, PreferenceId::PenButton, PreferenceValue::Choice(sample as u32));
    assert!(press(&mut s, PenButton::Primary, true).handled);
    assert!(sampling(&s));
    assert!(press(&mut s, PenButton::Primary, false).handled);
    assert!(painting_with(&s, preset));

    let pan = options(&s, PreferenceId::PenSecondaryButton).iter().position(|o| o == "Pan while held").unwrap();
    edit_preference(&mut s, PreferenceId::PenSecondaryButton, PreferenceValue::Choice(pan as u32));
    assert!(press(&mut s, PenButton::Secondary, true).handled);
    assert!(!s.pointer_contact_paints(PointerButton::Primary), "a held pan button turns contacts into navigation");
    press(&mut s, PenButton::Secondary, false);
    assert!(s.pointer_contact_paints(PointerButton::Primary));

    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    press(&mut s, PenButton::Primary, true);
    assert!(painting_with(&s, preset), "a button change never splits the stroke");
    s.pen(event(&s, 2, PenPhase::Move, 1.)).unwrap();
    s.pen(event(&s, 3, PenPhase::Up, 1.)).unwrap();
    s.frame(4, 4).unwrap();
    assert!(sampling(&s));
    s.input(UiInput::Blur).unwrap();
    assert!(painting_with(&s, preset));
    assert!(!press(&mut s, PenButton::Primary, false).handled);

    let undo = options(&s, PreferenceId::PenButton).iter().position(|o| o == "Undo").unwrap();
    edit_preference(&mut s, PreferenceId::PenButton, PreferenceValue::Choice(undo as u32));
    let before = s.engine.document().clone();
    assert!(press(&mut s, PenButton::Primary, true).handled);
    press(&mut s, PenButton::Primary, false);
    assert_ne!(s.engine.document(), &before);
}

#[test]
fn remote_and_gamepad_keys_share_canonical_names() {
    for (native, canonical, label) in [
        ("AudioVolumeUp", "volumeup", "Volume Up"),
        ("AudioRaiseVolume", "volumeup", "Volume Up"),
        ("XF86AudioRaiseVolume", "volumeup", "Volume Up"),
        ("XF86AudioPlay", "mediaplaypause", "Play/Pause"),
        ("AudioLowerVolume", "volumedown", "Volume Down"),
        ("MediaPlayPause", "mediaplaypause", "Play/Pause"),
        ("AudioNext", "mediatracknext", "Next Track"),
        ("gamepad_a", "gamepad_a", "Gamepad A"),
        ("gamepad_l2", "gamepad_l2", "Gamepad L2"),
        ("gamepad_up", "gamepad_up", "Gamepad ↑"),
        ("gamepad_start", "gamepad_start", "Gamepad Start"),
        ("F13", "f13", "F13"),
    ] {
        let chord = KeyChord::new(native, Modifiers::default());
        assert_eq!(chord.key, canonical);
        assert_eq!(chord.label(Platform::Android), label);
        chord.validate().unwrap();
    }
    assert!(KeyChord::new("gamepad_z", Modifiers::default()).validate().is_err());
    let mut s = session(Platform::Android);
    s.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts }).unwrap();
    preference(&mut s, PreferenceAction::BeginShortcut { id: CommandId::Undo.shortcut_id() });
    key(&mut s, "volumeup", true, false, false);
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    key(&mut s, "volumeup", false, false, false);
    preference(&mut s, PreferenceAction::BeginShortcut { id: "tool_setting.size.increase".into() });
    key(&mut s, "gamepad_r1", true, false, false);
    preference(&mut s, PreferenceAction::ConfirmShortcut { replace: false });
    key(&mut s, "gamepad_r1", false, false, false);
    s.dispatch(UiAction::CloseSettings).unwrap();
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    s.frame(3, 3).unwrap();
    let size = s.state.brush.diameter;
    let press = |s: &mut UiSession<Recorder>, name: &str, repeat: bool| {
        s.input(UiInput::Key {
            key: name.into(),
            pressed: true,
            repeat,
            modifiers: Modifiers::default(),
            editing: false,
            divider: None,
        })
        .unwrap()
    };
    assert!(press(&mut s, "gamepad_r1", false).handled);
    assert!(press(&mut s, "gamepad_r1", true).handled);
    key(&mut s, "gamepad_r1", false, false, false);
    assert!(s.state.brush.diameter > size, "repeats step while the button is held");
    assert!(press(&mut s, "AudioVolumeUp", false).handled);
    assert!(!s.command(CommandId::Undo).enabled);
    assert!(!press(&mut s, "volumedown", false).handled, "unbound device keys stay with the system");
}

#[test]
fn stick_axes_navigate_with_dead_zones_and_stop_on_blur() {
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    let axes = |s: &mut UiSession<Recorder>, pan: [f32; 2], zoom: f32| {
        s.input(UiInput::Axes { pan, zoom }).unwrap()
    };
    let camera = s.state.camera.clone();
    assert!(!axes(&mut s, [0.1, -0.1], 0.12).change.canvas_wake, "drift inside the dead zone");
    assert!(!s.wants_continuous_frames());
    assert!(axes(&mut s, [1., 0.], 0.).change.canvas_wake);
    assert!(s.wants_continuous_frames());
    s.frame(1_000_000_000, 1_000_000_000).unwrap();
    let change = s.frame(1_016_000_000, 1_016_000_000).unwrap();
    assert!(change.regions & regions::CAMERA != 0 && change.canvas_wake);
    let moved = s.state.camera.clone();
    assert!(moved.translation[0] < camera.translation[0], "right stick deflection pans toward the right");
    assert_eq!(moved.zoom, camera.zoom);
    s.frame(1_500_000_000, 1_500_000_000).unwrap();
    let step = camera.translation[0] - moved.translation[0];
    assert!(moved.translation[0] - s.state.camera.translation[0] < step * 10., "long frame gaps are clamped");
    axes(&mut s, [0., 0.], 1.);
    s.frame(2_000_000_000, 2_000_000_000).unwrap();
    s.frame(2_100_000_000, 2_100_000_000).unwrap();
    assert!(s.state.camera.zoom > moved.zoom, "positive zoom deflection zooms in");
    s.input(UiInput::Blur).unwrap();
    assert!(!s.wants_continuous_frames());
    let stopped = s.state.camera.clone();
    s.frame(2_200_000_000, 2_200_000_000).unwrap();
    assert_eq!(s.state.camera, stopped);
    assert!(s.input(UiInput::Axes { pan: [2., 0.], zoom: 0. }).is_err());
    axes(&mut s, [1., 0.], 0.);
    s.pen(event(&s, 5, PenPhase::Down, 1.)).unwrap();
    s.frame(3_000_000_000, 3_000_000_000).unwrap();
    s.frame(3_050_000_000, 3_050_000_000).unwrap();
    assert_eq!(s.state.camera.translation, stopped.translation, "a stroke keeps the view still");
}
