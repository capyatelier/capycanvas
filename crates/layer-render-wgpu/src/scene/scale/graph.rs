use super::*;
use std::collections::{HashMap, HashSet};

pub(super) type Node = Arc<Expression>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Expression {
    Color([u32; 4]),
    Source { id: SourceTarget, placement: pixel_transform::GeometryKey, extent: [u32; 2], outside: u32 },
    Opacity { input: Node, opacity: u32 },
    Combine { front: Node, back: Node, blend: u32, flags: u32 },
    Effect { input: Node, chain: Vec<(OccurrenceHandle, u64, u32)>, masks: Vec<Option<Node>>, radius: Option<u32> },
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
                let work = u32::from(!placement.0.is_identity());
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
                (work + 1 + mask_work, scratch.max(mask_scratch + count + 3))
            }
        }
    }
    pub(super) fn damage(&self, sources: &Sources, plan: display_mips::Plan) -> Damage {
        match self {
            Self::Color(_) => Damage::EMPTY,
            Self::Source { id, placement, .. } => sources.entries.get(id).map_or_else(|| plan.bounds.into(), |source| {
                if source.damage.is_empty() { return Damage::EMPTY; }
                let placement = placement.0.clone();
                if placement.is_identity() { return source.damage.clone(); }
                let local = source.damage.expand(1 << source_level(plan.level, &placement, source.extent), source.extent);
                local.map(|local| pixel_rect(placement.forward_bounds(local.to_rect()), plan.extent).expand(
                    source.watercolor.map_or(0, |w| w.radius()), plan.extent))
            }),
            Self::Opacity { input, .. } => input.damage(sources, plan),
            Self::Combine { front, back, .. } => front.damage(sources, plan).union(back.damage(sources, plan)),
            Self::Effect { input, masks, radius, .. } => masks.iter().flatten().fold(
                input.damage(sources, plan).dependency(*radius, display_mips::Plan::at(plan.extent, plan.level)),
                |r, n| r.union(n.damage(sources, plan))),
        }
    }
    pub(super) fn required(&self, region: Damage, plan: display_mips::Plan) -> Damage {
        match self {
            Self::Color(_) => Damage::EMPTY,
            Self::Source { .. } => region,
            Self::Opacity { input, .. } => input.required(region, plan),
            Self::Combine { front, back, .. } => front.required(region.clone(), plan).union(back.required(region, plan)),
            Self::Effect { input, masks, radius, .. } => masks.iter().flatten().fold(
                input.required(region.dependency(*radius, plan), plan), |r, n| r.union(n.required(region.clone(), plan))),
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
    pub(super) fn deferred(&self, r: &WgpuRasterizer, scene: SceneView<'_>, batches: &[DabBatch]) -> bool {
        match self {
            Self::Source { id, placement, .. } => {
                if let Some(transforms) = r.transforms.as_ref().filter(|t| t.display_source(*id)) {
                    return transforms.direct_source(*id);
                }
                placement.0.as_affine().is_some() && r.watercolor_style(*id, batches).is_none()
                    && (r.moving_layer == scene.source_owner(*id) || !placement.0.is_identity())
            }
            Self::Opacity { input, .. } => input.deferred(r, scene, batches),
            Self::Combine { front, back, blend: 0, flags: 0 } => matches!(back.as_ref(), Self::Color(_)) && front.deferred(r, scene, batches),
            _ => false,
        }
    }
    pub(super) fn fused_transform(&self, r: &WgpuRasterizer) -> bool {
        match self {
            Self::Source { id, .. } => r.transforms.as_ref().is_some_and(|t| t.display_source(*id)),
            Self::Opacity { input, .. } => input.fused_transform(r),
            Self::Combine { front, back, blend: 0, flags: 0 } =>
                matches!(back.as_ref(), Self::Color(_)) && front.fused_transform(r),
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
    effects: HashMap<OccurrenceHandle, (metadata::Metadata, u64)>,
    revision: u64,
    blend_space: layer_core::BlendSpace,
}
impl Graph {
    pub fn without_pixels(&self) -> Self {
        Self { root: self.root.clone(), branches: HashMap::new(), effects: self.effects.clone(),
            revision: self.revision, blend_space: self.blend_space }
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, plan: display_mips::Plan, budget: u64) -> Result<(), GpuRasterError> {
        if std::mem::replace(&mut self.blend_space, packet.blend_space) != packet.blend_space { self.branches.clear(); }
        self.effects.retain(|id, _| packet.scene.effect(*id).is_some());
        for &handle in packet.scene.order().iter().filter(|handle| packet.scene.effect(**handle).is_some()) {
            let metadata = metadata::Metadata::new(packet.scene, handle);
            if self.effects.get(&handle).is_none_or(|(old, _)| *old != metadata) {
                self.revision += 1;
                self.effects.insert(handle, (metadata, self.revision));
            }
        }
        let root = compose(packet, Some(sources), Some(&self.effects), plan.level, r.device.working_space())?;
        let mut all = HashSet::new();
        Expression::visit(&root, &mut all);
        let mut eligible: Vec<_> = all.into_iter().filter(|n| !Arc::ptr_eq(n, &root)
            && matches!(n.as_ref(), Expression::Combine { .. } | Expression::Effect { .. })).collect();
        eligible.sort_by_cached_key(|n| (n.damage(sources, plan).area(), std::cmp::Reverse(n.cost().0), n.clone()));
        eligible.truncate((budget / plan.level_bytes(plan.level)) as usize);
        let wanted: HashSet<_> = eligible.into_iter().collect();
        let mut reusable: Vec<_> = self.branches.extract_if(|node, _| !wanted.contains(node))
            .filter_map(|(_, branch)| branch.image.filter(|image| image.plan == plan)).collect();
        for node in wanted {
            let dirty = node.damage(sources, plan);
            let branch = self.branches.entry(node).or_insert_with(|| Branch { image: reusable.pop(), valid: BTreeSet::new() });
            if branch.image.as_ref().is_some_and(|image| image.plan != plan) { branch.image = None; branch.valid.clear(); }
            branch.valid.retain(|c| !dirty.intersects(page_rect(*c)));
        }
        self.root = Some(root);
        Ok(())
    }
    pub fn reserved_bytes(&self, plan: display_mips::Plan) -> u64 { self.branches.len() as u64 * plan.level_bytes(plan.level) }
    pub fn storage_bytes(&self) -> u64 {
        self.branches.values().filter_map(|b| b.image.as_ref()).map(|i| texture_bytes(&i.texture)).sum()
    }
}

pub(super) fn scratch_images(packet: FramePacket<'_>, level: u32, space: layer_core::color::RgbSpace) -> Result<u64, GpuRasterError> {
    Ok(u64::from(compose(packet, None, None, level, space)?.cost().1) + 1)
}

fn compose(
    packet: FramePacket<'_>, sources: Option<&Sources>, effects: Option<&HashMap<OccurrenceHandle, (metadata::Metadata, u64)>>,
    level: u32, space: layer_core::color::RgbSpace,
) -> Result<Node, GpuRasterError> {
    let mut builder = Builder { packet, sources, effects, level, space };
    let output = stack::compose(&mut builder, packet.scene, None, packet.scene.effect_input())?;
    let mut root = Expression::over(&output);
    if let Some(handle) = packet.inspect_mask
        && packet.scene.mask(handle).is_some_and(|(mask, _)| mask.enabled) {
        root = Expression::combine(builder.source(handle, true), root, layer_core::LayerBlend::Normal, 64);
    }
    Ok(root)
}

struct Builder<'a> {
    packet: FramePacket<'a>,
    sources: Option<&'a Sources>,
    effects: Option<&'a HashMap<OccurrenceHandle, (metadata::Metadata, u64)>>,
    level: u32,
    space: layer_core::color::RgbSpace,
}
impl Builder<'_> {
    fn source(&self, handle: OccurrenceHandle, mask: bool) -> Node {
        let scene = self.packet.scene;
        let id = if mask { SourceTarget::Coverage(scene.mask(handle).unwrap().0.source) } else { scene.source_target(handle).unwrap() };
        if self.sources.is_some_and(|sources| !sources.entries.contains_key(&id)) { return Expression::color([0.; 4]); }
        let outside = scene.mask(handle).filter(|_| mask).map_or(0., |(use_, source)| if use_.inverted { 1. - source.default_coverage } else { source.default_coverage });
        Arc::new(Expression::Source { id, placement: pixel_transform::GeometryKey(scene.target_geometry(id)),
            extent: scene.target_extent(id), outside: outside.to_bits() })
    }

}
impl stack::Compositor for Builder<'_> {
    type Image = Vec<Node>;
    fn clear(&mut self) -> Self::Image { Vec::new() }
    fn discard(&mut self, _: Self::Image) {}
    fn duplicate(&mut self, image: &Self::Image) -> Self::Image { image.clone() }
    fn fade(&mut self, front: Self::Image, back: Self::Image, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        let back = Expression::over(&back);
        let front = Expression::over(&front);
        let (front, back) = if layer.mask.as_ref().is_some_and(|m| m.enabled) {
            let change = Expression::combine(front, Expression::opacity(back.clone(), -1.), layer_core::LayerBlend::Normal, 128);
            let masked = Expression::combine(change, self.source(index, true), layer_core::LayerBlend::Normal, 32);
            (Expression::opacity(masked, layer.opacity), back)
        } else { (Expression::opacity(front, layer.opacity), Expression::opacity(back, 1. - layer.opacity)) };
        Ok(vec![Expression::combine(front, back, layer_core::LayerBlend::Normal, 128)])
    }
    fn layer(&mut self, index: OccurrenceHandle) -> Result<Self::Image, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        let output = if layer.kind() == LayerKind::Group {
            stack::compose(self, self.packet.scene, Some(index), None)?
        } else if let Some(color) = self.packet.scene.effect(index).and_then(|effect| effect.constant_color()) {
            let [r, g, b, a] = color.linear_in(self.space).map_err(GpuRasterError::Color)?;
            if a == 0. { Vec::new() }
            else { vec![Expression::color(self.packet.blend_space.composite(self.space, [r * a, g * a, b * a, a]))] }
        } else if layer.kind() == LayerKind::Effect {
            self.effect(&[index], Vec::new())?
        } else { vec![self.source(index, false)] };
        if layer.mask.as_ref().is_some_and(|m| m.enabled) {
            Ok(vec![Expression::combine(Expression::over(&output), self.source(index, true), layer_core::LayerBlend::Normal, 32)])
        } else { Ok(output) }
    }
    fn blend(&mut self, front: Self::Image, mut back: Self::Image, index: OccurrenceHandle, clipped: bool) -> Result<Self::Image, GpuRasterError> {
        let layer = self.packet.scene.occurrence(index).unwrap();
        let front = if layer.opacity == 1. { front } else { vec![Expression::opacity(Expression::over(&front), layer.opacity)] };
        if layer.blend == layer_core::LayerBlend::Normal && !clipped {
            back.extend(front);
            Ok(back)
        } else {
            Ok(vec![Expression::combine(Expression::over(&front), Expression::over(&back), layer.blend, if clipped { 16 } else { 0 })])
        }
    }
    fn effect(&mut self, indices: &[OccurrenceHandle], input: Self::Image) -> Result<Self::Image, GpuRasterError> {
        let radius = indices.iter().try_fold(0u32, |radius, i| radius.checked_add(
            crate::effects::damage_radius(self.packet.scene.effect(*i).unwrap(), self.level)?));
        let chain = indices.iter().map(|i| {
            let effect = self.packet.scene.effect(*i).unwrap();
            (*i, self.effects.map_or(0, |effects| effects[i].1), if effect.animated() { self.packet.time_seconds.to_bits() } else { 0 })
        }).collect();
        let masks = indices.iter().map(|i| {
            let layer = self.packet.scene.occurrence(*i).unwrap();
            (self.packet.scene.effect(*i).unwrap().program.kind == layer_core::EffectKind::Adjustment
                && layer.mask.as_ref().is_some_and(|m| m.enabled)).then(|| self.source(*i, true))
        }).collect();
        Ok(vec![Arc::new(Expression::Effect { input: Expression::over(&input), chain, masks, radius })])
    }
    fn has_content(&self, index: OccurrenceHandle) -> bool {
        let layer = self.packet.scene.occurrence(index).unwrap();
        matches!(layer.kind(), LayerKind::Group | LayerKind::Paint | LayerKind::Effect)
    }
}

impl Evaluator<'_> {
    pub(super) fn evaluate_root(&mut self, node: &Node, direct: bool) -> Result<Value, GpuRasterError> {
        let deferred = if direct && self.cache.plan.level > 0 && self.cache.plan.bounds == PixelRect::full(self.cache.plan.extent)
            && node.deferred(self.r, self.packet.scene, self.packet.dab_batches) { Some(self.evaluate(node)?) } else { None };
        if matches!(deferred, Some(Value::Placed(_) | Value::Transform(_))) { return Ok(deferred.unwrap()); }
        self.cache.pixels.ensure_root(self.r, self.cache.plan, "composition level");
        let image = self.cache.pixels.root().unwrap();
        let output = Target { view: image.view.clone(), slot: Some(Slot::Root), plan: image.plan };
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
                return Ok(Value::Image { view: image.view.clone(), slot: None, opacity: 1., plan: image.plan, preview: None, encode: false });
        }
        let output = if let Some(branch) = self.cache.graph.branches.get_mut(node) {
            let image = branch.image.get_or_insert_with(|| Image::new(self.r, self.input, "composition branch"));
            Some(Target { view: image.view.clone(), slot: None, plan: image.plan })
        } else { output };
        let result = match node.as_ref() {
            Expression::Color(c) => Value::Color(c.map(f32::from_bits)),
            Expression::Source { id, placement, extent, outside } => self.source(*id, placement.0.clone(), *extent, f32::from_bits(*outside))?,
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
            let bounds = branch.image.as_ref().unwrap().plan.bounds;
            branch.valid.extend(page_coordinates(region).filter(|c| page_rect(*c).intersect(bounds).intersect(region) == page_rect(*c).intersect(bounds)));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::*;
    use super::super::tests::{document_at, add_fill, coverage_mask, set_effect_value, display_pixels};
    use layer_core::{EffectValue, Point, Selection};
    use layer_core::color::RgbColor;

    #[test]
    fn fill_edits_reuse_branch_textures_without_reusing_old_pixels() {
        let mut doc = document_at([517, 259]);
        let paint = doc.scene().order()[0];
        doc.artwork.occurrences.get_mut(paint).unwrap().opacity = 0.37;
        let color = |rgb, alpha| RgbColor::from_linear(doc.composition().color.space, [rgb, 0.27, 0.61, alpha]).unwrap();
        let initial = color(0.13, 0.43);
        let fill = add_fill(&mut doc, initial);
        coverage_mask(&mut doc, fill, Point::default(), Some(Selection::polygon(vec![
            Point { x: 0., y: 0. }, Point { x: 301., y: 0. },
            Point { x: 301., y: 259. }, Point { x: 0., y: 259. },
        ]).unwrap()));
        let mut renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let extent = doc.composition().size;
        let mut previous_pixels: Option<Vec<[f32; 4]>> = None;
        let mut previous_textures = Vec::new();
        for (step, (red, alpha)) in [(0.13, 0.43), (0.83, 0.71), (0.31, 0.19)].into_iter().enumerate() {
            let value = RgbColor::from_linear(doc.composition().color.space, [red, 0.27, 0.61, alpha]).unwrap();
            set_effect_value(&mut doc, fill, "color", EffectValue::Color(value));
            let mut frame = crate::test_support::packet(doc.scene(), extent);
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            renderer.submit(frame).unwrap();
            let cache = renderer.scale_display.as_ref().unwrap();
            let textures: Vec<_> = cache.graph.branches.values().filter_map(|branch|
                branch.image.as_ref().map(|image| image.texture.clone())).collect();
            assert!(!textures.is_empty(), "fixture must materialize cached effect branches");
            if step > 0 {
                assert_eq!(textures.len(), previous_textures.len());
                assert!(textures.iter().all(|texture| previous_textures.contains(texture)),
                    "same-plan Fill edits must reuse all retired branch textures");
            }
            let actual = display_pixels(&renderer);
            let mut fresh = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            fresh.submit(frame).unwrap();
            let expected = display_pixels(&fresh);
            assert!(crate::test_support::max_error(&actual, &expected) < 2e-5,
                "recycled validity must not expose old masked/translucent Fill pixels");
            if let Some(previous) = previous_pixels {
                assert!(crate::test_support::max_error(&actual, &previous) > 0.01, "visible Fill edits must change pixels");
            }
            previous_pixels = Some(actual);
            previous_textures = textures;
        }
    }
}
