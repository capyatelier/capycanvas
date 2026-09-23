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
