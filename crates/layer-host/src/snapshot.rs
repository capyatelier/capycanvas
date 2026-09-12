//! One snapshot schema for value consumers and streaming native transports.
use super::{NativeHost, SnapshotKey};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::{Value, json};

/// Value's serializer widens f32 to f64. Preserve those exact JSON numbers when
/// bypassing Value so geometry, color channels and settings retain their values.
struct SnapshotFormatter;
impl serde_json::ser::Formatter for SnapshotFormatter {
    fn write_f32<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        value: f32,
    ) -> std::io::Result<()> {
        serde_json::ser::Formatter::write_f64(
            &mut serde_json::ser::CompactFormatter,
            writer,
            f64::from(value),
        )
    }
}

impl NativeHost {
    /// Existing value consumers retain the same schema and publication policy.
    pub fn take_snapshot(&mut self) -> Option<Value> {
        self.take_snapshot_with(serde_json::value::Serializer)
            .expect("Native snapshot contains JSON-compatible fields")
    }

    /// Serialize directly from the shared models, avoiding an intermediate JSON
    /// tree and its allocation/destruction on the serial input/render owner.
    pub fn take_snapshot_bytes(&mut self) -> Result<Option<Vec<u8>>, serde_json::Error> {
        let mut bytes = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, SnapshotFormatter);
        Ok(self.take_snapshot_with(&mut serializer)?.map(|()| bytes))
    }

    fn take_snapshot_with<S: Serializer>(
        &mut self,
        serializer: S,
    ) -> Result<Option<S::Ok>, S::Error> {
        let key = SnapshotKey {
            revision: self.session.state().revision,
            logical: self.logical,
            chrome_hidden: self.chrome_hidden,
            hide_floating_panels: self.hide_floating_panels,
            keep_zen_button: self.keep_zen_button,
            gpu_ready: self.session.engine().backend().0.is_some(),
            startup: self.startup,
            error: self.error.clone(),
        };
        let camera = &self.session.state().camera;
        if self.last_snapshot.as_ref() == Some(&key) {
            if self.last_camera_revision != Some(camera.revision) {
                let snapshot =
                    json!({"camera": camera, "revision": key.revision}).serialize(serializer)?;
                self.last_camera_revision = Some(camera.revision);
                return Ok(Some(snapshot));
            }
            return Ok(None);
        }
        let workspace = self.session.durable_workspace();
        let changed_workspace = self.last_durable_workspace.as_ref() != Some(&workspace);
        let snapshot =
            self.serialize_snapshot(serializer, changed_workspace.then_some(&workspace))?;
        // A serializer failure cannot acknowledge an update that was not sent.
        self.last_snapshot = Some(key);
        self.last_camera_revision = Some(self.session.state().camera.revision);
        if changed_workspace {
            self.last_durable_workspace = Some(workspace);
        }
        Ok(Some(snapshot))
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Value {
        self.serialize_snapshot(serde_json::value::Serializer, None)
            .expect("Native snapshot contains JSON-compatible fields")
    }

    fn serialize_snapshot<S: Serializer>(
        &self,
        serializer: S,
        workspace: Option<&layer_ui::WorkspaceState>,
    ) -> Result<S::Ok, S::Error> {
        let layout = self.session.layout(self.logical);
        let state = self.session.state();
        let zen = if state.partial_zen() {
            state.workspace.layout.zen_toolbars(self.logical)
        } else {
            Default::default()
        };
        // Drawers and partial Zen can project panels absent from ordinary docks.
        let mut panel_ids: Vec<_> = layout
            .groups
            .iter()
            .flat_map(|g| &g.panels)
            .copied()
            .collect();
        for panel in zen
            .sections
            .iter()
            .map(|s| s.panel)
            .chain(
                layout
                    .collapsed
                    .iter()
                    .flat_map(|c| &c.groups)
                    .flat_map(|g| &g.icons)
                    .map(|i| i.panel),
            )
            .chain(
                state
                    .customization
                    .drawer
                    .iter()
                    .chain(&state.customization.column_drawers)
                    .flat_map(|d| d.columns.iter().flatten())
                    .copied(),
            )
        {
            if !panel_ids.contains(&panel) {
                panel_ids.push(panel);
            }
        }
        let panels: Vec<_> = panel_ids
            .into_iter()
            .filter_map(|p| self.session.panel_view(p).ok())
            .collect();
        #[derive(Serialize)]
        struct Menu {
            id: layer_ui::ApplicationMenu,
            label: &'static str,
            model: layer_ui::ContextMenu,
        }
        let menus = layer_ui::ApplicationMenu::ALL.map(|id| Menu {
            id,
            label: id.label(),
            model: self.session.application_menu(id),
        });
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("state", state)?;
        map.serialize_entry("layout", &layout)?;
        map.serialize_entry("panels", &panels)?;
        map.serialize_entry(
            "filter_preview_revision",
            &self.session.filter_preview_revision(),
        )?;
        map.serialize_entry("partial_zen", &state.partial_zen())?;
        map.serialize_entry("zen_toolbars", &zen)?;
        map.serialize_entry("application_menus", &menus)?;
        map.serialize_entry("color_panel", &state.colors.view())?;
        map.serialize_entry("document_options", &json!({"extent": layer_ui::DEFAULT_DOCUMENT_EXTENT,
            "max_dimension": layer_ui::MAX_NEW_DOCUMENT_DIMENSION,
            "width_label": layer_ui::DOCUMENT_WIDTH_LABEL, "height_label": layer_ui::DOCUMENT_HEIGHT_LABEL,
            "new_title": layer_ui::DocumentRequest::New.title(),
            "unsaved_description": layer_ui::UNSAVED_DESCRIPTION, "discard_label": layer_ui::DISCARD_DOCUMENT_LABEL,
            "cancel_label": layer_ui::CANCEL_DOCUMENT_LABEL,
            "save_label": layer_ui::DocumentRequest::ConfirmClose { title: String::new() }.accept_label(),
            "open_label": layer_ui::DocumentRequest::Open.accept_label(),
            "filter_label": layer_ui::DocumentRequest::Open.filter().0,
            "extension": layer_ui::DocumentRequest::Open.filter().1,
            "export_label": layer_ui::DocumentRequest::Export { name: String::new() }.accept_label(),
            "export_filter_label": layer_ui::DocumentRequest::Export { name: String::new() }.filter().0,
            "export_extension": layer_ui::DocumentRequest::Export { name: String::new() }.filter().1}))?;
        map.serialize_entry("preferences", &self.session.preferences())?;
        map.serialize_entry("picker", &self.session.tool_picker())?;
        map.serialize_entry("workspace_menu", &self.session.workspace_menu())?;
        map.serialize_entry("toolbar_prompt", &self.session.toolbar_prompt())?;
        map.serialize_entry("toolbar_manager", &self.session.toolbar_manager())?;
        map.serialize_entry("panel_measurements", &state.workspace.layout.measurements)?;
        map.serialize_entry("chrome_hidden", &self.chrome_hidden)?;
        let gpu_ready = self.session.engine().backend().0.is_some();
        map.serialize_entry("gpu_ready", &gpu_ready)?;
        map.serialize_entry("hide_floating_panels", &self.hide_floating_panels)?;
        map.serialize_entry("keep_zen_button", &self.keep_zen_button)?;
        map.serialize_entry("canvas_ready", &(gpu_ready && self.startup.canvas_ready))?;
        map.serialize_entry("brush_ready", &(gpu_ready && self.startup.brush_ready))?;
        map.serialize_entry("shaders_ready", &(gpu_ready && self.startup.complete))?;
        map.serialize_entry("error", &self.error)?;
        if let Some(workspace) = workspace {
            map.serialize_entry("workspace_persistence", workspace)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{CommandId, Platform, UiAction, WorkspaceState};

    fn host(platform: Platform) -> NativeHost {
        let mut host = NativeHost::new(platform).unwrap();
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: WorkspaceState::for_platform(platform),
        })
        .unwrap();
        host.resize(2410, 1810, 2.).unwrap();
        host
    }
    fn decoded(bytes: Option<Vec<u8>>) -> Option<Value> {
        bytes.map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }
    fn same(value: &mut NativeHost, stream: &mut NativeHost) {
        // Compare the actual legacy wire representation, including its float
        // formatting, rather than relying on an in-memory Number comparison.
        let legacy = value
            .take_snapshot()
            .map(|v| serde_json::to_vec(&v).unwrap());
        assert_eq!(
            decoded(stream.take_snapshot_bytes().unwrap()),
            decoded(legacy)
        );
        assert!(value.take_snapshot().is_none());
        assert!(stream.take_snapshot_bytes().unwrap().is_none());
    }
    #[test]
    fn streamed_snapshots_preserve_fields_values_and_publication_policy() {
        for platform in [
            Platform::Ios,
            Platform::Mac,
            Platform::Android,
            Platform::Windows,
        ] {
            let (mut value, mut stream) = (host(platform), host(platform));
            same(&mut value, &mut stream);
            for action in [
                UiAction::SetBrushSize { value: 37.3 },
                UiAction::SetColor {
                    rgba: [0.1, 0.2, 0.7, 0.43],
                },
                UiAction::Invoke {
                    command: CommandId::ZoomIn,
                },
                UiAction::OpenSettings {
                    page: layer_ui::SettingsPage::Input,
                },
                UiAction::CloseSettings,
                UiAction::Invoke {
                    command: CommandId::ZenMode,
                },
            ] {
                value.dispatch(action.clone()).unwrap();
                stream.dispatch(action).unwrap();
                same(&mut value, &mut stream);
            }
            for host in [&mut value, &mut stream] {
                host.error = Some("Synthetic surface error: \"quoted\" and λ\n".into());
                host.chrome_hidden = true;
                host.resize(1810, 2410, 2.).unwrap();
            }
            same(&mut value, &mut stream);
            for host in [&mut value, &mut stream] {
                host.error = None;
                host.document_adopted();
            }
            same(&mut value, &mut stream);
        }
    }

    #[test]
    fn direct_formatter_matches_value_float_precision_in_nested_values() {
        let values = [
            0.1f32,
            37.3,
            f32::MIN_POSITIVE,
            f32::MAX,
            -0.,
            f32::NAN,
            f32::INFINITY,
        ];
        let nested = (
            Some(values),
            vec![values],
            [f64::MIN_POSITIVE, 0.1, f64::MAX],
        );
        let expected = serde_json::to_vec(&serde_json::to_value(&nested).unwrap()).unwrap();
        let mut actual = Vec::new();
        nested
            .serialize(&mut serde_json::Serializer::with_formatter(
                &mut actual,
                SnapshotFormatter,
            ))
            .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn failed_serialization_keeps_full_camera_and_workspace_updates_pending() {
        struct FailedWriter;
        impl std::io::Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic write failure"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let fail = |host: &mut NativeHost| {
            let mut serializer =
                serde_json::Serializer::with_formatter(FailedWriter, SnapshotFormatter);
            assert!(host.take_snapshot_with(&mut serializer).is_err());
        };
        let mut host = host(Platform::Mac);
        fail(&mut host);
        assert!(
            decoded(host.take_snapshot_bytes().unwrap())
                .unwrap()
                .get("workspace_persistence")
                .is_some()
        );
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        // The first edit can also synchronize initial document controls. Consume
        // that state before testing a subsequent camera-only publication.
        host.take_snapshot().unwrap();
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        })
        .unwrap();
        fail(&mut host);
        assert!(host.take_snapshot().unwrap().get("camera").is_some());
        host.dispatch(UiAction::Invoke {
            command: CommandId::ZenMode,
        })
        .unwrap();
        fail(&mut host);
        assert!(
            decoded(host.take_snapshot_bytes().unwrap())
                .unwrap()
                .get("workspace_persistence")
                .is_some()
        );
        assert!(host.take_snapshot().is_none());
    }
}
