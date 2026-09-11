//! Private Windows preferences. Disk writes never run on the UI/live canvas owner.
use layer_host::NativeHost;
use layer_ui::{HostRequestKind, Settings, UiAction};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

const MAX_BYTES: usize = 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn io_error(operation: &str, error: std::io::Error) -> String {
    format!("Could not {operation} preferences ({:?}).", error.kind())
}

pub(crate) struct SettingsFile {
    directory: PathBuf,
    preserve_existing: bool,
}
impl SettingsFile {
    fn environment() -> Result<Self, String> {
        let directory = if let Some(value) = std::env::var_os("CAPY_SETTINGS_DIRECTORY") {
            PathBuf::from(value)
        } else {
            PathBuf::from(
                std::env::var_os("LOCALAPPDATA").ok_or("Windows app data is unavailable.")?,
            )
            .join("CapyAtelier")
            .join("CapyCanvas")
        };
        Self::new(directory)
    }
    fn new(directory: PathBuf) -> Result<Self, String> {
        if !directory.is_absolute() {
            return Err("The preferences directory must be an absolute path.".into());
        }
        Ok(Self {
            directory,
            preserve_existing: false,
        })
    }
    fn load(&mut self) -> Result<Option<Settings>, String> {
        let result = self.read();
        self.preserve_existing = result.is_err();
        result
    }
    fn read(&self) -> Result<Option<Settings>, String> {
        let file = match File::open(self.directory.join("settings.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error("read saved", error)),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error("read saved", error))?;
        if bytes.len() > MAX_BYTES {
            return Err("Saved preferences exceed the size limit.".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "Saved preferences are not valid JSON.")?;
        // Reuse the shared migration and validation policy, including retired fields.
        let action: UiAction = serde_json::from_value(serde_json::json!({
            "type": "restore_settings", "settings": value
        }))
        .map_err(|_| "Saved preferences use an unsupported or invalid format.")?;
        let UiAction::RestoreSettings { settings } = action else {
            unreachable!()
        };
        settings
            .validate()
            .map_err(|error| format!("Saved preferences are invalid: {error}"))?;
        Ok(Some(settings))
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_BYTES {
            return Err("Preferences exceed the storage size limit.".into());
        }
        fs::create_dir_all(&self.directory)
            .map_err(|error| io_error("create storage for", error))?;
        // Windows canonicalization supplies a verbatim path, including long paths.
        let directory =
            fs::canonicalize(&self.directory).map_err(|error| io_error("locate", error))?;
        let target = directory.join("settings.json");
        let (temporary, mut file) = reserve(&directory, "pending")?;
        let result = (|| {
            file.write_all(bytes)
                .map_err(|error| io_error("write", error))?;
            file.sync_all().map_err(|error| io_error("flush", error))?;
            drop(file);
            if self.preserve_existing && target.is_file() {
                let (recovery, reserved) = reserve(&directory, "recovery")?;
                drop(reserved);
                if let Err(error) = replace(&target, &recovery) {
                    let _ = fs::remove_file(&recovery); // Only our reserved placeholder.
                    return Err(io_error("preserve unreadable", error));
                }
                self.preserve_existing = false;
            }
            replace(&temporary, &target).map_err(|error| io_error("replace saved", error))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
fn reserve(directory: &Path, kind: &str) -> Result<(PathBuf, File), String> {
    for _ in 0..32 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("settings.{kind}.{}.{id}.json", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create temporary", error)),
        }
    }
    Err("Could not reserve a preferences file.".into())
}
#[cfg(target_os = "windows")]
fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xffff))
}
#[cfg(not(target_os = "windows"))]
fn replace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
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
                    let error = write(&job.bytes).err(); // Never hold the mailbox during disk work.
                    state.mailbox.lock().unwrap().completed =
                        Some(Completion { id: job.id, error });
                    wake();
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

pub(crate) struct SettingsService {
    worker: Result<Worker, String>,
    submitted: Option<u32>,
    load_error: Option<String>,
}
impl SettingsService {
    /// Called on the render owner before GPU startup or queued user actions.
    pub(crate) fn open(native: &mut NativeHost, wake: impl Fn() + Send + 'static) -> Self {
        Self::at(native, SettingsFile::environment(), wake)
    }
    fn at(
        native: &mut NativeHost,
        file: Result<SettingsFile, String>,
        wake: impl Fn() + Send + 'static,
    ) -> Self {
        let mut load_error = None;
        let worker = file.and_then(|mut file| {
            match file.load() {
                Ok(Some(settings)) => {
                    if let Err(error) = native.dispatch(UiAction::RestoreSettings { settings }) {
                        load_error = Some(error);
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    load_error = Some(format!("{error} Defaults are in use; the saved file will be preserved on the next change."));
                }
            }
            Worker::start(move |bytes| file.write(bytes), wake)
        });
        if let Err(error) = &worker {
            load_error = Some(error.clone());
        }
        if native.error.is_none() {
            native.error = load_error.clone();
        }
        Self {
            worker,
            submitted: None,
            load_error,
        }
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
    pub(crate) fn poll(&mut self, native: &mut NativeHost) -> Result<(), String> {
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
            return Ok(());
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
            let result = encode(settings).and_then(|bytes| {
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
        // An older desired state no longer needs its own write. Keep the latest
        // request pending until durable completion, and retain any prior error.
        for &id in &saves[..saves.len() - 1] {
            let error = native.session.state().host_error.clone();
            native.dispatch(UiAction::CompleteRequest { id, error })?;
        }
        Ok(())
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
        match &mut self.worker {
            Ok(worker) => worker.finish(),
            Err(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
