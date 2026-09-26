//! Worker preparation and atomic owner publication of document color changes
//! and retained-source edits, shared by the native hosts.
use crate::{NativeHost, Renderer, open::PREPARE_DEADLINE};
use layer_core::{
    BrushSnapshot, Project,
    color::{ColorProfile, RgbSpace, source::SourceImage},
};
use layer_render::{CanvasRenderer, ViewState};
use layer_render_wgpu::{
    WgpuRasterizer,
    snapshot::{CaptureControl, SnapshotGpu},
};
use layer_ui::{
    ColorPreparation, ColorWorkflow, DocumentRequest, HostRequestKind, SourceWorkflow, UiSession,
};
use serde_json::{Value, json};
use std::{io::Write, sync::Arc, time::Instant};

/// Straight-alpha RGBA8 bytes encoded in the task's UI space.
pub struct Preview {
    pub extent: [u32; 2],
    pub pixels: Vec<u8>,
}

pub fn compare(
    gpu: &SnapshotGpu,
    projects: [(&Project, [f32; 4]); 2],
    time: f32,
    space: RgbSpace,
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
            let preview = snapshot.preview_document([512, 384], space)?;
            Ok(Preview {
                extent: preview.extent,
                pixels: preview.encoded_bytes(space)?,
            })
        })
        .collect()
}

fn pending(
    session: &UiSession<Renderer>,
    request: Option<u32>,
    wanted: impl Fn(&DocumentRequest) -> bool,
    missing: &str,
) -> Result<u32, String> {
    request
        .or_else(|| {
            session.state().requests.iter().find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if wanted(request) => Some(r.id),
                _ => None,
            })
        })
        .ok_or_else(|| missing.to_string())
}

fn gpu(session: &UiSession<Renderer>) -> Result<&WgpuRasterizer, String> {
    session
        .engine()
        .backend()
        .0
        .as_deref()
        .ok_or_else(|| "Canvas unavailable".to_string())
}

fn device_current(session: &UiSession<Renderer>, device: &wgpu::Device) -> bool {
    session.engine().backend().0.as_ref().map(|r| r.device()) == Some(device)
}

fn encode_budget() -> usize {
    layer_color::photo::PhotoMemoryBudget::current().encode_bytes
}

pub struct ColorTask {
    workflow: ColorWorkflow,
    renderer: Option<Box<WgpuRasterizer>>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    brush: BrushSnapshot,
    view: ViewState,
    time: f32,
    space: RgbSpace,
    previews: Vec<Preview>,
    clipped: u64,
}

impl ColorTask {
    /// `None` selects the pending document color or color-history request.
    pub fn capture(
        session: &UiSession<Renderer>,
        request: Option<u32>,
        space: RgbSpace,
    ) -> Result<Self, String> {
        let request = pending(
            session,
            request,
            |r| {
                matches!(
                    r,
                    DocumentRequest::ChangeColor { .. } | DocumentRequest::ColorHistory { .. }
                )
            },
            "No document color request is pending",
        )?;
        let workflow = ColorWorkflow::begin(session, request)?;
        let gpu = gpu(session)?;
        Ok(Self {
            workflow,
            renderer: None,
            gpu: gpu.snapshot_gpu(),
            device: gpu.device().clone(),
            brush: session.engine().configured_brush().clone(),
            view: session.engine().view(),
            time: session.engine().animation_time(),
            space,
            previews: Vec::new(),
            clipped: 0,
        })
    }

    pub fn work(
        &mut self,
        choice: Option<layer_color::DocumentColorChange>,
        copy: bool,
        control: CaptureControl,
    ) -> Result<(), String> {
        if self.renderer.is_some() || !self.previews.is_empty() {
            return Err("Color result was already prepared".into());
        }
        let prepared = match self.workflow.select(choice, copy)? {
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
                    .flattened_document(color, options, encode_budget())?,
            ),
            ColorPreparation::Edit(change) => Some(layer_color::prepare_document_color(
                &self.workflow.original,
                change,
                encode_budget(),
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
                self.space,
                control.clone(),
            )?;
        }
        if !copy {
            let mut canvas = self
                .gpu
                .color_canvas(project.clone(), &brush, view, self.time, control)
                .map_err(|e| e.to_string())?;
            let deadline = Instant::now() + PREPARE_DEADLINE;
            while !canvas.poll().map_err(|e| e.to_string())? {
                if Instant::now() >= deadline {
                    return Err("Color canvas preparation timed out".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let mut renderer = canvas.take_ready().map_err(|e| e.to_string())?;
            renderer
                .configure_ui_previews(self.space)
                .map_err(|e| e.to_string())?;
            self.renderer = Some(renderer.into());
        }
        if !self.workflow.is_history() {
            self.workflow.comparison_completed()?;
        }
        Ok(())
    }

    pub fn previews(&self) -> &[Preview] {
        &self.previews
    }

    pub fn details(&self) -> Value {
        json!({
            "color": self.workflow.original.document.color,
            "result": self.workflow.candidate.as_ref().map(|p| p.document.color),
            "clipped_channels": self.clipped,
            "copy": self.workflow.is_copy(),
        })
    }

    pub fn write_copy(&self, stream: impl Write, cancelled: bool) -> Result<(), String> {
        self.workflow.copy_project(cancelled)?.write(stream)
    }

    /// Publishes the prepared renderer, document and history in one owner turn.
    /// The task keeps the retired GPU resources for destruction on its worker.
    pub fn adopt(
        &mut self,
        host: &mut NativeHost,
        cancelled: bool,
        begin_commit: impl FnOnce() -> bool,
    ) -> Result<(), String> {
        let session = &mut host.session;
        let current = device_current(session, &self.device);
        let prepared = self.workflow.prepare_commit(session, cancelled, current)?;
        let next = self.renderer.as_mut().ok_or("Color canvas is not ready")?;
        let [w, h] = session.state().camera.viewport;
        next.resize_surface(w, h).map_err(|e| e.to_string())?;
        if !begin_commit() {
            return Err("Document operation cancelled".into());
        }
        session.commit_document_color_candidate(prepared, |renderer| {
            std::mem::swap(&mut renderer.0, &mut self.renderer)
        })?;
        session.complete_document_request(self.workflow.identity.request(), Ok(true))?;
        host.document_adopted();
        Ok(())
    }
}

pub struct SourceTask {
    workflow: SourceWorkflow,
    candidate: Option<Project>,
    converted: Option<Arc<SourceImage>>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    background: [f32; 4],
    time: f32,
    space: RgbSpace,
    clipped: u64,
    previews: Vec<Preview>,
}

impl SourceTask {
    /// `None` selects the pending source repair or rasterization request.
    pub fn capture(
        session: &UiSession<Renderer>,
        request: Option<u32>,
        space: RgbSpace,
    ) -> Result<Self, String> {
        let request = pending(
            session,
            request,
            |r| {
                matches!(
                    r,
                    DocumentRequest::RepairSourceProfile { .. }
                        | DocumentRequest::RasterizeSource { .. }
                )
            },
            "No source edit is pending",
        )?;
        let workflow = SourceWorkflow::begin(session, request)?;
        let gpu = gpu(session)?;
        Ok(Self {
            workflow,
            candidate: None,
            converted: None,
            gpu: gpu.snapshot_gpu(),
            device: gpu.device().clone(),
            background: session.engine().view().background_rgba_linear,
            time: session.engine().animation_time(),
            space,
            clipped: 0,
            previews: Vec::new(),
        })
    }

    pub fn work(
        &mut self,
        profile: Option<ColorProfile>,
        cancelled: impl FnMut() -> bool,
    ) -> Result<(), String> {
        if self.converted.is_some() {
            return Err("Source result was already prepared".into());
        }
        let (source, clipped) = self.workflow.prepare(profile, encode_budget(), cancelled)?;
        self.clipped = clipped;
        self.converted = Some(source);
        Ok(())
    }

    /// Owner only: validates the conversion against the live document.
    pub fn prepare(&mut self, host: &NativeHost, cancelled: bool) -> Result<(), String> {
        let source = self
            .converted
            .clone()
            .ok_or("Source conversion is not ready")?;
        let session = &host.session;
        let current = device_current(session, &self.device);
        self.candidate = Some(self.workflow.preview(session, source, cancelled, current)?);
        Ok(())
    }

    pub fn compare(&mut self, control: CaptureControl) -> Result<(), String> {
        let candidate = self
            .candidate
            .as_ref()
            .ok_or("Source candidate is not ready")?;
        self.previews = compare(
            &self.gpu,
            [
                (&self.workflow.project, self.background),
                (candidate, self.background),
            ],
            self.time,
            self.space,
            control,
        )?;
        self.workflow.comparison_completed()?;
        Ok(())
    }

    pub fn previews(&self) -> &[Preview] {
        &self.previews
    }

    pub fn details(&self) -> Result<Value, String> {
        let original = &self.workflow.original;
        let builtin = match original.interpretation.profile {
            ColorProfile::Builtin(space) => Some(space),
            _ => None,
        };
        Ok(json!({
            "color": self.workflow.project.document.color,
            "profile_builtin": builtin,
            "channels": original.interpretation.channels,
            "depth": original.interpretation.depth,
            "source_profile": layer_color::profile_description(&original.interpretation.profile)?,
            "adds_layer": self.workflow.adds_layer(),
            "clipped_channels": self.clipped,
        }))
    }

    pub fn adopt(
        &mut self,
        host: &mut NativeHost,
        cancelled: bool,
        begin_commit: impl FnOnce() -> bool,
    ) -> Result<(), String> {
        if !begin_commit() {
            return Err("Source edit cancelled".into());
        }
        let session = &mut host.session;
        let previous = session.state().revision;
        let current = device_current(session, &self.device);
        self.workflow.commit(session, cancelled, current)?;
        let mut change =
            session.complete_document_request(self.workflow.identity.request(), Ok(true))?;
        change.canvas_wake = true;
        host.apply_change(previous, change);
        self.converted = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{
        DocumentColor, SampleDepth,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
    };
    use layer_ui::{CommandId, UiAction};

    fn host() -> NativeHost {
        let document = layer_core::Document::new("Tasks", 64, 48);
        let gpu = WgpuRasterizer::new_native_headless(document.color).unwrap();
        let mut host = NativeHost::new(layer_ui::Platform::Mac).unwrap();
        host.session = UiSession::new(Renderer(Some(gpu.into())), document, [64, 48], layer_ui::Platform::Mac).unwrap();
        host.session.frame(0, 0).unwrap();
        host
    }

    fn invoke(host: &mut NativeHost, command: CommandId) {
        host.dispatch(UiAction::Invoke { command }).unwrap();
    }

    #[test]
    fn color_task_converts_previews_and_rejects_a_replaced_device() {
        let mut host = host();
        let convert: layer_color::DocumentColorChange = serde_json::from_value(json!({"Convert":
            {"space":"DisplayP3","options":{"intent":"RelativeColorimetric","black_point_compensation":false}}}))
        .unwrap();
        invoke(&mut host, CommandId::ConvertColorSpace);
        let mut task = ColorTask::capture(&host.session, None, RgbSpace::Srgb).unwrap();
        task.work(Some(convert.clone()), false, Default::default())
            .unwrap();
        assert_eq!(task.previews().len(), 2);
        assert!(
            task.previews()
                .iter()
                .all(|p| p.pixels.len() == (p.extent[0] * p.extent[1] * 4) as usize)
        );
        assert_eq!(task.details()["result"]["space"], "DisplayP3");
        let gpu = host.session.engine().backend().0.as_ref().unwrap();
        let adapter = gpu.adapter().clone();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: gpu.device().features(),
            required_limits: gpu.device().limits(),
            ..Default::default()
        }))
        .unwrap();
        let replacement = crate::GpuContext {
            adapter,
            device,
            queue,
        }
        .rasterizer(DocumentColor::default(), &Default::default(), true)
        .unwrap();
        host.session
            .replace_renderer(Renderer(Some(replacement.into())))
            .unwrap();
        assert!(task.adopt(&mut host, false, || true).is_err());
        assert_eq!(
            host.session.engine().document().color,
            DocumentColor::default()
        );
        let mut task = ColorTask::capture(&host.session, None, RgbSpace::Srgb).unwrap();
        task.work(Some(convert), false, Default::default()).unwrap();
        assert!(task.adopt(&mut host, false, || false).is_err());
        task.adopt(&mut host, false, || true).unwrap();
        assert_eq!(
            host.session.engine().document().color.space,
            RgbSpace::DisplayP3
        );
        assert!(host.session.state().requests.is_empty());
        drop((task, host));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }

    #[test]
    fn source_task_repairs_profile_and_commits_once() {
        let mut host = host();
        let interpretation = SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: true,
        };
        let mut source = SourceBuilder::new([24, 16], interpretation, 1 << 20).unwrap();
        for _ in 0..16 {
            source.push_row(&[200, 40, 90, 255].repeat(24)).unwrap();
        }
        host.session
            .import_layer_source("Photo", source.finish().unwrap())
            .unwrap();
        host.session.frame(0, 0).unwrap();
        let layer = host.session.engine().document().active_layer;
        invoke(&mut host, CommandId::RepairSourceProfile);
        let mut task = SourceTask::capture(&host.session, None, RgbSpace::Srgb).unwrap();
        task.work(Some(ColorProfile::Builtin(RgbSpace::DisplayP3)), || false)
            .unwrap();
        task.prepare(&host, false).unwrap();
        task.compare(Default::default()).unwrap();
        assert_eq!(task.previews().len(), 2);
        assert_ne!(task.previews()[0].pixels, task.previews()[1].pixels);
        assert_eq!(task.details().unwrap()["adds_layer"], false);
        task.adopt(&mut host, false, || true).unwrap();
        let repaired = host.session.engine().document().layer(layer).unwrap();
        assert_eq!(
            repaired.source.as_ref().unwrap().interpretation.profile,
            ColorProfile::Builtin(RgbSpace::DisplayP3)
        );
        assert!(host.session.state().requests.is_empty());
        assert!(task.adopt(&mut host, false, || true).is_err());
        drop((task, host));
        layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
