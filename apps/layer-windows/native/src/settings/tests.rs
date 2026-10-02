use super::*;
use crate::test_support::TempDir;
use std::{sync::mpsc, time::Duration};

impl SettingsFile {
    fn load(&self) -> Result<Option<Settings>, String> {
        self.load_saved().map(|saved| saved.map(|saved| Settings::restore_localized(&saved, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))))
    }
}
fn storage(directory: &TempDir) -> SettingsFile {
    SettingsFile::new(directory.path.clone()).unwrap()
}
fn service_over(worker: Worker) -> SettingsService {
    SettingsService {
        subscription: None,
        worker: Ok(worker),
        submitted: None,
        load_error: None,
        save_error: None,
        close: CloseStatus::default(),
    }
}
fn edited(gamma: f32) -> Settings {
    Settings {
        pressure_gamma: gamma,
        ..Default::default()
    }
}
fn preference(id: layer_ui::PreferenceId, value: f32) -> UiAction {
    UiAction::Preferences {
        action: layer_ui::PreferenceAction::Edit {
            id,
            value: layer_ui::PreferenceValue::Number(value),
        },
    }
}

#[test]
fn settings_round_trip_uses_shared_validation() {
    let directory = TempDir::new();
    let mut file = storage(&directory);
    assert!(file.load().unwrap().is_none());
    let first = edited(1.25);
    file.write(&encode(&first).unwrap()).unwrap();
    assert_eq!(file.load().unwrap(), Some(first));
    let second = edited(1.75);
    file.write(&encode(&second).unwrap()).unwrap();
    assert_eq!(file.load().unwrap(), Some(second));
    fs::write(
        directory.path.join("settings.json"),
        br#"{"pressure_gamma":1.5}"#,
    )
    .unwrap();
    assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.5);
    fs::write(
        directory.path.join("settings.json"),
        br#"{"pressure_gamma":1.5,"tip_lock":true,"zoom_speed":"fast"}"#,
    )
    .unwrap();
    assert_eq!(file.load().unwrap().unwrap().pressure_gamma, 1.5);
    assert_eq!(fs::read_dir(&directory.path).unwrap().count(), 1);
    assert!(SettingsFile::new(PathBuf::from("relative")).is_err());
}

#[test]
fn oversized_saved_preferences_are_bounded_and_not_rewritten_on_load() {
    let directory = TempDir::new();
    let path = directory.path.join("settings.json");
    fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
    assert_eq!(storage(&directory).load().unwrap(), Some(Settings::default()));
    assert_eq!(fs::metadata(path).unwrap().len(), (MAX_BYTES + 1) as u64);
}

#[cfg(target_os = "windows")]
#[test]
fn failed_replace_preserves_last_good_file_and_retry_succeeds() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = TempDir::new();
    let mut file = storage(&directory);
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
    let directory = TempDir::new();
    let mut file = storage(&directory);
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
    let mut service = service_over(worker);
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    host.dispatch(UiAction::Invoke {
        command: layer_ui::CommandId::OpenDocument,
    })
    .unwrap();
    host.dispatch(UiAction::OpenSettings {
        page: layer_ui::SettingsPage::Appearance,
    })
    .unwrap();
    host.dispatch(preference(layer_ui::PreferenceId::Pressure, 1.1))
        .unwrap();
    service.poll(&mut host).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    for i in 2..=100 {
        host.dispatch(preference(
            layer_ui::PreferenceId::Pressure,
            1. + i as f32 / 100.,
        ))
        .unwrap();
        service.poll(&mut host).unwrap();
        assert_eq!(
            host.session.state().requests.len(),
            2,
            "one settings save plus the unrelated Open document request"
        );
    }
    release.send(()).unwrap();
    service.finish(&mut host).unwrap();
    assert_eq!(host.session.state().requests.len(), 1);
    assert!(matches!(
        host.session.state().requests[0].kind,
        HostRequestKind::Document {
            request: layer_ui::DocumentRequest::Open
        }
    ));
    assert_eq!(storage(&directory).load().unwrap().unwrap().pressure_gamma, 2.);
    assert!(host.session.state().host_error.is_none());
}

#[test]
fn windows_merge_unrelated_edits_and_share_current_preferences() {
    let directory = TempDir::new();
    let mut first = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut second = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let wakes = Arc::new(AtomicU64::new(0));
    let count = wakes.clone();
    let mut a = SettingsService::at(&mut first, Ok(storage(&directory)), || {});
    let mut b = SettingsService::at(&mut second, Ok(storage(&directory)), move || {
        count.fetch_add(1, Ordering::Relaxed);
    });
    for host in [&mut first, &mut second] {
        host.dispatch(UiAction::OpenSettings {
            page: layer_ui::SettingsPage::Appearance,
        })
        .unwrap();
    }
    first
        .dispatch(preference(layer_ui::PreferenceId::Pressure, 1.25))
        .unwrap();
    a.poll(&mut first).unwrap();
    assert!(wakes.load(Ordering::Relaxed) > 0);
    // This owner has not consumed the notification yet: its edit is based on
    // the original settings and must not undo the first owner's pressure edit.
    second
        .dispatch(preference(layer_ui::PreferenceId::PanSpeed, 1.5))
        .unwrap();
    b.poll(&mut second).unwrap();
    a.poll(&mut first).unwrap();
    assert_eq!(
        first.session.state().settings,
        second.session.state().settings
    );
    assert_eq!(first.session.state().settings.pressure_gamma, 1.25);
    assert_eq!(first.session.state().settings.pan_speed, 1.5);
    let expected = first.session.state().settings.clone();
    a.finish(&mut first).unwrap();
    b.finish(&mut second).unwrap();
    assert_eq!(storage(&directory).load().unwrap(), Some(expected));
    assert!(first.session.state().requests.is_empty());
    assert!(second.session.state().requests.is_empty());
}

#[test]
fn new_window_inherits_pending_settings_and_stale_writes_cannot_replace_them() {
    let directory = TempDir::new();
    let hub = shared::Hub::open(storage(&directory), Settings::default()).unwrap();
    let mut first = shared::Subscription::new(hub.clone(), || {});
    let old = first.edit(&edited(1.25)).unwrap();
    let latest = first.edit(&edited(1.75)).unwrap();
    // There is no disk checkpoint yet. A new owner still sees the live value.
    assert!(!directory.path.join("settings.json").exists());
    let again = shared::Hub::open(storage(&directory), Settings::default()).unwrap();
    assert!(Arc::ptr_eq(&hub, &again));
    let mut second = shared::Subscription::new(again, || {});
    assert_eq!(
        second.adopt(&Settings::default()).unwrap().pressure_gamma,
        1.75
    );
    hub.write(&latest).unwrap();
    hub.write(&old).unwrap();
    assert_eq!(
        storage(&directory).load().unwrap().unwrap().pressure_gamma,
        1.75
    );
}

#[test]
fn settings_profiles_are_isolated_and_closed_callbacks_are_disarmed() {
    let first = TempDir::new();
    let other = TempDir::new();
    let a = shared::Hub::open(storage(&first), Settings::default()).unwrap();
    let b = shared::Hub::open(storage(&other), Settings::default()).unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    let wakes = Arc::new(AtomicU64::new(0));
    let count = wakes.clone();
    let closed = shared::Subscription::new(a.clone(), move || {
        count.fetch_add(1, Ordering::Relaxed);
    });
    let delayed = closed.notifier();
    closed.stop();
    let mut live = shared::Subscription::new(a, || {});
    live.edit(&edited(1.5)).unwrap();
    delayed();
    assert_eq!(wakes.load(Ordering::Relaxed), 0);
    let mut isolated = shared::Subscription::new(b, || {});
    assert!(isolated.adopt(&Settings::default()).is_none());
}

#[test]
fn disconnect_waits_for_an_inflight_settings_callback() {
    let directory = TempDir::new();
    let hub = shared::Hub::open(storage(&directory), Settings::default()).unwrap();
    let (entered, started) = mpsc::channel();
    let (release, held) = mpsc::channel();
    let client = Arc::new(shared::Subscription::new(hub, move || {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let notify = client.notifier();
    let running = std::thread::spawn(notify);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done, stopped) = mpsc::channel();
    let closing = client.clone();
    let disconnect = std::thread::spawn(move || {
        closing.stop();
        done.send(()).unwrap();
    });
    assert!(stopped.recv_timeout(Duration::from_millis(20)).is_err());
    release.send(()).unwrap();
    stopped.recv_timeout(Duration::from_secs(5)).unwrap();
    running.join().unwrap();
    disconnect.join().unwrap();
    client.notifier()();
    assert!(
        started.try_recv().is_err(),
        "a retained notifier called a closed host"
    );
}

#[test]
fn concurrent_shortcut_edits_do_not_resurrect_a_removed_override() {
    let directory = TempDir::new();
    let mut baseline = Settings::default();
    baseline.shortcuts.insert("command.Undo".into(), vec![]);
    let hub = shared::Hub::open(storage(&directory), baseline.clone()).unwrap();
    let mut first = shared::Subscription::new(hub.clone(), || {});
    let mut second = shared::Subscription::new(hub, || {});
    let mut removed = baseline.clone();
    removed.shortcuts.remove("command.Undo");
    first.edit(&removed).unwrap();
    let mut other = baseline.clone();
    other.shortcuts.insert("command.Redo".into(), vec![]);
    second.edit(&other).unwrap();
    let merged = second.adopt(&baseline).unwrap();
    assert!(!merged.shortcuts.contains_key("command.Undo"));
    assert_eq!(merged.shortcuts.get("command.Redo"), Some(&vec![]));
}

#[test]
fn restoring_settings_does_not_echo_a_save_request() {
    let directory = TempDir::new();
    storage(&directory)
        .write(&encode(&edited(1.5)).unwrap())
        .unwrap();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(storage(&directory)), || {});
    assert_eq!(host.session.state().settings.pressure_gamma, 1.5);
    assert!(host.session.state().requests.is_empty());
    service.finish(&mut host).unwrap();
}

#[test]
fn slider_bookmarks_survive_the_shared_save_and_sync() {
    let directory = TempDir::new();
    let (wake, woke) = mpsc::channel();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(storage(&directory)), move || {
        let _ = wake.send(());
    });
    let context = host.session.state().toolbar_context();
    host.dispatch(UiAction::ToolbarEdit {
        context,
        action: Box::new(UiAction::ToggleSliderBookmark {
            control: layer_ui::ToolbarControl::BrushSizeSlider,
        }),
    })
    .unwrap();
    let saved = host.session.state().settings.slider_bookmarks.clone();
    assert!(!saved.is_empty());
    for _ in 0..4 {
        service.poll(&mut host).unwrap();
        let _ = woke.recv_timeout(Duration::from_millis(200));
    }
    service.poll(&mut host).unwrap();
    assert_eq!(host.session.state().settings.slider_bookmarks, saved);
    assert_eq!(storage(&directory).load().unwrap().unwrap().slider_bookmarks, saved);
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
    let mut service = service_over(worker);
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    host.dispatch(UiAction::OpenSettings {
        page: layer_ui::SettingsPage::Appearance,
    })
    .unwrap();
    for (gamma, failed) in [(1.25, true), (1.75, false)] {
        host.dispatch(preference(layer_ui::PreferenceId::Pressure, gamma))
            .unwrap();
        service.poll(&mut host).unwrap();
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
        service.poll(&mut host).unwrap();
        assert_eq!(host.session.state().host_error.is_some(), failed);
        assert!(host.session.state().requests.is_empty());
    }
    service.finish(&mut host).unwrap();
}

fn change_preferences(host: &mut NativeHost, gamma: f32) {
    host.dispatch(UiAction::OpenSettings {
        page: layer_ui::SettingsPage::Appearance,
    })
    .unwrap();
    host.dispatch(preference(layer_ui::PreferenceId::Pressure, gamma))
        .unwrap();
    host.dispatch(UiAction::CloseSettings).unwrap();
}
fn pump_close(
    host: &mut NativeHost,
    service: &mut SettingsService,
    wake: &mpsc::Receiver<()>,
    failed: bool,
) {
    loop {
        service.poll(host).unwrap();
        if if failed {
            service.close_status().error.is_some()
        } else {
            service.close_status().ready
        } {
            break;
        }
        wake.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}

#[test]
fn close_waits_for_the_latest_write_and_does_not_stop_the_worker() {
    let directory = TempDir::new();
    let mut file = storage(&directory);
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (wake, completed) = mpsc::channel();
    let mut first = true;
    let worker = Worker::start(
        move |bytes| {
            if std::mem::take(&mut first) {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            file.write(bytes)
        },
        move || {
            let _ = wake.send(());
        },
    )
    .unwrap();
    let mut service = service_over(worker);
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    change_preferences(&mut host, 1.25);
    service.poll(&mut host).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    change_preferences(&mut host, 1.75);
    host.session.request_document_close().unwrap();
    service.poll(&mut host).unwrap();
    assert!(service.close_status().busy);
    assert!(!service.close_status().ready);
    // A stale recovery button must not authorize an in-flight write to stop.
    service.discard_close(&mut host);
    service.keep_open(&mut host);
    assert!(!service.close_status().ready);
    assert!(host.session.state().document_file.close_ready);
    release.send(()).unwrap();
    pump_close(&mut host, &mut service, &completed, false);
    assert_eq!(
        storage(&directory).load().unwrap().unwrap().pressure_gamma,
        1.75
    );
    service.finish(&mut host).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn failed_close_retains_edits_and_retries_without_changing_a_preference() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = TempDir::new();
    storage(&directory)
        .write(&encode(&edited(1.25)).unwrap())
        .unwrap();
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(directory.path.join("settings.json"))
        .unwrap();
    let (wake, completed) = mpsc::channel();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(storage(&directory)), move || {
        let _ = wake.send(());
    });
    change_preferences(&mut host, 1.75);
    host.session.request_document_close().unwrap();
    pump_close(&mut host, &mut service, &completed, true);
    assert!(!service.close_status().ready);
    let attempt = service.close_status().attempt;
    service.retry_close(&mut host).unwrap();
    pump_close(&mut host, &mut service, &completed, true);
    assert!(service.close_status().attempt > attempt);
    assert_eq!(
        storage(&directory).load().unwrap().unwrap().pressure_gamma,
        1.25
    );
    service.keep_open(&mut host);
    service.poll(&mut host).unwrap();
    assert!(!host.session.state().document_file.close_ready);
    assert!(!service.close_status().requested);
    assert_eq!(host.session.state().settings.pressure_gamma, 1.75);
    drop(locked);
    host.session.request_document_close().unwrap();
    pump_close(&mut host, &mut service, &completed, false);
    assert_eq!(
        storage(&directory).load().unwrap().unwrap().pressure_gamma,
        1.75
    );
    assert!(host.session.state().host_error.is_none());
    service.finish(&mut host).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
fn discard_failed_preferences_leaves_the_saved_file_unchanged() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = TempDir::new();
    let expected = encode(&edited(1.25)).unwrap();
    storage(&directory).write(&expected).unwrap();
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(directory.path.join("settings.json"))
        .unwrap();
    let (wake, completed) = mpsc::channel();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(storage(&directory)), move || {
        let _ = wake.send(());
    });
    change_preferences(&mut host, 1.75);
    host.session.request_document_close().unwrap();
    pump_close(&mut host, &mut service, &completed, true);
    service.discard_close(&mut host);
    service.poll(&mut host).unwrap();
    assert!(service.close_status().ready);
    service.finish(&mut host).unwrap();
    drop(locked);
    assert_eq!(
        fs::read(directory.path.join("settings.json")).unwrap(),
        expected
    );
}

#[cfg(target_os = "windows")]
#[test]
fn another_window_can_flush_shared_unsaved_preferences_when_closing() {
    use std::os::windows::fs::OpenOptionsExt;
    let directory = TempDir::new();
    storage(&directory)
        .write(&encode(&edited(1.25)).unwrap())
        .unwrap();
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(directory.path.join("settings.json"))
        .unwrap();
    let (wake_a, completed_a) = mpsc::channel();
    let mut first = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut a = SettingsService::at(&mut first, Ok(storage(&directory)), move || {
        let _ = wake_a.send(());
    });
    change_preferences(&mut first, 1.75);
    a.poll(&mut first).unwrap();
    while SettingsService::pending(&first) {
        completed_a.recv_timeout(Duration::from_secs(5)).unwrap();
        a.poll(&mut first).unwrap();
    }
    assert!(first.session.state().host_error.is_some());
    let (wake_b, completed_b) = mpsc::channel();
    let mut second = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut b = SettingsService::at(&mut second, Ok(storage(&directory)), move || {
        let _ = wake_b.send(());
    });
    assert_eq!(second.session.state().settings.pressure_gamma, 1.75);
    assert!(second.session.state().requests.is_empty());
    drop(locked);
    second.session.request_document_close().unwrap();
    pump_close(&mut second, &mut b, &completed_b, false);
    assert_eq!(
        storage(&directory).load().unwrap().unwrap().pressure_gamma,
        1.75
    );
    a.finish(&mut first).unwrap();
    b.finish(&mut second).unwrap();
}

#[test]
fn clean_close_does_not_write_missing_preferences() {
    let directory = TempDir::new();
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = SettingsService::at(&mut host, Ok(storage(&directory)), || {});
    host.session.request_document_close().unwrap();
    service.poll(&mut host).unwrap();
    assert!(service.close_status().ready);
    service.finish(&mut host).unwrap();
    assert!(!directory.path.join("settings.json").exists());
}

#[test]
fn stopped_storage_reports_the_latest_close_write_and_allows_discard() {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (wake, completed) = mpsc::channel();
    let worker = Worker::start(
        move |_| {
            entered.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            panic!("isolated storage worker failure");
        },
        move || {
            let _ = wake.send(());
        },
    )
    .unwrap();
    let mut service = service_over(worker);
    let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    change_preferences(&mut host, 1.25);
    service.poll(&mut host).unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    change_preferences(&mut host, 1.75);
    host.session.request_document_close().unwrap();
    service.poll(&mut host).unwrap();
    release.send(()).unwrap();
    completed
        .recv_timeout(Duration::from_secs(2))
        .expect("stopped storage must wake close recovery");
    service.poll(&mut host).unwrap();
    assert!(service.close_status().error.is_some());
    assert!(!service.close_status().busy && !service.close_status().ready);
    service.retry_close(&mut host).unwrap();
    assert!(service.close_status().error.is_some());
    assert!(!service.close_status().busy);
    service.discard_close(&mut host);
    assert!(service.close_status().ready);
    service.finish(&mut host).unwrap();
}

#[test]
fn prepared_launch_restores_before_views_and_keeps_the_profile_context() {
    let directory = TempDir::new();
    let literal = "HDR { $name } 🖌 日本語";
    let mut initial = edited(1.5);
    initial.language = layer_ui::LanguagePreference::Explicit(layer_ui::UiLanguage::English);
    initial.new_document.presets.push(layer_ui::NewDocumentPreset { name: literal.into(), options: initial.new_document.defaults });
    let saved = serde_json::to_string(&initial).unwrap();
    fs::write(directory.path.join("settings.json"), &saved).unwrap();
    let (first, prepared) = SettingsService::launch_at(Ok(storage(&directory)), &["ja-JP", "ko", "en"]).unwrap();
    assert_eq!(first.session.localization().language(), layer_ui::UiLanguage::English);
    assert_eq!(first.session.state().settings.pressure_gamma, 1.5);
    assert_eq!(first.session.state().settings.new_document.presets[0].name, literal);
    assert_eq!(fs::read_to_string(directory.path.join("settings.json")).unwrap(), saved);
    let active = first.session.localization().clone();
    drop(first);drop(prepared);
    let (reopened, _) = shared::Hub::with_launch(storage(&directory), |_| panic!("Closing all windows must retain the launch context")).unwrap();
    assert!(Arc::ptr_eq(reopened.session.localization(), &active));
    let japanese = TempDir::new();
    fs::write(japanese.path.join("settings.json"), &saved).unwrap();
    let (native, hub) = shared::Hub::with_launch(storage(&japanese), |saved| NativeHost::launch_localized(layer_ui::Platform::Windows, saved.unwrap_or_default(), layer_ui::Localizer::shared(layer_ui::UiLanguage::Japanese))).unwrap();
    let retained = native.session.localization().clone();
    assert!(!Arc::ptr_eq(&active, &retained));
    let mut subscriber = shared::Subscription::new(hub.clone(), || {});
    let mut desired = native.session.state().settings.clone();
    desired.language = layer_ui::LanguagePreference::System;
    subscriber.edit(&desired).unwrap();
    drop(native);drop(subscriber);drop(hub);
    let (later, prepared) = SettingsService::launch_at(Ok(storage(&japanese)), &["en-US"]).unwrap();
    drop(prepared);
    assert!(Arc::ptr_eq(later.session.localization(), &retained));
    assert_eq!(later.session.state().settings.language, layer_ui::LanguagePreference::System);
    assert_eq!(later.session.state().settings.pressure_gamma, 1.5);
    assert_eq!(later.session.state().settings.new_document.presets[0].name, literal);
}
