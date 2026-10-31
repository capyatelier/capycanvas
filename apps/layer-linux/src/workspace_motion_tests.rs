//! Sustained real Mutter mouse/touch delivery and native presentation feedback.
use super::*;

#[test]
#[ignore = "isolated 120 Hz Mutter and native-input.js --workspace-motion"]
fn native_workspace_motion_input() {
    workspace_motion_input(&["group", "tab", "header", "tear-off", "navigator", "color-overlap"]);
}

#[test]
#[ignore = "isolated 120 Hz Mutter and native-input.js --native-test=native_workspace_tab_motion_input"]
fn native_workspace_tab_motion_input() {
    workspace_motion_input(&["tab"]);
}

fn workspace_motion_input(scenarios: &[&str]) {
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
    let report_path=std::env::var("LAYER_MOTION_REPORT").unwrap_or_else(|_|"../../artifacts/workspace-motion/gtk.json".into());
    for touch in [false, true] {
        for &scenario in scenarios {
            if scenario == "tab" {
                until(|| {
                    w.gpu.borrow().as_ref().is_some_and(|gpu| {
                        let clipped = gpu.session.engine().backend().screen_clipped;
                        clipped.is_some() && gpu.session.state().screen.clipped == clipped
                    })
                }, "initial screen clipping report published");
            }
            let mut fixture = original.clone();
            if scenario == "header" { fixture.layout.header = layer_ui::HeaderLayout::painter_for_platform(layer_ui::Platform::Gtk); }
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
            if scenario == "header" { w.dispatch(layer_ui::HeaderAction::Edit { editing: true }.action()); }
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
            let view = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .root
                .clone();
            let target = if scenario == "header" {
                let id = state(&w).workspace.layout.header.entries().find(|entry| matches!(entry.item, HeaderItem::Tool { .. })).unwrap().id;
                find_named(w.surface.upcast_ref(), &format!("header-item-{id}")).unwrap()
            } else if matches!(scenario, "group" | "navigator" | "color-overlap") {
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
            } else if scenario == "header" {
                [viewport[0] / 2., start[1]]
            } else if scenario == "tab" {
                start
            } else {
                [start[0] - 28., start[1]]
            };
            let device = if touch { "touch" } else { "mouse" };
            let event = |phase: &str, p: [f32; 2]| contact(device, phase, p);
            input.perform(serde_json::json!([event("down", start)]));
            pump(35);
            let before = saved();
            let pickup_started = glib::monotonic_time();
            let pickup_timings = Rc::new(RefCell::new(Vec::new()));
            let pickup_clock = w.surface.frame_clock().unwrap();
            let pickup_after = pickup_clock.connect_after_paint(glib::clone!(#[strong] pickup_timings, move |clock| {
                if let Some(timing) = clock.current_timings() { pickup_timings.borrow_mut().push(timing); }
            }));
            if scenario == "tab" {
                input.perform(serde_json::json!([event(
                    "move",
                    [start[0] - 14., start[1]]
                )]));
                until(
                    || w.workspace_drag.borrow().as_ref().is_some_and(|drag| drag.started),
                    "native tab pickup before returning to the press point",
                );
            }
            input.perform(serde_json::json!([event("move", origin)]));
            pump(350);
            pickup_clock.disconnect(pickup_after);
            let pickup_command_to_present_ms = pickup_timings.borrow().iter()
                .filter(|timing| timing.is_complete() && timing.frame_time() >= pickup_started && timing.presentation_time() > 0)
                .map(|timing| (timing.presentation_time() - pickup_started) as f64 / 1000.)
                .min_by(f64::total_cmp);
            assert!(
                w.workspace_drag
                    .borrow()
                    .as_ref()
                    .is_some_and(|d| d.started),
                "{device}/{scenario}: native recognizer started; theme={:?}",
                std::env::var("CAPY_NATIVE_TEST_THEME").unwrap_or_else(|_| "default".into())
            );
            let model = w.publication.model_revision.get();
            let refreshes = w.publication.refreshes.get();
            let color_cache = (scenario == "color-overlap").then(|| color_panel::hue_guide(&w));
            let colors = state(&w).colors;
            w.publication.inputs.borrow_mut().clear();
            w.publication.frames.borrow_mut().clear();
            let motion_samples = Rc::new(Cell::new(0usize));
            let motion = gtk::EventControllerLegacy::new();
            motion.set_propagation_phase(gtk::PropagationPhase::Capture);
            motion.connect_event(glib::clone!(#[strong] motion_samples, move |_, event| {
                if matches!(event.event_type(), gdk::EventType::MotionNotify | gdk::EventType::TouchUpdate) {
                    motion_samples.set(motion_samples.get() + 1);
                }
                glib::Propagation::Proceed
            }));
            w.surface.add_controller(motion.clone());
            let clock = w.surface.frame_clock().unwrap();
            let timings = Rc::new(RefCell::new(Vec::new()));
            let sliding_motion=matches!(scenario,"tab"|"header");
            let header_motion=scenario=="header";
            let scale=crate::squircle::device_scale(&w.window) as f32;
            let pose=Rc::new(glib::clone!(#[weak] w, #[upgrade_or_default] move || if header_motion {
                w.header.drag_for_test().map(|drag| drag.held)
            } else { w.workspace_drag.borrow().as_ref().and_then(|d|d.tab.as_ref()).map(|tab|tab.bounds) }
                .map(|bounds|[bounds.x,bounds.y,bounds.width,bounds.height].map(|v|(v*scale).round() as i32))));
            let last_pose=Rc::new(Cell::new(if sliding_motion {pose()} else {None}));
            let poses=Rc::new(RefCell::new(Vec::new()));
            let (paint_timer, before_paint) = PaintTimer::connect(&clock);
            let paint_cpu = Rc::new(RefCell::new(Vec::new()));
            let after = clock.connect_after_paint(glib::clone!(
                #[strong]timings,#[strong]last_pose,#[strong]poses,#[strong]pose,#[strong]paint_timer,#[strong]paint_cpu,
                move |clock| {
                    let pose=sliding_motion.then(||pose()).flatten();
                    if sliding_motion && (pose.is_none() || last_pose.replace(pose)==pose) {return;}
                    if let Some(t) = clock.current_timings() {
                        timings.borrow_mut().push(t);
                        if let Some(pose)=pose {poses.borrow_mut().push(pose);}
                        paint_cpu.borrow_mut().push(paint_timer.elapsed().1);
                    }
                }
            ));
            let events: Vec<_> = (0..if sliding_motion { 1250 } else { 550 })
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
                                + if sliding_motion {
                                    triangle(t * 8.) * 40.
                                } else {
                                    triangle(t * 4.) * 100.
                                },
                            origin[1]
                                + if sliding_motion {
                                    0.
                                } else {
                                    triangle(t * 3.) * 65.
                                },
                        ],
                    )
                })
                .collect();
            let worker_stats = ui_session(&w).engine().backend().stats.clone();
            *worker_stats.lock().unwrap() = Default::default();
            let mut events = events;
            if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                events.insert(events.len() / 2, serde_json::json!({"capture": format!("{touch}-{scenario}")}));
            }
            input.perform(serde_json::to_value(events).unwrap());
            pump(50);
            w.surface.remove_controller(&motion);
            clock.disconnect(after);
            clock.disconnect(before_paint);
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
            let update = ui_session(&w).workspace_update();
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
            let frames = w.publication.frames.borrow().clone();
            let mut presented: Vec<_> = timings
                .borrow()
                .iter()
                .filter(|t| t.is_complete() && t.presentation_time() > 0)
                .map(|t| t.presentation_time())
                .collect();
            presented.sort_unstable();
            presented.dedup();
            let mut presentation_gaps: Vec<_> = presented.windows(2).map(|pair| (pair[1] - pair[0]) as f64 / 1000.).collect();
            presentation_gaps.sort_by(f64::total_cmp);
            let presentation_gap_p99_ms = presentation_gaps.get(presentation_gaps.len() * 99 / 100).copied();
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
            let input_samples = if header_motion { motion_samples.get() } else { cpu.len() };
            let report = serde_json::json!({"touch":touch,"scenario":scenario,"pickup_command_to_present_ms":pickup_command_to_present_ms,"native_input_samples":cpu.len(),"native_motion_samples":motion_samples.get(),"native_input_gate_passed":input_samples>200,"native_input_minimum_exclusive":200,"model_refreshes":0,"dispatch_ms":{"p50":cpu.get(cpu.len()/2),"p95":cpu.get(cpu.len()*95/100)},"placement_ms":{"p50":placement_cpu.get(placement_cpu.len()/2),"p95":placement_cpu.get(placement_cpu.len()*95/100)},"placement_hz":(!frames.is_empty()).then(||hz(&frames.iter().map(|f|f.0).collect::<Vec<_>>())),"presentation_hz":hz(&presented),"presented_frames":presented.len(),"presentation_gap_p99_ms":presentation_gap_p99_ms,"refresh_intervals_us":timings.borrow().iter().map(|t|t.refresh_interval()).collect::<std::collections::BTreeSet<_>>(),"canvas":canvas});
            let mut report=report;let mut paints=paint_cpu.borrow().clone();paints.sort_by(f64::total_cmp);
            report["paint_thread_cpu_p95_ms"]=serde_json::json!(paints.get(paints.len()*95/100));
            if sliding_motion {
                let poses=poses.borrow();let range:[[_;2];4]=std::array::from_fn(|axis|[poses.iter().map(|p|p[axis]).min(),poses.iter().map(|p|p[axis]).max()]);
                assert!(poses.len()>20&&range[0][1].unwrap()-range[0][0].unwrap()>10,"observe changing requested device-rounded drag geometry");
                report["changed_requested_device_poses"]=serde_json::json!(poses.len());report["requested_device_pose_range"]=serde_json::json!(range);
            }
            eprintln!("{report}");
            reports.push(report);
            if scenario != "header" { assert!(!placement_cpu.is_empty(), "publish native placement"); }
            assert!(input_samples > 200, "receive high-rate native input: {} samples", input_samples);
            if let Ok(minimum) = std::env::var("LAYER_MOTION_MIN_HZ") {
                assert!(
                    hz(&presented) >= minimum.parse::<f64>().unwrap(),
                    "{touch}/{scenario}: {} Hz",
                    hz(&presented)
                );
            }
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
            if scenario == "header" { w.dispatch(layer_ui::HeaderAction::Edit { editing: false }.action()); }
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
