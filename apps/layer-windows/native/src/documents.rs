//! Document jobs transfer immutable state; the live canvas remains on its owner.
//! One job and one completion are bounded. GPU/session destruction stays on the worker.
//!
use crate::document_io::{Stream, atomic_write, check_cancelled, io_error, location};
use layer_core::{Project, ProjectAsset, ProjectLimits};
use layer_host::{NativeHost, Renderer};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::{ExportReadback, WgpuRasterizer};
use layer_ui::{CloseDecision, DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use serde::Deserialize;
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
    RequestImport,
    NewPreferences { id: u32, action: layer_ui::NewDocumentAction },
    Recovery { action: crate::recovery::Action },
    WorkflowBegin { id: u32 },
    DropImages { epoch: u64, revision: u64, active_layer: u64, paths: Vec<String>,
        screen: Option<layer_core::Point>, layer: Option<(u64, f32)> },
    Workflow { id: u32, action: crate::document_workflows::Action },
    Create { id: u32, epoch: u64, revision: u64, options: layer_ui::NewDocumentOptions,
        #[serde(default)] preset: String, #[serde(default)] defaults: bool },
    Interpret { id: u32, profile: Option<crate::color_storage::ProfileChoice> },
    /// Lossless request identity; a null path is ordinary picker cancellation.
    ImportImage {
        id: String,
        path: Option<String>,
    },
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
    New {
        id: u32,
        epoch: u64,
        revision: u64,
        width: u32,
        height: u32,
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
    Export {
        id: u32,
        path: String,
    },
}
pub(crate) struct Environment {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: [u32; 2],
    defaults: layer_ui::NewDocumentOptions,
    photo_policy: layer_ui::PhotoOpenPolicy,
}
impl Environment {
    pub(crate) fn capture(host: &NativeHost) -> Result<Self, String> {
        let gpu = host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Wait for the canvas to finish starting")?;
        Ok(Self {
            adapter: gpu.adapter().clone(),
            device: gpu.device().clone(),
            queue: gpu.queue().clone(),
            viewport: host.session.state().camera.viewport,
            defaults: host.session.state().settings.new_document.defaults,
            photo_policy: host.session.state().settings.photo_open,
        })
    }
}
struct Opening { environment: Environment, imported: layer_ui::ImportedDocument, profiles: Vec<layer_ui::profile_library::ProfileEntry> }
enum Source {
    Create(layer_ui::NewDocumentOptions),
    Recovery(PathBuf),
    Interpret(Box<layer_ui::ImportedDocument>, crate::color_storage::ProfileChoice),
    New { width: u32, height: u32 },
    Open(PathBuf),
}
enum Job {
    Workflow { task: Box<crate::document_workflows::Task>, action: crate::document_workflows::Action },
    DiscardOpening(Box<Opening>),
    ImportImage {
        path: PathBuf,
        limit: u32,
        cancelled: Arc<AtomicBool>,
    },
    Export {
        readback: ExportReadback,
        path: PathBuf,
    },
    Save {
        project: Project,
        path: PathBuf,
    },
    Prepare {
        environment: Environment,
        source: Source,
    },
}
enum Completed {
    Workflow(Box<crate::document_workflows::Task>),
    Cancelled,
    Interpretation(Box<Opening>),
    PhotoPrepared(Box<UiSession<Renderer>>),
    Imported(ProjectAsset),
    Saved,
    Exported,
    Prepared(Box<UiSession<Renderer>>),
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Job>,
    completed: Option<Result<Completed, String>>,
    retired: Option<Box<UiSession<Renderer>>>,
    retired_image: Option<ProjectAsset>,
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
                    let (job, retired, retired_image, retired_workflow, stopping, completed) = {
                        let mut mailbox = state.mailbox.lock().unwrap();
                        while mailbox.pending.is_none()
                            && mailbox.retired.is_none()
                            && mailbox.retired_image.is_none()
                            && mailbox.retired_workflow.is_none()
                            && !state.stopping.load(Ordering::Acquire)
                        {
                            mailbox = state.ready.wait(mailbox).unwrap();
                        }
                        let stopping = state.stopping.load(Ordering::Acquire);
                        (
                            mailbox.pending.take(),
                            mailbox.retired.take(),
                            mailbox.retired_image.take(),
                            mailbox.retired_workflow.take(),
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
                    drop(retired_image);
                    drop(retired_workflow);
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
    fn take(&self, defer_import: bool) -> Option<Result<Completed, String>> {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        if defer_import && matches!(mailbox.completed, Some(Ok(Completed::Imported(_)))) {
            return None;
        }
        mailbox.completed.take()
    }
    fn retire(&self, session: Box<UiSession<Renderer>>) {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.retired.is_none());
        mailbox.retired = Some(session);
        self.shared.ready.notify_one();
    }
    fn discard_image(&self, image: ProjectAsset) {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        assert!(mailbox.retired_image.is_none());
        mailbox.retired_image = Some(image);
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
        Job::Workflow { mut task, action } => { task.work(action); Ok(Completed::Workflow(task)) }
        Job::DiscardOpening(opening) => { drop(opening); Ok(Completed::Cancelled) }
        Job::ImportImage {
            path,
            limit,
            cancelled,
        } => crate::image_import::decode(&path, limit, cancel, &cancelled).map(Completed::Imported),
        Job::Export { readback, path } => {
            // The ticket owns its GPU buffer; the live renderer stays on its owner.
            let image = readback.finish().map_err(|e| e.to_string())?;
            check_cancelled(cancel)?;
            atomic_write(&path, cancel, |file| image.write_png(file))?;
            Ok(Completed::Exported)
        }
        Job::Save { project, path } => {
            let project = project.pruned()?;
            atomic_write(&path, cancel, |file| project.write(file))?;
            Ok(Completed::Saved)
        }
        Job::Prepare {
            environment,
            source,
        } => prepare(environment, source, cancel),
    }
}
fn prepare(
    environment: Environment,
    source: Source,
    cancel: &AtomicBool,
) -> Result<Completed, String> {
    let limits = ProjectLimits {
        dimension: environment
            .device
            .limits()
            .max_texture_dimension_2d
            .min(ProjectLimits::default().dimension),
        ..Default::default()
    };
    let imported = match source {
        Source::New { width, height } => layer_ui::ImportedDocument {
            project: layer_ui::NewDocumentOptions { extent: [width, height], ..environment.defaults }.project()?,
            source: layer_ui::ImportSource::Master,
        },
        Source::Create(options) => layer_ui::ImportedDocument { project: options.project()?, source: layer_ui::ImportSource::Master },
        Source::Recovery(path) => layer_ui::read_import(Stream { inner: File::open(path).map_err(|e| io_error("open recovery", e))?, cancel },
            layer_ui::ImportIntent::Recovery, environment.photo_policy, "Recovered drawing", limits, Default::default(), cancel)?,
        Source::Interpret(mut imported, profile) => { imported.interpret(profile.resolve(cancel)?)?; *imported },
        Source::Open(path) => {
            let file = File::open(&path).map_err(|e| io_error("open", e))?;
            layer_ui::read_import(Stream { inner: BufReader::new(file), cancel }, layer_ui::ImportIntent::Open,
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
    check_cancelled(cancel)?;
    // Eager preparation is isolated from the independently presented live canvas.
    let mut gpu = WgpuRasterizer::from_wgpu_native_staged(environment.adapter,
        environment.device, environment.queue, project.document.color).map_err(|e| e.to_string())?;
    gpu.configure_ui_previews(layer_core::color::RgbSpace::Srgb).map_err(|e| e.to_string())?;
    gpu.finish_startup_cache();
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
        UiSession::from_project(Renderer(Some(gpu)), project, None, environment.viewport)?;
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
}
struct ImageImport {
    id: u64,
    submitted: bool,
    limit: u32,
    epoch: u64,
    revision: u64,
    target: u64,
    cancelled: Arc<AtomicBool>,
}
impl ImageImport {
    fn check(&self, host: &NativeHost) -> Result<(), String> {
        host.session.require_document_idle()?;
        let file = &host.session.state().document_file;
        let document = host.session.engine().document();
        if file.epoch != self.epoch
            || document.revision != self.revision
            || document.active_layer.0 != self.target
            || file.busy
            || file.close_ready
        {
            Err("The drawing changed while importing. Import the image again.".into())
        } else {
            Ok(())
        }
    }
}
pub(crate) struct DocumentService {
    import: Option<ImageImport>,
    next_import: u64,
    worker: Worker,
    active: Option<Active>,
    export: Option<PathBuf>,
    opening: Option<Box<Opening>>,
    workflow: Option<Box<crate::document_workflows::Task>>,
    workflow_control: Option<(u32, layer_render_wgpu::snapshot::CaptureControl)>,
    workflow_running: bool,
}
impl DocumentService {
    pub(crate) fn open(wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        Ok(Self {
            worker: Worker::start(wake)?,
            import: None,
            next_import: 1,
            active: None,
            export: None,
            opening: None,
            workflow: None,
            workflow_control: None,
            workflow_running: false,
        })
    }
    pub(crate) fn status(&self) -> Option<serde_json::Value> {
        if let Some(task) = &self.workflow { return Some(task.status()); }
        if self.workflow_running { return self.workflow_control.as_ref().map(|(id, _)| serde_json::json!({"type":"workflow_busy","id":id})); }
        self.opening.as_ref().map(|opening| serde_json::json!({
            "type": "interpret", "id": self.active.as_ref().map(|a| a.id),
            "spaces": layer_core::color::RgbSpace::ALL.map(|s| (s, s.name())),
            "profiles": opening.profiles,
            "channels": opening.imported.project.document.layers.iter().find_map(|l| l.source.as_ref()).map(|s| s.interpretation.channels),
        }))
    }
    pub(crate) fn importing(&self) -> bool {
        self.import.is_some()
    }

    pub(crate) fn import_request(&self) -> Option<serde_json::Value> {
        self.import.as_ref().map(|request| {
            serde_json::json!({
                "id": request.id.to_string(), "picking": !request.submitted,
            })
        })
    }
    fn request_import(&mut self, host: &mut NativeHost) -> Result<(), String> {
        if self.active.is_some() || self.import.is_some() || self.workflow_control.is_some() {
            return Err("A document operation is already running".into());
        }
        let document = host.session.engine().document();
        let mut import = ImageImport {
            id: self.next_import,
            submitted: false,
            limit: 0,
            epoch: host.session.state().document_file.epoch,
            revision: document.revision,
            target: document.active_layer.0,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        import.check(host)?;
        import.limit = host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("Wait for the canvas to finish starting")?
            .device()
            .limits()
            .max_texture_dimension_2d
            .min(crate::image_import::MAX_DIMENSION);
        self.next_import = self
            .next_import
            .checked_add(1)
            .ok_or("Image request identity exhausted")?;
        self.import = Some(import);
        host.error = None;
        host.invalidate_snapshot();
        Ok(())
    }
    fn import_picked(
        &mut self,
        host: &mut NativeHost,
        id: String,
        path: Option<String>,
    ) -> Result<(), String> {
        let id = id
            .parse::<u64>()
            .map_err(|_| "Invalid image import identity")?;
        if !self
            .import
            .as_ref()
            .is_some_and(|r| r.id == id && !r.submitted)
        {
            return Ok(()); // An old picker may finish after close or another document request.
        }
        let mut import = self.import.take().unwrap();
        host.invalidate_snapshot();
        let Some(path) = path else {
            return Ok(());
        };
        import.check(host)?;
        location(&path)?;
        self.worker.submit(Job::ImportImage {
            path: PathBuf::from(path),
            limit: import.limit,
            cancelled: import.cancelled.clone(),
        });
        import.submitted = true;
        self.import = Some(import);
        Ok(())
    }
    pub(crate) fn renderer_unavailable(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.cancel_import(host);
        if let Some((_, control)) = &self.workflow_control { control.cancel(); }
        if let Some(task) = self.workflow.take() {
            task.complete(host, false)?;
            self.workflow_control = None;
            self.worker.retire_workflow(task);
        }
        if self.export.take().is_some() {
            let active = self.active.take().ok_or("Missing PNG export request")?;
            Self::complete(
                host,
                active.id,
                Err("PNG export stopped because painting is unavailable".into()),
            )?;
        }
        Ok(())
    }
    fn cancel_import(&mut self, host: &mut NativeHost) {
        if let Some(import) = &self.import {
            import.cancelled.store(true, Ordering::Release);
        }
        if self.import.as_ref().is_some_and(|r| !r.submitted) {
            self.import = None;
            host.invalidate_snapshot();
        }
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
    pub(crate) fn dispatch(
        &mut self,
        host: &mut NativeHost,
        action: DocumentAction,
    ) -> Result<(), String> {
        if let DocumentAction::DropImages { epoch, revision, active_layer, paths, screen, layer } = action {
            Self::matches(host, epoch, revision)?;
            if self.active.is_some() || self.import.is_some() || self.workflow_control.is_some()
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
            if self.active.is_some() || self.import.is_some() || self.workflow_control.is_some() { return Err("A document operation is already running".into()); }
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
                self.worker.submit(Job::Prepare { environment, source: Source::Interpret(Box::new(imported), profile) });
            } else { self.worker.submit(Job::DiscardOpening(opening)); }
            host.invalidate_snapshot();
            return Ok(());
        }
        if let DocumentAction::NewPreferences { id, action } = action {
            if !matches!(Self::request(host,id)?,DocumentRequest::New) { return Err("Drawing preset dialog expired".into()); }
            return host.dispatch(layer_ui::UiAction::NewDocumentPreferences {action});
        }
        if let DocumentAction::RequestImport = action {
            return self.request_import(host);
        }
        if let DocumentAction::ImportImage { id, path } = action {
            return self.import_picked(host, id, path);
        }
        if let DocumentAction::Close = action {
            self.cancel_import(host);
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
        if self.active.is_some() || self.import.is_some() {
            return Err("A document operation is already running".into());
        }
        let id = match &action {
            DocumentAction::Cancel { id }
            | DocumentAction::Failure { id, .. }
            | DocumentAction::New { id, .. }
            | DocumentAction::Create { id, .. }
            | DocumentAction::Open { id, .. }
            | DocumentAction::Save { id, .. }
            | DocumentAction::Export { id, .. } => *id,
            _ => unreachable!(),
        };
        let request = Self::request(host, id)?;
        if matches!(request, DocumentRequest::ConfirmClose { .. }) {
            return Err("Respond to the unsaved changes dialog".into());
        }
        host.error = None;
        if let DocumentAction::Export { path, .. } = &action {
            let checked = (|| {
                if !matches!(request, DocumentRequest::Export { .. }) {
                    return Err("The file dialog no longer matches this document operation".into());
                }
                host.session.require_document_idle()?;
                if host.session.rendering_suspended() { return Err("PNG export requires an available GPU".into()); }
                location(path)?;
                Ok(())
            })();
            if let Err(error) = checked {
                return Self::complete(host, id, Err(error));
            }
            self.active = Some(Active {
                id,
                epoch: host.session.state().document_file.epoch,
                revision: host.session.engine().document().revision,
                location: None,
            });
            self.export = Some(PathBuf::from(path));
            host.dirty = true;
            return Ok(());
        }
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
                    let environment = Environment::capture(host)?;
                    if defaults || !preset.trim().is_empty() {
                        host.dispatch(layer_ui::UiAction::NewDocumentPreferences { action: layer_ui::NewDocumentAction::Remember {
                            options, name: preset, defaults,
                        } })?;
                    }
                    (Job::Prepare { environment, source: Source::Create(options) }, None)
                }
                DocumentAction::New {
                    epoch,
                    revision,
                    width,
                    height,
                    ..
                } if matches!(request, DocumentRequest::New) => {
                    Self::matches(host, epoch, revision)?;
                    let environment = Environment::capture(host)?;
                    (
                        Job::Prepare {
                            environment,
                            source: Source::New { width, height },
                        },
                        None,
                    )
                }
                DocumentAction::Open {
                    epoch,
                    revision,
                    path,
                    ..
                } if matches!(request, DocumentRequest::Open) => {
                    Self::matches(host, epoch, revision)?;
                    let selected = location(&path)?;
                    let environment = Environment::capture(host)?;
                    (
                        Job::Prepare {
                            environment,
                            source: Source::Open(PathBuf::from(path)),
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
                });
                self.worker.submit(job);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(error) => Self::complete(host, id, Err(error)),
        }
    }
    /// After the shared frame has applied pending document edits. No GPU wait,
    /// row packing or file I/O runs on the canvas owner.
    pub(crate) fn after_frame(&mut self, host: &mut NativeHost) -> Result<(), String> {
        if self.export.is_none() {
            return Ok(());
        }
        let active = self.active.as_ref().ok_or("Missing export request")?;
        let captured = (|| {
            // A delayed shader/replay must not silently export intervening edits.
            host.session.require_document_snapshot_idle()?;
            if active.epoch != host.session.state().document_file.epoch
                || active.revision != host.session.engine().document().revision
            {
                return Err(
                    "The drawing changed before its PNG snapshot was ready. Export again.".into(),
                );
            }
            if !host.startup.canvas_ready || host.session.engine().has_pending_document_edits() {
                return Ok(None);
            }
            let gpu = host
                .session
                .renderer_mut()
                .0
                .as_mut()
                .ok_or("Canvas is unavailable")?;
            if !gpu.export_ready() {
                return Ok(None);
            }
            gpu.begin_export_readback(u64::from(active.id))
                .map(Some)
                .map_err(|e| e.to_string())
        })();
        match captured {
            Ok(Some(readback)) => {
                let path = self.export.take().unwrap();
                self.worker.submit(Job::Export { readback, path });
            }
            Ok(None) => host.dirty = true,
            Err(error) => {
                self.export = None;
                let active = self.active.take().unwrap();
                Self::complete(host, active.id, Err(error))?;
            }
        }
        Ok(())
    }
    pub(crate) fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        // New/Open/Save/Export/Close supersede a pending import. Native dialogs
        // wait for its bounded worker slot to drain before responding.
        if host.session.state().document_file.busy || host.session.state().document_file.close_ready
        {
            self.cancel_import(host);
        }
        // Keep decoded CPU bytes in the bounded completion slot until they can
        // be uploaded. Canceled/stale imports and errors drain without a GPU.
        let defer_import = self.import.as_ref().is_some_and(|import| {
            if import.cancelled.load(Ordering::Acquire) || import.check(host).is_err() {
                return false;
            }
            let gpu = host.session.engine().backend().0.as_ref();
            let renderer_ready = gpu.is_some();
            #[cfg(target_os = "windows")]
            let renderer_ready = renderer_ready
                && !gpu.is_some_and(|gpu| crate::device::removed(gpu.device()));
            !renderer_ready
        });
        let Some(completed) = self.worker.take(defer_import) else {
            return Ok(());
        };
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
                self.workflow_control = None;
                self.worker.retire_workflow(task);
            } else {
                match task.prepare_owner(host) {
                    Ok(true) => { self.workflow_running = true; self.worker.submit(Job::Workflow { task, action: crate::document_workflows::Action::Compare }); return Ok(()); }
                    Err(error) => { task.fail(error); }
                    _ => {}
                }
                // Placement begins only after the native progress sheet has closed.
                // Its queued focus-loss event must precede the shared placement.
                if task.stage == "commit" && !task.awaits_placement_ui() {
                    match task.commit(host) {
                        Ok(()) => { self.workflow_control = None; self.worker.retire_workflow(task); host.invalidate_snapshot(); return Ok(()); }
                        Err(error) => task.fail(error),
                    }
                }
                self.workflow = Some(task);
            }
            host.invalidate_snapshot();
            return Ok(());
        }
        if let Some(import) = self.import.take() {
            host.invalidate_snapshot();
            let cancelled = import.cancelled.load(Ordering::Acquire);
            let result = match completed {
                Ok(Completed::Imported(image)) => {
                    if cancelled {
                        self.worker.discard_image(image);
                        Ok(())
                    } else if let Err(error) = import.check(host) {
                        self.worker.discard_image(image);
                        Err(error)
                    } else {
                        // The worker packed the source. Core retains an Arc and owns placement/Undo.
                        let result = host
                            .session
                            .import_layer_asset("Imported image", image.clone());
                        self.worker.discard_image(image);
                        host.dirty |= result.is_ok();
                        result
                    }
                }
                Err(_) if cancelled => Ok(()),
                Err(error) => Err(error),
                _ => Err("Unexpected image import completion".into()),
            };
            if let Err(error) = result {
                host.error = Some(error);
            }
            return Ok(());
        }
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
            Ok(Completed::Interpretation(_) | Completed::PhotoPrepared(_) | Completed::Workflow(_)) => unreachable!(),
            Ok(Completed::Imported(image)) => {
                self.worker.discard_image(image);
                Err("Unexpected image import completion".into())
            }
            Ok(Completed::Saved | Completed::Exported) => {
                // Only Save reserves a checkpoint. Export completion never clears dirty state.
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
                match host.session.adopt_project(
                    candidate,
                    active.epoch,
                    active.revision,
                    active.location,
                ) {
                    Ok(retired) => {
                        self.worker.retire(retired);
                        host.document_adopted();
                        Ok(true)
                    }
                    Err((error, candidate)) => {
                        self.worker.retire(candidate);
                        Err(error)
                    }
                }
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
        if let Some((_, control)) = &self.workflow_control { control.cancel(); }
        if let Some(task) = self.workflow.take() { self.worker.retire_workflow(task); }
        self.worker.stop()
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
    fn renderer_failure_cancels_waiting_export_but_preserves_an_accepted_save() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        let document = f.host.session.engine().document().clone();
        f.invoke(CommandId::ExportDocument);
        let export = f.path("Not captured.png");
        f.act(DocumentAction::Export {
            id: f.request(),
            path: export.clone(),
        });
        assert!(f.service.export.is_some());
        f.host.suspend_renderer().unwrap();
        f.service.renderer_unavailable(&mut f.host).unwrap();
        assert!(!f.host.session.state().document_file.busy);
        assert!(f.host.session.state().document_file.modified);
        assert!(
            f.host
                .session
                .state()
                .host_error
                .as_ref()
                .unwrap()
                .contains("PNG export stopped")
        );
        assert!(!std::path::Path::new(&export).exists());
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

    fn pending_import(f: &mut Fixture, id: u64, submitted: bool) {
        let document = f.host.session.engine().document();
        f.service.import = Some(ImageImport {
            id,
            submitted,
            limit: 8192,
            epoch: f.host.session.state().document_file.epoch,
            revision: document.revision,
            target: document.active_layer.0,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }
    #[test]
    fn import_picker_identity_cancel_and_invalid_path_do_not_edit_or_stick_busy() {
        let mut f = Fixture::new();
        let before = f.host.session.engine().document().clone();
        pending_import(&mut f, u64::MAX, false);
        assert_eq!(
            f.service.import_request().unwrap()["id"],
            u64::MAX.to_string()
        );
        f.act(DocumentAction::ImportImage {
            id: "1".into(),
            path: None,
        });
        assert!(
            f.service.importing(),
            "an obsolete picker cannot cancel the new one"
        );
        f.act(DocumentAction::ImportImage {
            id: u64::MAX.to_string(),
            path: None,
        });
        assert!(!f.service.importing());
        pending_import(&mut f, 2, false);
        assert!(
            f.service
                .dispatch(
                    &mut f.host,
                    DocumentAction::ImportImage {
                        id: "2".into(),
                        path: Some("relative.png".into())
                    }
                )
                .is_err()
        );
        assert!(!f.service.importing());
        assert_eq!(f.host.session.engine().document(), &before);
        assert!(!f.host.session.state().document_file.modified);
    }
    #[test]
    fn import_completion_rejects_changed_document_target_and_close_and_keeps_undo() {
        // Repeated idle/retired-worker teardown also covers stop racing with
        // the next condition-variable wait after a discarded completion.
        for change in (0..5).cycle().take(100) {
            let mut f = Fixture::new();
            pending_import(&mut f, 1, true);
            match change {
                0 => f.invoke(CommandId::AddLayer),
                1 => f.service.import.as_mut().unwrap().epoch += 1,
                2 => f.service.import.as_mut().unwrap().target += 1,
                3 => {
                    f.act(DocumentAction::Close);
                }
                _ => f.invoke(CommandId::SaveDocument),
            }
            let before = f.host.session.engine().document().clone();
            let asset = ProjectAsset {
                extent: [1, 1],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: Arc::from([255u8; 4]),
            };
            f.service.worker.shared.mailbox.lock().unwrap().completed =
                Some(Ok(Completed::Imported(asset)));
            f.service.poll(&mut f.host).unwrap();
            assert!(!f.service.importing());
            assert_eq!(f.host.session.engine().document(), &before);
            assert_eq!(f.host.error.is_some(), change < 3);
            if change == 0 {
                f.invoke(CommandId::Undo);
                assert_eq!(f.host.session.engine().document().layers.len(), 2);
            }
        }
    }
    #[test]
    fn decoded_import_waits_for_renderer_and_can_still_be_superseded() {
        let mut f = Fixture::new();
        pending_import(&mut f, 1, true);
        let original = f.host.session.engine().document().clone();
        let asset = ProjectAsset {
            extent: [1, 1],
            format: layer_core::ProjectAssetFormat::Rgba8Srgb,
            bytes: Arc::from([40u8, 60, 80, 255]),
        };
        f.service.worker.shared.mailbox.lock().unwrap().completed =
            Some(Ok(Completed::Imported(asset)));
        f.service.poll(&mut f.host).unwrap();
        assert!(
            f.service.importing(),
            "a temporary GPU gap must retain the decode"
        );
        assert!(f.host.error.is_none());
        assert_eq!(f.host.session.engine().document(), &original);
        // Save must remain able to cancel the import even without a renderer.
        f.invoke(CommandId::SaveDocument);
        f.service.poll(&mut f.host).unwrap();
        assert!(!f.service.importing());
        assert!(f.host.error.is_none());
        assert_eq!(f.host.session.engine().document(), &original);
        f.act(DocumentAction::Cancel { id: f.request() });
        assert!(!f.host.session.state().document_file.busy);
    }

    #[test]
    fn import_errors_recover_and_superseding_document_request_cancels_picker() {
        let mut f = Fixture::new();
        pending_import(&mut f, 1, true);
        f.service.worker.shared.mailbox.lock().unwrap().completed =
            Some(Err("Synthetic decoder failure".into()));
        f.service.poll(&mut f.host).unwrap();
        assert!(!f.service.importing());
        assert_eq!(f.host.error.as_deref(), Some("Synthetic decoder failure"));
        pending_import(&mut f, 2, false);
        f.invoke(CommandId::SaveDocument);
        f.service.poll(&mut f.host).unwrap();
        assert!(!f.service.importing());
        f.act(DocumentAction::Cancel { id: f.request() });
        assert!(!f.host.session.state().document_file.busy);
    }

    #[test]
    fn export_cancel_and_invalid_destination_preserve_the_drawing() {
        let mut f = Fixture::new();
        f.host.session.set_platform(Platform::Windows);
        f.invoke(CommandId::AddLayer);
        let document = f.host.session.engine().document().clone();
        f.invoke(CommandId::ExportDocument);
        f.act(DocumentAction::Cancel { id: f.request() });
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.state().document_file.location.is_none());
        f.invoke(CommandId::ExportDocument);
        f.act(DocumentAction::Export {
            id: f.request(),
            path: "relative.png".into(),
        });
        assert!(f.host.session.state().host_error.is_some());
        assert!(!f.host.session.state().document_file.busy);
        assert!(f.service.active.is_none());
        assert!(f.service.export.is_none());
        assert_eq!(f.host.session.engine().document(), &document);
        assert_eq!(std::fs::read_dir(&f.directory).unwrap().count(), 0);
    }

    #[test]
    fn deferred_export_rejects_intervening_edits_without_touching_the_destination() {
        let mut f = Fixture::new();
        let path = f.path("existing.png");
        std::fs::write(&path, b"previous file").unwrap();
        f.invoke(CommandId::ExportDocument);
        f.act(DocumentAction::Export {
            id: f.request(),
            path: path.clone(),
        });
        f.host.startup.canvas_ready = false;
        f.host.dirty = false;
        f.service.after_frame(&mut f.host).unwrap();
        assert!(
            f.host.dirty,
            "An unready export must schedule another canvas frame"
        );
        assert!(f.service.export.is_some());
        assert!(
            f.service
                .worker
                .shared
                .mailbox
                .lock()
                .unwrap()
                .pending
                .is_none()
        );
        f.invoke(CommandId::AddLayer);
        f.service.after_frame(&mut f.host).unwrap();
        assert!(f.service.active.is_none() && f.service.export.is_none());
        assert!(!f.host.session.state().document_file.busy);
        assert!(
            f.host
                .session
                .state()
                .host_error
                .as_ref()
                .unwrap()
                .contains("changed")
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"previous file");
        assert!(f.host.session.state().document_file.modified);
    }

    #[test]
    fn export_response_cannot_consume_a_save_request() {
        let mut f = Fixture::new();
        f.invoke(CommandId::AddLayer);
        f.invoke(CommandId::SaveDocument);
        f.act(DocumentAction::Export {
            id: f.request(),
            path: f.path("wrong.png"),
        });
        assert!(f.service.active.is_none());
        assert!(f.host.session.state().host_error.is_some());
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.state().document_file.location.is_none());
        assert_eq!(std::fs::read_dir(&f.directory).unwrap().count(), 0);
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
        let renderer = host.session.renderer_mut();
        renderer.request_readback(1).unwrap();
        renderer.take_readback().unwrap().unwrap()
    }

    pub(super) fn png_pixels(path: &std::path::Path) -> layer_render::ReadbackImage {
        let mut reader = png::Decoder::new(File::open(path).unwrap())
            .read_info()
            .unwrap();
        assert_eq!(
            reader.info().srgb,
            Some(png::SrgbRenderingIntent::Perceptual)
        );
        let mut bytes = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut bytes).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        layer_render::ReadbackImage {
            request_id: 0,
            width: info.width,
            height: info.height,
            stride: info.width * 4,
            bytes,
        }
    }
    pub(super) fn capture_export(
        service: &mut DocumentService,
        host: &mut NativeHost,
        path: &std::path::Path,
    ) {
        invoke(host, CommandId::ExportDocument);
        let (id, _, _) = request(host);
        service
            .dispatch(
                host,
                DocumentAction::Export {
                    id,
                    path: path.to_str().unwrap().into(),
                },
            )
            .unwrap();
        host.prepare_canvas_frame(0, 0, true).unwrap();
        service.after_frame(host).unwrap();
        assert!(
            service.export.is_none(),
            "Prepared export must issue a worker ticket"
        );
    }
    #[test]
    #[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
    fn d3d12_png_snapshot_preserves_alpha_checkpoint_and_atomic_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let gpu = WgpuRasterizer::new_headless().unwrap();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert_ne!(gpu.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
        let mut project = layer_ui::new_drawing(63, 47).unwrap();
        for layer in &mut project.document.layers {
            if layer.kind == layer_core::LayerKind::Background {
                layer.visible = false;
            }
        }
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session =
            UiSession::from_project(Renderer(Some(gpu)), project, None, [31, 29]).unwrap();
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
            "capy-export-gpu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let destination = directory.join("透明 café.png");
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
        let expected = image(&mut host);
        assert!(expected.bytes.as_chunks::<4>().0.iter().any(|p| p[3] == 0));
        assert!(
            expected
                .bytes
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0 && p[3] < 255)
        );
        invoke(&mut host, CommandId::ZoomIn);
        invoke(&mut host, CommandId::RotateRight);
        capture_export(&mut service, &mut host, &destination);
        // New GPU work runs while the independently owned ticket completes.
        host.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: 0.25,
        })
        .unwrap();
        let later = image(&mut host);
        assert_ne!(later.bytes, expected.bytes);
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_none());
        let exported = png_pixels(&destination);
        assert_eq!(
            [exported.width, exported.height, exported.stride],
            [63, 47, 252]
        );
        assert_eq!(
            exported.bytes, expected.bytes,
            "Export excludes viewport rotation, zoom and later edits"
        );
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
        let previous = std::fs::read(&destination).unwrap();
        let locked = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&destination)
            .unwrap();
        capture_export(&mut service, &mut host, &destination);
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_some());
        assert!(host.session.state().document_file.modified);
        drop(locked);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            previous,
            "Failed replacement preserves the previous PNG"
        );
        capture_export(&mut service, &mut host, &destination);
        finish(&mut service, &mut host, &done);
        assert!(host.session.state().host_error.is_none());
        assert_eq!(png_pixels(&destination).bytes, later.bytes);
        assert!(host.session.state().document_file.modified);
        assert_eq!(
            std::fs::read_dir(&directory).unwrap().count(),
            2,
            "Temporary files are cleaned after failure and retry"
        );
        service.stop_worker().unwrap();
        let detached = directory.join("detached.png");
        let readback = host
            .session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap()
            .begin_export_readback(900)
            .unwrap();
        // Deliberately hold this ticket until after later GPU work and renderer
        // destruction. Its worker must need neither the live canvas nor its owner.
        host.dispatch(UiAction::SetLayerOpacity {
            id: None,
            opacity: 0.1,
        })
        .unwrap();
        assert_ne!(image(&mut host).bytes, later.bytes);
        drop(host);
        let output = detached.clone();
        let completed = std::thread::spawn(move || {
            execute(
                Job::Export {
                    readback,
                    path: output,
                },
                &AtomicBool::new(false),
            )
        })
        .join()
        .unwrap()
        .unwrap();
        assert!(matches!(completed, Completed::Exported));
        assert_eq!(png_pixels(&detached).bytes, later.bytes);
        std::fs::remove_file(detached).unwrap();
        std::fs::remove_file(destination).unwrap();
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
            Renderer(Some(gpu)),
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
        let source = directory.join("source.png");
        let rgba = [210u8, 45, 83, 180].repeat(12);
        {
            let mut encoder = png::Encoder::new(File::create(&source).unwrap(), 4, 3);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&rgba)
                .unwrap();
        }
        let clean = image(&mut host);
        service
            .dispatch(&mut host, DocumentAction::RequestImport)
            .unwrap();
        let id = service.import_request().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        service
            .dispatch(
                &mut host,
                DocumentAction::ImportImage {
                    id: id.clone(),
                    path: Some(source.to_str().unwrap().into()),
                },
            )
            .unwrap();
        assert!(
            service
                .dispatch(&mut host, DocumentAction::RequestImport)
                .is_err(),
            "one decode at a time"
        );
        service
            .dispatch(&mut host, DocumentAction::ImportImage { id, path: None })
            .unwrap();
        assert!(
            service.importing(),
            "duplicate picker reply cannot cancel a submitted decode"
        );
        finish(&mut service, &mut host, &done);
        assert!(!service.importing());
        assert!(host.error.is_none(), "{:?}", host.error);
        assert_eq!(host.session.engine().document().layers.len(), 3);
        let expected = image(&mut host);
        assert_ne!(expected.bytes, clean.bytes);
        invoke(&mut host, CommandId::Undo);
        assert_eq!(image(&mut host).bytes, clean.bytes);
        assert!(!host.session.state().document_file.modified);
        invoke(&mut host, CommandId::Redo);
        assert_eq!(image(&mut host).bytes, expected.bytes);

        // A completed decode is still rejected if editing changed before adoption.
        service
            .dispatch(&mut host, DocumentAction::RequestImport)
            .unwrap();
        let id = service.import_request().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        service
            .dispatch(
                &mut host,
                DocumentAction::ImportImage {
                    id,
                    path: Some(source.to_str().unwrap().into()),
                },
            )
            .unwrap();
        done.recv_timeout(Duration::from_secs(15)).unwrap();
        invoke(&mut host, CommandId::AddLayer);
        service.poll(&mut host).unwrap();
        assert!(host.error.as_ref().unwrap().contains("changed"));
        assert_eq!(host.session.engine().document().layers.len(), 4);
        invoke(&mut host, CommandId::Undo);
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
        let saved = Project::read(File::open(&path).unwrap(), Default::default()).unwrap();
        assert_eq!(saved.assets.len(), 1);
        assert_eq!(&*saved.assets.values().next().unwrap().bytes, &rgba);
        std::fs::remove_file(&source).unwrap(); // Reopening must use the embedded source.
        assert_eq!(image(&mut host).bytes, expected.bytes);
        invoke(&mut host, CommandId::NewDocument);
        let (id, epoch, revision) = request(&host);
        service
            .dispatch(
                &mut host,
                DocumentAction::New {
                    id,
                    epoch,
                    revision,
                    width: 96,
                    height: 72,
                },
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
                DocumentAction::New {
                    id,
                    epoch,
                    revision,
                    width: 0,
                    height: 48,
                },
            )
            .unwrap();
        finish(&mut service, &mut host, &done);
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
