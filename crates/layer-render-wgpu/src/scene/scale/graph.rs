use super::*;
use std::collections::{HashMap, HashSet};

pub(super) type Node = Arc<Expression>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Expression {
    Color([u32; 4]),
    Objects {content:object_spatial::Content,preview:bool},
    Source { id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: u32 },
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
    pub(super) fn support(&self, scene: SceneView<'_>, extent: [u32; 2], level:u32) -> DocRect {
        match self {
            Self::Color(color) => if color[3] == 0 { DocRect::default() } else { PixelRect::full(extent).into() },
            Self::Source { frame, extent, .. } => DocRect::from(PixelRect::full(*extent)).translated(frame.origin()),
            Self::Objects {content,..} => content.bounds_at(level),
            Self::Opacity { input, .. } => input.support(scene, extent,level),
            Self::Combine { front, back, flags, .. } => {
                if flags & 32 != 0 { front.support(scene, extent,level) }
                else if flags & 16 != 0 { back.support(scene, extent,level) }
                else { front.support(scene, extent,level).union(back.support(scene, extent,level)) }
            },
            Self::Effect { input, chain, .. } => chain.iter().fold(input.support(scene, extent,level), |support, (handle, _, _)| {
                let effect = scene.effect(*handle).unwrap();
                if effect.program.kind == layer_core::EffectKind::Generator { PixelRect::full(extent).into() }
                else { crate::effects::pass_input_support(effect, effect.program.passes.len(), support, level).unwrap_or(support) }
            }),
        }
    }
    fn cost(&self) -> (u32, u32) {
        match self {
            Self::Color(_) => (0, 0),
            Self::Objects {content,..} => (content.len() as u32*2, 3),
            Self::Source { frame, .. } => {
                let work = u32::from(!frame.aligned());
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
        Damage::from_regions(self.document_damage(sources, plan).into_iter().map(|region| region.in_frame(plan.extent)))
    }
    fn document_damage(&self, sources: &Sources, plan: display_mips::Plan) -> Vec<DocRect> {
        match self {
            Self::Color(_) => Vec::new(),
            Self::Objects {content,..} => [None, Some(content.owner())].into_iter().flat_map(|owner| sources.object_damage.get(&owner).into_iter()
                .chain(sources.object_refinements.get(&(plan.level, owner)))).flat_map(|damage| damage.regions.iter()).copied().map(DocRect::from).collect(),
            Self::Source { id, frame, .. } => sources.entries.get(id).map_or_else(|| vec![plan.doc_bounds], |source| {
                let local = if frame.aligned() { source.damage.clone() } else {
                    source.damage.expand(1 << source_level(plan.level, *frame), frame.extent(source.extent))
                };
                local.regions.iter().map(|region| frame.document(*region)
                    .expand(source.watercolor.map_or(0, |w| w.radius()))).collect()
            }),
            Self::Opacity { input, .. } => input.document_damage(sources, plan),
            Self::Combine { front, back, .. } => front.document_damage(sources, plan).into_iter().chain(back.document_damage(sources, plan)).collect(),
            Self::Effect { input, masks, radius, .. } => {
                let input = input.document_damage(sources, plan);
                let mut damage = if input.is_empty() { Vec::new() } else { radius.map_or_else(|| vec![plan.doc_bounds], |radius| input.into_iter().map(|region| region.expand(radius)).collect()) };
                for mask in masks.iter().flatten() { damage.extend(mask.document_damage(sources, plan)); }
                damage
            }
        }
    }
    pub(super) fn required(&self, region: Damage, plan: display_mips::Plan) -> Damage {
        match self {
            Self::Color(_) => Damage::EMPTY,
            Self::Objects {..} => region,
            Self::Source { .. } => region,
            Self::Opacity { input, .. } => input.required(region, plan),
            Self::Combine { front, back, .. } => front.required(region.clone(), plan).union(back.required(region, plan)),
            Self::Effect { input, masks, radius, .. } => masks.iter().flatten().fold(
                input.required(region.dependency(*radius, plan), plan), |r, n| r.union(n.required(region.clone(), plan))),
        }
    }
    pub(super) fn source_domains(&self, scene: SceneView<'_>) -> [BTreeSet<SourceTarget>; 2] {
        fn visit(node: &Expression, scene: SceneView<'_>, native: bool, domains: &mut [BTreeSet<SourceTarget>; 2]) {
            match node {
                Expression::Source { id, .. } => { domains[usize::from(native)].insert(*id); }
                Expression::Opacity { input, .. } => visit(input, scene, native, domains),
                Expression::Combine { front, back, .. } => { visit(front, scene, native, domains); visit(back, scene, native, domains); }
                Expression::Effect { input, chain, masks, .. } => {
                    let native = native || chain.iter().any(|(h, ..)| native_pointwise_alpha(&scene.effect(*h).unwrap().program));
                    visit(input, scene, native, domains);
                    for mask in masks.iter().flatten() { visit(mask, scene, native, domains); }
                }
                Expression::Color(_) | Expression::Objects {..} => {}
            }
        }
        let mut domains = Default::default();
        visit(self, scene, false, &mut domains);
        domains
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
            Self::Source { id, frame, .. } => {
                if let Some(transforms) = r.transforms.as_ref().filter(|t| t.display_source(*id)) {
                    return transforms.direct_source(*id);
                }
                r.watercolor_style(*id, batches).is_none() && (r.moving_layer == scene.source_owner(*id) || !frame.aligned())
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
    initialized:BTreeSet<[u32;2]>,
}
#[derive(Default)]
pub(super) struct Graph {
    pub root: Option<Node>,
    branches: HashMap<Node, Branch>,
    effects: HashMap<OccurrenceHandle, (metadata::Metadata, u64)>,
    revision: u64,
    blend_space: layer_core::BlendSpace,
    objects:object_spatial::SpatialIndex,
}
impl Graph {
    pub fn objects(&self)->&object_spatial::SpatialIndex {&self.objects}
    pub fn without_pixels(&self) -> Self {
        Self { root: self.root.clone(), branches: HashMap::new(), effects: self.effects.clone(),
            revision: self.revision, blend_space: self.blend_space, objects:self.objects.clone() }
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, plan: display_mips::Plan, budget: u64) -> Result<(), GpuRasterError> {
        self.objects.prepare(packet.scene);
        if std::mem::replace(&mut self.blend_space, packet.blend_space) != packet.blend_space { self.branches.clear(); }
        self.effects.retain(|id, _| packet.scene.effect(*id).is_some());
        for &handle in packet.scene.order().iter().filter(|handle| packet.scene.effect(**handle).is_some()) {
            let metadata = metadata::Metadata::new(packet.scene, handle);
            if self.effects.get(&handle).is_none_or(|(old, _)| *old != metadata) {
                self.revision += 1;
                self.effects.insert(handle, (metadata, self.revision));
            }
        }
        let root = compose(packet, Some(sources), Some(&self.effects), plan.level, r.device.working_space(), Some(&mut self.objects))?;
        let mut all = HashSet::new();
        Expression::visit(&root, &mut all);
        let mut eligible: Vec<_> = all.into_iter().filter(|n| !Arc::ptr_eq(n, &root)
            && matches!(n.as_ref(), Expression::Objects {..} | Expression::Combine { .. } | Expression::Effect { .. })).collect();
        eligible.sort_by_cached_key(|n| (n.damage(sources, plan).area(), std::cmp::Reverse(n.cost().0), n.clone()));
        eligible.truncate((budget / plan.level_bytes(plan.level)) as usize);
        let wanted: HashSet<_> = eligible.into_iter().collect();
        let mut reusable: Vec<_> = self.branches.extract_if(|node, _| !wanted.contains(node))
            .filter_map(|(_, branch)| branch.image.filter(|image| image.plan == plan)).collect();
        for node in wanted {
            let dirty = node.damage(sources, plan);
            let branch = self.branches.entry(node).or_insert_with(|| Branch { image: reusable.pop(), valid: BTreeSet::new(),initialized:BTreeSet::new() });
            if branch.image.as_ref().is_some_and(|image| image.plan != plan) { branch.image = None; branch.valid.clear();branch.initialized.clear(); }
            branch.valid.retain(|c| !dirty.intersects(page_rect(*c)));
        }
        self.root = Some(root);
        Ok(())
    }
    pub fn reserved_bytes(&self, plan: display_mips::Plan) -> u64 { self.branches.len() as u64 * plan.level_bytes(plan.level) }
    pub fn storage_bytes(&self) -> u64 {
        self.branches.values().filter_map(|b| b.image.as_ref()).map(|i| texture_bytes(&i.texture)).sum::<u64>() + self.objects.storage_bytes()
    }
}

pub(super) fn scratch_images(packet: FramePacket<'_>, level: u32, space: layer_core::color::RgbSpace, objects:Option<&object_spatial::SpatialIndex>) -> Result<u64, GpuRasterError> {
    let mut objects=objects.cloned().unwrap_or_default();
    Ok(u64::from(compose(packet, None, None, level, space, Some(&mut objects))?.cost().1) + 1)
}

fn compose(
    packet: FramePacket<'_>, sources: Option<&Sources>, effects: Option<&HashMap<OccurrenceHandle, (metadata::Metadata, u64)>>,
    level: u32, space: layer_core::color::RgbSpace, objects:Option<&mut object_spatial::SpatialIndex>,
) -> Result<Node, GpuRasterError> {
    let mut builder = Builder { packet, sources, effects, level, space, objects };
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
    objects:Option<&'a mut object_spatial::SpatialIndex>,
}
impl Builder<'_> {
    fn source(&self, handle: OccurrenceHandle, mask: bool) -> Node {
        let scene = self.packet.scene;
        let id = if mask { SourceTarget::Coverage(scene.mask(handle).unwrap().0.source) } else { scene.source_target(handle).unwrap() };
        if self.sources.is_some_and(|sources| !sources.entries.contains_key(&id)) { return Expression::color([0.; 4]); }
        let outside = scene.mask(handle).filter(|_| mask).map_or(0., |(use_, source)| if use_.inverted { 1. - source.default_coverage } else { source.default_coverage });
        Arc::new(Expression::Source { id, frame: source_frame(self.sources, scene, id), extent: scene.target_extent(id), outside: outside.to_bits() })
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
        } else if let Some(color) = self.packet.scene.effect(index).filter(|_| bounded(self.packet.scene)).and_then(|effect| effect.constant_color()) {
            let [r, g, b, a] = color.linear_in(self.space).map_err(GpuRasterError::Color)?;
            if a == 0. { Vec::new() }
            else { vec![Expression::color(self.packet.blend_space.composite(self.space, [r * a, g * a, b * a, a]))] }
        } else if layer.kind() == LayerKind::Effect {
            self.effect(&[index], Vec::new())?
        } else if self.packet.scene.object_layer(index).is_some() {
            let content=if let Some(cache)=&mut self.objects {cache.content(self.packet.scene,index)}
                else {object_spatial::SpatialIndex::default().content(self.packet.scene,index)}.unwrap();
            vec![Arc::new(Expression::Objects {content,preview:self.sources.is_some_and(|sources|sources.object_moving==Some(index))})]
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
            (*i, self.effects.map_or(0, |effects| effects[i].1), crate::effects::effective_phase(self.packet.scene,*i,self.packet.time_seconds).to_bits())
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
        matches!(layer.kind(), LayerKind::Group | LayerKind::Paint | LayerKind::Effect) || self.packet.scene.object_layer(index).is_some()
    }
}

impl Cache {
    pub(super) fn initialized_pages(&self,target:&Target)->Option<&BTreeSet<[u32;2]>> {
        match target.slot {
            Some(Slot::Root)=>Some(&self.valid),
            None=>self.graph.branches.values().find_map(|b|b.image.as_ref()
                .filter(|image|image.view==target.view && image.plan==target.plan).map(|_|&b.initialized)),
            _=>None,
        }
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
        let region = self.region.in_frame(self.cache.plan.extent);
        if let Some(branch) = self.cache.graph.branches.get(node)
            && self.region.min.iter().all(|n| *n >= 0) && self.region.max.iter().zip(self.cache.plan.extent).all(|(n, e)| *n <= i64::from(e))
            && page_coordinates(region).all(|c| branch.valid.contains(&c))
            && let Some(image) = &branch.image {
                return Ok(Value::Image { view: image.view.clone(), slot: None, opacity: 1., plan: image.plan, preview: None, encode: false });
        }
        let output = if let Some(branch) = self.cache.graph.branches.get_mut(node) {
            let image = branch.image.get_or_insert_with(|| Image::new(self.r, self.input, "composition branch"));
            Some(Target { view: image.view.clone(), slot: None, plan: image.plan })
        } else { output };
        let evaluation=self.cache.graph.branches.get(node).filter(|branch| self.region == DocRect::from(region)
            && branch.image.as_ref().unwrap().plan.doc_bounds == DocRect::from(branch.image.as_ref().unwrap().plan.bounds)
            && matches!(node.as_ref(),Expression::Effect {chain,..}
            if self.cache.plan.level>0 && chain.iter().any(|(h,..)|native_pointwise_alpha(&self.packet.scene.effect(*h).unwrap().program))))
            .map_or(self.region,|branch|page_coordinates(region).filter(|c|!branch.initialized.contains(c))
                .fold(region,|bounds,c|bounds.union(page_rect(c).intersect(branch.image.as_ref().unwrap().plan.bounds))).into());
        let result = self.with_region(evaluation,|compositor|Ok(match node.as_ref() {
            Expression::Color(c) => Value::Color(c.map(f32::from_bits)),
            Expression::Objects {content,preview} => compositor.objects(content,*preview,output)?,
            Expression::Source { id, frame, extent, outside } => compositor.source(*id, *frame, *extent, f32::from_bits(*outside))?,
            Expression::Opacity { input, opacity } => compositor.evaluate(input)?.with_opacity(f32::from_bits(*opacity)),
            Expression::Combine { front, back, blend, flags } => {
                let (front, back) = if front.cost().1 >= back.cost().1 {
                    let front = compositor.evaluate(front)?;
                    (front, compositor.evaluate(back)?)
                } else {
                    let back = compositor.evaluate(back)?;
                    (compositor.evaluate(front)?, back)
                };
                compositor.draw(front, back, layer_core::LayerBlend::ALL[*blend as usize], *flags, output)?
            }
            Expression::Effect { input, chain, masks, .. } => compositor.effect(input, chain, masks, output)?,
        }))?;
        if let Some(branch) = self.cache.graph.branches.get_mut(node) {
            let bounds = branch.image.as_ref().unwrap().plan.doc_bounds.in_frame(self.cache.plan.extent);
            let complete:Vec<_>=page_coordinates(evaluation.in_frame(self.cache.plan.extent)).filter(|c|page_rect(*c).intersect(bounds).intersect(evaluation.in_frame(self.cache.plan.extent))==page_rect(*c).intersect(bounds)).collect();
            branch.valid.extend(complete.iter().copied());branch.initialized.extend(complete);
        }
        Ok(result)
    }
}

impl Cache {
    pub(super) fn prepare_native_branches(
        &mut self, scene: &mut Scene, r: &mut WgpuRasterizer, packet: FramePacket<'_>, regions: &[PixelRect],
        input: display_mips::Plan, encoding: &mut Encoding<'_>,
    ) -> Result<(), GpuRasterError> {
        if self.plan.level != 0 || input.doc_bounds != DocRect::from(input.bounds) { return Ok(()); }
        let mut pending = vec![self.graph.root.clone().unwrap()]; let mut wanted = BTreeSet::new();
        while let Some(node) = pending.pop() {
            match node.as_ref() {
                graph::Expression::Opacity { input, .. } => pending.push(input.clone()),
                graph::Expression::Combine { front, back, .. } => pending.extend([front.clone(), back.clone()]),
                graph::Expression::Effect { chain, .. } if self.graph.branches.contains_key(&node)
                    && super::effects::native_alpha_capture(packet.scene, &chain.iter().map(|(h, ..)| *h).collect::<Vec<_>>(), 0) => { wanted.insert(node); }
                _ => {}
            }
        }
        if wanted.is_empty() { return Ok(()); }
        let Encoding { encoder, commands } = encoding;
        commands.flush(r, encoder)?; if !scene.jobs.is_empty() { scene.encode_jobs(r, encoder)?; }
        for node in wanted {
            let graph::Expression::Effect { chain, .. } = node.as_ref() else { unreachable!() };
            let branch = self.graph.branches.get_mut(&node).unwrap();
            let image = branch.image.get_or_insert_with(|| Image::new(r, input, "composition branch"));
            let target = Target { view: image.view.clone(), slot: None, plan: image.plan };
            let mut pages: Vec<_> = regions.iter().flat_map(|region| page_coordinates(region.intersect(target.plan.bounds)))
                .filter(|page| !branch.valid.contains(page)).collect::<BTreeSet<_>>().into_iter().collect();
            pages.sort_by_key(|[x, y]| (*y, *x));
            let mut first = 0;
            while first < pages.len() {
                let mut end = first + 1;
                while end < pages.len() && end - first < SOURCE_SLOTS
                    && pages[end][1] == pages[first][1] && pages[end][0] == pages[end - 1][0] + 1 { end += 1; }
                let regions: Vec<DocRect> = pages[first..end].iter().map(|page| page_rect(*page).intersect(target.plan.bounds).into()).collect();
                super::effects::capture_native_effect(scene, r, packet, chain.last().unwrap().0, &target, &regions, None, encoder)?;
                branch.valid.extend(pages[first..end].iter().copied()); branch.initialized.extend(pages[first..end].iter().copied());
                first = end;
            }
        }
        Ok(())
    }
}


#[cfg(test)]
mod reuse_tests {
    use super::*;
    use super::super::tests::{paint_mut, paint_occurrence, insert_occurrence, effect_occurrence};
    use layer_core::{color::source::rgba8_source, authored::Attachment};
    use crate::test_support::{packet, float_pixels as pixels};
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
        coverage_mask(&mut doc, fill, [0, 0], Some(Selection::polygon(vec![
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
#[test]
fn native_level_retained_branches_batch_live_graph_pages_and_keep_islands_separate() {
    for count in [6, 16, 19] { for name in ["threshold", "brightness_to_opacity"] {
        let extent = [count * PAGE_SIZE - 13, 117];
        let mut doc = document_at(extent);
        paint_mut(&mut doc, 0).base = (Some(rgba8_source(extent, |x, y| [(x * 17) as u8, (y * 23) as u8, 129, 173]))).map(|source|layer_core::authored::PaintBase::new(source.into()));
        let filter = effect_occurrence(&mut doc, layer_core::EffectInstance::new(crate::tests::fixture(name).program()), name); doc.artwork.occurrences.get_mut(filter).unwrap().attachment = Attachment::Effect;
        insert_occurrence(&mut doc, filter, 0);
        let photo = paint_occurrence(&mut doc, "photo", Some(rgba8_source(extent, |x, y| [(x * 29) as u8, (y * 31) as u8, 173, 255])));
        insert_occurrence(&mut doc, photo, 2);
        let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let frame = packet(doc.scene(), extent); r.submit(frame).unwrap(); r.wait_idle().unwrap();
        let reference = display_pixels(&r);
        let mut scene = r.scene.take().unwrap(); let mut cache = r.scale_display.take().unwrap();
        r.document_damage.clear(); r.transform_damage.clear();
        assert_eq!(cache.plan.level, 0); assert_eq!(cache.plan.bounds, PixelRect::full(extent));
        assert!(cache.graph.branches.keys().any(|node| matches!(node.as_ref(), graph::Expression::Effect { .. })));
        for sparse in [false, true] {
            for source in scene.scale_sources.entries.values_mut() { source.damage = Damage::EMPTY; }
            cache.valid.clear();
            for branch in cache.graph.branches.values_mut() { branch.valid.clear(); }
            let regions = if sparse { vec![PixelRect::new(11, 17, 29, 37), PixelRect::new(extent[0] - 19, 71, extent[0] - 3, 91)] }
                else { vec![PixelRect::full(extent)] };
            if sparse {
                cache.valid.extend((1..count - 1).map(|x| [x, 0]));
                for branch in cache.graph.branches.values_mut() { branch.valid.extend((1..count - 1).map(|x| [x, 0])); }
                let target = doc.scene().source_target(doc.scene().order()[1]).unwrap();
                scene.scale_sources.entries.get_mut(&target).unwrap().damage = Damage::from_regions(regions);
            }
            let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default()); let mut commands = Commands::new(&r);
            let work = r.metrics.composited_pixels;
            cache.render_graph(&mut scene, &mut r, frame, PixelRect::EMPTY,
                &mut Encoding { encoder: &mut encoder, commands: &mut commands }, Destination::View, Some(&BTreeSet::new())).unwrap();
            commands.flush(&mut r, &mut encoder).unwrap();
            let passes = encoder.pass_count(); let pages = if sparse { 2 } else { count };
            let batches = if sparse { 2 } else { count.div_ceil(SOURCE_SLOTS as u32) };
            assert!(passes <= u64::from(batches + pages.div_ceil(32) + 1),
                "{name} count={count} sparse={sparse}: physical passes={passes} must batch branch pages, not materialize each page");
            assert!(r.metrics.composited_pixels - work <= u64::from(pages * PAGE_SIZE * extent[1]));
            r.uploads.finish(&encoder); encoder.submit(&r.queue);
            let actual = pixels(&r, cache.texture());
            for (i, (actual, expected)) in actual.iter().zip(&reference).enumerate() {
                assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits), "{name} count={count} sparse={sparse} pixel={i}");
            }
            for (node, branch) in &cache.graph.branches {
                if matches!(node.as_ref(), graph::Expression::Effect { .. }) { assert_eq!(branch.valid.len(), count as usize); }
            }
        }
        r.scene = Some(scene); r.scale_display = Some(cache);
    }}
}

}

#[cfg(test)]
mod object_reuse_tests {
    use super::*;
    use layer_core::{Edit, authored::*};
    fn collection(root:&Node)->Node {
        let mut nodes=HashSet::new();Expression::visit(root,&mut nodes);
        assert!(nodes.len()<8,"A collection remains one cached expression regardless of child count");
        nodes.into_iter().find(|node|matches!(node.as_ref(),Expression::Objects {..})).unwrap()
    }
    #[test]
    fn paint_only_edits_reuse_the_object_collection_branch_and_object_edits_replace_it() {
        let (mut doc,owner,handles)=object_spatial::tests::document();
        let mut cache=object_spatial::SpatialIndex::default();
        let build=|doc:&layer_core::Document,cache:&mut object_spatial::SpatialIndex| {
            collection(&compose(crate::test_support::packet(doc.scene(),doc.composition().size),None,None,0,doc.composition().color.space,Some(cache)).unwrap())
        };
        let before=build(&doc,&mut cache);
        let paint=doc.artwork.paint.iter().next().unwrap().0;
        let mut source=doc.artwork.paint.get(paint).unwrap().clone();source.domain=[257,256];
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(source)).unwrap())).unwrap();
        let painted=build(&doc,&mut cache);
        assert!(before==painted);
        let packet=crate::test_support::packet(doc.scene(),doc.composition().size);
        assert!(scratch_images(packet,2,doc.composition().color.space,Some(&cache)).unwrap()>=4);
        let mut planning=cache.clone();
        let planned=collection(&compose(packet,None,None,2,doc.composition().color.space,Some(&mut planning)).unwrap());
        assert!(before==planned,"scratch planning and display retain the same collection index after a paint edit");
        if let (Expression::Objects {content:a,..},Expression::Objects {content:b,..})=(before.as_ref(),painted.as_ref()) {
            assert!(Arc::ptr_eq(&a.keys(),&b.keys()));
            assert_eq!(b.query_at(DocRect {min:[-4,-4],max:[12,12]},0).len(),1);
        } else {unreachable!()}
        doc.apply(doc.set_image_object_affine_edit(handles[1],Affine64([1.,0.,0.,1.,128.,0.])).unwrap()).unwrap();
        assert!(before!=build(&doc,&mut cache));
        let mut occurrence=doc.scene().occurrence(owner).unwrap().clone();occurrence.offset[0]=256;
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,owner,Some(occurrence)).unwrap())).unwrap();
        let moved=build(&doc,&mut cache);assert!(before!=moved);
    }
}
