use super::*;
use crate::session::test_support::{session, invoke, event, Recorder};

#[derive(Debug)]
struct Field;
impl layer_core::liquify::Field for Field {
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn resources(&self, visit: &mut dyn FnMut(u64, usize)) { visit(987, 4096); }
    fn tiles(&self) -> Vec<[u32; 2]> { vec![[0, 0]] }
}

fn deformed_with_alpha_lock(alpha_locked: bool) -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    invoke(&mut s, CommandId::Liquify);
    if alpha_locked { set_alpha_locked(&mut s, true); }
    let target = s.engine.document().working.target.unwrap();
    let SourceTarget::Paint(handle) = target else { panic!() };
    let source = s.engine.document().scene().paint(handle).unwrap().clone();
    let revision = RasterRevision::backed(Default::default());
    revision.set_liquify(Arc::new(layer_core::liquify::State::new(
        Context::for_source(&source, alpha_locked).original, Arc::new(Field), &source, alpha_locked)));
    s.engine.apply_edit(Edit::SetRaster { target, revision }).unwrap();
    s.refresh_tools();
    s
}

fn deformed() -> UiSession<Recorder> { deformed_with_alpha_lock(false) }

fn set_alpha_locked(s: &mut UiSession<Recorder>, locked: bool) {
    let target = s.engine.document().working.target.unwrap();
    let owner = s.engine.document().scene().source_owner(target).unwrap();
    let mut occurrence = s.engine.document().scene().occurrence(owner).unwrap().clone();
    occurrence.alpha_locked = locked;
    let change = RecordChange::replace(&s.engine.document().artwork.occurrences, owner, Some(occurrence)).unwrap();
    s.engine.apply_edit(Edit::Occurrence(change)).unwrap();
}

#[test]
fn liquify_adjust_unchanged_amount_keeps_history_and_pending_adjustment_stays_open() {
    let mut s = deformed();
    let checkpoint = s.engine.checkpoint();
    assert!(s.state.tool_actions.iter().any(|action| action.command == CommandId::LiquifyAdjust));
    assert!(!s.state.tool_settings.iter().any(|setting| setting.id == "liquify_amount"));
    invoke(&mut s, CommandId::LiquifyAdjust);
    assert_eq!(s.state.tool_settings.iter().find(|setting| setting.id == "liquify_amount").unwrap().value, 1.);
    s.adjust_liquify(1.).unwrap();
    assert_eq!(s.engine.checkpoint(), checkpoint);
    s.adjust_liquify(0.5).unwrap();
    assert!(s.operation.liquify.is_some());
    let checkpoint = s.engine.checkpoint();
    s.adjust_liquify(0.5).unwrap();
    assert_eq!(s.engine.checkpoint(), checkpoint);
    invoke(&mut s, CommandId::LiquifyAdjust);
    assert_eq!(s.deferred_edits.len(), 1);
    assert!(s.operation.liquify.is_some());
    for frame in 1..=3 {
        s.frame(frame, frame).unwrap();
        if s.deferred_edits.is_empty() { break; }
    }
    assert!(s.deferred_edits.is_empty());
    assert!(s.operation.liquify.is_none());
    assert!(!s.state.tool_settings.iter().any(|setting| setting.id == "liquify_amount"));
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn liquify_adjust_zero_keeps_controls_until_reset_and_undo_restores_deformation() {
    let mut s = deformed();
    let before = s.engine.document().clone();
    s.liquify_action(false).unwrap();
    s.adjust_liquify(0.).unwrap();
    assert!(s.operation.liquify.is_some());
    s.engine.render_frame().unwrap();
    s.refresh_tools();
    assert!(s.operation.liquify.is_some());
    s.engine.undo().unwrap();
    assert_eq!(s.engine.document().artwork, before.artwork);
    s.refresh_tools();
    assert!(s.operation.liquify.is_none());
}

#[test]
fn liquify_reset_is_one_undoable_edit_and_requires_a_session() {
    let mut s = deformed();
    let before = s.engine.document().clone();
    s.liquify_action(true).unwrap();
    assert!(s.engine.liquify_context().is_none());
    assert!(s.liquify_action(true).is_err());
    s.engine.undo().unwrap();
    assert_eq!(s.engine.document().artwork, before.artwork);
    assert!(s.engine.liquify_context().is_some());
}

#[test]
fn liquify_adjust_repeated_slider_changes_are_one_undo_step() {
    let mut s = deformed();
    let original = s.engine.document().artwork.clone();
    s.liquify_action(false).unwrap();
    for amount in [0.8, 0.3, 0.6] {
        s.dispatch(UiAction::SetToolSetting { id: "liquify_amount".into(), value: amount }).unwrap();
    }
    s.engine.render_frame().unwrap();
    assert_eq!(s.engine.backend().liquify_amount_batches, vec![0.6]);
    let adjusted = s.engine.document().artwork.clone();
    assert_ne!(adjusted, original);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, original);
    assert!(s.engine.redo().unwrap());
    assert_eq!(s.engine.document().artwork, adjusted);
}

#[test]
fn liquify_reset_immediately_after_amount_drains_it_and_undo_restores_adjustment() {
    let mut s=deformed();
    let target=s.engine.document().working.target.unwrap();
    invoke(&mut s,CommandId::LiquifyAdjust);
    s.dispatch(UiAction::SetToolSetting{id:"liquify_amount".into(),value:0.5}).unwrap();
    let adjusted=s.engine.document().target_raster(target).unwrap().clone();
    assert!(adjusted.try_data().is_none());
    let pending=s.engine.raster_backing_pending();
    let reset=s.dispatch(UiAction::Invoke{command:CommandId::LiquifyReset});
    assert!(reset.is_ok(),"Reset while Amount is pending: {:?}, backing_pending={pending}",reset.err());
    assert!(pending,"Amount needs its raster backing before a later edit");
    assert_eq!(s.deferred_edits.len(),1,"Reset waits for the Amount raster");
    for frame in 1..=3 {
        s.frame(frame,frame).unwrap();
        if s.deferred_edits.is_empty() { break; }
    }
    assert!(s.deferred_edits.is_empty());
    assert!(adjusted.try_data().is_some(),"Reset submits Amount before replacing its raster");
    assert_ne!(s.engine.document().target_raster(target).unwrap().identity(),adjusted.identity());
    s.engine.render_frame().unwrap();
    invoke(&mut s,CommandId::Undo);
    s.engine.render_frame().unwrap();
    let restored=s.engine.document().target_raster(target).unwrap();
    assert_eq!(restored.identity(),adjusted.identity());
    assert!(restored.liquify().is_some(),"Undo restores the adjusted deformation");
}

#[test]
fn liquify_slider_amends_latest_amount_while_previous_submission_is_pending() {
    let mut s=deformed();
    let original=s.engine.document().artwork.clone();
    invoke(&mut s,CommandId::LiquifyAdjust);
    s.dispatch(UiAction::SetToolSetting{id:"liquify_amount".into(),value:0.8}).unwrap();
    s.engine.render_frame().unwrap();
    s.renderer_mut().settling=true;
    let next=s.dispatch(UiAction::SetToolSetting{id:"liquify_amount".into(),value:0.6});
    assert!(next.is_ok(),"a later slider value must supersede pending capture: {:?}",next.err());
    s.renderer_mut().settling=false;
    s.engine.render_frame().unwrap();
    assert_eq!(s.engine.backend().liquify_amount_batches,vec![0.6]);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork,original);
}

#[test]
fn liquify_adjust_after_intervening_edit_has_its_own_undo_step() {
    let mut s = deformed();
    s.liquify_action(false).unwrap();
    s.adjust_liquify(0.8).unwrap();
    let target = s.engine.document().working.target.unwrap();
    let owner = s.engine.document().scene().source_owner(target).unwrap();
    let mut occurrence = s.engine.document().scene().occurrence(owner).unwrap().clone();
    occurrence.opacity = 0.5;
    let change = RecordChange::replace(&s.engine.document().artwork.occurrences, owner, Some(occurrence)).unwrap();
    s.engine.apply_edit(Edit::Occurrence(change)).unwrap();
    let intervening = s.engine.document().artwork.clone();
    s.adjust_liquify(0.4).unwrap();
    assert_ne!(s.engine.document().artwork, intervening);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, intervening);
}

#[test]
fn liquify_adjust_rejects_alpha_lock_transitions() {
    for initial in [false, true] {
        let mut s = deformed_with_alpha_lock(initial);
        s.liquify_action(false).unwrap();
        set_alpha_locked(&mut s, !initial);
        let before = s.engine.document().artwork.clone();
        let checkpoint = s.engine.checkpoint();
        assert_eq!(s.adjust_liquify(0.5), Err("Liquify target changed".into()));
        assert_eq!(s.engine.document().artwork, before);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        s.refresh_tools();
        assert!(s.operation.liquify.is_none());
    }
}

#[test]
fn liquify_adjust_rejects_stale_expected_raster_revision() {
    let mut s = deformed();
    s.liquify_action(false).unwrap();
    let target = s.engine.document().working.target.unwrap();
    s.engine.apply_edit(Edit::SetRaster { target, revision: RasterRevision::pending() }).unwrap();
    let before = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    assert_eq!(s.adjust_liquify(0.5), Err("Liquify target changed".into()));
    assert_eq!(s.engine.document().artwork, before);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    s.refresh_tools();
    assert!(s.operation.liquify.is_none());
}

#[test]
fn liquify_busy_command_reports_canvas_interaction_with_live_session() {
    let mut s = deformed();
    let target = s.engine.document().working.target.unwrap();
    let revision = s.engine.document().target_raster(target).unwrap().identity();
    assert!(s.engine.liquify_context().is_some());
    assert!(s.command_flags(CommandId::LiquifyAdjust).0);
    let hover = event(&s, 1, PenPhase::Hover, 0.);
    s.cursor_input(Some(hover));
    assert!(s.command_flags(CommandId::LiquifyAdjust).0);

    s.pen(event(&s, 2, PenPhase::Down, 1.));
    assert!(s.engine.liquify_context().is_some());
    assert!(!s.command_flags(CommandId::LiquifyAdjust).0);
    assert_eq!(
        s.dispatch(UiAction::Invoke { command: CommandId::LiquifyAdjust }).unwrap_err(),
        "Finish the canvas interaction first",
    );
    assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), revision);
    assert!(s.operation.liquify.is_none());

    s.pen(event(&s, 3, PenPhase::Cancel, 0.));
    s.frame(40_000_000, 40_000_000).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::LiquifyAdjust }).unwrap();
    assert!(s.operation.liquify.is_some());
}

#[test]
fn liquify_amount_zero_keeps_adjust_and_reset_available() {
    let mut s = deformed();
    s.dispatch(UiAction::Invoke { command: CommandId::LiquifyAdjust }).unwrap();
    for (frame, amount) in [(1, 0.), (2, 0.5), (3, 1.)] {
        s.dispatch(UiAction::SetToolSetting { id: "liquify_amount".into(), value: amount }).unwrap();
        s.frame(frame, frame).unwrap();
        assert!(s.operation.liquify.is_some());
        assert!(s.command_flags(CommandId::LiquifyAdjust).0);
        assert!(s.command_flags(CommandId::LiquifyReset).0);
        if amount > 0. {
            assert!(s.engine.liquify_context().is_some());
        }
    }
}

#[test]
fn liquify_reset_at_zero_restores_original_in_one_undo_step() {
    let mut s = deformed();
    let target = s.engine.document().working.target.unwrap();
    let original = s.engine.liquify_context().unwrap().1.original.clone();
    s.dispatch(UiAction::Invoke { command: CommandId::LiquifyAdjust }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "liquify_amount".into(), value: 0. }).unwrap();
    s.frame(1, 1).unwrap();
    let zero = s.engine.document().target_raster(target).unwrap().identity();
    assert!(s.engine.liquify_context().is_none());
    assert!(s.command_flags(CommandId::LiquifyReset).0);

    s.dispatch(UiAction::Invoke { command: CommandId::LiquifyReset }).unwrap();
    let SourceTarget::Paint(handle) = target else { panic!() };
    assert_eq!(s.engine.document().scene().paint(handle).unwrap(), original.as_ref());
    assert!(s.operation.liquify.is_none());
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), zero);
}

#[test]
fn liquify_reset_rejects_a_stale_zero_amount_adjustment() {
    let mut s = deformed();
    let target = s.engine.document().working.target.unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::LiquifyAdjust }).unwrap();
    s.dispatch(UiAction::SetToolSetting { id: "liquify_amount".into(), value: 0. }).unwrap();
    s.frame(1, 1).unwrap();
    s.engine.apply_edit(Edit::SetRaster { target, revision: RasterRevision::backed(Default::default()) }).unwrap();
    let before = s.engine.document().artwork.clone();
    assert!(s.liquify_action(true).is_err());
    assert_eq!(s.engine.document().artwork, before);
    s.refresh_tools();
    assert!(s.operation.liquify.is_none());
    assert!(!s.command_flags(CommandId::LiquifyReset).0);
}

fn amount_contact(s: &mut UiSession<Recorder>, phase: ContactPhase, value: Option<f32>) {
    s.dispatch(UiAction::ToolSettingContact { id: "liquify_amount".into(), phase, value }).unwrap();
}

fn published_revisions(s: &UiSession<Recorder>, target: SourceTarget) -> (u64, u64) {
    let owner = s.engine.document().scene().source_owner(target).unwrap();
    let paint = s.state.layers.iter().find(|layer| layer.id == occurrence_token(owner)).unwrap().paint_revision;
    (s.state.document_file.revision, paint)
}

#[test]
fn liquify_amount_motion_previews_across_frames_without_editing_artwork() {
    let mut s = deformed();
    s.refresh_document();
    invoke(&mut s, CommandId::LiquifyAdjust);
    assert!(s.state.tool_settings.iter().find(|setting| setting.id == "liquify_amount").unwrap().preview);
    let target = s.engine.document().working.target.unwrap();
    let revision = s.engine.document().target_raster(target).unwrap().identity();
    let artwork = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    let published = published_revisions(&s, target);
    assert_eq!(published.0, s.engine.document().revision);

    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    for frame in 1..=2 {
        s.frame(frame, frame).unwrap();
        assert_eq!(s.engine.backend().liquify_amount_batches, vec![0.4]);
        assert_eq!(s.engine.document().artwork, artwork);
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), revision);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(published_revisions(&s, target), published);
    }
    amount_contact(&mut s, ContactPhase::Up, None);
    let committed = s.engine.document().target_raster(target).unwrap().identity();
    assert_ne!(committed, revision);
    let after = published_revisions(&s, target);
    assert_eq!(after.0, s.engine.document().revision);
    assert_ne!(after.1, published.1);
    assert_ne!(s.engine.checkpoint(), checkpoint);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, artwork);
}

#[test]
fn liquify_discrete_amount_publishes_document_and_paint_revisions() {
    let mut s = deformed();
    s.refresh_document();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let target = s.engine.document().working.target.unwrap();
    let before = published_revisions(&s, target);

    s.dispatch(UiAction::SetToolSetting { id: "liquify_amount".into(), value: 0.5 }).unwrap();
    assert_ne!(published_revisions(&s, target), before);
    let after = published_revisions(&s, target);
    assert_eq!(after.0, s.engine.document().revision);
    assert_ne!(after.1, before.1);
}

#[test]
fn liquify_amount_cancel_and_return_to_start_leave_no_edit() {
    for finish in [ContactPhase::Cancel, ContactPhase::Up] {
        let mut s = deformed();
        invoke(&mut s, CommandId::LiquifyAdjust);
        let artwork = s.engine.document().artwork.clone();
        let checkpoint = s.engine.checkpoint();
        amount_contact(&mut s, ContactPhase::Down, None);
        amount_contact(&mut s, ContactPhase::Move, Some(0.3));
        s.frame(1, 1).unwrap();
        if finish == ContactPhase::Up { amount_contact(&mut s, ContactPhase::Move, Some(1.)); }
        amount_contact(&mut s, finish, None);
        s.frame(2, 2).unwrap();
        assert_eq!(s.engine.document().artwork, artwork);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.operation.liquify.is_some());
        assert_eq!(s.operation.liquify.as_ref().unwrap().amount, 1.);
    }
}

#[test]
fn liquify_amount_successive_contacts_share_one_undo_step() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let original = s.engine.document().artwork.clone();
    for (frame, amount) in [(1, 0.7), (2, 0.25), (3, 0.9)] {
        amount_contact(&mut s, ContactPhase::Down, None);
        amount_contact(&mut s, ContactPhase::Move, Some(amount));
        amount_contact(&mut s, ContactPhase::Up, None);
        s.frame(frame, frame).unwrap();
    }
    let final_artwork = s.engine.document().artwork.clone();
    assert_ne!(final_artwork, original);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, original);
    assert!(s.engine.redo().unwrap());
    assert_eq!(s.engine.document().artwork, final_artwork);
}

#[test]
fn liquify_amount_contact_finishes_before_tool_change() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let original = s.engine.document().artwork.clone();
    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    s.dispatch(UiAction::Invoke { command: CommandId::Brush }).unwrap();
    assert_ne!(s.engine.document().artwork, original);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, original);
}

#[test]
fn liquify_stale_contact_events_do_not_edit_without_a_down() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let artwork = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    for phase in [ContactPhase::Move, ContactPhase::Up] {
        s.dispatch(UiAction::ToolSettingContact { id: "liquify_amount".into(), phase, value: Some(0.4) }).unwrap();
    }
    assert_eq!(s.engine.document().artwork, artwork);
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn liquify_preview_does_not_survive_renderer_replacement() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    s.frame(1, 1).unwrap();
    assert_eq!(s.engine.backend().liquify_amount_batches, vec![0.4]);

    s.replace_renderer(Recorder::default()).unwrap();
    assert!(s.operation.liquify.is_none());
    s.frame(2, 2).unwrap();
    assert!(s.engine.backend().liquify_amount_batches.is_empty());
}

#[test]
fn liquify_contact_rejects_changed_target_or_alpha_lock() {
    for alpha_lock in [false, true] {
        let mut s = deformed();
        invoke(&mut s, CommandId::LiquifyAdjust);
        amount_contact(&mut s, ContactPhase::Down, None);
        amount_contact(&mut s, ContactPhase::Move, Some(0.4));
        if alpha_lock {
            set_alpha_locked(&mut s, true);
        } else {
            let target = s.engine.document().working.target.unwrap();
            s.engine.apply_edit(Edit::SetRaster { target, revision: RasterRevision::backed(Default::default()) }).unwrap();
        }
        let target = s.engine.document().working.target.unwrap();
        let raster = s.engine.document().target_raster(target).unwrap().identity();
        let checkpoint = s.engine.checkpoint();
        let _ = s.dispatch(UiAction::ToolSettingContact { id: "liquify_amount".into(), phase: ContactPhase::Up, value: None });
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), raster);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        s.refresh_tools();
        assert!(s.operation.liquify.is_none());
    }
}

#[test]
fn liquify_amount_contact_commits_before_a_new_pen_down() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let target = s.engine.document().working.target.unwrap();
    let original = s.engine.document().target_raster(target).unwrap().identity();
    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    let down = event(&s, 2, PenPhase::Down, 0.5);
    s.pen(down);
    assert!(!s.liquify_preview_active());
    assert_ne!(s.engine.document().target_raster(target).unwrap().identity(), original);
}

#[test]
fn liquify_cancel_drops_a_preview_held_in_a_pending_frame() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let artwork = s.engine.document().artwork.clone();
    let checkpoint = s.engine.checkpoint();
    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    s.renderer_mut().raster_dependencies_blocked = true;
    s.frame(1, 1).unwrap();
    assert!(s.engine.backend().liquify_amount_batches.is_empty());

    amount_contact(&mut s, ContactPhase::Cancel, None);
    s.renderer_mut().raster_dependencies_blocked = false;
    s.frame(2, 2).unwrap();
    assert!(s.engine.backend().liquify_amount_batches.is_empty());
    assert_eq!(s.engine.document().artwork, artwork);
    assert_eq!(s.engine.checkpoint(), checkpoint);
}

#[test]
fn liquify_up_replaces_a_preview_held_in_a_pending_frame() {
    let mut s = deformed();
    invoke(&mut s, CommandId::LiquifyAdjust);
    let artwork = s.engine.document().artwork.clone();
    amount_contact(&mut s, ContactPhase::Down, None);
    amount_contact(&mut s, ContactPhase::Move, Some(0.4));
    s.renderer_mut().raster_dependencies_blocked = true;
    s.frame(1, 1).unwrap();
    assert!(s.engine.backend().liquify_amount_batches.is_empty());

    amount_contact(&mut s, ContactPhase::Up, None);
    assert_ne!(s.engine.document().artwork, artwork);
    s.renderer_mut().raster_dependencies_blocked = false;
    s.frame(2, 2).unwrap();
    assert_eq!(s.engine.backend().liquify_amount_batches, vec![0.4]);
    assert!(s.engine.undo().unwrap());
    assert_eq!(s.engine.document().artwork, artwork);
}
