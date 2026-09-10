//! Content drawers: tool semantics and placement are shared; hosts only measure
//! panel bodies and animate the returned rectangles. No dock widgets are moved.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileAnchor {
    pub panel: Panel,
    pub tile: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawerDismissal {
    OutsideContact,
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContentDrawer {
    pub anchor: TileAnchor,
    /// Each column contains vertically stacked, undecorated panel bodies.
    pub columns: Vec<Vec<Panel>>,
    pub dismissal: DrawerDismissal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawerPlacement {
    pub bounds: Bounds,
    pub anchor: Bounds,
    pub direction: Edge,
    /// Column bounds are local to the drawer; scroll overflow within each one.
    pub columns: Vec<Bounds>,
}
impl DrawerPlacement {
    pub fn interpolate_from(&self, from: &Self, progress: f32) -> Self {
        Self {
            bounds: self.bounds.interpolate_from(from.bounds, progress),
            ..self.clone()
        }
    }
    pub fn closed(&self) -> Self {
        Self {
            bounds: self.anchor,
            ..self.clone()
        }
    }
}

impl Panel {
    /// Roomier than ordinary dock defaults. Shared by tile and column drawers.
    pub fn drawer_width(self) -> f32 {
        match self {
            Self::Color => 280.0,
            Self::ToolSettings | Self::Properties | Self::Layers => 320.0,
            _ => 272.0,
        }
    }
}

impl ResolvedLayout {
    pub(crate) fn tile_at(&self, layout: &DockLayout, point: [f32; 2]) -> Option<TileAnchor> {
        let group = self
            .groups
            .iter()
            .rev()
            .find(|g| g.bounds.contains(point[0], point[1]))?;
        let local = [
            point[0] - group.bounds.x,
            point[1]
                - group.bounds.y
                - if group.tabs_visible {
                    TAB_BAR_HEIGHT
                } else {
                    0.0
                },
        ];
        let config = layout.panel(group.active).ok()?;
        group
            .tiles
            .as_ref()?
            .tiles
            .iter()
            .zip(config.tiles())
            .find_map(|(bounds, tile)| {
                bounds.contains(local[0], local[1]).then_some(TileAnchor {
                    panel: group.active,
                    tile: tile.id,
                })
            })
    }
}

impl ToolbarControl {
    pub(crate) fn drawer_columns(self) -> Option<Vec<Vec<Panel>>> {
        match self {
            Self::Command { command }
                if command.paint_tool().is_some()
                    || matches!(command, CommandId::Lasso | CommandId::Move) =>
            {
                Some(vec![vec![Panel::Brushes], vec![Panel::ToolSettings]])
            }
            Self::Brush { .. } => Some(vec![vec![Panel::Brushes], vec![Panel::ToolSettings]]),
            Self::Color => Some(vec![vec![Panel::Color]]),
            Self::Opacity => Some(vec![vec![Panel::ToolSettings]]),
            Self::Panel { panel } => Some(vec![vec![panel]]),
            _ => None,
        }
    }
    pub(crate) fn selectable(self) -> bool {
        matches!(self, Self::Brush { .. } | Self::Command { .. })
    }
}

impl ContentDrawer {
    pub(crate) fn for_tile(layout: &DockLayout, anchor: TileAnchor) -> Result<Self, String> {
        let control = layout
            .panel(anchor.panel)?
            .tiles()
            .iter()
            .find(|t| t.id == anchor.tile)
            .ok_or("The tool no longer exists")?
            .control;
        Ok(Self {
            anchor,
            columns: control.drawer_columns().ok_or("This tile has no drawer")?,
            dismissal: DrawerDismissal::OutsideContact,
        })
    }

    pub fn column_widths(&self) -> Vec<f32> {
        self.columns
            .iter()
            .map(|panels| panels.iter().map(|p| p.drawer_width()).fold(0.0, f32::max))
            .collect()
    }

    pub fn placement(
        &self,
        layout: &DockLayout,
        viewport: [f32; 2],
        heights: &[f32],
    ) -> Option<DrawerPlacement> {
        if heights.len() != self.columns.len()
            || self.columns.is_empty()
            || !viewport
                .into_iter()
                .chain(heights.iter().copied())
                .all(f32::is_finite)
            || viewport[0] <= 2.0 * WORKSPACE_SPACING
            || viewport[1] <= HEADER_HEIGHT + WORKSPACE_SPACING
        {
            return None;
        }
        let resolved = layout.workspace(viewport[0], viewport[1], HEADER_HEIGHT, STATUS_HEIGHT);
        let group = resolved
            .groups
            .iter()
            .find(|g| g.active == self.anchor.panel)?;
        let index = layout
            .panel(self.anchor.panel)
            .ok()?
            .tiles()
            .iter()
            .position(|t| t.id == self.anchor.tile)?;
        let tile = *group.tiles.as_ref()?.tiles.get(index)?;
        let anchor = Bounds {
            x: group.bounds.x + tile.x,
            y: group.bounds.y
                + tile.y
                + if group.tabs_visible {
                    TAB_BAR_HEIGHT
                } else {
                    0.0
                },
            ..tile
        }
        .intersection(group.bounds)?;
        let available = Bounds {
            x: WORKSPACE_SPACING,
            y: HEADER_HEIGHT,
            width: viewport[0] - WORKSPACE_SPACING * 2.0,
            height: viewport[1] - HEADER_HEIGHT - WORKSPACE_SPACING,
        };
        let edge = layout
            .bands
            .iter()
            .find(|b| b.root.group_for(self.anchor.panel).is_some())
            .map(|b| b.edge);
        let direction = match edge {
            Some(Edge::Top) => Edge::Bottom,
            Some(Edge::Bottom) => Edge::Top,
            Some(Edge::Left) => Edge::Right,
            Some(Edge::Right) => Edge::Left,
            None if group.axis == Axis::Horizontal => {
                if anchor.y + anchor.height * 0.5 < available.y + available.height * 0.5 {
                    Edge::Bottom
                } else {
                    Edge::Top
                }
            }
            None => {
                if anchor.x + anchor.width * 0.5 < viewport[0] * 0.5 {
                    Edge::Right
                } else {
                    Edge::Left
                }
            }
        };
        let natural_widths = self.column_widths();
        let gaps = (natural_widths.len() - 1) as f32;
        let total = natural_widths.iter().sum::<f32>();
        let mut width = (total + gaps).min(available.width);
        // Preserve the originating tile where possible, rather than overlaying it.
        let room = match direction {
            Edge::Right => {
                available.x + available.width - anchor.x - anchor.width - WORKSPACE_SPACING
            }
            Edge::Left => anchor.x - WORKSPACE_SPACING - available.x,
            _ => available.width,
        };
        width = width.min(room.max(1.0));
        let height = heights.iter().copied().fold(TILE_SIZE, f32::max);
        let (x, y, height) = match direction {
            Edge::Bottom => {
                let y = (anchor.y + anchor.height + WORKSPACE_SPACING)
                    .min(available.y + available.height - 1.0);
                (anchor.x, y, height.min(available.y + available.height - y))
            }
            Edge::Top => {
                let bottom = (anchor.y - WORKSPACE_SPACING).max(available.y + 1.0);
                let height = 800.0_f32.min(available.height).min(bottom - available.y);
                (anchor.x, bottom - height, height)
            }
            Edge::Left | Edge::Right => {
                let y = (anchor.y - 500.0).max(available.y);
                let height = height
                    .max(anchor.y + anchor.height - y)
                    .min(available.y + available.height - y);
                (
                    if direction == Edge::Right {
                        anchor.x + anchor.width + WORKSPACE_SPACING
                    } else {
                        anchor.x - WORKSPACE_SPACING - width
                    },
                    y,
                    height,
                )
            }
        };
        let bounds = Bounds {
            x: x.clamp(available.x, available.x + available.width - width),
            y,
            width,
            height,
        };
        let scale = (width - gaps).max(0.0) / total;
        let mut x = 0.0;
        let columns = natural_widths
            .into_iter()
            .map(|width| {
                let bounds = Bounds {
                    x,
                    y: 0.0,
                    width: width * scale,
                    height,
                };
                x += bounds.width + 1.0;
                bounds
            })
            .collect();
        Some(DrawerPlacement {
            bounds,
            anchor,
            direction,
            columns,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const VIEWPORT: [f32; 2] = [1200.0, 900.0];
    fn drawer(layout: &DockLayout) -> ContentDrawer {
        let tile = layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
        ContentDrawer::for_tile(
            layout,
            TileAnchor {
                panel: Panel::Toolbar,
                tile,
            },
        )
        .unwrap()
    }
    fn contained(p: &DrawerPlacement, viewport: [f32; 2]) {
        assert!(
            p.bounds.x >= WORKSPACE_SPACING && p.bounds.y >= HEADER_HEIGHT,
            "{p:?}"
        );
        assert!(
            p.bounds.x + p.bounds.width <= viewport[0] - WORKSPACE_SPACING + 0.001,
            "{p:?}"
        );
        assert!(
            p.bounds.y + p.bounds.height <= viewport[1] - WORKSPACE_SPACING + 0.001,
            "{p:?}"
        );
    }
    #[test]
    fn tile_drawers_fit_all_edges_and_content_changes() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let mut layout = DockLayout::default();
            layout
                .move_panel(
                    VIEWPORT,
                    Panel::Toolbar,
                    DockTarget::Edge { edge, outer: false },
                )
                .unwrap();
            let d = drawer(&layout);
            assert_eq!(d.column_widths(), [272.0, 320.0]);
            for viewport in [VIEWPORT, [640.0, 480.0], [320.0, 240.0]] {
                for heights in [[80.0, 120.0], [700.0, 3000.0]] {
                    let Some(p) = d.placement(&layout, viewport, &heights) else {
                        let resolved = layout.workspace(
                            viewport[0],
                            viewport[1],
                            HEADER_HEIGHT,
                            STATUS_HEIGHT,
                        );
                        let source = resolved
                            .groups
                            .iter()
                            .find(|g| g.active == Panel::Toolbar)
                            .unwrap();
                        assert!(source.bounds.width == 0.0 || source.bounds.height == 0.0);
                        continue; // A completely clipped origin cannot open a drawer.
                    };
                    contained(&p, viewport);
                    assert_eq!(p.columns.len(), 2);
                    assert!((p.columns[1].x - p.columns[0].width - 1.0).abs() < 0.001);
                    if viewport == VIEWPORT {
                        match edge {
                            Edge::Top => assert_eq!(p.direction, Edge::Bottom),
                            Edge::Bottom => assert_eq!(p.direction, Edge::Top),
                            Edge::Left | Edge::Right => {
                                assert_eq!(p.bounds.y, (p.anchor.y - 500.0).max(HEADER_HEIGHT));
                                assert!(
                                    p.bounds.y + p.bounds.height >= p.anchor.y + p.anchor.height
                                );
                            }
                        }
                    }
                    assert_eq!(p.interpolate_from(&p.closed(), 0.0).bounds, p.anchor);
                    assert_eq!(p.interpolate_from(&p.closed(), 1.0).bounds, p.bounds);
                }
            }
            assert!(d.placement(&layout, VIEWPORT, &[f32::NAN, 10.0]).is_none());
            assert!(d.placement(&layout, VIEWPORT, &[]).is_none());
        }
    }
    #[test]
    fn floating_drawers_choose_available_side_and_stacked_columns_share_widths() {
        for position in [[20.0, 100.0], [960.0, 720.0]] {
            let mut layout = DockLayout::default();
            layout
                .move_panel(VIEWPORT, Panel::Toolbar, DockTarget::Float { position })
                .unwrap();
            let mut d = drawer(&layout);
            d.columns = vec![vec![Panel::Sizes, Panel::Layers], vec![Panel::Color]];
            assert_eq!(d.column_widths(), [320.0, 280.0]);
            let p = d.placement(&layout, VIEWPORT, &[500.0, 200.0]).unwrap();
            contained(&p, VIEWPORT);
            if matches!(p.direction, Edge::Left | Edge::Top) {
                assert_eq!(position[0], 960.0);
            } else {
                assert_eq!(position[0], 20.0);
            }
        }
    }
}
