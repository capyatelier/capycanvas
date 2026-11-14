use super::*;
use layer_core::{TextureSource, paper::{PaperBinding, PaperSurface}};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const PROBE_SIZE: u32 = 256;
const PAGE_SIZE: u32 = 256;
const PAGES_PER_STEP: u32 = 4;
const MAX_TEXELS: u32 = 4096;
const CACHED_BAKES: usize = 6;
const REFERENCE_RELIEF_UM: f32 = 100.;
pub(super) const SLOPE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
const FLASH_RELIEF: f32 = 4.;
const FLASH_ANGLE: f32 = 225.;

pub(super) struct PaperLighting {
    pub binding: PaperBinding,
    pub light: layer_core::paper::PaperLight,
    pub separated: Option<[f32; 4]>,
}

pub(super) fn separated_color(scene: layer_core::authored::SceneView<'_>, working: layer_core::color::RgbSpace) -> Result<Option<[f32; 4]>, GpuRasterError> {
    let Some((paper, _)) = layer_core::paper::separate_paper(scene) else { return Ok(None) };
    let [r, g, b, a] = scene.effect(paper).and_then(|effect| effect.constant_color()).expect("separated paper is a constant fill")
        .linear_in(working).map_err(GpuRasterError::Color)?;
    let alpha = a * scene.occurrence(paper).expect("placed paper").opacity;
    Ok(Some(scene.composition().blend.composite(working, [r * alpha, g * alpha, b * alpha, alpha])))
}

pub(super) struct ExportShade {
    layout: wgpu::BindGroupLayout,
    pub(super) pipeline: Deferred<wgpu::RenderPipeline>,
    sampler: wgpu::Sampler,
}
impl ExportShade {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = bindings::layout(device, "paper export", &[
            bindings::buffer(0, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Uniform, false, None),
            bindings::texture(1, wgpu::ShaderStages::FRAGMENT, false),
            bindings::texture(2, wgpu::ShaderStages::FRAGMENT, true),
            bindings::sampler(3, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
            bindings::texture(4, wgpu::ShaderStages::FRAGMENT, false),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("paper export"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = Deferred::wgsl(device, "paper export", [include_str!("paper_shade.wgsl"), include_str!("paper_export.wgsl")].join("\n"));
        let format = device.working_format();
        let pipeline = {
            let (device, layout, module) = (device.for_recipe(), pipeline_layout, module);
            Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &module, "paper_export", None, format, "paper export"))
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("paper export slope"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { layout, pipeline, sampler }
    }
    #[expect(clippy::too_many_arguments, reason = "Export shading keeps the source, coverage, slope and paper placement explicit")]
    pub fn encode(&self, device: &PipelineDevice, encoder: &mut wgpu::CommandEncoder, source: &wgpu::Texture, coverage: Option<&wgpu::Texture>,
        slope: &wgpu::TextureView, tile: [f32; 4], light: layer_core::paper::PaperLight, origin: [f64; 2]) -> Result<wgpu::Texture, GpuRasterError> {
        let [x, y] = light.direction();
        let data = [tile[0], tile[1], tile[2], tile[3], x, y, light.relief, light.through_paint, origin[0] as f32, origin[1] as f32, 0., 0.,
            1., f32::from(u8::from(coverage.is_some())), 0., 0.];
        let uniform = initialized_buffer(device, &wgpu::util::BufferInitDescriptor {
            label: Some("paper export"),
            contents: &data.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        })?;
        let (shaded, view) = create_color_target(device, [source.width(), source.height()], "paper export");
        let input = source.create_view(&Default::default());
        let covered = coverage.unwrap_or(source).create_view(&Default::default());
        let group = bindings::group(device, "paper export", &self.layout, [uniform.as_entire_binding(), wgpu::BindingResource::TextureView(&input),
            wgpu::BindingResource::TextureView(slope), wgpu::BindingResource::Sampler(&self.sampler), wgpu::BindingResource::TextureView(&covered)]);
        let mut pass = Papers::pass(encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        Ok(shaded)
    }
}

pub(super) fn flash_light(opacity: f32) -> layer_core::paper::PaperLight {
    layer_core::paper::PaperLight { relief: FLASH_RELIEF * opacity, angle: FLASH_ANGLE, through_paint: 1. }
}
const LOW_PERCENTILE: f64 = 0.005;
const HIGH_PERCENTILE: f64 = 0.995;
const PROGRAM_JITTER: f64 = 0.5;
const IMAGE_PROGRAM: &str = "image";
const MAPPING: u8 = 1;
const MAPPED: u8 = 2;
const FAILED: u8 = 3;

pub(super) struct PaperTile {
    pixels: [f64; 2],
    origin: [i64; 2],
    fraction: [f64; 2],
    inverse: [f64; 4],
    texels: [u32; 2],
    period: [f64; 2],
}
impl PaperTile {
    pub fn new(surface: &PaperSurface) -> Result<Self, GpuRasterError> {
        let period = surface.texture.period().ok_or(GpuRasterError::Effect("Paper texture without a physical size".into()))?;
        let [a, b, c, d, x, y] = surface.mapping.0;
        let raw = [period[0] * a.abs() + period[1] * c.abs(), period[0] * b.abs() + period[1] * d.abs()];
        let pixels = raw.map(|v| v.round().max(1.));
        let stretch = [pixels[0] / raw[0], pixels[1] / raw[1]];
        let inverse = layer_core::Affine64([a * stretch[0], b * stretch[1], c * stretch[0], d * stretch[1], 0., 0.])
            .inverse().ok_or(GpuRasterError::InvalidTransform("Singular paper mapping"))?.0;
        Ok(Self {
            pixels,
            origin: [x.floor() as i64, y.floor() as i64],
            fraction: [x - x.floor(), y - y.floor()],
            inverse: [inverse[0], inverse[1], inverse[2], inverse[3]],
            texels: pixels.map(|v| (v as u32).min(MAX_TEXELS)),
            period,
        })
    }
    pub fn id_for(binding: &PaperBinding) -> Option<AssetId> {
        Self::new(&binding.surface).ok().map(|tile| tile.id(&binding.surface))
    }
    pub fn id(&self, surface: &PaperSurface) -> AssetId {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let name = match &surface.texture {
            TextureSource::Program(instance) => {
                for value in &instance.values {
                    match value {
                        layer_core::EffectValue::Number(v) => v.to_bits().hash(&mut hasher),
                        layer_core::EffectValue::Toggle(v) => v.hash(&mut hasher),
                        layer_core::EffectValue::Choice(v) => v.hash(&mut hasher),
                        _ => {}
                    }
                }
                instance.program.id.to_string()
            }
            TextureSource::Image(image) => {
                image.id().to_string().hash(&mut hasher);
                IMAGE_PROGRAM.into()
            }
        };
        for value in self.inverse.iter().chain(&self.fraction).chain(&self.pixels) {
            value.to_bits().hash(&mut hasher);
        }
        AssetId::from(format!("paper:{name}:{:016x}", hasher.finish()).as_str())
    }
    fn texel_mm(&self) -> f64 {
        (self.period[0] * self.period[1] / f64::from(self.texels[0]) / f64::from(self.texels[1])).sqrt()
    }
    fn image_shrink(&self, surface: &PaperSurface) -> Option<f64> {
        let TextureSource::Image(image) = &surface.texture else { return None };
        Some((f64::from(image.extent[0]) * f64::from(image.extent[1]) / f64::from(self.texels[0]) / f64::from(self.texels[1])).sqrt())
    }
    fn parameters(&self, surface: &PaperSurface, low: f32, high: f32) -> [f32; 20] {
        let (jitter, nearest) = self.image_shrink(surface)
            .map_or((PROGRAM_JITTER, 0.), |shrink| (0.5 * (shrink - 1.).clamp(0., 1.), f64::from(u8::from(shrink >= 1.))));
        let [ia, ib, ic, id] = self.inverse.map(|v| v as f32);
        [ia, ib, ic, id,
            self.fraction[0] as f32, self.fraction[1] as f32, self.pixels[0] as f32, self.pixels[1] as f32,
            self.texels[0] as f32, self.texels[1] as f32, low, high,
            self.period[0] as f32, self.period[1] as f32, self.texel_mm() as f32, jitter as f32,
            nearest as f32, 0., 0., 0.]
    }
    pub fn grain_radius(&self) -> u32 {
        (3. * (self.pixels[0] / f64::from(self.texels[0])).max(self.pixels[1] / f64::from(self.texels[1]))).ceil().clamp(1.,16.) as u32
    }
    pub fn grain(&self, binding: &PaperBinding) -> [f32; 4] {
        let offset = [0, 1].map(|axis| ((binding.target_offset[axis] - self.origin[axis]) as f64).rem_euclid(self.pixels[axis]));
        [self.pixels[0] as f32, self.pixels[1] as f32, offset[0] as f32, offset[1] as f32]
    }
}

struct Programs {
    id: Arc<str>,
    probe: Deferred<wgpu::RenderPipeline>,
    bake: Deferred<wgpu::RenderPipeline>,
}

struct SlopePrograms {
    layout: wgpu::BindGroupLayout,
    slope: Deferred<wgpu::RenderPipeline>,
    mip: Deferred<wgpu::RenderPipeline>,
}
impl SlopePrograms {
    fn new(device: &PipelineDevice) -> Self {
        let layout = bindings::layout(device, "paper slope", &[
            bindings::buffer(0, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Uniform, false, None),
            bindings::texture(1, wgpu::ShaderStages::FRAGMENT, false),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("paper slope"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = Deferred::wgsl(device, "paper slope", include_str!("paper_slope.wgsl"));
        let pipeline = |entry: &'static str| {
            let (device, layout, module) = (device.for_recipe(), pipeline_layout.clone(), module.clone());
            Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &module, entry, None, SLOPE_FORMAT, "paper slope"))
        };
        Self { slope: pipeline("paper_slope"), mip: pipeline("paper_slope_mip"), layout }
    }
    fn ready(&self) -> bool {
        self.slope.ready() && self.mip.ready()
    }
    fn encode(&self, device: &PipelineDevice, queue: &wgpu::Queue, heights: &wgpu::TextureView, scale: f32) -> Result<wgpu::TextureView, GpuRasterError> {
        let source = heights.texture();
        let [width, height] = [source.width(), source.height()];
        let levels = 32 - width.max(height).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paper slope"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SLOPE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let uniform = initialized_buffer(device, &wgpu::util::BufferInitDescriptor {
            label: Some("paper slope scale"),
            contents: &[scale, 0., 0., 0.].iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        })?;
        let level = |level: u32| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("paper slope") });
        for target in 0..levels {
            let input = if target == 0 { heights.clone() } else { level(target - 1) };
            let group = bindings::group(device, "paper slope", &self.layout, [uniform.as_entire_binding(), wgpu::BindingResource::TextureView(&input)]);
            let output = level(target);
            let mut pass = Papers::pass(&mut encoder, &output, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(if target == 0 { &self.slope } else { &self.mip });
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        device.submit(queue,[encoder.finish()]);
        Ok(texture.create_view(&Default::default()))
    }
}

enum Stage {
    Waiting,
    Probing { readback: wgpu::Buffer, state: Arc<AtomicU8>, image: wgpu::TextureView },
    Baking { texture: wgpu::Texture, group: wgpu::BindGroup, next: u32, relief: f32 },
    Ready { view: wgpu::TextureView, relief: f32 },
}

struct ImageInput {texture:wgpu::Texture,uploaded:Vec<bool>,count:usize}

struct Bake {
    id: AssetId,
    program: Arc<str>,
    surface: Arc<PaperSurface>,
    tile: PaperTile,
    data: wgpu::Buffer,
    priority: startup::Class,
    used: u64,
    stage: Stage,
    image: Option<ImageInput>,
    wants_slope: bool,
    slope: Option<wgpu::TextureView>,
}
impl Bake {
    fn complete(&self) -> bool {
        matches!(self.stage, Stage::Ready { .. }) && (!self.wants_slope || self.slope.is_some())
    }
}

pub(super) struct Recipes {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    empty_image: wgpu::TextureView,
    programs: std::sync::Mutex<Vec<Arc<Programs>>>,
    slope: std::sync::OnceLock<Arc<SlopePrograms>>,
}
pub(super) struct Papers {
    recipes: Arc<Recipes>,
    programs: Vec<Arc<Programs>>,
    slope_programs: Option<Arc<SlopePrograms>>,
    bakes: Vec<Bake>,
    clock: u64,
    submitted: Arc<AtomicBool>,
    decode: Option<object_image_mips::MipDecodeQueue>,
    #[cfg(not(target_arch="wasm32"))]
    wake: Option<Arc<dyn Fn()+Send+Sync>>,
    #[cfg(target_arch="wasm32")]
    decoder: Option<BrowserRasterDecoder>,
    #[cfg(target_arch="wasm32")]
    decoded: std::rc::Rc<std::cell::RefCell<Option<object_image_mips::PreparedImageWork>>>,
    #[cfg(target_arch="wasm32")]
    wake: std::rc::Rc<std::cell::RefCell<Option<std::task::Waker>>>,
}

impl std::ops::Deref for Papers {
    type Target = Recipes;
    fn deref(&self) -> &Self::Target { &self.recipes }
}
impl Recipes {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = bindings::layout(device, "paper bake", &[
            bindings::buffer(0, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Uniform, false, None),
            bindings::buffer(1, wgpu::ShaderStages::FRAGMENT, wgpu::BufferBindingType::Storage { read_only: true }, false, None),
            bindings::texture(2, wgpu::ShaderStages::FRAGMENT, true),
            bindings::sampler(3, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
        ]);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("paper bake"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("paper image"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let empty_image = Papers::image_texture(device, [1, 1]).create_view(&Default::default());
        Self { layout, pipeline_layout, sampler, empty_image, programs: Default::default(), slope: Default::default() }
    }
}
impl Papers {
    pub fn from_recipes(recipes: Arc<Recipes>) -> Self {
        Self { recipes, programs: Vec::new(), slope_programs: None, bakes: Vec::new(), clock: 0, submitted: Arc::new(AtomicBool::new(true)),
            decode:None,wake:Default::default(),
            #[cfg(target_arch="wasm32")] decoder:None,
            #[cfg(target_arch="wasm32")] decoded:Default::default(),
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    pub fn set_wake(&mut self,wake:Arc<dyn Fn()+Send+Sync>) {
        if let Some(queue)=&self.decode {queue.set_wake(wake.clone());}self.wake=Some(wake);
    }
    #[cfg(target_arch="wasm32")]
    pub fn set_decoder(&mut self,decoder:BrowserRasterDecoder) {self.decoder=Some(decoder);}
    pub fn register_waker(&self,wake:&std::task::Waker) {
        #[cfg(not(target_arch="wasm32"))]
        if let Some(queue)=&self.decode {queue.register_waker(wake);}
        #[cfg(target_arch="wasm32")]
        { *self.wake.borrow_mut()=Some(wake.clone()); }
    }
    pub fn clear_waker(&self) {
        #[cfg(not(target_arch="wasm32"))]
        if let Some(queue)=&self.decode {queue.clear_waker();}
        #[cfg(target_arch="wasm32")]
        { self.wake.borrow_mut().take(); }
    }
    pub fn images_ready(&self)->bool {
        #[cfg(target_arch="wasm32")]
        if self.decoded.borrow().is_some() {return true;}
        self.decode.as_ref().is_some_and(|queue|queue.ready())
    }
    pub fn image_waiting(&self)->bool {self.decode.as_ref().is_some_and(|queue|queue.pending()) && !self.images_ready()}
    fn image_complete(bake:&Bake)->bool {
        match &bake.surface.texture {
            TextureSource::Image(source)=>bake.image.as_ref().is_some_and(|input|input.count==(source.extent[0].div_ceil(PAGE_SIZE)*source.extent[1].div_ceil(PAGE_SIZE)) as usize),
            TextureSource::Program(_)=>true,
        }
    }
    fn image_source(bake:&Bake)->Option<&layer_core::Image> {
        if !matches!(bake.stage,Stage::Waiting) || Self::image_complete(bake) {return None;}
        let TextureSource::Image(source)=&bake.surface.texture else {return None;};Some(source)
    }
    fn advance_images(&mut self,device:&PipelineDevice,queue:&wgpu::Queue,held:Option<&TextureSource>)->Result<(),GpuRasterError> {
        #[cfg(target_arch="wasm32")]
        if let Some(result)=self.decoded.borrow_mut().take() && let Some(decode)=&mut self.decode {decode.complete_external(result);}
        while let Some(prepared)=self.decode.as_mut().and_then(|decode|decode.poll()) {
            let object_image_mips::PreparedImageWork::Paper {blob,coordinate,pixels}=prepared else {return Err(GpuRasterError::InvalidImage);};
            for bake in &mut self.bakes {
                let Some(source)=Self::image_source(bake) else {continue;};
                if !source.tiles.get(&coordinate).is_some_and(|expected|Arc::ptr_eq(&blob,expected)) {continue;}
                let pixels=pixels.as_ref().map_err(|error|GpuRasterError::Effect(error.clone()))?;
                if pixels.len()!=(PAGE_SIZE*PAGE_SIZE) as usize {return Err(GpuRasterError::InvalidImage);}
                let extent=source.extent;let columns=extent[0].div_ceil(PAGE_SIZE);
                let input=bake.image.get_or_insert_with(||ImageInput {texture:Self::image_texture(device,extent),
                    uploaded:vec![false;(columns*extent[1].div_ceil(PAGE_SIZE)) as usize],count:0});
                let index=(coordinate[1]*columns+coordinate[0]) as usize;
                if input.uploaded[index] {continue;}
                let origin=coordinate.map(|v|v*PAGE_SIZE);
                queue.write_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:origin[0],y:origin[1],z:0},..input.texture.as_image_copy()},pixels,
                    wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(PAGE_SIZE),rows_per_image:Some(PAGE_SIZE)},
                    wgpu::Extent3d {width:PAGE_SIZE.min(extent[0]-origin[0]),height:PAGE_SIZE.min(extent[1]-origin[1]),depth_or_array_layers:1});
                input.uploaded[index]=true;input.count+=1;
            }
        }
        'inputs: for bake in &self.bakes {
            if held==Some(&bake.surface.texture) {continue;}
            let Some(source)=Self::image_source(bake) else {continue;};
            if self.decode.is_none() {
                let decode=object_image_mips::MipDecodeQueue::new(device)?;
                #[cfg(not(target_arch="wasm32"))]
                if let Some(wake)=&self.wake {decode.set_wake(wake.clone());}
                self.decode=Some(decode);
            }
            let columns=source.extent[0].div_ceil(PAGE_SIZE);
            for index in 0..columns*source.extent[1].div_ceil(PAGE_SIZE) {
                if bake.image.as_ref().is_some_and(|input|input.uploaded[index as usize]) {continue;}
                let coordinate=[index%columns,index/columns];
                let blob=source.tiles.get(&coordinate).ok_or(GpuRasterError::InvalidImage)?;
                if blob.descriptor.channels!=1 || blob.descriptor.depth()!=layer_core::color::SampleDepth::U8 {return Err(GpuRasterError::InvalidImage);}
                if !self.decode.as_mut().unwrap().request_paper(blob.clone(),coordinate) {break 'inputs;}
            }
        }
        #[cfg(target_arch="wasm32")]
        if let Some(request)=self.decode.as_mut().and_then(|decode|decode.take_external()) {
            let object_image_mips::MipDecodeRequest::Paper {blob,coordinate}=request else {return Err(GpuRasterError::InvalidImage);};
            let callback=self.decoder.clone().ok_or_else(||GpuRasterError::Effect("Paper preparation worker unavailable".into()))?;
            let (result,wake)=(self.decoded.clone(),self.wake.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let space=layer_core::color::RgbSpace::Srgb;
                let pixels=callback(blob.clone(),space,space,true).await.and_then(|pixels|match pixels {
                    PreparedImagePixels::NativeSamples(pixels)=>Ok(pixels),_=>Err("Invalid prepared paper samples".into()),
                });
                *result.borrow_mut()=Some(object_image_mips::PreparedImageWork::Paper {blob,coordinate,pixels});
                if let Some(wake)=wake.borrow_mut().take() {wake.wake();}
            });
        }
        Ok(())
    }

    fn image_texture(device: &PipelineDevice, extent: [u32; 2]) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paper image"),
            size: wgpu::Extent3d { width: extent[0], height: extent[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn programs(&mut self, device: &PipelineDevice, texture: &TextureSource) -> Result<Arc<str>, GpuRasterError> {
        let id: Arc<str> = texture.program().map_or(IMAGE_PROGRAM.into(), |instance| instance.program.id.clone());
        if self.programs.iter().any(|p| p.id == id) {
            return Ok(id);
        }
        if let Some(program) = self.recipes.programs.lock().unwrap().iter().find(|p| p.id == id).cloned() {
            self.programs.push(program);
            return Ok(id);
        }
        let mut source = String::from(include_str!("paper_bake.wgsl"));
        match texture.program() {
            Some(instance) => {
                for part in instance.program.wgsl.sources().map_err(|e| GpuRasterError::Effect(e.into()))? {
                    source.push('\n');
                    source.push_str(part);
                }
                source.push_str(&format!("\nfn paper_height(p: vec2<f32>) -> f32 {{ return {}(p, 0u); }}\n", instance.program.entry));
            }
            None => source.push_str(include_str!("paper_image.wgsl")),
        }
        let module = Deferred::wgsl(device, "paper texture", source);
        let pipeline = |entry: &'static str, format: wgpu::TextureFormat| {
            let (device, layout, module) = (device.for_recipe(), self.pipeline_layout.clone(), module.clone());
            Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &module, entry, None, format, "paper texture"))
        };
        let program = Arc::new(Programs {
            id: id.clone(),
            probe: pipeline("paper_probe", wgpu::TextureFormat::R32Float),
            bake: pipeline("paper_bake", wgpu::TextureFormat::R8Unorm),
        });
        let program = {
            let mut programs = self.recipes.programs.lock().unwrap();
            if let Some(shared) = programs.iter().find(|p| p.id == id) { shared.clone() }
            else { programs.push(program.clone()); program }
        };
        self.programs.push(program);
        Ok(id)
    }

    pub fn require(&mut self, device: &PipelineDevice, compiler: Option<&compile::Scope>, binding: &PaperBinding, priority: startup::Class) -> Result<AssetId, GpuRasterError> {
        let tile = PaperTile::new(&binding.surface)?;
        let id = tile.id(&binding.surface);
        self.clock += 1;
        let clock = self.clock;
        let program = self.programs(device, &binding.surface.texture)?;
        if let Some(compiler) = compiler {
            let programs = self.programs.iter().find(|p| p.id == program).expect("prepared paper program");
            compiler.require([&programs.probe, &programs.bake], priority);
        }
        if let Some(bake) = self.bakes.iter_mut().find(|b| b.id == id) {
            bake.priority = bake.priority.min(priority);
            bake.used = clock;
            return Ok(id);
        }
        let parameters = match &binding.surface.texture {
            TextureSource::Program(instance) => instance.gpu_parameters(layer_core::color::RgbSpace::Srgb).map_err(GpuRasterError::Effect)?,
            TextureSource::Image(_) => vec![[0.; 4]],
        };
        let data = initialized_buffer(device, &wgpu::util::BufferInitDescriptor {
            label: Some("paper texture parameters"),
            contents: &parameters.iter().flatten().flat_map(|v| v.to_ne_bytes()).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE,
        })?;
        self.bakes.retain(|b| !(matches!(b.stage, Stage::Waiting) && b.priority == priority && b.surface.texture == binding.surface.texture));
        self.bakes.push(Bake { id: id.clone(), program, surface: binding.surface.clone(), tile, data, priority, used: clock, stage: Stage::Waiting,image:None,
            wants_slope: false, slope: None });
        while self.bakes.len() > CACHED_BAKES {
            let Some(oldest) = self.bakes.iter().enumerate().filter(|(_, b)| matches!(b.stage, Stage::Ready { .. } | Stage::Waiting))
                .min_by_key(|(_, b)| b.used).map(|(i, _)| i) else { break };
            self.bakes.remove(oldest);
        }
        Ok(id)
    }

    pub fn require_slope(&mut self, device: &PipelineDevice, compiler: Option<&compile::Scope>, id: &AssetId, priority: startup::Class) {
        let programs = self.slope_programs.get_or_insert_with(|| self.recipes.slope.get_or_init(|| Arc::new(SlopePrograms::new(device))).clone());
        if let Some(compiler) = compiler {
            compiler.require([&programs.slope, &programs.mip], priority);
        }
        if let Some(bake) = self.bakes.iter_mut().find(|b| b.id == *id) {
            bake.wants_slope = true;
        }
    }

    pub fn slope(&self, id: &AssetId) -> Option<&wgpu::TextureView> {
        self.bakes.iter().find(|b| b.id == *id)?.slope.as_ref()
    }

    pub fn work_pending(&self, id: &AssetId) -> bool {
        self.bakes.iter().any(|bake| bake.id == *id && !bake.complete() && (self.ready_to_step(bake)||self.images_ready()
            || matches!(bake.stage,Stage::Waiting) && !Self::image_complete(bake) && !self.image_waiting()))
    }

    fn ready_to_step(&self, bake: &Bake) -> bool {
        if matches!(bake.stage,Stage::Waiting) && !Self::image_complete(bake) {return false;}
        if matches!(bake.stage, Stage::Ready { .. }) {
            bake.wants_slope && bake.slope.is_none() && self.slope_programs.as_ref().is_some_and(|programs| programs.ready())
        } else {
            let program = self.programs.iter().find(|program| program.id == bake.program).expect("prepared paper program");
            program.probe.ready() && program.bake.ready()
        }
    }

    pub fn requested(&self, id: &AssetId) -> bool {
        self.bakes.iter().any(|bake| bake.id == *id)
    }

    pub fn view(&self, id: &AssetId) -> Option<&wgpu::TextureView> {
        self.bakes.iter().find_map(|b| match &b.stage { Stage::Ready { view, .. } if b.id == *id => Some(view), _ => None })
    }

    pub fn nearest_slope(&self, surface: &PaperSurface, tile: &PaperTile) -> Option<(&wgpu::TextureView, f32)> {
        let orientation = |surface: &PaperSurface| surface.mapping.0[..4].iter().map(|v| v.partial_cmp(&0.)).collect::<Vec<_>>();
        let area = tile.pixels[0] * tile.pixels[1];
        self.bakes.iter()
            .filter(|b| b.surface.texture == surface.texture && orientation(&b.surface) == orientation(surface))
            .filter_map(|b| Some(((b.tile.pixels[0] * b.tile.pixels[1] / area).ln().abs(), b.slope.as_ref()?, (b.tile.pixels[0] / tile.pixels[0]) as f32)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, view, stretch)| (view, stretch))
    }

    pub fn relief(&self, id: &AssetId) -> Option<f32> {
        self.bakes.iter().find_map(|b| match &b.stage { Stage::Ready { relief, .. } if b.id == *id => Some(*relief), _ => None })
    }

    pub fn advance(&mut self, device: &PipelineDevice, queue: &wgpu::Queue, wait: bool, held: Option<&TextureSource>) -> Result<(), GpuRasterError> {
        #[cfg(not(target_arch="wasm32"))]
        if wait {
            pollster::block_on(device.compilation()?.prepare_all(self.programs.iter().flat_map(|program|[&program.probe,&program.bake])
                .chain(self.slope_programs.iter().flat_map(|programs|[&programs.slope,&programs.mip])))).map_err(GpuRasterError::Effect)?;
        }
        self.advance_images(device,queue,held)?;
        if !wait && !self.submitted.load(Ordering::Acquire) {
            return Ok(());
        }
        loop {
            let Some(index) = self.bakes.iter().enumerate()
                .filter(|(_, b)| !b.complete() && (matches!(b.stage, Stage::Ready { .. }) || held != Some(&b.surface.texture)))
                .filter(|(_,b)|!matches!(b.stage,Stage::Waiting) || Self::image_complete(b))
                .min_by_key(|(_, b)| (b.priority, matches!(b.stage, Stage::Waiting), std::cmp::Reverse(b.used))).map(|(i, _)| i) else {
                return Ok(());
            };
            let progressed = self.step(index, device, queue, wait)?;
            if !wait {
                if progressed {
                    let submitted = Arc::new(AtomicBool::new(false));
                    let signal = crate::CompletionFlag(submitted.clone());
                    device.on_queue_done(queue,move || drop(signal));
                    self.submitted = submitted;
                    device.request_poll();
                }
                return Ok(());
            }
            if !progressed {
                device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| GpuRasterError::WaitFailed(e.to_string()))?;
            }
        }
    }

    fn step(&mut self, index: usize, device: &PipelineDevice, queue: &wgpu::Queue, wait: bool) -> Result<bool, GpuRasterError> {
        if !wait && !self.ready_to_step(&self.bakes[index]) { return Ok(false); }
        if let Stage::Ready { view, relief } = &self.bakes[index].stage {
            let bake = &self.bakes[index];
            let Some(programs) = self.slope_programs.as_ref().filter(|_| bake.wants_slope && bake.slope.is_none()) else { return Ok(false) };
            let scale = relief * REFERENCE_RELIEF_UM / (bake.tile.texel_mm() as f32 * 1000.);
            let slope = programs.encode(device, queue, view, scale)?;
            self.bakes[index].slope = Some(slope);
            return Ok(true);
        }
        let programs = self.programs.iter().find(|p| p.id == self.bakes[index].program).expect("prepared paper program");
        let bake = &mut self.bakes[index];
        let next = match &mut bake.stage {
            Stage::Waiting => {
                let image = match &bake.surface.texture {
                    TextureSource::Image(_) => {
                        bake.image.take().ok_or(GpuRasterError::InvalidImage)?.texture.create_view(&Default::default())
                    }
                    TextureSource::Program(_) => self.recipes.empty_image.clone(),
                };
                let probe = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("paper probe"),
                    size: wgpu::Extent3d { width: PROBE_SIZE, height: PROBE_SIZE, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let readback = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("paper probe readback"),
                    size: u64::from(PROBE_SIZE * PROBE_SIZE * 4),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                let group = Self::group(device, &self.recipes.layout, &bake.tile.parameters(&bake.surface, 0., 1.), &bake.data, &image, &self.recipes.sampler)?;
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("paper probe") });
                {
                    let view = probe.create_view(&Default::default());
                    let mut pass = Self::pass(&mut encoder, &view, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
                    pass.set_pipeline(&programs.probe);
                    pass.set_bind_group(0, &group, &[]);
                    pass.draw(0..3, 0..1);
                }
                encoder.copy_texture_to_buffer(probe.as_image_copy(), wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PROBE_SIZE * 4), rows_per_image: Some(PROBE_SIZE) },
                }, wgpu::Extent3d { width: PROBE_SIZE, height: PROBE_SIZE, depth_or_array_layers: 1 });
                device.submit(queue,[encoder.finish()]);
                let state = Arc::new(AtomicU8::new(MAPPING));
                let signal = state.clone();
                let abandoned=MappingCompletion(signal.clone());
                readback.slice(..).map_async(wgpu::MapMode::Read, {let callback=move |result:Result<(),wgpu::BufferAsyncError>| {
                    signal.store(if result.is_ok() { MAPPED } else { FAILED }, Ordering::Release);drop(abandoned);
                }; #[cfg(not(target_arch="wasm32"))] let callback=device.callback(callback);callback});
                device.request_poll();
                Stage::Probing { readback, state, image }
            }
            Stage::Probing { readback, state, image } => {
                match state.load(Ordering::Acquire) {
                    MAPPED => {}
                    FAILED => return Err(GpuRasterError::MapFailed("Paper probe readback failed".into())),
                    _ => return Ok(false),
                }
                let mut heights: Vec<f32> = readback.slice(..).get_mapped_range().map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
                    .as_chunks::<4>().0.iter().map(|b| f32::from_ne_bytes(*b)).filter(|v| v.is_finite()).collect();
                readback.unmap();
                if heights.is_empty() {
                    return Err(GpuRasterError::Effect("Paper texture produced no finite heights".into()));
                }
                let mut percentile = |p: f64| {
                    let index = ((heights.len() - 1) as f64 * p).round() as usize;
                    *heights.select_nth_unstable_by(index, f32::total_cmp).1
                };
                let low = percentile(LOW_PERCENTILE);
                let high = percentile(HIGH_PERCENTILE).max(low + 1e-3);
                let relief = match bake.surface.texture {
                    TextureSource::Program(_) => (high - low) / REFERENCE_RELIEF_UM,
                    TextureSource::Image(_) => 1.,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("paper texture"),
                    size: wgpu::Extent3d { width: bake.tile.texels[0], height: bake.tile.texels[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let group = Self::group(device, &self.recipes.layout, &bake.tile.parameters(&bake.surface, low, high), &bake.data, image, &self.recipes.sampler)?;
                Stage::Baking { texture, group, next: 0, relief }
            }
            Stage::Baking { texture, group, next, relief } => {
                let columns = bake.tile.texels[0].div_ceil(PAGE_SIZE);
                let total = columns * bake.tile.texels[1].div_ceil(PAGE_SIZE);
                let end = if wait { total } else { (*next + PAGES_PER_STEP).min(total) };
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("paper bake") });
                {
                    let view = texture.create_view(&Default::default());
                    let load = if *next == 0 { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load };
                    let mut pass = Self::pass(&mut encoder, &view, load);
                    pass.set_pipeline(&programs.bake);
                    pass.set_bind_group(0, &*group, &[]);
                    for page in *next..end {
                        let [x, y] = [page % columns * PAGE_SIZE, page / columns * PAGE_SIZE];
                        pass.set_scissor_rect(x, y, PAGE_SIZE.min(bake.tile.texels[0] - x), PAGE_SIZE.min(bake.tile.texels[1] - y));
                        pass.draw(0..3, 0..1);
                    }
                }
                device.submit(queue,[encoder.finish()]);
                *next = end;
                if end < total {
                    return Ok(true);
                }
                Stage::Ready { view: texture.create_view(&Default::default()), relief: *relief }
            }
            Stage::Ready { .. } => return Ok(false),
        };
        bake.stage = next;
        Ok(true)
    }

    fn group(device: &PipelineDevice, layout: &wgpu::BindGroupLayout, parameters: &[f32; 20], data: &wgpu::Buffer,
        image: &wgpu::TextureView, sampler: &wgpu::Sampler) -> Result<wgpu::BindGroup, GpuRasterError> {
        let uniform = initialized_buffer(device, &wgpu::util::BufferInitDescriptor {
            label: Some("paper bake parameters"),
            contents: &parameters.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        })?;
        Ok(bindings::group(device, "paper bake", layout, [uniform.as_entire_binding(), data.as_entire_binding(),
            wgpu::BindingResource::TextureView(image), wgpu::BindingResource::Sampler(sampler)]))
    }

    fn pass<'a>(encoder: &'a mut wgpu::CommandEncoder, view: &'a wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("paper texture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::paper::{reference, PaperBinding, PaperSurface};

    #[test]
    fn stored_image_paper_does_not_wait_for_reads_on_the_render_owner() {
        use layer_core::{Document,authored::{Artwork,PaintBase,SourceTarget},color::source::{SourceBuilder,SourceChannels,SourceInterpretation}};
        let interpretation=SourceInterpretation {channels:SourceChannels::Gray,depth:layer_core::color::SampleDepth::U8,profile:Default::default(),profile_assumed:false};
        let extent=[260,261];
        let source=|| {
            let mut builder=SourceBuilder::new(extent,interpretation.clone(),1<<20).unwrap();
            for y in 0..extent[1] {builder.push_row(&(0..extent[0]).map(|x|((x*37+y*101)%251) as u8).collect::<Vec<_>>()).unwrap();}
            let mut source=builder.finish().unwrap();source.resolution=Some(layer_core::ImageResolution::ppi(300));Arc::new(source)
        };
        let memory=source();let source=source();let image=layer_core::Image::new(source.clone());
        let mut artwork=Artwork::new(source.extent).unwrap();
        let (_,SourceTarget::Paint(target))=crate::test_support::add_paint(&mut artwork,"Stored paper",source.extent) else {unreachable!()};
        artwork.paint.get_mut(target).unwrap().base=Some(PaintBase::new(image.clone()));
        let retained=layer_core::Editor::new(Document::from_artwork(artwork).unwrap()).retained_tiles();
        let store=crate::test_support::GatedStore::new(&retained,false);
        assert!(source.tiles.values().all(|tile|tile.memory_compressed().unwrap().is_none()));
        let texture=TextureSource::Image(image);
        let binding=PaperBinding {surface:Arc::new(PaperSurface {texture,mapping:reference(extent,[300.;2]).mapping}),target_offset:[0;2]};
        let mut renderer=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let id=renderer.papers.require(&renderer.device,None,&binding,startup::BRUSH).unwrap();
        let bake=renderer.papers.bakes.iter().find(|bake|bake.id==id).unwrap();
        let programs=renderer.papers.programs.iter().find(|programs|programs.id==bake.program).unwrap();
        pollster::block_on(renderer.compilation.prepare_all([&programs.probe,&programs.bake])).unwrap();
        let (wake,awoken)=std::sync::mpsc::channel();
        renderer.set_preparation_wake(Arc::new(move || {let _=wake.send(());}));
        let (send,receive)=std::sync::mpsc::channel();
        let owner=std::thread::spawn(move || {
            let result=renderer.papers.advance(&renderer.device,&renderer.queue,false,None);
            if renderer.papers.image_waiting() {
                assert!(renderer.image_decode_waiting());
                let target=Arc::new(std::sync::Mutex::new(object_image_mips::CoordinateSlot::new(&Default::default())));
                renderer.source_results.push_back((renderer.device.working_space(),object_image_mips::PreparedImageWork::Coordinates {
                    destination:Arc::downgrade(&target),pixels:Ok(Arc::new(vec![0;8])),
                }));
                assert!(!renderer.image_decode_waiting(),"ready source work must run while paper reads are pending");
                renderer.source_results.clear();
            }
            {
                struct Notify(std::sync::mpsc::Sender<()>);
                impl std::task::Wake for Notify {fn wake(self:Arc<Self>) {let _=self.0.send(());}}
                let (notify,woken)=std::sync::mpsc::channel();
                let waker=std::task::Waker::from(Arc::new(Notify(notify)));
                let mut future=std::pin::pin!(renderer.await_image_decode());
                assert!(std::future::Future::poll(future.as_mut(),&mut std::task::Context::from_waker(&waker)).is_pending());
                let _=send.send(());
                woken.recv_timeout(std::time::Duration::from_secs(5)).expect("stored paper decode must wake its snapshot waiter");
                assert!(matches!(std::future::Future::poll(future.as_mut(),&mut std::task::Context::from_waker(&waker)),std::task::Poll::Ready(Ok(()))));
            }
            (renderer,result)
        });
        let returned=receive.recv_timeout(std::time::Duration::from_secs(2)).is_ok();
        store.gate.entered();store.gate.release();
        let (mut renderer,result)=owner.join().unwrap();result.unwrap();
        assert!(returned,"paper preparation waited for storage on the render owner");
        awoken.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(renderer.papers.images_ready());
        assert!(!renderer.papers.image_waiting());
        assert!(renderer.frame_preparation_pending(),"ready paper decode must retain its delegated frame consumer until consumed");
        assert!(renderer.papers.view(&id).is_none());
        let stored=bake_binding(&mut renderer,binding.clone());
        renderer.papers.bakes.clear();
        let mut surface=(*binding.surface).clone();surface.texture=TextureSource::Image(layer_core::Image::new(memory));
        let expected=bake_binding(&mut renderer,PaperBinding {surface:Arc::new(surface),..binding});
        assert_eq!(stored,expected);
    }

    #[test]
    fn device_loss_before_paper_parameters_returns_an_error() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let surface = PaperSurface { texture: layer_core::paper::default_texture(), mapping: reference([64, 64], [300.; 2]).mapping };
        let binding = PaperBinding { surface: Arc::new(surface.clone()), target_offset: [0; 2] };
        let previous = renderer.papers.require(&renderer.device, None, &binding, startup::BRUSH).unwrap();
        let mut changed = surface;
        changed.mapping.0[4] += 0.5;
        let binding = PaperBinding { surface: Arc::new(changed), target_offset: [0; 2] };
        assert_ne!(Some(previous), PaperTile::id_for(&binding));
        renderer.device.destroy();
        let result = renderer.papers.require(&renderer.device, None, &binding, startup::BRUSH);
        assert!(matches!(result, Err(GpuRasterError::MapFailed(_))));
    }

    #[test]
    fn device_loss_before_paper_bake_uniform_does_not_panic() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let surface = PaperSurface { texture: layer_core::paper::default_texture(), mapping: reference([64, 64], [300.; 2]).mapping };
        let binding = PaperBinding { surface: Arc::new(surface), target_offset: [0; 2] };
        let id = renderer.papers.require(&renderer.device, None, &binding, startup::BRUSH).unwrap();
        let bake = renderer.papers.bakes.iter().find(|bake| bake.id == id).unwrap();
        let parameters = bake.tile.parameters(&bake.surface, 0., 1.);
        renderer.device.destroy();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Papers::group(&renderer.device, &renderer.papers.layout, &parameters, &bake.data,
                &renderer.papers.empty_image, &renderer.papers.sampler)
        }));
        assert!(result.is_ok(), "Paper bake uniform initialization must report device loss without panicking");
        assert!(matches!(result.unwrap(), Err(GpuRasterError::MapFailed(_))));
    }

    fn bake(renderer: &mut WgpuRasterizer, texture: TextureSource) -> (Vec<u8>, [u32; 2], f32) {
        bake_binding(renderer, PaperBinding { surface: Arc::new(PaperSurface { texture, mapping: reference([64, 64], [300.; 2]).mapping }), target_offset: [0; 2] })
    }

    fn bake_binding(renderer: &mut WgpuRasterizer, binding: PaperBinding) -> (Vec<u8>, [u32; 2], f32) {
        let id = renderer.papers.require(&renderer.device, None, &binding, startup::BRUSH).unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        while renderer.papers.view(&id).is_none() {
            renderer.papers.advance(&renderer.device, &renderer.queue, true, None).unwrap();
            assert!(std::time::Instant::now()<deadline,"paper preparation did not complete");
            std::thread::yield_now();
        }
        let bake = renderer.papers.bakes.iter().find(|b| b.id == id).unwrap();
        let Stage::Ready { view, relief } = &bake.stage else { panic!("unfinished bake") };
        let texture = view.texture();
        let [width, height] = bake.tile.texels;
        let row = width.div_ceil(256) * 256;
        let readback = renderer.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = renderer.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(texture.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) } },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
        renderer.queue.submit([encoder.finish()]);
        readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        renderer.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = readback.slice(..).get_mapped_range().unwrap();
        let pixels = data.chunks(row as usize).flat_map(|r| r[..width as usize].to_vec()).collect();
        (pixels, [width, height], *relief)
    }

    fn scaled(texture: &TextureSource, scale: f64, flip: bool) -> PaperBinding {
        let mut mapping = layer_core::paper::with_scale(&reference([64, 64], [300.; 2]), scale, [300.; 2]).mapping;
        if flip { mapping.0[0] = -mapping.0[0]; }
        PaperBinding { surface: Arc::new(PaperSurface { texture: texture.clone(), mapping }), target_offset: [0; 2] }
    }

    #[test]
    fn newer_scales_replace_unstarted_bakes_and_bakes_advance_one_at_a_time() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let [rough, hot] = ["rough", "hot_press"].map(|id| layer_core::paper::built_in_paper(id).unwrap().texture.clone().unwrap());
        for texture in [&rough, &hot] { bake_binding(&mut renderer, scaled(texture, 1., false)); }
        for scale in [1.5, 2., 2.5] {
            renderer.papers.require(&renderer.device, None, &scaled(&rough, scale, false), startup::OTHER).unwrap();
        }
        let waiting = |papers: &Papers| papers.bakes.iter().filter(|b| matches!(b.stage, Stage::Waiting)).map(|b| b.tile.pixels[0]).collect::<Vec<_>>();
        assert_eq!(waiting(&renderer.papers), [PaperTile::new(&scaled(&rough, 2.5, false).surface).unwrap().pixels[0]], "only the latest unstarted scale stays queued");
        renderer.papers.require(&renderer.device, None, &scaled(&hot, 2., false), startup::OTHER).unwrap();
        let started = |papers: &Papers| papers.bakes.iter().filter(|b| matches!(b.stage, Stage::Probing { .. } | Stage::Baking { .. })).map(|b| b.surface.texture.clone()).collect::<Vec<_>>();
        renderer.papers.advance(&renderer.device, &renderer.queue, false, Some(&hot)).unwrap();
        assert_eq!(started(&renderer.papers).as_slice(), std::slice::from_ref(&rough), "a held paper waits while another advances");
        renderer.papers.advance(&renderer.device, &renderer.queue, true, None).unwrap();
        renderer.papers.require(&renderer.device, None, &scaled(&rough, 3., false), startup::OTHER).unwrap();
        renderer.papers.require(&renderer.device, None, &scaled(&hot, 3., false), startup::OTHER).unwrap();
        renderer.papers.advance(&renderer.device, &renderer.queue, false, None).unwrap();
        assert_eq!(started(&renderer.papers), [hot], "one bake advances, the most recently requested first");
        renderer.papers.advance(&renderer.device, &renderer.queue, true, None).unwrap();
        assert!(renderer.papers.bakes.iter().all(|b| matches!(b.stage, Stage::Ready { .. })));
    }

    #[test]
    fn a_bake_step_waits_until_the_previous_submission_completes() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let rough = layer_core::paper::built_in_paper("rough").unwrap().texture.clone().unwrap();
        bake_binding(&mut renderer, scaled(&rough, 1., false));
        let id = renderer.papers.require(&renderer.device, None, &scaled(&rough, 3., false), startup::OTHER).unwrap();
        let stage = |papers: &Papers| papers.bakes.iter().find(|b| b.id == id).map(|b| match b.stage { Stage::Waiting => 0, Stage::Probing { .. } => 1, Stage::Baking { next, .. } => 2 + next, Stage::Ready { .. } => u32::MAX }).unwrap();
        renderer.papers.advance(&renderer.device, &renderer.queue, false, None).unwrap();
        assert_eq!(stage(&renderer.papers), 1);
        assert!(!renderer.papers.submitted.load(Ordering::Acquire), "the probe is still in flight");
        renderer.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        assert!(renderer.papers.submitted.load(Ordering::Acquire));
        while stage(&renderer.papers) < 2 + PAGES_PER_STEP {
            renderer.papers.advance(&renderer.device, &renderer.queue, false, None).unwrap();
            renderer.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        assert_eq!(stage(&renderer.papers), 2 + PAGES_PER_STEP, "one step of pages per completed submission");
    }

    #[test]
    fn a_new_scale_borrows_the_nearest_slope_of_the_same_paper_and_orientation() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let [rough, hot] = ["rough", "hot_press"].map(|id| layer_core::paper::built_in_paper(id).unwrap().texture.clone().unwrap());
        let baked = [1., 3.].map(|scale| {
            let binding = scaled(&rough, scale, false);
            renderer.require_paper_slope(&binding, startup::OTHER).unwrap();
            renderer.papers.slope(&PaperTile::id_for(&binding).unwrap()).unwrap().clone()
        });
        let nearest = |binding: PaperBinding| renderer.papers.nearest_slope(&binding.surface, &PaperTile::new(&binding.surface).unwrap())
            .map(|(view, stretch)| (view.clone(), stretch));
        let (view, stretch) = nearest(scaled(&rough, 1.2, false)).unwrap();
        assert_eq!(view, baked[0]);
        assert!((stretch - 1. / 1.2).abs() < 1e-2, "slopes flatten as the paper stretches: {stretch}");
        assert_eq!(nearest(scaled(&rough, 2.5, false)).map(|(view, _)| view).as_ref(), Some(&baked[1]));
        assert_eq!(nearest(scaled(&hot, 1., false)), None, "another paper is never borrowed");
        assert_eq!(nearest(scaled(&rough, 1., true)), None, "nor a flipped tile");
    }

    fn preview(renderer: &mut WgpuRasterizer, texture: &TextureSource) -> Vec<u8> {
        let size = 2 * layer_core::paper::PREVIEW_SIZE;
        let ppi = size as f64 / layer_core::paper::PREVIEW_MM * 25.4;
        let binding = PaperBinding { surface: Arc::new(PaperSurface { texture: texture.clone(), mapping: reference([size as u32; 2], [ppi; 2]).mapping }), target_offset: [0; 2] };
        let (pixels, [width, _], relief) = bake_binding(renderer, binding);
        let heights: Vec<f32> = (0..size * size).map(|i| f32::from(pixels[(i / size) * width as usize + i % size]) / 255. * relief * REFERENCE_RELIEF_UM).collect();
        layer_core::paper::preview(&heights)
    }

    #[test]
    #[ignore = "writes assets/filters/paper_previews.gray"]
    fn write_built_in_paper_previews() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut atlas = Vec::new();
        for definition in layer_core::bundled_texture_catalog().filters() {
            for preset in &definition.presets {
                atlas.extend(preview(&mut renderer, &TextureSource::Program(definition.preset(preset).unwrap())));
            }
        }
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/filters/paper_previews.gray"), atlas).unwrap();
    }

    #[test]
    fn built_in_paper_previews_match_their_bakes() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let papers = layer_core::paper::built_in_papers();
        assert_eq!(papers.len(), 1 + layer_core::bundled_texture_catalog().filters().iter().map(|d| d.presets.len()).sum::<usize>());
        for paper in papers {
            let Some(texture) = &paper.texture else {
                assert_eq!(paper.id.as_ref(), layer_core::paper::NO_TEXTURE);
                assert_eq!(paper.preview.len(), layer_core::paper::PREVIEW_SIZE * layer_core::paper::PREVIEW_SIZE);
                assert_eq!(&*paper.preview, layer_core::paper::preview(&vec![0.; 4 * layer_core::paper::PREVIEW_SIZE * layer_core::paper::PREVIEW_SIZE]));
                assert!(paper.preview.iter().all(|value| *value == paper.preview[0]));
                continue;
            };
            let fresh = preview(&mut renderer, texture);
            let difference = fresh.iter().zip(paper.preview.iter()).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / fresh.len() as f64;
            assert!(difference < 1., "{}: preview is {difference:.2} codes from its bake; run write_built_in_paper_previews", paper.id);
        }
    }

    fn seam_ratio(pixels: &[u8], [width, height]: [u32; 2]) -> f64 {
        let at = |x: u32, y: u32| f64::from(pixels[(y * width + x) as usize]);
        let (mut seam, mut interior) = (0., 0.);
        for y in 0..height {
            seam += (at(width - 1, y) - at(0, y)).abs();
            interior += (at(width / 2, y) - at(width / 2 - 1, y)).abs();
        }
        seam / interior.max(1.)
    }

    #[test]
    fn every_built_in_paper_bakes_a_seamless_tile_with_its_relief() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut relief = std::collections::BTreeMap::new();
        for definition in layer_core::bundled_texture_catalog().filters() {
            for preset in &definition.presets {
                let instance = definition.preset(preset).unwrap();
                let (pixels, extent, measured) = bake(&mut renderer, TextureSource::Program(instance));
                let period = f64::from(definition.program.period.unwrap()) * 300. / 25.4;
                assert_eq!(extent, [period.round() as u32; 2], "{}", preset.id);
                let mean = pixels.iter().map(|v| f64::from(*v)).sum::<f64>() / pixels.len() as f64;
                let spread = (pixels.iter().map(|v| (f64::from(*v) - mean).powi(2)).sum::<f64>() / pixels.len() as f64).sqrt();
                assert!(spread > 20., "{} has no visible tooth: {spread}", preset.id);
                assert!(seam_ratio(&pixels, extent) < 2.5, "{} shows a seam", preset.id);
                relief.insert(preset.id.to_string(), measured);
            }
        }
        assert!(relief["hot_press"] < relief["cold_press"] && relief["cold_press"] < relief["rough"] && relief["rough"] < relief["torchon"], "{relief:?}");
        assert!(relief["bristol"] < relief["drawing_paper"], "{relief:?}");
        assert!(relief["sanded_pastel"] < relief["coarse_sanded"] && relief["gesso_panel"] < relief["brushed_gesso"], "{relief:?}");
        assert!(relief["drafting_film"] < relief["chalkboard"] && relief["sand"] < relief["asphalt"] && relief["rough_linen"] < relief["burlap"], "{relief:?}");
    }

    fn blurred(values: &[f64], [width, height]: [usize; 2], radius: usize) -> Vec<f64> {
        let mut out = values.to_vec();
        for _ in 0..2 {
            for (count, lines, along, across) in [(width, height, 1, width), (height, width, width, 1)] {
                let source = out.clone();
                let taps = (2 * radius + 1) as f64;
                for line in 0..lines {
                    let at = |i: usize| source[line * across + (i % count) * along];
                    let mut sum: f64 = (count - radius..=count + radius).map(at).sum();
                    for i in 0..count {
                        out[line * across + i * along] = sum / taps;
                        sum += at(i + radius + 1) - at(i + count - radius);
                    }
                }
            }
        }
        out
    }

    #[test]
    #[ignore = "prints relief statistics and bake times of the built-in papers; PAPERS picks papers by ID"]
    fn built_in_paper_relief_statistics() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let only = std::env::var("PAPERS").ok();
        println!("paper\tperiod\tfirst_ms\trelief_um\tbake_ms\tns_texel\tmean\tskew\tfine_um\tmid_um\tcoarse_um\treach25\treach50\treach75\treach100");
        for definition in layer_core::bundled_texture_catalog().filters() {
            for preset in &definition.presets {
                if only.as_deref().is_some_and(|only| !only.split(',').any(|id| id == preset.id.as_ref())) { continue; }
                let instance = definition.preset(preset).unwrap();
                let start = std::time::Instant::now();
                let (pixels, [width, height], relief) = bake(&mut renderer, TextureSource::Program(instance.clone()));
                let first = start.elapsed().as_secs_f64() * 1000.;
                let mut time = |ppi: f64| {
                    let mut times: Vec<f64> = (2..7).map(|seed| {
                        let mut varied = instance.clone();
                        varied.set("seed", layer_core::EffectValue::Number(seed as f32)).unwrap();
                        let binding = PaperBinding { surface: Arc::new(PaperSurface { texture: TextureSource::Program(varied), mapping: reference([64, 64], [ppi; 2]).mapping }), target_offset: [0; 2] };
                        let start = std::time::Instant::now();
                        renderer.papers.require(&renderer.device, None, &binding, startup::BRUSH).unwrap();
                        renderer.papers.advance(&renderer.device, &renderer.queue, true, None).unwrap();
                        start.elapsed().as_secs_f64() * 1000.
                    }).collect();
                    times.sort_by(f64::total_cmp);
                    times[2]
                };
                let texels = |ppi: f64| (f64::from(definition.program.period.unwrap()) * ppi / 25.4).round().min(f64::from(MAX_TEXELS)).powi(2);
                let (base, fine) = (time(300.), time(1200.));
                let ns_per_texel = (fine - base) * 1e6 / (texels(1200.) - texels(300.));
                let relief_um = f64::from(relief * REFERENCE_RELIEF_UM);
                let heights: Vec<f64> = pixels.iter().map(|v| f64::from(*v) / 255. * relief_um).collect();
                let n = heights.len() as f64;
                let mean = heights.iter().sum::<f64>() / n;
                let variance = heights.iter().map(|h| (h - mean).powi(2)).sum::<f64>() / n;
                let skew = heights.iter().map(|h| (h - mean).powi(3)).sum::<f64>() / n / variance.powf(1.5);
                let texel = f64::from(definition.program.period.unwrap()) / f64::from(width);
                let extent = [width as usize, height as usize];
                let fine_cut = blurred(&heights, extent, (0.15 / texel).round().max(1.) as usize);
                let coarse = blurred(&heights, extent, (2.5 / texel).round() as usize);
                let deviation = |values: &mut dyn Iterator<Item = f64>| { let v: Vec<f64> = values.collect(); let m = v.iter().sum::<f64>() / n; (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n).sqrt() };
                let fine = deviation(&mut heights.iter().zip(&fine_cut).map(|(h, b)| h - b));
                let mid = deviation(&mut fine_cut.iter().zip(&coarse).map(|(a, b)| a - b));
                let large = deviation(&mut coarse.iter().copied());
                let reach = |pressure: f64| heights.iter().filter(|h| **h >= 0.5 * relief_um - 50. * pressure).count() as f64 / n;
                println!("{}\t{}\t{first:.1}\t{relief_um:.0}\t{base:.2}\t{ns_per_texel:.2}\t{:.3}\t{skew:.2}\t{fine:.1}\t{mid:.1}\t{large:.1}\t{:.2}\t{:.2}\t{:.2}\t{:.2}",
                    preset.id, definition.program.period.unwrap(), mean / relief_um, reach(0.25), reach(0.5), reach(0.75), reach(1.));
            }
        }
    }

    #[test]
    fn turned_and_flipped_papers_bake_the_turned_tile() {
        use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
        let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: layer_core::color::SampleDepth::U8,
            profile: Default::default(), profile_assumed: false };
        let mut builder = SourceBuilder::new([24, 17], interpretation, 1 << 20).unwrap();
        for y in 0..17 { builder.push_row(&(0..24).map(|x| ((x * 37 + y * 101) % 251) as u8).collect::<Vec<_>>()).unwrap(); }
        let mut source = builder.finish().unwrap();
        source.resolution = Some(layer_core::ImageResolution::ppi(300));
        let texture = TextureSource::Image(layer_core::Image::new(Arc::new(source)));
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let k = 300. / 25.4;
        let binding = |mapping: [f64; 6]| PaperBinding { surface: Arc::new(PaperSurface { texture: texture.clone(), mapping: layer_core::Affine64(mapping) }), target_offset: [0; 2] };
        let (straight, _, _) = bake_binding(&mut renderer, binding([k, 0., 0., k, 0., 0.]));
        for mapping in [[0., k, -k, 0., 17., 0.], [-k, 0., 0., k, 24., 0.], [0., -k, -k, 0., 17., 24.]] {
            let tile = PaperTile::new(&binding(mapping).surface).unwrap();
            let (turned, [width, height], _) = bake_binding(&mut renderer, binding(mapping));
            let [a, b, c, d, x, y] = mapping;
            let det = a * d - b * c;
            for j in 0..height {
                for i in 0..width {
                    let p = [(tile.origin[0] + i64::from(i)) as f64 + 0.5 - x, (tile.origin[1] + i64::from(j)) as f64 + 0.5 - y];
                    let mm = [(d * p[0] - c * p[1]) / det, (a * p[1] - b * p[0]) / det];
                    let source = [(mm[0] * k).floor().rem_euclid(24.) as usize, (mm[1] * k).floor().rem_euclid(17.) as usize];
                    assert_eq!(turned[(j * width + i) as usize], straight[source[1] * 24 + source[0]], "{mapping:?} texel {i},{j}");
                }
            }
        }
    }

    #[test]
    fn image_papers_at_their_own_resolution_are_copied_exactly() {
        use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
        let interpretation = SourceInterpretation { channels: SourceChannels::Gray, depth: layer_core::color::SampleDepth::U8,
            profile: Default::default(), profile_assumed: false };
        let mut builder = SourceBuilder::new([24, 17], interpretation, 1 << 20).unwrap();
        let sample = |x: u32, y: u32| ((x * 37 + y * 101) % 251) as u8;
        for y in 0..17 { builder.push_row(&(0..24).map(|x| sample(x, y)).collect::<Vec<_>>()).unwrap(); }
        let mut source = builder.finish().unwrap();
        source.resolution = Some(layer_core::ImageResolution::ppi(300));
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (pixels, extent, relief) = bake(&mut renderer, TextureSource::Image(layer_core::Image::new(Arc::new(source))));
        assert_eq!((extent, relief), ([24, 17], 1.));
        let (low, high) = (0., 250.);
        for y in 0..17 {
            for x in 0..24 {
                let expected = ((f64::from(sample(x, y)) - low) / (high - low) * 255.).round().clamp(0., 255.);
                assert!((f64::from(pixels[(y * 24 + x) as usize]) - expected).abs() <= 1., "texel {x},{y}");
            }
        }
    }
}

struct MappingCompletion(Arc<AtomicU8>);
impl Drop for MappingCompletion {fn drop(&mut self) {let _=self.0.compare_exchange(MAPPING,FAILED,Ordering::AcqRel,Ordering::Acquire);}}
