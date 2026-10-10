fn picker_pointer(
    s: &mut UiSession<Recorder>,
    id: u64,
    phase: ContactPhase,
    kind: PointerKind,
    p: [f32; 2],
) -> InputReply {
    s.input(pointer_input(id, phase, kind, PointerButton::Primary, p, 0))
    .unwrap()
}
fn picker_reply(s: &mut UiSession<Recorder>, rgba: [f32; 4]) {
    let request = *s.renderer_mut().sample_requests.last().unwrap();
    s.renderer_mut().sample_reply = Some(layer_render::ColorSample {
        request_id: request.request_id,
        rgba,
    });
    s.frame(10, 10).unwrap();
}

#[test]
fn color_picker_mouse_press_and_pen_release_commit_without_a_stroke() {
    for kind in [PointerKind::Mouse, PointerKind::Pen] {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Eraser);
        let previous = s.state.layer_tools.tool;
        let original = s.state.colors.clone();
        let brush = s.engine.brush().clone();
        let revision = s.engine.document().revision;
        invoke(&mut s, CommandId::Eyedropper);
        assert_eq!(
            s.editing_state().canvas_tool,
            LayerCanvasTool::Paint
        );
        let hover = event(&s, 1, PenPhase::Hover, 0.);
        s.cursor_input(Some(hover));
        s.frame(1, 1).unwrap();
        picker_reply(&mut s, [0.2, 0.4, 0.8, 1.]);
        assert_eq!(s.state.colors, original);
        assert_eq!(s.engine.brush(), &brush);
        assert_ne!(*s.state.preview_colors(), original);
        let ring = s.color_picker_overlay().unwrap();
        assert_eq!(
            ring.original,
            original
                .definition()
                .linear_in(s.engine.document().composition().color.space)
                .unwrap()
        );
        assert!(!ring.classic);
        assert!(
            !picker_pointer(
                &mut s,
                1,
                ContactPhase::Down,
                kind,
                [hover.surface_position.x, hover.surface_position.y]
            )
            .paint
        );
        if kind == PointerKind::Pen {
            s.frame(11, 11).unwrap();
            assert_eq!(s.state.colors, original);
            assert!(s.color_picker_overlay().is_some());
            picker_pointer(&mut s, 1, ContactPhase::Move, kind, [400., 400.]);
            s.frame(12, 12).unwrap();
            picker_reply(&mut s, [0.8, 0.2, 0.1, 1.]);
            assert_eq!(s.state.colors, original);
            picker_pointer(&mut s, 1, ContactPhase::Up, kind, [400., 400.]);
        }
        assert!(s.color_picker_overlay().is_none());
        s.frame(13, 13).unwrap();
        assert_eq!(s.state.layer_tools.tool, previous);
        assert_eq!(s.state.brush.tool, Tool::Eraser);
        assert_ne!(s.state.colors, original);
        assert!(!picker_pointer(&mut s, 1, ContactPhase::Move, kind, [400., 400.]).paint);
        assert!(!picker_pointer(&mut s, 1, ContactPhase::Up, kind, [400., 400.]).paint);
        assert_eq!(s.renderer_mut().dabs, 0);
        assert_eq!(s.engine.document().revision, revision);
        assert!(s.state.color_picker.preview.is_none());
    }
}

#[test]
fn color_picker_editor_picks_publish_the_sample_without_changing_paint_or_history() {
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        let mut s = session(Platform::Gtk);
        let original = s.state.colors.clone();
        let history = s.state.color_library.history.clone();
        let space = s.engine.document().composition().color.space;
        let draft = layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.1, 0.9, 0.3, 1.]).unwrap();
        s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Editor { original: draft, touch_offset: 0. } }).unwrap();
        assert!(s.state.color_picker.editor);
        let hover = event(&s, 1, PenPhase::Hover, 0.);
        let point = [hover.surface_position.x, hover.surface_position.y];
        if kind == PointerKind::Mouse {
            s.cursor_input(Some(hover));
        } else {
            picker_pointer(&mut s, 7, ContactPhase::Down, kind, point);
        }
        s.frame(1, 1).unwrap();
        picker_reply(&mut s, [0.2, 0.4, 0.8, 1.]);
        assert!(s.state.color_picker.preview.is_some());
        assert_eq!(s.state.color_picker.sample_point, Some(s.color_picker_overlay().unwrap().sample));
        assert_eq!(*s.state.preview_colors(), original);
        assert_eq!(s.color_picker_overlay().unwrap().original, draft.linear_in(space).unwrap());
        let phase = if kind == PointerKind::Mouse { ContactPhase::Down } else { ContactPhase::Up };
        picker_pointer(&mut s, 7, phase, kind, point);
        s.frame(2, 2).unwrap();
        assert!(!s.state.color_picker.editor);
        assert_eq!(s.state.color_picker.sample_point, None);
        let picked = s.state.color_picker.picked.unwrap().linear_in(space).unwrap();
        assert!(picked.iter().zip([0.2, 0.4, 0.8, 1.]).all(|(a, b)| (a - b).abs() < 1e-6));
        assert_eq!(s.state.colors, original);
        assert_eq!(s.state.color_library.history, history);
        assert_eq!(s.renderer_mut().dabs, 0);
    }
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Editor { original: layer_core::color::RgbColor::WHITE, touch_offset: 0. } }).unwrap();
    s.cursor_input(Some(event(&s, 1, PenPhase::Hover, 0.)));
    assert_ne!(s.frame(1, 1).unwrap().regions & crate::regions::COLOR_PREVIEW, 0, "a moved sample republishes the picker");
    assert_eq!(s.frame(2, 2).unwrap().regions & crate::regions::COLOR_PREVIEW, 0);
    key(&mut s, "Escape", true, false, false);
    assert!(!s.state.color_picker.editor);
    assert!(s.state.color_picker.picked.is_none());
    assert!(s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Editor { original: layer_core::color::RgbColor::WHITE, touch_offset: f32::NAN } }).is_err());
}

#[test]
fn color_editor_memory_reaches_paint_and_mask_colors_without_editing_the_drawing() {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::SelectAll);
    invoke(&mut s, CommandId::QuickMask);
    let revision = s.engine.document().revision;
    let memory = crate::ColorEditorMemory { forms: [crate::ColorForm::LinearRgb, crate::ColorForm::Hsl, crate::ColorForm::Oklab], search: "sand".into() };
    s.dispatch(UiAction::Color { action: ColorAction::EditorMemory { memory: memory.clone() } }).unwrap();
    for colors in [s.state.display_colors(), &s.state.colors] {
        assert_eq!(crate::ColorEditor::for_slot(colors, crate::ColorSlot::Foreground).unwrap().memory(), &memory);
    }
    assert_eq!(s.engine.document().revision, revision);
    let mixed = crate::ColorEditorMemory { forms: [crate::ColorForm::Hsb; 3], search: String::new() };
    assert!(s.dispatch(UiAction::Color { action: ColorAction::EditorMemory { memory: mixed } }).is_err());
    assert_eq!(crate::ColorEditor::for_slot(&s.state.colors, crate::ColorSlot::Foreground).unwrap().memory(), &memory);
}

#[test]
fn color_picker_keeps_selection_mask_colors_separate_from_artwork() {
    for saved in [false, true] {
        let mut s = session(Platform::Gtk);
        let artwork = s.state.colors.clone();
        invoke(&mut s, CommandId::SelectAll);
        invoke(&mut s, CommandId::QuickMask);
        if saved {
            invoke(&mut s, CommandId::SaveSelectionLayer);
        }
        s.dispatch(UiAction::SetColor {
            rgba: [0., 1., 0., 1.],
        })
        .unwrap();
        let original = s.state.display_colors().clone();
        assert_ne!(original, artwork);
        invoke(&mut s, CommandId::Eyedropper);
        let hover = event(&s, 1, PenPhase::Hover, 0.);
        s.cursor_input(Some(hover));
        s.frame(1, 1).unwrap();
        picker_reply(&mut s, [0.2, 0.4, 0.8, 1.]);
        assert_eq!(s.state.display_colors(), &original);
        assert_eq!(s.state.colors, artwork);
        assert_eq!(
            s.color_picker_overlay().unwrap().original,
            original
                .definition()
                .linear_in(s.engine.document().composition().color.space)
                .unwrap()
        );
        let preview = s.state.preview_colors().into_owned();
        assert_ne!(preview, original);
        picker_pointer(
            &mut s,
            1,
            ContactPhase::Down,
            PointerKind::Mouse,
            [hover.surface_position.x, hover.surface_position.y],
        );
        s.frame(13, 13).unwrap();
        assert_eq!(s.state.display_colors(), &preview);
        assert_eq!(s.state.colors, artwork);
        assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Paint);

        // Leaving a mask while a sample is in flight must not recolor artwork.
        invoke(&mut s, CommandId::Eyedropper);
        s.cursor_input(Some(hover));
        s.frame(14, 14).unwrap();
        invoke(&mut s, CommandId::ReturnToArtwork);
        picker_reply(&mut s, [1., 0., 0., 1.]);
        assert_eq!(s.state.colors, artwork);
        assert_eq!(s.selection_masks.colors, preview);
        assert!(s.state.color_picker.preview.is_none());
        assert!(s.color_picker_overlay().is_none());
        assert_eq!(s.renderer_mut().dabs, 0);
    }
}

#[test]
fn color_picker_cancellation_rejects_inflight_results_and_restores_wheel() {
    for cancel in 0..5 {
        let mut s = session(Platform::Gtk);
        let original = s.state.colors.clone();
        invoke(&mut s, CommandId::Eyedropper);
        let hover = event(&s, 1, PenPhase::Hover, 0.);
        s.cursor_input(Some(hover));
        s.frame(1, 1).unwrap();
        match cancel {
            0 => {
                invoke(&mut s, CommandId::Eyedropper);
            }
            1 => {
                key(&mut s, "Escape", true, false, false);
            }
            2 => {
                s.input(UiInput::Blur).unwrap();
            }
            3 => {
                invoke(&mut s, CommandId::Move);
            }
            _ => {
                picker_pointer(
                    &mut s,
                    2,
                    ContactPhase::Down,
                    PointerKind::Touch,
                    [200., 200.],
                );
            }
        }
        picker_reply(&mut s, [1., 0., 0., 1.]);
        assert_eq!(s.state.colors, original);
        assert_eq!(s.state.layer_tools.tool, if cancel == 3 { LayerCanvasTool::Move } else { LayerCanvasTool::Paint });
        assert!(s.state.color_picker.preview.is_none());
        assert!(s.color_picker_overlay().is_none());
    }
}

#[test]
fn color_picker_touch_tracks_one_contact_toggles_source_and_commits_on_lift() {
    let mut s = session(Platform::Gtk);
    let original = s.state.colors.clone();
    let camera = s.state.camera.clone();
    picker_pointer(&mut s, 8, ContactPhase::Down, PointerKind::Touch, [250., 300.]);
    s.input(UiInput::ColorPickerHold {
        id: 8,
        position: [250., 300.],
        offset: 44.,
    })
    .unwrap();
    let ring = s.color_picker_overlay().unwrap();
    assert_eq!(ring.center, [250., 256.]);
    assert_eq!(ring.sample, ring.center);
    assert!(!ring.layer);
    picker_pointer(
        &mut s,
        8,
        ContactPhase::Move,
        PointerKind::Touch,
        [300., 350.],
    );
    picker_pointer(
        &mut s,
        9,
        ContactPhase::Down,
        PointerKind::Touch,
        [800., 600.],
    );
    assert!(s.state.color_picker.layer);
    assert_eq!(s.color_picker_overlay().unwrap().sample, [300., 306.]);
    assert!(s.color_picker_overlay().unwrap().layer);
    picker_pointer(
        &mut s,
        9,
        ContactPhase::Move,
        PointerKind::Touch,
        [700., 500.],
    );
    picker_pointer(
        &mut s,
        9,
        ContactPhase::Up,
        PointerKind::Touch,
        [700., 500.],
    );
    assert_eq!(s.state.camera, camera);
    s.frame(1, 1).unwrap();
    assert!(matches!(
        s.renderer_mut().sample_requests.last().unwrap().source,
        layer_render::ColorSampleSource::Source(_)
    ));
    let aim = s
        .state
        .camera
        .input_transform()
        .map(layer_core::Point { x: 300., y: 306. });
    assert_eq!(
        s.renderer_mut().sample_requests.last().unwrap().position,
        [aim.x.floor() as u32, aim.y.floor() as u32]
    );
    picker_reply(&mut s, [0.2, 0.5, 0.1, 1.]);
    assert_eq!(s.state.colors, original);
    picker_pointer(
        &mut s,
        8,
        ContactPhase::Up,
        PointerKind::Touch,
        [300., 350.],
    );
    s.frame(12, 12).unwrap();
    assert_ne!(s.state.colors, original);
    assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Paint);
    assert!(s.eyedropper.picking.consumed.is_empty());
    assert_eq!(s.renderer_mut().dabs, 0);
}

#[test]
fn color_picker_retires_touches_that_predate_toolbar_entry() {
    for platform in Platform::ALL {
        for terminal in [ContactPhase::Up, ContactPhase::Cancel] {
            for finger_cancels in [false, true] {
                let mut s = session(platform);
                let camera = s.state.camera.clone();
                picker_pointer(&mut s, 10, ContactPhase::Down, PointerKind::Touch, [200., 200.]);
                s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }).unwrap();
                picker_pointer(&mut s, 10, terminal, PointerKind::Touch, [200., 200.]);
                if finger_cancels {
                    picker_pointer(&mut s, 11, ContactPhase::Down, PointerKind::Touch, [400., 200.]);
                    picker_pointer(&mut s, 11, ContactPhase::Up, PointerKind::Touch, [400., 200.]);
                } else {
                    s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }).unwrap();
                }
                // Each isolated finger must remain inert; no pen press or blur
                // between them is allowed to conceal a stranded contact.
                for id in 12..15 {
                    picker_pointer(&mut s, id, ContactPhase::Down, PointerKind::Touch, [400., 200.]);
                    picker_pointer(&mut s, id, ContactPhase::Move, PointerKind::Touch, [500., 300.]);
                    picker_pointer(&mut s, id, terminal, PointerKind::Touch, [500., 300.]);
                    assert_eq!(s.state.camera, camera, "{platform:?}/{terminal:?}");
                }
                // New, genuinely simultaneous contacts still navigate.
                picker_pointer(&mut s, 20, ContactPhase::Down, PointerKind::Touch, [200., 200.]);
                picker_pointer(&mut s, 21, ContactPhase::Down, PointerKind::Touch, [400., 200.]);
                picker_pointer(&mut s, 21, ContactPhase::Move, PointerKind::Touch, [500., 300.]);
                assert_ne!(s.state.camera.zoom, camera.zoom);
                assert_ne!(s.state.camera.rotation, camera.rotation);
            }
        }
    }
}

#[test]
fn color_picker_hold_requires_a_live_unclaimed_single_touch() {
    for interruption in ["up", "cancel", "blur", "pen", "second finger", "picker", "picker cancelled"] {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Move);
        let previous = s.state.layer_tools.tool;
        picker_pointer(&mut s, 8, ContactPhase::Down, PointerKind::Touch, [250., 300.]);
        match interruption {
            "up" => { picker_pointer(&mut s, 8, ContactPhase::Up, PointerKind::Touch, [250., 300.]); }
            "cancel" => { picker_pointer(&mut s, 8, ContactPhase::Cancel, PointerKind::Touch, [250., 300.]); }
            "blur" => { s.input(UiInput::Blur).unwrap(); }
            "pen" => {
                picker_pointer(&mut s, 50, ContactPhase::Down, PointerKind::Pen, [400., 400.]);
                picker_pointer(&mut s, 50, ContactPhase::Up, PointerKind::Pen, [400., 400.]);
            }
            "second finger" => { picker_pointer(&mut s, 9, ContactPhase::Down, PointerKind::Touch, [400., 300.]); }
            _ => {
                s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }).unwrap();
                if interruption == "picker cancelled" {
                    s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }).unwrap();
                }
            }
        }
        let tool = s.state.layer_tools.tool;
        s.input(UiInput::ColorPickerHold { id: 8, position: [250., 300.], offset: 44. }).unwrap();
        assert_eq!(s.state.layer_tools.tool, tool, "{interruption}");
        if interruption == "picker" {
            s.picker_position([250., 300.]);
            assert_eq!(s.color_picker_overlay().unwrap().sample, [250., 300.]);
            s.dispatch(UiAction::ColorPicker { action: ColorPickerAction::Toggle }).unwrap();
        }
        assert_eq!(s.state.layer_tools.tool, previous, "{interruption}");
    }
}

#[test]
fn color_picker_consumed_touch_does_not_capture_another_pointer_kind() {
    for kind in [PointerKind::Mouse, PointerKind::Pen] {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::Eyedropper);
        picker_pointer(&mut s, 1, ContactPhase::Down, PointerKind::Touch, [200., 200.]);
        assert!(picker_pointer(&mut s, 1, ContactPhase::Down, kind, [400., 400.]).paint);
        assert!(!picker_pointer(&mut s, 1, ContactPhase::Up, PointerKind::Touch, [200., 200.]).paint);
        // The picker no longer owns this touch; it still cannot release the
        // pen/mouse contact with the same numeric ID in ordinary routing.
        assert!(!picker_pointer(&mut s, 1, ContactPhase::Down, PointerKind::Touch, [200., 200.]).paint);
        assert!(!picker_pointer(&mut s, 1, ContactPhase::Cancel, PointerKind::Touch, [200., 200.]).paint);
        assert!(picker_pointer(&mut s, 1, ContactPhase::Move, kind, [450., 450.]).paint);
        assert!(picker_pointer(&mut s, 1, ContactPhase::Up, kind, [450., 450.]).paint);
        assert!(s.interaction.pointer.is_none());
    }
}

#[test]
fn touch_release_cleans_up_even_when_normal_routing_is_blocked() {
    for terminal in [ContactPhase::Up, ContactPhase::Cancel] {
        for blocked in ["settings", "workspace transition", "invalid position"] {
            let mut s = session(Platform::Gtk);
            picker_pointer(&mut s, 1, ContactPhase::Down, PointerKind::Touch, [200., 200.]);
            s.state.settings_open = blocked == "settings";
            s.workspace_transition = blocked == "workspace transition";
            let reply = s.input(pointer_input(1, terminal, PointerKind::Touch, PointerButton::Primary, if blocked == "invalid position" { [f32::NAN; 2] } else { [200., 200.] }, 0));
            assert_eq!(reply.is_err(), blocked == "invalid position");
            assert!(!s.touch.is_active(), "{blocked}/{terminal:?}");
        }
    }
}

#[test]
fn color_picker_motion_previews_without_starvation_and_press_rejects_old_readback() {
    let mut s = session(Platform::Gtk);
    let original = s.state.colors.clone();
    invoke(&mut s, CommandId::Eyedropper);
    s.picker_position([200., 200.]);
    s.frame(1, 1).unwrap();
    let first = s.renderer_mut().sample_requests[0];
    s.picker_position([300., 300.]);
    s.renderer_mut().sample_reply = Some(layer_render::ColorSample {
        request_id: first.request_id,
        rgba: [1., 0., 0., 1.],
    });
    s.frame(2, 2).unwrap();
    assert!(s.state.color_picker.preview.is_some());
    assert_eq!(s.renderer_mut().sample_requests.len(), 2);
    picker_pointer(
        &mut s,
        1,
        ContactPhase::Down,
        PointerKind::Pen,
        [300., 300.],
    );
    picker_pointer(&mut s, 1, ContactPhase::Up, PointerKind::Pen, [300., 300.]);
    picker_reply(&mut s, [0., 1., 0., 1.]); // Old hover is drained, never committed.
    assert_eq!(s.state.colors, original);
    assert!(s.state.layer_tools.tool.picks_color());
    picker_reply(&mut s, [0.; 4]); // Exact acceptance point is transparent.
    assert_eq!(s.state.colors, original);
    assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Paint);
}

#[test]
fn color_picker_size_and_style_are_settings_and_navigator_stays_navigation() {
    let mut s = session(Platform::Gtk);
    s.state.workspace.layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    invoke(&mut s, CommandId::Eyedropper);
    let anchor = tile_anchor(&s.state.workspace.layout, ToolbarControl::ColorPicker);
    let mut drawer = ContentDrawer::for_tile(&s.state.workspace.layout, anchor).unwrap();
    drawer.configure_picker(&s.state.workspace.layout);
    assert_eq!(drawer.columns, [vec![Panel::ToolSettings]]);
    assert_eq!(drawer.column_widths(1200.), [240.]);
    assert_eq!(drawer.dismissal, DrawerDismissal::Explicit);
    s.state.customization.drawer = Some(drawer.clone());
    chrome(
        &mut s,
        ChromeEvent::Contact {
            position: [900., 20.],
            canvas: false,
        },
        ChromeFacts::default(),
    );
    assert_eq!(s.state.customization.drawer, Some(drawer.clone()));
    assert_eq!(
        s.state
            .tool_set
            .subtools
            .iter()
            .map(|i| i.label.as_ref())
            .collect::<Vec<_>>(),
        ["Color Picker", "Eyedropper"]
    );
    for width in crate::COLOR_SAMPLE_WIDTHS {
        s.dispatch(UiAction::SetColorSampleSize { width }).unwrap();
        assert_eq!(s.state.color_picker.sample_width, width);
    }
    s.dispatch(UiAction::ColorPicker {
        action: ColorPickerAction::Style {
            style: ColorPickerStyle::Eyedropper,
        },
    })
    .unwrap();
    s.picker_position([300., 300.]);
    assert!(s.color_picker_overlay().unwrap().classic);
    for layer in [true, false] {
        s.dispatch(UiAction::ColorPicker {
            action: ColorPickerAction::Source { layer },
        })
        .unwrap();
        assert_eq!(s.color_picker_overlay().unwrap().layer, layer);
        assert_eq!(s.state.customization.drawer, Some(drawer.clone()));
    }
    let color = s.state.colors.clone();
    s.dispatch(UiAction::Navigator {
        phase: ContactPhase::Down,
        position: [40., 40.],
        viewport: [220., 164.],
    })
    .unwrap();
    s.dispatch(UiAction::Navigator {
        phase: ContactPhase::Up,
        position: [80., 80.],
        viewport: [220., 164.],
    })
    .unwrap();
    assert_eq!(s.state.colors, color);
}

#[test]
fn standalone_picker_restores_glass_and_categories_retain_both_tools() {
    let mut s = session(Platform::Gtk);
    s.state.color_picker.style = ColorPickerStyle::Eyedropper;
    s.dispatch(ToolbarControl::ColorPicker.action().unwrap())
        .unwrap();
    assert_eq!(s.state.color_picker.style, ColorPickerStyle::Glass);
    assert!(tool_state(s.state(), ToolbarControl::ColorPicker).1);
    s.dispatch(ToolbarControl::ColorPicker.action().unwrap())
        .unwrap();
    assert!(!s.state.layer_tools.tool.picks_color());
    assert!(!tool_state(s.state(), ToolbarControl::ColorPicker).1);
    for preset in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
        s.state.workspace.layout = preset.layout(Platform::Gtk);
        let anchor = tile_anchor(&s.state.workspace.layout, ToolbarControl::Command { command: CommandId::Eyedropper });
        let mut drawer = ContentDrawer::for_tile(&s.state.workspace.layout, anchor).unwrap();
        drawer.configure_picker(&s.state.workspace.layout);
        assert_eq!(
            drawer.columns,
            [vec![Panel::Brushes], vec![Panel::ToolSettings]]
        );
        assert_eq!(drawer.column_widths(1200.), [184., 240.]);
        assert_eq!(drawer.dismissal, DrawerDismissal::Explicit);
        assert_eq!(s.command(CommandId::Eyedropper).icon, Some("eyedropper"));
        assert_eq!(s.command(CommandId::Eyedropper).label.as_ref(), "Eyedropper");
        invoke(&mut s, CommandId::Eyedropper);
        assert_eq!(
            s.state
                .tool_set
                .subtools
                .iter()
                .map(|item| item.label.as_ref())
                .collect::<Vec<_>>(),
            ["Color Picker", "Eyedropper"]
        );
        invoke(&mut s, CommandId::Eyedropper);
    }
}

#[test]
fn picker_preview_keeps_workspace_models_retained_on_supported_hosts() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        invoke(&mut s, CommandId::Eyedropper);
        let model = s.workspace_model_revision();
        let content = s.workspace_content_revision();
        let colors = s.state.colors.clone();
        let hover = event(&s, 1, PenPhase::Hover, 0.);
        s.cursor_input(Some(hover));
        s.frame(1, 1).unwrap();
        picker_reply(&mut s, [0.2, 0.4, 0.8, 1.]);
        assert!(s.state.color_picker.preview.is_some());
        assert_eq!(s.workspace_model_revision(), model);
        assert_eq!(s.workspace_content_revision(), content);
        let reply = s.input(UiInput::CursorLeave).unwrap();
        assert_eq!(reply.change.regions, regions::COLOR_PREVIEW);
        assert!(s.state.color_picker.preview.is_none());
        assert_eq!(s.state.colors, colors);
        assert_eq!(s.workspace_model_revision(), model);
        assert!(s.state.layer_tools.tool.picks_color());
    }
}
