//! Offline, causal replay of the versioned pen dataset. Enable `prediction-bench`.
//! This module has no recording hooks and is absent from production builds.
use crate::{InstantFeedbackConfig, SampleFlags, ToolKind};
use layer_core::{Point, StrokePoint};
use serde::{Deserialize, Serialize};

mod metrics;
mod replay;
pub use metrics::Accuracy;
pub use replay::{ReplaySummary, replay};

/// First JSON line of each recording. Counts detect incomplete datasets.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetHeader {
    pub format: String,
    pub version: u32,
    pub contacts: usize,
    pub events: usize,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

/// One JSON line per contact, streamed independently of the recording's size.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub id: u64,
    pub policy: Policy,
    pub events: Vec<Event>,
    pub cancelled: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub config: InstantFeedbackConfig,
    /// Document coordinates to physical surface pixels, including translation.
    pub transform: [f32; 6],
}

/// `[elapsed_us, x, y, pressure, tilt_x, tilt_y, twist]`; angles are radians.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Sample(
    pub u32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
    pub f32,
);
impl From<Sample> for StrokePoint {
    fn from(p: Sample) -> Self {
        Self {
            elapsed_micros: p.0,
            position: Point { x: p.1, y: p.2 },
            pressure: p.3,
            tilt: [p.4, p.5],
            twist: p.6,
        }
    }
}

/// Replay order is delivery order, not timestamp order. Actuals are never
/// supplied before their event, even when used later as the scoring reference.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Sample(Sample),
    Predicted(Sample),
    Stationary(Sample),
    Replace(usize, Sample),
    /// `[contact_relative_ns, raw_pressure, tool, flags]`; raw pressure precedes
    /// the brush pressure curve. Only these fields affect pressure observation.
    Observe(u64, f32, ToolKind, SampleFlags),
    Reset,
    Policy(Policy),
    /// `[query_id, frame_elapsed_us, requested_elapsed_us]`.
    Query(u64, u32, u32),
}
