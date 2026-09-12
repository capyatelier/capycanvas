use super::*;

const VIEW: [f32; 2] = [1200., 900.];
const TOOL: ToolbarControl = ToolbarControl::Color;
const DIVIDER: ToolbarControl = ToolbarControl::Divider;

#[test]
fn toolbar_group_moves_preserve_ids_and_only_add_needed_separators() {
    for cross_toolbar in [false, true] {
        for destination_controls in [
            &[TOOL, TOOL, DIVIDER, TOOL][..],
            &[TOOL, TOOL, DIVIDER, DIVIDER, TOOL],
            &[TOOL, TOOL, DIVIDER],
        ] {
            let mut layout = DockLayout::default();
            let destination = layout
                .add_toolbar(None, "Target", destination_controls)
                .unwrap();
            let source = if cross_toolbar {
                layout.add_toolbar(None, "Source", &[TOOL]).unwrap()
            } else {
                destination
            };
            let tile = layout.panel(source).unwrap().tiles()[0].clone();
            let divider = layout.panel(destination).unwrap().tiles()[2].id;
            let next_id = layout.next_tile_id;
            let need_separator = destination_controls.get(3) == Some(&TOOL);
            layout
                .move_item(
                    VIEW,
                    DockItem::Tile {
                        panel: source,
                        tile: tile.id,
                    },
                    DockTarget::TileGroup {
                        panel: destination,
                        divider,
                    },
                )
                .unwrap();
            layout.validate().unwrap();
            let tiles = layout.panel(destination).unwrap().tiles();
            let index = tiles.iter().position(|t| t.id == tile.id).unwrap();
            assert_eq!(tiles[index], tile);
            assert_eq!(tiles[index - 1].id, divider);
            assert!(tiles.get(index + 1).is_none_or(|t| t.control == DIVIDER));
            assert_eq!(layout.next_tile_id, next_id + u32::from(need_separator));
            if cross_toolbar {
                assert!(layout.panel(source).unwrap().tiles().is_empty());
            }
            let before = layout.clone();
            layout
                .move_item(
                    VIEW,
                    DockItem::Tile {
                        panel: destination,
                        tile: tile.id,
                    },
                    DockTarget::TileGroup {
                        panel: destination,
                        divider,
                    },
                )
                .unwrap();
            assert_eq!(
                layout, before,
                "an already separate neighboring tool is a no-op"
            );
        }
    }
}

#[test]
fn toolbar_group_moves_fail_atomically_and_do_not_isolate_separators() {
    let mut layout = DockLayout::default();
    let panel = layout
        .add_toolbar(None, "Target", &[TOOL, TOOL, DIVIDER, TOOL])
        .unwrap();
    let tiles = layout.panel(panel).unwrap().tiles();
    let tile = tiles[0].id;
    let divider = tiles[2].id;
    for (source, target) in [(tile, tile), (divider, divider), (tile, u32::MAX)] {
        let before = layout.clone();
        assert!(
            layout
                .move_item(
                    VIEW,
                    DockItem::Tile {
                        panel,
                        tile: source
                    },
                    DockTarget::TileGroup {
                        panel,
                        divider: target
                    }
                )
                .is_err()
        );
        assert_eq!(layout, before);
    }
    layout.next_tile_id = u32::MAX;
    let before = layout.clone();
    assert!(
        layout
            .move_item(
                VIEW,
                DockItem::Tile { panel, tile },
                DockTarget::TileGroup { panel, divider }
            )
            .is_err()
    );
    assert_eq!(
        layout, before,
        "ID exhaustion cannot remove the source tool"
    );
}

#[test]
fn toolbar_divider_targets_scale_follow_flow_and_clip() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for style in [
            TileStyle::Small,
            TileStyle::Medium,
            TileStyle::Large,
            TileStyle::MediumLabeled,
            TileStyle::Labeled,
        ] {
            for wrapped in [false, true] {
                let mut config = DockLayout::default();
                let panel = config
                    .add_toolbar(
                        None,
                        "Target",
                        &[TOOL, TOOL, DIVIDER, TOOL, TOOL, DIVIDER, TOOL],
                    )
                    .unwrap();
                config.panel_mut(panel).unwrap().tile_style = style;
                config
                    .move_panel(
                        VIEW,
                        panel,
                        DockTarget::Float {
                            position: [300., 200.],
                        },
                    )
                    .unwrap();
                let mut resolved =
                    config.workspace(VIEW[0], VIEW[1], crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
                let g = resolved
                    .groups
                    .iter_mut()
                    .find(|g| g.active == panel)
                    .unwrap();
                let [w, h] = style.size();
                let (along, across) = if axis == Axis::Horizontal {
                    (w, h)
                } else {
                    (h, w)
                };
                let length = if wrapped { along * 3. + 30. } else { 1200. };
                let cross = if wrapped { across * 3. + 4. } else { across };
                g.bounds.width = if axis == Axis::Horizontal {
                    length
                } else {
                    cross
                };
                g.bounds.height = if axis == Axis::Horizontal {
                    cross
                } else {
                    length
                };
                g.axis = axis;
                g.tabs_visible = false;
                g.tiles = Some(toolbar_tile_layout(
                    g.bounds.width,
                    g.bounds.height,
                    axis,
                    config.panel(panel).unwrap().tiles(),
                    true,
                    style,
                ));
                let group = g.clone();
                for (tile, b) in config
                    .panel(panel)
                    .unwrap()
                    .tiles()
                    .iter()
                    .zip(&group.tiles.as_ref().unwrap().tiles)
                {
                    if tile.control != DIVIDER {
                        continue;
                    }
                    let center = [
                        group.bounds.x + b.x + b.width / 2.,
                        group.bounds.y + b.y + b.height / 2.,
                    ];
                    let along_index = if axis == Axis::Horizontal { 0 } else { 1 };
                    for offset in [-along / 6. + 0.5, 0., along / 6. - 0.5] {
                        let mut point = center;
                        point[along_index] += offset;
                        let hint = resolved.tile_group_drop_hint(point, &config).unwrap();
                        assert_eq!(
                            hint.target,
                            DockTarget::TileGroup {
                                panel,
                                divider: tile.id
                            }
                        );
                        assert_eq!(
                            [
                                hint.bounds.x + hint.bounds.width / 2.,
                                hint.bounds.y + hint.bounds.height / 2.
                            ],
                            center
                        );
                    }
                    for offset in [-along / 6. - 0.5, along / 6. + 0.5] {
                        let mut point = center;
                        point[along_index] += offset;
                        assert!(resolved.tile_group_drop_hint(point, &config).is_none());
                    }
                }
                let grip = group.tiles.as_ref().unwrap().grip.unwrap();
                assert!(
                    resolved
                        .tile_group_drop_hint(
                            [
                                group.bounds.x + grip.x + grip.width / 2.,
                                group.bounds.y + grip.y + grip.height / 2.
                            ],
                            &config
                        )
                        .is_none()
                );
                // A divider outside the body must not acquire a target at its edge.
                let g = resolved
                    .groups
                    .iter_mut()
                    .find(|g| g.active == panel)
                    .unwrap();
                for b in &mut g.tiles.as_mut().unwrap().tiles {
                    b.x += g.bounds.width;
                }
                assert!(
                    resolved
                        .tile_group_drop_hint(
                            [
                                group.bounds.x + group.bounds.width - 1.,
                                group.bounds.y + 10.
                            ],
                            &config
                        )
                        .is_none()
                );
            }
        }
    }
}
