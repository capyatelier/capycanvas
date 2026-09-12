//! CPU-only queries for native workspace projections. The general host query
//! API also installs filter packages; that mutating route is deliberately not
//! available through the optional workspace queue.
use crate::previews::CapyPreview;
use layer_host::NativeHost;
use serde_json::{Value, json};

// Match Android's native startup before optional preferences restore a workspace.
pub(crate) fn initialize(native: &mut NativeHost) -> Result<(), String> {
    native.dispatch(layer_ui::UiAction::RestoreWorkspace {
        workspace: layer_ui::WorkspaceState::for_platform(layer_ui::Platform::Windows),
    })
}

fn request(json: &str) -> Result<Value, String> {
    if json.len() > 8192 {
        return Err("Workspace query is too large".into());
    }
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    match value.get("type").and_then(Value::as_str) {
        Some(
            "context"
            | "panel_handle_target"
            | "drop"
            | "workspace_drag_preview"
            | "drawer"
            | "drawer_toolbar"
            | "expansion"
            | "renderer_stats"
            | "application_menu"
            | "application_link",
        ) => Ok(value),
        _ => Err("Unsupported workspace query".into()),
    }
}

pub fn query(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    // A delayed query can reference a group removed by intervening input.
    // Return that rejection to the view instead of failing the render loop.
    let (result, error) = match request(json).and_then(|value| host.query(value)) {
        Ok(result) => (result, Value::Null),
        Err(error) => (Value::Null, Value::String(error)),
    };
    CapyPreview::packet(json!({"result":result,"error":error}), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn metadata(packet: CapyPreview) -> Value {
        let owned = Box::into_raw(Box::new(packet));
        // Exercise the same CPU packet interface as the C++ projection.
        unsafe {
            let json = std::ffi::CStr::from_ptr(crate::previews::capy_preview_metadata(owned));
            let result = serde_json::from_slice(json.to_bytes()).unwrap();
            let mut count = usize::MAX;
            crate::previews::capy_preview_bytes(owned, &mut count);
            assert_eq!(count, 0);
            crate::previews::capy_preview_free(owned);
            result
        }
    }
    #[test]
    fn native_startup_uses_editor_preset_and_saved_workspaces_remain_authoritative() {
        use layer_ui::{Platform, UiAction, WorkspaceState};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        initialize(&mut host).unwrap();
        assert_eq!(
            host.session.state().workspace,
            WorkspaceState::for_platform(Platform::Android)
        );
        assert!(host.session.state().requests.is_empty());
        let saved = WorkspaceState::default();
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: saved.clone(),
        })
        .unwrap();
        assert_eq!(host.session.state().workspace, saved);
        assert!(host.session.state().requests.is_empty());
    }
    #[test]
    fn titlebar_measurements_are_transient_validated_and_published() {
        use layer_ui::{Platform, UiAction};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        initialize(&mut host).unwrap();
        let saved = serde_json::to_value(&host.session.state().workspace).unwrap();
        let revision = host.session.engine().document().revision;
        host.session.begin_workspace_transition().unwrap();
        for insets in [[0., 138., 48.], [0., 92., 32.], [0.; 3]] {
            host.dispatch(UiAction::MeasureTitlebar { insets }).unwrap();
            assert_eq!(
                host.take_snapshot().unwrap()["titlebar_insets"],
                json!(insets)
            );
            assert_eq!(
                serde_json::to_value(&host.session.state().workspace).unwrap(),
                saved
            );
            assert!(host.session.state().requests.is_empty());
            assert_eq!(host.session.engine().document().revision, revision);
        }
        host.session.end_workspace_transition();
        let captured = host.session.capture_workspace().unwrap();
        assert_eq!(captured.history.revisions.len(), 1);
        host.dispatch(UiAction::MeasureTitlebar {
            insets: [0., 144., 48.],
        })
        .unwrap();
        host.session
            .restore_workspace_layout(layer_ui::DockLayout::default(), "Reset")
            .unwrap();
        assert_eq!(
            host.session.state().workspace.layout.titlebar_insets,
            [0., 144., 48.]
        );
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::UndoWorkspace,
        })
        .unwrap();
        assert_eq!(
            host.session.state().workspace.layout.titlebar_insets,
            [0., 144., 48.]
        );
        host.session
            .adopt_workspace(layer_ui::PreparedWorkspace::new(captured).unwrap())
            .unwrap();
        assert_eq!(
            host.session.state().workspace.layout.titlebar_insets,
            [0., 144., 48.]
        );
        host.dispatch(UiAction::MeasureTitlebar { insets: [0.; 3] })
            .unwrap();
        for insets in [[-1., 0., 0.], [0., f32::NAN, 0.], [0., 0., 1_000_000.]] {
            assert!(host.dispatch(UiAction::MeasureTitlebar { insets }).is_err());
            assert_eq!(
                host.session.state().workspace.layout.titlebar_insets,
                [0.; 3]
            );
        }
    }
    #[test]
    fn workspace_queries_follow_shared_policy_without_mutating_the_document() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision = host.session.engine().document().revision;
        for query in [
            json!({"type":"application_menu","menu":"edit"}),
            json!({"type":"application_link","link":"website"}),
            json!({"type":"application_link","link":"source_code"}),
            json!({"type":"renderer_stats"}),
            json!({"type":"panel_handle_target","item":{"kind":"panel","panel":"layers"}}),
            json!({"type":"drawer","column":null,"heights":[],"progress":1}),
        ] {
            let expected = host.query(query.clone()).unwrap();
            let result = metadata(super::query(&mut host, &query.to_string()).unwrap());
            assert_eq!(result["result"], expected);
            assert!(result["error"].is_null());
        }
        assert_eq!(revision, host.session.engine().document().revision);
    }
    #[test]
    fn windows_tab_capture_and_preview_use_frozen_shared_insertion() {
        use layer_ui::{Bounds, ContactPhase, DockItem, Panel, TabHit, UiAction};
        for offset in [2., 70.] {
            let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
            initialize(&mut host).unwrap();
            host.resize(986, 658, 1.).unwrap();
            let before = host.session.state().workspace.layout.clone();
            let revision = host.session.engine().document().revision;
            let group = host
                .session
                .layout([986., 658.])
                .groups
                .into_iter()
                .find(|g| g.panels.contains(&Panel::ToolSettings))
                .unwrap();
            let clip = Bounds {
                height: 36.,
                ..group.bounds
            };
            let tabs = vec![
                TabHit {
                    group: group.id,
                    index: 0,
                    bounds: Bounds { width: 80., ..clip },
                },
                TabHit {
                    group: group.id,
                    index: 1,
                    bounds: Bounds {
                        x: clip.x + 80.,
                        width: 120.,
                        ..clip
                    },
                },
            ];
            let press = [clip.x + offset, clip.y + 18.];
            let item = DockItem::Panel {
                panel: Panel::ToolSettings,
            };
            let action = |phase, position| UiAction::DragWorkspace {
                item,
                phase,
                position,
                viewport: [986., 658.],
                tabs: tabs.clone(),
            };
            let down: crate::actions::Action = serde_json::from_value(json!({
                "windows_tab_drag":{"tabs":tabs,"clip":clip},
                "action":action(ContactPhase::Down,press)
            }))
            .unwrap();
            down.dispatch(&mut host).unwrap();
            let moved = [press[0] + 65., press[1]];
            host.dispatch(action(ContactPhase::Move, moved)).unwrap();
            let query =
                json!({"type":"workspace_drag_preview","item":item,"tabs":tabs,"position":moved});
            let result = metadata(super::query(&mut host, &query.to_string()).unwrap());
            assert!(result["error"].is_null());
            assert_eq!(result["result"]["tab"]["insertion"], 2);
            assert_eq!(result["result"]["tab"]["offsets"][1]["x"], -80.);
            // Release without an additional motion uses the identical partition.
            host.dispatch(action(ContactPhase::Up, moved)).unwrap();
            assert_eq!(
                host.session
                    .state()
                    .workspace
                    .layout
                    .group_panels(group.id)
                    .unwrap(),
                &[Panel::Sizes, Panel::ToolSettings]
            );
            let ended = metadata(super::query(&mut host, &query.to_string()).unwrap());
            assert!(ended["result"]["tab"].is_null());
            host.dispatch(UiAction::Invoke {
                command: layer_ui::CommandId::UndoWorkspace,
            })
            .unwrap();
            assert_eq!(host.session.state().workspace.layout, before);
            assert_eq!(host.session.engine().document().revision, revision);
        }
    }
    #[test]
    fn windows_help_commands_resolve_and_acknowledge_shared_links() {
        use layer_ui::{ApplicationLink, CommandId, HostRequestKind, UiAction};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision = host.session.engine().document().revision;
        for (command, link) in [
            (CommandId::Website, ApplicationLink::Website),
            (CommandId::SourceCode, ApplicationLink::SourceCode),
        ] {
            host.dispatch(UiAction::Invoke { command }).unwrap();
            let id = host.session.state().requests.iter().find(|request|
                matches!(request.kind, HostRequestKind::OpenLink { link: actual } if actual == link)
            ).unwrap().id;
            let query = json!({"type":"application_link","link":link}).to_string();
            let result = metadata(super::query(&mut host, &query).unwrap());
            assert_eq!(result["result"], link.url());
            host.dispatch(UiAction::CompleteRequest { id, error: None })
                .unwrap();
            assert!(
                host.session
                    .state()
                    .requests
                    .iter()
                    .all(|request| request.id != id)
            );
        }
        assert_eq!(revision, host.session.engine().document().revision);
    }
    #[test]
    fn windows_drawer_queries_measure_and_close_after_the_model_is_removed() {
        use layer_ui::{CustomizationAction, Panel, ToolbarControl, UiAction};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(1600, 1000, 1.).unwrap();
        let tile = host
            .session
            .state()
            .workspace
            .layout
            .panel(Panel::Toolbar)
            .unwrap()
            .tiles()
            .iter()
            .find(|tile| tile.control == ToolbarControl::Color)
            .unwrap()
            .id;
        host.dispatch(UiAction::ActivateTile {
            panel: Panel::Toolbar,
            tile,
        })
        .unwrap();
        // One measured height is required per model column, even before layout.
        let invalid = metadata(
            super::query(&mut host, r#"{"type":"drawer","heights":[],"progress":1}"#).unwrap(),
        );
        assert!(invalid["result"].is_null());
        let open = metadata(
            super::query(
                &mut host,
                r#"{"type":"drawer","heights":[360],"progress":1}"#,
            )
            .unwrap(),
        );
        let placement = open["result"]["placement"].clone();
        assert_eq!(placement["bounds"]["width"], 280.);
        assert_eq!(placement["bounds"]["height"], 360.);
        assert!(open["result"]["connection"].is_object());
        let revision = host.session.engine().document().revision;
        host.dispatch(UiAction::Customize {
            action: CustomizationAction::CloseExpanded,
        })
        .unwrap();
        assert!(host.session.state().customization.drawer.is_none());
        for progress in [0., 1.] {
            let request = json!({"type":"drawer","heights":[360],"progress":progress,"from":placement,"closing":true});
            let closed = metadata(super::query(&mut host, &request.to_string()).unwrap());
            let bounds = &closed["result"]["placement"]["bounds"];
            if progress == 0. {
                assert_eq!(bounds, &placement["bounds"]);
            } else {
                assert_eq!(bounds["height"], 0.);
            }
        }
        assert_eq!(host.session.engine().document().revision, revision);
    }
    #[test]
    fn windows_expansion_retains_presented_geometry_on_resize_and_close() {
        use layer_ui::{CustomizationAction, Panel, UiAction};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(1600, 1000, 1.).unwrap();
        let revision = host.session.engine().document().revision;
        for panel in [Panel::Sizes, Panel::Toolbar] {
            host.dispatch(UiAction::Customize {
                action: CustomizationAction::ShowAllControls { panel },
            })
            .unwrap();
            let request = json!({"type":"expansion","panel":panel,"heights":[0,420],"progress":1});
            let open = metadata(super::query(&mut host, &request.to_string()).unwrap());
            assert!(open["error"].is_null());
            let placement = open["result"].clone();
            assert_eq!(placement["configuration"]["width"], 380.);
            if panel == Panel::Toolbar {
                let layout = host.session.layout([1600., 1000.]);
                let group = layout
                    .groups
                    .iter()
                    .find(|g| g.panels.contains(&panel))
                    .unwrap();
                let config = host.session.state().workspace.layout.panel(panel).unwrap();
                let expected = layer_ui::toolbar_tile_layout(
                    placement["preview"]["width"].as_f64().unwrap() as f32,
                    (placement["preview"]["height"].as_f64().unwrap()
                        - placement["configuration"]["y"].as_f64().unwrap())
                        as f32,
                    group.axis,
                    config.tiles(),
                    !group.tabs_visible,
                    config.tile_style,
                );
                let expected: Value = serde_json::from_str(&json!(expected).to_string()).unwrap();
                assert_eq!(placement["tiles"], expected);
            }
            host.resize(1000, 700, 1.).unwrap();
            let request = json!({"type":"expansion","panel":panel,"heights":[0,420],
                "from":placement,"progress":0});
            let resized = metadata(super::query(&mut host, &request.to_string()).unwrap());
            assert_eq!(resized["result"]["bounds"], placement["bounds"]);
            assert_eq!(resized["result"]["preview"], placement["preview"]);
            host.dispatch(UiAction::Customize {
                action: CustomizationAction::CloseExpanded,
            })
            .unwrap();
            assert!(host.session.state().customization.expanded.is_none());
            for progress in [0., 1.] {
                let request = json!({"type":"expansion","panel":panel,"heights":[0,420],
                    "from":placement,"progress":progress,"closing":true});
                let result = metadata(super::query(&mut host, &request.to_string()).unwrap());
                assert!(result["error"].is_null());
                if progress == 0. {
                    assert_eq!(result["result"]["bounds"], placement["bounds"]);
                } else {
                    let layout = host.session.layout([1000., 700.]);
                    let group = layout
                        .groups
                        .iter()
                        .find(|g| g.panels.contains(&panel))
                        .unwrap();
                    let expected: Value =
                        serde_json::from_str(&json!(group.bounds).to_string()).unwrap();
                    assert_eq!(result["result"]["bounds"], expected);
                }
            }
            host.resize(1600, 1000, 1.).unwrap();
        }
        assert_eq!(host.session.engine().document().revision, revision);
    }
    #[test]
    fn optional_queries_reject_mutations_oversize_and_stale_geometry_without_failing_host() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision = host.session.engine().document().revision;
        for query in [
            r#"{"type":"load_filter_package"}"#.to_owned(),
            r#"{"type":"invoke","command":"new_layer"}"#.to_owned(),
            r#"{"type":"drawer_toolbar","panel":"layers","width":0,"height":480}"#.to_owned(),
            " ".repeat(8193),
            "{".to_owned(),
        ] {
            let result = metadata(super::query(&mut host, &query).unwrap());
            assert!(result["result"].is_null());
            assert!(result["error"].is_string());
        }
        assert_eq!(revision, host.session.engine().document().revision);
        assert!(
            metadata(super::query(&mut host, r#"{"type":"renderer_stats"}"#).unwrap())["error"]
                .is_null()
        );
    }
}
