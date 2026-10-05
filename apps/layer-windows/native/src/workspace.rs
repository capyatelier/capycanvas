//! CPU-only queries for native workspace projections.
use crate::previews::CapyPreview;
use layer_host::NativeHost;
use serde_json::{Value, json};

// Match Android's native startup before optional preferences restore a workspace.
pub(crate) fn initialize(native: &mut NativeHost) -> Result<(), String> {
    native.dispatch(layer_ui::UiAction::RestoreWorkspace {
        workspace: Box::new(layer_ui::WorkspaceState::for_platform(
            layer_ui::Platform::Windows,
        )),
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
            | "layer_drop"
            | "image_layer_drop"
            | "drawer"
            | "drawer_toolbar"
            | "expansion"
            | "renderer_stats"
            | "application_menu"
            | "application_link"
            | "header"
            | "palette_menu"
            | "palette_reorder_preview"
            | "palette_action"
            | "reveal_panel"
            | "canvas_bar_layout"
            | "canvas_bar_menu"
            | "canvas_bar_choice_menu"
            | "zoom_menu"
            | "action_tooltip"
            | "stroke_recording",
        ) => Ok(value),
        _ => Err("Unsupported workspace query".into()),
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StampQuery {
    ToolbarStamp { context: layer_ui::ToolbarContext },
    Scopes { #[serde(default)] size: Option<[u32; 2]> },
}

pub fn query(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    match serde_json::from_str(json) {
        Ok(StampQuery::ToolbarStamp { context }) => return match host.session.toolbar_stamp(context) {
            Ok(stamp) => CapyPreview::packet(
                json!({"result":{"size":stamp.size,"extent":stamp.extent},"error":null}),
                stamp.alpha,
            ),
            Err(error) => CapyPreview::packet(json!({"result":null,"error":error}), Vec::new()),
        },
        Ok(StampQuery::Scopes { size }) => return crate::scopes::query(host.session.state(), size),
        Err(_) => {}
    }
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
    use crate::test_support::*;
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
    fn layer_drop_query_validates_epoch_and_preserves_document_and_history() {
        use layer_ui::{LayerAction, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.dispatch(UiAction::Layer {
            action: LayerAction::New {
                group: true,
                clipped: false,
            },
        })
        .unwrap();
        let group = host.session.engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap();
        let epoch = host.session.state().document_file.epoch;
        let before = host.session.engine().document().clone();
        for requested_epoch in [epoch, epoch + 1] {
            let reply = metadata(query(&mut host, &json!({
                "type":"layer_drop", "epoch":requested_epoch, "id":1, "target":group, "fraction":0.5, "surface":"row"
            }).to_string()).unwrap());
            assert!(reply["error"].is_null());
            assert_eq!(reply["result"]["epoch"], epoch);
            assert_eq!(reply["result"]["target"], if requested_epoch == epoch { json!(group) } else { Value::Null });
            assert_eq!(
                reply["result"]["position"],
                if requested_epoch == epoch {
                    json!("into")
                } else {
                    Value::Null
                }
            );
            assert_authored_eq(host.session.engine().document(),&before);
        }
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::Undo,
        })
        .unwrap();
        assert!(
            host.session
                .engine()
                .document()
                .scene().occurrence(layer_ui::occurrence_handle(group).unwrap())
                .is_none()
        );
    }

    #[test]
    fn toolbar_stamp_returns_alpha_bytes_and_rejects_stale_context() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let context = host.session.state().toolbar_context();
        let packet = query(
            &mut host,
            &json!({"type":"toolbar_stamp","context":context}).to_string(),
        )
        .unwrap();
        let owned = Box::into_raw(Box::new(packet));
        unsafe {
            let json = std::ffi::CStr::from_ptr(crate::previews::capy_preview_metadata(owned));
            let reply: Value = serde_json::from_slice(json.to_bytes()).unwrap();
            assert!(reply["error"].is_null(), "{reply}");
            let size = reply["result"]["size"].as_u64().unwrap() as usize;
            assert!(reply["result"]["extent"].as_f64().unwrap() > 0.);
            let mut count = 0;
            let bytes = crate::previews::capy_preview_bytes(owned, &mut count);
            assert_eq!(count, size * size);
            assert!(std::slice::from_raw_parts(bytes, count).iter().any(|&a| a > 0));
            crate::previews::capy_preview_free(owned);
        }
        let mut stale = context;
        stale.generation += 1;
        let reply = metadata(
            query(&mut host, &json!({"type":"toolbar_stamp","context":stale}).to_string()).unwrap(),
        );
        assert!(reply["result"].is_null());
        assert!(reply["error"].is_string());
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
            workspace: Box::new(saved.clone()),
        })
        .unwrap();
        assert_eq!(host.session.state().workspace, saved);
        assert!(host.session.state().requests.is_empty());
    }
    #[test]
    fn titlebar_measurements_publish_native_snapshot_insets() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        initialize(&mut host).unwrap();
        host.session.begin_workspace_transition().unwrap();
        for insets in [[0., 138., 48.], [0., 92., 32.], [0.; 3]] {
            host.dispatch(layer_ui::UiAction::MeasureTitlebar { insets }).unwrap();
            let snapshot: Value = serde_json::from_slice(&host.take_update_bytes().unwrap().unwrap()).unwrap();
            assert_eq!(snapshot["titlebar_insets"], json!(insets));
        }
        host.session.end_workspace_transition();
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
            json!({"type":"expansion","panel":"toolbar","heights":[0,420],"progress":1}),
            json!({"type":"header","request":{"op":"geometry","width":1400,"insets":[0,0],"metrics":[]}}),
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
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        initialize(&mut host).unwrap();
        host.resize(986, 658, 1.).unwrap();
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
        let press = [clip.x + 2., clip.y + 18.];
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
        let query = json!({"type":"workspace_drag_preview","item":item,"tabs":tabs,"position":moved});
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
