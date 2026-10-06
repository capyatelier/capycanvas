mod enclose_fill_checks {
    use super::*;
    use layer_core::{Selection, SelectionMode, SelectionPixels, authored::SceneScope};
    use layer_render::{RegionRequest, RegionResult};
    use std::sync::Arc;

    fn contact(s: &mut UiSession<Recorder>, phase: PenPhase, point: [f32; 2]) {
        pen_at(s, 1, phase, point);
        assert!(s.state.host_error.is_none(), "{:?}", s.state.host_error);
    }

    fn enclose(s: &mut UiSession<Recorder>) {
        for (phase, point) in [
            (PenPhase::Down, [20., 30.]),
            (PenPhase::Move, [180., 30.]),
            (PenPhase::Move, [180., 190.]),
            (PenPhase::Up, [20., 190.]),
        ] {
            contact(s, phase, point);
        }
    }

    fn reply(s: &mut UiSession<Recorder>, request: &RegionRequest) -> Arc<SelectionPixels> {
        let pixels = Arc::new(SelectionPixels::bytes([8, 1], [0, 0, 8, 1], vec![0xffffffff; 2]).unwrap());
        s.renderer_mut().region_reply = Some(RegionResult {
            request_id: request.request_id, pixels: pixels.clone(),
            placement: layer_core::Affine::IDENTITY, tonal_sample: None,
        });
        pixels
    }

    #[test]
    fn enclose_fill_is_a_remembered_lasso_subtool_in_toolbar_and_header_drawers() {
        for platform in Platform::ALL {
            for control in [ToolbarControl::Command { command: CommandId::Fill },
                ToolbarControl::ToolSlot { slot: ToolSlotId::PhotoFill }] {
                let (mut s, panel, ids) = group_fixture(platform, &[control]);
                let tile = DrawerAnchor::Tile { panel, tile: ids[0] };
                let header = add_group_header(&mut s, control);
                let enclose = ToolVariant::Command { command: CommandId::EncloseFill };
                let fill_label = CommandId::Fill.localized_label(s.localization());
                let lasso_label = CommandId::LassoFill.localized_label(s.localization());
                let enclose_label = CommandId::EncloseFill.localized_label(s.localization());
                for anchor in [tile, header] {
                    s.dispatch(slot_choice(&s, anchor, enclose)).unwrap();
                    let check = |s: &UiSession<Recorder>, view: &ToolSetView| {
                        let categories = view.groups.iter().filter(|item|
                            [fill_label.as_ref(), lasso_label.as_ref(), enclose_label.as_ref()].contains(&item.label.as_ref()))
                            .map(|item| item.label.as_ref()).collect::<Vec<_>>();
                        assert_eq!(categories, [fill_label.as_ref(), lasso_label.as_ref()]);
                        assert_eq!(view.groups.iter().filter(|item| item.selected).map(|item| item.label.as_ref()).collect::<Vec<_>>(), [lasso_label.as_ref()]);
                        assert_eq!(view.subtools.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(), [lasso_label.as_ref(), enclose_label.as_ref()]);
                        assert_eq!(view.subtools.iter().filter(|item| item.selected).map(|item| item.label.as_ref()).collect::<Vec<_>>(), [enclose_label.as_ref()]);
                        assert!(s.command(CommandId::EncloseFill).selected);
                    };
                    check(&s, &s.state().tool_set);
                    activate_slot(&mut s, anchor);
                    let drawer = s.state().customization.drawer.as_ref().unwrap();
                    assert_eq!(drawer.anchor, anchor);
                    check(&s, drawer.tool_set.as_ref().unwrap());
                    for source in [CommandId::SelectionVisible, CommandId::SelectionEditing, CommandId::SelectionReference] {
                        let options = s.state().tool_options();
                        let ToolOption::Choice { label, items: variants, .. } = options.iter().find(|option|
                            matches!(option, ToolOption::Choice { id: "variant", .. })).unwrap() else { unreachable!() };
                        assert_eq!(label, &s.localization().text(MessageId::TOOLBAR_VARIANT));
                        assert_eq!(variants.len(), 2);
                        let ToolOption::Choice { items, .. } = options.iter().find(|option|
                            matches!(option, ToolOption::Choice { id: "selection-source", .. })).unwrap() else { unreachable!() };
                        assert_eq!(items.len(), 3);
                        let action = items.iter().find(|item| item.action == UiAction::Invoke { command: source }).unwrap().action.clone();
                        s.dispatch(UiAction::ToolbarEdit { context: s.state().toolbar_context(), action: Box::new(action) }).unwrap();
                        assert!(s.command(source).selected);
                        check(&s, &s.state().tool_set);
                        check(&s, s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap());
                    }
                    let category = |s: &UiSession<Recorder>, label: &str| s.state().tool_set.groups.iter().find(|item| item.label.as_ref() == label).unwrap().action.clone();
                    s.dispatch(category(&s, &fill_label)).unwrap();
                    assert!(s.command(CommandId::Fill).selected);
                    s.dispatch(category(&s, &lasso_label)).unwrap();
                    check(&s, &s.state().tool_set);
                    assert_eq!(selected_slot_variant(&s, tile), enclose);
                    assert_eq!(selected_slot_variant(&s, header), enclose);
                    let ordinary = s.state().tool_set.subtools.iter().find(|item| item.label.as_ref() == lasso_label.as_ref()).unwrap().action.clone();
                    s.dispatch(ordinary).unwrap();
                    assert!(s.command(CommandId::LassoFill).selected);
                    assert!(!s.state().tool_actions.iter().any(|action| action.group() == Some(ToolActionGroup::SelectionSource)));
                    s.dispatch(category(&s, &fill_label)).unwrap();
                    s.dispatch(category(&s, &lasso_label)).unwrap();
                    assert!(s.command(CommandId::LassoFill).selected);
                    activate_slot(&mut s, anchor);
                    assert!(s.state().customization.drawer.is_none());
                }
                s.dispatch(slot_choice(&s, tile, enclose)).unwrap();
                s.dispatch(slot_choice(&s, tile, ToolVariant::Command { command: CommandId::Fill })).unwrap();
                let capture = s.capture_workspace().unwrap();
                let mut restored = session(platform);
                restored.restore_editing(s.editing_state()).unwrap();
                restored.adopt_workspace(PreparedWorkspace::new(capture).unwrap()).unwrap();
                let category = restored.state().tool_set.groups.iter().find(|item| item.label.as_ref() == lasso_label.as_ref()).unwrap().action.clone();
                restored.dispatch(category).unwrap();
                assert!(restored.command(CommandId::EncloseFill).selected);
                assert_eq!(selected_slot_variant(&restored, tile), enclose);
                assert_eq!(selected_slot_variant(&restored, header), enclose);
            }
        }
    }

    #[test]
    fn enclose_fill_starts_with_reference_and_keeps_its_source_separate_from_bucket() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionReference).selected);
            assert!(ToolSlotId::Fill.variants().iter().any(|variant| variant.command() == CommandId::EncloseFill));
            assert!(ToolSlotId::PhotoFill.variants().iter().any(|variant| variant.command() == CommandId::EncloseFill));
            let checkpoint = s.engine.checkpoint();
            enclose(&mut s);
            s.frame(1, 1).unwrap();
            assert!(s.renderer_mut().region_requests.is_empty());
            assert_eq!(notice_text(&s), Some(s.localization().text(MessageId::RESOURCES_REFERENCE_TOOL_MARK_FIRST).as_ref()));
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert!(!s.engine.can_undo());
            invoke(&mut s, CommandId::SelectionEditing);
            invoke(&mut s, CommandId::Fill);
            assert!(s.command(CommandId::SelectionVisible).selected);
            invoke(&mut s, CommandId::SelectionReference);
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionEditing).selected);
            invoke(&mut s, CommandId::Fill);
            assert!(s.command(CommandId::SelectionReference).selected);
        }
    }

    #[test]
    fn enclose_fill_and_bucket_share_controls_and_source_commands_on_every_platform() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.set_references([s.engine.document().working.occurrence.unwrap()].into()).unwrap();
            for command in [CommandId::EncloseFill, CommandId::Fill] {
                invoke(&mut s, command);
                assert!(s.command(command).selected);
                assert_eq!(s.state.tool_settings.iter().map(|setting| setting.id).collect::<Vec<_>>(),
                    ["tolerance", "gap_closing", "expansion", "smoothing", "opacity"]);
                for (id, value) in [("tolerance", 0.25), ("gap_closing", 3.), ("expansion", -2.), ("smoothing", 0.5)] {
                    s.dispatch(UiAction::SetToolSetting { id: id.into(), value }).unwrap();
                }
                for (source_command, scope) in [
                    (CommandId::SelectionVisible, SceneScope::All),
                    (CommandId::SelectionEditing, SceneScope::Raw(s.engine.document().active_target().unwrap())),
                    (CommandId::SelectionReference, SceneScope::Members(vec![s.engine.document().working.occurrence.unwrap()].into())),
                ] {
                    assert!(s.command(source_command).enabled);
                    invoke(&mut s, source_command);
                    assert!(s.command(command).selected);
                    assert!(s.command(source_command).selected);
                    if command == CommandId::EncloseFill { enclose(&mut s); }
                    else {
                        contact(&mut s, PenPhase::Down, [40., 60.]);
                        contact(&mut s, PenPhase::Up, [40., 60.]);
                    }
                    s.frame(1, 1).unwrap();
                    let request = s.renderer_mut().region_requests.last().unwrap().clone();
                    assert_eq!(request.enclosure.is_some(), command == CommandId::EncloseFill);
                    assert!(request.contiguous);
                    assert_eq!(request.tolerance, 0.25);
                    assert_eq!(request.refinement.gap_closing, 3);
                    assert_eq!(request.refinement.expansion, -2);
                    assert_eq!(request.refinement.smoothing, 0.5);
                    let layer_render::RegionSource::Scene { scope: actual, .. } = &request.source else { panic!("scene source"); };
                    assert_eq!(actual, &scope);
                    assert!(s.cancel_layer_gesture().unwrap());
                    reply(&mut s, &request);
                    s.frame(2, 2).unwrap();
                }
            }
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionReference).selected);
        }
    }

    #[test]
    fn enclose_fill_keeps_camera_geometry_and_intersects_selection_after_detection() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle([80., 50., 120., 100.]));
        let previous = s.engine.document().working.selection.clone();
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        s.state.camera.zoom = 2.;
        s.state.camera.rotation = 0.4;
        s.sync_camera();
        contact(&mut s, PenPhase::Down, [20., 30.]);
        contact(&mut s, PenPhase::Move, [180., 30.]);
        contact(&mut s, PenPhase::Move, [180., 190.]);
        s.frame(2, 2).unwrap();
        assert!(s.renderer_mut().region_requests.is_empty());
        let mut overlay = Vec::new();
        s.append_layer_overlay(&mut overlay);
        assert!(overlay.len() >= 2);
        assert_eq!(s.engine.document().working.selection, previous);
        contact(&mut s, PenPhase::Up, [20., 190.]);
        s.frame(3, 3).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        let enclosure = request.enclosure.unwrap();
        assert_eq!(enclosure.contours().len(), 1);
        for (actual, expected) in enclosure.contours()[0].iter().zip([[20., 30.], [180., 30.], [180., 190.], [20., 190.]]) {
            assert!((actual.x - expected[0]).abs() < 0.001 && (actual.y - expected[1]).abs() < 0.001);
        }
        assert!(request.limit.is_none());
        let refinement = request.selection.unwrap();
        assert_eq!(refinement.mode, SelectionMode::Intersect);
        assert_eq!(refinement.previous.as_deref(), previous.as_ref());
        assert_eq!(s.engine.document().working.selection, previous);
    }

    #[test]
    fn enclose_fill_result_uses_atomic_paint_history_and_captured_opacity() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            let target = s.engine.document().active_target().unwrap();
            let original = s.engine.document().target_raster(target).unwrap().identity();
            invoke(&mut s, CommandId::EncloseFill);
            invoke(&mut s, CommandId::SelectionEditing);
            s.dispatch(UiAction::SetToolSetting { id: "opacity".into(), value: 0.25 }).unwrap();
            let checkpoint = s.engine.checkpoint();
            enclose(&mut s);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert!(s.engine.document().target_operations(target).unwrap().is_empty());
            s.frame(1, 1).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            s.dispatch(UiAction::SetToolSetting { id: "opacity".into(), value: 0.75 }).unwrap();
            let pixels = reply(&mut s, &request);
            s.frame(2, 2).unwrap();
            let operations = s.engine.document().target_operations(target).unwrap();
            assert_eq!(operations.len(), 1);
            assert_eq!(operations[0].coverage.selection, Some(Selection::pixels(pixels)));
            let layer_core::RasterOperationKind::Fill { color, .. } = operations[0].kind else { panic!("fill operation"); };
            assert_eq!(color[3], 0.25);
            assert!(s.engine.document().working.selection.is_none());
            s.frame(3, 3).unwrap();
            let committed = s.engine.document().target_raster(target).unwrap().identity();
            assert_ne!(original, committed);
            invoke(&mut s, CommandId::Undo);
            s.frame(4, 4).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
            assert!(!s.engine.can_undo());
            invoke(&mut s, CommandId::Redo);
            s.frame(5, 5).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), committed);
        }
    }

    #[test]
    fn enclose_fill_abandoned_paths_and_stale_results_leave_no_history() {
        for cancellation in 0..6 {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, CommandId::EncloseFill);
            invoke(&mut s, CommandId::SelectionEditing);
            let checkpoint = s.engine.checkpoint();
            let target = s.engine.document().active_target().unwrap();
            if cancellation < 3 {
                contact(&mut s, PenPhase::Down, [20., 30.]);
                contact(&mut s, PenPhase::Move, [180., 30.]);
                match cancellation {
                    0 => contact(&mut s, PenPhase::Cancel, [180., 190.]),
                    1 => { assert!(key(&mut s, "escape", true, false, false).handled); }
                    _ => { s.input(UiInput::Blur).unwrap(); }
                }
                contact(&mut s, PenPhase::Up, [20., 190.]);
            } else {
                enclose(&mut s);
                if cancellation == 3 { invoke(&mut s, CommandId::Brush); }
                else {
                    s.frame(1, 1).unwrap();
                    let request = s.renderer_mut().region_requests.last().unwrap().clone();
                    if cancellation == 4 { invoke(&mut s, CommandId::Brush); }
                    else { s.dispatch(UiAction::SetToolSetting { id: "tolerance".into(), value: 0.4 }).unwrap(); }
                    reply(&mut s, &request);
                }
            }
            s.frame(2, 2).unwrap();
            assert!(s.layer_interaction.path.is_empty());
            assert_eq!(s.engine.checkpoint(), checkpoint, "cancellation {cancellation}");
            assert!(!s.engine.can_undo());
            assert!(s.engine.document().target_operations(target).unwrap().is_empty());
            assert!(!s.region_tools.busy());
            assert!(s.renderer_mut().region_requests.is_empty() || cancellation >= 4);
        }
    }

    #[test]
    fn enclose_fill_new_enclosure_supersedes_pending_detection_without_losing_its_reply() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let target = s.engine.document().active_target().unwrap();
        enclose(&mut s);
        s.frame(1, 1).unwrap();
        let first = s.renderer_mut().region_requests.last().unwrap().clone();
        enclose(&mut s);
        s.frame(2, 2).unwrap();
        assert_eq!(s.renderer_mut().region_requests.len(), 1);
        reply(&mut s, &first);
        s.frame(3, 3).unwrap();
        assert!(s.engine.document().target_operations(target).unwrap().is_empty());
        assert!(!s.engine.can_undo());
        assert_eq!(s.renderer_mut().region_requests.len(), 2);
        let second = s.renderer_mut().region_requests.last().unwrap().clone();
        assert_ne!(first.request_id, second.request_id);
        reply(&mut s, &second);
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.document().target_operations(target).unwrap().len(), 1);
        s.frame(5, 5).unwrap();
        invoke(&mut s, CommandId::Undo);
        s.frame(6, 6).unwrap();
        assert!(!s.engine.can_undo());
    }

    #[test]
    fn enclose_fill_undo_waits_for_completed_detection_and_preserves_redo() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let target = s.engine.document().active_target().unwrap();
        let original = s.engine.document().target_raster(target).unwrap().identity();
        enclose(&mut s);
        s.frame(1, 1).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
        reply(&mut s, &request);
        for tick in 2..=5 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
        assert!(!s.engine.can_undo());
        assert!(s.engine.can_redo());
        invoke(&mut s, CommandId::Redo);
        s.frame(6, 6).unwrap();
        assert_ne!(s.engine.document().target_raster(target).unwrap().identity(), original);
        invoke(&mut s, CommandId::Undo);
        s.frame(7, 7).unwrap();
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
    }

    #[test]
    fn enclose_fill_stationary_contacts_and_empty_results_preserve_redo() {
        let mut s = session(Platform::Gtk);
        layer(&mut s, LayerAction::New { group: false, clipped: false });
        invoke(&mut s, CommandId::Undo);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let checkpoint = s.engine.checkpoint();
        contact(&mut s, PenPhase::Down, [40., 60.]);
        contact(&mut s, PenPhase::Move, [40., 60.]);
        contact(&mut s, PenPhase::Up, [40., 60.]);
        s.frame(1, 1).unwrap();
        assert!(s.renderer_mut().region_requests.is_empty());
        enclose(&mut s);
        s.frame(2, 2).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        s.renderer_mut().region_reply = Some(RegionResult {
            request_id: request.request_id,
            pixels: Arc::new(SelectionPixels::bytes([8, 1], [0; 4], vec![0; 2]).unwrap()),
            placement: layer_core::Affine::IDENTITY, tonal_sample: None,
        });
        s.frame(3, 3).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.engine.can_redo());
        assert!(!s.engine.can_undo());
    }

    #[test]
    fn enclose_fill_is_disabled_while_editing_masks() {
        for platform in Platform::ALL {
            for command in [CommandId::QuickMask, CommandId::NewSelectionLayer, CommandId::EditLayerMask] {
                let mut s = session(platform);
                if command == CommandId::EditLayerMask {
                    let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
                    layer(&mut s, LayerAction::AddMask { id, replace: false });
                    invoke(&mut s, CommandId::EditLayerContent);
                }
                invoke(&mut s, command);
                assert!(!s.command(CommandId::EncloseFill).enabled);
                assert!(s.dispatch(UiAction::Invoke { command: CommandId::EncloseFill }).is_err());
                assert!(s.dispatch(UiAction::Layer { action: LayerAction::Tool {
                    tool: LayerCanvasTool::EncloseFill { source: RegionSource::Visible },
                }}).is_err());
            }
        }
    }
}
