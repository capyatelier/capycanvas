use crate::input::{PenEvent, PenPhase, SampleFlags, ToolKind};
use layer_core::{Point, StrokePoint};
use layer_render::ViewState;

pub(crate) const IDENTITY: [f32; 6] = [1., 0., 0., 1., 0., 0.];

pub(crate) fn point(x: f32, y: f32, pressure: f32, elapsed_micros: u32) -> StrokePoint {
    StrokePoint {
        position: Point { x, y },
        pressure,
        tilt: [0.; 2],
        twist: 0.,
        elapsed_micros,
    }
}

pub(crate) fn event(sequence: u64, phase: PenPhase, x: f32) -> PenEvent {
    PenEvent {
        device_id: 1,
        sequence,
        timestamp_ns: sequence * 1_000_000,
        view_revision: 1,
        surface_position: Point { x, y: 16. },
        pressure: 0.8,
        tilt_radians: [0.; 2],
        twist_radians: 0.,
        distance: 0.,
        phase,
        tool: ToolKind::Pen,
        flags: SampleFlags::PRIMARY,
    }
}

pub(crate) fn view(width_px: u32, height_px: u32) -> ViewState {
    ViewState {
        width_px,
        height_px,
        document_to_surface: IDENTITY,
        background_rgba_linear: [1.; 4],
    }
}
