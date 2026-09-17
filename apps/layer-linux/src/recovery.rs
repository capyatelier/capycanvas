//! One immutable autosave in flight per window; publication never clears dirty.
use crate::{files::atomic_write, workspace::Workspace};
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{Project, ProjectLimits};
use layer_ui::recovery::{RecoveryState, RecoveryEvent, RecoveryWork, RecoveryWorkKind};
use std::{
    cell::{Cell, RefCell},
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(crate) struct Recovery {
    pub recovered: Cell<bool>,
    pub origin: RefCell<Option<PathBuf>>,
    path: PathBuf,
    origin_lock: RefCell<Option<Arc<std::fs::File>>>,
    policy: RefCell<RecoveryState>,
    discarded: Arc<AtomicBool>,
}
fn directory() -> PathBuf {
    std::env::var_os("CAPY_RECOVERY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| glib::home_dir().join(".local/state"))
                .join("capycanvas/recovery")
        })
}
impl Default for Recovery {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            recovered: Cell::new(false),
            origin: RefCell::new(None),
            path: directory().join(format!(
                "{}-{stamp}-{}.capy",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )),
            origin_lock: RefCell::new(None),
            policy: RefCell::new({
                let mut policy = RecoveryState::default();
                policy.event(RecoveryEvent::Ownership { owned: true }).unwrap();
                policy
            }),
            discarded: Arc::new(AtomicBool::new(false)),
        }
    }
}
// File locks are the GTK storage adapter's ownership observation. A restored
// drawing explicitly takes over the dialog's lease before that dialog releases it.
fn claim_origin(path: &std::path::Path, transfer: bool) -> Result<Option<Arc<std::fs::File>>, String> {
    static CLAIMS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, std::sync::Weak<std::fs::File>>>> = std::sync::OnceLock::new();
    let mut claims = CLAIMS.get_or_init(Default::default).lock().map_err(|e| e.to_string())?;
    claims.retain(|_, lease| lease.strong_count() > 0);
    if let Some(lease) = claims.get(path).and_then(std::sync::Weak::upgrade) {
        return Ok(transfer.then_some(lease));
    }
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(path.with_extension("capy.lock")).map_err(|e| e.to_string())?;
    if lock.try_lock().is_err() { return Ok(None); }
    let lock = Arc::new(lock);
    claims.insert(path.to_path_buf(), Arc::downgrade(&lock));
    Ok(Some(lock))
}

impl Recovery {
    pub fn set_origin(&self, path: Option<PathBuf>) -> Result<(), String> {
        if let Some(path) = &path {
            *self.origin_lock.borrow_mut() = Some(claim_origin(path, true)?.ok_or("Recovery copy is owned by another window")?);
        }
        self.origin.replace(path);
        Ok(())
    }
    fn adopt_origin(&self) -> Result<(), String> {
        if let Some(path) = self.origin.take() {
            self.policy.borrow_mut().event(RecoveryEvent::Adopted { key: path.to_string_lossy().into_owned() })?;
        }
        Ok(())
    }
    pub fn discard(self: &Rc<Self>) {
        self.discarded.store(true, Ordering::Release);
        let _ = self.adopt_origin();
        let work = self.policy.borrow_mut().event(RecoveryEvent::Retire { discard_origin: true }).unwrap().work;
        self.policy.borrow_mut().event(RecoveryEvent::Close).unwrap();
        self.execute(None, work);
    }
    pub fn capture(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.discarded.load(Ordering::Acquire) { return; }
        if let Err(error) = self.adopt_origin() { eprintln!("Recovery origin unavailable: {error}"); return; }
        let document = {
            let gpu = w.gpu.borrow();
            let Some(gpu) = gpu.as_ref() else { return; };
            gpu.session.recovery_document()
        };
        let work = self.policy.borrow_mut().event(RecoveryEvent::Observe { document, owned: true }).unwrap().work;
        self.execute(Some(w.clone()), work);
    }
    fn execute(self: &Rc<Self>, workspace: Option<Rc<Workspace>>, first: Option<RecoveryWork>) {
        if first.is_none() { return; }
        let recovery = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let mut next = first;
            while let Some(work) = next {
                let result = match work.kind {
                    RecoveryWorkKind::Capture => {
                        let project = workspace.as_ref().ok_or_else(|| "Canvas closed".to_string()).and_then(|w| {
                            let gpu = w.gpu.borrow();
                            gpu.as_ref().ok_or("Canvas unavailable").map_err(String::from)?.session.capture_project_recovery()
                        });
                        match project {
                            Ok(project) => {
                                let path = recovery.path.clone();
                                let discarded = recovery.discarded.clone();
                                gio::spawn_blocking(move || publish(&path, project, &discarded)).await.map_err(|e| format!("Recovery writer failed: {e:?}")).and_then(|r| r)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    RecoveryWorkKind::Retire | RecoveryWorkKind::RetireOrigin { .. } => {
                        let path = match work.kind { RecoveryWorkKind::RetireOrigin { key } => PathBuf::from(key), _ => recovery.path.clone() };
                        gio::spawn_blocking(move || match std::fs::remove_file(path) {
                            Ok(()) => Ok(()), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()), Err(e) => Err(e.to_string()),
                        }).await.map_err(|e| format!("Recovery cleanup failed: {e:?}")).and_then(|r| r)
                    }
                    RecoveryWorkKind::Restore { .. } => Err("GTK restores into a separate drawing window".into()),
                };
                if let Err(error) = &result { eprintln!("Recovery operation failed; previous copy retained: {error}"); }
                let update = recovery.policy.borrow_mut().event(RecoveryEvent::Complete { token: work.token, success: result.is_ok() }).unwrap();
                if !update.release.is_empty() { recovery.origin_lock.borrow_mut().take(); }
                next = update.work;
            }
        });
    }
}


fn publish(path: &std::path::Path, project: Project, discarded: &AtomicBool) -> Result<(), String> {
    if discarded.load(Ordering::Acquire) {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let result = atomic_write(path, |out| project.pruned()?.write(out));
    // Close can finish while the worker awaits tile backing or writes a file.
    if discarded.load(Ordering::Acquire) {
        let _ = std::fs::remove_file(path);
    }
    result
}

pub(crate) fn install(w: &Rc<Workspace>) {
    if cfg!(test) && std::env::var_os("CAPY_RECOVERY_DIR").is_none() {
        return;
    }
    let weak = Rc::downgrade(w);
    glib::timeout_add_local(Duration::from_secs(15), move || {
        let Some(w) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        w.recovery.capture(&w);
        glib::ControlFlow::Continue
    });
}

pub(crate) fn offer_stale(w: &Rc<Workspace>) {
    let weak = Rc::downgrade(w);
    glib::MainContext::default().spawn_local(async move {
        let candidates = gio::spawn_blocking(|| {
            let mut paths = Vec::new();
            if let Ok(entries) = std::fs::read_dir(directory()) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "capy")
                        && let Some(pid) = path.file_name().and_then(|v| v.to_str()).and_then(|v| v.split('-').next()).and_then(|v| v.parse::<u32>().ok())
                        && !PathBuf::from(format!("/proc/{pid}")).exists() { paths.push(path); }
                }
            }
            paths.sort();
            paths
        }).await.unwrap_or_default();
        for path in candidates {
            let Some(w) = weak.upgrade() else { break; };
            let claim_path = path.clone();
            let Ok(Ok(Some(_lease))) = gio::spawn_blocking(move || claim_origin(&claim_path, false)).await else { continue; };
            let mut policy = RecoveryState::default();
            policy.event(RecoveryEvent::Ownership { owned: true }).unwrap();
            policy.event(RecoveryEvent::Offer { key: path.to_string_lossy().into_owned(), owned: true }).unwrap();
            let dialog = adw::AlertDialog::builder().heading("Recover an unsaved drawing?")
                .body("A recovery copy contains completed edits from a previous session. Samples that had not reached a checkpoint may be missing.").build();
            dialog.add_responses(&[("later", "Later"), ("discard", "Discard Copy"), ("recover", "Recover")]);
            dialog.set_close_response("later");
            dialog.set_default_response(Some("recover"));
            match crate::alert::choose(dialog, &w.window).await.as_str() {
                "recover" => {
                    let restore = policy.event(RecoveryEvent::Restore).unwrap().work.unwrap();
                    let source = path.clone();
                    let result = gio::spawn_blocking(move || {
                        let file = std::fs::File::open(source).map_err(|e| e.to_string())?;
                        Project::read(BufReader::new(file), ProjectLimits::default())
                    }).await;
                    match result {
                        Ok(Ok(project)) => {
                            if let Some(open) = w.open_document.borrow().as_ref() { open(project, None, Some(path.clone())); }
                            // Ownership of durable replacement moves to that window.
                            policy.event(RecoveryEvent::Close).unwrap();
                            policy.event(RecoveryEvent::Complete { token: restore.token, success: true }).unwrap();
                            // Keep the original until the recovered document is
                            // explicitly saved/discarded; a failed GPU startup
                            // must not destroy its only durable checkpoint.
                        }
                        _ => { let error = adw::AlertDialog::builder().heading("Cannot read recovery copy").body("The copy was kept on disk. Other open drawings are unchanged.").build(); error.add_response("ok", "OK"); error.present(Some(&w.window)); }
                    }
                }
                "discard" => {
                    if let Some(work) = policy.event(RecoveryEvent::Dismiss { discard: true }).unwrap().work {
                        if let RecoveryWorkKind::RetireOrigin { key } = work.kind {
                            let success = gio::spawn_blocking(move || std::fs::remove_file(key)).await.is_ok_and(|r| r.is_ok());
                            policy.event(RecoveryEvent::Complete { token: work.token, success }).unwrap();
                        }
                    }
                }
                _ => { policy.event(RecoveryEvent::Dismiss { discard: false }).unwrap(); },
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{
        Document,
        raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey},
    };

    #[test]
    fn failed_backing_preserves_the_previous_durable_recovery_copy() {
        let path =
            std::env::temp_dir().join(format!("capy-recovery-failure-{}.capy", std::process::id()));
        let mut project = Project {
            document: Document::new("recovery", 256, 256),
            assets: Default::default(),
        };
        let discarded = AtomicBool::new(false);
        publish(&path, project.clone(), &discarded).unwrap();
        let previous = std::fs::read(&path).unwrap();
        let tile = RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        tile.publish(Err("Device lost before host capture".into()))
            .unwrap();
        project.document.layers[0].raster = RasterRevision::backed(RasterData {
            tiles: [(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                tile,
            )]
            .into(),
            ..Default::default()
        });
        assert!(
            publish(&path, project, &discarded)
                .unwrap_err()
                .contains("Device lost")
        );
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        Project::read(
            std::fs::File::open(&path).unwrap(),
            ProjectLimits::default(),
        )
        .unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn close_discards_an_inflight_recovery_after_tile_publication() {
        let dir = std::env::temp_dir().join(format!("capy-recovery-close-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drawing.capy");
        let tile = RasterTile::pending(RasterPlane::Color.descriptor(Default::default()));
        let mut project = Project {
            document: Document::new("recovery", 256, 256),
            assets: Default::default(),
        };
        project.document.layers[0].raster = RasterRevision::backed(RasterData {
            tiles: [(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                tile.clone(),
            )]
            .into(),
            ..Default::default()
        });
        let discarded = Arc::new(AtomicBool::new(false));
        let cancelled = discarded.clone();
        let output = path.clone();
        let worker = std::thread::spawn(move || publish(&output, project, &cancelled));
        // Atomic writer has entered the file operation and is awaiting this tile.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::fs::read_dir(&dir).unwrap().next().is_none() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        discarded.store(true, Ordering::Release);
        tile.publish(TileBlob::encode(
            RasterPlane::Color.descriptor(Default::default()),
            &vec![0; 256 * 256 * 4],
        ))
        .unwrap();
        worker.join().unwrap().unwrap();
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir(dir).unwrap();
    }
}
