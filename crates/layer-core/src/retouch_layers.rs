//! New Dodge & Burn Layer and Frequency Separation: retouching layers that
//! are inserted with their pixels in one undo step, through pending
//! operations on the new layers.
use super::*;

pub const SEPARATION_IDS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetouchLayerRefusal {
    NoLayer,
    NotPaint,
    Hidden,
    NotNormal,
    Linear,
    GroupLocked,
    TooLarge,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SeparationFilters {
    pub blur: Arc<EffectInstance>,
}
impl SeparationFilters {
    pub const BLUR: &'static str = "gaussian_blur";
    const RADIUS: &'static str = "sigma";

    pub fn new(catalog: &EffectCatalog, radius: f32) -> Result<Self, &'static str> {
        let instance = |id: &str| catalog.get(id).map(|f| EffectInstance::new(f.program())).ok_or("Frequency Separation's filters are missing");
        let mut blur = instance(Self::BLUR)?;
        blur.set(Self::RADIUS, EffectValue::Number(radius))?;
        Ok(Self { blur: Arc::new(blur) })
    }

    /// The blur's radius parameter, which bounds the radius a dialog offers.
    pub fn radius_parameter(catalog: &EffectCatalog) -> Option<&EffectParameter> {
        catalog.get(Self::BLUR)?.program.parameters.iter().find(|p| p.key.as_ref() == Self::RADIUS)
    }

    /// An effect layer that applies `effect` to the layer it is clipped to.
    pub fn clipped(id: LayerId, effect: &Arc<EffectInstance>, name: impl Into<Arc<str>>) -> Layer {
        let mut layer = Layer::paint(id, name);
        layer.kind = LayerKind::Effect;
        layer.effect = Some(effect.clone());
        layer.properties.clipped = true;
        layer
    }
}

/// Edits that insert the new layers, and the pending operations that give
/// them their pixels. `CanvasEngine::insert_with_operations` runs both as one
/// step.
#[derive(Clone, Debug)]
pub struct RetouchLayerPlan {
    pub edits: Vec<Edit>,
    pub operations: Vec<(LayerId, LayerOperation)>,
    /// The layer that becomes active.
    pub active: LayerId,
}

impl Document {
    /// The gray a Soft Light layer leaves the image under it unchanged with:
    /// in Perceptual documents the linear value that encodes to 8-bit 128 or
    /// 16-bit 32768, and in Linear-light ones linear 0.5.
    pub fn soft_light_neutral(&self) -> f32 {
        let code = |maximum: f64| self.color.space.decode((maximum + 1.) / 2. / maximum) as f32;
        match (self.blend_space, self.color.depth) {
            (BlendSpace::Perceptual, color::SampleDepth::U8) => code(255.),
            (BlendSpace::Perceptual, color::SampleDepth::U16) => code(65535.),
            _ => 0.5,
        }
    }

    /// Where a layer goes to sit above `id` and everything clipped to it, in
    /// `id`'s group; on an empty stack, the top.
    pub fn above_clipping_stack(&self, id: LayerId) -> (usize, Option<LayerId>) {
        let parent = self.layer(id).and_then(|l| l.properties.parent);
        let index = self
            .clipping_stack_top(id)
            .and_then(|top| self.layers.iter().position(|l| l.id == top))
            .unwrap_or(0);
        (index, parent)
    }

    /// Why New Dodge & Burn Layer can't insert above the active layer.
    pub fn dodge_burn_refusal(&self) -> Option<RetouchLayerRefusal> {
        let (_, parent) = self.above_clipping_stack(self.active_layer);
        parent.is_some_and(|p| self.is_locked(p)).then_some(RetouchLayerRefusal::GroupLocked)
    }

    /// A Soft Light layer filled with `soft_light_neutral`, above the active
    /// layer and its clipping stack, made active.
    pub fn dodge_burn_plan(&self, [id, coverage]: [LayerId; 2], name: impl Into<Arc<str>>) -> Result<RetouchLayerPlan, RetouchLayerRefusal> {
        if let Some(refusal) = self.dodge_burn_refusal() {
            return Err(refusal);
        }
        let (index, parent) = self.above_clipping_stack(self.active_layer);
        let mut layer = Layer::paint(id, name);
        layer.properties.parent = parent;
        layer.properties.blend = LayerBlend::SoftLight;
        let gray = self.soft_light_neutral();
        let fill = LayerOperation {
            placement: Affine::IDENTITY,
            coverage: LayerMask::reveal_all(coverage, Point::default()),
            kind: LayerOperationKind::Fill { color: [gray, gray, gray, 1.], alpha_locked: false },
        };
        Ok(RetouchLayerPlan {
            edits: vec![Edit::InsertLayer { index, layer }, Edit::SetActiveLayer { id }],
            operations: vec![(id, fill)],
            active: id,
        })
    }

    /// Why Frequency Separation can't split `target`. The size limit is
    /// checked by `separation_plan` only.
    pub fn separation_refusal(&self, target: LayerId) -> Option<RetouchLayerRefusal> {
        use RetouchLayerRefusal as R;
        if self.blend_space != BlendSpace::Perceptual {
            return Some(R::Linear);
        }
        let Some(layer) = self.layer(target) else {
            return Some(R::NoLayer);
        };
        if layer.kind != LayerKind::Paint {
            Some(R::NotPaint)
        } else if !layer.visible {
            Some(R::Hidden)
        } else if layer.properties.blend != LayerBlend::Normal {
            Some(R::NotNormal)
        } else if layer.properties.parent.is_some_and(|p| self.is_locked(p)) {
            Some(R::GroupLocked)
        } else {
            None
        }
    }

    /// Split `target` into Low, its blur, and above it High, its detail
    /// against that blur in Linear Light, in an isolated Normal group that
    /// takes the layer's place and opacity. The layer stays, hidden, below the
    /// group. Both are baked from the layer's pixels and mask over the canvas.
    pub fn separation_plan(
        &self,
        target: LayerId,
        filters: &SeparationFilters,
        ids: [LayerId; SEPARATION_IDS],
        [group_name, low_name, high_name]: [Arc<str>; 3],
    ) -> Result<RetouchLayerPlan, RetouchLayerRefusal> {
        if let Some(refusal) = self.separation_refusal(target) {
            return Err(refusal);
        }
        let [group_id, low, high, blur, low_coverage, high_coverage] = ids;
        let index = self.layers.iter().position(|l| l.id == target).ok_or(RetouchLayerRefusal::NoLayer)?;
        let layer = &self.layers[index];
        let parent_offset = layer.properties.parent.map_or(Point::default(), |p| self.layer_offset(p));
        let mut source = merge::bake_member(layer);
        source.opacity = 1.;
        source.properties.parent = None;
        source.properties.clipped = false;
        let canvas = [self.width, self.height];
        let operation = |kind, coverage: LayerId| {
            let operation = LayerOperation {
                placement: Affine::IDENTITY,
                coverage: LayerMask::reveal_all(coverage, Point::default()),
                kind,
            };
            if self.exceeds_publication(&operation, canvas) {
                return Err(RetouchLayerRefusal::TooLarge);
            }
            Ok(operation)
        };
        let operations = vec![
            (low, operation(LayerOperationKind::Bake {
                members: [SeparationFilters::clipped(blur, &filters.blur, filters.blur.program.id.clone()), source.clone()].into(), offset: parent_offset,
            }, low_coverage)?),
            (high, operation(LayerOperationKind::FrequencyDetail {
                members: [source].into(), offset: parent_offset, low,
            }, high_coverage)?),
        ];
        let mut group = Layer::paint(group_id, group_name);
        group.kind = LayerKind::Group;
        group.opacity = layer.opacity;
        group.properties.parent = layer.properties.parent;
        group.properties.clipped = layer.properties.clipped;
        let part = |id, name: Arc<str>, blend| {
            let mut part = Layer::paint(id, name);
            part.properties.parent = Some(group_id);
            part.properties.offset = Point { x: -parent_offset.x, y: -parent_offset.y };
            part.properties.blend = blend;
            part
        };
        Ok(RetouchLayerPlan {
            edits: vec![
                Edit::InsertLayer { index, layer: group },
                Edit::InsertLayer { index: index + 1, layer: part(high, high_name, LayerBlend::LinearLight) },
                Edit::InsertLayer { index: index + 2, layer: part(low, low_name, LayerBlend::Normal) },
                Edit::SetLayerVisibility { id: target, visible: false },
                Edit::SetActiveLayer { id: high },
            ],
            operations,
            active: high,
        })
    }
}

#[cfg(test)]
#[path = "retouch_layers_tests.rs"]
mod tests;
