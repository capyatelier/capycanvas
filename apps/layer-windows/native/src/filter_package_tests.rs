use crate::test_support::*;
use super::*;
use crate::test_support::TempDir;
use std::{
    fs,
    time::{Duration, Instant},
};

fn copy_example(directory: &TempDir) {
    let example =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../examples/filters/tent-blur");
    for name in ["manifest.json", "prepare.wgsl", "tent.wgsl"] {
        fs::copy(example.join(name), directory.path.join(name)).unwrap();
    }
}
fn read_manifest(directory: &TempDir) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(directory.path.join("manifest.json")).unwrap())
        .unwrap()
}
fn write_manifest(directory: &TempDir, value: serde_json::Value) {
    fs::write(
        directory.path.join("manifest.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
}
fn read_package(directory: &TempDir) -> Result<Package, String> {
    read_directory(&directory.path, EffectInstallMode::Merge)
}
fn error(directory: &TempDir) -> String {
    match read_package(directory) {
        Ok(_) => panic!("Invalid package was accepted"),
        Err(error) => error,
    }
}
#[test]
fn file_transport_resolves_shared_render_and_preparation_modules() {
    let directory = TempDir::new();
    copy_example(&directory);
    let package = read_package(&directory).unwrap();
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
        read_package(&directory).unwrap().modules["tent.wgsl"].as_ref(),
        changed
    );
}
#[test]
fn property_wire_keeps_section_identity_and_choice_indices_with_equal_labels() {
    let mut definition = layer_core::bundled_effect_catalog().get("curves").unwrap().clone();
    let program = Arc::make_mut(&mut definition.program);
    let parameters = Arc::make_mut(&mut program.parameters);
    parameters[0].section = Some(layer_core::ResourceLabel::Message { message: "common-cancel".into() });
    parameters[1].section = Some("Cancel".into());
    parameters[1].page = parameters[0].page.clone();
    let domain = parameters.iter_mut().find(|parameter| parameter.key.as_ref() == "domain").unwrap();
    let layer_core::EffectParameterKind::Choice { options } = &mut domain.kind else { panic!() };
    for option in Arc::make_mut(options) {
        *option = layer_core::EffectOption::Labeled { value: option.value().into(), label: "Same".into() };
    }
    let mut effect = definition.preview().unwrap();
    effect.set("domain", layer_core::EffectValue::Choice(1)).unwrap();
    assert_eq!(effect.choice("domain"), Some("Log HDR"));
    let mut project = layer_ui::new_drawing(64, 48,
        &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    insert_effect(&mut project,"Literal filter name",effect);
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    native.session = layer_ui::UiSession::from_project(layer_host::Renderer(None), project,
        None, [64, 48], layer_ui::Platform::Windows).unwrap();
    let wire = serde_json::to_value(&native.session.state().layer_properties).unwrap();
    let controls = wire["controls"].as_array().unwrap();
    let first = controls.iter().find(|control| control["key"] == "curve_0").unwrap();
    let second = controls.iter().find(|control| control["key"] == "curve_1").unwrap();
    assert_eq!(wire["pages"].as_array().unwrap().iter().map(|page| page["id"].as_str().unwrap()).collect::<Vec<_>>(), ["rgb", "green", "blue"]);
    assert_eq!((&wire["page"], &first["page"], &second["page"]), (&serde_json::json!("rgb"), &serde_json::json!("rgb"), &serde_json::json!("rgb")));
    assert_eq!(first["section"], second["section"]);
    assert_ne!(first["section_id"], second["section_id"]);
    assert_eq!(first["section_id"], serde_json::json!({"message":"common-cancel"}));
    assert_eq!(second["section_id"], "Cancel");
    let domain = controls.iter().find(|control| control["key"] == "domain").unwrap();
    assert_eq!(domain["value"]["value"], 1);
    assert!(domain["kind"]["options"].as_array().unwrap().iter().all(|label| label == "Same"));
    let round_trip: serde_json::Value = serde_json::from_str(&wire.to_string()).unwrap();
    assert_eq!(wire, round_trip);
}

#[test]
fn manifest_paths_are_rejected_before_any_module_read() {
    let directory = TempDir::new();
    copy_example(&directory);
    let original = read_manifest(&directory);
    for name in [
        "../outside.wgsl",
        "C:/outside.wgsl",
        "nested/module.wgsl",
        ".hidden.wgsl",
        "other.wgsl:stream",
    ] {
        let mut manifest = original.clone();
        manifest["filters"][0]["program"]["wgsl"] = serde_json::json!([name]);
        write_manifest(&directory, manifest);
        assert!(error(&directory).contains("module filename"));
    }
}
#[test]
fn missing_invalid_and_oversized_resources_preserve_source_files() {
    let directory = TempDir::new();
    copy_example(&directory);
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
    let directory = TempDir::new();
    copy_example(&directory);
    let mut manifest = read_manifest(&directory);
    let modules: Vec<_> = (0..17).map(|i| format!("module{i}.wgsl")).collect();
    manifest["filters"][0]["program"]["wgsl"] = serde_json::json!(modules);
    write_manifest(&directory, manifest);
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
    let directory = TempDir::new();
    copy_example(&directory);
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let before = native.session.state().adjustments.clone();
    let mut service = FilterService::new(|| {});
    let request = || Request {
        directory: Some(directory.path.clone()),
        mode: EffectInstallMode::Add,
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
    let directory = TempDir::new();
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = FilterService::new(|| {});
    let request = || Request {
        directory: Some(directory.path.clone()),
        mode: EffectInstallMode::Merge,
    };
    service.load(&mut native, request()).unwrap();
    finish_read(&mut service, &mut native);
    assert!(!service.status.pending);
    assert_eq!(service.status.phase, "failed");
    assert!(service.status.error.is_some());
    assert_eq!(native.session.state().filter_catalog_revision, 0);
    copy_example(&directory);
    service.load(&mut native, request()).unwrap();
    finish_read(&mut service, &mut native);
    assert_eq!(service.status.phase, "waiting_for_canvas");
    assert_eq!(service.status.request_id, 2);
    assert!(service.status.error.is_none());
    service.stop();
}

#[test]
fn delayed_catalog_import_survives_document_replacement() {
    let directory = TempDir::new();
    copy_example(&directory);
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    let mut service = FilterService::new(|| {});
    service
        .load(
            &mut native,
            Request {
                directory: Some(directory.path.clone()),
                mode: EffectInstallMode::Merge,
            },
        )
        .unwrap();
    finish_read(&mut service, &mut native);
    let mut replacement = layer_ui::UiSession::from_project(
        layer_host::Renderer(None),
        layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
        None,
        [32, 24],
        layer_ui::Platform::Windows,
    )
    .unwrap();
    let epoch = native.session.state().document_file.epoch;
    replacement.inherit_window_state(&native.session).unwrap();
    drop(std::mem::replace(&mut native.session, replacement));
    assert_ne!(native.session.state().document_file.epoch, epoch);
    service.poll(&mut native);
    assert_eq!(service.status.phase,"waiting_for_canvas");
    assert_eq!(native.session.state().filter_catalog_revision, 0);
    service.stop();
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Requires an explicitly selected hardware D3D12 adapter"]
fn d3d12_file_packages_update_catalog_without_rewriting_live_filters() {
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
    assert!(adapter.get_info().device_type != wgpu::DeviceType::Cpu || layer_render_wgpu::software_adapter_tests());
    assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Windows runtime filter correctness"),
        required_features: adapter.features()
            & (wgpu::Features::FLOAT32_FILTERABLE
                | wgpu::Features::FLOAT32_BLENDABLE
                | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
        required_limits: limits,
        ..Default::default()
    }))
    .unwrap();
    let gpu = layer_render_wgpu::WgpuRasterizer::from_wgpu_native_staged(
        adapter,
        device,
        queue,
        Default::default(),
    )
    .unwrap();
    let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
    native.session = layer_ui::UiSession::from_project(
        layer_host::Renderer(Some(gpu.into())),
        layer_ui::new_drawing(64, 48, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),
        None,
        [64, 48],
        layer_ui::Platform::Windows,
    )
    .unwrap();
    native.startup = Default::default();
    let source = layer_core::color::source::rgba8_source([4, 3], |_, _| [230, 71, 42, 255]);
    native
        .session
        .import_layer_source("Synthetic color", std::sync::Arc::unwrap_or_clone(source))
        .unwrap();
    native.dirty = true;
    let directory = TempDir::new();
    copy_example(&directory);
    let shader = fs::read_to_string(directory.path.join("tent.wgsl")).unwrap();
    let mut service = FilterService::new(|| {});
    let request = |mode| Request {
        directory: Some(directory.path.clone()),
        mode,
    };
    service
        .load(&mut native, request(EffectInstallMode::Add))
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
    let active = native.session.engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap();
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
        .load(&mut native, request(EffectInstallMode::Add))
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
        .load(&mut native, request(EffectInstallMode::Replace))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "ready", "{:?}", service.status);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    let replaced = image(&mut native);
    assert_eq!(replaced,before);
    let revision = native.session.engine().document().revision;
    let catalog = native.session.state().filter_catalog_revision;
    fs::write(directory.path.join("tent.wgsl"), "not valid WGSL").unwrap();
    service
        .load(&mut native, request(EffectInstallMode::Replace))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "failed");
    assert_eq!(native.session.engine().document().revision, revision);
    assert_eq!(native.session.state().filter_catalog_revision, catalog);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    assert_eq!(image(&mut native), replaced);

    fs::write(directory.path.join("tent.wgsl"), &shader).unwrap();
    service.load(&mut native, request(EffectInstallMode::Merge)).unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase,"ready");
    assert_eq!(native.session.engine().document().revision,revision);
    assert_eq!(image(&mut native),before);

    fs::write(
        directory.path.join("tent.wgsl"),
        shader.replace("tent_", "library_tent_"),
    )
    .unwrap();
    let mut manifest = read_manifest(&directory);
    manifest["filters"][0]["program"]["entry"] = serde_json::json!("library_tent_vertical");
    manifest["filters"][0]["program"]["passes"][0]["entry"] =
        serde_json::json!("library_tent_horizontal");
    manifest["filters"][0]["program"]["passes"][1]["entry"] =
        serde_json::json!("library_tent_vertical");
    write_manifest(&directory, manifest);
    service
        .load(&mut native, request(EffectInstallMode::Merge))
        .unwrap();
    finish(&mut service, &mut native);
    assert_eq!(service.status.phase, "ready", "{:?}", service.status);
    assert_eq!(native.session.engine().document().revision, revision);
    assert_eq!(radius(&native), layer_core::EffectValue::Number(7.));
    assert_eq!(image(&mut native), replaced);
    service.stop();
}

#[cfg(target_os = "windows")]
fn finish(service: &mut FilterService, native: &mut NativeHost) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while service.status.pending {
        service.poll(native);
        native.prepare_canvas_frame(0, 0, true).unwrap();
        assert!(Instant::now() < deadline, "Package loading did not settle");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[cfg(target_os = "windows")]
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
    let renderer = native.session.renderer_mut().0.as_mut().unwrap();
    renderer.readback_srgb_rgba8().unwrap()
}
#[cfg(target_os = "windows")]
fn radius(native: &NativeHost) -> layer_core::EffectValue {
    let doc=native.session.engine().document();let owner=doc.working.occurrence.unwrap();doc.scene().effect(owner).unwrap().value("radius").unwrap().clone()
}

#[cfg(target_os = "windows")]
#[path = "filter_recovery_tests.rs"]
mod recovery_tests;

#[test]
fn suspension_finishes_pending_reads_and_rejects_new_loads() {
    for acquired in [false, true] {
        let directory = TempDir::new();
        copy_example(&directory);
        let mut native = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut service = FilterService::new(|| {});
        let request = || Request {
            directory: Some(directory.path.clone()),
            mode: EffectInstallMode::Merge,
        };
        service.load(&mut native, request()).unwrap();
        if acquired {
            finish_read(&mut service, &mut native);
        } else {
            service.poll(&mut native); // Start the real bounded file worker.
        }
        native.suspend_renderer().unwrap();
        service.poll(&mut native);
        assert!(!service.status.pending);
        assert_eq!(service.status.phase, "failed");
        assert!(
            service
                .status
                .error
                .as_ref()
                .unwrap()
                .contains("unavailable")
        );
        assert!(service.acquired.is_none());
        assert!(service.validating.is_none());
        assert!(service.load(&mut native, request()).is_err());
        // A late file completion cannot revive the failed request.
        let deadline = Instant::now() + Duration::from_secs(5);
        while service.task.busy() {
            service.poll(&mut native);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(service.status.phase, "failed");
        assert!(service.acquired.is_none());
        assert_eq!(native.session.state().filter_catalog_revision, 0);
        service.stop();
    }
}
