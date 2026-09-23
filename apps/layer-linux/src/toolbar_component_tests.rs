//! Real GTK input on the private Mutter display (including the tablet proxy).
use super::*;

fn restore(d: &Driver, preset: WorkspacePreset) {
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: preset.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    });
    pump(350);
}
fn component_id(d: &Driver, control: ToolbarControl) -> u32 {
    state(&d.w)
        .workspace
        .layout
        .panel(Panel::Commands)
        .unwrap()
        .tiles()
        .iter()
        .find(|t| t.control == control)
        .unwrap()
        .id
}
fn drag(d: &mut Driver, device: &str, a: [f32; 2], b: [f32; 2], held: bool) {
    let down = match device {
        "touch" => serde_json::json!({"touch":"down","point":a}),
        "pen" => serde_json::json!({"pen":"down","point":a}),
        _ => serde_json::json!({"point":a,"down":true}),
    };
    let motion = match device {
        "touch" => serde_json::json!({"touch":"move","point":b}),
        "pen" => serde_json::json!({"pen":"move","point":b}),
        _ => serde_json::json!({"point":b}),
    };
    let up = match device {
        "touch" => serde_json::json!({"touch":"up"}),
        "pen" => serde_json::json!({"pen":"up"}),
        _ => serde_json::json!({"down":false}),
    };
    if device == "pen" {
        d.perform(serde_json::json!([{"pen":"move","point":a}]));
    }
    d.perform(serde_json::Value::Array(vec![
        down,
        serde_json::json!({"wait_ms":if held {800} else {0}}),
        motion,
        up,
    ]));
    if device == "pen" {
        d.perform(serde_json::json!([{"pen":"leave"}]));
    }
}

fn slider_gestures(d: &mut Driver, devices: &[&str]) {
    let size = component_id(d, ToolbarControl::BrushSizeSlider);
    let opacity = component_id(d, ToolbarControl::BrushOpacitySlider);
    let initial = state(&d.w).workspace;
    for &device in devices {
        for (id, action) in [
            (size, UiAction::SetBrushSize { value: 5. }),
            (opacity, UiAction::SetBrushOpacity { value: 0.1 }),
        ] {
            d.w.dispatch(action);
            pump(50);
            let scale = d.named(&format!("component-slider-{id}"));
            let b = scale.compute_bounds(&d.w.window).unwrap();
            let a = [b.x() + b.width() * 0.25, b.y() + b.height() / 2.];
            let z = [b.x() + b.width() * 0.8, a[1]];
            drag(d, device, a, z, false);
            let current = state(&d.w);
            assert_eq!(
                current.workspace, initial,
                "{device}: sliders cannot reorder"
            );
            assert!(
                if id == size {
                    current.brush.diameter > 5.
                } else {
                    current.brush.opacity > 0.1
                },
                "{device}: immediate native slider edit"
            );
            assert_eq!(
                d.named(&format!("component-slider-{id}")),
                scale,
                "retained native slider"
            );
            drag(d, device, z, a, true);
            assert!(
                !d.w.popovers
                    .borrow()
                    .iter()
                    .filter_map(|p| p.upgrade())
                    .any(|p| p.has_css_class("panel-context-menu") && p.is_visible()),
                "{device}: track hold is not a context/reorder gesture"
            );
            assert!(d.w.workspace_drag.borrow().is_none());
        }
        // Component handles reorder immediately with all three devices.
        let before = state(&d.w).workspace;
        let handle = d.named(&format!("component-grip-{opacity}"));
        let target = d.named(&format!("component-grip-{size}"));
        let a = d.point(&handle);
        let b = d.point(&target);
        drag(d, device, a, b, false);
        assert_ne!(
            state(&d.w).workspace,
            before,
            "{device}: immediate component handle"
        );
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(150);
        assert_eq!(state(&d.w).workspace, before, "one-step undo");
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::RedoWorkspace,
        });
        pump(150);
        assert_ne!(state(&d.w).workspace, before);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(150);
    }
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_components_input"]
fn native_toolbar_components_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarComponents");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        restore(&d, WorkspacePreset::Painter);
        let size = component_id(&d, ToolbarControl::BrushSizeSlider);
        slider_gestures(&mut d, &["mouse", "touch"]);
        // Exact entry uses the same expression/clamping policy as panel controls.
        let value = d.named(&format!("tile-{size}"));
        d.click(&value);
        let root = value
            .parent()
            .unwrap()
            .downcast::<crate::workspace::toolbar_components::ComponentBody>()
            .unwrap();
        let p = root.imp().popover.borrow().as_ref().unwrap().clone();
        let number = p.child().unwrap();
        assert!(number.is_mapped(), "exact-value popup opens on click");
        d.number(&number, "85/2");
        assert_eq!(state(&d.w).brush.diameter, 42.5);
        d.number(&number, "0");
        assert_eq!(state(&d.w).brush.diameter, 0.5);
        d.number(&number, "24");
        d.number(&number, "1/0");
        assert_eq!(state(&d.w).brush.diameter, 24.);
        assert!(number.has_css_class("error"));
        d.key(0xff1b); // Cancel invalid text, then dismiss the popup.
        d.key(0xff1b);
        d.capture_canvas(&format!("sketch-{theme:?}.png"));

        // A tool change cancels an in-flight expression before focus leaves.
        d.click(&value);
        d.click(&find_css(&number, "number-value").unwrap());
        find_css(&number, "number-entry")
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap()
            .set_text("517.6");
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Eraser,
        });
        pump(150);
        assert!(!p.is_visible());
        assert_ne!(state(&d.w).brush.diameter, 517.6);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });

        // Every size style supports a vertical slider and a floating toolbar.
        for style in [
            TileStyle::Small,
            TileStyle::Medium,
            TileStyle::Large,
            TileStyle::MediumLabeled,
            TileStyle::Labeled,
        ] {
            d.w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetTileStyle {
                    panel: Panel::Commands,
                    style,
                },
            });
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Commands,
                target: DockTarget::Edge {
                    edge: Edge::Left,
                    outer: false,
                },
                viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
            });
            pump(200);
            let scale = d
                .named(&format!("component-slider-{size}"))
                .downcast::<gtk::Scale>()
                .unwrap();
            assert_eq!(scale.orientation(), gtk::Orientation::Vertical);
            assert!(scale.is_inverted());
            let b = scale.compute_bounds(&d.w.window).unwrap();
            drag(
                &mut d,
                "mouse",
                [b.x() + b.width() / 2., b.y() + b.height() * 0.8],
                [b.x() + b.width() / 2., b.y() + b.height() * 0.2],
                false,
            );
            assert!(state(&d.w).brush.diameter > 24.);
        }
        d.w.dispatch(UiAction::MovePanel {
            panel: Panel::Commands,
            target: DockTarget::Float {
                position: [200., 250.],
            },
            viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
        });
        pump(250);
        assert!(d.named(&format!("component-slider-{size}")).is_mapped());
        d.capture_canvas(&format!("floating-sliders-{theme:?}.png"));

        restore(&d, WorkspacePreset::Photographer);
        let options = component_id(&d, ToolbarControl::ToolOptions);
        let top = state(&d.w).workspace.layout.workspace(
            d.w.surface.width() as f32,
            d.w.surface.height() as f32,
            0.,
            0.,
        );
        let canvas = top.work_area;
        let root = d.named(&format!("tile-{options}")).parent().unwrap();
        assert!(root.width() > 400, "Photo options fill remaining width");
        for command in [
            CommandId::Brush,
            CommandId::Fill,
            CommandId::RectangleSelect,
            CommandId::Gradient,
            CommandId::Move,
        ] {
            d.w.dispatch(UiAction::Invoke { command });
            pump(120);
            assert_eq!(root, d.named(&format!("tile-{options}")).parent().unwrap());
            let now = state(&d.w).workspace.layout.workspace(
                d.w.surface.width() as f32,
                d.w.surface.height() as f32,
                0.,
                0.,
            );
            assert_eq!(now.work_area, canvas, "tool switches do not resize canvas");
        }
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Fill,
        });
        pump(150);
        let number = d.named("toolbar-setting-tolerance");
        assert!(number.is_mapped());
        d.number(&number, "17");
        assert_eq!(
            state(&d.w)
                .tool_settings
                .iter()
                .find(|f| f.id == "tolerance")
                .unwrap()
                .value,
            0.17
        );
        d.capture_canvas(&format!("photo-fill-{theme:?}.png"));
        d.click_name(&format!("tile-{options}"));
        assert!(state(&d.w).customization.drawer.is_some());
        assert!(d.named("drawer-panel-ToolSettings").is_mapped());
        d.capture_canvas(&format!("photo-overflow-{theme:?}.png"));
        d.click_name(&format!("tile-{options}"));
        assert!(state(&d.w).customization.drawer.is_none());

        // Native choices dispatch through the same contextual action binding.
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::RectangleSelect,
        });
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::SelectionNew,
        });
        pump(150);
        d.click_name("toolbar-choice-selection-mode");
        d.key(0xff54); // Down
        d.key(0xff0d); // Enter
        assert!(
            state(&d.w)
                .commands
                .iter()
                .any(|c| c.id == CommandId::SelectionAdd && c.selected)
        );

        // Actual canvas content enables a transform. Completion actions stay
        // at the start of the bar, and both exit the active operation.
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });
        d.w.dispatch(UiAction::SetBrushSize { value: 12. });
        let a = d.point(d.w.area.upcast_ref());
        drag(&mut d, "mouse", a, [a[0] + 60., a[1] + 30.], false);
        for command in [CommandId::CancelTransform, CommandId::ApplyTransform] {
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::ScaleRotate,
            });
            pump(200);
            assert!(state(&d.w).toolbar_context().operation);
            d.click_name(&format!("toolbar-action-{command:?}"));
            assert!(!state(&d.w).toolbar_context().operation);
        }

        // Vertical options are a launcher, not a clipped row of editors.
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });
        d.w.dispatch(UiAction::MovePanel {
            panel: Panel::Commands,
            target: DockTarget::Edge {
                edge: Edge::Left,
                outer: false,
            },
            viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
        });
        pump(200);
        assert!(!d.named("toolbar-setting-size").is_mapped());
        d.click_name(&format!("tile-{options}"));
        assert!(state(&d.w).customization.drawer.is_some());
        assert!(d.named("drawer-panel-ToolSettings").is_mapped());
        d.click_name(&format!("tile-{options}"));
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_components_pen_input --tablet"]
fn native_toolbar_components_pen_input() {
    // The proxy tests real GDK tablet contacts but cannot authorize native
    // popup grabs. Popup/keyboard journeys run separately without the proxy.
    let mut d = Driver::new("art.capycanvas.ToolbarComponentsPen");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        restore(&d, WorkspacePreset::Painter);
        slider_gestures(&mut d, &["pen"]);
    }
    d.finish();
}

#[test]
#[ignore = "680x500 private Mutter: --native-test=native_toolbar_components_narrow_input"]
fn native_toolbar_components_narrow_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarComponentsNarrow");
    restore(&d, WorkspacePreset::Photographer);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Fill,
    });
    pump(200);
    let options = component_id(&d, ToolbarControl::ToolOptions);
    assert!(d.named(&format!("tile-{options}")).is_mapped());
    assert!(
        !d.named("toolbar-setting-tolerance").is_mapped(),
        "narrow options overflow instead of clipping editors"
    );
    d.click_name(&format!("tile-{options}"));
    assert!(d.named("drawer-panel-ToolSettings").is_mapped());
    crate::capture(&d.w, d.dir.join("narrow-options.png").to_str().unwrap());
    d.click_name(&format!("tile-{options}"));
    assert!(state(&d.w).customization.drawer.is_none());
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_components_drawer_input"]
fn native_toolbar_components_drawer_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarComponentsDrawer");
    restore(&d, WorkspacePreset::Painter);
    d.w.dispatch(UiAction::MovePanel {
        panel: Panel::Commands,
        target: DockTarget::Edge {
            edge: Edge::Right,
            outer: false,
        },
        viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
    });
    let group = state(&d.w)
        .workspace
        .layout
        .panel_group(Panel::Commands)
        .unwrap();
    // Standalone toolbars deliberately cannot collapse. A tab group containing
    // ordinary content exercises the supported retained toolbar drawer path.
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetPanelVisible {
            panel: Panel::Color,
            visible: true,
        },
    });
    d.w.dispatch(UiAction::MovePanel {
        panel: Panel::Color,
        target: DockTarget::Tab { group, index: None },
        viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
    });
    d.w.dispatch(UiAction::SelectPanelTab {
        group,
        panel: Panel::Commands,
    });
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetColumnCollapsed {
            group,
            collapsed: true,
        },
    });
    let column = state(&d.w)
        .workspace
        .layout
        .collapsed_column_for_group(group)
        .unwrap();
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetColumnDrawers {
            column,
            drawers: true,
        },
    });
    pump(200);
    let size = component_id(&d, ToolbarControl::BrushSizeSlider);
    d.click_name("column-icon-Commands");
    let body = d.named("drawer-panel-Commands");
    let scale = find_named(&body, &format!("component-slider-{size}")).unwrap();
    assert!(scale.is_mapped());
    for device in ["mouse", "touch"] {
        d.w.dispatch(UiAction::SetBrushSize { value: 4. });
        pump(100);
        let b = scale.compute_bounds(&d.w.window).unwrap();
        let x = b.x() + b.width() / 2.;
        drag(
            &mut d,
            device,
            [x, b.y() + b.height() * 0.8],
            [x, b.y() + b.height() * 0.2],
            false,
        );
        assert!(state(&d.w).brush.diameter > 4.);
        assert!(d.w.workspace_drag.borrow().is_none());
    }
    let value = find_named(&body, &format!("tile-{size}")).unwrap();
    d.click(&value);
    let root = value
        .parent()
        .unwrap()
        .downcast::<crate::workspace::toolbar_components::ComponentBody>()
        .unwrap();
    let popup = root.imp().popover.borrow().as_ref().unwrap().clone();
    d.number(&popup.child().unwrap(), "33");
    assert_eq!(state(&d.w).brush.diameter, 33.);
    d.key(0xff1b);
    d.click_name("column-icon-Commands");
    assert!(state(&d.w).customization.column_drawers.is_empty());
    assert!(!popup.is_visible());
    d.finish();
}
