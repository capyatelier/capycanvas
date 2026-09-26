//! Offline, causal replay of binary stroke recordings. Enable `prediction-bench`.
//! Production capture lives in `recording`; scoring is feature-gated.
mod metrics;
mod replay;
pub use crate::recording::{Event, Policy, Sample};
pub use metrics::Accuracy;
pub use replay::{ReplaySummary, replay, replay_with_frames};
