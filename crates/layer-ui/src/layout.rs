//! Semantic docking topology. Coordinates are logical UI units, never pixels
//! belonging to the raster document. Earlier bands own shared corners.

use crate::{PanelConfig, PanelContent, ToolbarControl, ToolbarTile, WORKSPACE_SPACING};
use serde::{Deserialize, Serialize};

pub const TILE_SIZE: f32 = 36.0;
pub const TAB_BAR_HEIGHT: f32 = TILE_SIZE;
#[cfg(test)]
const TOOL_TILE_COUNT: usize = crate::TOOLBAR_CONTROLS.len();

// Reserve 20px for the trailing grip and 2px between it and the last tile.
fn ribbon_lanes(length: f32, count: usize) -> usize {
    let slots = (((length - 20.0) / (TILE_SIZE + 2.0)).floor() as usize).max(1);
    count.max(1).div_ceil(slots)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TileLayout {
    pub tiles: Vec<Bounds>,
    pub grip: Option<Bounds>,
    /// One insertion marker per slot, including append. Same order as tiles.
    pub insertion: Vec<Bounds>,
}

impl TileLayout {
    // Overflow is clipped, not scrolled. Preserve slot indices but never turn a
    // hidden tile into a visible insertion target at the panel edge.
    fn drop_slot(&self, point: [f32; 2], width: f32, height: f32) -> Option<(usize, Bounds)> {
        let clip = Bounds {
            width,
            height,
            ..Bounds::default()
        };
        if !clip.contains(point[0], point[1])
            || self.grip.is_some_and(|b| b.contains(point[0], point[1]))
        {
            return None;
        }
        let distance = |b: Bounds| {
            (point[0] - point[0].clamp(b.x, b.x + b.width)).powi(2)
                + (point[1] - point[1].clamp(b.y, b.y + b.height)).powi(2)
        };
        self.insertion
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                if let Some(tile) = self.tiles.get(index).or_else(|| self.tiles.last()) {
                    tile.intersection(clip)?;
                }
                Some((index, line.intersection(clip)?))
            })
            .min_by(|(_, a), (_, b)| distance(*a).total_cmp(&distance(*b)))
    }
}

/// One square-tile allocator for native and web strips. Cross-axis resizing
/// adds lanes; standalone ribbons also wrap when their long axis is constrained.
/// Tabbed tools retain their content inset; standalone strips are flush.
pub fn tile_layout(
    width: f32,
    height: f32,
    axis: Axis,
    count: usize,
    standalone: bool,
) -> TileLayout {
    let horizontal = axis == Axis::Horizontal;
    let cross = if horizontal { height } else { width };
    let padding = if standalone { 0.0 } else { 4.0 };
    let mut lanes = (((cross - padding * 2.0 + 2.0) / (TILE_SIZE + 2.0)).floor() as usize)
        .clamp(1, count.max(1));
    if standalone {
        let length = if horizontal { width } else { height };
        lanes = lanes.max(ribbon_lanes(length, count));
    }
    let slots = count.max(1).div_ceil(lanes);
    let grid = lanes as f32 * (TILE_SIZE + 2.0) - 2.0;
    let inset = ((cross - grid) * 0.5).max(padding);
    let tiles: Vec<_> = (0..count)
        .map(|i| {
            let (along, across) = if standalone {
                (i % slots, i / slots)
            } else {
                (i / lanes, i % lanes)
            };
            let along = padding + along as f32 * (TILE_SIZE + 2.0);
            let across = inset + across as f32 * (TILE_SIZE + 2.0);
            Bounds {
                x: if horizontal { along } else { across },
                y: if horizontal { across } else { along },
                width: TILE_SIZE,
                height: TILE_SIZE,
            }
        })
        .collect();
    // Same 20×24 footprint as the tab-bar grip, transposed for a side ribbon.
    // Matching the footprint also matches the visible dots' trailing inset.
    let grip = standalone.then_some(if horizontal {
        Bounds {
            x: width - 20.0,
            y: (height - 24.0) * 0.5,
            width: 20.0,
            height: 24.0,
        }
    } else {
        Bounds {
            x: (width - 24.0) * 0.5,
            y: height - 20.0,
            width: 24.0,
            height: 20.0,
        }
    });
    let flow = if (!standalone && lanes > 1) || (standalone && slots == 1) {
        if horizontal {
            Axis::Vertical
        } else {
            Axis::Horizontal
        }
    } else {
        axis
    };
    let line = |b: Bounds, after: bool| {
        let marker = if flow == Axis::Horizontal {
            Bounds {
                x: (b.x + if after { b.width } else { 0.0 } - 1.5).max(0.0),
                y: b.y,
                width: 3.0,
                height: b.height,
            }
        } else {
            Bounds {
                x: b.x,
                y: (b.y + if after { b.height } else { 0.0 } - 1.5).max(0.0),
                width: b.width,
                height: 3.0,
            }
        };
        marker
            .intersection(Bounds {
                width,
                height,
                ..Bounds::default()
            })
            .unwrap_or(marker)
    };
    let mut insertion = tiles.iter().map(|&b| line(b, false)).collect::<Vec<_>>();
    insertion.push(line(
        tiles.last().copied().unwrap_or(Bounds {
            x: padding,
            y: padding,
            width: TILE_SIZE.min(width),
            height: TILE_SIZE.min(height),
        }),
        !tiles.is_empty(),
    ));
    TileLayout {
        tiles,
        grip,
        insertion,
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ResizeDrag {
    offset: [f32; 2],
}
impl ResizeDrag {
    pub fn new(pointer: [f32; 2], divider: Bounds) -> Self {
        Self {
            offset: [
                pointer[0] - divider.x - divider.width * 0.5,
                pointer[1] - divider.y - divider.height * 0.5,
            ],
        }
    }
    pub fn position(self, pointer: [f32; 2]) -> [f32; 2] {
        [pointer[0] - self.offset[0], pointer[1] - self.offset[1]]
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabHit {
    pub group: u32,
    pub index: usize,
    pub bounds: Bounds,
}
#[derive(Clone, Debug, Serialize)]
pub struct DropHint {
    pub target: DockTarget,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Panel {
    Toolbar,
    Brushes,
    Sizes,
    Layers,
    CustomToolbar(u32),
}

// Retain the original system-panel string IDs in saved workspaces and DOM keys.
impl From<Panel> for String {
    fn from(panel: Panel) -> Self {
        match panel {
            Panel::Toolbar => "toolbar".into(),
            Panel::Brushes => "brushes".into(),
            Panel::Sizes => "sizes".into(),
            Panel::Layers => "layers".into(),
            Panel::CustomToolbar(id) => format!("toolbar:{id}"),
        }
    }
}
impl TryFrom<String> for Panel {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        Ok(match value.as_str() {
            "toolbar" => Self::Toolbar,
            "brushes" => Self::Brushes,
            "sizes" => Self::Sizes,
            "layers" => Self::Layers,
            _ => {
                let id: u32 = value
                    .strip_prefix("toolbar:")
                    .and_then(|id| id.parse().ok())
                    .filter(|id| *id > 0)
                    .ok_or("Unknown panel identity")?;
                if value != format!("toolbar:{id}") {
                    return Err("Invalid toolbar identity".into());
                }
                Self::CustomToolbar(id)
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelKind {
    Content,
    Tiles,
}

impl Panel {
    pub fn kind(self) -> PanelKind {
        if matches!(self, Self::Toolbar | Self::CustomToolbar(_)) {
            PanelKind::Tiles
        } else {
            PanelKind::Content
        }
    }
    pub const ALL: [Self; 4] = [Self::Toolbar, Self::Brushes, Self::Sizes, Self::Layers];
    pub fn label(self) -> &'static str {
        match self {
            Self::Toolbar => "Tools",
            Self::Brushes => "Brushes",
            Self::Sizes => "Brush size",
            Self::Layers => "Layers",
            Self::CustomToolbar(_) => "Toolbar",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Toolbar | Self::CustomToolbar(_) => "menu",
            Self::Brushes => "brush",
            Self::Sizes => "size",
            Self::Layers => "layers",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}
impl Edge {
    pub fn axis(self) -> Axis {
        match self {
            Self::Top | Self::Bottom => Axis::Vertical,
            Self::Left | Self::Right => Axis::Horizontal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockNode {
    Tabs {
        id: u32,
        panels: Vec<Panel>,
        active: Panel,
    },
    Split {
        id: u32,
        axis: Axis,
        fraction: f32,
        first: Box<DockNode>,
        second: Box<DockNode>,
    },
}
impl DockNode {
    fn group_for(&self, panel: Panel) -> Option<(u32, Panel)> {
        match self {
            Self::Tabs { id, panels, active } => panels.contains(&panel).then_some((*id, *active)),
            Self::Split { first, second, .. } => {
                first.group_for(panel).or_else(|| second.group_for(panel))
            }
        }
    }
    pub fn id(&self) -> u32 {
        match self {
            Self::Tabs { id, .. } | Self::Split { id, .. } => *id,
        }
    }
    fn find_mut(&mut self, target: u32) -> Option<&mut Self> {
        if self.id() == target {
            return Some(self);
        }
        match self {
            Self::Split { first, second, .. } => {
                first.find_mut(target).or_else(|| second.find_mut(target))
            }
            _ => None,
        }
    }
    fn remove(self, panel: Panel) -> Option<Self> {
        match self {
            Self::Tabs {
                id,
                mut panels,
                mut active,
            } => {
                panels.retain(|item| *item != panel);
                if panels.is_empty() {
                    return None;
                }
                if active == panel {
                    active = panels[0];
                }
                Some(Self::Tabs { id, panels, active })
            }
            Self::Split {
                id,
                axis,
                fraction,
                first,
                second,
            } => match (first.remove(panel), second.remove(panel)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    id,
                    axis,
                    fraction,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (first, second) => first.or(second),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockBand {
    pub id: u32,
    pub edge: Edge,
    pub extent: f32,
    pub root: DockNode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    /// Outermost first. Reordering changes corner ownership explicitly.
    pub bands: Vec<DockBand>,
    pub panels_visible: bool,
    #[serde(default = "PanelConfig::defaults")]
    pub panels: Vec<PanelConfig>,
    #[serde(default = "initial_tile_id")]
    next_tile_id: u32,
    next_id: u32,
}
fn initial_tile_id() -> u32 {
    crate::TOOLBAR_CONTROLS.len() as u32 + 1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockTarget {
    /// A new band adjacent to the center; `outer` grants corner priority.
    Edge {
        edge: Edge,
        outer: bool,
    },
    Tab {
        group: u32,
        /// Insertion slot before removal; absent appends. Supports same-group reorder.
        #[serde(default)]
        index: Option<usize>,
    },
    Split {
        group: u32,
        edge: Edge,
    },
    Tile {
        panel: Panel,
        /// Insert before this stable tile ID; absent appends.
        before: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DockItem {
    Panel { panel: Panel },
    Group { group: u32 },
    Tile { panel: Panel, tile: u32 },
}
impl DockItem {
    pub fn move_action(self, target: DockTarget, viewport: [f32; 2]) -> crate::UiAction {
        match self {
            Self::Panel { panel } => crate::UiAction::MovePanel {
                panel,
                target,
                viewport,
            },
            Self::Group { group } => crate::UiAction::MoveGroup {
                group,
                target,
                viewport,
            },
            Self::Tile { panel, tile } => crate::UiAction::MoveTile {
                panel,
                tile,
                target,
                viewport,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Bounds {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
    fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let width = (self.x + self.width).min(other.x + other.width) - x;
        let height = (self.y + self.height).min(other.y + other.height) - y;
        (width > 0.0 && height > 0.0).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }
    fn strip(&mut self, edge: Edge, amount: f32) -> Self {
        let mut out = *self;
        match edge {
            Edge::Left => {
                out.width = amount;
                self.x += amount;
                self.width -= amount;
            }
            Edge::Right => {
                out.width = amount;
                self.width -= amount;
                out.x += self.width;
            }
            Edge::Top => {
                out.height = amount;
                self.y += amount;
                self.height -= amount;
            }
            Edge::Bottom => {
                out.height = amount;
                self.height -= amount;
                out.y += self.height;
            }
        }
        out
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupPlacement {
    pub id: u32,
    pub bounds: Bounds,
    pub panels: Vec<Panel>,
    pub active: Panel,
    pub axis: Axis,
    pub tabs_visible: bool,
    /// Content-local geometry for the current tool ribbon (no host tab inset math).
    pub tiles: Option<TileLayout>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Divider {
    pub id: u32,
    pub band: bool,
    pub axis: Axis,
    pub bounds: Bounds,
    /// Parent bounds make native drag translation unambiguous.
    pub parent: Bounds,
    pub reversed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedLayout {
    pub tab_bar_height: f32,
    /// Header edge plus edges occupied by visible dock bands. The status HUD
    /// alone does not create a bottom-edge Zen reveal target.
    pub reveal_edges: Vec<Edge>,
    /// Unobstructed document fitting area, not the GPU widget allocation.
    pub work_area: Bounds,
    /// HUD strip inside the free area, above any bottom dock.
    pub status: Bounds,
    pub groups: Vec<GroupPlacement>,
    pub dividers: Vec<Divider>,
}

/// Transient two-column presentation; coordinates of the columns are local to
/// `bounds`. Hosts supply measured content heights and an animation fraction.
/// No saved docking dimensions, neighbor allocations or canvas fit are changed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PanelExpansion {
    pub group: u32,
    pub bounds: Bounds,
    pub preview: Bounds,
    pub configuration: Bounds,
    /// Only the first active tab reaches the left edge with the content color.
    /// Other joins meet the darker tab strip and must stay flat.
    pub concave_join: bool,
}
impl PanelExpansion {
    /// One interpolation for opening, closing and changing tabs. Starting from
    /// the last presented placement also makes interrupted animations continuous.
    pub fn interpolate_from(self, from: Self, progress: f32) -> Self {
        let p = progress.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * p;
        let rect = |a: Bounds, b: Bounds| Bounds {
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            width: mix(a.width, b.width),
            height: mix(a.height, b.height),
        };
        Self {
            bounds: rect(from.bounds, self.bounds),
            preview: rect(from.preview, self.preview),
            configuration: rect(from.configuration, self.configuration),
            ..self
        }
    }
    pub fn contains(self, point: [f32; 2]) -> bool {
        let local = [point[0] - self.bounds.x, point[1] - self.bounds.y];
        self.bounds.contains(point[0], point[1])
            && (self.preview.contains(local[0], local[1])
                || self.configuration.contains(local[0], local[1]))
    }
    pub fn header_contains(self, point: [f32; 2]) -> bool {
        let mut header = self.preview;
        header.x += self.bounds.x;
        header.y += self.bounds.y;
        header.height = self.configuration.y.min(header.height);
        header.contains(point[0], point[1])
    }
}

pub const PANEL_CONFIGURATION_WIDTH: f32 = 380.0;
pub const PANEL_EXPANSION_MS: u32 = 200;

impl DockLayout {
    pub fn panel_group(&self, panel: Panel) -> Option<u32> {
        self.bands
            .iter()
            .find_map(|b| b.root.group_for(panel))
            .map(|(id, _)| id)
    }

    pub(crate) fn active_panel(&self, panel: Panel) -> Option<Panel> {
        self.bands
            .iter()
            .find_map(|b| b.root.group_for(panel))
            .map(|(_, active)| active)
    }

    pub fn expanded_panel(
        &self,
        viewport: [f32; 2],
        panel: Panel,
        content_heights: [f32; 2],
        progress: f32,
    ) -> Option<PanelExpansion> {
        if !viewport
            .into_iter()
            .chain(content_heights)
            .chain([progress])
            .all(f32::is_finite)
            || viewport[0] < 1.0
            || viewport[1] < 1.0
        {
            return None;
        }
        let band = self
            .bands
            .iter()
            .find(|b| b.root.group_for(panel).is_some())?;
        let resolved = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let group = resolved.groups.iter().find(|g| g.panels.contains(&panel))?;
        let docked = group.bounds;
        let gap = crate::WORKSPACE_SPACING;
        let available = Bounds {
            x: gap,
            y: crate::HEADER_HEIGHT,
            width: (viewport[0] - gap * 2.0).max(1.0),
            height: (viewport[1] - crate::HEADER_HEIGHT - gap).max(1.0),
        };
        let side = matches!(band.edge, Edge::Left | Edge::Right);
        let preview_width = if side {
            docked.width
        } else {
            docked.width.min(280.0)
        }
        .min(available.width * 0.5);
        let config_width = PANEL_CONFIGURATION_WIDTH.min(available.width - preview_width);
        let tab_height = if group.tabs_visible {
            TAB_BAR_HEIGHT
        } else {
            0.0
        };
        let width = preview_width + config_width;
        let height = docked
            .height
            .max(content_heights[0])
            .max(content_heights[1] + tab_height)
            .min(available.height);
        // Side panels open inward. Top/bottom panels retain their nearest
        // horizontal anchor and grow down/up rather than outside the window.
        let config_left = band.edge == Edge::Right
            || (!side && docked.x + docked.width * 0.5 > viewport[0] * 0.5);
        let x = if config_left {
            docked.x + docked.width - width
        } else {
            docked.x
        };
        let y = if band.edge == Edge::Bottom {
            docked.y + docked.height - height
        } else {
            docked.y
        };
        let end = Bounds {
            x: x.clamp(available.x, available.x + available.width - width),
            y: y.clamp(available.y, available.y + available.height - height),
            width,
            height,
        };
        let p = progress.clamp(0.0, 1.0);
        let mix = |a, b| a + (b - a) * p;
        let bounds = Bounds {
            x: mix(docked.x, end.x),
            y: mix(docked.y, end.y),
            width: mix(docked.width, end.width),
            height: mix(docked.height, end.height),
        };
        let preview_width = mix(docked.width, preview_width);
        let revealed = (bounds.width - preview_width).max(0.0);
        Some(PanelExpansion {
            group: group.id,
            bounds,
            concave_join: config_left
                && group.tabs_visible
                && group.panels.first() == Some(&group.active),
            preview: Bounds {
                x: if config_left { revealed } else { 0.0 },
                y: 0.0,
                width: preview_width,
                height: bounds.height,
            },
            configuration: Bounds {
                x: if config_left {
                    revealed - config_width
                } else {
                    preview_width
                },
                y: tab_height,
                width: config_width,
                height: (bounds.height - tab_height).max(0.0),
            },
        })
    }
}

impl Default for DockLayout {
    fn default() -> Self {
        let tabs = |id, panel| DockNode::Tabs {
            id,
            panels: vec![panel],
            active: panel,
        };
        Self {
            panels_visible: true,
            panels: PanelConfig::defaults(),
            next_tile_id: initial_tile_id(),
            bands: vec![
                DockBand {
                    id: 3,
                    edge: Edge::Left,
                    extent: 232.0,
                    root: DockNode::Split {
                        id: 4,
                        axis: Axis::Vertical,
                        fraction: 0.68,
                        first: Box::new(tabs(5, Panel::Brushes)),
                        second: Box::new(tabs(6, Panel::Sizes)),
                    },
                },
                DockBand {
                    id: 7,
                    edge: Edge::Right,
                    extent: 232.0,
                    root: tabs(8, Panel::Layers),
                },
                DockBand {
                    id: 1,
                    edge: Edge::Top,
                    extent: TILE_SIZE + WORKSPACE_SPACING,
                    root: tabs(2, Panel::Toolbar),
                },
            ],
            next_id: 9,
        }
    }
}

impl DockLayout {
    /// A restored topology must contain each configured panel exactly once,
    /// globally unique IDs, finite dimensions, and valid active tabs/ratios.
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        let mut panels = Vec::new();
        fn node(
            n: &DockNode,
            ids: &mut std::collections::BTreeSet<u32>,
            panels: &mut Vec<Panel>,
            depth: usize,
        ) -> Result<(), String> {
            if depth > 128 || !ids.insert(n.id()) {
                return Err("Invalid workspace node identity or depth".into());
            }
            match n {
                DockNode::Tabs {
                    panels: tabs,
                    active,
                    ..
                } => {
                    if tabs.is_empty() || !tabs.contains(active) {
                        return Err("Invalid workspace tab selection".into());
                    }
                    for panel in tabs {
                        if panels.contains(panel) {
                            return Err("Workspace contains duplicate panels".into());
                        }
                        panels.push(*panel);
                    }
                }
                DockNode::Split {
                    fraction,
                    first,
                    second,
                    ..
                } => {
                    if !fraction.is_finite() || *fraction <= 0.0 || *fraction >= 1.0 {
                        return Err("Invalid workspace split ratio".into());
                    }
                    node(first, ids, panels, depth + 1)?;
                    node(second, ids, panels, depth + 1)?;
                }
            }
            Ok(())
        }
        for band in &self.bands {
            if !ids.insert(band.id) || !band.extent.is_finite() || band.extent <= 0.0 {
                return Err("Invalid workspace dock band".into());
            }
            node(&band.root, &mut ids, &mut panels, 0)?;
        }
        let mut tile_ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for config in &self.panels {
            config.validate()?;
            if let Panel::CustomToolbar(id) = config.id
                && (id == 0 || !ids.insert(id))
            {
                return Err("Invalid toolbar identity".into());
            }
            if !names.insert(config.title().to_lowercase()) {
                return Err("Panel names must be unique".into());
            }
            for tile in config.tiles() {
                if tile.id == 0 || !tile_ids.insert(tile.id) {
                    return Err("Duplicate or invalid toolbar tile identity".into());
                }
            }
            let index = panels
                .iter()
                .position(|id| *id == config.id)
                .ok_or("Panel configuration is not docked")?;
            panels.remove(index);
        }
        if !panels.is_empty()
            || Panel::ALL
                .iter()
                .any(|id| !self.panels.iter().any(|p| p.id == *id))
        {
            return Err("Workspace panel configuration is incomplete".into());
        }
        if tile_ids.last().is_some_and(|id| self.next_tile_id <= *id) {
            return Err("Invalid toolbar tile ID allocator".into());
        }
        if ids.last().is_some_and(|id| self.next_id <= *id) {
            return Err("Invalid workspace ID allocator".into());
        }
        Ok(())
    }

    pub fn panel(&self, id: Panel) -> Result<&PanelConfig, String> {
        self.panels
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "Unknown panel".into())
    }
    pub(crate) fn panel_mut(&mut self, id: Panel) -> Result<&mut PanelConfig, String> {
        self.panels
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| "Unknown panel".into())
    }
    pub fn group_panels(&self, id: u32) -> Result<&[Panel], String> {
        fn find(n: &DockNode, id: u32) -> Option<&[Panel]> {
            match n {
                DockNode::Tabs {
                    id: group, panels, ..
                } => (*group == id).then_some(panels),
                DockNode::Split { first, second, .. } => {
                    find(first, id).or_else(|| find(second, id))
                }
            }
        }
        self.bands
            .iter()
            .find_map(|b| find(&b.root, id))
            .ok_or_else(|| "Unknown tab group".into())
    }

    pub fn add_toolbar(
        &mut self,
        group: u32,
        name: &str,
        controls: &[ToolbarControl],
    ) -> Result<Panel, String> {
        let name = name.trim();
        self.validate_toolbar_name(name)?;
        self.group_panels(group)?;
        let mut next = self.clone();
        let id = Panel::CustomToolbar(next.allocate()?);
        next.panels.push(PanelConfig {
            id,
            tab_style: crate::TabStyle::Name,
            content: PanelContent::Toolbar {
                name: name.into(),
                tiles: Vec::new(),
            },
        });
        next.insert_tools(id, None, controls)?;
        let Some(DockNode::Tabs { panels, active, .. }) = next.node_mut(group) else {
            unreachable!()
        };
        panels.push(id);
        *active = id;
        next.validate()?;
        *self = next;
        Ok(id)
    }

    pub fn insert_tools(
        &mut self,
        panel: Panel,
        before: Option<u32>,
        controls: &[ToolbarControl],
    ) -> Result<(), String> {
        if controls.is_empty() {
            return Err("Select at least one tool".into());
        }
        for control in controls {
            control.validate()?;
        }
        let tiles = self.panel(panel)?.tiles();
        let index = match before {
            Some(id) => tiles
                .iter()
                .position(|t| t.id == id)
                .ok_or("The target tool no longer exists")?,
            None => tiles.len(),
        };
        let end = self
            .next_tile_id
            .checked_add(u32::try_from(controls.len()).map_err(|_| "Too many tools")?)
            .ok_or("Toolbar tile ID space exhausted")?;
        let added = controls
            .iter()
            .zip(self.next_tile_id..end)
            .map(|(&control, id)| ToolbarTile { id, control })
            .collect::<Vec<_>>();
        self.panel_mut(panel)?
            .tiles_mut()?
            .splice(index..index, added);
        self.next_tile_id = end;
        Ok(())
    }

    pub fn remove_tool(&mut self, panel: Panel, tile: u32) -> Result<(), String> {
        let tiles = self.panel_mut(panel)?.tiles_mut()?;
        let index = tiles
            .iter()
            .position(|t| t.id == tile)
            .ok_or("The tool no longer exists")?;
        tiles.remove(index);
        Ok(())
    }

    fn move_tile(&mut self, panel: Panel, tile: u32, target: DockTarget) -> Result<(), String> {
        let source = self
            .panel(panel)?
            .tiles()
            .iter()
            .find(|t| t.id == tile)
            .ok_or("The dragged tool no longer exists")?
            .clone();
        let DockTarget::Tile {
            panel: destination,
            before,
        } = target
        else {
            return Err("Drop tools inside a toolbar".into());
        };
        if self.panel(destination)?.id.kind() != PanelKind::Tiles {
            return Err("Drop tools inside a toolbar".into());
        }
        if let Some(id) = before
            && !self.panel(destination)?.tiles().iter().any(|t| t.id == id)
        {
            return Err("The target tool no longer exists".into());
        }
        if panel == destination && before == Some(tile) {
            return Ok(());
        }
        // All fallible lookups are checked before either toolbar changes.
        self.remove_tool(panel, tile)?;
        let tiles = self.panel_mut(destination)?.tiles_mut()?;
        let index = before
            .and_then(|id| tiles.iter().position(|t| t.id == id))
            .unwrap_or(tiles.len());
        tiles.insert(index, source);
        Ok(())
    }

    /// Restore docking defaults without throwing away customized panels/tools.
    pub fn reset_docking(&mut self) {
        let defaults = Self::default();
        self.bands = defaults.bands;
        self.panels_visible = true;
        let tools = self
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
            .map(|p| p.id)
            .collect();
        if let Some(DockNode::Tabs { panels, active, .. }) = self.node_mut(2) {
            *panels = tools;
            *active = Panel::Toolbar;
        }
    }

    pub fn resize_workspace(
        &mut self,
        id: u32,
        position: [f32; 2],
        viewport: [f32; 2],
    ) -> Result<(), String> {
        self.resize(
            id,
            [
                position[0] - WORKSPACE_SPACING,
                position[1] - crate::HEADER_HEIGHT,
            ],
            [
                viewport[0] - WORKSPACE_SPACING * 2.0,
                viewport[1] - crate::HEADER_HEIGHT - WORKSPACE_SPACING,
            ],
        )
    }
    fn allocate(&mut self) -> Result<u32, String> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("Workspace ID space exhausted")?;
        Ok(id)
    }
    fn node_mut(&mut self, id: u32) -> Option<&mut DockNode> {
        self.bands
            .iter_mut()
            .find_map(|band| band.root.find_mut(id))
    }

    pub fn move_panel(
        &mut self,
        viewport: [f32; 2],
        panel: Panel,
        target: DockTarget,
    ) -> Result<(), String> {
        self.move_item(viewport, DockItem::Panel { panel }, target)
    }

    /// Both individual tabs and whole groups use one transactional move path.
    /// Group moves preserve tab order, active tab and the group's identity.
    pub fn move_item(
        &mut self,
        viewport: [f32; 2],
        item: DockItem,
        target: DockTarget,
    ) -> Result<(), String> {
        if !viewport.into_iter().all(|v| v.is_finite() && v > 0.0) {
            return Err("Invalid workspace size".into());
        }
        if let DockItem::Tile { panel, tile } = item {
            return self.move_tile(panel, tile, target);
        }
        if matches!(target, DockTarget::Tile { .. }) {
            return Err("Only tools can be dropped inside a toolbar".into());
        }
        let before = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let mut next = self.clone();
        let (moving, selected, source_group, source_index, source_len) = match item {
            DockItem::Panel { panel } => {
                let (id, index, len) = next
                    .bands
                    .iter()
                    .find_map(|b| find_tab(&b.root, panel))
                    .ok_or("Unknown panel")?;
                (vec![panel], panel, id, index, len)
            }
            DockItem::Group { group } => {
                let Some(DockNode::Tabs { panels, active, .. }) = next.node_mut(group) else {
                    return Err("Unknown tab group".into());
                };
                (panels.clone(), *active, group, 0, panels.len())
            }
            DockItem::Tile { .. } => unreachable!(),
        };
        let whole = moving.len() == source_len;
        if matches!(target, DockTarget::Tab {group, ..} if group == source_group) && whole {
            return Ok(());
        }
        let moving_id = if whole || matches!(target, DockTarget::Tab { .. }) {
            source_group
        } else {
            next.allocate()?
        };
        next.bands = next
            .bands
            .into_iter()
            .filter_map(|band| {
                let mut root = Some(band.root);
                for panel in &moving {
                    root = root?.remove(*panel);
                }
                Some(DockBand {
                    root: root?,
                    ..band
                })
            })
            .collect();
        let tiles = moving.len() == 1 && selected.kind() == PanelKind::Tiles;
        let source = before.groups.iter().find(|g| g.id == source_group);
        let moved_width = if tiles && source.is_some_and(|g| g.axis == Axis::Horizontal) {
            TILE_SIZE
        } else {
            source.map(|g| g.bounds.width).unwrap_or(246.0)
        };
        let moving = DockNode::Tabs {
            id: moving_id,
            panels: moving,
            active: selected,
        };
        match target {
            DockTarget::Tile { .. } => unreachable!(),
            DockTarget::Edge { edge, outer } => {
                let id = next.allocate()?;
                let band = DockBand {
                    id,
                    edge,
                    extent: if tiles {
                        TILE_SIZE + WORKSPACE_SPACING
                    } else {
                        252.0
                    },
                    root: moving,
                };
                let index = if outer { 0 } else { next.bands.len() };
                next.bands.insert(index, band);
            }
            DockTarget::Tab { group, index } => {
                let Some(DockNode::Tabs { panels, active, .. }) = next.node_mut(group) else {
                    return Err("The tab target no longer exists".into());
                };
                let mut index = index.unwrap_or(panels.len() + usize::from(group == source_group));
                if group == source_group && index > source_index {
                    index -= 1;
                }
                let DockNode::Tabs { panels: moved, .. } = moving else {
                    unreachable!()
                };
                let index = index.min(panels.len());
                panels.splice(index..index, moved);
                *active = selected;
            }
            DockTarget::Split { group, edge } => {
                let mut fraction = 0.5;
                if edge.axis() == Axis::Horizontal {
                    let target_width = before
                        .groups
                        .iter()
                        .find(|g| g.id == group)
                        .ok_or("Unknown split target")?
                        .bounds
                        .width;
                    let after = next.workspace(
                        viewport[0],
                        viewport[1],
                        crate::HEADER_HEIGHT,
                        crate::STATUS_HEIGHT,
                    );
                    let current = after
                        .groups
                        .iter()
                        .find(|g| g.id == group)
                        .ok_or("The split target no longer exists")?
                        .bounds
                        .width;
                    let band = next
                        .bands
                        .iter_mut()
                        .find_map(|b| b.root.find_mut(group).is_some().then_some(b))
                        .ok_or("Unknown target dock")?;
                    let bounds = after
                        .dividers
                        .iter()
                        .find(|d| d.band && d.id == band.id)
                        .unwrap();
                    let root_width = if band.edge.axis() == Axis::Horizontal {
                        if bounds.reversed {
                            bounds.parent.x + bounds.parent.width
                                - bounds.bounds.x
                                - bounds.bounds.width
                        } else {
                            bounds.bounds.x - bounds.parent.x
                        }
                    } else {
                        bounds.parent.width
                    };
                    let desired = target_width + moved_width + WORKSPACE_SPACING;
                    let grow = desired - current;
                    if band.edge.axis() == Axis::Horizontal {
                        grow_node_width(&mut band.root, group, root_width, grow);
                        band.extent = root_width + grow + WORKSPACE_SPACING;
                    } else if grow > 0.5 {
                        return Err("Not enough horizontal space to preserve panel widths".into());
                    }
                    let width = if band.edge.axis() == Axis::Horizontal {
                        desired
                    } else {
                        current
                    };
                    fraction = if edge == Edge::Left {
                        moved_width / (width - WORKSPACE_SPACING)
                    } else {
                        1.0 - moved_width / (width - WORKSPACE_SPACING)
                    };
                }
                let id = next.allocate()?;
                let node = next
                    .node_mut(group)
                    .ok_or("The split target no longer exists")?;
                if !matches!(node, DockNode::Tabs { .. }) {
                    return Err("Split targets must be tab groups".into());
                }
                let (first, second) = if matches!(edge, Edge::Top | Edge::Left) {
                    (moving, node.clone())
                } else {
                    (node.clone(), moving)
                };
                *node = DockNode::Split {
                    id,
                    axis: edge.axis(),
                    fraction,
                    first: Box::new(first),
                    second: Box::new(second),
                };
            }
        }
        if let DockTarget::Split {
            group,
            edge: Edge::Left | Edge::Right,
        } = target
        {
            let after = next.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            );
            let width = |layout: &ResolvedLayout, id| {
                layout
                    .groups
                    .iter()
                    .find(|g| g.id == id)
                    .map(|g| g.bounds.width)
                    .unwrap_or(0.0)
            };
            if width(&after, group) + 0.5 < width(&before, group)
                || width(&after, moving_id) + 0.5 < moved_width
                || after.groups.iter().any(|g| {
                    g.axis == Axis::Vertical && g.bounds.width + 0.5 < width(&before, g.id)
                })
            {
                return Err("Not enough horizontal space to preserve panel widths".into());
            }
        }
        *self = next;
        Ok(())
    }
    /// Select a tab, returning whether the active tab changed.
    pub fn select_tab(&mut self, group: u32, panel: Panel) -> Result<bool, String> {
        if let Some(DockNode::Tabs { panels, active, .. }) = self.node_mut(group)
            && panels.contains(&panel)
        {
            let changed = *active != panel;
            *active = panel;
            Ok(changed)
        } else {
            Err("Panel is not in this tab group".into())
        }
    }
    /// Both hosts supply a divider center. The shared allocator owns handle
    /// thickness, reversed edges, split ratios and clamping in one place.
    pub fn resize(
        &mut self,
        id: u32,
        position: [f32; 2],
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if !position.into_iter().chain(viewport).all(f32::is_finite)
            || viewport.into_iter().any(|v| v <= 0.0)
        {
            return Err("Invalid dock size".into());
        }
        let resolved = self.resolve(viewport[0], viewport[1]);
        let d = resolved
            .dividers
            .iter()
            .find(|d| d.id == id)
            .ok_or("Unknown divider")?;
        let center = if d.axis == Axis::Horizontal {
            d.bounds.x + d.bounds.width * 0.5
        } else {
            d.bounds.y + d.bounds.height * 0.5
        };
        let coordinate = position[usize::from(d.axis == Axis::Vertical)];
        if (coordinate - center).abs() < 0.001 {
            return Ok(());
        }
        let (offset, extent, gap) = if d.axis == Axis::Horizontal {
            (position[0] - d.parent.x, d.parent.width, d.bounds.width)
        } else {
            (position[1] - d.parent.y, d.parent.height, d.bounds.height)
        };
        if d.band {
            let extent = if d.reversed {
                extent - offset + gap * 0.5
            } else {
                offset + gap * 0.5
            };
            self.bands.iter_mut().find(|b| b.id == id).unwrap().extent =
                extent.clamp(TILE_SIZE + WORKSPACE_SPACING, 800.0);
        } else if let Some(DockNode::Split { fraction, .. }) = self.node_mut(id) {
            *fraction = ((offset - gap * 0.5) / (extent - gap).max(1.0)).clamp(0.15, 0.85);
        }
        Ok(())
    }
    pub fn prioritize(&mut self, id: u32) -> Result<(), String> {
        let index = self
            .bands
            .iter()
            .position(|band| band.id == id)
            .ok_or("Unknown dock")?;
        let band = self.bands.remove(index);
        self.bands.insert(0, band);
        Ok(())
    }
    pub fn resolve(&self, width: f32, height: f32) -> ResolvedLayout {
        let mut remaining = Bounds {
            x: 0.0,
            y: 0.0,
            width: finite_extent(width),
            height: finite_extent(height),
        };
        let mut result = ResolvedLayout {
            tab_bar_height: TAB_BAR_HEIGHT,
            reveal_edges: vec![Edge::Top],
            work_area: remaining,
            status: Bounds::default(),
            groups: Vec::new(),
            dividers: Vec::new(),
        };
        for band in &self.bands {
            if !self.panels_visible {
                continue;
            }
            let parent = remaining;
            let axis = if matches!(band.edge, Edge::Left | Edge::Right) {
                Axis::Vertical
            } else {
                Axis::Horizontal
            };
            let length = if axis == Axis::Horizontal {
                remaining.width
            } else {
                remaining.height
            };
            let ribbon_min = ribbon_cross_min(&band.root, axis, length, self);
            let available = if band.edge.axis() == Axis::Horizontal {
                remaining.width
            } else {
                remaining.height
            };
            // Use free canvas space before compressing a dock on small windows.
            // Automatic growth is derived, not written back into the user's
            // saved thickness. Widening/tallening the window can unwrap again.
            let minimum = if ribbon_min > 0.0 {
                ribbon_min + WORKSPACE_SPACING
            } else {
                0.0
            };
            let limit = (available - 64.0).max(minimum).min(available).max(0.0);
            let extent = band.extent.max(minimum).min(limit);
            let mut bounds = remaining.strip(band.edge, extent);
            // The inside six logical units are a generous native drag handle.
            let opposite = match band.edge {
                Edge::Left => Edge::Right,
                Edge::Right => Edge::Left,
                Edge::Top => Edge::Bottom,
                Edge::Bottom => Edge::Top,
            };
            let divider = bounds.strip(opposite, extent.min(WORKSPACE_SPACING));
            if bounds.width > 0.0
                && bounds.height > 0.0
                && !result.reveal_edges.contains(&band.edge)
            {
                result.reveal_edges.push(band.edge);
            }
            result.dividers.push(Divider {
                id: band.id,
                band: true,
                axis: band.edge.axis(),
                bounds: divider,
                parent,
                reversed: matches!(band.edge, Edge::Bottom | Edge::Right),
            });
            resolve_node(&band.root, bounds, axis, self, &mut result);
        }
        result.work_area = remaining;
        result
    }

    /// Hosts supply measured native chrome in logical units. All panel and
    /// divider coordinates remain relative to the full-window canvas.
    pub fn workspace(&self, width: f32, height: f32, top: f32, bottom: f32) -> ResolvedLayout {
        // The header already includes bottom padding. Keep the outer inset on
        // the sides/bottom only, rather than doubling the gap above the docks.
        let mut result = self.resolve(
            (width - WORKSPACE_SPACING * 2.0).max(1.0),
            (height - top - WORKSPACE_SPACING).max(1.0),
        );
        let offset = |b: &mut Bounds| {
            b.x += WORKSPACE_SPACING;
            b.y += top;
        };
        offset(&mut result.work_area);
        let height = bottom.min(result.work_area.height);
        result.status = Bounds {
            x: result.work_area.x,
            y: result.work_area.y + result.work_area.height - height,
            width: result.work_area.width,
            height,
        };
        result.work_area.height -= height;
        for group in &mut result.groups {
            offset(&mut group.bounds);
        }
        for divider in &mut result.dividers {
            offset(&mut divider.bounds);
            offset(&mut divider.parent);
        }
        result
    }
}

impl ResolvedLayout {
    pub fn tile_drop_hint(&self, point: [f32; 2], config: &DockLayout) -> Option<DropHint> {
        for group in &self.groups {
            let Some(tiles) = &group.tiles else { continue };
            let mut body = group.bounds;
            if group.tabs_visible {
                body.y += TAB_BAR_HEIGHT;
                body.height -= TAB_BAR_HEIGHT;
            }
            if !body.contains(point[0], point[1]) {
                continue;
            }
            let local = [point[0] - body.x, point[1] - body.y];
            let (index, line) = tiles.drop_slot(local, body.width, body.height)?;
            return Some(DropHint {
                target: DockTarget::Tile {
                    panel: group.active,
                    before: config
                        .panel(group.active)
                        .ok()?
                        .tiles()
                        .get(index)
                        .map(|t| t.id),
                },
                bounds: Bounds {
                    x: body.x + line.x,
                    y: body.y + line.y,
                    ..line
                },
            });
        }
        None
    }
    /// Measured native tab rectangles take priority over split zones. Body
    /// centers append tabs; the top/bottom 20% of the body split vertically.
    pub fn drop_hint(&self, x: f32, y: f32, tabs: &[TabHit]) -> DropHint {
        for group in &self.groups {
            let b = group.bounds;
            if !b.contains(x, y) {
                continue;
            }
            let header = !(group.panels.len() == 1 && group.active.kind() == PanelKind::Tiles);
            if header && y < b.y + TAB_BAR_HEIGHT {
                let mut index = group.panels.len();
                // The trailing grip is fixed even when tabs scroll/overflow.
                for tab in tabs
                    .iter()
                    .filter(|t| t.group == group.id && x < b.x + b.width - 20.0)
                {
                    if x < tab.bounds.x + tab.bounds.width * 0.5 {
                        index = tab.index;
                        break;
                    }
                }
                return DropHint {
                    target: DockTarget::Tab {
                        group: group.id,
                        index: Some(index),
                    },
                    bounds: tab_insertion_line(group, tabs, index),
                };
            }
            let body = if header {
                Bounds {
                    y: b.y + TAB_BAR_HEIGHT,
                    height: (b.height - TAB_BAR_HEIGHT).max(0.0),
                    ..b
                }
            } else {
                b
            };
            // Side strips stay narrow; vertical splits get generous targets.
            // Header handling above always wins over the upper body zone.
            let edge = if x < body.x + 18.0 {
                Some(Edge::Left)
            } else if x > body.x + body.width - 18.0 {
                Some(Edge::Right)
            } else if y < body.y + body.height * 0.2 {
                Some(Edge::Top)
            } else if y > body.y + body.height * 0.8 {
                Some(Edge::Bottom)
            } else {
                None
            };
            if let Some(edge) = edge {
                return DropHint {
                    target: DockTarget::Split {
                        group: group.id,
                        edge,
                    },
                    bounds: edge_line(b, edge),
                };
            }
            return DropHint {
                target: DockTarget::Tab {
                    group: group.id,
                    index: None,
                },
                bounds: tab_insertion_line(group, tabs, group.panels.len()),
            };
        }
        let mut all = self.work_area;
        all.height += self.status.height;
        for g in &self.groups {
            let right = (all.x + all.width).max(g.bounds.x + g.bounds.width);
            let bottom = (all.y + all.height).max(g.bounds.y + g.bounds.height);
            all.x = all.x.min(g.bounds.x);
            all.y = all.y.min(g.bounds.y);
            all.width = right - all.x;
            all.height = bottom - all.y;
        }
        let (distance, outer_edge) = nearest_edge(all, x, y);
        let outer = distance < 12.0;
        let edge = if outer {
            outer_edge
        } else {
            nearest_edge(self.work_area, x, y).1
        };
        DropHint {
            target: DockTarget::Edge { edge, outer },
            bounds: edge_line(if outer { all } else { self.work_area }, edge),
        }
    }
    /// Hidden chrome reveals only at the window edge; visible chrome remains
    /// available near its controls. Hosts may also pin it for focus/popovers.
    pub fn near_chrome(&self, point: [f32; 2], viewport: [f32; 2], hidden: bool) -> bool {
        self.near_chrome_with_distances(point, viewport, hidden, 80.0, 40.0)
    }
    pub fn near_chrome_with_distances(
        &self,
        point: [f32; 2],
        viewport: [f32; 2],
        hidden: bool,
        reveal: f32,
        margin: f32,
    ) -> bool {
        let [x, y] = point;
        let [width, height] = viewport;
        if !(Bounds {
            x: 0.0,
            y: 0.0,
            width,
            height,
        })
        .contains(x, y)
        {
            return false;
        }
        // Reveal only edges with controls. The header always occupies the top;
        // the HUD alone must not activate an otherwise empty bottom edge.
        // Keep enabled reveal zones visible too, preserving hysteresis.
        let edge = self.reveal_edges.iter().any(|edge| match edge {
            Edge::Top => y <= reveal,
            Edge::Bottom => height - y <= reveal,
            Edge::Left => x <= reveal,
            Edge::Right => width - x <= reveal,
        });
        if hidden || edge {
            return edge;
        }
        y <= crate::HEADER_HEIGHT + margin
            || (x >= self.status.x - margin
                && x <= self.status.x + self.status.width + margin
                && y >= self.status.y - margin
                && y <= self.status.y + self.status.height + margin)
            || self.groups.iter().any(|g| {
                let b = g.bounds;
                x >= b.x - margin
                    && x <= b.x + b.width + margin
                    && y >= b.y - margin
                    && y <= b.y + b.height + margin
            })
    }
}

fn finite_extent(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

// Intrinsic ribbon thickness, including ribbons nested beside other panels.
// Use the same split fractions as allocation; no resize callbacks or feedback.
fn ribbon_cross_min(node: &DockNode, ribbon_axis: Axis, length: f32, layout: &DockLayout) -> f32 {
    match node {
        DockNode::Tabs { panels, active, .. }
            if panels.len() == 1 && active.kind() == PanelKind::Tiles =>
        {
            ribbon_lanes(
                length,
                layout.panel(*active).map(|p| p.tiles().len()).unwrap_or(0),
            ) as f32
                * (TILE_SIZE + 2.0)
                - 2.0
        }
        DockNode::Tabs { panels, .. } if panels.iter().any(|p| p.kind() == PanelKind::Tiles) => {
            // A tab bar must not consume the ribbon's entire old one-row
            // allocation. Reserve one padded lane; additional tiles may clip.
            TILE_SIZE
                + 8.0
                + if ribbon_axis == Axis::Horizontal {
                    TAB_BAR_HEIGHT
                } else {
                    0.0
                }
        }
        DockNode::Tabs { .. } => 0.0,
        DockNode::Split {
            axis,
            fraction,
            first,
            second,
            ..
        } => {
            if *axis == ribbon_axis {
                let usable = (length - WORKSPACE_SPACING).max(0.0);
                ribbon_cross_min(first, ribbon_axis, usable * fraction, layout).max(
                    ribbon_cross_min(second, ribbon_axis, usable * (1.0 - fraction), layout),
                )
            } else {
                let a = ribbon_cross_min(first, ribbon_axis, length, layout);
                let b = ribbon_cross_min(second, ribbon_axis, length, layout);
                if a == 0.0 && b == 0.0 {
                    0.0
                } else {
                    (a / fraction).max(b / (1.0 - fraction)) + WORKSPACE_SPACING
                }
            }
        }
    }
}
fn find_tab(node: &DockNode, panel: Panel) -> Option<(u32, usize, usize)> {
    match node {
        DockNode::Tabs { id, panels, .. } => panels
            .iter()
            .position(|p| *p == panel)
            .map(|index| (*id, index, panels.len())),
        DockNode::Split { first, second, .. } => {
            find_tab(first, panel).or_else(|| find_tab(second, panel))
        }
    }
}
// Expand only the target branch. Horizontal siblings keep their width;
// vertical siblings share the wider column, keeping their height fractions.
fn grow_node_width(node: &mut DockNode, group: u32, width: f32, delta: f32) -> bool {
    if let DockNode::Tabs { id, .. } = node {
        return *id == group;
    }
    let DockNode::Split {
        axis,
        fraction,
        first,
        second,
        ..
    } = node
    else {
        unreachable!()
    };
    if *axis == Axis::Vertical {
        return grow_node_width(first, group, width, delta)
            || grow_node_width(second, group, width, delta);
    }
    let usable = (width - WORKSPACE_SPACING).max(0.0);
    let first_width = usable * *fraction;
    if grow_node_width(first, group, first_width, delta) {
        *fraction = (first_width + delta) / (usable + delta);
        true
    } else if grow_node_width(second, group, usable - first_width, delta) {
        *fraction = first_width / (usable + delta);
        true
    } else {
        false
    }
}

fn tab_insertion_line(group: &GroupPlacement, tabs: &[TabHit], index: usize) -> Bounds {
    let b = group.bounds;
    let slot = tabs
        .iter()
        .find(|t| t.group == group.id && t.index == index)
        .map(|t| t.bounds.x)
        .or_else(|| {
            tabs.iter()
                .filter(|t| t.group == group.id)
                .max_by_key(|t| t.index)
                .map(|t| t.bounds.x + t.bounds.width)
        })
        .unwrap_or(b.x);
    // Reserve the fixed 20px grip, including the full
    // marker width. Off-screen/wrapped tabs never hide an append indicator.
    Bounds {
        x: (slot - 1.5).clamp(b.x, b.x + (b.width - 23.0).max(0.0)),
        y: b.y,
        width: 3.0,
        height: TAB_BAR_HEIGHT.min(b.height),
    }
}

fn nearest_edge(b: Bounds, x: f32, y: f32) -> (f32, Edge) {
    [
        (x - b.x, Edge::Left),
        (b.x + b.width - x, Edge::Right),
        (y - b.y, Edge::Top),
        (b.y + b.height - y, Edge::Bottom),
    ]
    .into_iter()
    .min_by(|a, b| a.0.total_cmp(&b.0))
    .unwrap()
}
fn edge_line(mut b: Bounds, edge: Edge) -> Bounds {
    b.strip(edge, 3.0)
}
fn resolve_node(
    node: &DockNode,
    bounds: Bounds,
    orientation: Axis,
    layout: &DockLayout,
    result: &mut ResolvedLayout,
) {
    match node {
        DockNode::Tabs { id, panels, active } => {
            let standalone = panels.len() == 1;
            let tabs_visible = !standalone || active.kind() != PanelKind::Tiles;
            result.groups.push(GroupPlacement {
                id: *id,
                bounds,
                panels: panels.clone(),
                active: *active,
                axis: orientation,
                tabs_visible,
                tiles: (active.kind() == PanelKind::Tiles).then(|| {
                    tile_layout(
                        bounds.width,
                        bounds.height - if tabs_visible { TAB_BAR_HEIGHT } else { 0.0 },
                        orientation,
                        layout.panel(*active).map(|p| p.tiles().len()).unwrap_or(0),
                        standalone,
                    )
                }),
            });
        }
        DockNode::Split {
            id,
            axis,
            fraction,
            first,
            second,
        } => {
            let mut rest = bounds;
            let length = if *axis == Axis::Horizontal {
                bounds.width
            } else {
                bounds.height
            };
            let gap = length.min(WORKSPACE_SPACING);
            let edge = if *axis == Axis::Horizontal {
                Edge::Left
            } else {
                Edge::Top
            };
            let a = rest.strip(edge, (length - gap) * fraction);
            let divider = rest.strip(edge, gap);
            result.dividers.push(Divider {
                id: *id,
                band: false,
                axis: *axis,
                bounds: divider,
                parent: bounds,
                reversed: false,
            });
            resolve_node(first, a, orientation, layout, result);
            resolve_node(second, rest, orientation, layout, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_columns_open_inward_and_only_grow_to_content() {
        let mut layout = DockLayout::default();
        let viewport = [1200.0, 900.0];
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            layout.bands = vec![DockBand {
                id: 3,
                edge,
                extent: 232.0,
                root: DockNode::Tabs {
                    id: 5,
                    panels: vec![Panel::Sizes],
                    active: Panel::Sizes,
                },
            }];
            let before = layout.clone();
            let normal = layout.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            );
            let docked = normal.groups[0].bounds;
            let start = layout
                .expanded_panel(viewport, Panel::Sizes, [340.0, 480.0], 0.0)
                .unwrap();
            let end = layout
                .expanded_panel(viewport, Panel::Sizes, [340.0, 480.0], 1.0)
                .unwrap();
            let middle = layout
                .expanded_panel(viewport, Panel::Sizes, [340.0, 480.0], 0.5)
                .unwrap();
            assert_eq!(start.bounds, docked);
            assert_eq!(end.bounds.height, docked.height.max(480.0 + TAB_BAR_HEIGHT));
            assert_eq!(
                middle.bounds.height,
                (docked.height + end.bounds.height) * 0.5
            );
            assert_eq!(
                end.preview.height,
                end.configuration.height + TAB_BAR_HEIGHT
            );
            assert_eq!(end.configuration.y, TAB_BAR_HEIGHT);
            assert_eq!(
                end.bounds.width,
                end.preview.width + end.configuration.width
            );
            if matches!(edge, Edge::Left | Edge::Right) {
                assert_eq!(end.preview.width, docked.width);
            }
            if edge == Edge::Right {
                assert_eq!(end.configuration.x, 0.0);
                assert!(end.concave_join);
            }
            if edge == Edge::Left {
                assert_eq!(end.preview.x, 0.0);
                assert!(!end.concave_join);
            }
            assert!(end.bounds.x >= 6.0 && end.bounds.y >= crate::HEADER_HEIGHT);
            assert!(end.bounds.x + end.bounds.width <= viewport[0] - 6.0);
            assert!(end.bounds.y + end.bounds.height <= viewport[1] - 6.0);
            assert_eq!(layout, before);
            for small in [[640.0, 480.0], [320.0, 400.0]] {
                let expanded = layout
                    .expanded_panel(small, Panel::Sizes, [2000.0; 2], 1.0)
                    .unwrap();
                assert!(expanded.bounds.x + expanded.bounds.width <= small[0]);
                assert!(expanded.bounds.y + expanded.bounds.height <= small[1]);
            }
        }
        assert!(
            layout
                .expanded_panel(viewport, Panel::Sizes, [f32::NAN, 10.0], 1.0)
                .is_none()
        );
        layout.bands[0].edge = Edge::Right;
        layout.bands[0].root = DockNode::Tabs {
            id: 5,
            panels: vec![Panel::Layers, Panel::Sizes],
            active: Panel::Sizes,
        };
        assert!(
            !layout
                .expanded_panel(viewport, Panel::Sizes, [300.0, 400.0], 1.0)
                .unwrap()
                .concave_join
        );
    }
    #[test]
    fn tab_size_changes_interpolate_from_the_presented_bounds() {
        let layout = DockLayout {
            bands: vec![DockBand {
                id: 3,
                edge: Edge::Bottom,
                extent: 160.0,
                root: DockNode::Tabs {
                    id: 5,
                    panels: vec![Panel::Layers, Panel::Sizes],
                    active: Panel::Sizes,
                },
            }],
            ..DockLayout::default()
        };
        let shape = |height, progress| {
            layout
                .expanded_panel([1200.0, 900.0], Panel::Sizes, [240.0, height], progress)
                .unwrap()
        };
        let from = shape(320.0, 1.0);
        let to = shape(480.0, 1.0);
        let check = |actual: PanelExpansion, expected: PanelExpansion| {
            assert_eq!(actual.bounds, expected.bounds);
            assert_eq!(actual.preview, expected.preview);
            assert_eq!(actual.configuration, expected.configuration);
        };
        check(to.interpolate_from(from, 0.0), from);
        check(to.interpolate_from(from, 1.0), to);
        let middle = to.interpolate_from(from, 0.5);
        assert_eq!(
            middle.bounds.height,
            (from.bounds.height + to.bounds.height) * 0.5
        );
        assert_eq!(middle.bounds.y, (from.bounds.y + to.bounds.y) * 0.5);
        assert_eq!(
            middle.preview.height,
            middle.configuration.height + TAB_BAR_HEIGHT
        );
        // Switching again or closing midway must start exactly at this frame.
        let next = shape(280.0, 1.0);
        check(next.interpolate_from(middle, 0.0), middle);
        let closing = shape(280.0, 0.0);
        check(closing.interpolate_from(middle, 0.0), middle);
        check(closing.interpolate_from(middle, 1.0), closing);
        for progress in [0.0, 0.25, 0.5, 1.0] {
            check(from.interpolate_from(from, progress), from);
        }
    }
    #[test]
    fn clipped_ribbons_keep_tiles_but_only_offer_visible_drop_slots() {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            for standalone in [false, true] {
                for (width, height) in [(59.0, 75.0), (81.0, 51.0)] {
                    let layout = tile_layout(width, height, axis, 30, standalone);
                    let clip = Bounds {
                        width,
                        height,
                        ..Bounds::default()
                    };
                    assert_eq!(layout.tiles.len(), 30);
                    assert!(layout.tiles.iter().any(|b| b.intersection(clip).is_none()));
                    let mut targets = std::collections::BTreeSet::new();
                    for x in 0..width as usize {
                        for y in 0..height as usize {
                            let point = [x as f32 + 0.5, y as f32 + 0.5];
                            let hint = layout.drop_slot(point, width, height);
                            if layout.grip.is_some_and(|b| b.contains(point[0], point[1])) {
                                assert!(hint.is_none());
                            }
                            if let Some((index, line)) = hint {
                                assert_eq!(line.intersection(clip), Some(line));
                                assert!(layout.tiles[index.min(29)].intersection(clip).is_some());
                                targets.insert(index);
                            }
                        }
                    }
                    assert!(!targets.is_empty());
                    assert!(targets.len() < 31);
                    let expanded = tile_layout(900.0, 900.0, axis, 30, standalone);
                    for (index, line) in expanded.insertion.iter().enumerate() {
                        let point = [line.x + line.width * 0.5, line.y + line.height * 0.5];
                        assert_eq!(expanded.drop_slot(point, 900.0, 900.0).unwrap().0, index);
                    }
                }
            }
        }
    }

    #[test]
    fn hidden_slots_are_not_clamped_into_the_last_visible_row() {
        let layout = tile_layout(36.0, 36.0, Axis::Horizontal, 12, true);
        let (index, line) = layout.drop_slot([1.0, 35.5], 36.0, 36.0).unwrap();
        assert_eq!(index, 0);
        assert_eq!(line.y, 0.0);
        // The next row is still hidden even if its marker straddles the clip.
        let layout = tile_layout(80.0, 38.0, Axis::Horizontal, 12, true);
        assert_eq!(layout.drop_slot([1.0, 37.5], 80.0, 38.0).unwrap().0, 0);
    }

    #[test]
    fn exhausted_workspace_ids_fail_atomically() {
        let mut layout = DockLayout {
            next_id: u32::MAX,
            ..DockLayout::default()
        };
        layout.validate().unwrap();
        let before = layout.clone();
        assert!(
            layout
                .move_panel(
                    [1200.0, 900.0],
                    Panel::Toolbar,
                    DockTarget::Edge {
                        edge: Edge::Bottom,
                        outer: true
                    }
                )
                .is_err()
        );
        assert_eq!(layout, before);
    }
    const VIEWPORT: [f32; 2] = [1200.0, 900.0];
    #[test]
    fn side_drops_preserve_widths_while_vertical_drops_share_height() {
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let mut layout = DockLayout::default();
            layout.bands[0].extent = 272.0;
            let before = layout.workspace(1200.0, 900.0, 48.0, 28.0);
            layout
                .move_panel(
                    VIEWPORT,
                    Panel::Brushes,
                    DockTarget::Split { group: 8, edge },
                )
                .unwrap();
            let after = layout.workspace(1200.0, 900.0, 48.0, 28.0);
            let moved = group(&after, Panel::Brushes);
            let target = group(&after, Panel::Layers);
            if edge.axis() == Axis::Horizontal {
                assert!((moved.width - group(&before, Panel::Brushes).width).abs() < 0.01);
                assert!((target.width - group(&before, Panel::Layers).width).abs() < 0.01);
                assert_eq!(moved.height, target.height);
                layout
                    .move_panel(VIEWPORT, Panel::Sizes, DockTarget::Split { group: 8, edge })
                    .unwrap();
                let nested = layout.workspace(1200.0, 900.0, 48.0, 28.0);
                assert!((group(&nested, Panel::Brushes).width - moved.width).abs() < 0.01);
                assert!((group(&nested, Panel::Layers).width - target.width).abs() < 0.01);
            } else {
                assert_eq!(moved.width, target.width);
                assert_eq!(moved.height, target.height);
                assert_eq!(
                    moved.height + target.height + 6.0,
                    group(&before, Panel::Layers).height
                );
            }
        }
        for viewport in [[480.0, 480.0], [700.0, 900.0]] {
            let mut layout = DockLayout::default();
            let before = layout.clone();
            assert!(
                layout
                    .move_panel(
                        viewport,
                        Panel::Sizes,
                        DockTarget::Split {
                            group: 5,
                            edge: Edge::Right
                        }
                    )
                    .is_err()
            );
            assert_eq!(
                layout, before,
                "an impossible width-preserving drop is atomic"
            );
        }
    }
    #[test]
    fn group_grip_moves_all_tabs_and_preserves_active_order_and_identity() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Toolbar,
                DockTarget::Tab {
                    group: 8,
                    index: None,
                },
            )
            .unwrap();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Brushes,
                DockTarget::Tab {
                    group: 8,
                    index: None,
                },
            )
            .unwrap();
        layout.select_tab(8, Panel::Toolbar).unwrap();
        let original = layout.node_mut(8).unwrap().clone();
        let before = layout.clone();
        assert!(
            layout
                .move_item(
                    VIEWPORT,
                    DockItem::Group { group: 8 },
                    DockTarget::Split {
                        group: 8,
                        edge: Edge::Left
                    }
                )
                .is_err()
        );
        assert_eq!(layout, before);
        layout
            .move_item(
                VIEWPORT,
                DockItem::Group { group: 8 },
                DockTarget::Edge {
                    edge: Edge::Bottom,
                    outer: false,
                },
            )
            .unwrap();
        assert_eq!(layout.bands.last().unwrap().root, original);
        assert_eq!(
            layout.bands.last().unwrap().extent,
            252.0,
            "a mixed tab group is not a thin tool ribbon"
        );
        layout
            .move_item(
                VIEWPORT,
                DockItem::Group { group: 8 },
                DockTarget::Split {
                    group: 6,
                    edge: Edge::Top,
                },
            )
            .unwrap();
        assert_eq!(*layout.node_mut(8).unwrap(), original);
        layout
            .move_item(
                VIEWPORT,
                DockItem::Group { group: 8 },
                DockTarget::Tab {
                    group: 6,
                    index: Some(0),
                },
            )
            .unwrap();
        assert!(
            matches!(layout.node_mut(6).unwrap(), DockNode::Tabs {panels, active: Panel::Toolbar, ..} if *panels == [Panel::Layers, Panel::Toolbar, Panel::Brushes, Panel::Sizes])
        );
    }
    #[test]
    fn square_tiles_wrap_across_strip_and_tabbed_strips_have_no_grip() {
        for (axis, width, height) in [
            (Axis::Horizontal, 500.0, TILE_SIZE),
            (Axis::Vertical, TILE_SIZE, 500.0),
        ] {
            let layout = tile_layout(width, height, axis, 6, true);
            assert_eq!((layout.tiles[0].x, layout.tiles[0].y), (0.0, 0.0));
            assert!(layout.grip.is_some());
            let grip = layout.grip.unwrap();
            assert_eq!(
                (grip.width, grip.height),
                if axis == Axis::Horizontal {
                    (20.0, 24.0)
                } else {
                    (24.0, 20.0)
                }
            );
            assert_eq!(
                if axis == Axis::Horizontal {
                    grip.x + grip.width
                } else {
                    grip.y + grip.height
                },
                if axis == Axis::Horizontal {
                    width
                } else {
                    height
                }
            );
            assert!(
                layout
                    .tiles
                    .iter()
                    .all(|b| b.width == 36.0 && b.height == 36.0)
            );
            assert!(
                layout
                    .tiles
                    .windows(2)
                    .all(|p| if axis == Axis::Horizontal {
                        p[0].y == p[1].y
                    } else {
                        p[0].x == p[1].x
                    })
            );
        }
        let rows = tile_layout(500.0, 82.0, Axis::Horizontal, 6, true);
        assert_eq!(rows.tiles[0].x, rows.tiles[3].x);
        assert_eq!(rows.tiles[0].y, rows.tiles[2].y);
        let columns = tile_layout(82.0, 500.0, Axis::Vertical, 6, false);
        assert!(columns.grip.is_none());
        assert_eq!((columns.tiles[0].x, columns.tiles[0].y), (4.0, 4.0));
        assert_eq!(columns.tiles[0].y, columns.tiles[1].y);
        assert_eq!(columns.tiles[0].x, columns.tiles[2].x);
    }
    #[test]
    fn standalone_ribbons_wrap_grow_and_unwrap_without_changing_saved_extent() {
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let mut layout = DockLayout::default();
            layout.bands.retain(|b| b.id == 1);
            layout.bands[0].edge = edge;
            let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
            for (length, cross) in [
                (58.0, 226.0),
                (96.0, 112.0),
                (134.0, 74.0),
                (247.0, 74.0),
                (248.0, 36.0),
                (500.0, 36.0),
            ] {
                let viewport = if horizontal {
                    [length, 800.0]
                } else {
                    [800.0, length]
                };
                let resolved = layout.resolve(viewport[0], viewport[1]);
                let g = &resolved.groups[0];
                assert_eq!(
                    if horizontal {
                        g.bounds.height
                    } else {
                        g.bounds.width
                    },
                    cross
                );
                let tiles = tile_layout(
                    g.bounds.width,
                    g.bounds.height,
                    g.axis,
                    TOOL_TILE_COUNT,
                    true,
                );
                let grip = tiles.grip.unwrap();
                for tile in &tiles.tiles {
                    assert!(tile.x >= 0.0 && tile.y >= 0.0);
                    assert!(
                        tile.x + tile.width <= g.bounds.width
                            && tile.y + tile.height <= g.bounds.height
                    );
                    assert!(if horizontal {
                        tile.x + tile.width + 2.0 <= grip.x
                    } else {
                        tile.y + tile.height + 2.0 <= grip.y
                    });
                }
                let d = &resolved.dividers[0];
                layout
                    .resize(
                        d.id,
                        [
                            d.bounds.x + d.bounds.width * 0.5,
                            d.bounds.y + d.bounds.height * 0.5,
                        ],
                        viewport,
                    )
                    .unwrap();
                assert_eq!(layout.bands[0].extent, TILE_SIZE + 6.0);
            }
        }
    }
    #[test]
    fn nested_ribbons_grow_and_tabbed_ribbons_keep_one_visible_lane() {
        for split_axis in [Axis::Horizontal, Axis::Vertical] {
            let mut layout = DockLayout::default();
            layout.bands.retain(|b| b.id == 1);
            layout.bands[0].root = DockNode::Split {
                id: 9,
                axis: split_axis,
                fraction: 0.5,
                first: Box::new(DockNode::Tabs {
                    id: 2,
                    panels: vec![Panel::Toolbar],
                    active: Panel::Toolbar,
                }),
                second: Box::new(DockNode::Tabs {
                    id: 10,
                    panels: vec![Panel::Brushes],
                    active: Panel::Brushes,
                }),
            };
            let resolved = layout.resolve(200.0, 800.0);
            let b = group(&resolved, Panel::Toolbar);
            let tiles = tile_layout(b.width, b.height, Axis::Horizontal, TOOL_TILE_COUNT, true);
            assert!(tiles.tiles.iter().all(
                |t| t.y + t.height <= b.height && t.x + t.width + 2.0 <= tiles.grip.unwrap().x
            ));
        }
        let mut layout = DockLayout::default();
        layout.bands.retain(|b| b.id == 1);
        layout.bands[0].root = DockNode::Tabs {
            id: 2,
            panels: vec![Panel::Toolbar, Panel::Brushes],
            active: Panel::Toolbar,
        };
        assert_eq!(
            group(&layout.resolve(100.0, 800.0), Panel::Toolbar).height,
            TAB_BAR_HEIGHT + TILE_SIZE + 8.0
        );
        let resolved = layout.resolve(100.0, 800.0);
        let g = &resolved.groups[0];
        let first = g.tiles.as_ref().unwrap().tiles[0];
        assert!(first.y + first.height <= g.bounds.height - TAB_BAR_HEIGHT);
        // Selecting another tab must not collapse the ribbon's reserved lane.
        layout.select_tab(2, Panel::Brushes).unwrap();
        assert_eq!(layout.resolve(100.0, 800.0).groups[0].bounds, g.bounds);
    }
    #[test]
    fn toolbar_and_status_stay_between_sides_and_above_bottom() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Sizes,
                DockTarget::Edge {
                    edge: Edge::Bottom,
                    outer: false,
                },
            )
            .unwrap();
        let r = layout.workspace(1200.0, 900.0, 48.0, 28.0);
        let toolbar = group(&r, Panel::Toolbar);
        let bottom = group(&r, Panel::Sizes);
        assert_eq!(toolbar.y, 48.0, "header padding must not be counted twice");
        assert_eq!(toolbar.x, r.status.x);
        assert_eq!(toolbar.width, r.status.width);
        assert!(r.status.y + r.status.height <= bottom.y);
        assert_eq!(r.work_area.y + r.work_area.height, r.status.y);
    }
    #[test]
    fn surface_coordinates_do_not_feed_back_during_resize() {
        let mut layout = DockLayout::default();
        let d = layout
            .workspace(1200.0, 900.0, 48.0, 28.0)
            .dividers
            .into_iter()
            .find(|d| d.id == 3)
            .unwrap();
        let pointer = [d.bounds.x + 1.0, d.bounds.y + 20.0];
        let drag = ResizeDrag::new(pointer, d.bounds);
        for dx in [0.0, 8.0, 16.0, 32.0, 32.0, 64.0, 64.0, 0.0] {
            layout
                .resize_workspace(
                    3,
                    drag.position([pointer[0] + dx, pointer[1]]),
                    [1200.0, 900.0],
                )
                .unwrap();
            assert_eq!(layout.bands[0].extent, 232.0 + dx);
        }
    }
    #[test]
    fn vertical_drop_zones_cover_twenty_percent_but_not_the_tab_bar() {
        let layout = DockLayout::default().workspace(1200.0, 900.0, 48.0, 28.0);
        let b = group(&layout, Panel::Layers);
        for (fraction, edge) in [
            (0.01, Some(Edge::Top)),
            (0.19, Some(Edge::Top)),
            (0.21, None),
            (0.79, None),
            (0.81, Some(Edge::Bottom)),
            (0.99, Some(Edge::Bottom)),
        ] {
            let hint = layout.drop_hint(
                b.x + b.width * 0.5,
                b.y + TAB_BAR_HEIGHT + (b.height - TAB_BAR_HEIGHT) * fraction,
                &[],
            );
            assert_eq!(
                hint.target,
                match edge {
                    Some(edge) => DockTarget::Split { group: 8, edge },
                    None => DockTarget::Tab {
                        group: 8,
                        index: None
                    },
                }
            );
        }
        assert!(matches!(
            layout.drop_hint(b.x + 40.0, b.y + 10.0, &[]).target,
            DockTarget::Tab { .. }
        ));
    }
    #[test]
    fn append_preview_is_vertical_and_clamped_before_the_grip() {
        let layout = DockLayout::default().workspace(1200.0, 900.0, 48.0, 28.0);
        let group = layout.groups.iter().find(|g| g.id == 8).unwrap();
        let b = group.bounds;
        for width in [70.0, b.width + 200.0] {
            let tabs = [TabHit {
                group: 8,
                index: 0,
                bounds: Bounds {
                    width,
                    height: TAB_BAR_HEIGHT,
                    ..b
                },
            }];
            let hint = layout.drop_hint(b.x + b.width * 0.5, b.y + b.height * 0.5, &tabs);
            assert_eq!(
                hint.target,
                DockTarget::Tab {
                    group: 8,
                    index: None
                }
            );
            assert_eq!(hint.bounds.width, 3.0);
            assert_eq!(hint.bounds.height, TILE_SIZE);
            assert_eq!(hint.bounds.y, b.y);
            assert_eq!(hint.bounds.x, (b.x + width - 1.5).min(b.x + b.width - 23.0));
            let over_grip = layout.drop_hint(b.x + b.width - 10.0, b.y + 10.0, &tabs);
            assert_eq!(
                over_grip.target,
                DockTarget::Tab {
                    group: 8,
                    index: Some(1)
                }
            );
            assert_eq!(over_grip.bounds, hint.bounds);
        }
    }
    #[test]
    fn header_slots_take_priority_and_tabs_can_reorder_in_place() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Brushes,
                DockTarget::Tab {
                    group: 8,
                    index: None,
                },
            )
            .unwrap();
        let r = layout.workspace(1200.0, 900.0, 48.0, 28.0);
        let b = r.groups.iter().find(|g| g.id == 8).unwrap().bounds;
        let tabs = [
            TabHit {
                group: 8,
                index: 0,
                bounds: Bounds {
                    width: 70.0,
                    height: TAB_BAR_HEIGHT,
                    ..b
                },
            },
            TabHit {
                group: 8,
                index: 1,
                bounds: Bounds {
                    x: b.x + 70.0,
                    width: 90.0,
                    height: TAB_BAR_HEIGHT,
                    ..b
                },
            },
        ];
        let hint = r.drop_hint(b.x + 10.0, b.y + 5.0, &tabs);
        assert_eq!(
            hint.target,
            DockTarget::Tab {
                group: 8,
                index: Some(0)
            }
        );
        assert_eq!(hint.bounds.width, 3.0);
        layout
            .move_panel(VIEWPORT, Panel::Brushes, hint.target)
            .unwrap();
        assert!(
            matches!(layout.node_mut(8).unwrap(), DockNode::Tabs {panels, ..} if *panels == [Panel::Brushes, Panel::Layers])
        );
        layout
            .move_panel(
                VIEWPORT,
                Panel::Brushes,
                DockTarget::Tab {
                    group: 8,
                    index: None,
                },
            )
            .unwrap();
        assert!(
            matches!(layout.node_mut(8).unwrap(), DockNode::Tabs {panels, ..} if *panels == [Panel::Layers, Panel::Brushes])
        );
        assert_eq!(
            r.drop_hint(b.x + 100.0, b.y + b.height - 2.0, &tabs).target,
            DockTarget::Split {
                group: 8,
                edge: Edge::Bottom
            }
        );
    }
    #[test]
    fn workspace_insets_and_zen_proximity_share_panel_geometry() {
        let layout = DockLayout::default();
        let resolved = layout.workspace(1200.0, 900.0, 48.0, 28.0);
        assert_eq!(resolved.groups[0].bounds.x, 6.0);
        assert_eq!(resolved.groups[0].bounds.y, 48.0);
        assert!(resolved.near_chrome([600.0, 30.0], VIEWPORT, false));
        assert!(resolved.near_chrome([20.0, 450.0], VIEWPORT, false));
        assert!(resolved.near_chrome([600.0, 875.0], VIEWPORT, false));
        assert!(!resolved.near_chrome([600.0, 450.0], VIEWPORT, false));
        let mut hidden = layout;
        hidden.panels_visible = false;
        let resolved = hidden.workspace(1200.0, 900.0, 48.0, 28.0);
        assert!(resolved.groups.is_empty() && resolved.dividers.is_empty());
        assert!(!resolved.near_chrome([20.0, 450.0], VIEWPORT, false));
        assert!(!resolved.near_chrome([100.0, 450.0], VIEWPORT, false));
        assert_eq!(resolved.work_area.width, 1188.0);
    }
    #[test]
    fn zen_reveals_at_window_edges_but_hides_away_from_controls() {
        let resolved = DockLayout::default().workspace(1200.0, 900.0, 48.0, 28.0);
        for point in [[600.0, 80.0], [80.0, 450.0], [1120.0, 450.0]] {
            assert!(resolved.near_chrome(point, VIEWPORT, true));
        }
        for point in [
            [600.0, 81.0],
            [81.0, 450.0],
            [1119.0, 450.0],
            [600.0, 819.0],
        ] {
            assert!(!resolved.near_chrome(point, VIEWPORT, true));
        }
        // Near a ribbon/side panel, but not a window edge: remain visible if
        // already shown, never reveal merely by approaching the hidden tools.
        for point in [
            [600.0, 120.0],
            [600.0, 124.0],
            [270.0, 450.0],
            [936.0, 450.0],
        ] {
            assert!(!resolved.near_chrome(point, VIEWPORT, true));
            assert!(resolved.near_chrome(point, VIEWPORT, false));
        }
        for point in [[600.0, 125.0], [600.0, 450.0], [-1.0, 20.0]] {
            assert!(!resolved.near_chrome(point, VIEWPORT, true));
            assert!(!resolved.near_chrome(point, VIEWPORT, false));
        }
        // An empty bottom edge does not reveal, even directly over the HUD.
        assert!(!resolved.near_chrome([600.0, 899.0], VIEWPORT, true));
    }
    #[test]
    fn zen_reveal_edges_follow_docking_and_panel_visibility() {
        let center = [600.0, 450.0];
        let edges = [
            (Edge::Top, [600.0, 1.0]),
            (Edge::Bottom, [600.0, 899.0]),
            (Edge::Left, [1.0, 450.0]),
            (Edge::Right, [1199.0, 450.0]),
        ];
        for destination in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let mut layout = DockLayout::default();
            // Consolidate every tab into one group, then move it as a unit.
            for panel in [Panel::Toolbar, Panel::Sizes, Panel::Layers] {
                layout
                    .move_panel(
                        VIEWPORT,
                        panel,
                        DockTarget::Tab {
                            group: 5,
                            index: None,
                        },
                    )
                    .unwrap();
            }
            layout
                .move_item(
                    VIEWPORT,
                    DockItem::Group { group: 5 },
                    DockTarget::Edge {
                        edge: destination,
                        outer: true,
                    },
                )
                .unwrap();
            let resolved = layout.workspace(1200.0, 900.0, 48.0, 28.0);
            assert!(!resolved.near_chrome(center, VIEWPORT, true));
            for (edge, point) in edges {
                let enabled = edge == Edge::Top || edge == destination;
                assert_eq!(
                    resolved.near_chrome(point, VIEWPORT, true),
                    enabled,
                    "dock={destination:?}, edge={edge:?}"
                );
                if enabled {
                    assert!(
                        resolved.near_chrome(point, VIEWPORT, false),
                        "revealed chrome must not oscillate"
                    );
                }
            }
            layout.panels_visible = false;
            let resolved = layout.workspace(1200.0, 900.0, 48.0, 28.0);
            for (edge, point) in edges {
                assert_eq!(
                    resolved.near_chrome(point, VIEWPORT, true),
                    edge == Edge::Top
                );
            }
        }
    }
    fn group(layout: &ResolvedLayout, panel: Panel) -> Bounds {
        layout
            .groups
            .iter()
            .find(|g| g.active == panel)
            .unwrap()
            .bounds
    }
    #[test]
    fn corners_follow_explicit_band_priority() {
        let mut layout = DockLayout::default();
        let before = layout.resolve(1400.0, 900.0);
        assert!(group(&before, Panel::Toolbar).width < 1400.0);
        assert_eq!(group(&before, Panel::Layers).y, 0.0);
        layout.prioritize(1).unwrap();
        let after = layout.resolve(1400.0, 900.0);
        assert!(group(&after, Panel::Layers).y > 0.0);
        assert_eq!(group(&after, Panel::Toolbar).width, 1400.0);
    }
    #[test]
    fn divider_centers_roundtrip_for_every_edge_and_nested_split() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Sizes,
                DockTarget::Edge {
                    edge: Edge::Bottom,
                    outer: false,
                },
            )
            .unwrap();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Brushes,
                DockTarget::Split {
                    group: 8,
                    edge: Edge::Top,
                },
            )
            .unwrap();
        let viewport = [1600.0, 1000.0];
        for d in layout.resolve(viewport[0], viewport[1]).dividers {
            let before = layout.clone();
            let center = [
                d.bounds.x + d.bounds.width * 0.5,
                d.bounds.y + d.bounds.height * 0.5,
            ];
            layout.resize(d.id, center, viewport).unwrap();
            assert_eq!(layout, before, "zero-distance resize should not jump");
        }
        let before = layout.clone();
        assert!(layout.resize(99, [10.0; 2], viewport).is_err());
        assert!(layout.resize(1, [f32::NAN, 0.0], viewport).is_err());
        assert_eq!(layout, before);
        layout
            .move_panel(
                VIEWPORT,
                Panel::Toolbar,
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: true,
                },
            )
            .unwrap();
        assert_eq!(
            layout.bands[0].extent,
            TILE_SIZE + 6.0,
            "a side toolbar defaults to one square tile column"
        );
    }
    #[test]
    fn adjacent_right_docks_and_tabs_preserve_every_panel() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Sizes,
                DockTarget::Edge {
                    edge: Edge::Right,
                    outer: false,
                },
            )
            .unwrap();
        let resolved = layout.resolve(1600.0, 1000.0);
        let layers = group(&resolved, Panel::Layers);
        let sizes = group(&resolved, Panel::Sizes);
        assert!(sizes.x + sizes.width <= layers.x);
        layout
            .move_panel(
                VIEWPORT,
                Panel::Brushes,
                DockTarget::Tab {
                    group: 8,
                    index: None,
                },
            )
            .unwrap();
        let node = layout.node_mut(8).unwrap();
        assert!(
            matches!(node, DockNode::Tabs {panels, active: Panel::Brushes, ..} if panels.len() == 2)
        );
        layout.select_tab(8, Panel::Layers).unwrap();
        for panel in Panel::ALL {
            assert_eq!(
                layout
                    .resolve(1600.0, 1000.0)
                    .groups
                    .iter()
                    .filter(|g| g.panels.contains(&panel))
                    .count(),
                1
            );
        }
    }
    #[test]
    fn failed_move_is_atomic_and_small_viewports_never_overlap() {
        let mut layout = DockLayout::default();
        let before = layout.clone();
        assert!(
            layout
                .move_panel(
                    VIEWPORT,
                    Panel::Brushes,
                    DockTarget::Tab {
                        group: 999,
                        index: None
                    }
                )
                .is_err()
        );
        assert_eq!(before, layout);
        layout
            .move_panel(
                VIEWPORT,
                Panel::Sizes,
                DockTarget::Split {
                    group: 8,
                    edge: Edge::Right,
                },
            )
            .unwrap();
        for size in [0.0, 12.0, 360.0, 1400.0] {
            let out = layout.resolve(size, size);
            for g in out.groups {
                assert!(g.bounds.width >= 0.0 && g.bounds.height >= 0.0);
                assert!(g.bounds.x + g.bounds.width <= size + 0.001);
            }
            assert!(out.work_area.width >= 0.0 && out.work_area.height >= 0.0);
        }
        assert!(layout.resize(4, [f32::NAN, 0.0], [1400.0, 900.0]).is_err());
    }
}
