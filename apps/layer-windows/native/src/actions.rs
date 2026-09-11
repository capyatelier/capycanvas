//! Cold UI actions may carry the document epoch of the widget or draft.
//! The render owner checks it after earlier queued document work has completed.
use layer_host::NativeHost;
use layer_ui::UiAction;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum Action {
    Document {
        windows_epoch: String,
        action: UiAction,
    },
    Ordinary(UiAction),
}
impl Action {
    pub(crate) fn dispatch(self, host: &mut NativeHost) -> Result<(), String> {
        let action = match self {
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
