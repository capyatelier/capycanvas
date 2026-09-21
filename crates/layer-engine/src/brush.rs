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

const SPEED_FILTER_SECONDS: f32 = 0.015;
const PRESSURE_FALL_RESPONSE_SECONDS: f32 = 0.004;
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
/// live rendering and transient stroke correction produce the same variation.
/// Colors are straight linear document RGB; HSL dynamics use its encoded RGB.
#[derive(Clone, Debug)]
pub struct DabGenerator {
    space: RgbSpace,
    last: Option<DynamicPoint>,
    stabilized_input: Option<StrokePoint>,
    pressure_fall_target: f32,
    distance_until_next: f32,
    filtered_speed: f32,
    rng: u32,
    stroke_seed: u32,
    last_evaluated: Option<DynamicPoint>,
    last_emitted_dab: Option<Dab>,
    total_distance: Option<f32>,
    continuous_fraction: f32,
}

impl Default for DabGenerator {
    fn default() -> Self {
        Self {
            space: RgbSpace::Srgb,
            last: None,
            stabilized_input: None,
            pressure_fall_target: 0.0,
            distance_until_next: 0.0,
            filtered_speed: 0.0,
            rng: 1,
            stroke_seed: 1,
            last_evaluated: None,
            last_emitted_dab: None,
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
        let mut preview = self.clone();
        let evaluated = preview.evaluate(current, brush);
        let mut output = Vec::new();
        let mut damage = Rect::EMPTY;
        preview.emit_contacts(evaluated, brush, &mut output, &mut damage);
        self.last = Some(current);
        output
    }
    pub fn reset(&mut self) {
        *self = Self::new(self.space);
    }

    pub fn reset_for_stroke(&mut self, stroke_id: StrokeId, brush: &BrushSnapshot) {
        self.last = None;
        self.stabilized_input = None;
        self.pressure_fall_target = 0.0;
        self.distance_until_next = 0.0;
        self.filtered_speed = 0.0;
        self.rng = mix_seed(brush.seed, stroke_id.0);
        self.stroke_seed = self.rng;
        self.last_evaluated = None;
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
        let radius = self.last_emitted_dab.map_or(brush.diameter * 0.5, |dab| {
            dab.radii[0].min(dab.radii[1])
        });
        let tolerance = (radius * 0.01).max(0.25);
        let traveled = f64::from((current.stroke_distance - start.stroke_distance).max(0.));
        let dx = f64::from(current.position().x) - f64::from(start.position().x);
        let dy = f64::from(current.position().y) - f64::from(start.position().y);
        // Every intervening vertex lies inside the ellipse whose focal points
        // are the endpoints and whose major axis is the traveled path length.
        // Its minor radius bounds the error without retaining a point list.
        let error2 = (traveled * traveled - dx * dx - dy * dy).max(0.) * 0.25;
        let pressure_change = (current.point.pressure - start.point.pressure).abs()
            * brush.diameter * 0.5;
        let tilt_change = (current.point.tilt[0] - start.point.tilt[0])
            .hypot(current.point.tilt[1] - start.point.tilt[1]);
        let twist_change = (current.point.twist - start.point.twist + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let pose_changed = pressure_change > tolerance
            || tilt_change > 0.04
            || twist_change.abs() > 0.04;
        // Keep the pose before a pressure/tilt transition just as we keep the
        // vertex before a bend. Otherwise a release narrows an earlier span.
        if (error2 > f64::from(tolerance).powi(2) || pose_changed)
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
        if current.stroke_distance - start.stroke_distance >= (brush.diameter * 0.5).max(4.)
            || pose_changed
        {
            let evaluated = self.evaluate(current, brush);
            self.emit_contacts(evaluated, brush, output, damage);
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
    pub(crate) fn finish(&mut self, brush: &BrushSnapshot, output: &mut Vec<Dab>) -> Rect {
        if brush.contact.is_none() {
            return Rect::EMPTY;
        }
        let (Some(last), Some(dab)) = (self.last, self.last_emitted_dab) else {
            return Rect::EMPTY;
        };
        if (last.position().x - dab.center.x).hypot(last.position().y - dab.center.y) <= 0.0001 {
            return Rect::EMPTY;
        }
        let evaluated = self.evaluate(last, brush);
        let mut damage = Rect::EMPTY;
        self.emit_contacts(evaluated, brush, output, &mut damage);
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
        let modeled = if dab.previous[0] > 0.0 {
            dab.previous = [dab.radii[0], dab.radii[1], dab.rotation[0], dab.rotation[1]];
            dab.previous_contact = dab.contact;
            dab.center
        } else {
            self.modeled_position().unwrap_or(endpoint)
        };
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
            self.pressure_fall_target = point.pressure;
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
            position: Point {
                x: last.position.x + (point.position.x - last.position.x) * response,
                y: last.position.y + (point.position.y - last.position.y) * response,
            },
            pressure,
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
        let taper_reference = if brush.contact.is_some() {
            brush.diameter
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
        if let Some(contact) = brush.contact {
            let spread = contact.tilt_spread * tilt * tilt;
            radii[0] *= 1.0 + spread;
            radii[1] *= 1.0 + spread * 0.18;
            if contact.tilt_spread > 0.0 && tilt > 0.001 {
                rotation = tilt_direction + brush.angle_radians;
            }
        }
        let (rotation_sin, rotation_cos) = rotation.sin_cos();
        let motion = self.last_evaluated.map_or([0.0; 2], |last| {
            [point.position().x - last.position().x, point.position().y - last.position().y]
        });
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
                flow: finite_or(values.flow, brush.flow).clamp(0.0, 1.0),
                hardness: finite_or(values.hardness, brush.hardness).clamp(0.0, 1.0),
                texture_sign: [1.0, 1.0],
                material: [
                    values.grain_depth.clamp(0.0, 1.0),
                    values.pull.clamp(0.0, 1.0),
                    (values.deposit * charge).clamp(0.0, 1.0),
                    values.deform_strength.clamp(0.0, 1.0),
                ],
                previous: [0.0; 4],
                contact: [
                    point.point.pressure,
                    tilt,
                    point.stroke_distance / brush.diameter.max(0.01),
                    (self.stroke_seed & 65535) as f32,
                ],
                previous_contact: [0.0; 4],
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
                self.space,
                brush.color_rgba_linear,
                evaluated.values,
                brush,
                variant,
                self.stroke_seed,
            );
            if brush.contact.is_some() {
                let previous = self.last_emitted_dab.unwrap_or(dab);
                dab.previous = [
                    previous.radii[0],
                    previous.radii[1],
                    previous.rotation[0],
                    previous.rotation[1],
                ];
                dab.previous_contact = previous.contact;
                dab.motion = [
                    dab.center.x - previous.center.x,
                    dab.center.y - previous.center.y,
                ];
            }
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
    fn contact_lift_closes_the_spacing_gap_and_replays_the_same_point() {
        let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        // Test explicit terminal pressure independently of pressure stabilization.
        brush.stabilization.pressure_fall_micros = 0;
        let stroke = Stroke::new(
            StrokeId(17),
            layer_core::LayerId(1),
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
    fn swept_input_does_not_expand_a_fast_light_stroke_into_stamps() {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
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
                pressure_fall_micros: 0,
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
    fn falling_pressure_uses_input_time_without_moving_or_forcing_the_endpoint() {
        for interval in [1000, 2000, 4000, 8000] {
            for scale in [0.02, 1., 16.] {
                for diameter in [18., 2048.] {
                    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
                    brush.diameter = diameter;
                    let mut generator = DabGenerator::default();
                    generator.stabilize(point(0., 0.6, 0), &brush);
                    for time in (interval..=16000).step_by(interval as usize) {
                        let raw = point(time as f32 * scale, 0., time);
                        let modeled = generator.stabilize(raw, &brush);
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
                    let up = generator.stabilize(point(end.position.x, 0., 20000), &brush);
                    assert_eq!(up.pressure, end.pressure);
                    let rebound =
                        generator.stabilize(point(end.position.x + scale, 0.8, 24000), &brush);
                    assert!((rebound.pressure - 0.8).abs() < 0.000001);
                }
            }
        }
    }

    #[test]
    fn steady_light_is_literal_gradual_falls_have_bounded_lag_and_new_contacts_reset() {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let mut generator = DabGenerator::default();
        for light in [true, false] {
            generator.reset_for_stroke(StrokeId(1), &brush);
            for i in 0..=10 {
                let pressure = if light { 0.04 } else { 0.6 - i as f32 * 0.04 };
                let raw = point(i as f32, pressure, i * 8000);
                let modeled = generator.stabilize(raw, &brush);
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
        assert_eq!(generator.stabilize(light_start, &brush), light_start);
    }

    #[test]
    fn repeated_pressure_reports_do_not_make_a_staircase_in_falling_size() {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let size = |pressure| {
            let mapping = &brush.mappings[0];
            mapping.curve.sample(pressure) * mapping.output_scale + mapping.output_bias
        };
        let mut generator = DabGenerator::default();
        let mut previous = 0.8_f32;
        let mut old_sizes = vec![size(previous); 2];
        let mut sizes = old_sizes.clone();
        for i in 0..=20 {
            // Wacom position reports at ~4 ms, pressure changes at ~8 ms.
            let raw = point(i as f32 * 20., 0.8 - (i / 2) as f32 * 0.025, i * 4000);
            previous = raw.pressure.max(previous - 0.05);
            old_sizes.push(size(previous));
            sizes.push(size(generator.stabilize(raw, &brush).pressure));
        }
        let curvature = |values: &[f32]| {
            values.windows(3)
                .map(|w| (w[2] - 2. * w[1] + w[0]).abs())
                .fold(0_f32, f32::max)
        };
        assert!(curvature(&sizes) < curvature(&old_sizes) * 0.4);
        // Once the fall begins, repeated raw readings must keep narrowing
        // smoothly instead of alternating a shrinking span and a flat span.
        assert!(sizes[4..].windows(2).all(|w| w[1] < w[0]));
    }

    #[test]
    fn abrupt_lift_bounds_the_change_in_gpen_size_velocity() {
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let mapping = &brush.mappings[0];
        let size = |pressure| {
            mapping.curve.sample(pressure) * mapping.output_scale + mapping.output_bias
        };
        for interval in [1000, 2000, 4000, 8000] {
            let mut generator = DabGenerator::default();
            generator.stabilize(point(0., 0.8, 0), &brush);
            let mut sizes = vec![size(0.8); 2];
            for time in (interval..=64000).step_by(interval as usize) {
                let modeled = generator.stabilize(point(time as f32, 0., time), &brush);
                sizes.push(size(modeled.pressure));
            }
            let dt = interval as f32 / 1_000_000.;
            let max_acceleration = sizes.windows(3)
                .map(|w| (w[2] - 2. * w[1] + w[0]).abs() / (dt * dt))
                .fold(0_f32, f32::max);
            // Check resolved G-Pen size, including its nonlinear sampled curve,
            // rather than claiming a pressure bound is also a size bound.
            // With the 34.133 ms fall limit and unchanged 4 ms response, the
            // pressure acceleration ceiling is about 7324.29 units/s². This
            // fixture's sampled size acceleration stays below 7500/s².
            assert!(
                max_acceleration < 7500.,
                "dt={dt}, size acceleration={max_acceleration}"
            );
        }
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
