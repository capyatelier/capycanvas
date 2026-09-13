//! Actual process-owned device removal at document worker/adoption boundaries.
use super::gpu_tests::{image, invoke, png_pixels, request};
use super::*;
use crate::device::DeviceState;
use layer_ui::{CommandId, Platform};
use std::sync::{atomic::AtomicU64, mpsc};
use windows::{Win32::Graphics::Direct3D12::ID3D12Device5, core::Interface};

fn renderer() -> (Renderer, Arc<DeviceState>) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::DX12;
    descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
    let instance = wgpu::Instance::new(descriptor);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(adapter.get_info().backend, wgpu::Backend::Dx12);
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            assert!(
                Instant::now() < deadline,
                "Removed hardware device is still retained"
            );
            drop(adapter);
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        let state = DeviceState::observe(&device);
        #[allow(deprecated)]
        let gpu = WgpuRasterizer::from_wgpu(adapter, device, queue).unwrap();
        assert!(!state.is_lost(Some(gpu.device())));
        state.check().unwrap();
        return (Renderer(Some(gpu)), state);
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    host: NativeHost,
    service: DocumentService,
    done: mpsc::Receiver<()>,
    device: Arc<DeviceState>,
    directory: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let (renderer, device) = renderer();
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.session = UiSession::from_project(
            renderer,
            layer_ui::new_drawing(64, 48).unwrap(),
            None,
            [64, 48],
        )
        .unwrap();
        host.session.set_platform(Platform::Windows);
        host.session.set_document_replacement(true);
        host.resize(64, 48, 1.).unwrap();
        let (wake, done) = mpsc::channel();
        let service = DocumentService::open(move || {
            let _ = wake.send(());
        })
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "capy-document-removal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        Self {
            host,
            service,
            done,
            device,
            directory,
        }
    }
    fn path(&self, name: &str) -> String {
        self.directory.join(name).to_str().unwrap().into()
    }
    fn dispatch(&mut self, action: DocumentAction) {
        self.service.dispatch(&mut self.host, action).unwrap();
    }
    fn wait(&self) {
        self.done.recv_timeout(Duration::from_secs(60)).unwrap();
    }
    fn poll(&mut self) {
        self.service.poll(&mut self.host).unwrap();
    }
    fn finish(&mut self) {
        self.wait();
        self.poll();
    }
    fn remove_device(&mut self) {
        let gpu = self.host.session.engine().backend().0.as_ref().unwrap();
        {
            let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }.unwrap();
            let device: ID3D12Device5 = native.raw_device().cast().unwrap();
            unsafe { device.RemoveDevice() };
        }
        let _ = gpu.device().poll(wgpu::PollType::Poll);
        assert!(self.device.is_lost(Some(gpu.device())));
    }
    fn retire_renderer(&mut self) {
        drop(self.host.session.renderer_mut().0.take());
        self.host.startup = Default::default();
    }
    fn restore_renderer(&mut self) {
        let (renderer, device) = renderer();
        let revision = self.host.session.state().revision;
        let (old, change) = self.host.session.replace_renderer(renderer).unwrap();
        self.host.apply_change(revision, change);
        drop(old);
        self.device = device;
        self.host.startup = Default::default();
    }
    fn import(&mut self, path: &str) {
        self.dispatch(DocumentAction::RequestImport);
        let id = self.service.import_request().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.dispatch(DocumentAction::ImportImage {
            id,
            path: Some(path.into()),
        });
    }
    fn prepare(&mut self, open: Option<&str>) {
        invoke(
            &mut self.host,
            if open.is_some() {
                CommandId::OpenDocument
            } else {
                CommandId::NewDocument
            },
        );
        let (id, epoch, revision) = request(&self.host);
        self.dispatch(match open {
            Some(path) => DocumentAction::Open {
                id,
                epoch,
                revision,
                path: path.into(),
            },
            None => DocumentAction::New {
                id,
                epoch,
                revision,
                width: 96,
                height: 72,
            },
        });
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.service.stop_worker().unwrap();
        // Only ordinary files in this test's uniquely reserved directory.
        for entry in std::fs::read_dir(&self.directory).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            std::fs::remove_file(entry.path()).unwrap();
        }
        std::fs::remove_dir(&self.directory).unwrap();
    }
}

#[test]
#[ignore = "Removes process-owned D3D12 hardware devices; run this module alone"]
fn decoded_import_survives_removal_before_adoption() {
    let mut f = Fixture::new();
    let source = f.path("source.png");
    let rgba = [210u8, 45, 83, 180].repeat(12);
    let mut encoder = png::Encoder::new(File::create(&source).unwrap(), 4, 3);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&rgba)
        .unwrap();
    f.import(&source);
    f.finish();
    let expected = image(&mut f.host).bytes;
    invoke(&mut f.host, CommandId::Undo);
    let baseline = image(&mut f.host).bytes;
    assert_ne!(expected, baseline);
    let document = f.host.session.engine().document().clone();
    f.import(&source);
    f.wait();
    std::fs::remove_file(&source).unwrap(); // Recovery must use the accepted decode.
    f.remove_device();
    f.poll();
    assert!(f.service.importing(), "Do not upload to a removed renderer");
    assert_eq!(f.host.session.engine().document(), &document);
    f.retire_renderer();
    f.poll();
    assert!(
        f.service.importing(),
        "Retain decoded bytes during reconstruction"
    );
    assert!(f.host.error.is_none());
    f.restore_renderer();
    f.poll();
    assert!(!f.service.importing());
    assert!(f.host.error.is_none(), "{:?}", f.host.error);
    assert_eq!(image(&mut f.host).bytes, expected);
    invoke(&mut f.host, CommandId::Undo);
    assert_eq!(image(&mut f.host).bytes, baseline);
    invoke(&mut f.host, CommandId::Redo);
    assert_eq!(image(&mut f.host).bytes, expected);
    f.device.check().unwrap();
}

#[test]
#[ignore = "Removes process-owned D3D12 hardware devices; run this module alone"]
fn new_open_and_save_keep_the_authoritative_document_across_removal() {
    let mut f = Fixture::new();
    f.host
        .session
        .import_layer_asset(
            "Embedded",
            ProjectAsset {
                extent: [2, 2],
                format: layer_core::ProjectAssetFormat::Rgba8Srgb,
                bytes: Arc::from([30u8, 90, 210, 180].repeat(4)),
            },
        )
        .unwrap();
    invoke(&mut f.host, CommandId::AddLayer);
    let original = f.host.session.engine().document().clone();
    let expected = image(&mut f.host).bytes;
    let path = f.path("drawing.capy");
    invoke(&mut f.host, CommandId::SaveDocument);
    let (id, _, _) = request(&f.host);
    f.dispatch(DocumentAction::Save {
        id,
        path: path.clone(),
    });
    f.remove_device();
    f.retire_renderer();
    f.finish();
    assert!(!f.host.session.state().document_file.modified);
    let saved = Project::read(File::open(&path).unwrap(), Default::default()).unwrap();
    assert_eq!(saved.document.layers, original.layers);
    f.restore_renderer();
    assert_eq!(image(&mut f.host).bytes, expected);
    // Check both a completed candidate awaiting adoption and removal immediately
    // after submission, while the real worker owns preparation.
    for completed in [true, false] {
        for open in [None, Some(path.as_str())] {
            let original = f.host.session.engine().document().clone();
            let epoch = f.host.session.state().document_file.epoch;
            let pixels = image(&mut f.host).bytes;
            f.prepare(open);
            assert!(f.service.active.is_some());
            if completed {
                f.wait();
            }
            f.remove_device();
            f.retire_renderer();
            if !completed {
                f.wait();
            }
            f.poll();
            assert!(!f.host.session.state().document_file.busy);
            assert_eq!(f.host.session.state().document_file.epoch, epoch);
            assert_eq!(f.host.session.engine().document(), &original);
            let error = f.host.session.state().host_error.as_ref().unwrap();
            assert_ne!(
                error, "Document worker failed",
                "Device loss must not unwind document preparation"
            );
            f.restore_renderer();
            assert_eq!(image(&mut f.host).bytes, pixels);
            f.prepare(open);
            f.finish();
            assert!(f.host.session.state().host_error.is_none());
            assert_eq!(f.host.session.state().document_file.epoch, epoch + 1);
            let document = f.host.session.engine().document();
            assert_eq!(
                [document.width, document.height],
                if open.is_some() { [64, 48] } else { [96, 72] }
            );
            assert_eq!(
                f.host.session.state().document_file.location.is_some(),
                open.is_some()
            );
            image(&mut f.host);
        }
    }
    f.device.check().unwrap();
}

#[test]
#[ignore = "Removes process-owned D3D12 hardware devices; run this module alone"]
fn captured_export_after_removal_preserves_destination_and_allows_retry() {
    let mut f = Fixture::new();
    invoke(&mut f.host, CommandId::AddLayer);
    let original = f.host.session.engine().document().clone();
    let expected = image(&mut f.host).bytes;
    let path = f.path("drawing.png");
    std::fs::write(&path, b"Existing destination").unwrap();
    invoke(&mut f.host, CommandId::ExportDocument);
    let (id, _, _) = request(&f.host);
    f.dispatch(DocumentAction::Export {
        id,
        path: path.clone(),
    });
    // Isolate the capture/worker boundary: remove the actual device after the
    // owner obtains its ticket and before the worker receives that ticket.
    let readback = f
        .host
        .session
        .renderer_mut()
        .0
        .as_mut()
        .unwrap()
        .begin_export_readback(u64::from(id))
        .unwrap();
    let destination = f.service.export.take().unwrap();
    f.remove_device();
    f.retire_renderer();
    f.service.worker.submit(Job::Export {
        readback,
        path: destination,
    });
    f.finish();
    assert!(!f.host.session.state().document_file.busy);
    assert!(f.host.session.state().document_file.modified);
    assert_eq!(f.host.session.engine().document(), &original);
    if f.host.session.state().host_error.is_some() {
        assert_eq!(std::fs::read(&path).unwrap(), b"Existing destination");
    } else {
        // A ticket already mapped before removal may still finish losslessly.
        assert_eq!(png_pixels(std::path::Path::new(&path)).bytes, expected);
    }
    f.restore_renderer();
    assert_eq!(image(&mut f.host).bytes, expected);
    super::gpu_tests::capture_export(&mut f.service, &mut f.host, std::path::Path::new(&path));
    f.finish();
    assert!(f.host.session.state().host_error.is_none());
    assert_eq!(png_pixels(std::path::Path::new(&path)).bytes, expected);
    assert!(f.host.session.state().document_file.modified);
    f.device.check().unwrap();
}

#[test]
#[ignore = "Removes process-owned D3D12 hardware devices; run this module alone"]
fn viewport_upload_returns_device_failure_without_unwinding() {
    let mut f = Fixture::new();
    let expected = image(&mut f.host).bytes;
    let gpu = f.host.session.engine().backend().0.as_ref().unwrap();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("Device-loss viewport target"),
        size: wgpu::Extent3d {
            width: 64,
            height: 48,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut presenter = layer_render_wgpu::ViewportPresenter::new(gpu.device(), format);
    f.remove_device();
    let error = presenter
        .present(
            f.host.session.engine().backend().0.as_ref().unwrap(),
            &texture.create_view(&Default::default()),
            f.host.session.state().camera.view(),
            [0.; 4],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        layer_render_wgpu::GpuRasterError::MapFailed(_)
    ));
    drop(presenter);
    drop(texture);
    f.retire_renderer();
    f.restore_renderer();
    assert_eq!(image(&mut f.host).bytes, expected);
    f.device.check().unwrap();
}
