//! One immutable autosave in flight per window; publication never clears dirty.
use crate::{files::atomic_write, workspace::Workspace};
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{Project, ProjectLimits};
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
    pending: Cell<bool>,
    checkpoint: Cell<Option<(u64, u64)>>,
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
            pending: Cell::new(false),
            checkpoint: Cell::new(None),
            discarded: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Recovery {
    pub fn discard(&self) {
        self.discarded.store(true, Ordering::Release);
        let paths = std::iter::once(self.path.clone())
            .chain(self.origin.take())
            .collect();
        gio::spawn_blocking(move || remove_copies(paths));
    }
    fn clean(self: &Rc<Self>) {
        let paths = std::iter::once(self.path.clone())
            .chain(self.origin.take())
            .collect();
        self.pending.set(true);
        let recovery = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let _ = gio::spawn_blocking(move || remove_copies(paths)).await;
            recovery.checkpoint.set(None);
            recovery.pending.set(false);
        });
    }
    pub fn capture(self: &Rc<Self>, w: &Rc<Workspace>) {
        if self.pending.get() || self.discarded.load(Ordering::Acquire) {
            return;
        }
        let snapshot = {
            let gpu = w.gpu.borrow();
            let Some(gpu) = gpu.as_ref() else {
                return;
            };
            let state = &gpu.session.state().document_file;
            if !state.modified {
                self.clean();
                return;
            }
            let checkpoint = (state.epoch, gpu.session.engine().checkpoint());
            if self.checkpoint.get() == Some(checkpoint) {
                return;
            }
            let Ok(project) = gpu.session.capture_project_recovery() else {
                return;
            };
            (checkpoint, project)
        };
        self.pending.set(true);
        let recovery = self.clone();
        let path = self.path.clone();
        let discarded = self.discarded.clone();
        glib::MainContext::default().spawn_local(async move {
            let (checkpoint, project) = snapshot;
            let result = gio::spawn_blocking(move || publish(&path, project, &discarded)).await;
            recovery.pending.set(false);
            match result {
                Ok(Ok(())) => recovery.checkpoint.set(Some(checkpoint)),
                Ok(Err(error)) => {
                    eprintln!("Recovery checkpoint failed; previous copy retained: {error}")
                }
                Err(error) => {
                    eprintln!("Recovery worker failed; previous copy retained: {error:?}")
                }
            }
        });
    }
}

fn remove_copies(paths: Vec<PathBuf>) {
    for path in paths {
        let _ = std::fs::remove_file(path);
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
            let dialog = adw::AlertDialog::builder().heading("Recover an unsaved drawing?")
                .body("A recovery copy contains completed edits from a previous session. Samples that had not reached a checkpoint may be missing.").build();
            dialog.add_responses(&[("later", "Later"), ("discard", "Discard Copy"), ("recover", "Recover")]);
            dialog.set_close_response("later");
            dialog.set_default_response(Some("recover"));
            match crate::alert::choose(dialog, &w.window).await.as_str() {
                "recover" => {
                    let source = path.clone();
                    let result = gio::spawn_blocking(move || {
                        let file = std::fs::File::open(source).map_err(|e| e.to_string())?;
                        Project::read(BufReader::new(file), ProjectLimits::default())
                    }).await;
                    match result {
                        Ok(Ok(project)) => {
                            if let Some(open) = w.open_document.borrow().as_ref() { open(project, None, Some(path.clone())); }
                            // Keep the original until the recovered document is
                            // explicitly saved/discarded; a failed GPU startup
                            // must not destroy its only durable checkpoint.
                        }
                        _ => { let error = adw::AlertDialog::builder().heading("Cannot read recovery copy").body("The copy was kept on disk. Other open drawings are unchanged.").build(); error.add_response("ok", "OK"); error.present(Some(&w.window)); }
                    }
                }
                "discard" => { let _ = gio::spawn_blocking(move || std::fs::remove_file(path)).await; }
                _ => (),
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
