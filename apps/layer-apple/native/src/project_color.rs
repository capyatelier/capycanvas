//! Worker preparation and atomic owner publication of shared color/history edits.
use super::*;
use layer_core::{ColorTransition, PreparedColorTransition};
use layer_render::ViewState;
use layer_render_wgpu::snapshot::{SnapshotGpu, CaptureControl};
use layer_ui::DocumentColorOperation;

pub(super) struct Task {
    original: Project,
    candidate: Option<Project>,
    transition: Option<PreparedColorTransition>,
    operation: Option<DocumentColorOperation>,
    renderer: Option<WgpuRasterizer>,
    gpu: SnapshotGpu,
    device: wgpu::Device,
    brush: layer_core::BrushSnapshot,
    view: ViewState,
    time: f32,
    request: u32,
    previews: Vec<Preview>,
    clipped: u64,
    copy: bool,
}
pub(super) struct Preview { pub extent: [u32; 2], pub pixels: Vec<u8> }
pub(super) fn compare(gpu: &SnapshotGpu, projects: [(&Project, [f32; 4]); 2], time: f32, control: CaptureControl) -> Result<Vec<Preview>, String> {
    projects.into_iter().map(|(project, background)| {
        let mut snapshot = gpu.capture(project.clone(), background, time, Default::default(), control.clone()).map_err(|e| e.to_string())?;
        let preview = snapshot.preview_document([512, 384], crate::DISPLAY_SPACE)?;
        Ok(Preview { extent: preview.extent, pixels: preview.encoded_bytes(crate::DISPLAY_SPACE)? })
    }).collect()
}
impl Task {
    pub(super) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        session.require_document_idle()?;
        let (request, operation, history) = session.state().requests.iter().find_map(|r| match r.kind {
            HostRequestKind::Document { request: DocumentRequest::ChangeColor { operation } } => Some((r.id, Some(operation), None)),
            HostRequestKind::Document { request: DocumentRequest::ColorHistory { redo } } => Some((r.id, None, Some(redo))),
            _ => None,
        }).ok_or("No document color request is pending")?;
        let (transition, candidate) = if let Some(redo) = history {
            let (transition, project) = session.prepare_document_color_transition(if redo { ColorTransition::Redo } else { ColorTransition::Undo })?;
            (Some(transition), Some(project))
        } else { (None, None) };
        let gpu = session.engine().backend().0.as_ref().ok_or("Canvas unavailable")?;
        Ok(Self {
            original: session.capture_project_recovery()?, candidate, transition, operation,
            renderer: None, gpu: gpu.snapshot_gpu(), device: gpu.device().clone(),
            brush: session.engine().configured_brush().clone(), view: session.engine().view(),
            time: session.engine().animation_time(), request, previews: Vec::new(), clipped: 0, copy: false,
        })
    }
    fn work(&mut self, choice: Option<layer_color::DocumentColorChange>, copy: bool, control: CaptureControl) -> Result<(), String> {
        use layer_color::DocumentColorChange as Change;
        if self.renderer.is_some() || !self.previews.is_empty() { return Err("Color result was already prepared".into()); }
        if copy && self.operation != Some(DocumentColorOperation::Convert) { return Err("Only conversion can make a flattened copy".into()); }
        self.copy = copy;
        let budget = layer_color::photo::PhotoMemoryBudget::current().encode_bytes;
        if let Some(operation) = self.operation {
            let change = choice.ok_or("Choose a color change")?;
            if !matches!((operation, change), (DocumentColorOperation::Assign, Change::Assign(_))
                | (DocumentColorOperation::Convert, Change::Convert { .. }) | (DocumentColorOperation::Depth, Change::Depth { .. })) {
                return Err("Color choice does not match the request".into());
            }
            let prepared = if copy {
                let Change::Convert { space, options } = change else { unreachable!() };
                self.gpu.capture(self.original.clone(), self.view.background_rgba_linear, self.time,
                    Default::default(), control.clone()).map_err(|e| e.to_string())?
                    .flattened_document(layer_core::color::DocumentColor { space, depth: self.original.document.color.depth }, options, budget)?
            } else {
                layer_color::prepare_document_color(&self.original, change, budget, || control.is_cancelled())?
            };
            self.clipped = prepared.statistics.clipped_channels;
            self.candidate = Some(prepared.project);
        } else if choice.is_some() { return Err("History uses its saved color result".into()); }
        let project = self.candidate.as_ref().ok_or("Color candidate is missing")?;
        let matrix = self.original.document.color.space.linear_transform(project.document.color.space);
        let transform = |color: &mut [f32; 4]| {
            let rgb = layer_core::color::rgb::apply(matrix, [color[0], color[1], color[2]].map(f64::from));
            color[..3].copy_from_slice(&rgb.map(|v| v as f32));
        };
        let mut view = self.view; transform(&mut view.background_rgba_linear);
        let mut brush = self.brush.clone(); transform(&mut brush.color_rgba_linear);
        transform(&mut brush.color_dynamics.secondary_color_rgba_linear);
        if self.operation.is_some() {
            self.previews = compare(&self.gpu, [(&self.original, self.view.background_rgba_linear), (project, view.background_rgba_linear)], self.time, control.clone())?;
        }
        if !copy {
            let mut canvas = self.gpu.color_canvas(project.clone(), &brush, view, self.time, control).map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(120);
            while !canvas.poll().map_err(|e| e.to_string())? {
                if Instant::now() >= deadline { return Err("Color canvas preparation timed out".into()); }
                std::thread::sleep(Duration::from_millis(2));
            }
            let mut renderer = canvas.take_ready().map_err(|e| e.to_string())?;
            renderer.configure_ui_previews(crate::DISPLAY_SPACE).map_err(|e| e.to_string())?;
            self.renderer = Some(renderer);
        }
        Ok(())
    }
    pub(super) fn write_copy(&self, stream: impl Write) -> Result<(), String> {
        if !self.copy || self.previews.len() != 2 { return Err("Preview a flattened copy before saving".into()); }
        self.candidate.as_ref().ok_or("Converted copy is not ready")?.write(stream)
    }
    pub(super) fn adopt(&mut self, app: &mut CapyApple, task: &CapyProjectTask) -> Result<(), String> {
        if self.copy { return Err("Save the flattened copy separately".into()); }
        let session = &mut app.host.session;
        if session.state().document_file.epoch != task.epoch || session.engine().document().revision != task.revision
            || session.renderer_mut().0.as_ref().map(|r| r.device()) != Some(&self.device)
            || !session.state().requests.iter().any(|r| r.id == self.request) {
            return Err("The drawing or canvas changed; prepare the color change again".into());
        }
        session.require_document_idle()?;
        let prepared = if let Some(prepared) = self.transition.take() { prepared }
        else {
            let candidate = self.candidate.as_ref().ok_or("Color result is not ready")?;
            session.prepare_document_color_transition(ColorTransition::Apply {
                color: candidate.document.color, layers: candidate.document.layers.clone(),
            })?.0
        };
        let next = self.renderer.as_mut().ok_or("Color canvas is not ready")?;
        let [w, h] = session.state().camera.viewport;
        next.resize_surface(w, h).map_err(|e| e.to_string())?;
        if unsafe { capy_project_begin_commit(task) } < 0 { return Err("Document operation cancelled".into()); }
        let retired = session.renderer_mut().0.replace(self.renderer.take().unwrap());
        if let Err(error) = session.commit_document_color_transition(prepared) {
            self.renderer = std::mem::replace(&mut session.renderer_mut().0, retired);
            return Err(error);
        }
        self.renderer = retired; // Retired GPU resources leave on the file worker.
        session.complete_document_request(self.request, Ok(true))?;
        app.host.document_adopted();
        Ok(())
    }
}

/// # Safety
/// File worker only. Choice is optional DocumentColorChange/ColorProfile JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_edit_work(task: *const CapyProjectTask, choice: *const c_char, copy: bool) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    let choice = unsafe { read_title(choice) }.map(str::to_owned);
    task.perform(|payload| {
        let choice = choice?;
        match payload {
            Payload::Color(color) => color.work(serde_json::from_str(&choice).map_err(|e| e.to_string())?, copy, task.control.clone()),
            Payload::Source(source) if !copy => source.work(serde_json::from_str(&choice).map_err(|e| e.to_string())?, task.control.clone()),
            _ => Err("Not an editable color/source task".into()),
        }
    })
}
/// # Safety
/// Owner only, after edit_work. Shared validation constructs the source candidate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_project_candidate(app: *mut CapyApple, task: *const CapyProjectTask) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else { return -1; };
    app.perform(|app| {
        task.check_cancelled()?;
        let mut state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &state.error { return Err(error.clone()); }
        match &mut state.payload {
            Payload::Source(source) => source.prepare(app, task),
            Payload::Color(_) => Ok(()),
            _ => Err("Not an editable color/source task".into()),
        }
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Worker only, after project_candidate; renders complete source comparisons.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_compare(task: *const CapyProjectTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else { return -1; };
    task.perform(|payload| match payload {
        Payload::Source(source) => source.compare(task.control.clone()),
        Payload::Export(export) => export.compare(task.control.clone()),
        Payload::Color(_) => Ok(()),
        _ => Err("Not an editable color/source task".into()),
    })
}
/// # Safety
/// File worker only. Returns owned metadata JSON. Parsing profiles stays off owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_details(task: *const CapyProjectTask) -> *mut c_char {
    let Some(task) = (unsafe { task.as_ref() }) else { return std::ptr::null_mut(); };
    let mut json = String::new();
    let result = task.perform(|payload| {
        json = match payload {
            Payload::Info(info) => serde_json::to_string(&info.describe()?),
            Payload::Inspection(inspection) => serde_json::to_string(&inspection.histogram(task)?),
            Payload::Source(source) => serde_json::to_string(&source.details()?),
            Payload::Export(export) => serde_json::to_string(&export.details()?),
            Payload::Color(t) => serde_json::to_string(&serde_json::json!({
                "color":t.original.document.color, "result":t.candidate.as_ref().map(|p| p.document.color),
                "clipped_channels":t.clipped, "copy":t.copy,
            })),
            _ => return Err("No document details are available".into()),
        }.map_err(|e| e.to_string())?;
        Ok(())
    });
    if result < 0 { std::ptr::null_mut() } else { CString::new(json).unwrap().into_raw() }
}
#[repr(C)]
pub struct CapyProjectPreview {
    pub width: u32, pub height: u32, pub pixels: *const u8, pub count: usize,
}
/// # Safety
/// Worker only, after successful preparation. Borrowed pixels remain readable
/// until the next mutating job call/free; copy them before returning to the UI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_preview(task: *const CapyProjectTask, after: bool, output: *mut CapyProjectPreview) -> i32 {
    let (Some(task), Some(output)) = (unsafe { task.as_ref() }, unsafe { output.as_mut() }) else { return -1; };
    let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
    let previews = match &state.payload { Payload::Color(c) => &c.previews, Payload::Source(s) => &s.previews, Payload::Export(e) => &e.previews, _ => return -1 };
    let Some(preview) = previews.get(usize::from(after)) else { return -1; };
    *output = CapyProjectPreview { width: preview.extent[0], height: preview.extent[1], pixels: preview.pixels.as_ptr(), count: preview.pixels.len() };
    0
}
