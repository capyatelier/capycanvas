//! Native DOM geometry and shared editor models. Painting remains in the GPU surface.
use super::*;
use serde_json::json;

// Each DOM Navigator owns a GPU surface at native resolution. The browser
// retains its pixels while workspace transforms move the enclosing controls.
pub(super) struct NavigatorSurface {
    canvas: web_sys::HtmlCanvasElement,
    size: [f32; 2],
    scale: f32,
    gpu: Option<(
        wgpu::Surface<'static>,
        ViewportPresenter,
        wgpu::SurfaceConfiguration,
    )>,
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
    pub fn navigator_surface(&mut self, id: u32, canvas: web_sys::HtmlCanvasElement) {
        self.overviews.insert(
            id,
            NavigatorSurface {
                canvas,
                size: [0.; 2],
                scale: 1.,
                gpu: None,
            },
        );
    }
    pub fn remove_navigator_surface(&mut self, id: u32) {
        self.overviews.remove(&id);
    }
    pub fn navigator_size(
        &mut self,
        id: u32,
        width: f32,
        height: f32,
        scale: f32,
    ) -> Result<(), JsValue> {
        if ![width, height, scale].iter().all(|v| v.is_finite())
            || width < 0.
            || height < 0.
            || scale <= 0.
        {
            return Err(js("Invalid Navigator geometry"));
        }
        if let Some(slot) = self.overviews.get_mut(&id) {
            slot.size = [width, height];
            slot.scale = scale;
        }
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
    pub(super) fn present_navigators(&mut self) -> Result<bool, JsValue> {
        if !self.startup.canvas_ready {
            return Ok(false);
        }
        let state = self.session.state();
        let camera = state.camera.clone();
        let fg = state.palette.text.linear();
        let bg = state.palette.panel.linear();
        let document = self.session.engine().document();
        let extent = [document.width, document.height];
        let gpu = self.session.renderer_mut().0.as_mut().unwrap();
        let mut retry = false;
        for slot in self.overviews.values_mut() {
            let Some(g) = layer_ui::NavigatorGeometry::new(&camera, extent, slot.size) else {
                continue;
            };
            let scale = slot.scale;
            let width = (slot.size[0] * scale).round().max(1.) as u32;
            let height = (slot.size[1] * scale).round().max(1.) as u32;
            if slot.gpu.is_none() {
                let surface = gpu
                    .instance
                    .create_surface(wgpu::SurfaceTarget::Canvas(slot.canvas.clone()))
                    .map_err(js)?;
                let mut config = gpu.config.clone();
                config.width = width;
                config.height = height;
                config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
                slot.canvas.set_width(width);
                slot.canvas.set_height(height);
                surface.configure(gpu.renderer.device(), &config);
                let presenter = ViewportPresenter::for_overviews(&gpu.renderer, config.format);
                slot.gpu = Some((surface, presenter, config));
            }
            let (surface, presenter, config) = slot.gpu.as_mut().unwrap();
            if config.width != width || config.height != height {
                config.width = width;
                config.height = height;
                slot.canvas.set_width(width);
                slot.canvas.set_height(height);
                surface.configure(gpu.renderer.device(), config);
            }
            let target = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(target)
                | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
                wgpu::CurrentSurfaceTexture::Lost => {
                    slot.gpu = None;
                    retry = true;
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    surface.configure(gpu.renderer.device(), config);
                    retry = true;
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Timeout => {
                    retry = true;
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Occluded => continue,
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(js("Navigator WebGPU surface validation failed"));
                }
            };
            presenter.set_overviews(
                &gpu.renderer,
                &[layer_render_wgpu::OverviewPlacement {
                    bounds: [
                        g.image.x * scale,
                        g.image.y * scale,
                        g.image.width * scale,
                        g.image.height * scale,
                    ],
                    clip: None,
                    work_area: g.work_area.map(|p| p.map(|v| v * scale)),
                    outline_linear: [fg[0], fg[1], fg[2]],
                    background_linear: [bg[0], bg[1], bg[2]],
                    scale,
                    opacity: 1.,
                }],
            );
            presenter.present_overviews(
                &gpu.renderer,
                &target.texture.create_view(&Default::default()),
                [width, height],
            );
            gpu.renderer.queue().present(target);
        }
        Ok(retry)
    }
}
