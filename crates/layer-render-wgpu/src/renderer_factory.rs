use super::*;
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
pub struct RendererFactory(Arc<Factory>);
struct Factory {
    adapter: wgpu::Adapter,
    adapter_summary: Arc<layer_render::GpuAdapter>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    choices: Mutex<Vec<Arc<Choice>>>,
    shared: pipeline_device::DeviceRecipes,
    budget: layer_core::MemoryBudget,
    memory_device: layer_core::MemoryDevice,
    #[cfg(target_arch = "wasm32")]
    memory_summary: Mutex<Option<Arc<layer_render::MemoryBudgetSummary>>>,
    #[cfg(target_arch = "wasm32")]
    memory_update_pending: std::sync::atomic::AtomicBool,
    #[cfg(not(target_arch = "wasm32"))]
    memory: Mutex<std::sync::Weak<crate::memory_sample::MemorySampler>>,
    #[cfg(not(target_arch = "wasm32"))]
    allocator_memory: layer_core::MemoryOwner,
    #[cfg(all(test, not(target_arch = "wasm32")))]
    _memory_fixture: Option<Arc<crate::memory_sample::MemorySampler>>,
}
impl Drop for Factory {
    fn drop(&mut self) { self.budget.unregister_device(self.memory_device); }
}
#[cfg(test)]
pub(crate) fn fixture_factory(hardware: &RendererFactory, budget: layer_core::MemoryBudget, facts: layer_core::DeviceMemoryFacts) -> RendererFactory {
    let memory_device = budget.register_device(facts);
    let mut shared = hardware.0.shared.clone();
    shared.transient_memory = Some(budget.register_owner(layer_core::MemoryOwnerKind::Other, layer_core::MemoryClass::Pinned, Some(memory_device)));
    #[cfg(not(target_arch = "wasm32"))]
    let memory=Arc::new(crate::memory_sample::fixture_sampler(budget.clone(), memory_device));
    let factory = RendererFactory(Arc::new(Factory { adapter: hardware.adapter().clone(), adapter_summary: hardware.0.adapter_summary.clone(), device: hardware.device().clone(), queue: hardware.queue().clone(),
        choices: Default::default(), shared, budget: budget.clone(), memory_device,
        #[cfg(target_arch = "wasm32")] memory_summary: Default::default(),
        #[cfg(target_arch = "wasm32")] memory_update_pending: Default::default(),
        #[cfg(not(target_arch = "wasm32"))] memory: Mutex::new(Arc::downgrade(&memory)),
        #[cfg(not(target_arch = "wasm32"))] allocator_memory: budget.register_owner(layer_core::MemoryOwnerKind::Other, layer_core::MemoryClass::Pinned, Some(memory_device)),
        #[cfg(not(target_arch = "wasm32"))] _memory_fixture: Some(memory),
    }));
    factory
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key { space: layer_core::color::RgbSpace, depth: layer_core::color::SampleDepth }
struct Choice { key: Key, device: PipelineDevice, recipes: OnceLock<Arc<Recipes>> }

impl RendererFactory {
    pub fn new(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        { Self::new_saved(adapter, device, queue, None) }
        #[cfg(target_arch = "wasm32")]
        { Self::with_device(adapter, &PipelineDevice::from(device), queue) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_saved(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue, saved: Option<Arc<SavedPipelines>>) -> Self {
        Self::with_device(adapter, &PipelineDevice::from(device).with_saved_pipelines(saved), queue)
    }
    pub(super) fn with_device(adapter: wgpu::Adapter, device: &PipelineDevice, queue: wgpu::Queue) -> Self {
        #[cfg(not(target_arch="wasm32"))]
        device.bind_queue(&queue);
        let budget = layer_core::MemoryBudget::process();
        #[cfg(not(target_arch = "wasm32"))]
        let facts = crate::display_memory::facts(device, crate::display_memory::usage(device));
        #[cfg(target_arch = "wasm32")]
        let facts = layer_core::DeviceMemoryFacts { unified: true, ..Default::default() };
        let memory_device = budget.register_device(facts);
        let mut shared = pipeline_device::DeviceRecipes::from(device);
        shared.transient_memory = Some(budget.register_owner(layer_core::MemoryOwnerKind::Other, layer_core::MemoryClass::Pinned, Some(memory_device)));
        #[cfg(not(target_arch = "wasm32"))]
        let allocator_memory = budget.register_owner(layer_core::MemoryOwnerKind::Other, layer_core::MemoryClass::Pinned, Some(memory_device));
        let factory = Self(Arc::new(Factory { adapter_summary: Arc::new(adapter_summary(&adapter.get_info())), adapter, device: (**device).clone(), queue, choices: Default::default(), shared, budget, memory_device,
            #[cfg(target_arch = "wasm32")] memory_summary: Default::default(),
            #[cfg(target_arch = "wasm32")] memory_update_pending: Default::default(),
            #[cfg(not(target_arch = "wasm32"))] memory: Default::default(),
            #[cfg(not(target_arch = "wasm32"))] allocator_memory,
            #[cfg(all(test, not(target_arch = "wasm32")))] _memory_fixture: None }));
        factory
    }
    pub(super) fn pipeline_device(&self, color: layer_core::color::DocumentColor) -> PipelineDevice {
        PipelineDevice::from(self.device().clone()).with_device_recipes(&self.0.shared)
            .with_working_space(color.space).with_depth(color.depth)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retire_device_services(self) -> std::io::Result<()> {
        crate::compile::retire_device_services(self.0.shared.poll_binding(), self)
    }
    pub fn adapter(&self) -> &wgpu::Adapter { &self.0.adapter }
    pub(super) fn adapter_summary(&self) -> Arc<layer_render::GpuAdapter> { self.0.adapter_summary.clone() }
    pub fn device(&self) -> &wgpu::Device { &self.0.device }
    pub fn queue(&self) -> &wgpu::Queue { &self.0.queue }
    pub fn submit(&self,commands:impl IntoIterator<Item=wgpu::CommandBuffer>)->Option<wgpu::SubmissionIndex> {
        self.pipeline_device(Default::default()).submit(self.queue(),commands)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn memory_samples(&self) -> Arc<crate::memory_sample::MemorySampler> {
        let mut cached=self.0.memory.lock().unwrap();
        if let Some(memory)=cached.upgrade() { return memory; }
        let memory=Arc::new(crate::memory_sample::MemorySampler::new(self.device().clone(),self.0.budget.clone(),self.0.memory_device,self.0.allocator_memory.clone()));
        *cached=Arc::downgrade(&memory);
        memory
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn memory_snapshot(&self) -> Option<layer_render::MemorySnapshot> { self.0.memory.lock().unwrap().upgrade().and_then(|memory| memory.snapshot()) }
    pub(super) fn memory_budget(&self) -> &layer_core::MemoryBudget { &self.0.budget }
    pub(super) fn device_memory_limit(&self)->Option<u64> {
        self.0.budget.snapshot().devices.into_iter().find(|device|device.device==self.0.memory_device)
            .map(|device|device.limit_bytes).filter(|limit|*limit!=0)
    }
    pub(super) fn unified_memory(&self)->bool {self.0.budget.snapshot().devices.iter().any(|device|device.device==self.0.memory_device && device.unified)}
    pub(super) fn request_memory_update(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(memory) = self.0.memory.lock().unwrap().upgrade() { memory.request_update(); }
        #[cfg(target_arch = "wasm32")]
        {
            if self.0.memory_update_pending.swap(true, std::sync::atomic::Ordering::AcqRel) { return; }
            let factory = self.clone();
            wasm_bindgen_futures::spawn_local(async move {
                factory.0.budget.flush_reports();
                let snapshot = factory.0.budget.snapshot();
                *factory.0.memory_summary.lock().unwrap() = Some(Arc::new(layer_render::MemoryBudgetSummary::new(&snapshot, factory.0.memory_device)));
                factory.0.memory_update_pending.store(false, std::sync::atomic::Ordering::Release);
            });
        }
    }
    pub(super) fn memory_owner(&self, kind: layer_core::MemoryOwnerKind) -> layer_core::MemoryOwner {
        self.memory_owner_in(kind,layer_core::MemoryClass::Visible)
    }
    pub(super) fn memory_owner_in(&self,kind:layer_core::MemoryOwnerKind,class:layer_core::MemoryClass)->layer_core::MemoryOwner {
        self.0.budget.register_owner(kind,class,Some(self.0.memory_device))
    }
    pub(super) fn budget_snapshot(&self) -> Option<Arc<layer_render::MemoryBudgetSummary>> {
        #[cfg(not(target_arch = "wasm32"))]
        { self.0.memory.lock().unwrap().upgrade().and_then(|memory| memory.budget_snapshot()) }
        #[cfg(target_arch = "wasm32")]
        { self.0.memory_summary.try_lock().ok().and_then(|summary| summary.clone()) }
    }
    pub fn rasterizer(&self, color: layer_core::color::DocumentColor) -> Result<Box<WgpuRasterizer>, GpuRasterError> {
        let device = self.pipeline_device(color);
        WgpuRasterizer::native_staged_on_factory(self.clone(), device, color)
    }
    pub(super) fn resources(&self, requested: &PipelineDevice) -> (PipelineDevice, Arc<Recipes>) {
        let key = Key { space: requested.working_space(), depth: requested.depth() };
        let choice = {
            let mut choices = self.0.choices.lock().unwrap();
            if let Some(choice) = choices.iter().find(|choice| choice.key == key) { choice.clone() }
            else {
                let device = self.pipeline_device(layer_core::color::DocumentColor { space: key.space, depth: key.depth });
                let choice = Arc::new(Choice { key, device, recipes: OnceLock::new() });
                choices.push(choice.clone()); choice
            }
        };
        let recipes = choice.recipes.get_or_init(|| Arc::new(Recipes::new(&choice.device))).clone();
        (requested.with_recipes(&choice.device), recipes)
    }
}

struct SurfaceChoice {
    format: wgpu::TextureFormat,
    color: SdrSurfaceColor,
    recipes: OnceLock<Arc<present::Recipes>>,
}

pub(super) struct Recipes {
    pub sampler: wgpu::Sampler,
    pub brush_sampler: wgpu::Sampler,
    pub style_layout: wgpu::BindGroupLayout,
    pub texture_layout: wgpu::BindGroupLayout,
    pub float_texture_layout: wgpu::BindGroupLayout,
    pub advanced_texture_layout: wgpu::BindGroupLayout,
    pub target_layout: wgpu::BindGroupLayout,
    pub material_layout: wgpu::BindGroupLayout,
    pub edge_layout: wgpu::BindGroupLayout,
    pub watercolor_layout: wgpu::BindGroupLayout,
    pub transport_layout: wgpu::BindGroupLayout,
    pub pipelines: Arc<Pipelines>,
    pub scene: scene::Pipelines,
    pub transforms: Arc<paint_transform::Recipes>,
    pub masks: Arc<layer_masks::Pipelines>,
    pub selection: Arc<selection_clip::Pipelines>,
    pub effects: OnceLock<Arc<effects::Recipes>>,
    pub retouch: OnceLock<Arc<retouch_sources::Pipelines>>,
    pub liquify: OnceLock<Arc<liquify::Pipelines>>,
    pub display: display_mips::Pipelines,
    pub selection_paint: Arc<selection_paint::Pipelines>,
    pub paper: Arc<paper::Recipes>,
    pub paper_export: OnceLock<Arc<paper::ExportShade>>,
    surfaces: Mutex<Vec<Arc<SurfaceChoice>>>,
    _portable: Arc<portable_blend::Pipelines>,
}
impl Recipes {
    pub fn presentation(&self, device: &PipelineDevice, queue: &wgpu::Queue, format: wgpu::TextureFormat, color: SdrSurfaceColor) -> Arc<present::Recipes> {
        let choice = {
            let mut surfaces = self.surfaces.lock().unwrap();
            if let Some(choice) = surfaces.iter().find(|c| c.format == format && c.color == color) { choice.clone() }
            else {
                let choice = Arc::new(SurfaceChoice { format, color, recipes: OnceLock::new() });
                surfaces.push(choice.clone()); choice
            }
        };
        choice.recipes.get_or_init(|| Arc::new(present::Recipes::new(device, queue, format, color))).clone()
    }
    fn new(device: &PipelineDevice) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer linear clamp sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let brush_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("layer repeating brush sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let style_layout = create_style_layout(device);
        let texture_layout = create_texture_layout(device, true);
        let float_texture_layout = create_texture_layout(device, device.float32_filterable());
        let advanced_texture_layout = create_advanced_texture_layout(device);
        let target_layout = create_target_layout(device);
        let material_layout = create_material_layout(device);
        let edge_layout = fragment_textures_layout(device, 10, "layer post-stroke edge sources");
        let watercolor_layout = bindings::layout(device, "layer watercolor pigment and wetness neighborhood",
            &(0..10).map(|i| bindings::texture(i, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, false)).collect::<Vec<_>>());
        let transport_layout = bindings::layout(device, "layer watercolor transport sources",
            &(0..10).map(|i| bindings::texture(i, wgpu::ShaderStages::COMPUTE, false)).chain([
                bindings::storage_texture(10, wgpu::ShaderStages::COMPUTE, device.working_format(), wgpu::StorageTextureAccess::WriteOnly),
                bindings::storage_texture(11, wgpu::ShaderStages::COMPUTE, device.scalar_format(), wgpu::StorageTextureAccess::WriteOnly),
                bindings::buffer(12, wgpu::ShaderStages::COMPUTE, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(160)),
            ]).collect::<Vec<_>>());
        let pipelines = create_pipelines(
            device,
            PipelineLayouts {
                style: &style_layout,
                texture: &texture_layout,
                float_texture: &float_texture_layout,
                advanced_texture: &advanced_texture_layout,
                target: &target_layout,
                material: &material_layout,
                edge: &edge_layout,
                watercolor: &watercolor_layout,
                transport: &transport_layout,
            },
        );
        let masks = Arc::new(layer_masks::Pipelines::new(device, &style_layout, &target_layout, &texture_layout, &advanced_texture_layout));
        let selection = Arc::new(selection_clip::Pipelines::new(device));
        let selection_paint = Arc::new(selection_paint::Pipelines::new(device, &advanced_texture_layout));
        let paper = Arc::new(paper::Recipes::new(device));
        let display = display_mips::Pipelines::new(device);
        let scene = scene::Pipelines::new(device);
        let transforms = paint_transform::PaintTransforms::new(device).recipes();
        let portable = Arc::new(portable_blend::Pipelines::new(&device.for_recipe()));
        *device.blend_pipelines.lock().unwrap() = Arc::downgrade(&portable);
        Self { sampler, brush_sampler, style_layout, texture_layout, float_texture_layout, advanced_texture_layout, target_layout,
            material_layout, edge_layout, watercolor_layout, transport_layout, pipelines: Arc::new(pipelines), scene, transforms, masks, selection, selection_paint, paper, display, effects: OnceLock::new(), retouch: OnceLock::new(), liquify: OnceLock::new(), paper_export: OnceLock::new(), surfaces: Mutex::new(Vec::new()), _portable: portable }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};

    #[test]
    fn explicit_factory_owners_share_recipes_without_sharing_document_state() {
        let first = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
        let factory = first.renderer_factory();
        assert!(Arc::ptr_eq(&factory.0, &first.factory.0));
        let second = factory.rasterizer(DocumentColor::default()).unwrap();
        assert!(Arc::ptr_eq(&first.pipelines, &second.pipelines));
        pollster::block_on(first.compilation.prepare_all([&first.pipelines.export,&first.pipelines.direct[0],&first.pipelines.direct[1]])).unwrap();
        assert_eq!(first.pipelines.export.compile(), second.pipelines.export.compile());
        assert_eq!(first.pipelines.direct[0].compile(), second.pipelines.direct[0].compile());
        assert_ne!(first.pipelines.direct[0].compile(), second.pipelines.direct[1].compile());
        assert!(Arc::ptr_eq(&first.recipes.masks, &second.recipes.masks));
        assert!(Arc::ptr_eq(&first.recipes.selection, &second.recipes.selection));
        assert!(Arc::ptr_eq(&first.recipes.transforms, &second.recipes.transforms));
        assert!(!Arc::ptr_eq(&first.device.source_samples, &second.device.source_samples));
        assert!(!Arc::ptr_eq(&first.device.analysis_memory, &second.device.analysis_memory));
        assert!(!Arc::ptr_eq(&first.device.effect_resources, &second.device.effect_resources));
        assert_ne!(first.style_buffer, second.style_buffer);
        assert!(first.paint_layers.is_empty() && second.paint_layers.is_empty());
    }

    #[test]
    fn recipe_choices_isolate_working_color_depth_and_surface_encoding() {
        let renderer = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
        let factory = renderer.renderer_factory();
        let mut choices = Vec::new();
        for space in RgbSpace::ALL {
            for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
                let device = factory.pipeline_device(DocumentColor { space, depth });
                let (shared, recipes) = factory.resources(&device);
                let (_, again) = factory.resources(&device);
                assert!(Arc::ptr_eq(&recipes, &again));
                assert!(Arc::ptr_eq(&shared.bounds_pipeline, &renderer.device.bounds_pipeline));
                assert!(Arc::ptr_eq(&shared.levels_pipeline, &renderer.device.levels_pipeline));
                assert!(Arc::ptr_eq(&shared.statistics_pipeline, &renderer.device.statistics_pipeline));
                assert!(Arc::ptr_eq(&shared.sample_pipeline, &renderer.device.sample_pipeline));
                assert!(Arc::ptr_eq(&shared.native_publication, &renderer.device.native_publication));
                assert!(Arc::ptr_eq(&shared.native_transfers, &renderer.device.native_transfers));
                assert_eq!(shared.hdr(), depth.is_float());
                assert_eq!(shared.portable_blend(), device.portable_blend());
                assert!(choices.iter().all(|other| !Arc::ptr_eq(other, &recipes)));
                choices.push(recipes);
            }
        }
        let float = WgpuRasterizer::new_native_headless(DocumentColor { depth: SampleDepth::F32, ..DocumentColor::default() }).unwrap();
        let (_, matching) = float.factory.resources(&float.device);
        assert!(Arc::ptr_eq(&float.recipes, &matching));
        assert!(!Arc::ptr_eq(&float.factory.0, &renderer.factory.0));
        let recipes = &renderer.recipes;
        let srgb = recipes.presentation(&renderer.device, &renderer.queue, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb);
        let repeated = recipes.presentation(&renderer.device, &renderer.queue, wgpu::TextureFormat::Rgba8UnormSrgb, SdrSurfaceColor::Srgb);
        let hdr = recipes.presentation(&renderer.device, &renderer.queue, wgpu::TextureFormat::Rgba16Float, SdrSurfaceColor::ExtendedSrgb);
        assert!(Arc::ptr_eq(&srgb, &repeated));
        assert!(!Arc::ptr_eq(&srgb, &hdr));
    }

    #[test]
    fn shared_effect_recipes_keep_each_renderers_parameters_and_pixels_private() {
        use layer_core::{Document, DocumentNames, EffectValue, color::RgbColor, authored::PortableId};
        let color = DocumentColor::default();
        let mut first = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut second = first.renderer_factory().rasterizer(color).unwrap();
        second.finish_startup_catalog();
        let mut red = Document::new(PortableId::random(), 16, 16, DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut blue = red.clone();
        let paper = red.scene().order()[1];
        let set = |document: &mut Document, rgba| crate::tests::native_effects::set_effect(document, paper, "color",
            EffectValue::Color(RgbColor::from_linear(color.space, rgba).unwrap()));
        set(&mut red, [1., 0., 0., 1.]);
        set(&mut blue, [0., 0., 1., 1.]);
        let render = |renderer: &mut WgpuRasterizer, document: &Document| {
            crate::test_support::submit_settled(renderer, crate::test_support::packet(document.scene(), [16; 2]));
            renderer.readback_srgb_rgba8().unwrap()
        };
        let red_pixels = render(&mut first, &red);
        let blue_pixels = render(&mut second, &blue);
        assert!(Arc::ptr_eq(first.recipes.effects.get().unwrap(), second.recipes.effects.get().unwrap()));
        assert!(red_pixels.as_chunks::<4>().0.iter().all(|p| *p == [255, 0, 0, 255]));
        assert!(blue_pixels.as_chunks::<4>().0.iter().all(|p| *p == [0, 0, 255, 255]));
        set(&mut red, [0., 1., 0., 1.]);
        assert!(render(&mut first, &red).as_chunks::<4>().0.iter().all(|p| *p == [0, 255, 0, 255]));
        assert_eq!(second.readback_srgb_rgba8().unwrap(), blue_pixels);
    }

    #[test]
    fn shared_factory_preserves_linear_and_perceptual_composition() {
        use layer_core::{BlendSpace, Document, DocumentNames, EffectValue, color::RgbColor, authored::*};
        let reference = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
        let factory = reference.renderer_factory();
        for (blend, expected) in [(BlendSpace::Linear, 188u8), (BlendSpace::Perceptual, 128u8)] {
            let mut document = Document::new(PortableId::random(), 16, 16, DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
            let paper = document.scene().order()[1];
            let source = document.scene().effect_handle(paper).unwrap();
            let effect = document.artwork.effects.get(source).unwrap().clone();
            let effect = document.artwork.effects.insert(PortableId::random(), effect).unwrap();
            let mut black = document.artwork.occurrences.get(paper).unwrap().clone();
            black.content = OccurrenceContent::Effect(effect);
            black.opacity = 0.5;
            let black = document.artwork.occurrences.insert(PortableId::random(), black).unwrap();
            let root = document.artwork.compositions.get_mut(document.artwork.root).unwrap();
            root.blend = blend;
            document.artwork.stacks.get_mut(root.result).unwrap().entries.insert(0, black);
            let mut document = Document::from_artwork(document.artwork).unwrap();
            crate::tests::native_effects::set_effect(&mut document, black, "color",
                EffectValue::Color(RgbColor::from_linear(RgbSpace::Srgb, [0., 0., 0., 1.]).unwrap()));
            let mut renderer = factory.rasterizer(document.composition().color).unwrap();
            renderer.finish_startup_catalog();
            assert!(Arc::ptr_eq(&renderer.pipelines, &reference.pipelines));
            crate::test_support::submit_settled(&mut renderer, layer_render::FramePacket { blend_space: blend,
                ..crate::test_support::packet(document.scene(), [16; 2]) });
            for pixel in renderer.readback_srgb_rgba8().unwrap().as_chunks::<4>().0 {
                assert!(pixel[..3].iter().all(|code| code.abs_diff(expected) <= 1) && pixel[3] == 255, "{blend:?}: {pixel:?}");
            }
        }
    }

    #[test]
    fn recipes_and_raw_handles_do_not_keep_factory_or_renderer_owners_alive() {
        let renderer = WgpuRasterizer::new_native_headless(DocumentColor::default()).unwrap();
        let factory = renderer.renderer_factory();
        let owner = Arc::downgrade(&factory.0);
        let raw = renderer.device().clone();
        let pipeline = renderer.compilation.blocking(||renderer.pipelines.export.compile().clone());
        drop(renderer);
        assert!(owner.upgrade().is_some());
        drop(factory);
        assert!(owner.upgrade().is_none());
        drop(pipeline);
        drop(raw);
    }
}
fn adapter_summary(info: &wgpu::AdapterInfo) -> layer_render::GpuAdapter {
    let api = match info.backend {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "Direct3D 12",
        wgpu::Backend::Gl => "OpenGL",
        wgpu::Backend::BrowserWebGpu => "WebGPU",
        _ => "",
    };
    let driver = [info.driver.trim(), info.driver_info.trim()].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
    layer_render::GpuAdapter { name: info.name.trim().to_string(), api, driver }
}
