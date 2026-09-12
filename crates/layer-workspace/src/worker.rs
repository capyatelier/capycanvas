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
}
type Message = (WorkerRequest, async_channel::Sender<Result<StoreResponse>>);
pub struct StoreReply(async_channel::Receiver<Result<StoreResponse>>);
impl StoreReply {
    /// Hosts poll from their event loop; no disk or SQLite lock is acquired here.
    pub fn poll(&self) -> Option<Result<StoreResponse>> {
        match self.0.try_recv() {
            Ok(value) => Some(value),
            Err(async_channel::TryRecvError::Empty) => None,
            Err(async_channel::TryRecvError::Closed) => Some(Err(StoreError::new(
                ErrorKind::Unavailable,
                "Workspace storage worker stopped.",
            ))),
        }
    }
    pub fn wait(self) -> Result<StoreResponse> {
        self.0.recv_blocking().unwrap_or_else(|_| {
            Err(StoreError::new(
                ErrorKind::Unavailable,
                "Workspace storage worker stopped.",
            ))
        })
    }
}
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
    /// Other processes coordinate through SQLite transactions and owner fences.
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
                        WorkerRequest::Backup(destination) => {
                            let _ = reply.try_send(
                                backup_database(&database, &destination)
                                    .map(|()| StoreResponse::Done),
                            );
                            continue;
                        }
                        WorkerRequest::Store(request) => request,
                    };
                    if matches!(request, StoreRequest::Reopen) && store.is_err() {
                        store = SqliteStore::open(&database);
                    }
                    let result = match &mut store {
                        Ok(store) => store.handle(request),
                        Err(error) => Err(error.clone()),
                    };
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
    for protected in [
        source.to_path_buf(),
        PathBuf::from(format!("{}-wal", source.display())),
        PathBuf::from(format!("{}-shm", source.display())),
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

fn backup_database(source: &Path, destination: &Path) -> Result<()> {
    validate_database_export_destination(source, destination)?;
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
        let database = rusqlite::Connection::open_with_flags(
            source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        database.backup(rusqlite::MAIN_DB, &temporary, None)?;
        drop(database);
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
    async fn execute(&self, request: StoreRequest) -> Result<StoreResponse> {
        self.request(request).await
    }
}
