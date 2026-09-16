//! Retained-source conversion on the worker; shared candidate/edit on the owner.
use super::*;
use layer_core::{LayerId, color::{ColorProfile, source::SourceImage}};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::sync::Arc;
use serde_json::Value;

pub(super) struct Task {
    project: Project,
    candidate: Option<Project>,
    original: Arc<SourceImage>,
    converted: Option<Arc<SourceImage>>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    background: [f32; 4],
    time: f32,
    request: u32,
    layer: LayerId,
    rasterize: bool,
    baked: bool,
    clipped: u64,
    pub previews: Vec<color::Preview>,
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        session.require_document_idle()?;
        let (request, layer, rasterize) = session.state().requests.iter().find_map(|r| match r.kind {
            HostRequestKind::Document { request: DocumentRequest::RepairSourceProfile { layer } } => Some((r.id, LayerId(layer), false)),
            HostRequestKind::Document { request: DocumentRequest::RasterizeSource { layer } } => Some((r.id, LayerId(layer), true)),
            _ => None,
        }).ok_or("No source edit is pending")?;
        let project = session.capture_project_recovery()?;
        let target = project.document.layer(layer).ok_or("Source layer no longer exists")?;
        let original = target.source.clone().ok_or("No retained source")?;
        let baked = !target.raster.is_empty() || !target.pending_operations.is_empty() || target.asset.is_some();
        let gpu = session.engine().backend().0.as_ref().ok_or("Canvas unavailable")?;
        Ok(Self { project, candidate: None, original, converted: None, gpu: gpu.snapshot_gpu(), device: gpu.device().clone(),
            background: session.engine().view().background_rgba_linear, time: session.engine().animation_time(),
            request, layer, rasterize, baked, clipped: 0, previews: Vec::new() })
    }
    pub(super) fn work(&mut self, profile: Option<ColorProfile>, control: CaptureControl) -> Result<(), String> {
        if self.converted.is_some() { return Err("Source result was already prepared".into()); }
        let source = if self.rasterize {
            if profile.is_some() { return Err("Rasterization uses the document profile".into()); }
            let (source, statistics) = layer_color::rasterize_source(&self.original, self.project.document.color,
                layer_color::photo::PhotoMemoryBudget::current().encode_bytes, || control.is_cancelled())?;
            self.clipped = statistics.clipped_channels;
            source
        } else {
            let mut source = (*self.original).clone();
            source.interpretation.profile = profile.ok_or("Choose a source profile")?;
            source.interpretation.profile_assumed = false;
            layer_color::WorkingDecoder::new(&source.interpretation, self.project.document.color.space, Default::default())?;
            source.validate()?;
            source
        };
        if control.is_cancelled() { return Err("Source edit cancelled".into()); }
        self.converted = Some(Arc::new(source));
        Ok(())
    }
    fn current(&self, app: &CapyApple, task: &CapyProjectTask) -> Result<(), String> {
        task.check_cancelled()?;
        let session = &app.host.session;
        if session.state().document_file.epoch != task.epoch || session.engine().document().revision != task.revision
            || session.engine().backend().0.as_ref().map(|r| r.device()) != Some(&self.device)
            || !session.state().requests.iter().any(|r| r.id == self.request) {
            return Err("The drawing or canvas changed; prepare the source edit again".into());
        }
        Ok(())
    }
    pub(super) fn prepare(&mut self, app: &CapyApple, task: &CapyProjectTask) -> Result<(), String> {
        self.current(app, task)?;
        let source = self.converted.clone().ok_or("Source conversion is not ready")?;
        let session = &app.host.session;
        self.candidate = Some(if self.rasterize { session.preview_rasterized_source(self.layer, &self.original, source)? }
            else { session.preview_layer_source(self.layer, &self.original, (*source).clone())? });
        Ok(())
    }
    pub(super) fn compare(&mut self, control: CaptureControl) -> Result<(), String> {
        let candidate = self.candidate.as_ref().ok_or("Source candidate is not ready")?;
        self.previews = color::compare(&self.gpu, [(&self.project, self.background), (candidate, self.background)], self.time, control)?;
        Ok(())
    }
    pub(super) fn details(&self) -> Result<Value, String> {
        let builtin = match self.original.interpretation.profile { ColorProfile::Builtin(space) => Some(space), _ => None };
        Ok(serde_json::json!({ "color": self.project.document.color, "profile_builtin": builtin,
            "channels": self.original.interpretation.channels, "depth": self.original.interpretation.depth,
            "source_profile": layer_color::profile_description(&self.original.interpretation.profile)?,
            "adds_layer": self.baked && !self.rasterize && self.converted.as_ref().is_none_or(|s| **s != *self.original),
            "clipped_channels": self.clipped }))
    }
    pub(super) fn adopt(&mut self, app: &mut CapyApple, task: &CapyProjectTask) -> Result<(), String> {
        self.current(app, task)?;
        if self.previews.len() != 2 { return Err("Preview the complete source result first".into()); }
        let source = self.converted.clone().ok_or("Source candidate is missing")?;
        if unsafe { capy_project_begin_commit(task) } < 0 { return Err("Source edit cancelled".into()); }
        let session = &mut app.host.session;
        let previous = session.state().revision;
        if self.rasterize { session.apply_rasterized_source(self.layer, &self.original, source)?; }
        else { session.repair_layer_source(self.layer, &self.original, (*source).clone())?; }
        let mut change = session.complete_document_request(self.request, Ok(true))?;
        change.canvas_wake = true;
        app.host.apply_change(previous, change);
        self.converted = None;
        Ok(())
    }
}

fn inspect_profile(bytes: Vec<u8>, summary: bool) -> Result<Value, String> {
    let profile = ColorProfile::Icc(bytes.into());
    let channels = layer_color::profile_channels(&profile)?;
    let name = layer_color::profile_description(&profile)?;
    if summary { Ok(serde_json::json!({"name":name,"channels":channels})) }
    else { serde_json::to_value(layer_ui::ExportProfile { profile, channels, name }).map_err(|e| e.to_string()) }
}
/// # Safety
/// Worker only. Borrows count bytes for this call; returns owned profile/error JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_color_profile_inspect(bytes: *const u8, count: usize, summary: bool) -> *mut c_char {
    let result = if bytes.is_null() || count == 0 || count > layer_color::MAX_ICC_BYTES {
        Err("Choose an ICC profile of at most 16 MiB".into())
    } else { inspect_profile(unsafe { std::slice::from_raw_parts(bytes, count) }.to_vec(), summary) };
    CString::new(result.unwrap_or_else(|error| serde_json::json!({"error":error})).to_string()).unwrap().into_raw()
}
