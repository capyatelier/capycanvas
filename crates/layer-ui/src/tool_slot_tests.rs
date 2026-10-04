fn slot_fixture(platform: Platform, slots: &[ToolSlotId]) -> (UiSession<Recorder>, Panel, Vec<u32>) {
    let controls: Vec<_> = slots.iter().map(|&slot| ToolbarControl::ToolSlot { slot }).collect();
    group_fixture(platform, &controls)
}

fn group_fixture(platform: Platform, controls: &[ToolbarControl]) -> (UiSession<Recorder>, Panel, Vec<u32>) {
    let mut s = session(platform);
    let mut workspace = s.state().workspace.clone();
    let panel = workspace.layout.add_toolbar(None, "Variants", controls).unwrap();
    let ids = workspace.layout.panel(panel).unwrap().tiles().iter().map(|tile| tile.id).collect();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
    (s, panel, ids)
}

fn slot_menu(s: &UiSession<Recorder>, anchor: DrawerAnchor) -> Vec<ContextMenuItem> {
    s.context_menu(ContextTarget::ToolVariants { anchor }).unwrap().sections.into_iter().flatten().collect()
}

fn slot_choice(s: &UiSession<Recorder>, anchor: DrawerAnchor, variant: ToolVariant) -> UiAction {
    slot_menu(s, anchor).into_iter().find_map(|item| match item.action {
        Some(action @ UiAction::ChooseToolVariant { variant: value, .. }) if value == variant => Some(action),
        _ => None,
    }).unwrap()
}

fn selected_slot_variant(s: &UiSession<Recorder>, anchor: DrawerAnchor) -> ToolVariant {
    let selected: Vec<_> = slot_menu(s, anchor).into_iter().filter(|item| item.selected == Some(true)).collect();
    assert_eq!(selected.len(), 1);
    let Some(UiAction::ChooseToolVariant { variant, .. }) = selected[0].action.clone() else { panic!("a checked variation is actionable") };
    variant
}

fn activate_slot(s: &mut UiSession<Recorder>, anchor: DrawerAnchor) {
    let action = match anchor {
        DrawerAnchor::Tile { panel, tile } => UiAction::ActivateTile { panel, tile },
        DrawerAnchor::Header { id } => UiAction::ActivateHeaderItem { id },
        _ => panic!("a tool slot belongs to a tile or title-bar item"),
    };
    s.dispatch(action).unwrap();
}

fn add_group_header(s: &mut UiSession<Recorder>, control: ToolbarControl) -> DrawerAnchor {
    customize(s, CustomizationAction::Header { action: HeaderAction::Add {
        zone: HeaderZone::Left, before: None, item: HeaderItem::Tool { control },
    } });
    let id = s.state().workspace.layout.header.entries().filter(|entry|
        entry.item == HeaderItem::Tool { control }).last().unwrap().id;
    s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![HeaderItemBounds {
        id, bounds: Bounds { x: 800., y: 0., width: 40., height: 60. },
    }] }).unwrap();
    DrawerAnchor::Header { id }
}

fn selection_group_commands<'a>(items: impl IntoIterator<Item = &'a ToolSetItem>) -> std::collections::BTreeSet<String> {
    items.into_iter().filter_map(|item| match item.action {
        UiAction::ChooseToolVariant { variant, .. } => Some(variant.command()),
        UiAction::Invoke { command } => Some(command),
        _ => None,
    }).filter(|command| SelectionTool::ALL.iter().any(|tool| tool.command() == *command))
        .map(CommandId::shortcut_id).collect()
}

#[test]
fn tool_group_commands_publish_medium_choices_and_remember_dynamic_icons() {
    for platform in Platform::ALL {
        for (command, groups) in [
            (CommandId::Pen, &[ToolGroup::Pen, ToolGroup::Marker][..]),
            (CommandId::Pencil, &[ToolGroup::Pencil, ToolGroup::Pastel]),
            (CommandId::Brush, &[ToolGroup::Paint, ToolGroup::Watercolor, ToolGroup::Oil]),
            (CommandId::Airbrush, &[ToolGroup::Airbrush, ToolGroup::Spray]),
        ] {
            let control = ToolbarControl::Command { command };
            assert!(control.has_variants());
            let (mut s, panel, ids) = group_fixture(platform, &[control]);
            let tile = DrawerAnchor::Tile { panel, tile: ids[0] };
            let header = add_group_header(&mut s, control);
            let original_layout = s.state().workspace.layout.clone();
            assert_eq!(slot_menu(&s, tile).len(), groups.len());
            for &group in groups {
                let variant = ToolVariant::BrushGroup { group };
                s.dispatch(slot_choice(&s, tile, variant)).unwrap();
                let remembered = s.state().brush.preset;
                assert_eq!(tools::group(remembered), group);
                for active in [true, false] {
                    if !active { invoke(&mut s, CommandId::Hand); }
                    let tile_view = s.panel_view(panel).unwrap().tiles.into_iter().find(|item| item.id == ids[0]).unwrap();
                    assert!(tile_view.has_variants);
                    assert_eq!(tile_view.choice.control, control);
                    assert_eq!(tile_view.choice.icon, group.icon());
                    assert_eq!(tile_view.choice.selected, active);
                    let DrawerAnchor::Header { id } = header else { unreachable!() };
                    let header_view = s.header_view();
                    let header_item = header_view.items.iter().find(|item| item.id == id).unwrap();
                    assert!(header_item.has_variants);
                    assert_eq!(header_item.icon, group.icon());
                    assert_eq!(header_item.selected, active);
                    assert_eq!(selected_slot_variant(&s, tile), variant);
                    assert_eq!(selected_slot_variant(&s, header), variant);
                }
                activate_slot(&mut s, header);
                assert_eq!(s.state().brush.preset, remembered);
                assert!(s.state().customization.drawer.is_none());
                activate_slot(&mut s, header);
                assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, header);
                activate_slot(&mut s, header);
                assert!(s.state().customization.drawer.is_none());
            }
            assert_eq!(s.state().workspace.layout, original_layout, "existing command controls retain their saved identity");
            invoke(&mut s, CommandId::Hand);
            let capture = s.capture_workspace().unwrap();
            let mut restored = session(platform);
            restored.adopt_workspace(PreparedWorkspace::new(capture).unwrap()).unwrap();
            let variant = ToolVariant::BrushGroup { group: *groups.last().unwrap() };
            assert_eq!(selected_slot_variant(&restored, tile), variant);
            assert_eq!(selected_slot_variant(&restored, header), variant);
        }
    }
}

#[test]
fn tool_group_temporary_brushes_preserve_memory_from_a_nonpainting_tool() {
    for modifier in [false, true] {
        let (mut s, panel, ids) = group_fixture(Platform::Gtk, &[
            ToolbarControl::Command { command: CommandId::Brush },
            ToolbarControl::Command { command: CommandId::DrawingBrush },
        ]);
        let brush = DrawerAnchor::Tile { panel, tile: ids[0] };
        let drawing = DrawerAnchor::Tile { panel, tile: ids[1] };
        let header = add_group_header(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush });
        let watercolor = ToolVariant::BrushGroup { group: ToolGroup::Watercolor };
        s.dispatch(slot_choice(&s, brush, watercolor)).unwrap();
        invoke(&mut s, CommandId::Hand);
        if modifier {
            let mut settings = s.state().settings.clone();
            let mut holds = settings.hold_keys(Platform::Gtk);
            let hold = holds.iter_mut().find(|hold|
                hold.key == KeyChord::new("alt", Modifiers::default())).unwrap();
            hold.actions.insert(ToolCategory::Navigation, CommandId::Pen.shortcut_id());
            settings.hold_keys = Some(holds);
            s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
        }
        let permanent = s.workspace_working_state();
        let name = if modifier { "Alt_L" } else { "p" };
        key(&mut s, name, true, false, false);
        assert!(s.command(CommandId::Pen).selected);
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        let pen = ToolVariant::BrushGroup { group: ToolGroup::Pen };
        assert_eq!(selected_slot_variant(&s, brush), watercolor);
        for anchor in [drawing, header] { assert_eq!(selected_slot_variant(&s, anchor), pen); }
        let tiles = s.panel_view(panel).unwrap().tiles;
        assert_eq!(tiles.iter().find(|tile| tile.id == ids[0]).unwrap().choice.icon, ToolGroup::Watercolor.icon());
        assert_eq!(tiles.iter().find(|tile| tile.id == ids[1]).unwrap().choice.icon, ToolGroup::Pen.icon());
        let DrawerAnchor::Header { id } = header else { unreachable!() };
        assert_eq!(s.header_view().items.iter().find(|item| item.id == id).unwrap().icon, ToolGroup::Pen.icon());
        let during = s.workspace_working_state();
        assert_eq!(during.canvas_tool, permanent.canvas_tool);
        assert_eq!(during.preset, permanent.preset);
        assert_eq!(during.tools, permanent.tools);
        key(&mut s, name, false, false, false);
        let queued = s.workspace_working_state();
        assert_eq!(queued.canvas_tool, permanent.canvas_tool);
        assert_eq!(queued.preset, permanent.preset);
        assert_eq!(queued.tools, permanent.tools);
        s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
        s.frame(3, 3).unwrap();
        assert!(s.command(CommandId::Hand).selected);
        assert_eq!(s.state().brush.preset, permanent.preset);
        assert_eq!(s.workspace_working_state().tools, permanent.tools);
        for anchor in [brush, drawing, header] {
            assert_eq!(selected_slot_variant(&s, anchor), watercolor);
        }
        if !modifier {
            key(&mut s, "p", true, false, false);
            let released = key(&mut s, "p", false, false, false);
            assert_eq!(released.change.regions & (regions::BRUSH | regions::COMMANDS), regions::BRUSH | regions::COMMANDS);
            assert!(s.command(CommandId::Pen).selected);
            assert_eq!(selected_slot_variant(&s, drawing), pen);
            assert_eq!(selected_slot_variant(&s, header), pen);
            assert_eq!(selected_slot_variant(&s, brush), watercolor);
            assert_eq!(s.header_view().items.iter().find(|item| item.id == id).unwrap().icon, ToolGroup::Pen.icon());
        } else {
            key(&mut s, "Alt_L", true, false, false);
            assert!(s.command(CommandId::Pen).selected);
            invoke(&mut s, CommandId::Eraser);
            assert!(s.command(CommandId::Eraser).selected);
            assert_eq!(s.workspace_working_state().tools.drawing(), permanent.tools.drawing());
            assert_eq!(selected_slot_variant(&s, drawing), watercolor);
            assert_eq!(selected_slot_variant(&s, header), watercolor);
            key(&mut s, "Alt_L", false, false, false);
            assert!(s.command(CommandId::Eraser).selected);
        }
    }
}

#[test]
fn tool_group_presets_leave_pinned_brushes_as_leaf_controls() {
    for command in [CommandId::Eraser, CommandId::Decoration, CommandId::Clone, CommandId::Heal, CommandId::SpotHeal, CommandId::Blend, CommandId::Liquify] {
        let control = ToolbarControl::Command { command };
        assert!(control.has_variants());
        let (mut s, panel, ids) = group_fixture(Platform::Gtk, &[control]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let menu = slot_menu(&s, anchor);
        assert!(!menu.is_empty());
        for item in menu {
            let Some(UiAction::ChooseToolVariant { variant: ToolVariant::BrushPreset { id }, .. }) = item.action else { panic!("single-medium groups publish brush presets") };
            assert_eq!(tools::group(id).tool().command(), command);
            s.dispatch(UiAction::ChooseToolVariant { anchor, variant: ToolVariant::BrushPreset { id } }).unwrap();
            assert_eq!(s.state().brush.preset, id);
            assert_eq!(selected_slot_variant(&s, anchor), ToolVariant::BrushPreset { id });
        }
    }
    for control in [ToolbarControl::Brush { id: Tool::Pen.default_preset() }, ToolbarControl::Command { command: CommandId::Hand }, ToolbarControl::Command { command: CommandId::Eyedropper }] {
        assert!(!control.has_variants());
        let (mut s, panel, ids) = group_fixture(Platform::Gtk, &[control]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let header = add_group_header(&mut s, control);
        assert!(!s.panel_view(panel).unwrap().tiles.iter().find(|item| item.id == ids[0]).unwrap().has_variants);
        let DrawerAnchor::Header { id } = header else { unreachable!() };
        assert!(!s.header_view().items.iter().find(|item| item.id == id).unwrap().has_variants);
        assert!(s.context_menu(ContextTarget::ToolVariants { anchor }).is_err());
        assert!(s.context_menu(ContextTarget::ToolVariants { anchor: header }).is_err());
    }
}

#[test]
fn tool_group_sketch_markers_memory_and_drawer_origins() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(platform), ..WorkspaceState::default()
        }) }).unwrap();
        let original_layout = crate::durable_layout(&s.state().workspace.layout);
        for (command, variant) in [
            (CommandId::DrawingBrush, ToolVariant::BrushGroup { group: ToolGroup::Marker }),
            (CommandId::Sculpt, ToolVariant::BrushGroup { group: ToolGroup::Liquify }),
            (CommandId::Select, ToolVariant::Command { command: CommandId::PolygonSelect }),
        ] {
            let control = ToolbarControl::Command { command };
            assert!(control.has_variants());
            let (panel, id) = s.state().workspace.layout.panels.iter().find_map(|panel|
                panel.tiles().iter().find(|tile| tile.control == control).map(|tile| (panel.id, tile.id))).unwrap();
            let header = s.state().workspace.layout.header.entries().find(|entry| entry.item == HeaderItem::Tool { control }).map(|entry| DrawerAnchor::Header { id: entry.id });
            let anchor = header.unwrap_or(DrawerAnchor::Tile { panel, tile: id });
            if let Some(DrawerAnchor::Header { id }) = header {
                s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![HeaderItemBounds {
                    id, bounds: Bounds { x: 800., y: 0., width: 40., height: 60. },
                }] }).unwrap();
            }
            assert!(s.panel_view(panel).unwrap().tiles.iter().find(|tile| tile.id == id).unwrap().has_variants);
            if let Some(DrawerAnchor::Header { id }) = header {
                assert!(s.header_view().items.iter().find(|item| item.id == id).unwrap().has_variants);
            }
            s.dispatch(slot_choice(&s, anchor, variant)).unwrap();
            assert_eq!(selected_slot_variant(&s, anchor), variant);
            let expected_icon = match variant { ToolVariant::BrushGroup { group } => group.icon(), _ => variant.command().icon().unwrap() };
            if let Some(DrawerAnchor::Header { id }) = header {
                let view = s.header_view();
                let item = view.items.iter().find(|item| item.id == id).unwrap();
                assert!(item.selected);
                assert_eq!(item.icon, expected_icon);
                assert_eq!(selected_slot_variant(&s, header.unwrap()), variant);
            }
            invoke(&mut s, CommandId::Hand);
            assert_eq!(s.panel_view(panel).unwrap().tiles.iter().find(|tile| tile.id == id).unwrap().choice.icon, expected_icon);
            assert_eq!(selected_slot_variant(&s, anchor), variant);
            if let Some(DrawerAnchor::Header { id }) = header {
                let view = s.header_view();
                let item = view.items.iter().find(|item| item.id == id).unwrap();
                assert!(!item.selected);
                assert_eq!(item.icon, expected_icon);
                assert_eq!(selected_slot_variant(&s, header.unwrap()), variant);
            }
            activate_slot(&mut s, anchor);
            assert!(s.state().customization.drawer.is_none());
            activate_slot(&mut s, anchor);
            let drawer = s.state().customization.drawer.as_ref().unwrap();
            assert_eq!(drawer.anchor, anchor);
            if matches!(command, CommandId::DrawingBrush | CommandId::Sculpt) { assert_eq!(drawer.columns.len(), 3); }
            let group_panel = match command {
                CommandId::DrawingBrush => Panel::BrushSets,
                CommandId::Sculpt => Panel::SculptSets,
                _ => Panel::Tools,
            };
            let choices = drawer.tool_set.as_ref().unwrap_or_else(|| s.state().tool_panel(group_panel));
            let sibling = choices.groups.iter().chain(&choices.subtools).find(|item| !item.selected).unwrap().action.clone();
            s.dispatch(sibling).unwrap();
            assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, anchor);
            activate_slot(&mut s, anchor);
            assert!(s.state().customization.drawer.is_none());
        }
        assert_eq!(crate::durable_layout(&s.state().workspace.layout), original_layout);
    }
}

#[test]
fn tool_group_selection_scopes_agree_without_overlap() {
    let manual = [CommandId::Lasso, CommandId::RectangleSelect, CommandId::EllipseSelect, CommandId::PolygonSelect, CommandId::SelectionBrush];
    let automatic = [CommandId::AutoSelect, CommandId::ColorSelect];
    assert!(!manual.iter().any(|command| automatic.contains(command)));
    for (control, commands) in [
        (ToolbarControl::ToolSlot { slot: ToolSlotId::ManualSelection }, &manual[..]),
        (ToolbarControl::ToolSlot { slot: ToolSlotId::AutomaticSelection }, &automatic[..]),
        (ToolbarControl::ToolSlot { slot: ToolSlotId::Marquee }, &[CommandId::RectangleSelect, CommandId::EllipseSelect][..]),
        (ToolbarControl::ToolSlot { slot: ToolSlotId::Lasso }, &[CommandId::Lasso, CommandId::PolygonSelect][..]),
        (ToolbarControl::Command { command: CommandId::Select }, &SelectionTool::ALL.map(SelectionTool::command)[..]),
    ] {
        let (mut s, panel, ids) = group_fixture(Platform::Gtk, &[control]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let expected: std::collections::BTreeSet<_> = commands.iter().map(|command| command.shortcut_id()).collect();
        let menu = slot_menu(&s, anchor);
        assert_eq!(menu.len(), commands.len());
        assert_eq!(menu.iter().map(|item| match item.action {
            Some(UiAction::ChooseToolVariant { variant, .. }) => variant.command().shortcut_id(),
            _ => panic!("selection siblings are anchored choices"),
        }).collect::<std::collections::BTreeSet<_>>(), expected);
        for &command in commands {
            s.dispatch(slot_choice(&s, anchor, ToolVariant::Command { command })).unwrap();
            let docked = s.state().tool_panel(Panel::Tools);
            assert_eq!(selection_group_commands(docked.groups.iter().chain(&docked.subtools)), expected);
            activate_slot(&mut s, anchor);
            let drawer = s.state().customization.drawer.as_ref().unwrap();
            assert_eq!(drawer.anchor, anchor);
            let view = drawer.tool_set.as_ref().unwrap_or_else(|| s.state().tool_panel(Panel::Tools));
            assert_eq!(selection_group_commands(view.groups.iter().chain(&view.subtools)), expected);
            assert_eq!(view.groups.iter().chain(&view.subtools).filter(|item| item.selected).count(), 1);
            activate_slot(&mut s, anchor);
            assert!(s.state().customization.drawer.is_none());
        }
    }
}

#[test]
fn tool_group_existing_nonpaint_commands_keep_layout_identity() {
    for (command, slot) in [
        (CommandId::Figure, ToolSlotId::Figure), (CommandId::Ruler, ToolSlotId::Ruler),
        (CommandId::Gradient, ToolSlotId::Gradient), (CommandId::Move, ToolSlotId::Operation),
        (CommandId::Fill, ToolSlotId::Fill),
    ] {
        let control = ToolbarControl::Command { command };
        assert!(control.has_variants());
        let (mut s, panel, ids) = group_fixture(Platform::Gtk, &[control]);
        if command == CommandId::Move {
            s.fill_selection(rectangle([100., 100., 300., 300.])).unwrap();
            s.frame(1, 1).unwrap();
        }
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let header = add_group_header(&mut s, control);
        let layout = s.state().workspace.layout.clone();
        assert_eq!(slot_menu(&s, anchor).len(), slot.variants().len());
        for &variant in slot.variants() {
            s.dispatch(slot_choice(&s, anchor, variant)).unwrap();
            crate::session::test_support::finish_fixture_content_bounds(&mut s);
            let view = s.panel_view(panel).unwrap();
            let tile = view.tiles.iter().find(|tile| tile.id == ids[0]).unwrap();
            assert!(tile.has_variants && tile.choice.selected);
            assert_eq!(tile.choice.control, control);
            assert_eq!(tile.choice.icon, variant.icon());
            assert_eq!(selected_slot_variant(&s, anchor), variant);
            assert_eq!(selected_slot_variant(&s, header), variant);
            if s.command(CommandId::CancelTransform).enabled { invoke(&mut s, CommandId::CancelTransform); }
            invoke(&mut s, CommandId::Hand);
            let view = s.panel_view(panel).unwrap();
            let tile = view.tiles.iter().find(|tile| tile.id == ids[0]).unwrap();
            assert!(!tile.choice.selected);
            assert_eq!(tile.choice.icon, variant.icon());
            assert_eq!(selected_slot_variant(&s, anchor), variant);
            assert_eq!(selected_slot_variant(&s, header), variant);
        }
        assert_eq!(s.state().workspace.layout, layout);
        let capture = s.capture_workspace().unwrap();
        let mut restored = session(Platform::Gtk);
        restored.adopt_workspace(PreparedWorkspace::new(capture).unwrap()).unwrap();
        assert_eq!(selected_slot_variant(&restored, anchor), *slot.variants().last().unwrap());
        assert_eq!(selected_slot_variant(&restored, header), *slot.variants().last().unwrap());
    }
}

#[test]
fn queued_header_measurements_cannot_overwrite_a_replaced_header() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        let old_items: Vec<_> = s.state().workspace.layout.header.entries().enumerate().map(|(index, entry)|
            HeaderItemBounds { id: entry.id, bounds: Bounds { x: index as f32 * 50., y: 0., width: 50., height: 60. } }).collect();
        assert!(old_items.len() > 1);
        let mut workspace = s.state().workspace.clone();
        let entry = workspace.layout.header.entries().next().unwrap().clone();
        workspace.layout.header.zones = [vec![entry], Vec::new(), Vec::new()];
        s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
        let current_items = vec![HeaderItemBounds { id: old_items[0].id,
            bounds: Bounds { x: 20., y: 0., width: 40., height: 48. } }];
        s.dispatch(UiAction::MeasureHeader { height: 48., items: current_items }).unwrap();
        let current = s.state().workspace.layout.header_presentation.clone();
        let revision = s.state().revision;
        let stale = UiAction::MeasureHeader { height: 60., items: old_items.clone() };
        let change = s.dispatch(stale.clone()).unwrap();
        assert_eq!(change.regions, 0);
        assert_eq!(s.state().revision, revision);
        assert_eq!(s.state().workspace.layout.header_presentation, current);
        let mut malformed = old_items.clone();
        malformed[0].bounds.x = -1.;
        assert!(s.dispatch(UiAction::MeasureHeader { height: 60., items: malformed }).is_err());
        assert!(s.dispatch(UiAction::MeasureHeader { height: f32::NAN, items: old_items.clone() }).is_err());
        assert!(s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![old_items[0], old_items[0]] }).is_err());
        assert_eq!(s.state().workspace.layout.header_presentation, current);
    }
}

#[test]
fn shipped_tool_slot_presets_keep_compact_counts_and_separate_pen_and_pencil() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows] {
        for (preset, count) in [(WorkspacePreset::Illustrator, 17), (WorkspacePreset::Photographer, 15)] {
            let layout = preset.layout(platform);
            let controls: Vec<_> = layout.panel(Panel::Toolbar).unwrap().tiles().iter().map(|tile| tile.control).collect();
            assert_eq!(controls.iter().filter(|control| !matches!(control, ToolbarControl::Color | ToolbarControl::Divider)).count(), count);
            if preset == WorkspacePreset::Illustrator {
                for command in [CommandId::Pen, CommandId::Pencil] {
                    assert!(controls.contains(&ToolbarControl::Command { command }));
                }
            }
            layout.validate().unwrap();
        }
        let sketch = WorkspacePreset::Painter.layout(platform);
        assert!(!sketch.panels.iter().flat_map(|panel| panel.tiles()).any(|tile|
            matches!(tile.control, ToolbarControl::ToolSlot { .. })));
        assert!(!sketch.header.entries().any(|entry|
            matches!(entry.item, HeaderItem::Tool { control: ToolbarControl::ToolSlot { .. } })));
    }
}

#[test]
fn slot_menus_publish_only_member_choices_and_activate_their_leaf_tools() {
    let slots = [ToolSlotId::Drawing, ToolSlotId::Marquee, ToolSlotId::Lasso,
        ToolSlotId::AutomaticSelection, ToolSlotId::ManualSelection, ToolSlotId::Healing,
        ToolSlotId::PhotoFill, ToolSlotId::Fill, ToolSlotId::Blend, ToolSlotId::Operation,
        ToolSlotId::Figure, ToolSlotId::Ruler, ToolSlotId::Gradient];
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows] {
        let (mut s, panel, ids) = slot_fixture(platform, &slots);
        for (slot, tile) in slots.into_iter().zip(ids) {
            let anchor = DrawerAnchor::Tile { panel, tile };
            let menu = slot_menu(&s, anchor);
            assert_eq!(menu.len(), slot.variants().len(), "{platform:?} {slot:?}");
            for (item, variant) in menu.iter().zip(slot.variants()) {
                assert!(item.sections.is_empty());
                assert!(!item.label.is_empty());
                assert_eq!(item.action, Some(UiAction::ChooseToolVariant { anchor, variant: *variant }));
                assert!(item.selected.is_some());
            }
            for variant in slot.variants() {
                let item = slot_menu(&s, anchor).into_iter().find(|item|
                    item.action == Some(UiAction::ChooseToolVariant { anchor, variant: *variant })).unwrap();
                if item.enabled {
                    s.dispatch(item.action.unwrap()).unwrap();
                    crate::session::test_support::finish_fixture_content_bounds(&mut s);
                    if variant.command() == CommandId::ScaleRotate {
                        assert_eq!(s.state().layer_tools.tool, LayerCanvasTool::Transform);
                    } else {
                        assert!(s.command(variant.command()).selected, "{platform:?} {slot:?} {variant:?}");
                    }
                    assert_eq!(selected_slot_variant(&s, anchor), *variant);
                    if s.command(CommandId::CancelTransform).enabled { invoke(&mut s, CommandId::CancelTransform); }
                }
            }
            let view = s.panel_view(panel).unwrap();
            let item = view.tiles.iter().find(|item| item.id == tile).unwrap();
            assert!(item.has_variants);
            assert_eq!(item.choice.control, ToolbarControl::ToolSlot { slot });
        }
    }
}

#[test]
fn choosing_a_variant_selects_once_and_active_slot_clicks_keep_settings_drawers() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows] {
        let (mut s, panel, ids) = slot_fixture(platform, &[ToolSlotId::Drawing, ToolSlotId::Gradient]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
        s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
        assert!(s.command(CommandId::Airbrush).selected);
        assert!(s.state().customization.drawer.is_none());
        let chosen = s.panel_view(panel).unwrap().tiles.into_iter().find(|item| item.id == ids[0]).unwrap();
        assert!(chosen.choice.selected);
        assert_eq!(chosen.choice.icon, s.command(CommandId::Airbrush).icon.unwrap());
        invoke(&mut s, CommandId::Eraser);
        activate_slot(&mut s, anchor);
        assert!(s.command(CommandId::Airbrush).selected);
        assert!(s.state().customization.drawer.is_none());
        activate_slot(&mut s, anchor);
        let drawer = s.state().customization.drawer.as_ref().unwrap();
        assert_eq!(drawer.anchor, anchor);
        assert!(drawer.columns.iter().flatten().any(|panel| *panel == Panel::ToolSettings));
        let pencil = ToolVariant::Command { command: CommandId::Pencil };
        s.dispatch(slot_choice(&s, anchor, pencil)).unwrap();
        assert_eq!(selected_slot_variant(&s, anchor), pencil);
        assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, anchor);
        activate_slot(&mut s, anchor);
        assert!(s.state().customization.drawer.is_none());
    }
}

#[test]
fn grouped_slot_drawers_publish_all_siblings_and_the_active_brush_presets() {
    for slot in ToolSlotId::ALL {
        let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[slot]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let variant = slot.variants()[0];
        s.dispatch(slot_choice(&s, anchor, variant)).unwrap();
        crate::session::test_support::finish_fixture_content_bounds(&mut s);
        activate_slot(&mut s, anchor);
        let drawer = s.state().customization.drawer.as_ref().unwrap();
        assert_eq!(drawer.columns, [vec![Panel::Brushes], vec![Panel::ToolSettings]]);
        let choices = drawer.tool_set.as_ref().unwrap();
        assert_eq!(choices.groups.len(), slot.variants().len());
        assert_eq!(choices.groups.iter().filter(|choice| choice.selected).count(), 1);
        for (choice, &variant) in choices.groups.iter().zip(slot.variants()) {
            assert_eq!(choice.action, UiAction::ChooseToolVariant { anchor, variant });
            assert!(!choice.label.is_empty());
            assert!(!choice.icon.is_empty());
        }
        if s.state().layer_tools.tool == LayerCanvasTool::Paint {
            assert!(!choices.subtools.is_empty());
            assert_eq!(choices.subtools.iter().filter(|choice|
                choice.selected && matches!(choice.action, UiAction::SelectBrush { .. })).count(), 1);
            for choice in &choices.subtools {
                match choice.action {
                    UiAction::SelectBrush { id } => {
                        assert_eq!(tools::group(id).tool(), s.state().brush.tool);
                        if choice.selected { assert_eq!(id, s.state().brush.preset); }
                    }
                    UiAction::SelectToolGroup { group } => assert_eq!(group.tool(), s.state().brush.tool),
                    _ => panic!("brush subtools expose mediums and presets"),
                }
            }
        }
    }
}

#[test]
fn title_bar_slots_use_the_same_choices_and_resolved_presentation() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Windows] {
        let (mut s, panel, ids) = slot_fixture(platform, &[ToolSlotId::Drawing]);
        customize(&mut s, CustomizationAction::Header { action: HeaderAction::Add {
            zone: HeaderZone::Left, before: None,
            item: HeaderItem::Tool { control: ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing } },
        } });
        let id = s.state().workspace.layout.header.entries().find(|entry|
            entry.item == HeaderItem::Tool { control: ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing } }).unwrap().id;
        let anchor = DrawerAnchor::Header { id };
        let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
        let tile_anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        assert_eq!(slot_menu(&s, anchor).len(), slot_menu(&s, tile_anchor).len());
        s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
        let view = s.header_view();
        let item = view.items.iter().find(|item| item.id == id).unwrap();
        assert!(item.has_variants && item.enabled && item.selected);
        assert_eq!(item.icon, s.command(CommandId::Airbrush).icon.unwrap());
        assert!(!item.label.is_empty());
        s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![HeaderItemBounds {
            id, bounds: Bounds { x: 800., y: 0., width: 40., height: 60. },
        }] }).unwrap();
        invoke(&mut s, CommandId::Eraser);
        activate_slot(&mut s, anchor);
        assert!(s.command(CommandId::Airbrush).selected);
        assert!(s.state().customization.drawer.is_none());
        activate_slot(&mut s, anchor);
        assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, anchor);
    }
}

#[test]
fn family_shortcuts_cycle_full_membership_and_held_use_preserves_slot_memory() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    for &command in ToolFamily::Paint.commands().iter().cycle().take(ToolFamily::Paint.commands().len() * 2) {
        if command == ToolFamily::Paint.commands()[0] { invoke(&mut s, CommandId::Eraser); }
        key(&mut s, "b", true, false, false);
        key(&mut s, "b", false, false, false);
        assert!(s.command(command).selected);
    }
    let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
    s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
    key(&mut s, "b", true, false, false);
    assert!(s.command(CommandId::Decoration).selected);
    s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
    s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
    key(&mut s, "b", false, false, false);
    s.frame(3, 3).unwrap();
    assert!(s.command(CommandId::Airbrush).selected);
    assert_eq!(selected_slot_variant(&s, anchor), airbrush);
    invoke(&mut s, CommandId::Eraser);
    activate_slot(&mut s, anchor);
    assert!(s.command(CommandId::Airbrush).selected);
}

#[test]
fn tapping_family_keys_remembers_choices_and_blurring_held_use_restores_them() {
    for blur in [false, true] {
        let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        invoke(&mut s, CommandId::Eraser);
        key(&mut s, "b", true, false, false);
        key(&mut s, "b", false, false, false);
        let brush = ToolVariant::Command { command: CommandId::Brush };
        invoke(&mut s, CommandId::Eraser);
        assert_eq!(selected_slot_variant(&s, anchor), brush);
        activate_slot(&mut s, anchor);
        key(&mut s, "b", true, false, false);
        assert!(s.command(CommandId::Airbrush).selected);
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
        if blur { s.input(UiInput::Blur).unwrap(); }
        else { key(&mut s, "b", false, false, false); }
        s.frame(3, 3).unwrap();
        assert!(s.command(CommandId::Brush).selected);
        invoke(&mut s, CommandId::Eraser);
        assert_eq!(selected_slot_variant(&s, anchor), brush);
    }
}

#[test]
fn workspace_capture_during_spring_and_modifier_holds_keeps_the_permanent_tool() {
    for modifier in [false, true] {
        let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing, ToolSlotId::Marquee]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
        s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
        s.dispatch(UiAction::SetBrushSize { value: 73. }).unwrap();
        let permanent = s.capture_workspace().unwrap();
        if modifier {
            let mut settings = s.state().settings.clone();
            let mut holds = settings.hold_keys(Platform::Gtk);
            let hold = holds.iter_mut().find(|hold|
                hold.key == KeyChord::new("alt", Modifiers::default())).unwrap();
            hold.actions.insert(ToolCategory::Drawing, CommandId::Eraser.shortcut_id());
            settings.hold_keys = Some(holds);
            s.dispatch(UiAction::RestoreSettings { settings }).unwrap();
            key(&mut s, "Alt_L", true, false, false);
            assert!(s.command(CommandId::Eraser).selected);
        } else {
            key(&mut s, "b", true, false, false);
            assert!(s.command(CommandId::Decoration).selected);
        }
        s.pen(event(&s, 1, PenPhase::Down, 1.)).unwrap();
        let during = s.capture_workspace().unwrap();
        assert_eq!(during.working.preset, permanent.working.preset);
        assert_eq!(during.working.canvas_tool, permanent.working.canvas_tool);
        assert_eq!(during.working.tool_slots, permanent.working.tool_slots);
        assert_eq!(during.working.tools.drawing(), permanent.working.tools.drawing());
        let mut restored = session(Platform::Gtk);
        restored.adopt_workspace(PreparedWorkspace::new(during).unwrap()).unwrap();
        assert!(restored.command(CommandId::Airbrush).selected);
        assert_eq!(restored.state().brush.diameter, 73.);
        if modifier { key(&mut s, "Alt_L", false, false, false); }
        else { key(&mut s, "b", false, false, false); }
        let pending = s.capture_workspace().unwrap();
        assert_eq!(pending.working.preset, permanent.working.preset);
        assert_eq!(pending.working.canvas_tool, permanent.working.canvas_tool);
        assert_eq!(pending.working.tool_slots, permanent.working.tool_slots);
        s.pen(event(&s, 2, PenPhase::Up, 1.)).unwrap();
        s.frame(3, 3).unwrap();
        assert!(s.command(CommandId::Airbrush).selected);
    }
}

#[test]
fn disabled_slot_choices_are_revalidated_before_they_change_memory() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    let remembered = selected_slot_variant(&s, anchor);
    let other = *ToolSlotId::Drawing.variants().iter().find(|&&variant| variant != remembered).unwrap();
    let stale_action = slot_choice(&s, anchor, other);
    assert!(s.dispatch(UiAction::ChooseToolVariant {
        anchor, variant: ToolVariant::Command { command: CommandId::Undo },
    }).is_err());
    assert_eq!(selected_slot_variant(&s, anchor), remembered);
    s.suspend_renderer().unwrap();
    assert!(slot_menu(&s, anchor).iter().all(|item| !item.enabled));
    assert!(s.dispatch(stale_action).is_err());
    assert_eq!(selected_slot_variant(&s, anchor), remembered);
    assert!(s.dispatch(UiAction::ChooseToolVariant {
        anchor, variant: ToolVariant::Command { command: CommandId::Undo },
    }).is_err());
}

#[test]
fn slot_choices_round_trip_with_independent_workspace_working_state() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Marquee, ToolSlotId::Gradient]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    let original = selected_slot_variant(&s, anchor);
    let blank = s.capture_workspace().unwrap();
    let other = *ToolSlotId::Marquee.variants().iter().find(|&&variant| variant != original).unwrap();
    s.dispatch(slot_choice(&s, anchor, other)).unwrap();
    let gradient_anchor = DrawerAnchor::Tile { panel, tile: ids[1] };
    let gradient = *ToolSlotId::Gradient.variants().last().unwrap();
    s.dispatch(slot_choice(&s, gradient_anchor, gradient)).unwrap();
    let edited = s.capture_workspace().unwrap();
    assert_eq!(blank.history, edited.history, "variation choices are working state");
    s.adopt_workspace(PreparedWorkspace::new(blank).unwrap()).unwrap();
    assert_eq!(selected_slot_variant(&s, anchor), original);
    let encoded = serde_json::to_string(&edited).unwrap();
    let mut restored = session(Platform::Gtk);
    restored.adopt_workspace(PreparedWorkspace::new(serde_json::from_str(&encoded).unwrap()).unwrap()).unwrap();
    assert_eq!(selected_slot_variant(&restored, anchor), other);
    assert_eq!(selected_slot_variant(&restored, gradient_anchor), gradient);
    invoke(&mut restored, CommandId::Eraser);
    activate_slot(&mut restored, anchor);
    assert!(restored.command(other.command()).selected);
}

#[test]
fn removing_slots_invalidates_open_menu_actions_and_undo_restores_the_choice() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
    s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
    let stale_action = slot_choice(&s, anchor, ToolVariant::Command { command: CommandId::Pen });
    customize(&mut s, CustomizationAction::RemoveTool { panel, tile: ids[0] });
    assert!(s.context_menu(ContextTarget::ToolVariants { anchor }).is_err());
    assert!(s.dispatch(stale_action).is_err());
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(selected_slot_variant(&s, anchor), airbrush);
    invoke(&mut s, CommandId::Eraser);
    activate_slot(&mut s, anchor);
    assert!(s.command(CommandId::Airbrush).selected);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert!(s.context_menu(ContextTarget::ToolVariants { anchor }).is_err());
}

#[test]
fn duplicated_slot_toolbars_copy_memory_and_pinned_tools_stay_single_choices() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
    s.dispatch(slot_choice(&s, anchor, airbrush)).unwrap();
    invoke(&mut s, CommandId::Eraser);
    customize(&mut s, CustomizationAction::DuplicateToolbar { panel });
    customize(&mut s, CustomizationAction::ConfirmToolbar);
    let duplicate = s.state().workspace.layout.panels.last().unwrap();
    assert_ne!(duplicate.id, panel);
    let copy = DrawerAnchor::Tile { panel: duplicate.id, tile: duplicate.tiles()[0].id };
    invoke(&mut s, CommandId::Eraser);
    assert_eq!(selected_slot_variant(&s, copy), airbrush);
    let pencil = ToolVariant::Command { command: CommandId::Pencil };
    s.dispatch(slot_choice(&s, copy, pencil)).unwrap();
    invoke(&mut s, CommandId::Eraser);
    assert_eq!(selected_slot_variant(&s, anchor), pencil, "permanent choices synchronize matching slots");
    assert_eq!(selected_slot_variant(&s, copy), pencil);
    let pinned_control = ToolbarControl::Brush { id: Tool::Airbrush.default_preset() };
    customize(&mut s, CustomizationAction::InsertTools { panel, before: None });
    customize(&mut s, CustomizationAction::PickerSelect {
        control: pinned_control, selected: true,
    });
    customize(&mut s, CustomizationAction::ConfirmTools);
    let pinned = s.state().workspace.layout.panel(panel).unwrap().tiles().iter().find(|tile|
        tile.control == pinned_control).unwrap().id;
    let pinned_view = s.panel_view(panel).unwrap().tiles.into_iter().find(|tile| tile.id == pinned).unwrap();
    assert!(!pinned_view.has_variants);
    assert!(s.context_menu(ContextTarget::ToolVariants { anchor: DrawerAnchor::Tile { panel, tile: pinned } }).is_err());
    s.dispatch(UiAction::ActivateTile { panel, tile: pinned }).unwrap();
    assert!(s.command(CommandId::Airbrush).selected);
    invoke(&mut s, CommandId::Eraser);
    assert_eq!(selected_slot_variant(&s, copy), airbrush);
}

#[test]
fn plain_transform_tools_select_after_activation_and_open_on_the_second_click() {
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        let mut workspace = s.state().workspace.clone();
        let panel = workspace.layout.add_toolbar(None, "Transform", &[ToolbarControl::Command { command: CommandId::ScaleRotate }]).unwrap();
        let tile = workspace.layout.panel(panel).unwrap().tiles()[0].id;
        let anchor = DrawerAnchor::Tile { panel, tile };
        s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
        assert!(!s.command(CommandId::ScaleRotate).selected);
        activate_slot(&mut s, anchor);
        assert!(s.content_bounds.busy());
        assert!(!s.command(CommandId::ScaleRotate).selected);
        s.engine.backend_mut().bounds_reply = Some(Ok(layer_core::Rect::EMPTY));
        s.frame(1, 1).unwrap();
        assert!(!s.content_bounds.busy());
        assert!(!s.command(CommandId::ScaleRotate).selected);
        assert!(s.state().customization.drawer.is_none());
        assert!(s.dispatch(UiAction::ActivateTile { panel, tile }).is_err());
        assert!(!s.command(CommandId::ScaleRotate).selected);
        s.fill_selection(rectangle([100., 100., 300., 300.])).unwrap();
        s.frame(2, 2).unwrap();
        activate_slot(&mut s, anchor);
        crate::session::test_support::finish_fixture_content_bounds(&mut s);
        assert!(s.command(CommandId::ScaleRotate).selected);
        assert!(s.command(CommandId::Move).selected);
        assert!(s.panel_view(panel).unwrap().tiles[0].choice.selected);
        assert!(s.state().customization.drawer.is_none());
        activate_slot(&mut s, anchor);
        let drawer = s.state().customization.drawer.as_ref().unwrap();
        assert_eq!(drawer.anchor, anchor);
        assert_eq!(drawer.columns, [vec![Panel::Brushes], vec![Panel::ToolSettings]]);
        activate_slot(&mut s, anchor);
        assert!(s.state().customization.drawer.is_none());
        invoke(&mut s, CommandId::CancelTransform);
        assert!(!s.command(CommandId::ScaleRotate).selected);
        s.suspend_renderer().unwrap();
        assert!(!s.command(CommandId::ScaleRotate).enabled);
        assert!(s.dispatch(UiAction::ActivateTile { panel, tile }).is_err());
        assert!(!s.command(CommandId::ScaleRotate).selected);
    }
}

#[test]
fn asynchronous_transform_slot_choices_commit_only_after_successful_activation() {
    for succeeds in [false, true] {
        let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Operation]);
        let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
        s.fill_selection(rectangle([100., 100., 300., 300.])).unwrap();
        s.frame(1, 1).unwrap();
        let before = selected_slot_variant(&s, anchor);
        let transform = ToolVariant::Command { command: CommandId::ScaleRotate };
        s.dispatch(slot_choice(&s, anchor, transform)).unwrap();
        assert!(s.content_bounds.busy());
        assert_eq!(selected_slot_variant(&s, anchor), before);
        if succeeds {
            crate::session::test_support::finish_fixture_content_bounds(&mut s);
            assert_eq!(s.state().layer_tools.tool, LayerCanvasTool::Transform);
            assert_eq!(selected_slot_variant(&s, anchor), transform);
            invoke(&mut s, CommandId::CancelTransform);
        } else {
            s.engine.backend_mut().bounds_reply = Some(Err(layer_render::BackendError("bounds failed")));
            s.frame(2, 2).unwrap();
            assert!(!s.content_bounds.busy());
            assert_eq!(selected_slot_variant(&s, anchor), before);
        }
        invoke(&mut s, CommandId::Eraser);
        assert_eq!(selected_slot_variant(&s, anchor), if succeeds { transform } else { before });
    }
}

#[test]
fn tool_options_drawers_follow_variants_and_revalidate_moved_or_removed_origins() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing, ToolSlotId::Marquee]);
    let mut workspace = s.state().workspace.clone();
    workspace.layout.insert_tools(panel, None, &[ToolbarControl::TOOL_OPTIONS]).unwrap();
    let options = workspace.layout.panel(panel).unwrap().tiles().last().unwrap().id;
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
    let drawing = DrawerAnchor::Tile { panel, tile: ids[0] };
    let marquee = DrawerAnchor::Tile { panel, tile: ids[1] };
    let opener = DrawerAnchor::Tile { panel, tile: options };
    let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
    s.dispatch(slot_choice(&s, drawing, airbrush)).unwrap();
    activate_slot(&mut s, opener);
    assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, opener);
    assert_eq!(s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap().groups.len(), ToolSlotId::Drawing.variants().len());
    s.dispatch(slot_choice(&s, marquee, ToolSlotId::Marquee.variants()[1])).unwrap();
    assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, opener);
    assert_eq!(s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap().groups.len(), ToolSlotId::Marquee.variants().len());
    s.dispatch(slot_choice(&s, drawing, airbrush)).unwrap();
    let stale = slot_choice(&s, drawing, ToolVariant::Command { command: CommandId::Pen });
    s.dispatch(UiAction::MoveTile {
        panel, tile: ids[0], target: DockTarget::Tile { panel: Panel::Toolbar, before: None }, viewport: [1200., 900.],
    }).unwrap();
    let moved = DrawerAnchor::Tile { panel: Panel::Toolbar, tile: ids[0] };
    assert_eq!(s.state().customization.drawer.as_ref().unwrap().anchor, opener);
    assert!(s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap().groups.iter().all(|choice|
        matches!(choice.action, UiAction::ChooseToolVariant { anchor, .. } if anchor == moved)));
    assert!(s.dispatch(stale).is_err());
    customize(&mut s, CustomizationAction::RemoveTool { panel: Panel::Toolbar, tile: ids[0] });
    let drawer = s.state().customization.drawer.as_ref().unwrap();
    assert_eq!(drawer.anchor, opener);
    assert!(drawer.tool_set.is_none());
}

#[test]
fn grouped_slot_drawer_availability_tracks_workspace_and_renderer_state() {
    let (mut s, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    invoke(&mut s, CommandId::Eraser);
    activate_slot(&mut s, anchor);
    activate_slot(&mut s, anchor);
    let available = |s: &UiSession<Recorder>| s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap().groups.iter().map(|choice| choice.enabled).collect::<Vec<_>>();
    assert!(available(&s).iter().all(|enabled| *enabled));
    s.set_workspace_read_only(true);
    assert!(available(&s).iter().all(|enabled| !enabled));
    s.set_workspace_read_only(false);
    assert!(available(&s).iter().all(|enabled| *enabled));
    s.suspend_renderer().unwrap();
    assert!(available(&s).iter().all(|enabled| !enabled));
}

#[test]
fn window_workspace_inheritance_replaces_slot_memory_and_keeps_document_tool_state() {
    let (mut previous, panel, ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing, ToolSlotId::Marquee]);
    let anchor = DrawerAnchor::Tile { panel, tile: ids[0] };
    let airbrush = ToolVariant::Command { command: CommandId::Airbrush };
    previous.dispatch(slot_choice(&previous, anchor, airbrush)).unwrap();
    invoke(&mut previous, CommandId::RectangleSelect);
    for parked in [false, true] {
        let mut next = if parked {
            let (mut s, old_panel, old_ids) = slot_fixture(Platform::Gtk, &[ToolSlotId::Drawing]);
            let old = DrawerAnchor::Tile { panel: old_panel, tile: old_ids[0] };
            s.dispatch(slot_choice(&s, old, ToolVariant::Command { command: CommandId::Pencil })).unwrap();
            invoke(&mut s, CommandId::Eraser);
            s
        } else { session(Platform::Gtk) };
        let document_tool = next.state().layer_tools.tool;
        let document_brush = next.state().brush.preset;
        next.inherit_window_state(&previous).unwrap();
        assert_eq!(next.state().layer_tools.tool, document_tool);
        assert_eq!(next.state().brush.preset, document_brush);
        assert_eq!(next.state().tool_slots, previous.state().tool_slots);
        invoke(&mut next, CommandId::Eraser);
        activate_slot(&mut next, anchor);
        assert!(next.command(CommandId::Airbrush).selected);
    }
}
