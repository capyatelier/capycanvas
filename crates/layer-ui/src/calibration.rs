use super::*;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, EffectValue, authored::EffectBaseline};
use std::sync::Arc;

pub(crate) struct Calibration {
    pub role:layer_core::levels::CalibrationRole,
    pub page:u8,
    pub original: EffectBaseline,
    pub epoch: u64,
    pub document_epoch: u64,
    pub width: u32,
    pub request: Option<ArtworkSampleRequest>,
    pub queued: Option<[f32; 2]>,
    pub submitted: bool,
}

pub(super) enum CalibrationFailure { Message(MessageId), Diagnostic(String) }

impl From<String> for CalibrationFailure {
    fn from(reason: String) -> Self { Self::Diagnostic(reason) }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn start_calibration(&mut self, layer: u64, epoch: u64, role:layer_core::levels::CalibrationRole) -> Result<(), String> {
        if !Panel::Histogram.available_on(self.state.platform) { return Err(self.localization().text(MessageId::RESOURCES_PICKER_UNAVAILABLE).to_string()); }
        self.require_idle()?;
        let doc = self.engine.document();
        let handle = occurrence_handle(layer)?;
        if doc.is_locked(handle) || doc.scene().effect(handle).is_none_or(|effect| !matches!(effect.program.id.as_ref(),"white_balance"|"levels"|"curves")) {
            return Err(self.localization().text(MessageId::RESOURCES_ERROR_ARTWORK_REQUIRED).to_string());
        }
        let original = effects::effect_baseline(doc, handle)?;
        self.cancel_picker();
        self.cancel_auto_levels();self.cancel_histogram();
        self.start_picker()?;
        self.eyedropper.layer = false;
        let page=match self.state.layer_properties.page.as_deref() {Some("red")=>1,Some("green")=>2,Some("blue")=>3,_=>0};
        self.eyedropper.calibration = Some(Calibration { role, page, original, epoch, document_epoch: self.state.document_file.epoch,
            width: 5, request: None, queued: None, submitted: false });
        self.eyedropper.cancel();
        self.layer_interaction.tool = LayerCanvasTool::PickVisible;
        self.state.layer_tools.tool = LayerCanvasTool::PickVisible;
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn queue_calibration(&mut self, position: [f32; 2]) {
        if self.eyedropper.picking.finishing && self.eyedropper.calibration.as_ref().is_some_and(|calibration| calibration.submitted) {
            self.engine.backend_mut().cancel_snapshot();
            self.eyedropper.calibration.as_mut().unwrap().submitted = false;
        }
        let Some(calibration) = self.eyedropper.calibration.as_mut() else { return; };
        if !self.eyedropper.picking.finishing {
            if self.eyedropper.picking.touch.is_none() { return; }
            if calibration.request.as_ref().is_some_and(|request| request.position == position) { return; }
            if calibration.submitted { calibration.queued = Some(position); return; }
        }
        calibration.queued = None;
        let mut request = ArtworkSampleRequest::new(self.engine.document(), ArtworkSource::EffectInput(calibration.original.occurrence), position, calibration.width);
        Arc::make_mut(&mut request.query.snapshot).context.elapsed = self.engine.animation_time();
        calibration.request = Some(request);
    }

    pub(super) fn poll_calibration(&mut self) -> u32 {
        let Some(mut calibration) = self.eyedropper.calibration.take() else { return 0; };
        let current = self.engine.document().working.occurrence == Some(calibration.original.occurrence)
            && self.state.document_file.epoch == calibration.document_epoch
            && self.property_editor.accepts(occurrence_token(calibration.original.occurrence), calibration.epoch)
            && calibration.request.as_ref().is_none_or(|request| request.matches_artwork(self.engine.document()));
        if !current {
            if calibration.submitted { self.engine.backend_mut().cancel_snapshot(); }
            self.cancel_picker();
            return regions::BRUSH | regions::COMMANDS | regions::COLOR_PREVIEW;
        }
        let completed = if calibration.submitted { self.engine.backend_mut().take_snapshot() } else { None };
        let result = if let Some(result) = completed {
            calibration.submitted = false;
            calibration.request = None;
            if !self.eyedropper.picking.finishing {
                self.state.color_picker.preview = match result {
                    Ok(layer_render::SnapshotResult::ArtworkSample(ArtworkSample::Color([r, g, b, _]))) =>
                        layer_core::color::RgbColor::from_linear(self.engine.document().composition().color.space, [r, g, b, 1.]).ok(),
                    _ => None,
                };
                let next = calibration.queued.take();
                self.eyedropper.calibration = Some(calibration);
                if let Some(position) = next { self.queue_calibration(position); }
                return regions::COLOR_PREVIEW;
            }
            Some(result.map_err(error).map_err(CalibrationFailure::Diagnostic).and_then(|result| {
                let layer_render::SnapshotResult::ArtworkSample(sample) = result else { return Err(CalibrationFailure::Diagnostic("Unexpected artwork sample".into())); };
                let ArtworkSample::Color([r, g, b, _]) = sample else {
                    return Err(CalibrationFailure::Message(MessageId::RESOURCES_PICKER_EMPTY));
                };
                let original = effects::effect_draft(self.engine.document(), calibration.original.occurrence)?;
                let mut effect = original.clone();
                effect.program = effect.program.for_depth(self.engine.document().composition().color.depth);
                if effect.program.id.as_ref() == "curves" {
                    effect = layer_core::curves::calibrate_curves(&effect,[r,g,b],self.engine.document().composition().color.space,calibration.page,calibration.role)
                        .map_err(|_|CalibrationFailure::Message(MessageId::RESOURCES_CALIBRATION_FAILED))?;
                } else if effect.program.id.as_ref() == "levels" {
                    effect = layer_core::levels::calibrate_levels(&effect,[r,g,b],self.engine.document().composition().color.space,calibration.page,calibration.role)
                        .map_err(|_|CalibrationFailure::Message(MessageId::RESOURCES_CALIBRATION_FAILED))?;
                } else {
                    let preserve = effect.value("preserve_luminance") == Some(&EffectValue::Toggle(true));
                    let values = layer_core::white_balance_neutral([r,g,b],self.engine.document().composition().color.space,preserve)
                        .map_err(|_|CalibrationFailure::Message(MessageId::RESOURCES_PICKER_NEUTRAL_FAILED))?;
                    effect.set("temperature", EffectValue::Number(values[0])).map_err(str::to_string)?;
                    effect.set("tint", EffectValue::Number(values[1])).map_err(str::to_string)?;
                }
                if effect != original { self.layer_edit(effects::effect_edit(self.engine.document(),calibration.original.occurrence,effect)?)?; }
                Ok(())
            }))
        } else if !calibration.submitted && let Some(request) = &calibration.request {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::ArtworkSample(request.clone())).map_err(error) {
                Ok(submitted) => { calibration.submitted = submitted; None }
                Err(error) => Some(Err(CalibrationFailure::Diagnostic(error))),
            }
        } else { None };
        match result {
            Some(Ok(())) => regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::COLOR_PREVIEW,
            Some(Err(reason)) => {
                self.eyedropper.picking.finishing = false;
                calibration.request = None;
                self.eyedropper.calibration = Some(calibration);
                match reason {
                    CalibrationFailure::Message(message) => self.raise_message_notice(message),
                    CalibrationFailure::Diagnostic(reason) => self.raise_notice(reason, None),
                }
                regions::BRUSH | regions::COMMANDS | regions::COLOR_PREVIEW
            }
            None => { self.eyedropper.calibration = Some(calibration); 0 }
        }
    }
}

pub(crate) struct AutoLevels {
    original:EffectBaseline,
    query:layer_core::ArtworkQuery,
    epoch:u64,
    document_epoch:u64,
    page:u8,
    submitted:bool,
}
impl<R:CanvasRenderer> UiSession<R> {
    pub(super) fn cancel_auto_levels(&mut self) {
        if self.auto_levels.take().is_some_and(|task|task.submitted) {self.engine.backend_mut().cancel_snapshot();}
    }
    pub(super) fn start_auto_levels(&mut self,layer:u64,epoch:u64)->Result<(),String> {
        if !Panel::Histogram.available_on(self.state.platform) {return Ok(());}
        if self.auto_levels.is_some() {self.cancel_auto_levels();self.refresh_document();return Ok(());}
        self.require_idle()?;
        let document=self.engine.document();
        let handle = occurrence_handle(layer)?;
        if document.is_locked(handle) || document.scene().effect(handle).is_none_or(|effect| effect.program.id.as_ref() != "levels") { return Err("Choose a Levels adjustment".into()); }
        let original = effects::effect_baseline(document, handle)?;
        let page=match self.state.layer_properties.page.as_deref() {Some("red")=>1,Some("green")=>2,Some("blue")=>3,_=>0};
        let mut query=layer_core::ArtworkQuery::new(document,if page==0 {ArtworkSource::EffectChannels(original.occurrence)} else {ArtworkSource::EffectInput(original.occurrence)});
        Arc::make_mut(&mut query.snapshot).context.elapsed=self.engine.animation_time();query.validate()?;
        self.cancel_picker();self.cancel_histogram();
        self.auto_levels=Some(AutoLevels {original,query,epoch,document_epoch:self.state.document_file.epoch,page,submitted:false});
        self.refresh_document();Ok(())
    }
    pub(super) fn poll_auto_levels(&mut self)->u32 {
        let Some(mut task)=self.auto_levels.take() else {return 0;};
        let current=self.state.document_file.epoch==task.document_epoch && self.engine.document().working.occurrence==Some(task.original.occurrence)
            && self.property_editor.accepts(occurrence_token(task.original.occurrence),task.epoch) && task.query.matches_artwork(self.engine.document())
            && self.panel_is_presented(Panel::Properties) && !self.rendering_suspended;
        if !current {
            if task.submitted {self.engine.backend_mut().cancel_snapshot();}
            self.refresh_document();return regions::DOCUMENT;
        }
        let outcome=if task.submitted {
            self.engine.backend_mut().take_snapshot().map(|result|result.map_err(error).map_err(CalibrationFailure::Diagnostic).and_then(|result| {
                let layer_render::SnapshotResult::LevelsStatistics(statistics)=result else {return Err(CalibrationFailure::Diagnostic("Unexpected Auto statistics".into()));};
                let original = effects::effect_draft(self.engine.document(), task.original.occurrence)?;
                let mut effect = original.clone();
                effect.program = effect.program.for_depth(self.engine.document().composition().color.depth);
                let candidate = layer_core::levels::auto_levels(&effect,&statistics,task.page).map_err(|_|CalibrationFailure::Message(MessageId::RESOURCES_LEVELS_AUTO_FAILED))?;
                if candidate != original { self.layer_edit(effects::effect_edit(self.engine.document(),task.original.occurrence,candidate)?)?; }
                Ok(())
            }))
        } else {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::LevelsStatistics(task.query.clone())) {
                Ok(submitted)=>{task.submitted=submitted;None},Err(reason)=>Some(Err(CalibrationFailure::Diagnostic(error(reason)))),
            }
        };
        match outcome {
            None=>{self.auto_levels=Some(task);0},
            Some(result)=>{if let Err(reason)=result {match reason {
                CalibrationFailure::Message(message)=>self.raise_message_notice(message),
                CalibrationFailure::Diagnostic(reason)=>self.raise_notice(reason,None),
            }}self.refresh_document();regions::DOCUMENT},
        }
    }
}
