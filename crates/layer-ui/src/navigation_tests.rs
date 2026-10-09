mod navigation_controls {
    use super::*;

    fn key(s: &mut UiSession<Recorder>, key: &str, pressed: bool, modifiers: Modifiers) -> InputReply {
        s.input(UiInput::Key { key: key.into(), pressed, repeat: false, modifiers,
            editing: false, divider: None }).unwrap()
    }

    fn pointer(s: &mut UiSession<Recorder>, phase: ContactPhase, position: [f32; 2]) -> InputReply {
        let reply = s.input(test_support::pointer_input(71, phase, PointerKind::Mouse,
            PointerButton::Primary, position, 0)).unwrap();
        assert!(!reply.paint, "navigation must not route samples into painting");
        reply
    }

    fn modifier_keys(s: &mut UiSession<Recorder>, value: Modifiers, pressed: bool) {
        for (active, name) in [(value.command, "Control_L"), (value.alt, "Alt_L"), (value.shift, "Shift_L")] {
            if active { key(s, name, pressed, value); }
        }
    }

    fn anchor(s: &UiSession<Recorder>, position: [f32; 2]) -> layer_core::Point {
        s.state.camera.input_transform().map(layer_core::Point { x: position[0], y: position[1] })
    }

    fn same_point(a: layer_core::Point, b: layer_core::Point) {
        assert!((a.x - b.x).abs() < 0.002 && (a.y - b.y).abs() < 0.002, "{a:?} != {b:?}");
    }

    fn same_view(a: &Camera, b: &Camera) {
        assert!((a.zoom - b.zoom).abs() < 1e-5);
        assert!((a.rotation - b.rotation).abs() < 1e-5);
        assert_eq!(a.flipped, b.flipped);
        for axis in 0..2 { assert!((a.translation[axis] - b.translation[axis]).abs() < 0.002); }
    }

    fn click_vertex(s: &mut UiSession<Recorder>, p: [f32; 2]) {
        pen_at(s, 1, PenPhase::Down, p);
        pen_at(s, 2, PenPhase::Up, p);
        s.frame(2, 2).unwrap();
    }

    #[test]
    fn zoom_tool_click_steps_on_release_and_alt_reverses_it() {
        for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
            let mut s = session(platform);
            invoke(&mut s, CommandId::Zoom);
            let checkpoint = s.engine.checkpoint();
            let before = s.state.camera.clone();
            let at = [330., 410.];
            let point = anchor(&s, at);
            assert!(pointer(&mut s, ContactPhase::Down, at).handled);
            same_view(&s.state.camera, &before);
            pointer(&mut s, ContactPhase::Up, at);
            assert!(s.state.camera.zoom > before.zoom);
            same_point(anchor(&s, at), point);
            let zoom = s.state.camera.zoom;
            key(&mut s, "Alt_L", true, Modifiers::default());
            pointer(&mut s, ContactPhase::Down, at);
            pointer(&mut s, ContactPhase::Up, at);
            assert!(s.state.camera.zoom < zoom);
            same_point(anchor(&s, at), point);
            key(&mut s, "Alt_L", false, Modifiers { alt: true, ..Modifiers::default() });
            s.frame(3, 3).unwrap();
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.engine.backend().dabs, 0);
        }
    }

    #[test]
    fn zoom_drag_uses_horizontal_distance_and_retains_the_press_anchor() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Zoom);
        let at = [340., 420.];
        let point = anchor(&s, at);
        let initial = s.state.camera.clone();
        pointer(&mut s, ContactPhase::Down, at);
        pointer(&mut s, ContactPhase::Move, [at[0] + 90., at[1] + 30.]);
        assert!(s.state.camera.zoom > initial.zoom);
        same_point(anchor(&s, at), point);
        let moved = s.state.camera.clone();
        pointer(&mut s, ContactPhase::Move, [at[0] + 90., at[1] + 120.]);
        same_view(&s.state.camera, &moved);
        pointer(&mut s, ContactPhase::Up, [at[0] + 90., at[1] + 120.]);
        same_view(&s.state.camera, &moved);
        pointer(&mut s, ContactPhase::Down, at);
        pointer(&mut s, ContactPhase::Move, [at[0] - 90., at[1]]);
        assert!(s.state.camera.zoom < moved.zoom);
        pointer(&mut s, ContactPhase::Up, [at[0] - 90., at[1]]);
    }

    #[test]
    fn zoom_and_rotation_locks_stop_pointer_tools_without_painting() {
        for (tool, lock) in [
            (CommandId::Zoom, UiAction::SetZoomLocked { locked: true }),
            (CommandId::RotateView, UiAction::SetRotationLocked { locked: true }),
        ] {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, tool);
            s.dispatch(lock).unwrap();
            let before = s.state.camera.clone();
            let checkpoint = s.engine.checkpoint();
            pointer(&mut s, ContactPhase::Down, [350., 350.]);
            pointer(&mut s, ContactPhase::Move, [460., 420.]);
            pointer(&mut s, ContactPhase::Up, [460., 420.]);
            same_view(&s.state.camera, &before);
            assert_eq!(s.engine.checkpoint(), checkpoint);
        }
    }

    #[test]
    fn temporary_navigation_chords_capture_their_mode_until_pointer_release() {
        for modifiers in [
            Modifiers::default(),
            Modifiers { command: true, ..Modifiers::default() },
            Modifiers { alt: true, ..Modifiers::default() },
            Modifiers { command: true, alt: true, ..Modifiers::default() },
            Modifiers { shift: true, ..Modifiers::default() },
        ] {
            let mut s = session(Platform::Gtk);
            let tool = s.layer_interaction.tool;
            let checkpoint = s.engine.checkpoint();
            modifier_keys(&mut s, modifiers, true);
            key(&mut s, " ", true, modifiers);
            let before = s.state.camera.clone();
            let at = [370., 420.];
            let point = anchor(&s, at);
            assert!(pointer(&mut s, ContactPhase::Down, at).handled);
            key(&mut s, " ", false, Modifiers::default());
            modifier_keys(&mut s, modifiers, false);
            pointer(&mut s, ContactPhase::Move, [at[0] + 90., at[1] + 40.]);
            if modifiers.shift {
                assert!((s.state.camera.rotation - before.rotation).abs() > 0.01);
            } else if modifiers.command || modifiers.alt {
                assert!(s.state.camera.zoom > before.zoom);
                same_point(anchor(&s, at), point);
            } else {
                assert_eq!(s.state.camera.zoom, before.zoom);
                assert_ne!(s.state.camera.translation, before.translation);
            }
            pointer(&mut s, ContactPhase::Up, [at[0] + 90., at[1] + 40.]);
            assert_eq!(s.layer_interaction.tool, tool);
            s.frame(3, 3).unwrap();
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.engine.backend().dabs, 0);
        }
    }

    #[test]
    fn temporary_zoom_out_click_waits_until_up() {
        let mut s = session(Platform::Gtk);
        let alt = Modifiers { alt: true, ..Modifiers::default() };
        modifier_keys(&mut s, alt, true);
        key(&mut s, " ", true, alt);
        let before = s.state.camera.clone();
        pointer(&mut s, ContactPhase::Down, [370., 420.]);
        same_view(&s.state.camera, &before);
        pointer(&mut s, ContactPhase::Up, [370., 420.]);
        assert!(s.state.camera.zoom < before.zoom);
        key(&mut s, " ", false, Modifiers::default());
        modifier_keys(&mut s, alt, false);
    }

    #[test]
    fn navigation_tool_keys_switch_on_tap_and_spring_back_after_use() {
        for (name, command, tool) in [("z", CommandId::Zoom, LayerCanvasTool::Zoom),
            ("r", CommandId::RotateView, LayerCanvasTool::RotateView)] {
            let mut s = session(Platform::Gtk);
            let original = s.layer_interaction.tool;
            key(&mut s, name, true, Modifiers::default());
            key(&mut s, name, false, Modifiers::default());
            assert_eq!(s.layer_interaction.tool, tool);
            assert!(s.command(command).selected);
            invoke(&mut s, CommandId::Brush);
            key(&mut s, name, true, Modifiers::default());
            pointer(&mut s, ContactPhase::Down, [350., 420.]);
            pointer(&mut s, ContactPhase::Move, [450., 460.]);
            pointer(&mut s, ContactPhase::Up, [450., 460.]);
            key(&mut s, name, false, Modifiers::default());
            assert_eq!(s.layer_interaction.tool, original);
        }
    }

    #[test]
    fn cancel_and_blur_do_not_turn_a_zoom_contact_into_a_click() {
        for blur in [false, true] {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, CommandId::Zoom);
            let before = s.state.camera.clone();
            pointer(&mut s, ContactPhase::Down, [350., 420.]);
            if blur { s.input(UiInput::Blur).unwrap(); }
            else { pointer(&mut s, ContactPhase::Cancel, [350., 420.]); }
            pointer(&mut s, ContactPhase::Up, [350., 420.]);
            same_view(&s.state.camera, &before);
            pointer(&mut s, ContactPhase::Down, [350., 420.]);
            pointer(&mut s, ContactPhase::Up, [350., 420.]);
            assert!(s.state.camera.zoom > before.zoom);
            assert_eq!(s.engine.backend().dabs, 0);
        }
    }

    #[test]
    fn polygon_vertices_survive_navigation_and_finish_as_one_undoable_selection() {
        for name in [" ", "z", "r"] {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, CommandId::PolygonSelect);
            click_vertex(&mut s, [100., 100.]);
            click_vertex(&mut s, [400., 100.]);
            let vertices = s.layer_interaction.path.clone();
            let checkpoint = s.engine.checkpoint();
            let before = s.state.camera.clone();
            key(&mut s, name, true, Modifiers::default());
            pointer(&mut s, ContactPhase::Down, [350., 420.]);
            pointer(&mut s, ContactPhase::Move, [450., 460.]);
            pointer(&mut s, ContactPhase::Up, [450., 460.]);
            key(&mut s, name, false, Modifiers::default());
            assert_ne!(s.state.camera.document_to_surface(), before.document_to_surface());
            assert_eq!(s.layer_interaction.path, vertices);
            assert_eq!(s.layer_interaction.tool.selection_tool(), Some(SelectionTool::Polygon));
            assert_eq!(s.engine.checkpoint(), checkpoint);
            s.scroll([400., 350.], [0., -40.], 1., true, false).unwrap();
            invoke(&mut s, CommandId::FitCanvas);
            assert_eq!(s.layer_interaction.path, vertices);
            click_vertex(&mut s, [400., 400.]);
            key(&mut s, "enter", true, Modifiers::default());
            key(&mut s, "enter", false, Modifiers::default());
            s.frame(4, 4).unwrap();
            let selection = s.engine.document().working.selection.clone();
            assert_eq!(selection.as_ref().unwrap().contours()[0].len(), 3);
            invoke(&mut s, CommandId::Undo);
            assert!(s.engine.document().working.selection.is_none());
            invoke(&mut s, CommandId::Redo);
            assert_eq!(s.engine.document().working.selection, selection);
            assert_eq!(s.engine.backend().dabs, 0);
        }
    }

    #[test]
    fn space_arrow_keys_pan_without_switching_tools_or_making_art_history() {
        let mut s = session(Platform::Gtk);
        let tool = s.layer_interaction.tool;
        let checkpoint = s.engine.checkpoint();
        key(&mut s, " ", true, Modifiers::default());
        for (index, arrow) in ["arrowleft", "arrowright", "arrowup", "arrowdown"].into_iter().enumerate() {
            let before = s.state.camera.clone();
            assert!(key(&mut s, arrow, true, Modifiers::default()).handled);
            let now = index as u64 * 200_000_000 + 10;
            s.frame(now, now).unwrap();
            s.frame(now + 100_000_000, now + 100_000_000).unwrap();
            assert_ne!(s.state.camera.translation, before.translation, "{arrow}");
            assert_eq!(s.state.camera.zoom, before.zoom);
            assert_eq!(s.state.camera.rotation, before.rotation);
            key(&mut s, arrow, false, Modifiers::default());
        }
        key(&mut s, " ", false, Modifiers::default());
        assert_eq!(s.layer_interaction.tool, tool);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn fit_preserves_orientation_and_reset_view_clears_it() {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::SetRotation { rotation: 0.47 }).unwrap();
        invoke(&mut s, CommandId::FlipHorizontal);
        let checkpoint = s.engine.checkpoint();
        for command in [CommandId::FitCanvas, CommandId::FitWidth, CommandId::FillView] {
            s.dispatch(UiAction::SetZoom { zoom: 2.3 }).unwrap();
            invoke(&mut s, command);
            assert!((s.state.camera.rotation - 0.47).abs() < 1e-5);
            assert_eq!(s.state.camera.flipped, [true, false]);
            same_point(anchor(&s, s.state.camera.work_area_center()), layer_core::Point { x: 500., y: 500. });
        }
        invoke(&mut s, CommandId::ResetView);
        assert_eq!(s.state.camera.rotation, 0.);
        assert_eq!(s.state.camera.flipped, [false, false]);
        assert!(s.state.camera.zoom < 1.);
        same_point(anchor(&s, s.state.camera.work_area_center()), layer_core::Point { x: 500., y: 500. });
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn fit_width_and_fill_enlarge_a_portrait_canvas_beyond_fit_canvas() {
        let mut s = UiSession::new(Recorder::default(), layer_core::Document::new(
            layer_core::PortableId::random(), 400, 1600,
            layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() }),
            [1000, 1000], Platform::Gtk).unwrap();
        invoke(&mut s, CommandId::FitCanvas);
        let fit = s.state.camera.zoom;
        invoke(&mut s, CommandId::FitWidth);
        let width = s.state.camera.zoom;
        assert!(width > fit * 2.);
        invoke(&mut s, CommandId::FillView);
        assert!(s.state.camera.zoom >= width);
        same_point(anchor(&s, s.state.camera.work_area_center()), layer_core::Point { x: 200., y: 800. });
    }

    #[test]
    fn zoom_selection_centers_its_bounds_and_preserves_the_selection() {
        let mut s = session(Platform::Gtk);
        assert!(!s.command(CommandId::ZoomSelection).enabled);
        select(&mut s, rectangle([100., 200., 300., 400.]));
        let selection = s.engine.document().working.selection.clone();
        let checkpoint = s.engine.checkpoint();
        s.dispatch(UiAction::SetRotation { rotation: 0.31 }).unwrap();
        invoke(&mut s, CommandId::FlipVertical);
        let before = s.state.camera.zoom;
        invoke(&mut s, CommandId::ZoomSelection);
        assert!(s.state.camera.zoom > before);
        same_point(anchor(&s, s.state.camera.work_area_center()), layer_core::Point { x: 200., y: 300. });
        assert!((s.state.camera.rotation - 0.31).abs() < 1e-5);
        assert_eq!(s.state.camera.flipped, [false, true]);
        assert_eq!(s.engine.document().working.selection, selection);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn reset_rotation_keeps_the_center_and_mirror_and_previous_view_restores_orientation() {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::SetRotation { rotation: 0.47 }).unwrap();
        invoke(&mut s, CommandId::FlipHorizontal);
        let before = s.state.camera.clone();
        let center = anchor(&s, s.state.camera.work_area_center());
        invoke(&mut s, CommandId::ResetRotation);
        assert_eq!(s.state.camera.rotation, 0.);
        assert_eq!(s.state.camera.flipped, before.flipped);
        assert_eq!(s.state.camera.zoom, before.zoom);
        same_point(anchor(&s, s.state.camera.work_area_center()), center);
        invoke(&mut s, CommandId::PreviousView);
        same_view(&s.state.camera, &before);
    }

    #[test]
    fn rotation_commands_follow_the_selected_keymap() {
        for (preset, degrees) in [("capy", 5_f32), ("krita", 15_f32)] {
            let mut s = session(Platform::Gtk);
            let mut settings = s.state.settings.clone();
            crate::keymaps::select(&mut settings, preset).unwrap();
            s.apply_settings(settings).unwrap();
            let checkpoint = s.engine.checkpoint();
            let center = anchor(&s, s.state.camera.work_area_center());
            invoke(&mut s, CommandId::RotateRight);
            assert!((s.state.camera.rotation.to_degrees() - degrees).abs() < 0.001);
            same_point(anchor(&s, s.state.camera.work_area_center()), center);
            invoke(&mut s, CommandId::RotateLeft);
            assert!(s.state.camera.rotation.abs() < 1e-5);
            assert_eq!(s.engine.checkpoint(), checkpoint);
        }
    }

    #[test]
    fn zoom_in_aliases_zoom_without_starting_the_zoom_tool() {
        for preset in ["capy", "clip-studio"] {
            for (name, shift) in [(";", false), ("+", true), ("keypadadd", false)] {
                let mut s = session(Platform::Gtk);
                let mut settings = s.state.settings.clone();
                crate::keymaps::select(&mut settings, preset).unwrap();
                s.apply_settings(settings).unwrap();
                let before = s.state.camera.zoom;
                let tool = s.layer_interaction.tool;
                let modifiers = Modifiers { command: true, shift, alt: false };
                assert!(key(&mut s, name, true, modifiers).handled);
                assert!(s.state.camera.zoom > before, "{preset}: {name}");
                key(&mut s, name, false, modifiers);
                assert_eq!(s.layer_interaction.tool, tool);
            }
        }
    }

    #[test]
    fn zoom_and_rotation_finger_contacts_convert_to_two_finger_navigation_without_painting() {
        for command in [CommandId::Zoom, CommandId::RotateView] {
            let mut s = session(Platform::Android);
            invoke(&mut s, command);
            let checkpoint = s.engine.checkpoint();
            let touch = |s: &mut UiSession<Recorder>, id, phase, position| {
                let reply = s.input(test_support::pointer_input(id, phase, PointerKind::Touch,
                    PointerButton::Primary, position, 100_000_000)).unwrap();
                assert!(!reply.paint);
            };
            let before = s.state.camera.clone();
            touch(&mut s, 71, ContactPhase::Down, [300., 350.]);
            touch(&mut s, 71, ContactPhase::Move, [400., 390.]);
            assert_ne!(s.state.camera.document_to_surface(), before.document_to_surface());
            let before = s.state.camera.clone();
            touch(&mut s, 72, ContactPhase::Down, [650., 450.]);
            same_view(&s.state.camera, &before);
            touch(&mut s, 72, ContactPhase::Move, [750., 510.]);
            assert_ne!(s.state.camera.document_to_surface(), before.document_to_surface());
            touch(&mut s, 72, ContactPhase::Up, [750., 510.]);
            touch(&mut s, 71, ContactPhase::Up, [400., 390.]);
            s.frame(200_000_000, 200_000_000).unwrap();
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.engine.backend().dabs, 0);
            pointer(&mut s, ContactPhase::Down, [350., 420.]);
            pointer(&mut s, ContactPhase::Up, [350., 420.]);
        }
    }

    #[test]
    fn shift_zoom_rectangle_defers_zoom_until_release_and_centers_its_bounds() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Zoom);
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        modifier_keys(&mut s, shift, true);
        let before = s.state.camera.clone();
        let center = anchor(&s, [400., 400.]);
        pointer(&mut s, ContactPhase::Down, [300., 300.]);
        pointer(&mut s, ContactPhase::Move, [500., 500.]);
        same_view(&s.state.camera, &before);
        let mut overlay = Vec::new();
        s.append_layer_overlay(&mut overlay);
        assert_eq!(overlay.len(), 4);
        assert_eq!(overlay[0].from, [300., 300.]);
        assert_eq!(overlay[0].to, [500., 300.]);
        pointer(&mut s, ContactPhase::Up, [500., 500.]);
        assert!(s.state.camera.zoom > before.zoom);
        same_point(anchor(&s, s.state.camera.work_area_center()), center);
        overlay.clear();
        s.append_layer_overlay(&mut overlay);
        assert!(overlay.is_empty());
        modifier_keys(&mut s, shift, false);
    }

    #[test]
    fn rapid_canvas_zoom_clicks_each_apply_the_same_step() {
        for alt in [false, true] {
            let mut s = session(Platform::Gtk);
            s.dispatch(UiAction::SetZoom { zoom: if alt { 3.2 } else { 0.7 } }).unwrap();
            invoke(&mut s, CommandId::Zoom);
            if alt { key(&mut s, "Alt_L", true, Modifiers::default()); }
            let checkpoint = s.engine.checkpoint();
            let point = anchor(&s, [350., 420.]);
            let mut previous = s.state.camera.zoom;
            let mut step: Option<f32> = None;
            for time in [100_000_000, 200_000_000, 300_000_000] {
                for (phase, time) in [(ContactPhase::Down, time), (ContactPhase::Up, time + 10_000_000)] {
                    let reply = s.input(test_support::pointer_input(71, phase, PointerKind::Mouse,
                        PointerButton::Primary, [350., 420.], time)).unwrap();
                    assert!(!reply.paint);
                }
                let zoom = s.state.camera.zoom;
                assert!(if alt { zoom < previous } else { zoom > previous });
                if let Some(step) = step { assert!((zoom / previous - step).abs() < 1e-5); }
                else { step = Some(zoom / previous); }
                previous = zoom;
                same_point(anchor(&s, [350., 420.]), point);
            }
            assert_eq!(s.engine.checkpoint(), checkpoint);
        }
    }

    #[test]
    fn tool_button_double_click_uses_the_displayed_navigation_variant() {
        for platform in [Platform::Gtk, Platform::Android, Platform::Web] {
            for command in [CommandId::Hand, CommandId::Zoom, CommandId::RotateView] {
                let control = ToolbarControl::Command { command: CommandId::Hand };
                let (mut s, panel, ids) = group_fixture(platform, &[control]);
                let tile = ids[0];
                let anchor = DrawerAnchor::Tile { panel, tile };
                let choice = slot_choice(&s, anchor, ToolVariant::Command { command });
                s.dispatch(choice).unwrap();
                let view = s.panel_view(panel).unwrap();
                let button = view.tiles.iter().find(|item| item.id == tile).unwrap();
                assert_eq!(Some(button.choice.icon), command.icon());
                assert!(button.double_click);
                s.dispatch(UiAction::SetZoom { zoom: 3.1 }).unwrap();
                s.dispatch(UiAction::SetRotation { rotation: 0.37 }).unwrap();
                invoke(&mut s, CommandId::FlipHorizontal);
                s.scroll([450., 400.], [30., 40.], 1., false, false).unwrap();
                let center = self::anchor(&s, s.state.camera.work_area_center());
                let checkpoint = s.engine.checkpoint();
                activate_slot(&mut s, anchor);
                assert!(s.state.customization.drawer.is_some());
                s.dispatch(UiAction::DoubleClickTool { control }).unwrap();
                assert!(s.state.customization.drawer.is_none());
                assert!(s.command(command).selected);
                assert_eq!(s.state.camera.flipped, [true, false]);
                match command {
                    CommandId::Hand => {
                        assert!(s.state.camera.zoom < 1.);
                        assert!((s.state.camera.rotation - 0.37).abs() < 1e-5);
                        same_point(self::anchor(&s, s.state.camera.work_area_center()), layer_core::Point { x: 500., y: 500. });
                    }
                    CommandId::Zoom => {
                        assert_eq!(s.state.camera.zoom, 1.);
                        assert!((s.state.camera.rotation - 0.37).abs() < 1e-5);
                        same_point(self::anchor(&s, s.state.camera.work_area_center()), center);
                    }
                    CommandId::RotateView => {
                        assert_eq!(s.state.camera.rotation, 0.);
                        assert!((s.state.camera.zoom - 3.1).abs() < 1e-5);
                        same_point(self::anchor(&s, s.state.camera.work_area_center()), center);
                    }
                    _ => unreachable!(),
                }
                assert_eq!(s.engine.checkpoint(), checkpoint);
                assert_eq!(s.engine.backend().dabs, 0);
            }
        }
    }

    #[test]
    fn saved_view_retains_its_position_through_other_navigation_and_artwork_history() {
        let mut s = session(Platform::Gtk);
        assert!(!s.command(CommandId::RestoreView).enabled);
        s.dispatch(UiAction::SetZoom { zoom: 1.7 }).unwrap();
        s.dispatch(UiAction::SetRotation { rotation: 0.37 }).unwrap();
        invoke(&mut s, CommandId::FlipVertical);
        s.scroll([450., 400.], [30., 40.], 1., false, false).unwrap();
        let saved = s.state.camera.clone();
        let checkpoint = s.engine.checkpoint();
        invoke(&mut s, CommandId::SaveView);
        assert!(s.command(CommandId::RestoreView).enabled);
        invoke(&mut s, CommandId::ResetView);
        invoke(&mut s, CommandId::RestoreView);
        same_view(&s.state.camera, &saved);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        pen_at(&mut s, 1, PenPhase::Down, [100., 100.]);
        pen_at(&mut s, 2, PenPhase::Up, [130., 120.]);
        s.frame(2, 2).unwrap();
        invoke(&mut s, CommandId::Undo);
        invoke(&mut s, CommandId::ResetView);
        invoke(&mut s, CommandId::RestoreView);
        same_view(&s.state.camera, &saved);
    }

    #[test]
    fn drawing_cycle_commands_issue_host_requests_and_obey_custom_shortcuts() {
        for (command, forward) in [(CommandId::NextDrawing, true), (CommandId::PreviousDrawing, false)] {
            let mut s = session(Platform::Gtk);
            assert!(s.command_catalog().iter().any(|entry| entry.id == format!("command.{}",
                if forward { "next_drawing" } else { "previous_drawing" })));
            invoke(&mut s, command);
            let request = s.state.requests.iter().find(|r| matches!(r.kind,
                HostRequestKind::AdjacentDrawing { forward: direction } if direction == forward)).unwrap().id;
            s.dispatch(UiAction::CompleteRequest { id: request, error: None }).unwrap();
            let mut settings = s.state.settings.clone();
            settings.shortcuts.insert(command.shortcut_id(), vec![KeyChord::new("f6", Modifiers::default())]);
            s.apply_settings(settings).unwrap();
            assert!(key(&mut s, "f6", true, Modifiers::default()).handled);
            assert!(s.state.requests.iter().any(|r| matches!(r.kind,
                HostRequestKind::AdjacentDrawing { forward: direction } if direction == forward)));
            key(&mut s, "f6", false, Modifiers::default());
            let request = s.state.requests.iter().find(|r| matches!(r.kind,
                HostRequestKind::AdjacentDrawing { .. })).unwrap().id;
            s.dispatch(UiAction::CompleteRequest { id: request, error: None }).unwrap();
            let mut settings = s.state.settings.clone();
            settings.shortcuts.remove(&command.shortcut_id());
            s.apply_settings(settings).unwrap();
            invoke(&mut s, CommandId::Hand);
            for modifiers in [Modifiers { command: true, ..Modifiers::default() },
                Modifiers { alt: true, ..Modifiers::default() }] {
                modifier_keys(&mut s, modifiers, true);
                let before = s.state.camera.clone();
                let page = if forward { "pagedown" } else { "pageup" };
                assert!(key(&mut s, page, true, modifiers).handled);
                same_view(&s.state.camera, &before);
                let request = s.state.requests.iter().find(|r| matches!(r.kind,
                    HostRequestKind::AdjacentDrawing { forward: direction } if direction == forward))
                    .unwrap_or_else(|| panic!("{modifiers:?}+{page} must cycle drawings with Hand selected")).id;
                key(&mut s, page, false, modifiers);
                modifier_keys(&mut s, modifiers, false);
                s.dispatch(UiAction::CompleteRequest { id: request, error: None }).unwrap();
            }
            let before = s.state.camera.clone();
            let page = if forward { "pagedown" } else { "pageup" };
            assert!(key(&mut s, page, true, Modifiers::default()).handled);
            assert_ne!(s.state.camera.translation, before.translation);
            assert_eq!(s.state.camera.zoom, before.zoom);
            assert!(!s.state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::AdjacentDrawing { .. })));
            key(&mut s, page, false, Modifiers::default());
        }
    }

    #[test]
    fn zoom_selection_refuses_empty_coverage_and_fits_the_canvas_for_inverted_coverage() {
        let mut s = session(Platform::Gtk);
        select(&mut s, layer_core::Selection::empty());
        assert!(!s.command(CommandId::ZoomSelection).enabled);
        let before = s.state.camera.clone();
        assert!(s.dispatch(UiAction::Invoke { command: CommandId::ZoomSelection }).is_err());
        same_view(&s.state.camera, &before);
        let mut inverted = rectangle([100., 200., 300., 400.]);
        inverted.inverted = true;
        select(&mut s, inverted);
        assert!(s.command(CommandId::ZoomSelection).enabled);
        let selection = s.engine.document().working.selection.clone();
        let checkpoint = s.engine.checkpoint();
        invoke(&mut s, CommandId::FitCanvas);
        let fit = s.state.camera.clone();
        s.dispatch(UiAction::SetZoom { zoom: 3. }).unwrap();
        invoke(&mut s, CommandId::ZoomSelection);
        same_view(&s.state.camera, &fit);
        assert_eq!(s.engine.document().working.selection, selection);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }

    #[test]
    fn alt_hover_shows_zoom_out_and_pointer_capture_retains_its_original_mode() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Zoom);
        let alt = Modifiers { alt: true, ..Modifiers::default() };
        assert_eq!(key(&mut s, "Alt_L", true, Modifiers::default()).navigation_cursor, Some(NavigationMode::ZoomOut));
        let before = s.state.camera.zoom;
        pointer(&mut s, ContactPhase::Down, [350., 420.]);
        assert_eq!(key(&mut s, "Alt_L", false, alt).navigation_cursor, Some(NavigationMode::ZoomOut));
        let reply = pointer(&mut s, ContactPhase::Up, [350., 420.]);
        assert!(s.state.camera.zoom < before);
        assert_eq!(reply.navigation_cursor, Some(NavigationMode::Zoom));
        let before = s.state.camera.zoom;
        pointer(&mut s, ContactPhase::Down, [350., 420.]);
        assert_eq!(key(&mut s, "Alt_L", true, Modifiers::default()).navigation_cursor, Some(NavigationMode::Zoom));
        pointer(&mut s, ContactPhase::Up, [350., 420.]);
        assert!(s.state.camera.zoom > before);
        key(&mut s, "Alt_L", false, alt);
    }

    #[test]
    fn parked_drawing_accepts_a_fresh_cycle_key_without_the_old_keyup() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Hand);
        s.frame(0, 0).unwrap();
        let document = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        let camera = s.state.camera.clone();
        let alt = Modifiers { alt: true, ..Modifiers::default() };
        modifier_keys(&mut s, alt, true);
        assert!(key(&mut s, "pageup", true, alt).handled);
        let request = s.state.requests.iter().find(|r| matches!(r.kind,
            HostRequestKind::AdjacentDrawing { forward: false })).unwrap().id;
        s.dispatch(UiAction::CompleteRequest { id: request, error: None }).unwrap();
        assert!(s.can_park_document());
        s.park_document().unwrap();
        s.replace_renderer(Recorder::default()).unwrap();
        assert!(key(&mut s, "pageup", true, alt).handled);
        assert!(s.state.requests.iter().any(|r| matches!(r.kind,
            HostRequestKind::AdjacentDrawing { forward: false })), "a fresh PageUp must not inherit the old drawing's repeat state");
        assert_eq!(s.engine.document(), &document);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        same_view(&s.state.camera, &camera);
        assert_eq!(s.engine.backend().dabs, 0);
    }

    #[test]
    fn parked_drawing_releases_idle_temporary_navigation_without_switching_tools() {
        for (name, modifiers, mode) in [(" ", Modifiers::default(), NavigationMode::Pan),
            (" ", Modifiers { command: true, ..Modifiers::default() }, NavigationMode::Zoom),
            ("z", Modifiers::default(), NavigationMode::Zoom)] {
            let mut s = session(Platform::Gtk);
            s.frame(0, 0).unwrap();
            let tool = s.layer_interaction.tool;
            let document = s.engine.document().clone();
            let checkpoint = s.engine.checkpoint();
            let camera = s.state.camera.clone();
            modifier_keys(&mut s, modifiers, true);
            assert_eq!(key(&mut s, name, true, modifiers).navigation_cursor, Some(mode));
            assert!(s.can_park_document());
            s.park_document().unwrap();
            s.replace_renderer(Recorder::default()).unwrap();
            assert_eq!(s.input(UiInput::CursorLeave).unwrap().navigation_cursor, None);
            assert_eq!(s.layer_interaction.tool, tool);
            assert_eq!(s.engine.document(), &document);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            same_view(&s.state.camera, &camera);
        }
    }

    #[test]
    fn failed_renderer_keeps_drawing_cycle_commands_available() {
        for (command, forward) in [(CommandId::NextDrawing, true), (CommandId::PreviousDrawing, false)] {
            let mut s = session(Platform::Gtk);
            s.suspend_renderer().unwrap();
            let document = s.engine.document().clone();
            let checkpoint = s.engine.checkpoint();
            let camera = s.state.camera.clone();
            assert!(s.command(CommandId::Drawings).enabled);
            assert!(s.command(command).enabled, "{command:?} must remain available with the drawing selector");
            invoke(&mut s, command);
            assert!(s.state.requests.iter().any(|r| matches!(r.kind,
                HostRequestKind::AdjacentDrawing { forward: direction } if direction == forward)));
            assert_eq!(s.engine.document(), &document);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            same_view(&s.state.camera, &camera);
        }
    }
}
