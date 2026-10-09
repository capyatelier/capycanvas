//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Document, Edit, RecordChange, authored::{ImageObjectHandle, Occurrence, OccurrenceContent, OccurrenceHandle, PaintHandle, PaintSource, SourceTarget}, color::source::SourceImage};
use std::sync::Arc;

/// The image use a source edit replaces: a paint layer's base, or one image
/// object. Other users of the same image keep their interpretation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceUse { Paint(OccurrenceHandle), Object(ImageObjectHandle) }
impl SourceUse {
    pub fn token(self) -> u64 {
        match self { Self::Paint(h) => occurrence_token(h), Self::Object(h) => object_token(h) }
    }
    pub fn from_token(token: u64) -> Result<Self, String> {
        object_handle(token).map(Self::Object).or_else(|_| occurrence_handle(token).map(Self::Paint))
    }
    pub(crate) fn original(self, document: &Document) -> Option<&Arc<SourceImage>> {
        match self {
            Self::Paint(h) => paint_source(document, h)?.1.base.as_ref().map(|base| base.image.storage()),
            Self::Object(h) => document.scene().object(h).map(|object| object.image.storage()),
        }
    }
    fn owner(self, document: &Document) -> Option<OccurrenceHandle> {
        match self { Self::Paint(h) => Some(h), Self::Object(h) => document.scene().object_owner(h) }
    }
}

pub(crate) fn baked(source: &PaintSource) -> bool {
    !source.raster.is_empty() || !source.operations.is_empty()
}
fn paint_source(document: &Document, handle: OccurrenceHandle) -> Option<(PaintHandle, &PaintSource)> {
    let OccurrenceContent::Paint(paint) = document.scene().occurrence(handle)?.content else { return None; };
    Some((paint, document.artwork.paint.get(paint)?))
}
fn repair_edit(document: &Document, target: SourceUse, corrected: SourceImage) -> Result<(Edit, SourceUse), String> {
    let handle = match target {
        SourceUse::Object(object) => {
            let mut value = document.scene().object(object).ok_or("Select an image")?.clone();
            value.image = Arc::new(corrected).into();
            return Ok((Edit::ImageObject(RecordChange::replace(&document.artwork.objects, object, Some(value)).map_err(error)?), target));
        }
        SourceUse::Paint(handle) => handle,
    };
    let (paint, source) = paint_source(document, handle).ok_or("Select a photo layer")?;
    let mut replacement = source.clone();
    replacement.base.as_mut().ok_or("Missing source binding")?.image = Arc::new(corrected).into();
    if !baked(source) {
        return Ok((Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(replacement)).map_err(error)?), target));
    }
    let occurrence = document.scene().occurrence(handle).unwrap();
    replacement.raster = Default::default();
    replacement.operations = Default::default();
    let paint_change = RecordChange::insert(&document.artwork.paint, replacement);
    let mut replacement = Occurrence::new(OccurrenceContent::Paint(paint_change.handle), format!("{} (corrected source)", occurrence.name.chars().take(109).collect::<String>()));
    replacement.offset = occurrence.offset;
    let occurrence_change = RecordChange::insert(&document.artwork.occurrences, replacement);
    let result = occurrence_change.handle;
    let stack = document.scene().stack(handle).ok_or("Missing source placement")?;
    let mut entries = document.artwork.stacks.get(stack).unwrap().clone();
    let index = entries.entries.iter().position(|entry| *entry == handle).ok_or("Missing source placement")?;
    entries.entries.insert(index, result);
    let mut working = document.working.clone();
    working.occurrence = Some(result);
    working.target = Some(SourceTarget::Paint(paint_change.handle));
    working.inspect_mask = None;
    Ok((Edit::Batch(vec![Edit::Paint(paint_change), Edit::Occurrence(occurrence_change), Edit::Stack(RecordChange::replace(&document.artwork.stacks, stack, Some(entries)).map_err(error)?), Edit::Working(working)]), SourceUse::Paint(result)))
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn source_edit_candidates(&self, edit: &Edit, limits: layer_core::ProjectLimits) -> Result<Document, String> {
        let mut document = self.document_snapshot()?;
        document.apply(edit.clone()).map_err(error)?;
        document.validate(limits)?;
        self.engine.validate_edit(edit).map_err(error)?;
        Ok(document)
    }

    pub(super) fn can_edit_original(&self, handle: OccurrenceHandle) -> bool {
        self.can_repair_source(SourceUse::Paint(handle))
    }
    /// The image use Repair Source Profile edits: the one selected image of
    /// the active object layer, or the active layer's retained photo.
    pub(super) fn active_source_use(&self) -> Option<SourceUse> {
        let document = self.engine.document();
        let active = document.working.occurrence?;
        if document.scene().object_layer(active).is_some() {
            let [object] = document.selected_objects().iter().copied().collect::<Vec<_>>()[..] else { return None; };
            return (document.scene().object_owner(object) == Some(active)).then_some(SourceUse::Object(object));
        }
        Some(SourceUse::Paint(active))
    }
    pub(super) fn can_repair_source(&self, target: SourceUse) -> bool {
        let document = self.engine.document();
        target.owner(document).is_some_and(|owner| !document.is_locked(owner)) && match target {
            SourceUse::Paint(handle) => paint_source(document, handle).is_some_and(|(_, source)| source.base.as_ref().is_some_and(|source| source.is_original())),
            SourceUse::Object(handle) => document.scene().object(handle).is_some(),
        }
    }
    pub(super) fn discard_paint_edits_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.target().is_some() { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST)); }
        if matches!(document.working.target, Some(SourceTarget::Coverage(_))) { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST)); }
        if self.state.document_file.busy { return Some(l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION)); }
        let Some(handle) = document.working.occurrence else { return Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PHOTO_LAYER)); };
        let Some((_, source)) = paint_source(document, handle) else { return Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PHOTO_LAYER)); };
        match &source.base {
            None => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PHOTO_LAYER)),
            Some(original) if !original.is_original() => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_THE_PHOTO_IS_ALREADY_PIXELS)),
            Some(_) if document.is_locked(handle) => Some(l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)),
            Some(_) => (!baked(source)).then_some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_THIS_PHOTO_HAS_NO_EDITS)),
        }
    }
    pub(super) fn discard_paint_edits(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.discard_paint_edits_refusal())?;
        let document = self.engine.document();
        let handle = document.working.occurrence.ok_or("Select a photo layer")?;
        let (paint, source) = paint_source(document, handle).ok_or("Select a photo layer")?;
        let mut source = source.clone();
        source.raster = Default::default(); source.operations = Default::default();
        self.layer_edit(Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(source)).map_err(error)?))?;
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn request_source_edit(&mut self, target: SourceUse, request: DocumentRequest) -> Result<(), String> {
        if !self.can_repair_source(target) { return Err("Select an unlocked photo layer or image".into()); }
        self.request_document(request)
    }
    fn validate_source_repair(&self, target: SourceUse, original: &Arc<SourceImage>, corrected: &SourceImage) -> Result<(), String> {
        self.require_document_idle()?;
        if !self.can_repair_source(target) { return Err("Select an unlocked photo layer or image".into()); }
        if !target.original(self.engine.document()).is_some_and(|current| Arc::ptr_eq(current, original)) { return Err("The source changed while choosing its profile; try again".into()); }
        corrected.validate()?;
        if corrected.extent != original.extent || corrected.interpretation.channels != original.interpretation.channels
            || corrected.interpretation.depth != original.interpretation.depth || corrected.interpretation.profile_assumed || corrected.tiles.len() != original.tiles.len()
            || !corrected.tiles.iter().zip(&original.tiles).all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y)) {
            return Err("Source profile repair must preserve the original image samples".into());
        }
        Ok(())
    }
    pub(crate) fn prepare_source_edit(&self, target: SourceUse, original: &Arc<SourceImage>, converted: Arc<SourceImage>, rasterize: bool) -> Result<(Document, Edit), String> {
        let edit = if rasterize {
            let SourceUse::Paint(handle) = target else { return Err("Select an unlocked photo layer or image".into()); };
            let (paint, source) = self.rasterized_source(handle, original, converted)?;
            Edit::Paint(RecordChange::replace(&self.engine.document().artwork.paint, paint, Some(source)).map_err(error)?)
        } else {
            let converted=self.engine.document().artwork.intern_source_image(converted)?;
            self.validate_source_repair(target, original, &converted)?;
            if *converted == **original { Edit::Batch(Vec::new()) }
            else { repair_edit(self.engine.document(), target, Arc::unwrap_or_clone(converted))?.0 }
        };
        let candidate = if matches!(&edit, Edit::Batch(edits) if edits.is_empty()) { self.document_snapshot()? } else { self.source_edit_candidates(&edit, Default::default())? };
        Ok((candidate, edit))
    }
    pub(crate) fn commit_prepared_source_edit(&mut self, edit: Edit) -> Result<(), String> {
        if matches!(&edit, Edit::Batch(edits) if edits.is_empty()) { return Ok(()); }
        self.source_edit_candidates(&edit, Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document(); self.refresh_commands(); self.layer_interaction.changed = true;
        Ok(())
    }
    pub fn preview_layer_source(&self, target: SourceUse, original: &Arc<SourceImage>, corrected: SourceImage) -> Result<Document, String> {
        let corrected=Arc::unwrap_or_clone(self.engine.document().artwork.intern_source_image(Arc::new(corrected))?);
        self.validate_source_repair(target, original, &corrected)?;
        if corrected == **original { return self.document_snapshot(); }
        let (edit, _) = repair_edit(self.engine.document(), target, corrected)?;
        self.source_edit_candidates(&edit, Default::default())
    }
    pub fn repair_layer_source(&mut self, target: SourceUse, original: &Arc<SourceImage>, corrected: SourceImage) -> Result<SourceUse, String> {
        let corrected=Arc::unwrap_or_clone(self.engine.document().artwork.intern_source_image(Arc::new(corrected))?);
        self.validate_source_repair(target, original, &corrected)?;
        if corrected == **original { return Ok(target); }
        let (edit, result) = repair_edit(self.engine.document(), target, corrected)?;
        self.source_edit_candidates(&edit, Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document(); self.refresh_commands(); self.layer_interaction.changed = true;
        Ok(result)
    }
    fn rasterized_source(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, converted: Arc<SourceImage>) -> Result<(PaintHandle, PaintSource), String> {
        self.require_document_idle()?;
        if !self.can_edit_original(handle) { return Err("Select an unlocked photo layer or image".into()); }
        let document = self.engine.document();
        let (paint, source) = paint_source(document, handle).unwrap();
        if !Arc::ptr_eq(source.base.as_ref().unwrap().image.storage(), original) { return Err("The source changed while rasterizing; try again".into()); }
        let converted=document.artwork.intern_source_image(converted)?;
        if converted.extent != original.extent
            || converted.interpretation.profile != layer_core::color::ColorProfile::Builtin(document.composition().color.space)
            || converted.interpretation.depth != document.composition().color.depth {
            return Err("Rasterization must retain the full image extent in the document color mode".into());
        }
        let mut source = source.clone();
        let base = source.base.as_mut().ok_or("Missing source binding")?;
        base.image = converted.into(); base.policy = layer_core::authored::PaintBasePolicy::WorkingPixels;
        base.validate(source.domain, document.composition().color)?;
        Ok((paint, source))
    }
    pub fn preview_rasterized_source(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, converted: Arc<SourceImage>) -> Result<Document, String> {
        let (paint, source) = self.rasterized_source(handle, original, converted)?;
        self.source_edit_candidates(&Edit::Paint(RecordChange::replace(&self.engine.document().artwork.paint, paint, Some(source)).map_err(error)?), Default::default())
    }
    pub fn apply_rasterized_source(&mut self, handle: OccurrenceHandle, original: &Arc<SourceImage>, converted: Arc<SourceImage>) -> Result<(), String> {
        let (paint, source) = self.rasterized_source(handle, original, converted)?;
        let edit = Edit::Paint(RecordChange::replace(&self.engine.document().artwork.paint, paint, Some(source)).map_err(error)?);
        self.source_edit_candidates(&edit, Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document(); self.refresh_commands(); self.layer_interaction.changed = true;
        Ok(())
    }
}
