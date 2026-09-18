//! Retained-source conversion on the worker; shared candidate/edit on the owner.
use super::*;
use layer_core::color::{ColorProfile, source::SourceImage};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use layer_ui::SourceWorkflow;
use serde_json::Value;
use std::sync::Arc;

pub(super) struct Task {
    workflow: SourceWorkflow,
    candidate: Option<Project>,
    converted: Option<Arc<SourceImage>>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    background: [f32; 4],
    time: f32,
    clipped: u64,
    pub previews: Vec<color::Preview>,
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        let request = session
            .state()
            .requests
            .iter()
            .find_map(|r| match r.kind {
                HostRequestKind::Document {
                    request:
                        DocumentRequest::RepairSourceProfile { .. }
                        | DocumentRequest::RasterizeSource { .. },
                } => Some(r.id),
                _ => None,
            })
            .ok_or("No source edit is pending")?;
        let workflow = SourceWorkflow::begin(session, request)?;
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?;
        Ok(Self {
            workflow,
            candidate: None,
            converted: None,
            gpu: gpu.snapshot_gpu(),
            device: gpu.device().clone(),
            background: session.engine().view().background_rgba_linear,
            time: session.engine().animation_time(),
            clipped: 0,
            previews: Vec::new(),
        })
    }
    pub(super) fn work(
        &mut self,
        profile: Option<ColorProfile>,
        control: CaptureControl,
    ) -> Result<(), String> {
        if self.converted.is_some() {
            return Err("Source result was already prepared".into());
        }
        let (source, clipped) = self.workflow.prepare(
            profile,
            layer_color::photo::PhotoMemoryBudget::current().encode_bytes,
            || control.is_cancelled(),
        )?;
        self.clipped = clipped;
        self.converted = Some(source);
        Ok(())
    }
    pub(super) fn prepare(
        &mut self,
        host: &NativeHost,
        control: &CaptureControl,
    ) -> Result<(), String> {
        let source = self
            .converted
            .clone()
            .ok_or("Source conversion is not ready")?;
        let session = &host.session;
        let device_current =
            session.engine().backend().0.as_ref().map(|r| r.device()) == Some(&self.device);
        self.candidate =
            Some(
                self.workflow
                    .preview(session, source, control.is_cancelled(), device_current)?,
            );
        Ok(())
    }
    pub(super) fn compare(&mut self, control: CaptureControl) -> Result<(), String> {
        let candidate = self
            .candidate
            .as_ref()
            .ok_or("Source candidate is not ready")?;
        self.previews = color::compare(
            &self.gpu,
            [
                (&self.workflow.project, self.background),
                (candidate, self.background),
            ],
            self.time,
            control,
        )?;
        self.workflow.comparison_completed()?;
        Ok(())
    }
    pub(super) fn details(&self) -> Result<Value, String> {
        let original = &self.workflow.original;
        let builtin = match original.interpretation.profile {
            ColorProfile::Builtin(space) => Some(space),
            _ => None,
        };
        Ok(
            serde_json::json!({ "color": self.workflow.project.document.color, "profile_builtin": builtin,
            "channels": original.interpretation.channels, "depth": original.interpretation.depth,
            "source_profile": layer_color::profile_description(&original.interpretation.profile)?,
            "adds_layer": self.workflow.adds_layer(),
            "clipped_channels": self.clipped }),
        )
    }
    pub(super) fn adopt(
        &mut self,
        host: &mut NativeHost,
        control: &CaptureControl,
    ) -> Result<(), String> {
        let session = &mut host.session;
        let previous = session.state().revision;
        let device_current =
            session.engine().backend().0.as_ref().map(|r| r.device()) == Some(&self.device);
        self.workflow
            .commit(session, control.is_cancelled(), device_current)?;
        let mut change =
            session.complete_document_request(self.workflow.identity.request(), Ok(true))?;
        change.canvas_wake = true;
        host.apply_change(previous, change);
        self.converted = None;
        Ok(())
    }
}
