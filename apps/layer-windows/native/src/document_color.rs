//! Worker preparation and atomic owner publication of shared color/history edits.
use super::*;
use layer_render::ViewState;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use layer_ui::{ColorPreparation, ColorWorkflow};

pub(super) struct Task {
    pub(super) workflow: ColorWorkflow,
    renderer: Option<WgpuRasterizer>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    brush: layer_core::BrushSnapshot,
    view: ViewState,
    time: f32,
    pub(super) previews: Vec<Preview>,
    pub(super) clipped: u64,
}
pub(super) struct Preview {
    pub extent: [u32; 2],
    pub pixels: Vec<u8>,
}
pub(super) fn compare(
    gpu: &SnapshotGpu,
    projects: [(&Project, [f32; 4]); 2],
    time: f32,
    control: CaptureControl,
) -> Result<Vec<Preview>, String> {
    projects
        .into_iter()
        .map(|(project, background)| {
            let mut snapshot = gpu
                .capture(
                    project.clone(),
                    background,
                    time,
                    Default::default(),
                    control.clone(),
                )
                .map_err(|e| e.to_string())?;
            let preview =
                snapshot.preview_document([512, 384], layer_core::color::RgbSpace::Srgb)?;
            Ok(Preview {
                extent: preview.extent,
                pixels: preview.encoded_bytes(layer_core::color::RgbSpace::Srgb)?,
            })
        })
        .collect()
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
                        DocumentRequest::ChangeColor { .. } | DocumentRequest::ColorHistory { .. },
                } => Some(r.id),
                _ => None,
            })
            .ok_or("No document color request is pending")?;
        let workflow = ColorWorkflow::begin(session, request)?;
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Canvas unavailable")?;
        Ok(Self {
            workflow,
            renderer: None,
            gpu: gpu.snapshot_gpu(),
            device: gpu.device().clone(),
            brush: session.engine().configured_brush().clone(),
            view: session.engine().view(),
            time: session.engine().animation_time(),
            previews: Vec::new(),
            clipped: 0,
        })
    }
    pub(super) fn work(
        &mut self,
        choice: Option<layer_color::DocumentColorChange>,
        copy: bool,
        control: CaptureControl,
    ) -> Result<(), String> {
        if self.renderer.is_some() || !self.previews.is_empty() {
            return Err("Color result was already prepared".into());
        }
        let plan = self.workflow.select(choice, copy)?;
        let budget = layer_color::photo::PhotoMemoryBudget::current().encode_bytes;
        let prepared = match plan {
            ColorPreparation::History => None,
            ColorPreparation::Flatten { color, options } => Some(
                self.gpu
                    .capture(
                        self.workflow.original.clone(),
                        self.view.background_rgba_linear,
                        self.time,
                        Default::default(),
                        control.clone(),
                    )
                    .map_err(|e| e.to_string())?
                    .flattened_document(color, options, budget)?,
            ),
            ColorPreparation::Edit(change) => Some(layer_color::prepare_document_color(
                &self.workflow.original,
                change,
                budget,
                || control.is_cancelled(),
            )?),
        };
        if let Some(prepared) = prepared {
            self.clipped = prepared.statistics.clipped_channels;
            self.workflow.candidate = Some(prepared.project);
        }
        let project = self
            .workflow
            .candidate
            .as_ref()
            .ok_or("Color candidate is missing")?;
        let mut view = self.view;
        let mut brush = self.brush.clone();
        layer_render::remap_document_colors(
            self.workflow.original.document.color.space,
            project.document.color.space,
            &mut brush,
            &mut view,
        );
        if !self.workflow.is_history() {
            self.previews = compare(
                &self.gpu,
                [
                    (&self.workflow.original, self.view.background_rgba_linear),
                    (project, view.background_rgba_linear),
                ],
                self.time,
                control.clone(),
            )?;
        }
        if !copy {
            let mut canvas = self
                .gpu
                .color_canvas(project.clone(), &brush, view, self.time, control)
                .map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(120);
            while !canvas.poll().map_err(|e| e.to_string())? {
                if Instant::now() >= deadline {
                    return Err("Color canvas preparation timed out".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut renderer = canvas.take_ready().map_err(|e| e.to_string())?;
            renderer
                .configure_ui_previews(layer_core::color::RgbSpace::Srgb)
                .map_err(|e| e.to_string())?;
            self.renderer = Some(renderer);
        }
        if !self.workflow.is_history() {
            self.workflow.comparison_completed()?;
        }
        Ok(())
    }
    pub(super) fn write_copy(&self, stream: impl Write, cancelled: bool) -> Result<(), String> {
        self.workflow.copy_project(cancelled)?.write(stream)
    }
    pub(super) fn adopt(
        &mut self,
        host: &mut NativeHost,
        control: &CaptureControl,
    ) -> Result<(), String> {
        let session = &mut host.session;
        let device_current =
            session.engine().backend().0.as_ref().map(|r| r.device()) == Some(&self.device);
        let prepared =
            self.workflow
                .prepare_commit(session, control.is_cancelled(), device_current)?;
        let next = self.renderer.as_mut().ok_or("Color canvas is not ready")?;
        let [w, h] = session.state().camera.viewport;
        next.resize_surface(w, h).map_err(|e| e.to_string())?;
        session.commit_document_color_candidate(prepared, |renderer| {
            std::mem::swap(&mut renderer.0, &mut self.renderer)
        })?;
        // The task keeps retired GPU resources for destruction on the file worker.
        session.complete_document_request(self.workflow.identity.request(), Ok(true))?;
        host.document_adopted();
        Ok(())
    }
}
