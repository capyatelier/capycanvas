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
pub enum PenPhase {
    Hover,
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SampleFlags(pub u16);

impl SampleFlags {
    pub const NONE: Self = Self(0);
    pub const PREDICTED: Self = Self(1 << 0);
    pub const PRIMARY: Self = Self(1 << 1);
    pub const BARREL_BUTTON: Self = Self(1 << 2);
    pub const INVERTED: Self = Self(1 << 3);
    /// A later correction may replace this real sample. `sequence` is its
    /// contact-local token until the final correction releases it.
    pub const ESTIMATED: Self = Self(1 << 4);
    /// Replace the registered sample with this token; never append a point,
    /// route a pointer action, or use the correction's delivery-time camera.
    pub const CORRECTION: Self = Self(1 << 5);

    pub const fn contains(self, flag: Self) -> bool {
        self.0 & flag.0 != 0
    }
}

/// Stable C-compatible event written directly by platform adapters.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressureCurve {
    pub dead_zone: f32,
    pub gamma: f32,
    pub ceiling: f32,
}

impl Default for PressureCurve {
    fn default() -> Self {
        Self {
            dead_zone: 0.0,
            gamma: 1.0,
            ceiling: 1.0,
        }
    }
}

impl PressureCurve {
    pub fn map(self, raw: f32) -> f32 {
        let span = (self.ceiling - self.dead_zone).max(f32::EPSILON);
        let normalized = ((raw - self.dead_zone) / span).clamp(0.0, 1.0);
        normalized.powf(self.gamma.max(0.01))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
    StrokePoint {
        position: transform.map(event.surface_position),
        pressure: curve.map(event.pressure),
        tilt: event.tilt_radians,
        twist: event.twist_radians,
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
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop().ok()
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
            device_id: 1,
            sequence,
            timestamp_ns: sequence * 1_000_000,
            view_revision: 0,
            surface_position: Point {
                x: sequence as f32,
                y: 2.0,
            },
            pressure: 0.5,
            tilt_radians: [0.0; 2],
            twist_radians: 0.0,
            distance: 0.0,
            phase: PenPhase::Move,
            tool: ToolKind::Pen,
            flags: if predicted {
                SampleFlags::PREDICTED
            } else {
                SampleFlags::NONE
            },
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
}
