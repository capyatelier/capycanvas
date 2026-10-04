use crate::*;
use layer_ui::{DockLayout, WorkspaceCapture};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) struct TestClock(pub AtomicU64);
impl Clock for TestClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}
pub(crate) fn workspace(name: &str) -> Entity {
    let layout = DockLayout::default();
    Entity::workspace(
        name,
        WorkspaceCapture::from_template(&layout).unwrap(),
        layout,
        1_000_000,
    )
}
pub(crate) fn create(entity: Entity) -> Mutation {
    Mutation::Create {
        entity,
        claim: true,
        name_policy: NamePolicy::Exact,
    }
}
pub(crate) fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("capy-{tag}-{}", new_id()))
}
