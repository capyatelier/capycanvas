use super::*;
use std::collections::{HashMap, HashSet};

pub(super) type Node = Arc<Expression>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Expression {
    Color([u32; 4]),
    Source { id: LayerId, placement: [u32; 6], extent: [u32; 2], outside: u32 },
    Opacity { input: Node, opacity: u32 },
    Combine { front: Node, back: Node, blend: u32, flags: u32 },
    Effect { input: Node, chain: Vec<(LayerId, u64, u32)>, masks: Vec<Option<Node>>, radius: Option<u32> },
}
impl Expression {
    fn color(value: [f32; 4]) -> Node { Arc::new(Self::Color(value.map(f32::to_bits))) }
    fn combine(front: Node, back: Node, blend: layer_core::LayerBlend, flags: u32) -> Node {
        if blend == layer_core::LayerBlend::Normal && flags == 0 {
            if matches!(front.as_ref(), Self::Color(c) if *c == [0; 4]) { return back; }
            if matches!(back.as_ref(), Self::Color(c) if *c == [0; 4]) { return front; }
        }
        Arc::new(Self::Combine { front, back, blend: blend as u32, flags })
    }
    fn over(nodes: &[Node]) -> Node {
        if nodes.is_empty() { return Self::color([0.; 4]); }
        if nodes.len() == 1 { return nodes[0].clone(); }
        let split = nodes.len().next_power_of_two() / 2;
        Self::combine(Self::over(&nodes[split..]), Self::over(&nodes[..split]), layer_core::LayerBlend::Normal, 0)
    }
    fn opacity(input: Node, opacity: f32) -> Node {
        if let Self::Color(c) = input.as_ref() { return Self::color(c.map(|v| f32::from_bits(v) * opacity)); }
        if opacity == 1. { input } else { Arc::new(Self::Opacity { input, opacity: opacity.to_bits() }) }
    }
    fn cost(&self) -> (u32, u32) {
        match self {
            Self::Color(_) => (0, 0),
            Self::Source { placement, .. } => {
                let work = u32::from(*placement != layer_core::Affine::IDENTITY.0.map(f32::to_bits));
                (work, work)
            }
            Self::Opacity { input, .. } => input.cost(),
            Self::Combine { front, back, .. } => {
                let (a, x) = front.cost();
                let (b, y) = back.cost();
                (a + b + 1, x.max(y).max(x.min(y) + 1).max(3))
            }
            Self::Effect { input, masks, .. } => {
                let (work, scratch) = input.cost();
                let (mask_work, mask_scratch, count) = masks.iter().flatten().fold((0, 0, 0), |(w, s, n), mask| {
                    let (work, scratch) = mask.cost();
                    (w + work, s.max(scratch), n + 1)
                });
                (work + 1 + mask_work, scratch.max(mask_scratch + count + 2))
            }
        }
    }
    pub(super) fn damage(&self, sources: &Sources, plan: display_mips::Plan) -> PixelRect {
        match self {
            Self::Color(_) => PixelRect::EMPTY,
            Self::Source { id, placement, .. } => sources.entries.get(id).map_or(plan.bounds, |source| {
                if source.damage.is_empty() { return PixelRect::EMPTY; }
                let placement = layer_core::Affine(placement.map(f32::from_bits));
                if placement == layer_core::Affine::IDENTITY { return source.damage; }
                let local = source.damage.expand(1 << source_level(plan.level, placement), source.extent);
                pixel_rect(placement.bounds(local.to_rect()), plan.extent)
            }),
            Self::Opacity { input, .. } => input.damage(sources, plan),
            Self::Combine { front, back, .. } => front.damage(sources, plan).union(back.damage(sources, plan)),
            Self::Effect { input, masks, radius, .. } => masks.iter().flatten().fold(
                crate::effects::dependency(input.damage(sources, plan), *radius, plan), |r, n| r.union(n.damage(sources, plan))),
        }
    }
    pub(super) fn required(&self, region: PixelRect, plan: display_mips::Plan) -> PixelRect {
        match self {
            Self::Color(_) => PixelRect::EMPTY,
            Self::Source { .. } => region,
            Self::Opacity { input, .. } => input.required(region, plan),
            Self::Combine { front, back, .. } => front.required(region, plan).union(back.required(region, plan)),
            Self::Effect { input, masks, radius, .. } => masks.iter().flatten().fold(
                input.required(crate::effects::dependency(region, *radius, plan), plan), |r, n| r.union(n.required(region, plan))),
        }
    }
    fn visit(node: &Node, all: &mut HashSet<Node>) {
        if !all.insert(node.clone()) { return; }
        match node.as_ref() {
            Self::Opacity { input, .. } => Self::visit(input, all),
            Self::Combine { front, back, .. } => { Self::visit(front, all); Self::visit(back, all); }
            Self::Effect { input, masks, .. } => {
                Self::visit(input, all);
                for mask in masks.iter().flatten() { Self::visit(mask, all); }
            }
            _ => {}
        }
    }
    fn deferred(&self) -> bool {
        match self {
            Self::Source { placement, .. } => *placement != layer_core::Affine::IDENTITY.0.map(f32::to_bits),
            Self::Opacity { input, .. } => input.deferred(),
            Self::Combine { front, back, blend: 0, flags: 0 } => matches!(back.as_ref(), Self::Color(_)) && front.deferred(),
            _ => false,
        }
    }
}

pub(super) struct Branch {
    image: Option<Image>,
    valid: BTreeSet<[u32; 2]>,
}
#[derive(Default)]
pub(super) struct Graph {
    pub root: Option<Node>,
    branches: HashMap<Node, Branch>,
    effects: HashMap<LayerId, (metadata::Metadata, u64)>,
    revision: u64,
    blend_space: layer_core::BlendSpace,
}
impl Graph {
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, plan: display_mips::Plan, budget: u64) -> Result<(), GpuRasterError> {
        if std::mem::replace(&mut self.blend_space, packet.blend_space) != packet.blend_space { self.branches.clear(); }
        self.effects.retain(|id, _| packet.layers.iter().any(|l| l.id == *id && l.effect.is_some()));
        for layer in packet.layers.iter().filter(|l| l.effect.is_some()) {
            let metadata = metadata::Metadata::new(layer);
            if self.effects.get(&layer.id).is_none_or(|(old, _)| *old != metadata) {
                self.revision += 1;
                self.effects.insert(layer.id, (metadata, self.revision));
            }
        }
        let mut builder = Builder { packet, sources, effects: &self.effects, level: plan.level, space: r.device.working_space() };
        let output = stack::compose(&mut builder, packet.layers, None, None)?;
        let mut root = Expression::over(&output);
        for layer in packet.layers {
            if layer.mask.as_ref().is_some_and(|m| m.enabled && m.show_area) {
                root = Expression::combine(builder.source(layer, true), root, layer_core::LayerBlend::Normal, 64);
            }
        }
        let mut all = HashSet::new();
        Expression::visit(&root, &mut all);
        let mut eligible: Vec<_> = all.into_iter().filter(|n| !Arc::ptr_eq(n, &root)
            && matches!(n.as_ref(), Expression::Combine { .. } | Expression::Effect { .. })).collect();
        eligible.sort_by_cached_key(|n| (n.damage(sources, plan).area(), std::cmp::Reverse(n.cost().0), n.clone()));
        eligible.truncate((budget / plan.level_bytes(plan.level)) as usize);
        let wanted: HashSet<_> = eligible.into_iter().collect();
        self.branches.retain(|node, _| wanted.contains(node));
        for node in wanted {
            let dirty = node.damage(sources, plan);
            let branch = self.branches.entry(node).or_insert_with(|| Branch { image: None, valid: BTreeSet::new() });
            branch.valid.retain(|c| page_rect(*c).intersect(dirty).is_empty());
        }
        self.root = Some(root);
        Ok(())
    }
    pub fn reserved_bytes(&self, plan: display_mips::Plan) -> u64 { self.branches.len() as u64 * plan.level_bytes(plan.level) }
    pub fn storage_bytes(&self) -> u64 {
        self.branches.values().filter_map(|b| b.image.as_ref()).map(|i| texture_bytes(&i.texture)).sum()
    }
}

struct Builder<'a> {
    packet: FramePacket<'a>,
    sources: &'a Sources,
    effects: &'a HashMap<LayerId, (metadata::Metadata, u64)>,
    level: u32,
    space: layer_core::color::RgbSpace,
}
impl Builder<'_> {
    fn source(&self, layer: &Layer, mask: bool) -> Node {
        let id = if mask { layer.mask.as_ref().unwrap().id } else { layer.id };
        if !self.sources.entries.contains_key(&id) { return Expression::color([0.; 4]); }
        let outside = layer.mask.as_ref().filter(|_| mask).map_or(0., |m| if m.inverted { 1. - m.default_coverage } else { m.default_coverage });
        Arc::new(Expression::Source { id, placement: layer_core::target_transform(self.packet.layers, id).0.map(f32::to_bits),
            extent: layer.local_extent(self.packet.document_extent), outside: outside.to_bits() })
    }
}
impl stack::Compositor for Builder<'_> {
    type Image = Vec<Node>;
    fn clear(&mut self, paper: bool) -> Self::Image {
        if !paper { return Vec::new(); }
        let p = self.packet.view.background_rgba_linear;
        vec![Expression::color(self.packet.blend_space.composite(self.space, [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]))]
    }
    fn discard(&mut self, _: Self::Image) {}
    fn duplicate(&mut self, image: &Self::Image) -> Self::Image { image.clone() }
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: usize) -> Result<Self::Image, GpuRasterError> {
        let layer = &self.packet.layers[index];
        let back = Expression::over(&back);
        let front = Expression::over(&front);
        let (front, back) = if layer.mask.as_ref().is_some_and(|m| m.enabled) {
            let change = Expression::combine(front, Expression::opacity(back.clone(), -1.), layer_core::LayerBlend::Normal, 128);
            let masked = Expression::combine(change, self.source(layer, true), layer_core::LayerBlend::Normal, 32);
            (Expression::opacity(masked, layer.opacity), back)
        } else { (Expression::opacity(front, layer.opacity), Expression::opacity(back, 1. - layer.opacity)) };
        Ok(vec![Expression::combine(front, back, layer_core::LayerBlend::Normal, 128)])
    }
    fn layer(&mut self, index: usize) -> Result<Self::Image, GpuRasterError> {
        let layer = &self.packet.layers[index];
        let output = if layer.kind == LayerKind::Group {
            stack::compose(self, self.packet.layers, Some(layer.id), None)?
        } else if layer.kind == LayerKind::Effect {
            self.effect(&[index], Vec::new())?
        } else { vec![self.source(layer, false)] };
        if layer.mask.as_ref().is_some_and(|m| m.enabled) {
            Ok(vec![Expression::combine(Expression::over(&output), self.source(layer, true), layer_core::LayerBlend::Normal, 32)])
        } else { Ok(output) }
    }
    fn blend(&mut self, front: Self::Image, mut back: Self::Image, index: usize, clipped: bool) -> Result<Self::Image, GpuRasterError> {
        let layer = &self.packet.layers[index];
        let front = if layer.opacity == 1. { front } else { vec![Expression::opacity(Expression::over(&front), layer.opacity)] };
        if layer.properties.blend == layer_core::LayerBlend::Normal && !clipped {
            back.extend(front);
            Ok(back)
        } else {
            Ok(vec![Expression::combine(Expression::over(&front), Expression::over(&back), layer.properties.blend, if clipped { 16 } else { 0 })])
        }
    }
    fn effect(&mut self, indices: &[usize], input: Self::Image) -> Result<Self::Image, GpuRasterError> {
        let radius = indices.iter().try_fold(0u32, |radius, i| radius.checked_add(
            crate::effects::damage_radius(self.packet.layers[*i].effect.as_ref().unwrap(), self.level)?));
        let chain = indices.iter().map(|i| {
            let layer = &self.packet.layers[*i];
            let effect = layer.effect.as_ref().unwrap();
            (layer.id, self.effects[&layer.id].1, if effect.animated() { self.packet.time_seconds.to_bits() } else { 0 })
        }).collect();
        let masks = indices.iter().map(|i| {
            let layer = &self.packet.layers[*i];
            (layer.effect.as_ref().unwrap().program.kind == layer_core::EffectKind::Adjustment
                && layer.mask.as_ref().is_some_and(|m| m.enabled)).then(|| self.source(layer, true))
        }).collect();
        Ok(vec![Arc::new(Expression::Effect { input: Expression::over(&input), chain, masks, radius })])
    }
    fn has_content(&self, index: usize) -> bool {
        let layer = &self.packet.layers[index];
        matches!(layer.kind, LayerKind::Group | LayerKind::Paint | LayerKind::Effect)
    }
}

impl Evaluator<'_> {
    pub(super) fn evaluate_root(&mut self, node: &Node) -> Result<Value, GpuRasterError> {
        let deferred = if self.cache.plan.level > 0 && node.deferred() && self.cache.plan.bounds == PixelRect::full(self.cache.plan.extent)
            && self.r.transform_preview.is_none() { Some(self.evaluate(node)?) } else { None };
        if matches!(deferred, Some(Value::Placed(_))) { return Ok(deferred.unwrap()); }
        if self.cache.output.is_empty() { self.cache.allocate(self.r); }
        self.cache.used[0] = true;
        let view = self.cache.output[0].view.clone();
        let output = Target { view, slot: Some(Slot::Cache(0)), plan: self.cache.plan };
        let value = match deferred { Some(value) => value, None => self.evaluate_into(node, Some(output.clone()))? };
        self.materialize(value, Some(output))
    }
    pub(super) fn evaluate(&mut self, node: &Node) -> Result<Value, GpuRasterError> {
        self.evaluate_into(node, None)
    }
    fn evaluate_into(&mut self, node: &Node, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let region = self.region;
        if let Some(branch) = self.cache.graph.branches.get(node)
            && page_coordinates(region).all(|c| branch.valid.contains(&c))
            && let Some(image) = &branch.image {
                return Ok(Value::Image { view: image.view.clone(), slot: None, opacity: 1., plan: self.cache.plan, preview: None, encode: false });
        }
        let output = if let Some(branch) = self.cache.graph.branches.get_mut(node) {
            Some(Target { view: branch.image.get_or_insert_with(|| Image::new(self.r, self.cache.plan, "composition branch")).view.clone(), slot: None, plan: self.cache.plan })
        } else { output };
        let result = match node.as_ref() {
            Expression::Color(c) => Value::Color(c.map(f32::from_bits)),
            Expression::Source { id, placement, extent, outside } => self.source(*id, layer_core::Affine(placement.map(f32::from_bits)), *extent, f32::from_bits(*outside))?,
            Expression::Opacity { input, opacity } => self.evaluate(input)?.with_opacity(f32::from_bits(*opacity)),
            Expression::Combine { front, back, blend, flags } => {
                let (front, back) = if front.cost().1 >= back.cost().1 {
                    let front = self.evaluate(front)?;
                    (front, self.evaluate(back)?)
                } else {
                    let back = self.evaluate(back)?;
                    (self.evaluate(front)?, back)
                };
                self.draw(front, back, layer_core::LayerBlend::ALL[*blend as usize], *flags, output)?
            }
            Expression::Effect { input, chain, masks, .. } => self.effect(input, chain, masks, output)?,
        };
        if let Some(branch) = self.cache.graph.branches.get_mut(node) {
            branch.valid.extend(page_coordinates(region).filter(|c| page_rect(*c).intersect(self.cache.plan.bounds).intersect(region) == page_rect(*c).intersect(self.cache.plan.bounds)));
        }
        Ok(result)
    }
}
