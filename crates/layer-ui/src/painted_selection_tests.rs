mod painted_selection_checks {
    use super::*;
    fn send(s: &mut UiSession<Recorder>, phase: PenPhase, x: f32) {
        let mut e = event(s, 1, phase, 1.);
        e.surface_position = Point { x, y: 100. };
        s.pen(e).unwrap();
    }
    fn reply(s: &mut UiSession<Recorder>, value: u32) {
        let id = s.engine.backend().selection_updates.last().unwrap().id;
        s.engine.backend_mut().selection_reply = Some(layer_render::SelectionPaintResult {
            changed: true,
            request_id: id,
            pixels: std::sync::Arc::new(
                layer_core::SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![value]).unwrap(),
            ),
        });
    }
    #[test]
    fn mask_final_replay_waits_for_the_existing_submission_acknowledgement() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let mut brush = s.engine.configured_brush().clone();
        brush.taper.end_distance_diameters = 2.;
        s.engine.set_brush(brush).unwrap();
        invoke(&mut s, CommandId::QuickMask);
        s.engine.backend_mut().selection_wait = true;
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Move, 200.);
        s.frame(1, 1).unwrap();
        let original = s.engine.backend().selection_updates[0].dabs.clone();
        send(&mut s, PenPhase::Up, 250.);
        s.frame(2, 2).unwrap();
        let pending = s.engine.backend().selection_updates.last().unwrap();
        assert_eq!(pending.dabs, original);
        assert!(!pending.finish && !pending.restart);
        s.engine.backend_mut().selection_wait = false;
        s.frame(3, 3).unwrap();
        s.frame(4, 4).unwrap();
        let replay = s.engine.backend().selection_updates.last().unwrap();
        assert!(replay.restart && replay.finish);
    }
    #[test]
    fn quick_mask_final_taper_replaces_preview_and_adoption_clears_runtime_target() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let mut brush = s.engine.configured_brush().clone();
        brush.taper.end_distance_diameters = 2.;
        s.engine.set_brush(brush).unwrap();
        invoke(&mut s, CommandId::QuickMask);
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Move, 200.);
        s.frame(1, 1).unwrap();
        assert!(!s.engine.backend().selection_updates.last().unwrap().restart);
        send(&mut s, PenPhase::Up, 250.);
        s.frame(2, 2).unwrap();
        let update = s.engine.backend().selection_updates.last().unwrap();
        assert!(update.restart && update.finish);
        reply(&mut s, 0x80808080);
        s.frame(3, 3).unwrap();
        assert!(s.state.layer_tools.quick_mask);
        assert!(s.state.layer_properties.controls.is_empty());
        assert!(!s.state.layer_tools.controls.opacity);
        assert!(s.dispatch(UiAction::SetLayerOpacity { id: None, opacity: 0.5 }).is_err());
        let epoch = s.state.document_file.epoch;
        let revision = s.engine.document().revision;
        assert!(
            s.adopt_project(Box::new(session()), epoch, revision, None)
                .is_ok()
        );
        assert!(!s.state.layer_tools.quick_mask);
        assert!(s.selection_masks.target().is_none());
        assert!(s.selection_masks.reselect.is_none());
    }
    #[test]
    fn mask_editing_blocks_artwork_filters_and_destructive_commands() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::QuickMask);
        let id = s.engine.document().active_layer.0;
        let layers = s.engine.document().layers.clone();
        for action in [
            LayerAction::Clear { id },
            LayerAction::Delete { id },
            LayerAction::AddMask { id, replace: false },
            LayerAction::FillSelection,
        ] {
            assert!(s.dispatch(UiAction::Layer { action }).is_err());
        }
        assert!(
            s.dispatch(UiAction::Effect {
                action: EffectAction::Insert {
                    effect: "missing".into()
                }
            })
            .unwrap_err()
            .contains("Return to artwork")
        );
        assert_eq!(s.engine.document().layers, layers);
    }
    #[test]
    fn quick_mask_colors_are_independent_grayscale_and_swap_the_paint_slot() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let artwork = s.state.colors.clone();
        invoke(&mut s, CommandId::QuickMask);
        s.mask_color_action(ColorAction::SetSlot {
            slot: ColorSlot::Foreground,
            color: layer_core::color::RgbColor::new(
                layer_core::color::RgbSpace::Srgb,
                [1., 0., 0., 1.],
            )
            .unwrap(),
        })
        .unwrap();
        let color = s.state.display_colors().definition().rgba;
        assert!((color[0] - 0.2126).abs() < 0.0001);
        assert_eq!(color[0], color[1]);
        assert_eq!(color[1], color[2]);
        invoke(&mut s, CommandId::SwapMaskColors);
        assert_eq!(s.selection_masks.gray(), 1.);
        invoke(&mut s, CommandId::ResetMaskColors);
        assert_eq!(s.selection_masks.gray(), 0.);
        assert_eq!(s.state.colors, artwork);
        invoke(&mut s, CommandId::ReturnToArtwork);
        assert_eq!(s.state.display_colors(), &artwork);
    }
    #[test]
    fn selection_brush_keeps_completed_contacts_ordered_and_defers_undo() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectionBrush);
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 100.);
        s.frame(1, 1).unwrap();
        send(&mut s, PenPhase::Down, 200.);
        send(&mut s, PenPhase::Up, 200.);
        assert_eq!(s.engine.backend().selection_updates.len(), 1);
        reply(&mut s, 0x80808080);
        s.frame(2, 2).unwrap();
        let first = s.engine.document().selection.clone().unwrap();
        assert_eq!(
            *s.engine.backend().selection_updates.last().unwrap().before,
            first
        );
        invoke(&mut s, CommandId::Undo); // Must wait for the second completed contact.
        assert_eq!(s.engine.document().selection, Some(first.clone()));
        reply(&mut s, 0xc0c0c0c0);
        s.frame(3, 3).unwrap();
        assert_eq!(s.engine.document().selection, Some(first));
        assert!(!s.painted_selections.busy());
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn quick_mask_entry_preserves_none_and_empty_without_history() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let colors = s.state.colors.clone();
        invoke(&mut s, CommandId::QuickMask);
        assert!(s.state.layer_tools.quick_mask);
        assert!(s.engine.document().selection.is_none());
        assert_eq!(s.current_selection(), Some(layer_core::Selection::full()));
        assert!(!s.engine.can_undo());
        invoke(&mut s, CommandId::QuickMask);
        assert!(!s.state.layer_tools.quick_mask);
        assert!(s.engine.document().selection.is_none());
        assert!(!s.engine.can_undo());
        assert_eq!(s.state.colors, colors);
        s.layer_edit(layer_core::Edit::SetSelection(Some(
            layer_core::Selection::empty(),
        )))
        .unwrap();
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(s.current_selection(), Some(layer_core::Selection::empty()));
        invoke(&mut s, CommandId::QuickMask);
        assert_eq!(
            s.engine.document().selection,
            Some(layer_core::Selection::empty())
        );
    }
    #[test]
    fn quick_mask_captures_before_exit_and_keeps_artwork_untouched() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        let layers = s.engine.document().layers.clone();
        invoke(&mut s, CommandId::QuickMask);
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 120.);
        s.frame(1, 1).unwrap();
        let update = s.engine.backend().selection_updates.last().unwrap();
        assert_eq!(update.mode, layer_render::SelectionPaintMode::Gray);
        assert_eq!(*update.before, layer_core::Selection::full());
        assert_eq!(update.gray, 0.);
        invoke(&mut s, CommandId::ReturnToArtwork);
        assert!(
            s.state.layer_tools.quick_mask,
            "exit waits for committed coverage"
        );
        reply(&mut s, 0x80808080);
        s.frame(2, 2).unwrap();
        assert!(!s.state.layer_tools.quick_mask);
        assert_eq!(s.engine.document().layers, layers);
        assert!(s.engine.document().selection.is_some());
        invoke(&mut s, CommandId::Undo);
        assert!(s.engine.document().selection.is_none());
    }
    #[test]
    fn saved_selection_edit_load_and_replace_are_independent() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectAll);
        let original = s.engine.document().selection.clone().unwrap();
        invoke(&mut s, CommandId::SaveSelectionLayer);
        let id = s
            .engine
            .document()
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Selection)
            .unwrap()
            .id;
        assert!(s.selection_masks.target().is_none());
        s.dispatch(UiAction::Selection {
            action: SelectionAction::EditLayer { id: id.0 },
        })
        .unwrap();
        invoke(&mut s, CommandId::ClearSelectionMask);
        assert_eq!(
            s.engine.document().saved_selection(id).unwrap(),
            layer_core::Selection::empty()
        );
        assert_eq!(s.engine.document().selection.as_ref(), Some(&original));
        s.dispatch(UiAction::Selection {
            action: SelectionAction::ReplaceLayer { id: id.0 },
        })
        .unwrap();
        assert_eq!(s.engine.document().saved_selection(id).unwrap(), original);
        invoke(&mut s, CommandId::ClearSelectionMask);
        s.dispatch(UiAction::Selection {
            action: SelectionAction::LoadLayer {
                id: id.0,
                mode: SelectionMode::New,
                inverted: false,
            },
        })
        .unwrap();
        assert!(s.selection_masks.target().is_none());
        assert_eq!(
            s.engine.document().selection,
            Some(layer_core::Selection::empty())
        );
        invoke(&mut s, CommandId::SelectAll);
        assert_eq!(
            s.engine.document().saved_selection(id).unwrap(),
            layer_core::Selection::empty()
        );
        invoke(&mut s, CommandId::Deselect);
        assert!(s.command(CommandId::Reselect).enabled);
        invoke(&mut s, CommandId::Reselect);
        assert_eq!(s.engine.document().selection, Some(original));
    }
    #[test]
    fn saved_selection_locked_destination_and_non_dry_brush_do_not_paint_artwork() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::NewSelectionLayer);
        let id = s.engine.document().active_layer;
        assert!(
            !s.state.layer_tools.controls.opacity
                && !s.state.layer_tools.controls.blend
                && !s.state.layer_tools.controls.mask
        );
        s.layer_action(LayerAction::Lock {
            id: id.0,
            value: true,
        })
        .unwrap();
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 120.);
        s.frame(1, 1).unwrap();
        assert!(s.engine.backend().selection_updates.is_empty());
        assert!(s.mask_brush_reason().unwrap().contains("locked"));
        s.layer_action(LayerAction::Lock {
            id: id.0,
            value: false,
        })
        .unwrap();
        let mut brush = s.engine.configured_brush().clone();
        brush.execution = layer_core::BrushExecution::Smudge;
        s.engine.set_brush(brush).unwrap();
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 120.);
        s.frame(2, 2).unwrap();
        assert!(s.engine.backend().selection_updates.is_empty());
        assert!(s.mask_brush_reason().unwrap().contains("dry brushes"));
    }
    #[test]
    fn selection_submission_stays_frozen_until_ack_while_new_contacts_accumulate() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectionBrush);
        s.engine.backend_mut().selection_wait = true;
        send(&mut s, PenPhase::Down, 100.);
        s.frame(1, 1).unwrap();
        let first = s.engine.backend().selection_updates[0].dabs.clone();
        send(&mut s, PenPhase::Move, 200.);
        send(&mut s, PenPhase::Up, 200.);
        s.frame(2, 2).unwrap();
        assert_eq!(s.engine.backend().selection_updates[1].dabs, first);
        assert!(!s.engine.backend().selection_updates[1].finish);
        s.engine.backend_mut().selection_wait = false;
        s.frame(3, 3).unwrap();
        s.frame(4, 4).unwrap();
        let last = s.engine.backend().selection_updates.last().unwrap();
        assert!(last.finish && !last.dabs.is_empty());
        assert_ne!(last.dabs, first);
    }
    #[test]
    fn unchanged_selection_contact_preserves_none_and_undo() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectionBrush);
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 100.);
        s.frame(1, 1).unwrap();
        reply(&mut s, 0);
        s.engine
            .backend_mut()
            .selection_reply
            .as_mut()
            .unwrap()
            .changed = false;
        s.frame(2, 2).unwrap();
        assert!(s.engine.document().selection.is_none());
        assert!(!s.engine.can_undo());
        assert!(!s.painted_selections.busy());
    }
    #[test]
    fn selection_brush_latches_alt_preserves_empty_and_keeps_its_settings() {
        let mut s = session();
        s.set_platform(Platform::Gtk);
        invoke(&mut s, CommandId::SelectionBrush);
        invoke(&mut s, CommandId::SelectionSubtract);
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Up, 100.);
        s.frame(1, 1).unwrap();
        assert!(s.engine.backend().selection_updates.is_empty());
        s.interaction.modifiers.alt = true;
        send(&mut s, PenPhase::Down, 100.);
        s.interaction.modifiers.alt = false;
        send(&mut s, PenPhase::Up, 100.);
        s.frame(2, 2).unwrap();
        assert_eq!(
            s.engine.backend().selection_updates[0].mode,
            layer_render::SelectionPaintMode::Add
        );
        reply(&mut s, 0);
        s.frame(3, 3).unwrap();
        assert!(
            s.engine.document().selection.is_some(),
            "empty remains a selection"
        );
        send(&mut s, PenPhase::Down, 100.);
        send(&mut s, PenPhase::Cancel, 100.);
        assert!(!s.painted_selections.busy());
        s.dispatch(UiAction::SetToolSetting {
            id: "selection_brush_size".into(),
            value: 75.,
        })
        .unwrap();
        let saved = s.capture_workspace().unwrap();
        let saved = serde_json::to_string(&saved).unwrap();
        let prepared = PreparedWorkspace::new(serde_json::from_str(&saved).unwrap()).unwrap();
        let mut restored = session();
        restored.set_platform(Platform::Gtk);
        restored.adopt_workspace(prepared).unwrap();
        assert_eq!(
            restored.selection_tools.options.brush,
            s.selection_tools.options.brush
        );
        assert_eq!(restored.state.tool_settings[0].value, 75.);
        assert_eq!(restored.state.tool_actions.len(), 3);
        assert!(restored.state.tool_actions.iter().all(|a| !matches!(
            a.command,
            CommandId::SelectionNew | CommandId::SelectionIntersect
        )));
    }
}
