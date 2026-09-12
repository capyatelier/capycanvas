//! Follow the rendered tool choices instead of duplicating the list of modes.
use super::*;
use std::collections::{BTreeSet, VecDeque};

pub(super) fn inventory(platform: Platform) -> Vec<Value> {
    let catalog = layer_ui::ui_catalog();
    let mut queue: VecDeque<Vec<UiAction>> = catalog
        .tool_commands
        .iter()
        .map(|&command| vec![UiAction::Invoke { command }])
        .collect();
    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    let mut host = apple_host(platform);
    let hardware = std::env::args().any(|arg| arg == "--gpu");
    let preparation = if hardware {
        let gpu = layer_render_wgpu::WgpuRasterizer::new_headless().unwrap();
        assert_ne!(
            gpu.adapter().get_info().device_type,
            wgpu::DeviceType::Cpu,
            "GPU inventory requires a hardware adapter"
        );
        host.session.renderer_mut().0 = Some(gpu);
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
