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
#[cfg(not(target_arch="wasm32"))]
mod device_poll;
mod resident;
mod residency;
use layer_core::raster::RasterPlane;
use submission::ColorPass;
use pixel_rect::{DocRect, PixelRect, page_coordinates, page_rect, pixel_rect};
use raster::staging::Output as RasterOutput;

use layer_core::{
    AssetId, BrushAccumulation, BrushBlendMode, BrushExecution,
    BrushTip,
    LiquifyMode,
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
pub(crate) type IdMap<K, V> = std::collections::HashMap<K, V, layer_core::authored::vector::IdBuild>;
pub(crate) type IdSet<K> = std::collections::HashSet<K, layer_core::authored::vector::IdBuild>;

mod builtin_masks;
mod paper;
mod bristle_table;
#[cfg(test)]
mod kernel_spirv;
mod color_sample;
mod source_access;
mod material_sources;
mod brush_tiles;
mod dry_material;
mod destination_material;
mod liquify;
mod brush_stamp;
mod changed_cells;
mod material_tiles;
mod bindings;
use brush_tiles::BrushTile;
#[cfg(not(target_arch = "wasm32"))]
mod export_readback;
mod view_color;
mod working_color;
#[cfg(test)]
mod flat_areas_tests;
pub use view_color::SdrSurfaceColor;
mod raster;
mod deferred;
mod compile;
mod paint_transform;
mod upload_capacity;
#[cfg(all(test,not(target_arch="wasm32")))]
mod upload_capacity_tests;
mod pixel_transform;
mod object_sampling;
mod vector;
mod object_image_mips;
mod moving_projection;
use moving_projection::MovingProjection;
pub use scene::sources::PreparedSourcePixels as PreparedImagePixels;
pub use object_image_mips::{prepare_image_tile, prepare_raster_tile,prepare_image_tile_from_bytes,prepare_raster_tile_from_bytes};
pub use object_sampling::prepare_nearest_coordinates;
#[cfg(target_arch = "wasm32")]
pub type BrowserImageDecoder = std::rc::Rc<dyn Fn(Arc<layer_core::color::source::SourceImage>, [u32; 2], layer_core::color::RgbSpace)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PreparedImagePixels, String>>>>>;
#[cfg(target_arch="wasm32")]
pub type BrowserRasterDecoder = std::rc::Rc<dyn Fn(Arc<layer_core::raster::TileBlob>, layer_core::color::RgbSpace, layer_core::color::RgbSpace, bool)
    -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<PreparedImagePixels,String>>>>>;
#[cfg(target_arch="wasm32")]
pub type BrowserNearestCoordinateDecoder = std::rc::Rc<dyn Fn([f64;6],[u32;2],u32,u32)
    -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<Arc<Vec<u8>>,String>>>>>;
use deferred::{Compilation, CompileMode, Deferred};
mod pipeline_device;
#[cfg(not(target_arch = "wasm32"))]
mod saved_pipelines;
#[cfg(not(target_arch = "wasm32"))]
pub use saved_pipelines::SavedPipelines;
mod renderer_factory;
mod memory;
pub use memory::new_drawing_working_bytes as new_drawing_memory_requirement;
pub use renderer_factory::RendererFactory;
use pipeline_device::PipelineDevice;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod startup_pixels_tests;
mod startup;
pub use startup::{StartupProgress, ShaderDocument, ShaderBrushKey};
#[cfg(not(target_arch = "wasm32"))]
pub use startup::{finish_shader_compiler_shutdown, stop_shader_compilers};
#[cfg(not(target_arch = "wasm32"))]
pub use startup::ShaderActivity;
#[cfg(all(feature = "test-support", not(target_arch = "wasm32")))]
pub use startup::enqueue_compiler_failure;
mod effect_validation;
mod effects;
mod shader_limits;
mod gradient;
mod flood;
mod frame_timing;
mod performance_trace;
mod layer_masks;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod cold_camera_storm;
mod frame_demand;
#[cfg(test)]
mod test_support;
mod present;
mod present_transition;
pub use present_transition::HeldCanvas;
mod backdrop_blur;
pub use backdrop_blur::{BackdropBlurStyle, BackdropRegion};
mod panel_surface;
pub use panel_surface::{PanelSurface, PanelSurfaceGroup};
mod present_picker;
mod artwork;
mod display_precision;
mod display_mips;
#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
mod display_memory;
#[cfg(not(target_arch = "wasm32"))]
mod memory_sample;
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
const READBACK_TIMEOUT: Duration = layer_core::raster::CAPTURE_TIMEOUT;
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
const TRANSPORT_OFFSETS: [[i32; 2]; 5] = [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]];

pub(crate) fn initialized_buffer(device: &wgpu::Device, descriptor: &wgpu::util::BufferInitDescriptor<'_>) -> Result<wgpu::Buffer, GpuRasterError> {
    let size = descriptor.contents.len() as u64;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: descriptor.label, size: size.next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT), usage: descriptor.usage,
        mapped_at_creation: size != 0,
    });
    if size != 0 {
        buffer.get_mapped_range_mut(..).map_err(|e| GpuRasterError::MapFailed(e.to_string()))?
            .slice(..descriptor.contents.len()).copy_from_slice(descriptor.contents);
        buffer.unmap();
    }
    Ok(buffer)
}

/// Uploads are encoded at their point of use on every host. Queue::write_buffer
/// runs before submitted commands, so it cannot replace ordered copies when
/// source decoding, brush neighborhoods and publication reuse uniform offsets.
struct Uploads {
    belt: wgpu::util::StagingBelt,
    device: PipelineDevice,
    chunk_size: u64,
    retained: u64,
    chunks: std::collections::HashSet<wgpu::Buffer>,
    written: u64,
    outstanding: Arc<residency::Tier>,
}
impl Uploads {
    fn memory_bytes(&self) -> [u64; 2] {
        let inflight = self.outstanding.used().saturating_add(self.written);
        [self.retained, inflight]
    }
    fn trim_unused(&mut self, limit: u64) -> u64 {
        if self.retained <= limit || self.memory_bytes()[1] != 0 { return 0; }
        self.belt = wgpu::util::StagingBelt::new((*self.device).clone(), self.chunk_size);
        self.chunks.clear();
        std::mem::take(&mut self.retained)
    }
    fn new(device: &PipelineDevice, chunk_size: u64) -> Self {
        let chunk_size = if cfg!(target_arch = "wasm32") { 0 } else { chunk_size };
        Self {
            belt: wgpu::util::StagingBelt::new((**device).clone(), chunk_size),
            device: device.for_resource(), chunk_size, retained: 0, chunks: Default::default(), written: 0,
            outstanding: Default::default(),
        }
    }
    fn allocate(&mut self, size: wgpu::BufferSize, alignment: u64) -> wgpu::BufferSlice<'_> {
        #[cfg(target_arch = "wasm32")]
        self.belt.finish();
        let slice = self.belt.allocate(size, wgpu::BufferSize::new(alignment).unwrap());
        if slice.offset() == 0 {
            let bytes = slice.buffer().size();
            self.written = self.written.saturating_add(bytes);
            if !self.chunks.contains(slice.buffer()) {
                self.chunks.insert(slice.buffer().clone());
                self.retained = self.retained.saturating_add(bytes);
            }
        }
        slice
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
        let slice = self.allocate(size, wgpu::COPY_BUFFER_ALIGNMENT);
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
        self.write_texture_region(encoder, texture, Default::default(), texture.size(), bytes_per_row, fill)
    }
    fn write_tile(&mut self, encoder: &mut wgpu::CommandEncoder, tile: &source_access::TileTexture, bytes_per_row: u32,
        fill: impl FnOnce(&mut wgpu::BufferViewMut)) -> Result<(), GpuRasterError> {
        self.write_texture_region(encoder, tile.raw(), tile.as_image_copy().origin, tile.size(), bytes_per_row, fill)
    }
    fn write_texture_region(&mut self, encoder:&mut wgpu::CommandEncoder, texture:&wgpu::Texture,
        origin:wgpu::Origin3d, extent:wgpu::Extent3d, bytes_per_row:u32, fill:impl FnOnce(&mut wgpu::BufferViewMut),
    )->Result<(),GpuRasterError> {
        let size = wgpu::BufferSize::new(u64::from(bytes_per_row) * u64::from(extent.height))
            .ok_or(GpuRasterError::SizeOverflow)?;
        let slice = self.allocate(size, u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
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
                    rows_per_image: Some(extent.height),
                },
            },
            wgpu::TexelCopyTextureInfo {origin,..texture.as_image_copy()}, extent,
        );
        Ok(())
    }
    fn finish(&mut self, encoder: &wgpu::CommandEncoder) {
        self.belt.finish_and_recall_on_submit(encoder);
        let bytes = std::mem::take(&mut self.written);
        if bytes != 0 {
            let flight = self.outstanding.charge(bytes);
            self.device.on_encoder_done(encoder,move || drop(flight));
        }
    }
}

impl WgpuRasterizer {
    async fn await_capture_submission(&mut self,idle:bool)->Result<(),GpuRasterError> {
        if cfg!(target_arch="wasm32") && !idle && self.uploads.memory_bytes()[1]<=16*1024*1024 {return Ok(());}
        crate::local_tone::wait_async(&self.device,&self.queue).await.map_err(GpuRasterError::Color)
    }
}

struct DisplayRetirement {
    _charge:residency::Charge,
    #[cfg(not(target_arch="wasm32"))]
    _scratch:(Vec<PageSurface>,Vec<scene::Image>),
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
    /// Passes that convert display levels to half float outside a reduction.
    pub half_level_passes: u64,
    pub frame_composited_pages: Vec<(u32, [u32; 2])>,
    /// 16-pixel tiles the vector coverage kernel drew.
    pub vector_tiles: u64,
    /// Layer pages reduced into thumbnail levels.
    pub thumbnail_pages: u64,
    pub paint_pages: u64,
    pub preview_pages: u64,
    pub destination_companion_pages: u64,
    pub coverage_pages: u64,
    pub material_pages: u64,
    pub raster_backing_reserved_bytes: u64,
    /// Peak native source upload charge. Packed imports include CPU/GPU packing
    /// tiles; tiled sources charge in-flight staging here and their fixed GPU
    /// cache in scene storage. Excludes retained source bytes and driver overhead.
    pub source_upload_peak_bytes: u64,
    pub source_upload_submissions: u64,
    pub source_tile_hits: u64,
    pub source_tile_misses: u64,
    pub source_tile_evictions: u64,
    /// Transform previews kept at their last pose because a pixel would read
    /// more source than one pass binds.
    pub transform_reach_refusals: u64,
    pub moving_image_evictions: u64,
    pub moving_image_request_bytes: u64,
    pub material_sample_storage_bytes: u64,
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

pub fn rendering_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features() & (wgpu::Features::FLOAT32_FILTERABLE | wgpu::Features::FLOAT32_BLENDABLE | wgpu::Features::PIPELINE_CACHE)
        | native_tiles::native_in_place_features(adapter)
}

pub fn rendering_limits(supported: wgpu::Limits) -> wgpu::Limits {
    let full = supported.max_compute_workgroup_storage_size >= 16384;
    let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(supported);
    if full { limits.max_compute_workgroup_storage_size = limits.max_compute_workgroup_storage_size.max(16384); }
    limits
}

pub fn memory_hints() -> wgpu::MemoryHints {
    wgpu::MemoryHints::Manual { suballocated_device_memory_block_size: (16 << 20)..(16 << 20) }
}

pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload.downcast_ref::<String>().cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_owned()))
        .unwrap_or_else(|| "Panicked without a message".into())
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
    NotReady,
    NotPublished,
    Invariant(&'static str),
    StaleExpression,
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
    TransformReach,
    MultiplePreviewLayers,
    SizeOverflow,
    UploadLimit {required:u64,limit:u64},
    UploadAllocationFailed,
    MapFailed(String),
    WaitFailed(String),
    FilterPreviewCancelled,
    Effect(String),
}

impl fmt::Display for GpuRasterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Color(message) => write!(formatter, "color: {message}"),
            Self::NotReady => formatter.write_str("Canvas inputs are not ready"),
            Self::NotPublished => formatter.write_str("Canvas pixels are not published yet"),
            Self::Invariant(message) => write!(formatter, "Renderer invariant failed: {message}"),
            Self::StaleExpression => formatter.write_str("Composite expression no longer exists"),
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
            Self::TransformReach => formatter.write_str("A transformed pixel reads more of its source than one pass binds"),
            Self::MultiplePreviewLayers => {
                formatter.write_str("one frame cannot preview strokes on multiple layers")
            }
            Self::UploadLimit {required,limit}=>write!(formatter,"Canvas upload requires {required} bytes; device limit is {limit}"),
            Self::UploadAllocationFailed=>formatter.write_str("Could not allocate canvas upload memory"),
            Self::SizeOverflow => formatter.write_str("GPU buffer size overflow"),
            Self::MapFailed(message) => write!(formatter, "GPU readback mapping failed: {message}"),
            Self::WaitFailed(message) => write!(formatter, "GPU completion wait failed: {message}"),
        }
    }
}

impl GpuRasterError {
    pub(crate) fn is_preparation_pending(&self)->bool {matches!(self,Self::NotReady|Self::NotPublished|Self::SourceWorkingSetExceeded)}
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

/// What a material pass reads besides its destination page.
#[derive(Clone, Copy)]
enum MaterialInputs<'a> {
    /// The pages around the destination.
    Neighborhood,
    /// A gathered sample field.
    /// A retouching source's target and reference pages, in the eight slots
    /// around the destination.
    Retouch(&'a [wgpu::TextureView; 8]),
}

/// Brushes whose output pixels read only the same pixel of their destination
/// and of per-page inputs, so tiles, dab ranges and damage stay per page.
fn pointwise(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Dry || style.execution.retouches()
}

fn ordered_dither(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Smudge && style.blender.kind == layer_core::BlenderKind::Dither
}

fn stable_blender(style: &layer_render::DabStyle) -> bool {
    style.execution == BrushExecution::Smudge
        && matches!(style.blender.kind, layer_core::BlenderKind::Dither | layer_core::BlenderKind::Scatter)
}

fn stroke_coverage_slabs(style: &layer_render::DabStyle) -> u32 {
    if ordered_dither(style) { 2 + u32::from(style.rendering.edge_after_stroke) } else { 1 }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrushEncodingTarget {
    Persistent,
    Preview { from_persistent: bool },
}

impl BrushEncodingTarget {
    fn is_preview(self) -> bool {
        matches!(self, Self::Preview { .. })
    }

    fn material_source(self) -> Self {
        if self == (Self::Preview { from_persistent: true }) { Self::Persistent } else { self }
    }
}

fn has_brush_work(batch: &DabBatch) -> bool {
    batch.dab_count > 0 || batch.style.liquify.as_ref().is_some_and(|c| c.amount.is_some()
        || batch.kind == DabBatchKind::Preview && !batch.stroke_start)
}

fn prepared_raster_output<T>(output: Option<T>) -> Result<T, GpuRasterError> {
    #[cfg(test)]
    if output.is_none() { test_support::PassCounts { raster_output_misses: 1, ..Default::default() }.record(); }
    debug_assert!(output.is_some(), "accepted raster output is not prepared");
    output.ok_or(GpuRasterError::Invariant("accepted raster output is not prepared"))
}

fn prepared_brush_error(error:GpuRasterError)->GpuRasterError {
    if error.is_preparation_pending() {GpuRasterError::Invariant("prepared brush input expectation is missing")}else {error}
}

#[derive(Clone, Copy)]
struct BrushEncodingContext<'a> {
    source:SceneView<'a>,
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
    stroke_edge: bool,
}

impl BrushPassPlan {
    fn for_style(style: &layer_render::DabStyle) -> Self {
        let grouped = pointwise(style) && style.opacity < 1.;
        let state = BrushStateTargets {
            // Watercolor's physical shader always reads/writes stroke coverage,
            // independently of the ordinary brush accumulation preference.
            coverage: (style.rendering.accumulation == BrushAccumulation::Uniform
                && (!grouped || style.contact.is_some_and(|c| c.bristles.is_some())
                    || matches!(style.execution, BrushExecution::Clone | BrushExecution::Heal | BrushExecution::SpotHeal)))
                || (style.opacity < 1. && !grouped)
                || style.rendering.edge_after_stroke
                || style.execution == BrushExecution::Watercolor
                || stable_blender(style),
            watercolor_wetness: style.execution == BrushExecution::Watercolor,
        };
        let needs_destination = style.execution != BrushExecution::Dry
            || grouped
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
}

struct StrokeCoveragePage {
    coordinate: [u32; 2],
    primary: resident::Resident<PageSurface>,
    secondary: resident::Resident<PageSurface>,
    active_secondary: bool,
    // Persistent coverage restarts from zero when a stroke claims it: the
    // claiming batch reads the shared zero scalar. Prediction forks committed
    // coverage. Each batch writes the whole inactive surface.
    owner: Option<StrokeId>,
    state:Option<MaterialState>,
}

impl StrokeCoveragePage {
    fn matches(&self, coordinate: [u32; 2], style: &layer_render::DabStyle) -> bool {
        self.coordinate == coordinate && self.primary.size().height == PAGE_SIZE * stroke_coverage_slabs(style)
            && (self.primary.format() == wgpu::TextureFormat::Rgba32Float) == stable_blender(style)
    }

    fn release_color_bindings(&self) {
        for surface in [&self.primary, &self.secondary] {
            surface.clear_bindings();
        }
    }

    fn active(&self) -> &resident::Resident<PageSurface> {
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
    pending:Option<(MaterialState,resident::Written,Option<scene::PreviewVersion>)>,
    derived:Option<scene::PreviewVersion>,
    coordinate: [u32; 2],
    primary: resident::Resident<PageSurface>,
    secondary: resident::Resident<PageSurface>,
    active_secondary: bool,
    primary_needs_clear: bool,
    secondary_needs_clear: bool,
}

impl WatercolorWetnessPage {
    fn surface_mut(&mut self,secondary:bool)->&mut resident::Resident<PageSurface> {if secondary {&mut self.secondary}else {&mut self.primary}}
    fn write<'a>(&'a mut self,secondary:bool,intent:&raster::contents::Intent,receipt:&'a submission::Submission)->Option<resident::RasterWrite<'a,PageSurface>> {
        if secondary==self.active_secondary {return self.surface_mut(secondary).write(intent,receipt);}
        let input=self.active().read(intent.version(),Some(receipt))?.input();self.surface_mut(secondary).overwrite_from(intent,&input,receipt)
    }

    fn active(&self) -> &resident::Resident<PageSurface> {
        if self.active_secondary {
            &self.secondary
        } else {
            &self.primary
        }
    }
}

struct MaterialState {
    stroke:StrokeId,
    input:raster::contents::Intent,
    version:layer_core::raster::ContentVersion,
}
impl MaterialState {
    fn matches(&self,stroke:StrokeId,input:&raster::contents::Intent)->bool {self.stroke==stroke && self.input.same_selection(input)}
}
struct BrushReservoir {
    state:Option<MaterialState>,
    primary: resident::Resident<PageSurface>,
    secondary: resident::Resident<PageSurface>,
    active_secondary: bool,
}


// Scratch descriptors retain capacity, but never retain page resources after encoding.
struct WatercolorTransportJob {
    coordinate: [u32; 2],
    local: PixelRect,
    record_offset: u32,
    sources: wgpu::BindGroup,
}

enum MaterialOutputKind {Color,Coverage,Wetness,Reservoir}
struct MaterialPublication {
    kind:MaterialOutputKind,
    coordinate:[u32;2],
    preview:Option<scene::PreviewVersion>,
    input:raster::contents::Intent,
    encoding:resident::Encoding,
}

struct MaterialWritten {
    kind:MaterialOutputKind,
    coordinate:[u32;2],
    preview:Option<scene::PreviewVersion>,
    input:raster::contents::Intent,
    written:resident::Written,
}

impl MaterialPublication {
    fn encoded(self,receipt:&submission::Submission)->MaterialWritten {
        MaterialWritten {kind:self.kind,coordinate:self.coordinate,preview:self.preview,input:self.input,written:self.encoding.publish(receipt)}
    }
}

struct MaterialJob {
    coordinate: [u32; 2],
    local: PixelRect,
    destination_secondary: bool,
    coverage_destination_secondary: Option<bool>,
    dabs: std::ops::Range<u32>,
    page_index: usize,
    coverage_index: Option<usize>,
}

struct StrokeStart {state:MaterialState,preview:Option<scene::PreviewVersion>,surface:resident::Resident<PageSurface>}
impl StrokeStart {
    fn matches(&self,stroke:StrokeId,input:&raster::contents::Intent,declaration:Option<&Arc<scene::PreviewDeclaration>>)->bool {
        self.state.matches(stroke,input) && self.preview.as_ref().is_none_or(|preview|declaration.is_some_and(|declaration|preview.same_declaration(declaration)))
    }
    fn storage_bytes(&self)->u64 {self.surface.storage_bytes()}
}
struct LayerPage {
    derived:Option<scene::PreviewVersion>,
    coordinate: [u32; 2],
    primary: resident::Resident<PageSurface>,
    // Inactive color is overwritten by a full-page copy, dry draw or post-stroke
    // edge pass before use. Only a new primary needs a separate clear.
    secondary: Option<resident::Resident<PageSurface>>,
    active_secondary: bool,
    primary_needs_clear: bool,
    stroke_start: Option<StrokeStart>,
}

struct PageSurface {
    preview: std::cell::Cell<Option<(SourceTarget,[u32;2])>>,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    texture_bind_group: wgpu::BindGroup,
    material_output: bindings::MaterialOutput,
    scene_output: std::cell::OnceCell<wgpu::BindGroup>,
    charge: Arc<residency::Charge>,
}

#[derive(Default)]
struct RetainedTextures(Vec<wgpu::Texture>);
impl RetainedTextures {
    fn insert(&mut self,texture:wgpu::Texture)->bool {
        match self.0.binary_search(&texture) {Ok(_)=>false,Err(index)=>{self.0.insert(index,texture);true}}
    }
}

fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let format = texture.format();
    let (block_width, block_height) = format.block_dimensions();
    let block_bytes = u64::from(format.block_copy_size(None).unwrap());
    (0..texture.mip_level_count()).map(|level| {
        let size = texture.size().mip_level_size(level, texture.dimension()).physical_size(format);
        u64::from(size.width / block_width)
            * u64::from(size.height / block_height)
            * u64::from(size.depth_or_array_layers)
            * block_bytes
    }).sum()
}
impl resident::Storage for PageSurface {fn storage_bytes(&self)->u64 {self.storage_bytes()}}
impl resident::TextureStorage for PageSurface {
    fn texture(&self)->&wgpu::Texture {&self.texture}
    fn view(&self)->&wgpu::TextureView {&self.view}
}
impl PageSurface {
    fn sharing(&self) -> Self {
        Self {
            texture:self.texture.clone(),view:self.view.clone(),texture_bind_group:self.texture_bind_group.clone(),
            preview:Default::default(),
            material_output:Default::default(),scene_output:Default::default(),
            charge:self.charge.clone(),
        }
    }
    fn discharge(&mut self) { self.charge = Arc::new(self.charge.tier().charge(0)); }
    fn scene_output(&self, r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout) -> &wgpu::BindGroup {
        self.scene_output.get_or_init(|| bindings::group(&r.device, "scene page output", layout,
            [wgpu::BindingResource::TextureView(&self.view)]))
    }
    fn storage_bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }

    fn copy_from(&self, source: impl Into<source_access::TileTexture>, extent: wgpu::Extent3d, encoder: &mut crate::submission::CommandEncoder) {
        let source = &source.into();
        self.preview.set(None);
        encoder.copy_texture_to_texture(source.as_image_copy(), self.texture.as_image_copy(), extent);
    }

    fn clear(&self, zeros: &[wgpu::Texture; 2], encoder: &mut crate::submission::CommandEncoder) {
        let source = zeros.iter().find(|source| source.format() == self.texture.format())
            .expect("page format has an immutable zero source");
        self.preview.set(None);
        for y in (0..self.texture.height()).step_by(source.height() as usize) {
            encoder.copy_texture_to_texture(source.as_image_copy(), wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x: 0, y, z: 0 }, ..self.texture.as_image_copy()
            }, wgpu::Extent3d { width: self.texture.width(), height: source.height().min(self.texture.height() - y), depth_or_array_layers: 1 });
        }
    }
}

impl LayerPage {
    fn discharge(&mut self) {
        std::iter::once(&mut self.primary).chain(self.secondary.as_mut()).chain(self.stroke_start.as_mut().map(|start|&mut start.surface))
            .for_each(resident::Resident::discharge);
    }
    fn clear_stroke_start(&mut self) {
        if self.stroke_start.take().is_some() {
            for surface in std::iter::once(&self.primary).chain(self.secondary.as_ref()) {
                surface.clear_bindings();
            }
        }
    }
    fn surface(&self, secondary: bool) -> &resident::Resident<PageSurface> {
        if secondary {
            self.secondary
                .as_ref()
                .expect("destination page has a secondary surface")
        } else {
            &self.primary
        }
    }

    fn surface_mut(&mut self,secondary:bool)->&mut resident::Resident<PageSurface> {
        if secondary {self.secondary.as_mut().expect("destination page has a secondary surface")}else {&mut self.primary}
    }
    fn active_mut(&mut self)->&mut resident::Resident<PageSurface> {self.surface_mut(self.active_secondary)}
    fn write<'a>(&'a mut self,secondary:bool,intent:&raster::contents::Intent,receipt:&'a submission::Submission)
        ->Option<resident::RasterWrite<'a,PageSurface>> {
        if secondary==self.active_secondary {return self.surface_mut(secondary).write(intent,receipt);}
        let input=self.active().read(intent.version(),Some(receipt))?.input();
        self.surface_mut(secondary).overwrite_from(intent,&input,receipt)
    }

    fn active(&self) -> &resident::Resident<PageSurface> {
        self.surface(self.active_secondary)
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
    relief: f32,
}

#[derive(Clone)]
struct Pipelines {
    dry_material: dry_material::Pipelines,
    destination_material: destination_material::Pipelines,
    direct: [Deferred<wgpu::RenderPipeline>; DirectPipelineKind::COUNT],
    watercolor_transport: [Deferred<wgpu::ComputePipeline>; WATERCOLOR_TRANSPORT_STEPS as usize],
    stroke_edge: Deferred<wgpu::RenderPipeline>,
    watercolor_composite: Deferred<wgpu::RenderPipeline>,
    watercolor_compute: (wgpu::BindGroupLayout, Deferred<wgpu::ComputePipeline>),
    export: Deferred<wgpu::RenderPipeline>,
}

impl Pipelines {
    fn direct(&self, kind: DirectPipelineKind) -> &wgpu::RenderPipeline {
        &self.direct[kind as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Initialization {
    #[cfg(not(target_arch="wasm32"))]
    Headless,
    Interactive,
    /// A worker compiles only the dependencies of its immutable capture.
    Snapshot,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct DrawingPresentation {
    document: Option<(u64, layer_core::PortableId)>,
    showing: bool,
    navigator: bool,
}

/// Headless-capable wgpu brush renderer. A platform presenter can sample the
/// same composite texture rather than requesting readback.
pub struct WgpuRasterizer {
    #[cfg(not(target_arch = "wasm32"))]
    memory_samples: Arc<memory_sample::MemorySampler>,
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
    restore_preflight:Option<raster::staging::RestorePreflight>,
    source_receipt:Option<std::sync::Weak<scene::sources::PendingTile>>,
    source_results:std::collections::VecDeque<(layer_core::color::RgbSpace,object_image_mips::PreparedImageWork)>,
    bake_preflight:Option<scene::bake_preflight::BakePreflight>,
    fill_edges_preflight:Option<scene::fill_edges::FillEdgesPreflight>,
    raster_buffers: std::sync::Arc<raster::BufferPool>,
    raster_tiers: memory::RasterTiers,
    layer_masks: layer_masks::MaskRenderer,
    selection_clip: selection_clip::SelectionClip,
    display_selection: Option<(layer_core::Selection, wgpu::Buffer)>,
    pending_display_selection: Option<layer_core::Selection>,
    /// Counts replacements of the displayed selection; the latest one
    /// repaints `display_selection_damage`, in document pixels.
    display_selection_revision: u64,
    display_selection_damage: PixelRect,
    selection_painter: Option<selection_paint::SelectionPainter>,
    selection_overlay: Option<layer_render::SelectionOverlay>,
    crop_overlay: Option<layer_render::CropOverlay>,
    paper_overlay: Option<layer_render::PaperOverlay>,
    paper_overlay_drawn: std::cell::Cell<bool>,
    paper_lighting: Option<paper::PaperLighting>,
    paper_export: Option<Arc<paper::ExportShade>>,
    vector_preview: Option<layer_render::VectorPreview>,
    vector_preview_shown: Option<(OccurrenceHandle, u64, DocRect)>,
    vector_preview_drawn: Option<layer_render::VectorPreview>,
    vector_preview_damage: Vec<DocRect>,
    vector_motion: Option<layer_render::VectorMotion>,
    vector_motion_drawn: Option<(layer_render::VectorMotion, DocRect)>,
    vector_motion_paths: Option<Arc<[layer_core::authored::vector::PathItem]>>,
    vector_motion_paths_drawn: Option<Arc<[layer_core::authored::vector::PathItem]>>,
    vector_motion_damage: Vec<DocRect>,
    vector_exclusion_damage: Vec<DocRect>,
    vector_motion_split: Option<scene::MotionSplit>,
    vector_selection: Option<layer_core::authored::VectorItemSelection>,
    pivot: Option<layer_core::OccurrenceHandle>,
    vector_selection_changed: bool,
    vector_prepared: Option<(scene::Prepared, layer_core::authored::vector::VectorObject, [f64; 2])>,
    clipping_preview: [bool; 2],
    selection_previews: selection_previews::SelectionPreviews,
    selection_paint_revision: u64,
    selection_paint_damage: PixelRect,
    regions: Option<region_requests::RegionRequests>,
    unclipped: wgpu::Buffer,
    scene: Option<Box<scene::Scene>>,
    source_tiles: std::cell::RefCell<scene::sources::DecodedTiles>,
    moving_images: object_image_mips::MovingImages,
    moving_projection: Option<MovingProjection>,
    moving_images_waiting: bool,
    moving_decode: Option<(layer_core::color::RgbSpace, object_image_mips::MipDecodeQueue)>,
    preparation_progress: u64,
    preparation_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(not(target_arch = "wasm32"))]
    decode_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    arrivals: Arc<std::sync::atomic::AtomicU64>,
    observed_completions: std::sync::atomic::AtomicU64,
    failed_image_tiles: Vec<moving_projection::FailedImageTile>,
    failed_raster_tiles: Vec<(std::sync::Weak<layer_core::raster::TileBlob>,layer_core::color::RgbSpace,layer_core::color::RgbSpace,String)>,
    #[cfg(target_arch = "wasm32")]
    browser_image_decoder: Option<BrowserImageDecoder>,
    #[cfg(target_arch="wasm32")]
    browser_raster_decoder: Option<BrowserRasterDecoder>,
    #[cfg(target_arch = "wasm32")]
    browser_nearest_coordinate_decoder: Option<BrowserNearestCoordinateDecoder>,
    #[cfg(target_arch = "wasm32")]
    browser_image_result: std::rc::Rc<std::cell::RefCell<Option<(u64,object_image_mips::PreparedImageWork)>>>,
    #[cfg(target_arch = "wasm32")]
    browser_image_wake: std::rc::Rc<std::cell::RefCell<Option<std::task::Waker>>>,
    snapshot_decode_wake: Option<Arc<std::sync::Mutex<Option<std::task::Waker>>>>,
    transforms: Option<paint_transform::PaintTransforms>,
    compilation: compile::Scope,
    context_compilation: compile::Scope,
    preparation: layer_render::PreparationContext,
    transform_preview: Option<layer_render::TransformPreview>,
    fill_preview: Option<layer_render::FillPreview>,
    fill_preview_shown: Option<(u64,u64,SourceTarget)>,
    fill_preview_done:Option<Arc<std::sync::atomic::AtomicBool>>,
    fill_reveal: Option<layer_render::FillReveal>,
    refused_transform: Option<layer_render::TransformPreview>,
    transform_sources_pending: bool,
    transform_damage: Vec<(SourceTarget, PixelRect)>,
    /// Document regions to recompose whose layers' own pixels are unchanged.
    document_damage: Vec<PixelRect>,
    background_ready: Arc<std::sync::atomic::AtomicBool>,
    background_refinement: bool,
    moving_pixels: Option<(SourceTarget, layer_core::Selection)>,
    moving_layer: Option<OccurrenceHandle>,
    #[cfg(test)]
    test: TestHooks,
    thumbnails: thumbnails::Thumbnails,
    brush_stamps: Option<brush_stamp::BrushStamps>,
    ui_preview_space: layer_core::color::RgbSpace,
    ui_rendition: Option<layer_core::color::hdr::SdrRendition>,
    ui_preview_pipeline: Option<Deferred<wgpu::RenderPipeline>>,
    display_pipelines: Option<display_mips::Pipelines>,
    scale_display: Option<scene::scale::Cache>,
    retired_display_bytes: Arc<residency::Tier>,
    scene_tiers: scene::Tiers,
    navigator: scene::scale::Navigator,
    color_sampler: color_sample::ColorSampler,
    composite_revision: u64,
    presentation: DrawingPresentation,
    canvas_transition: layer_render::CanvasFade,
    artwork_revision: u64,
    composite_damage: PixelRect,
    /// The composite's blend space, from the latest frame.
    blend_space: layer_core::BlendSpace,
    filter_previews: Option<Box<scene::FilterPreviews>>,
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
    tiled_sources: std::collections::BTreeMap<SourceTarget, layer_core::authored::PaintBase>,
    preview_declaration:Option<Arc<scene::PreviewDeclaration>>,
    preview_pages: Vec<LayerPage>,
    preview_valid: bool,
    preview_coverage_pages: Vec<StrokeCoveragePage>,
    live_stroke: Option<StrokeId>,
    preview_watercolor_wetness_pages: Vec<WatercolorWetnessPage>,
    preview_damage: PixelRect,
    preview_footprint: std::collections::BTreeSet<[u32; 2]>,
    preview_layer_id: Option<SourceTarget>,
    preview_requires_base: bool,
    preview_contribution: bool,
    preview_level: u32,
    masks: Vec<MaskAsset>,
    papers: paper::Papers,
    texture_sets: Vec<TextureSet>,
    sampler: wgpu::Sampler,
    brush_sampler: wgpu::Sampler,
    style_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    float_texture_layout: wgpu::BindGroupLayout,
    advanced_texture_layout: wgpu::BindGroupLayout,
    target_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    material_records: material_sources::TileRecords,
    brush_inputs:Option<scene::sources::AcceptedInputs>,
    held_brush:Option<raster::staging::HeldBrush>,
    preview_working:Option<residency::Reservation>,
    material_bindings: scene::RecentBindings<scene::MaterialBindingKey>,
    transport_bindings: scene::RecentBindings<scene::TransportBindingKey>,
    transport_records: material_sources::TileRecords,
    transport_regions: Vec<PixelRect>,
    preview_names: std::cell::RefCell<Option<IdMap<(SourceTarget,usize,[u32;2],scene::PreviewStage),scene::PreviewVersion>>>,
    material_reads: Option<IdMap<(SourceTarget,[u32;2],Option<(scene::PreviewStage,Option<(scene::PreviewPass,u32)>)>),Option<source_access::RawTile>>>,
    transport_jobs: Vec<WatercolorTransportJob>,
    /// Pages whose wetness the current watercolor update has written.
    watercolor_written: Vec<[u32; 2]>,
    material_jobs: Vec<MaterialJob>,
    dry_jobs: Vec<dry_material::Job>,
    dry_display_inputs: Vec<dry_material::DisplayInputs>,
    material_gather: Option<material_sources::Gather>,
    retouch: Option<Box<retouch_sources::RetouchSources>>,
    liquify: Option<Box<liquify::Liquify>>,
    edge_layout: wgpu::BindGroupLayout,
    watercolor_layout: wgpu::BindGroupLayout,
    transport_layout: wgpu::BindGroupLayout,
    style_buffer: wgpu::Buffer,
    style_bind_group: wgpu::BindGroup,
    style_stride: u64,
    style_capacity: usize,
    style_upload: Vec<u8>,
    style_upload_valid: Arc<std::sync::atomic::AtomicBool>,
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
    zero_pages: [wgpu::Texture; 2],
    _zero_charge: residency::Charge,
    declared_scratch: [PageSurface; 2],
    empty_texture: wgpu::Texture,
    empty_view: wgpu::TextureView,
    empty_paper_view: wgpu::TextureView,
    empty_scalar_texture: wgpu::Texture,
    empty_scalar_view: wgpu::TextureView,
    reservoir: BrushReservoir,
    dab_buffer: wgpu::Buffer,
    dab_capacity_bytes: u64,
    pipelines: Arc<Pipelines>,
    factory: RendererFactory,
    memory: memory::Memory,
    recipes: Arc<renderer_factory::Recipes>,
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
    /// Drop the next frame's encoder unsubmitted.
    discard_frame: bool,
}

impl WgpuRasterizer {
    /// Borrowed device/queue for platform surface setup on the same GPU.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn renderer_factory(&self) -> RendererFactory { self.factory.clone() }

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
    async fn headless(color: layer_core::color::DocumentColor, initialization: Initialization, omitted: wgpu::Features) -> Result<Self, GpuRasterError> {
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
        let limits = rendering_limits(adapter.limits());
        let omitted = omitted | if std::env::var_os("CAPY_GPU_NO_FLOAT32_BLEND").is_some() { wgpu::Features::FLOAT32_BLENDABLE } else { wgpu::Features::empty() };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("layer canvas device"),
                required_features: (rendering_features(&adapter)
                    | adapter.features() & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)) & !omitted,
                required_limits: limits,
                memory_hints: memory_hints(),
                ..Default::default()
            })
            .await
            .map_err(|error| GpuRasterError::DeviceRequest(error.to_string()))?;

        Self::from_wgpu_inner(
            adapter,
            PipelineDevice::from(device).with_working_space(color.space).with_depth(color.depth),
            queue,
            initialization,
        ).map(|renderer| *renderer)
    }

    #[cfg(any(not(target_arch = "wasm32"), test))]
    fn from_wgpu_inner(
        adapter: wgpu::Adapter,
        device: PipelineDevice,
        queue: wgpu::Queue,
        initialization: Initialization,
    ) -> Result<Box<Self>, GpuRasterError> {
        Self::from_factory_inner(RendererFactory::with_device(adapter, &device, queue), device, initialization)
    }
    fn from_factory_inner(factory: RendererFactory, device: PipelineDevice, initialization: Initialization) -> Result<Box<Self>, GpuRasterError> {
        let (adapter, queue) = (factory.adapter().clone(), factory.queue().clone());
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu && !software_adapter_tests() {
            return Err(GpuRasterError::HardwareAdapterRequired);
        }
        let (mut device, recipes) = factory.resources(&device);
        let memory = memory::Memory::new(&factory, device.sample_memory.clone());
        device.sample_memory = Some(memory.sample_owner());
        device.resource_limit = memory.resource_limit();
        device.source_samples.set_limit(memory.sample_grant().min(usize::MAX as u64) as usize);
        let scene_tiers = scene::Tiers::default();
        if device.analysis_memory.is_root() { device.analysis_memory = residency::Tier::within(&scene_tiers.display); }
        let sampler = recipes.sampler.clone();
        let brush_sampler = recipes.brush_sampler.clone();
        let style_layout = recipes.style_layout.clone();
        let texture_layout = recipes.texture_layout.clone();
        let float_texture_layout = recipes.float_texture_layout.clone();
        let advanced_texture_layout = recipes.advanced_texture_layout.clone();
        let target_layout = recipes.target_layout.clone();
        let material_layout = recipes.material_layout.clone();
        let edge_layout = recipes.edge_layout.clone();
        let watercolor_layout = recipes.watercolor_layout.clone();
        let transport_layout = recipes.transport_layout.clone();
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
        let empty_paper_view = create_target(&device, [1, 1], wgpu::TextureFormat::R8Unorm, "empty fill paper").1;
        let (empty_scalar_texture, empty_scalar_view) = create_target(
            &device,
            [1, 1],
            device.scalar_format(),
            "layer zero missing paint state",
        );
        let zero_pages = [device.working_format(), device.scalar_format()].map(|format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layer immutable zero page"),
                size: wgpu::Extent3d { width: PAGE_SIZE, height: PAGE_SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let raster_tiers = memory::RasterTiers::default();
        let zero_charge = raster_tiers.paint.charge(zero_pages.iter().map(texture_bytes).sum());
        let declared_scratch = [device.working_format(), device.scalar_format()]
            .map(|format| create_page_surface(&device, &float_texture_layout, &sampler, [1, 1], format, "declared scratch page", &raster_tiers.paint));
        let reservoir = BrushReservoir {
            primary: resident::Resident::unwritten(create_page_surface(
                &device,
                &float_texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                device.working_format(),
                "layer brush reservoir A",
                &raster_tiers.paint,
            )),
            secondary: resident::Resident::unwritten(create_page_surface(
                &device,
                &float_texture_layout,
                &sampler,
                [RESERVOIR_SIZE, RESERVOIR_SIZE],
                device.working_format(),
                "layer brush reservoir B",
                &raster_tiers.paint,
            )),
            active_secondary: false,
            state:None,
        };
        let dab_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("layer dab instances"),
            size: INITIAL_DAB_BYTES,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let pipelines = recipes.pipelines.clone();

        let uploads = Uploads::new(&device, 64 * 1024);
        let restore_uploads = Uploads::new(&device, 64 * 1024);
        let layer_masks =
            layer_masks::MaskRenderer::from_pipelines(recipes.masks.clone());
        let papers = paper::Papers::from_recipes(recipes.paper.clone());
        let telemetry = telemetry::Telemetry::new(&device, &queue);
        let selection_clip = selection_clip::SelectionClip::from_pipelines(recipes.selection.clone(), &raster_tiers.paint);
        let scene_pipelines = recipes.scene.clone();
        let changed_cells = Some(changed_cells::ChangedCells::new(&device, &scene_tiers.display));
        let portable_blend = portable_blend::Renderer::new(&device);
        let transforms = Some(paint_transform::PaintTransforms::from_recipes(&recipes.transforms).charged(&scene_tiers.display));
        #[cfg(not(target_arch = "wasm32"))]
        let memory_samples=factory.memory_samples();
        let compilation = device.compilation()?;
        let context_compilation = device.compilation()?;
        let mut renderer = Box::new(Self {
            #[cfg(not(target_arch = "wasm32"))]
            memory_samples,
            factory,
            memory,
            recipes: recipes.clone(),
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
            pending_display_selection: None,
            display_selection_revision: 0,
            display_selection_damage: PixelRect::EMPTY,
            selection_painter: None,
            selection_overlay: None,
            crop_overlay: None,
            paper_overlay: None,
            paper_overlay_drawn: std::cell::Cell::new(false),
            paper_lighting: None,
            paper_export: None,
            vector_preview: None,
            vector_preview_shown: None,
            vector_preview_drawn: None,
            vector_preview_damage: Vec::new(),
            vector_motion: None,
            vector_motion_drawn: None,
            vector_motion_paths: None,
            vector_motion_paths_drawn: None,
            vector_motion_damage: Vec::new(),
            vector_exclusion_damage: Vec::new(),
            vector_motion_split: None,
            vector_selection: None,
            pivot: None,
            vector_selection_changed: false,
            vector_prepared: None,
            clipping_preview: [false; 2],
            selection_previews: Default::default(),
            selection_paint_revision: 0,
            selection_paint_damage: PixelRect::EMPTY,
            regions: None,
            unclipped,
            scene: None,
            source_tiles: std::cell::RefCell::new(scene::sources::DecodedTiles::within(&scene_tiers.display, Default::default())),
            moving_images: object_image_mips::MovingImages::within(&scene_tiers),
            moving_projection: None,
            moving_images_waiting: false,
            moving_decode: None,
            preparation_progress: 0,
            preparation_wake: None,
            #[cfg(not(target_arch = "wasm32"))]
            decode_wake: None,
            arrivals: Default::default(),
            observed_completions: Default::default(),
            failed_image_tiles: Vec::new(),
            failed_raster_tiles: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            browser_image_decoder: None,
            #[cfg(target_arch="wasm32")]
            browser_raster_decoder: None,
            #[cfg(target_arch = "wasm32")]
            browser_nearest_coordinate_decoder: None,
            #[cfg(target_arch = "wasm32")]
            browser_image_result: Default::default(),
            #[cfg(target_arch = "wasm32")]
            browser_image_wake: Default::default(),
            snapshot_decode_wake: None,
            transforms,
            compilation,
            context_compilation,
            preparation: Default::default(),
            transform_preview: None,
            fill_preview: None,
            fill_preview_shown: None,
            fill_preview_done:None,
            fill_reveal: None,
            refused_transform: None,
            transform_sources_pending: false,
            transform_damage: Vec::with_capacity(2),
            document_damage: Vec::new(),
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
            display_pipelines: None,
            scale_display: None,
            retired_display_bytes: residency::Tier::within(&scene_tiers.display),
            scene_tiers,
            navigator: Default::default(),
            color_sampler: color_sample::ColorSampler::new(&raster_tiers.paint),
            composite_revision: 0,
            presentation: DrawingPresentation::default(),
            canvas_transition: Default::default(),
            artwork_revision: 0,
            composite_damage: PixelRect::EMPTY,
            thumbnails: thumbnails::Thumbnails::new(),
            brush_stamps: None,
            ui_preview_space: layer_core::color::RgbSpace::Srgb,
            ui_rendition: None,
            ui_preview_pipeline: None,
            tiled_sources: Default::default(),
            paint_layers: Vec::with_capacity(8),
            changed_cells,
            raster: None,
            document_color: Default::default(),
            native_edit: None,
            restore_preflight:None,source_receipt:None,source_results:Default::default(),bake_preflight:None,fill_edges_preflight:None,
            raster_buffers: std::sync::Arc::new(raster::BufferPool::within(&raster_tiers.raster)),
            raster_tiers,
            blend_space: layer_core::BlendSpace::Linear,
            preview_declaration:None,
            preview_pages: Vec::with_capacity(16),
            preview_valid: false,
            preview_coverage_pages: Vec::with_capacity(8),
            live_stroke: None,
            preview_watercolor_wetness_pages: Vec::with_capacity(8),
            preview_damage: PixelRect::EMPTY,
            preview_footprint: std::collections::BTreeSet::new(),
            preview_layer_id: None,
            preview_requires_base: false,
            preview_contribution: false,
            preview_level: 0,
            masks: Vec::with_capacity(8),
            papers,
            texture_sets: Vec::with_capacity(16),
            sampler,
            brush_sampler,
            style_layout,
            texture_layout,
            float_texture_layout,
            advanced_texture_layout,
            target_layout,
            material_layout,
            material_records: Default::default(),
            brush_inputs:None,
            held_brush:None,
            preview_working:None,
            material_bindings: Default::default(),
            transport_bindings: Default::default(),
            transport_records: Default::default(),
            transport_regions: Vec::new(),
            preview_names: Default::default(),
            material_reads: None,
            transport_jobs: Vec::new(),
            watercolor_written: Vec::new(),
            material_jobs: Vec::new(),
            dry_jobs: Vec::with_capacity(SOURCE_SLOTS),
            dry_display_inputs: Vec::new(),
            material_gather: None,
            retouch: None,
            liquify: None,
            edge_layout,
            watercolor_layout,
            transport_layout,
            style_buffer,
            style_bind_group,
            style_stride,
            style_capacity,
            style_upload: Vec::with_capacity(style_stride as usize * style_capacity),
            style_upload_valid: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            zero_pages,
            _zero_charge: zero_charge,
            declared_scratch,
            empty_texture,
            empty_view,
            empty_paper_view,
            empty_scalar_texture,
            empty_scalar_view,
            reservoir,
            dab_buffer,
            dab_capacity_bytes: INITIAL_DAB_BYTES,
            pipelines,
            scene_pipelines,
            portable_blend,
            last_submission: None,
            metrics: GpuRasterMetrics::default(),
        });
        #[cfg(not(target_arch = "wasm32"))]
        if initialization == Initialization::Headless {
            renderer.compilation.clone().blocking(||renderer.install_builtin_masks())?;
        }
        if initialization == Initialization::Interactive {
            renderer.upload_mask(&AssetId::from(WHITE_MASK_ASSET), 1, 1, 1, &[255])?;
            renderer.validated_effects = Some(renderer.scene_pipelines.effects(&renderer));
            renderer.startup = Some(startup::Startup::new(renderer.compilation.clone())?);
        }
        renderer.refresh_memory_limits();
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
    pub fn submit_commands(&self,commands:impl IntoIterator<Item=wgpu::CommandBuffer>)->Option<wgpu::SubmissionIndex> {
        self.device.submit(&self.queue,commands)
    }
    pub fn map_read(&self,buffer:&wgpu::Buffer,callback:impl FnOnce(Result<(),wgpu::BufferAsyncError>)+Send+'static) {
        buffer.map_async(wgpu::MapMode::Read,..,self.device.callback(callback));self.device.request_poll();
    }
    pub fn on_gpu_done(&self,callback:impl FnOnce()+Send+'static) { self.device.on_queue_done(&self.queue,callback); }

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

    pub(crate) fn selected_display_cache(&self)->Option<&scene::scale::Cache> {
        let cache=self.scale_display.as_ref()?;
        cache.presentable()
    }
    fn retire_display_resources(&mut self, encoder: &mut submission::CommandEncoder, trim: bool) {
        if !trim {return;}
        let (mut scene,mut cache)=(self.scene.take(),self.scale_display.take());
        self.retire_scratch(scene.as_deref_mut(),cache.as_mut(),encoder);
        self.scene=scene;self.scale_display=cache;
    }
    pub(crate) fn retire_scratch(&self,scene:Option<&mut scene::Scene>,cache:Option<&mut scene::scale::Cache>,encoder:&mut submission::CommandEncoder) {
        let keep=cache.as_ref().map_or(0,|cache|cache.scratch_images());
        let mut pages=scene.map_or_else(Vec::new,|scene|scene.trim_scratch(keep));
        let images=cache.map_or_else(Vec::new,|cache|cache.trim_scratch());
        let bytes=pages.iter().map(PageSurface::storage_bytes).sum::<u64>()
            +images.iter().map(scene::Image::bytes).sum::<u64>();
        if bytes!=0 {
            let retirement=self.retired_display_bytes.charge(bytes);
            pages.iter_mut().for_each(PageSurface::discharge);
            let charge=DisplayRetirement {_charge:retirement,
                #[cfg(not(target_arch="wasm32"))] _scratch:(pages,images)};
            #[cfg(target_arch="wasm32")] drop((pages,images));
            encoder.retire(charge);
        }
    }
    pub fn has_presentable_frame(&self) -> bool { self.presentation_views().is_some() }
    fn activate_presentation(&mut self, scene: layer_core::SceneView<'_>) {
        let document = (scene.owner(), scene.artwork().id);
        if self.presentation.document != Some(document) {
            self.presentation.document = Some(document);
            self.presentation.showing = false;
            self.presentation.navigator = false;
        }
    }
    pub(crate) fn presentation_page(&self)->Option<scene::scale::PresentedPage<'_>> {
        self.selected_display_cache()?.presentation()
    }
    pub(crate) fn presentation_views(&self)->Option<[&wgpu::TextureView;3]> {
        self.presentation_page().map(|page|page.views())
    }
    pub(crate) fn composed_vector_motion(&self) -> Option<&(layer_render::VectorMotion, DocRect)> {
        self.vector_motion_drawn.as_ref().filter(|_| self.vector_motion_split.is_none())
    }
    fn vector_motion_moved(&self) -> Option<(DocRect, DocRect)> {
        let (motion, bounds) = self.vector_motion_drawn.as_ref().filter(|_| self.vector_motion_split.is_some())?;
        Some((*bounds, scene::moved_bounds(motion.transform, *bounds).expand(2)))
    }
    pub(crate) fn vector_motion_region(&self, extent: [u32; 2]) -> Option<PixelRect> {
        let (bounds, moved) = self.vector_motion_moved()?;
        Some(DocRect { min: bounds.min.map(|v| v - 2), max: bounds.max.map(|v| v + 2) }.union(moved).in_frame(extent))
    }
    pub(crate) fn display_motion(&self)->Option<([&wgpu::TextureView;2],[f32;24],DocRect,u64)> {
        let (motion, _) = self.vector_motion_drawn.as_ref().filter(|_| self.vector_motion_split.is_some())?;
        let (_, moved) = self.vector_motion_moved()?;
        let region = self.vector_motion_region(self.document_extent)?;
        let (view, window, level, size, revision) = self.scene.as_ref()?.presented_motion(motion)?;
        let (above, plan) = self.scale_display.as_ref()?.motion_above(region)?;
        let [x, y] = window.min.map(|v| v as f32);
        let [w, h] = size.map(|v| v as f32);
        let [sx, sy] = plan.doc_bounds.min.map(|v| v as f32);
        let [sw, sh] = plan.size.map(|v| v as f32);
        let [a, b, c, d, tx, ty] = motion.transform.inverse()?.0.map(|v| v as f32);
        let [rx0, ry0, rx1, ry1] = [region.min_x(), region.min_y(), region.max_x(), region.max_y()].map(|v| v as f32);
        Some(([view, above.unwrap_or(&self.empty_view)], [x, y, (1u32 << level) as f32, 1., tx, ty, w, h, sx, sy, (1u32 << plan.level) as f32, 1., sw, sh, 0., 0., rx0, ry0, rx1, ry1, a, b, c, d], moved, revision))
    }
    /// Benchmark/export synchronization only. Live drawing never calls this.
    pub fn wait_idle(&mut self) -> Result<(), GpuRasterError> {
        let Some(_submission) = self.last_submission.take() else {
            return Ok(());
        };
        #[cfg(not(target_arch="wasm32"))]
        self.device.wait_queue().map_err(GpuRasterError::WaitFailed)?;
        #[cfg(target_arch="wasm32")]
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(_submission),
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| GpuRasterError::WaitFailed(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(target_arch="wasm32"))]
    fn install_builtin_masks(&mut self) -> Result<(), GpuRasterError> {
        for (id, generate) in builtin_masks::builtin_masks() {
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
        let grain = style.grain.and(style.paper.as_ref()).and_then(paper::PaperTile::id_for).unwrap_or_else(|| white.clone());
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

    fn prepare_paper(&mut self, style: &layer_render::DabStyle, priority: startup::Class) -> Result<(), GpuRasterError> {
        let (Some(_), Some(binding)) = (style.grain, &style.paper) else { return Ok(()) };
        self.prepare_paper_binding(binding, priority)
    }

    fn prepare_paper_binding(&mut self, binding: &layer_core::paper::PaperBinding, priority: startup::Class) -> Result<(), GpuRasterError> {
        let compiler = Some(&self.compilation);
        self.papers.require(&self.device, compiler, binding, priority)?;
        if self.startup.is_none() { self.papers.advance(&self.device, &self.queue, true, None)?; }
        Ok(())
    }

    fn paper_ready(&mut self, style: &layer_render::DabStyle, priority: startup::Class) -> Result<bool, GpuRasterError> {
        self.prepare_paper(style, priority)?;
        Ok(style.grain.and(style.paper.as_ref()).and_then(paper::PaperTile::id_for).is_none_or(|id| self.papers.view(&id).is_some()))
    }

    fn paper_style(&self, style: &layer_render::DabStyle) -> ([f32; 4], f32) {
        let key = Self::texture_set_key(style);
        style.grain.and(style.paper.as_ref()).and_then(|binding| {
            let tile = paper::PaperTile::new(&binding.surface).ok()?;
            Some((tile.grain(binding), self.texture_sets.iter().find(|set| set.key == key)?.relief))
        }).unwrap_or(([1., 1., 0., 0.], 1.))
    }

    fn paper_shade(&self) -> Option<(&wgpu::TextureView, [f32; 16], bool)> {
        let (binding, light, separated, flash) = match (&self.paper_lighting, &self.paper_overlay) {
            (Some(lit), _) => (&lit.binding, lit.light, lit.separated, false),
            (None, Some(overlay)) => (&overlay.paper, paper::flash_light(overlay.opacity), None, true),
            _ => return None,
        };
        let tile = paper::PaperTile::new(&binding.surface).ok()?;
        let exact = self.papers.slope(&tile.id(&binding.surface));
        let (view, stretch) = exact.map(|view| (view, 1.)).or_else(|| self.papers.nearest_slope(&binding.surface, &tile))?;
        let [pixels_x, pixels_y, offset_x, offset_y] = tile.grain(binding);
        let [x, y] = light.direction();
        let [r, g, b, a] = separated.unwrap_or_default();
        Some((view, [pixels_x, pixels_y, offset_x, offset_y, x, y, light.relief * stretch, light.through_paint, r, g, b, a,
            1., f32::from(u8::from(separated.is_some())), f32::from(u8::from(flash)), 0.], flash && exact.is_some()))
    }

    fn require_paper_slope(&mut self, binding: &layer_core::paper::PaperBinding, priority: startup::Class) -> Result<(), GpuRasterError> {
        let compiler = Some(&self.compilation);
        let id = self.papers.require(&self.device, compiler, binding, priority)?;
        self.papers.require_slope(&self.device, compiler, &id, priority);
        if self.startup.is_none() {
            self.papers.advance(&self.device, &self.queue, true, None)?;
        }
        Ok(())
    }

    fn update_paper_lighting(&mut self, scene: layer_core::authored::SceneView<'_>) -> Result<(), GpuRasterError> {
        if scene.scope().is_some_and(|scope| !matches!(scope, layer_core::SceneScope::All)) { return Ok(()); }
        let separated = paper::separated_color(scene, self.device.working_space())?;
        let lighting = layer_core::paper::lit_paper(scene).and_then(|(paper, light)| Some(paper::PaperLighting {
            binding: layer_core::paper::PaperBinding { surface: Arc::new(layer_core::paper::surface(scene, paper)?), target_offset: [0; 2] },
            light,
            separated,
        }));
        if let Some(lighting) = &lighting {
            self.require_paper_slope(&lighting.binding, startup::DOCUMENT)?;
        }
        self.paper_lighting = lighting;
        Ok(())
    }

    fn shade_paper_export(&mut self, scene: layer_core::authored::SceneView<'_>, source: &wgpu::Texture, coverage: Option<&wgpu::Texture>,
        origin: [u32; 2], encoder: &mut wgpu::CommandEncoder) -> Result<Option<wgpu::Texture>, GpuRasterError> {
        if scene.scope().is_some_and(|scope| !matches!(scope, layer_core::SceneScope::All)) { return Ok(None); }
        let Some((sheet, light)) = layer_core::paper::lit_paper(scene) else { return Ok(None) };
        let Some(surface) = layer_core::paper::surface(scene, sheet) else { return Ok(None) };
        let binding = layer_core::paper::PaperBinding { surface: Arc::new(surface), target_offset: [0; 2] };
        if !cfg!(target_arch = "wasm32") {
            self.require_paper_slope(&binding, startup::OTHER)?;
        }
        let tile = paper::PaperTile::new(&binding.surface)?;
        let export = self.paper_export.get_or_insert_with(|| self.recipes.paper_export.get_or_init(|| Arc::new(paper::ExportShade::new(&self.device))).clone());
        let Some(slope) = self.papers.slope(&tile.id(&binding.surface)) else { return Ok(None) };
        let offset = scene.evaluation_offset64();
        let origin = [f64::from(origin[0]) + offset[0], f64::from(origin[1]) + offset[1]];
        export.encode(&self.device, encoder, source, coverage, slope, tile.grain(&binding), light, origin).map(Some)
    }

    fn paper_shade_work_pending(&self) -> bool {
        self.paper_lighting.as_ref().map(|lit| &lit.binding).or(self.paper_overlay.as_ref().map(|o| &o.paper))
            .and_then(paper::PaperTile::id_for).is_some_and(|id| self.papers.work_pending(&id))
    }

    fn texture_view(&self, id: &AssetId) -> Result<&wgpu::TextureView, GpuRasterError> {
        self.papers.view(id).map_or_else(|| self.mask(id).map(|mask| &mask.view), Ok)
    }

    fn ensure_texture_set(&mut self, style: &layer_render::DabStyle) -> Result<TextureSetKey, GpuRasterError> {
        self.prepare_paper(style, startup::BRUSH)?;
        let key = Self::texture_set_key(style);
        if self.texture_sets.iter().any(|set| set.key == key) {
            return Ok(key);
        }
        let primary = self.texture_view(&key.primary)?;
        let grain = self.texture_view(&key.grain)?;
        let transport = self.texture_view(&key.transport)?;
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
        let relief = self.papers.relief(&key.grain).unwrap_or(1.);
        self.texture_sets.push(TextureSet {
            key: key.clone(),
            bind_group,
            transport_bind_group,
            relief,
        });
        Ok(key)
    }

    fn brush_textures_ready(&mut self, style: &layer_render::DabStyle) -> bool {
        let key = Self::texture_set_key(style);
        if self.texture_sets.iter().any(|set| set.key == key) {
            return true;
        }
        if let Some(startup) = &mut self.startup {
            startup.masks.style(&startup.compiler, style, startup::BRUSH);
        }
        if self.prepare_paper(style, startup::BRUSH).is_err() {
            return true;
        }
        [&key.primary, &key.grain, &key.transport].into_iter().all(|id| self.texture_view(id).is_ok()
            || !self.papers.requested(id) && !self.startup.as_ref().is_some_and(|startup| startup.masks.generates(id)))
    }

    fn brush_batches_ready(&mut self, packet: FramePacket<'_>) -> bool {
        let mut ready = true;
        for batch in packet.dab_batches {
            if let DabBatchKind::RasterOperation(index) = batch.kind
                && let Some(operation) = packet.scene.operations(batch.target).and_then(|ops| ops.get(index as usize))
            {
                ready &= self.compilation.require([&self.layer_masks.initialize],startup::BRUSH)
                    & self.compilation.require(self.selection_clip.pipelines(),startup::BRUSH);
                if !batch.target.is_coverage()
                    && let Some(pipeline) = scene::operation_pipeline(&operation.kind) {
                    ready &= self.compilation.require([&self.scene_pipelines.pipeline[pipeline]], startup::BRUSH);
                }
                if matches!(operation.kind, layer_core::RasterOperationKind::ApplyMask | layer_core::RasterOperationKind::Erase { alpha_locked: false }) {
                    let settles = matches!(operation.kind, layer_core::RasterOperationKind::ApplyMask)
                        || batch_pixel_rect(batch, packet.scene.target_extent(batch.target)) == PixelRect::full(packet.scene.target_extent(batch.target));
                    if settles && self.paint_layers.iter().any(|layer| layer.id == batch.target && layer.watercolor.is_some()) {
                        ready &= self.compilation.require([&self.pipelines.watercolor_compute.1], startup::BRUSH);
                    }
                }
                if let Some(texture) = scene::fill_texture(&operation.kind)
                    && let Some(binding) = &texture.paper
                    && self.prepare_paper_binding(binding, startup::BRUSH).is_ok() {
                    ready &= paper::PaperTile::id_for(binding).is_none_or(|id| self.papers.view(&id).is_some());
                }
                if batch.dab_count==0 {continue;}
            }
            ready &= self.brush_textures_ready(&batch.style);
            if !self.startup.as_ref().is_some_and(|startup|startup.strokes.covers(&batch.style)) {
                let dabs = packet.dabs.get(batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize).unwrap_or_default();
                let mask = layer_masks::MaskRenderer::is_mask(packet.scene, batch.target);
                let mut required = compile::Needs::default();
                required.style(self, &batch.style, dry_material::Reach::of_dabs(dabs), mask, batch.kind == DabBatchKind::Preview);
                ready &= required.require(&self.compilation, startup::BRUSH);
            }
        }
        ready
    }

    pub(crate) fn background_pipeline_ready(&self, pipeline: &deferred::Deferred<wgpu::ComputePipeline>) -> bool {
        self.compilation.require([pipeline],startup::VALIDATION)
    }

    fn validate_and_prepare_brush_resources(
        &mut self,
        batches: &[DabBatch],
    ) -> Result<(), GpuRasterError> {
        for batch in batches {
            self.ensure_texture_set(&batch.style)?;
        }
        Ok(())
    }

    fn register_tiled_sources(&mut self, scene: SceneView<'_>) {
        self.tiled_sources.retain(|target, _| scene.paint_base(*target).is_some() && source_access::placed_targets(scene).any(|t|t==*target));
        for &handle in scene.order() {
            let Some(source) = scene.paint_source(handle).and_then(|p| p.base.as_ref()) else { continue; };
            let target = scene.source_target(handle).unwrap();
            if self.tiled_sources.get(&target).is_none_or(|current| !current.image.same_owner(&source.image) || current.offset != source.offset || current.policy != source.policy) {
                self.tiled_sources.insert(target, source.clone());
            }
        }
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
            self.clear_preview_pages();
            self.scale_display = None;
            self.preview_damage = PixelRect::EMPTY;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_contribution = false;
        }
        self.update_target_geometry(&retained.iter().map(|(target, scene)| (*target, scene.target_extent(*target))).collect(), resized)?;
        self.register_tiled_sources(scene);
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
        let primary = resident::Resident::unwritten(self.create_page_surface(&self.raster_tiers.paint, label));
        LayerPage {
            derived:None,
            coordinate,
            primary,
            secondary: None,
            active_secondary: false,
            primary_needs_clear: true,
            stroke_start: None,
        }
    }

    fn create_page_surface(&self, tier: &Arc<residency::Tier>, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.float_texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            self.device.working_format(),
            label,
            tier,
        )
    }

    fn create_scalar_page_surface(&self, tier: &Arc<residency::Tier>, label: &'static str) -> PageSurface {
        create_page_surface(
            &self.device,
            &self.float_texture_layout,
            &self.sampler,
            [PAGE_SIZE, PAGE_SIZE],
            self.device.scalar_format(),
            label,
            tier,
        )
    }

    fn create_stroke_coverage_page(&self, coordinate: [u32; 2], slabs: u32, rgba: bool) -> StrokeCoveragePage {
        let extent = [PAGE_SIZE, PAGE_SIZE * slabs];
        let format = if rgba { wgpu::TextureFormat::Rgba32Float } else { self.device.scalar_format() };
        let [primary, secondary] = ["layer stroke coverage page A", "layer stroke coverage page B"].map(|label|
            resident::Resident::unwritten(create_page_surface(&self.device, &self.float_texture_layout, &self.sampler, extent, format, label, &self.raster_tiers.paint)));
        StrokeCoveragePage { coordinate, primary, secondary, active_secondary: false, owner: None,state:None }
    }

    fn stroke_finish_pages<'a>(&'a self, batch: &'a DabBatch) -> impl Iterator<Item = [u32; 2]> + 'a {
        self.paint_layers.iter()
            .filter(move |layer| batch.stroke_end && revisits_stroke(&batch.style) && layer.id == batch.target)
            .flat_map(|layer| &layer.coverage_pages)
            .filter(move |page| page.owner == Some(batch.stroke_id))
            .map(|page| page.coordinate)
    }

    fn preview_from_persistent(&self, batches: &[DabBatch]) -> bool {
        let mut previews = batches.iter().filter(|batch| !batch.target.is_coverage() && batch.kind == DabBatchKind::Preview && has_brush_work(batch));
        let Some(batch) = previews.next() else { return false; };
        self.fill_preview.is_none() && previews.next().is_none()
            && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
            && !BrushPassPlan::for_device(&batch.style, &self.device).state.watercolor_wetness
    }

    fn sparse_preview(batches: &[DabBatch]) -> bool {
        batches.iter().filter(|batch| !batch.target.is_coverage() && batch.kind == DabBatchKind::Preview && has_brush_work(batch))
            .all(|batch| (pointwise(&batch.style) || batch.style.execution == BrushExecution::Liquify)
                && !batch.style.rendering.edge_after_stroke)
    }

    fn tracks_stroke_start(batch: &DabBatch) -> bool {
        pointwise(&batch.style) && batch.style.opacity < 1. || stable_blender(&batch.style)
    }

    fn brush_work_planned(batch: &DabBatch, tiles: &[BrushTile], extent: [u32; 2]) -> bool {
        if batch.style.execution == BrushExecution::Liquify { !tiles.is_empty() }
        else { batch.dab_count != 0 && !batch_pixel_rect(batch, extent).is_empty() }
    }

    fn preview_pages(batches: &[DabBatch], batch: &DabBatch, tiles: &[BrushTile], extent: [u32; 2]) -> std::collections::BTreeSet<[u32; 2]> {
        if batch.kind != DabBatchKind::Preview || batch.target.is_coverage() || tiles.is_empty() { return Default::default(); }
        if Self::sparse_preview(batches) { tiles.iter().map(|tile| tile.coordinate).collect() }
        else { page_coordinates(Self::brush_display_damage(batch, tiles, extent)).collect() }
    }

    fn brush_display_damage(batch: &DabBatch, tiles: &[BrushTile], extent: [u32; 2]) -> PixelRect {
        let bounds=batch_pixel_rect(batch,extent);
        if batch.style.execution==BrushExecution::Liquify {
            tiles.iter().fold(PixelRect::EMPTY,|bounds,tile|bounds.union(page_rect(tile.coordinate))).intersect(PixelRect::full(extent))
        } else if batch.style.execution==BrushExecution::Watercolor {
            bounds.expand(WatercolorLayerStyle::from_dab_style(&batch.style).radius(),extent)
        } else if pointwise(&batch.style) && !batch.style.rendering.edge_after_stroke {
            tiles.iter().fold(PixelRect::EMPTY,|bounds,tile|bounds.union(page_rect(tile.coordinate))).intersect(bounds)
        } else {bounds}
    }

    fn clears_stroke_starts(&self, batch: &DabBatch, tiles: &[BrushTile], extent: [u32; 2]) -> bool {
        batch.kind == DabBatchKind::Persistent && batch.stroke_start && batch.style.execution != BrushExecution::Liquify
            && Self::brush_work_planned(batch, tiles, extent) && BrushPassPlan::for_device(&batch.style, &self.device).requires_destination()
    }

    fn destination_pages(
        &self,
        batches: &[DabBatch],
        tiles: &[Vec<BrushTile>],
    ) -> std::collections::BTreeSet<(SourceTarget, [u32; 2])> {
        let mut destination_pages = std::collections::BTreeSet::new();
        for (batch, tiles) in batches.iter().zip(tiles).filter(|(batch, _)| {
            batch.kind == DabBatchKind::Persistent
                && batch.style.execution != BrushExecution::Liquify
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

    fn stroke_start_at(&self,batch:&DabBatch,coordinate:[u32;2],intent:&raster::contents::Intent,reads_preview:bool,
        declaration:Option<&Arc<scene::PreviewDeclaration>>)->Option<(&StrokeStart,bool)> {
        self.paint_layers.iter().find(|layer|layer.id==batch.target).and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate))
            .and_then(|page|page.stroke_start.as_ref()).filter(|start|start.state.matches(batch.stroke_id,intent)).map(|start|(start,false))
            .or_else(||reads_preview.then(||self.preview_page(coordinate)).flatten().and_then(|page|page.stroke_start.as_ref())
                .filter(|start|start.matches(batch.stroke_id,intent,declaration)).map(|start|(start,true)))
    }

    fn material_offsets(&self, batch: &DabBatch) -> [Option<[i32; 2]>; 9] {
        let stored = self.in_place_dry_material(batch) || self.compact_preview_contribution(batch);
        let center = pointwise(&batch.style);
        std::array::from_fn(|i| (!stored && (!center || i == 4)).then_some([i as i32 % 3 - 1, i as i32 / 3 - 1]))
    }

    fn material_bind_group(
        &mut self,
        batch: &DabBatch,
        batch_index:usize,
        coordinate: [u32; 2],
        target: BrushEncodingTarget,
        intent: &raster::contents::Intent,
        inputs: MaterialInputs<'_>,
        display: Option<usize>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(wgpu::BindGroup, Option<wgpu::BindGroup>), GpuRasterError> {
        let result=(|| {
            let reads_preview = target.material_source().is_preview();
            let declares_preview = target.is_preview();
            let offsets = self.material_offsets(batch);
            let mut neighborhood=self.brush_inputs.take().ok_or(GpuRasterError::Invariant("brush inputs are not prepared"))?;
            #[cfg(test)] let _record=scene::sources::record_reads(&neighborhood);
            let source_preview=reads_preview.then_some((scene::PreviewStage::BeforeBatch(batch_index),None));
            let receipt=encoder.submission();
            let retained = stable_blender(&batch.style);
            let mut history=Vec::new();
            let declaration=declares_preview.then(||self.preview_declaration.clone().ok_or(GpuRasterError::Invariant("prepared material bind group state is inconsistent"))).transpose()?;
            let accepting=declaration.as_ref().filter(|declaration|declaration.accepts(coordinate));
            if Self::tracks_stroke_start(batch) {
                let committed_start = declaration.as_ref().is_some_and(|declaration|declaration.committed_seed(coordinate));
                let needed = if reads_preview { !committed_start && self.preview_pages.iter().find(|page|page.coordinate==coordinate)
                    .is_some_and(|page|page.stroke_start.as_ref().is_none_or(|start|!start.matches(batch.stroke_id,&intent,declaration.as_ref()))) }
                    else {self.paint_layers.iter().find(|layer|layer.id==batch.target).and_then(|layer|layer.pages.iter().find(|page|page.coordinate==coordinate))
                        .is_some_and(|page|page.stroke_start.as_ref().is_none_or(|start|!start.state.matches(batch.stroke_id,&intent)))};
                let start = if needed {
                    self.accepted_raster_output(batch.target,RasterOutput::StrokeStart(coordinate,batch.stroke_id,reads_preview))?;
                    Some(prepared_raster_output(self.take_prepared_stroke_start(batch.target, coordinate, batch.stroke_id, reads_preview))?)
                }
                    else { None };
                let preview_expected=reads_preview.then(||self.preview_expected(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::BeforeBatch(batch_index))).transpose()?;
                let start_context=reads_preview.then(||self.preview_ticket(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::BeforeBatch(batch_index),scene::PreviewPass::Color,intent)).transpose()?;
                let start_read=if needed {
                    let [Some(read),None]=neighborhood.brush_color(self,batch.target,coordinate,source_preview,&receipt)?
                        else {return Err(GpuRasterError::Invariant("stroke start input is not prepared"));};
                    let witness=neighborhood.witnesses(batch.target,RasterPlane::Color,coordinate).into_iter().next()
                        .ok_or(GpuRasterError::Invariant("stroke start input witness is not prepared"))?;
                    Some((read.tile,witness))
                }else {None};
                let page = if reads_preview {
                    (!committed_start).then(|| self.preview_pages.iter_mut().find(|p| p.coordinate == coordinate)).flatten()
                } else {
                    self.paint_layers.iter_mut().find(|l| l.id == batch.target)
                        .and_then(|l| l.pages.iter_mut().find(|p| p.coordinate == coordinate))
                };
                if let (Some(page),Some(mut start))=(page,start) {
                    let expected=preview_expected.as_ref().unwrap_or_else(||intent.version());
                    let (read,witness)=start_read.as_ref().ok_or(GpuRasterError::Invariant("stroke start input is not prepared"))?;
                    let write=start.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                    if let Some(declaration)=accepting {
                        declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::Seed,scene::PreviewPass::StrokeStart,vec![witness.clone()])?;
                        history.push(witness.clone());
                    }
                    write.value().copy_from(&read.texture,read.texture.size(),encoder);let version=expected.clone();write.publish(&version);
                    page.stroke_start=Some(StrokeStart {state:MaterialState {stroke:batch.stroke_id,input:intent.clone(),version},preview:start_context,surface:start});
                }
            }
            let selected=self.brush_layer_neighborhood(&mut neighborhood,batch.target,coordinate,offsets,source_preview,accepting.is_some().then_some(&mut history),encoder)?;
            let layer = self
                .paint_layers
                .iter()
                .find(|layer| layer.id == batch.target)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
            let mut selected_starts:[Option<wgpu::TextureView>;9]=std::array::from_fn(|_|None);
            let mut views:[&wgpu::TextureView;9]=std::array::from_fn(|index|selected[index].as_ref().unwrap_or(&self.empty_view));
            let current = views[4];
            if retained {
                for ((index,view),selected_start) in views.iter_mut().enumerate().zip(selected_starts.iter_mut()) {
                    let [x, y] = [coordinate[0] as i32 + index as i32 % 3 - 1,
                        coordinate[1] as i32 + index as i32 / 3 - 1];
                    if x < 0 || y < 0 { continue; }
                    let at = [x as u32, y as u32];
                    if let Some((start,predicted))=self.stroke_start_at(batch,at,&intent,reads_preview,declaration.as_ref()) {
                        let (tile,witness)=neighborhood.brush_state(batch.target,at,scene::sources::BrushState::Start(batch.stroke_id),predicted,&start.state.version,&receipt)?;
                        if let Some(declaration)=accepting {
                            declaration.accept_inputs(batch.target,RasterPlane::Color,at,scene::PreviewStage::Seed,scene::PreviewPass::StrokeStart,vec![witness.clone()])?;
                            history.push(witness);
                        }*view=selected_start.insert(tile.view);
                    }
                }
            }
            if let MaterialInputs::Retouch(pages) = inputs {
                for (slot, page) in [0, 1, 2, 3, 5, 6, 7, 8].into_iter().zip(pages) {
                    views[slot] = page;
                }
            }
            let predicted_coverage = if reads_preview {
                self.preview_coverage_pages
                    .iter()
                    .find(|p| p.matches(coordinate, &batch.style))
            } else {
                None
            }
            ;
            let coverage_page=predicted_coverage.or_else(|| {
                layer
                    .coverage_pages
                    .iter()
                    .find(|p| p.matches(coordinate, &batch.style) && p.owner == Some(batch.stroke_id))
            })
            .map(|page| {
                let state=page.state.as_ref().filter(|state|state.matches(batch.stroke_id,&intent)).ok_or(GpuRasterError::Invariant("prepared material bind group state is inconsistent"))?;
                let expected=if predicted_coverage.is_some() {self.preview_pass_expected(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::BeforeBatch(batch_index),scene::PreviewPass::Coverage)?}
                    else {state.version.clone()};
                let (tile,witness)=neighborhood.brush_state(batch.target,coordinate,scene::sources::BrushState::Coverage(stroke_coverage_slabs(&batch.style),retained),predicted_coverage.is_some(),&expected,&receipt)?;
                if accepting.is_some() {history.push(witness);}Ok(tile.view)
            }).transpose()?;
            let coverage = coverage_page.as_ref().map_or(if retained { &self.empty_view } else { &self.empty_scalar_view }, |view|view);
            let wetness=if BrushPassPlan::for_device(&batch.style,&self.device).state.watercolor_wetness {
                let stage=reads_preview.then_some((scene::PreviewStage::BeforeBatch(batch_index),None));
                let read=neighborhood.brush_wetness(self,batch.target,coordinate,stage,&receipt)?
                    .ok_or(GpuRasterError::Invariant("material wetness input is not prepared"))?;
                if accepting.is_some() {history.extend(neighborhood.witnesses(batch.target,RasterPlane::WatercolorWetness,coordinate));}
                Some(read.tile.view)
            }else {None};
            let mut auxiliary_state=None;
            let auxiliary = if let Some(wetness)=&wetness {wetness
            } else if retained {
                current
            } else if pointwise(&batch.style) {
                if batch.style.opacity<1. {
                    if let Some((start,predicted))=self.stroke_start_at(batch,coordinate,&intent,reads_preview,declaration.as_ref()) {
                        let (tile,witness)=neighborhood.brush_state(batch.target,coordinate,scene::sources::BrushState::Start(batch.stroke_id),predicted,&start.state.version,&receipt)?;
                        if let Some(declaration)=accepting {
                            declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::Seed,scene::PreviewPass::StrokeStart,vec![witness.clone()])?;
                            history.push(witness);
                        }
                        auxiliary_state.insert(tile.view)
                    }else {views[4]}
                }else {&self.empty_view}
            } else if batch.style.execution==BrushExecution::Wet {
                if batch.stroke_start {
                    if let Some(declaration)=accepting {declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::Seed,scene::PreviewPass::Reservoir,Vec::new())?;}
                    &self.empty_view
                }else {
                    let state=self.reservoir.state.as_ref().filter(|state|state.matches(batch.stroke_id,&intent)).ok_or(GpuRasterError::Invariant("prepared material bind group state is inconsistent"))?;
                    let (tile,witness)=neighborhood.brush_state(batch.target,coordinate,scene::sources::BrushState::Reservoir,false,&state.version,&receipt)?;
                    if let Some(declaration)=accepting {
                        declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,scene::PreviewStage::Seed,scene::PreviewPass::Reservoir,vec![witness.clone()])?;
                        history.push(witness);
                    }auxiliary_state.insert(tile.view)
                }
            }else {&self.empty_view};
            if let Some(declaration)=accepting {
                let stage=scene::PreviewStage::AfterBatch(batch_index);
                declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,stage,scene::PreviewPass::Color,history.clone())?;
                if BrushPassPlan::for_device(&batch.style,&self.device).state.coverage {
                    declaration.accept_inputs(batch.target,RasterPlane::Color,coordinate,stage,scene::PreviewPass::Coverage,history.clone())?;
                }
                if BrushPassPlan::for_device(&batch.style,&self.device).state.watercolor_wetness {
                    declaration.accept_inputs(batch.target,RasterPlane::WatercolorWetness,coordinate,stage,scene::PreviewPass::Wetness,history)?;
                }
            }
            let display = display.map(|slot| self.dry_display_inputs[slot].inputs(self, views));
            let records=self.material_records.binding();let (offset,size)=(records.offset,records.size);
            let query:([&wgpu::Buffer;2],[&wgpu::TextureView;11])=([&self.dab_buffer,records.buffer],std::array::from_fn(|i|match i {9=>coverage,10=>auxiliary,_=>views[i]}));
            let (device,layout)=(&self.device,&self.material_layout);
            let result=Ok((self.material_bindings.get_by(&query as &dyn scene::ResourceBinding<2,11>,||(query.0.map(Clone::clone),query.1.map(Clone::clone)),|key| {
                let views:[&wgpu::TextureView;9]=std::array::from_fn(|i|&key.1[i]);
                create_material_bind_group(device,layout,&views,&key.0[0],&key.1[9],&key.1[10],
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&key.0[1],offset,size}))
            }).clone(),display));
            self.brush_inputs=Some(neighborhood);result
        })();
        result.map_err(prepared_brush_error)
    }

    fn refresh_storage_metrics(&mut self) {
        let paint_pages = self
            .paint_layers
            .iter()
            .map(|layer| layer.pages.len() as u64)
            .sum::<u64>();
        let preview_pages = self.preview_pages.len() as u64;
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
        self.metrics.paint_pages = paint_pages;
        self.metrics.preview_pages = preview_pages;
        self.metrics.destination_companion_pages = destination_companion_pages;
        self.metrics.coverage_pages = coverage_pages;
        self.metrics.material_pages = material_pages;
    }

    fn ensure_upload_capacity(&mut self, dabs: usize, styles: usize) -> Result<(), GpuRasterError> {
        let capacity=upload_capacity::plan(&self.device.limits(),self.dab_capacity_bytes,self.style_capacity,self.style_stride,dabs,styles)?;
        if self.style_upload.capacity()<capacity.style_bytes {
            self.style_upload.try_reserve_exact(capacity.style_bytes.saturating_sub(self.style_upload.len()))
                .map_err(|_|GpuRasterError::UploadAllocationFailed)?;
        }
        if dabs>self.dab_upload.capacity() {
            let maximum=usize::try_from(capacity.dab_bytes/mem::size_of::<DabGpu>() as u64)
                .map_err(|_|GpuRasterError::SizeOverflow)?;
            let records=dabs.checked_next_power_of_two().unwrap_or(maximum).min(maximum);
            self.dab_upload.try_reserve_exact(records.saturating_sub(self.dab_upload.len()))
                .map_err(|_|GpuRasterError::UploadAllocationFailed)?;
        }
        if capacity.dab_bytes!=self.dab_capacity_bytes {
            self.dab_buffer=self.device.create_buffer(&wgpu::BufferDescriptor {
                label:Some("layer dab instances"),size:capacity.dab_bytes,
                usage:wgpu::BufferUsages::VERTEX|wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,
                mapped_at_creation:false,
            });
            self.dab_capacity_bytes=capacity.dab_bytes;
        }
        if capacity.style_records!=self.style_capacity {
            self.style_upload_valid.store(false,std::sync::atomic::Ordering::Release);
            self.style_buffer=create_style_buffer(&self.device,self.style_stride,capacity.style_records);
            self.style_bind_group=create_style_bind_group(&self.device,&self.style_layout,&self.style_buffer);
            self.style_capacity=capacity.style_records;
        }
        Ok(())
    }

    fn reorders_dabs(&self,batch:&DabBatch)->bool {
        batch.dab_count!=0 && (pointwise(&batch.style)||stable_blender(&batch.style))
            && BrushPassPlan::for_device(&batch.style,&self.device).direct.is_none()
    }
    fn prepare_uploads(
        &mut self,
        packet: FramePacket<'_>,
        batch_tiles: &mut [Vec<BrushTile>],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if batch_tiles.len()!=packet.dab_batches.len() {return Err(GpuRasterError::InvalidDabRange);}
        let removed_members: Vec<_> = source_access::pending_bakes(packet.scene)
            .flat_map(|(snapshot, scope, _)| snapshot.with_scope(scope).order().iter().copied().filter(|h| packet.scene.occurrence(*h).is_none()).map(move |h| (h, snapshot)))
            .collect();
        let records=packet.dab_batches.len().checked_add(packet.scene.order().len())
            .and_then(|count|count.checked_add(removed_members.len())).ok_or(GpuRasterError::SizeOverflow)?;
        let mut dabs=packet.dabs.len();
        for (batch,tiles) in packet.dab_batches.iter().zip(batch_tiles.iter()) {
            let end=batch.first_dab.checked_add(batch.dab_count).ok_or(GpuRasterError::InvalidDabRange)?;
            if packet.dabs.get(batch.first_dab as usize..end as usize).is_none() {return Err(GpuRasterError::InvalidDabRange);}
            if !self.reorders_dabs(batch) {continue;}
            for tile in tiles {
                if !tile.indices.is_empty() && tile.indices.len()!=tile.dabs.len() {
                    if tile.indices.iter().any(|&index|index as usize>=packet.dabs.len()) {return Err(GpuRasterError::InvalidDabRange);}
                    dabs=dabs.checked_add(tile.indices.len()).ok_or(GpuRasterError::SizeOverflow)?;
                }
            }
        }
        u32::try_from(dabs).map_err(|_|GpuRasterError::SizeOverflow)?;
        self.ensure_upload_capacity(dabs,records)?;
        self.material_bindings.begin_frame();
        self.transport_bindings.begin_frame();
        self.material_records.restart();
        self.transport_records.restart();
        self.dab_upload.clear();
        self.dab_upload.extend(packet.dabs.iter().copied().map(DabGpu::from));
        for batch in packet.dab_batches {
            let range = batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize;
            dry_material::prepare_geometry(&batch.style, &mut self.dab_upload[range.clone()]);
            dry_material::prepare_colors(&batch.style, self.device.working_space(), &mut self.dab_upload[range.clone()]);
        }
        for (batch, tiles) in packet.dab_batches.iter().zip(batch_tiles) {
            if !self.reorders_dabs(batch) {continue;}
            for tile in tiles {
                if tile.indices.is_empty() || tile.indices.len() == tile.dabs.len() { continue; }
                let start = u32::try_from(self.dab_upload.len()).map_err(|_| GpuRasterError::SizeOverflow)?;
                for &index in &tile.indices {
                    let dab=*self.dab_upload.get(index as usize).ok_or(GpuRasterError::InvalidDabRange)?;
                    self.dab_upload.push(dab);
                }
                let end = u32::try_from(self.dab_upload.len()).map_err(|_| GpuRasterError::SizeOverflow)?;
                tile.dabs = start..end;
            }
        }
        if !packet.dabs.is_empty() {
            self.uploads.write(
                encoder,
                &self.dab_buffer,
                dab_bytes(&self.dab_upload),
            )?;
        }
        let used = self.style_stride as usize * records;
        let mut style_changed = self.style_upload.len() != used
            || !self.style_upload_valid.load(std::sync::atomic::Ordering::Acquire);
        self.style_upload.resize(used, 0);
        for index in 0..packet.dab_batches.len() {
            let batch = &packet.dab_batches[index];
            let block = self.dry_material_block(batch);
            let mut record = StyleGpu::brush(self.target_extent(batch.target), batch, block, &self.device, self.paper_style(&batch.style));
            if self.compact_preview_contribution(batch) {
                record.flags[3] = 2.;
            }
            if dry_material::single_pigment_marked(&batch.style) && batch.dab_count>0 {
                let source=&packet.dabs[batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize];
                record.material_a[3]=f32::from(batch.style.mode==DabMode::Erase
                    || source.iter().all(|d|d.color_rgba_linear[..3]==source[0].color_rgba_linear[..3]));
                record.material_b[..3].copy_from_slice(&source[0].color_rgba_linear[..3]);
            }
            record.color_mode = layer_color_parameters(packet.scene.color_mode(batch.target), self.document_color());
            record.color_mode[3] = if ordered_dither(&batch.style) { 1. } else if stable_blender(&batch.style) { 2. } else { 0. };
            record.color_mode[2] = f32::from(batch.style.blend_space == layer_core::BlendSpace::Perceptual);
            let offset = index * self.style_stride as usize;
            let bytes = style_bytes(&record);
            let destination = &mut self.style_upload[offset..offset + bytes.len()];
            style_changed |= destination != bytes;
            destination.copy_from_slice(bytes);
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
            let bytes = style_bytes(&record);
            let destination = &mut self.style_upload[offset..offset + bytes.len()];
            style_changed |= destination != bytes;
            destination.copy_from_slice(bytes);
        }
        if style_changed && !self.style_upload.is_empty() {
            let write = submission::CacheWrite::shared(self.style_upload_valid.clone());
            self.uploads.write(encoder, &self.style_buffer, &self.style_upload)?;
            self.style_upload_valid.store(true, std::sync::atomic::Ordering::Release);
            write.track(encoder);
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

    pub(crate) fn preview_version(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:scene::PreviewStage)
        ->Result<scene::PreviewVersion,GpuRasterError> {
        let key=(target,raster::contents::index(plane),coordinate,stage);
        if let Some(name)=self.preview_names.borrow().as_ref().and_then(|names|names.get(&key)) {return Ok(name.clone());}
        #[cfg(test)] crate::test_support::record_preview_name(key);
        let input=self.raster_intent(target,plane,coordinate)?;
        let name=self.preview_declaration.as_ref().ok_or(GpuRasterError::NotReady)?.ticket(target,plane,coordinate,stage,input)?;
        if let Some(names)=self.preview_names.borrow_mut().as_mut() {names.insert(key,name.clone());}
        Ok(name)
    }
    pub(crate) fn preview_pass_expected(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:scene::PreviewStage,pass:scene::PreviewPass)
        ->Result<layer_core::raster::ContentVersion,GpuRasterError> {
        self.preview_declaration.as_ref().ok_or(GpuRasterError::NotReady)?.version_for(target,plane,coordinate,stage,pass)
    }

    fn preview_ticket(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:scene::PreviewStage,pass:scene::PreviewPass,input:&raster::contents::Intent)
        ->Result<scene::PreviewVersion,GpuRasterError> {
        let declaration=self.preview_declaration.as_ref().ok_or(GpuRasterError::NotReady)?;
        if pass==scene::PreviewPass::Color || pass==scene::PreviewPass::Wetness {declaration.ticket(target,plane,coordinate,stage,input.clone())}
        else {declaration.ticket_for_accepted(target,plane,coordinate,stage,pass,input.clone())}
    }
    fn preview_material_version(&self,batch:&DabBatch,batch_index:usize,plane:RasterPlane,coordinate:[u32;2],pass:scene::PreviewPass,input:&raster::contents::Intent)
        ->Result<scene::PreviewVersion,GpuRasterError> {
        let ticket=self.preview_ticket(batch.target,plane,coordinate,scene::PreviewStage::AfterBatch(batch_index),pass,input)?;
        let transport=batch.style.execution==BrushExecution::Watercolor && batch.style.transport.as_ref()
            .is_some_and(|transport|transport.distance>=1. && (transport.wet_flow>0. || transport.dry_flow>0.));
        Ok(if transport && (pass==scene::PreviewPass::Color || pass==scene::PreviewPass::Wetness) {ticket.intermediate(pass,0)}else {ticket})
    }

    fn preview_seed_versions(&self,declaration:&scene::PreviewDeclaration,plane:RasterPlane,coordinate:[u32;2],pass:scene::PreviewPass,from_persistent:bool)
        ->Option<(Vec<layer_core::raster::ContentVersion>,bool)> {
        let target=declaration.target();let batch=declaration.seed_batch(plane,coordinate,pass)?;
        let input=self.raster_intent(target,RasterPlane::Color,coordinate).ok()?;
        let layer=self.paint_layers.iter().find(|layer|layer.id==target)?;
        match pass {
            scene::PreviewPass::Coverage=>match layer.coverage_pages.iter().find(|page|page.matches(coordinate,&batch.style) && page.owner==Some(batch.stroke_id)) {
                Some(page)=>page.state.as_ref().filter(|state|state.matches(batch.stroke_id,&input)).map(|state|(vec![state.version.clone()],false)),
                None=>Some((Vec::new(),false)),
            },
            scene::PreviewPass::Reservoir if batch.stroke_start=>Some((Vec::new(),false)),
            scene::PreviewPass::Reservoir=>self.reservoir.state.as_ref().filter(|state|state.matches(batch.stroke_id,&input)).map(|state|(vec![state.version.clone()],false)),
            scene::PreviewPass::StrokeStart=> {
                if let Some(start)=layer.pages.iter().find(|page|page.coordinate==coordinate).and_then(|page|page.stroke_start.as_ref())
                    .filter(|start|start.state.matches(batch.stroke_id,&input)) {Some((vec![start.state.version.clone()],true))}
                else if from_persistent {Some((vec![input.version().clone()],false))}
                else {declaration.version(target,RasterPlane::Color,coordinate,declaration.seed_stage(plane,coordinate,pass)?).ok().map(|version|(vec![version],false))}
            },
            _=>None,
        }
    }

    fn preview_material_plans(&self,packet:FramePacket<'_>,plans:&[Vec<BrushTile>],from_persistent:bool)->Result<Vec<Vec<BrushTile>>,GpuRasterError> {
        packet.dab_batches.iter().zip(plans).map(|(batch,tiles)| {
            if batch.kind==DabBatchKind::Preview && !batch.target.is_coverage() && BrushPassPlan::for_device(&batch.style,&self.device).requires_destination() {
                self.accepted_material_tiles(batch,BrushEncodingContext {source:packet.scene,batches:packet.dab_batches,dabs:packet.dabs,tiles,target_extent:self.target_extent(batch.target),
                    target:BrushEncodingTarget::Preview {from_persistent}}).map(|tiles|tiles.into_owned())
            }else {Ok(tiles.clone())}
        }).collect()
    }

    fn prepare_preview_declaration(&mut self,packet:FramePacket<'_>,quality:scene::PreviewQuality,plans:&[Vec<BrushTile>],from_persistent:bool,
        stamps:Option<Arc<scene::PreviewStamps>>,consumer:&submission::Submission)->Result<bool,GpuRasterError> {
        let mut inputs=IdMap::default();
        for batch in packet.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview && batch.style.execution.retouches()) {
            if let Some(input)=self.retouch_preview_input(batch.target,batch.stroke_id)? {inputs.insert(batch.target,input);}
        }
        for batch in packet.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview && batch.style.execution==BrushExecution::Liquify) {
            if let Some(input)=self.liquify.as_ref().and_then(|liquify|liquify.captured_input(batch.target,batch.stroke_id,consumer)) {inputs.insert(batch.target,input);}
        }
        let declaration=scene::PreviewDeclaration::capture_with_plans(self,packet,quality,inputs,self.fill_preview.clone(),plans,stamps.clone())?;
        if stamps.is_none() && let (Some(held),Some(candidate))=(&self.preview_declaration,&declaration)
            && !held.in_contact() && held.matches_metadata(candidate)?
            && held.matches_history(|plane,coordinate,pass|self.preview_seed_versions(held,plane,coordinate,pass,from_persistent)
                .map(|(versions,_)|scene::PreviewDeclaration::captured_version(pass,&versions))) {
            return Ok(true);
        }
        self.preview_declaration=declaration.map(|declaration| {
            let seeds=declaration.required_seed_inputs().into_iter().filter(|(_,coordinate,pass)|*pass==scene::PreviewPass::StrokeStart || !declaration.accepted(*coordinate)).filter_map(|(plane,coordinate,pass)|
                self.preview_seed_versions(&declaration,plane,coordinate,pass,from_persistent).map(|(versions,committed)|scene::PreviewSeed {plane,coordinate,pass,versions,committed})).collect();
            declaration.with_seed_inputs(seeds).map(Arc::new)
        }).transpose()?;
        Ok(false)
    }

    pub(crate) fn preview_expected(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],stage:scene::PreviewStage)
        ->Result<layer_core::raster::ContentVersion,GpuRasterError> {
        self.preview_declaration.as_ref().ok_or(GpuRasterError::NotReady)?.version(target,plane,coordinate,stage)
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
        let _pass = encoder.color_pass(
            label,
            view,
            wgpu::LoadOp::Clear(wgpu::Color { r: value as f64, g: value as f64, b: value as f64, a: value as f64 }),
        );
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
        let direct = plan.direct.expect("destination brushes use the material encoder");
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
        if !Self::brush_work_planned(batch, context.tiles, context.target_extent) {
            return Ok(());
        }
        self.prepare_target_selection(encoder, &batch.style, self.target_extent(batch.target))?;
        if batch.style.execution == BrushExecution::Liquify {
            let mut liquify = self.liquify.take().unwrap_or_else(|| Box::new(liquify::Liquify::new(self)));
            let result = liquify.encode(self, encoder, batch_index, batch, context);
            self.liquify = Some(liquify);
            return result;
        }

        let plan = BrushPassPlan::for_device(&batch.style, &self.device);
        if plan.requires_destination() {
            let preview = context.target.is_preview();
            let watercolor_last = plan.state.watercolor_wetness
                && is_last_watercolor_update_batch(context.batches, batch_index);
            self.material_reads=(!pointwise(&batch.style)).then(IdMap::default);
            let encoded=self.encode_material_batch(encoder, batch_index, batch, context);
            self.material_reads=None;
            encoded?;
            if watercolor_last {
                let damages = watercolor_update_damages(context.batches, batch_index, context.target_extent);
                self.finish_watercolor_wetness_update(batch.target, preview)?;
                self.encode_watercolor_transport(encoder, batch_index, batch, &damages, preview)?;
            }
            return Ok(());
        }

        let preview=context.target.is_preview();
        for tile in context.tiles {
            self.accepted_raster_output(batch.target,if preview {RasterOutput::PreviewColor(tile.coordinate)}else {RasterOutput::Color(tile.coordinate)})?;
            let receipt=encoder.submission();
            let intent=self.raster_intent(batch.target,RasterPlane::Color,tile.coordinate)?;
            let expected=if preview {
                let page=prepared_raster_output(self.preview_pages.iter().find(|page|page.coordinate==tile.coordinate))?;
                Some(self.preview_expected(batch.target,RasterPlane::Color,page.coordinate,scene::PreviewStage::BeforeBatch(batch_index))?)
            }else {None};
            let ticket=preview.then(||self.preview_version(batch.target,RasterPlane::Color,tile.coordinate,scene::PreviewStage::AfterBatch(batch_index))).transpose()?;
            let pages=if preview {&mut self.preview_pages}else {&mut self.paint_layers.iter_mut().find(|layer|layer.id==batch.target)
                .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?.pages};
            let page=prepared_raster_output(pages.iter_mut().find(|page|page.coordinate==tile.coordinate))?;
            let write=if let Some(expected)=&expected {page.active_mut().revise(expected,&receipt)}else {page.active_mut().write(&intent,&receipt)}
                .ok_or(GpuRasterError::Invariant("prepared encode brush batch state is inconsistent"))?;
            let target=write.value().view.clone();
            let output=write.encoded(ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()));
            self.encode_batch_to_target(encoder,batch_index,batch,&target,tile.local,self.paint_target_binding(&batch.style),
                self.layer_target_offset(batch.target,tile.coordinate))?;
            let actual=encoder.submission();let written=output.publish(&actual);
            if preview {
                self.preview_pages.iter_mut().find(|page|page.coordinate==tile.coordinate).unwrap().derived=
                    ticket;
            }else {self.record_raster_write(batch.target,written)?;}
        }
        Ok(())
    }

    fn finish_watercolor_wetness_update(
        &mut self,
        layer_id: SourceTarget,
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
        let mut written=Vec::new();
        for coordinate in self.watercolor_written.drain(..) {
            let page = pages
                .iter_mut()
                .find(|page| page.coordinate == coordinate)
                .expect("watercolor update page remains live at commit");
            let (_state,output,ticket)=page.pending.take().ok_or(GpuRasterError::Invariant("watercolor publication is missing"))?;
            page.active_secondary = !page.active_secondary;
            if preview {page.derived=ticket;}else {written.push(output);}
        }
        for output in written {self.record_raster_write(layer_id,output)?;}
        Ok(())
    }

    fn transport_reads(
        &mut self,
        layer_id: SourceTarget,
        tiles: &[[u32; 2]],
        preview: bool,
        batch_index:usize,
        step:u32,
        encoder:&mut submission::CommandEncoder,
    ) -> Result<IdMap<[u32;2],[wgpu::TextureView;2]>, GpuRasterError> {
        let result=(|| {
            let receipt=encoder.submission();
            let mut inputs=self.brush_inputs.take().ok_or(GpuRasterError::Invariant("brush inputs are not prepared"))?;
            #[cfg(test)] let _record=scene::sources::record_reads(&inputs);
            let stage=|pass|preview.then_some((scene::PreviewStage::AfterBatch(batch_index),Some((if step==0 {pass}else {scene::PreviewPass::Transport},step.saturating_sub(1)))));
            let colors=self.brush_layer_tiles(&mut inputs,layer_id,tiles,stage(scene::PreviewPass::Color),encoder)?;
            let mut reads=IdMap::with_capacity_and_hasher(tiles.len(),Default::default());
            for (at,color) in tiles.iter().zip(colors) {
                #[cfg(test)] for plane in [RasterPlane::Color,RasterPlane::WatercolorWetness] {crate::test_support::record_transport_read((raster::contents::index(plane),*at,batch_index,step));}
                let wetness=inputs.brush_wetness(self,layer_id,*at,stage(scene::PreviewPass::Wetness),&receipt)?;
                reads.insert(*at,[color.unwrap_or_else(||self.empty_view.clone()),wetness.map_or_else(||self.empty_scalar_view.clone(),|read|read.tile.view)]);
            }
            self.brush_inputs=Some(inputs);Ok(reads)
        })();
        result.map_err(prepared_brush_error)
    }

    fn transport_bind_group(&mut self,reads:&IdMap<[u32;2],[wgpu::TextureView;2]>,coordinate:[u32;2],color_output:&wgpu::TextureView,wetness_output:&wgpu::TextureView)->wgpu::BindGroup {
        let views=TRANSPORT_OFFSETS.map(|offset|source_access::neighborhood(coordinate,[Some(offset)]).next().and_then(|at|reads.get(&at)));
        let colors=views.map(|read|read.map_or(&self.empty_view,|[color,_]|color));
        let wetness=views.map(|read|read.map_or(&self.empty_scalar_view,|[_,wetness]|wetness));
        let records=self.transport_records.binding();
        let query:([&wgpu::Buffer;1],[&wgpu::TextureView;12])=([records.buffer],std::array::from_fn(|index|match index {0..5=>colors[index],5..10=>wetness[index-5],10=>color_output,_=>wetness_output}));
        let (device,layout,size)=(&self.device,&self.transport_layout,records.size);
        self.transport_bindings.get_by(&query as &dyn scene::ResourceBinding<1,12>,||(query.0.map(Clone::clone),query.1.map(Clone::clone)),|key| {
            #[cfg(test)] test_support::PassCounts {transport_bind_groups:1,..Default::default()}.record();
            bindings::group(device,"layer watercolor transport sources",layout,key.1.iter().map(wgpu::BindingResource::TextureView)
                .chain([wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer:&key.0[0],offset:0,size})]))
        }).clone()
    }

    fn encode_watercolor_transport_jobs(
        &self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        step: u32,
        texture: &wgpu::BindGroup,
        jobs: &[WatercolorTransportJob],
    ) {
        if jobs.is_empty() { return; }
        let pass = encoder.compute_pass();
        pass.set_pipeline(&self.pipelines.watercolor_transport[step as usize]);
        pass.set_bind_group(0, &self.style_bind_group, &[batch_index as u32 * self.style_stride as u32]);
        pass.set_bind_group(3, texture, &[]);
        for job in jobs {
            pass.set_bind_group(1, self.paint_target_binding(&batch.style), &[self.layer_target_offset(batch.target, job.coordinate)]);
            pass.set_bind_group(2, &job.sources, &[job.record_offset]);
            let size = if step == 0 { [PAGE_SIZE; 2] } else { [job.local.width(), job.local.height()] };
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
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
        let neighbourhood:Vec<_>=updated.iter().flat_map(|coordinate|source_access::neighborhood(*coordinate,TRANSPORT_OFFSETS.map(Some)))
            .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        for coordinate in &updated {
            for output in if preview { [RasterOutput::PreviewColor(*coordinate),RasterOutput::PreviewCompanion(*coordinate),RasterOutput::PreviewWetness(*coordinate)] }
                else { [RasterOutput::Color(*coordinate),RasterOutput::Companion(*coordinate),RasterOutput::Wetness(*coordinate)] } {
                self.accepted_raster_output(batch.target,output)?;
            }
        }
        let mut regions = mem::take(&mut self.transport_regions);
        let mut pending = mem::take(&mut self.transport_jobs);
        regions.clear();
        pending.clear();
        *self.preview_names.get_mut() = preview.then(IdMap::default);
        let result = (|| {
        regions.extend(updated.iter().map(|coordinate| damages.iter().fold(PixelRect::EMPTY, |combined, damage| {
            combined.union(damage.intersect(page_rect(*coordinate)))
        }).page_local(*coordinate)));
        let first = self.transport_records.upload(&self.device, &mut self.uploads, encoder,
            regions.iter().map(|local| material_sources::record(material_sources::rect_words(*local), [])))?;
        let texture_key = Self::texture_set_key(&batch.style);
        let transport_texture_bind_group = self
            .texture_sets
            .iter()
            .find(|set| set.key == texture_key)
            .expect("transport texture is prepared before encoding")
            .transport_bind_group
            .clone();

        // The first stage writes whole pages; later stages overwrite the same
        // region, leaving identical pixels outside.
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
                        Job {
                            coordinate: *coordinate,
                            color_destination_secondary: !color.active_secondary,
                            wetness_destination_secondary: !wetness.active_secondary,
                        }
                    })
                    .collect::<Vec<_>>()
            };

            let mut publications=Vec::with_capacity(jobs.len()*2);
            let mut written=Vec::with_capacity(jobs.len()*2);
            let reads=self.transport_reads(batch.target,&neighbourhood,preview,batch_index,step,encoder)?;
            for (index, job) in jobs.iter().enumerate() {
                let local = regions[index];
                let receipt=encoder.submission();
                let color_intent=self.raster_intent(batch.target,RasterPlane::Color,job.coordinate)?;
                let wetness_intent=self.raster_intent(batch.target,RasterPlane::WatercolorWetness,job.coordinate)?;
                let color_ticket=preview.then(||self.preview_version(batch.target,RasterPlane::Color,job.coordinate,scene::PreviewStage::AfterBatch(batch_index))
                    .map(|ticket|if step+1==WATERCOLOR_TRANSPORT_STEPS {ticket}else {ticket.intermediate(scene::PreviewPass::Transport,step)})).transpose()?;
                let wetness_ticket=preview.then(||self.preview_version(batch.target,RasterPlane::WatercolorWetness,job.coordinate,scene::PreviewStage::AfterBatch(batch_index))
                    .map(|ticket|if step+1==WATERCOLOR_TRANSPORT_STEPS {ticket}else {ticket.intermediate(scene::PreviewPass::Transport,step)})).transpose()?;
                let (colors,wetness)=if preview {(&mut self.preview_pages,&mut self.preview_watercolor_wetness_pages)}else {
                    let layer=self.paint_layers.iter_mut().find(|layer|layer.id==batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
                    (&mut layer.pages,&mut layer.watercolor_wetness_pages)
                };
                let color=colors.iter_mut().find(|page|page.coordinate==job.coordinate).ok_or(GpuRasterError::Invariant("prepared encode watercolor transport state is inconsistent"))?;
                let wetness=wetness.iter_mut().find(|page|page.coordinate==job.coordinate).ok_or(GpuRasterError::Invariant("prepared encode watercolor transport state is inconsistent"))?;
                let color_write=if preview {color.surface_mut(job.color_destination_secondary).overwrite(&receipt)}
                    else {color.write(job.color_destination_secondary,&color_intent,&receipt)}.ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                let wetness_write=if preview {wetness.surface_mut(job.wetness_destination_secondary).overwrite(&receipt)}
                    else {wetness.write(job.wetness_destination_secondary,&wetness_intent,&receipt)}.ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                let color_view=color_write.value().view.clone();let wetness_view=wetness_write.value().view.clone();
                let color_encoding=color_write.encoded(color_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()));
                let wetness_encoding=wetness_write.encoded(wetness_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()));
                let sources=self.transport_bind_group(&reads,job.coordinate,&color_view,&wetness_view);
                publications.extend([
                    MaterialPublication {kind:MaterialOutputKind::Color,coordinate:job.coordinate,preview:color_ticket,input:color_intent,encoding:color_encoding},
                    MaterialPublication {kind:MaterialOutputKind::Wetness,coordinate:job.coordinate,preview:wetness_ticket,input:wetness_intent,encoding:wetness_encoding},
                ]);
                pending.push(WatercolorTransportJob {
                    coordinate: job.coordinate, local, record_offset: self.transport_records.offset(first + index), sources,
                });

            }
            self.encode_watercolor_transport_jobs(encoder, batch_index, batch, step, &transport_texture_bind_group, &pending);
                    let actual=encoder.submission();written.extend(publications.drain(..).map(|publication:MaterialPublication|publication.encoded(&actual)));
            pending.clear();

            for output in written {
                if let Some(ticket)=output.preview {
                    match output.kind {
                        MaterialOutputKind::Color=>self.preview_pages.iter_mut().find(|page|page.coordinate==output.coordinate).unwrap().derived=Some(ticket),
                        MaterialOutputKind::Wetness=>self.preview_watercolor_wetness_pages.iter_mut().find(|page|page.coordinate==output.coordinate).unwrap().derived=Some(ticket),
                        _=>unreachable!(),
                    }
                }else {self.record_raster_write(batch.target,output.written)?;}
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
        })();
        pending.clear();
        regions.clear();
        *self.preview_names.get_mut() = None;
        self.transport_jobs = pending;
        self.transport_regions = regions;
        result
    }

    fn encode_stroke_edge(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
    ) -> Result<(), GpuRasterError> {
        self.prepare_target_selection(encoder, &batch.style, self.target_extent(batch.target))?;
        struct Job {
            coordinate:[u32;2], destination_secondary:bool,
            intent:raster::contents::Intent, bind_group:wgpu::BindGroup,
        }
        let receipt=encoder.submission();
        let layer_index=self.paint_layers.iter().position(|layer|layer.id==batch.target)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
        let layer=&self.paint_layers[layer_index];
        let mut jobs=Vec::new();
        for coverage in layer.coverage_pages.iter().filter(|page|page.owner==Some(batch.stroke_id)) {
            let coordinate=coverage.coordinate;
            self.accepted_raster_output(batch.target,RasterOutput::Color(coordinate))?;
            self.accepted_raster_output(batch.target,RasterOutput::Companion(coordinate))?;
            let page=prepared_raster_output(layer.pages.iter().find(|page|page.coordinate==coordinate))?;
            let intent=self.raster_intent(batch.target,RasterPlane::Color,coordinate)?;
            let color=page.active().read(intent.version(),Some(&receipt)).ok_or(GpuRasterError::Invariant("accepted input is not readable"))?;
            let mut coverage_views=Vec::with_capacity(9);
            for y in -1_i32..=1 {for x in -1_i32..=1 {
                let [x,y]=[coordinate[0] as i32+x,coordinate[1] as i32+y];
                let view=if x<0 || y<0 {&self.empty_scalar_view}else {
                    layer.coverage_pages.iter().find(|page|page.coordinate==[x as u32,y as u32] && page.owner==Some(batch.stroke_id))
                        .map(|page| {
                            let current=self.raster_intent(batch.target,RasterPlane::Color,page.coordinate)?;
                            let state=page.state.as_ref().filter(|state|state.matches(batch.stroke_id,&current)).ok_or(GpuRasterError::Invariant("prepared encode stroke edge state is inconsistent"))?;
                            page.active().read(&state.version,Some(&receipt)).map(|read|&read.value().view).ok_or(GpuRasterError::Invariant("accepted input is not readable"))
                        }).transpose()?.unwrap_or(&self.empty_scalar_view)
                };
                coverage_views.push(view);
            }}
            jobs.push(Job {coordinate,destination_secondary:!page.active_secondary,intent,
                bind_group:views_group(&self.device,"layer post-stroke edge sources",&self.edge_layout,
                    std::iter::once(&color.value().view).chain(coverage_views.iter().copied()))});
        }
        for job in &jobs {
            let page=self.paint_layers[layer_index].pages.iter_mut().find(|page|page.coordinate==job.coordinate).unwrap();
            let write=page.write(job.destination_secondary,&job.intent,&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let view=write.value().view.clone();let output=write.encoded(layer_core::raster::ContentName::fresh());
            let mut pass=encoder.color_pass("layer post-stroke edge page",&view,wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            pass.set_pipeline(&self.pipelines.stroke_edge);
            pass.set_bind_group(0,&self.style_bind_group,&[batch_index as u32*self.style_stride as u32]);
            pass.set_bind_group(1,self.paint_target_binding(&batch.style),&[self.layer_target_offset(batch.target,job.coordinate)]);
            pass.set_bind_group(2,&job.bind_group,&[]);pass.draw(0..3,0..1);drop(pass);
            let actual=encoder.submission();self.record_raster_write(batch.target,output.publish(&actual))?;
        }
        for job in jobs {self.paint_layers[layer_index].pages.iter_mut().find(|page|page.coordinate==job.coordinate).unwrap().active_secondary=job.destination_secondary;}
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
        self.pending_display_selection = None;
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
                self.compilation.check()?;
                if !self.compilation.require(self.selection_clip.pipelines().into_iter().take(2), startup::BRUSH) {
                    self.pending_display_selection = Some(selection.clone());
                    return Ok(());
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
    prepared: object_image_mips::PreparedMipTile,
    tile: source_access::RawTile,
}
impl WgpuRasterizer {
    fn held_result_ready(&self)->bool {
        let receipt=self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade);
        self.source_results.iter().any(|(context,result)|*context==self.device.working_space() && self.decode_result_eligible(receipt.as_deref(),result))
    }
    fn decode_result_admitted(&self,result:&object_image_mips::PreparedImageWork)->bool {
        !self.source_tiles.borrow().uploads_full(result.class())
    }
    fn decode_result_eligible(&self,receipt:Option<&scene::sources::PendingTile>,result:&object_image_mips::PreparedImageWork)->bool {
        if !self.decode_result_admitted(result) {return false;}
        if let Some(receipt)=receipt { return Self::receipt_matches(receipt,result); }
        !self.window_samples_needed(result)
    }
    fn window_samples_needed(&self,result:&object_image_mips::PreparedImageWork)->bool {
        if let object_image_mips::PreparedImageWork::Image(tile)=result
            && self.moving_images.accepts_tile(tile.id,&tile.source,tile.coordinate)
            && let Some(view)=self.source_tiles.borrow().prepared_view(&tile.source,tile.coordinate)
            && self.moving_images.needs_samples(tile.id,&tile.source,tile.coordinate,view.texture().format()) { return true; }
        false
    }
    fn receipt_matches(receipt:&scene::sources::PendingTile,prepared:&object_image_mips::PreparedImageWork)->bool {
        match prepared {
            object_image_mips::PreparedImageWork::Image(tile)=>receipt.matches_image(&tile.source,tile.coordinate),
            object_image_mips::PreparedImageWork::Raster(tile)=>receipt.matches_raster(&tile.blob,tile.space,tile.destination),
            object_image_mips::PreparedImageWork::Coordinates {..}|object_image_mips::PreparedImageWork::Paper {..}=>false,
        }
    }
    pub(crate) fn decode_queue_has_room(&self) -> bool {
        self.moving_decode.as_ref().is_none_or(|(_,queue)|queue.accepts_held(self.source_results.len(),false))
    }
    pub(crate) fn image_decode_waiting(&self) -> bool {
        if self.held_result_ready() || self.papers.images_ready() {return false;}
        if self.bake_preflight.as_ref().is_some_and(|state|state.waiting()) {return true;}
        #[cfg(target_arch = "wasm32")]
        if self.browser_image_result.borrow().is_some() { return false; }
        self.moving_decode.as_ref().map_or(self.papers.image_waiting(),|(_,queue)|
            !queue.ready() && (queue.pending() || self.papers.image_waiting()))
    }
    pub(crate) fn drain_image_decode(&mut self, encoder: &mut submission::CommandEncoder) -> Result<Option<PreparedImageUpload>, GpuRasterError> {
        #[cfg(target_arch = "wasm32")]
        if let Some((sequence,result)) = self.browser_image_result.borrow_mut().take() {
            if let Some((_,queue)) = &mut self.moving_decode { queue.complete_external(sequence,result); }
        }
        if self.source_tiles.borrow().uploads_full(residency::Class::Visible) { return Ok(None); }
        let receipt=self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade);
        let space=self.device.working_space();
        self.source_results.retain(|(context,_)|*context==space);
        let held=self.source_results.iter().position(|(_,result)|self.decode_result_eligible(receipt.as_deref(),result));
        let Some(prepared)=held.and_then(|index|self.source_results.remove(index)).map(|(_,result)|result)
            .or_else(||self.moving_decode.as_mut().and_then(|(_,queue)|queue.poll())) else {return Ok(None);};
        let prepared=match prepared {
            object_image_mips::PreparedImageWork::Raster(prepared) if prepared.scalar=>{
                self.decode_consumed();
                self.hold_restore_scalar(prepared);return Ok(None);
            },
            prepared=>prepared,
        };
        if !self.decode_result_admitted(&prepared) || receipt.as_ref().is_some_and(|receipt|!Self::receipt_matches(receipt,&prepared)) {
            self.source_results.push_back((space,prepared));return Ok(None);
        }
        let result = (|| match &prepared {
            object_image_mips::PreparedImageWork::Paper {..} => Err(GpuRasterError::InvalidImage),
            object_image_mips::PreparedImageWork::Coordinates {destination,pixels} => {
                if let Some(destination) = destination.upgrade() { destination.lock().unwrap().write(pixels.clone()); }
                Ok(None)
            },
            object_image_mips::PreparedImageWork::Raster(prepared) => {
                let pixels = match &prepared.pixels {
                    Ok(pixels) => pixels,
                    Err(error) => {
                        let weak=Arc::downgrade(&prepared.blob);
                        self.failed_raster_tiles.retain(|(blob,space,destination,_)|blob.strong_count()>0
                            && !(blob.ptr_eq(&weak) && *space==prepared.space && *destination==prepared.destination));
                        self.failed_raster_tiles.push((weak,prepared.space,prepared.destination,error.clone()));
                        return Ok(None);
                    },
                };
                if !self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade).is_some_and(|receipt|receipt.accept_raster(&prepared.blob,prepared.space,prepared.destination,pixels)) {
                    scene::Scene::decode_prepared_raster(self,encoder,&prepared.blob,prepared.space,prepared.destination,pixels.clone(),prepared.class)?;
                }
                Ok(None)
            },
            object_image_mips::PreparedImageWork::Image(prepared) => {
                if !object_image_mips::retain_image_decode(prepared,self.device.working_space(),&mut self.failed_image_tiles) {return Ok(None);}
                let pixels=prepared.pixels.as_ref().map_err(|error|GpuRasterError::Color(error.clone()))?;
                if self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade).is_some_and(|receipt|receipt.accept_image(&prepared.source,prepared.coordinate,pixels)) {
                    return Ok(None);
                }
                let tile=scene::Scene::decode_prepared_for(self,encoder,&prepared.source,prepared.coordinate,pixels.clone(),prepared.use_,prepared.class)?;
                Ok(Some(PreparedImageUpload {prepared:prepared.clone(),tile}))
            },
        })();
        if matches!(&result,Err(GpuRasterError::NotReady|GpuRasterError::SourceWorkingSetExceeded)) {
            self.source_results.push_front((space,prepared));
            return Ok(None);
        }
        self.decode_consumed();
        result
    }
    fn decode_consumed(&mut self) {
        self.preparation_progress=self.preparation_progress.saturating_add(1);
        self.arrivals.fetch_add(1,std::sync::atomic::Ordering::AcqRel);
    }
    pub(crate) fn drain_image_decodes(&mut self, encoder: &mut submission::CommandEncoder) -> Result<(), GpuRasterError> {
        loop {
            let previous=self.preparation_progress;
            if let Some(upload)=self.drain_image_decode(encoder)? {
                let prepared=upload.prepared;
                if self.moving_images.accepts_tile(prepared.id,&prepared.source,prepared.coordinate)
                    && self.moving_images.needs_samples(prepared.id,&prepared.source,prepared.coordinate,upload.tile.texture.format()) {
                    self.source_results.push_back((self.device.working_space(),object_image_mips::PreparedImageWork::Image(prepared)));
                    break;
                }
            }
            if self.preparation_progress==previous || !(self.held_result_ready() || self.moving_decode.as_ref().is_some_and(|(_,queue)|queue.ready())) {break;}
        }
        Ok(())
    }
    pub(crate) fn register_image_decode_waker(&self,wake:&std::task::Waker) {
        #[cfg(not(target_arch="wasm32"))]
        if let Some((_,queue))=&self.moving_decode {queue.register_waker(wake);}
        #[cfg(target_arch="wasm32")]
        { *self.browser_image_wake.borrow_mut()=Some(wake.clone()); }
        self.papers.register_waker(wake);
        if let Some(bake)=&self.bake_preflight {bake.register_decode_waker(wake);}
        if let Some(slot)=&self.snapshot_decode_wake {let previous=slot.lock().unwrap().replace(wake.clone());drop(previous);}
    }
    pub(crate) fn clear_image_decode_waker(&self) {
        #[cfg(not(target_arch="wasm32"))]
        if let Some((_,queue))=&self.moving_decode {queue.clear_waker();}
        #[cfg(target_arch="wasm32")]
        { self.browser_image_wake.borrow_mut().take(); }
        self.papers.clear_waker();
        if let Some(bake)=&self.bake_preflight {bake.clear_decode_waker();}
        if let Some(slot)=&self.snapshot_decode_wake {let waiter=slot.lock().unwrap().take();drop(waiter);}
    }
    pub(crate) async fn await_image_decode(&self) -> Result<(), GpuRasterError> {
        struct Subscription<'a>(&'a WgpuRasterizer);
        impl Drop for Subscription<'_> {fn drop(&mut self) {self.0.clear_image_decode_waker();}}
        let _subscription=Subscription(self);
        std::future::poll_fn(|cx| {
            self.register_image_decode_waker(cx.waker());
            if self.snapshot_cancelled.as_ref().is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                return std::task::Poll::Ready(Err(GpuRasterError::Color("Snapshot capture cancelled".into())));
            }
            if !self.image_decode_waiting() { return std::task::Poll::Ready(Ok(())); }
            std::task::Poll::Pending
        }).await
    }
    fn image_decode_queue(&mut self,space:layer_core::color::RgbSpace)->Result<&mut object_image_mips::MipDecodeQueue,GpuRasterError> {
        if self.moving_decode.is_none() {
            let queue=object_image_mips::MipDecodeQueue::new(&self.device)?;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(wake)=&self.decode_wake {queue.set_wake(wake.clone());}
            self.moving_decode=Some((space,queue));
        }
        Ok(&mut self.moving_decode.as_mut().unwrap().1)
    }
    pub fn set_preparation_wake(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.compilation.set_wake(wake.clone());
        #[cfg(not(target_arch = "wasm32"))]
        self.device.retirement_handle().on_retired(wake.clone());
        if let Some(job)=&self.analysis_job {job.set_wake(wake.clone());}
        if let Some(job)=&self.snapshot_job {job.set_wake(wake.clone());}
        for task in &self.bake_analyses {task.set_wake(wake.clone());}
        self.memory.set_wake(wake.clone());
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.papers.set_wake(wake.clone());
            if let Some((_,queue))=&self.moving_decode {queue.set_wake(wake.clone());}
            self.decode_wake=Some(wake.clone());
        }
        let arrivals=self.arrivals.clone();
        let wake:Arc<dyn Fn() + Send + Sync>=Arc::new(move||{arrivals.fetch_add(1,std::sync::atomic::Ordering::AcqRel);wake();});
        self.set_raster_preparation_wake(wake.clone());
        self.preparation_wake=Some(wake);
    }
    pub fn preparation_progress(&self) -> u64 { self.preparation_progress }
    pub(crate) fn arrival_token(&self) -> frame_demand::ArrivalToken { frame_demand::ArrivalToken(self.arrivals.load(std::sync::atomic::Ordering::Acquire)) }
    pub(crate) fn arrived_since(&self, token: frame_demand::ArrivalToken) -> bool { self.arrivals.load(std::sync::atomic::Ordering::Acquire)!=token.0 }
    fn gpu_results_pending(&self) -> bool { self.frame_results_pending() || self.thumbnails_pending() }
    fn frame_results_pending(&self) -> bool {
        self.settling.is_some() || !self.background_ready.load(std::sync::atomic::Ordering::Acquire)
            || self.color_sample_pending() || self.selection_paint_pending() || self.region_pending()
            || self.filter_previews_pending() || self.effect_validation_pending() || self.brush_stamp_pending() || self.fill_preview_in_flight()
    }
    pub fn raster_preparation_ready(&self, previous: u64) -> bool {
        self.preparation_progress()>previous && self.restore_preparation().is_some() && (self.restore_pages_ready()
            || !self.source_tiles.borrow().uploads_full(residency::Class::Authored)
                && (self.held_result_ready() || self.moving_decode.as_ref().is_some_and(|(_,queue)|queue.ready())))
    }
    pub(crate) fn request_image_decode(&mut self, id: layer_core::authored::PortableId, source: Arc<layer_core::color::source::SourceImage>, coordinate: [u32; 2], class: residency::Class) -> Result<bool, GpuRasterError> {
        self.request_image_decode_for(id,source,coordinate,scene::sources::SourceUse::Native,class)
    }
    pub(crate) fn request_exact_image_decode(&mut self,id:layer_core::authored::PortableId,source:Arc<layer_core::color::source::SourceImage>,coordinate:[u32;2],class:residency::Class)->Result<bool,GpuRasterError> {
        self.request_image_decode_for(id,source,coordinate,scene::sources::SourceUse::Exact,class)
    }
    fn request_image_decode_for(&mut self,id:layer_core::authored::PortableId,source:Arc<layer_core::color::source::SourceImage>,coordinate:[u32;2],use_:scene::sources::SourceUse,class:residency::Class)->Result<bool,GpuRasterError> {
        #[cfg(target_arch="wasm32")]
        if self.browser_image_decoder.is_none() { return Err(GpuRasterError::Color("Image preparation worker unavailable".into())); }
        let space = self.device.working_space();
        if let Some(error)=object_image_mips::image_decode_failure(&self.failed_image_tiles,id,&source,coordinate,space) {
            return Err(GpuRasterError::Color(error.to_owned()));
        }
        if self.moving_decode.as_ref().is_some_and(|(context,_)| *context != space) {
            self.moving_decode = None;self.source_results.clear();
            #[cfg(target_arch = "wasm32")]
            { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
        }
        for (context,result) in &mut self.source_results {
            if *context==space && let object_image_mips::PreparedImageWork::Image(tile)=result
                && Arc::ptr_eq(&tile.source,&source) && tile.coordinate==coordinate {
                if use_==scene::sources::SourceUse::Exact {tile.use_=use_;}
                tile.class=tile.class.min(class);
                return Ok(true);
            }
        }
        let receipt=use_==scene::sources::SourceUse::Exact || self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade).is_some_and(|receipt|receipt.matches_image(&source,coordinate));
        if self.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.accepts_held(self.source_results.len(),receipt)) {return Ok(false);}
        let requested = self.image_decode_queue(space)?.request_for(id,source,coordinate,use_,class);
        #[cfg(target_arch = "wasm32")]
        self.dispatch_browser_image_decode()?;
        Ok(requested)
    }
    pub(crate) fn request_raster_decode_to(&mut self, blob: Arc<layer_core::raster::TileBlob>, source: layer_core::color::RgbSpace,destination:layer_core::color::RgbSpace,class:residency::Class) -> Result<bool, GpuRasterError> {
        self.request_raster_preparation(blob,source,destination,false,class)
    }
    pub(crate) fn request_scalar_decode(&mut self, blob: Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace) -> Result<bool, GpuRasterError> {
        self.request_raster_preparation(blob,space,space,true,residency::Class::Authored)
    }
    fn request_raster_preparation(&mut self, blob: Arc<layer_core::raster::TileBlob>, source: layer_core::color::RgbSpace,destination:layer_core::color::RgbSpace,scalar:bool,class:residency::Class) -> Result<bool, GpuRasterError> {
        scene::sources::validate_raster(&blob, source)?;
        #[cfg(target_arch="wasm32")]
        if self.browser_raster_decoder.is_none() && !self.snapshot_worker { return Err(GpuRasterError::Color("Raster preparation worker unavailable".into())); }
        let space = self.device.working_space();
        if self.moving_decode.as_ref().is_some_and(|(context,_)|*context!=space) {
            self.moving_decode=None;self.source_results.clear();
            #[cfg(target_arch="wasm32")]
            {self.browser_image_result=Default::default();self.browser_image_wake=Default::default();}
        }
        if let Some((_,held))=self.source_results.iter_mut().find(|(context,result)|*context==space && matches!(result,object_image_mips::PreparedImageWork::Raster(tile) if Arc::ptr_eq(&tile.blob,&blob) && tile.space==source && tile.destination==destination && tile.scalar==scalar)) {
            held.raise(class);return Ok(true);
        }
        let receipt=!scalar && self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade).is_some_and(|receipt|receipt.matches_raster(&blob,source,destination));
        if self.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.accepts_held(self.source_results.len(),receipt)) {return Ok(false);}
        let weak=Arc::downgrade(&blob);
        if let Some((_,_,_,error))=self.failed_raster_tiles.iter().find(|(stored,from,to,_)|stored.ptr_eq(&weak) && *from==source && *to==destination) {
            return Err(GpuRasterError::Color(error.clone()));
        }
        let requested = self.image_decode_queue(space)?.request_raster(blob, source, destination, scalar, class);
        #[cfg(target_arch="wasm32")]
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
            self.moving_decode = None;self.source_results.clear();
            #[cfg(target_arch = "wasm32")]
            { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
        }
        if self.moving_decode.as_ref().is_some_and(|(_,queue)|!queue.accepts_held(self.source_results.len(),false)) {return Ok(false);}
        let requested = self.image_decode_queue(space)?
            .request_coordinates(inverse,size,first,count,destination);
        #[cfg(target_arch = "wasm32")]
        self.dispatch_browser_image_decode()?;
        Ok(requested)
    }
    #[cfg(target_arch = "wasm32")]
    fn dispatch_browser_image_decode(&mut self) -> Result<(), GpuRasterError> {
        if let Some((sequence,request)) = self.moving_decode.as_mut().and_then(|(_,queue)| queue.take_external()) {
            let result = self.browser_image_result.clone();
            let wake = self.browser_image_wake.clone();
            let future: std::pin::Pin<Box<dyn std::future::Future<Output=object_image_mips::PreparedImageWork>>> = match request {
                object_image_mips::MipDecodeRequest::Image(request) => {
                    let callback = self.browser_image_decoder.clone().ok_or_else(|| GpuRasterError::Color("Image preparation worker unavailable".into()))?;
                    let space = self.device.working_space();
                    Box::pin(async move {
                        let pixels = callback(request.source.clone(),request.coordinate,space).await;
                        object_image_mips::PreparedImageWork::Image(object_image_mips::PreparedMipTile {class:request.class,use_:request.use_,id:request.id,source:request.source,coordinate:request.coordinate,pixels})
                    })
                },
                object_image_mips::MipDecodeRequest::Raster(request) => {
                    let callback = self.browser_raster_decoder.clone();
                    if callback.is_none() && !self.snapshot_worker {return Err(GpuRasterError::Color("Raster preparation worker unavailable".into()));}
                    let cache=self.device.source_samples.clone();
                    Box::pin(async move {
                        let pixels = if let Some(callback)=callback {callback(request.blob.clone(), request.space, request.destination,false).await}
                            else {object_image_mips::prepare_raster_tile(&request.blob,request.space,&cache)};
                        object_image_mips::PreparedImageWork::Raster(object_image_mips::PreparedRasterTile {class:request.class,blob:request.blob,space:request.space,destination:request.destination,scalar:request.scalar,pixels})
                    })
                },
                object_image_mips::MipDecodeRequest::Paper {..}=>return Err(GpuRasterError::InvalidImage),
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
                let prepared=future.await;
                *result.borrow_mut() = Some((sequence,prepared));
                let signal = wake.borrow_mut().take();
                if let Some(waker) = signal { waker.wake(); }
            });
        }
        Ok(())
    }
    fn motion(&self,packet:FramePacket<'_>)->bool {
        !packet.dab_batches.is_empty() || self.moving_layer.is_some() || self.transform_preview.as_ref().is_some_and(|preview|preview.moving)
            || self.artwork_frame.as_ref().is_none_or(|old|old.view!=packet.view)
    }
    fn preparation_can_wake(&self)->bool {
        #[cfg(not(target_arch="wasm32"))] {self.preparation_wake.is_some()}
        #[cfg(target_arch="wasm32")] {true}
    }
    #[cfg_attr(target_arch="wasm32",expect(unused_variables,reason="Wasm uploads have no release wake to wait on"))]
    fn upload_window_open(&self,class:residency::Class)->bool {
        #[cfg(not(target_arch="wasm32"))]
        if let Some(blocked)=self.source_tiles.borrow().upload_blocked(class,scene::sources::NORMALIZED_PAGE_BYTES) {
            if let Some(wake)=self.preparation_wake.clone() {blocked.wake_on_release(&self.device.retirement_handle(),wake);}
            return false;
        }
        true
    }
    fn reserve_upload(&self,class:residency::Class,bytes:u64)->Option<residency::Reservation> {
        let admitted=self.source_tiles.borrow().admit_upload(class,bytes);
        match admitted {
            Ok(reservation)=>Some(reservation),
            #[cfg(not(target_arch="wasm32"))]
            Err(blocked)=>{
                if let Some(wake)=self.preparation_wake.clone() {blocked.wake_on_release(&self.device.retirement_handle(),wake);}
                None
            },
            #[cfg(target_arch="wasm32")]
            Err(blocked)=>Some(blocked.overcommit()),
        }
    }
    fn charge_upload(&mut self,encoder:&submission::CommandEncoder,reservation:residency::Reservation,bytes:u64) {
        let in_flight=self.source_tiles.borrow().charge_upload(encoder,reservation,bytes);
        self.metrics.source_upload_peak_bytes=self.metrics.source_upload_peak_bytes.max(in_flight);
    }
    fn decode_wait_blocks_frame(&self)->bool {self.preparation_can_wake() && self.image_decode_waiting()}
    pub(crate) fn objects_wait_for_wake(&self)->bool {self.decode_wait_blocks_frame() && self.scene.as_deref().is_some_and(scene::Scene::objects_waiting)}
    fn objects_runnable(&self)->bool {self.scene.as_deref().is_some_and(scene::Scene::objects_pending) && !self.objects_wait_for_wake()}
    fn moving_images_pending(&self) -> bool {
        self.moving_images_outstanding().is_some_and(|state| match state { frame_demand::Outstanding::Runnable => true, frame_demand::Outstanding::Blocked(input) => !self.wakes(input) })
    }
    fn window_samples_ready(&self) -> bool {
        self.source_results.iter().any(|(context,result)|*context==self.device.working_space() && self.window_samples_needed(result))
            && self.source_receipt.as_ref().and_then(std::sync::Weak::upgrade).is_none()
    }
    fn prepare_moving_projection(&mut self, packet:FramePacket<'_>) -> Result<(),GpuRasterError> {
        let available = !self.snapshot_worker;
        #[cfg(target_arch = "wasm32")]
        let available = available && self.browser_image_decoder.is_some();
        if MovingProjection::update(&mut self.moving_projection, packet.scene, packet.view,
            self.device.working_space(), available, &mut self.failed_image_tiles,self.moving_layer)? {
            let bytes = object_image_mips::MovingImages::demand(&self.moving_projection.as_ref().unwrap().requests, [packet.view.width_px,packet.view.height_px])?;
            if self.memory.mip_demand(bytes,[packet.view.width_px,packet.view.height_px]) {
                self.publish_memory();
                self.factory.memory_budget().flush_reports();
                self.refresh_memory_limits();
            }
        }
        Ok(())
    }
    fn prepare_moving_images(&mut self, packet: FramePacket<'_>, encoder: &mut submission::CommandEncoder) -> Result<bool, GpuRasterError> {
        if self.moving_projection.is_none() {self.prepare_moving_projection(packet)?;}
        let required = self.scene.as_ref().map(|scene| scene.required_object_sources()).transpose()?.unwrap_or_default();
        self.source_tiles.borrow().demand_images(required);
        let mut images = std::mem::take(&mut self.moving_images);
        images.retire_evicted(encoder);
        let budget = if packet.dab_batches.is_empty() && self.moving_layer.is_none() { MOVING_IMAGE_IDLE_STEPS } else { MOVING_IMAGE_INPUT_STEPS };
        let mut changed = false;
        let result:Result<bool,GpuRasterError> = (|| {
            if !self.moving_projection.as_ref().unwrap().requests.is_empty()
                && !self.compilation.require(images.pipelines(&self.device),startup::DOCUMENT) {
                self.moving_images_waiting=true;return Ok(false);
            }
            let budget_bytes = self.memory.mip_budget();
            let plan = images.plan(self, &self.moving_projection.as_ref().unwrap().requests, budget_bytes, encoder)?;
            self.moving_images_waiting = plan.waiting;
            changed = images.allocate(self, plan, encoder);
            if self.moving_decode.as_ref().is_some_and(|(space,_)| *space != self.device.working_space()) {
                self.moving_decode = None;self.source_results.clear();
                #[cfg(target_arch = "wasm32")]
                { self.browser_image_result = Default::default(); self.browser_image_wake = Default::default(); }
            }
            let mut steps=0;let mut allowance=object_image_mips::BuildBudget::new(budget);
            while steps<budget {
                let cached=images.next_build(&mut allowance,|_,source,coordinate,format| {
                    self.source_tiles.borrow().prepared_view(source,coordinate)
                        .is_some_and(|view|format.is_none_or(|format|view.texture().format()==format))
                });
                if let Some((id,source,coordinate))=cached {
                    let mut requested=scene::sources::Inputs::default();
                    requested.image_for(&source,coordinate,self.device.working_space(),scene::sources::SourceUse::Native);
                    let accepted=requested.accept_query(self,None)?;
                    let read=scene::sources::accepted_image(&accepted,&source,coordinate)?;
                    if !read.encode(self,encoder,residency::Class::Visible)? {return Err(GpuRasterError::NotReady);}
                    let result=images.write_tile(self,encoder,id,&source,coordinate,&read.tile.texture,None);
                    accepted.track(encoder);changed|=result?;
                    steps+=1;continue;
                }
                if let Some(upload)=self.drain_image_decode(encoder)? {
                    let prepared=upload.prepared;
                    if images.accept_build(&mut allowance,prepared.id,&prepared.source,prepared.coordinate) {
                        changed|=images.write_tile(self,encoder,prepared.id,&prepared.source,prepared.coordinate,&upload.tile.texture,
                            Some(prepared.pixels.as_ref().map_err(|error|GpuRasterError::Color(error.clone()))?))?;
                    }
                    steps+=1;continue;
                }
                break;
            }
            for (id,source,coordinate) in images.upcoming(MOVING_IMAGE_PREFETCH) {
                let needed=self.source_tiles.borrow().prepared_view(&source,coordinate)
                    .is_none_or(|view|images.needs_samples(id,&source,coordinate,view.texture().format()));
                if needed && !self.request_image_decode(id,source,coordinate,residency::Class::Visible)? {break;}
            }
            Ok(changed)
        })();
        self.moving_images = images;
        match result {
            Err(error) if error.is_preparation_pending() => {
                self.moving_images_waiting = true;
                Ok(changed)
            }
            result => result,
        }
    }
    fn hold_background(&mut self, refinement: bool) {
        self.background_refinement = refinement;
        self.background_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ready = CompletionFlag(self.background_ready.clone());
        self.device.on_queue_done(&self.queue,move || drop(ready));
    }
}
impl CanvasRenderer for WgpuRasterizer {
    fn is_preparation_pending(error: &Self::Error) -> bool { error.is_preparation_pending() }
    fn adopt_committed_pages(&mut self, previous:&mut Self) {
        #[cfg(not(target_arch="wasm32"))]
        self.adopt_raster_preparation_wake(previous);
        let current_device:&wgpu::Device=&self.device;
        let previous_device:&wgpu::Device=&previous.device;
        if current_device!=previous_device {return;}
        self.scale_display=previous.scale_display.take();
        self.artwork_frame=previous.artwork_frame.clone();
        self.document_extent=previous.document_extent;
        self.artwork_revision=previous.artwork_revision;
        self.composite_revision=previous.composite_revision;
        self.presentation=previous.presentation;
        self.navigator=std::mem::take(&mut previous.navigator);
        if self.document_color==previous.document_color {
            self.scene=previous.scene.take();
            std::mem::swap(&mut self.scene_tiers,&mut previous.scene_tiers);
            self.retired_display_bytes=residency::Tier::within(&self.scene_tiers.display);
            self.device.analysis_memory=residency::Tier::within(&self.scene_tiers.display);
            self.transforms=self.transforms.take().map(|transforms|transforms.charged(&self.scene_tiers.display));
            previous.transforms=previous.transforms.take().map(|transforms|transforms.charged(&previous.scene_tiers.display));
            std::mem::swap(self.source_tiles.get_mut(),previous.source_tiles.get_mut());
            std::mem::swap(&mut self.moving_images,&mut previous.moving_images);
            self.moving_projection=previous.moving_projection.take();
            self.moving_decode=previous.moving_decode.take();
            self.source_receipt=previous.source_receipt.take();
            self.source_results=std::mem::take(&mut previous.source_results);
            #[cfg(not(target_arch="wasm32"))]
            if let Some((_,queue))=&self.moving_decode {
                if self.preparation_wake.is_none() {self.preparation_wake=previous.preparation_wake.clone();}
                if self.decode_wake.is_none() {self.decode_wake=previous.decode_wake.clone();}
                if let Some(wake)=&self.decode_wake {queue.set_wake(wake.clone());}
            }
            #[cfg(target_arch="wasm32")]
            {
                std::mem::swap(&mut self.browser_image_result,&mut previous.browser_image_result);
                std::mem::swap(&mut self.browser_image_wake,&mut previous.browser_image_wake);
            }
            previous.publish_memory();self.publish_memory();
        }
    }
    fn shader_input(&mut self) { WgpuRasterizer::shader_input(self); }
    fn shader_idle(&mut self, idle: bool, speculative_idle: bool) { WgpuRasterizer::shader_idle(self, idle, speculative_idle); }
    fn preparation_key(&self, document: &layer_core::Document) -> Option<Arc<dyn layer_render::PreparationKey>> {
        Some(self.startup.as_ref().and_then(|startup| startup.document_key.as_ref())
            .filter(|key| key.matches(document)).cloned().unwrap_or_else(|| Arc::new(ShaderDocument::new(document))))
    }
    fn prepare_context(&mut self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, context: layer_render::PreparationContext) {
        WgpuRasterizer::prepare_context(self, document, brush, context);
    }
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
    fn region_fits(&self, extent: [u32; 2]) -> bool {
        flood::fits(extent, &self.device.limits())
    }
    fn memory_headroom(&self) -> Option<u64> {
        Some(self.factory.memory_budget().import_headroom_bytes())
    }
    fn new_drawing_working_bytes(&self, viewport:[u32;2])->Option<u64> {
        Some(memory::new_drawing_working_bytes(viewport,self.factory.unified_memory()))
    }
    fn paint_ready(&self, document: &layer_core::Document, brush: &layer_core::BrushSnapshot, transform: bool) -> bool {
        self.startup.as_ref().is_none_or(|startup| startup.brush_ready) && !self.startup_needs_update(document, brush, transform)
    }
    fn raster_dependencies_ready(&mut self, packet: FramePacket<'_>, preview: Option<&layer_render::TransformPreview>) -> bool {
        self.activate_presentation(packet.scene);
        if self.retained_transition_frame(packet, preview) { return true; }
        if preview.is_some_and(|preview| preview.transform.validate().is_err()) { return true; }
        let installed = preview.map(|preview| self.transform_preview.replace(preview.clone()));
        self.register_tiled_sources(packet.scene);
        self.begin_frame_memory(packet);
        if self.bake_preflight.as_ref().is_some_and(|state|!state.matches(packet,self.document_color())) {self.bake_preflight=None;}
        if self.fill_edges_preflight.as_ref().is_some_and(|state|!state.matches(packet,self.document_color())) {self.fill_edges_preflight=None;}
        let mut required = compile::Needs::default();
        self.transforms.as_ref().unwrap().require_frame(self, packet, preview, &mut required);
        if let Some(native)=&self.native_edit {
            let gray=packet.scene.targets().any(|t|packet.scene.color_mode(t)!=layer_core::color::LayerColorMode::FullColor);
            required.compute.extend(native.required_pipelines(self.document_color.depth,gray).cloned());
        }
        if packet.scene.targets().any(SourceTarget::is_coverage) {
            required.render.push(self.layer_masks.initialize.clone());
            required.compute.extend(self.selection_clip.pipelines().into_iter().cloned());
        }
        let transforms_ready = required.require(&self.compilation,startup::BRUSH);
        let scene_ready = self.compilation.require(self.scene_pipelines.for_scene(self,packet.scene,packet.dab_batches), startup::DOCUMENT)
            & self.compilation.require([&self.scene_pipelines.fold.pipeline], startup::DOCUMENT);
        let fill_ready=if let Some(fill)=self.fill_preview.clone() {
            let pipeline=scene::operation_pipeline(&fill.operation.kind).unwrap_or(0);
            self.compilation.require([&self.scene_pipelines.pipeline[pipeline],&self.layer_masks.initialize],startup::BRUSH)
                & self.compilation.require(self.selection_clip.pipelines(),startup::BRUSH)
        }else{true};
        let ready=fill_ready && transforms_ready && scene_ready && self.scene_scale_ready(packet.scene) && self.startup.as_ref().is_none_or(|s| s.document.ready() && s.current.ready())
            && self.brush_batches_ready(packet)
            && self.prepare_raster_restores(packet) && self.bake_analyses_ready(packet) && self.retouch_analyses_ready(packet) && self.retouch_references_ready(packet)
            && self.bake_vectors_ready(packet) && self.prepare_bakes(packet) && self.prepare_fill_edges(packet) && self.prepare_operation_masks(packet) && self.prepare_transform_cancel(packet) && self.prepare_transform_preview(packet,preview);
        if !ready {self.upload_window_open(residency::Class::Visible);}
        if let Some(installed) = installed { self.transform_preview = installed; }
        ready
    }
    fn has_pending_submission(&self) -> bool { self.settling.is_some() }
    fn next_frame(&self) -> layer_render::FrameDemand { self.frame_demand() }
    fn waits_for_frame_slot(&self) -> bool { self.frame_slot_blocked() }
    fn poll_pending(&mut self, view: layer_render::ViewState) -> Result<(), Self::Error> {
        self.refresh_memory_limits();
        if let Some(selection) = self.pending_display_selection.clone() { self.set_selection_outline(Some(&selection))?; }
        let Some(mut pending) = self.settling.take() else { return Ok(()); };
        let moving = pending.view != view;
        pending.view = view;
        if moving || !self.background_ready.load(std::sync::atomic::Ordering::Acquire) || !self.raster_ready() {
            self.settling = Some(pending);
            return Ok(());
        }
        let healing = self.retouch.as_ref().is_some_and(|s| s.heal_pending());
        if !healing && pending.capture.is_none() {
            pending.capture=match self.prepare_native_rasters(pending.frame.scene.view(),None) {
                Ok(capture)=>capture,
                Err(error) if error.is_preparation_pending()=>{self.settling=Some(pending);return Ok(());},
                Err(error)=>return Err(error),
            };
        }
        let work = healing || pending.capture.as_ref().is_some_and(|capture| !capture.complete());
        if healing { self.record_restore_uploads(|r, encoder| r.drain_image_decodes(encoder))?; }
        if !healing && let Some(capture) = &mut pending.capture {
            match self.admit_native_window(capture) {
                Err(error) if error.is_preparation_pending() => { self.settling = Some(pending); return Ok(()); },
                result => result?,
            }
        }
        if work {
            let mut encoder = submission::CommandEncoder::new(&self.device, &wgpu::CommandEncoderDescriptor { label: Some("healing settle") });
            self.telemetry.begin(&self.device, &self.queue, encoder.diagnostics(), self.metrics.submissions);
            let stepped = if healing {
                let mut sources = self.retouch_sources();
                let result = sources.step_heal(self, &mut encoder);
                self.retouch = Some(sources);
                match result {
                    Err(error) if error.is_preparation_pending()=>{self.settling=Some(pending);return Ok(());},
                    result=>result?,
                }
            } else if let Some(capture) = &mut pending.capture { self.step_native_rasters(capture, &mut encoder)? } else { false };
            self.telemetry.end(encoder.diagnostics());
            debug_assert!(stepped);
            self.apply_frame_grants(&mut encoder);
            self.uploads.finish(&encoder);
            self.metrics.command_passes += encoder.pass_count();
            self.last_submission = encoder.submit(&self.queue);
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
        self.settling.is_none() && self.raster_ready()
            && (self.background_ready.load(std::sync::atomic::Ordering::Acquire) || self.background_refinement)
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
    fn prepare_retouch(&mut self, retouch: Option<&layer_render::RetouchPreparation>) {
        self.prepare_retouch_sources(retouch);
    }
    fn prepare_regions(&mut self,regions:Option<&layer_render::RegionPreparation>) {self.prepare_region_cache(regions);}
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
        for layer in &mut self.paint_layers {
            for page in &mut layer.pages { page.clear_stroke_start(); }
            layer.coverage_pages.retain(|p| p.primary.size().height == PAGE_SIZE);
        }
        for page in &mut self.preview_pages { page.clear_stroke_start(); }
        self.preview_coverage_pages.retain(|p| p.primary.size().height == PAGE_SIZE);
        self.live_stroke = None;
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
    fn set_fill_preview(&mut self,preview:Option<layer_render::FillPreview>) {self.fill_preview=preview;}
    fn fill_preview_ready(&self)->Option<(u64,u64)> {
        let key=self.fill_preview.as_ref()?.key();
        (self.fill_preview_shown==Some(key) && self.fill_preview_done.as_ref()?.load(std::sync::atomic::Ordering::Acquire)).then_some((key.0,key.1))
    }
    fn set_fill_reveal(&mut self,reveal:Option<layer_render::FillReveal>) {self.fill_reveal=reveal;}
    fn request_region(
        &mut self,
        request: layer_render::RegionTask,
    ) -> Result<bool, Self::Error> {
        self.start_region(request)
    }
    fn take_region(&mut self) -> Option<Result<layer_render::RegionOutput, Self::Error>> {
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
    fn cancel_selection_paint(&mut self) { self.selection_painter = None; self.display_selection = None; self.pending_display_selection = None; }
    fn set_quick_mask_thumbnail(&mut self, selection: Option<&layer_core::Selection>) {
        if let Some(selection) = selection { self.selection_previews.definitions.insert(SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID), selection.clone()); }
        else { self.selection_previews.definitions.remove(&SourceTarget::Selection(layer_core::authored::SelectionHandle::INVALID)); }
    }
    fn set_selection_overlay(&mut self, overlay: Option<layer_render::SelectionOverlay>) { self.selection_overlay = overlay; }
    fn set_crop_overlay(&mut self, overlay: Option<layer_render::CropOverlay>) { self.crop_overlay = overlay; }
    fn set_vector_preview(&mut self, preview: Option<layer_render::VectorPreview>) {
        self.vector_preview = preview;
    }
    fn set_vector_selection(&mut self, selection: Option<&layer_core::authored::VectorItemSelection>) {
        let changed = selection.map(|s| (s.layer, Arc::as_ptr(&s.items))) != self.vector_selection.as_ref().map(|s| (s.layer, Arc::as_ptr(&s.items)));
        self.vector_selection_changed |= changed;
        if changed { self.vector_selection = selection.cloned(); }
    }
    fn set_vector_motion(&mut self, motion: Option<layer_render::VectorMotion>) {
        self.vector_motion = motion;
    }
    fn set_working_layer(&mut self, layer: Option<layer_core::OccurrenceHandle>) {
        self.pivot = layer;
    }
    fn set_vector_motion_paths(&mut self, paths: Option<Arc<[layer_core::authored::vector::PathItem]>>) {
        self.vector_motion_paths = paths;
    }
    fn set_paper_overlay(&mut self, overlay: Option<layer_render::PaperOverlay>) {
        if let Some(overlay) = &overlay && self.require_paper_slope(&overlay.paper, startup::VALIDATION).is_err() {
            return;
        }
        if self.paper_overlay.as_ref().map(|o| &o.paper) != overlay.as_ref().map(|o| &o.paper) {
            self.paper_overlay_drawn.set(false);
        }
        self.paper_overlay = overlay;
    }
    fn paper_overlay_ready(&self, paper: &layer_core::paper::PaperBinding) -> bool {
        self.paper_overlay_drawn.get() && self.paper_overlay.as_ref().is_some_and(|o| o.paper == *paper)
    }
    fn set_clipping_preview(&mut self, shadows: bool, highlights: bool) { self.clipping_preview = [shadows, highlights]; }
    fn set_telemetry_enabled(&mut self, enabled: bool) {
        self.telemetry.enabled = enabled;
    }
    fn showing_drawing(&self) -> bool { self.presentation.showing }
    fn showing_navigator(&self) -> bool { self.presentation.navigator }
    fn set_canvas_transition(&mut self, fade: layer_render::CanvasFade) { self.canvas_transition=fade; }
    fn telemetry(&self) -> layer_render::RendererTelemetry {
        let mut t = self.telemetry.snapshot(&self.device, &self.queue);
        #[cfg(not(target_arch = "wasm32"))]
        { t.memory = self.memory_samples.snapshot(); }
        t.memory_budget = self.factory.budget_snapshot();
        t.adapter = Some(self.factory.adapter_summary());
        let m = &self.metrics;
        t.submissions = m.submissions;
        t.dabs = m.dabs;
        t.dirty_pixels = m.composited_pixels;
        t.resident_bytes = self.resident_bytes();
        if let Some(scene) = &self.scene {
            t.effect_passes = scene.effect_passes;
        }
        t
    }
    fn request_thumbnail(&mut self, request_id: u64, target: layer_render::ThumbnailTarget) -> Result<(), Self::Error> {
        self.start_thumbnail(request_id, target)
    }
    fn take_thumbnail(&mut self) -> Option<Result<ReadbackImage, Self::Error>> {
        self.thumbnails.take()
    }
    fn request_brush_stamp(&mut self,id:u64,request:&layer_render::BrushStampRequest)->Result<bool,Self::Error> {
        self.start_brush_stamp(id,request)
    }
    fn take_brush_stamp(&mut self)->Option<Result<ReadbackImage,Self::Error>> {self.poll_brush_stamp()}
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
        #[cfg(target_arch="wasm32")]
        if let Some(wake)=&self.preparation_wake {self.snapshot_job.as_ref().unwrap().set_wake(wake.clone());}
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
        self.clear_preview_stamps();
        Ok(())
    }

    fn release_asset(&mut self, asset: &AssetId) {
        self.masks.retain(|stored| stored.id != *asset);
        self.texture_sets.clear();
        self.clear_preview_stamps();
    }

    fn evaluation_context(&self) -> EvaluationContext { self.submitted_context.clone().unwrap_or_default() }
    fn seed_evaluation_context(&mut self, context: EvaluationContext) { self.submitted_context = Some(context); self.effect_clocks.clear(); }
    fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> { self.submit_frame(packet, None) }
}
impl WgpuRasterizer {
    fn empty_artwork_frame(&self, packet: FramePacket<'_>, preview: Option<&layer_render::TransformPreview>) -> bool {
        packet.commit_rasters && !packet.composite_all && !packet.reset_layers && packet.restore_rasters.is_empty()
            && packet.dabs.is_empty() && packet.dab_batches.is_empty()
            && preview.is_none() && self.transform_preview.is_none()
            && self.fill_preview.as_ref().map(layer_render::FillPreview::key)==self.fill_preview_shown
            && self.transform_damage.is_empty()
            && self.moving_layer.is_none() && self.moving_pixels.is_none()
            && self.vector_preview.as_ref().map(|p| (p.owner, p.revision)) == self.vector_preview_shown.map(|(owner, revision, _)| (owner, revision))
            && self.vector_motion.as_ref() == self.vector_motion_drawn.as_ref().map(|(motion, _)| motion)
            && self.vector_motion_paths.as_ref().filter(|_| self.vector_motion.is_some()).map(Arc::as_ptr) == self.vector_motion_paths_drawn.as_ref().map(Arc::as_ptr)
            && !self.vector_selection_changed
            && self.artwork_frame.as_ref().is_some_and(|frame| frame.view == packet.view && frame.same_artwork(packet))
    }
    fn retained_transition_frame(&self, packet: FramePacket<'_>, preview: Option<&layer_render::TransformPreview>) -> bool {
        self.canvas_transition.drawing.is_some() && self.presentation.showing && self.presentation.navigator && !self.analysis_dirty
            && self.empty_artwork_frame(packet, preview)
            && self.document_damage.is_empty()
    }
    fn clear_preview_stamps(&mut self) {
        self.preview_valid = false;
        for page in &self.preview_pages {page.primary.set_preview(None);if let Some(secondary)=&page.secondary {secondary.set_preview(None);}}
    }
    fn submit_frame(&mut self, packet: FramePacket<'_>, native_commit: Option<raster::native_edit::NativeFrame>) -> Result<(), GpuRasterError> {
        self.activate_presentation(packet.scene);
        self.prepare_moving_projection(packet)?;
        let revision = self.composite_revision;
        self.begin_frame_memory(packet);
        let submitted = self.metrics.submissions;
        let navigator_revision = self.navigator.revision;
        let result = self.submit_frame_inner(packet, native_commit);
        self.end_frame_memory();
        self.frame_room(packet);
        self.brush_inputs=None;
        if let Err(error)=result {
            self.clear_preview_stamps();
            return Err(error);
        }
        if self.composite_revision != revision && self.has_presentable_frame()
            && matches!(packet.scene.scope(), None | Some(layer_core::SceneScope::All)) {
            self.presentation.showing = true;
        }
        if self.presentation.showing && self.navigator.revision != navigator_revision
            && matches!(packet.scene.scope(), None | Some(layer_core::SceneScope::All)) {
            self.presentation.navigator = true;
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
        #[cfg(test)]
        let artwork_revision_before=self.artwork_revision;
        if let Some(error)=self.take_bake_failure().or_else(||self.take_fill_edges_failure()).or_else(||self.take_restore_failure()) {return Err(error);}
        let retained_transition = self.retained_transition_frame(packet, self.transform_preview.as_ref());
        let analysis_dirty = std::mem::take(&mut self.analysis_dirty);
        let packet = FramePacket {composite_all: packet.composite_all || analysis_dirty, ..packet};
        let packet = FramePacket { inspect_mask: if self.clipping_preview.iter().any(|enabled| *enabled) { None } else { packet.inspect_mask }, ..packet };
        if packet.document_extent.iter().any(|n| *n > self.max_document_dimension()) {
            return Err(GpuRasterError::ExtentUnsupported);
        }
        if settled_commit.is_none() && (retained_transition
            || !self.background_ready.load(std::sync::atomic::Ordering::Acquire) && !self.navigator.pending()
            && !self.moving_images_pending()
            && !self.objects_runnable()
            && !self.scale_display.as_ref().is_some_and(scene::scale::Cache::approximating)
            && self.document_damage.is_empty()
            && self.empty_artwork_frame(packet, self.transform_preview.as_ref()))
        { return Ok(()); }
        self.update_paper_lighting(packet.scene)?;
        let display_request = scene::scale::request(self, packet)?;
        let mut trace_phase = performance_trace::Span::new(c"capy.prepare");
        if self.native_edit.is_some() {
            // Reject unsupported global dependencies before clearing/restoring
            // paint, allocating the composite, or submitting any part of a frame.
            let resident = self.scale_display.as_ref().map_or(0, |c| c.resident_bytes());
            let admitted = scene::Scene::window_plan(packet.scene, packet.document_extent, self.image_pixel_budget(resident), self.device.limits().max_texture_dimension_2d,self.scene.as_deref());
            if resident > 0 && admitted.is_err() {
                scene::Scene::window_plan(packet.scene, packet.document_extent, self.image_pixel_budget(0), self.device.limits().max_texture_dimension_2d,self.scene.as_deref())?;
            } else { admitted?; }
        }
        if source_access::placed_targets(packet.scene).any(|t| packet.scene.paint_base(t).is_some()) {
            // Source-backed photos own no paint initially. Prepare their bounded
            // capture spare pool during loading, before the first stroke needs it.
            self.prepare_source_backing()?;
        }

        let object_damage = (!analysis_dirty && !packet.reset_layers && packet.restore_rasters.is_empty()
            && packet.dabs.is_empty() && packet.dab_batches.is_empty()).then(|| {
            let old = self.artwork_frame.as_ref()?;
            if !old.same_evaluation(packet) { return None; }
            scene::Scene::object_edit_damage(old.scene.view().with_scope(&old.scope), packet.scene, packet.document_extent)
        }).flatten();
        if let Some(damage) = &object_damage { self.document_damage.extend(damage.regions.iter().copied()); }
        self.vector_preview_damage.clear();
        let shown = self.vector_preview.as_ref().map(|p| (p.owner, p.revision, scene::preview_bounds(packet.scene, p)));
        let preview_changed = shown.map(|(o, r, _)| (o, r)) != self.vector_preview_shown.map(|(o, r, _)| (o, r));
        if preview_changed {
            for bounds in scene::preview_damage(packet.scene, self.vector_preview_drawn.as_ref(), self.vector_preview.as_ref()) {
                let rect = bounds.in_frame(packet.document_extent);
                if !rect.is_empty() { self.vector_preview_damage.push(bounds); self.document_damage.push(rect); }
            }
            self.vector_preview_shown = shown;
            self.vector_preview_drawn = self.vector_preview.clone();
        }
        self.vector_motion_damage.clear();
        self.vector_exclusion_damage.clear();
        let paths = self.vector_motion_paths.clone().filter(|_| self.vector_motion.is_some());
        let paths_changed = paths.as_ref().map(Arc::as_ptr) != self.vector_motion_paths_drawn.as_ref().map(Arc::as_ptr);
        let motion_changed = self.vector_motion.as_ref() != self.vector_motion_drawn.as_ref().map(|(motion, _)| motion) || paths_changed;
        if motion_changed {
            let same = |a: &layer_render::VectorMotion, owner: OccurrenceHandle, items: &Arc<std::collections::BTreeSet<layer_core::authored::vector::ItemId>>| a.owner == owner && Arc::ptr_eq(&a.items, items);
            let drawn = self.vector_motion.as_ref().map(|motion| {
                let unchanged = self.vector_motion_drawn.as_ref().filter(|(old, _)| same(old, motion.owner, &motion.items)).map(|(_, bounds)| *bounds)
                    .or_else(|| self.vector_prepared.as_ref().filter(|((_, (owner, items), _), ..)| same(motion, *owner, &items.0)).map(|((_, _, bounds), ..)| *bounds));
                let bounds = unchanged.unwrap_or_else(|| scene::motion_bounds(packet.scene, motion.owner, &motion.items));
                (motion.clone(), paths.as_ref().map_or(bounds, |paths| bounds.union(scene::paths_bounds(packet.scene, motion.owner, paths))))
            });
            let moving = |drawn: &Option<(layer_render::VectorMotion, DocRect)>| drawn.as_ref().map(|(motion, _)| (motion.owner, Arc::as_ptr(&motion.items)));
            if moving(&drawn) != moving(&self.vector_motion_drawn) {
                self.vector_exclusion_damage.extend(self.vector_motion_drawn.iter().chain(&drawn).map(|(_, bounds)| *bounds));
            }
            let split = drawn.as_ref().and_then(|(motion, _)| scene::motion_split(packet.scene, motion.owner));
            let moved = |drawn: &Option<(layer_render::VectorMotion, DocRect)>, presented: bool| drawn.iter().filter(move |_| !presented)
                .map(|(motion, bounds)| scene::moved_bounds(motion.transform, *bounds).expand(64)).collect::<Vec<_>>();
            for bounds in moved(&self.vector_motion_drawn, self.vector_motion_split.is_some()).into_iter().chain(moved(&drawn, split.is_some())) {
                let rect = bounds.in_frame(packet.document_extent);
                if !rect.is_empty() { self.vector_motion_damage.push(bounds); self.document_damage.push(rect); }
            }
            self.vector_motion_drawn = drawn;
            self.vector_motion_paths_drawn = paths;
            self.vector_motion_split = split;
        }
        let prepared = self.vector_selection.as_ref().filter(|_| self.vector_motion.is_none())
            .and_then(|selection| scene::prepared(packet.scene, selection, self.vector_prepared.as_ref()));
        let key = |prepared: &Option<(scene::Prepared, layer_core::authored::vector::VectorObject, [f64; 2])>| prepared.as_ref().map(|((_, moving, bounds), ..)| (moving.clone(), *bounds));
        let prepared_changed = key(&prepared) != key(&self.vector_prepared);
        self.vector_prepared = prepared;
        self.vector_selection_changed = false;
        let packet = FramePacket { composite_all: packet.composite_all && object_damage.is_none(), ..packet };
        self.metrics.frame_composited_pages.clear();
        if let Some(t) = &mut self.transforms {
            t.begin_frame();
        }
        let context = self.frame_context(packet);
        let packet = FramePacket { scene: packet.scene.with_context(&context), ..packet };
        let started = self.telemetry.enabled.then(web_time::Instant::now);
        let mut cpu_phases = [0.; 6];
        self.metrics.frame_cpu_ms = [0.; 6];
        self.metrics.material_cpu_ms = [0.; 5];
        if let Some(scene) = &mut self.scene {
            scene.effect_passes = 0;
        }
        let requested_view = packet.view;
        let mut work = scene::Scene::begin_work(self.motion(packet));
        if let Some(scene) = &mut self.scene {
            scene.begin_frame();
            scene.begin_vector_frame();
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
        let same_artwork = self.artwork_frame.as_ref().is_some_and(|frame| frame.same_artwork(packet));
        let same_preview = self.preview_valid
            && packet.commit_rasters
            && !packet.reset_layers
            && packet.restore_rasters.is_empty()
            && settled_commit.is_none()
            && self
                .artwork_frame
                .as_ref()
                .is_some_and(|frame| frame.same_preview(packet));
        let same_frame = same_artwork || same_preview;
        let unchanged = !preview_changed
            && !motion_changed
            && !prepared_changed
            && same_frame
            && !(self.transform_sources_pending && self.transform_preview.is_some());
        let resized = self.ensure_document_metadata(packet.document_extent, packet.scene)?;
        let display_rebuilt = if packet.commit_rasters {
            let previous = self.scale_display.take();
            let (cache, rebuilt) = scene::scale::Cache::select(previous, self, packet, display_request, unchanged);
            self.scale_display = Some(cache);
            rebuilt
        } else { false };
        let packet = FramePacket { composite_all: packet.composite_all || blending_changed, ..packet };
        self.prepare_selection_previews(packet)?;
        let mut batch_tiles=brush_tiles::plan_packet(FramePacket {dab_batches:original_batches,..packet},|target|self.target_extent(target))?;
        let reset = packet.reset_layers || resized;
        liquify::Liquify::extend_required_plans(self,FramePacket {dab_batches:original_batches,..packet},&mut batch_tiles);
        if !self.prepare_raster_outputs(FramePacket {dab_batches:original_batches,reset_layers:reset,..packet},&batch_tiles) {
            return Err(GpuRasterError::NotReady);
        }
        let mut artwork_damage=self.transform_damage.iter().any(|(_,bounds)|!bounds.is_empty())
            || self.transform_preview.is_none() && self.transforms.as_ref().is_some_and(|transforms|transforms.has_preview());
        self.note_retouch_batches(packet.dab_batches, reset);
        if reset {
            if let Some(liquify) = &mut self.liquify { liquify.reset(); }
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
            self.clear_preview_pages();
            self.preview_damage = PixelRect::EMPTY;
            self.preview_layer_id = None;
            self.preview_requires_base = false;
            self.preview_contribution = false;
        }
        liquify::Liquify::extend_plans(self, packet, &mut batch_tiles);
        let mut encoder = crate::submission::CommandEncoder::new(
            &self.device,
            &wgpu::CommandEncoderDescriptor {
                label: Some("layer incremental sparse frame"),
            },
        );
        self.source_tiles.get_mut().flush_retired(&mut encoder);
        self.telemetry.begin(&self.device, &self.queue, encoder.diagnostics(), self.metrics.submissions + 1);
        let moving_images_changed = self.prepare_moving_images(packet, &mut encoder)?;
        if moving_images_changed && let Some(scene) = &mut self.scene {
            scene.invalidate_object_previews(packet.document_extent);
        }
        if self.transform_preview.is_none() && self.has_transform_commit() {self.transforms.as_mut().unwrap().discard_preview();}
        if !packet.dabs.is_empty() || !packet.dab_batches.is_empty() || !packet.restore_rasters.is_empty() || reset {
            self.transforms.as_mut().unwrap().release_standby();
        }
        // Restore both targets before allocating or painting new pages. Cancel
        // removes disposable preview pages, which must not swallow a new stroke.
        if self.cancels_transform_preview(packet) {
            let mut transforms = self.transforms.take().unwrap();
            let result = transforms.cancel_preview(self, &mut encoder);
            self.transforms = Some(transforms);
            self.transform_damage.extend(result?);
            self.transform_preview = None;
        }
        artwork_damage|=self.transform_damage.iter().any(|(_,bounds)|!bounds.is_empty());
        let raster_damage = self.reconcile_rasters(
            FramePacket {
                dab_batches: original_batches,
                ..packet
            },
            reset,
            &batch_tiles,
        )?;
        let committed_preview=if let Some(commit)=self.take_transform_commit() {
            commit.adopt(self,&encoder.submission())?
        }else {Vec::new()};
        if let Some(regions)=&mut self.regions {regions.changed(FramePacket {dab_batches:original_batches,..packet},&raster_damage);}
        self.transform_damage.extend(raster_damage);

        let fill_preview=self.fill_preview.clone();
        let fill_changed=fill_preview.as_ref().map(layer_render::FillPreview::key)!=self.fill_preview_shown;
        let old_preview_damage = self.preview_damage;
        let old_preview_footprint = if packet.commit_rasters {std::mem::take(&mut self.preview_footprint)} else {Default::default()};
        let mut new_preview_footprint = std::collections::BTreeSet::new();
        let old_preview_layer = self.preview_layer_id;
        let old_preview_level = self.preview_level;
        let old_preview_contribution = self.preview_contribution;
        let watercolor_style_dirty = self.update_watercolor_layer_styles(packet.dab_batches);
        let mut dirty = watercolor_style_dirty;
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
            let visual_dirty = Self::brush_display_damage(batch, tiles, self.target_extent(batch.target));
            dirty = self.stroke_finish_pages(batch).fold(dirty, |damage, page| {
                damage.union(page_rect(page).intersect(PixelRect::full(packet.document_extent)))
            });
            if tiles.is_empty() || (batch.style.execution != BrushExecution::Liquify && (batch_dirty.is_empty() || dabs.is_empty())) {
                continue;
            }
            if batch.kind == DabBatchKind::Preview {
                new_preview_footprint.extend(Self::preview_pages(packet.dab_batches, batch, tiles, self.target_extent(batch.target)));
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
            if batch.kind != DabBatchKind::Preview {
                dirty = dirty.union(visual_dirty);
            }
            self.metrics.dabs = self.metrics.dabs.saturating_add(dabs.len() as u64);
            for dab in dabs {
                self.metrics.raster_candidate_pixels = self
                    .metrics
                    .raster_candidate_pixels
                    .saturating_add(dab_candidate_pixels(*dab, self.target_extent(batch.target)));
            }
        }
        let preview_dirty = new_preview_damage;
        if let Some(fill)=&fill_preview {
            new_preview_layer=Some(fill.target);
            let damage=pixel_rect(fill.operation.bounds(packet.scene.target_extent(fill.target)),packet.scene.target_extent(fill.target));
            new_preview_damage=page_coordinates(damage).fold(PixelRect::EMPTY,|bounds,page|bounds.union(page_rect(page)))
                .intersect(PixelRect::full(packet.scene.target_extent(fill.target)));
            new_preview_requires_base=true;
            new_preview_footprint=page_coordinates(damage).collect();
            if fill_changed {dirty=dirty.union(new_preview_damage);}
        }
        let new_preview_from_persistent = self.preview_from_persistent(packet.dab_batches);
        let native_dry_preview = packet.dab_batches.iter()
            .filter(|b|b.kind==DabBatchKind::Preview && has_brush_work(b))
            .all(|b|dry_material::native_dry_preview_eligible(&b.style));
        let mut preview_contribution=new_preview_from_persistent && self.scale_display.as_ref().is_some_and(|cache|
                new_preview_layer.is_some_and(|id|cache.native_preview_input(packet,id)))
            && packet.dab_batches.iter().filter(|b|b.kind==DabBatchKind::Preview && has_brush_work(b))
                .all(|b|b.style.opacity==1. && dry_material::reduced_preview_eligible(&b.style) && b.style.mode==DabMode::Paint
                    && b.style.blend_space==packet.blend_space
                    && packet.scene.color_mode(b.target)==layer_core::color::LayerColorMode::FullColor);
        if preview_contribution {new_preview_requires_base=false;}
        let preview_level = self.scale_display.as_ref().filter(|_| new_preview_from_persistent
            && packet.dab_batches.iter().filter(|b| b.kind == DabBatchKind::Preview && has_brush_work(b))
                .all(|b| dry_material::reduced_preview_eligible(&b.style)
                    || b.style.execution == BrushExecution::Liquify && b.style.deform.mode != LiquifyMode::Crystals))
            .map_or(0, |cache| new_preview_layer.map_or(0, |id| cache.preview_level(self, packet, id, preview_contribution)));
        preview_contribution &= preview_level>0 || native_dry_preview;
        if !preview_contribution && new_preview_from_persistent {new_preview_requires_base=true;}
        let mut reuse_preview = same_preview
            && !reset
            && !display_rebuilt
            && !blending_changed
            && !moving_images_changed
            && !analysis_dirty
            && settled_commit.is_none()
            && fill_preview.is_none()
            && self.transform_preview.is_none()
            && !self.scale_display.as_ref().is_some_and(|cache|cache.has_pending_work(self))
            && packet.restore_rasters.is_empty()
            && self.transform_damage.is_empty()
            && self.document_damage.is_empty()
            && !self.preview_pages.is_empty()
            && old_preview_layer == new_preview_layer
            && old_preview_level == preview_level
            && old_preview_contribution == preview_contribution;
        let reuse_frame = reuse_preview
            && unchanged
            && !packet.composite_all
            && self.raster_ready()
            && !self.frame_work();
        if !reuse_frame {
            let mut changed_cells = self.changed_cells.take().unwrap();
            changed_cells.prepare(
                self,
                FramePacket {
                    dab_batches: original_batches,
                    ..packet
                },
                &batch_tiles,
                unchanged && !display_rebuilt,
                &mut encoder,
            );
            if settled_commit.is_some() {
                changed_cells.force_all();
            }
            if let Some(scene) = &mut self.scene {
                scene.retire_changed_cells(&changed_cells.take_retired());
            }
            self.changed_cells = Some(changed_cells);
        }
        if !reuse_preview {
            dirty = dirty.union(preview_dirty);
            if fill_preview.is_none() || fill_changed {
                dirty = dirty.union(old_preview_damage);
            }
        }
        self.preview_valid = false;
        if packet.commit_rasters {
            self.preview_contribution=preview_contribution;
            let mut preview_cells=self.changed_cells.take().unwrap();
            if !reuse_preview && (fill_preview.is_none() || fill_changed) {
                preview_cells.prepare_preview(
                    self,
                changed_cells::Preview {id:old_preview_layer,level:old_preview_level,contribution:old_preview_contribution,
                    tiles:&old_preview_footprint},
                changed_cells::Preview {id:new_preview_layer,level:preview_level,contribution:preview_contribution,
                    tiles:&new_preview_footprint});}
            self.changed_cells=Some(preview_cells);
            if self.preview_level != preview_level {
                self.preview_pages.clear();
                self.preview_level = preview_level;
            }
            if new_preview_layer.is_none() {
                self.clear_preview_pages();
            } else {
                if new_preview_from_persistent { self.preview_coverage_pages.clear(); }
                self.prepare_preview_outputs(self.preview_level)?;
            }

        }

        let stamps=self.scale_display.as_ref().and_then(|cache|cache.contact_stamps(packet));
        if stamps.is_none() {self.held_brush=None;}
        self.brush_inputs=if packet.dab_batches.iter().any(|batch|!batch.target.is_coverage()
            && BrushPassPlan::for_device(&batch.style,&self.device).requires_destination()) {
            Some(self.prepare_brush_inputs(new_preview_layer,stamps.clone())?)
        }else {None};

        if !reuse_frame {
            self.prepare_uploads(
                FramePacket {
                    dab_batches: original_batches,
                    ..packet
                },
                &mut batch_tiles,
                &mut encoder,
            )?;
        }
        self.reconcile_mask_previews();
        if !reuse_frame {
            self.layer_masks.prepare(
            &self.device,
            &mut encoder,
            (packet.scene, original_batches,fill_preview.as_ref()),
            packet.document_extent,
            false,
            &mut self.selection_clip,
            )?;
        }
        for batch in original_batches
            .iter()
            .filter(|b| layer_masks::MaskRenderer::is_mask(packet.scene, b.target))
        {
            dirty = dirty.union(batch_pixel_rect(batch, self.target_extent(batch.target)));
        }

        let receipt=encoder.submission();
        for layer in &mut self.paint_layers {
            let defaults=raster::contents::defaults(packet.scene,layer.id);
            for page in &mut layer.pages {
                if page.primary_needs_clear {
                    let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                    write.value().clear(&self.zero_pages,&mut encoder);
                    let transparent=raster::contents::defaults_for(packet.scene.target_extent(layer.id),None,None);
                    write.publish(raster::contents::fallback(&transparent[0],page.coordinate));
                }
            }
            for page in &mut layer.watercolor_wetness_pages {
                let name=raster::contents::fallback(&defaults[2],page.coordinate);
                if page.primary_needs_clear {
                    let write=page.primary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                    write.value().clear(&self.zero_pages,&mut encoder);write.publish(&name);
                }
                if page.secondary_needs_clear {
                    let write=page.secondary.initialize(&receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
                    write.value().clear(&self.zero_pages,&mut encoder);write.publish(&name);
                }
            }
        }
        if source_access::placed_targets(packet.scene).any(|t| packet.scene.paint_base(t).is_some() || self.native_backing(t).is_some()) {
            let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
            let result = scene.initialize_source_paint(self, packet.scene, &mut encoder);
            self.scene = Some(scene);
            result?;
        }
        self.keep_stroke_start_pages(packet, &batch_tiles, &mut encoder)?;
        for layer in &mut self.paint_layers {
            for page in &mut layer.pages {
                page.primary_needs_clear = false;
            }
            for page in &mut layer.watercolor_wetness_pages {
                page.primary_needs_clear = false;
                page.secondary_needs_clear = false;
            }
        }

        self.validate_raster_outputs()?;
        self.encode_mask_dabs(
            &mut encoder,
            packet,
            original_batches,
            &committed_preview,
            false,
        )?;

        if let Some(started) = started { cpu_phases[0] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.paint");
        self.telemetry.phase_begin(0, &self.device, &self.queue, encoder.diagnostics());
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
                        op,
                        operation,
                    );
                    self.transforms = Some(transforms);
                    result?;
                } else {
                    let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
                    let damage = batch_pixel_rect(batch, self.target_extent(batch.target));
                    let result = scene.apply_operation(self, packet, batch.target, scene::PaintOperation::Stored(op as usize), damage, &mut encoder);
                    self.scene = Some(scene);
                    result?;
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
                BrushEncodingContext {source:packet.scene,
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
            self.telemetry.phase_end(0, encoder.diagnostics());
            self.telemetry.end(encoder.diagnostics());
            self.metrics.command_passes += encoder.pass_count();
            self.last_submission = encoder.submit(&self.queue);
            self.telemetry.submitted(&self.queue);
            self.hold_background(false);
            self.metrics.submissions = self.metrics.submissions.saturating_add(1);
            self.submitted_context=Some(context);
            return Ok(());
        }

        if let Some(started) = started { cpu_phases[1] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.capture");
        self.telemetry.phase_end(0, encoder.diagnostics());
        let replay_window = !packet.commit_rasters && packet.dab_batches.iter().any(|batch| batch.kind == DabBatchKind::Persistent);
        let native_commit = match settled_commit {
            Some(frame) => Some(frame),
            None if replay_window => None,
            None => match self.encode_native_rasters(packet.scene,&mut encoder) {
                Ok(commit)=>commit,
                Err(error) if error.is_preparation_pending()=>{
                    self.settling=Some(artwork::PendingFrame {frame:Arc::new(artwork::Frame::new(packet,context.clone())),view:requested_view,capture:None});
                    None
                },
                Err(error)=>return Err(error),
            },
        };
        if let Some(started) = started { cpu_phases[2] = started.elapsed().as_secs_f64() * 1000.; }
        if !packet.commit_rasters {
            for (batch,tiles) in original_batches.iter().zip(&batch_tiles).filter(|(b,_)|b.kind!=DabBatchKind::Preview) {
                self.transform_damage.extend(tiles.iter().map(|tile|(batch.target,page_rect(tile.coordinate))));
                let finished:Vec<_>=self.stroke_finish_pages(batch).collect();
                self.transform_damage.extend(finished.into_iter().map(|c|(batch.target,page_rect(c))));
            }
            if let Some(frame)=&native_commit {self.transform_damage.extend(frame.canonical_pages.iter().map(|&(id,c)|(id,page_rect(c))));}
            self.transform_damage.sort_unstable_by_key(|&(id,rect)|(id,rect.min_x(),rect.min_y(),rect.max_x(),rect.max_y()));
            self.transform_damage.dedup();
            self.uploads.finish(&encoder);
            self.telemetry.end(encoder.diagnostics());
            self.metrics.command_passes += encoder.pass_count();
            let publication=encoder.submission();
            self.last_submission = encoder.submit(&self.queue);
            if let Some(liquify)=&mut self.liquify {liquify.publish(packet.scene,packet.dab_batches,&publication);}
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
        self.telemetry.phase_begin(1, &self.device, &self.queue, encoder.diagnostics());

        self.preview_damage = new_preview_damage;
        self.preview_footprint = new_preview_footprint;
        self.preview_layer_id = new_preview_layer;
        self.preview_requires_base = new_preview_requires_base;
        let preview_packet=FramePacket {dab_batches:original_batches,..packet};
        let preview_plans=self.preview_material_plans(preview_packet,&batch_tiles,new_preview_from_persistent)?;
        let declaration_matches=self.prepare_preview_declaration(preview_packet,scene::PreviewQuality {level:preview_level,contribution:preview_contribution,
            requires_base:new_preview_requires_base || original_batches.iter().any(|batch|batch.kind==DabBatchKind::Preview && batch.target.is_coverage())},&preview_plans,new_preview_from_persistent,stamps,&encoder.submission())?;
        reuse_preview&=declaration_matches;
        self.encode_mask_dabs(&mut encoder,packet,original_batches,&committed_preview,true)?;
        if let Some(layer_id) =
            new_preview_layer.filter(|_| !reuse_preview && (fill_preview.is_none() || fill_changed))
        {
            for page in &mut self.preview_pages {
                page.active_secondary = false;
                page.clear_stroke_start();
            }
            for page in &mut self.preview_coverage_pages {
                page.active_secondary = false;
            }
            for page in &mut self.preview_watercolor_wetness_pages {
                page.active_secondary = false;
            }
            let copied = new_preview_damage;
            let receipt=encoder.submission();
            if !new_preview_from_persistent {
                for coordinate in self.preview_footprint.clone() {
                    self.accepted_raster_output(layer_id,RasterOutput::PreviewColor(coordinate))?;
                    let intent=self.raster_intent(layer_id,RasterPlane::Color,coordinate)?;
                    let source=if new_preview_requires_base {
                        self.paint_layers.iter().find(|layer|layer.id==layer_id).ok_or(GpuRasterError::MissingPaintLayer(layer_id))?
                            .pages.iter().find(|page|page.coordinate==coordinate).map(|page|page.active().read(intent.version(),Some(&receipt))
                                .map(|read|(read.value().texture.clone(),read.version().clone())).ok_or(GpuRasterError::Invariant("prepared submit frame inner state is inconsistent"))).transpose()?
                    }else {None};
                    let ticket=self.preview_version(layer_id,RasterPlane::Color,coordinate,scene::PreviewStage::Seed)?;
                    let preview=prepared_raster_output(self.preview_pages.iter_mut().find(|page|page.coordinate==coordinate))?;
                    let write=preview.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                    if let Some((texture,_))=source {write.value().copy_from(&texture,texture.size(),&mut encoder);}
                    else {write.value().clear(&self.zero_pages,&mut encoder);}
                    write.publish(ticket.version());preview.derived=Some(ticket);
                }
            }
            if new_preview_requires_base && !new_preview_from_persistent
                && (packet.scene.paint_base(layer_id).is_some() || self.native_backing(layer_id).is_some()) {
                let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
                let result = scene.initialize_source_preview(self, packet.scene, layer_id, copied, &mut encoder);
                self.scene = Some(scene);
                result?;
            }
            for output in self.accepted_previews(layer_id) {
                let RasterOutput::PreviewCoverage(coordinate,slabs,rgba)=output else {continue;};
                let batch=prepared_raster_output(packet.dab_batches.iter().find(|batch|batch.kind==DabBatchKind::Preview && batch.target==layer_id
                    && BrushPassPlan::for_device(&batch.style,&self.device).state.coverage && stroke_coverage_slabs(&batch.style)==slabs && stable_blender(&batch.style)==rgba
                    && !batch_pixel_rect(batch,self.target_extent(batch.target)).intersect(page_rect(coordinate)).is_empty()))?;
                let index=prepared_raster_output(self.preview_coverage_pages.iter().position(|page|page.matches(coordinate,&batch.style)))?;
                let intent=self.raster_intent(layer_id,RasterPlane::Color,coordinate)?;
                let committed=self.paint_layers.iter().find(|layer|layer.id==layer_id).ok_or(GpuRasterError::MissingPaintLayer(layer_id))?
                    .coverage_pages.iter().find(|page|page.matches(coordinate,&batch.style) && page.owner==Some(batch.stroke_id));
                let source=committed.map(|page| {
                    let state=page.state.as_ref().filter(|state|state.matches(batch.stroke_id,&intent)).ok_or(GpuRasterError::Invariant("prepared submit frame inner state is inconsistent"))?;
                    page.active().read(&state.version,Some(&receipt)).map(|read|(read.value().texture.clone(),read.input()))
                        .ok_or(GpuRasterError::Invariant("prepared submit frame inner state is inconsistent"))
                }).transpose()?;
                let proofs=source.as_ref().map(|(_,input)|input.clone()).into_iter().collect();
                let ticket=self.preview_declaration.as_ref().ok_or(GpuRasterError::Invariant("prepared submit frame inner state is inconsistent"))?
                    .ticket_for(layer_id,RasterPlane::Color,coordinate,scene::PreviewStage::Seed,scene::PreviewPass::Coverage,intent.clone(),proofs)?;
                let coverage=&mut self.preview_coverage_pages[index];
                let write=coverage.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                if let Some((texture,_))=source {write.value().copy_from(&texture,texture.size(),&mut encoder);}
                else {write.value().clear(&self.zero_pages,&mut encoder);}
                let written=write.publish(ticket.version());
                coverage.state=Some(MaterialState {stroke:batch.stroke_id,input:intent,version:written.version().clone()});
            }
            for output in self.accepted_previews(layer_id) {
                let RasterOutput::PreviewWetness(coordinate)=output else {continue;};
                let index=prepared_raster_output(self.preview_watercolor_wetness_pages.iter().position(|page|page.coordinate==coordinate))?;
                let source=self.paint_layers.iter().find(|layer|layer.id==layer_id).ok_or(GpuRasterError::MissingPaintLayer(layer_id))?
                    .watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)
                    .map(|page|self.read_wetness(layer_id,page,Some(&receipt)).map(|read|(read.value().texture.clone(),read.version().clone()))).transpose()?;
                let ticket=self.preview_version(layer_id,RasterPlane::WatercolorWetness,coordinate,scene::PreviewStage::Seed)?;
                let page=&mut self.preview_watercolor_wetness_pages[index];
                let write=page.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                if let Some((texture,_))=source {write.value().copy_from(&texture,texture.size(),&mut encoder);}
                else {write.value().clear(&self.zero_pages,&mut encoder);}
                write.publish(ticket.version());page.derived=Some(ticket);
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
                    BrushEncodingContext {source:packet.scene,
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
        if fill_changed && let Some(fill)=&fill_preview {
            let mut scene=self.scene.take().map(Ok).unwrap_or_else(||scene::Scene::new(self))?;
            let result=scene.apply_operation(self,packet,fill.target,scene::PaintOperation::Preview(&fill.operation),new_preview_damage,&mut encoder);
            self.scene=Some(scene);
            result?;
        }
        self.fill_preview_shown=fill_preview.as_ref().map(layer_render::FillPreview::key);
        if fill_changed {self.fill_preview_done=None;}
        if preview_is_watercolor { self.canonicalize_preview_watercolor(&mut encoder)?; }
        if let Some(started) = started { cpu_phases[3] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.composition");
        self.telemetry.phase_end(1, encoder.diagnostics());
        self.telemetry.phase_begin(2, &self.device, &self.queue, encoder.diagnostics());
        self.preview_valid = new_preview_layer.is_some()
            && fill_preview.is_none()
            && packet
                .dab_batches
                .iter()
                .filter(|batch| batch.kind == DabBatchKind::Preview)
                .all(|batch| dry_material::dry_flow(&batch.style) && pointwise(&batch.style));

        self.transform_sources_pending = false;
        if let Some(preview) = self.transform_preview.clone().filter(|preview| self.refused_transform.as_ref() != Some(preview)) {
            let level = self.scale_display.as_ref().filter(|cache| cache.evaluation != scene::scale::Evaluation::Native).map(|cache| cache.plan.level).filter(|level| *level > 0);
            let mut transforms = self.transforms.take().expect("retained transform renderer");
            let result = transforms.update_preview(self, &mut encoder, &preview, packet.scene, level);
            self.transforms = Some(transforms);
            self.transform_sources_pending = matches!(&result, Err(error) if error.is_preparation_pending());
            match result {
                Err(GpuRasterError::TransformReach) => {
                    self.metrics.transform_reach_refusals += 1;
                    self.refused_transform = Some(preview);
                }
                Err(error) if error.is_preparation_pending() => {}
                result => {
                    let damage=result?;
                    artwork_damage|=damage.iter().any(|(_,bounds)|!bounds.is_empty());
                    self.transform_damage.extend(damage);
                },
            }
        }
        let mut scene=self.scene.take().map(Ok).unwrap_or_else(||scene::Scene::new(self))?;
        let signatures=(|| {
            let packet=FramePacket {dab_batches:original_batches,..packet};
            scene.prepare_scale_sources(self,packet,&batch_tiles);
            scene.prepare_signatures(self,packet)?;
            scene.prepare_scale_work(self,packet,reset||blending_changed)
        })();
        self.scene=Some(scene);signatures?;
        let local_contacts = !original_batches.is_empty()
            && watercolor_style_dirty.is_empty()
            && original_batches.iter().all(|b| {
                matches!(b.kind, DabBatchKind::Persistent | DabBatchKind::Preview)
                    && (pointwise(&b.style) || b.style.execution == BrushExecution::Liquify)
                    && !b.style.rendering.edge_after_stroke
            });
        let mut changed_cells = self.changed_cells.take().unwrap();
        for &(id, region) in &self.transform_damage { changed_cells.force(id, region); }
        self.changed_cells = Some(changed_cells);
        let canonical_pages = native_commit.as_ref().map_or(&[][..], |frame| &frame.canonical_pages[..]);
        if canonical_pages.iter().any(|(id,_)|!matches!(id,SourceTarget::Paint(_))) {self.changed_cells.as_mut().unwrap().force_all();}
        self.transform_damage.extend(canonical_pages.iter().map(|&(id,coordinate)|(id,page_rect(coordinate))));
        self.transform_damage.sort_unstable_by_key(|&(id,rect)|(id,rect.min_x(),rect.min_y(),rect.max_x(),rect.max_y()));
        self.transform_damage.dedup();
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
            if let Some(id) = old_preview_layer.filter(|_| !reuse_preview) {
                for &coordinate in &old_preview_footprint {
                    if !self.liquify.as_ref().is_some_and(|l| l.preview_unchanged(id, coordinate)) { include(id, page_rect(coordinate)); }
                }
            }
            for (batch, planned) in original_batches.iter().zip(&batch_tiles) {
                if reuse_preview && batch.kind == DabBatchKind::Preview {
                    continue;
                }
                for tile in planned {
                    if batch.kind != DabBatchKind::Preview
                        || !self.liquify.as_ref().is_some_and(|l| l.preview_unchanged(batch.target, tile.coordinate))
                    { include(batch.target, scene::scale::Damage::tile_region(tile)); }
                }
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
        if packet.composite_all || reset {
            dirty = PixelRect::full(packet.document_extent);
        }

        let animated = packet.scene.order().iter().any(|&h| packet.scene.visible(h) && packet.scene.effect(h).is_some_and(|e| e.animated()));
        // Newly populated display pages change visible pixels too, even when
        // the document itself did not change (for example after navigation).
        if !dirty.is_empty() || animated || display_rebuilt || !unchanged || self.objects_runnable()
            || self.scale_display.as_ref().is_some_and(|cache|cache.has_pending_work(self)) {
            let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
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
            let result = if self.transforms.as_ref().is_some_and(|t|t.waiting_for_display_input(self.scale_display.as_ref().and_then(|cache|cache.evaluation.fine_capacity()))) {
                Err(GpuRasterError::NotReady)
            } else {scene.compose(self, packet, dirty, &mut encoder, composite_tiles.as_ref(), &mut work)};
            self.scene = Some(scene);
            let published=match result {
                Err(error) if error.is_preparation_pending()=>PixelRect::EMPTY,
                result=>result?,
            };
            if !published.is_empty() {self.composite_revision=self.composite_revision.wrapping_add(1);}
            self.composite_damage=published;
        }
        let mut navigator = std::mem::take(&mut self.navigator);
        let navigator_revision = navigator.revision;
        let result = navigator.refresh(self, packet, !dirty.is_empty() || animated || preview_changed || motion_changed || !same_frame, &mut encoder, &mut work);
        self.navigator = navigator;
        result?;
        if let Some(transforms)=&mut self.transforms {transforms.finish_frame();}
        let mut refined = false;
        if navigator_revision == self.navigator.revision && dirty.is_empty() && !animated && original_batches.is_empty() && packet.dabs.is_empty()
            && self.background_ready.load(std::sync::atomic::Ordering::Acquire)
            && self.scale_display.as_ref().is_some_and(|cache| cache.has_pending_work(self))
            && self.artwork_frame.as_ref().is_some_and(|old|
                old.view.document_to_surface == packet.view.document_to_surface
                    && old.same_artwork(packet))
        {
            let mut scene = self.scene.take().map(Ok).unwrap_or_else(|| scene::Scene::new(self))?;
            let result = scene.refine_display(self, packet, &mut encoder, &mut work);
            self.scene = Some(scene);
            let changed=match result {Err(error) if error.is_preparation_pending()=>PixelRect::EMPTY,result=>result?};
            if !changed.is_empty() {
                refined = true;
                self.composite_damage = self.composite_damage.union(changed);
                self.composite_revision = self.composite_revision.wrapping_add(1);
            }
        }
        if self.transform_preview.is_none() && !reset && packet.dabs.is_empty()
            && packet.dab_batches.is_empty() && packet.restore_rasters.is_empty()
            && let Some((layer, selection)) = self.moving_pixels.clone()
            && let Some(level) = self.scale_display.as_ref().filter(|cache| cache.evaluation != scene::scale::Evaluation::Native).map(|cache| cache.plan.level).filter(|level| *level > 0)
        {
            let mut transforms = self.transforms.take().unwrap();
            let result = transforms.prepare_standby(self, &mut encoder, packet.scene, layer, &selection, level);
            self.transforms = Some(transforms);
            self.transform_sources_pending = match result {
                Ok(ready) => !ready,
                Err(error) if error.is_preparation_pending() => true,
                Err(error) => return Err(error),
            };
        }
        let frame = Arc::new(artwork::Frame::new(packet, context.clone()));
        let moving = !original_batches.is_empty()
            || animated
            || self.transform_preview.is_some()
            || self.artwork_frame.as_ref().is_none_or(|old| !old.same_artwork(packet));
        self.prefetch_retouch(&frame, moving, &mut encoder)?;
        self.prefetch_regions(&frame,&mut encoder)?;
        self.apply_frame_grants(&mut encoder);
        self.uploads.finish(&encoder);
        if let Some(started) = started { cpu_phases[4] = started.elapsed().as_secs_f64() * 1000.; }
        trace_phase.next(c"capy.publication");
        self.telemetry.phase_end(2, encoder.diagnostics());
        self.telemetry.end(encoder.diagnostics());
        self.metrics.command_passes += encoder.pass_count();
        let trim=!moving && self.artwork_frame.as_ref().is_some_and(|old| old.view==packet.view);
        self.retire_display_resources(&mut encoder, trim);
        if fill_preview.is_some() && self.fill_preview_done.is_none() {
            let done=Arc::new(std::sync::atomic::AtomicBool::new(false));self.fill_preview_done=Some(done.clone());
            let done=CompletionFlag(done);encoder.hold(done);
        }
        #[cfg(test)]
        if std::mem::take(&mut self.test.discard_frame) {drop(encoder);return Ok(());}
        let publication=encoder.submission();
        let submission = encoder.submit(&self.queue)
            .ok_or_else(|| GpuRasterError::WaitFailed("GPU device service retired".into()))?;
        if let Some(liquify)=&mut self.liquify {liquify.publish(packet.scene,packet.dab_batches,&publication);}
        self.region_chunks_submitted();
        self.telemetry.submitted(&self.queue);
        self.last_submission = Some(submission.clone());
        if refined {
            self.hold_background(true);
        }
        if let Some(commit) = native_commit {
            self.finish_native_rasters(commit, true)?;
        }
        if animated || !packet.dabs.is_empty() || artwork_damage || preview_changed || motion_changed || fill_changed
            || self.artwork_frame.as_ref().is_none_or(|old| !old.same_artwork(packet)) {
            self.artwork_revision = self.artwork_revision.wrapping_add(1);
            if let Some(regions) = &mut self.regions { regions.raw.invalidate_tonal(); }
        }
        #[cfg(test)]
        test_support::observe_submitted_artwork(FramePacket {dab_batches:original_batches,..packet}, artwork_revision_before, self.artwork_revision);
        self.submitted_context = Some(context);
        self.artwork_frame = Some(frame);
        self.transform_damage.clear();
        self.document_damage.clear();
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
            let sources = self.source_tiles.borrow();
            performance_trace::counter(c"Capy source resident bytes", sources.gpu_bytes());
            performance_trace::counter(c"Capy source upload limit bytes", sources.upload_limit());
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
    // Family-specific contact geometry and pigment-film bounds.
    invariants: [f32; 4],
}

fn midpoint_contact(mut dab:Dab)->Dab {
    dab.radii=[(dab.previous[0]+dab.radii[0])*0.5,(dab.previous[1]+dab.radii[1])*0.5].map(|v|v.max(0.005));
    let angle=[dab.previous[2]+dab.rotation[0],dab.previous[3]+dab.rotation[1]];
    let length2=angle[0]*angle[0]+angle[1]*angle[1];
    if length2>0.00001 {dab.rotation=angle.map(|v|v/length2.sqrt().max(0.00001));}
    dab.previous=[dab.radii[0],dab.radii[1],dab.rotation[0],dab.rotation[1]];
    dab
}

impl From<Dab> for DabGpu {
    fn from(dab: Dab) -> Self {
        let shape=midpoint_contact(dab);
        let axes=shape.radii;
        let rotation=shape.rotation;
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

    fn brush(extent: [u32; 2], batch: &DabBatch, block: u32, device: &PipelineDevice, paper: ([f32; 4], f32)) -> Self {
        let mut style = Self::for_brush(extent, &batch.style, batch.first_dab, batch.dab_count, device, paper);
        style.canvas_opacity[3] = f32::from(batch.stroke_start);
        style.operation[2] = block;
        if batch
            .style
            .contact
            .is_some_and(|contact| contact.dry.is_some())
            && batch.style.rendering.accumulation == BrushAccumulation::Flow
        {
            style.operation[1] = batch
                .preview_end
                .map_or(0, |index| batch.first_dab + index + 1);
        }
        if stable_blender(&batch.style) { style.transport_b[2] = (batch.stroke_id.0 & 0xffffff) as f32; }
        style
    }

    fn for_brush(extent: [u32; 2], style: &layer_render::DabStyle, first: u32, count: u32, device: &PipelineDevice, (paper, relief): ([f32; 4], f32)) -> Self {
        let grain = style.grain.as_ref();
        let mut result = Self::plain(extent, [0.0; 4], style.opacity);
        result.grain = paper;
        result.flags = [
            f32::from(matches!(style.tip, BrushTip::AnalyticEllipse)),
            f32::from(grain.is_some()),
            relief,
            0.0,
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
            if style.execution == BrushExecution::Dry { 0. } else { style.wet_mix.dilution },
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
            style.liquify.as_ref().and_then(|c| c.amount).unwrap_or(-1.),
            style.deform.momentum,
        ];
        let perceptual = style.blend_space == layer_core::BlendSpace::Perceptual;
        result.render_mode = [
            (blend_code(style.rendering.blend_mode.into(), device, style.blend_space) | u32::from(perceptual) << 8) as f32,
            f32::from(style.rendering.accumulation == BrushAccumulation::Uniform),
            f32::from(style.wet_mix.mix_space as u8),
            style.deform.distortion,
        ];
        if style.execution == BrushExecution::Smudge {
            let blender = style.blender;
            result.deformation = [blender.reach, blender.rate, blender.character, blender.scale];
            let tooth = if destination_material::blender(style).paper { grain.map_or(0., |grain| grain.depth) * relief } else { 0. };
            result.transport_a = [tooth, blender.pressure_gain, blender.engagement, blender.recess];
            result.transport_b = [blender.lift, 0., 0., 0.];
        }
        if style.execution == BrushExecution::Wet {
            result.deformation = [style.wet_mix.stretch, 0., style.wet_mix.refill, style.wet_mix.average];
        }
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
            let paper = contact.paper * grain.map_or(1., |grain| grain.depth) * relief;
            result.contact_a = [1.0, paper, f32::from(contact.dry.is_some()), contact.edge_roughness];
            result.contact_b = [
                contact.edge_scale,
                contact.fibers,
                contact.fiber_strength,
                contact.pooling,
            ];
            result.contact_c = [
                contact.pressure_gain,
                contact.depletion,
                0.,
                f32::from(contact.linear_edge),
            ];
            if let Some(dry) = contact.dry {
                let hardness = 1. - (1. - dry.loading).powi(2);
                result.material_a = [dry.corner, dry.softness.recip(), hardness, 0.];
                result.material_b[3] = 0.015625 * (dry.softness / 0.05).min(1.).powi(2);
                result.contact_c[0] = 1.;
                result.contact_c[2] = dry.relief_offset;
            }
        }
        // This lane is unused by dry and composite shaders and avoids growing
        // every style upload solely for one material-stage lifecycle bit.
        result.color[3] = f32::from(style.alpha_locked);
        if stable_blender(style) {
            result.deformation[3] = style.blender.cell_size;
            result.transport_b[1] = style.diameter;
            result.transport_b[3] = f32::from(style.rendering.edge_after_stroke);
        }
        result
    }
}

/// A blend as `blend_codes.wgsl` encodes it: the `LayerBlend` code in bits 0-7,
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
        LiquifyMode::TwirlCounterclockwise => 1.0,
        LiquifyMode::TwirlClockwise => 2.0,
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

fn create_texture_layout(device: &wgpu::Device, filterable: bool) -> wgpu::BindGroupLayout {
    bindings::layout(device, "layer sampled texture layout", &[
        bindings::texture(0, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, filterable),
        bindings::sampler(1, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
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

fn create_material_layout(device: &PipelineDevice) -> wgpu::BindGroupLayout {
    let stages = wgpu::ShaderStages::COMPUTE;
    let storage = wgpu::BufferBindingType::Storage { read_only: true };
    let entries = (0..12).map(|binding| match binding {
        9 => bindings::buffer(9, stages, storage, false, None),
        4 | 10 | 11 => bindings::float_texture(binding, stages, device),
        _ => bindings::texture(binding, stages, false),
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
    #[cfg(test)] test_support::PassCounts {material_bind_groups:1,..Default::default()}.record();
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
    #[cfg(test)] crate::scene::sources::gpu_object_created();
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
            | if matches!(format, wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::R32Float | wgpu::TextureFormat::Rgba32Uint) {
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
    tier: &Arc<residency::Tier>,
) -> PageSurface {
    let (texture, view) = create_target(device, extent, format, label);
    let charge = Arc::new(tier.charge(texture_bytes(&texture)));
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
        material_output: Default::default(),
        scene_output: Default::default(),
        charge,
    }
}

struct PipelineLayouts<'a> {
    style: &'a wgpu::BindGroupLayout,
    texture: &'a wgpu::BindGroupLayout,
    float_texture: &'a wgpu::BindGroupLayout,
    advanced_texture: &'a wgpu::BindGroupLayout,
    target: &'a wgpu::BindGroupLayout,
    material: &'a wgpu::BindGroupLayout,
    edge: &'a wgpu::BindGroupLayout,
    watercolor: &'a wgpu::BindGroupLayout,
    transport: &'a wgpu::BindGroupLayout,
}

fn export_shader_source(device: &PipelineDevice, space: layer_core::color::RgbSpace) -> Cow<'static, str> {
    compose_wgsl(&[&view_color::shader(device.working_space(), space), device.float_sampling_wgsl(), include_str!("export.wgsl")])
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

fn advanced_brush_shader(device: &PipelineDevice) -> Deferred<wgpu::ShaderModule> {
        let device = device.for_recipe();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer textured dry brush shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                    &working_color::shader(&device),
                    include_str!("brush_types.wgsl"), include_str!("advanced_brush.wgsl"),
                    include_str!("brush_math.wgsl"), include_str!("paper_sample.wgsl"), include_str!("contact_field.wgsl"),
                    include_str!("analytic_coverage.wgsl"), include_str!("brush_coverage.wgsl"),
                    include_str!("contact.wgsl"), include_str!("relief_transfer.wgsl"),include_str!("relief.wgsl"), include_str!("relief_fine.wgsl"),
                    include_str!("selection_clip.wgsl"),
                ])),
            })
        })
}

fn create_pipelines(device: &PipelineDevice, layouts: PipelineLayouts<'_>) -> Pipelines {
    let advanced_brush = advanced_brush_shader(device);
    // Native hosts request this feature only after checking Float32 read/write
    // storage support. Each dry invocation owns exactly one destination texel.
    let material_layouts = [layouts.target, layouts.material, layouts.advanced_texture];
    let outputs = dry_material::output_layouts::<3>(device, false);
    let dry_material = dry_material::Pipelines::new(device, material_layouts,
        device.features().contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES), &outputs);
    let destination_material = destination_material::Pipelines::new(device, material_layouts, outputs);
    let stroke_edge_shader = {
        let device = device.for_recipe();
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
        let device = device.for_recipe();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer live watercolor composite shader"),
                source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[&working_color::shader(&device), include_str!("watercolor_floor.wgsl"), include_str!("watercolor_composite.wgsl")])),
            })
        })
    };
    let watercolor_transport_shader = {
        let device = device.for_recipe();
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
        let device = device.for_recipe();
        Deferred::new(move || {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("layer export shader"),
                source: wgpu::ShaderSource::Wgsl(export_shader_source(&device, layer_core::color::RgbSpace::Srgb)),
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
        bind_group_layouts: &[Some(layouts.float_texture)],
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
        let (device, layout, shader) = (device.for_recipe(), advanced_layout.clone(), advanced_brush.clone());
        Deferred::pipeline(move |mode| {
            brush_pipeline_format_recipe(mode, &device, &layout, &shader, "fragment_main", blend, device.working_format(), label)
        })
    });
    let watercolor_transport = std::array::from_fn(|step| {
        let (device, layout, shader) = (
            device.for_recipe(),
            watercolor_transport_layout.clone(),
            watercolor_transport_shader.clone(),
        );
        Deferred::pipeline(move |mode| {
            mode.compute(&device, &wgpu::ComputePipelineDescriptor {
                label: Some("layer nonlinear watercolor capillary relaxation"),
                layout: Some(&layout), module: &shader, entry_point: Some("compute_main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("TRANSPORT_PASS", step as f64)], ..Default::default()
                },
                cache: None,
            })
        })
    });
    let stroke_edge = {
        let (device, layout, shader) = (
            device.for_recipe(),
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
            device.for_recipe(),
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
            (device.for_recipe(), export_layout.clone(), export_shader.clone());
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
        destination_material,
        direct,
        watercolor_transport,
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
) -> Deferred<wgpu::RenderPipeline> {
    let (device, layout, shader) = (device.for_recipe(), layout.clone(), shader.clone());
    Deferred::pipeline(move |mode| fullscreen_pipeline_recipe(mode, &device, &layout, &shader, fragment_entry, blend, format, label))
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
    1 << display_mips::view_level(document_to_surface, 3).unwrap_or(0).min(2)
}

fn dab_bytes(dabs: &[DabGpu]) -> &[u8] {
    // SAFETY: DabGpu is repr(C), has no padding, and contains only f32 fields.
    unsafe { std::slice::from_raw_parts(dabs.as_ptr().cast::<u8>(), mem::size_of_val(dabs)) }
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
mod canvas_transition_tests;
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
    mod blenders;
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
            diameter: 48.,
            opacity: 1.,
            alpha_locked: false,
            selection: None,
            tip: BrushTip::AnalyticEllipse,
            mode: DabMode::Paint,
            execution,
            grain: None,
            paper: None,
            rendering: BrushRendering::default(),
            wet_mix: BrushWetMix::default(),
            transport: None,
            deform: BrushDeform::default(),
            blender: layer_core::BrushBlender::default(),
            liquify: None,
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
                crate::test_support::submit_ready(r, FramePacket {
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
            .as_chunks::<4>().0.iter()
            .filter(|p| p[0] > 200 && p[1] < 100)
            .count();
        assert!(
            red > 150,
            "small antialiased 6x12 mark must fill the preview, got {red} pixels"
        );
        let colored_rows: Vec<_> = image
            .as_chunks::<{ 32 * 4 }>().0.iter()
            .enumerate()
            .filter(|(_, row)| row.as_chunks::<4>().0.iter().any(|p| p[0] > p[1].saturating_add(5)))
            .map(|(y, _)| y)
            .collect();
        assert!(
            colored_rows.last().unwrap() - colored_rows.first().unwrap() >= 27,
            "crop retains antialias fringe and fills height"
        );
        assert!(image.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        assert_ne!(
            image[0],
            image[4 * 4],
            "checker squares remain visible beside cropped mark"
        );
        assert!(
            preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper))
                .as_chunks::<4>().0.iter()
                .all(|p| p[..3] == [255; 3])
        );
        native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
            layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [1., 0., 0., 1.]).unwrap()));
        document.artwork.occurrences.get_mut(paper).unwrap().opacity = 0.25;
        frame(&mut r, &document, &[], &[]);
        assert!(preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper)).as_chunks::<4>().0.iter().all(|p| *p == [255, 0, 0, 255]), "thumbnail shows the fill content without its layer opacity");
        native_effects::solid_color(&mut document, paper);
        native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
            layer_core::color::RgbColor { rgba: [1., 1., 1., 0.], ..layer_core::color::RgbColor::WHITE }));
        frame(&mut r, &document, &[], &[]);
        let transparent_paper = preview(&mut r, layer_render::ThumbnailTarget::Occurrence(paper));
        assert_eq!(&transparent_paper[..4], [255; 4]);
        assert_eq!(&transparent_paper[4 * 4..][..4], [204, 204, 204, 255]);
        assert_ne!(transparent_paper[0], transparent_paper[4 * 4]);
        assert!(
            transparent_paper
                .as_chunks::<4>().0.iter()
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
    fn zoomed_out_previews_evaluate_paint_in_blocks_no_larger_than_a_surface_pixel() {
        let scaled = |scale: f32| [scale, 0., 0., scale, 0., 0.];
        assert_eq!(preview_block(scaled(1.)), 1);
        assert_eq!(preview_block(scaled(0.6)), 1);
        assert_eq!(preview_block(scaled(0.3)), 2);
        assert_eq!(preview_block(scaled(0.2)), 4);
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
        let idle = renderer.raster_tiers.paint.used();
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
        assert_eq!(renderer.raster_tiers.paint.used(), idle);
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
    pub fn set_browser_raster_decoder(&mut self, worker: BrowserRasterDecoder) { self.papers.set_decoder(worker.clone());self.browser_raster_decoder = Some(worker); }
    #[cfg(target_arch = "wasm32")]
    pub fn set_browser_nearest_coordinate_decoder(&mut self, worker: BrowserNearestCoordinateDecoder) { self.browser_nearest_coordinate_decoder = Some(worker); }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod package_render_tests;

#[cfg(feature = "bake")]
pub fn bundled_preview_webp(width: u32, height: u32, rgba: &[u8]) -> webpkit::Result<Vec<u8>> {
    let image = webpkit::ImageRef::new(webpkit::Dimensions::new(width, height)?, webpkit::PixelLayout::Rgba8, rgba)?;
    webpkit::Encoder::lossy().quality(90).effort(webpkit::Effort::Best).encode_ref(image)
}

#[cfg(test)]
mod dry_preview_tests;

struct CompletionFlag(Arc<std::sync::atomic::AtomicBool>);
impl Drop for CompletionFlag {fn drop(&mut self) {self.0.store(true,std::sync::atomic::Ordering::Release);}}

struct InFlightCompletion(Arc<std::sync::atomic::AtomicU32>);
impl Drop for InFlightCompletion {fn drop(&mut self) {self.0.fetch_sub(1,std::sync::atomic::Ordering::AcqRel);}}
