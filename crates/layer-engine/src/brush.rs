//! Deterministic brush dynamics compiled into renderer-ready dabs.
//!
//! The sequential, low-volume work stays here on the CPU: input-derived
//! sensors, curve evaluation, swept-path simplification, stamp resampling, and
//! deterministic variation. Render backends receive only resolved, fixed-size dab geometry.

use layer_core::color::RgbSpace;
use layer_core::{
    BrushCombine, BrushMapping, BrushSensor, BrushSnapshot, BrushTarget, MAX_BRUSH_DIAMETER,
    MAX_BRUSH_MAPPINGS, MAX_BRUSH_SCATTER_DIAMETERS, Point, Rect, Stroke, StrokeId, StrokePoint,
};
use layer_render::Dab;
use std::sync::{Arc, LazyLock};

mod smoothing;
mod completion;
pub(crate) use smoothing::{CurveModel, PositionField, CurvePreview, pen_up_settlement_micros};

const SPEED_FILTER_SECONDS: f32 = 0.015;
const PRESSURE_FALL_RESPONSE_SECONDS: f32 = 0.004;
const BARREL_TWIST_RESPONSE_SECONDS: f32 = 0.025;
const MIN_DAB_DISTANCE: f32 = 0.25;
const TAP_DISTANCE_PX: f32 = 1.;
const MAX_DABS_PER_SEGMENT: usize = 65_536;
/// Power applied to the smooth endpoint-correction envelope.
const CORRECTION_EASING: f32 = 1.5;
const MAX_FAN_ARCS: f32 = 64.0;
const FAN_TURN_RADIANS: f64 = 0.05;
const FAN_LIFT: f32 = 2.;
const MAX_FAN_PENDING: usize = 512;
static EMPTY_CONTACTS: LazyLock<Arc<Vec<Dab>>> = LazyLock::new(Default::default);

#[derive(Clone, Copy, Debug)]
struct DynamicPoint {
    point: StrokePoint,
    speed: f32,
    direction_turns: f32,
    stroke_distance: f32,
}

#[derive(Clone, Copy, Debug)]
struct Registers {
    diameter: f32,
    opacity: f32,
    flow: f32,
    hardness: f32,
    spacing: f32,
    aspect: f32,
    rotation: f32,
    scatter_along: f32,
    scatter_across: f32,
    hue: f32,
    saturation: f32,
    lightness: f32,
    secondary_color: f32,
    grain_depth: f32,
    pull: f32,
    deposit: f32,
    deform_strength: f32,
}

#[derive(Clone, Copy, Debug)]
struct EvaluatedDab {
    dab: Dab,
    spacing: f32,
    direction_radians: f32,
    values: Registers,
}

/// Stateful emitter for one stroke. Reset it with the committed stroke ID so
/// live rendering and transient stroke correction produce the same variation.
/// Colors are straight linear document RGB; HSL dynamics use its encoded RGB.
#[derive(Clone, Debug)]
pub struct DabGenerator {
    space: RgbSpace,
    last: Option<DynamicPoint>,
    stabilized_input: Option<StrokePoint>,
    position_smoother: smoothing::PositionSmoother,
    pressure_fall_target: f32,
    bristle_azimuth: Option<f32>,
    barrel_twist: bool,
    fan_slide: f32,
    edge_normal: [f32; 2],
    touchdown: Point,
    fan_roll: f32,
    fan_axis: [f32; 2],
    fan_heading: [f32; 2],
    fan_dragging: bool,
    fan_deepest: f32,
    fan_pressed: f32,
    pending_contacts: Arc<Vec<Dab>>,
    dry_completion: Option<completion::Path>,
    tap_peak: Option<DynamicPoint>,
    distance_until_next: f32,
    filtered_speed: f32,
    rng: u32,
    stroke_seed: u32,
    dab_index: u32,
    last_evaluated: Option<DynamicPoint>,
    last_contact: Option<Dab>,
    total_distance: Option<f32>,
    continuous_fraction: f32,
}

impl Default for DabGenerator {
    fn default() -> Self {
        Self {
            space: RgbSpace::Srgb,
            last: None,
            stabilized_input: None,
            position_smoother: smoothing::PositionSmoother::default(),
            pressure_fall_target: 0.0,
            bristle_azimuth: None,
            barrel_twist: false,
            fan_slide: 0.,
            edge_normal: [0.; 2],
            touchdown: Point { x: 0., y: 0. },
            fan_roll: 0.,
            fan_axis: [0.; 2],
            fan_heading: [0.; 2],
            fan_dragging: false,
            fan_deepest: 0.,
            fan_pressed: 0.,
            pending_contacts: EMPTY_CONTACTS.clone(),
            dry_completion: Some(Default::default()),
            tap_peak: None,
            distance_until_next: 0.0,
            filtered_speed: 0.0,
            rng: 1,
            stroke_seed: 1,
            dab_index: 0,
            last_evaluated: None,
            last_contact: None,
            total_distance: None,
            continuous_fraction: 0.0,
        }
    }
}

impl DabGenerator {
    pub fn new(space: RgbSpace) -> Self {
        Self {
            space,
            ..Self::default()
        }
    }

    pub(crate) fn set_space(&mut self, space: RgbSpace) {
        if self.space != space {
            *self = Self::new(space);
        }
    }
    pub(crate) fn cursor_seed(&mut self, id: StrokeId, brush: &BrushSnapshot) {
        self.rng = mix_seed(brush.seed, id.0);
        self.stroke_seed = self.rng;
    }
    /// Resolve a cursor contact through the painting dynamics without advancing
    /// spacing, random state or committed input. Hover updates sensors only.
    pub fn cursor_contacts(&mut self, point: StrokePoint, brush: &BrushSnapshot) -> Vec<Dab> {
        let current = self.characterize(point);
        let mut preview = self.preview();
        let evaluated = preview.evaluate(current, brush);
        let mut output = Vec::new();
        let mut damage = Rect::EMPTY;
        if brush.execution == layer_core::BrushExecution::Liquify { output.push(evaluated.dab); }
        else { preview.emit_contacts(evaluated, brush, &mut output, &mut damage); }
        if brush.contact.is_some_and(|c| c.bristles.is_some()) {
            output = output.last().or(preview.pending_contacts.last()).copied().into_iter().collect();
            for fan in &mut output {
                fan.texture_sign = [1.; 2];
            }
        }
        self.bristle_azimuth = preview.bristle_azimuth;
        self.last = Some(current);
        output
    }
    pub fn reset(&mut self) {
        *self = Self::new(self.space);
    }

    pub fn reset_for_stroke(&mut self, stroke_id: StrokeId, brush: &BrushSnapshot) {
        let pending_contacts = std::mem::replace(&mut self.pending_contacts, EMPTY_CONTACTS.clone());
        *self = Self { pending_contacts, ..Self::new(self.space) };
        if !self.pending_contacts.is_empty() { Arc::make_mut(&mut self.pending_contacts).clear(); }
        self.cursor_seed(stroke_id, brush);
    }

    pub(crate) fn preview(&self) -> Self {
        let mut preview = self.clone();
        preview.dry_completion = None;
        if preview.tap_peak.take().is_some() {
            preview.last_contact = self.pending_contacts.first().copied();
            preview.pending_contacts = EMPTY_CONTACTS.clone();
        }
        preview
    }

    /// Whether this stroke's twist is a measured barrel angle rather than a
    /// constant from a pen without a rotation sensor.
    pub fn set_barrel_twist(&mut self, measured: bool) {
        self.barrel_twist = measured;
    }

    pub fn reset_for_replay(&mut self, stroke: &Stroke) {
        self.reset_for_stroke(stroke.id, &stroke.brush);
        self.barrel_twist = stroke.barrel_twist;
        self.total_distance = Some(
            stroke
                .points
                .windows(2)
                .map(|pair| {
                    let dx = pair[1].position.x - pair[0].position.x;
                    let dy = pair[1].position.y - pair[0].position.y;
                    dx.hypot(dy)
                })
                .sum(),
        );
    }

    pub fn append(
        &mut self,
        point: StrokePoint,
        brush: &BrushSnapshot,
        output: &mut Vec<Dab>,
    ) -> Rect {
        self.append_with_field(point, brush, None, output)
    }

    pub(crate) fn preview_model(&self, brush: &BrushSnapshot, real: &[StrokePoint]) -> Option<CurveModel> {
        self.position_smoother.preview_model(&brush.stabilization, real)
    }

    pub(crate) fn append_with_field(
        &mut self,
        point: StrokePoint,
        brush: &BrushSnapshot,
        field: Option<&dyn PositionField>,
        output: &mut Vec<Dab>,
    ) -> Rect {
        if brush.execution == layer_core::BrushExecution::Liquify {
            if brush.deform.strength <= 0. { return Rect::EMPTY; }
        } else if brush.flow <= 0. || brush.opacity <= 0. {
            return Rect::EMPTY;
        }
        let previous = self.stabilized_input;
        let position = field.map(|field| field.position(f64::from(point.elapsed_micros) * 1e-6));
        let stabilized = self.stabilize(point, brush, position);
        let mut damage = Rect::EMPTY;
        if let Some(previous) = previous.filter(|previous| previous.elapsed_micros < point.elapsed_micros)
            && brush.stabilization.strength > 0.
        {
            let begin = f64::from(previous.elapsed_micros) * 1e-6;
            let end = f64::from(point.elapsed_micros) * 1e-6;
            let tolerance = 0.05 / f64::from(brush.stabilization.input_scale).max(1e-6);
            damage = damage.union(self.append_curve_interval(previous, stabilized, begin, end, field, tolerance, brush, output));
        }
        damage = damage.union(self.append_modeled(stabilized, brush, output));
        self.position_smoother.retain_from(point.elapsed_micros);
        damage
    }

    #[allow(clippy::too_many_arguments)]
    fn append_curve_interval(&mut self, previous: StrokePoint, current: StrokePoint, begin: f64, end: f64,
        field: Option<&dyn PositionField>, tolerance: f64, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        let bound = field.map_or_else(|| self.position_smoother.acceleration_bound(begin, end),
            |field| field.acceleration_bound(begin, end));
        if (end - begin) <= 1e-6 || bound * (end - begin).powi(2) <= 8. * tolerance { return Rect::EMPTY; }
        let middle = (begin + end) * 0.5;
        let mut damage = self.append_curve_interval(previous, current, begin, middle, field, tolerance, brush, output);
        let duration = current.elapsed_micros.saturating_sub(previous.elapsed_micros);
        let fraction = if duration == 0 { 0. } else { ((middle * 1e6 - f64::from(previous.elapsed_micros)) / f64::from(duration)) as f32 };
        let mut sample = interpolate_point(previous, current, fraction);
        sample.position = field.map_or_else(|| self.position_smoother.curve_position(middle).unwrap(), |field| field.position(middle));
        damage = damage.union(self.append_modeled(sample, brush, output));
        damage.union(self.append_curve_interval(previous, current, middle, end, field, tolerance, brush, output))
    }

    pub(crate) fn append_continuation(&mut self, endpoint: StrokePoint, brush: &BrushSnapshot, field: &dyn PositionField, output: &mut Vec<Dab>) -> Rect {
        let Some(previous) = self.stabilized_input else { return Rect::EMPTY; };
        let begin = f64::from(previous.elapsed_micros) * 1e-6;
        let end = f64::from(endpoint.elapsed_micros) * 1e-6;
        if end <= begin { return Rect::EMPTY; }
        let endpoint = StrokePoint { position: endpoint.position, ..previous };
        let tolerance = 0.05 / f64::from(brush.stabilization.input_scale).max(1e-6);
        let damage = self.append_curve_interval(previous, endpoint, begin, end, Some(field), tolerance, brush, output);
        self.stabilized_input = Some(endpoint);
        damage.union(self.append_modeled(endpoint, brush, output))
    }

    fn append_modeled(&mut self, point: StrokePoint, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        let current = self.characterize(point);

        let mut damage = Rect::EMPTY;
        let Some(last) = self.last else {
            if brush.contact.is_some_and(|c| c.dry.is_some()) {
                self.touchdown = current.position();
                self.tap_peak = Some(current);
            }
            let evaluated = self.evaluate(current, brush);
            self.emit_contacts(evaluated, brush, output, &mut damage);
            self.last = Some(current);
            if brush.contact.is_none() {
                self.distance_until_next = spacing_for(evaluated.dab, evaluated.spacing);
            }
            return damage;
        };

        if let Some(peak) = &mut self.tap_peak {
            let travel = (current.position().x-self.touchdown.x).hypot(current.position().y-self.touchdown.y);
            if current.point.pressure > peak.point.pressure { *peak = current; }
            if travel * brush.stabilization.input_scale > TAP_DISTANCE_PX {
                self.tap_peak = None;
                let start = output.len();
                output.extend(Arc::make_mut(&mut self.pending_contacts).drain(..).skip(1));
                damage = damage_for_dabs(&output[start..]);
            }
        }

        let dx = current.point.position.x - last.point.position.x;
        let dy = current.point.position.y - last.point.position.y;
        let distance = dx.hypot(dy);
        if distance <= f32::EPSILON {
            if brush.contact.is_some_and(|c| c.bristles.is_some()) {
                // A held brush can splay or roll without changing its center.
                self.append_swept(last, current, brush, output, &mut damage);
            }
            if brush.path.continuous_rate_hz > 0.0 {
                let elapsed_micros = current
                    .point
                    .elapsed_micros
                    .saturating_sub(last.point.elapsed_micros);
                let requested = self.continuous_fraction
                    + elapsed_micros as f32 / 1_000_000.0 * brush.path.continuous_rate_hz;
                let count = requested.floor().min(MAX_DABS_PER_SEGMENT as f32) as usize;
                self.continuous_fraction = requested - count as f32;
                for _ in 0..count {
                    let evaluated = self.evaluate(current, brush);
                    self.emit_contacts(evaluated, brush, output, &mut damage);
                }
            }
            self.last = Some(current);
            return damage;
        }
        self.continuous_fraction = 0.0;

        if brush.contact.is_some() {
            self.append_swept(last, current, brush, output, &mut damage);
            self.last = Some(current);
            return damage;
        }

        let mut traveled = self.distance_until_next;
        let mut emitted = 0;
        while traveled <= distance && emitted < MAX_DABS_PER_SEGMENT {
            let sample = interpolate(last, current, traveled / distance);
            let evaluated = self.evaluate(sample, brush);
            self.emit_contacts(evaluated, brush, output, &mut damage);
            traveled += spacing_for(evaluated.dab, evaluated.spacing);
            emitted += 1;
        }
        self.distance_until_next = (traveled - distance).max(0.0);
        self.last = Some(current);
        damage
    }

    /// Swept contacts already cover the space between poses. Keep modeled input
    /// vertices only when their path or pose matters, instead of inserting stamps
    /// at a pressure-dependent distance. Decisions depend on input, not frames.
    fn append_swept(
        &mut self,
        last: DynamicPoint,
        current: DynamicPoint,
        brush: &BrushSnapshot,
        output: &mut Vec<Dab>,
        damage: &mut Rect,
    ) {
        let start = self.last_evaluated.unwrap_or(last);
        let bristles = brush.contact.is_some_and(|c| c.bristles.is_some());
        let reference = brush.reference_diameter();
        let radius = self.last_contact.map_or(reference * 0.5, |dab| {
            if bristles {
                dab.radii[0].max(dab.radii[1])
            } else {
                dab.radii[0].min(dab.radii[1])
            }
        });
        let tolerance = (radius * 0.01).max(0.25);
        let traveled = f64::from((current.stroke_distance - start.stroke_distance).max(0.));
        let path_tolerance = if bristles {
            (FAN_TURN_RADIANS * traveled / 7.).max(0.25)
        } else {
            f64::from(tolerance)
        };
        let path_tolerance = if brush.stabilization.strength > 0. {
            path_tolerance.min((0.05 / f64::from(brush.stabilization.input_scale).max(1e-6)).max(f64::from(radius) * 0.01))
        } else { path_tolerance };
        let dx = f64::from(current.position().x) - f64::from(start.position().x);
        let dy = f64::from(current.position().y) - f64::from(start.position().y);
        // Every intervening vertex lies inside the ellipse whose focal points
        // are the endpoints and whose major axis is the traveled path length.
        // Its minor radius bounds the error without retaining a point list.
        let error2 = (traveled * traveled - dx * dx - dy * dy).max(0.) * 0.25;
        let pressure_change = (current.point.pressure - start.point.pressure).abs()
            * reference * 0.5;
        let tilt_change = (current.point.tilt[0] - start.point.tilt[0])
            .hypot(current.point.tilt[1] - start.point.tilt[1]);
        let twist_change = (current.point.twist - start.point.twist + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let pose_changed = pressure_change > tolerance
            || tilt_change > 0.04
            || twist_change.abs() > 0.04;
        // Keep the pose before a pressure/tilt transition just as we keep the
        // vertex before a bend. Otherwise a release narrows an earlier span.
        if (error2 > path_tolerance.powi(2) || pose_changed)
            && last.stroke_distance > start.stroke_distance
        {
            let evaluated = self.evaluate(last, brush);
            self.emit_contacts(evaluated, brush, output, damage);
        }

        let start = self.last_evaluated.unwrap_or(last);
        // Bound live spacing too: prediction can expose the pending endpoint,
        // but committed ink must also advance when prediction is disabled.
        // Large, steady brushes retain coarse sampling rather than emitting at
        // the input rate. Pen-up flushes the final pending pose in finish().
        if current.stroke_distance - start.stroke_distance >= (reference * 0.5).max(4.)
            || pose_changed
        {
            let evaluated = self.evaluate(current, brush);
            self.emit_contacts(evaluated, brush, output, damage);
        } else if brush.contact.is_some_and(|c| c.dry.is_some())
            && brush.rendering.accumulation == layer_core::BrushAccumulation::Flow
            && let Some(completion) = &mut self.dry_completion
        {
            for dab in completion.advance(current.position()) {
                if self.tap_peak.is_some() { Arc::make_mut(&mut self.pending_contacts).push(dab); }
                else { include_dab(damage, dab); output.push(dab); }
            }
        }
    }

    pub fn generate(stroke: &Stroke, space: RgbSpace, output: &mut Vec<Dab>) -> Rect {
        let mut generator = Self::new(space);
        generator.reset_for_replay(stroke);
        let mut damage = Rect::EMPTY;
        for point in stroke.points.iter().copied() {
            damage = damage.union(generator.append(point, &stroke.brush, output));
        }
        damage.union(generator.finish(&stroke.brush, output))
    }

    /// Close the last sub-spacing segment using the final pressure/pose. This
    /// matters for a pointed lift: copying the preceding large dab rounds it off.
    /// A bristle fan then lifts off the paper.
    pub(crate) fn finish(&mut self, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        let damage = self.finish_contact(brush, output);
        let tail = self.momentum(brush);
        let damage = damage.union(damage_for_dabs(&tail));
        output.extend(tail);
        damage
    }

    pub(crate) fn momentum(&self, brush: &BrushSnapshot) -> Vec<Dab> {
        if brush.execution != layer_core::BrushExecution::Liquify || brush.deform.momentum <= 0.
            || brush.deform.mode == layer_core::LiquifyMode::Reconstruct { return Vec::new(); }
        let (Some(last), Some(contact)) = (self.last, self.last_contact) else { return Vec::new(); };
        let count = (36. * brush.deform.momentum).ceil() as u32;
        let duration = count as f32 / 60.;
        let travel = (last.speed * duration * 0.5).min(brush.diameter * brush.deform.momentum);
        let angle = last.direction_turns * std::f32::consts::TAU;
        let motion = [angle.cos() * travel / count as f32, angle.sin() * travel / count as f32];
        if brush.deform.mode == layer_core::LiquifyMode::Push && travel < 0.001 { return Vec::new(); }
        (0..count).map(|i| {
            let mut dab = contact;
            dab.center = Point { x: last.point.position.x + motion[0] * (i + 1) as f32,
                y: last.point.position.y + motion[1] * (i + 1) as f32 };
            dab.motion = motion;
            dab.material[3] *= (1. - (i as f32 + 0.5) / count as f32).powi(2)
                * 36. * brush.deform.momentum / count as f32;
            dab.previous_contact[0] = layer_core::liquify::substeps(brush.deform.mode, dab.radii,
                dab.hardness, dab.material[3], dab.motion) as f32;
            dab.previous_contact[1] = (self.dab_index.wrapping_add(i) & 0x00ff_ffff) as f32;
            dab
        }).collect()
    }

    pub(crate) fn finish_contact(&mut self, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        let mut damage = Rect::EMPTY;
        if let Some(model) = self.position_smoother.take_completion_model() {
            damage = damage.union(self.append_continuation(model.endpoint(), brush, &model, output));
        }
        if let Some(peak) = self.tap_peak.take() {
            Arc::make_mut(&mut self.pending_contacts).clear();
            self.dry_completion = Some(Default::default());
            self.last_contact = None;
            let evaluated = self.evaluate(peak, brush);
            self.emit_contacts(evaluated, brush, output, &mut damage);
            return damage;
        }
        damage = damage.union(self.flush_contact(brush, output));
        if brush.contact.is_some_and(|c| c.dry.is_some()) && brush.rendering.accumulation == layer_core::BrushAccumulation::Flow
            && let Some(completion) = &mut self.dry_completion
        {
            for dab in completion.finish() { include_dab(&mut damage, dab); output.push(dab); }
        }
        damage
    }

    pub(crate) fn flush_contact(&mut self, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        let Some(contact) = brush.contact else {
            return Rect::EMPTY;
        };
        let (Some(last), Some(dab)) = (self.last, self.last_contact) else {
            return Rect::EMPTY;
        };
        let mut damage = Rect::EMPTY;
        if (last.position().x - dab.center.x).hypot(last.position().y - dab.center.y) > 0.0001 {
            let evaluated = self.evaluate(last, brush);
            self.emit_contacts(evaluated, brush, output, &mut damage);
        }
        if contact.bristles.is_some() {
            damage = damage.union(self.lift_fan(output));
        }
        damage
    }

    pub(crate) fn modeled_position(&self) -> Option<Point> {
        self.last.map(|point| point.point.position)
    }

    /// Unless the preview from `start` already reaches the endpoint, add one
    /// preview-only contact centered exactly there without advancing the
    /// committed generator or its random sequence.
    pub(crate) fn reach_endpoint(&self, endpoint: Point, start: usize, exact: bool, brush: &BrushSnapshot, output: &mut Vec<Dab>) {
        if !(exact && output.last().is_none_or(|dab| dab.center != endpoint)) && dabs_cover_point(&output[start..], endpoint) {
            return;
        }
        let Some(mut dab) = self.last_contact else {
            return;
        };
        let modeled = if dab.previous[0] > 0.0 {
            dab.previous = [dab.radii[0], dab.radii[1], dab.rotation[0], dab.rotation[1]];
            dab.previous_contact = dab.contact;
            dab.center
        } else {
            self.modeled_position().unwrap_or(endpoint)
        };
        dab.center = endpoint;
        dab.motion = [endpoint.x - modeled.x, endpoint.y - modeled.y];
        if dab.motion == [0.;2] && brush.contact.is_some_and(|c| c.dry.is_some()) { return; }
        output.push(dab);
    }

    fn characterize(&mut self, point: StrokePoint) -> DynamicPoint {
        let Some(last) = self.last else {
            return DynamicPoint {
                point,
                speed: 0.0,
                direction_turns: 0.0,
                stroke_distance: 0.0,
            };
        };
        let dx = point.position.x - last.point.position.x;
        let dy = point.position.y - last.point.position.y;
        let distance = dx.hypot(dy);
        let micros = point
            .elapsed_micros
            .saturating_sub(last.point.elapsed_micros);
        let seconds = micros as f32 / 1_000_000.0;
        if seconds > 0.0 {
            let instantaneous = distance / seconds;
            let retained = (-seconds / SPEED_FILTER_SECONDS).exp();
            self.filtered_speed =
                retained.mul_add(self.filtered_speed, (1.0 - retained) * instantaneous);
        }
        let direction_turns = if distance > f32::EPSILON {
            (dy.atan2(dx) / std::f32::consts::TAU).rem_euclid(1.0)
        } else {
            last.direction_turns
        };
        DynamicPoint {
            point,
            speed: self.filtered_speed,
            direction_turns,
            stroke_distance: last.stroke_distance + distance,
        }
    }

    fn stabilize(&mut self, point: StrokePoint, brush: &BrushSnapshot, position: Option<Point>) -> StrokePoint {
        let raw_distance = self.position_smoother.previous().map_or(0., |last|
            (point.position.x - last.position.x).hypot(point.position.y - last.position.y));
        let position = if let Some(position) = position {
            self.position_smoother.observe(point);
            position
        } else { self.position_smoother.append(point, &brush.stabilization) };
        let Some(last) = self.stabilized_input else {
            let point = StrokePoint { position, ..point };
            self.stabilized_input = Some(point);
            self.pressure_fall_target = point.pressure;
            return point;
        };
        let elapsed = point.elapsed_micros.saturating_sub(last.elapsed_micros) as f32 / 1_000_000.0;
        let pressure_response =
            (1.0 - brush.stabilization.pressure_smoothing * 0.92).clamp(0.04, 1.0);
        let mut pressure = last.pressure + (point.pressure - last.pressure) * pressure_response;
        if brush.stabilization.pressure_fall_micros > 0 && pressure < last.pressure {
            if raw_distance > f32::EPSILON && elapsed > 0.0 {
                let fall_seconds = brush.stabilization.pressure_fall_micros as f32 / 1_000_000.0;
                let maximum_change = elapsed / fall_seconds;
                let target = pressure.clamp(
                    self.pressure_fall_target - maximum_change,
                    (self.pressure_fall_target + maximum_change).min(last.pressure),
                );
                // Smooth the rate-limited target, not just its endpoint. Exact
                // integration of a linear target keeps the fall velocity
                // continuous at pressure steps and repeated sensor values.
                // Keep the smoothing response independent of the maximum fall
                // rate, so tuning the rate cannot also weaken the smoothing.
                let response_seconds = PRESSURE_FALL_RESPONSE_SECONDS;
                let slope = (target - self.pressure_fall_target) / elapsed;
                let retained = (-elapsed / response_seconds).exp();
                pressure = target - slope * response_seconds
                    + (last.pressure - self.pressure_fall_target + slope * response_seconds)
                        * retained;
                pressure = pressure.clamp(target.min(last.pressure), last.pressure);
                self.pressure_fall_target = target;
            } else {
                // A stationary up has no new segment in which to change size.
                pressure = last.pressure;
            }
        } else {
            // Increasing pressure remains immediate; each new contact also
            // initializes both states from its actual pressure.
            self.pressure_fall_target = pressure;
        }
        let stabilized = StrokePoint {
            position,
            pressure,
            tilt: [
                point.tilt[0],
                point.tilt[1],
            ],
            twist: mix_angle(
                last.twist,
                point.twist,
                if self.barrel_twist && brush.contact.is_some_and(|c| c.bristles.is_some()) {
                    1.0 - (-elapsed / BARREL_TWIST_RESPONSE_SECONDS).exp()
                } else {
                    1.
                },
                std::f32::consts::TAU,
            ),
            elapsed_micros: point.elapsed_micros,
        };
        self.stabilized_input = Some(stabilized);
        stabilized
    }

    fn evaluate(&mut self, point: DynamicPoint, brush: &BrushSnapshot) -> EvaluatedDab {
        let variant = self.next_random();
        let dab_index = self.dab_index;
        self.dab_index = self.dab_index.wrapping_add(1);
        let mut values = Registers {
            diameter: brush.diameter,
            opacity: 1.,
            flow: 1.,
            hardness: brush.hardness,
            spacing: brush.spacing,
            aspect: brush.aspect,
            rotation: brush.angle_radians,
            scatter_along: 0.0,
            scatter_across: 0.0,
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            secondary_color: 0.0,
            grain_depth: brush.grain.as_ref().map_or(0.0, |grain| grain.depth),
            pull: brush.wet_mix.pull,
            deposit: 1.0,
            deform_strength: brush.deform.strength
                * (1.0 - brush.deform.pressure + brush.deform.pressure * point.point.pressure),
        };
        for (index, mapping) in brush.mappings.iter().take(MAX_BRUSH_MAPPINGS).enumerate() {
            if brush.execution == layer_core::BrushExecution::Liquify && mapping.sensor == BrushSensor::Twist && !self.barrel_twist { continue; }
            apply_mapping(
                &mut values,
                mapping,
                sensor_value(point, mapping.sensor, variant, index as u32),
            );
        }

        let mut diameter = finite_or(values.diameter, brush.diameter)
            .clamp(brush.bounds.minimum_size, brush.bounds.maximum_size);
        let taper_reference = if brush.contact.is_some() {
            brush.reference_diameter()
        } else {
            diameter
        };
        let (taper_size, taper_opacity) = taper_factors(
            point.stroke_distance,
            self.total_distance,
            taper_reference,
            brush,
        );
        diameter = (diameter * taper_size).clamp(0.01, MAX_BRUSH_DIAMETER);
        let aspect = finite_or(values.aspect, brush.aspect).clamp(0.02, 50.0);
        let direction = point.direction_turns * std::f32::consts::TAU;
        let along = finite_or(values.scatter_along, 0.0)
            .clamp(-MAX_BRUSH_SCATTER_DIAMETERS, MAX_BRUSH_SCATTER_DIAMETERS)
            * diameter;
        let across = finite_or(values.scatter_across, 0.0)
            .clamp(-MAX_BRUSH_SCATTER_DIAMETERS, MAX_BRUSH_SCATTER_DIAMETERS)
            * diameter;
        let (sin, cos) = direction.sin_cos();
        let center = Point {
            x: point.position().x + cos.mul_add(along, -sin * across),
            y: point.position().y + sin.mul_add(along, cos * across),
        };
        let tilt_direction = point.point.tilt[1].atan2(point.point.tilt[0]);
        let tilt = (point.point.tilt[0].hypot(point.point.tilt[1]) / 1.2).clamp(0.0, 1.0);
        let mut rotation = finite_or(values.rotation, brush.angle_radians)
            + direction * brush.shape.follow_direction
            + tilt_direction * brush.shape.follow_tilt
            + point.point.twist * brush.shape.follow_twist;
        let mut radii = [diameter * 0.5, diameter * 0.5 / aspect];
        let mut contact_tilt = tilt;
        let mut dry_relief = None;
        if let Some(contact) = brush.contact {
            if let Some(bristles) = contact.bristles {
                (rotation, radii, contact_tilt) = self.fan_pose(
                    point.point, tilt, tilt_direction, diameter, bristles, brush.angle_radians,
                );
            } else if let Some(dry) = contact.dry {
                contact_tilt = (point.point.tilt[0].hypot(point.point.tilt[1]) / std::f32::consts::FRAC_PI_2).clamp(0., 1.);
                let pose = dry.pose(point.point.tilt[0].hypot(point.point.tilt[1]));
                radii[0] *= pose.radii[0];
                radii[1] *= pose.radii[1];
                dry_relief = Some(pose.relief);
                if contact_tilt > 0.02 {
                    rotation += tilt_direction * (1.-brush.shape.follow_tilt);
                }
            }
        }
        let (rotation_sin, rotation_cos) = rotation.sin_cos();
        let liquify = brush.execution == layer_core::BrushExecution::Liquify;
        if liquify { radii = radii.map(|radius| radius.max(1.)); }
        let motion = self.last_evaluated.map_or([0.0; 2], |last| {
            [point.position().x - last.position().x, point.position().y - last.position().y]
        });
        if motion[0].hypot(motion[1]) > 0. {
            let length = motion[0].hypot(motion[1]);
            self.edge_normal = [-motion[1] / length, motion[0] / length];
        }
        self.last_evaluated = Some(point);
        let falloff = if brush.path.falloff_distance > 0.0 {
            (1.0 - point.stroke_distance / (brush.path.falloff_distance * diameter).max(0.01))
                .clamp(0.0, 1.0)
        } else {
            1.0
        };
        values.opacity = (values.opacity * taper_opacity * falloff)
            .clamp(brush.bounds.minimum_opacity, brush.bounds.maximum_opacity);
        let charge = brush.wet_mix.charge
            * (-brush.wet_mix.charge_depletion * point.stroke_distance / diameter.max(0.01)).exp();
        let strength = brush.flow * finite_or(values.flow, 1.).clamp(0.0, 1.0);
        let (flow, pressure) = if let Some(contact) = brush.contact.filter(|c| c.dry.is_some()) {
            (1., (point.point.pressure * contact.pressure_gain).clamp(0., 1.) * strength)
        } else {
            (strength, point.point.pressure)
        };
        let hardness = finite_or(values.hardness, brush.hardness).clamp(0., if liquify { layer_core::liquify::MAX_HARDNESS } else { 1. });
        let deformation = values.deform_strength.clamp(0., 1.);
        EvaluatedDab {
            dab: Dab {
                center,
                radii,
                rotation: [rotation_cos, rotation_sin],
                motion,
                color_rgba_linear: resolve_color(
                    self.space,
                    brush.color_rgba_linear,
                    values,
                    brush,
                    variant,
                    self.stroke_seed,
                ),
                flow,
                hardness,
                texture_sign: [1.0, 1.0],
                material: [
                    values.grain_depth.clamp(0.0, if brush.contact.is_some_and(|c| c.dry.is_some()) { 2. } else { 1. }),
                    values.pull.clamp(0.0, 1.0),
                    (values.deposit * charge).clamp(0.0, 1.0),
                    dry_relief.map_or(values.deform_strength.clamp(0., 1.), |_| {
                        let t = (contact_tilt * std::f32::consts::FRAC_PI_2
                            / layer_core::BrushDryMedia::WRITING_TILT_RADIANS).clamp(0., 1.);
                        t*t*(3.-2.*t)
                    }),
                ],
                previous: [0.0; 4],
                contact: dry_relief.map_or([
                    point.point.pressure,
                    contact_tilt,
                    point.stroke_distance / brush.diameter.max(0.01),
                    (self.stroke_seed & 65535) as f32,
                ], |r| [pressure, r[0], r[1], r[2]]),
                previous_contact: if liquify {
                    [layer_core::liquify::substeps(brush.deform.mode, radii, hardness, deformation, motion) as f32,
                        (dab_index & 0xffffff) as f32, self.edge_normal[0], self.edge_normal[1]]
                } else { [0.; 4] },
            },
            spacing: if brush.contact.is_some_and(|c| c.dry.is_some()) { 0. } else { (finite_or(values.spacing, brush.spacing)
                * (1.0 + signed_unit(hash32(variant ^ 0x7f4a_7c15)) * brush.path.spacing_jitter))
                .max(0.005) },
            direction_radians: direction,
            values,
        }
    }

    fn fan_pose(
        &mut self,
        point: StrokePoint,
        tilt: f32,
        azimuth: f32,
        diameter: f32,
        bristles: layer_core::BrushBristles,
        angle: f32,
    ) -> (f32, [f32; 2], f32) {
        let previous = self.bristle_azimuth.unwrap_or(if tilt > 0.02 { azimuth } else { 0. });
        let facing = if tilt > 0.08 {
            mix_angle_unwrapped(
                previous,
                azimuth,
                ((tilt - 0.08) * 4.).clamp(0., 1.),
                std::f32::consts::TAU,
            )
        } else {
            previous
        };
        self.bristle_azimuth = Some(facing);
        let roll = angle + if self.barrel_twist { point.twist - facing } else { 0. };
        let (lean_sin, lean_cos) = (tilt * 1.2).sin_cos();
        let (roll_sin, roll_cos) = roll.sin_cos();
        let (azimuth_sin, azimuth_cos) = facing.sin_cos();
        let fan_x = -azimuth_sin * roll_cos - lean_cos * azimuth_cos * roll_sin;
        let fan_y = azimuth_cos * roll_cos - lean_cos * azimuth_sin * roll_sin;
        let pressure = point.pressure.clamp(0., 1.);
        let radius = diameter * 0.5;
        let width =
            radius * fan_x.hypot(fan_y) * (1. - 0.5 * bristles.splay * (1. - pressure.sqrt()));
        let depth = radius * (0.02 + 0.3 * pressure * pressure.sqrt() + 0.1 * tilt);
        (
            (-fan_x).atan2(fan_y),
            [depth.max(0.5), width.max(0.5)],
            lean_sin * roll_sin,
        )
    }

    fn emit_contacts(
        &mut self,
        evaluated: EvaluatedDab,
        brush: &BrushSnapshot,
        output: &mut Vec<Dab>,
        damage: &mut Rect,
    ) {
        if brush.execution == layer_core::BrushExecution::Liquify {
            let dab = evaluated.dab;
            if dab.material[3] <= 0. || match brush.deform.mode {
                layer_core::LiquifyMode::Push => dab.motion == [0.; 2],
                layer_core::LiquifyMode::Edge => dab.previous_contact[2..] == [0.; 2],
                layer_core::LiquifyMode::Crystals => 12. * brush.deform.distortion * dab.material[3] < 0.5,
                _ => false,
            } { return; }
        }
        if brush.contact.is_some_and(|c| c.bristles.is_some()) {
            self.emit_fan(evaluated.dab, output, damage);
            return;
        }
        if brush.contact.is_some_and(|c| c.dry.is_some()) {
            let mut dab = evaluated.dab;
            let opening = self.last_contact.is_none();
            self.connect_contact(&mut dab);
            self.last_contact = Some(dab);
            if opening || dab.motion != [0.;2] {
                let completion = if brush.rendering.accumulation == layer_core::BrushAccumulation::Flow {
                    self.dry_completion.as_mut().map_or_else(Vec::new, |path| path.append(dab))
                } else { Vec::new() };
                for added in completion {
                    if self.tap_peak.is_some() { Arc::make_mut(&mut self.pending_contacts).push(added); }
                    else { include_dab(damage, added); output.push(added); }
                }
                if self.tap_peak.is_some() { Arc::make_mut(&mut self.pending_contacts).push(dab); }
                else { include_dab(damage,dab); output.push(dab); }
            }
            return;
        }
        let maximum = brush.shape.count.max(1);
        let minimum =
            ((maximum as f32 * (1.0 - brush.shape.count_jitter)).ceil() as u8).clamp(1, maximum);
        let count = if minimum == maximum {
            maximum
        } else {
            minimum + (self.next_random() % u32::from(maximum - minimum + 1)) as u8
        };
        for _ in 0..count {
            let variant = self.next_random();
            let mut dab = evaluated.dab;
            let diameter = dab.radii[0] * 2.0;
            let distortion = if brush.execution == layer_core::BrushExecution::Liquify
                && !matches!(brush.deform.mode, layer_core::LiquifyMode::Crystals | layer_core::LiquifyMode::Reconstruct)
                { brush.deform.distortion * 0.1 } else { 0. };
            let along = signed_unit(hash32(variant ^ 0x68bc_21eb))
                * (brush.path.jitter_along + distortion).min(MAX_BRUSH_SCATTER_DIAMETERS)
                * diameter;
            let across = signed_unit(hash32(variant ^ 0x02e5_be93))
                * (brush.path.jitter_across + distortion).min(MAX_BRUSH_SCATTER_DIAMETERS)
                * diameter;
            let (sin, cos) = evaluated.direction_radians.sin_cos();
            dab.center.x += cos.mul_add(along, -sin * across);
            dab.center.y += sin.mul_add(along, cos * across);

            let size = 1.0 + signed_unit(hash32(variant ^ 0x31e7_9db5)) * brush.shape.size_jitter;
            dab.radii[0] *= size;
            dab.radii[1] *= size;

            let rotation = signed_unit(hash32(variant ^ 0x967a_889b))
                * brush.shape.rotation_jitter
                * std::f32::consts::TAU;
            let current = dab.rotation[1].atan2(dab.rotation[0]) + rotation;
            let (rotation_sin, rotation_cos) = current.sin_cos();
            dab.rotation = [rotation_cos, rotation_sin];
            dab.texture_sign = [
                if hash_to_unit(hash32(variant ^ 0xa511_e9b3)) < brush.shape.flip_x_probability {
                    -1.0
                } else {
                    1.0
                },
                if hash_to_unit(hash32(variant ^ 0x63d8_35d9)) < brush.shape.flip_y_probability {
                    -1.0
                } else {
                    1.0
                },
            ];
            dab.color_rgba_linear = resolve_color(
                self.space,
                brush.color_rgba_linear,
                evaluated.values,
                brush,
                variant,
                self.stroke_seed,
            );
            if brush.contact.is_some() {
                self.connect_contact(&mut dab);
            }
            include_dab(damage, dab);
            self.last_contact = Some(dab);
            output.push(dab);
        }
    }

    fn connect_contact(&self, dab: &mut Dab) {
        let previous = self.last_contact.unwrap_or(*dab);
        dab.previous = [previous.radii[0], previous.radii[1], previous.rotation[0], previous.rotation[1]];
        dab.previous_contact = previous.contact;
        dab.motion = [dab.center.x - previous.center.x, dab.center.y - previous.center.y];
    }

    fn emit_fan(&mut self, dab: Dab, output: &mut Vec<Dab>, damage: &mut Rect) {
        let from = self.last_contact.unwrap_or(dab);
        let start_angle = from.rotation[1].atan2(from.rotation[0]);
        let turn = mix_angle_unwrapped(
            start_angle,
            dab.rotation[1].atan2(dab.rotation[0]),
            1.,
            std::f32::consts::TAU,
        ) - start_angle;
        let reach = from.radii[1].max(dab.radii[1]);
        let depth = from.radii[0].min(dab.radii[0]);
        let arcs = (turn.abs() * reach / depth).ceil().clamp(1., MAX_FAN_ARCS) as usize;
        let mut previous = from;
        for arc in 1..=arcs {
            let mut next = dab;
            if arc < arcs {
                let t = arc as f32 / arcs as f32;
                let (sin, cos) = (start_angle + turn * t).sin_cos();
                next.center = Point {
                    x: mix(from.center.x, dab.center.x, t),
                    y: mix(from.center.y, dab.center.y, t),
                };
                next.radii = [mix(from.radii[0], dab.radii[0], t), mix(from.radii[1], dab.radii[1], t)];
                next.rotation = [cos, sin];
                for channel in 0..3 {
                    next.contact[channel] = mix(from.contact[channel], dab.contact[channel], t);
                }
            }
            next.previous = [previous.radii[0], previous.radii[1], previous.rotation[0], previous.rotation[1]];
            next.previous_contact = previous.contact;
            next.motion = [next.center.x - previous.center.x, next.center.y - previous.center.y];
            let turn = (previous.rotation[0] * next.rotation[1] - previous.rotation[1] * next.rotation[0])
                .atan2(previous.rotation[0] * next.rotation[0] + previous.rotation[1] * next.rotation[1]);
            let opening = self.last_contact.is_none() && arc == 1;
            if opening {
                self.touchdown = previous.center;
            }
            self.fan_roll += turn;
            self.fan_slide = self.fan_slide.max(
                (next.center.x - self.touchdown.x).hypot(next.center.y - self.touchdown.y)
                    + self.fan_roll.abs() * next.radii[1] * 0.5,
            );
            let start_heading = heading_angle(previous.rotation, self.fan_heading);
            self.turn_fan_heading(next.motion, next.radii);
            let coherent = self.fan_axis[0].hypot(self.fan_axis[1]) + self.fan_roll.abs() * next.radii[1] * 0.5;
            next.material = [0., 1., next.material[2], 1.];
            next.texture_sign = [start_heading, heading_angle(next.rotation, self.fan_heading)];
            if self.fan_dragging {
                include_dab(damage, next);
                output.push(next);
            } else {
                Arc::make_mut(&mut self.pending_contacts).push(next);
                self.fan_deepest = self.fan_deepest.max(next.radii[0]);
                self.fan_pressed = self.fan_pressed.max(next.contact[0]);
                if self.fan_slide.min(coherent) > self.fan_deepest + 4. {
                    self.fan_dragging = true;
                    self.release_pending(true, output, damage);
                } else if self.pending_contacts.len() >= MAX_FAN_PENDING {
                    self.release_pending(false, output, damage);
                }
            }
            previous = next;
        }
        self.last_contact = Some(previous);
    }

    /// Until the fan has dragged further than its deepest contact, beyond sensor
    /// jitter, a stroke may still be a tap. Its first spans wait here. A drag
    /// releases them streaked along its travel, so streaks run from the
    /// trailing edge right back to where the brush touched down; a tap releases
    /// them as a pressed imprint, carrying in deposit strength how hard it was
    /// pressed.
    fn release_pending(&mut self, dragged: bool, output: &mut Vec<Dab>, damage: &mut Rect) {
        let streak = f32::from(u8::from(dragged));
        for mut span in Arc::make_mut(&mut self.pending_contacts).drain(..) {
            span.material[1] = streak;
            span.material[3] = streak;
            if !dragged {
                span.material[2] = self.fan_pressed;
            }
            span.texture_sign = if dragged {
                [
                    heading_angle([span.previous[2], span.previous[3]], self.fan_heading),
                    heading_angle(span.rotation, self.fan_heading),
                ]
            } else {
                [hair_axis(span.texture_sign[0]), hair_axis(span.texture_sign[1])]
            };
            include_dab(damage, span);
            output.push(span);
        }
    }

    /// Spans commit the paint their trailing edge leaves behind. Lifting the fan
    /// commits what is still under it: the last pose, left where it is.
    fn lift_fan(&mut self, output: &mut Vec<Dab>) -> Rect {
        let Some(pose) = self.last_contact else {
            return Rect::EMPTY;
        };
        let mut damage = Rect::EMPTY;
        let dragged = self.fan_dragging;
        self.release_pending(dragged, output, &mut damage);
        let mut lift = pose;
        lift.motion = [0.; 2];
        lift.previous = [pose.radii[0], pose.radii[1], pose.rotation[0], pose.rotation[1]];
        lift.previous_contact = pose.contact;
        let heading = heading_angle(pose.rotation, self.fan_heading);
        lift.texture_sign = [if dragged { heading } else { hair_axis(heading) }; 2];
        let streak = f32::from(u8::from(dragged));
        let pressed = if dragged { pose.material[2] } else { self.fan_pressed };
        lift.material = [FAN_LIFT, streak, pressed, streak];
        include_dab(&mut damage, lift);
        output.push(lift);
        damage
    }

    /// Follows the axis of travel over about the fan's half width, so a quick
    /// reversal or a tight turn does not swing the streaks around. The heading
    /// keeps a continuous sense along that axis.
    fn turn_fan_heading(&mut self, motion: [f32; 2], radii: [f32; 2]) {
        let step = motion[0].hypot(motion[1]);
        if step <= 0.0001 {
            return;
        }
        let retained = (-step / radii[1].max(radii[0] * 2.).max(4.)).exp();
        let doubled = [(motion[0] * motion[0] - motion[1] * motion[1]) / step, 2. * motion[0] * motion[1] / step];
        self.fan_axis = [self.fan_axis[0] * retained + doubled[0], self.fan_axis[1] * retained + doubled[1]];
        let (sin, cos) = (self.fan_axis[1].atan2(self.fan_axis[0]) * 0.5).sin_cos();
        let reference = if self.fan_heading == [0.; 2] { motion } else { self.fan_heading };
        let sense = if cos * reference[0] + sin * reference[1] < 0. { -1. } else { 1. };
        self.fan_heading = [cos * sense, sin * sense];
    }

    fn next_random(&mut self) -> u32 {
        let mut value = self.rng;
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        self.rng = value;
        value
    }
}

/// Smoothly distributes an endpoint correction over a replaceable contact
/// tail. The first contact remains attached to the stable frontier and the last
/// contact receives the full correction.
pub(crate) fn lock_dab_tail(dabs: &mut [Dab], modeled_endpoint: Point, target: Point) {
    if dabs.is_empty() {
        return;
    }
    let correction = Point {
        x: target.x - modeled_endpoint.x,
        y: target.y - modeled_endpoint.y,
    };
    let count = dabs.len();
    let last = count.saturating_sub(1).max(1) as f32;
    let mut previous_weight = 0.0;
    for (index, dab) in dabs.iter_mut().enumerate() {
        let linear = if count == 1 { 1.0 } else { index as f32 / last };
        let smooth = linear * linear * (3.0 - 2.0 * linear);
        let weight = smooth.powf(CORRECTION_EASING);
        dab.center.x += correction.x * weight;
        dab.center.y += correction.y * weight;
        dab.motion[0] += correction.x * (weight - previous_weight);
        dab.motion[1] += correction.y * (weight - previous_weight);
        previous_weight = weight;
    }
}

pub(crate) fn taper_prediction(dabs: &mut [Dab], origin: Point, amount: f32) {
    if amount <= 0. { return; }
    let Some(first) = dabs.first() else { return };
    let length: f32 = dabs.iter().map(|d| d.motion[0].hypot(d.motion[1])).sum();
    let first_length = first.motion[0].hypot(first.motion[1]);
    let start = ((origin.x-first.center.x+first.motion[0])*first.motion[0]
        +(origin.y-first.center.y+first.motion[1])*first.motion[1])
        / first_length.max(0.000001);
    let start = start.clamp(0., first_length);
    if length <= start { return; }
    let scale = |distance: f32| {
        let t = ((distance - start) / (length - start)).clamp(0., 1.);
        1. - 0.25 * amount * t * t * (3. - 2. * t)
    };
    let mut distance = 0.;
    for dab in dabs {
        let previous_size = scale(distance);
        distance += dab.motion[0].hypot(dab.motion[1]);
        let size = scale(distance);

        dab.radii[0] *= size;
        dab.radii[1] *= size;
        dab.previous[0] *= previous_size;
        dab.previous[1] *= previous_size;
    }
}

pub(crate) fn dabs_cover_point(dabs: &[Dab], point: Point) -> bool {
    dabs.iter().any(|dab| {
        let dx = point.x - dab.center.x;
        let dy = point.y - dab.center.y;
        let local_x = dab.rotation[0].mul_add(dx, dab.rotation[1] * dy);
        let local_y = (-dab.rotation[1]).mul_add(dx, dab.rotation[0] * dy);
        let normalized = (local_x / dab.radii[0].max(0.005)).powi(2)
            + (local_y / dab.radii[1].max(0.005)).powi(2);
        normalized <= 0.04
    })
}

pub(crate) fn damage_for_dabs(dabs: &[Dab]) -> Rect {
    let mut damage = Rect::EMPTY;
    for dab in dabs {
        include_dab(&mut damage, *dab);
    }
    damage
}

impl DynamicPoint {
    fn position(self) -> Point {
        self.point.position
    }
}

fn apply_mapping(registers: &mut Registers, mapping: &BrushMapping, input: f32) {
    let width = mapping.input_max - mapping.input_min;
    let normalized = if width.abs() > f32::EPSILON {
        (input - mapping.input_min) / width
    } else {
        0.0
    };
    let contribution = mapping
        .curve
        .sample(normalized)
        .mul_add(mapping.output_scale, mapping.output_bias);
    let target = match mapping.target {
        BrushTarget::Diameter => &mut registers.diameter,
        BrushTarget::Opacity => &mut registers.opacity,
        BrushTarget::Flow => &mut registers.flow,
        BrushTarget::Hardness => &mut registers.hardness,
        BrushTarget::Spacing => &mut registers.spacing,
        BrushTarget::Aspect => &mut registers.aspect,
        BrushTarget::Rotation => &mut registers.rotation,
        BrushTarget::ScatterAlong => &mut registers.scatter_along,
        BrushTarget::ScatterAcross => &mut registers.scatter_across,
        BrushTarget::Hue => &mut registers.hue,
        BrushTarget::Saturation => &mut registers.saturation,
        BrushTarget::Lightness => &mut registers.lightness,
        BrushTarget::SecondaryColor => &mut registers.secondary_color,
        BrushTarget::GrainDepth => &mut registers.grain_depth,
        BrushTarget::Pull => &mut registers.pull,
        BrushTarget::Deposit => &mut registers.deposit,
        BrushTarget::DeformStrength => &mut registers.deform_strength,
    };
    *target = match mapping.combine {
        BrushCombine::Replace => contribution,
        BrushCombine::Add => *target + contribution,
        BrushCombine::Multiply => *target * contribution,
    };
}

fn sensor_value(point: DynamicPoint, sensor: BrushSensor, variant: u32, index: u32) -> f32 {
    match sensor {
        BrushSensor::Pressure => point.point.pressure,
        BrushSensor::Speed => point.speed,
        BrushSensor::Direction => point.direction_turns,
        BrushSensor::TiltMagnitude => point.point.tilt[0].hypot(point.point.tilt[1]).min(1.0),
        BrushSensor::TiltDirection => {
            (point.point.tilt[1].atan2(point.point.tilt[0]) / std::f32::consts::TAU).rem_euclid(1.0)
        }
        BrushSensor::Twist => (point.point.twist / std::f32::consts::TAU).rem_euclid(1.0),
        BrushSensor::StrokeDistance => point.stroke_distance,
        BrushSensor::StrokeTime => point.point.elapsed_micros as f32 / 1_000_000.0,
        BrushSensor::Random => hash_to_unit(variant ^ index.wrapping_mul(0x9e37_79b9)),
    }
}

fn interpolate_point(a: StrokePoint, b: StrokePoint, t: f32) -> StrokePoint {
    StrokePoint {
        position: Point { x: mix(a.position.x, b.position.x, t), y: mix(a.position.y, b.position.y, t) },
        pressure: mix(a.pressure, b.pressure, t),
        tilt: [mix(a.tilt[0], b.tilt[0], t), mix(a.tilt[1], b.tilt[1], t)],
        twist: mix_angle(a.twist, b.twist, t, std::f32::consts::TAU),
        elapsed_micros: (f64::from(a.elapsed_micros) + f64::from(b.elapsed_micros.saturating_sub(a.elapsed_micros)) * f64::from(t)).round() as u32,
    }
}

fn interpolate(a: DynamicPoint, b: DynamicPoint, t: f32) -> DynamicPoint {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    DynamicPoint {
        point: StrokePoint {
            position: Point {
                x: mix(a.point.position.x, b.point.position.x),
                y: mix(a.point.position.y, b.point.position.y),
            },
            pressure: mix(a.point.pressure, b.point.pressure),
            tilt: [
                mix(a.point.tilt[0], b.point.tilt[0]),
                mix(a.point.tilt[1], b.point.tilt[1]),
            ],
            twist: mix_angle(a.point.twist, b.point.twist, t, std::f32::consts::TAU),
            elapsed_micros: mix(a.point.elapsed_micros as f32, b.point.elapsed_micros as f32)
                as u32,
        },
        speed: mix(a.speed, b.speed),
        direction_turns: mix_angle(a.direction_turns, b.direction_turns, t, 1.0),
        stroke_distance: mix(a.stroke_distance, b.stroke_distance),
    }
}

fn mix(a: f32, b: f32, t: f32) -> f32 { a + (b - a) * t }

/// The travel direction as an angle from the fan's normal.
fn heading_angle(normal: [f32; 2], travel: [f32; 2]) -> f32 {
    if travel[0].hypot(travel[1]) <= 0.0001 {
        return 0.;
    }
    (normal[0] * travel[1] - normal[1] * travel[0]).atan2(normal[0] * travel[0] + normal[1] * travel[1])
}

/// The hair axis nearest a heading, for a pressed imprint.
fn hair_axis(heading: f32) -> f32 {
    if heading.abs() > std::f32::consts::FRAC_PI_2 {
        std::f32::consts::PI.copysign(heading)
    } else {
        0.
    }
}

fn mix_angle(a: f32, b: f32, t: f32, period: f32) -> f32 {
    mix_angle_unwrapped(a, b, t, period).rem_euclid(period)
}

// Preserve the directed material frame: folding an ellipse angle into [0, PI)
// mirrors its bristles whenever a gently curving stroke crosses horizontal.
fn mix_angle_unwrapped(a: f32, b: f32, t: f32, period: f32) -> f32 {
    let delta = (b - a + period * 0.5).rem_euclid(period) - period * 0.5;
    a + delta * t
}

fn spacing_for(dab: Dab, spacing: f32) -> f32 {
    (dab.radii[0] * 2.0 * spacing.max(0.005)).max(MIN_DAB_DISTANCE)
}

fn include_dab(damage: &mut Rect, dab: Dab) {
    *damage = damage.union(dab.bounds());
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

fn taper_factors(
    distance: f32,
    total_distance: Option<f32>,
    diameter: f32,
    brush: &BrushSnapshot,
) -> (f32, f32) {
    let envelope = |progress: f32, minimum: f32| {
        let progress = progress.clamp(0.0, 1.0);
        let smooth = (progress * progress * (3.0 - 2.0 * progress)).powf(brush.taper.tip_sharpness);
        minimum + (1.0 - minimum) * smooth
    };
    let start_extent = brush.taper.start_distance_diameters * diameter;
    let start_progress = if start_extent > 0.0 {
        distance / start_extent
    } else {
        1.0
    };
    let mut size = envelope(start_progress, brush.taper.start_size);
    let mut opacity = envelope(start_progress, brush.taper.start_opacity);
    if let Some(total) = total_distance {
        let end_extent = brush.taper.end_distance_diameters * diameter;
        let end_progress = if end_extent > 0.0 {
            (total - distance).max(0.0) / end_extent
        } else {
            1.0
        };
        size *= envelope(end_progress, brush.taper.end_size);
        opacity *= envelope(end_progress, brush.taper.end_opacity);
    }
    (size, opacity)
}

fn resolve_color(
    space: RgbSpace,
    primary: [f32; 4],
    values: Registers,
    brush: &BrushSnapshot,
    stamp_variant: u32,
    stroke_seed: u32,
) -> [f32; 4] {
    let dynamics = brush.color_dynamics;
    let stamp = [
        signed_unit(hash32(stamp_variant ^ 0xa341_316c)),
        signed_unit(hash32(stamp_variant ^ 0xc801_3ea4)),
        signed_unit(hash32(stamp_variant ^ 0xad90_777d)),
        hash_to_unit(hash32(stamp_variant ^ 0x7e95_761e)),
    ];
    let stroke = [
        signed_unit(hash32(stroke_seed ^ 0x9e37_79b9)),
        signed_unit(hash32(stroke_seed ^ 0x243f_6a88)),
        signed_unit(hash32(stroke_seed ^ 0xb7e1_5163)),
        hash_to_unit(hash32(stroke_seed ^ 0x85a3_08d3)),
    ];
    let secondary_mix = (values.secondary_color
        + stamp[3] * dynamics.stamp_secondary_jitter
        + stroke[3] * dynamics.stroke_secondary_jitter)
        .clamp(0.0, 1.0);
    let secondary = dynamics.secondary_color_rgba_linear;
    let mut linear = [
        mix_color(primary[0], secondary[0], secondary_mix),
        mix_color(primary[1], secondary[1], secondary_mix),
        mix_color(primary[2], secondary[2], secondary_mix),
    ];
    let hue =
        values.hue + stamp[0] * dynamics.stamp_hue_jitter + stroke[0] * dynamics.stroke_hue_jitter;
    let saturation = values.saturation
        + stamp[1] * dynamics.stamp_saturation_jitter
        + stroke[1] * dynamics.stroke_saturation_jitter;
    let lightness = values.lightness
        + stamp[2] * dynamics.stamp_lightness_jitter
        + stroke[2] * dynamics.stroke_lightness_jitter;
    if hue != 0.0 || saturation != 0.0 || lightness != 0.0 {
        // HSL is a coordinate operation in encoded document RGB. Extending
        // its cube to contain the input preserves negative/above-one channels
        // instead of mapping wide-gamut selections into bounded sRGB. In-gamut
        // colors use the ordinary [0,1] cube. Double intermediates avoid
        // overflow in that range calculation; emitted dabs remain Float32.
        let encoded = linear.map(|v| space.encode(f64::from(v)));
        let low = encoded.into_iter().fold(0.0_f64, f64::min);
        let high = encoded.into_iter().fold(1.0_f64, f64::max);
        let span = high - low;
        let (mut h, mut s, mut l) = rgb_to_hsl(encoded.map(|v| (v - low) / span));
        h = (h + f64::from(hue)).rem_euclid(1.0);
        s = (s + f64::from(saturation)).clamp(0.0, 1.0);
        l = (l + f64::from(lightness)).clamp(0.0, 1.0);
        linear = hsl_to_rgb(h, s, l).map(|v| space.decode(v * span + low) as f32);
    }
    [
        linear[0],
        linear[1],
        linear[2],
        mix_color(primary[3], secondary[3], secondary_mix) * values.opacity.clamp(0.0, 1.0),
    ]
}

fn rgb_to_hsl(rgb: [f64; 3]) -> (f64, f64, f64) {
    let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
    let minimum = rgb[0].min(rgb[1]).min(rgb[2]);
    let lightness = (maximum + minimum) * 0.5;
    let delta = maximum - minimum;
    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (2.0 * lightness.min(1.0 - lightness));
    let hue_sector = if maximum == rgb[0] {
        ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0)
    } else if maximum == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    (hue_sector / 6.0, saturation, lightness)
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let chroma = 2.0 * lightness.min(1.0 - lightness) * saturation;
    let sector = hue.rem_euclid(1.0) * 6.0;
    let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let rgb = match sector.floor() as u32 {
        0 => [chroma, second, 0.0],
        1 => [second, chroma, 0.0],
        2 => [0.0, chroma, second],
        3 => [0.0, second, chroma],
        4 => [second, 0.0, chroma],
        _ => [chroma, 0.0, second],
    };
    let match_value = lightness - chroma * 0.5;
    rgb.map(|value| value + match_value)
}

fn mix_color(a: f32, b: f32, amount: f32) -> f32 {
    if amount == 0.0 {
        return a;
    }
    if amount == 1.0 {
        return b;
    }
    (f64::from(a) + (f64::from(b) - f64::from(a)) * f64::from(amount)) as f32
}

fn signed_unit(value: u32) -> f32 {
    hash_to_unit(value).mul_add(2.0, -1.0)
}

fn hash32(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

fn mix_seed(seed: u32, stroke: u64) -> u32 {
    let mut value = seed ^ stroke as u32 ^ (stroke >> 32) as u32;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    value.max(1)
}

fn hash_to_unit(mut value: u32) -> f32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    (value as f64 / u32::MAX as f64) as f32
}

#[cfg(test)]
#[path = "brush/color_tests.rs"]
mod color_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::BrushCurve;

    fn point(x: f32, pressure: f32, micros: u32) -> StrokePoint {
        crate::test_support::point(x, 0., pressure, micros)
    }

    #[test]
    fn a_long_straight_gap_does_not_tessellate_at_the_short_bend_rate() {
        use std::cell::Cell;
        struct Field { evaluations: Cell<usize> }
        impl PositionField for Field {
            fn position(&self, time: f64) -> Point {
                self.evaluations.set(self.evaluations.get() + 1);
                let u = (time / 0.004).clamp(0., 1.);
                Point { x: (u.powi(3) * (10. + u * (-15. + u * 6.))) as f32, y: 0. }
            }
            fn acceleration_bound(&self, begin: f64, _: f64) -> f64 { if begin < 0.004 { 1e6 } else { 0. } }
        }
        let field = Field { evaluations: Cell::new(0) };
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);

        brush.stabilization.strength = 1.;
        let mut generator = DabGenerator::new(RgbSpace::Srgb);
        let mut output = Vec::new();
        generator.append_with_field(point(0., 1., 0), &brush, Some(&field), &mut output);
        generator.append_with_field(point(1., 1., 1_000_000), &brush, Some(&field), &mut output);
        assert!(field.evaluations.get() < 40, "{} field evaluations", field.evaluations.get());
        assert_eq!(generator.modeled_position(), Some(Point { x: 1., y: 0. }));
    }

    #[test]
    fn contact_lift_closes_the_spacing_gap_and_replays_the_same_point() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        // Test explicit terminal pressure independently of pressure stabilization.
        brush.stabilization.pressure_fall_micros = 0;
        brush.stabilization.strength = 0.;
        let stroke = Stroke::new(
            StrokeId(17),
            layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(1)),
            layer_core::StrokeTool::Brush,
            brush.clone(),
            vec![
                point(0., 0.8, 0),
                point(17., 0.5, 8_000),
                point(31.123, 0., 16_000),
            ],
        )
        .unwrap();
        let mut live = DabGenerator::default();
        live.reset_for_stroke(stroke.id, &brush);
        let mut dabs = Vec::new();
        for point in stroke.points.iter().copied() {
            live.append(point, &brush, &mut dabs);
        }
        live.finish(&brush, &mut dabs);
        let end = dabs.last().unwrap();
        assert_eq!(end.center.x, 31.123);
        assert!(end.radii[0] < 0.1, "the tip should resolve to a point");
        assert!(end.previous[0] > 0.);
        let mut replay = Vec::new();
        DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut replay);
        assert_eq!(dabs, replay);
    }

    #[test]
    fn retouching_keeps_its_size_through_a_release_without_motion() {
        for preset in [
            layer_core::DefaultBrushPreset::CloneStamp,
            layer_core::DefaultBrushPreset::HealingBrush,
            layer_core::DefaultBrushPreset::SpotHealingBrush,
        ] {
            let mut brush = layer_core::default_brush(preset);
            brush.stabilization.strength = 0.;
            let points = vec![point(0., 0.5, 0), point(5., 0.5, 16_000), point(10., 0.5, 32_000), point(10., 0., 40_000)];
            let stroke =
                Stroke::new(StrokeId(3), layer_core::SourceTarget::Paint(layer_core::authored::PaintHandle::from_index(1)), layer_core::StrokeTool::Brush, brush, points).unwrap();
            let mut dabs = Vec::new();
            DabGenerator::generate(&stroke, RgbSpace::Srgb, &mut dabs);
            let end = dabs.last().unwrap();
            assert_eq!(end.center.x, 10., "{preset:?}");
            assert_eq!(end.radii, dabs[0].radii, "{preset:?}: the stroke ends at its width");
        }
    }

    #[test]
    fn swept_input_does_not_expand_a_fast_light_stroke_into_stamps() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.stabilization.strength = 0.;
        let mut generator = DabGenerator::default();
        let mut dabs = Vec::new();
        for (i, x) in [0., 200., 400., 600., 800.].into_iter().enumerate() {
            generator.append(point(x, 0.1, i as u32 * 4_000), &brush, &mut dabs);
        }
        generator.finish(&brush, &mut dabs);
        assert_eq!(dabs.len(), 5);
        for pair in dabs.windows(2) {
            assert_eq!(pair[1].center.x - pair[1].motion[0], pair[0].center.x);
        }
    }

    #[test]
    fn swept_simplification_retains_corners_pressure_extrema_and_large_brush_spacing() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.stabilization.pressure_fall_micros = 0;
        brush.stabilization.strength = 0.;
        let mut generator = DabGenerator::default();
        let mut dabs = Vec::new();
        generator.append(point(0., 0.1, 0), &brush, &mut dabs);
        generator.append(point(3., 0.1, 4_000), &brush, &mut dabs);
        let mut corner = point(3., 0.1, 8_000);
        corner.position.y = 3.;
        generator.append(corner, &brush, &mut dabs);
        generator.finish(&brush, &mut dabs);
        assert!(dabs.iter().any(|dab| dab.center == Point { x: 3., y: 0. }));
        assert_eq!(dabs.last().unwrap().center, corner.position);

        generator.reset();
        dabs.clear();
        for (i, pressure) in [0.1, 0.9, 0.1].into_iter().enumerate() {
            generator.append(point(i as f32, pressure, i as u32 * 4_000), &brush, &mut dabs);
        }
        assert!(dabs.iter().any(|dab| dab.contact[0] == 0.9));

        generator.reset();
        dabs.clear();
        for (i, (x, pressure)) in [(0., 1.), (2., 1.), (4., 1.), (6., 0.5)]
            .into_iter().enumerate()
        {
            generator.append(point(x, pressure, i as u32 * 4_000), &brush, &mut dabs);
        }
        assert!(dabs.iter().any(|dab| dab.center.x == 4. && dab.contact[0] == 1.),
            "keep the last full-pressure pose before narrowing");

        let mut large = brush;
        large.diameter = 1024.;
        generator.reset();
        dabs.clear();
        for i in 0..=1000 {
            generator.append(point(i as f32, 1., i * 4_000), &large, &mut dabs);
        }
        generator.finish(&large, &mut dabs);
        assert!(dabs.len() <= 4, "large straight brushes retain coarse sampling: {}", dabs.len());
        assert_eq!(dabs.last().unwrap().center.x, 1000.);
    }

    #[test]
    fn curve_fit_retains_broad_contacts_at_their_radius_tolerance() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.diameter = 1024.;
        assert!(brush.stabilization.strength > 0.);
        let mut generator = DabGenerator::default();
        let mut dabs = Vec::new();
        for i in 0..=500u32 {
            let angle = i as f32 * 0.002;
            let sample = crate::test_support::point(1000. * angle.sin(), 1000. * (1. - angle.cos()), 1., i * 4_000);
            generator.append(sample, &brush, &mut dabs);
        }
        generator.finish(&brush, &mut dabs);
        assert!(dabs.len() <= 12, "a broad arc keeps vertices at 1% of its radius: {}", dabs.len());
    }

    #[test]
    fn dry_completion_keeps_segments_and_bounds_work_on_squiggles() {
        let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        brush.diameter=1024.;brush.stabilization.strength=0.;
        let mut strokes=Vec::new();
        for accumulation in [layer_core::BrushAccumulation::Uniform,layer_core::BrushAccumulation::Flow] {
            brush.rendering.accumulation=accumulation;
            let mut generator=DabGenerator::default();
            let mut dabs=Vec::new();
            for i in 0..=2000 {
                let angle=i as f32*0.01;
                let mut sample=crate::test_support::point(175.*angle.cos(),30.*angle.sin(),1.,i*4_000);
                sample.tilt=[std::f32::consts::FRAC_PI_4,0.];
                generator.append(sample,&brush,&mut dabs);
            }
            generator.finish(&brush,&mut dabs);
            strokes.push(dabs);
        }
        let completions=strokes[1].iter().filter(|d|d.is_completion()).count();
        assert!(completions>2 && completions<=24,
            "{completions} corrections for {} ordinary segments",strokes[0].len());
        let ordinary:Vec<_>=strokes[1].iter().copied().filter(|d|!d.is_completion()).collect();
        assert_eq!(ordinary,strokes[0]);
    }

    #[test]
    fn taper_sharpness_changes_the_tip_without_changing_its_length() {
        let mut brush = BrushSnapshot::default();
        brush.taper.end_distance_diameters = 1.;
        brush.taper.end_size = 0.;
        let soft = taper_factors(95., Some(100.), 10., &brush).0;
        brush.taper.tip_sharpness = 2.;
        let sharp = taper_factors(95., Some(100.), 10., &brush).0;
        assert!(sharp < soft);
        assert_eq!(taper_factors(100., Some(100.), 10., &brush).0, 0.);
        assert_eq!(taper_factors(90., Some(100.), 10., &brush).0, 1.);
    }

    #[test]
    fn prediction_width_uses_geometry_independently_of_material_payload() {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
        let mut generator = DabGenerator::default();
        let mut dabs = Vec::new();
        for i in 0..4 { generator.append(point(i as f32*20., 1., i*4000), &brush, &mut dabs); }
        generator.finish(&brush, &mut dabs);
        let mut alternate = dabs.clone();
        for d in &mut alternate { d.contact[2] = 19.; d.previous_contact[2] = 37.; }
        taper_prediction(&mut dabs, Point::default(), 1.);
        taper_prediction(&mut alternate, Point::default(), 1.);
        for (a, b) in dabs.iter().zip(&alternate) {
            assert_eq!(a.radii, b.radii);
            assert_eq!(a.previous, b.previous);
        }
        assert!(dabs.first().unwrap().radii[0] > dabs.last().unwrap().radii[0]);
        for pair in dabs.windows(2) { assert_eq!(pair[0].radii, [pair[1].previous[0], pair[1].previous[1]]); }
    }

    #[test]
    fn cursor_reuses_shape_dynamics_without_advancing_paint_randomness() {
        let mut brush = BrushSnapshot::default();
        brush.shape.rotation_jitter = 0.7;
        brush.shape.size_jitter = 0.3;
        brush.shape.flip_x_probability = 0.6;
        brush.shape.count = 3;
        brush.aspect = 2.5;
        brush.angle_radians = 0.4;
        let mut paint = DabGenerator::default();
        paint.reset_for_stroke(StrokeId(29), &brush);
        let mut cursor = paint.clone();
        let sample = point(20.0, 0.4, 0);
        let expected = cursor.cursor_contacts(sample, &brush);
        assert_eq!(cursor.cursor_contacts(sample, &brush), expected);
        let mut actual = Vec::new();
        paint.append(sample, &brush, &mut actual);
        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 3);
    }

    #[test]
    fn rotated_dab_damage_uses_oriented_extents() {
        let mut damage = Rect::EMPTY;
        include_dab(
            &mut damage,
            Dab {
                center: Point { x: 20.0, y: 30.0 },
                radii: [10.0, 1.0],
                rotation: [0.0, 1.0],
                motion: [0.0; 2],
                color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
                flow: 1.0,
                hardness: 1.0,
                texture_sign: [1.0; 2],
                material: [0.0; 4],
                previous: [0.0; 4],
                contact: [0.0; 4],
                previous_contact: [0.0; 4],
            },
        );
        assert_eq!(damage.min, Point { x: 18.0, y: 19.0 });
        assert_eq!(damage.max, Point { x: 22.0, y: 41.0 });
    }

    fn bristle_generator(twist: bool) -> (layer_core::BrushSnapshot, DabGenerator) {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::BristlePaintbrush);
        let mut generator = DabGenerator::default();
        generator.reset_for_stroke(StrokeId(7), &brush);
        generator.set_barrel_twist(twist);
        (brush, generator)
    }

    #[test]
    fn a_hovering_fan_outlines_its_full_contact() {
        let (brush, mut generator) = bristle_generator(false);
        let mut hover = point(40., 1., 0);
        hover.tilt = [0.5, 0.2];
        let outline = generator.cursor_contacts(hover, &brush);
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].texture_sign, [1.; 2], "the outline is never flipped or collapsed");
        assert!(outline[0].radii[0] > 1. && outline[0].radii[1] > outline[0].radii[0]);
    }

    #[test]
    fn a_held_fan_spreads_with_pressure_and_rolls_through_short_arcs() {
        let (brush, mut generator) = bristle_generator(true);
        let mut dabs = Vec::new();
        let mut light = point(0., 0.05, 0);
        light.twist = 0.02;
        generator.append(light, &brush, &mut dabs);
        let light_width = generator.last_contact.unwrap().radii[1];
        let mut heavy = light;
        heavy.pressure = 0.9;
        heavy.elapsed_micros = 10_000;
        generator.append(heavy, &brush, &mut dabs);
        let heavy_width = generator.last_contact.unwrap().radii[1];
        assert!(heavy_width > light_width * 1.15, "pressure spreads the fan");
        assert!(heavy_width < light_width * 1.6, "without changing its size as much as its paint");
        dabs.clear();
        let mut rolled = heavy;
        rolled.twist = std::f32::consts::FRAC_PI_2;
        for step in 2..=20 {
            rolled.elapsed_micros = step * 10_000;
            generator.append(rolled, &brush, &mut dabs);
        }
        assert!(dabs.len() > 4, "a held roll sweeps arcs no longer than the contact depth");
        for pair in dabs.windows(2) {
            let turn = (pair[0].rotation[0] * pair[1].rotation[1] - pair[0].rotation[1] * pair[1].rotation[0]).asin();
            assert!(turn.abs() * pair[1].radii[1] <= pair[1].radii[0] * 1.01);
            assert_eq!(pair[1].previous[2..], pair[0].rotation);
        }
        let last = dabs.last().unwrap();
        assert!(last.rotation[0].abs() < 0.05 && last.rotation[1] > 0.99, "the fan settles on the barrel angle");
        let mut a = generator.characterize(rolled);
        let mut b = a;
        a.point.twist = std::f32::consts::TAU - 0.02;
        b.point.twist = 0.02;
        let middle = interpolate(a, b, 0.5);
        assert!(middle.point.twist.sin().abs() < 0.001, "wrap must take the short arc");
    }

    #[test]
    fn a_measured_barrel_sets_the_fan_while_an_unmeasured_one_faces_the_lean() {
        let pose = |twist: bool, tilt: [f32; 2], roll: f32| {
            let (brush, mut generator) = bristle_generator(twist);
            let mut sample = point(0., 0.7, 0);
            sample.tilt = tilt;
            sample.twist = roll;
            let mut dabs = Vec::new();
            generator.append(sample, &brush, &mut dabs);
            generator.last_contact.unwrap()
        };
        let facing = pose(false, [0., 0.9], 1.3);
        assert!((facing.rotation[1].atan2(facing.rotation[0]) - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        let upright = pose(true, [0.; 2], 1.3);
        assert!((upright.rotation[1].atan2(upright.rotation[0]) - 1.3).abs() < 0.001);
        let flat = pose(true, [1.1, 0.], 0.);
        let edge = pose(true, [1.1, 0.], std::f32::consts::FRAC_PI_2);
        assert!(edge.radii[1] < flat.radii[1] * 0.5, "rolling a leaning fan foreshortens it");
        let opposite = pose(true, [1.1, 0.], -std::f32::consts::FRAC_PI_2);
        assert!(edge.contact[1] > 0.5 && opposite.contact[1] < -0.5, "opposite rolls lift opposite edges");
        assert_eq!(flat.contact[1], 0.);
    }

    #[test]
    fn moving_across_the_fan_does_not_rotate_it_to_follow_the_path() {
        let (brush, _) = bristle_generator(false);
        let ending_pose = |across: bool| {
            let mut generator = DabGenerator::default();
            let mut dabs = Vec::new();
            for i in 0..20 {
                let mut sample = point(i as f32 * 30., 0.55, i * 4_000);
                sample.tilt = [0.6, 0.];
                if across { std::mem::swap(&mut sample.position.x, &mut sample.position.y); }
                generator.append(sample, &brush, &mut dabs);
            }
            generator.finish(&brush, &mut dabs);
            *dabs.last().unwrap()
        };
        let along = ending_pose(false);
        let across = ending_pose(true);
        assert_eq!(along.rotation, across.rotation);
        assert_eq!(along.radii, across.radii);
        assert_ne!(along.center, across.center);
        let off_axis = |angle: f32| angle.abs().min(std::f32::consts::PI - angle.abs());
        assert!(off_axis(along.texture_sign[1]) < 0.2, "{:?}", along.texture_sign);
        assert!((off_axis(across.texture_sign[1]) - std::f32::consts::FRAC_PI_2).abs() < 0.2, "{:?}", across.texture_sign);
    }

    #[test]
    fn a_drag_streaks_its_first_spans_along_its_travel() {
        let (mut brush, mut generator) = bristle_generator(true);
        brush.diameter = 60.;
        let mut dabs = Vec::new();
        for i in 0..=60 {
            let mut sample = point(0., (i as f32 / 10.).min(1.) * 0.7, i * 4_000);
            sample.position.y = (i as f32 - 20.).max(0.) * 2.;
            generator.append(sample, &brush, &mut dabs);
            if i == 20 {
                assert!(dabs.is_empty(), "a pressed fan may still be a tap");
                let mut preview = Vec::new();
                generator.clone().flush_contact(&brush, &mut preview);
                assert!(preview.len() > 1, "the preview shows it meanwhile");
            }
        }
        assert!(dabs.len() > 10);
        for dab in &dabs {
            assert_eq!([dab.material[1], dab.material[3]], [1.; 2], "every span of a drag streaks");
            for angle in dab.texture_sign {
                assert!((angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 0.2, "streaks follow the sideways drag: {angle}");
            }
        }
        for pair in dabs.windows(2) {
            assert_eq!(pair[1].texture_sign[0], pair[0].texture_sign[1]);
        }
    }

    #[test]
    fn a_jittery_tap_keeps_its_imprint() {
        let (brush, mut generator) = bristle_generator(true);
        let mut dabs = Vec::new();
        for i in 0..12 {
            let jitter = |salt: f32| ((i as f32 * 12.9898 + salt).sin() * 43758.547).fract() - 0.5;
            let mut sample = point(610. + jitter(1.) * 1.5, (i as f32 / 11. * std::f32::consts::PI).sin().max(0.).sqrt(), i * 4_166);
            sample.position.y = 130. + jitter(7.) * 1.5;
            sample.twist = 0.4 + jitter(3.) * 0.05;
            generator.append(sample, &brush, &mut dabs);
        }
        assert!(dabs.is_empty(), "a stroke waits until it is a tap or a drag");
        generator.finish(&brush, &mut dabs);
        assert_eq!(dabs.pop().unwrap().material[0], FAN_LIFT);
        assert!(dabs.len() > 4);
        for dab in &dabs {
            assert_eq!([dab.material[0], dab.material[1], dab.material[3]], [0.; 3], "a tap leaves an imprint");
        }
        for dab in &dabs {
            for angle in dab.texture_sign {
                let off_axis = angle.abs().min(std::f32::consts::PI - angle.abs());
                assert!(off_axis < 0.5, "jitter must not turn a tap's texture off its hairs: {angle}");
            }
        }
    }

    #[test]
    fn a_fan_lifts_off_the_paper_where_it_last_rested() {
        let (mut brush, mut generator) = bristle_generator(true);
        brush.diameter = 40.;
        let mut dabs = Vec::new();
        for i in 0..=80 {
            generator.append(point(i as f32 * 4., 0.7, i * 4_000), &brush, &mut dabs);
        }
        generator.finish(&brush, &mut dabs);
        let (lift, last) = (dabs[dabs.len() - 1], dabs[dabs.len() - 2]);
        assert!(dabs[..dabs.len() - 1].iter().all(|d| d.material[0] != FAN_LIFT));
        assert_eq!(lift.material[0], FAN_LIFT);
        assert_eq!((lift.center, lift.radii, lift.rotation, lift.contact), (last.center, last.radii, last.rotation, last.contact));
        assert_eq!(lift.motion, [0.; 2], "the fan lifts where it stopped");
        assert_eq!(lift.previous, [last.radii[0], last.radii[1], last.rotation[0], last.rotation[1]]);
        assert_eq!(lift.texture_sign, [last.texture_sign[1]; 2]);

        let (brush, mut generator) = bristle_generator(true);
        let mut tap = Vec::new();
        generator.append(point(10., 0.5, 0), &brush, &mut tap);
        generator.finish(&brush, &mut tap);
        assert_eq!(tap.len(), 2);
        assert_eq!(tap[1].material[0], FAN_LIFT, "a tap lifts its imprint");
    }

    #[test]
    fn quick_reversals_keep_the_streaks_on_the_axis_of_travel() {
        let (mut brush, mut generator) = bristle_generator(true);
        brush.diameter = 60.;
        let mut dabs = Vec::new();
        for i in 0..=240 {
            let phase = (i as f32 / 30.).fract();
            let x = 200. + 50. * (1. - (2. * phase - 1.).abs());
            let mut sample = point(x + i as f32 * 0.2, 0.7, i * 4_000);
            sample.position.y = 30. * (i as f32 * 0.02).sin();
            generator.append(sample, &brush, &mut dabs);
        }
        dabs.retain(|d| d.material[0] != FAN_LIFT);
        let turns: Vec<f32> = dabs
            .iter()
            .map(|d| (d.texture_sign[1] - d.texture_sign[0] + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI)
            .collect();
        assert!(turns.iter().all(|t| t.abs() < 0.5), "a reversal swung the streaks across the stroke within a span: {turns:?}");
        for pair in dabs.windows(2) {
            assert_eq!(pair[1].texture_sign[0], pair[0].texture_sign[1]);
        }
    }

    #[test]
    fn fan_contacts_chain_through_turns_and_the_lift() {
        let (mut brush, mut generator) = bristle_generator(false);
        brush.diameter = 30.;
        let mut dabs = Vec::new();
        for i in 0..=120 {
            let t = i as f32 / 120.;
            let mut sample = point(t * 240., 0.8, i * 4_000);
            sample.position.y = 5. * (t * std::f32::consts::PI).sin();
            generator.append(sample, &brush, &mut dabs);
        }
        generator.finish(&brush, &mut dabs);
        dabs.retain(|d| d.material[0] != FAN_LIFT);
        assert!(dabs.iter().all(|d| d.rotation[0] > 0.99), "the fan keeps its frame across turns");
        for pair in dabs.windows(2) {
            let start = [pair[1].center.x - pair[1].motion[0], pair[1].center.y - pair[1].motion[1]];
            assert!((start[0] - pair[0].center.x).abs() < 0.001 && (start[1] - pair[0].center.y).abs() < 0.001);
            assert_eq!(pair[1].previous_contact, pair[0].contact);
            assert_eq!(pair[1].texture_sign[0], pair[0].texture_sign[1], "spans share their heading at each join");
        }
        let heading = dabs[dabs.len() / 2].texture_sign[1];
        assert!(heading.abs() < 0.2, "the stroke travels along the hairs: {heading}");
    }

    #[test]
    fn identical_stroke_ids_replay_deterministically() {
        let brush = BrushSnapshot {
            mappings: std::sync::Arc::from([
                BrushMapping::pressure_size(),
                BrushMapping {
                    sensor: BrushSensor::Random,
                    target: BrushTarget::ScatterAcross,
                    combine: BrushCombine::Replace,
                    input_min: 0.0,
                    input_max: 1.0,
                    output_scale: 0.5,
                    output_bias: -0.25,
                    curve: BrushCurve::LINEAR,
                },
            ]),
            ..BrushSnapshot::default()
        };
        let points = [point(0.0, 1.0, 0), point(20.0, 1.0, 10_000)];
        let render = || {
            let mut generator = DabGenerator::default();
            generator.reset_for_stroke(StrokeId(42), &brush);
            let mut dabs = Vec::new();
            for point in points {
                generator.append(point, &brush, &mut dabs);
            }
            dabs
        };
        assert_eq!(render(), render());
    }

    #[test]
    fn stabilization_filters_position_and_pressure_before_spacing() {
        let brush = BrushSnapshot {
            stabilization: layer_core::BrushStabilization {
                strength: 1.0,
                pressure_smoothing: 1.0,
                pressure_fall_micros: 0,
                ..Default::default()
            },
            ..BrushSnapshot::default()
        };
        let mut generator = DabGenerator::default();
        generator.reset_for_stroke(StrokeId(9), &brush);
        let first = generator.stabilize(point(0.0, 0.0, 0), &brush, None);
        let second = generator.stabilize(point(100.0, 1.0, 10_000), &brush, None);
        assert_eq!(first.position.x, 0.0);
        assert!(second.position.x > 0.0 && second.position.x < 40.0);
        assert!(second.pressure > 0.0 && second.pressure < 0.1);
    }

    #[test]
    fn falling_pressure_uses_input_time_without_moving_or_forcing_the_endpoint() {
        for interval in [1000, 2000, 4000, 8000] {
            for scale in [0.02, 1., 16.] {
                for diameter in [18., 2048.] {
                    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
                    brush.stabilization.strength = 0.;
                    brush.diameter = diameter;
                    let mut generator = DabGenerator::default();
                    generator.stabilize(point(0., 0.6, 0), &brush, None);
                    for time in (interval..=16000).step_by(interval as usize) {
                        let raw = point(time as f32 * scale, 0., time);
                        let modeled = generator.stabilize(raw, &brush, None);
                        assert_eq!(modeled.position, raw.position);
                        assert_eq!(modeled.elapsed_micros, raw.elapsed_micros);
                        let seconds = time as f32 / 1_000_000.;
                        let expected =
                            0.6 - 29.29716 * (seconds - 0.004 * (1. - (-seconds / 0.004).exp()));
                        assert!((modeled.pressure - expected).abs() < 0.00001);
                    }
                    let end = generator.stabilized_input.unwrap();
                    assert!((end.pressure - 0.24628767).abs() < 0.00001);
                    // A same-position zero-pressure up cannot spend more time
                    // tapering ink that has already reached its endpoint.
                    let up = generator.stabilize(point(end.position.x, 0., 20000), &brush, None);
                    assert_eq!(up.pressure, end.pressure);
                    let rebound =
                        generator.stabilize(point(end.position.x + scale, 0.8, 24000), &brush, None);
                    assert!((rebound.pressure - 0.8).abs() < 0.000001);
                }
            }
        }
    }

    #[test]
    fn steady_light_is_literal_gradual_falls_have_bounded_lag_and_new_contacts_reset() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        brush.stabilization.strength = 0.;
        let mut generator = DabGenerator::default();
        for light in [true, false] {
            generator.reset_for_stroke(StrokeId(1), &brush);
            for i in 0..=10 {
                let pressure = if light { 0.04 } else { 0.6 - i as f32 * 0.04 };
                let raw = point(i as f32, pressure, i * 8000);
                let modeled = generator.stabilize(raw, &brush, None);
                assert_eq!(modeled.position, raw.position);
                if light {
                    assert_eq!(modeled, raw);
                } else {
                    // A 5 pressure-unit/s ramp has at most 5 * 4 ms lag.
                    assert!((raw.pressure..=raw.pressure + 0.020001).contains(&modeled.pressure));
                }
            }
        }
        generator.reset_for_stroke(StrokeId(2), &brush);
        let light_start = point(0., 0.03, 0);
        assert_eq!(generator.stabilize(light_start, &brush, None), light_start);
    }

    #[test]
    fn stationary_spray_is_time_based_and_replayable() {
        let brush = BrushSnapshot {
            path: layer_core::BrushPath {
                continuous_rate_hz: 100.0,
                ..layer_core::BrushPath::default()
            },
            ..BrushSnapshot::default()
        };
        let render = || {
            let mut generator = DabGenerator::default();
            generator.reset_for_stroke(StrokeId(13), &brush);
            let mut dabs = Vec::new();
            generator.append(point(20.0, 0.7, 0), &brush, &mut dabs);
            generator.append(point(20.0, 0.7, 100_000), &brush, &mut dabs);
            dabs
        };
        let first = render();
        assert_eq!(first.len(), 11);
        assert_eq!(first, render());
    }

    #[test]
    fn curve_completion_does_not_add_spray_time_or_change_release_sensors() {
        let mut brush = BrushSnapshot {
            path: layer_core::BrushPath { continuous_rate_hz: 100., ..Default::default() },
            ..Default::default()
        };

        brush.stabilization.strength = 1.;
        let mut generator = DabGenerator::default();
        let mut dabs = Vec::new();
        generator.reset_for_stroke(StrokeId(13), &brush);
        generator.append(point(20., 0.7, 0), &brush, &mut dabs);
        generator.append(point(20., 0.7, 100_000), &brush, &mut dabs);
        generator.finish(&brush, &mut dabs);
        assert_eq!(dabs.len(), 11);
        assert_eq!(generator.last.unwrap().point.elapsed_micros, 100_000);

        brush.path.continuous_rate_hz = 0.;
        brush.stabilization.pressure_fall_micros = 50_000;
        generator.reset_for_stroke(StrokeId(14), &brush);
        dabs.clear();
        generator.append(point(0., 0.7, 0), &brush, &mut dabs);
        generator.append(point(100., 0.7, 4000), &brush, &mut dabs);
        generator.append(point(100., 0., 5000), &brush, &mut dabs);
        let previous = generator.stabilized_input.unwrap();
        let speed = generator.filtered_speed;
        generator.finish(&brush, &mut dabs);
        let last = generator.last.unwrap().point;
        assert_eq!(last.position.x, 100.);
        assert_eq!(last.elapsed_micros, previous.elapsed_micros);
        assert_eq!(last.pressure, previous.pressure);
        assert_eq!(last.tilt, previous.tilt);
        assert_eq!(last.twist, previous.twist);
        assert_eq!(generator.filtered_speed, speed);
        let completed = dabs.clone();
        generator.finish(&brush, &mut dabs);
        assert_eq!(dabs, completed);
    }

    #[test]
    fn paint_charge_depletes_deterministically_with_distance() {
        let brush = BrushSnapshot {
            diameter: 100.0,
            spacing: 0.1,
            wet_mix: layer_core::BrushWetMix {
                charge: 1.0,
                charge_depletion: 1.0,
                ..layer_core::BrushWetMix::default()
            },
            ..BrushSnapshot::default()
        };
        let mut generator = DabGenerator::default();
        generator.reset_for_stroke(StrokeId(17), &brush);
        let mut dabs = Vec::new();
        generator.append(point(0.0, 1.0, 0), &brush, &mut dabs);
        generator.append(point(220.0, 1.0, 20_000), &brush, &mut dabs);
        assert!(dabs.last().unwrap().material[2] < dabs.first().unwrap().material[2] * 0.2);
    }

    #[test]
    fn dry_pose_keys_use_physical_inclination_independent_of_azimuth() {
        use layer_core::DefaultBrushPreset as P;
        use crate::input::{to_stroke_point, PressureCurve, ViewTransform};
        for preset in [P::Pencil, P::PointyPencil, P::ShadingPencil, P::Charcoal,
            P::Chalk, P::PastelBlock, P::Crayon] {
            let brush = layer_core::default_brush(preset);
            let dry = brush.contact.unwrap().dry.unwrap();
            for (key, degrees) in [0f32, 40., 60.].into_iter().enumerate() {
                let inclination = degrees.to_radians();
                for azimuth in [-135f32, -45., 0., 30., 45., 90.] {
                    let (sin, cos) = azimuth.to_radians().sin_cos();
                    let mut event = crate::test_support::event(1, crate::PenPhase::Hover, 20.);
                    event.pressure = 1.;
                    event.tilt_radians = [(inclination.sin()*cos).atan2(inclination.cos()),
                        (inclination.sin()*sin).atan2(inclination.cos())];
                    let point = to_stroke_point(event, ViewTransform::IDENTITY, PressureCurve::default(), 0);
                    let mut generator = DabGenerator::default();
                    let pose = generator.characterize(point);
                    let dab = generator.evaluate(pose, &brush).dab;
                    for (actual, expected) in dab.radii.into_iter().zip(dry.poses[key].radii) {
                        assert!((actual - expected*brush.diameter*0.5).abs() < 1e-5,
                            "{preset:?}: {degrees}° at azimuth {azimuth} uses pose {key}: {dab:?}");
                    }
                    for (actual, expected) in dab.contact[1..].iter().zip(dry.poses[key].relief) {
                        assert!((actual - expected).abs() < 1e-6, "{preset:?}: pose relief");
                    }
                    if key > 0 {
                        assert!((dab.rotation[0]-cos).abs() < 1e-6 && (dab.rotation[1]-sin).abs() < 1e-6,
                            "{preset:?}: pose azimuth {azimuth}");
                        assert_eq!(dab.material[3], 1.);
                    }
                }
            }
        }
    }

    #[test]
    fn dry_strength_scales_calibrated_material_pressure_without_changing_geometry() {
        use layer_core::{DefaultBrushPreset, StrokeTool};
        for preset in [DefaultBrushPreset::Pencil, DefaultBrushPreset::PointyPencil,
            DefaultBrushPreset::ShadingPencil, DefaultBrushPreset::Charcoal,
            DefaultBrushPreset::Chalk, DefaultBrushPreset::PastelBlock, DefaultBrushPreset::Crayon] {
            for (pressure, gain, strength, expected) in [(1., 1., 0.25, 0.25),
                (0.4, 0.75, 0.5, 0.15), (0.8, 1., 1., 0.8), (0., 1., 0.5, 0.)] {
                let mut brush = layer_core::default_brush(preset);
                brush.flow = strength;
                brush.contact.as_mut().unwrap().pressure_gain = gain;
                let mut input = point(20., pressure, 0);
                input.tilt = [std::f32::consts::FRAC_PI_4, 0.];
                let mut generator = DabGenerator::default();
                let current = generator.characterize(input);
                let actual = generator.evaluate(current, &brush).dab;
                brush.flow = 1.;
                let full = DabGenerator::default().evaluate(current, &brush).dab;
                assert!((actual.contact[0] - expected).abs() < 0.000001, "{preset:?}: {actual:?}");
                assert_eq!(actual.flow, 1., "{preset:?}: material pressure is the only Strength path");
                assert_eq!(actual.radii, full.radii, "{preset:?}: size uses physical pressure");
                assert_eq!(actual.rotation, full.rotation);
                assert_eq!(actual.contact[1..], full.contact[1..]);
                assert_eq!(layer_render::DabStyle::for_brush(&brush, StrokeTool::Brush)
                    .contact.unwrap().pressure_gain, gain);
                brush.flow = strength;
                let mut mappings = brush.mappings.to_vec();
                mappings.push(BrushMapping { target: BrushTarget::Flow, output_scale: 0.,
                    output_bias: 0.25, ..BrushMapping::pressure_size() });
                brush.mappings = mappings.into();
                let mapped = DabGenerator::default().evaluate(current, &brush).dab;
                assert!((mapped.contact[0] - expected * 0.25).abs() < 0.000001);
                assert_eq!(mapped.flow, 1.);
                assert_eq!(mapped.radii, actual.radii);
            }
        }
    }

    #[test]
    fn calibrated_dry_maximum_matches_prior_seventy_percent_material_pressure() {
        use layer_core::{DefaultBrushPreset as P,StrokeTool};
        for (preset,prior_gain) in [(P::Pencil,1.),(P::PointyPencil,1.),(P::ShadingPencil,0.9),(P::Crayon,1.)] {
            let mut current=layer_core::default_brush(preset);current.flow=1.;
            assert!(current.contact.unwrap().dry.unwrap().loading<=1.);
            assert!((current.contact.unwrap().pressure_gain-prior_gain*0.7).abs()<0.000001,"{preset:?}: calibrated maximum");
            let mut prior=current.clone();prior.flow=0.7;prior.contact.as_mut().unwrap().pressure_gain=prior_gain;
            for pressure in [0.,0.15,0.4,0.75,1.] {for tilt in [0.,0.25,0.5,0.85,1.] {for azimuth in [0,1] {
                let mut input=point(20.,pressure,0);input.tilt=[0.;2];input.tilt[azimuth]=tilt*std::f32::consts::FRAC_PI_2;
                let mut a=DabGenerator::default();let pose=a.characterize(input);let actual=a.evaluate(pose,&current).dab;
                let mut expected=DabGenerator::default().evaluate(pose,&prior).dab;
                assert!((actual.contact[0]-expected.contact[0]).abs()<0.000001,"{preset:?} p={pressure} tilt={tilt}: material pressure");
                assert_eq!(actual.flow,1.);assert_eq!(expected.flow,1.);
                expected.contact[0]=actual.contact[0];assert_eq!(actual,expected,"{preset:?}: physical geometry and all other contact fields");
                assert_eq!(layer_render::DabStyle::for_brush(&current,StrokeTool::Brush).contact.unwrap().pressure_gain,current.contact.unwrap().pressure_gain);
                assert_eq!(layer_render::DabStyle::for_brush(&prior,StrokeTool::Brush).contact.unwrap().pressure_gain,prior_gain);
            }}}
        }
    }
}
