use super::{test_support::*, *};
use layer_core::TiltAxes;
use layer_engine::{SampleFlags,ToolKind};

fn device(id:u64,key:&str,turn:u8)->PenDeviceInfo {
    PenDeviceInfo{id,key:key.into(),label:format!("Pen {key}"),native_to_surface:TiltAxes::quarter_turns(turn)}
}
fn hover(s:&mut UiSession<Recorder>,id:u64,tilt:[f32;2]) {
    s.cursor_input(Some(PenEvent{device_id:id,tilt_radians:tilt,..event(s,1,PenPhase::Hover,0.)}));
}
fn panel(s:&mut UiSession<Recorder>,action:PressureCalibrationAction) {
    s.dispatch(UiAction::TiltCalibration{action:TiltCalibrationAction::Panel{action}}).unwrap();
}
fn axes(s:&mut UiSession<Recorder>,axes:TiltAxes) {
    s.dispatch(UiAction::TiltCalibration{action:TiltCalibrationAction::Axes{axes}}).unwrap();
}
fn edit(s:&mut UiSession<Recorder>,action:CurveEditorAction) {
    s.dispatch(UiAction::CurveEditor{target:CurveEditorTarget::Tilt,action}).unwrap();
}
fn open(s:&mut UiSession<Recorder>) {invoke(s,CommandId::PenTilt);}
fn view(s:&UiSession<Recorder>)->&TiltCalibrationView {s.state.tilt_calibration.as_ref().unwrap()}

#[test]
fn tilt_first_pen_curve_contact_keeps_the_displayed_owner_through_binding() {
    for platform in Platform::ALL {
        for ending in [ContactPhase::Up,ContactPhase::Cancel] {
            let mut s=session(platform);let mut saved=layer_core::TiltCalibration::default();
            saved.response.sensitivity(false);
            let mut settings=s.state.settings.clone();settings.tilt_calibrations.insert("pen".into(),saved.clone());
            s.apply_settings(settings).unwrap();open(&mut s);
            assert!(!view(&s).ready);let epoch=view(&s).panel.editor.controls.epoch;
            s.dispatch(UiAction::PenInput{device:device(0,"pen",0),
                sample:Some([400.,300.,0.5,0.4,0.2,0.,0.,1_000_000_000.,1.])}).unwrap();
            let loaded=saved.response.points();
            assert!(view(&s).ready);assert_eq!(view(&s).panel.editor.points,loaded);
            for (phase,point) in [(ContactPhase::Down,[50.,50.]),(ContactPhase::Move,[40.,25.])] {
                edit(&mut s,CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]});
            }
            let edited=view(&s).panel.editor.points.clone();
            assert_eq!(edited.len(),loaded.len()+1,"{platform:?} {ending:?}: the first pen gesture must edit the loaded response");
            assert_eq!(edited[0],loaded[0]);assert_eq!(edited[2],loaded[1]);
            assert!((edited[1][0]-0.4).abs()<f32::EPSILON && (edited[1][1]-0.75).abs()<f32::EPSILON);
            assert_eq!(view(&s).panel.editor.controls.epoch,epoch);
            edit(&mut s,CurveEditorAction::Contact{epoch,phase:ending,point:[40.,25.],extent:[100.,100.]});
            assert_eq!(view(&s).panel.editor.points,if ending==ContactPhase::Cancel {loaded.clone()}else{edited});
            panel(&mut s,PressureCalibrationAction::Cancel);
            assert_eq!(s.state.settings.tilt_calibrations["pen"],saved);
            assert_eq!(s.engine.configured_tilt_curve(),&layer_engine::TiltCurve::new(&saved,TiltAxes::default()));
            open(&mut s);let next=view(&s).panel.editor.controls.epoch;assert_ne!(next,epoch);
            assert_eq!(view(&s).panel.editor.points,loaded);
            edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Down,point:[50.,50.],extent:[100.,100.]});
            assert_eq!(view(&s).panel.editor.points,loaded,"A closed editor's Down must not enter the reopened editor");
            for (phase,point) in [(ContactPhase::Down,[50.,50.]),(ContactPhase::Move,[40.,25.])] {
                edit(&mut s,CurveEditorAction::Contact{epoch:next,phase,point,extent:[100.,100.]});
            }
            let current=view(&s).panel.editor.points.clone();assert_ne!(current,loaded);
            edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Cancel,point:[0.;2],extent:[100.,100.]});
            assert_eq!(view(&s).panel.editor.points,current,"A closed editor's Cancel must not roll back the current gesture");
            edit(&mut s,CurveEditorAction::Contact{epoch:next,phase:ContactPhase::Up,point:[40.,25.],extent:[100.,100.]});
            assert_eq!(view(&s).panel.editor.points,current);
            panel(&mut s,PressureCalibrationAction::Cancel);
            assert_eq!(s.state.settings.tilt_calibrations["pen"],saved);
        }
    }
}

#[test]
fn pen_sensor_dispatch_bypasses_workspace_action_gates_without_canvas_input() {
    for platform in Platform::ALL {
        for pressure in [false,true] {
            let mut s=session(platform);
            invoke(&mut s,if pressure {CommandId::PenPressure}else{CommandId::PenTilt});
            s.begin_workspace_transition().unwrap();
            let document=s.engine.document().clone();let camera=s.state.camera.clone();let cursor=s.cursor.event;
            let mut pen=device(0,"source",0);pen.label.clear();
            let input=UiAction::PenInput{device:pen,sample:Some([400.,300.,0.5,0.4,0.2,0.,0.,1_000_000_000.,1.])};
            assert!(input.is_host_report());let changed=s.dispatch(input.clone()).unwrap();
            assert_ne!(changed.regions & if pressure {regions::PRESSURE_CALIBRATION}else{regions::TILT_CALIBRATION},0);
            assert!(!s.input_ui_pending());
            let repeated=s.dispatch(input).unwrap();assert_eq!(repeated.regions,0);assert_eq!(repeated.revision,changed.revision);
            if pressure {assert!(s.state.pressure_calibration.as_ref().unwrap().editor.marker.is_some());}
            else {assert!(view(&s).ready);assert_eq!(view(&s).device,s.state.localization.text(MessageId::COMMAND_PEN).as_ref());}
            assert_eq!(s.engine.document(),&document);assert_eq!(s.state.camera,camera);assert_eq!(s.cursor.event,cursor);
            assert!(!s.engine.has_active_stroke());assert!(!s.engine.has_pending_input());
            s.end_workspace_transition();
        }
    }
}

#[test]
fn pen_sensor_dispatch_rejects_invalid_samples_before_registering_a_source() {
    let mut s=session(Platform::Gtk);open(&mut s);let initial=view(&s).clone();
    let sample=[400.,300.,0.5,0.4,0.2,0.,0.,1_000_000_000.,0.];
    for (index,value) in [(0,f64::NAN),(0,f64::MAX),(2,f64::INFINITY),(3,f64::NAN),(5,f64::INFINITY),(6,f64::NAN),(6,f64::MAX),(7,-1.),(7,u64::MAX as f64),(8,-1.),(8,5.),(8,0.5)] {
        let mut invalid=sample;invalid[index]=value;
        assert!(s.dispatch(UiAction::PenInput{device:device(0,"invalid",0),sample:Some(invalid)}).is_err());
        assert_eq!(view(&s),&initial);
    }
    assert!(s.dispatch(UiAction::PenInput{device:device(0,"",0),sample:None}).is_err());
    hover(&mut s,0,[0.4,0.2]);assert!(!view(&s).ready);
}

#[test]
fn tilt_target_survives_reassigned_hover_aliases_and_follows_its_own_new_contact_frame() {
    for id in [0,7] {
        let mut s=session(Platform::Gtk);open(&mut s);
        s.register_pen_device(&device(id,"first",0));hover(&mut s,id,[0.4,0.2]);
        let correction=TiltAxes{reverse_horizontal:true,..Default::default()};axes(&mut s,correction);
        panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true});
        let draft=view(&s).clone();
        s.register_pen_device(&device(id,"second",0));hover(&mut s,id,[0.2,0.6]);
        assert_eq!(view(&s),&draft,"A reused runtime alias cannot retarget or replace the draft");
        assert_eq!(s.engine.configured_tilt_curve().map([0.5,0.]),[0.5,0.],"The second pen uses its own neutral response");
        s.register_pen_device(&device(42,"first",1));
        assert_eq!(view(&s).axes,correction.in_frame(TiltAxes::quarter_turns(1)));
        hover(&mut s,42,[0.2,0.4]);
        assert_eq!(view(&s).device,"Pen first");
        assert_eq!(view(&s).panel.editor.points,draft.panel.editor.points);
        panel(&mut s,PressureCalibrationAction::Apply);
        let saved=&s.state.settings.tilt_calibrations["first"];
        assert_eq!(saved.axes,correction);assert_eq!(saved.response.points(),draft.panel.editor.points);
        assert!(!s.state.settings.tilt_calibrations.contains_key("second"));
    }
}

#[test]
fn tilt_editor_identifies_a_pen_and_keeps_its_target_until_closed() {
    let mut s=session(Platform::Gtk);open(&mut s);
    assert!(!view(&s).ready);panel(&mut s,PressureCalibrationAction::Apply);assert!(s.state.tilt_calibration.is_some());
    s.register_pen_device(&device(2,"first",0));hover(&mut s,2,[0.5,0.]);assert!(view(&s).ready);
    panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true});
    let draft=view(&s).panel.editor.points.clone();
    s.register_pen_device(&device(3,"second",0));hover(&mut s,3,[0.,0.6]);
    assert_eq!(view(&s).device,"Pen first");assert_eq!(view(&s).panel.editor.points,draft);
    assert_eq!(s.engine.configured_tilt_curve(),&layer_engine::TiltCurve::default());
    s.dispatch(UiAction::OpenSettings{page:SettingsPage::Input}).unwrap();
    open(&mut s);assert!(!s.state.settings_open);assert_eq!(view(&s).panel.editor.points,draft,"reopening retains the draft");
    panel(&mut s,PressureCalibrationAction::Apply);assert!(s.state.settings.tilt_calibrations.contains_key("first"));
    assert!(!s.state.settings.tilt_calibrations.contains_key("second"));
    open(&mut s);assert_eq!(view(&s).device,"Pen second");assert_eq!(view(&s).panel.editor.points,[[0.,0.],[1.,1.]]);
}

#[test]
fn tilt_preview_applies_cancels_resets_and_restores_per_device_settings() {
    let mut s=session(Platform::Gtk);s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.5,0.]);
    let workspace=serde_json::to_value(&s.state.workspace).unwrap();let revision=s.engine.document().revision;
    open(&mut s);hover(&mut s,2,[0.5,0.]);panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:false});
    axes(&mut s,TiltAxes{reverse_horizontal:true,..Default::default()});
    assert_eq!(view(&s).panel.editor.controls.axes[0].maximum,"90°");
    assert_eq!(view(&s).panel.editor.points,[[0.,0.],[1.,0.975]]);
    let direction=view(&s).adjusted_direction.unwrap();assert!(direction[0]<0.);
    assert!(direction[0].abs()<view(&s).input_direction.unwrap()[0]);
    assert!(s.state.settings.tilt_calibrations.is_empty());
    panel(&mut s,PressureCalibrationAction::Cancel);assert_eq!(s.engine.configured_tilt_curve(),&layer_engine::TiltCurve::default());
    open(&mut s);panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true});
    axes(&mut s,TiltAxes{swap:true,..Default::default()});panel(&mut s,PressureCalibrationAction::Apply);
    let settings:Settings=serde_json::from_slice(&serde_json::to_vec(&s.state.settings).unwrap()).unwrap();
    let saved=settings.tilt_calibrations["pen"].clone();assert!(saved.axes.swap);
    open(&mut s);edit(&mut s,CurveEditorAction::Reset);assert_eq!(view(&s).axes,TiltAxes::default());
    assert_eq!(view(&s).panel.editor.points,[[0.,0.],[1.,1.]]);panel(&mut s,PressureCalibrationAction::Cancel);
    assert_eq!(s.state.settings.tilt_calibrations["pen"],saved);
    assert_eq!(s.engine.document().revision,revision);assert_eq!(serde_json::to_value(&s.state.workspace).unwrap(),workspace);
    let mut restarted=session(Platform::Gtk);restarted.apply_settings(settings).unwrap();
    restarted.register_pen_device(&device(99,"pen",0));hover(&mut restarted,99,[0.5,0.]);open(&mut restarted);
    assert_eq!(view(&restarted).axes,saved.axes);assert_eq!(view(&restarted).panel.editor.points,saved.response.points());
}

#[test]
fn tilt_axis_choices_follow_native_rotation_and_ignore_canvas_rotation() {
    let mut s=session(Platform::Gtk);let mut info=device(2,"pen",0);s.register_pen_device(&info);hover(&mut s,2,[0.5,0.]);open(&mut s);
    let correction=TiltAxes{reverse_horizontal:true,..Default::default()};axes(&mut s,correction);
    for turn in 0..4 {
        info.native_to_surface=TiltAxes::quarter_turns(turn);s.register_pen_device(&info);
        let expected=correction.in_frame(info.native_to_surface);assert_eq!(view(&s).axes,expected);
        let v=info.native_to_surface.apply([0.5,0.]);let output=s.engine.configured_tilt_curve().map(v);
        assert_eq!(output,info.native_to_surface.apply([-0.5,0.]));
        axes(&mut s,expected);
    }
    s.state.camera.rotation=1.;s.sync_camera();assert_eq!(view(&s).axes,correction.in_frame(info.native_to_surface));
    panel(&mut s,PressureCalibrationAction::Apply);assert_eq!(s.state.settings.tilt_calibrations["pen"].axes,correction);
}

#[test]
fn tilt_curve_reuses_point_editing_and_protects_its_origin_and_endpoint() {
    let mut s=session(Platform::Gtk);s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.]);open(&mut s);
    let epoch=view(&s).panel.editor.controls.epoch;
    let contact=|phase,point|CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]};
    edit(&mut s,contact(ContactPhase::Down,[50.,50.]));edit(&mut s,contact(ContactPhase::Up,[50.,25.]));
    assert_eq!(view(&s).panel.editor.points,[[0.,0.],[0.5,0.75],[1.,1.]]);
    edit(&mut s,contact(ContactPhase::Down,[50.,25.]));edit(&mut s,contact(ContactPhase::Move,[50.,-40.]));
    assert_eq!(view(&s).panel.editor.points.len(),2);edit(&mut s,contact(ContactPhase::Cancel,[50.,-40.]));
    assert_eq!(view(&s).panel.editor.points.len(),3);
    edit(&mut s,contact(ContactPhase::Down,[100.,0.]));edit(&mut s,contact(ContactPhase::Up,[60.,0.]));
    assert_eq!(view(&s).panel.editor.points,[[0.,0.],[0.3,0.75],[0.6,1.]]);
    edit(&mut s,contact(ContactPhase::Down,[0.,100.]));edit(&mut s,contact(ContactPhase::Up,[30.,70.]));
    assert_eq!(view(&s).panel.editor.points[0],[0.,0.]);
    edit(&mut s,CurveEditorAction::Key{epoch,key_event:"Escape".into(),pressed:true,repeat:false,modifiers:Modifiers::default()});
    assert!(s.state.tilt_calibration.is_none());assert!(s.state.settings.tilt_calibrations.is_empty());
}

#[test]
fn tilt_and_pressure_share_one_utility_on_every_platform() {
    for platform in Platform::ALL {
        let mut s=session(platform);s.dispatch(UiAction::OpenSettings{page:SettingsPage::Input}).unwrap();
        let has=s.preferences().unwrap().pages.iter().flat_map(|p|&p.groups).flat_map(|g|&g.rows).any(|r|r.id==PreferenceId::PenTilt);
        assert!(has,"{platform:?}");
        invoke(&mut s,CommandId::PenPressure);open(&mut s);assert!(s.state.pressure_calibration.is_none());
        invoke(&mut s,CommandId::PenPressure);assert!(s.state.tilt_calibration.is_none());
    }
}

#[test]
fn tilt_window_measurement_clamps_short_viewports_without_rebuilding_curve_data() {
    let mut s=session(Platform::Gtk);s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.]);open(&mut s);
    let plot=view(&s).panel.editor.plot.as_ptr();let points=view(&s).panel.editor.points.as_ptr();
    panel(&mut s,PressureCalibrationAction::Measure{extent:[336.,640.],viewport:[640.,400.]});
    let bounds=view(&s).panel.bounds;assert_eq!(bounds.height,400.);assert_eq!(bounds.y,0.);
    for (phase,position) in [(ContactPhase::Down,[100.,100.]),(ContactPhase::Move,[-200.,700.]),(ContactPhase::Up,[-200.,700.])] {
        panel(&mut s,PressureCalibrationAction::Drag{phase,position,viewport:[640.,400.]});
    }
    let bounds=view(&s).panel.bounds;assert_eq!(bounds.x,0.);assert_eq!(bounds.y,0.);
    assert_eq!(view(&s).panel.editor.plot.as_ptr(),plot);assert_eq!(view(&s).panel.editor.points.as_ptr(),points);
}

#[test]
fn tilt_edits_before_first_pen_input_survive_binding_and_apply_to_its_input_frame() {
    let mut s=session(Platform::Gtk);open(&mut s);
    assert!(!view(&s).ready);assert!(view(&s).panel.lighter_enabled);
    panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true});
    axes(&mut s,TiltAxes{reverse_horizontal:true,..Default::default()});
    let draft=view(&s).panel.editor.points.clone();assert_eq!(draft,[[0.,0.],[0.975,1.]]);
    assert!(view(&s).axes.reverse_horizontal);
    let mut settings=s.state.settings.clone();
    settings.tilt_calibrations.insert("pen".into(),layer_core::TiltCalibration{axes:TiltAxes{swap:true,..Default::default()},..Default::default()});
    s.apply_settings(settings).unwrap();
    s.register_pen_device(&device(2,"pen",1));hover(&mut s,2,[0.4,0.]);
    assert!(view(&s).ready);assert_eq!(view(&s).panel.editor.points,draft);
    assert_eq!(view(&s).axes,TiltAxes{reverse_horizontal:true,..Default::default()});
    panel(&mut s,PressureCalibrationAction::Apply);
    let saved=&s.state.settings.tilt_calibrations["pen"];
    assert_eq!(saved.response.points(),draft);
    assert_eq!(saved.axes,TiltAxes{reverse_vertical:true,..Default::default()});
}

#[test]
fn tilt_cancel_and_reset_work_before_any_pen_is_known() {
    let mut s=session(Platform::Gtk);open(&mut s);
    let epoch=view(&s).panel.editor.controls.epoch;
    for phase in [ContactPhase::Down,ContactPhase::Up] {
        edit(&mut s,CurveEditorAction::Contact{epoch,phase,point:[50.,50.],extent:[100.,100.]});
    }
    assert_eq!(view(&s).panel.editor.points.len(),3);
    axes(&mut s,TiltAxes{swap:true,..Default::default()});edit(&mut s,CurveEditorAction::Reset);
    assert_eq!(view(&s).panel.editor.points,[[0.,0.],[1.,1.]]);assert_eq!(view(&s).axes,TiltAxes::default());
    panel(&mut s,PressureCalibrationAction::Cancel);assert!(s.state.tilt_calibration.is_none());
    open(&mut s);assert_eq!(view(&s).axes,TiltAxes::default());assert!(s.state.settings.tilt_calibrations.is_empty());
}

#[test]
fn binding_a_pen_preserves_saved_response_components_that_were_not_edited() {
    for edit_response in [false,true] {
        let mut s=session(Platform::Gtk);
        let mut saved=layer_core::TiltCalibration::default();saved.response.sensitivity(false);saved.axes.swap=true;
        let mut settings=s.state.settings.clone();settings.tilt_calibrations.insert("pen".into(),saved.clone());s.apply_settings(settings).unwrap();open(&mut s);
        if edit_response {panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true});}
        else {axes(&mut s,TiltAxes{reverse_horizontal:true,..Default::default()});}
        s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.]);
        if edit_response {assert_eq!(view(&s).axes,saved.axes);}
        else {assert_eq!(view(&s).panel.editor.points,saved.response.points());}
    }
}

#[test]
fn hidden_cursor_still_requests_a_frame_for_pen_binding_and_live_tilt() {
    let mut s=session(Platform::Gtk);let mut settings=s.state.settings.clone();settings.cursor=CursorMode::None;s.apply_settings(settings).unwrap();
    open(&mut s);s.frame(1,1).unwrap();assert!(!s.input_ui_pending());
    s.register_pen_device(&device(2,"pen",0));s.frame(2,2).unwrap();assert!(!s.input_ui_pending());
    hover(&mut s,2,[0.4,0.]);assert!(s.input_ui_pending());assert!(s.frame_demand() != layer_render::FrameDemand::Wait);assert!(view(&s).ready);
    let change=s.frame(3,3).unwrap();assert_ne!(change.regions & regions::TILT_CALIBRATION,0);assert!(!s.input_ui_pending());
    hover(&mut s,2,[0.6,0.]);assert!(s.input_ui_pending());
}

#[test]
fn tilt_first_pen_binding_preserves_measured_and_moved_geometry_for_every_canvas_tool() {
    let viewport=[1600.,1000.];
    for command in [CommandId::Lasso,CommandId::Pencil,CommandId::Hand] {
        for phase in [PenPhase::Hover,PenPhase::Down] {
            for moved in [false,true] {
                let mut s=session(Platform::Gtk);invoke(&mut s,command);open(&mut s);
                panel(&mut s,PressureCalibrationAction::Measure{extent:[384.,622.],viewport});
                let initial=view(&s).panel.bounds;
                if moved {
                    let press=[initial.x+20.,initial.y+20.];
                    for (phase,position) in [(ContactPhase::Down,press),
                        (ContactPhase::Move,[press[0]-60.,press[1]+40.]),
                        (ContactPhase::Up,[press[0]-60.,press[1]+40.])] {
                        panel(&mut s,PressureCalibrationAction::Drag{phase,position,viewport});
                    }
                }
                let bounds=view(&s).panel.bounds;
                assert!(!view(&s).ready);
                s.register_pen_device(&device(2,"pen",0));
                let sample=PenEvent{device_id:2,tilt_radians:[0.4,0.2],..event(&s,1,phase,0.5)};
                if phase==PenPhase::Hover {s.cursor_input(Some(sample));} else {s.pen(sample);}
                assert!(s.state.tilt_calibration.is_some(),"{command:?} {phase:?}, moved={moved}");
                assert!(view(&s).ready,"{command:?} {phase:?}");
                assert_eq!(view(&s).panel.bounds,bounds,"{command:?} {phase:?}, moved={moved}");
            }
        }
    }
}

#[test]
fn tilt_first_pen_binding_preserves_an_active_frame_drag_and_its_cancel_origin() {
    let viewport=[1600.,1000.];
    for phase in [PenPhase::Hover,PenPhase::Down] {
        let mut s=session(Platform::Gtk);open(&mut s);
        panel(&mut s,PressureCalibrationAction::Measure{extent:[384.,622.],viewport});
        let initial=view(&s).panel.bounds;let press=[initial.x+20.,initial.y+20.];
        panel(&mut s,PressureCalibrationAction::Drag{phase:ContactPhase::Down,position:press,viewport});
        panel(&mut s,PressureCalibrationAction::Drag{phase:ContactPhase::Move,position:[press[0]-60.,press[1]+40.],viewport});
        let dragged=view(&s).panel.bounds;
        s.register_pen_device(&device(2,"pen",0));
        let sample=PenEvent{device_id:2,tilt_radians:[0.4,0.2],..event(&s,1,phase,0.5)};
        if phase==PenPhase::Hover {s.cursor_input(Some(sample));} else {s.pen(sample);}
        assert_eq!(view(&s).panel.bounds,dragged,"{phase:?} preserves the current frame");
        panel(&mut s,PressureCalibrationAction::Drag{phase:ContactPhase::Move,position:[press[0]-90.,press[1]+60.],viewport});
        let moved=view(&s).panel.bounds;
        assert_eq!([moved.x,moved.y],[initial.x-90.,initial.y+60.]);
        assert_eq!([moved.width,moved.height],[initial.width,initial.height]);
        panel(&mut s,PressureCalibrationAction::Drag{phase:ContactPhase::Cancel,position:[0.;2],viewport});
        assert_eq!(view(&s).panel.bounds,initial,"{phase:?} preserves the drag's rollback frame");
    }
}

#[test]
fn tilt_cancelled_unbound_curve_gestures_restore_saved_response_and_prior_edit_intent() {
    for cancellation in 0..3 {
        for prior in 0..3 {
            let mut s=session(Platform::Gtk);let mut saved=layer_core::TiltCalibration::default();
            saved.response.sensitivity(false);saved.axes.swap=true;
            let mut settings=s.state.settings.clone();settings.tilt_calibrations.insert("pen".into(),saved.clone());
            s.apply_settings(settings).unwrap();open(&mut s);
            match prior {
                1=>panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true}),
                2=>edit(&mut s,CurveEditorAction::Reset),
                _=>{},
            }
            let before=view(&s).panel.editor.points.clone();let epoch=view(&s).panel.editor.controls.epoch;
            for (phase,point) in [(ContactPhase::Down,[50.,50.]),(ContactPhase::Move,[40.,25.])] {
                edit(&mut s,CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]});
            }
            assert_ne!(view(&s).panel.editor.points,before);
            match cancellation {
                0=>edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Cancel,point:[0.;2],extent:[100.,100.]}),
                1=>edit(&mut s,CurveEditorAction::Key{epoch,key_event:"Escape".into(),pressed:true,repeat:false,modifiers:Modifiers::default()}),
                _=>edit(&mut s,CurveEditorAction::RemoveAt{epoch,point:[40.,25.],extent:[100.,100.],point_count:Some(before.len())}),
            }
            assert_eq!(view(&s).panel.editor.points,before);
            s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.2]);
            let expected=if prior==0 {saved.response.points()} else {before};
            assert_eq!(view(&s).panel.editor.points,expected,"cancellation={cancellation}, prior={prior}");
            let expected_axes=if prior==2 {TiltAxes::default()} else {saved.axes};
            assert_eq!(view(&s).axes,expected_axes);
            panel(&mut s,PressureCalibrationAction::Apply);
            assert_eq!(s.state.settings.tilt_calibrations["pen"].response.points(),expected);
            assert_eq!(s.state.settings.tilt_calibrations["pen"].axes,expected_axes);
        }
    }
}

#[test]
fn tilt_first_pen_binding_waits_for_an_unbound_curve_gesture_to_commit_or_rollback() {
    for ending in 0..3 {
        for prior in 0..3 {
            let mut s=session(Platform::Gtk);let mut saved=layer_core::TiltCalibration::default();
            saved.response.sensitivity(false);saved.axes.swap=true;
            let mut settings=s.state.settings.clone();settings.tilt_calibrations.insert("pen".into(),saved.clone());
            s.apply_settings(settings).unwrap();open(&mut s);
            match prior {
                1=>panel(&mut s,PressureCalibrationAction::Sensitivity{lighter:true}),
                2=>edit(&mut s,CurveEditorAction::Reset),
                _=>{},
            }
            let baseline=view(&s).panel.editor.points.clone();let epoch=view(&s).panel.editor.controls.epoch;
            for (phase,point) in [(ContactPhase::Down,[50.,50.]),(ContactPhase::Move,[40.,25.])] {
                edit(&mut s,CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]});
            }
            let draft=view(&s).panel.editor.points.clone();
            s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.2]);
            assert!(!view(&s).ready,"ending={ending}, prior={prior}");
            assert_eq!(view(&s).panel.editor.points,draft);
            assert_eq!(view(&s).panel.editor.controls.epoch,epoch);
            edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Move,point:[35.,20.],extent:[100.,100.]});
            let continued_draft=view(&s).panel.editor.points.clone();
            assert_ne!(continued_draft,draft,"ending={ending}, prior={prior}");
            assert!(!view(&s).ready,"ending={ending}, prior={prior}");
            assert_eq!(view(&s).panel.editor.controls.epoch,epoch);
            match ending {
                0=>edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Cancel,point:[0.;2],extent:[100.,100.]}),
                1=>edit(&mut s,CurveEditorAction::Key{epoch,key_event:"Escape".into(),pressed:true,repeat:false,modifiers:Modifiers::default()}),
                _=>edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Up,point:[35.,20.],extent:[100.,100.]}),
            }
            assert!(view(&s).ready,"ending={ending}, prior={prior}");
            let expected=if ending==2 {continued_draft} else if prior==0 {saved.response.points()} else {baseline};
            let expected_axes=if prior==2 {TiltAxes::default()} else {saved.axes};
            assert_eq!(view(&s).panel.editor.points,expected,"ending={ending}, prior={prior}");
            assert_eq!(view(&s).axes,expected_axes);
            panel(&mut s,PressureCalibrationAction::Apply);
            assert_eq!(s.state.settings.tilt_calibrations["pen"].response.points(),expected);
            assert_eq!(s.state.settings.tilt_calibrations["pen"].axes,expected_axes);
        }
    }
}

#[test]
fn tilt_stale_curve_cancellation_cannot_discard_a_newer_unbound_edit() {
    let mut s=session(Platform::Gtk);open(&mut s);
    let stale=view(&s).panel.editor.controls.epoch;
    edit(&mut s,CurveEditorAction::Contact{epoch:stale,phase:ContactPhase::Down,point:[50.,50.],extent:[100.,100.]});
    panel(&mut s,PressureCalibrationAction::Cancel);open(&mut s);
    let epoch=view(&s).panel.editor.controls.epoch;assert_ne!(epoch,stale);
    for (phase,point) in [(ContactPhase::Down,[50.,50.]),(ContactPhase::Move,[40.,25.])] {
        edit(&mut s,CurveEditorAction::Contact{epoch,phase,point,extent:[100.,100.]});
    }
    let draft=view(&s).panel.editor.points.clone();
    edit(&mut s,CurveEditorAction::Contact{epoch:stale,phase:ContactPhase::Cancel,point:[0.;2],extent:[100.,100.]});
    assert_eq!(view(&s).panel.editor.points,draft);
    edit(&mut s,CurveEditorAction::Contact{epoch,phase:ContactPhase::Up,point:[40.,25.],extent:[100.,100.]});
    s.register_pen_device(&device(2,"pen",0));hover(&mut s,2,[0.4,0.2]);
    assert_eq!(view(&s).panel.editor.points,draft);
    panel(&mut s,PressureCalibrationAction::Apply);
    assert_eq!(s.state.settings.tilt_calibrations["pen"].response.points(),draft);
}

#[test]
fn calibration_input_updates_both_response_previews_without_canvas_input_or_cursor_changes() {
    for command in [CommandId::PenPressure,CommandId::PenTilt] {
        for tool in [CommandId::Lasso,CommandId::Pencil,CommandId::Hand] {
            for cursor_present in [false,true] {
                let mut s=session(Platform::Gtk);invoke(&mut s,tool);invoke(&mut s,CommandId::SelectAll);
                s.frame(1,1).unwrap();
                if cursor_present {s.cursor_input(Some(event(&s,1,PenPhase::Hover,0.3)));}
                invoke(&mut s,command);s.register_pen_device(&device(2,"pen",0));
                let document=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
                let camera=s.state.camera.clone();let cursor=s.cursor.event;
                let dabs=s.renderer_mut().recorded_dabs.clone();let selection_updates=s.renderer_mut().selection_updates.len();
                for (index,phase) in [PenPhase::Hover,PenPhase::Down,PenPhase::Move,PenPhase::Up,PenPhase::Cancel].into_iter().enumerate() {
                    let sample=PenEvent{device_id:2,tilt_radians:[0.4+index as f32*0.02,0.2],
                        ..event(&s,10+index as u64,phase,0.3+index as f32*0.1)};
                    s.calibration_input(sample);
                    if matches!(phase,PenPhase::Hover|PenPhase::Down|PenPhase::Move) {
                        if command==CommandId::PenTilt {
                            assert!(view(&s).ready);assert!(view(&s).panel.editor.marker.is_some());
                            assert!(view(&s).input_direction.is_some());
                        } else {assert!(s.state.pressure_calibration.as_ref().unwrap().editor.marker.is_some());}
                        assert!(s.input_ui_pending());
                    }
                    s.frame(sample.timestamp_ns,sample.timestamp_ns).unwrap();
                    assert_eq!(s.engine.document(),&document,"{command:?} {tool:?} {phase:?}");
                    assert_eq!(s.engine.checkpoint(),checkpoint);
                    assert_eq!(s.state.camera,camera);assert_eq!(s.cursor.event,cursor);
                    assert!(!s.engine.has_active_stroke());
                    assert_eq!(s.renderer_mut().recorded_dabs,dabs);
                    assert_eq!(s.renderer_mut().selection_updates.len(),selection_updates);
                }
            }
        }
    }
}

#[test]
fn calibration_input_rejects_predicted_corrected_nonpen_and_nonfinite_samples() {
    for command in [CommandId::PenPressure,CommandId::PenTilt] {
      for already_bound in [false,true] {
        let mut s=session(Platform::Gtk);invoke(&mut s,command);s.register_pen_device(&device(2,"pen",0));
        let valid=PenEvent{device_id:2,tilt_radians:[0.4,0.2],..event(&s,2,PenPhase::Hover,0.4)};
        if already_bound {s.calibration_input(valid);}
        s.frame(1,1).unwrap();
        let original_tilt=s.state.tilt_calibration.clone();let original_pressure=s.state.pressure_calibration.clone();
        for sample in [PenEvent{flags:SampleFlags::PREDICTED,..valid},PenEvent{flags:SampleFlags::CORRECTION,..valid},
            PenEvent{tool:ToolKind::Mouse,..valid},PenEvent{tool:ToolKind::Finger,..valid},
            PenEvent{pressure:f32::NAN,..valid},PenEvent{tilt_radians:[f32::NAN,0.2],..valid},
            PenEvent{tilt_radians:[0.4,f32::INFINITY],..valid},PenEvent{twist_radians:f32::NAN,..valid},PenEvent{distance:f32::NAN,..valid},
            PenEvent{surface_position:layer_core::Point{x:f32::NAN,y:300.},..valid},
            PenEvent{surface_position:layer_core::Point{x:250.,y:f32::INFINITY},..valid}] {
            s.calibration_input(sample);
            assert!(s.state.tilt_calibration==original_tilt,"{command:?}: {sample:?}");
            assert!(s.state.pressure_calibration==original_pressure,"{command:?}: {sample:?}");
            assert!(!s.input_ui_pending(),"{command:?}: {sample:?}");
            assert!(s.cursor.event.is_none());
        }
        s.calibration_input(PenEvent{pressure:0.6,tilt_radians:[0.6,0.3],..valid});
        if command==CommandId::PenTilt {assert!(view(&s).ready);} else {assert!(s.state.pressure_calibration.as_ref().unwrap().editor.marker.is_some());}
        assert!(s.input_ui_pending());
      }
    }
}
