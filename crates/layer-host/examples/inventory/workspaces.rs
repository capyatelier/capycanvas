//! Complete shipped panel registrations and their actual contextual commands.
use super::*;
use layer_ui::{ContextTarget, PanelContent};

pub(super) fn inventory(platform: Platform) -> Vec<Value> {
    WorkspacePreset::ALL.into_iter().map(|preset| {
        let host = workspace_host(platform, preset);
        let layout = &host.session.state().workspace.layout;
        let mut contexts = vec![ContextTarget::ZenMode];
        let mut groups = std::collections::BTreeSet::new();
        for panel in &layout.panels {
            contexts.push(ContextTarget::Panel { panel: panel.id });
            if let Some(group) = layout.panel_group(panel.id) { groups.insert(group); }
            if let PanelContent::Toolbar { tiles, .. } = &panel.content {
                contexts.push(ContextTarget::Ribbon { panel: panel.id });
                contexts.extend(tiles.iter().map(|tile| ContextTarget::Tile { panel: panel.id, tile: tile.id }));
            }
        }
        contexts.extend(groups.into_iter().map(|group| ContextTarget::Group { group }));
        json!({"name":preset.name(),"workspace":host.session.state().workspace,
            "working":host.session.workspace_working_state(),
            "panels":layout.panels.iter().map(|panel|host.session.panel_view(panel.id).unwrap()).collect::<Vec<_>>(),
            "contexts":contexts.into_iter().map(|target| json!({"target":target,
                "menu":host.session.context_menu(target).unwrap()})).collect::<Vec<_>>(),
            "menus":layer_ui::ApplicationMenu::ALL.map(|id| json!({"id":id,"model":host.session.application_menu(id)}))})
    }).collect()
}
