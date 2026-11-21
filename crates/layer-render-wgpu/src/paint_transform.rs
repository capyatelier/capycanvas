//! Ordered layer transforms. Pigment and both persistent wetness channels use
//! immutable GPU captures; stroke-local coverage/reservoirs are not artwork.
//! Linked paint and mask targets run the same transaction with separate origins.
use super::*;
use pixel_transform::PixelTransform;
pub(super) mod mesh;
pub(crate) use mesh::MeshBuffers;
use crate::scene::resample;
pub(super) mod snapshot;
pub(super) mod fine;
mod fine_demand;
mod region_batches;
pub(crate) mod sampling;
pub(crate) use sampling::input_level;
use snapshot::TileSnapshot;
use layer_core::raster::{TileKey,RasterPlane};

#[derive(Clone,Copy)]
enum TransformWrite {Authored,Preview,Restore(u32)}

pub(crate) enum NativeRead<'a> {
    Resident(resident::Read<'a,PageSurface>),
    Original(layer_core::raster::ContentVersion),
    Display,
}

struct TransformOutput {
    texture:wgpu::Texture,
    input:raster::contents::Intent,
    encoding:Option<resident::Encoding>,
    written:Option<resident::Written>,
}

enum CommitPage {
    Color {target:SourceTarget,version:layer_core::raster::ContentVersion,page:LayerPage},
    Wetness {target:SourceTarget,version:layer_core::raster::ContentVersion,page:WatercolorWetnessPage},
}
impl CommitPage {
    fn key(&self)->(SourceTarget,RasterPlane,[u32;2]) {
        match self {
            Self::Color {target,page,..}=>(*target,RasterPlane::Color,page.coordinate),
            Self::Wetness {target,page,..}=>(*target,RasterPlane::WatercolorWetness,page.coordinate),
        }
    }
    fn read(&self,receipt:Option<&submission::Submission>)->Option<resident::Read<'_,PageSurface>> {
        match self {Self::Color {page,version,..}=>page.active().read(version,receipt),Self::Wetness {page,version,..}=>page.active().read(version,receipt)}
    }
    fn preparation_bytes(&self,known:&mut RetainedTextures)->u64 {
        let account=|page:&resident::Resident<PageSurface>,known:&mut RetainedTextures|if page.account_texture(known) {page.storage_bytes()}else {0};
        match self {
            Self::Color {page,..}=>account(&page.primary,known)+page.secondary.as_ref().map_or(0,|page|account(page,known)),
            Self::Wetness {page,..}=>account(&page.primary,known)+account(&page.secondary,known),
        }
    }
}

pub(crate) struct PreparedTransformCommit {
    owner:u64,
    revision:u64,
    extent:[u32;2],
    colour:layer_core::color::DocumentColor,
    requests:Vec<(SourceTarget,u32,layer_core::RasterOperation)>,
    sources:Vec<(SourceTarget,crate::scene::Capture)>,
    pages:Vec<CommitPage>,
}
impl PreparedTransformCommit {
    pub(crate) fn read(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],receipt:Option<&submission::Submission>)->Option<resident::Read<'_,PageSurface>> {
        self.pages.iter().find(|page|page.key()==(target,plane,coordinate))?.read(receipt)
    }
    pub(crate) fn contains(&self,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2])->bool {self.read(target,plane,coordinate,None).is_some()}
    pub(crate) fn matches(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->bool {
        self.owner==packet.scene.owner() && self.revision==packet.scene.revision() && self.extent==packet.document_extent
            && self.colour==r.document_color() && self.pages.iter().all(|page|page.read(None).is_some())
            && packet.dabs.is_empty() && self.requests.iter().all(|(target,index,operation)|
                packet.dab_batches.iter().any(|batch|batch.target==*target && batch.kind==DabBatchKind::RasterOperation(*index))
                && packet.scene.operations(*target).and_then(|operations|operations.get(*index as usize))==Some(operation))
            && self.sources.iter().all(|(target,capture)|capture.matches_context(packet.scene) && r.current_raster_versions(*target).is_ok_and(|versions|
                [RasterPlane::Color,RasterPlane::WatercolorWetness].into_iter().all(|plane|capture.matches_source(plane,&versions)==Some(true))))
    }
    pub(crate) fn preparation_bytes(&self,known:&mut RetainedTextures)->u64 {self.pages.iter().map(|page|page.preparation_bytes(known)).sum()}
    pub(crate) fn adopt(self,r:&mut WgpuRasterizer,receipt:&submission::Submission)->Result<Vec<(SourceTarget,u32)>,GpuRasterError> {
        let inputs=self.pages.iter().map(|page| {
            let (target,plane,coordinate)=page.key();let intent=r.raster_intent(target,plane,coordinate)?;
            if page.read(Some(receipt)).is_none() || !r.paint_layers.iter().any(|layer|layer.id==target) {return Err(GpuRasterError::Invariant("accepted input is not readable"));}
            Ok(intent)
        }).collect::<Result<Vec<_>,GpuRasterError>>()?;
        for (page,intent) in self.pages.into_iter().zip(inputs) {
            let (target,_,coordinate)=page.key();
            r.record_raster_write(target,page.read(Some(receipt)).ok_or(GpuRasterError::Invariant("accepted input is not readable"))?.replace(&intent,receipt))?;
            let layer=r.paint_layers.iter_mut().find(|layer|layer.id==target).ok_or(GpuRasterError::Invariant("prepared adopt state is inconsistent"))?;
            match page {
                CommitPage::Color {mut page,..}=>{page.derived=None;page.stroke_start=None;layer.pages.retain(|page|page.coordinate!=coordinate);layer.pages.push(page);}
                CommitPage::Wetness {mut page,..}=>{page.derived=None;page.pending=None;layer.watercolor_wetness_pages.retain(|page|page.coordinate!=coordinate);layer.watercolor_wetness_pages.push(page);}
            }
        }
        Ok(self.requests.into_iter().map(|(target,index,_)|(target,index)).collect())
    }
}

pub(super) struct PaintTransforms([ImageTransformState; 2]);
#[derive(Clone)]
pub(crate) struct PreparedRegion(Region);
#[derive(Clone)]
enum Region {
    Copy {
        source: wgpu::TextureView,
        destination: wgpu::TextureView,
        origin: wgpu::Origin3d,
        size: wgpu::Extent3d,
        allocation: Arc<region_batches::Allocation>,
        valid: Arc<std::sync::atomic::AtomicBool>,
    },
    Draw {
        source: SourceTarget,
        values: [u8; resample::UNIFORM_BYTES as usize],
        sampling: [wgpu::TextureView; 2],
        destination: wgpu::TextureView,
        texels: [u32; 4],
        mesh: Option<Arc<mesh::MeshGeometry>>,
        kept: bool,
    },
}
impl PreparedRegion {
    pub fn pipelines<'a>(&self, r: &'a WgpuRasterizer) -> [Option<&'a Deferred<wgpu::RenderPipeline>>; 2] {
        match &self.0 {
            Region::Copy { .. } => [None, None],
            Region::Draw { mesh, kept, .. } => {
                let pipelines = &r.scene_pipelines.resample.mesh;
                if mesh.is_some() { [Some(&pipelines[usize::from(*kept)]), Some(&pipelines[2+usize::from(*kept)])] }
                else { [Some(&pipelines[4+usize::from(*kept)]), None] }
            }
        }
    }
    pub fn values(&self) -> Option<&[u8; resample::UNIFORM_BYTES as usize]> {
        match &self.0 { Region::Draw { values, .. } => Some(values), Region::Copy { .. } => None }
    }
    pub fn encode(&self, r: &mut WgpuRasterizer, encoder: &mut submission::CommandEncoder,
        uniforms: &wgpu::Buffer, offset: u64,
    ) -> Result<(), GpuRasterError> {
        match &self.0 {
            Region::Copy { source, destination, origin, size, allocation, valid } => {
                encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo { origin: *origin, ..source.texture().as_image_copy() },
                    wgpu::TexelCopyTextureInfo { origin: *origin, ..destination.texture().as_image_copy() }, *size);
                submission::CacheWrite::shared(valid.clone()).track(encoder);
                let lease = allocation.clone(); encoder.hold(lease);
            },
            Region::Draw { source, sampling, destination, texels, mesh, kept, .. } => {
                let mut transforms = r.transforms.take().unwrap();
                let result: Result<(), GpuRasterError> = (|| {
                    let mesh = if let Some(geometry) = mesh {
                        let state = transforms.0.iter_mut().find(|state|state.preview.as_ref().is_some_and(|preview|preview.target==*source)).unwrap();
                        state.display_mesh.upload(r, encoder, geometry)?;
                        Some((&state.display_mesh,geometry.range_for_region(layer_core::Rect::UNBOUNDED)))
                    } else { None };
                    let pass = &r.scene_pipelines.resample;
                    let binding = pass.mesh_binding_at(&r.device, uniforms, offset, sampling);
                    pass.encode_mesh(encoder,&binding,destination,*texels,mesh,*kept);
                    Ok(())
                })();
                r.transforms = Some(transforms);
                result?;
            },
        }
        Ok(())
    }
}
pub(super) struct Recipes {
    passes: [Arc<pixel_transform::Kernels>; 3],
    positions: Arc<mesh::Pipelines>,
}
impl PaintTransforms {
    pub fn recipes(&self) -> Arc<Recipes> {
        let state = &self.0[0];
        Arc::new(Recipes { passes: [&state.color, &state.scalar, &state.visibility].map(|p| p.kernels.clone()),
            positions: state.positions.pipelines.clone() })
    }
    pub fn from_recipes(recipes: &Recipes) -> Self {
        let [color, scalar, visibility] = recipes.passes.each_ref().map(|p| PixelTransform::from_kernels(p.clone()));
        let primary = ImageTransformState::with_passes(color, scalar, visibility, mesh::Positions::from_pipelines(recipes.positions.clone()));
        let companion = primary.fork();
        Self([primary, companion])
    }
    pub(super) fn placement_pass(&self, scalar: bool) -> PixelTransform {
        if scalar { self.0[0].scalar.fork() } else { self.0[0].color.fork() }
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let primary = ImageTransformState::new(device);
        let companion = primary.fork();
        Self([primary, companion])
    }
    pub fn pipelines(&self) -> [Deferred<wgpu::RenderPipeline>; 5] {
        let [color, scalar, visibility] = self.0[0].pipelines();
        [color, scalar, visibility, self.0[0].color.linear_affine_pipeline(false), self.0[0].color.linear_affine_pipeline(true)]
    }
    /// What warp meshes draw with, compiled when a mesh is first shown.
    pub fn mesh_pipelines(&self) -> [Deferred<wgpu::RenderPipeline>; 1] {
        self.0[0].mesh_pipelines()
    }
    /// Drag previews draw into the display with these once they are ready;
    /// they never delay input.
    pub fn display_pipelines(&self) -> [Deferred<wgpu::ComputePipeline>; 1] {
        [self.0[0].color.display_pipeline()]
    }
    pub fn placement_pipelines(&self) -> [Deferred<wgpu::ComputePipeline>; 2] {
        [self.placement_pipeline(false, &Default::default()), self.placement_pipeline(true, &Default::default())]
    }
    pub fn linear_affine_placement_pipelines(&self) -> [Deferred<wgpu::ComputePipeline>; 2] {
        [false, true].map(|area| self.0[0].color.linear_affine_placement_pipeline(area))
    }
    pub fn placement_pipeline(&self, scalar: bool, transform: &layer_core::ImageTransform) -> Deferred<wgpu::ComputePipeline> {
        let state = &self.0[0];
        let pass = if scalar { &state.scalar } else { &state.color };
        pass.placement_pipeline(transform, pixel_transform::exact_taps(transform))
    }
    pub fn require_frame(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, preview: Option<&layer_render::TransformPreview>, required: &mut compile::Needs) {
        let mut require = |target: SourceTarget, transform: &layer_core::ImageTransform, taps| {
            let state = &self.0[0];
            let wet = packet.dab_batches.iter().any(|batch|batch.target==target && batch.style.execution==BrushExecution::Watercolor)
                    || r.paint_layers.iter().any(|layer| layer.id == target && !layer.watercolor_wetness_pages.is_empty())
                    || r.native_backing(target).is_some_and(|data| data.has_plane(layer_core::raster::RasterPlane::WatercolorWetness))
                    || packet.scene.raster(target).and_then(|raster| raster.try_data()).and_then(Result::ok).is_some_and(|data| data.has_plane(layer_core::raster::RasterPlane::WatercolorWetness));
            state.extend_pipelines(target,transform,taps,wet,required);
        };
        for state in &self.0 {
            if let Some(previous)=&state.preview { require(previous.target,&previous.drawn(),preview_taps(previous)); }
        }
        let mut selected = false;
        let mut mesh = false;
        for batch in packet.dab_batches {
            if let DabBatchKind::RasterOperation(index) = batch.kind
                && let Some(layer_core::RasterOperation {kind: layer_core::RasterOperationKind::Transform(transform), coverage, ..}) = packet.scene.operations(batch.target).and_then(|ops| ops.get(index as usize)) {
                selected |= coverage.selection.is_some();
                mesh |= transform.placement.mesh.is_some();
                require(batch.target, transform, pixel_transform::exact_taps(transform));
            }
        }
        if let Some(preview) = preview {
            let companion = preview.companion(packet.scene);
            for preview in std::iter::once(preview).chain(companion.as_ref()) {
                selected |= preview.selection.is_some();
                require(preview.target, &preview.drawn(), preview_taps(preview));
            }
            mesh |= preview.transform.placement.mesh.is_some();
        }
        if mesh { required.render.extend(self.mesh_pipelines()); }
        if preview.is_some() { required.compute.extend(self.display_pipelines()); }
        if selected { required.compute.extend(r.selection_clip.pipelines().into_iter().cloned()); }
    }
    pub fn materialization_needs(&self) -> compile::Needs {
        let mut required=compile::Needs::default();
        for state in self.0.iter().filter(|state|!state.native_preview) {
            if let Some(preview)=&state.preview {
                state.extend_pipelines(preview.target,&preview.drawn(),preview_taps(preview),state.sources[1].is_some(),&mut required);
            }
        }
        required
    }
    pub fn begin_frame(&mut self) {
        for t in &mut self.0 {
            t.begin_frame();
        }
    }
    pub(crate) fn charged(mut self, tier: &Arc<crate::residency::Tier>) -> Self {
        for state in &mut self.0 { state.charge = tier.charge(0); state.account(); }
        self
    }
    pub(crate) fn charged_bytes(&self) -> u64 { self.0.iter().map(|state| state.charge.bytes()).sum() }
    pub fn preparation_bytes(&self,known:&mut RetainedTextures)->u64 {
        self.0.iter().map(|state|state.preparation_bytes(known)).sum()
    }

    pub fn has_preview(&self) -> bool {
        self.0.iter().any(ImageTransformState::has_preview)
    }

    pub fn discard_preview(&mut self) {
        for t in &mut self.0 {
            t.discard_preview();
        }
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        index: u32,
        operation: &layer_core::RasterOperation,
    ) -> Result<(), GpuRasterError> {
        let plan=r.accepted_transform_plan(layer,index)?;
        self.0[0].apply(r, encoder, layer, operation, r.target_extent(layer), plan)
    }
    pub(crate) fn prepare_commit(&self,r:&WgpuRasterizer,packet:FramePacket<'_>)->Result<Option<PreparedTransformCommit>,GpuRasterError> {
        let states=self.0.iter().filter(|state|state.has_preview()).collect::<Vec<_>>();
        if states.is_empty() {return Ok(None);}
        let Some(matches)=states.iter().map(|state|state.matching_commit(packet)).collect::<Option<Vec<_>>>() else {return Ok(None)};
        let mut result=PreparedTransformCommit {owner:packet.scene.owner(),revision:packet.scene.revision(),extent:packet.document_extent,
            colour:r.document_color(),requests:Vec::new(),sources:Vec::new(),pages:Vec::new()};
        for (state,(target,index)) in states.into_iter().zip(matches) {
            let Some(capture)=state.capture.as_ref() else {return Ok(None)};
            if !capture.matches_context(packet.scene) {return Ok(None);}
            let current=r.current_raster_versions(target)?;
            if [RasterPlane::Color,RasterPlane::WatercolorWetness].into_iter().any(|plane|capture.matches_source(plane,&current)!=Some(true)) {return Ok(None);}
            let preview=state.preview.as_ref().unwrap();
            let layer=r.paint_layers.iter().find(|layer|layer.id==target).ok_or(GpuRasterError::NotReady)?;
            let regions=state.plan.channel_regions(&preview.drawn(),r.target_extent(target));
            for channel in 0..2 {
                if state.sources[channel].is_none() {continue;}
                let plane=if channel==0 {RasterPlane::Color}else {RasterPlane::WatercolorWetness};
                let coordinates=regions[channel].into_iter().flat_map(page_coordinates).collect::<std::collections::BTreeSet<_>>();
                for coordinate in coordinates {
                    let version=state.native_version(capture,&preview.drawn(),r.target_extent(target),plane,coordinate,preview_taps(preview))?;
                    if channel==0 {
                        let Some(page)=layer.pages.iter().find(|page|page.coordinate==coordinate) else {return Ok(None)};
                        if page.active().read(&version,None).is_none() {return Ok(None);}
                        result.pages.push(CommitPage::Color {target,version,page:LayerPage {coordinate,primary:page.primary.shared_storage(),
                            secondary:page.secondary.as_ref().map(resident::Resident::shared_storage),active_secondary:page.active_secondary,
                            primary_needs_clear:page.primary_needs_clear,stroke_start:None,derived:None}});
                    } else {
                        let Some(page)=layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate) else {return Ok(None)};
                        if page.active().read(&version,None).is_none() {return Ok(None);}
                        result.pages.push(CommitPage::Wetness {target,version,page:WatercolorWetnessPage {coordinate,primary:page.primary.shared_storage(),
                            secondary:page.secondary.shared_storage(),active_secondary:page.active_secondary,primary_needs_clear:page.primary_needs_clear,
                            secondary_needs_clear:page.secondary_needs_clear,pending:None,derived:None}});
                    }
                }
            }
            result.requests.push((target,index,packet.scene.operations(target).unwrap()[index as usize].clone()));
            result.sources.push((target,capture.clone()));
        }
        Ok(Some(result))
    }
    pub(crate) fn declared_capture(&self,r:&WgpuRasterizer,scene:SceneView<'_>,desired:&layer_render::TransformPreview)->Result<crate::scene::Capture,GpuRasterError> {
        let versions=r.raster_versions(scene,desired.target)?;
        let state=self.0.iter().find(|state|state.preview.as_ref().is_some_and(|preview|preview.target==desired.target)
            || state.prepared_preview.as_ref().is_some_and(|(preview,_,_)|preview.target==desired.target));
        let mesh=state.and_then(|state|state.mesh.as_ref()).filter(|(placement,_,_)|*placement==desired.drawn().placement).map(|(_,_,mesh)|mesh.clone());
        if let Some(capture)=state.and_then(|state|state.capture.as_ref()).filter(|capture|capture.matches_context(scene)
            && [RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness].into_iter().all(|plane|capture.matches_source(plane,&versions)==Some(true))) {
            return Ok(capture.with_preview(desired.clone(),mesh).with_taps(preview_taps(desired)));
        }
        Ok(crate::scene::Capture::new(scene.snapshot(),versions,desired.clone(),mesh).with_taps(preview_taps(desired)))
    }
    pub(crate) fn requested_version_for_capture(&self,r:&WgpuRasterizer,wanted:&crate::scene::Capture,plane:RasterPlane,coordinate:[u32;2])->Result<layer_core::raster::ContentVersion,GpuRasterError> {
        let desired=wanted.desired_preview();
        let state=self.0.iter().find(|state|state.preview.as_ref().is_some_and(|preview|preview.target==desired.target && preview.selection==desired.selection)
            || state.prepared_preview.as_ref().is_some_and(|(preview,_,_)|preview.target==desired.target && preview.selection==desired.selection))
            .ok_or(GpuRasterError::NotReady)?;
        let held=state.capture.as_ref().ok_or(GpuRasterError::NotReady)?;
        if held.matches_source(plane,wanted.source_versions())!=Some(true) {return Err(GpuRasterError::NotReady);}
        state.native_version(wanted,&desired.drawn(),r.target_extent(desired.target),plane,coordinate,preview_taps(desired))
    }
    pub(crate) fn read_native<'a>(&self,r:&'a WgpuRasterizer,target:SourceTarget,plane:RasterPlane,coordinate:[u32;2],wanted:&crate::scene::Capture,receipt:Option<&submission::Submission>)
        ->Result<NativeRead<'a>,GpuRasterError> {
        if target.is_coverage() {return Ok(NativeRead::Display);}
        if wanted.desired_preview().target!=target {return Err(GpuRasterError::Invariant("prepared read native state is inconsistent"));}
        let state=self.0.iter().find(|state|state.preview.as_ref().is_some_and(|preview|preview.target==target)).ok_or(GpuRasterError::NotReady)?;
        if !state.native_preview && !state.queried.contains(&coordinate) {return Ok(NativeRead::Display);}
        let expected=self.requested_version_for_capture(r,wanted,plane,coordinate)?;
        if let Some(read)=destination(r,target,usize::from(plane==RasterPlane::WatercolorWetness),false,coordinate)
            .and_then(|surface|surface.read(&expected,receipt)) {return Ok(NativeRead::Resident(read));}
        if expected.matches(&wanted.original(plane,coordinate))==Some(true) {return Ok(NativeRead::Original(expected));}
        Err(GpuRasterError::NotReady)
    }
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        let mut damage = Vec::with_capacity(2);
        for t in &mut self.0 {
            damage.extend(t.cancel_preview(r, encoder)?);
        }
        Ok(damage)
    }
    pub fn cancel_preparation(&mut self) {
        for state in &mut self.0 {state.preparing=None;}
    }
    pub(crate) fn cancel_restores(&self,r:&WgpuRasterizer)->Vec<(SourceTarget,TileKey)> {
        let mut pages:Vec<_>=self.0.iter().flat_map(|state|state.cancel_restores(r)).collect();
        pages.sort_unstable();pages
    }
    pub fn prepare_cancel(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        self.0.iter_mut().try_for_each(|state|state.prepare_cancel(r,encoder))
    }
    pub fn prepare_preview(&mut self,r:&mut WgpuRasterizer,encoder:&mut submission::CommandEncoder,
        next:&layer_render::TransformPreview,scene:SceneView<'_>,level:u32,fine:bool)->Result<(),GpuRasterError> {
        let local=|next:&layer_render::TransformPreview,extent| if fine {1} else {input_level(level,next,extent)};
        let previews=[Some(next.clone()),next.companion(scene)];
        let projected=previews.iter().flatten().filter(|next|!next.target.is_coverage()).try_fold(0u64,|bytes,next| {
            let extent=scene.target_extent(next.target);let phase=crate::scene::page_phase(scene.target_offset(next.target));
            let plan=display_mips::Plan::at([extent[0]+phase[0],extent[1]+phase[1]],local(next,extent));
            bytes.checked_add(plan.pixel_bytes_through(display_mips::MAX_LEVEL)*if self.input_requirements(next,local(next,extent),extent).1 {2}else {1}).ok_or(GpuRasterError::SizeOverflow)
        })?;
        let restore=r.projected_restore_bytes().checked_add(projected).ok_or(GpuRasterError::SizeOverflow)?;
        r.admit_preparation_with_transforms(restore,r.bake_preflight.as_ref().map_or(0,|bake|bake.projected_bytes()),Some(self))?;
        for (state,next) in self.0.iter_mut().zip(previews) {
            let Some(next)=next else {state.preparing=None;continue;};
            if next.target.is_coverage() {state.preparing=None;continue;}
            let same=state.holds_source(&next,Some(scene));
            let extent=scene.target_extent(next.target);
            let source=if same {state.preparing=None;&mut *state}else {
                let prepared=state.preparing.as_ref().is_some_and(|state|state.prepared_preview.as_ref().is_some_and(|(old,owner,revision)|*owner==scene.owner() && *revision==scene.revision()
                    && old.transaction==next.transaction && old.target==next.target && old.selection==next.selection));
                if !prepared {
                    let mut candidate=state.fork();
                    candidate.capture_source(r,encoder,next.target,scene.paint_base(next.target),next.selection.as_ref(),(extent,Some(scene)),None)?;
                    candidate.prepared_preview=Some((next.clone(),scene.owner(),scene.revision()));
                    state.preparing=Some(Box::new(candidate));
                }
                state.preparing.as_mut().unwrap()
            };
            source.prepare_reduced(r,encoder,next.selection.as_ref(),Some((scene,next.target)),extent,local(&next,extent))?;
        }
        Ok(())
    }
    pub fn update_preview(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview, scene: SceneView<'_>, level: Option<u32>,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        let mut damage = Vec::new();
        let capacity=r.scale_display.as_ref().and_then(|cache|cache.evaluation.fine_capacity());
        let nexts = [Some(next.clone()), next.companion(scene)];
        let native = |r: &WgpuRasterizer, next: &layer_render::TransformPreview| level.is_none() || r.layer_masks.definitions.contains_key(&next.target);
        let mut updates = Vec::with_capacity(nexts.len());
        for (state, next) in self.0.iter_mut().zip(&nexts) {
            state.fine_steps=BATCH_ORIGINAL_TILES;
            state.fine_progress.retain(|progress|progress.valid.load(std::sync::atomic::Ordering::Acquire));
            updates.push(next.as_ref().map(|next| state.plan_preview(r, encoder, next, Some((scene, next.target)), r.target_extent(next.target), native(r, next))).transpose()?);
        }
        for ((state, next), update) in self.0.iter_mut().zip(nexts).zip(updates) {
            if let (Some(next), Some(update)) = (next, update) {
                let extent = r.target_extent(next.target);
                let native = native(r, &next);
                damage.extend(state.draw_preview(r, encoder, &next, native, update)?);
                if !native && capacity.is_some() && state.native_retirement_pending() {continue;}
                if let Some(level) = level.filter(|_| !native) {
                    if let Some(capacity)=capacity {
                        state.prepare_fine_input(r,extent,crate::scene::page_phase(scene.target_offset(next.target)),next.selection.as_ref(),capacity)?;
                    } else {
                        let local = input_level(level, &next, extent);
                        state.prepare_reduced(r, encoder, next.selection.as_ref(), Some((scene, next.target)), extent, local)?;
                    }
                } else if state.reduced.as_ref().is_some_and(|input|input.fine.is_some()) {state.reduced=None;}
            } else { damage.extend(state.cancel_preview(r, encoder)?); }
        }
        Ok(damage)
    }
    pub fn prepare_standby(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection, level: u32,
    ) -> Result<bool, GpuRasterError> {
        if !matches!(id, SourceTarget::Paint(_)) || scene.raster(id).is_none() { return Ok(true); }
        let state = &mut self.0[0];
        if state.preview.is_some() { return Ok(true); }
        {
            let mip = r.display_pipelines.get_or_insert_with(|| r.recipes.display.clone());
            let ready = r.compilation.require([&r.scene_pipelines.source.pipeline], startup::OTHER)
                & r.compilation.require([&state.color.display_pipeline()], startup::OTHER)
                & r.compilation.require([&mip.reduce, &mip.fused_reduce], startup::OTHER)
                & r.compilation.require(r.selection_clip.pipelines(), startup::OTHER);
            if !ready { return Ok(false); }
        }
        let result=(|| {
            let extent = r.target_extent(id);
            let standby = Standby::of(scene, id, selection);
            if state.standby.as_ref() != Some(&standby) {
                state.capture_source(r, encoder, id, None, Some(selection), (extent,None), None)?;
                state.standby = Some(standby);
            }
            let local = sampling::selection_level(level, Some(selection), extent);
            state.prepare_reduced(r, encoder, Some(selection), Some((scene, id)), extent, local)
        })();
        match result {Err(error) if error.is_preparation_pending()=>Ok(false),result=>result.map(|()|true)}
    }
    pub fn release_standby(&mut self) {
        for state in &mut self.0 {
            if state.preview.is_none() && state.standby.is_some() { state.release_snapshot(); }
        }
    }
    pub fn standby_requirements(&self, scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection, requested: u32, extent: [u32; 2]) -> (u32, bool) {
        let key = Standby::of(scene, id, selection);
        let state = self.0.iter().find(|state| state.standby.as_ref() == Some(&key));
        let level = state.and_then(|state| state.reduced.as_ref()).map_or(requested, |input| input.level.min(requested));
        let bounds = state.and_then(|state| state.sources[0].as_ref()).map_or(PixelRect::full(extent), |s| s.bounds);
        (level, keeps_pixels(Some(selection), bounds))
    }
    pub fn display_source(&self, id: SourceTarget) -> bool {
        self.0.iter().any(|t| t.preview.as_ref().is_some_and(|p| p.target == id) && !t.native_preview)
    }
    pub fn fine_compatible(&self,preview:&layer_render::TransformPreview)->bool {
        self.0.iter().find(|state|state.displayable(preview)).is_none_or(|state|state.reduced.as_ref()
            .is_none_or(|input|input.fine.is_some() || input.level==1))
    }
    pub fn finish_frame(&mut self) {
        for state in &mut self.0 {state.fine_progress.retain(|progress|progress.valid.load(std::sync::atomic::Ordering::Acquire));}
    }
    pub fn finish_region(&mut self, region: &PreparedRegion) {
        if let Region::Copy { allocation, .. } = &region.0 {
            for state in &mut self.0 {state.fine_progress.retain(|progress|!Arc::ptr_eq(&progress.allocation,allocation));}
        }
    }
    pub fn waiting_for_display_input(&self,capacity:Option<usize>)->bool {
        self.0.iter().any(|state|state.preview.is_some() && !state.native_preview
            && (state.reduced.is_none() || capacity.is_some_and(|capacity|state.native_retirement_pending()
                || state.reduced.as_ref().and_then(|input|input.fine.as_ref()).is_none_or(|fine|fine.capacity!=capacity))))
    }
    pub fn fine_progress_bytes(&self)->u64 {
        self.0.iter().flat_map(|state|&state.fine_progress).map(|p|p.allocation.bytes).sum()
    }
    pub fn fine_capacity(&self,preview:&layer_render::TransformPreview)->Option<usize> {
        self.0.iter().find(|state|state.displayable(preview)).and_then(|state|state.reduced.as_ref())
            .and_then(|input|input.fine.as_ref()).map(|fine|fine.capacity)
    }
    pub fn input_requirements(&self, preview: &layer_render::TransformPreview, requested: u32, extent: [u32; 2]) -> (u32, bool) {
        let state = self.0.iter().find(|t| t.displayable(preview));
        let level = state.and_then(|t| t.reduced.as_ref()).map_or(requested, |input| input.level.min(requested));
        let kept = state.map_or_else(|| keeps_pixels(preview.selection.as_ref(), PixelRect::full(extent)), |t| t.keeps_pixels(preview));
        (level, kept)
    }
    pub fn materialize_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        scene: SceneView<'_>, region: DocRect,
    ) -> Result<(), GpuRasterError> {
        #[cfg(test)]
        crate::scene::sources::assert_reading("transform materialization");
        if !self.materialization_needs().require(&r.compilation,startup::DOCUMENT) {return Err(GpuRasterError::NotReady);}
        let scene = scene.with_offset64(region.min.map(|value| -(value as f64)));
        let local = PixelRect::full(region.size()?);
        for state in self.0.iter_mut().filter(|t| !t.native_preview && t.preview.is_some()) {
            let preview = state.preview.clone().unwrap();
            let extent = r.target_extent(preview.target);
            let bounds = PixelRect::full(extent);
            let placement=scene::placement::Placement::identity(bounds,scene.target_offset(preview.target));
            let jobs=placement.footprints(local)?;
            let pages: std::collections::BTreeSet<_> = jobs.into_iter().flat_map(|job| job.sources).collect();
            let retired: Vec<_> = state.queried.difference(&pages).copied().map(page_rect).collect();
            state.render_source(r, encoder, preview.target, &Default::default(), pixel_transform::PREVIEW_TAPS, &retired,TransformWrite::Restore(preview_taps(&preview)))?;
            let missing: Vec<_> = pages.difference(&state.queried).map(|c| page_rect(*c).intersect(bounds)).collect();
            state.render_source(r, encoder, preview.target, &preview.drawn(), preview_taps(&preview), &missing,TransformWrite::Preview)?;
            let retained: Vec<_> = pages.iter().copied().map(page_rect).collect();
            state.retain_pages(r, preview.target, &retained, Some(&preview.transform));
            state.queried = pages;
        }
        Ok(())
    }
    #[expect(clippy::too_many_arguments, reason = "Paint transforms keep source placement and output sampling regions explicit")]
    pub fn prepare_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        id: SourceTarget, placement: layer_core::Affine, output: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel, target: display_mips::Plan, region: DocRect, fine_budget:u64,
    ) -> Result<PreparedRegion, GpuRasterError> {
        let fine_budget=fine_budget.saturating_sub(self.fine_progress_bytes());
        let state = self.0.iter_mut().find(|t| t.preview.as_ref().is_some_and(|p| p.target == id)).unwrap();
        let preview = state.preview.clone().unwrap();
        state.prepare_region(r, encoder, &preview, r.target_extent(id), placement, output, display, target, region, fine_budget)
    }

}

struct ImageTransformState {
    color: PixelTransform,
    scalar: PixelTransform,
    visibility: PixelTransform,
    background: Option<f32>,
    sources: [Option<TileSnapshot>; 2],
    captures: [std::collections::BTreeMap<[u32; 2], snapshot::SnapshotPage>; 2],
    selection: Option<wgpu::Buffer>,
    has_selection: bool,
    plan: Plan,
    original_pages: [Vec<[u32; 2]>; 2],
    preview: Option<layer_render::TransformPreview>,
    capture:Option<crate::scene::Capture>,
    prepared_preview: Option<(layer_render::TransformPreview,u64,u64)>,
    preparing:Option<Box<ImageTransformState>>,
    preview_regions: [PixelRect; 2],
    native_preview: bool,
    queried: std::collections::BTreeSet<[u32; 2]>,
    reduced: Option<snapshot::DisplayInputs>,
    fine_progress: Vec<FineProgress>,
    fine_steps:usize,
    retired_native:Vec<(PreviewPages,Arc<region_batches::Allocation>)>,
    standby: Option<Standby>,
    spares: PreviewPages,
    atlases: [Option<Atlas>; 2],
    /// Mesh positions for each window of pages a warp draws.
    positions: mesh::Positions,
    display_mesh: MeshBuffers,
    /// The warp mesh last tessellated, the display tolerance it was
    /// tessellated within, if any, and its geometry.
    mesh: Option<(layer_core::LayerPlacement, Option<f32>, Arc<mesh::MeshGeometry>)>,
    charge: crate::residency::Charge,
}

#[derive(Clone,Copy,PartialEq)]
pub(crate) struct Plan {source_bounds:[PixelRect;2],cut:layer_core::Rect}
impl Plan {
    const EMPTY:Self=Self {source_bounds:[PixelRect::EMPTY;2],cut:layer_core::Rect::EMPTY};
    pub(crate) fn of<I:Iterator<Item=[u32;2]>>(pages:[I;2],base:Option<&layer_core::authored::PaintBase>,selection:Option<&layer_core::Selection>,extent:[u32;2])->Self {
        let full=PixelRect::full(extent);
        let mut source_bounds=pages.map(|pages|pages.fold(PixelRect::EMPTY,|bounds,c|bounds.union(page_rect(c).intersect(full))));
        if let Some(base)=base {source_bounds[0]=source_bounds[0].union(source_access::paint_base_bounds(base).intersect(full));}
        let source=source_bounds.into_iter().fold(PixelRect::EMPTY,PixelRect::union);
        if source.is_empty() {return Self {source_bounds,cut:layer_core::Rect::EMPTY};}
        let mut cut=source.to_rect();
        if let Some(bounds)=selection.filter(|s|!s.inverted).map(|s|s.bounds()) {
            cut.min.x=cut.min.x.max(bounds.min.x);cut.min.y=cut.min.y.max(bounds.min.y);
            cut.max.x=cut.max.x.min(bounds.max.x);cut.max.y=cut.max.y.min(bounds.max.y);
        }
        Self {source_bounds,cut}
    }
    fn covers(&self,other:&Self)->bool {
        let cut=|a:layer_core::Rect,b:layer_core::Rect|b.is_empty() || (a.min.x<=b.min.x && a.min.y<=b.min.y && a.max.x>=b.max.x && a.max.y>=b.max.y);
        self.source_bounds.iter().zip(other.source_bounds).all(|(a,b)|b.is_empty() || a.union(b)==*a) && cut(self.cut,other.cut)
    }
    pub(crate) fn writes(&self,transform:&layer_core::ImageTransform,extent:[u32;2],wet:&std::collections::BTreeSet<[u32;2]>)->[std::collections::BTreeSet<[u32;2]>;2] {
        let color=transform.affected_regions(self.cut).into_iter().flat_map(|region|page_coordinates(pixel_rect(region,extent))).collect::<std::collections::BTreeSet<_>>();
        let support=self.channel_regions(transform,extent)[1];
        let wetness=color.iter().copied().filter(|c|!self.source_bounds[1].is_empty()
            && (wet.contains(c) || support.iter().any(|region|!region.page_local(*c).is_empty()))).collect();
        [color,wetness]
    }
    fn channel_regions(&self,transform:&layer_core::ImageTransform,extent:[u32;2])->[[PixelRect;2];2] {
        self.source_bounds.map(|b| {
            let cut=layer_core::Rect {
                min:layer_core::Point {x:(b.min_x() as f32).max(self.cut.min.x),y:(b.min_y() as f32).max(self.cut.min.y)},
                max:layer_core::Point {x:(b.max_x() as f32).min(self.cut.max.x),y:(b.max_y() as f32).min(self.cut.max.y)},
            };
            transform.affected_regions(cut).map(|b|pixel_rect(b,extent))
        })
    }
}
struct FineProgress {
    preview:layer_render::TransformPreview,
    destination:wgpu::TextureView,
    output:wgpu::TextureView,
    target:display_mips::Plan,
    region:DocRect,
    allocation:Arc<region_batches::Allocation>,
    values:[u8;resample::UNIFORM_BYTES as usize],
    regions:region_batches::Regions,
    demand:fine_demand::Demand,
    valid:Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FineProgress {
    fn drop(&mut self) {
        if Arc::strong_count(&self.allocation)>1 {self.allocation.retire();}
    }
}

#[derive(PartialEq)]
struct Standby {
    layer: SourceTarget,
    raster: u64,
    source: Option<(usize, [u32; 2])>,
    selection: layer_core::Selection,
}
impl Standby {
    fn of(scene: SceneView<'_>, id: SourceTarget, selection: &layer_core::Selection) -> Self {
        Self {
            layer: id,
            raster: scene.raster(id).map_or(0, |r| r.identity()),
            source: scene.paint_base(id).map(|base| (Arc::as_ptr(base.image.storage()) as usize, base.offset)),
            selection: selection.clone(),
        }
    }
}

/// Distinct source-cache tiles one batch may bind. Staying below the cache's
/// smallest capacity keeps every tile of a batch resident until it is drawn.
const BATCH_ORIGINAL_TILES: usize = 48;

/// Pages per window of a mesh transform, matching its positions texture.
const MESH_WINDOW: [u32; 2] = [mesh::WINDOW_PAGES; 2];

/// A destination page and the page-local part of it a transform draws.
struct Target<T = wgpu::Texture> {
    texture: T,
    region: PixelRect,
    initialize: bool,
}

struct PreviewUpdate {
    damage: Vec<(SourceTarget, PixelRect)>,
    draw: Option<PreviewDraw>,
}
struct PreviewDraw {
    regions: [PixelRect; 2],
    affected: [PixelRect; 3],
    source: Option<SourcePlan>,
}

struct SourcePlan {
    index: Option<usize>,
    mesh: Option<Arc<mesh::MeshGeometry>>,
    support: [[PixelRect; 2]; 2],
    coordinates: std::collections::BTreeSet<[u32; 2]>,
    initialize: std::collections::BTreeSet<[u32; 2]>,
    channels: Vec<ChannelPlan>,
    leases: Vec<crate::scene::sources::Lease>,
}
struct ChannelPlan {
    pages: Vec<[u32; 2]>,
    windows: Vec<Window<()>>,
}

/// Destination pages drawn together into the atlas, which holds the pages
/// from `origin`, then copied into place. Jobs may span several pages.
struct Window<T = wgpu::Texture> {
    origin: [u32; 2],
    /// Absolute destination regions, and whether each draws unmoved pixels.
    jobs: Vec<(snapshot::Footprint, bool)>,
    pages: Vec<([u32; 2], Target<T>)>,
}

/// The texels of `side` pixels each that cover `region`: the first, and how
/// many across and down.
pub(crate) fn texel_rect(region: PixelRect, side: u32) -> [u32; 4] {
    let low = [region.min_x() / side, region.min_y() / side];
    [low[0], low[1], region.max_x().div_ceil(side) - low[0], region.max_y().div_ceil(side) - low[1]]
}

/// `region` grown to whole texels of `side` pixels, within `extent`.
pub(crate) fn aligned(region: PixelRect, side: u32, extent: [u32; 2]) -> PixelRect {
    let near = |v: u32| v / side * side;
    let far = |v: u32, limit: u32| v.div_ceil(side).saturating_mul(side).min(limit);
    PixelRect::new(near(region.min_x()), near(region.min_y()), far(region.max_x(), extent[0]), far(region.max_y(), extent[1]))
}

/// `transform` followed by `placement`, from texels of the layer reduced in
/// `inputs` to texels of display `level`, sampled bilinearly. A warp resamples
/// from mesh positions instead and maps through the placement alone.
pub(crate) fn resample_map(
    transform: &layer_core::ImageTransform,
    placement: layer_core::Affine,
    inputs: &snapshot::DisplayInputs,
    level: u32,
) -> Result<layer_core::ImageTransform, GpuRasterError> {
    resample_map_at(transform,placement,inputs.level,inputs.phase,level)
}
fn resample_map_at(transform:&layer_core::ImageTransform,placement:layer_core::Affine,input_level:u32,phase:[u32;2],level:u32)
    ->Result<layer_core::ImageTransform,GpuRasterError> {
    let scale = |s: f32| layer_core::Projective([s, 0., 0., 0., s, 0., 0., 0., 1.]);
    let shift = |sign: f32| layer_core::Projective([1., 0., sign * phase[0] as f32, 0., 1., sign * phase[1] as f32, 0., 0., 1.]);
    if transform.placement.mesh.is_some() {
        let mut result = transform.clone();
        result.source_from_owner = Some([shift(1.), scale(1. / (1u32 << input_level) as f32)].into_iter()
            .try_fold(transform.source_from_owner.unwrap_or(layer_core::Projective::IDENTITY), layer_core::Projective::then)
            .ok_or(GpuRasterError::InvalidTransform("Invalid source placement"))?);
        return Ok(result);
    }
    let moved = transform.projective().unwrap_or(layer_core::Projective::IDENTITY);
    let map = [shift(-1.), moved, layer_core::Projective::from_affine(placement), scale(1. / (1u32 << level) as f32)]
        .into_iter()
        .try_fold(scale((1u32 << input_level) as f32), layer_core::Projective::then)
        .ok_or(GpuRasterError::InvalidTransform("Transform must be finite and invertible"))?;
    Ok(layer_core::ImageTransform { placement: layer_core::LayerPlacement { interpolation: layer_core::Interpolation::Linear, ..layer_core::LayerPlacement::from_projective(map) }, ..Default::default() })
}

pub(crate) fn fine_bytes(capacity:usize,kept:bool)->u64 {fine::FineInputs::bytes(capacity,kept)}
/// Taps per axis a preview's pages average over a minified pixel: the exact
/// count once it stops moving, since Apply may keep those pages.
fn preview_taps(preview: &layer_render::TransformPreview) -> u32 {
    let exact = pixel_transform::exact_taps(&preview.drawn());
    if preview.moving { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact }
}

/// Whether `selection` fully covers every pixel of `bounds`, so a transform
/// moves all of them and leaves none in place.
fn selects_all(selection: &layer_core::Selection, bounds: PixelRect) -> bool {
    let layer_core::SelectionShape::Contours(contours) = &selection.shape else {
        return false;
    };
    let [a, b, c, d, x, y] = selection.affine.0;
    match &contours[..] {
        [] => selection.inverted,
        [contour] if !selection.inverted && b == 0. && c == 0. && contour.len() == 4 => {
            let corners: Vec<_> = contour.iter().map(|p| [a * p.x + x, d * p.y + y]).collect();
            let rectangle = (0..4).all(|i| {
                let [p, q] = [corners[i], corners[(i + 1) % 4]];
                (p[0] == q[0]) != (p[1] == q[1])
            });
            let low = corners.iter().fold([f32::MAX; 2], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
            let high = corners.iter().fold([f32::MIN; 2], |m, p| [m[0].max(p[0]), m[1].max(p[1])]);
            rectangle
                && low[0] <= bounds.min_x() as f32
                && low[1] <= bounds.min_y() as f32
                && high[0] >= bounds.max_x() as f32
                && high[1] >= bounds.max_y() as f32
        }
        _ => false,
    }
}

pub(crate) fn keeps_pixels(selection: Option<&layer_core::Selection>, bounds: PixelRect) -> bool {
    selection.is_some_and(|selection| !selects_all(selection, bounds))
}

fn on_page(local: PixelRect, coordinate: [u32; 2]) -> PixelRect {
    let [x, y] = coordinate.map(|v| v * PAGE_SIZE);
    PixelRect::new(x + local.min_x(), y + local.min_y(), x + local.max_x(), y + local.max_y())
}

/// Consecutive jobs that together bind at most BATCH_ORIGINAL_TILES tiles
/// the transaction has not captured.
fn source_batches(
    jobs: &[(snapshot::Footprint, bool)],
    captured: impl Fn([u32; 2]) -> bool,
) -> Vec<std::ops::Range<usize>> {
    let mut batches = Vec::new();
    let mut originals = std::collections::HashSet::new();
    let mut start = 0;
    for (index, (job, _)) in jobs.iter().enumerate() {
        let needed: Vec<_> = job.sources.iter().filter(|c| !captured(**c)).collect();
        let fresh = needed.iter().filter(|c| !originals.contains(**c)).count();
        if index > start && originals.len() + fresh > BATCH_ORIGINAL_TILES {
            batches.push(start..index);
            start = index;
            originals.clear();
        }
        originals.extend(needed.into_iter().copied());
    }
    batches.push(start..jobs.len());
    batches
}

/// Scratch holding a window of destination pages. One pass per 256x256 page
/// would dominate the frame for large layers.
struct Atlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Atlas {
    fn pages(format: wgpu::TextureFormat) -> [u32; 2] {
        match format.block_copy_size(None).unwrap_or(16) {
            16.. => [4, 4],
            8.. => [8, 4],
            _ => [8, 8],
        }
    }
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let [columns, rows] = Self::pages(format);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("batched transform pages"),
            size: wgpu::Extent3d {
                width: columns * PAGE_SIZE,
                height: rows * PAGE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self { texture, view }
    }
    fn storage_bytes(&self) -> u64 {
        texture_bytes(&self.texture)
    }
}

/// Pages which left the live preview footprint. Reuse within the transaction,
/// not as document content; apply/cancel releases them. Capacity follows peak
/// live footprints, not the accumulated area visited by the moving selection.
#[derive(Default)]
struct PreviewPages {
    paint: Vec<LayerPage>,
    watercolor: Vec<WatercolorWetnessPage>,
    masks: Vec<layer_masks::MaskPage>,
}
impl PreviewPages {
    fn clear(&mut self) {
        self.paint.clear();
        self.watercolor.clear();
        self.masks.clear();
    }
    fn storage_bytes(&self) -> u64 {
        self.paint
            .iter()
            .map(|p| p.primary.storage_bytes() + p.secondary.as_ref().map_or(0, |surface|surface.storage_bytes()))
            .sum::<u64>()
            + self.watercolor.iter().map(|p| p.primary.storage_bytes() + p.secondary.storage_bytes()).sum::<u64>()
            + self.masks.iter().map(layer_masks::MaskPage::storage_bytes).sum::<u64>()
    }
}
impl ImageTransformState {
    pub fn new(device: &PipelineDevice) -> Self {
        let [color, scalar, visibility] = PixelTransform::passes(device);
        Self::with_passes(color, scalar, visibility, mesh::Positions::new(device))
    }
    pub fn fork(&self) -> Self {
        Self::with_passes(
            self.color.fork(),
            self.scalar.fork(),
            self.visibility.fork(),
            self.positions.fork(),
        )
    }
    fn with_passes(
        color: PixelTransform,
        scalar: PixelTransform,
        visibility: PixelTransform,
        positions: mesh::Positions,
    ) -> Self {
        Self {
            color,
            scalar,
            visibility,
            background: None,
            sources: Default::default(),
            captures: Default::default(),
            selection: None,
            has_selection: false,
            plan: Plan::EMPTY,
            original_pages: Default::default(),
            preview: None,
            capture:None,
            prepared_preview: None,
            preparing:None,
            preview_regions: [PixelRect::EMPTY; 2],
            native_preview: false,
            queried: Default::default(),
            reduced: None,
            fine_progress: Vec::new(),
            retired_native: Vec::new(),
            fine_steps:BATCH_ORIGINAL_TILES,
            standby: None,
            spares: PreviewPages::default(),
            atlases: Default::default(),
            display_mesh: MeshBuffers::default(),
            positions,
            mesh: None,
            charge: Arc::<crate::residency::Tier>::default().charge(0),
        }
    }
    pub fn pipelines(&self) -> [Deferred<wgpu::RenderPipeline>; 3] {
        [&self.color, &self.scalar, &self.visibility].map(|p| p.render_pipeline(&Default::default(), 1))
    }
    pub fn mesh_pipelines(&self) -> [Deferred<wgpu::RenderPipeline>; 1] {
        [self.positions.pipelines.pipeline.clone()]
    }
    pub fn begin_frame(&mut self) {
        self.retired_native.retain(|(_,lease)|Arc::strong_count(lease)>1);
        self.color.begin_frame();
        self.scalar.begin_frame();
        self.visibility.begin_frame();
        self.account();
    }
    fn spare_bytes(&self) -> u64 {
        self.spares.storage_bytes() + self.preparing.as_ref().map_or(0, |state| state.spare_bytes())
    }
    fn account(&mut self) {
        let bytes = self.storage_bytes().saturating_sub(self.spare_bytes());
        self.charge.resize(bytes);
    }
    pub fn storage_bytes(&self) -> u64 {
        self.spares.storage_bytes()
            + self.preparing.as_ref().map_or(0,|state|state.storage_bytes())
            + self.atlases.iter().flatten().map(Atlas::storage_bytes).sum::<u64>()
            + self.positions.storage_bytes()
            + self.display_mesh.storage_bytes()
            + self.color.storage_bytes()
            + self.selection.as_ref().map_or(0, wgpu::Buffer::size)
            + self.scalar.storage_bytes()
            + self.visibility.storage_bytes()
            + self.reduced.as_ref().map_or(0, snapshot::DisplayInputs::storage_bytes)
            + self.fine_progress.iter().map(|p|p.allocation.bytes).sum::<u64>()
            + self
                .captures
                .iter()
                .flat_map(|pages| pages.values())
                .map(|p| {
                    u64::from(p.texture.width())
                        * u64::from(p.texture.height())
                        * u64::from(p.texture.format().block_copy_size(None).unwrap())
                })
                .sum::<u64>()
    }
    fn preparation_bytes(&self,known:&mut RetainedTextures)->u64 {
        let captures=self.captures.iter().flat_map(|pages|pages.values());
        let captured=captures.clone().map(|page|texture_bytes(&page.texture)).sum::<u64>();
        let nested=self.preparing.as_ref().map_or(0,|state|state.storage_bytes());
        let owned=self.storage_bytes().saturating_sub(captured).saturating_sub(nested);
        owned+captures.chain(self.sources.iter().flatten().flat_map(|source|source.pages.values()))
            .filter(|page|known.insert(page.texture.clone())).map(|page|texture_bytes(&page.texture)).sum::<u64>()
            + self.preparing.as_ref().map_or(0,|state|state.preparation_bytes(known))
    }
    pub fn apply(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        operation: &layer_core::RasterOperation,
        extent: [u32; 2],
        plan: Plan,
    ) -> Result<(), GpuRasterError> {
        let layer_core::RasterOperationKind::Transform(transform) = &operation.kind else {
            unreachable!()
        };
        if transform.is_identity() {
            return Ok(());
        }
        self.capture_source(
            r,
            encoder,
            layer,
            transform.source_base.as_ref(),
            operation.coverage.selection.as_ref(),
            (extent,None),
            Some(plan),
        )?;
        let regions = transform
            .affected_regions(plan.cut)
            .map(|b| pixel_rect(b, extent));
        let taps = pixel_transform::exact_taps(transform);
        let result = self.render_source(r, encoder, layer, transform, taps, &regions,TransformWrite::Authored);
        let keeps_base = transform.source_base.is_none() && r.tiled_sources.contains_key(&layer);
        if result.is_ok() && !keeps_base && self.moves_everything(operation.coverage.selection.as_ref()) {
            let placed = regions[1];
            r.drop_vacated_pages(layer, |c| !placed.page_local(c).is_empty());
        }
        self.release_snapshot();
        result
    }
    /// Whether the captured transform moves every pixel its target holds,
    /// leaving only the pages its forward bounds reach.
    fn moves_everything(&self, selection: Option<&layer_core::Selection>) -> bool {
        let source = self.plan.source_bounds.into_iter().fold(PixelRect::EMPTY, PixelRect::union);
        self.sources[0].is_some()
            && !source.is_empty()
            && selection.is_none_or(|s| selects_all(s, source))
    }
    fn capture_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        base: Option<&layer_core::authored::PaintBase>,
        selection: Option<&layer_core::Selection>,
        source:([u32;2],Option<SceneView<'_>>),
        plan:Option<Plan>,
    ) -> Result<(), GpuRasterError> {
        let (extent,scene)=source;
        self.spares.clear();
        self.release_snapshot();
        self.has_selection = selection.is_some();
        let (background,pages)=if scene.is_some() && r.staged_transform_source(layer).is_none() {(None,[Vec::new(),Vec::new()])}else {source_pages(r,layer,Some(&encoder.submission()))?};
        let backing=if let Some(scene)=scene {
            scene.raster(layer).map(|root|root.try_data().ok_or(GpuRasterError::NotReady)?.map_err(GpuRasterError::Effect))
                .transpose()?.map(|data|(data,r.document_color().space))
        }else if let Some(source)=r.layer_masks.definitions.get(&layer) {Some((r.mask_data(layer,source)?,r.document_color().space))}
        else {background.is_none().then(||r.staged_transform_source(layer).map(|(data,_)|data.clone()).or_else(||r.native_backing(layer).cloned())
            .map(|data|(data,r.document_color().space))).flatten()};
        if let Some(source)=r.layer_masks.definitions.get(&layer) {
            let data=backing.as_ref().ok_or(GpuRasterError::NotReady)?.0.clone();
            let coordinates=data.tiles.keys().filter(|key|key.plane==RasterPlane::Mask).map(|key|key.coordinate)
                .chain(r.layer_masks.pages.keys().filter(|(target,_)|*target==layer).map(|(_,coordinate)|*coordinate));
            let captured=pages[0].iter().map(|(coordinate,_)|*coordinate).collect::<std::collections::BTreeSet<_>>();
            for coordinate in coordinates {
                if captured.contains(&coordinate) {continue;}
                let declared=data.tiles.get(&TileKey {plane:RasterPlane::Mask,coordinate}).map_or_else(
                    ||crate::raster::contents::mask_default(source.domain,source.default_coverage,coordinate),|tile|tile.content_version());
                if declared.matches(&r.current_mask_version(layer,coordinate)?)!=Some(true) {return Err(GpuRasterError::NotReady);}
            }
        }
        let original = background
            .is_none()
            .then(|| scene.map_or_else(||base.or_else(||r.tiled_sources.get(&layer)),|scene|scene.paint_base(layer)).cloned())
            .flatten();
        self.background = background;
        self.original_pages = std::array::from_fn(|i| pages[i].iter().map(|(c, _)| *c).collect());
        let planes = [if background.is_some() {layer_core::raster::RasterPlane::Mask} else {layer_core::raster::RasterPlane::Color}, layer_core::raster::RasterPlane::WatercolorWetness];
        if let Some((data, _)) = &backing {
            for (i, plane) in planes.iter().enumerate() {
                for key in data.tiles.keys().filter(|key| key.plane == *plane) {
                    if !self.original_pages[i].contains(&key.coordinate) { self.original_pages[i].push(key.coordinate); }
                }
            }
        }
        let versions=r.current_raster_versions(layer).ok();
        let content=|i:usize,coordinate:[u32;2]|backing.as_ref().is_some_and(|(data,_)|data.tiles.contains_key(&TileKey {plane:planes[i],coordinate}))
            || versions.as_ref().is_some_and(|versions|versions.authored(planes[i],coordinate));
        let derived=Plan::of(std::array::from_fn(|i|self.original_pages[i].iter().copied().filter(move |c|plan.is_none() || content(i,*c))),original.as_ref(),selection,extent);
        if plan.is_some_and(|plan|!plan.covers(&derived)) {
            debug_assert!(false,"transform source exceeds its accepted plan");
            return Err(GpuRasterError::Invariant("transform source exceeds its accepted plan"));
        }
        self.plan=plan.unwrap_or(derived);
        let source_bounds = self
            .plan
            .source_bounds
            .into_iter()
            .fold(PixelRect::EMPTY, PixelRect::union);
        if source_bounds.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        r.test.source_captures.update(|n| n + 1);
        if let Some(s) = selection {
            r.selection_clip.prepare(
                &r.device,
                encoder,
                extent,
                &std::sync::Arc::new(s.clone()),
            )?;
            let source = r.selection_clip.buffer.as_ref().unwrap();
            let target = self
                .selection
                .get_or_insert_with(|| selection_capture(&r.device, source.size()));
            if target.size() < source.size() {
                *target = selection_capture(&r.device, source.size());
            }
            encoder.copy_buffer_to_buffer(source, 0, target, 0, source.size());
        }
        for (channel, pages) in pages.into_iter().enumerate() {
            if channel == 0 || !self.plan.source_bounds[channel].is_empty() {
                let bounds = if self.plan.source_bounds[channel].is_empty() {
                    source_bounds
                } else {
                    self.plan.source_bounds[channel]
                };
                self.sources[channel] = Some(TileSnapshot {
                    pages: pages.into_iter().collect(),
                    base: if channel == 0 { original.clone() } else { None },
                    backing: backing.as_ref().map(|(data, space)| (data.clone(), *space, planes[channel])),
                    bounds,
                });
            }
        }
        for (channel, pages) in self.captures.iter_mut().enumerate() {
            pages.retain(|c, p| {
                self.sources[channel].as_ref().is_some_and(|s| {
                    s.pages
                        .get(c)
                        .is_some_and(|source| source.texture.format() == p.texture.format())
                })
            });
        }
        self.retain_pool_bindings();
        Ok(())
    }
    fn extend_pipelines(&self,target:SourceTarget,transform:&layer_core::ImageTransform,taps:u32,wet:bool,required:&mut compile::Needs) {
        let pass=if target.is_coverage() {&self.visibility} else {&self.color};
        required.render.push(pass.render_pipeline(transform,taps));
        if !transform.is_identity() {required.render.push(pass.render_pipeline(&Default::default(),pixel_transform::PREVIEW_TAPS));}
        if wet && !target.is_coverage() {
            required.render.push(self.scalar.render_pipeline(transform,taps));
            if !transform.is_identity() {required.render.push(self.scalar.render_pipeline(&Default::default(),pixel_transform::PREVIEW_TAPS));}
        }
    }
    fn render_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        transform: &layer_core::ImageTransform,
        taps: u32,
        regions: &[PixelRect],
        scope:TransformWrite,
    ) -> Result<(), GpuRasterError> {
        let plan = self.plan_source(r, encoder, layer, transform, regions)?;
        self.draw_source(r, encoder, layer, transform, taps, plan,scope,None)
    }
    /// Every job a render needs, with the original pages they read decoded,
    /// found before any page changes.
    fn plan_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        transform: &layer_core::ImageTransform,
        regions: &[PixelRect],
    ) -> Result<Option<SourcePlan>, GpuRasterError> {
        if self.sources[0].is_none() || regions.iter().all(|b| b.is_empty()) {
            return Ok(None);
        }
        let index = r.paint_layers.iter().position(|l| l.id == layer);
        let mesh = transform.placement.mesh.as_ref().map(|_| self.mesh_geometry(&transform.placement, None));
        let support = self.plan.channel_regions(transform, r.target_extent(layer));
        let coordinates: std::collections::BTreeSet<_> = regions
            .iter()
            .copied()
            .filter(|b| !b.is_empty())
            .flat_map(page_coordinates)
            .collect();
        let channel_pages: [Vec<[u32; 2]>; 2] = std::array::from_fn(|channel| {
            if self.sources[channel].is_none() {
                return Vec::new();
            }
            coordinates
                .iter()
                .copied()
                .filter(|c| {
                    channel == 0
                        || destination(r, layer, channel, false, *c).is_some()
                        || support[channel]
                            .iter()
                            .any(|b| !b.page_local(*c).is_empty())
                })
                .collect()
        });
        let receipt=encoder.submission();
        let mut initializes:[std::collections::BTreeSet<[u32;2]>;2]=Default::default();
        for channel in 0..2 {
            let Some(source)=self.sources[channel].as_ref() else {continue;};
            let plane=if self.background.is_some() {RasterPlane::Mask}else if channel==0 {RasterPlane::Color}else {RasterPlane::WatercolorWetness};
            for &coordinate in &channel_pages[channel] {
                let expected=if self.background.is_none() && (self.native_preview || self.queried.contains(&coordinate)) {
                    if let Some((capture,preview))=self.capture.as_ref().zip(self.preview.as_ref()) {
                        self.native_version(capture,&preview.drawn(),r.target_extent(layer),plane,coordinate,preview_taps(preview))?
                    }else {r.current_raster_version(layer,plane,coordinate)?}
                }else {r.current_raster_version(layer,plane,coordinate)?};
                let current=if self.background.is_some() {r.layer_masks.pages.get(&(layer,coordinate)).is_some_and(|page|page.read(&expected,Some(&receipt)).is_ok())}
                    else {destination(r,layer,channel,false,coordinate).is_some_and(|surface|surface.read(&expected,Some(&receipt)).is_some())};
                if !current && source.contains(coordinate) {initializes[channel].insert(coordinate);}
            }
        }
        let initialize=initializes[0].clone();
        let sampling=snapshot::sampling_support(transform);
        let reach = (sampling + u32::from(sampling!=0)) as f32;
        let read = pixel_rect(self.plan.cut.outset(reach), r.target_extent(layer));
        let plans = channel_pages.iter().enumerate().map(|(channel, pages)| {
            if pages.is_empty() { return Ok(Vec::new()); }
            let targets = pages.iter().filter_map(|&c| {
                let region = regions.iter().map(|b| b.intersect(page_rect(c))).fold(PixelRect::EMPTY, PixelRect::union);
                let target = Target { texture: (), region: region.page_local(c), initialize: initializes[channel].contains(&c) };
                (!region.is_empty()).then_some((c, target))
            }).collect();
            self.plan_windows(r, channel, transform, mesh.as_ref(), targets, read)
        }).collect::<Result<Vec<_>, _>>()?;
        let channels: Vec<_> = channel_pages.into_iter().zip(plans).map(|(pages, windows)| ChannelPlan { pages, windows }).collect();
        let mut leases = Vec::new();
        for (index, channel) in channels.iter().enumerate() {
            if self.sources[index].is_none() { continue; }
            let read: Vec<_> = channel.windows.iter().flat_map(|window| &window.jobs).flat_map(|(job, _)| job.sources.iter().copied())
                .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            if index == 0 { self.capture_originals(r, encoder, &read)?; }
            leases.extend(self.sources[index].as_ref().unwrap().prepare_pages(r, &read, encoder)?);
        }
        Ok(Some(SourcePlan { index, mesh, support, coordinates, initialize, leases, channels }))
    }
    fn draw_mask_source(
        &mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        layer:SourceTarget,transform:&layer_core::ImageTransform,taps:u32,plan:Option<SourcePlan>,preview:bool,declared:Option<&crate::scene::Capture>,
    )->Result<(),GpuRasterError> {
        let Some(SourcePlan {mesh,channels,initialize,leases:_leases,..})=plan else {return Ok(())};
        if let Some(geometry)=&mesh {self.positions.upload(r,encoder,geometry)?;}
        let receipt=encoder.submission();
        let background=self.background.ok_or(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"))?;
        for (channel,ChannelPlan {pages,windows:planned}) in channels.into_iter().enumerate() {
            if pages.is_empty() {continue;}
            let mut outputs=std::collections::BTreeMap::new();
            for coordinate in pages {
                let intent=r.raster_intent(layer,layer_core::raster::RasterPlane::Mask,coordinate)?;
                let current=r.layer_masks.pages.get(&(layer,coordinate)).filter(|page|page.read(intent.version(),Some(&receipt)).is_ok());
                let (page,version)=if preview {
                    if let Some(previous)=r.layer_masks.previews.get(&(layer,coordinate)).filter(|entry|entry.matches(intent.version())
                        && self.preview.as_ref().zip(self.capture.as_ref()).is_some_and(|(previous,capture)|self.native_version(capture,&previous.drawn(),r.target_extent(layer),RasterPlane::Mask,coordinate,preview_taps(previous)).is_ok_and(|expected|entry.version.matches(&expected)==Some(true)))).cloned() {(previous.page,previous.version)}
                    else {
                        let mut page=layer_masks::MaskPage::new(&r.device);
                        let mut version=intent.version().clone();
                        if let Some(base)=current {
                            let read=base.read(intent.version(),Some(&receipt))?;
                            let write=page.initialize(&receipt)?;
                            encoder.copy_texture_to_texture(read.value().texture.as_image_copy(),write.value().texture.as_image_copy(),read.value().texture.size());
                            write.publish(read.version().clone());
                        } else {
                            let source=r.layer_masks.definitions.get(&layer).ok_or(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"))?;
                            let default=crate::raster::contents::mask_default(source.domain,background,coordinate);
                            if !initialize.contains(&coordinate) && default.matches(intent.version())!=Some(true) {return Err(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"));}
                            version=layer_core::raster::ContentName::fresh().into();
                            page.clear(encoder,f64::from(background),version.clone())?;
                        }
                        (page,version)
                    }
                } else {
                    if let Some(page)=current {
                        let read=page.read(intent.version(),Some(&receipt))?;
                        if self.sources[channel].as_ref().is_some_and(|source|source.aliases(coordinate,&read.value().texture)) {
                            let reusable=self.captures[channel].remove(&coordinate);
                            let capture=self.sources[channel].as_ref().unwrap().pages.get(&coordinate).unwrap().copy_named(r,encoder,reusable)?;
                            self.captures[channel].insert(coordinate,capture.clone());
                            self.sources[channel].as_mut().unwrap().pages.insert(coordinate,capture);
                        }
                    }
                    if let Some(page)=current.cloned() {(page,intent.version().clone())}
                    else {
                        let source=r.layer_masks.definitions.get(&layer).ok_or(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"))?;
                        let default=crate::raster::contents::mask_default(source.domain,background,coordinate);
                        if !initialize.contains(&coordinate) && default.matches(intent.version())!=Some(true) {return Err(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"));}
                        let version:layer_core::raster::ContentVersion=layer_core::raster::ContentName::fresh().into();
                        let mut page=layer_masks::MaskPage::new(&r.device);page.clear(encoder,f64::from(background),version.clone())?;(page,version)
                    }
                };
                page.read(&version,Some(&receipt))?;
                outputs.insert(coordinate,(page,version,intent));
            }
            let windows=planned.into_iter().map(|window|Ok(Window {
                origin:window.origin,jobs:window.jobs,
                pages:window.pages.into_iter().map(|(coordinate,target)| {
                    let (page,version,_)=outputs.get(&coordinate).ok_or(GpuRasterError::Invariant("planned transform output is missing"))?;
                    let read=page.read(version,Some(&receipt))?;
                    Ok((coordinate,Target {texture:read.value().texture.clone(),region:target.region,initialize:target.initialize}))
                }).collect::<Result<Vec<_>,GpuRasterError>>()?,
            })).collect::<Result<Vec<_>,GpuRasterError>>()?;
            let names=outputs.keys().copied().map(|coordinate| {
                let name=if preview {self.native_version(declared.ok_or(GpuRasterError::Invariant("prepared draw mask source state is inconsistent"))?,transform,r.target_extent(layer),RasterPlane::Mask,coordinate,taps)?}
                    else {layer_core::raster::ContentName::fresh().into()};
                Ok((coordinate,name))
            }).collect::<Result<std::collections::BTreeMap<_,_>,GpuRasterError>>()?;
            let mut copied=|coordinate,actual:&crate::submission::Submission| {
                let (page,version,_)=outputs.get_mut(&coordinate).ok_or(GpuRasterError::Invariant("planned transform output is missing"))?;
                let name=names[&coordinate].clone();
                let written=page.revise(version,actual)?.publish(name);
                *version=written.version().clone();Ok(())
            };
            let result=self.draw_windows(r,encoder,channel,transform,taps,&windows,self.has_selection,Some(&mut copied));
            let receipt=encoder.submission();
            for (coordinate,(page,version,intent)) in outputs {
                if result.is_ok() {
                    if preview {r.layer_masks.previews.insert((layer,coordinate),layer_masks::MaskPreview {page,base:intent.version().clone(),version:version.clone()});}
                    else {
                        let read=page.read(&version,Some(&receipt))?;
                        r.record_raster_write(layer,read.replace(&intent,&receipt))?;
                        r.layer_masks.pages.insert((layer,coordinate),page);
                        r.layer_masks.previews.remove(&(layer,coordinate));
                    }
                } else if preview {r.layer_masks.previews.insert((layer,coordinate),layer_masks::MaskPreview {page,base:intent.version().clone(),version});}
                else {r.layer_masks.pages.insert((layer,coordinate),page);}
            }
            result?;
        }
        Ok(())
    }
    fn native_version(&self,capture:&crate::scene::Capture,transform:&layer_core::ImageTransform,extent:[u32;2],plane:RasterPlane,coordinate:[u32;2],taps:u32)->Result<layer_core::raster::ContentVersion,GpuRasterError> {
        let [cut,destination]=self.plan.channel_regions(transform,extent)[usize::from(plane==RasterPlane::WatercolorWetness)];
        if !destination.page_local(coordinate).is_empty() {return capture.version(plane,page_rect(coordinate).into(),taps);}
        Ok(if cut.page_local(coordinate).is_empty() {capture.original(plane,coordinate)}else {capture.cut(plane,coordinate)})
    }
    fn draw_source(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        layer: SourceTarget,
        transform: &layer_core::ImageTransform,
        taps: u32,
        plan: Option<SourcePlan>,
        scope:TransformWrite,
        declared:Option<&crate::scene::Capture>,
    ) -> Result<(), GpuRasterError> {
        if matches!(scope,TransformWrite::Authored) && let Some(plan)=&plan {
            for (channel,ChannelPlan {pages,..}) in plan.channels.iter().enumerate() {
                for &c in pages {r.accepted_raster_output(layer,match (self.background,channel) {
                    (Some(default),_)=>RasterOutput::Mask(c,default.to_bits()),(None,0)=>RasterOutput::Color(c),_=>RasterOutput::Wetness(c),
                })?;}
            }
        }
        if self.background.is_some() {
            let capture=declared.cloned().or_else(||self.capture.clone());
            return self.draw_mask_source(r,encoder,layer,transform,taps,plan,matches!(scope,TransformWrite::Preview),capture.as_ref());
        }
        let Some(SourcePlan {index,mesh,support,coordinates,initialize,channels,leases:_leases})=plan else {return Ok(())};
        let index=index.ok_or(GpuRasterError::MissingPaintLayer(layer))?;
        if let Some(geometry)=&mesh {self.positions.upload(r,encoder,geometry)?;}
        let receipt=encoder.submission();
        let mut cleared:[std::collections::BTreeMap<[u32;2],layer_core::raster::ContentVersion>;2]=Default::default();
        let full:[std::collections::BTreeSet<[u32;2]>;2]=std::array::from_fn(|channel|channels[channel].windows.iter().flat_map(|window|&window.pages)
            .filter(|(_,target)|target.initialize).map(|(coordinate,_)|*coordinate).collect());
        for coordinate in coordinates {
            if !r.paint_layers[index].pages.iter().any(|page|page.coordinate==coordinate) {
                let mut page=self.spares.paint.pop().unwrap_or_else(||r.create_page(coordinate,"transformed paint page"));
                page.coordinate=coordinate;page.active_secondary=false;page.derived=None;page.stroke_start=None;
                if !initialize.contains(&coordinate) {
                    let version:layer_core::raster::ContentVersion=layer_core::raster::ContentName::fresh().into();
                    let write=page.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                    write.value().clear(&r.zero_pages,encoder);write.publish(&version);cleared[0].insert(coordinate,version);
                }
                page.primary_needs_clear=false;r.paint_layers[index].pages.push(page);
            }
            if self.sources[1].is_some() && support[1].iter().any(|bounds|!bounds.page_local(coordinate).is_empty())
                && !r.paint_layers[index].watercolor_wetness_pages.iter().any(|page|page.coordinate==coordinate) {
                let mut page=self.spares.watercolor.pop().unwrap_or_else(||WatercolorWetnessPage {coordinate,
                    primary:resident::Resident::unwritten(r.create_scalar_page_surface(&r.raster_tiers.paint,"transformed watercolor wetness A")),
                    secondary:resident::Resident::unwritten(r.create_scalar_page_surface(&r.raster_tiers.paint,"transformed watercolor wetness B")),
                    active_secondary:false,primary_needs_clear:false,secondary_needs_clear:true,pending:None,derived:None});
                page.coordinate=coordinate;page.active_secondary=false;page.pending=None;page.derived=None;
                if !full[1].contains(&coordinate) {
                    let version:layer_core::raster::ContentVersion=layer_core::raster::ContentName::fresh().into();
                    let write=page.primary.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                    write.value().clear(&r.zero_pages,encoder);write.publish(&version);cleared[1].insert(coordinate,version);
                }
                page.primary_needs_clear=false;r.paint_layers[index].watercolor_wetness_pages.push(page);
            }
        }
        for (channel,ChannelPlan {pages,windows:planned}) in channels.into_iter().enumerate() {
            if pages.is_empty() {continue;}
            let plane=if channel==0 {RasterPlane::Color}else {RasterPlane::WatercolorWetness};
            let full=planned.iter().flat_map(|window|&window.pages).filter(|(_,target)|target.initialize).map(|(coordinate,_)|*coordinate).collect::<std::collections::BTreeSet<_>>();
            let mut outputs=std::collections::BTreeMap::new();
            for coordinate in pages {
                let input=r.raster_intent(layer,plane,coordinate)?;
                let existing=if let Some(version)=cleared[channel].get(&coordinate) {version.clone()}
                    else if matches!(scope,TransformWrite::Authored) {input.version().clone()}
                    else if let Some(capture)=self.capture.as_ref().filter(|_|self.native_preview || self.queried.contains(&coordinate)) {
                        let quality=match scope {TransformWrite::Restore(taps)=>taps,_=>self.preview.as_ref().map_or(taps,preview_taps)};
                        let previous=self.preview.as_ref().map(|preview|preview.drawn()).unwrap_or_else(||std::borrow::Cow::Owned(Default::default()));
                        self.native_version(capture,&previous,r.target_extent(layer),plane,coordinate,quality)?
                    } else {input.version().clone()};
                let name=match scope {
                    TransformWrite::Authored=>layer_core::raster::ContentName::fresh().into(),
                    TransformWrite::Restore(_)=>self.capture.as_ref().map_or_else(||input.version().clone(),|capture|capture.original(plane,coordinate)),
                    TransformWrite::Preview=>{
                        let capture=declared.or(self.capture.as_ref()).ok_or(GpuRasterError::Invariant("prepared draw source state is inconsistent"))?;
                        self.native_version(capture,transform,r.target_extent(layer),plane,coordinate,taps)?
                    },
                };
                if matches!(scope,TransformWrite::Restore(_)) && name.matches(input.version())!=Some(true) {return Err(GpuRasterError::Invariant("prepared draw source state is inconsistent"));}
                let surface=if channel==0 {r.paint_layers[index].pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap().active_mut()}
                    else {let page=r.paint_layers[index].watercolor_wetness_pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap();page.surface_mut(page.active_secondary)};
                let write=if full.contains(&coordinate) {surface.overwrite(&receipt)}else {surface.revise(&existing,&receipt)}.ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
                let texture=write.value().texture.clone();let encoding=write.encoded(name);
                self.capture_destination(r,encoder,channel,coordinate,&texture)?;
                outputs.insert(coordinate,TransformOutput {texture,input,encoding:Some(encoding),written:None});
            }
            let windows=planned.into_iter().map(|window|Window {origin:window.origin,jobs:window.jobs,
                pages:window.pages.into_iter().map(|(coordinate,target)|(coordinate,Target {texture:outputs[&coordinate].texture.clone(),region:target.region,initialize:target.initialize})).collect()}).collect::<Vec<_>>();
            let mut copied=|coordinate,actual:&submission::Submission| {
                let output=outputs.get_mut(&coordinate).ok_or(GpuRasterError::Invariant("planned transform output is missing"))?;
                output.written=Some(output.encoding.take().ok_or(GpuRasterError::Invariant("transform encoding already consumed"))?.publish(actual));Ok(())
            };
            self.draw_windows(r,encoder,channel,transform,taps,&windows,self.has_selection,Some(&mut copied))?;
            for (coordinate,output) in outputs {
                let written=output.written.ok_or(GpuRasterError::Invariant("transform output was not encoded"))?;
                if matches!(scope,TransformWrite::Authored) {
                    let surface=destination(r,layer,channel,false,coordinate).ok_or(GpuRasterError::Invariant("prepared draw source state is inconsistent"))?;
                    r.record_raster_write(layer,surface.read(written.version(),Some(written.submission())).ok_or(GpuRasterError::Invariant("accepted input is not readable"))?.replace(&output.input,written.submission()))?;
                }
                if channel==0 {r.paint_layers[index].pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap().derived=None;}
                else {let page=r.paint_layers[index].watercolor_wetness_pages.iter_mut().find(|page|page.coordinate==coordinate).unwrap();page.derived=None;page.pending=None;}
            }
        }
        // The next stroke establishes fresh stroke-scoped accumulation. Keep
        // persistent wetness and the layer-level watercolor edge style intact.
        {
            for p in &mut r.paint_layers[index].coverage_pages {
                p.owner = None;
            }
        }
        Ok(())
    }

    /// Group target pages into atlas windows and split each window's 2x2
    /// page blocks into jobs that fit the source bindings. Pages marked for
    /// initialization first receive their whole unmoved page.
    fn plan_windows<T>(
        &self,
        r: &WgpuRasterizer,
        channel: usize,
        transform: &layer_core::ImageTransform,
        mesh: Option<&Arc<mesh::MeshGeometry>>,
        targets: Vec<([u32; 2], Target<T>)>,
        read: PixelRect,
    ) -> Result<Vec<Window<T>>, GpuRasterError> {
        let [columns, rows] = if mesh.is_some() {
            MESH_WINDOW
        } else {
            Atlas::pages(self.atlas_format(r, channel))
        };
        let mut windows = std::collections::BTreeMap::new();
        for (c, target) in targets {
            let key = [c[0] / columns, c[1] / rows];
            windows
                .entry(key)
                .or_insert_with(|| Window {
                    origin: [key[0] * columns, key[1] * rows],
                    jobs: Vec::new(),
                    pages: Vec::new(),
                })
                .pages
                .push((c, target));
        }
        let splitter = self.sources[channel]
            .as_ref()
            .unwrap()
            .splitter(transform, mesh.cloned(), read)?;
        let mut found = Vec::new();
        for window in windows.values_mut() {
            let mut blocks = std::collections::BTreeMap::new();
            for (c, target) in &window.pages {
                let region = on_page(target.region, *c);
                blocks
                    .entry([c[0] / 2, c[1] / 2])
                    .and_modify(|b: &mut PixelRect| *b = b.union(region))
                    .or_insert(region);
            }
            window.jobs.extend(window.pages.iter().filter(|(_, t)| t.initialize).map(|(c, _)| {
                let job = snapshot::Footprint {
                    region: page_rect(*c),
                    sources: vec![*c],
                };
                (job, true)
            }));
            for start in blocks.into_values() {
                splitter.split(start, &mut found)?;
            }
            window.jobs.extend(found.drain(..).map(|job| (job, false)));
        }
        Ok(windows.into_values().collect())
    }

    fn atlas_format(&self, r: &WgpuRasterizer, channel: usize) -> wgpu::TextureFormat {
        if self.background.is_some() || channel != 0 {
            r.device.scalar_format()
        } else {
            r.device.working_format()
        }
    }

    /// Draw each window's jobs into the atlas, in as few passes as the source
    /// cache allows, then copy each page's region to its target.
    #[allow(clippy::too_many_arguments)]
    fn draw_windows(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        channel: usize,
        transform: &layer_core::ImageTransform,
        taps: u32,
        windows: &[Window],
        selected: bool,
        mut copied:Option<&mut dyn FnMut([u32;2],&crate::submission::Submission)->Result<(),GpuRasterError>>,
    ) -> Result<(), GpuRasterError> {
        let format = self.atlas_format(r, channel);
        let scalar = format != r.device.working_format() || channel != 0;
        let atlas = self.atlases[usize::from(scalar)]
            .get_or_insert_with(|| Atlas::new(&r.device, format));
        let atlas = (atlas.texture.clone(), atlas.view.clone());
        let snapshot = self.sources[channel].as_ref().unwrap();
        let bounds = snapshot.source_bounds();
        let records: Vec<_> = windows
            .iter()
            .flat_map(|window| {
                window.jobs.iter().map(|(job, unmoved)| pixel_transform::TiledTransformRecord {
                    source_size: [PAGE_SIZE; 2],
                    origin: window.origin.map(|v| i64::from(v * PAGE_SIZE)),
                    sources: &job.sources,
                    texels: [0; 4],
                    unmoved: *unmoved,
                    clear: false,
                })
            })
            .collect();
        let meshed = transform.placement.mesh.is_some();
        let positions = meshed
            .then(|| self.positions.view(&r.device, MESH_WINDOW.map(|n| n * PAGE_SIZE)));
        let pass = if self.background.is_some() {
            &mut self.visibility
        } else if channel == 0 {
            &mut self.color
        } else {
            &mut self.scalar
        };
        let offset = pass
            .prepare_tiled(
                &r.device,
                &mut r.uploads,
                encoder,
                bounds,
                self.background.unwrap_or(0.),
                transform,
                taps,
                &records,
                None,
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        let selection = selected.then_some(self.selection.as_ref()).flatten();
        let mut first = 0;
        for window in windows {
            let origin = window.origin.map(|v| v * PAGE_SIZE);
            if meshed {
                self.positions.draw(r, encoder, window.origin)?;
            }
            let snapshot = self.sources[channel].as_ref().unwrap();
            let receipt=encoder.submission();
            let batches = source_batches(&window.jobs, |c| snapshot.pages.get(&c).is_some_and(|page|page.readable(&receipt)));
            for (n, batch) in batches.into_iter().enumerate() {
                let mut sources = Vec::with_capacity(batch.len());
                for (job, _) in &window.jobs[batch.clone()] {
                    let snapshot = self.sources[channel].as_ref().unwrap();
                    sources.push(snapshot.binding(
                        r,
                        pass,
                        &job.sources,
                        selection,
                        positions.as_ref(),
                        encoder,
                    )?);
                }
                let draws: Vec<_> = batch
                    .zip(&sources)
                    .map(|(index, source)| {
                        let (job, _) = &window.jobs[index];
                        pixel_transform::BatchDraw {
                            source,
                            job: first + index,
                            scissor: [
                                job.region.min_x() - origin[0],
                                job.region.min_y() - origin[1],
                                job.region.width(),
                                job.region.height(),
                            ],
                        }
                    })
                    .collect();
                pass.encode_batch(encoder, &atlas.1, n == 0, offset, &draws);
            }
            first += window.jobs.len();
            for (c, target) in &window.pages {
                let region = if target.initialize {
                    PixelRect::full([PAGE_SIZE; 2])
                } else {
                    target.region
                };
                let slot = [0, 1].map(|axis| (c[axis] - window.origin[axis]) * PAGE_SIZE);
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &atlas.0,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: slot[0] + region.min_x(),
                            y: slot[1] + region.min_y(),
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &target.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: region.min_x(),
                            y: region.min_y(),
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width: region.width(),
                        height: region.height(),
                        depth_or_array_layers: 1,
                    },
                );
                if let Some(copied)=copied.as_mut() {copied(*c,&encoder.submission())?;}
            }
        }
        Ok(())
    }

    /// Copy the original photo or backing tiles the jobs sample into pages
    /// owned by the transaction when the free decoded slots cannot hold them
    /// at once. Later frames bind those pages instead of cycling a large photo
    /// through the bounded source cache, and batches waiting for decodes resume.
    fn capture_originals(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        read: &[[u32; 2]],
    ) -> Result<(), GpuRasterError> {
        let snapshot = self.sources[0].as_ref().unwrap();
        let receipt=encoder.submission();
        let uncaptured = |c: &[u32; 2]| snapshot.contains(*c) && !snapshot.pages.get(c).is_some_and(|page|page.readable(&receipt));
        if read.iter().filter(|c| uncaptured(c)).count() < r.source_tiles.borrow().free_slots() { return Ok(()); }
        let missing: Vec<_> = read.iter().copied().filter(uncaptured).collect();
        let format = self.atlas_format(r,0);
        let identity = layer_core::ImageTransform::default();
        let mut waiting = false;
        for batch in missing.chunks(SOURCE_SLOTS) {
            let targets: Vec<_> = batch.iter().map(|&coordinate| {
                let capture = self.captures[0]
                    .entry(coordinate)
                    .or_insert_with(||snapshot_page(&r.device,format));
                if capture.texture.format() != format {
                    *capture=snapshot_page(&r.device,format);
                }
                (coordinate, Target { texture: capture.texture.clone(), region: PixelRect::full([PAGE_SIZE; 2]), initialize: false })
            }).collect();
            let snapshot = self.sources[0].as_ref().unwrap();
            let copies = self.plan_windows(r, 0, &identity, None, targets, snapshot.bounds)?;
            let sources: Vec<_> = copies.iter().flat_map(|window| &window.jobs).flat_map(|(job, _)| job.sources.iter().copied())
                .collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            let _leases = match snapshot.prepare_pages(r, &sources, encoder) {
                Err(GpuRasterError::NotReady) => {
                    waiting = true;
                    if r.decode_queue_has_room() { continue; }
                    break;
                }
                leases => leases?,
            };
            {
                let mut captures=std::mem::take(&mut self.captures[0]);
                let mut copied=|coordinate,receipt:&crate::submission::Submission|captures.get_mut(&coordinate).ok_or(GpuRasterError::Invariant("planned transform capture is missing"))?.written(receipt);
                let result=self.draw_windows(r,encoder,0,&identity,pixel_transform::PREVIEW_TAPS,&copies,false,Some(&mut copied));
                self.captures[0]=captures;result?;
            }
            let snapshot = self.sources[0].as_mut().unwrap();
            for (coordinate, _) in copies.iter().flat_map(|window| &window.pages) {
                let capture = &self.captures[0][coordinate];
                snapshot.pages.insert(*coordinate, capture.clone());
            }
        }
        if waiting { Err(GpuRasterError::NotReady) } else { Ok(()) }
    }

    fn capture_destination(&mut self,r:&WgpuRasterizer,encoder:&mut submission::CommandEncoder,channel:usize,coordinate:[u32;2],texture:&wgpu::Texture)->Result<(),GpuRasterError> {
        let source=self.sources[channel].as_mut().unwrap();
        if !source.aliases(coordinate,texture) {return Ok(());}
        let reusable=self.captures[channel].remove(&coordinate);
        let capture=source.pages.get(&coordinate).unwrap().copy_named(r,encoder,reusable)?;
        self.captures[channel].insert(coordinate,capture.clone());source.pages.insert(coordinate,capture);Ok(())
    }
    fn mesh_geometry(&mut self, map: &layer_core::LayerPlacement, display: Option<f32>) -> Arc<mesh::MeshGeometry> {
        if let Some((cached, within, geometry)) = &self.mesh
            && *within == display
            && cached == map
        {
            return geometry.clone();
        }
        let geometry = Arc::new(mesh::MeshGeometry::new(map.mesh.as_ref().unwrap(), map.outer, display));
        self.mesh = Some((map.clone(), display, geometry.clone()));
        geometry
    }
    fn release_snapshot(&mut self) {
        self.capture=None;
        self.prepared_preview=None;self.preparing=None;
        self.sources = Default::default();
        self.atlases = Default::default();
        self.reduced = None;
        self.fine_progress.clear();
        self.standby = None;
        // Active copies follow the overwritten paint footprint. After the
        // transaction, retain only a small reusable pool, never every area
        // visited by unrelated transforms. Originals themselves stay tiled.
        for pages in &mut self.captures {
            while pages.len() > 64 {
                pages.pop_last();
            }
        }
        self.retain_pool_bindings();
    }
    fn retain_pool_bindings(&mut self) {
        let color: Vec<_> = self.captures[0].values().map(|p| &p.view).collect();
        let scalar: Vec<_> = self.captures[1..]
            .iter()
            .flat_map(|p| p.values().map(|p| &p.view))
            .collect();
        self.color
            .retain_source_bindings(&color, self.selection.as_ref());
        self.visibility
            .retain_source_bindings(&color, self.selection.as_ref());
        self.scalar
            .retain_source_bindings(&scalar, self.selection.as_ref());
    }

    pub fn discard_preview(&mut self) {
        self.fine_progress.clear();
        self.preview = None;
        self.native_preview = false;
        self.queried.clear();
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.spares.clear();
        self.release_snapshot();
    }
    pub fn has_preview(&self) -> bool {
        self.preview.is_some()
    }
    /// An identical committed operation can retain the already-rendered result.
    /// Any intervening paint or parameter change takes the normal replay path.
    pub fn matching_commit(&self, packet: FramePacket<'_>) -> Option<(SourceTarget, u32)> {
        if !self.native_preview { return None; }
        let preview = self.preview.as_ref()?;
        if preview.target.is_coverage() {return None;}
        if packet
            .dab_batches
            .iter()
            .any(|b| !matches!(b.kind, DabBatchKind::RasterOperation(_)))
        {
            return None;
        }
        let batch = packet
            .dab_batches
            .iter()
            .find(|b| b.target == preview.target)?;
        let DabBatchKind::RasterOperation(index) = batch.kind else {
            return None;
        };
        if batch.target != preview.target || !packet.dabs.is_empty() {
            return None;
        }
        let operation = packet.scene.operations(preview.target)?.get(index as usize)?;
        if !matches!(&operation.kind, layer_core::RasterOperationKind::Transform(t) if t == preview.drawn().as_ref())
            || operation.coverage.selection != preview.selection
            || preview_taps(preview) != pixel_transform::exact_taps(&preview.drawn())
        {
            return None;
        }
        Some((preview.target, index))
    }
    /// Restore before persistent edits; the same identity shader copies exact
    /// pigment/wetness, including fractional selection edges.
    pub fn cancel_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<Option<(SourceTarget, PixelRect)>, GpuRasterError> {
        let Some(previous) = self.preview.clone() else {
            return Ok(None);
        };
        let regions = self.preview_regions;
        // Removing the target already discarded its pixels. There is nothing
        // to restore, and cancellation must not turn deletion into a GPU error.
        if previous.target.is_coverage() {r.layer_masks.previews.retain(|(target,_),_|*target!=previous.target);}
        else if let Some((target, restored)) = self.cancel_regions(r) {
            self.render_source(r, encoder, target, &Default::default(), pixel_transform::PREVIEW_TAPS, &restored,TransformWrite::Restore(preview_taps(&previous)))?;
        }
        self.retain_pages(r, previous.target, &[], None);
        self.preview=None;
        self.preview_regions = [PixelRect::EMPTY; 2];
        self.native_preview = false;
        self.queried.clear();
        self.spares.clear();
        self.release_snapshot();
        Ok(Some((previous.target, regions[0].union(regions[1]))))
    }
    fn cancel_regions(&self, r: &WgpuRasterizer) -> Option<(SourceTarget, Vec<PixelRect>)> {
        let target = self.preview.as_ref()?.target;
        (!target.is_coverage() && (self.native_preview || !self.queried.is_empty())
            && (r.paint_layers.iter().any(|l| l.id == target) || r.layer_masks.definitions.contains_key(&target)))
            .then(|| (target, if self.native_preview { self.preview_regions.to_vec() } else { self.queried.iter().copied().map(page_rect).collect() }))
    }
    fn cancel_restores(&self, r: &WgpuRasterizer) -> Vec<(SourceTarget, TileKey)> {
        let Some((target, regions)) = self.cancel_regions(r).filter(|_| self.sources[0].is_some()) else { return Vec::new(); };
        let restored: std::collections::BTreeSet<_> = regions.into_iter().flat_map(page_coordinates).collect();
        let planes = [if self.background.is_some() { RasterPlane::Mask } else { RasterPlane::Color }, RasterPlane::WatercolorWetness];
        (0..2).filter(|channel| self.sources[*channel].is_some())
            .flat_map(|channel| self.original_pages[channel].iter().filter(|c| restored.contains(*c)).map(move |c| (target, TileKey { plane: planes[channel], coordinate: *c })))
            .collect()
    }
    pub fn prepare_cancel(&mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        let Some((target, restored)) = self.cancel_regions(r) else { return Ok(()); };
        self.plan_source(r, encoder, target, &Default::default(), &restored).map(drop)
    }
    fn plan_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        layer: Option<(SceneView<'_>, SourceTarget)>, extent: [u32; 2], native: bool,
    ) -> Result<PreviewUpdate, GpuRasterError> {
        if let Some(candidate)=self.preparing.take().filter(|candidate|candidate.prepared_preview.as_ref().is_some_and(|(prepared,owner,revision)|layer.is_some_and(|(scene,_)|scene.owner()==*owner && scene.revision()==*revision)
            && prepared.transaction==next.transaction && prepared.target==next.target && prepared.selection==next.selection)) {
            let mut damage=self.cancel_preview(r,encoder)?.into_iter().collect::<Vec<_>>();
            *self = *candidate;
            let mut update=self.plan_preview(r,encoder,next,layer,extent,native)?;
            damage.append(&mut update.damage);
            update.damage=damage;
            return Ok(update);
        }
        let same_source = ( self.holds_source(next, layer.map(|(scene, _)| scene))
            || self.prepared_preview.as_ref().is_some_and(|(p,owner,revision)|layer.is_some_and(|(scene,_)|scene.owner()==*owner && scene.revision()==*revision)
            && p.transaction==next.transaction && p.target==next.target && p.selection==next.selection)) && self.capture.as_ref().is_none_or(|capture|layer.is_some_and(|(scene,_)|
                capture.matches_context(scene) && r.raster_versions(scene,next.target).is_ok_and(|current|
                    [RasterPlane::Color,RasterPlane::Mask,RasterPlane::WatercolorWetness].into_iter().all(|plane|capture.matches_source(plane,&current)==Some(true)))));
        if same_source && self.preview.as_ref()==Some(next) && self.native_preview==native {
            return Ok(PreviewUpdate {damage:Vec::new(),draw:None});
        }
        self.standby = None;
        let mut damage = Vec::with_capacity(2);
        if !same_source {
            damage.extend(self.cancel_preview(r, encoder)?);
            self.capture_source(r, encoder, next.target, None, next.selection.as_ref(), (extent,None), None)?;
            self.prepared_preview = layer.map(|(scene, _)| (next.clone(), scene.owner(), scene.revision()));
        }
        if self.capture.is_none() {
            let scene=layer.map(|(scene,_)|scene).ok_or(GpuRasterError::Invariant("prepared plan preview state is inconsistent"))?;
            self.capture=Some(crate::scene::Capture::new(scene.snapshot(),r.current_raster_versions(next.target)?,next.clone(),None));
        }
        let regions = next
            .transform
            .affected_regions(self.plan.cut)
            .map(|b| pixel_rect(b, extent));
        let cut = if !same_source || self.native_preview != native || self.preview_regions[0] != regions[0] {
            self.preview_regions[0].union(regions[0])
        } else { PixelRect::EMPTY };
        let affected = [cut, self.preview_regions[1], regions[1]];
        let source = if native {
            self.plan_source(r, encoder, next.target, &next.drawn(), &affected)?
        } else if self.native_preview || !self.queried.is_empty() {
            let restored: Vec<_> = if self.native_preview { self.preview_regions.to_vec() } else { self.queried.iter().copied().map(page_rect).collect() };
            self.plan_source(r, encoder, next.target, &Default::default(), &restored)?
        } else { None };
        Ok(PreviewUpdate { damage, draw: Some(PreviewDraw { regions, affected, source }) })
    }
    fn draw_preview(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview,
        native: bool,
        update: PreviewUpdate,
    ) -> Result<Vec<(SourceTarget, PixelRect)>, GpuRasterError> {
        let PreviewUpdate { mut damage, draw } = update;
        let Some(PreviewDraw { regions, affected, source }) = draw else { return Ok(damage) };
        if native {
            let capture=self.capture.as_ref().ok_or(GpuRasterError::Invariant("prepared draw preview state is inconsistent"))?.with_preview(next.clone(),source.as_ref().and_then(|plan|plan.mesh.clone())).with_taps(preview_taps(next));
            if self.background.is_some() {self.draw_mask_source(r,encoder,next.target,&next.drawn(),preview_taps(next),source,true,Some(&capture))?;}
            else {self.draw_source(r,encoder,next.target,&next.drawn(),preview_taps(next),source,TransformWrite::Preview,Some(&capture))?;}
            self.capture=Some(capture);
            self.retain_pages(r,next.target,&regions,Some(&next.transform));
        } else if self.native_preview || !self.queried.is_empty() {
            if self.background.is_some() {r.layer_masks.previews.retain(|(target,_),_|*target!=next.target);}
            else {self.draw_source(r, encoder, next.target, &Default::default(), pixel_transform::PREVIEW_TAPS, source,TransformWrite::Restore(self.preview.as_ref().map_or(pixel_transform::PREVIEW_TAPS,preview_taps)),None)?;}
            self.retain_pages(r, next.target, &[], None);
        }
        if !native {
            if let Some(capture)=&self.capture {self.capture=Some(capture.with_preview(next.clone(),None).with_taps(preview_taps(next)));}
            self.retire_native_spares(r,encoder);
        }
        self.native_preview = native;
        self.queried.clear();
        damage.push((
            next.target,
            affected
                .into_iter()
                .fold(PixelRect::EMPTY, PixelRect::union),
        ));
        if self.preview.as_ref()!=Some(next) {self.fine_progress.clear();}
        self.preview = Some(next.clone());
        self.preview_regions = regions;
        Ok(damage)
    }
    fn native_retirement_pending(&self)->bool {!self.retired_native.is_empty()}
    fn retire_native_spares(&mut self,r:&WgpuRasterizer,encoder:&crate::submission::CommandEncoder) {
        let bytes=self.spares.storage_bytes();
        if bytes==0 {return;}
        let pages=std::mem::take(&mut self.spares);
        let lease=Arc::new(region_batches::Allocation {bytes,retired:r.retired_display_bytes.clone(),pending:Default::default()});
        lease.retire();
        let completion=lease.clone();encoder.hold(completion);
        self.retired_native.push((pages,lease));
    }
    /// Whether this state already holds `next`'s originals: those of its
    /// active transaction, or the moving pixels prepared for its selection.
    fn holds_source(&self, next: &layer_render::TransformPreview, scene: Option<SceneView<'_>>) -> bool {
        match &self.preview {
            Some(p) => p.transaction == next.transaction && p.target == next.target && p.selection == next.selection,
            None => scene.zip(next.selection.as_ref())
                .is_some_and(|(scene, selection)| self.standby.as_ref() == Some(&Standby::of(scene, next.target, selection))),
        }
    }
    /// Whether this transaction's captured originals can draw `next` into a
    /// display level.
    fn displayable(&self, next: &layer_render::TransformPreview) -> bool {
        self.preview.as_ref().is_some_and(|p| {
            p.transaction == next.transaction
                && p.target == next.target
                && p.selection == next.selection
        }) && self.reducible()
    }
    fn reducible(&self) -> bool { self.background.is_none() && self.sources[0].is_some() }
    #[expect(clippy::too_many_arguments, reason = "Transform previews keep source level, preview geometry, and output sampling regions explicit")]
    fn prepare_region(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        next: &layer_render::TransformPreview, extent: [u32; 2], placement: layer_core::Affine,
        level: &wgpu::TextureView, display: pixel_transform::DisplayLevel, target: display_mips::Plan, region: DocRect,
        fine_budget:u64,
    ) -> Result<PreparedRegion, GpuRasterError> {
        if self.reduced.as_ref().is_some_and(|input|input.fine.is_some()) {
            if self.native_retirement_pending() {return Err(GpuRasterError::NotReady);}
            self.prepare_fine_reduced(r,encoder,next.selection.as_ref(),extent)?;
            let side=display.side;
            let texels=texel_rect(target.doc_bounds.local(region),side);
            let values=self.display_record(next,extent,placement,display,target,texels)?;
            let capacity=self.reduced.as_ref().unwrap().fine.as_ref().unwrap().capacity;
            let geometry=if next.transform.placement.mesh.is_some() {
                let tolerance=0.5*side as f32/placement.magnification().max(1e-6);
                Some(self.mesh_geometry(&next.transform.placement,Some(tolerance)))
            } else {None};
            let index=if let Some(index)=self.fine_progress.iter().position(|p|p.preview==*next && p.destination==*level
                && p.target==target && p.region==region && p.values==values && p.valid.load(std::sync::atomic::Ordering::Acquire)) {index}
            else {
                let bytes=texture_bytes(level.texture());
                if bytes>fine_budget {return Err(GpuRasterError::SourceWorkingSetExceeded);}
                let bounds=PixelRect::new(texels[0],texels[1],texels[0]+texels[2],texels[1]+texels[3]);
                let phase=self.reduced.as_ref().unwrap().phase;
                let keeps=self.reduced.as_ref().unwrap().kept.is_some();
                let triangles=next.transform.placement.mesh.as_ref().map(|_|self.mesh.as_ref().unwrap().2.sampling_triangles().collect());
                let demand=fine_demand::Demand::new(values,keeps,phase,triangles);
                let texture=r.device.create_texture(&wgpu::TextureDescriptor {label:Some("pending transform region"),
                    size:level.texture().size(),mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,
                    format:level.texture().format(),usage:level.texture().usage()|wgpu::TextureUsages::COPY_SRC|wgpu::TextureUsages::COPY_DST,
                    view_formats:&[]});
                let output=texture.create_view(&Default::default());
                let allocation=Arc::new(region_batches::Allocation {bytes,retired:r.retired_display_bytes.clone(),pending:Default::default()});
                self.fine_progress.push(FineProgress {preview:next.clone(),destination:level.clone(),output,target,region,allocation,values,demand,
                    regions:region_batches::Regions::new(bounds),valid:Arc::new(std::sync::atomic::AtomicBool::new(true))});
                self.fine_progress.len()-1
            };
            let output=self.fine_progress[index].output.clone();
            let pass=r.scene_pipelines.fine_resample.clone();
            loop {
                let progress=&mut self.fine_progress[index];
                let batch=progress.regions.prepare(capacity,&mut self.fine_steps,|output|progress.demand.pages(output,capacity))?.cloned();
                let Some(batch)=batch else {break;};
                let texels=[batch.output.min_x(),batch.output.min_y(),batch.output.width(),batch.output.height()];
                self.reduced.as_mut().unwrap().fine.as_mut().unwrap().require(batch.pages.clone())?;
                let required=batch.pages.iter().copied().collect::<Vec<_>>();
                self.fill_fine_pages(r,encoder,&required,extent)?;
                if let Some(geometry)=&geometry {self.display_mesh.upload(r,encoder,geometry)?;}
                self.reduced.as_mut().unwrap().draw(r,&pass,encoder,&output,&values,texels,
                    next.transform.placement.mesh.as_ref().map(|_|&self.display_mesh))?;
                let progress=&mut self.fine_progress[index];
                crate::submission::CacheWrite::shared(progress.valid.clone()).track(encoder);
                let lease=progress.allocation.clone();encoder.hold(lease);
                progress.regions.advance();
            }
            let origin=wgpu::Origin3d {x:texels[0],y:texels[1],z:0};
            let progress=&self.fine_progress[index];
            return Ok(PreparedRegion(Region::Copy {source:output,destination:level.clone(),origin,
                size:wgpu::Extent3d {width:texels[2],height:texels[3],depth_or_array_layers:1},
                allocation:progress.allocation.clone(),valid:progress.valid.clone()}));
        }
        let side = display.side;
        let texels = texel_rect(target.doc_bounds.local(region), side);
        let values = self.display_record(next, extent, placement, display, target, texels)?;
        let mesh=if next.transform.placement.mesh.is_some() {
            let tolerance = 0.5 * side as f32 / placement.magnification().max(1e-6);
            Some(self.mesh_geometry(&next.transform.placement, Some(tolerance)))
        } else {None};
        let input=self.reduced.as_ref().unwrap();
        Ok(PreparedRegion(Region::Draw {source:next.target,values,sampling:input.sampling.clone(),destination:level.clone(),texels,mesh,kept:input.kept.is_some()}))
    }
    fn display_record(&self, next: &layer_render::TransformPreview, extent: [u32; 2], placement: layer_core::Affine,
        display: pixel_transform::DisplayLevel, target: display_mips::Plan, texels: [u32; 4],
    ) -> Result<[u8; resample::UNIFORM_BYTES as usize], GpuRasterError> {
        let inputs = self.reduced.as_ref().unwrap();
        let display_level = display.side.trailing_zeros();
        let moved = resample_map(&next.transform, placement, inputs, display_level)?;
        let kept = resample_map(&layer_core::ImageTransform::default(), placement, inputs, display_level)?;
        let side = display.side as f32;
        let clip = layer_core::Affine([side, 0., 0., side, 0., 0.])
            .then(placement.inverse().ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?);
        resample::Resample::values(resample::Request { moved: &moved, kept: &kept, clip, extent, texels, display, target,
            source: if inputs.fine.is_some() {display_mips::Plan::at([extent[0]+inputs.phase[0],extent[1]+inputs.phase[1]],0)} else {inputs.image.plan},
            max_lod: inputs.image.last_level()-inputs.level, outside: 0.,
            keep_source: next.transform.keep_source, identity: next.transform.is_identity() })
    }
    /// Draw `part` of `drawn`, whose corners lie on texel corners, by
    /// evaluating every layer pixel of every texel from the captured originals,
    /// shifted right and down by `phase`.
    #[allow(clippy::too_many_arguments)]
    fn draw_exact(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        phase: [u32; 2],
        drawn: PixelRect,
        level: &wgpu::TextureView,
        display: pixel_transform::DisplayLevel,
        part: pixel_transform::Part,
    ) -> Result<(), GpuRasterError> {
        let side = display.side;
        let snapshot = self.sources[0].as_ref().unwrap();
        let shift = layer_core::ImageTransform::affine(layer_core::Affine::translation(layer_core::Point { x: phase[0] as f32, y: phase[1] as f32 }));
        let splitter = snapshot.splitter(&shift, None, snapshot.bounds)?.aligned(side);
        let mut blocks = std::collections::BTreeMap::new();
        for c in page_coordinates(drawn) {
            blocks
                .entry([c[0] / 2, c[1] / 2])
                .and_modify(|b: &mut PixelRect| *b = b.union(page_rect(c).intersect(drawn)))
                .or_insert(page_rect(c).intersect(drawn));
        }
        let mut jobs = Vec::new();
        for block in blocks.into_values() {
            splitter.split(block, &mut jobs)?;
        }
        drop(splitter);
        let bounds = [
            snapshot.bounds.min_x() as i32,
            snapshot.bounds.min_y() as i32,
            snapshot.bounds.width() as i32,
            snapshot.bounds.height() as i32,
        ];
        let texels = |region| texel_rect(region, side);
        let records: Vec<_> = jobs
            .iter()
            .map(|job| pixel_transform::TiledTransformRecord {
                source_size: [PAGE_SIZE; 2],
                origin: phase.map(|v| -i64::from(v)),
                sources: &job.sources,
                texels: texels(job.region),
                unmoved: false,
                clear: false,
            })
            .collect();
        let offset = self
            .color
            .prepare_tiled(
                &r.device,
                &mut r.uploads,
                encoder,
                bounds,
                0.,
                &layer_core::ImageTransform::default(),
                pixel_transform::PREVIEW_TAPS,
                &records,
                Some((display, part)),
            )
            .map_err(GpuRasterError::InvalidTransform)?;
        let selection = self.has_selection.then_some(self.selection.as_ref()).flatten();
        let captured = |c| snapshot.pages.contains_key(&c);
        let pieces: Vec<_> = jobs.into_iter().map(|job| (job, false)).collect();
        for batch in source_batches(&pieces, captured) {
            let mut sources = Vec::with_capacity(batch.len());
            for (job, _) in &pieces[batch.clone()] {
                let snapshot = self.sources[0].as_ref().unwrap();
                sources.push(snapshot.binding(r, &mut self.color, &job.sources, selection, None, encoder)?);
            }
            let draws: Vec<_> = batch
                .zip(&sources)
                .map(|(index, source)| pixel_transform::BatchDraw {
                    source,
                    job: index,
                    scissor: texels(pieces[index].0.region),
                })
                .collect();
            self.color.encode_display(&r.device, encoder, level, side, offset, &draws);
        }
        Ok(())
    }
    /// Whether `next`'s selection keeps some of the layer's pixels in place.
    fn keeps_pixels(&self, next: &layer_render::TransformPreview) -> bool {
        keeps_pixels(next.selection.as_ref(), self.sources[0].as_ref().unwrap().bounds)
    }
    fn prepare_reduced(
        &mut self, r: &mut WgpuRasterizer, encoder: &mut crate::submission::CommandEncoder,
        selection: Option<&layer_core::Selection>, layer: Option<(SceneView<'_>, SourceTarget)>, extent: [u32; 2], level: u32,
    ) -> Result<(), GpuRasterError> {
        if !self.reducible() {return Ok(());}
        if self.reduced.as_ref().is_none_or(|input|input.level>level) {
            let kept=keeps_pixels(selection,self.sources[0].as_ref().unwrap().bounds);
            let phase=layer.map_or([0;2],|(scene,target)|crate::scene::page_phase(scene.target_offset(target)));
            let mut input=snapshot::DisplayInputs::new(r,level,extent,phase,kept);
            let snapshot=self.sources[0].as_ref().unwrap();
            let bounds=PixelRect::full([extent[0]+phase[0],extent[1]+phase[1]]);
            input.pending=Some(page_coordinates(snapshot.bounds).filter(|c|snapshot.contains(*c))
                .flat_map(|c|page_coordinates(page_rect(c).intersect(PixelRect::full(extent)).translated(phase)))
                .collect::<std::collections::BTreeSet<_>>().into_iter().map(|c|page_rect(c).intersect(bounds)).collect());
            self.reduced=Some(input);
        }
        if self.reduced.as_ref().unwrap().pending.is_none() {return Ok(());}
        r.drain_image_decodes(encoder)?;
        let pipelines=r.display_pipelines.take().unwrap_or_else(||r.recipes.display.clone());
        let result=(|| {
            while let Some(page)=self.reduced.as_ref().unwrap().pending.as_ref().unwrap().front().copied() {
                let input=self.reduced.as_ref().unwrap();let phase=input.phase;let level=input.level;
                if input.kept.is_none() && layer.is_some_and(|(scene,target)|r.scene.as_ref().is_some_and(|cached|
                    cached.copy_reduced_source(r,scene,target,extent,level,page,&input.image.texture,encoder))) {
                    self.reduced.as_mut().unwrap().pending.as_mut().unwrap().pop_front();continue;
                }
                let snapshot=self.sources[0].as_ref().unwrap();
                let local=page.window_local(PixelRect::new(phase[0],phase[1],u32::MAX,u32::MAX)).intersect(PixelRect::full(extent));
                let coordinates=page_coordinates(local).filter(|c|snapshot.contains(*c)).collect::<Vec<_>>();
                let leases=snapshot.prepare_pages(r,&coordinates,encoder)?;
                self.reduce_page(r,encoder,&pipelines,selection,extent,level,page)?;
                crate::scene::sources::Lease::track(leases,encoder);
                self.reduced.as_mut().unwrap().pending.as_mut().unwrap().pop_front();
                #[cfg(test)]
                r.test.reduced_pages.update(|n|n+1);
            }
            let input=self.reduced.as_mut().unwrap();
            for image in std::iter::once(&input.image).chain(input.kept.iter()) {image.generate_mips(&r.device,&pipelines,encoder);}
            input.pending=None;Ok(())
        })();
        r.display_pipelines=Some(pipelines);result
    }

    fn fill_fine_pages(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        coordinates:&[[u32;2]],extent:[u32;2])->Result<(),GpuRasterError> {
        r.drain_image_decodes(encoder)?;
        let mut pending=false;
        for coordinate in coordinates.iter().copied() {
            if self.reduced.as_ref().unwrap().fine.as_ref().unwrap().resident(coordinate) {continue;}
            let snapshot=self.sources[0].as_ref().unwrap();
            let _leases=match snapshot.prepare_pages(r,std::slice::from_ref(&coordinate),encoder) {
                Ok(leases)=>leases,
                Err(GpuRasterError::NotReady)=>{pending=true;continue;},
                Err(error)=>return Err(error),
            };
            let input=self.reduced.as_mut().unwrap();
            let fine=input.fine.as_mut().unwrap();
            let index=fine.assign(coordinate)?;
            let kept=input.kept.is_some();
            if !kept && let Some(tile)=snapshot.original_page(r,coordinate,encoder)?
                && tile.texture.format()==wgpu::TextureFormat::Rgba32Float {
                let origin=fine.origin(index,0);
                encoder.copy_texture_to_texture(tile.texture.as_image_copy(),wgpu::TexelCopyTextureInfo {
                    origin:wgpu::Origin3d {x:origin[0],y:origin[1],z:0},..fine.texture.as_image_copy()},
                    wgpu::Extent3d {width:PAGE_SIZE,height:PAGE_SIZE,depth_or_array_layers:1});
            } else {
                let selection=self.has_selection.then_some(self.selection.as_ref()).flatten();
                let source=snapshot.binding(r,&mut self.color,std::slice::from_ref(&coordinate),selection,None,encoder)?;
                for (part,plane) in std::iter::once((pixel_transform::Part::Selected,0))
                    .chain(kept.then_some((pixel_transform::Part::Kept,1))) {
                    self.color.encode_identity_display_scratch(&r.device,&mut r.uploads,encoder,pixel_transform::IdentityDisplay {
                        target:&fine.view,source:&source,bounds:snapshot.source_bounds(),coordinate,origin:fine.origin(index,plane),extent,part})?;
                }
            }
            fine.written(index,encoder);
            crate::scene::sources::Lease::track(_leases,encoder);
        }
        if pending {Err(GpuRasterError::NotReady)} else {Ok(())}
    }
    fn prepare_fine_input(&mut self,r:&WgpuRasterizer,extent:[u32;2],phase:[u32;2],selection:Option<&layer_core::Selection>,capacity:usize)->Result<(),GpuRasterError> {
        let kept=keeps_pixels(selection,self.sources[0].as_ref().unwrap().bounds);
        if self.reduced.as_ref().is_some_and(|input|input.phase==phase && input.kept.is_some()==kept
            && input.fine.as_ref().is_some_and(|fine|fine.capacity==capacity)) {return Ok(());}
        self.fine_progress.clear();
        if self.reduced.as_ref().is_some_and(|input|input.fine.is_some()) {
            self.reduced=None;
            return Err(GpuRasterError::NotReady);
        }
        let snapshot=self.sources[0].as_ref().unwrap();
        let bounds=PixelRect::full([extent[0]+phase[0],extent[1]+phase[1]]);
        let pages=page_coordinates(snapshot.bounds).filter(|c|snapshot.contains(*c))
            .flat_map(|c|page_coordinates(page_rect(c).intersect(PixelRect::full(extent)).translated(phase)))
            .collect::<std::collections::BTreeSet<_>>().into_iter().map(|c|page_rect(c).intersect(bounds)).collect();
        if self.reduced.as_ref().is_some_and(|input|input.level==1 && input.pending.is_none() && input.phase==phase && input.kept.is_some()==kept) {
            let input=self.reduced.as_mut().unwrap();
            let mut fine=fine::FineInputs::new(r,capacity,kept,input.storage_bytes(),Default::default())?;
            fine.complete=true;input.level=0;input.fine=Some(fine);
        } else {self.reduced=Some(snapshot::DisplayInputs::new_fine(r,extent,phase,kept,capacity,pages)?);}
        Ok(())
    }
    fn prepare_fine_reduced(&mut self,r:&mut WgpuRasterizer,encoder:&mut crate::submission::CommandEncoder,
        selection:Option<&layer_core::Selection>,extent:[u32;2])->Result<(),GpuRasterError> {
        self.reduced.as_mut().unwrap().fine.as_mut().unwrap().restart_discarded();
        if self.reduced.as_ref().unwrap().fine.as_ref().unwrap().complete {return Ok(());}
        r.drain_image_decodes(encoder)?;
        let pipelines=r.display_pipelines.take().unwrap_or_else(||r.recipes.display.clone());
        let result=(|| {
            loop {
                let page=self.reduced.as_ref().unwrap().fine.as_ref().unwrap().pages.front().copied();
                let Some(page)=page else {break;};
                let snapshot=self.sources[0].as_ref().unwrap();
                let phase=self.reduced.as_ref().unwrap().phase;
                let local=page.window_local(PixelRect::new(phase[0],phase[1],u32::MAX,u32::MAX)).intersect(PixelRect::full(extent));
                let coordinates=page_coordinates(local).filter(|c|snapshot.contains(*c)).collect::<Vec<_>>();
                let _leases=snapshot.prepare_pages(r,&coordinates,encoder)?;
                self.reduce_page(r,encoder,&pipelines,selection,extent,1,page)?;
                crate::scene::sources::Lease::track(_leases,encoder);
                let fine=self.reduced.as_mut().unwrap().fine.as_mut().unwrap();
                fine.track_coarse(encoder);fine.pages.pop_front();
            }
            let input=self.reduced.as_mut().unwrap();
            for image in std::iter::once(&input.image).chain(input.kept.iter()) {image.generate_mips(&r.device,&pipelines,encoder);}
            let fine=input.fine.as_mut().unwrap();fine.track_coarse(encoder);fine.complete=true;
            Ok(())
        })();
        r.display_pipelines=Some(pipelines);
        result
    }
    /// Reduce one page of the reduced image from the layer pixels it covers,
    /// into the image of the pixels that move, or of those kept when the
    /// selection leaves them out, or draw both exactly where a selection edge
    /// splits them.
    #[allow(clippy::too_many_arguments)]
    fn reduce_page(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        pipelines: &display_mips::Pipelines,
        selection: Option<&layer_core::Selection>,
        extent: [u32; 2],
        level: u32,
        page: PixelRect,
    ) -> Result<(), GpuRasterError> {
        let snapshot = self.sources[0].as_ref().unwrap();
        let reduced = self.reduced.as_mut().unwrap();
        let phase = reduced.phase;
        let local = page.window_local(PixelRect::new(phase[0], phase[1], u32::MAX, u32::MAX)).intersect(PixelRect::full(extent));
        let moves = reduced.kept.is_none()
            || selection.is_some_and(|s| selects_all(s, local.intersect(snapshot.bounds)));
        let kept = !moves && local.intersect(pixel_rect(self.plan.cut, extent)).is_empty();
        let image = match &mut reduced.kept {
            Some(image) if kept => Some(image),
            _ => moves.then_some(&mut reduced.image),
        };
        if let Some(image) = image {
            let shifted = phase != [0; 2];
            if shifted { image.clear_scratch(encoder); }
            let mut written = shifted;
            for c in page_coordinates(local).filter(|c| snapshot.contains(*c)) {
                if let Some(tile) = snapshot.original_page(r, c, encoder)? {
                    let part = page_rect(c).intersect(local);
                    let destination = part.translated(phase).page_local([page.min_x() / PAGE_SIZE, page.min_y() / PAGE_SIZE]);
                    image.copy_to_scratch(encoder, &tile.texture, part.page_local(c), [destination.min_x(), destination.min_y()])?;
                    written = true;
                }
            }
            if written { image.reduce_scratch(&r.device, pipelines, encoder, [page.min_x() / PAGE_SIZE, page.min_y() / PAGE_SIZE])?; }
            return Ok(());
        }
        let parts = [
            (pixel_transform::Part::Selected, reduced.image.view.clone()),
            (pixel_transform::Part::Kept, reduced.kept.as_ref().unwrap().view.clone()),
        ];
        let display = pixel_transform::DisplayLevel {
            side: 1 << level,
            opacity: 1.,
            extent,
            backdrop: [0.; 4],
            encode: false,
        };
        parts.iter().try_for_each(|(part, view)| self.draw_exact(r, encoder, phase, page, view, display, *part))
    }
    fn retain_pages(
        &mut self,
        r: &mut WgpuRasterizer,
        id: SourceTarget,
        regions: &[PixelRect],
        transform: Option<&layer_core::ImageTransform>,
    ) {
        let support = transform
            .map(|t| self.plan.channel_regions(t, r.target_extent(id)))
            .unwrap_or([[PixelRect::EMPTY; 2]; 2]);
        let keep = |c: [u32; 2], original: &Vec<_>, regions: &[PixelRect]| {
            original.contains(&c) || regions.iter().any(|b| !b.page_local(c).is_empty())
        };
        if self.background.is_some() {
            r.layer_masks.previews.retain(|(mask,c),_|*mask!=id || regions.iter().any(|region|!region.page_local(*c).is_empty()));
        } else if let Some(layer) = r.paint_layers.iter_mut().find(|l| l.id == id) {
            self.spares.paint.extend(layer.pages.extract_if(.., |p| {
                !keep(p.coordinate, &self.original_pages[0], regions)
            }));
            self.spares
                .watercolor
                .extend(layer.watercolor_wetness_pages.extract_if(.., |p| {
                    !keep(p.coordinate, &self.original_pages[1], &support[1])
                }));
        }
    }
}
fn snapshot_page(device: &wgpu::Device, format: wgpu::TextureFormat) -> snapshot::SnapshotPage {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("reusable transform copy before overwrite"),
        size: wgpu::Extent3d {
            width: PAGE_SIZE,
            height: PAGE_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    snapshot::SnapshotPage::unwritten(texture,view)
}

fn destination(r:&WgpuRasterizer,id:SourceTarget,channel:usize,mask:bool,coordinate:[u32;2])->Option<&resident::Resident<PageSurface>> {
    if mask {return None;}
    let layer=r.paint_layers.iter().find(|layer|layer.id==id)?;
    match channel {0=>Some(layer.pages.iter().find(|page|page.coordinate==coordinate)?.active()),
        _=>Some(layer.watercolor_wetness_pages.iter().find(|page|page.coordinate==coordinate)?.active())}
}

type TexturePages = [Vec<([u32; 2], snapshot::SnapshotPage)>; 2];
fn source_pages(
    r: &WgpuRasterizer,
    id: SourceTarget,
    receipt: Option<&crate::submission::Submission>,
) -> Result<(Option<f32>, TexturePages), GpuRasterError> {
    let capture=|coordinate,plane,surface:&crate::resident::Resident<PageSurface>,data:Option<&layer_core::raster::RasterData>| {
        let expected=match data {
            Some(data)=>data.tiles.get(&TileKey {plane,coordinate}).ok_or(GpuRasterError::Invariant("prepared source pages state is inconsistent"))?.content_version(),
            None=>r.current_raster_version(id,plane,coordinate)?,
        };
        let read=surface.read(&expected,receipt).ok_or(GpuRasterError::NotReady)?;
        Ok::<_,GpuRasterError>((coordinate,snapshot::SnapshotPage::captured(read)?))
    };
    if let Some((data,pages))=r.staged_transform_source(id).filter(|_|!id.is_coverage()) {
        let original=r.native_backing(id);
        let unchanged=|coordinate,plane|original.and_then(|old|old.tiles.get(&TileKey {plane,coordinate}))
            .zip(data.tiles.get(&TileKey {plane,coordinate})).is_some_and(|(before,after)|before.same_capture(after));
        let mut result:[std::collections::BTreeMap<_,_>;2]=Default::default();
        if let Some(layer)=r.paint_layers.iter().find(|layer|layer.id==id) {
            result[0].extend(layer.pages.iter().filter(|p|unchanged(p.coordinate,RasterPlane::Color))
                .map(|p|capture(p.coordinate,RasterPlane::Color,p.active(),Some(data))).collect::<Result<Vec<_>,_>>()?);
            result[1].extend(layer.watercolor_wetness_pages.iter().filter(|p|unchanged(p.coordinate,RasterPlane::WatercolorWetness))
                .map(|p|capture(p.coordinate,RasterPlane::WatercolorWetness,p.active(),Some(data))).collect::<Result<Vec<_>,_>>()?);
        }
        result[0].extend(pages.colors.iter().map(|p|capture(p.coordinate,RasterPlane::Color,p.active(),Some(data))).collect::<Result<Vec<_>,_>>()?);
        result[1].extend(pages.wetness.iter().map(|p|capture(p.coordinate,RasterPlane::WatercolorWetness,p.active(),Some(data))).collect::<Result<Vec<_>,_>>()?);
        return Ok((None,result.map(|pages|pages.into_iter().collect())));
    }
    if let Some(mask) = r.layer_masks.definitions.get(&id) {
        return Ok((
            Some(mask.default_coverage),
            [
                r.layer_masks
                    .pages
                    .iter()
                    .filter(|((target, _), _)| *target == id)
                    .filter_map(|((_, c), p)| match r.current_mask_version(id,*c) {
                        Err(error)=>Some(Err(error)),
                        Ok(expected)=>p.read(&expected,receipt).ok().map(|read|snapshot::SnapshotPage::captured(read).map(|page|(*c,page))),
                    })
                    .collect::<Result<Vec<_>,GpuRasterError>>()?,
                Vec::new(),
            ],
        ));
    }
    let l = r
        .paint_layers
        .iter()
        .find(|l| l.id == id)
        .ok_or(GpuRasterError::MissingPaintLayer(id))?;
    Ok((
        None,
        [
            l.pages
                .iter()
                .map(|p|capture(p.coordinate,RasterPlane::Color,p.active(),None))
                .collect::<Result<Vec<_>,_>>()?,
            l.watercolor_wetness_pages
                .iter()
                .map(|p|capture(p.coordinate,RasterPlane::WatercolorWetness,p.active(),None))
                .collect::<Result<Vec<_>,_>>()?,
        ],
    ))
}
fn selection_capture(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("immutable transform selection"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
#[path="paint_transform/preparation_tests.rs"]
mod preparation_tests;

#[cfg(test)]
mod prepared_region_tests {
    use super::*;

    fn renderer() -> (WgpuRasterizer, SourceTarget) {
        let doc=crate::layer_tests::placement::paint_document([64;2],"Transform");
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        r.ensure_document_metadata([64;2],doc.scene()).unwrap();
        let input=snapshot::DisplayInputs::new(&r,0,[64;2],[0;2],false);
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        {let _pass=encoder.color_pass("prepared transform input",&input.image.view,wgpu::LoadOp::Clear(wgpu::Color {r:1.,g:0.,b:0.,a:1.}));}
        encoder.submit(&r.queue);crate::test_support::complete(&r);
        r.transforms.as_mut().unwrap().0[0].reduced=Some(input);
        (r,crate::layer_tests::placement::target(&doc))
    }
    fn prepare(r:&mut WgpuRasterizer,target:SourceTarget,x:f32,destination:&wgpu::TextureView)->PreparedRegion {
        let mesh=layer_core::MeshMap::from_affine(layer_core::Rect::from_extent([64;2]),[2;2],
            layer_core::Affine::translation(layer_core::Point {x,y:0.})).unwrap();
        let mut transforms=r.transforms.take().unwrap();
        transforms.0[0].preview=Some(layer_render::TransformPreview {transaction:1,moving:false,target,selection:None,
            transform:layer_core::ImageTransform {placement:layer_core::LayerPlacement {mesh:Some(Arc::new(mesh)),..Default::default()},..Default::default()}});
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        let region=transforms.prepare_region(r,&mut encoder,target,layer_core::Affine::IDENTITY,destination,
            pixel_transform::DisplayLevel {side:1,extent:[64;2],opacity:1.,backdrop:[0.;4],encode:false},
            display_mips::Plan::at([64;2],0),PixelRect::full([64;2]).into(),0).unwrap();
        assert_eq!(encoder.pass_count(),0);
        r.transforms=Some(transforms);region
    }
    fn encode(r:&mut WgpuRasterizer,regions:&[PreparedRegion])->submission::CommandEncoder {
        let stride=resample::UNIFORM_BYTES.max(u64::from(r.device.limits().min_uniform_buffer_offset_alignment));
        let uniforms=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("prepared transform records"),
            size:stride*regions.len() as u64,usage:wgpu::BufferUsages::UNIFORM|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let mut encoder=submission::CommandEncoder::new(&r.device,&Default::default());
        for (index,region) in regions.iter().enumerate() {
            let offset=index as u64*stride;
            r.uploads.write_at(&mut encoder,&uniforms,offset,region.values().unwrap()).unwrap();
            region.encode(r,&mut encoder,&uniforms,offset).unwrap();
        }
        r.uploads.finish(&encoder);encoder
    }
    fn pixel(r:&WgpuRasterizer,texture:&wgpu::Texture)->[f32;4] {
        let bytes=crate::layer_tests::page_bytes(r,texture);let offset=(32*64+8)*16;
        std::array::from_fn(|channel|f32::from_le_bytes(bytes[offset+channel*4..offset+channel*4+4].try_into().unwrap()))
    }
    #[test]
    fn prepared_transform_draws_upload_their_own_geometry_at_commit() {
        let (mut r,target)=renderer();
        pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.resample.mesh.iter())).unwrap();
        let a=create_target(&r.device,[64;2],wgpu::TextureFormat::Rgba32Float,"first prepared mesh");
        let b=create_target(&r.device,[64;2],wgpu::TextureFormat::Rgba32Float,"second prepared mesh");
        let first=prepare(&mut r,target,0.,&a.1);let second=prepare(&mut r,target,16.,&b.1);
        assert_eq!(r.transforms.as_ref().unwrap().0[0].display_mesh.storage_bytes(),0);
        encode(&mut r,&[first,second]).submit(&r.queue);crate::test_support::complete(&r);
        let first=pixel(&r,&a.0);let second=pixel(&r,&b.0);
        assert!(first[0]>0.99 && first[3]>0.99,"first geometry: {first:?}");
        assert!(second[3]<0.01,"second geometry: {second:?}");
    }
    #[test]
    fn discarded_transform_geometry_is_uploaded_before_its_next_draw() {
        let (mut r,target)=renderer();
        pollster::block_on(r.compilation.prepare_all(r.scene_pipelines.resample.mesh.iter())).unwrap();
        let output=create_target(&r.device,[64;2],wgpu::TextureFormat::Rgba32Float,"discarded prepared mesh");
        let region=prepare(&mut r,target,0.,&output.1);
        drop(encode(&mut r,std::slice::from_ref(&region)));
        encode(&mut r,std::slice::from_ref(&region)).submit(&r.queue);crate::test_support::complete(&r);
        let actual=pixel(&r,&output.0);
        assert!(actual[0]>0.99 && actual[3]>0.99,"discarded geometry must be uploaded again: {actual:?}");
    }
}
