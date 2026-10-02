use crate::{Document, LayerId, Rect};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContentScope {
    Canvas,
    Visible,
    All,
    Target(LayerId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContentBoundsRequest {
    pub document: Arc<Document>,
    pub scope: ContentScope,
    pub time: f32,
    pub effect_times: Vec<(LayerId, f32)>,
}
impl ContentBoundsRequest {
    pub fn new(document: &Document, scope: ContentScope) -> Self {
        let mut document = document.clone();
        document.layers = if let ContentScope::Target(id) = scope
            && let Some(owner) = document.target_owner(id)
        {
            let mut layer = owner.composite_snapshot();
            let offset = document.layer_offset(layer.id);
            if let Some(mask) = &mut layer.mask {
                mask.offset.x += offset.x - layer.properties.offset.x;
                mask.offset.y += offset.y - layer.properties.offset.y;
            }
            layer.properties.offset = offset;
            layer.properties.parent = None;
            document.active_layer = layer.id;
            document.active_mask = id != layer.id;
            vec![layer]
        } else {
            document.layers.iter().map(crate::Layer::composite_snapshot).collect()
        };
        document.reference_layers.clear();
        Self { document: Arc::new(document), scope, time: 0., effect_times: Vec::new() }
    }
    pub fn known_bounds(&self) -> Option<Rect> {
        let ContentScope::Target(id) = self.scope else { return None; };
        if self.document.selection.is_some() { return None; }
        let layer = self.document.layer(id)?;
        let source = layer.source.as_ref()?;
        (!source.interpretation.channels.has_alpha() && layer.raster.is_empty())
            .then(|| Rect::from_extent(source.extent))
    }
}

#[derive(Default)]
pub struct ContentBoundsCache {
    entries: Vec<(ContentBoundsRequest, Rect)>,
}
impl ContentBoundsCache {
    pub fn get(&self, request: &ContentBoundsRequest) -> Option<Rect> {
        self.entries.iter().find(|(key, _)| key == request).map(|(_, bounds)| *bounds)
    }
    pub fn insert(&mut self, request: ContentBoundsRequest, bounds: Rect) {
        self.entries.retain(|(key, _)| key.document.id == request.document.id
            && key.document.revision == request.document.revision && key.scope != request.scope);
        self.entries.push((request, bounds));
    }
    pub fn discard_changed(&mut self, document: &Document) {
        self.entries.retain(|(key, _)| key.document.revision == document.revision && key.document.id == document.id);
    }
}
