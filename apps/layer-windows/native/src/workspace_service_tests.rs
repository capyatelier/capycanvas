use super::*;
use crate::test_support::TempDir;
use layer_workspace::{PackageKind, StoreWorker};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct Fixture {
    service: WorkspaceService<StoreWorker>,
    native: NativeHost,
    notifications: mpsc::Receiver<()>,
    directory: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self::localized(layer_ui::UiLanguage::English)
    }
    fn localized(language: layer_ui::UiLanguage) -> Self {
        let mut native = NativeHost::launch_localized(Platform::Windows, "", layer_ui::Localizer::shared(language)).unwrap();
        let directory = TempDir::new();
        let (notify, notifications) = mpsc::channel();
        let service = WorkspaceService::new(
            StoreWorker::shared(&directory.path).unwrap(),
            directory.path.clone(),
            native.session.localization().clone(),
            move || {
                let _ = notify.send(());
            },
        );
        crate::workspace::initialize(&mut native).unwrap();
        let mut f = Self {
            service,
            native,
            notifications,
            directory,
        };
        f.pump(|f| f.service.view().ready);
        f
    }
    fn notice(&self) -> Option<String> {
        self.service.notice.clone()
    }
    fn pump(&mut self, until: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.service.poll(&mut self.native, now_ms());
            if until(self) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Workspace operation timed out: {:?}",
                self.service.view().error
            );
            let _ = self.notifications.recv_timeout(Duration::from_millis(5));
        }
    }
}

#[test]
fn export_backup_waits_for_idle_and_reports_notice() {
    let mut f = Fixture::new();
    let path = f.directory.path.join("backup.capyworkspace");
    f.service
        .export_backup(&mut f.native, path.to_string_lossy().into_owned())
        .unwrap();
    assert!(
        f.service
            .export_backup(&mut f.native, path.to_string_lossy().into_owned())
            .is_err()
    );
    f.pump(|f| f.notice().is_some());
    assert_eq!(f.notice().as_deref(), Some("Workspace backup saved."));
    let restored = layer_workspace::import_package(
        &std::fs::read(&path).unwrap(),
        PackageKind::WorkspaceBackup,
        now_ms(),
    )
    .unwrap();
    assert_eq!(restored.metadata.name, f.service.view().name);
    let status = serde_json::to_value(f.service.status(&f.native)).unwrap();
    assert_eq!(status["notice"], "Workspace backup saved.");
    assert_eq!(status["id"], f.service.view().id.clone().unwrap());
    f.service
        .input(&mut f.native, WorkspaceInput::Close)
        .unwrap();
    f.pump(|f| f.service.view().closed);
    assert_eq!(
        serde_json::to_value(f.service.status(&f.native)).unwrap()["close_attempt"],
        1
    );
    f.service.stop();
}

#[test]
fn workspace_service_refreshes_open_prompts_and_preserves_literal_names() {
    let mut f = Fixture::localized(layer_ui::UiLanguage::Japanese);
    let active = f.native.session.localization().clone();
    assert!(Arc::ptr_eq(f.service.controller.localization(), &active));
    let input = serde_json::from_value(serde_json::json!({"type":"form","action":{"type":"new"}})).unwrap();
    f.service.input(&mut f.native, input).unwrap();
    let prompt = f.service.view().prompt.as_ref().unwrap();
    assert_eq!(prompt.title, "新規ワークスペース");
    assert_eq!(prompt.confirm, "作成して切り替え");
    let english = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
    f.native.set_localization(english.clone());f.service.poll(&mut f.native, now_ms());
    assert!(Arc::ptr_eq(f.service.controller.localization(), &english));
    assert_ne!(f.service.view().prompt.as_ref().unwrap().title, "新規ワークスペース");
    let literal = "HDR 日本語 中文 한국어 🖌️ { $name }\u{2068}literal\u{2069}";
    f.service.input(&mut f.native, WorkspaceInput::Submit { name: literal.into(), description: None, choice: None }).unwrap();
    f.pump(|f| f.service.view().name == literal && !f.service.view().busy);
    assert!(Arc::ptr_eq(f.service.controller.localization(), &english));
    assert_eq!(f.service.controller.manager.current().unwrap().metadata.name, literal);
    f.service.stop();
}
