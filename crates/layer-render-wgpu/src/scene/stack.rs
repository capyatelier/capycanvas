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
    scene: &mut Scene, r: &WgpuRasterizer, packet: FramePacket<'_>, parent: Option<OccurrenceHandle>, coordinate: [u32; 2],
) -> Result<usize, GpuRasterError> {
    let stop = scene.stop_before.take();
    let presented = scene.presented_backdrop;
    let result = compose(&mut Tile { scene, r, packet, coordinate, stop, presented }, packet.scene, parent, stop).map(|lease| scene.keep(lease));
    scene.stop_before = stop;
    result
}

struct Tile<'a> {
    scene: &'a mut Scene,
    r: &'a WgpuRasterizer,
    packet: FramePacket<'a>,
    coordinate: [u32; 2],
    stop: Option<OccurrenceHandle>,
    presented: Option<OccurrenceHandle>,
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
        if let Some((mask, source)) = self.packet.scene.mask(index).filter(|(m, _)| m.enabled) {
            let coverage = self.scene.mask_at(self.r, mask, source, self.packet.scene.target_offset(SourceTarget::Coverage(mask.source)), self.coordinate)?;
            let coverage = self.scene.own(coverage);
            let change = self.weighted_sum(&front, &back, [1., -1.])?;
            let masked = self.scene.reserve(self.r)?;
            let masked = self.scene.own(masked);
            self.scene.draw(self.r, masked.index, self.scene.pool[change.index].view.clone(), Some(self.scene.pool[coverage.index].view.clone()),
                [0., 0., 256., 256.], [3., 1., 0., 0.], false, Convert::None);
            self.weighted_sum(&masked, &back, [group.opacity, 1.])
        } else { self.weighted_sum(&front, &back, [group.opacity, 1. - group.opacity]) }
    }
    fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
        let output = self.scene.layer(self.r, self.packet, index, self.coordinate)?;
        Ok(self.scene.own(output))
    }
    fn blend(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle, clipped: bool) -> Result<Self::Image, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        let output = self.scene.combine(self.r, front.index, back.index, layer.opacity, layer.blend, clipped, self.packet.blend_space)?;
        Ok(self.scene.own(output))
    }
    fn effect(&mut self, chain: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError> {
        let output = self.scene.effect(self.r, self.packet, chain, self.coordinate, input.index)?;
        Ok(self.scene.own(output))
    }
    fn has_content(&self, index: OccurrenceHandle) -> bool { has_content(self.r, self.packet.scene, index) }
    fn draw_normal(&mut self, index: OccurrenceHandle, output: &Self::Image) -> Result<bool, GpuRasterError> {
        self.scene.draw_normal_layer(self.r, self.packet, index, self.coordinate, output.index)
    }
    fn checkpoint(&mut self, parent: Option<OccurrenceHandle>) -> Result<Option<Checkpoint<Self::Image>>, GpuRasterError> {
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
        let output = self.scene.image_tile(self.r, pixels.view, bounds, self.coordinate)?;
        Ok(Some((handle, self.scene.own(output))))
    }
    fn presented_backdrop(&self) -> Option<OccurrenceHandle> { self.presented }
}

impl Tile<'_> {
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
