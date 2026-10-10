//! Platform-neutral input records and the engine ingress queue.
//!
//! A platform adapter pushes complete event-history batches into a bounded SPSC
//! queue without locking or allocating. The engine consumes those events,
//! maps physical-surface coordinates through the matching view transform, and
//! keeps predicted points separate from document truth.

use layer_core::{Point, StrokePoint};
use rtrb::{Consumer, Producer, PushError, RingBuffer};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum PenPhase {
    Hover,
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ToolKind {
    Pen,
    Eraser,
    Brush,
    Pencil,
    Airbrush,
    Finger,
    Mouse,
    Unknown,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SampleFlags(pub u16);

impl SampleFlags {
    pub const NONE: Self = Self(0);
    pub const PREDICTED: Self = Self(1 << 0);
    pub const PRIMARY: Self = Self(1 << 1);
    pub const INVERTED: Self = Self(1 << 3);
    /// A later correction may replace this real sample. `sequence` is its
    /// contact-local token until the final correction releases it.
    pub const ESTIMATED: Self = Self(1 << 4);
    /// Replace the registered sample with this token; never append a point,
    /// route a pointer action, or use the correction's delivery-time camera.
    pub const CORRECTION: Self = Self(1 << 5);
    /// The host confirmed that this pen controls an indirect, screenless tablet.
    /// This affects cursor presentation only; the device is still a pen.
    pub const INDIRECT_POINTER: Self = Self(1 << 6);
    /// `twist_radians` is a measured barrel rotation, not a missing axis.
    pub const BARREL_TWIST: Self = Self(1 << 7);

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

/// Stable C-compatible event written directly by platform adapters.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct PenEvent {
    pub device_id: u64,
    pub sequence: u64,
    /// Monotonic platform timestamp converted to nanoseconds.
    pub timestamp_ns: u64,
    /// Identifies the exact screen-to-document transform seen by the callback.
    pub view_revision: u64,
    /// Position in physical surface pixels, never logical UI points.
    pub surface_position: Point,
    pub pressure: f32,
    pub tilt_radians: [f32; 2],
    pub twist_radians: f32,
    pub distance: f32,
    pub phase: PenPhase,
    pub tool: ToolKind,
    pub flags: SampleFlags,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(from = "layer_core::PressureResponse", into = "layer_core::PressureResponse")]
pub struct PressureCurve(Arc<PressureMapping>);

#[derive(Debug, PartialEq)]
struct PressureMapping { source: layer_core::PressureResponse, samples: [f32; 1025] }

impl Default for PressureCurve {
    fn default() -> Self {
        layer_core::PressureResponse::linear().into()
    }
}

impl PressureCurve {
    pub fn map(&self, raw: f32) -> f32 {
        let x = if raw.is_finite() { raw.clamp(0., 1.) * 1024. } else { 0. };
        let index = (x as usize).min(1023);
        let [a, b] = [self.0.samples[index], self.0.samples[index + 1]];
        if b - a > 0.001 { self.0.source.map(raw) } else { a + (b - a) * (x - index as f32) }
    }
}

impl From<layer_core::PressureResponse> for PressureCurve {
    fn from(source: layer_core::PressureResponse) -> Self {
        let samples = std::array::from_fn(|i| source.map(i as f32 / 1024.));
        Self(Arc::new(PressureMapping { source, samples }))
    }
}
impl From<PressureCurve> for layer_core::PressureResponse {
    fn from(curve: PressureCurve) -> Self { curve.0.source.clone() }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ViewTransform {
    pub revision: u64,
    /// Affine transform `[a, b, c, d, tx, ty]`.
    pub surface_to_document: [f32; 6],
}

impl ViewTransform {
    pub const IDENTITY: Self = Self {
        revision: 0,
        surface_to_document: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    pub fn map(self, point: Point) -> Point {
        let [a, b, c, d, tx, ty] = self.surface_to_document;
        Point {
            x: a.mul_add(point.x, c.mul_add(point.y, tx)),
            y: b.mul_add(point.x, d.mul_add(point.y, ty)),
        }
    }
}

pub fn to_stroke_point(
    event: PenEvent,
    transform: ViewTransform,
    curve: PressureCurve,
    stroke_start_ns: u64,
) -> StrokePoint {
    // Stylus orientation lives in the document just like position. Preserve
    // tilt magnitude through zoom; rotate/reflect its direction with the view.
    let [a, b, c, d, _, _] = transform.surface_to_document;
    let map_direction = |vector: [f32; 2]| {
        let mapped = [
            a.mul_add(vector[0], c * vector[1]),
            b.mul_add(vector[0], d * vector[1]),
        ];
        let scale = vector[0].hypot(vector[1]) / mapped[0].hypot(mapped[1]).max(f32::MIN_POSITIVE);
        [mapped[0] * scale, mapped[1] * scale]
    };
    let twist_axis = map_direction([event.twist_radians.cos(), event.twist_radians.sin()]);
    StrokePoint {
        position: transform.map(event.surface_position),
        pressure: curve.map(event.pressure),
        tilt: map_direction(event.tilt_radians),
        twist: twist_axis[1]
            .atan2(twist_axis[0])
            .rem_euclid(std::f32::consts::TAU),
        elapsed_micros: event
            .timestamp_ns
            .saturating_sub(stroke_start_ns)
            .saturating_div(1_000)
            .min(u32::MAX as u64) as u32,
    }
}

/// The input-adapter half of a bounded, allocation-free SPSC queue.
pub struct InputProducer<T> {
    inner: Producer<T>,
}

/// The engine-owner half of a bounded, allocation-free SPSC queue.
pub struct InputConsumer<T> {
    inner: Consumer<T>,
}

pub fn input_queue<T>(capacity: usize) -> (InputProducer<T>, InputConsumer<T>) {
    let (producer, consumer) = RingBuffer::new(capacity.max(2));
    (
        InputProducer { inner: producer },
        InputConsumer { inner: consumer },
    )
}

impl<T> InputProducer<T> {
    /// Returns the event to the adapter if the queue is full. Never blocks.
    pub fn push(&mut self, event: T) -> Result<(), T> {
        self.inner
            .push(event)
            .map_err(|PushError::Full(event)| event)
    }
}

impl<T> InputConsumer<T> {
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop().ok()
    }
    pub fn peek(&self) -> Option<&T> {
        self.inner.peek().ok()
    }
}

#[derive(Debug, Default)]
pub struct StrokeBuilder {
    real: Vec<StrokePoint>,
    predicted: Vec<StrokePoint>,
    start_ns: Option<u64>,
    last_real_source: usize,
}

impl StrokeBuilder {
    pub fn with_capacity(samples: usize) -> Self {
        Self {
            real: Vec::with_capacity(samples),
            predicted: Vec::with_capacity(32),
            start_ns: None,
            last_real_source: 0,
        }
    }

    pub fn begin(&mut self, event: PenEvent, transform: ViewTransform, curve: PressureCurve) {
        self.real.clear();
        self.predicted.clear();
        self.start_ns = Some(event.timestamp_ns);
        self.push(event, transform, curve);
    }

    pub fn push(&mut self, event: PenEvent, transform: ViewTransform, curve: PressureCurve) {
        let Some(start_ns) = self.start_ns else {
            return;
        };
        let point = to_stroke_point(event, transform, curve, start_ns);
        if event.flags.contains(SampleFlags::PREDICTED) {
            if self.predicted.len() == 32 {
                self.predicted.remove(0);
            }
            self.predicted.push(point);
        } else {
            self.predicted.clear();
            self.last_real_source = self.real.len();
            self.real.push(point);
        }
    }

    pub fn real_points(&self) -> &[StrokePoint] {
        &self.real
    }

    pub fn predicted_points(&self) -> &[StrokePoint] {
        &self.predicted
    }

    pub(crate) fn replace_real(&mut self, index: usize, point: StrokePoint) {
        self.real[index] = point;
        self.predicted.clear();
    }

    pub(crate) fn last_real_source(&self) -> usize {
        self.last_real_source
    }

    pub fn elapsed_micros_at(&self, timestamp_ns: u64) -> Option<u32> {
        let start_ns = self.start_ns?;
        Some(
            timestamp_ns
                .saturating_sub(start_ns)
                .saturating_div(1_000)
                .min(u32::MAX as u64) as u32,
        )
    }

    /// Append a replayable stationary sample for a time-driven brush such as
    /// an airbrush. Returns `None` when no stroke is active or time did not
    /// advance beyond the most recent real sample.
    pub fn append_stationary(&mut self, timestamp_ns: u64) -> Option<StrokePoint> {
        let start_ns = self.start_ns?;
        let last = *self.real.last()?;
        let elapsed_micros = timestamp_ns
            .saturating_sub(start_ns)
            .checked_div(1_000)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        if elapsed_micros <= last.elapsed_micros {
            return None;
        }
        let point = StrokePoint {
            elapsed_micros,
            ..last
        };
        self.real.push(point);
        Some(point)
    }

    /// Predicted points are intentionally never returned as document data.
    pub fn finish(&mut self) -> Option<Arc<[StrokePoint]>> {
        self.start_ns.take()?;
        self.predicted.clear();
        let points = Arc::from(self.real.as_slice());
        self.real.clear();
        Some(points)
    }

    pub fn cancel(&mut self) {
        self.real.clear();
        self.predicted.clear();
        self.start_ns = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn event(sequence: u64, predicted: bool) -> PenEvent {
        PenEvent {
            flags: if predicted {
                SampleFlags::PREDICTED
            } else {
                SampleFlags::NONE
            },
            ..crate::test_support::event(sequence, PenPhase::Move, sequence as f32)
        }
    }

    #[test]
    fn queue_preserves_order_across_threads() {
        let (mut producer, mut consumer) = input_queue(64);
        let writer = thread::spawn(move || {
            for sequence in 0..20_000 {
                let mut pending = sequence;
                loop {
                    match producer.push(pending) {
                        Ok(()) => break,
                        Err(returned) => {
                            pending = returned;
                            thread::yield_now();
                        }
                    }
                }
            }
        });
        for expected in 0..20_000 {
            loop {
                if let Some(actual) = consumer.pop() {
                    assert_eq!(actual, expected);
                    break;
                }
                thread::yield_now();
            }
        }
        writer.join().unwrap();
    }

    #[test]
    fn stylus_pose_rotates_and_reflects_with_the_document_without_zooming_tilt() {
        let mut pen = event(1, false);
        pen.tilt_radians = [0.6, 0.8];
        pen.twist_radians = 0.;
        let rotated = to_stroke_point(
            pen,
            ViewTransform {
                revision: 1,
                surface_to_document: [0., 2., -2., 0., 10., 20.],
            },
            PressureCurve::default(),
            0,
        );
        assert!((rotated.tilt[0] + 0.8).abs() < 0.00001);
        assert!((rotated.tilt[1] - 0.6).abs() < 0.00001);
        assert!((rotated.twist - std::f32::consts::FRAC_PI_2).abs() < 0.00001);
        let reflected = to_stroke_point(
            pen,
            ViewTransform {
                revision: 2,
                surface_to_document: [-0.5, 0., 0., 0.5, 0., 0.],
            },
            PressureCurve::default(),
            0,
        );
        assert!((reflected.tilt[0] + 0.6).abs() < 0.00001);
        assert!((reflected.tilt[1] - 0.8).abs() < 0.00001);
        assert!((reflected.twist - std::f32::consts::PI).abs() < 0.00001);
    }

    #[test]
    fn full_queue_returns_the_unwritten_value() {
        let (mut producer, mut consumer) = input_queue(2);
        producer.push(1).unwrap();
        producer.push(2).unwrap();
        assert_eq!(producer.push(3), Err(3));
        assert_eq!(consumer.pop(), Some(1));
        producer.push(3).unwrap();
        assert_eq!(consumer.pop(), Some(2));
        assert_eq!(consumer.pop(), Some(3));
    }

    #[test]
    fn predicted_points_never_enter_committed_output() {
        let mut builder = StrokeBuilder::with_capacity(8);
        let mut first = event(1, false);
        first.phase = PenPhase::Down;
        builder.begin(first, ViewTransform::IDENTITY, PressureCurve::default());
        builder.push(
            event(2, true),
            ViewTransform::IDENTITY,
            PressureCurve::default(),
        );
        assert_eq!(builder.predicted_points().len(), 1);
        builder.push(
            event(3, false),
            ViewTransform::IDENTITY,
            PressureCurve::default(),
        );
        assert!(builder.predicted_points().is_empty());
        assert_eq!(builder.finish().unwrap().len(), 2);
    }

    #[test]
    fn compiled_pressure_bounds_error_for_steep_and_collapsed_controls() {
        for source in [layer_core::PressureResponse::default(),
            layer_core::PressureResponse::try_from(vec![[0.,0.],[0.,1.],[0.,1.],[0.,1.],[1.,1.]]).unwrap(),
            layer_core::PressureResponse::try_from(vec![[0.,0.],[1e-8,0.8],[0.001,0.9],[1.,1.]]).unwrap()] {
            let curve=PressureCurve::from(source.clone());
            for input in (0..=10000).map(|i|i as f32/10000.).chain([1e-9,1e-6,0.0009]) {
                assert!((curve.map(input)-source.map(input)).abs()<=0.00101,"{input}");
            }
            assert_eq!(curve.map(0.),source.points()[0][1]); assert_eq!(curve.map(1.),1.);
            assert!(curve.map(f32::NAN).is_finite());
            let saved=serde_json::to_string(&curve).unwrap();
            let restored:PressureCurve=serde_json::from_str(&saved).unwrap();assert_eq!(curve,restored);
        }
    }
    #[test]
    fn predictions_retain_only_the_latest_bounded_tail() {
        let mut builder = StrokeBuilder::with_capacity(8);
        builder.begin(
            event(1, false),
            ViewTransform::IDENTITY,
            PressureCurve::default(),
        );
        for sequence in 2..102 {
            builder.push(
                event(sequence, true),
                ViewTransform::IDENTITY,
                PressureCurve::default(),
            );
        }
        assert_eq!(builder.predicted_points().len(), 32);
        assert_eq!(builder.predicted_points()[0].position.x, 70.);
        assert_eq!(builder.real_points().len(), 1);
        assert_eq!(builder.finish().unwrap().len(), 1);
    }
}
