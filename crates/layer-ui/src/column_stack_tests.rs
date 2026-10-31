const STACK_VIEW: [f32; 2] = [1800., 1100.];

fn measured_divider_session() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.state.workspace.layout.bands.iter_mut().find(|band| band.id == 3).unwrap().extent = 420.;
    s.dispatch(UiAction::MeasureHeader { height: HEADER_HEIGHT + 24., items: Vec::new() }).unwrap();
    s
}

#[test]
fn divider_geometry_measured_header_current_center_is_noop() {
    let mut failures = Vec::new();
    for axis in [Axis::Vertical, Axis::Horizontal] {
        let mut s = measured_divider_session();
        let divider = s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.axis == axis).unwrap();
        let center = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + divider.bounds.height * 0.5];
        let before = crate::durable_layout(&s.state.workspace.layout);
        s.state.workspace.layout.resize_workspace(divider.id, center, STACK_VIEW).unwrap();
        if crate::durable_layout(&s.state.workspace.layout) != before {
            failures.push(format!("{axis:?} divider {} changed at its current measured-header center {center:?}", divider.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
fn divider_geometry_measured_header_moves_twelve_pixels_in_both_axes() {
    let mut failures = Vec::new();
    for axis in [Axis::Vertical, Axis::Horizontal] {
        for delta in [-12., 12.] {
            let mut s = measured_divider_session();
            let divider = s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.axis == axis).unwrap();
            let dimension = usize::from(axis == Axis::Vertical);
            let center = |d: &Divider| [d.bounds.x + d.bounds.width * 0.5, d.bounds.y + d.bounds.height * 0.5];
            let start = center(&divider);
            let mut target = start; target[dimension] += delta;
            s.state.workspace.layout.resize_workspace(divider.id, target, STACK_VIEW).unwrap();
            let updated = s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.id == divider.id).unwrap();
            let actual = center(&updated)[dimension] - start[dimension];
            if (actual - delta).abs() >= 0.01 {
                failures.push(format!("{axis:?} divider {} moved {actual}px for requested {delta}px with a measured header", divider.id));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

fn assert_ordinary_divider_structure(s: &UiSession<Recorder>) {
    fn splits(node: &DockNode, expected: &mut std::collections::BTreeMap<u32, (bool, Axis)>) {
        if let DockNode::Split { id, axis, first, second, .. } = node {
            assert!(expected.insert(*id, (false, *axis)).is_none());
            splits(first, expected); splits(second, expected);
        }
    }
    let layout = &s.state.workspace.layout;
    layout.validate().unwrap();
    let resolved = s.layout(STACK_VIEW);
    assert!(resolved.collapsed.is_empty());
    let mut expected = std::collections::BTreeMap::new();
    for band in &layout.bands {
        if band.alignment.is_none() {
            assert!(expected.insert(band.id, (true, band.edge.axis())).is_none());
            splits(&band.root, &mut expected);
        }
    }
    let mut actual = std::collections::BTreeMap::new();
    for divider in &resolved.dividers {
        assert!(actual.insert(divider.id, (divider.band, divider.axis)).is_none(), "duplicate resolved divider {}", divider.id);
        assert!(!divider.fixed && divider.bounds.width > 0. && divider.bounds.height > 0.);
    }
    assert_eq!(actual, expected, "every surviving band and split owns exactly one divider");
}

#[test]
fn divider_geometry_merge_retires_active_drag_and_recovers_history() {
    let mut s = session(Platform::Gtk);
    assert!(!s.workspace_interaction_active());
    assert_ordinary_divider_structure(&s);
    let before = crate::durable_layout(&s.state.workspace.layout);
    let divider = s.layout(STACK_VIEW).dividers.into_iter().find(|d| !d.band && d.axis == Axis::Vertical).unwrap();
    let center = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + divider.bounds.height * 0.5];
    s.dispatch(UiAction::DragDivider { id: divider.id, phase: ContactPhase::Down, position: center, viewport: STACK_VIEW }).unwrap();
    assert!(s.workspace_interaction_active());
    let moved = [center[0], center[1] + 12.];
    s.dispatch(UiAction::DragDivider { id: divider.id, phase: ContactPhase::Move, position: moved, viewport: STACK_VIEW }).unwrap();
    let resized = crate::durable_layout(&s.state.workspace.layout);
    assert_ne!(resized, before);
    assert!(s.localization_input_busy());
    s.dispatch(UiAction::MovePanel { panel: Panel::Brushes, target: DockTarget::Tab { group: 8, index: None }, viewport: STACK_VIEW }).unwrap();
    assert_ordinary_divider_structure(&s);
    assert!(!s.layout(STACK_VIEW).dividers.iter().any(|d| d.id == divider.id));
    let merged = crate::durable_layout(&s.state.workspace.layout);
    assert_ne!(merged, before);
    for phase in [ContactPhase::Move, ContactPhase::Up] {
        s.dispatch(UiAction::DragDivider { id: divider.id, phase, position: moved, viewport: STACK_VIEW }).unwrap();
    }
    assert!(!s.localization_input_busy(), "removed divider must retire its gesture after late Move/Up");
    assert!(!s.workspace_interaction_active());
    s.require_workspace_idle().unwrap();
    assert!(s.command(CommandId::UndoWorkspace).enabled, "the tab merge remains one recoverable history entry");
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), resized, "undo merge preserves the preceding resize");
    assert_ordinary_divider_structure(&s);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    assert_ordinary_divider_structure(&s);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), resized);
    assert_ordinary_divider_structure(&s);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), merged);
    assert_ordinary_divider_structure(&s);
    let next = s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.band && d.axis == Axis::Horizontal).unwrap();
    let start = [next.bounds.x + next.bounds.width * 0.5, next.bounds.y + next.bounds.height * 0.5];
    for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up] {
        let position = if phase == ContactPhase::Down { start } else { [start[0] + 12., start[1]] };
        s.dispatch(UiAction::DragDivider { id: next.id, phase, position, viewport: STACK_VIEW }).unwrap();
    }
    s.require_workspace_idle().unwrap();
    assert_ne!(crate::durable_layout(&s.state.workspace.layout), merged);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), merged);
    assert_ordinary_divider_structure(&s);
}

#[test]
fn workspace_view_stays_buffered_until_a_new_workspace_gesture() {
    let mut s=measured_divider_session();
    s.set_viewport(STACK_VIEW,[1800,1100]).unwrap();
    let divider=s.layout(STACK_VIEW).dividers.into_iter().find(|d|!d.band && d.axis==Axis::Vertical).unwrap();
    let center=[divider.bounds.x+divider.bounds.width/2.,divider.bounds.y+divider.bounds.height/2.];
    let drag=|phase,position|UiAction::DragDivider {id:divider.id,phase,position,viewport:STACK_VIEW};
    assert!(!s.workspace_view_unchanged());
    s.dispatch(drag(ContactPhase::Down,center)).unwrap();
    assert!(s.workspace_view_unchanged());
    let moved=[center[0],center[1]+12.];
    s.dispatch(drag(ContactPhase::Move,moved)).unwrap();
    assert!(s.workspace_view_unchanged());
    let before=s.state.camera.view();
    s.gesture([500.,400.],[512.,400.],1.,0.).unwrap();
    assert_ne!(s.state.camera.view(),before);
    for _ in 0..3 {
        assert!(s.workspace_interaction_active());
        assert!(!s.workspace_view_unchanged());
        s.dispatch(drag(ContactPhase::Move,moved)).unwrap();
    }
    s.gesture([512.,400.],[500.,400.],1.,0.).unwrap();
    assert_eq!(s.state.camera.view(),before);
    assert!(!s.workspace_view_unchanged(),"returning the camera cannot re-admit the same gesture");
    s.dispatch(drag(ContactPhase::Up,moved)).unwrap();
    assert!(!s.workspace_interaction_active());
    assert!(!s.workspace_view_unchanged());
    s.dispatch(drag(ContactPhase::Down,moved)).unwrap();
    assert!(s.workspace_view_unchanged(),"a new gesture may retain its unchanged canvas view");
    s.set_viewport([1801.,1100.],[1801,1100]).unwrap();
    assert!(!s.workspace_view_unchanged(),"viewport changes also revoke retained presentation");
    s.dispatch(drag(ContactPhase::Move,moved)).unwrap();
    assert!(!s.workspace_view_unchanged());
    s.set_viewport(STACK_VIEW,[1800,1100]).unwrap();
    assert!(!s.workspace_view_unchanged(),"returning the viewport cannot re-admit the same gesture");
    s.dispatch(drag(ContactPhase::Cancel,moved)).unwrap();
    assert!(!s.workspace_view_unchanged());
    s.dispatch(drag(ContactPhase::Down,moved)).unwrap();
    assert!(s.workspace_view_unchanged());
    s.dispatch(drag(ContactPhase::Up,moved)).unwrap();
}

#[test]
fn workspace_view_readmits_after_failed_resize_and_cancel() {
    for floating in [false, true] {
        let mut s = measured_divider_session();
        s.set_viewport(STACK_VIEW, [1800, 1100]).unwrap();
        let (id, center) = if floating {
            s.dispatch(UiAction::MovePanel { panel: Panel::Brushes,
                target: DockTarget::Float { position: [700., 220.] }, viewport: STACK_VIEW }).unwrap();
            let group = s.layout(STACK_VIEW).groups.into_iter().find(|g| g.floating).unwrap();
            (group.id, [group.bounds.x + group.bounds.width, group.bounds.y + group.bounds.height * 0.5])
        } else {
            let divider = s.layout(STACK_VIEW).dividers.into_iter().find(|d| !d.band && d.axis == Axis::Vertical).unwrap();
            (divider.id, [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + divider.bounds.height * 0.5])
        };
        let resize = |phase, position| if floating {
            UiAction::ResizeFloating { group: id, edge: ResizeEdge::Right, phase, position, viewport: STACK_VIEW }
        } else {
            UiAction::DragDivider { id, phase, position, viewport: STACK_VIEW }
        };
        s.dispatch(resize(ContactPhase::Down, [center[0], -f32::MAX])).unwrap();
        assert!(s.workspace_view_unchanged());
        s.gesture([500., 400.], [512., 400.], 1., 0.).unwrap();
        assert!(!s.workspace_view_unchanged());
        let error = s.dispatch(resize(ContactPhase::Move, [center[0], f32::MAX])).unwrap_err();
        assert_eq!(error, if floating { "Invalid floating panel size" } else { "Invalid dock size" });
        assert!(!s.workspace_interaction_active());
        s.dispatch(resize(ContactPhase::Down, center)).unwrap();
        assert!(s.workspace_view_unchanged(), "a failed resize must retire its navigation latch");
        s.gesture([500., 400.], [512., 400.], 1., 0.).unwrap();
        s.dispatch(resize(ContactPhase::Cancel, center)).unwrap();
        assert!(!s.workspace_view_unchanged());
        s.dispatch(resize(ContactPhase::Down, center)).unwrap();
        assert!(s.workspace_view_unchanged(), "a canceled resize must admit the next gesture");
        s.dispatch(resize(ContactPhase::Cancel, center)).unwrap();
    }
}

#[test]
fn divider_geometry_floating_tab_merge_retires_resize_and_recovers_history() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::MovePanel { panel: Panel::Brushes, target: DockTarget::Float { position: [700., 220.] }, viewport: STACK_VIEW }).unwrap();
    let workspace = s.state.workspace.clone();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
    let before = crate::durable_layout(&s.state.workspace.layout);
    let source = s.layout(STACK_VIEW).groups.into_iter().find(|g| g.floating && g.panels.contains(&Panel::Brushes)).unwrap();
    let start = [source.bounds.x + source.bounds.width, source.bounds.y + source.bounds.height * 0.5];
    let moved = [start[0] + 12., start[1]];
    for phase in [ContactPhase::Down, ContactPhase::Move] {
        s.dispatch(UiAction::ResizeFloating { group: source.id, edge: ResizeEdge::Right, phase,
            position: if phase == ContactPhase::Down { start } else { moved }, viewport: STACK_VIEW }).unwrap();
    }
    let resized = crate::durable_layout(&s.state.workspace.layout);
    assert_ne!(resized, before);
    assert!(s.localization_input_busy());
    s.dispatch(UiAction::MovePanel { panel: Panel::Brushes, target: DockTarget::Tab { group: 8, index: None }, viewport: STACK_VIEW }).unwrap();
    assert!(s.state.workspace.layout.floating.is_empty());
    let merged = crate::durable_layout(&s.state.workspace.layout);
    assert_ordinary_divider_structure(&s);
    for phase in [ContactPhase::Move, ContactPhase::Up] {
        s.dispatch(UiAction::ResizeFloating { group: source.id, edge: ResizeEdge::Right, phase,
            position: moved, viewport: STACK_VIEW }).unwrap();
    }
    assert!(!s.localization_input_busy());
    s.require_workspace_idle().unwrap();
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), resized, "undo merge preserves the preceding floating resize");
    assert_ordinary_divider_structure(&s);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    assert_ordinary_divider_structure(&s);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), resized);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), merged);
    assert_ordinary_divider_structure(&s);
}

fn three_member_target() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    let layout = &mut s.state.workspace.layout;
    layout.set_column_collapsed(5, true, STACK_VIEW).unwrap();
    let mut target = 4;
    for panel in [Panel::Navigator, Panel::Color] {
        layout.set_panel_visible(panel, true).unwrap();
        let group = layout.panel_group(panel).unwrap();
        layout
            .set_column_collapsed(group, true, STACK_VIEW)
            .unwrap();
        layout
            .move_item(
                STACK_VIEW,
                DockItem::Column { column: group },
                DockTarget::StackColumn {
                    column: target,
                    before: false,
                },
            )
            .unwrap();
        target = group;
    }
    s
}

#[test]
fn member_trailing_edges_append_groups_without_taking_grips() {
    for index in 0..3 {
        for item in [
            DockItem::Panel {
                panel: Panel::Properties,
            },
            DockItem::Group { group: 8 },
            DockItem::Group { group: 2 },
        ] {
            for offset in [-5., 0., 5.] {
                let mut s = three_member_target();
                let r = s.layout(STACK_VIEW);
                let members = s.state.workspace.layout.column_stack(4).members;
                let c = r.collapsed.iter().find(|c| c.id == members[index]).unwrap();
                let last = c.groups.last().unwrap();
                let bottom = last.bounds.y + last.bounds.height;
                let x = c.bounds.x + TILE_SIZE * 0.5;
                if index < 2 {
                    assert_eq!(
                        c.empty.height, 0.,
                        "compact members have no empty-area target"
                    );
                    assert_eq!(bottom + WORKSPACE_SPACING, c.grip.y);
                }
                let hint = s
                    .drop_hint(STACK_VIEW, [x, bottom + offset], &[], item, None)
                    .unwrap();
                assert_eq!(
                    hint.target,
                    DockTarget::Split {
                        group: last.group,
                        edge: Edge::Bottom
                    }
                );
                assert_eq!(hint.bounds.height, 3.);
                assert_eq!(hint.bounds.y + 1.5, bottom);
                assert!(hint.bounds.y + hint.bounds.height <= c.grip.y);
                assert!(matches!(
                    s.drop_hint(STACK_VIEW, [x, bottom - 8.], &[], item, None)
                        .unwrap()
                        .target,
                    DockTarget::Tab { .. }
                ));
                assert_eq!(
                    s.drop_hint(
                        STACK_VIEW,
                        [x, c.grip.y + c.grip.height * 0.5],
                        &[],
                        item,
                        None
                    )
                    .unwrap()
                    .target,
                    DockTarget::StackColumn {
                        column: c.id,
                        before: false
                    }
                );
                let before = crate::durable_layout(&s.state.workspace.layout);
                s.dispatch(item.move_action(hint.target, STACK_VIEW))
                    .unwrap();
                let layout = &s.state.workspace.layout;
                layout.validate().unwrap();
                let moved = match item {
                    DockItem::Panel { panel } => vec![panel],
                    DockItem::Group { group: 8 } => {
                        vec![Panel::Layers, Panel::Adjustments, Panel::Properties]
                    }
                    _ => vec![Panel::Toolbar],
                };
                let member = layout
                    .collapsed_column_for_group(layout.panel_group(moved[0]).unwrap())
                    .unwrap();
                let stack = layout.column_stack(member);
                assert_eq!(stack.members.len(), members.len());
                assert_eq!(stack.members[index], member);
                for (other, expected) in members.iter().enumerate() {
                    if other != index {
                        assert_eq!(&stack.members[other], expected);
                    }
                }
                let updated = s.layout(STACK_VIEW);
                let column = updated.collapsed.iter().find(|c| c.id == member).unwrap();
                assert_eq!(column.groups.len(), c.groups.len() + 1);
                let appended = column.groups.last().unwrap();
                assert_eq!(
                    appended.icons.iter().map(|i| i.panel).collect::<Vec<_>>(),
                    moved
                );
                assert_eq!(appended.divider.height, 1.);
                let after = crate::durable_layout(layout);
                invoke(&mut s, CommandId::UndoWorkspace);
                assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
                invoke(&mut s, CommandId::RedoWorkspace);
                assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
            }
        }
    }
}

#[test]
fn collapsed_column_scroll_reverses_immediately_after_overshoot() {
    let mut s = three_member_target();
    let viewport = [STACK_VIEW[0], 220.];
    s.set_viewport(viewport, viewport.map(|value| value as u32)).unwrap();
    s.dispatch(UiAction::MeasureColumnScroll { column: 4, offset: 10000. }).unwrap();
    let before = s.layout(viewport).collapsed.into_iter().find(|column| column.id == 4).unwrap();
    let offset = s.state.workspace.layout.column_scroll.iter().find(|(id, _)| *id == 4).unwrap().1;
    s.dispatch(UiAction::MeasureColumnScroll { column: 4, offset: offset - 10. }).unwrap();
    let after = s.layout(viewport).collapsed.into_iter().find(|column| column.id == 4).unwrap();
    assert_eq!(after.groups[0].bounds.y - before.groups[0].bounds.y, 10.);
}

#[test]
fn trailing_group_targets_follow_scrolling_and_stay_out_of_grips() {
    let mut s = three_member_target();
    let viewport = [STACK_VIEW[0], 220.];
    s.set_viewport(viewport, viewport.map(|value| value as u32)).unwrap();
    let initial = s.layout(viewport);
    let c = initial.collapsed.iter().find(|c| c.id == 4).unwrap();
    let x = c.bounds.x + TILE_SIZE * 0.5;
    assert!(
        c.groups.last().unwrap().bounds.y + c.groups.last().unwrap().bounds.height
            > c.content.y + c.content.height
    );
    assert!(
        c.append_group_drop_hint([x, c.grip.y - 3.]).is_none(),
        "a clipped last group cannot be appended before scrolling to its end"
    );
    s.dispatch(UiAction::MeasureColumnScroll {
        column: 4,
        offset: 10000.,
    })
    .unwrap();
    let scrolled = s.layout(viewport);
    let c = scrolled.collapsed.iter().find(|c| c.id == 4).unwrap();
    let point = [x, c.grip.y - 3.];
    let hint = c.append_group_drop_hint(point).unwrap();
    assert_eq!(
        hint.target,
        DockTarget::Split {
            group: 6,
            edge: Edge::Bottom
        }
    );
    assert!(hint.bounds.y >= c.content.y && hint.bounds.y + hint.bounds.height <= c.grip.y);
    assert!(c.append_group_drop_hint([x, c.grip.y + 1.]).is_none());
}

#[test]
fn panels_groups_and_toolbars_become_independent_stack_members() {
    for stacked in [false, true] {
        for floating in [false, true] {
            for kind in ["panel", "group", "toolbar", "custom-toolbar"] {
                let mut layout = DockLayout::default();
                layout.set_column_collapsed(5, true, STACK_VIEW).unwrap();
                if stacked {
                    layout.set_panel_visible(Panel::Navigator, true).unwrap();
                    let group = layout.panel_group(Panel::Navigator).unwrap();
                    layout
                        .set_column_collapsed(group, true, STACK_VIEW)
                        .unwrap();
                    layout
                        .move_item(
                            STACK_VIEW,
                            DockItem::Column { column: group },
                            DockTarget::StackColumn {
                                column: 4,
                                before: false,
                            },
                        )
                        .unwrap();
                }
                let preference = layout.column_stack_mut(4);
                preference.drawers = false;
                preference.auto_hide = true;
                preference.open_column = Some(4);
                let members = preference.members.clone();
                let item = match kind {
                    "panel" => DockItem::Panel {
                        panel: Panel::Adjustments,
                    },
                    "group" => {
                        layout.select_tab(8, Panel::Properties).unwrap();
                        layout.set_tab_style(8, crate::TabStyle::IconName).unwrap();
                        DockItem::Group { group: 8 }
                    }
                    "toolbar" => DockItem::Panel {
                        panel: Panel::Toolbar,
                    },
                    _ => DockItem::Panel {
                        panel: layout
                            .add_toolbar(
                                None,
                                "Stack tools",
                                &[ToolbarControl::Command {
                                    command: CommandId::Brush,
                                }],
                            )
                            .unwrap(),
                    },
                };
                if floating {
                    layout
                        .move_item(
                            STACK_VIEW,
                            item,
                            DockTarget::Float {
                                position: [700., 300.],
                            },
                        )
                        .unwrap();
                }
                let source_group = match item {
                    DockItem::Panel { panel } => layout.panel_group(panel).unwrap(),
                    DockItem::Group { group } => group,
                    _ => unreachable!(),
                };
                let source_tree = layout.node(source_group).unwrap().clone();
                let registry = layout.panels.clone();
                let target = *members.last().unwrap();
                layout
                    .move_item(
                        STACK_VIEW,
                        item,
                        DockTarget::StackColumn {
                            column: target,
                            before: false,
                        },
                    )
                    .unwrap();
                layout.validate().unwrap();
                let stack = layout.column_stack(4);
                assert_eq!(&stack.members[..members.len()], &members);
                assert_eq!(stack.members.len(), members.len() + 1);
                assert!(!stack.drawers && stack.auto_hide);
                assert_eq!(stack.open_column, Some(4));
                assert_eq!(
                    layout.panels, registry,
                    "tool IDs and configuration survive"
                );
                let member = *stack.members.last().unwrap();
                assert!(layout.is_collapsed(member));
                assert!(layout.expanded_column_width(member, STACK_VIEW[0]) >= 128.);
                let moving = layout.node(member).unwrap();
                if kind == "panel" {
                    assert_eq!(layout.group_panels(member).unwrap(), [Panel::Adjustments]);
                    assert_eq!(
                        layout.group_panels(8).unwrap(),
                        [Panel::Layers, Panel::Properties]
                    );
                } else {
                    assert_eq!(
                        moving, &source_tree,
                        "whole groups retain identity and selection"
                    );
                }
                let durable = crate::durable_layout(&layout);
                let loaded: DockLayout =
                    serde_json::from_value(serde_json::to_value(&layout).unwrap()).unwrap();
                loaded.validate().unwrap();
                assert_eq!(crate::durable_layout(&loaded), durable);
            }
        }
    }
}

#[test]
fn extracting_a_group_from_its_own_stack_follows_the_surviving_member() {
    for whole in [false, true] {
        let (mut s, left, right) = stack_fixture();
        stack_move(&mut s, right, left, false);
        let layout = &mut s.state.workspace.layout;
        let source = if whole {
            DockItem::Group { group: 5 }
        } else {
            DockItem::Panel {
                panel: Panel::Adjustments,
            }
        };
        let target = if whole { left } else { right };
        layout
            .move_item(
                STACK_VIEW,
                source,
                DockTarget::StackColumn {
                    column: target,
                    before: false,
                },
            )
            .unwrap();
        layout.validate().unwrap();
        let group = layout
            .panel_group(if whole {
                Panel::Brushes
            } else {
                Panel::Adjustments
            })
            .unwrap();
        let stack = layout.column_stack(group);
        assert_eq!(stack.members.len(), 3);
        if whole {
            assert_eq!(stack.members, [6, 5, right]);
        } else {
            assert_eq!(stack.members, [left, right, group]);
        }
        let before = layout.clone();
        layout
            .move_item(
                STACK_VIEW,
                DockItem::Group { group },
                DockTarget::StackColumn {
                    column: group,
                    before: false,
                },
            )
            .unwrap();
        assert_eq!(
            layout, &before,
            "dropping a sole group on its own footer is a no-op"
        );
    }
}

#[test]
fn gtk_stack_member_targets_preserve_tabs_dividers_and_other_hosts() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    let item = DockItem::Panel {
        panel: Panel::Toolbar,
    };
    let resolved = s.layout(STACK_VIEW);
    let above = resolved.collapsed.iter().find(|c| c.id == left).unwrap();
    let below = resolved.collapsed.iter().find(|c| c.id == right).unwrap();
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    let gap = [
        above.bounds.x + TILE_SIZE * 0.5,
        below.bounds.y - WORKSPACE_SPACING * 0.5,
    ];
    for (point, column) in [
        (center(above.grip), left),
        (center(below.grip), right),
        (center(below.empty), right),
    ] {
        let hint = s.drop_hint(STACK_VIEW, point, &[], item, None).unwrap();
        assert_eq!(
            hint.target,
            DockTarget::StackColumn {
                column,
                before: false
            }
        );
    }
    assert_eq!(s.drop_hint(STACK_VIEW, gap, &[], item, None).unwrap().target,
        DockTarget::Split { group: below.groups[0].group, edge: Edge::Top });
    let icon = center(above.groups[0].icons[0].bounds);
    assert!(matches!(
        s.drop_hint(STACK_VIEW, icon, &[], item, None)
            .unwrap()
            .target,
        DockTarget::Tab { .. }
    ));
    let divider = center(above.groups[1].divider);
    assert!(matches!(
        s.drop_hint(STACK_VIEW, divider, &[], item, None)
            .unwrap()
            .target,
        DockTarget::Split {
            edge: Edge::Top,
            ..
        }
    ));
    let tile = s
        .state
        .workspace
        .layout
        .panel(Panel::Toolbar)
        .unwrap()
        .tiles()[0]
        .id;
    assert!(
        s.drop_hint(
            STACK_VIEW,
            center(below.grip),
            &[],
            DockItem::Tile {
                panel: Panel::Toolbar,
                tile
            },
            None
        )
        .is_none()
    );
    let before = s.state.workspace.layout.clone();
    assert!(
        s.state
            .workspace
            .layout
            .move_item(
                STACK_VIEW,
                item,
                DockTarget::StackColumn {
                    column: u32::MAX,
                    before: false
                }
            )
            .is_err()
    );
    assert_eq!(s.state.workspace.layout, before);
}

#[test]
fn first_groups_accept_drops_above_each_member_without_padding() {
    for platform in Platform::ALL {
        for edge in [Edge::Left, Edge::Right] {
            for index in 0..3 {
                for item in [DockItem::Panel { panel: Panel::Properties }, DockItem::Group { group: 8 },
                    DockItem::Group { group: 2 }] {
                    let mut s = three_member_target();
                    s.set_platform(platform);
                    s.state.workspace.layout.bands[0].edge = edge;
                    s.state.workspace.layout.bands[1].edge = if edge == Edge::Left { Edge::Right } else { Edge::Left };
                    let members = s.state.workspace.layout.column_stack(4).members;
                    let resolved = s.layout(STACK_VIEW);
                    let column = resolved.collapsed.iter().find(|c| c.id == members[index]).unwrap();
                    let first = column.groups.first().unwrap();
                    let x = column.bounds.x + TILE_SIZE * 0.5;
                    assert_eq!(first.bounds.y, column.bounds.y);
                    let hint = s.drop_hint(STACK_VIEW, [x, column.bounds.y - 3.], &[], item, None).unwrap();
                    assert_eq!(hint.target, DockTarget::Split { group: first.group, edge: Edge::Top });
                    assert_eq!(hint.bounds.y, column.bounds.y);
                    assert!(matches!(s.drop_hint(STACK_VIEW, [x, first.bounds.y + 1.], &[], item, None).unwrap().target,
                        DockTarget::Tab { group, .. } if group == first.group));
                    assert!(resolved.drop_hint(x, column.bounds.y - 3., &[], false).is_none());
                    let before = crate::durable_layout(&s.state.workspace.layout);
                    s.dispatch(item.move_action(hint.target, STACK_VIEW)).unwrap();
                    let stack = s.state.workspace.layout.column_stack(4);
                    assert_eq!(stack.members.len(), members.len());
                    for other in 0..members.len() {
                        if other != index { assert_eq!(stack.members[other], members[other]); }
                    }
                    let after_layout = s.layout(STACK_VIEW);
                    let updated = after_layout.collapsed.iter().find(|c| c.id == stack.members[index]).unwrap();
                    assert_eq!(updated.groups.len(), column.groups.len() + 1);
                    assert_eq!(updated.groups[1].group, first.group);
                    let after = crate::durable_layout(&s.state.workspace.layout);
                    invoke(&mut s, CommandId::UndoWorkspace);
                    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
                    invoke(&mut s, CommandId::RedoWorkspace);
                    assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
                }
            }
        }
    }
}

#[test]
fn panel_bridges_preserve_the_receivers_original_corner_radii() {
    for edge in [Edge::Left, Edge::Right] {
        for clearance in [0., 0.5, 4., 10., 18., 24.] {
            let mut resolved = column_bridge_fixture(edge);
            let target = resolved.groups.iter_mut().find(|group| group.id == 5).unwrap();
            target.bounds.y = 100. - clearance;
            target.bounds.height = 160.;
            let resolved = resolved.with_panel_bridges();
            let target = resolved.groups.iter().find(|group| group.id == 5).unwrap();
            assert_eq!(target.corner_radii, [SURFACE_RADIUS; 4]);
            let column = &resolved.collapsed[0];
            let corner = if edge == Edge::Left { 1 } else { 0 };
            assert_eq!(column.corner_radii[corner], 0.);
            let bridges = resolved.panel_bridges();
            assert_eq!(bridges.len(), 1);
            assert_eq!(bridges[0].2.anchor.y, 100.);
            assert_eq!(bridges[0].2.receivers[0].radii, [SURFACE_RADIUS;4]);
        }
    }
}

fn column_bridge_fixture(edge: Edge) -> ResolvedLayout {
    let mut s = three_member_target();
    s.state.workspace.layout.bands[0].edge = edge;
    s.state.workspace.layout.bands[1].edge = if edge == Edge::Left { Edge::Right } else { Edge::Left };
    click_column(&mut s, Panel::Brushes);
    let mut r = s.layout(STACK_VIEW);
    r.collapsed.retain(|column| column.id == 4);
    r.groups.retain(|group| matches!(group.id, 5 | 6));
    let c = &mut r.collapsed[0];
    c.bounds.y = 100.;
    c.bounds.height = 240.;
    c.content.y = 100.;
    c.content.height = 210.;
    c.groups[0].icons[0].bounds.y = 100.;
    c.groups[0].icons[0].bounds.height = TILE_SIZE;
    c.groups[1].icons.clear();
    for (id, y) in [(5, 100.), (6, 300.)] {
        let target = r.groups.iter_mut().find(|group| group.id == id).unwrap();
        target.bounds.y = y;
        target.bounds.height = 160.;
    }
    r
}

#[test]
fn panel_bridge_receivers_keep_identity_and_only_own_the_facing_groups() {
    for edge in [Edge::Left,Edge::Right] {
        let mut resolved=column_bridge_fixture(edge).with_panel_bridges();
        let radii=[18.,12.,6.,0.];
        resolved.groups.iter_mut().find(|g|g.id==5).unwrap().corner_radii=radii;
        resolved.groups.iter_mut().find(|g|g.id==6).unwrap().bounds.x+=100.;
        let bridges=resolved.panel_bridges();
        assert_eq!(bridges.len(),1);
        assert_eq!(bridges[0].2.receivers.iter().map(|r|r.group).collect::<Vec<_>>(),vec![5]);
        assert_eq!(bridges[0].2.receivers[0].radii,radii);
        resolved.groups.reverse();
        assert_eq!(resolved.panel_bridges(),bridges);
        resolved.collapsed[0].open=None;
        assert!(resolved.panel_bridges().is_empty());
        let closed=resolved.with_panel_bridges();
        assert!(closed.collapsed[0].groups.iter().flat_map(|g|&g.icons).all(|icon|
            icon.corner_radii==[icon.bounds.width.min(icon.bounds.height)*0.5;4]));
    }
}

struct PanelVectorPayload {
    group: u32,
    generation: u64,
    commands: Vec<[i32;7]>,
    triangles: Vec<[i32;7]>,
}

fn panel_vector_payload(mut words: &[i32]) -> Vec<PanelVectorPayload> {
    let mut result=Vec::new();let mut groups=std::collections::BTreeSet::new();
    while !words.is_empty() {
        assert!(words.len()>=5,"truncated vector header");
        let group=words[0] as u32;assert!(groups.insert(group),"duplicate vector owner");
        let generation=words[1] as u32 as u64|((words[2] as u32 as u64)<<32);
        let commands=usize::try_from(words[3]).unwrap();let triangles=usize::try_from(words[4]).unwrap();
        let end=5+(commands+triangles)*7;assert!(words.len()>=end,"truncated vector geometry");
        let decode=|slice: &[i32]|slice.chunks_exact(7).map(|chunk|chunk.try_into().unwrap()).collect();
        result.push(PanelVectorPayload {group,generation,commands:decode(&words[5..5+commands*7]),triangles:decode(&words[5+commands*7..end])});
        words=&words[end..];
    }
    result
}

fn panel_vector_generations(cache: &crate::PanelVectorCache) -> std::collections::BTreeMap<u32,u64> {
    panel_vector_payload(&cache.packed()).into_iter().map(|entry|(entry.group,entry.generation)).collect()
}

fn panel_vector_cache_resolve(cache: &mut crate::PanelVectorCache, layout: ResolvedLayout, scale: f32) -> ResolvedLayout {
    let mut retained=layout.clone();let mut fresh=layout;
    cache.resolve(&mut retained,scale);
    crate::PanelVectorCache::default().resolve(&mut fresh,scale);
    assert_eq!(retained.vector_surfaces,fresh.vector_surfaces,"retained cache differs from current geometry");
    let payload=panel_vector_payload(&cache.packed());
    assert_eq!(payload.len(),retained.vector_surfaces.len());
    for surface in &retained.vector_surfaces {
        let column=retained.collapsed.iter().find(|column|column.id==surface.column).unwrap();
        let open=column.open.as_ref().expect("vector surface outlived its open column");
        let group=retained.groups.iter().find(|group|group.id==surface.group).expect("vector surface outlived its receiving group");
        let distance=if open.direction==Edge::Right {group.bounds.x-open.bounds.x}
            else {group.bounds.x+group.bounds.width-open.bounds.x-open.bounds.width};
        assert!(distance.abs()<0.5,"vector surface outlived its facing attachment");
        assert!(column.groups.iter().any(|member|member.group==surface.group));
        assert!(!surface.commands.is_empty());
        let entry=payload.iter().find(|entry|entry.group==surface.group).unwrap();
        assert_eq!(entry.commands,surface.commands);assert_eq!(entry.triangles,surface.triangles);
    }
    retained
}

#[test]
fn panel_vector_cache_relocates_identical_groups_between_both_sides() {
    let mut cache=crate::PanelVectorCache::default();let mut previous: Option<ResolvedLayout>=None;
    let mut previous_generations=std::collections::BTreeMap::new();
    let mut side_generations=std::collections::BTreeMap::new();
    for edge in [Edge::Left,Edge::Right,Edge::Left,Edge::Right] {
        let mut layout=column_bridge_fixture(edge).with_panel_bridges();
        let open=layout.collapsed[0].open.as_ref().unwrap();
        let right=open.direction==Edge::Right;
        let plane=if right {open.bounds.x} else {open.bounds.x+open.bounds.width};
        for group in &mut layout.groups {group.bounds.width=240.;group.bounds.x=if right {plane} else {plane-240.};}
        let current=panel_vector_cache_resolve(&mut cache,layout,0.);
        let generations=panel_vector_generations(&cache);
        if let Some(prior)=side_generations.insert(right,generations.clone()) {
            assert_eq!(generations,prior,"returning to the same side must reuse its geometry generation");
        }
        assert_eq!(panel_vector_cache_resolve(&mut cache,current.clone(),0.).vector_surfaces,current.vector_surfaces);
        assert_eq!(panel_vector_generations(&cache),generations,"identical geometry must retain its packed generation");
        assert!(!current.vector_surfaces.is_empty());
        if let Some(previous)=previous {
            for surface in &current.vector_surfaces {
                let old=previous.vector_surfaces.iter().find(|old|old.group==surface.group).unwrap();
                assert_ne!(generations[&surface.group],previous_generations[&surface.group],"mirror relocation selects another geometry generation");
                assert_eq!(surface.commands.len(),old.commands.len());
                for (a,b) in surface.commands.iter().zip(&old.commands) {
                    assert_eq!(a[0],b[0]);
                    let points=match a[0] {0|1=>1,2=>3,_=>0};
                    for i in (1..=points*2).step_by(2) {
                        assert_eq!(a[i]+b[i],240*1024,"same group must mirror around its current receiving pane");
                        assert_eq!(a[i+1],b[i+1]);
                    }
                }
            }
        }
        previous=Some(current);
        previous_generations=generations;
    }
}

#[test]
fn panel_vector_cache_refreshes_source_radii_and_bounds_without_changing_paths() {
    for edge in [Edge::Left,Edge::Right] {
        let mut cache=crate::PanelVectorCache::default();
        let mut layout=column_bridge_fixture(edge).with_panel_bridges();
        layout.collapsed[0].content.height=TILE_SIZE*0.5;
        let squared=panel_vector_cache_resolve(&mut cache,layout.clone(),0.);
        let squared_generations=panel_vector_generations(&cache);
        let mut rounded=layout;
        rounded.collapsed[0].corner_radii=[SURFACE_RADIUS;4];
        let rounded=panel_vector_cache_resolve(&mut cache,rounded,0.);
        let rounded_generations=panel_vector_generations(&cache);
        for (group,generation) in &rounded_generations {assert_ne!(*generation,squared_generations[group],"source radius change selects another geometry generation");}
        assert_eq!(squared.vector_surfaces.len(),rounded.vector_surfaces.len());
        assert!(!squared.vector_surfaces.is_empty());
        assert!(squared.vector_surfaces.iter().zip(&rounded.vector_surfaces).any(|(a,b)|a.triangles!=b.triangles),"source corner coverage must refresh exterior edge flags");
        for (a,b) in squared.vector_surfaces.iter().zip(&rounded.vector_surfaces) {assert_eq!(a.commands,b.commands);}
        let mut taller=rounded.clone();
        taller.collapsed[0].bounds.y-=24.;taller.collapsed[0].bounds.height+=24.;
        let taller=panel_vector_cache_resolve(&mut cache,taller,0.);
        for (group,generation) in panel_vector_generations(&cache) {assert_ne!(generation,rounded_generations[&group],"source bounds change selects another geometry generation");}
        assert!(rounded.vector_surfaces.iter().zip(&taller.vector_surfaces).any(|(a,b)|a.triangles!=b.triangles),"moving source background past its corner must refresh coverage");
        for (a,b) in rounded.vector_surfaces.iter().zip(&taller.vector_surfaces) {assert_eq!(a.commands,b.commands);}
        assert_eq!(panel_vector_cache_resolve(&mut cache,squared.clone(),0.).vector_surfaces,squared.vector_surfaces);
    }
}

#[test]
fn panel_vector_cache_projects_fractional_physical_grids_from_current_layout() {
    for edge in [Edge::Left,Edge::Right] {
        let mut layout=column_bridge_fixture(edge).with_panel_bridges();
        let right=layout.collapsed[0].open.as_ref().unwrap().direction==Edge::Right;
        let shift=|bounds: &mut Bounds| {bounds.x+=0.23;bounds.y+=0.37;};
        for group in &mut layout.groups {
            shift(&mut group.bounds);group.bounds.width+=0.27;group.bounds.height+=0.31;
            if !right {group.bounds.x-=0.27;}
        }
        for column in &mut layout.collapsed {
            shift(&mut column.bounds);shift(&mut column.content);
            shift(&mut column.open.as_mut().unwrap().bounds);
            for icon in column.groups.iter_mut().flat_map(|group|&mut group.icons) {shift(&mut icon.bounds);}
        }
        let mut cache=crate::PanelVectorCache::default();let mut previous=None;
        for scale in [1.5,2.,1.5] {
            let current=panel_vector_cache_resolve(&mut cache,layout.clone(),scale);
            assert!(!current.vector_surfaces.is_empty());
            assert_ne!(current.groups[0].bounds.y,layout.groups[0].bounds.y);
            let on_grid=|bounds: Bounds| {for value in [bounds.x,bounds.y,bounds.x+bounds.width,bounds.y+bounds.height] {
                assert!((value*scale-(value*scale).round()).abs()<0.001,"edge is off physical grid at scale {scale}: {bounds:?}");
            }};
            for group in &current.groups {on_grid(group.bounds);}
            for column in &current.collapsed {
                on_grid(column.bounds);on_grid(column.content);on_grid(column.open.as_ref().unwrap().bounds);
                for icon in column.groups.iter().flat_map(|group|&group.icons) {on_grid(icon.bounds);}
            }
            if let Some(previous)=previous {assert_ne!(current.vector_surfaces,previous,"different snapped pane sizes must change the projection");}
            previous=Some(current.vector_surfaces);
        }
    }
}

#[test]
fn panel_vector_cache_retires_closed_removed_and_unattached_receivers() {
    let mut s=three_member_target();let mut cache=crate::PanelVectorCache::default();
    click_column(&mut s,Panel::Brushes);
    let opened=panel_vector_cache_resolve(&mut cache,s.layout(STACK_VIEW).with_panel_bridges(),0.);
    assert!(!opened.vector_surfaces.is_empty());
    click_column(&mut s,Panel::Brushes);
    let mut closed=s.layout(STACK_VIEW).with_panel_bridges();
    closed.vector_surfaces=opened.vector_surfaces.clone();
    assert!(panel_vector_cache_resolve(&mut cache,closed,0.).vector_surfaces.is_empty());
    assert!(cache.packed().is_empty(),"closed columns must retire the bulk geometry too");
    click_column(&mut s,Panel::Brushes);
    let reopened=panel_vector_cache_resolve(&mut cache,s.layout(STACK_VIEW).with_panel_bridges(),0.);
    assert_eq!(reopened.vector_surfaces,opened.vector_surfaces);
    let ids: Vec<_>=reopened.vector_surfaces.iter().map(|surface|surface.group).collect();
    let mut removed=reopened.clone();removed.groups.retain(|group|!ids.contains(&group.id));
    assert!(panel_vector_cache_resolve(&mut cache,removed,0.).vector_surfaces.is_empty());
    assert_eq!(panel_vector_cache_resolve(&mut cache,reopened.clone(),0.).vector_surfaces,reopened.vector_surfaces);
    let mut unattached=reopened.clone();
    for group in &mut unattached.groups {if ids.contains(&group.id) {group.bounds.x+=100.;}}
    assert!(panel_vector_cache_resolve(&mut cache,unattached,0.).vector_surfaces.is_empty());
    assert_eq!(panel_vector_cache_resolve(&mut cache,reopened.clone(),0.).vector_surfaces,reopened.vector_surfaces);
}

#[test]
fn panel_vector_cache_revisits_geometry_without_publishing_history() {
    let mut cache=crate::PanelVectorCache::default();
    let original=column_bridge_fixture(Edge::Left).with_panel_bridges();
    let a=panel_vector_cache_resolve(&mut cache,original.clone(),0.);
    let a_generations=panel_vector_generations(&cache);
    assert!(!a_generations.is_empty());
    let mut changed=original.clone();
    let id=a.vector_surfaces[0].group;
    changed.groups.iter_mut().find(|group|group.id==id).unwrap().bounds.height-=12.;
    let b=panel_vector_cache_resolve(&mut cache,changed,0.);
    let b_generations=panel_vector_generations(&cache);
    assert_ne!(a.vector_surfaces,b.vector_surfaces,"the intermediate pose must have different geometry");
    assert_ne!(a_generations[&id],b_generations[&id]);
    assert_eq!(panel_vector_cache_resolve(&mut cache,original.clone(),0.).vector_surfaces,a.vector_surfaces);
    assert_eq!(panel_vector_generations(&cache),a_generations,"A to B to A must reuse A's identity without publishing B");
    let mut closed=original.clone();closed.collapsed[0].open=None;
    assert!(panel_vector_cache_resolve(&mut cache,closed,0.).vector_surfaces.is_empty());
    assert!(cache.packed().is_empty(),"prepared history must not appear in a closed layout's payload");
    assert_eq!(panel_vector_cache_resolve(&mut cache,original,0.).vector_surfaces,a.vector_surfaces);
    assert_eq!(panel_vector_generations(&cache),a_generations,"reopening an identical pose may reuse its prepared identity");
}

#[test]
fn layout_resolve_input_roundtrips_runtime_geometry_and_zen_across_viewports() {
    for preset in 0..3 {
        let mut s=if preset==0 {three_member_target()} else {session(Platform::Windows)};
        if preset==0 {click_column(&mut s,Panel::Brushes);}
        if preset==1 {s.state.workspace.layout=WorkspacePreset::Painter.layout(Platform::Windows);}
        if preset==2 {s.state.workspace.layout=WorkspacePreset::Illustrator.layout(Platform::Windows);}
        s.state.workspace.layout.set_panel_visible(Panel::Layers,true).unwrap();
        let group=s.state.workspace.layout.panel_group(Panel::Layers).unwrap();
        s.state.workspace.layout.add_panel_to_group(Panel::Toolbar,group).unwrap();
        let ids=s.layout(STACK_VIEW).groups.iter().map(|g|g.id).collect::<Vec<_>>();
        let layout=&mut s.state.workspace.layout;
        layout.fit_tab_groups=ids.clone();layout.fit_height_groups=ids;
        layout.header_presentation=HeaderPresentation {height:HEADER_HEIGHT+18.,items:vec![HeaderItemBounds {
            id:42,bounds:Bounds{x:30.,y:4.,width:250.,height:32.},
        }]};
        layout.column_scroll=vec![(layout.collapsed.first().map_or(group,|c|c.root),17.)];
        layout.measurements=vec![PanelMeasurement {panel:Panel::Layers,tab_widths:[173.,36.],content_height:463.,
            scroll:Some(PanelScrollMeasurement {fixed_height:13.,unit_height:25.})}];
        layout.titlebar_insets=[41.,117.,32.];layout.bottom_inset=23.;
        for peek in [false,true] {
            if peek {
                s.set_viewport(STACK_VIEW,[1800,1100]).unwrap();invoke(&mut s,CommandId::ZenMode);
                chrome(&mut s,ChromeEvent::Refresh,ChromeFacts::default());
                let edge=s.layout(STACK_VIEW).groups.iter().find_map(|g|s.state.workspace.layout.group_edge(g.id)).unwrap();
                let (start,end)=match edge {
                    Edge::Left=>([4.,500.],[64.,500.]),Edge::Right=>([1796.,500.],[1736.,500.]),
                    Edge::Top=>([800.,4.],[800.,64.]),Edge::Bottom=>([800.,1096.],[800.,1036.]),
                };
                s.edge_swipe(99,ContactPhase::Down,start);assert!(s.edge_swipe(99,ContactPhase::Move,end));
            }
            let wire=serde_json::to_value(s.layout_resolve_input()).unwrap();
            assert_eq!(wire["header_presentation"],serde_json::json!(s.state.workspace.layout.header_presentation));
            assert_eq!(wire["column_scroll"],serde_json::json!(s.state.workspace.layout.column_scroll));
            assert_eq!(wire["column_open"],serde_json::json!(s.state.workspace.layout.column_stacks.iter()
                .map(|stack|(stack.column,stack.open_column)).collect::<Vec<_>>()));
            assert_eq!(wire["measurements"],serde_json::json!(s.state.workspace.layout.measurements));
            assert_eq!(wire["titlebar_insets"],serde_json::json!([41.,117.,32.]));assert_eq!(wire["bottom_inset"],23.);
            assert_eq!(!wire["zen_peek_edge"].is_null(),peek);
            for skipped in ["header_presentation","column_scroll","measurements","titlebar_insets","bottom_inset"] {
                assert!(wire["layout"].get(skipped).is_none());
            }
            let input:crate::LayoutResolveInput<'static>=serde_json::from_value(wire).unwrap();
            for viewport in [[1800.,1100.],[1100.,700.],[640.,480.],[1500.,900.]] {
                let expected=s.layout(viewport);
                assert_eq!(serde_json::to_value(input.resolve(viewport)).unwrap(),serde_json::to_value(&expected).unwrap());
                assert!(expected.groups.iter().any(|g|g.panels.len()>1));
                for group in &expected.groups {assert_eq!(group.zen_kept,s.zen_keeps(group.id));}
            }
        }
    }
}

#[test]
fn measured_panel_bridges_use_only_current_allocations_and_preserve_receiver_identity() {
    for edge in [Edge::Left,Edge::Right] {
        let resolved=column_bridge_fixture(edge);
        let measure=crate::PanelBridgeMeasure {
            groups:resolved.groups.iter().map(|g|(g.id,g.bounds)).collect(),
            columns:resolved.collapsed.iter().map(|c|crate::PanelColumnMeasure {
                column:c.id,bounds:c.bounds,content:c.content,
                icons:c.groups.iter().flat_map(|g|&g.icons).map(|i|(i.panel,i.bounds)).collect(),
            }).collect(),
        };
        let expected=resolved.clone().with_panel_bridges().panel_bridges();
        let actual=resolved.clone().measured_panel_bridges(measure.clone()).unwrap();
        assert_eq!(actual.bridges,expected);
        assert!(!actual.bridges.is_empty());
        let mut vectors=resolved.clone().with_panel_bridges();
        crate::PanelVectorCache::default().resolve(&mut vectors,0.);
        assert_eq!(actual.paths.len(),vectors.vector_surfaces.len());
        assert!(actual.paths.len()>1);
        for path in &actual.paths {
            let vector=vectors.vector_surfaces.iter().find(|v|v.group==path.group && v.column==path.column).unwrap();
            assert_eq!(path.commands.len(),vector.commands.len());
            for (command,packed) in path.commands.iter().zip(&vector.commands) {
                assert_eq!(command[0],packed[0] as f32);
                for i in 1..7 {assert!((command[i]-packed[i] as f32/1024.).abs()<=1./1024.);}
            }
            assert!(path.commands.iter().flatten().all(|value|value.is_finite()));
        }
        let mut closed=resolved.clone();closed.collapsed[0].open=None;
        assert!(closed.measured_panel_bridges(measure.clone()).unwrap().paths.is_empty());
        let json=serde_json::to_value(&actual).unwrap();
        let decoded:Vec<(u32,Panel,crate::PanelBridge)>=serde_json::from_value(json["bridges"].clone()).unwrap();
        assert_eq!(decoded,expected);
        for (_,_,bridge) in &actual.bridges {for receiver in &bridge.receivers {
            let native=actual.groups.iter().find(|g|g.group==receiver.group).unwrap();
            assert_eq!(receiver,native);
            assert_eq!(receiver.radii.len(),4);
        }}
        let mut partial=measure.clone();partial.groups.clear();
        let missing=resolved.clone().measured_panel_bridges(partial).unwrap();
        assert!(missing.groups.is_empty());assert!(missing.bridges.is_empty());assert!(missing.paths.is_empty());
        let mut missing_icon=measure.clone();missing_icon.columns[0].icons.clear();
        let missing_icon=resolved.clone().measured_panel_bridges(missing_icon).unwrap();
        assert!(missing_icon.bridges.is_empty());
        assert!(missing_icon.paths.iter().all(|path|path.commands.iter().filter(|c|c[0]==0.).count()==1));
        let mut moved=measure.clone();moved.groups[0].1.y+=0.5;
        let moved_frame=resolved.clone().measured_panel_bridges(moved.clone()).unwrap();
        assert_eq!(moved_frame.groups[0].bounds,moved.groups[0].1);
        let mut retired=measure.clone();
        retired.groups.push((u32::MAX,Bounds::default()));
        retired.columns[0].icons.push((Panel::CustomToolbar(u32::MAX),Bounds::default()));
        retired.columns.push(crate::PanelColumnMeasure {
            column:u32::MAX,bounds:Bounds::default(),content:Bounds::default(),icons:vec![],
        });
        let with_retired=resolved.clone().measured_panel_bridges(retired).unwrap();
        assert_eq!(serde_json::to_value(with_retired).unwrap(),json);
        for mutation in 0..6 {
            let mut invalid=measure.clone();
            match mutation {
                0=>invalid.groups.push(invalid.groups[0]),
                1=>invalid.groups[0].1.x=f32::NAN,
                2=>invalid.columns[0].content.width= -1.,
                3=>invalid.columns.push(invalid.columns[0].clone()),
                4=>{let icon=invalid.columns[0].icons[0];invalid.columns[0].icons.push(icon);},
                _=>invalid.groups.push((u32::MAX,Bounds{x:f32::NAN,..Bounds::default()})),
            }
            assert!(resolved.clone().measured_panel_bridges(invalid).is_err(),"{edge:?} case {mutation}");
        }
    }
}

#[test]
fn flush_column_connections_keep_the_receiving_radius_and_join_the_source_corner() {
    for platform in Platform::ALL {
        for edge in [Edge::Left, Edge::Right] {
            let mut s = three_member_target();
            s.set_platform(platform);
            s.state.workspace.layout.bands[0].edge = edge;
            s.state.workspace.layout.bands[1].edge = if edge == Edge::Left { Edge::Right } else { Edge::Left };
            let closed = s.layout(STACK_VIEW).with_panel_bridges();
            assert!(closed.collapsed.iter().all(|c| c.corner_radii.iter().all(|r| *r > 0.)));
            let original_radii: Vec<_> = closed.groups.iter().map(|g| (g.id,g.corner_radii)).collect();
            click_column(&mut s, Panel::Brushes);
            let resolved = s.layout(STACK_VIEW).with_panel_bridges();
            let column = resolved.collapsed.iter().find(|c| c.id == 4).unwrap();
            let open = column.open.as_ref().unwrap();
            let first = &column.groups[0];
            let connection = &open.connections.iter().find(|(panel, _)| *panel == first.active).unwrap().1;
            assert_eq!(first.bounds.y, open.bounds.y);
            assert_eq!(connection.bounds.y, first.bounds.y);
            assert_eq!(connection.radii, [0., WORKSPACE_SPACING]);
            let source_corner = if edge == Edge::Left { 1 } else { 0 };
            assert_eq!(column.corner_radii.map(|r| r == 0.), std::array::from_fn(|i| i == source_corner));
            let target = resolved.groups.iter().find(|g| g.id == first.group).unwrap();
            let target_corner = if edge == Edge::Left { 0 } else { 1 };
            assert_eq!(target.corner_radii[target_corner], SURFACE_RADIUS);
            for (_, lower) in open.connections.iter().skip(1) {
                assert!(lower.radii.iter().all(|r| *r > 0.));
            }
            click_column(&mut s, Panel::Brushes);
            let closed = s.layout(STACK_VIEW).with_panel_bridges();
            assert!(closed.collapsed.iter().all(|c| c.corner_radii.iter().all(|r| *r > 0.)));
            assert_eq!(closed.groups.iter().map(|g| (g.id,g.corner_radii)).collect::<Vec<_>>(),original_radii);
        }
    }
}

#[test]
fn stack_member_drops_cancel_and_undo_in_one_step() {
    for item in [
        DockItem::Panel {
            panel: Panel::Adjustments,
        },
        DockItem::Group { group: 8 },
        DockItem::Panel {
            panel: Panel::Toolbar,
        },
    ] {
        let mut s = session(Platform::Gtk);
        set_collapsed(&mut s, 5, true);
        let before = crate::durable_layout(&s.state.workspace.layout);
        let resolved = s.layout(STACK_VIEW);
        let source_group = match item {
            DockItem::Group { group } => group,
            DockItem::Panel { panel } => s.state.workspace.layout.panel_group(panel).unwrap(),
            _ => unreachable!(),
        };
        let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
        let source = center(
            resolved
                .groups
                .iter()
                .find(|g| g.id == source_group)
                .unwrap()
                .bounds,
        );
        let target = center(resolved.collapsed[0].grip);
        for end in [ContactPhase::Cancel, ContactPhase::Up] {
            for (phase, position) in [
                (ContactPhase::Down, source),
                (ContactPhase::Move, target),
                (end, target),
            ] {
                drag(&mut s, item, phase, position, STACK_VIEW);
                if phase == ContactPhase::Move {
                    assert!(matches!(
                        s.workspace_update().drag.unwrap().drop_hint.unwrap().target,
                        DockTarget::StackColumn { .. }
                    ));
                }
            }
            if end == ContactPhase::Cancel {
                assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            }
        }
        let after = crate::durable_layout(&s.state.workspace.layout);
        assert_eq!(s.state.workspace.layout.column_stack(4).members.len(), 2);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
        invoke(&mut s, CommandId::RedoWorkspace);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
    }
}

#[test]
fn paint_defaults_open_right_stack_on_load_and_reset() {
    for platform in Platform::ALL {
        check_paint_default_stack(platform);
    }
}

fn check_paint_default_stack(platform: Platform) {
    let mut s = session(platform);
    let layout = crate::WorkspacePreset::Illustrator.layout(platform);
    let capture = crate::WorkspaceCapture {
        history: crate::LayoutHistory::new(&layout),
        working: crate::WorkspaceWorkingState::default(),
    };
    let assert_open = |s: &UiSession<Recorder>| {
        let resolved = s.layout(STACK_VIEW);
        assert_eq!(resolved.collapsed.len(), 1);
        assert_eq!(resolved.collapsed[0].id, 12);
        assert!(
            resolved
                .groups
                .iter()
                .any(|g| g.panels.contains(&Panel::Brushes))
        );
        for column in &resolved.collapsed {
            let stack = s.state.workspace.layout.column_stack(column.id);
            assert!(!stack.auto_hide && !stack.drawers);
            assert_eq!(stack.members, [column.id]);
            let open = column.open.as_ref().unwrap();
            assert_eq!(open.bounds.y, HEADER_HEIGHT);
            assert_eq!(
                open.bounds.height,
                STACK_VIEW[1] - HEADER_HEIGHT - WORKSPACE_SPACING
            );
            assert_eq!(open.connections.len(), column.groups.len());
            for group in &column.groups {
                assert!(resolved.groups.iter().any(|g| g.id == group.group));
            }
            assert_eq!(column.bounds.x - open.bounds.x - open.bounds.width, WORKSPACE_SPACING);
        }
        let commands = resolved
            .groups
            .iter()
            .find(|g| g.panels.contains(&Panel::Commands))
            .unwrap()
            .bounds;
        let end = commands.x + commands.width;
        let left = resolved.groups.iter().map(|g| g.bounds.x + g.bounds.width)
            .filter(|x| *x <= commands.x).fold(f32::MIN, f32::max);
        let right = resolved.groups.iter().map(|g| g.bounds.x)
            .filter(|x| *x >= end).fold(f32::MAX, f32::min);
        assert_eq!(commands.x - left, WORKSPACE_SPACING);
        assert_eq!(right - end, WORKSPACE_SPACING);
    };
    s.adopt_workspace(crate::PreparedWorkspace::new(capture.clone()).unwrap())
        .unwrap();
    assert_open(&s);
    assert_eq!(s.capture_workspace().unwrap().history, capture.history);
    click_column(&mut s, Panel::Layers);
    assert!(
        s.layout(STACK_VIEW)
            .collapsed
            .iter()
            .find(|c| c.id == 12)
            .unwrap()
            .open
            .is_none()
    );
    assert_eq!(s.capture_workspace().unwrap().history, capture.history);

    let saved =
        serde_json::from_value(serde_json::to_value(s.capture_workspace().unwrap()).unwrap())
            .unwrap();
    s.adopt_workspace(crate::PreparedWorkspace::new(saved).unwrap())
        .unwrap();
    assert_open(&s);
    click_column(&mut s, Panel::Layers);
    s.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(crate::WorkspaceState {
            layout: layout.clone(),
            ..s.state.workspace.clone()
        }),
    })
    .unwrap();
    assert_open(&s);

    let mut customized = layout;
    customized.header.size = crate::HeaderSize::Large;
    s.adopt_workspace(
        crate::PreparedWorkspace::new(crate::WorkspaceCapture {
            history: crate::LayoutHistory::new(&customized),
            ..capture
        })
        .unwrap(),
    )
    .unwrap();
    assert!(
        s.layout(STACK_VIEW)
            .collapsed
            .iter()
            .all(|c| c.open.is_none())
    );
}

#[test]
fn replacing_the_workspace_keeps_host_measurements_and_follows_the_header_size() {
    let mut s = session(Platform::Gtk);
    let preset = |preset: crate::WorkspacePreset| preset.layout(Platform::Gtk);
    let adopt = |s: &mut UiSession<Recorder>, layout: &DockLayout| {
        s.adopt_workspace(crate::PreparedWorkspace::new(crate::WorkspaceCapture {
            history: crate::LayoutHistory::new(layout),
            working: crate::WorkspaceWorkingState::default(),
        }).unwrap()).unwrap();
    };
    let paint = preset(crate::WorkspacePreset::Illustrator);
    adopt(&mut s, &paint);
    let measurements = [(Panel::Color, 300.), (Panel::Navigator, 180.)]
        .map(|(panel, content_height)| PanelMeasurement { panel, tab_widths:[0.;2], content_height, scroll: None })
        .to_vec();
    s.dispatch(UiAction::MeasurePanels { measurements:measured_batch(&s,STACK_VIEW,measurements) }).unwrap();
    s.dispatch(UiAction::MeasureHeader { height: paint.header.size.height(), items: Vec::new() }).unwrap();
    let fitted = |s: &UiSession<Recorder>| {
        let resolved = s.layout(STACK_VIEW);
        [Panel::Color, Panel::Navigator].map(|panel| {
            resolved.groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds.height
        })
    };
    let expected = [300. + TAB_BAR_HEIGHT, 180. + TAB_BAR_HEIGHT];
    assert_eq!(fitted(&s), expected);
    let sketch = preset(crate::WorkspacePreset::Painter);
    assert_ne!(sketch.header.size, paint.header.size);
    adopt(&mut s, &sketch);
    assert_eq!(s.state.workspace.layout.header_presentation.height, sketch.header.size.height());
    assert_eq!(s.layout(STACK_VIEW).work_area.y, sketch.header.size.height());
    adopt(&mut s, &preset(crate::WorkspacePreset::Photographer));
    adopt(&mut s, &paint);
    assert_eq!(s.state.workspace.layout.header_presentation.height, paint.header.size.height());
    assert_eq!(fitted(&s), expected);
    s.begin_workspace_transition().unwrap();
    s.begin_workspace_layout_preview().unwrap();
    s.preview_workspace_layout(&sketch).unwrap();
    s.preview_workspace_layout(&paint).unwrap();
    assert_eq!(fitted(&s), expected);
    s.cancel_workspace_layout_preview();
    s.end_workspace_transition();
    assert_eq!(fitted(&s), expected);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(crate::WorkspaceState { layout: paint, ..Default::default() }) }).unwrap();
    assert_eq!(fitted(&s), expected);
}

fn stack_fixture() -> (UiSession<Recorder>, u32, u32) {
    let mut s = session(Platform::Gtk);
    let mut columns = Vec::new();
    for panel in [Panel::Brushes, Panel::Layers] {
        let group = s.state.workspace.layout.panel_group(panel).unwrap();
        set_collapsed(&mut s, group, true);
        columns.push(
            s.state
                .workspace
                .layout
                .collapsed_column_for_group(group)
                .unwrap(),
        );
    }
    (s, columns[0], columns[1])
}
fn click_column(s: &mut UiSession<Recorder>, panel: Panel) {
    let group = s.state.workspace.layout.panel_group(panel).unwrap();
    customize(s, CustomizationAction::ToggleColumnDrawer { group, panel });
}
fn stack_move(s: &mut UiSession<Recorder>, source: u32, target: u32, before: bool) {
    s.dispatch(UiAction::MoveColumn {
        column: source,
        target: DockTarget::StackColumn {
            column: target,
            before,
        },
        viewport: STACK_VIEW,
    })
    .unwrap();
}

fn drag_divider(s: &mut UiSession<Recorder>, id: u32, phase: ContactPhase, position: [f32; 2]) {
    s.dispatch(UiAction::DragDivider { id, phase, position, viewport: STACK_VIEW }).unwrap();
    s.state.workspace.validate().unwrap();
}

fn stack_divider(s: &UiSession<Recorder>, stack: u32) -> Divider {
    s.layout(STACK_VIEW).dividers.into_iter()
        .find(|d| s.state.workspace.layout.collapsed_divider_columns(d).contains(&Some(stack)))
        .unwrap()
}

#[test]
fn closed_multi_column_stacks_open_the_member_beside_the_contact_without_resizing_the_stack() {
    for on_left in [false, true] {
        for lower in [false, true] {
            let (mut s, left, right) = stack_fixture();
            let (source, target) = if on_left { (right, left) } else { (left, right) };
            stack_move(&mut s, source, target, false);
            let stack = s.state.workspace.layout.column_stack(target);
            let resolved = s.layout(STACK_VIEW);
            let members: Vec<_> = resolved.collapsed.iter().filter(|c| c.stack == stack.column).collect();
            let member = members[usize::from(lower)].clone();
            let other = members[usize::from(!lower)].id;
            let divider = stack_divider(&s, stack.column);
            assert!(!divider.fixed, "the closed stack edge opens a member");
            let outward = if divider.bounds.x > member.bounds.x { 1. } else { -1. };
            let start = [divider.bounds.x + divider.bounds.width * 0.5, member.bounds.y + member.bounds.height * 0.5];
            let at = |distance: f32| [start[0] + outward * distance, start[1]];
            let before = crate::durable_layout(&s.state.workspace.layout);
            let other_width = s.state.workspace.layout.expanded_column_width(other, STACK_VIEW[0]);
            let open = |s: &UiSession<Recorder>| s.state.workspace.layout.open_stack_column(stack.column);
            drag_divider(&mut s, divider.id, ContactPhase::Down, start);
            drag_divider(&mut s, divider.id, ContactPhase::Move, at(-900.));
            assert_eq!(open(&s), None, "pushing toward the screen edge opens nothing");
            drag_divider(&mut s, divider.id, ContactPhase::Move, at(TILE_SIZE - 1.));
            assert_eq!(open(&s), None);
            drag_divider(&mut s, divider.id, ContactPhase::Move, at(TILE_SIZE + 1.));
            assert_eq!(open(&s), Some(member.id), "on_left={on_left} lower={lower}");
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before, "opening a member is transient");
            drag_divider(&mut s, divider.id, ContactPhase::Move, at(0.));
            assert_eq!(open(&s), None, "reversing before the open edge closes the member again");
            drag_divider(&mut s, divider.id, ContactPhase::Move, at(900.));
            drag_divider(&mut s, divider.id, ContactPhase::Up, at(900.));
            assert_eq!(open(&s), Some(member.id));
            assert_eq!(s.state.workspace.layout.expanded_column_width(member.id, STACK_VIEW[0]), 800., "its edge resizes the open member");
            assert_eq!(s.state.workspace.layout.expanded_column_width(other, STACK_VIEW[0]), other_width);
            assert_eq!(s.state.workspace.layout.column_stack(target).members, stack.members);
            assert!(s.command(CommandId::UndoWorkspace).enabled, "the resize is one workspace step");
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            s.state.workspace.layout.column_stack_mut(stack.column).open_column = None;
            let divider = stack_divider(&s, stack.column);
            s.dispatch(UiAction::NudgeDivider { id: divider.id, forward: true, viewport: STACK_VIEW }).unwrap();
            s.dispatch(UiAction::ResetColumnWidth { id: divider.id, viewport: STACK_VIEW }).unwrap();
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before, "keyboard and reset never expand the aggregate");
        }
    }
}

#[test]
fn closed_multi_column_stacks_with_individual_panels_keep_a_fixed_edge() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    let stack = s.state.workspace.layout.column_stack(left).column;
    s.state.workspace.layout.column_stack_mut(stack).drawers = true;
    let divider = stack_divider(&s, stack);
    assert!(divider.fixed, "no member column can open, so hosts omit the affordance");
    let start = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + 100.];
    let before = crate::durable_layout(&s.state.workspace.layout);
    for offset in [-900., 900.] {
        drag_divider(&mut s, divider.id, ContactPhase::Down, start);
        drag_divider(&mut s, divider.id, ContactPhase::Move, [start[0] + offset, start[1]]);
        drag_divider(&mut s, divider.id, ContactPhase::Up, [start[0] + offset, start[1]]);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
        assert_eq!(s.state.workspace.layout.open_stack_column(stack), None);
    }
}

#[test]
fn dragging_an_open_member_inward_closes_it_and_reversing_reopens_it() {
    for panel in [Panel::Brushes, Panel::Layers] {
        let (mut s, ..) = stack_fixture();
        click_column(&mut s, panel);
        let group = s.state.workspace.layout.panel_group(panel).unwrap();
        let column = s.state.workspace.layout.collapsed_column_for_group(group).unwrap();
        assert_eq!(s.state.workspace.layout.open_stack_column(column), Some(column));
        let resolved = s.layout(STACK_VIEW);
        let divider = resolved.dividers.iter().find(|d| resolved.open_column_at_divider(d.id) == Some(column)).unwrap().clone();
        let open = resolved.collapsed.iter().find_map(|c| c.open.clone()).unwrap();
        let inward = if open.direction == Edge::Left { 1. } else { -1. };
        let start = [divider.bounds.x + divider.bounds.width * 0.5, divider.bounds.y + 200.];
        let at = |distance: f32| [start[0] + inward * distance, start[1]];
        let before = crate::durable_layout(&s.state.workspace.layout);
        let width = s.state.workspace.layout.expanded_column_width(column, STACK_VIEW[0]);
        let minimum = s.state.workspace.layout.open_column_minimum(column);
        let undo = s.command(CommandId::UndoWorkspace).enabled;
        let closing = open.bounds.width - (minimum - 3. * TILE_SIZE).max(TILE_SIZE);
        drag_divider(&mut s, divider.id, ContactPhase::Down, start);
        drag_divider(&mut s, divider.id, ContactPhase::Move, at(closing - 1.));
        assert_eq!(s.state.workspace.layout.open_stack_column(column), Some(column), "{panel:?} stays open at its minimum");
        drag_divider(&mut s, divider.id, ContactPhase::Move, at(closing + 1.));
        assert_eq!(s.state.workspace.layout.open_stack_column(column), None, "{panel:?} closes past the collapse distance");
        assert_eq!(s.state.workspace.layout.expanded_column_width(column, STACK_VIEW[0]), width, "closing keeps the width from before the drag");
        drag_divider(&mut s, divider.id, ContactPhase::Move, at(0.));
        assert_eq!(s.state.workspace.layout.open_stack_column(column), Some(column), "{panel:?} reopens on reversal");
        assert_eq!(s.state.workspace.layout.expanded_column_width(column, STACK_VIEW[0]), width);
        drag_divider(&mut s, divider.id, ContactPhase::Move, at(closing + 40.));
        drag_divider(&mut s, divider.id, ContactPhase::Up, at(closing + 40.));
        assert_eq!(s.state.workspace.layout.open_stack_column(column), None);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), before, "closing is transient");
        assert_eq!(s.command(CommandId::UndoWorkspace).enabled, undo, "closing adds no layout history");
    }
}

#[test]
fn stacking_and_unstacking_preserve_member_trees_and_widths() {
    let (mut s, left, right) = stack_fixture();
    let trees = [left, right].map(|id| s.state.workspace.layout.node(id).unwrap().clone());
    let widths = [left, right].map(|id| s.state.workspace.layout.expanded_column_width(id, STACK_VIEW[0]));
    stack_move(&mut s, right, left, false);
    let stack = s.state.workspace.layout.column_stack(left);
    assert_eq!(stack.members, [left, right]);
    let resolved = s.layout(STACK_VIEW);
    let members: Vec<_> = resolved
        .collapsed
        .iter()
        .filter(|c| c.stack == stack.column)
        .collect();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].bounds.x, members[1].bounds.x);
    assert_eq!(
        members[0].bounds.y + members[0].bounds.height + WORKSPACE_SPACING,
        members[1].bounds.y
    );
    assert!(members.iter().all(|c| c.groups[0].divider.height == 0.));
    let gap = [
        members[0].bounds.x + TILE_SIZE * 0.5,
        members[1].bounds.y - WORKSPACE_SPACING * 0.5,
    ];
    assert!(
        matches!(s.state.workspace.layout.column_drop_hint(&resolved, right, gap).unwrap().target,
        DockTarget::StackColumn { column, before: false } if column == left)
    );
    assert!(members.iter().all(|c| c.bounds.width == TILE_SIZE));
    for (index, id) in [left, right].into_iter().enumerate() {
        assert_eq!(s.state.workspace.layout.node(id).unwrap(), &trees[index]);
        assert_eq!(
            s.state.workspace.layout.expanded_column_width(id, STACK_VIEW[0]),
            widths[index]
        );
    }
    s.dispatch(UiAction::MoveColumn {
        column: right,
        target: DockTarget::Edge {
            edge: Edge::Right,
            outer: true,
        },
        viewport: STACK_VIEW,
    })
    .unwrap();
    for id in [left, right] {
        assert_eq!(s.state.workspace.layout.column_stack(id).members, [id]);
    }
    s.state.workspace.validate().unwrap();
}

#[test]
fn stack_reorder_and_gesture_cancellation_have_one_history_step() {
    let (mut s, left, right) = stack_fixture();
    let before = crate::durable_layout(&s.state.workspace.layout);
    let resolved = s.layout(STACK_VIEW);
    let source = resolved
        .collapsed
        .iter()
        .find(|c| c.id == right)
        .unwrap()
        .grip;
    let target = resolved
        .collapsed
        .iter()
        .find(|c| c.id == left)
        .unwrap()
        .grip;
    let at = |b: Bounds| [b.x + b.width / 2., b.y + b.height / 2.];
    for end in [ContactPhase::Cancel, ContactPhase::Up] {
        for (phase, position) in [
            (ContactPhase::Down, at(source)),
            (ContactPhase::Move, at(target)),
            (end, at(target)),
        ] {
            drag(&mut s, DockItem::Column { column: right }, phase, position, STACK_VIEW);
        }
        if end == ContactPhase::Cancel {
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
        }
    }
    let after = crate::durable_layout(&s.state.workspace.layout);
    assert_ne!(before, after);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
    stack_move(&mut s, right, left, true);
    assert_eq!(
        s.state.workspace.layout.column_stack(left).members,
        [right, left]
    );
}

#[test]
fn opening_a_member_uses_all_ordinary_groups_and_switches_within_the_stack() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    set_drawers(&mut s, left, false);
    let tree = s.state.workspace.layout.node(left).unwrap().clone();
    click_column(&mut s, Panel::Brushes);
    assert!(
        s.state.customization.column_drawers.is_empty(),
        "normal dock widgets render an open member"
    );
    let resolved = s.layout(STACK_VIEW);
    let c = resolved.collapsed.iter().find(|c| c.id == left).unwrap();
    let open = c.open.as_ref().unwrap();
    assert!((open.bounds.x - c.bounds.x - c.bounds.width - WORKSPACE_SPACING).abs() < 0.01);
    assert_eq!(resolved.work_area.x - open.bounds.x - open.bounds.width, WORKSPACE_SPACING);
    assert_eq!(open.bounds.y, HEADER_HEIGHT);
    assert_eq!(
        open.bounds.y + open.bounds.height,
        STACK_VIEW[1] - WORKSPACE_SPACING
    );
    for group in &c.groups {
        let expanded = resolved
            .groups
            .iter()
            .find(|g| g.id == group.group)
            .unwrap();
        assert_eq!(expanded.active, group.active);
        assert_eq!(expanded.panels.len(), group.icons.len());
        assert!(!expanded.floating);
        assert!(open.connections.iter().any(|(p, _)| *p == group.active));
    }
    assert_eq!(s.state.workspace.layout.node(left).unwrap(), &tree);
    click_column(&mut s, Panel::Layers);
    assert_eq!(
        s.state.workspace.layout.column_stack(left).open_column,
        Some(right)
    );
    assert_eq!(
        s.layout(STACK_VIEW)
            .collapsed
            .iter()
            .filter(|c| c.open.is_some())
            .count(),
        1
    );
    click_column(&mut s, Panel::Layers);
    assert!(
        s.state
            .workspace
            .layout
            .column_stack(left)
            .open_column
            .is_none()
    );
}

#[test]
fn stack_preferences_and_members_persist_but_open_state_does_not() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    set_drawers(&mut s, right, false);
    set_auto_hide(&mut s, right, true);
    let before = crate::durable_layout(&s.state.workspace.layout);
    click_column(&mut s, Panel::Brushes);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    let json = serde_json::to_value(&s.state.workspace).unwrap();
    let restored: WorkspaceState = serde_json::from_value(json).unwrap();
    restored.validate().unwrap();
    let stack = restored.layout.column_stack(right);
    assert_eq!(stack.members, [left, right]);
    assert!(stack.auto_hide && !stack.drawers && stack.open_column.is_none());
}

#[test]
fn auto_hide_consumes_canvas_contact_and_preserves_popup_and_nested_drawer_contacts() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    set_drawers(&mut s, left, false);
    set_auto_hide(&mut s, left, true);
    click_column(&mut s, Panel::Brushes);
    let point = [900., 700.];
    for facts in [
        ChromeFacts {
            popup_open: true,
            ..Default::default()
        },
        ChromeFacts {
            content_drawer: Some(Bounds {
                x: 850.,
                y: 650.,
                width: 100.,
                height: 100.,
            }),
            ..Default::default()
        },
    ] {
        s.input(UiInput::Chrome {
            event: ChromeEvent::Contact {
                position: point,
                canvas: true,
            },
            facts,
            viewport: STACK_VIEW,
        })
        .unwrap();
        assert_eq!(
            s.state.workspace.layout.column_stack(left).open_column,
            Some(left)
        );
    }
    let reply = s
        .input(UiInput::Chrome {
            event: ChromeEvent::Contact {
                position: point,
                canvas: true,
            },
            facts: ChromeFacts::default(),
            viewport: STACK_VIEW,
        })
        .unwrap();
    assert!(reply.handled);
    assert!(
        s.state
            .workspace
            .layout
            .column_stack(left)
            .open_column
            .is_none()
    );
}

#[test]
fn individual_panels_open_one_ordinary_drawer_per_stack() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    set_drawers(&mut s, left, true);
    click_column(&mut s, Panel::Brushes);
    assert_eq!(s.state.customization.column_drawers.len(), 1);
    assert!(s.state.customization.column_drawers[0].tabs.is_some());
    click_column(&mut s, Panel::Layers);
    assert_eq!(s.state.customization.column_drawers.len(), 1);
    assert!(
        s.layout(STACK_VIEW)
            .collapsed
            .iter()
            .all(|c| c.open.is_none())
    );
}

#[test]
fn ordinary_dividers_resize_open_columns_without_rebuilding_or_expanding_the_stack() {
    let (mut s, left, _) = stack_fixture();
    set_drawers(&mut s, left, false);
    click_column(&mut s, Panel::Brushes);
    let resolved = s.layout(STACK_VIEW);
    let ids: Vec<_> = resolved
        .dividers
        .iter()
        .filter(|d| {
            resolved.open_column_at_divider(d.id) == Some(left)
                || s.state.workspace.layout.column_contains(left, d.id)
        })
        .map(|d| d.id)
        .collect();
    assert!(ids.len() >= 2);
    for id in ids {
        let resolved = s.layout(STACK_VIEW);
        let d = resolved.dividers.iter().find(|d| d.id == id).unwrap();
        let start = [
            d.bounds.x + d.bounds.width / 2.,
            d.bounds.y + d.bounds.height / 2.,
        ];
        let mut moved = start;
        moved[usize::from(d.axis == Axis::Vertical)] += 45.;
        let before = crate::durable_layout(&s.state.workspace.layout);
        for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up] {
            let revision = s.workspace_content_revision();
            s.dispatch(UiAction::DragDivider {
                id,
                phase,
                position: if phase == ContactPhase::Down {
                    start
                } else {
                    moved
                },
                viewport: STACK_VIEW,
            })
            .unwrap();
            if phase == ContactPhase::Move {
                assert_eq!(s.workspace_content_revision(), revision);
            }
        }
        let after = crate::durable_layout(&s.state.workspace.layout);
        assert_ne!(before, after);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
        assert_eq!(
            s.state.workspace.layout.column_stack(left).open_column,
            Some(left)
        );
        invoke(&mut s, CommandId::RedoWorkspace);
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
    }
}

#[test]
fn adding_and_removing_groups_updates_member_identity_without_losing_panels() {
    let (mut s, left, right) = stack_fixture();
    set_drawers(&mut s, right, false);
    stack_move(&mut s, left, right, true);
    let group = s.state.workspace.layout.panel_group(Panel::Layers).unwrap();
    customize(
        &mut s,
        CustomizationAction::SetPanelVisible {
            panel: Panel::Color,
            visible: true,
        },
    );
    s.dispatch(UiAction::MovePanel {
        panel: Panel::Color,
        target: DockTarget::Split {
            group,
            edge: Edge::Top,
        },
        viewport: STACK_VIEW,
    })
    .unwrap();
    let member = s
        .state
        .workspace
        .layout
        .collapsed_column_for_group(group)
        .unwrap();
    assert_ne!(member, right);
    assert_eq!(
        s.state.workspace.layout.column_stack(left).members,
        [left, member]
    );
    click_column(&mut s, Panel::Layers);
    assert!(
        s.layout(STACK_VIEW)
            .groups
            .iter()
            .any(|g| g.active == Panel::Color)
    );
    customize(
        &mut s,
        CustomizationAction::SetPanelVisible {
            panel: Panel::Color,
            visible: false,
        },
    );
    assert_eq!(
        s.state.workspace.layout.column_stack(left).members,
        [left, right]
    );
    assert_eq!(
        s.state.workspace.layout.column_stack(left).open_column,
        Some(right)
    );
    s.state.workspace.validate().unwrap();
}

#[test]
fn member_can_unstack_beside_its_own_stack_and_use_the_existing_expand_action() {
    for expand in [false, true] {
        let (mut s, left, right) = stack_fixture();
        stack_move(&mut s, right, left, false);
        let stack = s.state.workspace.layout.column_stack(left).column;
        if expand {
            set_collapsed(&mut s, right, false);
            assert!(
                s.layout(STACK_VIEW)
                    .groups
                    .iter()
                    .any(|g| g.panels.contains(&Panel::Layers))
            );
        } else {
            s.dispatch(UiAction::MoveColumn {
                column: right,
                target: DockTarget::Split {
                    group: stack,
                    edge: Edge::Right,
                },
                viewport: STACK_VIEW,
            })
            .unwrap();
        }
        assert_eq!(s.state.workspace.layout.column_stack(left).members, [left]);
        assert_eq!(
            s.state.workspace.layout.column_stack(right).members,
            [right]
        );
        s.state.workspace.validate().unwrap();
    }
}

#[test]
fn cancelled_or_blurred_column_resize_restores_width_splits_and_open_member() {
    for blur in [false, true] {
        let (mut s, left, right) = stack_fixture();
        stack_move(&mut s, right, left, false);
        set_drawers(&mut s, left, false);
        click_column(&mut s, Panel::Brushes);
        let resolved = s.layout(STACK_VIEW);
        let dividers: Vec<_> = resolved
            .dividers
            .iter()
            .filter(|d| d.id == left || resolved.open_column_at_divider(d.id) == Some(left))
            .cloned()
            .collect();
        for d in dividers {
            let before = crate::durable_layout(&s.state.workspace.layout);
            let start = [
                d.bounds.x + d.bounds.width / 2.,
                d.bounds.y + d.bounds.height / 2.,
            ];
            let mut moved = start;
            moved[usize::from(d.axis == Axis::Vertical)] += 50.;
            for (phase, position) in [(ContactPhase::Down, start), (ContactPhase::Move, moved)] {
                s.dispatch(UiAction::DragDivider {
                    id: d.id,
                    phase,
                    position,
                    viewport: STACK_VIEW,
                })
                .unwrap();
            }
            assert_ne!(crate::durable_layout(&s.state.workspace.layout), before);
            if blur {
                s.input(UiInput::Blur).unwrap();
            } else {
                s.dispatch(UiAction::DragDivider {
                    id: d.id,
                    phase: ContactPhase::Cancel,
                    position: moved,
                    viewport: STACK_VIEW,
                })
                .unwrap();
            }
            assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
            assert_eq!(
                s.state.workspace.layout.column_stack(left).open_column,
                Some(left)
            );
        }
    }
}

#[test]
fn stack_validation_rejects_missing_overlapping_and_incomplete_members() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    let layout = s.state.workspace.layout;
    let stack = layout.column_stack(left).column;
    for members in [
        vec![],
        vec![left, left],
        vec![left, u32::MAX],
        vec![left],
        vec![stack, left],
    ] {
        let mut invalid = layout.clone();
        invalid.column_stack_mut(stack).members = members;
        assert!(invalid.validate().is_err());
    }
    let mut missing = layout;
    missing.column_stacks.push(ColumnStack::single(u32::MAX));
    assert!(missing.validate().is_err());
}

#[test]
fn resetting_open_member_width_preserves_stack_membership_and_open_state() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    set_drawers(&mut s, left, false);
    click_column(&mut s, Panel::Brushes);
    let resolved = s.layout(STACK_VIEW);
    let id = resolved
        .dividers
        .iter()
        .find(|d| resolved.open_column_at_divider(d.id) == Some(left))
        .unwrap()
        .id;
    s.dispatch(UiAction::ResetColumnWidth {
        id,
        viewport: STACK_VIEW,
    })
    .unwrap();
    assert_eq!(
        s.state.workspace.layout.column_stack(left).members,
        [left, right]
    );
    assert_eq!(
        s.state.workspace.layout.column_stack(left).open_column,
        Some(left)
    );
    let resolved = s.layout(STACK_VIEW);
    let open = resolved
        .collapsed
        .iter()
        .find(|c| c.id == left)
        .unwrap()
        .open
        .as_ref()
        .unwrap();
    assert!(open.bounds.width >= Panel::Brushes.default_width(STACK_VIEW[0]));
    assert!(resolved.groups.iter().any(|g| g.active == Panel::Brushes));
}

#[test]
fn adopting_drawers_off_closes_existing_drawer_presentations() {
    for merge in [false, true] {
        let (mut s, left, right) = stack_fixture();
        s.state.workspace.layout.column_stack_mut(right).drawers = true;
        set_drawers(&mut s, left, false);
        click_column(&mut s, Panel::Layers);
        assert_eq!(s.state.customization.column_drawers.len(), 1);
        if merge {
            stack_move(&mut s, right, left, false);
        } else {
            customize(
                &mut s,
                CustomizationAction::ApplyColumnStack { column: left },
            );
        }
        assert!(s.state.customization.column_drawers.is_empty());
        click_column(&mut s, Panel::Layers);
        assert_eq!(
            s.state.workspace.layout.column_stack(right).open_column,
            Some(right)
        );
    }
}

#[test]
fn same_order_drop_from_member_into_previous_column_is_not_cancelled() {
    let mut s = three_member_target();
    let before = crate::durable_layout(&s.state.workspace.layout);
    let resolved = s.layout(STACK_VIEW);
    let source = resolved.collapsed[2].groups[0].icons[0].bounds;
    let last = resolved.collapsed[1].groups.last().unwrap();
    let target = [last.bounds.x + 18., last.bounds.y + last.bounds.height + 3.];
    let item = DockItem::Panel { panel: Panel::Color };
    for (phase, position) in [
        (ContactPhase::Down, [source.x + 18., source.y + 18.]),
        (ContactPhase::Move, [900., 500.]),
        (ContactPhase::Move, target),
        (ContactPhase::Up, target),
    ] {
        s.dispatch(UiAction::DragWorkspace { item, phase, position, viewport: STACK_VIEW, tabs: Vec::new() }).unwrap();
    }
    let after = crate::durable_layout(&s.state.workspace.layout);
    assert_eq!(s.layout(STACK_VIEW).collapsed.len(), 2);
    assert_eq!(s.layout(STACK_VIEW).collapsed[1].groups.len(), 2);
    assert_ne!(before, after);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), after);
}

fn set_drawers(s: &mut UiSession<Recorder>, column: u32, drawers: bool) {
    customize(s, CustomizationAction::SetColumnDrawers { column, drawers });
}
fn set_auto_hide(s: &mut UiSession<Recorder>, column: u32, auto_hide: bool) {
    customize(s, CustomizationAction::SetColumnAutoHide { column, auto_hide });
}

fn set_collapsed(s: &mut UiSession<Recorder>, group: u32, collapsed: bool) {
    customize(s, CustomizationAction::SetColumnCollapsed { group, collapsed });
}

#[test]
fn column_edges_act_on_the_column_on_their_screen_side() {
    fn groups(node: &DockNode, out: &mut Vec<u32>) {
        match node {
            DockNode::Tabs { id, .. } => out.push(*id),
            DockNode::Split { first, second, .. } => { groups(first, out); groups(second, out); }
        }
    }
    let width = |s: &UiSession<Recorder>, node: &DockNode| {
        let mut ids = Vec::new();
        groups(node, &mut ids);
        let r = s.layout(STACK_VIEW);
        let bounds: Vec<_> = r.groups.iter().filter(|g| ids.contains(&g.id)).map(|g| g.bounds).collect();
        bounds.iter().map(|b| b.x + b.width).fold(f32::MIN, f32::max) - bounds.iter().map(|b| b.x).fold(f32::MAX, f32::min)
    };
    let center = |d: &Divider| [d.bounds.x + d.bounds.width * 0.5, 400.];
    for panel in [Panel::Navigator, Panel::Layers] {
        let mut s = session(Platform::Gtk);
        s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(crate::WorkspaceState { layout: WorkspacePreset::Illustrator.layout(Platform::Gtk), ..Default::default() }) }).unwrap();
        let toolbar = s.state.workspace.layout.panel_group(Panel::Toolbar).unwrap();
        let band = s.state.workspace.layout.bands.iter().find(|b| b.root.group_for(Panel::Toolbar).is_some()).unwrap().id;
        s.dispatch(UiAction::MovePanel { panel, target: DockTarget::Split { group: toolbar, edge: Edge::Right }, viewport: STACK_VIEW }).unwrap();
        let root = s.state.workspace.layout.bands.iter().find(|b| b.id == band).unwrap().root.clone();
        let DockNode::Split { id: gap, axis: Axis::Horizontal, first, second, .. } = &root else { panic!("{panel:?} shares the band: {root:?}") };
        let (outer, inner) = ((**first).clone(), (**second).clone());
        assert!(outer.group_for(Panel::Toolbar).is_some());
        let [outer_width, inner_width] = [width(&s, &outer), width(&s, &inner)];
        let divider = |s: &UiSession<Recorder>, id: u32| s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.id == id).unwrap();
        assert_eq!(divider(&s, *gap).owner_side, Some(Edge::Left));

        let start = center(&divider(&s, *gap));
        drag_divider(&mut s, *gap, ContactPhase::Down, start);
        drag_divider(&mut s, *gap, ContactPhase::Move, [start[0] + 40., start[1]]);
        assert!((width(&s, &outer) - outer_width - 40.).abs() < 0.5, "{panel:?}: dragging the gap outward grows the toolbar");
        assert!((width(&s, &inner) - inner_width).abs() < 0.5, "{panel:?}: the column farther in keeps its width");
        drag_divider(&mut s, *gap, ContactPhase::Move, [start[0] - 200., start[1]]);
        assert!(!s.state.workspace.layout.is_collapsed(inner.id()) && !s.state.workspace.layout.is_collapsed(outer.id()), "{panel:?}: a toolbar never collapses");
        assert!((width(&s, &outer) - TILE_SIZE).abs() < 0.5, "{panel:?}: the toolbar stops at one tile: {}", width(&s, &outer));
        assert!((width(&s, &inner) - inner_width).abs() < 0.5, "{panel:?}: the inner column keeps its width");
        drag_divider(&mut s, *gap, ContactPhase::Up, [start[0] - 200., start[1]]);

        let start = center(&divider(&s, band));
        drag_divider(&mut s, band, ContactPhase::Down, start);
        drag_divider(&mut s, band, ContactPhase::Move, [start[0] - 20., start[1]]);
        assert!((width(&s, &outer) - TILE_SIZE).abs() < 0.5, "{panel:?}: the band edge leaves the toolbar alone");
        assert!((inner_width - 20. - width(&s, &inner)).abs() < 0.5, "{panel:?}: the band edge resizes the column touching it");
        drag_divider(&mut s, band, ContactPhase::Move, [start[0] - inner_width, start[1]]);
        assert!(s.state.workspace.layout.is_collapsed(inner.id()), "{panel:?}: the touching column collapses");
        assert!((width(&s, &outer) - TILE_SIZE).abs() < 0.5);
        drag_divider(&mut s, band, ContactPhase::Move, start);
        assert!(!s.state.workspace.layout.is_collapsed(inner.id()), "{panel:?}: reversing reopens it");
        drag_divider(&mut s, band, ContactPhase::Up, start);
        assert!((width(&s, &inner) - inner_width).abs() < 0.5);
    }
}

#[test]
fn a_column_of_toolbars_shrinks_to_one_tile_without_collapsing() {
    let mut s = session(Platform::Gtk);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(crate::WorkspaceState { layout: WorkspacePreset::Illustrator.layout_for_device(Platform::Gtk, true), ..Default::default() }) }).unwrap();
    let band = s.state.workspace.layout.bands.iter().find(|b| b.root.group_for(Panel::Toolbar).is_some()).unwrap().clone();
    assert!(matches!(band.root, DockNode::Split { axis: Axis::Vertical, .. }), "the tablet layout stacks two toolbars");
    let d = s.layout(STACK_VIEW).dividers.into_iter().find(|d| d.id == band.id).unwrap();
    let start = [d.bounds.x + d.bounds.width * 0.5, 400.];
    drag_divider(&mut s, band.id, ContactPhase::Down, start);
    drag_divider(&mut s, band.id, ContactPhase::Move, [0., start[1]]);
    drag_divider(&mut s, band.id, ContactPhase::Up, [0., start[1]]);
    assert!(!s.state.workspace.layout.is_collapsed(band.root.id()));
    for g in s.layout(STACK_VIEW).groups.iter().filter(|g| g.panels.iter().all(|p| p.kind() == PanelKind::Tiles) && g.bounds.x < 20.) {
        assert_eq!(g.bounds.width, TILE_SIZE, "{:?} keeps one lane", g.panels);
    }
}

#[test]
fn panel_vector_cache_keeps_original_bodies_through_anchor_eligibility_keyframes() {
    for edge in [Edge::Left,Edge::Right] {
        let mut cache=crate::PanelVectorCache::default();
        let original=column_bridge_fixture(edge).with_panel_bridges();
        let mut ids=None;
        let mut connected=false;let mut body_only=false;
        for y in [100.,246.,253.,259.,260.,261.,266.,270.,300.,270.,260.,253.,100.] {
            let mut layout=original.clone();
            layout.collapsed[0].groups[0].icons[0].bounds.y=y;
            let current=panel_vector_cache_resolve(&mut cache,layout,0.);
            let current_ids=current.vector_surfaces.iter().map(|s|s.group).collect::<Vec<_>>();
            if let Some(ids)=&ids {assert_eq!(&current_ids,ids);} else {ids=Some(current_ids);}
            let receiver=current.vector_surfaces.iter().find(|s|s.group==5).unwrap();
            let bodies=receiver.commands.iter().filter(|c|c[0]==0).count();
            if bodies==1 {
                body_only=true;assert!(receiver.triangles.is_empty(),"original body must not supply backdrop filler at anchor y={y}");
            } else {connected=true;}
        }
        assert!(connected && body_only,"keyframes must cross the attachment eligibility boundary");
        let mut remote=original.clone();
        remote.collapsed[0].groups[0].icons[0].bounds.y= -10000.;
        let remote=panel_vector_cache_resolve(&mut cache,remote,0.);
        assert_eq!(remote.vector_surfaces.iter().map(|s|s.group).collect::<Vec<_>>(),ids.unwrap());
        for receiver in &remote.vector_surfaces {
            assert_eq!(receiver.commands.iter().filter(|c|c[0]==0).count(),1);
            assert!(receiver.triangles.is_empty(),"remote sources must leave only the original rounded body");
        }
    }
}
