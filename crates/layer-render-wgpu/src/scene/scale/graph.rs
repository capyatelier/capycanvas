use super::*;
use std::collections::{HashMap, HashSet};
use super::super::vectors::Moving;

pub(super) type Node = Arc<Expression>;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Expression {
    Color([u32; 4]),
    Objects {content:object_spatial::Content,preview:bool},
    Vector { owner: OccurrenceHandle, offset: [u64; 2], moving: Option<Moving> },
    VectorPreview { owner: OccurrenceHandle },
    VectorMotion { owner: OccurrenceHandle },
    Source { id: SourceTarget, frame: SourceFrame, extent: [u32; 2], outside: u32 },
    Opacity { input: Node, opacity: u32 },
    Combine { front: Node, back: Node, blend: u32, flags: u32 },
    Effect { input: Node, chain: Vec<(OccurrenceHandle, u64, u32)>, masks: Vec<Option<Node>>, radius: Option<u32> },
}
impl Expression {
    fn color(value: [f32; 4]) -> Node { Arc::new(Self::Color(value.map(f32::to_bits))) }
    fn content(node:&Node)->Node {
        match node.as_ref() {
            Self::Opacity {input,opacity}=>{
                let normalized=Self::content(input);
                if Arc::ptr_eq(input,&normalized) {node.clone()}else {Arc::new(Self::Opacity {input:normalized,opacity:*opacity})}
            }
            Self::Combine {front,back,blend,flags}=>{
                let a=Self::content(front);let b=Self::content(back);
                if Arc::ptr_eq(front,&a) && Arc::ptr_eq(back,&b) {node.clone()}
                else {Arc::new(Self::Combine {front:a,back:b,blend:*blend,flags:*flags})}
            }
            Self::Effect {input,chain,masks,..}=>Arc::new(Self::Effect {input:Self::content(input),chain:chain.clone(),
                masks:masks.iter().map(|mask|mask.as_ref().map(Self::content)).collect(),radius:None}),
            _=>node.clone(),
        }
    }

    pub(super) fn is_clear(node: &Node) -> bool { matches!(node.as_ref(), Self::Color(c) if *c == [0; 4]) }
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
            Self::Vector { owner, .. } => super::super::vectors::Vectors::bounds(scene, *owner),
            Self::VectorPreview { .. } | Self::VectorMotion { .. } => DocRect::from(PixelRect::full(extent)),
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
    pub(super) fn cost(&self) -> (u32, u32) {
        match self {
            Self::Color(_) => (0, 0),
            Self::Objects {content,..} => (content.len() as u32*2, 3),
            Self::Vector { .. } | Self::VectorPreview { .. } | Self::VectorMotion { .. } => (2, 3),
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
    pub(super) fn required(&self, region: Damage, plan: display_mips::Plan) -> Damage {
        match self {
            Self::Color(_) => Damage::EMPTY,
            Self::Objects {..} | Self::Vector { .. } | Self::VectorPreview { .. } | Self::VectorMotion { .. } => region,
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
                    let native = native || chain.iter().any(|(h, ..)| native_pointwise_alpha(scene.effect(*h).unwrap().program));
                    visit(input, scene, native, domains);
                    for mask in masks.iter().flatten() { visit(mask, scene, native, domains); }
                }
                Expression::Color(_) | Expression::Objects {..} | Expression::Vector { .. } | Expression::VectorPreview { .. } | Expression::VectorMotion { .. } => {}
            }
        }
        let mut domains = Default::default();
        visit(self, scene, false, &mut domains);
        domains
    }
    pub(super) fn reads(&self) -> (BTreeSet<SourceTarget>, BTreeSet<OccurrenceHandle>) {
        fn visit(node: &Expression, reads: &mut (BTreeSet<SourceTarget>, BTreeSet<OccurrenceHandle>)) {
            match node {
                Expression::Source { id, .. } => { reads.0.insert(*id); }
                Expression::Opacity { input, .. } => visit(input, reads),
                Expression::Combine { front, back, .. } => { visit(front, reads); visit(back, reads); }
                Expression::Effect { input, chain, masks, .. } => {
                    reads.1.extend(chain.iter().map(|(handle, ..)| *handle));
                    visit(input, reads);
                    for mask in masks.iter().flatten() { visit(mask, reads); }
                }
                Expression::Color(_) | Expression::Objects {..} | Expression::Vector { .. } | Expression::VectorPreview { .. } | Expression::VectorMotion { .. } => {}
            }
        }
        let mut reads = Default::default();
        visit(self, &mut reads);
        reads
    }
    pub(super) fn native_capture(&self, scene: SceneView<'_>, level: u32) -> bool {
        match self {
            Self::Opacity { input, .. } => input.native_capture(scene, level),
            Self::Combine { front, back, .. } => front.native_capture(scene, level) || back.native_capture(scene, level),
            Self::Effect { input, chain, masks, .. } => super::effects::native_alpha_capture(scene, &chain.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(), level)
                || input.native_capture(scene, level) || masks.iter().flatten().any(|mask| mask.native_capture(scene, level)),
            _ => false,
        }
    }
    pub(super) fn moves_other_than(node: &Node, live: &[Moving]) -> bool {
        match node.as_ref() {
            Self::Vector { moving: Some(moving), .. } => !live.contains(moving),
            Self::Opacity { input, .. } => Self::moves_other_than(input, live),
            Self::Combine { front, back, .. } => Self::moves_other_than(front, live) || Self::moves_other_than(back, live),
            Self::Effect { input, masks, .. } => Self::moves_other_than(input, live) || masks.iter().flatten().any(|mask| Self::moves_other_than(mask, live)),
            _ => false,
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
    fn intern(node:&Node,table:&mut HashSet<Node>)->Node {
        if let Some(existing)=table.get(node) {return existing.clone();}
        let interned=match node.as_ref() {
            Self::Opacity {input,opacity}=>Arc::new(Self::Opacity {input:Self::intern(input,table),opacity:*opacity}),
            Self::Combine {front,back,blend,flags}=>Arc::new(Self::Combine {front:Self::intern(front,table),back:Self::intern(back,table),blend:*blend,flags:*flags}),
            Self::Effect {input,chain,masks,radius}=>Arc::new(Self::Effect {input:Self::intern(input,table),chain:chain.clone(),
                masks:masks.iter().map(|mask|mask.as_ref().map(|mask|Self::intern(mask,table))).collect(),radius:*radius}),
            _=>node.clone(),
        };
        table.insert(interned.clone());interned
    }
    fn fused_target(&self)->Option<SourceTarget> {
        match self {
            Self::Source {id,..}=>Some(*id),
            Self::Opacity {input,..}=>input.fused_target(),
            Self::Combine {front,back,blend:0,flags:0} if matches!(back.as_ref(),Self::Color(_))=>front.fused_target(),
            _=>None,
        }
    }
    pub(super) fn fused_transform(&self, r: &WgpuRasterizer) -> bool {
        self.fused_target().is_some_and(|id|r.transforms.as_ref().is_some_and(|t|t.display_source(id)))
    }
}

struct Authored {
    compositions: layer_core::authored::Store<layer_core::authored::Composition>,
    stacks: layer_core::authored::Store<layer_core::authored::Stack>,
    occurrences: layer_core::authored::Store<layer_core::authored::Occurrence>,
    objects: layer_core::authored::Store<layer_core::authored::ImageObject>,
    vectors: layer_core::authored::Store<layer_core::authored::vector::VectorObject>,
    effects: layer_core::authored::Store<layer_core::authored::EffectApplication>,
}
impl PartialEq for Authored {
    fn eq(&self, other: &Self) -> bool {
        self.compositions.same_root(&other.compositions) && self.stacks.same_root(&other.stacks) && self.occurrences.same_root(&other.occurrences)
            && self.objects.same_root(&other.objects) && self.vectors.same_root(&other.vectors) && self.effects.same_root(&other.effects)
    }
}
#[derive(PartialEq)]
struct Inputs {
    scene: (u64, u64, [u64; 2]),
    authored: Authored,
    scope: Option<layer_core::authored::SceneScope>,
    phases: Vec<(OccurrenceHandle, u32)>,
    inspect_mask: Option<OccurrenceHandle>,
    blend_space: layer_core::BlendSpace,
    plan: display_mips::Plan,
    active: Option<OccurrenceHandle>,
    sources: Vec<(SourceTarget, SourceFrame, [u32; 2], u32)>,
    object_moving: Option<OccurrenceHandle>,
    motion_split: Option<super::super::MotionSplit>,
    moving: Option<(OccurrenceHandle, Moving)>,
    prepared: Option<(super::super::MotionSplit, (OccurrenceHandle, Moving), DocRect)>,
}
impl Inputs {
    fn new(packet: FramePacket<'_>, sources: &Sources, plan: display_mips::Plan, active: Option<OccurrenceHandle>) -> Self {
        let scene = packet.scene;let artwork = scene.artwork();
        let outside = |id: SourceTarget| match id {
            SourceTarget::Coverage(handle) => artwork.coverage.get(handle).map_or(0, |coverage| coverage.default_coverage.to_bits()),
            _ => 0,
        };
        let mut framed: Vec<_> = sources.entries.keys().map(|id| (*id, source_frame(Some(sources), scene, *id), scene.target_extent(*id), outside(*id))).collect();
        framed.sort_unstable_by_key(|(id, ..)| *id);
        Self {
            scene: (scene.owner(), scene.revision(), scene.offset().map(f64::to_bits)),
            authored: Authored { compositions: artwork.compositions.clone(), stacks: artwork.stacks.clone(), occurrences: artwork.occurrences.clone(),
                objects: artwork.objects.clone(), vectors: artwork.vectors.clone(), effects: artwork.effects.clone() },
            scope: scene.scope().cloned(),
            phases: scene.order().iter().filter(|handle| scene.effect(**handle).is_some())
                .map(|handle| (*handle, crate::effects::effective_phase(scene, *handle, packet.time_seconds).to_bits())).collect(),
            inspect_mask: packet.inspect_mask, blend_space: packet.blend_space, plan, active, sources: framed, object_moving: sources.object_moving,
            motion_split: sources.motion_split.clone(), moving: sources.moving.clone(), prepared: sources.prepared.clone(),
        }
    }
}

pub(super) struct Parts {pub(super) nodes:[Node;3],pub(super) scopes:crate::scene::vectors::ActiveSplit}

#[derive(Default)]
pub(super) struct Graph {
    pub root: Option<Node>,
    pub content: Option<Node>,
    pub parts: Option<Parts>,
    pub motion: Option<[Node; 2]>,
    pub prepared: Option<[Node; 2]>,
    cached: HashSet<Node>,
    interned: HashSet<Node>,
    effects: HashMap<OccurrenceHandle, (metadata::Metadata, u64)>,
    revision: u64,
    blend_space: layer_core::BlendSpace,
    objects:object_spatial::SpatialIndex,
    active: Option<OccurrenceHandle>,
    inputs: Option<Inputs>,
}
impl Graph {
    pub fn objects(&self)->&object_spatial::SpatialIndex {&self.objects}
    pub fn blend_space(&self)->layer_core::BlendSpace {self.blend_space}
    pub fn without_pixels(&self) -> Self {
        Self { root: self.root.clone(), content:self.content.clone(), parts: self.parts.as_ref().map(|parts|Parts {nodes:parts.nodes.clone(),scopes:parts.scopes.clone()}), motion: None, prepared: None, cached: HashSet::new(), interned: self.interned.clone(), effects: self.effects.clone(),
            revision: self.revision, blend_space: self.blend_space, objects:self.objects.clone(), active: self.active, inputs: None }
    }
    pub fn prepare(&mut self, r: &WgpuRasterizer, packet: FramePacket<'_>, sources: &Sources, plan: display_mips::Plan, _held: bool) -> Result<(), GpuRasterError> {
        let mut stroked = packet.dab_batches.iter().map(|batch| packet.scene.source_owner(batch.target));
        let active = match stroked.next() { Some(Some(owner)) if stroked.all(|other| other == Some(owner)) => Some(owner), _ => r.pivot.or(self.active) };
        let inputs = Inputs::new(packet, sources, plan, active);
        if self.inputs.as_ref() == Some(&inputs) { return Ok(()); }
        #[cfg(test)] crate::test_support::PassCounts {graph_prepares:1,..Default::default()}.record();
        self.inputs = None;
        self.objects.prepare(packet.scene);
        self.blend_space=packet.blend_space;
        self.effects.retain(|id, _| packet.scene.effect(*id).is_some());
        for &handle in packet.scene.order().iter().filter(|handle| packet.scene.effect(**handle).is_some()) {
            let metadata = metadata::Metadata::new(packet.scene, handle);
            if self.effects.get(&handle).is_none_or(|(old, _)| *old != metadata) {
                self.revision += 1;
                self.effects.insert(handle, (metadata, self.revision));
            }
        }
        self.motion.take();
        let mut halves = |split: &super::super::MotionSplit, moving: &(OccurrenceHandle, Moving)| -> Result<[Node; 2], GpuRasterError> {
            let mut half = |scope| compose(FramePacket { scene: packet.scene.with_scope(scope), ..packet }, Some(sources), Some(&self.effects), plan.level, r.device.working_space(), None, Some(moving))
                .map(|node| if Expression::is_clear(&node) { node } else { Arc::new(Expression::Combine { front: node, back: Expression::color([0.; 4]), blend: 0, flags: 0 }) })
                .map(|node| Expression::intern(&node, &mut self.interned));
            Ok([half(&split.0)?, half(&split.1)?])
        };
        self.motion = sources.motion_split.as_ref().zip(sources.moving.as_ref()).map(|(split, moving)| halves(split, moving)).transpose()?;
        self.prepared = sources.prepared.as_ref().filter(|_| self.motion.is_none()).map(|(split, moving, _)| halves(split, moving)).transpose()?;
        self.active = active;
        let (root, parts) = match &self.motion {
            Some([above, below]) => (Expression::combine(above.clone(), below.clone(), layer_core::LayerBlend::Normal, 0), None),
            None => split_root(packet, Some(sources), Some(&self.effects), plan.level, r.device.working_space(), Some(&mut self.objects), sources.moving.as_ref(), self.active)?,
        };
        let root = Expression::intern(&root, &mut self.interned);
        let parts = parts.map(|parts| Parts { nodes: parts.nodes.map(|node| Expression::intern(&node, &mut self.interned)), ..parts });
        let content = if self.root.as_ref() == Some(&root) { self.content.clone() } else {
            let whole = if parts.is_some() { compose(packet, Some(sources), Some(&self.effects), plan.level, r.device.working_space(), Some(&mut self.objects), sources.moving.as_ref())? } else { root.clone() };
            Some(Expression::intern(&Expression::content(&whole), &mut self.interned))
        };
        let mut all = HashSet::new();
        Expression::visit(&root, &mut all);
        for node in self.prepared.iter().flatten() { Expression::visit(node, &mut all); }
        let unmoved: Vec<Node> = all.iter().filter_map(|node| match node.as_ref() {
            Expression::Vector { owner, offset, moving: Some(_) } => Some(Expression::intern(&Arc::new(Expression::Vector { owner: *owner, offset: *offset, moving: None }), &mut self.interned)),
            _ => None,
        }).collect();
        all.extend(unmoved);
        self.interned = all.clone();
        for node in self.motion.iter().flatten().chain(&content) { Expression::visit(node, &mut self.interned); }
        let halves: HashSet<&Node> = parts.iter().flat_map(|parts| [&parts.nodes[0], &parts.nodes[2]]).chain(self.motion.iter().flatten()).chain(self.prepared.iter().flatten()).collect();
        self.cached = all.into_iter().filter(|n| !Arc::ptr_eq(n, &root)
            && (matches!(n.as_ref(), Expression::Objects {..} | Expression::Vector { .. } | Expression::Effect { .. }) || halves.contains(n))
            && !matches!(n.as_ref(), Expression::Combine { front, .. } if matches!(front.as_ref(), Expression::VectorPreview { .. } | Expression::VectorMotion { .. }))).collect();
        self.content = content;
        self.root = Some(root);
        self.parts = parts.filter(|_| !super::refinement::exact_at_view_level(packet.scene));
        self.inputs = Some(inputs);
        Ok(())
    }
    pub fn caches(&self, node: &Node) -> bool { self.cached.contains(node) }
    pub fn active(&self) -> Option<OccurrenceHandle> { self.parts.as_ref().and(self.active) }
    pub fn storage_bytes(&self) -> u64 { self.objects.storage_bytes() }

}


#[expect(clippy::too_many_arguments, reason = "The split root composes each part with the full root's inputs")]
pub(super) fn split_root(
    packet: FramePacket<'_>, sources: Option<&Sources>, effects: Option<&HashMap<OccurrenceHandle, (metadata::Metadata, u64)>>,
    level: u32, space: layer_core::color::RgbSpace, mut objects: Option<&mut object_spatial::SpatialIndex>, moving: Option<&(OccurrenceHandle, Moving)>,
    active: Option<OccurrenceHandle>,
) -> Result<(Node, Option<Parts>), GpuRasterError> {
    let Some(split_scopes) = active.filter(|_| packet.inspect_mask.is_none()).and_then(|owner| crate::scene::active_split(packet.scene, owner)) else {
        return compose(packet, sources, effects, level, space, objects, moving).map(|root| (root, None));
    };
    let mut part = |scope| compose(FramePacket { scene: packet.scene.with_scope(scope), ..packet }, sources, effects, level, space, objects.as_deref_mut(), moving);
    let nodes = [part(&split_scopes[0])?, part(&split_scopes[1])?, part(&split_scopes[2])?];
    let root = Expression::combine(nodes[0].clone(), Expression::combine(nodes[1].clone(), nodes[2].clone(), layer_core::LayerBlend::Normal, 0), layer_core::LayerBlend::Normal, 0);
    let pivot = !Expression::is_clear(&nodes[1]);
    Ok((root, pivot.then_some(Parts { nodes, scopes: split_scopes })))
}

pub(super) fn compose(
    packet: FramePacket<'_>, sources: Option<&Sources>, effects: Option<&HashMap<OccurrenceHandle, (metadata::Metadata, u64)>>,
    level: u32, space: layer_core::color::RgbSpace, objects:Option<&mut object_spatial::SpatialIndex>, moving: Option<&(OccurrenceHandle, Moving)>,
) -> Result<Node, GpuRasterError> {
    let presented = layer_core::paper::separate_paper(packet.scene).map(|(paper, _)| paper);
    let mut builder = Builder { packet, sources, effects, level, space, objects, presented, moving };
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
    presented: Option<OccurrenceHandle>,
    moving: Option<&'a (OccurrenceHandle, Moving)>,
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
    fn clear(&mut self) -> Result<Self::Image, GpuRasterError> { Ok(Vec::new()) }
    fn discard(&mut self, _: Self::Image) {}
    fn duplicate(&mut self, image: &Self::Image) -> Result<Self::Image, GpuRasterError> { Ok(image.clone()) }
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
        } else if self.packet.scene.vector_layer(index).is_some() {
            let moving = self.moving.filter(|(owner, _)| *owner == index).map(|(_, items)| items.clone());
            let vector = Arc::new(Expression::Vector { owner: index, offset: self.packet.scene.occurrence_offset64(index).map(f64::to_bits), moving });
            let moving = Expression::combine(Arc::new(Expression::VectorMotion { owner: index }), vector, layer_core::LayerBlend::Normal, 0);
            vec![Expression::combine(Arc::new(Expression::VectorPreview { owner: index }), moving, layer_core::LayerBlend::Normal, 0)]
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
        matches!(layer.kind(), LayerKind::Group | LayerKind::Paint | LayerKind::Effect) || self.packet.scene.object_layer(index).is_some() || self.packet.scene.vector_layer(index).is_some()
    }
    fn presented_backdrop(&self) -> Option<OccurrenceHandle> { self.presented }
}

impl Cache {
    pub(super) fn initialized_pages(&self,target:&Target)->Option<BTreeSet<[u32;2]>> {
        if self.pixels.root().is_some_and(|window|window.view==target.view) {return Some(self.shown_pages(target.plan.bounds));}
        self.pages.lock().unwrap().initialized_view(&target.view,target.plan.bounds)
    }
}
impl Evaluator<'_> {
    pub(super) fn evaluate_root(&mut self, node: &Node, target: Root) -> Result<Value, GpuRasterError> {
        if target==Root::Store || self.accepted.declaring() {return self.evaluate_page(node,self.class)?.ok_or(GpuRasterError::NotReady);}
        let region=self.region.in_frame(self.cache.plan.extent);
        let key=self.cache.page_key(node.clone(),region);
        let mut write=if self.cache.gesture.is_some() {pages::Write::keyless(key,region)}
            else {self.cache.pages.lock().unwrap().prepare_write(key,region,pages::Accuracy::Display,self.cache.required_input_level())?};
        let window=self.cache.pixels.root().ok_or(GpuRasterError::NotReady)?;
        let output=Target {view:window.view.clone(),slot:Some(Slot::Root),plan:window.plan};
        let fused=(window.texture.format()==wgpu::TextureFormat::Rgba32Float).then(||output.clone());
        let outer=std::mem::replace(&mut self.approximate,false);
        let value=self.evaluate_uncached(node,fused)?;
        if super::refinement::exact_at_view_level(self.packet.scene) && !self.approximate {write.accuracy=pages::Accuracy::Exact;}
        self.approximate|=outer;
        let value=self.materialize(value,Some(output))?;
        self.unit.window.push(write);
        Ok(value)
    }
    fn evaluate_page(&mut self,node:&Node,class:Class)->Result<Option<Value>,GpuRasterError> {
        if self.accepted.declaring() {return self.evaluate_uncached(node,None).map(Some);}
        let region=self.region.in_frame(self.cache.plan.extent);
        let required_input_level=self.cache.required_input_level();
        let mut key=self.cache.page_key(node.clone(),region);
        let exact_at_view=super::refinement::exact_at_view_level(self.packet.scene);
        let presented=super::refinement::presented_accuracy(self.r,self.packet.scene,self.preview);
        let exact_input_level=super::refinement::exact_input_level(self.packet.scene,required_input_level);
        let cached={
            let mut store=self.cache.pages.lock().unwrap();
            store.get(&key,region,pages::Accuracy::Display,required_input_level,self.class).or_else(||store.get(&key,region,pages::Accuracy::Exact,0,self.class)).map(|image| {
                let exact=store.get(&key,region,pages::Accuracy::Exact,exact_input_level,self.class).is_some();(image,exact)
            })
        };
        if let Some((image,exact))=cached.filter(|(_,exact)|*exact || presented==pages::Accuracy::Display) {
            if self.cache.graph.content.as_ref()==Some(&key.expression) {self.cache.pages.lock().unwrap().set_visible(&key);}
            self.approximate|=!exact;
            self.unit.reads.push(image.clone());
            return Ok(Some(Target {view:image.view.clone(),slot:Some(Slot::Root),plan:image.plan}.value()));
        }
        if !self.cache.pages.lock().unwrap().coverage(&key,region,required_input_level).needs(region,presented) {
            return Err(GpuRasterError::NotReady);
        }
        let reduction=self.cache.pages.lock().unwrap().reduction(&key,required_input_level,self.class)
            .map(|(inputs,accuracy)|(inputs,if exact_at_view {pages::Accuracy::Display}else {accuracy}))
            .filter(|(_,accuracy)|*accuracy==pages::Accuracy::Exact || presented==pages::Accuracy::Display);
        if matches!(&reduction,Some((_,pages::Accuracy::Exact))) {key.format=wgpu::TextureFormat::Rgba32Float;}
        let Ok(image)=self.cache.claim_page(self.scene,self.r,key.clone(),class)? else {return Ok(None);};
        self.unit.reads.push(image.clone());
        if let Some((inputs,accuracy))=reduction {
            let write=self.cache.pages.lock().unwrap().prepare_write(key.clone(),image.plan.bounds,accuracy,required_input_level)?;
            for (_,input) in inputs {
                let covered=input.plan.bounds.intersect(image.plan.bounds);
                let side=1<<image.plan.level;
                let [x,y,width,height]=paint_transform::texel_rect(covered.window_local(image.plan.bounds),side);
                let sources=[input.view.clone(),self.r.empty_view.clone(),self.r.empty_view.clone(),self.r.empty_view.clone()];
                let mut values=[0;20];
                values[..8].copy_from_slice(&[x,y,width,height,input.plan.bounds.width(),input.plan.bounds.height(),2,(input.plan.level<<8)|8]);
                values[14]=((x*2) as f32).to_bits();values[15]=((y*2) as f32).to_bits();
                self.scene.jobs.push(Job::Reduce {output:image.view.clone(),sources,values,size:[width,height],kernel:Reduction::Pages,format:image.texture.format()});
                self.unit.reads.push(input);
            }
            self.approximate|=accuracy==pages::Accuracy::Display;
            self.unit.pages.push(write);
            return Ok(Some(Target {view:image.view.clone(),slot:Some(Slot::Root),plan:image.plan}.value()));
        }
        let mut write=self.cache.pages.lock().unwrap().prepare_write(key.clone(),region,pages::Accuracy::Display,required_input_level)?;
        let output=Target {view:image.view.clone(),slot:Some(Slot::Root),plan:image.plan};
        let direct=(image.texture.format()==wgpu::TextureFormat::Rgba32Float || matches!(node.as_ref(),Expression::Vector {..})).then(||output.clone());
        let outer=std::mem::replace(&mut self.approximate,false);
        let value=self.evaluate_uncached(node,direct)?;
        if exact_at_view && !self.approximate {write.accuracy=pages::Accuracy::Exact;}
        self.approximate|=outer;
        let value=self.materialize(value,Some(output))?;
        self.unit.pages.push(write);
        Ok(Some(value))
    }

    fn combine(&mut self, front: &Node, back: &Node, blend: u32, flags: u32, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let (front, back) = if front.cost().1 >= back.cost().1 {
            let front = self.evaluate(front)?;
            (front, self.evaluate(back)?)
        } else {
            let back = self.evaluate(back)?;
            (self.evaluate(front)?, back)
        };
        if blend == 0 && flags == 0 && matches!(front, Value::Color(c) if c == [0.; 4]) { self.materialize(back, output) }
        else { self.draw(front, back, layer_core::LayerBlend::ALL[blend as usize], flags, output) }
    }
    pub(super) fn evaluate(&mut self, node: &Node) -> Result<Value, GpuRasterError> {
        self.evaluate_into(node, None)
    }
    fn evaluate_into(&mut self, node: &Node, output: Option<Target>) -> Result<Value, GpuRasterError> {
        let region=self.region.in_frame(self.cache.plan.extent);
        if output.is_none() && self.cache.graph.caches(node) && !self.cache.contact_reads(node)
            && DocRect::from(region)==self.region && self.cache.page_key(node.clone(),region).plan().bounds==region
            && let Some(value)=self.evaluate_page(node,self.class.max(Class::Prefetch))? {return Ok(value);}
        self.evaluate_uncached(node,output)
    }
    fn repaints(&mut self,node:&Node,owner:OccurrenceHandle,output:&Target)->bool {
        let region=self.region.in_frame(self.cache.plan.extent);
        let key=self.cache.page_key(node.clone(),region);
        let store=self.cache.pages.lock().unwrap();
        let Some((previous,exact))=store.superseded(&key,&output.view,region,self.cache.required_input_level()) else {return false;};
        let continues=previous.scene().vector_layer(owner).is_some_and(|previous|self.scene.vectors.changes_from(self.packet.scene,owner,previous));
        self.approximate|=continues && !exact;
        continues
    }
    fn evaluate_uncached(&mut self,node:&Node,output:Option<Target>)->Result<Value,GpuRasterError> {
        let outer_approximate=std::mem::replace(&mut self.approximate,false);
        let result=match node.as_ref() {
            Expression::Color(c)=>Ok(Value::Color(c.map(f32::from_bits))),
            Expression::Objects {content,preview}=>self.objects(content,*preview,output),
            Expression::Vector {owner,moving,..}=>{let append=output.as_ref().is_some_and(|output|self.repaints(node,*owner,output));self.vector(*owner,moving.as_ref(),output,append)},
            Expression::VectorPreview {owner}=>self.vector_preview(*owner,output),
            Expression::VectorMotion {owner}=>self.vector_motion(*owner,output),
            Expression::Source {id,frame,extent,outside}=>self.source(*id,*frame,*extent,f32::from_bits(*outside)),
            Expression::Opacity {input,opacity}=>self.evaluate(input).map(|value|value.with_opacity(f32::from_bits(*opacity))),
            Expression::Combine {front,back,blend,flags}=>self.combine(front,back,*blend,*flags,output),
            Expression::Effect {input,chain,masks,..}=>self.effect(input,chain,masks,output),
        };
        self.approximate|=outer_approximate;
        match result {
            Err(GpuRasterError::NotReady|GpuRasterError::NotPublished)=>{self.unit.waiting=true;Ok(Value::Color([0.;4]))},
            result=>result,
        }
    }

}


#[cfg(test)]
mod reuse_tests {
    use super::*;
    use super::super::tests::{paint_mut, paint_occurrence, insert_occurrence, effect_occurrence};
    use layer_core::{color::source::rgba8_source, authored::Attachment};
    use crate::test_support::{packet, float_pixels as pixels};
    use super::super::tests::{document_at, add_fill, coverage_mask, set_effect_value, display_pixels, submit_published};
    use layer_core::{EffectValue, Point, Selection};
    use layer_core::color::RgbColor;

    fn retained_branch_pages(cache:&Cache)->Vec<pages::RetainedPage> {
        cache.pages.lock().unwrap().retained_pages().into_iter().filter(|page|page.key.source.is_none()
            && page.key.level==cache.plan.level && !page.written.is_empty()
            && Some(&page.key.expression)!=cache.graph.root.as_ref() && Some(&page.key.expression)!=cache.graph.content.as_ref()
            && cache.graph.caches(&page.key.expression)).collect()
    }

    #[test]
    fn fill_edits_recompose_without_reusing_old_pixels() {
        let mut doc = document_at([517, 259]);
        let paint = doc.scene().order()[0];
        doc.artwork.occurrences.get_mut(paint).unwrap().opacity = 0.37;
        let color = |rgb, alpha| RgbColor::from_linear(doc.composition().color.space, [rgb, 0.27, 0.61, alpha]).unwrap();
        let initial = color(0.13, 0.43);
        let unchanged=effect_occurrence(&mut doc,layer_core::EffectInstance::new(crate::tests::fixture("exposure").program()),"Unchanged paint");
        doc.artwork.occurrences.get_mut(unchanged).unwrap().attachment=Attachment::Effect;
        insert_occurrence(&mut doc,unchanged,0);
        let fill = add_fill(&mut doc, initial);
        coverage_mask(&mut doc, fill, [0, 0], Some(Selection::polygon(vec![
            Point { x: 0., y: 0. }, Point { x: 301., y: 0. },
            Point { x: 301., y: 259. }, Point { x: 0., y: 259. },
        ]).unwrap()));
        let mut renderer = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
        let extent = doc.composition().size;
        let mut previous_pixels: Option<Vec<[f32; 4]>> = None;
        for (red, alpha) in [(0.13, 0.43), (0.83, 0.71), (0.31, 0.19)] {
            let value = RgbColor::from_linear(doc.composition().color.space, [red, 0.27, 0.61, alpha]).unwrap();
            set_effect_value(&mut doc, fill, "color", EffectValue::Color(value));
            let mut frame = crate::test_support::packet(doc.scene(), extent);
            frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
            submit_published(&mut renderer, frame);
            assert!(renderer.scale_display.as_ref().unwrap().storage_bytes()<=crate::memory::CACHE_BYTES);
            let actual = display_pixels(&renderer);
            let mut fresh = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
            submit_published(&mut fresh, frame);
            let expected = display_pixels(&fresh);
            assert!(crate::test_support::max_error(&actual, &expected) < 2e-5,
                "recycled validity must not expose old masked/translucent Fill pixels");
            if let Some(previous) = previous_pixels {
                assert!(crate::test_support::max_error(&actual, &previous) > 0.01, "visible Fill edits must change pixels");
            }
            previous_pixels = Some(actual);
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
        let mut r = crate::memory::memory_tests::legacy_scene_renderer(doc.composition().color);
        let frame = packet(doc.scene(), extent); crate::memory::memory_tests::legacy_scene_frame(&mut r, frame); submit_published(&mut r, frame); r.wait_idle().unwrap();
        let reference = display_pixels(&r);
        let mut scene = r.scene.take().unwrap(); let mut cache = r.scale_display.take().unwrap();
        r.document_damage.clear(); r.transform_damage.clear();
        assert_eq!(cache.plan.level, 0); assert_eq!(cache.plan.bounds, PixelRect::full(extent));
        assert!(retained_branch_pages(&cache).iter().any(|page|matches!(page.key.expression.as_ref(),Expression::Effect {..})));
        for sparse in [false, true] {
            if !sparse {tests::evict_composite_pages(&cache,&[PixelRect::full(extent)]);}
            if sparse {
                tests::evict_composite_pages(&cache,&[[0,0],[count-1,0]].map(page_rect));
            }
            let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default()); let mut commands = RegionRecords::new(&r);
            let work = r.metrics.composited_pixels;
            {let mut work=crate::test_support::work(false);cache.render_graph(&mut scene, &mut r, frame,
                &mut Encoding { encoder: &mut encoder, commands: &mut commands, retained:0, budget:&mut work }, Destination::View).unwrap();}
            scene.encode_jobs(&mut r, &mut encoder).unwrap();
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
            let effects=retained_branch_pages(&cache).into_iter().filter(|page|matches!(page.key.expression.as_ref(),Expression::Effect {..}))
                .map(|page|page.key.expression).collect::<HashSet<_>>();
            assert!(!effects.is_empty());
            for node in effects {for coordinate in pages::coordinates(cache.plan.bounds,cache.plan.level) {
                let key=cache.page_key(node.clone(),pages::bounds(coordinate,cache.plan.level,extent));
                assert!(cache.pages.lock().unwrap().get(&key,key.plan().bounds,pages::Accuracy::Display,key.level,crate::residency::Class::Visible).is_some(),
                    "every retained effect branch page must remain committed and current");
            }}
        }
        r.scene = Some(scene); r.scale_display = Some(cache);
    }}
}

}

#[cfg(test)]
mod object_reuse_tests {
    use super::*;
    use layer_core::{Edit, authored::*};
    fn collection(root:&Node,owner:OccurrenceHandle)->Node {
        let mut nodes=HashSet::new();Expression::visit(root,&mut nodes);
        assert!(nodes.len()<4*1024+8,"Sibling image layers produce a linear number of expressions");
        nodes.into_iter().find(|node|matches!(node.as_ref(),Expression::Objects {content,..} if content.owner()==owner)).unwrap()
    }
    #[test]
    fn paint_only_edits_reuse_the_object_collection_branch_and_object_edits_replace_it() {
        let (mut doc,owner,handles)=object_spatial::tests::document();
        let mut cache=object_spatial::SpatialIndex::default();
        let build=|doc:&layer_core::Document,cache:&mut object_spatial::SpatialIndex| {
            collection(&compose(crate::test_support::packet(doc.scene(),doc.composition().size),None,None,0,doc.composition().color.space,Some(cache),None).unwrap(),owner)
        };
        let before=build(&doc,&mut cache);
        let paint=doc.artwork.paint.iter().next().unwrap().0;
        let mut source=doc.artwork.paint.get(paint).unwrap().clone();source.domain=[257,256];
        doc.apply(Edit::Paint(RecordChange::replace(&doc.artwork.paint,paint,Some(source)).unwrap())).unwrap();
        let painted=build(&doc,&mut cache);
        assert!(before==painted);
        let packet=crate::test_support::packet(doc.scene(),doc.composition().size);
        let mut planning=cache.clone();
        let planned=collection(&compose(packet,None,None,2,doc.composition().color.space,Some(&mut planning),None).unwrap(),owner);
        assert!(before==planned,"requested levels retain the same collection index after a paint edit");
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
