//! CPU-only queries for native workspace projections. The general host query
//! API also installs filter packages; that mutating route is deliberately not
//! available through the optional workspace queue.
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
            | "stroke_recording",
        ) => Ok(value),
        _ => Err("Unsupported workspace query".into()),
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StampQuery {
    ToolbarStamp { context: layer_ui::ToolbarContext },
}

pub fn query(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    if let Ok(StampQuery::ToolbarStamp { context }) = serde_json::from_str(json) {
        return match host.session.toolbar_stamp(context) {
            Ok(stamp) => CapyPreview::packet(
                json!({"result":{"size":stamp.size,"extent":stamp.extent},"error":null}),
                stamp.alpha,
            ),
            Err(error) => CapyPreview::packet(json!({"result":null,"error":error}), Vec::new()),
        };
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
        let group = host.session.engine().document().active_layer.0;
        let epoch = host.session.state().document_file.epoch;
        let before = host.session.engine().document().layers.clone();
        for requested_epoch in [epoch, epoch + 1] {
            let reply = metadata(query(&mut host, &json!({
                "type":"layer_drop", "epoch":requested_epoch, "id":1, "target":group, "fraction":0.5
            }).to_string()).unwrap());
            assert!(reply["error"].is_null());
            assert_eq!(reply["result"]["epoch"], epoch);
            assert_eq!(
                reply["result"]["position"],
                if requested_epoch == epoch {
                    json!("into")
                } else {
                    Value::Null
                }
            );
            assert_eq!(host.session.engine().document().layers, before);
        }
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::Undo,
        })
        .unwrap();
        assert!(
            host.session
                .engine()
                .document()
                .layer(layer_core::LayerId(group))
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
    fn header_queries_keep_drag_motion_out_of_workspace_history() {
        use layer_ui::{HeaderAction, HeaderZone, Platform};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        initialize(&mut host).unwrap();
        let before = host.session.durable_workspace();
        host.dispatch(HeaderAction::Edit { editing: true }.action())
            .unwrap();
        let layout = host.session.state().workspace.layout.header.clone();
        let id = layout.zones[0][0].id;
        let metrics: Vec<_> = layout
            .entries()
            .map(|e| json!({"id":e.id,"width":56,"compact":56}))
            .collect();
        let call = |host: &mut NativeHost, request: Value| {
            let reply = metadata(
                query(
                    host,
                    &json!({"type":"header","request":request}).to_string(),
                )
                .unwrap(),
            );
            assert!(reply["error"].is_null(), "{reply}");
            reply["result"].clone()
        };
        let geometry = call(
            &mut host,
            json!({
                "op":"geometry","width":1400,"insets":[0,150],"metrics":metrics
            }),
        );
        let grab = geometry["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap()["bounds"]
            .clone();
        let begin = json!({"op":"begin","source":{"kind":"item","value":id},
            "width":1400,"insets":[0,150],"metrics":metrics,
            "press":[grab["x"].as_f64().unwrap()+10.,grab["y"].as_f64().unwrap()+10.],
            "grab":grab});
        for cancel in [true, false] {
            assert_eq!(call(&mut host, begin.clone()), true);
            assert!(!call(&mut host, json!({"op":"preview","position":[700,20]})).is_null());
            assert_eq!(host.session.state().workspace.layout.header, layout);
            assert_eq!(host.session.durable_workspace(), before);
            let action = call(
                &mut host,
                json!({"op":"finish","position":[700,20],"cancel":cancel}),
            );
            if cancel {
                assert!(action.is_null());
            } else {
                host.dispatch(serde_json::from_value(action).unwrap())
                    .unwrap();
                assert_eq!(
                    host.session
                        .state()
                        .workspace
                        .layout
                        .header
                        .location(id)
                        .unwrap()
                        .0,
                    HeaderZone::Center
                );
                assert_eq!(host.session.durable_workspace(), before);
                host.dispatch(HeaderAction::Cancel.action()).unwrap();
                assert_eq!(host.session.state().workspace.layout.header, layout);
            }
        }
        assert_eq!(call(&mut host, begin), false);
        assert!(
            call(
                &mut host,
                json!({"op":"finish","position":[700,20],"cancel":false})
            )
            .is_null()
        );
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
