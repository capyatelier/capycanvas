use super::*;

fn tilt_pen_input(app:&App,id:u64,key:&str,turn:u8,sample:Option<[f64;9]>) {
    app.action(json!({"type":"pen_input","device":{"id":id,"key":key,"label":format!("Pen {key}"),
        "native_to_surface":layer_core::TiltAxes::quarter_turns(turn)},"sample":sample}));
}

#[test]
fn apple_tilt_sensor_preview_never_routes_canvas_contacts() {
    for platform in [0,1] {
        for theme in ["light","dark"] {
            for panel in ["pen_tilt","pen_pressure"] {
                for tool in ["lasso","pencil","hand"] {
                    let app=App::new(platform);app.action(json!({"type":"set_theme","theme":theme}));
                    app.invoke(tool);app.invoke("select_all");app.invoke(panel);
                    let document=app.document();
                    let checkpoint=app.borrow().host.session.engine().checkpoint();
                    let camera=app.state()["camera"].clone();
                    let cursor=format!("{:?}",app.borrow().host.session.canvas_cursor());
                    let settings=app.state()["settings"].clone();
                    let events=app.borrow().host.session.engine().metrics().input_events;
                    for phase in 0..=4 {
                        let sample=[400.+f64::from(phase)*10.,300.,0.3+f64::from(phase)*0.1,0.4,0.2,0.,0.,
                            1_000_000_000.+f64::from(phase)*10_000_000.,f64::from(phase)];
                        tilt_pen_input(&app,0,"apple-fixture:pen",0,Some(sample));
                        let state=app.full_snapshot()["state"].clone();
                        if panel=="pen_tilt" {
                            assert_eq!(state["tilt_calibration"]["ready"],true);
                            assert_eq!(state["tilt_calibration"]["device"],"Pen apple-fixture:pen");
                            if phase<3 {assert!(!state["tilt_calibration"]["editor"]["marker"].is_null());}
                        } else if phase<3 {assert!(!state["pressure_calibration"]["editor"]["marker"].is_null());}
                        assert_eq!(app.document(),document,"{platform} {theme} {panel} {tool} {phase}");
                        assert_eq!(app.borrow().host.session.engine().checkpoint(),checkpoint);
                        assert_eq!(app.borrow().host.session.engine().metrics().input_events,events);
                        assert!(!app.borrow().host.session.engine().has_pending_input());
                        assert!(!app.borrow().host.session.engine().has_active_stroke());
                        assert_eq!(state["camera"],camera);assert_eq!(state["settings"],settings);
                        assert_eq!(format!("{:?}",app.borrow().host.session.canvas_cursor()),cursor);
                    }
                }
            }
        }
    }
}

#[test]
fn apple_tilt_response_uses_stable_source_keys_across_hover_aliases_and_restart() {
    for platform in [0,1] {
        for theme in ["light","dark"] {
            let app=App::new(platform);app.action(json!({"type":"set_theme","theme":theme}));app.invoke("pen_tilt");
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"sensitivity","lighter":true}}}));
            let axes=layer_core::TiltAxes{reverse_horizontal:true,reverse_vertical:true,swap:false};
            app.action(json!({"type":"tilt_calibration","action":{"kind":"axes","axes":axes}}));
            let response=app.state()["tilt_calibration"]["editor"]["points"].clone();
            let sample=[400.,300.,0.5,0.4,0.2,0.,0.,1_000_000_000.,0.];
            tilt_pen_input(&app,0,"apple-fixture:first",0,Some(sample));
            assert_eq!(app.state()["tilt_calibration"]["ready"],true);
            assert_eq!(app.state()["tilt_calibration"]["editor"]["points"],response);
            tilt_pen_input(&app,42,"apple-fixture:first",0,None);
            assert_eq!(unsafe {capy_apple_pointer(app.0,42,0,0,sample.as_ptr(),sample.len(),0,capy_apple_camera_revision(app.0),0)},0);
            let first=app.state()["tilt_calibration"].clone();
            tilt_pen_input(&app,0,"apple-fixture:second",0,Some([420.,340.,0.8,-0.2,0.6,0.,0.,1_100_000_000.,0.]));
            let retained=app.state()["tilt_calibration"].clone();
            for key in ["device","axes","ready","bounds","input_direction"] {assert_eq!(retained[key],first[key],"{key}");}
            for key in ["points","controls","plot"] {assert_eq!(retained["editor"][key],first["editor"][key],"{key}");}
            for (actual,previous) in [(retained["adjusted_direction"].clone(),first["adjusted_direction"].clone()),
                (retained["editor"]["marker"].clone(),first["editor"]["marker"].clone())] {
                for index in 0..2 {assert!((actual[index].as_f64().unwrap()-previous[index].as_f64().unwrap()).abs()<=f64::from(f32::EPSILON));}
            }
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"apply"}}}));
            let settings=app.state()["settings"].clone();
            assert!(settings["tilt_calibrations"]["apple-fixture:second"].is_null());
            assert_eq!(settings["tilt_calibrations"]["apple-fixture:first"]["axes"],json!(axes));
            tilt_pen_input(&app,63,"apple-fixture:first",0,Some(sample));
            app.invoke("pen_tilt");
            app.action(json!({"type":"curve_editor","target":{"kind":"tilt"},"action":{"kind":"reset"}}));
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"cancel"}}}));
            assert_eq!(app.state()["settings"],settings);
            let restored=App::new(platform);restored.action(json!({"type":"restore_settings","settings":settings}));restored.invoke("pen_tilt");
            tilt_pen_input(&restored,900,"apple-fixture:first",1,None);
            assert_eq!(restored.state()["tilt_calibration"]["ready"],false);
            tilt_pen_input(&restored,900,"apple-fixture:first",1,Some(sample));
            let panel=restored.full_snapshot()["state"]["tilt_calibration"].clone();
            assert_eq!(panel["ready"],true);assert_eq!(panel["editor"]["points"],response);
            assert_eq!(panel["axes"],json!(axes.in_frame(layer_core::TiltAxes::quarter_turns(1))));
            restored.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"apply"}}}));
            assert_eq!(restored.state()["settings"],settings,"Applying in a rotated frame must preserve native-axis settings");
        }
    }
}

#[test]
fn apple_tilt_editor_is_editable_before_first_pen_input_in_both_themes() {
    for platform in [0,1] {
        for theme in ["light","dark"] {
            let app=App::new(platform);
            app.action(json!({"type":"set_theme","theme":theme}));
            let baseline=app.state()["settings"].clone();
            let document=app.document();
            let camera=app.state()["camera"].clone();
            app.invoke("pen_tilt");
            let initial=app.full_snapshot()["state"]["tilt_calibration"].clone();
            assert_eq!(initial["ready"],false,"platform={platform}, theme={theme}");
            assert_eq!(initial["editor"]["points"],json!([[0.,0.],[1.,1.]]));
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{
                "kind":"measure","extent":[384.,622.],"viewport":[1200.,900.]}}}));
            let bounds=app.state()["tilt_calibration"]["bounds"].clone();
            for (horizontal,vertical,swap) in [(true,false,false),(true,true,false),(true,true,true),(false,true,true)] {
                let axes=json!({"reverse_horizontal":horizontal,"reverse_vertical":vertical,"swap":swap});
                app.action(json!({"type":"tilt_calibration","action":{"kind":"axes","axes":axes}}));
                let panel=app.full_snapshot()["state"]["tilt_calibration"].clone();
                assert_eq!(panel["axes"],axes);
                assert_eq!(panel["bounds"],bounds);
            }
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"sensitivity","lighter":true}}}));
            assert_ne!(app.state()["tilt_calibration"]["editor"]["points"],initial["editor"]["points"]);
            app.action(json!({"type":"curve_editor","target":{"kind":"tilt"},"action":{"kind":"reset"}}));
            let reset=app.state()["tilt_calibration"].clone();
            assert_eq!(reset["axes"],initial["axes"]);
            assert_eq!(reset["editor"]["points"],initial["editor"]["points"]);
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"apply"}}}));
            assert!(!app.state()["tilt_calibration"].is_null(),"Unbound Apply cannot save an unidentified pen");
            assert_eq!(app.state()["settings"],baseline);
            app.action(json!({"type":"tilt_calibration","action":{"kind":"panel","action":{"kind":"cancel"}}}));
            assert!(app.full_snapshot()["state"]["tilt_calibration"].is_null());
            assert_eq!(app.state()["settings"],baseline);
            {
                let fifo=App::new(platform);fifo.action(json!({"type":"set_theme","theme":theme}));fifo.invoke("pen_tilt");
                let cold=fifo.state()["tilt_calibration"].clone();assert_eq!(cold["ready"],false);
                let epoch=cold["editor"]["controls"]["epoch"].as_u64().unwrap();
                tilt_pen_input(&fifo,0,"apple-fixture:first-curve",0,Some([400.,300.,0.5,0.4,0.2,0.,0.,1_000_000_000.,1.]));
                let bound=fifo.state()["tilt_calibration"].clone();assert_eq!(bound["ready"],true);
                assert_eq!(bound["device"],"Pen apple-fixture:first-curve");assert_eq!(bound["editor"]["controls"]["epoch"],epoch);
                fifo.action(json!({"type":"curve_editor","target":{"kind":"tilt"},"action":{"kind":"contact","epoch":epoch,"phase":"down","point":[50.,50.],"extent":[100.,100.]}}));
                assert_eq!(fifo.state()["tilt_calibration"]["editor"]["points"].as_array().unwrap().len(),3,"The identifying pen contact must reach the displayed curve owner");
                fifo.action(json!({"type":"curve_editor","target":{"kind":"tilt"},"action":{"kind":"contact","epoch":epoch,"phase":"cancel","point":[0.,0.],"extent":[100.,100.]}}));
                assert_eq!(fifo.state()["tilt_calibration"]["editor"]["points"],cold["editor"]["points"]);
            }
            app.invoke("pen_pressure");app.invoke("pen_tilt");
            assert!(app.state()["pressure_calibration"].is_null());
            app.invoke("pen_pressure");
            assert!(app.full_snapshot()["state"]["tilt_calibration"].is_null());
            assert!(!app.state()["pressure_calibration"].is_null());
            assert_eq!(app.document(),document);
            assert_eq!(app.state()["camera"],camera);
        }
    }
}

#[test]
fn packed_pointer_device_flags_preserve_zero_rotation_and_indirect_hover() {
    use layer_engine::{SampleFlags, recording::{Record, StopReason}};
    let indirect = u32::from(SampleFlags::INDIRECT_POINTER.0);
    let barrel = u32::from(SampleFlags::BARREL_TWIST.0);
    for platform in [0, 1] {
        let app = App::new(platform);
        let mut settings = app.state()["settings"].clone();
        settings["cursor"] = json!("none");
        app.action(json!({"type":"restore_settings","settings":settings}));
        let recording = app.borrow().host.session.engine().recording.clone();
        recording.lock().unwrap().start("apple-device-flags").unwrap();
        let mut expected = Vec::new();
        for updates in [false, true] {
            for flags in [0, indirect, barrel, indirect | barrel] {
                let sequence = expected.len() as u64 + 1;
                let sample = [500., 400., 0.6, 0.2, -0.3, 0., 0., sequence as f64 * 1_000_000., 0.];
                let revision = unsafe { capy_apple_camera_revision(app.0) };
                let result = if updates {
                    unsafe { capy_apple_pointer_updates(app.0, 9, 0, 0, sample.as_ptr(), sample.len(),
                        [sequence, 0].as_ptr(), 0, revision, flags) }
                } else {
                    unsafe { capy_apple_pointer(app.0, 9, 0, 0, sample.as_ptr(), sample.len(), 0, revision, flags) }
                };
                assert_eq!(result, 0, "platform={platform} updates={updates} flags={flags:#x}");
                let cursor = app.borrow().host.session.canvas_cursor().unwrap();
                assert_eq!(cursor.mode, if flags & indirect != 0 { layer_ui::CursorMode::Sight } else { layer_ui::CursorMode::None });
                expected.push(flags);
            }
        }
        let mut recorder = recording.lock().unwrap();
        recorder.stop(StopReason::Manual);
        let events: Vec<_> = layer_engine::recording::read(recorder.bytes().unwrap().as_slice()).unwrap().into_iter()
            .filter_map(|record| match record { Record::Raw { event, .. } => Some(event), _ => None }).collect();
        assert_eq!(events.len(), expected.len());
        for (event, flags) in events.into_iter().zip(expected) {
            assert_eq!(u32::from(event.flags.0) & (indirect | barrel), flags);
            assert_eq!(event.twist_radians, 0., "zero rotation retains its explicit capability");
            assert_eq!(event.pressure, 0.6);
            assert_eq!(event.tilt_radians, [0.2, -0.3]);
        }
        assert!(app.borrow().host.session.engine().backend().0.is_none());
    }
}

#[test]
fn packed_pointer_device_flags_reject_sample_and_high_bits_without_changing_hover() {
    use layer_engine::SampleFlags;
    for platform in [0, 1] {
        let app = App::new(platform);
        let revision = unsafe { capy_apple_camera_revision(app.0) };
        let sample = [500., 400., 0.6, 0., 0., 0., 0., 1_000_000., 0.];
        assert_eq!(unsafe { capy_apple_pointer(app.0, 9, 0, 0, sample.as_ptr(), sample.len(), 0, revision, 0) }, 0);
        let before = app.borrow().host.session.canvas_cursor().unwrap();
        let mut invalid = sample;
        invalid[0] += 100.;
        for flags in [u32::from(SampleFlags::PREDICTED.0), u32::from(SampleFlags::PRIMARY.0),
            u32::from(SampleFlags::INVERTED.0), u32::from(SampleFlags::ESTIMATED.0),
            u32::from(SampleFlags::CORRECTION.0), 1 << 8, 1 << 15, 1 << 16, 1 << 31,
            (1 << 16) | u32::from(SampleFlags::BARREL_TWIST.0), u32::MAX] {
            for updates in [false, true] {
                let result = if updates {
                    unsafe { capy_apple_pointer_updates(app.0, 9, 0, 0, invalid.as_ptr(), invalid.len(),
                        [1, 0].as_ptr(), 0, revision, flags) }
                } else {
                    unsafe { capy_apple_pointer(app.0, 9, 0, 0, invalid.as_ptr(), invalid.len(), 0, revision, flags) }
                };
                assert_eq!(result, -1, "platform={platform} updates={updates} flags={flags:#x}");
                let after = app.borrow().host.session.canvas_cursor().unwrap();
                assert_eq!(after.center, before.center);
                assert_eq!(after.mode, before.mode);
            }
        }
    }
}

fn packed_admission_app(platform:u32)->App {
    let app=fixtures::selection_app(platform);
    app.action(json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::GPen as u32}));
    app.action(json!({"type":"set_brush_size","value":4}));
    app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
    app.draw_until_idle();
    fixtures::drag(&app,1,0,[8.,8.],[20.,8.]);
    app.invoke("undo");app.draw_until_idle();
    assert!(app.borrow().host.session.engine().can_redo());
    app
}

fn packed_admission_sample(app:&App,x:f64,time:f64,phase:f64)->[f64;9] {
    let [x,y]=fixtures::surface(app,[x,32.]);
    [x,y,0.8,0.,0.,0.,0.,time,phase]
}

fn packed_admission_records(app:&App)->Vec<layer_engine::recording::Record> {
    let recording=app.borrow().host.session.engine().recording.clone();
    let mut recorder=recording.lock().unwrap();
    recorder.stop(layer_engine::recording::StopReason::Manual);
    let records=layer_engine::recording::read(recorder.bytes().unwrap().as_slice()).unwrap();
    recorder.saved();records
}

#[test]
fn packed_pointer_malformed_samples_preserve_valid_siblings_and_exact_history() {
    use layer_engine::{PenPhase,recording::Record};
    for platform in [0,1] {
        for updates in [false,true] {
            let app=packed_admission_app(platform);
            let baseline=app.pixels();
            let revision=unsafe {capy_apple_camera_revision(app.0)};
            let valid=[packed_admission_sample(&app,16.,3_000_000_000.,1.),
                packed_admission_sample(&app,24.,3_010_000_000.,2.),
                packed_admission_sample(&app,40.,3_030_000_000.,2.),
                packed_admission_sample(&app,48.,3_040_000_000.,3.)];
            let oracle:Vec<_>=valid.into_iter().flatten().collect();
            assert_eq!(unsafe {capy_apple_pointer(app.0,9,0,0,oracle.as_ptr(),oracle.len(),0,revision,0)},0);
            app.draw_until_idle();let expected=app.pixels();assert_ne!(expected,baseline);
            app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),baseline);
            for (field,value) in [(0,f64::NAN),(0,f64::MAX),(3,f64::INFINITY),(7,-1.)] {
                let document=app.document();let checkpoint=app.borrow().host.session.engine().checkpoint();
                app.borrow().host.session.engine().recording.lock().unwrap().start("apple-malformed-siblings").unwrap();
                let mut damaged=packed_admission_sample(&app,32.,3_020_000_000.,2.);damaged[field]=value;
                let records:Vec<_>=[valid[0],valid[1],damaged,valid[2],valid[3]].into_iter().flatten().collect();
                let metadata=[1,0,2,0,3,0,4,0,5,0];
                let result=if updates {
                    unsafe {capy_apple_pointer_updates(app.0,9,0,0,records.as_ptr(),records.len(),metadata.as_ptr(),0,revision,0)}
                } else {unsafe {capy_apple_pointer(app.0,9,0,0,records.as_ptr(),records.len(),0,revision,0)}};
                assert_eq!(result,0,"platform={platform} updates={updates} field={field}");
                app.draw_until_idle();assert_eq!(app.pixels(),expected);
                let recorded=packed_admission_records(&app);
                let raw:Vec<_>=recorded.iter().filter_map(|r|if let Record::Raw{event,..}=r {Some(*event)}else{None}).collect();
                assert_eq!(raw.len(),valid.len());
                for (event,sample) in raw.iter().zip(valid) {
                    assert!(event.is_finite());assert_eq!(event.device_id,9);
                    assert_eq!([event.surface_position.x,event.surface_position.y],[sample[0] as f32,sample[1] as f32]);
                    assert_eq!(event.timestamp_ns,sample[7] as u64);
                }
                assert_eq!(raw.iter().map(|e|e.phase).collect::<Vec<_>>(),[PenPhase::Down,PenPhase::Move,PenPhase::Move,PenPhase::Up]);
                assert_eq!(recorded.iter().filter(|r|matches!(r,Record::End{cancelled:false,..})).count(),1);
                assert!(!recorded.iter().any(|r|matches!(r,Record::End{cancelled:true,..})));
                app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),baseline);
                assert_project_document(&app.document(),&document);assert_eq!(app.document().working,document.working);assert_eq!(app.borrow().host.session.engine().checkpoint(),checkpoint);
                assert!(app.borrow().host.session.engine().can_redo());
                app.invoke("redo");app.draw_until_idle();assert_eq!(app.pixels(),expected);
                app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),baseline);
            }
        }
    }
}

#[test]
fn packed_pointer_damaged_real_terminals_cancel_once_and_allow_fresh_contacts() {
    use layer_engine::{PenPhase,recording::Record};
    for platform in [0,1] {
        let app=packed_admission_app(platform);let baseline=app.pixels();
        for updates in [false,true] {
            for phase in [3.,4.] {
                let document=app.document();let checkpoint=app.borrow().host.session.engine().checkpoint();
                let revision=unsafe {capy_apple_camera_revision(app.0)};
                app.borrow().host.session.engine().recording.lock().unwrap().start("apple-malformed-terminal").unwrap();
                let down=packed_admission_sample(&app,16.,3_000_000_000.,1.);
                let moved=packed_admission_sample(&app,32.,3_010_000_000.,2.);
                let prefix:Vec<_>=[down,moved].into_iter().flatten().collect();
                assert_eq!(unsafe {capy_apple_pointer(app.0,9,0,0,prefix.as_ptr(),prefix.len(),0,revision,0)},0);
                fixtures::until(&app,"Real contact did not start",||app.borrow().host.session.engine().has_active_stroke());
                let mut damaged=packed_admission_sample(&app,48.,3_020_000_000.,phase);damaged[0]=f64::NAN;
                let result=if updates {
                    unsafe {capy_apple_pointer_updates(app.0,9,0,0,damaged.as_ptr(),damaged.len(),[3,0].as_ptr(),0,revision,0)}
                } else {unsafe {capy_apple_pointer(app.0,9,0,0,damaged.as_ptr(),damaged.len(),0,revision,0)}};
                assert_eq!(result,0,"platform={platform} updates={updates} phase={phase}");
                app.draw_until_idle();assert_eq!(app.pixels(),baseline);assert_project_document(&app.document(),&document);assert_eq!(app.document().working,document.working);
                assert_eq!(app.borrow().host.session.engine().checkpoint(),checkpoint);
                assert!(app.borrow().host.session.engine().can_redo());
                let late:Vec<_>=[packed_admission_sample(&app,52.,3_030_000_000.,2.),
                    packed_admission_sample(&app,56.,3_040_000_000.,3.)].into_iter().flatten().collect();
                assert_eq!(unsafe {capy_apple_pointer(app.0,9,0,0,late.as_ptr(),late.len(),0,revision,0)},0);
                app.draw_until_idle();assert_eq!(app.pixels(),baseline);assert_project_document(&app.document(),&document);assert_eq!(app.document().working,document.working);
                let recorded=packed_admission_records(&app);
                let raw:Vec<_>=recorded.iter().filter_map(|r|if let Record::Raw{event,..}=r {Some(*event)}else{None}).collect();
                assert_eq!(raw.iter().map(|e|e.phase).collect::<Vec<_>>(),[PenPhase::Down,PenPhase::Move,PenPhase::Cancel]);
                let cancelled=raw.last().unwrap();assert!(cancelled.is_finite());assert_eq!(cancelled.device_id,9);
                assert_eq!([cancelled.surface_position.x,cancelled.surface_position.y],[moved[0] as f32,moved[1] as f32]);
                assert_eq!(recorded.iter().filter(|r|matches!(r,Record::End{cancelled:true,..})).count(),1);
                fixtures::drag(&app,10,0,[16.,32.],[48.,32.]);let ink=app.pixels();assert_ne!(ink,baseline);
                app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),baseline);
                app.invoke("redo");app.draw_until_idle();assert_eq!(app.pixels(),ink);
                app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),baseline);
            }
        }
    }
}

#[test]
fn packed_pointer_structural_failures_cancel_only_matching_real_current_owners() {
    use layer_engine::{PenPhase,recording::Record};
    for platform in [0,1] {
        eprintln!("packed structure platform={platform}: initialize fixture and baseline");
        let app=packed_admission_app(platform);let baseline=app.pixels();
        let revision=unsafe {capy_apple_camera_revision(app.0)};
        assert!(!app.borrow().host.accepts_pointer_input(0));
        for case in ["short","null-records","metadata","null-updates","foreign-id","foreign-kind",
            "predicted","correction","stale-sample","stale-short","stale-metadata","stale-null-updates"] {
            eprintln!("packed structure platform={platform} case={case}: begin");
            let settle=|step:&str| {
                eprintln!("packed structure platform={platform} case={case}: {step}");
                app.draw_until_idle();
            };
            let retired=|terminal:&str| {
                let message=format!("packed structure platform={platform} case={case}: {terminal} must retire the real owner and drain input");
                eprintln!("{message}");
                fixtures::until(&app,&message,|| {
                    let native=app.borrow();let engine=native.host.session.engine();
                    !engine.has_active_stroke() && !engine.has_pending_input()
                });
            };
            let document=app.document();let checkpoint=app.borrow().host.session.engine().checkpoint();
            app.borrow().host.session.engine().recording.lock().unwrap().start("apple-malformed-structure").unwrap();
            let prefix:Vec<_>=[packed_admission_sample(&app,16.,3_000_000_000.,1.),
                packed_admission_sample(&app,32.,3_010_000_000.,2.)].into_iter().flatten().collect();
            assert_eq!(unsafe {capy_apple_pointer(app.0,9,0,0,prefix.as_ptr(),prefix.len(),0,revision,0)},0);
            let started=format!("packed structure platform={platform} case={case}: real contact must start");
            eprintln!("{started}");
            fixtures::until(&app,&started,||app.borrow().host.session.engine().has_active_stroke());
            let mut terminal=packed_admission_sample(&app,48.,3_020_000_000.,3.);
            let stale=case.starts_with("stale-");let delivered_revision=if stale {0}else{revision};
            let id=if case=="foreign-id" {99}else{9};let tool=if case=="foreign-kind" {1}else{0};
            let result=match case {
                "metadata"|"correction"|"stale-metadata"=>unsafe {capy_apple_pointer_updates(app.0,9,0,0,terminal.as_ptr(),terminal.len(),
                    [0,1].as_ptr(),u32::from(case=="correction"),delivered_revision,0)},
                "null-updates"|"stale-null-updates"=>unsafe {capy_apple_pointer_updates(app.0,9,0,0,terminal.as_ptr(),terminal.len(),
                    std::ptr::null(),0,delivered_revision,0)},
                "null-records"=>unsafe {capy_apple_pointer(app.0,9,0,0,std::ptr::null(),terminal.len(),0,delivered_revision,0)},
                "stale-sample"=>{terminal[0]=f64::NAN;unsafe {capy_apple_pointer(app.0,9,0,0,terminal.as_ptr(),terminal.len(),0,delivered_revision,0)}},
                _=>unsafe {capy_apple_pointer(app.0,id,tool,0,terminal.as_ptr(),terminal.len()-1,u32::from(case=="predicted"),delivered_revision,0)},
            };
            assert_eq!(result,if stale {0}else{-1},"platform={platform} case={case}");
            let cancels=matches!(case,"short"|"null-records"|"metadata"|"null-updates");
            if cancels {
                retired("rejected matching real batch");settle("settle cancelled contact");
                assert_eq!(app.pixels(),baseline);assert_project_document(&app.document(),&document);assert_eq!(app.document().working,document.working);
                assert_eq!(app.borrow().host.session.engine().checkpoint(),checkpoint);
                assert!(app.borrow().host.session.engine().can_redo());
            } else {
                assert!(app.borrow().host.session.engine().has_active_stroke(),"platform={platform} case={case}");
                let up=packed_admission_sample(&app,48.,3_030_000_000.,3.);
                assert_eq!(unsafe {capy_apple_pointer(app.0,9,0,0,up.as_ptr(),up.len(),0,revision,0)},0);
                retired("valid real Up after isolated rejection");settle("settle committed contact");
                assert_ne!(app.pixels(),baseline);
            }
            let recorded=packed_admission_records(&app);
            let raw:Vec<_>=recorded.iter().filter_map(|r|if let Record::Raw{event,..}=r {Some(*event)}else{None}).collect();
            assert_eq!(raw.iter().map(|e|e.phase).collect::<Vec<_>>(),
                [PenPhase::Down,PenPhase::Move,if cancels {PenPhase::Cancel}else{PenPhase::Up}],"platform={platform} case={case}");
            assert!(raw.iter().all(|e|e.is_finite() && e.device_id==9 && e.tool==layer_engine::ToolKind::Pen));
            assert_eq!(recorded.iter().filter(|r|matches!(r,Record::End{cancelled:true,..})).count(),usize::from(cancels));
            if !cancels {app.invoke("undo");settle("undo committed contact");assert_eq!(app.pixels(),baseline);assert_project_document(&app.document(),&document);assert_eq!(app.document().working,document.working);}
            eprintln!("packed structure platform={platform} case={case}: fresh native contact and idle");
            fixtures::drag(&app,10,0,[16.,32.],[48.,32.]);let ink=app.pixels();assert_ne!(ink,baseline);
            app.invoke("undo");settle("undo fresh contact");assert_eq!(app.pixels(),baseline);
            app.invoke("redo");settle("redo fresh contact");assert_eq!(app.pixels(),ink);
            app.invoke("undo");settle("restore baseline");assert_eq!(app.pixels(),baseline);
        }
    }
}

#[test]
fn consumed_paint_timestamp_advances_only_after_real_input_reaches_the_engine() {
    assert_eq!(unsafe { capy_apple_consumed_paint_ns(0) }, 0);
    for platform in [0, 1] {
        let app = fixtures::selection_app(platform);
        app.action(json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::GPen as u32}));
        app.action(json!({"type":"set_brush_size","value":4}));
        app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
        app.draw_until_idle();
        assert_eq!(unsafe { capy_apple_consumed_paint_ns(app.0) }, 0);
        let [x, y] = fixtures::surface(&app, [20., 20.]);
        for (offset, phase, timestamp, predicted, consumed) in [
            (0., 1., 1_000_000_000u64, 0, 1_000_000_000),
            (8., 2., 1_080_000_000, 1, 1_000_000_000),
            (2., 2., 1_020_000_000, 0, 1_020_000_000),
            (2., 3., 1_040_000_000, 0, 1_020_000_000),
        ] {
            let previous = unsafe { capy_apple_consumed_paint_ns(app.0) };
            let record = [x + offset, y, 0.8, 0., 0., 0., 0., timestamp as f64, phase];
            assert_eq!(unsafe {
                capy_apple_pointer(app.0, 1, 0, 0, record.as_ptr(), record.len(), predicted,
                    capy_apple_camera_revision(app.0), 0)
            }, 0);
            assert!(app.borrow().host.session.engine().has_pending_input());
            assert_eq!(unsafe { capy_apple_consumed_paint_ns(app.0) }, previous,
                "Receipt and enqueueing do not acknowledge engine consumption");
            let events = app.borrow().host.session.engine().metrics().input_events;
            fixtures::until(&app, "Pointer input did not reach the engine", || {
                !app.borrow().host.session.engine().has_pending_input()
            });
            assert!(app.borrow().host.session.engine().metrics().input_events > events);
            assert_eq!(unsafe { capy_apple_consumed_paint_ns(app.0) }, consumed,
                "Only consumed real Down/Move samples advance the timestamp");
        }
    }
}

#[test]
fn mac_manual_prediction_adds_lead_only_for_the_pen() {
    for tool in [0, 1] {
        for milliseconds in [0, 64] {
            let app = App::new(1);
            app.borrow().host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            app.action(json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::GPen as u32}));
            app.action(json!({"type":"set_brush_size","value":4}));
            app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
            app.action(json!({"type":"set_color","rgba":[0,0,1,1]}));
            app.invoke("settings");
            app.action(json!({"type":"preferences","action":{"type":"edit","id":"hide_cursor_while_drawing","value":false}}));
            app.action(json!({"type":"preferences","action":{"type":"edit","id":"prediction_horizon","value":milliseconds}}));
            app.action(json!({"type":"close_settings"}));
            app.draw_until_idle();
            let baseline = app.pixels();
            let transform = app.borrow().host.session.state().camera.input_transform();
            let ahead = transform.map(layer_core::Point { x: 720., y: 450. });
            let width = app.document().composition().size[0] as usize;
            let offset = (ahead.y as usize * width + ahead.x as usize) * 4;
            let mut record = [500., 450., 0.8, 0., 0., 0., 0., 0., 1.];
            let handle = app.0;
            let send = move |record: &[f64]| {
                assert_eq!(unsafe { capy_apple_pointer(handle, 1, tool, 0,
                    record.as_ptr(), record.len(), 0, capy_apple_camera_revision(handle), 0) }, 0);
                handles::app(handle).unwrap().host.prepare_canvas_frame(
                    record[7] as u64, record[7] as u64 + 8_000_000, true).unwrap();
            };
            for index in 0..101 {
                record[0] = 500. + f64::from(index) * 2.;
                record[7] = 3_000_000_000. + f64::from(index) * 5_000_000.;
                record[8] = if index == 0 { 1. } else { 2. };
                send(&record);
            }
            let cursor = app.borrow().host.session.canvas_cursor().unwrap();
            assert_eq!(cursor.center, [700., 450.]);
            let live = app.pixels();
            let pixel = &live[offset..offset + 4];
            let blue = i16::from(pixel[2]) > i16::from(pixel[0]) + 50;
            eprintln!("Mac tool={tool} prediction={milliseconds}ms: pixel 20px ahead of cursor={pixel:?}");
            let predicts = tool == 0 && milliseconds == 64;
            assert_eq!(blue, predicts, "Only pen input receives manual prediction lead");
            assert_eq!(app.borrow().host.session.engine().metrics().engine_prediction_frames > 0, predicts);
            record[7] += 1_000_000.;
            record[8] = 3.;
            send(&record);
            let committed = app.pixels();
            assert_eq!(&committed[offset..offset + 4], &baseline[offset..offset + 4],
                "Pen-up must remove predicted ink beyond the real endpoint");
            assert_eq!(app.borrow().host.session.engine().metrics().committed_strokes, 1);
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), baseline);
            app.invoke("redo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), committed);
        }
    }
}

#[test]
fn project_adoption_preserves_native_prediction_and_manual_lookahead() {
    for platform in [0, 1] {
        {
            let app = App::new(platform);
            app.borrow().host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            let mut settings = app.state()["settings"].clone();
            settings["platform_prediction"] = json!(true);
            settings["prediction_ms"] = json!(64.);
            app.action(json!({"type":"restore_settings","settings":settings}));
            app.action(json!({"type":"set_brush_size","value":4}));
            app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
            app.action(json!({"type":"set_color","rgba":[0,0,1,1]}));
            app.draw_until_idle();
            let project = ProjectJob::create(&app, [2048, 1536]);
            let adopted = unsafe {
                capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr())
            };
            assert_eq!(adopted, 0);
            settings["new_document"] = app.state()["settings"]["new_document"].clone();
            assert_eq!(app.state()["settings"], settings);
            app.draw_until_idle();
            let baseline = app.pixels();
            let transform = app.borrow()
                .host
                .session
                .state()
                .camera
                .input_transform();
            let ahead = transform.map(layer_core::Point { x: 720., y: 450. });
            let width = app.document().composition().size[0] as usize;
            let offset = (ahead.y as usize * width + ahead.x as usize) * 4;
            let send = |record: &[f64], predicted| {
                assert_eq!(
                    unsafe {
                        capy_apple_pointer(
                            app.0,
                            1,
                            0,
                            0,
                            record.as_ptr(),
                            record.len(),
                            predicted,
                            capy_apple_camera_revision(app.0),
                            0,
                        )
                    },
                    0
                );
            };
            let frame = |time: f64| {
                app.borrow()
                    .host
                    .prepare_canvas_frame(time as u64, time as u64 + 8_000_000, true)
                    .unwrap()
            };
            let mut record = [500., 450., 0.8, 0., 0., 0., 0., 0., 1.];
            for index in 0..101 {
                record[0] = 500. + f64::from(index) * 2.;
                record[7] = 3_000_000_000. + f64::from(index) * 5_000_000.;
                record[8] = if index == 0 { 1. } else { 2. };
                send(&record, 0);
                let mut predicted = record;
                predicted[0] += 3.2;
                predicted[1] -= 4.;
                predicted[7] += 8_000_000.;
                predicted[8] = 2.;
                send(&predicted, 1);
                frame(record[7]);
            }
            let metrics = app.borrow().host.session.engine().metrics();
            assert_eq!(metrics.platform_prediction_frames > 0, platform == 0);
            assert_eq!(metrics.engine_prediction_frames > 0, platform == 1);
            let live = app.pixels();
            let pixel = &live[offset..offset + 4];
            eprintln!(
                "adopt platform={platform} saved=64ms: ahead={pixel:?}, native={}, engine={}",
                metrics.platform_prediction_frames, metrics.engine_prediction_frames
            );
            assert_eq!(
                i16::from(pixel[2]) > i16::from(pixel[0]) + 50,
                platform == 1
            );
            record[7] += 1_000_000.;
            record[8] = 3.;
            send(&record, 0);
            frame(record[7]);
            let committed = app.pixels();
            assert_eq!(
                &committed[offset..offset + 4],
                &baseline[offset..offset + 4]
            );
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), baseline);
            app.invoke("redo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), committed);
        }
    }
}

#[test]
fn lasso_pointer_contacts_preserve_history_and_paint_enclosed_pixels_on_both_platforms() {
    for platform in [0, 1] {
        for tool in ["select", "lasso_fill"] {
            let app = App::new(platform);
            app.borrow().host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            app.action(json!({"type":"set_color","rgba":[0.2,0.45,0.8,1]}));
            app.layer_action(json!({"op":"new","group":false,"clipped":false}));
            app.invoke("undo");
            app.layer_action(json!({"op":"tool","tool":tool}));
            app.draw_until_idle();
            let baseline = app.pixels();
            let (selection,width,transform)=app.with(|native| {
                let session=&native.host.session;
                (session.engine().document().working.selection.clone(),session.engine().document().composition().size[0] as usize,session.state().camera.input_transform())
            });
            let a = transform.map(layer_core::Point { x: 500., y: 400. });
            let b = transform.map(layer_core::Point { x: 650., y: 550. });
            let send = |id: u64, points: &[[f64; 3]]| {
                let records: Vec<_> = points
                    .iter()
                    .enumerate()
                    .flat_map(|(i, &[x, y, phase])| {
                        [
                            x,
                            y,
                            1.,
                            0.,
                            0.,
                            0.,
                            0.,
                            (1_000_000_000 + id * 100_000_000 + i as u64 * 10_000_000) as f64,
                            phase,
                        ]
                    })
                    .collect();
                assert_eq!(
                    unsafe {
                        capy_apple_pointer(
                            app.0,
                            id,
                            0,
                            0,
                            records.as_ptr(),
                            records.len(),
                            0,
                            capy_apple_camera_revision(app.0),
                            0,
                        )
                    },
                    0
                );
                app.draw_until_idle();
                assert!(app.state()["host_error"].is_null());
            };
            assert!(app.borrow().host.session.engine().can_redo());
            for (id, points) in [
                (1, vec![[540., 440., 1.], [540., 440., 3.]]),
                (
                    2,
                    vec![
                        [540., 440., 1.],
                        [540., 440., 2.],
                        [540., 440., 2.],
                        [540., 440., 3.],
                    ],
                ),
                (
                    3,
                    vec![
                        [500., 400., 1.],
                        [650., 400., 2.],
                        [650., 550., 2.],
                        [500., 550., 4.],
                    ],
                ),
            ] {
                send(id, &points);
                app.with(|native| {
                let session = &native.host.session;
                assert_eq!(session.engine().document().working.selection, selection);
                assert!(
                    session.engine().can_redo(),
                    "Empty/cancelled contacts preserve Redo"
                );
                session.require_document_snapshot_idle().unwrap();
                });
                assert_eq!(app.pixels(), baseline);
            }
            send(
                4,
                &[
                    [500., 400., 1.],
                    [650., 400., 2.],
                    [650., 550., 2.],
                    [500., 550., 2.],
                    [500., 400., 3.],
                ],
            );
            let before_fill = if tool == "select" {
                assert_ne!(
                    app.document()
                        .working.selection,
                    selection
                );
                let pixels = app.pixels();
                app.invoke("fill_selection");
                app.draw_until_idle();
                pixels
            } else {
                baseline.clone()
            };
            let painted = app.pixels();
            // Readback contains document pixels, not the 1200-by-900 viewport.
            let center = (((a.y + b.y) * 0.5) as usize * width + ((a.x + b.x) * 0.5) as usize) * 4;
            assert!(
                i16::from(painted[center + 2]) > i16::from(painted[center]) + 50,
                "The enclosed area must be blue"
            );
            let xs = a.x.min(b.x) - 2. ..=a.x.max(b.x) + 2.;
            let ys = a.y.min(b.y) - 2. ..=a.y.max(b.y) + 2.;
            assert!(
                painted
                    .as_chunks::<4>().0.iter()
                    .zip(before_fill.as_chunks::<4>().0.iter())
                    .enumerate()
                    .all(|(i, (a, b))| {
                        let (x, y) = ((i % width) as f32, (i / width) as f32);
                        xs.contains(&x) && ys.contains(&y) || a == b
                    }),
                "Pixels outside the lasso must remain unchanged"
            );
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), before_fill);
            app.invoke("redo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), painted);
        }
    }
}

fn assert_sensor_pixels(actual: &[u8], expected: &[u8], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}");
    // Corrected contacts can use different GPU batch boundaries. Their sRGB8
    // output may differ by one encoding level; compare every byte without
    // changing the exact Undo/Redo checks within either rendering.
    assert!(
        actual == expected || actual.iter().zip(expected).all(|(&a, &b)| a.abs_diff(b) <= 1),
        "Sensor correction exceeds one encoding level: {context}"
    );
}

#[test]
fn estimated_input_abi_matches_final_sensor_oracle_pixels_and_history_on_both_platforms() {
    use layer_core::DefaultBrushPreset as Brush;
    for platform in [0, 1] {
        for brush in [
            Brush::GPen,
            Brush::Pencil,
            Brush::WatercolorWash,
            Brush::Smudge,
        ] {
            let apps = [App::new(platform), App::new(platform)];
            let mut results = Vec::new();
            for (estimated, app) in apps.iter().enumerate() {
                app.borrow().host.session.renderer_mut().0 = Some(
                    native_renderer(),
                );
                app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
                app.draw_frame();
                app.stroke();
                app.draw_until_idle();
                let baseline = app.pixels();
                app.action(json!({"type":"select_brush","id":brush as u32}));
                app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
                app.action(json!({"type":"set_color","rgba":[0.8,0.15,0.2,1]}));
                app.draw_frame();
                let revision = unsafe { capy_apple_camera_revision(app.0) };
                let mut final_sample = [540., 440., 0.85, 0.4, -0.25, 1.6, 0., 2_000_000_000., 1.];
                let mut initial = final_sample;
                if estimated == 1 {
                    initial[2] = 0.15;
                    initial[3] = 0.;
                    initial[5] = 0.;
                }
                let send = |record: &[f64], metadata: &[u64], correction| {
                    assert_eq!(
                        unsafe {
                            capy_apple_pointer_updates(
                                app.0,
                                9,
                                0,
                                0,
                                record.as_ptr(),
                                record.len(),
                                metadata.as_ptr(),
                                correction,
                                revision,
                                0,
                            )
                        },
                        0
                    );
                };
                send(&initial, &[1, estimated as u64], 0);
                app.draw_frame();
                let mut moved = final_sample;
                moved[0] += 45.;
                moved[1] += 20.;
                // Cross the bounded sensor wait before correcting the down
                // sample, so even stateful brushes must rebuild persistent ink.
                moved[7] += 80_000_000.;
                moved[8] = 2.;
                send(&moved, &[0, 0], 0);
                app.draw_frame();
                if estimated == 1 {
                    let mut partial = final_sample;
                    partial[2] = 0.65;
                    send(&partial, &[1, 1], 1);
                    app.draw_frame();
                }
                let mut up = moved;
                up[0] += 30.;
                up[7] += 10_000_000.;
                up[8] = 3.;
                send(&up, &[0, 0], 0);
                app.draw_frame();
                if estimated == 1 {
                    // The old down phase is metadata, never a new contact.
                    send(&final_sample, &[1, 0], 1);
                    app.draw_frame();
                }
                app.draw_until_idle();
                let ink = app.pixels();
                assert_ne!(
                    ink, baseline,
                    "fixture must change artwork: {platform}/{brush:?}"
                );
                assert_eq!(app.borrow().host.session.engine().metrics().committed_strokes,2);
                // Completed contacts are stored as immutable raster revisions.
                // Compare backing as well as composited pixels against
                // the contact delivered with final pressure, tilt and twist.
                let document = app.document();
                let samples =
                    raster_samples(document.target_raster(document.active_target().unwrap()).unwrap());
                app.invoke("undo");
                app.draw_until_idle();
                assert_eq!(app.pixels(), baseline);
                app.invoke("redo");
                app.draw_until_idle();
                assert_eq!(app.pixels(), ink);
                final_sample[2] = 0.1;
                send(&final_sample, &[1, 0], 1);
                app.draw_frame();
                assert_eq!(
                    app.pixels(),
                    ink,
                    "unregistered/finished updates cannot start painting"
                );
                results.push((ink, samples));
            }
            let context = format!("{platform}/{brush:?}");
            let (actual, expected) = (&results[0].1, &results[1].1);
            assert_eq!(actual.0.len(), expected.0.len(), "Raster tile count: {context}");
            assert_eq!(actual.1, expected.1, "Watercolor state: {context}");
            for (key, (descriptor, bytes)) in &actual.0 {
                let (expected_descriptor, expected_bytes) = &expected.0[key];
                assert_eq!(descriptor, expected_descriptor, "Raster format: {context}");
                assert_sensor_pixels(bytes, expected_bytes, &context);
            }
            assert_sensor_pixels(&results[0].0, &results[1].0, &context);
        }
    }
}

#[test]
fn apple_stroke_recordings_capture_hover_and_contacts_and_export_compressed() {
    use std::io::{Read as _, Seek as _};
    use std::os::fd::AsRawFd;
    for (platform, name) in [(0, "ios"), (1, "mac")] {
        let app = App::new(platform);
        app.borrow().host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let status = |action: Value| app.request(2, json!({"type":"stroke_recording","action":action})).unwrap();
        assert_eq!(status(Value::Null)["label"], "Start stroke recording");
        assert_eq!(status(json!("start"))["recording"], true);
        let handle = app.0;
        let send = |id: u64, points: &[(f64, f64)]| {
            let records: Vec<f64> = points.iter().enumerate().flat_map(|(i, &(x, phase))| {
                [x, 300., if phase == 0. { 0. } else { 0.6 }, 0., 0., 0., 0., 3_000_000_000. + i as f64 * 5_000_000., phase]
            }).collect();
            assert_eq!(unsafe { capy_apple_pointer(handle, id, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(handle), 0) }, 0);
        };
        send(5, &[(400., 0.)]);
        send(5, &[(420., 1.), (440., 2.), (460., 2.), (460., 3.)]);
        app.draw_until_idle();
        let stopped = status(json!("stop"));
        assert_eq!((stopped["recording"].as_bool(), stopped["ready"].as_bool()), (Some(false), Some(true)));
        assert!(stopped["raw_events"].as_u64().unwrap() >= 4);
        let bytes=capy_apple_stroke_recording_data(handle);
        let length=capy_bytes_length(bytes);
        let raw=copied_bytes(length,|offset,output,count|unsafe {capy_bytes_copy(bytes,offset,output,count)});
        assert!(!(bytes==0) && length > 0);
        let path = std::env::temp_dir().join(format!("capy-recording-{}-{name}", std::process::id()));
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!((unsafe { capy_stroke_recording_write(raw.as_ptr(), raw.len(), file.as_raw_fd()) }==0));
        capy_bytes_free(bytes);
        file.rewind().unwrap();
        let mut compressed = Vec::new();
        file.read_to_end(&mut compressed).unwrap();
        assert!(compressed.starts_with(layer_engine::recording::MAGIC));
        let records = layer_engine::recording::read(&compressed[..]).unwrap();
        use layer_engine::recording::Record;
        assert!(matches!(&records[0], Record::Metadata(text) if text.contains(name)), "{:?}", records[0]);
        assert!(records.iter().filter(|r| matches!(r, Record::Raw { .. })).count() >= 4);
        assert!(records.iter().any(|r| matches!(r, Record::Begin { .. })));
        assert!(records.iter().any(|r| matches!(r, Record::End { cancelled: false, .. })));
        assert_eq!(status(json!("saved"))["label"], "Start stroke recording");
    }
}

#[test]
fn ipad_touch_timing_and_pencil_actions_reach_the_session() {
    let app = App::new(0);
    app.request(2, json!({"type":"touch_policy","tap_ms":500,"slop":20.}));
    assert!(app.try_request(2, &json!({"type":"touch_policy","tap_ms":0,"slop":20.})).is_err());
    let reply = app.request(1, json!({"type":"stylus_action","action":"switch_eraser"})).unwrap();
    assert_eq!(reply["handled"], true);
    assert!(app.borrow().host.session.command(layer_ui::CommandId::Eraser).selected);
}

#[test]
fn control_activation_uses_keyboard_arrival() {
    for (key, keyboard_focus, expected) in [("Space", false, false), ("Space", true, true), ("Enter", false, true)] {
        let key = std::ffi::CString::new(key).unwrap();
        unsafe {
            assert_eq!(capy_apple_control_takes_key(key.as_ptr(), false, false, keyboard_focus), expected);
            assert!(!capy_apple_control_takes_key(key.as_ptr(), true, false, keyboard_focus));
            assert!(!capy_apple_control_takes_key(key.as_ptr(), false, true, keyboard_focus));
        }
    }
}

#[test]
fn shortcut_predicate_uses_shared_layout_identity() {
    for (key, latin, expected, matches) in [("я", "z", "z", true), ("Я", "Z", "z", true),
        ("й", "q", "q", true), ("и", "b", "b", true), ("é", "2", "2", true),
        ("a", "q", "a", true), ("a", "q", "q", false), ("я", "", "z", false), ("Я", "", "я", true)] {
        let key = std::ffi::CString::new(key).unwrap();
        let latin = std::ffi::CString::new(latin).unwrap();
        let expected = std::ffi::CString::new(expected).unwrap();
        assert_eq!(unsafe { capy_apple_shortcut_key_is(key.as_ptr(), latin.as_ptr(), expected.as_ptr()) }, matches);
    }
    let key = std::ffi::CString::new("Escape").unwrap();
    assert!(unsafe { capy_apple_shortcut_key_is(key.as_ptr(), std::ptr::null(), key.as_ptr()) });
}

#[test]
fn apple_mixing_and_retouch_presets_change_local_pixels_with_exact_history() {
    use layer_core::DefaultBrushPreset as Brush;
    for platform in [0, 1] {
        let app = fixtures::drawing_app(platform, [128, 96]);
        app.action(json!({"type":"set_color","rgba":[0.8,0.2,0.12,1]}));
        app.invoke("select_all"); app.invoke("fill_selection"); app.invoke("deselect");
        app.draw_until_idle();
        let paper = app.pixels();
        app.action(json!({"type":"select_brush","id":Brush::GPen as u32}));
        app.action(json!({"type":"set_brush_size","value":8}));
        app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
        app.action(json!({"type":"set_color","rgba":[0.04,0.15,0.85,1]}));
        fixtures::drag(&app, 1, 0, [64.,48.], [64.,48.]);
        let baseline = app.pixels();
        assert_ne!(baseline, paper, "The seed must contain a painted blue blemish on red");
        for (index, preset) in [Brush::BlendingStump, Brush::Tissue, Brush::DitherBlender,
            Brush::ScatterBlender, Brush::CloneStamp, Brush::HealingBrush, Brush::SpotHealingBrush]
            .into_iter().enumerate() {
            app.action(json!({"type":"select_brush","id":preset as u32}));
            app.action(json!({"type":"set_brush_size","value":24}));
            app.action(json!({"type":"set_brush_strength","value":1}));
            app.action(json!({"type":"set_tool_setting","id":"smoothing_strength","value":0}));
            match preset {
                Brush::DitherBlender => {
                    app.action(json!({"type":"set_tool_setting","id":"spread","value":0.5}));
                    app.action(json!({"type":"set_tool_setting","id":"dither_cell_size","value":6}));
                }
                Brush::ScatterBlender => {
                    app.action(json!({"type":"set_tool_setting","id":"spread","value":0.45}));
                    app.action(json!({"type":"set_tool_setting","id":"scatter_cell_size","value":5}));
                }
                Brush::CloneStamp | Brush::HealingBrush | Brush::SpotHealingBrush => {
                    app.invoke("selection_editing");
                    if preset != Brush::SpotHealingBrush {
                        app.invoke("clone_source_arm");
                        fixtures::drag(&app, 10 + index as u64 * 2, 0, [24.,48.], [24.,48.]);
                    }
                }
                _ => {}
            }
            app.draw_until_idle();
            assert_eq!(app.pixels(), baseline, "{platform} {preset:?}: configuring a tool cannot paint");
            fixtures::drag(&app, 11 + index as u64 * 2, 0, [58.,48.], [70.,48.]);
            let changed = app.pixels();
            assert_eq!(changed.len(), 128 * 96 * 4);
            let mut inside = 0;
            for (pixel, (actual, previous)) in changed.as_chunks::<4>().0.iter()
                .zip(baseline.as_chunks::<4>().0).enumerate() {
                let (x, y) = (pixel % 128, pixel / 128);
                if (34..94).contains(&x) && (18..78).contains(&y) {
                    inside += usize::from(actual != previous);
                } else {
                    assert_eq!(actual, previous, "{platform} {preset:?}: untouched exterior at {x},{y}");
                }
            }
            assert!(inside > 0, "{platform} {preset:?}: the stroke must change the seeded patch");
            assert!(app.state()["host_error"].is_null());
            app.invoke("undo"); app.draw_until_idle();
            assert_eq!(app.pixels(), baseline, "{platform} {preset:?}: one Undo restores every seed pixel");
            app.invoke("redo"); app.draw_until_idle();
            assert_eq!(app.pixels(), changed, "{platform} {preset:?}: Redo restores the exact painted result");
            app.invoke("undo"); app.draw_until_idle();
            assert_eq!(app.pixels(), baseline, "{platform} {preset:?}: each preset starts from the same artwork");
        }
    }
}

#[test]
fn color_wheel_contacts_taps_and_menu_step_each_paint() {
    for platform in [0, 1] {
        let app = App::new(platform);
        let foreground = |app: &App| app.state()["colors"]["foreground"].clone();
        let original = foreground(&app);
        app.action(json!({"type":"color_contact","phase":"down"}));
        for point in [[140., 70.], [150., 80.]] {
            app.action(json!({"type":"color","action":{"op":"pick_wheel","part":"field","point":point,"size":200.}}));
        }
        app.action(json!({"type":"color_contact","phase":"up"}));
        let picked = foreground(&app);
        assert_ne!(picked, original);
        let touch = |id: u64, phase: &str, x: f64, ms: u64| app.request(1, json!({"type":"color_touch","surface":7,"id":id,
            "phase":phase,"position":[x, 40.],"time_ns":ms * 1_000_000})).unwrap();
        assert_eq!(touch(1, "down", 20., 1)["handled"], false);
        assert_eq!(touch(2, "down", 80., 2)["handled"], true);
        touch(1, "up", 20., 40);
        touch(2, "up", 80., 41);
        assert_eq!(foreground(&app), original, "a two-finger tap undoes the whole contact");
        let menu = app.request(2, json!({"type":"context","target":{"kind":"color_wheel"}})).unwrap();
        let [undo, redo] = [&menu["sections"][0][0], &menu["sections"][0][1]];
        assert_eq!((&undo["enabled"], &redo["enabled"]), (&json!(false), &json!(true)));
        app.action(redo["action"].clone());
        assert_eq!(foreground(&app), picked, "the wheel menu redoes");
        app.action(json!({"type":"color_contact","phase":"down"}));
        app.action(json!({"type":"color","action":{"op":"pick_wheel","part":"hue","point":[100., 4.],"size":200.}}));
        app.action(json!({"type":"color_contact","phase":"cancel"}));
        assert_eq!(foreground(&app), picked, "a cancelled contact restores its paint");
    }
}
