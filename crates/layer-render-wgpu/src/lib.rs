//! GPU-resident implementation of Layer's GPU canvas contract.
//!
//! The current dry brush uses instanced quads and fixed-function blending. All
//! paint-layer and composite pixels remain in GPU textures. Explicit export and
//! bounded asynchronous UI thumbnails and explicit color samples are the only
//! readbacks. Destination-aware brush stages can be added beside this
//! fast path without changing the engine packet or duplicating pixel semantics.

use layer_core::{
    AssetId, BRISTLE_GRAIN_TEXTURE_ASSET, BrushAccumulation, BrushBlendMode, BrushExecution,
    BrushGrainBehavior, BrushTip, ColorMixSpace, DualCombineMode, Layer, LayerId, LayerKind,
    LiquifyMode, PAINTBRUSH_TEXTURE_ASSET, PAPER_GRAIN_TEXTURE_ASSET, PENCIL_TEXTURE_ASSET,
    StrokeId, WATERCOLOR_TIP_TEXTURE_ASSET, WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET,
};
use layer_render::{
    CanvasRenderer, Dab, DabBatch, DabBatchKind, DabMode, FramePacket, HostImage, PixelFormat,
    ReadbackImage,
};
use std::{borrow::Cow, fmt, mem, num::NonZeroU64, sync::mpsc, time::Duration};

mod canvas_preview;
mod color_sample;
mod effect_validation;
mod effects;
mod layer_masks;
#[cfg(test)]
mod layer_tests;
mod present;
mod scene;
mod telemetry;
mod thumbnails;
pub use present::ViewportPresenter;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const EXPORT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const INITIAL_DAB_BYTES: u64 = 4 * 1024 * 1024;
const INITIAL_STYLE_RECORDS: usize = 128;
const INITIAL_TARGET_RECORDS: usize = 257;
const READBACK_TIMEOUT: Duration = Duration::from_secs(30);
const WHITE_MASK_ASSET: &str = "builtin:brush-tip/solid-white-v1";
const PAGE_SIZE: u32 = 256;
const PAGE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64 * 4;
const SCALAR_PAGE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64;
const RESERVOIR_SIZE: u32 = 64;
const RESERVOIR_BYTES: u64 = RESERVOIR_SIZE as u64 * RESERVOIR_SIZE as u64 * 4 * 2;
const PROCEDURAL_GRAIN_SIZE: u32 = 256;
const WATERCOLOR_TRANSPORT_STEPS: u32 = 3;

fn needs_scene(layers: &[Layer]) -> bool {
    layers.iter().any(|l| {
        !l.operations.is_empty()
            || l.mask.is_some()
            || matches!(l.kind, LayerKind::Group | LayerKind::Effect)
            || l.properties.clipped
            || l.properties.parent.is_some()
            || l.properties.blend != layer_core::LayerBlend::Normal
            || l.properties.offset != layer_core::Point::default()
    })
}

/// Native writes reuse staging resources. On web, Queue::write_buffer transfers
/// Wasm bytes directly; mapped slices would allocate and copy through JS memory.
struct Uploads {
    #[cfg(not(target_arch = "wasm32"))]
    belt: wgpu::util::StagingBelt,
}
impl Uploads {
    fn new(device: &wgpu::Device, chunk_size: u64) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = (device, chunk_size);
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            belt: wgpu::util::StagingBelt::new(device.clone(), chunk_size),
        }
    }
    fn write(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        target: &wgpu::Buffer,
        bytes: &[u8],
    ) {
        self.write_at(encoder, queue, target, 0, bytes);
    }
    fn write_at(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        target: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = queue;
            self.belt
                .write_buffer(
                    encoder,
                    target,
                    offset,
                    wgpu::BufferSize::new(bytes.len() as u64).unwrap(),
                )
                .copy_from_slice(bytes);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = encoder;
            queue.write_buffer(target, offset, bytes);
        }
    }
    fn finish(&mut self, encoder: &wgpu::CommandEncoder) {
        #[cfg(not(target_arch = "wasm32"))]
        self.belt.finish_and_recall_on_submit(encoder);
        #[cfg(target_arch = "wasm32")]
        let _ = encoder;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuRasterMetrics {
    pub submissions: u64,
    pub dabs: u64,
    pub raster_candidate_pixels: u64,
    pub composited_pixels: u64,
    pub paint_pages: u64,
    pub preview_pages: u64,
    pub destination_companion_pages: u64,
    pub coverage_pages: u64,
    pub material_pages: u64,
    pub paint_storage_bytes: u64,
    pub preview_storage_bytes: u64,
    pub destination_storage_bytes: u64,
    pub paint_state_storage_bytes: u64,
    pub composite_storage_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GpuAdapterInfo {
    pub name: String,
    pub backend: u32,
    pub device_type: u32,
    pub vendor_id: u32,
    pub device_id: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GpuRasterError {
    AdapterUnavailable,
    HardwareAdapterRequired,
    DeviceRequest(String),
    InvalidExtent,
    ExtentUnsupported,
    InvalidImage,
    MissingBrushMask(AssetId),
    MissingPaintLayer(LayerId),
    UnsupportedBrushFeature(&'static str),
    InvalidDabRange,
    MultiplePreviewLayers,
    SizeOverflow,
    MapFailed(String),
    WaitFailed(String),
    Effect(String),
}

impl fmt::Display for GpuRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Effect(message) => write!(formatter, "effect shader: {message}"),
            Self::AdapterUnavailable => {
                formatter.write_str("no compatible wgpu adapter is available")
            }
            Self::HardwareAdapterRequired => {
                formatter.write_str("canvas painting requires a hardware GPU adapter")
            }
            Self::DeviceRequest(message) => {
                write!(formatter, "could not create wgpu device: {message}")
            }
            Self::InvalidExtent => formatter.write_str("canvas extent must be non-zero"),
            Self::ExtentUnsupported => {
                formatter.write_str("canvas extent exceeds the adapter limit")
            }
            Self::InvalidImage => formatter.write_str("invalid image data"),
            Self::MissingBrushMask(id) => write!(formatter, "brush mask is not prepared: {}", id.0),
            Self::MissingPaintLayer(id) => {
                write!(formatter, "paint layer is not available: {}", id.0)
            }
            Self::UnsupportedBrushFeature(feature) => {
                write!(
                    formatter,
                    "brush feature is not implemented by the GPU renderer: {feature}"
                )
            }
            Self::InvalidDabRange => formatter.write_str("dab batch references an invalid range"),
            Self::MultiplePreviewLayers => {
                formatter.write_str("one frame cannot preview strokes on multiple layers")
            }
            Self::SizeOverflow => formatter.write_str("GPU buffer size overflow"),
            Self::MapFailed(message) => write!(formatter, "GPU readback mapping failed: {message}"),
            Self::WaitFailed(message) => write!(formatter, "GPU completion wait failed: {message}"),
        }
    }
}

impl std::error::Error for GpuRasterError {}

struct PaintLayer {
    id: LayerId,
    pages: Vec<LayerPage>,
    coverage_pages: Vec<StrokeCoveragePage>,
    material_pages: Vec<CanvasMaterialPage>,
    watercolor_wetness_pages: Vec<WatercolorWetnessPage>,
    watercolor: Option<WatercolorLayerStyle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WatercolorLayerStyle {
    wet_edge: f32,
    burnt_edge: f32,
    edge_width: f32,
}

impl WatercolorLayerStyle {
    fn from_dab_style(style: &layer_render::DabStyle) -> Self {
        Self {
            wet_edge: style.rendering.wet_edge,
            burnt_edge: style.rendering.burnt_edge,
            edge_width: style.rendering.edge_width,
        }
    }

    fn radius(self) -> u32 {
        (self.edge_width.clamp(1.0, 16.0) * 2.0).ceil() as u32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterialOperation {
    Deposit = 0,
    Coverage = 1,
    Liquify = 2,
    Smudge = 3,
    Wet = 4,
    Watercolor = 5,
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectPipelineKind {
    AnalyticPaint,
    AnalyticErase,
    MaskPaint,
    MaskErase,
    TexturedPaint,
    TexturedErase,
}

impl DirectPipelineKind {
    const COUNT: usize = 6;

    fn is_textured(self) -> bool {
        matches!(self, Self::TexturedPaint | Self::TexturedErase)
    }
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterialPipelineKind {
    Color,
    Coverage,
    Wetness,
    State,
    Watercolor,
}

impl MaterialPipelineKind {
    const COUNT: usize = 5;

    fn for_attachments(watercolor: bool, coverage: bool, wetness: bool) -> Self {
        if watercolor {
            Self::Watercolor
        } else {
            match (coverage, wetness) {
                (false, false) => Self::Color,
                (true, false) => Self::Coverage,
                (false, true) => Self::Wetness,
                (true, true) => Self::State,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrushEncodingTarget {
    Persistent,
    Preview { from_persistent: bool },
}

impl BrushEncodingTarget {
    fn is_preview(self) -> bool {
        matches!(self, Self::Preview { .. })
    }
}

#[derive(Clone, Copy)]
struct BrushEncodingContext<'a> {
    batches: &'a [DabBatch],
    dabs: &'a [Dab],
    document_extent: [u32; 2],
    target: BrushEncodingTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrushStateTargets {
    coverage: bool,
    canvas_wetness: bool,
    watercolor_wetness: bool,
}

/// Renderer-private compilation of a brush style into the passes and sparse
/// state it needs. Keeping this decision in one place prevents allocation,
/// preview, and encoding paths from drifting apart as brush features grow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrushPassPlan {
    direct: Option<DirectPipelineKind>,
    material: MaterialOperation,
    state: BrushStateTargets,
    reservoir: bool,
    stroke_edge: bool,
}

impl BrushPassPlan {
    fn for_style(style: &layer_render::DabStyle) -> Self {
        let state = BrushStateTargets {
            coverage: style.rendering.accumulation == BrushAccumulation::Uniform
                || style.rendering.edge_after_stroke,
            canvas_wetness: style.wet_mix.wetness > 0.0,
            watercolor_wetness: style.execution == BrushExecution::Watercolor,
        };
        let needs_destination = style.execution != BrushExecution::Dry
            || style.alpha_locked
            || style.rendering.blend_mode != BrushBlendMode::Normal
            || state.coverage;
        let textured = style.grain.is_some()
            || style.dual.is_some()
            || style.rendering.alpha_threshold > 0.0
            || style.rendering.wet_edge > 0.0
            || style.rendering.burnt_edge > 0.0;
        let material = match style.execution {
            BrushExecution::Liquify => MaterialOperation::Liquify,
            BrushExecution::Smudge => MaterialOperation::Smudge,
            BrushExecution::Wet => MaterialOperation::Wet,
            BrushExecution::Watercolor => MaterialOperation::Watercolor,
            BrushExecution::Dry if state.coverage => MaterialOperation::Coverage,
            BrushExecution::Dry => MaterialOperation::Deposit,
        };
        let direct = (!needs_destination).then_some(match (textured, &style.tip, style.mode) {
            (true, _, DabMode::Paint) => DirectPipelineKind::TexturedPaint,
            (true, _, DabMode::Erase) => DirectPipelineKind::TexturedErase,
            (false, BrushTip::AnalyticEllipse, DabMode::Paint) => DirectPipelineKind::AnalyticPaint,
            (false, BrushTip::AnalyticEllipse, DabMode::Erase) => DirectPipelineKind::AnalyticErase,
            (false, BrushTip::Mask(_), DabMode::Paint) => DirectPipelineKind::MaskPaint,
            (false, BrushTip::Mask(_), DabMode::Erase) => DirectPipelineKind::MaskErase,
        });
        Self {
            direct,
            material,
            state,
            reservoir: style.execution == BrushExecution::Wet,
            stroke_edge: style.rendering.edge_after_stroke,
        }
    }

    fn requires_destination(self) -> bool {
        self.direct.is_none()
    }

    fn uses_texture_set(self) -> bool {
        self.requires_destination() || self.direct.is_some_and(DirectPipelineKind::is_textured)
    }

    fn uses_paint_state(self) -> bool {
        self.state.coverage || self.state.canvas_wetness || self.state.watercolor_wetness
    }
}

struct StrokeCoveragePage {
    coordinate: [u32; 2],
    primary: PageSurface,
    secondary: PageSurface,
    active_secondary: bool,
    owner: Option<StrokeId>,
    primary_needs_clear: bool,
    secondary_needs_clear: bool,
}

impl StrokeCoveragePage {
    fn active(&self) -> &PageSurface {
        if self.active_secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }
}

struct CanvasMaterialPage {
    coordinate: [u32; 2],
    wetness: PageSurface,
    needs_clear: bool,
}

/// Persistent watercolor wetness on one sparse layer page. The two R8
/// surfaces are one logical channel: deposition and capillary relaxation use
/// them as GPU-only ping-pong state.
struct WatercolorWetnessPage {
    coordinate: [u32; 2],
    primary: PageSurface,
    secondary: PageSurface,
    active_secondary: bool,
    primary_needs_clear: bool,
    secondary_needs_clear: bool,
}

impl WatercolorWetnessPage {
    fn surface(&self, secondary: bool) -> &PageSurface {
        if secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }

    fn active(&self) -> &PageSurface {
        if self.active_secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }

    fn inactive(&self) -> &PageSurface {
        if self.active_secondary {
            &self.primary
        } else {
            &self.secondary
        }
    }
}

struct BrushReservoir {
    primary: PageSurface,
    secondary: PageSurface,
    active_secondary: bool,
}

impl BrushReservoir {
    fn active(&self) -> &PageSurface {
        if self.active_secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }

    fn inactive(&self) -> &PageSurface {
        if self.active_secondary {
            &self.primary
        } else {
            &self.secondary
        }
    }
}

struct LayerPage {
    coordinate: [u32; 2],
    primary: PageSurface,
    secondary: Option<PageSurface>,
    active_secondary: bool,
    primary_needs_clear: bool,
    secondary_needs_clear: bool,
}

struct PageSurface {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    texture_bind_group: wgpu::BindGroup,
}

impl LayerPage {
    fn surface(&self, secondary: bool) -> &PageSurface {
        if secondary {
            self.secondary
                .as_ref()
                .expect("destination page has a secondary surface")
        } else {
            &self.primary
        }
    }

    fn active(&self) -> &PageSurface {
        self.surface(self.active_secondary)
    }
}

struct MaskAsset {
    id: AssetId,
    source: Vec<u8>,
    extent: [u32; 3],
    outline: std::sync::OnceLock<layer_render::TipOutline>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextureSetKey {
    primary: AssetId,
    grain: AssetId,
    dual: AssetId,
    dual_grain: AssetId,
    transport: AssetId,
}

struct TextureSet {
    key: TextureSetKey,
    bind_group: wgpu::BindGroup,
    transport_bind_group: wgpu::BindGroup,
}

struct Pipelines {
    direct: [wgpu::RenderPipeline; DirectPipelineKind::COUNT],
    material: [wgpu::RenderPipeline; MaterialPipelineKind::COUNT],
    watercolor_transport: [wgpu::RenderPipeline; WATERCOLOR_TRANSPORT_STEPS as usize],
    reservoir: wgpu::RenderPipeline,
    stroke_edge: wgpu::RenderPipeline,
    background: wgpu::RenderPipeline,
    background_empty: wgpu::BindGroup,
    composite: wgpu::RenderPipeline,
    watercolor_composite: wgpu::RenderPipeline,
    export: wgpu::RenderPipeline,
}

impl Pipelines {
    fn direct(&self, kind: DirectPipelineKind) -> &wgpu::RenderPipeline {
        &self.direct[kind as usize]
    }

    fn material(&self, watercolor: bool, coverage: bool, wetness: bool) -> &wgpu::RenderPipeline {
        &self.material
            [MaterialPipelineKind::for_attachments(watercolor, coverage, wetness) as usize]
    }
}

/// Headless-capable wgpu brush renderer. A platform presenter can sample the
/// same composite texture rather than requesting readback.
pub struct WgpuRasterizer {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_extent: [u32; 2],
    document_extent: [u32; 2],
    paint_layers: Vec<PaintLayer>,
    layer_masks: layer_masks::MaskRenderer,
    scene: Option<scene::Scene>,
    thumbnails: thumbnails::Thumbnails,
    canvas_preview: canvas_preview::CanvasOverview,
    color_sampler: color_sample::ColorSampler,
    composite_revision: u64,
    filter_previews: Option<scene::FilterPreviews>,
    effect_validation: Option<effect_validation::Pending>,
    validated_effects: Option<effects::Effects>,
    last_style_base: usize,
    filter_source_epoch: u64,
    images: std::collections::HashMap<AssetId, (wgpu::TextureView, [u32; 2])>,
    composite_texture: Option<wgpu::Texture>,
    composite_view: Option<wgpu::TextureView>,
    composite_bind_group: Option<wgpu::BindGroup>,
    preview_pages: Vec<LayerPage>,
    preview_coverage_pages: Vec<StrokeCoveragePage>,
    preview_watercolor_wetness_pages: Vec<WatercolorWetnessPage>,
    preview_damage: PixelRect,
    preview_layer_id: Option<LayerId>,
    preview_requires_base: bool,
    preview_direct_to_composite: bool,
    masks: Vec<MaskAsset>,
    texture_sets: Vec<TextureSet>,
    sampler: wgpu::Sampler,
    brush_sampler: wgpu::Sampler,
    style_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    advanced_texture_layout: wgpu::BindGroupLayout,
    target_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    edge_layout: wgpu::BindGroupLayout,
    watercolor_layout: wgpu::BindGroupLayout,
    transport_layout: wgpu::BindGroupLayout,
    style_buffer: wgpu::Buffer,
    style_bind_group: wgpu::BindGroup,
    style_stride: u64,
    style_capacity: usize,
    style_upload: Vec<u8>,
    target_buffer: wgpu::Buffer,
    target_bind_group: wgpu::BindGroup,
    target_stride: u64,
    target_capacity: usize,
    target_upload: Vec<u8>,
    uploads: Uploads,
    _empty_texture: wgpu::Texture,
    empty_view: wgpu::TextureView,
    _empty_scalar_texture: wgpu::Texture,
    empty_scalar_view: wgpu::TextureView,
    reservoir: BrushReservoir,
    dab_buffer: wgpu::Buffer,
    dab_capacity_bytes: u64,
    pipelines: Pipelines,
    last_submission: Option<wgpu::SubmissionIndex>,
    pending_readback: Option<ReadbackImage>,
    inspection: Option<(layer_render::ViewState, Vec<Layer>, f32)>,
    metrics: GpuRasterMetrics,
    telemetry: telemetry::Telemetry,
}

impl WgpuRasterizer {
    /// Borrowed device/queue for platform surface setup on the same GPU.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn new() -> Result<Self, GpuRasterError> {
        pollster::block_on(Self::new_async())
    }

    pub async fn new_async() -> Result<Self, GpuRasterError> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_descriptor);
        #[cfg(not(target_arch = "wasm32"))]
        let indexed_adapter = std::env::var("LAYER_GPU_INDEX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
        #[cfg(target_arch = "wasm32")]
        let indexed_adapter = None;
        let adapter = if let Some(index) = indexed_adapter {
            instance
                .enumerate_adapters(wgpu::Backends::PRIMARY)
                .await
                .into_iter()
                .nth(index)
                .ok_or(GpuRasterError::AdapterUnavailable)?
        } else {
            match wgpu::util::initialize_adapter_from_env(&instance, None).await {
                Ok(adapter) => adapter,
                Err(_) => instance
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        compatible_surface: None,
                        force_fallback_adapter: false,
                        apply_limit_buckets: false,
                    })
                    .await
                    .map_err(|_| GpuRasterError::AdapterUnavailable)?,
            }
        };
        let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("layer canvas device"),
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                required_limits: limits,
                ..Default::default()
            })
            .await
            .map_err(|error| GpuRasterError::DeviceRequest(error.to_string()))?;

        Self::from_wgpu(adapter, device, queue)
    }

    /// Builds the canvas engine on a platform-selected adapter/device. Native
    /// presenters use this after selecting an adapter compatible with their
    /// surface, so the canvas and presentation share one queue and resource set.
    pub fn from_wgpu(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<Self, GpuRasterError> {
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return Err(GpuRasterError::HardwareAdapterRequired);
        }
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
        let style_layout = create_style_layout(&device);
        let texture_layout = create_texture_layout(&device);
        let advanced_texture_layout = create_advanced_texture_layout(&device);
        let target_layout = create_target_layout(&device);
        let material_layout = create_material_layout(&device);
        let edge_layout = create_edge_layout(&device);
        let watercolor_layout = create_color_neighborhood_layout(&device);
        let transport_layout = create_transport_layout(&device);
        let style_stride = device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(mem::size_of::<StyleGpu>() as u32) as u64;
        let style_capacity = INITIAL_STYLE_RECORDS;
        let style_buffer = create_style_buffer(&device, style_stride, style_capacity);
        let style_bind_group = create_style_bind_group(&device, &style_layout, &style_buffer);
        let target_stride = device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(mem::size_of::<TargetGpu>() as u32) as u64;
        let target_capacity = INITIAL_TARGET_RECORDS;
        let target_buffer = create_target_buffer(&device, target_stride, target_capacity);
        let target_bind_group = create_target_bind_group(&device, &target_layout, &target_buffer);
        let (empty_texture, empty_view) =
            create_color_target(&device, [1, 1], "layer transparent missing page");
        let (empty_scalar_texture, empty_scalar_view) = create_target(
            &device,
            [1, 1],
            wgpu::TextureFormat::R8Unorm,
            "layer zero missing paint state",
        );
        let reservoir = BrushReservoir {
            primary: create_page_surface(
                &device,
                &texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                COLOR_FORMAT,
                "layer brush reservoir A",
            ),
            secondary: create_page_surface(
                &device,
                &texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                COLOR_FORMAT,
                "layer brush reservoir B",
            ),
            active_secondary: false,
        };
        let dab_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer dab instances"),
            size: INITIAL_DAB_BYTES,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipelines = create_pipelines(
            &device,
            PipelineLayouts {
                style: &style_layout,
                texture: &texture_layout,
                advanced_texture: &advanced_texture_layout,
                target: &target_layout,
                material: &material_layout,
                edge: &edge_layout,
                watercolor: &watercolor_layout,
                transport: &transport_layout,
            },
        );

        let uploads = Uploads::new(&device, 64 * 1024);
        let layer_masks =
            layer_masks::MaskRenderer::new(&device, &style_layout, &target_layout, &texture_layout);
        let telemetry = telemetry::Telemetry::new(&device, &queue);
        let mut renderer = Self {
            telemetry,
            adapter,
            device,
            queue,
            surface_extent: [0, 0],
            document_extent: [0, 0],
            layer_masks,
            scene: None,
            filter_previews: None,
            effect_validation: None,
            validated_effects: None,
            last_style_base: 0,
            filter_source_epoch: 0,
            canvas_preview: canvas_preview::CanvasOverview::new(),
            color_sampler: color_sample::ColorSampler::new(),
            composite_revision: 0,
            thumbnails: thumbnails::Thumbnails::new(),
            images: Default::default(),
            paint_layers: Vec::with_capacity(8),
            composite_texture: None,
            composite_view: None,
            composite_bind_group: None,
            preview_pages: Vec::with_capacity(16),
            preview_coverage_pages: Vec::with_capacity(8),
            preview_watercolor_wetness_pages: Vec::with_capacity(8),
            preview_damage: PixelRect::EMPTY,
            preview_layer_id: None,
            preview_requires_base: false,
            preview_direct_to_composite: false,
            masks: Vec::with_capacity(8),
            texture_sets: Vec::with_capacity(16),
            sampler,
            brush_sampler,
            style_layout,
            texture_layout,
            advanced_texture_layout,
            target_layout,
            material_layout,
            edge_layout,
            watercolor_layout,
            transport_layout,
            style_buffer,
            style_bind_group,
            style_stride,
            style_capacity,
            style_upload: Vec::with_capacity(style_stride as usize * style_capacity),
            target_buffer,
            target_bind_group,
            target_stride,
            target_capacity,
            target_upload: Vec::with_capacity(target_stride as usize * target_capacity),
            uploads,
            _empty_texture: empty_texture,
            empty_view,
            _empty_scalar_texture: empty_scalar_texture,
            empty_scalar_view,
            reservoir,
            dab_buffer,
            dab_capacity_bytes: INITIAL_DAB_BYTES,
            pipelines,
            last_submission: None,
            pending_readback: None,
            inspection: None,
            metrics: GpuRasterMetrics::default(),
        };
        renderer.install_builtin_masks()?;
        Ok(renderer)
    }

    /// The platform host validates replacement surfaces against this same GPU.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn adapter_info(&self) -> GpuAdapterInfo {
        let info = self.adapter.get_info();
        GpuAdapterInfo {
            name: info.name,
            backend: match info.backend {
                wgpu::Backend::Vulkan => 1,
                wgpu::Backend::Metal => 2,
                wgpu::Backend::Dx12 => 3,
                wgpu::Backend::Gl => 4,
                wgpu::Backend::BrowserWebGpu => 5,
                wgpu::Backend::Noop => 6,
            },
            device_type: match info.device_type {
                wgpu::DeviceType::IntegratedGpu => 1,
                wgpu::DeviceType::DiscreteGpu => 2,
                wgpu::DeviceType::VirtualGpu => 3,
                wgpu::DeviceType::Cpu => 4,
                wgpu::DeviceType::Other => 5,
            },
            vendor_id: info.vendor,
            device_id: info.device,
        }
    }

    pub fn metrics(&self) -> GpuRasterMetrics {
        self.metrics.clone()
    }

    pub fn document_extent(&self) -> [u32; 2] {
        self.document_extent
    }

    /// Benchmark/export synchronization only. Live drawing never calls this.
    pub fn wait_idle(&mut self) -> Result<(), GpuRasterError> {
        let Some(submission) = self.last_submission.take() else {
            return Ok(());
        };
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| GpuRasterError::WaitFailed(error.to_string()))?;
        Ok(())
    }

    pub fn copy_rgba8_srgb(
        &mut self,
        destination: &mut [u8],
        stride: usize,
    ) -> Result<(), GpuRasterError> {
        let srgb = self.readback_srgb_rgba8()?;
        let [width, height] = self.document_extent;
        let row_bytes = width as usize * 4;
        if stride < row_bytes || destination.len() < stride.saturating_mul(height as usize) {
            return Err(GpuRasterError::InvalidImage);
        }
        for y in 0..height as usize {
            let source = &srgb[y * row_bytes..(y + 1) * row_bytes];
            let target = &mut destination[y * stride..y * stride + row_bytes];
            target.copy_from_slice(source);
        }
        Ok(())
    }

    fn install_builtin_masks(&mut self) -> Result<(), GpuRasterError> {
        self.upload_mask(&AssetId::from(WHITE_MASK_ASSET), 1, 1, 1, &[255])?;
        for (id, bytes) in [
            (
                PENCIL_TEXTURE_ASSET,
                include_bytes!("../../../assets/brushes/pencil-grain.pgm").as_slice(),
            ),
            (
                PAINTBRUSH_TEXTURE_ASSET,
                include_bytes!("../../../assets/brushes/paint-bristles.pgm").as_slice(),
            ),
        ] {
            let (width, height, pixels) =
                parse_ascii_pgm(bytes).ok_or(GpuRasterError::InvalidImage)?;
            self.upload_mask(&AssetId::from(id), width, height, width, &pixels)?;
        }
        for (id, pixels) in [
            (PAPER_GRAIN_TEXTURE_ASSET, procedural_paper_grain()),
            (BRISTLE_GRAIN_TEXTURE_ASSET, procedural_bristle_grain()),
            (WATERCOLOR_TIP_TEXTURE_ASSET, procedural_watercolor_tip()),
            (
                WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET,
                procedural_transport_field(TransportFieldKind::LongNarrow),
            ),
            (
                WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
                procedural_transport_field(TransportFieldKind::LongBroad),
            ),
            (
                WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET,
                procedural_transport_field(TransportFieldKind::ShortNarrow),
            ),
            (
                WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
                procedural_transport_field(TransportFieldKind::ShortBroad),
            ),
        ] {
            self.upload_mask(
                &AssetId::from(id),
                PROCEDURAL_GRAIN_SIZE,
                PROCEDURAL_GRAIN_SIZE,
                PROCEDURAL_GRAIN_SIZE,
                &pixels,
            )?;
        }
        Ok(())
    }

    fn upload_mask(
        &mut self,
        id: &AssetId,
        width: u32,
        height: u32,
        stride: u32,
        pixels: &[u8],
    ) -> Result<(), GpuRasterError> {
        if width == 0
            || height == 0
            || stride < width
            || pixels.len() < stride as usize * height as usize
        {
            return Err(GpuRasterError::InvalidImage);
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer R8 brush tip"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = create_texture_bind_group(
            &self.device,
            &self.texture_layout,
            &view,
            &self.sampler,
            "layer brush tip binding",
        );
        let asset = MaskAsset {
            id: id.clone(),
            source: pixels.to_vec(),
            extent: [width, height, stride],
            outline: std::sync::OnceLock::new(),
            _texture: texture,
            view,
            bind_group,
        };
        if let Some(existing) = self.masks.iter_mut().find(|asset| asset.id == *id) {
            *existing = asset;
        } else {
            self.masks.push(asset);
        }
        // These bind groups retain views of the prior asset generation.
        self.texture_sets.clear();
        Ok(())
    }

    fn mask(&self, id: &AssetId) -> Result<&MaskAsset, GpuRasterError> {
        self.masks
            .iter()
            .find(|asset| asset.id == *id)
            .ok_or_else(|| GpuRasterError::MissingBrushMask(id.clone()))
    }

    fn texture_set_key(style: &layer_render::DabStyle) -> TextureSetKey {
        let white = AssetId::from(WHITE_MASK_ASSET);
        let primary = match &style.tip {
            BrushTip::AnalyticEllipse => white.clone(),
            BrushTip::Mask(id) => id.clone(),
        };
        let grain = style
            .grain
            .as_ref()
            .map(|grain| grain.asset.clone())
            .unwrap_or_else(|| white.clone());
        let (dual, dual_grain) = style
            .dual
            .as_ref()
            .map(|dual| {
                let tip = match &dual.tip {
                    BrushTip::AnalyticEllipse => white.clone(),
                    BrushTip::Mask(id) => id.clone(),
                };
                let grain = dual
                    .grain
                    .as_ref()
                    .map(|grain| grain.asset.clone())
                    .unwrap_or_else(|| white.clone());
                (tip, grain)
            })
            .unwrap_or_else(|| (white.clone(), white));
        TextureSetKey {
            primary,
            grain,
            dual,
            dual_grain,
            transport: style
                .transport
                .as_ref()
                .map(|transport| transport.conductance.clone())
                .unwrap_or_else(|| AssetId::from(WHITE_MASK_ASSET)),
        }
    }

    fn ensure_texture_set(&mut self, key: TextureSetKey) -> Result<(), GpuRasterError> {
        if self.texture_sets.iter().any(|set| set.key == key) {
            return Ok(());
        }
        let primary = &self.mask(&key.primary)?.view;
        let grain = &self.mask(&key.grain)?.view;
        let dual = &self.mask(&key.dual)?.view;
        let dual_grain = &self.mask(&key.dual_grain)?.view;
        let transport = &self.mask(&key.transport)?.view;
        let bind_group = create_advanced_texture_bind_group(
            &self.device,
            &self.advanced_texture_layout,
            [primary, grain, dual, dual_grain, transport],
            &self.brush_sampler,
        );
        let transport_bind_group = create_texture_bind_group(
            &self.device,
            &self.texture_layout,
            transport,
            &self.brush_sampler,
            "layer conductance texture binding",
        );
        self.texture_sets.push(TextureSet {
            key,
            bind_group,
            transport_bind_group,
        });
        Ok(())
    }

    fn validate_and_prepare_brush_resources(
        &mut self,
        batches: &[DabBatch],
    ) -> Result<(), GpuRasterError> {
        for batch in batches {
            let plan = BrushPassPlan::for_style(&batch.style);
            if batch.style.execution == BrushExecution::Liquify
                && batch.style.deform.mode == LiquifyMode::Reconstruct
            {
                return Err(GpuRasterError::UnsupportedBrushFeature(
                    "liquify reconstruct snapshot",
                ));
            }
            if plan.uses_texture_set() {
                self.ensure_texture_set(Self::texture_set_key(&batch.style))?;
            } else if let BrushTip::Mask(id) = &batch.style.tip {
                self.mask(id)?;
            }
        }
        Ok(())
    }

    fn ensure_document(
        &mut self,
        extent: [u32; 2],
        layers: &[Layer],
    ) -> Result<bool, GpuRasterError> {
        if extent[0] == 0 || extent[1] == 0 {
            return Err(GpuRasterError::InvalidExtent);
        }
        let limit = self.device.limits().max_texture_dimension_2d;
        if extent[0] > limit || extent[1] > limit {
            return Err(GpuRasterError::ExtentUnsupported);
        }
        let resized = extent != self.document_extent;
        if resized {
            self.document_extent = extent;
            self.paint_layers.clear();
            self.preview_pages.clear();
            self.preview_coverage_pages.clear();
            self.preview_watercolor_wetness_pages.clear();
            self.update_target_records(extent)?;
            let (texture, view) = create_color_target(&self.device, extent, "layer composite");
            self.composite_bind_group = Some(create_texture_bind_group(
                &self.device,
                &self.texture_layout,
                &view,
                &self.sampler,
                "layer composite export binding",
            ));
            self.composite_texture = Some(texture);
            self.composite_view = Some(view);
            self.preview_damage = PixelRect::EMPTY;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_direct_to_composite = false;
        }

        self.paint_layers.retain(|stored| {
            layers
                .iter()
                .any(|layer| layer.id == stored.id && layer.kind == LayerKind::Paint)
        });
        for layer in layers.iter().filter(|layer| layer.kind == LayerKind::Paint) {
            if self.paint_layers.iter().all(|stored| stored.id != layer.id) {
                self.paint_layers.push(PaintLayer {
                    id: layer.id,
                    pages: Vec::with_capacity(8),
                    coverage_pages: Vec::with_capacity(4),
                    material_pages: Vec::with_capacity(4),
                    watercolor_wetness_pages: Vec::with_capacity(4),
                    watercolor: None,
                });
            }
        }
        Ok(resized)
    }

    fn update_target_records(&mut self, extent: [u32; 2]) -> Result<(), GpuRasterError> {
        let page_columns = extent[0].div_ceil(PAGE_SIZE);
        let page_rows = extent[1].div_ceil(PAGE_SIZE);
        let records = 1_usize
            .checked_add(page_columns as usize * page_rows as usize)
            .ok_or(GpuRasterError::SizeOverflow)?;
        if records > self.target_capacity {
            self.target_capacity = records.next_power_of_two();
            self.target_buffer =
                create_target_buffer(&self.device, self.target_stride, self.target_capacity);
            self.target_bind_group =
                create_target_bind_group(&self.device, &self.target_layout, &self.target_buffer);
        }
        let used = records * self.target_stride as usize;
        self.target_upload.clear();
        self.target_upload.resize(used, 0);
        let full = TargetGpu::new([0, 0], extent, extent);
        self.target_upload[..mem::size_of::<TargetGpu>()].copy_from_slice(target_bytes(&full));
        for y in 0..page_rows {
            for x in 0..page_columns {
                let coordinate = [x, y];
                let target = TargetGpu::new(
                    [x * PAGE_SIZE, y * PAGE_SIZE],
                    [PAGE_SIZE, PAGE_SIZE],
                    extent,
                );
                let index = self.target_index(coordinate);
                let offset = index * self.target_stride as usize;
                self.target_upload[offset..offset + mem::size_of::<TargetGpu>()]
                    .copy_from_slice(target_bytes(&target));
            }
        }
        self.queue
            .write_buffer(&self.target_buffer, 0, &self.target_upload);
        Ok(())
    }

    fn target_index(&self, coordinate: [u32; 2]) -> usize {
        let columns = self.document_extent[0].div_ceil(PAGE_SIZE) as usize;
        1 + coordinate[1] as usize * columns + coordinate[0] as usize
    }

    fn target_offset(&self, coordinate: [u32; 2]) -> u32 {
        (self.target_index(coordinate) as u64 * self.target_stride) as u32
    }

    fn create_page(&self, coordinate: [u32; 2], label: &'static str) -> LayerPage {
        let primary = self.create_page_surface(label);
        LayerPage {
            coordinate,
            primary,
            secondary: None,
            active_secondary: false,
            primary_needs_clear: true,
            secondary_needs_clear: false,
        }
    }

    fn create_page_surface(&self, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            COLOR_FORMAT,
            label,
        )
    }

    fn create_scalar_page_surface(&self, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            wgpu::TextureFormat::R8Unorm,
            label,
        )
    }

    fn ensure_destination_companions(&mut self, batches: &[DabBatch]) {
        let mut destination_pages = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Persistent
                && BrushPassPlan::for_style(&batch.style).requires_destination()
        }) {
            let damage = batch_pixel_rect(batch, self.document_extent);
            if !damage.is_empty() {
                destination_pages.extend(
                    page_coordinates(damage).map(|coordinate| (batch.layer_id, coordinate)),
                );
            }
        }
        for layer_index in 0..self.paint_layers.len() {
            let layer_id = self.paint_layers[layer_index].id;
            let missing = self.paint_layers[layer_index]
                .pages
                .iter()
                .enumerate()
                .filter(|(_, page)| {
                    page.secondary.is_none()
                        && destination_pages.contains(&(layer_id, page.coordinate))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            for page_index in missing {
                let secondary = self.create_page_surface("layer sparse destination companion");
                let page = &mut self.paint_layers[layer_index].pages[page_index];
                page.secondary = Some(secondary);
                page.secondary_needs_clear = true;
            }
        }
    }

    fn material_bind_group(
        &self,
        layer_id: LayerId,
        coordinate: [u32; 2],
        stroke_id: StrokeId,
        watercolor: bool,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let layer = self
            .paint_layers
            .iter()
            .find(|layer| layer.id == layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?;
        Ok(self.material_bind_group_for_pages(
            &layer.pages,
            Some(layer),
            None,
            watercolor.then(|| {
                &layer
                    .watercolor_wetness_pages
                    .iter()
                    .find(|page| page.coordinate == coordinate)
                    .expect("watercolor wetness page is prepared before binding")
                    .active()
                    .view
            }),
            coordinate,
            stroke_id,
        ))
    }

    fn material_bind_group_for_pages(
        &self,
        pages: &[LayerPage],
        state_layer: Option<&PaintLayer>,
        coverage_override: Option<&wgpu::TextureView>,
        auxiliary_override: Option<&wgpu::TextureView>,
        coordinate: [u32; 2],
        stroke_id: StrokeId,
    ) -> wgpu::BindGroup {
        let mut views = Vec::with_capacity(9);
        for offset_y in -1_i32..=1 {
            for offset_x in -1_i32..=1 {
                let neighbor_x = coordinate[0] as i32 + offset_x;
                let neighbor_y = coordinate[1] as i32 + offset_y;
                let view = if neighbor_x < 0 || neighbor_y < 0 {
                    &self.empty_view
                } else {
                    let coordinate = [neighbor_x as u32, neighbor_y as u32];
                    pages
                        .iter()
                        .find(|page| page.coordinate == coordinate)
                        .or_else(|| {
                            state_layer.and_then(|layer| {
                                layer
                                    .pages
                                    .iter()
                                    .find(|page| page.coordinate == coordinate)
                            })
                        })
                        .map(|page| &page.active().view)
                        .unwrap_or(&self.empty_view)
                };
                views.push(view);
            }
        }
        let coverage = coverage_override.unwrap_or_else(|| {
            state_layer
                .and_then(|layer| {
                    layer
                        .coverage_pages
                        .iter()
                        .find(|page| page.coordinate == coordinate && page.owner == Some(stroke_id))
                })
                .map(|page| &page.active().view)
                .unwrap_or(&self.empty_scalar_view)
        });
        create_material_bind_group(
            &self.device,
            &self.material_layout,
            &views,
            &self.dab_buffer,
            coverage,
            auxiliary_override.unwrap_or(&self.reservoir.active().view),
        )
    }

    fn ensure_preview_destination_companions(&mut self, batches: &[DabBatch]) {
        let mut coordinates = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Preview
                && BrushPassPlan::for_style(&batch.style).requires_destination()
        }) {
            let damage = batch_pixel_rect(batch, self.document_extent);
            if !damage.is_empty() {
                coordinates.extend(page_coordinates(damage));
            }
        }
        let missing = self
            .preview_pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.secondary.is_none() && coordinates.contains(&page.coordinate))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for page_index in missing {
            let secondary = self.create_page_surface("layer sparse preview companion");
            self.preview_pages[page_index].secondary = Some(secondary);
        }
    }

    fn ensure_persistent_pages(&mut self, batches: &[DabBatch]) -> Result<(), GpuRasterError> {
        for batch in batches
            .iter()
            .filter(|batch| batch.kind == DabBatchKind::Persistent && batch.dab_count != 0)
        {
            let damage = batch_pixel_rect(batch, self.document_extent);
            if damage.is_empty() {
                continue;
            }
            let layer_index = self
                .paint_layers
                .iter()
                .position(|layer| layer.id == batch.layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
            for coordinate in page_coordinates(damage) {
                if self.paint_layers[layer_index]
                    .pages
                    .iter()
                    .all(|page| page.coordinate != coordinate)
                {
                    let page = self.create_page(coordinate, "layer sparse paint page");
                    self.paint_layers[layer_index].pages.push(page);
                }
            }
        }
        Ok(())
    }

    fn ensure_paint_state_pages(&mut self, batches: &[DabBatch]) -> Result<(), GpuRasterError> {
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Persistent
                && batch.dab_count != 0
                && BrushPassPlan::for_style(&batch.style).uses_paint_state()
        }) {
            let plan = BrushPassPlan::for_style(&batch.style);
            let damage = batch_pixel_rect(batch, self.document_extent);
            if damage.is_empty() {
                continue;
            }
            let layer_index = self
                .paint_layers
                .iter()
                .position(|layer| layer.id == batch.layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
            for coordinate in page_coordinates(damage) {
                if plan.state.coverage
                    && self.paint_layers[layer_index]
                        .coverage_pages
                        .iter()
                        .all(|page| page.coordinate != coordinate)
                {
                    let primary = self.create_scalar_page_surface("layer stroke coverage page A");
                    let secondary = self.create_scalar_page_surface("layer stroke coverage page B");
                    self.paint_layers[layer_index]
                        .coverage_pages
                        .push(StrokeCoveragePage {
                            coordinate,
                            primary,
                            secondary,
                            active_secondary: false,
                            owner: None,
                            primary_needs_clear: true,
                            secondary_needs_clear: true,
                        });
                }
                if plan.state.canvas_wetness
                    && self.paint_layers[layer_index]
                        .material_pages
                        .iter()
                        .all(|page| page.coordinate != coordinate)
                {
                    let wetness =
                        self.create_scalar_page_surface("layer sparse canvas wetness page");
                    self.paint_layers[layer_index]
                        .material_pages
                        .push(CanvasMaterialPage {
                            coordinate,
                            wetness,
                            needs_clear: true,
                        });
                }
                if plan.state.watercolor_wetness
                    && self.paint_layers[layer_index]
                        .watercolor_wetness_pages
                        .iter()
                        .all(|page| page.coordinate != coordinate)
                {
                    let primary =
                        self.create_scalar_page_surface("layer sparse watercolor wetness page A");
                    let secondary =
                        self.create_scalar_page_surface("layer sparse watercolor wetness page B");
                    self.paint_layers[layer_index]
                        .watercolor_wetness_pages
                        .push(WatercolorWetnessPage {
                            coordinate,
                            primary,
                            secondary,
                            active_secondary: false,
                            primary_needs_clear: true,
                            secondary_needs_clear: true,
                        });
                }
            }
        }
        Ok(())
    }

    fn ensure_preview_pages(&mut self, damage: PixelRect) {
        if damage.is_empty() {
            return;
        }
        for coordinate in page_coordinates(damage) {
            if self
                .preview_pages
                .iter()
                .all(|page| page.coordinate != coordinate)
            {
                if let Some(page) = self
                    .preview_pages
                    .iter_mut()
                    .find(|page| page_rect(page.coordinate).intersect(damage).is_empty())
                {
                    // Old prediction pixels are never persistent input. Rebind
                    // an off-tail page to the new coordinate; the preview path
                    // overwrites its complete active scissor from persistent
                    // canvas state before composition reads it.
                    page.coordinate = coordinate;
                    page.active_secondary = false;
                } else {
                    let page = self.create_page(coordinate, "layer sparse preview page");
                    self.preview_pages.push(page);
                }
            }
        }
    }

    fn ensure_preview_coverage_pages(&mut self, batches: &[DabBatch]) {
        let mut coordinates = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Preview
                && BrushPassPlan::for_style(&batch.style).state.coverage
        }) {
            let damage = batch_pixel_rect(batch, self.document_extent);
            if !damage.is_empty() {
                coordinates.extend(page_coordinates(damage));
            }
        }
        coordinates.sort_unstable();
        coordinates.dedup();
        let missing = coordinates
            .iter()
            .copied()
            .filter(|coordinate| {
                self.preview_coverage_pages
                    .iter()
                    .all(|page| page.coordinate != *coordinate)
            })
            .collect::<Vec<_>>();
        let mut reusable = self
            .preview_coverage_pages
            .iter()
            .enumerate()
            .filter(|(_, page)| !coordinates.contains(&page.coordinate))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for coordinate in missing {
            if let Some(index) = reusable.pop() {
                self.preview_coverage_pages[index].coordinate = coordinate;
                self.preview_coverage_pages[index].owner = None;
                self.preview_coverage_pages[index].active_secondary = false;
            } else {
                let primary =
                    self.create_scalar_page_surface("layer preview stroke coverage page A");
                let secondary =
                    self.create_scalar_page_surface("layer preview stroke coverage page B");
                self.preview_coverage_pages.push(StrokeCoveragePage {
                    coordinate,
                    primary,
                    secondary,
                    active_secondary: false,
                    owner: None,
                    primary_needs_clear: false,
                    secondary_needs_clear: false,
                });
            }
        }
    }

    fn ensure_preview_watercolor_wetness_pages(&mut self, damage: PixelRect, enabled: bool) {
        if !enabled || damage.is_empty() {
            return;
        }
        let coordinates = page_coordinates(damage).collect::<Vec<_>>();
        let missing = coordinates
            .iter()
            .copied()
            .filter(|coordinate| {
                self.preview_watercolor_wetness_pages
                    .iter()
                    .all(|page| page.coordinate != *coordinate)
            })
            .collect::<Vec<_>>();
        let mut reusable = self
            .preview_watercolor_wetness_pages
            .iter()
            .enumerate()
            .filter(|(_, page)| !coordinates.contains(&page.coordinate))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for coordinate in missing {
            if let Some(index) = reusable.pop() {
                self.preview_watercolor_wetness_pages[index].coordinate = coordinate;
                self.preview_watercolor_wetness_pages[index].active_secondary = false;
            } else {
                let primary =
                    self.create_scalar_page_surface("layer preview watercolor wetness page A");
                let secondary =
                    self.create_scalar_page_surface("layer preview watercolor wetness page B");
                self.preview_watercolor_wetness_pages
                    .push(WatercolorWetnessPage {
                        coordinate,
                        primary,
                        secondary,
                        active_secondary: false,
                        primary_needs_clear: false,
                        secondary_needs_clear: false,
                    });
            }
        }
    }

    fn refresh_storage_metrics(&mut self) {
        let paint_pages = self
            .paint_layers
            .iter()
            .map(|layer| layer.pages.len() as u64)
            .sum::<u64>();
        let preview_pages = self.preview_pages.len() as u64;
        let preview_coverage_pages = self.preview_coverage_pages.len() as u64;
        let preview_watercolor_wetness_pages = self.preview_watercolor_wetness_pages.len() as u64;
        let destination_companion_pages = self
            .paint_layers
            .iter()
            .flat_map(|layer| &layer.pages)
            .filter(|page| page.secondary.is_some())
            .count() as u64
            + self
                .preview_pages
                .iter()
                .filter(|page| page.secondary.is_some())
                .count() as u64;
        let coverage_pages = self
            .paint_layers
            .iter()
            .map(|layer| layer.coverage_pages.len() as u64)
            .sum::<u64>();
        let material_pages = self
            .paint_layers
            .iter()
            .map(|layer| {
                layer.material_pages.len() as u64 + layer.watercolor_wetness_pages.len() as u64
            })
            .sum::<u64>();
        let material_surface_pages = self
            .paint_layers
            .iter()
            .map(|layer| {
                layer.material_pages.len() as u64 + layer.watercolor_wetness_pages.len() as u64 * 2
            })
            .sum::<u64>();
        self.metrics.paint_pages = paint_pages;
        self.metrics.preview_pages = preview_pages;
        self.metrics.destination_companion_pages = destination_companion_pages;
        self.metrics.coverage_pages = coverage_pages;
        self.metrics.material_pages = material_pages;
        self.metrics.paint_storage_bytes = paint_pages.saturating_mul(PAGE_BYTES);
        self.metrics.preview_storage_bytes = preview_pages
            .saturating_mul(PAGE_BYTES)
            .saturating_add(preview_coverage_pages.saturating_mul(SCALAR_PAGE_BYTES * 2))
            .saturating_add(preview_watercolor_wetness_pages.saturating_mul(SCALAR_PAGE_BYTES * 2));
        self.metrics.destination_storage_bytes =
            destination_companion_pages.saturating_mul(PAGE_BYTES);
        self.metrics.paint_state_storage_bytes = coverage_pages
            .saturating_mul(SCALAR_PAGE_BYTES * 2)
            .saturating_add(material_surface_pages.saturating_mul(SCALAR_PAGE_BYTES))
            .saturating_add(RESERVOIR_BYTES);
        self.metrics.composite_storage_bytes =
            self.document_extent[0] as u64 * self.document_extent[1] as u64 * 4;
    }

    fn ensure_upload_capacity(&mut self, dabs: usize, styles: usize) -> Result<(), GpuRasterError> {
        let dab_bytes = (dabs as u64)
            .checked_mul(mem::size_of::<Dab>() as u64)
            .ok_or(GpuRasterError::SizeOverflow)?;
        if dab_bytes > self.dab_capacity_bytes {
            self.dab_capacity_bytes = dab_bytes.next_power_of_two();
            self.dab_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("layer dab instances"),
                size: self.dab_capacity_bytes,
                usage: wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if styles > self.style_capacity {
            self.style_capacity = styles.next_power_of_two();
            self.style_buffer =
                create_style_buffer(&self.device, self.style_stride, self.style_capacity);
            self.style_bind_group =
                create_style_bind_group(&self.device, &self.style_layout, &self.style_buffer);
            self.style_upload.reserve(
                self.style_stride as usize * self.style_capacity - self.style_upload.capacity(),
            );
        }
        Ok(())
    }

    fn prepare_uploads(
        &mut self,
        packet: FramePacket<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<usize, GpuRasterError> {
        let background_index = packet.dab_batches.len() + packet.layers.len();
        self.ensure_upload_capacity(packet.dabs.len(), background_index + 1)?;
        if !packet.dabs.is_empty() {
            self.uploads.write(
                encoder,
                &self.queue,
                &self.dab_buffer,
                dab_bytes(packet.dabs),
            );
        }
        let used = self.style_stride as usize * (background_index + 1);
        self.style_upload.clear();
        self.style_upload.resize(used, 0);
        for index in 0..packet.dab_batches.len() {
            let record = StyleGpu::brush(packet.document_extent, &packet.dab_batches[index]);
            let offset = index * self.style_stride as usize;
            self.style_upload[offset..offset + mem::size_of::<StyleGpu>()]
                .copy_from_slice(style_bytes(&record));
        }
        for (index, layer) in packet.layers.iter().enumerate() {
            let watercolor = packet
                .dab_batches
                .iter()
                .rev()
                .find(|batch| {
                    batch.layer_id == layer.id
                        && batch.style.execution == BrushExecution::Watercolor
                })
                .map(|batch| WatercolorLayerStyle::from_dab_style(&batch.style))
                .or_else(|| {
                    self.paint_layers
                        .iter()
                        .find(|stored| stored.id == layer.id)
                        .and_then(|stored| stored.watercolor)
                });
            let mut record = StyleGpu::layer(
                packet.document_extent,
                if needs_scene(packet.layers) {
                    1.0
                } else {
                    layer.opacity
                },
                watercolor,
            );
            record.color[0] = f32::from(needs_scene(packet.layers));
            let offset = (packet.dab_batches.len() + index) * self.style_stride as usize;
            self.style_upload[offset..offset + mem::size_of::<StyleGpu>()]
                .copy_from_slice(style_bytes(&record));
        }
        let background = StyleGpu::plain(
            packet.document_extent,
            packet.view.background_rgba_linear,
            1.0,
        );
        let offset = background_index * self.style_stride as usize;
        self.style_upload[offset..offset + mem::size_of::<StyleGpu>()]
            .copy_from_slice(style_bytes(&background));
        self.uploads
            .write(encoder, &self.queue, &self.style_buffer, &self.style_upload);
        Ok(background_index)
    }

    fn update_watercolor_layer_styles(&mut self, batches: &[DabBatch]) -> PixelRect {
        let mut dirty = PixelRect::EMPTY;
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Persistent
                && batch.style.execution == BrushExecution::Watercolor
        }) {
            let next = WatercolorLayerStyle::from_dab_style(&batch.style);
            let Some(layer) = self
                .paint_layers
                .iter_mut()
                .find(|layer| layer.id == batch.layer_id)
            else {
                continue;
            };
            if layer.watercolor == Some(next) {
                continue;
            }
            let radius = layer
                .watercolor
                .map_or(next.radius(), |old| old.radius().max(next.radius()));
            for page in &layer.pages {
                dirty =
                    dirty.union(page_rect(page.coordinate).expand(radius, self.document_extent));
            }
            layer.watercolor = Some(next);
        }
        dirty
    }

    fn watercolor_neighborhood_bind_group(
        &self,
        layer: &PaintLayer,
        coordinate: [u32; 2],
        preview: bool,
    ) -> Option<wgpu::BindGroup> {
        let mut color_views = Vec::with_capacity(5);
        let mut wetness_views = Vec::with_capacity(9);
        let mut any_source = false;
        for [offset_x, offset_y] in [[0_i32, 0_i32], [-1, 0], [1, 0], [0, -1], [0, 1]] {
            let x = coordinate[0] as i32 + offset_x;
            let y = coordinate[1] as i32 + offset_y;
            let view = if x < 0 || y < 0 {
                &self.empty_view
            } else {
                let neighbor = [x as u32, y as u32];
                let preview_page = preview
                    .then(|| {
                        self.preview_pages.iter().find(|page| {
                            page.coordinate == neighbor
                                && !self
                                    .preview_damage
                                    .intersect(page_rect(neighbor))
                                    .is_empty()
                        })
                    })
                    .flatten();
                let page = preview_page
                    .or_else(|| layer.pages.iter().find(|page| page.coordinate == neighbor));
                if page.is_some() {
                    any_source = true;
                }
                page.map(|page| &page.active().view)
                    .unwrap_or(&self.empty_view)
            };
            color_views.push(view);
        }
        for offset_y in -1_i32..=1 {
            for offset_x in -1_i32..=1 {
                let x = coordinate[0] as i32 + offset_x;
                let y = coordinate[1] as i32 + offset_y;
                let view = if x < 0 || y < 0 {
                    &self.empty_scalar_view
                } else {
                    let neighbor = [x as u32, y as u32];
                    let preview_page = preview
                        .then(|| {
                            self.preview_watercolor_wetness_pages.iter().find(|page| {
                                page.coordinate == neighbor
                                    && !self
                                        .preview_damage
                                        .intersect(page_rect(neighbor))
                                        .is_empty()
                            })
                        })
                        .flatten();
                    let page = preview_page.or_else(|| {
                        layer
                            .watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == neighbor)
                    });
                    if page.is_some() {
                        any_source = true;
                    }
                    page.map(|page| &page.active().view)
                        .unwrap_or(&self.empty_scalar_view)
                };
                wetness_views.push(view);
            }
        }
        any_source.then(|| {
            create_watercolor_neighborhood_bind_group(
                &self.device,
                &self.watercolor_layout,
                &color_views,
                &wetness_views,
            )
        })
    }

    fn encode_clear(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        label: &'static str,
    ) {
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    fn encode_batch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        target: &wgpu::TextureView,
        scissor: PixelRect,
        target_offset: u32,
    ) -> Result<(), GpuRasterError> {
        let start = batch.first_dab as u64 * mem::size_of::<Dab>() as u64;
        let end = start
            .checked_add(batch.dab_count as u64 * mem::size_of::<Dab>() as u64)
            .ok_or(GpuRasterError::InvalidDabRange)?;
        if end > self.dab_capacity_bytes {
            return Err(GpuRasterError::InvalidDabRange);
        }
        let plan = BrushPassPlan::for_style(&batch.style);
        let direct = plan
            .direct
            .expect("destination brushes use the material encoder");
        let pipeline = self.pipelines.direct(direct);
        let mask = if direct.is_textured() {
            None
        } else {
            match &batch.style.tip {
                BrushTip::AnalyticEllipse => None,
                BrushTip::Mask(id) => Some(self.mask(id)?),
            }
        };
        let texture_set = if direct.is_textured() {
            let key = Self::texture_set_key(&batch.style);
            Some(
                self.texture_sets
                    .iter()
                    .find(|set| set.key == key)
                    .expect("advanced brush resources are prepared before encoding"),
            )
        } else {
            None
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("layer raster brush batch"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_scissor_rect(
            scissor.min_x,
            scissor.min_y,
            scissor.width(),
            scissor.height(),
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(
            0,
            &self.style_bind_group,
            &[batch_index as u32 * self.style_stride as u32],
        );
        pass.set_bind_group(1, &self.target_bind_group, &[target_offset]);
        if let Some(mask) = mask {
            pass.set_bind_group(2, &mask.bind_group, &[]);
        } else if let Some(set) = texture_set {
            pass.set_bind_group(2, &set.bind_group, &[]);
        }
        pass.set_vertex_buffer(0, self.dab_buffer.slice(start..end));
        pass.draw(0..4, 0..batch.dab_count);
        Ok(())
    }

    fn encode_brush_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        context: BrushEncodingContext<'_>,
    ) -> Result<(), GpuRasterError> {
        let damage = batch_pixel_rect(batch, context.document_extent);
        if batch.dab_count == 0 || damage.is_empty() {
            return Ok(());
        }

        let plan = BrushPassPlan::for_style(&batch.style);
        if plan.requires_destination() {
            let preview = context.target.is_preview();
            let watercolor_first = plan.state.watercolor_wetness
                && is_first_watercolor_update_batch(context.batches, batch_index);
            let watercolor_last = plan.state.watercolor_wetness
                && is_last_watercolor_update_batch(context.batches, batch_index);
            let watercolor_damages = (watercolor_first || watercolor_last).then(|| {
                watercolor_update_damages(context.batches, batch_index, context.document_extent)
            });
            if watercolor_first {
                self.begin_watercolor_wetness_update(
                    encoder,
                    batch.layer_id,
                    watercolor_damages
                        .as_deref()
                        .expect("first watercolor update has damage"),
                    preview,
                )?;
            }
            match context.target {
                BrushEncodingTarget::Persistent => {
                    let start = batch.first_dab as usize;
                    let end = start + batch.dab_count as usize;
                    self.encode_material_batch(
                        encoder,
                        batch_index,
                        batch,
                        damage,
                        &context.dabs[start..end],
                    )?;
                }
                BrushEncodingTarget::Preview {
                    from_persistent: true,
                } => self.encode_preview_material_from_persistent(
                    encoder,
                    batch_index,
                    batch,
                    damage,
                )?,
                BrushEncodingTarget::Preview {
                    from_persistent: false,
                } => self.encode_preview_material_batch(encoder, batch_index, batch, damage)?,
            }
            if watercolor_last {
                let damages = watercolor_damages
                    .as_deref()
                    .expect("last watercolor update has damage");
                self.finish_watercolor_wetness_update(batch.layer_id, damages, preview)?;
                self.encode_watercolor_transport(encoder, batch_index, batch, damages, preview)?;
            }
            return Ok(());
        }

        let pages = match context.target {
            BrushEncodingTarget::Persistent => {
                &self
                    .paint_layers
                    .iter()
                    .find(|layer| layer.id == batch.layer_id)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?
                    .pages
            }
            BrushEncodingTarget::Preview { .. } => &self.preview_pages,
        };
        for coordinate in page_coordinates(damage) {
            let page = pages
                .iter()
                .find(|page| page.coordinate == coordinate)
                .expect("brush pages are prepared before encoding");
            let local = damage
                .intersect(page_rect(coordinate))
                .page_local(coordinate);
            if !local.is_empty() {
                self.encode_batch(
                    encoder,
                    batch_index,
                    batch,
                    &page.active().view,
                    local,
                    self.target_offset(coordinate),
                )?;
            }
        }
        Ok(())
    }

    fn begin_watercolor_wetness_update(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        layer_id: LayerId,
        damages: &[PixelRect],
        preview: bool,
    ) -> Result<(), GpuRasterError> {
        let persistent_pages;
        let pages = if preview {
            &self.preview_watercolor_wetness_pages
        } else {
            persistent_pages = &self
                .paint_layers
                .iter()
                .find(|layer| layer.id == layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?
                .watercolor_wetness_pages;
            persistent_pages
        };
        for coordinate in unique_page_coordinates(damages) {
            let page = pages
                .iter()
                .find(|page| page.coordinate == coordinate)
                .expect("watercolor update page is prepared before snapshot");
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &page.active().texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &page.inactive().texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: PAGE_SIZE,
                    height: PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }

    fn finish_watercolor_wetness_update(
        &mut self,
        layer_id: LayerId,
        damages: &[PixelRect],
        preview: bool,
    ) -> Result<(), GpuRasterError> {
        let persistent_pages;
        let pages = if preview {
            &mut self.preview_watercolor_wetness_pages
        } else {
            persistent_pages = &mut self
                .paint_layers
                .iter_mut()
                .find(|layer| layer.id == layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?
                .watercolor_wetness_pages;
            persistent_pages
        };
        for coordinate in unique_page_coordinates(damages) {
            let page = pages
                .iter_mut()
                .find(|page| page.coordinate == coordinate)
                .expect("watercolor update page remains live at commit");
            page.active_secondary = !page.active_secondary;
        }
        Ok(())
    }

    fn encode_material_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        damage: PixelRect,
        batch_dabs: &[Dab],
    ) -> Result<(), GpuRasterError> {
        struct Job {
            coordinate: [u32; 2],
            source_secondary: bool,
            destination_secondary: bool,
            coverage_source_secondary: Option<bool>,
            coverage_destination_secondary: Option<bool>,
            has_scalar_state: bool,
            source_bind_group: wgpu::BindGroup,
        }

        let plan = BrushPassPlan::for_style(&batch.style);
        let layer_index = self
            .paint_layers
            .iter()
            .position(|layer| layer.id == batch.layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
        if batch.stroke_start
            && plan.reservoir
            && let Some(first) = batch_dabs.first()
        {
            let amount = batch.style.wet_mix.amount_of_paint * first.material[2];
            let color = first.color_rgba_linear;
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer initialize brush reservoir"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.reservoir.active().view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color[0] as f64,
                            g: color[1] as f64,
                            b: color[2] as f64,
                            a: amount as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        if plan.state.coverage {
            for coordinate in page_coordinates(damage) {
                let page = self.paint_layers[layer_index]
                    .coverage_pages
                    .iter_mut()
                    .find(|page| page.coordinate == coordinate)
                    .expect("stroke coverage page is prepared before encoding");
                if page.owner != Some(batch.stroke_id) {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("layer reset stroke coverage page"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &page.active().view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    page.owner = Some(batch.stroke_id);
                }
            }
        }

        let mut jobs = Vec::new();
        for coordinate in page_coordinates(damage) {
            let page = self.paint_layers[layer_index]
                .pages
                .iter()
                .find(|page| page.coordinate == coordinate)
                .expect("destination page is prepared before encoding");
            let coverage = self.paint_layers[layer_index]
                .coverage_pages
                .iter()
                .find(|page| page.coordinate == coordinate && page.owner == Some(batch.stroke_id));
            jobs.push(Job {
                coordinate,
                source_secondary: page.active_secondary,
                destination_secondary: !page.active_secondary,
                coverage_source_secondary: coverage.map(|page| page.active_secondary),
                coverage_destination_secondary: coverage.map(|page| !page.active_secondary),
                has_scalar_state: if plan.state.watercolor_wetness {
                    self.paint_layers[layer_index]
                        .watercolor_wetness_pages
                        .iter()
                        .any(|page| page.coordinate == coordinate)
                } else {
                    self.paint_layers[layer_index]
                        .material_pages
                        .iter()
                        .any(|page| page.coordinate == coordinate)
                },
                source_bind_group: self.material_bind_group(
                    batch.layer_id,
                    coordinate,
                    batch.stroke_id,
                    plan.state.watercolor_wetness,
                )?,
            });
        }
        // Reservoir exchange samples the immutable pre-batch canvas. Keep a
        // bind group to that generation before the page ping-pong state flips.
        let reservoir_exchange = if plan.reservoir {
            batch_dabs
                .last()
                .map(|last| {
                    let coordinate = [
                        (last.center.x.max(0.0) as u32 / PAGE_SIZE)
                            .min(self.document_extent[0].saturating_sub(1) / PAGE_SIZE),
                        (last.center.y.max(0.0) as u32 / PAGE_SIZE)
                            .min(self.document_extent[1].saturating_sub(1) / PAGE_SIZE),
                    ];
                    self.material_bind_group(batch.layer_id, coordinate, batch.stroke_id, false)
                        .map(|bind_group| (coordinate, bind_group))
                })
                .transpose()?
        } else {
            None
        };

        let texture_key = Self::texture_set_key(&batch.style);
        for job in &jobs {
            let page = self.paint_layers[layer_index]
                .pages
                .iter()
                .find(|page| page.coordinate == job.coordinate)
                .expect("destination page remains live while encoding");
            let source = page.surface(job.source_secondary);
            let destination = page.surface(job.destination_secondary);
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &source.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &destination.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: PAGE_SIZE,
                    height: PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            if let (Some(source_secondary), Some(destination_secondary)) = (
                job.coverage_source_secondary,
                job.coverage_destination_secondary,
            ) {
                let coverage = self.paint_layers[layer_index]
                    .coverage_pages
                    .iter()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("stroke coverage page remains live while encoding");
                let source = if source_secondary {
                    &coverage.secondary
                } else {
                    &coverage.primary
                };
                let destination = if destination_secondary {
                    &coverage.secondary
                } else {
                    &coverage.primary
                };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &source.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &destination.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: PAGE_SIZE,
                        height: PAGE_SIZE,
                        depth_or_array_layers: 1,
                    },
                );
            }
            // Initialize the inactive wetness target once for the submitted
            // update. Every internal deposition microbatch accumulates into it
            // with fixed-function MAX blending before transport begins.
            let local = damage
                .intersect(page_rect(job.coordinate))
                .page_local(job.coordinate);
            if local.is_empty() {
                continue;
            }
            let texture_set = self
                .texture_sets
                .iter()
                .find(|set| set.key == texture_key)
                .expect("material brush textures are prepared before encoding");
            let coverage_view = job.coverage_destination_secondary.map(|secondary| {
                let coverage = self.paint_layers[layer_index]
                    .coverage_pages
                    .iter()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("stroke coverage page remains live while encoding");
                if secondary {
                    &coverage.secondary.view
                } else {
                    &coverage.primary.view
                }
            });
            let scalar_state_view = job.has_scalar_state.then(|| {
                if plan.state.watercolor_wetness {
                    let page = self.paint_layers[layer_index]
                        .watercolor_wetness_pages
                        .iter()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("watercolor wetness page remains live while encoding");
                    &page.inactive().view
                } else {
                    &self.paint_layers[layer_index]
                        .material_pages
                        .iter()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("canvas material page remains live while encoding")
                        .wetness
                        .view
                }
            });
            let color_attachments = [
                Some(wgpu::RenderPassColorAttachment {
                    view: &destination.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                coverage_view.map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                scalar_state_view.map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer destination brush page"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(local.min_x, local.min_y, local.width(), local.height());
            let pipeline = self.pipelines.material(
                plan.state.watercolor_wetness,
                coverage_view.is_some(),
                scalar_state_view.is_some(),
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                &self.target_bind_group,
                &[self.target_offset(job.coordinate)],
            );
            pass.set_bind_group(2, &job.source_bind_group, &[]);
            pass.set_bind_group(3, &texture_set.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        for job in jobs {
            let page = self.paint_layers[layer_index]
                .pages
                .iter_mut()
                .find(|page| page.coordinate == job.coordinate)
                .expect("destination page remains live after encoding");
            page.active_secondary = job.destination_secondary;
            if let Some(destination_secondary) = job.coverage_destination_secondary {
                self.paint_layers[layer_index]
                    .coverage_pages
                    .iter_mut()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("stroke coverage page remains live after encoding")
                    .active_secondary = destination_secondary;
            }
        }
        if let Some((coordinate, source_bind_group)) = reservoir_exchange {
            self.encode_reservoir_update(
                encoder,
                batch_index,
                batch,
                coordinate,
                &source_bind_group,
            )?;
        }
        Ok(())
    }

    fn transport_bind_group_for_pages(
        &self,
        pages: &[LayerPage],
        wetness_pages: &[WatercolorWetnessPage],
        coordinate: [u32; 2],
    ) -> wgpu::BindGroup {
        let mut colors = Vec::with_capacity(5);
        let mut wetness = Vec::with_capacity(5);
        for [offset_x, offset_y] in [[0_i32, 0_i32], [-1, 0], [1, 0], [0, -1], [0, 1]] {
            let x = coordinate[0] as i32 + offset_x;
            let y = coordinate[1] as i32 + offset_y;
            if x < 0 || y < 0 {
                colors.push(&self.empty_view);
                wetness.push(&self.empty_scalar_view);
                continue;
            }
            let neighbor = [x as u32, y as u32];
            colors.push(
                pages
                    .iter()
                    .find(|page| page.coordinate == neighbor)
                    .map(|page| &page.active().view)
                    .unwrap_or(&self.empty_view),
            );
            if let Some(page) = wetness_pages
                .iter()
                .find(|page| page.coordinate == neighbor)
            {
                wetness.push(&page.active().view);
            } else {
                wetness.push(&self.empty_scalar_view);
            }
        }
        create_transport_bind_group(&self.device, &self.transport_layout, &colors, &wetness)
    }

    fn encode_watercolor_transport(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        damages: &[PixelRect],
        preview: bool,
    ) -> Result<(), GpuRasterError> {
        let Some(transport) = &batch.style.transport else {
            return Ok(());
        };
        if transport.distance < 1.0 || (transport.wet_flow <= 0.0 && transport.dry_flow <= 0.0) {
            return Ok(());
        }

        struct Job {
            coordinate: [u32; 2],
            color_source_secondary: bool,
            color_destination_secondary: bool,
            wetness_source_secondary: bool,
            wetness_destination_secondary: bool,
            bind_group: wgpu::BindGroup,
        }

        let updated = unique_page_coordinates(damages);
        let texture_key = Self::texture_set_key(&batch.style);
        let transport_texture_bind_group = self
            .texture_sets
            .iter()
            .find(|set| set.key == texture_key)
            .expect("transport texture is prepared before encoding")
            .transport_bind_group
            .clone();

        for step in 0..WATERCOLOR_TRANSPORT_STEPS {
            let jobs = if preview {
                updated
                    .iter()
                    .map(|coordinate| {
                        let color = self
                            .preview_pages
                            .iter()
                            .find(|page| page.coordinate == *coordinate)
                            .expect("preview transport page is prepared before encoding");
                        let wetness = self
                            .preview_watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == *coordinate)
                            .expect("preview transport wetness is prepared before encoding");
                        Job {
                            coordinate: *coordinate,
                            color_source_secondary: color.active_secondary,
                            color_destination_secondary: !color.active_secondary,
                            wetness_source_secondary: wetness.active_secondary,
                            wetness_destination_secondary: !wetness.active_secondary,
                            bind_group: self.transport_bind_group_for_pages(
                                &self.preview_pages,
                                &self.preview_watercolor_wetness_pages,
                                *coordinate,
                            ),
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                let layer = self
                    .paint_layers
                    .iter()
                    .find(|layer| layer.id == batch.layer_id)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
                updated
                    .iter()
                    .map(|coordinate| {
                        let color = layer
                            .pages
                            .iter()
                            .find(|page| page.coordinate == *coordinate)
                            .expect("transport page is prepared before encoding");
                        let wetness = layer
                            .watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == *coordinate)
                            .expect("transport wetness is prepared before encoding");
                        Job {
                            coordinate: *coordinate,
                            color_source_secondary: color.active_secondary,
                            color_destination_secondary: !color.active_secondary,
                            wetness_source_secondary: wetness.active_secondary,
                            wetness_destination_secondary: !wetness.active_secondary,
                            bind_group: self.transport_bind_group_for_pages(
                                &layer.pages,
                                &layer.watercolor_wetness_pages,
                                *coordinate,
                            ),
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for job in &jobs {
                let (color_source, color_destination, wetness_source, wetness_destination) =
                    if preview {
                        let color = self
                            .preview_pages
                            .iter()
                            .find(|page| page.coordinate == job.coordinate)
                            .expect("preview transport page remains live while encoding");
                        let wetness = self
                            .preview_watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == job.coordinate)
                            .expect("preview transport wetness remains live while encoding");
                        (
                            color.surface(job.color_source_secondary),
                            color.surface(job.color_destination_secondary),
                            wetness.surface(job.wetness_source_secondary),
                            wetness.surface(job.wetness_destination_secondary),
                        )
                    } else {
                        let layer = self
                            .paint_layers
                            .iter()
                            .find(|layer| layer.id == batch.layer_id)
                            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
                        let color = layer
                            .pages
                            .iter()
                            .find(|page| page.coordinate == job.coordinate)
                            .expect("transport page remains live while encoding");
                        let wetness = layer
                            .watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == job.coordinate)
                            .expect("transport wetness remains live while encoding");
                        (
                            color.surface(job.color_source_secondary),
                            color.surface(job.color_destination_secondary),
                            wetness.surface(job.wetness_source_secondary),
                            wetness.surface(job.wetness_destination_secondary),
                        )
                    };
                // The first stage synchronizes both ping-pong generations.
                // Every later stage overwrites the full local scissor while
                // pixels outside it are already identical, so copying again
                // is redundant page traffic.
                if step == 0 {
                    for (source, destination) in [
                        (color_source, color_destination),
                        (wetness_source, wetness_destination),
                    ] {
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &source.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &destination.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: PAGE_SIZE,
                                height: PAGE_SIZE,
                                depth_or_array_layers: 1,
                            },
                        );
                    }
                }
                let page_damage = damages.iter().fold(PixelRect::EMPTY, |combined, damage| {
                    combined.union(damage.intersect(page_rect(job.coordinate)))
                });
                let local = page_damage.page_local(job.coordinate);
                if local.is_empty() {
                    continue;
                }
                let color_attachments = [
                    Some(wgpu::RenderPassColorAttachment {
                        view: &color_destination.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &wetness_destination.view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("layer nonlinear watercolor capillary relaxation"),
                    color_attachments: &color_attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_scissor_rect(local.min_x, local.min_y, local.width(), local.height());
                pass.set_pipeline(&self.pipelines.watercolor_transport[step as usize]);
                pass.set_bind_group(
                    0,
                    &self.style_bind_group,
                    &[batch_index as u32 * self.style_stride as u32],
                );
                pass.set_bind_group(
                    1,
                    &self.target_bind_group,
                    &[self.target_offset(job.coordinate)],
                );
                pass.set_bind_group(2, &job.bind_group, &[]);
                pass.set_bind_group(3, &transport_texture_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            if preview {
                for job in jobs {
                    self.preview_pages
                        .iter_mut()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("preview transport page remains live after encoding")
                        .active_secondary = job.color_destination_secondary;
                    self.preview_watercolor_wetness_pages
                        .iter_mut()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("preview transport wetness remains live after encoding")
                        .active_secondary = job.wetness_destination_secondary;
                }
            } else {
                let layer = self
                    .paint_layers
                    .iter_mut()
                    .find(|layer| layer.id == batch.layer_id)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
                for job in jobs {
                    layer
                        .pages
                        .iter_mut()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("transport page remains live after encoding")
                        .active_secondary = job.color_destination_secondary;
                    layer
                        .watercolor_wetness_pages
                        .iter_mut()
                        .find(|page| page.coordinate == job.coordinate)
                        .expect("transport wetness remains live after encoding")
                        .active_secondary = job.wetness_destination_secondary;
                }
            }
        }
        Ok(())
    }

    fn encode_reservoir_update(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        coordinate: [u32; 2],
        source_bind_group: &wgpu::BindGroup,
    ) -> Result<(), GpuRasterError> {
        let texture_key = Self::texture_set_key(&batch.style);
        let texture_set = self
            .texture_sets
            .iter()
            .find(|set| set.key == texture_key)
            .expect("reservoir brush textures are prepared before encoding");
        let target = &self.reservoir.inactive().view;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer brush reservoir exchange"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.reservoir);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                &self.target_bind_group,
                &[self.target_offset(coordinate)],
            );
            pass.set_bind_group(2, source_bind_group, &[]);
            pass.set_bind_group(3, &texture_set.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.reservoir.active_secondary = !self.reservoir.active_secondary;
        Ok(())
    }

    fn encode_stroke_edge(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
    ) -> Result<(), GpuRasterError> {
        struct Job {
            coordinate: [u32; 2],
            destination_secondary: bool,
            bind_group: wgpu::BindGroup,
        }

        let layer_index = self
            .paint_layers
            .iter()
            .position(|layer| layer.id == batch.layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
        let layer = &self.paint_layers[layer_index];
        let mut jobs = Vec::new();
        for coverage_page in layer
            .coverage_pages
            .iter()
            .filter(|page| page.owner == Some(batch.stroke_id))
        {
            let coordinate = coverage_page.coordinate;
            let color_page = layer
                .pages
                .iter()
                .find(|page| page.coordinate == coordinate)
                .expect("edge coverage always has a paint page");
            let mut coverage_views = Vec::with_capacity(9);
            for offset_y in -1_i32..=1 {
                for offset_x in -1_i32..=1 {
                    let x = coordinate[0] as i32 + offset_x;
                    let y = coordinate[1] as i32 + offset_y;
                    let view = if x < 0 || y < 0 {
                        &self.empty_scalar_view
                    } else {
                        layer
                            .coverage_pages
                            .iter()
                            .find(|page| {
                                page.coordinate == [x as u32, y as u32]
                                    && page.owner == Some(batch.stroke_id)
                            })
                            .map(|page| &page.active().view)
                            .unwrap_or(&self.empty_scalar_view)
                    };
                    coverage_views.push(view);
                }
            }
            jobs.push(Job {
                coordinate,
                destination_secondary: !color_page.active_secondary,
                bind_group: create_edge_bind_group(
                    &self.device,
                    &self.edge_layout,
                    &color_page.active().view,
                    &coverage_views,
                ),
            });
        }

        for job in &jobs {
            let page = self.paint_layers[layer_index]
                .pages
                .iter()
                .find(|page| page.coordinate == job.coordinate)
                .expect("edge paint page remains live while encoding");
            let destination = page.surface(job.destination_secondary);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer post-stroke edge page"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        // The full-page shader copies source color outside the
                        // edge band, so a preceding source-to-destination copy
                        // would be redundant.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.stroke_edge);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                &self.target_bind_group,
                &[self.target_offset(job.coordinate)],
            );
            pass.set_bind_group(2, &job.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        for job in jobs {
            self.paint_layers[layer_index]
                .pages
                .iter_mut()
                .find(|page| page.coordinate == job.coordinate)
                .expect("edge paint page remains live after encoding")
                .active_secondary = job.destination_secondary;
        }
        Ok(())
    }

    fn encode_preview_material_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        damage: PixelRect,
    ) -> Result<(), GpuRasterError> {
        struct Job {
            coordinate: [u32; 2],
            source_secondary: bool,
            destination_secondary: bool,
            coverage_source_secondary: Option<bool>,
            coverage_destination_secondary: Option<bool>,
            has_watercolor_wetness: bool,
            source_bind_group: wgpu::BindGroup,
        }

        let plan = BrushPassPlan::for_style(&batch.style);
        let mut jobs = Vec::new();
        for coordinate in page_coordinates(damage) {
            let page = self
                .preview_pages
                .iter()
                .find(|page| page.coordinate == coordinate)
                .expect("preview destination page is prepared before encoding");
            let coverage = plan.state.coverage.then(|| {
                self.preview_coverage_pages
                    .iter()
                    .find(|page| page.coordinate == coordinate)
                    .expect("preview coverage page is prepared before encoding")
            });
            jobs.push(Job {
                coordinate,
                source_secondary: page.active_secondary,
                destination_secondary: !page.active_secondary,
                coverage_source_secondary: coverage.map(|page| page.active_secondary),
                coverage_destination_secondary: coverage.map(|page| !page.active_secondary),
                has_watercolor_wetness: plan.state.watercolor_wetness,
                source_bind_group: self.material_bind_group_for_pages(
                    &self.preview_pages,
                    self.paint_layers
                        .iter()
                        .find(|layer| layer.id == batch.layer_id),
                    coverage.map(|page| &page.active().view),
                    plan.state.watercolor_wetness.then(|| {
                        &self
                            .preview_watercolor_wetness_pages
                            .iter()
                            .find(|page| page.coordinate == coordinate)
                            .expect("preview watercolor wetness page is prepared before binding")
                            .active()
                            .view
                    }),
                    coordinate,
                    batch.stroke_id,
                ),
            });
        }

        let texture_key = Self::texture_set_key(&batch.style);
        for job in &jobs {
            let page = self
                .preview_pages
                .iter()
                .find(|page| page.coordinate == job.coordinate)
                .expect("preview destination page remains live while encoding");
            let source = page.surface(job.source_secondary);
            let destination = page.surface(job.destination_secondary);
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &source.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &destination.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: PAGE_SIZE,
                    height: PAGE_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            if let (Some(source_secondary), Some(destination_secondary)) = (
                job.coverage_source_secondary,
                job.coverage_destination_secondary,
            ) {
                let coverage = self
                    .preview_coverage_pages
                    .iter()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("preview coverage page remains live while encoding");
                let source = if source_secondary {
                    &coverage.secondary
                } else {
                    &coverage.primary
                };
                let destination = if destination_secondary {
                    &coverage.secondary
                } else {
                    &coverage.primary
                };
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &source.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &destination.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: PAGE_SIZE,
                        height: PAGE_SIZE,
                        depth_or_array_layers: 1,
                    },
                );
            }
            // The preview update uses the same one-snapshot contract as
            // persistent watercolor. Initialization happens once before its
            // first microbatch.
            let local = damage
                .intersect(page_rect(job.coordinate))
                .page_local(job.coordinate);
            if local.is_empty() {
                continue;
            }
            let texture_set = self
                .texture_sets
                .iter()
                .find(|set| set.key == texture_key)
                .expect("preview material textures are prepared before encoding");
            let coverage_view = job.coverage_destination_secondary.map(|secondary| {
                let coverage = self
                    .preview_coverage_pages
                    .iter()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("preview coverage page remains live while encoding");
                if secondary {
                    &coverage.secondary.view
                } else {
                    &coverage.primary.view
                }
            });
            let watercolor_wetness_view = job.has_watercolor_wetness.then(|| {
                let page = self
                    .preview_watercolor_wetness_pages
                    .iter()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("preview watercolor wetness page remains live while encoding");
                &page.inactive().view
            });
            let color_attachments = [
                Some(wgpu::RenderPassColorAttachment {
                    view: &destination.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                coverage_view.map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
                watercolor_wetness_view.map(|view| wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                }),
            ];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer predicted destination brush page"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(local.min_x, local.min_y, local.width(), local.height());
            let pipeline = self.pipelines.material(
                plan.state.watercolor_wetness,
                coverage_view.is_some(),
                watercolor_wetness_view.is_some(),
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                &self.target_bind_group,
                &[self.target_offset(job.coordinate)],
            );
            pass.set_bind_group(2, &job.source_bind_group, &[]);
            pass.set_bind_group(3, &texture_set.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        for job in jobs {
            self.preview_pages
                .iter_mut()
                .find(|page| page.coordinate == job.coordinate)
                .expect("preview destination page remains live after encoding")
                .active_secondary = job.destination_secondary;
            if let Some(destination_secondary) = job.coverage_destination_secondary {
                self.preview_coverage_pages
                    .iter_mut()
                    .find(|page| page.coordinate == job.coordinate)
                    .expect("preview coverage page remains live after encoding")
                    .active_secondary = destination_secondary;
            }
        }
        Ok(())
    }

    /// The common predictive destination path has one batch. It can read the
    /// just-updated committed layer directly and fully write preview damage,
    /// avoiding both committed-to-preview and preview ping-pong copies.
    fn encode_preview_material_from_persistent(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        damage: PixelRect,
    ) -> Result<(), GpuRasterError> {
        struct Job {
            coordinate: [u32; 2],
            source_bind_group: wgpu::BindGroup,
        }

        let plan = BrushPassPlan::for_style(&batch.style);
        let jobs = page_coordinates(damage)
            .map(|coordinate| {
                Ok(Job {
                    coordinate,
                    source_bind_group: self.material_bind_group(
                        batch.layer_id,
                        coordinate,
                        batch.stroke_id,
                        plan.state.watercolor_wetness,
                    )?,
                })
            })
            .collect::<Result<Vec<_>, GpuRasterError>>()?;
        let texture_key = Self::texture_set_key(&batch.style);
        for job in jobs {
            let page = self
                .preview_pages
                .iter()
                .find(|page| page.coordinate == job.coordinate)
                .expect("preview page is prepared before encoding");
            let local = damage
                .intersect(page_rect(job.coordinate))
                .page_local(job.coordinate);
            if local.is_empty() {
                continue;
            }
            let texture_set = self
                .texture_sets
                .iter()
                .find(|set| set.key == texture_key)
                .expect("preview material textures are prepared before encoding");
            let color_attachments = [
                Some(wgpu::RenderPassColorAttachment {
                    view: &page.primary.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                }),
                None,
                None,
            ];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer direct predicted destination page"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(local.min_x, local.min_y, local.width(), local.height());
            pass.set_pipeline(self.pipelines.material(false, false, false));
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                &self.target_bind_group,
                &[self.target_offset(job.coordinate)],
            );
            pass.set_bind_group(2, &job.source_bind_group, &[]);
            pass.set_bind_group(3, &texture_set.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }

    fn readback_srgb_rgba8(&mut self) -> Result<Vec<u8>, GpuRasterError> {
        let [width, height] = self.document_extent;
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidExtent);
        }
        let row_bytes = width.checked_mul(4).ok_or(GpuRasterError::SizeOverflow)?;
        let padded_row_bytes = align_up(row_bytes, wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size = padded_row_bytes as u64 * height as u64;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer explicit readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let export_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer explicit sRGB export"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: EXPORT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let export_view = export_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer explicit readback encoder"),
            });
        // Inspection is a display aid, never exported. Recompose only on this
        // explicit export path, then restore the visible scene in GPU order.
        let inspection = self.inspection.take();
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
        }
        if let Some((view, layers, time)) = &inspection {
            let mut scene = self.scene.take().expect("inspection scene");
            scene.compose(
                self,
                FramePacket {
                    view: *view,
                    document_extent: [width, height],
                    layers,
                    dabs: &[],
                    dab_batches: &[],
                    reset_layers: false,
                    time_seconds: *time,
                    composite_all: true,
                },
                PixelRect::full([width, height]),
                &mut encoder,
                false,
            )?;
            self.scene = Some(scene);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer GPU sRGB export conversion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &export_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipelines.export);
            pass.set_bind_group(
                0,
                self.composite_bind_group
                    .as_ref()
                    .expect("document target exists"),
                &[],
            );
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &export_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if let Some((view, layers, time)) = &inspection {
            let mut scene = self.scene.take().expect("inspection scene");
            scene.compose(
                self,
                FramePacket {
                    view: *view,
                    document_extent: [width, height],
                    layers,
                    dabs: &[],
                    dab_batches: &[],
                    reset_layers: false,
                    time_seconds: *time,
                    composite_all: true,
                },
                PixelRect::full([width, height]),
                &mut encoder,
                true,
            )?;
            self.scene = Some(scene);
        }
        self.inspection = inspection;
        self.uploads.finish(&encoder);
        let submission = self.queue.submit([encoder.finish()]);
        let (sender, receiver) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result.map_err(|error| error.to_string()));
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| GpuRasterError::WaitFailed(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| GpuRasterError::MapFailed(error.to_string()))?
            .map_err(GpuRasterError::MapFailed)?;
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| GpuRasterError::MapFailed(error.to_string()))?;
        let mut pixels = vec![0; row_bytes as usize * height as usize];
        for y in 0..height as usize {
            let source = &mapped
                [y * padded_row_bytes as usize..y * padded_row_bytes as usize + row_bytes as usize];
            let target = &mut pixels[y * row_bytes as usize..(y + 1) * row_bytes as usize];
            target.copy_from_slice(source);
        }
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }
}

impl WgpuRasterizer {
    /// Source-asset cursor geometry for a host with a renderer on another thread.
    /// This never downloads canvas pixels.
    pub fn cursor_outlines(&self) -> std::collections::HashMap<AssetId, layer_render::TipOutline> {
        self.masks
            .iter()
            .map(|mask| (mask.id.clone(), self.tip_outline(&mask.id).unwrap().clone()))
            .collect()
    }
}

impl CanvasRenderer for WgpuRasterizer {
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        self.telemetry.enabled = enabled;
    }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        let mut t = self.telemetry.snapshot();
        let m = &self.metrics;
        t.submissions = m.submissions;
        t.dabs = m.dabs;
        t.dirty_pixels = m.composited_pixels;
        t.resident_bytes = m.paint_storage_bytes
            + m.preview_storage_bytes
            + m.destination_storage_bytes
            + m.paint_state_storage_bytes
            + m.composite_storage_bytes
            + self.canvas_preview.storage_bytes()
            + self.layer_masks.pages.len() as u64 * PAGE_SIZE as u64 * PAGE_SIZE as u64;
        if let Some(scene) = &self.scene {
            t.effect_passes = scene.effect_passes;
            t.compiled_effects = scene.effects.compilations;
            t.resident_bytes += scene.scratch_bytes();
        }
        if let Some(previews) = &self.filter_previews {
            t.resident_bytes += previews.storage_bytes();
        }
        t
    }
    fn request_thumbnail(&mut self, request_id: u64, target: LayerId) -> Result<(), Self::Error> {
        self.start_thumbnail(request_id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.thumbnails.take()
    }
    fn request_canvas_preview(&mut self, known_revision: Option<u64>) -> Result<bool, Self::Error> {
        self.start_canvas_preview(known_revision)
    }
    fn take_canvas_preview(&mut self) -> Option<Result<layer_render::CanvasPreview, Self::Error>> {
        self.canvas_preview.take()
    }
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        self.start_color_sample(request)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.color_sampler.take()
    }
    fn request_filter_previews(
        &mut self,
        request: layer_render::FilterPreviewRequest,
    ) -> Result<bool, Self::Error> {
        self.start_filter_previews(request)
    }
    fn request_effect_validation(
        &mut self,
        request: layer_render::EffectValidationRequest,
    ) -> Result<bool, Self::Error> {
        self.start_effect_validation(request)
    }
    fn take_effect_validation(&mut self) -> Option<layer_render::EffectValidationResult> {
        self.poll_effect_validation()
    }
    fn take_filter_previews(
        &mut self,
    ) -> Option<Result<layer_render::FilterPreviewImage, Self::Error>> {
        self.poll_filter_previews()
    }
    fn tip_outline(&self, asset: &AssetId) -> Option<&layer_render::TipOutline> {
        let mask = self.mask(asset).ok()?;
        Some(mask.outline.get_or_init(|| {
            let [width, height, stride] = mask.extent;
            layer_render::mask_outline(width, height, stride, &mask.source)
        }))
    }
    type Error = GpuRasterError;

    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidExtent);
        }
        self.surface_extent = [width, height];
        Ok(())
    }

    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        if image.format == PixelFormat::Rgba8Srgb {
            let limit = self.device.limits().max_texture_dimension_2d;
            if image.width == 0
                || image.height == 0
                || image.width > limit
                || image.height > limit
                || image.stride < image.width * 4
                || image.bytes.len() < image.stride as usize * image.height as usize
            {
                return Err(GpuRasterError::InvalidImage);
            }
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("immutable imported image"),
                size: wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: EXPORT_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                texture.as_image_copy(),
                image.bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.stride),
                    rows_per_image: Some(image.height),
                },
                texture.size(),
            );
            self.images.insert(
                asset.clone(),
                (
                    texture.create_view(&Default::default()),
                    [image.width, image.height],
                ),
            );
            return Ok(());
        }
        if image.format != PixelFormat::R8Unorm {
            return Err(GpuRasterError::InvalidImage);
        }
        self.upload_mask(asset, image.width, image.height, image.stride, image.bytes)
    }

    fn release_asset(&mut self, asset: &AssetId) {
        self.images.remove(asset);
        self.masks.retain(|stored| stored.id != *asset);
        self.texture_sets.clear();
    }

    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
        self.last_style_base = packet.dab_batches.len();
        if packet.reset_layers
            || !packet.dabs.is_empty()
            || packet
                .dab_batches
                .iter()
                .any(|b| b.kind != DabBatchKind::Preview)
        {
            self.filter_source_epoch = self.filter_source_epoch.wrapping_add(1);
        }
        let started = self.telemetry.enabled.then(web_time::Instant::now);
        if !needs_scene(packet.layers) {
            self.scene = None;
        }
        if let Some(scene) = &mut self.scene {
            scene.effect_passes = 0;
        }
        let mut view = packet.view;
        self.thumbnails.paper = packet
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Background)
            .map(|l| {
                let mut color = view.background_rgba_linear;
                color[3] *= l.opacity;
                (l.id, color)
            });
        if let Some(background) = packet
            .layers
            .iter()
            .find(|l| l.kind == LayerKind::Background)
        {
            view.background_rgba_linear[3] *= if background.visible {
                background.opacity
            } else {
                0.
            };
        }
        let packet = FramePacket { view, ..packet };
        self.inspection = packet
            .layers
            .iter()
            .any(|l| l.mask.as_ref().is_some_and(|m| m.enabled && m.show_area))
            .then(|| {
                (
                    packet.view,
                    packet
                        .layers
                        .iter()
                        .map(|l| {
                            let mut layer = l.clone();
                            layer.strokes.clear();
                            layer
                        })
                        .collect(),
                    packet.time_seconds,
                )
            });
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
            scene.style_base = packet.dab_batches.len();
        }
        let original_batches = packet.dab_batches;
        let filtered: std::borrow::Cow<'_, [DabBatch]> = if original_batches
            .iter()
            .any(|b| layer_masks::MaskRenderer::is_mask(packet.layers, b.layer_id))
        {
            std::borrow::Cow::Owned(
                original_batches
                    .iter()
                    .map(|b| {
                        let mut b = b.clone();
                        if layer_masks::MaskRenderer::is_mask(packet.layers, b.layer_id) {
                            b.dab_count = 0;
                        }
                        b
                    })
                    .collect(),
            )
        } else {
            std::borrow::Cow::Borrowed(original_batches)
        };
        let packet = FramePacket {
            dab_batches: &filtered,
            ..packet
        };
        self.validate_and_prepare_brush_resources(packet.dab_batches)?;
        let resized = self.ensure_document(packet.document_extent, packet.layers)?;
        let reset = packet.reset_layers || resized;
        if reset {
            for layer in &mut self.paint_layers {
                layer.pages.clear();
                layer.coverage_pages.clear();
                layer.material_pages.clear();
                layer.watercolor_wetness_pages.clear();
                layer.watercolor = None;
            }
            self.preview_pages.clear();
            self.preview_coverage_pages.clear();
            self.preview_watercolor_wetness_pages.clear();
            self.preview_damage = PixelRect::EMPTY;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_direct_to_composite = false;
        }
        self.ensure_persistent_pages(packet.dab_batches)?;
        self.ensure_destination_companions(packet.dab_batches);
        self.ensure_paint_state_pages(packet.dab_batches)?;

        let old_preview_damage = self.preview_damage;
        let watercolor_style_dirty = self.update_watercolor_layer_styles(packet.dab_batches);
        let mut dirty = old_preview_damage.union(watercolor_style_dirty);
        let mut new_preview_damage = PixelRect::EMPTY;
        let mut new_preview_layer = None;
        let mut new_preview_requires_base = false;
        let mut preview_is_watercolor = false;

        // Validate every range and accumulate metrics once before encoding.
        for batch in packet.dab_batches {
            let start = batch.first_dab as usize;
            let end = start
                .checked_add(batch.dab_count as usize)
                .ok_or(GpuRasterError::InvalidDabRange)?;
            let dabs = packet
                .dabs
                .get(start..end)
                .ok_or(GpuRasterError::InvalidDabRange)?;
            let batch_dirty = batch_pixel_rect(batch, packet.document_extent);
            let visual_dirty = if batch.style.execution == BrushExecution::Watercolor {
                batch_dirty.expand(
                    WatercolorLayerStyle::from_dab_style(&batch.style).radius(),
                    packet.document_extent,
                )
            } else {
                batch_dirty
            };
            if batch.stroke_end && batch.style.rendering.edge_after_stroke {
                let edge_dirty = self
                    .paint_layers
                    .iter()
                    .find(|layer| layer.id == batch.layer_id)
                    .map(|layer| {
                        layer
                            .coverage_pages
                            .iter()
                            .filter(|page| page.owner == Some(batch.stroke_id))
                            .fold(PixelRect::EMPTY, |damage, page| {
                                damage.union(
                                    page_rect(page.coordinate)
                                        .intersect(PixelRect::full(packet.document_extent)),
                                )
                            })
                    })
                    .unwrap_or(PixelRect::EMPTY);
                dirty = dirty.union(edge_dirty);
            }
            if batch_dirty.is_empty() || dabs.is_empty() {
                continue;
            }
            if batch.kind == DabBatchKind::Preview {
                if new_preview_layer.is_some_and(|id| id != batch.layer_id) {
                    return Err(GpuRasterError::MultiplePreviewLayers);
                }
                new_preview_layer = Some(batch.layer_id);
                new_preview_damage = new_preview_damage.union(visual_dirty);
                let plan = BrushPassPlan::for_style(&batch.style);
                new_preview_requires_base |=
                    plan.requires_destination() || batch.style.mode == DabMode::Erase;
                preview_is_watercolor |= plan.state.watercolor_wetness;
            }
            dirty = dirty.union(visual_dirty);
            self.metrics.dabs = self.metrics.dabs.saturating_add(dabs.len() as u64);
            for dab in dabs {
                self.metrics.raster_candidate_pixels = self
                    .metrics
                    .raster_candidate_pixels
                    .saturating_add(dab_candidate_pixels(*dab, packet.document_extent));
            }
        }
        if let Some(preview_layer_id) = new_preview_layer
            && packet
                .layers
                .iter()
                .find(|layer| layer.id == preview_layer_id)
                .is_some_and(|layer| layer.opacity != 1.0)
        {
            // Applying layer opacity independently to a base and overlay would
            // not equal applying it once to their combined layer result.
            new_preview_requires_base = true;
        }
        let new_preview_direct_to_composite = !needs_scene(packet.layers)
            && new_preview_layer.is_some()
            && !new_preview_requires_base
            && new_preview_layer.is_some_and(|layer_id| {
                preview_layer_is_frontmost_visible(layer_id, packet.layers)
            });
        let destination_preview_batches = packet
            .dab_batches
            .iter()
            .filter(|batch| {
                batch.kind == DabBatchKind::Preview
                    && batch.dab_count != 0
                    && BrushPassPlan::for_style(&batch.style).requires_destination()
            })
            .count();
        let new_preview_from_persistent = destination_preview_batches == 1
            && !needs_scene(packet.layers)
            && !preview_is_watercolor
            && packet
                .dab_batches
                .iter()
                .filter(|batch| batch.kind == DabBatchKind::Preview && batch.dab_count != 0)
                .all(|batch| BrushPassPlan::for_style(&batch.style).requires_destination());
        if new_preview_layer.is_none() {
            // Preview is disposable by contract. Release its high-water pool
            // when the tail commits or is cancelled instead of retaining pages
            // visited anywhere along the completed stroke.
            self.preview_pages.clear();
            self.preview_coverage_pages.clear();
            self.preview_watercolor_wetness_pages.clear();
        } else if new_preview_direct_to_composite {
            self.preview_pages.clear();
        } else {
            self.ensure_preview_pages(new_preview_damage);
            self.ensure_preview_coverage_pages(packet.dab_batches);
            self.ensure_preview_watercolor_wetness_pages(new_preview_damage, preview_is_watercolor);
            if !new_preview_from_persistent {
                self.ensure_preview_destination_companions(packet.dab_batches);
            }
        }

        // Style records serve brush batches, then composition layers.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("layer incremental sparse frame"),
            });
        self.telemetry.begin(&self.device, &mut encoder);
        let background_offset = self.prepare_uploads(packet, &mut encoder)?;
        self.layer_masks.prepare(
            &self.device,
            &mut encoder,
            packet.layers,
            original_batches,
            packet.document_extent,
            reset,
        );
        for layer in packet.layers {
            let Some(index) = self.paint_layers.iter().position(|l| l.id == layer.id) else {
                continue;
            };
            if reset
                && let Some((_, extent)) = layer.asset.as_ref().and_then(|a| self.images.get(a))
            {
                for c in page_coordinates(PixelRect::full([
                    extent[0].min(packet.document_extent[0]),
                    extent[1].min(packet.document_extent[1]),
                ])) {
                    if !self.paint_layers[index]
                        .pages
                        .iter()
                        .any(|p| p.coordinate == c)
                    {
                        let page = self.create_page(c, "imported paint page");
                        self.paint_layers[index].pages.push(page);
                    }
                }
            }
            for batch in original_batches.iter().filter(|b| b.layer_id == layer.id) {
                let DabBatchKind::LayerOperation(operation_index) = batch.kind else {
                    continue;
                };
                let op = &layer.operations[operation_index as usize];
                if matches!(
                    op.kind,
                    layer_core::LayerOperationKind::Fill { .. }
                        | layer_core::LayerOperationKind::Gradient { .. }
                ) {
                    // Coverage may be translated or inverted: its source mask
                    // pages are not necessarily the destination paint pages.
                    let bounds =
                        pixel_rect(op.bounds(packet.document_extent), packet.document_extent);
                    for c in page_coordinates(bounds) {
                        if self.paint_layers[index]
                            .pages
                            .iter()
                            .all(|p| p.coordinate != c)
                        {
                            let page = self.create_page(c, "fill paint page");
                            self.paint_layers[index].pages.push(page);
                        }
                    }
                }
            }
        }
        self.encode_mask_dabs(&mut encoder, packet.layers, original_batches)?;
        for batch in original_batches
            .iter()
            .filter(|b| layer_masks::MaskRenderer::is_mask(packet.layers, b.layer_id))
        {
            dirty = dirty.union(batch_pixel_rect(batch, packet.document_extent));
        }

        // Newly allocated pages are explicitly initialized on the GPU before
        // any Load operation. No pixel buffer crosses the CPU boundary.
        for layer in &self.paint_layers {
            for page in &layer.pages {
                if page.primary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.primary.view,
                        "layer clear new paint page",
                    );
                }
                if page.secondary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page
                            .secondary
                            .as_ref()
                            .expect("flag requires companion")
                            .view,
                        "layer clear new destination companion",
                    );
                }
            }
            for page in &layer.coverage_pages {
                if page.primary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.primary.view,
                        "layer clear new stroke coverage A",
                    );
                }
                if page.secondary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.secondary.view,
                        "layer clear new stroke coverage B",
                    );
                }
            }
            for page in &layer.material_pages {
                if page.needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.wetness.view,
                        "layer clear new canvas wetness",
                    );
                }
            }
            for page in &layer.watercolor_wetness_pages {
                if page.primary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.primary.view,
                        "layer clear new watercolor wetness A",
                    );
                }
                if page.secondary_needs_clear {
                    self.encode_clear(
                        &mut encoder,
                        &page.secondary.view,
                        "layer clear new watercolor wetness B",
                    );
                }
            }
        }
        if reset && packet.layers.iter().any(|l| l.asset.is_some()) {
            let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
            scene.initialize_images(self, packet.layers, &mut encoder)?;
            self.scene = Some(scene);
        }
        for layer in &mut self.paint_layers {
            for page in &mut layer.pages {
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
            }
            for page in &mut layer.coverage_pages {
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
            }
            for page in &mut layer.material_pages {
                page.needs_clear = false;
            }
            for page in &mut layer.watercolor_wetness_pages {
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
            }
        }

        // Persistent work is encoded before preview copies so prediction sees
        // this frame's committed ink.
        for (index, batch) in packet
            .dab_batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| batch.kind != DabBatchKind::Preview)
        {
            if let DabBatchKind::LayerOperation(op) = batch.kind {
                let layer_index = packet
                    .layers
                    .iter()
                    .position(|l| l.id == batch.layer_id)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
                let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
                scene.style_base = packet.dab_batches.len();
                scene.apply_operation(self, packet, layer_index, op as usize, &mut encoder)?;
                self.scene = Some(scene);
                let bounds = packet.layers[layer_index].operations[op as usize]
                    .bounds(packet.document_extent);
                let offset = scene::world_offset(packet.layers, batch.layer_id, false);
                dirty = dirty.union(pixel_rect(
                    layer_core::Rect {
                        min: layer_core::Point {
                            x: bounds.min.x + offset.x,
                            y: bounds.min.y + offset.y,
                        },
                        max: layer_core::Point {
                            x: bounds.max.x + offset.x,
                            y: bounds.max.y + offset.y,
                        },
                    },
                    packet.document_extent,
                ));
                continue;
            }
            if batch.style.execution == BrushExecution::Watercolor
                && batch.dab_count > 0
                && let Some(stored) = self
                    .paint_layers
                    .iter_mut()
                    .find(|l| l.id == batch.layer_id)
            {
                stored.watercolor = Some(WatercolorLayerStyle::from_dab_style(&batch.style));
            }
            self.encode_brush_batch(
                &mut encoder,
                index,
                batch,
                BrushEncodingContext {
                    batches: packet.dab_batches,
                    dabs: packet.dabs,
                    document_extent: packet.document_extent,
                    target: BrushEncodingTarget::Persistent,
                },
            )?;
            if batch.stroke_end && BrushPassPlan::for_style(&batch.style).stroke_edge {
                self.encode_stroke_edge(&mut encoder, index, batch)?;
            }
        }

        if let Some(layer_id) = new_preview_layer
            && !new_preview_direct_to_composite
        {
            for page in &mut self.preview_pages {
                page.active_secondary = false;
            }
            for page in &mut self.preview_coverage_pages {
                page.active_secondary = false;
            }
            for page in &mut self.preview_watercolor_wetness_pages {
                page.active_secondary = false;
            }
            let copied = new_preview_damage;
            let source = self
                .paint_layers
                .iter()
                .find(|layer| layer.id == layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?;
            if !copied.is_empty() && !new_preview_from_persistent {
                for coordinate in page_coordinates(copied) {
                    let preview = self
                        .preview_pages
                        .iter()
                        .find(|page| page.coordinate == coordinate)
                        .expect("preview pages are prepared before encoding");
                    let local = if preview_is_watercolor || needs_scene(packet.layers) {
                        page_rect(coordinate).page_local(coordinate)
                    } else {
                        copied
                            .intersect(page_rect(coordinate))
                            .page_local(coordinate)
                    };
                    if new_preview_requires_base
                        && let Some(source) = source
                            .pages
                            .iter()
                            .find(|page| page.coordinate == coordinate)
                    {
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &source.active().texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x,
                                    y: local.min_y,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &preview.primary.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x,
                                    y: local.min_y,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: local.width(),
                                height: local.height(),
                                depth_or_array_layers: 1,
                            },
                        );
                    } else {
                        self.encode_clear(
                            &mut encoder,
                            &preview.primary.view,
                            "layer reset preview page",
                        );
                    }
                }
            }
            // Prediction is a disposable fork of committed stroke coverage.
            // Initialize only pages touched by this preview, then let its
            // microbatches ping-pong the private copy exactly like persistent
            // watercolor. This prevents a darker preview or a pen-up pop.
            for coverage in &self.preview_coverage_pages {
                let Some(batch) = packet.dab_batches.iter().find(|batch| {
                    batch.kind == DabBatchKind::Preview
                        && batch.layer_id == layer_id
                        && BrushPassPlan::for_style(&batch.style).state.coverage
                        && !batch_pixel_rect(batch, packet.document_extent)
                            .intersect(page_rect(coverage.coordinate))
                            .is_empty()
                }) else {
                    continue;
                };
                if let Some(committed) = source.coverage_pages.iter().find(|page| {
                    page.coordinate == coverage.coordinate && page.owner == Some(batch.stroke_id)
                }) {
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &committed.active().texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &coverage.primary.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: PAGE_SIZE,
                            height: PAGE_SIZE,
                            depth_or_array_layers: 1,
                        },
                    );
                } else {
                    self.encode_clear(
                        &mut encoder,
                        &coverage.primary.view,
                        "layer reset preview stroke coverage",
                    );
                }
            }
            // Prediction forks the persistent watercolor wetness as another
            // disposable sparse surface. Preview dabs union into this copy,
            // so the live edge follows predicted paint without mutating the
            // committed material mask.
            if preview_is_watercolor {
                let mask_halo = packet
                    .dab_batches
                    .iter()
                    .filter(|batch| {
                        batch.kind == DabBatchKind::Preview
                            && batch.layer_id == layer_id
                            && BrushPassPlan::for_style(&batch.style)
                                .state
                                .watercolor_wetness
                    })
                    .map(|batch| WatercolorLayerStyle::from_dab_style(&batch.style).radius())
                    .max()
                    .unwrap_or(0);
                let mask_source_damage =
                    new_preview_damage.expand(mask_halo, packet.document_extent);
                for preview_wetness in &self.preview_watercolor_wetness_pages {
                    let local = mask_source_damage
                        .intersect(page_rect(preview_wetness.coordinate))
                        .page_local(preview_wetness.coordinate);
                    if local.is_empty() {
                        continue;
                    }
                    if let Some(committed) = source
                        .watercolor_wetness_pages
                        .iter()
                        .find(|page| page.coordinate == preview_wetness.coordinate)
                    {
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &committed.active().texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x,
                                    y: local.min_y,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &preview_wetness.primary.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x,
                                    y: local.min_y,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: local.width(),
                                height: local.height(),
                                depth_or_array_layers: 1,
                            },
                        );
                    } else {
                        self.encode_clear(
                            &mut encoder,
                            &preview_wetness.primary.view,
                            "layer reset preview watercolor wetness",
                        );
                    }
                }
            }
            for (index, batch) in packet
                .dab_batches
                .iter()
                .enumerate()
                .filter(|(_, batch)| batch.kind == DabBatchKind::Preview)
            {
                self.encode_brush_batch(
                    &mut encoder,
                    index,
                    batch,
                    BrushEncodingContext {
                        batches: packet.dab_batches,
                        dabs: packet.dabs,
                        document_extent: packet.document_extent,
                        target: BrushEncodingTarget::Preview {
                            from_persistent: new_preview_from_persistent,
                        },
                    },
                )?;
            }
        }
        self.preview_damage = new_preview_damage;
        self.preview_layer_id = new_preview_layer;
        self.preview_requires_base = new_preview_requires_base;
        self.preview_direct_to_composite = new_preview_direct_to_composite;

        if packet.composite_all || reset {
            dirty = PixelRect::full(packet.document_extent);
        }

        let animated = packet
            .layers
            .iter()
            .any(|l| l.visible && l.effect.as_ref().is_some_and(|e| e.animated()));
        if !dirty.is_empty() || animated {
            self.composite_revision = self.composite_revision.wrapping_add(1);
        }
        if (!dirty.is_empty() || animated) && needs_scene(packet.layers) {
            let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
            scene.style_base = packet.dab_batches.len();
            // A moved target's damage is stored in image coordinates. Round to
            // scene tiles after applying the target's document translation.
            for batch in original_batches {
                if let Some(layer) = packet.layers.iter().find(|l| {
                    l.id == batch.layer_id
                        || l.mask.as_ref().is_some_and(|m| m.id == batch.layer_id)
                }) {
                    let offset =
                        scene::world_offset(packet.layers, layer.id, layer.id != batch.layer_id);
                    let mut rect = batch.damage;
                    rect.min.x += offset.x;
                    rect.max.x += offset.x;
                    rect.min.y += offset.y;
                    rect.max.y += offset.y;
                    dirty = dirty.union(pixel_rect(rect, packet.document_extent));
                }
            }
            scene.compose(self, packet, dirty, &mut encoder, true)?;
            self.scene = Some(scene);
        } else if !dirty.is_empty() {
            struct WatercolorBinding {
                layer_id: LayerId,
                coordinate: [u32; 2],
                preview: bool,
                bind_group: wgpu::BindGroup,
            }

            // Bind groups must outlive the render pass that references them.
            // Build only sparse pages whose 3x3 neighborhood contains layer
            // color; this also covers the narrow outside edge halo.
            let mut watercolor_bindings = Vec::new();
            for layer in packet.layers.iter().filter(|layer| layer.visible) {
                let Some(stored) = self
                    .paint_layers
                    .iter()
                    .find(|stored| stored.id == layer.id)
                else {
                    continue;
                };
                let active_preview = self.preview_layer_id == Some(layer.id)
                    && packet.dab_batches.iter().any(|batch| {
                        batch.kind == DabBatchKind::Preview
                            && batch.layer_id == layer.id
                            && batch.style.execution == BrushExecution::Watercolor
                    });
                if stored.watercolor.is_none() && !active_preview {
                    continue;
                }
                for coordinate in page_coordinates(dirty) {
                    if let Some(bind_group) =
                        self.watercolor_neighborhood_bind_group(stored, coordinate, false)
                    {
                        watercolor_bindings.push(WatercolorBinding {
                            layer_id: layer.id,
                            coordinate,
                            preview: false,
                            bind_group,
                        });
                    }
                    let use_preview = self.preview_layer_id == Some(layer.id)
                        && !self.preview_direct_to_composite
                        && !self
                            .preview_damage
                            .intersect(page_rect(coordinate))
                            .is_empty();
                    if use_preview
                        && let Some(bind_group) =
                            self.watercolor_neighborhood_bind_group(stored, coordinate, true)
                    {
                        watercolor_bindings.push(WatercolorBinding {
                            layer_id: layer.id,
                            coordinate,
                            preview: true,
                            bind_group,
                        });
                    }
                }
            }

            // Encode composition directly so the background may use its own
            // dynamic record even when brush record zero is live.
            let target = self.composite_view.as_ref().expect("composite exists");
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer incremental composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(dirty.min_x, dirty.min_y, dirty.width(), dirty.height());
            pass.set_pipeline(&self.pipelines.background);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[background_offset as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(2, &self.target_bind_group, &[0]);
            pass.set_bind_group(1, &self.pipelines.background_empty, &[]);
            pass.draw(0..3, 0..1);
            let mut composited_pixels = 0_u64;
            for (layer_index, layer) in packet
                .layers
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, layer)| layer.visible)
            {
                let Some(stored) = self
                    .paint_layers
                    .iter()
                    .find(|stored| stored.id == layer.id)
                else {
                    continue;
                };
                let record = packet.dab_batches.len() + layer_index;
                pass.set_bind_group(
                    0,
                    &self.style_bind_group,
                    &[record as u32 * self.style_stride as u32],
                );
                let watercolor = stored.watercolor.is_some()
                    || packet.dab_batches.iter().any(|batch| {
                        batch.layer_id == layer.id
                            && batch.style.execution == BrushExecution::Watercolor
                    });
                if watercolor {
                    pass.set_pipeline(&self.pipelines.watercolor_composite);
                    for coordinate in page_coordinates(dirty) {
                        let page_dirty = dirty.intersect(page_rect(coordinate));
                        if page_dirty.is_empty() {
                            continue;
                        }
                        let use_preview = self.preview_layer_id == Some(layer.id)
                            && !self.preview_direct_to_composite
                            && !self
                                .preview_damage
                                .intersect(page_rect(coordinate))
                                .is_empty();
                        let preview_clip = if use_preview {
                            page_dirty.intersect(self.preview_damage)
                        } else {
                            PixelRect::EMPTY
                        };
                        let persistent = watercolor_bindings.iter().find(|binding| {
                            binding.layer_id == layer.id
                                && binding.coordinate == coordinate
                                && !binding.preview
                        });
                        let preview = watercolor_bindings.iter().find(|binding| {
                            binding.layer_id == layer.id
                                && binding.coordinate == coordinate
                                && binding.preview
                        });
                        let mut draws = [(None, PixelRect::EMPTY); 5];
                        if use_preview && self.preview_requires_base {
                            for (slot, region) in
                                page_dirty.subtract(preview_clip).into_iter().enumerate()
                            {
                                draws[slot] = (persistent, region);
                            }
                            draws[4] = (preview, preview_clip);
                        } else {
                            draws[0] = (persistent, page_dirty);
                            draws[1] = (preview, preview_clip);
                        }
                        for (binding, clipped) in draws
                            .into_iter()
                            .filter_map(|(binding, clipped)| {
                                binding.map(|binding| (binding, clipped))
                            })
                            .filter(|(_, clipped)| !clipped.is_empty())
                        {
                            pass.set_scissor_rect(
                                clipped.min_x,
                                clipped.min_y,
                                clipped.width(),
                                clipped.height(),
                            );
                            pass.set_bind_group(
                                1,
                                &self.target_bind_group,
                                &[self.target_offset(coordinate)],
                            );
                            pass.set_bind_group(2, &binding.bind_group, &[]);
                            pass.draw(0..3, 0..1);
                            composited_pixels = composited_pixels.saturating_add(clipped.area());
                        }
                    }
                    continue;
                }

                pass.set_pipeline(&self.pipelines.composite);
                for coordinate in page_coordinates(dirty) {
                    let use_preview = self.preview_layer_id == Some(layer.id)
                        && !self.preview_direct_to_composite
                        && !self
                            .preview_damage
                            .intersect(page_rect(coordinate))
                            .is_empty();
                    let preview = if use_preview {
                        self.preview_pages
                            .iter()
                            .find(|page| page.coordinate == coordinate)
                    } else {
                        None
                    };
                    let persistent = stored
                        .pages
                        .iter()
                        .find(|page| page.coordinate == coordinate);
                    let page_dirty = dirty.intersect(page_rect(coordinate));
                    if page_dirty.is_empty() {
                        continue;
                    }
                    let preview_clip = if use_preview {
                        page_dirty.intersect(self.preview_damage)
                    } else {
                        PixelRect::EMPTY
                    };
                    let mut draws = [(None, PixelRect::EMPTY); 5];
                    if use_preview && self.preview_requires_base {
                        for (slot, region) in
                            page_dirty.subtract(preview_clip).into_iter().enumerate()
                        {
                            draws[slot] = (persistent, region);
                        }
                        draws[4] = (preview, preview_clip);
                    } else {
                        draws[0] = (persistent, page_dirty);
                        draws[1] = (preview, preview_clip);
                    }
                    for (page, clipped) in draws
                        .into_iter()
                        .filter_map(|(page, clipped)| page.map(|page| (page, clipped)))
                        .filter(|(_, clipped)| !clipped.is_empty())
                    {
                        pass.set_scissor_rect(
                            clipped.min_x,
                            clipped.min_y,
                            clipped.width(),
                            clipped.height(),
                        );
                        pass.set_bind_group(1, &page.active().texture_bind_group, &[]);
                        pass.set_bind_group(
                            2,
                            &self.target_bind_group,
                            &[self.target_offset(coordinate)],
                        );
                        pass.draw(0..3, 0..1);
                        composited_pixels = composited_pixels.saturating_add(clipped.area());
                    }
                }
            }
            drop(pass);
            if self.preview_direct_to_composite {
                for (index, batch) in packet
                    .dab_batches
                    .iter()
                    .enumerate()
                    .filter(|(_, batch)| batch.kind == DabBatchKind::Preview)
                {
                    let batch_dirty = batch_pixel_rect(batch, packet.document_extent);
                    if batch.dab_count != 0 && !batch_dirty.is_empty() {
                        self.encode_batch(&mut encoder, index, batch, target, batch_dirty, 0)?;
                    }
                }
            }
            self.metrics.composited_pixels = self
                .metrics
                .composited_pixels
                .saturating_add(dirty.area().saturating_add(composited_pixels));
        }

        self.uploads.finish(&encoder);
        self.telemetry.end(&mut encoder);
        let submission = self.queue.submit([encoder.finish()]);
        self.telemetry.submitted();
        self.last_submission = Some(submission);
        self.metrics.submissions = self.metrics.submissions.saturating_add(1);
        self.refresh_storage_metrics();
        if let Some(started) = started {
            self.telemetry
                .cpu
                .push(started.elapsed().as_secs_f32() * 1000.);
        }
        Ok(())
    }

    fn request_readback(&mut self, request_id: u64) -> Result<(), Self::Error> {
        let [width, height] = self.document_extent;
        let stride = width.checked_mul(4).ok_or(GpuRasterError::SizeOverflow)?;
        let mut bytes = vec![0; stride as usize * height as usize];
        self.copy_rgba8_srgb(&mut bytes, stride as usize)?;
        self.pending_readback = Some(ReadbackImage {
            request_id,
            width,
            height,
            stride,
            bytes,
        });
        Ok(())
    }

    fn take_readback(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.pending_readback.take().map(Ok)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StyleGpu {
    color: [f32; 4],
    canvas_opacity: [f32; 4],
    grain: [f32; 4],
    dual: [f32; 4],
    dual_offset_flags: [f32; 4],
    flags: [f32; 4],
    dual_grain: [f32; 4],
    advanced: [f32; 4],
    edges: [f32; 4],
    material_a: [f32; 4],
    material_b: [f32; 4],
    operation: [u32; 4],
    deformation: [f32; 4],
    render_mode: [f32; 4],
    transport_a: [f32; 4],
    transport_b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TargetGpu {
    origin_extent: [f32; 4],
    document_extent: [f32; 4],
}

impl TargetGpu {
    fn new(origin: [u32; 2], extent: [u32; 2], document_extent: [u32; 2]) -> Self {
        Self {
            origin_extent: [
                origin[0] as f32,
                origin[1] as f32,
                extent[0] as f32,
                extent[1] as f32,
            ],
            document_extent: [
                document_extent[0] as f32,
                document_extent[1] as f32,
                0.0,
                0.0,
            ],
        }
    }
}

impl StyleGpu {
    fn plain(extent: [u32; 2], color: [f32; 4], opacity: f32) -> Self {
        Self {
            color,
            canvas_opacity: [extent[0] as f32, extent[1] as f32, opacity, 0.0],
            grain: [0.0; 4],
            dual: [0.0; 4],
            dual_offset_flags: [0.0; 4],
            flags: [0.0; 4],
            dual_grain: [0.0; 4],
            advanced: [0.0; 4],
            edges: [0.0; 4],
            material_a: [0.0; 4],
            material_b: [0.0; 4],
            operation: [0; 4],
            deformation: [0.0; 4],
            render_mode: [0.0; 4],
            transport_a: [0.0; 4],
            transport_b: [0.0; 4],
        }
    }

    fn layer(extent: [u32; 2], opacity: f32, watercolor: Option<WatercolorLayerStyle>) -> Self {
        let mut result = Self::plain(extent, [0.0; 4], opacity);
        if let Some(watercolor) = watercolor {
            result.edges = [
                watercolor.wet_edge,
                watercolor.burnt_edge,
                watercolor.edge_width,
                0.0,
            ];
        }
        result
    }

    fn brush(extent: [u32; 2], batch: &DabBatch) -> Self {
        let style = &batch.style;
        let plan = BrushPassPlan::for_style(style);
        let grain = style.grain.as_ref();
        let dual = style.dual.as_deref();
        let dual_grain = dual.and_then(|dual| dual.grain.as_ref());
        let (grain_cos, grain_sin) = grain
            .map(|grain| (grain.rotation_radians.cos(), grain.rotation_radians.sin()))
            .unwrap_or((1.0, 0.0));
        let (dual_cos, dual_sin) = dual
            .map(|dual| (dual.angle_radians.cos(), dual.angle_radians.sin()))
            .unwrap_or((1.0, 0.0));
        let (dual_grain_cos, dual_grain_sin) = dual_grain
            .map(|grain| (grain.rotation_radians.cos(), grain.rotation_radians.sin()))
            .unwrap_or((1.0, 0.0));
        let mut result = Self::plain(extent, [0.0; 4], 1.0);
        result.grain = [
            grain.map_or(1.0, |grain| grain.scale),
            grain.map_or(0.0, |grain| grain.depth),
            grain_cos,
            grain_sin,
        ];
        result.dual = [
            dual.map_or(1.0, |dual| dual.scale),
            dual.map_or(1.0, |dual| dual.aspect),
            dual_cos,
            dual_sin,
        ];
        result.dual_offset_flags = [
            dual.map_or(0.0, |dual| dual.offset[0]),
            dual.map_or(0.0, |dual| dual.offset[1]),
            dual.map_or(0.0, |dual| dual_combine_code(dual.combine)),
            style.rendering.alpha_threshold,
        ];
        result.flags = [
            f32::from(matches!(style.tip, BrushTip::AnalyticEllipse)),
            f32::from(grain.is_some()),
            f32::from(dual.is_some_and(|dual| matches!(dual.tip, BrushTip::AnalyticEllipse))),
            f32::from(dual.is_some()),
        ];
        result.dual_grain = [
            dual_grain.map_or(1.0, |grain| grain.scale),
            dual_grain.map_or(0.0, |grain| grain.depth),
            dual_grain_cos,
            dual_grain_sin,
        ];
        result.advanced = [
            f32::from(dual_grain.is_some()),
            f32::from(grain.is_some_and(|grain| grain.behavior == BrushGrainBehavior::Canvas)),
            f32::from(dual_grain.is_some_and(|grain| grain.behavior == BrushGrainBehavior::Canvas)),
            grain.map_or(0.0, |grain| grain.offset_jitter),
        ];
        result.edges = [
            style.rendering.wet_edge,
            style.rendering.burnt_edge,
            style.rendering.edge_width,
            dual_grain.map_or(0.0, |grain| grain.offset_jitter),
        ];
        result.material_a = [
            style.wet_mix.amount_of_paint,
            style.wet_mix.density,
            style.wet_mix.wetness,
            style.wet_mix.dilution,
        ];
        result.material_b = [
            style.wet_mix.attack,
            style.wet_mix.pull,
            style.wet_mix.blur,
            style.wet_mix.wetness_jitter,
        ];
        result.operation = [
            batch.first_dab,
            batch.dab_count,
            plan.material as u32,
            u32::from(style.mode == DabMode::Erase),
        ];
        result.deformation = [
            liquify_mode_code(style.deform.mode),
            style.deform.strength,
            style.deform.pressure,
            style.deform.momentum,
        ];
        result.render_mode = [
            blend_mode_code(style.rendering.blend_mode),
            f32::from(style.rendering.accumulation == BrushAccumulation::Uniform),
            f32::from(style.wet_mix.mix_space == ColorMixSpace::Oklab),
            style.deform.distortion,
        ];
        if let Some(transport) = &style.transport {
            result.transport_a = [
                transport.scale,
                transport.rotation_radians.cos(),
                transport.rotation_radians.sin(),
                transport.contrast,
            ];
            result.transport_b = [
                transport.wet_flow,
                transport.dry_flow,
                transport.distance,
                transport.water_load,
            ];
        }
        // This lane is unused by dry and composite shaders and avoids growing
        // every style upload solely for one material-stage lifecycle bit.
        result.canvas_opacity[3] = f32::from(batch.stroke_start);
        result.color[3] = f32::from(style.alpha_locked);
        result
    }
}

fn preview_layer_is_frontmost_visible(layer_id: LayerId, layers: &[Layer]) -> bool {
    let Some(index) = layers.iter().position(|layer| layer.id == layer_id) else {
        return false;
    };
    layers[index].visible && layers[..index].iter().all(|layer| !layer.visible)
}

fn dual_combine_code(mode: DualCombineMode) -> f32 {
    match mode {
        DualCombineMode::Multiply => 0.0,
        DualCombineMode::Add => 1.0,
        DualCombineMode::Subtract => 2.0,
        DualCombineMode::Difference => 3.0,
        DualCombineMode::Min => 4.0,
        DualCombineMode::Max => 5.0,
    }
}

fn blend_mode_code(mode: BrushBlendMode) -> f32 {
    match mode {
        BrushBlendMode::Normal => 0.0,
        BrushBlendMode::Multiply => 1.0,
        BrushBlendMode::Screen => 2.0,
        BrushBlendMode::Add => 3.0,
        BrushBlendMode::Subtract => 4.0,
        BrushBlendMode::Darken => 5.0,
        BrushBlendMode::Lighten => 6.0,
        BrushBlendMode::Overlay => 7.0,
    }
}

fn liquify_mode_code(mode: LiquifyMode) -> f32 {
    match mode {
        LiquifyMode::Push => 0.0,
        LiquifyMode::TwirlClockwise => 1.0,
        LiquifyMode::TwirlCounterClockwise => 2.0,
        LiquifyMode::Pinch => 3.0,
        LiquifyMode::Expand => 4.0,
        LiquifyMode::Crystals => 5.0,
        LiquifyMode::Edge => 6.0,
        LiquifyMode::Reconstruct => 7.0,
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PixelRect {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

impl PixelRect {
    const EMPTY: Self = Self {
        min_x: u32::MAX,
        min_y: u32::MAX,
        max_x: 0,
        max_y: 0,
    };
    fn full(extent: [u32; 2]) -> Self {
        Self {
            min_x: 0,
            min_y: 0,
            max_x: extent[0],
            max_y: extent[1],
        }
    }
    fn is_empty(self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y
    }
    fn width(self) -> u32 {
        self.max_x - self.min_x
    }
    fn height(self) -> u32 {
        self.max_y - self.min_y
    }
    fn area(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }
    fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn intersect(self, other: Self) -> Self {
        let result = Self {
            min_x: self.min_x.max(other.min_x),
            min_y: self.min_y.max(other.min_y),
            max_x: self.max_x.min(other.max_x),
            max_y: self.max_y.min(other.max_y),
        };
        if result.is_empty() {
            Self::EMPTY
        } else {
            result
        }
    }

    fn expand(self, radius: u32, extent: [u32; 2]) -> Self {
        if self.is_empty() {
            return self;
        }
        Self {
            min_x: self.min_x.saturating_sub(radius),
            min_y: self.min_y.saturating_sub(radius),
            max_x: self.max_x.saturating_add(radius).min(extent[0]),
            max_y: self.max_y.saturating_add(radius).min(extent[1]),
        }
    }

    /// Splits `self - other` into non-overlapping top, bottom, left, and right
    /// rectangles. This keeps sparse preview composition exact without a mask
    /// texture or a canvas-pixel operation on the host.
    fn subtract(self, other: Self) -> [Self; 4] {
        if self.is_empty() {
            return [Self::EMPTY; 4];
        }
        let overlap = self.intersect(other);
        if overlap.is_empty() {
            return [self, Self::EMPTY, Self::EMPTY, Self::EMPTY];
        }
        [
            Self {
                min_x: self.min_x,
                min_y: self.min_y,
                max_x: self.max_x,
                max_y: overlap.min_y,
            },
            Self {
                min_x: self.min_x,
                min_y: overlap.max_y,
                max_x: self.max_x,
                max_y: self.max_y,
            },
            Self {
                min_x: self.min_x,
                min_y: overlap.min_y,
                max_x: overlap.min_x,
                max_y: overlap.max_y,
            },
            Self {
                min_x: overlap.max_x,
                min_y: overlap.min_y,
                max_x: self.max_x,
                max_y: overlap.max_y,
            },
        ]
    }

    fn page_local(self, coordinate: [u32; 2]) -> Self {
        let origin_x = coordinate[0] * PAGE_SIZE;
        let origin_y = coordinate[1] * PAGE_SIZE;
        Self {
            min_x: self.min_x.saturating_sub(origin_x),
            min_y: self.min_y.saturating_sub(origin_y),
            max_x: self.max_x.saturating_sub(origin_x).min(PAGE_SIZE),
            max_y: self.max_y.saturating_sub(origin_y).min(PAGE_SIZE),
        }
    }
}

fn page_rect(coordinate: [u32; 2]) -> PixelRect {
    let min_x = coordinate[0] * PAGE_SIZE;
    let min_y = coordinate[1] * PAGE_SIZE;
    PixelRect {
        min_x,
        min_y,
        max_x: min_x + PAGE_SIZE,
        max_y: min_y + PAGE_SIZE,
    }
}

fn page_coordinates(rect: PixelRect) -> impl Iterator<Item = [u32; 2]> {
    let min_x = rect.min_x / PAGE_SIZE;
    let min_y = rect.min_y / PAGE_SIZE;
    let max_x = rect.max_x.saturating_sub(1) / PAGE_SIZE;
    let max_y = rect.max_y.saturating_sub(1) / PAGE_SIZE;
    (min_y..=max_y).flat_map(move |y| (min_x..=max_x).map(move |x| [x, y]))
}

fn pixel_rect(rect: layer_core::Rect, extent: [u32; 2]) -> PixelRect {
    if rect.is_empty() {
        return PixelRect::EMPTY;
    }
    PixelRect {
        min_x: rect.min.x.floor().max(0.0).min(extent[0] as f32) as u32,
        min_y: rect.min.y.floor().max(0.0).min(extent[1] as f32) as u32,
        max_x: rect.max.x.ceil().max(0.0).min(extent[0] as f32) as u32,
        max_y: rect.max.y.ceil().max(0.0).min(extent[1] as f32) as u32,
    }
}

fn batch_pixel_rect(batch: &DabBatch, extent: [u32; 2]) -> PixelRect {
    let damage = pixel_rect(batch.damage, extent);
    let transport_radius = batch
        .style
        .transport
        .as_ref()
        .filter(|transport| transport.wet_flow > 0.0 || transport.dry_flow > 0.0)
        .map_or(0, |transport| transport.distance.ceil() as u32);
    damage.expand(transport_radius, extent)
}

fn is_first_watercolor_update_batch(batches: &[DabBatch], index: usize) -> bool {
    let current = &batches[index];
    batches[..index].iter().all(|earlier| {
        earlier.dab_count == 0
            || earlier.kind != current.kind
            || earlier.stroke_id != current.stroke_id
            || earlier.layer_id != current.layer_id
    })
}

fn is_last_watercolor_update_batch(batches: &[DabBatch], index: usize) -> bool {
    let current = &batches[index];
    batches[index + 1..].iter().all(|later| {
        later.dab_count == 0
            || later.kind != current.kind
            || later.stroke_id != current.stroke_id
            || later.layer_id != current.layer_id
    })
}

fn watercolor_update_damages(
    batches: &[DabBatch],
    index: usize,
    extent: [u32; 2],
) -> Vec<PixelRect> {
    let current = &batches[index];
    batches
        .iter()
        .filter(|batch| {
            batch.dab_count != 0
                && batch.kind == current.kind
                && batch.stroke_id == current.stroke_id
                && batch.layer_id == current.layer_id
        })
        .map(|batch| batch_pixel_rect(batch, extent))
        .collect()
}

fn unique_page_coordinates(damages: &[PixelRect]) -> Vec<[u32; 2]> {
    let mut coordinates = Vec::new();
    for damage in damages {
        for coordinate in page_coordinates(*damage) {
            if !coordinates.contains(&coordinate) {
                coordinates.push(coordinate);
            }
        }
    }
    coordinates
}

fn dab_candidate_pixels(dab: Dab, extent: [u32; 2]) -> u64 {
    let [cos, sin] = dab.rotation;
    let extent_x = (dab.radii[0] * cos).hypot(dab.radii[1] * sin) + 1.0;
    let extent_y = (dab.radii[0] * sin).hypot(dab.radii[1] * cos) + 1.0;
    let rect = PixelRect {
        min_x: (dab.center.x - extent_x)
            .floor()
            .max(0.0)
            .min(extent[0] as f32) as u32,
        min_y: (dab.center.y - extent_y)
            .floor()
            .max(0.0)
            .min(extent[1] as f32) as u32,
        max_x: (dab.center.x + extent_x)
            .ceil()
            .max(0.0)
            .min(extent[0] as f32) as u32,
        max_y: (dab.center.y + extent_y)
            .ceil()
            .max(0.0)
            .min(extent[1] as f32) as u32,
    };
    if rect.is_empty() { 0 } else { rect.area() }
}

fn create_style_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer style layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
            },
            count: None,
        }],
    })
}

fn create_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer sampled texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn create_advanced_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture = wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let mut entries = Vec::with_capacity(6);
    for binding in 0..5 {
        entries.push(wgpu::BindGroupLayoutEntry { binding, ..texture });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 5,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    });
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer advanced brush texture layout"),
        entries: &entries,
    })
}

fn create_target_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer render target layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: NonZeroU64::new(mem::size_of::<TargetGpu>() as u64),
            },
            count: None,
        }],
    })
}

fn create_material_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = Vec::with_capacity(12);
    for binding in 0..9 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 9,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    for binding in 10..12 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer material source neighborhood layout"),
        entries: &entries,
    })
}

fn create_edge_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entries = (0..10)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer post-stroke edge sources"),
        entries: &entries,
    })
}

fn create_color_neighborhood_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    // Five cardinal color pages plus the full 3x3 watercolor-wetness
    // neighborhood stay within the portable 16-texture fragment-stage limit.
    let entries = (0..14)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect::<Vec<_>>();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer watercolor pigment and wetness neighborhood"),
        entries: &entries,
    })
}

fn create_transport_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let mut entries = Vec::with_capacity(10);
    for binding in 0..10 {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer watercolor transport sources"),
        entries: &entries,
    })
}

fn create_style_buffer(device: &wgpu::Device, stride: u64, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layer dynamic styles"),
        size: stride * capacity as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_target_buffer(device: &wgpu::Device, stride: u64, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("layer dynamic render targets"),
        size: stride * capacity as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn create_style_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer dynamic style binding"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
            }),
        }],
    })
}

fn create_target_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer dynamic render target binding"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: NonZeroU64::new(mem::size_of::<TargetGpu>() as u64),
            }),
        }],
    })
}

fn create_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &'static str,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_advanced_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: [&wgpu::TextureView; 5],
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer advanced brush textures"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(views[0]),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(views[1]),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(views[2]),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(views[3]),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(views[4]),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_material_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: &[&wgpu::TextureView],
    dabs: &wgpu::Buffer,
    coverage: &wgpu::TextureView,
    reservoir: &wgpu::TextureView,
) -> wgpu::BindGroup {
    debug_assert_eq!(views.len(), 9);
    let mut entries = Vec::with_capacity(10);
    for (binding, view) in views.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    entries.push(wgpu::BindGroupEntry {
        binding: 9,
        resource: dabs.as_entire_binding(),
    });
    for (binding, view) in [(10, coverage), (11, reservoir)] {
        entries.push(wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer material source neighborhood"),
        layout,
        entries: &entries,
    })
}

fn create_edge_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    color: &wgpu::TextureView,
    coverage: &[&wgpu::TextureView],
) -> wgpu::BindGroup {
    debug_assert_eq!(coverage.len(), 9);
    let mut entries = Vec::with_capacity(10);
    entries.push(wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(color),
    });
    entries.extend(
        coverage
            .iter()
            .enumerate()
            .map(|(index, view)| wgpu::BindGroupEntry {
                binding: index as u32 + 1,
                resource: wgpu::BindingResource::TextureView(view),
            }),
    );
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer post-stroke edge sources"),
        layout,
        entries: &entries,
    })
}

fn create_watercolor_neighborhood_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    colors: &[&wgpu::TextureView],
    wetness: &[&wgpu::TextureView],
) -> wgpu::BindGroup {
    debug_assert_eq!(colors.len(), 5);
    debug_assert_eq!(wetness.len(), 9);
    let mut entries = colors
        .iter()
        .enumerate()
        .map(|(index, view)| wgpu::BindGroupEntry {
            binding: index as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect::<Vec<_>>();
    entries.extend(
        wetness
            .iter()
            .enumerate()
            .map(|(index, view)| wgpu::BindGroupEntry {
                binding: index as u32 + 5,
                resource: wgpu::BindingResource::TextureView(view),
            }),
    );
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer watercolor pigment and wetness neighborhood"),
        layout,
        entries: &entries,
    })
}

fn create_transport_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    colors: &[&wgpu::TextureView],
    wetness: &[&wgpu::TextureView],
) -> wgpu::BindGroup {
    debug_assert_eq!(colors.len(), 5);
    debug_assert_eq!(wetness.len(), 5);
    let entries = colors
        .iter()
        .chain(wetness.iter())
        .enumerate()
        .map(|(index, view)| wgpu::BindGroupEntry {
            binding: index as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect::<Vec<_>>();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer watercolor transport sources"),
        layout,
        entries: &entries,
    })
}

fn create_color_target(
    device: &wgpu::Device,
    extent: [u32; 2],
    label: &'static str,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_target(device, extent, COLOR_FORMAT, label)
}

fn create_target(
    device: &wgpu::Device,
    extent: [u32; 2],
    format: wgpu::TextureFormat,
    label: &'static str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: extent[0],
            height: extent[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_page_surface(
    device: &wgpu::Device,
    texture_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    extent: [u32; 2],
    format: wgpu::TextureFormat,
    label: &'static str,
) -> PageSurface {
    let (texture, view) = create_target(device, extent, format, label);
    let texture_bind_group = create_texture_bind_group(
        device,
        texture_layout,
        &view,
        sampler,
        "layer sparse surface binding",
    );
    PageSurface {
        texture,
        view,
        texture_bind_group,
    }
}

struct PipelineLayouts<'a> {
    style: &'a wgpu::BindGroupLayout,
    texture: &'a wgpu::BindGroupLayout,
    advanced_texture: &'a wgpu::BindGroupLayout,
    target: &'a wgpu::BindGroupLayout,
    material: &'a wgpu::BindGroupLayout,
    edge: &'a wgpu::BindGroupLayout,
    watercolor: &'a wgpu::BindGroupLayout,
    transport: &'a wgpu::BindGroupLayout,
}

fn compose_wgsl(parts: &[&str]) -> Cow<'static, str> {
    let mut source = String::with_capacity(parts.iter().map(|part| part.len() + 1).sum());
    for part in parts {
        source.push_str(part);
        source.push('\n');
    }
    Cow::Owned(source)
}

fn create_pipelines(device: &wgpu::Device, layouts: PipelineLayouts<'_>) -> Pipelines {
    let brush = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer dry brush shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("brush.wgsl").into()),
    });
    let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer composite shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
    });
    let advanced_brush = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer textured dry brush shader"),
        source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
            include_str!("advanced_brush.wgsl"),
            include_str!("brush_coverage.wgsl"),
        ])),
    });
    let material_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer destination brush shader"),
        source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
            include_str!("material_brush.wgsl"),
            include_str!("brush_coverage.wgsl"),
        ])),
    });
    let stroke_edge_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer post-stroke edge shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("stroke_edge.wgsl").into()),
    });
    let watercolor_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer live watercolor composite shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("watercolor_composite.wgsl").into()),
    });
    let watercolor_transport_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer watercolor capillary transport shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("watercolor_transport.wgsl").into()),
    });
    let export_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("layer export shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("export.wgsl").into()),
    });
    let analytic_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer analytic brush pipeline layout"),
        bind_group_layouts: &[Some(layouts.style), Some(layouts.target)],
        immediate_size: 0,
    });
    let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer mask brush pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(layouts.target),
            Some(layouts.texture),
        ],
        immediate_size: 0,
    });
    let advanced_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer textured brush pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(layouts.target),
            Some(layouts.advanced_texture),
        ],
        immediate_size: 0,
    });
    let material_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer destination brush pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(layouts.target),
            Some(layouts.material),
            Some(layouts.advanced_texture),
        ],
        immediate_size: 0,
    });
    let edge_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer post-stroke edge pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(layouts.target),
            Some(layouts.edge),
        ],
        immediate_size: 0,
    });
    let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer composite pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(layouts.texture),
            Some(layouts.target),
        ],
        immediate_size: 0,
    });
    let watercolor_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layer live watercolor composite pipeline layout"),
            bind_group_layouts: &[
                Some(layouts.style),
                Some(layouts.target),
                Some(layouts.watercolor),
            ],
            immediate_size: 0,
        });
    let watercolor_transport_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("layer watercolor transport pipeline layout"),
            bind_group_layouts: &[
                Some(layouts.style),
                Some(layouts.target),
                Some(layouts.transport),
                Some(layouts.texture),
            ],
            immediate_size: 0,
        });
    // Older WebGPU implementations reject null slots in pipeline layouts.
    // Keep the shared shader's group numbering, with an explicit empty group.
    let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("layer background empty layout"),
        entries: &[],
    });
    let background_empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("layer background empty binding"),
        layout: &empty_layout,
        entries: &[],
    });
    let background_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer background pipeline layout"),
        bind_group_layouts: &[
            Some(layouts.style),
            Some(&empty_layout),
            Some(layouts.target),
        ],
        immediate_size: 0,
    });
    let export_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("layer export pipeline layout"),
        bind_group_layouts: &[Some(layouts.texture)],
        immediate_size: 0,
    });
    let paint_blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let erase_blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let direct = [
        (
            &analytic_layout,
            &brush,
            "analytic_fragment",
            paint_blend,
            "layer analytic paint",
        ),
        (
            &analytic_layout,
            &brush,
            "analytic_fragment",
            erase_blend,
            "layer analytic erase",
        ),
        (
            &mask_layout,
            &brush,
            "mask_fragment",
            paint_blend,
            "layer mask paint",
        ),
        (
            &mask_layout,
            &brush,
            "mask_fragment",
            erase_blend,
            "layer mask erase",
        ),
        (
            &advanced_layout,
            &advanced_brush,
            "fragment_main",
            paint_blend,
            "layer textured paint",
        ),
        (
            &advanced_layout,
            &advanced_brush,
            "fragment_main",
            erase_blend,
            "layer textured erase",
        ),
    ]
    .map(|(layout, shader, entry, blend, label)| {
        brush_pipeline(device, layout, shader, entry, blend, label)
    });
    let max_blend = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Max,
        },
    };
    let color_target = Some(wgpu::ColorTargetState {
        format: COLOR_FORMAT,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    });
    let coverage_target = Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::R8Unorm,
        blend: None,
        write_mask: wgpu::ColorWrites::RED,
    });
    let wetness_target = Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::R8Unorm,
        blend: Some(max_blend),
        write_mask: wgpu::ColorWrites::RED,
    });
    let watercolor_wetness_target = Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::R8Unorm,
        // All microbatches in one submitted update write the same destination
        // surface while the source remains the immutable pre-update snapshot.
        blend: Some(max_blend),
        write_mask: wgpu::ColorWrites::RED,
    });
    let material = [
        (
            [color_target.clone(), None, None],
            "layer destination brush color",
        ),
        (
            [color_target.clone(), coverage_target.clone(), None],
            "layer destination brush with stroke coverage",
        ),
        (
            [color_target.clone(), None, wetness_target.clone()],
            "layer destination brush with canvas material",
        ),
        (
            [
                color_target.clone(),
                coverage_target.clone(),
                wetness_target,
            ],
            "layer destination brush with paint state",
        ),
        (
            [
                color_target,
                coverage_target.clone(),
                watercolor_wetness_target,
            ],
            "layer watercolor brush with wetness state",
        ),
    ]
    .map(|(targets, label)| {
        fullscreen_pipeline_targets(
            device,
            &material_pipeline_layout,
            &material_shader,
            "fragment_main",
            &targets,
            label,
        )
    });
    let watercolor_transport = std::array::from_fn(|step| {
        fullscreen_pipeline_targets_with_constants(
            device,
            &watercolor_transport_layout,
            &watercolor_transport_shader,
            "fragment_main",
            &[
                Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::R8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::RED,
                }),
            ],
            &[("TRANSPORT_PASS", step as f64)],
            "layer nonlinear watercolor capillary relaxation",
        )
    });
    let reservoir = fullscreen_pipeline(
        device,
        &material_pipeline_layout,
        &material_shader,
        "reservoir_fragment",
        None,
        COLOR_FORMAT,
        "layer brush reservoir exchange",
    );
    let stroke_edge = fullscreen_pipeline(
        device,
        &edge_pipeline_layout,
        &stroke_edge_shader,
        "fragment_main",
        None,
        COLOR_FORMAT,
        "layer post-stroke edge",
    );
    let background = fullscreen_pipeline(
        device,
        &background_layout,
        &composite_shader,
        "background_fragment",
        None,
        COLOR_FORMAT,
        "layer background",
    );
    let composite = fullscreen_pipeline(
        device,
        &composite_layout,
        &composite_shader,
        "layer_fragment",
        Some(paint_blend),
        COLOR_FORMAT,
        "layer composition",
    );
    let watercolor_composite = fullscreen_pipeline(
        device,
        &watercolor_pipeline_layout,
        &watercolor_shader,
        "fragment_main",
        Some(paint_blend),
        COLOR_FORMAT,
        "layer live watercolor composition",
    );
    let export = fullscreen_pipeline(
        device,
        &export_layout,
        &export_shader,
        "fragment_main",
        None,
        EXPORT_FORMAT,
        "layer sRGB export",
    );
    Pipelines {
        direct,
        material,
        watercolor_transport,
        reservoir,
        stroke_edge,
        background,
        background_empty,
        composite,
        watercolor_composite,
        export,
    }
}

fn brush_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: wgpu::BlendState,
    label: &'static str,
) -> wgpu::RenderPipeline {
    brush_pipeline_format(
        device,
        layout,
        shader,
        fragment_entry,
        blend,
        COLOR_FORMAT,
        label,
    )
}

fn brush_pipeline_format(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: wgpu::BlendState,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> wgpu::RenderPipeline {
    const ATTRIBUTES: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2,
        4 => Float32x4, 5 => Float32x2, 6 => Float32x2, 7 => Float32x4
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: mem::size_of::<Dab>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &ATTRIBUTES,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: Option<wgpu::BlendState>,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn fullscreen_pipeline_targets(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    targets: &[Option<wgpu::ColorTargetState>],
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn fullscreen_pipeline_targets_with_constants(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    targets: &[Option<wgpu::ColorTargetState>],
    constants: &[(&str, f64)],
    label: &'static str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
            targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn align_up(value: u32, alignment: u32) -> u32 {
    value.div_ceil(alignment) * alignment
}

fn style_bytes(style: &StyleGpu) -> &[u8] {
    // SAFETY: StyleGpu is repr(C), contains only four-byte scalar arrays, and
    // has no padding.
    unsafe {
        std::slice::from_raw_parts(
            (style as *const StyleGpu).cast::<u8>(),
            mem::size_of::<StyleGpu>(),
        )
    }
}

fn target_bytes(target: &TargetGpu) -> &[u8] {
    // SAFETY: TargetGpu is repr(C), contains only f32 arrays, and has no padding.
    unsafe {
        std::slice::from_raw_parts(
            (target as *const TargetGpu).cast::<u8>(),
            mem::size_of::<TargetGpu>(),
        )
    }
}

fn dab_bytes(dabs: &[Dab]) -> &[u8] {
    // SAFETY: Dab is repr(C), exactly 80 bytes, has alignment four, and contains
    // only f32-based fields. layer-render has a layout test guarding this ABI.
    unsafe { std::slice::from_raw_parts(dabs.as_ptr().cast::<u8>(), mem::size_of_val(dabs)) }
}

fn parse_ascii_pgm(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut tokens = text
        .lines()
        .flat_map(|line| line.split('#').next().unwrap_or("").split_whitespace());
    if tokens.next()? != "P2" {
        return None;
    }
    let width: u32 = tokens.next()?.parse().ok()?;
    let height: u32 = tokens.next()?.parse().ok()?;
    let max: u32 = tokens.next()?.parse().ok()?;
    if width == 0 || height == 0 || max == 0 {
        return None;
    }
    let mut pixels = Vec::with_capacity(width as usize * height as usize);
    for _ in 0..width as usize * height as usize {
        let value: u32 = tokens.next()?.parse().ok()?;
        pixels.push(((value.min(max) * 255 + max / 2) / max) as u8);
    }
    Some((width, height, pixels))
}

fn procedural_paper_grain() -> Vec<u8> {
    procedural_grain(|x, y| {
        let coarse = periodic_value_noise(x, y, 8, 8, 0x7a31_24ed);
        let medium = periodic_value_noise(x, y, 24, 24, 0x93d8_5f17);
        let fine = periodic_value_noise(x, y, 64, 64, 0xc142_9b63);
        let fibers = periodic_value_noise(x, y, 48, 12, 0x5f6a_d209);
        (0.18 + 0.82 * (coarse * 0.18 + medium * 0.34 + fine * 0.32 + fibers * 0.16))
            .clamp(0.0, 1.0)
    })
}

fn procedural_bristle_grain() -> Vec<u8> {
    procedural_grain(|x, y| {
        // Anisotropic periodic noise makes continuous bristle channels without
        // embedding the circular silhouette of a brush tip in the grain.
        let broad = periodic_value_noise(x, y, 20, 3, 0x19ab_74c5);
        let bristles = periodic_value_noise(x, y, 96, 4, 0xe371_2da9);
        let breakup = periodic_value_noise(x, y, 40, 18, 0x48f2_c617);
        (0.10 + 0.90 * (broad * 0.28 + bristles * 0.56 + breakup * 0.16)).clamp(0.0, 1.0)
    })
}

fn procedural_watercolor_tip() -> Vec<u8> {
    procedural_grain(|x, y| {
        let centered_x = x * 2.0 - 1.0;
        let centered_y = y * 2.0 - 1.0;
        let radius = centered_x.hypot(centered_y);
        let angle = centered_y.atan2(centered_x);
        // Broad deterministic lobes make an artist-legible, roughly round
        // silhouette. Low-frequency mottling varies pigment and water load
        // without the repeated high-frequency lines that expose dab cadence.
        let boundary = 0.91
            + 0.032 * (angle * 5.0 + 0.7).sin()
            + 0.021 * (angle * 9.0 - 1.2).sin()
            + 0.013 * (angle * 17.0 + 2.1).sin();
        let silhouette = ((boundary - radius) / 0.045 + 0.5).clamp(0.0, 1.0);
        let coarse = periodic_value_noise(x, y, 5, 5, 0x63ab_7241);
        let broad = periodic_value_noise(x, y, 11, 9, 0xa714_2fd3);
        silhouette * (0.68 + 0.22 * coarse + 0.10 * broad)
    })
}

#[derive(Clone, Copy)]
enum TransportFieldKind {
    LongNarrow,
    LongBroad,
    ShortNarrow,
    ShortBroad,
}

fn procedural_transport_field(kind: TransportFieldKind) -> Vec<u8> {
    let (coarse_cells, ridge_width) = match kind {
        TransportFieldKind::LongNarrow => (4, 0.060),
        TransportFieldKind::LongBroad => (4, 0.125),
        TransportFieldKind::ShortNarrow => (8, 0.055),
        TransportFieldKind::ShortBroad => (8, 0.115),
    };
    procedural_grain(|x, y| {
        // A continuously warped Voronoi boundary is a connected capillary
        // graph on the tiled plane. Smaller independent graphs add tributaries
        // without breaking the coarse paths, producing a multi-scale web rather
        // than the disconnected contour loops of a scalar noise threshold.
        let warp_x = (periodic_value_noise(x, y, 3, 3, 0x315b_49a7) - 0.5) * 0.18;
        let warp_y = (periodic_value_noise(x, y, 3, 3, 0x5f23_c871) - 0.5) * 0.18;
        let warped_x = (x + warp_x).rem_euclid(1.0);
        let warped_y = (y + warp_y).rem_euclid(1.0);

        let coarse =
            periodic_voronoi_web(warped_x, warped_y, coarse_cells, ridge_width, 0x72d9_184b);
        let middle = periodic_voronoi_web(
            warped_x,
            warped_y,
            coarse_cells * 2,
            ridge_width * 0.82,
            0xa197_4d2b,
        ) * 0.62;
        let fine = periodic_voronoi_web(
            warped_x,
            warped_y,
            coarse_cells * 4,
            ridge_width * 0.70,
            0x81d3_4b27,
        ) * 0.34;
        let web = coarse.max(middle).max(fine);
        let paper_breakup = 0.84 + 0.16 * periodic_value_noise(x, y, 32, 32, 0xc4e1_75a9);
        (0.01 + 0.99 * web * paper_breakup).clamp(0.0, 1.0)
    })
}

fn periodic_voronoi_web(
    normalized_x: f32,
    normalized_y: f32,
    cells: u32,
    ridge_width: f32,
    seed: u32,
) -> f32 {
    let grid_x = normalized_x * cells as f32;
    let grid_y = normalized_y * cells as f32;
    let base_x = grid_x.floor() as i32;
    let base_y = grid_y.floor() as i32;
    let mut nearest_squared = f32::INFINITY;
    let mut second_squared = f32::INFINITY;
    for offset_y in -1..=1 {
        for offset_x in -1..=1 {
            let cell_x = (base_x + offset_x).rem_euclid(cells as i32) as u32;
            let cell_y = (base_y + offset_y).rem_euclid(cells as i32) as u32;
            let jitter_x = 0.12 + 0.76 * lattice_noise(cell_x, cell_y, seed);
            let jitter_y = 0.12 + 0.76 * lattice_noise(cell_x, cell_y, seed ^ 0x9e37_79b9);
            let feature_x = (base_x + offset_x) as f32 + jitter_x;
            let feature_y = (base_y + offset_y) as f32 + jitter_y;
            let delta_x = grid_x - feature_x;
            let delta_y = grid_y - feature_y;
            let distance_squared = delta_x * delta_x + delta_y * delta_y;
            if distance_squared < nearest_squared {
                second_squared = nearest_squared;
                nearest_squared = distance_squared;
            } else if distance_squared < second_squared {
                second_squared = distance_squared;
            }
        }
    }
    let border_distance = second_squared.sqrt() - nearest_squared.sqrt();
    (-(border_distance / ridge_width).powi(2)).exp()
}

fn procedural_grain(mut sample: impl FnMut(f32, f32) -> f32) -> Vec<u8> {
    let size = PROCEDURAL_GRAIN_SIZE as usize;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let normalized_x = (x as f32 + 0.5) / PROCEDURAL_GRAIN_SIZE as f32;
            let normalized_y = (y as f32 + 0.5) / PROCEDURAL_GRAIN_SIZE as f32;
            pixels.push((sample(normalized_x, normalized_y) * 255.0).round() as u8);
        }
    }
    pixels
}

fn periodic_value_noise(
    normalized_x: f32,
    normalized_y: f32,
    cells_x: u32,
    cells_y: u32,
    seed: u32,
) -> f32 {
    let grid_x = normalized_x * cells_x as f32;
    let grid_y = normalized_y * cells_y as f32;
    let x0 = grid_x.floor() as u32 % cells_x;
    let y0 = grid_y.floor() as u32 % cells_y;
    let x1 = (x0 + 1) % cells_x;
    let y1 = (y0 + 1) % cells_y;
    let fraction_x = grid_x.fract();
    let fraction_y = grid_y.fract();
    let smooth_x = fraction_x * fraction_x * (3.0 - 2.0 * fraction_x);
    let smooth_y = fraction_y * fraction_y * (3.0 - 2.0 * fraction_y);
    let top = lattice_noise(x0, y0, seed)
        + (lattice_noise(x1, y0, seed) - lattice_noise(x0, y0, seed)) * smooth_x;
    let bottom = lattice_noise(x0, y1, seed)
        + (lattice_noise(x1, y1, seed) - lattice_noise(x0, y1, seed)) * smooth_x;
    top + (bottom - top) * smooth_y
}

fn lattice_noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut value = x
        .wrapping_mul(0x9e37_79b9)
        .wrapping_add(y.wrapping_mul(0x85eb_ca6b))
        ^ seed;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^= value >> 16;
    (value & 0x00ff_ffff) as f32 / 0x00ff_ffff_u32 as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixtures() -> &'static [layer_core::EffectDefinition] {
        layer_core::bundled_effect_catalog().filters()
    }
    pub(super) fn fixture(id: &str) -> &'static layer_core::EffectDefinition {
        layer_core::bundled_effect_catalog().get(id).unwrap()
    }
    mod adjustments;
    mod filter_library;
    use layer_core::{
        BrushDeform, BrushGrain, BrushRendering, BrushTransport, BrushWetMix, DualBrush, Point,
        Rect, WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
    };
    use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};
    use std::sync::Arc;

    fn test_view() -> ViewState {
        ViewState {
            width_px: 128,
            height_px: 128,
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            background_rgba_linear: [1.0, 1.0, 1.0, 1.0],
        }
    }

    fn test_dab(center: [f32; 2], color: [f32; 4], flow: f32) -> Dab {
        Dab {
            center: Point {
                x: center[0],
                y: center[1],
            },
            radii: [20.0, 20.0],
            rotation: [1.0, 0.0],
            motion: [0.0, 0.0],
            color_rgba_linear: color,
            flow,
            hardness: 1.0,
            texture_sign: [1.0, 1.0],
            material: [0.0, 0.0, 1.0, 0.0],
        }
    }

    fn test_style(execution: BrushExecution) -> DabStyle {
        DabStyle {
            alpha_locked: false,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution,
            grain: None,
            dual: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
        }
    }

    #[test]
    fn thumbnails_frame_nontransparent_pixels_and_show_paper_and_checkerboard() {
        let mut r = WgpuRasterizer::new().expect("physical GPU required");
        let mut paper = Layer::paint(LayerId(2), "Paper");
        paper.kind = LayerKind::Background;
        let paint = Layer::paint(LayerId(1), "Small mark");
        let mut layers = vec![paint, paper];
        let mut dab = test_dab([40., 100.], [1., 0., 0., 1.], 1.);
        dab.radii = [3., 6.];
        let mut batch = DabBatch {
            stroke_id: StrokeId(1),
            layer_id: LayerId(1),
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point { x: 32., y: 92. },
                max: Point { x: 48., y: 108. },
            },
        };
        let frame =
            |r: &mut WgpuRasterizer, layers: &[Layer], dabs: &[Dab], batches: &[DabBatch]| {
                r.submit(FramePacket {
                    view: test_view(),
                    document_extent: [128, 128],
                    layers,
                    dabs,
                    dab_batches: batches,
                    reset_layers: false,
                    time_seconds: 0.,
                    composite_all: true,
                })
                .unwrap();
            };
        let preview = |r: &mut WgpuRasterizer, id| {
            r.request_thumbnail(7, LayerId(id)).unwrap();
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            r.take_thumbnail().expect("mapped thumbnail").unwrap().bytes
        };
        frame(&mut r, &layers, &[dab], std::slice::from_ref(&batch));
        let image = preview(&mut r, 1);
        let red = image
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] < 100)
            .count();
        assert!(
            red > 150,
            "small antialiased 6x12 mark must fill the preview, got {red} pixels"
        );
        let colored_rows: Vec<_> = image
            .chunks_exact(32 * 4)
            .enumerate()
            .filter(|(_, row)| row.chunks_exact(4).any(|p| p[0] > p[1].saturating_add(5)))
            .map(|(y, _)| y)
            .collect();
        assert!(
            colored_rows.last().unwrap() - colored_rows.first().unwrap() >= 27,
            "crop retains antialias fringe and fills height"
        );
        assert!(image.chunks_exact(4).all(|p| p[3] == 255));
        assert_ne!(
            image[0],
            image[4 * 4],
            "checker squares remain visible beside cropped mark"
        );
        assert!(
            preview(&mut r, 2)
                .chunks_exact(4)
                .all(|p| p[..3] == [255; 3])
        );
        layers[1].opacity = 0.;
        frame(&mut r, &layers, &[], &[]);
        let transparent_paper = preview(&mut r, 2);
        assert_ne!(transparent_paper[0], transparent_paper[4 * 4]);
        assert!(
            transparent_paper
                .chunks_exact(4)
                .all(|p| p[0] == p[1] && p[1] == p[2])
        );
        // Previously allocated pages must not contribute empty bounds after erase.
        batch.style.mode = DabMode::Erase;
        dab.radii = [10., 10.];
        frame(&mut r, &layers, &[dab], &[batch]);
        let empty = preview(&mut r, 1);
        assert_eq!(empty, transparent_paper);
    }

    #[test]
    fn layer_system_selection_mask_and_incremental_erase() {
        let mut r = WgpuRasterizer::new().expect("physical GPU required");
        let mut layer = Layer::paint(LayerId(1), "Masked red");
        let mut mask = layer_core::LayerMask::reveal_all(LayerId(3), Point::default());
        mask.default_coverage = 0.;
        mask.initial = Some(
            layer_core::Selection::polygon(vec![
                Point { x: 0., y: 0. },
                Point { x: 64., y: 0. },
                Point { x: 64., y: 128. },
                Point { x: 0., y: 128. },
            ])
            .unwrap(),
        );
        layer.mask = Some(mask);
        let mut dab = test_dab([64., 64.], [1., 0., 0., 1.], 1.);
        dab.radii = [58., 58.];
        let mut batch = DabBatch {
            stroke_id: StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point { x: 0., y: 0. },
                max: Point { x: 128., y: 128. },
            },
        };
        r.submit(FramePacket {
            view: test_view(),
            document_extent: [128, 128],
            layers: std::slice::from_ref(&layer),
            dabs: std::slice::from_ref(&dab),
            dab_batches: std::slice::from_ref(&batch),
            reset_layers: true,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        let mut image = vec![0; 128 * 128 * 4];
        r.copy_rgba8_srgb(&mut image, 512).unwrap();
        assert!(
            image[(64 * 128 + 40) * 4 + 1] < 5,
            "selected half reveals red"
        );
        assert!(
            image[(64 * 128 + 90) * 4 + 1] > 250,
            "outside selection is hidden"
        );
        batch.layer_id = LayerId(3);
        batch.style.mode = DabMode::Erase;
        dab.center = Point { x: 40., y: 64. };
        dab.radii = [10., 10.];
        dab.color_rgba_linear = [1.; 4];
        r.submit(FramePacket {
            view: test_view(),
            document_extent: [128, 128],
            layers: std::slice::from_ref(&layer),
            dabs: std::slice::from_ref(&dab),
            dab_batches: std::slice::from_ref(&batch),
            reset_layers: false,
            time_seconds: 0.,
            composite_all: false,
        })
        .unwrap();
        r.copy_rgba8_srgb(&mut image, 512).unwrap();
        assert!(
            image[(64 * 128 + 40) * 4 + 1] > 250,
            "erase on mask hides without erasing paint"
        );
        layer.mask = None;
        r.submit(FramePacket {
            view: test_view(),
            document_extent: [128, 128],
            layers: std::slice::from_ref(&layer),
            dabs: &[],
            dab_batches: &[],
            reset_layers: false,
            time_seconds: 0.,
            composite_all: true,
        })
        .unwrap();
        r.copy_rgba8_srgb(&mut image, 512).unwrap();
        assert!(
            image[(64 * 128 + 40) * 4 + 1] < 5,
            "delete mask restores paint"
        );
    }

    #[test]
    fn brush_pass_plan_is_the_single_style_classifier() {
        let dry = test_style(BrushExecution::Dry);
        let dry_plan = BrushPassPlan::for_style(&dry);
        assert_eq!(dry_plan.direct, Some(DirectPipelineKind::AnalyticPaint));
        assert_eq!(dry_plan.material, MaterialOperation::Deposit);
        assert!(!dry_plan.uses_paint_state());

        let mut textured = dry.clone();
        textured.rendering.alpha_threshold = 0.25;
        assert_eq!(
            BrushPassPlan::for_style(&textured).direct,
            Some(DirectPipelineKind::TexturedPaint)
        );

        let mut uniform = dry.clone();
        uniform.rendering.accumulation = BrushAccumulation::Uniform;
        let uniform_plan = BrushPassPlan::for_style(&uniform);
        assert!(uniform_plan.requires_destination());
        assert_eq!(uniform_plan.material, MaterialOperation::Coverage);
        assert!(uniform_plan.state.coverage);

        let mut wet = test_style(BrushExecution::Wet);
        wet.wet_mix.wetness = 1.0;
        let wet_plan = BrushPassPlan::for_style(&wet);
        assert_eq!(wet_plan.material, MaterialOperation::Wet);
        assert!(wet_plan.reservoir);
        assert!(wet_plan.state.canvas_wetness);

        let watercolor_plan = BrushPassPlan::for_style(&test_style(BrushExecution::Watercolor));
        assert_eq!(watercolor_plan.material, MaterialOperation::Watercolor);
        assert!(watercolor_plan.state.watercolor_wetness);
    }

    fn pixel(renderer: &mut WgpuRasterizer, x: usize, y: usize) -> [u8; 4] {
        renderer.wait_idle().unwrap();
        let pixels = renderer.readback_srgb_rgba8().unwrap();
        pixels[(y * 128 + x) * 4..][..4].try_into().unwrap()
    }

    fn watercolor_style_with_transport(wet_flow: f32, dry_flow: f32) -> DabStyle {
        let mut style = test_style(BrushExecution::Watercolor);
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.wet_mix.amount_of_paint = 0.78;
        style.wet_mix.density = 0.9;
        style.wet_mix.attack = 0.88;
        style.transport = Some(BrushTransport {
            conductance: AssetId::from(WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET),
            scale: 1.0,
            rotation_radians: 0.0,
            contrast: 0.0,
            wet_flow,
            dry_flow,
            distance: 16.0,
            water_load: 1.0,
        });
        style
    }

    fn transport_samples(existing_wet: bool, wet_flow: f32, dry_flow: f32) -> [[u8; 4]; 2] {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Watercolor");
        if existing_wet {
            let mut base_dab = test_dab([64.0, 64.0], [0.01, 0.05, 0.72, 1.0], 0.72);
            base_dab.radii = [36.0, 36.0];
            let mut base_style = test_style(BrushExecution::Watercolor);
            base_style.rendering.accumulation = BrushAccumulation::Uniform;
            base_style.wet_mix.amount_of_paint = 0.72;
            base_style.wet_mix.density = 0.9;
            base_style.wet_mix.attack = 0.88;
            let base = DabBatch {
                stroke_id: StrokeId(30),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: true,
                stroke_end: true,
                first_dab: 0,
                dab_count: 1,
                style: base_style,
                damage: Rect {
                    min: Point { x: 27.0, y: 27.0 },
                    max: Point { x: 101.0, y: 101.0 },
                },
            };
            renderer
                .submit(FramePacket {
                    view: test_view(),
                    document_extent: [128, 128],
                    layers: std::slice::from_ref(&layer),
                    dabs: std::slice::from_ref(&base_dab),
                    dab_batches: std::slice::from_ref(&base),
                    reset_layers: true,
                    time_seconds: 0.,
                    composite_all: true,
                })
                .unwrap();
        }

        let mut active_dab = test_dab([64.0, 64.0], [0.78, 0.01, 0.005, 1.0], 0.9);
        active_dab.radii = if existing_wet {
            [12.0, 12.0]
        } else {
            [20.0, 20.0]
        };
        let radius = active_dab.radii[0];
        let damage = Rect {
            min: Point {
                x: 64.0 - radius - 1.0,
                y: 64.0 - radius - 1.0,
            },
            max: Point {
                x: 64.0 + radius + 1.0,
                y: 64.0 + radius + 1.0,
            },
        };
        // Two internal batches in one visible update catch accidental
        // reclassification of first-batch deposition as pre-existing water.
        let active_batches = [
            DabBatch {
                stroke_id: StrokeId(31),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: true,
                stroke_end: false,
                first_dab: 0,
                dab_count: 1,
                style: watercolor_style_with_transport(wet_flow, dry_flow),
                damage,
            },
            DabBatch {
                stroke_id: StrokeId(31),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: false,
                stroke_end: true,
                first_dab: 1,
                dab_count: 1,
                style: watercolor_style_with_transport(wet_flow, dry_flow),
                damage,
            },
        ];
        let active_dabs = [active_dab, active_dab];
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: &active_dabs,
                dab_batches: &active_batches,
                reset_layers: !existing_wet,
                time_seconds: 0.,
                composite_all: !existing_wet,
            })
            .unwrap();
        [
            pixel(&mut renderer, if existing_wet { 82 } else { 90 }, 64),
            pixel(&mut renderer, 64, 64),
        ]
    }

    #[test]
    fn gpu_records_match_shader_layouts() {
        assert_eq!(mem::size_of::<Dab>(), 80);
        assert_eq!(mem::size_of::<StyleGpu>(), 256);
        assert_eq!(mem::size_of::<TargetGpu>(), 32);
    }

    #[test]
    fn zero_dab_boundary_preserves_watercolor_update_bounds() {
        let painted = DabBatch {
            stroke_id: StrokeId(4),
            layer_id: LayerId(1),
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style: watercolor_style_with_transport(0.5, 0.2),
            damage: Rect {
                min: Point { x: 20.0, y: 30.0 },
                max: Point { x: 30.0, y: 40.0 },
            },
        };
        let mut later = painted.clone();
        later.stroke_start = false;
        later.first_dab = 1;
        later.damage = Rect {
            min: Point { x: 80.0, y: 70.0 },
            max: Point { x: 90.0, y: 80.0 },
        };
        let mut boundary = later.clone();
        boundary.stroke_start = false;
        boundary.stroke_end = true;
        boundary.dab_count = 0;
        boundary.damage = Rect::EMPTY;
        let batches = [painted, later, boundary];
        assert!(is_first_watercolor_update_batch(&batches, 0));
        assert!(!is_last_watercolor_update_batch(&batches, 0));
        assert!(is_last_watercolor_update_batch(&batches, 1));
        let combined = watercolor_update_damages(&batches, 1, [128, 128])
            .into_iter()
            .fold(PixelRect::EMPTY, PixelRect::union);
        assert_eq!(
            combined,
            PixelRect {
                min_x: 4,
                min_y: 14,
                max_x: 106,
                max_y: 96,
            }
        );
    }

    #[test]
    fn uniform_coverage_persists_across_frames_and_resets_per_stroke() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let dab = test_dab([64.0, 64.0], [0.0, 0.0, 0.0, 1.0], 0.45);
        let mut style = test_style(BrushExecution::Dry);
        style.rendering.accumulation = BrushAccumulation::Uniform;
        let mut batch = DabBatch {
            stroke_id: StrokeId(7),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 43.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        let once = pixel(&mut renderer, 64, 64);

        batch.stroke_start = false;
        batch.stroke_end = true;
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        let repeated = pixel(&mut renderer, 64, 64);
        assert!(
            once[0].abs_diff(repeated[0]) <= 2,
            "{once:?} vs {repeated:?}"
        );

        batch.stroke_id = StrokeId(8);
        batch.stroke_start = true;
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        let next_stroke = pixel(&mut renderer, 64, 64);
        assert!(
            next_stroke[0] + 20 < repeated[0],
            "{repeated:?} vs {next_stroke:?}"
        );
        assert_eq!(renderer.metrics().coverage_pages, 1);
    }

    #[test]
    fn watercolor_uses_coverage_and_wetness_without_pen_up_change() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Watercolor");
        let dab = test_dab([64.0, 64.0], [0.55, 0.02, 0.01, 1.0], 0.68);
        let mut style = test_style(BrushExecution::Watercolor);
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.rendering.wet_edge = 0.8;
        style.rendering.burnt_edge = 0.25;
        style.rendering.edge_width = 4.0;
        style.wet_mix.amount_of_paint = 0.7;
        style.wet_mix.density = 0.9;
        style.wet_mix.attack = 0.8;
        style.wet_mix.pull = 0.4;
        style.wet_mix.dilution = 0.3;
        style.wet_mix.mix_space = ColorMixSpace::Oklab;
        let mut batch = DabBatch {
            stroke_id: StrokeId(17),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 43.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        let before_pen_up = pixel(&mut renderer, 64, 64);
        assert_eq!(renderer.metrics().coverage_pages, 1);
        assert_eq!(renderer.metrics().material_pages, 1);
        assert_eq!(renderer.paint_layers[0].watercolor_wetness_pages.len(), 1);
        assert!(renderer.paint_layers[0].watercolor.is_some());

        batch.stroke_start = false;
        batch.stroke_end = true;
        batch.dab_count = 0;
        batch.damage = Rect::EMPTY;
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: &[],
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        assert_eq!(before_pen_up, pixel(&mut renderer, 64, 64));
    }

    #[test]
    fn watercolor_transport_selects_independent_wet_and_dry_rates() {
        let [dry_control, dry_control_center] = transport_samples(false, 1.0, 0.0);
        let [dry_transport, dry_transport_center] = transport_samples(false, 0.0, 1.0);
        assert!(
            u16::from(dry_transport[1]) + 8 < u16::from(dry_control[1]),
            "dry flow did not carry pigment: {dry_control:?} vs {dry_transport:?}"
        );
        let center_difference = dry_control_center
            .iter()
            .zip(dry_transport_center)
            .map(|(before, after)| before.abs_diff(after) as u32)
            .sum::<u32>();
        assert!(
            center_difference <= 8,
            "dry bleed hollowed its source: {dry_control_center:?} vs {dry_transport_center:?}"
        );

        let [wet_control, _] = transport_samples(true, 0.0, 1.0);
        let [wet_transport, _] = transport_samples(true, 1.0, 0.0);
        let wet_difference = wet_control
            .iter()
            .zip(wet_transport)
            .map(|(before, after)| before.abs_diff(after) as u32)
            .sum::<u32>();
        assert!(
            wet_difference > 12,
            "wet flow did not mix pigment: {wet_control:?} vs {wet_transport:?}"
        );
    }

    #[test]
    fn watercolor_preview_microbatches_share_private_coverage() {
        fn preview_pixel(batch_count: usize) -> [u8; 4] {
            let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
            renderer.resize_surface(128, 128).unwrap();
            let layer = Layer::paint(LayerId(1), "Watercolor");
            let dab = test_dab([64.0, 64.0], [0.05, 0.12, 0.7, 1.0], 0.62);
            let mut style = test_style(BrushExecution::Watercolor);
            style.rendering.accumulation = BrushAccumulation::Uniform;
            style.rendering.wet_edge = 0.7;
            style.rendering.burnt_edge = 0.2;
            style.rendering.edge_width = 4.0;
            style.wet_mix.amount_of_paint = 0.7;
            style.wet_mix.density = 0.9;
            style.wet_mix.attack = 0.8;
            style.wet_mix.dilution = 0.3;
            let batches = (0..batch_count)
                .map(|index| DabBatch {
                    stroke_id: StrokeId(23),
                    layer_id: layer.id,
                    kind: DabBatchKind::Preview,
                    stroke_start: index == 0,
                    stroke_end: false,
                    first_dab: index as u32,
                    dab_count: 1,
                    style: style.clone(),
                    damage: Rect {
                        min: Point { x: 43.0, y: 43.0 },
                        max: Point { x: 85.0, y: 85.0 },
                    },
                })
                .collect::<Vec<_>>();
            let dabs = vec![dab; batch_count];
            renderer
                .submit(FramePacket {
                    view: test_view(),
                    document_extent: [128, 128],
                    layers: std::slice::from_ref(&layer),
                    dabs: &dabs,
                    dab_batches: &batches,
                    reset_layers: true,
                    time_seconds: 0.,
                    composite_all: true,
                })
                .unwrap();
            assert!(!renderer.preview_coverage_pages.is_empty());
            assert!(!renderer.preview_watercolor_wetness_pages.is_empty());
            pixel(&mut renderer, 64, 64)
        }

        let once = preview_pixel(1);
        let repeated = preview_pixel(2);
        assert!(
            once.iter()
                .zip(repeated)
                .all(|(left, right)| left.abs_diff(right) <= 2),
            "{once:?} vs {repeated:?}"
        );
    }

    #[test]
    fn watercolor_density_is_independent_of_fringe_contact_count() {
        fn render(include_fringe: bool) -> [u8; 4] {
            let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
            renderer.resize_surface(128, 128).unwrap();
            let layer = Layer::paint(LayerId(1), "Watercolor");
            let mut style = test_style(BrushExecution::Watercolor);
            style.tip = BrushTip::Mask(AssetId::from(WATERCOLOR_TIP_TEXTURE_ASSET));
            style.rendering.accumulation = BrushAccumulation::Uniform;
            style.wet_mix.amount_of_paint = 0.7;
            style.wet_mix.density = 0.9;
            style.wet_mix.attack = 0.8;
            let fringe = test_dab([64.0, 64.0], [0.55, 0.02, 0.01, 1.0], 0.7);
            let solid = test_dab([65.0, 64.0], [0.55, 0.02, 0.01, 1.0], 0.7);

            if include_fringe {
                let first = DabBatch {
                    stroke_id: StrokeId(31),
                    layer_id: layer.id,
                    kind: DabBatchKind::Persistent,
                    stroke_start: true,
                    stroke_end: false,
                    first_dab: 0,
                    dab_count: 1,
                    style: style.clone(),
                    damage: Rect {
                        min: Point { x: 43.0, y: 43.0 },
                        max: Point { x: 85.0, y: 85.0 },
                    },
                };
                renderer
                    .submit(FramePacket {
                        view: test_view(),
                        document_extent: [128, 128],
                        layers: std::slice::from_ref(&layer),
                        dabs: std::slice::from_ref(&fringe),
                        dab_batches: std::slice::from_ref(&first),
                        reset_layers: true,
                        time_seconds: 0.,
                        composite_all: true,
                    })
                    .unwrap();
            }

            let second = DabBatch {
                stroke_id: StrokeId(31),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: !include_fringe,
                stroke_end: true,
                first_dab: 0,
                dab_count: 1,
                style,
                damage: Rect {
                    min: Point { x: 44.0, y: 43.0 },
                    max: Point { x: 86.0, y: 85.0 },
                },
            };
            renderer
                .submit(FramePacket {
                    view: test_view(),
                    document_extent: [128, 128],
                    layers: std::slice::from_ref(&layer),
                    dabs: std::slice::from_ref(&solid),
                    dab_batches: std::slice::from_ref(&second),
                    reset_layers: !include_fringe,
                    time_seconds: 0.,
                    composite_all: !include_fringe,
                })
                .unwrap();
            pixel(&mut renderer, 82, 64)
        }

        let direct = render(false);
        let through_fringe = render(true);
        assert!(
            direct
                .iter()
                .zip(through_fringe)
                .all(|(left, right)| left.abs_diff(right) <= 2),
            "{direct:?} vs {through_fringe:?}"
        );
    }

    #[test]
    fn watercolor_wetness_removes_internal_overlap_edges() {
        fn render(edge_strength: f32) -> ([u8; 4], [u8; 4]) {
            let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
            renderer.resize_surface(128, 128).unwrap();
            let layer = Layer::paint(LayerId(1), "Watercolor");
            let mut style = test_style(BrushExecution::Watercolor);
            style.rendering.accumulation = BrushAccumulation::Uniform;
            style.rendering.wet_edge = edge_strength;
            style.rendering.burnt_edge = edge_strength;
            style.rendering.edge_width = 8.0;
            style.wet_mix.amount_of_paint = 0.7;
            style.wet_mix.density = 0.9;
            style.wet_mix.attack = 0.8;

            let mut first = test_dab([48.0, 64.0], [0.52, 0.03, 0.02, 1.0], 0.62);
            first.radii = [38.0, 38.0];
            let mut second = test_dab([80.0, 64.0], [0.52, 0.03, 0.02, 1.0], 0.62);
            second.radii = [38.0, 38.0];
            for (index, dab) in [first, second].iter().enumerate() {
                let batch = DabBatch {
                    stroke_id: StrokeId(41 + index as u64),
                    layer_id: layer.id,
                    kind: DabBatchKind::Persistent,
                    stroke_start: true,
                    stroke_end: true,
                    first_dab: 0,
                    dab_count: 1,
                    style: style.clone(),
                    damage: Rect {
                        min: Point {
                            x: dab.center.x - 39.0,
                            y: dab.center.y - 39.0,
                        },
                        max: Point {
                            x: dab.center.x + 39.0,
                            y: dab.center.y + 39.0,
                        },
                    },
                };
                renderer
                    .submit(FramePacket {
                        view: test_view(),
                        document_extent: [128, 128],
                        layers: std::slice::from_ref(&layer),
                        dabs: std::slice::from_ref(dab),
                        dab_batches: std::slice::from_ref(&batch),
                        reset_layers: index == 0,
                        time_seconds: 0.,
                        composite_all: index == 0,
                    })
                    .unwrap();
            }
            // x=45 crosses the second stroke's former alpha edge but is more
            // than two edge widths from the combined watercolor boundary.
            (pixel(&mut renderer, 45, 64), pixel(&mut renderer, 12, 64))
        }

        let (neutral_overlap, neutral_outer) = render(0.0);
        let (strong_overlap, strong_outer) = render(1.0);
        assert!(
            neutral_overlap
                .iter()
                .zip(strong_overlap)
                .all(|(left, right)| left.abs_diff(right) <= 2),
            "internal overlap changed: {neutral_overlap:?} vs {strong_overlap:?}"
        );
        assert!(
            strong_outer[..3]
                .iter()
                .map(|value| *value as u16)
                .sum::<u16>()
                < neutral_outer[..3]
                    .iter()
                    .map(|value| *value as u16)
                    .sum::<u16>(),
            "outer edge was not darkened: {neutral_outer:?} vs {strong_outer:?}"
        );
    }

    #[test]
    fn dry_paint_on_a_mixed_layer_is_not_watercolor_wetness() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Mixed media");
        let dry_dab = test_dab([32.0, 64.0], [0.08, 0.02, 0.01, 1.0], 1.0);
        let dry = DabBatch {
            stroke_id: StrokeId(51),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point { x: 11.0, y: 43.0 },
                max: Point { x: 53.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dry_dab),
                dab_batches: std::slice::from_ref(&dry),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        let dry_edge = pixel(&mut renderer, 14, 64);

        let watercolor_dab = test_dab([96.0, 64.0], [0.02, 0.08, 0.62, 1.0], 0.7);
        let mut watercolor_style = test_style(BrushExecution::Watercolor);
        watercolor_style.rendering.accumulation = BrushAccumulation::Uniform;
        watercolor_style.rendering.wet_edge = 1.0;
        watercolor_style.rendering.burnt_edge = 1.0;
        watercolor_style.rendering.edge_width = 8.0;
        watercolor_style.wet_mix.amount_of_paint = 0.7;
        watercolor_style.wet_mix.density = 0.9;
        watercolor_style.wet_mix.attack = 0.8;
        let watercolor = DabBatch {
            stroke_id: StrokeId(52),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: watercolor_style,
            damage: Rect {
                min: Point { x: 75.0, y: 43.0 },
                max: Point { x: 117.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&watercolor_dab),
                dab_batches: std::slice::from_ref(&watercolor),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();

        let recomposited_dry_edge = pixel(&mut renderer, 14, 64);
        assert!(
            dry_edge
                .iter()
                .zip(recomposited_dry_edge)
                .all(|(left, right)| left.abs_diff(right) <= 2),
            "dry paint was treated as watercolor: {dry_edge:?} vs {recomposited_dry_edge:?}"
        );
        assert_eq!(renderer.paint_layers[0].watercolor_wetness_pages.len(), 1);
    }

    #[test]
    fn pure_smudge_does_not_invent_paint_on_an_empty_layer() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let dab = test_dab([64.0, 64.0], [0.8, 0.0, 0.4, 1.0], 1.0);
        let mut style = test_style(BrushExecution::Smudge);
        style.wet_mix.amount_of_paint = 0.0;
        style.wet_mix.attack = 1.0;
        style.wet_mix.pull = 1.0;
        let batch = DabBatch {
            stroke_id: StrokeId(12),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 43.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();

        assert_eq!(pixel(&mut renderer, 64, 64), [255; 4]);
    }

    #[test]
    fn gpu_smudge_composes_batch_motion_without_selected_color() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let red = test_dab([32.0, 64.0], [1.0, 0.0, 0.0, 1.0], 1.0);
        let dry = DabBatch {
            stroke_id: StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point { x: 11.0, y: 43.0 },
                max: Point { x: 53.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&red),
                dab_batches: std::slice::from_ref(&dry),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();

        let mut smudge_style = test_style(BrushExecution::Smudge);
        smudge_style.wet_mix.amount_of_paint = 0.0;
        let selected_green = [0.0, 1.0, 0.0, 1.0];
        let mut first = test_dab([48.0, 64.0], selected_green, 1.0);
        first.motion = [16.0, 0.0];
        first.material[1] = 1.0;
        let mut second = test_dab([64.0, 64.0], selected_green, 1.0);
        second.motion = [16.0, 0.0];
        second.material[1] = 1.0;
        smudge_style.wet_mix.pull = 1.0;
        let smudge = DabBatch {
            stroke_id: StrokeId(2),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 2,
            style: smudge_style,
            damage: Rect {
                min: Point { x: 27.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: &[first, second],
                dab_batches: std::slice::from_ref(&smudge),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        let result = pixel(&mut renderer, 64, 64);
        assert!(
            result[0] > 180 && result[1] < 100 && result[2] < 100,
            "{result:?}"
        );
    }

    #[test]
    fn wet_brush_mixes_loaded_and_destination_color() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let red = test_dab([64.0, 64.0], [1.0, 0.0, 0.0, 1.0], 1.0);
        let dry = DabBatch {
            stroke_id: StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: test_style(BrushExecution::Dry),
            damage: Rect {
                min: Point { x: 43.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&red),
                dab_batches: std::slice::from_ref(&dry),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();

        let blue = test_dab([64.0, 64.0], [0.0, 0.0, 1.0, 1.0], 1.0);
        let mut wet_style = test_style(BrushExecution::Wet);
        wet_style.wet_mix.amount_of_paint = 0.5;
        wet_style.wet_mix.pull = 1.0;
        wet_style.wet_mix.mix_space = ColorMixSpace::Oklab;
        let wet = DabBatch {
            stroke_id: StrokeId(2),
            style: wet_style,
            ..dry.clone()
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&blue),
                dab_batches: std::slice::from_ref(&wet),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();

        let mixed = pixel(&mut renderer, 64, 64);
        assert!(
            mixed[0] > 80 && mixed[2] > 80 && mixed[1] < 120,
            "{mixed:?}"
        );
    }

    #[test]
    fn canvas_material_and_post_stroke_edge_are_lazy_gpu_state() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let dab = test_dab([64.0, 64.0], [0.2, 0.05, 0.01, 1.0], 0.55);
        let mut style = test_style(BrushExecution::Wet);
        style.wet_mix.wetness = 0.8;
        style.rendering.accumulation = BrushAccumulation::Uniform;
        style.rendering.edge_after_stroke = true;
        style.rendering.wet_edge = 0.8;
        style.rendering.burnt_edge = 0.4;
        style.rendering.edge_width = 4.0;
        let mut batch = DabBatch {
            stroke_id: StrokeId(9),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: false,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 43.0, y: 43.0 },
                max: Point { x: 85.0, y: 85.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        let before = pixel(&mut renderer, 80, 64);
        assert_eq!(renderer.metrics().material_pages, 1);
        assert_eq!(renderer.metrics().coverage_pages, 1);
        assert!(renderer.paint_layers[0].pages[0].active_secondary);

        batch.stroke_start = false;
        batch.stroke_end = true;
        batch.first_dab = 0;
        batch.dab_count = 0;
        batch.damage = Rect::EMPTY;
        renderer
            .submit(FramePacket {
                view: test_view(),
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: &[],
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        let after = pixel(&mut renderer, 80, 64);
        assert!(!renderer.paint_layers[0].pages[0].active_secondary);
        assert_ne!(before, after, "post-stroke edge must update visible pixels");
    }

    #[test]
    fn bundled_masks_parse() {
        assert!(
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/pencil-grain.pgm")).is_some()
        );
        assert!(
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/paint-bristles.pgm")).is_some()
        );
    }

    #[test]
    fn procedural_grains_are_full_frame_and_have_useful_range() {
        for grain in [procedural_paper_grain(), procedural_bristle_grain()] {
            assert_eq!(grain.len(), PROCEDURAL_GRAIN_SIZE.pow(2) as usize);
            let minimum = *grain.iter().min().unwrap();
            let maximum = *grain.iter().max().unwrap();
            assert!(minimum > 0, "grain must not contain a tip-mask border");
            assert!(maximum.saturating_sub(minimum) > 80, "{minimum}..{maximum}");
        }
    }

    #[test]
    fn procedural_watercolor_tip_has_varied_interior_and_ragged_border() {
        let tip = procedural_watercolor_tip();
        assert_eq!(tip.len(), PROCEDURAL_GRAIN_SIZE.pow(2) as usize);
        let size = PROCEDURAL_GRAIN_SIZE as usize;
        assert!(tip[(size / 2) * size + size / 2] >= 170);
        assert_eq!(tip[0], 0);

        let mut interior_minimum = u8::MAX;
        let mut interior_maximum = u8::MIN;
        for y in 0..size {
            for x in 0..size {
                let nx = x as f32 / (size - 1) as f32 * 2.0 - 1.0;
                let ny = y as f32 / (size - 1) as f32 * 2.0 - 1.0;
                if nx.hypot(ny) <= 0.72 {
                    interior_minimum = interior_minimum.min(tip[y * size + x]);
                    interior_maximum = interior_maximum.max(tip[y * size + x]);
                }
            }
        }
        assert!(
            interior_minimum >= 170,
            "interior must not contain pinholes"
        );
        assert!(
            interior_maximum.saturating_sub(interior_minimum) >= 25,
            "interior must carry useful low-frequency variation"
        );
        let transition_pixels = tip
            .iter()
            .filter(|value| **value > 0 && **value < 255)
            .count();
        assert!(transition_pixels > size, "ragged edge must be antialiased");
    }

    #[test]
    fn procedural_transport_fields_are_distinct_and_useful() {
        let long_narrow = procedural_transport_field(TransportFieldKind::LongNarrow);
        let long_broad = procedural_transport_field(TransportFieldKind::LongBroad);
        let short_narrow = procedural_transport_field(TransportFieldKind::ShortNarrow);
        let short_broad = procedural_transport_field(TransportFieldKind::ShortBroad);
        for field in [&long_narrow, &long_broad, &short_narrow, &short_broad] {
            assert_eq!(field.len(), PROCEDURAL_GRAIN_SIZE.pow(2) as usize);
            // Broad ridged-noise fibers can leave a small conductance floor;
            // require strong contrast, not a brittle exactly-zero texel.
            assert!(*field.iter().min().unwrap() < 16);
            assert!(*field.iter().max().unwrap() > 180);
        }
        let conductive = |field: &[u8]| field.iter().filter(|value| **value >= 96).count();
        assert!(conductive(&long_broad) > conductive(&long_narrow));
        assert!(conductive(&short_broad) > conductive(&short_narrow));
        assert_ne!(long_narrow, short_narrow);
        assert_ne!(long_broad, short_broad);
    }

    #[test]
    fn textured_dual_brush_executes_on_the_gpu() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Ink");
        let grain = BrushGrain {
            asset: AssetId::from(PENCIL_TEXTURE_ASSET),
            behavior: BrushGrainBehavior::Canvas,
            scale: 2.0,
            depth: 0.8,
            rotation_radians: 0.2,
            offset_jitter: 0.0,
        };
        let dual = DualBrush {
            tip: BrushTip::Mask(AssetId::from(PAINTBRUSH_TEXTURE_ASSET)),
            grain: Some(grain.clone()),
            combine: DualCombineMode::Multiply,
            scale: 0.9,
            aspect: 1.0,
            angle_radians: -0.3,
            offset: [0.05, 0.0],
        };
        let style = DabStyle {
            alpha_locked: false,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution: BrushExecution::Dry,
            grain: Some(grain),
            dual: Some(Arc::new(dual)),
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
        };
        let dab = Dab {
            center: Point { x: 64.0, y: 64.0 },
            radii: [38.0, 30.0],
            rotation: [1.0, 0.0],
            motion: [4.0, 1.0],
            color_rgba_linear: [0.05, 0.1, 0.3, 1.0],
            flow: 1.0,
            hardness: 0.8,
            texture_sign: [1.0, 1.0],
            material: [0.8, 0.0, 1.0, 0.0],
        };
        let batch = DabBatch {
            stroke_id: layer_core::StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style,
            damage: Rect {
                min: Point { x: 24.0, y: 24.0 },
                max: Point { x: 104.0, y: 104.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view: ViewState {
                    width_px: 128,
                    height_px: 128,
                    document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    background_rgba_linear: [1.0, 1.0, 1.0, 1.0],
                },
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dab),
                dab_batches: std::slice::from_ref(&batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        renderer.wait_idle().unwrap();
        assert_eq!(renderer.texture_sets.len(), 1);
        let pixels = renderer.readback_srgb_rgba8().unwrap();
        let painted = pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] < 245 || pixel[1] < 245 || pixel[2] < 245)
            .count();
        assert!(
            painted > 100,
            "advanced pipeline should rasterize textured ink"
        );
    }

    #[test]
    fn smudge_and_liquify_read_prior_gpu_pixels() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(128, 128).unwrap();
        let layer = Layer::paint(LayerId(1), "Paint");
        let view = ViewState {
            width_px: 128,
            height_px: 128,
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            background_rgba_linear: [1.0, 1.0, 1.0, 1.0],
        };

        let dry_dab = Dab {
            center: Point { x: 32.0, y: 64.0 },
            radii: [12.0, 12.0],
            rotation: [1.0, 0.0],
            motion: [0.0, 0.0],
            color_rgba_linear: [1.0, 0.0, 0.0, 1.0],
            flow: 1.0,
            hardness: 1.0,
            texture_sign: [1.0, 1.0],
            material: [0.0, 0.0, 1.0, 0.0],
        };
        let dry_batch = DabBatch {
            stroke_id: layer_core::StrokeId(1),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: DabStyle {
                alpha_locked: false,
                tip: BrushTip::AnalyticEllipse,
                mode: DabMode::Paint,
                execution: BrushExecution::Dry,
                grain: None,
                dual: None,
                rendering: BrushRendering::default(),
                wet_mix: BrushWetMix::default(),
                transport: None,
                deform: BrushDeform::default(),
            },
            damage: Rect {
                min: Point { x: 18.0, y: 50.0 },
                max: Point { x: 46.0, y: 78.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view,
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&dry_dab),
                dab_batches: std::slice::from_ref(&dry_batch),
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();

        let wet_mix = BrushWetMix {
            amount_of_paint: 0.0,
            pull: 1.0,
            ..BrushWetMix::default()
        };
        let smudge_dab = Dab {
            center: Point { x: 64.0, y: 64.0 },
            radii: [16.0, 16.0],
            rotation: [1.0, 0.0],
            motion: [32.0, 0.0],
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            flow: 1.0,
            hardness: 1.0,
            texture_sign: [1.0, 1.0],
            material: [0.0, 1.0, 1.0, 0.0],
        };
        let smudge_batch = DabBatch {
            stroke_id: layer_core::StrokeId(2),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: DabStyle {
                alpha_locked: false,
                tip: BrushTip::AnalyticEllipse,
                mode: DabMode::Paint,
                execution: BrushExecution::Smudge,
                grain: None,
                dual: None,
                rendering: BrushRendering::default(),
                wet_mix,
                transport: None,
                deform: BrushDeform::default(),
            },
            damage: Rect {
                min: Point { x: 47.0, y: 47.0 },
                max: Point { x: 81.0, y: 81.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view,
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&smudge_dab),
                dab_batches: std::slice::from_ref(&smudge_batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        assert!(renderer.paint_layers[0].pages[0].active_secondary);

        let liquify_dab = Dab {
            center: Point { x: 90.0, y: 64.0 },
            radii: [18.0, 18.0],
            rotation: [1.0, 0.0],
            motion: [26.0, 0.0],
            color_rgba_linear: [0.0; 4],
            flow: 1.0,
            hardness: 1.0,
            texture_sign: [1.0, 1.0],
            material: [0.0, 0.0, 0.0, 1.0],
        };
        let liquify_batch = DabBatch {
            stroke_id: layer_core::StrokeId(3),
            layer_id: layer.id,
            kind: DabBatchKind::Persistent,
            stroke_start: true,
            stroke_end: true,
            first_dab: 0,
            dab_count: 1,
            style: DabStyle {
                alpha_locked: false,
                tip: BrushTip::AnalyticEllipse,
                mode: DabMode::Paint,
                execution: BrushExecution::Liquify,
                grain: None,
                dual: None,
                rendering: BrushRendering::default(),
                wet_mix: BrushWetMix::default(),
                transport: None,
                deform: BrushDeform {
                    mode: LiquifyMode::Push,
                    strength: 1.0,
                    ..BrushDeform::default()
                },
            },
            damage: Rect {
                min: Point { x: 71.0, y: 45.0 },
                max: Point { x: 109.0, y: 83.0 },
            },
        };
        renderer
            .submit(FramePacket {
                view,
                document_extent: [128, 128],
                layers: std::slice::from_ref(&layer),
                dabs: std::slice::from_ref(&liquify_dab),
                dab_batches: std::slice::from_ref(&liquify_batch),
                reset_layers: false,
                time_seconds: 0.,
                composite_all: false,
            })
            .unwrap();
        assert!(!renderer.paint_layers[0].pages[0].active_secondary);

        let pixels = renderer.readback_srgb_rgba8().unwrap();
        for x in [64_usize, 90] {
            let pixel = &pixels[(64 * 128 + x) * 4..][..4];
            assert!(
                pixel[0] > 180 && pixel[1] < 100 && pixel[2] < 100,
                "destination brush should move the red source to x={x}: {pixel:?}"
            );
        }
    }

    #[test]
    fn empty_4k_layers_allocate_no_layer_pixels() {
        let mut renderer = WgpuRasterizer::new().expect("physical GPU is required");
        renderer.resize_surface(4096, 4096).unwrap();
        let layers = (1..=128)
            .map(|id| Layer::paint(LayerId(id), format!("Layer {id}")))
            .collect::<Vec<_>>();
        renderer
            .submit(FramePacket {
                view: ViewState {
                    width_px: 4096,
                    height_px: 4096,
                    document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    background_rgba_linear: [1.0, 1.0, 1.0, 1.0],
                },
                document_extent: [4096, 4096],
                layers: &layers,
                dabs: &[],
                dab_batches: &[],
                reset_layers: true,
                time_seconds: 0.,
                composite_all: true,
            })
            .unwrap();
        renderer.wait_idle().unwrap();
        let metrics = renderer.metrics();
        assert_eq!(metrics.paint_pages, 0);
        assert_eq!(metrics.paint_storage_bytes, 0);
        assert_eq!(metrics.preview_storage_bytes, 0);
        assert_eq!(metrics.composite_storage_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn pixel_rect_subtraction_preserves_every_pixel_outside_the_overlap() {
        let outer = PixelRect {
            min_x: 10,
            min_y: 20,
            max_x: 90,
            max_y: 100,
        };
        let overlap = PixelRect {
            min_x: 30,
            min_y: 40,
            max_x: 70,
            max_y: 80,
        };
        let pieces = outer.subtract(overlap);
        assert_eq!(
            pieces.iter().map(|piece| piece.area()).sum::<u64>(),
            outer.area() - overlap.area()
        );
        assert!(
            pieces
                .iter()
                .all(|piece| piece.intersect(overlap).is_empty())
        );
    }
}
