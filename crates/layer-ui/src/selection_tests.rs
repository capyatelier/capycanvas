mod selection_tools_checks {
    use super::*;

    fn send(s: &mut UiSession<Recorder>, phase: PenPhase, p: [f32; 2]) {
        let mut e = event(s, 1, phase, 1.);
        let m = s.state.camera.document_to_surface();
        e.surface_position = Point {
            x: m[0] * p[0] + m[2] * p[1] + m[4],
            y: m[1] * p[0] + m[3] * p[1] + m[5],
        };
        s.pen(e).unwrap();
        s.frame(1, 1).unwrap();
        assert!(s.state.host_error.is_none(), "{:?}", s.state.host_error);
    }
    fn click(s: &mut UiSession<Recorder>, p: [f32; 2]) {
        send(s, PenPhase::Down, p);
        send(s, PenPhase::Up, p);
    }
    fn bounds(s: &UiSession<Recorder>) -> [f32; 4] {
        let selection = s.engine.document().selection.as_ref().unwrap();
        let points = selection.contours()[0].iter();
        points.fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |mut b, p| {
                b[0] = b[0].min(p.x);
                b[1] = b[1].min(p.y);
                b[2] = b[2].max(p.x);
                b[3] = b[3].max(p.y);
                b
            },
        )
    }
    #[test]
    fn geometric_selection_constraints_history_cancel_and_workspace_memory() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android] {
            for command in [CommandId::RectangleSelect, CommandId::EllipseSelect] {
                let mut s = session();
                s.set_platform(platform);
                invoke(&mut s, command);
                // Reversed drags work under a transformed camera.
                s.state.camera.zoom = 2.;
                s.state.camera.rotation = 0.4;
                s.sync_camera();
                send(&mut s, PenPhase::Down, [180., 140.]);
                send(&mut s, PenPhase::Move, [60., 40.]);
                assert!(s.engine.document().selection.is_none());
                assert!(!s.selection_outline().is_empty());
                send(&mut s, PenPhase::Up, [60., 40.]);
                let b = bounds(&s);
                for (actual, expected) in b.into_iter().zip([60., 40., 180., 140.]) {
                    assert!((actual - expected).abs() < 0.3, "{b:?}");
                }
                let selection = s.engine.document().selection.clone();
                invoke(&mut s, CommandId::Undo);
                assert!(s.engine.document().selection.is_none());
                invoke(&mut s, CommandId::Redo);
                assert_eq!(s.engine.document().selection, selection);
                send(&mut s, PenPhase::Down, [0., 0.]);
                send(&mut s, PenPhase::Cancel, [20., 20.]);
                assert_eq!(s.engine.document().selection, selection);
                click(&mut s, [80., 80.]);
                assert_eq!(s.engine.document().selection, selection);
                invoke(&mut s, CommandId::SelectionFixedSize);
                for (id, value) in [("selection_width", 80.), ("selection_height", 40.)] {
                    s.dispatch(UiAction::SetToolSetting {
                        id: id.into(),
                        value,
                    })
                    .unwrap();
                }
                assert!(
                    s.dispatch(UiAction::SetToolSetting {
                        id: "selection_width".into(),
                        value: 1.5
                    })
                    .is_err()
                );
                invoke(&mut s, CommandId::SelectionFromCenter);
                click(&mut s, [200., 200.]);
                let b = bounds(&s);
                for (actual, expected) in b.into_iter().zip([160., 180., 240., 220.]) {
                    assert!((actual - expected).abs() < 0.3, "{b:?}");
                }
                let capture = s.capture_workspace().unwrap();
                let saved = serde_json::to_string(&capture).unwrap();
                let mut restored = session();
                restored.set_platform(platform);
                restored
                    .adopt_workspace(
                        PreparedWorkspace::new(serde_json::from_str(&saved).unwrap()).unwrap(),
                    )
                    .unwrap();
                invoke(&mut restored, CommandId::Brush);
                assert!(!restored.command(CommandId::Select).selected);
                assert_eq!(restored.command(CommandId::Select).icon, command.icon());
                invoke(&mut restored, CommandId::Select);
                assert!(restored.command(command).selected);
                assert_eq!(restored.command(CommandId::Select).icon, command.icon());
                assert_eq!(restored.selection_tools.options, s.selection_tools.options);
                invoke(&mut s, CommandId::SelectionFixedRatio);
                s.dispatch(UiAction::SetToolSetting {
                    id: "selection_ratio_width".into(),
                    value: 2.,
                })
                .unwrap();
                send(&mut s, PenPhase::Down, [200., 200.]);
                send(&mut s, PenPhase::Up, [230., 240.]);
                let b = bounds(&s);
                assert!(((b[2] - b[0]) / (b[3] - b[1]) - 2.).abs() < 0.02);
                send(&mut s, PenPhase::Down, [200., 200.]);
                s.interaction.modifiers.shift = true;
                send(&mut s, PenPhase::Up, [230., 240.]);
                let b = bounds(&s);
                assert!(((b[2] - b[0]) / (b[3] - b[1]) - 1.).abs() < 0.02);
            }
        }
    }
    #[test]
    fn polygon_selection_points_finish_backspace_blur_and_history() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::PolygonSelect);
        click(&mut s, [20., 20.]);
        click(&mut s, [150., 20.]);
        click(&mut s, [150., 160.]);
        assert!(s.engine.document().selection.is_none());
        assert!(s.command(CommandId::CompleteSelection).enabled);
        s.selection_key("backspace").unwrap();
        assert_eq!(s.layer_interaction.path.len(), 2);
        assert!(!s.command(CommandId::CompleteSelection).enabled);
        click(&mut s, [120., 140.]);
        let mut predicted = event(&s, 5, PenPhase::Move, 1.);
        predicted.flags = SampleFlags::PREDICTED;
        s.pen(predicted).unwrap();
        assert_eq!(s.layer_interaction.path.len(), 3);
        s.selection_key("enter").unwrap();
        s.frame(2, 2).unwrap();
        let selection = s.engine.document().selection.clone();
        assert_eq!(selection.as_ref().unwrap().contours()[0].len(), 3);
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().selection, selection);
        click(&mut s, [40., 40.]);
        click(&mut s, [100., 40.]);
        s.input(UiInput::Blur).unwrap();
        assert!(s.layer_interaction.path.is_empty());
        assert_eq!(s.engine.document().selection, selection);
        click(&mut s, [60., 60.]);
        click(&mut s, [200., 60.]);
        click(&mut s, [200., 200.]);
        click(&mut s, [60., 60.]);
        assert!(s.layer_interaction.path.is_empty());
        assert_ne!(s.engine.document().selection, selection);
        click(&mut s, [20., 20.]);
        invoke(&mut s, CommandId::Brush);
        assert!(s.layer_interaction.path.is_empty());
    }
    #[test]
    fn global_color_selection_uses_sources_and_rejects_stale_results() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::ColorSelect);
        assert_eq!(s.state.tool_set.subtools.len(), 8);
        assert!(!s.state.tool_settings.iter().any(|c| c.id == "gap_closing"));
        s.region_tools.refinement.gap_closing = 5;
        for command in [
            CommandId::SelectionVisible,
            CommandId::SelectionEditing,
            CommandId::SelectionReference,
        ] {
            if command == CommandId::SelectionReference {
                s.dispatch(UiAction::Layer {
                    action: LayerAction::ReferenceSelection,
                })
                .unwrap();
            }
            invoke(&mut s, command);
            click(&mut s, [40., 60.]);
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            assert!(!request.contiguous);
            assert_eq!(request.refinement.gap_closing, 0);
            assert!(s.command(command).selected);
            invoke(&mut s, CommandId::Lasso);
            s.renderer_mut().region_reply = Some(layer_render::RegionResult {
                tonal_sample: None,
                request_id: request.request_id,
                pixels: std::sync::Arc::new(
                    layer_core::SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x44444444])
                        .unwrap(),
                ),
            });
            s.frame(4, 4).unwrap();
            assert!(s.engine.document().selection.is_none());
            invoke(&mut s, CommandId::ColorSelect);
        }
        invoke(&mut s, CommandId::AutoSelect);
        click(&mut s, [40., 60.]);
        assert!(s.renderer_mut().region_requests.last().unwrap().contiguous);
        assert_eq!(
            s.renderer_mut()
                .region_requests
                .last()
                .unwrap()
                .refinement
                .gap_closing,
            5
        );
    }
    #[test]
    fn selection_defaults_and_tools_follow_platform_rollout() {
        for platform in [
            Platform::Gtk,
            Platform::Web,
            Platform::Android,
            Platform::Mac,
            Platform::Ios,
            Platform::Windows,
        ] {
            let layout = WorkspacePreset::Painter.layout(platform);
            assert_eq!(
                layout.header.entries().any(|e| e.item
                    == HeaderItem::Tool {
                        control: ToolbarControl::Command {
                            command: CommandId::Select
                        }
                    }),
                matches!(platform, Platform::Gtk | Platform::Web | Platform::Android)
            );
            let photo = WorkspacePreset::Photographer.layout(platform);
            for command in [
                CommandId::RectangleSelect,
                CommandId::EllipseSelect,
                CommandId::PolygonSelect,
                CommandId::ColorSelect,
            ] {
                assert_eq!(
                    command.available_on(platform),
                    matches!(platform, Platform::Gtk | Platform::Web | Platform::Android)
                );
                assert_eq!(
                    photo
                        .panel(Panel::Toolbar)
                        .unwrap()
                        .tiles()
                        .iter()
                        .any(|t| t.control == ToolbarControl::Command { command }),
                    matches!(platform, Platform::Gtk | Platform::Web | Platform::Android)
                );
            }
        }
    }
    #[test]
    fn all_selection_tools_share_options_with_atomic_history_and_persistence() {
        use std::sync::Arc;
        for tool in SelectionTool::ALL.into_iter().filter(|t| !matches!(t, SelectionTool::Brush | SelectionTool::Tonal)) {
            let mut s = session();
            s.set_platform(Platform::Gtk);
            invoke(&mut s, tool.command());
            for command in [
                CommandId::SelectionNew,
                CommandId::SelectionAdd,
                CommandId::SelectionSubtract,
                CommandId::SelectionIntersect,
                CommandId::SelectionAntialias,
            ] {
                assert!(
                    s.state.tool_actions.iter().any(|a| a.command == command),
                    "{tool:?}"
                );
            }
            assert!(
                s.state
                    .tool_settings
                    .iter()
                    .any(|c| c.id == "selection_feather")
            );
            assert_eq!(
                s.state.tool_settings.iter().any(|c| c.id == "gap_closing"),
                tool == SelectionTool::Wand
            );
            invoke(&mut s, CommandId::SelectAll);
            let original = s.engine.document().selection.clone();
            invoke(&mut s, CommandId::SelectionAdd);
            invoke(&mut s, CommandId::SelectionAntialias);
            assert!(!s.command(CommandId::SelectionAntialias).selected);
            assert!(!s.state.tool_settings.iter().any(|c| c.id == "smoothing"));
            s.dispatch(UiAction::SetToolSetting {
                id: "selection_feather".into(),
                value: 8.,
            })
            .unwrap();
            for value in [-1., 101., f32::NAN, f32::INFINITY] {
                assert!(
                    s.dispatch(UiAction::SetToolSetting {
                        id: "selection_feather".into(),
                        value
                    })
                    .is_err()
                );
            }
            assert_eq!(s.selection_tools.options.feather, 8.);
            match tool {
                SelectionTool::Polygon => {
                    for p in [[20., 20.], [80., 20.], [80., 80.]] {
                        click(&mut s, p);
                    }
                    invoke(&mut s, CommandId::CompleteSelection);
                    s.frame(1, 1).unwrap();
                }
                SelectionTool::Lasso => {
                    send(&mut s, PenPhase::Down, [20., 20.]);
                    send(&mut s, PenPhase::Move, [80., 20.]);
                    send(&mut s, PenPhase::Up, [80., 80.]);
                }
                SelectionTool::Rectangle | SelectionTool::Ellipse => {
                    send(&mut s, PenPhase::Down, [20., 20.]);
                    send(&mut s, PenPhase::Up, [80., 80.]);
                }
                _ => click(&mut s, [40., 40.]),
            }
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            let options = request.selection.as_ref().unwrap();
            assert_eq!(options.mode, SelectionMode::Add);
            assert_eq!(options.feather, 8.);
            assert!(!options.antialias);
            assert_eq!(options.previous.as_deref(), original.as_ref());
            assert_eq!(
                s.engine.document().selection,
                original,
                "no partial history entry"
            );
            s.renderer_mut().region_reply = Some(layer_render::RegionResult {
                tonal_sample: None,
                request_id: request.request_id,
                pixels: Arc::new(
                    layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xff804020])
                        .unwrap(),
                ),
            });
            s.frame(1, 1).unwrap();
            let result = s.engine.document().selection.clone();
            assert_ne!(result, original);
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().selection, original);
            invoke(&mut s, CommandId::Redo);
            assert_eq!(s.engine.document().selection, result);
            let working = s.capture_workspace().unwrap();
            let restored: WorkspaceCapture =
                serde_json::from_str(&serde_json::to_string(&working).unwrap()).unwrap();
            assert_eq!(restored.working.selection, s.selection_tools.options);
        }
    }
    #[test]
    fn selection_options_defaults_stale_results_and_polygon_angle_constraint() {
        let old: SelectionOptions=serde_json::from_str(r#"{"tool":"rectangle","constraint":"free","from_center":false,"ratio":[1,1],"size":[256,256]}"#).unwrap();
        assert_eq!(old.mode, SelectionMode::New);
        assert!(old.antialias);
        assert_eq!(old.feather, 0.);
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::PolygonSelect);
        invoke(&mut s, CommandId::SelectionConstrainAngles);
        click(&mut s, [20., 20.]);
        click(&mut s, [160., 24.]);
        assert!((s.layer_interaction.path[1].y - 20.).abs() < 0.001);
        click(&mut s, [162., 50.]);
        assert!((s.layer_interaction.path[2].x - s.layer_interaction.path[1].x).abs() < 0.001);
        click(&mut s, [20., 20.]);
        assert!(
            s.layer_interaction.path.is_empty(),
            "first vertex closes even when the closing edge is not at 45 degrees"
        );
        assert!(s.engine.document().selection.is_some());
        invoke(&mut s, CommandId::Deselect);
        invoke(&mut s, CommandId::RectangleSelect);
        invoke(&mut s, CommandId::SelectionAdd);
        send(&mut s, PenPhase::Down, [20., 20.]);
        send(&mut s, PenPhase::Up, [80., 80.]);
        let id = s.renderer_mut().region_requests.last().unwrap().request_id;
        invoke(&mut s, CommandId::SelectionSubtract);
        s.renderer_mut().region_reply = Some(layer_render::RegionResult {
                tonal_sample: None,
            request_id: id,
            pixels: std::sync::Arc::new(
                layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xffffffff]).unwrap(),
            ),
        });
        s.frame(1, 1).unwrap();
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn held_selection_modifiers_latch_per_contact_and_preserve_configured_mode() {
        for (shift, alt, expected) in [(true,false,SelectionMode::Add),(false,true,SelectionMode::Subtract),(true,true,SelectionMode::Intersect)] {
            for tool in [CommandId::RectangleSelect, CommandId::EllipseSelect, CommandId::Lasso] {
                let mut s = session(); s.set_platform(Platform::Gtk);
                invoke(&mut s, tool);
                s.interaction.modifiers.shift = shift; s.interaction.modifiers.alt = alt;
                assert_eq!(s.effective_selection_mode(), expected);
                send(&mut s, PenPhase::Down, [20.,20.]);
                s.interaction.modifiers = Default::default();
                send(&mut s, PenPhase::Move, [80.,20.]);
                send(&mut s, PenPhase::Up, [80.,60.]);
                let request = s.renderer_mut().region_requests.last().unwrap();
                assert_eq!(request.selection.as_ref().unwrap().mode, expected);
                assert_eq!(s.selection_tools.options.mode, SelectionMode::New);
                assert_eq!(s.effective_selection_mode(), SelectionMode::New);
            }
        }
        let mut s = session(); s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectionBrush);
        s.interaction.modifiers.alt = true;
        assert_eq!(s.effective_selection_mode(), SelectionMode::Subtract);
        s.interaction.modifiers = Default::default();
        assert_eq!(s.effective_selection_mode(), SelectionMode::Add);
    }

}
