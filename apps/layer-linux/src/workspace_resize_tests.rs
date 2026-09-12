//! Width changes observed after GTK paint, paired with completed presentation feedback.
use super::*;

#[test]
#[ignore = "isolated Mutter and native-input.js --workspace-resize"]
fn native_workspace_resize_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.WorkspaceResize");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut perform = |events: serde_json::Value| {
        let path = dir.join(format!("step-{step}.json"));
        std::fs::write(
            path.with_extension("tmp"),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        std::fs::rename(path.with_extension("tmp"), path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native resize timed out");
            pump(2);
        }
        step += 1;
    };
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let mut reports = Vec::new();
    for touch in [false, true] {
        for scenario in ["left", "right", "navigator"] {
            let mut fixture = layer_ui::WorkspaceState::default();
            let edge = if scenario == "left" {
                Edge::Left
            } else {
                Edge::Right
            };
            let band = fixture
                .layout
                .bands
                .iter_mut()
                .find(|b| b.edge == edge)
                .unwrap();
            band.extent = if scenario == "left" { 252. } else { 310. };
            let divider = band.id;
            if scenario == "navigator" {
                if let DockNode::Tabs { panels, active, .. } = &mut band.root {
                    panels.push(Panel::Navigator);
                    *active = Panel::Navigator;
                }
            }
            w.dispatch(UiAction::RestoreWorkspace { workspace: fixture });
            pump(350);
            let panel = if scenario == "left" {
                Panel::Brushes
            } else if scenario == "navigator" {
                Panel::Navigator
            } else {
                Panel::Layers
            };
            let group = state(&w).workspace.layout.panel_group(panel).unwrap();
            let view = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .root
                .clone();
            let content = w.panel_widget(panel);
            let b = w
                .resolved()
                .dividers
                .iter()
                .find(|d| d.id == divider && d.band)
                .unwrap()
                .bounds;
            let start = [b.x + b.width * 0.5, b.y + b.height * 0.5];
            let origin = [
                start[0] + if scenario == "left" { 20. } else { -20. },
                start[1],
            ];
            let event = |phase: &str, point: [f32; 2]| {
                if touch {
                    serde_json::json!({"touch":phase,"point":point})
                } else {
                    match phase {
                        "down" => serde_json::json!({"point":point,"down":true}),
                        "up" => serde_json::json!({"down":false}),
                        _ => serde_json::json!({"point":point}),
                    }
                }
            };
            let before = saved();
            perform(serde_json::json!([event("down", start)]));
            pump(35);
            perform(serde_json::json!([event("move", origin)]));
            pump(300);
            assert!(
                w.workspace_drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.started)
            );
            let refreshes = w.publication.refreshes.get();
            w.publication.inputs.borrow_mut().clear();
            let geometry = Rc::new(RefCell::new(Vec::new()));
            let last = Rc::new(Cell::new(view.width()));
            let clock = w.surface.frame_clock().unwrap();
            let after_paint = clock.connect_after_paint(glib::clone!(
                #[strong]
                geometry,
                #[strong]
                last,
                #[strong]
                view,
                move |clock| {
                    let width = view.width();
                    if last.replace(width) != width {
                        if let Some(timing) = clock.current_timings() {
                            geometry.borrow_mut().push((timing, width));
                        }
                    }
                }
            ));
            let events: Vec<_> = (0..550)
                .map(|i| {
                    let p = (i as f32 * 0.004 * 5.) % 4.;
                    let triangle = if p < 1. {
                        p
                    } else if p < 3. {
                        2. - p
                    } else {
                        p - 4.
                    };
                    event("move", [origin[0] + triangle * 65., origin[1]])
                })
                .collect();
            perform(serde_json::to_value(events).unwrap());
            pump(50);
            clock.disconnect(after_paint);
            let mut cpu = w.publication.inputs.borrow().clone();
            cpu.sort_by(f64::total_cmp);
            let times: Vec<_> = geometry
                .borrow()
                .iter()
                .filter(|(t, _)| t.is_complete() && t.presentation_time() > 0)
                .map(|(t, _)| t.presentation_time())
                .collect();
            let hz = if times.len() > 1 {
                (times.len() - 1) as f64 * 1_000_000. / (times.last().unwrap() - times[0]) as f64
            } else {
                0.
            };
            let refreshed = w.publication.refreshes.get() - refreshes;
            assert!(
                cpu.len() > 100 && times.len() > 20,
                "changing geometry, not idle frame callbacks"
            );
            let expected = w
                .resolved()
                .groups
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .bounds
                .width;
            assert!(
                (view.width() as f32 - expected).abs() <= 1.,
                "native width matches Rust"
            );
            assert_eq!(content, w.panel_widget(panel), "retain panel resources");
            if std::env::var_os("LAYER_RESIZE_RETAINED").is_some() {
                assert_eq!(refreshed, 0);
            }
            if let Ok(minimum) = std::env::var("LAYER_RESIZE_MIN_HZ") {
                assert!(
                    hz >= minimum.parse::<f64>().unwrap(),
                    "{touch}/{scenario}: {hz} Hz"
                );
            }
            let report = serde_json::json!({"touch":touch,"scenario":scenario,"inputs":cpu.len(),"model_refreshes":refreshed,"dispatch_ms":{"p50":cpu[cpu.len()/2],"p95":cpu[cpu.len()*95/100]},"geometry_hz":hz,"changed_presentations":times.len(),"width_range":[geometry.borrow().iter().map(|(_,w)|*w).min(),geometry.borrow().iter().map(|(_,w)|*w).max()]});
            eprintln!("{report}");
            reports.push(report);
            perform(serde_json::json!([event("up", origin)]));
            pump(200);
            let after = saved();
            assert_ne!(after, before);
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            pump(150);
            assert_eq!(saved(), before);
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            pump(150);
            assert_eq!(saved(), after);
            assert!(w.workspace_drag.borrow().is_none());

            // Host focus cancellation must discard any pending reflow and
            // restore the complete gesture before the native contact releases.
            let b = w
                .resolved()
                .dividers
                .into_iter()
                .find(|d| d.id == divider && d.band)
                .unwrap()
                .bounds;
            let start = [b.x + b.width * 0.5, b.y + b.height * 0.5];
            perform(serde_json::json!([event("down", start)]));
            pump(25);
            perform(serde_json::json!([event(
                "move",
                [start[0] + 35., start[1]]
            )]));
            pump(100);
            assert_ne!(saved(), after);
            w.interact(UiInput::Blur);
            perform(serde_json::json!([event("up", start)]));
            pump(150);
            assert_eq!(saved(), after, "cancellation restores the live layout");
        }
    }
    let output = std::env::var("LAYER_TEST_ARTIFACTS")
        .unwrap_or_else(|_| "../../artifacts/workspace-resize".into());
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        format!("{output}/gtk.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.close();
    pump(100);
}
