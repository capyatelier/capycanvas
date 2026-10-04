use layer_host::NativeHost;
use layer_ui::{ColorAction, ColorLibrary, ColorLibraryAction, PaletteFormat, UiAction};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool, mpsc},
};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Import { path: String },
    Export { id: u64, format: PaletteFormat, path: String },
}

enum Outcome {
    Imported(ColorLibraryAction),
    Exported(Option<String>),
}

#[derive(Clone, Default, PartialEq, Serialize)]
struct Status {
    generation: u64,
    busy: bool,
    notice: Option<String>,
    error: Option<String>,
}

pub(crate) struct Service {
    wake: Arc<dyn Fn() + Send + Sync>,
    pending: Option<mpsc::Receiver<Result<Outcome, String>>>,
    status: Status,
}

fn read_limited(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(ColorLibrary::MAX_IMPORT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

impl Service {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { wake, pending: None, status: Status::default() }
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        serde_json::json!({
            "generation": self.status.generation,
            "busy": self.status.busy,
            "notice": self.status.notice,
            "error": self.status.error,
            "extensions": PaletteFormat::IMPORT_EXTENSIONS,
            "formats": PaletteFormat::ALL.map(|format| serde_json::json!({"format": format, "extension": format.extension()})),
        })
    }

    pub(crate) fn dispatch(&mut self, host: &mut NativeHost, action: Action) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("Wait for the current palette file to finish".into());
        }
        let job: Box<dyn FnOnce() -> Result<Outcome, String> + Send> = match action {
            Action::Import { path } => {
                let path = PathBuf::from(path);
                Box::new(move || {
                    let stem = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("Imported palette")
                        .to_owned();
                    ColorLibrary::import_file(&read_limited(&path)?, &stem).map(Outcome::Imported)
                })
            }
            Action::Export { id, format, path } => {
                let palette = host
                    .session
                    .state()
                    .color_library
                    .palettes
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or("Palette no longer exists")?;
                let path = PathBuf::from(path);
                Box::new(move || {
                    let export = palette.export(format)?;
                    crate::document_io::atomic_write(&path, &AtomicBool::new(false), |file| {
                        file.write_all(&export.bytes).map_err(|e| e.to_string())
                    })?;
                    Ok(Outcome::Exported(export.notice))
                })
            }
        };
        let (send, receive) = mpsc::channel();
        let wake = self.wake.clone();
        std::thread::Builder::new()
            .name("capy-palette-file".into())
            .spawn(move || {
                let _ = send.send(job());
                wake();
            })
            .map_err(|e| e.to_string())?;
        self.pending = Some(receive);
        self.status.busy = true;
        self.status.notice = None;
        self.status.error = None;
        host.invalidate_snapshot();
        Ok(())
    }

    pub(crate) fn poll(&mut self, host: &mut NativeHost) {
        let Some(receive) = &self.pending else { return };
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Palette file worker stopped".into()),
        };
        self.pending = None;
        self.status.busy = false;
        self.status.generation += 1;
        match result {
            Ok(Outcome::Imported(action)) => {
                if let Err(error) = host.dispatch(UiAction::Color { action: ColorAction::Library { action } }) {
                    self.status.error = Some(error);
                }
            }
            Ok(Outcome::Exported(notice)) => self.status.notice = notice,
            Err(error) => self.status.error = Some(error),
        }
        host.invalidate_snapshot();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn settle(service: &mut Service, host: &mut NativeHost) {
        for _ in 0..500 {
            service.poll(host);
            if service.pending.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("palette file did not finish");
    }

    #[test]
    fn exported_palettes_import_through_the_worker_and_failures_leave_the_library() {
        let directory = TempDir::new();
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut service = Service::new(Arc::new(|| {}));
        let id = host.session.state().color_library.palettes[0].id;
        let count = host.session.state().color_library.palettes.len();
        for (exported, format) in PaletteFormat::ALL.into_iter().enumerate() {
            let path = directory.path.join(format!("round trip.{}", format.extension()));
            service.dispatch(&mut host, Action::Export { id, format, path: path.to_string_lossy().into() }).unwrap();
            assert!(service.dispatch(&mut host, Action::Import { path: String::new() }).is_err());
            settle(&mut service, &mut host);
            assert!(service.status.error.is_none(), "{format:?}: {:?}", service.status.error);
            assert!(path.exists());
            assert_eq!(std::fs::read_dir(&directory.path).unwrap().count(), exported + 1);
            service.dispatch(&mut host, Action::Import { path: path.to_string_lossy().into() }).unwrap();
            settle(&mut service, &mut host);
            assert!(service.status.error.is_none(), "{format:?}: {:?}", service.status.error);
        }
        assert_eq!(host.session.state().color_library.palettes.len(), count + PaletteFormat::ALL.len());
        let broken = directory.path.join("broken.aco");
        std::fs::write(&broken, b"not a palette").unwrap();
        service.dispatch(&mut host, Action::Import { path: broken.to_string_lossy().into() }).unwrap();
        settle(&mut service, &mut host);
        assert!(service.status.error.is_some());
        assert_eq!(host.session.state().color_library.palettes.len(), count + PaletteFormat::ALL.len());
        assert_eq!(service.status().get("generation").and_then(|v| v.as_u64()), Some(11));
    }
}
