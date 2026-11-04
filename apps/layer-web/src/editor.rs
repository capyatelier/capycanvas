//! Native DOM geometry and shared editor models. Painting remains in the GPU surface.
use super::*;
use serde_json::json;

// Each DOM Navigator owns a GPU surface at native resolution. The browser
// retains its pixels while workspace transforms move the enclosing controls.
pub(super) struct NavigatorSurface {
    canvas: web_sys::HtmlCanvasElement,
    size: [f32; 2],
    scale: f32,
    capacity: [u32; 2],
    pub(super) gpu: Option<(
        wgpu::Surface<'static>,
        ViewportPresenter,
        wgpu::SurfaceConfiguration,
        layer_core::color::RgbSpace,
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
    color: Option<DrawerColorMeasure>,
    menu: Option<MenuMeasurement>,
}
#[derive(Deserialize)]
struct MenuMeasurement { width: f32, height: f32, row_height: f32 }
#[derive(Deserialize)]
struct DrawerColorMeasure { fixed_height: f32, hdr: bool }


#[derive(Deserialize)]
struct LayerDrawerQuery {
    measurement: layer_ui::PanelScrollMeasurement,
    properties_height: f32,
    viewport: [f32; 2],
    column: Option<u32>,
}

#[wasm_bindgen]
pub struct WebMenuContact { contact: layer_ui::PhoneMenuContact }
#[wasm_bindgen]
pub struct WebBarContact { contact: layer_ui::PhoneBarContact }
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BarInput {
    Arm,
    Motion { position: [f32; 2], past_slop: bool },
    Finish { position: [f32; 2] },
}
#[derive(Serialize)]
struct BarReply<'a> {
    outcome: layer_ui::PhoneBarOutcome,
    geometry: &'a [layer_ui::Bounds],
    lifted: Option<usize>,
}

#[wasm_bindgen]
impl WebApp {
    pub fn preferences_layout(&self, viewport: JsValue, width: f32, minimum: JsValue) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().preferences_layout(serde_wasm_bindgen::from_value(viewport).map_err(js)?, width,
            serde_wasm_bindgen::from_value(minimum).map_err(js)?))
    }
    pub fn new_drawing_layout(&self, width: f32) -> Result<JsValue, JsValue> { serialize(&layer_ui::new_drawing_layout(width)) }
    pub fn dialog_panes_fit(&self, width: f32, minimum: JsValue, gap: f32) -> Result<bool, JsValue> { Ok(layer_ui::dialog_panes_fit(width,serde_wasm_bindgen::from_value(minimum).map_err(js)?,gap)) }
    pub fn preferences_content_width(&self, width: f32, minimum: f32) -> f32 { layer_ui::preferences_content_width(width, minimum) }
    pub fn dialog_header(&self, intent: JsValue, subpage: bool) -> Result<JsValue, JsValue> {
        let intent: layer_ui::DialogIntent = serde_wasm_bindgen::from_value(intent).map_err(js)?;
        serialize(&intent.header(subpage))
    }
    pub fn dialog_escape(&self, subpage: bool, search_focused: bool, search_has_text: bool) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::dialog_escape(subpage, search_focused, search_has_text))
    }
    pub fn dialog_presentation(&self, role: JsValue, width: f32, height: f32, visible: JsValue) -> Result<JsValue, JsValue> {
        let role = serde_wasm_bindgen::from_value(role).map_err(js)?;
        let visible = serde_wasm_bindgen::from_value(visible).map_err(js)?;
        serialize(&self.session.state().dialog_presentation_in(role, [width, height], visible))
    }
    pub fn dialog_bounds(&self, role: JsValue, viewport: JsValue, visible: JsValue, measured: JsValue) -> Result<JsValue, JsValue> {
        let role=serde_wasm_bindgen::from_value(role).map_err(js)?;
        let viewport=serde_wasm_bindgen::from_value(viewport).map_err(js)?;
        let visible=serde_wasm_bindgen::from_value(visible).map_err(js)?;
        let measured=serde_wasm_bindgen::from_value(measured).map_err(js)?;
        serialize(&self.session.state().dialog_presentation_in(role,viewport,visible).bounds(|| measured))
    }
    pub fn calibration_extent(&self, area: JsValue, natural: JsValue) -> Result<JsValue, JsValue> {
        serialize(&layer_ui::calibration_extent(serde_wasm_bindgen::from_value(area).map_err(js)?,serde_wasm_bindgen::from_value(natural).map_err(js)?))
    }
    pub fn notice_above(&self, notice: JsValue, bar: JsValue) -> Result<JsValue, JsValue> {
        let notice:layer_ui::NoticePlacement=serde_wasm_bindgen::from_value(notice).map_err(js)?;
        serialize(&notice.above(serde_wasm_bindgen::from_value(bar).map_err(js)?))
    }
    pub fn touch_hold_ms(&self) -> u32 { self.session.touch_policy().tap_ms }
    pub fn finger_loupe_offset(&self, scale: f32) -> f32 { layer_ui::finger_loupe_offset(None, scale) }
    pub fn phone_bar_contact(&self, x: f32, y: f32) -> Option<WebBarContact> {
        let mut contact = layer_ui::PhoneBarContact::begin(&self.session.phone_layout()?, [x, y], false)?;
        contact.set_tools(self.session.state().workspace.layout.phone.as_deref().unwrap_or_default());
        Some(WebBarContact { contact })
    }
    pub fn phone_bar_valid(&self, contact: &WebBarContact) -> bool {
        self.session.phone_layout().is_some_and(|layout| contact.contact.valid(&layout,
            self.session.state().workspace.layout.phone.iter().flatten()))
    }
    pub fn phone_bar_input(&self, contact: &mut WebBarContact, input: JsValue) -> Result<JsValue, JsValue> {
        let input: BarInput = serde_wasm_bindgen::from_value(input).map_err(js)?;
        let tiles = self.session.state().workspace.layout.phone.as_deref().unwrap_or_default();
        let Some(layout) = self.session.phone_layout().filter(|layout| contact.contact.valid(layout, tiles)) else {
            return serialize(&BarReply { outcome: contact.contact.cancel(), geometry: &[], lifted: None });
        };
        let outcome = match input {
            BarInput::Arm => contact.contact.arm(),
            BarInput::Motion { position, past_slop } => contact.contact.motion(&layout, tiles, position, past_slop),
            BarInput::Finish { position } => contact.contact.finish(&layout, tiles, position),
        };
        let geometry = contact.contact.geometry(&layout);
        serialize(&BarReply { outcome, geometry: &geometry[..tiles.len().min(geometry.len())], lifted: contact.contact.lifted() })
    }
    pub fn command_search_placement(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        let viewport = [width, height];
        let visible = layer_ui::Bounds { x: 0., y: 0., width, height };
        let phone = self.session.phone_layout();
        let area = phone.as_ref().and_then(|phone| phone.work_area.intersection(visible)).unwrap_or(visible);
        serialize(&layer_ui::COMMAND_SEARCH_STYLE.placement(viewport, area, phone.is_some()))
    }
    pub fn phone_menu(&self) -> Result<JsValue, JsValue> { serialize(&self.phone_menu_view()) }
    pub fn phone_menu_contact(&self) -> WebMenuContact { WebMenuContact { contact: Default::default() } }
    pub fn phone_menu_input(&self, contact: &mut WebMenuContact, input: JsValue) -> Result<JsValue, JsValue> {
        let navigation = &self.session.state().phone.menu_navigation;
        let input = serde_wasm_bindgen::from_value(input).map_err(js)?;
        serialize(&contact.contact.input(&navigation.current(), !navigation.is_root(), input))
    }
    pub fn phone_menu_width(&self, minimum: f32, natural: f32, row_height: f32) -> Option<f32> {
        self.session.phone_layout().map(|phone| phone.menu_width(minimum, natural, row_height))
    }
    pub fn phone_menu_geometry(&self, placement: JsValue, row_height: f32) -> Result<JsValue, JsValue> {
        let placement = serde_wasm_bindgen::from_value(placement).map_err(js)?;
        serialize(&self.session.phone_layout().map(|phone| phone.menu_geometry(&placement, row_height)))
    }
    pub fn application_menus(&self) -> Result<JsValue, JsValue> {
        js_sys::JSON::parse(&serde_json::to_string(&self.application_menu_models()).map_err(js)?)
    }
    pub fn editor_models(&self, _width: f32, _height: f32) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let document = layer_ui::new_document_spec(self.session.localization());
        js_sys::JSON::parse(&serde_json::to_string(&json!({
            "color_panel": state.preview_colors().view_mapped(self.session.effective_sdr_rendition(), self.session.localization()),
            "application_menus": self.application_menu_models(),
            "document_options": json!({
                "dialog": document.dialog, "unsaved_description": document.unsaved_description,
                "discard_label": document.discard,
                "cancel_label": document.cancel,
            }),
        })).map_err(js)?)
    }
    pub fn color_panel(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().preview_colors().view_mapped(self.session.effective_sdr_rendition(), self.session.localization()))
    }
    pub fn paint_pair(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.state().display_colors().paint_pair(layer_core::color::RgbSpace::Srgb, Some(self.session.effective_sdr_rendition())))
    }
    pub fn palette_panel(&self, query: Option<String>) -> Result<JsValue, JsValue> {
        let state = self.session.state();
        let colors = state.display_colors();
        let rendition = self.session.effective_sdr_rendition();
        serialize(&layer_ui::PalettePanelView::new(
            colors,
            &state.color_library,
            query.as_deref().unwrap_or_default(),
            |color| colors.mapped_swatch(color, rendition),
            self.session.localization(),
        ))
    }
    pub fn swatch_sheet(&self, query: String, current: JsValue) -> Result<JsValue, JsValue> {
        let current = serde_wasm_bindgen::from_value(current).map_err(js)?;
        let state = self.session.state();
        let colors = state.display_colors();
        let rendition = self.session.effective_sdr_rendition();
        serialize(&layer_ui::SwatchSheetView::new(&state.color_library, &query, current, |color| colors.mapped_swatch(color, rendition), self.session.localization()))
    }
    pub fn paper_preview(&self, id: String) -> Option<Vec<u8>> {
        self.session.paper_preview(&id).map(|preview| preview.to_vec())
    }
    /// Small cached UI raster only; the painting canvas remains on WebGPU.
    pub fn color_field_pixels(&self, side: u32) -> Result<Vec<u8>, JsValue> {
        color_field(&self.session.state().preview_colors(), side,
            (self.session.state().layer_tools.mask_editing.is_none() && self.session.engine().document().composition().color.depth.is_float()).then(|| self.session.effective_sdr_rendition()))
    }
    pub fn color_preview(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.color_preview())
    }
    pub fn color_field_request(&self, side: u32) -> Result<String, JsValue> {
        serde_json::to_string(&json!({"side":side,"colors":self.session.state().preview_colors(),
            "rendition":(self.session.state().layer_tools.mask_editing.is_none() && self.session.engine().document().composition().color.depth.is_float()).then(|| self.session.effective_sdr_rendition())})).map_err(js)
    }

    pub fn workspace_persistence(&self) -> Result<JsValue, JsValue> {
        serialize(&self.session.durable_workspace())
    }
    pub fn navigator_geometry(&self, width: f32, height: f32) -> Result<JsValue, JsValue> {
        let document = self.session.engine().document();
        serialize(&layer_ui::NavigatorGeometry::new(
            &self.session.state().camera,
            document.composition().size,
            [width, height],
        ))
    }
    pub fn navigator_aspect(&self) -> f32 {
        let document = self.session.engine().document();
        layer_ui::NavigatorGeometry::overview_aspect(document.composition().size)
    }
    pub fn navigator_surface(&mut self, id: u32, canvas: web_sys::HtmlCanvasElement) {
        self.overviews.insert(
            id,
            NavigatorSurface {
                canvas,
                size: [0.; 2],
                scale: 1.,
                capacity: [0; 2],
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
    ) -> Result<Vec<u32>, JsValue> {
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
            // Grow in small native-pixel blocks and retain the allocation while
            // shrinking. CSS clips a 1:1 canvas; it never scales a stale image.
            for (capacity, size) in slot.capacity.iter_mut().zip(slot.size) {
                let needed = (size * scale).round().max(1.) as u32;
                *capacity = (*capacity).max(needed.div_ceil(64).saturating_mul(64));
            }
            return Ok(slot.capacity.to_vec());
        }
        Ok(Vec::new())
    }
    /// Live DOM reflow must resize and redraw the backing canvas before the
    /// browser presents that layout. Reuse the existing GPU document image.
    pub fn reflow_navigators(&mut self) -> Result<bool, JsValue> {
        self.present_navigators()
    }
    pub fn set_glass(&mut self, boxes: &[f32], connections: JsValue, bubbles: JsValue, vectors: &[f32]) -> Result<(), JsValue> {
        let connections: Vec<layer_ui::DrawerConnection> = serde_wasm_bindgen::from_value(connections).map_err(js)?;
        let scale = self.viewport_scale;
        use layer_render_wgpu::BackdropRegion;
        self.glass = boxes
            .chunks_exact(9)
            .map(|b| BackdropRegion::rounded(
                [b[0], b[1], b[2], b[3]].map(|v| v * scale),
                [b[4], b[5], b[6], b[7]].map(|v| v * scale),
                if b[8] != 0. { BackdropRegion::SQUIRCLE } else { BackdropRegion::CIRCULAR },
            ))
            .chain(connections.iter().flat_map(|c| c.glass()).map(|(bounds, radii)| BackdropRegion {
                bounds: bounds.map(|v| v * scale),
                radii: radii.map(|v| v * scale),
                shape: BackdropRegion::SQUIRCLE,
            }))
            .collect();
        self.glass.extend(vectors.chunks_exact(7).filter(|t|t.iter().all(|v|v.is_finite()) && (0. ..=7.).contains(&t[6])).map(|t|
            BackdropRegion::triangle(std::array::from_fn(|i|t[i]*scale),t[6] as u32)));
        if !bubbles.is_null() && !bubbles.is_undefined() {
            #[derive(Deserialize)]
            struct Regions { boxes: Vec<[f32; 10]>, arcs: Vec<[f32; 7]>, colors: Vec<[f32; 7]> }
            let regions: Regions = serde_wasm_bindgen::from_value(bubbles).map_err(js)?;
            self.glass.extend(regions.boxes.into_iter().map(|b| BackdropRegion::rounded(
                [b[0], b[1], b[2], b[3]].map(|v| v * scale),
                [b[4], b[5], b[6], b[7]].map(|v| v * scale),
                if b[8] != 0. { BackdropRegion::SQUIRCLE } else { BackdropRegion::CIRCULAR },
            ).with_strength(b[9])));
            self.glass.extend(regions.colors.into_iter().map(|c| BackdropRegion::color_bubble(
                [c[0] * scale, c[1] * scale], c[2] * scale, c[3] * scale, c[4] * scale, c[5],
            ).with_strength(c[6])));
            self.glass.extend(regions.arcs.into_iter().map(|a| BackdropRegion::arc(
                [a[0] * scale, a[1] * scale], a[2] * scale, a[3] * scale, a[4], a[5],
            ).with_strength(a[6])));
        }
        Ok(())
    }
    pub fn tool_panel(&self, panel: JsValue, anchor: JsValue) -> Result<JsValue, JsValue> {
        let panel = serde_wasm_bindgen::from_value(panel).map_err(js)?;
        let anchor: Option<layer_ui::DrawerAnchor> = serde_wasm_bindgen::from_value(anchor).map_err(js)?;
        let state = self.session.state();
        let drawer = anchor.and_then(|anchor| state.customization.drawer.iter().chain(&state.customization.column_drawers)
            .find(|drawer| drawer.anchor == anchor));
        serialize(&state.tool_panel_presentation(panel, drawer))
    }
    pub fn drawer(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: DrawerQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let state = self.session.state();
        let drawer = match q.column {
            None => state.customization.drawer.as_ref(),
            Some(id) => state.customization.column_drawers.iter().find(|d|
                matches!(d.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
        };
        let mut heights = q.heights;
        let color_fit = drawer.and_then(|drawer| q.color.as_ref().and_then(|color|
            state.customization.drawer_measure(drawer, &self.session.layout_resolve_input(), q.viewport)
                .and_then(|measure| measure.color_fit(&heights, color.fixed_height, color.hdr))));
        if let (Some(drawer), Some(fit), Some(color)) = (drawer, color_fit, &q.color) {
            if let Some(column) = drawer.columns.iter().position(|panels| panels.contains(&layer_ui::Panel::Color)) {
                heights[column] = fit.height.ceil() + color.fixed_height;
            }
        }
        let end = if q.closing {
            q.from.as_ref().map(|p| p.closed())
        } else if q.column.is_none() && state.phone.menu {
            q.menu.as_ref().and_then(|menu| self.session.phone_layout()?.menu(q.viewport, menu.width, menu.height, menu.row_height))
        } else {
            drawer.and_then(|d| {
                state.customization.drawer_placement(
                    d,
                    &self.session.layout_resolve_input(),
                    q.viewport,
                    &heights,
                )
            })
        };
        #[derive(Serialize)]
        struct Frame {connection:Option<layer_ui::DrawerConnection>,placement:layer_ui::DrawerPlacement,color_fit:Option<layer_ui::ColorPanelFit>}
        serialize(&end.map(|end| {
            let from = q.from.unwrap_or_else(|| end.closed());
            let placement = end.interpolate_from(&from, q.progress);
            Frame {connection:placement.connection(),placement,color_fit}
        }))
    }
    pub fn layer_drawer_sizing(&self, query: JsValue) -> Result<JsValue, JsValue> {
        let q: LayerDrawerQuery = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let state = self.session.state();
        let drawer = match q.column {
            None => state.customization.drawer.as_ref(),
            Some(id) => state.customization.column_drawers.iter().find(|drawer|
                matches!(drawer.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
        };
        serialize(&state.customization.layer_drawer_sizing(drawer, &self.session.layout_resolve_input(),
            q.viewport, q.measurement, state.layer_tools.presented_rows(&state.layers), q.properties_height).map_err(js)?)
    }
    pub fn drawer_measure(&self, query: JsValue) -> Result<JsValue, JsValue> {
        #[derive(Deserialize)]
        struct Query {
            viewport: [f32; 2],
            column: Option<u32>,
            tiles: Vec<layer_ui::DrawerTileMeasurement>,
        }
        let q: Query = serde_wasm_bindgen::from_value(query).map_err(js)?;
        let state = self.session.state();
        let drawer = match q.column {
            None => state.customization.drawer.as_ref(),
            Some(id) => state.customization.column_drawers.iter().find(|d|
                matches!(d.anchor, layer_ui::DrawerAnchor::Column { column, .. } if column == id)),
        };
        let value = match drawer {
            Some(drawer) => self.session.layout_resolve_input().drawer_geometry(q.viewport, &json!({
                "model":drawer,"column_drawers":state.customization.column_drawers,
                "tiles":q.tiles,"toolbars":true,
            }).to_string()).map_err(js)?,
            None => json!({"measure":null,"toolbars":[],"minimums":[]}),
        };
        let mut value = value;
        value["scrolls_together"] = json!(drawer.is_some_and(layer_ui::ContentDrawer::scrolls_together));
        value["minimums"] = drawer.map(|drawer| json!(drawer.columns.iter().map(|panels| panels.iter()
            .map(|panel| drawer.body_minimum_height(*panel)).collect::<Vec<_>>()).collect::<Vec<_>>())).unwrap_or_else(|| json!([]));
        serialize(&value)
    }
    pub fn application_link(&self, link: JsValue) -> Result<String, JsValue> {
        let link: layer_ui::ApplicationLink = serde_wasm_bindgen::from_value(link).map_err(js)?;
        Ok(link.url().into())
    }
    pub fn application_menu_hover(&self, open: JsValue, hovered: JsValue) -> Result<bool, JsValue> {
        let open = serde_wasm_bindgen::from_value(open).map_err(js)?;
        let hovered = serde_wasm_bindgen::from_value(hovered).map_err(js)?;
        Ok(layer_ui::ApplicationMenu::switches_on_hover(open, hovered))
    }
}

impl WebApp {
    fn application_menu_models(&self) -> serde_json::Value { json!(layer_ui::ApplicationMenu::ALL.map(|menu| json!({"id":menu, "label":menu.localized_label(self.session.localization()), "model":self.session.application_menu(menu)}))) }
    pub(super) fn present_navigators(&mut self) -> Result<bool, JsValue> {
        // DOM reflow can run while an attached renderer is suspended/replaced.
        if !self.gpu_ready() || !self.startup.canvas_ready {
            return Ok(false);
        }
        let state = self.session.state();
        let camera = state.camera.clone();
        let fg = state.palette.text.linear();
        let document = self.session.engine().document();
        let extent = document.composition().size;
        let (Some(gpu), Some(window)) = (self.session.engine().backend().0.as_deref(), self.surface.as_ref()) else {
            return Ok(false);
        };
        let space = gpu.document_color().space;
        let mut retry = false;
        for slot in self.overviews.values_mut() {
            let Some(g) = layer_ui::NavigatorGeometry::new(&camera, extent, slot.size) else {
                continue;
            };
            let scale = slot.scale;
            let [width, height] = slot.capacity;
            if slot.gpu.is_none() {
                let surface = window
                    .instance
                    .create_surface(wgpu::SurfaceTarget::Canvas(slot.canvas.clone()))
                    .map_err(js)?;
                let mut config = window.config.clone();
                config.width = width;
                config.height = height;
                config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
                slot.canvas.set_width(width);
                slot.canvas.set_height(height);
                surface.configure(gpu.device(), &config);
                let presenter = ViewportPresenter::for_overview_surface(gpu, config.format, window.color).map_err(js)?;
                slot.gpu = Some((surface, presenter, config, space));
            }
            let (surface, presenter, config, presented_space) = slot.gpu.as_mut().unwrap();
            // Retained DOM canvases can outlive document/color adoption. Their
            // display transform must follow the new renderer, including undo.
            let output_changed = config.format != window.config.format || config.color_space != window.config.color_space;
            if *presented_space != space || output_changed {
                let held=presenter.capture_held(gpu.device(),gpu.queue());
                config.format = window.config.format;
                config.color_space = window.config.color_space;
                *presenter = ViewportPresenter::for_overview_surface(gpu, config.format, window.color).map_err(js)?;
                presenter.adopt_held(held);
                *presented_space = space;
            }
            presenter.inherit_proof(gpu, &window.presenter);
            if output_changed || config.width != width || config.height != height {
                config.width = width;
                config.height = height;
                slot.canvas.set_width(width);
                slot.canvas.set_height(height);
                surface.configure(gpu.device(), config);
                presenter.set_target_retention(false);
            }
            presenter.set_overviews(
                gpu,
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
                    scale,
                    opacity: 1.,
                }],
            );
            if !presenter.pipelines_ready(gpu) { retry = true; continue; }
            if !presenter.needs_overview_present(gpu,[width,height]) { continue; }
            let target = match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(target)
                | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
                wgpu::CurrentSurfaceTexture::Lost => {
                    slot.gpu = None;
                    retry = true;
                    continue;
                }
                wgpu::CurrentSurfaceTexture::Outdated => {
                    surface.configure(gpu.device(), config);
                    presenter.set_target_retention(false);
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
            presenter.present_overviews(
                gpu,
                &target.texture.create_view(&Default::default()),
                [width, height],
            ).map_err(js)?;
            gpu.queue().present(target);
        }
        Ok(retry)
    }
}

fn color_field(colors: &layer_ui::ColorState, side: u32, rendition: Option<layer_core::color::hdr::SdrRendition>) -> Result<Vec<u8>, JsValue> {
    if !(1..=2048).contains(&side) { return Err(js("Invalid color field size")); }
    let mut pixels = vec![0; side as usize * side as usize * 4];
    let valid = match rendition {
        Some(recipe) => colors.render_field_mapped(side, recipe, &mut pixels),
        None => colors.render_field(side, &mut pixels),
    };
    if !valid { return Err(js("Color field does not use a raster")); }
    Ok(pixels)
}
#[wasm_bindgen]
pub fn raster_worker_color_field(metadata: &str) -> Result<Vec<u8>, JsValue> {
    #[derive(Deserialize)]
    struct Request { colors: layer_ui::ColorState, side: u32, rendition: Option<layer_core::color::hdr::SdrRendition> }
    let request: Request = serde_json::from_str(metadata).map_err(js)?;
    color_field(&request.colors, request.side, request.rendition)
}

impl WebApp {
    fn phone_menu_view(&self) -> Option<layer_ui::PhoneMenuView> {
        if !self.session.state().phone.menu { return None; }
        let labels = self.documents.labels(&self.session.state().document_file, |session| &session.state().document_file, self.session.localization());
        let selector = layer_ui::DrawingSelector::new(&labels, self.documents.selected(), &self.session.command(layer_ui::CommandId::NewDocument), self.session.localization());
        Some(self.session.phone_menu()?.view(&selector, layer_ui::Platform::Web, self.session.localization()))
    }
}
