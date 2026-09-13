//! Workspace-owned window-bar items. No dock panels or native widgets live here.
use crate::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderSize {
    #[default]
    Small,
    Medium,
    Large,
}
impl HeaderSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }
    pub fn tile(self) -> f32 {
        match self {
            Self::Small => 36.,
            Self::Medium => 48.,
            Self::Large => 60.,
        }
    }
    pub fn icon(self) -> i32 {
        match self {
            Self::Small => 20,
            Self::Medium => 28,
            Self::Large => 36,
        }
    }
    pub fn height(self) -> f32 {
        self.tile() + 12.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeaderZone {
    Left,
    Center,
    Right,
}
impl HeaderZone {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];
    pub fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::Right => 2,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeaderItem {
    Capy,
    Menu,
    MenuLabels,
    Workspaces,
    DocumentTitle,
    Clock,
    Battery,
    Space,
    Tool { control: ToolbarControl },
}
impl HeaderItem {
    pub const COMPONENTS: [Self; 8] = [
        Self::Capy,
        Self::Menu,
        Self::MenuLabels,
        Self::Workspaces,
        Self::DocumentTitle,
        Self::Clock,
        Self::Battery,
        Self::Space,
    ];
    pub fn label(self) -> String {
        match self {
            Self::Capy => "Capy (Zen Mode)",
            Self::Menu => "Main Menu",
            Self::MenuLabels => "Menu Labels",
            Self::Workspaces => "Workspace Switcher",
            Self::DocumentTitle => "Document Title",
            Self::Clock => "Clock",
            Self::Battery => "Battery",
            Self::Space => "Space",
            Self::Tool { control } => return tool_choice(control).label,
        }
        .into()
    }
    pub fn singleton(self) -> bool {
        !matches!(self, Self::Tool { .. } | Self::Space)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderEntry {
    pub id: u32,
    pub item: HeaderItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeaderLayout {
    pub size: HeaderSize,
    pub zones: [Vec<HeaderEntry>; 3],
    pub show_menu_labels: bool,
    next_id: u32,
}
impl Default for HeaderLayout {
    fn default() -> Self {
        Self::from_items(
            HeaderSize::Small,
            true,
            [
                vec![HeaderItem::Capy, HeaderItem::MenuLabels],
                vec![HeaderItem::DocumentTitle],
                vec![
                    HeaderItem::Workspaces,
                    HeaderItem::Clock,
                    HeaderItem::Battery,
                    HeaderItem::Menu,
                ],
            ],
        )
    }
}
impl HeaderLayout {
    pub fn context_menu(&self, id: Option<u32>) -> Result<ContextMenu, String> {
        let entry =
            |label: &str, action: HeaderAction| ContextMenuItem::command(label, action.action());
        let mut sections = vec![vec![ContextMenuItem::command(
            "Customize Workspace UI…",
            UiAction::Invoke {
                command: CommandId::CustomizeWorkspaceUi,
            },
        )]];
        let mut title = "Window Bar".to_string();
        if let Some(id) = id {
            let item = self.entry(id)?;
            title = item.item.label();
            if item.item == HeaderItem::Capy {
                sections.push(vec![ContextMenuItem::command(
                    "Change icon…",
                    UiAction::Preferences {
                        action: PreferenceAction::Reveal {
                            id: PreferenceId::ZenIcon,
                        },
                    },
                )]);
            }
            let (zone, index) = self.location(id).unwrap();
            sections.push(
                HeaderZone::ALL
                    .into_iter()
                    .map(|zone| {
                        entry(
                            &format!("Move to {}", zone.label()),
                            HeaderAction::Move {
                                id,
                                zone,
                                before: None,
                            },
                        )
                    })
                    .collect(),
            );
            let items = &self.zones[zone.index()];
            let mut earlier = entry(
                "Move Earlier",
                HeaderAction::Move {
                    id,
                    zone,
                    before: index.checked_sub(1).map(|i| items[i].id),
                },
            );
            earlier.enabled = index > 0;
            let mut later = entry(
                "Move Later",
                HeaderAction::Move {
                    id,
                    zone,
                    before: items.get(index + 2).map(|e| e.id),
                },
            );
            later.enabled = index + 1 < items.len();
            sections.push(vec![
                earlier,
                later,
                entry("Remove from Window Bar", HeaderAction::Remove { id }),
            ]);
        }
        Ok(ContextMenu { title, sections })
    }
    fn from_items(size: HeaderSize, show_menu_labels: bool, zones: [Vec<HeaderItem>; 3]) -> Self {
        let mut next_id = 1;
        let zones = zones.map(|zone| {
            zone.into_iter()
                .map(|item| {
                    let entry = HeaderEntry { id: next_id, item };
                    next_id += 1;
                    entry
                })
                .collect()
        });
        Self {
            size,
            zones,
            show_menu_labels,
            next_id,
        }
    }
    pub fn painter() -> Self {
        use HeaderItem::*;
        let tool = |command| Tool {
            control: ToolbarControl::Command { command },
        };
        Self::from_items(
            HeaderSize::Medium,
            false,
            [
                vec![
                    Capy,
                    Menu,
                    Tool {
                        control: ToolbarControl::Panel {
                            panel: Panel::Adjustments,
                        },
                    },
                    tool(CommandId::Lasso),
                    tool(CommandId::ScaleRotate),
                ],
                vec![Workspaces],
                vec![
                    tool(CommandId::Brush),
                    tool(CommandId::Blend),
                    tool(CommandId::Eraser),
                    Tool {
                        control: ToolbarControl::Panel {
                            panel: Panel::Layers,
                        },
                    },
                    Tool {
                        control: ToolbarControl::Color,
                    },
                ],
            ],
        )
    }
    pub fn entries(&self) -> impl Iterator<Item = &HeaderEntry> {
        self.zones.iter().flatten()
    }
    pub fn entry(&self, id: u32) -> Result<&HeaderEntry, String> {
        self.entries()
            .find(|e| e.id == id)
            .ok_or_else(|| "The window-bar item no longer exists".into())
    }
    pub fn location(&self, id: u32) -> Option<(HeaderZone, usize)> {
        HeaderZone::ALL.into_iter().find_map(|zone| {
            self.zones[zone.index()]
                .iter()
                .position(|e| e.id == id)
                .map(|i| (zone, i))
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        let entries: Vec<_> = self.entries().collect();
        if entries.len() > 128 || self.next_id == 0 || self.next_id == u32::MAX {
            return Err("Too many window-bar items".into());
        }
        for (i, e) in entries.iter().enumerate() {
            if e.id == 0
                || e.id >= self.next_id
                || entries[..i]
                    .iter()
                    .any(|p| p.id == e.id || (e.item.singleton() && p.item == e.item))
            {
                return Err("Invalid window-bar item identity".into());
            }
            if let HeaderItem::Tool { control } = e.item {
                control.validate()?;
            }
        }
        Ok(())
    }
    pub fn add(
        &mut self,
        zone: HeaderZone,
        before: Option<u32>,
        items: &[HeaderItem],
    ) -> Result<(), String> {
        let mut next = self.clone();
        let index = next.insertion(zone, before)?;
        let mut entries = Vec::new();
        for item in items {
            entries.push(HeaderEntry {
                id: next.next_id,
                item: *item,
            });
            next.next_id = next
                .next_id
                .checked_add(1)
                .ok_or("Window-bar item IDs exhausted")?;
        }
        next.zones[zone.index()].splice(index..index, entries);
        if items.contains(&HeaderItem::MenuLabels) {
            next.show_menu_labels = true;
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn insertion(&self, zone: HeaderZone, before: Option<u32>) -> Result<usize, String> {
        let entries = &self.zones[zone.index()];
        before.map_or(Ok(entries.len()), |id| {
            entries
                .iter()
                .position(|e| e.id == id)
                .ok_or_else(|| "The destination item no longer exists".into())
        })
    }
    pub fn move_item(
        &mut self,
        id: u32,
        zone: HeaderZone,
        before: Option<u32>,
    ) -> Result<(), String> {
        let (source, index) = self
            .location(id)
            .ok_or("The window-bar item no longer exists")?;
        self.insertion(zone, before)?;
        if before == Some(id) {
            return Ok(());
        }
        let entry = self.zones[source.index()].remove(index);
        let index = self.insertion(zone, before)?;
        self.zones[zone.index()].insert(index, entry);
        Ok(())
    }
    pub fn remove(&mut self, id: u32) -> Result<(), String> {
        let (zone, index) = self
            .location(id)
            .ok_or("The window-bar item no longer exists")?;
        let removed = self.zones[zone.index()].remove(index);
        if removed.item == HeaderItem::MenuLabels {
            self.show_menu_labels = false;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CanvasInfoLayout {
    pub visible: bool,
}
impl Default for CanvasInfoLayout {
    fn default() -> Self {
        Self { visible: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeaderPresentation {
    /// Zero until the host implements this header projection.
    pub height: f32,
    pub items: Vec<HeaderItemBounds>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeaderItemBounds {
    pub id: u32,
    pub bounds: Bounds,
}

#[derive(Clone, Copy, Debug)]
pub struct HeaderMetric {
    pub id: u32,
    pub width: f32,
    pub compact: f32,
}
#[derive(Clone, Debug, Default)]
pub struct HeaderGeometry {
    pub items: Vec<HeaderItemBounds>,
    pub zones: [Bounds; 3],
    pub overflow: [Option<Bounds>; 3],
    pub hidden: [Vec<u32>; 3],
}
impl HeaderLayout {
    /// Whole-item overflow, a truly centered middle region and protected native
    /// caption areas. Hosts measure text; the allocation/overflow policy is shared.
    pub fn resolve(
        &self,
        width: f32,
        insets: [f32; 2],
        metrics: &[HeaderMetric],
        editing: bool,
    ) -> HeaderGeometry {
        let mut result = HeaderGeometry::default();
        if !width.is_finite() || width <= 0. {
            return result;
        }
        let tile = self.size.tile();
        let left = (insets[0].max(0.) + 6.).min(width);
        let right = (width - insets[1].max(0.) - 6.).max(left);
        let mid = width / 2.;
        let gap = 12.; // At least 24 logical px of window-drag space around center.
        let item_gap = 6.;
        let gaps = |count: usize| count.saturating_sub(1) as f32 * item_gap;
        // Zero-width native metrics denote unavailable informational items
        // (e.g. a desktop without a battery), not overflowed interactive items.
        let visible = |e: &&HeaderEntry| {
            editing
                || ((e.item != HeaderItem::MenuLabels || self.show_menu_labels)
                    && !metrics.iter().any(|m| m.id == e.id && m.width == 0.))
        };
        let metric = |e: &HeaderEntry, compact: bool| {
            let m = metrics.iter().find(|m| m.id == e.id);
            m.map_or(tile, |m| if compact { m.compact } else { m.width })
                .max(1.)
                .min(4096.)
        };
        let compact = |zone: usize| {
            self.zones[zone]
                .iter()
                .filter(visible)
                .map(|e| metric(e, true))
                .sum::<f32>()
                + gaps(self.zones[zone].iter().filter(visible).count())
        };
        let center_limit = ((right - left) / 3.)
            .min(2. * (mid - left).min(right - mid))
            .max(0.);
        let center_natural: f32 = self.zones[1]
            .iter()
            .filter(visible)
            .map(|e| metric(e, false))
            .sum::<f32>()
            + gaps(self.zones[1].iter().filter(visible).count());
        let center_width = center_natural
            .min(center_limit)
            .max(compact(1).min(center_limit));
        let center_left = (mid - center_width / 2.).max(left);
        let center_right = (mid + center_width / 2.).min(right);
        let y = 6.;
        result.zones = [
            Bounds {
                x: left,
                y,
                width: (center_left - gap - left).max(0.),
                height: tile,
            },
            Bounds {
                x: center_left,
                y,
                width: center_width,
                height: tile,
            },
            Bounds {
                x: center_right + gap,
                y,
                width: (right - center_right - gap).max(0.),
                height: tile,
            },
        ];
        if self.zones[1].is_empty() {
            let split = (mid).clamp(left, right);
            result.zones[0].width = (split - gap - left).max(0.);
            result.zones[1] = Bounds {
                x: split - gap,
                y,
                width: 2. * gap,
                height: tile,
            };
            result.zones[2].x = split + gap;
            result.zones[2].width = (right - split - gap).max(0.);
        }
        for zone in 0..3 {
            let bounds = result.zones[zone];
            let entries: Vec<_> = self.zones[zone].iter().filter(visible).collect();
            let total: f32 =
                entries.iter().map(|e| metric(e, false)).sum::<f32>() + gaps(entries.len());
            let compacting = total > bounds.width;
            let widths: Vec<_> = entries.iter().map(|e| metric(e, compacting)).collect();
            let overflow = widths.iter().sum::<f32>() + gaps(widths.len()) > bounds.width;
            let overflow_width = tile.min(bounds.width);
            let available = (bounds.width
                - if overflow {
                    overflow_width + item_gap
                } else {
                    0.
                })
            .max(0.);
            let mut used = 0.;
            let mut shown = Vec::new();
            let mut full = false;
            for (entry, w) in entries.into_iter().zip(widths) {
                let spacing = if shown.is_empty() { 0. } else { item_gap };
                if full || used + spacing + w > available + 0.01 {
                    full = true;
                    result.hidden[zone].push(entry.id);
                } else {
                    shown.push((entry.id, w));
                    used += spacing + w;
                }
            }
            let total = used
                + if overflow {
                    overflow_width + if shown.is_empty() { 0. } else { item_gap }
                } else {
                    0.
                };
            let mut x = bounds.x
                + match zone {
                    1 => (bounds.width - total) / 2.,
                    2 => bounds.width - total,
                    _ => 0.,
                };
            for (id, w) in shown {
                result.items.push(HeaderItemBounds {
                    id,
                    bounds: Bounds {
                        x,
                        y,
                        width: w,
                        height: tile,
                    },
                });
                x += w + item_gap;
            }
            if overflow && overflow_width > 0. {
                result.overflow[zone] = Some(Bounds {
                    x,
                    y,
                    width: overflow_width,
                    height: tile,
                });
            }
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HeaderAction {
    Edit {
        editing: bool,
    },
    Cancel,
    SetSize {
        size: HeaderSize,
    },
    Add {
        zone: HeaderZone,
        before: Option<u32>,
        item: HeaderItem,
    },
    Move {
        id: u32,
        zone: HeaderZone,
        before: Option<u32>,
    },
    Remove {
        id: u32,
    },
    ShowMenuLabels {
        visible: bool,
    },
    CanvasInfo {
        visible: bool,
    },
    RestoreDefaults,
}
impl HeaderAction {
    pub fn action(self) -> UiAction {
        UiAction::Customize {
            action: CustomizationAction::Header { action: self },
        }
    }
}

/// The same selected/enabled policy is used by toolbar and window-bar tools.
pub fn tool_state(state: &UiState, control: ToolbarControl) -> (bool, bool) {
    match control {
        ToolbarControl::Command { command } => state
            .commands
            .iter()
            .find(|c| c.id == command)
            .map_or((false, false), |c| (c.enabled, c.selected)),
        ToolbarControl::Brush { id } => (
            true,
            state.brush.preset == id && state.layer_tools.tool == LayerCanvasTool::Paint,
        ),
        ToolbarControl::Size { pixels } => {
            (true, (state.brush.diameter - pixels as f32).abs() < 0.01)
        }
        ToolbarControl::Panel { panel } => (panel.available_on(state.platform), false),
        _ => (true, false),
    }
}
pub fn header_tool_catalog(platform: Platform) -> Vec<HeaderItem> {
    crate::customization::tool_catalog(platform)
        .into_iter()
        .map(|c| HeaderItem::Tool { control: c.control })
        .collect()
}

impl<R: layer_render::CanvasRenderer> UiSession<R> {
    pub(crate) fn activate_tool(
        &mut self,
        control: ToolbarControl,
        anchor: DrawerAnchor,
    ) -> Result<UiChange, String> {
        if control == ToolbarControl::Divider {
            return Ok(UiChange::default());
        }
        let (enabled, selected) = tool_state(self.state(), control);
        if !enabled {
            return Err("This tool is unavailable".into());
        }
        let open = self.state().customization.drawer.as_ref().map(|d| d.anchor);
        let has_drawer = control.drawer_columns().is_some();
        let activate = control.selectable() && !selected;
        let switching = match anchor {
            DrawerAnchor::Header { id } => {
                open.is_some_and(|old| matches!(old, DrawerAnchor::Header { id: old } if old != id))
            }
            DrawerAnchor::Tile { panel, tile } => {
                self.switches_toolbar_drawer(TileAnchor { panel, tile })
            }
            _ => false,
        };
        let mut change = if activate || !has_drawer {
            self.dispatch(control.action().ok_or("This tool is unavailable")?)?
        } else {
            UiChange::default()
        };
        if has_drawer && (!activate || switching || open == Some(anchor)) {
            let action = match anchor {
                DrawerAnchor::Header { id } => CustomizationAction::ToggleHeaderDrawer { id },
                DrawerAnchor::Tile { panel, tile } => CustomizationAction::ToggleToolDrawer {
                    anchor: TileAnchor { panel, tile },
                },
                _ => return Err("Not a tool origin".into()),
            };
            let update = self.dispatch(UiAction::Customize { action })?;
            change.regions |= update.regions;
            change.canvas_wake |= update.canvas_wake;
        }
        Ok(change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_are_atomic_and_ids_are_not_reused() {
        let mut h = HeaderLayout::painter();
        let original = h.clone();
        assert!(h.add(HeaderZone::Left, None, &[HeaderItem::Menu]).is_err());
        assert_eq!(h, original);
        assert!(h.move_item(1, HeaderZone::Right, Some(1)).is_err());
        assert_eq!(h, original);
        assert!(h.move_item(99, HeaderZone::Right, None).is_err());
        assert_eq!(h, original);
        h.move_item(2, HeaderZone::Center, None).unwrap();
        assert_eq!(h.location(2), Some((HeaderZone::Center, 1)));
        h.remove(2).unwrap();
        h.add(HeaderZone::Left, None, &[HeaderItem::Menu]).unwrap();
        assert!(h.entries().find(|e| e.item == HeaderItem::Menu).unwrap().id > 11);
        h.validate().unwrap();
        let saved = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<HeaderLayout>(&saved).unwrap(), h);
    }
    #[test]
    fn hiding_labels_and_removing_the_component_agree() {
        let mut h = HeaderLayout::default();
        let id = h
            .entries()
            .find(|e| e.item == HeaderItem::MenuLabels)
            .unwrap()
            .id;
        h.remove(id).unwrap();
        assert!(!h.show_menu_labels);
        h.add(HeaderZone::Right, None, &[HeaderItem::MenuLabels])
            .unwrap();
        assert!(h.show_menu_labels);
    }
    #[test]
    fn every_item_has_one_visible_or_overflow_destination_at_every_size() {
        for size in HeaderSize::ALL {
            for width in [320., 640., 800., 1200., 1600., 3200.] {
                for insets in [[0., 36.], [96., 0.], [80., 120.]] {
                    for editing in [false, true] {
                        let mut h = HeaderLayout::painter();
                        h.size = size;
                        let metrics = h
                            .entries()
                            .map(|e| HeaderMetric {
                                id: e.id,
                                width: if e.item == HeaderItem::Workspaces {
                                    420.
                                } else {
                                    size.tile()
                                },
                                compact: if e.item == HeaderItem::Workspaces {
                                    144.
                                } else {
                                    size.tile()
                                },
                            })
                            .collect::<Vec<_>>();
                        let g = h.resolve(width, insets, &metrics, editing);
                        for e in h.entries() {
                            assert_eq!(
                                g.items.iter().filter(|m| m.id == e.id).count()
                                    + g.hidden.iter().flatten().filter(|id| **id == e.id).count(),
                                1
                            );
                        }
                        let rects: Vec<_> = g
                            .items
                            .iter()
                            .map(|m| m.bounds)
                            .chain(g.overflow.into_iter().flatten())
                            .collect();
                        for (i, b) in rects.iter().enumerate() {
                            assert!(
                                b.x >= insets[0] && b.x + b.width <= width - insets[1] + 0.01,
                                "{b:?}"
                            );
                            for other in &rects[..i] {
                                assert!(
                                    b.intersection(*other).is_none(),
                                    "overlap {b:?} {other:?}"
                                );
                            }
                        }
                        if let Some(center) = g.items.iter().find(|m| m.id == h.zones[1][0].id) {
                            assert!(
                                (center.bounds.x + center.bounds.width / 2. - width / 2.).abs()
                                    < 0.01
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn native_measurements_are_not_portable_workspace_content() {
        let mut layout = DockLayout::default();
        layout.header_presentation = HeaderPresentation {
            height: 60.,
            items: vec![HeaderItemBounds {
                id: 1,
                bounds: Bounds {
                    x: 6.,
                    y: 6.,
                    width: 48.,
                    height: 48.,
                },
            }],
        };
        let history = LayoutHistory::new(&layout);
        history.validate().unwrap();
        assert_eq!(
            history.layout().header_presentation,
            HeaderPresentation::default()
        );
        assert!(
            !serde_json::to_string(&layout)
                .unwrap()
                .contains("header_presentation")
        );
    }
}
