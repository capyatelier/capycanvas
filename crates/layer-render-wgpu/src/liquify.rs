use super::*;
use layer_core::Rect;
use std::{collections::{BTreeMap, BTreeSet}, sync::{Mutex, Weak, atomic::{AtomicBool, AtomicU64, Ordering}}};

const ORIGINAL_LAYERS: u32 = 16;
const ORIGINAL_BINDINGS: usize = 11;
const CHUNK_TILES: u32 = ORIGINAL_BINDINGS as u32 * ORIGINAL_LAYERS;
pub(super) const ORIGINAL_ATLAS_BYTES: u64 = 87381 * ORIGINAL_LAYERS as u64 * 16;
const MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;
static NEXT_RESOURCE: AtomicU64 = AtomicU64::new(1);

fn original_chunk(coordinate: [u32; 2], extent: [u32; 2]) -> u32 {
    coordinate[1] / ORIGINAL_BINDINGS as u32 * extent[0].div_ceil(PAGE_SIZE).div_ceil(ORIGINAL_LAYERS) + coordinate[0] / ORIGINAL_LAYERS
}
fn new_atlases(missing: impl IntoIterator<Item = usize>) -> u64 {
    missing.into_iter().map(|count| (count as u64).div_ceil(u64::from(ORIGINAL_LAYERS))).sum()
}

fn map_bindings(mode: u32) -> usize {
    if matches!(mode, 0..=2 | 6) { 12 } else { 9 }
}

#[derive(Clone)]
struct TileStorage {texture:wgpu::Texture,view:wgpu::TextureView,_charge:Arc<crate::residency::Charge>}
impl TileStorage {
    fn new(r:&WgpuRasterizer,block:u32)->Self {
        let (texture,view)=create_target(&r.device,[PAGE_SIZE/block;2],MAP_FORMAT,"liquify displacement and footprint");
        Self {_charge:Arc::new(r.raster_tiers.paint.charge(texture_bytes(&texture))),texture,view}
    }
}
impl resident::Storage for TileStorage {fn storage_bytes(&self)->u64 {texture_bytes(&self.texture)}}
impl resident::TextureStorage for TileStorage {
    fn texture(&self)->&wgpu::Texture {&self.texture}
    fn view(&self)->&wgpu::TextureView {&self.view}
}
struct Tile {
    id:u64,
    capture:layer_core::raster::ContentVersion,
    pixels:resident::Resident<TileStorage>,
    bounds:Rect,
}
impl std::fmt::Debug for Tile {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.debug_struct("Tile").field("id",&self.id).field("bounds",&self.bounds).finish()}
}
impl Tile {
    fn at_block(r:&WgpuRasterizer,bounds:Rect,block:u32)->Self {
        Self {id:NEXT_RESOURCE.fetch_add(1,Ordering::Relaxed),capture:layer_core::raster::ContentName::fresh().into(),
            pixels:resident::Resident::unwritten(TileStorage::new(r,block)),bounds}
    }
    fn read(&self,receipt:&submission::Submission)->Result<resident::Read<'_,TileStorage>,GpuRasterError> {
        self.pixels.read(&self.capture,Some(receipt)).ok_or(GpuRasterError::NotReady)
    }
    fn source_bounds(&self,extent:[u32;2],preview:bool)->PixelRect {field_source_bounds(self.bounds,extent,preview)}
}

fn decodes_original(source:&layer_core::authored::PaintSource,coordinate:[u32;2])->bool {
    source.raster.try_data().and_then(Result::ok).is_none_or(|data|data.tiles.contains_key(&layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate}))
        || source.base.as_ref().is_some_and(|base|source_access::paint_base_contains(base,coordinate))
}
fn field_source_bounds(bounds:Rect,extent:[u32;2],preview:bool)->PixelRect {
    let diameter=(bounds.max.x-bounds.min.x).hypot(bounds.max.y-bounds.min.y);
    let level=(diameter/if preview {2.}else {4.}).max(1.).log2().ceil().clamp(0.,8.) as u32;
    pixel_rect(material_sources::expand(bounds,(1u32<<level) as f32),extent)
}
fn field_input_bounds(batch:&DabBatch,dabs:&[Dab],coordinate:[u32;2],extent:[u32;2],fields:impl Iterator<Item=([u32;2],Rect)>)->(PixelRect,Rect) {
    let trace=if batch.style.liquify.as_ref().is_some_and(|c|c.amount.is_some()) {page_rect(coordinate).to_rect()}else {material_sources::sample_rect(batch,dabs,coordinate)};
    let input=pixel_rect(trace,extent).expand(1,extent);
    (input,fields.filter(|(at,_)|!page_rect(*at).intersect(input).is_empty()).fold(trace,|bounds,(_,field)|bounds.union(field)))
}

struct OriginalTile {
    capture:layer_core::raster::ContentVersion,
    index:u32,
    surface:resident::Resident<source_access::RawTile>,
}
impl std::fmt::Debug for OriginalTile {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.debug_struct("OriginalTile").field("index",&self.index).finish()}
}
impl OriginalTile {
    fn read(&self,receipt:&submission::Submission)->Result<resident::Read<'_,source_access::RawTile>,GpuRasterError> {
        self.surface.read(&self.capture,Some(receipt)).ok_or(GpuRasterError::NotReady)
    }
}

#[derive(Debug)]
struct OriginalAtlas {
    id: u64,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    held: bool,
    _charge: crate::residency::Charge,
}

pub(crate) struct Originals {
    versions: raster::contents::Versions,
    tiles: BTreeMap<[u32; 2], Arc<OriginalTile>>,
    atlases: Vec<Option<OriginalAtlas>>,
    free: BTreeSet<u32>,
    idle: Arc<AtomicU64>,
}
impl Originals {
    fn new(versions:raster::contents::Versions,idle:Arc<AtomicU64>)->Self {Self {versions,tiles:BTreeMap::new(),atlases:Vec::new(),free:BTreeSet::new(),idle}}
    fn set_held(&mut self,atlas:usize,held:bool) {
        let Some(atlas)=self.atlases.get_mut(atlas).and_then(Option::as_mut).filter(|atlas|atlas.held!=held) else {return;};
        atlas.held=held;
        if held {self.idle.fetch_sub(ORIGINAL_ATLAS_BYTES,Ordering::SeqCst);} else {self.idle.fetch_add(ORIGINAL_ATLAS_BYTES,Ordering::SeqCst);}
    }
    fn readable(&self,coordinate:[u32;2])->bool {
        self.tiles.get(&coordinate).is_some_and(|tile|tile.capture.matches(&self.versions.tile(layer_core::raster::RasterPlane::Color,coordinate))==Some(true)
            && tile.surface.read(&tile.capture,None).is_some())
    }
    fn idle_atlases(&self)->impl Iterator<Item=usize>+'_ {
        self.atlases.iter().enumerate().filter(|(_,atlas)|atlas.as_ref().is_some_and(|atlas|!atlas.held)).map(|(at,_)|at)
    }
    fn release(&mut self) {for atlas in 0..self.atlases.len() {self.set_held(atlas,false);}}
    fn hold(&mut self,needed:&BTreeSet<[u32;2]>,extent:[u32;2])->u64 {
        let (mut held,mut missing)=(Vec::new(),BTreeMap::<u32,usize>::new());
        for at in needed {match self.tiles.get(at) {Some(tile)=>held.push(tile.index),None=>*missing.entry(original_chunk(*at,extent)).or_default()+=1}}
        let mut unplaced=Vec::new();
        for (chunk,count) in missing {
            let reused=self.free.range(chunk*CHUNK_TILES..(chunk+1)*CHUNK_TILES).copied().take(count).collect::<Vec<_>>();
            unplaced.push(count-reused.len());
            held.extend(reused);
        }
        for index in held {self.set_held((index/ORIGINAL_LAYERS) as usize,true);}
        new_atlases(unplaced)
    }
    fn evict(&mut self,atlas:usize)->Option<wgpu::Texture> {
        self.set_held(atlas,true);
        let OriginalAtlas {texture,..}=self.atlases.get_mut(atlas)?.take()?;
        self.tiles.retain(|_,tile|(tile.index/ORIGINAL_LAYERS) as usize!=atlas);
        self.free.retain(|index|(index/ORIGINAL_LAYERS) as usize!=atlas);
        Some(texture)
    }
}
impl Drop for Originals {
    fn drop(&mut self) {for atlas in 0..self.atlases.len() {self.set_held(atlas,true);}}
}
impl std::fmt::Debug for Originals {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        f.debug_struct("Originals").field("tiles",&self.tiles).field("atlases",&self.atlases).finish()
    }
}

#[derive(Debug)]
struct Field {
    content: layer_core::raster::ContentName,
    device: wgpu::Device,
    tiles: BTreeMap<[u32; 2], Arc<Tile>>,
    originals: Arc<Mutex<Originals>>,
}

impl layer_core::liquify::Field for Field {
    fn content_version(&self)->layer_core::raster::ContentVersion {self.content.into()}
    fn tiles(&self) -> Vec<[u32; 2]> { self.tiles.keys().copied().collect() }
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn resources(&self, visit: &mut dyn FnMut(u64, usize)) {
        for tile in self.tiles.values() { visit(tile.id, tile.pixels.storage_bytes() as usize); }
        for atlas in self.originals.lock().expect("liquify sources").atlases.iter().flatten() { visit(atlas.id, ORIGINAL_ATLAS_BYTES as usize); }
    }
}

struct PendingField {
    receipt:submission::Submission,
    tiles:BTreeMap<[u32;2],Option<Arc<Tile>>>,
    colored:BTreeMap<[u32;2],Option<Arc<Tile>>>,
    dirty:BTreeMap<[u32;2],Option<PixelRect>>,
    content:layer_core::raster::ContentName,
}
struct Stroke {
    pending:Option<PendingField>,
    source:scene::Input,
    id: StrokeId,
    preview_only: bool,
    original: Arc<layer_core::authored::PaintSource>,
    field: Field,
    colored_tiles: BTreeMap<[u32; 2], Arc<Tile>>,
    preview_tiles: BTreeMap<[u32; 2], ([u64; 2], layer_core::raster::ContentVersion)>,
    dirty: BTreeMap<[u32; 2], PixelRect>,
}

impl Stroke {
    fn resolve(&mut self) {
        if self.pending.as_ref().is_some_and(|pending|pending.receipt.discarded()) {self.pending=None;}
        if self.pending.as_ref().is_some_and(|pending|pending.receipt.submitted()) {
            let pending=self.pending.take().unwrap();self.field.content=pending.content;
            for (at,tile) in pending.tiles {match tile {Some(tile)=>{self.field.tiles.insert(at,tile);},None=>{self.field.tiles.remove(&at);}}}
            for (at,tile) in pending.colored {match tile {Some(tile)=>{self.colored_tiles.insert(at,tile);},None=>{self.colored_tiles.remove(&at);}}}
            for (at,area) in pending.dirty {match area {Some(area)=>{self.dirty.insert(at,area);},None=>{self.dirty.remove(&at);}}}
        }
    }
    fn stage(&mut self,receipt:&submission::Submission)->Result<(),GpuRasterError> {
        self.resolve();
        if self.pending.as_ref().is_some_and(|pending|!pending.receipt.same(receipt)) {return Err(GpuRasterError::Invariant("prepared stage state is inconsistent"));}
        self.pending.get_or_insert_with(||PendingField {receipt:receipt.clone(),tiles:BTreeMap::new(),colored:BTreeMap::new(),dirty:BTreeMap::new(),content:self.field.content});
        Ok(())
    }
    fn tiles(&self)->impl Iterator<Item=(&[u32;2],&Arc<Tile>)> {
        self.field.tiles.iter().filter(|(at,_)|self.pending.as_ref().is_none_or(|pending|!pending.tiles.contains_key(*at)))
            .chain(self.pending.iter().flat_map(|pending|pending.tiles.iter().filter_map(|(at,tile)|tile.as_ref().map(|tile|(at,tile)))))
    }
    fn tile(&self,at:&[u32;2])->Option<&Arc<Tile>> {
        self.pending.as_ref().and_then(|pending|pending.tiles.get(at)).map_or_else(||self.field.tiles.get(at),Option::as_ref)
    }
    fn colored(&self,at:&[u32;2])->Option<&Arc<Tile>> {
        self.pending.as_ref().and_then(|pending|pending.colored.get(at)).map_or_else(||self.colored_tiles.get(at),Option::as_ref)
    }
    fn preview_inputs(&self,at:&[u32;2])->[u64;2] {[self.tile(at),self.colored(at)].map(|tile|tile.map_or(0,|tile|tile.id))}
    fn dirty_at(&self,at:&[u32;2])->Option<PixelRect> {
        self.pending.as_ref().and_then(|pending|pending.dirty.get(at)).map_or_else(||self.dirty.get(at).copied(),|area|*area)
    }
    fn dirty(&self)->impl Iterator<Item=([u32;2],PixelRect)>+'_ {
        self.dirty.iter().filter(|(at,_)|self.pending.as_ref().is_none_or(|pending|!pending.dirty.contains_key(*at))).map(|(at,area)|(*at,*area))
            .chain(self.pending.iter().flat_map(|pending|pending.dirty.iter().filter_map(|(at,area)|area.map(|area|(*at,area)))))
    }
}

pub(super) struct Liquify {
    pub pipelines: Arc<Pipelines>,
    strokes: BTreeMap<SourceTarget, Stroke>,
    unchanged_previews: std::collections::BTreeSet<(SourceTarget, [u32; 2])>,
    empty: TileStorage,
    scratch_map: [TileStorage; 2],
    partials: Partials,
    sessions: Vec<Weak<Mutex<Originals>>>,
    idle: Arc<AtomicU64>,
}

#[derive(Default)]
struct Partials {
    block: u32,
    surfaces: Vec<(wgpu::Texture, wgpu::TextureView, crate::residency::Charge)>,
}
impl Partials {
    fn reserve(&mut self, r: &WgpuRasterizer, encoder: &submission::CommandEncoder, block: u32, wanted: usize) -> usize {
        if self.block != block && wanted != 0 {
            let retired = std::mem::replace(self, Self { block, surfaces: Vec::new() }).surfaces;
            #[cfg(not(target_arch = "wasm32"))]
            if !retired.is_empty() { encoder.retire(retired); }
            #[cfg(target_arch = "wasm32")]
            if !retired.is_empty() { encoder.retire(retired.into_iter().map(|(_, _, charge)| charge).collect::<Vec<_>>()); }
        }
        while self.surfaces.len() < wanted.min(2 * (block * block) as usize) {
            let (texture, view) = create_target(&r.device, [PAGE_SIZE / block; 2], r.device.working_format(), "liquify color gather");
            let charge = r.raster_tiers.paint.charge(texture_bytes(&texture));
            self.surfaces.push((texture, view, charge));
        }
        self.surfaces.len()
    }
}

#[derive(Default)]
pub(super) struct OriginalDemand {
    pub atlases: u64,
    pub staged: Vec<(SourceTarget, Arc<layer_core::authored::PaintSource>, Arc<Mutex<Originals>>, Vec<[u32; 2]>)>,
}

struct Session {
    stroke: StrokeId,
    fields: BTreeMap<[u32; 2], Rect>,
    originals: Option<Arc<Mutex<Originals>>>,
    source: Option<Arc<layer_core::authored::PaintSource>>,
    needed: BTreeSet<[u32; 2]>,
    staged: BTreeSet<[u32; 2]>,
}

struct Dispatch {
    output: wgpu::BindGroup,
    sources: wgpu::BindGroup,
    coordinate: [u32; 2],
    record: u32,
}

pub(super) struct Pipelines {
    map_layout: wgpu::BindGroupLayout,
    color_layout: wgpu::BindGroupLayout,
    map_sources: [wgpu::BindGroupLayout; 2],
    color_sources: wgpu::BindGroupLayout,
    update: [Deferred<wgpu::ComputePipeline>; 16],
    pub reconstruct: Deferred<wgpu::ComputePipeline>,
    pub mips: display_mips::Pipelines,
}

fn source_bindings(integer: bool, count: usize) -> String {
    let sample = if integer { "u32" } else { "f32" };
    let dimension = if integer { "texture_2d" } else { "texture_2d_array" };
    let mut source = (0..count).map(|i| format!("@group(2) @binding({i}) var source_{i}: {dimension}<{sample}>;\n")).collect::<String>();
    source += &format!("fn load_source(index:u32,pixel:vec2<i32>{})->vec4<{sample}> {{ switch index {{\n",
        if integer { "" } else { ",layer:i32,level:i32" });
    for i in 0..count {
        source += &format!("case {i}u: {{return textureLoad(source_{i},pixel,{});}}\n", if integer { "0" } else { "layer,level" });
    }
    source += &format!("default: {{return vec4<{sample}>(0);}} }} }}\n");
    if integer {
        source += &format!("const MAP_BINDINGS: u32 = {count}u;\n@group(2) @binding({count}) var<uniform> sources: Sources;\n@group(2) @binding({}) var previous_map: texture_2d<u32>;\n", count + 1);
    }
    source
}

fn map_shader(mode: u32, analytic: bool) -> String {
    let source = compose_wgsl(&[&format!("const LIQUIFY_MODE: u32 = {mode}u;\nconst LIQUIFY_ANALYTIC: bool = {analytic};"),
        include_str!("brush_types.wgsl"), include_str!("brush_textures.wgsl"),
        include_str!("liquify_update.wgsl"), include_str!("liquify_map.wgsl"), &source_bindings(true, map_bindings(mode)),
        &dry_material::coverage_source(0),
        if analytic { include_str!("selection_absent.wgsl") } else { include_str!("selection_clip.wgsl") }]);
    let source = dry_material::specialize_contact(source, 0).into_owned();
    if analytic { source.replace("style.flags.x > .5", "true").replace("style.flags.x > 0.5", "true")
        .replace("style.flags.y <= .5", "true").replace("style.flags.y > 0.5", "false") }
    else { source }
}

pub(super) fn color_shader(device: &PipelineDevice) -> String {
    let packing = include_str!("liquify_map.wgsl").split("fn liquify_hash").next().unwrap();
    let mut sampling =
        "fn sample_tile(image:texture_2d_array<f32>,pixel:vec2<i32>,t:vec2<f32>,layer:i32,level:u32)->vec4<f32> {\n\
            return mix(mix(textureLoad(image,pixel,layer,i32(level)),textureLoad(image,pixel+vec2(1,0),layer,i32(level)),t.x),\n\
                mix(textureLoad(image,pixel+vec2(0,1),layer,i32(level)),textureLoad(image,pixel+vec2(1,1),layer,i32(level)),t.x),t.y);\n}\n".to_string();
    sampling += "fn sample_source(index:u32,pixel:vec2<i32>,t:vec2<f32>,layer:i32,level:u32)->vec4<f32> { switch index {\n";
    for i in 0..ORIGINAL_BINDINGS {
        sampling += &format!("case {i}u: {{return sample_tile(source_{i},pixel,t,layer,level);}}\n");
    }
    sampling += "default: {return vec4(0.);} } }\n";
    compose_wgsl(&[&working_color::shader(device), include_str!("brush_types.wgsl"), packing,
        &source_bindings(false, ORIGINAL_BINDINGS), &sampling, include_str!("liquify_reconstruct.wgsl")]).into_owned()
}

impl Pipelines {
    pub fn update(&self, style: &layer_render::DabStyle) -> &Deferred<wgpu::ComputePipeline> {
        let analytic = matches!(style.tip, BrushTip::AnalyticEllipse) && style.grain.is_none() && style.selection.is_none();
        &self.update[liquify_mode_code(style.deform.mode) as usize * 2 + usize::from(analytic)]
    }

    pub fn new(r: &WgpuRasterizer) -> Self {
        let stages = wgpu::ShaderStages::COMPUTE;
        let output_layout = |format, dabs| {
            let mut entries = vec![
                bindings::buffer(0, stages, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(mem::size_of::<StyleGpu>() as u64)),
                bindings::storage_texture(1, stages, format, wgpu::StorageTextureAccess::WriteOnly),
            ];
            if dabs { entries.push(bindings::buffer(2, stages, wgpu::BufferBindingType::Storage { read_only: true }, false, None)); }
            bindings::layout(&r.device, "liquify outputs", &entries)
        };
        let map_layout = output_layout(MAP_FORMAT, true);
        let color_layout = output_layout(r.device.working_format(), false);
        let inputs = |integer, count: u32| {
            let ty = if integer { wgpu::TextureSampleType::Uint } else { wgpu::TextureSampleType::Float { filterable: false } };
            let dimension = if integer { wgpu::TextureViewDimension::D2 } else { wgpu::TextureViewDimension::D2Array };
            let mut entries = (0..count).map(|i| bindings::texture_of(i, stages, ty, dimension)).collect::<Vec<_>>();
            entries.push(bindings::buffer(count, stages, wgpu::BufferBindingType::Uniform, true, NonZeroU64::new(160)));
            entries.push(bindings::texture_of(count + 1, stages, wgpu::TextureSampleType::Uint, wgpu::TextureViewDimension::D2));
            if !integer {
                entries.extend([bindings::texture(count + 2, stages, false), bindings::texture(count + 3, stages, false)]);
                entries.push(bindings::buffer(count + 4, stages, wgpu::BufferBindingType::Storage { read_only: true }, false, None));
                entries.push(bindings::texture(count + 5, stages, false));
                entries.push(bindings::texture_of(count + 6, stages, wgpu::TextureSampleType::Uint, wgpu::TextureViewDimension::D2));
            }
            bindings::layout(&r.device, "liquify sparse inputs", &entries)
        };
        let map_sources = [9, 12].map(|count| inputs(true, count));
        let color_sources = inputs(false, ORIGINAL_BINDINGS as u32);
        let make = |label, output, inputs, code: String, entry, textures| {
            let mut layouts = vec![Some(output), Some(&r.target_layout), Some(inputs)];
            if textures { layouts.push(Some(&r.advanced_texture_layout)); }
            let layout = r.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label), bind_group_layouts: &layouts, immediate_size: 0,
            });
            let source = Deferred::wgsl(&r.device, label, code);
            Deferred::compute(&r.device, label, &layout, &source, entry)
        };
        let update = std::array::from_fn(|i| make("liquify compose coordinates", &map_layout, &map_sources[usize::from(map_bindings(i as u32 / 2) == 12)],
            map_shader(i as u32 / 2, i % 2 == 1), "update_map", true));
        let reconstruct = make("liquify reconstruct original", &color_layout, &color_sources, color_shader(&r.device), "reconstruct", false);
        Self { map_layout, color_layout, map_sources, color_sources, update, reconstruct,
            mips: display_mips::Pipelines::new(&r.device) }
    }
}

impl Liquify {
    pub fn extend_plans(r: &mut WgpuRasterizer, packet: FramePacket<'_>, plans: &mut [Vec<BrushTile>]) {
        if let Some(liquify) = &mut r.liquify {
            for stroke in liquify.strokes.values_mut() {stroke.resolve();}
            liquify.unchanged_previews.clear();
            liquify.strokes.retain(|target, stroke| !stroke.preview_only || packet.dab_batches.iter()
                .any(|batch| batch.target == *target && batch.stroke_id == stroke.id));
            for (target, stroke) in &mut liquify.strokes {
                if !packet.dab_batches.iter().any(|batch| batch.kind == DabBatchKind::Preview
                    && batch.target == *target && batch.stroke_id == stroke.id) { stroke.preview_tiles.clear(); }
            }
        }
        Self::extend_required_plans(r,packet,plans);
    }
    pub fn extend_required_plans(r:&WgpuRasterizer,packet:FramePacket<'_>,plans:&mut [Vec<BrushTile>]) {
        let previews: Vec<_> = packet.dab_batches.iter().enumerate()
            .filter(|(_, batch)| batch.kind == DabBatchKind::Preview && has_brush_work(batch)).collect();
        let reduced = previews.first().filter(|(_, batch)| previews.len() == 1
            && batch.style.execution == BrushExecution::Liquify && batch.style.deform.mode != LiquifyMode::Crystals
            && r.scale_display.as_ref().is_some_and(|cache| cache.preview_level(r, packet, batch.target, false) > 0))
            .map(|(index, batch)| (*index, batch.target, batch.stroke_id));
        for index in 0..packet.dab_batches.len() {
            let batch = &packet.dab_batches[index];
            if batch.style.execution != BrushExecution::Liquify { continue; }
            let preview = reduced.is_some_and(|(i, _, _)| i == index);
            let resolve = batch.kind == DabBatchKind::Persistent
                && (batch.stroke_end || reduced.is_none_or(|(_, target, id)| target != batch.target || id != batch.stroke_id))
                && packet.dab_batches[index + 1..].iter().filter(|b| b.kind == DabBatchKind::Persistent)
                    .all(|b| b.target != batch.target || b.stroke_id != batch.stroke_id);
            if !preview && !resolve { continue; }
            let mut dirty = r.liquify.as_ref().and_then(|l| l.strokes.get(&batch.target))
                .filter(|s| s.id == batch.stroke_id).map_or_default(|s| s.dirty.clone());
            if preview {
                for (_, tiles) in packet.dab_batches[..index].iter().zip(&plans[..index])
                    .filter(|(b, _)| b.kind == DabBatchKind::Persistent && b.target == batch.target && b.stroke_id == batch.stroke_id)
                {
                    for tile in tiles {
                        let bounds = dirty.entry(tile.coordinate).or_default();
                        *bounds = bounds.union(tile.local);
                    }
                }
            }
            for (coordinate, local) in dirty {
                if let Some(tile) = plans[index].iter_mut().find(|t| t.coordinate == coordinate) {
                    tile.local = tile.local.union(local);
                } else {
                    plans[index].push(BrushTile { coordinate, local, dabs: 0..0, indices: Vec::new() });
                }
            }
        }
    }

    pub fn required_sources(r:&WgpuRasterizer,packet:FramePacket<'_>,plans:&[Vec<BrushTile>],required:&mut BTreeMap<SourceTarget,BTreeSet<[u32;2]>>)->OriginalDemand {
        let plane=layer_core::raster::RasterPlane::Color;
        for session in r.liquify.iter().flat_map(|liquify|&liquify.sessions).filter_map(Weak::upgrade) {session.lock().expect("liquify sources").release();}
        let mut sessions:Vec<(SourceTarget,Session)>=Vec::new();
        for (batch,tiles) in packet.dab_batches.iter().zip(plans).filter(|(batch,_)|batch.style.execution==BrushExecution::Liquify) {
            let preview=batch.kind==DabBatchKind::Preview;
            let starts=batch.stroke_start && !preview;
            let known=sessions.iter().rposition(|(target,_)|*target==batch.target).filter(|at|sessions[*at].1.stroke==batch.stroke_id && !starts);
            let at=known.unwrap_or_else(|| {
                let live=r.liquify.as_ref().and_then(|liquify|liquify.strokes.get(&batch.target)).filter(|stroke|stroke.id==batch.stroke_id && !starts);
                let context=batch.style.liquify.as_ref();
                let previous=context.and_then(|c|c.field.as_ref()).and_then(|field|field.as_any().downcast_ref::<Field>()).filter(|field|field.device==*r.device);
                let field=live.map(|stroke|&stroke.field).or(previous);
                sessions.push((batch.target,Session {stroke:batch.stroke_id,fields:field.map_or_default(|field|field.tiles.iter().map(|(at,tile)|(*at,tile.bounds)).collect()),
                    originals:field.map(|field|field.originals.clone()),source:live.map(|stroke|stroke.original.clone()).or_else(||context.map(|c|c.original.clone())),
                    needed:BTreeSet::new(),staged:BTreeSet::new()}));
                sessions.len()-1
            });
            let session=&mut sessions[at].1;
            let shared=session.originals.clone();
            let originals=shared.as_ref().map(|originals|originals.lock().expect("liquify sources"));
            let current=std::cell::LazyCell::new(||r.raster_versions(packet.scene,batch.target).ok());
            let extent=packet.scene.target_extent(batch.target);
            let dabs=&packet.dabs[batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize];
            let mut outputs=Vec::new();
            for tile in tiles {
                let bounds=if !preview && tile.dabs.is_empty() && batch.style.liquify.as_ref().is_none_or(|c|c.amount.is_none()) {
                    let Some(bounds)=session.fields.get(&tile.coordinate) else {continue;};
                    *bounds
                }else {
                    let (_,bounds)=field_input_bounds(batch,dabs,tile.coordinate,extent,session.fields.iter().map(|(at,bounds)|(*at,*bounds)));
                    outputs.push((tile.coordinate,bounds));
                    bounds
                };
                let coordinates=required.entry(batch.target).or_default();coordinates.insert(tile.coordinate);
                for at in page_coordinates(field_source_bounds(bounds,extent,preview)).chain([tile.coordinate]) {
                    if !session.needed.insert(at) {continue;}
                    let Some(originals)=&originals else {coordinates.insert(at);continue;};
                    if originals.readable(at) {continue;}
                    let authored=current.as_ref().and_then(|current|current.tile(plane,at).matches(&originals.versions.tile(plane,at)));
                    if authored!=Some(false) {coordinates.insert(at);}
                    if authored!=Some(true) && session.source.as_ref().is_none_or(|source|decodes_original(source,at)) {session.staged.insert(at);}
                }
            }
            if !preview {session.fields.extend(outputs);}
        }
        let mut demand=OriginalDemand::default();
        let mut shared:Vec<(Arc<Mutex<Originals>>,BTreeSet<[u32;2]>,[u32;2])>=Vec::new();
        for (target,session) in sessions {
            let extent=packet.scene.target_extent(target);
            let Some(originals)=session.originals else {
                let mut chunks=BTreeMap::<u32,usize>::new();
                for at in &session.needed {*chunks.entry(original_chunk(*at,extent)).or_default()+=1;}
                demand.atlases+=new_atlases(chunks.into_values());
                continue;
            };
            if let Some(source)=session.source.filter(|_|!session.staged.is_empty()) {demand.staged.push((target,source,originals.clone(),session.staged.into_iter().collect()));}
            match shared.iter_mut().find(|(known,..)|Arc::ptr_eq(known,&originals)) {
                Some((_,needed,_))=>needed.extend(session.needed),
                None=>shared.push((originals,session.needed,extent)),
            }
        }
        for (originals,needed,extent) in shared {demand.atlases+=originals.lock().expect("liquify sources").hold(&needed,extent);}
        demand
    }

    pub fn new(r: &WgpuRasterizer) -> Self {
        Self { pipelines: r.recipes.liquify.get_or_init(|| Arc::new(Pipelines::new(r))).clone(), strokes: BTreeMap::new(),
            unchanged_previews: Default::default(),
            empty: TileStorage::new(r,1),
            scratch_map: std::array::from_fn(|_| TileStorage::new(r,1)),
            partials: Partials::default(),
            sessions: Vec::new(), idle: Default::default() }
    }

    pub fn idle_original_bytes(&self)->u64 {self.idle.load(Ordering::SeqCst)}
    #[cfg(test)]
    pub fn walked_idle_original_bytes(&self)->u64 {
        self.sessions.iter().filter_map(Weak::upgrade).map(|session|session.lock().expect("liquify sources").idle_atlases().count() as u64*ORIGINAL_ATLAS_BYTES).sum()
    }

    fn offer_originals(&self,heap:&Arc<crate::residency::Heap>)->Vec<(Arc<Mutex<Originals>>,usize,Arc<crate::residency::Lease>)> {
        let notice=Arc::new(AtomicBool::new(false));
        self.sessions.iter().filter_map(Weak::upgrade).flat_map(|session| {
            let idle=session.lock().expect("liquify sources").idle_atlases().collect::<Vec<_>>();
            idle.into_iter().map(|atlas| {
                let lease=heap.lease(&notice);
                lease.offer(ORIGINAL_ATLAS_BYTES,crate::residency::rebuild_value(ORIGINAL_ATLAS_BYTES,ORIGINAL_ATLAS_BYTES));
                (session.clone(),atlas,lease)
            }).collect::<Vec<_>>()
        }).collect()
    }

    pub fn reset(&mut self) { self.strokes.clear(); self.unchanged_previews.clear(); }

    pub fn preview_unchanged(&self, target: SourceTarget, coordinate: [u32; 2]) -> bool {
        self.unchanged_previews.contains(&(target, coordinate))
    }

    pub(crate) fn captured_input(&self,target:SourceTarget,id:StrokeId,consumer:&submission::Submission)->Option<scene::Input> {
        let stroke=self.strokes.get(&target).filter(|stroke|stroke.id==id)?;
        let content=stroke.pending.as_ref().filter(|pending|pending.receipt.submitted() || (!pending.receipt.discarded() && pending.receipt.same(consumer)))
            .map_or(stroke.field.content,|pending|pending.content);
        Some(scene::Input {source:stroke.source.source.clone(),version:layer_core::raster::ContentVersion::derive(0x6c697175696679,
            &[stroke.source.version.clone(),content.into()])})
    }

    pub fn publish(&mut self,scene:SceneView<'_>,batches:&[DabBatch],receipt:&submission::Submission) {
        if !receipt.submitted() {return;}
        for stroke in self.strokes.values_mut() {stroke.resolve();}
        for batch in batches.iter().filter(|b| b.stroke_end && b.kind == DabBatchKind::Persistent) {
            let target = batch.target;
            let Some(stroke)=self.strokes.get(&target).filter(|s|s.id==batch.stroke_id) else {continue;};
            if stroke.field.tiles.values().any(|tile|tile.read(receipt).is_err()) {continue;}
            let stroke=self.strokes.remove(&target).unwrap();
            stroke.field.originals.lock().expect("liquify sources").release();
            if batch.style.execution != BrushExecution::Liquify { continue; }
            let SourceTarget::Paint(handle) = target else { continue };
            let Some(source) = scene.paint(handle).filter(|s| s.raster.try_data().is_none()) else { continue };
            if stroke.field.tiles.is_empty() { continue; }
            let field = Field { content:stroke.field.content, device: stroke.field.device.clone(), tiles: stroke.field.tiles.clone(), originals: stroke.field.originals.clone() };
            source.raster.set_liquify(Arc::new(layer_core::liquify::State::new(stroke.original.clone(), Arc::new(field), source,
                scene.source_owner(target).and_then(|h| scene.occurrence(h)).is_some_and(|o| o.alpha_locked))));
        }
    }

    fn original(r: &mut WgpuRasterizer, stroke: &Stroke, target: SourceTarget, coordinate: [u32; 2],
        encoder:&mut submission::CommandEncoder,pending:&mut Vec<display_mips::CompleteUpdates>,
        writes:&mut Vec<(Arc<OriginalTile>,resident::Encoding)>) -> Result<Arc<OriginalTile>,GpuRasterError> {
        #[cfg(test)] crate::test_support::record_liquify_tile(crate::test_support::LiquifyVisit::Original,coordinate);
        let key=layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Color,coordinate};
        let (capture,prior)={let mut originals=stroke.field.originals.lock().expect("liquify sources");
            let prior=originals.tiles.get(&coordinate).cloned();
            if let Some(tile)=&prior {originals.set_held((tile.index/ORIGINAL_LAYERS) as usize,true);}
            (originals.versions.tile(key.plane,coordinate),prior)};
        if let Some(tile)=&prior {
            if tile.capture.matches(&capture)!=Some(true) {return Err(GpuRasterError::NotReady);}
            if tile.read(&encoder.submission()).is_ok() || writes.iter().any(|(pending,_)|Arc::ptr_eq(pending,tile)) {return Ok(tile.clone());}
        }
        let receipt=encoder.submission();
        let source=if let Some(read)=r.read_layer_color(target,coordinate,&capture,Some(&receipt)) {
            Some(source_access::Sampled::new(source_access::RawTile {texture:read.value().texture.clone().into(),view:read.value().view.clone()},read.complete()))
        }else {
            let data=stroke.original.raster.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect)?;
            let declared=data.tiles.get(&key).map_or_else(||crate::raster::contents::fallback(
                &crate::raster::contents::defaults_for(stroke.original.domain,stroke.original.base.as_ref(),None)[0],coordinate),
                layer_core::raster::RasterTile::content_version);
            match declared.matches(&capture) {
                Some(true)=>{},Some(false)=>return Err(GpuRasterError::Invariant("Liquify backing does not match its captured input")),
                None=>return Err(GpuRasterError::NotReady),
            }
            if data.tiles.contains_key(&key) || stroke.original.base.as_ref().is_some_and(|base|source_access::paint_base_contains(base,coordinate)) {
                return Err(GpuRasterError::Invariant("Liquify original is not prepared"));
            }
            None
        };
        let extent=r.target_extent(target);
        let mut room=r.original_room();
        let stored=Self::store(r,&mut room,&mut stroke.field.originals.lock().expect("liquify sources"),coordinate,extent,capture,prior,source,encoder,pending,writes);
        r.keep_original_room(room);
        stored
    }

    pub(crate) fn restore_original(r:&WgpuRasterizer,originals:&Mutex<Originals>,room:&mut Option<crate::residency::Reservation>,coordinate:[u32;2],extent:[u32;2],
        version:&layer_core::raster::ContentVersion,source:source_access::Sampled<source_access::RawTile>,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let mut originals=originals.lock().expect("liquify sources");
        let capture=originals.versions.tile(layer_core::raster::RasterPlane::Color,coordinate);
        if version.matches(&capture)!=Some(true) {return Err(GpuRasterError::Invariant("Liquify backing does not match its captured input"));}
        if originals.readable(coordinate) {return Ok(());}
        let prior=originals.tiles.get(&coordinate).cloned();
        let (mut pending,mut writes)=(Vec::new(),Vec::new());
        Self::store(r,room,&mut originals,coordinate,extent,capture,prior,Some(source),encoder,&mut pending,&mut writes)?;
        display_mips::CompleteUpdates::flush_all(&mut pending,encoder);
        let actual=encoder.submission();
        for (_,encoding) in writes {encoding.publish(&actual);}
        Ok(())
    }

    fn store(r:&WgpuRasterizer,room:&mut Option<crate::residency::Reservation>,originals:&mut Originals,coordinate:[u32;2],extent:[u32;2],
        capture:layer_core::raster::ContentVersion,prior:Option<Arc<OriginalTile>>,source:Option<source_access::Sampled<source_access::RawTile>>,encoder:&mut submission::CommandEncoder,
        pending:&mut Vec<display_mips::CompleteUpdates>,writes:&mut Vec<(Arc<OriginalTile>,resident::Encoding)>)->Result<Arc<OriginalTile>,GpuRasterError> {
        let receipt=encoder.submission();
        let chunk=original_chunk(coordinate,extent);
        let layers=chunk*CHUNK_TILES..(chunk+1)*CHUNK_TILES;
        if prior.is_none() && originals.free.range(layers.clone()).next().is_none() {
            crate::prepared_raster_output(room.as_mut().and_then(|room|room.split(ORIGINAL_ATLAS_BYTES)).map(drop))?;
            let texture=r.device.create_texture(&wgpu::TextureDescriptor {
                label:Some("liquify original tiles"),size:wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:ORIGINAL_LAYERS},
                mip_level_count:9,sample_count:1,dimension:wgpu::TextureDimension::D2,format:r.device.working_format(),
                usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::STORAGE_BINDING|wgpu::TextureUsages::COPY_DST|wgpu::TextureUsages::COPY_SRC,
                view_formats:&[],
            });
            let view=texture.create_view(&wgpu::TextureViewDescriptor {dimension:Some(wgpu::TextureViewDimension::D2Array),..Default::default()});
            let first=chunk as usize*ORIGINAL_BINDINGS;
            let atlas=(first..first+ORIGINAL_BINDINGS).find(|at|originals.atlases.get(*at).is_none_or(Option::is_none))
                .ok_or(GpuRasterError::Invariant("Liquify original chunk is full"))?;
            if originals.atlases.len()<=atlas {originals.atlases.resize_with(atlas+1,||None);}
            originals.atlases[atlas]=Some(OriginalAtlas {id:NEXT_RESOURCE.fetch_add(1,Ordering::Relaxed),held:true,_charge:r.raster_tiers.paint.charge(texture_bytes(&texture)),texture,view});
            originals.free.extend((0..ORIGINAL_LAYERS).map(|layer|atlas as u32*ORIGINAL_LAYERS+layer));
        }
        let index=match &prior {Some(tile)=>tile.index,None=> {
            let index=originals.free.range(layers).next().copied().ok_or(GpuRasterError::Invariant("Liquify original has no atlas layer"))?;
            originals.free.remove(&index);index
        }};
        let layer=index%ORIGINAL_LAYERS;
        originals.set_held(index as usize/ORIGINAL_LAYERS as usize,true);
        let texture=&originals.atlases[index as usize/ORIGINAL_LAYERS as usize].as_ref().ok_or(GpuRasterError::Invariant("Liquify original has no atlas"))?.texture;
        let descriptor=wgpu::TextureViewDescriptor {dimension:Some(wgpu::TextureViewDimension::D2),base_array_layer:layer,array_layer_count:Some(1),..Default::default()};
        let mut surface=prior.as_ref().map_or_else(||resident::Resident::unwritten(source_access::RawTile {
            texture:source_access::TileTexture::layer_of(texture.clone(),layer),view:texture.create_view(&descriptor),
        }),|tile|tile.surface.clone());
        let mut complete=true;
        let source=source.map(|source|source.take(&mut complete));
        let write=surface.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?.completed(complete);
        if let Some(source)=source {
            let mut destination=write.value().texture.as_image_copy();destination.origin.z=layer;
            encoder.copy_texture_to_texture(source.texture.as_image_copy(),destination,source.texture.size());
        }
        let views=(0..9).map(|level|write.value().texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level:level,mip_level_count:Some(1),..descriptor.clone()
        })).collect::<Vec<_>>();
        let refs=views.iter().collect::<Vec<_>>();
        let pipelines=r.recipes.liquify.get().expect("liquify prepared");
        let mut reductions=display_mips::CompleteUpdates::from_level(&r.device,&pipelines.mips,
            display_mips::Plan::at([PAGE_SIZE;2],8),0,&refs);
        reductions.tile(encoder,[0;2]);pending.push(reductions);
        let encoding=write.encoded(capture.clone());
        let tile=Arc::new(OriginalTile {capture,index,surface});
        writes.push((tile.clone(),encoding));originals.tiles.insert(coordinate,tile.clone());
        Ok(tile)
    }

    fn carry_preview(r:&mut WgpuRasterizer,stroke:&Stroke,batch:&DabBatch,batch_index:usize,coordinate:[u32;2],receipt:&submission::Submission)
        ->Result<Option<layer_core::raster::ContentVersion>,GpuRasterError> {
        let Some((_,held))=stroke.preview_tiles.get(&coordinate).filter(|(inputs,_)|*inputs==stroke.preview_inputs(&coordinate)) else {return Ok(None);};
        if !r.preview_pages.iter().any(|page|page.coordinate==coordinate && page.active().read(held,Some(receipt)).is_some_and(|read|read.complete())) {return Ok(None);}
        r.accepted_raster_output(batch.target,RasterOutput::PreviewColor(coordinate))?;
        let ticket=r.preview_version(batch.target,layer_core::raster::RasterPlane::Color,coordinate,scene::PreviewStage::AfterBatch(batch_index))?;
        let page=prepared_raster_output(r.preview_pages.iter_mut().find(|page|page.coordinate==coordinate))?;
        let write=page.active_mut().revise(held,receipt).ok_or(GpuRasterError::Invariant("a carried Liquify preview changed"))?;
        let ticket=ticket.written(write.complete());
        let version=ticket.version().clone();
        let encoding=write.encoded(version.clone()).certified(resident::Delta::InPlace(PixelRect::EMPTY));
        r.publish_material(batch,vec![MaterialPublication {kind:MaterialOutputKind::Color,coordinate,input:ticket.input().clone(),preview:Some(ticket),encoding}.encoded(receipt)])?;
        Ok(Some(version))
    }

    fn keep_previews(&mut self,target:SourceTarget,tail:PixelRect,tiles:&[BrushTile],versions:BTreeMap<[u32;2],layer_core::raster::ContentVersion>) {
        let stroke=self.strokes.get_mut(&target).unwrap();
        let kept=|coordinate:&[u32;2]|tail.intersect(page_rect(*coordinate)).is_empty() && tiles.iter().any(|tile|tile.coordinate==*coordinate);
        stroke.preview_tiles.retain(|coordinate,_|kept(coordinate));
        for (coordinate,version) in versions.into_iter().filter(|(coordinate,_)|kept(coordinate)) {
            stroke.preview_tiles.insert(coordinate,(stroke.preview_inputs(&coordinate),version));
        }
    }

    fn dispatch(&self, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, batch_index: usize,
        batch: &DabBatch, passes: &[Vec<Dispatch>], map: bool) {
        let count = passes.iter().map(Vec::len).sum::<usize>();
        if count == 0 { return; }
        let phase = if map { 16 } else { 18 };
        let measured = r.telemetry.phase_begin(phase, &r.device, &r.queue, encoder);
        performance_trace::counter(if map { c"Capy Liquify map dispatches" } else { c"Capy Liquify reconstruct dispatches" }, count as u64);
        #[cfg(test)] crate::test_support::PassCounts {liquify_dispatches:count as u64,..Default::default()}.record();
        let size = PAGE_SIZE / r.dry_material_block(batch);
        for jobs in passes.iter().filter(|jobs| !jobs.is_empty()) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("liquify sparse tiles"), timestamp_writes: None });
            pass.set_pipeline(if map { self.pipelines.update(&batch.style) } else { &self.pipelines.reconstruct });
            if map {
                let key = WgpuRasterizer::texture_set_key(&batch.style);
                pass.set_bind_group(3, &r.texture_sets.iter().find(|t| t.key == key).unwrap().bind_group, &[]);
            }
            for job in jobs {
                pass.set_bind_group(0, &job.output, &[batch_index as u32 * r.style_stride as u32]);
                pass.set_bind_group(1, r.paint_target_binding(&batch.style), &[r.layer_target_offset(batch.target, job.coordinate)]);
                pass.set_bind_group(2, &job.sources, &[job.record]);
                pass.dispatch_workgroups(size.div_ceil(16), size.div_ceil(8), 1);
            }
        }
        if measured { r.telemetry.phase_end(phase, encoder); }
    }

    pub fn encode(&mut self, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder, batch_index: usize,
        batch: &DabBatch, context: BrushEncodingContext<'_>) -> Result<(), GpuRasterError> {
        let preview = context.target.is_preview();
        let deferred = !batch.stroke_end && r.preview_level > 0 && context.batches.iter().any(|next|
            next.kind == DabBatchKind::Preview && has_brush_work(next) && next.target == batch.target
                && next.stroke_id == batch.stroke_id && next.style.execution == BrushExecution::Liquify
                && next.style.deform.mode != LiquifyMode::Crystals);
        let block = r.dry_material_block(batch);
        if preview { performance_trace::counter(c"Capy Liquify preview block", block as u64); }
        if self.strokes.get(&batch.target).is_none_or(|s| s.id != batch.stroke_id) || batch.stroke_start && !preview {
            let previous = batch.style.liquify.as_ref().and_then(|c| c.field.as_ref())
                .and_then(|f| f.as_any().downcast_ref::<Field>()).filter(|f| f.device == *r.device);
            let original = batch.style.liquify.as_ref().map(|c| if previous.is_some() { c.original.clone() } else { c.current.clone() })
                .or_else(|| r.artwork_frame.as_ref().and_then(|f| match batch.target {
                    SourceTarget::Paint(handle) => f.scene.view().paint(handle).cloned().map(Arc::new), _ => None,
                })).unwrap_or_else(|| Arc::new(layer_core::authored::PaintSource {
                    color_mode: Default::default(), domain: context.target_extent, raster: Default::default(), base: None, operations: Arc::default(),
                }));
            let mut original_scene=context.source.snapshot();
            let SourceTarget::Paint(handle)=batch.target else {return Err(GpuRasterError::MissingPaintLayer(batch.target));};
            *original_scene.artwork.paint.get_mut(handle).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?=(*original).clone();
            let owner=original_scene.view().source_owner(batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
            original_scene.scope=SceneScope::Members(vec![owner].into());
            let originals=match previous {Some(field)=>field.originals.clone(),None=> {
                let originals=Arc::new(Mutex::new(Originals::new(r.raster_versions(original_scene.view(),batch.target)?,self.idle.clone())));
                self.sessions.retain(|session|session.strong_count()!=0);
                self.sessions.push(Arc::downgrade(&originals));
                originals
            }};
            let versions=originals.lock().expect("liquify sources").versions.clone();
            let extent=original_scene.view().composition().size;
            let source=scene::Input::from_declared(FramePacket {scene:original_scene.view(),view:layer_render::ViewState {width_px:extent[0],height_px:extent[1],document_to_surface:[1.,0.,0.,1.,0.,0.]},
                document_extent:extent,time_seconds:original_scene.context.elapsed,blend_space:original_scene.view().composition().blend,
                dabs:&[],dab_batches:&[],restore_rasters:&[],reset_layers:false,composite_all:false,commit_rasters:true,inspect_mask:None,selection_overlays:None},
                crate::IdMap::from_iter([(batch.target,versions)]))?;
            self.strokes.insert(batch.target, Stroke { pending:None,source,id: batch.stroke_id, preview_only: preview, original, dirty: Default::default(),
                colored_tiles: previous.map_or_default(|f| f.tiles.clone()),
                preview_tiles: Default::default(),
                field: Field { content:previous.map_or_else(layer_core::raster::ContentName::fresh,|field|field.content), device: (*r.device).clone(), tiles: previous.map_or_default(|f| f.tiles.clone()),
                    originals } });
        }
        self.strokes.get_mut(&batch.target).unwrap().stage(&encoder.submission())?;
        let stroke = self.strokes.get(&batch.target).unwrap();
        let extent = context.target_extent;
        let count = map_bindings(liquify_mode_code(batch.style.deform.mode) as u32);
        let tail = batch_pixel_rect(batch, extent);
        let dabs = &context.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize];
        let mut outputs = Vec::new();
        let mut dispatches = Vec::new();
        let mut map_publications=Vec::new();
        let mut reductions = Vec::new();
        let mut original_writes=Vec::new();
        let mut preview_versions=BTreeMap::new();
        let mut taken=BTreeSet::new();
        for tile in context.tiles {
            if !preview && tile.dabs.is_empty() && batch.style.liquify.as_ref().is_none_or(|c| c.amount.is_none()) { continue; }
            if context.target == (BrushEncodingTarget::Preview { from_persistent: true })
                && tail.intersect(page_rect(tile.coordinate)).is_empty()
                && let Some(version)=Self::carry_preview(r,stroke,batch,batch_index,tile.coordinate,&encoder.submission())?
            {
                self.unchanged_previews.insert((batch.target, tile.coordinate));
                preview_versions.insert(tile.coordinate,version);
                continue;
            }
            if preview { self.unchanged_previews.remove(&(batch.target, tile.coordinate)); }
            if taken.insert(tile.coordinate) {Self::original(r, stroke, batch.target, tile.coordinate, encoder, &mut reductions,&mut original_writes)?;}
            let (input_bounds,bounds)=field_input_bounds(batch,dabs,tile.coordinate,extent,stroke.tiles().map(|(at,tile)|(*at,tile.bounds)));
            let input_tiles:Vec<_>=stroke.tiles().filter(|(at,_)|!page_rect(**at).intersect(input_bounds).is_empty())
                .map(|(at,tile)|(*at,tile.clone())).collect();
            let output = Arc::new(Tile::at_block(r, bounds, block));
            let mut output_storage=output.pixels.clone();
            let output_receipt=encoder.submission();
            let output_write=output_storage.initialize(&output_receipt).ok_or(GpuRasterError::Invariant("prepared output was already initialized"))?;
            let output_view=output_write.value().view.clone();
            map_publications.push(output_write.encoded(output.capture.clone()));
            let chunks = input_tiles.len().div_ceil(count).max(1);
            let first_record = r.prepare_records((0..chunks).map(|chunk| {
                let pages = &input_tiles[(chunk * count).min(input_tiles.len())..((chunk + 1) * count).min(input_tiles.len())];
                let own = pages.iter().position(|(at, _)| *at == tile.coordinate).map_or(0, |index| index as u32 + 1);
                let mut record = material_sources::record([pages.len() as u32, u32::from(chunk > 0), u32::from(chunk + 1 == chunks), own],
                    pages.chunks(if count == 9 { 1 } else { 2 }).map(|pair| {
                        let [x, y] = pair[0].0;
                        let [u, v] = pair.get(1).map_or([0; 2], |p| p.0);
                        [x, y, u, v]
                    }));
                let bounds_index = if count == 9 { 6 } else { 4 + count * 2 };
                record[bounds_index] = tile.local.min_x() | tile.local.min_y() << 16;
                record[bounds_index + 1] = tile.local.max_x() | tile.local.max_y() << 16;
                record
            }), encoder)?;
            for chunk in 0..chunks {
                let pages = &input_tiles[(chunk * count).min(input_tiles.len())..((chunk + 1) * count).min(input_tiles.len())];
                let source_views=pages.iter().map(|(_,tile)|tile.read(&encoder.submission()).map(|read|read.value().view.clone())).collect::<Result<Vec<_>,_>>()?;
                let destination = if chunk + 1 == chunks { &output_view } else { &self.scratch_map[chunk % 2].view };
                let previous = &self.scratch_map[(chunk + 1) % 2].view;
                let output_binding = bindings::group(&r.device, "liquify map output", &self.pipelines.map_layout, [
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer: &r.style_buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64)}),
                    wgpu::BindingResource::TextureView(destination), r.dab_buffer.as_entire_binding(),
                ]);
                let inputs = bindings::group(&r.device, "liquify map inputs", &self.pipelines.map_sources[usize::from(count == 12)],
                    (0..count).map(|i| wgpu::BindingResource::TextureView(source_views.get(i).unwrap_or(&self.empty.view)))
                        .chain([wgpu::BindingResource::Buffer(r.material_records.binding()), wgpu::BindingResource::TextureView(previous)]));
                dispatches.push(Dispatch { output: output_binding, sources: inputs, coordinate: tile.coordinate,
                    record: r.material_records.offset(first_record + chunk) });
            }
            outputs.push((tile.coordinate, output));
        }
        #[cfg(test)] if preview {for (coordinate,_) in &outputs {crate::test_support::record_liquify_tile(crate::test_support::LiquifyVisit::Preview,*coordinate);}}
        self.dispatch(r, encoder, batch_index, batch, &[dispatches], true);
        let actual=encoder.submission();
        for publication in map_publications {publication.publish(&actual);}
        let mut color_bounds = BTreeMap::new();
        if !preview {
            let stroke = self.strokes.get_mut(&batch.target).unwrap();
            stroke.preview_only = false;
            stroke.stage(&encoder.submission())?;
            for tile in context.tiles {
                let before=stroke.dirty_at(&tile.coordinate).unwrap_or(PixelRect::EMPTY);
                stroke.pending.as_mut().unwrap().dirty.insert(tile.coordinate,Some(before.union(tile.local)));
            }
            if !outputs.is_empty() {stroke.pending.as_mut().unwrap().content=layer_core::raster::ContentName::fresh();}
            stroke.pending.as_mut().unwrap().tiles.extend(outputs.drain(..).map(|(at,tile)|(at,Some(tile))));
            let next = context.batches[batch_index + 1..].iter().find(|next| next.kind != DabBatchKind::Preview);
            if !deferred && (batch.stroke_end || next.is_none_or(|next| next.target != batch.target || next.stroke_id != batch.stroke_id
                || next.style != batch.style || next.stroke_start))
            {
                color_bounds=stroke.dirty().collect();
                for coordinate in color_bounds.keys() {
                    outputs.push((*coordinate,stroke.tile(coordinate).ok_or(GpuRasterError::Invariant("prepared encode state is inconsistent"))?.clone()));
                    stroke.pending.as_mut().unwrap().dirty.insert(*coordinate,None);
                }
            }
        }
        let stroke = self.strokes.get(&batch.target).unwrap();
        let (mut regions,mut known)=(Vec::new(),BTreeMap::new());
        let sources=outputs.iter().map(|(_,field)| {
            let region=field.source_bounds(extent,preview);
            *known.entry([region.min_x(),region.min_y(),region.max_x(),region.max_y()]).or_insert_with(||{regions.push(region);regions.len()-1})
        }).collect::<Vec<_>>();
        for coordinate in regions.iter().flat_map(|region|page_coordinates(*region)) {
            if taken.insert(coordinate) {Self::original(r, stroke, batch.target, coordinate, encoder, &mut reductions,&mut original_writes)?;}
        }
        let measured = !reductions.is_empty() && r.telemetry.phase_begin(17, &r.device, &r.queue, encoder);
        display_mips::CompleteUpdates::flush_all(&mut reductions, encoder);
        let actual=encoder.submission();
        for (_,encoding) in original_writes {encoding.publish(&actual);}
        if measured { r.telemetry.phase_end(17, encoder); }
        if outputs.is_empty() {
            if preview {self.keep_previews(batch.target,tail,context.tiles,preview_versions);}
            return Ok(());
        }
        let mut publications=Vec::new();
        let receipt=encoder.submission();
        let mut current_tiles = BTreeMap::new();
        if preview {
            for (coordinate, _) in &outputs {
                if let Some(view) = r.raw_page(batch.target, *coordinate, None, encoder)? { current_tiles.insert(*coordinate, view); }
            }
        }
        let originals = stroke.field.originals.lock().expect("liquify sources");
        let grid = extent.map(|n| n.div_ceil(PAGE_SIZE));
        let mut indices = vec![0u32; (4 + (grid[0] * grid[1]) as usize).max(8)];
        indices[..2].copy_from_slice(&grid);
        for (at, tile) in &originals.tiles { indices[4 + (at[1] * grid[0] + at[0]) as usize] = tile.index + 1; }
        let index_buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquify original tile indices"), size: (indices.len() * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
        });
        let bytes: Vec<_> = indices.iter().flat_map(|v| v.to_ne_bytes()).collect();
        r.uploads.write(encoder, &index_buffer, &bytes)?;
        let resident=originals.atlases.iter().flatten().next().ok_or(GpuRasterError::Invariant("Liquify original has no atlas"))?;
        let region_chunks=regions.iter().map(|region| {
            let (mut chunks,mut complete)=(BTreeSet::new(),true);
            for at in page_coordinates(*region) {
                if let Some(tile)=originals.tiles.get(&at) {
                    let read=tile.read(&receipt)?;complete&=read.complete();
                    if originals.atlases[tile.index as usize/ORIGINAL_LAYERS as usize].as_ref().is_none_or(|atlas|read.value().texture.raw()!=&atlas.texture) {return Err(GpuRasterError::InvalidExtent);}
                    chunks.insert(tile.index as usize/(ORIGINAL_BINDINGS*ORIGINAL_LAYERS as usize));
                }
            }
            Ok((chunks.into_iter().collect::<Vec<_>>(),complete))
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        let partials_of=|source:usize|region_chunks[source].0.len().saturating_sub(1).min(2);
        let slots=self.partials.reserve(r,encoder,block,sources.iter().map(|source|partials_of(*source)).sum());
        let (mut passes,mut round,mut used)=(Vec::<Vec<Dispatch>>::new(),0,0);
        #[cfg(test)] let call=crate::test_support::next_liquify_reconstruct();
        for ((coordinate, field), source) in outputs.iter().zip(sources) {
            let (chunks,mut complete)=(&region_chunks[source].0,region_chunks[source].1);
            if used+partials_of(source)>slots {(round,used)=(passes.len(),0);}
            if passes.len()<round+chunks.len() {passes.resize_with(round+chunks.len(),Vec::new);}
            let current=current_tiles.remove(coordinate).map(|view|view.take(&mut complete));
            let prior_view=stroke.colored(coordinate).and_then(|tile|tile.read(&receipt).ok()).map(|read|read.value().view.clone());
            let field_view=field.read(&receipt)?.value().view.clone();
            let local = color_bounds.get(coordinate).copied().unwrap_or_else(|| PixelRect::full([PAGE_SIZE; 2]));
            let first_record = r.prepare_records(chunks.iter().enumerate().map(|(index, &chunk)| {
                let count = (originals.atlases.len() - chunk * ORIGINAL_BINDINGS).min(ORIGINAL_BINDINGS);
                material_sources::record([count as u32, u32::from(index > 0), u32::from(index + 1 == chunks.len()), if preview { 4 } else { 16 }],
                    [[(chunk * ORIGINAL_BINDINGS) as u32, ORIGINAL_LAYERS, u32::from(preview), u32::from(prior_view.is_some())],
                        [local.min_x(), local.min_y(), local.max_x(), local.max_y()],
                        [field.bounds.min.x.to_bits(), field.bounds.min.y.to_bits(), field.bounds.max.x.to_bits(), field.bounds.max.y.to_bits()]])
            }), encoder)?;
            let original_view=source_access::Sampled::read(&originals.tiles.get(coordinate).ok_or(GpuRasterError::Invariant("Liquify original is not prepared"))?.read(&receipt)?).take(&mut complete);
            r.accepted_raster_output(batch.target,if preview {RasterOutput::PreviewColor(*coordinate)}else {RasterOutput::Color(*coordinate)})?;
            let preview_version=if preview {Some(r.preview_version(batch.target,layer_core::raster::RasterPlane::Color,*coordinate,scene::PreviewStage::AfterBatch(batch_index))?)}else {None};
            let intent=match &preview_version {Some(version)=>version.input().clone(),None=>r.raster_intent(batch.target,layer_core::raster::RasterPlane::Color,*coordinate)?};
            let layer=r.paint_layers.iter().position(|l|l.id==batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
            let copied_outside=(preview && block==1 && batch.style.liquify.as_ref().is_none_or(|context|context.amount.is_none())
                && stroke.colored(coordinate).is_some()==prior_view.is_some()).then(|| {
                let committed=r.raster_intent(batch.target,layer_core::raster::RasterPlane::Color,*coordinate).ok()?;
                let read=r.paint_layers[layer].pages.iter().find(|page|page.coordinate==*coordinate)?.active().read(committed.version(),Some(&receipt))?;
                let predicted=context.tiles.iter().find(|tile|tile.coordinate==*coordinate)?.local;
                (*current.as_ref()?==read.value().view)
                    .then(||resident::Delta::seeded(&read.input(),predicted.union(stroke.dirty_at(coordinate).unwrap_or(PixelRect::EMPTY))))
            }).flatten();
            let pages=if preview {&mut r.preview_pages}else {&mut r.paint_layers[layer].pages};
            let page=prepared_raster_output(pages.iter_mut().find(|p|p.coordinate==*coordinate))?;
            let write=if preview {page.active_mut().overwrite(&receipt)}else {page.active_mut().write(&intent,&receipt)}
                .ok_or(GpuRasterError::Invariant("prepared encode state is inconsistent"))?.completed(complete);
            let preview_version=preview_version.map(|ticket|ticket.written(write.complete()));
            let destination=write.value().view.clone();
            let version=preview_version.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|preview|preview.version().clone());
            if preview {preview_versions.insert(*coordinate,version.clone());}
            publications.push(MaterialPublication {kind:MaterialOutputKind::Color,coordinate:*coordinate,preview:preview_version,input:intent,
                encoding:write.encoded(version).certified(if preview {copied_outside.unwrap_or_default()}else {resident::Delta::InPlace(local)})});
            for (index, &chunk) in chunks.iter().enumerate() {
                let current = if preview {
                    current.as_ref().unwrap_or(&r.empty_view)
                } else { &original_view };
                let output_view = if index + 1 == chunks.len() { &destination } else { &self.partials.surfaces[used + index % 2].1 };
                let previous = if index == 0 { &r.empty_view } else { &self.partials.surfaces[used + (index + 1) % 2].1 };
                #[cfg(test)] crate::test_support::record_liquify_write([call,round + index],output_view,previous);
                let output_binding = bindings::group(&r.device, "liquify color output", &self.pipelines.color_layout, [
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {buffer: &r.style_buffer, offset: 0, size: NonZeroU64::new(mem::size_of::<StyleGpu>() as u64)}),
                    wgpu::BindingResource::TextureView(output_view),
                ]);
                let inputs = bindings::group(&r.device, "liquify original inputs", &self.pipelines.color_sources,
                    (0..ORIGINAL_BINDINGS).map(|i| wgpu::BindingResource::TextureView(&originals.atlases.get(chunk * ORIGINAL_BINDINGS + i).and_then(Option::as_ref).unwrap_or(resident).view))
                        .chain([wgpu::BindingResource::Buffer(r.material_records.binding()), wgpu::BindingResource::TextureView(&field_view),
                            wgpu::BindingResource::TextureView(previous),
                            wgpu::BindingResource::TextureView(&original_view), index_buffer.as_entire_binding(),
                            wgpu::BindingResource::TextureView(current), wgpu::BindingResource::TextureView(prior_view.as_ref().unwrap_or(&self.empty.view))]));
                passes[round + index].push(Dispatch { output: output_binding, sources: inputs, coordinate: *coordinate,
                    record: r.material_records.offset(first_record + index) });
            }
            used += partials_of(source);
        }
        self.dispatch(r, encoder, batch_index, batch, &passes, false);
        let actual=encoder.submission();
        r.publish_material(batch,publications.into_iter().map(|publication|publication.encoded(&actual)).collect())?;
        drop(originals);
        if preview {self.keep_previews(batch.target,tail,context.tiles,preview_versions);}
        if !preview {
            let stroke = self.strokes.get_mut(&batch.target).unwrap();
            stroke.stage(&encoder.submission())?;
            stroke.pending.as_mut().unwrap().colored.extend(outputs.into_iter().map(|(at,tile)|(at,Some(tile))));
            if batch.style.liquify.as_ref().is_some_and(|c|c.amount==Some(0.)) {
                let coordinates=stroke.tiles().map(|(at,_)|*at).collect::<Vec<_>>();
                if !coordinates.is_empty() {stroke.pending.as_mut().unwrap().content=layer_core::raster::ContentName::fresh();}
                for coordinate in coordinates {stroke.pending.as_mut().unwrap().tiles.insert(coordinate,None);}
                let coordinates=stroke.colored_tiles.keys().copied().chain(stroke.pending.as_ref().unwrap().colored.keys().copied()).collect::<Vec<_>>();
                for coordinate in coordinates {stroke.pending.as_mut().unwrap().colored.insert(coordinate,None);}
            }
        }
        Ok(())
    }
}

impl WgpuRasterizer {
    pub(crate) fn idle_original_bytes(&self)->u64 {self.liquify.as_ref().map_or(0,|liquify|liquify.idle_original_bytes())}

    pub(crate) fn evict_originals(&self,evict:impl FnOnce(&Self)) {
        let offers=self.liquify.as_ref().map_or_default(|liquify|liquify.offer_originals(self.raster_tiers.raster.heap()));
        evict(self);
        let (mut textures,mut leases)=(Vec::new(),Vec::new());
        for (session,atlas,lease) in offers {
            if !lease.evicted() {continue;}
            textures.extend(session.lock().expect("liquify sources").evict(atlas));
            leases.push(lease);
        }
        if textures.is_empty() {return;}
        let charge=self.raster_buffers.transfer.charge(textures.len() as u64*ORIGINAL_ATLAS_BYTES);
        #[cfg(not(target_arch="wasm32"))]
        let retired=self.device.retirement((textures,charge,leases));
        #[cfg(target_arch="wasm32")]
        let retired={drop(textures);(charge,leases)};
        self.device.on_queue_done(&self.queue,move|| {#[cfg(not(target_arch="wasm32"))] retired.complete();#[cfg(target_arch="wasm32")] drop(retired);});
    }
}

#[cfg(test)]
#[path = "liquify_field_tests.rs"]
mod field_tests;

#[cfg(test)]
#[path = "liquify_memory_tests.rs"]
mod memory_tests;

#[cfg(test)]
mod publication_tests {
    use super::*;
    use crate::layer_tests::{placement,preset_style};

    #[test]
    fn a_field_is_published_only_after_its_actual_submission() {
        let extent=[PAGE_SIZE;2];
        let r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut doc=placement::paint_document(extent,"Liquify publication");
        let original=Arc::new(placement::paint(&doc).clone());
        let input=scene::Input::capture(&r,crate::test_support::packet(doc.scene(),extent)).unwrap();
        let target=placement::target(&doc);
        let versions=r.raster_versions(doc.scene(),target).unwrap();
        placement::paint_mut(&mut doc).raster=layer_core::raster::RasterRevision::pending();
        let tile=Arc::new(Tile::new(&r,PixelRect::full(extent)));
        let mut pixels=tile.pixels.clone();
        let mut producer=submission::CommandEncoder::new(&r.device,&Default::default());
        let receipt=producer.submission();
        let write=pixels.initialize(&receipt).unwrap();
        let bytes=vec![0;PAGE_SIZE as usize*PAGE_SIZE as usize*16];
        r.queue.write_texture(write.value().texture.as_image_copy(),&bytes,
            wgpu::TexelCopyBufferLayout {offset:0,bytes_per_row:Some(PAGE_SIZE*16),rows_per_image:Some(PAGE_SIZE)},write.value().texture.size());
        let encoding=write.encoded(tile.capture.clone());
        producer.submit(&r.queue);encoding.publish(&receipt);
        let field=||Field {content:layer_core::raster::ContentName::fresh(),device:(*r.device).clone(),
            tiles:BTreeMap::from([([0,0],tile.clone())]),originals:Arc::new(Mutex::new(Originals::new(versions.clone(),Default::default())))};
        let source=placement::paint(&doc);
        let previous=Arc::new(layer_core::liquify::State::new(original.clone(),Arc::new(field()),source,false));
        let mut batch=crate::test_support::dab_batch(target,preset_style(layer_core::DefaultBrushPreset::LiquifyPush),layer_core::Rect::from_extent(extent));
        batch.stroke_id=StrokeId(71);batch.stroke_end=true;
        let mut liquify=Liquify::new(&r);
        for status in 0..3 {
            source.raster.set_liquify(previous.clone());
            let next=field();let expected:layer_core::raster::ContentVersion=next.content.into();
            liquify.strokes.insert(target,Stroke {pending:None,source:input.clone(),id:batch.stroke_id,preview_only:false,original:original.clone(),field:next,
                colored_tiles:BTreeMap::new(),preview_tiles:BTreeMap::new(),dirty:BTreeMap::new()});
            let mut encoder=Some(submission::CommandEncoder::new(&r.device,&Default::default()));
            let receipt=encoder.as_mut().unwrap().submission();
            if status==1 {drop(encoder.take());}
            if status==2 {encoder.take().unwrap().submit(&r.queue);}
            liquify.publish(doc.scene(),std::slice::from_ref(&batch),&receipt);
            let current=source.raster.liquify().unwrap();
            if status<2 {
                assert!(Arc::ptr_eq(&current,&previous),"unsubmitted/discarded producer replaced the captured CPU field: {status}");
                assert!(liquify.strokes.contains_key(&target),"refused publication lost its stroke: {status}");
            }else {
                assert!(!Arc::ptr_eq(&current,&previous));
                assert!(current.field.content_version().matches(&expected)==Some(true));
                assert!(!liquify.strokes.contains_key(&target));
            }
        }
    }

    #[test]
    fn a_discarded_mid_contact_map_preserves_the_submitted_field_for_retry() {
        let render=|discard:bool| {
            let extent=[256;2];let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let mut doc=placement::paint_document(extent,"Liquify discarded field");
            placement::set_source(&mut doc,layer_core::color::source::rgba8_source(extent,|x,y|[(x%251) as u8,(y%239) as u8,173,255]));
            let target=placement::target(&doc);
            let view=layer_render::ViewState {width_px:64,height_px:64,document_to_surface:[0.25,0.,0.,0.25,0.,0.]};
            crate::test_support::submit_settled(&mut r,FramePacket {view,..crate::test_support::packet(doc.scene(),extent)});
            let mut style=preset_style(layer_core::DefaultBrushPreset::LiquifyPush);
            style.liquify=Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc),false)));
            let packet=crate::test_support::packet(doc.scene(),extent);
            let contacts=|x:f32| {
                let mut real=crate::layer_tests::dab([0.;4]);real.center=layer_core::Point {x,y:128.};real.motion=[9.,2.];real.material[3]=1.;
                let mut predicted=real;predicted.center.x+=8.;
                [real,predicted]
            };
            let batches=|dabs:&[Dab;2],start:bool| {
                let mut persistent=crate::test_support::dab_batch(target,style.clone(),dabs[0].bounds());
                persistent.stroke_id=StrokeId(89);persistent.stroke_start=start;persistent.stroke_end=false;
                let mut preview=crate::test_support::dab_batch(target,style.clone(),dabs[1].bounds());
                preview.stroke_id=StrokeId(89);preview.kind=DabBatchKind::Preview;preview.stroke_start=false;preview.stroke_end=false;preview.first_dab=1;
                [persistent,preview]
            };
            let first=contacts(96.);let first_batches=batches(&first,true);
            crate::test_support::submit_ready(&mut r,FramePacket {view,composite_all:false,dabs:&first,dab_batches:&first_batches,..packet}).unwrap();
            assert!(r.preview_level>0,"the real Fit frame must defer persistent color");
            let bytes=|r:&WgpuRasterizer| {
                r.liquify.as_ref().unwrap().strokes[&target].field.tiles.iter().map(|(at,tile)| {
                    let read=tile.pixels.read(&tile.capture,None).expect("the committed field must remain a submitted source");
                    (*at,crate::layer_tests::page_bytes(r,&read.value().texture))
                }).collect::<BTreeMap<_,_>>()
            };
            let before=bytes(&r);assert!(!before.is_empty());
            let second=contacts(121.);let second_batches=batches(&second,false);
            let second_packet=FramePacket {view,composite_all:false,dabs:&second,dab_batches:&second_batches,..packet};
            for submit in if discard {vec![false,true]}else {vec![true]} {
                let mut plans=brush_tiles::plan_packet(second_packet,|target|second_packet.scene.target_extent(target)).unwrap();
                Liquify::extend_plans(&mut r,second_packet,&mut plans);
                if discard && submit {assert_eq!(bytes(&r),before,"discard must retain the accepted mid-contact field");}
                crate::test_support::prepare_raster(&mut r,|r|if r.raster_dependencies_ready(second_packet,None) {Ok(())}else {Err(GpuRasterError::NotReady)}).unwrap();
                let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
                r.prepare_uploads(second_packet,&mut plans,&mut encoder).unwrap();
                let mut liquify=r.liquify.take().unwrap();
                liquify.encode(&mut r,&mut encoder,0,&second_batches[0],BrushEncodingContext {
                    source:second_packet.scene,batches:&second_batches,dabs:&second,tiles:&plans[0],target_extent:extent,target:BrushEncodingTarget::Persistent,
                }).unwrap();
                r.uploads.finish(&encoder);
                if submit {let receipt=encoder.submission();encoder.submit(&r.queue);liquify.publish(second_packet.scene,&second_batches,&receipt);}
                else {drop(encoder);}
                r.liquify=Some(liquify);
            }
            let actual=bytes(&r);assert_ne!(actual,before,"the accepted second map must actually change displacement");
            let dirty=r.liquify.as_ref().unwrap().strokes[&target].dirty.clone();
            (actual,dirty)
        };
        assert_eq!(render(true),render(false),"discard/retry must match the fresh two-submission field and deferred-color debt");
    }

}
