//! Windows recovery transport. Shared RecoveryState owns checkpoint, origin and
//! close ordering. File locks keep concurrent windows from claiming one copy.
use crate::{
    document_io::{atomic_write, check_cancelled, io_error},
    documents::Environment,
};
use layer_core::Project;
use layer_host::Renderer;
use layer_ui::{
    UiSession,
    recovery::{RecoveryEvent, RecoveryState, RecoveryUpdate, RecoveryWork, RecoveryWorkKind},
};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
fn valid(key: &str) -> bool {
    !key.is_empty() && key.len() <= 96 && key.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
struct Storage {
    directory: PathBuf,
    key: String,
    _lease: File,
    origin: Option<(String, File)>,
}
impl Storage {
    fn open(directory: PathBuf) -> Result<(Self, Option<String>), String> {
        fs::create_dir_all(&directory).map_err(|e| io_error("prepare recovery storage", e))?;
        let key = format!(
            "{:x}-{:x}-{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let lease = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(directory.join(format!("{key}.lock")))
            .map_err(|e| io_error("claim recovery", e))?;
        lease
            .try_lock()
            .map_err(|_| "Could not claim private recovery storage")?;
        let mut candidates = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|e| io_error("list recovery copies", e))? {
            let entry = entry.map_err(|e| io_error("list recovery copies", e))?;
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("capy") {
                continue;
            }
            let Some(id) = path
                .file_stem()
                .and_then(|v| v.to_str())
                .filter(|v| valid(v))
            else {
                continue;
            };
            let metadata = entry
                .metadata()
                .map_err(|e| io_error("inspect recovery copy", e))?;
            if metadata.is_file() {
                candidates.push((metadata.modified().unwrap_or(UNIX_EPOCH), id.to_string()));
            }
        }
        candidates.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        let mut origin = None;
        for (_, id) in candidates {
            let Ok(lease) = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(directory.join(format!("{id}.lock")))
            else {
                continue;
            };
            if lease.try_lock().is_ok() {
                origin = Some((id, lease));
                break;
            }
        }
        let offer = origin.as_ref().map(|(id, _)| id.clone());
        Ok((
            Self {
                directory,
                key,
                _lease: lease,
                origin,
            },
            offer,
        ))
    }
    fn path(&self, key: &str) -> Result<PathBuf, String> {
        if !valid(key)
            || (key != self.key && self.origin.as_ref().map(|(id, _)| id.as_str()) != Some(key))
        {
            return Err("Recovery copy is not owned by this window".into());
        }
        Ok(self.directory.join(format!("{key}.capy")))
    }
    fn remove(&self, key: &str) -> Result<(), String> {
        match fs::remove_file(self.path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error("retire recovery copy", e)),
        }
    }
}
enum Job {
    Initialize,
    Work {
        work: RecoveryWork,
        project: Option<Box<Project>>,
        environment: Option<Box<Environment>>,
    },
    Release(Vec<String>),
    RetiredSession(Box<UiSession<Renderer>>),
    RetiredRenderer(Box<Renderer>),
    Stop,
}
enum Finished {
    Storage(Result<Option<String>, String>),
    Work(u64, Result<Option<Box<UiSession<Renderer>>>, String>),
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Restore,
    Later,
    Discard,
    Retry,
    KeepOpen,
}
pub(crate) struct Restored {
    pub token: u64,
    pub identity: (u64,u64),
    pub candidate: Box<UiSession<Renderer>>,
}
pub(crate) struct Service {
    restored: Option<Restored>,
    changed: bool,
    state: RecoveryState,
    update: RecoveryUpdate,
    send: SyncSender<Job>,
    receive: Receiver<Finished>,
    completed: Option<Finished>,
    thread: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
    ready: bool,
    closing: bool,
    error: Option<String>,
    next_observation: Instant,
    restore_identity: Option<(u64, u64)>,
    deferred: VecDeque<Job>,
}
impl Service {
    pub fn open(wake: impl Fn() + Send + 'static) -> Result<Self, String> {
        let directory = crate::settings::data_directory()?.join("recovery");
        let (send, jobs) = mpsc::sync_channel(4);
        let (reply, receive) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stopping = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("capy-recovery".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let (mut storage, offer) = match Storage::open(directory.clone()) {
                    Ok((s, o)) => (Some(s), Ok(o)),
                    Err(e) => (None, Err(e)),
                };
                let _ = reply.send(Finished::Storage(offer));
                wake();
                while let Ok(job) = jobs.recv() {
                    match job {
                        Job::Initialize => {
                            let result = Storage::open(directory.clone());
                            let offer = match result {
                                Ok((next, offer)) => {
                                    storage = Some(next);
                                    Ok(offer)
                                }
                                Err(error) => Err(error),
                            };
                            let _ = reply.send(Finished::Storage(offer));
                            wake();
                        }
                        Job::Stop => break,
                        Job::RetiredSession(session) => drop(session),
                        Job::RetiredRenderer(renderer) => drop(renderer),
                        Job::Release(keys) => {
                            if let Some(storage) = &mut storage
                                && storage
                                    .origin
                                    .as_ref()
                                    .is_some_and(|(id, _)| keys.contains(id))
                            {
                                storage.origin = None;
                            }
                        }
                        Job::Work {
                            work,
                            project,
                            environment,
                        } => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    check_cancelled(&stopping)?;
                                    let storage = storage
                                        .as_mut()
                                        .ok_or("Recovery storage is unavailable")?;
                                    match work.kind {
                                        RecoveryWorkKind::Capture => {
                                            let project = project
                                                .ok_or("Recovery snapshot is missing")?
                                                .pruned()?;
                                            atomic_write(
                                                &storage.path(&storage.key)?,
                                                &stopping,
                                                |file| project.write(file),
                                            )?;
                                            Ok(None)
                                        }
                                        RecoveryWorkKind::Retire => {
                                            storage.remove(&storage.key)?;
                                            Ok(None)
                                        }
                                        RecoveryWorkKind::RetireOrigin { key } => {
                                            storage.remove(&key)?;
                                            Ok(None)
                                        }
                                        RecoveryWorkKind::Restore { key } => {
                                            crate::documents::prepare_recovery(
                                                *environment
                                                    .ok_or("Recovery GPU is unavailable")?,
                                                storage.path(&key)?,
                                                &stopping,
                                            )
                                            .map(Some)
                                        }
                                    }
                                }))
                                .unwrap_or_else(|_| Err("Recovery worker failed".into()));
                            let _ = reply.send(Finished::Work(work.token, result));
                            wake();
                        }
                    }
                }
            })
            .map_err(|_| "Could not start recovery worker")?;
        Ok(Self {
            restored: None,
            changed: true,
            state: Default::default(),
            update: Default::default(),
            send,
            receive,
            completed: None,
            thread: Some(thread),
            cancel,
            ready: false,
            closing: false,
            error: None,
            next_observation: Instant::now(),
            restore_identity: None,
            deferred: VecDeque::new(),
        })
    }
    fn queue(&mut self, job: Job) {
        self.deferred.push_back(job);
    }
    fn drain(&mut self) -> Result<(), String> {
        while let Some(job) = self.deferred.pop_front() {
            match self.send.try_send(job) {
                Ok(()) => (),
                Err(mpsc::TrySendError::Full(job)) => {
                    self.deferred.push_front(job);
                    break;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err("Recovery worker stopped".into());
                }
            }
        }
        Ok(())
    }
    fn event(&mut self, session: &mut UiSession<Renderer>, event: RecoveryEvent) -> Result<(), String> {
        self.update = self.state.event(event)?;
        loop {
            if !self.update.release.is_empty() {
                let release = std::mem::take(&mut self.update.release);
                self.queue(Job::Release(release));
            }
            if let Some(work) = self.update.work.take() {
                let prepared = (|| {
                    let project = if matches!(work.kind, RecoveryWorkKind::Capture) {
                        Some(Box::new(session.capture_project_recovery()?))
                    } else {
                        None
                    };
                    let environment = if matches!(work.kind, RecoveryWorkKind::Restore { .. }) {
                        self.restore_identity = Some((
                            session.state().document_file.epoch,
                            session.engine().document().revision,
                        ));
                        Some(Box::new(Environment::capture(session)?))
                    } else {
                        None
                    };
                    Ok::<_, String>((project, environment))
                })();
                match prepared {
                    Ok((project, environment)) => {
                        self.queue(Job::Work {
                            work,
                            project,
                            environment,
                        });
                        break;
                    }
                    Err(error) => {
                        self.restore_identity = None;
                        self.error = Some(error);
                        self.update = self.state.event(RecoveryEvent::Complete {
                            token: work.token,
                            success: false,
                        })?;
                    }
                }
            } else {
                break;
            }
        }
        self.changed = true;
        self.drain()
    }
    pub fn poll(&mut self, session: &mut UiSession<Renderer>) -> Result<bool, String> {
        while let Some(completed) = self
            .completed
            .take()
            .or_else(|| self.receive.try_recv().ok())
        {
            // Startup filter validation may still own document replacement. Keep
            // the prepared candidate and its origin until the shared idle check
            // permits adoption; do not turn this transient state into a failure.
            if matches!(&completed, Finished::Work(_, Ok(Some(_))))
                && session.require_document_idle().is_err()
            {
                self.completed = Some(completed);
                break;
            }
            match completed {
                Finished::Storage(result) => match result {
                    Ok(offer) => {
                        self.ready = true;
                        self.event(session, RecoveryEvent::Ownership { owned: true })?;
                        if let Some(key) = offer {
                            self.event(session, RecoveryEvent::Offer { key, owned: true })?;
                        }
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.changed = true;
                    }
                },
                Finished::Work(token, result) => {
                    if let Ok(Some(candidate)) = result {
                        self.restored = Some(Restored { token, identity: self.restore_identity.ok_or("Recovery identity missing")?, candidate });
                        self.changed = true;
                        continue;
                    }
                    if result.is_err() {
                        self.restore_identity = None;
                    }
                    self.error = result.as_ref().err().cloned();
                    self.event(
                        session,
                        RecoveryEvent::Complete {
                            token,
                            success: result.is_ok(),
                        },
                    )?;
                    self.next_observation = Instant::now();
                }
            }
        }
        {
            if session.state().document_file.close_ready && !self.closing {
                self.closing = true;
                self.event(
                    session,
                    RecoveryEvent::Retire {
                        discard_origin: true,
                    },
                )?;
                self.event(session, RecoveryEvent::Close)?;
            } else if !session.state().document_file.close_ready
                && self.closing
                && !self.update.busy
            {
                self.closing = false;
                self.event(session, RecoveryEvent::Resume)?;
            }
            if self.ready && !self.closing && Instant::now() >= self.next_observation {
                self.next_observation = Instant::now() + Duration::from_secs(3);
                self.event(
                    session,
                    RecoveryEvent::Observe {
                        document: session.recovery_document(),
                        owned: true,
                    },
                )?;
            }
        }
        self.drain()?;
        Ok(std::mem::take(&mut self.changed))
    }
    pub fn dispatch(&mut self, session: &mut UiSession<Renderer>, action: Action) -> Result<(), String> {
        if matches!(action, Action::Restore | Action::Retry | Action::Discard) {
            self.error = None;
        }
        match action {
            Action::Restore => {
                if session.state().document_file.modified {
                    return Err(
                        "Save or close the current drawing before restoring this copy".into(),
                    );
                }
                self.event(session, RecoveryEvent::Restore)
            }
            Action::Later => self.event(session, RecoveryEvent::Dismiss { discard: false }),
            Action::Discard => self.event(session, RecoveryEvent::Dismiss { discard: true }),
            Action::Retry if !self.ready => {
                self.queue(Job::Initialize);
                self.drain()
            }
            Action::Retry => {
                self.next_observation = Instant::now();
                self.event(
                    session,
                    if self.closing {
                        RecoveryEvent::Retire {
                            discard_origin: true,
                        }
                    } else {
                        RecoveryEvent::Observe {
                            document: session.recovery_document(),
                            owned: true,
                        }
                    },
                )
            }
            Action::KeepOpen => {
                session.reset_document_close();
                self.changed = true;
                Ok(())
            }
        }
    }
    pub fn take_restored(&mut self) -> Option<Restored> { self.restored.take() }
    pub fn complete_restore(&mut self, session: &mut UiSession<Renderer>, token: u64, result: Result<(),String>) -> Result<(),String> {
        self.restore_identity = None;
        if result.is_ok() { self.event(session, RecoveryEvent::Observe { document: session.recovery_document(), owned: true })?; }
        self.error = result.as_ref().err().cloned();
        self.event(session, RecoveryEvent::Complete { token, success: result.is_ok() })?;
        self.next_observation = Instant::now();
        Ok(())
    }
    pub fn restoring(&self) -> bool {
        self.restore_identity.is_some() && self.update.busy
    }
    pub fn close_ready(&self) -> bool {
        self.closing && self.update.current && self.error.is_none()
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"offer":self.update.offer,"busy":self.update.busy,"restoring":self.restoring(),"closing":self.closing,"ready":self.close_ready(),"error":self.error})
    }
    pub fn retire_renderer(&mut self, renderer: Renderer) {
        self.queue(Job::RetiredRenderer(Box::new(renderer)));
    }
    pub fn stop(&mut self) -> Result<(), String> {
        self.cancel.store(true, Ordering::Release);
        if let Some(restored) = self.restored.take() { self.queue(Job::RetiredSession(restored.candidate)); }
        if let Some(Finished::Work(_, Ok(Some(candidate)))) = self.completed.take() {
            self.queue(Job::RetiredSession(candidate));
        }
        while let Some(job) = self.deferred.pop_front() {
            self.send.send(job).map_err(|_| "Recovery worker stopped")?;
        }
        let _ = self.send.send(Job::Stop);
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "Recovery shutdown failed")?;
        }
        Ok(())
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "capy-recovery-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn live_windows_cannot_claim_each_others_checkpoint() {
        let dir = Directory::new();
        let (first, offer) = Storage::open(dir.0.clone()).unwrap();
        assert!(offer.is_none());
        fs::write(first.path(&first.key).unwrap(), b"durable checkpoint").unwrap();
        let (second, offer) = Storage::open(dir.0.clone()).unwrap();
        assert!(offer.is_none());
        let key = first.key.clone();
        drop(first);
        let (third, offer) = Storage::open(dir.0.clone()).unwrap();
        assert_eq!(offer.as_deref(), Some(key.as_str()));
        let (fourth, offer) = Storage::open(dir.0.clone()).unwrap();
        assert!(offer.is_none());
        assert!(third.path("../drawing").is_err());
        assert!(third.path(&second.key).is_err());
        drop(fourth);
        drop(third);
        drop(second);
    }
    #[test]
    fn failed_atomic_checkpoint_preserves_the_durable_copy() {
        let dir = Directory::new();
        let (storage, _) = Storage::open(dir.0.clone()).unwrap();
        let path = storage.path(&storage.key).unwrap();
        fs::write(&path, b"previous").unwrap();
        let cancel = AtomicBool::new(false);
        let result = atomic_write(&path, &cancel, |file| {
            file.write_all(b"partial").unwrap();
            Err("simulated encoder failure".into())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(path).unwrap(), b"previous");
        storage.remove(&storage.key).unwrap();
        storage.remove(&storage.key).unwrap();
    }
}
