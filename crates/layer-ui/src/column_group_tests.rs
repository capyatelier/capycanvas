// Included in session::tests: exercise the public actions and publication contract.
fn group_edit(s: &mut UiSession<Recorder>, action: CustomizationAction) {
    s.dispatch(UiAction::Customize { action }).unwrap();
}
fn group_fixture() -> (UiSession<Recorder>, u32, u32, Panel) {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let group = s
        .state
        .workspace
        .layout
        .panel_group(Panel::Brushes)
        .unwrap();
    group_edit(
        &mut s,
        CustomizationAction::SetColumnCollapsed {
            group,
            collapsed: true,
        },
    );
    let column = s
        .state
        .workspace
        .layout
        .collapsed_column_for_group(group)
        .unwrap();
    group_edit(
        &mut s,
        CustomizationAction::SetColumnMode {
            column,
            mode: ColumnMode::GroupPanel,
        },
    );
    (s, column, group, Panel::Brushes)
}
#[test]
fn group_panel_shifts_neighbors_stacks_all_tabs_and_restores_closed_geometry() {
    let (mut s, column, group, panel) = group_fixture();
    let viewport = [1800., 1100.];
    let before = s.layout(viewport);
    group_edit(
        &mut s,
        CustomizationAction::ToggleColumnDrawer { group, panel },
    );
    let resolved = s.layout(viewport);
    let c = resolved.collapsed.iter().find(|c| c.id == column).unwrap();
    let p = c.group_panel.as_ref().unwrap();
    assert_eq!(
        p.panels.iter().map(|p| p.panel).collect::<Vec<_>>(),
        s.state.workspace.layout.group_panels(group).unwrap()
    );
    assert_eq!(p.bounds.y, c.bounds.y);
    assert_eq!(p.bounds.height, c.bounds.height);
    let max_width = s
        .state
        .workspace
        .layout
        .panels
        .iter()
        .filter(|p| {
            s.state.workspace.layout.panel_group(p.id).is_some_and(|g| {
                s.state.workspace.layout.collapsed_column_for_group(g) == Some(column)
            })
        })
        .map(|p| p.id.drawer_width())
        .fold(0., f32::max);
    assert!((p.bounds.width - max_width).abs() < 0.01);
    for g in &resolved.groups {
        assert!(
            g.bounds.intersection(p.bounds).is_none(),
            "group {} overlaps open column: {:?} {:?}",
            g.id,
            g.bounds,
            p.bounds
        );
    }
    assert!(
        (before.work_area.width - resolved.work_area.width - max_width - WORKSPACE_SPACING).abs()
            < 0.01
    );
    let settings = s.state.workspace.layout.column_settings(column);
    group_edit(
        &mut s,
        CustomizationAction::ToggleColumnDrawer { group, panel },
    );
    assert_eq!(
        serde_json::to_value(s.layout(viewport)).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(
        s.state.workspace.layout.column_settings(column).width,
        settings.width
    );
}
#[test]
fn group_panel_resize_retains_content_cancels_and_roundtrips_sizes() {
    let (mut s, _, group, panel) = group_fixture();
    let viewport = [1800., 1100.];
    // Make a multi-panel group so both split and width handles are exercised.
    s.dispatch(UiAction::MovePanel {
        panel: Panel::Sizes,
        target: DockTarget::Tab { group, index: None },
        viewport,
    })
    .unwrap();
    let column = s
        .state
        .workspace
        .layout
        .collapsed_column_for_group(group)
        .unwrap();
    group_edit(
        &mut s,
        CustomizationAction::ToggleColumnDrawer { group, panel },
    );
    for after in [None, Some(panel)] {
        let p = s
            .layout(viewport)
            .collapsed
            .into_iter()
            .find(|c| c.id == column)
            .unwrap()
            .group_panel
            .unwrap();
        let bounds = after.map_or(p.resize, |_| p.dividers[0]);
        let start = [
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height * 0.5,
        ];
        let initial = s.state.workspace.clone();
        let drag = |s: &mut UiSession<Recorder>, phase, position| {
            s.dispatch(UiAction::ResizeColumnPanel {
                column,
                after,
                phase,
                position,
                viewport,
            })
            .unwrap();
        };
        drag(&mut s, ContactPhase::Down, start);
        let revision = s.workspace_content_revision();
        for i in 1..=20 {
            let mut at = start;
            at[usize::from(after.is_some())] += i as f32 * 2.;
            drag(&mut s, ContactPhase::Move, at);
            assert_eq!(
                s.workspace_content_revision(),
                revision,
                "resize rebuilt retained contents"
            );
        }
        drag(&mut s, ContactPhase::Cancel, start);
        assert_eq!(s.state.workspace, initial);
        drag(&mut s, ContactPhase::Down, start);
        let mut end = start;
        end[usize::from(after.is_some())] += 70.;
        drag(&mut s, ContactPhase::Up, end);
        let committed = crate::durable_layout(&s.state.workspace.layout);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(
            crate::durable_layout(&s.state.workspace.layout),
            crate::durable_layout(&initial.layout)
        );
        invoke(&mut s, CommandId::RedoWorkspace);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), committed);
    }
    let saved = s.state.workspace.layout.column_settings(column);
    group_edit(&mut s, CustomizationAction::CloseColumn { column });
    group_edit(
        &mut s,
        CustomizationAction::ToggleColumnDrawer { group, panel },
    );
    assert_eq!(s.state.workspace.layout.column_settings(column), saved);
    let json = serde_json::to_string(&s.state.workspace).unwrap();
    let roundtrip: WorkspaceState = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.layout.column_settings(column).width, saved.width);
    assert_eq!(
        roundtrip.layout.column_settings(column).heights,
        saved.heights
    );
    assert!(
        roundtrip
            .layout
            .column_settings(column)
            .open_group
            .is_none()
    );
}
#[test]
fn column_auto_hide_both_modes_and_apply_all_only_copies_preferences() {
    let (mut s, column, group, panel) = group_fixture();
    group_edit(
        &mut s,
        CustomizationAction::SetColumnAutoHide {
            column,
            auto_hide: true,
        },
    );
    let other = s
        .state
        .workspace
        .layout
        .column_roots()
        .into_iter()
        .find(|c| *c != column)
        .unwrap();
    s.state.workspace.layout.column_settings_mut(other).width = Some(380.);
    group_edit(&mut s, CustomizationAction::ApplyColumnSettings { column });
    assert_eq!(
        s.state.workspace.layout.column_settings(other).width,
        Some(380.)
    );
    assert!(s.state.workspace.layout.column_settings(other).auto_hide);
    assert_eq!(
        s.state.workspace.layout.column_settings(other).mode,
        ColumnMode::GroupPanel
    );
    let menu = s.context_menu(ContextTarget::Column { column }).unwrap();
    assert!(
        menu.sections
            .iter()
            .flatten()
            .any(|i| i.label == "Apply to all columns")
    );
    for mode in [ColumnMode::GroupPanel, ColumnMode::Drawers] {
        group_edit(&mut s, CustomizationAction::SetColumnMode { column, mode });
        group_edit(
            &mut s,
            CustomizationAction::ToggleColumnDrawer { group, panel },
        );
        let drawer = &s.state.customization.column_drawers[0];
        assert_eq!(drawer.dismissal, DrawerDismissal::OutsideContact);
        let reply = s
            .input(UiInput::Chrome {
                event: ChromeEvent::Contact {
                    position: [900., 800.],
                    canvas: true,
                },
                facts: ChromeFacts::default(),
                viewport: [1800., 1100.],
            })
            .unwrap();
        assert!(reply.handled);
        assert!(s.state.customization.column_drawers.is_empty());
        assert!(
            s.state
                .workspace
                .layout
                .column_settings(column)
                .open_group
                .is_none()
        );
    }
}

#[test]
fn group_panel_drag_cancellation_restores_open_projection_and_preferences() {
    let (mut s, column, group, panel) = group_fixture();
    let viewport = [1800., 1100.];
    group_edit(&mut s, CustomizationAction::ToggleColumnDrawer { group, panel });
    let resolved = s.layout(viewport);
    let c = resolved.collapsed.iter().find(|c| c.id == column).unwrap();
    let icon = c.groups.iter().find(|g| g.group == group).unwrap().icons[0].bounds;
    let bounds = c.group_panel.as_ref().unwrap().bounds;
    s.dispatch(UiAction::MeasureColumnDrawers {
        measurements: vec![ColumnDrawerMeasurement { group, bounds }],
    }).unwrap();
    let before = s.state.workspace.clone();
    let at = [icon.x + icon.width * 0.5, icon.y + icon.height * 0.5];
    for (phase, position) in [(ContactPhase::Down, at), (ContactPhase::Move, [750.,400.]), (ContactPhase::Cancel, [750.,400.])] {
        s.dispatch(UiAction::DragWorkspace { item: DockItem::Panel { panel }, phase, position, viewport, tabs: vec![] }).unwrap();
    }
    assert_eq!(s.state.workspace, before);
    assert!(s.state.customization.column_drawers.iter().any(ContentDrawer::is_group_panel));
    assert!(s.layout(viewport).collapsed.iter().find(|c| c.id == column).unwrap().group_panel.is_some());
}
