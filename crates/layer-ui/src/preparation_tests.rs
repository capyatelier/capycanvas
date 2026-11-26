use super::*;
use test_support::*;

fn preparation_fixture(platform: Platform) -> UiSession<Recorder> {
    let mut s = session(platform);
    for panel in &mut s.state.workspace.layout.panels {
        if let PanelContent::Toolbar { tiles, .. } = &mut panel.content { tiles.clear(); }
    }
    for zone in &mut s.state.workspace.layout.header.zones { zone.clear(); }
    s.state.workspace.layout.phone = None;
    s.changed(regions::LAYOUT, false);
    s
}

fn assert_toolbar(s: &UiSession<Recorder>, presets: &[u32]) {
    let expected: Vec<_> = presets.iter().map(|&id| s.tools.brush_in(
        tools::preset(id).unwrap(), s.engine.document().composition().color.space,
    )).collect();
    let actual = &s.engine.backend().contexts.last().unwrap().brushes;
    assert_eq!(actual.len(), expected.len());
    for brush in &expected { assert!(actual.contains(brush)); }
}

#[test]
fn toolbar_brushes_follow_visible_controls() {
    for platform in Platform::ALL {
        let mut s = preparation_fixture(platform);
        let pencil = Tool::Pencil.default_preset();
        let airbrush = Tool::Airbrush.default_preset();
        let eraser = Tool::Eraser.default_preset();
        let layout = &mut s.state.workspace.layout;
        layout.set_panel_visible(Panel::Color, true).unwrap();
        let body_group = layout.panel_group(Panel::Color).unwrap();
        let visible = layout.add_toolbar(Some(body_group), "Visible", &[ToolbarControl::Brush { id: pencil }]).unwrap();
        let group = layout.panel_group(visible).unwrap();
        let inactive = layout.add_toolbar(Some(group), "Other tab", &[ToolbarControl::Brush { id: airbrush }]).unwrap();
        layout.select_tab(group, visible).unwrap();
        let hidden = layout.add_toolbar(None, "Hidden", &[ToolbarControl::Brush { id: eraser }]).unwrap();
        layout.set_panel_visible(hidden, false).unwrap();
        s.changed(regions::LAYOUT, false);
        assert_toolbar(&s, &[pencil]);
        s.state.workspace.layout.select_tab(group, inactive).unwrap();
        s.changed(regions::LAYOUT, false);
        assert_toolbar(&s, &[airbrush]);
        s.state.workspace.layout.set_column_collapsed(group, true, [1000., 1000.]).unwrap();
        s.changed(regions::LAYOUT, false);
        assert_toolbar(&s, &[]);
    }
}

#[test]
fn toolbar_commands_remember_brushes_and_overrides_without_camera_rebuilds() {
    let mut s = preparation_fixture(Platform::Gtk);
    let drawing = Tool::Pencil.default_preset();
    let sculpt = Tool::Blend.default_preset();
    s.dispatch(UiAction::SelectBrush { id: drawing }).unwrap();
    s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
    s.dispatch(UiAction::SelectBrush { id: sculpt }).unwrap();
    s.dispatch(UiAction::SelectBrush { id: Tool::Eraser.default_preset() }).unwrap();
    s.state.workspace.layout.add_toolbar(None, "Remembered", &[
        ToolbarControl::Command { command: CommandId::DrawingBrush },
        ToolbarControl::Command { command: CommandId::Sculpt },
        ToolbarControl::Brush { id: Tool::Eraser.default_preset() },
    ]).unwrap();
    customize(&mut s, CustomizationAction::Header { action: HeaderAction::Add {
        zone: HeaderZone::Left, before: None,
        item: HeaderItem::Tool { control: ToolbarControl::Brush { id: Tool::Airbrush.default_preset() } },
    } });
    s.changed(regions::LAYOUT, false);
    assert_toolbar(&s, &[drawing, sculpt, Tool::Airbrush.default_preset()]);
    let before = s.toolbar_brushes.clone();
    let allocation = s.toolbar_brushes.as_ptr();
    let contexts = s.engine.backend().contexts.len();
    s.state.camera.translation[0] += 10.;
    s.changed(regions::CAMERA, false);
    assert_eq!(s.toolbar_brushes.as_ptr(), allocation);
    assert_eq!(s.toolbar_brushes, before);
    assert_eq!(s.engine.backend().contexts.len(), contexts);
}

#[test]
fn closed_phone_drawer_keeps_the_persistent_brush_bar_visible() {
    for platform in Platform::ALL.into_iter().filter(|p| p.presents_phone_layout()) {
        let mut s = preparation_fixture(platform);
        let pencil = Tool::Pencil.default_preset();
        s.state.workspace.layout.phone = Some(Vec::new());
        s.state.workspace.layout.insert_phone_tools(None, &[ToolbarControl::Brush { id: pencil }]).unwrap();
        s.state.phone.platform = Some(platform);
        s.state.phone.hidden = false;
        s.state.customization.drawer = None;
        s.set_viewport([390., 844.], [390, 844]).unwrap();
        s.changed(regions::LAYOUT, false);
        assert!(s.phone_layout().is_some());
        assert_toolbar(&s, &[pencil]);
    }
}

#[test]
fn replacing_the_renderer_resends_one_unchanged_preparation_context() {
    let mut s = preparation_fixture(Platform::Windows);
    s.state.workspace.layout.add_toolbar(None, "Brush", &[
        ToolbarControl::Brush { id: Tool::Pencil.default_preset() },
    ]).unwrap();
    s.changed(regions::LAYOUT, false);
    let expected = s.preparation_context.clone().unwrap();
    s.replace_renderer(Recorder::default()).unwrap();
    assert_eq!(s.engine.backend().contexts, vec![expected]);
    s.changed(regions::CAMERA, false);
    assert_eq!(s.engine.backend().contexts.len(), 1);
}

#[test]
fn default_toolbar_warming_matches_presented_tile_and_header_models() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.changed(regions::LAYOUT, false);
        let mut controls = Vec::new();
        if s.phone_layout().is_some() {
            controls.extend(s.state.phone_tools().into_iter().map(|v| v.resolved_control));
        } else {
            for group in s.layout(s.workspace_viewport()).groups {
                controls.extend(s.panel_view(group.active).unwrap().tiles.into_iter().map(|v| v.resolved_control));
            }
            controls.extend(s.header_view().items.into_iter().filter_map(|v| v.resolved_control));
        }
        let mut ids = Vec::new();
        for control in controls {
            let id = match control {
                ToolbarControl::Brush { id } => Some(id),
                ToolbarControl::Command { command } => s.tools.command_preset_in(command, &s.state.brush, s.layer_interaction.tool),
                _ => None,
            };
            if let Some(id) = id.filter(|&id| id != s.state.brush.preset && !ids.contains(&id)) { ids.push(id); }
        }
        assert!(!ids.is_empty(), "{platform:?} exposes alternative brushes");
        assert_toolbar(&s, &ids);
    }
}

#[test]
fn customized_slot_warming_follows_the_displayed_variant_and_its_override() {
    let mut s = preparation_fixture(Platform::Gtk);
    let id = Tool::Airbrush.default_preset();
    s.dispatch(UiAction::SelectBrush { id }).unwrap();
    s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
    invoke(&mut s, CommandId::Eraser);
    let panel = s.state.workspace.layout.add_toolbar(None, "Slot", &[ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing }]).unwrap();
    s.changed(regions::LAYOUT, false);
    let tile = s.state.workspace.layout.panel(panel).unwrap().tiles()[0].id;
    s.dispatch(UiAction::ChooseToolVariant { anchor: DrawerAnchor::Tile { panel, tile }, variant: ToolVariant::Command { command: CommandId::Airbrush } }).unwrap();
    invoke(&mut s, CommandId::Eraser);
    let shown = s.panel_view(panel).unwrap().tiles[0].resolved_control;
    assert!(shown == ToolbarControl::Brush { id } || shown == ToolbarControl::Command { command: CommandId::Airbrush });
    assert_toolbar(&s, &[id]);
    let overridden = s.toolbar_brushes.clone();
    s.dispatch(UiAction::SelectBrush { id }).unwrap();
    s.dispatch(UiAction::SetBrushSize { value: 91. }).unwrap();
    invoke(&mut s, CommandId::Eraser);
    assert_toolbar(&s, &[id]);
    assert_ne!(s.toolbar_brushes, overridden);
}

#[test]
fn preparation_context_follows_tool_and_selection() {
    use layer_render::{OperationUse, PreparationUses as Uses};
    let mut mismatches = Vec::new();
    for platform in Platform::ALL {
        for (command, expected) in [
            (CommandId::Lasso, Uses {selection_strokes: true, ..Default::default()}),
            (CommandId::RectangleSelect, Uses {selection_strokes: true, ..Default::default()}),
            (CommandId::AutoSelect, Uses {selection_strokes: true, ..Default::default()}),
            (CommandId::ColorSelect, Uses {selection_strokes: true, ..Default::default()}),
            (CommandId::Fill, Uses {fill: true, ..Default::default()}),
            (CommandId::LassoFill, Uses {fill: true, ..Default::default()}),
            (CommandId::EncloseFill, Uses {fill: true, ..Default::default()}),
            (CommandId::SelectionBrush, Uses {selection_strokes: true, selection_paint: true, ..Default::default()}),
            (CommandId::QuickMask, Uses {selection_paint: true, ..Default::default()}),
            (CommandId::PaintUnfilled, Uses {selection_paint: true, fill: true, ..Default::default()}),
            (CommandId::Gradient, Uses {operation: Some(OperationUse::Gradient), ..Default::default()}),
            (CommandId::Figure, Uses {operation: Some(OperationUse::Figure), ..Default::default()}),
            (CommandId::Brush, Uses::default()),
        ] {
            let mut s = session(platform);
            invoke(&mut s, command);
            let actual = s.engine.backend().contexts.last().unwrap().next;
            if actual != expected { mismatches.push(format!("{platform:?}/{command:?}: {actual:?} != {expected:?}")); }
            let count = s.engine.backend().contexts.len();
            s.changed(regions::CAMERA, false);
            assert_eq!(s.engine.backend().contexts.len(), count, "camera changes keep the same next uses");
        }
        let mut s = session(platform);
        invoke(&mut s, CommandId::SelectAll);
        let actual = s.engine.backend().contexts.last().unwrap().next;
        let expected = Uses {transform: true, fill: true, ..Default::default()};
        if actual != expected { mismatches.push(format!("{platform:?}/SelectAll: {actual:?} != {expected:?}")); }
        invoke(&mut s, CommandId::Deselect);
        assert_eq!(s.engine.backend().contexts.last().unwrap().next, Uses::default());
    }
    assert!(mismatches.is_empty(), "{mismatches:#?}");
}
