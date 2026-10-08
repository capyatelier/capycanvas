//! GPU-resident implementation of Layer's GPU canvas contract.
//!
//! The current dry brush uses instanced quads and fixed-function blending. All
//! paint-layer and composite pixels remain in GPU textures. Explicit export and
//! bounded asynchronous UI thumbnails and explicit color samples are the only
//! readbacks. Destination-aware brush stages can be added beside this
//! fast path without changing the engine packet or duplicating pixel semantics.

pub mod native_tiles;
pub mod snapshot;
pub mod local_tone;
mod analysis_compute;
mod dehaze;
pub mod effect_analysis;
mod attached;
pub use attached::AttachedRenderer;
mod portable_blend;
mod target_geometry;
mod pixel_rect;
mod submission;
use submission::ColorPass;
use pixel_rect::{DocRect, PixelRect, page_coordinates, page_rect, pixel_rect};

use layer_core::{
    AssetId, BrushAccumulation, BrushBlendMode, BrushExecution,
    BrushGrainBehavior, BrushTip,
    LiquifyMode, PAPER_GRAIN_TEXTURE_ASSET,
    StrokeId, WATERCOLOR_TIP_TEXTURE_ASSET, WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET,
};
use layer_core::authored::{OccurrenceHandle, OccurrenceContent, SourceTarget, EvaluationContext};
use layer_core::{SceneView, SceneSnapshot, SceneScope};
use layer_render::{
    CanvasRenderer, Dab, DabBatch, DabBatchKind, DabMode, FramePacket, HostImage, PixelFormat,
    ReadbackImage,
};
use std::{borrow::Cow, fmt, mem, num::NonZeroU64, sync::{Arc, mpsc}, time::Duration};

mod builtin_masks;
mod bristle_table;
#[cfg(test)]
mod kernel_spirv;
mod color_sample;
mod source_access;
mod material_sources;
mod brush_tiles;
mod dry_material;
mod changed_cells;
mod material_tiles;
mod bindings;
use brush_tiles::BrushTile;
#[cfg(not(target_arch = "wasm32"))]
mod export_readback;
mod view_color;
mod working_color;
pub use view_color::SdrSurfaceColor;
mod raster;
mod deferred;
mod paint_transform;
mod pixel_transform;
mod object_sampling;
mod object_image_mips;
mod moving_projection;
use moving_projection::MovingProjection;
pub use scene::sources::PreparedSourcePixels as PreparedImagePixels;
pub use object_image_mips::prepare_image_tile;
pub use object_sampling::prepare_nearest_coordinates;
#[cfg(target_arch = "wasm32")]
pub type BrowserImageDecoder = std::rc::Rc<dyn Fn(Arc<layer_core::color::source::SourceImage>, [u32; 2], layer_core::color::RgbSpace)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PreparedImagePixels, String>>>>>;
#[cfg(target_arch="wasm32")]
pub type BrowserNearestCoordinateDecoder = std::rc::Rc<dyn Fn([f64;6],[u32;2],u32,u32)
    -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<Arc<Vec<u8>>,String>>>>>;
use builtin_masks::builtin_masks;
use deferred::{Compilation, CompileMode, Deferred};
mod pipeline_device;
use pipeline_device::PipelineDevice;
#[cfg(not(target_arch = "wasm32"))]
mod shader_cache;
mod startup;
pub use startup::{StartupProgress, ShaderDocument};
#[cfg(not(target_arch = "wasm32"))]
pub use startup::finish_shader_compiler_shutdown;
#[cfg(not(target_arch = "wasm32"))]
pub use startup::ShaderActivity;
mod effect_validation;
mod effects;
mod gradient;
mod flood;
mod frame_timing;
mod performance_trace;
mod layer_masks;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod test_support;
mod present;
mod backdrop_blur;
pub use backdrop_blur::{BackdropBlurStyle, BackdropRegion};
mod present_picker;
mod artwork;
mod display_mips;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
mod display_memory;
mod present_damage;
mod present_screen;
mod region_requests;
mod region_sources;
mod retouch_sources;
mod scene;
mod selection_clip;
mod selection_refine;
mod selection_paint;
mod selection_previews;
mod selection_readback;
mod tonal;
mod telemetry;
pub use frame_timing::{GpuFrameSample, GpuFrameTimer, GpuFrameTimingStats};
mod thumbnails;
mod source_thumbnails;
pub use present::{OverviewPlacement, ViewportPresenter};
pub use present_screen::ScreenCheck;

// RGB stores encode(linear RGB * alpha); sampling/blending uses Float32 linear
// premultiplied values. Alpha is ordinary, unencoded UNORM8 coverage.
const EXPORT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const INITIAL_DAB_BYTES: u64 = 4 * 1024 * 1024;
const INITIAL_STYLE_RECORDS: usize = 128;
const INITIAL_TARGET_RECORDS: usize = 257;
const READBACK_TIMEOUT: Duration = Duration::from_secs(30);
const WHITE_MASK_ASSET: &str = "builtin:brush-tip/solid-white-v1";
const PAGE_SIZE: u32 = 256;
const MOVING_IMAGE_IDLE_STEPS: usize = 32;
const MOVING_IMAGE_INPUT_STEPS: usize = 2;
const MOVING_IMAGE_PREFETCH: usize = 18;
// Queries consume each group before its source slots can be reused. This also
// fits the portable sixteen sampled-texture bindings per shader stage.
const SOURCE_SLOTS: usize = 16;
const SCALAR_PAGE_BYTES: u64 = PAGE_SIZE as u64 * PAGE_SIZE as u64;
const RESERVOIR_SIZE: u32 = 64;
const PROCEDURAL_GRAIN_SIZE: u32 = 256;
const WATERCOLOR_TRANSPORT_STEPS: u32 = 3;



/// Uploads are encoded at their point of use on every host. Queue::write_buffer
/// runs before submitted commands, so it cannot replace ordered copies when
/// source decoding, brush neighborhoods and publication reuse uniform offsets.
struct Uploads {
    belt: wgpu::util::StagingBelt,
}
impl Uploads {
    fn new(device: &wgpu::Device, chunk_size: u64) -> Self {
        Self {
            belt: wgpu::util::StagingBelt::new(device.clone(), chunk_size),
        }
    }
    fn write(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Buffer,
        bytes: &[u8],
    ) -> Result<(), GpuRasterError> {
        self.write_at(encoder, target, 0, bytes)
    }
    fn write_at(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Buffer,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), GpuRasterError> {
        self.write_mapped(encoder, target, offset, bytes.len() as u64,
            |mapped| mapped.copy_from_slice(bytes))
    }
    fn write_mapped(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Buffer,
        offset: u64,
        size: u64,
        fill: impl FnOnce(&mut wgpu::BufferViewMut),
    ) -> Result<(), GpuRasterError> {
        let size = wgpu::BufferSize::new(size).ok_or(GpuRasterError::SizeOverflow)?;
        // StagingBelt::write_buffer unwraps mapping failures. Allocate the
        // same reusable slice and let device-loss errors reach the host.
        let slice = self.belt.allocate(
            size,
            wgpu::BufferSize::new(wgpu::COPY_BUFFER_ALIGNMENT).unwrap(),
        );
        {
            let mut mapped = slice.get_mapped_range_mut()
                .map_err(|error| GpuRasterError::MapFailed(format!("upload buffer staging: {error}")))?;
            fill(&mut mapped);
        }
        encoder.copy_buffer_to_buffer(
            slice.buffer(),
            slice.offset(),
            target,
            offset,
            size.get(),
        );
        Ok(())
    }
    /// Write directly into reusable mapped staging storage. The copy stays at
    /// its point of use, before a decoder can reuse the same input texture.
    fn write_texture(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        bytes_per_row: u32,
        fill: impl FnOnce(&mut wgpu::BufferViewMut),
    ) -> Result<(), GpuRasterError> {
        let size = wgpu::BufferSize::new(u64::from(bytes_per_row) * u64::from(texture.height()))
            .ok_or(GpuRasterError::SizeOverflow)?;
        let slice = self.belt.allocate(size,
            wgpu::BufferSize::new(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)).unwrap());
        {
            let mut mapped = slice.get_mapped_range_mut()
                .map_err(|error| GpuRasterError::MapFailed(format!("upload texture staging: {error}")))?;
            fill(&mut mapped);
        }
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: slice.buffer(),
                layout: wgpu::TexelCopyBufferLayout {
                    offset: slice.offset(), bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.as_image_copy(), texture.size(),
        );
        Ok(())
    }
    fn finish(&mut self, encoder: &wgpu::CommandEncoder) {
        self.belt.finish_and_recall_on_submit(encoder);
    }
}

struct DisplayRetirement {
    bytes:u64,
    charge:Arc<std::sync::atomic::AtomicU64>,
    #[cfg(not(target_arch="wasm32"))]
    _backup:scene::scale::DisplayBackup,
}
impl Drop for DisplayRetirement {
    fn drop(&mut self) {self.charge.fetch_sub(self.bytes,std::sync::atomic::Ordering::AcqRel);}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuRasterMetrics {
    pub submissions: u64,
    pub command_passes: u64,
    pub dabs: u64,
    pub raster_candidate_pixels: u64,
    pub composited_pixels: u64,
    /// Texels that display level reductions read: four for each filtered
    /// sample and one for each load.
    pub display_reduction_reads: u64,
    pub frame_composited_pages: Vec<(u32, [u32; 2])>,
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
    pub raster_backing_reserved_bytes: u64,
    /// Peak native source upload charge. Packed imports include CPU/GPU packing
    /// tiles; tiled sources charge in-flight staging here and their fixed GPU
    /// cache in scene storage. Excludes retained source bytes and driver overhead.
    pub source_upload_peak_bytes: u64,
    pub source_upload_submissions: u64,
    pub source_tile_hits: u64,
    pub source_tile_misses: u64,
    pub source_tile_evictions: u64,
    pub moving_image_evictions: u64,
    pub moving_image_request_bytes: u64,
    /// Cumulative output-page gathers/passes for distant material samples.
    /// Storage counts the reusable Float32 fields and both metadata buffers,
    /// excluding source-cache tiles, staging and driver memory.
    pub material_sample_jobs: u64,
    pub material_sample_passes: u64,
    pub material_sample_storage_bytes: u64,
    /// Retouching stroke-start pages, the reference composite cache and its
    /// capture scratch, excluding driver memory.
    pub retouch_storage_bytes: u64,
    /// Latest submitted frame's elapsed CPU phases when telemetry is enabled:
    /// preparation, committed paint, native capture encoding, prediction,
    /// composition, and submission/publication. These include waits.
    pub frame_cpu_ms: [f64; 6],
    /// Material binding elapsed CPU phases for the latest telemetry frame:
    /// source bounds, source preparation, gather bindings, gather encoding,
    /// and the final/nearby material binding. These include waits.
    pub material_cpu_ms: [f64; 5],
    /// Completed intermediate display batches; the final batch stays in the
    /// ordinary frame submission so presentation does not wait on the CPU.
    pub display_composition_submissions: u64,
    /// Successful final submissions of native color/scalar restore batches.
    /// Upload-ceiling drains are counted separately in source_upload_submissions.
    pub native_restore_submissions: u64,
    /// Native physical-filter window execution, separate from source uploads.
    /// Pixel caches include clipping uniforms here, but exclude tile scratch,
    /// paint, the full composite and driver allocations.
    pub image_window_submissions: u64,
    pub image_window_peak_bytes: u64,
    /// Awaited exact object sampling submissions of snapshot captures.
    pub object_drain_submissions: u64,
    pub native_effect_reuse_candidate_cells:u64,
    pub native_effect_forced_cells:u64,
}

pub fn memory_hints() -> wgpu::MemoryHints {
    let blocks = if cfg!(target_os = "android") { (16 << 20)..(16 << 20) } else { (64 << 20)..(128 << 20) };
    wgpu::MemoryHints::Manual { suballocated_device_memory_block_size: blocks }
}

/// Unit tests and `software-adapter-tests` builds admit a CPU adapter while
/// `LAYER_TEST_SOFTWARE_GPU` is set. Production hosts always require hardware.
pub fn software_adapter_tests() -> bool {
    cfg!(any(test, feature = "software-adapter-tests"))
        && std::env::var_os("LAYER_TEST_SOFTWARE_GPU").is_some()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GpuRasterError {
    Color(String),
    CaptureBudget { required: u64, limit: u64 },
    SourceWorkingSetExceeded,
    DeferredObjectWork,
    AdapterUnavailable,
    HardwareAdapterRequired,
    DeviceRequest(String),
    InvalidExtent,
    ThumbnailUnavailable(layer_render::ThumbnailTarget),
    ExtentUnsupported,
    InvalidImage,
    MissingBrushMask(AssetId),
    MissingPaintLayer(SourceTarget),
    UnsupportedBrushFeature(&'static str),
    InvalidDabRange,
    InvalidTransform(&'static str),
    MultiplePreviewLayers,
    SizeOverflow,
    MapFailed(String),
    WaitFailed(String),
    FilterPreviewCancelled,
    Effect(String),
}

impl fmt::Display for GpuRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color(message) => write!(formatter, "color: {message}"),
            Self::DeferredObjectWork => formatter.write_str("Canonical image sampling is pending"),
            Self::SourceWorkingSetExceeded => formatter.write_str("Source working set exceeds the decoded tile budget"),
            Self::CaptureBudget { required, limit } => write!(formatter,
                "Snapshot dependency plan requires {required} bytes; limit is {limit}"),
            Self::Effect(message) => write!(formatter, "effect shader: {message}"),
            Self::FilterPreviewCancelled => formatter.write_str("Filter preview source changed"),
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
            Self::ThumbnailUnavailable(target) => write!(formatter, "Thumbnail target is no longer available: {target:?}"),
            Self::ExtentUnsupported => {
                formatter.write_str("canvas extent exceeds the adapter limit")
            }
            Self::InvalidImage => formatter.write_str("invalid image data"),
            Self::MissingBrushMask(id) => write!(formatter, "brush mask is not prepared: {}", id.0),
            Self::MissingPaintLayer(id) => {
                write!(formatter, "paint source is not available: {:?}", id)
            }
            Self::UnsupportedBrushFeature(feature) => {
                write!(
                    formatter,
                    "brush feature is not implemented by the GPU renderer: {feature}"
                )
            }
            Self::InvalidDabRange => formatter.write_str("dab batch references an invalid range"),
            Self::InvalidTransform(message) => formatter.write_str(message),
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
    id: SourceTarget,
    pages: Vec<LayerPage>,
    coverage_pages: Vec<StrokeCoveragePage>,
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
    Clone = 6,
}

impl MaterialOperation {
    const ALL: [Self; 7] = [
        Self::Deposit,
        Self::Coverage,
        Self::Liquify,
        Self::Smudge,
        Self::Wet,
        Self::Watercolor,
        Self::Clone,
    ];
}

/// What a material pass reads besides its destination page.
#[derive(Clone, Copy)]
enum MaterialInputs<'a> {
    /// The pages around the destination.
    Neighborhood,
    /// A gathered sample field and its page metadata.
    Gathered(&'a wgpu::TextureView, &'a wgpu::Buffer),
    /// A retouching source's target and reference pages, in the eight slots
    /// around the destination.
    Retouch(&'a [wgpu::TextureView; 8]),
}

/// Brushes whose output pixels read only the same pixel of their destination
/// and of per-page inputs, so tiles, dab ranges and damage stay per page.
fn pointwise(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry || style.execution.retouches()
}

/// Pen-up passes that revisit every page the stroke painted: a wet or burnt
/// edge, or healing.
fn revisits_stroke(style: &layer_render::DabStyle) -> bool {
    style.rendering.edge_after_stroke || style.execution.heals()
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectPipelineKind {
    TexturedPaint,
    TexturedErase,
}

impl DirectPipelineKind {
    const COUNT: usize = 2;
}

#[repr(usize)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MaterialPipelineKind {
    Color,
    Coverage,
    Watercolor,
}

impl MaterialPipelineKind {
    const COUNT: usize = 3;

    fn index(self, operation: MaterialOperation) -> usize {
        operation as usize * Self::COUNT + self as usize
    }

    fn for_attachments(watercolor: bool, coverage: bool) -> Self {
        match (watercolor, coverage) {
            (true, _) => Self::Watercolor,
            (false, true) => Self::Coverage,
            (false, false) => Self::Color,
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
    tiles: &'a [BrushTile],
    target_extent: [u32; 2],
    target: BrushEncodingTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrushStateTargets {
    coverage: bool,
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
            // Watercolor's physical shader always reads/writes stroke coverage,
            // independently of the ordinary brush accumulation preference.
            coverage: style.rendering.accumulation == BrushAccumulation::Uniform
                || style.rendering.edge_after_stroke
                || style.execution == BrushExecution::Watercolor,
            watercolor_wetness: style.execution == BrushExecution::Watercolor,
        };
        let needs_destination = style.execution != BrushExecution::Dry
            || style.alpha_locked
            || style.rendering.blend_mode != BrushBlendMode::Normal
            || state.coverage;
        let edged = style.rendering.wet_edge > 0.0 || style.rendering.burnt_edge > 0.0;
        let material = match style.execution {
            BrushExecution::Liquify => MaterialOperation::Liquify,
            BrushExecution::Smudge => MaterialOperation::Smudge,
            BrushExecution::Wet => MaterialOperation::Wet,
            BrushExecution::Watercolor => MaterialOperation::Watercolor,
            BrushExecution::Clone | BrushExecution::Heal | BrushExecution::SpotHeal => MaterialOperation::Clone,
            BrushExecution::Dry if state.coverage => MaterialOperation::Coverage,
            BrushExecution::Dry => MaterialOperation::Deposit,
        };
        let hardware_blending = style.blend_space == layer_core::BlendSpace::Linear;
        let direct = (!needs_destination && edged && hardware_blending).then_some(match style.mode {
            DabMode::Paint => DirectPipelineKind::TexturedPaint,
            DabMode::Erase => DirectPipelineKind::TexturedErase,
        });
        Self {
            direct,
            material,
            state,
            reservoir: style.execution == BrushExecution::Wet,
            stroke_edge: style.rendering.edge_after_stroke,
        }
    }

    fn for_device(style: &layer_render::DabStyle, device: &PipelineDevice) -> Self {
        let mut plan = Self::for_style(style);
        if device.portable_blend() {
            plan.direct = None;
        }
        plan
    }
    fn requires_destination(self) -> bool {
        self.direct.is_none()
    }

    fn uses_paint_state(self) -> bool {
        self.state.coverage || self.state.watercolor_wetness
    }
}

struct StrokeCoveragePage {
    coordinate: [u32; 2],
    primary: PageSurface,
    secondary: PageSurface,
    active_secondary: bool,
    // Persistent coverage is cleared when a stroke claims it; prediction forks
    // committed coverage. Each batch copies the active surface before writing.
    owner: Option<StrokeId>,
}

impl StrokeCoveragePage {
    fn release_color_bindings(&self) {
        for surface in [&self.primary, &self.secondary] {
            surface.material_input.clear();
            surface.material_in_place_input.clear();
            surface.material_output.clear();
        }
    }

    fn active(&self) -> &PageSurface {
        if self.active_secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }
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

// Scratch descriptors retain capacity, but never retain page resources after encoding.
struct MaterialJob {
    coordinate: [u32; 2],
    local: PixelRect,
    destination_secondary: bool,
    coverage_destination_secondary: Option<bool>,
    dabs: std::ops::Range<u32>,
    page_index: usize,
    coverage_index: Option<usize>,
}

struct LayerPage {
    coordinate: [u32; 2],
    primary: PageSurface,
    // Inactive color is overwritten by a full-page copy, dry draw or post-stroke
    // edge pass before use. Only a new primary needs a separate clear.
    secondary: Option<PageSurface>,
    active_secondary: bool,
    primary_needs_clear: bool,
}

struct PageSurface {
    preview: std::cell::Cell<Option<(SourceTarget,[u32;2])>>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    texture_bind_group: wgpu::BindGroup,
    material_input: bindings::MaterialInput,
    material_in_place_input: bindings::MaterialInput,
    material_output: bindings::MaterialOutput,
}

fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    u64::from(texture.width())
        * u64::from(texture.height())
        * u64::from(texture.depth_or_array_layers())
        * u64::from(texture.format().block_copy_size(None).unwrap())
}
impl PageSurface {
    fn storage_bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }

    fn copy_to(&self, destination: &Self, encoder: &mut crate::submission::CommandEncoder) {
        destination.preview.set(None);
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            destination.texture.as_image_copy(),
            self.texture.size(),
        );
    }
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

    /// Keep the current pixels and view identity, regardless of which ping-pong
    /// surface owns them. The other surface contains no persistent state.
    fn discard_inactive(&mut self) -> u64 {
        let Some(secondary) = self.secondary.take() else {
            return 0;
        };
        let bytes = if self.active_secondary {
            let bytes = self.primary.storage_bytes();
            self.primary = secondary;
            self.primary_needs_clear = false;
            bytes
        } else {
            secondary.storage_bytes()
        };
        self.active_secondary = false;
        bytes
    }
}

struct MaskAsset {
    id: AssetId,
    source: std::sync::Arc<[u8]>,
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
    transport: AssetId,
}

struct TextureSet {
    key: TextureSetKey,
    bind_group: wgpu::BindGroup,
    transport_bind_group: wgpu::BindGroup,
}

struct Pipelines {
    dry_material: dry_material::Pipelines,
    dry_display: dry_material::Pipelines,
    dry_display_tracked: Option<dry_material::Pipelines>,
    dry_in_place: Option<dry_material::Pipelines>,
    dry_tracked: Option<dry_material::Pipelines>,
    direct: [Deferred<wgpu::RenderPipeline>; DirectPipelineKind::COUNT],
    material: [Deferred<wgpu::RenderPipeline>;
        MaterialOperation::ALL.len() * MaterialPipelineKind::COUNT],
    material_gather: [Deferred<wgpu::RenderPipeline>; 2],
    watercolor_transport: [Deferred<wgpu::RenderPipeline>; WATERCOLOR_TRANSPORT_STEPS as usize],
    reservoir: Deferred<wgpu::RenderPipeline>,
    stroke_edge: Deferred<wgpu::RenderPipeline>,
    watercolor_composite: Deferred<wgpu::RenderPipeline>,
    watercolor_compute: (wgpu::BindGroupLayout, Deferred<wgpu::ComputePipeline>),
    export: Deferred<wgpu::RenderPipeline>,
}

impl Pipelines {
    fn direct(&self, kind: DirectPipelineKind) -> &wgpu::RenderPipeline {
        &self.direct[kind as usize]
    }

    fn material(
        &self,
        operation: MaterialOperation,
        watercolor: bool,
        coverage: bool,
    ) -> &wgpu::RenderPipeline {
        &self.material[MaterialPipelineKind::for_attachments(watercolor, coverage).index(operation)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Initialization {
    Headless,
    Interactive,
    /// A worker compiles only the dependencies of its immutable capture.
    Snapshot,
}

/// Headless-capable wgpu brush renderer. A platform presenter can sample the
/// same composite texture rather than requesting readback.
pub struct WgpuRasterizer {
    snapshot_worker: bool,
    snapshot_cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    startup: Option<startup::Startup>,
    adapter: wgpu::Adapter,
    device: PipelineDevice,
    queue: wgpu::Queue,
    document_extent: [u32; 2],
    target_geometry: target_geometry::TargetGeometry,
    paint_layers: Vec<PaintLayer>,
    changed_cells: Option<changed_cells::ChangedCells>,
    raster: Option<raster::RasterRuntime>,
    document_color: layer_core::color::DocumentColor,
    native_edit: Option<raster::native_edit::NativeEdit>,
    raster_buffers: std::sync::Arc<raster::BufferPool>,
    layer_masks: layer_masks::MaskRenderer,
    selection_clip: selection_clip::SelectionClip,
    display_selection: Option<(layer_core::Selection, wgpu::Buffer)>,
    /// Counts replacements of the displayed selection; the latest one
    /// repaints `display_selection_damage`, in document pixels.
    display_selection_revision: u64,
    display_selection_damage: PixelRect,
    selection_painter: Option<selection_paint::SelectionPainter>,
    selection_overlay: Option<layer_render::SelectionOverlay>,
    crop_overlay: Option<layer_render::CropOverlay>,
    clipping_preview: [bool; 2],
    selection_previews: selection_previews::SelectionPreviews,
    selection_paint_revision: u64,
    selection_paint_damage: PixelRect,
    regions: Option<region_requests::RegionRequests>,
    unclipped: wgpu::Buffer,
    scene: Option<scene::Scene>,
    source_tiles: std::cell::RefCell<scene::sources::DecodedTiles>,
    moving_images: object_image_mips::MovingImages,
    moving_projection: Option<MovingProjection>,
    moving_images_waiting: bool,
    moving_decode: Option<(layer_core::color::RgbSpace, object_image_mips::MipDecodeQueue)>,
    failed_image_tiles: Vec<moving_projection::FailedImageTile>,
    #[cfg(target_arch = "wasm32")]
    browser_image_decoder: Option<BrowserImageDecoder>,
    #[cfg(target_arch = "wasm32")]
    browser_nearest_coordinate_decoder: Option<BrowserNearestCoordinateDecoder>,
    #[cfg(target_arch = "wasm32")]
    browser_image_result: std::rc::Rc<std::cell::RefCell<Option<object_image_mips::PreparedImageWork>>>,
    #[cfg(target_arch = "wasm32")]
    browser_image_wake: std::rc::Rc<std::cell::RefCell<Option<std::task::Waker>>>,
    #[cfg(target_arch = "wasm32")]
    snapshot_decode_wake: Option<Arc<std::sync::Mutex<Option<std::task::Waker>>>>,
    transforms: Option<paint_transform::PaintTransforms>,
    transform_preview: Option<layer_render::TransformPreview>,
    transform_damage: Vec<(SourceTarget, PixelRect)>,
    /// Document regions to recompose whose layers' own pixels are unchanged.
    document_damage: Vec<PixelRect>,
    /// A warp preview waits, showing the frame before it, until what draws
    /// meshes has compiled in the background.
    awaiting_meshes: bool,
    background_ready: Arc<std::sync::atomic::AtomicBool>,
    background_refinement: bool,
    moving_pixels: Option<(SourceTarget, layer_core::Selection)>,
    moving_layer: Option<OccurrenceHandle>,
    #[cfg(test)]
    test: TestHooks,
    thumbnails: thumbnails::Thumbnails,
    ui_preview_space: layer_core::color::RgbSpace,
    ui_rendition: Option<layer_core::color::hdr::SdrRendition>,
    ui_preview_pipeline: Option<wgpu::RenderPipeline>,
    display_pipelines: Option<display_mips::Pipelines>,
    scale_display: Option<scene::scale::Cache>,
    display_backup: Option<scene::scale::DisplayBackup>,
    retired_display_bytes: Arc<std::sync::atomic::AtomicU64>,
    object_deferred: bool,
    navigator: scene::scale::Navigator,
    color_sampler: color_sample::ColorSampler,
    composite_revision: u64,
    artwork_revision: u64,
    evaluated_object_revision: Option<u64>,
    composite_damage: PixelRect,
    /// The composite's blend space, from the latest frame.
    blend_space: layer_core::BlendSpace,
    filter_previews: Option<scene::FilterPreviews>,
    effect_validation: Option<effect_validation::Pending>,
    validated_effects: Option<effects::Effects>,
    layer_style_records: std::collections::HashMap<OccurrenceHandle, u32>,
    artwork_frame: Option<Arc<artwork::Frame>>,
    settling: Option<artwork::PendingFrame>,
    effect_clocks: effects::Clocks,
    submitted_context: Option<EvaluationContext>,
    effect_analyses: Vec<Arc<effect_analysis::Prepared>>,
    analysis_job: Option<effect_analysis::Job>,
    analysis_candidate: Option<effect_analysis::Candidate>,
    analysis_dirty: bool,
    bake_analyses: Vec<effect_analysis::BakeTask>,
    #[cfg(target_arch = "wasm32")]
    analysis_backing_waiter: Option<effect_analysis::BackingWaiter>,
    filter_source_epoch: u64,
    tiled_sources: std::collections::BTreeMap<SourceTarget, layer_core::authored::PaintBase>,
    preview_pages: Vec<LayerPage>,
    preview_coverage_pages: Vec<StrokeCoveragePage>,
    preview_watercolor_wetness_pages: Vec<WatercolorWetnessPage>,
    preview_damage: PixelRect,
    preview_contact_tiles: Option<std::collections::BTreeSet<[u32; 2]>>,
    preview_layer_id: Option<SourceTarget>,
    preview_requires_base: bool,
    preview_contribution: bool,
    preview_level: u32,
    masks: Vec<MaskAsset>,
    texture_sets: Vec<TextureSet>,
    sampler: wgpu::Sampler,
    brush_sampler: wgpu::Sampler,
    style_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    advanced_texture_layout: wgpu::BindGroupLayout,
    target_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    empty_material_input: bindings::MaterialInput,
    material_source_meta: wgpu::Buffer,
    dry_records: material_sources::DryRecords,
    material_jobs: Vec<MaterialJob>,
    dry_jobs: Vec<dry_material::Job>,
    material_gather: Option<material_sources::Gather>,
    retouch: Option<Box<retouch_sources::RetouchSources>>,
    retouch_pipelines: Option<Arc<retouch_sources::Pipelines>>,
    edge_layout: wgpu::BindGroupLayout,
    watercolor_layout: wgpu::BindGroupLayout,
    transport_layout: wgpu::BindGroupLayout,
    style_buffer: wgpu::Buffer,
    style_bind_group: wgpu::BindGroup,
    style_stride: u64,
    style_capacity: usize,
    style_upload: Vec<u8>,
    dab_upload: Vec<DabGpu>,
    target_buffer: wgpu::Buffer,
    target_bind_group: wgpu::BindGroup,
    target_stride: u64,
    target_capacity: usize,
    target_upload: Vec<u8>,
    uploads: Uploads,
    restore_uploads: Uploads,
    snapshot_job: Option<snapshot::SnapshotJob>,
    #[cfg(target_arch = "wasm32")]
    snapshot_worker_callback: Option<snapshot::BrowserSnapshot>,
    _empty_texture: wgpu::Texture,
    empty_view: wgpu::TextureView,
    _empty_scalar_texture: wgpu::Texture,
    empty_scalar_view: wgpu::TextureView,
    reservoir: BrushReservoir,
    dab_buffer: wgpu::Buffer,
    dab_capacity_bytes: u64,
    pipelines: Pipelines,
    scene_pipelines: scene::Pipelines,
    portable_blend: portable_blend::Renderer,
    last_submission: Option<wgpu::SubmissionIndex>,
    metrics: GpuRasterMetrics,
    telemetry: telemetry::Telemetry,
}

/// What tests switch and count in a renderer.
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    /// Recompose every change, as a reference for layers drawn straight into
    /// the display.
    reference: bool,
    /// Exercise the exact display fallback independently of scale eligibility.
    exact_display: bool,
    reduced_pages: std::cell::Cell<u64>,
    source_captures: std::cell::Cell<u64>,
}

impl WgpuRasterizer {
    /// Borrowed device/queue for platform surface setup on the same GPU.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn headless_instance() -> wgpu::Instance {
        let create = || {
            let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
            descriptor.backends = wgpu::Backends::PRIMARY;
            descriptor.flags.remove(wgpu::InstanceFlags::DEBUG);
            wgpu::Instance::new(descriptor)
        };
        static RESIDENT_DRIVERS: std::sync::OnceLock<wgpu::Instance> = std::sync::OnceLock::new();
        RESIDENT_DRIVERS.get_or_init(create);
        create()
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn headless(space: layer_core::color::RgbSpace, initialization: Initialization) -> Result<Self, GpuRasterError> {
        let instance = Self::headless_instance();
        let indexed_adapter = std::env::var("LAYER_GPU_INDEX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok());
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
        let blendable = if std::env::var_os("CAPY_GPU_NO_FLOAT32_BLEND").is_some() { wgpu::Features::empty() } else { wgpu::Features::FLOAT32_BLENDABLE };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("layer canvas device"),
                required_features: (adapter.features() & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS | blendable))
                    | wgpu::Features::FLOAT32_FILTERABLE
                    | native_tiles::native_in_place_features(&adapter),
                required_limits: limits,
                memory_hints: memory_hints(),
                ..Default::default()
            })
            .await
            .map_err(|error| GpuRasterError::DeviceRequest(error.to_string()))?;

        Self::from_wgpu_inner(
            adapter,
            PipelineDevice::from(device).require_float32()?.with_working_space(space),
            queue,
            initialization,
        )
    }

    fn from_wgpu_inner(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        initialization: Initialization,
    ) -> Result<Self, GpuRasterError> {
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu && !software_adapter_tests() {
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
        let material_source_meta = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adjacent material source pages"), size: 160,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        let edge_layout = fragment_textures_layout(&device, 10, "layer post-stroke edge sources");
        let watercolor_layout = bindings::layout(&device, "layer watercolor pigment and wetness neighborhood",
            &(0..10).map(|i| bindings::texture(i, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, false)).collect::<Vec<_>>());
        let transport_layout = fragment_textures_layout(&device, 10, "layer watercolor transport sources");
        let style_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let style_stride =
            (mem::size_of::<StyleGpu>() as u64).div_ceil(style_alignment) * style_alignment;
        let style_capacity = INITIAL_STYLE_RECORDS;
        let style_buffer = create_style_buffer(&device, style_stride, style_capacity);
        let style_bind_group = create_style_bind_group(&device, &style_layout, &style_buffer);
        let target_stride = device
            .limits()
            .min_uniform_buffer_offset_alignment
            .max(mem::size_of::<TargetGpu>() as u32) as u64;
        let target_capacity = INITIAL_TARGET_RECORDS;
        let target_buffer = create_target_buffer(&device, target_stride, target_capacity);
        // wgpu initializes new buffers to zero. Mapping merely to write zeros
        // adds no data and can panic when device removal races construction.
        let unclipped = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("unrestricted brush coverage"),
            size: 48,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let target_bind_group =
            create_target_bind_group(&device, &target_layout, &target_buffer, &unclipped);
        let (empty_texture, empty_view) =
            create_color_target(&device, [1, 1], "layer transparent missing page");
        let (empty_scalar_texture, empty_scalar_view) = create_target(
            &device,
            [1, 1],
            device.scalar_format(),
            "layer zero missing paint state",
        );
        let reservoir = BrushReservoir {
            primary: create_page_surface(
                &device,
                &texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                device.working_format(),
                "layer brush reservoir A",
            ),
            secondary: create_page_surface(
                &device,
                &texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                device.working_format(),
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
        let restore_uploads = Uploads::new(&device, 64 * 1024);
        let layer_masks =
            layer_masks::MaskRenderer::new(&device, &style_layout, &target_layout, &texture_layout);
        let telemetry = telemetry::Telemetry::new(&device, &queue);
        let selection_clip = selection_clip::SelectionClip::new(&device);
        let scene_pipelines = scene::Pipelines::new(&device);
        let changed_cells = Some(changed_cells::ChangedCells::new(&device));
        let portable_blend = portable_blend::Renderer::new(&device);
        let transforms = Some(paint_transform::PaintTransforms::new(&device));
        let mut renderer = Self {
            snapshot_worker: initialization == Initialization::Snapshot,
            snapshot_cancelled: None,
            startup: None,
            telemetry,
            adapter,
            device,
            queue,
            document_extent: [0, 0],
            target_geometry: Default::default(),
            layer_masks,
            selection_clip,
            display_selection: None,
            display_selection_revision: 0,
            display_selection_damage: PixelRect::EMPTY,
            selection_painter: None,
            selection_overlay: None,
            crop_overlay: None,
            clipping_preview: [false; 2],
            selection_previews: Default::default(),
            selection_paint_revision: 0,
            selection_paint_damage: PixelRect::EMPTY,
            regions: None,
            unclipped,
            scene: None,
            source_tiles: Default::default(),
            moving_images: Default::default(),
            moving_projection: None,
            moving_images_waiting: false,
            moving_decode: None,
            failed_image_tiles: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            browser_image_decoder: None,
            #[cfg(target_arch = "wasm32")]
            browser_nearest_coordinate_decoder: None,
            #[cfg(target_arch = "wasm32")]
            browser_image_result: Default::default(),
            #[cfg(target_arch = "wasm32")]
            browser_image_wake: Default::default(),
            #[cfg(target_arch = "wasm32")]
            snapshot_decode_wake: None,
            transforms,
            transform_preview: None,
            transform_damage: Vec::with_capacity(2),
            document_damage: Vec::new(),
            awaiting_meshes: false,
            background_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            background_refinement: false,
            moving_pixels: None,
            moving_layer: None,
            #[cfg(test)]
            test: Default::default(),
            filter_previews: None,
            effect_validation: None,
            validated_effects: None,
            layer_style_records: std::collections::HashMap::new(),
            artwork_frame: None,
            settling: None,
            effect_clocks: Default::default(),
            submitted_context: None,
            effect_analyses: Vec::new(),
            analysis_job: None,
            analysis_candidate: None,
            analysis_dirty: false,
            bake_analyses: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            analysis_backing_waiter: None,
            filter_source_epoch: 0,
            display_pipelines: None,
            scale_display: None,
            display_backup: None,
            retired_display_bytes: Default::default(),
            object_deferred: false,
            navigator: Default::default(),
            color_sampler: color_sample::ColorSampler::new(),
            composite_revision: 0,
            artwork_revision: 0,
            evaluated_object_revision: None,
            composite_damage: PixelRect::EMPTY,
            thumbnails: thumbnails::Thumbnails::new(),
            ui_preview_space: layer_core::color::RgbSpace::Srgb,
            ui_rendition: None,
            ui_preview_pipeline: None,
            tiled_sources: Default::default(),
            paint_layers: Vec::with_capacity(8),
            changed_cells,
            raster: None,
            document_color: Default::default(),
            native_edit: None,
            raster_buffers: std::sync::Arc::new(raster::BufferPool::default()),
            blend_space: layer_core::BlendSpace::Linear,
            preview_pages: Vec::with_capacity(16),
            preview_coverage_pages: Vec::with_capacity(8),
            preview_watercolor_wetness_pages: Vec::with_capacity(8),
            preview_damage: PixelRect::EMPTY,
            preview_contact_tiles: None,
            preview_layer_id: None,
            preview_requires_base: false,
            preview_contribution: false,
            preview_level: 0,
            masks: Vec::with_capacity(8),
            texture_sets: Vec::with_capacity(16),
            sampler,
            brush_sampler,
            style_layout,
            texture_layout,
            advanced_texture_layout,
            target_layout,
            material_layout,
            empty_material_input: Default::default(),
            material_source_meta,
            dry_records: Default::default(),
            material_jobs: Vec::new(),
            dry_jobs: Vec::with_capacity(SOURCE_SLOTS),
            material_gather: None,
            retouch: None,
            retouch_pipelines: None,
            edge_layout,
            watercolor_layout,
            transport_layout,
            style_buffer,
            style_bind_group,
            style_stride,
            style_capacity,
            style_upload: Vec::with_capacity(style_stride as usize * style_capacity),
            dab_upload: Vec::new(),
            target_buffer,
            target_bind_group,
            target_stride,
            target_capacity,
            target_upload: Vec::with_capacity(target_stride as usize * target_capacity),
            uploads,
            restore_uploads,
            snapshot_job: None,
            #[cfg(target_arch = "wasm32")]
            snapshot_worker_callback: None,
            _empty_texture: empty_texture,
            empty_view,
            _empty_scalar_texture: empty_scalar_texture,
            empty_scalar_view,
            reservoir,
            dab_buffer,
            dab_capacity_bytes: INITIAL_DAB_BYTES,
            pipelines,
            scene_pipelines,
            portable_blend,
            last_submission: None,
            metrics: GpuRasterMetrics::default(),
        };
        if initialization == Initialization::Headless { renderer.install_builtin_masks()?; }
        if initialization == Initialization::Interactive {
            renderer.upload_mask(&AssetId::from(WHITE_MASK_ASSET), 1, 1, 1, &[255])?;
            renderer.validated_effects = Some(renderer.scene_pipelines.effects(&renderer));
            renderer.startup = Some(startup::Startup::new(&renderer.device)?);
        }
        Ok(renderer)
    }

    /// The platform host validates replacement surfaces against this same GPU.
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// Canvas submissions, excluding viewport-only cursor/navigation updates.
    pub fn submitted_updates(&self) -> u64 {
        self.metrics.submissions
    }

    pub fn metrics(&self) -> GpuRasterMetrics {
        let mut metrics = self.metrics.clone();
        metrics.raster_backing_reserved_bytes = self.raster_staging_bytes();
        [metrics.source_tile_hits, metrics.source_tile_misses] = self.source_cache_work();
        metrics.source_tile_evictions = self.source_tiles.borrow().evictions;
        metrics.moving_image_evictions = self.moving_images.evictions;
        metrics.moving_image_request_bytes = self.moving_projection.as_ref().map_or(0, MovingProjection::storage_bytes);
        metrics
    }

    pub fn document_extent(&self) -> [u32; 2] {
        self.document_extent
    }

    /// Changes only when document composition changes, never for camera motion.
    pub fn canvas_preview_revision(&self) -> u64 {
        self.artwork_revision
    }

    pub fn evaluated_object_revision(&self) -> Option<u64> {
        self.evaluated_object_revision
    }

    pub(crate) fn display_views(&self)->Option<[&wgpu::TextureView;3]> {
        if let Some(backup)=&self.display_backup {return Some([&backup.views[0],&backup.views[2],&backup.views[1]]);}
        if self.object_deferred {return None;}
        self.scale_display.as_ref().map(|cache|[cache.view(),cache.coarse_view(),cache.next_view()])
    }
    pub(crate) fn display_geometry(&self)->Option<&wgpu::Buffer> {
        self.display_backup.as_ref().map(|backup|&backup.geometry).or_else(||self.scale_display.as_ref().map(|cache|&cache.geometry))
    }
    pub(crate) fn display_placement(&self)->[f32;20] {
        self.display_backup.as_ref().map_or_else(||self.scale_display.as_ref().unwrap().placement_values(),|backup|backup.placement)
    }
    pub(crate) fn display_resample(&self)->[u8;scene::resample::UNIFORM_BYTES as usize] {
        self.display_backup.as_ref().map_or_else(||self.scale_display.as_ref().unwrap().resample_values(),|backup|backup.resample)
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

    fn install_builtin_masks(&mut self) -> Result<(), GpuRasterError> {
        for (id, generate) in builtin_masks() {
            let (width, height, pixels) = generate()?;
            self.upload_mask(&AssetId::from(id), width, height, width, &pixels)?;
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
        self.prepare_asset(
            id,
            HostImage {
                width,
                height,
                stride,
                format: PixelFormat::R8Unorm,
                bytes: pixels,
            },
        )
    }

    // Called only after the common owned-source validation.
    fn upload_mask_source(&mut self, id: &AssetId, source: &layer_core::ProjectAsset) {
        let [width, height] = source.extent;
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
            &source.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
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
            source: source.bytes.clone(),
            extent: [width, height, width],
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
        if style.contact.is_some_and(|c| c.bristles.is_some()) {
            return TextureSetKey {
                primary: AssetId::from(bristle_table::HAIRS_ASSET),
                grain,
                transport: AssetId::from(bristle_table::FIELD_ASSET),
            };
        }
        TextureSetKey {
            primary,
            grain,
            transport: style
                .transport
                .as_ref()
                .map(|transport| transport.conductance.clone())
                .unwrap_or(white),
        }
    }

    fn ensure_texture_set(&mut self, key: TextureSetKey) -> Result<(), GpuRasterError> {
        if self.texture_sets.iter().any(|set| set.key == key) {
            return Ok(());
        }
        let primary = &self.mask(&key.primary)?.view;
        let grain = &self.mask(&key.grain)?.view;
        let transport = &self.mask(&key.transport)?.view;
        let bind_group = create_advanced_texture_bind_group(
            &self.device,
            &self.advanced_texture_layout,
            [primary, grain, transport],
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

    /// Whether warp meshes draw without compiling on this thread, asking the
    /// background compiler for what they draw with. Without one, a frame
    /// compiles it.
    fn mesh_pipelines_ready(&self) -> bool {
        let (Some(startup), Some(transforms)) = (&self.startup, &self.transforms) else {
            return true;
        };
        startup.compiler.require(transforms.mesh_pipelines(), startup::BRUSH)
            & startup.compiler.require(self.scene_pipelines.resample.mesh.iter(), startup::BRUSH)
    }

    pub(crate) fn background_pipeline_ready(&self, pipeline: &deferred::Deferred<wgpu::ComputePipeline>) -> bool {
        let Some(startup) = &self.startup else { return true };
        let ready = startup.compiler.require([pipeline], startup::VALIDATION);
        startup.compiler.start();
        ready
    }

    fn validate_and_prepare_brush_resources(
        &mut self,
        batches: &[DabBatch],
    ) -> Result<(), GpuRasterError> {
        for batch in batches {
            if batch.style.execution == BrushExecution::Liquify
                && batch.style.deform.mode == LiquifyMode::Reconstruct
            {
                return Err(GpuRasterError::UnsupportedBrushFeature(
                    "liquify reconstruct snapshot",
                ));
            }
            self.ensure_texture_set(Self::texture_set_key(&batch.style))?;
        }
        Ok(())
    }

    /// Set the document topology independently of full-composite allocation.
    /// Snapshot/window consumers prepare only their own pixel dependencies.
    fn ensure_document_metadata(&mut self, extent: [u32; 2], scene: SceneView<'_>) -> Result<bool, GpuRasterError> {
        if let Some(regions) = &mut self.regions { regions.raw.clear_bindings(); }
        if extent.contains(&0) { return Err(GpuRasterError::InvalidExtent); }
        let resized = extent != self.document_extent;
        if resized || self.artwork_frame.as_ref().is_some_and(|frame| {
            frame.scene.view().order().iter().any(|&handle| {
                scene.occurrence(handle).is_none_or(|new| {
                    let old_scene = frame.scene.view();
                    let old = old_scene.occurrence(handle).unwrap();
                    old.content != new.content || match (old_scene.paint_source(handle).and_then(|p| p.base.as_ref()), scene.paint_source(handle).and_then(|p| p.base.as_ref())) {
                        (Some(a), Some(b)) => !a.image.same_owner(&b.image) || a.offset != b.offset || a.policy != b.policy,
                        (None, None) => false,
                        _ => true,
                    }
                })
            })
        }) { self.artwork_frame = None; }
        let retained = source_access::retained_targets(scene);
        self.retain_native_backing(&retained, resized);
        if resized {
            self.selection_clip.reset();
            self.document_extent = extent;
            let vector_selection = self.display_selection.as_ref()
                .filter(|(selection, _)| !matches!(selection.shape, layer_core::SelectionShape::Pixels(_)))
                .map(|(selection, _)| selection.clone());
            if let Some(selection) = vector_selection {
                self.display_selection = None;
                self.replace_display_selection(Some(&selection))?;
            }
            self.paint_layers.clear();
            self.preview_pages.clear();
            self.preview_coverage_pages.clear();
            self.preview_watercolor_wetness_pages.clear();
            self.scale_display = None;
            self.preview_damage = PixelRect::EMPTY;
            self.preview_contact_tiles = None;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_contribution = false;
        }
        self.update_target_geometry(&retained.iter().map(|(target, scene)| (*target, scene.target_extent(*target))).collect(), resized)?;
        self.tiled_sources.retain(|target, _| scene.paint_base(*target).is_some() && source_access::placed_targets(scene).any(|t|t==*target));
        for &handle in scene.order() {
            let Some(source) = scene.paint_source(handle).and_then(|p| p.base.as_ref()) else { continue; };
            let target = scene.source_target(handle).unwrap();
            if self.tiled_sources.get(&target).is_none_or(|current| !current.image.same_owner(&source.image) || current.offset != source.offset || current.policy != source.policy) {
                self.tiled_sources.insert(target, source.clone());
            }
        }
        self.paint_layers.retain(|stored| retained.contains_key(&stored.id) && matches!(stored.id, SourceTarget::Paint(_)));
        for &target in retained.keys().filter(|target| matches!(target, SourceTarget::Paint(_))) {
            if self.paint_layers.iter().all(|stored| stored.id != target) {
                self.paint_layers.push(PaintLayer { id: target, pages: Vec::with_capacity(8), coverage_pages: Vec::with_capacity(4),
                    watercolor_wetness_pages: Vec::with_capacity(4), watercolor: None });
            }
        }
        Ok(resized)
    }

    fn create_page(&self, coordinate: [u32; 2], label: &'static str) -> LayerPage {
        let primary = self.create_page_surface(label);
        LayerPage {
            coordinate,
            primary,
            secondary: None,
            active_secondary: false,
            primary_needs_clear: true,
        }
    }

    fn create_page_surface(&self, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            self.device.working_format(),
            label,
        )
    }

    fn create_scalar_page_surface(&self, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            self.device.scalar_format(),
            label,
        )
    }

    fn stroke_finish_pages<'a>(&'a self, batch: &'a DabBatch) -> impl Iterator<Item = [u32; 2]> + 'a {
        self.paint_layers.iter()
            .filter(move |layer| batch.stroke_end && revisits_stroke(&batch.style) && layer.id == batch.target)
            .flat_map(|layer| &layer.coverage_pages)
            .filter(move |page| page.owner == Some(batch.stroke_id))
            .map(|page| page.coordinate)
    }

    fn destination_pages(
        &self,
        batches: &[DabBatch],
        tiles: &[Vec<BrushTile>],
    ) -> std::collections::BTreeSet<(SourceTarget, [u32; 2])> {
        let mut destination_pages = std::collections::BTreeSet::new();
        for (batch, tiles) in batches.iter().zip(tiles).filter(|(batch, _)| {
            batch.kind == DabBatchKind::Persistent
                && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
        }) {
            let revisits = batch.stroke_end && batch.style.rendering.edge_after_stroke;
            if !self.in_place_dry_material(batch) || revisits {
                destination_pages.extend(tiles.iter().map(|tile| (batch.target, tile.coordinate)));
            }
            if revisits { destination_pages.extend(self.stroke_finish_pages(batch).map(|page| (batch.target, page))); }
        }
        destination_pages
    }

    fn ensure_destination_companions(&mut self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) {
        let destination_pages = self.destination_pages(batches, tiles);
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
            }
        }
    }

    fn material_bind_group(
        &mut self,
        batch: &DabBatch,
        coordinate: [u32; 2],
        preview: bool,
        inputs: MaterialInputs<'_>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let in_place = self.in_place_dry_material(batch);
        let gathered = match inputs {
            MaterialInputs::Gathered(field, meta) => Some((field, meta)),
            _ => None,
        };
        let offsets = std::array::from_fn::<_, 9, _>(|i| {
            if in_place || self.compact_preview_contribution(batch)
                || ((pointwise(&batch.style) || gathered.is_some()) && i != 4) {
                // Dry paint reads only its destination pixel. Completed gather
                // fields already contain nonlocal smudge/liquify samples.
                // Neither needs to decode or bind surrounding source tiles.
                None
            } else {
                Some([i as i32 % 3 - 1, i as i32 / 3 - 1])
            }
        });
        self.prepare_raw_neighborhood(batch.target, coordinate, offsets, preview, encoder)?;
        let layer = self
            .paint_layers
            .iter()
            .find(|layer| layer.id == batch.target)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
        let sources = self.source_tiles.borrow();
        let mut views = self.raw_layer_neighborhood(&sources, layer, coordinate, offsets, preview);
        if let MaterialInputs::Retouch(pages) = inputs {
            for (slot, page) in [0, 1, 2, 3, 5, 6, 7, 8].into_iter().zip(pages) {
                views[slot] = page;
            }
        }
        let coverage_page = if preview {
            self.preview_coverage_pages
                .iter()
                .find(|p| p.coordinate == coordinate)
        } else {
            None
        }
        .or_else(|| {
            layer
                .coverage_pages
                .iter()
                .find(|p| p.coordinate == coordinate && p.owner == Some(batch.stroke_id))
        })
        .map(|p| p.active());
        let coverage = coverage_page.map_or(&self.empty_scalar_view, |p| &p.view);
        let auxiliary = if BrushPassPlan::for_device(&batch.style, &self.device).state.watercolor_wetness {
            let pages = if preview {
                &self.preview_watercolor_wetness_pages
            } else {
                &layer.watercolor_wetness_pages
            };
            &pages
                .iter()
                .find(|p| p.coordinate == coordinate)
                .expect("watercolor wetness page is prepared before binding")
                .active()
                .view
        } else if pointwise(&batch.style) {
            &self.empty_view
        } else {
            &self.reservoir.active().view
        };
        let create = || create_material_bind_group(
            &self.device,
            &self.material_layout,
            &views,
            &self.dab_buffer,
            coverage,
            gathered.map_or(auxiliary, |g| g.0),
            if pointwise(&batch.style) {
                wgpu::BindingResource::Buffer(self.dry_records.binding())
            } else { gathered.map_or(&self.material_source_meta, |g| g.1).as_entire_binding() },
        );
        if pointwise(&batch.style) {
            // Coverage owns state-dependent bindings, so ending a stroke can
            // release both coverage textures even while color pages survive.
            let color_page = if preview { self.preview_page(coordinate) } else { None }
                .or_else(|| layer.pages.iter().find(|p| p.coordinate == coordinate));
            let cache = coverage_page.or_else(|| color_page.map(|p| p.active()))
                .map(|p| if in_place { &p.material_in_place_input } else { &p.material_input })
                .unwrap_or(&self.empty_material_input);
            let bound = std::array::from_fn(|i| views.get(i).map_or(coverage, |view| *view).clone());
            Ok(cache.get(([self.dab_buffer.clone(), self.dry_records.binding().buffer.clone()], bound), create))
        } else { Ok(create()) }
    }

    fn ensure_preview_destination_companions(&mut self, batches: &[DabBatch]) {
        let mut coordinates = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Preview
                && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
        }) {
            let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
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

    fn ensure_persistent_pages(&mut self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) -> Result<(), GpuRasterError> {
        for (batch, tiles) in batches.iter().zip(tiles)
            .filter(|(batch, _)| batch.kind == DabBatchKind::Persistent && batch.dab_count != 0)
        {
            if tiles.is_empty() {
                continue;
            }
            let layer_index = self
                .paint_layers
                .iter()
                .position(|layer| layer.id == batch.target)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
            for tile in tiles {
                let coordinate = tile.coordinate;
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

    fn ensure_paint_state_pages(&mut self, batches: &[DabBatch], tiles: &[Vec<BrushTile>]) -> Result<(), GpuRasterError> {
        for (batch, tiles) in batches.iter().zip(tiles).filter(|(batch, _)| {
            batch.kind == DabBatchKind::Persistent
                && batch.dab_count != 0
                && BrushPassPlan::for_device(&batch.style, &self.device).uses_paint_state()
        }) {
            let plan = BrushPassPlan::for_device(&batch.style, &self.device);
            if tiles.is_empty() {
                continue;
            }
            let layer_index = self
                .paint_layers
                .iter()
                .position(|layer| layer.id == batch.target)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
            for tile in tiles {
                let coordinate = tile.coordinate;
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

    fn ensure_preview_pages(&mut self, damage: PixelRect, sparse: Option<&std::collections::BTreeSet<[u32; 2]>>) {
        if damage.is_empty() {
            return;
        }
        for coordinate in page_coordinates(damage).filter(|c| sparse.is_none_or(|set| set.contains(c))) {
            if self
                .preview_pages
                .iter()
                .all(|page| page.coordinate != coordinate)
            {
                if let Some(page) = self
                    .preview_pages
                    .iter_mut()
                    .find(|page| sparse.map_or_else(|| page_rect(page.coordinate).intersect(damage).is_empty(), |set| !set.contains(&page.coordinate)))
                {
                    // Old prediction pixels are never persistent input. Rebind
                    // an off-tail page to the new coordinate; the preview path
                    // overwrites its complete active scissor from persistent
                    // canvas state before composition reads it.
                    page.coordinate = coordinate;
                    page.active_secondary = false;
                } else {
                    let page = LayerPage {
                        coordinate,
                        primary: create_page_surface(&self.device, &self.texture_layout, &self.sampler,
                            [PAGE_SIZE >> self.preview_level; 2], self.device.working_format(), "prediction page"),
                        secondary: None, active_secondary: false, primary_needs_clear: true,
                    };
                    self.preview_pages.push(page);
                }
            }
        }
    }

    fn ensure_preview_coverage_pages(&mut self, batches: &[DabBatch]) {
        let mut coordinates = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Preview
                && BrushPassPlan::for_device(&batch.style, &self.device).state.coverage
        }) {
            let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
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
        let pixel_bytes = u64::from(self.device.working_format().block_copy_size(None).unwrap());
        let page_bytes = u64::from(PAGE_SIZE * PAGE_SIZE) * pixel_bytes;
        let scalar_bytes = u64::from(PAGE_SIZE * PAGE_SIZE) * u64::from(self.device.scalar_format().block_copy_size(None).unwrap());
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
            .map(|layer| layer.watercolor_wetness_pages.len() as u64)
            .sum::<u64>();
        let material_surface_pages = self
            .paint_layers
            .iter()
            .map(|layer| layer.watercolor_wetness_pages.len() as u64 * 2)
            .sum::<u64>();
        self.metrics.paint_pages = paint_pages;
        self.metrics.preview_pages = preview_pages;
        self.metrics.destination_companion_pages = destination_companion_pages;
        self.metrics.coverage_pages = coverage_pages;
        self.metrics.material_pages = material_pages;
        self.metrics.paint_storage_bytes = paint_pages.saturating_mul(page_bytes);
        self.metrics.preview_storage_bytes = preview_pages
            .saturating_mul(page_bytes >> (2 * self.preview_level))
            .saturating_add(preview_coverage_pages.saturating_mul(scalar_bytes * 2))
            .saturating_add(preview_watercolor_wetness_pages.saturating_mul(scalar_bytes * 2));
        self.metrics.destination_storage_bytes =
            destination_companion_pages.saturating_mul(page_bytes);
        self.metrics.paint_state_storage_bytes = coverage_pages
            .saturating_mul(scalar_bytes * 2)
            .saturating_add(material_surface_pages.saturating_mul(scalar_bytes))
            .saturating_add(u64::from(RESERVOIR_SIZE * RESERVOIR_SIZE) * pixel_bytes * 2)
            .saturating_add(self.selection_clip.storage_bytes())
            .saturating_add(self.selection_painter.as_ref().map_or(0, |p| p.storage_bytes()))
            .saturating_add(self.selection_previews.buffer.as_ref().map_or(0, |b|b.size()*2))
            .saturating_add(self.color_sampler.storage_bytes())
            .saturating_add(self.regions.as_ref().map_or(0, |r| r.storage_bytes()));
        self.metrics.paint_state_storage_bytes += self.changed_cells.as_ref().map_or(0, changed_cells::ChangedCells::storage_bytes);
        self.metrics.retouch_storage_bytes =
            self.retouch.as_ref().map_or(0, |retouch| retouch.storage_bytes());
        self.metrics.composite_storage_bytes =
            self.scale_display.as_ref().map_or(0, scene::scale::Cache::storage_bytes) + self.navigator.storage_bytes()
                + self.display_backup.as_ref().map_or(0,|backup|backup.bytes);
        self.metrics.composite_storage_bytes+=self.retired_display_bytes.load(std::sync::atomic::Ordering::Acquire);
    }

    fn ensure_upload_capacity(&mut self, dabs: usize, styles: usize) -> Result<(), GpuRasterError> {
        let dab_bytes = (dabs as u64)
            .checked_mul(mem::size_of::<DabGpu>() as u64)
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
        batch_tiles: &mut [Vec<BrushTile>],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let removed_members: Vec<_> = source_access::pending_bakes(packet.scene)
            .flat_map(|(snapshot, scope, _)| snapshot.with_scope(scope).order().iter().copied().filter(|h| packet.scene.occurrence(*h).is_none()).map(move |h| (h, snapshot)))
            .collect();
        let records = packet.dab_batches.len() + packet.scene.order().len() + removed_members.len();
        self.dab_upload.clear();
        self.dab_upload.extend(packet.dabs.iter().copied().map(DabGpu::from));
        for batch in packet.dab_batches {
            let range = batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize;
            dry_material::prepare_colors(&batch.style, self.device.working_space(), &mut self.dab_upload[range.clone()]);
            dry_material::prepare_film(&batch.style, &mut self.dab_upload[range]);
        }
        for (batch, tiles) in packet.dab_batches.iter().zip(batch_tiles) {
            if batch.dab_count == 0 || !pointwise(&batch.style)
                || BrushPassPlan::for_device(&batch.style, &self.device).direct.is_some() { continue; }
            for tile in tiles {
                if tile.indices.is_empty() || tile.indices.len() == tile.dabs.len() { continue; }
                let start = u32::try_from(self.dab_upload.len()).map_err(|_| GpuRasterError::SizeOverflow)?;
                for &index in &tile.indices {
                    let dab = self.dab_upload[index as usize];
                    self.dab_upload.push(dab);
                }
                let end = u32::try_from(self.dab_upload.len()).map_err(|_| GpuRasterError::SizeOverflow)?;
                tile.dabs = start..end;
            }
        }
        self.ensure_upload_capacity(self.dab_upload.len(), records)?;
        if !packet.dabs.is_empty() {
            self.uploads.write(
                encoder,
                &self.dab_buffer,
                dab_bytes(&self.dab_upload),
            )?;
        }
        let used = self.style_stride as usize * records;
        self.style_upload.clear();
        self.style_upload.resize(used, 0);
        for index in 0..packet.dab_batches.len() {
            let batch = &packet.dab_batches[index];
            let block = if self.compute_dry_material(batch) { self.dry_material_block(batch) } else { 1 };
            let mut record = StyleGpu::brush(self.target_extent(batch.target), batch, block, &self.device);
            record.color_mode = layer_color_parameters(packet.scene.color_mode(batch.target), self.document_color());
            record.color_mode[2] = f32::from(batch.style.blend_space == layer_core::BlendSpace::Perceptual);
            let offset = index * self.style_stride as usize;
            self.style_upload[offset..offset + mem::size_of::<StyleGpu>()]
                .copy_from_slice(style_bytes(&record));
        }
        self.layer_style_records.clear();
        let occurrences = packet.scene.order().iter().map(|&handle| (handle, packet.scene)).chain(removed_members);
        for (index, (handle, scene)) in occurrences.enumerate() {
            let record_index = packet.dab_batches.len() + index;
            self.layer_style_records.insert(handle, record_index as u32);
            let watercolor = scene.source_target(handle).and_then(|target| self.watercolor_style(target, packet.dab_batches));
            let mut record = StyleGpu::layer(packet.document_extent, watercolor);
            record.color_mode = layer_color_parameters(scene.source_target(handle).map_or(Default::default(), |target| scene.color_mode(target)), self.document_color());
            let offset = record_index * self.style_stride as usize;
            self.style_upload[offset..offset + mem::size_of::<StyleGpu>()]
                .copy_from_slice(style_bytes(&record));
        }
        if !self.style_upload.is_empty() {
            self.uploads
                .write(encoder, &self.style_buffer, &self.style_upload)?;
        }
        Ok(())
    }

    fn watercolor_style(&self, layer_id: SourceTarget, batches: &[DabBatch]) -> Option<WatercolorLayerStyle> {
        batches.iter().rev().find(|batch| batch.target == layer_id
            && batch.style.execution == BrushExecution::Watercolor)
            .map(|batch| WatercolorLayerStyle::from_dab_style(&batch.style))
            .or_else(|| self.paint_layers.iter().find(|layer| layer.id == layer_id).and_then(|layer| layer.watercolor))
    }

    fn update_watercolor_layer_styles(&mut self, batches: &[DabBatch]) -> PixelRect {
        let mut dirty = PixelRect::EMPTY;
        let mut cold_damage = Vec::new();
        for batch in batches.iter().filter(|batch| {
            batch.kind == DabBatchKind::Persistent
                && batch.style.execution == BrushExecution::Watercolor
        }) {
            let next = WatercolorLayerStyle::from_dab_style(&batch.style);
            let Some(layer) = self
                .paint_layers
                .iter_mut()
                .find(|layer| layer.id == batch.target)
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
            cold_damage.push((layer.id, radius));
            layer.watercolor = Some(next);
        }
        for (id, radius) in cold_damage {
            for coordinate in self.native_color_coordinates(id) {
                dirty = dirty.union(page_rect(coordinate).expand(radius, self.document_extent));
            }
        }
        dirty
    }

    fn watercolor_binding_with_colors(
        &self,
        layer: &PaintLayer,
        coordinate: [u32; 2],
        preview: bool,
        color_views: &[&wgpu::TextureView],
    ) -> wgpu::BindGroup {
        let mut wetness_views = Vec::with_capacity(5);
        for [offset_x, offset_y] in [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]] {
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
                page.map(|page| &page.active().view)
                    .unwrap_or(&self.empty_scalar_view)
            };
            wetness_views.push(view);
        }
        views_group(&self.device, "layer watercolor pigment and wetness neighborhood", &self.watercolor_layout,
            color_views.iter().chain(&wetness_views).copied())
    }

    fn encode_clear(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        view: &wgpu::TextureView,
        label: &'static str,
    ) {
        self.encode_clear_value(encoder, view, label, 0.);
    }
    fn encode_clear_value(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        view: &wgpu::TextureView,
        label: &'static str,
        value: f32,
    ) {
        for page in &self.preview_pages {
            if page.primary.preview.get().is_some() && page.primary.view==*view {page.primary.preview.set(None);}
            if let Some(secondary)=&page.secondary && secondary.preview.get().is_some() && secondary.view==*view {secondary.preview.set(None);}
        }
        let _pass = encoder.color_pass(
            label,
            view,
            wgpu::LoadOp::Clear(wgpu::Color { r: value as f64, g: value as f64, b: value as f64, a: value as f64 }),
        );
    }

    fn encode_batch(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        target: &wgpu::TextureView,
        scissor: PixelRect,
        target_offset: u32,
    ) -> Result<(), GpuRasterError> {
        self.encode_batch_to_target(encoder, batch_index, batch, target, scissor,
            self.paint_target_binding(&batch.style), target_offset)
    }

    #[expect(clippy::too_many_arguments, reason = "Brush batch encoding keeps target, scissor, bind group, and instance offset explicit")]
    fn encode_batch_to_target(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        target: &wgpu::TextureView,
        scissor: PixelRect,
        binding: &wgpu::BindGroup,
        target_offset: u32,
    ) -> Result<(), GpuRasterError> {
        let start = batch.first_dab as u64 * mem::size_of::<DabGpu>() as u64;
        let end = start
            .checked_add(batch.dab_count as u64 * mem::size_of::<DabGpu>() as u64)
            .ok_or(GpuRasterError::InvalidDabRange)?;
        if end > self.dab_capacity_bytes {
            return Err(GpuRasterError::InvalidDabRange);
        }
        let plan = BrushPassPlan::for_device(&batch.style, &self.device);
        let direct = plan
            .direct
            .expect("destination brushes use the material encoder");
        let pipeline = self.pipelines.direct(direct);
        let key = Self::texture_set_key(&batch.style);
        let texture_set = self
            .texture_sets
            .iter()
            .find(|set| set.key == key)
            .expect("advanced brush resources are prepared before encoding");
        let mut pass = encoder.color_pass("layer raster brush batch", target, wgpu::LoadOp::Load);
        pass.set_scissor_rect(
            scissor.min_x(),
            scissor.min_y(),
            scissor.width(),
            scissor.height(),
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(
            0,
            &self.style_bind_group,
            &[batch_index as u32 * self.style_stride as u32],
        );
        pass.set_bind_group(1, binding, &[target_offset]);
        pass.set_bind_group(2, &texture_set.bind_group, &[]);
        pass.set_vertex_buffer(0, self.dab_buffer.slice(start..end));
        pass.draw(0..4, 0..batch.dab_count);
        Ok(())
    }

    fn prepare_target_selection(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        style: &layer_render::DabStyle,
        extent: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        if let Some(geometry) = &style.selection {
            self.selection_clip
                .prepare(&self.device, encoder, extent, geometry)?;
            if self.selection_clip.binding.is_none() {
                self.selection_clip.binding = Some(create_target_bind_group(
                    &self.device,
                    &self.target_layout,
                    &self.target_buffer,
                    self.selection_clip.buffer.as_ref().unwrap(),
                ));
            }
        }
        Ok(())
    }

    fn paint_target_binding(&self, style: &layer_render::DabStyle) -> &wgpu::BindGroup {
        if style.selection.is_some() {
            self.selection_clip
                .binding
                .as_ref()
                .expect("selection prepared before painting")
        } else {
            &self.target_bind_group
        }
    }

    fn encode_brush_batch(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        context: BrushEncodingContext<'_>,
    ) -> Result<(), GpuRasterError> {
        let damage = batch_pixel_rect(batch, context.target_extent);
        if batch.dab_count == 0 || damage.is_empty() {
            return Ok(());
        }
        self.prepare_target_selection(encoder, &batch.style, self.target_extent(batch.target))?;

        let plan = BrushPassPlan::for_device(&batch.style, &self.device);
        if plan.requires_destination() {
            let preview = context.target.is_preview();
            let watercolor_first = plan.state.watercolor_wetness
                && is_first_watercolor_update_batch(context.batches, batch_index);
            let watercolor_last = plan.state.watercolor_wetness
                && is_last_watercolor_update_batch(context.batches, batch_index);
            let watercolor_damages = (watercolor_first || watercolor_last).then(|| {
                watercolor_update_damages(context.batches, batch_index, context.target_extent)
            });
            if watercolor_first {
                self.begin_watercolor_wetness_update(
                    encoder,
                    batch.target,
                    watercolor_damages
                        .as_deref()
                        .expect("first watercolor update has damage"),
                    preview,
                )?;
            }
            self.encode_material_batch(encoder, batch_index, batch, context)?;
            if watercolor_last {
                let damages = watercolor_damages
                    .as_deref()
                    .expect("last watercolor update has damage");
                self.finish_watercolor_wetness_update(batch.target, damages, preview)?;
                self.encode_watercolor_transport(encoder, batch_index, batch, damages, preview)?;
            }
            return Ok(());
        }

        let pages = match context.target {
            BrushEncodingTarget::Persistent => {
                &self
                    .paint_layers
                    .iter()
                    .find(|layer| layer.id == batch.target)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?
                    .pages
            }
            BrushEncodingTarget::Preview { .. } => &self.preview_pages,
        };
        for tile in context.tiles {
            let page = pages
                .iter()
                .find(|page| page.coordinate == tile.coordinate)
                .expect("brush pages are prepared before encoding");
            self.encode_batch(
                encoder,
                batch_index,
                batch,
                &page.active().view,
                tile.local,
                self.layer_target_offset(batch.target, tile.coordinate),
            )?;
        }
        Ok(())
    }

    fn begin_watercolor_wetness_update(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        layer_id: SourceTarget,
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
        layer_id: SourceTarget,
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

    fn transport_bind_group(
        &mut self,
        layer_id: SourceTarget,
        coordinate: [u32; 2],
        preview: bool,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<wgpu::BindGroup, GpuRasterError> {
        let offsets = [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]];
        self.prepare_raw_neighborhood(layer_id, coordinate, offsets.map(Some), preview, encoder)?;
        let layer = self
            .paint_layers
            .iter()
            .find(|l| l.id == layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?;
        let sources = self.source_tiles.borrow();
        let colors = self.raw_layer_neighborhood(&sources, layer, coordinate, offsets.map(Some), preview);
        let wetness_pages = if preview {
            &self.preview_watercolor_wetness_pages
        } else {
            &layer.watercolor_wetness_pages
        };
        let wetness = offsets.map(|[dx, dy]| {
            let [x, y] = [coordinate[0] as i32 + dx, coordinate[1] as i32 + dy];
            if x < 0 || y < 0 {
                return &self.empty_scalar_view;
            }
            wetness_pages
                .iter()
                .find(|p| p.coordinate == [x as u32, y as u32])
                .map(|p| &p.active().view)
                .unwrap_or(&self.empty_scalar_view)
        });
        Ok(views_group(&self.device, "layer watercolor transport sources", &self.transport_layout,
            colors.iter().chain(&wetness).copied()))
    }

    fn encode_watercolor_transport(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
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
            color_destination_secondary: bool,
            wetness_destination_secondary: bool,
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

        // Synchronize the first stage's destinations in one copy batch. Later
        // stages overwrite the same scissor, leaving identical pixels outside.
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
                        if step == 0 {
                            color.active().copy_to(color.surface(!color.active_secondary), encoder);
                            wetness.active().copy_to(wetness.inactive(), encoder);
                        }
                        Job {
                            coordinate: *coordinate,
                            color_destination_secondary: !color.active_secondary,
                            wetness_destination_secondary: !wetness.active_secondary,
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                let layer = self
                    .paint_layers
                    .iter()
                    .find(|layer| layer.id == batch.target)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
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
                        if step == 0 {
                            color.active().copy_to(color.surface(!color.active_secondary), encoder);
                            wetness.active().copy_to(wetness.inactive(), encoder);
                        }
                        Job {
                            coordinate: *coordinate,
                            color_destination_secondary: !color.active_secondary,
                            wetness_destination_secondary: !wetness.active_secondary,
                        }
                    })
                    .collect::<Vec<_>>()
            };

            for job in &jobs {
                let bind_group =
                    self.transport_bind_group(batch.target, job.coordinate, preview, encoder)?;
                let (color_destination, wetness_destination) =
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
                            color.surface(job.color_destination_secondary),
                            wetness.surface(job.wetness_destination_secondary),
                        )
                    } else {
                        let layer = self
                            .paint_layers
                            .iter()
                            .find(|layer| layer.id == batch.target)
                            .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
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
                            color.surface(job.color_destination_secondary),
                            wetness.surface(job.wetness_destination_secondary),
                        )
                    };
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
                pass.set_scissor_rect(local.min_x(), local.min_y(), local.width(), local.height());
                pass.set_pipeline(&self.pipelines.watercolor_transport[step as usize]);
                pass.set_bind_group(
                    0,
                    &self.style_bind_group,
                    &[batch_index as u32 * self.style_stride as u32],
                );
                pass.set_bind_group(
                    1,
                    self.paint_target_binding(&batch.style),
                    &[self.layer_target_offset(batch.target, job.coordinate)],
                );
                pass.set_bind_group(2, &bind_group, &[]);
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
                    .find(|layer| layer.id == batch.target)
                    .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
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
        encoder: &mut crate::submission::CommandEncoder,
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
            let mut pass = encoder.color_pass(
                "layer brush reservoir exchange",
                target,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(&self.pipelines.reservoir);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.target, coordinate)],
            );
            pass.set_bind_group(2, source_bind_group, &[0]);
            pass.set_bind_group(3, &texture_set.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.reservoir.active_secondary = !self.reservoir.active_secondary;
        Ok(())
    }

    fn encode_stroke_edge(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
    ) -> Result<(), GpuRasterError> {
        self.prepare_target_selection(encoder, &batch.style, self.target_extent(batch.target))?;
        struct Job {
            coordinate: [u32; 2],
            destination_secondary: bool,
            bind_group: wgpu::BindGroup,
        }

        let layer_index = self
            .paint_layers
            .iter()
            .position(|layer| layer.id == batch.target)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
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
                bind_group: views_group(&self.device, "layer post-stroke edge sources", &self.edge_layout,
                    std::iter::once(&color_page.active().view).chain(coverage_views.iter().copied())),
            });
        }

        for job in &jobs {
            let page = self.paint_layers[layer_index]
                .pages
                .iter()
                .find(|page| page.coordinate == job.coordinate)
                .expect("edge paint page remains live while encoding");
            let destination = page.surface(job.destination_secondary);
            // The full-page shader copies source color outside the edge
            // band, so a preceding source-to-destination copy would be redundant.
            let mut pass = encoder.color_pass(
                "layer post-stroke edge page",
                &destination.view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            );
            pass.set_pipeline(&self.pipelines.stroke_edge);
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.target, job.coordinate)],
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

}

impl WgpuRasterizer {
    /// Source assets and cursor geometry for a host with a separate render thread.
    /// This shares immutable bytes and never downloads canvas pixels.
    pub fn brush_sources(&self) -> std::collections::HashMap<AssetId, layer_render::BrushSource> {
        self.masks
            .iter()
            .map(|mask| (mask.id.clone(), layer_render::BrushSource {
                image: layer_core::ProjectAsset {
                    extent: [mask.extent[0], mask.extent[1]],
                    format: PixelFormat::R8Unorm,
                    bytes: mask.source.clone(),
                },
                outline: self.tip_outline(&mask.id).unwrap().clone(),
            }))
            .collect()
    }
}

impl WgpuRasterizer {
    fn replace_display_selection(
        &mut self,
        selection: Option<&layer_core::Selection>,
    ) -> Result<(), GpuRasterError> {
        if let Some(selection) = selection
            && let layer_core::SelectionShape::Pixels(pixels) = &selection.shape
        {
            if selection.affine.inverse().is_none() {
                return Err(GpuRasterError::InvalidTransform(
                    "Invalid selection transform",
                ));
            }
            if let Some((old, _)) = self.display_selection.as_mut().filter(|(old, _)| old.shape == selection.shape) {
                old.clone_from(selection);
            } else {
                if 32 + pixels.words().len() as u64 * 4
                    > self.device.limits().max_storage_buffer_binding_size
                {
                    return Err(GpuRasterError::SizeOverflow);
                }
                let buffer = self.selection_clip.pixel_buffer(&self.device, pixels);
                self.display_selection = Some((selection.clone(), buffer));
            }
        } else if let Some(selection) = selection.filter(|_| self.selection_overlay.is_some_and(|o|o.active)) {
            if self.display_selection.as_ref().is_none_or(|(old,_)| old != selection) {
                if let Some(startup) = &self.startup {
                    startup.compiler.check()?;
                    if !startup.compiler.require(self.selection_clip.pipelines().into_iter().take(2), startup::BRUSH) { return Ok(()); }
                }
                let mut encoder = crate::submission::CommandEncoder::new(&self.device,
                    &wgpu::CommandEncoderDescriptor { label: Some("selection overlay") });
                self.selection_clip.prepare(&self.device, &mut encoder, self.document_extent, &Arc::new(selection.clone()))?;
                let input = self.selection_clip.buffer.as_ref().unwrap();
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("retained selection overlay"), size: input.size(),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
                });
                encoder.copy_buffer_to_buffer(input,0,&buffer,0,input.size());
                encoder.submit(&self.queue);
                self.display_selection = Some((selection.clone(),buffer));
            }
        } else {
            self.display_selection = None;
        }
        Ok(())
    }
}
/// Document pixels whose display a change of selection affects: wherever
/// either selection has coverage, or the whole document when only one of them
/// is inverted. Resampled coverage reaches one cell beyond its pixels.
fn outline_damage(a: Option<&layer_core::Selection>, b: Option<&layer_core::Selection>, extent: [u32; 2]) -> PixelRect {
    if a.is_some_and(|s| s.inverted) != b.is_some_and(|s| s.inverted) {
        return PixelRect::full(extent);
    }
    a.into_iter().chain(b).fold(PixelRect::EMPTY, |damage, selection| {
        let mut bounds = selection.bounds();
        if bounds.is_empty() {
            return damage;
        }
        let cell = selection.affine.0[..4].iter().fold(0f32, |m, v| m.max(v.abs())).ceil() + 1.;
        bounds.min.x -= cell;
        bounds.min.y -= cell;
        bounds.max.x += cell;
        bounds.max.y += cell;
        damage.union(pixel_rect(bounds, extent))
    })
}
pub(crate) struct PreparedImageUpload {
    id: layer_core::authored::PortableId,
    source: Arc<layer_core::color::source::SourceImage>,
    coordinate: [u32; 2],
    tile: source_access::RawTile,
}
impl WgpuRasterizer {
    pub(crate) fn image_decode_waiting(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.browser_image_result.borrow().is_some() { return false; }
        self.moving_decode.as_ref().is_some_and(|(_,queue)| queue.pending() && !queue.ready())
    }
    pub(crate) fn drain_image_decode(&mut self, encoder: &mut submission::CommandEncoder) -> Result<Option<PreparedImageUpload>, GpuRasterError> {
        #[cfg(target_arch = "wasm32")]
        if let Some(result) = self.browser_image_result.borrow_mut().take() {
            if let Some((_,queue)) = &mut self.moving_decode { queue.complete_external(result); }
        }
        let Some(prepared) = self.moving_decode.as_mut().and_then(|(_,queue)| queue.poll()) else { return Ok(None); };
        let prepared = match prepared {
            object_image_mips::PreparedImageWork::Image(prepared) => prepared,
            object_image_mips::PreparedImageWork::Coordinates {destination,pixels} => {
                if let Some(destination) = destination.upgrade() { *destination.lock().unwrap() = Some(pixels); }
                return Ok(None);
            },
        };
        let pixels = match prepared.pixels {
            Ok(pixels) => pixels,
            Err(error) => {
                self.failed_image_tiles.push((prepared.id, Arc::downgrade(&prepared.source), prepared.coordinate, self.device.working_space(), error.clone()));
                return Err(GpuRasterError::Color(error));
            }
        };
        let tile = scene::Scene::decode_prepared(self, encoder, &prepared.source, prepared.coordinate, pixels)?;
        Ok(Some(PreparedImageUpload {id:prepared.id, source:prepared.source, coordinate:prepared.coordinate, tile}))
    }
    pub(crate) fn drain_image_decodes(&mut self, encoder: &mut submission::CommandEncoder) -> Result<(), GpuRasterError> {
        self.drain_image_decode(encoder)?;
        while self.moving_decode.as_ref().is_some_and(|(_, queue)| queue.ready()) { self.drain_image_decode(encoder)?; }
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn await_image_decode(&self) -> Result<(), GpuRasterError> {
        while self.image_decode_waiting() {
            if self.snapshot_cancelled.as_ref().is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
            }
            std::thread::sleep(std::time::Duration::from_micros(250));
        }
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn await_image_decode(&self) -> Result<(), GpuRasterError> {
        std::future::poll_fn(|cx| {
            if self.snapshot_cancelled.as_ref().is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                return std::task::Poll::Ready(Err(GpuRasterError::Color("Snapshot capture cancelled".into())));
            }
            if !self.image_decode_waiting() { return std::task::Poll::Ready(Ok(())); }
            *self.browser_image_wake.borrow_mut() = Some(cx.waker().clone());
            if let Some(wake) = &self.snapshot_decode_wake { *wake.lock().unwrap() = Some(cx.waker().clone()); }
            std::task::Poll::Pending
        }).await
    }
    pub(crate) fn request_image_decode(&mut self, id: layer_core::authored::PortableId, source: Arc<layer_core::color::source::SourceImage>, coordinate: [u32; 2]) -> Result<bool, GpuRasterError> {
        #[cfg(target_arch="wasm32")]
        if self.browser_image_decoder.is_none() { return Err(GpuRasterError::Color("Image preparation worker unavailable".into())); }
        let space = self.device.working_space();
        let weak = Arc::downgrade(&source);
        if let Some((_,_,_,_,error)) = self.failed_image_tiles.iter().find(|(owner,stored,tile,context,_)| *owner == id && stored.ptr_eq(&weak) && *tile == coordinate && *context == space) {
            return Err(GpuRasterError::Color(error.clone()));
        }
        if self.moving_decode.as_ref().is_some_and(|(context,_)| *context != space) {
            self.moving_decode = None;
            #[cfg(target_arch = "wasm32")]
            { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
        }
        let requested = self.moving_decode.get_or_insert_with(|| (space, object_image_mips::MipDecodeQueue::new(&self.device))).1.request(id, source, coordinate);
        #[cfg(target_arch = "wasm32")]
        self.dispatch_browser_image_decode()?;
        Ok(requested)
    }
    pub(crate) fn request_nearest_coordinates(&mut self,inverse:[f64;6],size:[u32;2],first:u32,count:u32,destination:object_image_mips::CoordinateDestination)->Result<bool,GpuRasterError> {
        #[cfg(target_arch="wasm32")]
        if self.browser_nearest_coordinate_decoder.is_none() && (!self.snapshot_worker || self.browser_image_decoder.is_some()) {
            return Err(GpuRasterError::Color("Coordinate preparation worker unavailable".into()));
        }
        let space = self.device.working_space();
        if self.moving_decode.as_ref().is_some_and(|(context,_)| *context != space) {
            self.moving_decode = None;
            #[cfg(target_arch = "wasm32")]
            { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
        }
        let requested = self.moving_decode.get_or_insert_with(|| (space,object_image_mips::MipDecodeQueue::new(&self.device))).1
            .request_coordinates(inverse,size,first,count,destination);
        #[cfg(target_arch = "wasm32")]
        self.dispatch_browser_image_decode()?;
        Ok(requested)
    }
    #[cfg(target_arch = "wasm32")]
    fn dispatch_browser_image_decode(&mut self) -> Result<(), GpuRasterError> {
        if let Some(request) = self.moving_decode.as_mut().and_then(|(_,queue)| queue.take_external()) {
            let result = self.browser_image_result.clone();
            let wake = self.browser_image_wake.clone();
            let future: std::pin::Pin<Box<dyn std::future::Future<Output=object_image_mips::PreparedImageWork>>> = match request {
                object_image_mips::MipDecodeRequest::Image(request) => {
                    let callback = self.browser_image_decoder.clone().ok_or_else(|| GpuRasterError::Color("Image preparation worker unavailable".into()))?;
                    let space = self.device.working_space();
                    Box::pin(async move {
                        let pixels = callback(request.source.clone(),request.coordinate,space).await;
                        object_image_mips::PreparedImageWork::Image(object_image_mips::PreparedMipTile {id:request.id,source:request.source,coordinate:request.coordinate,pixels})
                    })
                },
                object_image_mips::MipDecodeRequest::Coordinates(request) => {
                    let callback = self.browser_nearest_coordinate_decoder.clone();
                    if callback.is_none() && (!self.snapshot_worker || self.browser_image_decoder.is_some()) {
                        return Err(GpuRasterError::Color("Coordinate preparation worker unavailable".into()));
                    }
                    Box::pin(async move {
                        let pixels = if let Some(callback) = callback {callback(request.inverse,request.size,request.first,request.count).await}
                            else {object_sampling::prepare_nearest_coordinates(request.inverse,request.size,request.first,request.count)};
                        object_image_mips::PreparedImageWork::Coordinates {destination:request.destination,pixels}
                    })
                },
            };
            wasm_bindgen_futures::spawn_local(async move {
                *result.borrow_mut() = Some(future.await);
                let signal = wake.borrow_mut().take();
                if let Some(waker) = signal { waker.wake(); }
            });
        }
        Ok(())
    }
    fn moving_images_pending(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if self.image_decode_waiting() {
            return self.moving_images_waiting;
        }
        self.moving_images_waiting || self.moving_images.pending()
    }
    fn prepare_moving_projection(&mut self, packet:FramePacket<'_>) -> Result<(),GpuRasterError> {
        let available = !self.snapshot_worker;
        #[cfg(target_arch = "wasm32")]
        let available = available && self.browser_image_decoder.is_some();
        MovingProjection::update(&mut self.moving_projection, packet.scene, packet.view.document_to_surface,
            self.device.working_space(), available, &mut self.failed_image_tiles)
    }
    fn prepare_moving_images(&mut self, packet: FramePacket<'_>, encoder: &mut submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        self.prepare_moving_projection(packet)?;
        let mut images = std::mem::take(&mut self.moving_images);
        let completed = images.completed_images;
        let budget = if packet.dab_batches.is_empty() && self.moving_layer.is_none() { MOVING_IMAGE_IDLE_STEPS } else { MOVING_IMAGE_INPUT_STEPS };
        let result = (|| {
            let budget_bytes = self.source_tiles.borrow().mip_budget();
            let plan = images.plan(self, &self.moving_projection.as_ref().unwrap().requests, budget_bytes, encoder)?;
            self.moving_images_waiting = plan.waiting;
            if !self.source_tiles.borrow_mut().reserve_mips(plan.bytes, encoder)? {
                self.moving_images_waiting = true;
                return Ok(false);
            }
            images.allocate(self, plan);
            if self.moving_decode.as_ref().is_some_and(|(space,_)| *space != self.device.working_space()) {
                self.moving_decode = None;
                #[cfg(target_arch = "wasm32")]
                { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
            }
            let mut steps = 0;
            while steps < budget && let Some(prepared) = self.drain_image_decode(encoder)? {
                if images.accepts_tile(prepared.id, &prepared.source, prepared.coordinate) {
                    images.write_tile(self, encoder, prepared.id, &prepared.source, prepared.coordinate, &prepared.tile.texture)?;
                }
                steps += 1;
            }
            while steps < budget {
                if let Some((id, source, coordinate)) = images.next_build() {
                    let cached = self.source_tiles.borrow().prepared_view(&source, coordinate).map(|view| view.texture().clone());
                    let Some(texture) = cached else {
                        for (id, source, coordinate) in images.upcoming(MOVING_IMAGE_PREFETCH) {
                            if self.source_tiles.borrow().prepared_view(&source, coordinate).is_none() && !self.request_image_decode(id, source, coordinate)? { break; }
                        }
                        break;
                    };
                    images.write_tile(self, encoder, id, &source, coordinate, &texture)?;
                } else if !images.advance_coarse(self, encoder) { break; }
                steps += 1;
            }
            Ok(images.completed_images != completed)
        })();
        self.moving_images = images;
        result
    }
    fn hold_background(&mut self, refinement: bool) {
        self.background_refinement = refinement;
        self.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ready = self.background_ready.clone();
        self.queue.on_submitted_work_done(move || ready.store(true, std::sync::atomic::Ordering::Release));
    }
}
impl CanvasRenderer for WgpuRasterizer {
    fn shader_input(&mut self) { WgpuRasterizer::shader_input(self); }
    fn shader_idle(&mut self, idle: bool, speculative_idle: bool) { WgpuRasterizer::shader_idle(self, idle, speculative_idle); }
    fn shaders_need_update(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.startup_needs_update(document, brush, transform)
    }
    fn document_color(&self) -> layer_core::color::DocumentColor { self.document_color }
    fn supports_tiled_sources(&self) -> bool {
        self.native_edit.is_some()
    }
    fn supports_raster_damage(&self) -> bool { true }
    fn max_document_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }
    fn raster_dependencies_ready(&mut self, packet: FramePacket<'_>) -> bool {
        self.raster_restore_ready(packet) && self.bake_analyses_ready(packet) && self.retouch_analyses_ready(packet)
    }
    fn has_pending_submission(&self) -> bool { self.settling.is_some() }
    fn poll_pending(&mut self, view: layer_render::ViewState) -> Result<(), Self::Error> {
        let Some(mut pending) = self.settling.take() else { return Ok(()); };
        let moving = pending.view != view;
        pending.view = view;
        #[cfg(not(target_arch = "wasm32"))]
        if let Err(error) = self.device.poll(wgpu::PollType::Poll) {
            self.retouch = None;
            return Err(GpuRasterError::WaitFailed(error.to_string()));
        }
        if moving || !self.background_ready.load(std::sync::atomic::Ordering::Acquire) || !self.raster_ready() {
            self.settling = Some(pending);
            return Ok(());
        }
        let healing = self.retouch.as_ref().is_some_and(|s| s.heal_pending());
        if !healing && pending.capture.is_none() { pending.capture = self.prepare_native_rasters(pending.frame.scene.view())?; }
        let work = healing || pending.capture.as_ref().is_some_and(|capture| !capture.complete());
        if work {
            let mut encoder = submission::CommandEncoder::new(&self.device, &wgpu::CommandEncoderDescriptor { label: Some("healing settle") });
            self.telemetry.begin(&self.device, &self.queue, &mut encoder, self.metrics.submissions);
            let stepped = if healing {
                let mut sources = self.retouch_sources();
                let result = sources.step_heal(self, &mut encoder);
                self.retouch = Some(sources);
                result?
            } else if let Some(capture) = &mut pending.capture { self.step_native_rasters(capture, &mut encoder)? } else { false };
            self.telemetry.end(&mut encoder);
            debug_assert!(stepped);
            self.uploads.finish(&encoder);
            self.metrics.command_passes += encoder.pass_count();
            self.last_submission = Some(encoder.submit(&self.queue));
            self.telemetry.submitted(&self.queue);
            self.hold_background(false);
            self.settling = Some(pending);
        } else {
            let mut packet = pending.frame.packet(self.document_extent);
            packet.view = view;
            packet.composite_all = pending.capture.is_none();
            let native = pending.capture.take().map(|capture| capture.frame);
            self.submit_frame(packet, native)?;
            self.release_unused_retouch();
        }
        Ok(())
    }
    fn can_submit(&self) -> bool {
        use std::sync::atomic::Ordering;
        if !self.background_ready.load(Ordering::Acquire) {
            #[cfg(not(target_arch = "wasm32"))]
            let _ = self.device.poll(wgpu::PollType::Poll);
        }
        self.settling.is_none() && self.raster_ready()
            && (self.background_ready.load(Ordering::Acquire) || self.background_refinement)
    }
    fn can_capture_raster(&self) -> bool {
        self.settling.is_none() && self.raster_ready()
    }
    fn prepare_moving_layer(&mut self, layer: Option<OccurrenceHandle>) {
        if self.moving_layer != layer {
            self.moving_layer = layer;
            self.artwork_frame = None;
        }
    }
    fn preflight_image_object_affine(&self, scene: SceneView<'_>, object: layer_core::authored::ImageObjectHandle,
        affine: layer_core::authored::Affine64, view: layer_render::ViewState,
    ) -> Result<(), Self::Error> {
        object_sampling::preflight_object_affine(scene, object, affine, view, self.device.limits().max_storage_buffer_binding_size)
    }

    fn prepare_moving_pixels(&mut self, pixels: Option<(SourceTarget, layer_core::Selection)>) {
        if self.moving_pixels != pixels { self.transforms.as_mut().unwrap().release_standby(); }
        self.moving_pixels = pixels;
    }
    fn has_pending_work(&self) -> bool {
        self.analysis_dirty || self.settling.is_some() || self.awaiting_meshes || self.retouch.as_ref().is_some_and(|retouch| retouch.pending() && (!cfg!(target_arch="wasm32") || !self.image_decode_waiting()))
            || self.navigator.pending()
            || self.moving_images_pending()
            || self.moving_decode.as_ref().is_some_and(|(_,queue)|queue.pending())
                && (!cfg!(target_arch="wasm32") || !self.image_decode_waiting())
            || (self.object_deferred || self.scene.as_ref().is_some_and(|scene| scene.objects_pending()))
                && (!cfg!(target_arch="wasm32") || !self.image_decode_waiting())
            || self.scale_display.as_ref().is_some_and(|cache| cache.has_pending_work(self))
    }
    fn prepare_retouch(&mut self, retouch: Option<&layer_render::RetouchPreparation>) {
        self.prepare_retouch_sources(retouch);
    }
    fn take_retouch_miss(&mut self) -> Option<StrokeId> {
        self.retouch.as_mut()?.take_miss()
    }
    fn retouch_waiting(&self) -> Option<StrokeId> {
        self.retouch.as_ref()?.waiting()
    }
    fn retire_stroke_sources(&mut self) {
        if let Some(retouch) = &mut self.retouch {
            retouch.retire_stroke();
        }
    }
    fn set_transform_preview(
        &mut self,
        preview: Option<&layer_render::TransformPreview>,
    ) -> Result<(), Self::Error> {
        if preview.is_some_and(|p| p.transform.validate().is_err()) {
            return Err(GpuRasterError::InvalidTransform(
                "Invalid preview transform",
            ));
        }
        if self.transform_preview.as_ref() != preview {
            self.transform_preview = preview.cloned();
        }
        Ok(())
    }
    fn request_region(
        &mut self,
        request: layer_render::RegionRequest,
    ) -> Result<bool, Self::Error> {
        self.start_region(request)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionResult, Self::Error>> {
        self.poll_region()
    }
    fn cancel_region(&mut self) {
        WgpuRasterizer::cancel_region(self);
    }
    fn set_selection_outline(
        &mut self,
        selection: Option<&layer_core::Selection>,
    ) -> Result<(), Self::Error> {
        if self.selection_painter.as_ref().is_some_and(|p| p.active.is_some()) { return Ok(()); }
        let before = self.display_selection.as_ref().map(|(s, _)| s.clone());
        let result = self.replace_display_selection(selection);
        let after = self.display_selection.as_ref().map(|(s, _)| s);
        if before.as_ref() != after {
            self.display_selection_damage = outline_damage(before.as_ref(), after, self.document_extent);
            self.display_selection_revision = self.display_selection_revision.wrapping_add(1);
        }
        result
    }
    fn paint_selection(&mut self, update: &layer_render::SelectionPaint) -> Result<bool,Self::Error> { self.update_selection_paint(update) }
    fn take_selection_paint(&mut self) -> Option<Result<layer_render::SelectionPaintResult,Self::Error>> { self.poll_selection_paint() }
    fn cancel_selection_paint(&mut self) { self.selection_painter = None; self.display_selection = None; }
    fn set_quick_mask_thumbnail(&mut self, selection: Option<&layer_core::Selection>) {
        if let Some(selection) = selection { self.selection_previews.definitions.insert(SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID), selection.clone()); }
        else { self.selection_previews.definitions.remove(&SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID)); }
    }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) { self.selection_overlay = overlay; }
    fn set_crop_overlay(&mut self, overlay: Option<layer_render::CropOverlay>) { self.crop_overlay = overlay; }
    fn set_clipping_preview(&mut self, shadows: bool, highlights: bool) { self.clipping_preview = [shadows, highlights]; }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        self.telemetry.enabled = enabled;
    }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        let mut t = self.telemetry.snapshot(&self.device, &self.queue);
        let m = &self.metrics;
        t.submissions = m.submissions;
        t.dabs = m.dabs;
        t.dirty_pixels = m.composited_pixels;
        t.resident_bytes = self.raster_staging_bytes()
            + self.source_tiles.borrow().gpu_bytes()
            + self.moving_images.storage_bytes()
            + m.paint_storage_bytes
            + m.preview_storage_bytes
            + m.destination_storage_bytes
            + m.paint_state_storage_bytes
            + m.composite_storage_bytes
            + self.thumbnails.storage_bytes()
            + self.portable_blend.byte_len()
            + 160 + self.dry_records.storage_bytes()
            + self.material_gather.as_ref().map_or(0, material_sources::Gather::storage_bytes)
            + self.device.effect_resources.lock().unwrap().bytes()
            + *self.device.analysis_memory.lock().unwrap()
            + m.retouch_storage_bytes
            + self
                .transforms
                .as_ref()
                .map_or(0, paint_transform::PaintTransforms::storage_bytes)
            + self.layer_masks.pages.values().chain(self.layer_masks.command_pages.values()).chain(self.layer_masks.snapshots.values().flat_map(|snapshot| snapshot.pages.values())).map(|p| texture_bytes(&p.texture)).sum::<u64>();
        if let Some(scene) = &self.scene {
            t.effect_passes = scene.effect_passes;
            t.compiled_effects = scene.effects.compilations;
            t.resident_bytes += scene.scratch_bytes();
        }
        if let Some(native) = &self.native_edit {
            t.resident_bytes += native.storage_bytes();
        }
        if let Some(previews) = &self.filter_previews {
            t.resident_bytes += previews.storage_bytes();
        }
        t
    }
    fn request_thumbnail(&mut self, request_id: u64, target: layer_render::ThumbnailTarget) -> Result<(), Self::Error> {
        self.start_thumbnail(request_id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.thumbnails.take()
    }
    fn request_effect_analysis(&mut self, query: layer_core::ArtworkQuery) -> Result<bool, Self::Error> {
        if self.analysis_job.is_some() || self.analysis_candidate.is_some() { return Ok(false); }
        self.analysis_job = Some(effect_analysis::Job::start(self.snapshot_gpu(), query).map_err(GpuRasterError::Effect)?);
        Ok(true)
    }
    fn take_effect_analysis(&mut self) -> Option<Result<(), Self::Error>> {
        let result = self.analysis_job.as_mut()?.take()?;
        self.analysis_job = None;
        Some(result.map(|candidate| self.analysis_candidate = Some(candidate)).map_err(GpuRasterError::Effect))
    }
    fn accept_effect_analysis(&mut self) -> Result<(), Self::Error> {
        if let Some(candidate) = self.analysis_candidate.take() { self.apply_effect_analysis(candidate); }
        Ok(())
    }
    fn cancel_effect_analysis(&mut self) { self.analysis_job = None; self.analysis_candidate = None; }
    fn retain_effect_analyses(&mut self, layers: &[OccurrenceHandle]) -> Result<(), Self::Error> {
        let before = self.effect_analyses.len();
        self.effect_analyses.retain(|entry| layers.contains(&entry.layer()));
        if self.effect_analyses.len() != before { self.analysis_changed(); }
        Ok(())
    }
    fn request_snapshot(&mut self, request: layer_render::SnapshotRequest) -> Result<bool, Self::Error> {
        if self.snapshot_job.is_some() { return Ok(false); }
        #[cfg(not(target_arch = "wasm32"))]
        let job = snapshot::SnapshotJob::start(self.snapshot_gpu(), request);
        #[cfg(target_arch = "wasm32")]
        let job = {
            let mut request = request;
            let context = self.evaluation_context();
            let query = match &mut request {
                layer_render::SnapshotRequest::ArtworkSample(request) => Some(&mut request.query),
                layer_render::SnapshotRequest::ArtworkStatistics(request) => Some(&mut request.query),
                layer_render::SnapshotRequest::LevelsStatistics(query) => Some(query), _ => None,
            };
            if let Some(query) = query { query.set_context(context); }
            snapshot::SnapshotJob::start(self.snapshot_worker_callback.clone()
                .ok_or_else(|| GpuRasterError::Color("Snapshot worker unavailable".into()))?, request)
        };
        self.snapshot_job = Some(job.map_err(GpuRasterError::Color)?);
        Ok(true)
    }
    fn take_snapshot(&mut self) -> Option<Result<layer_render::SnapshotResult, Self::Error>> {
        let result = self.snapshot_job.as_mut()?.take()?;
        self.snapshot_job = None;
        Some(result.map_err(GpuRasterError::Color))
    }
    fn cancel_snapshot(&mut self) { self.snapshot_job = None; }
    fn request_color_sample(
        &mut self,
        request: layer_render::ColorSampleRequest,
    ) -> Result<bool, Self::Error> {
        self.start_color_sample(request)
    }
    fn take_color_sample(&mut self) -> Option<Result<layer_render::ColorSample, Self::Error>> {
        self.poll_color_sample()
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
    fn cancel_filter_previews(&mut self) {
        self.cancel_filter_preview_request();
    }
    fn tip_outline(&self, asset: &AssetId) -> Option<&layer_render::TipOutline> {
        let mask = self.mask(asset).ok()?;
        Some(mask.outline.get_or_init(|| {
            let [width, height, stride] = mask.extent;
            layer_render::mask_outline(width, height, stride, &mask.source)
        }))
    }
    fn tip_mask(&self, asset: &AssetId) -> Option<HostImage<'_>> {
        let mask = self.mask(asset).ok()?;
        let [width, height, stride] = mask.extent;
        Some(HostImage { width, height, stride, format: PixelFormat::R8Unorm, bytes: &mask.source })
    }
    type Error = GpuRasterError;

    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), Self::Error> {
        if width == 0 || height == 0 {
            return Err(GpuRasterError::InvalidExtent);
        }
        Ok(())
    }

    fn prepare_asset(&mut self, asset: &AssetId, image: HostImage<'_>) -> Result<(), Self::Error> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if image.width > limit || image.height > limit {
            return Err(GpuRasterError::InvalidImage);
        }
        let source = layer_core::ProjectAsset::copy_rows(
            [image.width, image.height],
            image.format,
            image.stride as usize,
            image.bytes,
        )
        .map_err(|_| GpuRasterError::InvalidImage)?;
        self.upload_mask_source(asset, &source);
        Ok(())
    }

    fn release_asset(&mut self, asset: &AssetId) {
        self.masks.retain(|stored| stored.id != *asset);
        self.texture_sets.clear();
    }

    fn evaluation_context(&self) -> EvaluationContext { self.submitted_context.clone().unwrap_or_default() }
    fn seed_evaluation_context(&mut self, context: EvaluationContext) { self.submitted_context = Some(context); self.effect_clocks.clear(); }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> { self.submit_frame(packet, None) }
}
impl WgpuRasterizer {
    fn clear_preview_stamps(&self) {
        for page in &self.preview_pages {page.primary.preview.set(None);if let Some(secondary)=&page.secondary {secondary.preview.set(None);}}
    }
    fn submit_frame(&mut self, packet: FramePacket<'_>, native_commit: Option<raster::native_edit::NativeFrame>) -> Result<(), GpuRasterError> {
        let submitted = self.metrics.submissions;
        if let Err(error)=self.submit_frame_inner(packet, native_commit) {
            self.clear_preview_stamps();
            return Err(error);
        }
        if self.metrics.submissions != submitted {
            let context = self.submitted_context.as_ref().unwrap();
            self.effect_clocks.retain(|handle, _| packet.scene.effect(*handle).is_some());
            for ((handle, effect), (_, phase)) in packet.scene.order().iter().filter_map(|&handle|
                packet.scene.effect(handle).map(|effect| (handle, effect))).zip(context.phases.iter()) {
                let clock = self.effect_clocks.entry(handle).or_insert_with(|| (effect.program.id.clone(), Default::default()));
                if clock.0 != effect.program.id { clock.0 = effect.program.id.clone(); }
                clock.1 = layer_core::EffectClock::at(effect, packet.time_seconds, *phase);
            }
        }
        Ok(())
    }
    fn frame_context(&self, packet: FramePacket<'_>) -> EvaluationContext {
        let seed = self.submitted_context.as_ref().unwrap_or(&packet.scene.output().context);
        let phases = || packet.scene.order().iter().filter_map(|&handle| {
            let effect = packet.scene.effect(handle)?;
            let target = packet.scene.effect_handle(handle)?;
            let captured = packet.scene.evaluation_context().and_then(|context|
                context.phases.iter().find(|(h, _)| *h == target).map(|(_, phase)| *phase));
            let phase = captured.unwrap_or_else(|| {
                let mut clock = match self.effect_clocks.get(&handle) {
                    Some((id, clock)) if *id == effect.program.id => clock.clone(),
                    Some(_) => Default::default(),
                    None => seed.phases.iter().find(|(h, _)| *h == target).map_or_else(Default::default,
                        |(_, phase)| layer_core::EffectClock::at(effect, seed.elapsed, *phase)),
                };
                clock.advance(effect, packet.time_seconds)
            });
            Some((target, phase))
        });
        let phases = if seed.phases.iter().map(|(h, phase)| (*h, phase.to_bits()))
            .eq(phases().map(|(h, phase)| (h, phase.to_bits()))) {
            seed.phases.clone()
        } else {
            let mut retained = Vec::with_capacity(packet.scene.artwork().effects.len());
            retained.extend(phases());
            Arc::new(retained)
        };
        EvaluationContext { elapsed: packet.time_seconds, phases }
    }

    fn submit_frame_inner(&mut self, packet: FramePacket<'_>, settled_commit: Option<raster::native_edit::NativeFrame>) -> Result<(), GpuRasterError> {
        let analysis_dirty = std::mem::take(&mut self.analysis_dirty);
        let packet = FramePacket {composite_all: packet.composite_all || analysis_dirty, ..packet};
        let packet = FramePacket { inspect_mask: if self.clipping_preview.iter().any(|enabled| *enabled) { None } else { packet.inspect_mask }, ..packet };
        if packet.document_extent.iter().any(|n| *n > self.max_document_dimension()) {
            return Err(GpuRasterError::ExtentUnsupported);
        }
        if !self.background_ready.load(std::sync::atomic::Ordering::Acquire) && !self.navigator.pending()
            && !self.moving_images_pending()
            && !self.object_deferred && !self.scene.as_ref().is_some_and(|scene| scene.objects_pending())
            && packet.commit_rasters && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty()
            && packet.dabs.is_empty() && packet.dab_batches.is_empty()
            && settled_commit.is_none() && self.transform_preview.is_none()
            && self.moving_layer.is_none() && self.moving_pixels.is_none()
            && self.artwork_frame.as_ref().is_some_and(|frame|
                frame.view == packet.view && frame.same_artwork(packet))
        { return Ok(()); }
        let display_request = scene::scale::request(self, packet)?;
        let mut trace_phase = performance_trace::Span::new(c"capy.prepare");
        if let Some(native) = &self.native_edit {
            // Reject unsupported global dependencies before clearing/restoring
            // paint, allocating the composite, or submitting any part of a frame.
            let resident = self.scale_display.as_ref().map_or(0, |c| c.resident_bytes());
            let admitted = scene::Scene::window_plan(packet.scene, packet.document_extent, native.image_pixel_budget(resident), self.device.limits().max_texture_dimension_2d,self.scene.as_ref());
            if resident > 0 && admitted.is_err() {
                scene::Scene::window_plan(packet.scene, packet.document_extent, native.image_pixel_budget(0), self.device.limits().max_texture_dimension_2d,self.scene.as_ref())?;
                self.scale_display = None;
            } else { admitted?; }
        }
        if source_access::placed_targets(packet.scene).any(|t| packet.scene.paint_base(t).is_some()) {
            // Source-backed photos own no paint initially. Prepare their bounded
            // capture spare pool during loading, before the first stroke needs it.
            self.prepare_source_backing()?;
        }
        self.transform_damage.clear();
        self.document_damage.clear();
        let object_damage = (!analysis_dirty && !packet.reset_layers && packet.restore_rasters.is_empty()
            && packet.dabs.is_empty() && packet.dab_batches.is_empty()).then(|| {
            let old = self.artwork_frame.as_ref()?;
            if !old.same_evaluation(packet) { return None; }
            scene::Scene::object_edit_damage(old.scene.view().with_scope(&old.scope), packet.scene, packet.document_extent)
        }).flatten();
        if let Some(damage) = &object_damage { self.document_damage.extend(damage.regions.iter().copied()); }
        let packet = FramePacket { composite_all: packet.composite_all && object_damage.is_none(), ..packet };
        self.metrics.frame_composited_pages.clear();
        if let Some(t) = &mut self.transforms {
            t.begin_frame();
        }
        let context = self.frame_context(packet);
        let packet = FramePacket { scene: packet.scene.with_context(&context), ..packet };
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
        let mut cpu_phases = [0.; 6];
        self.metrics.frame_cpu_ms = [0.; 6];
        self.metrics.material_cpu_ms = [0.; 5];
        if let Some(scene) = &mut self.scene {
            scene.effect_passes = 0;
        }
        let requested_view = packet.view;
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
        }
        let original_batches = packet.dab_batches;
        let filtered: std::borrow::Cow<'_, [DabBatch]> = if original_batches
            .iter()
            .any(|b| layer_masks::MaskRenderer::is_mask(packet.scene, b.target))
        {
            std::borrow::Cow::Owned(
                original_batches
                    .iter()
                    .map(|b| {
                        let mut b = b.clone();
                        if layer_masks::MaskRenderer::is_mask(packet.scene, b.target) {
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
        let blending_changed = std::mem::replace(&mut self.blend_space, packet.blend_space) != packet.blend_space;
        let unchanged = self.artwork_frame.as_ref().is_some_and(|frame|
            frame.same_artwork(packet));
        if packet.commit_rasters && (!unchanged || packet.composite_all || packet.reset_layers || self.object_deferred
            || self.scene.as_ref().is_some_and(|scene|scene.objects_pending())
            || self.artwork_frame.as_ref().is_none_or(|frame|frame.view!=packet.view)) {
            let mut scene=self.scene.take().unwrap_or_else(||scene::Scene::new(self));
            let bounds=scene.cached_capture_window(packet.scene,display_request.plan.bounds);
            let side=if display_request.evaluation==scene::scale::Evaluation::Native {1.} else {f64::from(1u32<<display_request.plan.level)};
            let cold=scene.live_objects_cold(self,packet.scene,bounds,side);
            self.scene=Some(scene);
            if cold? && self.display_backup.is_none() && !self.object_deferred {
                let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
                self.display_backup=self.scale_display.as_ref().and_then(|cache|cache.backup(self,&mut encoder));
                if self.display_backup.is_some() {encoder.submit(&self.queue);}
            }
        }
        let resized = self.ensure_document_metadata(packet.document_extent, packet.scene)?;
        let display_rebuilt = if packet.commit_rasters {
            let previous = self.scale_display.take();
            let (cache, rebuilt) = scene::scale::Cache::select(previous, self, packet, display_request, unchanged);
            self.scale_display = Some(cache);
            rebuilt
        } else { false };
        let packet = FramePacket { composite_all: packet.composite_all || blending_changed, ..packet };
        self.prepare_selection_previews(packet)?;
        let mut batch_tiles = original_batches.iter().map(|batch| {
            let start = batch.first_dab as usize;
            let end = start.checked_add(batch.dab_count as usize)
                .ok_or(GpuRasterError::InvalidDabRange)?;
            let dabs = packet.dabs.get(start..end).ok_or(GpuRasterError::InvalidDabRange)?;
            Ok(brush_tiles::plan(batch, dabs, self.target_extent(batch.target)))
        }).collect::<Result<Vec<_>, GpuRasterError>>()?;
        self.trim_native_color_cache(packet.dab_batches, &batch_tiles);
        let reset = packet.reset_layers || resized;
        self.note_retouch_batches(packet.dab_batches, reset);
        if reset {
            self.layer_masks.pages.clear();
            if let Some(t) = &mut self.transforms {
                t.discard_preview();
            }
            for layer in &mut self.paint_layers {
                layer.pages.clear();
                layer.coverage_pages.clear();
                layer.watercolor_wetness_pages.clear();
                layer.watercolor = None;
            }
            self.preview_pages.clear();
            self.preview_coverage_pages.clear();
            self.preview_watercolor_wetness_pages.clear();
            self.preview_damage = PixelRect::EMPTY;
            self.preview_contact_tiles = None;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_contribution = false;
        }
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("layer incremental sparse frame"),
            },
        );
        self.telemetry.begin(&self.device, &self.queue, &mut encoder, self.metrics.submissions + 1);
        let moving_images_changed = self.prepare_moving_images(packet, &mut encoder)?;
        if moving_images_changed && let Some(scene) = &mut self.scene {
            scene.invalidate_object_previews(packet.document_extent);
        }
        let committed_preview = if self.transform_preview.is_none() {
            self.transforms.as_mut().unwrap().consume_commit(packet)
        } else {
            Vec::new()
        };
        if !packet.dabs.is_empty() || !packet.dab_batches.is_empty() || !packet.restore_rasters.is_empty() || reset {
            self.transforms.as_mut().unwrap().release_standby();
        }
        // Restore both targets before allocating or painting new pages. Cancel
        // removes disposable preview pages, which must not swallow a new stroke.
        if self.transforms.as_ref().is_some_and(|t| t.has_preview())
            && (self.transform_preview.is_none() || !packet.dab_batches.is_empty())
        {
            let mut transforms = self.transforms.take().unwrap();
            let result = transforms.cancel_preview(self, &mut encoder);
            self.transforms = Some(transforms);
            self.transform_damage.extend(result?);
            self.transform_preview = None;
        }
        let raster_damage = self.reconcile_rasters(
            FramePacket {
                dab_batches: original_batches,
                ..packet
            },
            reset,
            &batch_tiles,
        )?;
        self.transform_damage.extend(raster_damage);
        self.ensure_persistent_pages(packet.dab_batches, &batch_tiles)?;
        self.ensure_destination_companions(packet.dab_batches, &batch_tiles);
        self.ensure_paint_state_pages(packet.dab_batches, &batch_tiles)?;

        let mut changed_cells = self.changed_cells.take().unwrap();
        changed_cells.prepare(self, FramePacket {dab_batches:original_batches,..packet}, &batch_tiles, unchanged && !display_rebuilt, &mut encoder);
        if settled_commit.is_some() { changed_cells.force_all(); }
        if let Some(scene)=&mut self.scene {scene.retire_changed_cells(&changed_cells.take_retired());}
        self.changed_cells = Some(changed_cells);

        let old_preview_damage = self.preview_damage;
        let old_preview_contact_tiles = if packet.commit_rasters {self.preview_contact_tiles.take()} else {None};
        let mut new_preview_contact_tiles = Some(std::collections::BTreeSet::new());
        let old_preview_layer = self.preview_layer_id;
        let old_preview_level = self.preview_level;
        let old_preview_contribution = self.preview_contribution;
        let watercolor_style_dirty = self.update_watercolor_layer_styles(packet.dab_batches);
        let mut dirty = old_preview_damage.union(watercolor_style_dirty);
        if moving_images_changed { dirty = PixelRect::full(packet.document_extent); }
        let mut new_preview_damage = PixelRect::EMPTY;
        let mut new_preview_layer = None;
        let mut new_preview_requires_base = false;
        let mut preview_is_watercolor = false;

        // Ranges were validated during tile planning; accumulate metrics once.
        for (batch, tiles) in packet.dab_batches.iter().zip(&batch_tiles) {
            let start = batch.first_dab as usize;
            let dabs = &packet.dabs[start..start + batch.dab_count as usize];
            let batch_dirty = batch_pixel_rect(batch, self.target_extent(batch.target));
            let visual_dirty = if batch.style.execution == BrushExecution::Watercolor {
                batch_dirty.expand(
                    WatercolorLayerStyle::from_dab_style(&batch.style).radius(),
                    self.target_extent(batch.target),
                )
            } else if pointwise(&batch.style) && !batch.style.rendering.edge_after_stroke {
                // Prediction retirement must use the same bounded footprint as
                // painting, rather than reintroducing the generic brush halo.
                tiles.iter().fold(PixelRect::EMPTY, |bounds, tile| {
                    bounds.union(page_rect(tile.coordinate))
                }).intersect(batch_dirty)
            } else {
                batch_dirty
            };
            dirty = self.stroke_finish_pages(batch).fold(dirty, |damage, page| {
                damage.union(page_rect(page).intersect(PixelRect::full(packet.document_extent)))
            });
            if batch_dirty.is_empty() || dabs.is_empty() {
                continue;
            }
            if batch.kind == DabBatchKind::Preview {
                if pointwise(&batch.style)
                    && !batch.style.rendering.edge_after_stroke {
                    if let Some(sparse) = &mut new_preview_contact_tiles {
                        sparse.extend(tiles.iter().map(|tile| tile.coordinate));
                    }
                } else { new_preview_contact_tiles = None; }
                if new_preview_layer.is_some_and(|id| id != batch.target) {
                    return Err(GpuRasterError::MultiplePreviewLayers);
                }
                new_preview_layer = Some(batch.target);
                new_preview_damage = new_preview_damage.union(visual_dirty);
                let plan = BrushPassPlan::for_device(&batch.style, &self.device);
                new_preview_requires_base |=
                    plan.requires_destination() || batch.style.mode == DabMode::Erase
                    || packet.scene.color_mode(batch.target) != layer_core::color::LayerColorMode::FullColor;
                preview_is_watercolor |= plan.state.watercolor_wetness;
            }
            dirty = dirty.union(visual_dirty);
            self.metrics.dabs = self.metrics.dabs.saturating_add(dabs.len() as u64);
            for dab in dabs {
                self.metrics.raster_candidate_pixels = self
                    .metrics
                    .raster_candidate_pixels
                    .saturating_add(dab_candidate_pixels(*dab, self.target_extent(batch.target)));
            }
        }
        let destination_preview_batches = packet
            .dab_batches
            .iter()
            .filter(|batch| {
                batch.kind == DabBatchKind::Preview
                    && batch.dab_count != 0
                    && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
            })
            .count();
        let new_preview_from_persistent = destination_preview_batches == 1
            && !preview_is_watercolor
            && packet
                .dab_batches
                .iter()
                .filter(|batch| batch.kind == DabBatchKind::Preview && batch.dab_count != 0)
                .all(|batch| {
                    BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
                });
        let mut preview_contribution=new_preview_from_persistent && self.scale_display.as_ref().is_some_and(|cache|
            new_preview_layer.is_some_and(|id|cache.native_preview_input(packet,id)))
            && packet.dab_batches.iter().filter(|b|b.kind==DabBatchKind::Preview && b.dab_count!=0)
                .all(|b|dry_material::display_preview_eligible(&b.style) && b.style.mode==DabMode::Paint
                    && b.style.blend_space==packet.blend_space
                    && packet.scene.color_mode(b.target)==layer_core::color::LayerColorMode::FullColor);
        if preview_contribution {new_preview_requires_base=false;}
        let preview_level = self.scale_display.as_ref().filter(|_| new_preview_from_persistent
            && packet.dab_batches.iter().filter(|b| b.kind == DabBatchKind::Preview && b.dab_count != 0)
                .all(|b| dry_material::display_preview_eligible(&b.style)))
            .map_or(0, |cache| new_preview_layer.map_or(0, |id| cache.preview_level(self, packet, id, preview_contribution)));
        preview_contribution &= preview_level>0;
        if !preview_contribution && new_preview_from_persistent {new_preview_requires_base=true;}
        if packet.commit_rasters {
            self.preview_contribution=preview_contribution;
            let mut preview_cells=self.changed_cells.take().unwrap();
            preview_cells.prepare_preview(self,
                changed_cells::Preview {id:old_preview_layer,level:old_preview_level,contribution:old_preview_contribution,
                    damage:old_preview_damage,tiles:old_preview_contact_tiles.as_ref()},
                changed_cells::Preview {id:new_preview_layer,level:preview_level,contribution:preview_contribution,
                    damage:new_preview_damage,tiles:new_preview_contact_tiles.as_ref()});
            self.changed_cells=Some(preview_cells);
            if self.preview_level != preview_level {
                self.preview_pages.clear();
                self.preview_level = preview_level;
            }
            if new_preview_layer.is_none() {
                self.preview_pages.clear();
                self.preview_coverage_pages.clear();
                self.preview_watercolor_wetness_pages.clear();
            } else {
                self.ensure_preview_pages(new_preview_damage, new_preview_contact_tiles.as_ref());
                self.ensure_preview_watercolor_wetness_pages(new_preview_damage, preview_is_watercolor);
                if new_preview_from_persistent {
                    self.preview_coverage_pages.clear();
                } else {
                    self.ensure_preview_coverage_pages(packet.dab_batches);
                    self.ensure_preview_destination_companions(packet.dab_batches);
                }
            }

        }

        self.prepare_uploads(packet, &mut batch_tiles, &mut encoder)?;
        self.layer_masks.prepare(
            &self.device,
            &mut encoder,
            (packet.scene, original_batches),
            packet.document_extent,
            false,
            &mut self.selection_clip,
        )?;
        for target in source_access::placed_targets(packet.scene).filter(|t| matches!(t, SourceTarget::Paint(_))) {
            let Some(index) = self.paint_layers.iter().position(|l| l.id == target) else {
                continue;
            };
            for batch in original_batches.iter().filter(|b| b.target == target) {
                let DabBatchKind::RasterOperation(operation_index) = batch.kind else {
                    continue;
                };
                let op = &packet.scene.operations(target).ok_or(GpuRasterError::MissingPaintLayer(target))?[operation_index as usize];
                let masking = matches!(
                    op.kind,
                    layer_core::RasterOperationKind::ColorMode(_) | layer_core::RasterOperationKind::ApplyMask | layer_core::RasterOperationKind::Erase { .. }
                );
                if matches!(
                    op.kind,
                    layer_core::RasterOperationKind::Fill { .. }
                        | layer_core::RasterOperationKind::Gradient { .. }
                        | layer_core::RasterOperationKind::Figure(_)
                        | layer_core::RasterOperationKind::Bake { .. }
                        | layer_core::RasterOperationKind::FrequencyDetail { .. }
                ) || (masking && (packet.scene.paint_base(target).is_some() || self.native_backing(target).is_some()))
                {
                    // Coverage may be translated or inverted: its source mask
                    // pages are not necessarily the destination paint pages.
                    let bounds = batch_pixel_rect(batch, self.target_extent(target));
                    for c in page_coordinates(bounds) {
                        if masking
                            && !self.native_backing(target).is_some_and(|data| {
                                data.tiles.contains_key(&layer_core::raster::TileKey {
                                    plane: layer_core::raster::RasterPlane::Color,
                                    coordinate: c,
                                })
                            })
                            && !packet.scene.paint_base(target).is_some_and(|base| source_access::paint_base_contains(base, c))
                        {
                            continue;
                        }
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
        for batch in original_batches
            .iter()
            .filter(|b| layer_masks::MaskRenderer::is_mask(packet.scene, b.target))
        {
            dirty = dirty.union(batch_pixel_rect(batch, self.target_extent(batch.target)));
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
        if source_access::placed_targets(packet.scene).any(|t| packet.scene.paint_base(t).is_some() || self.native_backing(t).is_some()) {
            let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
            scene.initialize_source_paint(self, packet.scene, &mut encoder)?;
            self.scene = Some(scene);
        }
        self.keep_stroke_start_pages(packet.dab_batches, &batch_tiles, &mut encoder);
        for layer in &mut self.paint_layers {
            for page in &mut layer.pages {
                page.primary_needs_clear = false;
            }
            for page in &mut layer.watercolor_wetness_pages {
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
            }
        }

        self.encode_mask_dabs(
            &mut encoder,
            packet,
            original_batches,
            &committed_preview,
        )?;

        if let Some(started) = started { cpu_phases[0] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.paint");
        self.telemetry.phase_begin(0, &self.device, &self.queue, &mut encoder);
        // Persistent work is encoded before preview copies so prediction sees
        // this frame's committed ink.
        for (index, batch) in packet
            .dab_batches
            .iter()
            .enumerate()
            .filter(|(_, batch)| batch.kind != DabBatchKind::Preview)
        {
            if let DabBatchKind::RasterOperation(op) = batch.kind {
                if layer_masks::MaskRenderer::is_mask(packet.scene, batch.target) {
                    continue;
                }
                let operation = &packet.scene.operations(batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?[op as usize];
                if committed_preview.contains(&(batch.target, op)) {
                    // Already present in the layer pages: no recapture/resample.
                } else if matches!(operation.kind, layer_core::RasterOperationKind::Transform(_)) {
                    let mut transforms =
                        self.transforms.take().expect("retained transform renderer");
                    let result = transforms.apply(
                        self,
                        &mut encoder,
                        batch.target,
                        operation,
                    );
                    self.transforms = Some(transforms);
                    result?;
                } else {
                    let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
                    let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
                    scene.apply_operation(self, packet, batch.target, op as usize, damage, &mut encoder)?;
                    self.scene = Some(scene);
                }
                let bounds = operation
                    .bounds(self.target_extent(batch.target));
                let offset = layer_core::offsets::point(packet.scene.target_offset(batch.target));
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
                    .find(|l| l.id == batch.target)
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
                    tiles: &batch_tiles[index],
                    target_extent: self.target_extent(batch.target),
                    target: BrushEncodingTarget::Persistent,
                },
            )?;
            if batch.stroke_end && BrushPassPlan::for_device(&batch.style, &self.device).stroke_edge {
                self.encode_stroke_edge(&mut encoder, index, batch)?;
            }
            if batch.stroke_end && batch.style.execution.heals() {
                let cooperative = !packet.reset_layers && packet.dab_batches[index + 1..].iter().all(|b| b.kind == DabBatchKind::Preview);
                self.encode_heal(batch, &mut encoder, cooperative)?;
                if cooperative && self.retouch.as_ref().is_some_and(|s| s.heal_pending()) {
                    self.settling = Some(artwork::PendingFrame { frame: Arc::new(artwork::Frame::new(packet, context.clone())), view: requested_view, capture: None });
                }
            }
        }

        if self.settling.is_some() {
            self.clear_preview_stamps();
            self.uploads.finish(&encoder);
            self.telemetry.phase_end(0, &mut encoder);
            self.telemetry.end(&mut encoder);
            self.metrics.command_passes += encoder.pass_count();
            self.last_submission = Some(encoder.submit(&self.queue));
            self.telemetry.submitted(&self.queue);
            self.hold_background(false);
            self.metrics.submissions = self.metrics.submissions.saturating_add(1);
            self.submitted_context=Some(context);
            return Ok(());
        }

        if let Some(started) = started { cpu_phases[1] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.capture");
        self.telemetry.phase_end(0, &mut encoder);
        let native_commit = match settled_commit { Some(frame) => Some(frame), None => self.encode_native_rasters(packet.scene, &mut encoder)? };
        if let Some(started) = started { cpu_phases[2] = started.elapsed().as_secs_f64() * 1000.; }
        if !packet.commit_rasters {
            self.uploads.finish(&encoder);
            self.telemetry.end(&mut encoder);
            self.metrics.command_passes += encoder.pass_count();
            self.last_submission = Some(encoder.submit(&self.queue));
            self.telemetry.submitted(&self.queue);
            if let Some(commit) = native_commit {
                self.finish_native_rasters(commit, false)?;
            }
            self.hold_background(false);
            self.metrics.submissions = self.metrics.submissions.saturating_add(1);
            self.refresh_storage_metrics();
            if let Some(started) = started {
                cpu_phases[3] = cpu_phases[2];
                cpu_phases[4] = cpu_phases[2];
                cpu_phases[5] = started.elapsed().as_secs_f64() * 1000.;
                for i in (1..cpu_phases.len()).rev() { cpu_phases[i] -= cpu_phases[i - 1]; }
                self.metrics.frame_cpu_ms = cpu_phases;
            }
            self.submitted_context=Some(context);
            return Ok(());
        }
        trace_phase.next(c"capy.prediction");
        self.telemetry.phase_begin(1, &self.device, &self.queue, &mut encoder);

        self.preview_damage = new_preview_damage;
        self.preview_contact_tiles = new_preview_contact_tiles;
        if let Some(layer_id) = new_preview_layer {
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
                for coordinate in page_coordinates(copied).filter(|c| self.preview_contact_tiles.as_ref().is_none_or(|set| set.contains(c))) {
                    let preview = self
                        .preview_pages
                        .iter()
                        .find(|page| page.coordinate == coordinate)
                        .expect("preview pages are prepared before encoding");
                    let local = page_rect(coordinate).page_local(coordinate);
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
                                    x: local.min_x(),
                                    y: local.min_y(),
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &preview.primary.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x(),
                                    y: local.min_y(),
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
            if new_preview_requires_base && !new_preview_from_persistent
                && (packet.scene.paint_base(layer_id).is_some() || self.native_backing(layer_id).is_some()) {
                let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
                scene.initialize_source_preview(self, packet.scene, layer_id, copied, &mut encoder)?;
                self.scene = Some(scene);
            }

            let source = self.paint_layers.iter().find(|l| l.id == layer_id)
                .ok_or(GpuRasterError::MissingPaintLayer(layer_id))?;
            // Prediction is a disposable fork of committed stroke coverage.
            // Initialize only pages touched by this preview, then let its
            // microbatches ping-pong the private copy exactly like persistent
            // watercolor. This prevents a darker preview or a pen-up pop.
            for coverage in &self.preview_coverage_pages {
                let Some(batch) = packet.dab_batches.iter().find(|batch| {
                    batch.kind == DabBatchKind::Preview
                        && batch.target == layer_id
                        && BrushPassPlan::for_device(&batch.style, &self.device).state.coverage
                        && !batch_pixel_rect(batch, self.target_extent(batch.target))
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
                            && batch.target == layer_id
                            && BrushPassPlan::for_device(&batch.style, &self.device)
                                .state
                                .watercolor_wetness
                    })
                    .map(|batch| WatercolorLayerStyle::from_dab_style(&batch.style).radius())
                    .max()
                    .unwrap_or(0);
                let mask_source_damage =
                    new_preview_damage.expand(mask_halo, self.target_extent(new_preview_layer.unwrap()));
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
                                    x: local.min_x(),
                                    y: local.min_y(),
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: &preview_wetness.primary.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: local.min_x(),
                                    y: local.min_y(),
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
                        tiles: &batch_tiles[index],
                        target_extent: self.target_extent(batch.target),
                        target: BrushEncodingTarget::Preview {
                            from_persistent: new_preview_from_persistent,
                        },
                    },
                )?;
            }
        }
        if preview_is_watercolor { self.canonicalize_preview_watercolor(&mut encoder)?; }
        if let Some(started) = started { cpu_phases[3] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.composition");
        self.telemetry.phase_end(1, &mut encoder);
        self.telemetry.phase_begin(2, &self.device, &self.queue, &mut encoder);
        self.preview_layer_id = new_preview_layer;
        self.preview_requires_base = new_preview_requires_base;

        self.awaiting_meshes = self.transform_preview.as_ref()
            .is_some_and(|p| p.transform.placement.mesh.is_some())
            && !self.mesh_pipelines_ready();
        if let Some(preview) = self.transform_preview.clone().filter(|_| !self.awaiting_meshes) {
            let level = self.scale_display.as_ref().filter(|cache| cache.evaluation == scene::scale::Evaluation::Display).map(|cache| cache.plan.level).filter(|level| *level > 0);
            let mut transforms = self.transforms.take().expect("retained transform renderer");
            let result = transforms.update_preview(self, &mut encoder, &preview, packet.scene, level);
            self.transforms = Some(transforms);
            self.transform_damage.extend(result?);
        }
        let local_contacts = !original_batches.is_empty()
            && watercolor_style_dirty.is_empty()
            && original_batches.iter().all(|b| {
                matches!(b.kind, DabBatchKind::Persistent | DabBatchKind::Preview)
                    && pointwise(&b.style)
                    && !b.style.rendering.edge_after_stroke
            });
        let mut changed_cells = self.changed_cells.take().unwrap();
        for &(id, region) in &self.transform_damage { changed_cells.force(id, region); }
        self.changed_cells = Some(changed_cells);
        let canonical_pages = native_commit.as_ref().map_or(&[][..], |frame| &frame.canonical_pages[..]);
        if canonical_pages.iter().any(|(id,_)|!matches!(id,SourceTarget::Paint(_))) {self.changed_cells.as_mut().unwrap().force_all();}
        self.transform_damage.extend(canonical_pages.iter().map(|&(id,coordinate)|(id,page_rect(coordinate))));
        let mut composite_tiles = (!reset && !packet.composite_all
            && (local_contacts || (dirty.is_empty()
                && (!self.transform_damage.is_empty()
                    || !self.document_damage.is_empty()
                    || !canonical_pages.is_empty())
                && original_batches.is_empty() && packet.dabs.is_empty())))
        .then(std::collections::BTreeSet::new);
        if local_contacts && let Some(tiles) = &mut composite_tiles {
            dirty = PixelRect::EMPTY;
            let mut include = |id, local| {
                let bounds = brush_tiles::document_damage(packet.scene, id, local, packet.document_extent);
                dirty = dirty.union(bounds);
                tiles.extend(page_coordinates(bounds));
            };
            if let Some(id) = old_preview_layer {
                if let Some(sparse) = &old_preview_contact_tiles {
                    for &coordinate in sparse { include(id, page_rect(coordinate)); }
                } else { include(id, old_preview_damage); }
            }
            for (batch, planned) in original_batches.iter().zip(&batch_tiles) {
                for tile in planned { include(batch.target, scene::scale::Damage::tile_region(tile)); }
            }
        }
        for &(layer, bounds) in &self.transform_damage {
            let bounds = pixel_rect(
                bounds.to_rect().translated(layer_core::offsets::point(packet.scene.target_offset(layer))),
                packet.document_extent,
            );
            if let Some(tiles) = &mut composite_tiles {
                tiles.extend(page_coordinates(bounds));
            }
            dirty = dirty.union(bounds);
        }
        for &bounds in &self.document_damage {
            if let Some(tiles) = &mut composite_tiles {
                tiles.extend(page_coordinates(bounds));
            }
            dirty = dirty.union(bounds);
        }
        if self.transform_damage.iter().any(|(_, b)| !b.is_empty()) || !self.document_damage.is_empty() {
            self.filter_source_epoch = self.filter_source_epoch.wrapping_add(1);
        }
        if let Some(previews) = &mut self.filter_previews {
            previews.note_frame(FramePacket { view: requested_view, ..packet }, self.filter_source_epoch);
        }
        if packet.composite_all || reset {
            dirty = PixelRect::full(packet.document_extent);
        }

        let animated = packet.scene.order().iter().any(|&h| packet.scene.visible(h) && packet.scene.effect(h).is_some_and(|e| e.animated()));
        // Newly populated display pages change visible pixels too, even when
        // the document itself did not change (for example after navigation).
        let mut evaluated_objects = false;
        if !dirty.is_empty() || animated || display_rebuilt || !unchanged || self.object_deferred || self.scene.as_ref().is_some_and(|scene| scene.objects_pending()) {
            let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
            // A moved target's damage is stored in image coordinates. Round to
            // scene tiles after applying the target's document translation.
            for batch in original_batches {
                if packet.scene.source_owner(batch.target).is_some() {
                    let offset = layer_core::offsets::point(packet.scene.target_offset(batch.target));
                    let mut rect = batch.damage;
                    rect.min.x += offset.x;
                    rect.max.x += offset.x;
                    rect.min.y += offset.y;
                    rect.max.y += offset.y;
                    dirty = dirty.union(pixel_rect(rect, packet.document_extent));
                }
            }
            let result = scene.compose(self, packet, &batch_tiles, dirty, &mut encoder, composite_tiles.as_ref());
            self.scene = Some(scene);
            let published=match result {
                Err(GpuRasterError::DeferredObjectWork)=> {
                    self.object_deferred=true;
                    self.document_damage.push(PixelRect::full(packet.document_extent));
                    self.uploads.finish(&encoder);
                    self.telemetry.phase_end(2,&mut encoder);self.telemetry.end(&mut encoder);
                    self.metrics.command_passes+=encoder.pass_count();
                    self.last_submission=Some(encoder.submit(&self.queue));
                    self.telemetry.submitted(&self.queue);
                    if let Some(commit)=native_commit {self.finish_native_rasters(commit,true)?;}
                    self.artwork_frame=Some(Arc::new(artwork::Frame::new(packet,context.clone())));
                    self.submitted_context=Some(context);
                    self.metrics.submissions=self.metrics.submissions.saturating_add(1);
                    self.refresh_storage_metrics();
                    return Ok(());
                },
                result=>result?,
            };
            self.composite_revision=self.composite_revision.wrapping_add(1);
            self.object_deferred=false;
            self.composite_damage = published;
            evaluated_objects = packet.scene.order().iter().any(|&owner| packet.scene.object_layer(owner).is_some());
        }
        let mut navigator = std::mem::take(&mut self.navigator);
        let navigator_revision = navigator.revision;
        let result = navigator.refresh(self, packet, !dirty.is_empty() || animated || !unchanged,
            reset || blending_changed, &mut encoder);
        self.navigator = navigator;
        result?;
        let mut refined = false;
        if navigator_revision == self.navigator.revision && dirty.is_empty() && !animated && original_batches.is_empty() && packet.dabs.is_empty()
            && self.background_ready.load(std::sync::atomic::Ordering::Acquire)
            && self.scale_display.as_ref().is_some_and(|cache| cache.has_pending_work(self))
            && self.artwork_frame.as_ref().is_some_and(|old|
                old.view.document_to_surface == packet.view.document_to_surface
                    && old.same_artwork(packet))
        {
            let mut scene = self.scene.take().unwrap_or_else(|| scene::Scene::new(self));
            let result = scene.refine_display(self, packet, &mut encoder);
            self.scene = Some(scene);
            let changed = result?;
            if !changed.is_empty() {
                refined = true;
                self.composite_damage = self.composite_damage.union(changed);
                self.composite_revision = self.composite_revision.wrapping_add(1);
            }
        }
        if self.transform_preview.is_none() && !reset && packet.dabs.is_empty()
            && packet.dab_batches.is_empty() && packet.restore_rasters.is_empty()
            && let Some((layer, selection)) = self.moving_pixels.clone()
            && let Some(level) = self.scale_display.as_ref().filter(|cache| cache.evaluation == scene::scale::Evaluation::Display).map(|cache| cache.plan.level).filter(|level| *level > 0)
        {
            let mut transforms = self.transforms.take().unwrap();
            let result = transforms.prepare_standby(self, &mut encoder, packet.scene, layer, &selection, level);
            self.transforms = Some(transforms);
            result?;
        }
        let frame = Arc::new(artwork::Frame::new(packet, context.clone()));
        let moving = !original_batches.is_empty()
            || animated
            || self.transform_preview.is_some()
            || self.artwork_frame.as_ref().is_none_or(|old| !old.same_artwork(packet));
        self.prefetch_retouch(&frame, moving, &mut encoder)?;
        self.uploads.finish(&encoder);
        if let Some(started) = started { cpu_phases[4] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.publication");
        self.telemetry.phase_end(2, &mut encoder);
        self.telemetry.end(&mut encoder);
        self.metrics.command_passes += encoder.pass_count();
        if let Some(backup)=self.display_backup.take() {
            let bytes=backup.bytes;
            self.retired_display_bytes.fetch_add(bytes,std::sync::atomic::Ordering::AcqRel);
            let charge=DisplayRetirement {bytes,charge:self.retired_display_bytes.clone(),
                #[cfg(not(target_arch="wasm32"))] _backup:backup};
            #[cfg(target_arch="wasm32")] drop(backup);
            encoder.on_submitted_work_done(move||drop(charge));
        }
        let submission = encoder.submit(&self.queue);
        self.telemetry.submitted(&self.queue);
        self.last_submission = Some(submission.clone());
        if evaluated_objects { self.evaluated_object_revision = Some(packet.scene.revision()); }
        else if !packet.scene.order().iter().any(|&owner| packet.scene.object_layer(owner).is_some()) { self.evaluated_object_revision = None; }
        if refined {
            self.hold_background(true);
        }
        if let Some(commit) = native_commit {
            self.finish_native_rasters(commit, true)?;
        }
        if animated || reset || !packet.dabs.is_empty() || !packet.restore_rasters.is_empty()
            || self.transform_damage.iter().any(|(_, bounds)| !bounds.is_empty()) || !self.document_damage.is_empty()
            || self.artwork_frame.as_ref().is_none_or(|old| !old.same_artwork(packet)) {
            self.artwork_revision = self.artwork_revision.wrapping_add(1);
            if let Some(regions) = &mut self.regions { regions.raw.invalidate_tonal(); }
        }
        self.submitted_context = Some(context);
        self.artwork_frame = Some(frame);
        self.metrics.submissions = self.metrics.submissions.saturating_add(1);
        self.refresh_storage_metrics();
        performance_trace::counter(c"Capy renderer frames", self.metrics.submissions);
        performance_trace::counter(c"Capy command passes", self.metrics.command_passes);
        performance_trace::counter(c"Capy dabs", self.metrics.dabs);
        performance_trace::counter(c"Capy composited pixels", self.metrics.composited_pixels);
        performance_trace::counter(c"Capy display batches", self.metrics.display_composition_submissions);
        performance_trace::counter(c"Capy upload drains", self.metrics.source_upload_submissions);
        performance_trace::counter(c"Capy source upload peak bytes", self.metrics.source_upload_peak_bytes);
        performance_trace::counter(c"Capy restore batches", self.metrics.native_restore_submissions);
        performance_trace::counter(c"Capy paint pages", self.metrics.paint_pages);
        performance_trace::counter(c"Capy preview pages", self.metrics.preview_pages);
        {
            let [hits, misses] = self.source_cache_work();
            let [resident, uploads] = self.source_tiles.borrow().admitted_bytes();
            performance_trace::counter(c"Capy source resident limit bytes", resident);
            performance_trace::counter(c"Capy source upload limit bytes", uploads);
            performance_trace::counter(c"Capy source hits", hits);
            performance_trace::counter(c"Capy source misses", misses);
        }
        if let Some(started) = started {
            cpu_phases[5] = started.elapsed().as_secs_f64() * 1000.;
            for i in (1..cpu_phases.len()).rev() { cpu_phases[i] -= cpu_phases[i - 1]; }
            self.metrics.frame_cpu_ms = cpu_phases;
            self.telemetry
                .cpu
                .push(started.elapsed().as_secs_f32() * 1000.);
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DabGpu {
    dab: Dab,
    // Rows of the midpoint nib metric. These depend on the contact, not pixels.
    metric: [f32; 4],
    // World travel, firm-nib feed, incoming film ceiling, maximum pressure.
    invariants: [f32; 4],
}

impl From<Dab> for DabGpu {
    fn from(dab: Dab) -> Self {
        let axes = [(dab.previous[0] + dab.radii[0]) * 0.5,
            (dab.previous[1] + dab.radii[1]) * 0.5].map(|v| v.max(0.005));
        let angle = [dab.previous[2] + dab.rotation[0], dab.previous[3] + dab.rotation[1]];
        let length2 = angle[0] * angle[0] + angle[1] * angle[1];
        let rotation = if length2 > 0.00001 { angle.map(|v| v / length2.sqrt().max(0.00001)) }
            else { dab.rotation };
        let travel = dab.motion[0].hypot(dab.motion[1]);
        Self { dab, metric: [rotation[0] / axes[0], rotation[1] / axes[0],
            -rotation[1] / axes[1], rotation[0] / axes[1]],
            invariants: [travel, dab.hardness.powi(12), 1., 0.] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StyleGpu {
    color: [f32; 4],
    canvas_opacity: [f32; 4],
    grain: [f32; 4],
    flags: [f32; 4],
    edges: [f32; 4],
    material_a: [f32; 4],
    material_b: [f32; 4],
    operation: [u32; 4],
    deformation: [f32; 4],
    render_mode: [f32; 4],
    transport_a: [f32; 4],
    transport_b: [f32; 4],
    contact_a: [f32; 4],
    contact_b: [f32; 4],
    contact_c: [f32; 4],
    bristles: [f32; 4],
    bristle_streak: [f32; 4],
    color_mode: [f32; 4],
}

fn layer_color_parameters(mode: layer_core::color::LayerColorMode, color: layer_core::color::DocumentColor) -> [f32; 4] {
    use layer_core::color::LayerColorMode::*;
    [match mode { FullColor => 0., Grayscale => 1., TwoTone => 2. },
        if color.depth.is_float() { 0.5 } else { color.space.decode(0.5) as f32 }, 0., 0.]
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
            flags: [0.0; 4],
            edges: [0.0; 4],
            material_a: [0.0; 4],
            material_b: [0.0; 4],
            operation: [0; 4],
            deformation: [0.0; 4],
            render_mode: [0.0; 4],
            transport_a: [0.0; 4],
            transport_b: [0.0; 4],
            contact_a: [0.0; 4],
            contact_b: [0.0; 4],
            contact_c: [0.0; 4],
            bristles: [0.0; 4],
            bristle_streak: [0.0; 4],
            color_mode: [0.0; 4],
        }
    }

    fn layer(extent: [u32; 2], watercolor: Option<WatercolorLayerStyle>) -> Self {
        let mut result = Self::plain(extent, [0.0; 4], 1.0);
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

    fn brush(extent: [u32; 2], batch: &DabBatch, block: u32, device: &PipelineDevice) -> Self {
        let mut style = Self::for_brush(extent, &batch.style, batch.first_dab, batch.dab_count, device);
        style.canvas_opacity[3] = f32::from(batch.stroke_start);
        style.operation[2] = block;
        style
    }

    fn for_brush(extent: [u32; 2], style: &layer_render::DabStyle, first: u32, count: u32, device: &PipelineDevice) -> Self {
        let grain = style.grain.as_ref();
        let (grain_cos, grain_sin) = grain
            .map(|grain| (grain.rotation_radians.cos(), grain.rotation_radians.sin()))
            .unwrap_or((1.0, 0.0));
        let mut result = Self::plain(extent, [0.0; 4], 1.0);
        result.grain = [
            grain.map_or(1.0, |grain| grain.scale),
            grain.map_or(0.0, |grain| grain.depth),
            grain_cos,
            grain_sin,
        ];
        result.flags = [
            f32::from(matches!(style.tip, BrushTip::AnalyticEllipse)),
            f32::from(grain.is_some()),
            f32::from(grain.is_some_and(|grain| grain.behavior == BrushGrainBehavior::Canvas)),
            grain.map_or(0.0, |grain| grain.offset_jitter),
        ];
        result.edges = [
            style.rendering.wet_edge,
            style.rendering.burnt_edge,
            style.rendering.edge_width,
            style.rendering.alpha_threshold,
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
        result.operation = [first, count, 1, u32::from(style.mode == DabMode::Erase)];
        result.deformation = [
            liquify_mode_code(style.deform.mode),
            style.deform.strength,
            style.deform.pressure,
            style.deform.momentum,
        ];
        let perceptual = style.blend_space == layer_core::BlendSpace::Perceptual;
        result.render_mode = [
            (blend_code(style.rendering.blend_mode.into(), device, style.blend_space) | u32::from(perceptual) << 8) as f32,
            f32::from(style.rendering.accumulation == BrushAccumulation::Uniform),
            f32::from(style.wet_mix.mix_space as u8),
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
        if let Some(contact) = style.contact {
            if let Some(bristles) = contact.bristles {
                result.bristles = [1., bristles.texture_scale, bristles.load, bristles.splay];
                result.bristle_streak = bristles.streak_rgba_linear;
            }
            let paper = contact.paper * grain.map_or(1., |grain| grain.depth);
            result.contact_a = [1.0, paper, contact.tip_bias, contact.edge_roughness];
            result.contact_b = [
                contact.edge_scale,
                contact.fibers,
                contact.fiber_strength,
                contact.pooling,
            ];
            result.contact_c = [
                contact.pressure_gain,
                contact.depletion,
                contact.tilt_shading,
                f32::from(contact.linear_edge),
            ];
        }
        // This lane is unused by dry and composite shaders and avoids growing
        // every style upload solely for one material-stage lifecycle bit.
        result.color[3] = f32::from(style.alpha_locked);
        result
    }
}

/// A blend as `blend_modes.wgsl` reads it: the `LayerBlend` code in bits 0-7,
/// Perceptual documents in bit 8 and float documents in bit 9.
/// Normal needs no flags, so its code is always 0; a clipped Pass Through
/// group composites isolated, as Normal.
pub(crate) fn blend_code(blend: layer_core::LayerBlend, device: &PipelineDevice, space: layer_core::BlendSpace) -> u32 {
    if matches!(blend, layer_core::LayerBlend::Normal | layer_core::LayerBlend::PassThrough) {
        return 0;
    }
    blend.code() | u32::from(space == layer_core::BlendSpace::Perceptual) << 8 | u32::from(device.hdr()) << 9
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
    batches[..index]
        .iter()
        .rev()
        .take_while(|b| same_material_update(b, current))
        .all(|b| b.dab_count == 0)
}

fn is_last_watercolor_update_batch(batches: &[DabBatch], index: usize) -> bool {
    let current = &batches[index];
    batches[index + 1..]
        .iter()
        .take_while(|b| same_material_update(b, current))
        .all(|b| b.dab_count == 0)
}

fn same_material_update(a: &DabBatch, b: &DabBatch) -> bool {
    a.kind == b.kind
        && a.stroke_id == b.stroke_id
        && a.target == b.target
        && a.material_update == b.material_update
}

fn watercolor_update_damages(
    batches: &[DabBatch],
    index: usize,
    extent: [u32; 2],
) -> Vec<PixelRect> {
    let current = &batches[index];
    let start = index
        - batches[..index]
            .iter()
            .rev()
            .take_while(|b| same_material_update(b, current))
            .count();
    batches[start..]
        .iter()
        .take_while(|b| same_material_update(b, current))
        .filter(|batch| batch.dab_count != 0)
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
    let rect = PixelRect::new(
        (dab.center.x - extent_x)
            .floor()
            .max(0.0)
            .min(extent[0] as f32) as u32,
        (dab.center.y - extent_y)
            .floor()
            .max(0.0)
            .min(extent[1] as f32) as u32,
        (dab.center.x + extent_x)
            .ceil()
            .max(0.0)
            .min(extent[0] as f32) as u32,
        (dab.center.y + extent_y)
            .ceil()
            .max(0.0)
            .min(extent[1] as f32) as u32,
    );
    if rect.is_empty() { 0 } else { rect.area() }
}

fn create_style_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    bindings::layout(device, "layer style layout", &[bindings::buffer(
        0,
        wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
        wgpu::BufferBindingType::Uniform,
        true,
        NonZeroU64::new(mem::size_of::<StyleGpu>() as u64),
    )])
}

fn create_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    bindings::layout(device, "layer sampled texture layout", &[
        bindings::texture(0, wgpu::ShaderStages::FRAGMENT, true),
        bindings::sampler(1, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
    ])
}

fn create_advanced_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let stages = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
    let entries = (0..3).map(|binding| bindings::texture(binding, stages, true))
        .chain([bindings::sampler(3, stages, wgpu::SamplerBindingType::Filtering)])
        .collect::<Vec<_>>();
    bindings::layout(device, "layer advanced brush texture layout", &entries)
}

fn create_target_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    bindings::layout(device, "layer render target layout", &[
        bindings::buffer(
            0,
            wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Uniform,
            true,
            NonZeroU64::new(mem::size_of::<TargetGpu>() as u64),
        ),
        bindings::buffer(
            1,
            wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Storage { read_only: true },
            false,
            NonZeroU64::new(48),
        ),
    ])
}

fn create_material_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let stages = wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
    let storage = wgpu::BufferBindingType::Storage { read_only: true };
    let entries = (0..12).map(|binding| match binding {
        9 => bindings::buffer(9, stages, storage, false, None),
        _ => bindings::texture(binding, stages, binding == 4 || binding == 10),
    })
    .chain([bindings::buffer(12, stages, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(160))])
    .collect::<Vec<_>>();
    bindings::layout(device, "layer material source neighborhood layout", &entries)
}

fn fragment_textures_layout(device: &wgpu::Device, count: u32, label: &str) -> wgpu::BindGroupLayout {
    let entries = (0..count)
        .map(|binding| bindings::texture(binding, wgpu::ShaderStages::FRAGMENT, false))
        .collect::<Vec<_>>();
    bindings::layout(device, label, &entries)
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
    bindings::group(device, "layer dynamic style binding", layout, [
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64), }),
    ])
}

fn create_target_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
    selection: &wgpu::Buffer,
) -> wgpu::BindGroup {
    bindings::group(device, "layer dynamic render target binding", layout, [
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<TargetGpu>() as u64), }),
        selection.as_entire_binding(),
    ])
}

fn create_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    label: &'static str,
) -> wgpu::BindGroup {
    bindings::group(device, label, layout, [
        wgpu::BindingResource::TextureView(view),
        wgpu::BindingResource::Sampler(sampler),
    ])
}

fn create_advanced_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: [&wgpu::TextureView; 3],
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let resources = views.map(wgpu::BindingResource::TextureView).into_iter();
    bindings::group(device, "layer advanced brush textures", layout, resources.chain([wgpu::BindingResource::Sampler(sampler)]))
}

fn create_material_bind_group(
    device: &PipelineDevice,
    layout: &wgpu::BindGroupLayout,
    views: &[&wgpu::TextureView],
    dabs: &wgpu::Buffer,
    coverage: &wgpu::TextureView,
    reservoir: &wgpu::TextureView,
    sources: wgpu::BindingResource<'_>,
) -> wgpu::BindGroup {
    debug_assert_eq!(views.len(), 9);
    let pages = views.iter().copied().map(wgpu::BindingResource::TextureView);
    bindings::group(device, "layer material source neighborhood", layout, pages.chain([
        dabs.as_entire_binding(),
        wgpu::BindingResource::TextureView(coverage),
        wgpu::BindingResource::TextureView(reservoir),
        sources,
    ]))
}

fn views_group<'a>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    views: impl IntoIterator<Item = &'a wgpu::TextureView>,
) -> wgpu::BindGroup {
    bindings::group(device, label, layout, views.into_iter().map(wgpu::BindingResource::TextureView))
}

fn create_color_target(
    device: &PipelineDevice,
    extent: [u32; 2],
    label: &'static str,
) -> (wgpu::Texture, wgpu::TextureView) {
    create_target(device, extent, device.working_format(), label)
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
            | wgpu::TextureUsages::COPY_DST
            | if matches!(format, wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float) {
                wgpu::TextureUsages::STORAGE_BINDING
            } else { wgpu::TextureUsages::empty() },
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
        preview: Default::default(),
        texture,
        view,
        texture_bind_group,
        material_input: Default::default(),
        material_in_place_input: Default::default(),
        material_output: Default::default(),
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

fn texture_switch(group: u32, count: usize, function: &str) -> String {
    let mut shader: String =
        (0..count).map(|i| format!("@group({group}) @binding({i}) var source{i}:texture_2d<f32>;\n")).collect();
    shader += &format!("fn {function}(i:u32,p:vec2<i32>)->vec4<f32>{{switch i {{\n");
    shader.extend((0..count).map(|i| format!("case {i}u:{{return textureLoad(source{i},p,0);}}\n")));
    shader + "default:{return vec4(0.);}}}\n"
}

fn create_pipelines(device: &PipelineDevice, layouts: PipelineLayouts<'_>) -> Pipelines {
    let advanced_brush = {
        let device = device.clone();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer textured dry brush shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                    &working_color::shader(&device),
                    include_str!("advanced_brush.wgsl"),
                    include_str!("analytic_coverage.wgsl"), include_str!("brush_coverage.wgsl"),
                    include_str!("contact.wgsl"),
                    include_str!("selection_clip.wgsl"),
                ])),
            })
        })
    };
    let material_shader = dry_material::shader(device, dry_material::Target::Exact);
    let dry_material = dry_material::Pipelines::new(device, &layouts, &material_shader, dry_material::Target::Exact);
    let dry_display = dry_material::Pipelines::new(device, &layouts,
        &dry_material::shader(device, dry_material::Target::Display), dry_material::Target::Display);
    // Native hosts request this feature only after checking Float32 read/write
    // storage support. Each dry invocation owns exactly one destination texel.
    let dry_in_place = device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
        .then(|| dry_material::Pipelines::new(device, &layouts, &dry_material::shader(device, dry_material::Target::InPlace), dry_material::Target::InPlace));
    let dry_display_tracked = dry_in_place.as_ref().map(|_| dry_material::Pipelines::new(device, &layouts,
        &dry_material::shader(device, dry_material::Target::DisplayTracked), dry_material::Target::DisplayTracked));
    let dry_tracked = dry_in_place.as_ref().map(|_| dry_material::Pipelines::new(device, &layouts,
        &dry_material::shader(device, dry_material::Target::Tracked), dry_material::Target::Tracked));
    let stroke_edge_shader = {
        let device = device.clone();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer post-stroke edge shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                    &working_color::shader(&device),
                    include_str!("stroke_edge.wgsl"),
                    include_str!("selection_clip.wgsl"),
                ])),
            })
        })
    };
    let watercolor_shader = {
        let device = device.clone();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer live watercolor composite shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&working_color::shader(&device), include_str!("watercolor_floor.wgsl"), include_str!("watercolor_composite.wgsl")])),
            })
        })
    };
    let watercolor_transport_shader = {
        let device = device.clone();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer watercolor capillary transport shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                    &working_color::shader(&device),
                    include_str!("watercolor_floor.wgsl"),
                    include_str!("watercolor_transport.wgsl"),
                    include_str!("selection_clip.wgsl"),
                ])),
            })
        })
    };
    let export_shader = {
        let device = device.clone();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer export shader"),
                source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", view_color::shader(device.working_space(), layer_core::color::RgbSpace::Srgb), include_str!("export.wgsl")).into()),
            })
        })
    };
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
        (paint_blend, "layer textured paint"),
        (erase_blend, "layer textured erase"),
    ]
    .map(|(blend, label)| {
        let (device, layout, shader) = (device.clone(), advanced_layout.clone(), advanced_brush.clone());
        Deferred::pipeline(move |mode| {
            brush_pipeline_format_recipe(mode, &device, &layout, &shader, "fragment_main", blend, device.working_format(), label)
        })
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
        format: device.working_format(),
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    });
    let coverage_target = Some(wgpu::ColorTargetState {
        format: device.scalar_format(),
        blend: None,
        write_mask: wgpu::ColorWrites::RED,
    });
    let watercolor_wetness_target = Some(wgpu::ColorTargetState {
        format: device.scalar_format(),
        // All microbatches in one submitted update write the same destination
        // surface while the source remains the immutable pre-update snapshot.
        blend: Some(max_blend),
        write_mask: wgpu::ColorWrites::RED,
    });
    let material_targets = [
        (
            [color_target.clone(), None, None],
            "layer destination brush color",
        ),
        (
            [color_target.clone(), coverage_target.clone(), None],
            "layer destination brush with stroke coverage",
        ),
        (
            [
                color_target,
                coverage_target.clone(),
                watercolor_wetness_target,
            ],
            "layer watercolor brush with wetness state",
        ),
    ];
    // Keep each destination operation in a separate optimized shader. Compiling
    // their combined control flow can hold a native driver call for seconds,
    // delaying newly selected tools and final compiler shutdown. Attachment
    // variants still share one shader module and the same brush calculations.
    let material = std::array::from_fn(|index| {
        let operation = MaterialOperation::ALL[index / MaterialPipelineKind::COUNT];
        let (targets, label) = material_targets[index % MaterialPipelineKind::COUNT].clone();
        let (device, layout, shader) = (
            device.clone(),
            material_pipeline_layout.clone(),
            material_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_targets_with_constants_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "fragment_main",
                &targets,
                &[("MATERIAL_OPERATION", operation as u32 as f64)],
                label,
            )
        })
    });
    let material_gather = std::array::from_fn(|index| {
        let operation = [MaterialOperation::Liquify, MaterialOperation::Smudge][index];
        let (device, layout, shader) = (
            device.clone(),
            material_pipeline_layout.clone(),
            material_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_targets_with_constants_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "gather_fragment",
                &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba32Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                &[("MATERIAL_OPERATION", operation as u32 as f64)],
                "gather distant material samples",
            )
        })
    });
    let watercolor_transport = std::array::from_fn(|step| {
        let (device, layout, shader) = (
            device.clone(),
            watercolor_transport_layout.clone(),
            watercolor_transport_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_targets_with_constants_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "fragment_main",
                &[
                    Some(wgpu::ColorTargetState {
                        format: device.working_format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: device.scalar_format(),
                        blend: None,
                        write_mask: wgpu::ColorWrites::RED,
                    }),
                ],
                &[("TRANSPORT_PASS", step as f64)],
                "layer nonlinear watercolor capillary relaxation",
            )
        })
    });
    let reservoir = {
        let (device, layout, shader) = (
            device.clone(),
            material_pipeline_layout.clone(),
            material_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "reservoir_fragment",
                None,
                device.working_format(),
                "layer brush reservoir exchange",
            )
        })
    };
    let stroke_edge = {
        let (device, layout, shader) = (
            device.clone(),
            edge_pipeline_layout.clone(),
            stroke_edge_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "fragment_main",
                None,
                device.working_format(),
                "layer post-stroke edge",
            )
        })
    };
    let watercolor_composite = {
        let (device, layout, shader) = (
            device.clone(),
            watercolor_pipeline_layout.clone(),
            watercolor_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "fragment_main",
                Some(paint_blend),
                device.working_format(),
                "layer live watercolor composition",
            )
        })
    };
    let watercolor_compute = {
        let output = bindings::layout(device, "watercolor output", &[bindings::storage_texture(
            0, wgpu::ShaderStages::COMPUTE, wgpu::TextureFormat::Rgba32Float, wgpu::StorageTextureAccess::WriteOnly,
        )]);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("watercolor composition"),
            bind_group_layouts: &[Some(layouts.style), Some(layouts.target), Some(layouts.watercolor), Some(&output)],
            immediate_size: 0,
        });
        (output, Deferred::compute(device, "watercolor composition", &layout, &watercolor_shader, "composite"))
    };
    let export = {
        let (device, layout, shader) =
            (device.clone(), export_layout.clone(), export_shader.clone());
        Deferred::pipeline(move |mode| {
            fullscreen_pipeline_recipe(
                mode,
                &device,
                &layout,
                &shader,
                "fragment_main",
                None,
                EXPORT_FORMAT,
                "layer sRGB export",
            )
        })
    };
    Pipelines {
        dry_material,
        dry_display,
        dry_display_tracked,
        dry_in_place,
        dry_tracked,
        direct,
        material,
        material_gather,
        watercolor_transport,
        reservoir,
        stroke_edge,
        watercolor_composite,
        watercolor_compute,
        export,
    }
}

#[expect(clippy::too_many_arguments, reason = "Brush pipeline recipes retain explicit shader entry, blend, format, and compilation mode")]
fn brush_pipeline_format_recipe(
    mode: CompileMode,
    device: &PipelineDevice,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: wgpu::BlendState,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> Compilation<wgpu::RenderPipeline> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 13] = wgpu::vertex_attr_array![
        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x2,
        4 => Float32x4, 5 => Float32x2, 6 => Float32x2, 7 => Float32x4,
        8 => Float32x4, 9 => Float32x4, 10 => Float32x4,
        11 => Float32x4, 12 => Float32x4
    ];
    mode.render(
        device,
        &wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vertex_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<DabGpu>() as u64,
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
                    blend: device.attachment_blend(format, Some(blend)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        },
    )
}

#[expect(clippy::too_many_arguments, reason = "Fullscreen pipeline recipes retain explicit shader entry, blend, format, and compilation mode")]
fn fullscreen_pipeline_recipe(
    mode: CompileMode,
    device: &PipelineDevice,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: Option<wgpu::BlendState>,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> Compilation<wgpu::RenderPipeline> {
    let target = wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL };
    fullscreen_pipeline_targets_with_constants_recipe(mode, device, layout, shader, fragment_entry, &[Some(target)], &[], label)
}

#[expect(clippy::too_many_arguments, reason = "Fullscreen pipeline recipes retain explicit target states and specialization constants")]
fn fullscreen_pipeline_targets_with_constants_recipe(
    mode: CompileMode,
    device: &PipelineDevice,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    targets: &[Option<wgpu::ColorTargetState>],
    constants: &[(&str, f64)],
    label: &'static str,
) -> Compilation<wgpu::RenderPipeline> {
    let targets: Vec<_> = targets.iter().map(|target| target.clone().map(|mut target| {
        target.blend = device.attachment_blend(target.format, target.blend); target
    })).collect();
    mode.render(
        device,
        &wgpu::RenderPipelineDescriptor {
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
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        },
    )
}

fn fullscreen_pipeline(
    device: &PipelineDevice,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &'static str,
    blend: Option<wgpu::BlendState>,
    format: wgpu::TextureFormat,
    label: &'static str,
) -> wgpu::RenderPipeline {
    fullscreen_pipeline_recipe(
        CompileMode::Immediate,
        device,
        layout,
        shader,
        fragment_entry,
        blend,
        format,
        label,
    )
    .immediate()
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

fn preview_block(document_to_surface: [f32; 6]) -> u32 {
    1 << display_mips::view_level(document_to_surface, 3).unwrap_or(0).saturating_sub(1).min(2)
}

fn dab_bytes(dabs: &[DabGpu]) -> &[u8] {
    // SAFETY: DabGpu is repr(C), has no padding, and contains only f32 fields.
    unsafe { std::slice::from_raw_parts(dabs.as_ptr().cast::<u8>(), mem::size_of_val(dabs)) }
}

/// Original paper-height recipe, generated once and cached/uploaded as R8.
/// Fine tooth survives light pressure; broader fibers avoid white-noise grain.
fn procedural_contact_paper() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(1024 * 1024);
    for y in 0..1024 {
        for x in 0..1024 {
            let u = (x as f32 + 0.5) / 1024.;
            let v = (y as f32 + 0.5) / 1024.;
            let tooth = periodic_value_noise(u, v, 640, 640, 0x124f_4139);
            let fibers = periodic_value_noise(u, v, 360, 180, 0x823a_5421);
            let structure = periodic_value_noise(u, v, 72, 72, 0x3ae2_9141);
            let height = 0.12 + 0.76 * (tooth * 0.64 + fibers * 0.26 + structure * 0.1);
            pixels.push((height.clamp(0., 1.) * 255.).round() as u8);
        }
    }
    pixels
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
    pub(super) fn with_time_controls(
        mut program: layer_core::EffectProgram,
    ) -> layer_core::EffectProgram {
        program.time = true;
        let mut parameters = program.parameters.to_vec();
        parameters.extend([
            layer_core::EffectParameter {
                opaque: false,
                dimension: Default::default(),
                page: None, visible_when: None, soft_bounds: None, mapping: Default::default(),
                key: "animate".into(),
                label: "Animate".into(),
                section: Some("Animation".into()),
                kind: layer_core::EffectParameterKind::Toggle,
                default: layer_core::EffectValue::Toggle(true),
            },
            layer_core::EffectParameter {
                opaque: false,
                dimension: Default::default(),
                page: None, visible_when: None, soft_bounds: None, mapping: Default::default(),
                key: "time".into(),
                label: "Frozen time".into(),
                section: Some("Animation".into()),
                kind: layer_core::EffectParameterKind::Number {
                    min: 0.,
                    max: 3600.,
                    step: 0.1,
                    decimals: 2,
                    unit: "s".into(),
                },
                default: layer_core::EffectValue::Number(0.),
            },
        ]);
        program.parameters = parameters.into();
        program
    }
    mod adjustments;
    mod blend_modes;
    mod blend_space;
    mod brush_blending;
    pub(crate) mod image_windows;
    mod pass_through;
    mod live_windows;
    mod cold_paint;
    mod color_picker;
    mod curve_reference;
    mod filter_library;
    mod filter_spaces;
    pub(crate) mod native_effects;
    mod view_color;
    use crate::test_support::packet;
    use layer_core::{BrushDeform, BrushRendering, BrushWetMix, Point, Rect};
    use layer_core::authored::OccurrenceHandle;
    use layer_core::SceneView;
use layer_render::{DabBatchKind, DabStyle, FramePacket, ViewState};
    use std::sync::Arc;

    fn test_view() -> ViewState {
        ViewState {
            width_px: 128,
            height_px: 128,
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub(crate) fn test_dab(center: [f32; 2], color: [f32; 4], flow: f32) -> Dab {
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
            previous: [0.0; 4],
            contact: [0.0; 4],
            previous_contact: [0.0; 4],
        }
    }

    pub(crate) fn test_style(execution: BrushExecution) -> DabStyle {
        DabStyle {
            alpha_locked: false,
            selection: None,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution,
            grain: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
            contact: None,
            retouch: None,
            blend_space: layer_core::BlendSpace::Linear,
        }
    }

    #[test]
    fn thumbnails_frame_nontransparent_pixels_and_show_paper_and_checkerboard() {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
        use layer_core::authored::*;
        let mut document = layer_core::Document::new(PortableId::random(), 128, 128,
            layer_core::DocumentNames { paint: "Small mark".into(), paper: "Paper".into() });
        let paper = document.scene().order()[1];
        let paint = document.scene().source_target(document.scene().order()[0]).unwrap();
        let mut dab = test_dab([40., 100.], [1., 0., 0., 1.], 1.);
        dab.radii = [3., 6.];
        let mut batch = crate::test_support::dab_batch(
            paint,
            test_style(BrushExecution::Dry),
            Rect { min: Point { x: 32., y: 92. }, max: Point { x: 48., y: 108. } },
        );
        let frame =
            |r: &mut WgpuRasterizer, document: &layer_core::Document, dabs: &[Dab], batches: &[DabBatch]| {
                r.submit(FramePacket {
                    view: test_view(),
                    dabs,
                    dab_batches: batches,
                    ..packet(document.scene(), [128, 128])
                })
                .unwrap();
            };
        let preview = |r: &mut WgpuRasterizer, target| crate::source_thumbnails::tests::thumbnail(r, target);
        frame(&mut r, &document, &[dab], std::slice::from_ref(&batch));
        let image = preview(&mut r, layer_render::ThumbnailTarget::Source(paint));
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
            preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper))
                .chunks_exact(4)
                .all(|p| p[..3] == [255; 3])
        );
        native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
            layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [1., 0., 0., 1.]).unwrap()));
        document.artwork.occurrences.get_mut(paper).unwrap().opacity = 0.25;
        frame(&mut r, &document, &[], &[]);
        assert!(preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper)).chunks_exact(4).all(|p| p == [255, 0, 0, 255]), "thumbnail shows the fill content without its layer opacity");
        native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
            layer_core::color::RgbColor { rgba: [1., 1., 1., 0.], ..layer_core::color::RgbColor::WHITE }));
        frame(&mut r, &document, &[], &[]);
        let transparent_paper = preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper));
        assert_ne!(transparent_paper[0], transparent_paper[4 * 4]);
        assert!(
            transparent_paper
                .chunks_exact(4)
                .all(|p| p[0] == p[1] && p[1] == p[2])
        );
        // Previously allocated pages must not contribute empty bounds after erase.
        batch.style.mode = DabMode::Erase;
        dab.radii = [10., 10.];
        frame(&mut r, &document, &[dab], &[batch]);
        let empty = preview(&mut r, layer_render::ThumbnailTarget::Source(paint));
        assert_eq!(empty, transparent_paper);
    }

    #[test]
    fn zoomed_out_previews_evaluate_paint_in_blocks_under_half_a_surface_pixel() {
        let scaled = |scale: f32| [scale, 0., 0., scale, 0., 0.];
        assert_eq!(preview_block(scaled(1.)), 1);
        assert_eq!(preview_block(scaled(0.3)), 1);
        assert_eq!(preview_block(scaled(0.2)), 2);
        assert_eq!(preview_block(scaled(0.1)), 4);
        let (sin, cos) = 0.7_f32.sin_cos();
        assert_eq!(preview_block([0.1 * cos, 0.1 * sin, -0.1 * sin, 0.1 * cos, 40., 9.]), 4);
    }

    #[test]
    fn gpu_records_match_shader_layouts() {
        assert_eq!(mem::size_of::<Dab>(), 128);
        assert_eq!(mem::size_of::<DabGpu>(), 160);
        assert_eq!(mem::size_of::<StyleGpu>(), 288);
        assert_eq!(mem::size_of::<TargetGpu>(), 32);
    }

    #[test]
    fn empty_4k_layers_allocate_no_layer_pixels() {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU is required");
        renderer.resize_surface(4096, 4096).unwrap();
        let mut artwork = layer_core::authored::Artwork::new([4096, 4096]).unwrap();
        for id in 1..=128 {
            crate::test_support::add_paint(&mut artwork, format!("Layer {id}"), [4096, 4096]);
        }
        let unused=layer_core::raster::RasterRevision::pending();unused.publish(Err("unused library source failed".into())).unwrap();
        artwork.paint.insert(layer_core::authored::PortableId::random(),layer_core::authored::PaintSource { color_mode: Default::default(),domain:[4096;2],raster:unused,base: None,operations:Arc::default()}).unwrap();
        let index = Arc::new(layer_core::authored::SceneIndex::build(&artwork).unwrap());
        renderer
            .submit(FramePacket {
                view: ViewState { width_px: 4096, height_px: 4096, document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], },
                reset_layers: true,
                ..packet(SceneView::new(&artwork, &index), [4096, 4096])
            })
            .unwrap();
        renderer.wait_idle().unwrap();
        let metrics = renderer.metrics();
        assert_eq!(metrics.paint_pages, 0);
        assert_eq!(metrics.paint_storage_bytes, 0);
        assert_eq!(metrics.preview_storage_bytes, 0);
    }

    #[test]
    fn clipped_empty_damage_never_visits_a_page() {
        for extent in [[1537, 769], [1536, 768]] {
            for rect in [
                Rect {
                    min: Point { x: 1280., y: -900. },
                    max: Point { x: 2000., y: -1. },
                },
                Rect {
                    min: Point { x: -900., y: 270. },
                    max: Point { x: -1., y: 700. },
                },
                Rect {
                    min: Point { x: 2000., y: 270. },
                    max: Point { x: 3000., y: 700. },
                },
                Rect {
                    min: Point { x: 270., y: 1000. },
                    max: Point { x: 700., y: 2000. },
                },
            ] {
                let clipped = pixel_rect(rect, extent);
                assert!(clipped.is_empty());
                assert_eq!(page_coordinates(clipped).count(), 0, "{rect:?}, {extent:?}");
                assert_eq!(clipped.area(), 0);
                assert!(clipped.page_local([5, 0]).is_empty());
            }
        }
        assert_eq!(PixelRect::EMPTY.area(), 0);
    }

    #[test]
    fn pixel_rect_subtraction_preserves_every_pixel_outside_the_overlap() {
        let outer = PixelRect::new(10, 20, 90, 100);
        let overlap = PixelRect::new(30, 40, 70, 80);
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

#[cfg(test)]
mod content_bounds_tests;
#[cfg(test)]
mod artwork_sample_tests;
#[cfg(test)]
mod artwork_statistics_tests;
#[cfg(test)]
mod levels_statistics_tests;
#[cfg(test)]
mod curves_calibration_tests;
#[cfg(test)]
mod lut3d_tests;
#[cfg(target_arch = "wasm32")]
impl WgpuRasterizer {
    pub fn set_snapshot_worker(&mut self, worker: snapshot::BrowserSnapshot) { self.snapshot_worker_callback = Some(worker); }
    pub fn set_browser_image_decoder(&mut self, worker: BrowserImageDecoder) { self.browser_image_decoder = Some(worker); }
    #[cfg(target_arch = "wasm32")]
    pub fn set_browser_nearest_coordinate_decoder(&mut self, worker: BrowserNearestCoordinateDecoder) { self.browser_nearest_coordinate_decoder = Some(worker); }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod package_render_tests;
