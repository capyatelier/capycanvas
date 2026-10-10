use super::{test_support::*, *};
use layer_core::PressureResponse;

fn source(s: &UiSession<Recorder>) -> PressureResponse { s.engine.configured_pressure_curve().clone().into() }

fn edit(s: &mut UiSession<Recorder>, action: CurveEditorAction) {
    s.dispatch(UiAction::CurveEditor { target: CurveEditorTarget::Pressure, action }).unwrap();
}
fn calibration(s: &mut UiSession<Recorder>, action: PressureCalibrationAction) {
    s.dispatch(UiAction::PressureCalibration { action }).unwrap();
}

#[test]
fn live_pressure_preview_is_independent_of_artwork_settings_and_workspace_history() {
    let mut s = session(Platform::Gtk);
    let layout = serde_json::to_value(&s.state.workspace).unwrap();
    let original = s.state.settings.pressure_curve.clone();
    let revision = s.engine.document().revision;
    s.dispatch(UiAction::Invoke { command: CommandId::PenPressure }).unwrap();
    assert!(!s.state.settings_open);
    let view=&s.state.pressure_calibration.as_ref().unwrap().editor;
    assert_eq!(view.points,&[[0.,0.125],[0.25,1.],[1.,1.]]);
    assert!(!view.controls.coordinate_readouts);
    assert!(view.controls.input.is_none() && view.controls.output.is_none());
    calibration(&mut s, PressureCalibrationAction::Sensitivity { lighter: true });
    let draft = source(&s);
    assert!((draft.points()[0][1] - 0.15).abs() < 1e-6);
    assert_eq!(&draft.points()[1..], &original.points()[1..]);
    assert_eq!(s.state.settings.pressure_curve, original);
    assert_eq!(s.engine.document().revision, revision);
    let mut settings = s.state.settings.clone(); settings.pan_speed = 2.;
    s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
    assert_eq!(source(&s), draft);
    s.pen(event(&s, 1, PenPhase::Down, 0.2)).unwrap();
    s.frame(10_000_000,18_000_000).unwrap();
    calibration(&mut s, PressureCalibrationAction::Cancel);
    s.pen(event(&s, 2, PenPhase::Move, 0.2)).unwrap();
    s.pen(event(&s, 3, PenPhase::Up, 0.2)).unwrap();
    s.frame(30_000_000,38_000_000).unwrap();
    assert!(s.state.pressure_calibration.is_none());
    assert_eq!(s.state.settings.pressure_curve, original);
    assert_eq!(s.state.settings.pan_speed, 2.);
    assert!(s.engine.can_undo());
    assert_eq!(serde_json::to_value(&s.state.workspace).unwrap(), layout);
    assert!(s.renderer_mut().recorded_dabs.len() > 1);
}

#[test]
fn curve_drag_cancel_apply_reset_and_stale_events_are_transactional() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::PenPressure }).unwrap();
    let original = source(&s);
    let epoch = s.state.pressure_calibration.as_ref().unwrap().editor.controls.epoch;
    for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Cancel] {
        let point = if phase == ContactPhase::Down { [25.,0.] } else { [35.,20.] };
        edit(&mut s, CurveEditorAction::Contact {epoch,phase,point,extent:[100.,100.]});
    }
    assert_eq!(source(&s), original);
    calibration(&mut s, PressureCalibrationAction::Sensitivity { lighter: false });
    let chosen = source(&s);
    calibration(&mut s, PressureCalibrationAction::Apply);
    assert_eq!(s.state.settings.pressure_curve, chosen);
    assert!(s.state.requests.iter().any(|r|matches!(&r.kind,HostRequestKind::SaveSettings{settings} if settings.pressure_curve==chosen)));
    s.dispatch(UiAction::Invoke { command: CommandId::PenPressure }).unwrap();
    edit(&mut s, CurveEditorAction::Contact {epoch,phase:ContactPhase::Down,point:[0.,80.],extent:[100.,100.]});
    assert_eq!(source(&s),chosen);
    edit(&mut s, CurveEditorAction::Reset);
    assert_eq!(source(&s),PressureResponse::default());
    calibration(&mut s, PressureCalibrationAction::Cancel);
    assert_eq!(s.state.settings.pressure_curve,chosen);
}

#[test]
fn utility_placement_cancel_and_resize_leave_workspace_untouched() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::Invoke { command: CommandId::PenPressure }).unwrap();
    calibration(&mut s, PressureCalibrationAction::Measure{extent:[336.,420.],viewport:[1000.,760.]});
    let bounds=s.state.pressure_calibration.as_ref().unwrap().bounds;
    let plot=s.state.pressure_calibration.as_ref().unwrap().editor.plot.as_ptr();
    let points=s.state.pressure_calibration.as_ref().unwrap().editor.points.as_ptr();
    for (phase,position) in [(ContactPhase::Down,[700.,100.]),(ContactPhase::Move,[-100.,-100.]),(ContactPhase::Cancel,[0.,0.])] {
        calibration(&mut s,PressureCalibrationAction::Drag{phase,position,viewport:[1000.,760.]});
    }
    assert_eq!(s.state.pressure_calibration.as_ref().unwrap().bounds,bounds);
    calibration(&mut s,PressureCalibrationAction::Measure{extent:[336.,420.],viewport:[320.,360.]});
    let fit=s.state.pressure_calibration.as_ref().unwrap().bounds;
    assert_eq!([fit.x,fit.y,fit.width,fit.height],[0.,0.,320.,360.]);
    assert_eq!(s.state.pressure_calibration.as_ref().unwrap().editor.plot.as_ptr(),plot);
    assert_eq!(s.state.pressure_calibration.as_ref().unwrap().editor.points.as_ptr(),points);
}

#[test]
fn preferences_launch_closes_settings_on_every_platform() {
    for platform in Platform::ALL {
        let mut s=session(platform);
        s.dispatch(UiAction::OpenSettings {page:SettingsPage::Input}).unwrap();
        let view=s.preferences().unwrap();
        let groups=&view.pages.iter().find(|p|p.id==SettingsPage::Input).unwrap().groups;
        let rows=&groups.iter().find(|g|g.rows.iter().any(|r|r.id==PreferenceId::Feedback)).unwrap().rows;
        assert_eq!(rows[0].id,PreferenceId::PenPressure);assert_eq!(rows[1].id,PreferenceId::Feedback);
        assert!(rows[0].reset.is_none());
        let PreferenceKind::Action {action,..}=&rows[0].kind else {panic!("pressure opens a separate editor")};
        let action=action.clone();s.dispatch(*action).unwrap();
        assert!(!s.state.settings_open);assert!(s.state.pressure_calibration.is_some());
        let menu=s.application_menu(ApplicationMenu::Edit);
        assert!(!menu.sections.iter().flatten().any(|item|matches!(item.action.as_ref(),Some(UiAction::Invoke{command:CommandId::PenPressure}))));
    }
}

#[test]
fn dragging_outside_removes_interior_points_before_release_and_cancel_restores() {
    for platform in Platform::ALL {
        let mut s=session(platform);s.dispatch(UiAction::Invoke{command:CommandId::PenPressure}).unwrap();
        let original=source(&s);let epoch=s.state.pressure_calibration.as_ref().unwrap().editor.controls.epoch;
        let contact=|phase,point|CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]};
        for outside in [[25.,-25.],[25.,125.],[-25.,0.],[125.,0.]] {
            edit(&mut s,contact(ContactPhase::Down,[25.,0.]));edit(&mut s,contact(ContactPhase::Move,[25.,-24.]));
            assert_eq!(source(&s).points().len(),3);
            edit(&mut s,contact(ContactPhase::Move,outside));
            assert_eq!(source(&s).points(),&[[0.,0.125],[1.,1.]]);
            let view=&s.state.pressure_calibration.as_ref().unwrap().editor;
            assert_eq!(view.points,source(&s).points());assert!(view.controls.selected.is_none());
            assert_eq!(s.state.settings.pressure_curve,original);
            edit(&mut s,contact(ContactPhase::Cancel,outside));assert_eq!(source(&s),original);
        }
        edit(&mut s,contact(ContactPhase::Down,[10.,50.]));edit(&mut s,contact(ContactPhase::Up,[10.,50.]));
        edit(&mut s,contact(ContactPhase::Down,[10.,50.]));edit(&mut s,contact(ContactPhase::Move,[10.,-40.]));
        assert_eq!(source(&s),original);
        for phase in [ContactPhase::Move,ContactPhase::Up] {
            edit(&mut s,contact(phase,[40.,70.]));assert_eq!(source(&s),original);
        }
        edit(&mut s,contact(ContactPhase::Down,[25.,0.]));edit(&mut s,contact(ContactPhase::Move,[25.,-40.]));
        assert_eq!(source(&s).points().len(),2);
        edit(&mut s,CurveEditorAction::Key{epoch,key_event:"Escape".into(),pressed:true,repeat:false,modifiers:Modifiers::default()});
        assert_eq!(source(&s),original);assert!(s.state.pressure_calibration.is_some());
        edit(&mut s,contact(ContactPhase::Down,[25.,0.]));edit(&mut s,contact(ContactPhase::Up,[25.,-40.]));
        assert_eq!(source(&s).points().len(),2);edit(&mut s,CurveEditorAction::Reset);
        for (point,outside) in [([0.,87.5],[-40.,87.5]),([100.,0.],[140.,0.])] {
            edit(&mut s,contact(ContactPhase::Down,point));edit(&mut s,contact(ContactPhase::Move,outside));
            assert_eq!(source(&s).points().len(),3);edit(&mut s,contact(ContactPhase::Up,outside));
            assert_eq!(source(&s).points().len(),3);
        }
    }
}
