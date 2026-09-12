//! Workspace storage has its own serial owner. No database call is made through
//! CapyApple, the UI thread, or the render/input queue.
use super::*;
use layer_ui::{ManagedWorkspace, Panel, PanelConfig, PreparedWorkspace, WorkspaceCapture};
use serde::Deserialize;
use serde_json::{Value, json};
#[path = "workspace_library.rs"]
mod library;
pub use library::*;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionRequest {
    Capture {
        generation: Option<u64>,
    },
    Begin,
    End,
    PreviewBegin,
    PreviewLayout {
        layout: layer_ui::DockLayout,
    },
    PreviewCancel,
    Adopt {
        #[serde(deserialize_with = "json_field")]
        capture: WorkspaceCapture,
    },
    Configure {
        binding: ManagedWorkspace,
    },
    ReadOnly {
        value: bool,
    },
    InstallToolbar {
        config: PanelConfig,
        replace: Option<Panel>,
        group: Option<u32>,
        #[serde(default)]
        exact_name: bool,
    },
}

// Internally tagged enum buffering loses JSON's integer-map-key decoder.
// Re-enter the JSON value deserializer for workspace aggregates, whose sparse
// brush overrides use stable integer IDs as object keys.
fn json_field<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned>(
    decoder: D,
) -> std::result::Result<T, D::Error> {
    serde_json::from_value(Value::deserialize(decoder)?).map_err(serde::de::Error::custom)
}

pub(crate) fn session_request(host: &mut NativeHost, value: Value) -> Result<Value, String> {
    let previous = host.session.state().revision;
    let change = match serde_json::from_value::<SessionRequest>(value).map_err(|e| e.to_string())? {
        SessionRequest::Capture { generation } => {
            let current = host.session.workspace_layout_generation();
            // A gesture can have transient topology and the same generation.
            // Never turn it into a persistent history capture.
            let capture = if generation.is_none() || current != generation {
                host.session.capture_workspace().ok()
            } else {
                None
            };
            return Ok(
                json!({"generation":host.session.workspace_layout_generation(),"capture":capture,
                "working":host.session.workspace_working_state(),
                "idle":host.session.require_workspace_idle().is_ok()}),
            );
        }
        SessionRequest::Begin => {
            host.session.begin_workspace_transition()?;
            match host.session.capture_workspace() {
                Ok(capture) => return Ok(json!({"capture":capture})),
                Err(error) => {
                    host.session.end_workspace_transition();
                    return Err(error);
                }
            }
        }
        SessionRequest::End => {
            host.session.end_workspace_transition();
            return Ok(Value::Null);
        }
        SessionRequest::PreviewBegin => {
            host.session.begin_workspace_layout_preview()?;
            return Ok(Value::Null);
        }
        SessionRequest::PreviewLayout { layout } => {
            host.session.preview_workspace_layout(&layout)?
        }
        SessionRequest::PreviewCancel => host.session.cancel_workspace_layout_preview(),
        SessionRequest::Adopt { capture } => host
            .session
            .adopt_workspace(PreparedWorkspace::new(capture)?)?,
        SessionRequest::Configure { binding } => {
            host.session.configure_workspace_manager(binding)?
        }
        SessionRequest::ReadOnly { value } => {
            host.session.set_workspace_read_only(value);
            return Ok(Value::Null);
        }
        SessionRequest::InstallToolbar {
            config,
            replace,
            group,
            exact_name,
        } => {
            // Explicit New Toolbar names use the shared collision validation.
            // Adding an unnamed library copy still allocates a unique suffix.
            if exact_name {
                let layer_ui::PanelContent::Toolbar { name, .. } = &config.content else {
                    return Err("Choose a toolbar".into());
                };
                let mut validation = host.session.state().workspace.layout.clone();
                if let Some(panel) = replace {
                    validation.rename_toolbar(panel, name)?;
                } else {
                    validation.validate_toolbar_name(name)?;
                }
            }
            let (panel, change) = host
                .session
                .install_workspace_toolbar(config, replace, group)?;
            host.apply_change(previous, change);
            return Ok(json!({"panel":panel}));
        }
    };
    host.apply_change(previous, change);
    Ok(Value::Null)
}
