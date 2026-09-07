//! Deterministic brush dynamics compiled into renderer-ready dabs.
//!
//! The sequential, low-volume work stays here on the CPU: input-derived
//! sensors, curve evaluation, distance-based resampling, and deterministic
//! variation. Render backends receive only resolved, fixed-size dab geometry.

use layer_core::{
    BrushCombine, BrushMapping, BrushSensor, BrushSnapshot, BrushTarget, MAX_BRUSH_DIAMETER,
    MAX_BRUSH_MAPPINGS, MAX_BRUSH_SCATTER_DIAMETERS, Point, Rect, Stroke, StrokeId, StrokePoint,
};
use layer_render::Dab;

const SPEED_FILTER_SECONDS: f32 = 0.015;
const MIN_DAB_DISTANCE: f32 = 0.25;
const MAX_DABS_PER_SEGMENT: usize = 65_536;

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
/// live rendering and later replay produce the same variation.
#[derive(Clone, Debug)]
pub struct DabGenerator {
    last: Option<DynamicPoint>,
    stabilized_input: Option<StrokePoint>,
    distance_until_next: f32,
    filtered_speed: f32,
    rng: u32,
    stroke_seed: u32,
    last_contact_position: Option<Point>,
    last_emitted_dab: Option<Dab>,
    total_distance: Option<f32>,
    continuous_fraction: f32,
}

impl Default for DabGenerator {
    fn default() -> Self {
        Self {
            last: None,
            stabilized_input: None,
            distance_until_next: 0.0,
            filtered_speed: 0.0,
            rng: 1,
            stroke_seed: 1,
            last_contact_position: None,
            last_emitted_dab: None,
            total_distance: None,
            continuous_fraction: 0.0,
        }
    }
}

impl DabGenerator {
    pub(crate) fn cursor_seed(&mut self, id: StrokeId, brush: &BrushSnapshot) {
        self.rng = mix_seed(brush.seed, id.0);
        self.stroke_seed = self.rng;
    }
    /// Resolve a cursor contact through the painting dynamics without advancing
    /// spacing, random state or committed input. Hover updates sensors only.
    pub fn cursor_contacts(&mut self, point: StrokePoint, brush: &BrushSnapshot) -> Vec<Dab> {
        let current = self.characterize(point);
        let mut preview = self.clone();
        let evaluated = preview.evaluate(current, brush);
        let mut output = Vec::new();
        let mut damage = Rect::EMPTY;
        preview.emit_contacts(evaluated, brush, &mut output, &mut damage);
        self.last = Some(current);
        output
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn reset_for_stroke(&mut self, stroke_id: StrokeId, brush: &BrushSnapshot) {
        self.last = None;
        self.stabilized_input = None;
        self.distance_until_next = 0.0;
        self.filtered_speed = 0.0;
        self.rng = mix_seed(brush.seed, stroke_id.0);
        self.stroke_seed = self.rng;
        self.last_contact_position = None;
        self.last_emitted_dab = None;
        self.total_distance = None;
        self.continuous_fraction = 0.0;
    }

    pub fn reset_for_replay(&mut self, stroke: &Stroke) {
        self.reset_for_stroke(stroke.id, &stroke.brush);
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
        let stabilized = self.stabilize(point, brush);
        let current = self.characterize(stabilized);
        let mut damage = Rect::EMPTY;
        let Some(last) = self.last else {
            let evaluated = self.evaluate(current, brush);
            self.emit_contacts(evaluated, brush, output, &mut damage);
            self.last = Some(current);
            self.distance_until_next = spacing_for(evaluated.dab, evaluated.spacing);
            return damage;
        };

        let dx = current.point.position.x - last.point.position.x;
        let dy = current.point.position.y - last.point.position.y;
        let distance = dx.hypot(dy);
        if distance <= f32::EPSILON {
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

    pub fn generate(stroke: &Stroke, output: &mut Vec<Dab>) -> Rect {
        let mut generator = Self::default();
        generator.reset_for_replay(stroke);
        let mut damage = Rect::EMPTY;
        for point in stroke.points.iter().copied() {
            damage = damage.union(generator.append(point, &stroke.brush, output));
        }
        damage
    }

    pub(crate) fn modeled_position(&self) -> Option<Point> {
        self.last.map(|point| point.point.position)
    }

    /// Add one preview-only contact centered exactly at the requested endpoint
    /// without advancing the committed generator or its random sequence.
    pub(crate) fn append_terminal_copy(&self, endpoint: Point, output: &mut Vec<Dab>) -> Rect {
        let Some(mut dab) = self.last_emitted_dab else {
            return Rect::EMPTY;
        };
        let modeled = self.modeled_position().unwrap_or(endpoint);
        dab.center = endpoint;
        dab.motion = [endpoint.x - modeled.x, endpoint.y - modeled.y];
        let mut damage = Rect::EMPTY;
        include_dab(&mut damage, dab);
        output.push(dab);
        damage
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

    fn stabilize(&mut self, point: StrokePoint, brush: &BrushSnapshot) -> StrokePoint {
        let Some(last) = self.stabilized_input else {
            self.stabilized_input = Some(point);
            return point;
        };
        let elapsed = point.elapsed_micros.saturating_sub(last.elapsed_micros) as f32 / 1_000_000.0;
        let raw_distance =
            (point.position.x - last.position.x).hypot(point.position.y - last.position.y);
        let speed = if elapsed > 0.0 {
            raw_distance / elapsed
        } else {
            0.0
        };
        let slow_motion = 1.0 - (speed / 1_500.0).clamp(0.0, 1.0);
        let retained = (brush.stabilization.streamline * 0.72
            + brush.stabilization.stabilization * 0.20
            + brush.stabilization.motion_filtering * slow_motion * 0.24)
            .clamp(0.0, 0.96);
        let response = (1.0 - retained)
            .powf(brush.stabilization.expression.max(0.05))
            .clamp(0.04, 1.0);
        let pressure_response =
            (1.0 - brush.stabilization.pressure_smoothing * 0.92).clamp(0.04, 1.0);
        let stabilized = StrokePoint {
            position: Point {
                x: last.position.x + (point.position.x - last.position.x) * response,
                y: last.position.y + (point.position.y - last.position.y) * response,
            },
            pressure: last.pressure + (point.pressure - last.pressure) * pressure_response,
            tilt: [
                last.tilt[0] + (point.tilt[0] - last.tilt[0]) * response,
                last.tilt[1] + (point.tilt[1] - last.tilt[1]) * response,
            ],
            twist: mix_angle(last.twist, point.twist, response, std::f32::consts::TAU),
            elapsed_micros: point.elapsed_micros,
        };
        self.stabilized_input = Some(stabilized);
        stabilized
    }

    fn evaluate(&mut self, point: DynamicPoint, brush: &BrushSnapshot) -> EvaluatedDab {
        let variant = self.next_random();
        let mut values = Registers {
            diameter: brush.diameter,
            opacity: brush.opacity,
            flow: brush.flow,
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
            apply_mapping(
                &mut values,
                mapping,
                sensor_value(point, mapping.sensor, variant, index as u32),
            );
        }

        let mut diameter = finite_or(values.diameter, brush.diameter)
            .clamp(brush.bounds.minimum_size, brush.bounds.maximum_size);
        let (taper_size, taper_opacity) =
            taper_factors(point.stroke_distance, self.total_distance, diameter, brush);
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
        let rotation = finite_or(values.rotation, brush.angle_radians)
            + direction * brush.shape.follow_direction
            + tilt_direction * brush.shape.follow_tilt
            + point.point.twist * brush.shape.follow_twist;
        let (rotation_sin, rotation_cos) = rotation.sin_cos();
        let motion = self.last_contact_position.map_or([0.0; 2], |last| {
            [point.position().x - last.x, point.position().y - last.y]
        });
        self.last_contact_position = Some(point.position());
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
        EvaluatedDab {
            dab: Dab {
                center,
                radii: [diameter * 0.5, diameter * 0.5 / aspect],
                rotation: [rotation_cos, rotation_sin],
                motion,
                color_rgba_linear: resolve_color(
                    brush.color_rgba_linear,
                    values,
                    brush,
                    variant,
                    self.stroke_seed,
                ),
                flow: finite_or(values.flow, brush.flow).clamp(0.0, 1.0),
                hardness: finite_or(values.hardness, brush.hardness).clamp(0.0, 1.0),
                texture_sign: [1.0, 1.0],
                material: [
                    values.grain_depth.clamp(0.0, 1.0),
                    values.pull.clamp(0.0, 1.0),
                    (values.deposit * charge).clamp(0.0, 1.0),
                    values.deform_strength.clamp(0.0, 1.0),
                ],
            },
            spacing: (finite_or(values.spacing, brush.spacing)
                * (1.0 + signed_unit(hash32(variant ^ 0x7f4a_7c15)) * brush.path.spacing_jitter))
                .max(0.005),
            direction_radians: direction,
            values,
        }
    }

    fn emit_contacts(
        &mut self,
        evaluated: EvaluatedDab,
        brush: &BrushSnapshot,
        output: &mut Vec<Dab>,
        damage: &mut Rect,
    ) {
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
            let along = signed_unit(hash32(variant ^ 0x68bc_21eb))
                * brush.path.jitter_along.min(MAX_BRUSH_SCATTER_DIAMETERS)
                * diameter;
            let across = signed_unit(hash32(variant ^ 0x02e5_be93))
                * brush.path.jitter_across.min(MAX_BRUSH_SCATTER_DIAMETERS)
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
                brush.color_rgba_linear,
                evaluated.values,
                brush,
                variant,
                self.stroke_seed,
            );
            include_dab(damage, dab);
            self.last_emitted_dab = Some(dab);
            output.push(dab);
        }
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
pub(crate) fn lock_dab_tail(
    dabs: &mut [Dab],
    modeled_endpoint: Point,
    target: Point,
    strength: f32,
    easing: f32,
) {
    if dabs.is_empty() || strength <= 0.0 {
        return;
    }
    let correction = Point {
        x: (target.x - modeled_endpoint.x) * strength,
        y: (target.y - modeled_endpoint.y) * strength,
    };
    let count = dabs.len();
    let last = count.saturating_sub(1).max(1) as f32;
    let mut previous_weight = 0.0;
    for (index, dab) in dabs.iter_mut().enumerate() {
        let linear = if count == 1 { 1.0 } else { index as f32 / last };
        let smooth = linear * linear * (3.0 - 2.0 * linear);
        let weight = smooth.powf(easing);
        dab.center.x += correction.x * weight;
        dab.center.y += correction.y * weight;
        dab.motion[0] += correction.x * (weight - previous_weight);
        dab.motion[1] += correction.y * (weight - previous_weight);
        previous_weight = weight;
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

fn mix_angle(a: f32, b: f32, t: f32, period: f32) -> f32 {
    let delta = (b - a + period * 0.5).rem_euclid(period) - period * 0.5;
    (a + delta * t).rem_euclid(period)
}

fn spacing_for(dab: Dab, spacing: f32) -> f32 {
    (dab.radii[0] * 2.0 * spacing.max(0.005)).max(MIN_DAB_DISTANCE)
}

fn include_dab(damage: &mut Rect, dab: Dab) {
    let [cos, sin] = dab.rotation;
    let extent_x = (dab.radii[0] * cos).hypot(dab.radii[1] * sin) + 1.0;
    let extent_y = (dab.radii[0] * sin).hypot(dab.radii[1] * cos) + 1.0;
    damage.min.x = damage.min.x.min(dab.center.x - extent_x);
    damage.min.y = damage.min.y.min(dab.center.y - extent_y);
    damage.max.x = damage.max.x.max(dab.center.x + extent_x);
    damage.max.y = damage.max.y.max(dab.center.y + extent_y);
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
        let smooth = progress * progress * (3.0 - 2.0 * progress);
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
        mix(primary[0], secondary[0], secondary_mix),
        mix(primary[1], secondary[1], secondary_mix),
        mix(primary[2], secondary[2], secondary_mix),
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
        let srgb = linear.map(linear_to_srgb);
        let (mut h, mut s, mut l) = rgb_to_hsl(srgb);
        h = (h + hue).rem_euclid(1.0);
        s = (s + saturation).clamp(0.0, 1.0);
        l = (l + lightness).clamp(0.0, 1.0);
        linear = hsl_to_rgb(h, s, l).map(srgb_to_linear);
    }
    [
        linear[0],
        linear[1],
        linear[2],
        mix(primary[3], secondary[3], secondary_mix) * values.opacity.clamp(0.0, 1.0),
    ]
}

fn rgb_to_hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
    let minimum = rgb[0].min(rgb[1]).min(rgb[2]);
    let lightness = (maximum + minimum) * 0.5;
    let delta = maximum - minimum;
    if delta <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(f32::EPSILON);
    let hue_sector = if maximum == rgb[0] {
        ((rgb[1] - rgb[2]) / delta).rem_euclid(6.0)
    } else if maximum == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    (hue_sector / 6.0, saturation, lightness)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
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

fn linear_to_srgb(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_linear(srgb: f32) -> f32 {
    if srgb <= 0.040_45 {
        srgb / 12.92
    } else {
        ((srgb + 0.055) / 1.055).powf(2.4)
    }
}

fn mix(a: f32, b: f32, amount: f32) -> f32 {
    a + (b - a) * amount
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
mod tests {
    use super::*;
    use layer_core::{BrushCurve, BrushTip};

    fn point(x: f32, pressure: f32, micros: u32) -> StrokePoint {
        StrokePoint {
            position: Point { x, y: 0.0 },
            pressure,
            tilt: [0.0; 2],
            twist: 0.0,
            elapsed_micros: micros,
        }
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
            },
        );
        assert_eq!(damage.min, Point { x: 18.0, y: 19.0 });
        assert_eq!(damage.max, Point { x: 22.0, y: 41.0 });
    }

    #[test]
    fn pressure_mapping_is_resolved_before_backend_submission() {
        let brush = BrushSnapshot::default();
        assert_eq!(brush.tip, BrushTip::AnalyticEllipse);
        let mut generator = DabGenerator::default();
        generator.reset_for_stroke(StrokeId(7), &brush);
        let mut dabs = Vec::new();
        generator.append(point(0.0, 0.0, 0), &brush, &mut dabs);
        generator.append(point(20.0, 1.0, 10_000), &brush, &mut dabs);
        assert!(dabs.first().unwrap().radii[0] < dabs.last().unwrap().radii[0]);
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
                streamline: 1.0,
                pressure_smoothing: 1.0,
                stabilization: 0.5,
                motion_filtering: 0.5,
                expression: 1.0,
            },
            ..BrushSnapshot::default()
        };
        let mut generator = DabGenerator::default();
        generator.reset_for_stroke(StrokeId(9), &brush);
        let first = generator.stabilize(point(0.0, 0.0, 0), &brush);
        let second = generator.stabilize(point(100.0, 1.0, 10_000), &brush);
        assert_eq!(first.position.x, 0.0);
        assert!(second.position.x > 0.0 && second.position.x < 40.0);
        assert!(second.pressure > 0.0 && second.pressure < 0.1);
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
}
