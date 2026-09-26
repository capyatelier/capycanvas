//! Real compositor input, picker rendering and retained drawer checks.
use super::*;

fn picker_dropdown(d: &mut Driver, name: &str, label: &str, touch: bool) {
    let widget = find_named(&d.named("tool-drawer"), name).unwrap();
    let point = d.point(&widget);
    let target =
        d.w.window
            .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
            .unwrap();
    assert!(
        target == widget || target.is_ancestor(&widget),
        "{name} hit {} ({}) at {point:?}",
        target.widget_name(),
        target.type_().name()
    );
    if touch {
        let p = d.point(&widget);
        d.input
            .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
    } else {
        d.click(&widget);
    }
    assert!(state(&d.w).customization.drawer.is_some());
    let item = d.label(label);
    assert!(item.native().is_some_and(|n| n.is::<gtk::Popover>()));
    if touch {
        let p = d.point(&item);
        d.input
            .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
    } else {
        d.click(&item);
    }
    assert!(state(&d.w).customization.drawer.is_some());
    assert!(widget.is_mapped());
}

fn picker_artwork(d: &mut Driver) {
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    d.w.dispatch(UiAction::SetBrushSize { value: 100. });
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.06, 0.42, 0.64, 1.],
    });
    pump(500);
    let a = [690., 450.];
    let b = [920., 500.];
    d.input.perform(serde_json::json!([{"point":a,"down":true},{"point":[750,460]},{"point":[830,480]},{"point":b},{"down":false}]));
    pump(400);
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.93, 0.30, 0.19, 1.],
    });
    d.input.perform(serde_json::json!([{"point":[735,510],"down":true},{"point":[790,500]},{"point":[850,510]},{"point":[905,540]},{"down":false}]));
    pump(300);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_color_picker_input --tablet"]
fn native_color_picker_input() {
    let mut d = Driver::new("art.capycanvas.ColorPickerReview");
    let toolbar = state(&d.w)
        .workspace
        .layout
        .panels
        .into_iter()
        .find(|p| {
            p.tiles()
                .iter()
                .any(|t| t.control == ToolbarControl::BrushSizeSlider)
        })
        .unwrap();
    let id = toolbar.tiles()[1].id;
    let button = format!("tile-{id}");
    let undo = format!("tile-{}", toolbar.tiles()[3].id);
    let redo = format!("tile-{}", toolbar.tiles()[4].id);
    assert!(find_named(&d.named(&button), "layer-color-picker-symbolic").is_some());
    picker_artwork(&mut d);
    assert!(d.named(&undo).is_sensitive());
    assert!(!d.named(&redo).is_sensitive());
    d.click_name(&undo);
    assert!(d.named(&redo).is_sensitive());
    d.click_name(&redo);
    assert!(!d.named(&redo).is_sensitive());
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.78, 0.58, 0.23, 1.],
    });
    let original = state(&d.w).colors.clone();
    let previous = state(&d.w).layer_tools.tool;
    d.input.perform(serde_json::json!([{ "key": 0xffe9, "down": true }]));
    pump(200);
    assert!(state(&d.w).layer_tools.tool.picks_color(), "Alt samples while held");
    d.input.perform(serde_json::json!([{ "key": 0xffe9, "down": false }]));
    pump(200);
    assert_eq!(state(&d.w).layer_tools.tool, previous);
    assert_eq!(state(&d.w).colors, original);
    let revision =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision;
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.click_name(&button);
        assert!(state(&d.w).layer_tools.tool.picks_color());
        d.input.perform(serde_json::json!([{"point":[820,479]}]));
        pump(300);
        assert!(state(&d.w).color_picker.preview.is_some());
        assert_eq!(state(&d.w).colors, original);
        assert!(
            d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .color_picker_overlay()
                .is_some()
        );
        d.capture_canvas(&format!("color-picker-glass-{theme:?}.png"));
        d.input.key(0xff1b);
        assert_eq!(state(&d.w).layer_tools.tool, previous);
        assert_eq!(state(&d.w).colors, original);
        assert!(state(&d.w).color_picker.preview.is_none());
        // Sketch opens only settings. Native popup clicks must retain the drawer.
        let p = d.point(&d.named(&button));
        d.input.perform(serde_json::json!([{"point":p,"down":true},{"down":false},{"wait_ms":70},{"down":true},{"down":false}]));
        assert!(state(&d.w).customization.drawer.is_some());
        assert_eq!(
            state(&d.w)
                .customization
                .drawer
                .as_ref()
                .unwrap()
                .columns
                .len(),
            1
        );
        assert!(d.named("color-picker-source").is_mapped());
        assert!(d.named(&button).has_css_class("drawer-open"));
        assert!(d.named(&button).has_css_class("drawer-origin-right"));
        d.input
            .perform(serde_json::json!([{"point":[1100,30],"down":true},{"down":false}]));
        assert!(state(&d.w).customization.drawer.is_some());
        d.capture_canvas(&format!("color-picker-drawer-{theme:?}.png"));
        let source = d
            .named("color-picker-source")
            .downcast::<gtk::DropDown>()
            .unwrap();
        picker_dropdown(
            &mut d,
            "color-picker-source",
            "Selected layer",
            theme == Theme::Dark,
        );
        assert_eq!(source.selected(), 1);
        d.input
            .perform(serde_json::json!([{"point":[810,479]},{"point":[820,479]}]));
        assert!(
            state(&d.w).layer_tools.tool.picks_color(),
            "source selection preserves temporary picking"
        );
        assert!(
            d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .color_picker_overlay()
                .unwrap()
                .layer
        );
        d.capture_canvas(&format!("color-picker-layer-{theme:?}.png"));
        picker_dropdown(
            &mut d,
            "color-picker-source",
            "Visible color",
            theme == Theme::Dark,
        );
        assert_eq!(source.selected(), 0);
        assert!(state(&d.w).customization.drawer.is_some());
        assert_eq!(d.named("color-picker-source"), source);
        d.input
            .perform(serde_json::json!([{"point":[810,479]},{"point":[820,479]}]));
        assert!(
            !d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .color_picker_overlay()
                .unwrap()
                .layer
        );
        let size = d.named("color-picker-size");
        picker_dropdown(
            &mut d,
            "color-picker-size",
            "101 px circle",
            theme == Theme::Dark,
        );
        assert_eq!(state(&d.w).color_picker.sample_width, 101);
        assert_eq!(d.named("color-picker-size"), size);
        d.input.key(0xff1b);
        // Escape restores the tool; close any remaining options drawer.
        d.w.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        });
        d.w.dispatch(UiAction::SetColorSampleSize { width: 1 });
        pump(200);
    }
    d.w.dispatch(UiAction::SetColorSampleSize { width: 1 });
    // I temporarily enters picking, native mouse press commits and consumes Up.
    d.input.key('i' as u32);
    d.input.click([820., 479.]);
    pump(300);
    assert_eq!(state(&d.w).layer_tools.tool, previous);
    assert_ne!(state(&d.w).colors, original);
    assert_eq!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        revision
    );
    let sketch_workspace = state(&d.w).workspace;
    // Exercise native popup grabs before synthetic tablet serials are injected;
    // the tablet proxy cannot authorize compositor grabs.
    // Paint and Photo retain an Eyedropper category with both presentations.
    for preset in [WorkspacePreset::Illustrator, WorkspacePreset::Photographer] {
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: preset.layout(Platform::Gtk),
                ..WorkspaceState::default()
            }),
        });
        pump(500);
        d.w.dispatch(UiAction::ColorPicker {
            action: layer_ui::ColorPickerAction::Source { layer: false },
        });
        let category = state(&d.w)
            .workspace
            .layout
            .panels
            .iter()
            .find_map(|p| {
                p.tiles()
                    .iter()
                    .find(|t| {
                        t.control
                            == ToolbarControl::Command {
                                command: CommandId::Eyedropper,
                            }
                    })
                    .map(|t| format!("tile-{}", t.id))
            })
            .unwrap();
        let button = d.named(&category);
        assert!(find_named(&button, "layer-eyedropper-symbolic").is_some());
        let p = d.point(&button);
        d.input.perform(serde_json::json!([{"point":p,"down":true},{"down":false},{"wait_ms":70},{"down":true},{"down":false}]));
        assert_eq!(
            state(&d.w)
                .customization
                .drawer
                .as_ref()
                .unwrap()
                .columns
                .len(),
            2
        );
        d.click(&find_named(&d.named("tool-drawer"), "layer-eyedropper-symbolic").unwrap());
        assert_eq!(
            state(&d.w).color_picker.style,
            layer_ui::ColorPickerStyle::Eyedropper
        );
        picker_dropdown(&mut d, "color-picker-source", "Selected layer", false);
        d.input.perform(serde_json::json!([{"point":[820,479]}]));
        assert!(
            d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .color_picker_overlay()
                .unwrap()
                .classic
        );
        d.capture_canvas(&format!("color-picker-category-{preset:?}.png"));
        d.click(&find_named(&d.named("tool-drawer"), "layer-color-picker-symbolic").unwrap());
        assert_eq!(
            state(&d.w).color_picker.style,
            layer_ui::ColorPickerStyle::Glass
        );
        picker_dropdown(&mut d, "color-picker-source", "Visible color", false);
        d.input.key(0xff1b);
        assert!(state(&d.w).customization.drawer.is_none());
    }
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(sketch_workspace),
    });
    pump(500);
    // Pen contact and dragging continue previewing; only release commits.
    d.input.key('i' as u32);
    d.input
        .perform(serde_json::json!([{"pen":"move","point":[860,520]}]));
    assert!(state(&d.w).color_picker.preview.is_some());
    let pen_before = state(&d.w).colors.clone();
    d.input
        .perform(serde_json::json!([{"pen":"down","point":[860,520]}]));
    assert!(state(&d.w).layer_tools.tool.picks_color());
    assert_eq!(state(&d.w).colors, pen_before);
    d.input
        .perform(serde_json::json!([{"pen":"move","point":[780,440]}]));
    assert_eq!(state(&d.w).colors, pen_before);
    assert!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .color_picker_overlay()
            .is_some()
    );
    d.input
        .perform(serde_json::json!([{"pen":"up"},{"pen":"leave"}]));
    assert_ne!(state(&d.w).colors, pen_before);
    assert_eq!(state(&d.w).layer_tools.tool, previous);
    assert_eq!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        revision
    );
    // A touch tap cancels button entry, whereas a touch hold picks on release.
    let before = state(&d.w).colors.clone();
    d.input.key('i' as u32);
    d.input
        .perform(serde_json::json!([{"touch":"down","point":[820,479]},{"touch":"up"}]));
    assert_eq!(state(&d.w).colors, before);
    assert_eq!(state(&d.w).layer_tools.tool, previous);
    d.input.perform(serde_json::json!([{"touch":"down","point":[820,529]},{"wait_ms":700},{"touch":"move","point":[860,530]}]));
    let ring =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .color_picker_overlay()
            .unwrap();
    assert_eq!(ring.center, ring.sample);
    assert!(ring.center[1] < 500.);
    assert_eq!(state(&d.w).colors, before);
    d.capture_canvas("color-picker-touch.png");
    let camera = state(&d.w).camera;
    d.input
        .perform(serde_json::json!([{"touch":"down","slot":1,"point":[1000,600]}]));
    assert!(state(&d.w).color_picker.layer);
    assert!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .color_picker_overlay()
            .unwrap()
            .layer
    );
    d.capture_canvas("color-picker-touch-layer.png");
    d.input.perform(
        serde_json::json!([{"touch":"move","slot":1,"point":[900,580]},{"touch":"up","slot":1}]),
    );
    assert_eq!(state(&d.w).camera, camera);
    d.input.perform(serde_json::json!([{"touch":"up"}]));
    assert_eq!(state(&d.w).layer_tools.tool, previous);
    assert_eq!(
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        revision
    );
    // Moving before the native hold prevents picker activation.
    d.input.perform(serde_json::json!([{"touch":"down","point":[650,400]},{"touch":"move","point":[720,400]},{"wait_ms":700}]));
    assert!(!state(&d.w).layer_tools.tool.picks_color());
    d.input.perform(serde_json::json!([{"touch":"up"}]));
    // A resting palm predates a toolbar press. Neither its pending hold nor
    // its eventual release may acquire picker ownership or survive as a
    // navigation contact. Exercise real mouse and tablet toolbar activation.
    for pen in [false, true] {
        let tile = d.point(&d.named(&button));
        let press = if pen { serde_json::json!({"pen":"down","point":tile}) }
            else { serde_json::json!({"point":tile,"down":true}) };
        let release = if pen { serde_json::json!({"pen":"up"}) }
            else { serde_json::json!({"down":false}) };
        d.input.perform(serde_json::json!([
            {"touch":"down","point":[650,400]}, press, release, {"wait_ms":700},
            {"pen":"move","point":[820,479]}
        ]));
        assert!(state(&d.w).layer_tools.tool.picks_color());
        let ring = d.w.gpu.borrow().as_ref().unwrap().session.color_picker_overlay().unwrap();
        let scale = d.w.area.scale_factor() as f32;
        assert_eq!(ring.sample, [820. * scale, 479. * scale], "old hold must not own the picker");
        d.input.perform(serde_json::json!([{"touch":"up"},{"pen":"leave"}]));
        d.input.key('i' as u32);
        let camera = state(&d.w).camera;
        for _ in 0..2 {
            d.input.perform(serde_json::json!([
                {"touch":"down","point":[850,600]},
                {"touch":"move","point":[920,650]}, {"touch":"up"}
            ]));
            assert_eq!(state(&d.w).camera, camera, "released palm must not become a ghost finger");
        }
        d.input.perform(serde_json::json!([
            {"touch":"down","point":[650,400]},
            {"touch":"down","slot":1,"point":[850,400]},
            {"touch":"move","slot":1,"point":[920,470]},
            {"touch":"up","slot":1}, {"touch":"up"}
        ]));
        assert_ne!(state(&d.w).camera.zoom, camera.zoom);
        assert_ne!(state(&d.w).camera.rotation, camera.rotation);
        d.w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    }
    // Retained Color panels show a reversible preview without changing paint.
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Color,
            visible: true,
        },
    });
    pump(400);
    assert!(d.w.color_panel.root.is_mapped());
    let wheel = find_named(d.w.color_panel.root.upcast_ref(), "color-wheel")
        .unwrap()
        .downcast::<crate::tool_panels::ColorWheel>()
        .unwrap();
    let committed = state(&d.w).colors.clone();
    d.input.key('i' as u32);
    d.input.perform(serde_json::json!([{"point":[820,479]}]));
    assert_eq!(*wheel.imp().color.borrow(), *state(&d.w).preview_colors());
    assert_eq!(state(&d.w).colors, committed);
    d.capture_canvas("color-picker-wheel-preview.png");
    let stats =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .backend()
            .stats
            .clone();
    *stats.lock().unwrap() = Default::default();
    let events: Vec<_> = (0..60)
        .map(|i| serde_json::json!({"point":[740 + i * 2,480]}))
        .collect();
    d.input.perform(serde_json::Value::Array(events));
    {
        let timings = stats.lock().unwrap();
        let mut cpu = timings.frame_handler_cpu.clone();
        cpu.sort_by(f64::total_cmp);
        let mut worker: Vec<_> = timings
            .thread_cpu
            .iter()
            .map(|row| row[1..].iter().sum::<f64>())
            .collect();
        worker.sort_by(f64::total_cmp);
        assert!(!cpu.is_empty() && !worker.is_empty());
        let report = serde_json::json!({"owner_frame_ms_p50":cpu[cpu.len()/2], "owner_frame_ms_p95":cpu[cpu.len()*95/100],
            "worker_cpu_ms_p50":worker[worker.len()/2], "worker_cpu_ms_p95":worker[worker.len()*95/100], "frames":worker.len()});
        eprintln!("Picker hover timing: {report}");
        std::fs::write(
            d.input.dir.join("color-picker-timing.json"),
            report.to_string(),
        )
        .unwrap();
    }
    d.input.key(0xff1b);
    assert_eq!(*wheel.imp().color.borrow(), committed);
    // The new button retains the application-wide hold-before-reorder rule.
    for kind in ["mouse", "touch", "pen"] {
        let before = state(&d.w).workspace.layout;
        let point = d.point(&d.named(&button));
        let moved = [point[0] + 120., point[1]];
        let event = |phase: &str, p: [f32; 2]| contact(kind, phase, p);
        d.input.perform(serde_json::json!([
            event("down", point),
            event("move", moved)
        ]));
        assert!(
            !d.w.workspace_drag
                .borrow()
                .as_ref()
                .is_some_and(|v| v.started),
            "unheld {kind}"
        );
        d.input.perform(serde_json::json!([event("up", moved)]));
        assert_eq!(state(&d.w).workspace.layout, before);
        d.input.perform(
            serde_json::json!([event("down", point),{"wait_ms":700},event("move", moved)]),
        );
        assert!(
            d.w.workspace_drag
                .borrow()
                .as_ref()
                .is_some_and(|v| v.started),
            "held {kind}"
        );
        d.input.key(0xff1b);
        d.input.perform(serde_json::json!([event("up", moved)]));
        if kind == "pen" {
            d.input.perform(serde_json::json!([{"pen":"leave"}]));
        }
        assert_eq!(state(&d.w).workspace.layout, before);
        assert!(!state(&d.w).layer_tools.tool.picks_color());
    }
    // Focus moving to another toplevel still cancels temporary picking.
    d.input.key('i' as u32);
    assert!(state(&d.w).layer_tools.tool.picks_color());
    let other = gtk::Window::builder()
        .title("Picker focus check")
        .default_width(320)
        .default_height(200)
        .child(&gtk::Button::with_label("Another window"))
        .build();
    other.present();
    pump(500);
    assert!(other.is_active());
    assert!(!state(&d.w).layer_tools.tool.picks_color());
    other.destroy();
    d.w.window.present();
    pump(200);
    assert!(!d.w.status.is_visible(), "{}", d.w.status.text());
    d.w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_color_picker_preview_pacing at LAYER_NATIVE_EVENT_MS=8"]
fn native_color_picker_preview_pacing() {
    let mut d = Driver::new("art.capycanvas.PickerPacing");
    picker_artwork(&mut d);
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.78, 0.58, 0.23, 1.],
    });
    let stats =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .backend()
            .stats
            .clone();
    let clock = d.w.window.frame_clock().unwrap();
    let paint_start = Rc::new(Cell::new(Instant::now()));
    let paints = Rc::new(RefCell::new(Vec::<f64>::new()));
    let before = clock.connect_before_paint(glib::clone!(
        #[strong]
        paint_start,
        move |_| paint_start.set(Instant::now())
    ));
    let after = clock.connect_after_paint(glib::clone!(
        #[strong]
        paint_start,
        #[strong]
        paints,
        move |_| paints
            .borrow_mut()
            .push(paint_start.get().elapsed().as_secs_f64() * 1000.)
    ));
    let mut reports = Vec::new();
    let percentile = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        values.get(values.len() * 95 / 100).copied().unwrap_or(0.)
    };
    for (depth, placement) in [
        ("SDR", "closed"),
        ("SDR", "docked"),
        ("HDR", "closed"),
        ("HDR", "docked"),
    ] {
        if depth == "HDR" && !state(&d.w).colors.hdr_depth().is_float() {
            use super::super::new_photo::{combo, finish, invoke, ready, response};
            invoke(&d.w, CommandId::ChangeBitDepth);
            combo(&d.w, "document-color-depth").set_selected(2);
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                pump(20);
                let dialog =
                    d.w.window
                        .visible_dialog()
                        .unwrap()
                        .downcast::<adw::AlertDialog>()
                        .unwrap();
                if dialog.is_response_enabled("apply") {
                    break;
                }
                assert!(Instant::now() < deadline);
            }
            response(&d.w, "apply");
            finish(&d.w);
            ready(&d.w);
            assert!(state(&d.w).colors.hdr_depth().is_float());
        }
        let committed = state(&d.w).colors.clone();
        d.w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible {
                panel: Panel::Color,
                visible: placement == "docked",
            },
        });
        pump(400);
        let wheel = find_named(d.w.color_panel.root.upcast_ref(), "color-wheel")
            .unwrap()
            .downcast::<crate::tool_panels::ColorWheel>()
            .unwrap();
        d.input.key('i' as u32);
        d.input.perform(serde_json::json!([{"point":[750,445]}]));
        *stats.lock().unwrap() = Default::default();
        wheel.imp().field_render_ms.borrow_mut().clear();
        wheel.imp().snapshot_ms.borrow_mut().clear();
        wheel.imp().refresh_ms.borrow_mut().clear();
        paints.borrow_mut().clear();
        // Continuous motion across the painted hues, rather than repeated jumps
        // between identical colors that can alias with a throttled preview.
        let events: Vec<_> = (0..400)
            .map(|i| {
                let phase = (i as f64 * 0.093).sin();
                serde_json::json!({"point": [820. + phase*85., 487. + (i as f64*0.071).cos()*45.]})
            })
            .collect();
        let started = Instant::now();
        d.input.perform(serde_json::Value::Array(events));
        let elapsed = started.elapsed().as_secs_f64();
        let samples = stats.lock().unwrap();
        let mut presented: Vec<_> = samples
            .presented
            .iter()
            .filter(|p| p[3] == 1)
            .map(|p| p[1])
            .collect();
        presented.sort_unstable();
        let gaps: Vec<_> = presented
            .windows(2)
            .map(|p| (p[1] - p[0]) as f64 / 1e6)
            .collect();
        let span = presented.last().unwrap() - presented.first().unwrap();
        let renders = wheel.imp().field_render_ms.borrow();
        let report = serde_json::json!({"depth":depth,"placement":placement,"scale":wheel.scale_factor(),"seconds":elapsed,
            "canvas_fps": (presented.len()-1) as f64 * 1e9 / span as f64,
            "canvas_gap_p95_ms":percentile(gaps),"canvas_wake_lateness_p95_ms":percentile(samples.wake_lateness.clone()),
            "gtk_paint_p95_ms":percentile(paints.borrow().clone()),
            "wheel_snapshot_p95_ms":percentile(wheel.imp().snapshot_ms.borrow().clone()),
            "wheel_refresh_p95_ms":percentile(wheel.imp().refresh_ms.borrow().clone()),
            "field_cpu_ms":renders.iter().sum::<f64>(),"field_rebuilds":renders.len(),
            "field_p95_ms":percentile(renders.clone())});
        eprintln!("Picker presentation: {report}");
        reports.push(report);
        drop(renders);
        drop(samples);
        if placement != "closed" {
            assert_eq!(*wheel.imp().color.borrow(), *state(&d.w).preview_colors());
            assert_eq!(
                wheel.imp().disc.borrow().as_ref().unwrap().1,
                state(&d.w).preview_colors().wheel_components()[0],
                "last preview field must finish"
            );
        }
        assert_eq!(state(&d.w).colors, committed);
        d.input.key(0xff1b);
        if placement != "closed" {
            assert_eq!(*wheel.imp().color.borrow(), committed);
            assert_eq!(
                wheel.imp().disc.borrow().as_ref().unwrap().1,
                committed.wheel_components()[0],
                "cancel restores the field as well as its marker"
            );
        }
        d.w.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        });
    }
    clock.disconnect(before);
    clock.disconnect(after);
    // The color-space badge belongs to the same optional footer as canvas info.
    d.edit();
    let checkbox = d
        .named("header-canvas-info")
        .downcast::<gtk::CheckButton>()
        .unwrap();
    checkbox.set_active(true);
    pump(100);
    assert!(d.w.hdr_status.is_visible());
    d.click(checkbox.upcast_ref());
    assert!(!d.w.hdr_status.is_visible());
    assert!(!d.w.view_info.is_visible());
    crate::snapshot(&d.w)
        .save_to_png(d.input.dir.join("color-picker-footer-hidden.png"))
        .unwrap();
    d.w.dispatch(UiAction::SetColor {
        rgba: [0.4, 0.3, 0.8, 1.],
    });
    assert!(
        !d.w.hdr_status.is_visible(),
        "color updates must respect the footer preference"
    );
    d.click(checkbox.upcast_ref());
    assert!(d.w.hdr_status.is_visible());
    crate::snapshot(&d.w)
        .save_to_png(d.input.dir.join("color-picker-footer-visible.png"))
        .unwrap();
    d.click_name("header-edit-done");
    std::fs::write(
        d.input.dir.join("color-picker-preview-timing.json"),
        serde_json::to_string_pretty(&reports).unwrap(),
    )
    .unwrap();
    assert!(!d.w.status.is_visible(), "{}", d.w.status.text());
    d.finish();
}
