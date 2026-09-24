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
        .panels
        .iter()
        .flat_map(|p| p.tiles())
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
            let vertical = scale
                .clone()
                .downcast::<gtk::Scale>()
                .unwrap()
                .orientation()
                == gtk::Orientation::Vertical;
            let (a, z) = if vertical {
                (
                    [b.x() + b.width() / 2., b.y() + b.height() * 0.75],
                    [b.x() + b.width() / 2., b.y() + b.height() * 0.2],
                )
            } else {
                (
                    [b.x() + b.width() * 0.25, b.y() + b.height() / 2.],
                    [b.x() + b.width() * 0.8, b.y() + b.height() / 2.],
                )
            };
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
        assert_eq!(state(&d.w).workspace, initial);
        // The value cap requires a hold with all three devices.
        let before = state(&d.w).workspace;
        let handle = d.named(&format!("tile-{opacity}"));
        let target = d.named(&format!("tile-{size}"));
        let a = d.point(&handle);
        let b = d.point(&target);
        drag(d, device, a, b, false);
        assert_eq!(
            state(&d.w).workspace,
            before,
            "quick cap movement must not reorder"
        );
        drag(d, device, a, b, true);
        assert_ne!(
            state(&d.w).workspace,
            before,
            "{device}: held component cap"
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
        slider_preview_gestures(&mut d, &["mouse", "touch"]);
        d.capture_canvas(&format!("sketch-{theme:?}.png"));

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
                    panel: brush_panel(&d),
                    style,
                },
            });
            d.w.dispatch(UiAction::MovePanel {
                panel: brush_panel(&d),
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
            d.w.dispatch(UiAction::SetBrushSize { value: 2047.9 });
            pump(80);
            assert!(
                find_named(
                    &d.w.surface.clone().upcast(),
                    &format!("component-value-{size}")
                )
                .is_none()
            );
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
            panel: brush_panel(&d),
            target: DockTarget::Float {
                position: [200., 250.],
            },
            viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
        });
        pump(250);
        assert!(d.named(&format!("component-slider-{size}")).is_mapped());
        d.capture_canvas(&format!("floating-sliders-{theme:?}.png"));

        restore(&d, WorkspacePreset::Photographer);
        let options = component_id(&d, ToolbarControl::TOOL_OPTIONS);
        let top = state(&d.w).workspace.layout.workspace(
            d.w.surface.width() as f32,
            d.w.surface.height() as f32,
            0.,
            0.,
        );
        let canvas = top.work_area;
        let root = d
            .named(&format!("tile-{options}"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
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
            assert_eq!(
                root,
                d.named(&format!("tile-{options}"))
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
            );
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
        let scale = descendant::<gtk::Scale>(&number).unwrap();
        let bounds = scale.compute_bounds(&d.w.window).unwrap();
        drag(
            &mut d,
            "mouse",
            [
                bounds.x() + bounds.width() * 0.2,
                bounds.y() + bounds.height() / 2.,
            ],
            [
                bounds.x() + bounds.width() * 0.7,
                bounds.y() + bounds.height() / 2.,
            ],
            false,
        );
        assert!(
            state(&d.w)
                .tool_settings
                .iter()
                .find(|f| f.id == "tolerance")
                .unwrap()
                .value
                > 0.4
        );
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
        d.click_name("toolbar-segment-selection-mode-1");
        assert!(
            state(&d.w)
                .commands
                .iter()
                .any(|c| c.id == CommandId::SelectionAdd && c.selected)
        );

        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Eyedropper,
        });
        pump(150);
        d.w.dispatch(UiAction::Layer {
            action: LayerAction::Tool {
                tool: LayerCanvasTool::PickVisible,
            },
        });
        d.w.dispatch(UiAction::SetColorSampleSize { width: 1 });
        pump(100);
        d.click_name("toolbar-choice-sample-size");
        d.key(0xff54);
        d.key(0xff0d);
        let view = state(&d.w);
        let selected: Vec<_> = view
            .tool_set
            .subtools
            .iter()
            .filter(|i| i.selected)
            .collect();
        assert_eq!(selected.len(), 2, "sample source and size are independent");
        assert!(
            selected
                .iter()
                .any(|i| matches!(i.action, UiAction::SetColorSampleSize { width: 3 }))
        );
        d.click_name("toolbar-choice-variant");
        d.key(0xff54);
        d.key(0xff0d);
        let view = state(&d.w);
        assert_eq!(view.layer_tools.tool, LayerCanvasTool::PickLayer);
        assert!(
            view.tool_set.subtools.iter().any(
                |i| i.selected && matches!(i.action, UiAction::SetColorSampleSize { width: 3 })
            )
        );
        d.click_name("toolbar-choice-variant");
        d.capture_canvas(&format!("eyedropper-menu-{theme:?}.png"));
        d.key(0xff1b);

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

        // Vertical options expose individual controls as well as the full drawer.
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
        assert!(d.named("toolbar-setting-size").is_mapped());
        assert!(d.named("toolbar-choice-tool").is_mapped());
        let number = d.named("toolbar-setting-size");
        d.click(&find_css(&number, "number-value").unwrap());
        let popup = descendant::<gtk::Popover>(&number).unwrap();
        d.number(&popup.child().unwrap(), "51");
        d.key(0xff1b);
        assert_eq!(state(&d.w).brush.diameter, 51.);
        d.capture_canvas(&format!("vertical-options-{theme:?}.png"));
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
        slider_preview_gestures(&mut d, &["pen"]);
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
    let options = component_id(&d, ToolbarControl::TOOL_OPTIONS);
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
        panel: brush_panel(&d),
        target: DockTarget::Edge {
            edge: Edge::Right,
            outer: false,
        },
        viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
    });
    let group = state(&d.w)
        .workspace
        .layout
        .panel_group(brush_panel(&d))
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
        panel: brush_panel(&d),
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
    d.click_name(&format!("column-icon-{:?}", brush_panel(&d)));
    let body = d.named(&format!("drawer-panel-{:?}", brush_panel(&d)));
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
    d.click_name(&format!("column-icon-{:?}", brush_panel(&d)));
    assert!(state(&d.w).customization.column_drawers.is_empty());
    d.finish();
}

fn brush_panel(d: &Driver) -> Panel {
    state(&d.w)
        .workspace
        .layout
        .panels
        .iter()
        .find(|p| {
            p.tiles()
                .iter()
                .any(|t| t.control == ToolbarControl::BrushSizeSlider)
        })
        .unwrap()
        .id
}

fn descendant<T: IsA<gtk::Widget> + glib::object::IsClass>(root: &gtk::Widget) -> Option<T> {
    if let Ok(widget) = root.clone().downcast::<T>() {
        return Some(widget);
    }
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(found) = descendant(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_options_presentation_input"]
fn native_toolbar_options_presentation_input() {
    let mut d = Driver::new("art.capycanvas.OptionsPresentation");
    restore(&d, WorkspacePreset::Photographer);
    let options = component_id(&d, ToolbarControl::TOOL_OPTIONS);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Ruler,
    });
    pump(150);
    for command in [CommandId::ShowRulers, CommandId::SnapRulers] {
        let tile = d.named(&format!("toolbar-action-{command:?}"));
        assert_eq!(
            [tile.width(), tile.height()],
            [36, 36],
            "action uses toolbar tile dimensions"
        );
    }
    d.capture_canvas("options-action-tiles.png");
    let more = d.named(&format!("tile-{options}"));
    let root = more.parent().unwrap().parent().unwrap();
    let b = root.compute_bounds(&d.w.window).unwrap();
    let a = [b.x() + b.width() - 100., b.y() + b.height() / 2.];
    d.perform(serde_json::json!([{ "point": a, "down": true },{"wait_ms":800},{"down":false}]));
    assert!(
        !d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.has_css_class("panel-context-menu") && p.is_visible()),
        "mouse holds never open a menu"
    );
    d.perform(serde_json::json!([
        {"touch":"down","point":a},{"wait_ms":800},{"touch":"up"}
    ]));
    let menu =
        d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find(|p| p.has_css_class("panel-context-menu") && p.is_visible())
            .expect("empty-space touch hold opens display preferences");
    menu.activate_action("context.item-0-1", None).unwrap();
    pump(120);
    let config = state(&d.w)
        .workspace
        .layout
        .panel(Panel::Commands)
        .unwrap()
        .clone();
    assert!(
        !config
            .tiles()
            .iter()
            .find(|t| t.id == options)
            .unwrap()
            .control
            .options_style()
            .unwrap()
            .text
    );
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetToolOptionsStyle {
            panel: Panel::Commands,
            tile: options,
            style: ToolOptionsStyle {
                text: false,
                sliders: false,
            },
        },
    });
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    pump(150);
    assert!(
        !descendant::<gtk::Scale>(&d.named("toolbar-setting-size"))
            .unwrap()
            .is_mapped()
    );
    d.capture_canvas("options-icon-values.png");
    d.w.dispatch(UiAction::MovePanel {
        panel: Panel::Commands,
        target: DockTarget::Edge {
            edge: Edge::Left,
            outer: false,
        },
        viewport: [1600., 1000.],
    });
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
        d.w.dispatch(UiAction::SetBrushSize { value: 2048. });
        pump(150);
        let number = d.named("toolbar-setting-size");
        assert!(number.is_mapped(), "size visible at {style:?}");
        let button = find_css(&number, "number-value").unwrap();
        assert!(
            (button.compute_bounds(&number).unwrap().width() - style.size()[0]).abs() < 1.,
            "value box fills {style:?} column"
        );
        let image = descendant::<gtk::Image>(&button).unwrap();
        assert!(image.is_mapped());
        let label = find_css(&button, "number-readout")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert_eq!(
            label.text(),
            if style.label_lines() > 0 {
                "2048 px"
            } else {
                "2048"
            }
        );
        assert!(
            label.layout().pixel_size().0 <= label.width(),
            "four digits fit {style:?}"
        );
        let choice = d.named("toolbar-choice-tool");
        let image = descendant::<gtk::Image>(&choice).unwrap();
        let b = image.compute_bounds(&choice).unwrap();
        if matches!(
            style,
            TileStyle::Small | TileStyle::Medium | TileStyle::Large
        ) {
            assert!(
                (b.x() + b.width() / 2. - choice.width() as f32 / 2.).abs() < 2.,
                "centered dropdown icon {style:?}"
            );
        }
        d.capture_canvas(&format!("options-{style:?}.png"));
    }
    d.finish();
}

fn toolbar_grip(d: &Driver, panel: Panel) -> [f32; 2] {
    let r = d.w.resolved();
    let g = r.groups.iter().find(|g| g.active == panel).unwrap();
    let grip = g.tiles.as_ref().unwrap().grip.unwrap();
    let point =
        d.w.surface
            .compute_point(
                &d.w.window,
                &gtk::graphene::Point::new(
                    g.bounds.x + grip.x + grip.width / 2.,
                    g.bounds.y + grip.y + grip.height / 2.,
                ),
            )
            .unwrap();
    [point.x(), point.y()]
}
fn compact_edge_gestures(d: &mut Driver, devices: &[&str]) {
    for &device in devices {
        restore(d, WorkspacePreset::Painter);
        let panel = brush_panel(d);
        let initial = state(&d.w).workspace;
        let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
        let center = (state(&d.w).workspace.layout.header_presentation.height + viewport[1]
            - WORKSPACE_SPACING)
            / 2.;
        for (alignment, y) in [
            (EdgeAlignment::Center, center),
            (EdgeAlignment::Start, 80.),
            (EdgeAlignment::End, viewport[1] - 20.),
        ] {
            let a = toolbar_grip(d, panel);
            // Approach through the broad target and inspect the live preview,
            // instead of teleporting directly into a near-edge target.
            let event = |phase: &str, p: [f32; 2]| match device {
                "touch" => serde_json::json!({"touch":phase,"point":p}),
                "pen" => serde_json::json!({"pen":phase,"point":p}),
                _ => match phase {
                    "down" => serde_json::json!({"point":p,"down":true}),
                    "up" => serde_json::json!({"down":false}),
                    _ => serde_json::json!({"point":p}),
                },
            };
            if device == "pen" {
                d.perform(serde_json::json!([event("move", a)]));
            }
            let mut events = vec![event("down", a)];
            for distance in [140., 70., 38., 28., 20.] {
                events.push(event("move", [viewport[0] - distance, y]));
            }
            d.perform(serde_json::Value::Array(events));
            assert!(
                matches!(d.w.drop_hint.borrow().as_ref().map(|h| &h.target),
                Some(DockTarget::CompactEdge { edge: Edge::Right, alignment: a }) if *a == alignment),
                "{device}: near-edge preview at a usable contact distance"
            );
            d.perform(serde_json::json!([event("up", [viewport[0] - 20., y])]));
            if device == "pen" {
                d.perform(serde_json::json!([{"pen":"leave"}]));
            }
            let view = state(&d.w);
            let band = view
                .workspace
                .layout
                .bands
                .iter()
                .find(|b| b.root.id() == view.workspace.layout.panel_group(panel).unwrap())
                .unwrap();
            assert_eq!(
                (band.edge, band.alignment),
                (Edge::Right, Some(alignment)),
                "{device}: near-edge {alignment:?}"
            );
            let resolved = d.w.resolved();
            let group = resolved.groups.iter().find(|g| g.active == panel).unwrap();
            assert!(
                group.bounds.height < viewport[1] * 0.7,
                "content-sized toolbar"
            );
            if alignment == EdgeAlignment::Center {
                assert!((group.bounds.y + group.bounds.height / 2. - center).abs() < 1.);
            }
            d.capture_canvas(&format!("compact-{device}-{alignment:?}.png"));
        }
        let a = toolbar_grip(d, panel);
        drag(d, device, a, [viewport[0] - 28., center], false);
        let view = state(&d.w);
        assert!(
            view.workspace
                .layout
                .bands
                .iter()
                .any(|b| b.edge == Edge::Right && b.alignment.is_none())
        );
        let full =
            d.w.resolved()
                .groups
                .into_iter()
                .find(|g| g.active == panel)
                .unwrap();
        assert!(
            full.bounds.height > viewport[1] * 0.8,
            "farther from edge keeps full-height target"
        );
        let before = view.workspace;
        let a = toolbar_grip(d, panel);
        drag(d, device, a, [8., center], false);
        let moved = state(&d.w).workspace;
        assert_eq!(
            moved
                .layout
                .bands
                .iter()
                .find(|b| b.root.id() == moved.layout.panel_group(panel).unwrap())
                .unwrap()
                .alignment,
            Some(EdgeAlignment::Center)
        );
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::UndoWorkspace,
        });
        pump(180);
        assert_eq!(state(&d.w).workspace, before);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::RedoWorkspace,
        });
        pump(180);
        assert_eq!(state(&d.w).workspace, moved);
        assert_eq!(
            moved.layout.panel(panel).unwrap().tiles(),
            initial.layout.panel(panel).unwrap().tiles()
        );
        // Dock an independently draggable toolbar at the compact bar's end.
        let mut layout = moved.layout;
        let companion = layout
            .add_toolbar(
                None,
                "Companion",
                &[ToolbarControl::Color, ToolbarControl::Opacity],
            )
            .unwrap();
        layout
            .move_panel(
                viewport,
                companion,
                DockTarget::Float {
                    position: [450., 300.],
                },
            )
            .unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout,
                ..Default::default()
            }),
        });
        pump(250);
        let r = d.w.resolved();
        let b = r.groups.iter().find(|g| g.active == panel).unwrap().bounds;
        let a = toolbar_grip(d, companion);
        drag(
            d,
            device,
            a,
            [b.x + b.width / 2., b.y + b.height + 2.],
            false,
        );
        let r = d.w.resolved();
        let a = r.groups.iter().find(|g| g.active == panel).unwrap().bounds;
        let b = r
            .groups
            .iter()
            .find(|g| g.active == companion)
            .unwrap()
            .bounds;
        assert!(
            (b.y - a.y - a.height - WORKSPACE_SPACING).abs() < 1.,
            "small gap between stacked bars"
        );
        assert!(
            (a.y + b.y + b.height - center * 2.).abs() < 1.,
            "whole stack centers together"
        );
        if device == "mouse" {
            let before = state(&d.w).workspace;
            let grip = toolbar_grip(d, panel);
            d.perform(serde_json::json!([
                {"point":grip,"down":true},{"down":false},
                {"down":true},{"down":false}
            ]));
            assert_eq!(
                state(&d.w).workspace,
                before,
                "double-click keeps compact stack"
            );
        }
        d.capture_canvas(&format!("compact-stack-{device}.png"));
    }
}
#[test]
#[ignore = "private Mutter: --native-test=native_compact_toolbar_edges_input"]
fn native_compact_toolbar_edges_input() {
    let mut d = Driver::new("art.capycanvas.CompactEdges");
    compact_edge_gestures(&mut d, &["mouse", "touch"]);
    d.finish();
}
#[test]
#[ignore = "private Mutter: --native-test=native_compact_toolbar_edges_pen_input --tablet"]
fn native_compact_toolbar_edges_pen_input() {
    let mut d = Driver::new("art.capycanvas.CompactEdgesPen");
    compact_edge_gestures(&mut d, &["pen"]);
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_value_controls_input"]
fn native_toolbar_value_controls_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarValues");
    restore(&d, WorkspacePreset::Painter);
    slider_preview_gestures(&mut d, &["mouse", "touch"]);
    restore(&d, WorkspacePreset::Photographer);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    d.w.dispatch(UiAction::ResetToolSetting { id: "size".into() });
    let default = state(&d.w).brush.diameter;
    d.w.dispatch(UiAction::SetToolSetting {
        id: "size".into(),
        value: 517.,
    });
    pump(100);
    let number = d.named("toolbar-setting-size");
    let label = find_css(&number.parent().unwrap(), "option-label").unwrap();
    let p = d.point(&label);
    d.perform(
        serde_json::json!([{"point":p,"down":true},{"down":false},{"down":true},{"down":false}]),
    );
    assert_eq!(
        state(&d.w).brush.diameter,
        default,
        "label double click resets only this field"
    );
    d.w.dispatch(UiAction::MovePanel {
        panel: Panel::Commands,
        target: DockTarget::Edge {
            edge: Edge::Left,
            outer: true,
        },
        viewport: [1600., 1000.],
    });
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetTileStyle {
            panel: Panel::Commands,
            style: TileStyle::Medium,
        },
    });
    pump(150);
    let number = d.named("toolbar-setting-size");
    let p = d.point(&find_css(&number, "number-value").unwrap());
    drag(&mut d, "touch", p, [p[0], p[1] + 20.], false);
    d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
    let popup = descendant::<gtk::Popover>(&number).expect("vertical value opens slider popover");
    assert!(popup.is_visible());
    assert_eq!(
        find_css(popup.upcast_ref(), "number-title")
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap()
            .text(),
        "Brush size",
        "popup keeps its title after a number drag"
    );
    let scale = descendant::<gtk::Scale>(popup.upcast_ref()).unwrap();
    assert!(
        scale.is_mapped(),
        "popover {:?}: mapped {}, size {}x{}, scale visible {}, child visible {}, size {}x{}",
        popup.widget_name(),
        popup.is_mapped(),
        popup.width(),
        popup.height(),
        scale.is_visible(),
        scale.is_child_visible(),
        scale.width(),
        scale.height()
    );
    capture_popover(
        &popup,
        d.dir
            .join("vertical-options-slider-popover.png")
            .to_str()
            .unwrap(),
    );
    let before = state(&d.w).brush.diameter;
    let center = d.point(scale.upcast_ref());
    let width = scale.width() as f32;
    drag(
        &mut d,
        "touch",
        [center[0] - width * 0.1, center[1]],
        [center[0] + width * 0.15, center[1]],
        false,
    );
    assert_ne!(state(&d.w).brush.diameter, before);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Eraser,
    });
    pump(120);
    assert!(
        !popup.is_visible(),
        "tool changes close stale slider popovers"
    );
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_visible_edges_input"]
fn native_toolbar_visible_edges_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarVisibleEdges");
    restore(&d, WorkspacePreset::Painter);
    // Medium floats are wider than the old fixed 24px pointer target. Bring
    // the visible toolbar edge to the screen while its grip remains inside.
    let panel = brush_panel(&d);
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::SetTileStyle {
            panel,
            style: TileStyle::Medium,
        },
    });
    for edge in [Edge::Right, Edge::Top, Edge::Bottom, Edge::Left] {
        for alignment in [
            EdgeAlignment::Center,
            EdgeAlignment::Start,
            EdgeAlignment::End,
        ] {
            let a = toolbar_grip(&d, panel);
            d.perform(serde_json::json!([{"point":a,"down":true},{"point":[800.,500.]}]));
            let update =
                d.w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .workspace_update();
            let preview = update.drag.unwrap().group.unwrap().bounds;
            let grab = [800. - preview.x, 500. - preview.y];
            let viewport = [d.w.surface.width() as f32, d.w.surface.height() as f32];
            let top = state(&d.w).workspace.layout.header_presentation.height;
            let coordinate = |length: f32| match alignment {
                EdgeAlignment::Start => 80.,
                EdgeAlignment::Center => length / 2.,
                EdgeAlignment::End => length - 80.,
            };
            let p = match edge {
                Edge::Right => [
                    viewport[0] - (preview.width - grab[0]),
                    top + coordinate(viewport[1] - top),
                ],
                Edge::Left => [grab[0], top + coordinate(viewport[1] - top)],
                Edge::Top => [coordinate(viewport[0]), top + grab[1]],
                Edge::Bottom => [
                    coordinate(viewport[0]),
                    viewport[1] - (preview.height - grab[1]),
                ],
            };
            d.perform(serde_json::json!([{"point":p}]));
            assert!(
                matches!(d.w.drop_hint.borrow().as_ref().map(|h| &h.target), Some(DockTarget::CompactEdge { edge: e, alignment: a }) if *e == edge && *a == alignment),
                "visible toolbar touches {edge:?} {alignment:?}: {:?}",
                d.w.drop_hint.borrow()
            );
            d.perform(serde_json::json!([{"down":false}]));
            assert!(
                state(&d.w)
                    .workspace
                    .layout
                    .bands
                    .iter()
                    .any(|b| b.edge == edge && b.alignment == Some(alignment))
            );
        }
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_visual_audit_input"]
fn native_toolbar_visual_audit_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarVisualAudit");
    let styles = [
        TileStyle::Small,
        TileStyle::Medium,
        TileStyle::Large,
        TileStyle::MediumLabeled,
        TileStyle::Labeled,
    ];
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for edge in [Edge::Top, Edge::Left] {
            restore(&d, WorkspacePreset::Painter);
            let panel = brush_panel(&d);
            let size = component_id(&d, ToolbarControl::BrushSizeSlider);
            d.w.dispatch(UiAction::MovePanel {
                panel,
                target: DockTarget::Edge { edge, outer: true },
                viewport: [1600., 1000.],
            });
            for style in styles {
                d.w.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTileStyle { panel, style },
                });
                d.w.dispatch(UiAction::SetBrushSize { value: 2048. });
                pump(180);
                d.capture_canvas(&format!("audit-slider-{theme:?}-{edge:?}-{style:?}.png"));
                let slider = d.named(&format!("component-slider-{size}"));
                assert!(slider.is_mapped());
                let cap = d.named(&format!("tile-{size}"));
                assert!(find_css(&cap, "number-readout").is_none());
                d.click(&cap);
                pump(100);
                let caption = d
                    .named("slider-preview-label")
                    .downcast::<gtk::Label>()
                    .unwrap();
                assert!(caption.text().contains("2048 px"));
                assert!(caption.layout().pixel_size().0 <= caption.width());
                d.key(0xff1b);
            }
        }
        restore(&d, WorkspacePreset::Photographer);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });
        for edge in [Edge::Top, Edge::Left] {
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Commands,
                target: DockTarget::Edge { edge, outer: true },
                viewport: [1600., 1000.],
            });
            let mut prior_font_height = 0;
            for style in styles {
                d.w.dispatch(UiAction::Customize {
                    action: CustomizationAction::SetTileStyle {
                        panel: Panel::Commands,
                        style,
                    },
                });
                d.w.dispatch(UiAction::SetBrushSize { value: 2048. });
                pump(180);
                d.capture_canvas(&format!("audit-options-{theme:?}-{edge:?}-{style:?}.png"));
                let number = d.named("toolbar-setting-size");
                let label = find_css(&number, "number-readout")
                    .unwrap()
                    .downcast::<gtk::Label>()
                    .unwrap();
                let button = find_css(&number, "number-value").unwrap();
                if number.is_mapped() {
                    if edge == Edge::Left {
                        assert_eq!(
                            descendant::<gtk::Image>(&number).unwrap().pixel_size(),
                            16,
                            "form icons leave room for values and labels at {style:?}"
                        );
                        assert!(
                            number.width() as f32 <= style.size()[0],
                            "native number must not grow outside its tile: {style:?}, {}, value {:?}, entry {:?}, label {:?}",
                            number.width(),
                            button.measure(gtk::Orientation::Horizontal, -1),
                            find_css(&number, "number-entry")
                                .unwrap()
                                .measure(gtk::Orientation::Horizontal, -1),
                            label.measure(gtk::Orientation::Horizontal, -1)
                        );
                    }
                    let face = button.first_child().unwrap();
                    let b = button.compute_bounds(&d.w.window).unwrap();
                    let f = face.compute_bounds(&d.w.window).unwrap();
                    assert!(
                        (b.y() + b.height() / 2. - f.y() - f.height() / 2.).abs() < 2.,
                        "numeric face centered: {edge:?} {style:?} {b:?} {f:?}"
                    );
                    assert!(
                        label.layout().pixel_size().0 <= label.width(),
                        "option digits fit {edge:?} {style:?}"
                    );
                    let readout = label.compute_bounds(&number).unwrap();
                    assert!(
                        readout.x() >= 0. && readout.x() + readout.width() <= number.width() as f32,
                        "readout stays inside its field: {edge:?} {style:?} {readout:?} width {}",
                        number.width()
                    );
                    if edge == Edge::Left && style.label_lines() == 0 {
                        let height = label.layout().pixel_size().1;
                        assert!(
                            height >= prior_font_height,
                            "vertical readout retains app typography, except fitting small tiles"
                        );
                        prior_font_height = height;
                    }
                }
            }
        }
        // A compact text edit occupies the same box and a tap on non-focusable
        // toolbar space retires it, just like a tap on another native input.
        restore(&d, WorkspacePreset::Photographer);
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::Brush,
        });
        pump(150);
        let options = component_id(&d, ToolbarControl::TOOL_OPTIONS);
        d.w.dispatch(UiAction::Customize {
            action: CustomizationAction::SetToolOptionsStyle {
                panel: Panel::Commands,
                tile: options,
                style: ToolOptionsStyle {
                    text: false,
                    sliders: true,
                },
            },
        });
        pump(150);
        let number = d.named("toolbar-setting-size");
        let before = number.compute_bounds(&d.w.window).unwrap();
        let button = find_css(&number, "number-value").unwrap();
        let row = number.parent().unwrap();
        let icon = find_css(&row, "option-icon").unwrap();
        let scale = descendant::<gtk::Scale>(&number).unwrap();
        let icon_bounds = icon.compute_bounds(&d.w.window).unwrap();
        let scale_bounds = scale.compute_bounds(&d.w.window).unwrap();
        let value_bounds = button.compute_bounds(&d.w.window).unwrap();
        assert!(icon.is_mapped() && scale.is_mapped());
        assert!(icon_bounds.x() + icon_bounds.width() <= scale_bounds.x());
        assert!(scale_bounds.x() + scale_bounds.width() <= value_bounds.x());
        assert!(!descendant::<gtk::Image>(&button).unwrap().is_mapped());
        d.capture_canvas(&format!("audit-icon-slider-value-{theme:?}.png"));
        let p = d.point(&button);
        d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
        let entry = find_css(&number, "number-entry").unwrap();
        assert!(entry.is_mapped());
        assert!(
            descendant::<gtk::Popover>(&number).is_none_or(|p| !p.is_visible()),
            "a value tap edits inline without a popup"
        );
        let after = number.compute_bounds(&d.w.window).unwrap();
        assert!(
            (before.width() - after.width()).abs() < 1.,
            "editing keeps its footprint"
        );
        d.capture_canvas(&format!("audit-edit-{theme:?}.png"));
        let bar = number.parent().unwrap().parent().unwrap();
        let b = bar.compute_bounds(&d.w.window).unwrap();
        let p = [b.x() + b.width() - 80., b.y() + b.height() / 2.];
        d.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
        assert!(!entry.is_mapped(), "tap outside ends numeric edit");
        assert!(
            !gtk::prelude::GtkWindowExt::focus(&d.w.window)
                .is_some_and(|f| f == entry || f.is_ancestor(&entry))
        );
        d.click_name(&format!("tile-{options}"));
        d.capture_canvas(&format!("audit-drawer-{theme:?}.png"));
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_rows_input"]
fn native_toolbar_rows_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarRows");
    restore(&d, WorkspacePreset::Photographer);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::Brush,
    });
    pump(150);
    for (id, values) in [
        ("size", vec![0.5, 31.9, 32., 2048.]),
        ("opacity", vec![0., 0.125, 0.999, 1.]),
    ] {
        let number = d.named(&format!("toolbar-setting-{id}"));
        let button = find_css(&number, "number-value").unwrap();
        let before = button.compute_bounds(&d.w.window).unwrap();
        for value in values {
            d.w.dispatch(UiAction::SetToolSetting {
                id: id.into(),
                value,
            });
            pump(80);
            let after = button.compute_bounds(&d.w.window).unwrap();
            assert_eq!(before, after, "{id} readout must reserve its numeric range");
            let label = find_css(&number, "number-readout")
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            assert!(
                label.layout().pixel_size().0 <= label.width(),
                "units and digits fit {id}"
            );
        }
    }
    d.capture_canvas("stable-horizontal-values.png");
    for edge in [Edge::Top, Edge::Bottom] {
        for alignment in [
            EdgeAlignment::Start,
            EdgeAlignment::Center,
            EdgeAlignment::End,
        ] {
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Commands,
                target: DockTarget::CompactEdge { edge, alignment },
                viewport: [1600., 1000.],
            });
            pump(100);
            assert!(d.named("toolbar-choice-tool").is_mapped());
            assert!(
                d.named("toolbar-setting-size").is_mapped(),
                "compact {edge:?} {alignment:?} shows inline settings"
            );
        }
    }
    d.capture_canvas("compact-horizontal-options.png");
    d.w.dispatch(UiAction::MovePanel {
        panel: Panel::Commands,
        target: DockTarget::Float {
            position: [320., 180.],
        },
        viewport: [1600., 1000.],
    });
    pump(150);
    let mut view = state(&d.w).workspace;
    let group = view.layout.panel_group(Panel::Commands).unwrap();
    let f = view
        .layout
        .floating
        .iter_mut()
        .find(|f| f.root.id() == group)
        .unwrap();
    f.toolbar_layout = FloatingToolbarLayout::Compact;
    f.width = 3. * 38. - 2.;
    f.height = None;
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(view),
    });
    pump(180);
    for edge in [None, Some(Edge::Left), Some(Edge::Right)] {
        if let Some(edge) = edge {
            let mut view = state(&d.w).workspace;
            view.layout
                .move_panel(
                    [1600., 1000.],
                    Panel::Commands,
                    DockTarget::Edge { edge, outer: true },
                )
                .unwrap();
            let group = view.layout.panel_group(Panel::Commands).unwrap();
            let band = view
                .layout
                .bands
                .iter_mut()
                .find(|b| b.root.id() == group)
                .unwrap();
            band.extent = 3. * 38. - 2. + WORKSPACE_SPACING;
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(view),
            });
            pump(150);
        }
        let view = state(&d.w);
        let resolved = view
            .workspace
            .layout
            .workspace(1600., 1000., HEADER_HEIGHT, STATUS_HEIGHT);
        let group = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Commands)
            .unwrap();
        let cells = &group.tiles.as_ref().unwrap().tiles;
        assert_eq!(cells[0].y, cells[1].y);
        assert!(cells[1].x > cells[0].x);
        let config = view.workspace.layout.panel(Panel::Commands).unwrap();
        for (tile, bounds) in config.tiles().iter().zip(cells) {
            if tile.control == ToolbarControl::Divider || tile.control.options_style().is_some() {
                assert_eq!(bounds.width, group.bounds.width);
            }
        }
        let tool = d.named("toolbar-choice-tool");
        let size = d.named("toolbar-setting-size");
        assert!(tool.is_mapped() && size.is_mapped());
        let a = tool.compute_bounds(&d.w.window).unwrap();
        let b = size.compute_bounds(&d.w.window).unwrap();
        assert!(
            (a.y() - b.y()).abs() < 1. && b.x() > a.x(),
            "options pack across the first row"
        );
        d.click(&find_css(&size, "number-value").unwrap());
        let popup = descendant::<gtk::Popover>(&size).unwrap();
        d.number(&popup.child().unwrap(), "51");
        d.key(0xff1b);
        assert_eq!(state(&d.w).brush.diameter, 51.);
        d.capture_canvas(&format!("toolbox-rows-{edge:?}.png"));
    }
    d.finish();
}

fn select_toolbar_segment(d: &mut Driver, device: &str, index: usize, command: CommandId) {
    let button = d.named(&format!("toolbar-segment-selection-mode-{index}"));
    assert!(button.is_mapped());
    let p = d.point(&button);
    drag(d, device, p, p, false);
    let view = state(&d.w);
    assert!(view.commands.iter().any(|c| c.id == command && c.selected));
    for i in 0..4 {
        let b = d.named(&format!("toolbar-segment-selection-mode-{i}"));
        assert_eq!(
            b.downcast_ref::<gtk::ToggleButton>().unwrap().is_active(),
            i == index
        );
    }
    assert_eq!(
        d.named(&format!("toolbar-segment-selection-mode-{index}")),
        button
    );
    assert!(view.customization.drawer.is_none());
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_segments_input"]
fn native_toolbar_segments_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarSegments");
    for theme in [Theme::Light, Theme::Dark] {
        restore(&d, WorkspacePreset::Photographer);
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let mut workspace = state(&d.w).workspace;
        let removed: Vec<_> = workspace
            .layout
            .panel(Panel::Commands)
            .unwrap()
            .tiles()
            .iter()
            .filter(|t| t.control.options_style().is_none())
            .map(|t| t.id)
            .collect();
        for tile in removed {
            // Removing an item also collapses neighboring dividers.
            if workspace
                .layout
                .panel(Panel::Commands)
                .unwrap()
                .tiles()
                .iter()
                .any(|t| t.id == tile)
            {
                workspace.layout.remove_tool(Panel::Commands, tile).unwrap();
            }
        }
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::RectangleSelect,
        });
        for edge in [Edge::Top, Edge::Left] {
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Commands,
                target: DockTarget::Edge { edge, outer: true },
                viewport: [1600., 1000.],
            });
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
                pump(120);
                let bar = d.named("toolbar-segments-selection-mode");
                assert!(bar.has_css_class("linked") && bar.is_mapped());
                let a = d
                    .named("toolbar-segment-selection-mode-0")
                    .compute_bounds(&d.w.window)
                    .unwrap();
                let b = d
                    .named("toolbar-segment-selection-mode-3")
                    .compute_bounds(&d.w.window)
                    .unwrap();
                if edge == Edge::Top {
                    assert_eq!(a.y(), b.y());
                    assert!((b.x() - a.x() - 3. * style.size()[0]).abs() <= 1.);
                } else {
                    assert_eq!(a.x(), b.x());
                    assert!((b.y() - a.y() - 3. * style.size()[1]).abs() <= 1.);
                }
                select_toolbar_segment(&mut d, "mouse", 1, CommandId::SelectionAdd);
                select_toolbar_segment(&mut d, "touch", 2, CommandId::SelectionSubtract);
                select_toolbar_segment(&mut d, "touch", 2, CommandId::SelectionSubtract);
                d.capture_canvas(&format!("segments-{theme:?}-{edge:?}-{style:?}.png"));
            }
        }
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::AutoSelect,
        });
        pump(120);
        assert!(
            d.named("toolbar-choice-selection-source")
                .is::<gtk::DropDown>()
        );
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_segments_pen_input --tablet"]
fn native_toolbar_segments_pen_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarSegmentsPen");
    restore(&d, WorkspacePreset::Photographer);
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::RectangleSelect,
    });
    pump(150);
    for (i, command) in [
        CommandId::SelectionNew,
        CommandId::SelectionAdd,
        CommandId::SelectionSubtract,
        CommandId::SelectionIntersect,
    ]
    .into_iter()
    .enumerate()
    {
        select_toolbar_segment(&mut d, "pen", i, command);
    }
    d.finish();
}

fn slider_preview_gestures(d: &mut Driver, devices: &[&str]) {
    let id = component_id(d, ToolbarControl::BrushSizeSlider);
    for &device in devices {
        let context = state(&d.w).toolbar_context();
        let stamp =
            d.w.gpu
                .borrow()
                .as_ref()
                .unwrap()
                .session
                .toolbar_stamp(context);
        assert!(
            stamp.is_ok(),
            "{device}: preview assets ready: {:?}",
            stamp.err()
        );
        let scale = d.named(&format!("component-slider-{id}"));
        let p = d.point(&scale);
        drag(d, device, p, p, false);
        pump(100);
        let popup = find_named(&d.w.window.clone().upcast(), "brush-slider-preview")
            .unwrap_or_else(|| {
                d.capture_canvas("missing-slider-preview.png");
                panic!(
                    "{device}: missing preview at {p:?}, active={}, context={context:?}",
                    d.w.window.is_active()
                );
            });
        assert!(popup.is_mapped(), "{device}: tap retains the stamp");
        let saved = state(&d.w).brush.diameter;
        if device == "pen" {
            // The proxy routes pen events only to the first toplevel surface.
            // Activate the popup button through GTK; its native hit path is
            // covered by mouse/touch here and pen on the Wacom hosts.
            d.named("slider-bookmark")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            pump(100);
        } else {
            let button = d.point(&d.named("slider-bookmark"));
            drag(d, device, button, button, false);
        }
        assert_eq!(
            state(&d.w)
                .toolbar_component(ToolbarControl::BrushSizeSlider)
                .unwrap()
                .bookmarks
                .len(),
            1
        );
        d.capture_canvas(&format!("slider-preview-{device}.png"));
        capture_popover(
            &popup.clone().downcast::<gtk::Popover>().unwrap(),
            d.dir
                .join(format!("slider-stamp-{device}.png"))
                .to_str()
                .unwrap(),
        );
        d.w.dispatch(UiAction::SetBrushSize { value: 3. });
        pump(50);
        drag(d, device, p, p, false);
        assert_eq!(
            state(&d.w).brush.diameter,
            saved,
            "{device}: bookmark recalls its exact value"
        );
        if device == "pen" {
            // The proxy routes pen events only to the first toplevel surface.
            // Activate the popup button through GTK; its native hit path is
            // covered by mouse/touch here and pen on the Wacom hosts.
            d.named("slider-bookmark")
                .downcast::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            pump(100);
        } else {
            let button = d.point(&d.named("slider-bookmark"));
            drag(d, device, button, button, false);
        }
        assert!(
            state(&d.w)
                .toolbar_component(ToolbarControl::BrushSizeSlider)
                .unwrap()
                .bookmarks
                .is_empty()
        );
        d.key(0xff1b);
        pump(100);
        assert!(!popup.is_mapped());
        drag(d, device, p, [p[0], p[1] - 20.], false);
        pump(100);
        assert!(
            !find_named(&d.w.window.clone().upcast(), "brush-slider-preview")
                .is_some_and(|w| w.is_mapped()),
            "{device}: dragging dismisses on lift"
        );
    }
}
