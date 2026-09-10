//! Native control/lifecycle integration on a hardware desktop. Control signals
//! exercise GTK bindings; pen records exercise scheduling and GPU presentation.
//! Physical tablet/touch delivery remains a human test (not faked here).
use super::*;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_ui::FloatingToolbarLayout;
use std::time::{Duration, Instant};

fn pump(ms: u64) {
    let until = Instant::now() + Duration::from_millis(ms);
    let context = glib::MainContext::default();
    while Instant::now() < until {
        while context.pending() && Instant::now() < until {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn state(w: &Workspace) -> UiState {
    w.gpu.borrow().as_ref().unwrap().session.state().clone()
}

struct NativeTestApp(adw::Application);
impl std::ops::Deref for NativeTestApp {
    type Target = adw::Application;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Drop for NativeTestApp {
    fn drop(&mut self) {
        // A failed assertion must not leave GPU workers alive while the test
        // process tears down GTK and the Vulkan driver.
        for window in self.0.windows() {
            window.destroy();
        }
    }
}
fn native_test_app(id: &str) -> NativeTestApp {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(&crate::stylesheet());
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id(id)
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    NativeTestApp(app)
}

fn assert_drawer_connected(w: &Workspace) {
    let p = w.drawer.placement().unwrap();
    let c = p.connection().unwrap();
    let connection = find_named(w.surface.upcast_ref(), "drawer-connection").unwrap();
    assert!(connection.is_mapped() && connection.can_target());
    let actual = connection.compute_bounds(&w.surface).unwrap();
    assert!((actual.x() - c.bounds.x).abs() <= 1.0);
    assert!((actual.y() - c.bounds.y).abs() <= 1.0);
    assert!((actual.width() - c.bounds.width).abs() <= 1.0);
    assert!((actual.height() - c.bounds.height).abs() <= 1.0);
    let center = [
        c.bounds.x + c.bounds.width * 0.5,
        c.bounds.y + c.bounds.height * 0.5,
    ];
    assert_eq!(
        w.surface
            .pick(center[0] as f64, center[1] as f64, gtk::PickFlags::DEFAULT),
        Some(connection)
    );
    w.chrome_event(ChromeEvent::Contact {
        position: center,
        canvas: false,
    });
    assert!(
        state(w).customization.drawer.is_some(),
        "the connecting stem is part of the drawer, not an outside click"
    );
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_drawers() {
    let app = native_test_app("art.capycanvas.ToolDrawers");
    let w = Workspace::new(&app);
    w.window.present();
    pump(800);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    let old = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .to_vec();
    for tile in old {
        workspace
            .layout
            .remove_tool(Panel::Toolbar, tile.id)
            .unwrap();
    }
    let controls = [
        ToolbarControl::Command {
            command: CommandId::Pen,
        },
        ToolbarControl::Command {
            command: CommandId::Pencil,
        },
        ToolbarControl::Color,
    ]
    .into_iter()
    .chain(
        Panel::ALL
            .into_iter()
            .filter(|p| p.kind() == PanelKind::Content)
            .map(|panel| ToolbarControl::Panel { panel }),
    )
    .collect::<Vec<_>>();
    workspace
        .layout
        .insert_tools(Panel::Toolbar, None, &controls)
        .unwrap();
    let ids = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .map(|t| t.id)
        .collect::<Vec<_>>();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Toolbar,
            DockTarget::Edge {
                edge: Edge::Left,
                outer: true,
            },
        )
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(200);
    let output = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for id in &ids {
            let button = find_named(w.surface.upcast_ref(), &format!("tile-{id}"))
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap();
            click(&button);
            if state(&w).customization.drawer.is_none() {
                click(&button);
            }
            pump(300);
            assert!(state(&w).customization.drawer.is_some(), "tile {id}");
            assert_drawer_connected(&w);
            let drawer = find_named(w.surface.upcast_ref(), "tool-drawer").unwrap();
            assert!(drawer.is_mapped());
            let b = drawer.compute_bounds(&w.surface).unwrap();
            assert!(b.width() > 100.0 && b.height() > 30.0, "tile {id}: {b:?}");
            assert!(b.x() >= 0.0 && b.y() >= HEADER_HEIGHT);
            assert!(b.x() + b.width() <= viewport[0] && b.y() + b.height() <= viewport[1]);
            if *id == ids[0] {
                let size = find_named(&drawer, "tool-setting-size")
                    .unwrap()
                    .downcast::<crate::number_control::NumberControl>()
                    .unwrap();
                edit_number(&size, "12*3");
                assert_eq!(state(&w).brush.diameter, 36.0);
                assert_eq!(w.size_number.value(), 36.0);
            }
            if controls[ids.iter().position(|t| t == id).unwrap()]
                == (ToolbarControl::Panel {
                    panel: Panel::Stats,
                })
            {
                w.dispatch(UiAction::SetLayerOpacity {
                    id: None,
                    opacity: if theme == Theme::Dark { 0.9 } else { 1.0 },
                });
                pump(250);
                let telemetry = w.gpu.borrow().as_ref().unwrap().session.renderer_stats();
                assert_ne!(
                    telemetry
                        .rows
                        .iter()
                        .find(|r| r.label == "GPU · ms")
                        .unwrap()
                        .value,
                    "Unavailable"
                );
                assert_ne!(
                    telemetry
                        .rows
                        .iter()
                        .find(|r| r.label == "Frames")
                        .unwrap()
                        .value,
                    "0"
                );
            }
            // The original dock still owns its own live panel body.
            for (panel, widget) in &w.panels {
                if state(&w).workspace.layout.panel_group(*panel).is_some() {
                    assert!(widget.parent().is_some());
                }
            }
            capture_reference(&w, &format!("{output}/drawer-{id}-{theme:?}.png"), 1.0);
            click(&button);
            pump(240);
            assert!(state(&w).customization.drawer.is_none());
            assert!(find_named(w.surface.upcast_ref(), "tool-drawer").is_none());
        }
    }
    // Two live filter projections share one GPU producer and the same textures.
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    pump(1000);
    let filter_button = find_named(w.surface.upcast_ref(), &format!("tile-{}", ids[8]))
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    click(&filter_button);
    pump(1200);
    let filter_texture = |root: &gtk::Widget| {
        find_css(root, "filter-row")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::Picture>()
            .unwrap()
            .paintable()
            .unwrap()
    };
    assert_eq!(
        filter_texture(w.effects.adjustments.upcast_ref()),
        filter_texture(w.drawer.effects().unwrap().adjustments.upcast_ref())
    );
    for root in [
        w.effects.adjustments.clone(),
        w.drawer.effects().unwrap().adjustments.clone(),
    ] {
        let scroll = find_css(root.upcast_ref(), "filter-picker-scroll").unwrap();
        let bounds = scroll.compute_bounds(&root).unwrap();
        assert_eq!(bounds.x(), 0.0);
        assert_eq!(
            bounds.width(),
            root.width() as f32,
            "Filters scrollbar reaches the panel edge in docks and drawers"
        );
        let header = find_css(root.upcast_ref(), "filter-picker-header").unwrap();
        assert_eq!(
            header.compute_bounds(&root).unwrap().x(),
            6.0,
            "moving the scrollbar preserves content padding"
        );
    }
    let requests = w.effects.preview_requests();
    assert!(requests > 0);
    click(&filter_button);
    pump(500);
    click(&filter_button);
    pump(800);
    assert_eq!(
        w.effects.preview_requests(),
        requests,
        "closing/reopening must reuse the preview cache"
    );
    click(&filter_button);
    pump(250);
    let layers = find_named(w.surface.upcast_ref(), &format!("tile-{}", ids[7]))
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    let requests = w.layer_panel.preview_requests();
    click(&layers);
    pump(500);
    assert_eq!(
        w.layer_panel.preview_requests(),
        requests,
        "unchanged layers reuse cached thumbnails"
    );
    let layer_view = w.drawer.layers().unwrap();
    click(
        &layer_view
            .footer
            .last_child()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
    pump(80);
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.is_visible())
        .unwrap();
    assert_eq!(menu.parent().as_ref(), Some(layer_view.root.upcast_ref()));
    assert!(
        menu.is_mapped(),
        "the menu belongs to its drawer projection, not the hidden dock"
    );
    menu.popdown();
    pump(50);
    click(&layers);
    pump(250);
    for edge in [Edge::Top, Edge::Bottom, Edge::Right] {
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            target: DockTarget::Edge { edge, outer: false },
            viewport,
        });
        pump(150);
        let button = find_named(w.surface.upcast_ref(), &format!("tile-{}", ids[2]))
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        click(&button);
        pump(300);
        assert!(state(&w).customization.drawer.is_some());
        assert_drawer_connected(&w);
        capture_reference(&w, &format!("{output}/drawer-color-{edge:?}.png"), 1.0);
        let dismissed = w.chrome_event(ChromeEvent::Contact {
            position: [1100.0, 700.0],
            canvas: true,
        });
        assert!(dismissed.handled);
        assert!(state(&w).customization.drawer.is_none());
        pump(250);
    }
    w.window.close();
    pump(50);
}

fn native_pen_path(w: &Rc<Workspace>, points: &[[f32; 2]]) {
    let camera = state(w).camera;
    let m = camera.document_to_surface();
    for (i, p) in points.iter().enumerate() {
        let now = glib::monotonic_time() as u64 * 1000;
        w.input.send(
            w,
            PenEvent {
                device_id: 92,
                sequence: now,
                timestamp_ns: now,
                view_revision: camera.revision,
                surface_position: Point {
                    x: m[0] * p[0] + m[2] * p[1] + m[4],
                    y: m[1] * p[0] + m[3] * p[1] + m[5],
                },
                pressure: 1.,
                tilt_radians: [0.; 2],
                twist_radians: 0.,
                distance: 0.,
                phase: if i == 0 {
                    PenPhase::Down
                } else if i + 1 == points.len() {
                    PenPhase::Up
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            },
        );
        pump(20);
    }
    pump(180);
    assert!(!w.status.is_visible(), "{}", w.status.text());
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_selected_brushes() {
    use layer_core::DefaultBrushPreset;
    let app = native_test_app("art.capycanvas.SelectedBrushes");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    w.dispatch(UiAction::Invoke {
        command: CommandId::Lasso,
    });
    native_pen_path(
        &w,
        &[
            [760., 460.],
            [1280., 460.],
            [1280., 1060.],
            [760., 1060.],
            [760., 460.],
        ],
    );
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .selection
            .is_some()
    );
    for (i, (preset, color)) in [
        (DefaultBrushPreset::GPen, [0.8, 0.1, 0.25, 1.]),
        (DefaultBrushPreset::WetRound, [0.1, 0.4, 0.8, 1.]),
        (DefaultBrushPreset::WatercolorWash, [0.2, 0.6, 0.3, 1.]),
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::SelectBrush { id: preset as u32 });
        w.dispatch(UiAction::SetBrushSize { value: 200. });
        w.dispatch(UiAction::SetColor { rgba: color });
        let y = 550. + i as f32 * 200.;
        native_pen_path(
            &w,
            &(0..25)
                .map(|j| [580. + j as f32 * 38., y])
                .collect::<Vec<_>>(),
        );
    }
    {
        let gpu = w.gpu.borrow();
        let doc = gpu.as_ref().unwrap().session.engine().document();
        assert_eq!(doc.strokes().count(), 3);
        assert!(doc.strokes().all(|s| s.selection.is_some()));
    }
    let dir = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(150);
        capture_reference(&w, &format!("{dir}/selected-brushes-{theme:?}.png"), 1.);
    }
    w.dispatch(UiAction::Layer {
        action: LayerAction::InvertSelection,
    });
    w.dispatch(UiAction::SelectBrush {
        id: DefaultBrushPreset::GPen as u32,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.55, 0.25, 0.8, 1.],
    });
    native_pen_path(
        &w,
        &(0..25)
            .map(|j| [580. + j as f32 * 38., 755.])
            .collect::<Vec<_>>(),
    );
    w.dispatch(UiAction::Layer {
        action: LayerAction::Deselect,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Undo,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Redo,
    });
    pump(200);
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(150);
        capture_reference(
            &w,
            &format!("{dir}/selection-inverted-replayed-{theme:?}.png"),
            1.,
        );
    }
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_gradient_tool() {
    let app = native_test_app("art.capycanvas.GradientTool");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[ToolbarControl::Command {
                command: CommandId::Gradient,
            }],
        )
        .unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    w.dispatch(UiAction::SetColor {
        rgba: [0.1, 0.25, 0.9, 1.0],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Background,
        },
    });
    w.dispatch(UiAction::SetColor {
        rgba: [1.0, 0.6, 0.1, 1.0],
    });
    w.dispatch(UiAction::Color {
        action: layer_ui::ColorAction::Select {
            slot: layer_ui::ColorSlot::Foreground,
        },
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::Gradient,
    });
    pump(150);
    assert_eq!(w.tool_set.buttons.borrow().len(), 4);
    let opacity = find_named(&w.panel_widget(Panel::ToolSettings), "tool-setting-opacity")
        .unwrap()
        .downcast::<crate::number_control::NumberControl>()
        .unwrap();
    edit_number(&opacity, "80");
    assert!((state(&w).brush.opacity - 0.8).abs() < 0.001);
    let dir = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(dir).unwrap();
    for (index, name) in [
        "linear-colors",
        "linear-clear",
        "radial-colors",
        "radial-clear",
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::Layer {
            action: LayerAction::Clear { id: 1 },
        });
        pump(50);
        let button = w.tool_set.buttons.borrow()[index].1.clone();
        click(&button);
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
        for (i, (phase, p)) in [
            (PenPhase::Down, [760.0, 620.0]),
            (PenPhase::Move, [1220.0, 850.0]),
            (PenPhase::Up, [1220.0, 850.0]),
        ]
        .into_iter()
        .enumerate()
        {
            let e = PenEvent {
                device_id: 1,
                sequence: i as u64,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: m[0] * p[0] + m[2] * p[1] + m[4],
                    y: m[1] * p[0] + m[3] * p[1] + m[5],
                },
                pressure: 1.0,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            w.cursor_input(Some(e));
            w.input.send(&w, e);
            pump(25);
        }
        pump(200);
        assert!(!w.status.is_visible(), "{}", w.status.text());
        let color = |phase| {
            let e = PenEvent {
                device_id: 1,
                sequence: 100,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: m[0] * 760.0 + m[2] * 620.0 + m[4],
                    y: m[1] * 760.0 + m[3] * 620.0 + m[5],
                },
                pressure: 1.0,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase,
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            w.input.send(&w, e);
        };
        // Probe rendered pigment using the real asynchronous GPU path.
        w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::PickLayer,
            },
        });
        color(PenPhase::Down);
        color(PenPhase::Up);
        pump(150);
        assert!(state(&w).colors.foreground[2] > 0.85 && state(&w).colors.foreground[0] < 0.2);
        w.dispatch(UiAction::SetColor {
            rgba: [0.1, 0.25, 0.9, 1.0],
        });
        w.dispatch(UiAction::Invoke {
            command: CommandId::Gradient,
        });
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(150);
            capture_reference(&w, &format!("{dir}/gradient-{name}-{theme:?}.png"), 1.0);
        }
        w.dispatch(UiAction::Invoke {
            command: CommandId::Undo,
        });
        pump(75);
        w.dispatch(UiAction::Invoke {
            command: CommandId::Redo,
        });
        pump(75);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_navigation_tools() {
    let app = native_test_app("art.capycanvas.NavigationTools");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[
                ToolbarControl::Command {
                    command: CommandId::Hand,
                },
                ToolbarControl::Command {
                    command: CommandId::Eyedropper,
                },
            ],
        )
        .unwrap();
    let ids: Vec<_> = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .iter()
        .rev()
        .take(2)
        .map(|t| t.id)
        .collect();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::GPen as u32,
    });
    w.dispatch(UiAction::SetBrushSize { value: 512.0 });
    w.dispatch(UiAction::SetColor {
        rgba: [1.0, 0.0, 0.0, 1.0],
    });
    let contact = |phase| {
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
        let e = PenEvent {
            device_id: 1,
            sequence: 0,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            view_revision: camera.revision,
            surface_position: Point {
                x: m[0] * 1024.0 + m[2] * 768.0 + m[4],
                y: m[1] * 1024.0 + m[3] * 768.0 + m[5],
            },
            pressure: 1.0,
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        w.cursor_input(Some(e));
        w.input.send(&w, e);
    };
    contact(PenPhase::Down);
    contact(PenPhase::Up);
    pump(250);
    w.dispatch(UiAction::SetLayerOpacity {
        id: None,
        opacity: 0.5,
    });
    w.dispatch(UiAction::SetColor {
        rgba: [0.0, 0.0, 0.0, 1.0],
    });
    pump(200);
    let tile = |id| {
        find_named(w.surface.upcast_ref(), &format!("tile-{id}"))
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
    };
    let eye = tile(ids[0]);
    click(&eye);
    assert!(eye.tooltip_text().unwrap().ends_with("(I)"));
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::PickVisible);
    contact(PenPhase::Down);
    contact(PenPhase::Up);
    pump(250);
    let color = state(&w).colors.foreground;
    assert!(
        color[0] > 0.99 && (color[1] - 0.735).abs() < 0.015 && (color[2] - 0.735).abs() < 0.015,
        "visible color {color:?}"
    );
    let subtool = w.tool_set.buttons.borrow()[1].1.clone();
    click(&subtool);
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::PickLayer);
    contact(PenPhase::Down);
    contact(PenPhase::Up);
    pump(250);
    let color = state(&w).colors.foreground;
    for (actual, expected) in color.into_iter().zip([1.0, 0.0, 0.0, 1.0]) {
        assert!((actual - expected).abs() < 0.001, "raw color {color:?}");
    }
    assert!(
        !w.ticking.get(),
        "sampling completes and stops the frame timer"
    );
    let dir = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        capture_reference(&w, &format!("{dir}/eyedropper-{theme:?}.png"), 1.0);
    }
    click(&tile(ids[1]));
    assert_eq!(state(&w).layer_tools.tool, LayerCanvasTool::Hand);
    let camera = state(&w).camera;
    for (phase, position) in [
        (ContactPhase::Down, [600.0, 400.0]),
        (ContactPhase::Move, [700.0, 450.0]),
        (ContactPhase::Up, [700.0, 450.0]),
    ] {
        let reply = w.interact(UiInput::Pointer {
            id: 42,
            phase,
            kind: PointerKind::Mouse,
            button: PointerButton::Primary,
            position,
        });
        assert!(!reply.paint && reply.pan_cursor);
    }
    pump(200);
    let p = camera.input_transform().map(Point { x: 600.0, y: 400.0 });
    let after = state(&w)
        .camera
        .input_transform()
        .map(Point { x: 700.0, y: 450.0 });
    assert!((p.x - after.x).abs() < 0.001 && (p.y - after.y).abs() < 0.001);
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_navigator() {
    let app = native_test_app("art.capycanvas.NavigatorReview");
    let w = Workspace::new(&app);
    w.window.present();
    pump(800);
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Navigator,
            visible: true,
        },
    });
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    let layers = workspace.layout.panel_group(Panel::Layers).unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Navigator,
            DockTarget::Split {
                group: layers,
                edge: Edge::Top,
            },
        )
        .unwrap();
    let navigator = workspace.layout.panel_group(Panel::Navigator).unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::Stats, true)
        .unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Stats,
            DockTarget::Tab {
                group: navigator,
                index: None,
            },
        )
        .unwrap();
    workspace
        .layout
        .select_tab(navigator, Panel::Navigator)
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(250);
    w.dispatch(UiAction::SetBrushSize { value: 96.0 });
    let mut sequence = 0;
    for (row, rgba) in [
        [0.75, 0.18, 0.2, 1.0],
        [0.18, 0.55, 0.36, 1.0],
        [0.2, 0.36, 0.75, 1.0],
    ]
    .into_iter()
    .enumerate()
    {
        w.dispatch(UiAction::SetColor { rgba });
        let camera = state(&w).camera;
        let m = camera.document_to_surface();
        for i in 0..32 {
            let x = 200.0 + i as f32 * 50.0;
            let y = 350.0 + row as f32 * 350.0 + (i as f32 * 0.2).sin() * 120.0;
            sequence += 1;
            w.gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .pen(PenEvent {
                    device_id: 91,
                    sequence,
                    timestamp_ns: glib::monotonic_time() as u64 * 1000,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * x + m[2] * y + m[4],
                        y: m[1] * x + m[3] * y + m[5],
                    },
                    pressure: 1.0,
                    tilt_radians: [0.0; 2],
                    twist_radians: 0.0,
                    distance: 0.0,
                    phase: if i == 0 {
                        PenPhase::Down
                    } else if i == 31 {
                        PenPhase::Up
                    } else {
                        PenPhase::Move
                    },
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .unwrap();
        }
        w.wake();
        pump(250);
    }
    pump(400);
    let original = w
        .navigator_images
        .texture()
        .expect("live document overview");
    assert_eq!([original.width(), original.height()], [256, 192]);
    let doc_revision = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .revision;
    let zoom = state(&w).camera.zoom;
    let button = |id: CommandId| {
        find_named(w.navigator.root.upcast_ref(), &format!("navigator-{id:?}"))
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
    };
    for _ in 0..3 {
        click(&button(CommandId::ZoomIn));
    }
    click(&button(CommandId::RotateRight));
    click(&button(CommandId::FlipHorizontal));
    pump(300);
    assert!((state(&w).camera.zoom - zoom * 8.0_f32.sqrt()).abs() < 0.001);
    assert_eq!(state(&w).camera.flipped, [true, false]);
    assert!(button(CommandId::FlipHorizontal).has_css_class("selected-tool"));
    assert_eq!(
        w.navigator_images.texture(),
        Some(original.clone()),
        "camera-only commands reuse the exact texture"
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .revision,
        doc_revision
    );
    let overview = find_named(w.navigator.root.upcast_ref(), "navigator-overview").unwrap();
    assert!(
        w.navigator.root.measure(gtk::Orientation::Horizontal, -1).0 <= 192,
        "compact control row must fit minimum panel width"
    );
    let size = [overview.width() as f32, overview.height() as f32];
    let g = NavigatorGeometry::new(&state(&w).camera, [2048, 1536], size).unwrap();
    let position = [
        g.image.x + g.image.width * 0.3,
        g.image.y + g.image.height * 0.65,
    ];
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Down,
        position,
        viewport: size,
    });
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Move,
        position: [position[0] + 10.0, position[1]],
        viewport: size,
    });
    w.dispatch(UiAction::Navigator {
        phase: ContactPhase::Up,
        position,
        viewport: size,
    });
    pump(200);
    let dir = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreSettings {
            settings: Settings {
                theme: Some(theme),
                ..Settings::default()
            },
        });
        pump(250);
        capture_reference(&w, &format!("{dir}/navigator-{theme:?}.png"), 1.0);
    }
    // A second projection shares the same image/cache producer.
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .insert_tools(
            Panel::Toolbar,
            None,
            &[ToolbarControl::Panel {
                panel: Panel::Navigator,
            }],
        )
        .unwrap();
    let tile = workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()
        .last()
        .unwrap()
        .id;
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(150);
    click(
        &find_named(w.surface.upcast_ref(), &format!("tile-{tile}"))
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
    pump(300);
    assert!(
        find_named(w.surface.upcast_ref(), "drawer-panel-Navigator")
            .unwrap()
            .is_mapped()
    );
    assert_eq!(w.navigator_images.texture(), Some(original));
    capture_reference(&w, &format!("{dir}/navigator-drawer.png"), 1.0);
    assert!(!w.status.is_visible(), "{}", w.status.text());
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_families() {
    let app = native_test_app("art.capycanvas.ToolFamilies");
    let w = Workspace::new(&app);
    w.window.present();
    pump(800);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    let toolbar = workspace
        .layout
        .panels
        .iter_mut()
        .find(|p| p.id == Panel::Toolbar)
        .unwrap();
    if let layer_ui::PanelContent::Toolbar { tiles, .. } = &mut toolbar.content {
        *tiles = layer_ui::Tool::ALL
            .into_iter()
            .enumerate()
            .map(|(i, tool)| layer_ui::ToolbarTile {
                id: i as u32 + 1,
                control: layer_ui::ToolbarControl::Command {
                    command: tool.command(),
                },
            })
            .collect();
    }
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Toolbar,
            DockTarget::Edge {
                edge: Edge::Left,
                outer: true,
            },
        )
        .unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::ToolSettings,
            DockTarget::Float {
                position: [500., 120.],
            },
        )
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(150);
    let output = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for tool in layer_ui::Tool::ALL {
            click(&command(&w, tool.command()));
            if state(&w).customization.drawer.is_some() {
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::CloseExpanded,
                });
                pump(220);
            }
            pump(80);
            assert_eq!(state(&w).brush.tool, tool);
            let groups = state(&w).tool_set.groups;
            for (index, _) in groups.iter().enumerate() {
                // Activate actual native group and brush buttons, not just the
                // state API. Copy the widget before invoking its callback.
                let button = w.tool_set.group_buttons.borrow()[index].clone();
                click(&button);
                pump(30);
                assert!(button.has_css_class("selected-tool"));
                let bounds = button.compute_bounds(&w.tool_set.root).unwrap();
                assert_eq!(
                    bounds.width(),
                    layer_ui::TOOL_PANEL_MIN_WIDTH - 2. * layer_ui::PANEL_CONTENT_INSET
                );
                assert_eq!(bounds.height(), layer_ui::TILE_SIZE);
                let buttons = w.tool_set.buttons.borrow().clone();
                for (item, button, _) in buttons {
                    click(&button);
                    pump(20);
                    assert_eq!(Some(state(&w).brush.preset), item.preview);
                    assert!(button.has_css_class("selected-tool"));
                    assert_eq!(
                        w.tool_set
                            .buttons
                            .borrow()
                            .iter()
                            .filter(|(_, b, _)| b.has_css_class("selected-tool"))
                            .count(),
                        1
                    );
                    let snapshot = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .unwrap()
                        .session
                        .engine()
                        .configured_brush()
                        .clone();
                    assert_eq!(snapshot.diameter, state(&w).brush.diameter);
                    assert_eq!(snapshot.opacity, state(&w).brush.opacity);
                    if tool == layer_ui::Tool::Liquify {
                        assert_eq!(
                            snapshot.execution_class(),
                            layer_core::BrushExecution::Liquify
                        );
                    }
                }
            }
            capture_reference(&w, &format!("{output}/tool-{tool:?}-{theme:?}.png"), 1.0);
        }
    }
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_tool_and_color_panels() {
    let app = native_test_app("art.capycanvas.ToolPanels");
    let w = Workspace::new(&app);
    w.window.present();
    pump(800);
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .set_panel_visible(Panel::ToolSettings, true)
        .unwrap();
    workspace
        .layout
        .set_panel_visible(Panel::Color, true)
        .unwrap();
    for (panel, x) in [(Panel::ToolSettings, 330.), (Panel::Color, 610.)] {
        workspace
            .layout
            .move_panel(
                [1200., 900.],
                panel,
                DockTarget::Float {
                    position: [x, 130.],
                },
            )
            .unwrap();
    }
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(200);
    for choice in layer_ui::brush_catalog() {
        w.dispatch(UiAction::SelectBrush { id: choice.id });
        pump(10);
        assert!(
            w.panel_widget(Panel::ToolSettings)
                .measure(gtk::Orientation::Horizontal, -1)
                .0
                <= layer_ui::TOOL_PANEL_MIN_WIDTH as i32,
            "{} settings are too wide",
            choice.label
        );
    }
    assert!(
        w.panel_widget(Panel::Brushes)
            .measure(gtk::Orientation::Horizontal, -1)
            .0
            <= layer_ui::TOOL_PANEL_MIN_WIDTH as i32
    );
    w.dispatch(UiAction::SelectBrush {
        id: layer_core::DefaultBrushPreset::WetWatercolor as u32,
    });
    let flow = find_named(&w.panel_widget(Panel::ToolSettings), "tool-setting-flow")
        .unwrap()
        .downcast::<crate::number_control::NumberControl>()
        .unwrap();
    edit_number(&flow, "25+10");
    assert!(
        (state(&w)
            .tool_settings
            .iter()
            .find(|s| s.id == "flow")
            .unwrap()
            .value
            - 0.35)
            .abs()
            < 1e-6
    );
    let color = w.panel_widget(Panel::Color);
    click(
        &find_named(&color, "color-Background")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.slot, layer_ui::ColorSlot::Background);
    click(
        &find_named(&color, "color-Transparent")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert!(state(&w).colors.transparent());
    let hue = find_named(&color, "color-component-0")
        .unwrap()
        .downcast::<crate::number_control::NumberControl>()
        .unwrap();
    edit_number(&hue, "180/2");
    assert_eq!(state(&w).colors.slot, layer_ui::ColorSlot::Background);
    assert!((state(&w).colors.components()[0] - 90.0).abs() < 1e-4);
    click(
        &find_named(&color, "color-space")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.space, layer_ui::ColorSpace::Hls);
    let before = state(&w).colors;
    click(
        &find_named(&color, "color-swap")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).colors.foreground, before.background);
    assert_eq!(state(&w).colors.background, before.foreground);
    click(
        &find_named(&color, "color-Foreground")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.2, 0.72, 0.34, 1.],
    });
    let output = std::path::Path::new("../../artifacts/familiar-workspace");
    std::fs::create_dir_all(output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(100);
        for space in [layer_ui::ColorSpace::Hsv, layer_ui::ColorSpace::Hls] {
            w.dispatch(UiAction::Color {
                action: layer_ui::ColorAction::Space { space },
            });
            pump(100);
            capture_reference(
                &w,
                output
                    .join(format!("color-{space:?}-{theme:?}.png"))
                    .to_str()
                    .unwrap(),
                1.0,
            );
        }
    }
    let mut workspace = state(&w).workspace;
    workspace
        .layout
        .move_panel(
            [1200., 900.],
            Panel::Brushes,
            DockTarget::Float {
                position: [320., 150.],
            },
        )
        .unwrap();
    for float in &mut workspace.layout.floating {
        if let DockNode::Tabs { panels, .. } = &float.root
            && (panels.contains(&Panel::Brushes) || panels.contains(&Panel::ToolSettings))
        {
            float.width = layer_ui::TOOL_PANEL_MIN_WIDTH;
            float.height = Some(600.0);
            float.position = [
                if panels.contains(&Panel::Brushes) {
                    260.0
                } else {
                    400.0
                },
                120.0,
            ];
        } else {
            float.position = [600.0, 120.0];
        }
    }
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(150);
    capture_reference(
        &w,
        output.join("three-tile-minimum.png").to_str().unwrap(),
        1.0,
    );
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_runtime_filter_packages() {
    let app = native_test_app("art.capycanvas.RuntimeFilters");
    let w = Workspace::new(&app);
    w.window.present();
    pump(900);
    let load = |path: &str, mode| {
        crate::canvas::load_filter_directory(
            &mut w.gpu.borrow_mut().as_mut().unwrap().session,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path),
            mode,
        )
        .unwrap();
        w.wake();
        let deadline = Instant::now() + Duration::from_secs(20);
        while state(&w).filter_load.pending && Instant::now() < deadline {
            pump(20);
        }
        assert!(!state(&w).filter_load.pending);
        assert!(
            state(&w).filter_load.error.is_none(),
            "{:?}",
            state(&w).filter_load.error
        );
    };
    load("../../assets/filters", layer_core::EffectInstallMode::Merge);
    assert_eq!(state(&w).adjustments.len(), 40);
    load(
        "../../examples/filters/tent-blur",
        layer_core::EffectInstallMode::Merge,
    );
    assert_eq!(state(&w).adjustments.len(), 41);
    let pixels: Vec<u8> = (0..1024 * 768)
        .flat_map(|i| {
            if (i % 1024 / 32 + i / 1024 / 32) % 2 == 0 {
                [230, 50, 80, 255]
            } else {
                [30, 160, 220, 255]
            }
        })
        .collect();
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_image(
            "Runtime checker",
            layer_render::HostImage {
                width: 1024,
                height: 768,
                stride: 4096,
                format: layer_render::PixelFormat::Rgba8Srgb,
                bytes: &pixels,
            },
        )
        .unwrap();
    w.wake();
    pump(100);
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category {
            category: Some("examples".into()),
        },
    });
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    pump(300);
    find_named(
        w.effects.adjustments.upcast_ref(),
        "adjustment-example:tent_blur",
    )
    .unwrap()
    .downcast::<gtk::Button>()
    .unwrap()
    .emit_clicked();
    pump(300);
    let view = state(&w).layer_properties;
    assert_eq!(view.description, "Tent Blur");
    assert_eq!(view.controls[0].label, "Radius");
    assert!(view.enabled);
    w.dispatch(UiAction::Effect {
        action: layer_ui::EffectAction::Set {
            layer: view.layer.unwrap(),
            key: "radius".into(),
            value: layer_core::EffectValue::Number(9.),
        },
    });
    pump(150);
    load(
        "../../examples/filters/tent-blur",
        layer_core::EffectInstallMode::Replace,
    );
    assert_eq!(
        state(&w).layer_properties.controls[0].value,
        layer_core::EffectValue::Number(9.)
    );
    let dir = "../../artifacts/ui/runtime-filters-gtk";
    std::fs::create_dir_all(dir).unwrap();
    crate::capture(&w, &format!("{dir}/runtime-properties.png"));
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "private Wayland display and GPU"]
fn native_adjustment_panels_review() {
    use layer_core::EffectValue;
    use layer_ui::EffectAction;
    let app = native_test_app("art.capycanvas.AdjustmentReview");
    let w = Workspace::new(&app);
    w.window.present();
    pump(900);
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    let bytes: Vec<u8> = (0..512 * 512)
        .flat_map(|i| {
            let x = (i % 512) as f32 / 511.;
            let y = (i / 512) as f32 / 511.;
            [
                (x * 255.) as u8,
                (y * 255.) as u8,
                ((1. - x) * 255.) as u8,
                255,
            ]
        })
        .collect();
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_image(
            "Color study",
            layer_render::HostImage {
                width: 512,
                height: 512,
                stride: 2048,
                format: layer_render::PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
    w.refresh(regions::ALL);
    w.wake();
    pump(300);
    let dir = "../../artifacts/ui/adjustments-gtk";
    std::fs::create_dir_all(dir).unwrap();
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    pump(900);
    crate::capture(&w, &format!("{dir}/01-adjustments.png"));
    let first = find_named(w.effects.adjustments.upcast_ref(), "adjustment-curves").unwrap();
    let picture = first
        .downcast_ref::<gtk::Button>()
        .unwrap()
        .child()
        .unwrap()
        .first_child()
        .unwrap()
        .downcast::<gtk::Picture>()
        .unwrap();
    assert!(
        picture.paintable().is_some(),
        "visible filter rows receive asynchronous GPU previews"
    );
    let second = find_named(w.effects.adjustments.upcast_ref(), "adjustment-levels").unwrap();
    assert!(first.width() > 160);
    assert!(
        second.compute_bounds(&w.effects.adjustments).unwrap().y()
            > first.compute_bounds(&w.effects.adjustments).unwrap().y()
    );
    for (i, kind) in layer_core::bundled_effect_catalog()
        .filters()
        .iter()
        .enumerate()
    {
        w.dispatch(UiAction::SelectPanelTab {
            group: 8,
            panel: Panel::Adjustments,
        });
        pump(80);
        find_named(
            w.effects.adjustments.upcast_ref(),
            &format!("adjustment-{}", kind.id()),
        )
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
        .emit_clicked();
        pump(180);
        assert!(!w.status.is_visible(), "{}", w.status.text());
        assert!(w.effects.properties.is_mapped());
        let s = state(&w);
        let id = s.layer_properties.layer.unwrap();
        assert_eq!(s.layer_properties.description, kind.label());
        if kind.id() == "color_balance" {
            let mut headings = Vec::new();
            let mut child = w.effects.properties.last_child().unwrap().first_child();
            while let Some(widget) = child {
                if widget.has_css_class("property-section") {
                    headings.push(
                        widget
                            .downcast_ref::<gtk::Label>()
                            .unwrap()
                            .text()
                            .to_string(),
                    );
                }
                child = widget.next_sibling();
            }
            assert_eq!(headings, ["Shadows", "Midtones", "Highlights"]);
        }
        let program = kind.program();
        let (key, value) = match kind.id() {
            "curves" => (
                "curve_0",
                EffectValue::Curve(vec![[0., 0.], [0.4, 0.65], [1., 1.]]),
            ),
            "levels" => ("gamma", EffectValue::Number(1.5)),
            "brightness_contrast" => ("contrast", EffectValue::Number(30.)),
            "hue_saturation" => ("hue", EffectValue::Number(40.)),
            "color_balance" => ("midtones_red", EffectValue::Number(25.)),
            "exposure" => ("exposure", EffectValue::Number(1.)),
            "vibrance" => ("vibrance", EffectValue::Number(75.)),
            "black_white" => ("reds", EffectValue::Number(80.)),
            "gradient_map" => (
                "gradient",
                EffectValue::Gradient(vec![
                    layer_core::GradientStop {
                        position: 0.,
                        color: [0.03, 0.05, 0.2, 1.],
                    },
                    layer_core::GradientStop {
                        position: 0.5,
                        color: [0.8, 0.2, 0.1, 1.],
                    },
                    layer_core::GradientStop {
                        position: 1.,
                        color: [1., 0.9, 0.5, 1.],
                    },
                ]),
            ),
            "posterize" => ("levels", EffectValue::Number(4.)),
            _ => {
                let parameter = program
                    .parameters
                    .iter()
                    .find(|p| matches!(p.kind, layer_core::EffectParameterKind::Number { .. }))
                    .unwrap();
                let layer_core::EffectParameterKind::Number { min, max, .. } = parameter.kind
                else {
                    unreachable!()
                };
                (
                    parameter.key.as_ref(),
                    EffectValue::Number(min + (max - min) * 0.3),
                )
            }
        };
        w.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer: id,
                key: key.into(),
                value,
            },
        });
        pump(200);
        assert!(!w.status.is_visible(), "{}", w.status.text());
        if kind.id() == "gradient_map" {
            let bar = find_named(w.effects.properties.upcast_ref(), "effect-gradient").unwrap();
            let controllers = bar.observe_controllers();
            for i in 0..controllers.n_items() {
                if let Some(click) = controllers
                    .item(i)
                    .and_then(|c| c.downcast::<gtk::GestureClick>().ok())
                {
                    click.emit_by_name::<()>(
                        "pressed",
                        &[&1i32, &(bar.width() as f64 * 0.3), &20f64],
                    );
                }
            }
            pump(80);
            let EffectValue::Gradient(stops) = &state(&w).layer_properties.controls[0].value else {
                panic!("gradient control")
            };
            assert_eq!(
                stops.len(),
                4,
                "native gradient insertion is handled by Rust"
            );
        }
        crate::capture(&w, &format!("{dir}/{:02}-{}.png", i + 2, kind.id()));
        w.dispatch(UiAction::SetLayerVisibility { id, visible: false });
    }
    // Category/search changes use the shared policy and preserve the selected
    // editing layer. The GTK view only rebuilds the matching rows.
    w.dispatch(UiAction::SelectPanelTab {
        group: 8,
        panel: Panel::Adjustments,
    });
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category {
            category: Some("distort".into()),
        },
    });
    pump(700);
    crate::capture(&w, &format!("{dir}/41-distort-picker.png"));
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::ToggleSearch,
    });
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Search {
            query: "glass".into(),
        },
    });
    pump(700);
    assert_eq!(state(&w).adjustments.len(), 2);
    crate::capture(&w, &format!("{dir}/42-glass-search.png"));
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::ToggleSearch,
    });
    w.dispatch(UiAction::FilterPicker {
        action: layer_ui::FilterPickerAction::Category { category: None },
    });
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Stats,
            visible: true,
        },
    });
    pump(300);
    // Keep the stable stats exercise on a simple pointwise filter.
    w.dispatch(UiAction::Effect {
        action: EffectAction::Insert {
            effect: "posterize".into(),
        },
    });
    let layer = state(&w).layer_properties.layer.unwrap();
    w.dispatch(UiAction::SetLayerVisibility {
        id: layer,
        visible: true,
    });
    for i in 0..16 {
        w.dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer,
                key: "levels".into(),
                value: EffectValue::Number(i as f32 + 2.),
            },
        });
        pump(20);
    }
    pump(250);
    let stats = w.gpu.borrow().as_ref().unwrap().session.renderer_stats();
    assert!(!stats.samples.is_empty(), "live CPU samples");
    assert_ne!(stats.rows[1].value, "—", "live GPU timestamps");
    crate::capture(&w, &format!("{dir}/07-stats.png"));
    w.window.close();
    pump(80);
}

#[test]
#[ignore = "requires the private Wayland display and hardware GPU"]
fn native_layer_panel_review() {
    use layer_ui::{LayerAction as A, LayerCanvasTool as T};
    fn send(w: &Rc<Workspace>, action: A) {
        w.dispatch(UiAction::Layer { action });
        pump(60);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    fn polygon(w: &Rc<Workspace>, points: &[[f32; 2]], tool: T) {
        send(w, A::Tool { tool });
        let camera = state(w).camera;
        let m = camera.view().document_to_surface;
        for (i, p) in points.iter().enumerate() {
            w.gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .pen(PenEvent {
                    device_id: 91,
                    sequence: i as u64 + 1,
                    timestamp_ns: glib::monotonic_time() as u64 * 1000,
                    view_revision: camera.revision,
                    surface_position: Point {
                        x: m[0] * p[0] + m[2] * p[1] + m[4],
                        y: m[1] * p[0] + m[3] * p[1] + m[5],
                    },
                    pressure: 1.,
                    tilt_radians: [0.; 2],
                    twist_radians: 0.,
                    distance: 0.,
                    phase: if i == 0 {
                        PenPhase::Down
                    } else if i + 1 == points.len() {
                        PenPhase::Up
                    } else {
                        PenPhase::Move
                    },
                    tool: ToolKind::Pen,
                    flags: SampleFlags::PRIMARY,
                })
                .unwrap();
        }
        w.wake();
        pump(180);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    let app = native_test_app("art.capycanvas.LayerPanelReview");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let delete = find_named(w.layer_panel.root.upcast_ref(), "delete-selected-layers")
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    let count = state(&w).layers.len();
    send(
        &w,
        A::New {
            group: false,
            clipped: false,
        },
    );
    assert!(delete.is_sensitive());
    delete.emit_clicked();
    pump(80);
    assert_eq!(state(&w).layers.len(), count);
    w.dispatch(UiAction::SelectLayer { id: 2 });
    assert!(!delete.is_sensitive());
    w.dispatch(UiAction::SelectLayer { id: 1 });
    let dir = "../../artifacts/ui/layers-gtk";
    std::fs::create_dir_all(dir).unwrap();
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    let names = [
        ("Atmosphere", [0.77, 0.86, 0.83, 1.]),
        ("Far hills", [0.35, 0.54, 0.49, 1.]),
        ("Shirt base", [0.65, 0.32, 0.24, 1.]),
        ("Skin base", [0.79, 0.62, 0.40, 1.]),
    ];
    for (i, (name, color)) in names.iter().enumerate() {
        if i > 0 {
            send(
                &w,
                A::New {
                    group: false,
                    clipped: false,
                },
            );
        }
        let id = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
        send(
            &w,
            A::Rename {
                id,
                name: (*name).into(),
            },
        );
        w.dispatch(UiAction::SetColor { rgba: *color });
        let points = match i {
            0 => vec![[180., 160.], [1800., 160.], [1800., 1360.], [180., 1360.]],
            1 => vec![
                [180., 900.],
                [500., 540.],
                [860., 740.],
                [1350., 390.],
                [1800., 850.],
                [1800., 1360.],
                [180., 1360.],
            ],
            2 => vec![
                [700., 690.],
                [980., 640.],
                [1150., 980.],
                [1110., 1350.],
                [560., 1350.],
                [550., 950.],
            ],
            _ => (0..48)
                .map(|i| {
                    let a = i as f32 / 48. * std::f32::consts::TAU;
                    [830. + a.cos() * 195., 500. + a.sin() * 260.]
                })
                .collect(),
        };
        polygon(&w, &points, T::LassoFill);
        send(&w, A::AlphaLock { id, value: true });
    }
    send(
        &w,
        A::New {
            group: false,
            clipped: true,
        },
    );
    let shading = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    send(
        &w,
        A::Rename {
            id: shading,
            name: "Skin · soft shadow".into(),
        },
    );
    send(
        &w,
        A::Blend {
            id: shading,
            value: 1,
        },
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.32, 0.25, 0.21, 0.6],
    });
    polygon(
        &w,
        &[[810., 200.], [1100., 200.], [1100., 820.], [900., 800.]],
        T::LassoFill,
    );
    // A deterministic source-image fixture, not a CPU canvas implementation.
    let bytes: Vec<u8> = (0..2048 * 1536)
        .flat_map(|i| {
            let x = i % 2048;
            let y = i / 2048;
            if (x / 12 + y / 12) % 2 == 0 {
                [220, 170, 66, 255]
            } else {
                [178, 124, 42, 255]
            }
        })
        .collect();
    w.gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .import_layer_image(
            "Fabric texture",
            layer_render::HostImage {
                width: 2048,
                height: 1536,
                stride: 8192,
                format: layer_render::PixelFormat::Rgba8Srgb,
                bytes: &bytes,
            },
        )
        .unwrap();
    w.wake();
    pump(200);
    let texture = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    polygon(
        &w,
        &[[640., 870.], [940., 810.], [1080., 1280.], [680., 1260.]],
        T::Select,
    );
    send(
        &w,
        A::AddMask {
            id: texture,
            replace: false,
        },
    );
    send(
        &w,
        A::LinkMask {
            id: texture,
            value: false,
        },
    );
    pump(650);
    let minimum = w
        .layer_panel
        .root
        .measure(gtk::Orientation::Horizontal, -1)
        .0;
    assert!(
        minimum <= layer_ui::LAYERS_MIN_WIDTH as i32,
        "Layer widgets require {minimum}px"
    );
    capture_reference(&w, &format!("{dir}/01-compact-dark.png"), 1.);
    send(
        &w,
        A::ShowMask {
            id: texture,
            value: true,
        },
    );
    pump(200);
    capture_reference(&w, &format!("{dir}/02-mask-overlay.png"), 1.);
    send(
        &w,
        A::ShowMask {
            id: texture,
            value: false,
        },
    );
    send(&w, A::ApplyMask { id: texture });
    pump(200);
    capture_reference(&w, &format!("{dir}/03-applied-mask.png"), 1.);
    click(&command(&w, CommandId::Undo));
    assert!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == texture)
            .unwrap()
            .has_mask
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(300);
    capture_reference(&w, &format!("{dir}/04-compact-light.png"), 1.);
    // Exercise the native row controls, including virtual-row identity across
    // selection changes (double clicks must not lose their GTK gesture).
    let row = |id| find_named(w.layer_panel.root.upcast_ref(), &format!("art-layer-{id}")).unwrap();
    let label = find_css(&row(1), "layer-name").unwrap();
    let controllers = row(1).observe_controllers();
    let rename = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .find(|g| g.button() == 1)
        .unwrap();
    let label_bounds = label.compute_bounds(&row(1)).unwrap();
    let (label_x, label_y) = (label_bounds.x() as f64 + 5., label_bounds.y() as f64 + 5.);
    rename.emit_by_name::<()>("released", &[&1i32, &label_x, &label_y]);
    pump(100);
    assert_eq!(label, find_css(&row(1), "layer-name").unwrap());
    rename.emit_by_name::<()>("released", &[&2i32, &label_x, &label_y]);
    pump(100);
    let entry: gtk::Entry = find_css(&row(1), "layer-name-entry")
        .unwrap()
        .downcast()
        .unwrap();
    assert!(entry.is_mapped());
    entry.set_text("Atmosphere wash");
    capture_reference(&w, &format!("{dir}/06-inline-rename.png"), 1.);
    entry.emit_activate();
    pump(100);
    assert_eq!(
        state(&w).layers.iter().find(|l| l.id == 1).unwrap().label,
        "Atmosphere wash"
    );
    send(
        &w,
        A::Select {
            id: texture,
            mask: true,
        },
    );
    send(
        &w,
        A::New {
            group: true,
            clipped: false,
        },
    );
    let outer = state(&w).layers.iter().find(|l| l.editing).unwrap().id;
    send(
        &w,
        A::Rename {
            id: outer,
            name: "Character".into(),
        },
    );
    send(
        &w,
        A::New {
            group: true,
            clipped: false,
        },
    );
    let inner = state(&w).layers.iter().find(|l| l.editing).unwrap().id;
    send(
        &w,
        A::Rename {
            id: inner,
            name: "Fabric details".into(),
        },
    );
    send(&w, A::Collapse { id: inner });
    let source_row = row(texture);
    let preview = crate::layers::drag_preview(&source_row, state(&w).palette.panel).unwrap();
    let snapshot = gtk::Snapshot::new();
    preview.snapshot(
        &snapshot,
        source_row.width() as f64,
        source_row.height() as f64,
    );
    let node = snapshot
        .to_node()
        .expect("drag preview contains the row image");
    w.window
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(format!("{dir}/10-drag-preview.png"))
        .unwrap();
    let controllers = source_row.observe_controllers();
    let drag = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::DragSource>())
        .unwrap();
    assert!(
        drag.emit_by_name::<Option<gdk::ContentProvider>>("prepare", &[&80f64, &18f64])
            .is_some()
    );
    let target = row(inner);
    let controllers = target.observe_controllers();
    let drop = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::DropTarget>())
        .unwrap();
    let y = target.height() as f64 / 2.;
    drop.emit_by_name::<gdk::DragAction>("enter", &[&80f64, &y]);
    pump(100);
    assert!(target.has_css_class("layer-drop-into"));
    capture_reference(&w, &format!("{dir}/07-group-drop-target.png"), 1.);
    assert!(drop.emit_by_name::<bool>(
        "drop",
        &[
            &glib::BoxedValue(format!("capy-layer:{texture}").to_value()),
            &80f64,
            &y
        ]
    ));
    pump(150);
    assert!(
        !state(&w)
            .layers
            .iter()
            .find(|l| l.id == inner)
            .unwrap()
            .collapsed
    );
    send(
        &w,
        A::Select {
            id: texture,
            mask: true,
        },
    );
    // Both fixed columns stay aligned at every nesting depth.
    let x = row(1)
        .first_child()
        .unwrap()
        .compute_bounds(&w.layer_panel.root)
        .unwrap()
        .x();
    for id in [texture, inner, outer] {
        assert_eq!(
            row(id)
                .first_child()
                .unwrap()
                .compute_bounds(&w.layer_panel.root)
                .unwrap()
                .x(),
            x
        );
    }
    let skin = state(&w)
        .layers
        .iter()
        .find(|l| l.label == "Skin base")
        .unwrap()
        .id;
    for id in [1, skin] {
        click(
            &row(id)
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap(),
        );
    }
    send(&w, A::ReferenceSelection);
    click(
        &row(texture)
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap(),
    );
    let current = state(&w);
    assert!(
        current
            .layers
            .iter()
            .any(|l| l.id == texture && l.editing && l.mask_selected && !l.selected)
    );
    assert_eq!(current.layers.iter().filter(|l| l.reference).count(), 3);
    send(
        &w,
        A::Lock {
            id: skin,
            value: true,
        },
    );
    edit_number(&w.layer_panel.opacity, "25*2");
    pump(100);
    assert_eq!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == texture)
            .unwrap()
            .opacity,
        0.5
    );
    assert_eq!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == skin)
            .unwrap()
            .opacity,
        1.
    );
    edit_number(&w.layer_panel.opacity, "100");
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(300);
    assert!(
        w.layer_panel
            .root
            .measure(gtk::Orientation::Horizontal, -1)
            .0
            <= layer_ui::LAYERS_MIN_WIDTH as i32
    );
    capture_reference(&w, &format!("{dir}/08-groups-selection-dark.png"), 1.);
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(300);
    capture_reference(&w, &format!("{dir}/09-groups-selection-light.png"), 1.);
    send(&w, A::Collapse { id: outer });
    assert!(!state(&w).layers.iter().any(|l| l.editing));
    assert_eq!(
        state(&w).layer_tools.editing_layer.as_ref().unwrap().id,
        texture
    );
    edit_number(&w.layer_panel.opacity, "50");
    pump(100);
    assert_eq!(
        state(&w)
            .layer_tools
            .editing_layer
            .as_ref()
            .unwrap()
            .opacity,
        0.5
    );
    edit_number(&w.layer_panel.opacity, "100");
    send(&w, A::Collapse { id: outer });
    // Menus are the same shared commands as direct controls. Exercise their
    // native activation, not just the underlying Rust enum.
    let more: gtk::Button = w
        .layer_panel
        .footer
        .last_child()
        .unwrap()
        .downcast()
        .unwrap();
    let popover: gtk::PopoverMenu = w.layer_panel.root.last_child().unwrap().downcast().unwrap();
    let open_menu = |id, mask, name: &str| {
        send(&w, A::Context { id, mask });
        click(&more);
        pump(120);
        capture_reference(&w, &format!("{dir}/{name}.png"), 1.);
        capture_popover(popover.upcast_ref(), &format!("{dir}/{name}-menu.png"));
    };
    open_menu(texture, true, "11-mask-context");
    let activate = |label| {
        let action = menu_action(&popover.menu_model().unwrap(), label).unwrap();
        popover.activate_action(&action, None).unwrap();
        popover.popdown();
        pump(150);
        assert!(!w.status.is_visible(), "{}", w.status.text());
    };
    activate("Copy mask");
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    open_menu(texture, false, "12-layer-context");
    activate("Rename layer…");
    let name: gtk::Entry = find_css(&row(texture), "layer-name-entry")
        .unwrap()
        .downcast()
        .unwrap();
    assert!(name.is_mapped());
    name.set_text("Fabric · copied silhouette");
    name.emit_activate();
    pump(150);
    // Clear an imported texture (no brush strokes) must invalidate its GPU
    // source and thumbnail. Undo restores the exact pixels and attached mask.
    let pixels = || {
        let button = find_css(&row(texture), "layer-thumbnail").unwrap();
        fn picture(w: &gtk::Widget) -> Option<gtk::Picture> {
            if let Ok(p) = w.clone().downcast() {
                return Some(p);
            }
            let mut child = w.first_child();
            while let Some(c) = child {
                child = c.next_sibling();
                if let Some(p) = picture(&c) {
                    return Some(p);
                }
            }
            None
        }
        let t: gdk::Texture = picture(&button)
            .unwrap()
            .paintable()
            .unwrap()
            .downcast()
            .unwrap();
        let mut bytes = vec![0; t.width() as usize * t.height() as usize * 4];
        t.download(&mut bytes, t.width() as usize * 4);
        bytes
    };
    pump(250);
    let before = pixels();
    send(&w, A::Clear { id: texture });
    pump(250);
    assert!(
        pixels() != before,
        "Clear must update imported-image GPU thumbnails"
    );
    capture_reference(&w, &format!("{dir}/13-cleared-texture.png"), 1.);
    click(&command(&w, CommandId::Undo));
    pump(250);
    assert!(pixels() == before, "Undo restores exact imported pixels");
    send(
        &w,
        A::New {
            group: false,
            clipped: false,
        },
    );
    let copied = state(&w).layer_tools.editing_layer.unwrap().id;
    send(&w, A::PasteMask { id: copied });
    send(
        &w,
        A::Select {
            id: copied,
            mask: false,
        },
    );
    w.dispatch(UiAction::SetColor {
        rgba: [0.9, 0.2, 0.5, 1.],
    });
    polygon(
        &w,
        &[[0., 0.], [2048., 0.], [2048., 1536.], [0., 1536.]],
        T::Select,
    );
    send(&w, A::FillSelection);
    send(&w, A::Deselect);
    send(
        &w,
        A::Rename {
            id: copied,
            name: "Copied mask · independent layer".into(),
        },
    );
    capture_reference(&w, &format!("{dir}/14-copied-mask.png"), 1.);
    send(&w, A::ToggleSelection { id: texture });
    click(&more);
    capture_popover(
        popover.upcast_ref(),
        &format!("{dir}/15-multi-layer-menu.png"),
    );
    activate("Group selected layers");
    let grouped = state(&w).layers.iter().find(|l| l.selected).unwrap().id;
    open_menu(grouped, false, "16-group-context");
    activate("Ungroup");
    assert!(!state(&w).layers.iter().any(|l| l.id == grouped));
    // The opacity track keeps its bounds for every digit count and text edit.
    let header = w.layer_panel.opacity.first_child().unwrap();
    let slider: gtk::Scale = header.first_child().unwrap().downcast().unwrap();
    let mut width = None;
    for value in [1., 9., 10., 99., 100.] {
        w.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: value / 100.,
        });
        pump(60);
        let now = slider.compute_bounds(&header).unwrap();
        if let Some(previous) = width {
            assert_eq!(now.width(), previous);
        }
        width = Some(now.width());
    }
    let value_stack: gtk::Stack = header.last_child().unwrap().downcast().unwrap();
    click(&value_stack.visible_child().unwrap().downcast().unwrap());
    pump(60);
    let entry: gtk::Entry = value_stack.visible_child().unwrap().downcast().unwrap();
    entry.set_text("50*2");
    pump(60);
    assert_eq!(
        slider.compute_bounds(&header).unwrap().width(),
        width.unwrap()
    );
    entry.emit_activate();
    pump(60);
    assert_eq!(
        slider.compute_bounds(&header).unwrap().width(),
        width.unwrap()
    );
    // Async preview arrival cannot change the thumbnail's geometry.
    let thumbnail = find_css(&row(texture), "layer-thumbnail").unwrap();
    let overlay: gtk::Overlay = thumbnail
        .clone()
        .downcast::<gtk::Button>()
        .unwrap()
        .child()
        .unwrap()
        .downcast()
        .unwrap();
    let image: gtk::Picture = overlay
        .child()
        .unwrap()
        .next_sibling()
        .unwrap()
        .downcast()
        .unwrap();
    let paintable = image.paintable();
    let before = thumbnail.compute_bounds(&row(texture)).unwrap();
    image.set_paintable(None::<&gdk::Paintable>);
    pump(30);
    let empty = thumbnail.compute_bounds(&row(texture)).unwrap();
    image.set_paintable(paintable.as_ref());
    pump(30);
    let loaded = thumbnail.compute_bounds(&row(texture)).unwrap();
    assert_eq!(
        (before.width(), before.height()),
        (empty.width(), empty.height())
    );
    assert_eq!(
        (before.width(), before.height()),
        (loaded.width(), loaded.height())
    );
    assert_eq!(loaded.width(), loaded.height());
    // Paper is selectable, exposes only meaningful controls, and is anchored.
    let paper = row(2);
    click(
        &find_css(&paper, "layer-thumbnail")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    assert_eq!(state(&w).layer_tools.editing_layer.unwrap().id, 2);
    assert!(
        state(&w)
            .layers
            .iter()
            .find(|l| l.id == 2)
            .unwrap()
            .selected
    );
    assert!(w.layer_panel.opacity.is_sensitive());
    assert!(!state(&w).layer_tools.controls.mask);
    capture_reference(&w, &format!("{dir}/17-paper-selected.png"), 1.);
    open_menu(2, false, "18-paper-context");
    popover.popdown();
    for _ in 0..110 {
        send(
            &w,
            A::New {
                group: false,
                clipped: false,
            },
        );
    }
    pump(300);
    assert!(state(&w).layers.len() > 100);
    capture_reference(&w, &format!("{dir}/05-many-layers.png"), 1.);
    w.window.close();
    pump(100);
}

fn command(w: &Workspace, id: CommandId) -> gtk::Button {
    if let Some((_, button)) = w.commands.borrow().iter().find(|(c, _)| *c == id) {
        return button.clone();
    }
    for panel in &state(w).workspace.layout.panels {
        if let Some(tile) = panel
            .tiles()
            .iter()
            .find(|t| t.control.action() == Some(UiAction::Invoke { command: id }))
            && let Some(button) =
                find_named(&w.panel_widget(panel.id), &format!("tile-{}", tile.id))
        {
            return button.downcast().unwrap();
        }
    }
    // Menu commands are native GActions, not ad-hoc GtkButtons.
    let action = w.menu_actions.lookup_action(&id.shortcut_id()).unwrap();
    let button = gtk::Button::new();
    button.set_sensitive(action.is_enabled());
    button.connect_clicked(move |_| action.activate(None));
    button
}
fn click(button: &gtk::Button) {
    assert!(button.is_sensitive());
    button.emit_clicked();
    pump(100);
}
fn edit_number(control: &crate::number_control::NumberControl, text: &str) {
    let display: gtk::Button = find_css(control.upcast_ref(), "number-value")
        .unwrap()
        .downcast()
        .unwrap();
    click(&display);
    let entry: gtk::Entry = find_css(control.upcast_ref(), "number-entry")
        .unwrap()
        .downcast()
        .unwrap();
    entry.set_text(text);
    entry.emit_activate();
}
struct WorkspaceDragTest {
    workspace: Rc<Workspace>,
    origin: [f32; 2],
}
impl WorkspaceDragTest {
    fn update(&self, delta: [f64; 2]) {
        self.workspace.workspace_drag_input(
            ContactPhase::Move,
            [
                self.origin[0] + delta[0] as f32,
                self.origin[1] + delta[1] as f32,
            ],
            None,
        );
    }
    fn end(&self) {
        let point = self
            .workspace
            .workspace_drag
            .borrow()
            .as_ref()
            .unwrap()
            .point;
        self.workspace
            .workspace_drag_input(ContactPhase::Up, point, None);
    }
}
fn begin_workspace_drag(
    w: &Rc<Workspace>,
    widget: &gtk::Widget,
    x: f32,
    y: f32,
) -> WorkspaceDragTest {
    let point = widget
        .compute_point(&w.surface, &gtk::graphene::Point::new(x, y))
        .unwrap();
    assert!(
        w.drag_target_at([point.x(), point.y()]).is_some(),
        "No drag target at {}, {} (picked {:?}, expected {:?})",
        point.x(),
        point.y(),
        w.surface
            .pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT),
        widget
    );
    let controllers = w.surface.observe_controllers();
    let _controller = (0..controllers.n_items())
        .filter_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerLegacy>()
        })
        .find(|g| g.name().as_deref() == Some("workspace-drag"))
        .unwrap();
    let origin = [point.x(), point.y()];
    w.workspace_drag_input(ContactPhase::Down, origin, None);
    WorkspaceDragTest {
        workspace: w.clone(),
        origin,
    }
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_toolbar_sizing() {
    let app = native_test_app("dev.layer.ToolbarSizingTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    let initial = state(&w).workspace;
    let placement = || {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&Panel::Toolbar))
            .unwrap()
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            viewport,
            target: DockTarget::Float {
                position: [600.0, 300.0],
            },
        });
        pump(120);
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTileStyle {
                    panel: Panel::Toolbar,
                    style,
                },
            });
            pump(120);
            let natural = placement();
            let corner = [
                natural.bounds.x + natural.bounds.width + 2.0,
                natural.bounds.y + natural.bounds.height + 2.0,
            ];
            for (phase, position) in [
                (ContactPhase::Down, corner),
                (ContactPhase::Up, [corner[0] + 150.0, corner[1] + 60.0]),
            ] {
                w.dispatch(UiAction::ResizeFloating {
                    group: natural.id,
                    edge: ResizeEdge::BottomRight,
                    phase,
                    position,
                    viewport,
                });
            }
            pump(100);
            let resized = state(&w).workspace;
            let g = placement();
            let grip = g.tiles.unwrap().grip.unwrap();
            // The blank strip beside the dots is part of the draggable/reset area.
            let point = [
                g.bounds.x + grip.x + 3.0,
                g.bounds.y + grip.y + grip.height * 0.5,
            ];
            assert!(matches!(
                w.drag_target_at(point),
                Some(DragTarget::Dock(DockItem::Panel {
                    panel: Panel::Toolbar
                }))
            ));
            let controllers = w.surface.observe_controllers();
            let double_click = (0..controllers.n_items())
                .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
                .find(|g| g.name().as_deref() == Some("panel-handle-double-click"))
                .unwrap();
            double_click
                .emit_by_name::<()>("pressed", &[&2i32, &(point[0] as f64), &(point[1] as f64)]);
            pump(250);
            assert_eq!(placement().bounds, natural.bounds);
            capture_reference(
                &w,
                &format!("{dir}/toolbar-grip-reset-{style:?}-{theme:?}.png"),
                1.0,
            );
            for mode in [
                FloatingToolbarLayout::Vertical,
                FloatingToolbarLayout::Horizontal,
                FloatingToolbarLayout::Compact,
            ] {
                let g = placement();
                let grip = g.tiles.unwrap().grip.unwrap();
                let point = [g.bounds.x + grip.x + 3.0, g.bounds.y + grip.y + 3.0];
                double_click.emit_by_name::<()>(
                    "pressed",
                    &[&2i32, &(point[0] as f64), &(point[1] as f64)],
                );
                pump(250);
                assert_eq!(state(&w).workspace.layout.floating[0].toolbar_layout, mode);
                let g = placement();
                let horizontal = mode == FloatingToolbarLayout::Horizontal;
                assert_eq!(
                    g.axis,
                    if horizontal {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    }
                );
                let strip = w.panel_widget(Panel::Toolbar);
                let handle = find_css(&strip, "panel-grip").unwrap();
                let actual = handle.compute_bounds(&strip).unwrap();
                if horizontal {
                    assert_eq!(actual.x() + actual.width(), strip.width() as f32);
                } else {
                    assert_eq!(actual.y() + actual.height(), strip.height() as f32);
                }
                capture_reference(
                    &w,
                    &format!("{dir}/toolbar-cycle-{mode:?}-{style:?}-{theme:?}.png"),
                    1.0,
                );
                let before_style = state(&w).workspace;
                w.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTileStyle {
                        panel: Panel::Toolbar,
                        style: if style == TileStyle::Large {
                            TileStyle::Small
                        } else {
                            TileStyle::Large
                        },
                    },
                });
                pump(80);
                assert_eq!(state(&w).workspace.layout.floating[0].toolbar_layout, mode);
                assert_eq!(placement().axis, g.axis);
                w.dispatch(UiAction::Invoke {
                    command: CommandId::UndoWorkspace,
                });
                assert_eq!(
                    serde_json::to_value(state(&w).workspace).unwrap(),
                    serde_json::to_value(before_style).unwrap()
                );
                pump(80);
            }
            assert_eq!(placement().bounds, natural.bounds);
            for _ in 0..3 {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::UndoWorkspace,
                });
            }
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(&resized).unwrap()
            );
            let next = if style == TileStyle::Small {
                TileStyle::Large
            } else {
                TileStyle::Small
            };
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTileStyle {
                    panel: Panel::Toolbar,
                    style: next,
                },
            });
            pump(100);
            assert_eq!(
                placement().bounds.width,
                if next == TileStyle::Large {
                    220.0
                } else {
                    112.0
                }
            );
            assert!(state(&w).workspace.layout.floating[0].height.is_none());
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(&resized).unwrap(),
                "style and refit share one undo entry"
            );
            w.dispatch(UiAction::DoubleClickPanelHandle {
                group: natural.id,
                viewport,
            });
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Layers,
                viewport,
                target: DockTarget::Tab {
                    group: natural.id,
                    index: None,
                },
            });
            pump(100);
            let b = placement().bounds;
            for (phase, position) in [
                (ContactPhase::Down, [b.x + b.width, b.y + b.height]),
                (
                    ContactPhase::Up,
                    [b.x + b.width + 160.0, b.y + b.height + 100.0],
                ),
            ] {
                w.dispatch(UiAction::ResizeFloating {
                    group: natural.id,
                    edge: ResizeEdge::BottomRight,
                    phase,
                    position,
                    viewport,
                });
            }
            let grouped = state(&w).workspace;
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetPanelVisible {
                    panel: Panel::Layers,
                    visible: false,
                },
            });
            pump(150);
            assert!(!placement().tabs_visible);
            assert_eq!(
                placement().bounds,
                natural.bounds,
                "a lone toolbar regains its default grid"
            );
            capture_reference(
                &w,
                &format!("{dir}/toolbar-group-collapse-{style:?}-{theme:?}.png"),
                1.0,
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(grouped).unwrap()
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetPanelVisible {
                    panel: Panel::Layers,
                    visible: true,
                },
            });
        }
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Toolbar,
                viewport,
                target: DockTarget::Edge { edge, outer: true },
            });
            pump(100);
            let g = placement();
            assert!(!g.floating);
            assert_eq!(
                if g.axis == Axis::Vertical {
                    g.bounds.width
                } else {
                    g.bounds.height
                },
                if g.axis == Axis::Vertical {
                    108.0
                } else {
                    72.0
                }
            );
            capture_reference(
                &w,
                &format!("{dir}/toolbar-docked-{edge:?}-{theme:?}.png"),
                1.0,
            );
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_zen_icons() {
    let app = native_test_app("dev.layer.ZenIconsTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let dir = "../../artifacts/ui/zen-icons";
    std::fs::create_dir_all(dir).unwrap();
    let zen = command(&w, CommandId::ZenMode);
    let image = zen.child().and_downcast::<gtk::Image>().unwrap();
    let bounds = zen.compute_bounds(&w.surface).unwrap();
    assert_eq!(image.pixel_size(), layer_ui::ZEN_ICON_SIZE as i32);
    assert_eq!(w.preferences.dialog.content_height(), 744);
    assert_eq!(
        image.icon_name().as_deref(),
        Some("layer-zen-looking-up-symbolic")
    );
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::OpenSettings {
            page: SettingsPage::Appearance,
        });
        pump(300);
        let row = find_named(w.preferences.dialog.upcast_ref(), "setting-zen-icon").unwrap();
        let grid = find_css(&row, "image-selector")
            .unwrap()
            .downcast::<gtk::Grid>()
            .unwrap();
        let scroll = grid
            .ancestor(gtk::ScrolledWindow::static_type())
            .unwrap()
            .downcast::<gtk::ScrolledWindow>()
            .unwrap();
        let adjustment = scroll.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
        pump(200);
        let tile_bounds = grid.compute_bounds(&row).unwrap();
        assert!(
            (tile_bounds.x() + tile_bounds.width() / 2.0 - row.width() as f32 / 2.0).abs() < 1.0
        );
        assert!(find_named(w.preferences.dialog.upcast_ref(), "close-settings").is_none());
        for (i, (symbol, label)) in layer_ui::ZenIcon::CHOICES.into_iter().enumerate() {
            let tile = grid
                .child_at(i as i32, 0)
                .unwrap()
                .downcast::<gtk::ToggleButton>()
                .unwrap();
            assert_eq!(tile.tooltip_text().as_deref(), Some(label));
            tile.emit_clicked();
            pump(100);
            assert!(tile.is_active());
            assert_eq!(state(&w).settings.zen_icon, symbol);
            assert_eq!(
                image.icon_name().as_deref(),
                Some(format!("layer-{}-symbolic", symbol.icon()).as_str())
            );
            assert_eq!(image.pixel_size(), layer_ui::ZEN_ICON_SIZE as i32);
            assert_eq!(zen.compute_bounds(&w.surface).unwrap(), bounds);
            for other in 0..4 {
                assert_eq!(
                    grid.child_at(other, 0)
                        .unwrap()
                        .downcast::<gtk::ToggleButton>()
                        .unwrap()
                        .is_active(),
                    other as usize == i
                );
            }
            capture_reference(&w, &format!("{dir}/gtk-selector-{name}-{i}.png"), 1.0);
        }
        // Reset uses the same action as the existing per-setting context menu.
        w.dispatch(UiAction::Preferences {
            action: PreferenceAction::Reset {
                id: PreferenceId::ZenIcon,
            },
        });
        pump(100);
        assert!(
            grid.child_at(0, 0)
                .unwrap()
                .downcast::<gtk::ToggleButton>()
                .unwrap()
                .is_active()
        );
        capture_reference(&w, &format!("{dir}/gtk-selector-{name}.png"), 1.0);
        w.dispatch(UiAction::CloseSettings);
        pump(250);
        // The always-visible button suppresses its active highlight until the
        // full UI is revealed, without changing the underlying toggle state.
        click(&zen);
        w.chrome_event(layer_ui::ChromeEvent::Motion {
            position: [600.0, 450.0],
        });
        pump(250);
        assert!(zen.has_css_class("selected-tool"));
        assert!(!zen.has_css_class("zen-hidden"));
        assert!(zen.has_css_class("zen-button-neutral"));
        capture_reference(&w, &format!("{dir}/gtk-button-only-{name}.png"), 1.0);
        w.chrome_event(layer_ui::ChromeEvent::Motion {
            position: [600.0, 1.0],
        });
        pump(250);
        assert!(zen.has_css_class("zen-button-neutral"));
        capture_reference(&w, &format!("{dir}/gtk-active-{name}.png"), 1.0);
        click(&zen);
        pump(250);
        assert!(!zen.has_css_class("selected-tool"));
        capture_reference(&w, &format!("{dir}/gtk-inactive-{name}.png"), 1.0);
    }
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "group tab presentation: requires private Wayland and GPU"]
fn native_group_tab_styles() {
    let app = native_test_app("art.capycanvas.GroupTabStylesTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(500);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let group = state(&w)
        .workspace
        .layout
        .panel_group(Panel::Brushes)
        .unwrap();
    for panel in [Panel::Sizes, Panel::Layers] {
        w.dispatch(UiAction::MovePanel {
            panel,
            viewport,
            target: DockTarget::Tab { group, index: None },
        });
    }
    let dir = "../../artifacts/ui/group-tab-styles";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for style in TabStyle::ALL {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTabStyle { group, style },
            });
            for active in [Panel::Brushes, Panel::Sizes, Panel::Layers] {
                w.dispatch(UiAction::SelectPanelTab {
                    group,
                    panel: active,
                });
                pump(150);
                let layout = state(&w).workspace.layout;
                for (panel, button) in &w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .tabs
                {
                    let content = button.child().unwrap();
                    let icon = content.first_child().and_downcast::<gtk::Image>().unwrap();
                    let label = content.last_child().and_downcast::<gtk::Label>().unwrap();
                    let expected = layout.tab_presentation(*panel);
                    assert_eq!(icon.is_visible(), expected.show_icon);
                    assert_eq!(label.is_visible(), expected.show_name);
                    if !expected.show_name {
                        let bounds = button.compute_bounds(&w.surface).unwrap();
                        assert_eq!(bounds.width(), bounds.height(), "icon-only tabs are square");
                    }
                    assert_eq!(label.text(), layout.panel(*panel).unwrap().title());
                    assert_eq!(
                        button.compute_bounds(&w.surface).unwrap().height(),
                        TAB_BAR_HEIGHT
                    );
                }
            }
            capture_reference(&w, &format!("{dir}/gtk-{theme:?}-{style:?}.png"), 1.0);
        }
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabStyle {
                group,
                style: TabStyle::Automatic,
            },
        });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            viewport,
            target: DockTarget::Float {
                position: [850.0, 200.0],
            },
        });
        pump(150);
        let visible_names = || {
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .iter()
                .filter(|(_, button)| button.child().unwrap().last_child().unwrap().is_visible())
                .count()
        };
        assert_eq!(visible_names(), 2);
        capture_reference(
            &w,
            &format!("{dir}/gtk-{theme:?}-Automatic-two-tabs.png"),
            1.0,
        );
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Layers,
            viewport,
            target: DockTarget::Tab { group, index: None },
        });
        pump(150);
        assert_eq!(visible_names(), 1);
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native settings typography/geometry reference: requires private Wayland and GPU"]
fn native_settings_typography() {
    // Measure an unmodified Adwaita row first, before loading application CSS.
    adw::init().unwrap();
    let reference = adw::Window::new();
    let css = gtk::CssProvider::new();
    css.load_from_string(&format!("window {{ font-size: {UI_TEXT_PT}pt; }}"));
    let display = gdk::Display::default().unwrap();
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let row = adw::ActionRow::builder()
        .title("Native title")
        .subtitle("Native description")
        .build();
    let list = gtk::ListBox::new();
    list.append(&row);
    reference.set_content(Some(&list));
    reference.present();
    pump(150);
    let font_px = |widget: &gtk::Widget| {
        widget.pango_context().font_description().unwrap().size() as f64 / gtk::pango::SCALE as f64
    };
    let title_px = font_px(&find_css(row.upcast_ref(), "title").unwrap());
    let subtitle_px = font_px(&find_css(row.upcast_ref(), "subtitle").unwrap());
    assert!(subtitle_px < title_px);
    eprintln!(
        "Unmodified Adwaita: title={title_px:.3}px, subtitle={subtitle_px:.3}px, ratio={:.4}",
        subtitle_px / title_px
    );
    reference.destroy();
    gtk::style_context_remove_provider_for_display(&display, &css);

    fn labels(widget: &gtk::Widget, root: &gtk::Widget, out: &mut Vec<serde_json::Value>) {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && widget.is_mapped()
        {
            let b = widget.compute_bounds(root).unwrap();
            out.push(serde_json::json!({"text":label.text().to_string(), "font_px":label.pango_context().font_description().unwrap().size() as f64 / gtk::pango::SCALE as f64,
                "bounds":[b.x(),b.y(),b.width(),b.height()]}));
        }
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            labels(&c, root, out);
        }
    }
    let app = native_test_app("art.capycanvas.SettingsTypographyTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    pump(600);
    let dir = "../../artifacts/ui/settings-audit";
    std::fs::create_dir_all(dir).unwrap();
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for page in SettingsPage::ALL {
            w.dispatch(UiAction::OpenSettings { page });
            pump(200);
            let content =
                find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap();
            let view = w
                .gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .preferences()
                .unwrap();
            let page = view.pages.iter().find(|p| p.id == page).unwrap();
            let mut rows = Vec::new();
            for row in page.groups.iter().flat_map(|g| &g.rows) {
                let widget = find_named(
                    w.preferences.dialog.upcast_ref(),
                    &format!("preference-{}", row.id.key()),
                )
                .or_else(|| {
                    find_named(
                        w.preferences.dialog.upcast_ref(),
                        &format!("setting-{}", row.id.key()),
                    )
                })
                .unwrap();
                let b = widget.compute_bounds(&content).unwrap();
                let mut text = Vec::new();
                labels(&widget, &content, &mut text);
                for (expected, size) in [(&row.title, title_px), (&row.description, subtitle_px)] {
                    if expected.is_empty() {
                        continue;
                    }
                    let label = text
                        .iter()
                        .find(|l| l["text"] == *expected)
                        .unwrap_or_else(|| panic!("Missing settings label: {expected}"));
                    assert!(
                        (label["font_px"].as_f64().unwrap() - size).abs() < 0.02,
                        "{expected}: {label}"
                    );
                }
                rows.push(serde_json::json!({"id":row.id,"bounds":[b.x(),b.y(),b.width(),b.height()],"labels":text}));
            }
            if page.id == SettingsPage::Shortcuts {
                for id in std::iter::once("shortcuts-search".into())
                    .chain(view.shortcuts.iter().map(|s| format!("shortcut-{}", s.id)))
                {
                    let widget = find_named(w.preferences.dialog.upcast_ref(), &id).unwrap();
                    let b = widget.compute_bounds(&content).unwrap();
                    let mut text = Vec::new();
                    labels(&widget, &content, &mut text);
                    rows.push(serde_json::json!({"id":id,"bounds":[b.x(),b.y(),b.width(),b.height()],"labels":text}));
                }
            }
            capture_reference(&w, &format!("{dir}/gtk-{}-{name}.png", page.id.key()), 1.0);
            std::fs::write(format!("{dir}/gtk-{}-{name}.json", page.id.key()), serde_json::to_vec_pretty(&serde_json::json!({
                "title_px":title_px,"subtitle_px":subtitle_px,"content_width":content.width(),"rows":rows})).unwrap()).unwrap();
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_zen_behaviors() {
    let app = native_test_app("art.capycanvas.ZenSectionsTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let dir = "../../artifacts/familiar-workspace";
    std::fs::create_dir_all(dir).unwrap();
    let zen = command(&w, CommandId::ZenMode);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let mut workspace = state(&w).workspace;
    workspace.zen_mode = false;
    workspace
        .layout
        .move_panel(
            viewport,
            Panel::Toolbar,
            DockTarget::Edge {
                edge: Edge::Left,
                outer: true,
            },
        )
        .unwrap();
    for edge in [Edge::Top, Edge::Bottom, Edge::Right] {
        let toolbar = workspace
            .layout
            .add_toolbar(
                None,
                &format!("{edge:?}"),
                &[
                    ToolbarControl::Command {
                        command: CommandId::Pen,
                    },
                    ToolbarControl::Color,
                    ToolbarControl::Divider,
                    ToolbarControl::Command {
                        command: CommandId::Eraser,
                    },
                    ToolbarControl::Divider,
                    ToolbarControl::Opacity,
                ],
            )
            .unwrap();
        workspace
            .layout
            .move_panel(viewport, toolbar, DockTarget::Edge { edge, outer: true })
            .unwrap();
    }
    workspace
        .layout
        .insert_tools(Panel::Toolbar, Some(2), &[ToolbarControl::Divider])
        .unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    w.dispatch(UiAction::RestoreSettings {
        settings: Settings::default(),
    });
    pump(200);
    let saved = state(&w).workspace.layout;
    for (theme, name) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        capture_reference(&w, &format!("{dir}/zen-normal-{name}.png"), 1.0);
        click(&zen);
        pump(300);
        assert!(state(&w).partial_zen());
        assert!(zen.can_target() && zen.has_css_class("zen-button-neutral"));
        for (slot, widget) in w.surface.imp().children.borrow().iter() {
            let visible = matches!(
                slot,
                Slot::Canvas
                    | Slot::ZenButton
                    | Slot::ZenToolbars
                    | Slot::Drawer
                    | Slot::DrawerConnection
            );
            assert_eq!(!widget.has_css_class("zen-hidden"), visible);
            assert_eq!(widget.can_target(), visible);
        }
        let model = saved.zen_toolbars(viewport);
        let root = find_named(w.surface.upcast_ref(), "zen-toolbars").unwrap();
        for section in &model.sections {
            for (id, _) in &section.tiles {
                let Some(expected) = section.tile_bounds(*id) else {
                    continue;
                };
                let tile: gtk::Button = find_named(&root, &format!("zen-tile-{id}"))
                    .unwrap()
                    .downcast()
                    .unwrap();
                let actual = tile.compute_bounds(&w.surface).unwrap();
                assert!((actual.x() - expected.x).abs() <= 1.0);
                assert!((actual.y() - expected.y).abs() <= 1.0);
                assert_eq!(actual.width(), section.style.size()[0]);
                assert_eq!(actual.height(), section.style.size()[1]);
                let center = [
                    expected.x + expected.width * 0.5,
                    expected.y + expected.height * 0.5,
                ];
                let picked = w
                    .surface
                    .pick(center[0] as f64, center[1] as f64, gtk::PickFlags::DEFAULT)
                    .unwrap();
                if tile.is_sensitive() {
                    assert!(
                        picked == tile.clone().upcast::<gtk::Widget>() || picked.is_ancestor(&tile),
                        "{edge:?} tile {id} picked {}",
                        picked.widget_name(),
                        edge = section.edge
                    );
                }
            }
        }
        assert!(
            !root.contains(600.0, 450.0),
            "section gaps must let drawing input through"
        );
        for position in [
            [6.0, 6.0],
            [600.0, 1.0],
            [1.0, 400.0],
            [viewport[0] - 1.0, 400.0],
            [600.0, viewport[1] - 1.0],
        ] {
            let reply = w.chrome_event(ChromeEvent::Motion { position });
            assert!(reply.chrome_hidden && reply.partial_zen);
            assert!(!w.reveal_chrome_at(position[0], position[1]));
        }
        capture_reference(&w, &format!("{dir}/zen-partial-{name}.png"), 1.0);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let section = model
                .sections
                .iter()
                .find(|s| {
                    s.edge == edge
                        && s.tiles.iter().any(|(id, _)| {
                            saved
                                .panel(s.panel)
                                .unwrap()
                                .tiles()
                                .iter()
                                .any(|t| t.id == *id && t.control == ToolbarControl::Color)
                        })
                })
                .unwrap();
            let id = section
                .tiles
                .iter()
                .find(|(id, _)| {
                    saved
                        .panel(section.panel)
                        .unwrap()
                        .tiles()
                        .iter()
                        .any(|t| t.id == *id && t.control == ToolbarControl::Color)
                })
                .unwrap()
                .0;
            let tile = find_named(&root, &format!("zen-tile-{id}"))
                .unwrap()
                .downcast()
                .unwrap();
            click(&tile);
            pump(250);
            assert!(state(&w).customization.drawer.is_some());
            let drawer = find_named(w.surface.upcast_ref(), "tool-drawer").unwrap();
            assert!(!drawer.has_css_class("zen-hidden") && drawer.can_target());
            let placement = w.drawer.placement().unwrap();
            assert_drawer_connected(&w);
            let anchor = section.tile_bounds(id).unwrap();
            assert_eq!(
                [placement.anchor.x, placement.anchor.y],
                [anchor.x, anchor.y]
            );
            let bar = tile.ancestor(TileStrip::static_type()).unwrap();
            assert!(bar.has_css_class("drawer-source"));
            let corners = placement.source_corners(section.bounds);
            for (expected, class) in corners
                .into_iter()
                .zip(["join-nw", "join-ne", "join-se", "join-sw"])
            {
                assert_eq!(bar.has_css_class(class), expected);
            }
            // Inspect the final composited GTK pixels, not just button CSS:
            // a rounded toolbar clip used to cut off the square tile corners.
            let texture = crate::snapshot(&w);
            let stride = texture.width() as usize * 4;
            let mut bytes = vec![0; stride * texture.height() as usize];
            texture.download(&mut bytes, stride);
            let offset = w
                .surface
                .compute_point(&w.window, &gtk::graphene::Point::new(0.0, 0.0))
                .unwrap();
            let pixel = |x: f32, y: f32| {
                let i = (y + offset.y()).floor() as usize * stride
                    + (x + offset.x()).floor() as usize * 4;
                &bytes[i..i + 3]
            };
            let c = placement.connection().unwrap().bounds;
            let expected = pixel(c.x + c.width * 0.5, c.y + c.height * 0.5);
            for (joined, [x, y]) in placement.source_corners(anchor).into_iter().zip([
                [anchor.x + 1.0, anchor.y + 1.0],
                [anchor.x + anchor.width - 2.0, anchor.y + 1.0],
                [
                    anchor.x + anchor.width - 2.0,
                    anchor.y + anchor.height - 2.0,
                ],
                [anchor.x + 1.0, anchor.y + anchor.height - 2.0],
            ]) {
                if joined {
                    assert!(
                        pixel(x, y)
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.abs_diff(*b) <= 3),
                        "{edge:?} joined tile corner: {:?} vs {expected:?}",
                        pixel(x, y)
                    );
                }
            }
            if section.tiles.len() > 1 {
                let first = section.tiles[0].1;
                let [x, y] = if matches!(edge, Edge::Left | Edge::Right) {
                    [
                        section.bounds.x + first.width * 0.5,
                        section.bounds.y + first.height + 1.0,
                    ]
                } else {
                    [
                        section.bounds.x + first.width + 1.0,
                        section.bounds.y + first.height * 0.5,
                    ]
                };
                assert!(
                    pixel(x, y).iter().zip(expected).all(|(a, b)| *a + 2 < *b),
                    "source toolbar should be subtly darker than its connected tile"
                );
            }
            capture_reference(&w, &format!("{dir}/zen-drawer-{edge:?}-{name}.png"), 1.0);
            let dismissed = w.chrome_event(ChromeEvent::Contact {
                position: [600.0, 450.0],
                canvas: true,
            });
            assert!(dismissed.handled && dismissed.chrome_hidden);
            pump(250);
            assert!(state(&w).customization.drawer.is_none());
            assert!(!bar.has_css_class("drawer-source"));
            for class in ["join-nw", "join-ne", "join-se", "join-sw"] {
                assert!(!bar.has_css_class(class));
            }
        }
        click(&zen);
        pump(250);
        assert!(!state(&w).workspace.zen_mode);
        assert_eq!(state(&w).workspace.layout, saved);
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Appearance,
    });
    pump(250);
    let row: adw::SwitchRow = find_named(w.preferences.dialog.upcast_ref(), "setting-total-zen")
        .unwrap()
        .downcast()
        .unwrap();
    row.set_active(true);
    pump(100);
    assert!(state(&w).settings.total_zen);
    assert!(find_named(w.preferences.dialog.upcast_ref(), "setting-zen-reveal-mode").is_none());
    capture_reference(&w, &format!("{dir}/zen-preferences.png"), 1.0);
    w.dispatch(UiAction::CloseSettings);
    pump(250);
    click(&zen);
    assert!(
        w.chrome_event(ChromeEvent::Motion {
            position: [600.0, 450.0]
        })
        .chrome_hidden
    );
    assert!(!zen.can_target());
    pump(250);
    capture_reference(&w, &format!("{dir}/zen-total.png"), 1.0);
    assert!(
        !w.chrome_event(ChromeEvent::Motion {
            position: [6.0, 6.0]
        })
        .chrome_hidden
    );
    for pressed in [true, false] {
        w.interact(UiInput::Key {
            key: "Tab".into(),
            pressed,
            repeat: false,
            modifiers: Modifiers::default(),
            editing: false,
            divider: None,
        });
    }
    assert!(!state(&w).workspace.zen_mode);
    let menu = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .context_menu(ContextTarget::ZenMode)
        .unwrap();
    assert_eq!(menu.sections[0][0].label, "Total zen");
    w.dispatch(menu.sections[0][0].action.clone().unwrap());
    assert!(!state(&w).settings.total_zen);
    w.dispatch(menu.sections[1][0].action.clone().unwrap());
    pump(300);
    assert_eq!(state(&w).preferences.reveal, Some(PreferenceId::ZenIcon));
    assert!(find_named(w.preferences.dialog.upcast_ref(), "setting-zen-icon").is_some());
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_zen_floating_targets() {
    let app = native_test_app("dev.layer.ZenFloatingTargetsTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::TotalZen,
            value: layer_ui::PreferenceValue::Bool(true),
        },
    });
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Toolbar,
        viewport,
        target: DockTarget::Float {
            position: [600.0, 400.0],
        },
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    assert!(
        w.chrome_event(ChromeEvent::Motion {
            position: [600.0, 450.0]
        })
        .chrome_hidden
    );
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    let begin = || {
        pump(100);
        let strip = w.panel_widget(Panel::Toolbar);
        let grip = find_css(&strip, "panel-grip").unwrap();
        let origin = grip
            .compute_point(&w.surface, &gtk::graphene::Point::new(3.0, 3.0))
            .unwrap();
        (begin_workspace_drag(&w, &grip, 3.0, 3.0), origin)
    };
    for (name, point) in [
        ("bottom", [viewport[0] * 0.5, viewport[1] - 1.0]),
        ("top", [viewport[0] * 0.5, HEADER_HEIGHT + 50.0]),
        ("hidden-sidebar", [100.0, 450.0]),
    ] {
        let (drag, origin) = begin();
        drag.update([
            (point[0] - origin.x()) as f64,
            (point[1] - origin.y()) as f64,
        ]);
        pump(150);
        assert!(w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
        assert!(
            w.drop_hint.borrow().is_none(),
            "{name} must not offer a hidden dock"
        );
        capture_reference(&w, &format!("{dir}/zen-no-snap-{name}.png"), 1.0);
        drag.end();
        assert_eq!(state(&w).workspace.layout.floating.len(), 1);
    }
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        viewport,
        target: DockTarget::Float {
            position: [950.0, 650.0],
        },
    });
    let mut workspace = state(&w).workspace;
    let target = workspace.layout.panel_group(Panel::Sizes).unwrap();
    let f = workspace
        .layout
        .floating
        .iter_mut()
        .find(|f| f.root.id() == target)
        .unwrap();
    f.position = [850.0, viewport[1] - 160.0];
    f.width = 200.0;
    f.height = Some(100.0);
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    let (drag, origin) = begin();
    let point = [950.0, viewport[1] - 10.0];
    drag.update([
        (point[0] - origin.x()) as f64,
        (point[1] - origin.y()) as f64,
    ]);
    pump(150);
    assert!(w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
    assert_eq!(
        w.drop_hint.borrow().as_ref().unwrap().target,
        DockTarget::Tab {
            group: target,
            index: None
        }
    );
    capture_reference(&w, &format!("{dir}/zen-floating-only-merge.png"), 1.0);
    drag.end();
    assert_eq!(
        state(&w).workspace.layout.panel_group(Panel::Toolbar),
        Some(target)
    );
    assert_eq!(state(&w).workspace.layout.floating.len(), 1);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_same_slot_drop() {
    let app = native_test_app("art.capycanvas.SameSlotDrop");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    for tearoff in [false, true] {
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .root
            .clone();
        let before = root.compute_bounds(&w.surface).unwrap();
        let mut baseline = state(&w).workspace;
        baseline.layout.measurements.clear();
        let grip = find_css(root.upcast_ref(), "panel-grip").unwrap();
        let origin = grip
            .compute_point(&w.surface, &gtk::graphene::Point::new(3.0, 3.0))
            .unwrap();
        let drag = begin_workspace_drag(&w, &grip, 3.0, 3.0);
        if tearoff {
            drag.update([(600.0 - origin.x()) as f64, (400.0 - origin.y()) as f64]);
            pump(120);
            assert_eq!(state(&w).workspace.layout.floating.len(), 1);
        }
        let neighbor = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == 6)
            .unwrap()
            .bounds;
        drag.update([
            (neighbor.x + neighbor.width * 0.5 - origin.x()) as f64,
            (neighbor.y + TAB_BAR_HEIGHT + 3.0 - origin.y()) as f64,
        ]);
        pump(80);
        drag.end();
        pump(350);
        let mut after = state(&w).workspace;
        after.layout.measurements.clear();
        assert_eq!(after, baseline, "tearoff={tearoff}");
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .root
            .clone();
        let after = root.compute_bounds(&w.surface).unwrap();
        assert_eq!(
            [after.x(), after.y(), after.width(), after.height()],
            [before.x(), before.y(), before.width(), before.height()]
        );
        assert!(!w.status.is_visible(), "{}", w.status.text());
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "requires a private Wayland display and GPU"]
fn native_floating_gestures() {
    let app = native_test_app("dev.layer.FloatingGesturesTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::TotalZen,
            value: layer_ui::PreferenceValue::Bool(true),
        },
    });
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    w.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        viewport,
        target: DockTarget::Float {
            position: [640.0, 250.0],
        },
    });
    // This regression exercises an explicitly visible floating title bar.
    w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetTabHidden {
            panel: Panel::Sizes,
            hidden: false,
        },
    });
    pump(150);
    let group = state(&w)
        .workspace
        .layout
        .panel_group(Panel::Sizes)
        .unwrap();
    let baseline = state(&w).workspace;
    let restore = || {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: baseline.clone(),
        });
        pump(120);
    };
    let bounds = || {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap()
            .bounds
    };
    let root = || {
        w.groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .root
            .clone()
    };
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    let initial = bounds();
    let handles = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.id == group)
        .unwrap()
        .resize_handles;
    for handle in handles {
        restore();
        let b = handle.bounds;
        let native = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{group}-{:?}", handle.edge),
        )
        .unwrap();
        let point = [b.x + b.width / 2.0, b.y + b.height / 2.0];
        assert!(
            matches!(w.drag_target_at(point), Some(DragTarget::Resize(id, edge)) if id == group && edge == handle.edge)
        );
        assert!(!initial.contains(point[0], point[1]));
        let drag = begin_workspace_drag(&w, &native, b.width / 2.0, b.height / 2.0);
        drag.update([18.0f64, 16.0f64]);
        pump(40);
        assert_ne!(bounds(), initial);
        drag.end();
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        // Undo restores durable state; native text measurements may still be
        // settling after the resize and are intentionally not history entries.
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    // Releasing into each screen edge rebuilds the docking widgets while the
    // workspace gesture remains alive. This caught the RefCell teardown panic.
    for (edge, target) in [
        (Edge::Left, [1.0, viewport[1] * 0.5]),
        (Edge::Right, [viewport[0] - 1.0, viewport[1] * 0.5]),
    ] {
        restore();
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .tabs[0]
            .1
            .clone();
        let origin = tab
            .compute_point(&w.surface, &gtk::graphene::Point::new(12.0, 12.0))
            .unwrap();
        let drag = begin_workspace_drag(&w, tab.upcast_ref(), 12.0, 12.0);
        let delta = [
            (target[0] - origin.x()) as f64,
            (target[1] - origin.y()) as f64,
        ];
        drag.update([delta[0], delta[1]]);
        pump(60);
        let hint = w
            .drop_hint
            .borrow()
            .clone()
            .expect("screen edge has a snap line");
        assert_eq!(hint.target, DockTarget::Edge { edge, outer: true });
        if edge == Edge::Top {
            assert_eq!(hint.bounds.y, HEADER_HEIGHT);
        }
        capture_reference(&w, &format!("{dir}/snap-{edge:?}.png"), 1.0);
        drag.end();
        pump(100);
        assert!(state(&w).workspace.layout.floating.is_empty());
        assert_eq!(
            state(&w).workspace.layout.panel_group(Panel::Sizes),
            Some(group)
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    restore();
    // Create space in the title bar and verify inside-top is move, not resize.
    let corner = [
        initial.x + initial.width + 2.0,
        initial.y + initial.height + 2.0,
    ];
    for (phase, position) in [
        (ContactPhase::Down, corner),
        (ContactPhase::Up, [corner[0] + 160.0, corner[1] + 80.0]),
    ] {
        w.dispatch(UiAction::ResizeFloating {
            group,
            edge: ResizeEdge::BottomRight,
            phase,
            position,
            viewport,
        });
    }
    pump(80);
    let before = bounds();
    let header = find_css(root().upcast_ref(), "dock-tabs").unwrap();
    let point = header
        .compute_point(
            &w.surface,
            &gtk::graphene::Point::new(header.width() as f32 - 28.0, 1.0),
        )
        .unwrap();
    assert!(
        matches!(w.drag_target_at([point.x(), point.y()]), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
    );
    let controllers = w.surface.observe_controllers();
    let click = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .find(|g| g.name().as_deref() == Some("panel-handle-double-click"))
        .unwrap();
    // Tabs are excluded from double-click reset.
    click.emit_by_name::<()>(
        "pressed",
        &[
            &2i32,
            &((before.x + 15.0) as f64),
            &((before.y + 15.0) as f64),
        ],
    );
    assert_eq!(bounds(), before);
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(true);
    click.emit_by_name::<()>(
        "pressed",
        &[&2i32, &(point.x() as f64), &(point.y() as f64)],
    );
    assert_eq!(
        [bounds().width, bounds().height],
        [initial.width, initial.height]
    );
    pump(50);
    let middle = root().compute_bounds(&w.surface).unwrap();
    assert!(
        middle.width() > initial.width && middle.width() < before.width,
        "reset interpolates: {} < {} < {}",
        initial.width,
        middle.width(),
        before.width
    );
    capture_reference(&w, &format!("{dir}/reset-size-mid-animation.png"), 1.0);
    pump(220);
    assert_eq!(root().width() as f32, initial.width);
    capture_reference(&w, &format!("{dir}/reset-size-complete.png"), 1.0);
    w.dispatch(UiAction::Invoke {
        command: CommandId::UndoWorkspace,
    });
    assert_eq!(bounds(), before);
    restore();
    // At natural size, empty title space toggles the lone panel's header.
    // The entire footer strip toggles it back, including outside the dots.
    click.emit_by_name::<()>(
        "pressed",
        &[
            &2i32,
            &((initial.x + initial.width - 28.0) as f64),
            &((initial.y + 10.0) as f64),
        ],
    );
    pump(250);
    assert!(
        state(&w)
            .workspace
            .layout
            .panel(Panel::Sizes)
            .unwrap()
            .hide_tab
    );
    let g = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.id == group)
        .unwrap();
    let footer = g.footer_grip.unwrap();
    let point = [g.bounds.x + footer.x + 3.0, g.bounds.y + footer.y + 10.0];
    assert!(
        matches!(w.drag_target_at(point), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
    );
    capture_reference(&w, &format!("{dir}/panel-cycle-tab-hidden.png"), 1.0);
    click.emit_by_name::<()>("pressed", &[&2i32, &(point[0] as f64), &(point[1] as f64)]);
    pump(250);
    assert!(
        !state(&w)
            .workspace
            .layout
            .panel(Panel::Sizes)
            .unwrap()
            .hide_tab
    );
    assert_eq!(bounds(), initial);
    capture_reference(&w, &format!("{dir}/panel-cycle-tab-shown.png"), 1.0);
    restore();
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    let center = [650.0, 440.0];
    assert!(
        w.chrome_event(ChromeEvent::Motion { position: center })
            .chrome_hidden
    );
    let tab = w
        .groups
        .borrow()
        .iter()
        .find(|g| g.id == group)
        .unwrap()
        .tabs[0]
        .1
        .clone();
    let origin = tab
        .compute_point(&w.surface, &gtk::graphene::Point::new(12.0, 12.0))
        .unwrap();
    let drag = begin_workspace_drag(&w, tab.upcast_ref(), 12.0, 12.0);
    let move_to = |position: [f32; 2]| {
        drag.update([
            ((position[0] - origin.x()) as f64),
            ((position[1] - origin.y()) as f64),
        ]);
        pump(50);
    };
    move_to(center);
    assert!(w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
    capture_reference(&w, &format!("{dir}/zen-floating-drag-hidden.png"), 1.0);
    move_to([40.0, 450.0]);
    assert!(!w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
    move_to(center);
    assert!(!w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
    capture_reference(&w, &format!("{dir}/zen-floating-drag-revealed.png"), 1.0);
    drag.end();
    assert!(w.chrome_event(ChromeEvent::Refresh).chrome_hidden);
    pump(180);
    capture_reference(&w, &format!("{dir}/zen-floating-drop-hidden.png"), 1.0);
    // The wider blue line targets the entire original stacked sidebar.
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(120);
    let left = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.active == Panel::Brushes)
        .unwrap();
    let point = [
        left.bounds.x + left.bounds.width + 60.0,
        left.bounds.y + left.bounds.height * 0.5,
    ];
    let strip = w.panel_widget(Panel::Toolbar);
    let grip = find_css(&strip, "panel-grip").unwrap();
    let origin = grip
        .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
        .unwrap();
    let drag = begin_workspace_drag(&w, &grip, 10.0, 10.0);
    let delta = [
        (point[0] - origin.x()) as f64,
        (point[1] - origin.y()) as f64,
    ];
    drag.update([delta[0], delta[1]]);
    pump(80);
    assert!(matches!(
        w.drop_hint.borrow().as_ref().unwrap().target,
        DockTarget::BesideBand { .. }
    ));
    capture_reference(&w, &format!("{dir}/whole-sidebar-snap.png"), 1.0);
    drag.end();
    pump(100);
    assert!(state(&w).workspace.layout.floating.is_empty());
    w.window.close();
    pump(100);
}
fn white_pixels(w: &Workspace) -> usize {
    // Inspect the same whole-window scene as the review PNG, not a fresh
    // standalone canvas snapshot that could hide host invalidation errors.
    let texture = crate::snapshot(w);
    canvas_white(w, &texture)
}
fn canvas_white(w: &Workspace, texture: &gdk::Texture) -> usize {
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    let bounds = w.area.compute_bounds(&w.window).unwrap();
    let stride = texture.width() as usize * 4;
    (bounds.y() as usize..(bounds.y() + bounds.height()) as usize)
        .map(|y| {
            let row = &bytes[y * stride + bounds.x() as usize * 4
                ..y * stride + (bounds.x() + bounds.width()) as usize * 4];
            row.chunks_exact(4)
                .filter(|p| p[0] > 245 && p[1] > 245 && p[2] > 245 && p[3] > 245)
                .count()
        })
        .sum()
}

fn capture_reference(w: &Workspace, path: &str, scale: f32) {
    // Include AdwDialog's overlay host, not only ApplicationWindow::child().
    // This measures GTK widgets, not compositor delivery or OS window shadows.
    crate::with_canvas_snapshot(w, || {
        crate::snapshot_window(&w.window, scale)
            .save_to_png(path)
            .unwrap();
    });
}

fn menu_action(model: &gtk::gio::MenuModel, label: &str) -> Option<String> {
    for i in 0..model.n_items() {
        if model
            .item_attribute_value(i, "label", None)
            .and_then(|v| v.get::<String>())
            .as_deref()
            == Some(label)
            && let Some(action) = model
                .item_attribute_value(i, "action", None)
                .and_then(|v| v.get::<String>())
        {
            return Some(action);
        }
        for link in ["section", "submenu"] {
            if let Some(child) = model.item_link(i, link)
                && let Some(action) = menu_action(&child, label)
            {
                return Some(action);
            }
        }
    }
    None
}

fn capture_popover(popover: &gtk::Popover, path: &str) {
    popover.present();
    pump(100);
    // An occluded Wayland popup can still be awaiting configure. Complete its
    // real native allocation for widget inspection, not a presentation timing test.
    let width = popover
        .width()
        .max(popover.measure(gtk::Orientation::Horizontal, -1).1);
    let height = popover
        .height()
        .max(popover.measure(gtk::Orientation::Vertical, width).1);
    popover.allocate(width, height, -1, None);
    let snapshot = gtk::Snapshot::new();
    let mut child = popover.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        popover.snapshot_child(&widget, &snapshot);
    }
    popover
        .renderer()
        .unwrap()
        .render_texture(snapshot.to_node().unwrap_or_else(|| panic!(
            "Empty popup capture {path}: visible={}, mapped={}, size={}x{}, opacity={}, child={:?}",
            popover.is_visible(), popover.is_mapped(), popover.width(), popover.height(), popover.opacity(),
            popover.first_child().map(|c| (c.is_visible(), c.is_mapped(), c.width(), c.height()))
        )), None)
        .save_to_png(path)
        .unwrap();
}

#[test]
#[ignore = "workspace management: requires a private Wayland/Vulkan display"]
fn native_workspace_management() {
    fn submenu(root: &gtk::Widget, label: &str) -> Option<gtk::Widget> {
        if root.type_().name() == "GtkModelButton" && root.property::<String>("text") == label {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(found) = submenu(&widget, label) {
                return Some(found);
            }
        }
        None
    }
    let app = native_test_app("dev.layer.WorkspaceManagementTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    pump(600);
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let send = |action| {
        w.dispatch(UiAction::Customize { action });
        pump(80);
    };
    let menu = w
        .popovers
        .borrow()
        .iter()
        .filter_map(|p| p.upgrade())
        .find(|p| p.has_css_class("panel-context-menu") && p.is::<gtk::PopoverMenu>())
        .unwrap()
        .downcast::<gtk::PopoverMenu>()
        .unwrap();
    menu.set_autohide(false);
    menu.set_pointing_to(Some(&gdk::Rectangle::new(350, 170, 1, 1)));
    let open_context = |target| {
        let model = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .context_menu(target)
            .unwrap();
        w.populate_workspace_menu(&menu, model);
        menu.popup();
        pump(100);
    };
    let activate = |popup: &gtk::PopoverMenu, label| {
        let action = menu_action(&popup.menu_model().unwrap(), label)
            .unwrap_or_else(|| panic!("Missing menu item {label}"));
        popup.activate_action(&action, None).unwrap();
        pump(150);
    };
    let workspace_menu = find_named(w.window.upcast_ref(), "workspace-menu")
        .unwrap()
        .downcast::<gtk::PopoverMenu>()
        .unwrap();
    workspace_menu.set_autohide(false);
    let snapshot = |name: &str| {
        pump(120);
        capture_reference(&w, &format!("{dir}/{name}.png"), 1.0);
    };
    let prompt = || {
        find_named(w.window.upcast_ref(), "toolbar-dialog")
            .unwrap()
            .downcast::<adw::AlertDialog>()
            .unwrap()
    };
    let confirm_prompt = || {
        let label = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .toolbar_prompt()
            .unwrap()
            .confirm_label;
        click(&find_button(prompt().upcast_ref(), label).unwrap());
        pump(180);
        assert!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .toolbar_prompt()
                .is_none()
        );
        assert!(find_named(w.window.upcast_ref(), "toolbar-dialog").is_none());
    };
    let group = |panel| state(&w).workspace.layout.panel_group(panel).unwrap();
    let placement = |panel| {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&panel))
            .unwrap()
    };

    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(180);
        workspace_menu.popup();
        pump(120);
        capture_popover(
            workspace_menu.upcast_ref(),
            &format!("{dir}/workspace-menu-{theme:?}.png"),
        );
        activate(&workspace_menu, "Brushes panel");
        assert!(
            state(&w)
                .workspace
                .layout
                .panel_group(Panel::Brushes)
                .is_none()
        );
        workspace_menu.popup();
        activate(&workspace_menu, "Brushes panel");
        assert!(
            state(&w)
                .workspace
                .layout
                .panel_group(Panel::Brushes)
                .is_some()
        );
        workspace_menu.popup();
        activate(&workspace_menu, "New Toolbar…");
        assert!(find_named(w.window.upcast_ref(), "tool-picker").is_some());
        send(CustomizationAction::CancelTools);

        open_context(ContextTarget::Ribbon {
            panel: Panel::Toolbar,
        });
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/toolbar-menu-{theme:?}.png"),
        );
        activate(&menu, "Duplicate Tools toolbar…");
        let name = find_named(w.window.upcast_ref(), "edit-toolbar-name")
            .unwrap()
            .downcast::<adw::EntryRow>()
            .unwrap();
        assert_eq!(name.text(), "Tools Copy");
        name.set_text("Brushes");
        assert!(!prompt().is_response_enabled("confirm"));
        name.set_text("Painting Tools");
        assert!(prompt().is_response_enabled("confirm"));
        snapshot(&format!("duplicate-{theme:?}"));
        confirm_prompt();
        let panel = state(&w)
            .workspace
            .layout
            .panels
            .iter()
            .find(|p| p.title() == "Painting Tools")
            .unwrap()
            .id;
        open_context(ContextTarget::Ribbon { panel });
        activate(&menu, "Rename Painting Tools toolbar…");
        name.set_text("Painting");
        snapshot(&format!("rename-{theme:?}"));
        confirm_prompt();
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().title(),
            "Painting"
        );

        // Pull away from the source: the live float exists before release,
        // and free canvas has no target rectangle.
        let area = w.resolved().work_area;
        let point = [area.x + area.width * 0.5, area.y + area.height * 0.4];
        let item = DockItem::Panel { panel };
        let tab = find_css(&w.panel_widget(panel), "panel-grip").unwrap();
        let origin = tab
            .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
            .unwrap();
        snapshot(&format!("before-tear-off-{theme:?}"));
        let drag = begin_workspace_drag(&w, &tab, 10.0, 10.0);
        let delta = [
            (point[0] - origin.x()) as f64,
            (point[1] - origin.y()) as f64,
        ];
        drag.update([delta[0], delta[1]]);
        pump(100);
        assert!(placement(panel).floating);
        assert!(w.drop_at(point[0], point[1], item).is_none());
        assert!(w.drop_hint.borrow().is_none());
        snapshot(&format!("tear-off-live-{theme:?}"));
        drag.end();
        pump(180);
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            open_context(ContextTarget::Ribbon { panel });
            activate(&menu, style.label());
            // Recreate using each style's default floating dimensions.
            w.dispatch(UiAction::MovePanel {
                panel,
                viewport,
                target: DockTarget::Edge {
                    edge: Edge::Top,
                    outer: false,
                },
            });
            w.dispatch(UiAction::MovePanel {
                panel,
                viewport,
                target: DockTarget::Float { position: point },
            });
            pump(180);
            let p = placement(panel);
            let columns = if style == TileStyle::Labeled {
                2.0
            } else {
                3.0
            };
            assert_eq!(p.bounds.width, columns * (style.size()[0] + 2.0) - 2.0);
            let strip = w.panel_widget(panel);
            let tile = strip
                .first_child()
                .unwrap()
                .next_sibling()
                .unwrap_or_else(|| strip.first_child().unwrap());
            assert_eq!(
                [tile.width(), tile.height()],
                style.size().map(|v| v as i32)
            );
            snapshot(&format!("floating-{style:?}-{theme:?}"));
        }
        let p = placement(panel);
        let strip = w.panel_widget(panel);
        let grip = find_css(&strip, "panel-grip").unwrap();
        assert_eq!(grip.width(), strip.width());
        let drag = begin_workspace_drag(&w, &grip, 3.0, 10.0);
        drag.update([45.0f64, 30.0f64]);
        pump(100);
        assert_eq!(placement(panel).bounds.x, p.bounds.x + 45.0);
        assert_eq!(placement(panel).bounds.y, p.bounds.y + 30.0);
        snapshot(&format!("live-toolbar-move-{theme:?}"));
        drag.end();
        pump(100);
        workspace_menu.popup();
        activate(&workspace_menu, "Undo Workspace Change");
        assert_eq!(placement(panel).bounds, p.bounds);
        workspace_menu.popup();
        activate(&workspace_menu, "Redo Workspace Change");
        assert_eq!(placement(panel).bounds.x, p.bounds.x + 45.0);

        let floated = group(panel);
        // Narrow the float first, so adding a tab must actually grow it.
        let before = placement(panel).bounds;
        let corner = [before.x + before.width, before.y + before.height];
        for (phase, position) in [
            (ContactPhase::Down, corner),
            (ContactPhase::Up, [before.x + 112.0, corner[1]]),
        ] {
            w.dispatch(UiAction::ResizeFloating {
                group: floated,
                edge: ResizeEdge::BottomRight,
                phase,
                position,
                viewport,
            });
        }
        pump(100);
        let narrow_width = placement(panel).bounds.width;
        open_context(ContextTarget::Group { group: floated });
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/group-menu-{theme:?}.png"),
        );
        for label in ["Add built-in panel", "Add Toolbar"] {
            let trigger = submenu(menu.upcast_ref(), label).unwrap();
            assert!(trigger.activate());
            pump(100);
            capture_popover(
                menu.upcast_ref(),
                &format!("{dir}/submenu-{label}-{theme:?}.png"),
            );
            menu.set_visible_submenu(Some("main"));
            pump(50);
        }
        activate(&menu, "Tools toolbar");
        assert_eq!(group(Panel::Toolbar), floated);
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_ne!(group(Panel::Toolbar), floated);
        open_context(ContextTarget::Group { group: floated });
        activate(&menu, "Layers panel");
        assert_eq!(group(Panel::Layers), floated);
        // Addition grows the native allocation to fit measured tab labels.
        pump(150);
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .root
            .clone();
        let labels: i32 = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .tabs
            .iter()
            .map(|(_, t)| t.measure(gtk::Orientation::Horizontal, -1).1)
            .sum();
        assert!(root.width() >= labels + 20);
        assert!(root.width() as f32 > narrow_width);
        snapshot(&format!("floating-tab-group-{theme:?}"));
        // Manual sizing takes over again and leaves genuine empty header space.
        let resize = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{floated}-BottomRight"),
        )
        .unwrap();
        let resize_drag = begin_workspace_drag(&w, &resize, 3.0, 3.0);
        resize_drag.update([80.0f64, 0.0f64]);
        resize_drag.end();
        pump(100);
        let header = find_css(root.upcast_ref(), "dock-tabs").unwrap();
        let original = placement(panel).bounds;
        let drag = begin_workspace_drag(&w, &header, header.width() as f32 - 28.0, 12.0);
        drag.update([-25.0f64, 20.0f64]);
        pump(100);
        assert_eq!(placement(panel).bounds.x, original.x - 25.0);
        assert_eq!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == floated)
                .unwrap()
                .root,
            root
        );
        drag.end();
        let resize = find_named(
            w.surface.upcast_ref(),
            &format!("floating-resize-{floated}-BottomRight"),
        )
        .unwrap();
        let before = placement(panel).bounds;
        let drag = begin_workspace_drag(&w, &resize, 3.0, 3.0);
        drag.update([30.0f64, 50.0f64]);
        pump(100);
        assert_eq!(placement(panel).bounds.width, before.width + 30.0);
        drag.end();
        let tall = placement(panel).bounds.height;
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == floated)
            .unwrap()
            .tabs
            .iter()
            .find(|(p, _)| *p == panel)
            .unwrap()
            .1
            .clone();
        click(&tab);
        assert_ne!(placement(panel).bounds.height, tall);
        open_context(ContextTarget::Panel { panel });
        assert!(menu_action(&menu.menu_model().unwrap(), "Rename Painting toolbar…").is_none());
        activate(&menu, "Configure Painting toolbar…");
        snapshot(&format!("toolbar-configuration-{theme:?}"));
        send(CustomizationAction::CloseExpanded);
        send(CustomizationAction::SetTabStyle {
            group: floated,
            style: TabStyle::Icon,
        });
        let expected = state(&w).workspace.layout.panel(panel).unwrap().icon();
        assert_eq!(
            tab.child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some(format!("layer-{expected}-symbolic").as_str())
        );
        send(CustomizationAction::SetTabStyle {
            group: floated,
            style: TabStyle::Name,
        });
        w.dispatch(UiAction::Preferences {
            action: PreferenceAction::Edit {
                id: PreferenceId::TotalZen,
                value: layer_ui::PreferenceValue::Bool(true),
            },
        });
        w.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        });
        pump(200);
        assert!(!root.has_css_class("zen-hidden"));
        assert!(
            w.groups
                .borrow()
                .iter()
                .filter(|g| !g.floating)
                .all(|g| g.root.has_css_class("zen-hidden"))
        );
        snapshot(&format!("floating-zen-{theme:?}"));
        w.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        });
        open_context(ContextTarget::Ribbon { panel });
        activate(&menu, "Hide Painting toolbar");
        assert!(state(&w).workspace.layout.panel_group(panel).is_none());
        workspace_menu.popup();
        activate(&workspace_menu, "Painting toolbar");
        workspace_menu.popup();
        activate(&workspace_menu, "Manage Toolbars…");
        let list = find_named(w.window.upcast_ref(), "managed-toolbars")
            .unwrap()
            .downcast::<gtk::ListBox>()
            .unwrap();
        let index = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .toolbar_manager()
            .unwrap()
            .toolbars
            .iter()
            .position(|p| p.panel == panel)
            .unwrap();
        list.select_row(list.row_at_index(index as i32).as_ref());
        click(
            &find_named(w.window.upcast_ref(), "delete-managed-toolbar")
                .unwrap()
                .downcast::<gtk::Button>()
                .unwrap(),
        );
        assert!(prompt().body().contains("Undo Workspace Change"));
        snapshot(&format!("delete-{theme:?}"));
        confirm_prompt();
        assert!(state(&w).workspace.layout.panel(panel).is_err());
        send(CustomizationAction::CloseToolbarManager);
        workspace_menu.popup();
        activate(&workspace_menu, "Undo Workspace Change");
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().title(),
            "Painting"
        );
    }
    menu.popdown();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "workspace customization: requires a Wayland/Vulkan display"]
fn native_panel_customization() {
    let app = native_test_app("dev.layer.CustomizationTest");
    // This test checks native controls and static snapshots. Test expansion
    // animation separately; occluded popups do not receive animation frames.
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    pump(600);
    let dir = "../../artifacts/ui/customization";
    std::fs::create_dir_all(dir).unwrap();
    let send = |action| w.dispatch(UiAction::Customize { action });
    // Signal-driven holds have no Wayland input serial for a popup grab. Keep
    // this control/snapshot test independent of external desktop focus changes;
    // production retains native autohide and actions still dismiss the menu.
    for popover in w.popovers.borrow().iter().filter_map(|p| p.upgrade()) {
        if popover.has_css_class("panel-context-menu") {
            popover.set_autohide(false);
        }
    }
    let hold_count = Cell::new(0);
    let hold = |widget: &gtk::Widget, x: f64, y: f64| {
        hold_count.set(hold_count.get() + 1);
        assert!(
            widget.width() > 0 && widget.height() > 0,
            "unallocated context target {}: {}x{}",
            widget.widget_name(),
            widget.width(),
            widget.height()
        );
        assert!(
            widget.pick(x, y, gtk::PickFlags::DEFAULT).is_some(),
            "unpickable context target {}: {}x{}, mapped={}, visible={}, sensitive={}",
            widget.widget_name(),
            widget.width(),
            widget.height(),
            widget.is_mapped(),
            widget.is_visible(),
            widget.is_sensitive()
        );
        let controllers = widget.observe_controllers();
        let gesture = (0..controllers.n_items())
            .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureLongPress>())
            .find(|g| g.name().as_deref() == Some("workspace-context-hold"))
            .unwrap();
        gesture.emit_by_name::<()>("pressed", &[&x, &y]);
        pump(150);
    };
    let context = || {
        w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find_map(|p| {
                (p.is_visible() && p.has_css_class("panel-context-menu"))
                    .then(|| p.downcast::<gtk::PopoverMenu>().ok())
                    .flatten()
            })
            .unwrap_or_else(|| {
                panic!(
                    "Context menu did not stay open after hold {}",
                    hold_count.get()
                )
            })
    };
    let snapshot_popover = |popover: &gtk::Popover, file: &str| {
        capture_popover(popover, &format!("{dir}/{file}.png"));
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        send(CustomizationAction::SetControlVisible {
            panel: Panel::Sizes,
            control: PanelControl::BrushOpacity,
            visible: false,
        });
        pump(250);
        capture_reference(&w, &format!("{dir}/initial-{theme:?}.png"), 1.0);
        let initial = state(&w).workspace;
        let tab = w
            .groups
            .borrow()
            .iter()
            .flat_map(|g| &g.tabs)
            .find(|(p, _)| *p == Panel::Sizes)
            .unwrap()
            .1
            .clone();
        hold(tab.upcast_ref(), 12.0, 12.0);
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("panel-menu-{theme:?}"));
        assert!(menu_action(&menu.menu_model().unwrap(), "Icons only").is_none());
        menu.popdown();
        let group = state(&w)
            .workspace
            .layout
            .panel_group(Panel::Sizes)
            .unwrap();
        let root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .root
            .clone();
        let header = find_css(root.upcast_ref(), "dock-tabs").unwrap();
        hold(&header, header.width() as f64 - 10.0, 12.0);
        let menu = context();
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Icons only").unwrap(),
            None,
        )
        .unwrap();
        pump(100);
        assert_eq!(
            state(&w).workspace.layout.group_tab_style(group).unwrap(),
            TabStyle::Icon
        );
        assert_eq!(
            tab.child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap()
                .icon_name()
                .as_deref(),
            Some("layer-size-symbolic")
        );
        send(CustomizationAction::SetTabStyle {
            group: 8,
            style: TabStyle::Name,
        });
        let before_expansion = state(&w).workspace.layout.bands;

        let original_panel = w.panel_widget(Panel::Sizes);
        let original_parent = original_panel.parent().unwrap();
        let original_root = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.panels.contains(&Panel::Sizes))
            .unwrap()
            .root
            .clone();
        send(CustomizationAction::ShowAllControls {
            panel: Panel::Sizes,
        });
        pump(300);
        capture_reference(
            &w,
            &format!("{dir}/expanded-sizes-before-{theme:?}.png"),
            1.0,
        );
        let inspector = original_root;
        assert!(inspector.has_css_class("expanded-panel"));
        assert_eq!(original_panel.parent().unwrap(), original_parent);
        assert!(
            w.popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .all(|p| !p.has_css_class("expanded-panel"))
        );
        assert_eq!(state(&w).workspace.layout.bands, before_expansion);
        let compact_opacity = find_named(
            original_panel.upcast_ref(),
            "panel-field-Sizes-BrushOpacity",
        )
        .unwrap();
        assert!(!compact_opacity.is_visible());
        let opacity = find_named(inspector.upcast_ref(), "configure-Sizes-BrushOpacity").unwrap();
        assert!(opacity.is_visible());
        let input = opacity
            .last_child()
            .and_downcast::<crate::number_control::NumberControl>()
            .unwrap();
        input.set_value(0.42);
        input.emit_by_name::<()>("value-changed", &[]);
        assert!((state(&w).brush.opacity - 0.42).abs() < 0.001);
        let visible = find_named(inspector.upcast_ref(), "panel-visible-BrushOpacity")
            .unwrap()
            .downcast::<gtk::CheckButton>()
            .unwrap();
        visible.set_active(true);
        pump(100);
        assert!(compact_opacity.is_visible());
        capture_reference(&w, &format!("{dir}/expanded-sizes-{theme:?}.png"), 1.0);
        assert!(w.reveal_chrome_at(850.0, 850.0));
        pump(300);
        assert!(state(&w).customization.expanded.is_none());
        assert!(
            w.panel_widget(Panel::Sizes)
                .parent()
                .is_some_and(|p| p.is::<gtk::Stack>())
        );
        assert!(compact_opacity.is_visible());
        assert_eq!(original_panel.parent().unwrap(), original_parent);

        let header = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 8)
            .unwrap()
            .stack
            .parent()
            .unwrap()
            .first_child()
            .unwrap();
        hold(&header, (header.width() - 12) as f64, 12.0);
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("group-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "New Toolbar…").unwrap(),
            None,
        )
        .unwrap();
        pump(250);
        let name = find_named(w.window.upcast_ref(), "toolbar-name")
            .unwrap()
            .downcast::<adw::EntryRow>()
            .unwrap();
        let confirm = find_named(w.window.upcast_ref(), "confirm-tools")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert!(!confirm.is_sensitive());
        name.set_text("Tools");
        assert!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .tool_picker()
                .unwrap()
                .error
                .is_some()
        );
        name.set_text(&format!("Illustration {theme:?}"));
        let search = find_named(w.window.upcast_ref(), "tool-search")
            .unwrap()
            .downcast::<gtk::SearchEntry>()
            .unwrap();
        search.set_text("pencil");
        pump(250);
        let choices = w
            .gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .tool_picker()
            .unwrap()
            .choices;
        assert!(!choices.is_empty());
        for choice in choices.iter().take(2) {
            send(CustomizationAction::PickerSelect {
                control: choice.control,
                selected: true,
            });
        }
        assert!(confirm.is_sensitive());
        capture_reference(&w, &format!("{dir}/tool-picker-{theme:?}.png"), 1.0);
        click(&confirm);
        pump(250); // Finish AdwDialog's closing animation before targeting the ribbon.
        let layout = state(&w).workspace.layout;
        let panel = layout
            .panels
            .iter()
            .find(|p| p.title() == format!("Illustration {theme:?}"))
            .unwrap()
            .id;
        let toolbar = w.panel_widget(panel).downcast::<TileStrip>().unwrap();
        assert_eq!(toolbar.overflow(), gtk::Overflow::Hidden);
        let tile = layout.panel(panel).unwrap().tiles()[0].id;
        let button = find_named(toolbar.upcast_ref(), &format!("tile-{tile}"))
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        click(&button);
        assert_eq!(
            state(&w).brush.preset,
            match choices[0].control {
                ToolbarControl::Brush { id } => id,
                _ => panic!("brush choice"),
            }
        );
        let root = button.parent().unwrap();
        hold(&root, 10.0, 10.0);
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("tile-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Insert Tools…").unwrap(),
            None,
        )
        .unwrap();
        pump(200);
        send(CustomizationAction::PickerSearch { query: "".into() });
        send(CustomizationAction::PickerSelect {
            control: ToolbarControl::Size { pixels: 64 },
            selected: true,
        });
        send(CustomizationAction::ConfirmTools);
        assert_eq!(
            state(&w).workspace.layout.panel(panel).unwrap().tiles()[0].control,
            ToolbarControl::Size { pixels: 64 }
        );
        let moved = state(&w).workspace.layout.panel(panel).unwrap().tiles()[0].id;
        let group = w
            .resolved()
            .groups
            .iter()
            .find(|g| g.panels.contains(&Panel::Toolbar))
            .unwrap()
            .id;
        w.dispatch(UiAction::SelectPanelTab {
            group,
            panel: Panel::Toolbar,
        });
        send(CustomizationAction::CloseExpanded);
        pump(250);
        let resolved = w.resolved();
        let destination = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Toolbar)
            .unwrap();
        let line = destination
            .tiles
            .as_ref()
            .unwrap()
            .insertion
            .last()
            .unwrap();
        let point = [
            destination.bounds.x + line.x + line.width * 0.5,
            destination.bounds.y
                + if destination.tabs_visible {
                    TAB_BAR_HEIGHT
                } else {
                    0.0
                }
                + line.y
                + line.height * 0.5,
        ];
        let item = DockItem::Tile { panel, tile: moved };
        let hint = w.drop_at(point[0], point[1], item).unwrap();
        *w.drop_hint.borrow_mut() = Some(hint);
        capture_reference(&w, &format!("{dir}/tile-drop-{theme:?}.png"), 1.0);
        let controllers = w.surface.observe_controllers();
        let drop = (0..controllers.n_items())
            .find_map(|i| controllers.item(i).and_downcast::<gtk::DropTarget>())
            .unwrap();
        assert!(drop.emit_by_name::<bool>(
            "drop",
            &[
                &glib::BoxedValue(NativeDockItem(item).to_value()),
                &(point[0] as f64),
                &(point[1] as f64)
            ]
        ));
        assert_eq!(
            state(&w)
                .workspace
                .layout
                .panel(Panel::Toolbar)
                .unwrap()
                .tiles()
                .last()
                .unwrap()
                .id,
            moved
        );
        w.dispatch(UiAction::MovePanel {
            viewport: [1200.0, 900.0],
            panel,
            target: DockTarget::Edge {
                edge: Edge::Left,
                outer: false,
            },
        });
        pump(200);
        capture_reference(&w, &format!("{dir}/custom-workspace-{theme:?}.png"), 1.0);
        let saved = state(&w).workspace;
        w.dispatch(UiAction::RestoreWorkspace { workspace: initial });
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: saved.clone(),
        });
        assert_eq!(state(&w).workspace, saved);
        w.dispatch(UiAction::Invoke {
            command: CommandId::ResetLayout,
        });
        assert!(state(&w).workspace.layout.panel(panel).is_ok());
        for tile in state(&w)
            .workspace
            .layout
            .panel(panel)
            .unwrap()
            .tiles()
            .to_vec()
        {
            send(CustomizationAction::RemoveTool {
                panel,
                tile: tile.id,
            });
        }
        let group = w
            .resolved()
            .groups
            .iter()
            .find(|g| g.panels.contains(&panel))
            .unwrap()
            .id;
        w.dispatch(UiAction::SelectPanelTab { group, panel });
        pump(150);
        capture_reference(&w, &format!("{dir}/empty-toolbar-{theme:?}.png"), 1.0);
        hold(&w.panel_widget(panel), 12.0, 12.0);
        let menu = context();
        snapshot_popover(menu.upcast_ref(), &format!("ribbon-menu-{theme:?}"));
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Add Tools…").unwrap(),
            None,
        )
        .unwrap();
        pump(150);
        assert!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .tool_picker()
                .is_some()
        );
        send(CustomizationAction::CancelTools);
        pump(250);
        // A large tabbed ribbon keeps all tiles but clips overflow rather than
        // installing a scroller that would compete with drag-to-reorder.
        let mut overflow = state(&w).workspace;
        let group = overflow.layout.panel_group(Panel::Sizes).unwrap();
        let many = overflow
            .layout
            .add_toolbar(
                Some(group),
                &format!("Many tools {theme:?}"),
                &vec![
                    ToolbarControl::Command {
                        command: CommandId::Brush
                    };
                    80
                ],
            )
            .unwrap();
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: overflow,
        });
        pump(200);
        let strip = w.panel_widget(many);
        assert_eq!(strip.overflow(), gtk::Overflow::Hidden);
        assert!(strip.is::<TileStrip>());
        let resolved = w.resolved();
        let group = resolved.groups.iter().find(|g| g.active == many).unwrap();
        let tiles = &group.tiles.as_ref().unwrap().tiles;
        assert_eq!(tiles.len(), 80);
        assert!(tiles.last().unwrap().y >= group.bounds.height - TAB_BAR_HEIGHT);
        capture_reference(&w, &format!("{dir}/clipped-ribbon-{theme:?}.png"), 1.0);
        w.dispatch(UiAction::MovePanel {
            panel: many,
            target: DockTarget::Edge {
                edge: Edge::Left,
                outer: false,
            },
            viewport: [1200.0, 900.0],
        });
        pump(200);
        assert!(w.panel_widget(many).width() > (TILE_SIZE * 2.0) as i32);
        capture_reference(&w, &format!("{dir}/wrapped-ribbon-{theme:?}.png"), 1.0);
    }
    w.window.destroy();
    pump(150);
}

#[test]
#[ignore = "toolbar manager: requires a private Wayland/Vulkan display"]
fn native_toolbar_manager() {
    let app = native_test_app("art.capycanvas.ToolbarManagerTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    pump(500);
    let initial = state(&w).workspace;
    let send = |action| w.dispatch(UiAction::Customize { action });
    let model = || {
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .toolbar_manager()
            .unwrap()
    };
    let dir = "../../artifacts/ui/toolbar-manager";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for name in ["Sketching", "Painting"] {
            send(CustomizationAction::DuplicateToolbar {
                panel: Panel::Toolbar,
            });
            pump(100);
            send(CustomizationAction::ToolbarName { name: name.into() });
            send(CustomizationAction::ConfirmToolbar);
            pump(200);
        }
        let hidden = state(&w).workspace.layout.panels.last().unwrap().id;
        send(CustomizationAction::SetPanelVisible {
            panel: hidden,
            visible: false,
        });
        let before = serde_json::to_value(state(&w).workspace).unwrap();
        let menu = find_named(w.window.upcast_ref(), "workspace-menu")
            .unwrap()
            .downcast::<gtk::PopoverMenu>()
            .unwrap();
        menu.set_autohide(false);
        menu.popup();
        pump(150);
        capture_popover(
            menu.upcast_ref(),
            &format!("{dir}/workspace-menu-{theme:?}.png"),
        );
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Manage Toolbars…").unwrap(),
            None,
        )
        .unwrap();
        pump(300);
        let dialog = find_named(w.window.upcast_ref(), "toolbar-manager")
            .unwrap()
            .downcast::<adw::Dialog>()
            .unwrap();
        let list = find_named(dialog.upcast_ref(), "managed-toolbars")
            .unwrap()
            .downcast::<gtk::ListBox>()
            .unwrap();
        let delete = find_named(dialog.upcast_ref(), "delete-managed-toolbar")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap();
        assert_eq!(model().toolbars.len(), 3);
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-initial.png"), 1.0);
        list.select_row(list.row_at_index(2).as_ref());
        assert_eq!(model().selected, Some(hidden));
        assert!(delete.is_sensitive());
        pump(100);
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-selected.png"), 1.0);
        click(&delete);
        pump(200);
        let prompt = find_named(w.window.upcast_ref(), "toolbar-dialog")
            .unwrap()
            .downcast::<adw::AlertDialog>()
            .unwrap();
        assert_eq!(w.window.visible_dialog(), Some(prompt.clone().upcast()));
        assert!(prompt.body().contains("Painting"));
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-confirm.png"), 1.0);
        click(&find_button(prompt.upcast_ref(), "Cancel").unwrap());
        pump(200);
        assert_eq!(serde_json::to_value(state(&w).workspace).unwrap(), before);
        assert_eq!(model().selected, Some(hidden));
        click(&delete);
        pump(150);
        click(&find_button(prompt.upcast_ref(), "Delete Toolbar").unwrap());
        pump(200);
        assert_eq!(model().toolbars.len(), 2);
        assert!(model().selected.is_none());
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-deleted.png"), 1.0);
        while !model().toolbars.is_empty() {
            list.select_row(list.row_at_index(0).as_ref());
            click(&delete);
            pump(150);
            click(&find_button(prompt.upcast_ref(), "Delete Toolbar").unwrap());
            pump(150);
        }
        assert!(!delete.is_sensitive());
        capture_reference(&w, &format!("{dir}/gtk-{theme:?}-empty.png"), 1.0);
        dialog.close();
        pump(300);
        assert!(
            w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .toolbar_manager()
                .is_none()
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert!(
            state(&w)
                .workspace
                .layout
                .panels
                .iter()
                .any(|p| p.id.kind() == PanelKind::Tiles)
        );
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native menu sections: requires a Wayland/Vulkan display"]
fn native_menu_sections() {
    let app = native_test_app("dev.layer.MenuTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(500);
    let dir = "../../artifacts/ui/menus";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let theme_action = w
            .menu_actions
            .lookup_action(&CommandId::ToggleTheme.shortcut_id())
            .unwrap();
        assert_eq!(
            theme_action.state().unwrap().get::<bool>(),
            Some(theme == Theme::Dark)
        );
        theme_action.activate(None);
        assert_eq!(
            theme_action.state().unwrap().get::<bool>(),
            Some(theme != Theme::Dark)
        );
        theme_action.activate(None);
        assert_eq!(state(&w).theme, theme);
        for (label, sections) in MENUS
            .iter()
            .map(|m| (m.label, m.sections))
            .chain([("Main Menu", PRIMARY_MENU)])
        {
            let menu = w
                .popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .filter_map(|p| p.downcast::<gtk::PopoverMenu>().ok())
                .find(|p| p.parent().unwrap().tooltip_text().as_deref() == Some(label))
                .unwrap();
            menu.popup();
            pump(100);
            let root = menu.menu_model().unwrap();
            let expected: Vec<Vec<String>> = if sections.is_empty() {
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .workspace_menu()
                    .sections
                    .into_iter()
                    .map(|section| section.into_iter().map(|item| item.label).collect())
                    .collect()
            } else {
                sections
                    .iter()
                    .map(|section| {
                        section
                            .iter()
                            .map(|command| command.label().to_owned())
                            .collect()
                    })
                    .collect()
            };
            assert_eq!(root.n_items() as usize, expected.len());
            for (index, commands) in expected.iter().enumerate() {
                let model = root.item_link(index as i32, "section").unwrap();
                assert_eq!(model.n_items() as usize, commands.len());
                for (index, command) in commands.iter().enumerate() {
                    assert_eq!(
                        model
                            .item_attribute_value(index as i32, "label", None)
                            .unwrap()
                            .str(),
                        Some(command.as_str())
                    );
                }
            }
            menu.popup();
            pump(200);
            capture_popover(menu.upcast_ref(), &format!("{dir}/{label}-{theme:?}.png"));
            menu.popdown();
            pump(100);
        }
    }
    // Replacing an accelerator must update an item inside its section, not
    // replace a section in the root or leave the old hint on screen.
    let mut settings = state(&w).settings;
    settings
        .shortcuts
        .insert(CommandId::Settings.shortcut_id(), vec![]);
    w.dispatch(UiAction::RestoreSettings { settings });
    let menus = w.menus.borrow();
    let section = menus
        .iter()
        .find(|m| m.commands.contains(&CommandId::Settings))
        .unwrap();
    assert_eq!(section.model.n_items(), 3);
    assert_eq!(
        section
            .model
            .item_attribute_value(0, "accel", None)
            .unwrap()
            .str(),
        Some("")
    );
}

#[test]
#[ignore = "two-column panel expansion: requires a Wayland/Vulkan display"]
fn native_panel_expansion() {
    let app = native_test_app("dev.layer.ExpansionTest");
    // Static geometry/picking captures also verify the reduced-motion path.
    // Interpolation fractions are covered by the shared layout tests.
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    let w = Workspace::new(&app);
    w.window.present();
    pump(600);
    let initial = state(&w).workspace;
    let dir = "../../artifacts/ui/customization";
    std::fs::create_dir_all(dir).unwrap();
    let tap_tab = |panel| {
        let tab = w
            .groups
            .borrow()
            .iter()
            .flat_map(|g| &g.tabs)
            .find(|(p, _)| *p == panel)
            .unwrap()
            .1
            .clone();
        let bounds = tab.compute_bounds(&w.surface).unwrap();
        let reply = w.chrome_event(ChromeEvent::Contact {
            position: [
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            ],
            canvas: false,
        });
        assert!(
            !reply.handled,
            "tab press must remain available for native drag/hold"
        );
        click(&tab);
    };
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: initial.clone(),
            });
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Sizes,
                target: DockTarget::Edge { edge, outer: false },
                viewport: [1200.0, 900.0],
            });
            pump(100);
            let saved = state(&w).workspace;
            let panel = w.panel_widget(Panel::Sizes);
            let parent = panel.parent().unwrap();
            let root = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.panels.contains(&Panel::Sizes))
                .unwrap()
                .root
                .clone();
            tap_tab(Panel::Sizes);
            pump(300);
            capture_reference(&w, &format!("{dir}/expanded-{edge:?}-{theme:?}.png"), 1.0);
            let placement = w.customization.placement().unwrap();
            assert_eq!(panel.parent().unwrap(), parent);
            assert_eq!(
                placement.preview.height,
                placement.configuration.height + TAB_BAR_HEIGHT
            );
            assert_eq!(placement.configuration.y, TAB_BAR_HEIGHT);
            assert!(placement.configuration.width > placement.preview.width);
            let point = gtk::graphene::Point::new(
                placement.bounds.x + placement.configuration.x + 16.0,
                placement.bounds.y + 80.0,
            );
            let picked = w
                .surface
                .pick(point.x() as f64, point.y() as f64, gtk::PickFlags::DEFAULT)
                .unwrap();
            assert!(
                picked.is_ancestor(&root),
                "configuration must be the actual raised group, not a popover"
            );
            let check = find_named(root.upcast_ref(), "panel-visible-BrushColor")
                .unwrap()
                .downcast::<gtk::CheckButton>()
                .unwrap();
            check.set_active(true);
            pump(100);
            assert!(
                find_named(&panel, "panel-field-Sizes-BrushColor")
                    .unwrap()
                    .is_visible()
            );
            check.set_active(false);
            tap_tab(Panel::Sizes);
            pump(300);
            assert!(w.customization.placement().is_none());
            assert_eq!(state(&w).workspace, saved);
            assert_eq!(panel.parent().unwrap(), parent);
        }
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Sizes,
            target: DockTarget::Tab {
                group: 8,
                index: None,
            },
            viewport: [1200.0, 900.0],
        });
        pump(100);
        let preview_parent = w.panel_widget(Panel::Sizes).parent().unwrap();
        for (panel, name, concave) in [
            (Panel::Sizes, "second", false),
            (Panel::Layers, "first", true),
        ] {
            tap_tab(panel);
            pump(150);
            capture_reference(
                &w,
                &format!("{dir}/expanded-left-{name}-tab-{theme:?}.png"),
                1.0,
            );
            let placement = w.customization.placement().unwrap();
            assert_eq!(placement.concave_join, concave);
            assert_eq!(state(&w).customization.expanded, Some(panel));
            assert_eq!(
                w.panel_widget(Panel::Sizes).parent().unwrap(),
                preview_parent
            );
        }
        tap_tab(Panel::Layers);
        assert!(state(&w).customization.expanded.is_none());
    }
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::TotalZen,
            value: layer_ui::PreferenceValue::Bool(true),
        },
    });
    w.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    w.dragging.set(true);
    w.update_zen();
    assert!(!w.interact(UiInput::Blur).chrome_hidden);
    for group in w.groups.borrow().iter() {
        assert!(!group.root.has_css_class("zen-hidden"));
        assert!(group.root.can_target());
    }
    capture_reference(&w, &format!("{dir}/zen-drag-Light.png"), 1.0);
    w.dragging.set(false);
    w.update_zen();
    assert!(
        w.groups
            .borrow()
            .iter()
            .all(|g| g.root.has_css_class("zen-hidden"))
    );
    let hide = w.chrome_event(ChromeEvent::Contact {
        position: [600.0, 350.0],
        canvas: true,
    });
    assert!(!hide.handled && hide.chrome_hidden);
    assert!(
        w.groups
            .borrow()
            .iter()
            .all(|g| g.root.has_css_class("zen-hidden"))
    );
}

#[test]
#[ignore = "GTK visual reference for the web: requires a Wayland display"]
fn native_web_parity_reference() {
    let app = native_test_app("dev.layer.ParityTest");
    gtk::Settings::default()
        .unwrap()
        .set_gtk_enable_animations(false);
    std::fs::create_dir_all("../../artifacts/ui/parity").unwrap();
    fn record(widget: &gtk::Widget, root: &gtk::Widget) -> serde_json::Value {
        let b = widget.compute_bounds(root).unwrap();
        let mut children = Vec::new();
        let mut child = widget.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            if c.is_visible() && c.is_child_visible() {
                children.push(record(&c, root));
            }
        }
        serde_json::json!({
            "type": widget.type_().name(),
            "name": widget.widget_name().to_string(),
            "css": widget.css_classes().iter().map(ToString::to_string).collect::<Vec<_>>(),
            "bounds": [b.x(), b.y(), b.width(), b.height()],
            "font": widget.pango_context().font_description().map(|f| f.to_string()),
            "text": widget.downcast_ref::<gtk::Label>().map(|l| l.text().to_string()),
            "icon": widget.downcast_ref::<gtk::Image>().and_then(|i| i.icon_name()).map(|s| s.to_string()),
            "children": children,
        })
    }
    for (scheme, name) in [
        (adw::ColorScheme::ForceDark, "dark"),
        (adw::ColorScheme::ForceLight, "light"),
    ] {
        for modal in [false, true] {
            adw::StyleManager::default().set_color_scheme(scheme);
            adw::StyleManager::for_display(&gdk::Display::default().unwrap())
                .set_color_scheme(adw::ColorScheme::Default);
            let w = Workspace::new(&app);
            w.window.present();
            assert!(w.gpu.borrow().is_some());
            if modal {
                w.dispatch(UiAction::Invoke {
                    command: CommandId::Settings,
                });
            }
            pump(2500);
            let name = if modal {
                format!("settings-{name}")
            } else {
                name.to_string()
            };
            capture_reference(
                &w,
                &format!("../../artifacts/ui/parity/gtk-{name}.png"),
                1.0,
            );
            if !modal {
                capture_reference(
                    &w,
                    &format!("../../artifacts/ui/parity/gtk-{name}-2x.png"),
                    2.0,
                );
            }
            let mut reference = record(w.window.upcast_ref(), w.window.upcast_ref());
            reference["scale"] = serde_json::json!(w.area.scale_factor());
            reference["camera"] = serde_json::to_value(state(&w).camera).unwrap();
            std::fs::write(
                format!("../../artifacts/ui/parity/gtk-{name}.json"),
                serde_json::to_vec(&reference).unwrap(),
            )
            .unwrap();
            w.window.destroy();
            pump(100);
        }
    }
}

#[test]
#[ignore = "recursive column collapse: requires a private Wayland/Vulkan display"]
fn native_column_removal() {
    let app = native_test_app("art.capycanvas.ColumnRemovalTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    let placement = |panel| {
        w.resolved()
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&panel))
            .unwrap()
            .bounds
    };
    for edge in [Edge::Left, Edge::Right] {
        for multiple in [false, true] {
            let mut workspace = initial.clone();
            let layout = &mut workspace.layout;
            layout.set_panel_visible(Panel::Toolbar, false).unwrap();
            let sizes = layout.panel_group(Panel::Sizes).unwrap();
            layout
                .move_panel(
                    viewport,
                    Panel::Layers,
                    DockTarget::Split { group: sizes, edge },
                )
                .unwrap();
            if multiple {
                let toolbar = layout
                    .add_toolbar(None, "Test", &[ToolbarControl::Color])
                    .unwrap();
                let brushes = layout.panel_group(Panel::Brushes).unwrap();
                layout
                    .move_panel(
                        viewport,
                        toolbar,
                        DockTarget::Split {
                            group: brushes,
                            edge,
                        },
                    )
                    .unwrap();
            }
            layout.bands[0].edge = edge;
            w.dispatch(UiAction::RestoreWorkspace { workspace });
            pump(180);
            let before = state(&w).workspace;
            let original = placement(Panel::Sizes);
            let band = before.layout.bands[0].extent;
            let removed = placement(Panel::Layers).width;
            capture_reference(
                &w,
                &format!("{dir}/column-{edge:?}-{multiple}-before.png"),
                1.0,
            );
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetPanelVisible {
                    panel: Panel::Layers,
                    visible: false,
                },
            });
            pump(180);
            if multiple {
                assert_eq!(state(&w).workspace.layout.bands[0].extent, band);
                assert!(placement(Panel::Sizes).width > original.width);
            } else {
                assert!((placement(Panel::Sizes).width - original.width).abs() < 0.1);
                assert!(
                    (state(&w).workspace.layout.bands[0].extent - band
                        + removed
                        + WORKSPACE_SPACING)
                        .abs()
                        < 0.1
                );
                assert!((placement(Panel::Brushes).width - original.width).abs() < 0.1);
            }
            capture_reference(
                &w,
                &format!("{dir}/column-{edge:?}-{multiple}-after.png"),
                1.0,
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            assert_eq!(
                serde_json::to_value(state(&w).workspace).unwrap(),
                serde_json::to_value(before).unwrap()
            );
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "docked handle double-click: requires a Wayland/Vulkan display"]
fn native_docked_handles() {
    let app = native_test_app("art.capycanvas.DockedHandleTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let panel = Panel::Sizes;
    let group = state(&w).workspace.layout.panel_group(panel).unwrap();
    let controllers = w.surface.observe_controllers();
    let click = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureClick>())
        .find(|g| g.name().as_deref() == Some("panel-handle-double-click"))
        .unwrap();
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for style in TabStyle::ALL {
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTabStyle { group, style },
            });
            pump(200);
            let initial = state(&w).workspace;
            for hidden in [true, false] {
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group)
                    .unwrap()
                    .root
                    .clone();
                let grip = find_css(root.upcast_ref(), "panel-grip").unwrap();
                let b = grip.compute_bounds(&w.surface).unwrap();
                let point = [b.x() + b.width() / 2.0, b.y() + b.height() / 2.0];
                assert!(
                    matches!(w.drag_target_at(point), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
                );
                click.emit_by_name::<()>(
                    "pressed",
                    &[&2i32, &(point[0] as f64), &(point[1] as f64)],
                );
                pump(300);
                let mut expected = initial.clone();
                expected
                    .layout
                    .panels
                    .iter_mut()
                    .find(|p| p.id == panel)
                    .unwrap()
                    .hide_tab = hidden;
                // Hiding a header changes host text measurements, not dock sizes.
                expected.layout.measurements = state(&w).workspace.layout.measurements;
                assert_eq!(
                    state(&w).workspace,
                    expected,
                    "First double-click toggles only tab visibility"
                );
                let resolved = w
                    .resolved()
                    .groups
                    .into_iter()
                    .find(|g| g.id == group)
                    .unwrap();
                assert!(!resolved.floating);
                assert_eq!(resolved.tabs_visible, !hidden);
                capture_reference(
                    &w,
                    &format!("{dir}/docked-handle-{style:?}-{hidden}-{theme:?}.png"),
                    1.0,
                );
            }
        }
    }
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: initial.clone(),
            });
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTileStyle {
                    panel: Panel::Toolbar,
                    style,
                },
            });
            w.dispatch(UiAction::MovePanel {
                panel: Panel::Toolbar,
                viewport,
                target: DockTarget::Edge { edge, outer: true },
            });
            pump(200);
            let natural = w
                .resolved()
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Toolbar)
                .unwrap();
            let mut oversized = state(&w).workspace;
            oversized
                .layout
                .bands
                .iter_mut()
                .find(|b| b.root.id() == natural.id)
                .unwrap()
                .extent += 120.0;
            w.dispatch(UiAction::RestoreWorkspace {
                workspace: oversized,
            });
            pump(200);
            let root = w.panel_widget(Panel::Toolbar);
            let grip = find_css(&root, "panel-grip").unwrap();
            let b = grip.compute_bounds(&w.surface).unwrap();
            click.emit_by_name::<()>(
                "pressed",
                &[
                    &2i32,
                    &((b.x() + b.width() / 2.0) as f64),
                    &((b.y() + b.height() / 2.0) as f64),
                ],
            );
            pump(300);
            let fitted = w
                .resolved()
                .groups
                .into_iter()
                .find(|g| g.id == natural.id)
                .unwrap();
            assert_eq!(fitted.bounds, natural.bounds, "{edge:?} {style:?}");
            assert!(!fitted.tabs_visible && !fitted.floating);
            capture_reference(
                &w,
                &format!("{dir}/docked-toolbar-reset-{edge:?}-{style:?}.png"),
                1.0,
            );
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "tab visibility and bottom grips: requires a private Wayland/Vulkan display"]
fn native_hidden_tabs() {
    let app = native_test_app("art.capycanvas.HiddenTabsTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
    let initial = state(&w).workspace;
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let dir = "../../artifacts/ui/workspace-management/gtk";
    std::fs::create_dir_all(dir).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let panel = Panel::Sizes;
        let group = state(&w).workspace.layout.panel_group(panel).unwrap();
        for action in [
            CustomizationAction::SetTabStyle {
                group,
                style: TabStyle::Icon,
            },
            CustomizationAction::SetTabHidden {
                panel,
                hidden: true,
            },
        ] {
            w.dispatch(UiAction::Customize { action });
        }
        pump(150);
        let footer = || {
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}"),
            )
            .unwrap()
        };
        let handle = footer();
        let b = handle.compute_bounds(&w.surface).unwrap();
        assert_eq!(b.height(), 20.0);
        assert!(
            matches!(w.drag_target_at([b.x() + b.width() / 2.0, b.y() + 10.0]), Some(DragTarget::Dock(DockItem::Group { group: id })) if id == group)
        );
        assert!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .is_empty()
        );
        capture_reference(&w, &format!("{dir}/tab-hidden-docked-{theme:?}.png"), 1.0);
        // The footer's actual touch context binding still exposes name/icon
        // selection and the independent Show tab bar toggle.
        let controllers = handle.observe_controllers();
        let hold = (0..controllers.n_items())
            .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureLongPress>())
            .find(|c| c.name().as_deref() == Some("workspace-context-hold"))
            .unwrap();
        hold.emit_by_name::<()>("pressed", &[&(b.width() as f64 * 0.5), &10.0f64]);
        pump(150);
        let menu = w
            .popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find(|p| p.has_css_class("panel-context-menu"))
            .unwrap();
        capture_popover(&menu, &format!("{dir}/tab-hidden-menu-{theme:?}.png"));
        let popup = menu.clone().downcast::<gtk::PopoverMenu>().unwrap();
        let actions = popup.menu_model().unwrap().item_link(2, "section").unwrap();
        assert_eq!(
            actions
                .item_attribute_value(0, "label", None)
                .unwrap()
                .get::<String>()
                .unwrap(),
            "Configure Brush size panel…"
        );
        let action = actions
            .item_attribute_value(0, "action", None)
            .unwrap()
            .get::<String>()
            .unwrap();
        popup.activate_action(&action, None).unwrap();
        pump(300);
        assert_eq!(state(&w).customization.expanded, Some(panel));
        capture_reference(
            &w,
            &format!("{dir}/tab-hidden-configure-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        });
        pump(300);
        menu.popdown();
        pump(100);
        let drag = begin_workspace_drag(&w, &handle, b.width() * 0.5, 10.0);
        let target = [viewport[0] * 0.55, viewport[1] * 0.55];
        drag.update([
            (target[0] - drag.origin[0]) as f64,
            (target[1] - drag.origin[1]) as f64,
        ]);
        pump(150);
        drag.end();
        pump(100);
        let placement = w
            .resolved()
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap();
        assert!(placement.floating && !placement.tabs_visible);
        assert!(footer().is_mapped());
        capture_reference(&w, &format!("{dir}/tab-hidden-floating-{theme:?}.png"), 1.0);
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabHidden {
                panel,
                hidden: false,
            },
        });
        pump(150);
        let tab = w
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == group)
            .unwrap()
            .tabs[0]
            .1
            .clone();
        let content = tab.child().unwrap();
        assert!(content.first_child().unwrap().is_visible());
        assert!(!content.last_child().unwrap().is_visible());
        assert!(
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}")
            )
            .is_none()
        );
        capture_reference(
            &w,
            &format!("{dir}/tab-restored-floating-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetTabHidden {
                panel,
                hidden: true,
            },
        });
        pump(100);
        let before_dock = state(&w).workspace;
        let grip = footer().compute_bounds(&w.surface).unwrap();
        for (phase, position) in [
            (
                layer_ui::ContactPhase::Down,
                [grip.x() + grip.width() * 0.5, grip.y() + 10.],
            ),
            (
                layer_ui::ContactPhase::Move,
                [viewport[0] - 1., viewport[1] * 0.5],
            ),
            (
                layer_ui::ContactPhase::Up,
                [viewport[0] - 1., viewport[1] * 0.5],
            ),
        ] {
            w.dispatch(UiAction::DragWorkspace {
                item: DockItem::Group { group },
                phase,
                position,
                viewport,
                tabs: vec![],
            });
        }
        pump(150);
        assert!(!state(&w).workspace.layout.panel(panel).unwrap().hide_tab);
        assert_eq!(
            w.groups
                .borrow()
                .iter()
                .find(|g| g.id == group)
                .unwrap()
                .tabs
                .len(),
            1
        );
        assert!(
            find_named(
                w.surface.upcast_ref(),
                &format!("panel-footer-grip-{group}")
            )
            .is_none()
        );
        capture_reference(
            &w,
            &format!("{dir}/tab-shown-after-docking-{theme:?}.png"),
            1.0,
        );
        w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        assert_eq!(
            serde_json::to_value(state(&w).workspace).unwrap(),
            serde_json::to_value(before_dock).unwrap()
        );
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "native divider hit testing: requires a Wayland display"]
fn native_stacked_divider() {
    let app = native_test_app("art.capycanvas.DividerTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(800);
    let divider = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.id == 4)
        .unwrap();
    let b = divider.bounds;
    let point = [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let picked = w
        .surface
        .pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT)
        .unwrap();
    let handle = w
        .surface
        .imp()
        .children
        .borrow()
        .iter()
        .find(|(slot, _)| *slot == Slot::Divider(4))
        .unwrap()
        .1
        .clone();
    assert_eq!(
        picked, handle,
        "the horizontal divider must own its complete hit area"
    );
    let drag = begin_workspace_drag(&w, &handle, b.width * 0.5, b.height * 0.5);
    for dy in [-100.0, 50.0, -70.0] {
        drag.update([0.0f64, (dy as f64)]);
        pump(50);
        let actual = handle.compute_bounds(&w.surface).unwrap();
        assert!(
            (actual.y() - b.y - dy).abs() <= 1.0,
            "divider y={} expected {}",
            actual.y(),
            b.y + dy
        );
    }
    drag.update([0.0f64, 0.0f64]);
    drag.end();
    let mut workspace = state(&w).workspace;
    if let DockNode::Split { id, .. } = &mut workspace.layout.bands[0].root {
        *id = 40;
    }
    let mut json = serde_json::to_value(&workspace).unwrap();
    json["layout"]["next_id"] = 41.into();
    workspace = serde_json::from_value(json).unwrap();
    w.dispatch(UiAction::RestoreWorkspace { workspace });
    pump(100);
    assert!(
        w.surface
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(s, _)| *s == Slot::Divider(40)),
        "same panels with a new split must replace the old native handle"
    );
    let tool = command(&w, CommandId::Brush);
    std::fs::create_dir_all("../../artifacts/ui/preferences").unwrap();
    capture_reference(&w, "../../artifacts/ui/preferences/gtk-typography.png", 1.0);
    for widget in [
        w.groups.borrow()[0].tabs[0]
            .1
            .clone()
            .upcast::<gtk::Widget>(),
        w.size_number.clone().upcast(),
        w.view_info.clone().upcast(),
    ] {
        let font = widget.pango_context().font_description().unwrap();
        assert!(
            (font.size() as f64 / gtk::pango::SCALE as f64 - UI_TEXT_PT as f64 * 4.0 / 3.0).abs()
                < 0.02,
            "expected {UI_TEXT_PT}pt, got {font}"
        );
    }
    assert_eq!([tool.width(), tool.height()], [36, 36]);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "multiple-window native teardown: requires a Wayland display"]
fn native_window_lifecycle() {
    let app = native_test_app("art.capycanvas.LifecycleTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    let first = windows.borrow()[0].clone();
    pump(200);
    for _ in 0..12 {
        app.activate_action("new-window", None);
        pump(100);
        let next = windows.borrow().last().unwrap().clone();
        next.window.destroy();
        pump(150);
        assert!(next.gpu.borrow().is_none());
        assert_eq!(windows.borrow().len(), 1);
    }
    first.window.destroy();
    pump(100);
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native GTK widgets: requires a Wayland display"]
fn native_ribbon_allocation() {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(&crate::stylesheet());
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let strip = TileStrip::new();
    strip.add_css_class("toolbar-controls");
    for _ in 0..6 {
        strip.append(&gtk::Button::from_icon_name("document-edit-symbolic"));
    }
    strip.set_grip(&tiles::grip());
    for (axis, edge) in [(Axis::Horizontal, Edge::Top), (Axis::Vertical, Edge::Left)] {
        let mut layout = DockLayout::default();
        layout.bands.retain(|b| b.id == 1);
        layout.bands[0].edge = edge;
        strip.configure(axis, true);
        for length in [96.0, 500.0] {
            let viewport = if axis == Axis::Horizontal {
                [length, 800.0]
            } else {
                [800.0, length]
            };
            let g = layout.resolve(viewport[0], viewport[1]).groups.remove(0);
            strip.allocate(g.bounds.width as i32, g.bounds.height as i32, -1, None);
            let expected = tile_layout(
                g.bounds.width,
                g.bounds.height,
                axis,
                6,
                true,
                TileStyle::Small,
            );
            let mut child = strip.first_child();
            for b in expected.tiles.into_iter().chain(expected.grip) {
                let widget = child.unwrap();
                let actual = widget.compute_bounds(&strip).unwrap();
                assert_eq!(
                    (actual.x(), actual.y(), actual.width(), actual.height()),
                    (b.x, b.y, b.width, b.height)
                );
                child = widget.next_sibling();
            }
            assert!(child.is_none());
            assert_eq!(layout.bands[0].extent, TILE_SIZE + 6.0);
        }
    }
}

#[test]
#[ignore = "native sidebar, shortcut recording and persistence: requires a Wayland display"]
fn native_preferences_and_shortcuts() {
    if let Some(path) = std::env::var_os("LAYER_SETTINGS_FILE") {
        assert!(
            !std::path::Path::new(&path).exists(),
            "Use a fresh isolated preferences path for this test"
        );
    }
    let app = native_test_app("dev.layer.PreferencesTest");
    let windows: Rc<RefCell<Vec<Rc<Workspace>>>> = Rc::default();
    crate::install_actions(&app, &windows);
    app.activate_action("new-window", None);
    let w = windows.borrow()[0].clone();
    pump(300);
    assert_eq!(w.window.title().as_deref(), Some(APP_NAME));
    click(&command(&w, CommandId::NewWindow));
    assert_eq!(windows.borrow().len(), 2);
    let second = windows.borrow()[1].clone();
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(100);
    assert_eq!(state(&second).settings, state(&w).settings);
    second.window.destroy();
    w.window.present();
    pump(100);
    assert_eq!(windows.borrow().len(), 1);
    let dir = "../../artifacts/ui/preferences";
    std::fs::create_dir_all(dir).unwrap();
    assert_eq!(w.preferences.dialog.content_width(), 1000);
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Appearance,
    });
    pump(300);
    find_named(
        w.preferences.dialog.upcast_ref(),
        "preferences-search-toggle",
    )
    .unwrap()
    .grab_focus();
    let controllers = w.window.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .and_downcast::<gtk::EventControllerKey>()
        })
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::P, &0u32, &gdk::ModifierType::SHIFT_MASK]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::P, &0u32, &gdk::ModifierType::SHIFT_MASK],
    );
    pump(250);
    let search: gtk::SearchEntry = find_named(w.preferences.dialog.upcast_ref(), "settings-search")
        .unwrap()
        .downcast()
        .unwrap();
    assert_eq!(search.text().as_str(), "P");
    assert!(
        gtk::prelude::GtkWindowExt::focus(&w.window)
            .is_some_and(|focus| focus.is_ancestor(&search))
    );
    assert_eq!(search.position(), 1);
    assert!(!keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::r, &0u32, &gdk::ModifierType::empty()]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::r, &0u32, &gdk::ModifierType::empty()],
    );
    assert_eq!(
        state(&w).preferences.query,
        "P",
        "focused search uses native text input"
    );
    for (theme, suffix) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for page in SettingsPage::ALL {
            w.dispatch(UiAction::OpenSettings { page });
            assert!(state(&w).settings_open, "open action must reach the core");
            pump(400);
            assert!(state(&w).settings_open, "settings must remain open");
            assert!(
                w.preferences.dialog.is_mapped(),
                "open settings must map the native dialog"
            );
            let search_toggle: gtk::ToggleButton = find_named(
                w.preferences.dialog.upcast_ref(),
                "preferences-search-toggle",
            )
            .unwrap()
            .downcast()
            .unwrap();
            assert_eq!(
                search_toggle.icon_name().as_deref(),
                Some("edit-find-symbolic")
            );
            let sidebar =
                find_named(w.preferences.dialog.upcast_ref(), "preferences-sidebar").unwrap();
            let sidebar_bounds = sidebar.compute_bounds(&w.window).unwrap();
            assert!(find_named(w.preferences.dialog.upcast_ref(), "close-settings").is_none());
            let content = find_named(w.preferences.dialog.upcast_ref(), "preferences-content")
                .unwrap()
                .compute_bounds(&w.window)
                .unwrap();
            capture_reference(&w, &format!("{dir}/gtk-{}-{suffix}.png", page.key()), 1.0);
            assert!(
                (sidebar_bounds.y() + sidebar_bounds.height() - content.y() - content.height())
                    .abs()
                    < 1.0,
                "sidebar and settings content finish at the same height"
            );
            assert_eq!(
                w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .preferences()
                    .unwrap()
                    .page,
                page
            );
            if page == SettingsPage::Input {
                let prediction: adw::SpinRow = find_named(
                    w.preferences.dialog.upcast_ref(),
                    "setting-prediction-horizon",
                )
                .unwrap()
                .downcast()
                .unwrap();
                assert_eq!(prediction.text(), "8 ms");
                prediction.set_text("32");
                prediction.update();
                assert_eq!(state(&w).settings.prediction_ms, 32.0);
                prediction.set_text("");
                assert_eq!(
                    state(&w).settings.prediction_ms,
                    32.0,
                    "empty spin draft does not reset"
                );
                prediction.update();
                assert_eq!(state(&w).settings.prediction_ms, 8.0);
                assert_eq!(prediction.text(), "8 ms");
                let field =
                    find_named(w.preferences.dialog.upcast_ref(), "setting-pressure").unwrap();
                let title = find_css(&field, "number-title").unwrap();
                let feedback =
                    find_named(w.preferences.dialog.upcast_ref(), "setting-feedback").unwrap();
                let native_title = find_css(&feedback, "title").unwrap();
                assert_eq!(
                    title.compute_bounds(&w.window).unwrap().x(),
                    native_title.compute_bounds(&w.window).unwrap().x(),
                    "slider labels align with native settings rows"
                );
                assert_eq!(
                    title.compute_bounds(&field).unwrap().x(),
                    0.0,
                    "only panel slider labels get an extra inset"
                );
                let scale = field
                    .last_child()
                    .unwrap()
                    .first_child()
                    .unwrap()
                    .next_sibling()
                    .unwrap()
                    .downcast::<gtk::Scale>()
                    .unwrap();
                assert_eq!(scale.height(), 32, "settings keep the expanded slider");
                let (start, end) = scale.slider_range();
                assert!(end - start >= 16, "settings keep the visible thumb");
            }
            if page == SettingsPage::About {
                assert!(w.preferences.dialog.is_mapped());
                let view = w
                    .gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .preferences()
                    .unwrap();
                for row in view
                    .pages
                    .iter()
                    .flat_map(|p| &p.groups)
                    .flat_map(|g| &g.rows)
                {
                    if let PreferenceKind::Link { label, url } = &row.kind {
                        let native: adw::ActionRow = find_named(
                            w.preferences.dialog.upcast_ref(),
                            &format!("setting-{}", row.id.key()),
                        )
                        .unwrap()
                        .downcast()
                        .unwrap();
                        let link = native
                            .activatable_widget()
                            .unwrap()
                            .downcast::<gtk::LinkButton>()
                            .unwrap();
                        assert_eq!(native.title().as_str(), row.title);
                        assert_eq!(link.uri().as_str(), url);
                        assert_eq!(link.label().as_deref(), Some(label.as_str()));
                        // Exercise activation without opening the user's browser.
                        let activated = Rc::new(std::cell::Cell::new(false));
                        let seen = activated.clone();
                        let handler = link.connect_activate_link(move |_| {
                            seen.set(true);
                            glib::Propagation::Stop
                        });
                        link.emit_clicked();
                        assert!(activated.get());
                        link.disconnect(handler);
                    }
                }
            }
        }
        w.dispatch(UiAction::CloseSettings);
        pump(250);
    }
    for (theme, id, color) in [
        (Theme::Dark, PreferenceId::DarkBase, "#1C2C3C"),
        (Theme::Light, PreferenceId::LightBase, "#C0B49C"),
    ] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::OpenSettings {
            page: SettingsPage::Appearance,
        });
        pump(300);
        let entry: gtk::Entry = find_named(
            w.preferences.dialog.upcast_ref(),
            &format!("setting-text-{}", id.key()),
        )
        .unwrap()
        .downcast()
        .unwrap();
        entry.grab_focus();
        entry.set_text("invalid");
        entry.emit_activate();
        assert!(state(&w).preferences.error.is_some());
        entry.set_text(color);
        entry.emit_activate();
        pump(250);
        assert!(state(&w).preferences.error.is_none());
        assert_eq!(state(&w).palette.bg.to_string(), color.to_lowercase());
        let suffix = if theme == Theme::Dark {
            "dark"
        } else {
            "light"
        };
        capture_reference(&w, &format!("{dir}/gtk-custom-base-{suffix}.png"), 1.0);
        let row = find_named(
            w.preferences.dialog.upcast_ref(),
            &format!("setting-{}", id.key()),
        )
        .unwrap();
        let controllers = row.observe_controllers();
        let hold = (0..controllers.n_items())
            .filter_map(|i| controllers.item(i).and_downcast::<gtk::GestureLongPress>())
            .find(|c| c.name().as_deref() == Some("preference-context-hold"))
            .unwrap();
        hold.emit_by_name::<()>("pressed", &[&20.0f64, &20.0f64]);
        pump(100);
        let popup: gtk::PopoverMenu =
            find_named(w.preferences.dialog.upcast_ref(), "preference-context-menu")
                .unwrap()
                .downcast()
                .unwrap();
        assert!(
            popup.is_visible(),
            "reset menu must survive the editor losing focus"
        );
        let reset: gtk::Button = find_named(popup.upcast_ref(), "preference-reset")
            .unwrap()
            .downcast()
            .unwrap();
        assert!(reset.is_sensitive());
        let labels = reset.child().unwrap();
        assert_eq!(
            labels
                .last_child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .text(),
            theme.default_base().to_string()
        );
        capture_popover(popup.upcast_ref(), &format!("{dir}/gtk-reset-{suffix}.png"));
        click(&reset);
        assert_eq!(state(&w).palette.bg, theme.default_base());
        hold.emit_by_name::<()>("pressed", &[&20.0f64, &20.0f64]);
        pump(100);
        let reset: gtk::Button = find_named(popup.upcast_ref(), "preference-reset")
            .unwrap()
            .downcast()
            .unwrap();
        assert!(!reset.is_sensitive());
        popup.popdown();
        entry.grab_focus();
        entry.set_text(color);
        entry.emit_activate();
        entry.set_text("");
        assert_eq!(
            state(&w).palette.bg.to_string(),
            color.to_lowercase(),
            "empty draft does not reset yet"
        );
        entry.emit_activate();
        assert_eq!(entry.text(), theme.default_base().to_string());
        assert_eq!(state(&w).palette.bg, theme.default_base());
        entry.set_text(color);
        entry.emit_activate();
        w.dispatch(UiAction::CloseSettings);
        pump(300);
        capture_reference(&w, &format!("{dir}/gtk-custom-workspace-{suffix}.png"), 1.0);
    }
    let mut defaults = state(&w).settings;
    defaults.dark_base = Theme::Dark.default_base();
    defaults.light_base = Theme::Light.default_base();
    w.dispatch(UiAction::RestoreSettings { settings: defaults });
    click(&command(&w, CommandId::KeyboardShortcuts));
    let search: gtk::SearchEntry = find_named(w.preferences.dialog.upcast_ref(), "settings-search")
        .unwrap()
        .downcast()
        .unwrap();
    search.set_text("pressure response");
    pump(300);
    let results = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .preferences()
        .unwrap()
        .search_results;
    assert_eq!(results.len(), 1);
    w.dispatch(UiAction::Preferences {
        action: results[0].action.clone(),
    });
    search.set_text("");
    pump(300);
    let feedback: adw::SwitchRow =
        find_named(w.preferences.dialog.upcast_ref(), "setting-feedback")
            .unwrap()
            .downcast()
            .unwrap();
    feedback.set_active(false);
    assert!(
        !find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-prediction-horizon"
        )
        .unwrap()
        .is_sensitive()
    );
    feedback.set_active(true);
    assert!(
        find_named(
            w.preferences.dialog.upcast_ref(),
            "setting-platform-prediction"
        )
        .is_none()
    );
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Shortcuts,
    });
    let search: gtk::SearchEntry =
        find_named(w.preferences.dialog.upcast_ref(), "shortcuts-search")
            .unwrap()
            .downcast()
            .unwrap();
    search.set_text("z");
    pump(300);
    for command in ["Undo", "Redo", "UndoWorkspace", "RedoWorkspace"] {
        assert!(
            find_named(
                w.preferences.dialog.upcast_ref(),
                &format!("shortcut-command.{command}")
            )
            .unwrap()
            .is_visible()
        );
    }
    search.set_text("");
    pump(300);
    let row: adw::ActionRow =
        find_named(w.preferences.dialog.upcast_ref(), "shortcut-command.Brush")
            .unwrap()
            .downcast()
            .unwrap();
    let binding = find_css(row.upcast_ref(), "dim-label").unwrap();
    assert!(!binding.has_css_class("heading"));
    row.emit_by_name::<()>("activated", &[]);
    pump(200);
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::RemoveShortcut {
            id: CommandId::Brush.shortcut_id(),
            index: 0,
        },
    });
    assert!(binding.has_css_class("heading"));
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::ResetShortcut {
            id: CommandId::Brush.shortcut_id(),
        },
    });
    assert!(!binding.has_css_class("heading"));
    click(
        &find_named(w.window.upcast_ref(), "add-shortcut")
            .unwrap()
            .downcast()
            .unwrap(),
    );
    let capture_dialog = find_named(w.window.upcast_ref(), "shortcut-capture").unwrap();
    let controllers = capture_dialog.observe_controllers();
    let keys = (0..controllers.n_items())
        .find_map(|i| {
            controllers
                .item(i)
                .unwrap()
                .downcast::<gtk::EventControllerKey>()
                .ok()
        })
        .unwrap();
    assert!(keys.emit_by_name::<bool>(
        "key-pressed",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()]
    ));
    keys.emit_by_name::<()>(
        "key-released",
        &[&gdk::Key::e, &0u32, &gdk::ModifierType::empty()],
    );
    assert_eq!(
        state(&w).preferences.capture.unwrap().conflict.as_deref(),
        Some("Eraser")
    );
    capture_reference(&w, &format!("{dir}/gtk-shortcut-conflict.png"), 1.0);
    let confirm: gtk::Button = find_named(w.window.upcast_ref(), "confirm-shortcut")
        .unwrap()
        .downcast()
        .unwrap();
    click(&confirm);
    assert!(state(&w).preferences.capture.is_none());
    assert_eq!(state(&w).settings.shortcuts["command.Brush"][1].key, "e");
    assert!(binding.has_css_class("heading"));
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::CloseShortcutEditor,
    });
    pump(250);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(400);
        assert_eq!(
            adw::StyleManager::for_display(&w.area.display()).is_dark(),
            theme == Theme::Dark
        );
        assert_eq!(
            binding
                .clone()
                .downcast::<gtk::Label>()
                .unwrap()
                .layout()
                .iter()
                .run_readonly()
                .unwrap()
                .item()
                .analysis()
                .font()
                .describe()
                .weight(),
            gtk::pango::Weight::Bold
        );
        capture_reference(
            &w,
            &format!("{dir}/gtk-shortcut-modified-{theme:?}.png"),
            1.0,
        );
    }
    click(
        &find_css(
            &find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap(),
            "close",
        )
        .unwrap()
        .downcast()
        .unwrap(),
    );
    assert!(
        state(&w).requests.is_empty(),
        "host acknowledged the saved snapshot"
    );
    assert!(w.menus.borrow().iter().any(|menu| {
        menu.commands
            .iter()
            .zip(&menu.accelerators)
            .any(|(id, accel)| *id == CommandId::Settings && accel == "<Control>comma")
    }));
    // Explicit override isolates persistence from the user's actual config.
    if std::env::var_os("LAYER_SETTINGS_FILE").is_some() {
        assert_eq!(
            crate::preferences::load().unwrap().unwrap(),
            state(&w).settings
        );
        click(&command(&w, CommandId::NewWindow));
        let next = windows.borrow().last().unwrap().clone();
        assert_eq!(state(&next).settings, state(&w).settings);
        next.window.destroy();
        pump(100);
        w.window.present();
    }
    w.dispatch(UiAction::OpenSettings {
        page: SettingsPage::Canvas,
    });
    w.window.set_default_size(640, 600);
    pump(500);
    capture_reference(&w, &format!("{dir}/gtk-narrow.png"), 1.0);
    w.window.destroy();
    pump(200);
    assert!(windows.borrow().is_empty());
}

#[test]
#[ignore = "native slider feedback: requires a Wayland/Vulkan display"]
fn native_slider_feedback() {
    let app = native_test_app("dev.layer.SliderFeedbackTest");
    let slider = |control: &crate::number_control::NumberControl| {
        control
            .last_child()
            .unwrap()
            .first_child()
            .unwrap()
            .next_sibling()
            .unwrap()
            .downcast::<gtk::Scale>()
            .unwrap()
    };
    for spec in [
        NumericControl::brush_size(),
        NumericControl::percent(),
        NumericControl::pressure(),
    ] {
        let control = crate::number_control::NumberControl::new(spec.clone(), "Value", "");
        let scale = slider(&control);
        let notifications = Rc::new(RefCell::new(Vec::new()));
        control.connect_value_changed({
            let notifications = notifications.clone();
            move |field| {
                let mut values = notifications.borrow_mut();
                values.push(field.value());
                // End a broken feedback loop at an exactly representable value,
                // so the regression fails instead of hanging the test process.
                let echo = if values.len() > 8 {
                    1.0
                } else {
                    field.value() as f32 as f64
                };
                drop(values);
                field.set_value(echo);
            }
        });
        control.set_value(0.6);
        assert!(
            notifications.borrow().is_empty(),
            "model refresh is not a user edit"
        );
        for i in 1..100 {
            notifications.borrow_mut().clear();
            let position = i as f64 / 100.0;
            scale.set_value(position);
            assert!(
                notifications.borrow().len() <= 1,
                "feedback at {position}: {:?}",
                notifications.borrow()
            );
            let expected = spec
                .resolve(0.0, NumericOperation::Position { position })
                .unwrap()
                .value as f32;
            assert_eq!(control.value() as f32, expected);
        }
    }
    // Exercise the real session/refresh path, including fractional f32 echoes,
    // in both directions while allowing GTK's event loop to advance.
    let w = Workspace::new(&app);
    w.window.present();
    pump(600);
    let spec = NumericControl::brush_size();
    let scale = slider(&w.size_number);
    let edits = Rc::new(Cell::new(0));
    w.size_number.connect_value_changed({
        let edits = edits.clone();
        move |_| edits.set(edits.get() + 1)
    });
    for i in (0..=200).chain((0..200).rev()) {
        edits.set(0);
        let position = i as f64 / 200.0;
        scale.emit_by_name::<bool>("change-value", &[&gtk::ScrollType::Jump, &position]);
        let expected = spec
            .resolve(0.0, NumericOperation::Position { position })
            .unwrap()
            .value as f32;
        assert_eq!(state(&w).brush.diameter, expected);
        assert!(edits.get() <= 1, "one user action must not feed back");
        if i % 10 == 0 {
            pump(1);
        }
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "numeric widget editing and review sheet: requires a Wayland display"]
fn native_number_controls() {
    let app = native_test_app("dev.layer.NumberTest");
    gtk::gio::resources_register_include!("layer-icons.gresource").unwrap();
    gtk::IconTheme::for_display(&gdk::Display::default().unwrap())
        .add_resource_path("/dev/layer/icons");
    let window = adw::ApplicationWindow::builder()
        .application(&*app)
        .default_width(560)
        .default_height(360)
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.add_css_class("dock-panel");
    let size =
        crate::number_control::NumberControl::new(NumericControl::brush_size(), "Brush size", "");
    let alpha = crate::number_control::NumberControl::new(NumericControl::percent(), "Opacity", "");
    let small = crate::number_control::NumberControl::new(
        NumericControl::number(0.0, 16.0, 1.0, 0),
        "Small integer",
        "",
    );
    for field in [&size, &alpha, &small] {
        body.append(field);
    }
    size.set_value(32.0);
    alpha.set_value(0.5);
    small.set_value(4.0);
    let narrow = crate::number_control::NumberControl::new(
        NumericControl::percent(),
        "A long slider name that must not wrap",
        "",
    );
    narrow.set_value(0.5);
    let narrow_container = adw::Clamp::builder()
        .maximum_size(176)
        .tightening_threshold(176)
        .child(&narrow)
        .build();
    body.append(&narrow_container);
    let described = crate::number_control::NumberControl::new(
        NumericControl::pressure(),
        "Pressure response",
        "Adjust how pen pressure affects your brush. The value centers against this complete label block.",
    );
    described.set_value(1.0);
    body.append(&described);
    window.set_content(Some(&body));
    window.present();
    pump(300);
    fn descendant<T: IsA<gtk::Widget> + glib::types::StaticType + Clone>(
        w: &impl IsA<gtk::Widget>,
    ) -> T {
        let mut child = w.first_child();
        while let Some(node) = child {
            if let Ok(found) = node.clone().downcast::<T>() {
                return found;
            }
            // Flatten without depending on private GTK node layout.
            let mut queue = vec![node.clone()];
            while let Some(parent) = queue.pop() {
                let mut nested = parent.first_child();
                while let Some(n) = nested {
                    if let Ok(found) = n.clone().downcast::<T>() {
                        return found;
                    }
                    nested = n.next_sibling();
                    queue.push(n);
                }
            }
            child = node.next_sibling();
        }
        panic!("missing widget {}", T::static_type());
    }
    let scale: gtk::Scale = descendant(&size);
    for field in [&size, &narrow, &described] {
        let header = field.first_child().unwrap();
        let labels = header.first_child().unwrap();
        let title = labels
            .first_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        let value = header.last_child().unwrap();
        let lb = labels.compute_bounds(field).unwrap();
        let vb = value.compute_bounds(field).unwrap();
        assert!((lb.y() + lb.height() / 2.0 - vb.y() - vb.height() / 2.0).abs() <= 1.0);
        assert!((vb.x() + vb.width() - field.width() as f32).abs() <= 1.0);
        assert!(!title.wraps());
        assert_eq!(title.ellipsize(), gtk::pango::EllipsizeMode::End);
        assert_eq!(title.tooltip_text().unwrap(), title.text());
        assert_eq!(title.compute_bounds(field).unwrap().x(), 6.0);
        if field == &narrow {
            assert!(title.layout().is_ellipsized());
        }
        assert!(
            vb.width() < 90.0,
            "hidden entry must not reserve width: {vb:?}"
        );
    }
    assert!(
        narrow.height() <= 50,
        "compact row height: {}",
        narrow.height()
    );
    assert_eq!(narrow.width(), 176);
    let track = scale.parent().unwrap();
    let minus = track.first_child().unwrap().compute_bounds(&track).unwrap();
    let plus = track.last_child().unwrap().compute_bounds(&track).unwrap();
    let bar = scale.compute_bounds(&track).unwrap();
    assert_eq!(bar.height(), 24.0);
    assert_eq!(bar.x(), minus.x() + minus.width() + 6.0);
    assert_eq!(bar.x() + bar.width() + 6.0, plus.x());
    assert_eq!(scale.range_rect().width(), scale.width());
    let (start, end) = scale.slider_range();
    assert_eq!(start, end, "compact slider reserves no thumb width");
    let value_label: gtk::Label = descendant(&descendant::<gtk::Button>(&size));
    assert_eq!(value_label.xalign(), 1.0);
    scale.set_value(0.5);
    assert!((size.value() - 32.0).abs() < 0.1);
    let display: gtk::Button = descendant(&size);
    click(&display);
    let entry: gtk::Entry = descendant(&size);
    entry.set_text("85/2");
    entry.emit_activate();
    assert_eq!(size.value(), 42.5);
    click(&display);
    entry.set_text("1/0");
    entry.emit_activate();
    assert!(size.has_css_class("error"));
    assert_eq!(size.value(), 42.5);
    entry.set_text("2049");
    entry.emit_activate();
    assert_eq!(size.value(), 2048.0);
    let spin: gtk::SpinButton = descendant(&small);
    spin.set_text("3*2");
    spin.update();
    assert_eq!(small.value(), 6.0);
    spin.set_text("sqrt(81)");
    spin.update();
    assert_eq!(small.value(), 9.0);
    click(&descendant::<gtk::Button>(&alpha));
    let percent: gtk::Entry = descendant(&alpha);
    percent.set_text("75%");
    percent.emit_activate();
    assert_eq!(alpha.value(), 0.75);
    let dir = "../../artifacts/ui/numeric";
    std::fs::create_dir_all(dir).unwrap();
    for (theme, scheme) in [
        ("dark", adw::ColorScheme::ForceDark),
        ("light", adw::ColorScheme::ForceLight),
    ] {
        app.style_manager().set_color_scheme(scheme);
        if theme == "light" {
            window.add_css_class("light-theme");
        }
        pump(150);
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-{theme}.png"))
            .unwrap();
        let value: gtk::Button = descendant(&narrow);
        click(&value);
        pump(100);
        let input: gtk::Entry = descendant(&narrow);
        assert!(input.width() < 100, "short values use compact editors");
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-edit-{theme}.png"))
            .unwrap();
        input.emit_activate();
    }
    // Preferences use the native entry height, not the compact panel editor.
    body.remove(&described);
    let preferences = gtk::Box::new(gtk::Orientation::Vertical, 12);
    preferences.add_css_class("number-preference");
    preferences.append(&described);
    let standard = gtk::Entry::builder().text("Standard GTK entry").build();
    preferences.append(&standard);
    window.set_content(Some(&preferences));
    for (theme, scheme) in [
        ("dark", adw::ColorScheme::ForceDark),
        ("light", adw::ColorScheme::ForceLight),
    ] {
        app.style_manager().set_color_scheme(scheme);
        if theme == "light" {
            window.add_css_class("light-theme");
        } else {
            window.remove_css_class("light-theme");
        }
        pump(150);
        let value: gtk::Button = descendant(&described);
        assert_eq!(value.height(), standard.height());
        click(&value);
        pump(100);
        let entry: gtk::Entry = descendant(&described);
        assert_eq!(entry.height(), standard.height());
        assert_eq!(entry.height(), 34);
        crate::snapshot_window(&window, 1.0)
            .save_to_png(format!("{dir}/gtk-settings-edit-{theme}.png"))
            .unwrap();
        entry.set_text("sqrt(4)");
        entry.emit_activate();
        assert_eq!(described.value(), 2.0);
    }
    window.destroy();
}

#[test]
#[ignore = "cursor vectors: requires a Wayland/Vulkan display"]
fn native_cursor_vectors() {
    let app = native_test_app("dev.layer.CursorTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1800);
    let theme = gtk::IconTheme::for_display(&w.area.display());
    for icon in ui_catalog().icons {
        for scale in [1, 2] {
            let asset = theme.lookup_icon(
                &format!("layer-{icon}-symbolic"),
                &[],
                16,
                scale,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::FORCE_SYMBOLIC,
            );
            assert!(asset.is_symbolic());
            assert!(
                asset
                    .file()
                    .unwrap()
                    .uri()
                    .starts_with("resource:///dev/layer/icons/"),
                "{icon} must come from the shared bank"
            );
        }
    }
    std::fs::create_dir_all("../../artifacts/ui/cursors").unwrap();
    let scale = w.area.scale_factor() as f32;
    assert_eq!(
        w.area.cursor().and_then(|c| c.name()).as_deref(),
        Some("none")
    );
    let mut report = Vec::new();
    for (preset, label) in [
        (layer_core::DefaultBrushPreset::GPen, "round"),
        (layer_core::DefaultBrushPreset::TexturedFlat, "flat"),
        (layer_core::DefaultBrushPreset::WatercolorWash, "watercolor"),
    ] {
        w.dispatch(UiAction::SelectBrush { id: preset as u32 });
        w.dispatch(UiAction::SetBrushSize { value: 512.0 });
        pump(150);
        w.cursor_input(Some(PenEvent {
            device_id: 123,
            sequence: 1,
            timestamp_ns: 1_000_000_000_000,
            view_revision: state(&w).camera.revision,
            surface_position: Point {
                x: 600.0 * scale,
                y: 460.0 * scale,
            },
            pressure: 0.0,
            tilt_radians: [0.3, 0.6],
            twist_radians: 0.5,
            distance: 0.0,
            phase: PenPhase::Hover,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        }));
        let shape = w
            .gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .canvas_cursor()
            .unwrap();
        assert!(!shape.outline.is_empty());
        assert!(shape.segments.len() > 20);
        assert!(
            shape
                .segments
                .iter()
                .all(|s| s.from.into_iter().chain(s.to).all(f32::is_finite))
        );
        pump(50);
        capture_reference(
            &w,
            &format!("../../artifacts/ui/cursors/gtk-{label}.png"),
            1.0,
        );
        let mut times = Vec::new();
        for _ in 0..1000 {
            let start = Instant::now();
            let shape = w
                .gpu
                .borrow_mut()
                .as_mut()
                .unwrap()
                .session
                .canvas_cursor()
                .unwrap();
            std::hint::black_box(&shape.segments);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        report.push(serde_json::json!({"brush":label,"median_ms":times[500],"p95_ms":times[950],"p99_ms":times[990],"segments":shape.segments.len()}));
        assert!(
            times[990] < 8.333,
            "cursor preparation must fit a 120Hz frame"
        );
    }
    std::fs::write(
        "../../artifacts/ui/cursors/native-preparation.json",
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    w.cursor_input(None);
    assert!(
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .canvas_cursor()
            .is_none()
    );
    w.window.destroy();
}

#[test]
#[ignore = "workspace restore integration: requires a Wayland display"]
fn native_workspace_restore() {
    let app = native_test_app("dev.layer.RestoreTest");
    let source = Workspace::new(&app);
    source.window.present();
    pump(1000);
    source.dispatch(UiAction::MovePanel {
        panel: Panel::Toolbar,
        target: DockTarget::Tab {
            group: 5,
            index: Some(0),
        },
        viewport: [1200.0, 900.0],
    });
    source.dispatch(UiAction::ResizeDock {
        id: 3,
        position: [320.0, 450.0],
        viewport: [1200.0, 900.0],
    });
    source.dispatch(UiAction::Invoke {
        command: CommandId::ZenMode,
    });
    pump(150);
    let saved = serde_json::to_string(&state(&source).workspace).unwrap();
    let geometry = |w: &Workspace| {
        w.groups
            .borrow()
            .iter()
            .map(|g| {
                let b = g
                    .stack
                    .parent()
                    .unwrap()
                    .compute_bounds(&w.surface)
                    .unwrap();
                (
                    g.id,
                    g.panels.clone(),
                    g.stack.visible_child_name(),
                    [b.x(), b.y(), b.width(), b.height()],
                )
            })
            .collect::<Vec<_>>()
    };
    let expected = geometry(&source);
    source.window.destroy();
    drop(source);
    pump(100);
    let fresh = Workspace::new(&app);
    fresh.window.present();
    pump(1000);
    fresh.dispatch(UiAction::RestoreWorkspace {
        workspace: serde_json::from_str(&saved).unwrap(),
    });
    pump(150);
    assert_eq!(
        serde_json::to_string(&state(&fresh).workspace).unwrap(),
        saved
    );
    assert_eq!(geometry(&fresh), expected);
    assert!(command(&fresh, CommandId::ZenMode).has_css_class("selected-tool"));
    assert_eq!(
        fresh
            .groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .tabs
            .len(),
        2
    );
    assert!(
        !find_css(fresh.toolbar.upcast_ref(), "panel-grip")
            .unwrap()
            .is_child_visible()
    );
    assert!(!fresh.status.is_visible());
    fresh.window.destroy();
    pump(100);
}

#[test]
#[ignore = "hardware Wayland benchmark: run separately in release with --ignored --test-threads=1"]
fn native_frame_pacing() {
    use layer_core::DefaultBrushPreset;
    let app = native_test_app("dev.layer.FramePacingTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1500);
    if std::env::var("LAYER_PACING_SELECTION").as_deref() == Ok("1") {
        let points: Vec<_> = (0..=256)
            .map(|i| {
                let a = i as f32 / 256. * std::f32::consts::TAU;
                [1024. + 1000. * a.cos(), 768. + 740. * a.sin()]
            })
            .collect();
        w.dispatch(UiAction::Invoke {
            command: CommandId::Lasso,
        });
        native_pen_path(&w, &points);
    }
    if std::env::var("LAYER_PACING_NAVIGATOR").as_deref() == Ok("1") {
        w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetPanelVisible {
                panel: Panel::Navigator,
                visible: true,
            },
        });
        pump(300);
        assert!(w.navigator_images.texture().is_some());
    }
    if let Ok(mode) = std::env::var("LAYER_PACING_ZEN") {
        assert!(matches!(mode.as_str(), "normal" | "partial"));
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let mut workspace = state(&w).workspace;
        workspace.zen_mode = mode == "partial";
        workspace
            .layout
            .insert_tools(Panel::Toolbar, Some(2), &[ToolbarControl::Divider])
            .unwrap();
        workspace
            .layout
            .insert_tools(Panel::Toolbar, Some(5), &[ToolbarControl::Divider])
            .unwrap();
        workspace
            .layout
            .move_panel(
                viewport,
                Panel::Toolbar,
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: true,
                },
            )
            .unwrap();
        w.dispatch(UiAction::RestoreWorkspace { workspace });
        w.dispatch(UiAction::RestoreSettings {
            settings: Settings::default(),
        });
        pump(300);
    }
    assert!(
        w.gpu.borrow().is_some(),
        "hardware Vulkan canvas must initialize"
    );
    let bounds = w.area.compute_bounds(&w.surface).unwrap();
    assert_eq!(
        [bounds.x(), bounds.y(), bounds.width(), bounds.height()],
        [
            0.0,
            0.0,
            w.surface.width() as f32,
            w.surface.height() as f32
        ]
    );
    let worker_stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    let mut reports = Vec::new();
    let mut sequence = 0;
    for (name, preset) in [
        ("GPen", Some(DefaultBrushPreset::GPen)),
        ("NaturalBlender", Some(DefaultBrushPreset::NaturalBlender)),
        ("WetRound", Some(DefaultBrushPreset::WetRound)),
        ("WatercolorWash", Some(DefaultBrushPreset::WatercolorWash)),
        ("Pan", None),
        ("Hand", None),
    ] {
        if std::env::var("LAYER_PACING_BRUSH").is_ok_and(|s| s != name) {
            continue;
        }
        if let Some(preset) = preset {
            w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            w.dispatch(UiAction::SetBrushSize { value: 384.0 });
        } else if name == "Hand" {
            w.dispatch(UiAction::Invoke {
                command: CommandId::Hand,
            });
        }
        pump(300);
        *worker_stats.lock().unwrap() = Default::default();
        let camera = state(&w).camera;
        let start = Instant::now();
        let mut first = true;
        let mut last_event = None;
        while start.elapsed() < Duration::from_secs(6) {
            let t = start.elapsed().as_secs_f32() * 5.0;
            let event = PenEvent {
                device_id: 1,
                sequence,
                timestamp_ns: glib::monotonic_time() as u64 * 1000,
                view_revision: camera.revision,
                surface_position: Point {
                    x: camera.viewport[0] as f32 * (0.5 + 0.23 * t.cos()),
                    y: camera.viewport[1] as f32 * (0.5 + 0.20 * (t * 1.3).sin()),
                },
                pressure: 0.8,
                tilt_radians: [0.0; 2],
                twist_radians: 0.0,
                distance: 0.0,
                phase: if first {
                    PenPhase::Down
                } else {
                    PenPhase::Move
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            };
            sequence += 1;
            if preset.is_some() {
                if std::env::var("LAYER_PACING_CURSOR").as_deref() != Ok("0") {
                    w.cursor_input(Some(event));
                }
                w.input.send(&w, event);
            } else {
                w.interact(UiInput::Pointer {
                    id: 55,
                    phase: if first {
                        ContactPhase::Down
                    } else {
                        ContactPhase::Move
                    },
                    kind: PointerKind::Mouse,
                    button: if name == "Hand" {
                        PointerButton::Primary
                    } else {
                        PointerButton::Pan
                    },
                    position: [event.surface_position.x, event.surface_position.y],
                });
            }
            first = false;
            last_event = Some(event);
            pump(2);
        }
        if preset.is_some() {
            if std::env::var("LAYER_PACING_NAVIGATOR").as_deref() == Ok("1") {
                assert!(
                    w.navigator_images.updating(),
                    "live preview retains GTK timing"
                );
            }
            w.input.send(
                &w,
                PenEvent {
                    phase: PenPhase::Up,
                    ..last_event.unwrap()
                },
            );
        } else {
            w.interact(UiInput::Pointer {
                id: 55,
                phase: ContactPhase::Up,
                kind: PointerKind::Mouse,
                button: if name == "Hand" {
                    PointerButton::Primary
                } else {
                    PointerButton::Pan
                },
                position: [600.0, 450.0],
            });
        }
        pump(150);
        assert!(
            !w.navigator_images.updating(),
            "idle preview releases GTK timing"
        );
        let stats = worker_stats.lock().unwrap();
        assert!(
            stats.cpu.len() > 100,
            "canvas must schedule independently of GTK painting; worker frames = {}, GPU = {}, presented = {}",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        assert!(
            stats.gpu.len() > 100,
            "hardware timestamps must cover GPU rendering"
        );
        assert!(
            stats.presented.iter().filter(|p| p[3] == 1).count() > 100,
            "measure real child-surface presentation"
        );
        let report = serde_json::json!({
            "brush": name, "viewport": camera.viewport, "brush_size": 384, "stroke_seconds": 6,
            "path": "app-owned Wayland Vulkan subsurface",
            "cursor": std::env::var("LAYER_PACING_CURSOR").as_deref() != Ok("0"),
            "navigator": std::env::var("LAYER_PACING_NAVIGATOR").as_deref() == Ok("1"),
            "input_cpu": stats.input_cpu,
            "input_handler_cpu": stats.input_handler_cpu,
            "frame_handler_cpu": stats.frame_handler_cpu,
            "wake_lateness": stats.wake_lateness,
            "worker_cpu": stats.cpu, "worker_gpu": stats.gpu, "canvas_presentation": stats.presented,
        });
        eprintln!(
            "{name}: {} canvas frames, {} GPU timings, {} presentation feedbacks",
            stats.cpu.len(),
            stats.gpu.len(),
            stats.presented.len()
        );
        reports.push(report);
    }
    let path = std::env::var("LAYER_PACING_REPORT")
        .unwrap_or_else(|_| "/tmp/layer-wayland-pacing.json".into());
    std::fs::write(path, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    if let Ok(path) = std::env::var("LAYER_PACING_CAPTURE") {
        crate::capture(&w, &path);
    }
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_floating_click_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("dev.layer.FloatingClickInputTest");
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let initial = state(&w).workspace;
    let mut step = 0;
    std::fs::write(dir.join("ready"), "ready").unwrap();
    let mut perform = |events: serde_json::Value| {
        std::fs::write(
            dir.join(format!("step-{step}.json")),
            serde_json::to_vec(&events).unwrap(),
        )
        .unwrap();
        let timeout = Instant::now() + Duration::from_secs(4);
        while Instant::now() < timeout && !dir.join(format!("done-{step}")).exists() {
            pump(10);
        }
        assert!(
            dir.join(format!("done-{step}")).exists(),
            "native pointer timed out"
        );
        step += 1;
        pump(250);
    };
    for panel in [Panel::Sizes, Panel::Toolbar] {
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: initial.clone(),
        });
        w.dispatch(UiAction::MovePanel {
            panel,
            viewport,
            target: DockTarget::Float {
                position: [600.0, 250.0],
            },
        });
        pump(250);
        let placement = || {
            w.resolved()
                .groups
                .into_iter()
                .find(|g| g.panels.contains(&panel))
                .unwrap()
        };
        let natural = placement().bounds;
        let corner = [
            natural.x + natural.width + 2.0,
            natural.y + natural.height + 2.0,
        ];
        perform(serde_json::json!([
            {"point": corner}, {"down": true},
            {"point": [corner[0] + 60.0, corner[1] + 40.0]},
            {"point": [corner[0] + 120.0, corner[1] + 80.0]}, {"down": false}
        ]));
        assert_ne!(placement().bounds, natural, "native resize");
        for cycle in 0..4 {
            let g = placement();
            let grip = g.tiles.as_ref().and_then(|t| t.grip).or(g.footer_grip);
            let point = grip.map_or(
                [g.bounds.x + g.bounds.width - 28.0, g.bounds.y + 12.0],
                |b| [g.bounds.x + b.x + 3.0, g.bounds.y + b.y + 3.0],
            );
            perform(serde_json::json!([
                {"point": point}, {"down": true}, {"down": false}, {"down": true}, {"down": false}
            ]));
            let actual = placement();
            capture_reference(
                &w,
                &dir.join(format!("{panel:?}-{cycle}.png")).to_string_lossy(),
                1.0,
            );
            if cycle == 0 {
                assert_eq!(
                    actual.bounds, natural,
                    "first double-click must reset {panel:?}"
                );
            } else if panel == Panel::Toolbar {
                assert_eq!(
                    state(&w).workspace.layout.floating[0].toolbar_layout,
                    [
                        FloatingToolbarLayout::Vertical,
                        FloatingToolbarLayout::Horizontal,
                        FloatingToolbarLayout::Compact
                    ][cycle - 1]
                );
            } else {
                assert_eq!(actual.tabs_visible, cycle % 2 == 1, "panel toggle {cycle}");
            }
        }
    }
    std::fs::write(dir.join("finished"), "done").unwrap();
    pump(100);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_toolbar_drag_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let app = native_test_app("dev.layer.ToolbarDragInputTest");
    let w = Workspace::new(&app);
    w.window.maximize();
    w.window.present();
    pump(1200);
    let target = if std::env::var_os("LAYER_NATIVE_DRAG_TAB").is_some() {
        w.dispatch(UiAction::MovePanel {
            panel: Panel::Toolbar,
            viewport: [w.surface.width() as f32, w.surface.height() as f32],
            target: DockTarget::Tab {
                group: 5,
                index: None,
            },
        });
        pump(150);
        w.groups
            .borrow()
            .iter()
            .find(|g| g.id == 5)
            .unwrap()
            .tabs
            .iter()
            .find(|(p, _)| *p == Panel::Toolbar)
            .unwrap()
            .1
            .clone()
            .upcast::<gtk::Widget>()
    } else {
        find_css(&w.panel_widget(Panel::Toolbar), "panel-grip").unwrap()
    };
    let start = target
        .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
        .unwrap();
    let middle = [
        w.surface.width() as f32 * 0.5,
        w.surface.height() as f32 * 0.5,
    ];
    let mut points = Vec::new();
    let mut from = [start.x(), start.y()];
    for to in [
        [middle[0], 260.0],
        middle,
        [middle[0] + 170.0, 120.0],
        [middle[0] - 100.0, middle[1] + 80.0],
        middle,
    ] {
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            points.push([
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ]);
        }
        from = to;
    }
    std::fs::write(
        dir.join("ready"),
        serde_json::to_vec(&serde_json::json!({
            "start": [start.x(), start.y()], "points": points,
        }))
        .unwrap(),
    )
    .unwrap();
    let timeout = Instant::now() + Duration::from_secs(12);
    let mut floated = false;
    let mut cancelled = false;
    let mut states = Vec::new();
    while Instant::now() < timeout && !dir.join("finished").exists() {
        pump(10);
        let workspace = state(&w).workspace;
        let now_floating = !workspace.layout.floating.is_empty();
        cancelled |= floated && !now_floating;
        floated |= now_floating;
        states.push(workspace.layout.floating);
    }
    pump(200);
    capture_reference(&w, &dir.join("toolbar-drag.png").to_string_lossy(), 1.0);
    std::fs::write(
        dir.join("states.json"),
        serde_json::to_vec(&states).unwrap(),
    )
    .unwrap();
    assert!(dir.join("finished").exists(), "native driver timed out");
    assert!(floated, "real GTK input must tear the ribbon off");
    assert!(
        !cancelled,
        "floating toolbar reverted during a continuous native drag"
    );
    assert_eq!(state(&w).workspace.layout.floating.len(), 1);
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "isolated Mutter remote-input driver required; see native-input benchmark"]
fn native_compositor_input() {
    let report_dir = std::path::PathBuf::from(
        std::env::var("LAYER_NATIVE_INPUT_DIR")
            .expect("run with apps/layer-linux/bench/native-input.js in isolated Mutter"),
    );
    let app = native_test_app("dev.layer.NativeInputTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(1500);
    assert!(w.gpu.borrow().is_some());
    let events = Rc::new(RefCell::new(Vec::<[f64; 5]>::new()));
    let motion = gtk::EventControllerLegacy::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_event(glib::clone!(
        #[strong]
        events,
        move |_, event| {
            if event.event_type() == gdk::EventType::MotionNotify {
                let (x, y) = event.position().unwrap();
                events.borrow_mut().push([
                    glib::monotonic_time() as f64 * 1000.0,
                    event.time() as f64,
                    f64::from(
                        event
                            .modifier_state()
                            .contains(gdk::ModifierType::BUTTON2_MASK),
                    ),
                    x,
                    y,
                ]);
            }
            glib::Propagation::Proceed
        }
    ));
    w.area.add_controller(motion);
    let stats = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .stats
        .clone();
    *stats.lock().unwrap() = Default::default();
    let before = state(&w).camera;
    let loop_ = glib::MainLoop::new(None, false);
    glib::timeout_add_local_once(
        Duration::from_secs(9),
        glib::clone!(
            #[strong]
            loop_,
            move || loop_.quit()
        ),
    );
    std::fs::write(report_dir.join("ready"), b"ready").unwrap();
    loop_.run();
    let stats = stats.lock().unwrap();
    let report = serde_json::json!({
        "events": &*events.borrow(), "input_cpu": stats.input_cpu, "input_handler_cpu": stats.input_handler_cpu,
        "wake_lateness": stats.wake_lateness, "worker_cpu": stats.cpu, "worker_gpu": stats.gpu,
        "frame_handler_cpu": stats.frame_handler_cpu,
        "canvas_presentation": stats.presented,
    });
    std::fs::write(
        report_dir.join("received.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Native delivery: {} pointer events; {} rendered frames",
        events.borrow().len(),
        stats.cpu.len()
    );
    assert_ne!(
        state(&w).camera,
        before,
        "native input must actually pan the canvas"
    );
    assert!(
        events.borrow().iter().filter(|e| e[2] == 1.0).count() > 720,
        "must receive compositor-originated panning events above 120 Hz"
    );
    drop(stats);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "hardware desktop: run this test separately with --ignored --test-threads=1"]
fn native_workspace_controls_docking_and_ink() {
    adw::init().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(&crate::stylesheet());
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().unwrap(),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let app = adw::Application::builder()
        .application_id("dev.layer.CopilotTest")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let w = Workspace::new(&app);
    w.window.present();
    pump(3000);
    assert!(w.gpu.borrow().is_some());
    assert_eq!(state(&w).settings.theme, None);
    assert_eq!(
        state(&w).theme == Theme::Dark,
        adw::StyleManager::default().is_dark()
    );
    w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Dark),
    });
    pump(100);
    assert!(!w.ticking.get(), "idle canvas must stop requesting frames");
    let close = find_css(w.header.upcast_ref(), "close").unwrap();
    let circle = close.first_child().unwrap().compute_bounds(&close).unwrap();
    assert_eq!((circle.width(), circle.height()), (24.0, 24.0));
    let hit = close.compute_bounds(&w.header).unwrap();
    assert!(hit.width() >= 34.0 && hit.height() >= 34.0);
    let circle = close
        .first_child()
        .unwrap()
        .compute_bounds(&w.window)
        .unwrap();
    let right = w.window.width() as f32 - circle.x() - circle.width();
    assert!(
        (10.0..=14.0).contains(&circle.y()) && (10.0..=14.0).contains(&right),
        "native close insets: top={}, right={right}",
        circle.y()
    );
    // Exercise the same native signal callbacks as drawing. Space-pan uses
    // that path instead of a second recognizer competing for left-button input.
    let controllers = w.area.observe_controllers();
    let stylus = (0..controllers.n_items())
        .find_map(|i| controllers.item(i).and_downcast::<gtk::GestureStylus>())
        .unwrap();
    let before = state(&w).camera;
    w.interact(crate::input::key_input(
        gdk::Key::space,
        true,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("down", &[&600.0f64, &450.0f64]);
    stylus.emit_by_name::<()>("motion", &[&640.0f64, &470.0f64]);
    w.interact(crate::input::key_input(
        gdk::Key::space,
        false,
        gdk::ModifierType::empty(),
        false,
        None,
    ));
    stylus.emit_by_name::<()>("motion", &[&660.0f64, &490.0f64]);
    stylus.emit_by_name::<()>("up", &[&660.0f64, &490.0f64]);
    pump(100);
    let dpi = w.area.scale_factor() as f32;
    assert_eq!(
        state(&w).camera.translation,
        [
            before.translation[0] + 60.0 * dpi,
            before.translation[1] + 40.0 * dpi
        ]
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        0
    );
    click(&command(&w, CommandId::FitCanvas));
    let initial = white_pixels(&w);
    assert!(initial > 100_000, "GPU paper should be visibly presented");
    w.dispatch(UiAction::Invoke {
        command: CommandId::Pencil,
    });
    let pencil = w
        .tool_set
        .buttons
        .borrow()
        .iter()
        .find(|(_, b, _)| b.widget_name() == "brush-2")
        .unwrap()
        .clone();
    click(&pencil.1);
    assert_eq!(Some(state(&w).brush.preset), pencil.0.preview);
    let size = w
        .size_buttons
        .borrow()
        .iter()
        .find(|(v, _)| *v == 96.0)
        .unwrap()
        .1
        .clone();
    click(&size);
    assert_eq!(state(&w).brush.diameter, 96.0);
    edit_number(&w.size_number, "84");
    assert_eq!(state(&w).brush.diameter, 84.0);
    assert_eq!(w.size_number.value(), 84.0);
    click(&command(&w, CommandId::AddLayer));
    assert_eq!(state(&w).layers.len(), 3);
    let view = state(&w).camera;
    for i in 0..=36 {
        let t = i as f32 / 36.0;
        let phase = if i == 0 {
            PenPhase::Down
        } else if i == 36 {
            PenPhase::Up
        } else {
            PenPhase::Move
        };
        let event = PenEvent {
            device_id: 1,
            sequence: i + 1,
            timestamp_ns: glib::monotonic_time() as u64 * 1000,
            view_revision: view.revision,
            surface_position: Point {
                x: view.viewport[0] as f32 * (0.2 + 0.6 * t),
                y: view.viewport[1] as f32 * (0.5 + 0.09 * (t * std::f32::consts::TAU).sin()),
            },
            pressure: 0.3 + 0.7 * (t * std::f32::consts::PI).sin(),
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase,
            tool: ToolKind::Pen,
            flags: SampleFlags::PRIMARY,
        };
        w.gpu
            .borrow_mut()
            .as_mut()
            .unwrap()
            .session
            .pen(event)
            .unwrap();
        w.wake();
        pump(15);
    }
    pump(300);
    let after_ink = white_pixels(&w);
    assert!(
        after_ink + 500 < initial,
        "ink must be visibly presented: {initial} -> {after_ink}"
    );
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        1
    );
    assert_stroke_positions(&w, &crate::snapshot(&w));
    // Presentation must follow the same top-left transform as input even
    // after an asymmetric pan/zoom/rotation (a centered fit hides a Y flip).
    let center = view.viewport.map(|v| v as f32 * 0.5);
    let change = w.gpu.borrow_mut().as_mut().unwrap().session.gesture(
        center,
        [center[0] + 30.0, center[1] + 45.0],
        1.1,
        0.35,
    );
    w.changed(change);
    pump(150);
    assert_stroke_positions(&w, &crate::snapshot(&w));
    click(&command(&w, CommandId::FitCanvas));
    click(&command(&w, CommandId::Undo));
    assert!(white_pixels(&w) > after_ink + 500);
    click(&command(&w, CommandId::Redo));
    assert!(white_pixels(&w) < initial - 500);
    let visibility = || {
        find_named(
            &w.layer_panel.root.clone().upcast(),
            &format!("art-layer-{}", state(&w).layers[0].id),
        )
        .unwrap()
        .downcast::<gtk::Box>()
        .unwrap()
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap()
    };
    visibility().emit_clicked();
    pump(100);
    assert!(white_pixels(&w) > after_ink + 500);
    visibility().emit_clicked();
    pump(100);
    assert!(white_pixels(&w) < initial - 500);
    edit_number(&w.layer_panel.opacity, "50%");
    pump(100);
    assert_eq!(state(&w).layers[0].opacity, 0.5);
    edit_number(&w.layer_panel.opacity, "100%");
    pump(100);
    let painted_id = state(&w).layers[0].id;
    click(&command(&w, CommandId::LowerLayer));
    assert_eq!(state(&w).layers[1].id, painted_id);
    click(&command(&w, CommandId::RaiseLayer));
    assert_eq!(state(&w).layers[0].id, painted_id);

    click(&command(&w, CommandId::Settings));
    assert!(state(&w).settings_open);
    assert!(w.preferences.dialog.root().is_some());
    let pressure: crate::number_control::NumberControl =
        find_named(w.preferences.dialog.upcast_ref(), "setting-pressure")
            .unwrap()
            .downcast()
            .unwrap();
    edit_number(&pressure, "1.45");
    assert_eq!(state(&w).settings.pressure_gamma, 1.45);
    w.preferences.dialog.close();
    pump(300);
    assert!(!state(&w).settings_open);
    assert_eq!(state(&w).settings.pressure_gamma, 1.45);
    click(&command(&w, CommandId::Settings));
    edit_number(&pressure, "1.5");
    let close = find_css(
        &find_named(w.preferences.dialog.upcast_ref(), "preferences-content").unwrap(),
        "close",
    )
    .unwrap()
    .downcast()
    .unwrap();
    click(&close);
    pump(300);
    assert_eq!(state(&w).settings.pressure_gamma, 1.5);
    assert!(!state(&w).settings_open);
    pump(300);

    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Sizes,
        target: DockTarget::Edge {
            edge: Edge::Right,
            outer: false,
        },
    });
    pump(100);
    assert_eq!(
        state(&w)
            .workspace
            .layout
            .bands
            .iter()
            .filter(|b| b.edge == Edge::Right)
            .count(),
        2
    );
    let layers = w
        .resolved()
        .groups
        .into_iter()
        .find(|g| g.active == Panel::Layers)
        .unwrap()
        .bounds;
    let hint = w
        .drop_at(
            layers.x + layers.width * 0.5,
            layers.y + layers.height * 0.5,
            DockItem::Panel {
                panel: Panel::Brushes,
            },
        )
        .unwrap();
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Brushes,
        target: hint.target,
    });
    pump(100);
    assert!(
        state(&w)
            .workspace
            .layout
            .resolve(1200.0, 900.0)
            .groups
            .iter()
            .any(|g| g.panels == [Panel::Layers, Panel::Brushes])
    );
    let tab = w
        .groups
        .borrow()
        .iter()
        .flat_map(|g| g.tabs.iter())
        .find(|(p, _)| *p == Panel::Layers)
        .unwrap()
        .1
        .clone();
    click(&tab);
    assert!(
        state(&w)
            .workspace
            .layout
            .resolve(1200.0, 900.0)
            .groups
            .iter()
            .any(|g| g.panels.len() == 2 && g.active == Panel::Layers)
    );
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Sizes,
        target: DockTarget::Tab {
            group: 8,
            index: None,
        },
    });
    pump(100);
    click(&tab);
    let root = w
        .groups
        .borrow()
        .iter()
        .find(|g| g.id == 8)
        .unwrap()
        .stack
        .parent()
        .unwrap();
    let header = find_css(&root, "dock-tabs").unwrap();
    let grip = header.last_child().unwrap();
    let point = grip
        .compute_point(&w.surface, &gtk::graphene::Point::new(10.0, 10.0))
        .unwrap();
    let Some(DragTarget::Dock(item)) = w.drag_target_at([point.x(), point.y()]) else {
        panic!("The group grip must be a workspace drag target");
    };
    assert_eq!(item, DockItem::Group { group: 8 });
    let b = root.compute_bounds(&w.surface).unwrap();
    let hint = w
        .drop_at(
            b.x() + b.width() * 0.5,
            b.y() + b.height() * 0.5,
            DockItem::Panel {
                panel: Panel::Toolbar,
            },
        )
        .unwrap();
    assert_eq!(hint.bounds.width, 3.0);
    assert_eq!(hint.bounds.y, b.y());
    let grip_bounds = grip.compute_bounds(&w.surface).unwrap();
    assert!(hint.bounds.x + hint.bounds.width <= grip_bounds.x());
    *w.drop_hint.borrow_mut() = Some(hint);
    w.surface.queue_draw();
    pump(50);
    crate::capture(&w, "../../artifacts/ui/gtk-tab-insertion.png");
    w.clear_drop();
    let expected = w
        .groups
        .borrow()
        .iter()
        .find(|g| g.id == 8)
        .unwrap()
        .panels
        .clone();
    w.dispatch(item.move_action(
        DockTarget::Edge {
            edge: Edge::Left,
            outer: false,
        },
        [w.surface.width() as f32, w.surface.height() as f32],
    ));
    pump(100);
    assert_eq!(
        w.groups.borrow().iter().find(|g| g.id == 8).unwrap().panels,
        expected
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(150);
    // Dock resizing must retain its native drag handle and GPU session.
    let handle = w
        .surface
        .imp()
        .children
        .borrow()
        .iter()
        .find(|(s, _)| *s == Slot::Divider(3))
        .unwrap()
        .1
        .clone();
    w.dispatch(UiAction::ResizeDock {
        id: 3,
        position: [282.0, 0.0],
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
    });
    pump(100);
    assert!(
        w.surface
            .imp()
            .children
            .borrow()
            .iter()
            .any(|(s, h)| *s == Slot::Divider(3) && *h == handle)
    );
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(100);
    // Repeated allocations retire differently sized display images. The pool
    // must remain bounded, eventually present the latest size, then go idle.
    for position in [250.0, 310.0, 260.0, 320.0, 270.0, 300.0] {
        w.dispatch(UiAction::ResizeDock {
            id: 3,
            position: [position, 0.0],
            viewport: [w.surface.width() as f32, w.surface.height() as f32],
        });
        pump(25);
    }
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(200);
    let scale = w.area.scale_factor() as u32;
    assert_eq!(
        state(&w).camera.viewport,
        [
            w.area.width() as u32 * scale,
            w.area.height() as u32 * scale
        ]
    );
    assert!(!w.ticking.get(), "resize presentation must settle");
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .strokes()
            .count(),
        1
    );
    click(&command(&w, CommandId::FitCanvas));
    w.window.set_visible(false);
    pump(50);
    w.window.present();
    pump(200);
    std::fs::create_dir_all("../../artifacts/ui").unwrap();
    review(&w, "dark");
    assert_eq!(
        command(&w, CommandId::ZenMode).icon_name().as_deref(),
        Some("layer-zen-looking-up-symbolic")
    );
    let toolbar_bounds = w.toolbar.compute_bounds(&w.surface).unwrap();
    w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    pump(60);
    for pair in w.tool_set.buttons.borrow().windows(2).take(2) {
        let a = pair[0].1.compute_bounds(&w.surface).unwrap();
        let b = pair[1].1.compute_bounds(&w.surface).unwrap();
        assert_eq!(b.y() - a.y() - a.height(), 2.0);
    }
    assert_eq!(toolbar_bounds.y(), HEADER_HEIGHT);
    let zen = command(&w, CommandId::ZenMode)
        .compute_bounds(&w.surface)
        .unwrap();
    let below = toolbar_bounds.y() - zen.y() - zen.height();
    assert!(
        (below - zen.y()).abs() <= 2.0,
        "header control margins: above={}, below={below}",
        zen.y()
    );
    assert_eq!(toolbar_bounds.x(), w.resolved().status.x);
    assert_eq!(toolbar_bounds.width(), w.resolved().status.width);
    let mut child = w.toolbar.first_child();
    let mut previous_right = None;
    while let Some(tile) = child {
        if tile.has_css_class("tile-button") {
            assert_eq!((tile.width(), tile.height()), (36, 36));
            let b = tile.compute_bounds(&w.toolbar).unwrap();
            assert_eq!(b.y(), 0.0);
            assert_eq!(b.x(), previous_right.map_or(0.0, |x| x + 2.0));
            previous_right = Some(b.x() + b.width());
        }
        child = tile.next_sibling();
    }
    let grip = w
        .toolbar
        .last_child()
        .unwrap()
        .compute_bounds(&w.toolbar)
        .unwrap();
    assert_eq!(grip.x() + grip.width(), w.toolbar.width() as f32);
    assert_eq!(
        (grip.width(), grip.height()),
        (20.0, w.toolbar.height() as f32)
    );
    assert_eq!(
        grip.y() + grip.height() * 0.5,
        w.toolbar.height() as f32 * 0.5
    );
    for group in w.groups.borrow().iter() {
        for (_, tab) in &group.tabs {
            assert_eq!(tab.compute_bounds(&w.surface).unwrap().height(), TILE_SIZE);
        }
    }
    // Window narrowing wraps the lone ribbon, without persisting its growth.
    w.window.set_default_size(680, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 74);
    let first = w.toolbar.first_child().unwrap();
    let fourth = first
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap();
    let a = first.compute_bounds(&w.toolbar).unwrap();
    let b = fourth.compute_bounds(&w.toolbar).unwrap();
    assert_eq!(a.x(), b.x());
    assert_eq!(b.y() - a.y(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-auto-wrap.png");
    w.window.set_default_size(1200, 900);
    pump(300);
    assert_eq!(w.toolbar.height(), 36);
    // One-column side ribbon, then two columns after cross-axis resizing.
    w.dispatch(UiAction::MovePanel {
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
        panel: Panel::Toolbar,
        target: DockTarget::Edge {
            edge: Edge::Left,
            outer: false,
        },
    });
    pump(100);
    assert_eq!(w.toolbar.width(), 36);
    let grip = w
        .toolbar
        .last_child()
        .unwrap()
        .compute_bounds(&w.toolbar)
        .unwrap();
    assert_eq!(
        (grip.width(), grip.height()),
        (w.toolbar.width() as f32, 20.0)
    );
    assert_eq!(grip.y() + grip.height(), w.toolbar.height() as f32);
    let layout = state(&w).workspace.layout;
    let band = layout.bands.last().unwrap();
    let d = w
        .resolved()
        .dividers
        .into_iter()
        .find(|d| d.id == band.id)
        .unwrap();
    w.dispatch(UiAction::ResizeDock {
        id: band.id,
        position: [d.bounds.x + d.bounds.width * 0.5 + 38.0, d.bounds.y],
        viewport: [w.surface.width() as f32, w.surface.height() as f32],
    });
    pump(100);
    assert_eq!(w.toolbar.width(), 74);
    let first = w.toolbar.first_child().unwrap();
    let second = first
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap()
        .next_sibling()
        .unwrap();
    let a = first.compute_bounds(&w.toolbar).unwrap();
    let b = second.compute_bounds(&w.toolbar).unwrap();
    assert_eq!(a.y(), b.y());
    assert_eq!(b.x() - a.x(), 38.0);
    crate::capture(&w, "../../artifacts/ui/gtk-tools-wrapped.png");
    w.dispatch(UiAction::Invoke {
        command: CommandId::ResetLayout,
    });
    pump(100);
    let camera = state(&w).camera;
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::Edit {
            id: PreferenceId::TotalZen,
            value: layer_ui::PreferenceValue::Bool(true),
        },
    });
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::ZenMode));
    pump(250);
    // Native enter events from reparenting may arrive during the pump. This
    // assertion supplies its own hover location, independently of the desktop cursor.
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    w.update_zen();
    // Capture the settled 180ms fade, not the first frame after hiding chrome.
    pump(200);
    assert!(w.header.has_css_class("zen-hidden"));
    assert_eq!(state(&w).camera, camera);
    assert!(
        !w.ticking.get(),
        "Zen fade must not run the canvas frame loop"
    );
    for (slot, widget) in w.surface.imp().children.borrow().iter() {
        if *slot != Slot::Canvas {
            assert!(!widget.can_target());
        }
    }
    crate::capture(&w, "../../artifacts/ui/gtk-zen.png");
    assert!(w.reveal_chrome_at(24.0, 24.0));
    assert!(!w.header.has_css_class("zen-hidden"));
    assert!(command(&w, CommandId::ZenMode).has_css_class("selected-tool"));
    pump(200);
    crate::capture(&w, "../../artifacts/ui/gtk-zen-controls.png");
    w.chrome_event(ChromeEvent::Motion {
        position: [600.0, 450.0],
    });
    click(&command(&w, CommandId::Settings));
    assert!(
        !w.header.has_css_class("zen-hidden"),
        "settings must pin chrome"
    );
    w.dispatch(UiAction::CloseSettings);
    pump(100);
    click(&command(&w, CommandId::ZenMode));
    assert!(!w.header.has_css_class("zen-hidden"));
    assert_eq!(state(&w).camera.translation, camera.translation);
    click(&command(&w, CommandId::ToggleTheme));
    pump(200);
    assert_eq!(state(&w).theme, Theme::Light);
    review(&w, "light");
    let center = state(&w).camera.viewport.map(|v| v as f32 * 0.5);
    let change = w
        .gpu
        .borrow_mut()
        .as_mut()
        .unwrap()
        .session
        .gesture(center, center, 4.0, 0.0);
    w.changed(change);
    pump(150);
    let texture = crate::snapshot(&w);
    texture
        .save_to_png("../../artifacts/ui/gtk-zoom.png")
        .unwrap();
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    // Between the live menus and centered document title, not on a menu button.
    let offset = (24 * texture.width() as usize + 420) * 4;
    assert!(
        bytes[offset..offset + 3].iter().all(|c| *c > 245),
        "zoomed paper must show through the title bar"
    );
    w.window.destroy();
    pump(100);
    assert!(w.gpu.borrow().is_none());
}

fn review(w: &Workspace, theme: &str) {
    // Save the exact texture that passed the ink assertion, without taking a
    // second scene snapshot between validation and artifact generation.
    let texture = crate::snapshot(w);
    assert_stroke_positions(w, &texture);
    // Sample the stroke itself: light-theme chrome is also almost white, so
    // whole-window white-pixel counts cannot compare across themes.
    let path = format!("../../artifacts/ui/gtk-{theme}.png");
    texture.save_to_png(&path).unwrap();
    assert_stroke_positions(w, &gdk::Texture::from_filename(&path).unwrap());
}

fn assert_stroke_positions(w: &Workspace, texture: &gdk::Texture) {
    let camera = state(w).camera;
    let gpu = w.gpu.borrow();
    let stroke = gpu
        .as_ref()
        .unwrap()
        .session
        .engine()
        .document()
        .strokes()
        .next()
        .unwrap();
    let [a, b, c, d, tx, ty] = camera.document_to_surface();
    let origin = w.area.compute_bounds(&w.window).unwrap();
    let scale = w.area.scale_factor() as f32;
    let mut bytes = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut bytes, texture.width() as usize * 4);
    for fraction in 1..=3 {
        let point = stroke.points[stroke.points.len() * fraction / 4].position;
        let x = (origin.x() + (a * point.x + c * point.y + tx) / scale).round() as usize;
        let y = (origin.y() + (b * point.x + d * point.y + ty) / scale).round() as usize;
        let offset = (y * texture.width() as usize + x) * 4;
        assert!(
            bytes[offset] < 235 && bytes[offset + 1] < 235 && bytes[offset + 2] < 235,
            "committed stroke fraction {fraction}/4 must be visible at its input position: ({x}, {y})"
        );
    }
}

fn find_button(root: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(b) = root.downcast_ref::<gtk::Button>()
        && b.label().as_deref() == Some(label)
    {
        return Some(b.clone());
    }
    let mut child = root.first_child();
    while let Some(w) = child {
        if let Some(button) = find_button(&w, label) {
            return Some(button);
        }
        child = w.next_sibling();
    }
    None
}

fn find_named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_named(&widget, name) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

fn find_css(root: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    if root.has_css_class(class) {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = find_css(&widget, class) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}
