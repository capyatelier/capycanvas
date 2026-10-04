use super::*;

type Checkpoint<I> = (OccurrenceHandle, I, Option<(I, OccurrenceHandle)>);

pub(super) trait Compositor {
    type Image;
    fn clear(&mut self) -> Self::Image;
    fn discard(&mut self, image: Self::Image);
    fn duplicate(&mut self, image: &Self::Image) -> Self::Image;
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError>;
    fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError>;
    fn blend(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle, clipped: bool) -> Result<Self::Image, GpuRasterError>;
    fn effect(&mut self, chain: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError>;
    fn has_content(&self, index: OccurrenceHandle) -> bool;
    fn draw_normal(&mut self, _index: OccurrenceHandle, _output: &Self::Image) -> Result<bool, GpuRasterError> { Ok(false) }
    fn checkpoint(&mut self, _parent: Option<OccurrenceHandle>) -> Option<Checkpoint<Self::Image>> { None }
}

pub(super) fn has_content(r: &WgpuRasterizer, scene: layer_core::SceneView<'_>, handle: OccurrenceHandle) -> bool {
    let occurrence = scene.occurrence(handle).unwrap();
    matches!(occurrence.kind(), LayerKind::Group | LayerKind::Effect)
        || scene.paint_source(handle).is_some_and(|paint| paint.original.is_some())
        || scene.source_target(handle).is_some_and(|target| {
            r.native_color_coordinates(target).next().is_some()
                || r.paint_layers.iter().any(|stored| stored.id == target && !stored.pages.is_empty())
                || r.preview_layer_id == Some(target)
        })
}

pub(super) fn compose<C: Compositor>(
    c: &mut C, scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop_before: Option<(OccurrenceHandle, bool)>,
) -> Result<C::Image, GpuRasterError> {
    let checkpoint = c.checkpoint(parent);
    let cut = checkpoint.as_ref().map(|(index, ..)| *index);
    let (output, stack) = checkpoint.map_or_else(
        || (c.clear(), None), |(_, output, stack)| (output, stack),
    );
    let (Flow::Done(output) | Flow::Stopped(output)) =
        group_into(c, scene, parent, stop_before, cut, output, stack)?;
    Ok(output)
}

enum Flow<I> { Done(I), Stopped(I) }

fn stop_root(scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop: Option<(OccurrenceHandle, bool)>) -> Option<OccurrenceHandle> {
    let (mut root, _) = stop?;
    for _ in 0..scene.order().len() {
        let up = scene.evaluation_parent(root);
        if up == parent { return Some(root); }
        root = up?;
    }
    None
}

fn group_into<C: Compositor>(
    c: &mut C, scene: layer_core::SceneView<'_>, parent: Option<OccurrenceHandle>, stop_before: Option<(OccurrenceHandle, bool)>,
    cut: Option<OccurrenceHandle>, mut output: C::Image, mut stack: Option<(C::Image, OccurrenceHandle)>,
) -> Result<Flow<C::Image>, GpuRasterError> {
    let stop = stop_root(scene, parent, stop_before);
    let mut siblings = scene.members(parent).rev()
        .filter(|handle| {
            let occurrence = scene.occurrence(*handle).unwrap();
            occurrence.is_artwork() && scene.includes(*handle)
                && cut.is_none_or(|cut| scene.position(*handle) < scene.position(cut))
        }).peekable();
    while let Some(i) = siblings.next() {
        let layer = scene.occurrence(i).unwrap();
        if let Some((stop, clipped)) = stop_before && stop == i {
            if clipped {
                c.discard(output);
                return Ok(Flow::Stopped(stack.map_or_else(|| c.clear(), |(pixels, _)| pixels)));
            }
            if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
            return Ok(Flow::Stopped(output));
        }
        if scene.effect(i).is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment) {
            if !scene.effective_clipped(i) && let Some((pixels, base)) = stack.take() {
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
            if scene.effective_clipped(i) {
                if let Some((pixels, base)) = stack.take() { stack = Some((c.effect(&chain, pixels)?, base)); }
            } else { output = c.effect(&chain, output)?; }
            continue;
        }
        if !scene.effective_clipped(i) {
            if let Some((pixels, base)) = stack.take() { output = c.blend(pixels, output, base, false)?; }
            if layer.kind() == LayerKind::Group && layer.blend == layer_core::LayerBlend::PassThrough {
                if !scene.visible(i) { continue; }
                let fade = layer.opacity != 1. || layer.mask.as_ref().is_some_and(|m| m.enabled);
                let (input, backdrop) = if fade { (c.duplicate(&output), Some(output)) } else { (output, None) };
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
            if scene.visible(i) && !clips_above && c.draw_normal(i, &output)? { continue; }
            if scene.visible(i) && c.has_content(i) { stack = Some((c.layer(i)?, i)); }
        } else if scene.visible(i) && let Some((pixels, base)) = stack.take() {
            let source = c.layer(i)?;
            stack = Some((c.blend(source, pixels, i, true)?, base));
        }
    }
    if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
    Ok(Flow::Done(output))
}

pub(super) fn tile(
    scene: &mut Scene, r: &WgpuRasterizer, packet: FramePacket<'_>, parent: Option<OccurrenceHandle>, coordinate: [u32; 2],
) -> Result<usize, GpuRasterError> {
    let stop = scene.stop_before;
    compose(&mut Tile { scene, r, packet, coordinate }, packet.scene, parent, stop)
}

struct Tile<'a> {
    scene: &'a mut Scene,
    r: &'a WgpuRasterizer,
    packet: FramePacket<'a>,
    coordinate: [u32; 2],
}
impl Compositor for Tile<'_> {
    type Image = usize;
    fn clear(&mut self) -> usize { self.scene.alloc(self.r, wgpu::Color::TRANSPARENT) }
    fn discard(&mut self, image: usize) { self.scene.free(image); }
    fn duplicate(&mut self, image: &usize) -> usize {
        let output = self.scene.reserve(self.r);
        self.scene.draw(self.r, output, self.scene.pool[*image].view.clone(), None,
            [0., 0., 256., 256.], [1., 1., 0., 0.], false, Convert::None);
        output
    }
    fn fade(&mut self, front: usize, back: usize, index: OccurrenceHandle) -> Result<usize, GpuRasterError> {
        let group = self.packet.scene.occurrence(index).unwrap();
        let output = if let Some((mask, source)) = self.packet.scene.mask(index).filter(|(m, _)| m.enabled) {
            let coverage = self.scene.mask_at(self.r, mask, source, self.packet.scene.target_geometry(SourceTarget::Coverage(mask.source)),
                source.domain, self.coordinate)?;
            let change = self.weighted_sum(front, back, [1., -1.]);
            let masked = self.scene.reserve(self.r);
            self.scene.draw(self.r, masked, self.scene.pool[change].view.clone(), Some(self.scene.pool[coverage].view.clone()),
                [0., 0., 256., 256.], [3., 1., 0., 0.], false, Convert::None);
            self.scene.free(change);
            self.scene.free(coverage);
            let output = self.weighted_sum(masked, back, [group.opacity, 1.]);
            self.scene.free(masked);
            output
        } else { self.weighted_sum(front, back, [group.opacity, 1. - group.opacity]) };
        self.scene.free(front);
        self.scene.free(back);
        Ok(output)
    }
    fn layer(&mut self, index: OccurrenceHandle) -> Result<usize, GpuRasterError> {
        self.scene.layer(self.r, self.packet, index, self.coordinate)
    }
    fn blend(&mut self, front: usize, back: usize, index: OccurrenceHandle, clipped: bool) -> Result<usize, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        Ok(self.scene.combine(self.r, front, back, layer.opacity, layer.blend, clipped, self.packet.blend_space))
    }
    fn effect(&mut self, chain: &[OccurrenceHandle], input: usize) -> Result<usize, GpuRasterError> {
        self.scene.effect(self.r, self.packet, chain, self.coordinate, input)
    }
    fn has_content(&self, index: OccurrenceHandle) -> bool {
        has_content(self.r, self.packet.scene, index)
    }
    fn draw_normal(&mut self, index: OccurrenceHandle, output: &usize) -> Result<bool, GpuRasterError> {
        self.scene.draw_normal_layer(self.r, self.packet, index, self.coordinate, *output)
    }
    fn checkpoint(&mut self, parent: Option<OccurrenceHandle>) -> Option<(OccurrenceHandle, usize, Option<(usize, OccurrenceHandle)>)> {
        let scene = self.packet.scene;
        let stop = stop_root(scene, parent, self.scene.stop_before);
        let handle = scene.members(parent).find(|handle| {
            scene.occurrence(*handle).unwrap().visible
                && scene.effect(*handle).is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && stop.is_none_or(|stop| scene.position(*handle) > scene.position(stop))
                && self.scene.images.checkpoint(*handle, scene).is_some()
        })?;
        let (pixels, pending) = self.scene.images.checkpoint(handle, scene)?;
        let bounds = self.scene.images.bounds;
        let output = self.scene.image_tile(self.r, pixels, bounds, self.coordinate);
        let pending = pending.map(|(pixels, base)| (self.scene.image_tile(self.r, pixels, bounds, self.coordinate), base));
        Some((handle, output, pending))
    }
}

impl Tile<'_> {
    fn weighted_sum(&mut self, front: usize, back: usize, weights: [f32; 2]) -> usize {
        let output = self.scene.reserve(self.r);
        self.scene.draw(self.r, output, self.scene.pool[front].view.clone(), Some(self.scene.pool[back].view.clone()),
            [0., 0., 256., 256.], [16., weights[0], weights[1], 0.], false, Convert::None);
        output
    }
}
