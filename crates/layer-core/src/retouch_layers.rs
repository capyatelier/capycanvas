use super::*;
use crate::authored::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetouchLayerRefusal {
    NoLayer,
    NotPaint,
    Objects,
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
        let instance =
            |id: &str| catalog.get(id).map(|f| EffectInstance::new(f.program())).ok_or("Frequency Separation's filters are missing");
        let mut blur = instance(Self::BLUR)?;
        blur.set(Self::RADIUS, EffectValue::Number(radius))?;
        Ok(Self { blur: Arc::new(blur) })
    }

    /// The blur's radius parameter, which bounds the radius a dialog offers.
    pub fn radius_parameter(catalog: &EffectCatalog) -> Option<&EffectParameter> {
        catalog.get(Self::BLUR)?.program.parameters.iter().find(|p| p.key.as_ref() == Self::RADIUS)
    }
}
#[derive(Clone, Debug)]
pub struct RetouchLayerPlan {
    pub edits: Vec<Edit>,
    pub operations: Vec<(SourceTarget, RasterOperation)>,
    pub active: OccurrenceHandle,
}
impl Document {
    pub fn soft_light_neutral(&self) -> f32 {
        let color = self.composition().color;
        let code = |maximum: f64| color.space.decode((maximum + 1.) / 2. / maximum) as f32;
        match (self.composition().blend, color.depth) {
            (BlendSpace::Perceptual, color::SampleDepth::U8) => code(255.),
            (BlendSpace::Perceptual, color::SampleDepth::U16) => code(65535.),
            _ => 0.5,
        }
    }
    pub fn above_clipping_stack(&self, id: OccurrenceHandle) -> (usize, Option<OccurrenceHandle>) {
        let scene = self.scene();
        let parent = scene.parent(id);
        let top = self.clipping_stack_top(id).unwrap_or(id);
        let index = scene.children(parent).iter().position(|h| *h == top).unwrap_or(0);
        (index, parent)
    }
    pub fn dodge_burn_refusal(&self) -> Option<RetouchLayerRefusal> {
        let parent = self.working.occurrence.and_then(|h| self.scene().parent(h));
        parent.is_some_and(|p| self.is_locked(p)).then_some(RetouchLayerRefusal::GroupLocked)
    }
    pub fn dodge_burn_plan(&self, name: impl Into<Arc<str>>) -> Result<RetouchLayerPlan, RetouchLayerRefusal> {
        if let Some(refusal) = self.dodge_burn_refusal() {
            return Err(refusal);
        }
        let (index, parent) = self.working.occurrence.map_or((0, None), |h| self.above_clipping_stack(h));
        let containing = parent
            .and_then(|h| match self.scene().occurrence(h)?.content {
                OccurrenceContent::Stack(s) => Some(s),
                _ => None,
            })
            .unwrap_or(self.composition().result);
        let canvas = self.composition().size;
        let paint = RecordChange::insert(
            &self.artwork.paint,
            PaintSource { color_mode:Default::default(), domain: canvas, raster: Default::default(), base: None, operations: Arc::default() },
        );
        let target = SourceTarget::Paint(paint.handle);
        let mut o = Occurrence::new(OccurrenceContent::Paint(paint.handle), name);
        o.blend = LayerBlend::SoftLight;
        let occurrence = RecordChange::insert(&self.artwork.occurrences, o);
        let active = occurrence.handle;
        let mut stack = self.artwork.stacks.get(containing).unwrap().clone();
        stack.entries.insert(index, active);
        let gray = self.soft_light_neutral();
        let fill = RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(self.artwork.coverage.next_handle(), canvas, [0; 2]),
            kind: RasterOperationKind::Fill { color: [gray, gray, gray, 1.], alpha_locked: false },
        };
        let mut working = self.working.clone();
        working.occurrence = Some(active);
        working.layer_selection = [active].into();
        working.layer_anchor = Some(active);
        working.target = Some(target);
        working.inspect_mask = None;
        Ok(RetouchLayerPlan {
            edits: vec![
                Edit::Paint(paint),
                Edit::Occurrence(occurrence),
                Edit::Stack(
                    RecordChange::replace(&self.artwork.stacks, containing, Some(stack)).map_err(|_| RetouchLayerRefusal::NoLayer)?,
                ),
                Edit::Working(working),
            ],
            operations: vec![(target, fill)],
            active,
        })
    }
    pub fn separation_refusal(&self, target: OccurrenceHandle) -> Option<RetouchLayerRefusal> {
        use RetouchLayerRefusal as R;
        if self.composition().blend != BlendSpace::Perceptual {
            return Some(R::Linear);
        }
        let scene = self.scene();
        let Some(o) = scene.occurrence(target) else {
            return Some(R::NoLayer);
        };
        if o.kind() == LayerKind::Object {
            Some(R::Objects)
        } else if o.kind() != LayerKind::Paint {
            Some(R::NotPaint)
        } else if !o.visible {
            Some(R::Hidden)
        } else if o.blend != LayerBlend::Normal {
            Some(R::NotNormal)
        } else if scene.parent(target).is_some_and(|p| self.is_locked(p)) {
            Some(R::GroupLocked)
        } else {
            None
        }
    }
    pub fn separation_plan(
        &self,
        target: OccurrenceHandle,
        filters: &SeparationFilters,
        [group_name, low_name, high_name]: [Arc<str>; 3],
    ) -> Result<RetouchLayerPlan, RetouchLayerRefusal> {
        if let Some(refusal) = self.separation_refusal(target) {
            return Err(refusal);
        }
        let scene = self.scene();
        let old = scene.occurrence(target).unwrap();
        let parent = scene.parent(target);
        let parent_origin = self.scene().layer_origin(parent);
        let canvas = self.composition().size;
        let mut high_scene = self.snapshot();
        let authored = &mut Arc::make_mut(&mut high_scene).artwork;
        let source = authored.occurrences.get_mut(target).unwrap();
        source.opacity = 1.;
        source.attachment = crate::Attachment::None;
        let high_scope = SceneScope::Members(vec![target].into());
        let mut low_artwork = high_scene.artwork.clone();
        let effect = low_artwork
            .effects
            .insert(PortableId::random(), EffectApplication::new(filters.blur.program.clone(), filters.blur.values.clone(), canvas))
            .map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let mut blur = Occurrence::new(OccurrenceContent::Effect(effect), filters.blur.program.id.clone());
        blur.attachment = crate::Attachment::Effect;
        let blur = low_artwork.occurrences.insert(PortableId::random(), blur).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let stack = scene.stack(target).unwrap();
        let entries = &mut low_artwork.stacks.get_mut(stack).unwrap().entries;
        let index = entries.iter().position(|h| *h == target).unwrap();
        entries.insert(index, blur);
        let low_doc = Document::from_artwork(low_artwork).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let low_scene = low_doc.snapshot();
        let low_scope = SceneScope::Members(vec![blur, target].into());
        let mut allocator = self.artwork.clone();
        let source = |domain| PaintSource { color_mode:Default::default(), domain, raster: Default::default(), base: None, operations: Arc::default() };
        let low = RecordChange::insert(&allocator.paint, source(canvas));
        allocator.paint.change(low.handle, low.id, low.value.clone()).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let high = RecordChange::insert(&allocator.paint, source(canvas));
        let low_target = SourceTarget::Paint(low.handle);
        let high_target = SourceTarget::Paint(high.handle);
        let operation = |kind| {
            let op = RasterOperation {
                placement: Affine::IDENTITY,
                coverage: CoverageSnapshot::reveal_all(self.artwork.coverage.next_handle(), canvas, [0; 2]),
                kind,
            };
            if self.exceeds_publication(&op, canvas) { Err(RetouchLayerRefusal::TooLarge) } else { Ok(op) }
        };
        let operations = vec![
            (low_target, operation(RasterOperationKind::Bake { scene: low_scene, scope: low_scope, offset: Point::default() })?),
            (
                high_target,
                operation(RasterOperationKind::FrequencyDetail {
                    scene: high_scene,
                    scope: high_scope,
                    offset: Point::default(),
                    low: low.handle,
                })?,
            ),
        ];
        let nested = RecordChange::insert(&allocator.stacks, Stack::default());
        allocator.stacks.change(nested.handle, nested.id, nested.value.clone()).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let mut group = Occurrence::new(OccurrenceContent::Stack(nested.handle), group_name);
        group.opacity = old.opacity;
        group.attachment = old.attachment;
        let group = RecordChange::insert(&allocator.occurrences, group);
        allocator.occurrences.change(group.handle, group.id, group.value.clone()).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let offset = offsets::checked_sub([0; 2], parent_origin).ok_or(RetouchLayerRefusal::TooLarge)?;
        let part = |h, name, blend| {
            let mut o = Occurrence::new(OccurrenceContent::Paint(h), name);
            o.offset = offset;
            o.blend = blend;
            o
        };
        let high_o = RecordChange::insert(&allocator.occurrences, part(high.handle, high_name, LayerBlend::LinearLight));
        allocator.occurrences.change(high_o.handle, high_o.id, high_o.value.clone()).map_err(|_| RetouchLayerRefusal::TooLarge)?;
        let low_o = RecordChange::insert(&allocator.occurrences, part(low.handle, low_name, LayerBlend::Normal));
        let active = high_o.handle;
        let mut nested = nested;
        nested.value.as_mut().unwrap().entries = vec![high_o.handle, low_o.handle];
        let containing = scene.stack(target).unwrap();
        let mut stack = self.artwork.stacks.get(containing).unwrap().clone();
        let index = stack.entries.iter().position(|h| *h == target).unwrap();
        stack.entries.insert(index, group.handle);
        let mut hidden = old.clone();
        hidden.visible = false;
        let mut working = self.working.clone();
        working.occurrence = Some(active);
        working.layer_selection = [active].into();
        working.layer_anchor = Some(active);
        working.target = Some(high_target);
        working.inspect_mask = None;
        Ok(RetouchLayerPlan {
            edits: vec![
                Edit::Paint(low),
                Edit::Paint(high),
                Edit::Stack(nested),
                Edit::Occurrence(group),
                Edit::Occurrence(high_o),
                Edit::Occurrence(low_o),
                Edit::Occurrence(
                    RecordChange::replace(&self.artwork.occurrences, target, Some(hidden)).map_err(|_| RetouchLayerRefusal::NoLayer)?,
                ),
                Edit::Stack(
                    RecordChange::replace(&self.artwork.stacks, containing, Some(stack)).map_err(|_| RetouchLayerRefusal::NoLayer)?,
                ),
                Edit::Working(working),
            ],
            operations,
            active,
        })
    }
}
#[cfg(test)]
#[path = "retouch_layers_tests.rs"]
mod tests;
