
/// Newest first; revision ids break ties.
pub fn layout_history_versions(history: &layer_ui::LayoutHistory) -> Vec<layer_ui::LayoutRevision> {
    let mut versions = history.revisions.values().cloned().collect::<Vec<_>>();
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
