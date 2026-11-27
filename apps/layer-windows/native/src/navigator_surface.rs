#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub struct CapyNavigatorViewport {
    pub image: [f32; 2],
    pub maximum: [f32; 2],
    pub nominal: [f32; 2],
    pub scale: f32,
    pub moving: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct NavigatorSizing {
    pub(crate) buffer: [u32; 2],
    pub(crate) capacity: [u32; 2],
    pub(crate) nominal: [f32; 2],
}

impl NavigatorSizing {
    pub(crate) fn settle(&mut self, viewport: CapyNavigatorViewport, limit: u32) -> Result<bool, String> {
        if !viewport.image.into_iter().chain(viewport.maximum).chain(viewport.nominal).chain([viewport.scale])
            .all(|v| v.is_finite() && v > 0.)
            || viewport.image.into_iter().zip(viewport.maximum).any(|(value, maximum)| value > maximum)
            || viewport.maximum.into_iter().any(|value| value.ceil() > limit as f32) {
            return Err("Invalid Navigator surface size".into());
        }
        if self.nominal == [0.; 2] { self.nominal = viewport.nominal; }
        if self.nominal != viewport.nominal { return Err("Navigator nominal size changed".into()); }
        if viewport.moving != 0 { return Ok(false); }
        let buffer = viewport.image.map(|value| value.ceil() as u32);
        let capacity = std::array::from_fn(|i| {
            if buffer[i] <= self.capacity[i] { return self.capacity[i]; }
            let headroom = (u64::from(buffer[i]) * 3).div_ceil(2).div_ceil(64) * 64;
            headroom.min(viewport.maximum[i].ceil() as u64).min(u64::from(limit)) as u32
        });
        let changed = buffer != self.buffer || capacity != self.capacity;
        self.buffer = buffer;
        self.capacity = capacity;
        Ok(changed)
    }

    pub(crate) fn matrix(&self) -> [f32; 2] {
        std::array::from_fn(|i| self.nominal[i] / self.capacity[i].max(1) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewport(image: [f32; 2], moving: bool) -> CapyNavigatorViewport {
        CapyNavigatorViewport { image, maximum: [784., 440.], nominal: [256., 192.], scale: 1., moving: u32::from(moving) }
    }
    #[test]
    fn motion_retains_resolution_and_never_allocates_or_grows() {
        let mut sizing = NavigatorSizing::default();
        assert!(!sizing.settle(viewport([200., 150.], true), 4096).unwrap());
        assert_eq!(sizing.capacity, [0, 0]);
        assert!(sizing.settle(viewport([200., 150.], false), 4096).unwrap());
        let retained = sizing;
        for image in [[330., 240.], [100., 80.], [784., 440.]] {
            assert!(!sizing.settle(viewport(image, true), 4096).unwrap());
            assert_eq!(sizing, retained);
        }
        assert!(sizing.settle(viewport([784., 440.], false), 4096).unwrap());
        assert_eq!(sizing.capacity, [784, 440]);
        assert!(!sizing.settle(viewport([784., 440.], false), 4096).unwrap());
    }
    #[test]
    fn bounded_headroom_retains_capacity_while_refining_the_source() {
        let mut sizing = NavigatorSizing::default();
        sizing.settle(viewport([200., 150.], false), 4096).unwrap();
        assert_eq!(sizing.capacity, [320, 256]);
        assert_eq!(sizing.buffer, [200, 150]);
        assert!(sizing.settle(viewport([210., 155.], false), 4096).unwrap());
        assert_eq!(sizing.capacity, [320, 256]);
        assert!(sizing.settle(viewport([80., 60.], false), 4096).unwrap());
        assert_eq!(sizing.capacity, [320, 256]);
    }
    #[test]
    fn both_producer_orders_preserve_current_logical_and_dpi_size() {
        let mut sizing = NavigatorSizing::default();
        sizing.settle(viewport([200., 150.], false), 4096).unwrap();
        let before = sizing;
        sizing.settle(viewport([400., 300.], false), 4096).unwrap();
        for renderer in [before, sizing] {
            for current in [[160., 120.], [400., 300.]] {
                for dpi in [1., 1.25, 1.5, 2.] {
                    for (i, value) in current.into_iter().enumerate() {
                        let result = renderer.capacity[i] as f32 * renderer.matrix()[i]
                            * value / renderer.nominal[i] * dpi;
                        assert!((result - value * dpi).abs() < 0.001);
                    }
                }
            }
        }
    }
    #[test]
    fn invalid_bounds_do_not_change_the_retained_allocation() {
        let mut sizing = NavigatorSizing::default();
        sizing.settle(viewport([200., 150.], false), 4096).unwrap();
        let before = sizing;
        for image in [[f32::NAN, 10.], [0., 10.], [785., 200.]] {
            assert!(sizing.settle(viewport(image, false), 4096).is_err());
            assert_eq!(sizing, before);
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct CapyNavigatorSurfaceInfo {
    pub handle: usize,
    pub generation: u64,
    pub ready: u32,
}

#[cfg(target_os = "windows")]
mod windows_surface {
    use super::*;
    use layer_host::NativeHost;
    use layer_render::CanvasRenderer;
    use layer_render_wgpu::{OverviewPlacement, ViewportPresenter};
    use std::{collections::BTreeMap, os::windows::io::{AsRawHandle, OwnedHandle}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

    struct GpuSurface {
        pending: Option<wgpu::SurfaceTexture>,
        presenter: ViewportPresenter,
        surface: wgpu::Surface<'static>,
        handle: OwnedHandle,
        config: wgpu::SurfaceConfiguration,
        sizing: NavigatorSizing,
        device: wgpu::Device,
        space: layer_core::color::DocumentColor,
        completed: Arc<AtomicBool>,
        generation: u64,
    }
    struct Slot { viewport: CapyNavigatorViewport, gpu: Option<GpuSurface> }
    impl Slot {
        fn retire(&mut self) -> Result<(), String> {
            if let Some(gpu) = self.gpu.take() { layer_host::window::retire_render_resources(Box::new(gpu))?; }
            Ok(())
        }
    }
    #[derive(Default)]
    pub(crate) struct NavigatorPresenters {
        prepared: Vec<ViewportPresenter>,
        key: Option<(wgpu::Device, layer_core::color::DocumentColor, wgpu::TextureFormat, layer_render_wgpu::SdrSurfaceColor)>,
    }
    impl NavigatorPresenters {
        pub(crate) fn prepare(&mut self, gpu: &layer_render_wgpu::WgpuRasterizer, count: usize,
            format: wgpu::TextureFormat, encoding: layer_render_wgpu::SdrSurfaceColor) -> Result<(), String> {
            self.prepared = (0..count.max(1)).map(|_| {
                ViewportPresenter::for_overview_surface(gpu, format, encoding).map_err(|error| error.to_string())
            }).collect::<Result<_, _>>()?;
            self.key = Some((gpu.device().clone(), gpu.document_color(), format, encoding));
            Ok(())
        }
        pub(crate) fn take(&mut self, gpu: &layer_render_wgpu::WgpuRasterizer,
            format: wgpu::TextureFormat, encoding: layer_render_wgpu::SdrSurfaceColor) -> Result<ViewportPresenter, String> {
            let key = (gpu.device().clone(), gpu.document_color(), format, encoding);
            let presenter = if self.key.as_ref() == Some(&key) { self.prepared.pop() } else { None };
            match presenter {
                Some(presenter) => Ok(presenter),
                None => ViewportPresenter::for_overview_surface(gpu, format, encoding).map_err(|error| error.to_string()),
            }
        }
    }
    #[derive(Default)]
    pub(crate) struct NavigatorSurfaces {
        slots: BTreeMap<u64, Slot>, generation: u64,
        pub(crate) presenters: NavigatorPresenters,
    }
    impl NavigatorSurfaces {
        pub(crate) fn prepare(&mut self, gpu: &layer_render_wgpu::WgpuRasterizer,
            format: wgpu::TextureFormat, encoding: layer_render_wgpu::SdrSurfaceColor) -> Result<(), String> {
            self.presenters.prepare(gpu, self.slots.len(), format, encoding)
        }
        pub(crate) fn update(&mut self, id: u64, viewport: Option<CapyNavigatorViewport>) -> Result<(), String> {
            if id == 0 { return Err("Invalid Navigator surface id".into()); }
            if let Some(viewport) = viewport {
                let mut validation = NavigatorSizing::default();
                validation.settle(viewport, u32::MAX)?;
                self.slots.entry(id).and_modify(|slot| slot.viewport = viewport)
                    .or_insert(Slot { viewport, gpu: None });
            } else if let Some(mut slot) = self.slots.remove(&id) { slot.retire()?; }
            Ok(())
        }
        pub(crate) fn reset(&mut self) -> Result<(), String> {
            self.presenters = NavigatorPresenters::default();
            for slot in self.slots.values_mut() { slot.retire()?; }
            Ok(())
        }
        pub(crate) fn info(&self, id: u64) -> CapyNavigatorSurfaceInfo {
            self.slots.get(&id).and_then(|slot| slot.gpu.as_ref()).map_or_default(|gpu| CapyNavigatorSurfaceInfo {
                handle: gpu.handle.as_raw_handle() as usize, generation: gpu.generation,
                ready: u32::from(gpu.completed.load(Ordering::Acquire)),
            })
        }
        pub(crate) fn present(&mut self, native: &NativeHost, instance: &wgpu::Instance,
            canvas: &ViewportPresenter, config: &wgpu::SurfaceConfiguration, display: &crate::display::Display) -> Result<bool, String> {
            let Some(gpu) = native.session.engine().backend().0.as_deref() else { return Ok(false); };
            gpu.device().poll(wgpu::PollType::Poll).map_err(|error| error.to_string())?;
            let mut dirty = false;
            let document = native.session.engine().document().composition().size;
            let state = native.session.state();
            let outline = state.palette.text.linear();
            let encoding = display.encoding(config.format);
            for slot in self.slots.values_mut() {
                if slot.gpu.as_ref().is_some_and(|surface| surface.device != *gpu.device()) { slot.retire()?; }
                if slot.gpu.is_none() {
                    if slot.viewport.moving != 0 { continue; }
                    let mut sizing = NavigatorSizing::default();
                    sizing.settle(slot.viewport, gpu.device().limits().max_texture_dimension_2d)?;
                    let (surface, handle) = crate::host::CapyHost::composition_surface(instance)?;
                    let mut target = config.clone();
                    target.width = sizing.capacity[0]; target.height = sizing.capacity[1];
                    surface.configure(gpu.device(), &target);
                    unsafe { surface.as_hal::<wgpu::hal::api::Dx12>() }.ok_or("Navigator requires D3D12")?
                        .set_nonblocking().map_err(|error| error.to_string())?;
                    let presenter = self.presenters.take(gpu, target.format, encoding)?;
                    self.generation = self.generation.saturating_add(1);
                    slot.gpu = Some(GpuSurface { pending: None, presenter, surface, handle, config: target, sizing,
                        device: gpu.device().clone(), space: gpu.document_color(), completed: Arc::new(AtomicBool::new(false)), generation: self.generation });
                }
                let surface = slot.gpu.as_mut().unwrap();
                if let Some(pending) = surface.pending.take() {
                    let chain = unsafe { surface.surface.as_hal::<wgpu::hal::api::Dx12>() }.and_then(|s| s.swap_chain()).ok_or("Missing Navigator swap chain")?;
                    let matrix = surface.sizing.matrix();
                    unsafe {
                        chain.SetSourceSize(surface.sizing.buffer[0], surface.sizing.buffer[1]).map_err(|error| error.to_string())?;
                        chain.SetMatrixTransform(&windows::Win32::Graphics::Dxgi::DXGI_MATRIX_3X2_F {
                            _11: matrix[0], _22: matrix[1], ..Default::default()
                        }).map_err(|error| error.to_string())?;
                    }
                    let before = unsafe { chain.GetLastPresentCount() }.map_err(|error| error.to_string())?;
                    match gpu.queue().try_present(pending) {
                        Err(pending) => { surface.pending = Some(pending); dirty = true; continue; },
                        Ok(()) => {
                            let after = unsafe { chain.GetLastPresentCount() }.map_err(|error| error.to_string())?;
                            if after == before { return Err("Navigator presentation was not accepted".into()); }
                            let completed = surface.completed.clone();
                            gpu.queue().on_submitted_work_done(move || completed.store(true, Ordering::Release));
                        },
                    }
                }
                let output_changed = surface.config.format != config.format || surface.config.color_space != config.color_space;
                if output_changed && slot.viewport.moving != 0 { continue; }
                let capacity = surface.sizing.capacity;
                surface.sizing.settle(slot.viewport, gpu.device().limits().max_texture_dimension_2d)?;
                if output_changed || capacity != surface.sizing.capacity {
                    surface.config.format = config.format;
                    surface.config.color_space = config.color_space;
                    surface.config.view_formats = config.view_formats.clone();
                    surface.config.width = surface.sizing.capacity[0]; surface.config.height = surface.sizing.capacity[1];
                    surface.surface.configure(gpu.device(), &surface.config);
                    unsafe { surface.surface.as_hal::<wgpu::hal::api::Dx12>() }.unwrap().set_nonblocking().map_err(|error| error.to_string())?;
                    surface.presenter.set_target_retention(layer_render_wgpu::TargetRetention::Discarded);
                }
                if output_changed || surface.space != gpu.document_color() {
                    surface.presenter = ViewportPresenter::for_overview_surface(gpu, config.format, encoding).map_err(|error| error.to_string())?;
                    surface.space = gpu.document_color();
                }
                surface.presenter.inherit_proof(gpu, canvas);
                let [width, height] = surface.sizing.buffer;
                let Some(geometry) = layer_ui::NavigatorGeometry::new(&state.camera, document, [width as f32 + 8., height as f32 + 8.]) else { continue; };
                let work_area = geometry.work_area.map(|point| [
                    (point[0] - geometry.image.x) * width as f32 / geometry.image.width,
                    (point[1] - geometry.image.y) * height as f32 / geometry.image.height,
                ]);
                surface.presenter.set_overviews(gpu, &[OverviewPlacement {
                    bounds: [0., 0., width as f32, height as f32], clip: None, work_area,
                    outline_linear: [outline[0], outline[1], outline[2]], scale: slot.viewport.scale * width as f32 / slot.viewport.image[0], opacity: 1.,
                }]);
                if !surface.presenter.pipelines_ready(gpu) || !gpu.showing_navigator() { dirty = true; continue; }
                if !surface.presenter.needs_overview_present(gpu, [width, height]) {
                    dirty |= !surface.completed.load(Ordering::Acquire);
                    continue;
                }
                let target = match surface.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(target) | wgpu::CurrentSurfaceTexture::Suboptimal(target) => target,
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => { dirty = true; continue; },
                    wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => { slot.retire()?; dirty = true; continue; },
                    wgpu::CurrentSurfaceTexture::Validation => return Err("Navigator surface validation failed".into()),
                };
                surface.presenter.present_overviews(gpu, &target.texture.create_view(&Default::default()), [width, height]).map_err(|error| error.to_string())?;
                surface.pending = Some(target);
                dirty = true;
            }
            Ok(dirty)
        }
    }
}
#[cfg(target_os = "windows")]
pub(crate) use windows_surface::NavigatorSurfaces;
