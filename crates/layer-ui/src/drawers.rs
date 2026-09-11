//! Content drawers: tool semantics and placement are shared; hosts only measure
//! panel bodies and animate the returned rectangles. No dock widgets are moved.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileAnchor {
    pub panel: Panel,
    pub tile: u32,
}

/// Visible, clipped tile bounds measured by a host inside a column drawer.
/// Transient geometry only; the core validates ownership and chooses placement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawerTileMeasurement {
    pub column: u32,
    pub anchor: TileAnchor,
    pub bounds: Bounds,
}

/// Presented column drawer bounds, including its header and clipped body.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnDrawerMeasurement {
    pub group: u32,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrawerAnchor {
    Tile {
        panel: Panel,
        tile: u32,
    },
    Column {
        column: u32,
        group: u32,
        origin: Panel,
    },
}
impl DrawerAnchor {
    pub fn tile(self) -> Option<TileAnchor> {
        match self {
            Self::Tile { panel, tile } => Some(TileAnchor { panel, tile }),
            _ => None,
        }
    }
}
impl From<TileAnchor> for DrawerAnchor {
    fn from(a: TileAnchor) -> Self {
        Self::Tile {
            panel: a.panel,
            tile: a.tile,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DrawerTabs {
    pub group: u32,
    pub panels: Vec<Panel>,
    pub active: Panel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DrawerDismissal {
    OutsideContact,
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ContentDrawer {
    pub anchor: DrawerAnchor,
    /// Each column contains vertically stacked, undecorated panel bodies.
    pub columns: Vec<Vec<Panel>>,
    pub dismissal: DrawerDismissal,
    pub tabs: Option<DrawerTabs>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawerPlacement {
    pub bounds: Bounds,
    pub anchor: Bounds,
    pub direction: Edge,
    /// Column bounds are local to the drawer; scroll overflow within each one.
    pub columns: Vec<Bounds>,
}

/// A tab-like bridge between the tile and drawer. The affine transform maps
/// normalized (along-tile, toward-drawer) coordinates into connection-local
/// coordinates; hosts can draw the same rectangle and two concave quarter arcs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawerConnection {
    pub bounds: Bounds,
    pub transform: [f32; 6],
    pub length: f32,
    pub depth: f32,
    pub radii: [f32; 2],
    /// NW, NE, SE, SW: the body corner is joined, not exposed.
    pub square_corners: [bool; 4],
}
impl DrawerPlacement {
    /// Ancestor clipping must not round off a tile's connected corners.
    /// Only flatten container corners actually reached by the originating tile.
    pub fn source_corners(&self, container: Bounds) -> [bool; 4] {
        let a = self.anchor;
        let b = container;
        let facing = match self.direction {
            Edge::Top => [true, true, false, false],
            Edge::Right => [false, true, true, false],
            Edge::Bottom => [false, false, true, true],
            Edge::Left => [true, false, false, true],
        };
        let corners = [
            [b.x, b.y],
            [b.x + b.width, b.y],
            [b.x + b.width, b.y + b.height],
            [b.x, b.y + b.height],
        ];
        std::array::from_fn(|i| {
            facing[i]
                && corners[i][0] >= a.x - 0.5
                && corners[i][0] <= a.x + a.width + 0.5
                && corners[i][1] >= a.y - 0.5
                && corners[i][1] <= a.y + a.height + 0.5
        })
    }
    pub fn connection(&self) -> Option<DrawerConnection> {
        let a = self.anchor;
        let b = self.bounds;
        let vertical = matches!(self.direction, Edge::Top | Edge::Bottom);
        let (start, end, body_start, body_end) = if vertical {
            (
                a.x.max(b.x),
                (a.x + a.width).min(b.x + b.width),
                b.x,
                b.x + b.width,
            )
        } else {
            (
                a.y.max(b.y),
                (a.y + a.height).min(b.y + b.height),
                b.y,
                b.y + b.height,
            )
        };
        let (near, far, corners) = match self.direction {
            Edge::Bottom => (a.y + a.height, b.y, [0, 1]),
            Edge::Top => (b.y + b.height, a.y, [3, 2]),
            Edge::Right => (a.x + a.width, b.x, [0, 3]),
            Edge::Left => (b.x + b.width, a.x, [1, 2]),
        };
        let depth = far - near;
        let length = end - start;
        if depth <= 0.0 || depth > WORKSPACE_SPACING + 0.5 || length <= 0.0 {
            return None;
        }
        let distances = [start - body_start, body_end - end];
        let radii = distances.map(|d| (d - 8.0).clamp(0.0, WORKSPACE_SPACING.min(depth)));
        let mut square_corners = [false; 4];
        for (corner, distance) in corners.into_iter().zip(distances) {
            square_corners[corner] = distance < 8.0;
        }
        Some(DrawerConnection {
            bounds: if vertical {
                Bounds {
                    x: start - radii[0],
                    y: near,
                    width: length + radii[0] + radii[1],
                    height: depth,
                }
            } else {
                Bounds {
                    x: near,
                    y: start - radii[0],
                    width: depth,
                    height: length + radii[0] + radii[1],
                }
            },
            transform: match self.direction {
                Edge::Bottom => [1.0, 0.0, 0.0, 1.0, radii[0], 0.0],
                Edge::Top => [1.0, 0.0, 0.0, -1.0, radii[0], depth],
                Edge::Right => [0.0, 1.0, 1.0, 0.0, 0.0, radii[0]],
                Edge::Left => [0.0, 1.0, -1.0, 0.0, depth, radii[0]],
            },
            length,
            depth,
            radii,
            square_corners,
        })
    }
    pub fn interpolate_from(&self, from: &Self, progress: f32) -> Self {
        Self {
            bounds: self.bounds.interpolate_from(from.bounds, progress),
            ..self.clone()
        }
    }
    pub fn closed(&self) -> Self {
        let a = self.anchor;
        let bounds = match self.direction {
            Edge::Bottom => Bounds {
                y: a.y + a.height + WORKSPACE_SPACING,
                height: 0.0,
                ..a
            },
            Edge::Top => Bounds {
                y: a.y - WORKSPACE_SPACING,
                height: 0.0,
                ..a
            },
            Edge::Right => Bounds {
                x: a.x + a.width + WORKSPACE_SPACING,
                width: 0.0,
                ..a
            },
            Edge::Left => Bounds {
                x: a.x - WORKSPACE_SPACING,
                width: 0.0,
                ..a
            },
        };
        Self {
            bounds,
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
                    || matches!(
                        command,
                        CommandId::Lasso
                            | CommandId::Move
                            | CommandId::Hand
                            | CommandId::Eyedropper
                            | CommandId::Gradient
                            | CommandId::Figure
                            | CommandId::Ruler
                            | CommandId::AutoSelect
                            | CommandId::Fill
                    ) =>
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
            anchor: anchor.into(),
            columns: control.drawer_columns().ok_or("This tile has no drawer")?,
            dismissal: DrawerDismissal::OutsideContact,
            tabs: None,
        })
    }

    pub(crate) fn for_column(
        layout: &DockLayout,
        group: u32,
        origin: Panel,
    ) -> Result<Self, String> {
        let column = layout
            .collapsed_column_for_group(group)
            .ok_or("The column is not collapsed")?;
        let panels = layout.group_panels(group)?;
        if !panels.contains(&origin) {
            return Err("The drawer's opening tab no longer exists".into());
        }
        let active = layout.active_panel(origin).unwrap();
        Ok(Self {
            anchor: DrawerAnchor::Column {
                column,
                group,
                origin,
            },
            columns: vec![vec![active]],
            tabs: Some(DrawerTabs {
                group,
                panels: panels.to_vec(),
                active,
            }),
            dismissal: DrawerDismissal::Explicit,
        })
    }

    pub fn column_widths(&self) -> Vec<f32> {
        if let Some(tabs) = &self.tabs {
            return vec![
                tabs.panels
                    .iter()
                    .map(|p| p.drawer_width())
                    .fold(0., f32::max),
            ];
        }
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
        partial_zen: bool,
    ) -> Option<DrawerPlacement> {
        self.place(layout, viewport, heights, partial_zen, None)
    }

    fn place(
        &self,
        layout: &DockLayout,
        viewport: [f32; 2],
        heights: &[f32],
        partial_zen: bool,
        measured_tile: Option<Bounds>,
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
        let (anchor, edge, axis) = if let DrawerAnchor::Column {
            column,
            group,
            origin,
        } = self.anchor
        {
            if partial_zen {
                return None;
            }
            let column = resolved.collapsed.iter().find(|c| c.id == column)?;
            let group = column.groups.iter().find(|g| g.group == group)?;
            let anchor = group
                .icons
                .iter()
                .find(|i| i.panel == origin)?
                .bounds
                .intersection(column.content)?;
            (anchor, layout.group_edge(group.group), Axis::Vertical)
        } else if let Some(anchor) = measured_tile.filter(|_| !partial_zen) {
            let group = layout.panel_group(self.anchor.tile()?.panel)?;
            (anchor, layout.group_edge(group), Axis::Vertical)
        } else {
            let tile_anchor = self.anchor.tile()?;
            let group = resolved
                .groups
                .iter()
                .find(|g| g.active == tile_anchor.panel)?;
            let index = layout
                .panel(tile_anchor.panel)
                .ok()?
                .tiles()
                .iter()
                .position(|t| t.id == tile_anchor.tile)?;
            let tile = *group.tiles.as_ref()?.tiles.get(index)?;
            let normal_anchor = Bounds {
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
            .intersection(group.bounds);
            let zen_anchor = partial_zen
                .then(|| layout.zen_toolbars(viewport).anchor(tile_anchor))
                .flatten();
            let anchor = if partial_zen {
                zen_anchor?.0
            } else {
                normal_anchor?
            };
            let edge = zen_anchor
                .map(|a| a.1)
                .or_else(|| layout.group_edge(group.id));
            (anchor, edge, group.axis)
        };
        let top = if partial_zen {
            WORKSPACE_SPACING
        } else {
            HEADER_HEIGHT
        };
        let available = Bounds {
            x: WORKSPACE_SPACING,
            y: top,
            width: viewport[0] - WORKSPACE_SPACING * 2.0,
            height: viewport[1] - top - WORKSPACE_SPACING,
        };
        let direction = match edge {
            Some(Edge::Top) => Edge::Bottom,
            Some(Edge::Bottom) => Edge::Top,
            Some(Edge::Left) => Edge::Right,
            Some(Edge::Right) => Edge::Left,
            None if axis == Axis::Horizontal => {
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

impl CustomizationState {
    pub(crate) fn measure_column_drawers(
        &mut self,
        measurements: Vec<ColumnDrawerMeasurement>,
    ) -> Result<(), String> {
        if measurements.iter().any(|m| {
            ![m.bounds.x, m.bounds.y, m.bounds.width, m.bounds.height]
                .into_iter()
                .all(f32::is_finite)
                || m.bounds.width <= 0.
                || m.bounds.height <= 0.
        }) {
            return Err("Invalid column drawer bounds".into());
        }
        self.column_drawer_bounds = measurements;
        Ok(())
    }

    pub(crate) fn column_drawer_groups(&self, layout: &DockLayout) -> Vec<GroupPlacement> {
        self.column_drawers
            .iter()
            .filter_map(|d| {
                let DrawerAnchor::Column {
                    column,
                    group,
                    origin,
                } = d.anchor
                else {
                    return None;
                };
                if layout.collapsed_column_for_group(group) != Some(column) {
                    return None;
                }
                let panels = layout.group_panels(group).ok()?;
                if !panels.contains(&origin) {
                    return None;
                }
                let bounds = self
                    .column_drawer_bounds
                    .iter()
                    .find(|m| m.group == group)?
                    .bounds;
                Some(GroupPlacement {
                    id: group,
                    bounds,
                    panels: panels.to_vec(),
                    active: layout.active_panel(origin)?,
                    axis: Axis::Vertical,
                    tabs_visible: true,
                    footer_grip: None,
                    floating: false,
                    resize_handles: Vec::new(),
                    tiles: None,
                })
            })
            .collect()
    }

    fn accepts_drawer_tile(&self, m: &DrawerTileMeasurement) -> bool {
        self.column_drawers.iter().any(|d| {
            matches!(d.anchor, DrawerAnchor::Column { column, .. } if column == m.column)
                && d.tabs.as_ref().is_some_and(|t| t.active == m.anchor.panel)
        })
    }

    pub(crate) fn measure_drawer_tiles(
        &mut self,
        layout: &DockLayout,
        measurements: Vec<DrawerTileMeasurement>,
    ) -> Result<(), String> {
        let mut accepted = Vec::with_capacity(measurements.len());
        for m in measurements {
            if ![m.bounds.x, m.bounds.y, m.bounds.width, m.bounds.height]
                .into_iter()
                .all(f32::is_finite)
                || m.bounds.width <= 0.
                || m.bounds.height <= 0.
            {
                return Err("Invalid drawer tile bounds".into());
            }
            if self.accepts_drawer_tile(&m)
                && layout
                    .panel(m.anchor.panel)
                    .is_ok_and(|p| p.tiles().iter().any(|t| t.id == m.anchor.tile))
                && !accepted
                    .iter()
                    .any(|a: &DrawerTileMeasurement| a.anchor == m.anchor)
            {
                accepted.push(m);
            }
        }
        self.drawer_tiles = accepted;
        Ok(())
    }

    pub(crate) fn drawer_tile_at(&self, point: [f32; 2]) -> Option<TileAnchor> {
        self.drawer_tiles.iter().rev().find_map(|m| {
            (self.accepts_drawer_tile(m) && m.bounds.contains(point[0], point[1]))
                .then_some(m.anchor)
        })
    }

    /// All hosts use this for current drawer geometry, including live projected
    /// toolbar origins. Normal dock and partial-Zen origins need no measurements.
    pub fn drawer_placement(
        &self,
        drawer: &ContentDrawer,
        layout: &DockLayout,
        viewport: [f32; 2],
        heights: &[f32],
        partial_zen: bool,
    ) -> Option<DrawerPlacement> {
        let measured = drawer.anchor.tile().and_then(|anchor| {
            self.drawer_tiles
                .iter()
                .find(|m| m.anchor == anchor && self.accepts_drawer_tile(m))
        });
        drawer.place(
            layout,
            viewport,
            heights,
            partial_zen,
            measured.map(|m| m.bounds),
        )
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
                    let Some(p) = d.placement(&layout, viewport, &heights, false) else {
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
                    assert_eq!(
                        p.interpolate_from(&p.closed(), 0.0).bounds,
                        p.closed().bounds
                    );
                    assert_eq!(p.interpolate_from(&p.closed(), 1.0).bounds, p.bounds);
                    if viewport == VIEWPORT {
                        for progress in [0.0, 0.1, 0.5, 1.0] {
                            let connection = p
                                .interpolate_from(&p.closed(), progress)
                                .connection()
                                .unwrap();
                            assert!((connection.depth - WORKSPACE_SPACING).abs() < 0.001);
                        }
                    }
                }
            }
            assert!(
                d.placement(&layout, VIEWPORT, &[f32::NAN, 10.0], false)
                    .is_none()
            );
            assert!(d.placement(&layout, VIEWPORT, &[], false).is_none());
        }
    }
    #[test]
    fn drawer_joins_rotate_and_avoid_body_corners() {
        for direction in [Edge::Bottom, Edge::Top, Edge::Right, Edge::Left] {
            for inset in [0.0_f32, 4.0, 8.0, 11.0, 14.0, 32.0] {
                let vertical = matches!(direction, Edge::Bottom | Edge::Top);
                let anchor = Bounds {
                    x: 100.0,
                    y: 100.0,
                    width: 36.0,
                    height: 36.0,
                };
                let body_start = 100.0 - inset;
                let bounds = match direction {
                    Edge::Bottom => Bounds {
                        x: body_start,
                        y: 142.0,
                        width: 200.0,
                        height: 150.0,
                    },
                    Edge::Top => Bounds {
                        x: body_start,
                        y: 4.0,
                        width: 200.0,
                        height: 90.0,
                    },
                    Edge::Right => Bounds {
                        x: 142.0,
                        y: body_start,
                        width: 150.0,
                        height: 200.0,
                    },
                    Edge::Left => Bounds {
                        x: 4.0,
                        y: body_start,
                        width: 90.0,
                        height: 200.0,
                    },
                };
                let p = DrawerPlacement {
                    bounds,
                    anchor,
                    direction,
                    columns: vec![],
                };
                let facing = match direction {
                    Edge::Top => [true, true, false, false],
                    Edge::Right => [false, true, true, false],
                    Edge::Bottom => [false, false, true, true],
                    Edge::Left => [true, false, false, true],
                };
                assert_eq!(p.source_corners(anchor), facing);
                assert_eq!(
                    p.source_corners(Bounds {
                        x: 50.0,
                        y: 50.0,
                        width: 150.0,
                        height: 150.0
                    }),
                    [false; 4]
                );
                let c = p.connection().unwrap();
                assert_eq!(c.depth, 6.0);
                assert_eq!(c.length, 36.0);
                assert_eq!(c.radii, [(inset - 8.0).clamp(0.0, 6.0), 6.0]);
                assert_eq!(
                    c.square_corners.iter().filter(|v| **v).count(),
                    usize::from(inset < 8.0)
                );
                let [xx, yx, xy, yy, tx, ty] = c.transform;
                let world = |u, v| {
                    [
                        c.bounds.x + xx * u + xy * v + tx,
                        c.bounds.y + yx * u + yy * v + ty,
                    ]
                };
                let near = world(18.0, 0.0);
                let far = world(18.0, c.depth);
                let axis = usize::from(vertical);
                let (expected_near, expected_far) = match direction {
                    Edge::Bottom | Edge::Right => (136.0, 142.0),
                    Edge::Top | Edge::Left => (100.0, 94.0),
                };
                assert_eq!(near[axis], expected_near);
                assert_eq!(far[axis], expected_far);
                assert_eq!(near[1 - axis], 118.0);
                assert_eq!(far[1 - axis], 118.0);
                // Mirror the cross-axis alignment: the other join must disappear.
                let mut mirrored = p.clone();
                if vertical {
                    mirrored.bounds.x = 136.0 + inset - bounds.width;
                } else {
                    mirrored.bounds.y = 136.0 + inset - bounds.height;
                }
                let opposite = mirrored.connection().unwrap();
                assert_eq!(opposite.radii, [6.0, c.radii[0]]);
            }
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
            let p = d
                .placement(&layout, VIEWPORT, &[500.0, 200.0], false)
                .unwrap();
            contained(&p, VIEWPORT);
            if matches!(p.direction, Edge::Left | Edge::Top) {
                assert_eq!(position[0], 960.0);
            } else {
                assert_eq!(position[0], 20.0);
            }
        }
    }
}
