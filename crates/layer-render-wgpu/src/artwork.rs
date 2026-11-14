//! Exact artwork queries own bounded captures, independently of presentation.
//! The retained frame contains composition metadata and current preview styles;
//! pixels remain in paint/source backing, never in a second document image.
use super::*;
use layer_render::ViewState;

#[derive(Clone)]
pub(super) struct Frame {
    pub scene: Arc<SceneSnapshot>,
    pub scope: SceneScope,
    pub view: ViewState,
    pub blend_space: layer_core::BlendSpace,
    pub time: f32,
    pub previews: Vec<DabBatch>,
    preview_dabs: Vec<Dab>,
}
pub(super) struct PendingFrame {
    pub frame: Arc<Frame>,
    pub view: ViewState,
    pub capture: Option<raster::native_edit::NativeJob>,
}
impl Drop for PendingFrame {
    fn drop(&mut self) {
        let view = self.frame.scene.view();
        for root in view.targets().filter_map(|target| view.raster(target)) {
            if root.try_data().is_none() { let _ = root.publish(Err("Healing stopped before raster capture".into())); }
        }
    }
}
impl Frame {
    /// Selection overlays, navigation and layer labels do not alter raw artwork.
    /// Source identity and raster publication catch edits without scanning pixels.
    pub fn same_evaluation(&self, packet: FramePacket<'_>) -> bool {
        self.blend_space == packet.blend_space
            && (self.time == packet.time_seconds || !packet.scene.order().iter().any(|&h|
                packet.scene.visible(h) && packet.scene.effect(h).is_some_and(|e| e.animated())))
    }
    pub fn same_artwork(&self, packet: FramePacket<'_>) -> bool {
        let old = self.scene.view();
        self.same_evaluation(packet)
            && self.previews.is_empty() && packet.dab_batches.is_empty()
            && old.same_artwork(packet.scene)
            && self.same_phases(packet)
    }

    fn same_phases(&self, packet: FramePacket<'_>) -> bool {
        let old = self.scene.view();
        packet.scene.evaluation_context().is_none_or(|_| {
            packet
                .scene
                .order()
                .iter()
                .filter(|h| packet.scene.effect(**h).is_some())
                .all(|h| {
                    effects::effective_phase(old, *h, self.time).to_bits()
                        == effects::effective_phase(packet.scene, *h, packet.time_seconds).to_bits()
                })
        })
    }

    pub fn same_preview_inputs(&self, packet: FramePacket<'_>) -> bool {
        let batches=packet.dab_batches.iter().filter(|batch|batch.kind==DabBatchKind::Preview);
        self.previews.len()==batches.clone().count()
            && self.previews
            .iter()
            .zip(batches)
            .all(|(old,batch)| {
                *old==DabBatch {first_dab:old.first_dab,..batch.clone()}
                    && packet.dabs.get(batch.first_dab as usize..(batch.first_dab+batch.dab_count) as usize)
                        == self.preview_dabs.get(old.first_dab as usize..(old.first_dab+old.dab_count) as usize)
            })
    }

    pub fn same_preview(&self, packet: FramePacket<'_>) -> bool {
        !self.previews.is_empty()
            && self.previews.len() == packet.dab_batches.len()
            && self.view == packet.view
            && self.scope == packet.scene.scope().cloned().unwrap_or_default()
            && self.same_preview_inputs(packet)
            && packet.dab_batches.iter().all(|batch|
                dry_material::dry_flow(&batch.style) && pointwise(&batch.style))
            && self.same_evaluation(packet)
            && self
                .scene
                .view()
                .with_scope(&self.scope)
                .same_artwork(packet.scene)
            && self.same_phases(packet)
    }

    pub fn new(packet: FramePacket<'_>, context: EvaluationContext) -> Self {
        let mut preview_dabs = Vec::new();
        let previews = packet.dab_batches.iter().filter(|b| b.kind == DabBatchKind::Preview)
            .map(|batch| {
                let mut retained = batch.clone();
                retained.first_dab = preview_dabs.len() as u32;
                preview_dabs.extend_from_slice(&packet.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize]);
                retained
            }).collect();
        Self {
            scene: Arc::new(layer_core::SceneSnapshot { context, ..packet.scene.snapshot() }),
            scope: packet.scene.scope().cloned().unwrap_or_default(),
            view: packet.view,
            blend_space: packet.blend_space,
            time: packet.time_seconds,
            previews,
            preview_dabs,
        }
    }
    pub fn packet(&self, extent: [u32; 2]) -> FramePacket<'_> {
        FramePacket {
            commit_rasters: true,
            scene: self.scene.view().with_scope(&self.scope),
            selection_overlays: None,
            inspect_mask: None,
            view: self.view,
            document_extent: extent,
            time_seconds: self.time,
            dabs: &self.preview_dabs,
            dab_batches: &self.previews,
            restore_rasters: &[],
            reset_layers: false,
            composite_all: true,
            blend_space: self.blend_space,
        }
    }
}

#[derive(Default)]
pub(super) struct Capture {
    scene: Option<Box<scene::Scene>>,
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    window: Option<DocRect>,
    pub peak_image_bytes: u64,
    pub image_limit: Option<u64>,
    retiring: Option<Arc<std::sync::atomic::AtomicBool>>,
}
impl Capture {
    pub fn layer_tile(&mut self, r: &mut WgpuRasterizer, id: SourceTarget, coordinate: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<source_access::RawTile,GpuRasterError> {
        let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        self.layer_tile_at(r,&frame,id,coordinate,encoder)
    }
    pub fn layer_tile_at(&mut self,r:&mut WgpuRasterizer,frame:&Frame,id:SourceTarget,coordinate:[u32;2],encoder:&mut crate::submission::CommandEncoder)->Result<source_access::RawTile,GpuRasterError> {
        if self.scene.is_none() {self.scene=Some(scene::Scene::new(r)?);}
        self.scene.as_mut().unwrap().layer_tile_for_query(r,frame.packet(r.document_extent),id,coordinate,encoder)
    }
    pub fn thumbnail_tile(&mut self, r: &mut WgpuRasterizer, target: layer_render::ThumbnailTarget, coordinate: [u32;2], encoder: &mut submission::CommandEncoder) -> Result<source_access::RawTile, GpuRasterError> {
        let frame = r.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        if let Some(source) = r.thumbnail_source(target) { return self.layer_tile(r, source, coordinate, encoder); }
        let layer_render::ThumbnailTarget::Occurrence(handle) = target else { return Err(GpuRasterError::InvalidExtent); };
        let view = frame.scene.view();
        let occurrence = view.occurrence(handle).ok_or(GpuRasterError::InvalidExtent)?;
        let scope = if matches!(occurrence.content, OccurrenceContent::Stack(_)) {
            SceneScope::Members(view.order().iter().copied().filter(|h| *h == handle || layer_core::descends_from(view, *h, Some(handle))).collect::<Vec<_>>().into())
        } else {
            let mut input = layer_core::composite_input_layers(view, handle); input.push(handle); SceneScope::Members(input.into())
        };
        let packet = FramePacket {scene:view.with_scope(&scope),..frame.packet(r.document_extent)};
        let region = page_rect(coordinate).intersect(PixelRect::full(r.document_extent));
        self.region(r, packet, region, [PAGE_SIZE;2], encoder)
    }
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, |(t, _)| texture_bytes(t))
    }
    /// The caller consumes this target before requesting another region. Edges
    /// occupy its top-left prefix; no samples outside `region` are meaningful.
    pub fn region(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        region: PixelRect,
        size: [u32; 2],
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<source_access::RawTile, GpuRasterError> {
        if size.contains(&0) || size[0] > PAGE_SIZE || size[1] > PAGE_SIZE {
            return Err(GpuRasterError::InvalidExtent);
        }
        self.image_bytes(packet, region)?;
        let resized = self
            .target
            .as_ref()
            .is_some_and(|(t, _)| [t.width(), t.height()] != size);
        if resized {
            scene::Scene::submit_chunk(r, encoder, "artwork query target")?;
        }
        if self.target.is_none() || resized {
            self.target = Some(create_color_target(
                &r.device,
                size,
                "bounded artwork query",
            ));
        }
        let (texture, view) = self.target.clone().unwrap();
        self.region_into(r, packet, region, &texture, encoder)?;
        Ok(source_access::RawTile { texture: texture.into(), view })
    }
    /// Capture `region` into the top-left prefix of `destination`, a working
    /// format texture at least as large.
    pub fn region_into(
        &mut self,
        r: &mut WgpuRasterizer,
        packet: FramePacket<'_>,
        region: PixelRect,
        destination: &wgpu::Texture,
        encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(), GpuRasterError> {
        if region.width() > destination.width() || region.height() > destination.height() {
            return Err(GpuRasterError::InvalidExtent);
        }
        r.ensure_exact_preview(encoder)?;
        let (window, bytes) = self.image_bytes(packet, region)?;
        let retiring = self.retiring.take();
        if self.retires_window(window) {
            let wake = r.preparation_wake.clone();
            let retired = match retiring { Some(retired) => retired, None => scene::Scene::submit_window(r, encoder, "artwork query window", wake)? };
            if !retired.load(std::sync::atomic::Ordering::Acquire) { self.retiring = Some(retired); return Err(GpuRasterError::NotReady); }
            if let Some(scene) = &mut self.scene {
                scene.release_capture_window(window);
            }
        }
        if self.scene.is_none() { self.scene = Some(scene::Scene::new(r)?); }
        let scene = self.scene.as_mut().unwrap();
        let output = packet.scene.scope().and_then(scene::raw_output).unwrap_or(scene::Output::Artwork(None));
        scene.capture_region_prepared(r, packet, destination, region, output, encoder)?;
        self.window = Some(window);
        self.peak_image_bytes = self.peak_image_bytes.max(bytes);
        Ok(())
    }
    /// The window capturing `region` reads and the filter images it needs,
    /// refused above the image limit before anything is allocated.
    fn image_bytes(&self, packet: FramePacket<'_>, region: PixelRect) -> Result<(DocRect, u64), GpuRasterError> {
        let window = scene::Scene::capture_window(packet.scene, region, packet.document_extent);
        let bytes = scene::Scene::capture_image_bound(packet.scene, region);
        let limit = self.image_limit.unwrap_or(256 * 1024 * 1024);
        if bytes > limit {
            return Err(GpuRasterError::Color(format!(
                "Artwork query requires {bytes} bytes of filter images; its limit is {limit} bytes"
            )));
        }
        Ok((window, bytes))
    }
    fn retires_window(&self, window: DocRect) -> bool {
        self.window.is_some_and(|old| old != window) && self.peak_image_bytes > 0
    }
    /// Whether capturing `region` now could wait for the GPU or upload source
    /// pixels: it needs filter images, retires earlier ones, or decodes tiles.
    pub fn would_block(&self, r: &WgpuRasterizer, packet: FramePacket<'_>, region: PixelRect) -> bool {
        let window = scene::Scene::capture_window(packet.scene, region, packet.document_extent);
        scene::Scene::capture_image_bound(packet.scene, region) > 0
            || self.retires_window(window) && !self.retiring.as_ref().is_some_and(|retired| retired.load(std::sync::atomic::Ordering::Acquire))
            || page_coordinates(region).any(|tile| {
                r.source_tiles.borrow().uploads_full(crate::residency::Class::Visible) || source_tile_decodes(r, packet.scene, tile)
            })
    }
}

pub(super) fn resident_first(r: &WgpuRasterizer, scene: SceneView<'_>, pages: &mut [[u32; 2]]) {
    pages.sort_by_cached_key(|tile| source_tile_decodes(r, scene, *tile));
}

fn source_tile_decodes(r: &WgpuRasterizer, scene: SceneView<'_>, tile: [u32; 2]) -> bool {
    let sources = r.source_tiles.borrow();
    let space = r.document_color().space;
    let resident = |id: SourceTarget| {
        r.paint_layers.iter().any(|l| l.id == id && l.pages.iter().any(|p| p.coordinate == tile))
    };
    scene.order().iter().any(|&h| {
        if !scene.visible(h) { return false; }
        if scene.object_layer(h).is_some() { return true; }
        let Some(target) = scene.source_target(h) else { return false; };
        let placed = scene.target_offset(target) != [0; 2];
        let source = scene.paint_base(target).is_some_and(|base| placed
            || (source_access::paint_base_contains(base, tile)
                && !resident(target) && sources.prepared_base_view(base, tile).is_none()));
        let native = r.native_backing(target).is_some() || scene.mask(h).is_some_and(|(use_, _)| r.native_backing(SourceTarget::Coverage(use_.source)).is_some());
        source || native && (placed || scene.mask(h).is_some() || (!resident(target)
            && r.native_color_tile(target, tile).map_or(true, |blob| blob.is_some_and(|blob| sources.prepared_raster_view(&blob, space, space).is_none()))))
    })
}

impl WgpuRasterizer {
    pub(super) fn ensure_exact_preview(&mut self, encoder: &mut crate::submission::CommandEncoder) -> Result<(), GpuRasterError> {
        if self.preview_level == 0 { return Ok(()); }
        let frame = self.artwork_frame.clone().ok_or(GpuRasterError::InvalidExtent)?;
        let packet = frame.packet(self.document_extent);
        self.validate_and_prepare_brush_resources(packet.dab_batches)?;
        let mut tiles = packet.dab_batches.iter().map(|batch| {
            let dabs = &packet.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize];
            brush_tiles::plan(batch, dabs, self.target_extent(batch.target))
        }).collect::<Vec<_>>();
        let quality=scene::PreviewQuality {level:self.preview_level,contribution:self.preview_contribution,requires_base:self.preview_requires_base};
        let declaration=self.preview_declaration.clone();
        let previous=self.brush_inputs.take();
        let result=(|| {
            self.preview_level=0;self.preview_contribution=false;self.preview_requires_base=true;
            if !self.prepare_raster_outputs(packet,&tiles) {return Err(GpuRasterError::NotReady);}
            if let Some(error)=self.take_restore_failure() {return Err(error);}
            self.prepare_preview_outputs(0)?;
            self.brush_inputs=Some(self.prepare_brush_inputs(self.preview_layer_id,None)?);
            self.brush_inputs.as_ref().unwrap().clone().track(encoder);
            self.prepare_uploads(packet, &mut tiles, encoder)?;
            let plans=self.preview_material_plans(packet,&tiles,true)?;
            self.prepare_preview_declaration(packet,scene::PreviewQuality {level:0,contribution:false,requires_base:true},
                &plans,true,None,&encoder.submission())?;
            for (index, batch) in packet.dab_batches.iter().enumerate() {
                self.encode_brush_batch(encoder, index, batch, BrushEncodingContext {source:packet.scene,
                    batches: packet.dab_batches, dabs: packet.dabs, tiles: &tiles[index],
                        target_extent: self.target_extent(batch.target),
                    target: BrushEncodingTarget::Preview { from_persistent: true },
                })?;
            }
            Ok::<_,GpuRasterError>(())
        })();
        self.brush_inputs=previous;
        if result.is_err() {
            self.preview_level=quality.level;self.preview_contribution=quality.contribution;self.preview_requires_base=quality.requires_base;
            self.preview_declaration=declaration;
        }
        result
    }
}

#[cfg(test)]
mod tests;
