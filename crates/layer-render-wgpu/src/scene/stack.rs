use super::*;

pub(super) trait Compositor {
    type Image;
    fn clear(&mut self, paper: bool) -> Self::Image;
    fn discard(&mut self, image: Self::Image);
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
    let (mut output, mut stack) = checkpoint.map_or_else(
        || (c.clear(parent.is_none()), None), |(_, output, stack)| (output, stack),
    );
    let mut siblings = layers.iter().enumerate().rev()
        .filter(|(_, l)| l.properties.parent == parent && l.kind != LayerKind::Background && l.is_artwork())
        .filter(|(i, _)| cut.is_none_or(|cut| *i < cut)).peekable();
    while let Some((i, layer)) = siblings.next() {
        if let Some((stop, clipped)) = stop_before && stop == i {
            if clipped {
                c.discard(output);
                return Ok(stack.map_or_else(|| c.clear(false), |(pixels, _)| pixels));
            }
            if let Some((pixels, base)) = stack { output = c.blend(pixels, output, base, false)?; }
            return Ok(output);
        }
        if layer.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment) {
            if !layer.properties.clipped && let Some((pixels, base)) = stack.take() {
                output = c.blend(pixels, output, base, false)?;
            }
            if !layer.visible { continue; }
            let mut chain = vec![i];
            if direct_effect_mask(layers, layer) && !layer.effect.as_ref().unwrap().program.image_boundary() {
                while let Some((j, _)) = siblings.peek().filter(|(j, next)| {
                    stop_before.is_none_or(|(stop, _)| *j > stop) && fuses_after(layers, layer, next, chain.len())
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
    Ok(output)
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
        self.scene.alloc(self.r, wgpu::Color {
            r: f64::from(c[0] * c[3]), g: f64::from(c[1] * c[3]), b: f64::from(c[2] * c[3]), a: f64::from(c[3]),
        })
    }
    fn discard(&mut self, image: usize) { self.scene.free(image); }
    fn layer(&mut self, index: usize) -> Result<usize, GpuRasterError> {
        self.scene.layer(self.r, self.packet, index, self.coordinate)
    }
    fn blend(&mut self, front: usize, back: usize, index: usize, clipped: bool) -> Result<usize, GpuRasterError> {
        let layer = &self.packet.layers[index];
        Ok(self.scene.combine(self.r, front, back, layer.opacity, layer.properties.blend, clipped))
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
        let (i, layer) = self.packet.layers.iter().enumerate().find(|(i, l)| {
            self.scene.cached_composition() && l.visible && l.properties.parent == parent
                && l.effect.as_ref().is_some_and(|e| e.program.kind == layer_core::EffectKind::Adjustment)
                && self.scene.stop_before.is_none_or(|(stop, _)| *i > stop)
                && self.scene.images.checkpoint(*i, l).is_some()
        })?;
        let (pixels, pending) = self.scene.images.checkpoint(i, layer)?;
        let bounds = self.scene.images.bounds;
        let output = self.scene.image_tile(self.r, pixels, bounds, self.coordinate);
        let pending = pending.map(|(pixels, base)| (self.scene.image_tile(self.r, pixels, bounds, self.coordinate), base));
        Some((i, output, pending))
    }
}
