const VIEW: [f32; 2] = [1600., 1000.];

#[test]
fn upper_bodies_snap_to_tab_slots_in_first_lower_and_floating_groups() {
    for floating in [false, true] {
        let mut s = session(Platform::Gtk);
        let layout = &mut s.state.workspace.layout;
        layout.add_panel_to_group(Panel::Adjustments, 5).unwrap();
        layout.add_panel_to_group(Panel::Properties, 6).unwrap();
        if floating {
            layout
                .move_item(
                    VIEW,
                    DockItem::Group { group: 6 },
                    DockTarget::Float {
                        position: [800., 300.],
                    },
                )
                .unwrap();
        }
        let r = s.layout(VIEW);
        let item = DockItem::Group { group: 2 };
        for group in [5, 6] {
            let g = r.groups.iter().find(|g| g.id == group).unwrap();
            let b = g.bounds;
            let tabs: Vec<_> = (0..2)
                .map(|index| TabHit {
                    group,
                    index,
                    bounds: Bounds {
                        x: b.x + b.width * 0.4 * index as f32,
                        width: b.width * 0.4,
                        height: TAB_BAR_HEIGHT,
                        ..b
                    },
                })
                .collect();
            let boundary = b.y + TAB_BAR_HEIGHT + (b.height - TAB_BAR_HEIGHT) * 0.2;
            for (index, fraction) in [(0, 0.1), (1, 0.5), (2, 0.85)] {
                let x = b.x + b.width * fraction;
                let tab = s
                    .drop_hint(VIEW, [x, b.y + 18.], &tabs, item, None)
                    .unwrap();
                for y in [b.y + TAB_BAR_HEIGHT + 1., boundary - 1.] {
                    let hint = s.drop_hint(VIEW, [x, y], &tabs, item, None).unwrap();
                    assert_eq!(
                        hint.target,
                        DockTarget::Tab {
                            group,
                            index: Some(index)
                        }
                    );
                    assert_eq!(hint.bounds, tab.bounds);
                    assert_eq!(hint.bounds.y, b.y);
                    assert_eq!(hint.bounds.width, 3.);
                }
            }
            let hint = s
                .drop_hint(
                    VIEW,
                    [b.x + b.width * 0.5, boundary + 1.],
                    &tabs,
                    item,
                    None,
                )
                .unwrap();
            assert_eq!(
                hint.target,
                DockTarget::Tab {
                    group,
                    index: Some(0)
                }
            );
            assert_eq!(hint.bounds.width, b.width);
            assert_eq!(hint.bounds.y, b.y + TAB_BAR_HEIGHT);
        }
    }
}

#[test]
fn menubar_prepends_every_column_payload_on_both_sides() {
    for edge in [Edge::Left, Edge::Right] {
        for collapsed in [false, true] {
            for item in [
                DockItem::Panel {
                    panel: Panel::Properties,
                },
                DockItem::Group { group: 8 },
                DockItem::Group { group: 2 },
                DockItem::Column { column: 8 },
            ] {
                let mut s = session(Platform::Gtk);
                let layout = &mut s.state.workspace.layout;
                layout.bands[0].edge = edge;
                layout.bands[1].edge = if edge == Edge::Left {
                    Edge::Right
                } else {
                    Edge::Left
                };
                if collapsed {
                    layout.set_column_collapsed(5, true, VIEW).unwrap();
                }
                if matches!(item, DockItem::Column { .. }) {
                    layout.set_column_collapsed(8, true, VIEW).unwrap();
                }
                let before = crate::durable_layout(layout);
                let r = s.layout(VIEW);
                let bounds = if collapsed {
                    r.collapsed[0].bounds
                } else {
                    r.groups.iter().find(|g| g.id == 5).unwrap().bounds
                };
                let x = bounds.x + bounds.width * 0.5;
                for y in [1., HEADER_HEIGHT * 0.5, HEADER_HEIGHT - 1.] {
                    let hint = s.drop_hint(VIEW, [x, y], &[], item, None).unwrap();
                    assert_eq!(
                        hint.target,
                        if collapsed {
                            DockTarget::StackColumn {
                                column: 4,
                                before: true,
                            }
                        } else {
                            DockTarget::Split {
                                group: 5,
                                edge: Edge::Top,
                            }
                        }
                    );
                    assert!(hint.bounds.height <= 3.);
                    assert_eq!(hint.bounds.y, bounds.y);
                }
                let hint = s.drop_hint(VIEW, [x, 24.], &[], item, None).unwrap();
                s.dispatch(item.move_action(hint.target, VIEW)).unwrap();
                let layout = &s.state.workspace.layout;
                layout.validate().unwrap();
                if collapsed {
                    let stack = layout.column_stack(4);
                    assert_eq!(stack.members.len(), 2);
                    assert_eq!(stack.members[1], 4);
                    assert!(!stack.drawers);
                } else {
                    let r = s.layout(VIEW);
                    let top = r
                        .groups
                        .iter()
                        .filter(|g| g.bounds.x == bounds.x)
                        .min_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y))
                        .unwrap();
                    assert_eq!(
                        top.active,
                        match item {
                            DockItem::Panel { panel } => panel,
                            DockItem::Group { group: 2 } => Panel::Toolbar,
                            _ => Panel::Layers,
                        }
                    );
                    assert!(layout.collapsed.is_empty());
                }
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
fn group_body_prepends_and_highlights_content_while_tabs_keep_insertion_lines() {
    for floating in [false, true] {
        let mut s = session(Platform::Gtk);
        if floating {
            s.state
                .workspace
                .layout
                .move_item(
                    VIEW,
                    DockItem::Group { group: 8 },
                    DockTarget::Float {
                        position: [800., 300.],
                    },
                )
                .unwrap();
        }
        let r = s.layout(VIEW);
        let g = r.groups.iter().find(|g| g.id == 8).unwrap();
        let b = g.bounds;
        let item = DockItem::Panel {
            panel: Panel::Brushes,
        };
        for y in [b.y + b.height * 0.4, b.y + b.height * 0.5] {
            let hint = s
                .drop_hint(VIEW, [b.x + b.width * 0.5, y], &[], item, None)
                .unwrap();
            assert_eq!(
                hint.target,
                DockTarget::Tab {
                    group: 8,
                    index: Some(0)
                }
            );
            assert_eq!(
                hint.bounds,
                Bounds {
                    y: b.y + TAB_BAR_HEIGHT,
                    height: b.height - TAB_BAR_HEIGHT,
                    ..b
                }
            );
        }
        let tab = TabHit {
            group: 8,
            index: 0,
            bounds: Bounds {
                width: 100.,
                height: TAB_BAR_HEIGHT,
                ..b
            },
        };
        let hint = s
            .drop_hint(VIEW, [b.x + 5., b.y + 18.], &[tab], item, None)
            .unwrap();
        assert_eq!(
            hint.target,
            DockTarget::Tab {
                group: 8,
                index: Some(0)
            }
        );
        assert!(hint.bounds.width <= 3.);
        let hint = s
            .drop_hint(
                VIEW,
                [b.x + b.width * 0.5, b.y + b.height * 0.5],
                &[],
                item,
                None,
            )
            .unwrap();
        s.dispatch(item.move_action(hint.target, VIEW)).unwrap();
        assert_eq!(
            s.state.workspace.layout.group_panels(8).unwrap(),
            [
                Panel::Brushes,
                Panel::Layers,
                Panel::Adjustments,
                Panel::Properties
            ]
        );
        assert_eq!(
            s.layout(VIEW)
                .groups
                .iter()
                .find(|g| g.id == 8)
                .unwrap()
                .active,
            Panel::Brushes
        );
    }
}

#[test]
fn nested_columns_reject_collapse() {
    let mut s = session(Platform::Gtk);
    let layout = &mut s.state.workspace.layout;
    layout
        .move_panel(
            VIEW,
            Panel::Properties,
            DockTarget::Split {
                group: 5,
                edge: Edge::Right,
            },
        )
        .unwrap();
    let nested = layout.panel_group(Panel::Properties).unwrap();
    let before = layout.clone();
    assert!(layout.set_column_collapsed(nested, true, VIEW).is_err());
    assert_eq!(*layout, before);
    let menu = layout
        .context_menu_on(
            ContextTarget::Panel {
                panel: Panel::Properties,
            },
            Platform::Gtk,
        )
        .unwrap();
    // Direct actions and resize paths enforce the same rule as menu eligibility.
    assert!(layout.collapsible_column_for_group(nested).is_none());
    assert!(
        !menu
            .sections
            .iter()
            .flatten()
            .any(|item| item.label == "Collapse column")
    );
    let r = s.layout(VIEW);
    for d in r
        .dividers
        .iter()
        .filter(|d| !d.band && d.axis == Axis::Horizontal)
    {
        assert!(
            s.state
                .workspace
                .layout
                .collapse_at_divider(d.id, [d.parent.x + 1., d.bounds.y], VIEW)
                .is_none()
        );
    }
    for action in [
        CustomizationAction::SetColumnDrawers { column: nested, drawers: true },
        CustomizationAction::SetColumnAutoHide { column: nested, auto_hide: true },
        CustomizationAction::ApplyColumnStack { column: nested },
    ] {
        assert!(s.dispatch(UiAction::Customize { action }).is_err());
        assert_eq!(s.state.workspace.layout, before);
    }
    let layout = &mut s.state.workspace.layout;
    layout.collapsed.push(crate::CollapsedColumn {
        root: nested,
        expanded_width: 200.,
    });
    layout
        .column_stacks
        .push(crate::ColumnStack::single(nested));
    assert!(layout.validate().is_err());
}

#[test]
fn new_stacks_default_to_full_columns() {
    assert!(!crate::ColumnStack::single(4).drawers);
}

#[test]
fn menubar_targets_follow_native_header_height_without_taking_the_tab_strip() {
    let mut s = session(Platform::Gtk);
    let item = DockItem::Panel {
        panel: Panel::Properties,
    };
    for height in [36., 48., 72.] {
        s.state.workspace.layout.header_presentation.height = height;
        let r = s.layout(VIEW);
        let b = r.groups.iter().find(|g| g.id == 5).unwrap().bounds;
        let x = b.x + b.width * 0.5;
        assert_eq!(
            s.drop_hint(VIEW, [x, height - 1.], &[], item, None)
                .unwrap()
                .target,
            DockTarget::Split {
                group: 5,
                edge: Edge::Top
            }
        );
        assert!(matches!(
            s.drop_hint(VIEW, [x, height + 1.], &[], item, None)
                .unwrap()
                .target,
            DockTarget::Tab { group: 5, .. }
        ));
    }
}

#[test]
fn prepend_keeps_preferences_after_a_column_has_been_expanded() {
    for item in [
        DockItem::Panel {
            panel: Panel::Brushes,
        },
        DockItem::Column { column: 4 },
    ] {
        let mut layout = DockLayout::default();
        layout.set_column_collapsed(8, true, VIEW).unwrap();
        layout.column_stack_mut(8).drawers = true;
        layout.set_column_collapsed(8, false, VIEW).unwrap();
        if matches!(item, DockItem::Column { .. }) {
            layout.set_column_collapsed(5, true, VIEW).unwrap();
        }
        layout
            .move_item(
                VIEW,
                item,
                DockTarget::Split {
                    group: 8,
                    edge: Edge::Top,
                },
            )
            .unwrap();
        layout.validate().unwrap();
        let root = layout.column_for_group(8).unwrap();
        assert_ne!(root, 8);
        assert!(layout.column_stack(root).drawers);
        assert_eq!(layout.column_stack(root).members, [root]);
        layout.set_column_collapsed(8, true, VIEW).unwrap();
        layout.validate().unwrap();
    }
}
