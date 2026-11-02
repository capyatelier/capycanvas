use crate::{Bounds, DockLayout, DrawerPlacement, Edge, ToolbarControl};
use serde::{Deserialize, Serialize};

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
    Panel { panel: crate::Panel },
    SwapSides,
    Slider { control: ToolbarControl, phase: crate::ContactPhase },
    Metrics { native: [f32; 2], native_counts: [u32; 2], insets: [f32; 4] },
}
impl crate::UiState {
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
        PhoneLayout::resolve(layout, viewport, self.insets, self.native, self.native_counts, self.swapped)
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
    pub tools: [Bounds; 19],
    pub tool_hits: [Bounds; 19],
    pub chips: [Bounds; 2],
    pub controls: [Bounds; 2],
    pub bars: [Bounds; 2],
    pub work_area: Bounds,
}

impl PhoneLayout {
    pub const MENU_GAP: f32 = 1.;
    pub const SLIDERS: [ToolbarControl; 2] = [ToolbarControl::BrushSizeSlider, ToolbarControl::BrushStrengthSlider];
    pub fn menu_column_widths(&self, minimum_widths: [f32; 2], natural_widths: [f32; 2]) -> [f32; 2] {
        if self.edge == Edge::Top { return [self.bars[0].width; 2]; }
        let minimum = minimum_widths.map(|width| width.max(crate::PANEL_MIN_WIDTH).min(self.work_area.width));
        let natural = std::array::from_fn::<_, 2, _>(|i| natural_widths[i].max(minimum[i]).min(self.work_area.width));
        let available = self.work_area.width - Self::MENU_GAP;
        if minimum.iter().sum::<f32>() > available { return natural; }
        let growth = natural.iter().sum::<f32>() - minimum.iter().sum::<f32>();
        let factor = if growth > 0. { ((available - minimum.iter().sum::<f32>()) / growth).min(1.) } else { 0. };
        std::array::from_fn(|i| minimum[i] + (natural[i] - minimum[i]) * factor)
    }
    pub fn menu(&self, viewport: [f32; 2], minimum_widths: [f32; 2], heights: [f32; 2]) -> Option<DrawerPlacement> {
        self.drawer(self.capy, viewport, &self.menu_column_widths(minimum_widths, minimum_widths), &heights, Self::MENU_GAP)
    }
    pub fn anchor(&self, layout: &DockLayout, tile: Option<u32>) -> Option<Bounds> {
        match tile {
            None => Some(self.capy),
            Some(id) => layout.phone.as_ref()?.iter().position(|t| t.id == id).map(|i| self.tools[i]),
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
        let x = match direction { Edge::Right => anchor.x + anchor.width + 8., Edge::Left => anchor.x - width - 8., _ => anchor.x.clamp(available.x, available.x + available.width - width) };
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
        let gaps = dividers as f32 * 8. + count.saturating_sub(dividers + 1) as f32 * 2.;
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
        let mut tools = [Bounds::default(); 19];
        let tool_start = along;
        for (i, t) in tiles.iter().enumerate().take(19) {
            if t.control == ToolbarControl::Divider {
                tools[i] = point(edge, along - 2., 8.);
                along += 6.;
                continue;
            }
            tools[i] = point(edge, along, tile);
            along += tile + 2.;
        }
        let capy_start = if vertical {
            let n = native_counts[usize::from(other == Edge::Right)] as f32 * tile;
            if n > 0. { n + 8. } else { 0. }
        } else {
            0.
        };
        let slider = (length - capy_start - tile - 10.).max(2.) / 2.;
        let mut capy = point(other, capy_start, tile);
        let mut size = point(other, capy_start + tile + 8., slider);
        let mut strength = point(other, capy_start + tile + 10. + slider, slider);
        let mut chips = [if count > 0 { point(edge, tool_start, along - tool_start - 2.) } else { Bounds::default() },
            point(other, capy_start + tile + 8., 2. * slider + 2.)];
        if !vertical && swapped {
            for b in [&mut capy, &mut size, &mut strength, &mut chips[1]] {
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
            edge, capy, capy_hit: hit(capy, other), size, strength, tools,
            tool_hits: std::array::from_fn(|i| if tiles.get(i).is_some_and(|t| t.control != ToolbarControl::Divider) { hit(tools[i], edge) } else { Bounds::default() }),
            chips, controls, bars, work_area })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phone_geometry_matrix() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        let paint = crate::WorkspacePreset::Illustrator.phone_controls();
        let position = |control| paint.iter().position(|value| *value == control).unwrap();
        let fill = position(ToolbarControl::ToolSlot { slot: crate::ToolSlotId::Fill });
        assert_eq!(fill, position(ToolbarControl::Command { command: crate::CommandId::Eraser }) + 1);
        assert_eq!(position(ToolbarControl::Panel { panel: crate::Panel::Layers }), fill + 1);
        for (short, long, expected) in [
            (412., 891., 47.),
            (375., 667., 42.375),
            (360., 780., 40.5),
            (320., 568., 35.5),
            (320., 320., 35.5),
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
                            let sliders = [p.size, p.strength];
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
                            assert!((gap - 8.).abs() < 0.01);
                            let gap = if viewport[0] > viewport[1] {
                                p.strength.y - p.size.y - p.size.height
                            } else if swapped {
                                p.size.x - p.strength.x - p.strength.width
                            } else {
                                p.strength.x - p.size.x - p.size.width
                            };
                            assert!((gap - 2.).abs() < 0.01);
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
                                        assert_eq!(
                                            (slot.width, slot.height),
                                            (native_counts[side] as f32 * p.tile, p.tile)
                                        );
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
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneMenuGeometry {
    pub columns: [Bounds; 2],
    pub separator: Bounds,
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
    pub fn geometry(&self, placement: &DrawerPlacement) -> PhoneMenuGeometry {
        let mut columns = std::array::from_fn(|i| placement.columns.get(i).copied().unwrap_or_default());
        let stacked = columns[1].y > columns[0].y;
        let separator = if !self.is_root() && stacked {
            columns = [Bounds::default(), Bounds { x: 0., y: 0., width: placement.bounds.width, height: placement.bounds.height }];
            Bounds::default()
        } else if stacked {
            Bounds { x: 6., y: (columns[0].y + columns[0].height + columns[1].y) * 0.5 - 0.5,
                width: (placement.bounds.width - 12.).max(0.), height: 1. }
        } else {
            Bounds { x: (columns[0].x + columns[0].width + columns[1].x) * 0.5 - 0.5, y: 6.,
                width: 1., height: (placement.bounds.height - 12.).max(0.) }
        };
        PhoneMenuGeometry { columns, separator }
    }
}
impl<R: layer_render::CanvasRenderer> crate::UiSession<R> {
    pub fn phone_menu_columns(&self) -> [crate::ContextMenu; 2] {
        let mut first = self.application_menu(crate::ApplicationMenu::Primary);
        let second = crate::ContextMenu { title: first.title.clone(), sections: first.sections.split_off(2) };
        [first, second]
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
    fn menu_uses_native_row_widths_without_displacing_columns_that_fit() {
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        let phone = PhoneLayout::resolve(&layout, [891., 412.], [0.; 4], [0.; 2], [0; 2], false).unwrap();
        assert_eq!(phone.menu_column_widths([242.; 2], [340., 242.]), [340., 242.]);
        let widths = phone.menu_column_widths([242.; 2], [900.; 2]);
        assert!(widths.iter().all(|width| *width >= 242.));
        assert!((widths.iter().sum::<f32>() + PhoneLayout::MENU_GAP - phone.work_area.width).abs() < 0.001);
        let narrow = phone.menu_column_widths([600., 242.], [600., 242.]);
        let placement = phone.menu([891., 412.], narrow, [200.; 2]).unwrap();
        assert!(placement.columns[1].y > placement.columns[0].y);
    }

    #[test]
    fn submenu_occupies_the_existing_single_column_menu_bounds() {
        let menu = menu();
        let layout = crate::WorkspacePreset::Painter.layout(crate::Platform::Gtk);
        for viewport in [[412., 891.], [891., 412.], [320., 568.], [568., 320.], [320., 320.]] {
            let layout = PhoneLayout::resolve(&layout, viewport, [0.; 4], [0.; 2], [0; 2], false).unwrap();
            let placement = layout.menu(viewport, [242., 242.], [200., 300.]).unwrap();
            let mut navigation = PhoneMenuNavigation::default();
            let root = navigation.geometry(&placement);
            assert_eq!(root.columns.as_slice(), placement.columns.as_slice());
            assert!(root.separator.width > 0. && root.separator.height > 0.);
            assert!(navigation.select(&menu, "page-0-0"));
            let page = navigation.geometry(&placement);
            let columns = page.columns;
            if placement.columns[1].y > placement.columns[0].y {
                assert_eq!(columns[0], Bounds::default());
                assert_eq!(columns[1], Bounds { x: 0., y: 0., width: placement.bounds.width, height: placement.bounds.height });
                assert_eq!(page.separator, Bounds::default());
            } else { assert_eq!(page, root); }
        }
    }
}
