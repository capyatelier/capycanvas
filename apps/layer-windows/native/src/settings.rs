//! Private Windows preferences. Disk writes never run on the UI/live canvas owner.
use layer_host::NativeHost;
use layer_ui::{HostRequestKind, Settings, UiAction};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::AtomicBool,
    },
    thread::JoinHandle,
};

const MAX_BYTES: usize = 1024 * 1024;

pub(crate) fn localization_presentation(native: &mut NativeHost) -> Result<serde_json::Value, String> {
    let mut catalog = native.query(serde_json::json!({"type":"catalog"}))?;
    catalog["delivery"] = serde_json::to_value(layer_ui::DocumentDeliveryCopy::new(native.session.localization())).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({"generation": native.localization_generation(), "bootstrap": native.bootstrap_view(), "catalog": catalog}))
}

fn io_error(operation: &str, error: std::io::Error) -> String {
    format!("Could not {operation} preferences ({:?}).", error.kind())
}

pub(crate) struct SettingsFile {
    path: PathBuf,
}
impl SettingsFile {
    fn environment() -> Result<Self, String> {
        Self::new(crate::storage::roots()?.settings())
    }
    fn new(path: PathBuf) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("The preferences file must have an absolute path.".into());
        }
        Ok(Self { path })
    }
    fn load_saved(&self) -> Result<Option<String>, String> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error("read saved", error)),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error("read saved", error))?;
        let saved = std::str::from_utf8(&bytes).ok().filter(|_| bytes.len() <= MAX_BYTES);
        Ok(Some(saved.unwrap_or_default().into()))
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_BYTES {
            return Err("Preferences exceed the storage size limit.".into());
        }
        if let Some(directory) = self.path.parent() {
            fs::create_dir_all(directory).map_err(|error| io_error("create storage for", error))?;
        }
        crate::document_io::atomic_write(&self.path, &AtomicBool::new(false), |file| {
            file.write_all(bytes).map_err(|error| io_error("write", error))
        })
    }
}
fn encode(settings: &Settings) -> Result<Vec<u8>, String> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_BYTES.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other(
                    "Preferences exceed the storage size limit.",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bounded(Vec::new());
    serde_json::to_writer(&mut bytes, settings).map_err(|error| error.to_string())?;
    Ok(bytes.0)
}

struct Save {
    id: u32,
    bytes: Vec<u8>,
}
struct Completion {
    id: u32,
    error: Option<String>,
}
#[derive(Default)]
struct Mailbox {
    pending: Option<Save>,
    completed: Option<Completion>,
    stopping: bool,
}
#[derive(Default)]
struct Shared {
    mailbox: Mutex<Mailbox>,
    ready: Condvar,
}
struct Worker {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(
        mut write: impl FnMut(&[u8]) -> Result<(), String> + Send + 'static,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared::default());
        let state = shared.clone();
        let thread = std::thread::Builder::new()
            .name("capy-preferences".into())
            .spawn(move || {
                loop {
                    let job = {
                        let mut mailbox = state.mailbox.lock().unwrap();
                        while mailbox.pending.is_none() && !mailbox.stopping {
                            mailbox = state.ready.wait(mailbox).unwrap();
                        }
                        match mailbox.pending.take() {
                            Some(job) => job,
                            None => return,
                        }
                    };
                    // A failed worker must complete the newest accepted save
                    // so close recovery cannot wait forever for a dead thread.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        write(&job.bytes) // Never hold the mailbox during disk work.
                    }));
                    let stopped = result.is_err();
                    {
                        let mut mailbox = state.mailbox.lock().unwrap();
                        let id = if stopped {
                            mailbox.stopping = true;
                            mailbox.pending.take().map_or(job.id, |pending| pending.id)
                        } else {
                            job.id
                        };
                        let error = result
                            .unwrap_or_else(|_| {
                                Err("Preferences storage stopped unexpectedly.".into())
                            })
                            .err();
                        mailbox.completed = Some(Completion { id, error });
                    }
                    wake();
                    if stopped {
                        return;
                    }
                }
            })
            .map_err(|error| io_error("start storage for", error))?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
    fn submit(&self, id: u32, bytes: Vec<u8>) -> Result<(), String> {
        let mut mailbox = self.shared.mailbox.lock().unwrap();
        if mailbox.stopping {
            return Err("Preferences storage is stopping.".into());
        }
        if bytes.len() > MAX_BYTES {
            return Err("Preferences exceed the storage size limit.".into());
        }
        // At most one in-flight write, one latest pending value and one completion.
        mailbox.pending = Some(Save { id, bytes });
        drop(mailbox);
        self.shared.ready.notify_one();
        Ok(())
    }
    fn completion(&self) -> Option<Completion> {
        self.shared.mailbox.lock().unwrap().completed.take()
    }
    fn finish(&mut self) -> Result<(), String> {
        self.shared.mailbox.lock().unwrap().stopping = true;
        self.shared.ready.notify_one();
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| "Preferences storage stopped unexpectedly.")?;
        }
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

mod shared;

#[derive(Clone, Default, PartialEq, serde::Serialize)]
pub(crate) struct CloseStatus {
    pub requested: bool,
    pub ready: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub attempt: u64,
}

pub(crate) struct PreparedSettings {
    hub: Result<Arc<shared::Hub>, String>,
}
impl PreparedSettings {
    pub(crate) fn start(self, native: &mut NativeHost, wake: impl Fn() + Send + 'static) -> SettingsService {
        SettingsService::from_hub(native, self.hub, wake)
    }
}
pub(crate) struct SettingsService {
    subscription: Option<shared::Subscription>,
    worker: Result<Worker, String>,
    submitted: Option<u32>,
    load_error: Option<String>,
    save_error: Option<String>,
    close: CloseStatus,
    localization_input_busy: bool,
}
impl SettingsService {
    #[cfg_attr(not(target_os = "windows"), expect(dead_code, reason = "Used by the Windows host"))]
    pub(crate) fn launch(preferred_tags: &[&str]) -> Result<(NativeHost, PreparedSettings), String> {
        Self::launch_at(SettingsFile::environment(), preferred_tags)
    }
    fn launch_at(file: Result<SettingsFile, String>, preferred_tags: &[&str]) -> Result<(NativeHost, PreparedSettings), String> {
        let prepared = file.and_then(|file| shared::Hub::with_launch(file, preferred_tags, |saved| {
            NativeHost::launch(layer_ui::Platform::Windows, saved.unwrap_or_default(), preferred_tags)
        }));
        let (native, hub) = match prepared {
            Ok((native, hub)) => (native, Ok(hub)),
            Err(error) => (NativeHost::launch(layer_ui::Platform::Windows, "", preferred_tags)?, Err(error)),
        };
        Ok((native, PreparedSettings { hub }))
    }
    #[cfg(test)]
    fn at(native: &mut NativeHost, file: Result<SettingsFile, String>, wake: impl Fn() + Send + 'static) -> Self {
        let hub = file.and_then(|file| shared::Hub::open(file, native.session.state().settings.clone()));
        Self::from_hub(native, hub, wake)
    }
    fn from_hub(native: &mut NativeHost, hub: Result<Arc<shared::Hub>, String>, wake: impl Fn() + Send + 'static) -> Self {
        let mut load_error = None;
        let mut subscription = None;
        let worker = hub.and_then(|hub| {
            load_error = hub.load_error();
            let mut client = shared::Subscription::new(hub.clone(), wake);
            if let Some(settings) = client.adopt(&native.session.state().settings) {
                native.dispatch(UiAction::RestoreSettings { settings })?;
            }
            let result = Worker::start(move |bytes| hub.write(bytes), client.notifier());
            subscription = Some(client);
            result
        });
        if let Err(error) = &worker {
            load_error = Some(error.clone());
        }
        if native.error.is_none() {
            native.error = load_error.clone();
        }
        Self { worker, subscription, submitted: None, load_error, save_error: None, close: CloseStatus::default(), localization_input_busy: false }
    }
    pub(crate) fn localization_input(&mut self, busy: bool) { self.localization_input_busy = busy; }
    fn sync(&mut self, native: &mut NativeHost) -> Result<(), String> {
        if let Some(client) = &mut self.subscription
            && let Some(settings) = client.adopt(&native.session.state().settings)
        {
            native.dispatch(UiAction::RestoreSettings { settings })?;
        }
        if !self.localization_input_busy && !native.session.localization_input_busy() && let Some(client) = &self.subscription {
            native.set_localization(client.localization());
        }
        Ok(())
    }
    fn complete(&mut self, native: &mut NativeHost, completion: Completion) -> Result<(), String> {
        // Superseded requests have already been retired; their late results
        // must not overwrite the latest save's status.
        if native
            .session
            .state()
            .requests
            .iter()
            .any(|request| request.id == completion.id)
        {
            self.save_error = completion.error.clone();
            if completion.error.is_none() && native.error == self.load_error {
                native.error = None;
                self.load_error = None;
            }
            native.dispatch(UiAction::CompleteRequest {
                id: completion.id,
                error: completion.error,
            })?;
        }
        Ok(())
    }
    fn poll_saves(&mut self, native: &mut NativeHost) -> Result<(), String> {
        let completed = self.worker.as_ref().ok().and_then(Worker::completion);
        if let Some(completed) = completed {
            self.complete(native, completed)?;
        }
        let saves: Vec<u32> = native
            .session
            .state()
            .requests
            .iter()
            .filter_map(|request| {
                matches!(request.kind, HostRequestKind::SaveSettings { .. }).then_some(request.id)
            })
            .collect();
        let Some(&latest) = saves.last() else {
            return self.sync(native);
        };
        if self.submitted != Some(latest) {
            let request = native
                .session
                .state()
                .requests
                .iter()
                .find(|request| request.id == latest)
                .unwrap();
            let HostRequestKind::SaveSettings { settings } = &request.kind else {
                unreachable!()
            };
            let encoded = match &mut self.subscription {
                Some(client) => client.edit(settings),
                None => encode(settings),
            };
            let result = encoded.and_then(|bytes| {
                self.worker
                    .as_ref()
                    .map_err(Clone::clone)?
                    .submit(latest, bytes)
            });
            self.submitted = Some(latest);
            if let Err(error) = result {
                self.complete(
                    native,
                    Completion {
                        id: latest,
                        error: Some(error),
                    },
                )?;
            }
        }
        self.sync(native)?;
        // An older desired state no longer needs its own write. Keep the latest
        // request pending until durable completion, and retain any prior error.
        for &id in &saves[..saves.len() - 1] {
            let error = native.session.state().host_error.clone();
            native.dispatch(UiAction::CompleteRequest { id, error })?;
        }
        Ok(())
    }
    pub(crate) fn close_status(&self) -> &CloseStatus {
        &self.close
    }
    fn pending(native: &NativeHost) -> bool {
        native
            .session
            .state()
            .requests
            .iter()
            .any(|request| matches!(request.kind, HostRequestKind::SaveSettings { .. }))
    }
    fn retry_save(&mut self, native: &mut NativeHost) -> Result<(), String> {
        if let Err(error) = native.session.retry_settings_save() {
            self.save_error = Some(error);
            return Ok(());
        }
        self.poll_saves(native)
    }
    pub(crate) fn poll(&mut self, native: &mut NativeHost) -> Result<(), String> {
        let previous = self.close.clone();
        self.poll_saves(native)?;
        if !native.session.state().document_file.close_ready {
            self.close = CloseStatus {
                attempt: self.close.attempt,
                ..Default::default()
            };
        } else if !self.close.requested {
            self.close.requested = true;
            self.close.attempt = self.close.attempt.saturating_add(1);
            // Flush accepted edits before the workspace releases its claim.
            // Shared dirty state also covers a write started by another window.
            if Self::pending(native)
                || self.save_error.is_some()
                || self.subscription.as_ref().is_some_and(|s| s.hub.dirty())
            {
                self.retry_save(native)?;
            }
        }
        if self.close.requested && !self.close.ready {
            self.close.busy = Self::pending(native);
            self.close.error = if self.close.busy {
                None
            } else {
                self.save_error.clone()
            };
            self.close.ready = !self.close.busy && self.close.error.is_none();
        }
        if self.close != previous {
            native.invalidate_snapshot();
        }
        Ok(())
    }
    pub(crate) fn retry_close(&mut self, native: &mut NativeHost) -> Result<(), String> {
        if self.close.requested && !self.close.busy && !self.close.ready {
            self.close.attempt = self.close.attempt.saturating_add(1);
            native.invalidate_snapshot();
            self.retry_save(native)?;
            self.poll(native)?;
        }
        Ok(())
    }
    pub(crate) fn keep_open(&mut self, native: &mut NativeHost) {
        if self.close.requested && !self.close.busy && !self.close.ready {
            native.session.reset_document_close();
            native.invalidate_snapshot();
        }
    }
    pub(crate) fn discard_close(&mut self, native: &mut NativeHost) {
        if self.close.requested && !self.close.busy && !self.close.ready {
            self.close.ready = true;
            self.close.error = None;
            native.invalidate_snapshot();
        }
    }
    /// Joins only on the render owner, before the callback context is destroyed.
    pub(crate) fn finish(&mut self, native: &mut NativeHost) -> Result<(), String> {
        let polled = self.poll(native);
        let stopped = self.stop_worker();
        polled?;
        stopped?;
        let completed = self.worker.as_ref().ok().and_then(Worker::completion);
        if let Some(completed) = completed {
            self.complete(native, completed)?;
        }
        Ok(())
    }
    pub(crate) fn stop_worker(&mut self) -> Result<(), String> {
        let result = match &mut self.worker {
            Ok(worker) => worker.finish(),
            Err(_) => Ok(()),
        };
        if let Some(client) = &self.subscription {
            client.stop();
        }
        result
    }
}

#[cfg(test)]
mod tests;
