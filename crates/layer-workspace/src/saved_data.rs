use super::*;
use layer_host::{NativeHost, StorageRoots};
use layer_ui::{ExportPresets, Localizer, Settings, UiLanguage};
use std::{collections::BTreeMap, path::Path, sync::mpsc, time::{Duration, Instant}};

#[derive(Debug, Serialize)]
pub struct SavedStoreReport {
    pub loaded: bool,
    pub items: usize,
    pub error: Option<String>,
}
impl SavedStoreReport {
    fn result(result: std::result::Result<usize, String>) -> Self {
        match result {
            Ok(items) => Self { loaded: true, items, error: None },
            Err(error) => Self { loaded: false, items: 0, error: Some(error) },
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SavedSessionReport {
    pub key: String,
    pub loaded: bool,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct SavedDataReport {
    pub stores: BTreeMap<String, SavedStoreReport>,
    pub startup: String,
    pub notices: Vec<String>,
    pub editing_dropped: bool,
    pub sessions: Vec<SavedSessionReport>,
}

fn read(path: &Path, limit: usize) -> std::result::Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            if bytes.len() > limit { return Err("Saved file exceeds its size limit".into()); }
            Ok(Some(bytes))
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn profiles(directory: &Path) -> std::result::Result<usize, String> {
    use layer_ui::profile_library::{self, ProfileRecord};
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.to_string()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry.file_type().map_err(|error| error.to_string())?.is_file()
            && entry.path().extension().is_some_and(|extension| extension == "icc") {
            records.push(ProfileRecord { id: entry.path().file_stem().unwrap().to_string_lossy().into_owned(),
                bytes: entry.metadata().map_err(|error| error.to_string())?.len(), issue: None });
        }
    }
    let records = profile_library::profile_inventory(records);
    for record in &records {
        let bytes = layer_core::package::session_store::read_bounded(
            &directory.join(format!("{}.icc", record.id)), profile_library::PROFILE_READ_LIMIT as u64)?;
        let entry = profile_library::inspect_library_entry(record, Ok(&bytes));
        if let Some(error) = entry.issue { return Err(format!("{}: {error:?}", record.id)); }
    }
    Ok(records.len())
}

fn advance(controller: &mut WorkspaceController<StoreWorker>, host: &mut NativeHost,
           wake: &mpsc::Receiver<()>, closed: bool) -> std::result::Result<(), String> {
    let start = Instant::now();
    loop {
        controller.tick(&mut host.session, crate::sqlite::SystemClock.now_ms());
        if if closed { controller.view.closed } else { controller.view.ready } { return Ok(()); }
        let remaining = Duration::from_secs(20).checked_sub(start.elapsed()).ok_or("Workspace startup did not finish")?;
        if controller.task.as_ref().is_some_and(|task| task.signal.ready.load(Ordering::Acquire))
            || controller.incoming_renew.as_ref().is_some_and(|task| task.signal.ready.load(Ordering::Acquire)) {
            continue;
        }
        wake.recv_timeout(remaining).map_err(|error| error.to_string())?;
    }
}

fn sessions(root: &Path) -> std::result::Result<Vec<SavedSessionReport>, String> {
    use layer_core::package::session_store::SessionStore;
    fn manifests(root: &Path, paths: &mut Vec<std::path::PathBuf>) -> std::result::Result<(), String> {
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_file() && path.extension().is_some_and(|extension| extension == "json") {
                paths.push(path);
            } else if kind.is_dir() {
                if entry.file_name().to_string_lossy().starts_with("preserved-") { continue; }
                if entry.file_name() == "packed" { manifests(&path, paths)?; }
                else {
                    for name in ["session.json", "window.json"] {
                        if path.join(name).is_file() { paths.push(path.join(name)); }
                    }
                }
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    manifests(root, &mut paths)?;
    let mut outcomes = Vec::new();
    for path in paths {
        let manifest = match layer_ui::session_recovery::SessionManifest::read(&path) {
            Ok(Some(manifest)) => manifest,
            other => {
                outcomes.push(SavedSessionReport { key: path.file_name().unwrap().to_string_lossy().into_owned(),
                    loaded: false, error: Some(format!("{other:?}")) });
                continue;
            }
        };
        for drawing in manifest.drawings {
            let result = pollster::block_on(async {
                let mut store = SessionStore::open(&path.parent().unwrap().join(&drawing.key)).await?;
                let opened = store.load(layer_core::ProjectLimits::for_import(), &AtomicBool::new(false)).await?
                    .ok_or("Saved drawing has no generation")?;
                layer_ui::session_recovery::SessionRestore::from_core(opened).map(|_| ())
            });
            outcomes.push(SavedSessionReport { key: drawing.key, loaded: result.is_ok(), error: result.err() });
        }
    }
    outcomes.sort_by(|a,b| a.key.cmp(&b.key));
    Ok(outcomes)
}

pub fn check_saved_data(roots: &StorageRoots) -> SavedDataReport {
    let mut report = SavedDataReport { stores: BTreeMap::new(), startup: "stored".into(),
        notices: Vec::new(), editing_dropped: false, sessions: Vec::new() };
    let settings = read(&roots.settings(), Settings::MAX_FILE_BYTES).and_then(|bytes| bytes.map(|bytes| {
        String::from_utf8(bytes).map(|saved| Settings::restore(&saved)).map_err(|error| error.to_string())
    }).transpose());
    report.stores.insert("settings".into(), SavedStoreReport::result(settings.as_ref()
        .map(|value| usize::from(value.is_some())).map_err(Clone::clone)));
    report.stores.insert("export_presets".into(), SavedStoreReport::result(read(&roots.export_presets(), ExportPresets::MAX_FILE_BYTES)
        .map(|bytes| bytes.map(|bytes| ExportPresets::restore(&bytes).names().count()).unwrap_or(0))));
    report.stores.insert("papers".into(), SavedStoreReport::result(layer_host::papers::list(&roots.papers())
        .map(|papers| papers.len())));
    report.stores.insert("color_profiles".into(), SavedStoreReport::result(profiles(&roots.color_profiles())));
    let workspace = (|| -> std::result::Result<usize, String> {
        let mut host = NativeHost::new(Platform::Gtk)?;
        if let Ok(Some(settings)) = settings {
            host.session.dispatch(UiAction::RestoreSettings { settings: Box::new(settings) })?;
        }
        let mut controller = WorkspaceController::new_localized(StoreWorker::shared(&roots.workspaces())
            .map_err(|error| error.to_string())?, Platform::Gtk, crate::sqlite::SystemClock.now_ms(), Localizer::shared(UiLanguage::English));
        let (sender, wake) = mpsc::channel();
        controller.set_wake(Arc::new(move || { let _ = sender.send(()); }));
        let adopted = advance(&mut controller, &mut host, &wake, false);
        report.editing_dropped = controller.manager.editing_dropped();
        if let Some(WorkspaceNotice { copy: WorkspaceNoticeCopy::Messages(ids), .. }) = &controller.notice {
            report.notices = ids.iter().map(|id| id.key().into()).collect();
        }
        report.startup = if controller.manager.in_memory() { "in memory" }
            else if controller.startup == Some(Startup::SetAside)
                || report.notices.iter().any(|id| id == layer_ui::MessageId::WORKSPACE_STORAGE_DAMAGED_NOTICE.key()) { "set aside" }
            else { "stored" }.into();
        adopted?;
        let items = controller.manager.items().len();
        controller.input(&mut host.session, WorkspaceInput::Close, crate::sqlite::SystemClock.now_ms()).map_err(|error| error.to_string())?;
        advance(&mut controller, &mut host, &wake, true)?;
        Ok(items)
    })();
    report.stores.insert("workspaces".into(), SavedStoreReport::result(workspace));
    let session_result = sessions(&roots.sessions());
    report.stores.insert("sessions".into(), SavedStoreReport::result(session_result.as_ref()
        .map(|sessions| sessions.iter().filter(|session| session.loaded).count()).map_err(Clone::clone)));
    if let Ok(sessions) = session_result { report.sessions = sessions; }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let target = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() { copy(&entry.path(), &target); }
            else { std::fs::copy(entry.path(), target).unwrap(); }
        }
    }

    fn released() -> (std::path::PathBuf, StorageRoots) {
        let directory = crate::test_support::temp_dir("saved-data");
        let roots = StorageRoots::within(&directory);
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let workspace = crates.join("layer-workspace/tests/compat/1.0.15");
        let ui = crates.join("layer-ui/tests/compat/1.0.15");
        for path in [&roots.config, &roots.workspaces(), &roots.color_profiles(), &roots.sessions()] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::copy(workspace.join("settings.json"), roots.settings()).unwrap();
        std::fs::copy(workspace.join("export-presets"), roots.export_presets()).unwrap();
        std::fs::copy(workspace.join("workspaces.sqlite3"), roots.workspaces().join("workspaces.sqlite3")).unwrap();
        copy(&crates.join("layer-host/tests/compat/1.0.15/papers"), &roots.papers());
        for entry in std::fs::read_dir(&ui).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "icc") {
                std::fs::copy(&path, roots.color_profiles().join(path.file_name().unwrap())).unwrap();
            }
        }
        for (manifest, key) in [("session.json", "drawing-a"), ("session-icc.json", "drawing-icc")] {
            std::fs::copy(ui.join(manifest), roots.sessions().join(manifest)).unwrap();
            copy(&ui.join(key), &roots.sessions().join(key));
        }
        (directory, roots)
    }

    #[test]
    fn check_saved_data_loads_the_assembled_release_without_resetting() {
        let (directory, roots) = released();
        let report = check_saved_data(&roots);
        assert_eq!(report.startup, "stored", "{report:?}");
        assert!(report.notices.is_empty(), "{report:?}");
        assert!(!report.editing_dropped, "{report:?}");
        assert!(report.stores.values().all(|store| store.loaded), "{report:?}");
        assert_eq!(report.sessions.len(), 2);
        assert!(report.sessions.iter().all(|session| session.loaded), "{report:?}");
        assert_eq!(report.stores["papers"].items, 1);
        assert_eq!(report.stores["color_profiles"].items, 1);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn check_saved_data_reports_a_damaged_store_and_an_unopenable_drawing() {
        let (directory, roots) = released();
        std::fs::write(roots.workspaces().join("workspaces.sqlite3"), b"damaged database").unwrap();
        std::fs::remove_file(roots.sessions().join("drawing-a/head.json")).unwrap();
        let report = check_saved_data(&roots);
        assert_eq!(report.startup, "set aside", "{report:?}");
        assert!(report.notices.iter().any(|id| id == layer_ui::MessageId::WORKSPACE_STORAGE_DAMAGED_NOTICE.key()));
        assert!(report.sessions.iter().any(|session| !session.loaded), "{report:?}");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn check_saved_data_loads_flat_and_window_manifests_in_packed_storage() {
        let (directory, roots) = released();
        let packed = roots.sessions().join("packed");
        std::fs::create_dir_all(&packed).unwrap();
        for name in ["session.json", "drawing-a"] {
            std::fs::rename(roots.sessions().join(name), packed.join(name)).unwrap();
        }
        let window = packed.join("window");
        std::fs::create_dir_all(&window).unwrap();
        std::fs::rename(roots.sessions().join("session-icc.json"), window.join("window.json")).unwrap();
        std::fs::rename(roots.sessions().join("drawing-icc"), window.join("drawing-icc")).unwrap();
        let preserved = packed.join("preserved-closed");
        copy(&window, &preserved);
        let original = std::fs::read(preserved.join("drawing-icc/head.json")).unwrap();
        let report = check_saved_data(&roots);
        assert_eq!(report.sessions.len(), 2);
        assert!(report.sessions.iter().all(|session| session.loaded), "{report:?}");
        assert_eq!(std::fs::read(preserved.join("drawing-icc/head.json")).unwrap(), original);
        std::fs::rename(window.join("window.json"), window.join("session.json")).unwrap();
        assert!(check_saved_data(&roots).sessions.iter().all(|session| session.loaded));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
