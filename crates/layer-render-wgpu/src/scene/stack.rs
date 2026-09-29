use super::*;

pub(super) trait Compositor {
    type Image;
    fn clear(&mut self, paper: bool) -> Self::Image;
    fn discard(&mut self, image: Self::Image);
    fn duplicate(&mut self, image: &Self::Image) -> Self::Image;
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: usize) -> Result<Self::Image, GpuRasterError>;
    fn layer(&mut self, index: usize) -> Result<Self::Image, GpuRasterError>;
    fn blend(&mut self, front: Self::Image, back: Self::Image, index: usize, clipped: bool) -> Result<Self::Image, GpuRasterError>;
    fn effect(&mut self, chain: &[usize], input: Self::Image) -> Result<Self::Image, GpuRasterError>;
    fn has_content(&self, index: usize) -> bool;
    fn draw_normal(&mut self, _index: usize, _output: &Self::Image) -> Result<bool, GpuRasterError> { Ok(false) }
    fn checkpoint(&mut self, _parent: Option<LayerId>) -> Option<(usize, Self::Image, Option<(Self::Image, usize)>)> { None }
}

pub(super) fn has_content(r: &WgpuRasterizer, layer: &Layer) -> bool {
    matches!(layer.kind, LayerKind::Group | LayerKind::Effect) || layer.source.is_some()
        || r.native_color_coordinates(layer.id).next().is_some()
        || r.paint_layers.iter().any(|l| l.id == layer.id && !l.pages.is_empty())
        || r.preview_layer_id == Some(layer.id)
}

pub(super) fn compose<C: Compositor>(
    c: &mut C, layers: &[Layer], parent: Option<LayerId>, stop_before: Option<(usize, bool)>,
) -> Result<C::Image, GpuRasterError> {
    let checkpoint = c.checkpoint(parent);
    let cut = checkpoint.as_ref().map(|(index, ..)| *index);
    let (output, stack) = checkpoint.map_or_else(
        || (c.clear(parent.is_none()), None), |(_, output, stack)| (output, stack),
    );
    let (Flow::Done(output) | Flow::Stopped(output)) =
        group_into(c, layers, parent, stop_before, cut, output, stack)?;
    Ok(output)
}

enum Flow<I> { Done(I), Stopped(I) }

fn stop_root(layers: &[Layer], parent: Option<LayerId>, stop: Option<(usize, bool)>) -> Option<usize> {
    let (mut root, _) = stop?;
    for _ in 0..layers.len() {
        let up = layers[root].properties.parent;
        if up == parent { return Some(root); }
        root = layers.iter().position(|l| Some(l.id) == up)?;
    }
    None
}

fn group_into<C: Compositor>(
    c: &mut C, layers: &[Layer], parent: Option<LayerId>, stop_before: Option<(usize, bool)>,
    cut: Option<usize>, mut output: C::Image, mut stack: Option<(C::Image, usize)>,
) -> Result<Flow<C::Image>, GpuRasterError> {
    let stop = stop_root(layers, parent, stop_before);
    let mut siblings = layers.iter().enumerate().rev()
        .filter(|(_, l)| l.properties.parent == parent && l.kind != LayerKind::Background && l.is_artwork())
        .filter(|(i, _)| cut.is_none_or(|cut| *i < cut)).peekable();
    while let Some((i, layer)) = siblings.next() {
        if let Some((stop, clipped)) = stop_before && stop == i {
            if clipped {
                c.discard(output);
                return Ok(Flow::Stopped(stack.map_or_else(|| c.clear(false), |(pixels, _)| pixels)));
            }
            if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
            return Ok(Flow::Stopped(output));
        }
        if layer.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment) {
            if !layer.properties.clipped && let Some((pixels, base)) = stack.take() {
                output = c.blend(pixels, output, base, false)?;
            }
            if !layer.visible { continue; }
            let mut chain = vec![i];
            if direct_effect_mask(layers, layer) && !layer.effect.as_ref().unwrap().program.image_boundary() {
                while let Some((j, _)) = siblings.peek().filter(|(j, next)| {
                    stop.is_none_or(|stop| *j > stop) && fuses_after(layers, layer, next, chain.len())
                }) {
                    chain.push(*j);
                    siblings.next();
                }
            }
            if layer.properties.clipped {
                if let Some((pixels, base)) = stack.take() { stack = Some((c.effect(&chain, pixels)?, base)); }
            } else { output = c.effect(&chain, output)?; }
            continue;
        }
        if !layer.properties.clipped {
            if let Some((pixels, base)) = stack.take() { output = c.blend(pixels, output, base, false)?; }
            if layer.passes_through() {
                if !layer.visible { continue; }
                let fade = layer.opacity != 1. || layer.mask.as_ref().is_some_and(|m| m.enabled);
                let (input, backdrop) = if fade { (c.duplicate(&output), Some(output)) } else { (output, None) };
                match group_into(c, layers, Some(layer.id), stop_before, None, input, None)? {
                    Flow::Done(result) => output = if let Some(backdrop) = backdrop { c.fade(result, backdrop, i)? } else { result },
                    Flow::Stopped(result) => {
                        if let Some(backdrop) = backdrop { c.discard(backdrop); }
                        return Ok(Flow::Stopped(result));
                    }
                }
                continue;
            }
            let clips_above = layers[..i].iter().rev().find(|l| l.properties.parent == parent)
                .is_some_and(|l| l.properties.clipped);
            if layer.visible && !clips_above && c.draw_normal(i, &output)? { continue; }
            if layer.visible && c.has_content(i) { stack = Some((c.layer(i)?, i)); }
        } else if layer.visible && let Some((pixels, base)) = stack.take() {
            let source = c.layer(i)?;
            stack = Some((c.blend(source, pixels, i, true)?, base));
        }
    }
    if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
    Ok(Flow::Done(output))
}

pub(super) fn tile(
    scene: &mut Scene, r: &WgpuRasterizer, packet: FramePacket<'_>, parent: Option<LayerId>, coordinate: [u32; 2],
) -> Result<usize, GpuRasterError> {
    let stop = scene.stop_before;
    compose(&mut Tile { scene, r, packet, coordinate }, packet.layers, parent, stop)
}

struct Tile<'a> {
    scene: &'a mut Scene,
    r: &'a WgpuRasterizer,
    packet: FramePacket<'a>,
    coordinate: [u32; 2],
}
impl Compositor for Tile<'_> {
    type Image = usize;
    fn clear(&mut self, paper: bool) -> usize {
        let c = if paper { self.packet.view.background_rgba_linear } else { [0.; 4] };
        self.scene.alloc(self.r, composite_color(self.r, self.packet, c))
    }
    fn discard(&mut self, image: usize) { self.scene.free(image); }
    fn duplicate(&mut self, image: &usize) -> usize {
        let output = self.scene.reserve(self.r);
        self.scene.draw(self.r, output, self.scene.pool[*image].view.clone(), None,
            [0., 0., 256., 256.], [1., 1., 0., 0.], false, Convert::None);
        output
    }
    fn fade(&mut self, front: usize, back: usize, index: usize) -> Result<usize, GpuRasterError> {
        let group = &self.packet.layers[index];
        let output = if let Some(mask) = group.mask.as_ref().filter(|m| m.enabled) {
            let coverage = self.scene.mask_at(self.r, mask, layer_core::target_transform(self.packet.layers, mask.id),
                group.local_extent(self.packet.document_extent), self.coordinate)?;
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
    fn layer(&mut self, index: usize) -> Result<usize, GpuRasterError> {
        self.scene.layer(self.r, self.packet, index, self.coordinate)
    }
    fn blend(&mut self, front: usize, back: usize, index: usize, clipped: bool) -> Result<usize, GpuRasterError> {
        let layer = &self.packet.layers[index];
        Ok(self.scene.combine(self.r, front, back, layer.opacity, layer.properties.blend, clipped, self.packet.blend_space))
    }
    fn effect(&mut self, chain: &[usize], input: usize) -> Result<usize, GpuRasterError> {
        self.scene.effect(self.r, self.packet, chain, self.coordinate, input)
    }
    fn has_content(&self, index: usize) -> bool {
        has_content(self.r, &self.packet.layers[index])
    }
    fn draw_normal(&mut self, index: usize, output: &usize) -> Result<bool, GpuRasterError> {
        self.scene.draw_normal_layer(self.r, self.packet, index, self.coordinate, *output)
    }
    fn checkpoint(&mut self, parent: Option<LayerId>) -> Option<(usize, usize, Option<(usize, usize)>)> {
        let stop = stop_root(self.packet.layers, parent, self.scene.stop_before);
        let (i, layer) = self.packet.layers.iter().enumerate().find(|(i, l)| {
            self.scene.cached_composition() && l.visible && l.properties.parent == parent
                && l.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && stop.is_none_or(|stop| *i > stop)
                && self.scene.images.checkpoint(*i, l).is_some()
        })?;
        let (pixels, pending) = self.scene.images.checkpoint(i, layer)?;
        let bounds = self.scene.images.bounds;
        let output = self.scene.image_tile(self.r, pixels, bounds, self.coordinate);
        let pending = pending.map(|(pixels, base)| (self.scene.image_tile(self.r, pixels, bounds, self.coordinate), base));
        Some((i, output, pending))
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
