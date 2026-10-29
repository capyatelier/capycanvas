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
#[ignore = "private compositor captures and native input"]
fn native_panel_tab_drag_matches_rest() {
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    let captures = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").unwrap());
    let app = native_test_app("art.capycanvas.PanelTabDragPixels");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let original = layer_ui::WorkspaceState::default();
    let group = original.layout.panel_group(Panel::Layers).unwrap();
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(15);
    input.ready();
    let mut reports = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Medium] {
            use_transparency(&w, level);
            for docked in [false, true] {
                for device in if docked { &["mouse"][..] } else { &["mouse", "touch"][..] } {
                    let panel = Panel::Layers;
                    let name = format!("{theme:?}-{level:?}-{device}-{}", if docked { "docked" } else { "floating" })
                        .to_lowercase();
                    let mut fixture = original.clone();
                    if docked { fixture.layout.set_column_collapsed(group, true, viewport).unwrap(); }
                    let column = docked.then(|| fixture.layout.collapsed_column_for_group(group).unwrap());
                    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
                    pump(200);
                    if !docked {
                        w.dispatch(UiAction::MoveGroup {
                            group,
                            target: DockTarget::Float { position: [500., 220.] },
                            viewport,
                        });
                        let mut fixture = state(&w).workspace;
                        let floating = fixture.layout.floating.iter_mut().find(|f| f.root.id() == group).unwrap();
                        floating.width = 300.;
                        floating.height = Some(360.);
                        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
                    } else {
                        let button = w.columns.button(column.unwrap(), Panel::Layers).unwrap();
                        input.click(screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
                        until(|| {
                            let groups = w.groups.borrow();
                            groups.iter().find(|g| g.id == group).is_some_and(|view|
                                view.root.has_css_class("gpu-panel-surface") && view.tabs_visible
                                    && view.tabs.iter().any(|(p, tab)| *p == panel && tab.is_mapped()))
                        }, "docked tab uses the GPU PanelSurface");
                    }
                    w.dispatch(UiAction::SelectPanelTab { group, panel: Panel::Properties });
                    pump(200);
                    let reference_bounds = w
                        .groups
                        .borrow()
                        .iter()
                        .find(|g| g.id == group)
                        .unwrap()
                        .tab_strip
                        .compute_bounds(&w.surface)
                        .unwrap();
                    input.perform(
                        serde_json::json!([{"point":[10.,10.]},{"wait_ms":180},{"capture":format!("strip-{name}")} ]),
                    );
                    w.dispatch(UiAction::SelectPanelTab { group, panel });
                    pump(250);
                    let tab = w
                        .groups
                        .borrow()
                        .iter()
                        .find(|g| g.id == group)
                        .unwrap()
                        .tabs
                        .iter()
                        .find(|(p, _)| *p == panel)
                        .unwrap()
                        .1
                        .clone();
                    let strip = tab.parent().unwrap();
                    let band = strip.compute_bounds(&w.surface).unwrap();
                    let bounds = tab.compute_bounds(&w.surface).unwrap();
                    assert_eq!(reference_bounds, band, "strip reference retains the same native geometry");
                    let content = gtk::Snapshot::new();
                    let parent_origin = strip.compute_point(&w.surface, &gtk::graphene::Point::zero()).unwrap();
                    content.translate(&parent_origin);
                    strip.snapshot_child(&tab, &content);
                    let source_content = content.to_node().unwrap();
                    let start = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() * 0.5];
                    input.perform(
                        serde_json::json!([{"point":start},{"wait_ms":180},{"capture":format!("rest-{name}")} ]),
                    );
                    input.perform(serde_json::json!([
                        contact(device, "down", start),
                        contact(device, "move", [start[0] + 40., start[1]])
                    ]));
                    until(
                        || w.workspace_drag.borrow().as_ref().is_some_and(|d| d.started && d.tab.is_some()),
                        "native panel tab pickup",
                    );
                    let shifted = w.workspace_drag.borrow().as_ref().unwrap().tab.as_ref().unwrap().bounds;
                    assert!(
                        shifted.x - bounds.x() > 34.,
                        "native pickup vacates the strip beyond the fixed outer shadow"
                    );
                    input.perform(serde_json::json!([{"wait_ms":180},{"capture":format!("shifted-{name}")} ]));
                    let scale = w.window.surface().unwrap().scale() as f32;
                    let (blank_pixels, checked, unexpected_content) =
                        assert_tab_preview_content(&w, &source_content, bounds, shifted, scale, &output, &name);
                    input.perform(serde_json::json!([contact(device, "move", start)]));
                    pump(250);
                    let held = w.workspace_drag.borrow().as_ref().unwrap().tab.as_ref().unwrap().bounds;
                    assert!(
                        (held.x - bounds.x()).abs() <= 1. && (held.y - bounds.y()).abs() <= 1.,
                        "compare the same resting and held geometry"
                    );
                    input.perform(serde_json::json!([{"wait_ms":180},{"capture":format!("held-{name}")} ]));
                    let rest = gdk::Texture::from_filename(captures.join(format!("rest-{name}.png"))).unwrap();
                    let held = gdk::Texture::from_filename(captures.join(format!("held-{name}.png"))).unwrap();
                    let moving = gdk::Texture::from_filename(captures.join(format!("shifted-{name}.png"))).unwrap();
                    let strip_reference =
                        gdk::Texture::from_filename(captures.join(format!("strip-{name}.png"))).unwrap();
                    assert_eq!((rest.width(), rest.height()), (held.width(), held.height()));
                    let monitor = w.area.display().monitor_at_surface(&w.window.surface().unwrap()).unwrap().geometry();
                    let scale =
                        [rest.width() as f32 / monitor.width() as f32, rest.height() as f32 / monitor.height() as f32];
                    let origin = w.surface.compute_point(&w.window, &gtk::graphene::Point::zero()).unwrap();
                    let (wx, wy) = w.window.surface_transform();
                    let point = |x: f32, y: f32| {
                        [
                            (x + origin.x() + wx as f32 - monitor.x() as f32) * scale[0],
                            (y + origin.y() + wy as f32 - monitor.y() as f32) * scale[1],
                        ]
                    };
                    let a = point(band.x(), band.y());
                    let b = point(band.x() + band.width(), band.y() + band.height() + 2.);
                    let crop = [
                        a[0].floor().max(0.) as usize,
                        a[1].floor().max(0.) as usize,
                        b[0].ceil().min(rest.width() as f32) as usize,
                        b[1].ceil().min(rest.height() as f32) as usize,
                    ];
                    let width = crop[2] - crop[0];
                    let height = crop[3] - crop[1];
                    let pixels = |texture: &gdk::Texture, label: &str| {
                        let stride = texture.width() as usize * 4;
                        let mut full = vec![0; stride * texture.height() as usize];
                        texture.download(&mut full, stride);
                        let cropped = (crop[1]..crop[3])
                            .flat_map(|y| full[y * stride + crop[0] * 4..y * stride + crop[2] * 4].iter().copied())
                            .collect::<Vec<_>>();
                        gdk::MemoryTexture::new(
                            width as i32,
                            height as i32,
                            gdk::MemoryFormat::B8g8r8a8Premultiplied,
                            &glib::Bytes::from_owned(cropped.clone()),
                            width * 4,
                        )
                        .save_to_png(output.join(format!("{name}-{label}.png")))
                        .unwrap();
                        cropped
                    };
                    let before = pixels(&rest, "rest");
                    let after = pixels(&held, "held");
                    let moving = pixels(&moving, "shifted");
                    let strip_reference = pixels(&strip_reference, "strip");
                    let at = |bytes: &[u8], x: f32, y: f32| {
                        let p = point(x, y);
                        let i = ((p[1].floor() as usize - crop[1]) * width + p[0].floor() as usize - crop[0]) * 4;
                        bytes[i..i + 4].to_vec()
                    };
                    let delta =
                        |a: Vec<u8>, b: Vec<u8>| a.into_iter().zip(b).map(|(a, b)| a.abs_diff(b)).max().unwrap();
                    let x = bounds.x() + bounds.width() - 4.;
                    let resting_blank = at(&before, x, bounds.y() + bounds.height() - 3.);
                    let moving_blank = at(&moving, x + shifted.x - bounds.x(), shifted.y + shifted.height - 3.);
                    let shifted_interior = delta(resting_blank.clone(), moving_blank.clone());
                    let vacated_reference = at(&strip_reference, bounds.x() + 28., bounds.y() + bounds.height() - 3.);
                    let vacated_observed = at(&moving, bounds.x() + 28., bounds.y() + bounds.height() - 3.);
                    let vacated = delta(vacated_reference.clone(), vacated_observed.clone());
                    let left = point(bounds.x() - 6., bounds.y())[0].floor().max(crop[0] as f32) as usize - crop[0];
                    let right = point(bounds.x() + bounds.width() + 6., bounds.y())[0]
                        .ceil().min(crop[2] as f32) as usize - crop[0];
                    let maximum = (0..height).flat_map(|y| (y * width + left) * 4..(y * width + right) * 4)
                        .map(|i| before[i].abs_diff(after[i])).max().unwrap_or(0);
                    let center = point(bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() - 3.);
                    let center =
                        ((center[1].floor() as usize - crop[1]) * width + center[0].floor() as usize - crop[0]) * 4;
                    let interior = before[center..center + 4]
                        .iter()
                        .zip(&after[center..center + 4])
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap();
                    reports.push(serde_json::json!({"case":name,"source_and_feet_byte_delta":maximum,"interior_byte_delta":interior,
                    "shifted_interior_byte_delta":shifted_interior,"vacated_strip_byte_delta":if docked {None} else {Some(vacated)},
                    "blank_reference_pixels":blank_pixels,"checked_content_pixels":checked,"unexpected_content_pixels":unexpected_content}));
                    std::fs::write(output.join("tab-drag-rest.json"), serde_json::to_vec_pretty(&reports).unwrap())
                        .unwrap();
                    input.perform(serde_json::json!([contact(device, "up", start)]));
                    pump(100);
                    assert!(
                        interior <= 1 && maximum <= 1 && shifted_interior <= 1 && (docked || vacated <= 1),
                        "{name}: held tab differs from rest: interior{interior}, source and feet{maximum}, shifted interior{shifted_interior}, vacated strip{vacated}; see tab-drag-rest.json"
                    );
                }
            }
        }
    }
    input.finish();
    w.window.destroy();
    pump(100);
}

fn assert_tab_preview_content(
    w: &Rc<Workspace>,
    source_content: &gtk::gsk::RenderNode,
    bounds: gtk::graphene::Rect,
    shifted: Bounds,
    scale: f32,
    output: &std::path::Path,
    name: &str,
) -> (usize, usize, usize) {
    let expected = gtk::Snapshot::new();
    expected.translate(&gtk::graphene::Point::new(shifted.x - bounds.x(), shifted.y - bounds.y()));
    expected.append_node(source_content);
    let expected = expected.to_node().unwrap();
    let preview = gtk::Snapshot::new();
    w.workspace_drag.borrow().as_ref().unwrap().tab.as_ref().unwrap().snapshot(
        &preview,
        w.window.frame_clock().unwrap().frame_time(),
        scale,
    );
    let preview = preview.to_node().unwrap();
    let viewport = gtk::graphene::Rect::new(
        (shifted.x * scale).floor(),
        (shifted.y * scale).floor(),
        ((shifted.x + shifted.width) * scale).ceil() - (shifted.x * scale).floor(),
        ((shifted.y + shifted.height) * scale).ceil() - (shifted.y * scale).floor(),
    );
    let transparent = |node: &gtk::gsk::RenderNode, label: &str| {
        let node = gtk::gsk::TransformNode::new(node, Some(&gtk::gsk::Transform::new().scale(scale, scale)));
        let texture = w.window.renderer().unwrap().render_texture(node, Some(&viewport));
        texture.save_to_png(output.join(format!("{name}-{label}.png"))).unwrap();
        let mut pixels = vec![0; texture.width() as usize * texture.height() as usize * 4];
        texture.download(&mut pixels, texture.width() as usize * 4);
        (texture, pixels)
    };
    let (expected, expected_pixels) = transparent(&expected, "source-content");
    let (preview, preview_pixels) = transparent(&preview, "preview-content");
    assert_eq!((expected.width(), expected.height()), (preview.width(), preview.height()));
    let (mut blank_pixels, mut unexpected_content, mut checked) = (0, 0, 0);
    let mut first_content = None;
    for y in 0..expected.height() as usize {
        for x in 0..expected.width() as usize {
            let gx = viewport.x() + x as f32 + 0.5;
            let gy = viewport.y() + y as f32 + 0.5;
            if gx < shifted.x * scale + 2.
                || gx > (shifted.x + shifted.width) * scale - 2.
                || gy < shifted.y * scale + 2.
                || gy > (shifted.y + shifted.height) * scale - 2.
            {
                continue;
            }
            checked += 1;
            let i = (y * expected.width() as usize + x) * 4;
            if expected_pixels[i + 3] <= 1 {
                blank_pixels += 1;
                if preview_pixels[i + 3] > 1 {
                    unexpected_content += 1;
                    if first_content.is_none() {
                        first_content = Some(
                            serde_json::json!({"pixel":[x,y],"expected_alpha":expected_pixels[i+3],"preview_alpha":preview_pixels[i+3]}),
                        );
                    }
                }
            }
        }
    }
    assert!(blank_pixels * 2 > checked, "source-only snapshot leaves most tab pixels transparent");
    assert_eq!(unexpected_content, 0, "{name}: neighbouring tab content inside source; first={first_content:?}");
    (blank_pixels, checked, unexpected_content)
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
