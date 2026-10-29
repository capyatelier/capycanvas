// Compositor recordings of workspace motion, analysed by tools/visual/ui_motion.py.
use super::*;

#[test]
#[ignore = "isolated Mutter recording; LAYER_NATIVE_CAPTURE_DIR=<dir> workspace-motion.sh gtk --native-test=native_ui_motion_recording --native-storage"]
fn native_ui_motion_recording() {
    let mut input = RemoteInput::new().settle_ms(1200).timeout_secs(30);
    let app = native_test_app("art.capycanvas.UiMotion");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(true);
    let w = Workspace::new(&app);
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(1500);
    stripes(&w);
    input.ready();
    pump(500);
    let mut record = |name: &str, widget: &gtk::Widget| {
        let point = screen_point(widget, &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([
            {"point": point},
            {"wait_ms": 300},
            {"record_start": name},
            {"down": true},
            {"down": false},
            {"wait_ms": 1200},
            {"record_stop": true},
        ]));
        until(|| !w.workspaces.busy(), "workspace settled");
    };
    let switcher = |name: &str| find_named(w.header.root.upcast_ref(), &format!("workspace-switch-{name}")).unwrap();
    let column_icon = |panel: Panel| {
        let resolved = w.resolved();
        let column = resolved.collapsed.iter().find(|c| c.groups.iter().any(|g| g.icons.iter().any(|i| i.panel == panel))).unwrap();
        w.columns.button(column.id, panel).unwrap().upcast::<gtk::Widget>()
    };
    record("switch-to-paint", &switcher("illustrator"));
    record("close-column", &column_icon(Panel::Layers));
    record("open-column", &column_icon(Panel::Layers));
    let proof = w.groups.borrow().iter().flat_map(|g| g.tabs.iter()).find(|(p, _)| *p == Panel::Proof).unwrap().1.clone();
    record("select-tab", proof.upcast_ref());
    record("switch-to-photo", &switcher("photographer"));
    record("switch-to-sketch", &switcher("painter"));
    record("switch-back-to-paint", &switcher("illustrator"));
    input.finish();
}

fn stripes(w: &Rc<Workspace>) {
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    for _ in 0..3 {
        w.dispatch(UiAction::Invoke { command: CommandId::ZoomIn });
    }
    pump(300);
    let [a, b, c, e, x0, y0] = state(w).camera.document_to_surface();
    let det = a * e - b * c;
    let document = |x: f32, y: f32| {
        let (x, y) = (x - x0, y - y0);
        [(e * x - c * y) / det, (a * y - b * x) / det]
    };
    let [width, height] = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::GPen as u32 });
    w.dispatch(UiAction::SetBrushSize { value: 6. / det.abs().sqrt() });
    w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
    let mut x = 4.;
    while x < width {
        native_pen_path(w, &[document(x, 0.), document(x, height * 0.5), document(x, height)]);
        x += 13.;
    }
    pump(500);
}

#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_panel_tab_feedback_preserves_join"]
fn native_panel_tab_feedback_preserves_join() {
    let mut input = RemoteInput::new().settle_ms(30).timeout_secs(10);
    let app = native_test_app("art.capycanvas.PanelTabFeedback");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let group = state(&w).workspace.layout.panel_group(Panel::Layers).unwrap();
    let tabs = w.groups.borrow().iter().find(|g| g.id == group).unwrap().tabs.clone();
    assert!(tabs.len() >= 2);
    let strip = tabs[0].1.parent().unwrap();
    let bottom = || {
        let texture = icons::capture_widget(&w.window, &strip);
        let stride = texture.width() as usize * 4;
        let mut pixels = vec![0; stride * texture.height() as usize];
        texture.download(&mut pixels, stride);
        pixels.split_off((texture.height() as usize - 6 * strip.scale_factor() as usize) * stride)
    };
    input.ready();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Low, layer_ui::Transparency::Medium, layer_ui::Transparency::High] {
            use_transparency(&w, level);
            for active in 0..2 {
                for device in ["mouse", "touch"] {
                    input.perform(serde_json::json!([{"point": [800., 500.]}]));
                    w.dispatch(UiAction::SelectPanelTab { group, panel: tabs[active].0 });
                    pump(200);
                    let _ = bottom();
                    until(|| w.chrome_released.get() == w.chrome.revision(), "panel chrome published");
                    let expected = bottom();
                    let next = &tabs[1 - active];
                    let edge = tabs[active].1.compute_bounds(&strip).unwrap();
                    let x = if active == 0 { edge.x() + edge.width() } else { edge.x() - 1. };
                    let join = |pixels: &[u8]| {
                        let scale = strip.scale_factor() as usize;
                        let stride = strip.width() as usize * scale * 4;
                        let i = (6 * scale - 1) * stride + x.round() as usize * scale * 4;
                        pixels[i..i + 4].to_vec()
                    };
                    let point = screen_point(next.1.upcast_ref(), &w.window, [0.5, 0.5]);
                    if device == "mouse" {
                        input.perform(serde_json::json!([{"point": point}]));
                        assert!(next.1.state_flags().contains(gtk::StateFlags::PRELIGHT));
                        let hovered = bottom();
                        assert!(hovered != expected, "{theme:?}/{level:?}/{active}: inactive tab has hover feedback");
                        let (hovered_join, expected_join) = (join(&hovered), join(&expected));
                        assert!(hovered_join.iter().zip(&expected_join).all(|(a, b)| a.abs_diff(*b) <= 1), "{theme:?}/{level:?}/{active}: hover stays behind the active shoulder: {hovered_join:?}/{expected_join:?}");
                    }
                    input.perform(serde_json::json!([contact(device, "down", point)]));
                    assert!(next.1.has_css_class("selected-tool"), "{device} press selects before release");
                    assert_eq!(next.1.cursor().and_then(|cursor| cursor.name()).as_deref(), Some("default"));
                    assert!(state(&w).customization.expanded.is_none());
                    input.perform(serde_json::json!([contact(device, "up", point)]));
                    pump(200);
                    assert!(next.1.has_css_class("selected-tool"), "{device} click selects the inactive tab");
                    assert!(state(&w).customization.expanded.is_none(), "release does not configure the selected tab");
                    assert_eq!(next.1.parent().as_ref(), Some(&strip), "pressed tab retains its native parent");
                }
            }
        }
    }
    let original = layer_ui::WorkspaceState::default();
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Low, layer_ui::Transparency::Medium, layer_ui::Transparency::High] {
            use_transparency(&w, level);
            for device in ["mouse", "touch"] {
                w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(original.clone()) });
                let group = original.layout.panel_group(Panel::Layers).unwrap();
                w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
                enable_individual_column_panels(&w, group);
                w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: Panel::Layers } });
                pump(300);
                let tab = named::<gtk::Button>(w.window.upcast_ref(), "column-drawer-tab-Adjustments");
                let parent = tab.parent().unwrap();
                until(|| {
                    let point = screen_point(tab.upcast_ref(), &w.window, [0.5, 0.5]);
                    tab.is_mapped() && point[0] > 0. && point[0] < viewport[0]
                        && point[1] > 0. && point[1] < viewport[1]
                }, "drawer tab reveal settles inside viewport");
                let point = screen_point(tab.upcast_ref(), &w.window, [0.5, 0.5]);
                if device == "mouse" {
                    input.perform(serde_json::json!([{"point": point}]));
                    until(|| tab.state_flags().contains(gtk::StateFlags::PRELIGHT), "drawer tab receives pointer hover");
                }
                input.perform(serde_json::json!([contact(device, "down", point)]));
                assert!(tab.has_css_class("selected-tool"), "{theme:?}/{level:?}/{device}: drawer selects on press at {point:?}; drawers={:?}; bounds={:?}", state(&w).customization.column_drawers, tab.compute_bounds(&w.window));
                assert_eq!(named::<gtk::Button>(w.window.upcast_ref(), "column-drawer-tab-Adjustments"), tab, "drawer retains selected button");
                assert_eq!(tab.parent().as_ref(), Some(&parent), "drawer retains header parent");
                assert_eq!(tab.cursor().and_then(|cursor| cursor.name()).as_deref(), Some("default"));
                assert!(state(&w).customization.expanded.is_none());
                let moved = [point[0] + 24., point[1]];
                input.perform(serde_json::json!([contact(device, "move", moved)]));
                assert!(w.workspace_drag.borrow().as_ref().is_some_and(|drag| drag.started && drag.tab.is_some()), "{theme:?}/{level:?}/{device}: selecting drawer tab retains same-contact drag");
                input.perform(serde_json::json!([
                    {"key":65307,"down":true}, {"key":65307,"down":false},
                    contact(device, "up", moved),
                ]));
                assert!(w.workspace_drag.borrow().is_none());
                assert!(state(&w).customization.expanded.is_none(), "drawer release does not configure");
            }
        }
    }
    input.finish();
}

#[test]
#[ignore = "isolated Mutter compositor captures; LAYER_NATIVE_CAPTURE_DIR=<dir> workspace-motion.sh gtk --native-test=native_active_tab_matches_panel"]
fn native_active_tab_matches_panel() {
    let captures = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_CAPTURE_DIR").expect("compositor captures"));
    let mut input = RemoteInput::new().settle_ms(300).timeout_secs(30);
    let app = native_test_app("art.capycanvas.ActiveTabShade");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    input.ready();
    pump(300);
    let origin = w.surface.compute_point(&w.window, &gtk::graphene::Point::zero()).unwrap();
    let (window_x, window_y) = w.window.surface_transform();
    let offset = [origin.x() + window_x as f32, origin.y() + window_y as f32];
    let mut mismatches = Vec::new();
    let mut reports = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Low, layer_ui::Transparency::Medium, layer_ui::Transparency::High] {
            use_transparency(&w, level);
            pump(400);
            let name = format!("tab-{theme:?}-{level:?}").to_lowercase();
            input.perform(serde_json::json!([{ "wait_ms": 300 }, { "capture": name }]));
            let texture = gdk::Texture::from_filename(captures.join(format!("{name}.png"))).unwrap();
            let stride = texture.width() as usize * 4;
            let mut pixels = vec![0; stride * texture.height() as usize];
            texture.download(&mut pixels, stride);
            let monitor=w.area.display().monitor_at_surface(&w.window.surface().unwrap()).unwrap().geometry();
            let scale=[texture.width() as f32/monitor.width() as f32,texture.height() as f32/monitor.height() as f32];
            let sample = |x: f32, y: f32| {
                let x=((x+offset[0]-monitor.x() as f32)*scale[0]).floor() as usize;
                let y=((y+offset[1]-monitor.y() as f32)*scale[1]).floor() as usize;
                let i = y * stride + x * 4;
                [pixels[i + 2], pixels[i + 1], pixels[i]]
            };
            for group in w.groups.borrow().iter().filter(|g| g.tabs_visible) {
                let Some((_, tab)) = group.tabs.iter().find(|(_, tab)| tab.has_css_class("selected-tool")) else { continue };
                let tab = tab.compute_bounds(&w.surface).unwrap();
                let body = group.stack.compute_bounds(&w.surface).unwrap();
                let tab_pixel = sample(tab.x() + 4., tab.y() + tab.height() - 3.);
                let body_pixel = sample(body.x() + 3., body.y() + 3.);
                reports.push(serde_json::json!({"case":name,"group":group.id,"scale":scale,"tab":tab_pixel,"body":body_pixel}));
                if tab_pixel.into_iter().zip(body_pixel).any(|(tab, body)| tab.abs_diff(body) > 1) {
                    mismatches.push(format!("{name} group {}: tab {tab_pixel:?} panel {body_pixel:?}", group.id));
                }
            }
        }
    }
    std::fs::write(captures.join("tab-body-pixels.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    input.finish();
    assert!(mismatches.is_empty(), "active tabs differ from their panels:\n{}", mismatches.join("\n"));
}

#[test]
#[ignore = "isolated Mutter captures; LAYER_NATIVE_CAPTURE_DIR=<dir> workspace-motion.sh gtk --native-test=native_workspace_switch_repaints_vacated_regions --native-storage"]
fn native_workspace_switch_repaints_vacated_regions() {
    let captures = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_CAPTURE_DIR").expect("compositor captures"));
    let mut input = RemoteInput::new().settle_ms(1200).timeout_secs(30);
    let app = native_test_app("art.capycanvas.VacatedRegions");
    let w = Workspace::new(&app);
    apply_fixture_theme(&w);
    w.window.maximize();
    w.window.present();
    wait_workspaces(&w);
    pump(1500);
    stripes(&w);
    input.ready();
    pump(500);
    let mut switch = |name: &str| {
        let button = find_named(w.header.root.upcast_ref(), &format!("workspace-switch-{name}")).unwrap();
        let point = screen_point(&button, &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point": point}, {"wait_ms": 300}, {"down": true}, {"down": false}, {"wait_ms": 1200}]));
        until(|| !w.workspaces.busy(), "workspace settled");
    };
    switch("photographer");
    switch("painter");
    input.perform(serde_json::json!([{"wait_ms": 300}, {"capture": "switched"}]));
    w.window.unmaximize();
    pump(800);
    w.window.maximize();
    pump(1500);
    input.perform(serde_json::json!([{"wait_ms": 300}, {"capture": "repainted"}]));
    input.finish();
    let pixels = |name: &str| {
        let texture = gdk::Texture::from_filename(captures.join(format!("{name}.png"))).unwrap();
        let stride = texture.width() as usize * 4;
        let mut bytes = vec![0; stride * texture.height() as usize];
        texture.download(&mut bytes, stride);
        (bytes, stride)
    };
    let ((switched, stride), (repainted, _)) = (pixels("switched"), pixels("repainted"));
    let below_header = (w.header.height() as usize + 8) * stride;
    let stale: Vec<_> = (below_header..switched.len())
        .step_by(4)
        .filter(|&i| (0..3).any(|c| switched[i + c].abs_diff(repainted[i + c]) > 2))
        .map(|i| [(i % stride) / 4, i / stride])
        .collect();
    let extent = stale.iter().fold(None, |e: Option<[usize; 4]>, &[x, y]| {
        Some(e.map_or([x, y, x, y], |[x0, y0, x1, y1]| [x0.min(x), y0.min(y), x1.max(x), y1.max(y)]))
    });
    assert!(stale.is_empty(), "{} pixels kept the previous workspace after the switch, within {extent:?}", stale.len());
}
