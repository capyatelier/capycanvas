//! Portable drawing runtime shared by Layer platform clients.
//!
//! `layer-engine` owns platform-event ingress, deterministic brush evaluation,
//! document edits, and construction of one incremental render packet per
//! display callback. It contains no window-system or GPU implementation.

mod brush;
mod canvas;
mod feedback;
mod input;
mod selection_stroke;
mod vector_stroke;
#[cfg(test)]
mod test_support;
pub use selection_stroke::SelectionStroke;
pub use vector_stroke::{VectorStroke, Sample as VectorSample, polyline, fit_polyline, PolylineFit};
#[cfg(feature = "prediction-bench")]
pub mod prediction_bench;
pub mod recording;

pub use brush::DabGenerator;
pub use canvas::{CanvasEngine, EngineError, EngineMetrics, ScenePreview, StrokeRefusal};
pub use feedback::{FeedbackConfigError, InstantFeedbackConfig};
pub use input::{
    InputConsumer, InputProducer, PenEvent, PenInput, PenPhase, PressureCurve, TiltCurve, SampleFlags, ToolKind,
    ViewTransform, input_queue,
};
