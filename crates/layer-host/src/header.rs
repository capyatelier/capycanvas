//! Native measurements/capture use the same frozen title-bar drag as GTK/Web.
use super::*;
use layer_ui::{Bounds, HeaderDrag, HeaderDragSource, HeaderDragStart, HeaderLayout, HeaderMetric};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub(crate) enum HeaderRequest {
    Geometry(layer_ui::HeaderGeometryRequest),
    Begin {
        model: HeaderLayout,
        editing: bool,
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

impl NativeHost {
    pub(crate) fn header_request(&mut self, request: HeaderRequest) -> Result<Value,String> {
        let state = self.session.state();
        let model = state.workspace.layout.header.projected_for(state.platform);
        let editing = state.customization.header_editing;
        if !matches!(request, HeaderRequest::Geometry(_)) && (!editing
            || self
                .header_drag
                .as_ref()
                .is_some_and(|d| !d.is_current(&model)))
        {
            self.header_drag = None;
        }
        Ok(match request {
            HeaderRequest::Geometry(request) => return request.resolve(),
            HeaderRequest::Begin {
                model: measured,
                editing: measured_editing,
                source,
                width,
                insets,
                metrics,
                press,
                grab,
            } => {
                let available = match source {
                    HeaderDragSource::Component(item) => item.available_on(state.platform),
                    _ => true,
                };
                self.header_drag = (editing && measured_editing && available && measured == model)
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
        })
    }
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
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::CustomizeWorkspaceUi,
        })
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
            model: model.clone(),
            editing: true,
            source: HeaderDragSource::Item(id),
            width: 1400.,
            insets: [0., 0.],
            metrics: metrics.clone(),
            press,
            grab,
        };
        let mut blocked = begin();
        if let HeaderRequest::Begin { editing, .. } = &mut blocked { *editing = false; }
        assert_eq!(host.header_request(blocked).unwrap(), json!(false));
        assert_eq!(host.header_request(begin()).unwrap(), json!(true));
        let finish = |cancel| HeaderRequest::Finish { position: [700., 20.], cancel };
        let measure = || HeaderRequest::Geometry(layer_ui::HeaderGeometryRequest { model: model.clone(), editing: true, documents: 1,
            width: 1400., insets: [0., 0.], metrics: metrics.clone() });
        assert_eq!(host.header_request(measure()).unwrap(), json!(geometry));
        assert!(host.header_drag.is_some());
        let preview = host.header_request(HeaderRequest::Preview {
            position: [700., 20.],
        }).unwrap();
        assert!(!preview.is_null());
        assert_eq!(host.session.state().workspace.layout.header, model);
        assert_eq!(host.session.durable_workspace(), before);
        assert!(host.header_request(finish(true)).unwrap().is_null());
        assert_eq!(host.header_request(begin()).unwrap(), json!(true));
        let action = host.header_request(finish(false)).unwrap();
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
        assert_eq!(host.header_request(begin()).unwrap(), json!(false));
        host.dispatch(HeaderAction::Edit { editing: true }.action())
            .unwrap();
        host.dispatch(HeaderAction::SetSize { size: layer_ui::HeaderSize::Large }.action()).unwrap();
        assert_eq!(host.header_request(measure()).unwrap(), json!(geometry));
        assert_eq!(host.header_request(begin()).unwrap(), json!(false));
        host.dispatch(HeaderAction::SetSize { size: model.size }.action()).unwrap();
        assert_eq!(host.header_request(begin()).unwrap(), json!(true));
        host.dispatch(HeaderAction::Remove { id }.action()).unwrap();
        assert_eq!(host.header_request(measure()).unwrap(), json!(geometry));
        assert!(host.header_request(finish(false)).unwrap().is_null());
        assert_eq!(host.header_request(begin()).unwrap(), json!(false));
        host.dispatch(HeaderAction::Cancel.action()).unwrap();
        assert_eq!(host.header_request(measure()).unwrap(), json!(geometry));
    }
}
