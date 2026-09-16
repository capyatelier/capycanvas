use super::*;

#[test]
fn lasso_pointer_contacts_preserve_history_and_paint_enclosed_pixels_on_both_platforms() {
    for platform in [0, 1] {
        for tool in ["select", "lasso_fill"] {
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(
                layer_render_wgpu::WgpuRasterizer::new_headless().expect("Hardware Metal required"),
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
                    layer_render_wgpu::WgpuRasterizer::new_headless()
                        .expect("Hardware Metal required"),
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
                moved[7] += 10_000_000.;
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
