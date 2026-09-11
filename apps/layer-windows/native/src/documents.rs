//! Document jobs transfer immutable state; the live canvas remains on its owner.
//! One job and one completion are bounded. GPU/session destruction stays on the worker.
use crate::document_io::{Stream, atomic_write, check_cancelled, io_error, location};
use layer_core::{Project, ProjectLimits};
use layer_host::{NativeHost, Renderer};
use layer_render::{CanvasRenderer, EffectValidationRequest};
use layer_render_wgpu::WgpuRasterizer;
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

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DocumentAction {
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
}
struct Environment {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    viewport: [u32; 2],
}
impl Environment {
    fn capture(host: &NativeHost) -> Result<Self, String> {
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
        })
    }
}
enum Source {
    New { width: u32, height: u32 },
    Open(PathBuf),
}
enum Job {
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
    Saved,
    Prepared(Box<UiSession<Renderer>>),
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Job>,
    completed: Option<Result<Completed, String>>,
    retired: Option<Box<UiSession<Renderer>>>,
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
                    let (job, retired, stopping, completed) = {
                        let mut mailbox = state.mailbox.lock().unwrap();
                        while mailbox.pending.is_none()
                            && mailbox.retired.is_none()
                            && !state.stopping.load(Ordering::Acquire)
                        {
                            mailbox = state.ready.wait(mailbox).unwrap();
                        }
                        let stopping = state.stopping.load(Ordering::Acquire);
                        (
                            mailbox.pending.take(),
                            mailbox.retired.take(),
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
    fn stop(&mut self) -> Result<(), String> {
        self.shared.stopping.store(true, Ordering::Release);
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
        Job::Save { project, path } => {
            let project = project.pruned()?;
            atomic_write(&path, cancel, |file| project.write(file))?;
            Ok(Completed::Saved)
        }
        Job::Prepare {
            environment,
            source,
        } => prepare(environment, source, cancel).map(Completed::Prepared),
    }
}
fn prepare(
    environment: Environment,
    source: Source,
    cancel: &AtomicBool,
) -> Result<Box<UiSession<Renderer>>, String> {
    let limits = ProjectLimits {
        dimension: environment
            .device
            .limits()
            .max_texture_dimension_2d
            .min(ProjectLimits::default().dimension),
        ..Default::default()
    };
    let project = match source {
        Source::New { width, height } => {
            if width > limits.dimension || height > limits.dimension {
                return Err("The canvas size exceeds this graphics device's limit".into());
            }
            layer_ui::new_drawing(width, height)?
        }
        Source::Open(path) => {
            let file = File::open(path).map_err(|e| io_error("open", e))?;
            Project::read(
                Stream {
                    inner: BufReader::new(file),
                    cancel,
                },
                limits,
            )?
        }
    };
    check_cancelled(cancel)?;
    // Eager preparation is isolated from the independently presented live canvas.
    #[allow(deprecated)]
    let mut gpu =
        WgpuRasterizer::from_wgpu(environment.adapter, environment.device, environment.queue)
            .map_err(|e| e.to_string())?;
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
    if !programs.is_empty() {
        gpu.request_effect_validation(EffectValidationRequest {
            request_id: 1,
            namespace: programs.clone(),
            programs,
        })
        .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            check_cancelled(cancel)?;
            gpu.device()
                .poll(wgpu::PollType::Poll)
                .map_err(|e| e.to_string())?;
            if let Some(result) = gpu.take_effect_validation() {
                result.result?;
                break;
            }
            if Instant::now() >= deadline {
                return Err("Project shader preparation timed out".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let mut candidate =
        UiSession::from_project(Renderer(Some(gpu)), project, None, environment.viewport)?;
    candidate.frame(0, 0)?;
    check_cancelled(cancel)?;
    Ok(Box::new(candidate))
}
struct Active {
    id: u32,
    epoch: u64,
    revision: u64,
    location: Option<DocumentLocation>,
}
pub(crate) struct DocumentService {
    worker: Worker,
    active: Option<Active>,
}
impl DocumentService {
    pub(crate) fn open(wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        Ok(Self {
            worker: Worker::start(wake)?,
            active: None,
        })
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
        host.session.require_document_idle()?;
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
        if let DocumentAction::Close = action {
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
            | DocumentAction::New { id, .. }
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
    pub(crate) fn poll(&mut self, host: &mut NativeHost) -> Result<(), String> {
        let Some(completed) = self.worker.take() else {
            return Ok(());
        };
        let active = self.active.take().ok_or("Unexpected document completion")?;
        let result = match completed {
            Ok(Completed::Saved) => {
                // Source checkpoint was captured before the worker ran; later edits stay dirty.
                if active.epoch == host.session.state().document_file.epoch {
                    Ok(true)
                } else {
                    Err("The saved document is no longer open".into())
                }
            }
            Ok(Completed::Prepared(candidate)) => {
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
    pub(crate) fn stop_worker(&mut self) -> Result<(), String> {
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
    fn invoke(host: &mut NativeHost, command: CommandId) {
        host.dispatch(UiAction::Invoke { command }).unwrap();
    }
    fn request(host: &NativeHost) -> (u32, u64, u64) {
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
    fn image(host: &mut NativeHost) -> layer_render::ReadbackImage {
        host.session.frame(0, 0).unwrap();
        // Explicit functional-test readback; project saving never reads the GPU.
        let renderer = host.session.renderer_mut();
        renderer.request_readback(1).unwrap();
        renderer.take_readback().unwrap().unwrap()
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
        host.import_layer_image(
            "Source image",
            layer_render::HostImage {
                width: 4,
                height: 3,
                stride: 16,
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: &[210, 45, 83, 180].repeat(12),
            },
        )
        .unwrap();
        let expected = image(&mut host);
        assert!(expected.bytes.chunks_exact(4).any(|p| p[3] != 0));
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
