//! Validated wire records. Positions/revisions are captured by the OS adapter,
//! never reconstructed from the render owner's newer camera.
use layer_engine::{PenEvent, PenPhase, SampleFlags, ToolKind};
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CapyPointer {
    pub id: u64,
    pub timestamp_ns: u64,
    pub sequence: u64,
    pub view_revision: u64,
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    pub tilt_x: f32,
    pub tilt_y: f32,
    pub twist: f32,
    pub distance: f32,
    pub phase: u32,
    pub tool: u32,
    pub button: u32,
    pub flags: u32,
}
impl CapyPointer {
    /// Preserve capture metadata without routing timestamps through JSON/f64.
    pub fn event(&self) -> PenEvent {
        PenEvent {
            device_id: self.id,
            sequence: self.sequence,
            timestamp_ns: self.timestamp_ns,
            view_revision: self.view_revision,
            surface_position: layer_core::Point {
                x: self.x,
                y: self.y,
            },
            pressure: self.pressure,
            tilt_radians: [self.tilt_x, self.tilt_y],
            twist_radians: self.twist,
            distance: self.distance,
            phase: self.phase(),
            tool: self.tool(),
            flags: SampleFlags(self.flags as u16),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.phase > 4 || self.tool > 3 || self.button > 2 || self.flags & !0x0f != 0 {
            return Err("Invalid pointer enum or flags");
        }
        if ![
            self.x,
            self.y,
            self.pressure,
            self.tilt_x,
            self.tilt_y,
            self.twist,
            self.distance,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return Err("Nonfinite pointer record");
        }
        if !(0.0..=1.0).contains(&self.pressure) || !(0.0..=1.0).contains(&self.distance) {
            return Err("Pointer axes are not normalized");
        }
        if self.sequence == 0 {
            return Err("Pointer sequence must be nonzero");
        }
        Ok(())
    }
    pub fn phase(&self) -> PenPhase {
        match self.phase {
            0 => PenPhase::Hover,
            1 => PenPhase::Down,
            2 => PenPhase::Move,
            3 => PenPhase::Up,
            _ => PenPhase::Cancel,
        }
    }
    pub fn tool(&self) -> ToolKind {
        match self.tool {
            1 => ToolKind::Mouse,
            2 => ToolKind::Eraser,
            3 => ToolKind::Finger,
            _ => ToolKind::Pen,
        }
    }
}
/// Validate the whole ingress batch before any session actions can run.
pub fn validate_batch(records: &[CapyPointer]) -> Result<(), &'static str> {
    for sample in records {
        sample.validate()?;
    }
    for pair in records.windows(2) {
        if pair[1].sequence <= pair[0].sequence || pair[1].timestamp_ns < pair[0].timestamp_ns {
            return Err("Pointer history must be chronological");
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn point(sequence: u64) -> CapyPointer {
        CapyPointer {
            id: 17,
            timestamp_ns: sequence * 1_000_000,
            sequence,
            view_revision: 7,
            x: 45.0,
            y: 60.0,
            pressure: 0.4,
            tilt_x: 0.1,
            tilt_y: 0.2,
            twist: 0.0,
            distance: 0.0,
            phase: 2,
            tool: 0,
            button: 0,
            flags: 2,
        }
    }
    #[test]
    fn invalid_tail_rejects_entire_batch_before_consumption() {
        let mut batch = [point(1), point(2), point(3)];
        batch[2].pressure = f32::NAN;
        assert!(validate_batch(&batch).is_err());
        batch[2].pressure = 0.4;
        batch[2].phase = 900;
        assert!(validate_batch(&batch).is_err());
        batch[2].phase = 3;
        assert!(validate_batch(&batch).is_ok());
    }
    #[test]
    fn history_cannot_reverse_or_repeat_sequence() {
        assert!(validate_batch(&[point(2), point(1)]).is_err());
        assert!(validate_batch(&[point(1), point(1)]).is_err());
        let mut last = point(3);
        last.timestamp_ns = 0;
        assert!(validate_batch(&[point(1), last]).is_err());
    }
    #[test]
    fn typed_conversion_preserves_capture_precision_and_all_axes() {
        let mut record = point(31);
        record.timestamp_ns = (1u64 << 54) + 1;
        record.view_revision = (1u64 << 54) + 3;
        record.tilt_x = -0.4;
        record.tilt_y = 0.8;
        record.twist = 2.5;
        record.distance = 0.25;
        record.flags = 0x0f;
        record.tool = 2;
        validate_batch(&[record]).unwrap();
        let event = record.event();
        assert_eq!(event.timestamp_ns, record.timestamp_ns);
        assert_eq!(event.view_revision, record.view_revision);
        assert_eq!(event.sequence, record.sequence);
        assert_eq!(event.device_id, record.id);
        assert_eq!(
            [event.surface_position.x, event.surface_position.y],
            [record.x, record.y]
        );
        assert_eq!(event.pressure, record.pressure);
        assert_eq!(event.tilt_radians, [record.tilt_x, record.tilt_y]);
        assert_eq!(event.twist_radians, record.twist);
        assert_eq!(event.distance, record.distance);
        assert_eq!(event.flags.0, 0x0f);
        assert_eq!(event.tool, ToolKind::Eraser);
    }
    #[test]
    fn capture_revision_and_eraser_survive_validation() {
        let mut record = point(1);
        record.view_revision = 123;
        record.tool = 2;
        record.phase = 4;
        validate_batch(&[record]).unwrap();
        assert_eq!(record.view_revision, 123);
        assert_eq!(record.tool(), ToolKind::Eraser);
        assert_eq!(record.phase(), PenPhase::Cancel);
        assert_eq!(std::mem::size_of::<CapyPointer>(), 80);
    }
}
