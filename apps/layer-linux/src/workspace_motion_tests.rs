//! Sustained real Mutter mouse/touch delivery and native presentation feedback.
use super::*;

#[test]
#[ignore = "isolated 120 Hz Mutter and native-input.js --workspace-motion"]
fn native_workspace_motion_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("art.capycanvas.WorkspaceMotion");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let original = state(&w).workspace;
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native motion timed out");
            pump(2);
        }
        step += 1;
    };
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let mut reports = Vec::new();
    for touch in [false, true] {
        for scenario in ["group", "tab", "tear-off", "navigator"] {
            let mut fixture = original.clone();
            if scenario == "navigator" {
                let band = fixture
                    .layout
                    .bands
                    .iter_mut()
                    .find(|b| b.edge == Edge::Right)
                    .unwrap();
                if let DockNode::Tabs { panels, active, .. } = &mut band.root {
                    panels.push(Panel::Navigator);
                    *active = Panel::Navigator;
                } else {
                    panic!("fixture must have a right tab group");
                }
            }
            w.dispatch(UiAction::RestoreWorkspace { workspace: fixture });
            pump(250);
            let group = state(&w)
                .workspace
                .layout
                .panel_group(Panel::Layers)
                .unwrap();
            let viewport = [w.surface.width() as f32, w.surface.height() as f32];
            if matches!(scenario, "group" | "navigator") {
                w.dispatch(UiAction::MoveGroup {
                    group,
                    target: DockTarget::Float {
                        position: [550., 220.],
                    },
                    viewport,
                });
            }
            pump(350);
            if scenario == "navigator" {
                assert!(w.navigator.root.is_mapped(), "measure a visible Navigator");
            }
            assert!(!w.status.is_visible(), "{}", w.status.text());
            let before = saved();
            let view = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .root
                .clone();
            let target = if matches!(scenario, "group" | "navigator") {
                find_css(view.upcast_ref(), "panel-grip").unwrap()
            } else {
                w.groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .tabs
                    .iter()
                    .find(|(p, _)| *p == Panel::Adjustments)
                    .unwrap()
                    .1
                    .clone()
                    .upcast()
            };
            let b = target.compute_bounds(&w.surface).unwrap();
            let start = [b.x() + b.width() / 2., b.y() + b.height() / 2.];
            let origin = if scenario == "tear-off" {
                [650., 400.]
            } else if scenario == "tab" {
                start
            } else {
                [start[0] - 28., start[1]]
            };
            let event = |phase: &str, p: [f32; 2]| {
                if touch {
                    serde_json::json!({"touch": phase, "point": p})
                } else {
                    match phase {
                        "down" => serde_json::json!({"point": p, "down": true}),
                        "up" => serde_json::json!({"down": false}),
                        _ => serde_json::json!({"point": p}),
                    }
                }
            };
            perform(serde_json::json!([event("down", start)]));
            pump(35);
            if scenario == "tab" {
                perform(serde_json::json!([event(
                    "move",
                    [start[0] - 14., start[1]]
                )]));
            }
            perform(serde_json::json!([event("move", origin)]));
            pump(350);
            assert!(
                w.workspace_drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.started),
                "{scenario}: native recognizer started"
            );
            let model = w.publication.model_revision.get();
            let refreshes = w.publication.refreshes.get();
            w.publication.inputs.borrow_mut().clear();
            w.publication.frames.borrow_mut().clear();
            let clock = w.surface.frame_clock().unwrap();
            let timings = Rc::new(RefCell::new(Vec::new()));
            let after = clock.connect_after_paint(glib::clone!(
                #[strong]
                timings,
                move |clock| {
                    if let Some(t) = clock.current_timings() {
                        timings.borrow_mut().push(t);
                    }
                }
            ));
            let events: Vec<_> = (0..550)
                .map(|i| {
                    let t = i as f32 * 0.004;
                    let triangle = |t: f32| {
                        let p = t % 4.;
                        if p < 1. {
                            p
                        } else if p < 3. {
                            2. - p
                        } else {
                            p - 4.
                        }
                    };
                    event(
                        "move",
                        [
                            origin[0]
                                + if scenario == "tab" {
                                    triangle(t * 8.) * 40.
                                } else {
                                    triangle(t * 4.) * 100.
                                },
                            origin[1]
                                + if scenario == "tab" {
                                    0.
                                } else {
                                    triangle(t * 3.) * 65.
                                },
                        ],
                    )
                })
                .collect();
            perform(serde_json::to_value(events).unwrap());
            pump(50);
            clock.disconnect(after);
            assert_eq!(
                w.publication.refreshes.get(),
                refreshes,
                "{touch}/{scenario}: retain all UI models during steady motion"
            );
            assert_eq!(w.publication.model_revision.get(), model);
            let update = w.gpu.borrow().as_ref().unwrap().session.workspace_update();
            if let Some(group) = update.drag.as_ref().and_then(|d| d.group.as_ref()) {
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group.id)
                    .unwrap()
                    .root
                    .clone();
                let b = root.compute_bounds(&w.surface).unwrap();
                assert!(
                    (b.x() - group.bounds.x).abs() <= 1. && (b.y() - group.bounds.y).abs() <= 1.,
                    "native bounds {b:?} shared {:?}",
                    group.bounds
                );
                let picked = w
                    .surface
                    .pick(
                        (b.x() + 10.) as f64,
                        (b.y() + 10.) as f64,
                        gtk::PickFlags::DEFAULT,
                    )
                    .unwrap();
                assert!(
                    picked == root || picked.is_ancestor(&root),
                    "picking follows native placement"
                );
            } else {
                assert_eq!(
                    target.compute_bounds(&w.surface).unwrap(),
                    b,
                    "tab insertion slots stay frozen"
                );
            }
            let mut cpu = w.publication.inputs.borrow().clone();
            cpu.sort_by(f64::total_cmp);
            assert!(cpu.len() > 200, "receive high-rate native input");
            let frames = w.publication.frames.borrow().clone();
            let mut presented: Vec<_> = timings
                .borrow()
                .iter()
                .filter(|t| t.is_complete() && t.presentation_time() > 0)
                .map(|t| t.presentation_time())
                .collect();
            presented.sort_unstable();
            presented.dedup();
            let hz = |times: &[i64]| {
                if times.len() > 1 {
                    (times.len() - 1) as f64 * 1_000_000.
                        / (times.last().unwrap() - times[0]) as f64
                } else {
                    0.
                }
            };
            let report = serde_json::json!({"touch":touch,"scenario":scenario,"inputs":cpu.len(),"model_refreshes":0,"dispatch_ms":{"p50":cpu[cpu.len()/2],"p95":cpu[cpu.len()*95/100]},"placement_hz":hz(&frames.iter().map(|f|f.0).collect::<Vec<_>>()),"presentation_hz":hz(&presented),"presented_frames":presented.len(),"refresh_intervals_us":timings.borrow().iter().map(|t|t.refresh_interval()).collect::<std::collections::BTreeSet<_>>()});
            eprintln!("{report}");
            reports.push(report);
            if scenario == "tab" {
                let first = w
                    .tab_hits()
                    .into_iter()
                    .find(|h| h.group == group && h.index == 0)
                    .unwrap()
                    .bounds;
                perform(serde_json::json!([event(
                    "move",
                    [first.x + first.width * 0.25, start[1]]
                )]));
                pump(50);
            }
            perform(serde_json::json!([event("up", origin)]));
            pump(250);
            let after = saved();
            assert_ne!(after, before, "drop changes layout");
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            pump(120);
            assert_eq!(saved(), before, "one undo restores the whole gesture");
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            pump(120);
            assert_eq!(saved(), after, "redo restores the drop");
            assert!(w.workspace_drag.borrow().is_none());
        }
    }
    std::fs::create_dir_all("../../artifacts/workspace-motion").unwrap();
    std::fs::write(
        "../../artifacts/workspace-motion/gtk.json",
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.join("finished"), "finished").unwrap();
    w.window.close();
    pump(100);
}
