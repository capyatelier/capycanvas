//! Stream shared workspace updates without allocating a second tree of models.
use layer_host::NativeHost;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub(crate) struct WindowsMetadata {
    pub windows_importing: bool,
    pub windows_image_import: Option<Value>,
    pub windows_isolated_settings: bool,
    pub windows_workspace: Option<crate::workspace_service::WorkspaceStatus>,
}

pub(crate) fn take(
    native: &mut NativeHost,
    metadata: &WindowsMetadata,
) -> Result<Option<Vec<u8>>, serde_json::Error> {
    // Serialize the extension before acknowledging a shared update. Both
    // serializers produce objects; append fields without parsing their content.
    let extension = serde_json::to_vec(metadata)?;
    let Some(mut bytes) = native.take_update_bytes()? else {
        return Ok(None);
    };
    let closing = bytes.pop();
    debug_assert_eq!(closing, Some(b'}'));
    bytes.push(b',');
    bytes.extend_from_slice(&extension[1..]);
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{ContactPhase, DockItem, Panel, Platform, UiAction};
    use serde_json::json;

    fn metadata() -> WindowsMetadata {
        WindowsMetadata {
            windows_importing: false,
            windows_image_import: None,
            windows_isolated_settings: true,
            windows_workspace: None,
        }
    }
    fn packet(host: &mut NativeHost) -> (Value, usize) {
        let bytes = take(host, &metadata()).unwrap().unwrap();
        (serde_json::from_slice(&bytes).unwrap(), bytes.len())
    }
    fn compare_full(old: &mut NativeHost, new: &mut NativeHost) -> Value {
        let expected: Value =
            serde_json::from_str(&old.take_snapshot().unwrap().to_string()).unwrap();
        let (mut actual, _) = packet(new);
        assert!(actual["workspace_update"].is_object());
        assert_eq!(actual["windows_isolated_settings"], true);
        assert_eq!(actual["windows_importing"], false);
        assert!(actual["windows_image_import"].is_null());
        for field in [
            "workspace_update",
            "windows_importing",
            "windows_image_import",
            "windows_isolated_settings",
            "windows_workspace",
        ] {
            actual.as_object_mut().unwrap().remove(field);
        }
        assert!(
            actual == expected,
            "Full models differ from the compatibility wire format"
        );
        actual
    }
    fn dispatch(old: &mut NativeHost, new: &mut NativeHost, action: UiAction) {
        old.dispatch(action.clone()).unwrap();
        new.dispatch(action).unwrap();
    }

    #[test]
    fn windows_motion_stream_preserves_models_geometry_camera_and_completion() {
        for cancel in [false, true] {
            let mut old = NativeHost::new(Platform::Windows).unwrap();
            let mut new = NativeHost::new(Platform::Windows).unwrap();
            for host in [&mut old, &mut new] {
                crate::workspace::initialize(host).unwrap();
                host.resize(986, 658, 1.).unwrap();
            }
            let initial = compare_full(&mut old, &mut new);
            let saved = initial["state"]["workspace"].clone();
            let bounds = old
                .session
                .layout([986., 658.])
                .groups
                .into_iter()
                .find(|g| g.active == Panel::Brushes)
                .unwrap()
                .bounds;
            let drag = |phase, position| UiAction::DragWorkspace {
                item: DockItem::Panel {
                    panel: Panel::Brushes,
                },
                phase,
                position,
                viewport: [986., 658.],
                tabs: vec![],
            };
            dispatch(
                &mut old,
                &mut new,
                drag(ContactPhase::Down, [bounds.x + 10., bounds.y + 10.]),
            );
            compare_full(&mut old, &mut new);
            dispatch(&mut old, &mut new, drag(ContactPhase::Move, [500., 350.]));
            let detached = compare_full(&mut old, &mut new);
            let mut full_bytes = 0;
            let mut motion_bytes = 0;
            let mut model_revision = None;
            for step in 1..=32 {
                dispatch(
                    &mut old,
                    &mut new,
                    drag(ContactPhase::Move, [500. + step as f32, 350. + step as f32]),
                );
                if step == 16 {
                    dispatch(
                        &mut old,
                        &mut new,
                        UiAction::Invoke {
                            command: layer_ui::CommandId::ZoomIn,
                        },
                    );
                }
                let legacy_wire = old.take_snapshot().unwrap().to_string();
                let legacy: Value = serde_json::from_str(&legacy_wire).unwrap();
                let (motion, size) = packet(&mut new);
                assert!(motion.get("state").is_none() && motion.get("layout").is_none());
                assert!(motion.get("workspace_persistence").is_none());
                let update = &motion["workspace_update"];
                if let Some(expected) = &model_revision {
                    assert_eq!(&update["model_revision"], expected);
                } else {
                    model_revision = Some(update["model_revision"].clone());
                }
                let moved = &update["drag"]["group"];
                let actual = legacy["layout"]["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|g| g["id"] == moved["id"])
                    .unwrap();
                assert_eq!(moved["bounds"], actual["bounds"]);
                if step == 16 {
                    assert_eq!(motion["camera"], legacy["state"]["camera"]);
                }
                assert_eq!(new.session.state().workspace, old.session.state().workspace);
                full_bytes += legacy_wire.len();
                motion_bytes += size;
            }
            assert!(motion_bytes < full_bytes / 10);
            println!("Windows 32 moves: full {full_bytes} bytes, incremental {motion_bytes} bytes");
            dispatch(
                &mut old,
                &mut new,
                drag(
                    if cancel {
                        ContactPhase::Cancel
                    } else {
                        ContactPhase::Up
                    },
                    [550., 400.],
                ),
            );
            let completed = compare_full(&mut old, &mut new);
            assert_eq!(new.session.state().workspace, old.session.state().workspace);
            assert_ne!(
                completed["state"]["workspace"],
                detached["state"]["workspace"]
            );
            if cancel {
                assert_eq!(completed["state"]["workspace"], saved);
            } else {
                assert!(completed.get("workspace_persistence").is_some());
                dispatch(
                    &mut old,
                    &mut new,
                    UiAction::Invoke {
                        command: layer_ui::CommandId::UndoWorkspace,
                    },
                );
                assert_eq!(
                    compare_full(&mut old, &mut new)["state"]["workspace"],
                    saved
                );
                dispatch(
                    &mut old,
                    &mut new,
                    UiAction::Invoke {
                        command: layer_ui::CommandId::RedoWorkspace,
                    },
                );
                assert_eq!(
                    compare_full(&mut old, &mut new)["state"]["workspace"],
                    completed["state"]["workspace"]
                );
            }
            assert!(take(&mut new, &metadata()).unwrap().is_none());
        }
    }

    #[test]
    fn windows_metadata_is_escaped_without_rewriting_shared_models() {
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        let mut extra = metadata();
        extra.windows_importing = true;
        extra.windows_image_import = Some(json!({"title":"Quoted \"layer\"\n\\","epoch":"42"}));
        let value: Value =
            serde_json::from_slice(&take(&mut host, &extra).unwrap().unwrap()).unwrap();
        assert_eq!(
            value["windows_image_import"],
            extra.windows_image_import.unwrap()
        );
        assert_eq!(value["windows_importing"], true);
        assert!(value["state"].is_object());
    }
}
