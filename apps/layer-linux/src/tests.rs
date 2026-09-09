//! Native control/lifecycle integration on a hardware desktop. Control signals
//! exercise GTK bindings; pen records exercise scheduling and GPU presentation.
//! Physical tablet/touch delivery remains a human test (not faked here).
use super::*;
use layer_core::Point;
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
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

fn command(w: &Workspace, id: CommandId) -> gtk::Button {
    if let Some((_, button)) = w.commands.borrow().iter().find(|(c, _)| *c == id) {
        return button.clone();
    }
    for panel in &state(w).workspace.layout.panels {
        if let Some(tile) = panel
            .tiles()
            .iter()
            .find(|t| t.control.action() == (UiAction::Invoke { command: id }))
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
                .find(|g| g.name().as_deref() == Some("floating-title-reset"))
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
            w.dispatch(UiAction::ResetFloatingSize { group: natural.id });
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
fn native_floating_gestures() {
    let app = native_test_app("dev.layer.FloatingGesturesTest");
    let w = Workspace::new(&app);
    w.window.present();
    pump(700);
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
        assert_eq!(state(&w).workspace, baseline);
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
        assert_eq!(state(&w).workspace, baseline);
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
        .find(|g| g.name().as_deref() == Some("floating-title-reset"))
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
            target: ContextTarget::Panel { panel },
            style: TabStyle::Icon,
        });
        let expected = state(&w).workspace.layout.panel(panel).unwrap().icon();
        assert_eq!(
            tab.icon_name().as_deref(),
            Some(format!("layer-{expected}-symbolic").as_str())
        );
        send(CustomizationAction::SetTabStyle {
            target: ContextTarget::Panel { panel },
            style: TabStyle::Name,
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
        open_context(ContextTarget::Ribbon { panel });
        activate(&menu, "Delete Painting toolbar…");
        assert!(prompt().body().contains("Undo Workspace Change"));
        snapshot(&format!("delete-{theme:?}"));
        confirm_prompt();
        assert!(state(&w).workspace.layout.panel(panel).is_err());
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
        menu.activate_action(
            &menu_action(&menu.menu_model().unwrap(), "Tab with icon").unwrap(),
            None,
        )
        .unwrap();
        pump(100);
        assert_eq!(
            state(&w)
                .workspace
                .layout
                .panel(Panel::Sizes)
                .unwrap()
                .tab_style,
            TabStyle::Icon
        );
        assert_eq!(tab.icon_name().as_deref(), Some("layer-size-symbolic"));
        send(CustomizationAction::SetTabStyle {
            target: ContextTarget::Group { group: 8 },
            style: TabStyle::Name,
        });

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
        assert_eq!(state(&w).workspace.layout.bands, initial.layout.bands);
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
                target: ContextTarget::Panel { panel },
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
        // selection and the independent Hide tab toggle.
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
        assert!(
            tab.icon_name().is_some(),
            "showing a tab preserves its icon choice"
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
            &format!("{dir}/tab-restored-floating-{theme:?}.png"),
            1.0,
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
    find_named(w.preferences.dialog.upcast_ref(), "close-settings")
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
            let done = find_named(w.preferences.dialog.upcast_ref(), "close-settings")
                .unwrap()
                .compute_bounds(&w.window)
                .unwrap();
            capture_reference(&w, &format!("{dir}/gtk-{}-{suffix}.png", page.key()), 1.0);
            assert!(
                sidebar_bounds.y() + sidebar_bounds.height() > done.y() + done.height(),
                "sidebar must extend beside the bottom action bar: sidebar {sidebar_bounds:?}, Done {done:?}"
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
    let row: adw::ActionRow =
        find_named(w.preferences.dialog.upcast_ref(), "shortcut-command.Brush")
            .unwrap()
            .downcast()
            .unwrap();
    row.emit_by_name::<()>("activated", &[]);
    pump(200);
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
    w.dispatch(UiAction::Preferences {
        action: PreferenceAction::CloseShortcutEditor,
    });
    pump(250);
    click(&find_button(w.preferences.dialog.upcast_ref(), "Done").unwrap());
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
    for &icon in ui_catalog().icons {
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
    ] {
        if std::env::var("LAYER_PACING_BRUSH").is_ok_and(|s| s != name) {
            continue;
        }
        if let Some(preset) = preset {
            w.dispatch(UiAction::SelectBrush { id: preset as u32 });
            w.dispatch(UiAction::SetBrushSize { value: 384.0 });
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
                    button: PointerButton::Pan,
                    position: [event.surface_position.x, event.surface_position.y],
                });
            }
            first = false;
            last_event = Some(event);
            pump(2);
        }
        if preset.is_some() {
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
                button: PointerButton::Pan,
                position: [600.0, 450.0],
            });
        }
        pump(150);
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
    let pencil = w
        .brush_buttons
        .borrow()
        .iter()
        .find(|(_, b)| b.widget_name() == "brush-2")
        .unwrap()
        .clone();
    click(&pencil.1);
    assert_eq!(state(&w).brush.preset, pencil.0);
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
        w.layers
            .first_child()
            .unwrap()
            .first_child()
            .unwrap()
            .downcast::<gtk::CheckButton>()
            .unwrap()
    };
    visibility().set_active(false);
    pump(100);
    assert!(white_pixels(&w) > after_ink + 500);
    visibility().set_active(true);
    pump(100);
    assert!(white_pixels(&w) < initial - 500);
    edit_number(&w.layer_opacity, "50%");
    pump(100);
    assert_eq!(state(&w).layers[0].opacity, 0.5);
    edit_number(&w.layer_opacity, "100%");
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
    let done = find_button(w.preferences.dialog.upcast_ref(), "Done").unwrap();
    click(&done);
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
        Some("layer-zen-symbolic")
    );
    let toolbar_bounds = w.toolbar.compute_bounds(&w.surface).unwrap();
    for pair in w.brush_buttons.borrow().windows(2).take(2) {
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
    click(&command(&w, CommandId::TogglePanels));
    assert_eq!(state(&w).camera.translation, camera.translation);
    click(&command(&w, CommandId::TogglePanels));
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
