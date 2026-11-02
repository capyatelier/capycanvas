use crate::{Bounds, DockLayout, DrawerPlacement, Edge, ToolbarControl};
use serde::{Deserialize, Serialize};

pub(crate) fn tool_allowed(control: ToolbarControl) -> bool {
    !control.is_component() && control != PhoneLayout::SETTINGS
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhoneState {
    pub(crate) platform: Option<crate::Platform>,
    pub insets: [f32; 4],
    pub native: [f32; 2],
    pub native_counts: [u32; 2],
    pub hidden: bool,
    pub menu: bool,
    pub capy_drag: bool,
    pub swapped: bool,
    pub slider: Option<(ToolbarControl, f32, crate::ToolbarContext)>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhoneAction {
    Capy,
    CapyHold,
    CapyDrop { position: [f32; 2] },
    Tool { tile: u32 },
    RemoveTool { tile: u32 },
    InsertTools { before: Option<u32> },
    MoveTool { tile: u32, before: Option<u32> },
    ToolSettings,
    Panel { panel: crate::Panel },
    Drawing { id: u64, close: bool },
    SwapSides,
    Slider { control: ToolbarControl, phase: crate::ContactPhase },
    Metrics { native: [f32; 2], native_counts: [u32; 2], insets: [f32; 4] },
}
impl crate::UiState {
    pub fn phone_settings(&self) -> crate::TileView {
        let choice = crate::customization::tool_choice_localized(PhoneLayout::SETTINGS, &self.localization);
        let label = choice.label.clone();
        crate::customization::control_view(self, PhoneLayout::SETTINGS, None, Default::default(), choice, &label)
    }
    pub fn phone_tools(&self) -> Vec<crate::TileView> {
        self.workspace.layout.phone.iter().flatten().map(|tile| {
            let choice = crate::customization::tool_choice_localized(tile.control, &self.localization);
            let label = choice.label.clone();
            crate::customization::tile_view(self, tile, choice, &label)
        }).collect()
    }
}
impl PhoneState {
    pub fn resolve(self, layout: &DockLayout, viewport: [f32; 2]) -> Option<PhoneLayout> {
        if !self.platform.is_some_and(crate::Platform::presents_phone_layout) { return None; }
        layout.phone.as_ref()?;
        let mut phone = PhoneLayout::resolve(layout, viewport, self.insets, self.native, self.native_counts, self.swapped)?;
        if self.hidden {
            phone.size = Bounds::default();
            phone.strength = Bounds::default();
            phone.settings = Bounds::default();
            phone.settings_hit = Bounds::default();
            phone.tools = [Bounds::default(); PhoneLayout::MAX_TILES];
            phone.tool_hits = [Bounds::default(); PhoneLayout::MAX_TILES];
            phone.chips = [Bounds::default(), phone.capy];
            phone.controls = [Bounds::default(); 2];
            phone.bars = [Bounds::default(); 2];
        }
        Some(phone)
    }
}
impl crate::UiState {
    pub fn phone_presentation(&self, viewport: [f32; 2]) -> Option<PhoneLayout> {
        self.phone.resolve(&self.workspace.layout, viewport)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhoneLayout {
    pub tile: f32,
    pub icon: f32,
    pub capy_icon: f32,
    pub edge: Edge,
    pub capy: Bounds,
    pub capy_hit: Bounds,
    pub size: Bounds,
    pub strength: Bounds,
    pub settings: Bounds,
    pub settings_hit: Bounds,
    pub tools: [Bounds; PhoneLayout::MAX_TILES],
    pub tool_hits: [Bounds; PhoneLayout::MAX_TILES],
    pub chips: [Bounds; 2],
    pub controls: [Bounds; 2],
    pub bars: [Bounds; 2],
    pub work_area: Bounds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoneBarOutcome { Pending, Held, Dragging, WindowMove, Cancel, Tap(u32), Menu(Option<u32>), Move { tile: u32, before: Option<u32> } }
#[derive(Clone, Copy, Debug, Default)]
pub struct PhoneBarContact {
    held: bool, dragged: bool, editable: bool, index: Option<usize>,
    origin: [f32; 2], point: [f32; 2], key: Option<(Edge, Bounds, Bounds)>, ids: [u32; PhoneLayout::MAX_TILES], dividers: [bool; PhoneLayout::MAX_TILES], count: usize,
}
impl PhoneBarContact {
    pub fn begin(layout: &PhoneLayout, point: [f32; 2], native_button: bool) -> Option<Self> {
        let contains = |b: &Bounds| b.width > 0. && b.height > 0. && b.contains(point[0], point[1]);
        (layout.bars.iter().chain(&layout.tool_hits).chain([&layout.capy_hit, &layout.settings_hit]).any(contains)
            && !native_button && ![layout.size, layout.strength].iter().any(contains)).then(|| Self {
                editable: ![layout.capy_hit, layout.settings_hit].iter().any(contains),
                index: layout.tool_hits.iter().position(contains).or_else(|| layout.tools.iter().position(contains)), origin: point, point,
                key: Some((layout.edge, layout.bars[0], layout.work_area)), ..Default::default()
            })
    }
    pub fn set_tools(&mut self, tiles: &[crate::ToolbarTile]) {
        self.count = tiles.len().min(PhoneLayout::MAX_TILES);
        for (i, tile) in tiles.iter().take(PhoneLayout::MAX_TILES).enumerate() { self.ids[i] = tile.id; self.dividers[i] = tile.control == ToolbarControl::Divider; }
    }
    pub fn valid<'a>(&self, layout: &PhoneLayout, tiles: impl IntoIterator<Item = &'a crate::ToolbarTile>) -> bool {
        if self.key != Some((layout.edge, layout.bars[0], layout.work_area)) { return false; }
        let mut ids = tiles.into_iter().map(|tile| tile.id);
        (0..self.count).all(|i| ids.next() == Some(self.ids[i])) && ids.next().is_none()
    }
    pub fn hold(&mut self) { self.held = true; }
    pub fn arm(&mut self) -> PhoneBarOutcome { self.hold(); if self.editable { PhoneBarOutcome::Held } else { PhoneBarOutcome::Pending } }
    pub fn cancel(&mut self) -> PhoneBarOutcome { self.key = None; PhoneBarOutcome::Cancel }
    pub fn move_window(&self, past_slop: bool) -> bool { past_slop && !self.held }
    pub fn motion(&mut self, layout: &PhoneLayout, tiles: &[crate::ToolbarTile], point: [f32; 2], past_slop: bool) -> PhoneBarOutcome {
        if !self.valid(layout, tiles) { return PhoneBarOutcome::Cancel; }
        self.point = point;
        if self.move_window(past_slop) { return PhoneBarOutcome::WindowMove; }
        if self.held && self.index.is_some() {
            self.dragged |= past_slop;
            if self.dragged { PhoneBarOutcome::Dragging } else { PhoneBarOutcome::Held }
        } else { PhoneBarOutcome::Pending }
    }
    pub fn lifted(&self) -> Option<usize> { self.index.filter(|_| self.held) }
    fn destination(&self, layout: &PhoneLayout) -> Option<u32> {
        let axis = usize::from(layout.edge != Edge::Top);
        (0..self.count).filter(|i| Some(*i) != self.index).find(|i| {
            let b = layout.tools[*i]; self.point[axis] < if axis == 0 { b.x + b.width / 2. } else { b.y + b.height / 2. }
        }).map(|i| self.ids[i])
    }
    pub fn finish(&mut self, layout: &PhoneLayout, tiles: &[crate::ToolbarTile], point: [f32; 2]) -> PhoneBarOutcome {
        if !self.valid(layout, tiles) { return PhoneBarOutcome::Cancel; }
        self.point = point;
        if self.dragged {
            let b = layout.bars[0]; let t = layout.tile;
            if !(Bounds { x: b.x-t, y: b.y-t, width: b.width+2.*t, height: b.height+2.*t }).contains(point[0],point[1]) { return PhoneBarOutcome::Cancel; }
            return PhoneBarOutcome::Move { tile: self.ids[self.index.unwrap()], before: self.destination(layout) };
        }
        if self.held && self.editable { PhoneBarOutcome::Menu(self.index.map(|i| self.ids[i])) }
        else if !self.held { self.index.map_or(PhoneBarOutcome::Pending, |i| PhoneBarOutcome::Tap(self.ids[i])) }
        else { PhoneBarOutcome::Pending }
    }
    pub fn geometry(&self, layout: &PhoneLayout) -> [Bounds; PhoneLayout::MAX_TILES] {
        let mut bounds = layout.tools;
        let Some(source) = self.index.filter(|_| self.held) else { return bounds; };
        let axis = usize::from(layout.edge != Edge::Top);
        let scale = ((if axis == 0 { layout.work_area.width } else { layout.work_area.height }) / 320.).min(1.);
        if self.dragged {
            let mut order: [usize; PhoneLayout::MAX_TILES] = std::array::from_fn(|i| i);
            let target = self.destination(layout).and_then(|id| self.ids[..self.count].iter().position(|v| *v == id)).unwrap_or(self.count);
            if target > source { order[source..target].rotate_left(1); } else { order[target..=source].rotate_right(1); }
            let first = layout.tools[0];
            let mut along = if axis == 0 { first.x } else { first.y } - PhoneLayout::tool_metrics(self.dividers[0], layout.tile, scale).0;
            for &i in &order[..self.count] {
                let (offset, _, advance) = PhoneLayout::tool_metrics(self.dividers[i], layout.tile, scale);
                if axis == 0 { bounds[i].x = along + offset; } else { bounds[i].y = along + offset; }
                along += advance;
            }
            if axis == 0 { bounds[source].x = layout.tools[source].x + self.point[0] - self.origin[0]; }
            else { bounds[source].y = layout.tools[source].y + self.point[1] - self.origin[1]; }
        }
        bounds[source].x -= 2.; bounds[source].y -= 2.; bounds[source].width += 4.; bounds[source].height += 4.;
        bounds
    }
}

impl PhoneLayout {
    pub const MAX_TOOLS: usize = 10;
    pub(crate) const MAX_TILES: usize = Self::MAX_TOOLS * 2 + 1;
    const TOOL_GAP: f32 = 2.;
    const DIVIDER_WIDTH: f32 = 8.;
    fn tool_metrics(divider: bool, tile: f32, scale: f32) -> (f32, f32, f32) {
        let gap = Self::TOOL_GAP * scale;
        if divider { let width = Self::DIVIDER_WIDTH * scale; (-gap, width, width - gap) }
        else { (0., tile, tile + gap) }
    }
    pub const MENU_GAP: f32 = 1.;
    pub const MENU_MARGIN: f32 = 8.;
    pub const MENU_TILE_GAP: f32 = 2.;
    pub const SETTINGS: ToolbarControl = ToolbarControl::Panel { panel: crate::Panel::ToolSettings };
    pub const SLIDERS: [ToolbarControl; 2] = [ToolbarControl::BrushSizeSlider, ToolbarControl::BrushStrengthSlider];
    pub fn slider_stretch_distance(&self) -> f32 { 2. * self.tile }
    fn menu_strip_extent(row_height: f32) -> f32 { row_height.max(1.) + 2. * Self::MENU_MARGIN }
    pub fn menu_width(&self, minimum: f32, natural: f32, row_height: f32) -> f32 {
        if self.edge == Edge::Top { self.bars[0].width }
        else { natural.max(minimum).max(crate::PANEL_MIN_WIDTH).min((self.work_area.width - Self::menu_strip_extent(row_height) - Self::MENU_GAP).max(1.)) }
    }
    pub fn menu(&self, viewport: [f32; 2], width: f32, height: f32, row_height: f32) -> Option<DrawerPlacement> {
        let vertical = self.edge != Edge::Top;
        let strip = Self::menu_strip_extent(row_height) + Self::MENU_GAP;
        self.drawer(self.capy, viewport,
            &[if vertical { width + strip } else { width }],
            &[if vertical { height } else { height + strip }], Self::MENU_GAP)
    }
    pub fn menu_geometry(&self, placement: &DrawerPlacement, row_height: f32) -> PhoneMenuGeometry {
        let Bounds { width, height, .. } = placement.bounds;
        let vertical = self.edge != Edge::Top;
        let tile = row_height.max(1.);
        let extent = Self::menu_strip_extent(row_height);
        let strip = if vertical {
            Bounds { x: if placement.direction == Edge::Left { width - extent } else { 0. }, y: 0., width: extent, height }
        } else { Bounds { x: 0., y: height - extent, width, height: extent } };
        let separator = if vertical {
            Bounds { x: if strip.x == 0. { extent } else { strip.x - Self::MENU_GAP }, y: 0., width: Self::MENU_GAP, height }
        } else { Bounds { x: 0., y: strip.y - Self::MENU_GAP, width, height: Self::MENU_GAP } };
        let list = if vertical {
            Bounds { x: if strip.x == 0. { separator.x + separator.width } else { 0. }, y: 0., width: width - extent - Self::MENU_GAP, height }
        } else { Bounds { x: 0., y: 0., width, height: separator.y } };
        let reversed = !vertical && self.capy.x > self.settings.x;
        let buttons = std::array::from_fn(|index| {
            let along = Self::MENU_MARGIN + index as f32 * (tile + Self::MENU_TILE_GAP);
            if vertical { Bounds { x: strip.x + Self::MENU_MARGIN, y: along, width: tile, height: tile } }
            else { Bounds { x: if reversed { width - along - tile } else { along }, y: strip.y + Self::MENU_MARGIN, width: tile, height: tile } }
        });
        PhoneMenuGeometry { list, strip, separator, buttons,
            icon: tile * crate::HeaderSize::Medium.icon() as f32 / crate::HeaderSize::Medium.tile() }
    }
    pub fn anchor(&self, layout: &DockLayout, anchor: crate::PhoneDrawerAnchor) -> Option<Bounds> {
        match anchor {
            crate::PhoneDrawerAnchor::Capy => Some(self.capy),
            crate::PhoneDrawerAnchor::ToolSettings => Some(self.settings),
            crate::PhoneDrawerAnchor::Tool(id) => layout.phone.as_ref()?.iter().position(|t| t.id == id).map(|i| self.tools[i]),
        }
    }
    pub fn drawer(&self, anchor: Bounds, viewport: [f32; 2], widths: &[f32], heights: &[f32], gap: f32) -> Option<DrawerPlacement> {
        if !gap.is_finite() || gap < 0. || widths.is_empty() || heights.len() != widths.len()
            || widths.iter().chain(heights).any(|v| !v.is_finite() || *v < 0.) { return None; }
        let vertical = self.edge != Edge::Top;
        let available = if vertical {
            Bounds { y: self.bars[0].y, height: self.bars[0].height, ..self.work_area }
        } else {
            Bounds { x: self.bars[0].x, width: self.bars[0].width, ..self.work_area }
        };
        let mut x = 0.;
        let mut y = 0.;
        let mut row = 0.;
        let mut width: f32 = 0.;
        let mut columns = Vec::with_capacity(widths.len());
        for (&w, &h) in widths.iter().zip(heights) {
            let w = w.min(available.width);
            if x > 0. && x + w > available.width {
                y += row + gap;
                x = 0.;
                row = 0.;
            }
            columns.push(Bounds { x, y, width: w, height: h.max(1.) });
            width = width.max(x + w);
            x += w + gap;
            row = row.max(h.max(1.));
        }
        if !vertical {
            for i in 0..columns.len() {
                if columns.get(i + 1).is_none_or(|next| next.y != columns[i].y) {
                    columns[i].width = available.width - columns[i].x;
                }
            }
            width = available.width;
        }
        let natural_height = y + row;
        let height = if vertical { available.height } else { natural_height.min(available.height) };
        if y > 0. {
            let scale = (height / natural_height).min(1.);
            for column in &mut columns {
                column.y *= scale;
                column.height *= scale;
            }
        } else {
            for column in &mut columns { column.height = height; }
        }
        let direction = if vertical { if anchor.x < viewport[0] / 2. { Edge::Right } else { Edge::Left } }
            else if anchor.y < viewport[1] / 2. { Edge::Bottom } else { Edge::Top };
        let x = match direction { Edge::Right => anchor.x + anchor.width + 8., Edge::Left => anchor.x - width - 8., _ => anchor.x.clamp(available.x, available.x + (available.width - width).max(0.)) };
        let y = match direction { Edge::Bottom => anchor.y + anchor.height + 8., Edge::Top => anchor.y - height - 8., _ => available.y };
        Some(DrawerPlacement { bounds: Bounds { x, y, width, height }, anchor, direction, columns, tab_presentations: Vec::new() })
    }
    pub fn native_control(&self, side: usize, index: u32) -> Option<Bounds> {
        let mut bounds = *self.controls.get(side)?;
        if bounds.width <= 0. || bounds.height <= 0. {
            return None;
        }
        if self.edge == Edge::Top {
            return (index == 0).then_some(bounds);
        }
        bounds.y += index as f32 * self.tile;
        (bounds.y + self.tile <= self.controls[side].y + self.controls[side].height + 0.01)
            .then_some(Bounds { height: self.tile, ..bounds })
    }
    pub fn resolve(layout: &DockLayout, viewport: [f32; 2], insets: [f32; 4], native: [f32; 2],
        native_counts: [u32; 2], swapped: bool) -> Option<Self> {
        if viewport.iter().any(|v| !v.is_finite() || *v <= 0.) || viewport[0].min(viewport[1]) >= 480. {
            return None;
        }
        let scale = (viewport[0].min(viewport[1]) / 320.).min(1.);
        if scale < 1. {
            let mut p = Self::resolve(layout, viewport.map(|v| (v / scale).max(320.)), insets.map(|v| v / scale), native, native_counts, swapped)?;
            let scaled = |b: &mut Bounds| { b.x *= scale; b.y *= scale; b.width *= scale; b.height *= scale; };
            for b in [&mut p.capy, &mut p.capy_hit, &mut p.size, &mut p.strength, &mut p.settings, &mut p.settings_hit, &mut p.work_area]
                .into_iter().chain(&mut p.tools).chain(&mut p.tool_hits).chain(&mut p.chips).chain(&mut p.controls).chain(&mut p.bars) { scaled(b); }
            p.tile *= scale;
            p.icon *= scale;
            p.capy_icon *= scale;
            return Some(p);
        }
        let vertical = viewport[0] > viewport[1];
        let edge = if vertical { if swapped { Edge::Left } else { Edge::Right } } else { Edge::Top };
        let [left, top, right, bottom] = insets.map(|v| v.max(8.));
        let available = Bounds { x: left, y: top,
            width: (viewport[0] - left - right).max(1.), height: (viewport[1] - top - bottom).max(1.) };
        let tiles = layout.phone.as_deref().unwrap_or(&[]);
        let count = tiles.iter().filter(|t| t.control != ToolbarControl::Divider).count();
        let dividers = tiles.len() - count;
        let native_counts: [u32; 2] = std::array::from_fn(|side| if native[side] > 0. { native_counts[side] } else { 0 });
        let length = if vertical { available.height } else { available.width };
        let side = usize::from(edge == Edge::Right);
        let count_on_bar = if vertical { native_counts[side] } else { native_counts.iter().sum() };
        let reservation = if vertical {
            if count_on_bar > 0 { 8. } else { 0. }
        } else {
            native_counts.iter().filter(|count| **count > 0).count() as f32 * 8.
        };
        let gaps = dividers as f32 * Self::DIVIDER_WIDTH + count.saturating_sub(dividers + 1) as f32 * Self::TOOL_GAP;
        let tile = ((length - reservation - gaps) / (count as u32 + count_on_bar).max(1) as f32)
            .clamp(1., 48.);
        let point = |edge: Edge, along: f32, extent: f32| match edge {
            Edge::Left => Bounds { x: available.x, y: available.y + along, width: tile, height: extent },
            Edge::Right => Bounds { x: available.x + available.width - tile, y: available.y + along, width: tile, height: extent },
            Edge::Top => Bounds { x: available.x + along, y: available.y, width: extent, height: tile },
            Edge::Bottom => Bounds { x: available.x + along, y: available.y + available.height - tile, width: extent, height: tile },
        };
        let other = match edge { Edge::Left => Edge::Right, Edge::Right => Edge::Left, _ => Edge::Bottom };
        let mut controls = [Bounds::default(); 2];
        for (side, &count) in native_counts.iter().enumerate() {
            let width = count as f32 * tile;
            if count > 0 {
                controls[side] = if vertical {
                    point(if side == 0 { Edge::Left } else { Edge::Right }, 0., native_counts[side] as f32 * tile)
                } else {
                    point(edge, if side == 0 { 0. } else { length - width }, width)
                };
            }
        }
        let start = if vertical { native_counts[side] as f32 * tile } else { native_counts[0] as f32 * tile };
        let mut along = if start > 0. { start + 8. } else { 0. };
        let mut tools = [Bounds::default(); PhoneLayout::MAX_TILES];
        let tool_start = along;
        for (i, t) in tiles.iter().enumerate().take(tools.len()) {
            let (offset, extent, advance) = Self::tool_metrics(t.control == ToolbarControl::Divider, tile, 1.);
            tools[i] = point(edge, along + offset, extent);
            along += advance;
        }
        let capy_start = if vertical {
            let n = native_counts[usize::from(other == Edge::Right)] as f32 * tile;
            if n > 0. { n + 8. } else { 0. }
        } else {
            0.
        };
        let slider = (length - capy_start - 2. * tile - 6.).max(2.) / 2.;
        let mut capy = point(other, capy_start, tile);
        let mut size = point(other, capy_start + tile + 2., slider);
        let mut strength = point(other, capy_start + tile + 4. + slider, slider);
        let mut settings = point(other, length - tile, tile);
        let mut chips = [if count > 0 { point(edge, tool_start, along - tool_start - Self::TOOL_GAP) } else { Bounds::default() },
            point(other, capy_start, length - capy_start)];
        if !vertical && swapped {
            for b in [&mut capy, &mut size, &mut strength, &mut settings, &mut chips[1]] {
                b.x = available.x + available.width - (b.x - available.x) - b.width;
            }
        }
        let bars = [point(edge, 0., length), point(other, 0., length)];
        let work_area = if vertical {
            Bounds { x: left + tile + 8., y: 0., width: (viewport[0] - left - right - 2. * tile - 16.).max(1.), height: viewport[1] }
        } else {
            Bounds { x: 0., y: top + tile + 8., width: viewport[0], height: (viewport[1] - top - bottom - 2. * tile - 16.).max(1.) }
        };
        let hit = |b: Bounds, edge: Edge| {
            if b.width <= 0. { return Bounds::default(); }
            let x = (b.x - 1.).max(0.);
            let y = (b.y - 1.).max(0.);
            match edge {
                Edge::Top => Bounds { x, y: 0., width: (b.x + b.width + 1.).min(viewport[0]) - x, height: b.y + b.height },
                Edge::Bottom => Bounds { x, y: b.y, width: (b.x + b.width + 1.).min(viewport[0]) - x, height: viewport[1] - b.y },
                Edge::Left => Bounds { x: 0., y, width: b.x + b.width, height: (b.y + b.height + 1.).min(viewport[1]) - y },
                Edge::Right => Bounds { x: b.x, y, width: viewport[0] - b.x, height: (b.y + b.height + 1.).min(viewport[1]) - y },
            }
        };
        Some(Self { tile, icon: tile * crate::HeaderSize::Medium.icon() as f32 / 48., capy_icon: tile * 440. / 512.,
            edge, capy, capy_hit: hit(capy, other), size, strength, settings, settings_hit: hit(settings, other), tools,
            tool_hits: std::array::from_fn(|i| if tiles.get(i).is_some_and(|t| t.control != ToolbarControl::Divider) { hit(tools[i], edge) } else { Bounds::default() }),
            chips, controls, bars, work_area })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phone_bar_contacts_keep_sliders_buttons_and_holds_out_of_window_movement() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        for viewport in [[320., 568.], [412., 891.], [891., 412.]] {
            for swapped in [false, true] {
                let p = PhoneLayout::resolve(&layout, viewport, [0.; 4], [0., 48.], [0, 1], swapped).unwrap();
                let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
                for b in p.tools.into_iter().filter(|b| b.width > 0.).chain([p.capy, p.settings]) {
                    let mut contact = PhoneBarContact::begin(&p, center(b), false).unwrap();
                    assert!(!contact.move_window(false));
                    assert!(contact.move_window(true));
                    contact.hold();
                    assert!(!contact.move_window(true));
                }
                for b in [p.size, p.strength, p.work_area] { assert!(PhoneBarContact::begin(&p, center(b), false).is_none()); }
                for b in p.controls.into_iter().filter(|b| b.width > 0.) {
                    assert!(PhoneBarContact::begin(&p, center(b), true).is_none());
                    assert!(PhoneBarContact::begin(&p, center(b), false).is_some(), "native gaps belong to the bar");
                }
                assert_eq!(p.anchor(&layout, crate::PhoneDrawerAnchor::ToolSettings), Some(p.settings));
                assert!(p.settings.intersection(p.size).is_none() && p.settings.intersection(p.strength).is_none());
                assert!(p.chips[1].contains(p.capy.x, p.capy.y));
                let length = if p.edge == Edge::Top { p.size.width } else { p.size.height };
                assert!(length >= 90., "the fixed settings tile preserves a usable slider at {viewport:?}");
            }
        }
    }
    #[test]
    fn hidden_phone_keeps_capy_and_reallocates_its_existing_chip() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        for viewport in [[412., 891.], [891., 412.], [280., 280.], [280., 560.], [560., 280.]] {
            for swapped in [false, true] {
                let state = PhoneState { platform: Some(crate::Platform::Gtk), swapped, ..Default::default() };
                let visible = state.resolve(&layout, viewport).unwrap();
                let hidden = PhoneState { hidden: true, ..state }.resolve(&layout, viewport).unwrap();
                assert_eq!(hidden.capy, visible.capy);
                assert_eq!(hidden.capy_hit, visible.capy_hit);
                assert_eq!(hidden.chips[1], visible.capy);
                assert!(hidden.tools.iter().chain(&hidden.tool_hits).chain(&hidden.controls).chain(&hidden.bars)
                    .chain([&hidden.chips[0], &hidden.size, &hidden.strength, &hidden.settings, &hidden.settings_hit])
                    .all(|b| *b == Bounds::default()));
            }
        }
    }
    #[test]
    fn phone_geometry_matrix() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        for preset in crate::WorkspacePreset::ALL { assert!(!preset.phone_controls().contains(&ToolbarControl::Divider)); }
        let paint = crate::WorkspacePreset::Illustrator.phone_controls();
        let position = |control| paint.iter().position(|value| *value == control).unwrap();
        let fill = position(ToolbarControl::ToolSlot { slot: crate::ToolSlotId::Fill });
        assert_eq!(fill, position(ToolbarControl::Command { command: crate::CommandId::Eraser }) + 1);
        assert_eq!(position(ToolbarControl::Panel { panel: crate::Panel::Layers }), fill + 1);
        for (short, long, expected) in [
            (412., 891., 47.75),
            (375., 667., 43.125),
            (360., 780., 41.25),
            (320., 568., 36.25),
            (320., 320., 36.25),
            (280., 280., 31.71875),
            (280., 560., 31.71875),
        ] {
            for viewport in [[short, long], [long, short]] {
                for insets in [[0.; 4], [30., 0., 0., 0.], [59., 0., 59., 0.], [0., 59., 0., 34.]] {
                    for (native, native_counts) in [
                        ([0.; 2], [0; 2]),
                        ([48., 0.], [1, 0]),
                        ([0., 48.], [0, 1]),
                        ([144., 0.], [3, 0]),
                        ([0., 144.], [0, 3]),
                        ([96., 48.], [2, 1]),
                        ([48., 96.], [1, 2]),
                    ] {
                        for swapped in [false, true] {
                            let p = PhoneLayout::resolve(
                                &layout,
                                viewport,
                                insets,
                                native,
                                native_counts,
                                swapped,
                            )
                            .unwrap();
                            if insets == [0.; 4] && native == [0.; 2] {
                                assert!((p.tile - expected).abs() < 0.01);
                            }
                            assert!(p.work_area.width > 0. && p.work_area.height > 0.);
                            let sliders = [p.size, p.strength, p.settings];
                            for (chip, contents) in [
                                (p.chips[0], p.tools.as_slice()),
                                (p.chips[1], sliders.as_slice()),
                            ] {
                                for b in contents.iter().filter(|b| b.width > 0.) {
                                    assert!(b.x >= chip.x - 0.001 && b.y >= chip.y - 0.001);
                                    assert!(b.x + b.width <= chip.x + chip.width + 0.001);
                                    assert!(b.y + b.height <= chip.y + chip.height + 0.001);
                                }
                                for controls in p.controls { assert!(chip.intersection(controls).is_none()); }
                            }
                            let gap = if viewport[0] > viewport[1] {
                                p.size.y - p.capy.y - p.capy.height
                            } else if swapped {
                                p.capy.x - p.size.x - p.size.width
                            } else {
                                p.size.x - p.capy.x - p.capy.width
                            };
                            assert!((gap - 2. * (short / 320.).min(1.)).abs() < 0.01);
                            let gap = if viewport[0] > viewport[1] {
                                p.strength.y - p.size.y - p.size.height
                            } else if swapped {
                                p.size.x - p.strength.x - p.strength.width
                            } else {
                                p.strength.x - p.size.x - p.size.width
                            };
                            assert!((gap - 2. * (short / 320.).min(1.)).abs() < 0.01);
                            if viewport[0] > viewport[1] {
                                for side in 0..2 {
                                    for index in 0..native_counts[side] {
                                        let slot = p.native_control(side, index).unwrap();
                                        assert_eq!((slot.width, slot.height), (p.tile, p.tile));
                                        assert!(slot.intersection(p.capy).is_none());
                                        assert!(slot.intersection(p.size).is_none());
                                        assert!(slot.intersection(p.strength).is_none());
                                    }
                                    assert!(p.native_control(side, native_counts[side]).is_none());
                                }
                            } else {
                                for side in 0..2 {
                                    let slot = p.native_control(side, 0);
                                    if native[side] == 0. {
                                        assert!(slot.is_none());
                                    } else {
                                        let slot = slot.unwrap();
                                        assert!((slot.width - native_counts[side] as f32 * p.tile).abs() < 0.0001);
                                        assert!((slot.height - p.tile).abs() < 0.0001);
                                    }
                                    assert!(p.native_control(side, 1).is_none());
                                }
                            }
                            for b in p.tools.iter().filter(|b| b.width > 0.) {
                                assert!(b.x >= insets[0] && b.y >= insets[1]);
                                assert!(
                                    b.x + b.width <= viewport[0] - insets[2] + 0.01
                                        && b.y + b.height <= viewport[1] - insets[3] + 0.01
                                );
                                for c in p.controls {
                                    assert!(
                                        b.intersection(c).is_none(),
                                        "{viewport:?} {insets:?} {native:?} {swapped}: {b:?} {c:?}"
                                    );
                                }
                            }
                            assert!(p.icon > 0. && p.icon <= p.tile);
                            assert!(p.capy_icon > 0. && p.capy_icon <= p.tile);
                            let capy_edge = match p.edge {
                                Edge::Left => Edge::Right,
                                Edge::Right => Edge::Left,
                                _ => Edge::Bottom,
                            };
                            for (visual, hit, edge) in p.tools.iter().zip(&p.tool_hits)
                                .filter(|(_, hit)| hit.width > 0.)
                                .map(|(visual, hit)| (*visual, *hit, p.edge))
                                .chain(std::iter::once((p.capy, p.capy_hit, capy_edge)))
                                .filter(|(visual, _, _)| visual.width > 0.) {
                                assert!(hit.contains(visual.x + visual.width / 2., visual.y + visual.height / 2.));
                                assert!(hit.x >= 0. && hit.y >= 0.);
                                assert!(hit.x + hit.width <= viewport[0] + 0.01);
                                assert!(hit.y + hit.height <= viewport[1] + 0.01);
                                assert!(match edge {
                                    Edge::Left => hit.x + hit.width <= visual.x + visual.width + 0.01,
                                    Edge::Right => hit.x >= visual.x - 0.01,
                                    Edge::Top => hit.y + hit.height <= visual.y + visual.height + 0.01,
                                    Edge::Bottom => hit.y >= visual.y - 0.01,
                                });
                            }
                        }
                    }
                }
            }
        }
        assert!(
            PhoneLayout::resolve(&layout, [480., 900.], [0.; 4], [0.; 2], [0; 2], false).is_none()
        );
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhoneMenuNavigation {
    path: Vec<[usize; 2]>,
    swipe_axis: Option<crate::Axis>,
    swipe_progress: f32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneMenuGeometry {
    pub list: Bounds,
    pub strip: Bounds,
    pub separator: Bounds,
    pub buttons: [Bounds; 5],
    pub icon: f32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneMenuPage {
    pub path: Vec<[usize; 2]>,
    pub menu: crate::ContextMenu,
}
impl PhoneMenuPage {
    pub fn name(&self) -> String {
        PhoneMenuNavigation::page_name(&self.path)
    }
    pub fn parent_name(&self) -> Option<String> {
        (!self.path.is_empty()).then(|| PhoneMenuNavigation::page_name(&self.path[..self.path.len() - 1]))
    }
}
impl PhoneMenuNavigation {
    pub fn page_name(path: &[[usize; 2]]) -> String {
        if path.is_empty() { return "main".into(); }
        path.iter().flatten().fold("page".to_string(), |name, index| format!("{name}-{index}"))
    }
    pub fn name(&self) -> String { Self::page_name(&self.path) }
    pub fn is_root(&self) -> bool { self.path.is_empty() }
    pub fn pages(menu: &crate::ContextMenu) -> Vec<PhoneMenuPage> {
        fn visit(menu: crate::ContextMenu, path: Vec<[usize; 2]>, out: &mut Vec<PhoneMenuPage>) {
            out.push(PhoneMenuPage { path: path.clone(), menu: menu.clone() });
            for (s, section) in menu.sections.into_iter().enumerate() {
                for (i, item) in section.into_iter().enumerate() {
                    if item.action.is_none() && item.enabled && !item.sections.is_empty() {
                        let mut path = path.clone();
                        path.push([s, i]);
                        visit(crate::ContextMenu { title: item.label, sections: item.sections }, path, out);
                    }
                }
            }
        }
        let mut pages = Vec::new();
        visit(menu.clone(), Vec::new(), &mut pages);
        pages
    }
    pub fn select(&mut self, menu: &crate::ContextMenu, name: &str) -> bool {
        let Some(page) = Self::pages(menu).into_iter().find(|page| page.name() == name) else { return false; };
        self.path = page.path;
        true
    }
    pub fn back(&mut self) -> bool { self.path.pop().is_some() }
    pub fn workspaces(&mut self, menu: &crate::ContextMenu, localization: &crate::Localizer) -> bool {
        let window = crate::ApplicationMenu::Window.localized_label(localization);
        let workspaces = localization.text(crate::MessageId::WORKSPACE_WORKSPACES);
        let Some(page) = Self::pages(menu).into_iter().find(|page| page.path.len() == 2
            && page.menu.title == workspaces.as_ref()
            && menu.sections[page.path[0][0]][page.path[0][1]].label == window.as_ref()) else { return false; };
        self.path = page.path;
        true
    }
    pub fn drawings(&mut self, menu: &crate::ContextMenu, localization: &crate::Localizer) -> bool {
        let title = localization.text(crate::MessageId::NATIVE_HEADER_DRAWINGS);
        let Some(page) = Self::pages(menu).into_iter().find(|page| page.path.len() == 1 && page.menu.title == title.as_ref()) else { return false; };
        self.path = page.path;
        true
    }
    pub fn begin_swipe(&mut self) { self.swipe_axis = None; self.swipe_progress = 0.; }
    pub fn swipe(&mut self, delta: [f32; 2], width: f32, past_slop: bool, rtl: bool) -> PhoneMenuSwipe {
        if self.is_root() { return PhoneMenuSwipe::Scroll; }
        let along = delta[0] * if rtl { -1. } else { 1. };
        if self.swipe_axis.is_none() && past_slop {
            self.swipe_axis = Some(if along > delta[1].abs() { crate::Axis::Horizontal } else { crate::Axis::Vertical });
        }
        match self.swipe_axis {
            None => PhoneMenuSwipe::Pending,
            Some(crate::Axis::Vertical) => PhoneMenuSwipe::Scroll,
            Some(crate::Axis::Horizontal) => {
                self.swipe_progress = (along / width.max(1.)).clamp(0., 1.);
                PhoneMenuSwipe::Back(self.swipe_progress)
            }
        }
    }
    pub fn end_swipe(&mut self, velocity: f32) -> bool {
        let complete = self.swipe_axis == Some(crate::Axis::Horizontal)
            && (self.swipe_progress >= 0.3 || velocity > 0.5) && self.back();
        self.begin_swipe();
        complete
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoneMenuSwipe { Pending, Scroll, Back(f32) }
#[derive(Clone, Debug, serde::Serialize)]
pub struct PhoneMenu {
    pub strip: Vec<crate::ContextMenuItem>,
    pub list: crate::ContextMenu,
}

impl<R: layer_render::CanvasRenderer> crate::UiSession<R> {
    pub fn phone_menu(&self) -> Option<PhoneMenu> {
        self.phone_layout()?;
        let mut primary = self.application_menu(crate::ApplicationMenu::Primary);
        let list = crate::ContextMenu { title: primary.title.clone(), sections: primary.sections.split_off(2) };
        let settings = self.command(crate::CommandId::Settings);
        let mut strip = vec![crate::ContextMenuItem { enabled: settings.enabled,
            ..crate::ContextMenuItem::command(settings.label.to_string(), crate::UiAction::Invoke { command: crate::CommandId::Settings }) }];
        strip.extend(primary.sections.into_iter().flatten());
        let zen = format!("layer-{}-symbolic", self.state().settings.zen_icon.icon());
        for (index, (item, icon)) in strip.iter_mut().zip(["layer-settings-symbolic", "layer-search-symbolic", "layer-finger-draw-symbolic", &zen, "layer-swap-orientation-symbolic"]).enumerate() {
            item.icon = Some(icon.into());
            if !matches!(index, 2 | 4) { item.selected = None; }
        }
        Some(PhoneMenu { strip, list })
    }
}

#[cfg(test)]
mod menu_tests {
    use super::*;
    use crate::{ApplicationMenu, ContextMenu, ContextMenuItem, Localizer, UiLanguage};

    fn menu() -> ContextMenu {
        ContextMenu { title: "Main Menu".into(), sections: vec![vec![
            ContextMenuItem::submenu("File", vec![vec![ContextMenuItem::submenu("Nested", vec![vec![
                ContextMenuItem::command("Save", crate::UiAction::Invoke { command: crate::CommandId::SaveDocument }),
            ]])]]),
            ContextMenuItem::submenu("Window", vec![vec![ContextMenuItem::submenu("Workspaces", vec![vec![
                ContextMenuItem::command("Manage Workspaces…", crate::UiAction::Invoke { command: crate::CommandId::Settings }),
            ]])]]),
        ]] }
    }
    #[test]
    fn menu_navigation_preserves_ancestors_and_rejects_missing_pages() {
        let menu = menu();
        let pages = PhoneMenuNavigation::pages(&menu);
        assert_eq!(pages.iter().map(PhoneMenuPage::name).collect::<Vec<_>>(),
            ["main", "page-0-0", "page-0-0-0-0", "page-0-1", "page-0-1-0-0"]);
        let mut navigation = PhoneMenuNavigation::default();
        assert!(navigation.select(&menu, "page-0-0-0-0"));
        assert_eq!(pages[2].parent_name().as_deref(), Some("page-0-0"));
        assert!(!navigation.select(&menu, "page-0-8"));
        assert_eq!(navigation.name(), "page-0-0-0-0");
        assert!(navigation.back());
        assert_eq!(navigation.name(), "page-0-0");
        assert!(navigation.back());
        assert!(!navigation.back());
        assert!(navigation.is_root());
        let mut disabled = menu.clone();
        disabled.sections[0][0].enabled = false;
        assert!(!navigation.select(&disabled, "page-0-0-0-0"));
        assert!(navigation.is_root());
    }
    #[test]
    fn workspace_route_keeps_window_as_back_destination_in_each_language() {
        for language in UiLanguage::ALL {
            let localization = Localizer::shared(language);
            let mut menu = menu();
            menu.sections[0][1].label = ApplicationMenu::Window.localized_label(&localization).to_string();
            menu.sections[0][1].sections[0][0].label = localization.text(crate::MessageId::WORKSPACE_WORKSPACES).to_string();
            let mut navigation = PhoneMenuNavigation::default();
            assert!(navigation.workspaces(&menu, &localization));
            assert_eq!(navigation.name(), "page-0-1-0-0");
            assert!(navigation.back());
            assert_eq!(navigation.name(), "page-0-1");
        }
    }
    #[test]
    fn menu_strip_tiles_match_rows_and_pack_from_capy_with_shared_margins() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        for viewport in [[412., 891.], [891., 412.], [320., 568.], [280., 560.], [560., 280.]] {
            for swapped in [false, true] {
                let phone = PhoneLayout::resolve(&layout, viewport, [0.; 4], [0.; 2], [0; 2], swapped).unwrap();
                for row in [28., 32., 44.] {
                    let width = phone.menu_width(242., 340., row);
                    let placement = phone.menu(viewport, width, 400., row).unwrap();
                    let geometry = phone.menu_geometry(&placement, row);
                    assert!(geometry.list.intersection(geometry.strip).is_none());
                    assert_eq!(geometry.icon, row * crate::HeaderSize::Medium.icon() as f32 / crate::HeaderSize::Medium.tile());
                    for (index, button) in geometry.buttons.into_iter().enumerate() {
                        assert_eq!([button.width, button.height], [row; 2]);
                        assert!(button.x >= geometry.strip.x && button.y >= geometry.strip.y);
                        assert!(button.x + row <= geometry.strip.x + geometry.strip.width + 0.001);
                        assert!(button.y + row <= geometry.strip.y + geometry.strip.height + 0.001);
                        let along = PhoneLayout::MENU_MARGIN + index as f32 * (row + PhoneLayout::MENU_TILE_GAP);
                        if phone.edge == Edge::Top {
                            assert_eq!(button.x, if swapped { placement.bounds.width - along - row } else { along });
                            assert_eq!(button.y - geometry.strip.y, PhoneLayout::MENU_MARGIN);
                            assert_eq!(geometry.strip.y + geometry.strip.height - button.y - row, PhoneLayout::MENU_MARGIN);
                            assert_eq!(button.y - geometry.separator.y - geometry.separator.height, PhoneLayout::MENU_MARGIN);
                        } else {
                            assert_eq!(button.y, along);
                            assert_eq!(button.x - geometry.strip.x, PhoneLayout::MENU_MARGIN);
                            assert_eq!(geometry.strip.x + geometry.strip.width - button.x - row, PhoneLayout::MENU_MARGIN);
                            let gap = if swapped { button.x - geometry.separator.x - geometry.separator.width }
                                else { geometry.separator.x - button.x - row };
                            assert_eq!(gap, PhoneLayout::MENU_MARGIN);
                        }
                    }
                    assert_eq!(geometry.separator.width.min(geometry.separator.height), PhoneLayout::MENU_GAP);
                }
            }
        }
    }
    #[test]
    fn back_swipes_lock_axes_and_use_distance_or_release_velocity() {
        for rtl in [false, true] {
            let sign = if rtl { -1. } else { 1. };
            for (distance, velocity, back) in [(29., 0., false), (30., 0., true), (10., 0.5, false), (10., 0.501, true)] {
                let mut nav = PhoneMenuNavigation::default();
                assert!(nav.select(&menu(), "page-0-0"));
                nav.begin_swipe();
                assert_eq!(nav.swipe([sign * distance, 0.], 100., false, rtl), PhoneMenuSwipe::Pending);
                assert_eq!(nav.swipe([sign * distance, 0.], 100., true, rtl), PhoneMenuSwipe::Back(distance / 100.));
                assert!(matches!(nav.swipe([sign * distance, 200.], 100., true, rtl), PhoneMenuSwipe::Back(_)));
                assert_eq!(nav.end_swipe(velocity), back);
                assert_eq!(nav.is_root(), back);
            }
            let mut nav = PhoneMenuNavigation::default();
            nav.select(&menu(), "page-0-0");
            assert_eq!(nav.swipe([0., 20.], 100., true, rtl), PhoneMenuSwipe::Scroll);
            assert_eq!(nav.swipe([sign * 80., 20.], 100., true, rtl), PhoneMenuSwipe::Scroll);
            assert!(!nav.end_swipe(2.));
        }
    }
}

#[cfg(test)]
#[path = "phone_contact_tests.rs"]
mod contact_tests;
