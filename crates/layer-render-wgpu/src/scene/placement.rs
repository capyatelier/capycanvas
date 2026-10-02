//! Render placed content from immutable source and local paint tiles. The
//! compositor owns disposable output; accepting a pose never publishes raster.
use super::*;
use paint_transform::snapshot::Splitter;
use pixel_transform::{PlacementDraw, TiledTransformRecord, TransformTile};

pub(super) struct MaterialPage {
    layer: LayerId,
    geometry: layer_core::ImageTransform,
    interpolation: layer_core::Interpolation,
    coordinate: [i32; 2],
    plane: layer_core::raster::RasterPlane,
    page: usize,
}

#[derive(Clone)]
pub(super) struct PlacementJob {
    pub scalar: bool,
    clear: bool,
    target: wgpu::TextureView,
    tile: [u32; 2],
    region: PixelRect,
    extent: [u32; 2],
    transform: layer_core::ImageTransform,
    taps: u32,
    background: f32,
    sources: Vec<([u32; 2], wgpu::TextureView)>,
    source_size: [u32; 2],
    positions: Option<wgpu::TextureView>,
}

impl Scene {
    pub(crate) const MATERIAL_CACHE_PAGES: usize = 64;

    pub(super) fn mask_at(
        &mut self,
        r: &WgpuRasterizer,
        mask: &layer_core::LayerMask,
        geometry: layer_core::ImageTransform,
        extent: [u32; 2],
        tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        if let Some(affine) = geometry.as_affine().filter(|a| a.0[..4] == [1., 0., 0., 1.]) {
            return Ok(self.mask_tile(
                r,
                mask,
                layer_core::Point {
                    x: affine.0[4],
                    y: affine.0[5],
                },
                tile,
            ));
        }
        let background = if mask.inverted {
            1. - mask.default_coverage
        } else {
            mask.default_coverage
        };
        let transform = geometry;
        self.placed_jobs(r, transform, extent, tile, background, |scene, c| {
            Ok(scene.mask_tile(r, mask, layer_core::Point::default(), c))
        })
    }

    pub(super) fn placed_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        index: usize,
        tile: [u32; 2],
        source_level: u32,
    ) -> Result<usize, GpuRasterError> {
        let layer = &packet.layers[index];
        let extent = layer.local_extent(r.document_extent);
        let geometry = layer_core::target_geometry(packet.layers, layer.id);
        if r.paint_layers.iter().any(|stored| stored.id == layer.id && stored.watercolor.is_some()) {
            return self.placed_material_tile(r, packet, layer, geometry, tile);
        }
        if self.placement_display
            && let Some(affine) = geometry.as_affine()
            && source_level > 0
            && let Some((plan, view)) = self.scale_sources.sample(layer.id, source_level)
        {
            let scale = (1 << plan.level) as f32;
            let size = plan.size;
            let view = view.clone();
            let transform = layer_core::ImageTransform::affine(
                layer_core::Affine([scale, 0., 0., scale, plan.bounds.min_x() as f32, plan.bounds.min_y() as f32]).then(affine),
            );
            let out = self.reserve(r);
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                scalar: false,
                clear: true,
                target: self.pool[out].view.clone(),
                tile,
                region: PixelRect::full([PAGE_SIZE; 2]),
                extent: size,
                transform,
                taps: pixel_transform::PREVIEW_TAPS,
                background: 0.,
                sources: vec![([0, 0], view)],
                source_size: size,
                positions: None,
            })));
            return Ok(out);
        }
        let transform = geometry.clone();
        self.placed_jobs(r, transform, extent, tile, 0., |scene, c| scene.local_color_tile(r, packet, layer, c))
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

    /// Place `tile` of a layer `extent` pixels large through `transform`, one
    /// job per piece whose source pages `source` draws into scratch. Exact
    /// capture averages as many taps as a minified pixel covers, up to
    /// EXACT_TAPS per axis; the live display keeps the preview's count.
    fn placed_jobs(
        &mut self,
        r: &WgpuRasterizer,
        transform: layer_core::ImageTransform,
        extent: [u32; 2],
        tile: [u32; 2],
        background: f32,
        mut source: impl FnMut(&mut Self, [u32; 2]) -> Result<usize, GpuRasterError>,
    ) -> Result<usize, GpuRasterError> {
        self.placed_plane_jobs(r, transform, extent, tile, background, false, [0;2], |scene, c| {
            let page = source(scene, c)?;
            Ok((ColorInput { view: scene.pool[page].view.clone(), lease: None }, Some(page)))
        })
    }

    fn placed_plane_jobs(
        &mut self, r: &WgpuRasterizer, transform: layer_core::ImageTransform,
        extent: [u32; 2], tile: [u32; 2], background: f32, scalar: bool, offset: [i32;2],
        mut source: impl FnMut(&mut Self, [u32; 2]) -> Result<(ColorInput, Option<usize>), GpuRasterError>,
    ) -> Result<usize, GpuRasterError> {
        let bounds = PixelRect::full(extent);
        let exact = pixel_transform::exact_taps(&transform);
        let taps = if self.placement_display { exact.min(pixel_transform::PREVIEW_TAPS) } else { exact };
        let mesh = self.mesh_geometry(&transform);
        let positions = mesh.as_ref().map(|geometry| {
            let view = self.positions.view(&r.device, [PAGE_SIZE; 2]);
            let key = (transform.placement.clone(), tile, offset);
            if self.position_key.as_ref() != Some(&key) {
                self.jobs.push(Job::Positions(geometry.clone(), tile, offset));
                self.position_key = Some(key);
            }
            view
        });
        let mut pieces = Vec::new();
        Splitter::new(bounds, &transform, mesh, |c| !page_rect(c).intersect(bounds).is_empty())?
            .placed().shifted(offset).split(page_rect(tile), &mut pieces)?;
        let out = self.reserve_format(r, scalar);
        if pieces.is_empty() {
            let color = f64::from(background);
            self.jobs.push(Job::Clear(self.pool[out].view.clone(), wgpu::Color { r:color,g:color,b:color,a:color }));
        }
        for (index, piece) in pieces.into_iter().enumerate() {
            let mut sources = Vec::with_capacity(piece.sources.len());
            let mut scratch = Vec::with_capacity(piece.sources.len());
            let mut leases = Vec::with_capacity(piece.sources.len());
            for c in piece.sources {
                let (ColorInput { view, lease }, page) = source(self, c)?;
                sources.push((c, view));
                scratch.extend(page);
                leases.extend(lease);
            }
            self.jobs.push(Job::Placement(Box::new(PlacementJob {
                scalar,
                clear: index == 0,
                target: self.pool[out].view.clone(),
                tile,
                region: piece.region.page_local(tile),
                extent,
                transform: transform.clone(),
                taps,
                background,
                sources,
                source_size: [PAGE_SIZE; 2],
                positions: positions.clone(),
            })));
            for page in scratch {
                self.free(page);
            }
        }
        Ok(out)
    }

    pub(crate) fn capture_raw_tile(
        &mut self, r: &mut WgpuRasterizer, packet: FramePacket<'_>, coordinate: [u32; 2],
        geometry: &layer_core::ImageTransform, scope: layer_core::TransformPixelsScope, encoder: &mut crate::submission::CommandEncoder,
    ) -> Result<(Vec<(layer_core::raster::RasterPlane, layer_core::raster::RasterTile)>, crate::raster::NativeCapture), GpuRasterError> {
        use layer_core::raster::{RasterPlane, RasterTile};
        self.clear_material_pages();
        self.placement_display = false;
        let layer = &packet.layers[0];
        let extent = layer.properties.extent.ok_or(GpuRasterError::InvalidExtent)?;
        let planes: &[RasterPlane] = match scope {
            layer_core::TransformPixelsScope::Mask => &[RasterPlane::Mask],
            layer_core::TransformPixelsScope::Paint { linked_mask: true } => &[RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness, RasterPlane::Mask],
            layer_core::TransformPixelsScope::Paint { linked_mask: false } => &[RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness],
        };
        let mut inputs = Vec::new();
        let mut tiles = Vec::new();
        for &plane in planes {
            let (mut transform, extent) = if plane == RasterPlane::Mask {
                let mask = layer.mask.as_ref().unwrap();
                (if scope == layer_core::TransformPixelsScope::Mask { geometry.clone() }
                    else { layer_core::target_geometry(packet.layers, mask.id) }, mask.local_extent(extent))
            } else { (geometry.clone(), extent) };
            transform.placement.interpolation = geometry.placement.interpolation;
            let page = self.placed_raw_plane(r, layer, transform, coordinate, plane, extent)?;
            self.encode_jobs(r, encoder)?;
            let tile = RasterTile::pending(plane.descriptor(r.document_color()));
            inputs.push((self.pool[page].texture.clone(), tile.clone()));
            tiles.push((plane, tile));
        }
        let capture = r.encode_private_tiles(inputs, encoder)?;
        self.used.fill(false);
        Ok((tiles, capture))
    }

    pub(super) fn placed_raw_plane(
        &mut self, r: &WgpuRasterizer, layer: &Layer,
        transform: layer_core::ImageTransform, tile: [u32; 2], plane: layer_core::raster::RasterPlane,
        extent: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        self.placed_raw_plane_offset(r,layer,transform,tile,plane,extent,[0;2])
    }

    fn placed_raw_plane_offset(
        &mut self, r: &WgpuRasterizer, layer: &Layer,
        mut transform: layer_core::ImageTransform, tile: [u32;2], plane: layer_core::raster::RasterPlane,
        extent: [u32;2], mut offset: [i32;2],
    ) -> Result<usize,GpuRasterError> {
        use layer_core::raster::RasterPlane;
        if transform.placement.mesh.is_none() && offset!=[0;2] {
            transform.placement=transform.placement.post(layer_core::Projective::from_affine(layer_core::Affine::translation(
                layer_core::Point {x:offset[0] as f32,y:offset[1] as f32}))).ok_or(GpuRasterError::InvalidTransform("Invalid layer placement"))?;
            offset=[0;2];
        }
        let stored = r.paint_layers.iter().find(|stored| stored.id == layer.id);
        let background = if plane == RasterPlane::Mask { layer.mask.as_ref().unwrap().default_coverage } else { 0. };
        self.placed_plane_jobs(r, transform, extent, tile, background, plane != RasterPlane::Color, offset, |scene, c| {
            if plane == RasterPlane::Mask {
                let mut mask = layer.mask.as_ref().unwrap().clone();
                mask.inverted = false;
                let page = scene.mask_tile(r, &mask, layer_core::Point::default(), c);
                return Ok((ColorInput { view: scene.pool[page].view.clone(), lease: None }, Some(page)));
            }
            if plane == RasterPlane::Color {
                let preview = r.preview_layer_id == Some(layer.id) && !r.preview_damage.intersect(page_rect(c)).is_empty();
                let inputs = scene.color_inputs(r, layer, stored, c, preview)?;
                return match inputs {
                    [Some(base), Some(flow)] => {
                        let page = scene.alloc(r, wgpu::Color::TRANSPARENT);
                        for input in [base, flow] {
                            scene.draw(r, page, input.view, None, [0., 0., 256., 256.], [1., 1., 0., 0.], true, Convert::None);
                        }
                        Ok((ColorInput { view: scene.pool[page].view.clone(), lease: None }, Some(page)))
                    }
                    [Some(input), None] | [None, Some(input)] => Ok((input, None)),
                    [None, None] => Ok((ColorInput { view: r.empty_view.clone(), lease: None }, None)),
                };
            }
            let view = stored.and_then(|stored| match plane {
                RasterPlane::Wetness => stored.material_pages.iter().find(|p| p.coordinate == c).map(|p| &p.wetness.view),
                RasterPlane::WatercolorWetness => r.preview_watercolor_wetness_pages.iter()
                    .find(|p| r.preview_layer_id == Some(layer.id) && p.coordinate == c && !r.preview_damage.intersect(page_rect(c)).is_empty())
                    .or_else(|| stored.watercolor_wetness_pages.iter().find(|p| p.coordinate == c)).map(|p| &p.active().view),
                _ => None,
            });
            if let Some(view) = view { return Ok((ColorInput { view: view.clone(), lease: None }, None)); }
            if let Some(blob) = r.native_plane_tile(layer.id, plane, c)? {
                let space = r.document_color().space;
                let (tile, pending) = r.source_tiles.borrow_mut().plan_raster(r, &blob, space, space)?;
                if let Some(pending) = pending { scene.enqueue_source_decode(pending); }
                let lease = r.source_tiles.borrow().lease(&tile.view);
                return Ok((ColorInput { view: tile.view, lease }, None));
            }
            Ok((ColorInput { view: r.empty_scalar_view.clone(), lease: None }, None))
        })
    }

    pub(super) fn placed_material_tile(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, layer: &Layer,
        geometry: layer_core::ImageTransform, tile: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        self.placed_material_inputs(r, packet, layer, geometry, tile).map(|(page, _)| page)
    }

    pub(super) fn placed_material_inputs(
        &mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, layer: &Layer,
        geometry: layer_core::ImageTransform, tile: [u32; 2],
    ) -> Result<(usize, wgpu::TextureView), GpuRasterError> {
        use layer_core::raster::RasterPlane;
        let base = geometry.clone();
        let (bounds, radius) = self.material_coverage(r, layer.id, &base);
        if bounds.is_empty() || bounds
            .outset(radius as f32).intersect(page_rect(tile).to_rect()).is_empty() {
            let page = self.placed_raw_plane(r, layer, base, tile, RasterPlane::Color, layer.local_extent(packet.document_extent))?;
            return Ok((page, self.pool[page].view.clone()));
        }
        let mut pages = vec![r.empty_scalar_view.clone();10];
        for (neighbor,c) in [[1,1],[0,1],[2,1],[1,0],[1,2]].into_iter().enumerate() {
            for (plane_index,plane) in [RasterPlane::Color,RasterPlane::WatercolorWetness].into_iter().enumerate() {
                pages[plane_index*5+neighbor] = self.material_page(r, packet, layer, geometry.clone(), tile, c, plane)?;
            }
        }
        let binding = views_group(&r.device, "placed material neighborhood", &r.watercolor_layout,
            pages.iter());
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        self.jobs.push(Job::Watercolor {
            coordinates: (self.material_coordinates.clone(), 0), target: self.pool[out].view.clone(), binding,
            record: *r.layer_style_records.get(&layer.id).ok_or(GpuRasterError::MissingPaintLayer(layer.id))?,
        });
        Ok((out, pages[0].clone()))
    }

    pub(super) fn material_coverage(&mut self, r: &WgpuRasterizer, id: LayerId, geometry: &layer_core::ImageTransform) -> (layer_core::Rect, u32) {
        let entry=self.material_bounds.entry(id).or_insert_with(||MaterialBounds {raw:material_bounds(r,id),mapped:None});
        let bounds=if let Some((key,bounds))=&entry.mapped && key==geometry {*bounds} else {
            let source=entry.raw.outset(geometry.placement.interpolation.support() as f32);
            let bounds=self.mesh_geometry(geometry).map_or_else(||geometry.forward_bounds(source),|mesh|mesh.forward_bounds(source,geometry.source_from_owner));
            self.material_bounds.get_mut(&id).unwrap().mapped=Some((geometry.clone(),bounds));bounds
        };
        let radius = r.paint_layers.iter().find(|stored| stored.id == id)
            .and_then(|stored| stored.watercolor).map_or(0, |style| style.radius());
        (bounds, radius)
    }

    pub(super) fn clear_material_pages(&mut self) {
        self.material_bounds.clear();
        self.position_key = None;
        while let Some(entry) = self.material_pages.pop_front() { self.free(entry.page); }
    }

    fn material_page(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, layer: &Layer,
        geometry: layer_core::ImageTransform, tile: [u32; 2], neighbor: [u32; 2],
        plane: layer_core::raster::RasterPlane,
    ) -> Result<wgpu::TextureView, GpuRasterError> {
        let coordinate = std::array::from_fn(|i| tile[i] as i32 + neighbor[i] as i32 - 1);
        if plane == layer_core::raster::RasterPlane::WatercolorWetness || neighbor != [1, 1] {
            let bounds = self.material_coverage(r, layer.id, &geometry).0;
            let origin = coordinate.map(|n| (n * PAGE_SIZE as i32) as f32);
            let page = layer_core::Rect { min: layer_core::Point { x: origin[0], y: origin[1] },
                max: layer_core::Point { x: origin[0] + PAGE_SIZE as f32, y: origin[1] + PAGE_SIZE as f32 } };
            if bounds.intersect(page).is_empty() {
                return Ok(if plane == layer_core::raster::RasterPlane::Color {
                    r.empty_view.clone()
                } else { r.empty_scalar_view.clone() });
            }
        }
        if let Some(index) = self.material_pages.iter().position(|entry| entry.layer == layer.id
            && entry.geometry == geometry && entry.interpolation == geometry.placement.interpolation
            && entry.coordinate == coordinate && entry.plane == plane) {
            let entry = self.material_pages.remove(index).unwrap();
            let page = entry.page;
            self.material_pages.push_back(entry);
            return Ok(self.pool[page].view.clone());
        }
        if self.material_pages.len() == Self::MATERIAL_CACHE_PAGES {
            let entry = self.material_pages.pop_front().unwrap();
            self.free(entry.page);
        }
        let offset=tile.map(|c| PAGE_SIZE as i32-(c*PAGE_SIZE) as i32);
        let page = self.placed_raw_plane_offset(r, layer, geometry.clone(), neighbor, plane, layer.local_extent(packet.document_extent),offset)?;
        self.material_pages.push_back(MaterialPage { layer: layer.id, interpolation: geometry.placement.interpolation, geometry,
            coordinate, plane, page });
        Ok(self.pool[page].view.clone())
    }

    /// A scratch tile of `layer`'s own pixels at its local `c`.
    pub(crate) fn local_color_tile(
        &mut self,
        r: &WgpuRasterizer,
        packet: FramePacket<'_>,
        layer: &Layer,
        c: [u32; 2],
    ) -> Result<usize, GpuRasterError> {
        let out = self.alloc(r, wgpu::Color::TRANSPARENT);
        let stored = r.paint_layers.iter().find(|l| l.id == layer.id);
        self.paint_page(r, packet, layer, stored, c, out, [0., 0., 256., 256.], Convert::None)?;
        Ok(out)
    }
}

pub(super) struct MaterialBounds {
    raw: layer_core::Rect,
    mapped: Option<(layer_core::ImageTransform, layer_core::Rect)>,
}

pub(super) fn material_bounds(r: &WgpuRasterizer, id: LayerId) -> layer_core::Rect {
    r.paint_layers.iter().filter(|stored| stored.id == id)
        .flat_map(|stored| &stored.watercolor_wetness_pages).map(|page| page.coordinate)
        .chain(r.preview_watercolor_wetness_pages.iter().filter(|_| r.preview_layer_id == Some(id)).map(|page| page.coordinate))
        .chain(r.native_backing(id).into_iter().flat_map(|data| data.tiles.keys())
            .filter(|key| key.plane == layer_core::raster::RasterPlane::WatercolorWetness).map(|key| key.coordinate))
        .fold(layer_core::Rect::EMPTY, |bounds, coordinate| bounds.union(page_rect(coordinate).to_rect()))
}

pub(super) fn prepare(
    pass: &mut pixel_transform::PixelTransform,
    r: &mut WgpuRasterizer,
    encoder: &mut crate::submission::CommandEncoder,
    job: &PlacementJob,
) -> Result<PlacementDraw, GpuRasterError> {
    let coordinates: Vec<_> = job.sources.iter().map(|(c, _)| *c).collect();
    let bounds = [0, 0, job.extent[0] as i32, job.extent[1] as i32];
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
                target: job.tile,
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
