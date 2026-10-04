//! Width changes observed after GTK paint, paired with completed presentation feedback.
use super::*;

#[test]
#[ignore = "isolated Mutter and native-input.js --workspace-resize"]
fn native_workspace_resize_input() {
    resize_input(&["left", "sizes", "right", "navigator"]);
}

#[test]
#[ignore = "isolated Mutter and native-input.js"]
fn native_color_wheel_resize_input() {
    resize_input(&["color", "color-float"]);
}

#[test]
#[ignore = "isolated Mutter and native-input.js"]
fn native_hdr_color_wheel_resize_input() {
    resize_input(&["color-hdr", "color-hdr-float"]);
}

fn resize_input(scenarios: &[&str]) {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS")
        .unwrap_or_else(|_| "../../artifacts/workspace-resize".into()));
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.WorkspaceResize");
    let hdr = scenarios.iter().any(|s| s.contains("-hdr"));
    let w = if hdr {
        Workspace::with_project(&app,
            Some((new_drawing_at(2048, 1536, layer_core::color::SampleDepth::F16), None)))
    } else { fixture_workspace(&app) };
    w.window.maximize();
    w.window.present();
    pump(1600);
    let theme = match std::env::var("CAPY_NATIVE_TEST_THEME").as_deref() {
        Ok("light") => Theme::Light,
        Ok("dark") | Err(_) => Theme::Dark,
        _ => panic!("CAPY_NATIVE_TEST_THEME must be light or dark"),
    };
    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
    pump(200);
    assert_eq!(state(&w).theme, theme);
    assert_eq!(adw::StyleManager::for_display(&w.area.display()).is_dark(), theme == Theme::Dark);
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(20);
    input.ready();
    input.click(screen_point(w.header.root.upcast_ref(), &w.window, [0.5, 0.5]));
    until(|| w.window.is_active(), "native resize window activation");
    let saved = || serde_json::to_value(state(&w).workspace).unwrap();
    let mut reports = Vec::new();
    for touch in [false, true] {
        for &scenario in scenarios {
            for repeat in 0..if scenario.starts_with("color") { 3 } else { 1 } {
                let mut fixture = layer_ui::WorkspaceState::default();
                let color = scenario.starts_with("color");
                let floating = scenario.ends_with("-float");
                if color {
                    let group = fixture.layout.panel_group(Panel::Brushes).unwrap();
                    fixture.layout.set_panel_visible(Panel::Color, true).unwrap();
                    fixture.layout.move_panel([w.surface.width() as f32, w.surface.height() as f32],
                        Panel::Color, DockTarget::Tab { group, index: None }).unwrap();
                    fixture.layout.select_tab(group, Panel::Color).unwrap();
                    if floating {
                        fixture.layout.move_panel([w.surface.width() as f32, w.surface.height() as f32],
                            Panel::Color, DockTarget::Float { position: [380., 140.] }).unwrap();
                        let group = fixture.layout.panel_group(Panel::Color).unwrap();
                        let panel = fixture.layout.floating.iter_mut().find(|p| p.root.id() == group).unwrap();
                        panel.width = 272.;
                        panel.height = Some(430.);
                    }
                }
                let left = matches!(scenario, "left" | "sizes") || (color && !floating);
                let edge = if left {
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
                band.extent = if left { 252. } else { 310. };
                let divider = band.id;
                if scenario == "navigator" {
                    if let DockNode::Tabs { panels, active, .. } = &mut band.root {
                        panels.push(Panel::Navigator);
                        *active = Panel::Navigator;
                    }
                }
                if scenario == "sizes" {
                    let group = fixture.layout.panel_group(Panel::Sizes).unwrap();
                    fixture.layout.select_tab(group, Panel::Sizes).unwrap();
                }
                w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(fixture),
                });
                pump(350);
                let panel = if color {
                    Panel::Color
                } else if scenario == "sizes" {
                    Panel::Sizes
                } else if left {
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
                let wheel = named::<crate::tool_panels::ColorWheel>(w.color_panel.root.upcast_ref(), "color-wheel");
                let handle = || if floating {
                    w.resolved().groups.iter().find(|g| g.id == group).unwrap()
                        .resize_handles.iter().find(|h| h.edge == ResizeEdge::Right).unwrap().bounds
                } else { w
                    .resolved()
                    .dividers
                    .iter()
                    .find(|d| d.id == divider && d.band)
                    .unwrap()
                    .bounds };
                let b = handle();
                let start = [b.x + b.width * 0.5, b.y + b.height * 0.5];
                let origin = [
                    start[0] + if left || floating { 20. } else { -20. },
                    start[1],
                ];
                let device = if touch { "touch" } else { "mouse" };
                let event = |phase: &str, point: [f32; 2]| contact(device, phase, point);
                let before = saved();
                if color { super::color_panel::settled_wheel(&wheel); }
                let ramp = named::<crate::hdr_color_scale::HdrColorScale>(w.color_panel.root.upcast_ref(), "color-hdr-intensity-ramp");
                let ramp_texture = if hdr { ramp.imp().texture.borrow().as_ref().map(|(_, t)| t.clone()) } else { None };
                input.perform(serde_json::json!([event("down", start)]));
                pump(35);
                input.perform(serde_json::json!([event("move", origin)]));
                pump(300);
                assert!(
                    w.workspace_drag
                        .borrow()
                        .as_ref()
                        .is_some_and(|d| d.started)
                );
                let refreshes = w.publication.refreshes.get();
                w.publication.inputs.borrow_mut().clear();
                wheel.imp().snapshot_ms.borrow_mut().clear();
                wheel.imp().field_render_ms.borrow_mut().clear();
                let geometry = Rc::new(RefCell::new(Vec::new()));
                let last = Rc::new(Cell::new(view.width()));
                let clock = w.surface.frame_clock().unwrap();
                let paint_start = Rc::new(Cell::new(Instant::now()));
                let before_paint = clock.connect_before_paint(glib::clone!(#[strong] paint_start, move |_| paint_start.set(Instant::now())));
                let after_paint = clock.connect_after_paint(glib::clone!(
                    #[strong]
                    geometry,
                    #[strong]
                    last,
                    #[strong]
                    view,
                    #[strong]
                    paint_start,
                    move |clock| {
                        let width = view.width();
                        if last.replace(width) != width {
                            if let Some(timing) = clock.current_timings() {
                                geometry.borrow_mut().push((timing, width, paint_start.get().elapsed().as_secs_f64() * 1000.));
                            }
                        }
                    }
                ));
                let events: Vec<_> = (0..if color { 1650 } else { 1250 })
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
                input.perform(serde_json::to_value(events).unwrap());
                pump(50);
                clock.disconnect(after_paint);
                clock.disconnect(before_paint);
                let mut cpu = w.publication.inputs.borrow().clone();
                cpu.sort_by(f64::total_cmp);
                let times: Vec<_> = geometry
                    .borrow()
                    .iter()
                    .filter(|(t, _, _)| t.is_complete() && t.presentation_time() > 0)
                    .map(|(t, _, _)| t.presentation_time())
                    .collect();
                let hz = if times.len() > 1 {
                    (times.len() - 1) as f64 * 1_000_000. / (times.last().unwrap() - times[0]) as f64
                } else {
                    0.
                };
                let refreshed = w.publication.refreshes.get() - refreshes;
                let percentile = |mut samples: Vec<f64>, p: usize| {
                    samples.sort_by(f64::total_cmp);
                    samples.get(samples.len() * p / 100).copied().unwrap_or(0.)
                };
                let gap_p99 = percentile(times.windows(2).map(|p| (p[1] - p[0]) as f64 / 1000.).collect(), 99);
                assert!(
                    cpu.len() > if color { 20 } else { 100 } && times.len() > 20,
                    "changing geometry: inputs {}, presentations {}, scenario {scenario}, touch {touch}", cpu.len(), times.len()
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
                if panel == Panel::Sizes {
                    let buttons = w.size_buttons.borrow();
                    let bounds: Vec<_> = buttons.iter().map(|(_, button)| button.compute_bounds(&content).unwrap()).collect();
                    assert_eq!(bounds[0].y(), bounds[6].y(), "wide panels fit more than six tiles");
                    for pair in bounds.windows(2).filter(|p| p[0].y() == p[1].y()) {
                        assert_eq!(pair[1].x() - pair[0].x() - pair[0].width(), layer_ui::TileStyle::Small.gap());
                    }
                }
                if std::env::var_os("LAYER_RESIZE_RETAINED").is_some() {
                    assert_eq!(refreshed, 0);
                }
                let report = serde_json::json!({"touch":touch,"scenario":scenario,"repeat":repeat,"scale":wheel.scale_factor(),"inputs":cpu.len(),"model_refreshes":refreshed,"dispatch_ms":{"p50":cpu[cpu.len()/2],"p95":cpu[cpu.len()*95/100]},"paint_ms":{"p95":percentile(geometry.borrow().iter().map(|(_,_,ms)|*ms).collect(),95)},"geometry_hz":hz,"gap_p99_ms":gap_p99,"wheel_snapshot_p95_ms":percentile(wheel.imp().snapshot_ms.borrow().clone(),95),"field_p95_ms":percentile(wheel.imp().field_render_ms.borrow().clone(),95),"field_rebuilds":wheel.imp().field_render_ms.borrow().len(),"changed_presentations":times.len(),"width_range":[geometry.borrow().iter().map(|(_,w,_)|*w).min(),geometry.borrow().iter().map(|(_,w,_)|*w).max()]});
                eprintln!("{report}");
                reports.push(report);
                if let Ok(minimum) = std::env::var("LAYER_RESIZE_MIN_HZ") {
                    assert!(hz >= minimum.parse::<f64>().unwrap(), "{touch}/{scenario}: {hz} Hz");
                }
                if color {
                    assert_eq!(wheel.imp().field_render_ms.borrow().len(), 0,
                        "resize samples retained colors until release");
                }
                if hdr {
                    assert_eq!(ramp.imp().texture.borrow().as_ref().map(|(_, t)| t.clone()), ramp_texture,
                        "resize retains the intensity raster until release");
                }
                let active = if color {
                    input.perform(serde_json::json!([event("move", origin)]));
                    pump(100);
                    Some(super::color_panel::wheel_texture(&w, wheel.scale_factor() as f32))
                } else { None };
                input.perform(serde_json::json!([event("up", origin)]));
                pump(200);
                if color {
                    super::color_panel::settled_wheel(&wheel);
                }
                if let Some(active) = active {
                    let final_texture = super::color_panel::wheel_texture(&w, wheel.scale_factor() as f32);
                    let name = format!("{scenario}-{device}-{repeat}");
                    active.save_to_png(output.join(format!("{name}-active.png"))).unwrap();
                    final_texture.save_to_png(output.join(format!("{name}-released.png"))).unwrap();
                    assert_resize_controls(&wheel, &active, &final_texture);
                }
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
                let b = handle();
                let start = [b.x + b.width * 0.5, b.y + b.height * 0.5];
                input.perform(serde_json::json!([event("down", start)]));
                pump(25);
                input.perform(serde_json::json!([event(
                    "move",
                    [start[0] + 35., start[1]]
                )]));
                pump(100);
                assert_ne!(saved(), after);
                w.interact(UiInput::Blur);
                input.perform(serde_json::json!([event("up", start)]));
                pump(150);
                assert_eq!(saved(), after, "cancellation restores the live layout");
                if color {
                    assert!(!wheel.imp().resizing.get(), "cancellation restores live wheel rendering");
                    super::color_panel::settled_wheel(&wheel);
                }
            }
        }
    }
    std::fs::write(
        output.join("gtk.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    input.finish();
    w.window.close();
    pump(100);
}

fn assert_resize_controls(wheel: &crate::tool_panels::ColorWheel, active: &gdk::Texture, released: &gdk::Texture) {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    assert_eq!((active.width(), active.height()), (released.width(), released.height()));
    let scale = wheel.scale_factor() as f32;
    let (size, [x, y]) = wheel.drawing_bounds();
    let g = layer_ui::ColorWheelGeometry::new(size).unwrap();
    let intensity = wheel.imp().intensity.borrow();
    let arc = intensity.as_ref().filter(|i| i.is_visible()).map(|i|
        (i.geometry().unwrap(), i.compute_bounds(wheel).unwrap()));
    let mut a = vec![0; active.width() as usize * active.height() as usize * 4];
    let mut b = a.clone();
    let stride = active.width() as usize * 4;
    active.download(&mut a, stride);
    released.download(&mut b, stride);
    let mut checked = 0;
    let mut changed = 0;
    for py in 0..active.height() as usize {
        for px in 0..active.width() as usize {
            let point = [(px as f32 + 0.5) / scale, (py as f32 + 0.5) / scale];
            if (point[0] - x - g.center[0]).hypot(point[1] - y - g.center[1]) <= g.outer + 3. { continue; }
            if arc.as_ref().is_some_and(|(g, b)| g.contains([point[0] - b.x(), point[1] - b.y()])) { continue; }
            let i = py * stride + px * 4;
            checked += 1;
            if a[i..i + 4].iter().zip(&b[i..i + 4]).any(|(a,b)| a.abs_diff(*b) > 1) { changed += 1; }
        }
    }
    assert!(checked > 1000);
    assert_eq!(changed, 0, "text and controls keep their current layout through resize release");
}
