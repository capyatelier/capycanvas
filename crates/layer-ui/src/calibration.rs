use super::*;
use layer_core::{ArtworkSample, ArtworkSampleRequest, ArtworkSource, EffectValue, Layer};
use std::sync::Arc;

pub(crate) struct Calibration {
    pub original: Layer,
    pub epoch: u64,
    pub document_epoch: u64,
    pub width: u32,
    pub request: Option<ArtworkSampleRequest>,
    pub queued: Option<[f32; 2]>,
    pub submitted: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn start_white_balance_picker(&mut self, layer: u64, epoch: u64) -> Result<(), String> {
        if self.state.platform != Platform::Gtk { return Err(self.localization().text(MessageId::RESOURCES_PICKER_UNAVAILABLE).to_string()); }
        if !self.property_editor.accepts(layer, epoch) { return Ok(()); }
        self.require_idle()?;
        let doc = self.engine.document();
        let original = doc.layer(LayerId(layer)).filter(|layer| !doc.is_locked(layer.id)
            && layer.effect.as_ref().is_some_and(|effect| effect.program.id.as_ref() == "white_balance"))
            .ok_or_else(|| self.localization().text(MessageId::RESOURCES_ERROR_ARTWORK_REQUIRED).to_string())?.clone();
        self.cancel_picker();
        self.start_picker()?;
        self.eyedropper.layer = false;
        self.eyedropper.calibration = Some(Calibration { original, epoch, document_epoch: self.state.document_file.epoch,
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
                let mut layer = calibration.original.clone();
                let effect = Arc::make_mut(layer.effect.as_mut().ok_or("Missing adjustment")?);
                let preserve = effect.value("preserve_luminance") == Some(&EffectValue::Toggle(true));
                let values = layer_core::white_balance_neutral([r, g, b], self.engine.document().color.space, preserve)
                    .map_err(|_| self.localization().text(MessageId::RESOURCES_PICKER_NEUTRAL_FAILED).to_string())?;
                effect.set("temperature", EffectValue::Number(values[0])).map_err(str::to_string)?;
                effect.set("tint", EffectValue::Number(values[1])).map_err(str::to_string)?;
                self.cancel_picker();
                if layer != calibration.original { self.layer_edit(layer_core::Edit::ReplaceLayer(Box::new(layer)))?; }
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
