use super::*;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, EffectValue, Layer};
use std::sync::Arc;

pub(crate) struct Calibration {
    pub role:layer_core::levels::CalibrationRole,
    pub page:u8,
    pub original: Layer,
    pub epoch: u64,
    pub document_epoch: u64,
    pub width: u32,
    pub request: Option<ArtworkSampleRequest>,
    pub queued: Option<[f32; 2]>,
    pub submitted: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn start_calibration(&mut self, layer: u64, epoch: u64, role:layer_core::levels::CalibrationRole) -> Result<(), String> {
        if self.state.platform != Platform::Gtk { return Err(self.localization().text(MessageId::RESOURCES_PICKER_UNAVAILABLE).to_string()); }
        self.require_idle()?;
        let doc = self.engine.document();
        let original = doc.layer(LayerId(layer)).filter(|layer| !doc.is_locked(layer.id)
            && layer.effect.as_ref().is_some_and(|effect| matches!(effect.program.id.as_ref(),"white_balance"|"levels"|"curves")))
            .ok_or_else(|| self.localization().text(MessageId::RESOURCES_ERROR_ARTWORK_REQUIRED).to_string())?.clone();
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
        let mut request = ArtworkSampleRequest::new(self.engine.document(), ArtworkSource::EffectInput(calibration.original.id), position, calibration.width);
        request.time = self.engine.animation_time();
        calibration.request = Some(request);
    }

    pub(super) fn poll_calibration(&mut self) -> u32 {
        let Some(mut calibration) = self.eyedropper.calibration.take() else { return 0; };
        let current = self.engine.document().active_layer == calibration.original.id
            && self.state.document_file.epoch == calibration.document_epoch
            && self.property_editor.accepts(calibration.original.id.0, calibration.epoch)
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
                        layer_core::color::RgbColor::from_linear(self.engine.document().color.space, [r, g, b, 1.]).ok(),
                    _ => None,
                };
                let next = calibration.queued.take();
                self.eyedropper.calibration = Some(calibration);
                if let Some(position) = next { self.queue_calibration(position); }
                return regions::COLOR_PREVIEW;
            }
            Some(result.map_err(error).and_then(|result| {
                let layer_render::SnapshotResult::ArtworkSample(sample) = result else { return Err("Unexpected artwork sample".into()); };
                let ArtworkSample::Color([r, g, b, _]) = sample else {
                    return Err(self.localization().text(MessageId::RESOURCES_PICKER_EMPTY).to_string());
                };
                let original=self.engine.document().layer(calibration.original.id).ok_or("The adjustment was removed")?.clone();
                let mut layer = original.clone();
                let effect = Arc::make_mut(layer.effect.as_mut().ok_or("Missing adjustment")?);
                effect.program=effect.program.for_depth(self.engine.document().color.depth);
                if effect.program.id.as_ref()=="curves" {
                    *effect=layer_core::curves::calibrate_curves(effect,[r,g,b],self.engine.document().color.space,calibration.page,calibration.role)
                        .map_err(|_|self.localization().text(MessageId::RESOURCES_CALIBRATION_FAILED).to_string())?;
                } else if effect.program.id.as_ref()=="levels" {
                    *effect=layer_core::levels::calibrate_levels(effect,[r,g,b],self.engine.document().color.space,calibration.page,calibration.role)
                        .map_err(|_|self.localization().text(MessageId::RESOURCES_CALIBRATION_FAILED).to_string())?;
                } else {
                    let preserve = effect.value("preserve_luminance") == Some(&EffectValue::Toggle(true));
                    let values = layer_core::white_balance_neutral([r, g, b], self.engine.document().color.space, preserve)
                        .map_err(|_| self.localization().text(MessageId::RESOURCES_PICKER_NEUTRAL_FAILED).to_string())?;
                    effect.set("temperature", EffectValue::Number(values[0])).map_err(str::to_string)?;
                    effect.set("tint", EffectValue::Number(values[1])).map_err(str::to_string)?;
                }
                self.cancel_picker();
                if layer != original { self.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer)))?; }
                Ok(())
            }))
        } else if !calibration.submitted && let Some(request) = &calibration.request {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::ArtworkSample(request.clone())).map_err(error) {
                Ok(submitted) => { calibration.submitted = submitted; None }
                Err(error) => Some(Err(error)),
            }
        } else { None };
        match result {
            Some(Ok(())) => regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::COLOR_PREVIEW,
            Some(Err(reason)) => {
                self.eyedropper.picking.finishing = false;
                calibration.request = None;
                self.eyedropper.calibration = Some(calibration);
                self.raise_notice(reason, None);
                regions::BRUSH | regions::COMMANDS | regions::COLOR_PREVIEW
            }
            None => { self.eyedropper.calibration = Some(calibration); 0 }
        }
    }
}

pub(crate) struct AutoLevels {
    original:Layer,
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
        if self.state.platform!=Platform::Gtk {return Ok(());}
        if self.auto_levels.is_some() {self.cancel_auto_levels();self.refresh_document();return Ok(());}
        self.require_idle()?;
        let document=self.engine.document();
        let original=document.layer(LayerId(layer)).filter(|l|!document.is_locked(l.id)
            && l.effect.as_ref().is_some_and(|e|e.program.id.as_ref()=="levels")).ok_or("Choose a Levels adjustment")?.clone();
        let page=match self.state.layer_properties.page.as_deref() {Some("red")=>1,Some("green")=>2,Some("blue")=>3,_=>0};
        let mut query=layer_core::ArtworkQuery::new(document,if page==0 {ArtworkSource::EffectChannels(original.id)} else {ArtworkSource::EffectInput(original.id)});
        query.time=self.engine.animation_time();query.validate()?;
        self.cancel_picker();self.cancel_histogram();
        self.auto_levels=Some(AutoLevels {original,query,epoch,document_epoch:self.state.document_file.epoch,page,submitted:false});
        self.refresh_document();Ok(())
    }
    pub(super) fn poll_auto_levels(&mut self)->u32 {
        let Some(mut task)=self.auto_levels.take() else {return 0;};
        let current=self.state.document_file.epoch==task.document_epoch && self.engine.document().active_layer==task.original.id
            && self.property_editor.accepts(task.original.id.0,task.epoch) && task.query.matches_artwork(self.engine.document())
            && self.panel_is_presented(Panel::Properties) && !self.rendering_suspended;
        if !current {
            if task.submitted {self.engine.backend_mut().cancel_snapshot();}
            self.refresh_document();return regions::DOCUMENT;
        }
        let outcome=if task.submitted {
            self.engine.backend_mut().take_snapshot().map(|result|result.map_err(error).and_then(|result| {
                let layer_render::SnapshotResult::LevelsStatistics(statistics)=result else {return Err("Unexpected Auto statistics".into());};
                let original=self.engine.document().layer(task.original.id).ok_or("The adjustment was removed")?.clone();
                let mut layer=original.clone();
                let mut effect=(**layer.effect.as_ref().ok_or("Missing adjustment")?).clone();
                effect.program=effect.program.for_depth(self.engine.document().color.depth);
                let candidate=layer_core::levels::auto_levels(&effect,&statistics,task.page).map_err(|_|self.localization().text(MessageId::RESOURCES_LEVELS_AUTO_FAILED).to_string())?;
                layer.effect=Some(Arc::new(candidate));
                if layer!=original {self.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer)))?;}Ok(())
            }))
        } else {
            match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::LevelsStatistics(task.query.clone())) {
                Ok(submitted)=>{task.submitted=submitted;None},Err(reason)=>Some(Err(error(reason))),
            }
        };
        match outcome {
            None=>{self.auto_levels=Some(task);0},
            Some(result)=>{if let Err(reason)=result {self.raise_notice(reason,None);}self.refresh_document();regions::DOCUMENT},
        }
    }
}
