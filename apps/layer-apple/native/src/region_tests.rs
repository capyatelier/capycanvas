//! Actual Apple pointer/action routing and Metal region results; no native UI.
use super::*;

fn region_app(platform: u32, gap: bool) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 =
        Some(native_renderer());
    app.draw_until_idle();
    let project = ProjectJob::new(&app, true);
    assert_eq!(unsafe { capy_project_new(project.0, 64, 64) }, 0);
    assert_eq!(
        unsafe {
            capy_apple_project_adopt(app.0, project.0, c"Region check".as_ptr(), c"".as_ptr())
        },
        0
    );
    let pixels: Vec<u8> = (0..64)
        .flat_map(|y| {
            (0..64).flat_map(move |x| {
                let wall = (16..48).contains(&x)
                    && (16..48).contains(&y)
                    && (!(20..44).contains(&x) || !(20..44).contains(&y))
                    && !(gap && (31..33).contains(&x) && y < 20);
                if wall { [0, 0, 0, 255] } else { [255; 4] }
            })
        })
        .collect();
    assert_eq!(
        unsafe {
            capy_apple_import_layer(
                app.0,
                app.state()["document_file"]["epoch"].as_u64().unwrap(),
                c"Outline".as_ptr(),
                64,
                64,
                pixels.as_ptr(),
                pixels.len(),
            )
        },
        0
    );
    app.action(json!({"type":"set_color","rgba":[0,0,1,1]}));
    app.draw_until_idle();
    app
}

fn contact(app: &App, id: u64, device: u32, phases: &[f64]) {
    let m = unsafe { &*app.0 }
        .host
        .session
        .state()
        .camera
        .document_to_surface();
    let [x, y] = [
        m[0] * 32.5 + m[2] * 32.5 + m[4],
        m[1] * 32.5 + m[3] * 32.5 + m[5],
    ];
    let records: Vec<_> = phases
        .iter()
        .enumerate()
        .flat_map(|(i, &phase)| {
            [
                x as f64,
                y as f64,
                1.,
                0.,
                0.,
                0.,
                0.,
                (1_000_000_000 + id * 1_000_000 + i as u64 * 100_000) as f64,
                phase,
            ]
        })
        .collect();
    assert_eq!(
        unsafe {
            capy_apple_pointer(
                app.0,
                id,
                device,
                0,
                records.as_ptr(),
                records.len(),
                0,
                capy_apple_camera_revision(app.0),
            )
        },
        0
    );
}

fn setting(app: &App, id: &str, value: f32) {
    app.action(json!({"type":"set_tool_setting","id":id,"value":value}));
}

#[test]
fn apple_region_refinement_changes_coverage_and_restores_exact_history() {
    for platform in [0, 1] {
        let app = region_app(platform, false);
        let baseline = app.pixels();
        let mut id = 0;
        for device in [0, 1] {
            for tool in ["auto_select", "fill"] {
                app.invoke(tool);
                setting(&app, "tolerance", 0.);
                for (expansion, smoothing) in [(0, 0.), (2, 0.), (-2, 0.), (0, 1.)] {
                    setting(&app, "expansion", expansion as f32);
                    setting(&app, "smoothing", smoothing);
                    id += 1;
                    contact(&app, id, device, &[1., 3.]);
                    app.draw_until_idle();
                    let selection = unsafe { &*app.0 }
                        .host
                        .session
                        .engine()
                        .document()
                        .selection
                        .clone();
                    if tool == "auto_select" {
                        let layer_core::SelectionShape::Pixels(mask) = &selection
                            .as_ref()
                            .expect("Selection from Apple contact")
                            .shape
                        else {
                            panic!("Auto select must retain the GPU-produced coverage");
                        };
                        assert_eq!(mask.extent(), [64, 64]);
                        assert_eq!(
                            mask.bounds(),
                            [
                                (20 - expansion) as u32,
                                (20 - expansion) as u32,
                                (44 + expansion) as u32,
                                (44 + expansion) as u32
                            ]
                        );
                        app.invoke("fill_selection");
                        app.draw_until_idle();
                    }
                    let painted = app.pixels();
                    let mut softened = 0;
                    for (i, (actual, before)) in painted
                        .chunks_exact(4)
                        .zip(baseline.chunks_exact(4))
                        .enumerate()
                    {
                        let (x, y) = ((i % 64) as i32, (i / 64) as i32);
                        let inside = (20 - expansion..44 + expansion).contains(&x)
                            && (20 - expansion..44 + expansion).contains(&y);
                        if !inside {
                            assert_eq!(actual, before, "Outside region at {x},{y}");
                        } else if smoothing == 0. {
                            assert_eq!(actual, [0, 0, 255, 255], "Inside region at {x},{y}");
                        } else {
                            assert_eq!(&actual[2..], &[255, 255]);
                            softened += usize::from(actual[0] > 0 && actual[0] < 255);
                        }
                    }
                    if smoothing != 0. {
                        assert_eq!(
                            softened, 4,
                            "Only the four square corners need softened coverage"
                        );
                    }
                    for (command, expected) in
                        [("undo", &baseline), ("redo", &painted), ("undo", &baseline)]
                    {
                        app.invoke(command);
                        app.draw_until_idle();
                        assert_eq!(app.pixels(), *expected);
                    }
                    if tool == "auto_select" {
                        app.invoke("undo");
                        app.draw_until_idle();
                        assert!(
                            unsafe { &*app.0 }
                                .host
                                .session
                                .engine()
                                .document()
                                .selection
                                .is_none()
                        );
                        app.invoke("redo");
                        app.draw_until_idle();
                        assert_eq!(
                            unsafe { &*app.0 }
                                .host
                                .session
                                .engine()
                                .document()
                                .selection,
                            selection
                        );
                        app.invoke("undo");
                        app.draw_until_idle();
                    }
                }
            }
        }
    }
}

#[test]
fn apple_region_gap_closing_and_cancelled_requests_preserve_artwork() {
    for platform in [0, 1] {
        let app = region_app(platform, true);
        let baseline = app.pixels();
        for (id, gap) in [(1, 0), (2, 4)] {
            app.invoke("fill");
            setting(&app, "gap_closing", gap as f32);
            setting(&app, "smoothing", 0.);
            contact(&app, id, 0, &[1., 3.]);
            app.draw_until_idle();
            let painted = app.pixels();
            assert_eq!(&painted[(32 * 64 + 32) * 4..][..4], [0, 0, 255, 255]);
            assert_eq!(
                &painted[..4],
                if gap == 0 {
                    &[0, 0, 255, 255]
                } else {
                    &[255; 4]
                },
                "Closing the two-pixel break keeps paint inside the outline"
            );
            app.invoke("undo");
            app.draw_until_idle();
            assert_eq!(app.pixels(), baseline);
        }
        let mut id = 2;
        for tool in ["auto_select", "fill"] {
            for cancel in ["contact", "escape", "blur", "tool"] {
                app.invoke(tool);
                assert!(unsafe { &*app.0 }.host.session.engine().can_redo());
                id += 1;
                contact(&app, id, 0, &[1.]);
                match cancel {
                    "contact" => contact(&app, id, 0, &[4.]),
                    "escape" => {
                        for pressed in [true, false] {
                            app.request(1, json!({"type":"key","key":"Escape","pressed":pressed}));
                        }
                    }
                    "blur" => {
                        app.request(1, json!({"type":"blur"}));
                    }
                    "tool" => app.invoke("hand"),
                    _ => unreachable!(),
                }
                contact(&app, id, 0, &[3.]);
                app.draw_until_idle();
                assert_eq!(app.pixels(), baseline, "{tool}/{cancel} must not paint");
                let session = &unsafe { &*app.0 }.host.session;
                assert!(session.engine().document().selection.is_none());
                assert!(
                    session.engine().can_redo(),
                    "{tool}/{cancel} must preserve Redo"
                );
                session.require_document_snapshot_idle().unwrap();
            }
            // Release queues asynchronous detection. Cancellation must also
            // discard its later result, without changing the existing Redo.
            for cancel in ["escape", "blur", "tool", "setting"] {
                app.invoke(tool);
                id += 1;
                contact(&app, id, 0, &[1., 3.]);
                app.draw_frame();
                let session = &unsafe { &*app.0 }.host.session;
                assert!(
                    session.require_document_snapshot_idle().is_err(),
                    "Region processing must still block a document snapshot"
                );
                assert!(session.engine().document().selection.is_none());
                match cancel {
                    "escape" => {
                        for pressed in [true, false] {
                            app.request(1, json!({"type":"key","key":"Escape","pressed":pressed}));
                        }
                    }
                    "blur" => {
                        app.request(1, json!({"type":"blur"}));
                    }
                    "tool" => app.invoke("hand"),
                    "setting" => setting(&app, "tolerance", 0.2),
                    _ => unreachable!(),
                }
                app.draw_until_idle();
                assert_eq!(
                    app.pixels(),
                    baseline,
                    "The stale {tool}/{cancel} result must not paint"
                );
                let session = &unsafe { &*app.0 }.host.session;
                assert!(session.engine().document().selection.is_none());
                assert!(session.engine().can_redo());
                session.require_document_snapshot_idle().unwrap();
            }
        }
    }
}
