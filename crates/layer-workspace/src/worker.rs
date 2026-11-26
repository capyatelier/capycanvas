use crate::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, mpsc},
};

type Result<T> = std::result::Result<T, StoreError>;
enum WorkerRequest {
    Store(StoreRequest),
    Backup(PathBuf),
    RetireOwner(Owner),
}
type Message = (WorkerRequest, async_channel::Sender<Result<StoreResponse>>);
pub struct StoreReply(async_channel::Receiver<Result<StoreResponse>>);

struct Worker {
    sender: Option<mpsc::Sender<Message>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        // The last native window drains already accepted requests and joins
        // SQLite teardown before its Rust library/process can be unloaded.
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[derive(Clone)]
pub struct StoreWorker(Arc<Worker>);
impl StoreWorker {
    /// Windows using one private directory share the same native I/O worker.
    /// Other processes coordinate through kernel locks and SQLite owner fences.
    pub fn shared(directory: &Path) -> Result<Self> {
        static WORKERS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Worker>>>> = OnceLock::new();
        let path = if directory.is_absolute() {
            directory.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| StoreError::new(ErrorKind::Unavailable, e.to_string()))?
                .join(directory)
        };
        let mut workers = WORKERS.get_or_init(Default::default).lock().map_err(|_| {
            StoreError::new(
                ErrorKind::Unavailable,
                "Workspace storage registry stopped.",
            )
        })?;
        workers.retain(|_, worker| worker.strong_count() > 0);
        if let Some(worker) = workers.get(&path).and_then(Weak::upgrade) {
            return Ok(Self(worker));
        }
        let (sender, receiver) = mpsc::channel::<Message>();
        let database = path.join("workspaces.sqlite3");
        let thread = std::thread::Builder::new()
            .name("workspace-storage".into())
            .spawn(move || {
                let mut store = SqliteStore::open(&database);
                while let Ok((request, reply)) = receiver.recv() {
                    let request = match request {
                        WorkerRequest::RetireOwner(owner) => {
                            if let Ok(store) = &mut store {
                                store.retire_owner(&owner);
                            }
                            continue;
                        }
                        WorkerRequest::Backup(destination) => {
                            let _ = reply.try_send(
                                backup_database(&database, &destination)
                                    .map(|()| StoreResponse::Done),
                            );
                            continue;
                        }
                        WorkerRequest::Store(request) => request,
                    };
                    if matches!(request, StoreRequest::SetAside) {
                        let result = if matches!(&store,Err(error) if error.kind==ErrorKind::DamagedStorage) {
                            let (opened, restored) = match set_aside_database(&database) {
                                Ok((opened, restored)) => (Ok(opened), restored),
                                Err(error) => (Err(error), false),
                            };
                            store = opened;
                            store.as_ref().map(|_|StoreResponse::SetAside {restored}).map_err(Clone::clone)
                        } else {
                            Err(StoreError::new(ErrorKind::Unavailable,"Only a damaged database can be set aside."))
                        };
                        if let Err(error) = &result && error.has_raw_detail() { eprintln!("Workspace storage {:?}: {}",error.kind,error.message); }
                        let _ = reply.try_send(result);
                        continue;
                    }
                    if matches!(request, StoreRequest::Reopen)
                        && store.is_err()
                    {
                        store = SqliteStore::open(&database);
                    }
                    let result = match &mut store {
                        Ok(store) => store.handle(request),
                        Err(error) => Err(error.clone()),
                    };
                    if let Err(error) = &result && error.has_raw_detail() { eprintln!("Workspace storage {:?}: {}",error.kind,error.message); }
                    // A dropped receiver leaves the persisted receipt/pending payload
                    // available for retry. It must never imply that a write failed.
                    let _ = reply.try_send(result);
                }
            })
            .map_err(|e| StoreError::new(ErrorKind::Unavailable, e.to_string()))?;
        let worker = Arc::new(Worker {
            sender: Some(sender),
            thread: Some(thread),
        });
        workers.insert(path, Arc::downgrade(&worker));
        Ok(Self(worker))
    }
    pub fn request(&self, request: StoreRequest) -> StoreReply {
        let (sender, receiver) = async_channel::unbounded();
        let _ = self
            .0
            .sender
            .as_ref()
            .unwrap()
            .send((WorkerRequest::Store(request), sender));
        StoreReply(receiver)
    }
    /// Native recovery also works for databases with an unsupported model
    /// schema: SQLite's backup API reads the coherent snapshot including WAL.
    pub fn backup_database(&self, destination: &Path) -> StoreReply {
        let (sender, receiver) = async_channel::unbounded();
        let _ = self
            .0
            .sender
            .as_ref()
            .unwrap()
            .send((WorkerRequest::Backup(destination.to_path_buf()), sender));
        StoreReply(receiver)
    }
}

/// Native file transport must not replace the live database or its sidecars.
/// Call on an I/O worker. Canonical parents also catch not-yet-created WAL/SHM
/// files and folder aliases; in-memory package export works if storage vanished.
pub fn validate_database_export_destination(source: &Path, destination: &Path) -> Result<()> {
    fn resolved(path: &Path) -> PathBuf {
        std::fs::canonicalize(path)
            .or_else(|_| {
                std::fs::canonicalize(path.parent().unwrap_or(Path::new(".")))
                    .map(|parent| parent.join(path.file_name().unwrap_or_default()))
            })
            .unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
    }
    let destination = resolved(destination);
    let lock_directory = resolved(&crate::sqlite::ownership::lock_directory(&resolved(source)));
    #[cfg(windows)]
    let in_locks = destination
        .to_string_lossy()
        .to_lowercase()
        .starts_with(&(lock_directory.to_string_lossy().to_lowercase() + "\\"))
        || destination
            .to_string_lossy()
            .eq_ignore_ascii_case(&lock_directory.to_string_lossy());
    #[cfg(not(windows))]
    let in_locks = destination.starts_with(&lock_directory);
    if in_locks {
        return Err(StoreError::invalid(
            "Choose an export file outside the live workspace ownership files.",
        ));
    }
    for protected in [
        source.to_path_buf(),
        PathBuf::from(format!("{}-wal", source.display())),
        PathBuf::from(format!("{}-shm", source.display())),
        PathBuf::from(format!("{}-lock", source.display())),
    ] {
        let protected = resolved(&protected);
        #[cfg(windows)]
        let same = protected
            .to_string_lossy()
            .eq_ignore_ascii_case(&destination.to_string_lossy());
        #[cfg(not(windows))]
        let same = protected == destination;
        if same {
            return Err(StoreError::invalid(
                "Choose an export file outside the live workspace database files.",
            ));
        }
    }
    Ok(())
}

fn set_aside_database(path: &Path) -> Result<(SqliteStore, bool)> {
    let io = |error: std::io::Error| StoreError::new(ErrorKind::Unavailable,error.to_string());
    let path = std::fs::canonicalize(path).map_err(io)?;
    let _recovery = crate::sqlite::ownership::storage_recovery_lock(&path)?;
    match SqliteStore::open(&path) {
        Err(error) if error.kind==ErrorKind::DamagedStorage => {},
        Err(error) => return Err(error),
        Ok(mut store) => {store.handle(StoreRequest::Reopen)?;return Err(StoreError::conflict());},
    }
    let destination = path.with_file_name(format!("workspaces.damaged.{}.sqlite3",new_id()));
    let mut files = Vec::new();
    for suffix in ["-wal", "-shm", ""] {
        let mut source = path.as_os_str().to_owned(); source.push(suffix);
        let mut target = destination.as_os_str().to_owned(); target.push(suffix);
        match std::fs::metadata(&source) {
            Ok(metadata) if metadata.is_file() => files.push((PathBuf::from(source),PathBuf::from(target))),
            Err(error) if !suffix.is_empty() && error.kind()==std::io::ErrorKind::NotFound => {},
            Ok(_) => return Err(StoreError::new(ErrorKind::Unavailable,"A database sidecar is not a file.")),
            Err(error) => return Err(io(error)),
        }
    }
    for (index,(source,target)) in files.iter().enumerate() {
        if let Err(error) = std::fs::rename(source,target) {
            let mut detail = error.to_string();
            for (source,target) in files[..index].iter().rev() {
                if let Err(error) = std::fs::rename(target,source) { detail.push_str(&format!("; {error}")); }
            }
            return Err(StoreError::new(ErrorKind::Unavailable,detail));
        }
    }
    let restored = crate::sqlite::install_newest_copy(&path)?;
    Ok((SqliteStore::open(&path)?, restored))
}

pub(crate) fn backup_database(source: &Path, destination: &Path) -> Result<()> {
    validate_database_export_destination(source, destination)?;
    publish_file(destination, |temporary| {
        let unavailable = |e: std::io::Error| StoreError::new(ErrorKind::Unavailable, e.to_string());
        let database = rusqlite::Connection::open_with_flags(
            std::fs::canonicalize(source).map_err(unavailable)?,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        database.backup(rusqlite::MAIN_DB, std::fs::canonicalize(temporary).map_err(unavailable)?, None)?;
        Ok(())
    })
}

pub(crate) fn publish_file(destination: &Path, prepare: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| StoreError::invalid("Choose a destination folder."))?;
    let temporary = parent.join(format!(".capy-workspace-backup-{}", new_id()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&temporary)
            .map_err(|e| StoreError::new(ErrorKind::FailedWrite, e.to_string()))?;
        prepare(&temporary)?;
        // Windows FlushFileBuffers requires a writable handle. SQLite has
        // closed its backup connection; reopen without truncating its result.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| StoreError::new(ErrorKind::FailedWrite, e.to_string()))?;
        std::fs::rename(&temporary, destination)
            .map_err(|e| StoreError::new(ErrorKind::FailedWrite, e.to_string()))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| StoreError::new(ErrorKind::FailedWrite, e.to_string()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

impl std::future::IntoFuture for StoreReply {
    type Output = Result<StoreResponse>;
    type IntoFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Self::Output>>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.0.recv().await.unwrap_or_else(|_| {
                Err(StoreError::new(
                    ErrorKind::Unavailable,
                    "Workspace storage worker stopped.",
                ))
            })
        })
    }
}
impl WorkspaceStore for StoreWorker {
    fn retire_owner(&self, owner: &Owner) {
        let (reply, _) = async_channel::bounded(1);
        let _ = self
            .0
            .sender
            .as_ref()
            .unwrap()
            .send((WorkerRequest::RetireOwner(owner.clone()), reply));
    }
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        self.request(request).await
    }
}
