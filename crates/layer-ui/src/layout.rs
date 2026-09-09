//! Semantic docking topology. Coordinates are logical UI units, never pixels
//! belonging to the raster document. Earlier bands own shared corners.

use crate::{PanelConfig, PanelContent, TileStyle, ToolbarControl, ToolbarTile, WORKSPACE_SPACING};
use serde::{Deserialize, Serialize};

pub const TILE_SIZE: f32 = 36.0;
/// Six standard toolbar tiles, including their five two-pixel gaps.
pub const LAYERS_MIN_WIDTH: f32 = 6.0 * TILE_SIZE + 5.0 * 2.0;
pub const TAB_BAR_HEIGHT: f32 = TILE_SIZE;
const PANEL_GRIP_HEIGHT: f32 = 20.0;
/// Shared gesture distances in logical UI pixels, not preferences.
pub const WORKSPACE_PROXIMITY: f32 = 80.0;
const PANEL_SNAP_DISTANCE: f32 = WORKSPACE_PROXIMITY * 0.5;
#[cfg(test)]
const TOOL_TILE_COUNT: usize = crate::TOOLBAR_CONTROLS.len();

// Reserve 20px for the trailing grip and 2px between it and the last tile.
fn ribbon_lanes(length: f32, count: usize, along: f32) -> usize {
    let slots = (((length - 20.0) / (along + 2.0)).floor() as usize).max(1);
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

/// One tile allocator for native and web strips. Cross-axis resizing
/// adds lanes; standalone ribbons also wrap when their long axis is constrained.
/// Tabbed tools retain their content inset; standalone strips are flush.
pub fn tile_layout(
    width: f32,
    height: f32,
    axis: Axis,
    count: usize,
    standalone: bool,
    style: TileStyle,
) -> TileLayout {
    let horizontal = axis == Axis::Horizontal;
    let [tile_width, tile_height] = style.size();
    let (along_size, cross_size) = if horizontal {
        (tile_width, tile_height)
    } else {
        (tile_height, tile_width)
    };
    let cross = if horizontal { height } else { width };
    let padding = if standalone { 0.0 } else { 4.0 };
    let mut lanes = (((cross - padding * 2.0 + 2.0) / (cross_size + 2.0)).floor() as usize)
        .clamp(1, count.max(1));
    if standalone {
        let length = if horizontal { width } else { height };
        lanes = lanes.max(ribbon_lanes(length, count, along_size));
    }
    let slots = count.max(1).div_ceil(lanes);
    let grid = lanes as f32 * (cross_size + 2.0) - 2.0;
    let inset = ((cross - grid) * 0.5).max(padding);
    let tiles: Vec<_> = (0..count)
        .map(|i| {
            let (along, across) = if standalone {
                (i % slots, i / slots)
            } else {
                (i / lanes, i % lanes)
            };
            let along = padding + along as f32 * (along_size + 2.0);
            let across = inset + across as f32 * (cross_size + 2.0);
            Bounds {
                x: if horizontal { along } else { across },
                y: if horizontal { across } else { along },
                width: tile_width,
                height: tile_height,
            }
        })
        .collect();
    // The entire trailing strip is a handle, including beside the centered
    // dots. A 20px trailing extent keeps their inset aligned with tab grips.
    let grip = standalone.then_some(if horizontal {
        Bounds {
            x: width - 20.0,
            y: 0.0,
            width: 20.0,
            height,
        }
    } else {
        Bounds {
            x: 0.0,
            y: height - 20.0,
            width,
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
            width: tile_width.min(width),
            height: tile_height.min(height),
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
        #[serde(default)]
        tab_style: crate::TabStyle,
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
    pub(crate) fn group_for(&self, panel: Panel) -> Option<(u32, Panel)> {
        match self {
            Self::Tabs {
                id, panels, active, ..
            } => panels.contains(&panel).then_some((*id, *active)),
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
    fn find(&self, target: u32) -> Option<&Self> {
        if self.id() == target {
            return Some(self);
        }
        match self {
            Self::Split { first, second, .. } => first.find(target).or_else(|| second.find(target)),
            _ => None,
        }
    }
    fn remove(self, panel: Panel) -> Option<Self> {
        match self {
            Self::Tabs {
                id,
                mut panels,
                mut active,
                tab_style,
            } => {
                panels.retain(|item| *item != panel);
                if panels.is_empty() {
                    return None;
                }
                if active == panel {
                    active = panels[0];
                }
                Some(Self::Tabs {
                    id,
                    panels,
                    active,
                    tab_style,
                })
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
pub struct FloatingGroup {
    pub root: DockNode,
    pub position: [f32; 2],
    pub width: f32,
    /// Width inherited on tear-off, retained when manually resizing.
    #[serde(default)]
    pub default_width: Option<f32>,
    /// None sizes to the active content; resizing supplies an explicit height.
    pub height: Option<f32>,
    /// Default-size cycle and flow direction for a standalone floating toolbar.
    #[serde(default)]
    pub toolbar_layout: FloatingToolbarLayout,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingToolbarLayout {
    #[default]
    Compact,
    Vertical,
    Horizontal,
}
impl FloatingToolbarLayout {
    fn next(self) -> Self {
        match self {
            Self::Compact => Self::Vertical,
            Self::Vertical => Self::Horizontal,
            Self::Horizontal => Self::Compact,
        }
    }
    fn width(self, style: TileStyle, count: usize) -> f32 {
        match self {
            Self::Compact => style.floating_width(),
            Self::Vertical => style.size()[0],
            Self::Horizontal => count.max(1) as f32 * (style.size()[0] + 2.0) + 20.0,
        }
    }
}

/// Native measurements only. Rust owns sizing rules and resulting geometry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelMeasurement {
    pub panel: Panel,
    pub tab_width: f32,
    pub content_height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockLayout {
    /// Outermost first. Reordering changes corner ownership explicitly.
    pub bands: Vec<DockBand>,
    #[serde(default = "PanelConfig::defaults")]
    pub panels: Vec<PanelConfig>,
    #[serde(default)]
    pub floating: Vec<FloatingGroup>,
    /// Adding tabs opts a group into natural width; manual width resize opts out.
    #[serde(default)]
    pub fit_tab_groups: Vec<u32>,
    #[serde(skip)]
    pub measurements: Vec<PanelMeasurement>,
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
    Float {
        position: [f32; 2],
    },
    /// A new band adjacent to the center; `outer` grants corner priority.
    Edge {
        edge: Edge,
        outer: bool,
    },
    /// Dock alongside the complete sidebar, without splitting one of its panels.
    BesideBand {
        band: u32,
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
    pub fn interpolate_from(self, from: Self, progress: f32) -> Self {
        let p = progress.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * p;
        Self {
            x: mix(from.x, self.x),
            y: mix(from.y, self.y),
            width: mix(from.width, self.width),
            height: mix(from.height, self.height),
        }
    }
    pub fn distance_to(self, point: [f32; 2]) -> f32 {
        (point[0] - point[0].clamp(self.x, self.x + self.width))
            .hypot(point[1] - point[1].clamp(self.y, self.y + self.height))
    }
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
pub struct FloatingResizeHandle {
    pub edge: ResizeEdge,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResizeEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl ResizeEdge {
    pub fn cursor(self) -> &'static str {
        match self {
            Self::Left | Self::Right => "ew-resize",
            Self::Top | Self::Bottom => "ns-resize",
            Self::TopLeft | Self::BottomRight => "nwse-resize",
            Self::TopRight | Self::BottomLeft => "nesw-resize",
        }
    }
    fn handles(b: Bounds) -> Vec<FloatingResizeHandle> {
        const HIT: f32 = 6.0;
        let right = b.x + b.width;
        let bottom = b.y + b.height;
        [
            (Self::Left, [b.x - HIT, b.y, HIT, b.height]),
            (Self::Right, [right, b.y, HIT, b.height]),
            (Self::Top, [b.x, b.y - HIT, b.width, HIT]),
            (Self::Bottom, [b.x, bottom, b.width, HIT]),
            (Self::TopLeft, [b.x - HIT, b.y - HIT, HIT, HIT]),
            (Self::TopRight, [right, b.y - HIT, HIT, HIT]),
            (Self::BottomLeft, [b.x - HIT, bottom, HIT, HIT]),
            (Self::BottomRight, [right, bottom, HIT, HIT]),
        ]
        .into_iter()
        .map(|(edge, [x, y, width, height])| FloatingResizeHandle {
            edge,
            bounds: Bounds {
                x,
                y,
                width,
                height,
            },
        })
        .collect()
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
    /// Content-local trailing drag strip for a lone built-in panel with no tab.
    pub footer_grip: Option<Bounds>,
    pub floating: bool,
    pub resize_handles: Vec<FloatingResizeHandle>,
    /// Content-local geometry for the current tool ribbon (no host tab inset math).
    pub tiles: Option<TileLayout>,
}
impl GroupPlacement {
    /// Presentation-only interpolation; external resize targets follow the panel.
    pub fn interpolate_from(&mut self, from: Bounds, progress: f32) {
        self.bounds = self.bounds.interpolate_from(from, progress);
        if self.floating {
            self.resize_handles = ResizeEdge::handles(self.bounds);
        }
    }
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
    pub viewport: [f32; 2],
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
        Self {
            bounds: self.bounds.interpolate_from(from.bounds, progress),
            preview: self.preview.interpolate_from(from.preview, progress),
            configuration: self
                .configuration
                .interpolate_from(from.configuration, progress),
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
            .map(|b| &b.root)
            .chain(self.floating.iter().map(|f| &f.root))
            .find_map(|root| root.group_for(panel))
            .map(|(id, _)| id)
    }

    pub(crate) fn active_panel(&self, panel: Panel) -> Option<Panel> {
        self.bands
            .iter()
            .map(|b| &b.root)
            .chain(self.floating.iter().map(|f| &f.root))
            .find_map(|root| root.group_for(panel))
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
        let edge = self
            .bands
            .iter()
            .find(|b| b.root.group_for(panel).is_some())
            .map(|b| b.edge);
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
        let side = edge.is_none() || matches!(edge, Some(Edge::Left | Edge::Right));
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
        let config_left = edge == Some(Edge::Right)
            || ((edge.is_none() || !side) && docked.x + docked.width * 0.5 > viewport[0] * 0.5);
        let x = if config_left {
            docked.x + docked.width - width
        } else {
            docked.x
        };
        let y = if edge == Some(Edge::Bottom) {
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
            tab_style: crate::TabStyle::default(),
            id,
            panels: vec![panel],
            active: panel,
        };
        Self {
            panels: PanelConfig::defaults(),
            floating: Vec::new(),
            fit_tab_groups: Vec::new(),
            measurements: Vec::new(),
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
    /// Visible panels occur exactly once; hidden panels retain their registry entry.
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
        for floating in &self.floating {
            if !matches!(floating.root, DockNode::Tabs { .. })
                || !floating.position.into_iter().all(f32::is_finite)
                || !floating.width.is_finite()
                || floating.width <= 0.0
                || floating
                    .default_width
                    .is_some_and(|w| !w.is_finite() || w <= 0.0)
                || floating.height.is_some_and(|h| !h.is_finite() || h <= 0.0)
            {
                return Err("Invalid floating panel geometry".into());
            }
            node(&floating.root, &mut ids, &mut panels, 0)?;
        }
        for (i, group) in self.fit_tab_groups.iter().enumerate() {
            self.group_panels(*group)?;
            if self.fit_tab_groups[..i].contains(group) {
                return Err("Duplicate tab sizing identity".into());
            }
        }
        let mut tile_ids = std::collections::BTreeSet::new();
        let mut names = std::collections::BTreeSet::new();
        for (index, config) in self.panels.iter().enumerate() {
            config.validate()?;
            if self.panels[..index].iter().any(|p| p.id == config.id) {
                return Err("Duplicate panel configuration".into());
            }
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
            if let Some(index) = panels.iter().position(|id| *id == config.id) {
                panels.remove(index);
            }
        }
        if !panels.is_empty()
            || Panel::ALL
                .iter()
                .filter(|id| id.kind() == PanelKind::Content)
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
        match self.node(id) {
            Some(DockNode::Tabs { panels, .. }) => Ok(panels),
            _ => Err("Unknown tab group".into()),
        }
    }
    pub fn group_tab_style(&self, group: u32) -> Result<crate::TabStyle, String> {
        match self.node(group) {
            Some(DockNode::Tabs { tab_style, .. }) => Ok(*tab_style),
            _ => Err("Unknown tab group".into()),
        }
    }
    pub(crate) fn set_tab_style(
        &mut self,
        group: u32,
        style: crate::TabStyle,
    ) -> Result<(), String> {
        match self.node_mut(group) {
            Some(DockNode::Tabs { tab_style, .. }) => {
                *tab_style = style;
                Ok(())
            }
            _ => Err("Unknown tab group".into()),
        }
    }

    pub fn add_toolbar(
        &mut self,
        group: Option<u32>,
        name: &str,
        controls: &[ToolbarControl],
    ) -> Result<Panel, String> {
        let name = name.trim();
        self.validate_toolbar_name(name)?;
        if let Some(group) = group {
            self.group_panels(group)?;
        }
        let mut next = self.clone();
        let id = Panel::CustomToolbar(next.allocate()?);
        next.panels.push(PanelConfig {
            id,
            hide_tab: false,
            tile_style: crate::TileStyle::Small,
            content: PanelContent::Toolbar {
                name: name.into(),
                tiles: Vec::new(),
            },
        });
        if !controls.is_empty() {
            next.insert_tools(id, None, controls)?;
        }
        if let Some(group) = group {
            next.add_panel_to_group(id, group)?;
        } else {
            next.set_panel_visible(id, true)?;
        }
        next.validate()?;
        *self = next;
        Ok(id)
    }

    fn detach(&mut self, panels: &[Panel]) {
        self.bands = std::mem::take(&mut self.bands)
            .into_iter()
            .filter_map(|band| {
                let mut root = Some(band.root);
                for panel in panels {
                    root = root?.remove(*panel);
                }
                Some(DockBand {
                    root: root?,
                    ..band
                })
            })
            .collect();
        self.floating = std::mem::take(&mut self.floating)
            .into_iter()
            .filter_map(|mut floating| {
                let was_group =
                    matches!(&floating.root, DockNode::Tabs { panels, .. } if panels.len() > 1);
                let mut root = Some(floating.root);
                for panel in panels {
                    root = root?.remove(*panel);
                }
                floating.root = root?;
                if was_group
                    && let DockNode::Tabs { panels, active, .. } = &floating.root
                    && panels.len() == 1
                    && active.kind() == PanelKind::Tiles
                {
                    let width = self
                        .panel(*active)
                        .expect("validated toolbar")
                        .tile_style
                        .floating_width();
                    floating.width = width;
                    floating.default_width = Some(width);
                    floating.height = None;
                    floating.toolbar_layout = FloatingToolbarLayout::Compact;
                }
                Some(floating)
            })
            .collect();
        let groups = self
            .fit_tab_groups
            .iter()
            .copied()
            .filter(|g| {
                self.group_panels(*g)
                    .is_ok_and(|panels| panels.len() != 1 || panels[0].kind() != PanelKind::Tiles)
            })
            .collect();
        self.fit_tab_groups = groups;
    }

    pub fn set_panel_visible(&mut self, panel: Panel, visible: bool) -> Result<(), String> {
        self.panel(panel)?;
        if visible == self.panel_group(panel).is_some() {
            return Ok(());
        }
        if !visible {
            self.detach(&[panel]);
            return Ok(());
        }
        let mut next = self.clone();
        let group = next.allocate()?;
        let band = next.allocate()?;
        let edge = match panel {
            Panel::Brushes | Panel::Sizes => Edge::Left,
            Panel::Layers => Edge::Right,
            _ => Edge::Top,
        };
        next.bands.push(DockBand {
            id: band,
            edge,
            extent: if panel.kind() == PanelKind::Tiles {
                next.panel(panel)?.tile_style.size()[1] + WORKSPACE_SPACING
            } else {
                232.0
            },
            root: DockNode::Tabs {
                tab_style: crate::TabStyle::default(),
                id: group,
                panels: vec![panel],
                active: panel,
            },
        });
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn add_panel_to_group(&mut self, panel: Panel, group: u32) -> Result<(), String> {
        self.panel(panel)?;
        if self.group_panels(group)?.contains(&panel) {
            return Ok(());
        }
        if matches!(self.group_edge(group), Some(Edge::Top | Edge::Bottom)) {
            return Err("Top and bottom docks only support standalone toolbars".into());
        }
        self.detach(&[panel]);
        let Some(DockNode::Tabs { panels, active, .. }) = self.node_mut(group) else {
            unreachable!()
        };
        panels.push(panel);
        *active = panel;
        let merged = panels.clone();
        for panel in merged {
            self.panel_mut(panel)?.hide_tab = false;
        }
        self.fit_tabs(group);
        Ok(())
    }

    pub(crate) fn group_edge(&self, group: u32) -> Option<Edge> {
        let panel = *self.group_panels(group).ok()?.first()?;
        self.bands
            .iter()
            .find(|b| b.root.group_for(panel).is_some())
            .map(|b| b.edge)
    }

    pub fn rename_toolbar(&mut self, panel: Panel, name: &str) -> Result<(), String> {
        self.check_toolbar_name(name, Some(panel))?;
        let PanelContent::Toolbar { name: old, .. } = &mut self.panel_mut(panel)?.content else {
            return Err("Built-in panels cannot be renamed".into());
        };
        *old = name.trim().into();
        Ok(())
    }

    pub fn duplicate_toolbar(&mut self, panel: Panel, name: &str) -> Result<Panel, String> {
        let original = self.panel(panel)?.clone();
        if panel.kind() != PanelKind::Tiles {
            return Err("Choose a toolbar".into());
        }
        let id = self.add_toolbar(
            self.panel_group(panel)
                .filter(|g| !matches!(self.group_edge(*g), Some(Edge::Top | Edge::Bottom))),
            name,
            &original
                .tiles()
                .iter()
                .map(|t| t.control)
                .collect::<Vec<_>>(),
        )?;
        let config = self.panel_mut(id)?;
        config.tile_style = original.tile_style;
        Ok(id)
    }

    pub fn delete_toolbar(&mut self, panel: Panel) -> Result<(), String> {
        self.panel(panel)?;
        if panel.kind() != PanelKind::Tiles {
            return Err("Built-in panels can be hidden, not deleted".into());
        }
        self.detach(&[panel]);
        self.panels.retain(|p| p.id != panel);
        Ok(())
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
    pub fn reset_docking(&mut self) -> Result<(), String> {
        let mut next = self.clone();
        let defaults = Self::default();
        next.bands = defaults.bands;
        next.floating.clear();
        next.fit_tab_groups.clear();
        let tools: Vec<_> = self
            .panels
            .iter()
            .filter(|p| p.id.kind() == PanelKind::Tiles)
            .map(|p| p.id)
            .collect();
        if !tools.contains(&Panel::Toolbar) {
            next.bands.retain(|b| b.id != 1);
        }
        for panel in tools {
            next.set_panel_visible(panel, true)?;
        }
        next.validate()?;
        *self = next;
        Ok(())
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
    pub(crate) fn node(&self, id: u32) -> Option<&DockNode> {
        self.bands
            .iter()
            .map(|b| &b.root)
            .chain(self.floating.iter().map(|f| &f.root))
            .find_map(|root| root.find(id))
    }
    fn node_mut(&mut self, id: u32) -> Option<&mut DockNode> {
        self.bands
            .iter_mut()
            .map(|b| &mut b.root)
            .chain(self.floating.iter_mut().map(|f| &mut f.root))
            .find_map(|root| root.find_mut(id))
    }
    fn fit_tabs(&mut self, group: u32) {
        if !self.fit_tab_groups.contains(&group) {
            self.fit_tab_groups.push(group);
        }
        if let Some(f) = self.floating.iter_mut().find(|f| f.root.id() == group) {
            f.height = None;
        }
    }
    fn tab_width(&self, group: u32) -> f32 {
        if !self.fit_tab_groups.contains(&group) {
            return 0.0;
        }
        self.group_panels(group)
            .map(|panels| {
                if let [panel] = panels
                    && (panel.kind() == PanelKind::Tiles
                        || self.panel(*panel).is_ok_and(|p| p.hide_tab))
                {
                    return 0.0;
                }
                panels
                    .iter()
                    .map(|p| {
                        self.measurements
                            .iter()
                            .find(|m| m.panel == *p)
                            .map_or(0.0, |m| m.tab_width)
                    })
                    .sum::<f32>()
                    + 20.0
                    + 2.0 * panels.len().saturating_sub(1) as f32
            })
            .unwrap_or(0.0)
    }

    fn group_min_width(&self, group: u32) -> f32 {
        let content = if self
            .group_panels(group)
            .is_ok_and(|panels| panels.contains(&Panel::Layers))
        {
            LAYERS_MIN_WIDTH
        } else {
            0.0
        };
        self.tab_width(group).max(content)
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
        if let DockTarget::Split { group, .. } = target
            && self.floating.iter().any(|f| f.root.id() == group)
        {
            return Err("Floating groups accept tabs, not docking splits".into());
        }
        if let DockTarget::Float { position } = target
            && !position.into_iter().all(f32::is_finite)
        {
            return Err("Invalid floating panel position".into());
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
                    .map(|b| &b.root)
                    .chain(next.floating.iter().map(|f| &f.root))
                    .find_map(|root| find_tab(root, panel))
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
        let previous_float = self
            .floating
            .iter()
            .find(|f| f.root.id() == source_group && whole)
            .cloned();
        let was_fitted = self.fit_tab_groups.contains(&source_group) && whole;
        next.detach(&moving);
        let tiles = moving.len() == 1 && selected.kind() == PanelKind::Tiles;
        let dock_edge = match &target {
            DockTarget::Edge { edge, .. } => Some(*edge),
            DockTarget::BesideBand { band } => {
                next.bands.iter().find(|b| b.id == *band).map(|b| b.edge)
            }
            DockTarget::Tab { group, .. } | DockTarget::Split { group, .. } => {
                next.group_edge(*group)
            }
            _ => None,
        };
        if matches!(dock_edge, Some(Edge::Top | Edge::Bottom))
            && (!tiles || matches!(target, DockTarget::Tab { .. }))
        {
            return Err("Top and bottom docks only support standalone toolbars".into());
        }
        let source = before.groups.iter().find(|g| g.id == source_group);
        let tile_size = next.panel(selected)?.tile_style.size();
        let moved_width = if tiles {
            tile_size[0]
        } else {
            source.map(|g| g.bounds.width).unwrap_or(246.0)
        };
        let moving = DockNode::Tabs {
            id: moving_id,
            panels: moving,
            active: selected,
            tab_style: if whole {
                self.group_tab_style(source_group)?
            } else {
                crate::TabStyle::default()
            },
        };
        match target {
            DockTarget::Float { position } => {
                if let DockNode::Tabs { panels, .. } = &moving
                    && let [panel] = panels.as_slice()
                    && panel.kind() == PanelKind::Content
                    && previous_float.is_none()
                {
                    next.panel_mut(*panel)?.hide_tab = true;
                }
                let width = previous_float.as_ref().map(|f| f.width).unwrap_or_else(|| {
                    if tiles {
                        next.panel(selected).unwrap().tile_style.floating_width()
                    } else {
                        source.map_or(232.0, |g| g.bounds.width)
                    }
                });
                next.floating.push(FloatingGroup {
                    root: moving,
                    width,
                    toolbar_layout: previous_float
                        .as_ref()
                        .map_or(FloatingToolbarLayout::Compact, |f| f.toolbar_layout),
                    default_width: previous_float
                        .as_ref()
                        .and_then(|f| f.default_width)
                        .or(Some(width)),
                    position: [
                        position[0] - width * 0.5,
                        position[1] - TAB_BAR_HEIGHT * 0.5,
                    ],
                    height: previous_float.and_then(|f| f.height),
                });
                if was_fitted {
                    next.fit_tabs(moving_id);
                }
            }
            DockTarget::Tile { .. } => unreachable!(),
            DockTarget::Edge { .. } | DockTarget::BesideBand { .. } => {
                let (edge, index) = match target {
                    DockTarget::Edge { edge, outer } => {
                        (edge, if outer { 0 } else { next.bands.len() })
                    }
                    DockTarget::BesideBand { band } => {
                        let index = next
                            .bands
                            .iter()
                            .position(|b| b.id == band)
                            .ok_or("The target sidebar no longer exists")?;
                        (next.bands[index].edge, index + 1)
                    }
                    _ => unreachable!(),
                };
                let id = next.allocate()?;
                let band = DockBand {
                    id,
                    edge,
                    extent: if edge.axis() == Axis::Horizontal {
                        moved_width + WORKSPACE_SPACING
                    } else if tiles {
                        tile_size[usize::from(edge.axis() == Axis::Vertical)] + WORKSPACE_SPACING
                    } else {
                        252.0
                    },
                    root: moving,
                };
                next.bands.insert(index, band);
                if was_fitted {
                    next.fit_tabs(moving_id);
                }
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
                let merged = panels.clone();
                for panel in merged {
                    next.panel_mut(panel)?.hide_tab = false;
                }
                next.fit_tabs(group);
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
                        resize_node_extent(
                            &mut band.root,
                            group,
                            Axis::Horizontal,
                            root_width,
                            grow,
                        );
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
                if was_fitted {
                    next.fit_tabs(moving_id);
                }
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
                    g.id != moving_id
                        && g.axis == Axis::Vertical
                        && g.bounds.width + 0.5 < width(&before, g.id)
                })
            {
                return Err("Not enough horizontal space to preserve panel widths".into());
            }
        }
        next.reclaim_removed_columns(self, &before);
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// A single multi-column section determines a sidebar's natural width;
    /// full-width rows follow it. Independent multi-column sections keep the
    /// shared width and let the surviving children fill their vacated space.
    pub(crate) fn reclaim_removed_columns(&mut self, before: &Self, geometry: &ResolvedLayout) {
        // Horizontal splits form one row, regardless of their binary nesting.
        // Vertical splits combine independent rows in the current column.
        // Return (original width, surviving width, split rows in this column).
        fn measure(
            node: &DockNode,
            retained: &DockNode,
            geometry: &ResolvedLayout,
            widths: &mut Vec<(u32, f32)>,
        ) -> (f32, Option<f32>, usize) {
            let result = match node {
                DockNode::Tabs { id, panels, .. } => {
                    let width = geometry
                        .groups
                        .iter()
                        .find(|g| g.id == *id)
                        .map(|g| g.bounds.width)
                        .unwrap_or(0.0);
                    let present = panels.iter().any(|p| {
                        retained
                            .group_for(*p)
                            .is_some_and(|(group, _)| group == *id)
                    });
                    (width, present.then_some(width), 0)
                }
                DockNode::Split {
                    axis,
                    first,
                    second,
                    ..
                } => {
                    let (old_a, a, sections_a) = measure(first, retained, geometry, widths);
                    let (old_b, b, sections_b) = measure(second, retained, geometry, widths);
                    if *axis == Axis::Horizontal {
                        let width = match (a, b) {
                            (Some(a), Some(b)) => Some(a + b + WORKSPACE_SPACING),
                            (a, b) => a.or(b),
                        };
                        (old_a + old_b + WORKSPACE_SPACING, width, 1)
                    } else {
                        let old = old_a.max(old_b);
                        let sections = sections_a + sections_b;
                        let width = if a.is_none() && b.is_none() {
                            None
                        } else if sections == 1 {
                            // Full-width rows follow the only split row. If
                            // that entire row disappears, keep the old width.
                            Some(if sections_a == 1 { a } else { b }.unwrap_or(old))
                        } else {
                            Some(old)
                        };
                        (old, width, sections)
                    }
                }
            };
            if let Some(width) = result.1 {
                widths.push((node.id(), width));
            }
            result
        }
        fn reweight(node: &mut DockNode, widths: &[(u32, f32)]) -> Option<f32> {
            match node {
                DockNode::Tabs { .. } => (),
                DockNode::Split {
                    axis,
                    fraction,
                    first,
                    second,
                    ..
                } => {
                    let a = reweight(first, widths)?;
                    let b = reweight(second, widths)?;
                    if *axis == Axis::Horizontal {
                        *fraction = a / (a + b).max(1.0);
                    }
                }
            }
            widths
                .iter()
                .find(|(id, _)| *id == node.id())
                .map(|(_, w)| *w)
        }
        for band in &mut self.bands {
            if band.edge.axis() != Axis::Horizontal {
                continue;
            }
            let Some(old) = before.bands.iter().find(|b| b.id == band.id) else {
                continue;
            };
            // Relocating into another part of this band can add a new split;
            // its explicit docking allocation must not be overwritten here.
            let old_groups: Vec<_> = before
                .panels
                .iter()
                .filter_map(|p| old.root.group_for(p.id).map(|g| g.0))
                .collect();
            if self
                .panels
                .iter()
                .filter_map(|p| band.root.group_for(p.id))
                .any(|(id, _)| !old_groups.contains(&id))
            {
                continue;
            }
            let mut widths = Vec::new();
            let (was, now, _) = measure(&old.root, &band.root, geometry, &mut widths);
            if let Some(now) = now
                && now + 0.5 < was
            {
                band.extent = now + WORKSPACE_SPACING;
                reweight(&mut band.root, &widths);
            }
        }
    }

    /// Select a tab, returning whether the active tab changed.
    pub fn select_tab(&mut self, group: u32, panel: Panel) -> Result<bool, String> {
        if let Some(DockNode::Tabs { panels, active, .. }) = self.node_mut(group)
            && panels.contains(&panel)
        {
            let changed = *active != panel;
            *active = panel;
            if changed
                && let Some(floating) = self.floating.iter_mut().find(|f| f.root.id() == group)
            {
                floating.height = None;
            }
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
        if d.axis == Axis::Horizontal {
            // A deliberate width adjustment takes over from tab auto-sizing.
            let affected: Vec<_> = resolved
                .groups
                .iter()
                .filter(|g| {
                    d.parent.contains(
                        g.bounds.x + g.bounds.width / 2.0,
                        g.bounds.y + g.bounds.height / 2.0,
                    )
                })
                .map(|g| g.id)
                .collect();
            self.fit_tab_groups.retain(|g| !affected.contains(g));
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
            viewport: [width, height],
            tab_bar_height: TAB_BAR_HEIGHT,
            reveal_edges: vec![Edge::Top],
            work_area: remaining,
            status: Bounds::default(),
            groups: Vec::new(),
            dividers: Vec::new(),
        };
        for (band_index, band) in self.bands.iter().enumerate() {
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
            let ribbon_min =
                ribbon_cross_min(&band.root, axis, length, self).max(if axis == Axis::Vertical {
                    tab_min_width(&band.root, self)
                } else {
                    0.0
                });
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
            let reserved_width = if axis == Axis::Vertical {
                self.bands[band_index + 1..]
                    .iter()
                    .filter(|b| matches!(b.edge, Edge::Left | Edge::Right))
                    .map(|b| tab_min_width(&b.root, self) + WORKSPACE_SPACING)
                    .sum::<f32>()
            } else {
                0.
            };
            let limit = (available - reserved_width - 64.0)
                .max(minimum)
                .min(available)
                .max(0.0);
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
        result.viewport = [width, height];
        let offset = |b: &mut Bounds| {
            b.x += WORKSPACE_SPACING;
            b.y += top;
        };
        offset(&mut result.work_area);
        let hud_height = bottom.min(result.work_area.height);
        result.status = Bounds {
            x: result.work_area.x,
            y: result.work_area.y + result.work_area.height - hud_height,
            width: result.work_area.width,
            height: hud_height,
        };
        result.work_area.height -= hud_height;
        for group in &mut result.groups {
            offset(&mut group.bounds);
        }
        for divider in &mut result.dividers {
            offset(&mut divider.bounds);
            offset(&mut divider.parent);
        }
        for floating in &self.floating {
            let DockNode::Tabs {
                id, panels, active, ..
            } = &floating.root
            else {
                continue;
            };
            let config = self.panel(*active).expect("validated floating panel");
            let toolbar = panels.len() == 1 && active.kind() == PanelKind::Tiles;
            let axis = if toolbar && floating.toolbar_layout == FloatingToolbarLayout::Horizontal {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            let max_width = (width - WORKSPACE_SPACING * 2.0).max(1.0);
            let max_height = (height - top - WORKSPACE_SPACING).max(1.0);
            let mut width = floating.width.max(self.group_min_width(*id)).min(max_width);
            let natural = if toolbar {
                let [tile_width, tile_height] = config.tile_style.size();
                let count = config.tiles().len().max(1);
                if axis == Axis::Horizontal {
                    ribbon_lanes(width, count, tile_width) as f32 * (tile_height + 2.0) - 2.0
                } else {
                    // Long vertical defaults wrap only when the viewport cannot
                    // contain one column. Compact grids use the same allocator.
                    if floating.height.is_none() {
                        width = width
                            .max(
                                ribbon_lanes(max_height, count, tile_height) as f32
                                    * (tile_width + 2.0)
                                    - 2.0,
                            )
                            .min(max_width);
                    }
                    let columns = ((width + 2.0) / (tile_width + 2.0)).floor().max(1.0) as usize;
                    count.div_ceil(columns) as f32 * (tile_height + 2.0) + 20.0
                }
            } else if active.kind() == PanelKind::Tiles {
                let [w, h] = config.tile_style.size();
                let columns = ((width - 8.0 + 2.0) / (w + 2.0)).floor().max(1.0) as usize;
                config.tiles().len().max(1).div_ceil(columns) as f32 * (h + 2.0) - 2.0
                    + 8.0
                    + TAB_BAR_HEIGHT
            } else {
                (self
                    .measurements
                    .iter()
                    .find(|m| m.panel == *active)
                    .map_or(320.0, |m| m.content_height)
                    + if panels.len() == 1 && config.hide_tab {
                        PANEL_GRIP_HEIGHT
                    } else {
                        TAB_BAR_HEIGHT
                    })
                .min(height * 0.75)
            };
            let height = floating
                .height
                .unwrap_or(natural)
                .max(TAB_BAR_HEIGHT)
                .min(max_height);
            let bounds = Bounds {
                x: floating.position[0]
                    .clamp(WORKSPACE_SPACING, WORKSPACE_SPACING + max_width - width),
                y: floating.position[1].clamp(top, top + max_height - height),
                width,
                height,
            };
            resolve_node(&floating.root, bounds, axis, self, &mut result);
            let group = result.groups.last_mut().unwrap();
            group.floating = true;
            group.resize_handles = ResizeEdge::handles(bounds);
        }
        result
    }

    /// Move without changing size or replacing the group; native hosts retain
    /// their widgets and pointer grab throughout the gesture.
    pub fn move_floating(
        &mut self,
        group: u32,
        position: [f32; 2],
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if !viewport.into_iter().all(|v| v.is_finite() && v > 0.0)
            || !position.into_iter().all(f32::is_finite)
        {
            return Err("Invalid floating panel position".into());
        }
        let b = self
            .workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            )
            .groups
            .into_iter()
            .find(|g| g.id == group && g.floating)
            .ok_or("Unknown floating group")?
            .bounds;
        self.floating
            .iter_mut()
            .find(|f| f.root.id() == group)
            .unwrap()
            .position = [
            position[0].clamp(
                WORKSPACE_SPACING,
                (viewport[0] - WORKSPACE_SPACING - b.width).max(WORKSPACE_SPACING),
            ),
            position[1].clamp(
                crate::HEADER_HEIGHT,
                (viewport[1] - WORKSPACE_SPACING - b.height).max(crate::HEADER_HEIGHT),
            ),
        ];
        Ok(())
    }

    pub fn resize_floating(
        &mut self,
        group: u32,
        edge: ResizeEdge,
        start: Bounds,
        delta: [f32; 2],
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if !viewport.into_iter().all(|v| v.is_finite() && v > 0.0)
            || !delta.into_iter().all(f32::is_finite)
            || ![start.x, start.y, start.width, start.height]
                .into_iter()
                .all(f32::is_finite)
            || start.width <= 0.0
            || start.height <= 0.0
        {
            return Err("Invalid floating panel size".into());
        }
        self.fit_tab_groups.retain(|id| *id != group);
        let minimum_width = self.group_min_width(group).max(TILE_SIZE);
        let floating = self
            .floating
            .iter_mut()
            .find(|f| f.root.id() == group)
            .ok_or("Unknown floating group")?;
        let mut left = start.x;
        let mut right = start.x + start.width;
        let mut top = start.y;
        let mut bottom = start.y + start.height;
        if matches!(
            edge,
            ResizeEdge::Left | ResizeEdge::TopLeft | ResizeEdge::BottomLeft
        ) {
            left = (left + delta[0]).clamp(
                WORKSPACE_SPACING,
                (right - minimum_width).max(WORKSPACE_SPACING),
            );
        }
        if matches!(
            edge,
            ResizeEdge::Right | ResizeEdge::TopRight | ResizeEdge::BottomRight
        ) {
            right = (right + delta[0]).clamp(
                left + minimum_width,
                (viewport[0] - WORKSPACE_SPACING).max(left + minimum_width),
            );
        }
        if matches!(
            edge,
            ResizeEdge::Top | ResizeEdge::TopLeft | ResizeEdge::TopRight
        ) {
            top = (top + delta[1]).clamp(
                crate::HEADER_HEIGHT,
                (bottom - TAB_BAR_HEIGHT).max(crate::HEADER_HEIGHT),
            );
        }
        if matches!(
            edge,
            ResizeEdge::Bottom | ResizeEdge::BottomLeft | ResizeEdge::BottomRight
        ) {
            bottom = (bottom + delta[1]).clamp(
                top + TAB_BAR_HEIGHT,
                (viewport[1] - WORKSPACE_SPACING).max(top + TAB_BAR_HEIGHT),
            );
        }
        floating.position = [left, top];
        floating.width = right - left;
        floating.height = Some(bottom - top);
        self.fit_tab_groups.retain(|id| *id != group);
        Ok(())
    }

    /// Empty headers and grips accept double-clicks, but tab labels do not.
    /// Lone docked handles toggle built-in panel tabs or refit toolbars;
    /// floating handles also restore sizes and cycle toolbar layouts.
    pub fn panel_handle_target(&self, item: DockItem) -> Option<u32> {
        let group = match item {
            DockItem::Group { group } => group,
            DockItem::Panel { panel } if panel.kind() == PanelKind::Tiles => {
                self.panel_group(panel)?
            }
            _ => return None,
        };
        let lone = self.group_panels(group).ok()?.len() == 1;
        (lone
            || (matches!(item, DockItem::Group { .. })
                && self.floating.iter().any(|f| f.root.id() == group)))
        .then_some(group)
    }

    pub fn set_tile_style(
        &mut self,
        panel: Panel,
        style: TileStyle,
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if panel.kind() != PanelKind::Tiles {
            return Err("Choose a toolbar".into());
        }
        let old = self.panel(panel)?.tile_style;
        if old == style {
            return Ok(());
        }
        let before = self.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        self.panel_mut(panel)?.tile_style = style;
        let Some(group) = before
            .groups
            .iter()
            .find(|g| g.active == panel && !g.tabs_visible)
        else {
            return Ok(());
        };
        if group.floating {
            let mode = self
                .floating
                .iter()
                .find(|f| f.root.id() == group.id)
                .expect("resolved floating group")
                .toolbar_layout;
            return self.set_floating_default(group.id, mode);
        }
        let axis = if group.axis == Axis::Horizontal {
            Axis::Vertical
        } else {
            Axis::Horizontal
        };
        let cross = if axis == Axis::Horizontal {
            group.bounds.width
        } else {
            group.bounds.height
        };
        let dimension = usize::from(axis == Axis::Vertical);
        // Preserve manually expanded multi-lane ribbons. A one-lane ribbon
        // follows the new tile size in either direction, including shrinking.
        if cross + 2.0 >= (old.size()[dimension] + 2.0) * 2.0 {
            return Ok(());
        }
        self.resize_docked_group_cross(&before, group, style.size()[dimension]);
        Ok(())
    }

    /// Change the target ribbon's thickness without stretching side-by-side
    /// neighbors. The resolver supplies any extra lanes needed for overflow.
    fn resize_docked_group_cross(
        &mut self,
        before: &ResolvedLayout,
        group: &GroupPlacement,
        size: f32,
    ) {
        let axis = if group.axis == Axis::Horizontal {
            Axis::Vertical
        } else {
            Axis::Horizontal
        };
        let cross = if axis == Axis::Horizontal {
            group.bounds.width
        } else {
            group.bounds.height
        };
        let band = self
            .bands
            .iter_mut()
            .find(|b| b.root.group_for(group.active).is_some())
            .unwrap();
        let divider = before
            .dividers
            .iter()
            .find(|d| d.band && d.id == band.id)
            .unwrap();
        let extent = if axis == Axis::Horizontal {
            if divider.reversed {
                divider.parent.x + divider.parent.width - divider.bounds.x - divider.bounds.width
            } else {
                divider.bounds.x - divider.parent.x
            }
        } else if divider.reversed {
            divider.parent.y + divider.parent.height - divider.bounds.y - divider.bounds.height
        } else {
            divider.bounds.y - divider.parent.y
        };
        let delta = size - cross;
        resize_node_extent(&mut band.root, group.id, axis, extent, delta);
        band.extent = extent + delta + WORKSPACE_SPACING;
    }

    pub fn reset_floating_size(&mut self, group: u32) -> Result<(), String> {
        self.set_floating_default(group, FloatingToolbarLayout::Compact)
    }

    /// Docked lone panels toggle their tab; lone toolbars refit to their dock.
    /// Floating groups first restore a custom size. At default size, a lone
    /// toolbar cycles layouts and a lone built-in panel toggles its tab.
    pub fn double_click_panel_handle(
        &mut self,
        group: u32,
        viewport: [f32; 2],
    ) -> Result<(), String> {
        let Some(floating) = self.floating.iter().find(|f| f.root.id() == group) else {
            if let [panel] = self.group_panels(group)? {
                if panel.kind() == PanelKind::Content {
                    let config = self.panel_mut(*panel)?;
                    config.hide_tab = !config.hide_tab;
                } else {
                    let before = self.workspace(
                        viewport[0],
                        viewport[1],
                        crate::HEADER_HEIGHT,
                        crate::STATUS_HEIGHT,
                    );
                    if let Some(g) = before.groups.iter().find(|g| g.id == group) {
                        let size = self.panel(*panel)?.tile_style.size()
                            [usize::from(g.axis == Axis::Horizontal)];
                        self.resize_docked_group_cross(&before, g, size);
                    }
                }
            }
            return Ok(());
        };
        let DockNode::Tabs { panels, active, .. } = &floating.root else {
            return Err("Invalid floating group".into());
        };
        let panel = *active;
        let lone = panels.len() == 1;
        let current = floating.toolbar_layout;
        let mut default = self.clone();
        default.set_floating_default(group, current)?;
        let size = |layout: &Self| {
            let g = layout
                .workspace(
                    viewport[0],
                    viewport[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                )
                .groups
                .into_iter()
                .find(|g| g.id == group)
                .unwrap();
            [g.bounds.width, g.bounds.height]
        };
        let at_default = size(self)
            .into_iter()
            .zip(size(&default))
            .all(|(a, b)| (a - b).abs() < 0.5);
        if lone && panel.kind() == PanelKind::Tiles {
            self.set_floating_default(
                group,
                if at_default {
                    current.next()
                } else {
                    FloatingToolbarLayout::Compact
                },
            )
        } else {
            self.reset_floating_size(group)?;
            if lone && at_default {
                let config = self.panel_mut(panel)?;
                config.hide_tab = !config.hide_tab;
            }
            Ok(())
        }
    }

    fn set_floating_default(
        &mut self,
        group: u32,
        layout: FloatingToolbarLayout,
    ) -> Result<(), String> {
        let floating = self
            .floating
            .iter()
            .find(|f| f.root.id() == group)
            .ok_or("Unknown floating group")?;
        let DockNode::Tabs { panels, active, .. } = &floating.root else {
            return Err("Invalid floating group".into());
        };
        let toolbar = panels.len() == 1 && active.kind() == PanelKind::Tiles;
        let width = if toolbar {
            let config = self.panel(*active)?;
            layout.width(config.tile_style, config.tiles().len())
        } else {
            floating.default_width.unwrap_or(floating.width)
        };
        let floating = self
            .floating
            .iter_mut()
            .find(|f| f.root.id() == group)
            .unwrap();
        floating.width = width;
        floating.height = None;
        floating.toolbar_layout = if toolbar {
            layout
        } else {
            FloatingToolbarLayout::Compact
        };
        if toolbar {
            self.fit_tab_groups.retain(|id| *id != group);
        } else {
            self.fit_tabs(group);
        }
        Ok(())
    }
}

impl ResolvedLayout {
    pub fn tile_drop_hint(&self, point: [f32; 2], config: &DockLayout) -> Option<DropHint> {
        for group in self.groups.iter().rev() {
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
    /// Hidden docks have no targets, including the otherwise bare screen edges.
    pub fn drop_hint(
        &self,
        x: f32,
        y: f32,
        tabs: &[TabHit],
        docks_visible: bool,
    ) -> Option<DropHint> {
        let screen = Bounds {
            width: self.viewport[0],
            height: self.viewport[1],
            ..Bounds::default()
        };
        if !screen.contains(x, y) {
            return None;
        }
        for group in self.groups.iter().rev() {
            if !docks_visible && !group.floating {
                continue;
            }
            let b = group.bounds;
            if !b.contains(x, y) {
                continue;
            }
            if group.tabs_visible && y < b.y + TAB_BAR_HEIGHT {
                let mut index = group.panels.len();
                for tab in tabs
                    .iter()
                    .filter(|t| t.group == group.id && x < b.x + b.width - 20.0)
                {
                    if x < tab.bounds.x + tab.bounds.width * 0.5 {
                        index = tab.index;
                        break;
                    }
                }
                return Some(DropHint {
                    target: DockTarget::Tab {
                        group: group.id,
                        index: Some(index),
                    },
                    bounds: tab_insertion_line(group, tabs, index),
                });
            }
            if group.floating {
                return Some(DropHint {
                    target: DockTarget::Tab {
                        group: group.id,
                        index: None,
                    },
                    bounds: tab_insertion_line(group, tabs, group.panels.len()),
                });
            }
            let body = if group.tabs_visible {
                Bounds {
                    y: b.y + TAB_BAR_HEIGHT,
                    height: (b.height - TAB_BAR_HEIGHT).max(0.0),
                    ..b
                }
            } else {
                b
            };
            let narrow_center = !group.tabs_visible
                && group.axis == Axis::Vertical
                && body.width
                    < group
                        .tiles
                        .as_ref()
                        .and_then(|t| t.tiles.first())
                        .map_or(TILE_SIZE, |t| t.width)
                        * 2.0
                        + 2.0
                && x >= body.x + body.width / 3.0
                && x <= body.x + body.width * 2.0 / 3.0;
            let edge = if narrow_center {
                None
            } else if x < body.x + 18.0 {
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
            return Some(if let Some(edge) = edge {
                DropHint {
                    target: DockTarget::Split {
                        group: group.id,
                        edge,
                    },
                    bounds: edge_line(b, edge),
                }
            } else {
                DropHint {
                    target: DockTarget::Tab {
                        group: group.id,
                        index: None,
                    },
                    bounds: tab_insertion_line(group, tabs, group.panels.len()),
                }
            });
        }
        // Free canvas has no drop indicator. Within the fixed snap reach,
        // prefer the closest panel boundary or the window edge.
        // The drawing workspace begins below the app title bar. Its top snap
        // distances and indicator share this boundary, not the window's y=0.
        let dock_screen = Bounds {
            y: crate::HEADER_HEIGHT.min(screen.height),
            height: (screen.height - crate::HEADER_HEIGHT).max(0.0),
            ..screen
        };
        let (mut screen_distance, screen_edge) = nearest_edge(dock_screen, x, y);
        if !docks_visible {
            // A hidden screen edge must not steal a nearby floating tab target.
            screen_distance = f32::INFINITY;
        }
        let nearest = self
            .groups
            .iter()
            .rev()
            .filter(|g| docks_visible || g.floating)
            .map(|g| (g.bounds.distance_to([x, y]), g))
            .filter(|(distance, group)| {
                *distance
                    <= if group.floating {
                        WORKSPACE_PROXIMITY
                    } else {
                        PANEL_SNAP_DISTANCE
                    }
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((distance, group)) = nearest
            && distance < screen_distance
        {
            if group.floating {
                return Some(DropHint {
                    target: DockTarget::Tab {
                        group: group.id,
                        index: None,
                    },
                    bounds: tab_insertion_line(group, tabs, group.panels.len()),
                });
            }
            let edge = nearest_edge(group.bounds, x, y).1;
            return Some(DropHint {
                target: DockTarget::Split {
                    group: group.id,
                    edge,
                },
                bounds: edge_line(group.bounds, edge),
            });
        }
        if !docks_visible {
            return None;
        }
        // Beyond an individual panel's 40px reach, the next 40px selects the
        // whole sidebar. Its complete inner divider is the insertion line.
        let sidebar = self
            .dividers
            .iter()
            .filter(|d| d.band)
            .filter(|d| {
                let mut body = d.parent;
                match (d.axis, d.reversed) {
                    (Axis::Horizontal, false) => body.width = d.bounds.x - body.x,
                    (Axis::Horizontal, true) => {
                        body.width -= d.bounds.x + d.bounds.width - body.x;
                        body.x = d.bounds.x + d.bounds.width;
                    }
                    (Axis::Vertical, false) => body.height = d.bounds.y - body.y,
                    (Axis::Vertical, true) => {
                        body.height -= d.bounds.y + d.bounds.height - body.y;
                        body.y = d.bounds.y + d.bounds.height;
                    }
                }
                self.groups.iter().any(|g| {
                    !g.floating
                        && body.contains(
                            g.bounds.x + g.bounds.width * 0.5,
                            g.bounds.y + g.bounds.height * 0.5,
                        )
                })
            })
            .map(|d| (d.bounds.distance_to([x, y]), d))
            .filter(|(distance, _)| *distance <= WORKSPACE_PROXIMITY && *distance < screen_distance)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, divider)) = sidebar {
            let mut line = divider.bounds;
            if divider.axis == Axis::Horizontal {
                line.x += (line.width - 3.0) * 0.5;
                line.width = 3.0;
            } else {
                line.y += (line.height - 3.0) * 0.5;
                line.height = 3.0;
            }
            return Some(DropHint {
                target: DockTarget::BesideBand { band: divider.id },
                bounds: line,
            });
        }
        (screen_distance <= WORKSPACE_PROXIMITY).then(|| {
            let outer = !matches!(screen_edge, Edge::Top | Edge::Bottom)
                || screen_distance <= PANEL_SNAP_DISTANCE;
            let bounds = if outer {
                dock_screen
            } else {
                Bounds {
                    height: self.work_area.height + self.status.height,
                    ..self.work_area
                }
            };
            DropHint {
                target: DockTarget::Edge {
                    edge: screen_edge,
                    outer,
                },
                bounds: edge_line(bounds, screen_edge),
            }
        })
    }
    /// Hidden chrome reveals only at the window edge; visible chrome remains
    /// available near its controls. Hosts may also pin it for focus/popovers.
    pub fn near_chrome(&self, point: [f32; 2], viewport: [f32; 2], hidden: bool) -> bool {
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
            Edge::Top => y <= WORKSPACE_PROXIMITY,
            Edge::Bottom => height - y <= WORKSPACE_PROXIMITY,
            Edge::Left => x <= WORKSPACE_PROXIMITY,
            Edge::Right => width - x <= WORKSPACE_PROXIMITY,
        });
        if hidden || edge {
            return edge;
        }
        y <= crate::HEADER_HEIGHT + WORKSPACE_PROXIMITY
            || (x >= self.status.x - WORKSPACE_PROXIMITY
                && x <= self.status.x + self.status.width + WORKSPACE_PROXIMITY
                && y >= self.status.y - WORKSPACE_PROXIMITY
                && y <= self.status.y + self.status.height + WORKSPACE_PROXIMITY)
            || self.groups.iter().filter(|g| !g.floating).any(|g| {
                let b = g.bounds;
                x >= b.x - WORKSPACE_PROXIMITY
                    && x <= b.x + b.width + WORKSPACE_PROXIMITY
                    && y >= b.y - WORKSPACE_PROXIMITY
                    && y <= b.y + b.height + WORKSPACE_PROXIMITY
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

fn tab_min_width(node: &DockNode, layout: &DockLayout) -> f32 {
    match node {
        DockNode::Tabs { id, .. } => layout.group_min_width(*id),
        DockNode::Split {
            axis,
            first,
            second,
            ..
        } => {
            let a = tab_min_width(first, layout);
            let b = tab_min_width(second, layout);
            if *axis == Axis::Horizontal && (a > 0.0 || b > 0.0) {
                a + b + WORKSPACE_SPACING
            } else {
                a.max(b)
            }
        }
    }
}

// Intrinsic ribbon thickness, including ribbons nested beside other panels.
// Use the same split fractions as allocation; no resize callbacks or feedback.
fn ribbon_cross_min(node: &DockNode, ribbon_axis: Axis, length: f32, layout: &DockLayout) -> f32 {
    let minimum = match node {
        DockNode::Tabs { panels, active, .. }
            if panels.len() == 1 && active.kind() == PanelKind::Tiles =>
        {
            let config = layout.panel(*active).unwrap();
            let [w, h] = config.tile_style.size();
            ribbon_lanes(
                length,
                config.tiles().len(),
                if ribbon_axis == Axis::Horizontal {
                    w
                } else {
                    h
                },
            ) as f32
                * (if ribbon_axis == Axis::Horizontal {
                    h
                } else {
                    w
                } + 2.0)
                - 2.0
        }
        DockNode::Tabs { panels, .. } if panels.iter().any(|p| p.kind() == PanelKind::Tiles) => {
            // A tab bar must not consume the ribbon's entire old one-row
            // allocation. Reserve one padded lane; additional tiles may clip.
            panels
                .iter()
                .filter_map(|p| layout.panel(*p).ok())
                .filter(|p| p.id.kind() == PanelKind::Tiles)
                .map(|p| p.tile_style.size()[usize::from(ribbon_axis == Axis::Horizontal)])
                .fold(TILE_SIZE, f32::max)
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
                    a + b + WORKSPACE_SPACING
                }
            }
        }
    };
    if ribbon_axis == Axis::Vertical
        && let DockNode::Tabs { id, .. } = node
    {
        minimum.max(layout.group_min_width(*id))
    } else {
        minimum
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
fn resize_node_extent(
    node: &mut DockNode,
    group: u32,
    dimension: Axis,
    width: f32,
    delta: f32,
) -> bool {
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
    if *axis != dimension {
        return resize_node_extent(first, group, dimension, width, delta)
            || resize_node_extent(second, group, dimension, width, delta);
    }
    let usable = (width - WORKSPACE_SPACING).max(0.0);
    let first_width = usable * *fraction;
    if resize_node_extent(first, group, dimension, first_width, delta) {
        *fraction = (first_width + delta) / (usable + delta);
        true
    } else if resize_node_extent(second, group, dimension, usable - first_width, delta) {
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
        DockNode::Tabs {
            id, panels, active, ..
        } => {
            let standalone = panels.len() == 1;
            let config = layout.panel(*active).expect("validated panel");
            let tabs_visible =
                !standalone || (active.kind() != PanelKind::Tiles && !config.hide_tab);
            result.groups.push(GroupPlacement {
                id: *id,
                bounds,
                panels: panels.clone(),
                active: *active,
                axis: orientation,
                tabs_visible,
                footer_grip: (!tabs_visible && active.kind() == PanelKind::Content).then_some(
                    Bounds {
                        x: 0.0,
                        y: (bounds.height - PANEL_GRIP_HEIGHT).max(0.0),
                        width: bounds.width,
                        height: PANEL_GRIP_HEIGHT.min(bounds.height),
                    },
                ),
                floating: false,
                resize_handles: Vec::new(),
                tiles: (active.kind() == PanelKind::Tiles).then(|| {
                    tile_layout(
                        bounds.width,
                        bounds.height - if tabs_visible { TAB_BAR_HEIGHT } else { 0.0 },
                        orientation,
                        layout.panel(*active).map(|p| p.tiles().len()).unwrap_or(0),
                        standalone,
                        layout
                            .panel(*active)
                            .map(|p| p.tile_style)
                            .unwrap_or_default(),
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
            let usable = length - gap;
            let mut first_size = usable * fraction;
            if *axis == Axis::Horizontal || *axis != orientation {
                let minimum = |node: &DockNode| {
                    let tabs = if *axis == Axis::Horizontal {
                        tab_min_width(node, layout)
                    } else {
                        0.0
                    };
                    let ribbon = if *axis != orientation {
                        ribbon_cross_min(
                            node,
                            orientation,
                            if orientation == Axis::Horizontal {
                                bounds.width
                            } else {
                                bounds.height
                            },
                            layout,
                        )
                    } else {
                        0.0
                    };
                    tabs.max(ribbon)
                };
                let a = minimum(first);
                let b = minimum(second);
                first_size = if a + b <= usable {
                    first_size.clamp(a, usable - b)
                } else {
                    usable * a / (a + b)
                };
            }
            let a = rest.strip(edge, first_size);
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
    fn layers_minimum_survives_dock_and_floating_resize() {
        let mut layout = DockLayout::default();
        layout.bands.iter_mut().find(|b| b.id == 7).unwrap().extent = 45.;
        let r = layout.workspace(1200., 900., crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        let row = r.groups.iter().find(|g| g.active == Panel::Layers).unwrap();
        assert_eq!(row.bounds.width, LAYERS_MIN_WIDTH);
        layout
            .move_panel(
                [1200., 900.],
                Panel::Layers,
                DockTarget::Float {
                    position: [600., 200.],
                },
            )
            .unwrap();
        let r = layout.workspace(1200., 900., crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        let row = r.groups.iter().find(|g| g.active == Panel::Layers).unwrap();
        let right = row.bounds.x + row.bounds.width;
        layout
            .resize_floating(
                row.id,
                ResizeEdge::Left,
                row.bounds,
                [1000., 0.],
                [1200., 900.],
            )
            .unwrap();
        let r = layout.workspace(1200., 900., crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        let row = r.groups.iter().find(|g| g.active == Panel::Layers).unwrap();
        assert_eq!(row.bounds.width, LAYERS_MIN_WIDTH);
        assert_eq!(row.bounds.x + row.bounds.width, right);
    }

    fn column_fixture(two_sections: bool) -> (DockLayout, [Panel; 4]) {
        let mut layout = DockLayout::default();
        let mut panels = [Panel::Toolbar; 4];
        for (i, panel) in panels.iter_mut().enumerate().skip(1) {
            *panel = layout
                .add_toolbar(None, &format!("Column {i}"), &[ToolbarControl::Color])
                .unwrap();
        }
        let tab = |id, panel| DockNode::Tabs {
            tab_style: crate::TabStyle::default(),
            id,
            panels: vec![panel],
            active: panel,
        };
        let horizontal = |id, fraction, first, second| DockNode::Split {
            id,
            axis: Axis::Horizontal,
            fraction,
            first: Box::new(first),
            second: Box::new(second),
        };
        let section = if two_sections {
            DockNode::Split {
                id: 107,
                axis: Axis::Vertical,
                fraction: 0.5,
                first: Box::new(horizontal(
                    108,
                    0.35,
                    tab(102, panels[0]),
                    tab(103, panels[1]),
                )),
                second: Box::new(horizontal(
                    109,
                    0.4,
                    tab(104, panels[2]),
                    tab(105, panels[3]),
                )),
            }
        } else {
            horizontal(
                107,
                0.15,
                tab(102, panels[0]),
                horizontal(108, 0.35, tab(103, panels[1]), tab(104, panels[2])),
            )
        };
        layout.bands = vec![DockBand {
            id: 99,
            edge: Edge::Left,
            extent: 612.0,
            root: DockNode::Split {
                id: 100,
                axis: Axis::Vertical,
                fraction: 0.2,
                first: Box::new(tab(101, Panel::Brushes)),
                second: Box::new(section),
            },
        }];
        layout.next_id = 110;
        layout.validate().unwrap();
        (layout, panels)
    }

    #[test]
    fn removing_nested_columns_reclaims_only_a_unique_multicolumn_section() {
        let viewport = [1600.0, 1600.0];
        for two_sections in [false, true] {
            for (index, merge) in (0..3).flat_map(|i| [(i, false), (i, true)]) {
                let (mut layout, panels) = column_fixture(two_sections);
                let before = layout.workspace(
                    viewport[0],
                    viewport[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                );
                let removed = group(&before, panels[index]).width;
                let original_width = group(&before, Panel::Brushes).width;
                layout
                    .move_panel(
                        viewport,
                        panels[index],
                        if merge {
                            DockTarget::Tab {
                                group: layout.panel_group(panels[(index + 1) % 3]).unwrap(),
                                index: None,
                            }
                        } else {
                            DockTarget::Float {
                                position: [1000.0, 600.0],
                            }
                        },
                    )
                    .unwrap();
                let after = layout.workspace(
                    viewport[0],
                    viewport[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                );
                assert!(
                    (group(&after, Panel::Brushes).width
                        - if two_sections {
                            original_width
                        } else {
                            original_width - removed - WORKSPACE_SPACING
                        })
                    .abs()
                        < 0.01
                );
                if !two_sections {
                    for (other, panel) in panels[..3].iter().enumerate() {
                        if other != index {
                            assert!(
                                (group(&after, *panel).width - group(&before, *panel).width).abs()
                                    < 0.01,
                                "surviving subcolumns retain their widths"
                            );
                        }
                    }
                }
                layout.validate().unwrap();
            }
        }
    }

    #[test]
    fn column_reclaim_recurses_through_stacked_rows_inside_subcolumns() {
        let (mut layout, panels) = column_fixture(false);
        *layout.node_mut(103).unwrap() = DockNode::Split {
            id: 110,
            axis: Axis::Vertical,
            fraction: 0.3,
            first: Box::new(DockNode::Tabs {
                tab_style: crate::TabStyle::default(),
                id: 111,
                panels: vec![Panel::Sizes],
                active: Panel::Sizes,
            }),
            second: Box::new(DockNode::Split {
                id: 112,
                axis: Axis::Horizontal,
                fraction: 0.45,
                first: Box::new(DockNode::Tabs {
                    tab_style: crate::TabStyle::default(),
                    id: 113,
                    panels: vec![panels[1]],
                    active: panels[1],
                }),
                second: Box::new(DockNode::Tabs {
                    tab_style: crate::TabStyle::default(),
                    id: 114,
                    panels: vec![panels[3]],
                    active: panels[3],
                }),
            }),
        };
        layout.next_id = 115;
        layout.validate().unwrap();
        let viewport = [1800.0, 1800.0];
        let before = layout.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        layout
            .move_panel(
                viewport,
                panels[1],
                DockTarget::Float {
                    position: [1200.0, 800.0],
                },
            )
            .unwrap();
        let after = layout.workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let removed = group(&before, panels[1]).width + WORKSPACE_SPACING;
        for panel in [Panel::Brushes, Panel::Sizes] {
            assert!(
                (group(&after, panel).width - group(&before, panel).width + removed).abs() < 0.01
            );
        }
        for panel in [panels[0], panels[2], panels[3]] {
            assert!((group(&after, panel).width - group(&before, panel).width).abs() < 0.01);
        }
    }

    #[test]
    fn column_shrinking_continues_after_each_nested_child_hits_its_minimum() {
        let (mut layout, panels) = column_fixture(false);
        layout.panel_mut(panels[1]).unwrap().tile_style = TileStyle::Large;
        layout.panel_mut(panels[2]).unwrap().tile_style = TileStyle::Large;
        let minimum = 36.0 + 72.0 + 72.0 + WORKSPACE_SPACING * 2.0;
        for requested in [600.0, 400.0, 300.0, 250.0, 220.0, 200.0, 192.0, 160.0] {
            layout.bands[0].extent = requested + WORKSPACE_SPACING;
            let resolved =
                layout.workspace(1600.0, 2000.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
            let full_width = group(&resolved, Panel::Brushes).width;
            assert!(
                (full_width - requested.max(minimum)).abs() < 0.01,
                "requested={requested}, actual={full_width}"
            );
            for (i, panel) in panels[..3].iter().enumerate() {
                let width = group(&resolved, *panel).width;
                let min = if i == 0 { 36.0 } else { 72.0 };
                assert!(width + 0.01 >= min);
                if requested < minimum {
                    assert!((width - min).abs() < 0.01);
                }
            }
        }
        // Mixed content/ribbon minima compose before applying split ratios;
        // max(sum(tab minima), sum(tile minima)) would undercount this case.
        *layout.node_mut(103).unwrap() = DockNode::Tabs {
            tab_style: crate::TabStyle::default(),
            id: 103,
            panels: vec![Panel::Sizes],
            active: Panel::Sizes,
        };
        layout.fit_tabs(103);
        layout.measurements.push(PanelMeasurement {
            panel: Panel::Sizes,
            tab_width: 80.0,
            content_height: 100.0,
        });
        layout.bands[0].extent = 50.0;
        let resolved = layout.workspace(1600.0, 2000.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        assert_eq!(group(&resolved, Panel::Sizes).width, 100.0);
        assert_eq!(
            group(&resolved, Panel::Brushes).width,
            36.0 + 100.0 + 72.0 + 12.0
        );
    }

    #[test]
    fn sidebar_snap_has_distinct_panel_and_whole_stack_zones() {
        for edge in [Edge::Left, Edge::Right] {
            let mut layout = DockLayout::default();
            // Keep only the stacked Sizes/Brushes band and the Layers band.
            layout.set_panel_visible(Panel::Toolbar, false).unwrap();
            layout
                .bands
                .iter_mut()
                .find(|b| b.root.group_for(Panel::Layers).is_some())
                .unwrap()
                .edge = if edge == Edge::Left {
                Edge::Right
            } else {
                Edge::Left
            };
            let band = layout
                .bands
                .iter_mut()
                .find(|b| b.root.group_for(Panel::Sizes).is_some())
                .unwrap();
            band.edge = edge;
            let band_id = band.id;
            let r = layout.workspace(1600.0, 1000.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
            let divider = r
                .dividers
                .iter()
                .find(|d| d.id == band_id && d.band)
                .unwrap();
            for panel in [Panel::Sizes, Panel::Brushes] {
                let g = r.groups.iter().find(|g| g.panels.contains(&panel)).unwrap();
                let y = g.bounds.y + g.bounds.height * 0.5;
                let boundary = if edge == Edge::Left {
                    g.bounds.x + g.bounds.width
                } else {
                    g.bounds.x
                };
                let sign = if edge == Edge::Left { 1.0 } else { -1.0 };
                let split = if edge == Edge::Left {
                    Edge::Right
                } else {
                    Edge::Left
                };
                for distance in [1.0, 20.0, 40.0] {
                    assert_eq!(
                        r.drop_hint(boundary + sign * distance, y, &[], true)
                            .unwrap()
                            .target,
                        DockTarget::Split {
                            group: g.id,
                            edge: split
                        }
                    );
                }
                let hint = r.drop_hint(boundary + sign * 60.0, y, &[], true).unwrap();
                assert_eq!(hint.target, DockTarget::BesideBand { band: band_id });
                assert_eq!(hint.bounds.height, divider.bounds.height);
                let mut moved = layout.clone();
                let old_width = r
                    .groups
                    .iter()
                    .find(|g| g.active == Panel::Layers)
                    .unwrap()
                    .bounds
                    .width;
                moved
                    .move_panel([1600.0, 1000.0], Panel::Layers, hint.target)
                    .unwrap();
                let after =
                    moved.workspace(1600.0, 1000.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
                let layers = after
                    .groups
                    .iter()
                    .find(|g| g.active == Panel::Layers)
                    .unwrap();
                assert_eq!(layers.bounds.height, divider.bounds.height);
                assert_eq!(layers.bounds.width, old_width);
                assert_eq!(
                    moved.bands.iter().find(|b| b.id == band_id).unwrap().root,
                    layout.bands.iter().find(|b| b.id == band_id).unwrap().root
                );
                assert!(r.drop_hint(boundary + sign * 90.0, y, &[], true).is_none());
            }
        }
        let empty = DockLayout {
            bands: Vec::new(),
            ..Default::default()
        }
        .workspace(1200.0, 900.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        let top = empty.drop_hint(600.0, 0.0, &[], true).unwrap();
        assert_eq!(
            top.target,
            DockTarget::Edge {
                edge: Edge::Top,
                outer: true
            }
        );
        assert_eq!(top.bounds.y, crate::HEADER_HEIGHT);
        let mut layout = DockLayout::default();
        layout.set_panel_visible(Panel::Toolbar, false).unwrap();
        let r = layout.workspace(1200.0, 900.0, crate::HEADER_HEIGHT, crate::STATUS_HEIGHT);
        for edge in [Edge::Top, Edge::Bottom] {
            for (distance, outer) in [(1.0, true), (40.0, true), (41.0, false), (80.0, false)] {
                let y = if edge == Edge::Top {
                    crate::HEADER_HEIGHT + distance
                } else {
                    900.0 - distance
                };
                let hint = r.drop_hint(600.0, y, &[], true).unwrap();
                assert_eq!(hint.target, DockTarget::Edge { edge, outer });
                assert_eq!(
                    hint.bounds.width,
                    if outer { 1200.0 } else { r.work_area.width }
                );
                assert_eq!(hint.bounds.x, if outer { 0.0 } else { r.work_area.x });
            }
            let y = if edge == Edge::Top {
                crate::HEADER_HEIGHT + 81.0
            } else {
                819.0
            };
            assert!(r.drop_hint(600.0, y, &[], true).is_none());
        }
    }

    #[test]
    fn floating_toolbar_cycles_orient_grip_and_fit_all_tiles() {
        for viewport in [[1600.0, 1200.0], [420.0, 300.0]] {
            for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
                let mut layout = DockLayout::default();
                layout.panel_mut(Panel::Toolbar).unwrap().tile_style = style;
                layout
                    .move_panel(
                        viewport,
                        Panel::Toolbar,
                        DockTarget::Float {
                            position: [200.0, 150.0],
                        },
                    )
                    .unwrap();
                let group = layout.panel_group(Panel::Toolbar).unwrap();
                for mode in [
                    FloatingToolbarLayout::Vertical,
                    FloatingToolbarLayout::Horizontal,
                    FloatingToolbarLayout::Compact,
                ] {
                    layout.double_click_panel_handle(group, viewport).unwrap();
                    assert_eq!(layout.floating[0].toolbar_layout, mode);
                    let g = layout
                        .workspace(
                            viewport[0],
                            viewport[1],
                            crate::HEADER_HEIGHT,
                            crate::STATUS_HEIGHT,
                        )
                        .groups
                        .into_iter()
                        .find(|g| g.id == group)
                        .unwrap();
                    let horizontal = mode == FloatingToolbarLayout::Horizontal;
                    assert_eq!(
                        g.axis,
                        if horizontal {
                            Axis::Horizontal
                        } else {
                            Axis::Vertical
                        }
                    );
                    let tiles = g.tiles.unwrap();
                    let grip = tiles.grip.unwrap();
                    if horizontal {
                        assert_eq!(
                            [grip.x, grip.y, grip.width, grip.height],
                            [g.bounds.width - 20.0, 0.0, 20.0, g.bounds.height]
                        );
                    } else {
                        assert_eq!(
                            [grip.x, grip.y, grip.width, grip.height],
                            [0.0, g.bounds.height - 20.0, g.bounds.width, 20.0]
                        );
                    }
                    for b in &tiles.tiles {
                        assert!(b.x >= 0.0 && b.y >= 0.0);
                        assert!(
                            b.x + b.width <= g.bounds.width && b.y + b.height <= g.bounds.height,
                            "{viewport:?} {style:?} {mode:?}: {b:?} outside {:?}",
                            g.bounds
                        );
                        assert!(b.intersection(grip).is_none());
                    }
                    if viewport[0] == 1600.0 {
                        if mode == FloatingToolbarLayout::Vertical {
                            assert!(tiles.tiles.iter().all(|b| b.x == 0.0));
                        } else if horizontal {
                            assert!(tiles.tiles.iter().all(|b| b.y == 0.0));
                        }
                    }
                    layout.validate().unwrap();
                    for new_style in [
                        TileStyle::Large,
                        TileStyle::Labeled,
                        TileStyle::Small,
                        style,
                    ] {
                        layout
                            .set_tile_style(Panel::Toolbar, new_style, viewport)
                            .unwrap();
                        assert_eq!(layout.floating[0].toolbar_layout, mode);
                        assert_eq!(
                            layout.floating[0].width,
                            mode.width(
                                new_style,
                                layout.panel(Panel::Toolbar).unwrap().tiles().len()
                            )
                        );
                        assert!(layout.floating[0].height.is_none());
                    }
                }
                // Older saved floats retain the compact default without a migration.
                let mut json = serde_json::to_value(&layout).unwrap();
                json["floating"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("toolbar_layout");
                assert_eq!(
                    serde_json::from_value::<DockLayout>(json).unwrap().floating[0].toolbar_layout,
                    FloatingToolbarLayout::Compact
                );
            }
        }
    }

    #[test]
    fn toolbar_style_changes_refit_floats_and_single_lane_docks() {
        let viewport = [1600.0, 1200.0];
        let resolve = |l: &DockLayout| {
            l.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            )
        };
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            let mut layout = DockLayout::default();
            layout
                .move_panel(
                    viewport,
                    Panel::Toolbar,
                    DockTarget::Edge { edge, outer: true },
                )
                .unwrap();
            for style in [TileStyle::Large, TileStyle::Labeled, TileStyle::Small] {
                layout
                    .set_tile_style(Panel::Toolbar, style, viewport)
                    .unwrap();
                let g = resolve(&layout)
                    .groups
                    .into_iter()
                    .find(|g| g.active == Panel::Toolbar)
                    .unwrap();
                assert_eq!(
                    if edge.axis() == Axis::Horizontal {
                        g.bounds.width
                    } else {
                        g.bounds.height
                    },
                    style.size()[usize::from(edge.axis() == Axis::Vertical)],
                    "{edge:?} {style:?}"
                );
                layout.validate().unwrap();
            }
            // An explicitly expanded, multi-lane dock keeps its allocation.
            let band = layout
                .bands
                .iter_mut()
                .find(|b| b.root.group_for(Panel::Toolbar).is_some())
                .unwrap();
            band.extent = 180.0;
            let before = resolve(&layout)
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Toolbar)
                .unwrap()
                .bounds;
            layout
                .set_tile_style(Panel::Toolbar, TileStyle::Large, viewport)
                .unwrap();
            let after = resolve(&layout)
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Toolbar)
                .unwrap()
                .bounds;
            assert_eq!(before, after);
        }
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                viewport,
                Panel::Toolbar,
                DockTarget::Float {
                    position: [700.0, 350.0],
                },
            )
            .unwrap();
        let group = layout.panel_group(Panel::Toolbar).unwrap();
        assert_eq!(
            layout.panel_handle_target(DockItem::Panel {
                panel: Panel::Toolbar
            }),
            Some(group)
        );
        for style in [TileStyle::Large, TileStyle::Labeled, TileStyle::Small] {
            layout.floating[0].width = 390.0;
            layout.floating[0].height = Some(310.0);
            let position = layout.floating[0].position;
            layout
                .set_tile_style(Panel::Toolbar, style, viewport)
                .unwrap();
            assert_eq!(layout.floating[0].width, style.floating_width());
            assert_eq!(layout.floating[0].height, None);
            assert_eq!(layout.floating[0].position, position);
        }
        layout
            .move_panel(
                viewport,
                Panel::Sizes,
                DockTarget::Tab { group, index: None },
            )
            .unwrap();
        layout.floating[0].width = 390.0;
        layout.floating[0].height = Some(310.0);
        layout
            .set_tile_style(Panel::Toolbar, TileStyle::Large, viewport)
            .unwrap();
        assert_eq!(layout.floating[0].width, 390.0);
        assert_eq!(layout.floating[0].height, Some(310.0));
        assert_eq!(
            layout.panel_handle_target(DockItem::Panel {
                panel: Panel::Toolbar
            }),
            None
        );
        assert_eq!(
            layout.panel_handle_target(DockItem::Panel {
                panel: Panel::Sizes
            }),
            None
        );
        assert_eq!(
            layout.panel_handle_target(DockItem::Group { group }),
            Some(group)
        );
    }

    #[test]
    fn docking_resets_toolbar_width_and_nested_tile_refits_preserve_neighbors() {
        let viewport = [1600.0, 2200.0];
        let resolve = |l: &DockLayout| {
            l.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            )
        };
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            for edge in [Edge::Left, Edge::Right] {
                for split in [false, true] {
                    let mut layout = DockLayout::default();
                    layout
                        .set_tile_style(Panel::Toolbar, style, viewport)
                        .unwrap();
                    layout
                        .move_panel(
                            viewport,
                            Panel::Toolbar,
                            DockTarget::Float {
                                position: [700.0, 350.0],
                            },
                        )
                        .unwrap();
                    layout.floating[0].width = 400.0;
                    layout.floating[0].height = Some(250.0);
                    let sizes = layout.panel_group(Panel::Sizes).unwrap();
                    let target = if split {
                        DockTarget::Split { group: sizes, edge }
                    } else {
                        DockTarget::Edge { edge, outer: true }
                    };
                    layout.move_panel(viewport, Panel::Toolbar, target).unwrap();
                    let before = resolve(&layout);
                    let toolbar = before
                        .groups
                        .iter()
                        .find(|g| g.active == Panel::Toolbar)
                        .unwrap();
                    assert!(
                        (toolbar.bounds.width - style.size()[0]).abs() < 0.01,
                        "{edge:?} {style:?} split={split}: {:?}",
                        toolbar.bounds
                    );
                    let other = before
                        .groups
                        .iter()
                        .find(|g| g.id == sizes)
                        .unwrap()
                        .bounds
                        .width;
                    layout.resize_docked_group_cross(&before, toolbar, style.size()[0] + 120.0);
                    layout
                        .double_click_panel_handle(toolbar.id, viewport)
                        .unwrap();
                    let restored = resolve(&layout);
                    assert!(
                        (restored
                            .groups
                            .iter()
                            .find(|g| g.id == toolbar.id)
                            .unwrap()
                            .bounds
                            .width
                            - toolbar.bounds.width)
                            .abs()
                            < 0.01
                    );
                    assert!(
                        (restored
                            .groups
                            .iter()
                            .find(|g| g.id == sizes)
                            .unwrap()
                            .bounds
                            .width
                            - other)
                            .abs()
                            < 0.01,
                        "Refitting a nested toolbar preserves the adjacent content width"
                    );
                    let next = if style == TileStyle::Small {
                        TileStyle::Large
                    } else {
                        TileStyle::Small
                    };
                    layout
                        .set_tile_style(Panel::Toolbar, next, viewport)
                        .unwrap();
                    let after = resolve(&layout);
                    assert!(
                        (after
                            .groups
                            .iter()
                            .find(|g| g.active == Panel::Toolbar)
                            .unwrap()
                            .bounds
                            .width
                            - next.size()[0])
                            .abs()
                            < 0.01,
                        "{edge:?} {style:?}->{next:?} split={split}: {:?}",
                        after
                            .groups
                            .iter()
                            .find(|g| g.active == Panel::Toolbar)
                            .unwrap()
                            .bounds
                    );
                    assert!(
                        (after
                            .groups
                            .iter()
                            .find(|g| g.id == sizes)
                            .unwrap()
                            .bounds
                            .width
                            - other)
                            .abs()
                            < 0.01
                    );
                    layout.validate().unwrap();
                }
            }
        }
    }

    #[test]
    fn horizontal_docks_reject_content_and_tab_groups_including_existing_ribbon_targets() {
        for edge in [Edge::Top, Edge::Bottom] {
            let mut base = DockLayout::default();
            base.move_panel(
                VIEWPORT,
                Panel::Toolbar,
                DockTarget::Edge { edge, outer: true },
            )
            .unwrap();
            let toolbar = base.panel_group(Panel::Toolbar).unwrap();
            let band = base
                .bands
                .iter()
                .find(|b| b.root.id() == toolbar)
                .unwrap()
                .id;
            for grouped in [false, true] {
                let mut layout = base.clone();
                if grouped {
                    layout.add_panel_to_group(Panel::Sizes, 5).unwrap();
                }
                let item = if grouped {
                    DockItem::Group { group: 5 }
                } else {
                    DockItem::Panel {
                        panel: Panel::Brushes,
                    }
                };
                for target in [
                    DockTarget::Edge { edge, outer: false },
                    DockTarget::Edge { edge, outer: true },
                    DockTarget::BesideBand { band },
                    DockTarget::Tab {
                        group: toolbar,
                        index: None,
                    },
                    DockTarget::Split {
                        group: toolbar,
                        edge: Edge::Top,
                    },
                    DockTarget::Split {
                        group: toolbar,
                        edge: Edge::Bottom,
                    },
                ] {
                    let before = layout.clone();
                    assert!(layout.move_item(VIEWPORT, item, target).is_err());
                    assert_eq!(layout, before, "a rejected drop is atomic");
                }
                layout
                    .move_item(
                        VIEWPORT,
                        item,
                        DockTarget::Split {
                            group: 8,
                            edge: Edge::Top,
                        },
                    )
                    .unwrap();
            }
            assert!(base.add_panel_to_group(Panel::Brushes, toolbar).is_err());
            let duplicate = base
                .duplicate_toolbar(Panel::Toolbar, "Another toolbar")
                .unwrap();
            assert_ne!(base.panel_group(duplicate), Some(toolbar));
            assert_eq!(base.group_panels(toolbar).unwrap(), &[Panel::Toolbar]);
            assert!(
                base.move_panel(
                    VIEWPORT,
                    duplicate,
                    DockTarget::Tab {
                        group: toolbar,
                        index: None
                    }
                )
                .is_err()
            );
            base.move_panel(VIEWPORT, duplicate, DockTarget::Edge { edge, outer: false })
                .unwrap();
            base.validate().unwrap();
        }
    }

    #[test]
    fn snapped_toolbars_add_only_the_lanes_required_to_avoid_overflow() {
        let viewport = [1000.0, 800.0];
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
                for count in [6, 17, 29] {
                    let mut layout = DockLayout::default();
                    let panel = layout
                        .add_toolbar(None, "Wrapping", &vec![ToolbarControl::Color; count])
                        .unwrap();
                    layout.set_tile_style(panel, style, viewport).unwrap();
                    layout
                        .move_panel(
                            viewport,
                            panel,
                            DockTarget::Float {
                                position: [500.0, 400.0],
                            },
                        )
                        .unwrap();
                    layout.floating[0].width = 480.0;
                    layout
                        .move_panel(viewport, panel, DockTarget::Edge { edge, outer: true })
                        .unwrap();
                    let resolved = layout.workspace(
                        viewport[0],
                        viewport[1],
                        crate::HEADER_HEIGHT,
                        crate::STATUS_HEIGHT,
                    );
                    let g = resolved.groups.iter().find(|g| g.active == panel).unwrap();
                    let horizontal = edge.axis() == Axis::Vertical;
                    let (length, cross, along_size, cross_size) = if horizontal {
                        (
                            g.bounds.width,
                            g.bounds.height,
                            style.size()[0],
                            style.size()[1],
                        )
                    } else {
                        (
                            g.bounds.height,
                            g.bounds.width,
                            style.size()[1],
                            style.size()[0],
                        )
                    };
                    let slots = ((length - 20.0) / (along_size + 2.0)).floor() as usize;
                    let lanes = count.div_ceil(slots);
                    assert_eq!(
                        cross,
                        lanes as f32 * (cross_size + 2.0) - 2.0,
                        "{style:?} {edge:?} count={count}"
                    );
                    let tiles = g.tiles.as_ref().unwrap();
                    let grip = tiles.grip.unwrap();
                    for tile in &tiles.tiles {
                        assert!(tile.x >= 0.0 && tile.y >= 0.0);
                        assert!(
                            tile.x + tile.width <= g.bounds.width
                                && tile.y + tile.height <= g.bounds.height
                        );
                        assert!(tile.intersection(grip).is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn floating_resize_targets_are_external_and_anchor_the_opposite_edges() {
        let viewport = [1600.0, 1200.0];
        let resolve = |l: &DockLayout| {
            l.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            )
        };
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                viewport,
                Panel::Sizes,
                DockTarget::Float {
                    position: [700.0, 350.0],
                },
            )
            .unwrap();
        let g = resolve(&layout)
            .groups
            .into_iter()
            .find(|g| g.floating)
            .unwrap();
        let b = g.bounds;
        for (i, handle) in g.resize_handles.iter().enumerate() {
            assert!(handle.bounds.intersection(b).is_none());
            assert!(
                g.resize_handles[..i]
                    .iter()
                    .all(|other| other.bounds.intersection(handle.bounds).is_none())
            );
            let mut changed = layout.clone();
            changed
                .resize_floating(g.id, handle.edge, b, [20.0, 30.0], viewport)
                .unwrap();
            let actual = resolve(&changed)
                .groups
                .into_iter()
                .find(|f| f.id == g.id)
                .unwrap()
                .bounds;
            let (left, right, top, bottom) = match handle.edge {
                ResizeEdge::Left => (20.0, 0.0, 0.0, 0.0),
                ResizeEdge::Right => (0.0, 20.0, 0.0, 0.0),
                ResizeEdge::Top => (0.0, 0.0, 30.0, 0.0),
                ResizeEdge::Bottom => (0.0, 0.0, 0.0, 30.0),
                ResizeEdge::TopLeft => (20.0, 0.0, 30.0, 0.0),
                ResizeEdge::TopRight => (0.0, 20.0, 30.0, 0.0),
                ResizeEdge::BottomLeft => (20.0, 0.0, 0.0, 30.0),
                ResizeEdge::BottomRight => (0.0, 20.0, 0.0, 30.0),
            };
            assert_eq!(
                actual,
                Bounds {
                    x: b.x + left,
                    y: b.y + top,
                    width: b.width + right - left,
                    height: b.height + bottom - top
                }
            );
            changed.reset_floating_size(g.id).unwrap();
            let reset = resolve(&changed)
                .groups
                .into_iter()
                .find(|f| f.id == g.id)
                .unwrap()
                .bounds;
            assert_eq!([reset.width, reset.height], [b.width, b.height]);
            assert_eq!([reset.x, reset.y], [actual.x, actual.y]);
        }
    }

    #[test]
    fn tile_modes_share_allocation_without_overlap_or_grip_collisions() {
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            for axis in [Axis::Horizontal, Axis::Vertical] {
                for standalone in [false, true] {
                    for count in [0, 1, 2, 7, 31] {
                        let layout = tile_layout(1000.0, 800.0, axis, count, standalone, style);
                        assert_eq!(layout.tiles.len(), count);
                        assert_eq!(layout.insertion.len(), count + 1);
                        for (i, tile) in layout.tiles.iter().enumerate() {
                            assert_eq!([tile.width, tile.height], style.size());
                            assert!(
                                tile.x >= 0.0
                                    && tile.y >= 0.0
                                    && tile.x + tile.width <= 1000.0
                                    && tile.y + tile.height <= 800.0
                            );
                            assert!(
                                layout.tiles[..i]
                                    .iter()
                                    .all(|other| tile.intersection(*other).is_none())
                            );
                            assert!(layout.grip.is_none_or(|g| tile.intersection(g).is_none()));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn floating_groups_share_tabs_size_from_content_and_survive_hidden_docks() {
        let viewport = [1200.0, 900.0];
        let resolve = |layout: &DockLayout| {
            layout.workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            )
        };
        let mut layout = DockLayout {
            measurements: vec![
                PanelMeasurement {
                    panel: Panel::Brushes,
                    tab_width: 200.0,
                    content_height: 1000.0,
                },
                PanelMeasurement {
                    panel: Panel::Layers,
                    tab_width: 140.0,
                    content_height: 100.0,
                },
            ],
            ..Default::default()
        };
        let initial = resolve(&layout);
        let original_width = initial
            .groups
            .iter()
            .find(|g| g.active == Panel::Brushes)
            .unwrap()
            .bounds
            .width;
        let center = [
            initial.work_area.x + initial.work_area.width / 2.0,
            initial.work_area.y + initial.work_area.height / 2.0,
        ];
        assert!(initial.drop_hint(center[0], center[1], &[], true).is_none());
        layout
            .move_panel(
                viewport,
                Panel::Brushes,
                DockTarget::Float { position: center },
            )
            .unwrap();
        let group = layout.panel_group(Panel::Brushes).unwrap();
        let floating = resolve(&layout)
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap();
        assert!(floating.floating && floating.resize_handles.len() == 8);
        assert_eq!(floating.bounds.width, original_width);
        assert_eq!(floating.bounds.height, 675.0);
        layout.add_panel_to_group(Panel::Layers, group).unwrap();
        let current = resolve(&layout);
        let floating = current.groups.iter().find(|g| g.id == group).unwrap();
        assert_eq!(floating.bounds.width, 362.0);
        assert_eq!(floating.bounds.height, 136.0);
        for [x, y] in [
            [floating.bounds.x + 1.0, floating.bounds.y + 1.0],
            [
                floating.bounds.x + floating.bounds.width - 1.0,
                floating.bounds.y + floating.bounds.height - 1.0,
            ],
        ] {
            assert!(
                matches!(current.drop_hint(x, y, &[], true).unwrap().target, DockTarget::Tab { group: id, .. } if id == group)
            );
        }
        layout
            .resize_floating(
                group,
                ResizeEdge::BottomRight,
                floating.bounds,
                [
                    230.0 - floating.bounds.width,
                    300.0 - floating.bounds.height,
                ],
                viewport,
            )
            .unwrap();
        layout.select_tab(group, Panel::Brushes).unwrap();
        let resized = resolve(&layout)
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap();
        assert_eq!(resized.bounds.width, 230.0);
        assert_eq!(
            resized.bounds.height, 675.0,
            "changing tabs restores natural height after manual resize"
        );
        layout.validate().unwrap();
        let saved = serde_json::to_value(&layout).unwrap();
        assert!(saved.get("measurements").is_none());
        let restored: DockLayout = serde_json::from_value(saved).unwrap();
        restored.validate().unwrap();
        layout
            .move_item(
                viewport,
                DockItem::Group { group },
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: false,
                },
            )
            .unwrap();
        assert!(layout.floating.is_empty());
        assert_eq!(
            layout.group_panels(group).unwrap(),
            &[Panel::Brushes, Panel::Layers]
        );
    }

    #[test]
    fn floating_toolbars_default_to_three_small_or_large_columns_and_two_labeled() {
        for (style, columns) in [
            (TileStyle::Small, 3),
            (TileStyle::Large, 3),
            (TileStyle::Labeled, 2),
        ] {
            let mut layout = DockLayout::default();
            layout.panel_mut(Panel::Toolbar).unwrap().tile_style = style;
            layout
                .move_panel(
                    VIEWPORT,
                    Panel::Toolbar,
                    DockTarget::Float {
                        position: [600.0, 350.0],
                    },
                )
                .unwrap();
            let resolved = layout.workspace(
                VIEWPORT[0],
                VIEWPORT[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT,
            );
            let group = resolved
                .groups
                .iter()
                .find(|g| g.active == Panel::Toolbar)
                .unwrap();
            let [w, h] = style.size();
            assert_eq!(group.bounds.width, columns as f32 * (w + 2.0) - 2.0);
            assert_eq!(
                group.bounds.height,
                crate::TOOLBAR_CONTROLS.len().div_ceil(columns) as f32 * (h + 2.0) + 20.0
            );
            let tiles = group.tiles.as_ref().unwrap();
            assert!(
                tiles
                    .tiles
                    .iter()
                    .all(|t| t.x + t.width <= group.bounds.width
                        && t.y + t.height <= group.bounds.height - 22.0)
            );
        }
    }

    #[test]
    fn narrow_vertical_toolbar_has_a_merge_target_and_tab_growth_is_reversible() {
        let mut layout = DockLayout::default();
        layout
            .move_panel(
                VIEWPORT,
                Panel::Toolbar,
                DockTarget::Edge {
                    edge: Edge::Left,
                    outer: false,
                },
            )
            .unwrap();
        let resolved = layout.workspace(
            VIEWPORT[0],
            VIEWPORT[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        let ribbon = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Toolbar)
            .unwrap();
        assert_eq!(ribbon.bounds.width, 36.0);
        let hint = resolved
            .drop_hint(ribbon.bounds.x + 18.0, ribbon.bounds.y + 100.0, &[], true)
            .unwrap();
        assert!(matches!(hint.target, DockTarget::Tab { .. }));
        layout.measurements = vec![
            PanelMeasurement {
                panel: Panel::Brushes,
                tab_width: 150.0,
                content_height: 200.0,
            },
            PanelMeasurement {
                panel: Panel::Layers,
                tab_width: 180.0,
                content_height: 200.0,
            },
        ];
        layout.add_panel_to_group(Panel::Brushes, 8).unwrap();
        let resolved = layout.workspace(
            VIEWPORT[0],
            VIEWPORT[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        );
        assert_eq!(
            resolved
                .groups
                .iter()
                .find(|g| g.id == 8)
                .unwrap()
                .bounds
                .width,
            352.0
        );
        let divider = resolved.dividers.iter().find(|d| d.id == 7).unwrap();
        layout
            .resize_workspace(7, [divider.bounds.x + 83.0, divider.bounds.y], VIEWPORT)
            .unwrap();
        assert!(
            layout
                .workspace(
                    VIEWPORT[0],
                    VIEWPORT[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT
                )
                .groups
                .iter()
                .find(|g| g.id == 8)
                .unwrap()
                .bounds
                .width
                < 352.0
        );
    }

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
                    tab_style: crate::TabStyle::default(),
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
            tab_style: crate::TabStyle::default(),
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
                    tab_style: crate::TabStyle::default(),
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
                    let layout = tile_layout(width, height, axis, 30, standalone, TileStyle::Small);
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
                    let expanded =
                        tile_layout(900.0, 900.0, axis, 30, standalone, TileStyle::Small);
                    for (index, line) in expanded.insertion.iter().enumerate() {
                        let point = [line.x + line.width * 0.5, line.y + line.height * 0.5];
                        assert_eq!(expanded.drop_slot(point, 900.0, 900.0).unwrap().0, index);
                    }
                }
            }
        }
    }

    #[test]
    fn floating_group_becoming_a_lone_toolbar_restores_its_default_grid() {
        for style in [TileStyle::Small, TileStyle::Large, TileStyle::Labeled] {
            for hide in [false, true] {
                let mut layout = DockLayout::default();
                layout.panel_mut(Panel::Toolbar).unwrap().tile_style = style;
                layout
                    .move_panel(
                        VIEWPORT,
                        Panel::Toolbar,
                        DockTarget::Float {
                            position: [600.0, 450.0],
                        },
                    )
                    .unwrap();
                let group = layout.panel_group(Panel::Toolbar).unwrap();
                layout.add_panel_to_group(Panel::Layers, group).unwrap();
                let f = &mut layout.floating[0];
                f.width = 450.0;
                f.height = Some(600.0);
                let position = f.position;
                if hide {
                    layout.set_panel_visible(Panel::Layers, false).unwrap();
                } else {
                    layout
                        .move_panel(
                            VIEWPORT,
                            Panel::Layers,
                            DockTarget::Edge {
                                edge: Edge::Right,
                                outer: true,
                            },
                        )
                        .unwrap();
                }
                let f = &layout.floating[0];
                assert_eq!(f.root.id(), group);
                assert_eq!(f.position, position);
                assert_eq!(f.width, style.floating_width());
                assert_eq!(f.default_width, Some(style.floating_width()));
                assert_eq!(f.height, None);
                assert!(!layout.fit_tab_groups.contains(&group));
                let resolved = layout.workspace(
                    VIEWPORT[0],
                    VIEWPORT[1],
                    crate::HEADER_HEIGHT,
                    crate::STATUS_HEIGHT,
                );
                let toolbar = resolved.groups.iter().find(|g| g.id == group).unwrap();
                assert!(!toolbar.tabs_visible);
                assert_eq!(toolbar.bounds.width, style.floating_width());
                assert!(toolbar.bounds.height < 600.0);
            }
        }
    }

    #[test]
    fn hidden_slots_are_not_clamped_into_the_last_visible_row() {
        let layout = tile_layout(36.0, 36.0, Axis::Horizontal, 12, true, TileStyle::Small);
        let (index, line) = layout.drop_slot([1.0, 35.5], 36.0, 36.0).unwrap();
        assert_eq!(index, 0);
        assert_eq!(line.y, 0.0);
        // The next row is still hidden even if its marker straddles the clip.
        let layout = tile_layout(80.0, 38.0, Axis::Horizontal, 12, true, TileStyle::Small);
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
                    edge: Edge::Left,
                    outer: false,
                },
            )
            .unwrap();
        assert_eq!(layout.bands.last().unwrap().root, original);
        assert_eq!(
            layout.bands.last().unwrap().extent,
            232.0,
            "a mixed tab group retains its width"
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
            let layout = tile_layout(width, height, axis, 6, true, TileStyle::Small);
            assert_eq!((layout.tiles[0].x, layout.tiles[0].y), (0.0, 0.0));
            assert!(layout.grip.is_some());
            let grip = layout.grip.unwrap();
            assert_eq!(
                (grip.width, grip.height),
                if axis == Axis::Horizontal {
                    (20.0, height)
                } else {
                    (width, 20.0)
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
        let rows = tile_layout(500.0, 82.0, Axis::Horizontal, 6, true, TileStyle::Small);
        assert_eq!(rows.tiles[0].x, rows.tiles[3].x);
        assert_eq!(rows.tiles[0].y, rows.tiles[2].y);
        let columns = tile_layout(82.0, 500.0, Axis::Vertical, 6, false, TileStyle::Small);
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
                    TileStyle::Small,
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
                    tab_style: crate::TabStyle::default(),
                    id: 2,
                    panels: vec![Panel::Toolbar],
                    active: Panel::Toolbar,
                }),
                second: Box::new(DockNode::Tabs {
                    tab_style: crate::TabStyle::default(),
                    id: 10,
                    panels: vec![Panel::Brushes],
                    active: Panel::Brushes,
                }),
            };
            let resolved = layout.resolve(200.0, 800.0);
            let b = group(&resolved, Panel::Toolbar);
            let tiles = tile_layout(
                b.width,
                b.height,
                Axis::Horizontal,
                TOOL_TILE_COUNT,
                true,
                TileStyle::Small,
            );
            assert!(tiles.tiles.iter().all(
                |t| t.y + t.height <= b.height && t.x + t.width + 2.0 <= tiles.grip.unwrap().x
            ));
        }
        let mut layout = DockLayout::default();
        layout.bands.retain(|b| b.id == 1);
        layout.bands[0].root = DockNode::Tabs {
            tab_style: crate::TabStyle::default(),
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
        let bottom_panel = layout
            .add_toolbar(None, "Bottom", &[ToolbarControl::Color])
            .unwrap();
        layout
            .move_panel(
                VIEWPORT,
                bottom_panel,
                DockTarget::Edge {
                    edge: Edge::Bottom,
                    outer: false,
                },
            )
            .unwrap();
        let r = layout.workspace(1200.0, 900.0, 48.0, 28.0);
        let toolbar = group(&r, Panel::Toolbar);
        let bottom = group(&r, bottom_panel);
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
            let hint = layout
                .drop_hint(
                    b.x + b.width * 0.5,
                    b.y + TAB_BAR_HEIGHT + (b.height - TAB_BAR_HEIGHT) * fraction,
                    &[],
                    true,
                )
                .unwrap();
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
            layout
                .drop_hint(b.x + 40.0, b.y + 10.0, &[], true)
                .unwrap()
                .target,
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
            let hint = layout
                .drop_hint(b.x + b.width * 0.5, b.y + b.height * 0.5, &tabs, true)
                .unwrap();
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
            let over_grip = layout
                .drop_hint(b.x + b.width - 10.0, b.y + 10.0, &tabs, true)
                .unwrap();
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
        let hint = r.drop_hint(b.x + 10.0, b.y + 5.0, &tabs, true).unwrap();
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
            r.drop_hint(b.x + 100.0, b.y + b.height - 2.0, &tabs, true)
                .unwrap()
                .target,
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
        hidden.bands.clear();
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
        for point in [[600.0, 165.0], [600.0, 450.0], [-1.0, 20.0]] {
            assert!(!resolved.near_chrome(point, VIEWPORT, true));
            assert!(!resolved.near_chrome(point, VIEWPORT, false));
        }
        // An empty bottom edge does not reveal, even directly over the HUD.
        assert!(!resolved.near_chrome([600.0, 899.0], VIEWPORT, true));
        // Visible controls retain the same fixed 80px proximity distance.
        let panel = resolved
            .groups
            .iter()
            .find(|g| g.active == Panel::Brushes)
            .unwrap()
            .bounds;
        let right = panel.x + panel.width;
        assert!(resolved.near_chrome([right + 80.0, 450.0], VIEWPORT, false));
        assert!(!resolved.near_chrome([right + 81.0, 450.0], VIEWPORT, false));
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
            // Leave one standalone toolbar; top/bottom docks only accept these.
            for panel in [Panel::Brushes, Panel::Sizes, Panel::Layers] {
                layout.set_panel_visible(panel, false).unwrap();
            }
            layout
                .move_item(
                    VIEWPORT,
                    DockItem::Group { group: 2 },
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
            layout.bands.clear();
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
            .find(|g| g.panels.contains(&panel))
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
        let bottom_panel = layout
            .add_toolbar(None, "Bottom", &[ToolbarControl::Color])
            .unwrap();
        layout
            .move_panel(
                VIEWPORT,
                bottom_panel,
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
