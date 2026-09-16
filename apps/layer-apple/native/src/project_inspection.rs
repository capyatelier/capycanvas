//! Full-resolution inspection uses the same immutable snapshot as color/export.
use super::*;
use layer_render_wgpu::snapshot::SnapshotGpu;

pub(super) struct Task {
    project: Option<Project>,
    gpu: SnapshotGpu,
    background: [f32; 4],
    time: f32,
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        session.require_document_snapshot_idle()?;
        Ok(Self {
            project: Some(session.capture_project_recovery()?),
            gpu: session.engine().backend().0.as_ref().ok_or("Canvas unavailable")?.snapshot_gpu(),
            background: session.engine().view().background_rgba_linear,
            time: session.engine().animation_time(),
        })
    }
    pub(super) fn histogram(&mut self, task: &CapyProjectTask) -> Result<serde_json::Value, String> {
        let project = self.project.take().ok_or("Inspection was already consumed")?;
        let sampled_time = project.document.has_animated_effects().then_some(self.time);
        let mut snapshot = self.gpu.capture(project, self.background, self.time,
            Default::default(), task.control.clone()).map_err(|e| e.to_string())?;
        let histogram = snapshot.histogram().map_err(|e| e.to_string())?;
        task.check_cancelled()?;
        Ok(serde_json::json!({"epoch":task.epoch, "revision":task.revision,
            "histogram":histogram, "sampled_time":sampled_time}))
    }
}
