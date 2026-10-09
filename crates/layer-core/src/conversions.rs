use super::*;
use crate::authored::*;

/// Why Convert to Object or Rasterize Layer can't run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConversionRefusal {
    NoLayer,
    NotPaint,
    NotObjects,
    Locked,
    NoMask,
    MaskDisabled,
    TooLarge,
}
impl From<MergeRefusal> for ConversionRefusal {
    fn from(refusal: MergeRefusal) -> Self {
        match refusal {
            MergeRefusal::TooLarge | MergeRefusal::UnboundedSupport => Self::TooLarge,
            _ => Self::NoLayer,
        }
    }
}

/// Pixels a capture worker evaluates into one immutable working image:
/// `scope` of `scene`, moved by `offset`, within `window` of `extent`. `trim`
/// narrows the window to that source's visible pixels; `selection` scales
/// coverage.
#[derive(Clone, Debug)]
pub struct ImageCapture {
    pub scene: Arc<SceneSnapshot>,
    pub scope: SceneScope,
    pub offset: Point,
    pub extent: [u32; 2],
    pub window: [u32; 4],
    pub trim: Option<SourceTarget>,
    pub selection: Option<Arc<Selection>>,
}

/// Convert to Object either reuses an untouched base image or evaluates the
/// layer's current appearance on the capture worker first.
#[derive(Clone, Debug)]
pub enum ObjectConversion {
    Ready(Edit),
    Capture(ImageCapture),
}

impl MergePlan {
    /// The capture worker request for a bake that reads image objects, or
    /// `None` when the bake can run inside a frame.
    pub fn image_capture(&self, extent: [u32; 2], selection: Option<Arc<Selection>>) -> Option<ImageCapture> {
        let RasterOperationKind::Bake { scene, scope, offset } = &self.operation.kind else { return None; };
        let view = scene.view().with_scope(scope);
        view.order().iter().any(|h| view.visible(*h) && view.object_layer(*h).is_some()).then(|| ImageCapture {
            scene: scene.clone(), scope: scope.clone(), offset: *offset, extent, window: [0, 0, extent[0], extent[1]], trim: None, selection,
        })
    }
    /// The plan's records with the captured pixels as the new layer's base,
    /// in place of the pending bake.
    pub fn with_image(&self, image: Option<(Image, [i64; 2])>) -> Result<Vec<Edit>, String> {
        let SourceTarget::Paint(handle) = self.target else { return Err("Missing merged paint".into()); };
        let local = |value: i64| u32::try_from(value).map_err(|_| "Captured pixels outside the layer".to_string());
        let base = image.map(|(image, [x, y])| Ok::<_, String>(PaintBase { image, offset: [local(x)?, local(y)?], policy: PaintBasePolicy::WorkingPixels })).transpose()?;
        let mut edits = self.edits.clone();
        for edit in &mut edits {
            if let Edit::Paint(change) = edit && change.handle == handle && let Some(source) = &mut change.value { source.base = base.clone(); }
        }
        Ok(edits)
    }
}

impl Document {
    pub fn convert_to_object_refusal(&self, h: OccurrenceHandle) -> Option<ConversionRefusal> {
        let occurrence = self.scene().occurrence(h)?;
        if occurrence.kind() != LayerKind::Paint { return Some(ConversionRefusal::NotPaint); }
        self.is_locked(h).then_some(ConversionRefusal::Locked)
    }
    pub fn convert_to_object(&self, h: OccurrenceHandle) -> Result<ObjectConversion, ConversionRefusal> {
        if let Some(refusal) = self.convert_to_object_refusal(h) { return Err(refusal); }
        let scene = self.scene();
        let target = scene.source_target(h).ok_or(ConversionRefusal::NotPaint)?;
        let paint = scene.paint_source(h).ok_or(ConversionRefusal::NotPaint)?;
        let untouched = paint.operations.is_empty() && paint.color_mode == Default::default()
            && matches!(paint.raster.try_data(), Some(Ok(data)) if data.tiles.is_empty() && data.watercolor.is_none());
        if untouched {
            let Some(base) = &paint.base else { return self.object_conversion_edit(h, None).map(ObjectConversion::Ready); };
            return self.object_conversion_edit(h, Some((base.image.clone(), base.offset.map(i64::from)))).map(ObjectConversion::Ready);
        }
        let scope = SceneScope::Raw(target);
        let origin = scene.layer_origin(Some(h));
        let local = self.scene().with_scope(&scope).with_offset64(origin.map(|v| -(v as f64)));
        let support = merge::output_support(local, merge::Reach::Content)?;
        let Some([min, max]) = support.grid() else { return self.object_conversion_edit(h, None).map(ObjectConversion::Ready); };
        let window = [min[0].max(0) as u32, min[1].max(0) as u32, max[0].clamp(0, i64::from(paint.domain[0])) as u32, max[1].clamp(0, i64::from(paint.domain[1])) as u32];
        Ok(ObjectConversion::Capture(ImageCapture {
            scene: self.snapshot(), scope, offset: Point { x: -origin[0] as f32, y: -origin[1] as f32 }, extent: paint.domain,
            window: [window[0], window[1], window[2].saturating_sub(window[0]), window[3].saturating_sub(window[1])], trim: Some(target), selection: None,
        }))
    }
    /// Replace the paint layer `h` with an object layer holding `image` at
    /// `origin` in the layer's pixels, keeping the layer's presentation and
    /// mask where they were.
    pub fn object_conversion_edit(&self, h: OccurrenceHandle, image: Option<(Image, [i64; 2])>) -> Result<Edit, ConversionRefusal> {
        if let Some(refusal) = self.convert_to_object_refusal(h) { return Err(refusal); }
        let scene = self.scene();
        let mut occurrence = scene.occurrence(h).ok_or(ConversionRefusal::NoLayer)?.clone();
        let OccurrenceContent::Paint(paint) = occurrence.content else { return Err(ConversionRefusal::NotPaint); };
        let mut edits = Vec::new();
        let (image, origin) = match image {
            Some(image) => image,
            None => {
                use crate::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
                let mut source = SourceBuilder::new([1, 1], SourceInterpretation { channels: SourceChannels::Rgba,
                    depth: crate::color::SampleDepth::U8, profile: Default::default(), profile_assumed: false }, crate::ProjectLimits::default().asset_bytes as usize).map_err(|_| ConversionRefusal::TooLarge)?;
                source.push_row(&[0; 4]).map_err(|_| ConversionRefusal::TooLarge)?;
                (Image::new(Arc::new(source.finish().map_err(|_| ConversionRefusal::TooLarge)?)), [0; 2])
            }
        };
        let mut object = ImageObject::new(image);
        object.affine = Affine64([1., 0., 0., 1., origin[0] as f64, origin[1] as f64]);
        object.validate().map_err(|_| ConversionRefusal::TooLarge)?;
        let object = RecordChange::insert(&self.artwork.objects, object);
        occurrence.content = OccurrenceContent::Objects(object.handle);
        occurrence.alpha_locked = false;
        edits.push(Edit::ImageObject(object));
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, h, Some(occurrence)).map_err(|_| ConversionRefusal::NoLayer)?));
        edits.push(Edit::Paint(RecordChange::remove(&self.artwork.paint, paint).map_err(|_| ConversionRefusal::NoLayer)?));
        let mut working = self.working.clone();
        working.occurrence = Some(h);
        if working.target == Some(SourceTarget::Paint(paint)) || working.target.is_none() { working.target = None; }
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        let edit = Edit::Batch(edits);
        let mut candidate = self.clone();
        candidate.apply(edit.clone()).map_err(|_| ConversionRefusal::NoLayer)?;
        Ok(edit)
    }

    pub fn rasterize_refusal(&self, h: OccurrenceHandle, apply_mask: bool) -> Option<ConversionRefusal> {
        let occurrence = self.scene().occurrence(h)?;
        if occurrence.kind() != LayerKind::Object { return Some(ConversionRefusal::NotObjects); }
        if self.is_locked(h) { return Some(ConversionRefusal::Locked); }
        match occurrence.mask.as_ref() {
            _ if !apply_mask => None,
            None => Some(ConversionRefusal::NoMask),
            Some(mask) => (!mask.enabled).then_some(ConversionRefusal::MaskDisabled),
        }
    }
    /// Rasterize Layer: the object layer's own content, at the document grid,
    /// becomes its paint. Its mask, attached effects, clipping, opacity and
    /// blend stay on the layer and apply once; `apply_mask` bakes the enabled
    /// mask into the pixels and removes it.
    pub fn rasterize_plan(&self, h: OccurrenceHandle, apply_mask: bool) -> Result<MergePlan, ConversionRefusal> {
        let scene = self.scene();
        let occurrence = scene.occurrence(h).ok_or(ConversionRefusal::NoLayer)?;
        if let Some(refusal) = self.rasterize_refusal(h, apply_mask) { return Err(refusal); }
        let OccurrenceContent::Objects(layer) = occurrence.content else { return Err(ConversionRefusal::NotObjects); };
        let scope = if apply_mask { SceneScope::Members(vec![h].into()) } else { SceneScope::RawObjects(h) };
        let (origin, extent) = self.bake_window(scene.with_scope(&SceneScope::RawObjects(h)))?;
        let parent_origin = scene.layer_origin(scene.parent(h));
        let offset = offsets::checked_sub(offsets::exact(origin).ok_or(ConversionRefusal::TooLarge)?, parent_origin).ok_or(ConversionRefusal::TooLarge)?;
        let mut snapshot = self.snapshot();
        let raw = Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(h).ok_or(ConversionRefusal::NoLayer)?;
        raw.visible = true; raw.opacity = 1.; raw.blend = LayerBlend::Normal; raw.attachment = Attachment::None;
        if !apply_mask { raw.mask = None; }
        let operation = RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(self.artwork.coverage.next_handle(), extent, [0; 2]),
            kind: RasterOperationKind::Bake { scene: snapshot, scope, offset: Point { x: -origin.x, y: -origin.y } },
        };
        if self.exceeds_publication(&operation, extent) { return Err(ConversionRefusal::TooLarge); }
        let paint = RecordChange::insert(&self.artwork.paint,
            PaintSource { color_mode: Default::default(), domain: extent, raster: Default::default(), base: None, operations: Arc::default() });
        let target = SourceTarget::Paint(paint.handle);
        let mut result = occurrence.clone();
        let previous = result.offset;
        result.content = OccurrenceContent::Paint(paint.handle);
        result.offset = offset;
        let mut edits = vec![Edit::Paint(paint)];
        if apply_mask {
            let mask = result.mask.take().ok_or(ConversionRefusal::NoMask)?;
            edits.push(Edit::Coverage(RecordChange::remove(&self.artwork.coverage, mask.source).map_err(|_| ConversionRefusal::NoLayer)?));
        } else if let Some(mask) = result.mask.as_mut().filter(|mask| mask.linked) {
            mask.offset = offsets::checked_sub(previous, offset).and_then(|shift| offsets::checked_add(mask.offset, shift)).ok_or(ConversionRefusal::TooLarge)?;
        }
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, h, Some(result)).map_err(|_| ConversionRefusal::NoLayer)?));
        edits.push(Edit::ImageObject(RecordChange::remove(&self.artwork.objects, layer).map_err(|_| ConversionRefusal::NoLayer)?));
        let mut working = self.working.clone();
        working.occurrence = Some(h);
        working.target = Some(target);
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        Ok(MergePlan { edits, result: h, target, operation })
    }
}

#[cfg(test)]
#[path = "conversions_tests.rs"]
mod tests;
