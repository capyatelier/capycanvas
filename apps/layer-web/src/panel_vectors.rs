use crate::{js, serialize};
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WebPanelVectors(layer_ui::PanelVectorCache);

#[derive(Serialize)]
struct Source {
    id: u32,
    bounds: layer_ui::Bounds,
    reverse: bool,
    commands: Vec<[i32;7]>,
}

#[derive(Serialize)]
struct Projection {
    sources: Vec<Source>,
    surfaces: Vec<layer_ui::PanelVectorSurface>,
}

#[wasm_bindgen]
impl WebPanelVectors {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let _=layer_render::panel_geometry::panel_profiles();
        Self(Default::default())
    }

    pub fn resolve(&mut self,layout: JsValue) -> Result<JsValue,JsValue> {
        let mut layout: layer_ui::ResolvedLayout=serde_wasm_bindgen::from_value(layout).map_err(js)?;
        self.0.resolve(&mut layout,0.);
        let sources=layout.collapsed.iter().filter(|c|layout.vector_surfaces.iter().any(|s|s.column==c.id)).map(|c| {
            let mut commands=Vec::new();
            layer_render::panel_geometry::panel_outline([0.,0.,c.bounds.width,c.bounds.height],c.corner_radii,&mut commands);
            Source {id:c.id,bounds:c.bounds,reverse:c.open.as_ref().is_some_and(|o|o.direction==layer_ui::Edge::Left),
                commands:commands.into_iter().map(|c|std::array::from_fn(|i|
                    if i==0 {c[i] as i32} else {(c[i]*1024.).round() as i32})).collect()}
        }).collect();
        serialize(&Projection {sources,surfaces:layout.vector_surfaces})
    }
}
