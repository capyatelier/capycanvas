use super::*;
use crate::authored::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    Down,
    Group,
    Range { first: OccurrenceHandle, last: OccurrenceHandle },
    Visible,
    Flatten,
    Stamp,
}

/// What Merge Down does for the active layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeDown {
    /// Merge into the layer below.
    Layer,
    /// A clipping base, or an adjustment clipped to one, bakes its stack.
    ClippingStack,
    /// An unclipped adjustment applies to the layer below only.
    ApplyEffect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeRefusal {
    NoLayer,
    SelectionLayer,
    Hidden,
    NotNormal,
    Locked,
    NoLayerBelow,
    BelowHidden,
    BelowLocked,
    BelowNotNormal,
    BelowEffect,
    BelowClipped,
    BaseHidden,
    BaseNotNormal,
    ClipsHidden,
    NotGroup,
    NothingVisible,
    SelectionLayersInside,
    TooLarge,
    UnboundedSupport,
}

#[derive(Clone, Debug)]
pub struct MergePlan {
    pub edits: Vec<Edit>,
    pub result: OccurrenceHandle,
    pub target: SourceTarget,
    pub operation: RasterOperation,
}
struct Merge {
    members: BTreeSet<OccurrenceHandle>,
    discarded: BTreeSet<OccurrenceHandle>,
    released: Vec<OccurrenceHandle>,
    anchor: Option<OccurrenceHandle>,
    group: bool,
}
fn adjustment(scene: SceneView<'_>, h: OccurrenceHandle) -> bool {
    scene.effect(h).is_some_and(|e| e.program.kind == EffectKind::Adjustment)
}
/// Signed evaluation-space bounds in F64, before rounding to the document grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SupportBounds { pub min: [f64; 2], pub max: [f64; 2] }
impl SupportBounds {
    pub const EMPTY: Self = Self { min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2] };
    pub fn is_empty(self) -> bool { (0..2).any(|axis| self.min[axis] >= self.max[axis]) }
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() { return other; }
        if other.is_empty() { return self; }
        Self { min: std::array::from_fn(|axis| self.min[axis].min(other.min[axis])), max: std::array::from_fn(|axis| self.max[axis].max(other.max[axis])) }
    }
    fn outset(self, amount: [f64; 2]) -> Self {
        if self.is_empty() { return self; }
        Self { min: std::array::from_fn(|axis| self.min[axis] - amount[axis]), max: std::array::from_fn(|axis| self.max[axis] + amount[axis]) }
    }
    fn offset(self, by: [f64; 2]) -> Self {
        if self.is_empty() { return self; }
        Self { min: std::array::from_fn(|axis| self.min[axis] + by[axis]), max: std::array::from_fn(|axis| self.max[axis] + by[axis]) }
    }
    fn extent(origin: [f64; 2], extent: [u32; 2]) -> Self {
        Self { min: origin, max: std::array::from_fn(|axis| origin[axis] + f64::from(extent[axis])) }
    }
    /// The document pixels this support touches, or `None` when it can't be
    /// addressed by the editor's integer offsets.
    pub fn grid(self) -> Option<[[i64; 2]; 2]> {
        if self.is_empty() { return None; }
        let min = self.min.map(f64::floor);
        let max = self.max.map(f64::ceil);
        let limit = offsets::MAX_OFFSET as f64 + MAX_EXTENT as f64;
        min.iter().chain(&max).all(|v| v.is_finite() && v.abs() <= limit).then(|| [min.map(|v| v as i64), max.map(|v| v as i64)])
    }
    fn rect(self) -> Rect {
        if self.is_empty() { return Rect::EMPTY; }
        Rect { min: Point { x: self.min[0] as f32, y: self.min[1] as f32 }, max: Point { x: self.max[0] as f32, y: self.max[1] as f32 } }
    }
}

fn paint_support(scene: SceneView<'_>, h: OccurrenceHandle, reach: Reach) -> SupportBounds {
    let Some(paint) = scene.paint_source(h) else { return SupportBounds::EMPTY; };
    let origin = scene.occurrence_offset64(h);
    let content = match paint.raster.try_data() {
        Some(Ok(data)) if paint.operations.is_empty() && reach == Reach::Content => data.tiles.keys().fold(SupportBounds::EMPTY, |bounds, key| {
            let tile = key.coordinate.map(|v| f64::from(v) * f64::from(raster::TILE_SIZE));
            bounds.union(SupportBounds::extent(tile, [raster::TILE_SIZE; 2]))
        }),
        _ => return SupportBounds::extent(origin, paint.domain),
    };
    let base = paint.base.as_ref().map_or(SupportBounds::EMPTY, |base| SupportBounds::extent(base.offset.map(f64::from), base.image.extent));
    let domain = SupportBounds::extent([0.; 2], paint.domain);
    let local = content.union(base);
    SupportBounds { min: std::array::from_fn(|axis| local.min[axis].max(domain.min[axis])), max: std::array::from_fn(|axis| local.max[axis].min(domain.max[axis])) }
        .offset(origin)
}

/// Where an image object can contribute pixels: its rectangle, widened by the
/// smooth filter's footprint unless it maps samples exactly onto the grid.
pub fn object_support(placement: Affine64, object: &ImageObject) -> SupportBounds {
    let [min, max] = placement.bounds(object.image.extent);
    let [a, b, c, d, tx, ty] = placement.0;
    let permutation = [a, b, c, d].iter().all(|v| *v == 0. || v.abs() == 1.) && (a * d - b * c).abs() == 1.;
    let exact = object.interpolation == ImageInterpolation::Nearest || (permutation && tx.fract() == 0. && ty.fract() == 0.);
    let halo = if exact { [0.; 2] } else { [(a.abs() + c.abs()) * 0.5 + 2., (b.abs() + d.abs()) * 0.5 + 2.] };
    SupportBounds { min, max }.outset(halo)
}

fn objects_support(scene: SceneView<'_>, h: OccurrenceHandle) -> SupportBounds {
    let Some(layer) = scene.object_layer(h) else { return SupportBounds::EMPTY; };
    let origin = scene.occurrence_offset64(h);
    let placement = Affine64([1., 0., 0., 1., origin[0], origin[1]]);
    object_support(placement.compose(layer.affine), layer)
}

fn vectors_support(scene: SceneView<'_>, h: OccurrenceHandle) -> SupportBounds {
    let Some([x0, y0, x1, y1]) = scene.vector_layer(h).and_then(|object| object.bounds()) else { return SupportBounds::EMPTY; };
    let [x, y] = scene.occurrence_offset64(h);
    SupportBounds { min: [x0 + x, y0 + y], max: [x1 + x, y1 + y] }.outset([1.; 2])
}

/// Whether paint contributes its painted pages or its whole editable domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach { Content, Domain }

/// The finite support of everything `scene` evaluates in its scope: paint,
/// object images with their sampling footprint, vector paths with their
/// strokes, and the output of filters and generators. A generator's output is defined on the authored frame; a filter
/// whose reach can't be represented is refused with `UnboundedSupport`.
pub fn output_support(scene: SceneView<'_>, reach: Reach) -> Result<SupportBounds, MergeRefusal> {
    let visible: Vec<_> = scene.order().iter().copied().filter(|h| layer_is_visible(scene, *h)).collect();
    let mut bounds = visible.iter().fold(SupportBounds::EMPTY, |bounds, h| bounds.union(paint_support(scene, *h, reach)).union(objects_support(scene, *h)).union(vectors_support(scene, *h)));
    let frame = SupportBounds::extent(scene.evaluation_offset64(), scene.composition().size);
    for effect in visible.iter().filter_map(|h| scene.effect(*h)) {
        match (effect.program.kind, effect.program.alpha) {
            (EffectKind::Generator, _) => bounds = bounds.union(frame),
            (EffectKind::Adjustment, EffectAlpha::Filter) => {
                bounds = bounds.outset([f64::from(effect.support_radius().ok_or(MergeRefusal::UnboundedSupport)?); 2]);
            }
            _ => {}
        }
    }
    Ok(bounds)
}

pub(crate) fn bake_bounds(snapshot: &SceneSnapshot, scope: &SceneScope, offset: Point, extent: [u32; 2]) -> Rect {
    let scene = snapshot.view().with_scope(scope).with_offset(offset);
    output_support(scene, Reach::Content).map_or(Rect::from_extent(extent), |support| support.rect().intersect(Rect::from_extent(extent)))
}
impl Document {
    fn clips_above(&self,h:OccurrenceHandle)->Vec<OccurrenceHandle> {
        let scene=self.scene();scene.children(scene.parent(h)).iter().rev().copied().filter(|member|scene.clipping_base(*member)==Some(h)).collect()
    }
    fn sibling_below(&self, h: OccurrenceHandle) -> Option<OccurrenceHandle> {
        let scene = self.scene();
        let siblings = scene.children(scene.parent(h));
        let index = siblings.iter().position(|s| *s == h)?;
        siblings[index + 1..].iter().copied().find(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork()))
    }
    pub fn merge_down(&self) -> MergeDown {
        let Some(h) = self.working.occurrence else {
            return MergeDown::Layer;
        };
        let scene = self.scene();
        let Some(o) = scene.occurrence(h) else {
            return MergeDown::Layer;
        };
        if let Some(owner)=scene.effect_owner(h) {return if scene.clipping_base(owner).is_some()||!self.clips_above(owner).is_empty(){MergeDown::ClippingStack}else{MergeDown::ApplyEffect};}
        match (o.attachment.is_clip(), adjustment(scene, h)) {
            (true, true) => MergeDown::ClippingStack,
            (true, false) => MergeDown::Layer,
            (false, _) if !self.clips_above(h).is_empty() => MergeDown::ClippingStack,
            (false, true) => MergeDown::ApplyEffect,
            _ => MergeDown::Layer,
        }
    }
    pub fn merge_refusal(&self, kind: MergeKind) -> Option<MergeRefusal> {
        self.merge(kind).err()
    }
    /// Merge Down of a plain vector layer onto another keeps paths: the upper
    /// layer's paths go in front of the lower layer's, in one edit.
    pub fn vector_merge_down(&self) -> Option<Vec<Edit>> {
        let m = self.merge(MergeKind::Down).ok()?;
        let lower = m.anchor?;
        let upper = self.working.occurrence?;
        let scene = self.scene();
        if m.members != BTreeSet::from([upper, lower]) { return None; }
        let plain = |h: OccurrenceHandle| scene.occurrence(h).filter(|o| o.opacity == 1. && o.mask.is_none() && !o.attachment.is_clip() && scene.attached_effects(h).is_empty());
        let (top, bottom) = (plain(upper)?, plain(lower)?);
        let (OccurrenceContent::Vector(top_object), OccurrenceContent::Vector(bottom_object)) = (&top.content, &bottom.content) else { return None };
        let (paths, base) = (self.artwork.vectors.get(*top_object)?, self.artwork.vectors.get(*bottom_object)?);
        if !paths.holds_only_known_paths() { return None; }
        let [ux, uy] = scene.occurrence_offset64(upper);
        let [lx, ly] = scene.occurrence_offset64(lower);
        let moved = paths.transformed(Affine64([1., 0., 0., 1., ux - lx, uy - ly])).ok()?;
        let mut taken = BTreeSet::new();
        let mut fresh = base.allocate_ids().filter(|id| !moved.contains(*id));
        let records = moved.paths().map(|(_, path)| {
            let id = if base.contains(path.id) || !taken.insert(path.id) { fresh.next().unwrap_or_default() } else { path.id };
            vector::Record::Path(vector::PathItem { id, ..path.clone() })
        }).collect();
        let mut edits = vec![Edit::Vector(RecordChange::replace(&self.artwork.vectors, *bottom_object, Some(base.insert_front(records))).ok()?)];
        for (h, _, before) in self.artwork.stacks.iter().filter(|(_, _, stack)| stack.entries.contains(&upper)) {
            let mut stack = before.clone();
            stack.entries.retain(|entry| *entry != upper);
            edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, h, Some(stack)).ok()?));
        }
        edits.extend(self.removal_edits(&BTreeSet::from([upper])).ok()?);
        let mut working = self.working.clone();
        working.occurrence = Some(lower);
        working.layer_selection = [lower].into();
        working.layer_anchor = Some(lower);
        working.target = None;
        working.vector_items = None;
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        Some(edits)
    }
    /// The vector layers a merge would turn into pixels.
    pub fn merge_rasterizes(&self, kind: MergeKind) -> Vec<OccurrenceHandle> {
        if kind == MergeKind::Down && self.vector_merge_down().is_some() { return Vec::new(); }
        let Ok(m) = self.merge(kind) else { return Vec::new() };
        let scene = self.scene();
        scene.order().iter().copied().filter(|h| m.members.contains(h) && scene.vector_layer(*h).is_some()).collect()
    }
    pub fn flatten_discards(&self) -> usize {
        self.merge(MergeKind::Flatten).map_or(0, |m| {
            let scene = self.scene();
            m.discarded
                .iter()
                .chain(&m.members)
                .filter(|h| {
                    scene
                        .occurrence(**h)
                        .is_some_and(|o| !o.visible && scene.parent(**h).is_none_or(|p| scene.occurrence(p).is_some_and(|o| o.visible)))
                })
                .count()
                + m.discarded.iter().filter(|h| scene.parent(**h).is_none() && scene.occurrence(**h).is_some_and(|o| o.visible)).count()
        })
    }
    fn checked(&self, members: BTreeSet<OccurrenceHandle>) -> Result<BTreeSet<OccurrenceHandle>, MergeRefusal> {
        for h in &members {
            if self.is_locked(*h) {
                return Err(MergeRefusal::Locked);
            }
            if self.scene().occurrence(*h).is_some_and(|o| o.kind() == LayerKind::Selection) {
                return Err(MergeRefusal::SelectionLayersInside);
            }
        }
        Ok(members)
    }
    fn merge(&self, kind: MergeKind) -> Result<Merge, MergeRefusal> {
        let scene = self.scene();
        let mut m = Merge {
            members: Default::default(),
            discarded: Default::default(),
            released: Vec::new(),
            anchor: None,
            group: false,
        };
        match kind {
            MergeKind::Range { first, last } => {
                if first == last || scene.occurrence(first).is_none() || scene.occurrence(last).is_none() {
                    return Err(MergeRefusal::NoLayerBelow);
                }
                let ancestors = |mut h| {
                    let mut parents = vec![scene.parent(h)];
                    while let Some(parent) = scene.parent(h) { h = parent; parents.push(scene.parent(h)); }
                    parents
                };
                let other = ancestors(last);
                let parent = ancestors(first).into_iter().find(|parent| other.contains(parent)).ok_or(MergeRefusal::NoLayer)?;
                let root = |mut h| {
                    while scene.parent(h) != parent { h = scene.parent(h).ok_or(MergeRefusal::NoLayer)?; }
                    Ok::<_, MergeRefusal>(h)
                };
                let (first, last) = (root(first)?, root(last)?);
                let siblings = scene.children(parent);
                let a = siblings.iter().position(|h| *h == first).ok_or(MergeRefusal::NoLayer)?;
                let b = siblings.iter().position(|h| *h == last).ok_or(MergeRefusal::NoLayer)?;
                let roots = &siblings[a.min(b)..=a.max(b)];
                if roots.iter().any(|h| scene.occurrence(*h).is_some_and(|o| !o.visible)) { return Err(MergeRefusal::Hidden); }
                m.members = self.checked(self.layer_subtrees(roots))?;
                if scene.order().iter().any(|h| scene.clipping_base(*h).into_iter().chain(scene.effect_owner(*h))
                    .any(|owner| m.members.contains(h) != m.members.contains(&owner))) { return Err(MergeRefusal::BelowClipped); }
                m.anchor = roots.last().copied();
                m.group = roots.len() == 1 && scene.occurrence(first).is_some_and(|o| o.kind() == LayerKind::Group);
            }
            MergeKind::Group => {
                let h = self.working.occurrence.ok_or(MergeRefusal::NoLayer)?;
                let o = scene.occurrence(h).ok_or(MergeRefusal::NoLayer)?;
                if o.kind() != LayerKind::Group {
                    return Err(MergeRefusal::NotGroup);
                }
                if !o.visible {
                    return Err(MergeRefusal::Hidden);
                }
                m.members = self.checked(self.layer_subtrees(&[h]))?;
                m.anchor = Some(h);
                m.group = true;
            }
            MergeKind::Down => {
                let h = self.working.occurrence.ok_or(MergeRefusal::NoLayer)?;
                let o = scene.occurrence(h).ok_or(MergeRefusal::NoLayer)?;
                if o.kind() == LayerKind::Selection { return Err(MergeRefusal::SelectionLayer); }
                if !o.visible {
                    return Err(MergeRefusal::Hidden);
                }
                if o.blend != LayerBlend::Normal {
                    return Err(MergeRefusal::NotNormal);
                }
                if self.is_locked(h) {
                    return Err(MergeRefusal::Locked);
                }
                if self.merge_down() == MergeDown::ClippingStack {
                    let owner=scene.effect_owner(h).unwrap_or(h);
                    let base=scene.clipping_base(owner).unwrap_or(owner);
                    let b = scene.occurrence(base).ok_or(MergeRefusal::NoLayer)?;
                    if !b.visible {
                        return Err(MergeRefusal::BaseHidden);
                    }
                    if b.blend != LayerBlend::Normal {
                        return Err(MergeRefusal::BaseNotNormal);
                    }
                    let mut stack = vec![base];
                    stack.extend(self.clips_above(base).into_iter().filter(|h| scene.occurrence(*h).is_some_and(|o| o.visible)));
                    if stack.len() == 1 {
                        return Err(MergeRefusal::ClipsHidden);
                    }
                    m.members = self.checked(self.layer_subtrees(&stack))?;
                    m.anchor = Some(base);
                } else {
                    let below=scene.effect_owner(h).or_else(||self.sibling_below(h)).ok_or(MergeRefusal::NoLayerBelow)?;
                    let b = scene.occurrence(below).unwrap();
                    if adjustment(scene, below) {
                        return Err(MergeRefusal::BelowEffect);
                    }
                    if b.attachment.is_clip() && !o.attachment.is_clip() {
                        return Err(MergeRefusal::BelowClipped);
                    }
                    if !b.visible {
                        return Err(MergeRefusal::BelowHidden);
                    }
                    if b.blend != LayerBlend::Normal {
                        return Err(MergeRefusal::BelowNotNormal);
                    }
                    if self.layer_subtrees(&[below]).iter().any(|h| self.is_locked(*h)) {
                        return Err(MergeRefusal::BelowLocked);
                    }
                    m.members = self.checked(self.layer_subtrees(&[h, below]))?;
                    m.anchor = Some(below);
                }
            }
            MergeKind::Visible | MergeKind::Flatten | MergeKind::Stamp => {
                let paper = scene.children(None).last().copied().filter(|h| kind != MergeKind::Stamp
                    && scene.effect_application(*h).is_some_and(|application| crate::paper::is_paper(&application.program)));
                let roots: Vec<_> = scene
                    .children(None)
                    .iter()
                    .copied()
                    .filter(|h| Some(*h) != paper && scene.occurrence(*h).is_some_and(|o| o.is_artwork()))
                    .collect();
                let visible: Vec<_> = roots
                    .iter()
                    .copied()
                    .filter(|h| {
                        scene.occurrence(*h).is_some_and(|o| {
                            o.visible && (!o.attachment.is_clip() || self.clipping_base(*h).and_then(|b| scene.occurrence(b)).is_some_and(|b| b.visible))
                        })
                    })
                    .collect();
                let anchor = *visible.last().ok_or(MergeRefusal::NothingVisible)?;
                m.members = self.layer_subtrees(&visible);
                if kind != MergeKind::Stamp {
                    m.members = self.checked(m.members)?;
                    m.anchor = Some(anchor);
                    if kind == MergeKind::Flatten {
                        let hidden: Vec<_> = roots.iter().copied().filter(|h| !m.members.contains(h)).collect();
                        m.discarded = self.checked(self.layer_subtrees(&hidden))?;
                    } else {
                        m.released = roots
                            .iter()
                            .copied()
                            .filter(|h| {
                                scene.occurrence(*h).is_some_and(|o| !o.visible && o.attachment.is_clip())
                                    && self.clipping_base(*h).is_some_and(|b| m.members.contains(&b))
                            })
                            .collect();
                    }
                }
            }
        }
        Ok(m)
    }
    /// The whole-page window, in document pixels, that holds `members`
    /// together with the composition frame.
    pub fn bake_extent(&self, members: &BTreeSet<OccurrenceHandle>) -> Result<(Point, [u32; 2]), MergeRefusal> {
        let scope = SceneScope::Members(self.scene().order().iter().copied().filter(|h| members.contains(h)).collect::<Vec<_>>().into());
        self.bake_window(self.scene().with_scope(&scope))
    }
    /// The whole pages that hold what `scene` draws, extended over the canvas
    /// when that still fits so the result can be painted across it.
    pub fn bake_window(&self, scene: SceneView<'_>) -> Result<(Point, [u32; 2]), MergeRefusal> {
        let support = output_support(scene, Reach::Domain)?;
        let window = |support: SupportBounds| {
            let [min, max] = support.grid()?;
            let size = i64::from(raster::TILE_SIZE);
            let origin = min.map(|v| v.div_euclid(size) * size);
            let extent: [u32; 2] = std::array::from_fn(|axis| u32::try_from(max[axis] - origin[axis]).unwrap_or(u32::MAX));
            (extent.iter().all(|v| *v <= MAX_EXTENT) && offsets::admitted(origin)).then(|| (offsets::point(origin), extent))
        };
        window(support.union(SupportBounds::extent([0.; 2], self.composition().size))).or_else(|| window(support)).ok_or(MergeRefusal::TooLarge)
    }
    pub(crate) fn exceeds_publication(&self, operation: &RasterOperation, extent: [u32; 2]) -> bool {
        raster::RasterPlane::Color.descriptor(self.composition().color).byte_len([raster::TILE_SIZE; 2]).is_none_or(|page| {
            raster::page_count(operation.bounds(extent), extent).saturating_mul(page as u64) > raster::MAX_PUBLICATION_BYTES
        })
    }
    pub fn merge_plan(&self, kind: MergeKind) -> Result<MergePlan, MergeRefusal> {
        let m = self.merge(kind)?;
        let scene = self.scene();
        let (origin, extent) = self.bake_extent(&m.members)?;
        let parent = m.anchor.and_then(|h| scene.parent(h));
        let parent_origin = self.scene().layer_origin(parent);
        let mut snapshot = self.snapshot();
        if m.group {
            let h = m.anchor.unwrap();
            let mut group = snapshot.artwork.occurrences.get(h).unwrap().clone();
            group.opacity = 1.;
            group.blend = LayerBlend::Normal;
            if let Some(occurrence) = Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(h) { *occurrence = group; }
        }
        let scope = SceneScope::Members(scene.order().iter().copied().filter(|h| m.members.contains(h)).collect::<Vec<_>>().into());
        let operation = RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(self.artwork.coverage.next_handle(), extent, [0; 2]),
            kind: RasterOperationKind::Bake { scene: snapshot, scope, offset: Point { x: -origin.x, y: -origin.y } },
        };
        if self.exceeds_publication(&operation, extent) {
            return Err(MergeRefusal::TooLarge);
        }
        let paint = RecordChange::insert(
            &self.artwork.paint,
            PaintSource { color_mode:Default::default(), domain: extent, raster: Default::default(), base: None, operations: Arc::default() },
        );
        let target = SourceTarget::Paint(paint.handle);
        let mut result = Occurrence::new(
            OccurrenceContent::Paint(paint.handle),
            m.anchor.and_then(|h| scene.occurrence(h)).map_or_else(|| Arc::from("Visible"), |o| o.name.clone()),
        );
        result.offset = offsets::checked_sub(offsets::exact(origin).ok_or(MergeRefusal::TooLarge)?, parent_origin).ok_or(MergeRefusal::TooLarge)?;
        if let Some(h) = m.anchor {
            let anchor = scene.occurrence(h).unwrap();
            result.attachment = anchor.attachment;
            result.alpha_locked = kind == MergeKind::Down && anchor.kind() == LayerKind::Paint && anchor.alpha_locked;
            if m.group {
                result.opacity = anchor.opacity;
                result.blend = if anchor.blend == LayerBlend::PassThrough { LayerBlend::Normal } else { anchor.blend };
            }
        }
        let removed: BTreeSet<_> = if m.anchor.is_some() { m.members.union(&m.discarded).copied().collect() } else { Default::default() };
        result.reference = removed.iter().any(|h| scene.occurrence(*h).is_some_and(|o| o.reference));
        let occurrence = RecordChange::insert(&self.artwork.occurrences, result);
        let result = occurrence.handle;
        let containing = m.anchor.and_then(|h| scene.stack(h)).unwrap_or(self.composition().result);
        let mut edits = vec![Edit::Paint(paint), Edit::Occurrence(occurrence)];
        for (h, _, before) in self.artwork.stacks.iter() {
            let mut stack = before.clone();
            let index = if h == containing { m.anchor.and_then(|a| stack.entries.iter().position(|h| *h == a)).unwrap_or(0) } else { 0 };
            let retained_before = stack.entries[..index].iter().filter(|h| !removed.contains(h)).count();
            stack.entries.retain(|h| !removed.contains(h));
            if h == containing {
                stack.entries.insert(retained_before, result);
            }
            if stack != *before {
                edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, h, Some(stack)).map_err(|_| MergeRefusal::NoLayer)?));
            }
        }
        edits.extend(self.removal_edits(&removed).map_err(|_| MergeRefusal::NoLayer)?);
        for h in m.released {
            let mut o = scene.occurrence(h).unwrap().clone();
            o.attachment = crate::Attachment::None;
            edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, h, Some(o)).map_err(|_| MergeRefusal::NoLayer)?));
        }
        let mut working = self.working.clone();
        working.occurrence = Some(result);
        working.layer_selection = [result].into();
        working.layer_anchor = Some(result);
        working.target = Some(target);
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        Ok(MergePlan { edits, result, target, operation })
    }
}
#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
