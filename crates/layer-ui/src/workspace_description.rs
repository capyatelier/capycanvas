//! Human names for completed layout edits, derived from semantic changes.
use crate::{DockItem, DockLayout, DockNode, Panel, PanelKind};

use serde::{Deserialize, Serialize};
use crate::{Localizer, FluentArgs, MessageId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPanelName {
    pub panel: Panel,
    pub custom_name: Option<String>,
}
impl LayoutPanelName {
    fn validate(&self) -> Result<(), String> {
        if matches!(self.panel, Panel::CustomToolbar(0 | u32::MAX)) { return Err("Invalid history panel identity".into()); }
        if let Some(name) = &self.custom_name { if self.panel.kind() != PanelKind::Tiles { return Err("History custom title requires a toolbar".into()); } crate::customization::validate_toolbar_name(name)?; }
        Ok(())
    }
    pub fn title(&self, localization: &Localizer) -> String {
        self.custom_name.clone().unwrap_or_else(|| self.panel.localized_label(localization).to_string())
    }
    fn display(&self, localization: &Localizer) -> String {
        let mut args = FluentArgs::new(); args.set("name", self.title(localization));
        localization.format(if self.panel.kind() == PanelKind::Tiles { MessageId::WORKSPACE_HISTORY_TOOLBAR } else { MessageId::WORKSPACE_HISTORY_PANEL }, &args)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutPanelAction { Added, Deleted, Moved, Resized, Collapsed, Expanded, Hidden, Shown, Rearranged, TabReordered, TabsChanged, ToolsAdded, ToolsRemoved, ToolsChanged, Customized, Replaced, Selected }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayoutChange {
    Automatic, Starting, HeaderSize { size: crate::HeaderSize }, HeaderAdded { item: crate::HeaderItem }, HeaderRemoved { item: crate::HeaderItem }, HeaderRearranged, CanvasInfo, CanvasBar, Restored, RestoredEarlier,
    Panels { action: LayoutPanelAction, panels: Vec<LayoutPanelName> },
    Renamed { before: LayoutPanelName, after: LayoutPanelName },
}
impl LayoutChange {
    pub fn panels(action: LayoutPanelAction, panels: Vec<LayoutPanelName>) -> Self { Self::Panels { action, panels } }
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Automatic => return Err("Automatic layout change must be resolved before storage".into()),
            Self::Panels { panels, .. } => {
                if panels.len() > 4096 { return Err("Layout history panel list exceeds supported limits".into()); }
                for panel in panels { panel.validate()?; }
            }
            Self::Renamed { before, after } => { before.validate()?; after.validate()?; }
            Self::HeaderAdded { item: crate::HeaderItem::Tool { control } } | Self::HeaderRemoved { item: crate::HeaderItem::Tool { control } } => control.validate()?,
            _ => {},
        }
        Ok(())
    }
    pub fn display(&self, localization: &Localizer) -> String {
        use LayoutPanelAction as A;
        let mut args = FluentArgs::new();
        let id = match self {
            Self::Automatic => MessageId::WORKSPACE_HISTORY_REARRANGED,
            Self::Starting => MessageId::WORKSPACE_HISTORY_STARTING,
            Self::HeaderSize { size } => { args.set("size", localization.text(match size { crate::HeaderSize::Small => MessageId::WORKSPACE_SIZE_SMALL, crate::HeaderSize::Medium => MessageId::WORKSPACE_SIZE_MEDIUM, crate::HeaderSize::Large => MessageId::WORKSPACE_SIZE_LARGE }).to_string()); MessageId::WORKSPACE_HISTORY_HEADER_SIZE },
            Self::HeaderAdded { item } | Self::HeaderRemoved { item } => { args.set("item", header_label(*item, localization)); if matches!(self, Self::HeaderAdded { .. }) { MessageId::WORKSPACE_HISTORY_HEADER_ADDED } else { MessageId::WORKSPACE_HISTORY_HEADER_REMOVED } },
            Self::HeaderRearranged => MessageId::WORKSPACE_HISTORY_HEADER_REARRANGED,
            Self::CanvasInfo => MessageId::WORKSPACE_HISTORY_CANVAS_INFO,
            Self::CanvasBar => MessageId::WORKSPACE_HISTORY_CANVAS_BAR,
            Self::Restored => MessageId::WORKSPACE_HISTORY_RESTORED,
            Self::RestoredEarlier => MessageId::WORKSPACE_HISTORY_RESTORED_EARLIER,
            Self::Renamed { before, after } => { args.set("before", before.display(localization)); args.set("after", after.title(localization)); MessageId::WORKSPACE_HISTORY_RENAMED },
            Self::Panels { action, panels } => { args.set("panels", display_names(localization, panels, *action == A::Selected)); match action {
                A::Added => MessageId::WORKSPACE_HISTORY_ADDED, A::Deleted => MessageId::WORKSPACE_HISTORY_DELETED, A::Moved => MessageId::WORKSPACE_HISTORY_MOVED, A::Resized => MessageId::WORKSPACE_HISTORY_RESIZED, A::Collapsed => MessageId::WORKSPACE_HISTORY_COLLAPSED, A::Expanded => MessageId::WORKSPACE_HISTORY_EXPANDED, A::Hidden => MessageId::WORKSPACE_HISTORY_HIDDEN, A::Shown => MessageId::WORKSPACE_HISTORY_SHOWN, A::Rearranged => MessageId::WORKSPACE_HISTORY_REARRANGED, A::TabReordered => MessageId::WORKSPACE_HISTORY_TAB_REORDERED, A::TabsChanged => MessageId::WORKSPACE_HISTORY_TABS_CHANGED, A::ToolsAdded => MessageId::WORKSPACE_HISTORY_TOOLS_ADDED, A::ToolsRemoved => MessageId::WORKSPACE_HISTORY_TOOLS_REMOVED, A::ToolsChanged => MessageId::WORKSPACE_HISTORY_TOOLS_CHANGED, A::Customized => MessageId::WORKSPACE_HISTORY_CUSTOMIZED, A::Replaced => MessageId::WORKSPACE_HISTORY_REPLACED, A::Selected => MessageId::WORKSPACE_HISTORY_SELECTED,
            } },
        };
        if matches!(self, Self::Automatic) { args.set("panels", localization.text(MessageId::WORKSPACE_HISTORY_LAYOUT).to_string()); }
        localization.format(id, &args)
    }
}
fn display_names(localization: &Localizer, panels: &[LayoutPanelName], titles: bool) -> String {
    if panels.is_empty() { return localization.text(MessageId::WORKSPACE_HISTORY_LAYOUT).to_string(); }
    let names: Vec<_> = panels.iter().take(3).map(|p| if titles { p.title(localization) } else { p.display(localization) }).collect();
    let mut args = FluentArgs::new();
    for (key, name) in ["first", "second", "third"].into_iter().zip(&names) { args.set(key, name.as_str()); }
    args.set("others", panels.len().saturating_sub(3));
    localization.format(match panels.len() { 1 => MessageId::WORKSPACE_HISTORY_LIST_ONE, 2 => MessageId::WORKSPACE_HISTORY_LIST_TWO, 3 => MessageId::WORKSPACE_HISTORY_LIST_THREE, _ => MessageId::WORKSPACE_HISTORY_LIST_MORE }, &args)
}
pub(crate) fn panel_name(layout: &DockLayout, panel: Panel) -> LayoutPanelName {
    LayoutPanelName { panel, custom_name: layout.panel(panel).ok().and_then(|p| p.custom_name()).map(str::to_owned) }
}
fn node_panels(node: &DockNode, panels: &mut Vec<Panel>) {
    match node {
        DockNode::Tabs { panels: items, .. } => panels.extend(items),
        DockNode::Split { first, second, .. } => {
            node_panels(first, panels);
            node_panels(second, panels);
        }
    }
}
fn find(node: &DockNode, id: u32) -> Option<&DockNode> {
    if node.id() == id {
        return Some(node);
    }
    match node {
        DockNode::Split { first, second, .. } => find(first, id).or_else(|| find(second, id)),
        _ => None,
    }
}
fn names(layout: &DockLayout, panels: &[Panel]) -> Vec<LayoutPanelName> {
    panels.iter().map(|p| panel_name(layout, *p)).collect()
}
pub(crate) fn item_name(layout: &DockLayout, item: DockItem) -> Vec<LayoutPanelName> {
    let id = match item {
        DockItem::Panel { panel } | DockItem::Tile { panel, .. } => {
            return vec![panel_name(layout, panel)];
        }
        DockItem::Group { group } => group,
        DockItem::Column { column } => column,
    };
    let mut panels = Vec::new();
    if let Some(node) = layout
        .bands
        .iter()
        .map(|b| &b.root)
        .chain(layout.floating.iter().map(|f| &f.root))
        .find_map(|n| find(n, id))
    {
        node_panels(node, &mut panels);
    }
    names(layout, &panels)
}
pub fn layout_change_description(before: &DockLayout, after: &DockLayout) -> LayoutChange {
    use LayoutPanelAction as A;
    if before.header != after.header {
        return if before.header.size != after.header.size {
            LayoutChange::HeaderSize { size: after.header.size }
        } else if let Some(e) = after
            .header
            .entries()
            .find(|e| before.header.entry(e.id).is_err())
        {
            LayoutChange::HeaderAdded { item: e.item }
        } else if let Some(e) = before
            .header
            .entries()
            .find(|e| after.header.entry(e.id).is_err())
        {
            LayoutChange::HeaderRemoved { item: e.item }
        } else {
            LayoutChange::HeaderRearranged
        };
    }
    if before.canvas_info != after.canvas_info {
        return LayoutChange::CanvasInfo;
    }
    if before.canvas_bar != after.canvas_bar {
        return LayoutChange::CanvasBar;
    }
    let added: Vec<_> = after
        .panels
        .iter()
        .filter(|p| before.panel(p.id).is_err())
        .map(|p| p.id)
        .collect();
    if !added.is_empty() {
        return LayoutChange::panels(A::Added, names(after, &added));
    }
    let deleted: Vec<_> = before
        .panels
        .iter()
        .filter(|p| after.panel(p.id).is_err())
        .map(|p| p.id)
        .collect();
    if !deleted.is_empty() {
        return LayoutChange::panels(A::Deleted, names(before, &deleted));
    }
    for old in &before.panels {
        let Ok(new) = after.panel(old.id) else {
            continue;
        };
        let name = panel_name(after, old.id);
        if old.custom_name() != new.custom_name() {
            return LayoutChange::Renamed { before: panel_name(before, old.id), after: panel_name(after, old.id) };
        }
        if old.tiles() != new.tiles() {
            return if old.tiles().len() < new.tiles().len() {
                LayoutChange::panels(A::ToolsAdded, vec![name])
            } else if old.tiles().len() > new.tiles().len() {
                LayoutChange::panels(A::ToolsRemoved, vec![name])
            } else {
                LayoutChange::panels(A::ToolsChanged, vec![name])
            };
        }
        if old != new {
            return LayoutChange::panels(A::Customized, vec![name]);
        }
    }
    for panel in &after.panels {
        let (old, new) = (before.panel_group(panel.id), after.panel_group(panel.id));
        if old.is_some() && new.is_none() {
            return LayoutChange::panels(A::Hidden, vec![panel_name(after, panel.id)]);
        }
        if old.is_none() && new.is_some() {
            return LayoutChange::panels(A::Shown, vec![panel_name(after, panel.id)]);
        }
    }
    for (layout, other, action) in [(after, before, A::Collapsed), (before, after, A::Expanded)] {
        if let Some(column) = layout
            .collapsed
            .iter()
            .find(|c| !other.collapsed.iter().any(|o| o.root == c.root))
        {
            return LayoutChange::panels(
                action,
                item_name(
                    layout,
                    DockItem::Column {
                        column: column.root
                    }
                )
            );
        }
    }
    for new in &after.floating {
        if let Some(old) = before
            .floating
            .iter()
            .find(|f| f.root.id() == new.root.id())
        {
            let name = item_name(
                after,
                DockItem::Group {
                    group: new.root.id(),
                },
            );
            if old.position != new.position {
                return LayoutChange::panels(A::Moved, name);
            }
            if old.width != new.width || old.height != new.height {
                return LayoutChange::panels(A::Resized, name);
            }
        }
    }
    let moved: Vec<_> = after
        .panels
        .iter()
        .filter(|p| before.panel_group(p.id) != after.panel_group(p.id))
        .map(|p| p.id)
        .collect();
    if !moved.is_empty() {
        return LayoutChange::panels(A::Moved, names(after, &moved));
    }
    fn tab_change(old: &DockNode, new: &DockNode, layout: &DockLayout) -> Option<LayoutChange> {
        match new {
            DockNode::Tabs {
                id,
                panels,
                active,
                tab_style,
            } => {
                if let Some(DockNode::Tabs {
                    panels: old_panels,
                    active: old_active,
                    tab_style: old_style,
                    ..
                }) = find(old, *id)
                {
                    if panels != old_panels {
                        return Some(LayoutChange::panels(LayoutPanelAction::TabReordered, names(layout, panels)));
                    }
                    if active != old_active {
                        return Some(LayoutChange::panels(LayoutPanelAction::Selected, vec![panel_name(layout, *active)]));
                    }
                    if tab_style != old_style {
                        return Some(LayoutChange::panels(LayoutPanelAction::TabsChanged, names(layout, panels)));
                    }
                }
                None
            }
            DockNode::Split { first, second, .. } => {
                tab_change(old, first, layout).or_else(|| tab_change(old, second, layout))
            }
        }
    }
    for new in &after.bands {
        if let Some(old) = before.bands.iter().find(|b| b.id == new.id) {
            if let Some(description) = tab_change(&old.root, &new.root, after) {
                return description;
            }
            if old != new {
                return LayoutChange::panels(
                    A::Resized,
                    item_name(
                        after,
                        DockItem::Column {
                            column: new.root.id()
                        }
                    )
                );
            }
        }
    }
    for new in &after.floating {
        if let Some(old) = before
            .floating
            .iter()
            .find(|f| f.root.id() == new.root.id())
            && let Some(description) = tab_change(&old.root, &new.root, after)
        {
            return description;
        }
    }
    let panels: Vec<_> = after
        .panels
        .iter()
        .filter(|p| after.panel_group(p.id).is_some())
        .map(|p| p.id)
        .collect();
    LayoutChange::panels(A::Rearranged, names(after, &panels))
}

fn header_label(item: crate::HeaderItem, localization: &Localizer) -> String {
    item.localized_label(localization)
}
