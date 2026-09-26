const STACK_VIEW: [f32; 2] = [1800., 1100.];

fn three_member_target() -> UiSession<Recorder> {
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
    for (platform, index) in [
        Platform::Gtk,
        Platform::Web,
        Platform::Android,
        Platform::Windows,
        Platform::Mac,
        Platform::Ios,
    ]
    .into_iter()
    .flat_map(|p| (0..3).map(move |i| (p, i)))
    {
        for item in [
            DockItem::Panel {
                panel: Panel::Properties,
            },
            DockItem::Group { group: 8 },
            DockItem::Group { group: 2 },
        ] {
            for offset in [-5., 0., 5.] {
                let mut s = three_member_target();
                s.set_platform(platform);
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
fn trailing_group_targets_follow_scrolling_and_stay_out_of_grips() {
    let mut s = three_member_target();
    let viewport = [STACK_VIEW[0], 220.];
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
                assert!(layout.expanded_column_width(member) >= 128.);
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
        (gap, left),
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
    for platform in [Platform::Web, Platform::Android, Platform::Windows, Platform::Mac, Platform::Ios] {
        s.set_platform(platform);
        for point in [center(below.empty), center(below.grip), gap] {
            assert!(matches!(s.drop_hint(STACK_VIEW, point, &[], item, None).unwrap().target, DockTarget::StackColumn { .. }));
        }
    }
    s.set_platform(Platform::Gtk);
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
        let mut s = session();
        s.set_platform(Platform::Gtk);
        stack_edit(
            &mut s,
            CustomizationAction::SetColumnCollapsed {
                group: 5,
                collapsed: true,
            },
        );
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
                s.dispatch(UiAction::DragWorkspace {
                    item,
                    phase,
                    position,
                    viewport: STACK_VIEW,
                    tabs: Vec::new(),
                })
                .unwrap();
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
    for platform in [
        Platform::Gtk,
        Platform::Web,
        Platform::Android,
        Platform::Windows,
        Platform::Mac,
        Platform::Ios,
    ] {
        check_paint_default_stack(platform);
    }
}

fn check_paint_default_stack(platform: Platform) {
    let mut s = session();
    s.set_platform(platform);
    let layout = crate::WorkspacePreset::Illustrator.layout(platform);
    let capture = crate::WorkspaceCapture {
        history: crate::LayoutHistory::new(&layout),
        working: crate::WorkspacePreset::Illustrator.working_state(),
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
        }
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

fn stack_edit(s: &mut UiSession<Recorder>, action: CustomizationAction) {
    s.dispatch(UiAction::Customize { action }).unwrap();
}
fn stack_fixture() -> (UiSession<Recorder>, u32, u32) {
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let mut columns = Vec::new();
    for panel in [Panel::Brushes, Panel::Layers] {
        let group = s.state.workspace.layout.panel_group(panel).unwrap();
        stack_edit(
            &mut s,
            CustomizationAction::SetColumnCollapsed {
                group,
                collapsed: true,
            },
        );
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
    stack_edit(s, CustomizationAction::ToggleColumnDrawer { group, panel });
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

#[test]
fn closed_multi_column_stacks_ignore_width_resize_without_expanding_their_tree() {
    for on_left in [false, true] {
        let (mut s, left, right) = stack_fixture();
        let (source, target) = if on_left {
            (right, left)
        } else {
            (left, right)
        };
        stack_move(&mut s, source, target, false);
        let stack = s.state.workspace.layout.column_stack(target).column;
        let resolved = s.layout(STACK_VIEW);
        let divider = resolved
            .dividers
            .iter()
            .find(|d| {
                s.state
                    .workspace
                    .layout
                    .collapsed_divider_columns(d)
                    .contains(&Some(stack))
            })
            .unwrap();
        assert!(divider.fixed, "Hosts must omit this resize affordance");
        let id = divider.id;
        let start = [
            divider.bounds.x + divider.bounds.width * 0.5,
            divider.bounds.y + 100.,
        ];
        let before = crate::durable_layout(&s.state.workspace.layout);
        for offset in [-900., 900.] {
            for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up] {
                s.dispatch(UiAction::DragDivider {
                    id,
                    phase,
                    viewport: STACK_VIEW,
                    position: if phase == ContactPhase::Down {
                        start
                    } else {
                        [start[0] + offset, start[1]]
                    },
                })
                .unwrap();
                assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
                s.state.workspace.validate().unwrap();
            }
        }
        s.dispatch(UiAction::NudgeDivider {
            id,
            forward: true,
            viewport: STACK_VIEW,
        })
        .unwrap();
        s.dispatch(UiAction::ResetColumnWidth {
            id,
            viewport: STACK_VIEW,
        })
        .unwrap();
        assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    }
}

#[test]
fn stacking_and_unstacking_preserve_member_trees_and_widths() {
    let (mut s, left, right) = stack_fixture();
    let trees = [left, right].map(|id| s.state.workspace.layout.node(id).unwrap().clone());
    let widths = [left, right].map(|id| s.state.workspace.layout.expanded_column_width(id));
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
            s.state.workspace.layout.expanded_column_width(id),
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
            s.dispatch(UiAction::DragWorkspace {
                item: DockItem::Column { column: right },
                phase,
                position,
                viewport: STACK_VIEW,
                tabs: Vec::new(),
            })
            .unwrap();
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
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: left,
            drawers: false,
        },
    );
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
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: right,
            drawers: false,
        },
    );
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnAutoHide {
            column: right,
            auto_hide: true,
        },
    );
    let before = crate::durable_layout(&s.state.workspace.layout);
    click_column(&mut s, Panel::Brushes);
    assert_eq!(crate::durable_layout(&s.state.workspace.layout), before);
    let json = serde_json::to_value(&s.state.workspace).unwrap();
    assert!(json["layout"].get("column_settings").is_none());
    let restored: WorkspaceState = serde_json::from_value(json).unwrap();
    restored.validate().unwrap();
    let stack = restored.layout.column_stack(right);
    assert_eq!(stack.members, [left, right]);
    assert!(stack.auto_hide && !stack.drawers && stack.open_column.is_none());
}

#[test]
fn legacy_group_panel_preferences_migrate_to_drawers_off() {
    let (s, left, _) = stack_fixture();
    let mut json = serde_json::to_value(&s.state.workspace).unwrap();
    json["layout"]
        .as_object_mut()
        .unwrap()
        .remove("column_stacks");
    json["layout"]["column_settings"] = serde_json::json!([{
        "column": left, "mode": "group_panel", "auto_hide": true,
        "width": 380, "heights": [{"group": 5, "panel": "brushes", "weight": 12}]
    }]);
    let restored: WorkspaceState = serde_json::from_value(json).unwrap();
    restored.validate().unwrap();
    let stack = restored.layout.column_stack(left);
    assert!(!stack.drawers && stack.auto_hide);
    assert_eq!(stack.members, [left]);
}

#[test]
fn auto_hide_consumes_canvas_contact_and_preserves_popup_and_nested_drawer_contacts() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: left,
            drawers: false,
        },
    );
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnAutoHide {
            column: left,
            auto_hide: true,
        },
    );
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
fn individual_panels_open_one_ordinary_drawer_per_stack_on_every_host() {
    for platform in [
        Platform::Gtk,
        Platform::Web,
        Platform::Android,
        Platform::Mac,
        Platform::Ios,
        Platform::Windows,
    ] {
        let (mut s, left, right) = stack_fixture();
        s.set_platform(platform);
        stack_move(&mut s, right, left, false);
        stack_edit(
            &mut s,
            CustomizationAction::SetColumnDrawers {
                column: left,
                drawers: true,
            },
        );
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
}

#[test]
fn ordinary_dividers_resize_open_columns_without_rebuilding_or_expanding_the_stack() {
    let (mut s, left, _) = stack_fixture();
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: left,
            drawers: false,
        },
    );
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
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: right,
            drawers: false,
        },
    );
    stack_move(&mut s, left, right, true);
    let group = s.state.workspace.layout.panel_group(Panel::Layers).unwrap();
    stack_edit(
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
    stack_edit(
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
            stack_edit(
                &mut s,
                CustomizationAction::SetColumnCollapsed {
                    group: right,
                    collapsed: false,
                },
            );
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
        stack_edit(
            &mut s,
            CustomizationAction::SetColumnDrawers {
                column: left,
                drawers: false,
            },
        );
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
    let mut legacy = layout;
    legacy.column_stacks.push(ColumnStack::single(u32::MAX));
    legacy.validate().unwrap();
}

#[test]
fn resetting_open_member_width_preserves_stack_membership_and_open_state() {
    let (mut s, left, right) = stack_fixture();
    stack_move(&mut s, right, left, false);
    stack_edit(
        &mut s,
        CustomizationAction::SetColumnDrawers {
            column: left,
            drawers: false,
        },
    );
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
    assert!(open.bounds.width >= Panel::Brushes.default_width());
    assert!(resolved.groups.iter().any(|g| g.active == Panel::Brushes));
}

#[test]
fn adopting_drawers_off_closes_existing_drawer_presentations() {
    for merge in [false, true] {
        let (mut s, left, right) = stack_fixture();
        s.state.workspace.layout.column_stack_mut(right).drawers = true;
        stack_edit(
            &mut s,
            CustomizationAction::SetColumnDrawers {
                column: left,
                drawers: false,
            },
        );
        click_column(&mut s, Panel::Layers);
        assert_eq!(s.state.customization.column_drawers.len(), 1);
        if merge {
            stack_move(&mut s, right, left, false);
        } else {
            stack_edit(
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
    for platform in [
        Platform::Gtk,
        Platform::Web,
        Platform::Android,
        Platform::Windows,
    ] {
        let mut s = three_member_target();
        s.set_platform(platform);
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
}
