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
        d.w.dispatch(UiAction::SetBrushSize { value: 20. });
        pump(80);
        let value = d.named(&format!("tile-{size}"));
        let a = d.point(&value);
        drag(d, device, a, [a[0], a[1] - 40.], false);
        assert_eq!(state(&d.w).workspace, initial);
        if device == "mouse" {
            assert_eq!(state(&d.w).brush.diameter, 20.);
        } else {
            assert!(
                state(&d.w).brush.diameter > 20.,
                "{device}: scrub the number upward"
            );
        }
        assert!(
            !find_css(&d.named(&format!("component-value-{size}")), "number-entry")
                .unwrap()
                .is_mapped(),
            "drag is not a text-entry click"
        );
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
        // Exact entry uses the same expression/clamping policy as panel controls.
        let number = d.named(&format!("component-value-{size}"));
        d.number(&number, "85/2");
        assert_eq!(state(&d.w).brush.diameter, 43.);
        d.number(&number, "0");
        assert_eq!(state(&d.w).brush.diameter, 0.5);
        d.number(&number, "24");
        d.number(&number, "1/0");
        assert!(number.has_css_class("error"));
        d.key(0xff1b);
        d.capture_canvas(&format!("sketch-{theme:?}.png"));

        // A tool change cancels an in-flight expression before focus leaves.
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
        assert!(!find_css(&number, "number-entry").unwrap().is_mapped());
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
            let cap = d.named(&format!("tile-{size}"));
            let readout = cap
                .clone()
                .downcast::<gtk::Button>()
                .unwrap()
                .child()
                .unwrap()
                .last_child()
                .unwrap()
                .last_child()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            assert!(
                readout.layout().pixel_size().0 <= readout.width(),
                "full six-character slider value at {style:?}"
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
        d.click_name("toolbar-choice-selection-mode");
        d.key(0xff54); // Down
        d.key(0xff0d); // Enter
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
        d.number(&number, "51");
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
    let number = find_named(&body, &format!("component-value-{size}")).unwrap();
    d.number(&number, "33");
    assert_eq!(state(&d.w).brush.diameter, 33.);
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
    let menu =
        d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .find(|p| p.has_css_class("panel-context-menu") && p.is_visible())
            .expect("empty-space mouse hold opens display preferences");
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
        let label = button
            .first_child()
            .unwrap()
            .last_child()
            .unwrap()
            .last_child()
            .unwrap()
            .downcast::<gtk::Label>()
            .unwrap();
        assert_eq!(label.text(), "2048");
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
