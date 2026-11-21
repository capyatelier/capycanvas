//! Tiled layer composition with explicit cached image boundaries. Pointwise
//! scratch follows nesting depth. Masks never download or rewrite paint.
use super::*;
pub(crate) mod fill_edges;
pub(crate) use fill_edges::read_bounds as operation_read_bounds;
use layer_core::{SceneView, SourceTarget, LayerKind, RasterOperationKind};
use layer_core::authored::{OccurrenceHandle, MaskUse, CoverageSource};
#[path = "scene_images.rs"]
mod images;
#[path = "filter_previews.rs"]
mod previews;
pub(super) use previews::preview_space;
mod metadata;
mod scratch;
fn omitted_ancestor_masks(scene: SceneView<'_>, layer: OccurrenceHandle) -> impl Iterator<Item = OccurrenceHandle> {
    let parent = matches!(scene.scope(), Some(SceneScope::Members(_))).then(|| scene.parent(layer)).flatten();
    std::iter::successors(parent, move |id| scene.parent(*id)).take_while(move |id| !scene.includes(*id))
        .filter(move |id| scene.mask(*id).is_some_and(|(mask, _)| mask.enabled))
}
fn watercolor_stencil(coordinate:[u32;2])->impl Iterator<Item=Option<[u32;2]>> {
    [[0,0],[-1,0],[1,0],[0,-1],[0,1]].into_iter().map(move |[dx,dy]|coordinate[0].checked_add_signed(dx).zip(coordinate[1].checked_add_signed(dy)).map(|(x,y)|[x,y]))
}
fn scratch_page_bytes(format: wgpu::TextureFormat) -> u64 { u64::from(PAGE_SIZE).pow(2) * u64::from(format.block_copy_size(None).unwrap()) }
fn normal_context(scene: SceneView<'_>, index: OccurrenceHandle) -> bool {
    !scene.clipping_base(index).is_some_and(|base| !scene.includes(base))
        && omitted_ancestor_masks(scene, index).next().is_none()
}
fn normal_paint(scene: SceneView<'_>, index: OccurrenceHandle) -> bool {
    let layer = scene.occurrence(index).unwrap();
    let mask = scene.mask(index).filter(|(mask, _)| mask.enabled);
    layer.kind() == LayerKind::Paint && layer.blend == layer_core::LayerBlend::Normal
        && world_offset(scene, index, false) == [0; 2]
        && mask.is_none_or(|(mask, _)| scene.target_offset(SourceTarget::Coverage(mask.source)) == [0; 2])
}
pub(super) fn composition_pipelines(scene: SceneView<'_>) -> [bool; 2] {
    [scene.targets().any(SourceTarget::is_coverage),
        startup::has_watercolor(scene) || scene.order().iter().copied().filter(|&index| scene.includes(index) && scene.occurrence(index).unwrap().is_artwork()).any(|index|
            scene.effective_clipped(index) || scene.visible(index) && (!normal_context(scene, index)
                || !normal_paint(scene, index) || !scene.attached_effects(index).is_empty()))]
}
pub(super) fn same_metadata(r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
    r.artwork_frame.as_ref().is_some_and(|old| {
        let scene=old.scene.view().with_scope(&old.scope);
        old.blend_space==packet.blend_space && scene.composition().color==packet.scene.composition().color
            && scene.composition().size==packet.scene.composition().size && scene.order()==packet.scene.order()
            && (old.time==packet.time_seconds || !packet.scene.order().iter().any(|h|packet.scene.effect(*h).is_some_and(|e|e.animated())))
            && packet.scene.order().iter().all(|h|metadata::Metadata::new(scene,*h)==metadata::Metadata::new(packet.scene,*h)
                && packet.scene.effect(*h).is_none_or(|_|effects::Gpu::effect_time(r,scene,*h,old.time).to_bits()
                    ==effects::Gpu::effect_time(r,packet.scene,*h,packet.time_seconds).to_bits())
                && scene.paint_source(*h).zip(packet.scene.paint_source(*h)).is_none_or(|(old,new)|
                    old.color_mode==new.color_mode && old.domain==new.domain && old.operations==new.operations))
    })
}
mod stack;
mod fold;
pub(crate) mod windows;
pub(super) use previews::FilterPreviews;
pub(super) mod sources;
pub(crate) mod placement;
mod objects;
mod object_cache;
pub(crate) use object_cache::PASS_TAPS;
mod object_spatial;
mod vectors;
pub(crate) use vectors::{MotionSplit, Prepared, active_split, motion_bounds, motion_split, moved_bounds, paths_bounds, prepared, preview_bounds, preview_damage};
mod bake;
pub(crate) mod bake_preflight;
pub(crate) mod resample;
pub(crate) mod scale;
pub(crate) use scale::{Capture,Declared,Input,SignatureFrame,PreviewDeclaration,PreviewVersion,PreviewStage,PreviewPass,PreviewQuality,PreviewSeed,PreviewStamps};

const FILL_LINES_PIPELINE:usize=9;
const RECORD_BYTES:usize=fold::RECORD_BYTES as usize;
pub(crate) const IDLE_FRAME_TILES:usize=(crate::frame_pace::IDLE_TAPS/PASS_TAPS) as usize;

use crate::residency::Class;
thread_local! {static ATTEMPT_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };}
pub(crate) struct WorkBudget { used: u64, capacity: u64, motion: bool }
impl Drop for WorkBudget {
    fn drop(&mut self) {ATTEMPT_OPEN.with(|open|open.set(false));}
}
impl WorkBudget {
    fn required(&self,class:Class)->bool {class==Class::Authored || class==Class::Visible && !self.motion}
    fn capacity(&self)->u64 {self.capacity}
    pub(crate) fn remaining(&self)->u64 {self.capacity().saturating_sub(self.used)}
    fn allowance(&self,class:Class)->u64 {if self.required(class) {u64::MAX}else {self.remaining()}}
    fn fits(&self,class:Class,progressed:bool,work:u64)->bool {self.required(class) || work<=self.remaining() || !progressed && !(self.motion && class==Class::Fallback)}
    pub(crate) fn spend(&mut self,work:u64) {self.used=self.used.saturating_add(work);}
    fn spent(&self)->u64 {self.used}
}
#[cfg(test)]
impl WorkBudget {
    pub(crate) fn counts(&self)->[u64;2] {[self.capacity,self.used]}
}
const FRAGMENTS: [(&str, &str); 11] = [
    ("fragment_main", "tile layer composition"),
    ("fragment_main", "tile layer over"),
    ("fragment_coverage", "tile mask coverage"),
    ("fragment_fill", "raster fill operation"),
    ("fragment_gradient", "raster gradient operation"),
    ("fragment_figure", "raster figure operation"),
    ("fragment_blend", "tile layer blend"),
    ("fragment_image", "tile image placement"),
    ("fragment_tone", "tile tone mapping"),
    ("fragment_fill_lines", "fill line capture"),
    ("fragment_fill_opaque", "raster opaque fill operation"),
];

fn fragment_pipeline(operation: u32, over: bool, scalar:bool) -> Option<usize> {
    Some(match operation {
        15 => return None,
        6 => 3,
        18 => 4,
        11 => 5,
        21 | 22 => 2,
        12 => 7,
        10 => 8,
        23 if scalar => FILL_LINES_PIPELINE,
        0 | 1 | 2 | 3 | 5 | 13 | 14 | 16 | 17 | 19 | 20 | 23 => usize::from(over),
        4 => 6,
        _ => panic!("Unknown scene draw operation {operation}"),
    })
}

fn fill_line_pipeline(device:&PipelineDevice)->usize {
    fragment_pipeline(23,true,!device.portable_blend()).unwrap()
}

#[derive(Clone)]
pub(super) struct Image {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    plan: display_mips::Plan,
}
impl Image {
    fn new(r: &WgpuRasterizer, plan: display_mips::Plan, label: &'static str) -> Self {
        let (texture, view) = create_color_target(&r.device, plan.size, label);
        Self { texture, view, plan }
    }
    fn with_format(r: &WgpuRasterizer, plan: display_mips::Plan, format: wgpu::TextureFormat) -> Self {
        let texture = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("keyed composite page"),
            size: wgpu::Extent3d {width: plan.size[0], height: plan.size[1], depth_or_array_layers: 1},
            mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {texture, view, plan}
    }
    pub(super) fn bytes(&self) -> u64 { texture_bytes(&self.texture) }
}

struct ColorInput {
    view: wgpu::TextureView,
    lease: Option<sources::Lease>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Reduction { Pages, Phased }
#[derive(Clone)]
enum Job {
    Publish {target:SourceTarget,output:Arc<std::sync::Mutex<Option<resident::Encoding>>>},
    Collection(Box<crate::object_sampling::CollectionPreview>),
    Sampling(Box<objects::SamplingStep>),
    Vector(Box<vectors::VectorJob>),
    PaintBase(Arc<sources::PaintBaseAssembly>),
    Positions(Arc<paint_transform::mesh::MeshGeometry>, [u32; 2]),
    Placement(Box<placement::PlacementJob>),
    Compose {
        format: wgpu::TextureFormat,
        values: [u32; 20],
        output: wgpu::TextureView,
        sources: [wgpu::TextureView; 4],
        _leases: Vec<sources::Lease>,
    },
    Resample { values: [u8; resample::UNIFORM_BYTES as usize], views: [wgpu::TextureView; 2], size: [u32; 2] },
    Transform(Box<paint_transform::PreparedRegion>),
    VectorResample {
        source: wgpu::TextureView, origin: [i64; 2], target: wgpu::TextureView, target_origin: [i64; 2],
        side: u32, size: [u32; 2], inverse: layer_core::authored::Affine64,
    },
    Reduce { output: wgpu::TextureView, sources: [wgpu::TextureView; 4], values: [u32; 20], size: [u32; 2], kernel: Reduction, format: wgpu::TextureFormat },
    DecodedTile(std::sync::Arc<sources::PendingTile>),
    Effect {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 4],
        data: [f32; 32],
        prepared: effects::PreparedEffect,
        // Keep ordinary draw jobs small: only effects carry the mask inputs.
        masks: Box<[wgpu::TextureView; effects::MASK_SLOTS]>,
        source_target: Option<SourceTarget>,
        changed_cells: Option<wgpu::Buffer>,
    },
    Draw {
        target: wgpu::TextureView,
        sources: [wgpu::TextureView; 4],
        data: [f32; 32],
        over: bool,
        clip: Option<PixelRect>,
        source_target: Option<SourceTarget>,
        edges: Option<wgpu::BindGroup>,
        pipeline: Option<usize>,
    },
    Clear(wgpu::TextureView, wgpu::Color),
    Fold(Box<fold::Fold>),
    Watercolor {
        coordinates: (wgpu::BindGroup, u32),
        target: wgpu::TextureView,
        views: Vec<wgpu::TextureView>,
        record: u32,
    },
    Copy {
        source: wgpu::Texture,
        source_origin: [u32; 2],
        destination: wgpu::Texture,
        origin: [u32; 2],
        width: u32,
        height: u32,
    },
}
fn needs_blend(device:&PipelineDevice,job:&Job)->bool {
    device.portable_blend() && matches!(job,Job::Draw {over:true,..}|Job::Watercolor {..})
}
fn portable_draw(device:&PipelineDevice,jobs:&[Job],index:usize)->bool {
    let target=match &jobs[index] {Job::Draw {target,..}|Job::Watercolor {target,..}=>target,_=>return false};
    needs_blend(device,&jobs[index]) && !(index>0 && matches!(&jobs[index-1],Job::Clear(previous,color)
        if previous==target && *color==wgpu::Color::TRANSPARENT))
}
fn bound_fragment(device: &PipelineDevice, jobs: &[Job], index: usize) -> Option<usize> {
    let Job::Draw {target, data, over, pipeline, ..} = &jobs[index] else {return None;};
    if is_compute(&jobs[index]) {return None;}
    let format = if portable_draw(device, jobs, index) {device.working_format()} else {target.texture().format()};
    pipeline.or_else(|| fragment_pipeline(data[8] as u32, *over, format == device.scalar_format()))
}
fn material_compute(jobs:&[Job],index:usize)->bool {
    match &jobs[index] {
        Job::Placement(_) | Job::Reduce {..} | Job::Compose {..} | Job::Resample {..}=>true,
        Job::Watercolor {target,..}=>index>0 && matches!(&jobs[index-1],Job::Clear(previous,color) if previous==target && *color==wgpu::Color::TRANSPARENT),
        _=>false,
    }
}
fn material_run_end(jobs:&[Job],start:usize)->usize {
    (start+1..jobs.len()).find(|&index|match &jobs[index] {
        Job::Publish {..}=>false,
        Job::DecodedTile(pending)=>jobs[start..index].iter().any(|job|match job {
            Job::Placement(job)=>job.reads(pending.texture.raw()),
            Job::Reduce {sources,..}|Job::Compose {sources,..}=>sources.iter().any(|source|source.texture()==pending.texture.raw()),
            Job::Resample {views,..}=>views[0].texture()==pending.texture.raw(),
            _=>false,
        }),
        Job::Clear(..)=>!matches!(jobs.get(index+1),Some(Job::Placement(_) | Job::Watercolor {..})) || !material_compute(jobs,index+1),
        _=>!material_compute(jobs,index),
    }).unwrap_or(jobs.len())
}
impl Job {
    fn work(&self)->u64 {
        let area=|size:[u32;2]|size.into_iter().map(u64::from).product::<u64>();
        let texture=|view:&wgpu::TextureView|area([view.texture().width(),view.texture().height()]);
        match self {
            Self::Collection(job)=>area(job.size).saturating_mul(job.objects.len() as u64),
            Self::Placement(job)=>job.work(),
            Self::Compose {values,..}=>area([values[2],values[3]]),
            Self::Resample {size,..}|Self::VectorResample {size,..}|Self::Reduce {size,..}=>area(*size),
            Self::Draw {target,clip,..}=>clip.map_or_else(||texture(target),PixelRect::area),
            Self::Effect {target,..}|Self::Clear(target,_)|Self::Watercolor {target,..}=>texture(target),
            Self::Fold(fold)=>u64::from(PAGE_SIZE).pow(2)*fold.layers,
            Self::Vector(job)=>texture(&job.output),
            Self::Copy {width,height,..}=>area([*width,*height]),
            Self::Publish {..}=>0,
            Self::DecodedTile(tile) if tile.recorded()=>0,
            Self::DecodedTile(tile)=>texture(&tile.view),
            Self::Sampling(step)=>match step.as_ref() {
                objects::SamplingStep::Begin {size,..}=>area(*size),
                objects::SamplingStep::Contribute {source,..}=>texture(source),
                objects::SamplingStep::Normalize=>object_cache::PASS_TAPS,
            },
            Self::Positions(_,size)=>area(*size),
            Self::PaintBase(assembly)=>if assembly.recorded() {0}else {assembly.sources().map(texture).sum()},
            Self::Transform(_)=>object_cache::PASS_TAPS,
        }
    }
    fn record(&self,r:&WgpuRasterizer,grid:Option<[u32;4]>)->[u8;RECORD_BYTES] {
        let words=|values:&[u32]| {
            let mut record=[0;RECORD_BYTES];
            for (bytes,value) in record.as_chunks_mut::<4>().0.iter_mut().zip(values) {*bytes=value.to_ne_bytes();}
            record
        };
        let settings=|data:&[f32;32],grid:[u32;4]| {
            let mut values=[0;36];
            for (value,data) in values.iter_mut().zip(data) {*value=data.to_bits();}
            values[32..].copy_from_slice(&grid);
            words(&values)
        };
        match self {
            Self::Reduce {values,..}=>words(&scale::RegionRecords::reduction_record(r,*values)),
            Self::Compose {values,..}=>words(values),
            Self::Resample {values,..}=>*values,
            Self::Fold(fold)=>fold.record(),
            Self::Transform(region)=>region.values().copied().unwrap_or([0;RECORD_BYTES]),
            Self::Draw {data,..}|Self::Effect {data,..}=>settings(data,grid.unwrap_or_default()),
            Self::Clear(target,color)=>{
                let size=target.texture().size();
                let [w,h]=[size.width as f32,size.height as f32];
                let mut data=[0.;32];
                data[..8].copy_from_slice(&[0.,0.,w,h,w,h,0.,0.]);
                data[12..16].copy_from_slice(&[color.r,color.g,color.b,color.a].map(|v|v as f32));
                settings(&data,[0;4])
            }
            Self::Publish {..}|Self::Collection(_)|Self::Sampling(_)|Self::Vector(_)|Self::PaintBase(_)|Self::Positions(..)|Self::Placement(_)
                |Self::VectorResample {..}|Self::DecodedTile(_)|Self::Watercolor {..}|Self::Copy {..}=>[0;RECORD_BYTES],
        }
    }
}
/// What a draw does to the color it writes (`scene_space` in scene.wgsl).
/// Layer pixels are linear; a Perceptual composite holds encoded values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Convert {
    None,
    Encode,
    Decode,
    Store,
    DecodeStore,
}
impl Convert {
    fn code(self) -> f32 {
        match self {
            Self::None => 0.,
            Self::Encode => 1.,
            Self::Decode => 2.,
            Self::Store => 3.,
            Self::DecodeStore => 4.,
        }
    }
    /// How the composite of `packet` becomes pixels a layer's pages store:
    /// linear, with coverage within 0–1, which filters that spread coverage
    /// can round past.
    pub(super) fn stored(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::DecodeStore } else { Self::Store }
    }
    /// How a layer's own pixels enter the composite of `packet`.
    pub(super) fn layers(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::Encode } else { Self::None }
    }
    /// How the composite of `packet` becomes linear pixels.
    pub(super) fn linear(packet: FramePacket<'_>) -> Self {
        if packet.blend_space == layer_core::BlendSpace::Perceptual { Self::Decode } else { Self::None }
    }
    /// How the composite of `packet` becomes the input of a filter that reads
    /// values in `space`.
    pub(super) fn filter_input(packet: FramePacket<'_>, space: layer_core::EffectSpace) -> Self {
        if space.encoded(packet.blend_space) { Self::None } else { Self::linear(packet) }
    }
}
/// The premultiplied `color` as the composite of `packet` holds it.
fn composite_color(r: &WgpuRasterizer, packet: FramePacket<'_>, [red, green, blue, alpha]: [f32; 4]) -> wgpu::Color {
    let [red, green, blue, alpha] = packet.blend_space.composite(r.device.working_space(), [red * alpha, green * alpha, blue * alpha, alpha]);
    wgpu::Color { r: f64::from(red), g: f64::from(green), b: f64::from(blue), a: f64::from(alpha) }
}
#[derive(Clone, Copy)]
pub(super) enum Output { Artwork(Option<OccurrenceHandle>), Objects(OccurrenceHandle), EffectInput(OccurrenceHandle), EffectChannels(OccurrenceHandle), EffectComposite(OccurrenceHandle), Source(SourceTarget), MaskImage(SourceTarget), Display }
pub(super) fn raw_output(scope:&SceneScope)->Option<Output> {Some(match scope {
    SceneScope::Raw(target) if target.is_coverage()=>Output::MaskImage(*target),
    SceneScope::Raw(target)=>Output::Source(*target),
    SceneScope::RawObjects(handle)=>Output::Objects(*handle),_=>return None,
})}

#[derive(Clone,Copy)]
pub(crate) struct Usage {pub(crate) working:u64,pub(crate) ram:u64,pub(crate) pinned:u64}
#[derive(Clone)]
pub(crate) struct Tiers {pub(super) display:Arc<crate::residency::Tier>,pub(super) windows:Arc<crate::residency::Tier>,pub(super) retiring:Arc<crate::residency::Tier>,pub(super) gpu:Arc<crate::residency::Tier>,pub(super) ram:Arc<crate::residency::Tier>,pub(super) objects:Arc<crate::residency::Tier>,pub(super) pinned:Arc<crate::residency::Tier>,pub(super) moving:[Arc<crate::residency::Tier>;2],pub(super) decodes:Arc<crate::residency::Tier>,pub(super) schedule:crate::schedule::Schedule}
impl Default for Tiers {
    fn default()->Self {
        let retiring=Arc::new(crate::residency::Tier::default());
        let display=crate::residency::Tier::retiring_into(&retiring);
        Self {windows:crate::residency::Tier::within(&display),retiring,gpu:crate::residency::Tier::within(&display),objects:crate::residency::Tier::within(&display),moving:[crate::residency::Tier::within(&display),Default::default()],
            ram:Default::default(),pinned:Default::default(),decodes:Default::default(),schedule:Default::default(),display}
    }
}
impl Tiers {
    pub(crate) fn usage(&self)->Usage {
        let working=self.gpu.used();
        Usage {working,ram:self.ram.used(),pinned:self.pinned.used()}
    }
}
pub(super) struct Scene {
    tiers: Tiers,
    records_charge: crate::residency::Charge,
    valid: Arc<std::sync::atomic::AtomicBool>,
    submission: Option<submission::Submission>,
    placement: [pixel_transform::PixelTransform; 2],
    positions: paint_transform::mesh::Positions,
    position_key: Option<(layer_core::LayerPlacement, [u32; 2])>,
    mesh_geometry: std::cell::RefCell<Vec<(layer_core::LayerPlacement, Arc<paint_transform::mesh::MeshGeometry>)>>,
    material_coordinates: wgpu::BindGroup,
    material_pages: std::collections::VecDeque<placement::MaterialPage>,
    material_bounds: std::collections::HashMap<SourceTarget, DocRect>,
    placement_display: bool,
    object_display: bool,
    object_live: bool,
    presented_backdrop: Option<OccurrenceHandle>,
    object_query: bool,
    authored: bool,
    scale_sources: scale::Sources,
    placement_frame: Arc<sources::Placements>,
    scale_pages: scale::Pages,
    object_results: object_cache::ObjectCache,
    exact_object_results: object_cache::ObjectCache,
    inline_objects: objects::InlineSampling,
    object_spatial: object_spatial::SpatialIndex,
    vectors: vectors::Vectors,
    scale_commands: Option<scale::RegionRecords>,
    pool: Vec<PageSurface>,
    used: Vec<scratch::Usage>,
    operation_scratch: Vec<scratch::Lease>,
    declaring: bool,
    jobs: Vec<Job>,
    source_jobs: Vec<std::ops::Range<usize>>,
    ordered:bool,
    // Retain table capacity across tile batches, but release resource handles
    // after encoding so these tables cannot pin evicted paint/source pages.
    source_bindings: RecentBindings<(bool,[wgpu::TextureView; 4])>,
    region_bindings: RecentBindings<([wgpu::TextureView; 4],wgpu::TextureView)>,
    watercolor_bindings: RecentBindings<Vec<wgpu::TextureView>>,
    compute_bindings: RecentBindings<[wgpu::TextureView; 4]>,
    output_bindings: RecentBindings<wgpu::TextureView>,
    mask_bindings: RecentBindings<([wgpu::TextureView; effects::MASK_SLOTS],wgpu::Buffer)>,
    effect_outputs: RecentBindings<(usize,[wgpu::TextureView; effects::MASK_SLOTS],wgpu::TextureView)>,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    binding: wgpu::BindGroup,
    reduction_binding: wgpu::BindGroup,
    fold_binding: wgpu::BindGroup,
    fold_bindings: RecentBindings<([wgpu::TextureView; fold::SLOTS],wgpu::Buffer)>,
    stride: usize,
    capacity: usize,
    record_count: usize,
    upload: Vec<u8>,
    pipeline: [Deferred<wgpu::RenderPipeline>; FRAGMENTS.len()],
    pending_pipelines: compile::Needs,
    pub(super) effects: effects::Effects,
    pub effect_passes: u64,
    images: images::ImageStages,
    image_window: Option<DocRect>,
    image_output: Option<PixelRect>,
    image_evaluation_origin: [i64; 2],
    stop_before: Option<OccurrenceHandle>,
    image_damage: Option<scale::Damage>,
    fill_capture: Option<Box<fill_edges::Capture>>,
    edge_binding: wgpu::BindGroup,
}

pub(super) enum PaintOperation<'a> { Stored(usize),Preview(&'a layer_core::RasterOperation) }

/// Immutable device resources, compiled before input is enabled and shared by
/// live composition, captures and recreated scenes. No canvas pixels retained.
#[derive(Clone)]
pub(super) struct Pipelines {
    pub objects: crate::object_sampling::ObjectSampler,
    pub vector: crate::vector::Kernel,
    pub scale: scale::Pipelines,
    pub resample: resample::Resample,
    pub fine_resample: resample::Resample,
    uniforms: wgpu::BindGroupLayout,
    layout: wgpu::BindGroupLayout,
    pub pipeline: [Deferred<wgpu::RenderPipeline>; FRAGMENTS.len()],
    pub source: sources::Pipelines,
    pub constant: (wgpu::BindGroupLayout, wgpu::BindGroupLayout, Deferred<wgpu::ComputePipeline>),
    pub fold: fold::Pipelines,
    edges: wgpu::BindGroupLayout,
}

fn is_compute(job: &Job) -> bool {
    if matches!(job,Job::Fold(_)) {return true;}
    if matches!(job,Job::Effect {prepared:effects::PreparedEffect {pipeline:effects::Pipeline::Compute(_),..},..}) {return true;}
    let Job::Draw { data, over: false, clip, .. } = job else { return false; };
    if data[8] == 15. || (data[8] == 13. && clip.is_some()) { return true; }
    if !matches!(data[8] as u32, 1 | 13 | 14 | 17 | 19) { return false; }
    data[..4].iter().all(|v| v.fract() == 0.) && clip.is_none_or(|clip| {
        clip == PixelRect::new(data[0].clamp(0., data[4]) as u32, data[1].clamp(0., data[5]) as u32,
            (data[0] + data[2]).clamp(0., data[4]) as u32,
            (data[1] + data[3]).clamp(0., data[5]) as u32)
            || (data[0] >= 0. && data[1] >= 0. && [clip.min_x(), clip.min_y()] == [data[0] as u32, data[1] as u32]
                && clip.max_x() as f32 <= data[0] + data[2] && clip.max_y() as f32 <= data[1] + data[3])
    })
}

impl Scene {
    #[cfg(test)]
    pub(crate) fn promote_source(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        tile: crate::source_access::RawTile,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        objects::promote(r, encoder, tile)
    }
    pub(crate) fn display_source(r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        source:&layer_core::color::source::SourceImage,coordinate:[u32;2],input:impl Into<crate::source_access::TileTexture>,output:&wgpu::TextureView,
    )->Result<(),GpuRasterError> {
        let input=&input.into();
        let samples=sources::Samples::for_source(source,coordinate,r.device.working_space()).ok_or(GpuRasterError::InvalidImage)?;
        let mut scene=r.scene.take().map(Ok).unwrap_or_else(||Self::new(r))?;
        let result=(|| {
            let offset=scene.source_uniform(r,encoder,samples.display_parameters())?;
            r.source_tiles.borrow_mut().display(r,encoder,samples,input,output,&scene.binding,offset)
        })();
        r.scene=Some(scene);
        result
    }
    fn source_uniform(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,data:sources::SourceColor)->Result<u32,GpuRasterError> {
        let offset=(self.reserve_records(r,1)*self.stride) as u32;
        let mut bytes=[0;144];
        data.write_uniform(&mut bytes);
        r.uploads.write_at(encoder,&self.buffer,u64::from(offset),&bytes)?;
        Ok(offset)
    }
    fn reserve_records(&mut self,r:&WgpuRasterizer,count:usize)->usize {
        let base=self.record_count;
        self.record_count+=count;
        if self.record_count>self.capacity {
            self.capacity=self.record_count.next_power_of_two();
            self.records_charge.resize((self.capacity*self.stride) as u64+32);
            self.buffer=r.device.create_buffer(&wgpu::BufferDescriptor {
                label:Some("scene records"),size:(self.capacity*self.stride) as u64,
                usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false,
            });
            self.binding=uniform_binding(&r.device,&self.uniforms,&self.buffer);
            self.reduction_binding=scale::RegionRecords::record_binding(r,&self.buffer);
            self.fold_binding=r.scene_pipelines.fold.records(&r.device,&self.buffer);
        }
        base
    }
    fn stage_records(&mut self,r:&WgpuRasterizer,jobs:&[Job],grid:impl Fn(usize)->Option<[u32;4]>) {
        self.upload.resize(jobs.len()*self.stride,0);
        for (i,(slot,job)) in self.upload.chunks_exact_mut(self.stride).zip(jobs).enumerate() {
            slot[..RECORD_BYTES].copy_from_slice(&job.record(r,grid(i)));
        }
    }
    pub(super) fn analysis_changed(&mut self) {
        self.retire_images(|scene| scene.images = images::ImageStages::charged(&scene.tiers.gpu, &scene.tiers.ram));
    }
    fn begin_write(&mut self, r: &WgpuRasterizer, encoder:&mut submission::CommandEncoder) -> Result<crate::submission::CacheWrite, GpuRasterError> {
        self.submission=Some(encoder.submission());
        if !self.valid.load(std::sync::atomic::Ordering::Acquire) {
            let replacement = Self::new(r)?;
            let exact = std::mem::replace(&mut self.exact_object_results,object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
            let display = std::mem::replace(&mut self.object_results,object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
            let objects = std::mem::take(&mut self.object_spatial);
            let pages=std::mem::take(&mut self.scale_pages);
            let sources=std::mem::take(&mut self.scale_sources);
            let query = self.object_query;
            *self = *replacement;
            self.exact_object_results = exact;
            self.object_results = display;
            self.object_spatial = objects;
            self.scale_pages=pages;
            self.scale_sources=sources;
            self.object_query = query;
        }
        Ok(crate::submission::CacheWrite::shared(self.valid.clone()))
    }
    #[cfg(test)]
    pub fn image_pass_pixels(&self) -> u64 {
        self.images.pass_pixels
    }
    #[cfg(test)]
    pub fn image_cache_bytes(&self) -> u64 {
        self.images.storage_bytes()
    }
    pub fn pool_bytes(&self) -> u64 {self.pool.iter().map(|page|page.charge.bytes()).sum()}
    #[cfg(test)]
    pub(crate) fn walked_usage(&self)->Usage {
        let [results_ram,_,results_pinned]=self.object_results.walked_bytes();let [exact_ram,_,exact_pinned]=self.exact_object_results.walked_bytes();
        let working=(self.capacity*self.stride) as u64+32+self.effects.storage_bytes()+self.positions.storage_bytes()
            +self.placement.iter().map(pixel_transform::PixelTransform::storage_bytes).sum::<u64>()+self.images.storage_bytes()
            +self.scale_commands.as_ref().map_or(0,scale::RegionRecords::storage_bytes)+self.vectors.storage_bytes()+self.pool.iter().map(|page|scratch_page_bytes(page.texture.format())).sum::<u64>();
        Usage {working,pinned:results_pinned+exact_pinned,
            ram:self.images.metadata_bytes()+results_ram+exact_ram+self.object_spatial.storage_bytes()-self.object_spatial.key_bytes()+self.object_spatial.walked_key_bytes()}
    }
    #[cfg(test)]
    pub(crate) fn object_gpu(&self)->[u64;2] {[self.tiers.objects.used(),self.object_results.walked_bytes()[1]+self.exact_object_results.walked_bytes()[1]+self.tiers.schedule.reserved()]}
    pub(crate) fn usage(&self)->Usage {self.tiers.usage()}
    pub(crate) fn working_bytes(&self)->u64 {self.usage().working}
    pub(crate) fn store_metadata_bytes(&self)->u64 {self.scale_pages.lock().unwrap().metadata_bytes()}
    pub(crate) fn trim_pages_by(&self,deficit:u64,class:crate::residency::Class) {scale::trim_pages_by(&self.scale_pages,deficit,class);}
    pub(crate) fn release_pages_by(&self,deficit:u64) {scale::release_pages_by(&self.scale_pages,deficit);}
    pub(crate) fn advance_demand_epoch(&self) {self.scale_pages.lock().unwrap().advance_epoch();}
    pub(crate) fn retained_scale_bytes(&self)->[u64;2] {
        let pages=self.scale_pages.lock().unwrap();[pages.bytes(),pages.pinned_bytes()]
    }
    pub(crate) fn retire_victims(&mut self, encoder: &mut crate::submission::CommandEncoder) -> u64 {
        let bytes = self.object_results.retire_evicted() + self.exact_object_results.retire_evicted();
        self.object_results.flush_retiring(encoder);
        self.exact_object_results.flush_retiring(encoder);
        bytes
    }
    pub(super) fn submit_source_uploads(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder, class: Class,
    ) -> Result<(), GpuRasterError> {
        r.metrics.source_upload_submissions += 1;
        if r.snapshot_worker {Self::retire_window(r, encoder, "after source upload")?;} else {Self::submit_chunk(r, encoder, "after source upload")?;}
        if r.upload_window_open(class) {Ok(())} else {Err(GpuRasterError::NotReady)}
    }
    pub(super) fn submit_chunk(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        label: &'static str,
    ) -> Result<(), GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("chunk submission");

        Self::submit_commands(r, encoder, label).map(drop)
            .ok_or_else(|| GpuRasterError::WaitFailed("GPU device service retired".into()))
    }
    pub(super) fn submit_window(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, label: &'static str,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Arc<std::sync::atomic::AtomicBool>, GpuRasterError> {
        let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = crate::CompletionFlag(retired.clone());
        encoder.on_submitted_work_done(move || { drop(flag); if let Some(wake) = wake { wake(); } });
        Self::submit_chunk(r, encoder, label)?;
        Ok(retired)
    }
    pub(super) fn retire_window(r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, label: &'static str) -> Result<(), GpuRasterError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (sender, retired) = std::sync::mpsc::channel();
            Self::submit_window(r, encoder, label, Some(Arc::new(move || { let _ = sender.send(Ok(())); })))?;
            crate::raster::wait_mapping(&r.device, &retired).map_err(GpuRasterError::WaitFailed)
        }
        #[cfg(target_arch = "wasm32")]
        Self::submit_chunk(r, encoder, label)
    }
    pub(super) fn submit_commands(
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        label: &'static str,
    ) -> Option<wgpu::SubmissionIndex> {
        let next = crate::submission::CommandEncoder::new(&r.device,
            &wgpu::CommandEncoderDescriptor { label: Some(label) });
        let previous = std::mem::replace(encoder, next);
        r.uploads.finish(&previous);
        r.metrics.command_passes += previous.pass_count();
        previous.submit(&r.queue)
    }
    pub fn initialize_source_paint(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        self.placement_display=false;
        self.jobs.clear(); self.source_jobs.clear();
        for &handle in scene.order() {
            let Some(target) = scene.source_target(handle) else { continue; };
            if scene.paint_source(handle).is_none_or(|paint| paint.base.is_none()) && r.native_backing(target).is_none() { continue; }
            let pages = r.paint_layers.iter().filter(|stored| stored.id == target)
                .flat_map(|stored| stored.pages.iter().filter(|page| page.primary_needs_clear)
                    .map(|page| page.coordinate)).collect();
            self.copy_source_pages(r, scene, target, pages, false, encoder)?;
        }
        self.encode_jobs(r, encoder)
    }
    pub fn initialize_source_preview(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        damage: PixelRect, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.placement_display=false;
        self.jobs.clear(); self.source_jobs.clear();
        let stored = r.paint_layers.iter().find(|stored| stored.id == target);
        let pages = r.preview_pages.iter().filter(|page| !page_rect(page.coordinate).intersect(damage).is_empty()
                && !stored.is_some_and(|stored| stored.pages.iter().any(|own| own.coordinate == page.coordinate)))
            .map(|page| page.coordinate).collect();
        self.copy_source_pages(r, scene, target, pages, true, encoder)?;
        self.encode_jobs(r, encoder)
    }
    fn copy_source_pages(&mut self, r: &mut WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        pages: Vec<[u32;2]>, preview:bool, encoder:&mut submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        for coordinate in pages {
            let declaration=sources::Declaration::raw(None,None,self.submission.clone());
            let mut accepted=self.accept_traversal(r,Default::default(),declaration,|_,r,accepted|accepted.raw_color(r,target,coordinate).map(drop))?;
            #[cfg(test)]
            let _record=sources::record_reads(&accepted);
            let [read,_]=accepted.raw_color(r,target,coordinate)?;
            let Some(read)=read else {continue;};
            accepted.clone().track(encoder);
            if !read.encode(r,encoder,Class::Authored)? {return Err(GpuRasterError::NotReady);}
            let view=read.tile.view;
            accepted.track(encoder);
            let ticket=preview.then(||r.preview_version(target,RasterPlane::Color,coordinate,PreviewStage::Seed)).transpose()?;
            let expected=match &ticket {Some(ticket)=>ticket.version().clone(),None=>r.raster_version_for(scene,target,RasterPlane::Color,coordinate)?};
            let pages=if preview {&mut r.preview_pages}else {
                &mut r.paint_layers.iter_mut().find(|layer|layer.id==target).ok_or(GpuRasterError::MissingPaintLayer(target))?.pages
            };
            let page=pages.iter_mut().find(|page|page.coordinate==coordinate).ok_or(GpuRasterError::Invariant("prepared copy source pages state is inconsistent"))?;
            let receipt=encoder.submission();
            let write=page.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let output=write.value().view.clone();
            let publication=write.encoded(expected);
            if let Some(ticket)=ticket {page.derived=Some(ticket);}
            let mut data = [0.; 32];
            data[..10].copy_from_slice(&[0., 0., 256., 256., 256., 256., 0., 0., 1., 1.]);
            self.jobs.push(Job::Draw { target: output, sources: [view, r.empty_view.clone(), r.empty_view.clone(), r.empty_paper_view.clone()], data, over: false, clip: None, source_target:None, edges:None,pipeline:None });
            self.publish_surface(target,publication);
        }
        Ok(())
    }
    fn recorded_upload(pending:&sources::PendingTile)->Result<(),GpuRasterError> {
        if pending.recorded() {Ok(())} else {Err(GpuRasterError::Invariant("a source upload reached its batch unrecorded"))}
    }
    pub(crate) fn geometry_bytes(scene: Option<&Self>) -> u64 {
        let buffers = scene.map_or(0, |scene| scene.mesh_geometry.borrow().iter().map(|(_, geometry)| geometry.buffer_bytes()).max().unwrap_or(0));
        if buffers == 0 { 0 } else { buffers.saturating_mul(2) + u64::from(PAGE_SIZE + 2).pow(2) * 16 + 48 }
    }
    fn planned_work(&self,start:usize)->u64 {
        let mut index=start;let mut passes=0u64;
        while index<self.jobs.len() {
            passes+=1;
            if material_compute(&self.jobs,index) {
                let end=material_run_end(&self.jobs,index);
                passes+=self.jobs[index..end].iter().filter(|job|matches!(job,Job::DecodedTile(tile) if !tile.recorded())).count() as u64;
                index=end;
            } else {index+=1;}
        }
        let records=(self.jobs.len()-start) as u64*object_cache::PASS_TAPS/submission::CommandEncoder::PASSES_PER_SUBMISSION as u64;
        self.jobs[start..].iter().map(Job::work).fold(records,u64::saturating_add)
            .saturating_add(passes.saturating_mul(object_cache::PASS_TAPS))
    }
    pub(crate) fn begin_paced(motion:bool,taps:u64)->WorkBudget {
        debug_assert!(!ATTEMPT_OPEN.with(|open|open.replace(true)),"a work attempt is already open on this thread");
        WorkBudget {used:0,capacity:taps.max(PASS_TAPS),motion}
    }
    pub fn begin_frame(&mut self) {
        self.clear_material_pages();
        for pass in &mut self.placement { pass.begin_frame(); }
        self.source_bindings.begin_frame();self.region_bindings.begin_frame();self.watercolor_bindings.begin_frame();
        self.compute_bindings.begin_frame();self.fold_bindings.begin_frame();
        self.output_bindings.begin_frame();
        self.mask_bindings.begin_frame();
        self.effect_outputs.begin_frame();
        self.record_count = 0;
        self.effect_passes = 0;
    }
    pub fn new(r: &WgpuRasterizer) -> Result<Box<Self>, GpuRasterError> {
        let tiers=r.scene_tiers.clone();
        let keys=object_spatial::KeyLeases::charged(&tiers.ram);
        let device = &r.device;
        let Pipelines {
            uniforms,
            layout,
            pipeline,
            ..
        } = r.scene_pipelines.clone();
        let stride = (device.limits().min_uniform_buffer_offset_alignment as usize).max(RECORD_BYTES);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene uniform records"),
            size: stride as u64 * 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding = uniform_binding(device, &uniforms, &buffer);
        let effects = effects::Effects::new(r, &uniforms).charged(&tiers.gpu);
        let coordinates = initialized_buffer(device, &wgpu::util::BufferInitDescriptor {
            label: Some("placed material neighborhood"),
            contents: target_bytes(&TargetGpu::new([PAGE_SIZE; 2], [PAGE_SIZE; 2], [PAGE_SIZE * 3; 2])),
            usage: wgpu::BufferUsages::UNIFORM,
        })?;
        Ok(Box::new(Self {
            valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            submission: None,
            placement_display: false,
            object_display: false,
            object_live: false,
            presented_backdrop: None,
            object_query: false,
            authored: false,
            positions: r.transforms.as_ref().map_or_else(|| paint_transform::mesh::Positions::new(device), paint_transform::mesh::Positions::sharing).charged(&tiers.gpu),
            position_key: None,
            mesh_geometry: Default::default(),
            scale_sources: Default::default(),
            placement_frame: Default::default(),
            scale_pages: r.scene.as_ref().map_or_else(||scale::pages_within(&tiers.display),|scene|scene.scale_pages.clone()),
            object_results: object_cache::ObjectCache::new(&tiers, keys.clone(), true),
            object_spatial: object_spatial::SpatialIndex::charged(keys.clone()),
            vectors: vectors::Vectors::charged(&tiers.gpu),
            exact_object_results: object_cache::ObjectCache::new(&tiers, keys, false),
            inline_objects: Default::default(),
            scale_commands: None,
            placement: std::array::from_fn(|i| r.transforms.as_ref().map_or_else(
                || pixel_transform::PixelTransform::staged(device, i == 1),
                |passes| passes.placement_pass(i == 1),
            ).charged(&tiers.gpu)),
            material_coordinates: create_target_bind_group(device, &r.target_layout, &coordinates, &r.unclipped),
            material_pages: Default::default(),
            material_bounds: Default::default(),
            pool: Vec::new(),
            used: Vec::new(),
            operation_scratch: Vec::new(),
            declaring: false,
            jobs: Vec::new(),
            source_jobs: Vec::new(),ordered:false,
            source_bindings: Default::default(),region_bindings: Default::default(),watercolor_bindings: Default::default(),
            compute_bindings: Default::default(),
            output_bindings: Default::default(),
            mask_bindings: Default::default(),
            effect_outputs: Default::default(),
            layout,
            uniforms,
            reduction_binding: scale::RegionRecords::record_binding(r, &buffer),
            fold_binding: r.scene_pipelines.fold.records(device, &buffer),
            fold_bindings: Default::default(),
            buffer,
            binding,
            stride,
            capacity: 128,
            records_charge: tiers.gpu.charge(stride as u64*128+32),
            record_count: 0,
            upload: Vec::new(),
            pipeline,
            pending_pipelines: Default::default(),
            effects,
            effect_passes: 0,
            images: images::ImageStages::charged(&tiers.gpu, &tiers.ram),
            image_window: None,
            image_output: None,
            image_evaluation_origin: [0; 2],
            stop_before: None,
            image_damage: None,fill_capture:None,edge_binding:fill_edges::binding(r,&r.empty_view,&r.empty_view,&r.empty_view,&r.unclipped,None),
            tiers,
        }))
    }
    pub(super) fn retire_changed_cells(&mut self,buffers:&[wgpu::Buffer]) {
        if buffers.is_empty() {return;}
        self.mask_bindings.forget(|(_,buffer)|buffers.contains(buffer));
    }
    fn forget_bindings(&mut self, retired: &[wgpu::TextureView]) {
        if retired.is_empty() { return; }
        self.source_bindings.forget(|(_,views)| views.iter().any(|view| retired.contains(view)));
        self.compute_bindings.forget(|views| views.iter().any(|view| retired.contains(view)));
        self.output_bindings.forget(|view| retired.contains(view));
        self.mask_bindings.forget(|views| views.0.iter().any(|view| retired.contains(view)));
        self.effect_outputs.forget(|(_,views,output)|retired.contains(output) || views.iter().any(|view|retired.contains(view)));
        self.fold_bindings.forget(|(views,_)|views.iter().any(|view|retired.contains(view)));
    }
    fn alloc(&mut self, r: &WgpuRasterizer, color: wgpu::Color) -> Result<usize, GpuRasterError> {
        let id = self.reserve(r)?;
        self.jobs
            .push(Job::Clear(self.pool[id].view.clone(), color));
        Ok(id)
    }
    fn reserve(&mut self, r: &WgpuRasterizer) -> Result<usize, GpuRasterError> {
        self.reserve_format(r, false)
    }
    fn reserve_format(&mut self, r: &WgpuRasterizer, scalar: bool) -> Result<usize, GpuRasterError> {
        self.reserve_allocation(r,scalar,None)
    }
    fn reserve_with_reserved(&mut self,r:&WgpuRasterizer,scalar:bool,limit:u64,reservation:&mut crate::residency::Reservation)->Result<usize,GpuRasterError> {
        self.reserve_allocation(r,scalar,Some((limit,reservation)))
    }
    fn reserve_allocation(&mut self,r:&WgpuRasterizer,scalar:bool,reserved:Option<(u64,&mut crate::residency::Reservation)>)->Result<usize,GpuRasterError> {
        let format = if scalar { r.device.scalar_format() } else { r.device.working_format() };
        let id = self
            .used
            .iter()
            .enumerate()
            .position(|(i, v)| !v.occupied() && self.pool[i].texture.format() == format)
            .unwrap_or(self.pool.len());
        if id == self.pool.len() {
            let bytes=scratch_page_bytes(format);
            let charge=match reserved {
                Some((limit,_)) if self.pool_bytes().saturating_add(bytes)>limit=>return Err(GpuRasterError::SourceWorkingSetExceeded),
                Some((_,reservation))=>reservation.split(bytes).ok_or(GpuRasterError::Invariant("scratch allocation exceeds its accepted reservation"))?.charge(bytes),
                None if self.declaring=>self.tiers.gpu.charge(bytes),
                None=>r.admit_tier(None,&self.tiers.gpu,crate::residency::Class::Visible,bytes).map_err(|_|GpuRasterError::NotReady)?.charge(bytes),
            };
            let mut page = if self.declaring { r.declared_scratch[usize::from(scalar)].sharing() }
                else if scalar { r.create_scalar_page_surface(&self.tiers.gpu, "scene reusable scalar") } else { r.create_page_surface(&self.tiers.gpu, "scene reusable tile") };
            page.charge=Arc::new(charge);
            self.pool.push(page);
            self.used.push(Default::default());
        }
        self.used[id].reserve();
        Ok(id)
    }
    fn free(&mut self, id: usize) {
        self.used[id].reset();
    }
    fn own(&mut self, id: usize) -> scratch::Lease { self.used[id].lease(id) }
    fn hold_operation_scratch(&mut self,r:&WgpuRasterizer,pages:usize)->Result<(),GpuRasterError> {
        for _ in self.operation_scratch.len()..pages {let page=self.reserve(r)?;let lease=self.own(page);self.operation_scratch.push(lease);}
        Ok(())
    }
    pub(crate) fn hold_settling_scratch(&mut self,r:&WgpuRasterizer,target:SourceTarget,kind:&RasterOperationKind,damage:PixelRect,extent:[u32;2],
        outputs:impl Iterator<Item=[u32;2]>)->Result<(),GpuRasterError> {
        if !settles(kind,damage,extent) || !r.paint_layers.iter().any(|layer|layer.id==target && layer.watercolor.is_some()) {return Ok(());}
        let mut pages:Vec<_>=outputs.filter(|page|!damage.page_local(*page).is_empty()).collect();
        self.hold_operation_scratch(r,scratch::settle(&mut pages))
    }
    pub(crate) fn release_operation_scratch(&mut self) {self.operation_scratch.clear();}
    fn keep(&mut self, lease: scratch::Lease) -> usize { self.used[lease.index].keep(lease) }
    pub(super) fn trim_scratch(&mut self, keep: usize) -> Vec<PageSurface> {
        if !self.jobs.is_empty() { return Vec::new(); }
        let mut retired = Vec::new();
        while self.pool.len() > keep && self.used.last().is_some_and(|usage| !usage.occupied()) {
            retired.push(self.pool.pop().unwrap());
            self.used.pop();
        }
        let views: Vec<_> = retired.iter().map(|page| page.view.clone()).collect();
        self.forget_bindings(&views);
        retired
    }
    fn enqueue_source_decode(&mut self, pending: Arc<sources::PendingTile>) {
        if pending.recorded() || self.jobs.iter().any(|job|matches!(job,Job::DecodedTile(input) if Arc::ptr_eq(input,&pending))) {return;}
        if let Some(dependency)=pending.dependency() {self.enqueue_source_decode(dependency.clone());}
        // Decoding writes only the separate source cache. Keep an adjacent
        // scratch clear beside its first draw so both share one render pass.
        let index = self.source_position();
        self.jobs.insert(index, Job::DecodedTile(pending));
    }
    fn source_position(&self)->usize {
        let n=self.jobs.len();
        match &self.jobs[n.saturating_sub(2)..] {
            [Job::Clear(cleared,_),Job::Fold(fold)] if fold.absorbs(cleared)=>n-2,
            [..,Job::Clear(..)|Job::Fold(_)]=>n-1,
            _=>n,
        }
    }
    fn read_input(&mut self,read:source_access::SourceRead)->wgpu::TextureView {
        for pending in read.pending {self.enqueue_source_decode(pending);}
        if let Some(assembly)=read.assembly {let index=self.source_position();self.jobs.insert(index,Job::PaintBase(assembly));}
        read.tile.view
    }
    pub(super) fn declare(&mut self,r:&mut WgpuRasterizer,inputs:sources::Inputs,declaration:sources::Declaration,
        traverse:impl FnOnce(&mut Self,&mut WgpuRasterizer,&mut sources::AcceptedInputs)->Result<(),GpuRasterError>,
    )->Result<sources::Inputs,GpuRasterError> {
        #[cfg(test)]
        crate::test_support::PassCounts {declarations:1,..Default::default()}.record();
        let jobs=std::mem::take(&mut self.jobs);
        let source_jobs=std::mem::take(&mut self.source_jobs);
        let (used,position,metrics)=(self.used.clone(),self.position_key.clone(),r.metrics.clone());
        let material=std::mem::take(&mut self.material_pages);
        let declaring=std::mem::replace(&mut self.declaring,true);
        let result=sources::declare(inputs,declaration,|accepted|traverse(self,r,accepted));
        self.declaring=declaring;
        self.jobs=jobs;self.source_jobs=source_jobs;
        self.pool.truncate(used.len());
        self.used=used;
        self.position_key=position;self.material_pages=material;r.metrics=metrics;
        result
    }
    pub(super) fn accept_traversal(&mut self,r:&mut WgpuRasterizer,inputs:sources::Inputs,declaration:sources::Declaration,
        traverse:impl FnOnce(&mut Self,&mut WgpuRasterizer,&mut sources::AcceptedInputs)->Result<(),GpuRasterError>,
    )->Result<sources::AcceptedInputs,GpuRasterError> {
        self.declare(r,inputs,declaration,traverse)?.accept_query(r,Some(self))
    }
    pub(super) fn accept_capture(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,output:Option<Output>,declaration:&sources::Declaration,
        traverse:impl FnOnce(&mut Self,&mut WgpuRasterizer,&mut sources::AcceptedInputs)->Result<(),GpuRasterError>,
    )->Result<sources::AcceptedInputs,GpuRasterError> {
        let placements=output.map(|output|self.source_placements(packet,output)).transpose()?.unwrap_or_default();
        self.accept_traversal(r,placements,declaration.clone(),traverse)
    }
    pub(super) fn capture_declaration(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<sources::Declaration,GpuRasterError> {
        let frame=Arc::new(SignatureFrame::capture(r,packet,if self.authored {Declared::default()} else {Declared::live(r)})?);
        Ok(if self.authored {sources::Declaration::authored(frame,self.submission.clone())} else {sources::Declaration::scene(frame,self.submission.clone())})
    }
    /// Consume a bounded group before gathering the next one: these textures
    /// belong to the fixed, queue-ordered source cache.
    pub fn layer_tile_for_query(&mut self, r: &mut WgpuRasterizer, packet:FramePacket<'_>, target: SourceTarget,
        coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<crate::source_access::RawTile, GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        self.used.iter_mut().for_each(scratch::Usage::reset); self.begin_frame();
        let scene=packet.scene;
        let extent = scene.target_extent(target);
        let placement=self.raw_placement(Default::default(),PixelRect::full(extent),scene.target_offset(target));
        let read=|this:&mut Self,r:&WgpuRasterizer,accepted:&mut sources::AcceptedInputs|match target {
            SourceTarget::Coverage(coverage) => {
                let owner = scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                let (use_, source) = scene.mask(owner).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                debug_assert_eq!(use_.source, coverage);
                this.mask_at(r, use_, source, scene.target_offset(target), coordinate, accepted)
            }
            _ => this.placed_raw_plane(r, scene, target, &placement, coordinate, RasterPlane::Color, accepted),
        };
        let declaration=self.capture_declaration(r,packet)?;
        let mut accepted=self.accept_capture(r,packet,None,&declaration,|this,r,accepted|read(this,r,accepted).map(drop))?;
        #[cfg(test)]
        let _record=sources::record_reads(&accepted);
        let page = read(self,r,&mut accepted)?;
        let _page = self.own(page);
        accepted.clone().track(encoder);self.encode_jobs(r, encoder)?;accepted.track(encoder);
        Ok(crate::source_access::RawTile { texture: self.pool[page].texture.clone().into(), view: self.pool[page].view.clone() })
    }

    pub(super) fn paint_base_tile_for_query(&mut self, r: &mut WgpuRasterizer, base: &layer_core::authored::PaintBase,
        coordinate: [u32; 2], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<crate::source_access::RawTile>, GpuRasterError> {
        let mut requested=sources::Inputs::default();requested.base(base,coordinate,r.document_color().space);
        let inputs=requested.accept_query(r,Some(self))?;
        if !inputs.prepare(r)? {return Err(GpuRasterError::NotReady);}
        #[cfg(test)]
        let _record=sources::record_reads(&inputs);
        let Some(read)=sources::accepted_base(&inputs,base,coordinate)? else {return Ok(None);};
        r.source_tiles.get_mut().flush_retired(encoder);
        inputs.clone().track(encoder);
        if !read.encode(r,encoder,Class::Authored)? {return Err(GpuRasterError::NotReady);}
        inputs.track(encoder);Ok(Some(read.tile))
    }
    pub fn prepare_native_transfer(&mut self, r: &WgpuRasterizer, space: layer_core::color::RgbSpace) -> Result<crate::native_tiles::NativeTransfer, GpuRasterError> {
        r.source_tiles.borrow_mut().prepare_transfer(&r.device, space)
    }
    pub fn restore_native_tiles(&mut self,r:&mut WgpuRasterizer,requests:&[crate::native_tiles::NativeTileRestore<'_>],inputs:&sources::AcceptedInputs,encoder:&mut crate::submission::CommandEncoder)->Result<(),GpuRasterError> {
        debug_assert!(self.jobs.is_empty());
        #[cfg(test)]
        let _record=sources::record_reads(inputs);
        for request in requests {
            let read=sources::accepted_raster(inputs,request.blob,request.space,request.destination)?;
            if !read.encode(r,encoder,Class::Authored)? {return Err(GpuRasterError::NotReady);}
            encoder.copy_texture_to_texture(read.tile.texture.as_image_copy(),request.working.as_image_copy(),request.working.size());
        }
        Ok(())
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn restore_native_scalars(
        &mut self,
        r: &mut WgpuRasterizer,
        requests: &[crate::native_tiles::scalar::NativeScalarRestore<'_>],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if !r.snapshot_worker {return Err(GpuRasterError::NotReady);}
        for request in requests {
            if r.source_tiles.borrow().uploads_full(Class::Authored) {
                Self::submit_source_uploads(r, encoder, Class::Authored)?;
            }
            let samples = request.blob.decode_on_worker().map_err(GpuRasterError::Color)?;
            crate::native_tiles::scalar::upload_coverage(r, encoder, request.working, request.blob.descriptor.depth(), &samples, 1, Class::Authored)?;
        }
        Ok(())
    }
    pub fn raster_tile_for_query(&mut self, r: &mut WgpuRasterizer, blob: &std::sync::Arc<layer_core::raster::TileBlob>, space: layer_core::color::RgbSpace, encoder: &mut crate::submission::CommandEncoder) -> Result<crate::source_access::RawTile, GpuRasterError> {
        let mut requested=sources::Inputs::default();requested.raster(blob.clone(),space,r.document_color().space);
        let inputs=requested.accept_query(r,Some(self))?;
        if !inputs.prepare(r)? {return Err(GpuRasterError::NotReady);}
        #[cfg(test)]
        let _record=sources::record_reads(&inputs);
        let read=sources::accepted_raster(&inputs,blob,space,r.document_color().space)?;
        r.source_tiles.get_mut().flush_retired(encoder);
        inputs.clone().track(encoder);
        if !read.encode(r,encoder,Class::Authored)? {return Err(GpuRasterError::NotReady);}
        inputs.track(encoder);Ok(read.tile)
    }

    fn watercolor_nearby(r: &WgpuRasterizer, packet: FramePacket<'_>, target: SourceTarget,
        stored: Option<&PaintLayer>, c: [u32; 2], preview: bool,
    ) -> bool {
        stored.is_some_and(|_| r.watercolor_style(target, packet.dab_batches).is_some()
            && (r.raster_versions(packet.scene,target).map_or(true,|versions| (c[1].saturating_sub(1)..=c[1]+1)
                .flat_map(|y|(c[0].saturating_sub(1)..=c[0]+1).map(move|x|[x,y]))
                .any(|coordinate|versions.authored(layer_core::raster::RasterPlane::WatercolorWetness,coordinate)))
                || preview && r.preview_watercolor_wetness_pages.iter()
                    .any(|page|page.coordinate[0].abs_diff(c[0])<=1 && page.coordinate[1].abs_diff(c[1])<=1)))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_page(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        handle: OccurrenceHandle,
        stored: Option<&PaintLayer>,
        c: [u32; 2],
        out: usize,
        rect: [f32; 4],
        convert: Convert,
     accepted:&mut sources::AcceptedInputs) -> Result<(), GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
        let preview = r.preview_layer_id == Some(target)
            && !r.preview_damage.intersect(page_rect(c)).is_empty();
        if Self::watercolor_nearby(r, packet, target, stored, c, preview) {
            let views = self.watercolor_views(r,packet.scene,target,c,accepted)?;
            // Aligned pages already cover the layer tile. Only a translated
            // or converted page needs an intermediate and placement.
            let page = if rect == [0., 0., 256., 256.] && convert == Convert::None {
                out
            } else {
                self.alloc(r, wgpu::Color::TRANSPARENT)?
            };
            let _page = self.own(page);
            self.jobs.push(Job::Watercolor {
                coordinates: (r.target_bind_group.clone(), r.layer_target_offset(target, c)),
                target: self.pool[page].view.clone(),
                views,
                record: *r.layer_style_records.get(&handle).ok_or(GpuRasterError::MissingPaintLayer(target))?,
            });
            if page != out {
                self.draw(
                    r,
                    out,
                    self.pool[page].view.clone(),
                    None,
                    rect,
                    [1., 1., 0., 0.],
                    true,
                    convert,
                );
                self.free(page);
            }
        } else {
            let [base, flow] = self.color_inputs(r,packet.scene,target,c,accepted)?.map(|input| input.map(|i| i.view));
            if preview && r.preview_contribution && let Some(flow)=flow.as_ref() {
                self.draw(r,out,flow.clone(),base,rect,[19.,1.,f32::from(packet.blend_space==layer_core::BlendSpace::Perceptual),0.],true,convert);
                return Ok(());
            }
            match (base, flow) {
                (Some(base), Some(flow)) if convert != Convert::None => {
                    self.draw(r, out, flow, Some(base), rect, [14., 1., 0., 0.], true, convert);
                }
                (base, flow) => {
                    for view in base.into_iter().chain(flow) {
                        self.draw(r, out, view, None, rect, [1., 1., 0., 0.], true, convert);
                    }
                }
            }
        }
        Ok(())
    }
    fn color_inputs(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,c:[u32;2],accepted:&mut sources::AcceptedInputs)->Result<[Option<ColorInput>;2],GpuRasterError> {
        Ok(accepted.color(r,scene,target,c)?.map(|read|read.map(|read|ColorInput {view:self.read_input(read),lease:None})))
    }
    fn scalar_input(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,plane:RasterPlane,c:[u32;2], accepted:&mut sources::AcceptedInputs)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        let read=accepted.scalar(r,scene,target,plane,c)?;
        Ok(read.map(|read|self.read_input(read)))
    }
    fn mask_input(&mut self,r:&WgpuRasterizer,target:SourceTarget,source:&CoverageSource,c:[u32;2],accepted:&mut sources::AcceptedInputs)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        let read=accepted.mask(r,target,source,c)?;
        Ok(read.map(|read|self.read_input(read)))
    }
    fn watercolor_views(&mut self,r:&WgpuRasterizer,scene:SceneView<'_>,target:SourceTarget,coordinate:[u32;2],accepted:&mut sources::AcceptedInputs)->Result<Vec<wgpu::TextureView>,GpuRasterError> {
        let mut views=Vec::with_capacity(10);
        for plane in [RasterPlane::Color,RasterPlane::WatercolorWetness] {
            for coordinate in watercolor_stencil(coordinate) {
                let read=coordinate.map(|c|if plane==RasterPlane::Color {accepted.color(r,scene,target,c).map(|[base,overlay]|overlay.or(base))}else {accepted.scalar(r,scene,target,plane,c)}).transpose()?.flatten();
                views.push(read.map_or_else(||if plane==RasterPlane::Color {r.empty_view.clone()}else {r.empty_scalar_view.clone()},|read|self.read_input(read)));
            }
        }
        Ok(views)
    }

    fn effect(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        indices: &[OccurrenceHandle],
        tile: [u32; 2],
        input: usize,
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let _input = self.own(input);
        let handle = indices[0];
        if packet.scene.effect(handle).unwrap().program.image_boundary() {
            let image = self.images.output(handle).ok_or_else(|| GpuRasterError::Effect("Missing image effect stage".into()))?;
            let bounds=image.plan.doc_bounds.translated(self.images.doc_bounds.min);
            let out = self.image_tile(r, image.view, bounds, tile)?;
            self.free(input);
            return Ok(out);
        }
        let mut waiting=false;
        let prepared = match self.effects.prepare(r, packet.scene, indices, effects::Execution::Fused, packet.time_seconds, 0, packet.blend_space) {
            Ok(prepared)=>Some(prepared),
            Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>{waiting=true;None},
            Err(error)=>return Err(error),
        };
        let indirect_mask=indices.len()==1 && !direct_effect_mask(packet.scene,handle);
        let mask = if indirect_mask {
            packet.scene.mask(handle).filter(|(use_, _)| use_.enabled).map(|(use_, source)| {
                self.mask_at(r, use_, source, packet.scene.target_offset(SourceTarget::Coverage(use_.source)), tile, accepted)
            }).transpose()
        } else { Ok(None) };
        let mask=match mask {
            Ok(mask)=>mask,
            Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>{waiting=true;None},
            Err(error)=>return Err(error),
        };
        let _mask = mask.map(|mask| self.own(mask));
        let mut masks = Box::new(std::array::from_fn(|_| r.empty_view.clone()));
        let mut present = 0u32;
        let mut inverted = 0u32;
        if !indirect_mask {
            for (i, handle) in indices.iter().enumerate().take(effects::MASK_SLOTS) {
                if let Some((m, source)) = packet.scene.mask(*handle).filter(|(m, _)| m.enabled) {
                    let view=match self.mask_page(r,SourceTarget::Coverage(m.source),source,tile, accepted) {
                        Ok(view)=>view,
                        Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>{waiting=true;None},
                        Err(error)=>return Err(error),
                    };
                    if let Some(view)=view {
                        masks[i] = view;
                        present |= 1 << i;
                        if m.inverted { inverted |= 1 << i; }
                    }
                }
            }
        }
        if waiting {return Err(GpuRasterError::NotReady);}
        let prepared=prepared.unwrap();
        let out = self.reserve(r)?;
        let held_out = self.own(out);
        let mut data = [0.; 32];
        data[..4].copy_from_slice(&[0., 0., 256., 256.]);
        data[4..6].copy_from_slice(&[256., 256.]);
        data[11] = f32::from(mask.is_some());
        data[6] = present as f32;
        data[7] = inverted as f32;
        data[12..16].copy_from_slice(&[
            (tile[0] * PAGE_SIZE) as f32,
            (tile[1] * PAGE_SIZE) as f32,
            packet.document_extent[0] as f32,
            packet.document_extent[1] as f32,
        ]);
        let mut source_target=None;
        let mut sources = [
            self.pool[input].view.clone(),
            mask.map_or_else(|| r.empty_view.clone(), |m| self.pool[m].view.clone()),
            r.empty_view.clone(),
            r.empty_paper_view.clone(),
        ];
        // Lower a source-over operation followed by an adjustment into one
        // shader invocation. No intermediate color tile or pass is necessary.
        // This is program-independent; masks and nonlocal operations delimit it.
        if mask.is_none() && self.jobs.len() >= 2 {
            let n = self.jobs.len();
            if let (
                Job::Clear(clear, bg),
                Job::Draw {
                    target,
                    sources: paint,
                    data: paint_data,
                    over,
                    source_target: ownership,
                    ..
                },
            ) = (&self.jobs[n - 2], &self.jobs[n - 1])
                && *clear == self.pool[input].view
                && target == clear
                && paint_data[..4] == [0., 0., 256., 256.]
                && paint_data[31] == Convert::layers(packet).code()
                && ((*over && paint_data[8] == 1.)
                    || (!*over && (paint_data[8] == 13. || paint_data[8]==19.)))
            {
                sources = paint.clone();
                source_target = *ownership;
                data[16..20].copy_from_slice(&[paint_data[9], 1., 0., if paint_data[8]==19. {3.} else {1.}]);
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.truncate(n - 2);
            }
        }
        self.jobs.push(Job::Effect {
            target: self.pool[out].view.clone(),
            sources,
            data,
            prepared,
            masks, source_target, changed_cells:None,
        });
        self.free(input);
        if let Some(m) = mask {
            self.free(m);
        }
        Ok(self.keep(held_out))
    }
    #[allow(clippy::too_many_arguments)] // Explicit tile draw operands.
    fn draw(
        &mut self,
        r: &WgpuRasterizer,
        target: usize,
        source: wgpu::TextureView,
        back: Option<wgpu::TextureView>,
        rect: [f32; 4],
        options: [f32; 4],
        mut over: bool,
        convert: Convert,
    ) {
        let mut data = [0.; 32];
        let mut sources = [source, back.unwrap_or_else(|| r.empty_view.clone()), r.empty_view.clone(), r.empty_paper_view.clone()];
        data[..4].copy_from_slice(&rect);
        data[4..8].copy_from_slice(&[256., 256., 0., 0.]);
        data[8..12].copy_from_slice(&options);
        data[31] = convert.code();
        if over && options[0]==19. && matches!(self.jobs.last(),Some(Job::Clear(clear,color)) if *clear==self.pool[target].view && *color==wgpu::Color::TRANSPARENT) {over=false;}
        // A normal draw over a constant clear supplies its own backdrop. Lower
        // this once when creating the job, for both tiled and direct composition.
        if over && options[0] == 1.
            && let Some(Job::Clear(clear, color)) = self.jobs.last()
            && *clear == self.pool[target].view
        {
            data[10]=1.;data[11]=0.;
            data[8] = 13.;
            data[16..20].copy_from_slice(&[
                color.r as f32, color.g as f32, color.b as f32, color.a as f32,
            ]);
            over = false;
        }
        // Two aligned normal layers, including a separate Flow preview, can
        // resolve over their constant backdrop in one destination write. This
        // preserves layer opacity after preview composition and needs neither
        // a scratch color tile nor an attachment destination read.
        if over && rect == [0., 0., 256., 256.]
            && options[0] == 14.
            && let Some(Job::Draw { target: prior_target, sources: prior_sources,
                data: prior, over: false, clip: None, .. }) = self.jobs.last()
            && *prior_target == self.pool[target].view
            && prior[..4] == rect && prior[8] == 13. && prior[31] == data[31]
        {
            sources[2] = prior_sources[0].clone();
            data[8] = 15.;
            data[16] = prior[9] * prior[10];
            data[20..24].copy_from_slice(&prior[16..20]);
            over = false;
            self.jobs.pop();
        }
        self.jobs.push(Job::Draw {
            target: self.pool[target].view.clone(),
            sources,
            data,
            over,
            clip: None, source_target:None, edges:None,pipeline:None,
        });
    }
    #[allow(clippy::too_many_arguments)] // Explicit composite operands.
    fn combine(
        &mut self,
        r: &WgpuRasterizer,
        front: usize,
        back: usize,
        opacity: f32,
        blend: layer_core::LayerBlend,
        clip: bool,
        space: layer_core::BlendSpace,
    ) -> Result<usize, GpuRasterError> {
        let held_front = self.own(front);
        let held_back = self.own(back);
        let n = self.jobs.len();
        // Watercolor already outputs premultiplied source-over. A complete,
        // unmasked layer at full opacity can draw straight onto its backdrop.
        if !clip && opacity == 1. && blend == layer_core::LayerBlend::Normal && n >= 2 {
            let clear = matches!(&self.jobs[n - 2], Job::Clear(view, color)
                if *view == self.pool[front].view && *color == wgpu::Color::TRANSPARENT);
            if clear
                && let Job::Watercolor { target, .. } = &mut self.jobs[n - 1]
                && *target == self.pool[front].view
            {
                *target = self.pool[back].view.clone();
                self.jobs.remove(n - 2);
                self.free(front);
                return Ok(self.keep(held_back));
            }
        }
        // An isolated clipping stack over a constant backdrop needs no color
        // intermediate after its last adjustment. Fold that final composite.
        if !clip && n >= 2 {
            let bg = if let Job::Clear(view, color) = &self.jobs[n - 2] {
                (*view == self.pool[back].view).then_some(*color)
            } else {
                None
            };
            if let Some(bg) = bg
                && let Job::Effect { target, data, .. } = &mut self.jobs[n - 1]
                && *target == self.pool[front].view
                && data[8] == 0.
            {
                data[8] = 1.;
                data[9] = opacity;
                data[10] = crate::blend_code(blend, &r.device, space) as f32;
                if data[19] > 0.5 {
                    data[19] = 2.;
                }
                data[20..24].copy_from_slice(&[bg.r as f32, bg.g as f32, bg.b as f32, bg.a as f32]);
                self.jobs.remove(n - 2);
                self.free(back);
                return Ok(self.keep(held_front));
            }
        }
        if !clip && blend==layer_core::LayerBlend::Normal
            && let Some(Job::Effect {target,sources,data,prepared,..})=self.jobs.last_mut()
            && *target==self.pool[front].view && prepared.pointwise && prepared.original_independent
            && data[8]==0. && data[11]<=0.5 && data[6]==0. && data[19]<=2.5
        {
            sources[1]=self.pool[back].view.clone();
            data[8]=2.;data[9]=opacity;data[10]=crate::blend_code(blend,&r.device,space) as f32;
            self.free(back);
            return Ok(self.keep(held_front));
        }
        if !clip && opacity == 1. && let Some(at) = self.untouched_clear(back) {
            self.jobs.remove(at);
            self.free(back);
            return Ok(self.keep(held_front));
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        self.draw(
            r,
            out,
            self.pool[front].view.clone(),
            Some(self.pool[back].view.clone()),
            [0., 0., 256., 256.],
            [4., opacity, crate::blend_code(blend, &r.device, space) as f32, f32::from(clip)],
            false,
            Convert::None,
        );
        self.free(front);
        self.free(back);
        Ok(out)
    }
    /// The position of the transparent clear `image` still holds when no
    /// later job reads or writes it. An unclipped blend at full opacity over
    /// it returns its source unchanged.
    fn untouched_clear(&self, image: usize) -> Option<usize> {
        let view = &self.pool[image].view;
        let at = self.jobs.iter().rposition(|job| matches!(job, Job::Clear(target, color) if target == view && *color == wgpu::Color::TRANSPARENT))?;
        self.jobs[at + 1..].iter().all(|job| match job {
            Job::Clear(target, _) => target != view,
            Job::Vector(job) => job.output != *view,
            Job::Draw { target, sources, .. } => target != view && !sources.contains(view),
            _ => false,
        }).then_some(at)
    }
    /// `tile` drawn through `convert` into a new tile, or `tile` itself when
    /// nothing converts.
    pub(super) fn converted(&mut self, r: &WgpuRasterizer, tile: usize, convert: Convert) -> Result<usize, GpuRasterError> {
        if convert == Convert::None {
            return Ok(tile);
        }
        if let Some(Job::Fold(fold)) = self.jobs.last_mut() && fold.target == self.pool[tile].view && fold.output_convert == Convert::None {
            fold.output_convert = convert;
            return Ok(tile);
        }
        if let Some(Job::Draw { target, data, over: false, clip: None, .. }) = self.jobs.last_mut()
            && *target == self.pool[tile].view && data[..4] == [0., 0., 256., 256.]
            && matches!(data[8] as u32, 13 | 15 | 19) && data[7] == 0.
        {
            if matches!(data[8] as u32,13|19) && data[31]==0. && data[16..20]==[0.;4]
                && matches!(convert,Convert::Encode | Convert::Decode) {
                data[31]=convert.code();
            } else {data[7] = convert.code();}
            return Ok(tile);
        }
        let _input = self.own(tile);
        let out = self.reserve(r)?;
        self.draw(r, out, self.pool[tile].view.clone(), None, [0., 0., 256., 256.], [1., 1., 0., 0.], false, convert);
        self.free(tile);
        Ok(out)
    }
    fn mask_tile(
        &mut self,
        r: &WgpuRasterizer,
        mask: &MaskUse,
        source: &CoverageSource,
        offset: [i64; 2],
        tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        self.mask_tile_input(r, mask, source, offset, tile, None, accepted)
    }

    fn mask_tile_input(
        &mut self, r: &WgpuRasterizer, mask: &MaskUse, source: &CoverageSource,
        offset: [i64; 2], tile: [u32; 2], command: Option<(layer_masks::CommandCoverage,&layer_core::CoverageSnapshot)>,
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let default = if mask.inverted {
            1. - source.default_coverage
        } else {
            source.default_coverage
        } as f64;
        let out = self.alloc(
            r,
            wgpu::Color {
                r: default,
                g: default,
                b: default,
                a: default,
            },
        )?;
        let held = self.own(out);
        let command_pages = command.into_iter().flat_map(|(key,coverage)| r.layer_masks.command_pages.iter()
            .filter_map(move |((id, c), page)| (*id == key).then_some((coverage,*c,page))));
        let mut ready=std::collections::BTreeSet::new();
        for (coverage,c,page) in command_pages {
            let rect = local_rect(c, offset, tile);
            if !intersects(rect) {
                continue;
            }
            let name=crate::raster::contents::command_mask(source,coverage.selection.as_ref(),c)?;
            let Ok(read)=page.read(name,self.submission.as_ref()) else {continue;};
            ready.insert(c);
            self.draw(
                r,
                out,
                read.value().view.clone(),
                None,
                rect,
                [2., 1., f32::from(mask.inverted), 0.],
                false,
                Convert::None,
            );
        }
        if command.is_none_or(|(_,coverage)|coverage.selection.is_none()) {
            let id=SourceTarget::Coverage(mask.source);
            let origin=std::array::from_fn(|i|i64::from(tile[i]*PAGE_SIZE)-offset[i]);
            let region=DocRect {min:origin,max:origin.map(|v|v+i64::from(PAGE_SIZE))}.in_frame(source.domain);
            for coordinate in page_coordinates(region) {
                if ready.contains(&coordinate) {continue;}
                if let Some(view)=self.mask_input(r,id,source,coordinate,accepted)? {
                    self.draw(r,out,view,None,local_rect(coordinate,offset,tile),[2.,1.,f32::from(mask.inverted),0.],false,Convert::None);
                }
            }
        }
        if let Some((key,coverage))=command && let Some(selection)=&coverage.selection {
            for c in page_coordinates(pixel_rect(selection.bounds(),source.domain)) {
                if intersects(local_rect(c,offset,tile)) && layer_masks::command_page_needed(coverage,key.2,c) && !ready.contains(&c) {
                    return Err(GpuRasterError::NotReady);
                }
            }
        }
        Ok(self.keep(held))
    }
    fn layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: OccurrenceHandle,
        tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let layer = packet.scene.occurrence(index).unwrap();
        let output = (|| Ok::<_,GpuRasterError>(if layer.kind() == LayerKind::Group {
            self.group(r, packet, Some(index), tile, accepted)?
        } else if let Some(color) = packet.scene.effect(index).and_then(|effect| effect.constant_color())
            .filter(|_| (self.image_window.is_none() && packet.scene.evaluation_offset64()==[0.;2]) || constant_frame_tile(packet.scene,tile,self.image_window)) {
            self.alloc(r, composite_color(r, packet, color.linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?))?
        } else if layer.kind() == LayerKind::Effect {
            let input = self.alloc(r, wgpu::Color::TRANSPARENT)?;
            // Generator coverage is applied below with ordinary layer masks.
            self.effect(r, packet, &[index], tile, input, accepted)?
        } else if packet.scene.object_layer(index).is_some() {
            self.object_tile(r, packet, index, tile, accepted)?
        } else if packet.scene.vector_layer(index).is_some() {
            self.vector_tile(r, packet, index, tile)?
        } else {
            let placement=accepted.placement(packet.scene.source_target(index).unwrap_or_default())?;
            let out = self.paint_tile(r, packet, index, &placement, tile, accepted)?;
            self.converted(r, out, Convert::layers(packet))?
        }))();
        let mut waiting=false;
        let mut out=match output {
            Ok(out)=>out,
            Err(error) if error.is_preparation_pending()=>{waiting=true;self.alloc(r,wgpu::Color::TRANSPARENT)?},
            Err(error)=>return Err(error),
        };
        let mut held = self.own(out);
        let mut apply_mask=|scene:&mut Self,result:Result<usize,GpuRasterError>,out:&mut usize,held:&mut scratch::Lease| {
            match result {
                Ok(mask)=>{
                    let _mask=scene.own(mask);
                    if !waiting {*out=scene.masked_image(r,*out,mask)?;*held=scene.own(*out);}
                    Ok(())
                },
                Err(error) if error.is_preparation_pending()=>{waiting=true;Ok(())},
                Err(error)=>Err(error),
            }
        };
        if let Some((mask, source)) = packet.scene.mask(index).filter(|(mask, _)| mask.enabled) {
            let mask = self.mask_at(r, mask, source, packet.scene.target_offset(SourceTarget::Coverage(mask.source)), tile, accepted);
            apply_mask(self,mask,&mut out,&mut held)?;
        }
        if let Some(base) = packet.scene.clipping_base(index).filter(|base| !packet.scene.includes(*base)) {
            let scope = SceneScope::All;
            let scene = packet.scene.with_scope(&scope);
            let mask = if scene.visible(base) { self.layer(r, FramePacket { scene, ..packet }, base, tile, accepted) }
                else { self.alloc(r, wgpu::Color::TRANSPARENT) };
            apply_mask(self,mask,&mut out,&mut held)?;
        }
        for ancestor in omitted_ancestor_masks(packet.scene, index) {
            let (mask, source) = packet.scene.mask(ancestor).unwrap();
            let mask = self.mask_at(r, mask, source, packet.scene.target_offset(SourceTarget::Coverage(mask.source)), tile, accepted);
            apply_mask(self,mask,&mut out,&mut held)?;
        }
        if waiting {return Err(GpuRasterError::NotReady);}
        Ok(self.keep(held))
    }
    fn masked_image(&mut self, r: &WgpuRasterizer, source: usize, mask: usize) -> Result<usize, GpuRasterError> {
        let _source = self.own(source);
        let _mask = self.own(mask);
        let result = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        self.draw(r, result, self.pool[source].view.clone(), Some(self.pool[mask].view.clone()),
            [0., 0., 256., 256.], [3., 1., 0., 0.], false, Convert::None);
        self.free(source);
        self.free(mask);
        Ok(result)
    }
    fn paint_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, index: OccurrenceHandle, placement:&placement::Placement, tile: [u32; 2], accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let target = packet.scene.source_target(index).unwrap_or_default();
        if placement.offset != [0; 2]
                && r.watercolor_style(target, packet.dab_batches).is_some() {
            return self.placed_material_tile(r, packet, index, placement, tile, accepted);
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        let held = self.own(out);
        let offset = placement.offset;
        let stored = r.paint_layers.iter().find(|l| l.id == target);
        if stored.is_some() || packet.scene.paint_source(index).is_some_and(|paint| paint.base.is_some()) {
            // A translated output tile intersects at most four native
            // source tiles. Watercolor samples its halo from their bindings;
            // never scan/expand every page in the layer for every output tile.
            let origin: [i64; 2] = std::array::from_fn(|i| i64::from(tile[i] * PAGE_SIZE) - offset[i]);
            let region = DocRect { min: origin, max: origin.map(|v| v + i64::from(PAGE_SIZE)) }.in_frame(packet.scene.local_extent(index));
            for c in page_coordinates(region) {
                let rect = local_rect(c, offset, tile);
                if !intersects(rect) {
                    continue;
                }
                self.paint_page(r, packet, index, stored, c, out, rect, Convert::None, accepted)?;
            }
        }
        if offset==[0; 2]
            && r.watercolor_style(target,packet.dab_batches).is_none()
            && let Some(Job::Draw {target:output,source_target,..})=self.jobs.last_mut()
            && *output==self.pool[out].view {
                *source_target=Some(target);
            }
        Ok(self.keep(held))
    }
    fn group(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        parent: Option<OccurrenceHandle>,
        tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        stack::tile(self, r, packet, parent, tile,accepted)
    }

    fn draw_normal_layer(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: OccurrenceHandle,
        tile: [u32; 2],
        output: &mut scratch::Lease,
     accepted:&mut sources::AcceptedInputs) -> Result<bool, GpuRasterError> {
        let layer = packet.scene.occurrence(index).unwrap();
        if !normal_context(packet.scene, index) { return Ok(false); }
        if layer.blend == layer_core::LayerBlend::Normal
            && !layer.mask.as_ref().is_some_and(|mask| mask.enabled)
            && let Some(color) = packet.scene.effect(index).and_then(|effect| effect.constant_color())
            && ((self.image_window.is_none() && packet.scene.evaluation_offset64()==[0.;2]) || constant_frame_tile(packet.scene,tile,self.image_window))
            && let Some(Job::Clear(view, back)) = self.jobs.last_mut()
            && *view == self.pool[output.index].view
        {
            let mut color = color.linear_in(r.device.working_space()).map_err(GpuRasterError::Color)?;
            color[3] *= layer.opacity;
            let front = composite_color(r, packet, color);
            let weight = 1. - front.a;
            *back = wgpu::Color { r: front.r + back.r * weight, g: front.g + back.g * weight,
                b: front.b + back.b * weight, a: front.a + back.a * weight };
            return Ok(true);
        }
        let target = packet.scene.source_target(index).unwrap_or_default();
        if !normal_paint(packet.scene, index) || r.watercolor_style(target, packet.dab_batches).is_some() { return Ok(false); }
        let mask = packet.scene.mask(index).filter(|(mask, _)| mask.enabled);
        let [base,overlay]=accepted.native_color(r,packet.scene,target,tile)?.map(|read|read.map(|read|self.read_input(read)));
        if overlay.is_some() && mask.is_some() || accepted.level(target,RasterPlane::Color,tile)!=0 {return Ok(false);}
        if base.is_none() && overlay.is_none() {return Ok(true);}
        let coverage = mask.map_or(1., |(m, source)| if m.inverted { 1. - source.default_coverage } else { source.default_coverage });
        let mask = match mask {
            Some((mask, source)) => self.mask_page(r, SourceTarget::Coverage(mask.source), source, tile, accepted)?.map(|view| (view, mask.inverted)),
            None => None,
        };
        let contribution = r.preview_contribution && packet.blend_space == layer_core::BlendSpace::Perceptual;
        self.fold(r, output, fold::Layer {base, overlay: overlay.map(|view| (view, contribution)), mask, opacity: layer.opacity, coverage}, Convert::layers(packet))?;
        Ok(true)
    }
    fn fold(&mut self, r: &WgpuRasterizer, output: &mut scratch::Lease, layer: fold::Layer, convert: Convert) -> Result<(), GpuRasterError> {
        let view = self.pool[output.index].view.clone();
        if !matches!(self.jobs.last(), Some(Job::Fold(fold)) if fold.extends(&view, &layer, convert)) {
            let transfer = r.source_tiles.borrow_mut().prepare_transfer(&r.device, r.document_color().space)?.table;
            let fold = if let Some(Job::Clear(cleared, color)) = self.jobs.last() && *cleared == view {
                fold::Fold::constant(view, transfer, convert, *color)
            } else {
                let next = self.reserve(r)?;
                let fold = fold::Fold::over(self.pool[next].view.clone(), transfer, convert, view);
                *output = self.own(next);
                fold
            };
            self.jobs.push(Job::Fold(Box::new(fold)));
        }
        let Some(Job::Fold(fold)) = self.jobs.last_mut() else { unreachable!() };
        fold.push(layer);
        Ok(())
    }
    /// Run a pending paint operation over `damage`, the batch's pages. Masks
    /// and erases that cover the whole layer settle watercolor and wet
    /// material first, so hidden pigment cannot bring erased content back.
    pub(crate) fn declare_operation_masks(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,index:u32,
        operation:&layer_core::RasterOperation,damage:PixelRect)->Result<sources::Inputs,GpuRasterError> {
        let declaration=self.capture_declaration(r,packet)?.with_raw_layers();
        let underfill=layer_masks::underfill_coverage(operation);
        self.declare(r,Default::default(),declaration,|scene,r,accepted| {
            for c in page_coordinates(damage) {
                scene.command_mask_at(r,&operation.coverage,c,(target,index,false),accepted)?;
                if let Some(coverage)=&underfill {scene.command_mask_at(r,coverage,c,(target,index,true),accepted)?;}
            }
            Ok(())
        })
    }
    pub fn apply_operation(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        target: SourceTarget,
        operation: PaintOperation<'_>,
        damage: PixelRect,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.submission=Some(encoder.submission());
        self.placement_display=false;
        self.jobs.clear();self.source_jobs.clear();
        self.clear_material_pages();
        self.used.iter_mut().for_each(scratch::Usage::reset);
        let handle = packet.scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        let (operation_index,op,preview)=match operation {
            PaintOperation::Stored(index)=>(index,&packet.scene.operations(target).ok_or(GpuRasterError::MissingPaintLayer(target))?[index],false),
            PaintOperation::Preview(operation)=>(u32::MAX as usize,operation,true),
        };
        let extent = packet.scene.target_extent(target);
        match &op.kind {
            RasterOperationKind::Bake { scene, scope, offset } =>
                return self.bake(r, packet, target, operation_index as u32, scene, scope, *offset, damage, &op.coverage, None, encoder),
            RasterOperationKind::FrequencyDetail { scene, scope, offset, low } =>
                return self.bake(r, packet, target, operation_index as u32, scene, scope, *offset, damage, &op.coverage, Some(SourceTarget::Paint(*low)), encoder),
            kind if !reads_coverage_pages(kind) => return Ok(()),
            _ => {}
        }
        if target.is_coverage() {
            let mut pages: Vec<_> = r.layer_masks.pages.iter().filter(|((id, c), _)| *id == target && !damage.page_local(*c).is_empty())
                .map(|((_, c), page)| (*c, page.clone())).collect();
            let default=packet.scene.mask(handle).map_or(1.,|(_,mask)|mask.default_coverage);
            for (coordinate,_) in &pages {r.accepted_raster_output(target,RasterOutput::Mask(*coordinate,default.to_bits()))?;}
            let receipt=encoder.submission();
            let declaration=self.capture_declaration(r,packet)?;
            for (coordinate, page) in &mut pages {
                let command=(target, operation_index as u32, false);
                let mut accepted=self.accept_capture(r,packet,None,&declaration,|this,r,accepted|
                    this.command_mask_at(r,&op.coverage,*coordinate,command,accepted).map(drop))?;
                let intent=r.raster_intent(target,layer_core::raster::RasterPlane::Mask,*coordinate)?;
                let write=page.write(&intent,&receipt)?;
                let mask = self.command_mask_at(r, &op.coverage, *coordinate, command, &mut accepted)?;
                let out = self.reserve_format(r, true)?;
                self.draw(r, out, write.value().view.clone(), Some(mask.view.clone()),
                    [0., 0., 256., 256.], [22., 1., 0., 0.], false, Convert::None);
                self.copy_window_tile(out, &Image { texture: write.value().texture.clone(), view: write.value().view.clone(),
                    plan: display_mips::Plan::window(extent, 0, page_rect(*coordinate).intersect(PixelRect::full(extent))) }, *coordinate);
                self.publish_surface(target,write.encoded(layer_core::raster::ContentName::fresh()));
                accepted.track(encoder);
            }
            self.encode_jobs(r,encoder)?;
            return Ok(());
        }
        let paper = fill_texture(&op.kind).and_then(|texture| texture.paper.as_ref().map(|binding| (texture.strength, binding))).map(|(strength, binding)| {
            r.prepare_paper_binding(binding, startup::BRUSH)?;
            let tile = crate::paper::PaperTile::new(&binding.surface)?;
            let id = tile.id(&binding.surface);
            let view = r.papers.view(&id).ok_or_else(|| GpuRasterError::MissingBrushMask(id.clone()))?.clone();
            Ok::<_, GpuRasterError>((view, tile.grain(binding), strength * r.papers.relief(&id).unwrap()))
        }).transpose()?;
        let edge_settings=match &op.kind {RasterOperationKind::Fill {edges:Some(edges),underfill:Some(_),..}=>Some(edges.as_ref()),_=>None};
        if let Some(edges)=edge_settings {self.prepare_fill_edges(r,packet,target,edges,op,encoder)?;}
        let Some(stored_index) = r.paint_layers.iter().position(|l| l.id == target) else {
            return Ok(());
        };
        let stored=&r.paint_layers[stored_index];
        let settles = settles(&op.kind,damage,extent);
        let mut coordinates: Vec<_> = if preview {&r.preview_pages}else{&stored.pages}
            .iter()
            .filter(|p| !damage.page_local(p.coordinate).is_empty())
            .map(|page|page.coordinate)
            .collect();
        if edge_settings.is_some() {coordinates.sort_unstable_by_key(|coordinate|(coordinate[1],coordinate[0]));}
        for coordinate in &coordinates {
            r.accepted_raster_output(target,if preview {RasterOutput::PreviewColor(*coordinate)}else {RasterOutput::Color(*coordinate)})?;
        }
        let watercolor = stored.watercolor.is_some() && settles;
        let record=watercolor.then(||r.layer_style_records.get(&handle).copied().ok_or(GpuRasterError::MissingPaintLayer(target))).transpose()?;
        let erase = f32::from(matches!(op.kind, RasterOperationKind::Erase { .. }));
        let gradient = if let RasterOperationKind::Gradient {gradient,..}=&op.kind {Some(crate::gradient::texture(r,gradient)?)} else {None};
        let underfill_coverage = layer_masks::underfill_coverage(op);
        let declaration=self.capture_declaration(r,packet)?.with_raw_layers();
        let pages=if watercolor {scratch::settle(&mut coordinates)}else {coordinates.len().min(1)};
        self.hold_operation_scratch(r,pages)?;
        let mut spare=std::mem::take(&mut self.operation_scratch);
        let resolve=record.and_then(|record|spare.pop().map(|page|(page,record)));
        let mut frontier=scratch::Frontier::new(if watercolor {&coordinates}else {&[]});
        let publish=|this:&mut Self,spare:&mut Vec<scratch::Lease>,(output,copy,publication):(scratch::Lease,Option<Job>,resident::Encoding)| {
            this.jobs.extend(copy);this.publish_surface(target,publication);spare.push(output);
        };
        for &c in &coordinates {
            let intent=r.raster_intent(target,RasterPlane::Color,c)?;
            let input=preview.then(||r.preview_expected(target,RasterPlane::Color,c,PreviewStage::BeforeFill)).transpose()?;
            let ticket=preview.then(||r.preview_version(target,RasterPlane::Color,c,PreviewStage::Final)).transpose()?;
            let pages=if preview {&mut r.preview_pages}else {&mut r.paint_layers[stored_index].pages};
            let page=pages.iter_mut().find(|page|page.coordinate==c).ok_or(GpuRasterError::Invariant("prepared apply operation state is inconsistent"))?;
            let receipt=encoder.submission();
            let write=if let Some(input)=input {page.active_mut().revise(&input,&receipt)}else {page.active_mut().write(&intent,&receipt)}
                .ok_or(GpuRasterError::Invariant("prepared apply operation state is inconsistent"))?;
            let source=write.value().view.clone();let destination=write.value().texture.clone();
            let name=ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone());
            let publication=write.encoded(name);
            if let Some(ticket)=ticket {page.derived=Some(ticket);}
            let edge_input=edge_settings.map(|_|self.fill_edge_page(r,packet,target,op,c,encoder)).transpose()?.flatten();
            let inputs=|this:&mut Self,r:&WgpuRasterizer,accepted:&mut sources::AcceptedInputs|->Result<_,GpuRasterError> {
                let mask=this.command_mask_at(r,&op.coverage,c,(target,operation_index as u32,false),accepted)?;
                let underfill=underfill_coverage.as_ref().map(|coverage|this.command_mask_at(r,coverage,c,(target,operation_index as u32,true),accepted)).transpose()?;
                let views=watercolor.then(||this.watercolor_views(r,packet.scene,target,c,accepted)).transpose()?;
                Ok((mask,underfill,views))
            };
            let mut accepted=self.accept_capture(r,packet,None,&declaration,|this,r,accepted|inputs(this,r,accepted).map(drop))?;
            let (mask,underfill,views)=inputs(self,r,&mut accepted)?;
            let resolved=views.zip(resolve.as_ref()).map(|(views,(page,record))| {
                let view=self.pool[page.index].view.clone();
                self.jobs.push(Job::Clear(view.clone(),wgpu::Color::TRANSPARENT));
                self.jobs.push(Job::Watercolor {
                    coordinates: (r.target_bind_group.clone(), r.layer_target_offset(target, c)),
                    target: view.clone(),
                    views,
                    record: *record,
                });
                view
            });
            for output in frontier.read(c) {publish(self,&mut spare,output);}
            let output=spare.pop().ok_or(GpuRasterError::Invariant("operation outputs exceed their admitted scratch"))?;
            let out=output.index;
            self.jobs.push(Job::Clear(self.pool[out].view.clone(),wgpu::Color::TRANSPARENT));
            match op.kind {
                RasterOperationKind::ColorMode(_) | RasterOperationKind::Transform(_) | RasterOperationKind::Bake { .. } | RasterOperationKind::FrequencyDetail { .. }
                    | RasterOperationKind::Coverage => {
                    unreachable!("transforms, bakes and mask coverage run before page operations")
                }
                RasterOperationKind::ApplyMask | RasterOperationKind::Erase { .. } => self.draw(
                    r,
                    out,
                    resolved.unwrap_or(source),
                    Some(mask.view.clone()),
                    [0., 0., 256., 256.],
                    [3., 2., erase, 0.],
                    false,
                    Convert::None,
                ),
                RasterOperationKind::Fill { .. }
                | RasterOperationKind::Gradient { .. }
                | RasterOperationKind::Figure(_) => {
                    let (colors, endpoints, options) = match op.kind {
                        RasterOperationKind::Fill {
                            color,
                            alpha_locked, ..
                        } => ([color; 2], [0.0; 4], [6., 1., f32::from(underfill.is_some()), f32::from(alpha_locked)]),
                        RasterOperationKind::Gradient {start,end,shape,reverse,opacity,alpha_locked,..} => (
                            [[opacity,0.,0.,0.],[0.;4]],
                            [start.x,start.y,end.x,end.y],
                            [18.,crate::gradient::quantum(r.document_color.depth),shape as u8 as f32+4.*f32::from(reverse),f32::from(alpha_locked)],
                        ),
                        RasterOperationKind::Figure(ref f) => (
                            f.colors,
                            [f.start.x, f.start.y, f.end.x, f.end.y],
                            [
                                11.,
                                f.shape as u32 as f32
                                    + 3. * f.paint as u32 as f32
                                    + 16. * f32::from(f.erase),
                                f.width,
                                f32::from(f.alpha_locked),
                            ],
                        ),
                        _ => unreachable!(),
                    };
                    self.draw(
                        r,
                        out,
                        mask.view.clone(),
                        Some(source),
                        [0., 0., 256., 256.],
                        options,
                        false,
                        Convert::None,
                    );
                    if let Some(Job::Draw { data, sources, edges, pipeline, .. }) = self.jobs.last_mut() {
                        *pipeline=operation_pipeline(&op.kind);
                        if let Some(gradient)=&gradient {sources[2]=gradient.clone();}
                        if let Some(underfill)=&underfill {sources[2]=underfill.view.clone();}
                        data[6..8].copy_from_slice(&c.map(|v| (v * PAGE_SIZE) as f32));
                        data[12..16].copy_from_slice(&colors[0]);
                        data[16..20].copy_from_slice(&colors[1]);
                        data[20..24].copy_from_slice(&endpoints);
                        data[24..30].copy_from_slice(&op.placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid paint operation placement"))?.0);
                        if let Some((view, grain, strength)) = &paper {
                            sources[3] = view.clone();
                            data[16..20].copy_from_slice(grain);
                            data[30] = *strength;
                        }
                        if let Some((binding,crop))=edge_input {
                            *edges=Some(binding);data[20..24].copy_from_slice(&crop);
                            data[24..28].copy_from_slice(&[edge_settings.unwrap().tolerance,fill_edges::RADIUS as f32,1.,0.]);
                        }
                    }
                }
            }
            let written = if matches!(op.kind, RasterOperationKind::Fill { .. } | RasterOperationKind::Gradient { .. } | RasterOperationKind::Figure(_)) {
                PixelRect::full(extent).page_local(c)
            } else {
                PixelRect::full([PAGE_SIZE; 2])
            };
            let copy = (!written.is_empty()).then(|| {
                let origin = [written.min_x(), written.min_y()];
                Job::Copy {
                    source: self.pool[out].texture.clone(),
                    source_origin: origin,
                    destination,
                    origin,
                    width: written.width(),
                    height: written.height(),
                }
            });
            if let Some(output)=frontier.hold(c,(output,copy,publication)) {publish(self,&mut spare,output);}
            accepted.track(encoder);
        }
        spare.extend(resolve.map(|(page,_)|page));
        self.operation_scratch=spare;
        self.encode_jobs(r, encoder)?;
        self.fill_capture=None;
        if settles {
            let intents=r.paint_layers[stored_index].watercolor_wetness_pages.iter().map(|page|
                r.raster_intent(target,RasterPlane::WatercolorWetness,page.coordinate)).collect::<Result<Vec<_>,_>>()?;
            let mut written=Vec::with_capacity(intents.len());
            let stored=&mut r.paint_layers[stored_index];
            for (page,intent) in stored.watercolor_wetness_pages.iter_mut().zip(intents) {
                let name=layer_core::raster::ContentName::fresh();let receipt=encoder.submission();
                let active=page.active_secondary;
                let output=page.surface_mut(active).write(&intent,&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                output.value().clear(&r.zero_pages,encoder);written.push(output.encoded(name).publish(&encoder.submission()));
                let output=page.surface_mut(!active).overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                output.value().clear(&r.zero_pages,encoder);output.encoded(name).publish(&encoder.submission());
                page.pending=None;page.derived=None;page.primary_needs_clear=false;page.secondary_needs_clear=false;
            }
            for page in &mut stored.coverage_pages {
                let name=layer_core::raster::ContentName::fresh();let receipt=encoder.submission();
                for surface in [&mut page.primary,&mut page.secondary] {
                    let output=surface.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                    output.value().clear(&r.zero_pages,encoder);output.encoded(name).publish(&encoder.submission());
                }
                page.owner=None;page.state=None;
            }
            stored.watercolor=None;
            for output in written {r.record_raster_write(target,output)?;}
        }
        Ok(())
    }

    pub(super) fn object_content_bounds(layers:SceneView<'_>,owner:OccurrenceHandle)->DocRect {
        object_spatial::SpatialIndex::default().content(layers,owner).map_or_else(DocRect::default,|content|content.bounds_at(0))
    }
    pub(super) fn capture_window(layers: SceneView<'_>, region: PixelRect, extent: [u32; 2]) -> DocRect {
        if matches!(layers.scope(),Some(layer_core::SceneScope::Raw(_) | layer_core::SceneScope::RawObjects(_))) {region.into()}
        else {images::capture_window(layers, region, extent)}
    }
    pub(super) fn cached_capture_window(&self,layers:SceneView<'_>,region:PixelRect)->DocRect {
        images::capture_window_cached(layers,region,Some(&self.object_spatial))
    }
    pub(super) fn window_plan(layers:SceneView<'_>,extent:[u32;2],limit:u64,maximum_side:u32,scene:Option<&Self>)->Result<Option<windows::Plan>,GpuRasterError> {
        windows::Plan::new_cached(layers,extent,limit,maximum_side,scene.map(|scene|&scene.object_spatial))
    }
    pub(super) fn release_capture_window(&mut self, window: DocRect) {
        // The snapshot owner has completed its previous readback. Release job
        // references and old image windows before restoring the next inputs.
        self.jobs.clear();self.source_jobs.clear();
        if self.images.doc_bounds != window {
            self.retire_images(|scene| scene.images = images::ImageStages::charged(&scene.tiers.gpu, &scene.tiers.ram));
        }
    }
    pub(super) fn capture_image_bound(scene: SceneView<'_>, output: PixelRect) -> u64 {
        Self::capture_bound(scene, output, true)
    }
    pub(super) fn capture_plan_bound(r: &WgpuRasterizer, scene: SceneView<'_>, output: PixelRect) -> u64 {
        if !objects::inline_sampling(r) { return Self::capture_image_bound(scene, output); }
        let sampled = scene.order().iter().any(|owner| scene.object_layer(*owner).is_some());
        Self::capture_bound(scene, output, false).saturating_add(if sampled { objects::INLINE_SAMPLING_BYTES } else { 0 })
    }
    fn capture_bound(scene: SceneView<'_>, output: PixelRect, object_pages: bool) -> u64 {
        if matches!(scene.scope(),Some(layer_core::SceneScope::Raw(_))) {return 0;}
        let window=DocRect::from(output);
        let raw = match scene.scope() { Some(layer_core::SceneScope::RawObjects(owner)) => Some(*owner), _ => None };
        let stages=images::has_images(scene).then(||images::stage_regions(scene,window,0,None));
        let objects = scene.order().iter().copied()
            .filter(|owner| object_pages && raw.map_or_else(|| scene.visible(*owner), |raw| raw == *owner) && scene.object_layer(*owner).is_some())
            .map(|owner| Self::object_content_bounds(scene, owner).intersect(stages.as_ref().and_then(|stages|stages.get(&owner)).map_or(window,|regions|regions[0])).aligned(PAGE_SIZE).area().saturating_mul(16))
            .fold(0u64, u64::saturating_add);
        let Some(stages)=stages else {return objects;};
        let scratch=images::scratch_shapes(&stages);
        let mut images = scratch.into_iter().map(|size|size.into_iter().map(u64::from).product::<u64>().saturating_mul(16)).fold(0u64,u64::saturating_add);
        for &handle in scene.order() {
            if !scene.visible(handle) { continue; }
            if scene.effect(handle).is_some_and(|effect| effect.program.image_boundary()) {
                let regions=&stages[&handle];
                let input=regions[0].area().max(1);let output=regions.last().unwrap().area().max(1);
                images=images.saturating_add(input.saturating_add(output.saturating_mul(1+u64::from(scene.mask(handle).is_some_and(|(mask,_)|mask.enabled)))).saturating_mul(16));
            }
        }
        objects.saturating_add(images)
    }

    /// Capture a document-coordinate crop with its finite upstream dependencies.
    /// A larger destination receives the crop in its top-left prefix.
    pub(super) fn capture_region(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        destination: &wgpu::Texture,
        region: PixelRect,
        output: Output,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.capture_query_region(r, packet, destination, region, output, false, encoder)
    }

    #[expect(clippy::too_many_arguments, reason = "Query capture keeps output encoding, preview mode, and destination region explicit")]
    pub(super) fn capture_query_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, preview: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.is_empty() || region.intersect(PixelRect::full(packet.document_extent)) != region
            || destination.width() < region.width() || destination.height() < region.height()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let window = if matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)) {region.into()} else {self.cached_capture_window(packet.scene, region)};
        let dirty = match output { Output::Display => PixelRect::EMPTY, _ => window.in_frame(packet.document_extent) };
        self.object_display = self.object_live || matches!(output, Output::Display);
        let raw = match output { Output::Objects(owner) => Some(owner), _ => None };
        if !matches!(output, Output::Source(_) | Output::MaskImage(_)) && !objects::inline_sampling(r) && !self.prepare_object_pages(r, packet, raw, page_coordinates(window.in_frame(packet.document_extent)))? {
            return Err(GpuRasterError::NotReady);
        }
        self.prepare_region(r, packet, (region, window), dirty, matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)), encoder)?;
        let destination = Image { texture: destination.clone(), view: destination.create_view(&Default::default()),
            plan: display_mips::Plan::window(packet.document_extent, 0, region) };
        let mut accepted=self.declare_capture(r,packet,&destination,&[region],output,None,None)?.accept_query(r,Some(self))?;
        #[cfg(test)]
        let _record=sources::record_reads(&accepted);
        accepted.clone().track(encoder);
        let result=self.capture_prepared_regions(r, packet, &destination, &[region], output, preview, None, encoder, &mut accepted);
        if result.is_ok() {accepted.track(encoder);}else {self.discard_unencoded_jobs();}
        result
    }

    pub(crate) fn capture_region_prepared(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if r.snapshot_worker || !packet.scene.order().iter().any(|owner| packet.scene.object_layer(*owner).is_some()) {
            return self.capture_region(r, packet, destination, region, output, encoder);
        }
        r.drain_image_decodes(encoder)?;
        self.object_results.retain(packet.scene,packet.blend_space,r.device.working_space());
        let mut objects = std::mem::replace(&mut self.object_results,object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
        let advanced = objects.advance_with_memory(r, self, encoder, |_| u64::MAX);
        self.object_results = objects;
        advanced?;
        self.object_query = true;
        let result = self.capture_region(r, packet, destination, region, output, encoder);
        self.object_query = false;
        result
    }
    pub(crate) fn capture_authored_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &wgpu::Texture,
        region: PixelRect, output: Output, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.authored = true;
        let result = self.capture_region_prepared(r, packet, destination, region, output, encoder);
        self.authored = false;
        result
    }

    fn prepare_region(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, (output, window): (PixelRect, DocRect),
        dirty: PixelRect, raw: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.begin_frame();
        self.prepare_region_in_frame(r, packet, (output, window), dirty, raw, encoder)
    }

    fn prepare_region_in_frame(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, (output, window): (PixelRect, DocRect),
        dirty: PixelRect, raw: bool, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let write = self.begin_write(r,encoder)?;
        self.object_spatial.prepare(packet.scene);
        if self.placement_display { self.retire_images(|scene| scene.images = images::ImageStages::charged(&scene.tiers.gpu, &scene.tiers.ram)); }
        self.placement_display = false;
        if let Some(mut transforms) = r.transforms.take() {
            let result = transforms.materialize_region(r, encoder, packet.scene, window);
            r.transforms = Some(transforms);
            result?;
        }
        self.clear_material_pages();
        self.record_count = 0;
        self.effect_passes = 0;
        self.image_window = Some(window);
        self.image_output = Some(output);
        if let Some((_,queue))=&mut r.moving_decode {queue.prioritize(packet.view,output);}
        self.effects.retain(packet.scene);
        let result = if raw {
            self.retire_images(|scene|scene.images=images::ImageStages::charged(&scene.tiers.gpu,&scene.tiers.ram));
            Ok(())
        } else { self.update_images(r, packet, dirty, encoder).map(|_|()) };
        self.stop_before = None;
        if result.is_ok() || matches!(&result, Err(error) if error.is_preparation_pending()) { write.track(encoder); }
        result.map(|_| ())
    }

    pub(crate) fn release_exact_objects(&mut self,encoder:&crate::submission::CommandEncoder) {
        self.exact_object_results.clear();
        self.exact_object_results.flush_retired(encoder);
    }

    pub(crate) async fn prepare_capture_work_async(&mut self,r:&mut WgpuRasterizer)->Result<(),GpuRasterError> {
        r.end_frame_memory();
        if !self.jobs.is_empty() { self.jobs_ready(r,0); }
        r.transforms.as_ref().unwrap().materialization_needs().prepare(&r.compilation).await.map_err(GpuRasterError::Effect)?;
        self.pending_pipelines.prepare(&r.compilation).await.map_err(GpuRasterError::Effect)?;
        self.effects.needs().prepare(&r.compilation).await.map_err(GpuRasterError::Effect)?;
        if self.exact_object_results.pending() {
            let sampler=r.scene_pipelines.objects.clone();
            r.compilation.prepare_all(sampler.pipelines().into_iter().chain([&r.scene_pipelines.constant.2])).await.map_err(GpuRasterError::Effect)?;
        }
        while self.exact_object_results.pending() || r.moving_decode.as_ref().is_some_and(|(_,queue)|queue.pending()) {
            if r.snapshot_cancelled.as_ref().is_some_and(|cancelled|cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
            }
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.drain_image_decodes(&mut encoder)?;
            let mut objects=std::mem::replace(&mut self.exact_object_results,object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
            let result=objects.advance_with_memory(r,self,&mut encoder,|_|crate::frame_pace::IDLE_TAPS);
            self.exact_object_results=objects;
            let idle=result? == 0 && !encoder.has_work();
            r.uploads.finish(&encoder);
            encoder.submit(&r.queue);
            if idle && r.image_decode_waiting() { r.await_image_decode().await?; continue; }
            r.metrics.object_drain_submissions += 1;
            r.await_capture_submission(idle).await?;
        }
        if r.snapshot_cancelled.as_ref().is_some_and(|cancelled|cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
            return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
        }
        Ok(())
    }

    async fn complete_capture_plan(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, grid: Option<[u32;4]>, declaration:&sources::Inputs, accepted:&mut Option<sources::AcceptedInputs>,
        mut plan: impl FnMut(&mut Self, &mut WgpuRasterizer, &mut crate::submission::CommandEncoder,&mut sources::AcceptedInputs) -> Result<(), GpuRasterError>,
    ) -> Result<(), GpuRasterError> {
        let result=async {
        let unit=declaration.unit().ok_or(GpuRasterError::Invariant("capture input unit is missing"))?;
        if accepted.as_ref().is_some_and(|inputs|inputs.unit()!=Some(unit)) {self.discard_unencoded_jobs();}
        *accepted=sources::AcceptedInputs::take_for(accepted,unit);
        let mut pending=false;
        loop {
            let result=(|| {
                if accepted.is_none() {*accepted=Some(declaration.accept_query(r,Some(self))?);}
                let inputs=accepted.as_mut().unwrap();inputs.check_unit(unit)?;
                inputs.clone().track(encoder);
                #[cfg(test)]
                let _record=sources::record_reads(inputs);
                if pending {self.encode_query_jobs(r,encoder,grid)}else {plan(self,r,encoder,inputs)}
            })();
            match &result {
                Err(error) if error.is_preparation_pending()=>pending=self.ordered&&!self.jobs.is_empty(),
                Ok(()) if pending=>pending=false,
                _=>return result,
            }
            let idle=!encoder.has_work();
            r.uploads.finish(encoder);
            std::mem::replace(encoder,crate::submission::CommandEncoder::new(&r.device,&Default::default())).submit(&r.queue);
            r.await_capture_submission(idle).await?;
            if result.is_err() {self.prepare_capture_work_async(r).await?;}
        }
        }.await;
        if result.is_err() {self.discard_unencoded_jobs();}
        result
    }

    #[expect(clippy::too_many_arguments, reason = "Tiled capture keeps submission size and generic tile consumer explicit")]
    pub(super) async fn capture_query_tiles(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, region: PixelRect, output: Output, preview:bool, tiles_per_submission:u32,
        backdrop:Option<OccurrenceHandle>, encoder: &mut crate::submission::CommandEncoder,
        mut consume: impl FnMut(&mut WgpuRasterizer, &wgpu::TextureView, PixelRect, &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError>,
    ) -> Result<(), GpuRasterError> {
        let ordered=self.ordered;let previous_backdrop=self.presented_backdrop;
        self.ordered|=!objects::inline_sampling(r);self.presented_backdrop=backdrop;
        let result=async {
            let window = if matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)) {region.into()} else {self.cached_capture_window(packet.scene, region)};
            let preparation_inputs=sources::Inputs::default().for_unit(sources::InputUnit::new(region,0))?;let mut preparation=None;
            self.complete_capture_plan(r,encoder,None,&preparation_inputs,&mut preparation,|scene,r,encoder,_|
                scene.prepare_region(r, packet, (region, window), window.in_frame(packet.document_extent), matches!(output,Output::Source(_) | Output::MaskImage(_) | Output::Objects(_)), encoder)).await?;
            let (texture, view) = create_color_target(&r.device, [PAGE_SIZE;2], "statistics tile");
            let mut tiles=0;
            for tile in page_coordinates(region) {
                let grid = [tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]];
                if preview && query_grid_size(grid).contains(&0) {continue;}
                let region = page_rect(tile).intersect(region);
                let destination = Image {texture:texture.clone(),view:view.clone(),plan:display_mips::Plan::window(packet.document_extent,0,region)};
                let unit=sources::InputUnit::tile(tile);
                let declaration=self.declare_capture(r,packet,&destination,&[region],output,None,None)?.for_unit(unit)?;let mut accepted=None;
                self.complete_capture_plan(r,encoder,preview.then_some(grid),&declaration,&mut accepted,|scene,r,encoder,accepted|
                    scene.capture_prepared_regions(r, packet, &destination, &[region], output, preview, None, encoder, accepted)).await?;
                accepted.take().unwrap().track(encoder);
                self.object_results.flush_retired(encoder);self.exact_object_results.flush_retired(encoder);
                consume(r, &view, region, encoder)?;
                tiles+=1;
                if !preview && tiles%tiles_per_submission==0 {
                    r.uploads.finish(encoder);
                    std::mem::replace(encoder, crate::submission::CommandEncoder::new(&r.device, &Default::default())).submit(&r.queue);
                    crate::local_tone::wait_async(&r.device, &r.queue).await.map_err(GpuRasterError::Color)?;
                }
            }
            self.release_exact_objects(encoder);
            Ok(())
        }.await;
        self.ordered=ordered;self.presented_backdrop=previous_backdrop;result
    }

    #[expect(clippy::too_many_arguments, reason = "Prepared capture keeps output encoding and independent destination regions explicit")]
    fn capture_prepared_regions(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &Image,
        regions: &[PixelRect], output: Output, preview: bool, reserved: Option<&[scratch::Usage]>, encoder: &mut crate::submission::CommandEncoder,
     accepted:&mut sources::AcceptedInputs) -> Result<(), GpuRasterError> {
        self.capture_prepared_regions_reuse(r,packet,destination,regions,output,preview,reserved,None,encoder, accepted)
    }
    #[expect(clippy::too_many_arguments, reason = "One capture tile keeps its output region and initialized pages explicit")]
    fn capture_prepared_tile_jobs(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, destination: &Image,
        tile: [u32; 2], covered: PixelRect, output: Output, initialized: Option<&std::collections::BTreeSet<[u32; 2]>>,
     accepted:&mut sources::AcceptedInputs) -> Result<(), GpuRasterError> {
        let mut waiting = false;
        let image = match output {
            Output::Artwork(parent) => {
                let image = self.group(r, packet, parent, tile, accepted)?;
                self.converted(r, image, Convert::linear(packet))?
            }
            Output::Objects(id) => {
                let image = self.object_tile(r, packet, id, tile, accepted)?;
                self.converted(r, image, Convert::linear(packet))?
            }
            Output::EffectInput(id) | Output::EffectChannels(id) | Output::EffectComposite(id) => {
                packet.scene.occurrence(id).ok_or(GpuRasterError::InvalidExtent)?;
                let previous = self.stop_before.replace(id);
                let image = self.group(r, packet, layer_core::composite_input_scope(packet.scene, id), tile, accepted);
                self.stop_before = previous;
                let image = match image {
                    Err(GpuRasterError::NotReady|GpuRasterError::NotPublished) => { waiting = true; self.alloc(r, wgpu::Color::TRANSPARENT)? }
                    image => image?,
                };
                let image = if matches!(output, Output::EffectInput(_)) { image } else { self.effect(r, packet, &[id], tile, image, accepted)? };
                if matches!(output, Output::EffectComposite(_)) { image } else { self.converted(r, image, Convert::linear(packet))? }
            }
            Output::Source(target) | Output::MaskImage(target) => {
                let owner=packet.scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                match target {
                    SourceTarget::Paint(_) => {
                        let placement=accepted.placement(target)?;
                        self.paint_tile(r, packet, owner, &placement, tile, accepted)?
                    },
                    SourceTarget::Coverage(_) => {
                        let (use_,source)=packet.scene.mask(owner).ok_or(GpuRasterError::MissingPaintLayer(target))?;
                        let mut use_=use_.clone(); use_.inverted=false;
                        let mask = self.mask_at(r,&use_,source,packet.scene.target_offset(target),tile, accepted)?;
                        let held = self.own(mask);
                        if matches!(output, Output::MaskImage(_)) {
                            let image = self.alloc(r, wgpu::Color::TRANSPARENT)?;
                            self.draw(r, image, self.pool[mask].view.clone(), None, [0., 0., 256., 256.], [20., 1., 0., 0.], false, Convert::None);
                            self.free(mask); image
                        } else { self.keep(held) }
                    }
                    SourceTarget::Selection(_) => return Err(GpuRasterError::MissingPaintLayer(target)),
                }
            },
            Output::Display => self.display_tile(r, packet, tile, accepted)?,
        };
        let _image = self.own(image);
        if waiting { return Err(GpuRasterError::NotReady); }
        if let Output::EffectComposite(handle)=output && destination.plan.level>0 {
            let execution=if matches!(self.jobs.last(),Some(Job::Effect {data,..}) if data[19]>2.5) {effects::Execution::ReducedContribution} else {effects::Execution::Reduced};
            let prepared=self.effects.prepare(r,packet.scene,&[handle],execution,packet.time_seconds,0,packet.blend_space)?;
            let Job::Effect {target,data,prepared:effect,source_target,changed_cells,..}=self.jobs.last_mut().unwrap() else {unreachable!()};
            let rect=paint_transform::texel_rect(covered.window_local(destination.plan.bounds),1<<destination.plan.level);
            *target=destination.view.clone();
            data[..4].copy_from_slice(&rect.map(|v|v as f32));
            data[4..6].copy_from_slice(&destination.plan.size.map(|v|v as f32));
            data[24..26].copy_from_slice(&[(covered.max_x()-tile[0]*PAGE_SIZE) as f32,(covered.max_y()-tile[1]*PAGE_SIZE) as f32]);
            data[26]=1.;
            data[27]=(1<<destination.plan.level) as f32;
            data[28..30].copy_from_slice(&[(covered.min_x()-tile[0]*PAGE_SIZE) as f32,(covered.min_y()-tile[1]*PAGE_SIZE) as f32]);
            *effect=prepared;
            if initialized.is_some_and(|pages|pages.contains(&tile)) && !packet.composite_all && r.document_damage.is_empty() {
                *changed_cells=source_target.and_then(|id|r.changed_cells.as_ref().unwrap().reusable(id,tile)).cloned();
            }
            let cells=u64::from(rect[2])*u64::from(rect[3]);
            if changed_cells.is_some() {r.metrics.native_effect_reuse_candidate_cells+=cells;}
            else {r.metrics.native_effect_forced_cells+=cells;}
            self.free(image);
        } else {self.copy_window_tile(image, destination, tile);}
        Ok(())
    }

    #[expect(clippy::too_many_arguments, reason = "Retained capture owns explicit initialized output and dependency regions")]
    fn capture_prepared_regions_reuse(
        &mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,destination:&Image,
        regions:&[PixelRect],output:Output,preview:bool,reserved:Option<&[scratch::Usage]>,initialized:Option<&std::collections::BTreeSet<[u32;2]>>,encoder:&mut submission::CommandEncoder,
     accepted:&mut sources::AcceptedInputs)->Result<(),GpuRasterError> {
        let object_pages = packet.scene.order().iter().any(|owner| packet.scene.object_layer(*owner).is_some());
        for (tile,covered) in self.begin_capture(r,packet,destination,regions,output,reserved) {
            self.capture_prepared_tile_jobs(r, packet, destination, tile, covered, output, initialized, accepted)?;
            if preview || object_pages {
                let result = self.encode_jobs_for(r, encoder, preview.then_some([tile[0]*PAGE_SIZE,tile[1]*PAGE_SIZE,packet.document_extent[0],packet.document_extent[1]]), accepted.decode_class());
                if result.is_err() { self.object_results.reset_used(); self.exact_object_results.reset_used(); return result; }
                self.object_results.flush_retired(encoder); self.exact_object_results.flush_retired(encoder);
                if self.inline_objects.taps > crate::frame_pace::IDLE_TAPS {
                    self.inline_objects.taps = 0;
                    Self::submit_chunk(r, encoder, "after inline object sampling")?;
                    if r.snapshot_cancelled.as_ref().is_some_and(|cancelled| cancelled.load(std::sync::atomic::Ordering::Relaxed)) {
                        return Err(GpuRasterError::Color("Snapshot capture cancelled".into()));
                    }
                }
            }
        }
        if !preview && self.jobs.len() <= SOURCE_SLOTS * 2 && self.jobs.iter().all(|job| match job {
            Job::DecodedTile(_) => true,
            Job::Effect { target, prepared, .. } => *target == destination.view && prepared.pointwise,
            _ => false,
        }) {
            let mut prefix = 0;
            for i in 0..self.jobs.len() {
                let Job::DecodedTile(pending) = &self.jobs[i] else { continue; };
                if self.jobs[..i].iter().any(|job| match job {
                    Job::DecodedTile(previous) => previous.view == pending.view,
                    Job::Effect { sources, masks, .. } => sources.contains(&pending.view) || masks.contains(&pending.view),
                    _ => true,
                }) { continue; }
                self.jobs[prefix..=i].rotate_right(1);
                prefix += 1;
            }
            self.source_jobs.clear();
        }
        crate::performance_trace::counter(c"Capy native effect reuse candidates",r.metrics.native_effect_reuse_candidate_cells);
        crate::performance_trace::counter(c"Capy native effect forced cells",r.metrics.native_effect_forced_cells);
        let result = self.encode_jobs_for(r, encoder, None, accepted.decode_class());
        if result.is_ok() {
            self.object_results.flush_retired(encoder);
            self.exact_object_results.flush_retired(encoder);
        } else {
            self.object_results.reset_used();
            self.exact_object_results.reset_used();
        }
        result
    }

    fn begin_capture(&mut self,r:&WgpuRasterizer,packet:FramePacket<'_>,destination:&Image,regions:&[PixelRect],output:Output,reserved:Option<&[scratch::Usage]>)
        ->Vec<([u32;2],PixelRect)> {
        self.jobs.clear();self.source_jobs.clear();
        self.object_display = self.object_live || matches!(output, Output::Display);
        self.clear_material_pages();
        self.used.iter_mut().for_each(scratch::Usage::reset);
        if let Some(reserved) = reserved { self.used[..reserved.len()].clone_from_slice(reserved); }
        self.stop_before = packet.scene.effect_input();
        let mut pages: Vec<_> = regions.iter().flat_map(|region| page_coordinates(*region)).collect();
        crate::artwork::resident_first(r, packet.scene, &mut pages);
        pages.into_iter().filter_map(|tile| {
            let covered = regions.iter().fold(PixelRect::EMPTY, |bounds, region| bounds.union(page_rect(tile).intersect(*region))).intersect(destination.plan.bounds);
            (!covered.is_empty()).then_some((tile,covered))
        }).collect()
    }
    #[expect(clippy::too_many_arguments, reason = "A capture declaration plans the same destination, regions and initialized pages as its reader")]
    fn declare_capture(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,destination:&Image,regions:&[PixelRect],output:Output,
        reserved:Option<&[scratch::Usage]>,initialized:Option<&std::collections::BTreeSet<[u32;2]>>,
    )->Result<sources::Inputs,GpuRasterError> {
        let inputs=self.source_placements(packet,output)?;
        let frame=matches!(output,Output::Display).then(||self.scale_pages.lock().unwrap().expected().cloned()).flatten();
        let declaration=match frame {Some(frame)=>sources::Declaration::scene(frame,self.submission.clone()),None=>self.capture_declaration(r,packet)?};
        self.jobs.clear();self.source_jobs.clear();
        self.declare(r,inputs,declaration,|scene,r,accepted| {
            let mut waiting=false;
            for (tile,covered) in scene.begin_capture(r,packet,destination,regions,output,reserved) {
                match scene.capture_prepared_tile_jobs(r,packet,destination,tile,covered,output,initialized,accepted) {
                    Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>waiting=true,
                    result=>result?,
                }
            }
            if waiting {Err(GpuRasterError::NotReady)}else {Ok(())}
        })
    }

    pub(crate) fn prepare_scale_sources(&mut self,r:&WgpuRasterizer,packet:FramePacket<'_>,batch_tiles:&[Vec<crate::brush_tiles::BrushTile>]) {
        self.scale_sources.vector_preview = r.vector_preview.as_ref().zip(r.vector_preview_shown).map(|(p, (_, _, bounds))| (p.owner, p.path.clone(), bounds));
        self.scale_sources.vector_motion = r.composed_vector_motion().cloned();
        self.scale_sources.motion_split = r.vector_motion_split.clone();
        self.scale_sources.moving = r.vector_motion_drawn.as_ref().map(|(motion, _)| (motion.owner, vectors::Moving(motion.items.clone())));
        self.scale_sources.prepared = r.vector_prepared.as_ref().map(|(prepared, ..)| prepared.clone());
        self.scale_pages.lock().unwrap().release_moved(self.scale_sources.moving.iter().chain(self.scale_sources.prepared.iter().map(|(_, moving, _)| moving)).map(|(_, moving)| moving));
        self.object_spatial.prepare(packet.scene);
        self.scale_sources.prepare(r,packet,batch_tiles);
    }

    pub(crate) fn prepare_scale_work(&self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,reset:bool)->Result<(),GpuRasterError> {
        let Some(mut cache)=r.scale_display.take() else {return Ok(());};
        let mut navigator=std::mem::take(&mut r.navigator);
        let prepared=(|| {
            cache.prepare_graph(r,packet,self)?;
            if reset || !navigator.follows(&cache) {navigator.prepare(r,packet,self,self.scale_pages.clone(),reset)?;}
            if !cache.contact_holds(r,packet) {cache.retain_shared_work(r,packet,self,&navigator);}
            Ok(())
        })();
        r.scale_display=Some(cache);
        r.navigator=navigator;
        prepared
    }

    pub(crate) fn prepare_signatures(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<(),GpuRasterError> {
        if let Some(display)=r.scale_display.as_ref() {
            if display.holds_contact_frame(r,packet) {self.scale_pages.lock().unwrap().advance_epoch();return Ok(());}
            display.exit_contact();
        }
        let frame=SignatureFrame::prepare(r,packet)?;
        self.scale_pages.lock().unwrap().set_expected(frame);
        Ok(())
    }

    pub fn compose(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        encoder: &mut crate::submission::CommandEncoder,
        budget: &mut WorkBudget,
    ) -> Result<PixelRect, GpuRasterError> {
        if let Some(startup) = &r.startup { startup.compiler.check()?; }
        if !r.scene_scale_ready(packet.scene) { return Err(GpuRasterError::NotReady); }
        let write = self.begin_write(r,encoder)?;
        self.object_results.retain(packet.scene,packet.blend_space,r.device.working_space());
        self.object_results.take_damage();
        self.vector_damage(r, packet);
        self.prepare_presented_motion(r, packet, encoder)?;
        let mut commands = self.scale_commands.take().unwrap_or_else(|| scale::RegionRecords::charged(r, &self.tiers.gpu));
        commands.begin();
        let mut cache = r.scale_display.take().expect("prepared display cache");
        cache.submission_valid = Some(self.valid.clone());
        self.object_live = true;
        self.object_display = true;
        self.placement_display = true;
        let result = (|| {
            cache.prepare_graph(r, packet, self)?;
            if r.moving_layer.is_none()
                && cache.render_preview(self, r, packet, &mut scale::Encoding { encoder, commands: &mut commands, retained:0, budget })? {
                return Ok(PixelRect::full(packet.document_extent));
            }
                self.retire_images(|scene| scene.images.release_window_pixels());
                self.image_window = None; self.image_output = None;
                if cache.plan.level > 0 && !scale::bounded(packet.scene) {
                    self.clear_material_pages();
                    self.pool.clear();
                    self.used.clear();
                }
                self.effects.retain(packet.scene);
                r.telemetry.phase_begin(8, &r.device, &r.queue, encoder);
                r.telemetry.phase_end(8, encoder);
                cache.render(self, r, packet, &mut scale::Encoding { encoder, commands: &mut commands, retained:0, budget })
        })();
        self.object_live = false;
        self.object_display = false;
        self.scale_commands = Some(commands);
        r.scale_display = Some(cache);
        let result=if result.is_ok() || matches!(&result,Err(error) if error.is_preparation_pending()) {
            let mut objects=std::mem::replace(&mut self.object_results,object_cache::ObjectCache::new(&self.tiers,Default::default(),false));
            let advanced=objects.advance_with_memory(r,self,encoder,|class|budget.allowance(class.min(Class::Visible))).map(|used|budget.spend(used));
            self.object_results=objects;
            advanced.and(result)
        } else {result};
        if matches!(&result, Err(error) if error.is_preparation_pending()) {
            self.clear_unencoded_jobs();
        }
        if result.is_err() { self.object_results.reset_used(); }
        self.object_results.flush_retired(encoder);
        self.image_damage = None;
        if result.is_ok() || matches!(&result, Err(error) if error.is_preparation_pending()) { write.track(encoder); }
        result
    }

    fn display_tile(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, tile: [u32; 2], accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        self.presented_backdrop = layer_core::paper::separate_paper(packet.scene).map(|(paper, _)| paper);
        let output = self.group(r, packet, None, tile, accepted);
        self.presented_backdrop = None;
        let mut output = output?;
        let mut held = self.own(output);
        if let Some(handle) = packet.inspect_mask
            && let Some((mask, source)) = packet.scene.mask(handle).filter(|(mask, _)| mask.enabled) {
            let m = self.mask_at(r, mask, source, packet.scene.target_offset(SourceTarget::Coverage(mask.source)), tile, accepted)?;
            let _mask = self.own(m);
            let tint = self.alloc(r, wgpu::Color::TRANSPARENT)?;
            self.draw(
                r,
                tint,
                self.pool[m].view.clone(),
                None,
                [0., 0., 256., 256.],
                [5., 1., 0., 0.],
                false,
                Convert::layers(packet),
            );
            self.free(m);
            output = self.combine(
                r,
                tint,
                output,
                1.,
                layer_core::LayerBlend::Normal,
                false,
                packet.blend_space,
            )?;
            held = self.own(output);
        }
        Ok(self.keep(held))
    }

    // wgpu handles hash by stable resource identity, not mutable GPU contents.
    #[allow(clippy::mutable_key_type)]
    fn encode_jobs(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        self.encode_query_jobs(r, encoder, None)
    }

    fn jobs_ready(&mut self,r:&WgpuRasterizer,start:usize)->bool {
        let mut required=std::mem::take(&mut self.pending_pipelines);
        required.render.clear();required.compute.clear();
        let mut render=|pipeline:&Deferred<wgpu::RenderPipeline>| {
            if !pipeline.ready() {required.render.push(pipeline.clone());}
        };
        let mut compute=|pipeline:&Deferred<wgpu::ComputePipeline>| {
            if !pipeline.ready() {required.compute.push(pipeline.clone());}
        };
        let mut effects_ready=self.effects.preparation_ready(self.jobs[start..].iter().filter_map(|job|match job {
            Job::Effect {prepared,..}=>Some(&prepared._parameters),_=>None,
        }));
        for (i,job) in self.jobs.iter().enumerate().skip(start) {
            if is_compute(job) || matches!(job,Job::Clear(..)) {compute(&r.scene_pipelines.constant.2);}
            match job {
                Job::Draw {..}=>if let Some(index)=bound_fragment(&r.device,&self.jobs,i) {render(&self.pipeline[index]);},
                Job::Fold(_)=>compute(&r.scene_pipelines.fold.pipeline),
                Job::Effect {prepared,..}=> {
                    effects_ready &= prepared.pipeline.ready();
                    match &prepared.pipeline {effects::Pipeline::Render(pipeline)=>render(pipeline),effects::Pipeline::Compute(pipeline)=>compute(pipeline)}
                },
                Job::Placement(job)=>compute(&self.placement[usize::from(job.scalar)].placement_pipeline(&job.transform,job.taps)),
                Job::Positions(..)=>r.transforms.as_ref().unwrap().mesh_pipelines().iter().for_each(&mut render),
                Job::Compose {format,..}=> {
                    let kernels=r.scene_pipelines.scale.output(*format);
                    for pipeline in [&kernels.compose,&kernels.reduce_pair,&kernels.reduce] {compute(pipeline);}
                },
                Job::Resample {..}=>compute(&r.scene_pipelines.resample.area),
                Job::Reduce {kernel,format,..}=> {
                    let kernels=r.scene_pipelines.scale.output(*format);
                    compute(match kernel {Reduction::Pages=>&kernels.reduce,Reduction::Phased=>&kernels.reduce_phased});
                },
                Job::Vector(_)=>compute(r.scene_pipelines.vector.pipeline()),
                Job::VectorResample {..}=>compute(r.scene_pipelines.vector.pipelines()[1]),
                Job::Sampling(step)=>compute(match step.as_ref() {
                    objects::SamplingStep::Begin {..}=>&r.scene_pipelines.objects.denominator,
                    objects::SamplingStep::Contribute {..}=>&r.scene_pipelines.objects.contribute,
                    objects::SamplingStep::Normalize=>&r.scene_pipelines.objects.normalize,
                }),
                Job::Collection(job)=>job.objects.iter().for_each(|object|compute(if object.samples.is_some_and(|samples|samples.format()==wgpu::TextureFormat::Rg32Uint) {&r.scene_pipelines.objects.preview_integer}else {&r.scene_pipelines.objects.preview})),
                Job::DecodedTile(source)=>if let Some(pipeline)=source.pipeline(&r.scene_pipelines.source) {render(pipeline);},
                Job::Transform(region)=>region.pipelines(r).into_iter().flatten().for_each(&mut render),
                Job::Watercolor {..}=>if material_compute(&self.jobs,i) {compute(&r.pipelines.watercolor_compute.1);}else {render(&r.pipelines.watercolor_composite);},
                _=>{},
            }
        }
        let ready=required.require(&r.compilation,startup::DOCUMENT)
            & (effects_ready || self.effects.enqueue(&r.compilation,startup::DOCUMENT));
        self.pending_pipelines=required;
        ready
    }
    fn publish_surface(&mut self,target:SourceTarget,output:resident::Encoding) {
        self.jobs.push(Job::Publish {target,output:Arc::new(std::sync::Mutex::new(Some(output)))});
    }
    fn publish_job(r:&mut WgpuRasterizer,receipt:&submission::Submission,job:&Job)->Result<(),GpuRasterError> {
        if let Job::Publish {target,output}=job {
            let output=output.lock().unwrap().take().ok_or(GpuRasterError::Invariant("publication encoding already consumed"))?;
            let authored=output.intent().is_some();let written=output.publish(receipt);
            if authored {r.record_raster_write(*target,written)?;}
        }
        Ok(())
    }
    fn encode_query_jobs(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, grid: Option<[u32;4]>) -> Result<(), GpuRasterError> {
        self.encode_jobs_for(r, encoder, grid, Class::Visible)
    }

    fn encode_jobs_for(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder, grid: Option<[u32;4]>, class: Class) -> Result<(), GpuRasterError> {
        r.source_tiles.get_mut().flush_retired(encoder);
        if self.ordered {return self.encode_ordered_jobs(r,encoder,grid).and_then(|ready|if ready {Ok(())}else {Err(GpuRasterError::NotReady)});}
        let result = self.prepare_jobs(r, encoder, 0, class).and_then(|ready|if ready {self.encode_jobs_inner(r, encoder, grid)}else {Err(GpuRasterError::NotReady)});
        if result.is_err() {self.discard_unencoded_jobs();}
        result
    }

    fn prepare_jobs(&mut self, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, start:usize, class:Class) -> Result<bool, GpuRasterError> {
        let mut ready = true;
        for job in &self.jobs[start..] {
            if let Job::DecodedTile(source) = job && !source.ready() && (!r.snapshot_worker || cfg!(target_arch="wasm32")) {
                source.prepare(r,class)?;
                ready = false;
            }
        }
        ready &= self.jobs_ready(r,start);
        if !ready {return Ok(false);}
        while !Self::upload_jobs(r,encoder,&self.jobs[start..],class)? {Self::submit_source_uploads(r,encoder,class)?;}
        let result = self.jobs[start..].iter().try_for_each(|job|match job {
            Job::DecodedTile(source) => r.source_tiles.get_mut().preflight(&r.device, source),
            Job::Publish {target,output}=> {
                let output=output.lock().unwrap();
                let output=output.as_ref().ok_or(GpuRasterError::Invariant("publication encoding already consumed"))?;
                if let Some(intent)=output.intent() {r.validate_raster_intent(*target,intent)?;}
                Ok(())
            },
            _ => Ok(()),
        });
        r.source_tiles.get_mut().flush_retired(encoder);
        result.map(|()|true)
    }

    fn encode_ordered_jobs(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,grid:Option<[u32;4]>)->Result<bool,GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("ordered job encoding");

        if !self.jobs_ready(r,0) {return Ok(false);}
        let ready=self.jobs.iter().position(|job|matches!(job,Job::DecodedTile(source) if !source.ready())).unwrap_or(self.jobs.len());
        let end=if Self::upload_jobs(r,encoder,&self.jobs[..ready],Class::Visible)? {ready}
            else {self.jobs.iter().position(|job|matches!(job,Job::DecodedTile(source) if !source.recorded())).unwrap_or(ready)};
        let remaining=self.jobs.split_off(end);
        let ranges=std::mem::take(&mut self.source_jobs);
        self.source_jobs=ranges.iter().filter_map(|range|{let end=range.end.min(end);(range.start<end).then_some(range.start..end)}).collect();
        let result=self.prepare_jobs(r,encoder,0,Class::Visible).and_then(|ready|if ready {self.encode_jobs_inner(r,encoder,grid)}else {Err(GpuRasterError::NotReady)});
        self.jobs=remaining;
        self.source_jobs=ranges.into_iter().filter_map(|range|{let start=range.start.max(end);(start<range.end).then(||start-end..range.end-end)}).collect();
        result?;
        if let Some(Job::DecodedTile(source))=self.jobs.first() {
            if !source.ready() {r.source_receipt=Some(Arc::downgrade(source));source.prepare(r,Class::Visible)?;}
            return Ok(false);
        }
        r.source_receipt=None;Ok(true)
    }
    fn upload_jobs(r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,jobs:&[Job],class:Class)->Result<bool,GpuRasterError> {
        let pending:Vec<_>=jobs.iter().filter_map(|job|match job {Job::DecodedTile(source) if !source.recorded()=>Some(source.clone()),_=>None}).collect();
        for source in pending {
            if !sources::encode_pending(r,encoder,source,class)? {return Ok(false);}
        }
        Ok(true)
    }

    fn clear_unencoded_jobs(&mut self) {
        self.jobs.clear();self.source_jobs.clear();self.clear_material_pages();
    }
    pub(crate) fn discard_unencoded_jobs(&mut self) {
        self.discard_jobs_from(0);
    }
    fn discard_jobs_from(&mut self, start: usize) {
        self.jobs.truncate(start);
        self.source_jobs.retain_mut(|range| {range.end=range.end.min(start);range.start<range.end});
        self.clear_material_pages();self.used.iter_mut().for_each(scratch::Usage::reset);
        self.effects.discard_unreferenced();
        self.object_results.reset_used();self.exact_object_results.reset_used();
    }

    fn encode_jobs_inner(&mut self, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, grid: Option<[u32;4]>) -> Result<(), GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("job encoding");

        let mut jobs=std::mem::take(&mut self.jobs);
        let mut source_jobs=std::mem::take(&mut self.source_jobs);
        let result=self.encode_batch(r,encoder,grid,&jobs,&source_jobs);
        if result.is_ok() {jobs.clear();source_jobs.clear();}
        self.jobs=jobs;self.source_jobs=source_jobs;
        result
    }

    fn encode_batch(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        grid: Option<[u32;4]>, jobs: &[Job], source_jobs: &[std::ops::Range<usize>],
    ) -> Result<(), GpuRasterError> {
        let job_grid = |i| grid.filter(|_| !source_jobs.iter().any(|range| range.contains(&i)));
        self.effects.encode_preparation(&mut r.uploads, encoder, jobs.iter().filter_map(|job| match job {
            Job::Effect { prepared, .. } => Some(&prepared._parameters), _ => None,
        }))?;
        let base=self.reserve_records(r,jobs.len());
        self.stage_records(r,jobs,job_grid);
        if !self.upload.is_empty() {
            r.uploads.write_at(
                encoder,
                &self.buffer,
                (base * self.stride) as u64,
                &self.upload,
            )?;
        }
        let overwritten_clear = |i: usize| {
            let Job::Clear(target, _) = &jobs[i] else { return false; };
            if matches!(jobs.get(i + 1), Some(Job::Fold(fold)) if fold.absorbs(target)) { return true; }
            jobs.get(i + 1).is_some_and(|next|
                matches!(next, Job::Draw { target: next, .. } | Job::Effect { target: next, .. }
                    | Job::Watercolor { target: next, .. } if next == target)
                && (!is_compute(next) || matches!(next, Job::Draw { data, .. } | Job::Effect {data,..}
                    if data[..2] == [0., 0.] && data[2..4] == data[4..6])))
        };
        #[cfg(test)]
        crate::test_support::PassCounts {scene_dispatches:jobs.iter().enumerate().map(|(i,job)|match job {
            Job::Clear(..)=>u64::from(!overwritten_clear(i)),
            Job::Draw {..}|Job::Watercolor {..}=>1+u64::from(portable_draw(&r.device,jobs,i)),
            Job::Effect {..}|Job::Reduce {..}|Job::Compose {..}|Job::Resample {..}|Job::Placement(_)|Job::Fold(_)=>1,
            _=>0,
        }).sum(),fold_inputs:jobs.iter().map(|job|if let Job::Fold(fold)=job {fold.layers}else {0}).sum(),..Default::default()}.record();
        let compute_clear = |i: usize| matches!(&jobs[i], Job::Clear(target, _) if !overwritten_clear(i) && {
            let texture = target.texture();
            texture.format() == wgpu::TextureFormat::Rgba32Float && texture.usage().contains(wgpu::TextureUsages::STORAGE_BINDING)
                && texture.mip_level_count() == 1 && texture.depth_or_array_layers() == 1
        });
        let source_bindings = &mut self.source_bindings;
        let fold_bindings = &mut self.fold_bindings;
        let mask_bindings = &mut self.mask_bindings;
        let output_bindings = &mut self.output_bindings;
        let compute_bindings = &mut self.compute_bindings;
        let effect_outputs=&mut self.effect_outputs;
        let mut encoded_through = 0;
        for (i, job) in jobs.iter().enumerate() {
            if i < encoded_through {
                continue;
            }
            if material_compute(jobs,i) {
                let end=material_run_end(jobs,i);
                let mut draws = Vec::new();
                let mut resamples = Vec::new();
                for (j,job) in jobs.iter().enumerate().take(end).skip(i) {
                    match job {
                        Job::DecodedTile(pending) => Self::recorded_upload(pending)?,
                        Job::Placement(job) => {
                            let plane = usize::from(job.scalar);
                            draws.push((plane, placement::prepare(&mut self.placement[plane], r, encoder, job)?));
                        },
                        Job::Resample { views, .. } => resamples.push(r.scene_pipelines.resample.binding(&r.device, &self.buffer,
                            ((base+j)*self.stride) as u64, [&views[0], &views[1], &views[0]])),
                        _ => {},
                    }
                }
                let mut draws = draws.iter();
                let mut resamples = resamples.iter();
                #[cfg(test)]
                if jobs[i..end].iter().any(|job|matches!(job,Job::Placement(_) | Job::Reduce {..} | Job::Watercolor {..})) {
                    crate::test_support::PassCounts {material_batches:1,..Default::default()}.record();
                }
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("placed material tiles"), timestamp_writes: None,
                });
                for (j, job) in jobs.iter().enumerate().take(end).skip(i) {
                    match job {
                        Job::Placement(_) => {
                            let (plane, draw) = draws.next().unwrap();
                            self.placement[*plane].encode_placement(&mut pass, draw);
                        }
                        Job::Watercolor { coordinates, target, views, record } => {
                            let binding=self.watercolor_bindings.get(views,||views_group(&r.device,"layer watercolor pigment and wetness neighborhood",&r.watercolor_layout,views)).clone();
                            let (layout, pipeline) = &r.pipelines.watercolor_compute;
                            let output = match self.pool.iter().find(|page| page.view == *target) {
                                Some(page) => page.scene_output(r, layout),
                                None => output_bindings.get(target, || crate::bindings::group(
                                    &r.device, "watercolor output", layout, [wgpu::BindingResource::TextureView(target)],
                                )),
                            };
                            pass.set_pipeline(pipeline);
                            pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                            pass.set_bind_group(1, &coordinates.0, &[coordinates.1]);
                            pass.set_bind_group(2, &binding, &[]);
                            pass.set_bind_group(3, output, &[]);
                            pass.dispatch_workgroups(PAGE_SIZE.div_ceil(8), PAGE_SIZE.div_ceil(8), 1);
                        }
                        Job::Reduce { output, sources, size, kernel, format,.. } => {
                            let binding=self.region_bindings.get(&(sources.clone(),output.clone()),||scale::RegionRecords::inputs(r,sources.each_ref(),output)).clone();
                            let kernels=r.scene_pipelines.scale.output(*format);
                            pass.set_pipeline(match kernel {
                                Reduction::Pages => &kernels.reduce,
                                Reduction::Phased => &kernels.reduce_phased,
                            });
                            pass.set_bind_group(0, &self.reduction_binding, &[((base + j) * self.stride) as u32]);
                            pass.set_bind_group(1, &binding, &[]);
                            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
                        }
                        Job::Compose { values, output, sources, format,.. } => {
                            let binding=self.region_bindings.get(&(sources.clone(),output.clone()),||scale::RegionRecords::inputs(r,sources.each_ref(),output)).clone();
                            pass.set_pipeline(&r.scene_pipelines.scale.output(*format).compose);
                            pass.set_bind_group(0, &self.reduction_binding, &[((base+j)*self.stride) as u32]);
                            pass.set_bind_group(1, &binding, &[]);
                            pass.dispatch_workgroups(values[2].div_ceil(8), values[3].div_ceil(8), 1);
                            #[cfg(test)]
                            crate::test_support::PassCounts {composed_texels:u64::from(values[2])*u64::from(values[3]),..Default::default()}.record();
                        }
                        Job::Resample { size, .. } => {
                            pass.set_pipeline(&r.scene_pipelines.resample.area);
                            pass.set_bind_group(0, resamples.next().unwrap(), &[]);
                            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
                        }
                        Job::Clear(..) | Job::DecodedTile(_) | Job::Publish {..} => {}
                        _ => unreachable!(),
                    }
                }
                drop(pass);
                let receipt=encoder.submission();
                for job in &jobs[i..end] {Self::publish_job(r,&receipt,job)?;}
                encoded_through = end;
                continue;
            }
            if is_compute(job) || compute_clear(i) {
                let (layout, inputs, pipeline) = &r.scene_pipelines.constant;
                let end = (i + 1..jobs.len()).find(|&j| !matches!(&jobs[j],Job::Publish {..}) && !is_compute(&jobs[j]) && !compute_clear(j) && !overwritten_clear(j))
                    .unwrap_or(jobs.len());
                let pass = encoder.compute_pass();
                self.effect_passes+=u64::from(jobs[i..end].iter().any(|job|matches!(job,Job::Effect {..})));
                for (j, job) in jobs.iter().enumerate().take(end).skip(i) {
                    if overwritten_clear(j) || matches!(job,Job::Publish {..}) { continue; }
                    if let Job::Clear(target, _) = job {
                        let size = target.texture().size();
                        let empty = [r.empty_view.clone(), r.empty_view.clone(), r.empty_view.clone(), r.empty_paper_view.clone()];
                        let input = compute_bindings.get(&empty, || compute_source_binding(r, inputs, &empty));
                        let output = match self.pool.iter().find(|page| page.view == *target) {
                            Some(page) => page.scene_output(r, layout),
                            None => output_bindings.get(target, || crate::bindings::group(&r.device, "scene normal layers destination", layout, [
                                wgpu::BindingResource::TextureView(target),
                            ])),
                        };
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                        pass.set_bind_group(1, input, &[]);
                        pass.set_bind_group(2, output, &[]);
                        pass.dispatch_workgroups(size.width.div_ceil(32), size.height.div_ceil(2), 1);
                        continue;
                    }
                    if let Job::Fold(fold)=job {
                        let key=(fold.views(&r.empty_view),fold.transfer.clone());
                        let inputs=fold_bindings.get(&key,||r.scene_pipelines.fold.inputs(&r.device,&key.0,&key.1));
                        let output = match self.pool.iter().find(|page| page.view == fold.target) {
                            Some(page) => page.scene_output(r, layout),
                            None => output_bindings.get(&fold.target, || crate::bindings::group(&r.device, "scene fold destination", layout, [
                                wgpu::BindingResource::TextureView(&fold.target),
                            ])),
                        };
                        pass.set_pipeline(&r.scene_pipelines.fold.pipeline);
                        pass.set_bind_group(0,&self.fold_binding,&[((base+j)*self.stride) as u32]);
                        pass.set_bind_group(1,inputs,&[]);
                        pass.set_bind_group(2,output,&[]);
                        pass.dispatch_workgroups(PAGE_SIZE.div_ceil(32),PAGE_SIZE.div_ceil(2),1);
                        continue;
                    }
                    if let Job::Effect {target,sources,data,prepared,masks,..}=job {
                        let effects::Pipeline::Compute(pipeline)=&prepared.pipeline else {unreachable!()};
                        let input=source_bindings.get(&(true,sources.clone()),||source_binding(r,&self.effects.sources,sources,false));
                        let output=effect_outputs.get(&(prepared.mask_slots,masks.as_ref().clone(),target.clone()),||crate::bindings::group(
                            &r.device,"pointwise effect output",&self.effects.outputs[prepared.mask_slots-1],
                            masks.iter().take(prepared.mask_slots).map(wgpu::BindingResource::TextureView).chain([wgpu::BindingResource::TextureView(target)]),
                        ));
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0,&self.binding,&[((base+j)*self.stride) as u32]);
                        pass.set_bind_group(1,input,&[]);
                        pass.set_bind_group(2,&prepared.binding,&[]);
                        pass.set_bind_group(3,output,&[]);
                        let size=job_grid(j).map_or([data[2] as u32,data[3] as u32],query_grid_size);
                        debug_assert!(!size.contains(&0), "empty pointwise effect job was queued");
                        pass.dispatch_workgroups(size[0].div_ceil(32),size[1].div_ceil(2),1);
                        continue;
                    }
                    let Job::Draw { target, sources, data, clip, .. } = job else { unreachable!() };
                    pass.set_pipeline(pipeline);
                    let input = compute_bindings.get(sources, || compute_source_binding(r, inputs, sources));
                    let output = match self.pool.iter().find(|page| page.view == *target) {
                        Some(page) => page.scene_output(r, layout),
                        None => output_bindings.get(target, || crate::bindings::group(&r.device, "scene normal layers destination", layout, [
                            wgpu::BindingResource::TextureView(target),
                        ])),
                    };
                    pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                    pass.set_bind_group(1, input, &[]);
                    pass.set_bind_group(2, output, &[]);
                    let drawn = clip.filter(|_| !matches!(data[8] as u32, 13 | 15))
                        .map_or([data[2], data[3]], |clip| [clip.max_x() as f32 - data[0], clip.max_y() as f32 - data[1]]).map(|n| n as u32);
                    let size = job_grid(j).map_or(drawn, query_grid_size);
                    debug_assert!(!size.contains(&0), "empty pointwise draw job was queued");
                    pass.dispatch_workgroups(size[0].div_ceil(32), size[1].div_ceil(2), 1);
                }
                let receipt=encoder.submission();
                for job in &jobs[i..end] {Self::publish_job(r,&receipt,job)?;}
                encoded_through = end;
                continue;
            }
            match job {
                Job::Publish {..}=>Self::publish_job(r,&encoder.submission(),job)?,
                Job::Collection(job) => r.scene_pipelines.objects.clone().preview(&r.device,encoder,&mut r.uploads,job,&r.empty_view)?,
                Job::Sampling(step) => self.inline_objects.encode(r,encoder,step)?,
                Job::Vector(job) => self.vectors.encode(r,encoder,job)?,
                Job::Transform(region) => {
                    region.encode(r, encoder, &self.buffer, ((base+i)*self.stride) as u64)?;
                }
                Job::PaintBase(assembly) => assembly.encode(r,encoder),
                Job::Placement(_) | Job::Reduce { .. } | Job::Compose { .. } | Job::Resample { .. } | Job::Fold(_) => unreachable!(),
                Job::VectorResample { source, origin, target, target_origin, side, size, inverse } => r.scene_pipelines.vector.resample(&r.device, encoder,
                    crate::vector::Placed { view: source, origin: *origin, side: *side },
                    crate::vector::Placed { view: target, origin: *target_origin, side: *side }, PixelRect::full(*size), *inverse),
                Job::Positions(geometry, tile) => {
                    self.positions.upload(r, encoder, geometry)?;
                    self.positions.draw(r, encoder, *tile)?;
                }
                Job::DecodedTile(pending) => Self::recorded_upload(pending)?,
                Job::Clear(target, color) => {
                    if overwritten_clear(i) { continue; }
                    let attachments = [Some(attachment(target, wgpu::LoadOp::Clear(*color)))];
                    let _pass = encoder.begin_render_pass(&descriptor(&attachments));
                }
                Job::Copy {
                    source,
                    source_origin,
                    destination,
                    origin,
                    width,
                    height,
                } => encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        origin: wgpu::Origin3d { x: source_origin[0], y: source_origin[1], z: 0 },
                        ..source.as_image_copy()
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: destination,
                        origin: wgpu::Origin3d {
                            x: origin[0],
                            y: origin[1],
                            z: 0,
                        },
                        ..source.as_image_copy()
                    },
                    wgpu::Extent3d {
                        width: *width,
                        height: *height,
                        depth_or_array_layers: 1,
                    },
                ),
                Job::Draw { target, .. }
                | Job::Effect { target, .. }
                | Job::Watercolor { target, .. } => {
                    let end = if needs_blend(&r.device,job) { i + 1 } else { (i + 1..jobs.len())
                        .find(|&j| !matches!(&jobs[j],
                            Job::Draw { target: next, .. }
                            | Job::Effect { target: next, .. }
                            | Job::Watercolor { target: next, .. } if next == target) || needs_blend(&r.device,&jobs[j])
                            || is_compute(&jobs[j]))
                        .unwrap_or(jobs.len()) };
                    let load = if i > 0
                        && let Job::Clear(previous, color) = &jobs[i - 1]
                        && previous == target
                    {
                        wgpu::LoadOp::Clear(*color)
                    } else {
                        wgpu::LoadOp::Load
                    };
                    let portable = portable_draw(&r.device,jobs,i);
                    let source = portable.then(|| r.portable_blend.source(&r.device,target,r.device.working_format()));
                    if portable && let wgpu::LoadOp::Clear(color) = load {
                        let attachments = [Some(attachment(target,wgpu::LoadOp::Clear(color)))];
                        let _pass = encoder.begin_render_pass(&descriptor(&attachments));
                    }
                    let effect = jobs[i..end].iter().any(|j| matches!(j, Job::Effect { .. }));
                    let extent = [target.texture().width(), target.texture().height()];
                    let spatial = jobs[i..end].iter().any(|j| matches!(j, Job::Effect { prepared, .. } if !prepared.pointwise));
                    let span = if spatial { PAGE_SIZE * 2 } else { extent[0].max(extent[1]) };
                    let windows = (0..extent[1]).step_by(span as usize).flat_map(|y|
                        (0..extent[0]).step_by(span as usize).map(move |x|
                            PixelRect::new(x, y, (x + span).min(extent[0]), (y + span).min(extent[1]))));
                    for window in windows {
                        let load = if portable { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) }
                            else if window.min_x() == 0 && window.min_y() == 0 { load } else { wgpu::LoadOp::Load };
                        let attachments = [Some(attachment(source.as_ref().unwrap_or(target), load))];
                        let mut pass = encoder.begin_render_pass(&descriptor(&attachments));
                        if effect { self.effect_passes += 1; }
                        for (j, job) in jobs.iter().enumerate().take(end).skip(i) {
                            if let Job::Watercolor { coordinates, views, record, .. } = job {
                                let binding=self.watercolor_bindings.get(views,||views_group(&r.device,"layer watercolor pigment and wetness neighborhood",&r.watercolor_layout,views)).clone();
                                pass.set_pipeline(&r.pipelines.watercolor_composite);
                                pass.set_bind_group(0, &r.style_bind_group, &[*record * r.style_stride as u32]);
                                pass.set_bind_group(1, &coordinates.0, &[coordinates.1]);
                                pass.set_bind_group(2, &binding, &[]);
                                pass.set_scissor_rect(0, 0, PAGE_SIZE, PAGE_SIZE);
                                pass.draw(0..3, 0..1);
                                continue;
                            }
                            let sources = match job {
                                Job::Draw { sources, .. } | Job::Effect { sources, .. } => sources,
                                _ => unreachable!(),
                            };
                            let effect=matches!(job,Job::Effect {..});
                            let layout=if effect {&self.effects.sources} else {&self.layout};
                            let binding = source_bindings.get(&(effect,sources.clone()), || source_binding(r, layout, sources, !effect));
                            if let Job::Effect {
                                prepared, masks, changed_cells, ..
                            } = job
                            {
                                let effects::Pipeline::Render(pipeline)=&prepared.pipeline else {unreachable!()};
                                pass.set_pipeline(pipeline);
                                pass.set_bind_group(2, &prepared.binding, &[]);
                                let cells=changed_cells.as_ref().unwrap_or(&r.changed_cells.as_ref().unwrap().disabled);
                                let masks = mask_bindings.get(&(masks.as_ref().clone(),cells.clone()), || {
                                    let mut entries:Vec<_>=masks.iter().enumerate().map(|(i,view)|wgpu::BindGroupEntry {
                                        binding:i as u32,resource:wgpu::BindingResource::TextureView(view),
                                    }).collect();
                                    entries.push(wgpu::BindGroupEntry {binding:effects::MASK_SLOTS as u32,resource:cells.as_entire_binding()});
                                    r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                                        label:Some("effect tile masks and changed cells"),layout:&self.effects.masks,entries:&entries,
                                    })
                                });
                                pass.set_bind_group(3, masks, &[]);
                            } else if let Job::Draw { edges, .. } = job {
                                pass.set_bind_group(2,edges.as_ref().unwrap_or(&self.edge_binding),&[]);
                                pass.set_pipeline(&self.pipeline[bound_fragment(&r.device, jobs, j).expect("fragment draw operation")]);
                            }
                            pass.set_bind_group(0, &self.binding, &[((base + j) * self.stride) as u32]);
                            pass.set_bind_group(1, binding, &[]);
                            let (data, clip) = match job {
                                Job::Draw { data, clip, .. } => (data, *clip),
                                // A fullscreen triangle extends beyond its rectangle.
                                // A final effect can write directly into one region
                                // of a larger composite: clip it to that region or
                                // it overwrites adjacent tiles with out-of-range reads.
                                Job::Effect { data, .. } => (data, Some(PixelRect::new(
                                    data[0].max(0.).floor() as u32,
                                    data[1].max(0.).floor() as u32,
                                    (data[0] + data[2]).min(data[4]).max(0.).ceil() as u32,
                                    (data[1] + data[3]).min(data[5]).max(0.).ceil() as u32,
                                ))),
                                _ => unreachable!(),
                            };
                            let clip = clip.unwrap_or(PixelRect::full([data[4] as u32, data[5] as u32])).intersect(window);
                            if clip.is_empty() { continue; }
                            pass.set_scissor_rect(
                                clip.min_x(),
                                clip.min_y(),
                                clip.width(),
                                clip.height(),
                            );
                            if let Some(grid) = job_grid(j) {
                                let size=query_grid_size(grid);pass.draw(0..6, 0..size[0]*size[1]);
                            } else {pass.draw(0..3, 0..1);}
                        }
                        drop(pass);
                    }
                    if let Some(source) = source {
                        r.portable_blend.apply(&r.device,encoder,&source,target,PixelRect::full([target.texture().width(),target.texture().height()]),0);
                    }
                    encoded_through = end;
                }
            }
        }
        if let Some(transforms) = &mut r.transforms {
            for job in jobs { if let Job::Transform(region) = job { transforms.finish_region(region); } }
        }
        Ok(())
    }
}

pub(super) fn fill_texture(kind: &RasterOperationKind) -> Option<&layer_core::FillTexture> {
    kind.fill_texture().filter(|texture| texture.strength > 0. && texture.paper.is_some())
}

pub(crate) fn settles(kind: &RasterOperationKind, damage: PixelRect, extent: [u32; 2]) -> bool {
    match kind {
        RasterOperationKind::ApplyMask => true,
        RasterOperationKind::Erase { .. } => damage == PixelRect::full(extent),
        _ => false,
    }
}
pub(crate) fn reads_coverage_pages(kind: &RasterOperationKind) -> bool {
    !matches!(kind, RasterOperationKind::ColorMode(_) | RasterOperationKind::Erase { alpha_locked: true } | RasterOperationKind::Bake { .. }
        | RasterOperationKind::FrequencyDetail { .. } | RasterOperationKind::Transform(_) | RasterOperationKind::Coverage)
}
pub(super) fn operation_pipeline(kind: &RasterOperationKind) -> Option<usize> {
    match kind {
        RasterOperationKind::Fill {edges:Some(edges),..} => Some(if edges.translucent_seed {3}else{10}),
        RasterOperationKind::Fill { .. } => Some(3),
        RasterOperationKind::Gradient { .. } => Some(operation_index(layer_render::OperationUse::Gradient)),
        RasterOperationKind::Figure(_) => Some(operation_index(layer_render::OperationUse::Figure)),
        _ => None,
    }
}

pub(super) fn operation_index(operation: layer_render::OperationUse) -> usize {
    match operation { layer_render::OperationUse::Gradient => 4, layer_render::OperationUse::Figure => 5 }
}

fn shader_source(space: layer_core::color::RgbSpace) -> Cow<'static,str> {
    compose_wgsl(&[&working_color::source(space), &crate::view_color::hdr_shader(space, layer_core::color::RgbSpace::Srgb),
        include_str!("blend_codes.wgsl"), include_str!("blend_modes.wgsl"), include_str!("scene_vertex.wgsl"),
        include_str!("relief_transfer.wgsl"), include_str!("paper_sample.wgsl"), &fill_edges::shader(),include_str!("scene_convert.wgsl"),include_str!("scene.wgsl"),
        "@group(1) @binding(3) var scene_extra:texture_2d<f32>; @group(1) @binding(4) var scene_paper:texture_2d<f32>; @group(1) @binding(5) var paper_sampling:sampler; fn gradient_record(base:u32,index:u32)->vec4<f32>{return textureLoad(scene_extra,vec2<i32>(i32(index),0),0);}",
        include_str!("float_number.wgsl"), crate::gradient::SOURCE, include_str!("scene_constant.wgsl")])
}

impl Pipelines {
    pub fn for_scene(&self, r:&WgpuRasterizer, scene: SceneView<'_>, batches:&[DabBatch]) -> impl Iterator<Item = &Deferred<wgpu::RenderPipeline>> {
        let mut used = [false; FRAGMENTS.len()];
        used[..2].fill(true);
        let [coverage, blend] = composition_pipelines(scene);
        used[2] = coverage;
        used[6] = blend
            || batches.iter().any(|batch| batch.style.execution == BrushExecution::Watercolor)
            || r.paint_layers.iter().any(|layer| layer.watercolor.is_some() && scene.source_owner(layer.id).is_some_and(|index| scene.visible(index)))
            || scene.order().iter().copied().filter(|&index| scene.visible(index) && scene.mask(index).is_some_and(|(mask, _)| mask.enabled)).any(|index|
                scene.source_target(index).is_some_and(|target| r.preview_layer_id == Some(target) && !r.preview_requires_base
                    || batches.iter().any(|batch| batch.target == target && batch.kind == DabBatchKind::Preview)));
        for operation in scene.targets().filter(|target| !target.is_coverage()).filter_map(|target| scene.operations(target)).flatten() {
            if let Some(pipeline)=operation_pipeline(&operation.kind) {used[pipeline]=true;}
            if matches!(operation.kind,RasterOperationKind::Fill {edges:Some(_),underfill:Some(_),..}) {used[fill_line_pipeline(&r.device)]=true;}
        }
        self.pipeline.iter().zip(used).filter_map(|(pipeline,used)| used.then_some(pipeline))
    }
    pub fn fill_pipelines(&self,device:&PipelineDevice)->[&Deferred<wgpu::RenderPipeline>;3] { [&self.pipeline[10],&self.pipeline[3],&self.pipeline[fill_line_pipeline(device)]] }
    pub fn scale_pipelines(&self,format:wgpu::TextureFormat) -> [&Deferred<wgpu::ComputePipeline>; 5] {
        let output=self.scale.output(format);
        [&output.reduce, &output.reduce_phased, &output.reduce_pair, &output.compose, &self.resample.area]
    }
    pub fn effects(&self, r: &WgpuRasterizer) -> effects::Effects {
        effects::Effects::new(r, &self.uniforms)
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let uniforms = crate::bindings::layout(device, "scene records", &[crate::bindings::buffer(
            0,
            wgpu::ShaderStages::VERTEX_FRAGMENT | wgpu::ShaderStages::COMPUTE,
            wgpu::BufferBindingType::Uniform,
            true,
            NonZeroU64::new(144),
        )]);
        let layout = crate::bindings::layout(device, "scene sources", &[
            texture_entry(0, device),
            texture_entry(1, device),
            crate::bindings::sampler(
                2,
                wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE,
                wgpu::SamplerBindingType::Filtering,
            ),
            texture_entry(3, device),
            crate::bindings::texture(4, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, true),
            crate::bindings::sampler(5, wgpu::ShaderStages::FRAGMENT, wgpu::SamplerBindingType::Filtering),
        ]);
        let edges=fill_edges::layout(device);
        let shader = Deferred::new({
            let device = device.for_recipe();
            move || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("layer scene"),
                    source: wgpu::ShaderSource::Wgsl(shader_source(device.working_space())),
                })
            }
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene composition"),
            bind_group_layouts: &[Some(&uniforms), Some(&layout),Some(&edges)],
            immediate_size: 0,
        });
        let mut pipeline: Vec<Deferred<wgpu::RenderPipeline>> = Vec::with_capacity(FRAGMENTS.len());
        for (i, (entry,label)) in FRAGMENTS.into_iter().enumerate() {
            if i == 1 && device.attachment_blend(device.working_format(), Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)).is_none() {
                pipeline.push(pipeline[0].clone());
                continue;
            }
            let (device, pipeline_layout, shader) =
                (device.for_recipe(), pipeline_layout.clone(), shader.clone());
            pipeline.push(Deferred::pipeline(move |mode| {
                fullscreen_pipeline_recipe(mode, &device, &pipeline_layout, &shader, entry,
                    (i == 1 || i == FILL_LINES_PIPELINE).then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    if i == 2 || i==FILL_LINES_PIPELINE { device.scalar_format() } else { device.working_format() }, label)
            }));
        }
        let pipeline = pipeline.try_into().ok().unwrap();
        let constant = {
            let inputs = crate::bindings::layout(device, "scene normal stack inputs", &[
                texture_entry(0, device), texture_entry(1, device),
                crate::bindings::sampler(2, wgpu::ShaderStages::COMPUTE, wgpu::SamplerBindingType::Filtering),
                texture_entry(3, device),
            ]);
            let output = crate::bindings::layout(device, "scene constant backdrop output", &[crate::bindings::storage_texture(
                0,
                wgpu::ShaderStages::COMPUTE,
                wgpu::TextureFormat::Rgba32Float,
                wgpu::StorageTextureAccess::WriteOnly,
            )]);
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene constant backdrop"),
                bind_group_layouts: &[Some(&uniforms), Some(&inputs), Some(&output)],
                immediate_size: 0,
            });
            let pipeline = Deferred::compute(device, "scene constant backdrop", &layout, &shader, "compose_constant");
            (output, inputs, pipeline)
        };
        let fold = fold::Pipelines::new(device, &constant.0);
        Self {
            objects: crate::object_sampling::ObjectSampler::new(device),
            vector: crate::vector::Kernel::new(device),
            scale: scale::Pipelines::new(device),
            resample: resample::Resample::new(device),
            fine_resample: resample::Resample::with_fine(device,true),
            source: sources::Pipelines::new(device, &uniforms),
            constant,fold,edges,
            uniforms,
            layout,
            pipeline,
        }
    }
}

fn direct_effect_mask(scene: SceneView<'_>, handle: OccurrenceHandle) -> bool {
    scene.mask(handle).is_none_or(|(use_, _)| !use_.enabled || scene.target_offset(SourceTarget::Coverage(use_.source)) == [0; 2])
}
fn texture_entry(binding: u32, device: &PipelineDevice) -> wgpu::BindGroupLayoutEntry {
    crate::bindings::float_texture(binding, wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE, device)
}
fn uniform_binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    crate::bindings::group(device, "scene uniform binding", layout, [
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer, offset: 0, size: NonZeroU64::new(144), }),
    ])
}
fn local_rect(c: [u32; 2], offset: [i64; 2], tile: [u32; 2]) -> [f32; 4] {
    let at = |i: usize| ((i64::from(c[i]) - i64::from(tile[i])) * i64::from(PAGE_SIZE) + offset[i]) as f32;
    [at(0), at(1), PAGE_SIZE as f32, PAGE_SIZE as f32]
}
fn intersects(r: [f32; 4]) -> bool {
    r[0] < 256. && r[1] < 256. && r[0] + r[2] > 0. && r[1] + r[3] > 0.
}
/// A layer's pixels placed in the document, for the samplers that transform
/// previews and pixel commits share.
/// How far right and down of a page corner `origin` lies.
pub(crate) fn page_phase(origin: [i64; 2]) -> [u32; 2] {
    origin.map(|value| value.rem_euclid(i64::from(PAGE_SIZE)) as u32)
}
pub(super) fn world_offset(scene: SceneView<'_>, handle: OccurrenceHandle, mask: bool) -> [i64; 2] {
    if mask { scene.mask(handle).map_or_else(Default::default, |(use_, _)| scene.target_offset(SourceTarget::Coverage(use_.source))) }
    else { scene.occurrence_offset(handle) }
}
fn attachment(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}
fn descriptor<'a>(
    attachments: &'a [Option<wgpu::RenderPassColorAttachment<'a>>],
) -> wgpu::RenderPassDescriptor<'a> {
    wgpu::RenderPassDescriptor {
        label: Some("layer tile scene"),
        color_attachments: attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }
}

fn constant_frame_tile(scene:SceneView<'_>,tile:[u32;2],window:Option<DocRect>)->bool {
    let start=scene.evaluation_offset64();let extent=scene.composition().size;
    let low=tile.map(|coordinate|i64::from(coordinate)*i64::from(PAGE_SIZE));
    let region=DocRect {min:low,max:low.map(|coordinate|coordinate+i64::from(PAGE_SIZE))};
    let region=window.map_or(region,|window|region.intersect(window));
    !region.is_empty()&&(0..2).all(|axis|region.min[axis] as f64>=start[axis] && region.max[axis] as f64<=start[axis]+f64::from(extent[axis]))
}
fn fusable_adjustment(scene: SceneView<'_>, handle: OccurrenceHandle) -> bool {
    scene.effect(handle).is_some_and(|effect| effect.program.kind == layer_core::EffectKind::Adjustment && !effect.program.fusion_boundary())
}
fn fuses_after(scene: SceneView<'_>, head: OccurrenceHandle, next: OccurrenceHandle, chain: usize) -> bool {
    let occurrence = scene.occurrence(next).unwrap();
    scene.visible(next) && scene.effect_owner(next) == scene.effect_owner(head)
        && scene.effect(next).is_some_and(|next| scene.effect(head).is_some_and(|head| next.program.space == head.program.space))
        && direct_effect_mask(scene, next)
        && !(chain >= effects::MASK_SLOTS && occurrence.mask.as_ref().is_some_and(|mask| mask.enabled))
        && fusable_adjustment(scene, next)
}
pub(super) enum EffectPreparation { Display, Capture }

pub(super) fn effect_chains(scene: SceneView<'_>, preparation:EffectPreparation) -> Vec<(Vec<OccurrenceHandle>, effects::Execution)> {
    let mut result = Vec::new();
    let mut parents = std::collections::BTreeSet::new();
    for &handle in scene.order() {
        parents.insert(scene.evaluation_parent(handle));
        if !scene.visible(handle) { continue; }
        if let Some(effect) = scene.effect(handle).filter(|effect| effect.constant_color().is_none()
            || matches!(preparation,EffectPreparation::Capture) || stack::support(scene,0)!=Some(0)) {
            if effect.program.image_boundary() {
                for pass in 0..effect.program.passes.len().max(1) { result.push((vec![handle], effects::Execution::Image(pass))); }
                if effect.program.passes.is_empty() && effect.program.resolution==layer_core::EffectResolution::Display {
                    result.push((vec![handle], effects::Execution::Fused));
                }
            } else {
                result.push((vec![handle], effects::Execution::Fused));
                if effect.program.resolution==layer_core::EffectResolution::Native
                    && effect.program.alpha==layer_core::EffectAlpha::Filter && !effect.program.fusion_boundary() {
                    result.push((vec![handle],effects::Execution::Reduced));
                    result.push((vec![handle],effects::Execution::ReducedContribution));
                }
            }
        }
    }
    for parent in parents {
        let mut siblings = scene.members(parent).rev().filter(|handle| scene.occurrence(*handle).unwrap().is_artwork() && scene.visible(*handle)).peekable();
        while let Some(handle) = siblings.next() {
            if !scene.visible(handle) || !direct_effect_mask(scene, handle) || !fusable_adjustment(scene, handle) { continue; }
            let mut chain = vec![handle];
            while let Some(next) = siblings.peek().filter(|next| fuses_after(scene, handle, **next, chain.len())) {
                chain.push(*next); siblings.next();
            }
            if chain.len() > 1 { result.push((chain, effects::Execution::Fused)); }
        }
    }
    result
}

/// Bind groups by the views they bind, kept while the current or previous
/// frame uses them: a drag redraws the same pages every frame. A scene that
/// never begins frames keeps at most BINDINGS entries.
const BINDINGS: usize = 4096;
pub(super) struct RecentBindings<K> {
    frame: u64,
    entries: std::collections::HashMap<K, (u64, wgpu::BindGroup), layer_core::authored::vector::IdBuild>,
}
impl<K> Default for RecentBindings<K> {
    fn default() -> Self {
        Self { frame: 0, entries: Default::default() }
    }
}
#[allow(clippy::mutable_key_type)] // Texture views hash by stable resource identity.
impl<K: std::hash::Hash + Eq + Clone> RecentBindings<K> {
    fn forget(&mut self, matches: impl Fn(&K) -> bool) {
        self.entries.retain(|key, _| !matches(key));
    }
    pub(super) fn begin_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.entries.retain(|_, (used, _)| frame - *used <= 1);
    }
    pub(super) fn get(&mut self, key: &K, create: impl FnOnce() -> wgpu::BindGroup) -> &wgpu::BindGroup {
        self.get_by(key, || key.clone(), |_| create())
    }
    pub(super) fn get_by<Q: std::hash::Hash + Eq + ?Sized>(&mut self, query: &Q, key: impl FnOnce() -> K,
        create: impl FnOnce(&K) -> wgpu::BindGroup) -> &wgpu::BindGroup where K: std::borrow::Borrow<Q> {
        let frame = self.frame;
        if let Some(entry) = self.entries.get_mut(query) { entry.0 = frame; } else {
            if self.entries.len() >= BINDINGS {
                self.frame += 1;
                self.entries.clear();
            }
            let key = key();
            let group = create(&key);
            self.entries.insert(key, (self.frame, group));
        }
        &self.entries.get(query).unwrap().1
    }
}
pub(super) type ResourceBindingKey<const B: usize, const V: usize> = ([wgpu::Buffer; B], [wgpu::TextureView; V]);
pub(super) type MaterialBindingKey = ResourceBindingKey<2, 11>;
pub(super) type TransportBindingKey = ResourceBindingKey<1, 12>;
pub(super) trait ResourceBinding<const B: usize, const V: usize> {
    fn parts(&self) -> ([&wgpu::Buffer; B], [&wgpu::TextureView; V]);
}
impl<const B: usize, const V: usize> ResourceBinding<B, V> for ResourceBindingKey<B, V> {
    fn parts(&self) -> ([&wgpu::Buffer; B], [&wgpu::TextureView; V]) { (self.0.each_ref(), self.1.each_ref()) }
}
impl<const B: usize, const V: usize> ResourceBinding<B, V> for ([&wgpu::Buffer; B], [&wgpu::TextureView; V]) {
    fn parts(&self) -> ([&wgpu::Buffer; B], [&wgpu::TextureView; V]) { *self }
}
impl<const B: usize, const V: usize> std::hash::Hash for dyn ResourceBinding<B, V> + '_ {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.parts().hash(state); }
}
impl<const B: usize, const V: usize> PartialEq for dyn ResourceBinding<B, V> + '_ {
    fn eq(&self, other: &Self) -> bool { self.parts() == other.parts() }
}
impl<const B: usize, const V: usize> Eq for dyn ResourceBinding<B, V> + '_ {}
impl<'a, const B: usize, const V: usize> std::borrow::Borrow<dyn ResourceBinding<B, V> + 'a> for ResourceBindingKey<B, V> {
    fn borrow(&self) -> &(dyn ResourceBinding<B, V> + 'a) { self }
}

fn compute_source_binding(r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout, sources: &[wgpu::TextureView; 4]) -> wgpu::BindGroup {
    crate::bindings::group(&r.device, "scene normal stack inputs", layout, [
        wgpu::BindingResource::TextureView(&sources[0]),
        wgpu::BindingResource::TextureView(&sources[1]),
        wgpu::BindingResource::Sampler(&r.sampler),
        wgpu::BindingResource::TextureView(&sources[2]),
    ])
}

fn source_binding(r: &WgpuRasterizer, layout: &wgpu::BindGroupLayout, sources: &[wgpu::TextureView; 4], extra:bool) -> wgpu::BindGroup {
    let entries=[wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(&sources[0])},
        wgpu::BindGroupEntry {binding:1,resource:wgpu::BindingResource::TextureView(&sources[1])},
        wgpu::BindGroupEntry {binding:2,resource:wgpu::BindingResource::Sampler(&r.sampler)},
        wgpu::BindGroupEntry {binding:3,resource:wgpu::BindingResource::TextureView(&sources[2])},
        wgpu::BindGroupEntry {binding:4,resource:wgpu::BindingResource::TextureView(&sources[3])},
        wgpu::BindGroupEntry {binding:5,resource:wgpu::BindingResource::Sampler(&r.brush_sampler)}];
    r.device.create_bind_group(&wgpu::BindGroupDescriptor {label:Some("scene tile inputs"),layout,entries:&entries[..if extra {6} else {3}]})
}

fn query_grid_size([x,y,w,h]:[u32;4])->[u32;2] {
    [(x,w),(y,h)].map(|(origin,extent)| (0..extent.min(256)).filter(|i| {
        let p=(2*i+1)*extent/(2*extent.min(256));p>=origin && p<origin+PAGE_SIZE
    }).count() as u32)
}

#[cfg(test)]
#[path="scene/committed_pages_tests.rs"]
mod committed_pages_tests;

#[cfg(test)]
mod capture_readiness_tests;

#[cfg(test)]
mod prediction_tests;

#[cfg(test)]
mod fold_tests;

#[cfg(test)]
mod shader_tests {
    use super::*;

    #[test]
    fn fallback_composition_reuses_the_unblended_recipe() {
        let adapter = pollster::block_on(WgpuRasterizer::headless_instance().request_adapter(&Default::default())).unwrap();
        let (device, _) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let device = PipelineDevice::from(device);
        assert!(device.attachment_blend(device.working_format(), Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)).is_none());
        let pipelines = Pipelines::new(&device);
        let scope=device.compilation().unwrap();
        pollster::block_on(scope.prepare_all([&pipelines.pipeline[0],&pipelines.pipeline[1]])).unwrap();
        assert!(std::ptr::eq(pipelines.pipeline[0].compile(), pipelines.pipeline[1].compile()));
    }

    #[test]
    fn scene_conversion_preserves_every_code_and_foreign_values() {
        let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let output = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
        for space in layer_core::color::RgbSpace::ALL {
            let source = format!("{}\n{}\n{}", working_color::source(space), include_str!("scene_convert.wgsl"), "
                @group(0) @binding(0) var output:texture_storage_2d<rgba32float,write>;
                fn expected(c:vec4<f32>,code:f32)->vec4<f32> {
                    if code==1. {return working_encode(c);}
                    if code==2. {return working_decode(c);}
                    if code==3. {return working_clamp(c);}
                    if code==4. {return working_clamp(working_decode(c));}
                    return c;
                }
                @compute @workgroup_size(1) fn check(@builtin(global_invocation_id) id:vec3<u32>) {
                    let codes=array<f32,9>(0.,1.,2.,3.,4.,5.,-1.,bitcast<f32>(0x7fc00000u),6.);
                    let color=vec4<f32>(1.4,-.2,.7,f32(id.x)/4.);
                    textureStore(output,vec2<i32>(id.xy),scene_convert(color,codes[id.y]));
                    textureStore(output,vec2<i32>(i32(id.x),i32(id.y)+16),expected(color,codes[id.y]));
                }
            ");
            let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
            let pipeline = r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None, layout: None, module: &shader, entry_point: Some("check"), compilation_options: Default::default(), cache: None,
            });
            let binding = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None, layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&output.create_view(&Default::default())) }],
            });
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                pass.set_pipeline(&pipeline); pass.set_bind_group(0, &binding, &[]); pass.dispatch_workgroups(8, 9, 1);
            }
            encoder.submit(&r.queue);
            let pixels = crate::test_support::float_pixels(&r, &output);
            for y in 0..9 { for x in 0..8 {
                assert_eq!(pixels[y*256+x].map(f32::to_bits), pixels[(y+16)*256+x].map(f32::to_bits), "{space:?}/{x}/{y}");
            }}
        }
    }

    #[test]
    fn fragment_operations_select_only_their_assigned_pipeline() {
        assert_eq!(FRAGMENTS.map(|(entry,_)| entry).as_slice(), ["fragment_main","fragment_main","fragment_coverage",
            "fragment_fill","fragment_gradient","fragment_figure","fragment_blend","fragment_image","fragment_tone","fragment_fill_lines","fragment_fill_opaque"]);
        let expected = [
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [Some(6),Some(6)],
            [Some(0),Some(1)],
            [Some(3),Some(3)],
            [None,None],
            [None,None],
            [None,None],
            [Some(8),Some(8)],
            [Some(5),Some(5)],
            [Some(7),Some(7)],
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [None,None],
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [Some(4),Some(4)],
            [Some(0),Some(1)],
            [Some(0),Some(1)],
            [Some(2),Some(2)],
            [Some(2),Some(2)],
            [Some(0),Some(1)],
        ];
        for (operation,pipelines) in expected.into_iter().enumerate() {
            for over in [false,true] {
                if matches!(operation,7..=9) {
                    assert!(std::panic::catch_unwind(|| fragment_pipeline(operation as u32,over,false)).is_err(),
                        "undefined operation {operation} over={over} selected a fragment pipeline");
                } else {
                    assert_eq!(fragment_pipeline(operation as u32,over,false),pipelines[usize::from(over)],
                        "operation {operation} over={over}");
                }
            }
        }
        for operation in [0,1,2,3,5,13,14,16,17,19,20] {
            for over in [false,true] {assert_eq!(fragment_pipeline(operation,over,true),Some(usize::from(over)));}
        }
        for over in [false,true] {assert_eq!(fragment_pipeline(23,over,true),Some(FILL_LINES_PIPELINE));}
        let document=layer_core::Document::new(layer_core::PortableId::random(),16,16,
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut fill=RasterOperationKind::Fill {color:[1.;4],texture:layer_core::FillTexture {strength:1.,paper:None},
            alpha_locked:false,edges:None,underfill:None};
        assert_eq!(operation_pipeline(&fill),Some(3));
        for translucent_seed in [false,true] {
            if let RasterOperationKind::Fill {edges,..}=&mut fill {*edges=Some(Box::new(layer_core::FillEdges {
                translucent_seed,colour_on_solid:false,snapshot:document.snapshot(),position:layer_core::Point{x:0.,y:0.},tolerance:0.,lines:Vec::new(),
            }));}
            assert_eq!(operation_pipeline(&fill),Some(if translucent_seed {3}else{10}));
        }
        for operation in [u32::MAX] {
            for over in [false,true] {
                assert!(std::panic::catch_unwind(|| fragment_pipeline(operation,over,false)).is_err(),
                    "unknown operation {operation} over={over} selected a fragment pipeline");
            }
        }
    }

    #[test]
    fn fragment_families_omit_unrelated_operations() {
        let families = ["gradient_shape", "ellipse_distance", "scene_image", "hdr_map_sdr", "blend_layer", "blend_composite", "blend_clip", "paper_sample",
            "fill_plateau_of", "fill_anchor", "fill_feasible_of", "fill_reconstruct_of"];
        for space in layer_core::color::RgbSpace::ALL {
            let source = shader_source(space);
            let module = crate::shader_limits::ValidatedModule::parse(&source).unwrap().into_parts().0;
            for (entry, required) in [
                ("fragment_main", &[][..]),
                ("fragment_coverage", &[][..]),
                ("fragment_fill_lines", &[][..]),
                ("fragment_fill", &["paper_sample"][..]),
                ("fragment_fill_opaque", &["paper_sample", "fill_plateau_of", "fill_anchor", "fill_feasible_of", "fill_reconstruct_of"][..]),
                ("fragment_gradient", &["gradient_shape", "paper_sample"][..]),
                ("fragment_figure", &["ellipse_distance"][..]),
                ("fragment_blend", &["blend_layer"][..]),
                ("fragment_image", &["scene_image"][..]),
                ("fragment_tone", &["hdr_map_sdr"][..]),
                ("compose_constant", &[][..]),
            ] {
                let mut selected = module.clone();
                selected.entry_points.retain(|e| e.name == entry);
                assert_eq!(selected.entry_points.len(), 1, "{entry}/{space:?}");
                naga::compact::compact(&mut selected, naga::compact::KeepUnused::No);
                for family in families {
                    let retained = selected.functions.iter().any(|(_, function)| function.name.as_deref() == Some(family));
                    assert_eq!(retained, required.contains(&family), "{entry}/{space:?} family {family}");
                }
            }
        }
    }
}

#[cfg(all(test,not(target_arch="wasm32")))]
mod terminal_tests {
    use super::*;
    #[test]
    fn retired_device_constructors_do_not_panic_on_a_valid_device() {
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter().get_info().device_type,wgpu::DeviceType::Cpu);
        r.renderer_factory().retire_device_services().unwrap();
        crate::finish_shader_compiler_shutdown();
        let image=Image::new(&r,display_mips::Plan::new([16,16]).unwrap(),"retired device consumer");
        assert!(image.bytes()>0);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let receipt=encoder.submission();
        assert!(encoder.submit(&r.queue).is_none());
        assert!(receipt.discarded());
    }
}
