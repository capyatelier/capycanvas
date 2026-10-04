use super::*;
use layer_core::{Affine, ContentBoundsRequest, ContentScope, Point, Rect};

impl SnapshotGpu {
    pub fn bounds_source_scene(request:&ContentBoundsRequest)->Result<Arc<SceneSnapshot>,String> {
        let mut scene=(*request.snapshot).clone();
        match request.scope {
            ContentScope::Target(target)=>scene.scope=SceneScope::Raw(target),
            ContentScope::PlacedTarget(handle)=>{
                let view=scene.view();view.occurrence(handle).ok_or("The bounds target was removed")?;
                let mut members:Vec<_>=view.order().iter().copied().filter(|h|*h==handle||view.effect_owner(*h)==Some(handle)||layer_core::descends_from(view,*h,Some(handle))).collect();
                let mut parent=view.parent(handle);
                while let Some(handle)=parent {members.push(handle);parent=view.parent(handle);}
                scene.scope=SceneScope::Members(members.into());
            }
            ContentScope::All=>{
                let handles=scene.view().order().to_vec();
                for handle in handles {
                    let occurrence=scene.artwork.occurrences.get_mut(handle).unwrap();
                    occurrence.visible=true;occurrence.opacity=1.;occurrence.mask=None;occurrence.attachment=layer_core::Attachment::None;occurrence.blend=layer_core::LayerBlend::Normal;
                }
            }
            _=>{}
        }
        scene.index = Arc::new(layer_core::SceneIndex::build(&scene.artwork)?);
        Ok(Arc::new(scene))
    }
    pub async fn content_bounds(&self, request: ContentBoundsRequest, control: CaptureControl) -> Result<Rect, String> {
        control.check().map_err(|e| e.to_string())?;
        if let Some(bounds) = request.known_bounds() { return Ok(bounds); }
        let mut scene = (*Self::bounds_source_scene(&request)?).clone();
        let original_extent = scene.view().composition().size;
        let mut selection = None;
        let target = if let ContentScope::Target(target) = request.scope {
            let view = scene.view();
            let owner = view.source_owner(target).ok_or("The bounds target was removed")?;
            selection = request.selection.as_ref().map(|s| {
                s.transformed(view.target_geometry(target).as_affine().and_then(Affine::inverse).ok_or("Invalid layer placement")?)
                    .map(Arc::new).map_err(|e| e.to_string())
            }).transpose()?;
            let extent = view.target_extent(target);
            let occurrence = scene.artwork.occurrences.get_mut(owner).unwrap();
            occurrence.translation = Point::default(); occurrence.placement = layer_core::LayerPlacement::IDENTITY;
            occurrence.attachment = layer_core::Attachment::None; occurrence.visible = true; occurrence.opacity = 1.;
            if matches!(target, SourceTarget::Paint(_)) { occurrence.mask = None; }
            else if let Some(mask) = &mut occurrence.mask {
                mask.placement = layer_core::Projective::IDENTITY; mask.translation = Point::default(); mask.linked = false; mask.enabled = true;
            }
            let mut parent = scene.view().parent(owner);
            while let Some(handle) = parent {
                parent = scene.view().parent(handle);
                let occurrence = scene.artwork.occurrences.get_mut(handle).unwrap();
                occurrence.translation = Point::default(); occurrence.placement = layer_core::LayerPlacement::IDENTITY;
                occurrence.visible = true;
            }
            scene.artwork.compositions.get_mut(scene.artwork.root).unwrap().size = extent;
            scene.scope = SceneScope::Raw(target); Some(target)
        } else {
            None
        };
        let view = scene.view();
        if target.is_none() {
            let members = view.order().iter().copied().filter(|&h| {
                let o = view.occurrence(h).unwrap();
                view.includes(h) && (request.scope != ContentScope::All
                    || !matches!(o.content, OccurrenceContent::Effect(_)) || view.effect(h).is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator))
            }).collect::<Vec<_>>();
            if members.is_empty() { return Ok(Rect::EMPTY); }
            scene.scope = SceneScope::Members(members.into());
        }
        let view = scene.view();
        let canvas = view.composition().size;
        let mut candidates = if let Some(target @ SourceTarget::Paint(_)) = target {
            vec![local_hull(view, target)?]
        } else if request.scope == ContentScope::Canvas || target.is_some() { vec![Rect::from_extent(canvas)] }
        else {
            let mut candidates = Vec::new();
            for &handle in view.order() {
                let occurrence = view.occurrence(handle).unwrap();
                if !view.visible(handle) || occurrence.opacity <= 0. { continue; }
                let next = match occurrence.content {
                    OccurrenceContent::Paint(h) => {
                        let target = SourceTarget::Paint(h); let transform = view.target_geometry(target);
                        let copied = transform.as_affine().is_some_and(|a| a.0[..4] == [1.,0.,0.,1.] && a.0[4..].iter().all(|v| v.fract() == 0.));
                        transform.forward_bounds(local_hull(view, target)?.outset(if copied {0.} else {transform.placement.interpolation.support() as f32}))
                            .outset(view.paint(h).unwrap().raster.wait_data()?.watercolor.map_or(0., |style| 2. * style.edge_width.clamp(1.,16.)))
                    },
                    OccurrenceContent::Effect(_) if view.effect(handle).is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator) => Rect::from_extent(original_extent), _ => Rect::EMPTY,
                };
                if !next.is_empty() { candidates.push(next); }
            }
            if matches!(request.scope, ContentScope::Visible | ContentScope::PlacedTarget(_)) {
                for effect in view.order().iter().filter(|&&h| view.visible(h) && view.occurrence(h).unwrap().opacity > 0.)
                    .filter_map(|&h| view.effect(h)).filter(|e| e.program.kind == layer_core::EffectKind::Adjustment && e.program.alpha == layer_core::EffectAlpha::Filter) {
                    if let Some(radius) = effect.damage_radius() { for bounds in &mut candidates { *bounds = bounds.outset(radius as f32); } }
                    candidates.push(Rect::from_extent(original_extent));
                }
            }
            candidates
        };
        if let Some(selection) = selection.as_ref().filter(|s| !s.inverted) {
            for bounds in &mut candidates { *bounds = bounds.intersect(selection.bounds().outset(1.)); }
        }
        candidates.retain(|bounds| !bounds.is_empty());
        let domain = if target.is_some() { Rect::from_extent(canvas) }
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
        if target.is_none() && (origin != [0.;2] || extent != original_extent) {
            let generators: Vec<_> = scene.view().order().iter().copied().filter(|&h| scene.view().effect(h).is_some_and(|e| e.program.kind == layer_core::EffectKind::Generator)).collect();
            for handle in generators {
                let view = scene.view(); let owner = view.occurrence(handle).unwrap();
                let parent_stack = view.stack(handle).unwrap();
                let world = scene::world_offset(view, handle, false);
                let offset = Point {x:owner.translation.x-world.x,y:owner.translation.y-world.y};
                let attachment = owner.attachment;
                let child = scene.artwork.stacks.insert(layer_core::authored::PortableId::random(), layer_core::authored::Stack {entries:vec![handle]}).map_err(str::to_owned)?;
                let coverage = scene.artwork.coverage.insert(layer_core::authored::PortableId::random(), layer_core::authored::CoverageSource {
                    domain:original_extent,raster:Default::default(),initial:Some(layer_core::Selection::polygon(Rect::from_extent(original_extent).corners().to_vec()).map_err(|e|e.to_string())?),default_coverage:0.,operations:Arc::default()
                }).map_err(str::to_owned)?;
                let mut group = layer_core::authored::Occurrence::new(OccurrenceContent::Stack(child), "");
                group.attachment = attachment;
                group.mask = Some(layer_core::authored::MaskUse {source:coverage,linked:false,enabled:true,inverted:false,translation:offset,placement:layer_core::Projective::IDENTITY});
                let group = scene.artwork.occurrences.insert(layer_core::authored::PortableId::random(),group).map_err(str::to_owned)?;
                let entries = &mut scene.artwork.stacks.get_mut(parent_stack).unwrap().entries;
                let entry = entries.iter_mut().find(|h| **h == handle).ok_or("Missing generator occurrence")?; *entry = group;
                scene.artwork.occurrences.get_mut(handle).unwrap().attachment = layer_core::Attachment::None;
                if let SceneScope::Members(members) = &mut scene.scope { let mut updated = members.to_vec(); updated.push(group); *members = updated.into(); }
                scene.index = Arc::new(SceneIndex::build(&scene.artwork)?);
            }
        }
        let roots = scene.view().children(None).to_vec();
        for handle in roots {
            let occurrence = scene.artwork.occurrences.get_mut(handle).unwrap();
            occurrence.translation.x -= origin[0]; occurrence.translation.y -= origin[1];
            if let Some(mask) = &mut occurrence.mask { mask.translation.x -= origin[0]; mask.translation.y -= origin[1]; }
        }
        scene.artwork.compositions.get_mut(scene.artwork.root).unwrap().size = extent;
        const BATCH: usize = 8;
        let scope = scene.scope.clone();
        let mut snapshot = SnapshotRenderer::construct(Arc::new(scene), scope, control.clone(), self).map_err(|e| e.to_string())?;
        snapshot.planned_pixel_bytes = PLANNED_PIXEL_BYTES / BATCH as u64;
        if target.is_none() { snapshot.renderer.capture_frame = Some((origin, original_extent)); }
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
            let snapshot_mask_inverted = target.and_then(|t| snapshot.scene.view().source_owner(t).and_then(|h| snapshot.scene.view().mask(h))).is_some_and(|(use_,_)| use_.inverted);
            let capture = |snapshot: &mut SnapshotRenderer| {
                if let Some(id) = target {
                    snapshot.with_region_gpu(region, 32, |r, packet, region, encoder| {
                        let coordinate = [x / PAGE_SIZE, y / PAGE_SIZE];
                        let (texture, mask) = if let Some(mask) = r.layer_masks.definitions.get(&id) {
                            let page = r.layer_masks.pages.get(&(id, coordinate));
                            (page.map(|p| p.texture.clone()), Some((snapshot_mask_inverted, page.is_none().then_some(mask.default_coverage))))
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

fn local_hull(scene: SceneView<'_>, target: SourceTarget) -> Result<Rect, String> {
    let mut bounds = scene.original(target).map_or(Rect::EMPTY, |source| Rect::from_extent(source.extent));
    let raster = scene.raster(target).ok_or("Missing raster source")?.wait_data()?;
    for key in raster.tiles.keys().filter(|key| key.plane == RasterPlane::Color) { bounds = bounds.union(page_rect(key.coordinate).to_rect()); }
    Ok(bounds.intersect(Rect::from_extent(scene.target_extent(target))))
}
