mod move_pixels_checks {
    use super::*;
    use layer_core::{Affine, ImageTransform, Interpolation, LayerOperationKind, TransformMap};

    fn surface(s: &UiSession<Recorder>, p: [f32; 2]) -> [f32; 2] {
        let m = s.state.camera.document_to_surface();
        [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
    }
    fn send(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, p: [f32; 2]) {
        let [x, y] = surface(s, p);
        s.pen(PenEvent { surface_position: Point { x, y }, ..event(s, sequence, phase, 1.) }).unwrap();
    }
    fn drag(s: &mut UiSession<Recorder>, from: [f32; 2], to: [f32; 2]) {
        send(s, 1, PenPhase::Down, from);
        send(s, 2, PenPhase::Move, to);
        send(s, 3, PenPhase::Up, to);
        s.frame(4, 4).unwrap();
    }
    fn alt(s: &mut UiSession<Recorder>, pressed: bool) {
        s.input(UiInput::Key {
            key: "Alt".into(),
            pressed,
            repeat: false,
            modifiers: Modifiers { alt: pressed, ..Modifiers::default() },
            editing: false,
            divider: None,
        })
        .unwrap();
    }
    fn moved(offset: [f32; 2], keep_source: bool) -> ImageTransform {
        ImageTransform {
            map: TransformMap::Affine(Affine::translation(Point { x: offset[0], y: offset[1] })),
            interpolation: Interpolation::Nearest,
            keep_source,
        }
    }
    fn committed(s: &UiSession<Recorder>) -> Vec<ImageTransform> {
        s.engine
            .backend()
            .pending_operations
            .iter()
            .filter_map(|(_, op)| match &op.kind {
                LayerOperationKind::Transform(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn move_drags_selected_pixels_by_whole_pixels_and_the_selection_follows_in_one_step() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Move);
        let before = s.engine.document().clone();
        send(&mut s, 1, PenPhase::Down, [200., 200.]);
        send(&mut s, 2, PenPhase::Move, [230.4, 211.6]);
        s.frame(2, 2).unwrap();
        assert!(s.operation.moving_pixels() && !s.operation.active(), "a pixel move is part of the contact");
        assert_eq!(s.state.layer_tools.tool, LayerCanvasTool::Move, "Move stays the tool");
        let preview = s.renderer_mut().transform.clone().expect("the drag previews the moved pixels");
        assert!(preview.moving);
        assert_eq!(preview.transform, moved([30., 12.], false), "whole pixels, sampled exactly");
        let actions: Vec<_> = s.state.tool_actions.iter().map(|a| a.command).collect();
        assert_eq!(actions, [CommandId::MoveLeaveCopy], "Tool Options stay Move's");
        let mut overlay = Vec::new();
        s.append_layer_overlay(&mut overlay);
        assert!(overlay.iter().all(|segment| segment.marker != 2.), "no transform handles");
        send(&mut s, 3, PenPhase::Up, [230.4, 211.6]);
        s.frame(3, 3).unwrap();
        assert!(!s.operation.moving_pixels());
        assert!(s.renderer_mut().transform.is_none());
        assert_eq!(committed(&s), [moved([30., 12.], false)]);
        let selection = s.engine.document().selection.clone().unwrap();
        assert_eq!(selection.affine, Affine::translation(Point { x: 30., y: 12. }), "the selection moves with the pixels");
        let bar = s.state.canvas_bar.clone().expect("the selection bar returns");
        assert_eq!(bar.context.kind, CanvasBarKind::Selection);
        assert_eq!(bar.anchor.map(|a| [a[0], a[1]]), Some([129., 111.]), "beside the moved selection");
        invoke(&mut s, CommandId::Undo);
        s.frame(5, 5).unwrap();
        assert_eq!(s.engine.document().layers, before.layers, "one undo step");
        assert_eq!(s.engine.document().selection, before.selection);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().selection, Some(selection));
    }

    #[test]
    fn leave_copy_or_alt_at_the_press_keeps_the_original() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Move);
        let state = s.command(CommandId::MoveLeaveCopy);
        assert!(state.checkable && state.enabled && !state.selected);
        let bar = s.state.canvas_bar.clone().unwrap();
        assert!(bar_commands(&bar.items).contains(&CommandId::MoveLeaveCopy), "on the selection bar under Move");
        for (leave_copy, held, keep_source) in [(false, false, false), (true, false, true), (false, true, true), (true, true, false)] {
            if s.operation.leave_copy != leave_copy {
                invoke(&mut s, CommandId::MoveLeaveCopy);
            }
            s.frame(1, 1).unwrap();
            assert_eq!(s.command(CommandId::MoveLeaveCopy).selected, leave_copy);
            alt(&mut s, held);
            send(&mut s, 1, PenPhase::Down, [200., 200.]);
            alt(&mut s, !held);
            send(&mut s, 2, PenPhase::Move, [240., 190.]);
            s.frame(2, 2).unwrap();
            assert_eq!(s.renderer_mut().transform.clone().unwrap().transform, moved([40., -10.], keep_source), "latched at the press");
            send(&mut s, 3, PenPhase::Up, [240., 190.]);
            alt(&mut s, false);
            s.frame(3, 3).unwrap();
            assert_eq!(committed(&s), [moved([40., -10.], keep_source)], "{leave_copy} {held}");
            invoke(&mut s, CommandId::Undo);
        }
        invoke(&mut s, CommandId::Lasso);
        s.frame(6, 6).unwrap();
        assert!(!s.state.tool_actions.iter().any(|a| a.command == CommandId::MoveLeaveCopy));
        let bar = s.state.canvas_bar.clone().unwrap();
        assert!(!bar_commands(&bar.items).contains(&CommandId::MoveLeaveCopy), "only while Move is active");
    }

    #[test]
    fn move_without_a_selection_keeps_the_lossless_layer_offset() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Deselect);
        invoke(&mut s, CommandId::Move);
        send(&mut s, 1, PenPhase::Down, [200., 200.]);
        send(&mut s, 2, PenPhase::Move, [230.5, 211.25]);
        assert!(!s.operation.moving_pixels() && s.renderer_mut().transform.is_none());
        send(&mut s, 3, PenPhase::Up, [230.5, 211.25]);
        s.frame(2, 2).unwrap();
        let offset = s.engine.document().layer(LayerId(1)).unwrap().properties.offset;
        assert!((offset.x - 30.5).abs() < 1e-3 && (offset.y - 11.25).abs() < 1e-3, "{offset:?}");
        assert!(committed(&s).is_empty(), "no pixels are resampled");
    }

    #[test]
    fn moving_selected_pixels_is_refused_with_a_notice_where_nothing_can_move() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Move);
        let layer = |s: &mut UiSession<Recorder>, action| s.dispatch(UiAction::Layer { action }).unwrap();
        let refused = |s: &mut UiSession<Recorder>, text: &str| {
            let revision = s.engine.document().revision;
            drag(s, [200., 200.], [260., 240.]);
            assert_eq!(notice_text(s), Some(text));
            assert!(!s.operation.moving_pixels() && s.renderer_mut().transform.is_none());
            assert_eq!(s.engine.document().revision, revision, "{text}");
        };
        layer(&mut s, LayerAction::Lock { id: 1, value: true });
        refused(&mut s, "The active layer is locked");
        layer(&mut s, LayerAction::Lock { id: 1, value: false });
        s.dispatch(UiAction::SelectLayer { id: 2 }).unwrap();
        refused(&mut s, "The paper can't be moved");
        layer(&mut s, LayerAction::New { group: true, clipped: false });
        refused(&mut s, "Choose a paint layer or a mask to move selected pixels");
        layer(&mut s, LayerAction::New { group: false, clipped: false });
        refused(&mut s, "This layer has no pixels to move");
    }

    #[test]
    fn a_finger_on_the_selection_moves_it_and_elsewhere_navigates() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Move);
        let touch = |id, phase, position| pointer_input(id, phase, PointerKind::Touch, PointerButton::Primary, position, 0);
        let [inside, outside] = [surface(&s, [200., 200.]), surface(&s, [600., 600.])];
        assert!(s.input(touch(1, ContactPhase::Down, inside)).unwrap().paint, "a finger on the selection drags it");
        s.input(touch(1, ContactPhase::Up, inside)).unwrap();
        assert!(!s.input(touch(2, ContactPhase::Down, outside)).unwrap().paint, "elsewhere a finger navigates");
        s.input(touch(2, ContactPhase::Up, outside)).unwrap();
        invoke(&mut s, CommandId::InvertSelection);
        assert!(!s.input(touch(3, ContactPhase::Down, inside)).unwrap().paint, "an inverted selection leaves its hole");
        s.input(touch(3, ContactPhase::Up, inside)).unwrap();
        assert!(s.input(touch(4, ContactPhase::Down, outside)).unwrap().paint);
        s.input(touch(4, ContactPhase::Up, outside)).unwrap();
        invoke(&mut s, CommandId::Lasso);
        assert!(!s.input(touch(5, ContactPhase::Down, outside)).unwrap().paint, "other tools navigate");
    }

    #[test]
    fn a_cancelled_or_still_press_leaves_no_history() {
        let mut s = filled_selection_session();
        invoke(&mut s, CommandId::Move);
        let before = s.engine.document().clone();
        send(&mut s, 1, PenPhase::Down, [200., 200.]);
        send(&mut s, 2, PenPhase::Move, [260., 240.]);
        assert!(s.input(UiInput::Blur).unwrap().cancel_paint);
        s.frame(2, 2).unwrap();
        assert!(!s.operation.moving_pixels() && s.renderer_mut().transform.is_none());
        assert_eq!(s.engine.document().revision, before.revision);
        drag(&mut s, [200., 200.], [200.3, 199.8]);
        assert!(committed(&s).is_empty());
        assert_eq!(s.engine.document().revision, before.revision, "a press that moves no whole pixel changes nothing");
    }

    #[test]
    fn the_renderer_prepares_the_next_move_of_the_selected_pixels_while_idle() {
        let mut s = filled_selection_session();
        let hint = |s: &mut UiSession<Recorder>| s.renderer_mut().moving_pixels.clone();
        assert_eq!(hint(&mut s), None, "only under Move");
        invoke(&mut s, CommandId::Move);
        s.frame(2, 2).unwrap();
        let selection = s.engine.document().selection.clone().unwrap();
        assert_eq!(hint(&mut s), Some((LayerId(1), selection)));
        drag(&mut s, [200., 200.], [230., 190.]);
        let moved = s.engine.document().selection.clone().unwrap();
        s.frame(5, 5).unwrap();
        assert_eq!(hint(&mut s), Some((LayerId(1), moved)), "the moved selection is prepared next");
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: true } }).unwrap();
        s.frame(6, 6).unwrap();
        assert_eq!(hint(&mut s), None, "nothing to prepare where Move refuses");
        s.dispatch(UiAction::Layer { action: LayerAction::Lock { id: 1, value: false } }).unwrap();
        invoke(&mut s, CommandId::Lasso);
        s.frame(7, 7).unwrap();
        assert_eq!(hint(&mut s), None);
    }

    #[test]
    fn a_linked_mask_moves_with_the_selected_pixels() {
        let mut s = filled_selection_session();
        let selection = s.engine.document().selection.clone();
        s.dispatch(UiAction::Layer { action: LayerAction::AddMask { id: 1, replace: false } }).unwrap();
        s.dispatch(UiAction::Layer { action: LayerAction::Select { id: 1, mask: false } }).unwrap();
        s.layer_edit(layer_core::Edit::SetSelection(selection)).unwrap();
        invoke(&mut s, CommandId::Move);
        drag(&mut s, [200., 200.], [216., 190.]);
        let transforms = committed(&s);
        assert_eq!(transforms.len(), 2, "the paint and its linked mask");
        assert!(transforms.iter().all(|t| !t.keep_source && t.interpolation == Interpolation::Nearest));
    }
}
