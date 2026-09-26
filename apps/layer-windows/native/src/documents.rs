//! Document jobs transfer immutable state; the live canvas remains on its owner.
//! One job and one completion are bounded. GPU/session destruction stays on the worker.
//!
use crate::document_io::{atomic_write, check_cancelled, io_error, location};
use layer_core::{Project, ProjectLimits};
use layer_host::{NativeHost, Renderer};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{CloseDecision, DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use serde::Deserialize;
#[path = "document_tabs.rs"]
mod tabs;
use std::{
    fs::File,
    io::BufReader,
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DocumentAction {
    Tabs { action: tabs::Action },
    Palette { action: crate::palette_files::Action },
    NewPreferences { id: u32, action: layer_ui::NewDocumentAction },
    Recovery { action: crate::recovery::Action },
    WorkflowBegin { id: u32 },
    OpenPaths { paths: Vec<String> },
    SaveStrokeRecording { path: String },
    DropImages { epoch: u64, revision: u64, active_layer: u64, paths: Vec<String>,
        screen: Option<layer_core::Point>, layer: Option<(u64, f32)> },
    Workflow { id: u32, action: crate::document_workflows::Action },
    Create { id: u32, epoch: u64, revision: u64, options: layer_ui::NewDocumentOptions,
        #[serde(default)] preset: String, #[serde(default)] defaults: bool },
    Interpret { id: u32, profile: Option<crate::color_storage::ProfileChoice> },
    Close,
    RespondClose {
        id: u32,
        epoch: u64,
        revision: u64,
        decision: CloseDecision,
    },
    Cancel {
        id: u32,
    },
    Failure {
        id: u32,
        error: String,
    },
    Open {
        id: u32,
        epoch: u64,
        revision: u64,
        path: String,
    },
    Save {
        id: u32,
        path: String,
    },
}
pub(crate) struct Environment {
    admission: layer_ui::DocumentAdmission,
    gpu: layer_host::GpuContext,
    viewport: [u32; 2],
    photo_policy: layer_ui::PhotoOpenPolicy,
}
impl Environment {
    pub(crate) fn capture(session: &UiSession<Renderer>) -> Result<Self, String> {
        let gpu = session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Wait for the canvas to finish starting")?;
        Ok(Self {
            admission: layer_ui::DocumentSessions::<()>::default().admission(&session.retained_document_tiles()),
            gpu: layer_host::GpuContext::of(gpu),
            viewport: session.state().camera.viewport,
            photo_policy: session.state().settings.photo_open,
        })
    }
}
struct Opening { environment: Environment, imported: layer_ui::ImportedDocument, profiles: Vec<layer_ui::profile_library::ProfileEntry> }
enum Source {
    Create(layer_ui::NewDocumentOptions),
    Recovery(PathBuf),
    Interpret(Box<layer_ui::ImportedDocument>, crate::color_storage::ProfileChoice),
    Open(PathBuf),
}
enum Job {
    Activate { gpu: layer_host::GpuContext, options: layer_host::RendererOptions, color: layer_core::color::DocumentColor },
    Spill { tiles: layer_core::raster_storage::RetainedTiles, directory: PathBuf },
    Workflow { task: Box<crate::document_workflows::Task>, action: crate::document_workflows::Action },
    DiscardOpening(Box<Opening>),
    Save {
        project: Project,
        path: PathBuf,
    },
    Prepare {
        environment: Environment,
        source: Source,
        cancelled: Arc<AtomicBool>,
    },
}
enum Completed {
    Activated(Box<WgpuRasterizer>),
    Spilled,
    Workflow(Box<crate::document_workflows::Task>),
    Cancelled,
    Interpretation(Box<Opening>),
    PhotoPrepared(Box<UiSession<Renderer>>),
    Saved,
    Prepared(Box<UiSession<Renderer>>),
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Job>,
    completed: Option<Result<Completed, String>>,
    retired: Option<Box<UiSession<Renderer>>>,
    retired_renderer: Option<Renderer>,
    retired_workflow: Option<Box<crate::document_workflows::Task>>,
}
#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    ready: Condvar,
    stopping: AtomicBool,
}
struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("capy-documents".into())
            // Debug WGSL translation needs more stack than Windows' default.
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                loop {
                    let (job, retired, retired_workflow, retired_renderer, stopping, completed) = {
                        let mut mailbox = state.mailbox.lock().unwrap();
                        while mailbox.pending.is_none()
                            && mailbox.retired.is_none()
                            && mailbox.retired_renderer.is_none()
                            && mailbox.retired_workflow.is_none()
                            && !state.stopping.load(Ordering::Acquire)
                        {
                            mailbox = state.ready.wait(mailbox).unwrap();
                        }
                        let stopping = state.stopping.load(Ordering::Acquire);
                        (
                            mailbox.pending.take(),
                            mailbox.retired.take(),
                            mailbox.retired_workflow.take(),
                            mailbox.retired_renderer.take(),
                            stopping,
                            if stopping {
                                mailbox.completed.take()
                            } else {
                                None
                            },
                        )
                    };
                    // Never destroy a candidate or retired GPU while holding the mailbox.
                    drop(retired);
                    drop(retired_workflow);
                    drop(retired_renderer);
                    if stopping {
                        drop(job);
                        drop(completed);
                        break;
                    }
                    let Some(job) = job else { continue };
                    let result = catch_unwind(AssertUnwindSafe(|| execute(job, &state.stopping)))
                        .unwrap_or_else(|_| Err("Document worker failed".into()));
                    state.mailbox.lock().unwrap().completed = Some(result);
                    wake();
                }
            })
            .map_err(|_| "Could not start the document worker")?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    fn submit(&self, job: Job) {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.pending.is_none() && mailbox.completed.is_none());
        mailbox.pending = Some(job);
        self.shared.ready.notify_one();
    }
    fn take(&self) -> Option<Result<Completed, String>> {
        self.shared.mailbox.lock().unwrap().completed.take()
    }
    fn retire(&self, session: Box<UiSession<Renderer>>) {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.retired.is_none());
        mailbox.retired = Some(session);
        self.shared.ready.notify_one();
    }
    fn retire_renderer(&self, renderer: Renderer) {
        if renderer.0.is_none() { return; }
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.retired_renderer.is_none());
        mailbox.retired_renderer = Some(renderer);
        self.shared.ready.notify_one();
    }
    fn retire_workflow(&self, task: Box<crate::document_workflows::Task>) {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.retired_workflow.is_none());
        mailbox.retired_workflow = Some(task);
        self.shared.ready.notify_one();
    }
    fn stop(&mut self) -> Result<(), String> {
        {
            // Serialize the predicate change with the worker entering wait.
            // An atomic alone permits notify to occur just before wait, losing
            // the only shutdown notification and leaving join blocked forever.
            let _mailbox = self.shared.mailbox.lock().unwrap();
            self.shared.stopping.store(true, Ordering::Release);
        }
        self.shared.ready.notify_one();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "Document worker shutdown failed")?;
        }
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn execute(job: Job, cancel: &AtomicBool) -> Result<Completed, String> {
    check_cancelled(cancel)?;
    match job {
        Job::Activate { gpu, options, color } => gpu.rasterizer(color, &options, true).map(|g| Completed::Activated(Box::new(g))),
        Job::Spill { tiles, directory } => layer_core::raster_storage::spill_to_directory(&tiles, &directory).map(|_| Completed::Spilled),
        Job::Workflow { mut task, action } => { task.work(action); Ok(Completed::Workflow(task)) }
        Job::DiscardOpening(opening) => { drop(opening); Ok(Completed::Cancelled) }
        Job::Save { project, path } => {
            let project = project.pruned()?;
            atomic_write(&path, cancel, |file| project.write(file))?;
            Ok(Completed::Saved)
        }
        Job::Prepare {
            environment,
            source,
            cancelled,
        } => prepare(environment, source, &cancelled),
    }
}
fn prepare(
    environment: Environment,
    source: Source,
    cancel: &AtomicBool,
) -> Result<Completed, String> {
    let limits = ProjectLimits {
        dimension: environment
            .gpu
            .device
            .limits()
            .max_texture_dimension_2d
            .min(ProjectLimits::default().dimension),
        ..Default::default()
    };
    let imported = match source {
        Source::Create(options) => layer_ui::ImportedDocument { project: options.project()?, source: layer_ui::ImportSource::Master },
        Source::Recovery(path) => layer_ui::read_import(layer_core::Cancellable { inner: File::open(path).map_err(|e| io_error("open recovery", e))?, cancelled: || cancel.load(Ordering::Acquire) },
            layer_ui::ImportIntent::Recovery, environment.photo_policy, "Recovered drawing", limits, Default::default(), cancel)?,
        Source::Interpret(mut imported, profile) => { imported.interpret(profile.resolve(cancel)?)?; *imported },
        Source::Open(path) => {
            let file = File::open(&path).map_err(|e| io_error("open", e))?;
            layer_ui::read_import(layer_core::Cancellable { inner: BufReader::new(file), cancelled: || cancel.load(Ordering::Acquire) }, layer_ui::ImportIntent::Open,
                environment.photo_policy, path.file_name().and_then(|v| v.to_str()).unwrap_or("Photo"),
                limits, Default::default(), cancel)?
        }
    };
    if imported.interpretation_required(environment.photo_policy).is_some() {
        return Ok(Completed::Interpretation(Box::new(Opening { environment, imported, profiles: crate::color_storage::list(cancel)? })));
    }
    let kind = imported.source;
    let project = imported.project;
    project.validate(limits)?;
    environment.admission.admit(&project)?;
    check_cancelled(cancel)?;
    // Eager preparation is isolated from the independently presented live canvas.
    let mut gpu = environment.gpu.rasterizer(project.document.color, &Default::default(), true)?;
    let mut programs = Vec::new();
    for effect in project
        .document
        .layers
        .iter()
        .filter_map(|layer| layer.effect.as_ref())
    {
        if !programs.contains(&effect.program) {
            programs.push(effect.program.clone());
        }
    }
    let mut validating = !programs.is_empty();
    if validating {
        gpu.request_effect_validation(EffectValidationRequest {
            request_id: 1,
            namespace: programs.clone(),
            programs,
        })
        .map_err(|e| e.to_string())?;
    }
    gpu.prepare_startup(&project.document, &Default::default(), false).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        check_cancelled(cancel)?;
        gpu.device().poll(wgpu::PollType::Poll).map_err(|e| e.to_string())?;
        if validating && let Some(result) = gpu.take_effect_validation() {
            result.result?;
            validating = false;
        }
        let ready = gpu.poll_startup().map_err(|e| e.to_string())?;
        if !validating && ready.canvas_ready && ready.brush_ready { break; }
        if Instant::now() >= deadline { return Err("Project canvas preparation timed out".into()); }
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut candidate =
        UiSession::from_project(Renderer(Some(gpu.into())), project, None, environment.viewport)?;
    candidate.frame(0, 0)?;
    check_cancelled(cancel)?;
    Ok(if kind == layer_ui::ImportSource::Photo { Completed::PhotoPrepared(Box::new(candidate)) } else { Completed::Prepared(Box::new(candidate)) })
}
pub(crate) fn prepare_recovery(environment: Environment, path: PathBuf, cancel: &AtomicBool) -> Result<Box<UiSession<Renderer>>, String> {
    match prepare(environment, Source::Recovery(path), cancel)? {
        Completed::Prepared(candidate) => Ok(candidate),
        _ => Err("Recovery is not a native drawing".into()),
    }
}
struct Active {
    id: u32,
    epoch: u64,
    revision: u64,
    location: Option<DocumentLocation>,
    cancelled: Option<Arc<AtomicBool>>,
}
pub(crate) struct DocumentService {
    tabs: layer_ui::DocumentSessions<tabs::Parked>,
    pub recovery: Option<crate::recovery::Service>,
    wake: Arc<dyn Fn() + Send + Sync>,
    tab_gpu: Option<layer_host::GpuContext>,
    activating: Option<(u64, u64)>,
    spilling: bool,
    deferred_action: Option<DocumentAction>,
    close_window: bool,
    close_next: bool,
    pub proof: crate::proof::Service,
    pub tone: crate::tone::Service,
    pub palettes: crate::palette_files::Service,
    worker: Worker,
    active: Option<Active>,
    opening: Option<Box<Opening>>,
    workflow: Option<Box<crate::document_workflows::Task>>,
    workflow_control: Option<(u32, layer_render_wgpu::snapshot::CaptureControl)>,
    workflow_running: bool,
    open_queue: std::collections::VecDeque<String>,
    recording_save: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
}
impl DocumentService {
    pub(crate) fn open(wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let wake = std::sync::Arc::new(wake);
        let notify = wake.clone();
        Ok(Self {
            proof: crate::proof::Service::new(wake.clone()),
            tone: crate::tone::Service::new(wake.clone()),
            palettes: crate::palette_files::Service::new(wake.clone()),
            wake,
            tabs: Default::default(),
            recovery: None,
            tab_gpu: None,
            activating: None,
            spilling: false,
            deferred_action: None,
            close_window: false,
            close_next: false,
            worker: Worker::start(move || notify())?,
            active: None,
            opening: None,
            workflow: None,
            workflow_control: None,
            workflow_running: false,
            open_queue: Default::default(),
            recording_save: None,
        })
    }
    pub(crate) fn status(&self) -> Option<serde_json::Value> {
        if self.opening.is_none() && let Some(active) = &self.active && active.cancelled.is_some() { return Some(serde_json::json!({"type":"opening_busy","id":active.id})); }
        if let Some(task) = &self.workflow { return Some(task.status()); }
        if self.workflow_running { return self.workflow_control.as_ref().map(|(id, _)| serde_json::json!({"type":"workflow_busy","id":id})); }
        self.opening.as_ref().map(|opening| serde_json::json!({
            "type": "interpret", "id": self.active.as_ref().map(|a| a.id),
            "spaces": layer_core::color::RgbSpace::ALL.map(|s| (s, s.name())),
            "profiles": opening.profiles,
            "channels": opening.imported.project.document.layers.iter().find_map(|l| l.source.as_ref()).map(|s| s.interpretation.channels),
        }))
    }
    pub(crate) fn renderer_unavailable(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.tab_gpu = None;
        self.tone.stop()?;
        self.proof.stop()?;
        if let Some((_, control)) = &self.workflow_control { control.cancel(); }
        if let Some(task) = self.workflow.take() {
            task.complete(host, false)?;
            self.workflow_control = None;
            self.worker.retire_workflow(task);
        }
        Ok(())
    }
    fn request(host: &NativeHost, id: u32) -> Result<DocumentRequest, String> {
        host.session
            .state()
            .requests
            .iter()
            .find_map(|r| match &r.kind {
                HostRequestKind::Document { request } if r.id == id => Some(request.clone()),
                _ => None,
            })
            .ok_or_else(|| "Unknown document request".into())
    }
    fn matches(host: &NativeHost, epoch: u64, revision: u64) -> Result<(), String> {
        host.session.require_document_snapshot_idle()?;
        if host.session.state().document_file.epoch != epoch
            || host.session.engine().document().revision != revision
        {
            Err("The document changed; review those changes before replacing or closing it".into())
        } else {
            Ok(())
        }
    }
    fn complete(
        host: &mut NativeHost,
        id: u32,
        result: Result<bool, String>,
    ) -> Result<(), String> {
        let previous = host.session.state().revision;
        let change = host.session.complete_document_request(id, result)?;
        host.apply_change(previous, change);
        Ok(())
    }
    const MAX_QUEUED_OPENS: usize = 64;
    fn open_idle(&self, host: &NativeHost) -> bool {
        let state = host.session.state();
        self.active.is_none() && self.workflow_control.is_none() && self.activating.is_none()
            && !self.spilling && self.opening.is_none() && self.recovery.as_ref().is_none_or(|r| !r.restoring())
            && !state.document_file.busy && !state.document_file.close_ready
            && !state.requests.iter().any(|r| matches!(r.kind, HostRequestKind::Document { .. }))
            && host.session.engine().backend().0.is_some()
            && host.session.command(layer_ui::CommandId::OpenDocument).enabled
    }
    fn open_queued(&mut self, host: &mut NativeHost) {
        if self.open_queue.is_empty() || !self.open_idle(host) {
            return;
        }
        if host.session.require_document_snapshot_idle().is_err() {
            host.dirty = true;
            return;
        }
        let path = self.open_queue.pop_front().unwrap();
        let result = (|| {
            host.dispatch(layer_ui::UiAction::Invoke { command: layer_ui::CommandId::OpenDocument })?;
            let id = host.session.state().requests.iter()
                .find(|r| matches!(r.kind, HostRequestKind::Document { request: DocumentRequest::Open }))
                .ok_or("Open the current drawing's pending dialog first")?.id;
            let epoch = host.session.state().document_file.epoch;
            let revision = host.session.engine().document().revision;
            self.dispatch(host, DocumentAction::Open { id, epoch, revision, path })
        })();
        if let Err(error) = result {
            host.error = Some(error);
            host.invalidate_snapshot();
        }
    }
    pub(crate) fn dispatch(
        &mut self,
        host: &mut NativeHost,
        action: DocumentAction,
    ) -> Result<(), String> {
        if let DocumentAction::SaveStrokeRecording { path } = action {
            if self.recording_save.is_some() {
                return Err("The stroke recording is already being saved".into());
            }
            location(&path)?;
            let data = host.session.stroke_recording().snapshot().map_err(|e| e.to_string())?;
            let (sender, receiver) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            std::thread::Builder::new()
                .name("capy-stroke-recording".into())
                .spawn(move || {
                    let result = layer_engine::recording::compress(&data)
                        .map_err(|e| io_error("compress stroke recording", e))
                        .and_then(|bytes| {
                            atomic_write(std::path::Path::new(&path), &AtomicBool::new(false), |file| {
                                std::io::Write::write_all(file, &bytes).map_err(|e| io_error("write stroke recording", e))
                            })
                        });
                    let _ = sender.send(result);
                    wake();
                })
                .map_err(|e| e.to_string())?;
            self.recording_save = Some(receiver);
            host.invalidate_snapshot();
            return Ok(());
        }
        if let DocumentAction::OpenPaths { paths } = action {
            if paths.is_empty() || self.open_queue.len() + paths.len() > Self::MAX_QUEUED_OPENS {
                return Err("Open up to 64 drawings at once".into());
            }
            for path in &paths {
                location(path)?;
            }
            self.open_queue.extend(paths);
            self.open_queued(host);
            return Ok(());
        }
        if let DocumentAction::Tabs { action } = action { return self.tab_action(host, action); }
        if let DocumentAction::Palette { action } = action { return self.palettes.dispatch(host, action); }
        if let DocumentAction::Recovery { action } = action {
            self.recovery.as_mut().ok_or("Recovery service unavailable")?.dispatch(&mut host.session, action)?;
            host.invalidate_snapshot();
            return Ok(());
        }
        if self.recovery.as_ref().is_some_and(|r| r.restoring()) { return Err("Wait for recovery to finish".into()); }
        if self.spilling {
            if self.deferred_action.is_some() { return Err("A file response is already queued".into()); }
            self.deferred_action = Some(action);
            return Ok(());
        }
        if self.activating.is_some() { return Err("Wait for the drawing to finish starting".into()); }
        if let DocumentAction::Cancel { id } = action
            && let Some(active) = self.active.as_ref().filter(|a| a.id == id)
            && let Some(cancelled) = &active.cancelled {
            cancelled.store(true, Ordering::Release);
            return Ok(());
        }
        if let DocumentAction::DropImages { epoch, revision, active_layer, paths, screen, layer } = action {
            let drawings = paths.iter().filter(|path| std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("capy"))).count();
            if drawings != 0 {
                if drawings != paths.len() { return Err("Open drawings or place images, not both".into()); }
                if layer.is_some() { return Err("Drop drawing files on the canvas to open them".into()); }
                return self.dispatch(host, DocumentAction::OpenPaths { paths });
            }
            Self::matches(host, epoch, revision)?;
            if self.active.is_some() || self.workflow_control.is_some()
                || host.session.engine().document().active_layer.0 != active_layer { return Err("The canvas changed while receiving images; try again".into()); }
            host.dispatch(layer_ui::UiAction::Invoke { command: layer_ui::CommandId::ImportImage })?;
            let id = host.session.state().requests.iter().find(|r| matches!(r.kind, HostRequestKind::Document { request: DocumentRequest::Place })).ok_or("Image placement request is missing")?.id;
            let prepared = (|| { let mut task = crate::document_workflows::Task::capture(host, id)?;task.place_at(host, screen, layer)?;Ok::<_, String>(task) })();
            let task = match prepared { Ok(task) => task, Err(error) => return Self::complete(host, id, Err(error)) };
            self.workflow_control = Some((id, task.control.clone()));self.workflow_running = true;
            self.worker.submit(Job::Workflow { task, action: crate::document_workflows::Action::ReadImages { paths } });
            return Ok(());
        }
        if let DocumentAction::WorkflowBegin { id } = action {
            if self.active.is_some() || self.workflow_control.is_some() { return Err("A document operation is already running".into()); }
            let task = crate::document_workflows::Task::capture(host, id)?;
            self.workflow_control = Some((id, task.control.clone()));
            self.workflow_running = true;
            self.worker.submit(Job::Workflow { task, action: crate::document_workflows::Action::Describe });
            return Ok(());
        }
        if let DocumentAction::Workflow { id, action } = action {
            if self.workflow_control.as_ref().map(|(id, _)| *id) != Some(id) { return Err("Document workflow expired".into()); }
            if matches!(action, crate::document_workflows::Action::Cancel) && self.workflow_running {
                self.workflow_control.as_ref().unwrap().1.cancel();
                return Ok(());
            }
            let mut task = self.workflow.take().ok_or("Wait for document preparation")?;
            let result = match action {
                crate::document_workflows::Action::Cancel => task.complete(host, false),
                crate::document_workflows::Action::Commit => task.commit(host),
                other => {
                    self.workflow_running = true;
                    self.worker.submit(Job::Workflow { task, action: other });
                    host.invalidate_snapshot();
                    return Ok(());
                }
            };
            if let Err(error) = result { self.workflow = Some(task); return Err(error); }
            task.retain_proof(&mut self.proof.view)?;
            self.workflow_control = None;
            self.worker.retire_workflow(task);
            host.invalidate_snapshot();
            return Ok(());
        }
        if let DocumentAction::Interpret { id, profile } = action {
            if self.active.as_ref().map(|a| a.id) != Some(id) { return Err("Image request is no longer current".into()); }
            let opening = self.opening.take().ok_or("No image interpretation is pending")?;
            if let Some(profile) = profile {
                let Opening { environment, imported, .. } = *opening;
                self.worker.submit(Job::Prepare { environment, source: Source::Interpret(Box::new(imported), profile), cancelled: self.active.as_ref().and_then(|a| a.cancelled.clone()).ok_or("Opening control missing")? });
            } else { self.worker.submit(Job::DiscardOpening(opening)); }
            host.invalidate_snapshot();
            return Ok(());
        }
        if let DocumentAction::NewPreferences { id, action } = action {
            if !matches!(Self::request(host,id)?,DocumentRequest::New) { return Err("Drawing preset dialog expired".into()); }
            return host.dispatch(layer_ui::UiAction::NewDocumentPreferences {action});
        }
        if let DocumentAction::Close = action {
            if self.recovery.as_ref().is_some_and(|s| s.restoring()) { return Ok(()); }
            self.close_window = true;
            let previous = host.session.state().revision;
            let change = host.session.request_document_close()?;
            host.apply_change(previous, change);
            return Ok(());
        }
        if let DocumentAction::RespondClose {
            id,
            epoch,
            revision,
            decision,
        } = action
        {
            if !matches!(
                Self::request(host, id)?,
                DocumentRequest::ConfirmClose { .. }
            ) {
                return Err("Not an unsaved changes request".into());
            }
            let checked = if decision == CloseDecision::Cancel {
                Ok(())
            } else {
                Self::matches(host, epoch, revision)
            };
            let previous = host.session.state().revision;
            let change = host.session.respond_document_close(
                id,
                if checked.is_ok() {
                    decision
                } else {
                    CloseDecision::Cancel
                },
            )?;
            host.apply_change(previous, change);
            host.error = checked.err();
            return Ok(());
        }
        // Dialog responses cannot cancel/replace work that is already writing.
        if self.active.is_some() {
            return Err("A document operation is already running".into());
        }
        let id = match &action {
            DocumentAction::Cancel { id }
            | DocumentAction::Failure { id, .. }
            | DocumentAction::Create { id, .. }
            | DocumentAction::Open { id, .. }
            | DocumentAction::Save { id, .. } => *id,
            _ => unreachable!(),
        };
        let request = Self::request(host, id)?;
        if matches!(request, DocumentRequest::ConfirmClose { .. }) {
            return Err("Respond to the unsaved changes dialog".into());
        }
        host.error = None;
        let started = (|| {
            let (job, location) = match action {
                DocumentAction::Cancel { .. } => {
                    Self::complete(host, id, Ok(false))?;
                    return Ok(None);
                }
                DocumentAction::Failure { error, .. } => {
                    return Err(if error.len() <= 1024 {
                        error
                    } else {
                        "File dialog failed".into()
                    });
                }
                DocumentAction::Save { path, .. }
                    if matches!(request, DocumentRequest::Save { .. }) =>
                {
                    let selected = location(&path)?;
                    let project = host.session.capture_project_save(id, selected.clone())?;
                    (
                        Job::Save {
                            project,
                            path: PathBuf::from(path),
                        },
                        Some(selected),
                    )
                }
                DocumentAction::Create { epoch, revision, options, preset, defaults, .. }
                    if matches!(request, DocumentRequest::New) => {
                    Self::matches(host, epoch, revision)?;
                    options.validate()?;
                    let mut environment = Environment::capture(&host.session)?;
                    environment.admission = self.tabs.admission(&host.session.retained_document_tiles());
                    if defaults || !preset.trim().is_empty() {
                        host.dispatch(layer_ui::UiAction::NewDocumentPreferences { action: layer_ui::NewDocumentAction::Remember {
                            options, name: preset, defaults,
                        } })?;
                    }
                    (Job::Prepare { environment, source: Source::Create(options), cancelled: Arc::new(AtomicBool::new(false)) }, None)
                }
                DocumentAction::Open {
                    epoch,
                    revision,
                    path,
                    ..
                } if matches!(request, DocumentRequest::Open) => {
                    Self::matches(host, epoch, revision)?;
                    let selected = location(&path)?;
                    let mut environment = Environment::capture(&host.session)?;
                    environment.admission = self.tabs.admission(&host.session.retained_document_tiles());
                    (
                        Job::Prepare {
                            environment,
                            source: Source::Open(PathBuf::from(path)),
                            cancelled: Arc::new(AtomicBool::new(false)),
                        },
                        Some(selected),
                    )
                }
                _ => return Err("The file dialog no longer matches this document operation".into()),
            };
            Ok(Some((job, location)))
        })();
        match started {
            Ok(Some((job, location))) => {
                self.active = Some(Active {
                    id,
                    epoch: host.session.state().document_file.epoch,
                    revision: host.session.engine().document().revision,
                    location,
                    cancelled: match &job { Job::Prepare { cancelled, .. } => Some(cancelled.clone()), _ => None },
                });
                self.worker.submit(job);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => Self::complete(host, id, Err(error)),
        }
    }
    pub(crate) fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.poll_tabs(host)?;
        self.palettes.poll(host);
        self.open_queued(host);
        if let Some(result) = self.recording_save.as_ref().and_then(|receiver| receiver.try_recv().ok()) {
            self.recording_save = None;
            match result {
                Ok(()) => host.session.stroke_recording().saved(),
                Err(error) => host.error = Some(format!("The stroke recording was not saved: {error}")),
            }
            host.invalidate_snapshot();
        }
        let Some(completed) = self.worker.take() else {
            return Ok(());
        };
        if self.activating.is_some() { return self.activated(host, completed); }
        if self.spilling {
            self.spilling = false;
            self.tabs.storage_completed(completed.and_then(|result| if matches!(result, Completed::Spilled) { Ok(()) } else { Err("Unexpected storage completion".into()) }));
            host.invalidate_snapshot();
            if let Some(action) = self.deferred_action.take() { self.dispatch(host, action)?; }
            return Ok(());
        }
        if self.workflow_running {
            self.workflow_running = false;
            let mut task = match completed {
                Ok(Completed::Workflow(task)) => task,
                Err(error) => {
                    let id = self.workflow_control.take().ok_or("Workflow identity is missing")?.0;
                    if matches!(host.session.state().requests.iter().find(|r| r.id == id).map(|r| &r.kind), Some(HostRequestKind::Histogram)) {
                        return host.dispatch(layer_ui::UiAction::CompleteRequest { id, error: Some(error) });
                    }
                    return Self::complete(host, id, Err(error));
                }
                _ => return Err("Unexpected workflow completion".into()),
            };
            if task.control.is_cancelled() || task.stage == "saved" {
                task.complete(host, task.stage == "saved")?;
                if let Some(notice) = task.notice() {
                    host.error = Some(notice.to_owned());
                }
                self.workflow_control = None;
                self.worker.retire_workflow(task);
            } else {
                match task.prepare_owner(host) {
                    Ok(true) => { self.workflow_running = true; let action = if task.stage == "proof_preserve" { crate::document_workflows::Action::ProofPreserve } else { crate::document_workflows::Action::Compare }; self.worker.submit(Job::Workflow { task, action }); return Ok(()); }
                    Err(error) => { task.fail(error); }
                    _ => {}
                }
                // Placement begins only after the native progress sheet has closed.
                // Its queued focus-loss event must precede the shared placement.
                if task.stage == "commit" && !task.awaits_placement_ui() {
                    match task.commit(host) {
                        Ok(()) => { task.retain_proof(&mut self.proof.view)?; self.workflow_control = None; self.worker.retire_workflow(task); host.invalidate_snapshot(); return Ok(()); }
                        Err(error) => task.fail(error),
                    }
                }
                self.workflow = Some(task);
            }
            host.invalidate_snapshot();
            return Ok(());
        }
        let completed = if self.active.as_ref().and_then(|a| a.cancelled.as_ref()).is_some_and(|c| c.load(Ordering::Acquire)) {
            match completed {
                Ok(Completed::Prepared(candidate) | Completed::PhotoPrepared(candidate)) => { self.worker.retire(candidate); Ok(Completed::Cancelled) }
                Ok(Completed::Interpretation(opening)) => { self.worker.submit(Job::DiscardOpening(opening)); return Ok(()); }
                _ => Ok(Completed::Cancelled),
            }
        } else { completed };
        let completed = match completed {
            Ok(Completed::Interpretation(opening)) => { self.opening = Some(opening); host.invalidate_snapshot(); return Ok(()); }
            Ok(Completed::PhotoPrepared(candidate)) => {
                if let Some(active) = &mut self.active { active.location = layer_ui::ImportSource::Photo.adoption_location(active.location.take()); }
                Ok(Completed::Prepared(candidate))
            }
            other => other,
        };
        let active = self.active.take().ok_or("Unexpected document completion")?;
        let result = match completed {
            Ok(Completed::Cancelled) => Ok(false),
            Ok(Completed::Interpretation(_) | Completed::PhotoPrepared(_) | Completed::Workflow(_) | Completed::Activated(_) | Completed::Spilled) => unreachable!(),
            Ok(Completed::Saved) => {
                if active.epoch == host.session.state().document_file.epoch {
                    Ok(true)
                } else {
                    Err("The completed file belongs to a document that is no longer open".into())
                }
            }
            Ok(Completed::Prepared(candidate)) => {
                let current_device = host
                    .session
                    .engine()
                    .backend()
                    .0
                    .as_ref()
                    .map(|gpu| gpu.device());
                let prepared_device = candidate
                    .engine()
                    .backend()
                    .0
                    .as_ref()
                    .map(|gpu| gpu.device());
                if current_device != prepared_device {
                    self.worker.retire(candidate);
                    return Self::complete(
                        host,
                        active.id,
                        Err("The GPU changed while opening the document. Try again.".into()),
                    );
                }
                return self.append_candidate(host, active, candidate);
            }
            Err(error) => Err(error),
        };
        Self::complete(host, active.id, result)
    }
    pub(crate) fn preview(&self, id: u32, index: usize) -> Result<crate::previews::CapyPreview, String> {
        let task = self.workflow.as_ref().filter(|t| t.id == id).ok_or("Document preview expired")?;
        task.preview(index)
    }
    pub(crate) fn stop_worker(&mut self) -> Result<(), String> {
        if let Some(cancelled) = self.active.as_ref().and_then(|a| a.cancelled.as_ref()) { cancelled.store(true, Ordering::Release); }
        if let Some(recovery) = &mut self.recovery { recovery.stop()?; }
        for (_, parked) in self.tabs.parked_mut() {
            if let Some(recovery) = &mut parked.owner.recovery { recovery.stop()?; }
        }
        let tone = self.tone.stop();
        let proof = self.proof.stop();
        if let Some((_, control)) = &self.workflow_control { control.cancel(); }
        if let Some(task) = self.workflow.take() { self.worker.retire_workflow(task); }
        let worker = self.worker.stop();
        proof.and(tone).and(worker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{CommandId, Platform, UiAction};
    use std::sync::{atomic::AtomicU64, mpsc};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        host: NativeHost,
        service: DocumentService,
        done: mpsc::Receiver<()>,
        directory: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let (wake, done) = mpsc::channel();
            let service = DocumentService::open(move || {
                let _ = wake.send(());
            })
            .unwrap();
            let mut host = NativeHost::new(Platform::Gtk).unwrap();
            host.session.set_document_replacement(true);
            let directory = std::env::temp_dir().join(format!(
                "capy-document-service-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&directory).unwrap();
            Self {
                host,
                service,
                done,
                directory,
            }
        }
        fn invoke(&mut self, command: CommandId) {
            self.host.dispatch(UiAction::Invoke { command }).unwrap();
        }
        fn request(&self) -> u32 {
            self.host
                .session
                .state()
                .requests
                .iter()
                .find(|r| matches!(r.kind, HostRequestKind::Document { .. }))
                .unwrap()
                .id
        }
        fn act(&mut self, action: DocumentAction) {
            self.service.dispatch(&mut self.host, action).unwrap();
        }
        fn finish(&mut self) {
            self.done.recv_timeout(Duration::from_secs(60)).unwrap();
            self.service.poll(&mut self.host).unwrap();
        }
        fn path(&self, name: &str) -> String {
            self.directory.join(name).to_str().unwrap().into()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.service.stop_worker().unwrap();
            // Only files created in this uniquely reserved fixture directory.
            for entry in std::fs::read_dir(&self.directory).unwrap() {
                let entry = entry.unwrap();
                if entry.file_type().unwrap().is_file() {
                    std::fs::remove_file(entry.path()).unwrap();
                }
            }
            std::fs::remove_dir(&self.directory).unwrap();
        }
    }

    #[test]
    fn renderer_failure_preserves_an_accepted_save() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        let document = f.host.session.engine().document().clone();
        f.host.suspend_renderer().unwrap();
        f.service.renderer_unavailable(&mut f.host).unwrap();
        f.invoke(CommandId::SaveDocumentAs);
        let path = f.path("Accepted save.capy");
        f.act(DocumentAction::Save {
            id: f.request(),
            path: path.clone(),
        });
        f.service.renderer_unavailable(&mut f.host).unwrap();
        assert!(
            f.service.active.is_some(),
            "retirement must retain the writing job"
        );
        f.finish();
        let saved = Project::read(File::open(path).unwrap(), Default::default()).unwrap();
        assert_eq!(saved.document.layers, document.layers);
        assert!(!f.host.session.state().document_file.modified);
    }

    #[test]
    fn suspended_renderer_saves_committed_raster_and_keeps_close_decisions() {
        use layer_core::color::PixelDescriptor;
        use layer_core::raster::{
            RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey,
        };
        let mut f = Fixture::new();
        let mut project = layer_ui::new_drawing(256, 256).unwrap();
        let bytes = [27, 89, 143, 255].repeat(256 * 256);
        let tile =
            RasterTile::backed(TileBlob::encode(PixelDescriptor::SRGB8_PAINT, &bytes).unwrap());
        project.document.layers[0].raster = RasterRevision::backed(RasterData {
            tiles: std::collections::BTreeMap::from([(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                tile,
            )]),
            watercolor: None,
        });
        // Completed host-backed pixels stay saveable with no GPU at any point.
        // Actual admitted pointer batches are covered by the native loss fixture.
        f.host.session =
            UiSession::from_project(Renderer(None), project, None, [256, 256]).unwrap();
        f.host.session.set_platform(Platform::Windows);
        f.host.session.set_document_replacement(true);
        f.host.session.mark_recovered();
        f.host.suspend_renderer().unwrap();
        f.service.renderer_unavailable(&mut f.host).unwrap();
        assert!(
            !f.host.session.engine().document().layers[0]
                .raster
                .is_empty(),
            "suspension must retain completed raster edits"
        );
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.command(CommandId::SaveDocumentAs).enabled);
        assert!(!f.host.session.command(CommandId::ExportDocument).enabled);
        assert!(!f.host.session.command(CommandId::Undo).enabled);
        f.act(DocumentAction::Close);
        let state = f.host.session.state().document_file.clone();
        f.act(DocumentAction::RespondClose {
            id: f.request(),
            epoch: state.epoch,
            revision: state.revision,
            decision: CloseDecision::Cancel,
        });
        assert!(!f.host.session.state().document_file.close_ready);
        let source = f.host.session.engine().document().clone();
        f.invoke(CommandId::SaveDocumentAs);
        let path = f.path("Recovered drawing.capy");
        f.act(DocumentAction::Save {
            id: f.request(),
            path: path.clone(),
        });
        assert!(
            f.host.session.state().document_file.modified,
            "only durable completion clears dirty"
        );
        f.finish();
        let project = Project::read(File::open(&path).unwrap(), Default::default()).unwrap();
        let mut expected = Vec::new();
        Project {
            document: source,
            assets: Default::default(),
        }
        .write(&mut expected)
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        let saved = project.document.layers[0].raster.wait_data().unwrap();
        assert_eq!(
            saved
                .tiles
                .values()
                .next()
                .unwrap()
                .wait_backing()
                .unwrap()
                .decode()
                .unwrap(),
            bytes
        );
        assert!(!f.host.session.state().document_file.modified);
        f.act(DocumentAction::Close);
        assert!(f.host.session.state().document_file.close_ready);
    }

    #[test]
    fn saving_an_older_checkpoint_retains_new_edits_and_close_rechecks_them() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        f.invoke(CommandId::SaveDocument);
        let id = f.request();
        let saved = f.host.session.engine().document().clone();
        let path = f.path("試し café.capy");
        f.act(DocumentAction::Save {
            id,
            path: path.clone(),
        });
        // Completion deliberately stays unacknowledged until after a newer edit.
        f.invoke(CommandId::AddLayer);
        f.act(DocumentAction::Close);
        assert!(f.host.session.state().document_file.busy);
        assert!(!f.host.session.state().document_file.close_ready);
        f.finish();
        let file = &f.host.session.state().document_file;
        assert!(file.modified);
        assert_eq!(file.location.as_ref().unwrap().uri, path);
        assert!(!file.close_ready);
        assert!(matches!(
            DocumentService::request(&f.host, f.request()).unwrap(),
            DocumentRequest::ConfirmClose { .. }
        ));
        let project = Project::read(File::open(path).unwrap(), Default::default()).unwrap();
        assert_eq!(
            project.document,
            Project {
                document: saved,
                assets: Default::default()
            }
            .pruned()
            .unwrap()
            .document
        );
        let state = &f.host.session.state().document_file;
        f.act(DocumentAction::RespondClose {
            id: f.request(),
            epoch: state.epoch,
            revision: state.revision,
            decision: CloseDecision::Cancel,
        });
        assert!(f.host.session.state().document_file.modified);
        assert!(!f.host.session.state().document_file.busy);
    }
    #[test]
    fn cancelled_and_failed_saves_keep_dirty_state_and_allow_retry() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        f.invoke(CommandId::SaveDocument);
        f.act(DocumentAction::Cancel { id: f.request() });
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.state().document_file.location.is_none());
        f.invoke(CommandId::SaveDocument);
        f.act(DocumentAction::Save {
            id: f.request(),
            path: f.path("missing/drawing.capy"),
        });
        f.finish();
        assert!(f.host.session.state().host_error.is_some());
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.state().document_file.location.is_none());
        assert!(!f.host.session.state().document_file.close_ready);
        f.invoke(CommandId::SaveDocument);
        let id = f.request();
        f.act(DocumentAction::Save {
            id,
            path: f.path("recovered.capy"),
        });
        // A duplicate picker response cannot overwrite the one active job.
        assert!(
            f.service
                .dispatch(&mut f.host, DocumentAction::Cancel { id })
                .is_err()
        );
        f.finish();
        assert!(f.host.session.state().host_error.is_none());
        assert!(!f.host.session.state().document_file.modified);
        assert!(!f.host.session.state().document_file.busy);
        // Unknown old responses cannot retire a later operation.
        f.invoke(CommandId::SaveDocumentAs);
        let next = f.request();
        assert!(
            f.service
                .dispatch(&mut f.host, DocumentAction::Cancel { id })
                .is_err()
        );
        assert_eq!(next, f.request());
        f.act(DocumentAction::Cancel { id: next });
    }
    #[test]
    fn stroke_recordings_save_off_thread_and_release_only_after_delivery() {
        let mut f = Fixture::new();
        let status = |f: &mut Fixture, action: Option<&str>| f.host.query(serde_json::json!({"type": "stroke_recording", "action": action})).unwrap();
        assert_eq!(status(&mut f, Some("start"))["recording"], true);
        assert_eq!(status(&mut f, Some("stop"))["ready"], true);
        let path = f.path("strokes.capystrokes");
        f.act(DocumentAction::SaveStrokeRecording { path: path.clone() });
        assert!(f.service.dispatch(&mut f.host, DocumentAction::SaveStrokeRecording { path: path.clone() }).is_err());
        f.finish();
        assert!(std::fs::read(&path).unwrap().starts_with(layer_engine::recording::MAGIC));
        assert_eq!(status(&mut f, None)["ready"], false);
        assert!(f.host.error.is_none());
    }
    #[test]
    fn dropped_drawings_queue_opens_while_mixed_and_layer_drops_are_refused() {
        let mut f = Fixture::new();
        let epoch = f.host.session.state().document_file.epoch;
        let revision = f.host.session.engine().document().revision;
        let active_layer = f.host.session.engine().document().active_layer.0;
        let drop = |paths: Vec<String>, layer: Option<(u64, f32)>| DocumentAction::DropImages {
            epoch, revision, active_layer, paths, screen: None, layer,
        };
        let [a, b, image, upper, c] = ["a.capy", "b.capy", "b.png", "a.CAPY", "c.capy"].map(|name| f.path(name));
        let mixed = f.service.dispatch(&mut f.host, drop(vec![a.clone(), image], None));
        assert!(mixed.unwrap_err().contains("not both"));
        let layered = f.service.dispatch(&mut f.host, drop(vec![upper], Some((active_layer, 0.5))));
        assert!(layered.unwrap_err().contains("canvas"));
        assert!(f.service.active.is_none() && f.service.open_queue.is_empty());
        f.act(drop(vec![a.clone(), b.clone()], None));
        f.service.poll(&mut f.host).unwrap();
        assert!(f.service.active.is_none() && f.host.session.state().requests.is_empty());
        assert_eq!(f.service.open_queue, [a, b]);
        let many = f.service.dispatch(&mut f.host, DocumentAction::OpenPaths { paths: vec![c; 63] });
        assert!(many.is_err());
        assert_eq!(f.service.open_queue.len(), 2);
    }
    #[test]
    fn stale_discard_and_open_responses_preserve_intervening_edits() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        f.act(DocumentAction::Close);
        let state = &f.host.session.state().document_file;
        let (id, epoch, revision) = (f.request(), state.epoch, state.revision);
        f.invoke(CommandId::AddLayer);
        f.act(DocumentAction::RespondClose {
            id,
            epoch,
            revision,
            decision: CloseDecision::Discard,
        });
        assert!(!f.host.session.state().document_file.close_ready);
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.error.as_ref().unwrap().contains("changed"));
        f.invoke(CommandId::OpenDocument);
        let state = &f.host.session.state().document_file;
        f.act(DocumentAction::RespondClose {
            id: f.request(),
            epoch: state.epoch,
            revision: state.revision,
            decision: CloseDecision::Discard,
        });
        let state = &f.host.session.state().document_file;
        let (id, epoch, revision) = (f.request(), state.epoch, state.revision);
        f.invoke(CommandId::AddLayer);
        let document = f.host.session.engine().document().clone();
        f.act(DocumentAction::Open {
            id,
            epoch,
            revision,
            path: f.path("not-read.capy"),
        });
        assert!(
            f.host
                .session
                .state()
                .host_error
                .as_ref()
                .unwrap()
                .contains("changed")
        );
        assert_eq!(f.host.session.engine().document(), &document);
        assert!(!f.host.session.state().document_file.busy);
    }
    #[test]
    fn ready_candidates_are_adopted_only_if_the_live_revision_still_matches() {
        for changed in [false, true] {
            let mut f = Fixture::new();
            f.invoke(CommandId::OpenDocument);
            let id = f.request();
            let state = &f.host.session.state().document_file;
            f.service.active = Some(Active {
                cancelled: None,
                id,
                epoch: state.epoch,
                revision: state.revision,
                location: Some(location(&f.path("opened.capy")).unwrap()),
            });
            let candidate = UiSession::from_project(
                Renderer(None),
                layer_ui::new_drawing(64, 48).unwrap(),
                None,
                f.host.session.state().camera.viewport,
            )
            .unwrap();
            f.service.worker.shared.mailbox.lock().unwrap().completed =
                Some(Ok(Completed::Prepared(Box::new(candidate))));
            if changed {
                f.invoke(CommandId::AddLayer);
            }
            // This CPU fixture has no renderer; suspend it before parking.
            f.host.session.suspend_renderer().unwrap();
            let old = f.host.session.engine().document().clone();
            f.service.poll(&mut f.host).unwrap();
            if changed {
                assert_eq!(f.host.session.engine().document(), &old);
                assert!(f.host.session.state().host_error.is_some());
            } else {
                let document = f.host.session.engine().document();
                assert_eq!([document.width, document.height], [64, 48]);
                assert_eq!(f.host.session.state().document_file.epoch, 1);
                assert!(!f.host.session.state().document_file.modified);
                assert!(f.host.dirty);
            }
            assert!(!f.host.session.state().document_file.busy);
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod gpu_tests {
    use super::*;
    use layer_ui::{CommandId, Platform, UiAction};
    use std::sync::mpsc;
    pub(super) fn invoke(host: &mut NativeHost, command: CommandId) {
        host.dispatch(UiAction::Invoke { command }).unwrap();
    }
    pub(super) fn request(host: &NativeHost) -> (u32, u64, u64) {
        let id = host
            .session
            .state()
            .requests
            .iter()
            .find(|r| matches!(r.kind, HostRequestKind::Document { .. }))
            .unwrap()
            .id;
        let state = &host.session.state().document_file;
        (id, state.epoch, state.revision)
    }
    fn finish(service: &mut DocumentService, host: &mut NativeHost, done: &mpsc::Receiver<()>) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match done.recv_timeout(Duration::from_millis(2)) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Exercise the old document's renderer while another renderer is prepared.
                    host.session.frame(0, 0).unwrap();
                    assert!(Instant::now() < deadline, "Document worker timed out");
                }
                Err(e) => panic!("Document worker disconnected: {e}"),
            }
        }
        service.poll(host).unwrap();
    }
    pub(super) fn image(host: &mut NativeHost) -> layer_render::ReadbackImage {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            host.session.frame(0, 0).unwrap();
            if !host.session.engine().has_pending_document_edits() {
                break;
            }
            assert!(Instant::now() < deadline, "Raster frame did not complete");
            std::thread::sleep(Duration::from_millis(1));
        }
        // Explicit functional-test readback; project saving never reads the GPU.
        let renderer = host.session.renderer_mut().0.as_mut().unwrap();
        let [width, height] = renderer.document_extent();
        let bytes = renderer.readback_srgb_rgba8().unwrap();
        layer_render::ReadbackImage {
            request_id: 1,
            width,
            height,
            stride: width * 4,
            bytes,
        }
    }

    #[test]
    #[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
    fn d3d12_save_checkpoint_survives_later_edits() {
        let gpu = WgpuRasterizer::new_headless().unwrap();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let project = layer_ui::new_drawing(63, 47).unwrap();
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session =
            UiSession::from_project(Renderer(Some(gpu.into())), project, None, [31, 29]).unwrap();
        host.session.set_platform(Platform::Windows);
        host.session.set_document_replacement(true);
        host.resize(31, 29, 1.).unwrap();
        host.import_layer_image(
            "Synthetic alpha",
            layer_render::HostImage {
                width: 4,
                height: 3,
                stride: 16,
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: &[210, 45, 83, 180].repeat(12),
            },
        )
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "capy-save-gpu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let source = directory.join("source.capy");
        let (wake, done) = mpsc::channel();
        let mut service = DocumentService::open(move || {
            let _ = wake.send(());
        })
        .unwrap();
        invoke(&mut host, CommandId::SaveDocument);
        let (id, _, _) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Save {
                    id,
                    path: source.to_str().unwrap().into(),
                },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
        assert!(!host.session.state().document_file.modified);
        let saved = std::fs::read(&source).unwrap();
        host.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: 0.75,
        })
        .unwrap();
        image(&mut host);
        assert!(host.session.state().document_file.modified);
        assert_eq!(
            host.session
                .state()
                .document_file
                .location
                .as_ref()
                .unwrap()
                .uri,
            source.to_str().unwrap()
        );
        assert_eq!(std::fs::read(&source).unwrap(), saved);
        service.stop_worker().unwrap();
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    #[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
    fn d3d12_background_save_open_new_and_stale_adoption() {
        let gpu = WgpuRasterizer::new_headless().unwrap();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let mut host = NativeHost::new(Platform::Gtk).unwrap();
        host.session = UiSession::from_project(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48).unwrap(),
            None,
            [64, 48],
        )
        .unwrap();
        host.session.set_platform(Platform::Gtk);
        host.session.set_document_replacement(true);
        host.resize(64, 48, 1.).unwrap();
        let directory =
            std::env::temp_dir().join(format!("capy-document-gpu-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory
            .join("round-trip.capy")
            .to_str()
            .unwrap()
            .to_owned();
        let (wake, done) = mpsc::channel();
        let mut service = DocumentService::open(move || {
            let _ = wake.send(());
        })
        .unwrap();
        let clean = image(&mut host);
        let paper = host
            .session
            .engine()
            .document()
            .layers
            .iter()
            .find(|l| l.kind == layer_core::LayerKind::Background)
            .unwrap()
            .id
            .0;
        host.dispatch(UiAction::SetLayerVisibility {
            id: paper,
            visible: false,
        })
        .unwrap();
        let expected = image(&mut host);
        assert_ne!(expected.bytes, clean.bytes);
        invoke(&mut host, CommandId::Undo);
        assert_eq!(image(&mut host).bytes, clean.bytes);
        assert!(!host.session.state().document_file.modified);
        invoke(&mut host, CommandId::Redo);
        assert_eq!(image(&mut host).bytes, expected.bytes);

        invoke(&mut host, CommandId::SaveDocument);
        let (id, _, _) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Save {
                    id,
                    path: path.clone(),
                },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
        assert!(!host.session.state().document_file.modified);
        assert_eq!(image(&mut host).bytes, expected.bytes);
        invoke(&mut host, CommandId::NewDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Create { id, epoch, revision, options: layer_ui::NewDocumentOptions { extent: [96, 72], ..Default::default() }, preset: String::new(), defaults: false },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_none());
        assert_eq!(host.session.engine().document().width, 96);
        assert_eq!(host.session.state().document_file.epoch, epoch + 1);
        assert!(host.session.state().document_file.location.is_none());
        invoke(&mut host, CommandId::OpenDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Open {
                    id,
                    epoch,
                    revision,
                    path: path.clone(),
                },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_none());
        assert_eq!(host.session.engine().document().width, 64);
        assert_eq!(host.session.state().document_file.epoch, epoch + 1);
        let reopened = image(&mut host);
        assert_eq!(
            [reopened.width, reopened.height, reopened.stride],
            [expected.width, expected.height, expected.stride]
        );
        assert_eq!(reopened.bytes, expected.bytes);
        service
            .dispatch(&mut host, DocumentAction::OpenPaths { paths: vec![path.clone(), path.clone()] })
            .unwrap();
        finish(&mut service, &mut host, &done);
        service.poll(&mut host).unwrap();
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_none());
        assert!(service.open_queue.is_empty());
        assert_eq!(host.session.state().document_file.epoch, epoch + 3);
        let original = host.session.engine().document().clone();
        let corrupt = directory.join("invalid.capy");
        std::fs::write(&corrupt, b"not a project").unwrap();
        invoke(&mut host, CommandId::OpenDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Open {
                    id,
                    epoch,
                    revision,
                    path: corrupt.to_str().unwrap().into(),
                },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_some());
        assert_eq!(host.session.engine().document(), &original);
        assert_eq!(image(&mut host).bytes, expected.bytes);
        invoke(&mut host, CommandId::NewDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Create { id, epoch, revision, options: layer_ui::NewDocumentOptions { extent: [0, 48], ..Default::default() }, preset: String::new(), defaults: false },
            )
            .unwrap();
        assert!(host.session.state().host_error.is_some());
        assert_eq!(host.session.engine().document(), &original);
        assert!(!host.session.state().document_file.modified);
        std::fs::remove_file(corrupt).unwrap();
        invoke(&mut host, CommandId::OpenDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::Open {
                    id,
                    epoch,
                    revision,
                    path: path.clone(),
                },
            )
            .unwrap();
        invoke(&mut host, CommandId::AddLayer);
        let edited = host.session.engine().document().clone();
        finish(&mut service, &mut host, &done);
        assert!(
            host.session
                .state()
                .host_error
                .as_ref()
                .unwrap()
                .contains("changed")
        );
        assert_eq!(host.session.engine().document(), &edited);
        assert_eq!(host.session.state().document_file.epoch, epoch);
        assert!(host.session.state().document_file.modified);
        assert!(!host.session.state().document_file.busy);
        service.stop_worker().unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "document_recovery_tests.rs"]
mod recovery_tests;
