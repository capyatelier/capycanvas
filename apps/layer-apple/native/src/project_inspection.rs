//! Full-resolution inspection uses the same immutable snapshot as color/export.
use super::*;
use layer_render_wgpu::snapshot::SnapshotGpu;

pub(super) struct Task {
    project: Option<ArtworkCapture>,
    gpu: SnapshotGpu,
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        session.require_document_snapshot_idle()?;
        Ok(Self {
            project: Some(session.capture_artwork()?),
            gpu: session.engine().backend().0.as_ref().ok_or("Canvas unavailable")?.snapshot_gpu(),
        })
    }
    pub(super) fn histogram(&mut self, task: &CapyProjectTask) -> Result<serde_json::Value, String> {
        let project = self.project.take().ok_or("Inspection was already consumed")?;
        let sampled_time = project.artwork.effects.iter().any(|(_,_,e)| project.artwork.definitions.get(e.definition).is_some_and(|d| layer_core::EffectView::new(&d.program, &e.values).animated())).then_some(project.output().context.elapsed);
        let mut snapshot = self.gpu.capture(project,
            task.control.clone()).map_err(|e| e.to_string())?;
        let histogram = snapshot.histogram().map_err(|e| e.to_string())?;
        task.check_cancelled()?;
        Ok(serde_json::json!({"epoch":task.epoch, "revision":task.revision,
            "axis":layer_ui::color_management::histogram_axis(&histogram),"histogram":histogram, "sampled_time":sampled_time}))
    }
}
