//! Native DOM geometry and shared editor models. Painting remains in the GPU surface.
use super::*;
use serde_json::json;

#[derive(Deserialize)]
pub(super) struct OverviewSlot {
    bounds: [f32; 4],
    clip: [f32; 4],
    order: i32,
}

#[derive(Deserialize)]
struct DrawerQuery {
    viewport: [f32; 2],
    column: Option<u32>,
    heights: Vec<f32>,
    progress: f32,
    from: Option<layer_ui::DrawerPlacement>,
    #[serde(default)]
    closing: bool,
}

#[wasm_bindgen]
impl WebApp {
    pub fn editor_models(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        js_sys::JSON::parse(&serde_json::to_string(&json!({
            "color_panel": state.colors.view(),
            "partial_zen": state.partial_zen(),
            "zen_toolbars": if state.partial_zen() { state.workspace.layout.zen_toolbars([width,height]) } else { Default::default() },
            "application_menus": layer_ui::ApplicationMenu::ALL.map(|menu| json!({"id":menu, "label":menu.label(), "model":self.session.application_menu(menu)})),
            "document_options": json!({
                "extent": layer_ui::DEFAULT_DOCUMENT_EXTENT,
                "max_dimension": layer_ui::MAX_NEW_DOCUMENT_DIMENSION,
                "width_label": layer_ui::DOCUMENT_WIDTH_LABEL,
                "height_label": layer_ui::DOCUMENT_HEIGHT_LABEL,
                "new_title": layer_ui::DocumentRequest::New.title(),
                "unsaved_description": layer_ui::UNSAVED_DESCRIPTION,
                "discard_label": layer_ui::DISCARD_DOCUMENT_LABEL,
            }),
        })).map_err(js)?)
    }
    pub fn color_panel(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().colors.view())
    }
    pub fn workspace_projection(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        serialize(&(
            state.partial_zen(),
            if state.partial_zen() {
                state.workspace.layout.zen_toolbars([width, height])
            } else {
                Default::default()
            },
        ))
    }
    pub fn workspace_persistence(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.durable_workspace())
    }
    pub fn color_wheel_hit(&self, size: f32, x: f32, y: f32) -> Result<JsValue, JsValue> {
        serialize(
            &layer_ui::ColorWheelGeometry::new(size)
                .and_then(|g| g.hit([x, y], self.session.state().colors.space)),
        )
    }
    pub fn navigator_geometry(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        let document = self.session.engine().document();
        serialize(&layer_ui::NavigatorGeometry::new(
            &self.session.state().camera,
            [document.width, document.height],
            [width, height],
        ))
    }
    pub fn navigator_placements(&mut self, placements: JsValue) -> Result<(), JsValue> {
        let mut slots: Vec<OverviewSlot> =
            serde_wasm_bindgen::from_value(placements).map_err(js)?;
        if slots.len() > 32
            || slots
                .iter()
                .any(|s| !s.bounds.iter().chain(&s.clip).all(|v| v.is_finite()))
        {
            return Err(js("Invalid Navigator geometry"));
        }
        slots.sort_by_key(|s| s.order);
        self.overviews = slots;
        Ok(())
    }
    pub fn drawer(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: DrawerQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let state = self.session.state();
        let drawer = match q.column {
            None => state.customization.drawer.as_ref(),
            Some(id) => state.customization.column_drawers.iter().find(|d|
                matches!(d.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
        };
        let end = if q.closing {
            q.from.as_ref().map(|p| p.closed())
        } else {
            drawer.and_then(|d| {
                state.customization.drawer_placement(
                    d,
                    &state.workspace.layout,
                    q.viewport,
                    &q.heights,
                    state.partial_zen(),
                )
            })
        };
        serialize(&end.map(|end| {
            let from = q.from.unwrap_or_else(|| end.closed());
            let placement = end.interpolate_from(&from, q.progress);
            json!({"connection":placement.connection(), "placement":placement})
        }))
    }
    pub fn drawer_toolbar(&self, panel: JsValue, width: f32) -> Result<JsValue, JsValue> {
        if !width.is_finite() || width <= 0. {
            return Err(js("Invalid drawer width"));
        }
        let panel = serde_wasm_bindgen::from_value(panel).map_err(js)?;
        let config = self
            .session
            .state()
            .workspace
            .layout
            .panel(panel)
            .map_err(js)?;
        let height = layer_ui::toolbar_content_height(width, config.tiles(), config.tile_style);
        let geometry = layer_ui::toolbar_tile_layout(
            width,
            height,
            layer_ui::Axis::Vertical,
            config.tiles(),
            false,
            config.tile_style,
        );
        let mut value = json!(geometry);
        value["content_height"] = json!(height);
        serialize(&value)
    }
    pub fn application_link(&self, link: JsValue) -> Result<String, JsValue> {
        let link: layer_ui::ApplicationLink = serde_wasm_bindgen::from_value(link).map_err(js)?;
        Ok(link.url().into())
    }
}

impl WebApp {
    pub(super) fn overview_placements(&self) -> Vec<layer_render_wgpu::OverviewPlacement> {
        let Some(gpu) = &self.session.engine().backend().0 else {
            return Vec::new();
        };
        if !gpu.blank_presented || !self.startup.canvas_ready {
            return Vec::new();
        }
        let state = self.session.state();
        let document = self.session.engine().document();
        let scale = self.canvas.width() as f32 / self.canvas.client_width().max(1) as f32;
        // Slots are supplied in device pixels; use DOM's CSS scale for shared geometry.
        self.overviews
            .iter()
            .filter_map(|slot| {
                let [x, y, w, h] = slot.bounds;
                let g = layer_ui::NavigatorGeometry::new(
                    &state.camera,
                    [document.width, document.height],
                    [w / scale, h / scale],
                )?;
                let fg = state.palette.text.linear();
                let bg = state.palette.panel.linear();
                Some(layer_render_wgpu::OverviewPlacement {
                    bounds: [
                        x + g.image.x * scale,
                        y + g.image.y * scale,
                        g.image.width * scale,
                        g.image.height * scale,
                    ],
                    clip: Some(slot.clip),
                    work_area: g.work_area.map(|[a, b]| [x + a * scale, y + b * scale]),
                    outline_linear: [fg[0], fg[1], fg[2]],
                    background_linear: [bg[0], bg[1], bg[2]],
                    scale,
                    opacity: 1.,
                })
            })
            .collect()
    }
}
