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

fn control_anchor(d: &Driver, control: ToolbarControl) -> DrawerAnchor {
    let layout = state(&d.w).workspace.layout;
    let (panel, tile) = layout.panels.iter().find_map(|panel| {
        panel.tiles().iter().find(|tile| tile.control == control)
            .map(|tile| (panel.id, tile.id))
    }).unwrap();
    DrawerAnchor::Tile { panel, tile }
}
fn slot_anchor(d: &Driver, slot: ToolSlotId) -> DrawerAnchor {
    control_anchor(d, ToolbarControl::ToolSlot { slot })
}
fn anchor_widget(d: &Driver, anchor: DrawerAnchor) -> gtk::Widget {
    d.named(&match anchor {
        DrawerAnchor::Tile { tile, .. } => format!("tile-{tile}"),
        DrawerAnchor::Header { id } => format!("header-item-{id}"),
        DrawerAnchor::Column { .. } => unreachable!(),
    })
}
fn assert_group_marker(widget: &gtk::Widget, expected: bool) {
    let marker = find_named(widget, "layer-tool-group-symbolic");
    assert_eq!(marker.is_some(), expected, "group marker on {}", widget.widget_name());
    if let Some(marker) = marker {
        let image = marker.downcast::<gtk::Image>().unwrap();
        assert!(image.is_mapped() && !image.can_target());
        assert!(image.paintable().is_some_and(|paintable| paintable.is::<gtk::Svg>()));
    }
}
fn assert_toolbar_markers(d: &Driver) {
    for panel in state(&d.w).workspace.layout.panels {
        for tile in panel.tiles().iter().filter(|tile| !tile.control.is_component()
            && tile.control != ToolbarControl::Divider) {
            assert_group_marker(&d.named(&format!("tile-{}", tile.id)), tile.control.has_variants());
        }
    }
}
fn painted_group_point(d: &Driver, button: &gtk::Widget) -> [f32; 2] {
    let marker = find_named(button, "layer-tool-group-symbolic").unwrap();
    let parent = marker.parent().unwrap();
    let bounds = marker.compute_bounds(&parent).unwrap();
    let snapshot = gtk::Snapshot::new();
    parent.snapshot_child(&marker, &snapshot);
    let texture = marker.native().unwrap().renderer().unwrap()
        .render_texture(snapshot.to_node().unwrap(), Some(&bounds));
    let width = texture.width() as usize;
    let height = texture.height() as usize;
    let mut pixels = vec![0; width * height * 4];
    texture.download(&mut pixels, width * 4);
    let painted = pixels.chunks_exact(4).enumerate().filter(|(_, pixel)| pixel[3] > 8)
        .map(|(i, _)| [i % width, i / width]).collect::<Vec<_>>();
    assert!(!painted.is_empty());
    let right = painted.iter().map(|point| point[0]).max().unwrap() + 1;
    let bottom = painted.iter().map(|point| point[1]).max().unwrap() + 1;
    assert!(width - right >= 6 && height - bottom >= 6,
        "painted marker clearance: right {}, bottom {}", width - right, height - bottom);
    let marker_bounds = marker.compute_bounds(button).unwrap();
    let button_bounds = button.compute_bounds(button).unwrap();
    assert!(button_bounds.x() + button_bounds.width() - marker_bounds.x() - right as f32 >= 6.
        && button_bounds.y() + button_bounds.height() - marker_bounds.y() - bottom as f32 >= 6.);
    let center = painted.iter().fold([0., 0.], |sum, point|
        [sum[0] + point[0] as f32 + 0.5, sum[1] + point[1] as f32 + 0.5]);
    screen_point(&marker, &d.w.window,
        [center[0] / painted.len() as f32 / width as f32,
         center[1] / painted.len() as f32 / height as f32])
}

fn secondary_group_click(d: &mut Driver, widget: &gtk::Widget) {
    let point = d.point(widget);
    d.input.perform(serde_json::json!([
        {"point":point},{"button":273,"down":true},{"button":273,"down":false}
    ]));
}
fn assert_triangle_activation(d: &mut Driver, button: &gtk::Widget, anchor: DrawerAnchor) {
    let corner = painted_group_point(d, button);
    d.input.click(corner);
    assert!(!variant_context(d).is_visible(), "decorative marker follows button activation");
    assert!(state(&d.w).customization.drawer.is_none(), "first click selects the inactive group");
    d.input.click(corner);
    assert!(!variant_context(d).is_visible());
    assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
    d.input.click(corner);
    assert!(state(&d.w).customization.drawer.is_none());
}
fn header_group_variations(d: &mut Driver, theme: Theme) {
    for size in [HeaderSize::Small, HeaderSize::Medium, HeaderSize::Large] {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
        pump(200);
        let control = ToolbarControl::Command { command: CommandId::DrawingBrush };
        let widget = d.named(&d.header_tool(control));
        let button = descendant::<gtk::Button>(&widget).unwrap().upcast::<gtk::Widget>();
        assert_group_marker(&button, true);
        let id = state(&d.w).workspace.layout.header.entries().find(|entry|
            entry.item == HeaderItem::Tool { control }).unwrap().id;
        let anchor = DrawerAnchor::Header { id };
        assert_triangle_activation(d, &button, anchor);
        assert!(ui_session(&d.w).command(CommandId::DrawingBrush).selected);
        d.capture_canvas(&format!("header-group-{size:?}-{theme:?}.png"));
        d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
        pump(150);
        let before = (state(&d.w).layer_tools.tool, state(&d.w).brush.preset);
        secondary_group_click(d, &button);
        assert!(variant_context(d).is_mapped());
        assert_eq!((state(&d.w).layer_tools.tool, state(&d.w).brush.preset), before);
        capture_popover(&variant_context(d), d.input.dir.join(
            format!("header-group-menu-{size:?}-{theme:?}.png")).to_str().unwrap());
        let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
        let index = menu.sections[0].iter().position(|item| matches!(item.action,
            Some(UiAction::ChooseToolVariant { variant: ToolVariant::BrushGroup { group: layer_ui::ToolGroup::Marker }, .. }))).unwrap();
        choose_variant(d, anchor, index);
        assert_eq!(state(&d.w).brush.tool, layer_ui::ToolGroup::Marker.tool());
        assert!(find_named(&widget, "layer-marker-symbolic").is_some());
        assert!(state(&d.w).customization.drawer.is_none());
        assert!(!variant_context(d).is_mapped());
    }
}
fn choose_group(d: &mut Driver, anchor: DrawerAnchor, group: layer_ui::ToolGroup) {
    let widget = anchor_widget(d, anchor);
    let before = (state(&d.w).layer_tools.tool, state(&d.w).brush.preset);
    secondary_group_click(d, &widget);
    assert!(variant_context(d).is_visible());
    assert_eq!((state(&d.w).layer_tools.tool, state(&d.w).brush.preset), before,
        "opening an inactive group's chooser does not activate it");
    let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
    let index = menu.sections.iter().flatten().position(|item|
        item.icon.as_deref() == Some(group.icon())).unwrap();
    choose_variant(d, anchor, index);
    assert_eq!(state(&d.w).brush.tool, group.tool());
    assert!(state(&d.w).customization.drawer.is_none());
    assert!(find_named(&widget, &format!("layer-{}-symbolic", group.icon())).is_some());
}
fn paint_category_variations(d: &mut Driver, theme: Theme) {
    restore(d, WorkspacePreset::Illustrator);
    assert_toolbar_markers(d);
    for (command, group) in [
        (CommandId::Pen, layer_ui::ToolGroup::Marker),
        (CommandId::Pencil, layer_ui::ToolGroup::Pastel),
        (CommandId::Brush, layer_ui::ToolGroup::Watercolor),
        (CommandId::Brush, layer_ui::ToolGroup::Oil),
        (CommandId::Airbrush, layer_ui::ToolGroup::Spray),
    ] {
        d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
        pump(150);
        let anchor = control_anchor(d, ToolbarControl::Command { command });
        let widget = anchor_widget(d, anchor);
        let image = descendant::<gtk::Image>(&widget).unwrap();
        choose_group(d, anchor, group);
        let preset = state(&d.w).brush.preset;
        d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
        pump(150);
        assert_eq!(descendant::<gtk::Image>(&widget).unwrap(), image);
        assert!(find_named(&widget, &format!("layer-{}-symbolic", group.icon())).is_some(),
            "an inactive category remembers its medium");
        d.click(&widget);
        assert_eq!(state(&d.w).brush.preset, preset);
        assert!(state(&d.w).customization.drawer.is_none(), "first click restores the medium");
        d.capture_canvas(&format!("paint-{group:?}-{theme:?}.png"));
    }
    for command in [CommandId::Eraser, CommandId::Decoration, CommandId::Liquify] {
        let anchor = control_anchor(d, ToolbarControl::Command { command });
        let widget = anchor_widget(d, anchor);
        assert_group_marker(&widget, true);
        secondary_group_click(d, &widget);
        assert!(variant_context(d).is_visible());
        let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
        assert!(!menu.sections[0].is_empty());
        assert!(menu.sections[0].iter().all(|item| matches!(item.action,
            Some(UiAction::ChooseToolVariant { variant: ToolVariant::BrushPreset { .. }, .. }))));
        choose_variant(d, anchor, 0);
        assert!(ui_session(&d.w).command(command).selected);
    }
    let DrawerAnchor::Tile { panel, .. } = control_anchor(d,
        ToolbarControl::Command { command: CommandId::Pen }) else { unreachable!() };
    let pinned = ToolbarControl::Brush { id: state(&d.w).brush.preset };
    for action in [CustomizationAction::InsertTools { panel, before: None },
        CustomizationAction::PickerSelect { control: pinned, selected: true },
        CustomizationAction::ConfirmTools] {
        d.w.dispatch(UiAction::Customize { action });
    }
    pump(250);
    let anchor = control_anchor(d, pinned);
    assert_group_marker(&anchor_widget(d, anchor), false);
    assert!(ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).is_err());
}
fn paint_slot_variations(d: &mut Driver, theme: Theme) {
    restore(d, WorkspacePreset::Illustrator);
    for (slot, commands) in [
        (ToolSlotId::ManualSelection, vec![CommandId::Lasso, CommandId::RectangleSelect,
            CommandId::EllipseSelect, CommandId::PolygonSelect, CommandId::SelectionBrush]),
        (ToolSlotId::AutomaticSelection, vec![CommandId::AutoSelect, CommandId::ColorSelect]),
        (ToolSlotId::Fill, vec![CommandId::Fill, CommandId::LassoFill]),
        (ToolSlotId::Blend, vec![CommandId::Blend, CommandId::Clone]),
    ] {
        let anchor = slot_anchor(d, slot);
        let widget = anchor_widget(d, anchor);
        secondary_group_click(d, &widget);
        choose_variant(d, anchor, 0);
        let view = state(&d.w).tool_set;
        let actual = view.groups.iter().chain(&view.subtools).filter_map(|item| match item.action {
            UiAction::ChooseToolVariant { variant, anchor: origin } if origin == anchor => Some(variant.command()),
            _ => None,
        }).collect::<Vec<_>>();
        assert_eq!(actual.len(), commands.len());
        assert!(commands.iter().all(|command| actual.contains(command)
            || slot == ToolSlotId::Fill && *command == CommandId::LassoFill && actual.contains(&CommandId::EncloseFill)));
        let tools = d.w.panel_widget(Panel::Brushes);
        for command in &commands {
            let label = ui_session(&d.w).command(*command).label;
            assert!(mapped_label(&tools, &label).is_some(), "native Tool Set contains {command:?}");
        }
        let last = commands.last().unwrap();
        let label = ui_session(&d.w).command(*last).label;
        d.click(&mapped_label(&tools, &label).unwrap());
        assert!(ui_session(&d.w).command(*last).selected
            || slot == ToolSlotId::Fill && ui_session(&d.w).command(CommandId::EncloseFill).selected);
        if slot == ToolSlotId::Fill {
            assert_eq!(state(&d.w).tool_set.subtools.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(),
                ["Lasso fill", "Enclose and Fill"]);
            let ordinary = widgets(&tools).find(|widget| widget.has_css_class("brush-choice")
                && mapped_label(widget, "Lasso fill").is_some()).unwrap();
            d.click(&ordinary);
            assert!(ui_session(&d.w).command(CommandId::LassoFill).selected);
            d.click(&mapped_label(&tools, "Enclose and Fill").unwrap());
            assert!(ui_session(&d.w).command(CommandId::EncloseFill).selected);
            let groups = widgets(&tools).find(|widget| widget.has_css_class("tool-groups")).unwrap();
            d.click(&mapped_label(&groups, "Fill").unwrap());
            assert!(ui_session(&d.w).command(CommandId::Fill).selected);
            d.click(&mapped_label(&groups, "Lasso fill").unwrap());
            assert!(ui_session(&d.w).command(CommandId::EncloseFill).selected,
                "the Lasso Fill category remembers its Enclose subtool");
            assert_eq!(state(&d.w).tool_set.groups.len(), 2);
        }
        d.capture_canvas(&format!("paint-selection-{slot:?}-{theme:?}.png"));
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_tool_set_category_input"]
fn native_tool_set_category_input() {
    let mut d = Driver::new("art.capycanvas.ToolSetCategory");
    restore(&d, WorkspacePreset::Illustrator);
    for floating in [false, true] {
        if floating {
            d.w.dispatch(UiAction::MovePanel {
                panel: Panel::Brushes,
                target: DockTarget::Float { position: [400., 200.] },
                viewport: [1600., 1000.],
            });
        }
        for theme in [Theme::Light, Theme::Dark] {
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            for command in [CommandId::Brush, CommandId::Fill, CommandId::Hand] {
                d.w.dispatch(UiAction::Invoke { command });
                pump(250);
                let buttons = d.w.tool_set.group_buttons.borrow().clone();
                assert!(!buttons.is_empty());
                for button in buttons {
                    let cell = button.parent().unwrap();
                    let bounds = button.compute_bounds(&cell).unwrap();
                    assert!(bounds.x().abs() < 1. && (bounds.width() - cell.width() as f32).abs() < 1.,
                        "category fills its cell: {command:?}/{theme:?}/{floating}: {bounds:?}, cell {}", cell.width());
                    let point = screen_point(&cell, &d.w.window, [0.85, 0.5]);
                    let hit = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
                    assert!(hit == button || hit.is_ancestor(&button), "category edge belongs to its button");
                    d.input.perform(serde_json::json!([{"point": point}]));
                    d.capture_canvas(&format!("tool-set-hover-{command:?}-{theme:?}-{floating}.png"));
                    assert!(button.state_flags().contains(gtk::StateFlags::PRELIGHT));
                    d.input.click(point);
                    assert!(button.has_css_class("selected-tool"));
                }
                d.capture_canvas(&format!("tool-set-{command:?}-{theme:?}-{floating}.png"));
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_panel_preview_input"]
fn native_panel_preview_input() {
    let mut d = Driver::new("art.capycanvas.PanelPreview");
    for panel in [Panel::Brushes, Panel::Adjustments] {
        restore(&d, WorkspacePreset::Illustrator);
        d.w.dispatch(UiAction::Invoke { command: CommandId::Brush });
        if panel == Panel::Adjustments {
            d.w.dispatch(UiAction::FilterPicker { action: layer_ui::FilterPickerAction::Category { category: Some("tone".into()) } });
        }
        d.w.dispatch(UiAction::MovePanel {
            panel, target: DockTarget::Float { position: [400., 180.] }, viewport: [1600., 1000.],
        });
        pump(300);
        let preset = state(&d.w).tool_set.subtools[0].preview.unwrap();
        let name = if panel == Panel::Brushes { format!("brush-{preset}") } else { "adjustment-brightness_contrast".into() };
        for theme in [Theme::Light, Theme::Dark] {
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            let button = d.named(&name).downcast::<gtk::Button>().unwrap();
            let body = button.child().unwrap();
            let preview = body.first_child().unwrap();
            let caption = body.last_child().unwrap();
            let mut full_height = 0;
            for height in [700., 360., 700.] {
                let layout = state(&d.w).workspace.layout;
                let group = layout.panel_group(panel).unwrap();
                let bounds = layout.workspace(1600., 1000., layer_ui::HEADER_HEIGHT, layer_ui::STATUS_HEIGHT)
                    .groups.into_iter().find(|placement| placement.id == group).unwrap().bounds;
                let corner = [bounds.x + bounds.width, bounds.y + bounds.height];
                for (phase, position) in [(ContactPhase::Down, corner), (ContactPhase::Up, [corner[0], bounds.y + height])] {
                    d.w.dispatch(UiAction::ResizeFloating { group, edge: layer_ui::ResizeEdge::BottomRight, phase, position, viewport: [1600., 1000.] });
                }
                pump(250);
                let image = preview.compute_bounds(&body).unwrap();
                let text = caption.compute_bounds(&body).unwrap();
                assert!((40..=41).contains(&preview.height()), "Compression preserves the preview");
                assert!(text.y() >= 0. && text.y() + text.height() <= body.height() as f32 + 1.);
                if height == 360. {
                    eprintln!("{panel:?} {theme:?} row height: {full_height} -> {}", button.height());
                    assert!(button.height() as f32 <= full_height as f32 * 0.8,
                        "Preview rows compact by about a quarter: {full_height} -> {}", button.height());
                    assert!(text.y() < image.y() + image.height(), "Compact captions share preview space");
                } else {
                    if full_height == 0 { full_height = button.height(); }
                    assert_eq!(button.height(), full_height, "Rows regain their original full height");
                    assert!(text.y() >= image.y() + image.height() - 1., "Full captions sit below the preview");
                }
                assert_eq!(button.upcast_ref::<gtk::Widget>(), &d.named(&name), "Resizing retains the button");
                d.capture_canvas(&format!("preview-{panel:?}-{theme:?}-{height}.png"));
                d.click(button.upcast_ref());
                if panel == Panel::Brushes { assert_eq!(state(&d.w).brush.preset, preset); }
                else { assert_eq!(state(&d.w).filter_picker.selected.as_deref(), Some("brightness_contrast")); }
            }
        }
    }
    d.finish();
}

fn sketch_group_variations(d: &mut Driver, theme: Theme) {
    restore(d, WorkspacePreset::Painter);
    for (command, group) in [
        (CommandId::DrawingBrush, layer_ui::ToolGroup::Pastel),
        (CommandId::Sculpt, layer_ui::ToolGroup::Liquify),
    ] {
        d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
        pump(150);
        let id = state(&d.w).workspace.layout.header.entries().find(|entry|
            entry.item == HeaderItem::Tool { control: ToolbarControl::Command { command } }).unwrap().id;
        let anchor = DrawerAnchor::Header { id };
        let widget = anchor_widget(d, anchor);
        assert_group_marker(&widget, true);
        choose_group(d, anchor, group);
        d.click(&widget);
        let drawer = state(&d.w).customization.drawer.unwrap();
        let sets_panel = if command == CommandId::DrawingBrush { Panel::BrushSets } else { Panel::SculptSets };
        assert_eq!(drawer.columns, [vec![sets_panel], vec![Panel::Tools], vec![Panel::ToolSettings]]);
        let sets = d.named(&format!("drawer-panel-{sets_panel:?}"));
        let tools = d.named("drawer-panel-Tools");
        let settings = d.named("drawer-panel-ToolSettings");
        assert!(sets.is_mapped() && tools.is_mapped() && settings.is_mapped());
        assert!(sets.width() < tools.width() && tools.width() < settings.width());
        let preset = state(&d.w).tool_panels.tools.subtools.last().unwrap().preview.unwrap();
        d.click(&find_named(&tools, &format!("brush-{preset}")).unwrap());
        assert_eq!(state(&d.w).brush.preset, preset);
        assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
        d.capture_canvas(&format!("sketch-{command:?}-{theme:?}.png"));
        d.click(&widget);
        assert!(state(&d.w).customization.drawer.is_none());
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::Brush });
    pump(150);
    let control = ToolbarControl::Command { command: CommandId::Select };
    let id = state(&d.w).workspace.layout.header.entries().find(|entry|
        entry.item == HeaderItem::Tool { control }).unwrap().id;
    let anchor = DrawerAnchor::Header { id };
    let widget = anchor_widget(d, anchor);
    assert_group_marker(&widget, true);
    let before = (state(&d.w).layer_tools.tool, state(&d.w).brush.preset);
    secondary_group_click(d, &widget);
    assert_eq!((state(&d.w).layer_tools.tool, state(&d.w).brush.preset), before);
    let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
    let index = menu.sections[0].iter().position(|item| matches!(item.action,
        Some(UiAction::ChooseToolVariant { variant: ToolVariant::Command { command: CommandId::RectangleSelect }, .. }))).unwrap();
    choose_variant(d, anchor, index);
    d.header_icon(CommandId::Select, "rectangle-select");
    d.click(&widget);
    let drawer = state(&d.w).customization.drawer.unwrap();
    assert_eq!(drawer.columns, [vec![Panel::Tools], vec![Panel::ToolSettings]]);
    let tools = d.named("drawer-panel-Tools");
    assert_eq!(state(&d.w).tool_panels.tools.subtools.len(), SelectionTool::ALL.len());
    for item in state(&d.w).tool_panels.tools.subtools {
        assert!(mapped_label(&tools, &item.label).is_some());
    }
    d.capture_canvas(&format!("sketch-Select-{theme:?}.png"));
    d.click(&widget);
    assert!(state(&d.w).customization.drawer.is_none());
}
fn sketch_overflow_variations(d: &mut Driver, theme: Theme) {
    d.w.dispatch(HeaderAction::SetSize { size: HeaderSize::Large }.action());
    for _ in 0..20 {
        d.w.dispatch(HeaderAction::Add {
            zone: HeaderZone::Left, before: None,
            item: HeaderItem::Tool { control: ToolbarControl::Command { command: CommandId::DrawingBrush } },
        }.action());
    }
    pump(350);
    let model = state(&d.w).workspace.layout.header;
    let hidden = model.entries().find(|entry|
        matches!(entry.item, HeaderItem::Tool { control } if control.has_variants())
            && !d.named(&format!("header-item-{}", entry.id)).is_mapped()).unwrap();
    d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    pump(150);
    let zone = model.location(hidden.id).unwrap().0;
    d.click_name(&format!("header-overflow-{}", zone.index()));
    let row = d.named(&format!("header-overflow-item-{}", hidden.id));
    assert_group_marker(&row, true);
    let overflow = d.named("header-overflow-popup").downcast::<gtk::Popover>().unwrap();
    capture_popover(&overflow, d.input.dir.join(format!("sketch-overflow-{theme:?}.png")).to_str().unwrap());
    let top_left = screen_point(overflow.upcast_ref(), &d.w.window, [0., 0.]);
    let bottom_right = screen_point(overflow.upcast_ref(), &d.w.window, [1., 1.]);
    assert!(top_left[0] >= 0. && top_left[1] >= 0.
        && bottom_right[0] <= d.w.window.width() as f32
        && bottom_right[1] <= d.w.window.height() as f32);
    let marker = find_named(&row, "layer-tool-group-symbolic").unwrap();
    let label = descendant::<gtk::Label>(&row).unwrap();
    let label_bounds = label.compute_bounds(&row).unwrap();
    assert!(label_bounds.x() + label_bounds.width() <= marker.compute_bounds(&row).unwrap().x());
    d.input.click(painted_group_point(d, &row));
    assert!(!variant_context(d).is_visible());
    assert!(!overflow.is_mapped());
    assert!(ui_session(&d.w).command(CommandId::DrawingBrush).selected);
    assert!(state(&d.w).customization.drawer.is_none());
    d.click_name(&format!("header-overflow-{}", zone.index()));
    let row = d.named(&format!("header-overflow-item-{}", hidden.id));
    d.input.click(painted_group_point(d, &row));
    assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor,
        DrawerAnchor::Header { id: hidden.id });
    assert!(!variant_context(d).is_visible());
    d.input.key(0xff1b);
    assert!(state(&d.w).customization.drawer.is_none());
    d.click_name(&format!("header-overflow-{}", zone.index()));
    let row = d.named(&format!("header-overflow-item-{}", hidden.id));
    let overflow = d.named("header-overflow-popup").downcast::<gtk::Popover>().unwrap();
    let before = (state(&d.w).layer_tools.tool, state(&d.w).brush.preset);
    secondary_group_click(d, &row);
    let context = variant_context(d);
    assert!(context.is_mapped());
    assert!(!overflow.is_mapped());
    assert_eq!((state(&d.w).layer_tools.tool, state(&d.w).brush.preset), before);
    capture_popover(&context, d.input.dir.join(format!("sketch-overflow-variants-{theme:?}.png")).to_str().unwrap());
    let anchor = DrawerAnchor::Header { id: hidden.id };
    let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
    let index = menu.sections[0].iter().position(|item| matches!(item.action,
        Some(UiAction::ChooseToolVariant { variant: ToolVariant::BrushGroup { group: layer_ui::ToolGroup::Marker }, .. }))).unwrap();
    choose_variant(d, anchor, index);
    assert_eq!(state(&d.w).brush.tool, layer_ui::ToolGroup::Marker.tool());
    assert!(state(&d.w).customization.drawer.is_none());
    assert_eq!(state(&d.w).workspace.layout.header, model);
    assert!(find_named(&anchor_widget(d, anchor), "layer-marker-symbolic").is_some());
    assert!(!context.is_mapped() && !overflow.is_mapped());
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_variations_overflow_input"]
fn native_toolbar_variations_overflow_input() {
    let mut d = Driver::new("art.capycanvas.ToolVariationsOverflow");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        restore(&d, WorkspacePreset::Painter);
        header_group_variations(&mut d, theme);
        sketch_overflow_variations(&mut d, theme);
    }
    d.finish();
}

fn variant_context(d: &Driver) -> gtk::Popover {
    d.w.popovers.borrow().iter().filter_map(|popup| popup.upgrade())
        .find(|popup| popup.has_css_class("panel-context-menu"))
        .unwrap()
}

fn choose_variant(d: &mut Driver, anchor: DrawerAnchor, index: usize) {
    let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
    let label = menu.sections.iter().flatten().nth(index).unwrap().label.clone();
    let item = find_menu_item(variant_context(&d).upcast_ref(), &label).unwrap();
    d.click(&item);
    assert!(!variant_context(&d).is_visible());
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_variations_input"]
fn native_toolbar_variations_input() {
    let mut d = Driver::new("art.capycanvas.ToolVariations");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (preset, slot) in [
            (WorkspacePreset::Photographer, ToolSlotId::Drawing),
            (WorkspacePreset::Illustrator, ToolSlotId::ManualSelection),
        ] {
            restore(&d, preset);
            assert_toolbar_markers(&d);
            let anchor = slot_anchor(&d, slot);
            let DrawerAnchor::Tile { panel, tile } = anchor else { unreachable!() };
            let name = format!("tile-{tile}");
            let button = d.named(&name);
            let panels = state(&d.w).workspace.layout.panels;
            let history = ui_session_mut(&d.w).capture_workspace().unwrap().history.generation;
            let revision = ui_session(&d.w).engine().document().revision;
            d.w.dispatch(UiAction::Invoke { command: if slot == ToolSlotId::Drawing {
                CommandId::Lasso
            } else { CommandId::Brush } });
            pump(150);
            assert_triangle_activation(&mut d, &button, anchor);
            secondary_group_click(&mut d, &button);
            assert!(variant_context(&d).is_visible());
            let icons = descendants::<gtk::Image>(variant_context(&d).upcast_ref())
                .into_iter().filter(|image| crate::icons::name(image).is_some_and(|name| name.starts_with("layer-")))
                .collect::<Vec<_>>();
            assert!(!icons.is_empty() && icons.iter().all(|image| image.is_visible() && image.paintable().is_some_and(|paintable| paintable.is::<gtk::Svg>())));
            capture_popover(&variant_context(&d), d.input.dir.join(format!("tool-variations-menu-{preset:?}-{theme:?}.png")).to_str().unwrap());
            let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
            assert!(menu.sections.iter().flatten().all(|row| row.icon.is_some()));
            choose_variant(&mut d, anchor, 1);
            assert!(state(&d.w).customization.drawer.is_none());
            assert_eq!(state(&d.w).workspace.layout.panels, panels);
            assert_eq!(ui_session_mut(&d.w).capture_workspace().unwrap().history.generation, history);
            assert_eq!(ui_session(&d.w).engine().document().revision, revision);
            assert_eq!(d.named(&name), button, "variant switches retain the native tile");
            let view = ui_session(&d.w).panel_view(panel).unwrap();
            let choice = &view.tiles.iter().find(|view| view.id == tile).unwrap().choice;
            let icon = descendant::<gtk::Image>(&button).unwrap();
            assert_eq!(crate::icons::name(&icon).as_deref(), Some(format!("layer-{}-symbolic", choice.icon).as_str()));
            assert!(choice.selected);
            d.click(&button);
            assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
            d.capture_canvas(&format!("tool-variations-drawer-{preset:?}-{theme:?}.png"));
            let drawer = d.named("drawer-panel-Brushes");
            let sibling = mapped_label(&drawer, &menu.sections[0][0].label).unwrap();
            d.click(&sibling);
            assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
            secondary_group_click(&mut d, &button);
            choose_variant(&mut d, anchor, 0);
            assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
            d.click(&button);
            assert!(state(&d.w).customization.drawer.is_none());
            pump(350);
            let p = d.point(&button);
            d.input.perform(serde_json::json!([
                {"point":p},{"button":273,"down":true},{"button":273,"down":false}
            ]));
            assert!(variant_context(&d).is_visible());
            assert!(find_menu_item(variant_context(&d).upcast_ref(), "Remove Tool").is_some());
            d.input.key(0xff1b);
            let button = button.clone().downcast::<gtk::Button>().unwrap();
            assert!(button.grab_focus());
            d.input.key(0xff67);
            assert!(variant_context(&d).is_visible(), "keyboard context action");
            d.input.key(0xff1b);
            assert!(button.has_focus(), "context menu restores its invoking button");
            for device in ["mouse", "touch"] {
                d.input.perform(serde_json::json!([
                    contact(device,"down",p),{"wait_ms":800},contact(device,"up",p)
                ]));
                assert_eq!(variant_context(&d).is_visible(), device == "touch");
                assert!(state(&d.w).customization.drawer.is_none(), "hold release must not click");
                if device == "touch" { d.input.key(0xff1b); }
            }
            d.capture_canvas(&format!("tool-variations-{preset:?}-{theme:?}.png"));
            d.w.dispatch(UiAction::Customize { action: CustomizationAction::SetTileStyle {
                panel, style: TileStyle::MediumLabeled,
            }});
            d.w.dispatch(UiAction::MovePanel {
                panel,
                target: DockTarget::Float { position: [250., 180.] },
                viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
            });
            pump(350);
            let floating = d.named(&name);
            secondary_group_click(&mut d, &floating);
            assert!(variant_context(&d).is_visible(), "labeled floating variation menu");
            choose_variant(&mut d, anchor, 1);
            d.capture_canvas(&format!("tool-variations-floating-{preset:?}-{theme:?}.png"));
            if preset == WorkspacePreset::Photographer {
                let control = ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing };
                d.w.dispatch(HeaderAction::Add {
                    zone: HeaderZone::Left,
                    before: None,
                    item: HeaderItem::Tool { control },
                }.action());
                pump(250);
                let header = d.named(&d.header_tool(control));
                let id = state(&d.w).workspace.layout.header.entries()
                    .find(|entry| entry.item == HeaderItem::Tool { control }).unwrap().id;
                let anchor = DrawerAnchor::Header { id };
                secondary_group_click(&mut d, &header);
                assert!(variant_context(&d).is_visible());
                choose_variant(&mut d, anchor, 2);
                let view = ui_session(&d.w).header_view_with(false);
                let item = view.items.iter().find(|item| item.id == id).unwrap();
                assert!(item.selected && item.has_variants);
                let icon = descendant::<gtk::Image>(&header).unwrap();
                assert_eq!(crate::icons::name(&icon).as_deref(), Some(format!("layer-{}-symbolic", item.icon).as_str()));
                d.click(&header);
                assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
                d.click(&header);
                assert!(state(&d.w).customization.drawer.is_none());
                pump(350);
                for (slot, index) in [(ToolSlotId::Healing, 1), (ToolSlotId::PhotoFill, 4)] {
                    let anchor = slot_anchor(&d, slot);
                    let DrawerAnchor::Tile { tile, .. } = anchor else { unreachable!() };
                    let opener = d.named(&format!("tile-{tile}"));
                    d.click(&opener);
                    d.click(&opener);
                    assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
                    let menu = ui_session(&d.w).context_menu(ContextTarget::ToolVariants { anchor }).unwrap();
                    let docked = state(&d.w).tool_set.groups;
                    assert_eq!(docked.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(),
                        menu.sections[0].iter().filter(|item| item.label != "Enclose and Fill")
                            .map(|item| item.label.as_str()).collect::<Vec<_>>());
                    let label = &menu.sections[0][index].label;
                    let drawer = d.named("drawer-panel-Brushes");
                    d.click(&mapped_label(&drawer, label).unwrap());
                    assert_eq!(state(&d.w).customization.drawer.as_ref().unwrap().anchor, anchor);
                    d.click(&opener);
                    assert!(state(&d.w).customization.drawer.is_none());
                    pump(350);
                }
            }
        }
        paint_category_variations(&mut d, theme);
        paint_slot_variations(&mut d, theme);
        sketch_group_variations(&mut d, theme);
    }
    d.finish();
}
pub(super) fn drag(d: &mut Driver, device: &str, a: [f32; 2], b: [f32; 2], held: bool) {
    if device == "pen" {
        d.input
            .perform(serde_json::json!([contact(device, "move", a)]));
    }
    d.input.perform(serde_json::json!([
        contact(device, "down", a),
        {"wait_ms":if held {800} else {0}},
        contact(device, "move", b),
        contact(device, "up", b)
    ]));
    if device == "pen" {
        d.input.perform(serde_json::json!([{"pen":"leave"}]));
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
        d.input.key(0xff54);
        d.input.key(0xff0d);
        let view = state(&d.w);
        assert_eq!(
            (view.color_picker.layer, view.color_picker.sample_width),
            (false, 5),
            "sample source and size are independent"
        );
        d.click_name("toolbar-choice-variant");
        d.input.key(0xff54);
        d.input.key(0xff0d);
        let view = state(&d.w);
        assert_eq!(view.layer_tools.tool, LayerCanvasTool::PickLayer);
        assert_eq!(view.color_picker.sample_width, 5);
        d.click_name("toolbar-choice-variant");
        d.capture_canvas(&format!("eyedropper-menu-{theme:?}.png"));
        d.input.key(0xff1b);
        d.input.key(0xff1b);

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
        d.input.key(0xff1b);
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
        slider_preview_placement(&mut d, &["pen"]);
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
    crate::capture(
        &d.w,
        d.input.dir.join("narrow-options.png").to_str().unwrap(),
    );
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
    d.input
        .perform(serde_json::json!([{ "point": a, "down": true },{"wait_ms":800},{"down":false}]));
    assert!(
        !d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.has_css_class("panel-context-menu") && p.is_visible()),
        "mouse holds never open a menu"
    );
    d.input.perform(serde_json::json!([
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
            let event = |phase: &str, p: [f32; 2]| contact(device, phase, p);
            if device == "pen" {
                d.input.perform(serde_json::json!([event("move", a)]));
            }
            let mut events = vec![event("down", a)];
            for distance in [140., 70., 38., 28., 20.] {
                events.push(event("move", [viewport[0] - distance, y]));
            }
            d.input.perform(serde_json::Value::Array(events));
            assert!(
                matches!(d.w.drop_hint.borrow().as_ref().map(|h| &h.target),
                Some(DockTarget::CompactEdge { edge: Edge::Right, alignment: a }) if *a == alignment),
                "{device}: near-edge preview at a usable contact distance"
            );
            d.input
                .perform(serde_json::json!([event("up", [viewport[0] - 20., y])]));
            if device == "pen" {
                d.input.perform(serde_json::json!([{"pen":"leave"}]));
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
        if device == "mouse" {
            let before = state(&d.w).workspace;
            let grip = toolbar_grip(d, panel);
            d.input.perform(serde_json::json!([
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
#[ignore = "private Mutter: --native-test=native_compact_toolbar_presentation_input"]
fn native_compact_toolbar_presentation_input() {
    let d = Driver::new("art.capycanvas.CompactPresentation");
    restore(&d, WorkspacePreset::Painter);
    let panel = brush_panel(&d);
    let undo = state(&d.w)
        .workspace
        .layout
        .panel(panel)
        .unwrap()
        .tiles()
        .iter()
        .find(|t| {
            t.control
                == ToolbarControl::Command {
                    command: CommandId::Undo,
                }
        })
        .unwrap()
        .id;
    d.w.window.unmaximize();
    for (width, height, style) in [
        (960, 600, TileStyle::Small),
        (1400, 950, TileStyle::Medium),
        (960, 600, TileStyle::Small),
        (1400, 950, TileStyle::Medium),
    ] {
        d.w.window.set_default_size(width, height);
        pump(600);
        let resolved = d.w.resolved();
        let group = resolved.groups.iter().find(|g| g.active == panel).unwrap();
        assert_eq!(group.tiles.as_ref().unwrap().presentation.tile_style, style);
        assert_eq!(
            group.bounds.width,
            style.size()[0],
            "{width}x{height}: one column"
        );
        assert!(!d.w.customization.presentation_stale(&resolved));
        let button = d.named(&format!("tile-{undo}"));
        assert_eq!(button.parent().unwrap().width(), style.size()[0] as i32);
        assert_eq!(
            descendant::<gtk::Image>(&button).unwrap().pixel_size(),
            style.icon_size() as i32
        );
        assert_eq!(
            state(&d.w)
                .workspace
                .layout
                .panel(panel)
                .unwrap()
                .tile_style,
            TileStyle::Medium,
            "the configured style is unchanged"
        );
    }
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_toolbar_value_controls_input"]
fn native_toolbar_value_controls_input() {
    let mut d = Driver::new("art.capycanvas.ToolbarValues");
    restore(&d, WorkspacePreset::Painter);
    slider_preview_gestures(&mut d, &["mouse", "touch"]);
    slider_preview_placement(&mut d, &["mouse", "touch"]);
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
    d.input.perform(
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
    d.input
        .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
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
    let scale = descendant::<gtk::Scale>(&popup).unwrap();
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
        d.input
            .dir
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
                capture_popover(
                    &d.named("brush-slider-preview").downcast().unwrap(),
                    d.input.dir
                        .join(format!("audit-slider-preview-{theme:?}-{edge:?}-{style:?}.png"))
                        .to_str()
                        .unwrap(),
                );
                d.input.key(0xff1b);
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
        d.input
            .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
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
        d.input
            .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
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
        d.input.key(0xff1b);
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
                    let bar = bar.compute_bounds(&d.w.window).unwrap();
                    let choice = d
                        .named("toolbar-choice-variant")
                        .compute_bounds(&d.w.window)
                        .unwrap();
                    assert_eq!(
                        (bar.y(), bar.height()),
                        (choice.y(), choice.height()),
                        "segments match the dropdown height"
                    );
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

fn slider_preview_placement(d: &mut Driver, devices: &[&str]) {
    let id = component_id(d, ToolbarControl::BrushSizeSlider);
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        d.w.dispatch(UiAction::MovePanel {
            panel: brush_panel(d),
            target: DockTarget::CompactEdge {
                edge,
                alignment: EdgeAlignment::Center,
            },
            viewport: [d.w.surface.width() as f32, d.w.surface.height() as f32],
        });
        pump(150);
        for &device in devices {
            let scale = d.named(&format!("component-slider-{id}"));
            let toolbar = scale
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .compute_bounds(&d.w.window)
                .unwrap();
            let p = d.point(&scale);
            drag(d, device, p, p, false);
            pump(100);
            let popup = d
                .named("brush-slider-preview")
                .downcast::<gtk::Popover>()
                .unwrap();
            let content = popup.child().unwrap();
            let [x, y] = screen_point(&content, &d.w.window, [0., 0.]);
            let [right, bottom] = screen_point(&content, &d.w.window, [1., 1.]);
            let gap = match edge {
                Edge::Left => x - toolbar.x() - toolbar.width(),
                Edge::Right => toolbar.x() - right,
                Edge::Top => y - toolbar.y() - toolbar.height(),
                Edge::Bottom => toolbar.y() - bottom,
            };
            assert!(
                (7. ..=16.).contains(&gap),
                "{device}/{edge:?}: preview clears the toolbar, gap={gap}"
            );
            capture_popover(
                &popup,
                d.input
                    .dir
                    .join(format!("slider-placement-{edge:?}-{device}.png"))
                    .to_str()
                    .unwrap(),
            );
            // A real drag dismisses the nonmodal preview, including with pen.
            let end = if matches!(edge, Edge::Left | Edge::Right) {
                [p[0], p[1] + 24.]
            } else {
                [p[0] + 24., p[1]]
            };
            drag(d, device, p, end, false);
            assert!(!popup.is_mapped());
        }
    }
}

fn slider_preview_gestures(d: &mut Driver, devices: &[&str]) {
    let id = component_id(d, ToolbarControl::BrushSizeSlider);
    for &device in devices {
        let context = state(&d.w).toolbar_context();
        let stamp =
            ui_session(&d.w)
                .toolbar_stamp(context);
        assert!(
            stamp.is_ok(),
            "{device}: preview assets ready: {:?}",
            stamp.err()
        );
        let scale = d.named(&format!("component-slider-{id}"));
        let center = d.point(&scale);
        let p = [center[0], center[1] - 23.];
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
            d.input
                .dir
                .join(format!("slider-stamp-{device}.png"))
                .to_str()
                .unwrap(),
        );
        d.w.dispatch(UiAction::SetBrushSize { value: 2048. });
        pump(50);
        capture_popover(
            &popup.clone().downcast::<gtk::Popover>().unwrap(),
            d.input
                .dir
                .join(format!("slider-stamp-large-{device}.png"))
                .to_str()
                .unwrap(),
        );
        d.w.dispatch(UiAction::SetBrushSize { value: 3. });
        pump(50);
        let near = [p[0], p[1] + 16.];
        let far = [p[0], p[1] + 24.];
        drag(d, device, far, far, false);
        assert_ne!(
            state(&d.w).brush.diameter,
            saved,
            "{device}: distant tap does not snap"
        );
        drag(d, device, near, near, false);
        assert_eq!(
            state(&d.w).brush.diameter,
            saved,
            "{device}: nearby tap recalls exact bookmark value"
        );
        drag(d, device, far, near, false);
        assert_ne!(
            state(&d.w).brush.diameter,
            saved,
            "{device}: dragging near bookmark does not snap"
        );
        drag(d, device, near, near, false);
        assert_eq!(state(&d.w).brush.diameter, saved);
        d.capture_canvas(&format!("slider-bookmark-centered-{device}.png"));
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
        d.input.key(0xff1b);
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

#[test]
#[ignore = "private Mutter: --native-test=native_tonal_toolbar_input"]
fn native_tonal_toolbar_input() {
    let mut d=Driver::new("art.capycanvas.TonalToolbar");
    restore(&d,WorkspacePreset::Photographer);
    let options=component_id(&d,ToolbarControl::TOOL_OPTIONS);
    // A horizontal lane fits both mode and tone bars. The six-tone group
    // exceeds the narrow vertical component's budget and uses its overflow.
    let mut workspace=state(&d.w).workspace;
    let removed:Vec<_>=workspace.layout.panel(Panel::Commands).unwrap().tiles().iter()
        .filter(|t|t.control.options_style().is_none()).map(|t|t.id).collect();
    for id in removed {
        if workspace.layout.panel(Panel::Commands).unwrap().tiles().iter().any(|t|t.id==id) {
            workspace.layout.remove_tool(Panel::Commands,id).unwrap();
        }
    }
    d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(workspace)});pump(200);
    d.w.dispatch(UiAction::Invoke {command:CommandId::TonalSelect});
    let ready=|d:&Driver| {
        let deadline=Instant::now()+Duration::from_secs(25);
        loop {pump(20);if ui_session(&d.w).require_document_idle().is_ok() {break;}
            assert!(Instant::now()<deadline,"tonal update: {:?}",state(&d.w).host_error);}
    };
    for edge in [Edge::Top,Edge::Left] {
        d.w.dispatch(UiAction::MovePanel {panel:Panel::Commands,target:DockTarget::Edge {edge,outer:true},viewport:[1600.,1000.]});pump(180);
        let choice=d.named("toolbar-segments-tonal-tones");
        if edge==Edge::Top {
            assert!(choice.is_mapped());
            assert_eq!(choice.height(),24,"horizontal presets retain main's partial-height bars");
            d.click_name("toolbar-segment-tonal-tones-4");ready(&d);
            assert_shared_icons(&choice);
        } else {
            assert!(!choice.is_mapped(),"complete tone group uses the narrow bar's overflow");
            d.click_name(&format!("tile-{options}"));pump(150);
            let bar=d.named("tool-choice-bar-tonal-tones");
            assert!(bar.is_mapped() && bar.height()<=36);
            assert_shared_icons(&bar);
            d.click_name("tool-choice-tonal-tones-0");ready(&d);
            d.click_name("tool-choice-tonal-tones-4");ready(&d);
        }
        assert!(state(&d.w).tool_extra.iter().any(|o|matches!(o,layer_ui::ToolOption::Choice {id:"tonal-tones",items,..} if items[4].selected)));
        assert!(ui_session(&d.w).engine().document().working.selection.is_some());
        save_snapshot(&d.w, 120, || d.input.dir.join(format!("tonal-toolbar-{edge:?}.png")));
        if edge==Edge::Left {d.click_name(&format!("tile-{options}"));pump(100);}
    }
    d.click_name(&format!("tile-{options}"));pump(150);
    let drawer=d.named("drawer-panel-ToolSettings");assert!(drawer.is_mapped());
    let custom=find_named(&drawer,"tool-choice-tonal-tones-5").unwrap();d.click(&custom);ready(&d);
    let lower=find_named(&drawer,"tool-setting-tonal_lower").unwrap();d.number(&lower,"1");ready(&d);
    assert_eq!(state(&d.w).tool_settings.iter().find(|f|f.id=="tonal_upper").unwrap().value,-1.5,"crossing bounds stops at the other endpoint");
    let upper=find_named(&drawer,"tool-setting-tonal_upper").unwrap();d.number(&upper,"3");ready(&d);
    let softness=find_named(&drawer,"tool-setting-tonal_softness").unwrap();d.number(&softness,"75");ready(&d);
    assert_eq!(state(&d.w).tool_settings.iter().find(|f|f.id=="tonal_softness").unwrap().value,0.75);
    assert!(state(&d.w).tool_actions.iter().all(|a|a.group().is_some()));
    save_snapshot(&d.w, 120, || d.input.dir.join("tonal-toolbar-overflow.png"));
    assert!(state(&d.w).host_error.is_none(),"{:?}",state(&d.w).host_error);
    // A floating-point document adds Bright HDR, with Custom still last.
    let project=new_drawing_at(2048,1536,layer_core::color::SampleDepth::F16);
    let hdr=Workspace::with_project(&d._app,Some((project,None)));
    hdr.window.maximize();hdr.window.present();pump(1800);
    d.w.window.destroy();d.w=hdr;
    restore(&d,WorkspacePreset::Painter);
    d.w.dispatch(UiAction::Invoke {command:CommandId::TonalSelect});pump(150);
    let opener=d.header_tool(ToolbarControl::Command {command:CommandId::Select});
    d.click_name(&opener);if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
    assert!(matches!(state(&d.w).tool_extra.as_slice(),[layer_ui::ToolOption::Choice {items,..}] if items.len()==7 && items[5].icon=="tonal-bright-hdr" && items[6].icon=="tonal-custom"));
    let bar=d.named("tool-choice-bar-tonal-tones");
    assert!(bar.height()<=36);
    d.click_name("tool-choice-tonal-tones-5");ready(&d);
    assert!(d.named("tool-choice-tonal-tones-5").downcast_ref::<gtk::ToggleButton>().unwrap().is_active());
    save_snapshot(&d.w, 120, || d.input.dir.join("tonal-hdr-presets.png"));
    d.click_name("tool-choice-tonal-tones-6");ready(&d);
    assert!(d.named("tool-setting-tonal_lower").is_mapped());
    assert!(state(&d.w).host_error.is_none(),"{:?}",state(&d.w).host_error);
    d.finish();
}

#[test]
#[ignore = "private Mutter: --native-test=native_tonal_toolbar_range_input"]
fn native_tonal_toolbar_range_input() {
    tonal_toolbar_range_input(false);
}
#[test]
#[ignore = "private Mutter: --native-test=native_tonal_toolbar_range_pen_input --tablet"]
fn native_tonal_toolbar_range_pen_input() {
    tonal_toolbar_range_input(true);
}
fn tonal_toolbar_range_input(pen: bool) {
    let mut d = Driver::new("art.capycanvas.TonalToolbarRange");
    restore(&d, WorkspacePreset::Photographer);
    let options = component_id(&d, ToolbarControl::TOOL_OPTIONS);
    d.w.dispatch(UiAction::Invoke { command: CommandId::TonalSelect });
    pump(180);
    d.click_name("toolbar-segment-tonal-tones-5");
    let ready = super::selection_tools::wait_tonal;
    let bounds = super::selection_tools::tonal_bounds;
    ready(&d);
    let range = d.named("tool-range-toolbar-tonal");
    assert!(range.is_mapped(), "the standard Photo toolbar fits the custom interval");
    assert_eq!(range.height(), 28);
    let track = find_named(&range, "range-track-toolbar-tonal").unwrap();
    assert!(track.width() >= 180, "wide inline track: {}", track.width());
    for id in ["tonal_lower", "tonal_upper"] {
        let input = find_named(&range, &format!("toolbar-setting-{id}")).unwrap();
        assert!(input.width() <= 48, "compact one-decimal endpoint");
    }
    assert_eq!(d.named("toolbar-segments-tonal-tones").height(), 24);
    let before_workspace = state(&d.w).workspace;
    super::selection_tools::tonal_range_contacts(&mut d, "tool-range-toolbar-tonal", pen, |d, _| {
        assert_eq!(d.named("tool-range-toolbar-tonal"), range, "retain the captured range");
        assert_eq!(state(&d.w).workspace, before_workspace, "range contacts do not reorder the toolbar");
    });
    if pen { assert!(state(&d.w).host_error.is_none()); d.finish(); return; }
    let range = d.named("tool-range-toolbar-tonal");
    let lower = find_named(&range, "toolbar-setting-tonal_lower").unwrap();
    let upper = find_named(&range, "toolbar-setting-tonal_upper").unwrap();
    d.number(&lower, "-7.2"); ready(&d);
    d.number(&upper, "2.3"); ready(&d);
    assert_eq!(bounds(&d), [-7.2, 2.3], "new context keeps both endpoints live");
    save_snapshot(&d.w, 120, || d.input.dir.join("tonal-toolbar-custom.png"));

    // The complete range is available in the narrow bar's existing overflow.
    d.w.dispatch(UiAction::MovePanel { panel: Panel::Commands, target: DockTarget::Edge { edge: Edge::Left, outer: true }, viewport: [1600., 1000.] }); pump(180);
    assert!(!range.is_mapped());
    d.click_name(&format!("tile-{options}")); pump(150);
    let drawer = d.named("drawer-panel-ToolSettings");
    let lower = find_named(&drawer, "tool-setting-tonal_lower").unwrap();
    d.number(&lower, "-8.5"); ready(&d);
    assert_eq!(bounds(&d), [-8.5, 2.3]);
    d.click_name(&format!("tile-{options}")); pump(100);
    d.w.dispatch(UiAction::MovePanel { panel: Panel::Commands, target: DockTarget::Edge { edge: Edge::Top, outer: true }, viewport: [1600., 1000.] }); pump(180);
    let old = d.named("toolbar-setting-tonal_lower").downcast::<crate::number_control::NumberControl>().unwrap();
    assert!(old.is_mapped());
    assert!((old.value() + 8.5).abs() < 0.001);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Brush }); pump(150);
    old.set_value(-9.); pump(80);
    assert!(state(&d.w).host_error.is_none(), "retired range cannot send stale edits");
    d.finish();
}
