use super::{InstantFeedbackConfig, PredictionState, TipSource};
use crate::brush::{
    CurvePreview, DabGenerator, LiftPreview, PositionField, has_completions, lock_dab_tail,
    taper_prediction,
};
use layer_core::{BrushSnapshot, StrokePoint};
use layer_render::Dab;

#[derive(Clone, Debug, Default)]
pub(crate) struct PreviewTip {
    pub prediction: PredictionState,
    pub curve_preview: CurvePreview,
    pub preview_taper: bool,
    pub frame_lift: Option<LiftPreview>,
}

pub(crate) struct TipInput<'a> {
    pub real: &'a [StrokePoint],
    pub pending: &'a [StrokePoint],
    pub platform: &'a [StrokePoint],
    pub brush: &'a BrushSnapshot,
    pub feedback: InstantFeedbackConfig,
    pub transform: [f32; 6],
    pub now: u32,
    pub requested: u32,
    pub ending: Option<u32>,
}

pub(crate) fn tip_times(feedback: InstantFeedbackConfig, latest: StrokePoint, elapsed: impl Fn(u64) -> u32,
    timestamp_ns: Option<u64>, presentation_timestamp_ns: Option<u64>) -> (u32, u32) {
    let fallback = timestamp_ns.map(|timestamp| timestamp.saturating_add(u64::from(feedback.prediction_horizon_micros).saturating_mul(1_000)));
    let requested = presentation_timestamp_ns.filter(|_| feedback.use_platform_prediction).or(fallback).map(&elapsed)
        .unwrap_or_else(|| latest.elapsed_micros.saturating_add(feedback.prediction_horizon_micros));
    (timestamp_ns.map(elapsed).unwrap_or(latest.elapsed_micros), requested)
}

impl PreviewTip {
    pub fn build(&mut self, generator: &DabGenerator, input: TipInput<'_>, start: usize, output: &mut Vec<Dab>) -> Option<TipSource> {
        let now = if input.ending.is_some() {
            input.real.last()?.elapsed_micros
        } else {
            input.now
        };
        let lift = self.frame_lift.take();
        generator.preview_lift(
            input.brush,
            input.pending,
            input.ending.is_some(),
            now,
            lift,
            start,
            output,
            |tip, output| self.build_main(generator, tip, &input, start, output),
        )
    }

    fn build_main(
        &mut self,
        generator: &DabGenerator,
        tip: &mut DabGenerator,
        input: &TipInput<'_>,
        start: usize,
        output: &mut Vec<Dab>,
    ) -> Option<TipSource> {
        if !input.feedback.enabled {
            tip.flush_contact(input.brush, output);
            return None;
        }
        let latest = *input.real.last()?;
        let estimate = if input.ending.is_none() {
            self.prediction.estimate_for(input.real, input.platform, input.requested, input.now, input.transform, input.feedback)
        } else { None };
        let predicted: Vec<_> = if input.ending.is_none() {
            let points = estimate.filter(|_| input.feedback.enabled).map_or_else(Vec::new, |estimate| {
                self.prediction.preview_points(latest, input.platform, estimate, input.transform).collect()
            });
            self.preview_taper = estimate.is_some_and(|estimate| estimate.source == TipSource::Engine);
            points
        } else { Vec::new() };
        let has_field = if let Some(model) = generator.preview_model(input.brush, input.real) {
            if let Some(ready) = input.ending {
                self.curve_preview.land(input.now, ready, model);
            } else {
                self.curve_preview.observe(input.now, model, &input.brush.stabilization, &predicted);
            }
            self.curve_preview.is_present()
        } else { false };
        let field = has_field.then_some(&self.curve_preview as &dyn PositionField);
        let prediction_start = output.len();
        let mut previous = generator.preview_previous();
        for point in input.pending.iter().copied() {
            tip.append_preview(point, input.brush, field, &mut previous, output);
        }
        if has_completions(input.brush) {
            tip.flush_contact(input.brush, output);
        }
        if has_field {
            let extra_start = output.len();
            let taper_start = tip.modeled_position().unwrap_or(latest.position);
            if let Some(point) = self.curve_preview.endpoint(latest) {
                tip.append_continuation(point, input.brush, &self.curve_preview, output);
            }
            tip.flush_contact(input.brush, output);
            if let Some(endpoint) = tip.modeled_position() {
                tip.reach_endpoint(endpoint, start, self.preview_taper, input.brush, output);
            }
            if self.preview_taper {
                taper_prediction(&mut output[extra_start..], taper_start, self.curve_preview.taper_amount());
            }
            return estimate.filter(|_| input.ending.is_none()).map(|estimate| estimate.source);
        }
        let Some(estimate) = estimate.filter(|_| input.feedback.enabled) else {
            tip.flush_contact(input.brush, output);
            return None;
        };
        let predicted_dab_start = output.len();
        let taper_start = tip.modeled_position().unwrap_or(latest.position);
        let taper = estimate.source == TipSource::Engine;
        for point in predicted {
            tip.append_preview(point, input.brush, field, &mut previous, output);
        }
        // Flush the actual swept endpoint just as finalization does. Coverage
        // by an earlier, wider contact is not the final pressure/pose.
        tip.flush_contact(input.brush, output);
        let modeled_endpoint = tip.modeled_position().unwrap_or(latest.position);
        let endpoint = estimate.point.position;
        if input.brush.stabilization.strength == 0. {
            lock_dab_tail(&mut output[prediction_start..], modeled_endpoint, endpoint);
            tip.reach_endpoint(endpoint, start, taper, input.brush, output);
        }
        if taper {
            taper_prediction(&mut output[predicted_dab_start..], taper_start, 1.);
        }
        (output.len() > start).then_some(estimate.source)
    }
}
