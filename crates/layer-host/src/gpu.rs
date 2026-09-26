//! One GPU context, renderer factory and device-failure watch for native hosts.
use layer_core::color::{DocumentColor, RgbSpace};
use layer_render_wgpu::WgpuRasterizer;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

/// UI preview colors: mapped through the SDR rendition, or tagged in a display space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum UiColor {
    #[default]
    Mapped,
    Tagged(RgbSpace),
}
impl UiColor {
    pub fn preview_space(self) -> RgbSpace {
        match self {
            Self::Mapped => RgbSpace::Srgb,
            Self::Tagged(space) => space,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RendererOptions {
    pub cache: Option<PathBuf>,
    pub ui_color: UiColor,
}

#[derive(Clone)]
pub struct GpuContext {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}
impl GpuContext {
    pub fn of(gpu: &WgpuRasterizer) -> Self {
        Self {
            adapter: gpu.adapter().clone(),
            device: gpu.device().clone(),
            queue: gpu.queue().clone(),
        }
    }
    /// Secondary renderers (tabs, open, color candidates) pass `finish_cache = true`;
    /// the window renderer passes false and calls `finish_startup_cache` after its catalog.
    pub fn rasterizer(
        &self,
        color: DocumentColor,
        options: &RendererOptions,
        finish_cache: bool,
    ) -> Result<WgpuRasterizer, String> {
        let (adapter, device, queue) = (
            self.adapter.clone(),
            self.device.clone(),
            self.queue.clone(),
        );
        let mut gpu = match &options.cache {
            Some(cache) => {
                WgpuRasterizer::from_wgpu_native_staged_cached(adapter, device, queue, cache, color)
            }
            None => WgpuRasterizer::from_wgpu_native_staged(adapter, device, queue, color),
        }
        .map_err(|e| e.to_string())?;
        gpu.configure_ui_previews(options.ui_color.preview_space())
            .map_err(|e| e.to_string())?;
        if finish_cache {
            gpu.finish_startup_cache();
        }
        Ok(gpu)
    }
}

#[derive(Clone, Default)]
pub struct DeviceWatch {
    lost: Arc<OnceLock<String>>,
    error: Arc<OnceLock<String>>,
}
impl DeviceWatch {
    pub fn observe(device: &wgpu::Device) -> Self {
        let watch = Self::default();
        let lost = watch.lost.clone();
        device.set_device_lost_callback(move |reason, message| {
            lost.get_or_init(|| format!("Canvas GPU stopped ({reason:?}): {message}"));
        });
        let error = watch.error.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            error.get_or_init(|| e.to_string());
        }));
        watch
    }
    pub fn lost(&self) -> Option<&str> {
        self.lost.get().map(String::as_str)
    }
    pub fn error(&self) -> Option<&str> {
        self.error.get().map(String::as_str)
    }
    pub fn failure(&self) -> Option<String> {
        self.lost().or(self.error()).map(str::to_owned)
    }
    pub fn report_lost(&self, message: impl Into<String>) {
        self.lost.get_or_init(|| message.into());
    }
    pub fn report_error(&self, message: impl Into<String>) {
        self.error.get_or_init(|| message.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_render::CanvasRenderer;

    #[test]
    fn renderer_factory_admits_input_and_follows_host_ui_color() {
        let color = DocumentColor::default();
        let headless = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut host = crate::NativeHost::new(layer_ui::Platform::Mac).unwrap();
        assert_eq!(
            host.renderer_options(None).ui_color.preview_space(),
            RgbSpace::Srgb
        );
        host.ui_color = UiColor::Tagged(RgbSpace::DisplayP3);
        let options = host.renderer_options(None);
        assert_eq!(options.ui_color.preview_space(), RgbSpace::DisplayP3);
        let gpu = GpuContext::of(&headless)
            .rasterizer(color, &options, true)
            .unwrap();
        assert_eq!(gpu.document_color(), color);
        assert!(gpu.device() == headless.device());
        gpu.shader_input();
        assert!(gpu.shader_wait_ms() > 0.);
    }
}
