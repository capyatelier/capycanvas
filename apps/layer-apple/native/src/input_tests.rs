use super::*;

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
