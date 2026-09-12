use super::*;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Directory {
    base: PathBuf,
    path: PathBuf,
}
impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../artifacts/windows/filter-tests");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let path = base.join(format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self { base, path }
    }
    fn example(&self) {
        let example =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples/filters/tent-blur");
        for name in ["manifest.json", "prepare.wgsl", "tent.wgsl"] {
            fs::copy(example.join(name), self.path.join(name)).unwrap();
        }
    }
    fn manifest(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.path.join("manifest.json")).unwrap()).unwrap()
    }
    fn set_manifest(&self, value: serde_json::Value) {
        fs::write(
            self.path.join("manifest.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn read(&self) -> Result<Package, String> {
        read_directory(&self.path, EffectInstallMode::Merge, false)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        if let Ok(path) = self.path.canonicalize()
            && path == self.path
            && path.parent() == Some(self.base.as_path())
        {
            let _ = fs::remove_dir_all(path);
        }
    }
}
fn error(directory: &Directory) -> String {
    match directory.read() {
        Ok(_) => panic!("Invalid package was accepted"),
        Err(error) => error,
    }
}
#[test]
fn file_transport_resolves_shared_render_and_preparation_modules() {
    let directory = Directory::new();
    directory.example();
    let package = directory.read().unwrap();
    assert_eq!(package.modules.len(), 2);
    let catalog = EffectPackage::parse(&package.manifest)
        .unwrap()
        .resolve(|name| {
            package
                .modules
                .get(name)
                .cloned()
                .ok_or("Missing module".into())
        })
        .unwrap();
    assert!(catalog.get("example:tent_blur").is_some());
    assert!(package.modules["prepare.wgsl"].contains("prepare_tent"));
    let changed = package.modules["tent.wgsl"].to_string()
        + "\n// Edited without rebuilding the application\n";
    fs::write(directory.path.join("tent.wgsl"), &changed).unwrap();
    assert_eq!(
        directory.read().unwrap().modules["tent.wgsl"].as_ref(),
        changed
    );
}
#[test]
fn manifest_paths_are_rejected_before_any_module_read() {
    let directory = Directory::new();
    directory.example();
    let original = directory.manifest();
    for name in [
        "../outside.wgsl",
        "C:/outside.wgsl",
        "nested/module.wgsl",
        ".hidden.wgsl",
        "other.wgsl:stream",
    ] {
        let mut manifest = original.clone();
        manifest["filters"][0]["program"]["wgsl"] = serde_json::json!([name]);
        directory.set_manifest(manifest);
        assert!(error(&directory).contains("module filename"));
    }
}
#[test]
fn missing_invalid_and_oversized_resources_preserve_source_files() {
    let directory = Directory::new();
    directory.example();
    let manifest = fs::read(directory.path.join("manifest.json")).unwrap();
    fs::remove_file(directory.path.join("prepare.wgsl")).unwrap();
    assert!(error(&directory).contains("open"));
    assert_eq!(
        fs::read(directory.path.join("manifest.json")).unwrap(),
        manifest
    );
    fs::write(directory.path.join("prepare.wgsl"), [0xff]).unwrap();
    assert!(error(&directory).contains("UTF-8"));
    fs::File::create(directory.path.join("prepare.wgsl"))
        .unwrap()
        .set_len((MODULE_LIMIT + 1) as u64)
        .unwrap();
    assert!(error(&directory).contains("limits"));
    fs::File::create(directory.path.join("manifest.json"))
        .unwrap()
        .set_len((MANIFEST_LIMIT + 1) as u64)
        .unwrap();
    assert!(error(&directory).contains("limits"));
}
#[test]
fn aggregate_module_reads_are_bounded() {
    let directory = Directory::new();
    directory.example();
    let mut manifest = directory.manifest();
    let modules: Vec<_> = (0..17).map(|i| format!("module{i}.wgsl")).collect();
    manifest["filters"][0]["program"]["wgsl"] = serde_json::json!(modules);
    directory.set_manifest(manifest);
    for name in modules {
        fs::File::create(directory.path.join(name))
            .unwrap()
            .set_len(MODULE_LIMIT as u64)
            .unwrap();
    }
    assert!(error(&directory).contains("limits"));
}
fn finish_read(service: &mut FilterService, native: &mut NativeHost) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while service.status.phase == "reading" && Instant::now() < deadline {
        service.poll(native);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_ne!(service.status.phase, "reading");
}
#[test]
fn asynchronous_read_retains_bytes_until_gpu_attachment_and_rejects_overlap() {
    let directory = Directory::new();
    directory.example();
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let before = native.session.state().adjustments.clone();
    let mut service = FilterService::new(|| {});
    let request = || Request {
        directory: Some(directory.path.clone()),
        mode: EffectInstallMode::Add,
        library: false,
    };
    service.load(&mut native, request()).unwrap();
    assert!(service.load(&mut native, request()).is_err());
    finish_read(&mut service, &mut native);
    assert_eq!(service.status.phase, "waiting_for_canvas");
    assert!(service.status.pending);
    assert_eq!(service.status.request_id, 1);
    assert!(service.acquired.is_some());
    assert!(!native.session.state().filter_load.pending);
    assert_eq!(
        serde_json::to_value(&native.session.state().adjustments).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    service.stop();
    assert!(service.acquired.is_none());
}
#[test]
fn asynchronous_failure_is_visible_and_a_later_read_can_retry() {
    let directory = Directory::new();
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = FilterService::new(|| {});
    let request = || Request {
        directory: Some(directory.path.clone()),
        mode: EffectInstallMode::Merge,
        library: true,
    };
    service.load(&mut native, request()).unwrap();
    finish_read(&mut service, &mut native);
    assert!(!service.status.pending);
    assert_eq!(service.status.phase, "failed");
    assert!(service.status.error.is_some());
    assert_eq!(native.session.state().filter_catalog_revision, 0);
    directory.example();
    service.load(&mut native, request()).unwrap();
    finish_read(&mut service, &mut native);
    assert_eq!(service.status.phase, "waiting_for_canvas");
    assert_eq!(service.status.request_id, 2);
    assert!(service.status.error.is_none());
    service.stop();
}

#[test]
fn delayed_explicit_import_cannot_migrate_a_replacement_document() {
    for library in [false, true] {
        let directory = Directory::new();
        directory.example();
        let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut service = FilterService::new(|| {});
        service
            .load(
                &mut native,
                Request {
                    directory: Some(directory.path.clone()),
                    mode: EffectInstallMode::Merge,
                    library,
                },
            )
            .unwrap();
        finish_read(&mut service, &mut native);
        let replacement = layer_ui::UiSession::from_project(
            layer_host::Renderer(None),
            layer_ui::new_drawing(32, 24).unwrap(),
            None,
            [32, 24],
        )
        .unwrap();
        let epoch = native.session.state().document_file.epoch;
        let revision = native.session.engine().document().revision;
        let retired = native
            .session
            .adopt_project(Box::new(replacement), epoch, revision, None)
            .map_err(|(error, _)| error)
            .unwrap();
        drop(retired);
        assert_ne!(native.session.state().document_file.epoch, epoch);
        service.poll(&mut native);
        if library {
            assert_eq!(service.status.phase, "waiting_for_canvas");
        } else {
            assert_eq!(service.status.phase, "failed");
            assert!(
                service
                    .status
                    .error
                    .as_ref()
                    .unwrap()
                    .contains("document changed")
            );
            assert!(service.acquired.is_none());
        }
        assert_eq!(native.session.state().filter_catalog_revision, 0);
        service.stop();
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
fn d3d12_file_packages_replace_pixels_atomically_and_preserve_live_values() {
    use layer_render::CanvasRenderer;
    use layer_ui::{EffectAction, UiAction};
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))
    .unwrap();
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Windows runtime filter correctness"),
        required_limits: limits,
        ..Default::default()
    }))
    .unwrap();
    let gpu = layer_render_wgpu::WgpuRasterizer::from_wgpu_staged(adapter, device, queue).unwrap();
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    native.session = layer_ui::UiSession::from_project(
        layer_host::Renderer(Some(gpu)),
        layer_ui::new_drawing(64, 48).unwrap(),
        None,
        [64, 48],
    )
    .unwrap();
    native.session.set_platform(layer_ui::Platform::Windows);
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
    let shader = fs::read_to_string(directory.path.join("tent.wgsl")).unwrap();
    let mut service = FilterService::new(|| {});
    let request = |mode, library| Request {
        directory: Some(directory.path.clone()),
        mode,
        library,
    };
    fn finish(service: &mut FilterService, native: &mut NativeHost) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while service.status.pending {
            service.poll(native);
            native.prepare_canvas_frame(0, 0, true).unwrap();
            assert!(Instant::now() < deadline, "Package loading did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn image(native: &mut NativeHost) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            native.prepare_canvas_frame(0, 0, true).unwrap();
            if native.startup.brush_ready {
                break;
            }
            assert!(Instant::now() < deadline, "Canvas did not become ready");
            std::thread::sleep(Duration::from_millis(1));
        }
        native.session.renderer_mut().request_readback(1).unwrap();
        native
            .session
            .renderer_mut()
            .take_readback()
            .unwrap()
            .unwrap()
            .bytes
    }
    fn radius(native: &NativeHost) -> layer_core::EffectValue {
        let layer = native.session.engine().document().active_layer;
        native
            .session
            .engine()
            .document()
            .layers
            .iter()
            .find(|l| l.id == layer)
            .unwrap()
            .effect
            .as_ref()
            .unwrap()
            .value("radius")
            .unwrap()
            .clone()
    }
    service
        .load(&mut native, request(EffectInstallMode::Add, false))
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
    let active = native.session.engine().document().active_layer.0;
    native
        .dispatch(UiAction::Effect {
            action: EffectAction::Set {
                layer: active,
                key: "radius".into(),
                value: layer_core::EffectValue::Number(7.),
            },
        })
        .unwrap();
    let before = image(&mut native);
    let revision = native.session.state().filter_catalog_revision;
    service
        .load(&mut native, request(EffectInstallMode::Add, false))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "failed");
    assert_eq!(native.session.state().filter_catalog_revision, revision);
    assert_eq!(image(&mut native), before);

    let changed = shader.replace(
        "return value;",
        "return vec4<f32>(value.r*0.5,value.g,value.b,value.a);",
    );
    assert_ne!(changed, shader);
    fs::write(directory.path.join("tent.wgsl"), &changed).unwrap();
    service
        .load(&mut native, request(EffectInstallMode::Replace, false))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "ready", "{:?}", service.status);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    let replaced = image(&mut native);
    assert_ne!(
        replaced, before,
        "Live WGSL replacement did not change rendered pixels"
    );
    let revision = native.session.engine().document().revision;
    let catalog = native.session.state().filter_catalog_revision;
    fs::write(directory.path.join("tent.wgsl"), "not valid WGSL").unwrap();
    service
        .load(&mut native, request(EffectInstallMode::Replace, false))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "failed");
    assert_eq!(native.session.engine().document().revision, revision);
    assert_eq!(native.session.state().filter_catalog_revision, catalog);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    assert_eq!(image(&mut native), replaced);

    // Different definitions of the same WGSL symbol cannot coexist with an
    // embedded document program. Reject that library update without migration.
    fs::write(directory.path.join("tent.wgsl"), &shader).unwrap();
    service
        .load(&mut native, request(EffectInstallMode::Merge, true))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "failed", "{:?}", service.status);
    assert_eq!(native.session.engine().document().revision, revision);
    assert_eq!(native.session.state().filter_catalog_revision, catalog);
    assert_eq!(image(&mut native), replaced);

    // A compatible namespace can refresh the library while preserving the
    // older program and values embedded in the current document.
    fs::write(
        directory.path.join("tent.wgsl"),
        shader.replace("tent_", "library_tent_"),
    )
    .unwrap();
    let mut manifest = directory.manifest();
    manifest["filters"][0]["program"]["entry"] = serde_json::json!("library_tent_vertical");
    manifest["filters"][0]["program"]["passes"][0]["entry"] =
        serde_json::json!("library_tent_horizontal");
    manifest["filters"][0]["program"]["passes"][1]["entry"] =
        serde_json::json!("library_tent_vertical");
    directory.set_manifest(manifest);
    service
        .load(&mut native, request(EffectInstallMode::Merge, true))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "ready", "{:?}", service.status);
    assert_eq!(native.session.engine().document().revision, revision);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    assert_eq!(image(&mut native), replaced);
    service.stop();
}
