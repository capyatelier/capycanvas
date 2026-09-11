use super::*;
use std::{sync::mpsc, time::Duration};

struct Directory {
    base: PathBuf,
    path: PathBuf,
}
impl Directory {
    fn new() -> Self {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../artifacts/windows/settings-tests");
        fs::create_dir_all(&base).unwrap();
        let base = fs::canonicalize(base).unwrap();
        loop {
            let path = base.join(format!(
                "{}.{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Self { base, path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }
    fn file(&self) -> SettingsFile {
        SettingsFile::new(self.path.clone()).unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        // Resolve and verify the owned test directory before recursive cleanup.
        if let Ok(path) = fs::canonicalize(&self.path) {
            if path.parent() == Some(self.base.as_path()) && path == self.path {
                let _ = fs::remove_dir_all(path);
            }
        }
    }
}
fn edited(gamma: f32) -> Settings {
    Settings {
        pressure_gamma: gamma,
        ..Default::default()
    }
}

#[test]
fn settings_round_trip_uses_shared_validation_and_migration() {
    let directory = Directory::new();
    let mut file = directory.file();
    assert!(file.load().unwrap().is_none());
    let first = edited(1.25);
    file.write(&encode(&first).unwrap()).unwrap();
    assert_eq!(file.load().unwrap(), Some(first));
    let second = edited(1.75);
    file.write(&encode(&second).unwrap()).unwrap();
    assert_eq!(file.load().unwrap(), Some(second));
    fs::write(
        directory.path.join("settings.json"),
        br#"{"pressure_gamma":1.5,"panel_text_pt":11}"#,
    )
    .unwrap();
    assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.5);
    assert_eq!(fs::read_dir(&directory.path).unwrap().count(), 1);
    assert!(SettingsFile::new(PathBuf::from("relative")).is_err());
}

#[test]
fn invalid_file_is_preserved_when_valid_preferences_are_saved() {
    for invalid in [
        br#"{"version":999,"custom_data":"retain this"}"#.as_slice(),
        b"{truncated",
    ] {
        let directory = Directory::new();
        let path = directory.path.join("settings.json");
        fs::write(&path, invalid).unwrap();
        let mut file = directory.file();
        assert!(file.load().is_err());
        assert_eq!(fs::read(&path).unwrap(), invalid);
        file.write(&encode(&edited(1.5)).unwrap()).unwrap();
        assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.5);
        let recovered: Vec<_> = fs::read_dir(&directory.path)
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("settings.recovery.")
            })
            .collect();
        assert_eq!(recovered.len(), 1);
        assert_eq!(fs::read(recovered[0].path()).unwrap(), invalid);
    }
}

#[test]
fn oversized_saved_preferences_are_bounded_and_not_rewritten_on_load() {
    let directory = Directory::new();
    let path = directory.path.join("settings.json");
    fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    assert!(directory.file().load().unwrap_err().contains("size limit"));
    assert_eq!(fs::metadata(path).unwrap().len(), (MAX_BYTES + 1) as u64);
}

#[cfg(target_os = "windows")]
#[test]
fn failed_replace_preserves_last_good_file_and_retry_succeeds() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = Directory::new();
    let mut file = directory.file();
    file.write(&encode(&edited(1.25)).unwrap()).unwrap();
    // Deny FILE_SHARE_DELETE, reproducing a real Windows replacement failure.
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(directory.path.join("settings.json"))
        .unwrap();
    assert!(file.write(&encode(&edited(1.75)).unwrap()).is_err());
    assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.25);
    assert_eq!(
        fs::read_dir(&directory.path).unwrap().count(),
        1,
        "failed temp file is removed"
    );
    drop(locked);
    file.write(&encode(&edited(1.75)).unwrap()).unwrap();
    assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.75);
}

#[test]
fn blocked_storage_retains_only_latest_pending_value_and_flushes_on_finish() {
    let (entered, wait) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let written = Arc::new(Mutex::new(Vec::new()));
    let records = written.clone();
    let wakes = Arc::new(AtomicU64::new(0));
    let counter = wakes.clone();
    let mut worker = Worker::start(
        move |bytes| {
            let value = u32::from_le_bytes(bytes.try_into().unwrap());
            if value == 1 {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            records.lock().unwrap().push(value);
            Ok(())
        },
        move || {
            counter.fetch_add(1, Ordering::Relaxed);
        },
    )
    .unwrap();
    worker.submit(1, 1u32.to_le_bytes().to_vec()).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    for id in 2..=1000 {
        worker.submit(id, id.to_le_bytes().to_vec()).unwrap();
    }
    assert_eq!(
        worker
            .shared
            .mailbox
            .lock()
            .unwrap()
            .pending
            .as_ref()
            .unwrap()
            .id,
        1000
    );
    assert!(worker.completion().is_none());
    release.send(()).unwrap();
    worker.finish().unwrap();
    assert_eq!(*written.lock().unwrap(), [1, 1000]);
    let complete = worker.completion().unwrap();
    assert_eq!(complete.id, 1000);
    assert!(complete.error.is_none());
    assert_eq!(wakes.load(Ordering::Relaxed), 2);
    assert!(worker.submit(1001, vec![]).is_err());
}

#[test]
fn shared_requests_stay_bounded_and_latest_save_is_acknowledged_after_flush() {
    let directory = Directory::new();
    let mut file = directory.file();
    let (entered, wait) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let mut first = true;
    let worker = Worker::start(
        move |bytes| {
            if first {
                first = false;
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            file.write(bytes)
        },
        || {},
    )
    .unwrap();
    let mut service = SettingsService {
        worker: Ok(worker),
        submitted: None,
        load_error: None,
    };
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    host.dispatch(UiAction::Invoke {
        command: layer_ui::CommandId::NewWindow,
    })
    .unwrap();
    host.dispatch(UiAction::OpenSettings {
        page: layer_ui::SettingsPage::Appearance,
    })
    .unwrap();
    host.dispatch(UiAction::EditSettings {
        settings: edited(1.1),
    })
    .unwrap();
    service.poll(&mut host).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    for i in 2..=100 {
        host.dispatch(UiAction::EditSettings {
            settings: edited(1. + i as f32 / 100.),
        })
        .unwrap();
        service.poll(&mut host).unwrap();
        assert_eq!(
            host.session.state().requests.len(),
            2,
            "one save plus the unrelated NewWindow request"
        );
    }
    release.send(()).unwrap();
    service.finish(&mut host).unwrap();
    assert_eq!(host.session.state().requests.len(), 1);
    assert!(matches!(
        host.session.state().requests[0].kind,
        HostRequestKind::NewWindow
    ));
    assert_eq!(directory.file().load().unwrap().unwrap().pressure_gamma, 2.);
    assert!(host.session.state().host_error.is_none());
}

#[test]
fn restoring_settings_does_not_echo_a_save_request() {
    let directory = Directory::new();
    directory
        .file()
        .write(&encode(&edited(1.5)).unwrap())
        .unwrap();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(directory.file()), || {});
    assert_eq!(host.session.state().settings.pressure_gamma, 1.5);
    assert!(host.session.state().requests.is_empty());
    service.finish(&mut host).unwrap();
}

#[test]
fn failed_save_is_reported_and_a_later_success_clears_the_error() {
    let (wake, completed) = mpsc::channel();
    let mut first = true;
    let worker = Worker::start(
        move |_| {
            if std::mem::take(&mut first) {
                Err("Storage temporarily unavailable.".into())
            } else {
                Ok(())
            }
        },
        move || {
            let _ = wake.send(());
        },
    )
    .unwrap();
    let mut service = SettingsService {
        worker: Ok(worker),
        submitted: None,
        load_error: None,
    };
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    host.dispatch(UiAction::OpenSettings {
        page: layer_ui::SettingsPage::Appearance,
    })
    .unwrap();
    for (gamma, failed) in [(1.25, true), (1.75, false)] {
        host.dispatch(UiAction::EditSettings {
            settings: edited(gamma),
        })
        .unwrap();
        service.poll(&mut host).unwrap();
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
        service.poll(&mut host).unwrap();
        assert_eq!(host.session.state().host_error.is_some(), failed);
        assert!(host.session.state().requests.is_empty());
    }
    service.finish(&mut host).unwrap();
}
