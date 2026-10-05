//! Document jobs transfer immutable state; the live canvas remains on its owner.
//! One job and one completion are bounded. GPU/session destruction stays on the worker.
//!
use crate::document_io::{atomic_write, check_cancelled, io_error, location};
use layer_core::authored::ArtworkCapture;
use layer_core::package::codec::PreparedPackage;
use layer_host::{NativeHost, Renderer, open::OpenEnvironment};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{CloseDecision, DocumentLocation, DocumentRequest, HostRequestKind, UiSession};
use serde::Deserialize;
#[path = "document_tabs.rs"]
mod tabs;
use std::{
    fs::File,
    io::{BufReader, Read},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DocumentAction {
    Tabs { action: tabs::Action },
    Package { id:u32, action:layer_ui::PackageAction, path:Option<String> },
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
    ImportLookup {
        id: u32,
        path: String,
    },
}
pub(crate) fn recovery_environment(session: &UiSession<Renderer>) -> Result<OpenEnvironment, String> {
    OpenEnvironment::capture(
        session,
        layer_ui::DocumentSessions::<()>::localized(session.localization()).admission(&session.retained_document_tiles()),
        Default::default(),
    )
}
struct Opening { environment: OpenEnvironment, imported: layer_ui::ImportedDocument, profiles: Vec<layer_ui::profile_library::ProfileEntry>, profile_view: serde_json::Value, language: layer_ui::UiLanguage }
enum Source {
    Create(layer_ui::NewDocumentOptions),
    Interpret(Box<layer_ui::ImportedDocument>, crate::color_storage::ProfileChoice),
    Open(PathBuf),
}
enum Job {
    WritePackage {view:layer_ui::PackageView,action:layer_ui::PackageAction,original:Option<PathBuf>,path:PathBuf,refusal:Arc<str>,cancelled:Arc<AtomicBool>},
    Activate(Box<layer_host::window::Activation>),
    Spill { tiles: layer_core::raster_storage::RetainedTiles },
    Workflow { task: Box<crate::document_workflows::Task>, action: crate::document_workflows::Action },
    DiscardOpening(Box<Opening>),
    Save {
        project: Box<ArtworkCapture>,
        gpu: Option<Box<layer_render_wgpu::snapshot::SnapshotGpu>>,
        path: PathBuf,
        expected:Option<layer_ui::DestinationExpectation>,
        changed_message:String,
    },
    Lookup { path: PathBuf },
    Prepare {
        environment: Box<OpenEnvironment>,
        source: Source,
        cancelled: Arc<AtomicBool>,
    },
}
enum Completed {
    Package(layer_ui::PackageView),
    PackageWritten,
    Activated(Box<layer_host::window::Activation>),
    Spilled,
    Workflow(Box<crate::document_workflows::Task>),
    Cancelled,
    Interpretation(Box<Opening>),
    ProfileFailure(layer_ui::ColorFeatureError),
    PhotoPrepared(Box<UiSession<Renderer>>),
    Saved(layer_ui::DestinationFingerprint),
    Lookup(Arc<layer_core::Lut3d>),
    Prepared(Box<UiSession<Renderer>>),
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Job>,
    completed: Option<Result<Completed, String>>,
    retired: Vec<Box<UiSession<Renderer>>>,
    retired_renderer: Vec<Renderer>,
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
                            && mailbox.retired.is_empty()
                            && mailbox.retired_renderer.is_empty()
                            && mailbox.retired_workflow.is_none()
                            && !state.stopping.load(Ordering::Acquire)
                        {
                            mailbox = state.ready.wait(mailbox).unwrap();
                        }
                        let stopping = state.stopping.load(Ordering::Acquire);
                        (
                            mailbox.pending.take(),
                            std::mem::take(&mut mailbox.retired),
                            mailbox.retired_workflow.take(),
                            std::mem::take(&mut mailbox.retired_renderer),
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
        mailbox.retired.push(session);
        self.shared.ready.notify_one();
    }
    fn retire_renderer(&self, renderer: Renderer) {
        if renderer.0.is_none() { return; }
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        mailbox.retired_renderer.push(renderer);
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
        Job::WritePackage {view,action,original,path,refusal,cancelled} => {
            match action {
                layer_ui::PackageAction::CopyOriginal=>atomic_write(&path,&cancelled,|mut file|view.copy_original(&mut file,&cancelled))?,
                layer_ui::PackageAction::ExportPreview=>export_preview(&view,&path,original.as_deref(),&refusal,&cancelled)?,
                layer_ui::PackageAction::Close=>return Err("Closing a package does not write a file".into()),
            }
            Ok(Completed::PackageWritten)
        },
        Job::Activate(mut activation) => activation.work().map(|()| Completed::Activated(activation)),
        Job::Spill { tiles } => layer_core::raster_storage::spill_to_directory(&tiles, layer_core::temp_files::directory()?).map(|_| Completed::Spilled),
        Job::Workflow { mut task, action } => { task.work(action); Ok(Completed::Workflow(task)) }
        Job::DiscardOpening(opening) => { drop(opening); Ok(Completed::Cancelled) }
        Job::Save { project, gpu, path,expected,changed_message } => {
            let preview = gpu.and_then(|gpu| gpu.package_preview(&project, cancel));
            let package = PreparedPackage::prepare(&project, preview, cancel)?;
            let mut fingerprint=None;
            crate::document_io::atomic_write_checked(&path,cancel,|file|{
                let mut writer=layer_ui::FingerprintWriter::new(file);
                package.write(&mut writer,cancel)?;
                fingerprint=Some(writer.finish());Ok(())
            },||{
                if let Some(expected)=&expected {
                    let observed=match layer_ui::DestinationFingerprint::read_path(&path){Ok(value)=>Some(value),Err(error)=>{if path.exists(){return Err(error)}None}};
                    if !expected.matches(observed.as_ref()){return Err(changed_message.clone());}
                }
                Ok(())
            })?;
            Ok(Completed::Saved(fingerprint.ok_or("Save fingerprint is missing")?))
        }
        Job::Lookup { path } => {
            let mut bytes = Vec::new();
            std::fs::File::open(&path).and_then(|file| file.take(layer_core::Lut3d::MAX_TEXT_BYTES as u64 + 1).read_to_end(&mut bytes))
                .map_err(|e| io_error("read the lookup table", e))?;
            check_cancelled(cancel)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            Ok(Completed::Lookup(Arc::new(layer_core::Lut3d::parse_cube_named(&bytes, &name)?)))
        }
        Job::Prepare {
            environment,
            source,
            cancelled,
        } => prepare(*environment, source, &cancelled),
    }
}
fn export_preview(view:&layer_ui::PackageView,path:&std::path::Path,original:Option<&std::path::Path>,refusal:&str,cancel:&AtomicBool)->Result<(),String> {
    check_cancelled(cancel)?;
    if let Some(original)=original && crate::document_io::same_file(original,path)? {
        return Err(refusal.into());
    }
    atomic_write(path,cancel,|mut file|view.export_preview(&mut file,cancel))
}

fn prepare(
    environment: OpenEnvironment,
    source: Source,
    cancel: &AtomicBool,
) -> Result<Completed, String> {
    let mut opened_destination=None;
    let imported = match source {
        Source::Create(options) => layer_ui::ImportOutcome::Editable(layer_ui::ImportedDocument::new(options.project(&environment.localization)?,layer_ui::ImportSource::Master)),
        Source::Interpret(mut imported, profile) => {
            let profile=match profile.resolve(cancel, &environment.localization) { Ok(profile)=>profile,Err(reason)=>return Ok(Completed::ProfileFailure(reason)) };
            imported.interpret(profile)?; layer_ui::ImportOutcome::Editable(*imported)
        },
        Source::Open(path) => {
            let mut file = File::open(&path).map_err(|e| io_error("open", e))?;
            let fingerprint=layer_ui::DestinationFingerprint::read(&mut file)?;
            std::io::Seek::rewind(&mut file).map_err(|e|io_error("read",e))?;
            opened_destination=Some((location(path.to_str().ok_or("Drawing path is invalid")?)?,fingerprint));
            environment.read(layer_core::Cancellable { inner: BufReader::new(file), cancelled: || cancel.load(Ordering::Acquire) }, layer_ui::ImportIntent::Open,
                path.file_name().and_then(|v| v.to_str()).unwrap_or("Photo"), cancel)?
        }
    };
    let imported=match imported {layer_ui::ImportOutcome::Editable(imported)=>imported,layer_ui::ImportOutcome::Package(outcome)=>return layer_ui::PackageView::new(outcome).map(Completed::Package)};
    if imported.interpretation_required(environment.photo_policy).is_some() {
        let profiles=crate::color_storage::list(cancel)?;
        let profile_view=serde_json::json!(profiles.iter().map(|entry|entry.localized_view(&environment.localization)).collect::<Vec<_>>());
        let language=environment.localization.language();
        return Ok(Completed::Interpretation(Box::new(Opening { environment, imported, profiles, profile_view, language })));
    }
    let kind = imported.source;
    let mut candidate = match environment.prepare(imported.project.clone(), || cancel.load(Ordering::Acquire)) {
        Ok(candidate)=>candidate,
        Err(reason)=>{
            check_cancelled(cancel)?;
            return match imported.preserve_unsupported(reason.clone()) {
                Some(outcome)=>layer_ui::PackageView::new(outcome).map(Completed::Package),
                None=>Err(reason),
            };
        },
    };
    if kind!=layer_ui::ImportSource::Photo && let Some((location,fingerprint))=opened_destination {candidate.initialize_document_location(Some(location.clone()))?;candidate.record_destination_fingerprint(&location,fingerprint)?;}
    Ok(if kind == layer_ui::ImportSource::Photo { Completed::PhotoPrepared(candidate) } else { Completed::Prepared(candidate) })
}
#[cfg(test)]
pub(crate) fn prepare_package(environment:OpenEnvironment,path:PathBuf,cancel:&AtomicBool)->Result<Box<UiSession<Renderer>>,String>{
    match prepare(environment,Source::Open(path),cancel)?{
        Completed::Prepared(candidate)=>Ok(candidate),
        Completed::Package(view)=>Err(format!("The package did not admit an editable drawing: {}",serde_json::to_string(&view.summary(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English))).unwrap_or_default())),
        _=>Err("The package did not admit an editable drawing".into()),
    }
}
struct Active {
    id: u32,
    epoch: u64,
    revision: u64,
    location: Option<DocumentLocation>,
    cancelled: Option<Arc<AtomicBool>>,
}
struct PackageOpening {id:u32,name:String,original:Option<PathBuf>,view:layer_ui::PackageView,summary:serde_json::Value,serial:u32,writing:Option<layer_ui::PackageAction>,close_requested:bool,cancelled:Arc<AtomicBool>}
pub(crate) struct DocumentService {
    package:Option<PackageOpening>,
    next_package_id:u32,
    profile_copy: serde_json::Value,
    window: layer_host::window::DocumentWindow<tabs::Parked>,
    pub recovery: Option<crate::recovery::Service>,
    wake: Arc<dyn Fn() + Send + Sync>,
    activating: bool,
    spilling: bool,
    deferred_action: Option<DocumentAction>,
    close_window: bool,
    quit_pending:bool,
    close_next: bool,
    prepared_close:Option<layer_host::window::PreparedClose>,
    pub proof: crate::proof::Service,
    pub tone: layer_host::tone::ToneService,
    pub palettes: crate::palette_files::Service,
    worker: Worker,
    active: Option<Active>,
    opening: Option<Box<Opening>>,
    workflow: Option<Box<crate::document_workflows::Task>>,
    workflow_control: Option<(u32, layer_render_wgpu::snapshot::CaptureControl)>,
    workflow_running: bool,
    workflow_quiet: bool,
    workflow_title: Option<String>,
    open_queue: std::collections::VecDeque<String>,
    recording_save: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
}
impl DocumentService {
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
    pub(crate) fn set_localization(&mut self, localization: Arc<layer_ui::Localizer>) -> Result<(), String> {
        if self.window.set_localization(localization.clone()) {
            self.profile_copy = serde_json::json!(layer_ui::color_feature_copy::ProfileCopy::new(&localization));
        }
        if let Some(opening)=&mut self.opening && opening.language!=localization.language() {
            opening.profile_view=serde_json::json!(opening.profiles.iter().map(|entry|entry.localized_view(&localization)).collect::<Vec<_>>());
            opening.language=localization.language();
        }
        if let Some(package)=&mut self.package {package.summary=serde_json::to_value(package.view.summary(&localization)).map_err(|e|e.to_string())?;}
        if let Some(task) = &mut self.workflow { task.set_localization(localization)?; }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn open(wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        Self::open_localized(&layer_ui::Localizer::shared(layer_ui::UiLanguage::English), wake)
    }
    pub(crate) fn open_localized(localization: &layer_ui::Localizer, wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let wake = std::sync::Arc::new(wake);
        let notify = wake.clone();
        Ok(Self {
            proof: crate::proof::Service::new(wake.clone()),
            tone: layer_host::tone::ToneService::new(Some(wake.clone())),
            palettes: crate::palette_files::Service::new(wake.clone()),
            wake,
            window: layer_host::window::DocumentWindow::localized(localization),
            profile_copy: serde_json::to_value(layer_ui::color_feature_copy::ProfileCopy::new(localization)).unwrap(),
            recovery: None,
            activating: false,
            spilling: false,
            deferred_action: None,
            close_window: false,
            quit_pending:false,
            close_next: false,
            prepared_close:None,
            worker: Worker::start(move || notify())?,
            active: None,
            opening: None,
            package:None,next_package_id:0,
            workflow: None,
            workflow_control: None,
            workflow_running: false,
            workflow_quiet: false,
            workflow_title: None,
            open_queue: Default::default(),
            recording_save: None,
        })
    }
    #[cfg_attr(all(not(target_os = "windows"), not(test)), expect(dead_code, reason = "Used by the Windows host"))]
    pub(crate) fn status(&self) -> Option<serde_json::Value> {
        if let Some(package)=&self.package {return Some(serde_json::json!({"type":if package.writing.is_some() {"package_busy"}else{"package"},"id":package.id,"name":package.name,"serial":package.serial,"summary":package.summary,"title":package.summary[if package.writing==Some(layer_ui::PackageAction::ExportPreview) {"export_preview"}else{"copy_original"}]}));}
        if self.opening.is_none() && let Some(active) = &self.active && active.cancelled.is_some() { return Some(serde_json::json!({"type":"opening_busy","id":active.id})); }
        if let Some(task) = &self.workflow { return Some(task.status()); }
        if self.workflow_running { return self.workflow_control.as_ref().filter(|_| !self.workflow_quiet).map(|(id, _)| serde_json::json!({"type":"workflow_busy","id":id,"title":self.workflow_title})); }
        self.opening.as_ref().map(|opening| serde_json::json!({
            "type": "interpret", "id": self.active.as_ref().map(|a| a.id), "copy":self.profile_copy,
            "spaces": layer_core::color::RgbSpace::ALL.map(|s| (s, s.name())),
            "profiles": opening.profile_view,
            "channels": opening.imported.project.artwork.paint.iter().find_map(|(_,_,p)| p.original.as_ref()).map(|s| s.interpretation.channels),
        }))
    }
    pub(crate) fn renderer_unavailable(&mut self, host: &mut NativeHost) -> Result<(), String> {
        self.window.gpu = None;
        self.tone.clear();
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
            Err(layer_ui::DocumentTransportRefusal::SnapshotChanged.message(host.session.localization()).to_string())
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
    fn adopt_clip(&mut self, host: &mut NativeHost, task: &mut crate::document_workflows::Task) -> Result<(), String> {
        let clip = task.take_clip().ok_or("The copy did not finish")?;
        self.window.documents.clip = Some(clip);
        let previous = host.session.state().revision;
        let mut change = host.session.complete_document_request(task.id, Ok(true))?;
        change.canvas_wake = true;
        host.apply_change(previous, change);
        Ok(())
    }
    fn paste_clip(&mut self, host: &mut NativeHost, id: u32, nonce: &str) -> Result<(), String> {
        let DocumentRequest::Paste { mode } = Self::request(host, id)? else { return Err("The paste request is no longer active".into()) };
        let clip = self.window.documents.clip.as_ref().filter(|clip| clip.nonce == nonce).ok_or("Nothing was copied in this window")?;
        let previous = host.session.state().revision;
        host.session.paste_clip(clip, mode)?;
        let mut change = host.session.complete_document_request(id, Ok(true))?;
        change.canvas_wake = true;
        change.regions |= layer_ui::regions::ALL;
        host.apply_change(previous, change);
        Ok(())
    }
    const MAX_QUEUED_OPENS: usize = 64;
    fn open_idle(&self, host: &NativeHost) -> bool {
        let state = host.session.state();
        self.package.is_none() && self.active.is_none() && self.workflow_control.is_none() && !self.activating
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
        if let DocumentAction::Package {id,action,path}=action {
            let package=self.package.as_mut().filter(|p|p.id==id).ok_or("Package view expired")?;
            match action {
                layer_ui::PackageAction::Close=>{if package.writing.is_some() {package.close_requested=true;package.cancelled.store(true,Ordering::Release);}else{self.package=None;}},
                action @ (layer_ui::PackageAction::CopyOriginal|layer_ui::PackageAction::ExportPreview)=>{
                    if package.writing.is_some(){return Err("A package file is already being written".into());}
                    if action==layer_ui::PackageAction::ExportPreview&&!package.view.capabilities().export {return Err("This package has no verified preview".into());}
                    let path=path.ok_or("Choose a destination for the package file")?;location(&path)?;
                    package.serial=package.serial.wrapping_add(1);package.writing=Some(action);package.cancelled=Arc::new(AtomicBool::new(false));
                    let refusal=package.view.summary(host.session.localization()).destination_error;
                    self.worker.submit(Job::WritePackage {view:package.view.clone(),action,original:package.original.clone(),path:PathBuf::from(path),refusal,cancelled:package.cancelled.clone()});
                },
            }
            host.invalidate_snapshot();return Ok(());
        }
        if self.package.is_some(){return Err("Close the package view before starting another document operation".into());}
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
            self.recovery.as_mut().ok_or_else(|| layer_ui::DocumentTransportRefusal::RecoveryServiceUnavailable.message(host.session.localization()).to_string())?.dispatch(&mut host.session, action)?;
            host.invalidate_snapshot();
            return Ok(());
        }
        if self.spilling {
            if self.deferred_action.is_some() { return Err("A file response is already queued".into()); }
            self.deferred_action = Some(action);
            return Ok(());
        }
        if self.activating { return Err("Wait for the drawing to finish starting".into()); }
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
                || host.session.engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap_or(0) != active_layer { return Err("The canvas changed while receiving images; try again".into()); }
            host.dispatch(layer_ui::UiAction::Invoke { command: layer_ui::CommandId::ImportImage })?;
            let id = host.session.state().requests.iter().find(|r| matches!(r.kind, HostRequestKind::Document { request: DocumentRequest::Place })).ok_or("Image placement request is missing")?.id;
            let prepared = (|| { let mut task = crate::document_workflows::Task::capture(host, id)?;task.place_at(host, screen, layer)?;Ok::<_, String>(task) })();
            let task = match prepared { Ok(task) => task, Err(error) => return Self::complete(host, id, Err(error)) };
            self.workflow_quiet = false;self.workflow_title = None;
            self.workflow_control = Some((id, task.control.clone()));self.workflow_running = true;
            self.worker.submit(Job::Workflow { task, action: crate::document_workflows::Action::ReadImages { paths } });
            return Ok(());
        }
        if let DocumentAction::WorkflowBegin { id } = action {
            if self.active.is_some() || self.workflow_control.is_some() { return Err("A document operation is already running".into()); }
            let mut task = match crate::document_workflows::Task::capture(host, id) { Ok(task) => task, Err(error) => return Self::complete(host, id, Err(error)) };
            task.offer_clip(self.window.documents.clip.as_ref().map(|clip| clip.nonce.clone()));
            self.workflow_quiet = task.quiet();
            self.workflow_title = task.progress_title(host);
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
                crate::document_workflows::Action::Commit if task.awaits_clipboard() => self.adopt_clip(host, &mut task).or_else(|error| Self::complete(host, id, Err(error))),
                crate::document_workflows::Action::Commit => task.commit(host),
                crate::document_workflows::Action::PasteClip { nonce } => self.paste_clip(host, id, &nonce).or_else(|error| Self::complete(host, id, Err(error))),
                other => {
                    if let crate::document_workflows::Action::ExportWrite { path } = &other
                        && let Err(error) = task.prepare_write(host, path) {
                        task.fail(error);self.workflow=Some(task);host.invalidate_snapshot();return Ok(());
                    }
                    self.workflow_running = true;
                    self.worker.submit(Job::Workflow { task, action: other });
                    host.invalidate_snapshot();
                    return Ok(());
                }
            };
            if let Err(error) = result {
                if task.retained_failure() {
                    task.fail(error);
                    self.workflow=Some(task);
                    host.invalidate_snapshot();
                    return Ok(());
                }
                self.workflow=Some(task);
                return Err(error);
            }
            task.retain_proof(&mut host.proof)?;
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
                self.worker.submit(Job::Prepare { environment: Box::new(environment), source: Source::Interpret(Box::new(imported), profile), cancelled: self.active.as_ref().and_then(|a| a.cancelled.clone()).ok_or("Opening control missing")? });
            } else { self.worker.submit(Job::DiscardOpening(opening)); }
            host.invalidate_snapshot();
            return Ok(());
        }
        if let DocumentAction::NewPreferences { id, action } = action {
            if !matches!(Self::request(host,id)?,DocumentRequest::New) { return Err("Drawing preset dialog expired".into()); }
            return host.dispatch(layer_ui::UiAction::NewDocumentPreferences {action});
        }
        if let DocumentAction::Close = action {
            self.close_window = true;
            self.quit_pending=host.session.state().document_file.busy||self.active.is_some()||self.activating||!host.session.can_park_document();
            if self.quit_pending{return Ok(());}
            let previous = host.session.state().revision;
            let change = host.session.request_session_close()?;
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
            | DocumentAction::Save { id, .. }
            | DocumentAction::ImportLookup { id, .. } => *id,
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
                            project: Box::new(project),
                            gpu: host.session.engine().backend().0.as_ref().map(|renderer| Box::new(renderer.snapshot_gpu())),
                            changed_message:layer_ui::DocumentDeliveryMessage::DestinationChanged.message(host.session.localization()),
                            expected:host.session.save_destination_expectation().filter(|expected|expected.location.uri==selected.uri),
                            path: PathBuf::from(path),
                        },
                        Some(selected),
                    )
                }
                DocumentAction::Create { epoch, revision, options, preset, defaults, .. }
                    if matches!(request, DocumentRequest::New) => {
                    Self::matches(host, epoch, revision)?;
                    options.validate().map_err(|error| error.message(host.session.localization()))?;
                    let environment = OpenEnvironment::capture(&host.session,
                        self.window.documents.admission(&host.session.retained_document_tiles()), host.renderer_options(None))?;
                    if defaults || !preset.trim().is_empty() {
                        host.dispatch(layer_ui::UiAction::NewDocumentPreferences { action: layer_ui::NewDocumentAction::Remember {
                            options, name: preset, defaults,
                        } })?;
                    }
                    (Job::Prepare { environment: Box::new(environment), source: Source::Create(options), cancelled: Arc::new(AtomicBool::new(false)) }, None)
                }
                DocumentAction::Open {
                    epoch,
                    revision,
                    path,
                    ..
                } if matches!(request, DocumentRequest::Open) => {
                    Self::matches(host, epoch, revision)?;
                    let selected = location(&path)?;
                    let environment = OpenEnvironment::capture(&host.session,
                        self.window.documents.admission(&host.session.retained_document_tiles()), host.renderer_options(None))?;
                    (
                        Job::Prepare {
                            environment: Box::new(environment),
                            source: Source::Open(PathBuf::from(path)),
                            cancelled: Arc::new(AtomicBool::new(false)),
                        },
                        Some(selected),
                    )
                }
                DocumentAction::ImportLookup { path, .. } if matches!(request, DocumentRequest::ImportLookup { .. }) =>
                    (Job::Lookup { path: PathBuf::from(path) }, None),
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
        let prepared=self.worker.shared.mailbox.lock().unwrap().completed.as_ref().is_some_and(|result|matches!(result,Ok(Completed::Prepared(_)|Completed::PhotoPrepared(_))));
        if prepared && !self.active.as_ref().and_then(|active|active.cancelled.as_ref()).is_some_and(|cancel|cancel.load(Ordering::Acquire)) && !self.window.adoption_ready(host)? {return Ok(());}
        let Some(completed) = self.worker.take() else {
            return Ok(());
        };
        if self.package.as_ref().is_some_and(|p|p.writing.is_some()) {
            let package=self.package.as_mut().unwrap();package.writing=None;package.serial=package.serial.wrapping_add(1);
            if !package.close_requested {match completed {Ok(Completed::PackageWritten)=>{},Err(error)=>host.error=Some(error),_=>return Err("Unexpected package write completion".into())}}
            if package.close_requested {self.package=None;}
            host.invalidate_snapshot();return Ok(());
        }
        if self.activating { return self.activated(host, completed); }
        if self.spilling {
            self.spilling = false;
            self.window.documents.storage_completed(completed.and_then(|result| if matches!(result, Completed::Spilled) { Ok(()) } else { Err("Unexpected storage completion".into()) }));
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
                    return Self::complete(host, id, Err(error));
                }
                _ => return Err("Unexpected workflow completion".into()),
            };
            if task.control.is_cancelled() || task.stage == "saved" {
                task.complete(host, task.stage == "saved")?;
                if let Some(notice) = task.notice() {
                    host.session.set_host_error_copy(Some(notice));
                    host.invalidate_snapshot();
                }
                self.workflow_control = None;
                self.worker.retire_workflow(task);
            } else {
                match task.prepare_owner(host) {
                    Ok(Some(action)) => { self.workflow_running = true; self.worker.submit(Job::Workflow { task, action }); return Ok(()); }
                    Err(error) => { task.fail(error); }
                    Ok(None) => {}
                }
                // Placement begins only after the native progress sheet has closed.
                // Its queued focus-loss event must precede the shared placement.
                if task.stage == "commit" && !task.awaits_placement_ui() {
                    match task.commit(host) {
                        Ok(()) => { task.retain_proof(&mut host.proof)?; self.workflow_control = None; self.worker.retire_workflow(task); host.invalidate_snapshot(); return Ok(()); }
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
            Ok(Completed::Saved(fingerprint)) => {
                if active.epoch == host.session.state().document_file.epoch {
                    let location=active.location.clone().ok_or("Saved drawing destination is missing")?;
                    Self::complete(host,active.id,Ok(true))?;
                    host.session.record_destination_fingerprint(&location,fingerprint)?;return Ok(())
                } else {
                    Err("The completed file belongs to a document that is no longer open".into())
                }
            }
            Ok(Completed::Lookup(table)) => host.session.apply_lookup(active.id, table),
            Ok(Completed::Package(view))=>{self.present_package(host,view,active.location.as_ref().map(|location|PathBuf::from(&location.uri)))?;Self::complete(host,active.id,Ok(false))?;return Ok(());},
            Ok(Completed::PackageWritten)=>return Err("Unexpected package write completion".into()),
            Ok(Completed::Prepared(candidate)) => return self.append_candidate(host, active, candidate),
            Ok(Completed::ProfileFailure(reason)) => {
                let previous=host.session.state().revision;
                let change=host.session.complete_document_request_failed(active.id,layer_ui::DocumentHostErrorCopy::Profile(reason))?;
                host.apply_change(previous,change);return Ok(());
            },
            Err(error) => Err(error),
        };
        Self::complete(host, active.id, result)
    }
    fn present_package(&mut self,host:&mut NativeHost,view:layer_ui::PackageView,original:Option<PathBuf>)->Result<(),String> {
        let summary=serde_json::to_value(view.summary(host.session.localization())).map_err(|error|error.to_string())?;
        let name=original.as_ref().and_then(|path|path.file_name()).and_then(|name|name.to_str()).unwrap_or("Drawing.capy").into();
        self.next_package_id=self.next_package_id.wrapping_add(1);
        self.package=Some(PackageOpening{id:self.next_package_id,name,original,view,summary,serial:0,writing:None,close_requested:false,cancelled:Arc::new(AtomicBool::new(false))});
        host.invalidate_snapshot();Ok(())
    }

    #[cfg_attr(all(not(target_os = "windows"), not(test)), expect(dead_code, reason = "Used by the Windows host"))]
    pub(crate) fn preview(&self, id: u32, index: usize) -> Result<crate::previews::CapyPreview, String> {
        if let Some(package)=self.package.as_ref().filter(|p|p.id==id) {
            if index!=0{return Err("Unknown package preview".into());}
            let preview=package.view.preview().ok_or("Package has no preview")?;let [width,height]=preview.size();
            return crate::previews::CapyPreview::packet(serde_json::json!({"id":id,"width":width,"height":height}),preview.pixels().to_vec());
        }
        let task = self.workflow.as_ref().filter(|t| t.id == id).ok_or("Document preview expired")?;
        task.preview(index)
    }
    pub(crate) fn stop_worker(&mut self) -> Result<(), String> {
        if let Some(package)=&self.package {package.cancelled.store(true,Ordering::Release);}
        if let Some(cancelled) = self.active.as_ref().and_then(|a| a.cancelled.as_ref()) { cancelled.store(true, Ordering::Release); }
        if let Some(recovery) = &mut self.recovery { recovery.stop()?; }
        self.tone.clear();
        let proof = self.proof.stop();
        if let Some((_, control)) = &self.workflow_control { control.cancel(); }
        if let Some(task) = self.workflow.take() { self.worker.retire_workflow(task); }
        let worker = self.worker.stop();
        proof.and(worker)
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::*;
    use super::*;
    use crate::test_support::TempDir;
    use layer_ui::{CommandId, Platform, UiAction};
    use std::sync::mpsc;
    use std::time::Duration;
    #[test]
    fn localized_service_seeds_japanese_tab_caption_without_gpu() {
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese);
        let host = NativeHost::new_localized(Platform::Windows, localization.clone()).unwrap();
        let mut service = DocumentService::open_localized(host.session.localization(), || {}).unwrap();
        assert!(std::sync::Arc::ptr_eq(host.session.localization(), &localization));
        assert_eq!(service.window.view(&host, 900.)["tabs"][0]["title"], "無題 1");
        service.stop_worker().unwrap();
    }
    struct Fixture {
        host: NativeHost,
        service: DocumentService,
        done: mpsc::Receiver<()>,
        directory: TempDir,
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
            Self {
                host,
                service,
                done,
                directory: TempDir::new(),
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
            self.directory.path.join(name).to_str().unwrap().into()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.service.stop_worker().unwrap();
        }
    }

    #[test]
    fn window_quit_preserves_unsaved_work_without_a_discard_prompt() {
        let mut f=Fixture::new();f.host.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.4}).unwrap();let checkpoint=f.host.session.engine().checkpoint();
        f.host.suspend_renderer().unwrap();f.act(DocumentAction::Close);f.service.poll(&mut f.host).unwrap();
        assert!(f.host.session.state().document_file.close_ready);assert!(f.host.session.state().document_file.modified);assert_eq!(f.host.session.engine().checkpoint(),checkpoint);assert!(f.host.session.engine().can_undo());assert!(f.host.session.state().requests.is_empty());
    }
    #[test]
    fn window_quit_waits_for_an_accepted_save_and_preserves_later_edits() {
        let mut f=Fixture::new();f.host.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.4}).unwrap();f.invoke(CommandId::SaveDocumentAs);let path=f.path("accepted-save.capy");f.act(DocumentAction::Save{id:f.request(),path:path.clone()});
        f.host.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.8}).unwrap();let checkpoint=f.host.session.engine().checkpoint();f.act(DocumentAction::Close);
        assert!(f.service.quit_pending);assert!(!f.host.session.state().document_file.close_ready);f.finish();f.service.poll(&mut f.host).unwrap();
        assert!(std::path::Path::new(&path).is_file());assert!(f.host.session.state().document_file.close_ready);assert!(f.host.session.state().document_file.modified);assert_eq!(f.host.session.engine().checkpoint(),checkpoint);
    }
    #[test]
    fn package_outcomes_preserve_the_editor_and_write_original_or_verified_preview_on_the_worker() {
        use layer_core::package::{ImmutableBacking,codec::OpenOutcome,preview::Preview,transport::ChunkedBytes};
        let bytes=b"original package bytes including unsupported content";
        for disposition in 0..3 {
            let mut f=Fixture::new();f.invoke(CommandId::OpenDocument);let id=f.request();let before=f.host.session.engine().document().clone();let epoch=f.host.session.state().document_file.epoch;
            let source=ImmutableBacking::new(Arc::new(ChunkedBytes::new(vec![Arc::from(bytes.as_slice())]).unwrap())).unwrap();let preview=Preview::from_rgba([1,1],Arc::from([16,32,48,255])).unwrap();
            let outcome=match disposition {0=>OpenOutcome::Preserved{source,preview:Some(preview),outputs:Vec::new(),reason:"unsupported content".into()},1=>OpenOutcome::RecoveredView{source,preview,reason:"invalid graph".into()},_=>OpenOutcome::Failure{source,reason:"invalid package".into()}};
            f.service.active=Some(Active{id,epoch,revision:before.revision,location:Some(DocumentLocation{uri:f.path("original.capy"),name:"original.capy".into()}),cancelled:None});
            f.service.worker.shared.mailbox.lock().unwrap().completed=Some(Ok(Completed::Package(layer_ui::PackageView::new(outcome).unwrap())));f.service.poll(&mut f.host).unwrap();
            assert_eq!(f.host.session.engine().document(),&before);assert_eq!(f.host.session.state().document_file.epoch,epoch);assert!(f.service.active.is_none());assert!(f.host.session.state().requests.iter().all(|r|r.id!=id));
            let status=f.service.status().unwrap();let id=status["id"].as_u64().unwrap() as u32;assert_eq!(status["type"],"package");assert_eq!(status["summary"]["capabilities"]["edit"],false);assert_eq!(status["summary"]["capabilities"]["save"],false);assert_eq!(status["summary"]["capabilities"]["export"],disposition!=2);assert_eq!(f.service.preview(id,0).is_ok(),disposition!=2);
            let destination=f.path("copied.capy");f.act(DocumentAction::Package{id,action:layer_ui::PackageAction::CopyOriginal,path:Some(destination.clone())});assert_eq!(f.service.status().unwrap()["type"],"package_busy");f.finish();assert_eq!(std::fs::read(&destination).unwrap(),bytes);assert_eq!(f.host.session.engine().document(),&before);
            let cancelled=Arc::new(AtomicBool::new(true));let view=f.service.package.as_ref().unwrap().view.clone();assert!(execute(Job::WritePackage{view,action:layer_ui::PackageAction::CopyOriginal,original:None,path:PathBuf::from(destination.clone()),refusal:"refused".into(),cancelled},&AtomicBool::new(false)).is_err());assert_eq!(std::fs::read(destination).unwrap(),bytes);
            let view=f.service.package.as_ref().unwrap().view.clone();let png=f.path("preview.png");
            if disposition!=2 {
                f.act(DocumentAction::Package{id,action:layer_ui::PackageAction::ExportPreview,path:Some(png.clone())});
                assert_eq!(f.service.status().unwrap()["type"],"package_busy");assert_eq!(f.service.status().unwrap()["title"],status["summary"]["export_preview"]);
                f.finish();let exported=std::fs::read(&png).unwrap();assert_eq!(exported,view.preview().unwrap().encoded().as_ref());
                assert_eq!(Preview::decode(exported.clone().into()).unwrap().pixels().as_ref(),&[16,32,48,255]);
                std::fs::write(&png,b"").unwrap();export_preview(&view,std::path::Path::new(&png),None,"refused",&AtomicBool::new(false)).unwrap();assert_eq!(std::fs::read(&png).unwrap(),exported);
                let original=f.path("original.png");std::fs::write(&original,bytes).unwrap();assert_eq!(export_preview(&view,std::path::Path::new(&original),Some(std::path::Path::new(&original)),"refused",&AtomicBool::new(false)).unwrap_err(),"refused");assert_eq!(std::fs::read(&original).unwrap(),bytes);std::fs::remove_file(&original).unwrap();assert!(export_preview(&view,std::path::Path::new(&original),Some(std::path::Path::new(&original)),"refused",&AtomicBool::new(false)).is_err());assert!(!std::path::Path::new(&original).exists());
                assert_eq!(f.host.session.engine().document(),&before);assert_eq!(f.host.session.state().document_file.epoch,epoch);
            } else {
                assert!(f.service.dispatch(&mut f.host,DocumentAction::Package{id,action:layer_ui::PackageAction::ExportPreview,path:Some(png.clone())}).is_err());
                assert!(export_preview(&view,std::path::Path::new(&png),None,"refused",&AtomicBool::new(false)).is_err());assert!(!std::path::Path::new(&png).exists());
            }
            let cancelled_png=f.path("cancelled.png");assert!(execute(Job::WritePackage{view,action:layer_ui::PackageAction::ExportPreview,original:None,path:PathBuf::from(&cancelled_png),refusal:"refused".into(),cancelled:Arc::new(AtomicBool::new(true))},&AtomicBool::new(false)).is_err());assert!(!std::path::Path::new(&cancelled_png).exists());
            f.act(DocumentAction::Package{id,action:layer_ui::PackageAction::Close,path:None});assert!(f.service.package.is_none());assert_eq!(f.host.session.engine().document(),&before);
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
        let saved = read_document(File::open(path).unwrap(), Default::default()).unwrap();
        assert_authored_eq(&saved,&document);
        assert!(!f.host.session.state().document_file.modified);
    }

    #[test]
    fn suspended_renderer_saves_committed_raster_and_keeps_close_decisions() {
        use layer_core::raster::{
            RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey,
        };
        let mut f = Fixture::new();
        let mut project = layer_ui::new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let bytes = [27, 89, 143, 255].repeat(256 * 256);
        let descriptor = RasterPlane::Color.descriptor(project.composition().color);
        let tile = RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap());
        paint_mut(&mut project,0).raster = RasterRevision::backed(RasterData {
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
            UiSession::from_project(Renderer(None), project, None, [256, 256], Platform::Windows).unwrap();
        f.host.session.set_document_replacement(true);
        f.host.dispatch(UiAction::SetLayerOpacity{id:None,opacity:0.8}).unwrap();
        f.host.suspend_renderer().unwrap();
        f.service.renderer_unavailable(&mut f.host).unwrap();
        assert!(
            !paint_at(f.host.session.engine().document(),0).raster
                .is_empty(),
            "suspension must retain completed raster edits"
        );
        assert!(f.host.session.state().document_file.modified);
        assert!(f.host.session.command(CommandId::SaveDocumentAs).enabled);
        assert!(!f.host.session.command(CommandId::ExportDocument).enabled);
        assert!(!f.host.session.command(CommandId::Undo).enabled);
        f.invoke(CommandId::CloseDocument);
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
        let project = read_document(File::open(&path).unwrap(), Default::default()).unwrap();
        let mut expected = Vec::new();
        write_document(&source,&mut expected).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        let saved = paint_at(&project,0).raster.wait_data().unwrap();
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
        f.invoke(CommandId::CloseDocument);
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
        f.invoke(CommandId::CloseDocument);
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
        let project = read_document(File::open(path).unwrap(), Default::default()).unwrap();
        assert_authored_eq(&project,&saved);
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
        let active_layer = f.host.session.engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap();
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
        f.invoke(CommandId::CloseDocument);
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
                layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
                None,
                f.host.session.state().camera.viewport,
                Platform::Windows,
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
                assert_eq!([document.composition().size[0], document.composition().size[1]], [64, 48]);
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
    use crate::test_support::TempDir;
    use layer_ui::{CommandId, Platform, UiAction};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
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
        crate::test_support::temporary_files();
        let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert!(gpu.adapter().get_info().device_type != wgpu::DeviceType::Cpu || layer_render_wgpu::software_adapter_tests());
        let project = layer_ui::new_drawing(63, 47, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session =
            UiSession::from_project(Renderer(Some(gpu.into())), project, None, [31, 29], Platform::Windows).unwrap();
        host.session.set_document_replacement(true);
        host.resize(31, 29, 1.).unwrap();
        let source = layer_core::color::source::rgba8_source([4, 3], |_, _| [210, 45, 83, 180]);
        host.session
            .import_layer_source("Synthetic alpha", std::sync::Arc::unwrap_or_clone(source))
            .unwrap();
        host.dirty = true;
        let directory = TempDir::new();
        let source = directory.path.join("source.capy");
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
        let backing=layer_core::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(saved.clone()))).unwrap();
        let opened=layer_core::package::codec::open(backing,Default::default(),&AtomicBool::new(false)).unwrap();
        assert!(matches!(opened,layer_core::package::codec::OpenOutcome::Candidate {preview:Some(ref p),..} if p.size()==[63,47]));
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
    }
    #[test]
    #[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
    fn d3d12_background_save_open_new_and_stale_adoption() {
        crate::test_support::temporary_files();
        let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_eq!(gpu.adapter().get_info().backend, wgpu::Backend::Dx12);
        assert!(gpu.adapter().get_info().device_type != wgpu::DeviceType::Cpu || layer_render_wgpu::software_adapter_tests());
        let mut host = NativeHost::new(Platform::Gtk).unwrap();
        host.session = UiSession::from_project(
            Renderer(Some(gpu.into())),
            layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
            None,
            [64, 48],
            Platform::Gtk,
        )
        .unwrap();
        host.session.set_document_replacement(true);
        host.resize(64, 48, 1.).unwrap();
        let directory = TempDir::new();
        let path = directory
            .path
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
        let paper=host.session.engine().document().scene().order().iter().copied().find(|h|host.session.engine().document().scene().effect(*h).is_some_and(|effect|effect.program.id.as_ref()=="solid_color")).map(layer_ui::occurrence_token).unwrap();
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
        assert_eq!(host.session.engine().document().composition().size[0], 96);
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
        assert_eq!(host.session.engine().document().composition().size[0], 64);
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
        let corrupt = directory.path.join("invalid.capy");
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
    }
}

#[cfg(all(test, target_os = "windows"))]
#[path = "document_recovery_tests.rs"]
mod recovery_tests;
