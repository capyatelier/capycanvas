//! Native measurements/capture use the same frozen title-bar drag as GTK/Web.
use super::*;
use layer_ui::{Bounds, HeaderDrag, HeaderDragSource, HeaderDragStart, HeaderMetric};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum HeaderRequest {
    Geometry {
        width: f32,
        insets: [f32; 2],
        metrics: Vec<HeaderMetric>,
    },
    Begin {
        source: HeaderDragSource,
        width: f32,
        insets: [f32; 2],
        metrics: Vec<HeaderMetric>,
        press: [f32; 2],
        grab: Bounds,
    },
    Preview {
        position: [f32; 2],
    },
    Finish {
        position: [f32; 2],
        cancel: bool,
    },
    Step {
        id: u32,
        forward: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{HeaderAction, HeaderZone, Platform, WorkspaceState};

    #[test]
    fn native_capture_uses_shared_preview_and_never_persists_motion_or_cancellation() {
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState::for_platform(Platform::Android)),
        })
        .unwrap();
        let before = host.session.durable_workspace();
        host.dispatch(UiAction::Invoke { command: layer_ui::CommandId::CustomizeWorkspaceUi })
            .unwrap();
        let model = host.session.state().workspace.layout.header.clone();
        let id = model.zones[0][0].id;
        let metrics: Vec<_> = model
            .entries()
            .map(|e| HeaderMetric {
                id: e.id,
                width: 56.,
                compact: 56.,
            })
            .collect();
        let geometry = model.resolve(1400., [0., 0.], &metrics, true);
        let grab = geometry.items.iter().find(|m| m.id == id).unwrap().bounds;
        let press = [grab.x + 10., grab.y + 10.];
        let begin = || HeaderRequest::Begin {
            source: HeaderDragSource::Item(id),
            width: 1400.,
            insets: [0., 0.],
            metrics: metrics.clone(),
            press,
            grab,
        };
        assert_eq!(host.header_request(begin()), json!(true));
        let preview = host.header_request(HeaderRequest::Preview {
            position: [700., 20.],
        });
        assert!(!preview.is_null());
        assert_eq!(host.session.state().workspace.layout.header, model);
        assert_eq!(host.session.durable_workspace(), before);
        assert!(
            host.header_request(HeaderRequest::Finish {
                position: [700., 20.],
                cancel: true
            })
            .is_null()
        );
        assert_eq!(host.header_request(begin()), json!(true));
        let action = host.header_request(HeaderRequest::Finish {
            position: [700., 20.],
            cancel: false,
        });
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
        assert_eq!(host.session.state().workspace.layout.header, model);
        assert_eq!(host.header_request(begin()), json!(false));
        host.dispatch(HeaderAction::Edit { editing: true }.action())
            .unwrap();
        assert_eq!(host.header_request(begin()), json!(true));
        host.dispatch(HeaderAction::Remove { id }.action()).unwrap();
        assert!(
            host.header_request(HeaderRequest::Finish {
                position: [700., 20.],
                cancel: false
            })
            .is_null()
        );
    }
}

impl NativeHost {
    pub(crate) fn header_request(&mut self, request: HeaderRequest) -> Value {
        let state = self.session.state();
        let model = state.workspace.layout.header.projected_for(state.platform);
        let editing = state.customization.header_editing;
        if !editing
            || self
                .header_drag
                .as_ref()
                .is_some_and(|d| !d.is_current(&model))
        {
            self.header_drag = None;
        }
        match request {
            HeaderRequest::Geometry {
                width,
                insets,
                metrics,
            } => json!(model.resolve(width, insets, &metrics, editing)),
            HeaderRequest::Begin {
                source,
                width,
                insets,
                metrics,
                press,
                grab,
            } => {
                self.header_drag = editing
                    .then(|| {
                        HeaderDrag::new(
                            &model,
                            HeaderDragStart {
                                source,
                                geometry: model.resolve(width, insets, &metrics, true),
                                width,
                                insets,
                                metrics,
                                press,
                                grab,
                            },
                        )
                    })
                    .flatten();
                json!(self.header_drag.is_some())
            }
            HeaderRequest::Preview { position } => {
                json!(self.header_drag.as_mut().and_then(|d| d.preview(position)))
            }
            HeaderRequest::Finish { position, cancel } => json!(
                self.header_drag
                    .take()
                    .filter(|_| !cancel)
                    .and_then(|mut d| d.preview(position)?.action)
                    .map(|a| a.action())
            ),
            HeaderRequest::Step { id, forward } => json!(
                editing
                    .then(|| model.step(id, forward))
                    .flatten()
                    .map(|a| a.action())
            ),
        }
    }
}
