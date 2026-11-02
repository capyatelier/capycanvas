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
impl ToolKind {
    pub const fn is_pen(self) -> bool {
        matches!(self, Self::Pen | Self::Eraser | Self::Brush | Self::Pencil | Self::Airbrush)
    }
    fn pointer_class(self)->u8 {match self {Self::Finger=>0,Self::Mouse=>1,_=>2}}
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
    /// The device is still a pen, with pressure and pose axes.
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
    /// Signed barrel angles projected onto the surface XZ and YZ planes.
    pub tilt_radians: [f32; 2],
    pub twist_radians: f32,
    pub distance: f32,
    pub phase: PenPhase,
    pub tool: ToolKind,
    pub flags: SampleFlags,
}
impl PenEvent {
    pub fn is_finite(self)->bool {
        [self.surface_position.x,self.surface_position.y,self.pressure,self.tilt_radians[0],self.tilt_radians[1],
            self.twist_radians,self.distance].into_iter().all(f32::is_finite)
    }
}

pub struct PenInput {
    contacts: Box<[Option<PenEvent>]>,
}
impl Default for PenInput {
    fn default()->Self {Self {contacts:vec![None;16].into_boxed_slice()}}
}
impl PenInput {
    pub fn clear(&mut self) {self.contacts.fill(None);}
    pub fn cancel(&mut self, id:u64, tool:ToolKind) -> Option<PenEvent> {
        let slot=self.contacts.iter().position(|previous|previous.is_some_and(|p|p.device_id==id && p.tool.pointer_class()==tool.pointer_class()))?;
        self.contacts[slot].take().map(|previous|PenEvent {phase:PenPhase::Cancel,..previous})
    }
    pub fn filter(&mut self, mut event: PenEvent) -> Option<PenEvent> {
        let real=!event.flags.contains(SampleFlags::PREDICTED) && !event.flags.contains(SampleFlags::CORRECTION);
        let contact=self.contacts.iter().position(|previous| previous.is_some_and(|p|p.device_id==event.device_id
            && p.tool.pointer_class()==event.tool.pointer_class()));
        if !event.is_finite() {
            if !real || !matches!(event.phase,PenPhase::Up|PenPhase::Cancel) {return None;}
            let previous=self.contacts[contact?]?;
            event=PenEvent {phase:PenPhase::Cancel,sequence:event.sequence,
                timestamp_ns:event.timestamp_ns.max(previous.timestamp_ns),..previous};
        }
        if real {
            match event.phase {
                PenPhase::Down => {
                    let slot=contact.or_else(||self.contacts.iter().position(Option::is_none))?;
                    self.contacts[slot]=Some(event);
                },
                PenPhase::Move => if let Some(slot)=contact {self.contacts[slot]=Some(event);},
                PenPhase::Up|PenPhase::Cancel => if let Some(slot)=contact {self.contacts[slot]=None;},
                PenPhase::Hover=>{},
            }
        }
        Some(event)
    }
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

#[derive(Clone, Debug, PartialEq)]
pub struct TiltCurve {
    response: PressureCurve,
    extent: [f32; 2],
    axes: layer_core::TiltAxes,
    identity: bool,
}
impl Default for TiltCurve {
    fn default() -> Self { Self::new(&layer_core::TiltCalibration::default(), layer_core::TiltAxes::default()) }
}
impl TiltCurve {
    pub fn new(source: &layer_core::TiltCalibration, frame: layer_core::TiltAxes) -> Self {
        Self {response:source.response.normalized().clone().into(),extent:source.response.extent(),axes:Default::default(),identity:false}
            .with_axes(source.axes,frame)
    }
    pub fn response_matches(&self,response:&layer_core::TiltResponse)->bool {
        self.extent==response.extent() && &self.response.0.source==response.normalized()
    }
    pub fn with_axes(&self,axes:layer_core::TiltAxes,frame:layer_core::TiltAxes)->Self {
        let axes=axes.in_frame(frame);
        let identity=axes==layer_core::TiltAxes::default() && self.extent==[1.,1.] && self.response.0.source.points()==[[0.,0.],[1.,1.]];
        Self {response:self.response.clone(),extent:self.extent,axes,identity}
    }
    pub fn map(&self, radial: [f32; 2]) -> [f32; 2] {
        if self.identity { return radial; }
        let angle=radial[0].hypot(radial[1]);
        if angle == 0. { return [0.;2]; }
        let adjusted=self.response(angle/std::f32::consts::FRAC_PI_2)*std::f32::consts::FRAC_PI_2;
        self.axes.apply(radial.map(|v|v*adjusted/angle))
    }
    pub fn response(&self, input: f32) -> f32 { self.response.map(input/self.extent[0])*self.extent[1] }
    pub fn projected(&self, angles: [f32; 2]) -> [f32; 2] { self.map(radial_tilt(angles)) }
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
    point_from_tilt(event, transform, curve, stroke_start_ns, radial_tilt(event.tilt_radians))
}

pub(crate) fn calibrated_stroke_point(event: PenEvent, transform: ViewTransform, curve: PressureCurve,
    tilt: &TiltCurve, stroke_start_ns: u64) -> StrokePoint {
    point_from_tilt(event, transform, curve, stroke_start_ns, tilt.projected(event.tilt_radians))
}

fn point_from_tilt(event: PenEvent, transform: ViewTransform, curve: PressureCurve,
    stroke_start_ns: u64, tilt: [f32; 2]) -> StrokePoint {
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
        tilt: map_direction(tilt),
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

fn radial_tilt(angles: [f32; 2]) -> [f32; 2] {
    let [(sx, cx), (sy, cy)] = angles.map(|angle| {
        let (sin, cos) = angle.clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2).sin_cos();
        (sin, cos.max(0.))
    });
    let mut direction = [sx * cy, sy * cx];
    let mut length = direction[0].hypot(direction[1]);
    if length == 0. && cx == 0. && cy == 0. {
        direction = [sx, sy];
        length = direction[0].hypot(direction[1]);
    }
    let inclination = length.atan2(cx * cy);
    let scale = inclination / length.max(f32::MIN_POSITIVE);
    direction.map(|axis| axis * scale)
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
    last_event: Option<PenEvent>,
    tilt: TiltCurve,
}

impl StrokeBuilder {
    pub fn with_capacity(samples: usize) -> Self {
        Self {
            real: Vec::with_capacity(samples),
            predicted: Vec::with_capacity(32),
            start_ns: None,
            last_real_source: 0,
            last_event: None,
            tilt: TiltCurve::default(),
        }
    }

    pub fn begin(&mut self, event: PenEvent, transform: ViewTransform, curve: PressureCurve) {
        self.begin_calibrated(event, transform, curve, TiltCurve::default());
    }

    pub(crate) fn begin_calibrated(&mut self, event: PenEvent, transform: ViewTransform, curve: PressureCurve, tilt: TiltCurve) {
        self.real.clear();
        self.predicted.clear();
        self.start_ns = Some(event.timestamp_ns);
        self.tilt = tilt;
        self.push(event, transform, curve);
    }

    pub fn push(&mut self, event: PenEvent, transform: ViewTransform, curve: PressureCurve) {
        let Some(start_ns) = self.start_ns else {
            return;
        };
        let point = calibrated_stroke_point(event, transform, curve, &self.tilt, start_ns);
        if event.flags.contains(SampleFlags::PREDICTED) {
            if self.predicted.len() == 32 {
                self.predicted.remove(0);
            }
            self.predicted.push(point);
        } else {
            self.predicted.clear();
            self.last_real_source = self.real.len();
            self.last_event = Some(event);
            self.real.push(point);
        }
    }

    pub fn real_points(&self) -> &[StrokePoint] {
        &self.real
    }

    pub(crate) fn last_event(&self) -> Option<PenEvent> { self.last_event }

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
        self.last_event=None;
        self.predicted.clear();
        let points = Arc::from(self.real.as_slice());
        self.real.clear();
        Some(points)
    }

    pub fn cancel(&mut self) {
        self.real.clear();
        self.predicted.clear();
        self.start_ns = None;
        self.last_event = None;
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
    fn invalid_predictions_and_corrections_cannot_replace_real_contact_data() {
        let mut input=PenInput::default();
        let down=crate::test_support::event(1,PenPhase::Down,16.);
        assert_eq!(input.filter(down),Some(down));
        for flag in [SampleFlags::PREDICTED,SampleFlags::CORRECTION] {
            let mut bad=crate::test_support::event(22,PenPhase::Up,32.);
            bad.flags=flag;bad.pressure=f32::NAN;
            assert!(input.filter(bad).is_none());
            let valid=PenEvent {pressure:0.6,..bad};
            assert_eq!(input.filter(valid),Some(valid));
        }
        let mut release=crate::test_support::event(40,PenPhase::Up,64.);
        release.surface_position.x=f32::NAN;
        let cancel=input.filter(release).unwrap();
        assert_eq!(cancel,PenEvent {phase:PenPhase::Cancel,sequence:release.sequence,timestamp_ns:release.timestamp_ns,..down});
        assert!(input.filter(release).is_none());
    }

    #[test]
    fn numerical_admission_bounds_contacts_and_cancels_only_matching_identity() {
        let mut input=PenInput::default();
        for device_id in 0..16 {
            let down=PenEvent {device_id,..crate::test_support::event(1,PenPhase::Down,device_id as f32)};
            assert_eq!(input.filter(down),Some(down));
        }
        assert!(input.filter(PenEvent {device_id:16,..crate::test_support::event(1,PenPhase::Down,16.)}).is_none());
        assert_eq!(input.cancel(5,ToolKind::Pen).unwrap().device_id,5);
        assert!(input.cancel(5,ToolKind::Pen).is_none());
        assert!(input.filter(PenEvent {device_id:16,..crate::test_support::event(2,PenPhase::Down,16.)}).is_some());
        assert_eq!(input.cancel(6,ToolKind::Pen).unwrap().device_id,6);
        input.clear();
        assert!(input.cancel(0,ToolKind::Pen).is_none());
    }

    #[test]
    fn pen_and_touch_contacts_with_the_same_id_keep_separate_valid_samples() {
        let mut input=PenInput::default();
        let pen=crate::test_support::event(1,PenPhase::Down,16.);
        let touch=PenEvent {tool:ToolKind::Finger,surface_position:Point{x:32.,y:32.},..pen};
        input.filter(pen).unwrap();input.filter(touch).unwrap();
        let up=PenEvent {phase:PenPhase::Up,pressure:f32::NAN,sequence:2,..pen};
        let cancel=input.filter(up).unwrap();
        assert_eq!(cancel.tool,pen.tool);assert_eq!(cancel.surface_position,pen.surface_position);
        let remaining=input.cancel(touch.device_id,touch.tool).unwrap();
        assert_eq!(remaining.tool,touch.tool);assert_eq!(remaining.surface_position,touch.surface_position);
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
    fn tilt_direction_changes_share_the_compiled_response() {
        let mut source=layer_core::TiltCalibration::default();source.response.sensitivity(true);
        let original=TiltCurve::new(&source,Default::default());
        for turn in 0..4 {
            let axes=layer_core::TiltAxes{reverse_horizontal:true,..Default::default()};
            let mapped=original.with_axes(axes,layer_core::TiltAxes::quarter_turns(turn));
            assert!(Arc::ptr_eq(&original.response.0,&mapped.response.0));
            assert!(mapped.response_matches(&source.response));
            assert_eq!(mapped.map([0.3,0.]),axes.in_frame(layer_core::TiltAxes::quarter_turns(turn)).apply(original.map([0.3,0.])));
        }
        source.response.sensitivity(false);assert!(!original.response_matches(&source.response));
    }

    #[test]
    fn tilt_response_precedes_camera_rotation_reflection_and_zoom() {
        use layer_core::{TiltAxes,TiltCalibration,TiltResponse};
        let calibration=TiltCalibration{response:TiltResponse::try_from(vec![[0.,0.],[0.5,1.]]).unwrap(),axes:TiltAxes{swap:true,reverse_horizontal:true,reverse_vertical:false}};
        let curve=TiltCurve::new(&calibration,TiltAxes::default());
        let mut pen=event(1,false);pen.tilt_radians=[30f32.to_radians(),0.];
        let angle=60f32.to_radians();
        for (matrix,expected) in [([1.,0.,0.,1.,0.,0.],[0.,angle]),([0.,2.,-2.,0.,0.,0.],[-angle,0.]),([-3.,0.,0.,3.,0.,0.],[0.,angle])] {
            let point=calibrated_stroke_point(pen,ViewTransform{revision:0,surface_to_document:matrix},PressureCurve::default(),&curve,0);
            assert!((point.tilt[0]-expected[0]).abs()<1e-5 && (point.tilt[1]-expected[1]).abs()<1e-5,"{:?} vs {expected:?}",point.tilt);
        }
        assert_eq!(curve.projected([0.;2]),[0.;2]);
        assert!((curve.projected([60f32.to_radians(),0.])[1]-std::f32::consts::FRAC_PI_2).abs()<1e-6);
    }

    #[test]
    fn projected_tilt_angles_resolve_to_the_physical_pen_pose() {
        let mut pen = event(1, false);
        pen.tilt_radians = [0.75f32.atan2(0.5), (3f32.sqrt() / 4.).atan2(0.5)];
        let point = to_stroke_point(pen, ViewTransform::IDENTITY, PressureCurve::default(), 0);
        let inclination = point.tilt[0].hypot(point.tilt[1]);
        let azimuth = point.tilt[1].atan2(point.tilt[0]);
        assert!((inclination - std::f32::consts::PI / 3.).abs() < 1e-6,
            "60° physical inclination became {}°", inclination.to_degrees());
        assert!((azimuth - std::f32::consts::PI / 6.).abs() < 1e-6,
            "30° physical azimuth became {}°", azimuth.to_degrees());
    }

    #[test]
    fn projected_tilt_preserves_inclination_and_azimuth_through_every_quadrant() {
        for degrees in [0f32, 10., 40., 60., 89.9] {
            let inclination = degrees.to_radians();
            for azimuth in [-170f32, -135., -90., -30., 0., 30., 45., 90., 150.] {
                let (sin, cos) = azimuth.to_radians().sin_cos();
                let mut pen = event(1, false);
                pen.tilt_radians = [(inclination.sin() * cos).atan2(inclination.cos()),
                    (inclination.sin() * sin).atan2(inclination.cos())];
                let point = to_stroke_point(pen, ViewTransform::IDENTITY, PressureCurve::default(), 0);
                for (actual, expected) in point.tilt.into_iter().zip([inclination * cos, inclination * sin]) {
                    assert!((actual - expected).abs() < 5e-5,
                        "inclination={degrees} azimuth={azimuth}: {} != {}", actual, expected);
                }
            }
        }
        let right_angle = std::f32::consts::FRAC_PI_2;
        for (angles, expected) in [([0., 0.], [0., 0.]),
            ([right_angle, 0.], [right_angle, 0.]), ([0., -right_angle], [0., -right_angle]),
            ([right_angle, -right_angle], [right_angle / 2f32.sqrt(), -right_angle / 2f32.sqrt()])] {
            let mut pen = event(1, false);
            pen.tilt_radians = angles;
            let point = to_stroke_point(pen, ViewTransform::IDENTITY, PressureCurve::default(), 0);
            for (actual, expected) in point.tilt.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-6, "{angles:?}: {actual} != {expected}");
            }
        }
    }

    #[test]
    fn stylus_pose_rotates_and_reflects_with_the_document_without_zooming_tilt() {
        let mut pen = event(1, false);
        pen.tilt_radians = [(1f32.tan() * 0.6).atan(), (1f32.tan() * 0.8).atan()];
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
