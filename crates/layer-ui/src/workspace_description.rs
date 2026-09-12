//! Human names for completed layout edits, derived from semantic changes.
use crate::{DockItem, DockLayout, DockNode, Panel, PanelKind};

pub(crate) fn panel_name(layout: &DockLayout, panel: Panel) -> String {
    let name = layout.panel(panel).map_or(panel.label(), |p| p.title());
    format!(
        "{name} {}",
        if panel.kind() == PanelKind::Tiles {
            "toolbar"
        } else {
            "panel"
        }
    )
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
fn names(layout: &DockLayout, panels: &[Panel]) -> String {
    let mut result = panels
        .iter()
        .take(3)
        .map(|p| panel_name(layout, *p))
        .collect::<Vec<_>>()
        .join(", ");
    if panels.len() > 3 {
        result.push_str(&format!(" and {} others", panels.len() - 3));
    }
    result
}
pub(crate) fn item_name(layout: &DockLayout, item: DockItem) -> String {
    let id = match item {
        DockItem::Panel { panel } | DockItem::Tile { panel, .. } => {
            return panel_name(layout, panel);
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
    if panels.is_empty() {
        "panel layout".into()
    } else {
        names(layout, &panels)
    }
}
pub fn layout_change_description(before: &DockLayout, after: &DockLayout) -> String {
    let added: Vec<_> = after
        .panels
        .iter()
        .filter(|p| before.panel(p.id).is_err())
        .map(|p| p.id)
        .collect();
    if !added.is_empty() {
        return format!("Added {}", names(after, &added));
    }
    let deleted: Vec<_> = before
        .panels
        .iter()
        .filter(|p| after.panel(p.id).is_err())
        .map(|p| p.id)
        .collect();
    if !deleted.is_empty() {
        return format!("Deleted {}", names(before, &deleted));
    }
    for old in &before.panels {
        let Ok(new) = after.panel(old.id) else {
            continue;
        };
        let name = panel_name(after, old.id);
        if old.title() != new.title() {
            return format!("Renamed {} to {}", panel_name(before, old.id), new.title());
        }
        if old.tiles() != new.tiles() {
            return if old.tiles().len() < new.tiles().len() {
                format!("Added tools to {name}")
            } else if old.tiles().len() > new.tiles().len() {
                format!("Removed tools from {name}")
            } else {
                format!("Changed tools in {name}")
            };
        }
        if old != new {
            return format!("Customized {name}");
        }
    }
    for panel in &after.panels {
        let (old, new) = (before.panel_group(panel.id), after.panel_group(panel.id));
        if old.is_some() && new.is_none() {
            return format!("Hid {}", panel_name(after, panel.id));
        }
        if old.is_none() && new.is_some() {
            return format!("Showed {}", panel_name(after, panel.id));
        }
    }
    for (layout, other, verb) in [(after, before, "Collapsed"), (before, after, "Expanded")] {
        if let Some(column) = layout
            .collapsed
            .iter()
            .find(|c| !other.collapsed.iter().any(|o| o.root == c.root))
        {
            return format!(
                "{verb} {}",
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
                return format!("Moved {name}");
            }
            if old.width != new.width || old.height != new.height {
                return format!("Resized {name}");
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
        return format!("Moved {}", names(after, &moved));
    }
    fn tab_change(old: &DockNode, new: &DockNode, layout: &DockLayout) -> Option<String> {
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
                        return Some(format!("Reordered {} tabs", names(layout, panels)));
                    }
                    if active != old_active {
                        return Some(format!(
                            "Selected {} tab",
                            layout.panel(*active).map_or(active.label(), |p| p.title())
                        ));
                    }
                    if tab_style != old_style {
                        return Some(format!("Changed tabs for {}", names(layout, panels)));
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
                return format!(
                    "Resized {}",
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
    format!("Rearranged {}", names(after, &panels))
}
