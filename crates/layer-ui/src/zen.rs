//! Transient Zen projections. Toolbar identities and saved dock geometry stay
//! untouched; the same rectangles drive native presentation and drawer anchors.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZenSection {
    pub panel: Panel,
    pub edge: Edge,
    pub style: TileStyle,
    pub bounds: Bounds,
    /// Stable tile IDs and content-local bounds. Overflow clips, never scales
    /// the configured tile size or adds a second row/column.
    pub tiles: Vec<(u32, Bounds)>,
}
impl ZenSection {
    pub fn tile_bounds(&self, id: u32) -> Option<Bounds> {
        let mut b = self.tiles.iter().find(|(tile, _)| *tile == id)?.1;
        b.x += self.bounds.x;
        b.y += self.bounds.y;
        b.intersection(self.bounds)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ZenToolbars {
    pub sections: Vec<ZenSection>,
}
impl ZenToolbars {
    pub fn tile_at(&self, point: [f32; 2]) -> Option<TileAnchor> {
        self.sections.iter().find_map(|s| {
            s.tiles.iter().find_map(|(tile, _)| {
                s.tile_bounds(*tile)?
                    .contains(point[0], point[1])
                    .then_some(TileAnchor {
                        panel: s.panel,
                        tile: *tile,
                    })
            })
        })
    }
    pub fn anchor(&self, anchor: TileAnchor) -> Option<(Bounds, Edge)> {
        self.sections.iter().find_map(|s| {
            (s.panel == anchor.panel)
                .then(|| Some((s.tile_bounds(anchor.tile)?, s.edge)))
                .flatten()
        })
    }
}

impl UiState {
    pub fn partial_zen(&self) -> bool {
        self.workspace.zen_mode && !self.settings.total_zen
    }
}

impl DockLayout {
    pub fn zen_toolbars(&self, viewport: [f32; 2]) -> ZenToolbars {
        let mut result = ZenToolbars::default();
        if !viewport
            .into_iter()
            .all(|v| v.is_finite() && v > 2.0 * WORKSPACE_SPACING)
        {
            return result;
        }
        let normal = self.workspace(viewport[0], viewport[1], HEADER_HEIGHT, STATUS_HEIGHT);
        // Only outward-facing lone toolbars in edge bands. Multiple toolbar
        // bands on an edge share its space. A toolbar nested behind a content
        // column or inside a tab group isn't a directly docked toolbar.
        for band in &self.bands {
            let groups: Vec<_> = normal
                .groups
                .iter()
                .filter(|g| band.root.group_for(g.active).is_some())
                .collect();
            let coordinate = |b: Bounds| match band.edge {
                Edge::Left => b.x,
                Edge::Right => -b.x - b.width,
                Edge::Top => b.y,
                Edge::Bottom => -b.y - b.height,
            };
            let outside = groups
                .iter()
                .map(|g| coordinate(g.bounds))
                .fold(f32::INFINITY, f32::min);
            let mut eligible: Vec<_> = groups
                .into_iter()
                .filter(|g| {
                    g.panels.len() == 1
                        && g.active.kind() == PanelKind::Tiles
                        && (coordinate(g.bounds) - outside).abs() < 0.5
                })
                .collect();
            eligible.sort_by(|a, b| {
                let start = |g: &GroupPlacement| {
                    if matches!(band.edge, Edge::Left | Edge::Right) {
                        g.bounds.y
                    } else {
                        g.bounds.x
                    }
                };
                start(a).total_cmp(&start(b))
            });
            for group in eligible {
                let config = self.panel(group.active).expect("validated toolbar");
                let vertical = matches!(band.edge, Edge::Left | Edge::Right);
                let [width, height] = config.tile_style.size();
                for tiles in config
                    .tiles()
                    .split(|t| t.control == ToolbarControl::Divider)
                {
                    if tiles.is_empty() {
                        continue;
                    }
                    result.sections.push(ZenSection {
                        panel: group.active,
                        edge: band.edge,
                        style: config.tile_style,
                        bounds: Bounds {
                            width: if vertical {
                                width
                            } else {
                                tiles.len() as f32 * (width + 2.0) - 2.0
                            },
                            height: if vertical {
                                tiles.len() as f32 * (height + 2.0) - 2.0
                            } else {
                                height
                            },
                            ..Bounds::default()
                        },
                        tiles: tiles
                            .iter()
                            .enumerate()
                            .map(|(i, tile)| {
                                (
                                    tile.id,
                                    Bounds {
                                        x: if vertical {
                                            0.0
                                        } else {
                                            i as f32 * (width + 2.0)
                                        },
                                        y: if vertical {
                                            i as f32 * (height + 2.0)
                                        } else {
                                            0.0
                                        },
                                        width,
                                        height,
                                    },
                                )
                            })
                            .collect(),
                    });
                }
            }
        }
        let gap = WORKSPACE_SPACING;
        let thickness = |edge| {
            result
                .sections
                .iter()
                .filter(|s| s.edge == edge)
                .map(|s| s.bounds.height)
                .fold(0.0_f32, f32::max)
        };
        let top = thickness(Edge::Top);
        let bottom = thickness(Edge::Bottom);
        let zen_end = TILE_SIZE + gap * 2.0;
        // Horizontal bars own the remaining corners; side bars occupy the
        // interval between them. The Zen button always owns the top-left corner.
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let vertical = matches!(edge, Edge::Left | Edge::Right);
            let (start, end) = match edge {
                Edge::Top => (
                    zen_end + self.titlebar_insets[0],
                    viewport[0] - gap - self.titlebar_insets[1],
                ),
                Edge::Bottom => (gap, viewport[0] - gap),
                Edge::Left | Edge::Right => (
                    (if top > 0.0 { top + gap * 2.0 } else { gap })
                        .max(if edge == Edge::Left { zen_end } else { gap })
                        .max(
                            if self.titlebar_insets[usize::from(edge == Edge::Right)] > 0.0 {
                                self.titlebar_insets[2] + gap
                            } else {
                                gap
                            },
                        ),
                    viewport[1] - gap - if bottom > 0.0 { bottom + gap } else { 0.0 },
                ),
            };
            let mut sections: Vec<_> = result
                .sections
                .iter_mut()
                .filter(|s| s.edge == edge)
                .collect();
            let count = sections.len();
            if count == 0 {
                continue;
            }
            let length = |s: &ZenSection| {
                if vertical {
                    s.bounds.height
                } else {
                    s.bounds.width
                }
            };
            let total = sections.iter().map(|s| length(s)).sum::<f32>();
            let available = (end - start).max(0.0);
            let spacing = if count > 1 {
                let free_gap = ((available - total) / (count - 1) as f32)
                    .max(0.0)
                    .max(gap.min(available / count as f32));
                if edge != Edge::Bottom {
                    // A tile-sized gap within side/top clusters. Only the
                    // bottom bar distributes every section across the edge.
                    free_gap.min(
                        sections
                            .iter()
                            .map(|s| s.style.size()[usize::from(vertical)])
                            .fold(TILE_SIZE, f32::max),
                    )
                } else {
                    free_gap
                }
            } else {
                0.0
            };
            let scale =
                ((available - spacing * count.saturating_sub(1) as f32).max(0.0) / total).min(1.0);
            let occupied = total * scale + spacing * count.saturating_sub(1) as f32;
            // The first section crossing the tile-count midpoint belongs to
            // the right cluster. Never split a section or change tile order.
            let split = if edge == Edge::Top {
                let midpoint = sections.iter().map(|s| s.tiles.len()).sum::<usize>() / 2;
                let mut seen = 0;
                sections
                    .iter()
                    .position(|s| {
                        seen += s.tiles.len();
                        seen > midpoint
                    })
                    .unwrap_or(count)
            } else {
                count
            };
            let right_start = end
                - sections
                    .iter()
                    .skip(split)
                    .map(|s| length(s) * scale)
                    .sum::<f32>()
                - spacing * count.saturating_sub(split + 1) as f32;
            let mut along = start
                + if edge == Edge::Top {
                    0.0
                } else {
                    (available - occupied).max(0.0) * 0.5
                };
            for (index, s) in sections.iter_mut().enumerate() {
                if index == split {
                    along = right_start;
                }
                let extent = length(s) * scale;
                s.bounds.x = match edge {
                    Edge::Left => gap,
                    Edge::Right => (viewport[0] - gap - s.bounds.width).max(gap),
                    _ => along,
                };
                s.bounds.y = match edge {
                    Edge::Top => gap,
                    Edge::Bottom => (viewport[1] - gap - s.bounds.height).max(gap),
                    _ => along,
                };
                if vertical {
                    s.bounds.height = extent;
                } else {
                    s.bounds.width = extent;
                }
                along += extent + spacing;
            }
        }
        result
            .sections
            .retain(|s| s.bounds.width > 0.0 && s.bounds.height > 0.0);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VIEW: [f32; 2] = [1200.0, 900.0];
    const COLOR: ToolbarControl = ToolbarControl::Color;
    const DIV: ToolbarControl = ToolbarControl::Divider;

    fn empty() -> DockLayout {
        let mut layout = DockLayout::default();
        layout.bands.clear();
        layout
    }
    fn add(layout: &mut DockLayout, edge: Edge, controls: &[ToolbarControl]) -> Panel {
        let panel = layout
            .add_toolbar(None, &format!("Bar {}", layout.panels.len()), controls)
            .unwrap();
        layout
            .move_panel(VIEW, panel, DockTarget::Edge { edge, outer: true })
            .unwrap();
        panel
    }
    fn overlaps(a: Bounds, b: Bounds) -> bool {
        a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
    }

    #[test]
    fn caption_insets_keep_zen_controls_and_hit_regions_outside_native_buttons() {
        let mut layout = DockLayout::editor_default();
        let saved = serde_json::to_string(&layout).unwrap();
        for viewport in [[1600., 1000.], [900., 640.], [400., 300.]] {
            let original = layout.zen_toolbars(viewport);
            layout.titlebar_insets = [72., 138., 48.];
            let projected = layout.zen_toolbars(viewport);
            let captions = [
                Bounds {
                    x: 0.,
                    y: 0.,
                    width: 72.,
                    height: 48.,
                },
                Bounds {
                    x: viewport[0] - 138.,
                    y: 0.,
                    width: 138.,
                    height: 48.,
                },
            ];
            for section in &projected.sections {
                assert!(
                    captions
                        .iter()
                        .all(|caption| !overlaps(section.bounds, *caption))
                );
                for &(id, _) in &section.tiles {
                    let Some(bounds) = section.tile_bounds(id) else {
                        continue;
                    };
                    let anchor = TileAnchor {
                        panel: section.panel,
                        tile: id,
                    };
                    assert_eq!(projected.anchor(anchor).unwrap().0, bounds);
                    assert_eq!(
                        projected
                            .tile_at([bounds.x + bounds.width / 2., bounds.y + bounds.height / 2.]),
                        Some(anchor)
                    );
                }
            }
            assert_eq!(
                original
                    .sections
                    .iter()
                    .map(|s| (s.panel, s.tiles.iter().map(|t| t.0).collect::<Vec<_>>()))
                    .collect::<Vec<_>>(),
                projected
                    .sections
                    .iter()
                    .map(|s| (s.panel, s.tiles.iter().map(|t| t.0).collect::<Vec<_>>()))
                    .collect::<Vec<_>>()
            );
            assert_eq!(serde_json::to_string(&layout).unwrap(), saved);
            layout.titlebar_insets = [0.; 3];
        }
        let mut right = empty();
        add(&mut right, Edge::Right, &[ToolbarControl::Color]);
        right.titlebar_insets = [0., 138., 48.];
        assert!(right.zen_toolbars(VIEW).sections[0].bounds.y >= 48.);
    }
    #[test]
    fn sections_keep_ids_and_size_and_align_or_center_on_each_edge() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
                for controls in [
                    vec![DIV, COLOR, COLOR, DIV],
                    vec![DIV, COLOR, DIV, DIV, COLOR, COLOR, DIV, COLOR, DIV],
                ] {
                    let mut layout = empty();
                    let panel = add(&mut layout, edge, &controls);
                    layout.panel_mut(panel).unwrap().tile_style = style;
                    let saved = serde_json::to_string(&layout).unwrap();
                    let model = layout.zen_toolbars(VIEW);
                    assert_eq!(
                        model.sections.len(),
                        if controls.len() == 4 { 1 } else { 3 }
                    );
                    let ids: Vec<_> = model
                        .sections
                        .iter()
                        .flat_map(|s| s.tiles.iter().map(|(id, _)| *id))
                        .collect();
                    assert_eq!(
                        ids,
                        layout
                            .panel(panel)
                            .unwrap()
                            .tiles()
                            .iter()
                            .filter(|t| t.control != DIV)
                            .map(|t| t.id)
                            .collect::<Vec<_>>()
                    );
                    let vertical = matches!(edge, Edge::Left | Edge::Right);
                    let along = |b: Bounds| {
                        if vertical {
                            (b.y, b.height)
                        } else {
                            (b.x, b.width)
                        }
                    };
                    let start = if matches!(edge, Edge::Top | Edge::Left) {
                        48.0
                    } else {
                        6.0
                    };
                    let end = VIEW[usize::from(vertical)] - 6.0;
                    let first = along(model.sections.first().unwrap().bounds);
                    let last = along(model.sections.last().unwrap().bounds);
                    if edge == Edge::Top {
                        assert!((last.0 + last.1 - end).abs() < 0.01);
                    } else if model.sections.len() == 1 || vertical {
                        assert!(
                            ((first.0 + last.0 + last.1) * 0.5 - (start + end) * 0.5).abs() < 0.01
                        );
                    } else {
                        assert!((first.0 - start).abs() < 0.01);
                        assert!((last.0 + last.1 - end).abs() < 0.01);
                    }
                    if model.sections.len() > 1 {
                        let gaps: Vec<_> = model
                            .sections
                            .windows(2)
                            .map(|s| {
                                let (a, n) = along(s[0].bounds);
                                along(s[1].bounds).0 - a - n
                            })
                            .collect();
                        if edge != Edge::Top {
                            assert!((gaps[0] - gaps[1]).abs() < 0.01);
                        }
                        if vertical {
                            assert!((gaps[0] - style.size()[1]).abs() < 0.01);
                        }
                    }
                    for section in &model.sections {
                        for (tile, b) in &section.tiles {
                            assert_eq!([b.width, b.height], style.size());
                            let anchor = section.tile_bounds(*tile).unwrap();
                            assert_eq!(
                                model.tile_at([anchor.x + 1.0, anchor.y + 1.0]),
                                Some(TileAnchor { panel, tile: *tile })
                            );
                            let drawer =
                                ContentDrawer::for_tile(&layout, TileAnchor { panel, tile: *tile })
                                    .unwrap();
                            let placement =
                                drawer.placement(&layout, VIEW, &[300.0], true).unwrap();
                            assert_eq!(placement.anchor, anchor);
                        }
                    }
                    assert_eq!(serde_json::to_string(&layout).unwrap(), saved);
                }
            }
        }
    }

    #[test]
    fn top_clusters_split_at_sections_before_the_midpoint() {
        for (counts, split) in [
            (vec![2, 2], 1),
            (vec![1, 3, 2], 1),
            (vec![3, 1, 1], 0),
            (vec![1, 1, 1, 1, 1], 2),
            (vec![3], 0),
        ] {
            let mut layout = empty();
            let mut controls = Vec::new();
            for count in &counts {
                controls.extend(std::iter::repeat_n(COLOR, *count));
                controls.push(DIV);
            }
            add(&mut layout, Edge::Top, &controls);
            let sections = layout.zen_toolbars(VIEW).sections;
            assert_eq!(
                sections.iter().map(|s| s.tiles.len()).collect::<Vec<_>>(),
                counts
            );
            let last = sections.last().unwrap().bounds;
            assert_eq!(last.x + last.width, VIEW[0] - WORKSPACE_SPACING);
            if split > 0 {
                assert_eq!(sections[0].bounds.x, 48.0);
            }
            for (i, pair) in sections.windows(2).enumerate() {
                let gap = pair[1].bounds.x - pair[0].bounds.x - pair[0].bounds.width;
                if i + 1 == split {
                    assert!(gap > TILE_SIZE);
                } else {
                    assert_eq!(gap, TILE_SIZE);
                }
            }
        }
    }

    #[test]
    fn all_edges_reserve_corners_and_overflow_clips_without_shrinking_tiles() {
        for viewport in [[320.0, 240.0], [640.0, 480.0], VIEW, [1920.0, 1080.0]] {
            let mut layout = empty();
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let p = add(
                    &mut layout,
                    edge,
                    &[COLOR, COLOR, DIV, COLOR, COLOR, DIV, COLOR],
                );
                layout.panel_mut(p).unwrap().tile_style = TileStyle::Large;
            }
            let model = layout.zen_toolbars(viewport);
            let zen = Bounds {
                x: 6.0,
                y: 6.0,
                width: TILE_SIZE,
                height: TILE_SIZE,
            };
            assert_eq!(model.sections.len(), 12);
            for (i, section) in model.sections.iter().enumerate() {
                let b = section.bounds;
                assert!(!overlaps(b, zen));
                assert!(
                    b.x >= 0.0
                        && b.y >= 0.0
                        && b.x + b.width <= viewport[0] + 0.01
                        && b.y + b.height <= viewport[1] + 0.01
                );
                for other in model.sections.iter().skip(i + 1) {
                    assert!(!overlaps(b, other.bounds), "{b:?} vs {:?}", other.bounds);
                }
                for (_, b) in &section.tiles {
                    assert_eq!([b.width, b.height], TileStyle::Large.size());
                }
            }
            assert!(
                model
                    .tile_at([viewport[0] * 0.5, viewport[1] * 0.5])
                    .is_none()
            );
        }
    }

    #[test]
    fn empty_dividers_and_multiple_toolbars_and_non_edge_panels() {
        let mut layout = empty();
        add(&mut layout, Edge::Top, &[DIV, DIV]);
        assert!(layout.zen_toolbars(VIEW).sections.is_empty());
        let a = add(&mut layout, Edge::Top, &[COLOR]);
        let b = add(&mut layout, Edge::Top, &[COLOR, DIV, COLOR]);
        assert_eq!(layout.zen_toolbars(VIEW).sections.len(), 3);
        layout
            .move_panel(
                VIEW,
                a,
                DockTarget::Float {
                    position: [600.0, 400.0],
                },
            )
            .unwrap();
        assert_eq!(layout.zen_toolbars(VIEW).sections.len(), 2);
        layout
            .move_panel(
                VIEW,
                b,
                DockTarget::Tab {
                    group: layout.panel_group(a).unwrap(),
                    index: None,
                },
            )
            .unwrap();
        assert!(layout.zen_toolbars(VIEW).sections.is_empty());
        layout
            .move_item(
                VIEW,
                DockItem::Group {
                    group: layout.panel_group(a).unwrap(),
                },
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: true,
                },
            )
            .unwrap();
        assert!(
            layout.zen_toolbars(VIEW).sections.is_empty(),
            "tabbed toolbars aren't retained"
        );
        assert!(layout.zen_toolbars([f32::NAN, 900.0]).sections.is_empty());
    }
}
