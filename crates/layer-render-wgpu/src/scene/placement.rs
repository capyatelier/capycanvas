//! Map a layer's pages onto document pages through its whole-pixel offset, or
//! through a Transform being applied to its pixels.
use super::*;
use paint_transform::snapshot::Splitter;
use pixel_transform::{PlacementDraw, TiledTransformRecord, TransformTile};

pub(super) struct MaterialPage {
    layer: SourceTarget,
    offset: [i64; 2],
    coordinate: [i32; 2],
    plane: layer_core::raster::RasterPlane,
    page: scratch::Lease,
}

#[derive(Clone)]
pub(super) struct PlacementJob {
    pub scalar: bool,
    clear: bool,
    target: wgpu::TextureView,
    origin: [i64; 2],
    region: PixelRect,
    bounds: PixelRect,
    pub(super) transform: layer_core::ImageTransform,
    pub(super) taps: u32,
    background: f32,
    sources: Vec<([u32; 2], wgpu::TextureView)>,
    source_size: [u32; 2],
    positions: Option<wgpu::TextureView>,
}

impl PlacementJob {
    pub(super) fn work(&self)->u64 {self.region.area().saturating_mul(u64::from(self.taps.max(1)))}
    pub(super) fn source_views(&self) -> impl Iterator<Item = &wgpu::TextureView> {
        self.sources.iter().map(|(_, view)| view).chain(self.positions.iter())
    }
    pub(super) fn reads(&self, texture: &wgpu::Texture) -> bool {
        self.source_views().any(|view| view.texture() == texture)
    }
}

#[derive(Clone)]
pub(crate) struct Placement {
    pub transform:layer_core::ImageTransform,
    pub mesh:Option<Arc<paint_transform::mesh::MeshGeometry>>,
    pub bounds:PixelRect,
    pub offset:[i64;2],
    pub(in crate::scene) frame:scale::SourceFrame,
    scene_offset:[f64;2],
}
impl Placement {
    pub fn identity(bounds:PixelRect,offset:[i64;2])->Arc<Self> {
        Arc::new(Self {transform:Default::default(),mesh:None,bounds,offset,frame:scale::SourceFrame::at(offset,[0;2]),scene_offset:[0.;2]})
    }
    pub fn capture(plan:&layer_core::TransformPixelsPlan,scene:SceneView<'_>,planes:&[RasterPlane])->Vec<(RasterPlane,Arc<Self>)> {
        let target=plan.paint.map(SourceTarget::Paint).unwrap_or(plan.target);
        planes.iter().map(|&plane| {
            let source=if plane==RasterPlane::Mask {plan.coverage.map(SourceTarget::Coverage).unwrap_or(plan.target)}else {target};
            let mut transform=plan.plane_geometry(source);
            transform.placement.interpolation=plan.geometry.placement.interpolation;
            let bounds=pixel_rect(plan.plane_domain(source),scene.target_extent(source));
            let mesh=transform.placement.mesh.as_ref().map(|mesh|Arc::new(paint_transform::mesh::MeshGeometry::new(mesh,transform.placement.outer,None)));
            (plane,Arc::new(Self {transform,mesh,bounds,offset:[0;2],frame:scale::SourceFrame::at([0;2],[0;2]),scene_offset:[0.;2]}))
        }).collect()
    }
    pub(in crate::scene) fn source(frame:scale::SourceFrame,extent:[u32;2])->Arc<Self> {
        Arc::new(Self {transform:Default::default(),mesh:None,bounds:PixelRect::full(extent),offset:frame.origin(),frame,scene_offset:[0.;2]})
    }
    pub fn packet<'a>(&self,packet:FramePacket<'a>)->FramePacket<'a> {
        FramePacket {scene:packet.scene.with_offset64(self.scene_offset),..packet}
    }
    pub fn material_window(&self,tile:[u32;2])->Self {
        Self {offset:std::array::from_fn(|i|self.offset[i]+i64::from(PAGE_SIZE)-i64::from(tile[i]*PAGE_SIZE)),..self.clone()}
    }
    pub fn material_view(&self,origin:[i64;2])->Arc<Self> {
        Arc::new(Self {offset:std::array::from_fn(|i|self.offset[i]-origin[i]),
            frame:scale::SourceFrame {shift:std::array::from_fn(|i|self.frame.shift[i]-origin[i]),..self.frame},scene_offset:[0.;2],..self.clone()})
    }
    pub fn matches(&self,other:&Self)->bool {
        self.transform==other.transform && self.bounds==other.bounds && self.offset==other.offset
            && self.frame==other.frame && self.scene_offset==other.scene_offset
            && match (&self.mesh,&other.mesh) {(None,None)=>true,(Some(a),Some(b))=>Arc::ptr_eq(a,b),_=>false}
    }
    pub fn footprints(&self,region:PixelRect)->Result<Vec<paint_transform::snapshot::Footprint>,GpuRasterError> {
        self.footprints_where(region,|_|true)
    }
    pub fn footprints_where(&self,region:PixelRect,contains:impl Fn([u32;2])->bool)->Result<Vec<paint_transform::snapshot::Footprint>,GpuRasterError> {
        let mut pieces=Vec::new();
        Splitter::new(self.bounds,&self.transform,self.mesh.clone(),|coordinate|!page_rect(coordinate).intersect(self.bounds).is_empty() && contains(coordinate))?
            .placed().shifted(self.offset).split(region,&mut pieces)?;
        Ok(pieces)
    }
}

impl Scene {
    pub(super) fn raw_placement(&self,transform:layer_core::ImageTransform,bounds:PixelRect,offset:[i64;2])->Arc<Placement> {
        let mesh=self.mesh_geometry(&transform);
        Arc::new(Placement {transform,mesh,bounds,offset,frame:scale::SourceFrame::at(offset,[0;2]),scene_offset:[0.;2]})
    }
    pub(super) fn reduced_placement(&self,packet:FramePacket<'_>,id:SourceTarget,phase:[u32;2])->Arc<Placement> {
        let origin=packet.scene.target_offset(id);
        Arc::new(Placement {transform:Default::default(),mesh:None,bounds:PixelRect::full(packet.scene.target_extent(id)),
            offset:phase.map(i64::from),frame:scale::SourceFrame::at(origin,phase),scene_offset:std::array::from_fn(|i|(i64::from(phase[i])-origin[i]) as f64)})
    }
    pub(crate) const MATERIAL_CACHE_PAGES: usize = 64;

    pub(super) fn mask_at(&mut self, r: &WgpuRasterizer, mask: &MaskUse, source: &CoverageSource, offset: [i64; 2], tile: [u32; 2], accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        self.mask_tile_input(r, mask, source, offset, tile, None, accepted)
    }

    pub(super) fn command_mask_at(&mut self, r: &WgpuRasterizer, coverage: &layer_core::CoverageSnapshot, tile: [u32; 2], command: layer_masks::CommandCoverage, accepted:&mut sources::AcceptedInputs) -> Result<ColorInput, GpuRasterError> {
        if coverage.use_.offset == [0; 2] && !coverage.use_.inverted {
            if let Some(page) = r.layer_masks.command_pages.get(&(command, tile)) {
                let name=crate::raster::contents::command_mask(&coverage.source,coverage.selection.as_ref(),tile)?;
                if let Ok(read)=page.read(name,self.submission.as_ref()) {return Ok(ColorInput { view: read.value().view.clone(), lease: None });}
            }
            if coverage.source.default_coverage == 0. && !layer_masks::command_page_needed(coverage,command.2,tile)
                && (coverage.selection.is_some() || r.mask_data(SourceTarget::Coverage(coverage.use_.source),&coverage.source)?.tiles.get(&layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Mask,coordinate:tile}).is_none()) {
                return Ok(ColorInput { view: r.empty_scalar_view.clone(), lease: None });
            }
        }
        let index = self.mask_tile_input(r, &coverage.use_, &coverage.source, coverage.use_.offset, tile, Some((command,coverage)), accepted)?;
        Ok(ColorInput { view: self.pool[index].view.clone(), lease: Some(self.own(index).into_owner().into()) })
    }

    pub(crate) fn mesh_geometry(&self, transform: &layer_core::ImageTransform) -> Option<Arc<paint_transform::mesh::MeshGeometry>> {
        transform.placement.mesh.as_ref()?;
        let mut cache=self.mesh_geometry.borrow_mut();
        if let Some(index)=cache.iter().position(|(key,_)| key == &transform.placement) {
            let entry=cache.remove(index);let geometry=entry.1.clone();cache.push(entry);return Some(geometry);
        }
        let geometry = Arc::new(paint_transform::mesh::MeshGeometry::new(transform.placement.mesh.as_ref().unwrap(), transform.placement.outer, None));
        while !cache.is_empty() && cache.iter().map(|(_,geometry)|geometry.storage_bytes()).sum::<u64>()+geometry.storage_bytes()>64*1024*1024 {cache.remove(0);}
        if geometry.storage_bytes()<=64*1024*1024 {cache.push((transform.placement.clone(), geometry.clone()));}
        Some(geometry)
    }

    fn placed_plane_jobs(
        &mut self, r: &WgpuRasterizer, placement:&Placement,
        tile: [u32; 2], background: f32, scalar: bool,
        mut source: impl FnMut(&mut Self, [u32; 2]) -> Result<(Option<ColorInput>, Option<usize>), GpuRasterError>,
    ) -> Result<usize, GpuRasterError> {
        let Placement {transform,mesh,bounds,offset,..}=placement;
        let (bounds,offset)=(*bounds,*offset);
        let exact = pixel_transform::exact_taps(transform);
        let taps = if self.placement_display { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact };
        let positions = mesh.as_ref().map(|geometry| {
            let view = self.positions.view(&r.device, [PAGE_SIZE; 2]);
            let key = (transform.placement.clone(), tile);
            if self.position_key.as_ref() != Some(&key) {
                self.jobs.push(Job::Positions(geometry.clone(), tile));
                self.position_key = Some(key);
            }
            view
        });
        let origin = std::array::from_fn(|axis| i64::from(tile[axis] * PAGE_SIZE) - offset[axis]);
        let pieces=placement.footprints(page_rect(tile))?;
        let out = self.reserve_format(r, scalar)?;
        let held = self.own(out);
        if pieces.is_empty() {
            let color = f64::from(background);
            self.jobs.push(Job::Clear(self.pool[out].view.clone(), wgpu::Color { r:color,g:color,b:color,a:color }));
        }
        for (index, piece) in pieces.into_iter().enumerate() {
            let mut sources = Vec::with_capacity(piece.sources.len());
            let mut scratch = Vec::with_capacity(piece.sources.len());
            let mut leases = Vec::with_capacity(piece.sources.len());
            for c in piece.sources {
                let first = self.jobs.len();
                let (input, page) = source(self, c)?;
                if first < self.jobs.len() {self.source_jobs.push(first..self.jobs.len());}
                if let Some(ColorInput { view, lease }) = input { sources.push((c, view)); leases.extend(lease); }
                scratch.extend(page.map(|page| self.own(page)));
            }
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                scalar,
                clear: index == 0,
                target: self.pool[out].view.clone(),
                origin,
                region: piece.region.page_local(tile),
                bounds,
                transform: transform.clone(),
                taps,
                background,
                sources,
                source_size: [PAGE_SIZE; 2],
                positions: positions.clone(),
            })));
            drop(scratch);
        }
        Ok(self.keep(held))
    }

    pub(crate) fn declare_raw_capture(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,placements:&[(RasterPlane,Arc<Placement>)],coordinate:[u32;2])->Result<sources::Inputs,GpuRasterError> {
        let declaration=self.capture_declaration(r,packet)?;
        self.declare(r,Default::default(),declaration,|scene,r,accepted| {
            scene.placement_display=false;
            let mut pages=Vec::with_capacity(placements.len());
            for (plane,placement) in placements {let page=scene.placed_raw_plane(r,packet.scene,target,placement,coordinate,*plane,accepted)?;pages.push(scene.own(page));}
            Ok(())
        })
    }
    pub(crate) fn capture_raw_tile(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, coordinate: [u32; 2],
        plan: &layer_core::TransformPixelsPlan, placements:&[(RasterPlane,Arc<Placement>)], encoder: &mut crate::submission::CommandEncoder,
     accepted:&mut sources::AcceptedInputs) -> Result<(Vec<(layer_core::raster::RasterPlane, layer_core::raster::RasterTile)>, crate::raster::NativeCapture), GpuRasterError> {
        use layer_core::raster::RasterTile;
        #[cfg(test)]
        let _record=sources::record_reads(accepted);
        let target = plan.paint.map(SourceTarget::Paint).unwrap_or(plan.target);
        self.clear_material_pages();
        self.placement_display = false;
        let mut inputs = Vec::new();
        let mut tiles = Vec::new();
        let mut pages = Vec::with_capacity(placements.len());
        for (plane,placement) in placements {
            let plane=*plane;
            let page = self.placed_raw_plane(r, packet.scene, target, &placement, coordinate, plane, accepted)?;
            let held = self.own(page);
            self.encode_jobs(r, encoder)?;
            let tile = RasterTile::pending(plane.descriptor_for(r.document_color(), packet.scene.color_mode(target)));
            inputs.push((self.pool[page].texture.clone(), tile.clone(), packet.scene.color_mode(target)));
            tiles.push((plane, tile));
            pages.push(held);
        }
        let capture = r.encode_private_tiles(inputs, encoder)?;
        Ok((tiles, capture))
    }

    /// Place `tile` of one raw plane through `transform`, `offset` pixels
    /// further on, reading its `bounds` pixels.
    #[expect(clippy::too_many_arguments, reason = "Raw-plane mapping keeps source bounds, geometry, offset and raster plane explicit")]
    pub(super) fn placed_raw_plane(
        &mut self, r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget,
        placement:&Placement, tile: [u32; 2], plane: layer_core::raster::RasterPlane,
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        use layer_core::raster::RasterPlane;
        let mask = scene.source_owner(target).and_then(|owner| scene.mask(owner));
        let background = if plane == RasterPlane::Mask { mask.unwrap().1.default_coverage } else { 0. };
        let source = if plane == RasterPlane::Mask { SourceTarget::Coverage(mask.unwrap().0.source) } else { target };
        self.placed_plane_jobs(r, placement, tile, background, plane != RasterPlane::Color, |this, c| {
            if plane == RasterPlane::Color {
                let inputs = this.color_inputs(r,scene,target,c,accepted)?;
                return match inputs {
                    [Some(base), Some(flow)] => {
                        let page = this.alloc(r, wgpu::Color::TRANSPARENT)?;
                        for input in [base, flow] {
                            this.draw(r, page, input.view, None, [0., 0., 256., 256.], [1., 1., 0., 0.], true, Convert::None);
                        }
                        Ok((Some(ColorInput { view: this.pool[page].view.clone(), lease: None }), Some(page)))
                    }
                    [Some(input), None] | [None, Some(input)] => Ok((Some(input), None)),
                    [None, None] => Ok((None, None)),
                };
            }
            let view=if plane==RasterPlane::Mask {this.mask_input(r,source,mask.unwrap().1,c,accepted)?}else {this.scalar_input(r,scene,source,plane,c,accepted)?};
            Ok((view.map(|view|ColorInput {view,lease:None}),None))
        })
    }

    pub(super) fn mask_page(&mut self,r:&WgpuRasterizer,id:SourceTarget,source:&CoverageSource,tile:[u32;2],accepted:&mut sources::AcceptedInputs)->Result<Option<wgpu::TextureView>,GpuRasterError> {
        self.mask_input(r,id,source,tile,accepted)
    }

    /// Document page `tile` of a paint layer whose pixel (0, 0) lies `offset`
    /// pixels further on, as its color and, while a stroke previews a flow
    /// over that color, the flow.
    pub(super) fn placed_color(
        &mut self, r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, placement:&Placement, tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<(usize, Option<usize>), GpuRasterError> {
        let flows=placement.footprints(page_rect(tile))?.iter().flat_map(|piece|&piece.sources)
            .try_fold(false,|present,&c|Ok::<_,GpuRasterError>(present|accepted.color(r,scene,target,c)?[1].is_some()))?;
        let mut plane = |this: &mut Self, flow: bool| this.placed_plane_jobs(r, placement, tile, 0., false, |this, c| {
            let [color, overlay] = this.color_inputs(r,scene,target,c,accepted)?;
            Ok((if flow { overlay } else { color }, None))
        });
        let color = plane(self, false)?;
        let color = self.own(color);
        let flow = flows.then(|| plane(self, true)).transpose()?;
        Ok((self.keep(color), flow))
    }

    pub(super) fn placed_material_tile(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
        placement:&Placement, tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        self.placed_material_inputs(r, packet, handle, placement, tile, accepted).map(|(page, _)| page)
    }

    pub(super) fn placed_material_inputs(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
        placement:&Placement, tile: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<(usize, wgpu::TextureView), GpuRasterError> {
        use layer_core::raster::RasterPlane;
        let offset=placement.offset;
        let target = packet.scene.source_target(handle).unwrap();
        let (bounds, radius) = self.material_coverage(r, target, offset, packet.dab_batches);
        if bounds.expand(radius).intersect(page_rect(tile).into()).is_empty() {
            let page = self.placed_raw_plane(r, packet.scene, target, placement, tile, RasterPlane::Color, accepted)?;
            return Ok((page, self.pool[page].view.clone()));
        }
        let mut pages = vec![r.empty_scalar_view.clone();10];
        for (neighbor,c) in [[1,1],[0,1],[2,1],[1,0],[1,2]].into_iter().enumerate() {
            for (plane_index,plane) in [RasterPlane::Color,RasterPlane::WatercolorWetness].into_iter().enumerate() {
                pages[plane_index*5+neighbor] = self.material_page(r, packet, handle, placement, tile, c, plane, accepted)?;
            }
        }
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        let held = self.own(out);
        let color=pages[0].clone();
        self.jobs.push(Job::Watercolor {
            coordinates: (self.material_coordinates.clone(), 0), target: self.pool[out].view.clone(), views: pages,
            record: *r.layer_style_records.get(&handle).ok_or(GpuRasterError::MissingPaintLayer(target))?,
        });
        Ok((self.keep(held), color))
    }

    pub(super) fn material_coverage(&mut self, r: &WgpuRasterizer, id: SourceTarget, offset: [i64; 2], batches: &[DabBatch]) -> (DocRect, u32) {
        let raw = *self.material_bounds.entry(id).or_insert_with(|| material_bounds(r, id));
        let radius = r.watercolor_style(id, batches).map_or(0, |style| style.radius());
        (raw.translated(offset), radius)
    }

    pub(super) fn clear_material_pages(&mut self) {
        self.material_bounds.clear();
        self.position_key = None;
        self.material_pages.clear();
    }

    #[expect(clippy::too_many_arguments, reason = "Material sampling keeps offset, tile, neighbor, and raster plane explicit")]
    fn material_page(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
        placement:&Placement, tile: [u32; 2], neighbor: [u32; 2],
        plane: layer_core::raster::RasterPlane,
     accepted:&mut sources::AcceptedInputs) -> Result<wgpu::TextureView, GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
        let offset=placement.offset;
        let coordinate = std::array::from_fn(|i| tile[i] as i32 + neighbor[i] as i32 - 1);
        if plane == layer_core::raster::RasterPlane::WatercolorWetness || neighbor != [1, 1] {
            let origin = coordinate.map(|n| i64::from(n) * i64::from(PAGE_SIZE));
            let page = DocRect { min: origin, max: origin.map(|v| v + i64::from(PAGE_SIZE)) };
            if self.material_coverage(r, target, offset, packet.dab_batches).0.intersect(page).is_empty() {
                return Ok(if plane == layer_core::raster::RasterPlane::Color {
                    r.empty_view.clone()
                } else { r.empty_scalar_view.clone() });
            }
        }
        if let Some(index) = self.material_pages.iter().position(|entry| entry.layer == target
            && entry.offset == offset && entry.coordinate == coordinate && entry.plane == plane) {
            let entry = self.material_pages.remove(index).unwrap();
            let page = entry.page.index;
            self.material_pages.push_back(entry);
            return Ok(self.pool[page].view.clone());
        }
        if self.material_pages.len() == Self::MATERIAL_CACHE_PAGES {
            self.material_pages.pop_front();
        }
        let window=placement.material_window(tile);
        let page = self.placed_raw_plane(r, packet.scene, target, &window, neighbor, plane, accepted)?;
        let held = self.own(page);
        self.material_pages.push_back(MaterialPage { layer: target, offset, coordinate, plane, page: held });
        Ok(self.pool[page].view.clone())
    }

    /// A scratch tile of `layer`'s own pixels at its local `c`.
    pub(crate) fn local_color_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        handle: OccurrenceHandle,
        c: [u32; 2],
     accepted:&mut sources::AcceptedInputs) -> Result<usize, GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        let held = self.own(out);
        let stored = r.paint_layers.iter().find(|l| l.id == target);
        self.paint_page(r, packet, handle, stored, c, out, [0., 0., 256., 256.], Convert::None, accepted)?;
        Ok(self.keep(held))
    }
}

pub(super) fn material_bounds(r: &WgpuRasterizer, id: SourceTarget) -> DocRect {
    r.paint_layers.iter().filter(|stored| stored.id == id)
        .flat_map(|stored| &stored.watercolor_wetness_pages).map(|page| page.coordinate)
        .chain(r.preview_watercolor_wetness_pages.iter().filter(|_| r.preview_layer_id == Some(id)).map(|page| page.coordinate))
        .chain(r.native_backing(id).into_iter().flat_map(|data| data.tiles.keys())
            .filter(|key| key.plane == layer_core::raster::RasterPlane::WatercolorWetness).map(|key| key.coordinate))
        .fold(DocRect::default(), |bounds, coordinate| bounds.union(page_rect(coordinate).into()))
}

pub(super) fn prepare(
    pass: &mut pixel_transform::PixelTransform,
    r: &mut WgpuRasterizer,
    encoder: &mut crate::submission::CommandEncoder,
    job: &PlacementJob,
) -> Result<PlacementDraw, GpuRasterError> {
    let coordinates: Vec<_> = job.sources.iter().map(|(c, _)| *c).collect();
    let bounds = [job.bounds.min_x() as i32, job.bounds.min_y() as i32, job.bounds.width() as i32, job.bounds.height() as i32];
    let offset = pass
        .prepare_tiled(
            &r.device,
            &mut r.uploads,
            encoder,
            bounds,
            job.background,
            &job.transform,
            job.taps,
            &[TiledTransformRecord {
                origin: job.origin,
                sources: &coordinates,
                source_size: job.source_size,
                texels: [job.region.min_x(), job.region.min_y(), job.region.width(), job.region.height()],
                unmoved: false,
                clear: job.clear,
            }],
            None,
        )
        .map_err(GpuRasterError::InvalidTransform)?;
    let views: Vec<_> = job
        .sources
        .iter()
        .map(|(c, view)| TransformTile {
            view,
            origin: c.map(|v| (v * PAGE_SIZE) as i32),
            extent: job.source_size,
        })
        .collect();
    let source = pass
        .source_views(&r.device, &views, bounds, None, job.positions.as_ref(), &r.empty_view)
        .map_err(GpuRasterError::InvalidTransform)?;
    Ok(pass.placement_draw(&r.device, &job.target, source, offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_mask_reuse_preserves_fractional_coverage_and_placement() {
        let color=layer_core::color::DocumentColor {depth:layer_core::color::SampleDepth::F32,..Default::default()};
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        let mut scene = Scene::new(&r).unwrap();
        let command = (SourceTarget::default(), 0, false);
        for value in [0., 0.0001, 1. / 255., 0.12345, 0.5, 0.9995, 1.] {
            let code=(value as f64*f64::from(u16::MAX)).round() as u16;
            let value=f64::from(code)/f64::from(u16::MAX);
            let tile=layer_core::raster::RasterTile::backed(layer_core::raster::TileBlob::encode(
                layer_core::raster::RasterPlane::Mask.descriptor(color),&code.to_le_bytes().repeat((PAGE_SIZE*PAGE_SIZE) as usize)).unwrap());
            let data=layer_core::raster::RasterRevision::backed(layer_core::raster::RasterData {
                tiles:[(layer_core::raster::TileKey {plane:layer_core::raster::RasterPlane::Mask,coordinate:[0;2]},tile.clone())].into(),..Default::default()});
            let mut page = layer_masks::MaskPage::new(&r.device);
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            page.clear(&mut encoder,value,tile.content_version()).unwrap();r.last_submission=encoder.submit(&r.queue);
            r.layer_masks.command_pages.insert((command, [0; 2]), page.clone());
            for (offset, inverted, tile, default) in [
                ([0; 2], false, [0; 2], 0.), ([8, 0], false, [0; 2], 0.),
                ([0; 2], true, [0; 2], 0.), ([0; 2], false, [1, 0], 0.),
                ([0; 2], false, [1, 0], 0.7),
            ] {
                let mut coverage = layer_core::CoverageSnapshot::reveal_all(Default::default(), [512, 256], offset);
                coverage.use_.inverted = inverted;
                coverage.source.default_coverage = default;
                coverage.source.raster=data.clone();
                let before = scene.pool.len();
                let doc=crate::scene::scale::tests::document_at([512,256]);
                let packet=crate::test_support::packet(doc.scene(),[512,256]);
                let declaration=scene.capture_declaration(&r,packet).unwrap();
                let mut inputs=scene.accept_traversal(&mut r,Default::default(),declaration,|scene,r,accepted| {
                    scene.command_mask_at(r,&coverage,tile,command,accepted)?;
                    scene.mask_tile_input(r,&coverage.use_,&coverage.source,offset,tile,Some((command,&coverage)),accepted).map(drop)
                }).unwrap();
                let accepted=&mut inputs;
                let input = scene.command_mask_at(&r, &coverage, tile, command, accepted).unwrap();
                if offset == [0; 2] && !inverted && (tile == [0; 2] || default == 0.) {
                    assert_eq!(scene.pool.len(), before);
                    assert!(input.lease.is_none());
                }
                let pooled = scene.mask_tile_input(&r, &coverage.use_, &coverage.source, offset, tile, Some((command,&coverage)), accepted).unwrap();
                let _pooled = scene.own(pooled);
                let mut outputs = Vec::new();
                for mask in [scene.pool[pooled].view.clone(), input.view.clone()] {
                    let output = scene.alloc(&r, wgpu::Color::TRANSPARENT).unwrap();
                    let held = scene.own(output);
                    scene.draw(&r, output, mask, None, [0., 0., 256., 256.], [6., 1., 0., 0.], false, Convert::None);
                    if let Some(Job::Draw { data, .. }) = scene.jobs.last_mut() { data[12..16].copy_from_slice(&[0.2, 0.7, 0.4, 0.6]); }
                    outputs.push((scene.pool[output].texture.clone(), held));
                }
                let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
                pollster::block_on(scene.prepare_capture_work_async(&mut r)).unwrap();
                scene.encode_jobs(&mut r, &mut encoder).unwrap();
                r.uploads.finish(&encoder);encoder.submit(&r.queue);
                assert_eq!(crate::test_support::float_pixels(&r, &outputs[0].0), crate::test_support::float_pixels(&r, &outputs[1].0),
                    "coverage={value}, offset={offset:?}, inverted={inverted}, tile={tile:?}, default={default}");
                scene.jobs.clear();
                scene.used.iter_mut().for_each(scratch::Usage::reset);
            }
        }
    }
}
