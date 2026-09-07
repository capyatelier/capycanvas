//! Portable drawing runtime shared by Layer platform clients.
//!
//! `layer-engine` owns platform-event ingress, deterministic brush evaluation,
//! document edits, and construction of one incremental render packet per
//! display callback. It contains no window-system or GPU implementation.

mod brush;
mod canvas;
mod feedback;
mod input;

pub use brush::DabGenerator;
pub use canvas::{CanvasEngine, EngineCapacity, EngineError, EngineMetrics};
pub use feedback::{FeedbackConfigError, InstantFeedbackConfig};
pub use input::{
    InputConsumer, InputProducer, PenEvent, PenPhase, PressureCurve, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
