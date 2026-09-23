//! Offline, causal replay of the versioned pen dataset. Enable `prediction-bench`.
//! Production capture lives in `recording`; scoring is feature-gated.
use serde::{Deserialize, Serialize};

mod metrics;
mod replay;
pub use metrics::Accuracy;
pub use replay::{ReplaySummary, replay, replay_with_frames, replay_with_options};

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

pub use crate::recording::{Event, Policy, Sample};
