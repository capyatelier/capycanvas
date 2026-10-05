//! Source repair never reconstructs or reinterprets committed raster edits.
use super::*;
use layer_core::{Document, Edit, RecordChange, authored::{Occurrence, OccurrenceContent, OccurrenceHandle, PaintHandle, PaintSource, SourceTarget}, color::source::SourceImage};
use std::sync::Arc;

pub(crate) fn baked(source: &PaintSource) -> bool {
    !source.raster.is_empty() || !source.operations.is_empty()
}
fn paint_source(document: &Document, handle: OccurrenceHandle) -> Option<(PaintHandle, &PaintSource)> {
    let OccurrenceContent::Paint(paint) = document.scene().occurrence(handle)?.content else { return None; };
    Some((paint, document.artwork.paint.get(paint)?))
}
fn repair_edit(document: &Document, handle: OccurrenceHandle, corrected: SourceImage) -> Result<(Edit, OccurrenceHandle), String> {
    let (paint, source) = paint_source(document, handle).ok_or("Select a placed photo layer")?;
    let mut replacement = source.clone();
    replacement.original = Some(Arc::new(corrected));
    if !baked(source) {
        return Ok((Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(replacement)).map_err(error)?), handle));
    }
    let occurrence = document.scene().occurrence(handle).unwrap();
    replacement.raster = Default::default();
    replacement.operations = Default::default();
    let paint_change = RecordChange::insert(&document.artwork.paint, replacement);
    let mut replacement = Occurrence::new(OccurrenceContent::Paint(paint_change.handle), format!("{} (corrected source)", occurrence.name.chars().take(109).collect::<String>()));
    replacement.translation = occurrence.translation;
    replacement.placement = occurrence.placement.clone();
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
    Ok((Edit::Batch(vec![Edit::Paint(paint_change), Edit::Occurrence(occurrence_change), Edit::Stack(RecordChange::replace(&document.artwork.stacks, stack, Some(entries)).map_err(error)?), Edit::Working(working)]), result))
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
        let document = self.engine.document();
        !document.is_locked(handle) && paint_source(document, handle).is_some_and(|(_, source)| source.original.as_ref().is_some_and(|source| source.is_original()))
    }
    pub(super) fn revert_to_original_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.target().is_some() { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST)); }
        if matches!(document.working.target, Some(SourceTarget::Coverage(_))) { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST)); }
        if self.state.document_file.busy { return Some(l.text(MessageId::COMMANDS_WAIT_FOR_THE_CURRENT_FILE_OPERATION)); }
        let Some(handle) = document.working.occurrence else { return Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PLACED_PHOTO_LAYER)); };
        let Some((_, source)) = paint_source(document, handle) else { return Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PLACED_PHOTO_LAYER)); };
        match &source.original {
            None => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_SELECT_A_PLACED_PHOTO_LAYER)),
            Some(original) if !original.is_original() => Some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_A_RASTERIZED_PHOTO_HAS_NO_ORIGINAL_TO_RETURN_TO)),
            Some(_) if document.is_locked(handle) => Some(l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED)),
            Some(_) => (!baked(source)).then_some(l.text(MessageId::COMMANDS_REFUSAL_SOURCE_EDIT_THIS_PHOTO_HAS_NO_EDITS)),
        }
    }
    pub(super) fn revert_to_original(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.revert_to_original_refusal())?;
        let document = self.engine.document();
        let handle = document.working.occurrence.ok_or("Select a placed photo layer")?;
        let (paint, source) = paint_source(document, handle).ok_or("Select a placed photo layer")?;
        let mut source = source.clone();
        source.raster = Default::default(); source.operations = Default::default(); source.color_mode = Default::default();
        self.layer_edit(Edit::Paint(RecordChange::replace(&document.artwork.paint, paint, Some(source)).map_err(error)?))?;
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn request_source_edit(&mut self, handle: OccurrenceHandle, request: DocumentRequest) -> Result<(), String> {
        if !self.can_edit_original(handle) { return Err("Select an unlocked retained image layer".into()); }
        self.request_document(request)
    }
    fn validate_source_repair(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, corrected: &SourceImage) -> Result<(), String> {
        self.require_document_idle()?;
        if !self.can_edit_original(handle) { return Err("Select an unlocked retained image layer".into()); }
        let (_, source) = paint_source(self.engine.document(), handle).unwrap();
        if !Arc::ptr_eq(source.original.as_ref().unwrap(), original) { return Err("The source changed while choosing its profile; try again".into()); }
        corrected.validate()?;
        if corrected.kind != original.kind || corrected.extent != original.extent || corrected.interpretation.channels != original.interpretation.channels
            || corrected.interpretation.depth != original.interpretation.depth || corrected.interpretation.profile_assumed || corrected.tiles.len() != original.tiles.len()
            || !corrected.tiles.iter().zip(&original.tiles).all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y)) {
            return Err("Source profile repair must preserve the original image samples".into());
        }
        Ok(())
    }
    pub(crate) fn prepare_source_edit(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, converted: Arc<SourceImage>, rasterize: bool) -> Result<(Document, Edit), String> {
        let edit = if rasterize {
            let (paint, source) = self.rasterized_source(handle, original, converted)?;
            Edit::Paint(RecordChange::replace(&self.engine.document().artwork.paint, paint, Some(source)).map_err(error)?)
        } else {
            self.validate_source_repair(handle, original, &converted)?;
            if *converted == **original { Edit::Batch(Vec::new()) }
            else { repair_edit(self.engine.document(), handle, Arc::unwrap_or_clone(converted))?.0 }
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
    pub fn preview_layer_source(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, corrected: SourceImage) -> Result<Document, String> {
        self.validate_source_repair(handle, original, &corrected)?;
        if corrected == **original { return self.document_snapshot(); }
        let (edit, _) = repair_edit(self.engine.document(), handle, corrected)?;
        self.source_edit_candidates(&edit, Default::default())
    }
    pub fn repair_layer_source(&mut self, handle: OccurrenceHandle, original: &Arc<SourceImage>, corrected: SourceImage) -> Result<OccurrenceHandle, String> {
        self.validate_source_repair(handle, original, &corrected)?;
        if corrected == **original { return Ok(handle); }
        let (edit, result) = repair_edit(self.engine.document(), handle, corrected)?;
        self.source_edit_candidates(&edit, Default::default())?;
        self.engine.apply_edit(edit).map_err(error)?;
        self.refresh_document(); self.refresh_commands(); self.layer_interaction.changed = true;
        Ok(result)
    }
    fn rasterized_source(&self, handle: OccurrenceHandle, original: &Arc<SourceImage>, converted: Arc<SourceImage>) -> Result<(PaintHandle, PaintSource), String> {
        self.require_document_idle()?;
        if !self.can_edit_original(handle) { return Err("Select an unlocked retained image layer".into()); }
        let document = self.engine.document();
        let (paint, source) = paint_source(document, handle).unwrap();
        if !Arc::ptr_eq(source.original.as_ref().unwrap(), original) { return Err("The source changed while rasterizing; try again".into()); }
        converted.validate()?;
        if converted.kind != layer_core::color::source::SourceKind::Rasterized || converted.extent != original.extent
            || converted.interpretation.profile != layer_core::color::ColorProfile::Builtin(document.composition().color.space)
            || converted.interpretation.depth != document.composition().color.depth {
            return Err("Rasterization must retain the full image extent in the document color mode".into());
        }
        let mut source = source.clone(); source.original = Some(converted);
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
