use crate::*;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, mpsc},
};

type Result<T> = std::result::Result<T, StoreError>;
type Message = (StoreRequest, mpsc::Sender<Result<StoreResponse>>);
pub struct StoreReply(mpsc::Receiver<Result<StoreResponse>>);
impl StoreReply {
    /// Hosts poll from their event loop; no disk or SQLite lock is acquired here.
    pub fn poll(&self) -> Option<Result<StoreResponse>> {
        match self.0.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(StoreError::new(
                ErrorKind::Unavailable,
                "Workspace storage worker stopped.",
            ))),
        }
    }
    pub fn wait(self) -> Result<StoreResponse> {
        self.0.recv().unwrap_or_else(|_| {
            Err(StoreError::new(
                ErrorKind::Unavailable,
                "Workspace storage worker stopped.",
            ))
        })
    }
}
struct Worker {
    sender: mpsc::Sender<Message>,
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
        std::thread::Builder::new()
            .name("workspace-storage".into())
            .spawn(move || {
                let mut store = SqliteStore::open(&database);
                while let Ok((request, reply)) = receiver.recv() {
                    if matches!(request, StoreRequest::Reopen) && store.is_err() {
                        store = SqliteStore::open(&database);
                    }
                    let result = match &mut store {
                        Ok(store) => store.handle(request),
                        Err(error) => Err(error.clone()),
                    };
                    // A dropped receiver leaves the persisted receipt/pending payload
                    // available for retry. It must never imply that a write failed.
                    let _ = reply.send(result);
                }
            })
            .map_err(|e| StoreError::new(ErrorKind::Unavailable, e.to_string()))?;
        let worker = Arc::new(Worker { sender });
        workers.insert(path, Arc::downgrade(&worker));
        Ok(Self(worker))
    }
    pub fn request(&self, request: StoreRequest) -> StoreReply {
        let (sender, receiver) = mpsc::channel();
        let _ = self.0.sender.send((request, sender));
        StoreReply(receiver)
    }
}
