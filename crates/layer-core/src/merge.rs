//! Merge Down, Merge Group, Merge Visible, Flatten Image and Stamp Visible.
//! Each composites its members, isolated, into one new paint layer through a
//! pending `Bake` operation, and replaces them with it in one undo step.
use super::*;

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

/// Edits that insert `result` with its pending `operation` and remove the
/// merged layers. `CanvasEngine::insert_with_operations` runs them as one step.
#[derive(Clone, Debug)]
pub struct MergePlan {
    pub edits: Vec<Edit>,
    pub result: LayerId,
    pub operation: LayerOperation,
}

struct Merge<'a> {
    /// Composited in document order, with their subtrees.
    members: BTreeSet<LayerId>,
    /// Removed without being composited: layers Flatten Image discards.
    discarded: BTreeSet<LayerId>,
    /// Hidden layers clipped to a merged base stay, released.
    released: Vec<LayerId>,
    /// The result takes this layer's place, name and clipping; without one it
    /// goes on top of the image and keeps every member.
    anchor: Option<&'a Layer>,
    /// A merged group keeps its blend mode and opacity instead of baking them;
    /// a Pass Through group becomes Normal.
    group: bool,
    /// The result covers the canvas only, dropping pixels outside it.
    canvas: bool,
}

fn adjustment(layer: &Layer) -> bool {
    layer.effect.as_ref().is_some_and(|e| e.program.kind == EffectKind::Adjustment)
}

/// The members of a `Bake` as root layers of the result's pixels. Members
/// whose parent is not a member move by `offset`; clipped layers left without
/// their base are released, since the result takes the base's place.
pub fn bake_layers(members: &[Layer], offset: Point) -> Vec<Layer> {
    let mut layers = members.to_vec();
    let mut based = false;
    for layer in layers.iter_mut().rev() {
        if layer.properties.parent.is_some_and(|p| members.iter().any(|m| m.id == p)) {
            continue;
        }
        layer.properties.parent = None;
        layer.properties.offset.x += offset.x;
        layer.properties.offset.y += offset.y;
        if let Some(mask) = &mut layer.mask {
            mask.offset.x += offset.x;
            mask.offset.y += offset.y;
        }
        if !layer.properties.clipped {
            based = true;
        } else if !based {
            layer.properties.clipped = false;
        }
    }
    layers
}

/// `layer` as a member of a `Bake`: as committed, without pending operations.
pub(crate) fn bake_member(layer: &Layer) -> Layer {
    let mut member = layer.clone();
    member.pending_operations.clear();
    if let Some(mask) = &mut member.mask {
        mask.pending_operations = Arc::default();
    }
    member
}

/// Local pixels a paint layer may hold: its tiles and its photo.
fn content(layer: &Layer, extent: [u32; 2]) -> Rect {
    let Some(Ok(data)) = layer.raster.try_data() else {
        return Rect::from_extent(layer.local_extent(extent));
    };
    let size = raster::TILE_SIZE as f32;
    let tiles = data.tiles.keys().fold(Rect::EMPTY, |bounds, key| {
        let [x, y] = key.coordinate.map(|v| v as f32 * size);
        bounds.union(Rect { min: Point { x, y }, max: Point { x: x + size, y: y + size } })
    });
    layer.source.as_ref().map_or(tiles, |source| tiles.union(Rect::from_extent(source.extent)))
}

/// Pixels of a result `extent` large that baking `members` can cover.
pub(crate) fn bake_bounds(members: &[Layer], offset: Point, extent: [u32; 2]) -> Rect {
    let layers = bake_layers(members, offset);
    let visible = |layer: &Layer| {
        let mut current = Some(layer);
        while let Some(layer) = current {
            if !layer.visible {
                return false;
            }
            current = layer.properties.parent.and_then(|p| layers.iter().find(|l| l.id == p));
        }
        true
    };
    let mut bounds = Rect::EMPTY;
    for layer in layers.iter().filter(|l| l.kind == LayerKind::Paint && visible(l)) {
        bounds = bounds.union(target_geometry(&layers, layer.id).forward_bounds(content(layer, extent)));
    }
    for effect in layers.iter().filter(|l| visible(l)).filter_map(|l| l.effect.as_ref()) {
        match (effect.program.kind, effect.program.alpha) {
            (EffectKind::Generator, _) => return Rect::from_extent(extent),
            (EffectKind::Adjustment, EffectAlpha::Filter) => match effect.damage_radius() {
                Some(radius) => bounds = bounds.outset(radius as f32),
                None => return Rect::from_extent(extent),
            },
            (EffectKind::Adjustment, EffectAlpha::Preserve) => {}
        }
    }
    if bounds.is_empty() {
        return bounds;
    }
    Rect {
        min: Point { x: bounds.min.x.max(0.), y: bounds.min.y.max(0.) },
        max: Point {
            x: bounds.max.x.min(extent[0] as f32),
            y: bounds.max.y.min(extent[1] as f32),
        },
    }
}

impl Document {
    fn sibling_below(&self, index: usize) -> Option<&Layer> {
        let parent = self.layers[index].properties.parent;
        self.layers[index + 1..]
            .iter()
            .find(|l| l.is_artwork() && l.properties.parent == parent)
    }

    fn clips_above(&self, index: usize) -> impl Iterator<Item = &Layer> {
        let parent = self.layers[index].properties.parent;
        self.layers[..index]
            .iter()
            .rev()
            .filter(move |l| l.is_artwork() && l.properties.parent == parent)
            .take_while(|l| l.properties.clipped)
    }

    /// What Merge Down does for the active layer, for its label.
    pub fn merge_down(&self) -> MergeDown {
        let Some(index) = self.layers.iter().position(|l| l.id == self.active_layer) else {
            return MergeDown::Layer;
        };
        let upper = &self.layers[index];
        match (upper.properties.clipped, adjustment(upper)) {
            (true, true) => MergeDown::ClippingStack,
            (true, false) => MergeDown::Layer,
            (false, _) if self.clips_above(index).next().is_some() => MergeDown::ClippingStack,
            (false, true) => MergeDown::ApplyEffect,
            (false, false) => MergeDown::Layer,
        }
    }

    /// Why a merge can't run, without planning it. The size limit is checked
    /// by `merge_plan` only.
    pub fn merge_refusal(&self, kind: MergeKind) -> Option<MergeRefusal> {
        self.merge(kind).err()
    }

    /// Hidden layers Flatten Image discards, counting a hidden group once.
    pub fn flatten_discards(&self) -> usize {
        self.merge(MergeKind::Flatten).map_or(0, |merge| {
            let hidden = |id: &LayerId| {
                self.layer(*id).is_some_and(|l| {
                    !l.visible && l.properties.parent.is_none_or(|p| self.layer(p).is_some_and(|p| p.visible))
                })
            };
            merge.discarded.iter().chain(&merge.members).filter(|id| hidden(id)).count()
                + self
                    .layers
                    .iter()
                    .filter(|l| l.visible && l.properties.parent.is_none() && merge.discarded.contains(&l.id))
                    .count()
        })
    }

    fn checked(&self, members: BTreeSet<LayerId>) -> Result<BTreeSet<LayerId>, MergeRefusal> {
        for &id in &members {
            if self.is_locked(id) {
                return Err(MergeRefusal::Locked);
            }
            if self.layer(id).is_some_and(|l| l.kind == LayerKind::Selection) {
                return Err(MergeRefusal::SelectionLayersInside);
            }
        }
        Ok(members)
    }

    fn merge(&self, kind: MergeKind) -> Result<Merge<'_>, MergeRefusal> {
        let merge = Merge {
            members: BTreeSet::new(),
            discarded: BTreeSet::new(),
            released: Vec::new(),
            anchor: None,
            group: false,
            canvas: false,
        };
        match kind {
            MergeKind::Down => self.merge_down_members(merge),
            MergeKind::Group => {
                let group = self.layer(self.active_layer).ok_or(MergeRefusal::NoLayer)?;
                if group.kind != LayerKind::Group {
                    return Err(MergeRefusal::NotGroup);
                }
                if !group.visible {
                    return Err(MergeRefusal::Hidden);
                }
                let members = self.checked(self.layer_subtrees(&[group.id]))?;
                Ok(Merge { members, anchor: Some(group), group: true, ..merge })
            }
            MergeKind::Visible | MergeKind::Flatten | MergeKind::Stamp => {
                let visible_base = |layer: &Layer| {
                    self.clipping_base(layer.id)
                        .and_then(|base| self.layer(base))
                        .is_some_and(|base| base.visible)
                };
                let roots: Vec<_> = self
                    .layers
                    .iter()
                    .filter(|l| {
                        l.properties.parent.is_none() && l.is_artwork()
                    })
                    .collect();
                let visible: Vec<_> = roots
                    .iter()
                    .copied()
                    .filter(|l| l.visible && (!l.properties.clipped || visible_base(l)))
                    .collect();
                let anchor = *visible.last().ok_or(MergeRefusal::NothingVisible)?;
                let members = self.layer_subtrees(&visible.iter().map(|l| l.id).collect::<Vec<_>>());
                if kind == MergeKind::Stamp {
                    return Ok(Merge { members, canvas: true, ..merge });
                }
                let members = self.checked(members)?;
                if kind == MergeKind::Flatten {
                    let hidden: Vec<_> = roots.iter().filter(|l| !members.contains(&l.id)).map(|l| l.id).collect();
                    let discarded = self.checked(self.layer_subtrees(&hidden))?;
                    return Ok(Merge { members, discarded, anchor: Some(anchor), canvas: true, ..merge });
                }
                let released = roots
                    .iter()
                    .filter(|l| {
                        !l.visible
                            && l.properties.clipped
                            && self.clipping_base(l.id).is_some_and(|base| members.contains(&base))
                    })
                    .map(|l| l.id)
                    .collect();
                Ok(Merge { members, released, anchor: Some(anchor), ..merge })
            }
        }
    }

    fn merge_down_members<'a>(&'a self, merge: Merge<'a>) -> Result<Merge<'a>, MergeRefusal> {
        let index = self
            .layers
            .iter()
            .position(|l| l.id == self.active_layer)
            .ok_or(MergeRefusal::NoLayer)?;
        let upper = &self.layers[index];
        if upper.kind == LayerKind::Selection { return Err(MergeRefusal::SelectionLayer); }
        if !upper.visible {
            return Err(MergeRefusal::Hidden);
        }
        if upper.properties.blend != LayerBlend::Normal {
            return Err(MergeRefusal::NotNormal);
        }
        if self.is_locked(upper.id) {
            return Err(MergeRefusal::Locked);
        }
        if self.merge_down() == MergeDown::ClippingStack {
            let base = if upper.properties.clipped {
                self.clipping_base(upper.id).ok_or(MergeRefusal::NoLayerBelow)?
            } else {
                upper.id
            };
            let base_index = self.layers.iter().position(|l| l.id == base).ok_or(MergeRefusal::NoLayer)?;
            let base = &self.layers[base_index];
            if !base.visible {
                return Err(MergeRefusal::BaseHidden);
            }
            if base.properties.blend != LayerBlend::Normal {
                return Err(MergeRefusal::BaseNotNormal);
            }
            let mut stack = vec![base.id];
            stack.extend(self.clips_above(base_index).filter(|l| l.visible).map(|l| l.id));
            if stack.len() == 1 {
                return Err(MergeRefusal::ClipsHidden);
            }
            let members = self.checked(self.layer_subtrees(&stack))?;
            return Ok(Merge { members, anchor: Some(base), ..merge });
        }
        let below = self.sibling_below(index).ok_or(MergeRefusal::NoLayerBelow)?;
        if adjustment(below) {
            return Err(MergeRefusal::BelowEffect);
        }
        if below.properties.clipped && !upper.properties.clipped {
            return Err(MergeRefusal::BelowClipped);
        }
        if !below.visible {
            return Err(MergeRefusal::BelowHidden);
        }
        if below.properties.blend != LayerBlend::Normal {
            return Err(MergeRefusal::BelowNotNormal);
        }
        let lower = self.layer_subtrees(&[below.id]);
        if lower.iter().any(|&id| self.is_locked(id)) {
            return Err(MergeRefusal::BelowLocked);
        }
        let members = self.checked(self.layer_subtrees(&[upper.id, below.id]))?;
        Ok(Merge { members, anchor: Some(below), ..merge })
    }

    /// Where the members' pixels lie in document coordinates, with the
    /// canvas, grown to whole pages from the canvas origin.
    pub fn bake_extent(&self, members: &BTreeSet<LayerId>) -> Result<(Point, [u32; 2]), MergeRefusal> {
        let canvas = [self.width, self.height];
        let bounds = self
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Paint && members.contains(&l.id))
            .fold(Rect::from_extent(canvas), |bounds, l| {
                bounds.union(self.layer_geometry(l.id).forward_bounds(Rect::from_extent(l.local_extent(canvas))))
            });
        let size = raster::TILE_SIZE as f32;
        let origin = Point {
            x: (bounds.min.x / size).floor() * size,
            y: (bounds.min.y / size).floor() * size,
        };
        let span = |max: f32, min: f32| (max - min).ceil();
        let extent = [span(bounds.max.x, origin.x), span(bounds.max.y, origin.y)];
        if !extent.iter().all(|v| v.is_finite() && *v <= MAX_EXTENT as f32) {
            return Err(MergeRefusal::TooLarge);
        }
        Ok((origin, extent.map(|v| v as u32)))
    }

    /// Whether the pages `operation` writes into a paint layer `extent` large
    /// exceed the publication limit of one edit.
    pub(crate) fn exceeds_publication(&self, operation: &LayerOperation, extent: [u32; 2]) -> bool {
        raster::RasterPlane::Color
            .descriptor(self.color)
            .byte_len([raster::TILE_SIZE; 2])
            .is_none_or(|page| {
                raster::page_count(operation.bounds(extent), extent).saturating_mul(page as u64)
                    > raster::MAX_PUBLICATION_BYTES
            })
    }

    /// Plan a merge into a new layer `result` whose pending bake uses the
    /// coverage identity `coverage`. Placed photos become document pixels.
    pub fn merge_plan(&self, kind: MergeKind, result: LayerId, coverage: LayerId) -> Result<MergePlan, MergeRefusal> {
        let merge = self.merge(kind)?;
        let canvas = [self.width, self.height];
        let members: Vec<Layer> = self
            .layers
            .iter()
            .filter(|l| merge.members.contains(&l.id))
            .map(|l| {
                let mut layer = bake_member(l);
                if merge.group && Some(l.id) == merge.anchor.map(|a| a.id) {
                    layer.opacity = 1.;
                    layer.properties.blend = LayerBlend::Normal;
                }
                layer
            })
            .collect();
        let (origin, extent) = if merge.canvas || members.iter().any(|l| l.kind == LayerKind::Effect) {
            (Point::default(), canvas)
        } else {
            self.bake_extent(&merge.members)?
        };
        let parent = merge.anchor.and_then(|a| a.properties.parent);
        let parent_offset = parent.map_or(Point::default(), |p| self.layer_offset(p));
        let offset = Point { x: parent_offset.x - origin.x, y: parent_offset.y - origin.y };
        let operation = LayerOperation {
            placement: Affine::IDENTITY,
            coverage: LayerMask::reveal_all(coverage, Point::default()),
            kind: LayerOperationKind::Bake { members: members.into(), offset },
        };
        if self.exceeds_publication(&operation, extent) {
            return Err(MergeRefusal::TooLarge);
        }
        let mut layer = Layer::paint(result, merge.anchor.map_or_else(|| "Visible".into(), |a| a.name.clone()));
        layer.properties.parent = parent;
        layer.properties.offset = Point { x: origin.x - parent_offset.x, y: origin.y - parent_offset.y };
        layer.properties.extent = (extent != canvas).then_some(extent);
        if let Some(anchor) = merge.anchor {
            layer.properties.clipped = anchor.properties.clipped;
            layer.properties.alpha_locked =
                kind == MergeKind::Down && anchor.kind == LayerKind::Paint && anchor.properties.alpha_locked;
            if merge.group {
                layer.opacity = anchor.opacity;
                layer.properties.blend = match anchor.properties.blend {
                    LayerBlend::PassThrough => LayerBlend::Normal,
                    blend => blend,
                };
            }
        }
        let removed: BTreeSet<_> = if merge.anchor.is_some() {
            merge.members.union(&merge.discarded).copied().collect()
        } else {
            BTreeSet::new()
        };
        let index = merge.anchor.map_or(0, |anchor| {
            self.layers.iter().position(|l| l.id == anchor.id).unwrap_or(0)
        });
        let mut edits = vec![Edit::InsertLayer { index, layer: Box::new(layer) }, Edit::SetActiveLayer { id: result }];
        edits.extend(
            self.ordered_layers()
                .into_iter()
                .rev()
                .filter(|l| removed.contains(&l.id))
                .map(|l| Edit::RemoveLayer { id: l.id }),
        );
        for id in &merge.released {
            let mut layer = self.layer(*id).ok_or(MergeRefusal::NoLayer)?.clone();
            layer.properties.clipped = false;
            edits.push(Edit::ReplaceLayer(Box::new(layer)));
        }
        if self.reference_layers.iter().any(|id| removed.contains(id)) {
            let mut references: BTreeSet<_> = self.reference_layers.difference(&removed).copied().collect();
            references.insert(result);
            edits.push(Edit::SetReferences(references));
        }
        Ok(MergePlan { edits, result, operation })
    }
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod tests;
