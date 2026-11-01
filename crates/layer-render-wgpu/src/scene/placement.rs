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
    transform: layer_core::ImageTransform,
    taps: u32,
    background: f32,
    sources: Vec<([u32; 2], wgpu::TextureView)>,
    source_size: [u32; 2],
    positions: Option<wgpu::TextureView>,
}

impl Scene {
    pub(crate) const MATERIAL_CACHE_PAGES: usize = 64;

    pub(super) fn mask_at(&mut self, r: &WgpuRasterizer, mask: &MaskUse, source: &CoverageSource, offset: [i64; 2], tile: [u32; 2]) -> Result<usize, GpuRasterError> {
        self.mask_tile_input(r, mask, source, offset, tile, None)
    }

    pub(super) fn command_mask_at(&mut self, r: &WgpuRasterizer, coverage: &layer_core::CoverageSnapshot, tile: [u32; 2], command: layer_masks::CommandCoverage) -> Result<ColorInput, GpuRasterError> {
        if coverage.use_.offset == [0; 2] && !coverage.use_.inverted {
            if let Some(page) = r.layer_masks.command_pages.get(&(command, tile)) {
                return Ok(ColorInput { view: page.view.clone(), lease: None });
            }
            if coverage.source.default_coverage == 0. {
                return Ok(ColorInput { view: r.empty_scalar_view.clone(), lease: None });
            }
        }
        let index = self.mask_tile_input(r, &coverage.use_, &coverage.source, coverage.use_.offset, tile, Some(command))?;
        Ok(ColorInput { view: self.pool[index].view.clone(), lease: Some(self.own(index).into_owner()) })
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

    #[expect(clippy::too_many_arguments, reason = "Placed-plane jobs keep transform, output domain, and generic tile source explicit")]
    fn placed_plane_jobs(
        &mut self, r: &WgpuRasterizer, transform: layer_core::ImageTransform,
        bounds: PixelRect, tile: [u32; 2], background: f32, scalar: bool, offset: [i64; 2],
        mut source: impl FnMut(&mut Self, [u32; 2]) -> Result<(Option<ColorInput>, Option<usize>), GpuRasterError>,
    ) -> Result<usize, GpuRasterError> {
        let exact = pixel_transform::exact_taps(&transform);
        let taps = if self.placement_display { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact };
        let mesh = self.mesh_geometry(&transform);
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
        let mut pieces = Vec::new();
        Splitter::new(bounds, &transform, mesh, |c| !page_rect(c).intersect(bounds).is_empty())?
            .placed().shifted(offset).split(page_rect(tile), &mut pieces)?;
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

    pub(crate) fn capture_raw_tile(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, coordinate: [u32; 2],
        plan: &layer_core::TransformPixelsPlan, planes: &[layer_core::raster::RasterPlane], encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(Vec<(layer_core::raster::RasterPlane, layer_core::raster::RasterTile)>, crate::raster::NativeCapture), GpuRasterError> {
        use layer_core::raster::{RasterPlane, RasterTile};
        let target = plan.paint.map(SourceTarget::Paint).unwrap_or(plan.target);
        let geometry = &plan.geometry;
        self.clear_material_pages();
        self.placement_display = false;
        let owner = packet.scene.source_owner(target).ok_or(GpuRasterError::MissingPaintLayer(target))?;
        let extent = packet.scene.target_extent(target);
        let mut inputs = Vec::new();
        let mut tiles = Vec::new();
        let mut pages = Vec::with_capacity(planes.len());
        for &plane in planes {
            let (mut transform, bounds) = if plane == RasterPlane::Mask {
                let (mask, source) = packet.scene.mask(owner).unwrap();
                let coverage = SourceTarget::Coverage(mask.source);
                (plan.plane_geometry(coverage), pixel_rect(plan.plane_domain(coverage), source.domain))
            } else { (geometry.clone(), pixel_rect(plan.plane_domain(target), extent)) };
            transform.placement.interpolation = geometry.placement.interpolation;
            let page = self.placed_raw_plane(r, packet.scene, target, transform, [0; 2], coordinate, plane, bounds)?;
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
        transform: layer_core::ImageTransform, offset: [i64; 2], tile: [u32; 2], plane: layer_core::raster::RasterPlane,
        bounds: PixelRect,
    ) -> Result<usize, GpuRasterError> {
        use layer_core::raster::RasterPlane;
        let stored = r.paint_layers.iter().find(|stored| stored.id == target);
        let mask = scene.source_owner(target).and_then(|owner| scene.mask(owner));
        let background = if plane == RasterPlane::Mask { mask.unwrap().1.default_coverage } else { 0. };
        let source = if plane == RasterPlane::Mask { SourceTarget::Coverage(mask.unwrap().0.source) } else { target };
        let scene_view = scene;
        self.placed_plane_jobs(r, transform, bounds, tile, background, plane != RasterPlane::Color, offset, |scene, c| {
            if plane == RasterPlane::Color {
                let preview = r.preview_layer_id == Some(target) && !r.preview_damage.intersect(page_rect(c)).is_empty();
                let inputs = scene.color_inputs(r, scene_view, target, stored, c, preview)?;
                return match inputs {
                    [Some(base), Some(flow)] => {
                        let page = scene.alloc(r, wgpu::Color::TRANSPARENT)?;
                        for input in [base, flow] {
                            scene.draw(r, page, input.view, None, [0., 0., 256., 256.], [1., 1., 0., 0.], true, Convert::None);
                        }
                        Ok((Some(ColorInput { view: scene.pool[page].view.clone(), lease: None }), Some(page)))
                    }
                    [Some(input), None] | [None, Some(input)] => Ok((Some(input), None)),
                    [None, None] => Ok((None, None)),
                };
            }
            let view = if plane == RasterPlane::Mask { r.layer_masks.pages.get(&(source, c)).map(|page| &page.view) }
            else { stored.and_then(|stored| match plane {
                RasterPlane::WatercolorWetness => r.preview_watercolor_wetness_pages.iter()
                    .find(|p| r.preview_layer_id == Some(target) && p.coordinate == c && !r.preview_damage.intersect(page_rect(c)).is_empty())
                    .or_else(|| stored.watercolor_wetness_pages.iter().find(|p| p.coordinate == c)).map(|p| &p.active().view),
                _ => None,
            }) };
            if let Some(view) = view { return Ok((Some(ColorInput { view: view.clone(), lease: None }), None)); }
            Ok((scene.cold_plane(r, source, plane, c)?, None))
        })
    }

    pub(super) fn cold_plane(&mut self, r: &WgpuRasterizer, id: SourceTarget, plane: layer_core::raster::RasterPlane, c: [u32; 2]) -> Result<Option<ColorInput>, GpuRasterError> {
        let Some(blob) = r.native_plane_tile(id, plane, c)? else { return Ok(None); };
        let space = r.document_color().space;
        let (tile, pending) = r.source_tiles.borrow_mut().plan_raster(r, &blob, space, space)?;
        if let Some(pending) = pending { self.enqueue_source_decode(pending); }
        let lease = r.source_tiles.borrow().lease(&tile.view);
        Ok(Some(ColorInput { view: tile.view, lease }))
    }

    pub(super) fn mask_page(&mut self, r: &WgpuRasterizer, id: SourceTarget, tile: [u32; 2]) -> Result<Option<wgpu::TextureView>, GpuRasterError> {
        if let Some(page) = r.layer_masks.pages.get(&(id, tile)) { return Ok(Some(page.view.clone())); }
        if !r.snapshot_worker { return Ok(None); }
        Ok(self.cold_plane(r, id, layer_core::raster::RasterPlane::Mask, tile)?.map(|input| input.view))
    }

    /// Document page `tile` of a paint layer whose pixel (0, 0) lies `offset`
    /// pixels further on, as its color and, while a stroke previews a flow
    /// over that color, the flow.
    pub(super) fn placed_color(
        &mut self, r: &WgpuRasterizer, scene: SceneView<'_>, target: SourceTarget, offset: [i64; 2], tile: [u32; 2],
    ) -> Result<(usize, Option<usize>), GpuRasterError> {
        let extent = scene.target_extent(target);
        let bounds = PixelRect::full(extent);
        let stored = r.paint_layers.iter().find(|stored| stored.id == target);
        let read = DocRect::from(page_rect(tile)).translated(offset.map(|v| -v)).in_frame(extent);
        let flows = r.preview_layer_id == Some(target) && !r.preview_requires_base
            && page_coordinates(read).any(|c| !r.preview_damage.intersect(page_rect(c)).is_empty() && r.preview_page(c).is_some());
        let plane = |this: &mut Self, flow: bool| this.placed_plane_jobs(r, Default::default(), bounds, tile, 0., false, offset, |this, c| {
            let preview = r.preview_layer_id == Some(target) && !r.preview_damage.intersect(page_rect(c)).is_empty();
            let [color, overlay] = this.color_inputs(r, scene, target, stored, c, preview)?;
            Ok((if flow { overlay } else { color }, None))
        });
        let color = plane(self, false)?;
        let color = self.own(color);
        let flow = flows.then(|| plane(self, true)).transpose()?;
        Ok((self.keep(color), flow))
    }

    pub(super) fn placed_material_tile(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
        offset: [i64; 2], tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        self.placed_material_inputs(r, packet, handle, offset, tile).map(|(page, _)| page)
    }

    pub(super) fn placed_material_inputs(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, handle: OccurrenceHandle,
        offset: [i64; 2], tile: [u32; 2],
    ) -> Result<(usize, wgpu::TextureView), GpuRasterError> {
        use layer_core::raster::RasterPlane;
        let target = packet.scene.source_target(handle).unwrap();
        let (bounds, radius) = self.material_coverage(r, target, offset, packet.dab_batches);
        if bounds.expand(radius).intersect(page_rect(tile).into()).is_empty() {
            let page = self.placed_raw_plane(r, packet.scene, target, Default::default(), offset, tile, RasterPlane::Color, PixelRect::full(packet.scene.local_extent(handle)))?;
            return Ok((page, self.pool[page].view.clone()));
        }
        let mut pages = vec![r.empty_scalar_view.clone();10];
        for (neighbor,c) in [[1,1],[0,1],[2,1],[1,0],[1,2]].into_iter().enumerate() {
            for (plane_index,plane) in [RasterPlane::Color,RasterPlane::WatercolorWetness].into_iter().enumerate() {
                pages[plane_index*5+neighbor] = self.material_page(r, packet, handle, offset, tile, c, plane)?;
            }
        }
        let binding = views_group(&r.device, "placed material neighborhood", &r.watercolor_layout,
            pages.iter());
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        let held = self.own(out);
        self.jobs.push(Job::Watercolor {
            coordinates: (self.material_coordinates.clone(), 0), target: self.pool[out].view.clone(), binding,
            record: *r.layer_style_records.get(&handle).ok_or(GpuRasterError::MissingPaintLayer(target))?,
        });
        Ok((self.keep(held), pages[0].clone()))
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
        offset: [i64; 2], tile: [u32; 2], neighbor: [u32; 2],
        plane: layer_core::raster::RasterPlane,
    ) -> Result<wgpu::TextureView, GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
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
        let shift = std::array::from_fn(|i| offset[i] + i64::from(PAGE_SIZE) - i64::from(tile[i] * PAGE_SIZE));
        let page = self.placed_raw_plane(r, packet.scene, target, Default::default(), shift, neighbor, plane, PixelRect::full(packet.scene.local_extent(handle)))?;
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
    ) -> Result<usize, GpuRasterError> {
        let target = packet.scene.source_target(handle).unwrap();
        let out = self.alloc(r, wgpu::Color::TRANSPARENT)?;
        let held = self.own(out);
        let stored = r.paint_layers.iter().find(|l| l.id == target);
        self.paint_page(r, packet, handle, stored, c, out, [0., 0., 256., 256.], Convert::None)?;
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
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut scene = Scene::new(&r).unwrap();
        let command = (SourceTarget::default(), 0, false);
        for value in [0., 0.0001, 1. / 255., 0.12345, 0.5, 0.9995, 1.] {
            let page = layer_masks::MaskPage::new(&r.device);
            scene.jobs.push(Job::Clear(page.view.clone(), wgpu::Color { r:value, g:value, b:value, a:value }));
            r.layer_masks.command_pages.insert((command, [0; 2]), page.clone());
            for (offset, inverted, tile, default) in [
                ([0; 2], false, [0; 2], 0.), ([8, 0], false, [0; 2], 0.),
                ([0; 2], true, [0; 2], 0.), ([0; 2], false, [1, 0], 0.),
                ([0; 2], false, [1, 0], 0.7),
            ] {
                let mut coverage = layer_core::CoverageSnapshot::reveal_all(Default::default(), [512, 256], offset);
                coverage.use_.inverted = inverted;
                coverage.source.default_coverage = default;
                let before = scene.pool.len();
                let input = scene.command_mask_at(&r, &coverage, tile, command).unwrap();
                if offset == [0; 2] && !inverted && (tile == [0; 2] || default == 0.) {
                    assert_eq!(scene.pool.len(), before);
                    assert!(input.lease.is_none());
                }
                let pooled = scene.mask_tile_input(&r, &coverage.use_, &coverage.source, offset, tile, Some(command)).unwrap();
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
