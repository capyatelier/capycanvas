//! Cold UI actions may carry the document epoch of the widget or draft.
//! The render owner checks it after earlier queued document work has completed.
use layer_host::NativeHost;
use layer_ui::{Bounds, ContactPhase, DockItem, TabHit, UiAction};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Action {
    TabDrag {
        windows_tab_drag: TabCapture,
        action: UiAction,
    },
    Document {
        windows_epoch: String,
        action: UiAction,
    },
    Ordinary(UiAction),
}
#[derive(Deserialize)]
pub(crate) struct TabCapture {
    tabs: Vec<TabHit>,
    clip: Bounds,
}
impl TabCapture {
    fn valid(&self) -> bool {
        let valid = |b: Bounds| {
            [b.x, b.y, b.width, b.height]
                .into_iter()
                .all(|v| v.is_finite() && v.abs() < 1_000_000.)
                && b.width > 0.
                && b.height > 0.
        };
        !self.tabs.is_empty()
            && self.tabs.len() <= 1024
            && valid(self.clip)
            && self.tabs.iter().all(|tab| valid(tab.bounds))
    }
}
impl Action {
    pub(crate) fn dispatch(self, host: &mut NativeHost) -> Result<(), String> {
        let action = match self {
            Self::TabDrag {
                windows_tab_drag,
                action,
            } => {
                if !matches!(
                    action,
                    UiAction::DragWorkspace {
                        phase: ContactPhase::Down,
                        item: DockItem::Panel { .. },
                        ..
                    }
                ) || !windows_tab_drag.valid()
                {
                    return Err("Invalid tab drag capture".into());
                }
                // Capture must follow Down and precede every queued Move/Up.
                // The optional query queue must never start a delayed gesture.
                host.dispatch(action)?;
                host.dispatch(UiAction::BeginTabDrag {
                    tabs: windows_tab_drag.tabs,
                    clip: windows_tab_drag.clip,
                })?;
                return Ok(());
            }
            Self::Document {
                windows_epoch,
                action,
            } => {
                let expected = windows_epoch
                    .parse::<u64>()
                    .map_err(|_| "Invalid document epoch")?;
                if expected != host.session.state().document_file.epoch {
                    return Ok(()); // An obsolete widget has no authority over a new drawing.
                }
                action
            }
            Self::Ordinary(action) => action,
        };
        host.dispatch(action)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_tab_capture_cannot_start_or_mutate_a_workspace_gesture() {
        use serde_json::json;
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        crate::workspace::initialize(&mut host).unwrap();
        host.resize(986, 658, 1.).unwrap();
        let before = host.session.state().workspace.clone();
        let revision = host.session.engine().document().revision;
        let bounds = json!({"x":86.,"y":320.,"width":80.,"height":36.});
        let tab = json!({"group":7,"index":0,"bounds":bounds});
        let capture = json!({"tabs":[tab],"clip":bounds});
        let action = json!({"type":"drag_workspace","item":{"kind":"panel","panel":"tool_settings"},
            "phase":"down","position":[90.,338.],"viewport":[986.,658.],"tabs":[]});
        let mut cases = Vec::new();
        let mut empty = capture.clone();
        empty["tabs"] = json!([]);
        cases.push((empty, action.clone()));
        let mut zero = capture.clone();
        zero["clip"]["width"] = json!(0.);
        cases.push((zero, action.clone()));
        let mut invalid = capture.clone();
        invalid["tabs"][0]["bounds"]["x"] = json!(1_000_000.);
        cases.push((invalid, action.clone()));
        let mut other = action.clone();
        other["phase"] = json!("move");
        cases.push((capture.clone(), other));
        cases.push((capture, json!({"type":"set_brush_opacity","value":0.2})));
        let opacity = host.session.state().brush.opacity;
        for (capture, action) in cases {
            let queued: Action = serde_json::from_value(json!({
                "windows_tab_drag":capture,"action":action
            }))
            .unwrap();
            assert!(queued.dispatch(&mut host).is_err());
            assert_eq!(host.session.state().workspace, before);
            assert_eq!(host.session.state().brush.opacity, opacity);
            assert!(host.session.tab_drag_preview([180., 338.]).is_none());
            assert_eq!(host.session.engine().document().revision, revision);
        }
    }
    #[test]
    fn queued_widget_actions_cannot_edit_a_replacement_document() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let epoch = host.session.state().document_file.epoch;
        let action = |epoch: u64| {
            serde_json::from_value::<Action>(serde_json::json!({
            "windows_epoch":epoch.to_string(),"action":{"type":"set_layer_opacity","opacity":0.4}
        })).unwrap()
        };
        action(epoch + 1).dispatch(&mut host).unwrap();
        assert_eq!(
            host.session
                .state()
                .layer_tools
                .editing_layer
                .as_ref()
                .unwrap()
                .opacity,
            1.
        );
        action(epoch).dispatch(&mut host).unwrap();
        assert_eq!(
            host.session
                .state()
                .layer_tools
                .editing_layer
                .as_ref()
                .unwrap()
                .opacity,
            0.4
        );
        let ordinary: Action =
            serde_json::from_str(r#"{"type":"set_layer_opacity","opacity":0.7}"#).unwrap();
        ordinary.dispatch(&mut host).unwrap();
        assert_eq!(
            host.session
                .state()
                .layer_tools
                .editing_layer
                .as_ref()
                .unwrap()
                .opacity,
            0.7
        );
        let malformed: Action = serde_json::from_str(
            r#"{"windows_epoch":"-1","action":{"type":"set_layer_opacity","opacity":0.2}}"#,
        )
        .unwrap();
        assert!(malformed.dispatch(&mut host).is_err());
    }
}
