use super::*;
use crate::authored::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    Down,
    Group,
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
    canvas: bool,
}
fn adjustment(scene: SceneView<'_>, h: OccurrenceHandle) -> bool {
    scene.effect(h).is_some_and(|e| e.program.kind == EffectKind::Adjustment)
}
fn content(scene: SceneView<'_>, h: OccurrenceHandle) -> Rect {
    let Some(paint) = scene.paint_source(h) else {
        return Rect::EMPTY;
    };
    let Some(Ok(data)) = paint.raster.try_data() else {
        return Rect::from_extent(paint.domain);
    };
    let size = raster::TILE_SIZE as f32;
    let bounds = data.tiles.keys().fold(Rect::EMPTY, |b, k| {
        let [x, y] = k.coordinate.map(|v| v as f32 * size);
        b.union(Rect { min: Point { x, y }, max: Point { x: x + size, y: y + size } })
    });
    paint.original.as_ref().map_or(bounds, |s| bounds.union(Rect::from_extent(s.extent)))
}
pub(crate) fn bake_bounds(snapshot: &SceneSnapshot, scope: &SceneScope, offset: Point, extent: [u32; 2]) -> Rect {
    let scene = snapshot.view().with_scope(scope).with_offset(offset);
    let mut bounds = Rect::EMPTY;
    for h in scene.order().iter().copied().filter(|h| layer_is_visible(scene, *h)) {
        if let Some(target) = scene.source_target(h).filter(|t| matches!(t, SourceTarget::Paint(_))) {
            let transform = scene.target_geometry(target);
            let mapped = transform.forward_bounds(content(scene, h));
            bounds = bounds.union(mapped);
        }
    }
    for effect in scene.order().iter().copied().filter(|h| layer_is_visible(scene, *h)).filter_map(|h| scene.effect(h)) {
        match (effect.program.kind, effect.program.alpha) {
            (EffectKind::Generator, _) => return Rect::from_extent(extent),
            (EffectKind::Adjustment, EffectAlpha::Filter) => match effect.damage_radius() {
                Some(r) => bounds = bounds.outset(r as f32),
                None => return Rect::from_extent(extent),
            },
            _ => {}
        }
    }
    if bounds.is_empty() { bounds } else { bounds.intersect(Rect::from_extent(extent)) }
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
            canvas: false,
        };
        match kind {
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
                let roots: Vec<_> = scene
                    .children(None)
                    .iter()
                    .copied()
                    .filter(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork()))
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
                if kind == MergeKind::Stamp {
                    m.canvas = true;
                } else {
                    m.members = self.checked(m.members)?;
                    m.anchor = Some(anchor);
                    if kind == MergeKind::Flatten {
                        let hidden: Vec<_> = roots.iter().copied().filter(|h| !m.members.contains(h)).collect();
                        m.discarded = self.checked(self.layer_subtrees(&hidden))?;
                        m.canvas = true;
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
    pub fn bake_extent(&self, members: &BTreeSet<OccurrenceHandle>) -> Result<(Point, [u32; 2]), MergeRefusal> {
        let scene = self.scene();
        let bounds =
            scene.order().iter().copied().filter(|h| members.contains(h) && scene.paint_source(*h).is_some()).fold(
                Rect::from_extent(self.composition().size),
                |b, h| {
                    b.union(scene.target_geometry(scene.source_target(h).unwrap()).forward_bounds(Rect::from_extent(scene.local_extent(h))))
                },
            );
        let size = raster::TILE_SIZE as f32;
        let origin = Point { x: (bounds.min.x / size).floor() * size, y: (bounds.min.y / size).floor() * size };
        let extent = [(bounds.max.x - origin.x).ceil(), (bounds.max.y - origin.y).ceil()];
        if !extent.iter().all(|v| v.is_finite() && *v <= MAX_EXTENT as f32) {
            return Err(MergeRefusal::TooLarge);
        }
        Ok((origin, extent.map(|v| v as u32)))
    }
    pub(crate) fn exceeds_publication(&self, operation: &RasterOperation, extent: [u32; 2]) -> bool {
        raster::RasterPlane::Color.descriptor(self.composition().color).byte_len([raster::TILE_SIZE; 2]).is_none_or(|page| {
            raster::page_count(operation.bounds(extent), extent).saturating_mul(page as u64) > raster::MAX_PUBLICATION_BYTES
        })
    }
    pub fn merge_plan(&self, kind: MergeKind) -> Result<MergePlan, MergeRefusal> {
        let m = self.merge(kind)?;
        let scene = self.scene();
        let canvas = self.composition().size;
        let (origin, extent) = if m.canvas || m.members.iter().any(|h| scene.effect(*h).is_some()) {
            (Point::default(), canvas)
        } else {
            self.bake_extent(&m.members)?
        };
        let parent = m.anchor.and_then(|h| scene.parent(h));
        let parent_offset = parent.map_or(Point::default(), |p| self.layer_offset(p));
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
            coverage: CoverageSnapshot::reveal_all(self.artwork.coverage.next_handle(), extent, Point::default()),
            kind: RasterOperationKind::Bake { scene: snapshot, scope, offset: Point { x: -origin.x, y: -origin.y } },
        };
        if self.exceeds_publication(&operation, extent) {
            return Err(MergeRefusal::TooLarge);
        }
        let paint = RecordChange::insert(
            &self.artwork.paint,
            PaintSource { color_mode: Default::default(), domain: extent, raster: Default::default(), original: None, operations: Arc::default() },
        );
        let target = SourceTarget::Paint(paint.handle);
        let mut result = Occurrence::new(
            OccurrenceContent::Paint(paint.handle),
            m.anchor.and_then(|h| scene.occurrence(h)).map_or_else(|| Arc::from("Visible"), |o| o.name.clone()),
        );
        result.translation = Point { x: origin.x - parent_offset.x, y: origin.y - parent_offset.y };
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
