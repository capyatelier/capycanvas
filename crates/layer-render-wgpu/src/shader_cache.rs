//! Disposable, app-private startup pipeline data. One generation avoids
//! accumulating obsolete shaders. The 64 MiB budget includes atomic-write files.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const BUDGET: u64 = 64 * 1024 * 1024;
const HEADER: u64 = 32;
const MAGIC: &[u8; 8] = b"CAPYPC01";

pub(super) struct Cache {
    active: Mutex<Option<(wgpu::PipelineCache, Store)>>,
}
impl Cache {
    pub fn open(device: &wgpu::Device, info: &wgpu::AdapterInfo, directory: &Path) -> Option<Self> {
        if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
            log("disabled: adapter has no pipeline cache support");
            return None;
        }
        let adapter = wgpu::util::pipeline_cache_key(info)?;
        let identity = format!(
            "{}:{adapter}:{}:{}",
            env!("CAPY_SHADER_GENERATION"),
            info.driver,
            info.driver_info
        );
        let key = identity.bytes().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
        let mut store = match Store::open(directory, key, BUDGET) {
            Ok(store) => store,
            Err(e) => {
                log(&format!("disabled: {e}"));
                return None;
            }
        };
        let data = store.load().unwrap_or_else(|e| {
            log(&format!("read skipped: {e}"));
            None
        });
        log(&format!(
            "load bytes={} generation={}",
            data.as_ref().map_or(0, Vec::len),
            env!("CAPY_SHADER_GENERATION")
        ));
        // SAFETY: Only bytes produced by get_data are written to this app-private
        // store. Its generation, length and checksum are checked before use.
        // wgpu additionally validates adapter/driver compatibility; incompatible
        // data falls back to an empty cache without affecting rendering.
        let pipeline = unsafe {
            device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                label: Some("Capy startup shaders"),
                data: data.as_deref(),
                fallback: true,
            })
        };
        Some(Self {
            active: Mutex::new(Some((pipeline, store))),
        })
    }
    pub fn pipeline(&self) -> Option<wgpu::PipelineCache> {
        self.active.lock().unwrap().as_ref().map(|(p, _)| p.clone())
    }
    pub fn finish(&self) {
        // Detach before serializing. Later runtime filter compilations cannot
        // grow this startup cache indefinitely. Existing pipelines remain live.
        let Some((pipeline, mut store)) = self.active.lock().unwrap().take() else {
            return;
        };
        let start = std::time::Instant::now();
        if let Some(data) = pipeline.get_data() {
            match store.save(&data) {
                Ok(true) => log(&format!(
                    "saved bytes={} elapsed_ms={:.2}",
                    data.len() + HEADER as usize,
                    start.elapsed().as_secs_f64() * 1000.
                )),
                Ok(false) => log(&format!(
                    "save skipped: bytes={} exceeds budget={BUDGET}",
                    data.len()
                )),
                Err(e) => log(&format!("save skipped: {e}")),
            }
        }
    }
}

struct Store {
    directory: PathBuf,
    key: u64,
    budget: u64,
    // Held through background save. A second renderer skips caching instead of
    // racing a retiring renderer's cleanup or write. OS releases it on death.
    _lock: File,
}
impl Store {
    fn open(directory: &Path, key: u64, budget: u64) -> io::Result<Self> {
        fs::create_dir_all(directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join("lock"))?;
        try_lock(&lock)?;
        let store = Self {
            directory: directory.into(),
            key,
            budget,
            _lock: lock,
        };
        store.clean()?;
        Ok(store)
    }
    fn path(&self) -> PathBuf {
        self.directory.join("startup.bin")
    }
    fn temporary(&self) -> PathBuf {
        self.directory.join("startup.tmp")
    }
    fn clean(&self) -> io::Result<()> {
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name();
            if name == "lock"
                || (name == "startup.bin"
                    && kind.is_file()
                    && entry.metadata()?.len() <= self.budget)
            {
                continue;
            }
            if kind.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }
    fn load(&mut self) -> io::Result<Option<Vec<u8>>> {
        let file = match File::open(self.path()) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        // Bound allocation even if the file changes after metadata inspection.
        let mut bytes = Vec::new();
        file.take(self.budget + 1).read_to_end(&mut bytes)?;
        let valid = bytes.len() as u64 <= self.budget
            && bytes.len() >= HEADER as usize
            && &bytes[..8] == MAGIC
            && u64::from_le_bytes(bytes[8..16].try_into().unwrap()) == self.key
            && u64::from_le_bytes(bytes[16..24].try_into().unwrap()) == bytes.len() as u64 - HEADER
            && u32::from_le_bytes(bytes[24..28].try_into().unwrap())
                == crc32fast::hash(&bytes[HEADER as usize..]);
        if !valid {
            fs::remove_file(self.path())?;
            log("discarded incompatible, oversized or corrupt data");
            return Ok(None);
        }
        bytes.drain(..HEADER as usize);
        Ok(Some(bytes))
    }
    fn save(&mut self, data: &[u8]) -> io::Result<bool> {
        self.clean()?;
        let size = data.len() as u64 + HEADER;
        if size > self.budget {
            return Ok(false);
        }
        let previous = fs::metadata(self.path()).map_or(0, |m| m.len());
        if previous + size > self.budget {
            // Losing an old cache on interruption is fine. Never exceed the
            // total budget just to preserve an atomic replacement's backup.
            fs::remove_file(self.path())?;
        }
        let mut header = Vec::from(*MAGIC);
        header.extend(self.key.to_le_bytes());
        header.extend((data.len() as u64).to_le_bytes());
        header.extend(crc32fast::hash(data).to_le_bytes());
        header.extend([0; 4]);
        let result = (|| {
            let mut file = File::create(self.temporary())?;
            file.write_all(&header)?;
            file.write_all(data)?;
            file.sync_all()?;
            fs::rename(self.temporary(), self.path())?;
            Ok(true)
        })();
        if result.is_err() {
            let _ = fs::remove_file(self.temporary());
        }
        result
    }
}

fn try_lock(file: &File) -> io::Result<()> {
    // This toolchain's std::fs locking is unsupported on Android. bionic has
    // flock on every Android API level supported by this application.
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: The File owns this live descriptor for the lifetime of Store.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(target_os = "android"))]
    file.try_lock().map_err(|e| match e {
        fs::TryLockError::WouldBlock => io::ErrorKind::WouldBlock.into(),
        fs::TryLockError::Error(e) => e,
    })
}

fn log(message: &str) {
    #[cfg(target_os = "android")]
    if let Ok(message) = std::ffi::CString::new(message) {
        #[link(name = "log")]
        unsafe extern "C" {
            fn __android_log_write(
                priority: i32,
                tag: *const std::ffi::c_char,
                text: *const std::ffi::c_char,
            ) -> i32;
        }
        unsafe {
            __android_log_write(4, c"CapyShaderCache".as_ptr(), message.as_ptr());
        }
    }
    #[cfg(not(target_os = "android"))]
    eprintln!("CapyShaderCache: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            Self(std::env::temp_dir().join(format!(
                "capy-cache-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn roundtrip_rejects_damage_and_obsolete_generation() {
        let temp = Temp::new();
        let mut store = Store::open(&temp.0, 1, 1024).unwrap();
        assert!(store.save(b"driver data").unwrap());
        assert_eq!(store.load().unwrap().unwrap(), b"driver data");
        let mut corrupt = fs::read(store.path()).unwrap();
        *corrupt.last_mut().unwrap() ^= 1;
        fs::write(store.path(), corrupt).unwrap();
        assert!(store.load().unwrap().is_none());
        store.save(b"driver data").unwrap();
        drop(store);
        let mut newer = Store::open(&temp.0, 2, 1024).unwrap();
        assert!(newer.load().unwrap().is_none());
        assert!(!newer.path().exists());
    }
    #[test]
    fn budget_covers_replacement_stale_files_and_oversized_data() {
        let temp = Temp::new();
        let mut store = Store::open(&temp.0, 1, 128).unwrap();
        assert!(store.save(&[1; 96]).unwrap());
        assert!(store.save(&[2; 96]).unwrap());
        assert!(!store.save(&[3; 97]).unwrap());
        assert_eq!(store.load().unwrap().unwrap(), [2; 96]);
        fs::write(store.temporary(), [0; 128]).unwrap();
        fs::write(temp.0.join("old-driver.bin"), [0; 256]).unwrap();
        drop(store);
        let mut store = Store::open(&temp.0, 1, 128).unwrap();
        let bytes: u64 = fs::read_dir(&temp.0)
            .unwrap()
            .map(|e| e.unwrap().metadata().unwrap().len())
            .sum();
        assert_eq!(bytes, 128);
        assert!(store.load().unwrap().is_some());
        fs::write(store.path(), [0; 129]).unwrap();
        assert!(store.load().unwrap().is_none());
        assert!(!store.path().exists());
    }
    #[test]
    fn concurrent_renderer_cannot_clean_or_overwrite_active_store() {
        let temp = Temp::new();
        let mut store = Store::open(&temp.0, 1, 128).unwrap();
        store.save(b"first renderer").unwrap();
        assert!(Store::open(&temp.0, 2, 128).is_err());
        assert_eq!(store.load().unwrap().unwrap(), b"first renderer");
        drop(store);
        assert!(Store::open(&temp.0, 2, 128).is_ok());
    }

    #[test]
    fn cached_gpu_startup_matches_eager_pixels_and_waits_for_catalog() {
        use crate::{PipelineDevice, WgpuRasterizer};
        use layer_render::CanvasRenderer;
        let temp = Temp::new();
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        assert!(adapter.features().contains(wgpu::Features::PIPELINE_CACHE));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::PIPELINE_CACHE,
            ..Default::default()
        }))
        .unwrap();
        let mut reference =
            WgpuRasterizer::from_wgpu(adapter.clone(), device.clone(), queue.clone()).unwrap();
        let doc = layer_core::Document::new("cached", 64, 64);
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        let dabs = [layer_render::Dab {
            center: layer_core::Point { x: 32., y: 32. },
            radii: [16.; 2],
            rotation: [1., 0.],
            motion: [0.; 2],
            color_rgba_linear: [0.8, 0.1, 0.3, 1.],
            flow: 1.,
            hardness: 1.,
            texture_sign: [1.; 2],
            material: [0.; 4],
        }];
        let batches = [layer_render::DabBatch {
            stroke_id: layer_core::StrokeId(1),
            layer_id: doc.active_layer,
            kind: layer_render::DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: layer_render::DabStyle {
                alpha_locked: false,
                selection: None,
                tip: brush.tip.clone(),
                mode: layer_render::DabMode::Paint,
                execution: brush.execution_class(),
                grain: brush.grain.clone(),
                dual: brush.dual.clone(),
                rendering: brush.rendering,
                wet_mix: brush.wet_mix,
                transport: brush.transport.clone(),
                deform: brush.deform,
            },
            damage: layer_core::Rect {
                min: layer_core::Point { x: 16., y: 16. },
                max: layer_core::Point { x: 48., y: 48. },
            },
        }];
        let packet = layer_render::FramePacket {
            time_seconds: 0.,
            view: layer_render::ViewState {
                width_px: 64,
                height_px: 64,
                document_to_surface: [1., 0., 0., 1., 0., 0.],
                background_rgba_linear: [1.; 4],
            },
            document_extent: [64; 2],
            layers: &doc.layers,
            dabs: &dabs,
            dab_batches: &batches,
            reset_layers: true,
            composite_all: true,
        };
        reference.submit(packet).unwrap();
        let expected = reference.readback_srgb_rgba8().unwrap();
        assert_ne!(
            &expected[..4],
            &expected[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4],
            "test must paint visible pixels"
        );
        for _ in 0..2 {
            let mut renderer = WgpuRasterizer::from_wgpu_staged_cached(
                adapter.clone(),
                device.clone(),
                queue.clone(),
                &temp.0,
            )
            .unwrap();
            renderer.prepare_startup(&doc, &brush).unwrap();
            assert!(!renderer.poll_startup().unwrap().complete);
            renderer.finish_startup_cache();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !renderer.poll_startup().unwrap().complete {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            renderer.submit(packet).unwrap();
            assert_eq!(renderer.readback_srgb_rgba8().unwrap(), expected);
            assert!(fs::metadata(temp.0.join("startup.bin")).unwrap().len() > HEADER);
        }
        // A new device wrapper reads the saved driver blob, then releases it.
        let cached = PipelineDevice::cached(device, &adapter, &temp.0);
        cached.finish_cache();
        assert!(
            Store::open(&temp.0, 1, BUDGET).is_ok(),
            "save must release the store lock and driver cache"
        );
    }
}
