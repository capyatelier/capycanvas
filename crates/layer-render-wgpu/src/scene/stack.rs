use super::*;

type Checkpoint<I> = (OccurrenceHandle, I);

pub(super) trait Compositor {
    type Image;
    fn clear(&mut self) -> Result<Self::Image, GpuRasterError>;
    fn discard(&mut self, image: Self::Image);
    fn duplicate(&mut self, image: &Self::Image) -> Result<Self::Image, GpuRasterError>;
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError>;
    fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError>;
    fn blend(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle, clipped: bool) -> Result<Self::Image, GpuRasterError>;
    fn effect(&mut self, chain: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError>;
    fn has_content(&self, index: OccurrenceHandle) -> bool;
    fn draw_normal(&mut self, _index: OccurrenceHandle, _output: &Self::Image) -> Result<bool, GpuRasterError> { Ok(false) }
    fn checkpoint(&mut self, _parent: Option<OccurrenceHandle>) -> Result<Option<Checkpoint<Self::Image>>, GpuRasterError> { Ok(None) }
    fn presented_backdrop(&self) -> Option<OccurrenceHandle> { None }
}

pub(super) fn has_content(r: &WgpuRasterizer, scene: layer_core::SceneView<'_>, handle: OccurrenceHandle) -> bool {
    let occurrence = scene.occurrence(handle).unwrap();
    matches!(occurrence.kind(), LayerKind::Group | LayerKind::Effect)
        || scene.object_layer(handle).is_some()
        || scene.vector_layer(handle).is_some()
        || scene.paint_source(handle).is_some_and(|paint| paint.base.is_some())
        || scene.source_target(handle).is_some_and(|target| {
            r.native_color_coordinates(target).next().is_some()
                || r.paint_layers.iter().any(|stored| stored.id == target && !stored.pages.is_empty())
                || r.preview_layer_id == Some(target)
        })
}

pub(super) fn compose<C: Compositor>(
    c: &mut C, scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop_before: Option<OccurrenceHandle>,
) -> Result<C::Image, GpuRasterError> {
    if let Some(effect) = stop_before && let Some(owner) = scene.effect_owner(effect) {
        return owner_image(c, scene, owner, Some(effect));
    }
    let checkpoint = c.checkpoint(parent)?;
    let cut = checkpoint.as_ref().map(|(index, ..)| *index);
    let output = match checkpoint { Some((_, output)) => output, None => c.clear()? };
    let (Flow::Done(output) | Flow::Stopped(output)) =
        group_into(c, scene, parent, stop_before, cut, output, None)?;
    Ok(output)
}

enum Flow<I> { Done(I), Stopped(I) }

fn stop_root(scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop: Option<OccurrenceHandle>) -> Option<OccurrenceHandle> {
    let mut root = stop?;
    for _ in 0..scene.order().len() {
        let up = scene.evaluation_parent(root);
        if up == parent { return Some(root); }
        root = up?;
    }
    None
}

fn group_into<C: Compositor>(
    c: &mut C, scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop_before: Option<OccurrenceHandle>,
    cut: Option<OccurrenceHandle>, mut output: C::Image, mut stack: Option<(C::Image, OccurrenceHandle)>,
) -> Result<Flow<C::Image>, GpuRasterError> {
    let stop = stop_root(scene, parent, stop_before);
    let presented = parent.is_none().then(|| c.presented_backdrop()).flatten();
    let mut siblings = scene.members(parent).rev()
        .filter(|handle| {
            let occurrence = scene.occurrence(*handle).unwrap();
            occurrence.is_artwork() && scene.includes(*handle) && scene.effect_owner(*handle).is_none() && presented != Some(*handle)
                && cut.is_none_or(|cut| scene.position(*handle) < scene.position(cut))
        }).peekable();
    while let Some(i) = siblings.next() {
        let layer = scene.occurrence(i).unwrap();
        if stop_before == Some(i) {
            if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
            return Ok(Flow::Stopped(output));
        }
        if scene.effect(i).is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment) {
            if let Some((pixels, base)) = stack.take() {
                output = c.blend(pixels, output, base, false)?;
            }
            if !scene.visible(i) { continue; }
            let mut chain = vec![i];
            if direct_effect_mask(scene, i) && !scene.effect(i).unwrap().program.fusion_boundary() {
                while let Some(j) = siblings.peek().filter(|j| {
                    stop.is_none_or(|stop| scene.position(**j) > scene.position(stop)) && fuses_after(scene, i, **j, chain.len())
                }) {
                    chain.push(*j);
                    siblings.next();
                }
            }
            output = c.effect(&chain, output)?;
            continue;
        }
        if !scene.effective_clipped(i) {
            if let Some((pixels, base)) = stack.take() { output = c.blend(pixels, output, base, false)?; }
            if layer.kind() == LayerKind::Group && layer.blend == layer_core::LayerBlend::PassThrough {
                if !scene.visible(i) { continue; }
                let fade = layer.opacity != 1. || layer.mask.as_ref().is_some_and(|m| m.enabled);
                let (input, backdrop) = if fade { (c.duplicate(&output)?, Some(output)) } else { (output, None) };
                match group_into(c, scene, Some(i), stop_before, None, input, None)? {
                    Flow::Done(result) => output = if let Some(backdrop) = backdrop { c.fade(result, backdrop, i)? } else { result },
                    Flow::Stopped(result) => {
                        if let Some(backdrop) = backdrop { c.discard(backdrop); }
                        return Ok(Flow::Stopped(result));
                    }
                }
                continue;
            }
            let clips_above = siblings.peek().is_some_and(|handle| scene.effective_clipped(*handle));
            if scene.visible(i) && !clips_above && scene.attached_effects(i).is_empty() && c.draw_normal(i, &output)? { continue; }
            if scene.visible(i) && (c.has_content(i) || !scene.attached_effects(i).is_empty()) { stack = Some((owner_image(c, scene, i, None)?, i)); }
        } else if scene.visible(i) && let Some((pixels, base)) = stack.take() {
            let source = owner_image(c, scene, i, None)?;
            stack = Some((c.blend(source, pixels, i, true)?, base));
        }
    }
    if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
    Ok(Flow::Done(output))
}

pub(super) fn owner_image<C: Compositor>(
    c: &mut C, scene: layer_core::SceneView<'_>, owner: OccurrenceHandle, before: Option<OccurrenceHandle>,
) -> Result<C::Image, GpuRasterError> {
    if !scene.visible(owner) { return c.clear(); }
    let mut output = c.layer(owner)?;
    let mut effects = scene.attached_effects(owner).iter().copied().take_while(|h| Some(*h) != before)
        .filter(|h| scene.visible(*h)).peekable();
    while let Some(head) = effects.next() {
        let mut chain = vec![head];
        if direct_effect_mask(scene, head) && fusable_adjustment(scene, head) {
            while let Some(next) = effects.peek().filter(|next| fuses_after(scene, head, **next, chain.len())) {
                chain.push(*next);
                effects.next();
            }
        }
        output = c.effect(&chain, output)?;
    }
    Ok(output)
}

pub(super) fn tile(
    scene: &mut Scene, r: &WgpuRasterizer, packet: FramePacket<'_>, parent: Option<OccurrenceHandle>, coordinate: [u32; 2], accepted:&mut sources::AcceptedInputs,
) -> Result<usize, GpuRasterError> {
    let stop = scene.stop_before.take();
    let presented = scene.presented_backdrop;
    let mut compositor = Tile { scene, r, packet, coordinate, stop, presented, waiting: false,accepted };
    let result = compose(&mut compositor, packet.scene, parent, stop);
    let waiting = compositor.waiting;
    let result = result.and_then(|lease| if waiting {Err(GpuRasterError::NotReady)} else {Ok(scene.keep(lease))});
    scene.stop_before = stop;
    result
}

struct Tile<'a> {
    scene: &'a mut Scene,
    accepted:&'a mut sources::AcceptedInputs,
    r: &'a WgpuRasterizer,
    packet: FramePacket<'a>,
    coordinate: [u32; 2],
    stop: Option<OccurrenceHandle>,
    presented: Option<OccurrenceHandle>,
    waiting: bool,
}
impl Compositor for Tile<'_> {
    type Image = scratch::Lease;
    fn clear(&mut self) -> Result<Self::Image, GpuRasterError> {
        let output = self.scene.alloc(self.r, wgpu::Color::TRANSPARENT)?;
        Ok(self.scene.own(output))
    }
    fn discard(&mut self, image: Self::Image) { drop(image); }
    fn duplicate(&mut self, image: &Self::Image) -> Result<Self::Image, GpuRasterError> {
        let output = self.scene.reserve(self.r)?;
        self.scene.draw(self.r, output, self.scene.pool[image.index].view.clone(), None,
            [0., 0., 256., 256.], [1., 1., 0., 0.], false, Convert::None);
        Ok(self.scene.own(output))
    }
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
        let group = self.packet.scene.occurrence(index).unwrap();
        let result = (|| if let Some((mask, source)) = self.packet.scene.mask(index).filter(|(m, _)| m.enabled) {
            let coverage = self.scene.mask_at(self.r, mask, source, self.packet.scene.target_offset(SourceTarget::Coverage(mask.source)), self.coordinate, self.accepted)?;
            let coverage = self.scene.own(coverage);
            let change = self.weighted_sum(&front, &back, [1., -1.])?;
            let masked = self.scene.reserve(self.r)?;
            let masked = self.scene.own(masked);
            self.scene.draw(self.r, masked.index, self.scene.pool[change.index].view.clone(), Some(self.scene.pool[coverage.index].view.clone()),
                [0., 0., 256., 256.], [3., 1., 0., 0.], false, Convert::None);
            self.weighted_sum(&masked, &back, [group.opacity, 1.])
        } else { self.weighted_sum(&front, &back, [group.opacity, 1. - group.opacity]) })();
        Ok(self.ready(result)?.unwrap_or(back))
    }
    fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
        let result = self.scene.layer(self.r, self.packet, index, self.coordinate, self.accepted);
        match self.ready(result)? {Some(output)=>Ok(self.scene.own(output)),None=>self.clear()}
    }
    fn blend(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle, clipped: bool) -> Result<Self::Image, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        let result = self.scene.combine(self.r, front.index, back.index, layer.opacity, layer.blend, clipped, self.packet.blend_space);
        Ok(match self.ready(result)? {Some(output)=>self.scene.own(output),None=>back})
    }
    fn effect(&mut self, chain: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError> {
        let result = self.scene.effect(self.r, self.packet, chain, self.coordinate, input.index, self.accepted);
        Ok(match self.ready(result)? {Some(output)=>self.scene.own(output),None=>input})
    }
    fn has_content(&self, index: OccurrenceHandle) -> bool { has_content(self.r, self.packet.scene, index) }
    fn draw_normal(&mut self, index: OccurrenceHandle, output: &Self::Image) -> Result<bool, GpuRasterError> {
        let result = self.scene.draw_normal_layer(self.r, self.packet, index, self.coordinate, output.index, self.accepted);
        Ok(self.ready(result)?.unwrap_or(true))
    }
    fn checkpoint(&mut self, parent: Option<OccurrenceHandle>) -> Result<Option<Checkpoint<Self::Image>>, GpuRasterError> {
        if self.accepted.declaring() { return Ok(None); }
        let scene = self.packet.scene;
        let stop = stop_root(scene, parent, self.stop);
        let Some(handle) = scene.members(parent).find(|handle| {
            scene.visible(*handle) && scene.effect_owner(*handle).is_none()
                && scene.effect(*handle).is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && stop.is_none_or(|stop| scene.position(*handle) > scene.position(stop))
                && self.scene.images.output(*handle).is_some()
        }) else { return Ok(None); };
        let Some(pixels) = self.scene.images.output(handle) else { return Ok(None); };
        let bounds = pixels.plan.doc_bounds.translated(self.scene.images.doc_bounds.min);
        let result = self.scene.image_tile(self.r, pixels.view, bounds, self.coordinate);
        Ok(self.ready(result)?.map(|output|(handle,self.scene.own(output))))
    }
    fn presented_backdrop(&self) -> Option<OccurrenceHandle> { self.presented }
}

impl Tile<'_> {
    fn ready<T>(&mut self, result: Result<T, GpuRasterError>) -> Result<Option<T>, GpuRasterError> {
        match result {
            Err(GpuRasterError::NotReady) => {self.waiting=true;Ok(None)},
            result=>result.map(Some),
        }
    }
    fn weighted_sum(&mut self, front: &scratch::Lease, back: &scratch::Lease, weights: [f32; 2]) -> Result<scratch::Lease, GpuRasterError> {
        let output = self.scene.reserve(self.r)?;
        self.scene.draw(self.r, output, self.scene.pool[front.index].view.clone(), Some(self.scene.pool[back.index].view.clone()),
            [0., 0., 256., 256.], [16., weights[0], weights[1], 0.], false, Convert::None);
        Ok(self.scene.own(output))
    }
}

pub(super) fn support(scene: SceneView<'_>, level: u32) -> Option<u32> {
    if scene.order().iter().filter_map(|h|scene.effect(*h).map(|effect|(*h,effect)))
        .all(|(h,effect)|!scene.visible(h)||crate::effects::damage_radius(effect,level)==Some(0)) {return Some(0);}
    struct Support<'a> { scene: SceneView<'a>, level: u32 }
    impl Compositor for Support<'_> {
        type Image = Option<u32>;
        fn clear(&mut self) -> Result<Self::Image, GpuRasterError> { Ok(Some(0)) }
        fn discard(&mut self, _: Self::Image) {}
        fn duplicate(&mut self, image: &Self::Image) -> Result<Self::Image, GpuRasterError> { Ok(*image) }
        fn fade(&mut self, front: Self::Image, back: Self::Image, _: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> { Ok(front.zip(back).map(|(a,b)| a.max(b))) }
        fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
            if self.scene.occurrence(index).unwrap().kind() == LayerKind::Group { compose(self, self.scene, Some(index), None) }
            else if self.scene.effect(index).is_some() { self.effect(&[index], Some(0)) } else { Ok(Some(0)) }
        }
        fn blend(&mut self, front: Self::Image, back: Self::Image, _: OccurrenceHandle, _: bool) -> Result<Self::Image, GpuRasterError> { Ok(front.zip(back).map(|(a,b)| a.max(b))) }
        fn effect(&mut self, chain: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError> {
            Ok(input.and_then(|input| chain.iter().try_fold(input, |radius,h| radius.checked_add(crate::effects::damage_radius(self.scene.effect(*h).unwrap(),self.level)?))))
        }
        fn has_content(&self, _: OccurrenceHandle) -> bool { true }
    }
    compose(&mut Support { scene, level }, scene, scene.effect_input().and_then(|h| layer_core::composite_input_scope(scene,h)), scene.effect_input()).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{authored::*,color::source::rgba8_source};

    #[test]
    fn a_waiting_native_tile_requests_outer_and_nested_object_layers_without_publishing() {
        for passthrough in [false,true] {
            let mut artwork=Artwork::new([16;2]).unwrap();
            let owners:Vec<_>=(0..3).map(|index| {
                let source=layer_core::authored::Image::new(rgba8_source([16;2],|_,_|[index*60,30,90,255]));
                let object=artwork.objects.insert(PortableId::random(),ImageObject::new(source)).unwrap();
                artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(object),"Photo")).unwrap()
            }).collect();
            let root=artwork.compositions.get(artwork.root).unwrap().result;
            artwork.stacks.get_mut(root).unwrap().entries=owners.clone();
            let mut mask=layer_core::CoverageSnapshot::reveal_all(artwork.coverage.next_handle(),[16;2],[0;2]);
            let descriptor=artwork.compositions.get(artwork.root).unwrap().color.coverage_descriptor();
            let data=Arc::new(layer_core::raster::RasterData {tiles:[(layer_core::raster::TileKey {
                plane:layer_core::raster::RasterPlane::Mask,coordinate:[0;2]},layer_core::raster::RasterTile::backed(
                    layer_core::raster::TileBlob::encode(descriptor,&vec![128;descriptor.byte_len([PAGE_SIZE;2]).unwrap()]).unwrap()))].into(),..Default::default()});
            mask.source.raster=layer_core::raster::RasterRevision::backed((*data).clone());
            let mask_target=SourceTarget::Coverage(mask.use_.source);
            artwork.coverage.insert(PortableId::random(),mask.source).unwrap();
            artwork.occurrences.get_mut(owners[0]).unwrap().mask=Some(mask.use_);
            let mut doc=layer_core::Document::from_artwork(artwork).unwrap();
            let group=crate::artwork_sample_tests::add_group(&mut doc,owners[..2].to_vec(),0);
            if passthrough {
                let group=doc.artwork.occurrences.get_mut(group).unwrap();
                group.blend=layer_core::LayerBlend::PassThrough;group.opacity=0.5;
                crate::artwork_sample_tests::refresh(&mut doc);
            }
            let gpu=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            let mut r=WgpuRasterizer::native_capture_on_factory(gpu.factory.clone(),gpu.device.clone(),doc.composition().color).unwrap();
            drop(gpu);
            r.ensure_document_metadata([16;2],doc.scene()).unwrap();
            r.publish_cold_raster(mask_target,&data).unwrap();
            let mut scene=Scene::new(&r).unwrap();scene.object_display=true;
            let packet=crate::test_support::packet(doc.scene(),[16;2]);
            let declaration=scene.capture_declaration(&r,packet).unwrap();let mut accepted=scene.accept_capture(&mut r,packet,Some(crate::scene::Output::Artwork(None)),&declaration,|scene,r,accepted|tile(scene,r,packet,None,[0;2],accepted).map(drop)).unwrap();
            let result=tile(&mut scene,&r,packet,None,[0;2],&mut accepted);
            assert!(matches!(result,Err(GpuRasterError::NotReady)));
            let requested:std::collections::BTreeSet<_>=scene.object_results.request_windows().into_iter().map(|(_,owner,_)|owner).collect();
            assert_eq!(requested,owners.iter().copied().collect(),"passthrough={passthrough}");
            assert!(scene.jobs.iter().any(|job|matches!(job,Job::DecodedTile(_))),"a waiting source must still gather its own cold mask");
            assert!(scene.used.iter().all(|usage|!usage.occupied()),"a waiting tile must not publish or keep a scratch output");
        }
    }
}
