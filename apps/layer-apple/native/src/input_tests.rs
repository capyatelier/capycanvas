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
            assert_eq!(unsafe {capy_apple_pointer(app.0,42,0,0,sample.as_ptr(),sample.len(),0,capy_apple_camera_revision(app.0))},0);
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
                    capy_apple_camera_revision(app.0))
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
                    record.as_ptr(), record.len(), 0, capy_apple_camera_revision(handle)) }, 0);
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
            assert_eq!(unsafe { capy_apple_pointer(handle, id, 1, 0, records.as_ptr(), records.len(), 0, capy_apple_camera_revision(handle)) }, 0);
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
