use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutVersion {
    pub id: String,
    #[serde(default, deserialize_with = "layer_ui::lenient::or_default")]
    pub description: layer_ui::LayoutChange,
    #[serde(with = "crate::counter")]
    pub timestamp_ms: u64,
}

/// Newest first; revision ids break ties.
pub fn layout_history_versions(mut versions: Vec<LayoutVersion>) -> Vec<LayoutVersion> {
    versions.sort_by(|a, b| {
        b.timestamp_ms.cmp(&a.timestamp_ms).then_with(|| {
            let n = |id: &str| {
                id.strip_prefix('r')
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            n(&b.id).cmp(&n(&a.id))
        })
    });
    versions
}
