//! Real removal at native file completion and shared validation publication boundaries.
use super::*;
use crate::{
    device::D3d12Watch,
    gpu_recovery_tests::{remove_device, renderer},
};
use layer_host::DeviceWatch;
use layer_ui::{CommandId, EffectAction, Platform, UiAction, UiSession};

struct Fixture {
    native: NativeHost,
    service: FilterService,
    device: DeviceWatch,
    directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let (renderer, device) = renderer();
        let mut native = NativeHost::new(Platform::Windows).unwrap();
        native.session = UiSession::from_project(
            renderer,
            layer_ui::new_drawing(64, 48).unwrap(),
            None,
            [64, 48],
        )
        .unwrap();
        native.session.set_platform(Platform::Windows);
        native.startup = Default::default();
        native
            .import_layer_image(
                "Synthetic color",
                layer_render::HostImage {
                    width: 4,
                    height: 3,
                    stride: 16,
                    format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                    bytes: &[230, 71, 42, 255].repeat(12),
                },
            )
            .unwrap();
        let directory = Directory::new();
        directory.example();
        let mut service = FilterService::new(|| {});
        service
            .load(
                &mut native,
                Request {
                    directory: Some(directory.path.clone()),
                    mode: EffectInstallMode::Add,
                    library: false,
                },
            )
            .unwrap();
        finish(&mut service, &mut native);
        assert_eq!(service.status.phase, "ready", "{:?}", service.status);
        native
            .dispatch(UiAction::Effect {
                action: EffectAction::Insert {
                    effect: "example:tent_blur".into(),
                },
            })
            .unwrap();
        let layer = native.session.engine().document().active_layer.0;
        native
            .dispatch(UiAction::Effect {
                action: EffectAction::Set {
                    layer,
                    key: "radius".into(),
                    value: layer_core::EffectValue::Number(7.),
                },
            })
            .unwrap();
        image(&mut native);
        Self {
            native,
            service,
            device,
            directory,
        }
    }
    fn begin_replace(&mut self) {
        let path = self.directory.path.join("tent.wgsl");
        let shader = fs::read_to_string(&path).unwrap();
        let changed = shader.replace(
            "return value;",
            "return vec4<f32>(value.r*0.5,value.g,value.b,value.a);",
        );
        assert_ne!(changed, shader);
        fs::write(path, changed).unwrap();
        self.service
            .load(
                &mut self.native,
                Request {
                    directory: Some(self.directory.path.clone()),
                    mode: EffectInstallMode::Replace,
                    library: false,
                },
            )
            .unwrap();
    }
    fn remove(&mut self) {
        remove_device(self.native.session.engine().backend(), &self.device);
    }
    fn retire(&mut self) {
        drop(self.native.session.renderer_mut().0.take());
        self.native.startup = Default::default();
    }
    fn restore(&mut self) {
        let (renderer, device) = renderer();
        let revision = self.native.session.state().revision;
        let (old, change) = self.native.session.replace_renderer(renderer).unwrap();
        self.native.apply_change(revision, change);
        drop(old);
        self.native.startup = Default::default();
        self.device = device;
    }
    fn erase_sources(&self) {
        for name in ["manifest.json", "prepare.wgsl", "tent.wgsl"] {
            fs::remove_file(self.directory.path.join(name)).unwrap();
        }
    }
    fn verify(&mut self, original: &[u8], expected: &[u8], catalog: u64) {
        finish(&mut self.service, &mut self.native);
        assert_eq!(
            self.service.status.phase, "ready",
            "{:?}",
            self.service.status
        );
        assert_eq!(
            self.native.session.state().filter_catalog_revision,
            catalog + 1
        );
        assert_eq!(self.service.status.request_id, 2);
        assert_eq!(radius(&self.native), layer_core::EffectValue::Number(7.));
        assert_eq!(image(&mut self.native), expected);
        self.native
            .dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            })
            .unwrap();
        assert_eq!(image(&mut self.native), original);
        self.native
            .dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            })
            .unwrap();
        assert_eq!(image(&mut self.native), expected);
        self.device.check().unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.service.stop();
    }
}
fn replacement_pixels() -> Vec<u8> {
    let mut fixture = Fixture::new();
    let before = image(&mut fixture.native);
    fixture.begin_replace();
    finish(&mut fixture.service, &mut fixture.native);
    assert_eq!(
        fixture.service.status.phase, "ready",
        "{:?}",
        fixture.service.status
    );
    let after = image(&mut fixture.native);
    assert_ne!(after, before);
    after
}

#[test]
#[ignore = "requires hardware D3D12; run this module alone and serially"]
fn file_completion_waits_for_replacement_device() {
    let expected = replacement_pixels();
    let mut fixture = Fixture::new();
    let original = image(&mut fixture.native);
    let catalog = fixture.native.session.state().filter_catalog_revision;
    fixture.begin_replace();
    fixture.service.poll(&mut fixture.native); // Start transport before removal.
    fixture.remove();
    finish_read(&mut fixture.service, &mut fixture.native);
    assert_eq!(fixture.service.status.phase, "waiting_for_canvas");
    assert!(fixture.service.acquired.is_some());
    assert!(!fixture.native.session.state().filter_load.pending);
    fixture.erase_sources();
    fixture.retire();
    fixture.service.poll(&mut fixture.native);
    assert_eq!(fixture.service.status.phase, "waiting_for_canvas");
    assert_eq!(
        fixture.native.session.state().filter_catalog_revision,
        catalog
    );
    fixture.restore();
    fixture.verify(&original, &expected, catalog);
}

#[test]
#[ignore = "requires hardware D3D12; run this module alone and serially"]
fn validation_restarts_from_retained_sources_after_removal() {
    let expected = replacement_pixels();
    let mut fixture = Fixture::new();
    let original = image(&mut fixture.native);
    let catalog = fixture.native.session.state().filter_catalog_revision;
    fixture.begin_replace();
    finish_read(&mut fixture.service, &mut fixture.native);
    assert_eq!(fixture.service.status.phase, "validating");
    assert!(fixture.native.session.state().filter_load.pending);
    let id = fixture.native.session.state().filter_load.request_id;
    fixture.erase_sources();
    fixture.remove();
    fixture.retire();
    fixture.service.poll(&mut fixture.native);
    assert!(fixture.native.session.state().filter_load.pending);
    assert_eq!(
        fixture.native.session.state().filter_catalog_revision,
        catalog
    );
    fixture.restore();
    assert_eq!(fixture.native.session.state().filter_load.request_id, id);
    fixture.verify(&original, &expected, catalog);
}

#[test]
#[ignore = "requires hardware D3D12; run this module alone and serially"]
fn failed_recovery_cancels_validation_without_publishing() {
    let mut fixture = Fixture::new();
    let original = image(&mut fixture.native);
    let catalog = fixture.native.session.state().filter_catalog_revision;
    let revision = fixture.native.session.engine().document().revision;
    fixture.begin_replace();
    finish_read(&mut fixture.service, &mut fixture.native);
    assert_eq!(fixture.service.status.phase, "validating");
    fixture.erase_sources();
    fixture.remove();
    fixture.retire();
    fixture.native.suspend_renderer().unwrap();
    fixture.service.poll(&mut fixture.native);
    assert!(!fixture.service.status.pending);
    assert_eq!(fixture.service.status.phase, "failed");
    assert!(
        fixture
            .service
            .status
            .error
            .as_ref()
            .unwrap()
            .contains("unavailable")
    );
    assert!(!fixture.native.session.state().filter_load.pending);
    assert_eq!(
        fixture.native.session.state().filter_catalog_revision,
        catalog
    );
    assert_eq!(
        fixture.native.session.engine().document().revision,
        revision
    );
    assert_eq!(radius(&fixture.native), layer_core::EffectValue::Number(7.));
    // A fresh renderer can still reconstruct the unchanged source document.
    fixture.restore();
    assert_eq!(image(&mut fixture.native), original);
    fixture.device.check().unwrap();
}
