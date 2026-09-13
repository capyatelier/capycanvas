//! Browser measurements and capture feed the same title-bar policy as GTK.
use super::*;
use layer_ui::{
    HeaderDrag, HeaderDragSource, HeaderDragStart, HeaderItem, HeaderMetric, HeaderSize, Platform,
};
use serde_json::json;

#[derive(Deserialize)]
struct DragQuery {
    source: HeaderDragSource,
    width: f32,
    insets: [f32; 2],
    metrics: Vec<HeaderMetric>,
    press: [f32; 2],
    grab: layer_ui::Bounds,
}

#[wasm_bindgen]
impl WebApp {
    pub fn header_view(&self) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let model = state.workspace.layout.header.projected_for(Platform::Web);
        let items = model
            .entries()
            .map(|entry| {
                let (enabled, selected, icon) = match entry.item {
                    HeaderItem::Tool { control } => {
                        let (enabled, selected) = layer_ui::tool_state(state, control);
                        (enabled, selected, layer_ui::tool_choice(control).icon)
                    }
                    HeaderItem::Capy => (true, state.workspace.zen_mode, ""),
                    _ => (true, false, ""),
                };
                json!({"id":entry.id, "label":entry.item.label(), "enabled":enabled,
                "selected":selected, "icon":icon})
            })
            .collect::<Vec<_>>();
        js_sys::JSON::parse(&serde_json::to_string(&json!({"model":model, "items":items,
            "editing":state.customization.header_editing,
            "sizes":HeaderSize::ALL.map(|size| json!({"id":size,"label":size.label(),
                "tile":size.tile(),"icon":size.icon(),"height":size.height()})),
            "components":HeaderItem::COMPONENTS.into_iter().filter(|item| item.available_on(Platform::Web))
                .map(|item| json!({"item":item,"label":item.label(),"singleton":item.singleton()})).collect::<Vec<_>>(),
            "primary_menu":self.session.application_menu(layer_ui::ApplicationMenu::Primary)})).map_err(js)?)
    }

    pub fn header_geometry(
        &self,
        width: f32,
        insets: JsValue,
        metrics: JsValue,
    ) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let insets = serde_wasm_bindgen::from_value(insets).map_err(js)?;
        let metrics: Vec<HeaderMetric> = serde_wasm_bindgen::from_value(metrics).map_err(js)?;
        serialize(
            &state
                .workspace
                .layout
                .header
                .projected_for(Platform::Web)
                .resolve(width, insets, &metrics, state.customization.header_editing),
        )
    }

    pub fn begin_header_drag(&mut self, query: JsValue) -> Result<bool, JsValue> {
        self.header_drag = None;
        let q: DragQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let state = self.session.state();
        if !state.customization.header_editing {
            return Ok(false);
        }
        let model = state.workspace.layout.header.projected_for(Platform::Web);
        let geometry = model.resolve(q.width, q.insets, &q.metrics, true);
        self.header_drag = HeaderDrag::new(
            &model,
            HeaderDragStart {
                source: q.source,
                geometry,
                metrics: q.metrics,
                width: q.width,
                insets: q.insets,
                press: q.press,
                grab: q.grab,
            },
        );
        Ok(self.header_drag.is_some())
    }

    pub fn header_drag_preview(&mut self, x: f32, y: f32) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let model = state.workspace.layout.header.projected_for(Platform::Web);
        if !state.customization.header_editing
            || self
                .header_drag
                .as_ref()
                .is_some_and(|d| !d.is_current(&model))
        {
            self.header_drag = None;
        }
        serialize(&self.header_drag.as_mut().and_then(|d| d.preview([x, y])))
    }

    pub fn finish_header_drag(&mut self, x: f32, y: f32, cancel: bool) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let model = state.workspace.layout.header.projected_for(Platform::Web);
        let action = self
            .header_drag
            .take()
            .filter(|d| !cancel && state.customization.header_editing && d.is_current(&model))
            .and_then(|mut d| d.preview([x, y])?.action)
            .map(|a| a.action());
        serialize(&action)
    }

    pub fn header_step(&self, id: u32, forward: bool) -> Result<JsValue, JsValue> {
        serialize(
            &self
                .session
                .state()
                .workspace
                .layout
                .header
                .projected_for(Platform::Web)
                .step(id, forward)
                .map(|a| a.action()),
        )
    }
}
