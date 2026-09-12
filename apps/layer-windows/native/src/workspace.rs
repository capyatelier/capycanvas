//! CPU-only queries for native workspace projections. The general host query
//! API also installs filter packages; that mutating route is deliberately not
//! available through the optional workspace queue.
use crate::previews::CapyPreview;
use layer_host::NativeHost;
use serde_json::{Value, json};

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
