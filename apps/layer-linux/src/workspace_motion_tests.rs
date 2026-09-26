//! Sustained real Mutter mouse/touch delivery and native presentation feedback.
use super::*;

#[test]
#[ignore = "isolated 120 Hz Mutter and native-input.js --workspace-motion"]
fn native_workspace_motion_input() {
    let app = native_test_app("art.capycanvas.WorkspaceMotion");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(600);
    super::set_transparency(&w, "LAYER_MOTION_TRANSPARENCY");
    pump(1600);
    // Explicitly restore the fixed tab fixture below. A fresh workspace store
    // can finish loading the shipped preset after the realize callback.
    let original = layer_ui::WorkspaceState::default();
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(10);
    input.ready();
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let mut reports = Vec::new();
    for touch in [false, true] {
        for scenario in ["group", "tab", "tear-off", "navigator", "color-overlap"] {
            let mut fixture = original.clone();
            let viewport = [w.surface.width() as f32, w.surface.height() as f32];
            if scenario == "color-overlap" {
                fixture.layout.set_panel_visible(Panel::Color, true).unwrap();
                fixture.layout.move_panel(
                    viewport,
                    Panel::Color,
                    DockTarget::Float { position: [430., 160.] },
                ).unwrap();
                let color = fixture.layout.floating.last_mut().unwrap();
                color.width = 360.;
                color.height = Some(400.);
            }
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
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(fixture),
            });
            pump(250);
            let group = state(&w)
                .workspace
                .layout
                .panel_group(Panel::Layers)
                .unwrap();
            if matches!(scenario, "group" | "navigator" | "color-overlap") {
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
            if scenario == "color-overlap" {
                assert!(w.color_panel.root.is_mapped(), "measure a visible Color panel");
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
            let target = if matches!(scenario, "group" | "navigator" | "color-overlap") {
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
            let device = if touch { "touch" } else { "mouse" };
            let event = |phase: &str, p: [f32; 2]| contact(device, phase, p);
            input.perform(serde_json::json!([event("down", start)]));
            pump(35);
            if scenario == "tab" {
                input.perform(serde_json::json!([event(
                    "move",
                    [start[0] - 14., start[1]]
                )]));
            }
            input.perform(serde_json::json!([event("move", origin)]));
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
            let color_cache = (scenario == "color-overlap").then(|| color_panel::hue_guide(&w));
            let colors = state(&w).colors;
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
            let worker_stats = w.gpu.borrow().as_ref().unwrap().session.engine().backend().stats.clone();
            *worker_stats.lock().unwrap() = Default::default();
            let mut events = events;
            if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                events.insert(events.len() / 2, serde_json::json!({"capture": format!("{touch}-{scenario}")}));
            }
            input.perform(serde_json::to_value(events).unwrap());
            pump(50);
            clock.disconnect(after);
            let canvas = {
                let stats = worker_stats.lock().unwrap();
                let mut gpu: Vec<f64> = stats.gpu.iter().map(|v| v[1]).collect();
                gpu.sort_by(f64::total_cmp);
                let mut cpu: Vec<f64> = stats.cpu.iter().map(|v| v[3]).collect();
                cpu.sort_by(f64::total_cmp);
                serde_json::json!({
                    "backdrop_frames": stats.backdrop_frames,
                    "frames": stats.cpu.len(),
                    "gpu_ms": gpu.get(gpu.len() / 2), "gpu_p95_ms": gpu.get(gpu.len() * 95 / 100),
                    "worker_cpu_ms": cpu.get(cpu.len() / 2),
                })
            };
            if let Some(cache) = color_cache {
                assert_eq!(color_panel::hue_guide(&w), cache, "overlapping motion reuses the texture");
                assert_eq!(state(&w).colors, colors, "overlapping motion does not pick colors");
                let color = w.color_panel.root.compute_bounds(&w.surface).unwrap();
                let moving = view.compute_bounds(&w.surface).unwrap();
                assert!(color.intersection(&moving).is_some(), "motion crosses the Color panel");
            }
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
            let mut placement_cpu: Vec<_> = frames.iter().map(|f| f.1).collect();
            placement_cpu.sort_by(f64::total_cmp);
            assert!(!placement_cpu.is_empty(), "publish native placement");
            if let Ok(minimum) = std::env::var("LAYER_MOTION_MIN_HZ") {
                assert!(
                    hz(&presented) >= minimum.parse::<f64>().unwrap(),
                    "{touch}/{scenario}: {} Hz",
                    hz(&presented)
                );
            }
            let report = serde_json::json!({"touch":touch,"scenario":scenario,"inputs":cpu.len(),"model_refreshes":0,"dispatch_ms":{"p50":cpu[cpu.len()/2],"p95":cpu[cpu.len()*95/100]},"placement_ms":{"p50":placement_cpu[placement_cpu.len()/2],"p95":placement_cpu[placement_cpu.len()*95/100]},"placement_hz":hz(&frames.iter().map(|f|f.0).collect::<Vec<_>>()),"presentation_hz":hz(&presented),"presented_frames":presented.len(),"refresh_intervals_us":timings.borrow().iter().map(|t|t.refresh_interval()).collect::<std::collections::BTreeSet<_>>(),"canvas":canvas});
            eprintln!("{report}");
            reports.push(report);
            if scenario == "tab" {
                let first = w
                    .tab_hits()
                    .into_iter()
                    .find(|h| h.group == group && h.index == 0)
                    .unwrap()
                    .bounds;
                input.perform(serde_json::json!([event(
                    "move",
                    [first.x + first.width * 0.25, start[1]]
                )]));
                pump(50);
            }
            input.perform(serde_json::json!([event("up", origin)]));
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
    let report_path = std::env::var("LAYER_MOTION_REPORT").unwrap_or_else(|_| "../../artifacts/workspace-motion/gtk.json".into());
    if let Some(parent) = std::path::Path::new(&report_path).parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        &report_path,
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    input.finish();
    w.window.close();
    pump(100);
}
