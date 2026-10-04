use super::*;
use layer_core::{Affine, ContentBoundsRequest, ContentScope, Point, Rect};

impl SnapshotGpu {
    pub async fn content_bounds(&self, request: ContentBoundsRequest, control: CaptureControl) -> Result<Rect, String> {
        control.check().map_err(|e| e.to_string())?;
        if let Some(bounds) = request.known_bounds() { return Ok(bounds); }
        let mut document = (*request.document).clone();
        Project { document: document.clone() }.validate(ProjectLimits::default())?;
        let original_extent = [document.width, document.height];
        let mut selection = None;
        let target = if let ContentScope::Target(id) = request.scope {
            let owner = document.target_owner(id).ok_or("The bounds target was removed")?;
            selection = document.selection.as_ref().map(|s| {
                s.transformed(document.layer_geometry(id).as_affine().and_then(Affine::inverse).ok_or("Invalid layer placement")?)
                    .map(Arc::new).map_err(|e| e.to_string())
            }).transpose()?;
            let mut layer = owner.composite_snapshot();
            let extent = if id == layer.id { layer.local_extent(original_extent) } else { layer.mask.as_ref().unwrap().local_extent(layer.local_extent(original_extent)) };
            layer.properties.parent = None;
            layer.properties.placement = layer_core::LayerPlacement::IDENTITY;
            layer.properties.offset = Point::default();
            layer.properties.clipped = false;
            layer.visible = true;
            layer.opacity = 1.;
            if id == layer.id { layer.mask = None; }
            else if let Some(mask) = &mut layer.mask {
                mask.placement = layer_core::Projective::IDENTITY;
                mask.offset = Point::default();
                mask.enabled = true;
            }
            document.active_layer = layer.id;
            document.active_mask = id != layer.id;
            document.layers = vec![layer];
            document.width = extent[0];
            document.height = extent[1];
            Some(id)
        } else {
            if request.scope == ContentScope::All {
                for layer in &mut document.layers {
                    layer.visible = true;
                    layer.opacity = 1.;
                    layer.mask = None;
                    layer.properties.clipped = false;
                    layer.properties.blend = layer_core::LayerBlend::Normal;
                }
            }
            None
        };
        document.selection = None;
        document.rulers.clear();
        document.reference_layers.clear();
        if document.layers.is_empty() { return Ok(Rect::EMPTY); }
        if document.layer(document.active_layer).is_none() {
            document.active_layer = document.layers[0].id;
            document.active_mask = false;
        }
        if request.scope == ContentScope::All {
            document.layers.retain(|l| l.kind != LayerKind::Effect
                || l.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator));
            if document.layers.is_empty() { return Ok(Rect::EMPTY); }
            if document.layer(document.active_layer).is_none() {
                document.active_layer = document.layers[0].id;
                document.active_mask = false;
            }
        }
        let mut candidates = if target == Some(document.layers[0].id) {
            vec![local_hull(&document.layers[0], [document.width, document.height])?]
        } else if request.scope == ContentScope::Canvas || target.is_some() {
            vec![Rect::from_extent([document.width, document.height])]
        } else {
            let mut candidates = Vec::new();
            for layer in document.layers.iter().filter(|l| document.layer_is_visible(l.id) && l.opacity > 0.) {
                let next = match layer.kind {
                    LayerKind::Paint => {
                        let transform = document.layer_geometry(layer.id);
                        let copied = transform.as_affine().is_some_and(|a| a.0[..4] == [1.,0.,0.,1.] && a.0[4..].iter().all(|v| v.fract() == 0.));
                        transform.forward_bounds(local_hull(layer, original_extent)?.outset(if copied { 0. }
                            else { transform.placement.interpolation.support() as f32 }))
                            .outset(layer.raster.wait_data()?.watercolor.map_or(0., |style|
                                2. * style.edge_width.clamp(1., 16.)))
                    }
                    LayerKind::Effect if layer.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator) => Rect::from_extent(original_extent),
                    _ => Rect::EMPTY,
                };
                if !next.is_empty() { candidates.push(next); }
            }
            if matches!(request.scope, ContentScope::Visible | ContentScope::PlacedTarget(_)) {
                for effect in document.layers.iter().filter(|l| document.layer_is_visible(l.id) && l.opacity > 0.)
                    .filter_map(|l| l.effect.as_ref()).filter(|e| e.program.kind == layer_core::EffectKind::Adjustment
                        && e.program.alpha == layer_core::EffectAlpha::Filter)
                {
                    if let Some(radius) = effect.damage_radius() {
                        for bounds in &mut candidates { *bounds = bounds.outset(radius as f32); }
                    }
                    candidates.push(Rect::from_extent(original_extent));
                }
            }
            candidates
        };
        if let Some(selection) = selection.as_ref().filter(|s| !s.inverted) {
            for bounds in &mut candidates { *bounds = bounds.intersect(selection.bounds().outset(1.)); }
        }
        candidates.retain(|bounds| !bounds.is_empty());
        let domain = if target.is_some() { Rect::from_extent([document.width, document.height]) }
            else { candidates.iter().copied().fold(Rect::EMPTY, Rect::union) };
        if domain.is_empty() { return Ok(Rect::EMPTY); }
        let origin = [domain.min.x, domain.min.y].map(|v| (v / PAGE_SIZE as f32).floor() * PAGE_SIZE as f32);
        let extent = [domain.max.x.ceil() - origin[0], domain.max.y.ceil() - origin[1]];
        if extent.iter().any(|n| !n.is_finite() || *n < 1. || *n > (1 << 24) as f32) {
            return Err("The content is too far apart to measure precisely".into());
        }
        let extent = extent.map(|v| v as u32);
        let mut coordinates = std::collections::BTreeSet::new();
        for candidate in candidates {
            let min = [candidate.min.x - origin[0], candidate.min.y - origin[1]].map(|v| v.floor().max(0.) as u32 / PAGE_SIZE);
            let max = [candidate.max.x - origin[0], candidate.max.y - origin[1]].map(|v| (v.ceil().max(0.) as u32).div_ceil(PAGE_SIZE));
            for y in min[1]..max[1] {
                for x in min[0]..max[0] {
                    coordinates.insert([x, y]);
                    if coordinates.len() > (layer_core::MAX_EXTENT / PAGE_SIZE).pow(2) as usize {
                        return Err("The content needs too much work to measure at once".into());
                    }
                }
            }
        }
        let last = extent.map(|v| v.div_ceil(PAGE_SIZE) - 1);
        let mut coordinates: Vec<_> = coordinates.into_iter().collect();
        coordinates.sort_unstable_by_key(|[x, y]| {
            let [dx, dy] = [(*x).min(last[0] - x), (*y).min(last[1] - y)];
            (dx.min(dy), dx + dy, *x, *y)
        });
        if target.is_none() && (origin != [0.; 2] || extent != original_extent) {
            let generators: Vec<_> = document.layers.iter().filter(|l| l.kind == LayerKind::Effect
                && l.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator))
                .map(|l| l.id).collect();
            for id in generators {
                let owner = document.layer(id).unwrap();
                let parent = owner.properties.parent;
                let world = document.layer_offset(id);
                let offset = Point { x: owner.properties.offset.x - world.x, y: owner.properties.offset.y - world.y };
                let mut group = Layer::paint(document.allocate_layer_id(), "");
                group.kind = LayerKind::Group;
                group.properties.parent = parent;
                let mut mask = layer_core::LayerMask::reveal_all(document.allocate_layer_id(), offset);
                mask.linked = false;
                mask.default_coverage = 0.;
                mask.initial = Some(layer_core::Selection::polygon(Rect::from_extent(original_extent).corners().to_vec()).map_err(|e| e.to_string())?);
                group.mask = Some(mask);
                document.layers.iter_mut().find(|l| l.id == id).unwrap().properties.parent = Some(group.id);
                document.layers.push(group);
            }
        }
        let canvas = [document.width, document.height];
        for layer in &mut document.layers {
            if layer.kind == LayerKind::Paint { layer.properties.extent = Some(layer.local_extent(canvas)); }
            if layer.properties.parent.is_none() {
                layer.properties.offset.x -= origin[0];
                layer.properties.offset.y -= origin[1];
                if let Some(mask) = &mut layer.mask {
                    mask.offset.x -= origin[0];
                    mask.offset.y -= origin[1];
                }
            }
        }
        document.width = extent[0];
        document.height = extent[1];
        const BATCH: usize = 8;
        let mut snapshot = SnapshotRenderer::construct(Project { document }, request.time, control.clone(), self).map_err(|e| e.to_string())?;
        snapshot.planned_pixel_bytes = PLANNED_PIXEL_BYTES / BATCH as u64;
        if target.is_none() { snapshot.renderer.capture_frame = Some((origin, original_extent)); }
        for (id, phase) in &request.effect_times {
            if !phase.is_finite() { return Err("Invalid effect time".into()); }
            if let Some(effect) = snapshot.layers.iter().find(|l| l.id == *id).and_then(|l| l.effect.as_ref()) {
                snapshot.renderer.effect_clocks.insert(*id, (effect.program.id.clone(), layer_core::EffectClock::at(effect, request.time, *phase)));
            }
        }
        if target.is_none() { snapshot.prepare_effect_analysis_async(scene::Output::Artwork(None)).await?; }
        let device = snapshot.renderer.device.clone();
        let queue = snapshot.renderer.queue.clone();
        let pipeline = crate::thumbnails::BoundsPipeline::new(&device);
        let output = crate::thumbnails::BoundsPipeline::buffer(&device);
        let mut pending = 0;
        let mut b = [u32::MAX, u32::MAX, 0, 0];
        for [column, row] in coordinates {
            let [x, y] = [column * PAGE_SIZE, row * PAGE_SIZE];
            control.check().map_err(|e| e.to_string())?;
            let size = [PAGE_SIZE.min(extent[0] - x), PAGE_SIZE.min(extent[1] - y)];
            if x >= b[0] && y >= b[1] && x + size[0] <= b[2] && y + size[1] <= b[3] { continue; }
            let region = [x, y, size[0], size[1]];
            let capture = |snapshot: &mut SnapshotRenderer| {
                if let Some(id) = target {
                    snapshot.with_region_gpu(region, 32, |r, packet, region, encoder| {
                        let coordinate = [x / PAGE_SIZE, y / PAGE_SIZE];
                        let (texture, mask) = if let Some(mask) = r.layer_masks.definitions.get(&id) {
                            let page = r.layer_masks.pages.get(&(id, coordinate));
                            (page.map(|p| p.texture.clone()), Some((mask.inverted, page.is_none().then_some(mask.default_coverage))))
                        } else { (r.raw_layer_tile(id, coordinate, encoder)?.map(|p| p.texture), None) };
                        let texture = texture.unwrap_or_else(|| r._empty_texture.clone());
                        if let Some(selection) = &selection {
                            r.selection_clip.prepare_region(&r.device, encoder, packet.document_extent, selection, Some(region))?;
                        }
                        pipeline.reduce(&r.device, encoder, &output, &texture, [x, y], extent, mask,
                            selection.as_ref().and(r.selection_clip.buffer.as_ref()));
                        Ok(())
                    })
                } else {
                    snapshot.capture_region_gpu(region, 32, |device, texture, encoder| {
                        pipeline.reduce(device, encoder, &output, texture, [x, y], extent, None, None);
                    })
                }
            };
            match capture(&mut snapshot) {
                Err(GpuRasterError::CaptureBudget { .. }) => {
                    if pending > 0 { read_bounds(&device, &queue, &output).await?; }
                    snapshot.planned_pixel_bytes = PLANNED_PIXEL_BYTES;
                    capture(&mut snapshot).map_err(|e| e.to_string())?;
                    snapshot.planned_pixel_bytes = PLANNED_PIXEL_BYTES / BATCH as u64;
                    pending = BATCH;
                }
                result => { result.map_err(|e| e.to_string())?; pending += 1; }
            }
            if pending == BATCH {
                b = read_bounds(&device, &queue, &output).await?;
                pending = 0;
            }
        }
        control.check().map_err(|e| e.to_string())?;
        if pending > 0 { b = read_bounds(&device, &queue, &output).await?; }
        control.check().map_err(|e| e.to_string())?;
        Ok(if b[0] >= b[2] || b[1] >= b[3] { Rect::EMPTY } else {
            Rect { min: Point { x: b[0] as f32 + origin[0], y: b[1] as f32 + origin[1] },
                max: Point { x: b[2] as f32 + origin[0], y: b[3] as f32 + origin[1] } }
        })
    }
}
async fn read_bounds(device: &PipelineDevice, queue: &wgpu::Queue, output: &wgpu::Buffer) -> Result<[u32; 4], String> {
    let bytes = crate::local_tone::read_buffer_async(device, queue, output).await?;
    Ok(std::array::from_fn(|i| u32::from_le_bytes(bytes[4*i..4*i+4].try_into().unwrap())))
}

fn local_hull(layer: &Layer, extent: [u32; 2]) -> Result<Rect, String> {
    let mut bounds = layer.source.as_ref().map_or(Rect::EMPTY, |s| Rect::from_extent(s.extent));
    let raster = layer.raster.wait_data()?;
    for key in raster.tiles.keys().filter(|key| key.plane == RasterPlane::Color) {
        bounds = bounds.union(page_rect(key.coordinate).to_rect());
    }
    Ok(bounds.intersect(Rect::from_extent(layer.local_extent(extent))))
}
