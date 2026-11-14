//! Bakes, such as merges and Frequency Separation: a bake composites its
//! members, isolated, into the empty pages of its target, with the sampling of
//! export rather than of the display. A bake keeps no filter images, so when
//! its members' filters would need more than the default image budget they
//! run in bounded windows with their halos, each retired before the next, as
//! the display composes them.
use super::*;

/// Tiles recorded before their jobs are encoded.
const TILES_PER_BATCH: usize = 64;

struct BakePage {
    coordinate:[u32;2],
    image:Image,
    pixels:resident::Resident<crate::source_access::RawTile>,
    intent:crate::raster::contents::Intent,
}
impl BakePage {
    fn publish(&self,scene:&mut Scene,r:&WgpuRasterizer,target:SourceTarget,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        r.validate_raster_intent(target,&self.intent)?;
        let mut pixels=self.pixels.clone();let receipt=encoder.submission();
        let write=pixels.write(&self.intent,&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
        scene.publish_surface(target,write.encoded(layer_core::raster::ContentName::fresh()));Ok(())
    }
}

impl Scene {
    /// Composite `members` into `layer`'s pages within `damage`. Watercolor
    /// members settle into the result, which keeps no wet state.
    #[allow(clippy::too_many_arguments)] // The operation's own operands.
    pub(super) fn bake(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        target: SourceTarget,
        operation: u32,
        snapshot: &Arc<layer_core::SceneSnapshot>,
        scope: &layer_core::SceneScope,
        offset: layer_core::Point,
        damage: PixelRect,
        coverage: &layer_core::CoverageSnapshot,
        low: Option<SourceTarget>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        let scene = snapshot.view().with_scope(scope).with_offset(offset);
        let extent = packet.scene.target_extent(target);
        let source = FramePacket {
            commit_rasters: true,
            view: packet.view,
            document_extent: extent,
            scene,
            selection_overlays: None,
            inspect_mask: None,
            dabs: &[],
            dab_batches: &[],
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            time_seconds: snapshot.context.elapsed,
            blend_space: scene.composition().blend,
        };
        let receipt=encoder.submission();
        let mut pages: Vec<_> = r
            .paint_layers
            .iter()
            .filter(|stored| stored.id == target)
            .flat_map(|stored| &stored.pages)
            .filter(|page| !damage.page_local(page.coordinate).is_empty())
            .map(|page| {
                let intent=r.raster_intent(target,RasterPlane::Color,page.coordinate)?;
                let read=page.active().read(intent.version(),Some(&receipt)).ok_or(GpuRasterError::NotReady)?;
                let image=Image {texture:read.value().texture.clone(),view:read.value().view.clone(),
                    plan:display_mips::Plan::window(extent,0,page_rect(page.coordinate).intersect(PixelRect::full(extent)))};
                let pixels=read.alias(crate::source_access::RawTile {texture:image.texture.clone().into(),view:image.view.clone()}).ok_or(GpuRasterError::Invariant("snapshot aliases different physical storage"))?;
                Ok(BakePage {coordinate:page.coordinate,image,pixels,intent})
            }).collect::<Result<_,GpuRasterError>>()?;
        if target.is_coverage() {
            let receipt=encoder.submission();
            for ((_,coordinate),page) in r.layer_masks.pages.iter().filter(|((id,c),_)|*id==target && !damage.page_local(*c).is_empty()) {
                let intent=r.raster_intent(target,layer_core::raster::RasterPlane::Mask,*coordinate)?;
                let read=page.read(intent.version(),Some(&receipt))?;
                let image=Image {texture:read.value().texture.clone(),view:read.value().view.clone(),
                    plan:display_mips::Plan::window(extent,0,page_rect(*coordinate).intersect(PixelRect::full(extent)))};
                let pixels=read.alias(crate::source_access::RawTile {texture:image.texture.clone().into(),view:image.view.clone()}).ok_or(GpuRasterError::Invariant("snapshot aliases different physical storage"))?;
                pages.push(BakePage {coordinate:*coordinate,image,pixels,intent});
            }
        }
        if !r.snapshot_worker {
            self.jobs.clear();self.source_jobs.clear();self.used.iter_mut().for_each(scratch::Usage::reset);
            let declaration=self.capture_declaration(r,source)?;
            for page in &pages {
                let coordinate=page.coordinate;let destination=&page.image;
                let mut accepted=self.accept_capture(r,source,Some(Output::Artwork(None)),&declaration,|this,r,accepted|
                    this.bake_output(r,source,coordinate,coverage,(target,operation,false),low,accepted).map(drop))?;
                let output=self.bake_output(r,source,coordinate,coverage,(target,operation,false),low,&mut accepted)?;
                let _output=self.own(output);
                let output=if target.is_coverage() {
                    let scalar=self.reserve_format(r,true)?;
                    self.draw(r,scalar,self.pool[output].view.clone(),Some(destination.view.clone()),[0.,0.,PAGE_SIZE as f32,PAGE_SIZE as f32],[21.,1.,0.,0.],false,Convert::None);
                    self.free(output);scalar
                }else {output};
                self.copy_window_tile(output,destination,coordinate);
                page.publish(self,r,target,encoder)?;accepted.track(encoder);
            }
            return self.encode_jobs(r,encoder);
        }
        self.placement_display = false;
        self.stop_before = None;
        let budget = r.image_pixel_budget(r.scale_display.as_ref().map_or(0, |c| c.resident_bytes()));
        let plan = windows::Plan::new(scene, extent, budget, r.device.limits().max_texture_dimension_2d)?;
        let regions: Vec<_> = plan.map_or_else(|| vec![(damage, Self::capture_window(scene, damage, extent))], |plan| plan.regions(scene, damage).collect());
        let multiple = regions.len() > 1;
        let analysis = r.bake_analysis_entries(snapshot, scope, offset, extent)?;
        let prepared_masks = r.bake_mask_pages(snapshot, scope, offset, extent)?;
        if let Some(prepared) = prepared_masks {
            let masks = r.layer_masks.snapshots.get_mut(&(target, operation, false)).expect("prepared bake mask inventory");
            masks.pages = prepared.pages.clone();
            masks._lease = prepared._lease.clone();
        }
        let previous = analysis.map(|entries| std::mem::replace(&mut r.effect_analyses, entries));
        let live_masks = r.layer_masks.bind_snapshot((target, operation, false));
        let result = (|| {
            for (output, window) in regions {
                let pages: Vec<_> = pages.iter().filter(|page| !output.page_local(page.coordinate).is_empty()).collect();
                if pages.is_empty() {
                    continue;
                }
                self.retire_images(|scene| scene.images = images::ImageStages::charged(&scene.tiers.gpu, &scene.tiers.ram));
                self.image_window = Some(window);
                self.image_output = Some(output);
                self.update_images(r, source, output, encoder)?;
                self.bake_pages(r, source, &pages, coverage, (target, operation, false), low, encoder)?;
                if plan.is_some() {
                    r.metrics.image_window_peak_bytes = r.metrics.image_window_peak_bytes.max(self.images.storage_bytes());
                    if multiple {
                        Self::retire_window(r, encoder, "after bounded bake window")?;
                        r.metrics.image_window_submissions += 1;
                    }
                }
            }
            Ok(())
        })();
        if let Some(live) = live_masks { r.layer_masks.restore_snapshot((target, operation, false), live); }
        if let Some(previous) = previous { r.effect_analyses = previous; }
        self.image_window = None; self.image_output = None;
        self.retire_images(|scene| scene.images = images::ImageStages::charged(&scene.tiers.gpu, &scene.tiers.ram));
        result
    }

    fn bake_output(&mut self,r:&WgpuRasterizer,source:FramePacket<'_>,coordinate:[u32;2],coverage:&layer_core::CoverageSnapshot,command:layer_masks::CommandCoverage,low:Option<SourceTarget>,accepted:&mut sources::AcceptedInputs)->Result<usize,GpuRasterError> {
                #[cfg(test)]
                let _record=sources::record_reads(accepted);
                let mut output=if let Some(view)=r.prepared_bake(command.0,command.1,coordinate) {
                    let output=self.alloc(r,wgpu::Color::TRANSPARENT)?;
                    self.draw(r,output,view,None,[0.,0.,PAGE_SIZE as f32,PAGE_SIZE as f32],[1.,1.,0.,0.],false,Convert::None);output
                }else if r.snapshot_worker {self.group(r,source,None,coordinate,accepted)?}else {return Err(GpuRasterError::NotReady);};
                let mut held=self.own(output);
                if let Some(low) = low {
                    let [read,_]=accepted.raw_color(r,low,coordinate)?;
                    let low=read.map(|read|self.read_input(read)).unwrap_or_else(||r.empty_view.clone());
                    let detail = self.alloc(r, wgpu::Color::TRANSPARENT)?;
                    self.draw(r, detail, self.pool[output].view.clone(), Some(low),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [17., 1., 0., 0.], false, Convert::None);
                    self.free(output);
                    output = detail;
                    held=self.own(output);
                }
                if coverage.source.default_coverage != 1. || coverage.use_.inverted || coverage.selection.is_some()
                    || !coverage.source.raster.is_empty() || !coverage.source.operations.is_empty() {
                    let mask = self.command_mask_at(r, coverage, coordinate, command, accepted)?;
                    let clipped = self.alloc(r, wgpu::Color::TRANSPARENT)?;
                    self.draw(r, clipped, self.pool[output].view.clone(), Some(mask.view.clone()),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [3., 2., 0., 0.], false, Convert::None);
                    self.free(output); output = clipped;
                    held=self.own(output);
                }
                let output=self.keep(held);
                self.converted(r, output, Convert::stored(source))
    }

    /// Composite `pages`, keeping each to the part inside `extent`; the rest
    /// of a page stays transparent.
    #[expect(clippy::too_many_arguments, reason = "Bake operands retain independent source, coverage and command ownership")]
    fn bake_pages(
        &mut self,
        r: &mut WgpuRasterizer,
        source: FramePacket<'_>,
        pages: &[&BakePage],
        coverage: &layer_core::CoverageSnapshot,
        command: layer_masks::CommandCoverage,
        low: Option<SourceTarget>,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        for batch in pages.chunks(TILES_PER_BATCH) {
            self.jobs.clear();
            self.used.iter_mut().for_each(scratch::Usage::reset);
            let declaration=self.capture_declaration(r,source)?;
            for page in batch.iter().copied() {
                let coordinate=page.coordinate;let destination=&page.image;
                let mut accepted=self.accept_capture(r,source,Some(Output::Artwork(None)),&declaration,|this,r,accepted|
                    this.bake_output(r,source,coordinate,coverage,command,low,accepted).map(drop))?;
                let output = self.bake_output(r,source,coordinate,coverage,command,low,&mut accepted)?;
                let _output = self.own(output);
                let output = if command.0.is_coverage() {
                    let scalar = self.reserve_format(r, true)?;
                    self.draw(r, scalar, self.pool[output].view.clone(), Some(destination.view.clone()),
                        [0., 0., PAGE_SIZE as f32, PAGE_SIZE as f32], [21., 1., 0., 0.], false, Convert::None);
                    self.free(output); scalar
                } else { output };
                self.copy_window_tile(output, destination, coordinate);
                page.publish(self,r,command.0,encoder)?;accepted.track(encoder);
            }
            self.encode_jobs(r, encoder)?;
        }
        Ok(())
    }
}
