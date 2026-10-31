// Native input coverage for collapsed column stacks.
use super::*;

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_scroll_reverses_after_overshoot"]
fn native_column_scroll_reverses_after_overshoot() {
    let app = native_test_app("art.capycanvas.ColumnScrollReversal");
    let w = fixture_workspace(&app);
    w.window.maximize(); w.window.present(); pump(1600); wait_workspaces(&w);
    let viewport = [w.surface.width() as f32,w.surface.height() as f32];
    let input = RemoteInput::new(); input.ready();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let (mut fixture,mut panels) = native_bridge_fixture(viewport,84.5);
        for index in 0..30 {
            let panel = fixture.layout.add_toolbar(None,&format!("Scroll {index}"),&[ToolbarControl::Command {command:CommandId::Brush}]).unwrap();
            fixture.layout.add_panel_to_group(panel,5).unwrap();
            panels.push(panel);
        }
        w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(fixture)}); pump(300);
        let geometry = ui_session(&w).layout(viewport).collapsed[0].clone();
        assert!(geometry.scroll_max>10.,"column fixture overflows: {geometry:?}");
        let column = geometry.id;
        w.dispatch(UiAction::MeasureColumnScroll {column,offset:10000.}); pump(150);
        let offset = state(&w).workspace.layout.column_scroll.into_iter().find(|(id,_)|*id==column).unwrap().1;
        let panel = *panels.last().unwrap();
        let icon = w.columns.button(column,panel).unwrap();
        let before = icon.compute_bounds(&w.surface).unwrap().y();
        w.dispatch(UiAction::MeasureColumnScroll {column,offset:offset-10.}); pump(150);
        let after = w.columns.button(column,panel).unwrap().compute_bounds(&w.surface).unwrap().y();
        assert!((after-before-10.).abs()<=1.,"{theme:?}: reversing an overshoot moves the native icon10px: {before}->{after},stored={offset}");
        let scroll = find_named(w.surface.upcast_ref(),&format!("column-scroll-{column}")).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        let resolved = ui_session(&w).layout(viewport).collapsed.into_iter().find(|candidate|candidate.id==column).unwrap();
        assert!((scroll.vadjustment().value() as f32-resolved.scroll_offset).abs()<=1.);
        assert!((scroll.vadjustment().upper() as f32-scroll.vadjustment().page_size() as f32-resolved.scroll_max).abs()<=1.);
    }
    input.finish(); w.window.destroy(); pump(100);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_panel_bridge_input --tablet"]
fn native_column_panel_bridge_input() {
    column_panel_bridges(&["mouse", "touch", "pen"], false);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_panel_bridge_motion"]
fn native_column_panel_bridge_motion() {
    column_panel_bridges(&["mouse"], true);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_panel_bridge_motion_diagnostic"]
fn native_column_panel_bridge_motion_diagnostic() {
    column_panel_bridge_cases(&["mouse"], true, &[Theme::Light], &[Edge::Left]);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_panel_bridge_configuration"]
fn native_column_panel_bridge_configuration() {
    let app = native_test_app("art.capycanvas.BridgeConfiguration");
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = fixture_workspace(&app);
    w.window.maximize(); w.window.present(); pump(1600); wait_workspaces(&w);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut input = RemoteInput::new().settle_ms(100).timeout_secs(10);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map_or(input.dir.clone(), Into::into);
    std::fs::create_dir_all(&output).unwrap();
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for edge in [Edge::Left, Edge::Right] {
            let (mut fixture, panels) = native_bridge_fixture(viewport, 84.5);
            fixture.layout.bands[0].edge = edge;
            w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) }); pump(200);
            let panel = panels[2];
            let button = w.columns.button(4, panel).unwrap().compute_bounds(&w.surface).unwrap();
            input.click([button.x() + button.width() * 0.5, button.y() + button.height() * 0.5]);
            assert!(!w.columns.snapshot(&w).is_empty(), "configuration begins on a GPU-connected column");
            let before = layer_ui::durable_layout(&state(&w).workspace.layout);
            w.dispatch(UiAction::Customize { action: CustomizationAction::ShowAllControls { panel } }); pump(300);
            assert!(w.customization.placement().is_some());
            let root = w.groups.borrow().iter().find(|group| group.id == 5).unwrap().root.clone();
            assert!(!root.has_css_class("gpu-panel-surface") && root.has_css_class("expanded-panel"));
            assert!(w.columns.button(4, panel).is_none(), "configuration retires its compact source strip");
            until(|| w.columns.snapshot(&w).is_empty() && ui_session(&w).engine().backend().panel_surfaces.is_empty(),
                "configuration must retire uploaded GPU panel surfaces");
            let preview = root.imp().preview.borrow().clone().unwrap().compute_bounds(&w.surface).unwrap();
            let configuration = root.imp().configuration.borrow().clone().unwrap().compute_bounds(&w.surface).unwrap();
            let whole = root.compute_bounds(&w.surface).unwrap();
            assert!(configuration.y() > whole.y() + 1., "configuration fixture leaves empty space above its body");
            assert!(whole.width() > preview.width() && whole.contains_rect(&configuration),
                "ordinary configuration remains a second column of the native panel");
            let expanded = layer_ui::durable_layout(&state(&w).workspace.layout);
            assert_ne!(expanded, before, "configuration reveals the ordinary column through shared policy");
            let gpu = ui_session(&w).engine().backend().capture().expect("capture retired GPU panel surfaces");
            let empty = [configuration.x() + configuration.width() * 0.5, (whole.y() + configuration.y()) * 0.5];
            let pixel = column_bridge_pixel(&gpu, viewport, empty);
            let clear = column_bridge_pixel(&gpu, viewport, [viewport[0] * 0.5, empty[1]]);
            assert!(pixel[..3].iter().zip(&clear[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                "GPU panel must not fill empty space above configuration: {pixel:?} vs {clear:?}");
            let texture = crate::with_canvas_snapshot(&w, || crate::snapshot_window(&w.window, 1.));
            texture.save_to_png(output.join(format!("connected-configuration-{theme:?}-{edge:?}.png"))).unwrap();
            let mut download = gdk::TextureDownloader::new(&texture);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (bytes, stride) = download.download_bytes();
            let point = w.surface.compute_point(&w.window,
                &gtk::graphene::Point::new(configuration.x() + 6., configuration.y() + configuration.height() * 0.5)).unwrap();
            let offset = point.y().floor() as usize * stride + point.x().floor() as usize * 4;
            let expected = w.palette.get().unwrap().panel.0;
            assert!(bytes[offset..offset + 3].iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "connected configuration keeps its native opaque panel background: {:?} vs {expected:?}", &bytes[offset..offset + 4]);
            w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded }); pump(200);
            assert!(w.customization.placement().is_none());
            assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), expanded);
        }
    }
    input.finish(); w.window.destroy(); pump(100);
}

#[test]
#[ignore = "isolated workspace-motion.sh gtk --native-test=native_column_panel_bridge_transitions; LAYER_NATIVE_CAPTURE_DIR=<dir>"]
fn native_column_panel_bridge_transitions() {
    let app = native_test_app("art.capycanvas.BridgeTransitions");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    wait_workspaces(&w);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut input = RemoteInput::new().settle_ms(60).timeout_secs(20);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map_or(input.dir.clone(), Into::into);
    std::fs::create_dir_all(&output).unwrap();
    let mut samples = Vec::new();
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for edge in [Edge::Left, Edge::Right] {
            let mut partial = [false; 2];
            input.perform(serde_json::json!([{"record_start":format!("bridge-transitions-{theme:?}-{edge:?}").to_lowercase()}]));
            for height in [68.5, 72.5, 76.5, 84.5, 104.5, 108.5, 112.5, 116.5] {
                let (mut fixture, panels) = native_bridge_fixture(viewport, height);
                fixture.layout.bands[0].edge = edge;
                w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
                pump(100);
                for panel in [panels[2], panels[3], panels[2]] {
                    let button = w.columns.button(4, panel).unwrap();
                    let b = button.compute_bounds(&w.surface).unwrap();
                    input.click([b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]);
                    let b = button.compute_bounds(&w.surface).unwrap();
                    let geometry = w.columns.connection_geometry.borrow();
                    let bridge = &geometry.iter().find(|(column, p, _)| *column == 4 && *p == panel).unwrap().2;
                    let top = bridge.anchor.y;
                    let bottom = top + bridge.anchor.height;
                    assert!((top - b.y()).abs() < 0.001 && (bottom - b.y() - b.height()).abs() < 0.001,
                        "the bridge spans the allocated tile exactly: {top}..{bottom} vs {b:?}");
                    let groups = w.groups.borrow();
                    let receiving: Vec<_> = [5, 6].into_iter().map(|id|
                        groups.iter().find(|g| g.id == id).unwrap().root.compute_bounds(&w.surface).unwrap()).collect();
                    let gap = [receiving[0].y() + receiving[0].height(), receiving[1].y()];
                    let gap_partial = [bottom > gap[0] && bottom < gap[1], top > gap[0] && top < gap[1]];
                    for i in 0..2 { partial[i] |= gap_partial[i]; }
                    let r = w.resolved();
                    let icon = r.collapsed.iter().find(|c| c.id == 4).unwrap().groups.iter().flat_map(|g| &g.icons)
                        .find(|i| i.panel == panel).unwrap();
                    samples.push(serde_json::json!({"theme":format!("{theme:?}"),"edge":format!("{edge:?}"),"panel":panel,
                        "height":height,"native_tile":[b.x(),b.y(),b.width(),b.height()],"logical_tile":icon.bounds,
                        "bridge":[top,bottom],"gap_partial":gap_partial}));
                }
            }
            assert!(partial[0] && partial[1], "recorded fixtures must cover upper-only and lower-only partial gap overlaps");
            input.perform(serde_json::json!([{"record_stop":true}]));
            capture_reference(&w, output.join(format!("bridge-transitions-{theme:?}-{edge:?}.png")).to_str().unwrap(), 1.);
        }
    }
    std::fs::write(output.join("bridge-alignment.json"), serde_json::to_vec_pretty(&samples).unwrap()).unwrap();
    input.finish();
    w.window.destroy();
    pump(100);
}

fn native_bridge_fixture(viewport: [f32; 2], height: f32) -> (layer_ui::WorkspaceState, Vec<Panel>) {
    let mut fixture = layer_ui::WorkspaceState::default();
    let panels: Vec<_> = (0..6).map(|i| fixture.layout.add_toolbar(None, &format!("Bridge {i}"),
        &[ToolbarControl::Command { command: CommandId::Brush }]).unwrap()).collect();
    fixture.layout.bands = vec![DockBand { alignment: None, id: 3, edge: Edge::Left, extent: 300.,
        root: DockNode::Split { id: 4, axis: Axis::Vertical,
            fraction: height / (viewport[1] - HEADER_HEIGHT - WORKSPACE_SPACING * 2.),
            first: Box::new(DockNode::Tabs { id: 5, panels: panels[..5].to_vec(), active: panels[0], tab_style: TabStyle::Icon }),
            second: Box::new(DockNode::Tabs { id: 6, panels: vec![panels[5]], active: panels[5], tab_style: TabStyle::Icon }),
        } }];
    fixture.layout.floating.clear();
    fixture.layout.collapsed.clear();
    fixture.layout.column_stacks.clear();
    fixture.layout.column_scroll.clear();
    fixture.layout.set_column_collapsed(5, true, viewport).unwrap();
    (fixture, panels)
}

fn column_panel_bridges(devices: &[&str], measure: bool) {
    column_panel_bridge_cases(devices, measure, &[Theme::Light, Theme::Dark], &[Edge::Left, Edge::Right]);
}

fn column_panel_bridge_cases(devices: &[&str], measure: bool, themes: &[Theme], edges: &[Edge]) {
    let app = native_test_app("art.capycanvas.PanelBridges");
    let w = fixture_workspace(&app);
    w.window.maximize(); w.window.present(); pump(1600); wait_workspaces(&w);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut input = RemoteInput::new().settle_ms(160).timeout_secs(10);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map_or(input.dir.clone(), Into::into);
    std::fs::create_dir_all(&output).unwrap();
    let (fixture, panels) = native_bridge_fixture(viewport, 84.5);
    let mut motion_failures = Vec::new();
    input.ready();
    for &theme in themes {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for &edge in edges { for &device in devices {
            let mut fixture = fixture.clone(); fixture.layout.bands[0].edge = edge;
            w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) }); pump(300);
            for (index, panel) in panels[..3].iter().copied().enumerate() {
                let button = w.columns.button(4, panel).unwrap();
                let b = button.compute_bounds(&w.surface).unwrap();
                let point = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
                assert!(w.drag_target_at(point) == Some(DragTarget::Dock(DockItem::Panel { panel })));
                if device == "mouse" { input.click(point); }
                else { input.perform(serde_json::json!([contact(device, "down", point), contact(device, "up", point)])); }
                pump(200);
                let r = w.resolved();
                let shared = ui_session(&w).layout(viewport);
                assert_eq!(shared.groups.iter().find(|g| g.id == 5).unwrap().active, panel);
                let c = r.collapsed.iter().find(|c| c.id == 4).unwrap();
                assert!(c.open.is_some());
                let target = r.groups.iter().find(|g| g.id == 5).unwrap();
                assert_eq!(target.corner_radii, [SURFACE_RADIUS; 4], "receiver keeps its original outline");
                let bridge = w.columns.connection_geometry.borrow().iter()
                    .find(|(column, p, _)| *column == 4 && *p == panel).unwrap().2.clone();
                let direction = if edge == Edge::Left { 1. } else { -1. };
                let source = if edge == Edge::Left { b.x() + b.width() } else { b.x() };
                let width = (bridge.plane - source).abs();
                let image = column_bridge_capture(&w);
                gdk::MemoryTexture::new(image.width as i32, image.height as i32, gdk::MemoryFormat::R8g8b8a8Premultiplied,
                    &glib::Bytes::from_owned(image.bytes.clone()), image.stride as usize)
                    .save_to_png(output.join(format!("gpu-bridge-{theme:?}-{edge:?}-{device}-{index}.png"))).unwrap();
                let reference = column_bridge_pixel(&image, viewport,
                    [bridge.plane + direction * 24., target.bounds.y + target.bounds.height - 18.]);
                for group in w.columns.snapshot(&w).iter().flat_map(|surface| &surface.groups) {
                    let pixel = column_bridge_pixel(&image, viewport,
                        [bridge.plane + direction * 24., group.bounds[1] + group.bounds[3] * 0.5]);
                    assert!(pixel[..3].iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                        "every receiving GPU panel keeps its themed body color: {pixel:?} vs {reference:?}; bounds={:?}", group.bounds);
                }
                if index == 2 {
                    let next = r.groups.iter().find(|g| g.id == 6).unwrap();
                    let gap = (target.bounds.y + target.bounds.height + next.bounds.y) * 0.5;
                    let connected = column_bridge_pixel(&image, viewport, [source + direction * width * 0.25, gap]);
                    assert!(connected[..3].iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                        "the GPU bridge remains connected beside the gap notch: {connected:?} vs {reference:?}");
                    let notch = column_bridge_pixel(&image, viewport, [bridge.plane - direction * width * 0.1, gap]);
                    let clear = column_bridge_pixel(&image, viewport,
                        [bridge.plane + direction * (target.bounds.width + 24.), gap]);
                    let distance = |a: [u8; 4], b: [u8; 4]| a[..3].iter().zip(&b[..3])
                        .map(|(a, b)| u32::from(a.abs_diff(*b))).sum::<u32>();
                    assert!(distance(notch, clear) < distance(notch, reference),
                        "the GPU gap curve cuts into the bridge: notch={notch:?}, clear={clear:?}, panel={reference:?}");
                    column_bridge_tile_pixels(&w, &button, [target.bounds.y + target.bounds.height, next.bounds.y]);
                }
                let groups = w.groups.borrow();
                let root = &groups.iter().find(|g| g.id == 5).unwrap().root;
                let bar = find_named(root.upcast_ref(), "panel-tab-bar").unwrap();
                if w.palette.get().unwrap().glass.transparency.enabled() {
                    let bounds = bar.compute_bounds(&w.surface).unwrap();
                    let p = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height() * 0.5];
                    let surfaces = w.columns.snapshot(&w);
                    assert!(surfaces.iter().flat_map(|surface| &surface.groups).any(|group|
                        p[0] >= group.tab_bar[0] && p[1] >= group.tab_bar[1]
                            && p[0] < group.tab_bar[0] + group.tab_bar[2] && p[1] < group.tab_bar[1] + group.tab_bar[3]),
                        "GPU panel surfaces include the complete tab bar");
                }
                let offset = 0.5 * viewport[1] / image.height as f32;
                for y in [b.y() + offset, b.y() + b.height() - offset] {
                    let pixel = column_bridge_pixel(&image, viewport, [source + direction * offset, y]);
                    assert!(pixel[..3].iter().zip(&reference[..3]).all(|(a, b)| a.abs_diff(*b) <= 2),
                        "complete first/last GPU tile rows: {pixel:?} vs {reference:?}");
                }
                drop(groups);
                capture_reference(&w, output.join(format!("bridge-{theme:?}-{edge:?}-{device}-{index}.png")).to_str().unwrap(), 1.);
                if index == 2 {
                    let before = layer_ui::durable_layout(&state(&w).workspace.layout);
                    let divider = r.dividers.iter().find(|d| d.id == 4 && !d.band).unwrap();
                    let start = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + divider.bounds.height * 0.5];
                    input.perform(serde_json::json!([contact(device, "down", start)]));
                    for delta in [12., 24., 36., 48.] {
                        input.perform(serde_json::json!([contact(device, "move", [start[0], start[1] + delta])]));
                        let current = w.resolved();
                        let target = current.groups.iter().find(|g| g.id == 5).unwrap();
                        assert_eq!(target.corner_radii, [SURFACE_RADIUS; 4]);
                    }
                    input.perform(serde_json::json!([contact(device, "up", [start[0], start[1] + 48.])]));
                    let after = layer_ui::durable_layout(&state(&w).workspace.layout);
                    assert_ne!(after, before);
                    w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
                    assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), before);
                    w.dispatch(UiAction::Invoke { command: CommandId::RedoWorkspace });
                    assert_eq!(layer_ui::durable_layout(&state(&w).workspace.layout), after);
                    if measure && device == "mouse" {
                        w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }); pump(200);
                        motion_failures.extend(column_bridge_motion(&w, &mut input, &output, theme, edge));
                    }
                }
            }
        }}
    }
    input.finish(); w.window.destroy(); pump(100);
    assert!(motion_failures.is_empty(), "bridge presentation targets failed: {motion_failures:?}");
}

fn column_bridge_capture(w: &Workspace) -> layer_render::ReadbackImage {
    until(|| {
        let expected = w.columns.snapshot(w);
        let scale = w.area.scale_factor() as f32;
        let uploaded: Vec<_> = expected.iter().cloned().map(|surface| surface.scaled(scale)).collect();
        !expected.is_empty() && ui_session(w).engine().backend().panel_surfaces == uploaded
    }, "GPU panel surfaces must reach the frame before capture");
    ui_session(w).engine().backend().capture().expect("capture GPU panel backgrounds")
}

fn column_bridge_pixel(image: &layer_render::ReadbackImage, extent: [f32; 2], point: [f32; 2]) -> [u8; 4] {
    let x = (point[0] * image.width as f32 / extent[0]).floor() as usize;
    let y = (point[1] * image.height as f32 / extent[1]).floor() as usize;
    assert!(x < image.width as usize && y < image.height as usize, "GPU sample {point:?} lies inside {extent:?}");
    image.bytes[y * image.stride as usize + x * 4..y * image.stride as usize + x * 4 + 4].try_into().unwrap()
}

fn column_bridge_tile_pixels(w: &Workspace, button: &gtk::Button, boundaries: [f32; 2]) {
    let scale = w.surface.native().and_then(|n| n.surface()).unwrap().scale() as f32;
    let texture = crate::with_canvas_snapshot(w, || crate::snapshot_window(&w.window, scale));
    let mut download = gdk::TextureDownloader::new(&texture);
    download.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = download.download_bytes();
    let bounds = button.compute_bounds(&w.window).unwrap();
    let origin = w.surface.compute_bounds(&w.window).unwrap();
    let x = ((if bounds.x() < w.window.width() as f32 * 0.5 { bounds.x() + bounds.width() - 3. }
        else { bounds.x() + 2. }) * scale).floor() as usize;
    let sample = |y: usize| <[u8; 4]>::try_from(&bytes[y * stride + x * 4..y * stride + x * 4 + 4]).unwrap();
    let reference = sample(((bounds.y() + bounds.height() * 0.5) * scale).floor() as usize);
    for boundary in boundaries {
        let y = (boundary + origin.y()) * scale;
        for y in y.floor() as usize - 1..=y.ceil() as usize + 1 {
            let pixel = sample(y);
            assert!(pixel.iter().zip(reference).all(|(a, b)| a.abs_diff(b) <= 1),
                "a GPU bridge boundary must not cut a line through its selected GTK tile at {x},{y}: {pixel:?} vs {reference:?}");
        }
    }
}

fn column_bridge_motion(w: &Rc<Workspace>, input: &mut RemoteInput, output: &std::path::Path, theme: Theme, edge: Edge) -> Vec<String> {
    let interval = std::env::var("LAYER_NATIVE_EVENT_MS").map_or(4, |value| value.parse::<u32>().unwrap());
    assert_eq!(interval, 4, "bridge motion uses 1500 moves at 4 ms cadence");
    let refresh = std::env::var("LAYER_MOTION_REFRESH").map_or(120., |value| value.parse::<f64>().unwrap());
    let minimum = std::env::var("LAYER_MOTION_MIN_HZ").map_or(refresh * 0.95, |value| value.parse::<f64>().unwrap());
    let settle = std::mem::replace(&mut input.settle_ms, 0);
    let resolved = w.resolved();
    let panel = resolved.collapsed.iter().find(|column| column.id == 4).unwrap().groups[0].icons.last().unwrap().panel;
    let button = w.columns.button(4, panel).unwrap().compute_bounds(&w.surface).unwrap();
    input.click([button.x() + button.width() * 0.5, button.y() + button.height() * 0.5]); pump(200);
    let before = layer_ui::durable_layout(&state(w).workspace.layout);
    let worker_stats = ui_session(w).engine().backend().stats.clone();
    let mut failures = Vec::new();
    for run in 0usize..4 {
        pump(gtk::Settings::for_display(&w.surface.display()).gtk_double_click_time().max(0) as u64 + 20);
        let r = w.resolved();
        let divider = r.dividers.iter().find(|d| d.id == 4 && !d.band).unwrap();
        let start = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + divider.bounds.height * 0.5];
        assert!(w.drag_target_at(start) == Some(DragTarget::Divider(4)), "motion begins on the receiving-panel divider");
        let root = w.groups.borrow().iter().find(|group| group.id == 5).unwrap().root.clone();
        let original = root.compute_bounds(&w.surface).unwrap().height();
        let original_revision = w.chrome.revision();
        input.perform(serde_json::json!([contact("mouse", "move", start)]));
        input.perform(serde_json::json!([contact("mouse", "down", start)]));
        input.perform(serde_json::json!([contact("mouse", "move", [start[0], start[1] + 40.])]));
        pump(50);
        assert!(w.workspace_drag.borrow().as_ref().is_some_and(|drag| drag.started && drag.target == DragTarget::Divider(4)),
            "motion keeps the native divider drag active");
        let picked_up = root.compute_bounds(&w.surface).unwrap().height();
        assert!((picked_up - original).abs() > 1.,
            "native divider pickup must resize before measuring: {original} -> {picked_up}; shared={:?}",
            w.resolved().groups.iter().find(|group| group.id == 5).unwrap().bounds);
        let pickup_revision = w.chrome.revision();
        let center = [start[0], start[1] + (picked_up - original).max(40.) + 40.];
        input.perform(serde_json::json!([contact("mouse", "move", center)])); pump(50);
        let centered = root.compute_bounds(&w.surface).unwrap().height();
        w.publication.inputs.borrow_mut().clear();
        w.publication.frames.borrow_mut().clear();
        *worker_stats.lock().unwrap() = Default::default();
        let clock = w.surface.frame_clock().unwrap();
        let samples = Rc::new(RefCell::new(Vec::new()));
        let previous = Rc::new(Cell::new(centered));
        let revisions = Rc::new(RefCell::new(std::collections::BTreeMap::from([
            (original_revision, original), (pickup_revision, picked_up), (w.chrome.revision(), centered),
        ])));
        let missing = Rc::new(RefCell::new(Vec::new()));
        let callbacks = Rc::new(Cell::new(0));
        let painted = clock.connect_after_paint(glib::clone!(#[strong] root, #[strong] samples, #[strong] previous,
            #[strong] revisions, #[strong] missing, #[strong] callbacks,
            #[weak] w, move |clock| {
                callbacks.set(callbacks.get() + 1);
                let bounds = root.compute_bounds(&w.surface).unwrap();
                revisions.borrow_mut().insert(w.chrome.revision(), bounds.height());
                let shown = w.chrome_released.get();
                let Some(height) = revisions.borrow().get(&shown).copied() else {
                    missing.borrow_mut().push(shown);
                    return;
                };
                let changed = previous.replace(height) != height;
                samples.borrow_mut().push((clock.frame_time(), height, changed, clock.current_timings()));
            }));
        let events: Vec<_> = (0..1500).map(|i| {
            let phase = ((i + 62) % 250) as f32 / 250.;
            contact("mouse", "move", [center[0], center[1] + 36. * (1. - 4. * (phase - 0.5).abs())])
        }).collect();
        let began = glib::monotonic_time();
        input.perform(serde_json::to_value(events).unwrap());
        let ended = glib::monotonic_time();
        clock.disconnect(painted);
        let inputs = w.publication.inputs.borrow().len();
        input.perform(serde_json::json!([contact("mouse", "up", center)]));
        pump(200);
        let mut presented: Vec<_> = samples.borrow().iter().filter(|(_, _, changed, _)| *changed)
            .filter_map(|(_, _, _, timing)| timing.as_ref()).filter(|t| t.is_complete() && t.presentation_time() > 0)
            .map(|t| t.presentation_time()).collect();
        presented.sort_unstable(); presented.dedup();
        let mut all_presented: Vec<_> = samples.borrow().iter().filter_map(|(_, _, _, timing)| timing.as_ref())
            .filter(|t| t.is_complete() && t.presentation_time() > 0).map(|t| t.presentation_time()).collect();
        all_presented.sort_unstable(); all_presented.dedup();
        let mut gaps: Vec<_> = presented.windows(2).map(|p| (p[1] - p[0]) as f64 / 1000.).collect();
        gaps.sort_by(f64::total_cmp);
        let hz = (presented.len() > 1).then(|| (presented.len() - 1) as f64 * 1e6 / (presented.last().unwrap() - presented[0]) as f64);
        let p99 = gaps.get(gaps.len() * 99 / 100).copied();
        let heights: Vec<_> = samples.borrow().iter().map(|(_, height, _, _)| *height).collect();
        let range = [heights.iter().copied().fold(f32::INFINITY, f32::min), heights.iter().copied().fold(f32::NEG_INFINITY, f32::max)];
        let onset = presented.first().map(|time| (*time - began) as f64 / 1000.);
        let tail = presented.last().map(|time| (ended - *time).max(0) as f64 / 1000.);
        let longest = gaps.last().copied().into_iter().chain(onset).chain(tail).fold(0., f64::max);
        let mut report = serde_json::json!({"run":run.checked_sub(1),"priming":run==0,"input":"mouse",
            "theme":format!("{theme:?}"),"edge":format!("{edge:?}"),"scale":w.surface.native().and_then(|n|n.surface()).unwrap().scale(),
            "scope":"GTK workstation diagnostic; custom toolbars and default drawing; no reference-tier qualification",
            "input_moves":1500,"path":"triangle","amplitude_px":36,"center":center,
            "pickup_height":picked_up,"native_input_samples":inputs,"input_window_us":[began,ended],
            "moving_presentations":presented,"presentation_hz":hz,"moving_gap_p99_ms":p99,
            "paint_callbacks":callbacks.get(),"height_range":range,"full_contact_presentations":all_presented.len(),
            "missing_shown_revisions":*missing.borrow(),"requested_revision_heights":*revisions.borrow(),
            "full_contact_duration_s":(ended-began) as f64/1e6,"onset_ms":onset,"tail_ms":tail,"longest_gap_ms":longest,
            "minimum_hz":minimum,"maximum_p99_ms":2000./refresh});
        {
            let stats = worker_stats.lock().unwrap();
            report["timing"] = serde_json::json!({
                "ui_input_ms":*w.publication.inputs.borrow(),"ui_placement_frames":*w.publication.frames.borrow(),
                "input_cpu":stats.input_cpu,"input_handler_cpu":stats.input_handler_cpu,
                "frame_handler_cpu":stats.frame_handler_cpu,"wake_lateness":stats.wake_lateness,
                "worker_cpu":stats.cpu,"worker_cpu_stages":stats.cpu_stages,"worker_thread_cpu":stats.thread_cpu,
                "worker_gpu":stats.gpu,"canvas_presentation":stats.presented,"backdrop_frames":stats.backdrop_frames,
                "raster_commits":stats.raster_commits,"camera_views":stats.camera_views,
                "shown_geometry_frames":samples.borrow().iter().map(|(time,height,changed,_)|
                    (*time,*height,*changed)).collect::<Vec<_>>(),
                "final_requested_chrome":w.chrome.revision(),"final_shown_chrome":w.chrome_released.get(),
            });
        }
        let name = if run==0 {"prime".into()} else {(run-1).to_string()};
        std::fs::write(output.join(format!("bridge-motion-{theme:?}-{edge:?}-{name}.json")), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        eprintln!("Bridge motion {theme:?}/{edge:?}/{name}: {report}");
        w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace }); pump(200);
        assert_eq!(layer_ui::durable_layout(&state(w).workspace.layout), before, "one undo restores the resize fixture");
        assert!(missing.borrow().is_empty(), "displayed chrome revisions must have measured geometry: {:?}", missing.borrow());
        assert!(inputs > 200 && range[1] - range[0] > 12. && presented.len() > 1,
            "native input must move actual panel geometry: inputs={inputs}, height_range={range:?}, presents={}", presented.len());
        if run > 0 {
            if hz.unwrap() < minimum || p99.unwrap() > 2000./refresh {
                failures.push(format!("{theme:?}/{edge:?}/{name}: {} Hz, p99 {} ms", hz.unwrap(), p99.unwrap()));
            }
        }
    }
    input.settle_ms = settle;
    failures
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_stack_member_drop_input"]
fn native_stack_member_drop_input() {
    native_stack_drop_input(false);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_column_group_append_input"]
fn native_column_group_append_input() {
    native_stack_drop_input(true);
}

fn native_stack_drop_input(append: bool) {
    let mut input = RemoteInput::new().timeout_secs(10);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map_or(input.dir.clone(), Into::into);
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.StackMemberDrops");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    input.ready();
    pump(500);
    for (theme, edge) in [(Theme::Dark, Edge::Left), (Theme::Light, Edge::Right)] {
        for touch in [false, true] {
            let device = if touch { "touch" } else { "mouse" };
            let event = |phase: &str, point: [f32; 2]| contact(device, phase, point);
            for stacked in [false, true] {
                for source in ["panel", "group", "toolbar", "icon", "drawer-tab"] {
                    eprintln!(
                        "Stack drop: {theme:?}, touch={touch}, stacked={stacked}, append={append}, {source}"
                    );
                    let mut fixture = layer_ui::WorkspaceState::default();
                    fixture.layout.bands[0].edge = edge;
                    fixture.layout.bands[1].edge = if edge == Edge::Left {
                        Edge::Right
                    } else {
                        Edge::Left
                    };
                    fixture
                        .layout
                        .set_column_collapsed(5, true, viewport)
                        .unwrap();
                    let target = if stacked {
                        fixture
                            .layout
                            .set_panel_visible(Panel::Navigator, true)
                            .unwrap();
                        let group = fixture.layout.panel_group(Panel::Navigator).unwrap();
                        fixture
                            .layout
                            .set_column_collapsed(group, true, viewport)
                            .unwrap();
                        fixture
                            .layout
                            .move_item(
                                viewport,
                                DockItem::Column { column: group },
                                DockTarget::StackColumn {
                                    column: 4,
                                    before: false,
                                },
                            )
                            .unwrap();
                        group
                    } else {
                        4
                    };
                    if append {
                        // Both the first multi-group member and a middle
                        // single-group member must have a trailing group target.
                        fixture
                            .layout
                            .set_panel_visible(Panel::Color, true)
                            .unwrap();
                        let group = fixture.layout.panel_group(Panel::Color).unwrap();
                        fixture
                            .layout
                            .set_column_collapsed(group, true, viewport)
                            .unwrap();
                        fixture
                            .layout
                            .move_item(
                                viewport,
                                DockItem::Column { column: group },
                                DockTarget::StackColumn {
                                    column: target,
                                    before: false,
                                },
                            )
                            .unwrap();
                    }
                    if source == "group" {
                        fixture.layout.select_tab(8, Panel::Properties).unwrap();
                        fixture
                            .layout
                            .move_item(
                                viewport,
                                DockItem::Group { group: 8 },
                                DockTarget::Float {
                                    position: [700., 300.],
                                },
                            )
                            .unwrap();
                    }
                    if source == "icon" || source == "drawer-tab" {
                        fixture
                            .layout
                            .set_column_collapsed(8, true, viewport)
                            .unwrap();
                    }
                    w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture) });
                    w.dispatch(UiAction::SetTheme { theme: Some(theme) });
                    w.dispatch(UiAction::Customize {
                        action: CustomizationAction::SetColumnDrawers {
                            column: 4,
                            drawers: false,
                        },
                    });
                    if source == "drawer-tab" {
                        enable_individual_column_panels(&w, 8);
                        w.dispatch(UiAction::Customize {
                            action: CustomizationAction::ToggleColumnDrawer {
                                group: 8,
                                panel: Panel::Layers,
                            },
                        });
                    }
                    pump(300);
                    let before = layer_ui::durable_layout(&state(&w).workspace.layout);
                    let old_members = state(&w).workspace.layout.column_stack(4).members;
                    let selected = if source == "group" {
                        Panel::Properties
                    } else if source == "toolbar" {
                        Panel::Toolbar
                    } else {
                        Panel::Layers
                    };
                    let start = if source == "icon" {
                        let b = w
                            .columns
                            .button(8, Panel::Layers)
                            .unwrap()
                            .compute_bounds(&w.surface)
                            .unwrap();
                        [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
                    } else if source == "panel" || source == "drawer-tab" {
                        let hit = w
                            .tab_hits()
                            .into_iter()
                            .find(|h| h.group == 8 && h.index == 0)
                            .unwrap();
                        center(hit.bounds)
                    } else if source == "toolbar" {
                        let grip = find_css(w.toolbar.upcast_ref(), "panel-grip").unwrap();
                        let b = grip.compute_bounds(&w.surface).unwrap();
                        [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
                    } else {
                        let b = w
                            .resolved()
                            .groups
                            .iter()
                            .find(|g| g.id == 8)
                            .unwrap()
                            .bounds;
                        [b.x + b.width - 10., b.y + layer_ui::TAB_BAR_HEIGHT * 0.5]
                    };
                    let target_column = w
                        .resolved()
                        .collapsed
                        .into_iter()
                        .find(|c| c.id == target)
                        .unwrap();
                    let last = target_column.groups.last().unwrap();
                    let destination = if append {
                        let tile = last.icons.last().unwrap();
                        let b = w
                            .columns
                            .button(target, tile.panel)
                            .unwrap()
                            .compute_bounds(&w.surface)
                            .unwrap();
                        assert!(
                            (b.y() + b.height() - last.bounds.y - last.bounds.height).abs() <= 1.
                        );
                        [
                            target_column.bounds.x + TILE_SIZE * 0.5,
                            last.bounds.y + last.bounds.height + 3.,
                        ]
                    } else {
                        center(target_column.grip)
                    };
                    let expected_target = if append {
                        DockTarget::Split {
                            group: last.group,
                            edge: Edge::Bottom,
                        }
                    } else {
                        DockTarget::StackColumn {
                            column: target,
                            before: false,
                        }
                    };
                    let capture_name = if append {
                        "column-group"
                    } else {
                        "stack-member"
                    };
                    if source == "icon" {
                        input.perform(serde_json::json!([
                            event("down", start),
                            event("move", destination),
                            event("up", destination)
                        ]));
                        let layout = state(&w).workspace.layout;
                        assert_eq!(layout.panel_group(Panel::Layers), before.panel_group(Panel::Layers),
                            "collapsed tiles cannot reorder before holding");
                        if layer_ui::durable_layout(&layout) != before {
                            w.dispatch(UiAction::Invoke { command: CommandId::UndoWorkspace });
                            pump(150);
                        }
                        assert_eq!(
                            layer_ui::durable_layout(&state(&w).workspace.layout),
                            before,
                            "an unheld swipe from a collapsed tile only grabs its column edge"
                        );
                    }
                    input.perform(serde_json::json!([event("down", start)]));
                    if source == "icon" {
                        pump(800);
                    }
                    input.perform(serde_json::json!([
                        event("move", [viewport[0] * 0.5, viewport[1] * 0.5]),
                        event("move", destination)
                    ]));
                    let hint = w
                        .drop_hint
                        .borrow()
                        .clone()
                        .expect("visible stack insertion preview");
                    assert_eq!(hint.target, expected_target);
                    assert!(hint.bounds.height <= 3. && hint.bounds.width == TILE_SIZE);
                    if append {
                        assert!(hint.bounds.y + hint.bounds.height <= target_column.grip.y);
                    }
                    if source == "panel" && !stacked {
                        // Cancellation must return the torn-off tab and its original selection.
                        input.perform(
                            serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]),
                        );
                        input.perform(serde_json::json!([event("up", destination)]));
                        assert_eq!(
                            layer_ui::durable_layout(&state(&w).workspace.layout),
                            before
                        );
                        input.perform(serde_json::json!([
                            event("down", start),
                            event("move", destination)
                        ]));
                    }
                    if source == "group" && stacked && !touch {
                        capture_reference(
                            &w,
                            output
                                .join(format!("{capture_name}-preview-{theme:?}.png"))
                                .to_str()
                                .unwrap(),
                            1.,
                        );
                    }
                    input.perform(serde_json::json!([event("up", destination)]));
                    assert!(w.workspace_drag.borrow().is_none() && w.drop_hint.borrow().is_none());
                    let layout = state(&w).workspace.layout;
                    layout.validate().unwrap();
                    let group = layout.panel_group(selected).unwrap();
                    let member = layout.collapsed_column_for_group(group).unwrap();
                    let stack = layout.column_stack(4);
                    if append {
                        let mut expected_members = old_members.clone();
                        *expected_members.iter_mut().find(|m| **m == target).unwrap() = member;
                        assert_eq!(stack.members, expected_members);
                        let resolved = w.resolved();
                        let column = resolved.collapsed.iter().find(|c| c.id == member).unwrap();
                        assert_eq!(column.groups.len(), target_column.groups.len() + 1);
                        assert_eq!(column.groups.last().unwrap().group, group);
                        let strip = find_named(
                            w.surface.upcast_ref(),
                            &format!("collapsed-column-{member}"),
                        )
                        .unwrap();
                        assert!(
                            find_css(&strip, "column-divider").is_some(),
                            "appending renders a group divider"
                        );
                    } else {
                        assert_eq!(stack.members, [old_members.clone(), vec![member]].concat());
                    }
                    let icon = w.columns.button(member, selected).unwrap();
                    let b = icon.compute_bounds(&w.surface).unwrap();
                    let point = [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5];
                    input.perform(serde_json::json!([
                        event("down", point),
                        event("up", point)
                    ]));
                    let r = w.resolved();
                    let open = r
                        .collapsed
                        .iter()
                        .find(|c| c.id == member)
                        .unwrap()
                        .open
                        .as_ref()
                        .unwrap();
                    assert!(open.bounds.width >= 128.);
                    assert_eq!(
                        r.groups.iter().find(|g| g.id == group).unwrap().active,
                        selected
                    );
                    if source == "group" && stacked && !touch {
                        crate::capture(
                            &w,
                            output
                                .join(format!("{capture_name}-open-{theme:?}.png"))
                                .to_str()
                                .unwrap(),
                        );
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "isolated Mutter mouse/touch driver: --column-stacks"]
fn native_column_stack_input() {
    let mut input = RemoteInput::new().settle_ms(160).timeout_secs(10);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map_or(input.dir.clone(), Into::into);
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.ColumnStacks");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    wait_workspaces(&w);
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    input.ready();
    pump(500);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState {
                layout: layer_ui::WorkspacePreset::Illustrator.layout(Platform::Gtk),
                ..Default::default()
            }),
        });
        pump(300);
        let resolved = w.resolved();
        for column in &resolved.collapsed {
            for group in &column.groups {
                let view = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group.group)
                    .unwrap()
                    .root
                    .clone();
                assert!(view.is_mapped() && view.width() > 100 && view.height() > 30);
            }
        }
        let canvas = center(resolved.work_area);
        input.perform(serde_json::json!([{"point":canvas,"down":true},{"down":false}]));
        assert!(w.resolved().collapsed.iter().all(|c| c.open.is_some()));
        crate::capture(
            &w,
            output
                .join(format!("paint-default-{theme:?}.png"))
                .to_str()
                .unwrap(),
        );
        let retained = |w: &Workspace| {
            let column = w.resolved().collapsed[0].id;
            let roots: Vec<_> = w.groups.borrow().iter()
                .filter(|g| [Panel::Brushes, Panel::ToolSettings, Panel::Color].iter().any(|p| g.panels.contains(p)))
                .map(|g| (g.root.clone(), g.tabs.iter().map(|(_, tab)| tab.clone()).collect::<Vec<_>>()))
                .collect();
            (roots, w.columns.button(column, Panel::Layers).unwrap())
        };
        let before = retained(&w);
        for _ in 0..2 {
            let icon = before.1.clone().upcast::<gtk::Widget>();
            input.perform(serde_json::json!([contact("mouse", "down", screen_point(&icon, &w.window, [0.5, 0.5])), contact("mouse", "up", [0.; 2])]));
            assert_eq!(retained(&w), before, "toggling a column keeps unchanged groups, tabs and strip tiles");
        }
        assert!(w.resolved().collapsed.iter().all(|c| c.open.is_some()));
    }
    for (theme, edge) in [(Theme::Dark, Edge::Left), (Theme::Light, Edge::Right)] {
        for touch in [false, true] {
            let device = if touch { "touch" } else { "mouse" };
            let event = |phase: &str, point: [f32; 2]| contact(device, phase, point);
            let click = |p| serde_json::json!([event("down", p), event("up", p)]);
            let mut initial = layer_ui::WorkspaceState::default();
            initial.layout.bands[0].edge = edge;
            initial.layout.bands[1].edge = if edge == Edge::Left {
                Edge::Right
            } else {
                Edge::Left
            };
            initial
                .layout
                .set_column_collapsed(5, true, viewport)
                .unwrap();
            initial
                .layout
                .set_column_collapsed(8, true, viewport)
                .unwrap();
            w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(initial) });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(300);
            assert!(find_named(w.surface.upcast_ref(), "expand-column-4").is_none());
            let r = w.resolved();
            let source = center(r.collapsed.iter().find(|c| c.id == 8).unwrap().grip);
            let target = center(r.collapsed.iter().find(|c| c.id == 4).unwrap().grip);
            // Handles pick up immediately for mouse and touch, without a hold.
            input.perform(serde_json::json!([
                event("down", source),
                event("move", target),
                event("up", target)
            ]));
            let stack = state(&w).workspace.layout.column_stack(4);
            assert_eq!(stack.members, [4, 8], "native column-handle drop");
            let stacked = layer_ui::durable_layout(&state(&w).workspace.layout);
            let resolved = w.resolved();
            let layout = state(&w).workspace.layout;
            let fixed = resolved
                .dividers
                .iter()
                .find(|d| layout.fixed_stack_divider(d))
                .unwrap();
            let handle = w
                .surface
                .imp()
                .children
                .borrow()
                .iter()
                .find(|(slot, _)| *slot == Slot::Divider(fixed.id))
                .unwrap()
                .1
                .clone();
            assert!(!handle.can_target() && !handle.is_focusable());
            let start = center(fixed.bounds);
            let moved = [viewport[0] * 0.5, start[1]];
            input.perform(serde_json::json!([
                event("down", start),
                event("move", moved),
                event("up", moved)
            ]));
            assert_eq!(
                layer_ui::durable_layout(&state(&w).workspace.layout),
                stacked
            );
            // Exercise switching from explicitly enabled individual drawers.
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnDrawers { column: 4, drawers: true },
            });
            let empty = center(
                w.resolved()
                    .collapsed
                    .into_iter()
                    .find(|c| c.id == 8)
                    .unwrap()
                    .empty,
            );
            if touch {
                input.perform(serde_json::json!([event("down", empty)]));
                pump(800);
                input.perform(serde_json::json!([event("up", empty)]));
            } else {
                input.perform(
                    serde_json::json!([{"point":empty,"button":273,"down":true},{"button":273,"down":false}]),
                );
            }
            let menu = find_css(w.surface.upcast_ref(), "panel-context-menu")
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            let action =
                menu_action(&menu.menu_model().unwrap(), "Open individual panels").unwrap();
            assert!(menu_action(&menu.menu_model().unwrap(), "Group panel").is_none());
            menu.activate_action(&action, None).unwrap();
            menu.popdown();
            pump(160);
            assert!(!state(&w).workspace.layout.column_stack(4).drawers);
            let icon = |panel| {
                w.resolved()
                    .collapsed
                    .into_iter()
                    .flat_map(|c| c.groups)
                    .flat_map(|g| g.icons)
                    .find(|i| i.panel == panel)
                    .unwrap()
                    .bounds
            };
            input.perform(click(center(icon(Panel::Brushes))));
            let r = w.resolved();
            let member = r.collapsed.iter().find(|c| c.id == 4).unwrap();
            let open = member.open.as_ref().unwrap();
            let second = r.collapsed.iter().find(|c| c.id == 8).unwrap();
            assert_eq!(open.bounds.y, member.bounds.y);
            assert_eq!(
                open.bounds.y + open.bounds.height,
                second.bounds.y + second.bounds.height
            );
            assert_eq!(
                second.bounds.y - member.bounds.y - member.bounds.height,
                WORKSPACE_SPACING
            );
            for column in [member, second] {
                let root = find_named(
                    w.surface.upcast_ref(),
                    &format!("collapsed-column-{}", column.id),
                )
                .unwrap();
                let bounds = root.compute_bounds(&w.surface).unwrap();
                assert!(w.column_corner_styles.borrow().iter().any(|(class, radii)|
                    class == &format!("column-strip-{}", column.id) && *radii == column.corner_radii));
                assert!((bounds.y() - column.bounds.y).abs() <= 1.);
                assert!((bounds.height() - column.bounds.height).abs() <= 1.);

                assert_eq!(widgets(&root).filter(|widget| widget.is::<gtk::Separator>()).count(), column.groups.len().saturating_sub(1));
                for icon in column.groups.iter().flat_map(|g| &g.icons) {
                    let tile = w
                        .columns
                        .button(column.id, icon.panel)
                        .unwrap()
                        .compute_bounds(&w.surface)
                        .unwrap();
                    assert!(
                        (tile.y() - icon.bounds.y).abs() <= 1.,
                        "tile geometry follows the shared column layout"
                    );
                }
            }
            for group in &member.groups {
                let shared = r.groups.iter().find(|g| g.id == group.group).unwrap();
                let views = w.groups.borrow();
                let native = views.iter().find(|v| v.id == group.group).unwrap();
                let b = native.root.compute_bounds(&w.surface).unwrap();
                assert!(w.column_corner_styles.borrow().iter().any(|(class, radii)|
                    class == &format!("column-group-{}", shared.id) && *radii == shared.corner_radii));
                assert!(
                    (b.x() - shared.bounds.x).abs() <= 1. && (b.y() - shared.bounds.y).abs() <= 1.
                );
                assert!(
                    (b.width() - shared.bounds.width).abs() <= 1.
                        && (b.height() - shared.bounds.height).abs() <= 1.
                );
                let button = w.columns.button(4, group.active).unwrap();
                assert!(
                    button.has_css_class("selected-tool"),
                    "all active tabs highlight their sidebar tiles"
                );
            }
            assert!(open.connections.len() >= 2);
            capture_reference(
                &w,
                output
                    .join(format!("column-stack-{theme:?}-{touch}.png"))
                    .to_str()
                    .unwrap(),
                1.,
            );
            // Width and internal split gestures retain ordinary GTK group widgets.
            let ids: Vec<_> = r
                .dividers
                .iter()
                .filter(|d| r.open_column_at_divider(d.id) == Some(4) || d.id == 4)
                .map(|d| d.id)
                .collect();
            assert_eq!(ids.len(), 2);
            for id in ids {
                eprintln!("Column resize {theme:?}, touch={touch}, divider={id}");
                let r = w.resolved();
                let divider = r.dividers.iter().find(|d| d.id == id).unwrap();
                let start = center(divider.bounds);
                let axis = usize::from(divider.axis == Axis::Vertical);
                let mut moved = start;
                moved[axis] += 45.;
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == 5)
                    .unwrap()
                    .root
                    .clone();
                input.perform(serde_json::json!([
                    event("down", start),
                    event("move", moved)
                ]));
                let revision = w.publication.content_revision.get();
                let refreshes = w.publication.refreshes.get();
                let samples = Rc::new(RefCell::new(Vec::new()));
                let missing = Rc::new(RefCell::new(None));
                let clock = w.surface.frame_clock().unwrap();
                let painted = clock.connect_after_paint(glib::clone!(
                    #[strong]
                    samples,
                    #[strong]
                    root,
                    #[strong]
                    missing,
                    #[weak]
                    w,
                    move |clock| {
                        let resolved = w.resolved();
                        let Some(group) = resolved.groups.iter().find(|g| g.id == 5) else {
                            *missing.borrow_mut() = Some(format!(
                                "native={:?}; state={:?}; open={:?}",
                                w.surface.imp().layout.borrow().collapsed,
                                state(&w).workspace.layout.collapsed,
                                state(&w).workspace.layout.column_stacks
                            ));
                            return;
                        };
                        let shared = group.bounds;
                        let actual = root.compute_bounds(&w.surface).unwrap();
                        let size = if axis == 0 {
                            actual.width()
                        } else {
                            actual.height()
                        };
                        let expected = if axis == 0 {
                            shared.width
                        } else {
                            shared.height
                        };
                        samples.borrow_mut().push((
                            clock.frame_time(),
                            size,
                            (size - expected).abs(),
                        ));
                    }
                ));
                let events: Vec<_> = (0..120)
                    .map(|i| {
                        let mut point = moved;
                        point[axis] += (i as f32 * 0.12).sin() * 35.;
                        event("move", point)
                    })
                    .collect();
                input.perform(serde_json::to_value(events).unwrap());
                clock.disconnect(painted);
                assert!(
                    missing.borrow().is_none(),
                    "column vanished while resizing: {:?}",
                    missing.borrow()
                );
                let samples = samples.borrow();
                let changing: Vec<_> = samples
                    .windows(2)
                    .filter(|p| p[0].1 != p[1].1)
                    .map(|p| p[1].0)
                    .collect();
                assert!(
                    changing.len() > 20,
                    "ordinary dock contents must resize continuously"
                );
                let error = samples.iter().map(|s| s.2).fold(0., f32::max);
                assert!(
                    error <= 1.,
                    "painted child allocation differs from shared geometry: {error}"
                );
                let report = serde_json::json!({"theme":format!("{theme:?}"),"touch":touch,"axis":axis,
                    "changed_frames":changing.len(), "max_error_px":error,
                    "geometry_hz":(changing.len()-1) as f64 * 1e6 / (changing.last().unwrap()-changing[0]) as f64});
                std::fs::write(
                    output.join(format!("resize-{theme:?}-{touch}-{axis}.json")),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
                assert_eq!(w.publication.content_revision.get(), revision);
                assert_eq!(w.publication.refreshes.get(), refreshes);
                assert_eq!(
                    w.groups.borrow().iter().find(|g| g.id == 5).unwrap().root,
                    root
                );
                input.perform(serde_json::json!([
                    event("move", moved),
                    event("up", moved)
                ]));
                let shared = w
                    .resolved()
                    .groups
                    .into_iter()
                    .find(|g| g.id == 5)
                    .unwrap()
                    .bounds;
                let actual = root.compute_bounds(&w.surface).unwrap();
                assert!(
                    (actual.width() - shared.width).abs() <= 1.
                        && (actual.height() - shared.height).abs() <= 1.
                );
            }
            input.perform(click(center(icon(Panel::Layers))));
            assert_eq!(
                state(&w).workspace.layout.column_stack(4).open_column,
                Some(8)
            );
            let resolved = w.resolved();
            let divider = resolved
                .dividers
                .iter()
                .find(|d| resolved.open_column_at_divider(d.id) == Some(8))
                .unwrap();
            let open = resolved
                .collapsed
                .iter()
                .find(|c| c.id == 8)
                .unwrap()
                .open
                .as_ref()
                .unwrap();
            let start = center(divider.bounds);
            let inward = if open.direction == Edge::Right {
                -600.
            } else {
                600.
            };
            let moved = [(start[0] + inward).clamp(2., viewport[0] - 2.), start[1]];
            input.perform(serde_json::json!([
                event("down", start),
                event("move", moved),
                event("up", moved)
            ]));
            let resolved = w.resolved();
            let open = resolved
                .collapsed
                .iter()
                .find(|c| c.id == 8)
                .unwrap()
                .open
                .as_ref()
                .unwrap();
            assert!(open.bounds.width >= layer_ui::LAYERS_MIN_WIDTH);
            assert_eq!(state(&w).workspace.layout.column_stack(4).members, [4, 8]);
            let group = resolved
                .groups
                .iter()
                .find(|g| g.panels.contains(&Panel::Layers))
                .unwrap();
            let root = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.id == group.id)
                .unwrap()
                .root
                .clone();
            let actual = root.compute_bounds(&w.surface).unwrap();
            assert!((actual.width() - group.bounds.width).abs() <= 1.);
            assert_eq!(
                w.resolved()
                    .collapsed
                    .iter()
                    .filter(|c| c.open.is_some())
                    .count(),
                1
            );
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnAutoHide {
                    column: 8,
                    auto_hide: true,
                },
            });
            pump(160);
            input.perform(click([viewport[0] * 0.5, viewport[1] * 0.5]));
            assert!(
                state(&w)
                    .workspace
                    .layout
                    .column_stack(4)
                    .open_column
                    .is_none()
            );
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnDrawers {
                    column: 8,
                    drawers: true,
                },
            });
            pump(160);
            input.perform(click(center(icon(Panel::Layers))));
            assert!(state(&w).customization.column_drawers[0].tabs.is_some());
            input
                .perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
            assert!(state(&w).customization.column_drawers.is_empty());
            // Pulling a member to the opposite side makes it a singleton stack.
            let r = w.resolved();
            let source = center(r.collapsed.iter().find(|c| c.id == 8).unwrap().grip);
            let target = [
                if edge == Edge::Left {
                    viewport[0] - 2.
                } else {
                    2.
                },
                viewport[1] * 0.5,
            ];
            input.perform(serde_json::json!([
                event("down", source),
                event("move", target),
                event("up", target)
            ]));
            assert_eq!(state(&w).workspace.layout.column_stack(8).members, [8]);
            assert_eq!(state(&w).workspace.layout.column_stack(4).members, [4]);
        }
    }
}
