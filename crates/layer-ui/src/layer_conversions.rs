//! Convert to Object Layer and Rasterize Layer. Bakes that read image objects,
//! including merges, rasterizing and copying selections to layers, are
//! evaluated on the snapshot worker, where canonical sampling finishes, and
//! publish once the drawing is unchanged. Converting reuses an untouched photo
//! or captures the layer's current appearance the same way.
use super::*;
use layer_core::{ConversionRefusal, ImageCapture, MergePlan, ObjectConversion};
use layer_core::authored::{Image, OccurrenceHandle};

enum Publish {
    Object(OccurrenceHandle),
    Bake { plan: Box<MergePlan>, reselect: Option<layer_core::Selection> },
    ClipboardCut {
        plan: Box<MergePlan>, remaining: std::collections::VecDeque<MergePlan>, edits: Vec<layer_core::Edit>,
        operations: Vec<(layer_core::SourceTarget, layer_core::RasterOperation)>, working: Box<layer_core::WorkingState>,
    },
}

pub(super) struct PendingConversion {
    epoch: u64,
    revision: u64,
    working: layer_core::WorkingState,
    capture: ImageCapture,
    publish: Publish,
    submitted: bool,
}

pub(super) fn conversion_refusal_text(refusal: ConversionRefusal, l: &Localizer) -> std::sync::Arc<str> {
    match refusal {
        ConversionRefusal::NoLayer => l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST),
        ConversionRefusal::NotPaint => l.text(MessageId::COMMANDS_SELECT_A_PAINT_LAYER),
        ConversionRefusal::NotObjects => l.text(MessageId::COMMANDS_REFUSAL_CONVERSIONS_SELECT_AN_IMAGE_LAYER),
        ConversionRefusal::Locked => l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED),
        ConversionRefusal::NoMask => l.text(MessageId::COMMANDS_THE_LAYER_HAS_NO_MASK),
        ConversionRefusal::MaskDisabled => l.text(MessageId::COMMANDS_ENABLE_THE_MASK_BEFORE_APPLYING_IT),
        ConversionRefusal::TooLarge => l.text(MessageId::COMMANDS_REFUSAL_CONVERSIONS_TOO_LARGE),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn conversion_busy(&self) -> bool { self.conversion.is_some() }

    pub(super) fn conversion_refusal(&self, command: CommandId) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.selection_masks.target().is_some() { return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST)); }
        if self.operation.active() { return Some(self.operation_refusal()); }
        if let Err(reason) = self.require_document_idle() { return Some(reason.into()); }
        let document = self.engine.document();
        let Some(layer) = document.working.occurrence else { return Some(conversion_refusal_text(ConversionRefusal::NoLayer, l)); };
        let refusal = match command {
            CommandId::ConvertToObject => document.convert_to_object_refusal(layer),
            CommandId::RasterizeLayer => document.rasterize_refusal(layer, false),
            CommandId::ApplyLayerMask => document.rasterize_refusal(layer, true),
            _ => None,
        };
        refusal.map(|refusal| conversion_refusal_text(refusal, l))
    }

    pub(super) fn rasterize_layer(&mut self, layer: OccurrenceHandle, apply_mask: bool) -> Result<(), String> {
        refused(self.conversion_refusal(if apply_mask { CommandId::ApplyLayerMask } else { CommandId::RasterizeLayer }))?;
        let plan = self.engine.document().rasterize_plan(layer, apply_mask)
            .map_err(|refusal| conversion_refusal_text(refusal, self.localization()).to_string())?;
        self.insert_bake(plan, None, None)
    }

    /// Insert a merge-like plan: directly when the renderer can bake it in a
    /// frame, otherwise after the snapshot worker has captured its pixels.
    pub(super) fn insert_bake(&mut self, plan: MergePlan, coverage: Option<layer_core::Selection>, reselect: Option<layer_core::Selection>) -> Result<(), String> {
        let extent = self.engine.document().target_extent(plan.target);
        let extent = plan.edits.iter().find_map(|edit| match edit {
            layer_core::Edit::Paint(change) if layer_core::SourceTarget::Paint(change.handle) == plan.target => change.value.as_ref().map(|paint| paint.domain),
            _ => None,
        }).unwrap_or(extent);
        if let Some(capture) = plan.image_capture(extent, coverage.map(std::sync::Arc::new)) {
            self.engine.validate_edit(&layer_core::Edit::Batch(plan.edits.clone())).map_err(error)?;
            return self.start_capture(capture, Publish::Bake { plan: Box::new(plan), reselect });
        }
        let selection_after = reselect.as_ref().map(|_| None);
        self.engine.insert_with_operations(plan.edits, vec![(plan.target, plan.operation)], selection_after).map_err(error)?;
        if reselect.is_some() { self.selection_masks.reselect = reselect; }
        self.layer_interaction.changed = true;
        Ok(())
    }

    pub(super) fn start_clipboard_cut(&mut self, mut plans: std::collections::VecDeque<MergePlan>, edits: Vec<layer_core::Edit>,
        operations: Vec<(layer_core::SourceTarget, layer_core::RasterOperation)>, working: layer_core::WorkingState,
    ) -> Result<(), String> {
        let plan = plans.pop_front().ok_or("Missing object layer capture")?;
        let extent = plan.edits.iter().find_map(|edit| match edit {
            layer_core::Edit::Paint(change) if layer_core::SourceTarget::Paint(change.handle) == plan.target => change.value.as_ref().map(|paint| paint.domain),
            _ => None,
        }).ok_or("Missing rasterized layer")?;
        let layer_core::RasterOperationKind::Bake { scene, scope, offset } = &plan.operation.kind else { return Err("Missing object layer capture".into()); };
        let capture = ImageCapture { scene: scene.clone(), scope: scope.clone(), offset: *offset, extent,
            window: [0, 0, extent[0], extent[1]], trim: None, selection: None };
        self.start_capture(capture, Publish::ClipboardCut { plan: Box::new(plan), remaining: plans, edits, operations, working: Box::new(working) })
    }

    fn start_capture(&mut self, capture: ImageCapture, publish: Publish) -> Result<(), String> {
        self.require_raster_snapshot()?;
        self.cancel_auto_levels();
        self.yield_histogram();
        let mut pending = PendingConversion { epoch: self.state.document_file.epoch, revision: self.engine.document().revision, working: self.engine.document().working.clone(), capture, publish, submitted: false };
        pending.submitted = self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::Image(pending.capture.clone())).map_err(error)?;
        self.conversion = Some(pending);
        self.refresh_commands();
        Ok(())
    }

    pub(super) fn convert_to_object(&mut self) -> Result<(), String> {
        refused(self.conversion_refusal(CommandId::ConvertToObject))?;
        let layer = self.engine.document().working.occurrence.ok_or("Select a layer first")?;
        let conversion = self.engine.document().convert_to_object(layer)
            .map_err(|refusal| conversion_refusal_text(refusal, self.localization()).to_string())?;
        match conversion {
            ObjectConversion::Ready(edit) => {
                self.layer_edit(edit)?;
                self.layer_interaction.changed = true;
            }
            ObjectConversion::Capture(capture) => self.start_capture(capture, Publish::Object(layer))?,
        }
        Ok(())
    }

    pub(super) fn poll_conversion(&mut self) -> u32 {
        let l = self.localization().clone();
        let (epoch, revision) = (self.state.document_file.epoch, self.engine.document().revision);
        let Some(pending) = self.conversion.as_mut() else { return 0; };
        let changed = pending.epoch != epoch || pending.revision != revision
            || matches!(pending.publish, Publish::ClipboardCut { .. }) && pending.working != self.engine.document().working;
        let result = if changed {
            self.engine.backend_mut().cancel_snapshot();
            Err(l.text(MessageId::COMMANDS_REFUSAL_CONVERSIONS_LAYER_CHANGED).to_string())
        } else if !pending.submitted {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::Image(pending.capture.clone())).map_err(error) {
                Ok(accepted) => { pending.submitted = accepted; return 0; }
                Err(error) => Err(error),
            }
        } else {
            match self.engine.backend_mut().take_snapshot() {
                None => return 0,
                Some(result) => result.map_err(error),
            }
        };
        let pending = self.conversion.take().expect("pending conversion");
        let result = result.and_then(|result| {
            let layer_render::SnapshotResult::Image(image) = result else { return Err(l.text(MessageId::COMMANDS_REFUSAL_CONVERSIONS_LAYER_CHANGED).to_string()); };
            let image = image.map(|(samples, origin)| (Image::new(samples), origin));
            match pending.publish {
                Publish::Object(layer) => {
                    let edit = self.engine.document().object_conversion_edit(layer, image)
                        .map_err(|refusal| conversion_refusal_text(refusal, &l).to_string())?;
                    self.layer_edit(edit)?;
                }
                Publish::ClipboardCut { plan, remaining, mut edits, operations, working } => {
                    edits.extend(plan.with_image(image)?);
                    if remaining.is_empty() {
                        edits.push(layer_core::Edit::Working(*working));
                        self.engine.insert_with_operations(edits, operations, None).map_err(error)?;
                    } else {
                        return self.start_clipboard_cut(remaining, edits, operations, *working);
                    }
                }
                Publish::Bake { plan, reselect } => {
                    self.engine.insert_with_operations(plan.with_image(image)?, Vec::new(), reselect.as_ref().map(|_| None)).map_err(error)?;
                    if reselect.is_some() { self.selection_masks.reselect = reselect; }
                }
            }
            self.layer_interaction.changed = true;
            self.refresh_document();
            Ok(())
        });
        if let Err(message) = result { self.notify(message); }
        self.refresh_commands();
        regions::DOCUMENT | regions::COMMANDS
    }

    pub(super) fn cancel_conversion(&mut self) -> bool {
        let cancelled = self.conversion.take().is_some();
        if cancelled { self.engine.backend_mut().cancel_snapshot(); }
        cancelled
    }

    pub(super) fn resubmit_conversion(&mut self) {
        if let Some(pending) = &mut self.conversion { pending.submitted = false; }
    }

    pub(super) fn conversion_menu_items(&self, handle: OccurrenceHandle) -> Vec<ContextMenuItem> {
        let document = self.engine.document();
        if Some(handle) != document.working.occurrence { return Vec::new(); }
        let command = match document.scene().occurrence(handle).map(|o| o.kind()) {
            Some(LayerKind::Paint) => CommandId::ConvertToObject,
            Some(LayerKind::Object) => CommandId::RasterizeLayer,
            _ => return Vec::new(),
        };
        let state = self.command(command);
        vec![ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label.to_string(), UiAction::Invoke { command }) }]
    }
}

#[cfg(test)]
#[path = "layer_conversions_tests.rs"]
mod tests;
