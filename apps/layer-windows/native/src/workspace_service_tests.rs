use super::*;
use layer_workspace::{PackageKind, StoreWorker, new_id};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct Fixture {
    directory: std::path::PathBuf,
    service: WorkspaceService<StoreWorker>,
    native: NativeHost,
    notifications: mpsc::Receiver<()>,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("capy-windows-workspaces-{}", new_id()));
        let (notify, notifications) = mpsc::channel();
        let service = WorkspaceService::new(
            StoreWorker::shared(&directory).unwrap(),
            directory.clone(),
            move || {
                let _ = notify.send(());
            },
        );
        let mut native = NativeHost::new(Platform::Windows).unwrap();
        crate::workspace::initialize(&mut native).unwrap();
        let mut f = Self {
            directory,
            service,
            native,
            notifications,
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
    let path = f.directory.join("backup.capyworkspace");
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
    let directory = f.directory.clone();
    drop(f);
    std::fs::remove_dir_all(directory).unwrap();
}
