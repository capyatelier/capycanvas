use layer_host::handles::{self,Handle,Borrowed};
use crate::owned::text;
use crate::device::D3d12Watch;
use crate::input::{read_json, with_text};
use layer_host::{
    NativeHost,
    scene::SceneDisplay,
};
use layer_render::CanvasRenderer;
use layer_render_wgpu::ViewportPresenter;
use layer_ui::{CanvasCursor, PointerButton};
use std::{
    cell::RefCell,
    ffi::{CString, c_char, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};

thread_local! { static ERROR: RefCell<CString> = RefCell::new(CString::default()); }
fn fail(e: impl std::fmt::Display) {
    ERROR.with(|slot| *slot.borrow_mut() = CString::new(e.to_string().replace('\0', " ")).unwrap());
}
fn guard(raw:u64, f: impl FnOnce(&mut CapyHost) -> Result<i32, String>) -> i32 {
    let mut owned=match borrow::<CapyHost>(raw) {Ok(host)=>host,Err(error)=> {fail(error);return -1;}};
    let host=&mut *owned;
    match layer_host::contain(host, f) {
        Ok(value) => value,
        Err(error) => {
            if host.native.poisoned().is_none() {
                host.native.error = Some(error.clone());
            }
            fail(error);
            -1
        }
    }
}
fn borrow<T:Send+'static>(raw:u64)->Result<Borrowed<T>,String> {
    Handle::from_raw(raw).and_then(handles::lease::<T>).and_then(|lease|lease.borrow()).map_err(err)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

use crate::events::{CapyPointer, validate_batch};
pub struct CapyLaunch {
    native: Option<NativeHost>,
    settings: Option<crate::settings::PreparedSettings>,
    bootstrap: CString,
}
#[derive(Default)]
struct SurfaceReadiness {
    queued: bool,
    queued_chrome: u64,
    ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
    completed_chrome: std::sync::Arc<std::sync::atomic::AtomicU64>,
    documents: std::collections::VecDeque<(u64,[u32;2])>,
    published: bool,
}
impl SurfaceReadiness {
    fn submitted(&mut self, queue: &wgpu::Queue, chrome: &crate::chrome::Chrome, document: Option<[u32;2]>) {
        let document=document.filter(|document|*document==chrome.document);
        let revision=if document.is_some(){chrome.revision()}else{0};
        if self.queued&&revision<=self.queued_chrome{return;}
        if let Some(document)=document{self.documents.push_back((revision,document));}
        self.queued=true;self.queued_chrome=revision;
        let ready=self.ready.clone();let completed=self.completed_chrome.clone();
        queue.on_submitted_work_done(move|| {
            completed.fetch_max(revision,std::sync::atomic::Ordering::Release);
            ready.store(true,std::sync::atomic::Ordering::Release);
        });
    }
    fn ready(&self)->bool {self.ready.load(std::sync::atomic::Ordering::Acquire)}
    fn pending(&self)->bool {
        self.queued&&(!self.ready()||self.completed_chrome.load(std::sync::atomic::Ordering::Acquire)<self.queued_chrome)
    }
    fn retire_chrome(&mut self){self.completed_chrome=Default::default();self.queued_chrome=0;self.documents.clear();}
    fn changed(&mut self, chrome: &mut crate::chrome::Chrome)->bool {
        let completed=self.completed_chrome.load(std::sync::atomic::Ordering::Acquire);
        while self.documents.front().is_some_and(|(revision,_)|*revision<=completed){
            let (revision,document)=self.documents.pop_front().unwrap();chrome.completed(revision,document);
        }
        let ready=self.ready();let changed=self.published!=ready;self.published=ready;changed
    }
}
pub struct CapyHost {
    // An acquired image drops before its surface; the panel is detached on the
    // UI thread before destruction. The surface retains its native COM reference.
    gpu: layer_host::DeviceWatch,
    gpu_generation: u64,
    localization_generation: Option<u64>,
    localization_input_busy: bool,
    document_epoch: u64,
    window: usize,
    display: crate::display::Display,
    display_checked: Option<std::time::Instant>,
    display_probe: crate::display::DisplayProbe,
    test_display: Option<crate::display::Display>,
    presenter_key: Option<(layer_core::color::DocumentColor, layer_render_wgpu::SdrSurfaceColor)>,
    target: Option<wgpu::SurfaceTexture>,
    pacing: crate::frame_pacing::FramePacing,
    surface: wgpu::Surface<'static>,
    surface_handle: OwnedHandle,
    config: Option<wgpu::SurfaceConfiguration>,
    surface_invalid: bool,
    presenter: Option<ViewportPresenter>,
    native: NativeHost,
    instance: wgpu::Instance,
    cursor: CanvasCursor,
    chrome_facts: layer_ui::ChromeFacts,
    chrome: crate::chrome::Chrome,
    published_chrome_revision: Option<u64>,
    published_glass_count: usize,
    scale: f32,
    blank_presented: bool,
    surface_ready: SurfaceReadiness,
    prediction_frames: Option<[u64; 2]>, // opt-in, accumulated across document switches
    frame_timing: Option<crate::frame_timing::FrameTiming>,
    services: Option<crate::settings::SettingsService>,
    launch_settings: Option<crate::settings::PreparedSettings>,
    filters: Option<crate::filter_packages::FilterService>,
    documents: Option<crate::documents::DocumentService>,
    workspaces: Option<crate::workspace_owner::WorkspaceOwner>,
    blocked_contacts: std::collections::BTreeSet<u64>,
    live_contacts: std::collections::BTreeMap<u64, CapyPointer>,
    edge_touches: std::collections::BTreeMap<u64, bool>,
    scopes: crate::scopes::Scopes,
}
fn touch_policy(scale: f32) -> layer_ui::TouchPolicy {
    use windows::Win32::UI::{HiDpi::GetSystemMetricsForDpi, WindowsAndMessaging::{SM_CXDRAG, SM_CYDRAG}};
    let dpi = (scale * 96.).round().clamp(1., u32::MAX as f32) as u32;
    let slop = unsafe { GetSystemMetricsForDpi(SM_CXDRAG, dpi).max(GetSystemMetricsForDpi(SM_CYDRAG, dpi)) }.max(1) as f32;
    layer_ui::TouchPolicy { slop, ..Default::default() }
}
fn pointer_button(sample: &CapyPointer) -> PointerButton {
    match sample.button {
        0 => PointerButton::Primary,
        1 => PointerButton::Pan,
        _ => PointerButton::Other,
    }
}
fn take_prepared_target<T>(presenter: &ViewportPresenter, renderer: &layer_render_wgpu::WgpuRasterizer, target: &mut Option<T>) -> Option<T> {
    (renderer.has_presentable_frame()&&presenter.pipelines_ready(renderer)).then(|| target.take()).flatten()
}
fn surface_extent(allocated: [u32; 2], viewport: [u32; 2], limit: u32) -> Result<[u32; 2], String> {
    if viewport.contains(&0) || viewport.into_iter().chain(allocated).any(|v| v > limit) {
        return Err("Invalid surface extent".into());
    }
    Ok(std::array::from_fn(|i| allocated[i].max(viewport[i].div_ceil(128).saturating_mul(128).min(limit))))
}

impl AsMut<NativeHost> for CapyHost {
    fn as_mut(&mut self) -> &mut NativeHost { &mut self.native }
}
impl CapyHost {
    fn composition_surface(instance: &wgpu::Instance) -> Result<(wgpu::Surface<'static>, OwnedHandle), String> {
        let raw = unsafe { windows::Win32::Graphics::DirectComposition::DCompositionCreateSurfaceHandle(3, None) }.map_err(err)?;
        let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::SurfaceHandle(handle.as_raw_handle())) }.map_err(err)?;
        Ok((surface, handle))
    }
    fn cancel_contacts(&mut self) -> Result<(), String> {
        self.blocked_contacts.extend(std::mem::take(&mut self.edge_touches).into_iter().filter_map(|(id, claimed)| claimed.then_some(id)));
        for (id, mut sample) in std::mem::take(&mut self.live_contacts) {
            sample.phase = 4;
            sample.flags &= !1;
            self.blocked_contacts.insert(id);
            self.native.pointer_event(sample.event(), pointer_button(&sample))?;
        }
        Ok(())
    }
    fn canvas_edge(&mut self, sample: &CapyPointer, phase: layer_ui::ContactPhase) -> Result<layer_ui::InputReply, String> {
        use layer_ui::{EdgeContact, PointerKind, UiInput};
        self.native.input(UiInput::EdgeContact {
            id: sample.id,
            phase,
            kind: match sample.tool() {
                layer_engine::ToolKind::Finger => PointerKind::Touch,
                layer_engine::ToolKind::Mouse => PointerKind::Mouse,
                _ => PointerKind::Pen,
            },
            contact: EdgeContact::Free,
            position: [sample.x / self.scale, sample.y / self.scale],
            viewport: self.native.logical,
            time_ns: sample.timestamp_ns,
        })
    }
    fn edge_touch(&mut self, sample: &CapyPointer) -> Result<bool, String> {
        use layer_ui::ContactPhase;
        if sample.tool() != layer_engine::ToolKind::Finger || sample.flags & 1 != 0 {
            return Ok(false);
        }
        let phase = match sample.phase {
            1 => ContactPhase::Down,
            2 => ContactPhase::Move,
            3 => ContactPhase::Up,
            4 => ContactPhase::Cancel,
            _ => return Ok(false),
        };
        let claimed = match self.edge_touches.get(&sample.id) {
            Some(claimed) => *claimed,
            None if phase == ContactPhase::Down => false,
            None => return Ok(false),
        };
        let reply = self.canvas_edge(sample, phase)?;
        if reply.edge_retired {
            self.edge_touches.retain(|_, claimed| *claimed);
        }
        if matches!(phase, ContactPhase::Up | ContactPhase::Cancel) {
            return Ok(self.edge_touches.remove(&sample.id).unwrap_or(false));
        }
        if claimed {
            return Ok(true);
        }
        if reply.edge_cursor.is_none() {
            if reply.edge_pending {
                self.edge_touches.insert(sample.id, false);
            } else {
                self.edge_touches.remove(&sample.id);
            }
            return Ok(false);
        }
        let mut cancel = *sample;
        cancel.phase = 4;
        self.native.pointer_event(cancel.event(), pointer_button(&cancel))?;
        self.live_contacts.remove(&sample.id);
        self.edge_touches.insert(sample.id, true);
        Ok(true)
    }
    fn cancel_contact(&mut self, sample: &CapyPointer) -> Result<(), String> {
        self.edge_touches.remove(&sample.id);
        self.canvas_edge(sample, layer_ui::ContactPhase::Cancel)?;
        if let Some(mut live) = self.live_contacts.remove(&sample.id) {
            live.phase = 4;
            live.flags &= !1;
            self.native.pointer_event(live.event(), pointer_button(&live))?;
        }
        Ok(())
    }
    fn track_contact(&mut self, sample: &CapyPointer) {
        if sample.phase == 0 {
            return;
        }
        if matches!(sample.phase, 3 | 4) {
            self.live_contacts.remove(&sample.id);
        } else {
            self.live_contacts.insert(sample.id, *sample);
        }
    }
    unsafe fn new(width: u32, height: u32, scale: f32, launch: &mut CapyLaunch) -> Result<Self, String> {
        if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid Windows canvas surface".into());
        }
        let mut native = launch.native.take().ok_or("Windows launch was already adopted")?;
        native.startup = Default::default();
        native.resize(width, height, scale)?;
        native.session.set_touch_policy(touch_policy(scale))?;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        descriptor.backend_options.dx12.shader_compiler =
            wgpu::Dx12Compiler::from_env().unwrap_or_else(bundled_shader_compiler);
        // GPU optimization is independent of Rust/C++ debugging. DXC's -Od
        // fragment storage-buffer code can be rejected by drivers; retain API
        // validation while using the same optimized shaders as Release.
        descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
        let instance = wgpu::Instance::new(descriptor);
        let (surface, surface_handle) = Self::composition_surface(&instance)?;
        // GPU allocation and shader work happen after ownership moves to the
        // render worker. No swap chain is attached during CPU/UI initialization.
        Ok(Self {
            gpu: Default::default(),
            gpu_generation: 0,
            localization_generation: None,
            localization_input_busy: false,
            document_epoch: native.session.state().document_file.epoch,
            window: 0,
            display: Default::default(),
            display_checked: None,
            display_probe: Default::default(),
            test_display: None,
            presenter_key: None,
            target: None,
            pacing: Default::default(),
            surface,
            surface_handle,
            config: None,
            surface_invalid: false,
            presenter: None,
            native,
            instance,
            cursor: CanvasCursor::default(),
            chrome_facts: layer_ui::ChromeFacts::default(),
            chrome: Default::default(),
            published_chrome_revision: None,
            published_glass_count: 0,
            scale,
            blank_presented: false,
            surface_ready: Default::default(),
            prediction_frames: std::env::var_os("CAPY_LATENCY_TRACE").map(|_| [0; 2]),
            frame_timing: std::env::var_os("CAPY_LATENCY_TRACE").map(|_| Default::default()),
            services: None,
            launch_settings: launch.settings.take(),
            filters: None,
            documents: None,
            workspaces: None,
            blocked_contacts: Default::default(),
            live_contacts: Default::default(),
            edge_touches: Default::default(),
            scopes: Default::default(),
        })
    }

    fn device_is_lost(&self) -> bool {
        self.gpu.is_lost(
            self.native
                .session
                .engine()
                .backend()
                .0
                .as_ref()
                .map(|gpu| gpu.device()),
        )
    }

    fn accepts_workspace_input(&self)->Result<bool,String> {
        if self.services.as_ref().is_some_and(|service|service.close_status().requested){return Ok(false);}
        self.workspaces.as_ref().map(|owner|owner.with(|service|Ok(service.accepts_input(crate::workspace_service::now_ms())))).transpose().map(|accepted|accepted.unwrap_or(true))
    }
    fn sync_document(&mut self) {
        let epoch = self.native.session.state().document_file.epoch;
        if epoch != self.document_epoch {
            self.document_epoch = epoch;
            self.target = None;
            self.presenter = None;
            self.presenter_key = None;
            self.surface_ready.retire_chrome();
            self.cursor = Default::default();
            // This window already presented. A prepared candidate owns its
            // composite; submitting a background-only startup frame would clear it.
        }
    }
    fn poll_services(&mut self) -> Result<(), String> {
        if self.display_checked.is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(500)) {
            self.display_checked = Some(std::time::Instant::now());
            let display = self.test_display.clone().unwrap_or_else(|| self.display_probe.poll(self.window).clone());
            if display != self.display {
                self.display = display;
                self.native.dirty = true;
                self.native.invalidate_snapshot();
            }
            let headroom = self.config.as_ref().map_or(1., |c| self.display.available_headroom(c.format));
            if self.native.session.set_hdr_display_available(headroom > 1.) {
                self.native.dirty = true;
                self.native.invalidate_snapshot();
            }
        }
        if let Some(service) = self.documents.as_mut() {
            service.poll(&mut self.native)?;
            service.proof.poll(&mut self.native)?;
            if !self.gpu.is_lost(self.native.session.engine().backend().0.as_ref().map(|g| g.device()))
                && service.tone.tick(&self.native)?
            {
                self.native.dirty = true;
                self.native.invalidate_snapshot();
            }
        }
        self.sync_document();
        if let Some(service) = self.services.as_mut()
            && (!self.native.session.state().document_file.close_ready || self.documents.as_ref().is_none_or(|d| d.window_close_ready(&self.native))) {
            service.localization_input(self.localization_input_busy || !self.live_contacts.is_empty());
            service.poll(&mut self.native)?;
        }
        if let Some(service) = self.documents.as_mut() {
            service.set_localization(self.native.session.localization().clone())?;
        }
        if let Some(service) = self.filters.as_mut() {
            service.poll(&mut self.native);
        }
        if let Some(owner) = self.workspaces.as_ref() {owner.with(|service| {
            if self.documents.as_ref().is_some_and(|d| d.window_close_ready(&self.native))
                && self.services.as_ref().is_none_or(|s| s.close_status().ready)
                && self.documents.as_ref().and_then(|d| d.recovery.as_ref()).is_none_or(|s| s.close_ready())
                && !service.view().closing
                && !service.view().closed
            {
                let _ = service.input(&mut self.native, layer_workspace::WorkspaceInput::Close);
            }
            service.poll(&mut self.native, crate::workspace_service::now_ms());Ok(())
        })?;}
        Ok(())
    }

    fn prepare_gpu(&mut self) -> Result<(), String> {
        if self.device_is_lost() && std::env::var_os("CAPY_TEST_GPU_UNAVAILABLE").is_some()
            && std::env::var_os("CAPY_SMOKE_TEST").is_some()
            && crate::storage::isolated()
        {
            return Err("Test GPU remains unavailable".into());
        }
        if self.config.is_some() {
            return Ok(());
        }
        if self.device_is_lost() {
            // D3D12 devices are process/adapter singletons. Release the removed
            // renderer and presenter before requesting another device. Retained
            // CPU assets, history and input remain in the same UiSession.
            if let Some(service) = &mut self.documents { service.renderer_unavailable(&mut self.native)?; }
            self.presenter = None;
            self.presenter_key = None;
            self.surface_ready.retire_chrome();
            let renderer = layer_host::Renderer(self.native.session.renderer_mut().0.take());
            if let Some(service) = self.documents.as_mut().and_then(|d| d.recovery.as_mut()) { service.retire_renderer(renderer); } else { drop(renderer); }
            self.native.startup = Default::default();
            // Completed document candidates may still own the removed device;
            // reject those while allowing an in-flight CPU Save to complete.
            self.poll_services()?;
        }
        let adapter =
            pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&self.surface),
                force_fallback_adapter: false,
                power_preference: wgpu::PowerPreference::HighPerformance,
                apply_limit_buckets: false,
            }))
            .map_err(err)?;
        let limits = layer_render_wgpu::rendering_limits(adapter.limits());
        layer_host::admit_adapter(self.native.session.localization(), &adapter, &limits)?;
        let (device, queue, gpu_state) = pollster::block_on(layer_host::request_device(&adapter, &wgpu::DeviceDescriptor {
            label: Some("Capy Canvas Windows"),
            required_features: layer_render_wgpu::rendering_features(&adapter)
                | adapter.features() & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            required_limits: limits,
            memory_hints: layer_render_wgpu::memory_hints(),
            ..Default::default()
        }))
        .map_err(err)?;
        let [width, height] = self.native.session.state().camera.viewport;
        let mut config = self
            .surface
            .get_default_config(&adapter, width, height)
            .ok_or("D3D12 surface unsupported")?;
        config.present_mode = if self.surface.get_capabilities(&adapter).present_modes
            .contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else { wgpu::PresentMode::Fifo };
        config.desired_maximum_frame_latency = crate::frame_pacing::FramePacing::IN_FLIGHT as u32;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        // Keep scRGB across monitor moves; only viewing changes, never artwork.
        if self.surface.get_capabilities(&adapter).format_capabilities.iter().any(|f|
            f.format == wgpu::TextureFormat::Rgba16Float
                && f.color_spaces.contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)) {
            config.format = wgpu::TextureFormat::Rgba16Float;
            config.color_space = wgpu::SurfaceColorSpace::ExtendedSrgbLinear;
            config.view_formats.clear();
        }
        let renderer = layer_host::GpuContext { adapter, device, queue }.rasterizer(
            self.native.session.engine().document().composition().color, &self.native.renderer_options(), false)?;
        let encoding = self.display.encoding(config.format);
        let presenter = ViewportPresenter::for_surface(&renderer, config.format, encoding).map_err(err)?;
        self.presenter_key = Some((renderer.document_color(), encoding));
        presenter.pipelines_ready(&renderer);
        gpu_state.check()?;
        let retired = self.native.replace_renderer(layer_host::Renderer(Some(renderer)))?;
        if let Some(service) = self.documents.as_mut().and_then(|d| d.recovery.as_mut()) { service.retire_renderer(retired); } else { drop(retired); }
        if self.device_is_lost() {
            self.native.error = None;
        }
        self.gpu = gpu_state;
        self.gpu_generation = self.gpu_generation.saturating_add(1);
        self.pacing = Default::default();
        self.surface_ready = Default::default();
        self.native.invalidate_snapshot();
        self.native.startup = Default::default();
        self.presenter = Some(presenter);
        self.chrome.reset_presented();
        self.config = Some(config);
        self.native.reset_canvas_frame_rate();
        Ok(())
    }

    fn frame(&mut self, now: u64, presentation: u64) -> Result<i32, String> {
        self.gpu.check()?;
        // A tab command can arrive after DXGI acquisition. Retire that image
        // before touching the newly selected editor, including failed activation.
        self.sync_document();
        if self.native.session.engine().backend().0.is_none() {
            self.target = None;
            self.native.dirty = false;
            return Ok(0);
        }
        if self.target.is_none() { return Ok(1); }
        if !self.surface_ready.ready() {
            self.native.session.engine().backend().0.as_ref().unwrap().device().poll(wgpu::PollType::Poll).map_err(err)?;
        }
        let prediction_before = self.prediction_frames.map(|_| self.native.session.engine().metrics());
        self.native
            .prepare_canvas_frame(now, presentation, self.blank_presented)?;
        if let Some(timing) = &mut self.frame_timing { timing.prepared_ns = crate::frame_timing::now(); }
        if let (Some(total), Some(before)) = (&mut self.prediction_frames, prediction_before) {
            let after = self.native.session.engine().metrics();
            total[0] += after.platform_prediction_frames.saturating_sub(before.platform_prediction_frames);
            total[1] += after.engine_prediction_frames.saturating_sub(before.engine_prediction_frames);
        }
        self.gpu.check()?;
        self.native.dirty |= self.native.session.wants_continuous_frames();
        let view = self.native.session.state().camera.view();
        let surround = self.native.session.state().palette.surround_linear;
        let gpu = self.native.session.engine().backend().0.as_ref().unwrap();
        let config = self.config.as_ref().ok_or("Missing surface configuration")?;
        let encoding = self.display.encoding(config.format);
        let headroom = self.display.available_headroom(config.format);
        let key = (gpu.document_color(), encoding);
        if self.presenter_key != Some(key) {
            let presenter = ViewportPresenter::for_surface(gpu, config.format, encoding).map_err(err)?;
            presenter.pipelines_ready(gpu);
            self.presenter = Some(presenter);
            self.presenter_key = Some(key);
            self.chrome.reset_presented();
            self.surface_ready.retire_chrome();
        }
        let presenter = self
            .presenter
            .as_mut()
            .ok_or("Viewport presenter is not prepared")?;
        let chrome=self.blank_presented;
        if chrome {
            presenter.set_panel_surfaces(gpu,self.chrome.glass.panels(self.scale));
            if !presenter.prepare_chrome(gpu) {
                self.native.dirty=true;
                return Ok(1);
            }
        }
        let tone = self.documents.as_ref().and_then(|s| s.tone.current(&self.native));
        let display = SceneDisplay {
            scale: self.scale,
            headroom,
            blank_presented: self.blank_presented,
            compositor_hdr: false,
        };
        let chrome_staged = self.native.compose_scene(
            presenter,
            &mut self.cursor,
            &self.chrome.navigators,
            self.chrome.glass.regions(self.scale),
            tone,
            display,
        )?;
        if let Some(timing) = &mut self.frame_timing { timing.composed_ns = crate::frame_timing::now(); }
        if !self.chrome.can_present(chrome_staged) {
            self.native.dirty = true;
            self.poll_services()?;
            return Ok(1);
        }
        let gpu = self.native.session.engine().backend().0.as_ref().unwrap();
        let Some(target) = take_prepared_target(presenter, gpu, &mut self.target) else {
            if !self.surface_ready.queued && let Some(target)=self.target.take() {
                let view=target.texture.create_view(&Default::default());
                let mut encoder=gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor{label:Some("startup surround")});
                {
                    let _pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label:Some("startup surround"),color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                            view:&view,depth_slice:None,resolve_target:None,
                            ops:wgpu::Operations{load:wgpu::LoadOp::Clear(encoding.clear_color(config.format,surround).map_err(err)?),store:wgpu::StoreOp::Store},
                        })],..Default::default()
                    });
                }
                let submission=gpu.queue().submit_for_present([encoder.finish()]);gpu.queue().present(target);
                self.native.canvas_frame_rate.submitted();self.pacing.submitted(submission);
                self.surface_ready.submitted(gpu.queue(), &self.chrome, None);
            }
            self.native.dirty = true;
            self.poll_services()?;
            return Ok(1);
        };
        let submission = presenter.present(
            gpu,
            &target.texture.create_view(&Default::default()),
            view,
            surround,
        ).map_err(err)?;
        if let Some(timing) = &mut self.frame_timing { timing.submitted_ns = crate::frame_timing::now(); }
        gpu.queue().present(target);
        if let Some(timing) = &mut self.frame_timing { timing.presented_ns = crate::frame_timing::now(); }
        self.native.canvas_frame_rate.submitted();
        self.pacing.submitted(submission);
        let document=self.native.session.engine().document().composition().size;
        self.surface_ready.submitted(gpu.queue(), &self.chrome, (chrome_staged&&presenter.chrome_ready()).then_some(document));
        self.blank_presented = true;
        gpu.device().poll(wgpu::PollType::Poll).map_err(err)?;
        self.gpu.check()?;
        self.poll_services()?;
        Ok(i32::from(self.native.dirty))
    }
}
/// # Safety
/// Call on the startup worker. `tags` and non-null `screen` are readable NUL-terminated JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_launch(tags: *const c_char, screen: *const c_char) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let tags: Vec<String> = unsafe { read_json(tags) }?;
        let screen = if screen.is_null() { None } else { unsafe { read_json(screen) }? };
        let (native, settings) = crate::settings::SettingsService::launch(&tags.iter().map(String::as_str).collect::<Vec<_>>(), screen)?;
        let bootstrap = CString::new(serde_json::to_string(&native.bootstrap_view()).map_err(err)?).map_err(err)?;
        Ok::<_, String>(CapyLaunch { native: Some(native), settings: Some(settings), bootstrap })
    }));
    match result {
        Ok(Ok(launch)) => match handles::insert(launch) {Ok(handle)=>handle.raw(),Err(error)=> {fail(error);0}},
        Ok(Err(error)) => { fail(error); 0 },
        Err(_) => { fail("Windows launch panic"); 0 },
    }
}
/// # Safety
/// The launch remains alive and exclusively owned until this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_launch_view(launch:u64)->u64 {
    borrow::<CapyLaunch>(launch).map_or(0,|launch|text(launch.bootstrap.clone()))
}
/// # Safety
/// Borrow a live, unadopted launch exclusively for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_launch_localization(launch: u64) -> u64 {
    borrow::<CapyLaunch>(launch).ok().and_then(|launch|launch.native.as_ref().map(|native|crate::shared_controls::store_localization(native.session.localization().clone()))).unwrap_or(0)
}
/// # Safety
/// Free a uniquely owned launch once, after its surface adoption or cancellation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_launch_free(launch: u64) {
    let _=Handle::from_raw(launch).and_then(handles::release::<CapyLaunch>);
}
/// # Safety
/// `launch` must be live, exclusively owned, and not previously adopted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_create(
    width: u32,
    height: u32,
    scale: f32,
    launch: u64,
) -> u64 {
    match catch_unwind(AssertUnwindSafe(|| unsafe {
        let mut launch=borrow::<CapyLaunch>(launch)?;
        CapyHost::new(width,height,scale,&mut launch)
    })) {
        Ok(Ok(host)) => match handles::insert(host) {Ok(handle)=>handle.raw(),Err(error)=> {fail(error);0}},
        Ok(Err(e)) => {
            fail(e);
            0
        }
        Err(_) => {
            fail("Canvas initialization panic");
            0
        }
    }
}
/// # Safety
/// Exclusive access to a live host; the returned handle is borrowed until surface replacement or host destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_surface_handle(host: u64) -> *mut c_void {
    borrow::<CapyHost>(host).map_or(std::ptr::null_mut(), |host| host.surface_handle.as_raw_handle())
}
/// # Safety
/// Exclusive host access, or the UI owner while the render worker is parked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_surface_generation(host: u64) -> u64 {
    borrow::<CapyHost>(host).map_or(0,|host|host.gpu_generation)
}
/// Associate the HWND before transferring ownership to the render worker.
/// # Safety
/// Exclusive access to a live host; window remains valid for its lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_set_window(host: u64, window: *mut c_void) -> i32 {
    guard(host, |host| { host.window = window as usize; host.display_checked = None; Ok(0) })
}
/// Destroy after stopping callers and detaching the panel.
/// # Safety
/// The pointer must be uniquely owned and freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_destroy(host: u64) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(host)=borrow::<CapyHost>(host) {
            // One opt-in write after input/rendering stop, never in the pen path.
            if let Some([platform, engine]) = host.prediction_frames {
                let report = serde_json::json!({
                    "process_id": std::process::id(),
                    "window": host.window,
                    "platform_prediction_frames": platform,
                    "engine_prediction_frames": engine,
                });
                let path = format!("prediction-{}-{}.json", std::process::id(), host.window);
                let _ = std::fs::write(path, report.to_string());
            }
        }
        let _=Handle::from_raw(host).and_then(handles::release::<CapyHost>);
    }));
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_error() -> u64 {
    ERROR.with(|v| text(v.borrow().clone()))
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `wake` and its `context` must stay valid until `capy_finish_services` succeeds on the render owner.
/// The callback must tolerate concurrent worker calls and must not unwind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_start_services(
    host: u64,
    context: *mut c_void,
    wake: Option<extern "C" fn(*mut c_void)>,
) -> i32 {
    guard(host, |host| {
        if host.documents.is_none() {
            let context = context as usize;
            host.documents = Some(crate::documents::DocumentService::open_localized(host.native.session.localization(), move || {
                if let Some(wake) = wake {
                    wake(context as *mut c_void);
                }
            })?);
        }
        if host.services.is_none() {
            let context = context as usize;
            let settings = host.launch_settings.take().ok_or("Missing Windows launch preferences")?;
            host.services = Some(settings.start(&mut host.native, move || {
                if let Some(wake) = wake { wake(context as *mut c_void); }
            }));
        }
        host.documents.as_mut().unwrap().start_recovery()?;
        if host.filters.is_none() {
            let context = context as usize;
            let mut service = crate::filter_packages::FilterService::new(move || {
                if let Some(wake) = wake {
                    wake(context as *mut c_void);
                }
            });
            service.startup(&mut host.native);
            host.filters = Some(service);
        }
        if host.workspaces.is_none() {
            let context = context as usize;
            let directory = crate::storage::roots()?.workspaces();
            let worker = layer_workspace::StoreWorker::shared(&directory).map_err(err)?;
            let service =
                crate::workspace_service::WorkspaceService::new(worker, directory, host.native.session.localization().clone(), move || {
                    if let Some(wake) = wake {
                        wake(context as *mut c_void);
                    }
                });
            host.workspaces = Some(crate::workspace_owner::WorkspaceOwner::new(service)?);
            host.poll_services()?;
        }
        Ok(0)
    })
}
/// # Safety
/// Exclusive render-owner access to a live host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_needs_frame(host: u64) -> i32 {
    guard(host, |host| Ok(i32::from(host.native.dirty)))
}

/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_poll_services(host: u64) -> i32 {
    guard(host, |host| {
        host.poll_services()?;
        if let Some(presenter)=host.presenter.as_mut() {
            host.native.poll_canvas_presentation(presenter)?;
        }
        if host.surface_ready.pending()&&let Some(gpu)=host.native.session.engine().backend().0.as_ref(){
            gpu.device().poll(wgpu::PollType::Poll).map_err(err)?;
        }
        if host.surface_ready.changed(&mut host.chrome){host.native.invalidate_snapshot();}
        Ok(i32::from(host.native.dirty))
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Keep callback code and context alive until this function succeeds on the render owner.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_finish_services(host: u64) -> i32 {
    let mut owned=match borrow::<CapyHost>(host) {Ok(host)=>host,Err(error)=> {fail(error);return -1;}};
    let host=&mut *owned;
    let filters = catch_unwind(AssertUnwindSafe(|| {
        if let Some(mut service) = host.filters.take() {
            service.stop();
        }
    }))
    .map_err(|_| "Filter transport shutdown failed".to_string());
    let workspaces = catch_unwind(AssertUnwindSafe(|| {
        if let Some(service) = host.workspaces.as_mut() { service.stop()?; }
        host.workspaces = None;
        Ok(())
    }))
    .map_err(|_| "Workspace shutdown failed".to_string()).and_then(|result|result);
    // Cleanup must join the storage callback even when another host operation
    // poisoned the renderer. Do not dispatch more actions into a poisoned host.
    let documents = catch_unwind(AssertUnwindSafe(|| {
        host.documents
            .as_mut()
            .map_or(Ok(()), |service| service.stop_worker())
    }))
    .unwrap_or_else(|_| Err("Document shutdown failed".into()));
    match catch_unwind(AssertUnwindSafe(|| {
        let settings = if let Some(service) = host.services.as_mut() {
            if host.native.poisoned().is_some() {
                service.stop_worker()
            } else {
                service.finish(&mut host.native)
            }
        } else {
            Ok(())
        };
        documents.and(settings).and(workspaces).and(filters)
    })) {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            fail(error);
            -1
        }
        Err(_) => {
            fail("Preferences shutdown failed");
            -1
        }
    }
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Call from the canvas owner, before acquiring surface images.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_prepare_gpu(host: u64) -> i32 {
    guard(host, |host| {
        host.prepare_gpu()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// Exclusive access on the render owner, with no acquired image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_resize(
    host: u64,
    width: u32,
    height: u32,
    scale: f32,
) -> i32 {
    guard(host, |host| {
        if width == 0 || height == 0 || !scale.is_finite() || scale <= 0.0 {
            return Err("Invalid viewport".into());
        }
        host.target = None;
        host.scale = scale;
        host.native.resize(width, height, scale)?;
        host.native.session.set_touch_policy(touch_policy(scale))?;
        if let Some(config) = &mut host.config {
            let device = host.native
                .session
                .engine()
                .backend()
                .0
                .as_ref().map(|g| g.device())
                .or_else(|| host.documents.as_ref().and_then(|d| d.tab_device()))
                .ok_or("GPU is not prepared")?
                .clone();
            let configured = unsafe { host.surface.as_hal::<wgpu::hal::api::Dx12>() }.and_then(|surface| surface.swap_chain()).is_some();
            let allocated = if configured { [config.width, config.height] } else { [0; 2] };
            let extent = surface_extent(allocated, [width, height], device.limits().max_texture_dimension_2d)?;
            if host.surface_invalid || !configured || extent != allocated {
                [config.width, config.height] = extent;
                let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                host.surface.configure(&device, config);
                if pollster::block_on(scope.pop()).is_some() {
                    host.surface.configure(&device, config);
                }
            }
        set_composition_viewport(&host.surface, [width, height], scale)?;
        host.surface_invalid = false;
        }
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// For nonzero `count`, `records` must point to that many initialized, aligned
/// CapyPointer values, readable and unchanged throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_pointer(
    host: u64,
    records: *const CapyPointer,
    count: usize,
) -> i32 {
    guard(host, |host| {
        if count > 32768 || (count > 0 && records.is_null()) {
            return Err("Invalid pointer batch".into());
        }
        if count > 0 {
            let batch = unsafe { std::slice::from_raw_parts(records, count) };
            validate_batch(batch).map_err(err)?;
            host.poll_services()?;
            let blocked = !host.accepts_workspace_input()?;
            for sample in batch {
                let terminal = matches!(sample.phase, 3 | 4);
                let down = sample.phase == 1 && sample.flags & 1 == 0;
                if !blocked && down {
                    host.blocked_contacts.remove(&sample.id);
                }
                if blocked || host.blocked_contacts.contains(&sample.id) {
                    host.cancel_contact(sample)?;
                    if terminal {
                        host.blocked_contacts.remove(&sample.id);
                    } else if sample.phase != 0 {
                        host.blocked_contacts.insert(sample.id);
                    }
                    continue;
                }
                if down {
                    host.cancel_contact(sample)?;
                }
                if host.edge_touch(sample)? {
                    continue;
                }
                let event=sample.event();
                host.native.pointer_event(event, pointer_button(sample))?;
                if sample.flags & 1 == 0 && (event.is_finite() || terminal) {
                    host.track_contact(sample);
                }
            }
        }
        Ok(0)
    })
}
/// Workspace recovery responses are serialized with document and canvas work.
/// # Safety
/// Exclusive access to a live host; json is a readable NUL-terminated buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_action(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        use crate::workspace_service::WorkspaceAction;
        let action = unsafe { read_json(json) }?;
        if matches!(
            action,
            WorkspaceAction::PreferencesRetry
                | WorkspaceAction::PreferencesKeepOpen
                | WorkspaceAction::PreferencesDiscardClose
        ) {
            let service = host
                .services
                .as_mut()
                .ok_or("Preferences service is unavailable")?;
            match action {
                WorkspaceAction::PreferencesRetry => service.retry_close(&mut host.native)?,
                WorkspaceAction::PreferencesKeepOpen => service.keep_open(&mut host.native),
                WorkspaceAction::PreferencesDiscardClose => service.discard_close(&mut host.native),
                _ => unreachable!(),
            }
            host.poll_services()?;
            return Ok(0);
        }
        host.workspaces.as_ref().ok_or("Workspace service is unavailable")?.with(|service| {
            if let Err(error)=service.action(&mut host.native,action){service.report_error(&mut host.native,error);}Ok(())
        })?;
        host.poll_services()?;
        Ok(0)
    })
}

/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_load_filter_directory(
    host: u64,
    json: *const c_char,
) -> i32 {
    guard(host, |host| {
        let request = unsafe { with_text(json, |text| {
            if text.len() > 8192 { return Err("Filter directory request is too large.".into()); }
            serde_json::from_str(text).map_err(err)
        }) }?;
        let service = host
            .filters
            .as_mut()
            .ok_or("Filter file transport is not started.")?;
        if let Err(error) = service.load(&mut host.native, request) {
            fail(error);
            return Ok(1);
        }
        service.poll(&mut host.native);
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_action(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        let action: crate::actions::Action =
            unsafe { read_json(json) }?;
        host.poll_services()?;
        if !host.accepts_workspace_input()? && !action.allowed_while_workspace_blocked() {
            return Ok(0); // A queued widget from before a workspace transition.
        }
        if let Err(error) = action.dispatch(&mut host.native) {
            fail(error);
            return Ok(1); // A valid action can be unavailable in the current state.
        }
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_action(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        let action = unsafe { read_json(json) }?;
        let result = host.documents.as_mut().ok_or("Document service is unavailable")?.dispatch(&mut host.native, action);
        if let Err(error) = result {
            fail(error);
            return Ok(1);
        }
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_native_prediction(host: u64, available: bool) -> i32 {
    guard(host, |host| {
        host.native.session.set_platform_prediction_available(available);
        host.native.invalidate_snapshot();
        Ok(0)
    })
}
/// # Safety
/// Exclusive access to a live host; json is a readable NUL-terminated buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_input(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        let input = match unsafe { read_json(json) } {
            Ok(input) => input,
            Err(error) => { fail(error); return Ok(1); }
        };
        host.poll_services()?;
        if !host.accepts_workspace_input()?
            && !matches!(
                &input,
                layer_ui::UiInput::Blur | layer_ui::UiInput::Chrome { .. }
            )
        {
            return Ok(0);
        }
        if let layer_ui::UiInput::Chrome { facts, .. } = &input {
            host.chrome_facts = *facts;
            // This hit belongs only to that UI contact, never later canvas input.
            host.chrome_facts.contact_tab = None;
        }
        if matches!(input, layer_ui::UiInput::Blur) {
            host.cancel_contacts()?;
        }
        host.native.input(input)?;
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// Exclusive access to a live host; json is a readable NUL-terminated buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_edge_contact(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        let mut input = unsafe { read_json(json) }?;
        let layer_ui::UiInput::EdgeContact { phase, .. } = &mut input else {
            return Err("Expected an edge contact".into());
        };
        host.poll_services()?;
        if !host.accepts_workspace_input()? {
            *phase = layer_ui::ContactPhase::Cancel;
            host.native.input(input)?;
            return Ok(0);
        }
        let reply = host.native.input(input)?;
        let cursor = ["col-resize", "row-resize", "ew-resize", "ns-resize", "nwse-resize", "nesw-resize"]
            .iter()
            .position(|cursor| Some(*cursor) == reply.edge_cursor)
            .map_or(0, |index| index as i32 + 1);
        Ok(i32::from(reply.handled) | i32::from(reply.edge_hold) << 1 | i32::from(reply.edge_select_on_release) << 2
            | i32::from(reply.edge_pending) << 3 | cursor << 4 | i32::from(reply.edge_retired) << 7)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_scroll(
    host: u64,
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    density: f32,
    zoom: bool,
    horizontal: bool,
) -> i32 {
    guard(host, |host| {
        host.poll_services()?;
        if !host.accepts_workspace_input()? {
            return Ok(0);
        }
        host.native
            .scroll([x, y], [dx, dy], density, zoom, horizontal)?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_chrome(
    host: u64,
    kind: u32,
    x: f32,
    y: f32,
    canvas: bool,
    popup_open: bool,
) -> i32 {
    guard(host, |host| {
        let event = match kind {
            0 => layer_ui::ChromeEvent::Refresh,
            1 => layer_ui::ChromeEvent::Motion,
            2 => layer_ui::ChromeEvent::Contact { position: [x / host.scale, y / host.scale], canvas },
            _ => return Err("Invalid chrome event".into()),
        };
        let reply = host.native.input(layer_ui::UiInput::Chrome {
            event,
            viewport: host.native.logical,
            facts: layer_ui::ChromeFacts {
                popup_open: popup_open
                    || host.native.session.state().customization.control.is_some(),
                ..host.chrome_facts
            },
        })?;
        Ok(i32::from(reply.handled) | (i32::from(reply.dismiss_popups) << 1))
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_acquire(host: u64) -> i32 {
    guard(host, |host| {
        if let Some(timing) = &mut host.frame_timing { timing.pacing_ns=0;timing.acquisition_ns=0; }
        if host.device_is_lost() {
            return Err("The GPU device was lost".into());
        }
        host.gpu.check()?;
        if host.native.session.engine().backend().0.is_none() { return Ok(3); }
        if host.config.is_none() {
            return Ok(2);
        }
        if host.target.is_some() {
            return Ok(1);
        }
        let pacing_start = host.frame_timing.map(|_| crate::frame_timing::now());
        let ready = host.pacing.ready(host.native.session.engine().backend().0.as_ref().unwrap().device()).map_err(err)?;
        if let (Some(timing), Some(start)) = (&mut host.frame_timing, pacing_start) { timing.pacing_ns = crate::frame_timing::now() - start; }
        if !ready {
            return Ok(0);
        }
        let acquisition_start = host.frame_timing.map(|_| crate::frame_timing::now());
        host.target = match host.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(0);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                host.surface_invalid = true;
                return Ok(2);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("D3D12 surface validation failed".into());
            }
        };
        if let (Some(timing), Some(start)) = (&mut host.frame_timing, acquisition_start) { timing.acquisition_ns = crate::frame_timing::now() - start; }
        Ok(1)
    })
}
/// # Safety
/// The host remains exclusively owned on the render thread during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_localization_input(host: u64, busy: bool) -> i32 {
    guard(host, |host| {
        host.localization_input_busy = busy;
        host.poll_services()?;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_frame(host: u64, now: u64, presentation: u64) -> i32 {
    guard(host, |host| {
        if let Some(timing) = &mut host.frame_timing { timing.prepared_ns=0;timing.composed_ns=0;timing.submitted_ns=0;timing.presented_ns=0; }
        host.frame(now, presentation)
    })
}

/// # Safety
/// Exclusive render-owner access; `output` writes one initialized FrameTiming.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_frame_timing(host: u64, output: *mut crate::frame_timing::FrameTiming) -> bool {
    let Ok(host) = borrow::<CapyHost>(host) else { return false; };
    if output.is_null() { return false; }
    unsafe { *output = host.frame_timing.unwrap_or_default(); }
    host.frame_timing.is_some()
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CapySnapshotRange { offset: usize, length: usize }
impl From<Option<std::ops::Range<usize>>> for CapySnapshotRange {
    fn from(range: Option<std::ops::Range<usize>>) -> Self {
        range.map_or_else(Self::default, |range| Self { offset: range.start, length: range.len() })
    }
}
#[repr(C)]
#[derive(Default)]
pub struct CapySnapshotInfo {
    full: u32,
    workspace: u32,
    search: u32,
    layout: u32,
    camera: CapySnapshotRange,
    localization: CapySnapshotRange,
    navigation_cursor: CapySnapshotRange,
    brush_ready: CapySnapshotRange,
    glass: CapySnapshotRange,
}
impl CapySnapshotInfo {
    fn read(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let fields = layer_host::SnapshotFields::parse(bytes)?;
        let full = fields.contains("state");
        Ok(Self {
            full: u32::from(full),
            workspace: u32::from(fields.contains("workspace_update")),
            search: u32::from(fields.contains("command_search")),
            layout: u32::from(!full && fields.contains("layout")),
            camera: (!full).then(|| fields.range("camera")).flatten().into(),
            localization: fields.nonempty_object_range("localization")?.into(),
            navigation_cursor: fields.range("navigation_cursor").into(),
            brush_ready: fields.range("brush_ready").into(),
            glass: fields.range("windows_glass").into(),
        })
    }
}
fn snapshot_layout_input<'a>(bytes:&[u8],host:&'a NativeHost)->Result<Option<layer_ui::LayoutResolveInput<'a>>,String> {
    Ok(layer_host::SnapshotFields::parse(bytes).map_err(err)?.contains("layout").then(||host.session.layout_resolve_input()))
}
#[derive(serde::Serialize)]
struct WindowsMetadata<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    localization: Option<serde_json::Value>,
    windows_gpu_generation: u64,
    windows_active_tag: &'static str,
    windows_display: serde_json::Value,
    windows_rendering_suspended: bool,
    windows_surface_ready: bool,
    windows_filter_load: Option<crate::filter_packages::Status>,
    windows_document: Option<serde_json::Value>,
    windows_proof: Option<layer_ui::proof_workflow::ProofStatus>,
    windows_proof_form: serde_json::Value,
    windows_recovery: Option<serde_json::Value>,
    windows_tabs: Option<serde_json::Value>,
    windows_palettes: Option<serde_json::Value>,
    windows_papers: Option<serde_json::Value>,
    windows_glass: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout_input: Option<layer_ui::LayoutResolveInput<'a>>,
    windows_isolated_settings: bool,
    windows_adapter: Option<String>,
    windows_workspace: Option<serde_json::Value>,
    windows_settings_close: Option<crate::settings::CloseStatus>,
    windows_scopes: u64,
    windows_request_copy: serde_json::Map<String, serde_json::Value>,
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `info` must be null or aligned, writable and exclusively borrowed for this call.
/// Its fields reset before host validation; null output never contains ranges.
/// Ranges address the returned UTF-8 allocation only until `capy_string_free`.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_snapshot(host: u64, info: *mut CapySnapshotInfo) -> u64 {
    if let Some(info) = unsafe { info.as_mut() } { *info = CapySnapshotInfo::default(); }
    let mut result = 0;
    guard(host, |host| {
        if host.surface_ready.changed(&mut host.chrome) { host.native.invalidate_snapshot(); }
        let generation = host.native.localization_generation();
        let localization = if host.localization_generation != Some(generation) {
            Some(crate::settings::localization_presentation(&mut host.native)?)
        } else { None };
        let mut metadata = WindowsMetadata {
            localization,
            layout_input: None,
            windows_gpu_generation: host.gpu_generation,
            windows_active_tag: host.native.session.localization().language().tag(),
            windows_display: serde_json::json!({"output": host.display, "format": host.config.as_ref().map(|c| format!("{:?}", c.format)), "headroom": host.config.as_ref().map_or(1., |c| host.display.available_headroom(c.format)), "analysis": host.documents.as_ref().map(|s| s.tone.status())}),
            windows_rendering_suspended: host.native.session.rendering_suspended(),
            windows_surface_ready: host.surface_ready.ready(),
            windows_recovery: host.documents.as_ref().and_then(|service|service.recovery_status(&host.native)),
            windows_tabs: host.documents.as_ref().map(|d| d.tabs_view(&host.native)),
            windows_palettes: host.documents.as_ref().map(|d| d.palettes.status()),
            windows_papers: host.documents.as_ref().map(|d| d.papers.status()),
            windows_glass: serde_json::json!({
                "regions": host.chrome.glass.count(),
                "chrome_revision": host.chrome.presented_revision,
                "document": host.chrome.presented_document,
                "frames": host.presenter.as_ref().map_or([0; 2], |p| p.backdrop_frames()),
            }),
            windows_document: host.documents.as_ref().and_then(|service| service.status()),
            windows_proof_form: host.documents.as_ref().map(|service|service.proof.form(&host.native)).unwrap_or_else(||serde_json::json!(layer_ui::proof_workflow::proof_copy(&host.native.session))),
            windows_proof: host
                .documents
                .is_some()
                .then(|| host.native.proof.observe(&host.native.session)),
            windows_workspace: host.workspaces.as_ref().map(|owner|owner.with(|service|serde_json::to_value(service.status(&host.native)).map_err(err))).transpose()?,
            windows_settings_close: host.services.as_ref().map(|s| s.close_status().clone()),
            windows_scopes: host.scopes.revision(host.native.session.state()),
            windows_request_copy: {
                let localization = host.native.session.localization();
                host.native.session.state().requests.iter().filter_map(|request| match &request.kind {
                    layer_ui::HostRequestKind::Document { request: document } => Some((request.id.to_string(), serde_json::json!({
                        "title": document.title(localization), "accept_label": document.accept_label(localization), "filter": document.filter(localization),
                    }))),
                    _ => None,
                }).collect()
            },
            windows_filter_load: host.filters.as_ref().map(|s| s.status().clone()),
            windows_isolated_settings: crate::storage::isolated(),
            windows_adapter: host.native.session.engine().backend().0.as_ref().map(|gpu| {
                let info = gpu.adapter().get_info();
                format!("{} ({:?})", info.name, info.device_type)
            }),
        };
        let glass_changed=host.published_chrome_revision!=host.chrome.presented_revision||host.published_glass_count!=host.chrome.glass.count();
        let update=host.native.take_layout_update_bytes().map_err(err)?;
        let bytes=match update {
            Some(bytes)=>{
                metadata.layout_input=snapshot_layout_input(&bytes,&host.native)?;
                let extension=serde_json::to_vec(&metadata).map_err(err)?;
                Some(layer_host::extend_update(bytes,&extension))
            },
            None if glass_changed=>Some(serde_json::to_vec(&serde_json::json!({
                "windows_glass":{"regions":host.chrome.glass.count(),"frames":host.presenter.as_ref().map_or([0;2],|p|p.backdrop_frames()),
                    "chrome_revision":host.chrome.presented_revision,"document":host.chrome.presented_document},
            })).map_err(err)?),
            None=>None,
        };
        if let Some(bytes) = bytes {
            host.localization_generation = Some(generation);
            host.published_chrome_revision=host.chrome.presented_revision;
            host.published_glass_count=host.chrome.glass.count();
            let routing = if info.is_null() { None } else { Some(CapySnapshotInfo::read(&bytes).map_err(err)?) };
            let snapshot = CString::new(bytes).map_err(err)?;
            if let Some(routing) = routing { unsafe { *info = routing; } }
            result = text(snapshot);
        }
        Ok(0)
    });
    result
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_query(host: u64, json: *const c_char) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        let query = unsafe { read_json(json) }?;
        result = CString::new(host.native.query(query)?.to_string())
            .map(text).map_err(err)?;
        Ok(0)
    });
    result
}
/// Diagnostic identity for app-only ETW correlation. Never treat submission
/// metadata or this steady-content probe as physical input/display timing.
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
/// A nonnull result belongs to the caller and must be freed with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_surface_info(host: u64) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        use windows::core::Interface;
        let native = unsafe { host.surface.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or("Missing D3D12 surface")?;
        let swapchain = native.swap_chain().ok_or("Missing DXGI swap chain")?;
        let base = swapchain
            .cast::<windows::Win32::Graphics::Dxgi::IDXGISwapChain>()
            .map_err(err)?;
        let config = host
            .config
            .as_ref()
            .ok_or("Missing surface configuration")?;
        let info = serde_json::json!({
            "scope": "steady_canvas_presentation_probe",
            "swap_chain_addresses": [
                format!("{:#x}", base.as_raw() as usize),
                format!("{:#x}", swapchain.as_raw() as usize)
            ],
            "viewport": host.native.session.state().camera.viewport,
            "buffer_extent": [config.width, config.height],
            "density": host.scale,
            "present_mode": format!("{:?}", config.present_mode),
            "format": format!("{:?}", config.format),
            "maximum_frame_latency": config.desired_maximum_frame_latency
        });
        result = CString::new(info.to_string()).map(text).map_err(err)?;
        Ok(0)
    });
    result
}
/// Binary, display-only comparison pixels for a prepared document candidate.
/// # Safety
/// The host is exclusively owned by the caller; json is a readable C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_preview(host: u64, json: *const c_char) -> u64 {
    let mut packet = 0;
    guard(host, |host| {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Query { id: u32, index: usize }
        let query: Query = unsafe { read_json(json) }?;
        let result = host.documents.as_ref().ok_or("Documents unavailable")?.preview(query.id, query.index)?;
        packet = crate::previews::store(result)?;
        Ok(0)
    });
    packet
}
/// Stateless shared numeric policy; safe on the UI thread without a host.
/// # Safety
/// `json` must be null or a readable NUL-terminated buffer that remains unchanged
/// for the duration of this call. The immutable context must be live and
/// unmodified. Free a nonnull result with `capy_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_number(context: u64, json: *const c_char) -> u64 {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<CString, String> {
        let request: layer_ui::NumericRequest =
            unsafe { read_json(json) }?;
        let context = crate::shared_controls::localization(context).ok_or("Missing numeric localization")?;
        let value = match request.resolve() {
            Ok(value) => serde_json::to_value(value).map_err(err)?,
            Err(reason) => serde_json::json!({"error":reason.message(&context.localizer),"error_reason":reason}),
        };
        CString::new(serde_json::to_string(&value).map_err(err)?).map_err(err)
    }));
    match result {
        Ok(Ok(value)) => text(value),
        Ok(Err(error)) => {
            fail(error);
            0
        }
        Err(_) => {
            fail("Numeric policy panic");
            0
        }
    }
}
/// # Safety
/// `host` must be null or a live host exclusively accessed by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_suspend(host: u64) -> i32 {
    guard(host, |host| {
        host.target = None;
        Ok(0)
    })
}
/// # Safety
/// `host` must be null or a valid host with no concurrent mutation or destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_view_revision(host: u64) -> u64 {
    borrow::<CapyHost>(host).map_or(0, |h| h.native.session.state().camera.revision)
}

// The backbuffer is sized in physical pixels. SwapChainPanel otherwise scales
// it again by the XAML composition scale, displacing both the image and input.
fn set_composition_viewport(surface: &wgpu::Surface<'_>, viewport: [u32; 2], scale: f32) -> Result<(), String> {
    let native = unsafe { surface.as_hal::<wgpu::hal::api::Dx12>() }
        .ok_or("The Windows surface has no D3D12 backend")?;
    let swapchain = native
        .swap_chain()
        .ok_or("The Windows swap chain is not configured")?;
    let extent = unsafe { swapchain.GetDesc1() }.map_err(err)?;
    let transform = windows::Win32::Graphics::Dxgi::DXGI_MATRIX_3X2_F {
        _11: viewport[0] as f32 / extent.Width as f32 / scale,
        _22: viewport[1] as f32 / extent.Height as f32 / scale,
        ..Default::default()
    };
    unsafe {
        swapchain.SetSourceSize(viewport[0], viewport[1]).map_err(err)?;
        swapchain.SetMatrixTransform(&transform)
    }.map_err(err)
}

fn bundled_shader_compiler() -> wgpu::Dx12Compiler {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("dxcompiler.dll"))
        .filter(|path| path.is_file())
        .and_then(|path| path.to_str().map(str::to_owned))
        .map_or(wgpu::Dx12Compiler::Fxc, |dxc_path| {
            wgpu::Dx12Compiler::DynamicDxc { dxc_path }
        })
}

/// # Safety
/// Exclusive host access, or the UI owner while the render worker is parked.
/// Poisoned hosts cannot resume after an ABI panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_device_lost(host: u64) -> bool {
    borrow::<CapyHost>(host).is_ok_and(|host| host.native.poisoned().is_none() && host.device_is_lost())
}

/// # Safety
/// Exclusive render-owner access, with no acquired image.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_reset_surface(host: u64) -> i32 {
    guard(host, |host| {
        if host.target.is_some() {
            return Err("Surface replacement requires no acquired image".into());
        }
        let (surface, handle) = CapyHost::composition_surface(&host.instance)?;
        host.surface = surface;
        host.surface_handle = handle;
        host.surface_ready=Default::default();
        host.native.reset_canvas_frame_rate();
        host.config = None;
        host.blank_presented = false;
        Ok(0)
    })
}

/// # Safety
/// Exclusive render-owner access. Available only in an isolated smoke-test host.
/// `json` is a readable NUL-terminated pair of group token and window count.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_test_device_loss(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        if std::env::var_os("CAPY_SMOKE_TEST").is_none() || !crate::storage::isolated() {
            return Err("Device-loss injection requires an isolated smoke test".into());
        }
        let (token, participants) = unsafe { read_json(json) }?;
        host.target = None;
        let gpu = host
            .native
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .ok_or("GPU is not ready")?;
        gpu.device().poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        }).map_err(err)?;
        // Remove the D3D12 device shared by this process's windows on the adapter.
        // The COM object remains owned by wgpu. Ordinary loss detection must
        // observe removal; this hook never sets the flag or resets the adapter.
        {
            use windows::{Win32::Graphics::Direct3D12::{ID3D12Device5, ID3D12Fence, D3D12_FENCE_FLAG_NONE}, core::Interface};
            let native = unsafe { gpu.device().as_hal::<wgpu::hal::api::Dx12>() }
                .ok_or("Device removal requires D3D12")?;
            let device: ID3D12Device5 = native.raw_device().cast().map_err(err)?;
            let presented: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.map_err(err)?;
            unsafe { native.raw_queue().Signal(&presented, 1) }.map_err(err)?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while unsafe { presented.GetCompletedValue() } < 1 {
                if std::time::Instant::now() >= deadline {
                    return Err("Device-loss injection timed out waiting for presentation".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            crate::device_loss_test::together(token, participants, || {
                unsafe { device.RemoveDevice() };
                Ok(())
            })?;
        }
        // Drop the HAL guard before polling wgpu's device-loss notification.
        let _ = gpu.device().poll(wgpu::PollType::Poll);
        Ok(0)
    })
}

/// # Safety
/// Exclusive render-owner access after canvas input has stopped. Unsubmitted
/// contacts are canceled; keep services alive until approved close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_suspend_renderer(host: u64) -> i32 {
    guard(host, |host| {
        host.target = None;
        host.cancel_contacts()?;
        host.native.suspend_renderer()?;
        host.native.reset_canvas_frame_rate();
        host.presenter = None;
        host.surface_ready = Default::default();
        host.chrome.reset_presented();
        drop(host.native.session.renderer_mut().0.take());
        host.config = None;
        if let Some(service) = host.documents.as_mut() {
            service.renderer_unavailable(&mut host.native)?;
        }
        if let Some(mut service) = host.filters.take() {
            service.stop();
        }
        host.native.invalidate_snapshot();
        Ok(0)
    })
}

/// # Safety
/// `host` must be null or a live exclusively accessed host. `json` must be null
/// or a readable, unchanged NUL-terminated buffer throughout the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_glass(host: u64, json: *const c_char) -> i32 {
    guard(host, |host| {
        let layout = unsafe { with_text(json, |text| Ok(serde_json::from_str(text))) }?
            .map_err(|_| "Invalid glass geometry".to_string());
        match layout.and_then(|layout| host.chrome.set(layout)) {
            Ok(changed) => { host.native.dirty |= changed; Ok(0) },
            Err(error) => {
                fail(error);
                Ok(1)
            }
        }
    })
}

/// # Safety
/// Exclusive render-owner access to host; json is a readable NUL-terminated
/// buffer. Transfer/free a nonnull CPU packet using capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews(
    host: u64,
    json: *const c_char,
) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        let packet = unsafe { with_text(json, |text| crate::previews::query(&mut host.native, text)) }?;
        result = crate::previews::store(packet)?;
        Ok(0)
    });
    result
}

/// Called once by the process that owns this installation, before any window.
#[unsafe(no_mangle)]
pub extern "C" fn capy_clear_temporary_files() {
    if let Err(error) = catch_unwind(|| crate::storage::roots().and_then(layer_host::StorageRoots::clear_temp))
        .unwrap_or_else(|_| Err("Temporary file cleanup panic".into()))
    {
        eprintln!("{error}");
    }
}

/// The NUL-terminated name every launch using this installation's storage
/// shares, or null when its storage is unavailable (see `capy_error`).
#[unsafe(no_mangle)]
pub extern "C" fn capy_instance_name() -> *const u16 {
    match catch_unwind(crate::storage::instance_name) {
        Ok(Ok(name)) => name.as_ptr(),
        Ok(Err(error)) => {
            fail(error);
            std::ptr::null()
        }
        Err(_) => {
            fail("Storage lookup panic");
            std::ptr::null()
        }
    }
}

/// Replaces the file at `path` only after `bytes` are durable beside it.
/// # Safety
/// Call on a worker. `path` is readable NUL-terminated UTF-8 and `bytes`
/// points to `length` readable bytes; both stay unchanged during the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_write_file(path: *const c_char, bytes: *const u8, length: usize) -> i32 {
    if length>isize::MAX as usize || (length>0 && bytes.is_null()) {
        fail("Invalid file output buffer");return -1;
    }
    let write = |path: &str| {
        let path = std::path::Path::new(path);
        let bytes = if length == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(bytes, length) } };
        crate::document_io::atomic_write(path, &std::sync::atomic::AtomicBool::new(false), |file| {
            file.write_all(bytes).map_err(err)
        })
    };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe { with_text(path, write) }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            fail(error);
            -1
        }
        Err(_) => {
            fail("File write panic");
            -1
        }
    }
}

/// Join retired shader workers after every host is destroyed, before process
/// runtime teardown. Ordinary surface/document replacement stays asynchronous.
#[unsafe(no_mangle)]
pub extern "C" fn capy_finish_process() -> i32 {
    match catch_unwind(layer_render_wgpu::finish_shader_compiler_shutdown) {
        Ok(()) => 0,
        Err(_) => {
            fail("Shader compiler shutdown panic");
            -1
        }
    }
}

/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layer_menu(
    host: u64,
    json: *const c_char,
) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        result = crate::previews::store(unsafe {with_text(json,|text|crate::previews::layer_menu(&mut host.native,text))}?)?;
        Ok(0)
    });
    result
}
/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layer_thumbnails(
    host: u64,
    json: *const c_char,
) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        result = crate::previews::store(unsafe {with_text(json,|text|crate::previews::layer_thumbnails(&mut host.native,text))}?)?;
        Ok(0)
    });
    result
}

/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_workspace_query(
    host: u64,
    json: *const c_char,
) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        result = crate::previews::store(unsafe {with_text(json,|text|crate::workspace::query(&mut host.native,text))}?)?;
        Ok(0)
    });
    result
}

/// Synthetic capability changes exercise native presentation, never hardware acceptance.
/// # Safety
/// Exclusive access to the render-owned host. Only an isolated smoke test can call it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_test_display(host: u64, hdr: bool) -> i32 {
    guard(host, |host| {
        if std::env::var_os("CAPY_SMOKE_TEST").is_none() || std::env::var_os("CAPY_TEST_HDR").is_none()
            || !crate::storage::isolated() {
            return Err("Display injection requires an isolated HDR smoke test".into());
        }
        host.test_display = Some(crate::display::Display::reported(hdr, if hdr { 1015. } else { 80. }, "Synthetic test output".into()));
        host.display_checked = None;
        host.poll_services()?;
        Ok(0)
    })
}

/// Optional read-only presentation counters. A failed query is diagnostic only;
/// it must not change document state or stop the renderer.
/// # Safety
/// `host` is exclusively owned by the render thread; `values` writes six u64s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_presentation_stats(host: u64, values: *mut u64) -> i32 {
    let Ok(host)=borrow::<CapyHost>(host) else {return -1;};
    if values.is_null() { return -1; }
    let Some(native) = (unsafe { host.surface.as_hal::<wgpu::hal::api::Dx12>() }) else { return -1; };
    let Some(swapchain) = native.swap_chain() else { return -1; };
    let output = unsafe { &mut *values.cast::<[u64; 6]>() };
    *output = [0; 6];
    let last = match unsafe { swapchain.GetLastPresentCount() } {
        Ok(value) => value, Err(error) => return error.code().0,
    };
    output[0] = u64::from(last);
    let mut stats = windows::Win32::Graphics::Dxgi::DXGI_FRAME_STATISTICS::default();
    if let Err(error) = unsafe { swapchain.GetFrameStatistics(&mut stats) } { return error.code().0; }
    output[1..].copy_from_slice(&[u64::from(stats.PresentCount), u64::from(stats.PresentRefreshCount),
        u64::from(stats.SyncRefreshCount), stats.SyncQPCTime as u64, stats.SyncGPUTime as u64]);
    0
}

/// # Safety
/// Exclusive render-owner access; json is a readable NUL-terminated request.
/// Free the returned CPU-only packet with capy_preview_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_previews(host: u64, json: *const c_char) -> u64 {
    let mut result = 0;
    guard(host, |host| {
        let packet = unsafe { with_text(json, |text| crate::previews::paper_previews(&host.native, text)) }?;
        result = crate::previews::store(packet)?;
        Ok(0)
    });
    result
}

#[cfg(test)]
mod presentation_readiness_tests {
    use super::*;

    #[test]
    #[ignore = "Requires hardware D3D12"]
    fn acquired_target_waits_for_drawable_background_without_optional_chrome() {
        use std::time::{Duration, Instant};
        let viewport=[1024,768];let extent=[1088,832];
        for (format,color) in [(wgpu::TextureFormat::Bgra8UnormSrgb,layer_render_wgpu::SdrSurfaceColor::Srgb),
            (wgpu::TextureFormat::Bgra8Unorm,layer_render_wgpu::SdrSurfaceColor::Srgb),
            (wgpu::TextureFormat::Rgba16Float,layer_render_wgpu::SdrSurfaceColor::ExtendedLinearSrgb)] {
            let mut renderer = crate::test_support::d3d12_renderer();
            let mut presenter = ViewportPresenter::for_surface(&renderer, format, color).unwrap();
            let mut target = Some(format);
            assert_eq!(take_prepared_target(&presenter, &renderer, &mut target), None);
            assert_eq!(target, Some(format));
            let deadline = Instant::now() + Duration::from_secs(30);
            while !presenter.pipelines_ready(&renderer) {
                renderer.poll_startup().unwrap();
                assert_eq!(target, Some(format));
                assert!(Instant::now() < deadline, "presenter pipelines did not finish");
                std::thread::yield_now();
            }
            assert_eq!(take_prepared_target(&presenter,&renderer,&mut target),None);
            assert_eq!(target,Some(format));
            let mut native=NativeHost::new(layer_ui::Platform::Windows).unwrap();
            let document=layer_core::Document::new(layer_core::authored::PortableId::random(),96,96,
                layer_core::DocumentNames{paint:"Current ink".into(),paper:"Paper".into()});
            *native.session=layer_ui::UiSession::new(layer_host::Renderer::default(),document,viewport,layer_ui::Platform::Windows).unwrap();
            native.resize(viewport[0],viewport[1],1.).unwrap();
            native.replace_renderer(layer_host::Renderer(Some(renderer.into()))).unwrap();
            native.startup=Default::default();
            let deadline=Instant::now()+Duration::from_secs(30);
            loop {
                native.prepare_canvas_frame(0,0,false).unwrap();
                let gpu=native.session.engine().backend().0.as_ref().unwrap();
                if let Some(acquired)=take_prepared_target(&presenter,gpu,&mut target){assert_eq!(acquired,format);break;}
                assert_eq!(target,Some(format));
                assert!(Instant::now()<deadline,"first constant background did not become drawable");
                std::thread::yield_now();
            }
            assert!(!presenter.chrome_ready());
            assert!(!native.startup.brush_ready);
            let mut readiness=SurfaceReadiness::default();
            assert!(!readiness.ready());assert!(!readiness.changed(&mut crate::chrome::Chrome::default()));
            let gpu=native.session.engine().backend().0.as_ref().unwrap();
            let output=gpu.device().create_texture(&wgpu::TextureDescriptor {
                label:Some("first drawable background"),size:wgpu::Extent3d{width:extent[0],height:extent[1],depth_or_array_layers:1},
                mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format,
                usage:wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::COPY_SRC,view_formats:&[],
            });
            let view=native.session.state().camera.view();let [a,b,c,d,tx,ty]=view.document_to_surface;
            let paper=[(a*48.+c*48.+tx).floor() as usize,(b*48.+d*48.+ty).floor() as usize];
            assert!(paper[0]<viewport[0] as usize&&paper[1]<viewport[1] as usize,"paper center outside fixture viewport: {view:?}");
            presenter.present(gpu,&output.create_view(&Default::default()),view,[0.25,0.25,0.25,1.]).unwrap();
            readiness.submitted(gpu.queue(), &crate::chrome::Chrome::default(), None);assert!(readiness.queued);
            let pixel:u32=if format==wgpu::TextureFormat::Rgba16Float{8}else{4};
            let row=extent[0]*pixel;
            let stride=row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)*wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let buffer=gpu.device().create_buffer(&wgpu::BufferDescriptor {
                label:Some("first background pixels"),size:u64::from(stride)*u64::from(extent[1]),
                usage:wgpu::BufferUsages::COPY_DST|wgpu::BufferUsages::MAP_READ,mapped_at_creation:false,
            });
            let mut encoder=gpu.device().create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(output.as_image_copy(),wgpu::TexelCopyBufferInfo {
                buffer:&buffer,layout:wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(stride),rows_per_image:Some(extent[1])},
            },output.size());
            gpu.queue().submit([encoder.finish()]);
            let (send,receive)=std::sync::mpsc::sync_channel(1);
            buffer.slice(..).map_async(wgpu::MapMode::Read,move|result|send.send(result).unwrap());
            gpu.device().poll(wgpu::PollType::Wait{timeout:Some(Duration::from_secs(30)),submission_index:None}).unwrap();
            receive.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
            assert!(readiness.ready());assert!(readiness.changed(&mut crate::chrome::Chrome::default()));assert!(!readiness.changed(&mut crate::chrome::Chrome::default()));
            let retired=std::mem::take(&mut readiness);retired.ready.store(true,std::sync::atomic::Ordering::Release);
            assert!(!readiness.ready());assert!(!readiness.changed(&mut crate::chrome::Chrome::default()));
            let pixels=buffer.slice(..).get_mapped_range().unwrap();
            for (y,row) in pixels.chunks_exact(stride as usize).enumerate(){
                for (x,rgba) in row[..extent[0] as usize*pixel as usize].chunks_exact(pixel as usize).enumerate(){
                    let padding=x>=viewport[0] as usize||y>=viewport[1] as usize;
                    if !padding&&[x,y]!=paper{continue;}
                    let value=if padding{0.25}else{1.};
                    if format==wgpu::TextureFormat::Rgba16Float {
                        let expected=layer_core::color::f16::from_f32(value).to_bits().to_le_bytes();
                        assert!(rgba[..6].as_chunks::<2>().0.iter().all(|channel|*channel==expected),"{format:?} {color:?} at {x},{y}, padding={padding}, actual={rgba:?}, expected={expected:?}, view={view:?}");assert_eq!(&rgba[6..],&[0,60]);
                    }else{
                        let expected=(layer_core::color::RgbSpace::Srgb.encode(f64::from(value))*255.).round() as u8;
                        assert!(rgba[..3].iter().all(|channel|channel.abs_diff(expected)<=1),"{format:?} {color:?} at {x},{y}, padding={padding}, actual={rgba:?}, expected={expected}, view={view:?}");assert_eq!(rgba[3],255);
                    }
                }
            }
            drop(pixels);buffer.unmap();
            assert_eq!(target, None);
            assert_eq!(take_prepared_target(&presenter,native.session.engine().backend().0.as_ref().unwrap(),&mut target),None);
        }
    }
}

#[cfg(test)]
mod surface_extent_tests {
    use super::surface_extent;

    #[test]
    fn live_resize_retains_capacity_and_admits_new_viewports() {
        let initial = surface_extent([0; 2], [1700, 1150], 8192).unwrap();
        assert_eq!(initial, [1792, 1152]);
        for width in 1340..=1792 {
            assert_eq!(surface_extent(initial, [width, 1100], 8192).unwrap(), initial);
        }
        assert_eq!(surface_extent(initial, [1793, 1153], 8192).unwrap(), [1920, 1280]);
        assert_eq!(surface_extent([0; 2], [1000, 900], 1000).unwrap(), [1000, 1000]);
        for viewport in [[0, 100], [100, 0], [8193, 100], [100, 8193]] {
            assert!(surface_extent(initial, viewport, 8192).is_err());
        }
    }
}

#[cfg(test)]
mod snapshot_routing_tests {
    use super::*;
    #[test]
    fn snapshot_descriptor_preserves_null_presence_and_owned_ranges() {
        let bytes = br#"{"state":null,"workspace_update":null,"command_search":null,"brush_ready":null,"camera":{"zoom":2},"localization":{"text":"\u65e5"},"navigation_cursor":"pan"}"#;
        let info = CapySnapshotInfo::read(bytes).unwrap();
        assert_eq!((info.full, info.workspace, info.search), (1,1,1));
        assert_eq!(info.camera.length,0);
        assert_eq!(&bytes[info.brush_ready.offset..info.brush_ready.offset+info.brush_ready.length],b"null");
        let bytes = br#"{"workspace_update":{},"camera":{"zoom":2},"localization":null}"#;
        let info = CapySnapshotInfo::read(bytes).unwrap();
        let camera = &bytes[info.camera.offset..info.camera.offset+info.camera.length];
        assert_eq!(serde_json::from_slice::<serde_json::Value>(camera).unwrap(),serde_json::json!({"zoom":2}));
        assert_eq!(info.localization.length,0);
        assert_eq!(info.brush_ready.length,0);
        assert_eq!(info.layout,0);
        let info=CapySnapshotInfo::read(br#"{"workspace_update":{},"layout":{},"camera":{"zoom":2}}"#).unwrap();
        assert_eq!((info.full,info.workspace,info.layout),(0,1,1));
        assert_eq!(CapySnapshotInfo::read(br#"{"state":{},"layout":{}}"#).unwrap().layout,0);
        for bytes in [b"{}".as_slice(),b"{\"localization\":{}}",b"{\"localization\":null}"] {
            assert_eq!(CapySnapshotInfo::read(bytes).unwrap().localization.length,0);
        }
        let a=std::mem::align_of::<usize>();
        let first=(4*std::mem::size_of::<u32>()+a-1)&!(a-1);
        assert_eq!(std::mem::offset_of!(CapySnapshotInfo,camera),first);
        assert_eq!(std::mem::offset_of!(CapySnapshotInfo,brush_ready),first+6*std::mem::size_of::<usize>());
        assert_eq!(std::mem::offset_of!(CapySnapshotInfo,glass),first+8*std::mem::size_of::<usize>());
        assert_eq!(std::mem::size_of::<CapySnapshotInfo>(),first+10*std::mem::size_of::<usize>());
        for bytes in [br#"{"camera":{},"windows_glass":{"chrome_revision":3}}"#.as_slice(),
            br#"{"command_search":null,"windows_glass":{"chrome_revision":3}}"#] {
            let info=CapySnapshotInfo::read(bytes).unwrap();
            assert_eq!(&bytes[info.glass.offset..info.glass.offset+info.glass.length],br#"{"chrome_revision":3}"#);
        }
    }
    #[test]
    fn snapshot_layout_input_is_windows_geometry_only() {
        let host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        for bytes in [br#"{"state":{},"layout":{}}"#.as_slice(),br#"{"workspace_update":{},"layout":{}}"#] {
            let input=snapshot_layout_input(bytes,&host).unwrap().unwrap();
            assert_eq!(serde_json::json!(input.resolve([1400.,900.])),serde_json::json!(host.session.layout([1400.,900.])));
        }
        for bytes in [br#"{"workspace_update":{}}"#.as_slice(),br#"{"camera":{}}"#,br#"{"command_search":null}"#,br#"{"windows_glass":{}}"#] {
            assert!(snapshot_layout_input(bytes,&host).unwrap().is_none());
        }
    }
    #[test]
    fn snapshot_error_resets_descriptor_without_a_host() {
        let mut info=CapySnapshotInfo::read(br#"{"state":{},"brush_ready":true,"localization":{"text":"x"}}"#).unwrap();
        assert_eq!(unsafe { capy_snapshot(0,&mut info) },0);
        assert_eq!((info.full,info.workspace,info.search,info.layout),(0,0,0,0));
        assert!(unsafe { capy_surface_handle(0) }.is_null());
        for range in [info.camera,info.localization,info.navigation_cursor,info.brush_ready,info.glass] {
            assert_eq!((range.offset,range.length),(0,0));
        }
        assert_eq!(unsafe { capy_snapshot(0,std::ptr::null_mut()) },0);
    }
}

#[cfg(test)]
mod numeric_localization_tests {
    use super::*;
    use layer_ui::{Localizer, MessageId, NumericControl, NumericError, UiLanguage};
    #[test]
    fn numeric_abi_retains_known_failure_reason_and_formats_current_context() {
        let control = NumericControl::percent();
        let input = CString::new(serde_json::json!({"control":control,"value":0.5,
            "operation":{"type":"expression","text":""}}).to_string()).unwrap();
        let format = CString::new(serde_json::json!({"control":control,"value":0.5,
            "operation":{"type":"format"}}).to_string()).unwrap();
        for language in UiLanguage::ALL {
            let context = crate::test_support::Localization::new(Localizer::shared(language));
            let result = unsafe { capy_number(context.raw(), input.as_ptr()) };
            assert_ne!(result,0);
            let value: serde_json::Value = serde_json::from_slice(crate::owned::take_text(result).as_bytes()).unwrap();
            assert_eq!(value["error"], context.localizer.text(MessageId::NUMERIC_EXPRESSION_REQUIRED).as_ref());
            assert_eq!(value["error_reason"], serde_json::to_value(NumericError::ExpressionRequired).unwrap());
            let request = CString::new(serde_json::json!({"type":"numeric_error","reason":value["error_reason"]}).to_string()).unwrap();
            let projected = unsafe { crate::shared_controls::capy_native_caption(context.raw(), request.as_ptr()) };
            assert_ne!(projected,0);
            let projected: serde_json::Value = serde_json::from_slice(crate::owned::take_text(projected).as_bytes()).unwrap();
            assert_eq!(projected["text"],value["error"]);
            let result = unsafe { capy_number(context.raw(), format.as_ptr()) };
            assert_ne!(result,0);
            let value: serde_json::Value = serde_json::from_slice(crate::owned::take_text(result).as_bytes()).unwrap();
            assert_eq!(value["value"],0.5);
            assert!(value.get("error").is_none());
        }
        let malformed = CString::new("literal İı ไทย { $reason }").unwrap();
        let context = crate::test_support::Localization::new(Localizer::shared(UiLanguage::English));
        assert_eq!(unsafe { capy_number(context.raw(), malformed.as_ptr()) },0);
        assert_eq!(unsafe { capy_number(0, format.as_ptr()) },0);
    }
}

const _:fn()=|| {fn send<T:Send>(){}send::<CapyHost>();send::<CapyLaunch>();};

#[cfg(test)]
mod handle_tests {
    use super::*;

    struct Owner(u64);
    impl Owner {
        fn new(width: u32, height: u32) -> Self {
            let native=NativeHost::new(layer_ui::Platform::Windows).unwrap();
            let mut launch=CapyLaunch {native:Some(native),settings:None,bootstrap:CString::default()};
            let host=unsafe {CapyHost::new(width,height,1.,&mut launch)}.unwrap();
            Self(handles::insert(host).unwrap().raw())
        }
        fn blocked(&self, blocked: bool) {
            let mut host=borrow::<CapyHost>(self.0).unwrap();
            if blocked {
                if host.services.is_none() {
                    let (_,settings)=crate::settings::SettingsService::launch(&[],None).unwrap();
                    host.services=Some(settings.start(&mut host.native,||{}));
                }
                host.native.session.request_document_close().unwrap();
            } else {
                host.native.session.reset_document_close();
            }
            host.poll_services().unwrap();
            assert_eq!(host.accepts_workspace_input().unwrap(),!blocked);
        }
        fn edge(&self, id: u64, phase: &str, position: [f32; 2], ms: u64) -> i32 {
            self.edge_as(id,phase,"touch",position,ms)
        }
        fn edge_as(&self, id: u64, phase: &str, kind: &str, position: [f32; 2], ms: u64) -> i32 {
            let input=CString::new(serde_json::json!({"type":"edge_contact","id":id,"phase":phase,"kind":kind,
                "contact":"free","position":position,"viewport":[1600,1000],"time_ns":ms*1_000_000}).to_string()).unwrap();
            unsafe {capy_edge_contact(self.0,input.as_ptr())}
        }
        fn pointer(&self, samples: &[CapyPointer]) {
            assert_eq!(unsafe {capy_pointer(self.0,samples.as_ptr(),samples.len())},0);
        }
        fn layout(&self) -> layer_ui::DockLayout {
            borrow::<CapyHost>(self.0).unwrap().native.session.state().workspace.layout.clone()
        }
    }
    impl Drop for Owner { fn drop(&mut self) { unsafe {capy_destroy(self.0);} } }

    #[test]
    fn native_layer_taps_use_physical_slop_after_dpi_changes() {
        for scale in [1., 1.5, 2., 1.] {
            for moved in [false, true] {
                let owner = Owner::new(1600, 1000);
                assert_eq!(unsafe { capy_resize(owner.0, 1600, 1000, scale) }, 0);
                let row = borrow::<CapyHost>(owner.0).unwrap().native.session.state().layers.iter().find(|row| row.editing).unwrap().id;
                let slop = touch_policy(scale).slop;
                for (phase, dx, ms) in [("down", 0., 0), ("move", if moved { slop + 1. } else { 0. }, 30), ("up", if moved { slop + 1. } else { 0. }, 60)] {
                    for id in [1, 2] {
                        let input = CString::new(serde_json::json!({"type":"layer_touch","surface":9,"id":id,"row":row,
                            "phase":phase,"position":[100. + id as f32 * 50. + dx,100.],"time_ns":1_000_000_000u64 + ms * 1_000_000 + id}).to_string()).unwrap();
                        assert_eq!(unsafe { capy_input(owner.0,input.as_ptr()) }, 0);
                    }
                }
                assert_eq!(borrow::<CapyHost>(owner.0).unwrap().native.session.state().layer_tools.opacity_popup, (!moved).then_some(row), "scale {scale}, slop {slop}, moved {moved}");
            }
        }
    }

    fn edge_owner() -> (Owner, [f32; 2]) {
        let owner=Owner::new(1600,1000);
        let mut host=borrow::<CapyHost>(owner.0).unwrap();
        host.native.dispatch(layer_ui::UiAction::RestoreWorkspace {workspace:Box::new(layer_ui::WorkspaceState {
            layout:layer_ui::WorkspacePreset::Illustrator.layout(layer_ui::Platform::Windows),..Default::default()
        })}).unwrap();
        let layout=host.native.session.layout([1600.,1000.]);
        let column=layout.groups.iter().find(|group|group.panels.contains(&layer_ui::Panel::Brushes)).unwrap().bounds;
        let divider=layout.dividers.iter().find(|divider|(divider.bounds.x-column.x-column.width).abs()<0.5).unwrap();
        let start=[divider.bounds.x+divider.bounds.width+20.,500.];
        drop(host);
        (owner,start)
    }
    fn sample(id: u64, phase: u32, position: [f32; 2], sequence: u64) -> CapyPointer {
        CapyPointer {id,phase,x:position[0],y:position[1],sequence,timestamp_ns:sequence*200_000_000,
            view_revision:0,pressure:0.5,tilt_x:0.,tilt_y:0.,twist:0.,distance:0.,tool:3,button:0,flags:2}
    }

    #[test]
    fn blocked_edge_abi_samples_cancel_instead_of_committing_or_retaining_contacts() {
        for claimed in [false,true] {
            for phase in ["move","up","cancel"] {
                let (owner,start)=edge_owner();
                let before=owner.layout();
                assert_eq!(owner.edge(7,"down",start,0),8);
                if claimed {
                    assert_ne!(owner.edge(7,"move",[start[0]+40.,start[1]],10)&0xf0,0);
                    owner.edge(7,"move",[start[0]+80.,start[1]],300);
                    assert_ne!(owner.layout(),before);
                }
                owner.blocked(true);
                assert_eq!(owner.edge(7,phase,[start[0]+100.,start[1]],310),0);
                assert_eq!(owner.layout(),before);
                owner.blocked(false);
                assert_eq!(owner.edge(7,"move",[start[0]+20.,start[1]],320),0);
                assert_eq!(owner.edge(7,"down",start,330),8);
                assert_ne!(owner.edge(7,"move",[start[0]+20.,start[1]],340)&0xf0,0);
                owner.edge(7,"cancel",start,350);
                assert_eq!(owner.layout(),before);
            }
        }
    }

    #[test]
    fn mixed_edge_down_abi_reports_retirement_only_for_another_touch() {
        for kind in ["mouse","pen","touch"] {
            let (owner,start)=edge_owner();
            assert_eq!(owner.edge(7,"down",start,0),8);
            assert_eq!(owner.edge_as(8,"down",kind,start,10),if kind=="touch" {128} else {0});
            assert_eq!(owner.edge(7,"move",[start[0]+40.,start[1]],20)&0x70!=0,kind!="touch");
            owner.edge(7,"cancel",start,30);
        }
    }

    #[test]
    fn a_canvas_edge_down_reusing_a_claimed_id_starts_a_fresh_contact() {
        let (owner,start)=edge_owner();
        let before=owner.layout();
        owner.pointer(&[sample(7,1,start,1),sample(7,2,[start[0]+40.,start[1]],2),sample(7,2,[start[0]+80.,start[1]],3)]);
        assert_ne!(owner.layout(),before);
        owner.pointer(&[sample(7,1,start,4)]);
        assert_eq!(owner.layout(),before);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert_eq!(host.edge_touches.get(&7),Some(&false));
            assert!(host.live_contacts.contains_key(&7));
        }
        owner.pointer(&[sample(7,2,[start[0]+6.,start[1]],5)]);
        assert_eq!(owner.layout(),before);
        owner.pointer(&[sample(7,2,[start[0]+40.,start[1]],6),sample(7,2,[start[0]+80.,start[1]],7)]);
        assert_ne!(owner.layout(),before);
        owner.pointer(&[sample(7,4,start,8)]);
        assert_eq!(owner.layout(),before);
        let host=borrow::<CapyHost>(owner.0).unwrap();
        assert!(host.edge_touches.is_empty() && host.live_contacts.is_empty());
    }

    #[test]
    fn a_canvas_id_reused_by_another_tool_retires_touch_edge_ownership() {
        for tool in [0,1,2] {
            for claimed in [false,true] {
                for via_canvas in [false,true] {
                    let (owner,start)=edge_owner();
                    let before=owner.layout();
                    let contact=|phase,position,sequence| {
                        if via_canvas {owner.pointer(&[sample(7,phase,position,sequence)]);}
                        else {owner.edge(7,["hover","down","move","up","cancel"][phase as usize],position,sequence*200);}
                    };
                    contact(1,start,1);
                    if claimed {
                        contact(2,[start[0]+40.,start[1]],2);contact(2,[start[0]+80.,start[1]],3);
                        assert_ne!(owner.layout(),before);
                    }
                    let mut next=sample(7,1,[800.,500.],4);next.tool=tool;next.button=1;
                    owner.pointer(&[next]);
                    assert_eq!(owner.layout(),before);
                    {
                        let host=borrow::<CapyHost>(owner.0).unwrap();
                        assert!(host.edge_touches.is_empty(),"tool {tool}, claimed {claimed}, canvas {via_canvas}");
                        assert_eq!(host.live_contacts.get(&7).unwrap().tool,tool);
                    }
                    next.phase=4;next.sequence=5;next.timestamp_ns=1_000_000_000;
                    owner.pointer(&[next]);
                    assert_eq!(owner.edge(7,"move",[start[0]+100.,start[1]],1100),0);
                }
            }
        }
    }

    #[test]
    fn a_blocked_canvas_edge_cancels_without_waiting_for_up_and_reuses_its_id() {
        let (owner,start)=edge_owner();
        let before=owner.layout();
        owner.pointer(&[sample(7,1,start,1)]);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert_eq!(host.edge_touches.get(&7),Some(&false));
            assert!(host.live_contacts.contains_key(&7));
        }
        owner.pointer(&[sample(7,2,[start[0]+40.,start[1]],2),sample(7,2,[start[0]+80.,start[1]],3)]);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert_eq!(host.edge_touches.get(&7),Some(&true));
            assert!(host.live_contacts.is_empty());
        }
        assert_ne!(owner.layout(),before);
        owner.blocked(true);
        owner.pointer(&[sample(7,2,[start[0]+100.,start[1]],4)]);
        assert_eq!(owner.layout(),before);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert!(host.edge_touches.is_empty() && host.live_contacts.is_empty());
            assert!(host.blocked_contacts.contains(&7));
        }
        owner.blocked(false);
        owner.pointer(&[sample(7,2,[start[0]+120.,start[1]],5)]);
        assert_eq!(owner.layout(),before);
        owner.pointer(&[sample(7,1,start,6)]);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert_eq!(host.edge_touches.get(&7),Some(&false));
            assert!(host.live_contacts.contains_key(&7) && host.blocked_contacts.is_empty());
        }
        owner.pointer(&[sample(7,4,start,7)]);
        let host=borrow::<CapyHost>(owner.0).unwrap();
        assert!(host.edge_touches.is_empty() && host.live_contacts.is_empty());
    }

    #[test]
    fn read_only_workspace_finishes_an_existing_canvas_contact() {
        let owner=Owner::new(1600,1000);
        let mut contact=sample(7,1,[800.,500.],1);contact.tool=0;
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert!(host.native.session.pointer_contact_paints(layer_ui::PointerKind::Pen,PointerButton::Primary,[800.,500.]));
        }
        owner.pointer(&[contact]);
        assert!(borrow::<CapyHost>(owner.0).unwrap().native.session.require_workspace_idle().is_err());
        borrow::<CapyHost>(owner.0).unwrap().native.session.set_workspace_read_only(true);
        contact.phase=2;contact.x+=40.;contact.sequence=2;contact.timestamp_ns=400_000_000;
        owner.pointer(&[contact]);
        assert!(borrow::<CapyHost>(owner.0).unwrap().native.session.require_workspace_idle().is_err(),"read-only interrupted the admitted paint contact");
        contact.phase=3;contact.sequence=3;contact.timestamp_ns=600_000_000;
        owner.pointer(&[contact]);
        borrow::<CapyHost>(owner.0).unwrap().native.session.require_workspace_idle().unwrap();
        contact.phase=1;contact.sequence=4;contact.timestamp_ns=800_000_000;
        owner.pointer(&[contact]);
        borrow::<CapyHost>(owner.0).unwrap().native.session.require_workspace_idle().unwrap();
        contact.phase=3;contact.sequence=5;contact.timestamp_ns=1_000_000_000;
        owner.pointer(&[contact]);
    }

    #[test]
    fn blocked_pointer_batches_retire_every_live_contact() {
        let owner=Owner::new(1600,1000);
        owner.pointer(&[sample(7,1,[800.,500.],1),sample(8,1,[900.,500.],2)]);
        assert_eq!(borrow::<CapyHost>(owner.0).unwrap().live_contacts.len(),2);
        owner.blocked(true);
        owner.pointer(&[sample(7,3,[800.,500.],3),sample(8,4,[900.,500.],4)]);
        let host=borrow::<CapyHost>(owner.0).unwrap();
        assert!(host.live_contacts.is_empty() && host.edge_touches.is_empty() && host.blocked_contacts.is_empty());
    }

    #[test]
    fn rejected_touch_json_keeps_a_windowless_owner_alive() {
        let owner=Owner::new(640,480);
        let malformed=CString::new(r#"{"type":"color_touch","phase":"down","position":[1.5,2.5],"surface":7,"id":1,"time_ns":1101152828577000.0}"#).unwrap();
        let valid=CString::new(r#"{"type":"blur"}"#).unwrap();
        let revision=borrow::<CapyHost>(owner.0).unwrap().native.session.state().revision;
        assert_eq!(unsafe {capy_input(owner.0,malformed.as_ptr())},1);
        {
            let host=borrow::<CapyHost>(owner.0).unwrap();
            assert!(host.native.poisoned().is_none());
            assert_eq!(host.native.session.state().revision,revision);
            assert!(host.live_contacts.is_empty());
        }
        assert_eq!(unsafe {capy_input(owner.0,valid.as_ptr())},0);
        assert!(borrow::<CapyHost>(owner.0).unwrap().native.poisoned().is_none());
        assert_eq!(unsafe {capy_input(0,valid.as_ptr())},-1);
        {
            let mut host=borrow::<CapyHost>(owner.0).unwrap();
            host.native.poison(&"fixture failure");
        }
        assert_eq!(unsafe {capy_input(owner.0,valid.as_ptr())},-1);
        assert_eq!(unsafe {capy_input(owner.0,malformed.as_ptr())},-1);
    }

    #[test]
    fn invalid_file_buffers_are_rejected_before_accessing_path_or_bytes() {
        for length in [1,usize::MAX] {
            assert_eq!(unsafe {capy_write_file(std::ptr::null(),std::ptr::null(),length)},-1);
        }
    }

    #[test]
    fn invalid_and_wrong_kind_calls_leave_immutable_handles_usable() {
        let context=crate::shared_controls::store_localization(layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        for raw in [0,1,u64::MAX,context] {
            unsafe {
                assert_eq!(capy_needs_frame(raw),-1);
                assert_eq!(capy_finish_services(raw),-1);
                assert_eq!(capy_query(raw,std::ptr::null()),0);
                assert_eq!(capy_launch_view(raw),0);
                assert_eq!(capy_launch_localization(raw),0);
                assert_eq!(capy_create(0,0,1.,raw),0);
                capy_destroy(raw);capy_launch_free(raw);
            }
        }
        assert!(crate::shared_controls::localization(context).is_some());
        unsafe {crate::capy_localization_free(context);}
        assert!(crate::shared_controls::localization(context).is_none());
    }

    #[test]
    fn launch_release_retires_active_tokens_and_text_outlives_its_parent() {
        let launch=handles::insert(CapyLaunch {native:None,settings:None,bootstrap:CString::new("{\"日本語\":1}").unwrap()}).unwrap().raw();
        let view=unsafe {capy_launch_view(launch)};assert_ne!(view,0);
        let active=borrow::<CapyLaunch>(launch).unwrap();
        unsafe {capy_launch_free(launch);assert_eq!(capy_launch_view(launch),0);}
        assert_eq!(active.bootstrap.to_str().unwrap(),"{\"日本語\":1}");
        drop(active);
        unsafe {capy_launch_free(launch);capy_launch_free(launch);}
        assert_eq!(unsafe {capy_launch_view(launch)},0);
        assert_eq!(crate::owned::take_text(view).to_str().unwrap(),"{\"日本語\":1}");
        let next=handles::insert(CapyLaunch {native:None,settings:None,bootstrap:CString::default()}).unwrap().raw();
        assert_ne!(launch,next);unsafe {capy_launch_free(launch);}
        let view=unsafe {capy_launch_view(next)};assert_ne!(view,0);
        assert_eq!(crate::owned::take_text(view).as_bytes(),b"");
        unsafe {capy_launch_free(next);}
    }
}

#[cfg(test)]
mod chrome_completion_tests {
    use super::*;
    use std::time::Duration;
    use windows::{core::Interface, Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, D3D12_FENCE_FLAG_NONE}};
    struct Gate(ID3D12Fence);
    impl Drop for Gate {fn drop(&mut self){let _=unsafe{self.0.Signal(1)};}}
    #[test]
    fn completed_chrome_preserves_retained_ui_models() {
        let mut native=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let mut chrome=crate::chrome::Chrome::default();
        chrome.set(serde_json::from_value(serde_json::json!({"revision":7,"document":[192,192],"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap();
        let mut readiness=SurfaceReadiness::default();readiness.ready.store(true,std::sync::atomic::Ordering::Release);
        if readiness.changed(&mut chrome){native.invalidate_snapshot();}
        assert!(native.take_layout_update_bytes().unwrap().is_some());
        readiness.documents.push_back((7,[192;2]));
        readiness.completed_chrome.fetch_max(7,std::sync::atomic::Ordering::Release);
        if readiness.changed(&mut chrome){native.invalidate_snapshot();}
        assert_eq!(chrome.presented_revision,Some(7));assert!(native.take_layout_update_bytes().unwrap().is_none());
    }
    #[test]
    fn retired_chrome_completion_cannot_acknowledge_a_new_presenter() {
        let mut readiness=SurfaceReadiness::default();
        let ready=readiness.ready.clone();let completed=readiness.completed_chrome.clone();
        readiness.documents.push_back((7,[192;2]));
        readiness.retire_chrome();
        ready.store(true,std::sync::atomic::Ordering::Release);completed.fetch_max(7,std::sync::atomic::Ordering::Release);
        let mut chrome=crate::chrome::Chrome::default();
        chrome.set(serde_json::from_value(serde_json::json!({"revision":7,"document":[128,64],"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap();
        assert!(readiness.changed(&mut chrome));assert!(readiness.ready());assert_eq!(chrome.presented_revision,None);
        readiness.documents.push_back((7,[128,64]));
        readiness.completed_chrome.fetch_max(7,std::sync::atomic::Ordering::Release);
        assert!(!readiness.changed(&mut chrome));assert_eq!(chrome.presented_revision,Some(7));assert_eq!(chrome.presented_document,Some([128,64]));
    }
    #[test]
    fn chrome_acknowledgement_waits_for_its_submission_without_another_frame() {
        let gpu=crate::test_support::d3d12_renderer();
        let first=gpu.queue().submit([]);
        gpu.device().poll(wgpu::PollType::Wait{submission_index:Some(first),timeout:Some(Duration::from_secs(10))}).unwrap();
        let gate={
            let native=unsafe{gpu.device().as_hal::<wgpu::hal::api::Dx12>()}.unwrap();
            let device:ID3D12Device=native.raw_device().cast().unwrap();
            let gate=Gate(unsafe{device.CreateFence(0,D3D12_FENCE_FLAG_NONE)}.unwrap());
            unsafe{native.raw_queue().Wait(&gate.0,1)}.unwrap();gate
        };
        let output=gpu.device().create_texture(&wgpu::TextureDescriptor{
            label:Some("chrome completion regression"),size:wgpu::Extent3d{width:4,height:4,depth_or_array_layers:1},
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba8Unorm,
            usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[],
        });
        let view=output.create_view(&Default::default());let mut encoder=gpu.device().create_command_encoder(&Default::default());
        {let _pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor{color_attachments:&[Some(wgpu::RenderPassColorAttachment{
            view:&view,depth_slice:None,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color::WHITE),store:wgpu::StoreOp::Store},
        })],..Default::default()});}
        let mut chrome=crate::chrome::Chrome::default();
        chrome.set(serde_json::from_value(serde_json::json!({"revision":7,"document":[192,192],"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap();
        let mut readiness=SurfaceReadiness::default();
        let submitted=gpu.queue().submit([encoder.finish()]);readiness.submitted(gpu.queue(),&chrome,Some([192;2]));
        chrome.set(serde_json::from_value(serde_json::json!({"revision":8,"document":[128,64],"glass":{"regions":[],"connections":[]},"overviews":[]})).unwrap()).unwrap();
        readiness.submitted(gpu.queue(),&chrome,Some([192;2]));assert_eq!(readiness.queued_chrome,7);
        gpu.device().poll(wgpu::PollType::Poll).unwrap();readiness.changed(&mut chrome);
        assert_eq!(chrome.presented_revision,None,"queued chrome must remain covered until its submission completes");
        let unrelated={
            let native=unsafe{gpu.device().as_hal::<wgpu::hal::api::Dx12>()}.unwrap();
            let device:ID3D12Device=native.raw_device().cast().unwrap();
            let gate=Gate(unsafe{device.CreateFence(0,D3D12_FENCE_FLAG_NONE)}.unwrap());
            unsafe{native.raw_queue().Wait(&gate.0,1)}.unwrap();gate
        };
        gpu.queue().submit([]);drop(gate);
        gpu.device().poll(wgpu::PollType::Wait{submission_index:Some(submitted),timeout:Some(Duration::from_secs(10))}).unwrap();
        assert!(readiness.changed(&mut chrome));assert_eq!(chrome.presented_revision,Some(7));assert!(!readiness.changed(&mut chrome));
        assert_eq!(chrome.document,[128,64]);assert_eq!(chrome.presented_document,Some([192;2]));
        drop(unrelated);let next=gpu.queue().submit([]);readiness.submitted(gpu.queue(),&chrome,Some([128,64]));
        gpu.device().poll(wgpu::PollType::Wait{submission_index:Some(next),timeout:Some(Duration::from_secs(10))}).unwrap();
        assert!(!readiness.changed(&mut chrome));assert_eq!(chrome.presented_revision,Some(8));assert_eq!(chrome.presented_document,Some([128,64]));
        drop(gpu);layer_render_wgpu::finish_shader_compiler_shutdown();
    }
}
