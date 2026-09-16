use super::*;

#[test]
fn mac_manual_prediction_paints_ahead_of_pen_and_mouse_without_committing_the_tip() {
    for tool in [0, 1] {
        for milliseconds in [0, 64] {
            let app = App::new(1);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            app.action(json!({"type":"select_brush","id":layer_core::DefaultBrushPreset::GPen as u32}));
            app.action(json!({"type":"set_brush_size","value":4}));
            app.action(json!({"type":"set_color","rgba":[0,0,1,1]}));
            app.invoke("settings");
            app.action(json!({"type":"preferences","action":{"type":"edit","id":"prediction_horizon","value":milliseconds}}));
            app.action(json!({"type":"close_settings"}));
            app.draw_until_idle();
            let baseline = app.pixels();
            let transform = unsafe { &*app.0 }.host.session.state().camera.input_transform();
            let ahead = transform.map(layer_core::Point { x: 720., y: 450. });
            let width = unsafe { &*app.0 }.host.session.engine().document().width as usize;
            let offset = (ahead.y as usize * width + ahead.x as usize) * 4;
            let mut record = [500., 450., 0.8, 0., 0., 0., 0., 0., 1.];
            let handle = app.0;
            let send = move |record: &[f64]| {
                assert_eq!(unsafe { capy_apple_pointer(handle, 1, tool, 0,
                    record.as_ptr(), record.len(), 0, capy_apple_camera_revision(handle)) }, 0);
                unsafe { &mut *handle }.host.prepare_canvas_frame(
                    record[7] as u64, record[7] as u64 + 8_000_000, true).unwrap();
            };
            for index in 0..101 {
                record[0] = 500. + f64::from(index) * 2.;
                record[7] = 3_000_000_000. + f64::from(index) * 5_000_000.;
                record[8] = if index == 0 { 1. } else { 2. };
                send(&record);
            }
            let cursor = unsafe { &mut *app.0 }.host.session.canvas_cursor().unwrap();
            assert_eq!(cursor.center, [700., 450.]);
            let live = app.pixels();
            let pixel = &live[offset..offset + 4];
            let blue = i16::from(pixel[2]) > i16::from(pixel[0]) + 50;
            eprintln!("Mac tool={tool} prediction={milliseconds}ms: pixel 20px ahead of cursor={pixel:?}");
            assert_eq!(blue, milliseconds == 64, "Manual prediction must reach the rendered preview");
            record[7] += 1_000_000.;
            record[8] = 3.;
            send(&record);
            let committed = app.pixels();
            assert_eq!(&committed[offset..offset + 4], &baseline[offset..offset + 4],
                "Pen-up must remove predicted ink beyond the real endpoint");
            assert_eq!(unsafe { &*app.0 }.host.session.engine().metrics().committed_strokes, 1);
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
        for recovered in [false, true] {
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            let mut settings = app.state()["settings"].clone();
            settings["platform_prediction"] = json!(true);
            settings["prediction_ms"] = json!(64.);
            app.action(json!({"type":"restore_settings","settings":settings}));
            app.action(json!({"type":"set_brush_size","value":4}));
            app.action(json!({"type":"set_color","rgba":[0,0,1,1]}));
            app.draw_until_idle();
            let project = ProjectJob::new(&app, true);
            assert_eq!(unsafe { capy_project_new(project.0, 2048, 1536) }, 0);
            let adopted = unsafe {
                if recovered {
                    capy_apple_project_recover(app.0, project.0)
                } else {
                    capy_apple_project_adopt(app.0, project.0, c"Untitled".as_ptr(), c"".as_ptr())
                }
            };
            assert_eq!(adopted, 0);
            assert_eq!(app.state()["settings"], settings);
            app.draw_until_idle();
            let baseline = app.pixels();
            let transform = unsafe { &*app.0 }
                .host
                .session
                .state()
                .camera
                .input_transform();
            let ahead = transform.map(layer_core::Point { x: 720., y: 450. });
            let width = unsafe { &*app.0 }.host.session.engine().document().width as usize;
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
                unsafe { &mut *app.0 }
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
            let metrics = unsafe { &*app.0 }.host.session.engine().metrics();
            assert_eq!(metrics.platform_prediction_frames > 0, platform == 0);
            assert_eq!(metrics.engine_prediction_frames > 0, platform == 1);
            let live = app.pixels();
            let pixel = &live[offset..offset + 4];
            eprintln!(
                "adopt platform={platform} recovered={recovered} saved=64ms: ahead={pixel:?}, native={}, engine={}",
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
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                native_renderer(),
            );
            app.action(json!({"type":"set_color","rgba":[0.2,0.45,0.8,1]}));
            app.layer_action(json!({"op":"new","group":false,"clipped":false}));
            app.invoke("undo");
            app.layer_action(json!({"op":"tool","tool":tool}));
            app.draw_until_idle();
            let baseline = app.pixels();
            let session = &unsafe { &*app.0 }.host.session;
            let selection = session.engine().document().selection.clone();
            let width = session.engine().document().width as usize;
            let transform = session.state().camera.input_transform();
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
            assert!(unsafe { &*app.0 }.host.session.engine().can_redo());
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
                let session = &unsafe { &*app.0 }.host.session;
                assert_eq!(session.engine().document().selection, selection);
                assert!(
                    session.engine().can_redo(),
                    "Empty/cancelled contacts preserve Redo"
                );
                session.require_document_snapshot_idle().unwrap();
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
                    unsafe { &*app.0 }
                        .host
                        .session
                        .engine()
                        .document()
                        .selection,
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
                    .chunks_exact(4)
                    .zip(before_fill.chunks_exact(4))
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
                unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                    native_renderer(),
                );
                app.draw_frame();
                app.stroke();
                app.draw_until_idle();
                let baseline = app.pixels();
                app.action(json!({"type":"select_brush","id":brush as u32}));
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
                let engine = unsafe { &*app.0 }.host.session.engine();
                assert_eq!(engine.metrics().committed_strokes, 2);
                // Completed contacts are stored as immutable raster revisions.
                // Compare backing as well as composited pixels against
                // the contact delivered with final pressure, tilt and twist.
                let document = engine.document();
                let samples =
                    raster_samples(document.target_raster(document.active_target()).unwrap());
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
