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
    Settings,
    Fullscreen,
    Workspaces,
    DocumentTitle,
    Clock,
    Battery,
    Space,
    Tool { control: ToolbarControl },
}
impl HeaderItem {
    /// Native hosts retain their OS/menu fullscreen actions, not a title-bar tile.
    pub fn available_on(self, platform: Platform) -> bool {
        match self {
            Self::Fullscreen => platform == Platform::Web,
            Self::Menu | Self::MenuLabels => platform != Platform::Mac,
            _ => true,
        }
    }

    pub const COMPONENTS: [Self; 10] = [
        Self::Capy,
        Self::Menu,
        Self::MenuLabels,
        Self::Settings,
        Self::Fullscreen,
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
            Self::Settings => "Settings",
            Self::Fullscreen => "Full Screen",
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
    pub fn joins_bar(self) -> bool {
        matches!(
            self,
            Self::Menu | Self::Settings | Self::Fullscreen | Self::Tool { .. }
        )
    }
    pub fn has_bar(self) -> bool {
        self.joins_bar() || self == Self::Capy
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
    next_id: u32,
}
impl Default for HeaderLayout {
    fn default() -> Self {
        Self::from_items(
            HeaderSize::Small,
            [
                vec![HeaderItem::Capy, HeaderItem::MenuLabels],
                vec![HeaderItem::DocumentTitle],
                vec![
                    HeaderItem::Workspaces,
                    HeaderItem::Clock,
                    HeaderItem::Battery,
                    HeaderItem::Settings,
                ],
            ],
        )
    }
}
impl HeaderLayout {
    /// Omit host-inapplicable controls from presentation and editing without
    /// changing the portable, saved arrangement or its stable item identities.
    pub fn projected_for(&self, platform: Platform) -> Self {
        let mut header = self.clone();
        for zone in &mut header.zones {
            zone.retain(|entry| entry.item.available_on(platform));
        }
        header
    }

    pub fn for_platform(platform: Platform) -> Self {
        Self::default().with_platform_controls(platform)
    }

    pub fn painter_for_platform(platform: Platform) -> Self {
        let mut header = Self::painter();
        if CommandId::Select.available_on(platform) { header.replace_tool(CommandId::Lasso, CommandId::Select); }
        if !CommandId::DrawingBrush.available_on(platform) {
            header.replace_tool(CommandId::DrawingBrush, CommandId::Brush);
            header.replace_tool(CommandId::Sculpt, CommandId::Blend);
        }
        header.with_platform_controls(platform)
    }

    pub(crate) fn replace_tool(&mut self, old: CommandId, new: CommandId) {
        let item = |command| HeaderItem::Tool { control: ToolbarControl::Command { command } };
        for entry in self.zones.iter_mut().flatten() {
            if entry.item == item(old) { entry.item = item(new); }
        }
    }

    fn with_platform_controls(mut self, platform: Platform) -> Self {
        if platform == Platform::Web {
            let settings = self.zones[2]
                .iter()
                .find(|entry| entry.item == HeaderItem::Settings)
                .map(|entry| entry.id);
            self.add(HeaderZone::Right, settings, &[HeaderItem::Fullscreen])
                .unwrap();
        }
        self.projected_for(platform)
    }

    pub fn context_menu(&self, id: Option<u32>, editing: bool) -> Result<ContextMenu, String> {
        let entry =
            |label: &str, action: HeaderAction| ContextMenuItem::command(label, action.action());
        let mut sections = if editing {
            Vec::new()
        } else {
            vec![vec![ContextMenuItem::command(
                "Customize Title Bar…",
                UiAction::Invoke {
                    command: CommandId::CustomizeWorkspaceUi,
                },
            )]]
        };
        let mut title = "Title Bar".to_string();
        if let Some(id) = id {
            let item = self.entry(id)?;
            title = item.item.label();
            if item.item == HeaderItem::Capy && !editing {
                sections.push(vec![ContextMenuItem::command(
                    "Change icon…",
                    UiAction::Preferences {
                        action: PreferenceAction::Reveal {
                            id: PreferenceId::ZenIcon,
                        },
                    },
                )]);
            }
            if !editing {
                return Ok(ContextMenu { title, sections });
            }
            let (zone, index) = self.location(id).unwrap();
            sections.push(
                HeaderZone::ALL
                    .into_iter()
                    .map(|destination| {
                        let mut item = entry(
                            &format!("Move to {}", destination.label()),
                            HeaderAction::Move {
                                id,
                                zone: destination,
                                before: None,
                            },
                        );
                        item.enabled = destination != zone;
                        item
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
                entry("Remove from Title Bar", HeaderAction::Remove { id }),
            ]);
        }
        if editing {
            sections.push(vec![
                entry("Done", HeaderAction::Edit { editing: false }),
                entry("Cancel Changes", HeaderAction::Cancel),
            ]);
        }
        Ok(ContextMenu { title, sections })
    }
    fn from_items(size: HeaderSize, zones: [Vec<HeaderItem>; 3]) -> Self {
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
                    tool(CommandId::DrawingBrush),
                    tool(CommandId::Sculpt),
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
            .ok_or_else(|| "The title-bar item no longer exists".into())
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
            return Err("Too many title-bar items".into());
        }
        for (i, e) in entries.iter().enumerate() {
            if e.id == 0
                || e.id >= self.next_id
                || entries[..i]
                    .iter()
                    .any(|p| p.id == e.id || (e.item.singleton() && p.item == e.item))
            {
                return Err("Invalid title-bar item identity".into());
            }
            if let HeaderItem::Tool { control } = e.item {
                control.validate()?;
                if control.is_component() {
                    return Err("Place this component in a toolbar".into());
                }
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
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub(crate) fn insertion(&self, zone: HeaderZone, before: Option<u32>) -> Result<usize, String> {
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
            .ok_or("The title-bar item no longer exists")?;
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
            .ok_or("The title-bar item no longer exists")?;
        self.zones[zone.index()].remove(index);
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct HeaderMetric {
    pub id: u32,
    pub width: f32,
    pub compact: f32,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HeaderGeometry {
    pub items: Vec<HeaderItemBounds>,
    pub zones: [Bounds; 3],
    pub overflow: [Option<Bounds>; 3],
    pub hidden: [Vec<u32>; 3],
    #[serde(default)]
    pub bars: Vec<HeaderBar>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeaderBar {
    pub bounds: Bounds,
    pub items: Vec<u32>,
    pub overflow: Option<usize>,
}

impl HeaderGeometry {
    /// Pack neighboring items toward the edges and give the document the space
    /// between them, including in custom side zones. Controls and drag gutters
    /// stay outside this allocation; customization uses the original geometry.
    pub fn expand_document(&mut self, id: u32, width: f32, insets: [f32; 2]) {
        let mut slots: Vec<_> = self.items.iter().map(|i| (Some(i.id), None, i.bounds))
            .chain(self.overflow.iter().enumerate().filter_map(|(i, b)| b.map(|b| (None, Some(i), b))))
            .collect();
        slots.sort_by(|a, b| a.2.x.total_cmp(&b.2.x));
        let Some(index) = slots.iter().position(|s| s.0 == Some(id)) else { return; };
        let bar = |slot: &(Option<u32>, Option<usize>, Bounds)| self.bars.iter().position(|b| {
            slot.0.is_some_and(|id| b.items.contains(&id)) || slot.1.is_some() && b.overflow == slot.1
        });
        let gaps: Vec<_> = slots.windows(2)
            .map(|p| if bar(&p[0]).is_some() && bar(&p[0]) == bar(&p[1]) { 0. } else { 6. })
            .collect();
        let mut left = insets[0] + 6.;
        let mut right = width - insets[1] - 6.;
        for (i, slot) in slots[..index].iter_mut().enumerate() {
            slot.2.x = left;
            left += slot.2.width + gaps[i];
        }
        for (i, slot) in slots.iter_mut().enumerate().skip(index + 1).rev() {
            right -= slot.2.width;
            slot.2.x = right;
            right -= gaps[i - 1];
        }
        let left = left + if index == 0 { 12. } else { 6. };
        let right = right - if index + 1 == slots.len() { 12. } else { 6. };
        if right <= left { return; }
        slots[index].2.x = left;
        slots[index].2.width = right - left;
        for (item, overflow, bounds) in slots {
            if let Some(id) = item {
                self.items.iter_mut().find(|i| i.id == id).unwrap().bounds = bounds;
            } else if let Some(index) = overflow {
                self.overflow[index] = Some(bounds);
            }
        }
        for i in 0..self.bars.len() {
            self.bars[i].bounds = self.bar_bounds(&self.bars[i]);
        }
    }
    fn bar_bounds(&self, bar: &HeaderBar) -> Bounds {
        let members = self
            .items
            .iter()
            .filter(|i| bar.items.contains(&i.id))
            .map(|i| i.bounds)
            .chain(bar.overflow.and_then(|zone| self.overflow[zone]));
        let (mut left, mut right, mut top, mut bottom) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for b in members {
            left = left.min(b.x);
            right = right.max(b.x + b.width);
            top = top.min(b.y);
            bottom = bottom.max(b.y + b.height);
        }
        Bounds {
            x: left,
            y: top + 1.,
            width: right - left,
            height: bottom - top - 2.,
        }
    }
}

/// Toolkit-independent title-bar content. Hosts supply text measurements and
/// device status; component availability and tool semantics stay in the session.
#[derive(Serialize)]
pub struct HeaderView {
    pub model: HeaderLayout,
    pub editing: bool,
    pub items: Vec<HeaderItemView>,
    pub sizes: Vec<HeaderSizeView>,
    pub components: Vec<HeaderComponentView>,
    pub primary_menu: ContextMenu,
}
#[derive(Serialize)]
pub struct HeaderItemView {
    pub id: u32,
    pub label: String,
    pub enabled: bool,
    pub selected: bool,
    pub icon: &'static str,
}
#[derive(Serialize)]
pub struct HeaderSizeView {
    pub id: HeaderSize,
    pub label: &'static str,
    pub tile: f32,
    pub icon: i32,
    pub height: f32,
}
#[derive(Serialize)]
pub struct HeaderComponentView {
    pub item: HeaderItem,
    pub label: String,
    pub singleton: bool,
}
impl<R: layer_render::CanvasRenderer> UiSession<R> {
    pub fn header_view(&self) -> HeaderView {
        let state = self.state();
        let model = state.workspace.layout.header.projected_for(state.platform);
        let items = model
            .entries()
            .map(|entry| {
                let (enabled, selected, icon) = match entry.item {
                    HeaderItem::Tool { control } => {
                        let (enabled, selected) = tool_state(state, control);
                        (enabled, selected, tool_icon(state, control))
                    }
                    HeaderItem::Capy => (true, state.workspace.zen_mode, ""),
                    _ => (true, false, ""),
                };
                HeaderItemView {
                    id: entry.id,
                    label: entry.item.label(),
                    enabled,
                    selected,
                    icon,
                }
            })
            .collect();
        HeaderView {
            model,
            items,
            editing: state.customization.header_editing,
            sizes: HeaderSize::ALL
                .into_iter()
                .map(|id| HeaderSizeView {
                    id,
                    label: id.label(),
                    tile: id.tile(),
                    icon: id.icon(),
                    height: id.height(),
                })
                .collect(),
            components: HeaderItem::COMPONENTS
                .into_iter()
                .filter(|item| item.available_on(state.platform))
                .map(|item| HeaderComponentView {
                    item,
                    label: item.label(),
                    singleton: item.singleton(),
                })
                .collect(),
            primary_menu: self.application_menu(ApplicationMenu::Primary),
        }
    }
}
impl HeaderLayout {
    /// Drawing tabs consume the remaining title interval on every host. The
    /// single-title and customization presentations keep their normal geometry.
    pub fn resolve_documents(
        &self,
        width: f32,
        insets: [f32; 2],
        metrics: &[HeaderMetric],
        editing: bool,
        documents: usize,
    ) -> HeaderGeometry {
        let mut geometry = self.resolve(width, insets, metrics, editing);
        if !editing && documents > 1
            && let Some(entry) = self.entries().find(|e| e.item == HeaderItem::DocumentTitle)
        {
            geometry.expand_document(entry.id, width, insets);
        }
        geometry
    }

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
        let joins = |e: &HeaderEntry| !editing && e.item.joins_bar();
        let barred = |e: &HeaderEntry| !editing && e.item.has_bar();
        let spacing =
            |a: &HeaderEntry, b: &HeaderEntry| if joins(a) && joins(b) { 0. } else { item_gap };
        // Zero-width native metrics denote unavailable informational items
        // (e.g. a desktop without a battery), not overflowed interactive items.
        let visible = |e: &&HeaderEntry| !metrics.iter().any(|m| m.id == e.id && m.width == 0.);
        let metric = |e: &HeaderEntry, compact: bool| {
            let m = metrics.iter().find(|m| m.id == e.id);
            let width = m.map_or(tile, |m| if compact { m.compact } else { m.width });
            // Invalid native measurements must not introduce NaN into geometry.
            if width.is_nan() {
                1.
            } else {
                width.clamp(1., 4096.)
            }
        };
        let span = |zone: usize, compact: bool| {
            let entries: Vec<_> = self.zones[zone].iter().filter(visible).collect();
            entries.iter().map(|e| metric(e, compact)).sum::<f32>()
                + entries.windows(2).map(|p| spacing(p[0], p[1])).sum::<f32>()
        };
        let center_limit = ((right - left) / 3.)
            .min(2. * (mid - left).min(right - mid))
            .max(0.);
        let center_width = span(1, false)
            .min(center_limit)
            .max(span(1, true).min(center_limit));
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
            // Reserve a generous visible drop target, not a tiny fixed slot.
            // Scale with the usable width, retaining true centering and room
            // for both side regions and protected native window controls.
            let half = if editing {
                center_limit.min(320.) / 2.
            } else {
                gap
            };
            let spacing = if editing { gap } else { 0. };
            result.zones[0].width = (split - half - spacing - left).max(0.);
            result.zones[1] = Bounds {
                x: split - half,
                y,
                width: 2. * half,
                height: tile,
            };
            result.zones[2].x = (split + half + spacing).min(right);
            result.zones[2].width = (right - result.zones[2].x).max(0.);
        }
        for zone in 0..3 {
            let bounds = result.zones[zone];
            let entries: Vec<_> = self.zones[zone].iter().filter(visible).collect();
            let compacting = span(zone, false) > bounds.width;
            let widths: Vec<_> = entries.iter().map(|e| metric(e, compacting)).collect();
            let overflow = span(zone, compacting) > bounds.width;
            let overflow_width = tile.min(bounds.width);
            let available = (bounds.width
                - if overflow {
                    overflow_width + item_gap
                } else {
                    0.
                })
            .max(0.);
            let mut used = 0.;
            let mut shown: Vec<(&HeaderEntry, f32, f32)> = Vec::new();
            let mut full = false;
            for (entry, w) in entries.into_iter().zip(widths) {
                let gap = shown.last().map_or(0., |p| spacing(p.0, entry));
                if full || used + gap + w > available + 0.01 {
                    full = true;
                    result.hidden[zone].push(entry.id);
                } else {
                    shown.push((entry, w, gap));
                    used += gap + w;
                }
            }
            let overflow_gap = match shown.last() {
                None => 0.,
                Some(p) if joins(p.0) => 0.,
                Some(_) => item_gap,
            };
            let total = used
                + if overflow {
                    overflow_width + overflow_gap
                } else {
                    0.
                };
            let mut x = bounds.x
                + match zone {
                    1 => (bounds.width - total) / 2.,
                    2 => bounds.width - total,
                    _ => 0.,
                };
            let mut bar: Option<HeaderBar> = None;
            for (entry, w, gap) in shown {
                x += gap;
                result.items.push(HeaderItemBounds {
                    id: entry.id,
                    bounds: Bounds {
                        x,
                        y,
                        width: w,
                        height: tile,
                    },
                });
                x += w;
                if !joins(entry) || gap > 0. {
                    result.bars.extend(bar.take());
                }
                if barred(entry) {
                    bar.get_or_insert_default().items.push(entry.id);
                }
                if !joins(entry) {
                    result.bars.extend(bar.take());
                }
            }
            if overflow && overflow_width > 0. {
                x += overflow_gap;
                result.overflow[zone] = Some(Bounds {
                    x,
                    y,
                    width: overflow_width,
                    height: tile,
                });
                if !editing {
                    if overflow_gap > 0. {
                        result.bars.extend(bar.take());
                    }
                    bar.get_or_insert_default().overflow = Some(zone);
                }
            }
            result.bars.extend(bar);
        }
        for i in 0..result.bars.len() {
            result.bars[i].bounds = result.bar_bounds(&result.bars[i]);
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
    InsertTools {
        zone: HeaderZone,
        before: Option<u32>,
    },
    Move {
        id: u32,
        zone: HeaderZone,
        before: Option<u32>,
    },
    Remove {
        id: u32,
    },
    CanvasInfo {
        visible: bool,
    },
}
impl HeaderAction {
    pub fn action(self) -> UiAction {
        UiAction::Customize {
            action: CustomizationAction::Header { action: self },
        }
    }
}

/// Publish command icons from shared tool memory to every retained projection.
pub fn tool_icon(state: &UiState, control: ToolbarControl) -> &'static str {
    if let ToolbarControl::Command { command } = control
        && let Some(icon) = state.commands.iter().find(|c| c.id == command).and_then(|c| c.icon)
    {
        return icon;
    }
    tool_choice(control).icon
}

/// The same selected/enabled policy is used by toolbar and window-bar tools.
pub fn tool_state(state: &UiState, control: ToolbarControl) -> (bool, bool) {
    match control {
        ToolbarControl::ColorPicker => (
            state.platform.color_picker() && tool_state(state, ToolbarControl::Command { command: CommandId::Eyedropper }).0,
            state.layer_tools.tool.picks_color() && state.color_picker.style == crate::ColorPickerStyle::Glass,
        ),
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
impl<R: layer_render::CanvasRenderer> UiSession<R> {
    pub(crate) fn activate_tool(
        &mut self,
        control: ToolbarControl,
        anchor: DrawerAnchor,
    ) -> Result<UiChange, String> {
        if control == ToolbarControl::Divider {
            return Ok(UiChange::default());
        }
        if control == ToolbarControl::ColorPicker {
            return self.dispatch(control.action().unwrap());
        }
        if self.state().platform.color_picker() && control == (ToolbarControl::Command { command: CommandId::Eyedropper }) {
            return self.dispatch(UiAction::Invoke { command: CommandId::Eyedropper });
        }
        if control.selectable() && self.state().platform.color_picker() && self.state().layer_tools.tool.picks_color() {
            return self.dispatch(UiAction::Invoke { command: CommandId::Eyedropper });
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
    fn native_measurement_limits_keep_geometry_finite_and_zero_width_hidden() {
        let layout = HeaderLayout::painter();
        let id = layout.zones[0][0].id;
        for (input, bounded) in [
            (f32::NAN, 1.),
            (f32::NEG_INFINITY, 1.),
            (f32::INFINITY, 4096.),
            (-20., 1.),
            (0., 0.),
            (0.5, 1.),
            (48., 48.),
            (8192., 4096.),
        ] {
            for width in [320., 1600., 24000.] {
                for editing in [false, true] {
                    let resolve = |value| {
                        layout.resolve(
                            width,
                            [0., 72.],
                            &[HeaderMetric {
                                id,
                                width: value,
                                compact: value,
                            }],
                            editing,
                        )
                    };
                    let actual = resolve(input);
                    let expected = resolve(bounded);
                    assert_eq!(actual.items, expected.items);
                    assert_eq!(actual.zones, expected.zones);
                    assert_eq!(actual.overflow, expected.overflow);
                    assert_eq!(actual.hidden, expected.hidden);
                    if input == 0. {
                        assert!(!actual.items.iter().any(|m| m.id == id));
                        assert!(!actual.hidden.iter().flatten().any(|m| *m == id));
                    }
                }
            }
        }
    }

    #[test]
    fn unavailable_items_take_no_space_even_when_customizing() {
        let mut h = HeaderLayout::for_platform(Platform::Web);
        let id = h
            .entries()
            .find(|e| e.item == HeaderItem::Fullscreen)
            .unwrap()
            .id;
        let metrics = [HeaderMetric {
            id,
            width: 0.,
            compact: 0.,
        }];
        let hidden = h.resolve(1600., [0.; 2], &metrics, false);
        h.remove(id).unwrap();
        let web = HeaderLayout::for_platform(Platform::Web);
        assert_eq!(web.projected_for(Platform::Gtk), h);
        assert_eq!(web.projected_for(Platform::Web), web);
        let battery = h
            .entries()
            .find(|e| e.item == HeaderItem::Battery)
            .unwrap()
            .id;
        assert_eq!(
            web.projected_for(Platform::Gtk).step(battery, true),
            h.step(battery, true)
        );
        assert_eq!(hidden.items, h.resolve(1600., [0.; 2], &[], false).items);
        for editing in [false, true] {
            for width in [320., 1600.] {
                let portable = HeaderLayout::for_platform(Platform::Web);
                let g = portable.resolve(width, [0.; 2], &metrics, editing);
                assert!(!g.items.iter().any(|item| item.id == id));
                let removed = h.resolve(width, [0.; 2], &[], editing);
                assert_eq!(g.items, removed.items);
                assert_eq!(g.zones, removed.zones);
                assert_eq!(g.overflow, removed.overflow);
                assert_eq!(g.hidden, removed.hidden);
            }
        }
    }

    #[test]
    fn context_actions_match_editing_state_and_actual_region() {
        let h = HeaderLayout::painter();
        let capy = h.zones[0][0].id;
        let normal = h.context_menu(Some(capy), false).unwrap();
        let labels = normal
            .sections
            .iter()
            .flatten()
            .map(|i| i.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["Customize Title Bar…", "Change icon…"]);
        for id in [None, Some(capy), Some(h.zones[2][0].id)] {
            let menu = h.context_menu(id, true).unwrap();
            let items = menu.sections.iter().flatten().collect::<Vec<_>>();
            assert!(items.iter().any(|i| i.label == "Done"));
            assert!(items.iter().any(|i| i.label == "Cancel Changes"));
            assert!(
                !items
                    .iter()
                    .any(|i| i.label.contains("Customize") || i.label.contains("icon"))
            );
            if let Some(id) = id {
                let (zone, index) = h.location(id).unwrap();
                for destination in HeaderZone::ALL {
                    let item = items
                        .iter()
                        .find(|i| i.label == format!("Move to {}", destination.label()))
                        .unwrap();
                    assert_eq!(item.enabled, destination != zone);
                }
                assert_eq!(
                    items
                        .iter()
                        .find(|i| i.label == "Move Earlier")
                        .unwrap()
                        .enabled,
                    index > 0
                );
            } else {
                assert!(
                    !items
                        .iter()
                        .any(|i| i.label.starts_with("Move") || i.label.starts_with("Remove"))
                );
            }
        }
        assert!(h.context_menu(Some(u32::MAX), true).is_err());
    }
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
    fn menu_labels_are_present_or_absent_not_separately_hidden() {
        let mut h = HeaderLayout::default();
        let id = h
            .entries()
            .find(|e| e.item == HeaderItem::MenuLabels)
            .unwrap()
            .id;
        h.remove(id).unwrap();
        assert!(!h.entries().any(|e| e.item == HeaderItem::MenuLabels));
        h.add(HeaderZone::Right, None, &[HeaderItem::MenuLabels])
            .unwrap();
        assert!(h.entries().any(|e| e.item == HeaderItem::MenuLabels));
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
    fn adjacent_icon_controls_share_bars_outside_customization() {
        use HeaderItem::*;
        let tool = |command| Tool {
            control: ToolbarControl::Command { command },
        };
        let h = HeaderLayout::from_items(
            HeaderSize::Small,
            [
                vec![
                    Capy,
                    Menu,
                    tool(CommandId::Lasso),
                    Space,
                    tool(CommandId::ScaleRotate),
                ],
                vec![DocumentTitle],
                vec![
                    tool(CommandId::DrawingBrush),
                    tool(CommandId::Eraser),
                    Workspaces,
                    Settings,
                ],
            ],
        );
        let id = |zone: usize, i: usize| h.zones[zone][i].id;
        let at = |g: &HeaderGeometry, id| g.items.iter().find(|i| i.id == id).unwrap().bounds;
        let contiguous = |g: &HeaderGeometry| {
            for bar in &g.bars {
                let mut members: Vec<_> = bar
                    .items
                    .iter()
                    .map(|id| at(g, *id))
                    .chain(bar.overflow.and_then(|zone| g.overflow[zone]))
                    .collect();
                members.sort_by(|a, b| a.x.total_cmp(&b.x));
                for pair in members.windows(2) {
                    assert_eq!(pair[0].x + pair[0].width, pair[1].x, "{bar:?}");
                }
                let (first, last) = (members[0], members[members.len() - 1]);
                assert_eq!(
                    bar.bounds,
                    Bounds {
                        x: first.x,
                        y: first.y + 1.,
                        width: last.x + last.width - first.x,
                        height: first.height - 2.,
                    }
                );
            }
        };
        let g = h.resolve(1600., [0.; 2], &[], false);
        assert_eq!(
            g.bars.iter().map(|b| b.items.clone()).collect::<Vec<_>>(),
            vec![
                vec![id(0, 0)],
                vec![id(0, 1), id(0, 2)],
                vec![id(0, 4)],
                vec![id(2, 0), id(2, 1)],
                vec![id(2, 3)]
            ]
        );
        assert_eq!(
            g.bars[1].bounds.height, 34.,
            "matches the workspace switcher"
        );
        assert_eq!(
            at(&g, id(0, 1)).x,
            at(&g, id(0, 0)).x + 42.,
            "Capy stays separate"
        );
        assert_eq!(
            at(&g, id(0, 3)).x,
            at(&g, id(0, 2)).x + 42.,
            "Space separates bars"
        );
        contiguous(&g);
        contiguous(&h.resolve_documents(1600., [0.; 2], &[], false, 3));

        let editing = h.resolve(1600., [0.; 2], &[], true);
        assert!(editing.bars.is_empty());
        assert_eq!(at(&editing, id(0, 2)).x, at(&editing, id(0, 1)).x + 42.);

        let mut joined = false;
        for width in (300..900).step_by(12) {
            let g = h.resolve(width as f32, [0.; 2], &[], false);
            contiguous(&g);
            if let Some(overflow) = g.overflow[2] {
                let bar = g
                    .bars
                    .iter()
                    .find(|b| b.overflow == Some(2))
                    .expect("overflow joins a bar");
                if let Some(last) = bar.items.last() {
                    joined = true;
                    assert_eq!(at(&g, *last).x + at(&g, *last).width, overflow.x);
                }
            }
        }
        assert!(
            joined,
            "a partly hidden tool bar ends with its overflow button"
        );
    }
    #[test]
    fn empty_center_drop_target_scales_with_width_without_covering_native_controls() {
        for size in HeaderSize::ALL {
            for width in [320., 640., 800., 1600., 3200.] {
                for insets in [[0., 72.], [96., 0.], [80., 120.]] {
                    let mut h = HeaderLayout::painter();
                    h.size = size;
                    h.zones[1].clear();
                    let g = h.resolve(width, insets, &[], true);
                    let b = g.zones[1];
                    assert!((b.x + b.width / 2. - width / 2.).abs() < 0.01);
                    let usable = width - insets[0] - insets[1] - 12.;
                    let centered =
                        2. * (width / 2. - insets[0] - 6.).min(width / 2. - insets[1] - 6.);
                    assert!((b.width - (usable / 3.).min(centered).min(320.)).abs() < 0.01);
                    for x in [b.x + 1., b.x + b.width / 2., b.x + b.width - 1.] {
                        assert_eq!(
                            g.destination([x, size.height() / 2.], size.height()),
                            Some((HeaderZone::Center, None))
                        );
                    }
                    for rect in g
                        .items
                        .iter()
                        .map(|i| i.bounds)
                        .chain(g.overflow.into_iter().flatten())
                    {
                        assert!(rect.intersection(b).is_none());
                        assert!(
                            rect.x >= insets[0] && rect.x + rect.width <= width - insets[1] + 0.01
                        );
                    }
                    let normal = h.resolve(width, insets, &[], false);
                    assert_eq!(
                        normal.zones[1].width, 24.,
                        "extra space is only reserved while editing"
                    );
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
