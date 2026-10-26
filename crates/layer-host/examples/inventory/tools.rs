//! Traverse catalog entry points and their rendered tool choices.
use super::*;
use std::collections::{BTreeSet, VecDeque};

pub(super) fn inventory(platform: Platform) -> Vec<Value> {
    let catalog = layer_ui::ui_catalog();
    let mut queue: VecDeque<Vec<UiAction>> = catalog
        .tool_commands
        .iter()
        .map(|&command| vec![UiAction::Invoke { command }])
        .chain(catalog.brush_categories.iter().flat_map(|category| &category.brushes)
            .map(|brush| vec![UiAction::SelectBrush { id: brush.id }]))
        .collect();
    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    let mut host = apple_host(platform);
    let hardware = std::env::args().any(|arg| arg == "--gpu");
    let preparation = if hardware {
        let gpu =
            layer_render_wgpu::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(
            gpu.adapter().get_info().device_type,
            wgpu::DeviceType::Cpu,
            "GPU inventory requires a hardware adapter"
        );
        host.session.renderer_mut().0 = Some(gpu.into());
        // Transform controls require real content and a renderer-owned preview.
        // Fill a disposable drawing through ordinary actions, without UI input.
        let actions = [
            CommandId::SelectAll,
            CommandId::FillSelection,
            CommandId::Deselect,
        ]
        .map(|command| UiAction::Invoke { command });
        for action in &actions {
            host.dispatch(action.clone()).unwrap();
        }
        host.session.frame(0, 0).unwrap();
        actions.to_vec()
    } else {
        Vec::new()
    };
    let baseline = host.session.capture_workspace().unwrap();
    let editing = host.session.editing_state();
    while let Some(path) = queue.pop_front() {
        let action = path.last().unwrap();
        if !visited.insert(serde_json::to_string(action).unwrap()) {
            continue;
        }
        // Tool choice actions contain their resolved mode. Replaying the path
        // also preserves the source family's subtool memory and setting schema.
        if host.session.command(CommandId::CancelTransform).enabled {
            host.dispatch(UiAction::Invoke {
                command: CommandId::CancelTransform,
            })
            .unwrap();
        }
        host.session
            .adopt_workspace(PreparedWorkspace::new(baseline.clone()).unwrap())
            .unwrap();
        host.session.restore_editing(editing.clone()).unwrap();
        let error = path
            .iter()
            .find_map(|step| host.dispatch(step.clone()).err());
        let state = host.session.state();
        if error.is_none() {
            for item in state.tool_set.groups.iter().chain(&state.tool_set.subtools) {
                let mut next = path.clone();
                next.push(item.action.clone());
                queue.push_back(next);
            }
        }
        result.push(json!({"select":action,"path":path,"preparation":preparation,
            "hardware_renderer":hardware,"error":error,"tool_set":state.tool_set,
            "settings":state.tool_settings,"actions":state.tool_actions.iter()
                .map(|item| json!({"checkable":item.checkable,"command":host.session.command(item.command)})).collect::<Vec<_>>() }));
        assert!(result.len() < 1024, "Tool choice graph did not converge");
    }
    result
}

#[test]
fn tool_paths_restore_remembered_brush_families() {
    for platform in [Platform::Mac, Platform::Ios] {
        let choices = inventory(platform);
        for brush in layer_ui::ui_catalog().brush_categories.iter().flat_map(|category| &category.brushes) {
            let selected = json!(UiAction::SelectBrush { id: brush.id });
            let visits: Vec<_> = choices.iter().filter(|choice| choice["select"] == selected).collect();
            assert_eq!(visits.len(), 1, "Every catalog preset must have its own inventory path: {selected}");
            assert!(visits[0]["error"].is_null(), "{}", visits[0]);
        }
        let sets: Vec<_> = choices.iter().filter(|choice| choice["select"]["type"] == "select_brush_set").collect();
        assert!(!sets.is_empty());
        for set in sets { assert!(set["error"].is_null(), "{set}"); }
    }
}
