//! Layer ownership and coverage, independent of UI widgets and GPU storage.
use super::*;

#[cfg(test)]
mod organization_tests {
    use super::*;
    #[test]
    fn grouping_and_ungrouping_preserve_order_and_world_coordinates() {
        let mut doc = Document::new("groups", 100, 100);
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::paint(LayerId(3), "Texture"),
        })
        .unwrap();
        doc.layers[0].mask = Some(LayerMask::reveal_all(LayerId(99), Point { x: 7., y: 9. }));
        let roots = doc.layer_roots(&BTreeSet::from([LayerId(1), LayerId(3)]));
        doc.apply(doc.group_layers_edit(&roots, LayerId(10)).unwrap())
            .unwrap();
        assert_eq!(
            doc.layer_roots(&BTreeSet::from([LayerId(10), LayerId(3)])),
            vec![LayerId(10)]
        );
        let mut group = doc.layer(LayerId(10)).unwrap().clone();
        group.properties.offset = Point { x: 20., y: -10. };
        doc.apply(Edit::ReplaceLayer(Box::new(group))).unwrap();
        doc.apply(Edit::SetReferences(BTreeSet::from([LayerId(10)])))
            .unwrap();
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::paint(LayerId(4), "Outside"),
        })
        .unwrap();
        // Flat storage need not be contiguous or parent-first after reparenting.
        doc.apply(Edit::MoveLayer {
            id: LayerId(3),
            to: 0,
        })
        .unwrap();
        let offset = doc.layer_offset(LayerId(3));
        let mask_offset = doc.layer_offset(LayerId(99));
        let before = doc.clone();
        let order: Vec<_> = doc
            .ordered_layers()
            .iter()
            .map(|l| l.id)
            .filter(|id| *id != LayerId(10))
            .collect();
        let undo = doc
            .apply(doc.ungroup_layer_edit(LayerId(10)).unwrap())
            .unwrap();
        assert_eq!(
            doc.ordered_layers()
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            order
        );
        assert_eq!(doc.layer_offset(LayerId(3)), offset);
        assert_eq!(doc.layer_offset(LayerId(99)), mask_offset);
        assert_eq!(
            doc.reference_layers,
            BTreeSet::from([LayerId(1), LayerId(3)])
        );
        doc.apply(undo).unwrap();
        assert_eq!(doc.layers, before.layers);
        assert_eq!(doc.reference_layers, before.reference_layers);
        let mut group = doc.layer(LayerId(10)).unwrap().clone();
        group.opacity = 0.5;
        doc.apply(Edit::ReplaceLayer(Box::new(group))).unwrap();
        assert!(doc.ungroup_layer_edit(LayerId(10)).is_err());
    }
    #[test]
    fn bulk_edits_protect_clipping_stacks_locks_and_last_paint_layer() {
        let mut doc = Document::new("clipping", 100, 100);
        let mut clip = Layer::paint(LayerId(3), "Shade");
        clip.properties.clipped = true;
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: clip,
        })
        .unwrap();
        assert!(doc.delete_layers_edit(&[LayerId(1)]).is_err());
        assert!(doc.group_layers_edit(&[LayerId(1)], LayerId(10)).is_err());
        assert!(doc.delete_layers_edit(&[LayerId(1), LayerId(3)]).is_err());
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::paint(LayerId(4), "Other"),
        })
        .unwrap();
        let before = doc.layers.clone();
        let undo = doc
            .apply(doc.delete_layers_edit(&[LayerId(1), LayerId(3)]).unwrap())
            .unwrap();
        assert!(doc.layer(LayerId(1)).is_none());
        doc.apply(undo).unwrap();
        assert_eq!(doc.layers, before);
        doc.layers
            .iter_mut()
            .find(|l| l.id == LayerId(3))
            .unwrap()
            .properties
            .locked = true;
        assert!(doc.delete_layers_edit(&[LayerId(1), LayerId(3)]).is_err());
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum LayerBlend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Add,
    Overlay,
    SoftLight,
    Color,
}
impl LayerBlend {
    pub const ALL: [Self; 7] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Add,
        Self::Overlay,
        Self::SoftLight,
        Self::Color,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Add => "Add",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::Color => "Color",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerProperties {
    pub parent: Option<LayerId>,
    pub offset: Point,
    pub alpha_locked: bool,
    pub locked: bool,
    pub clipped: bool,
    pub blend: LayerBlend,
}

/// Polygon selection: even/odd interiors support holes and disjoint islands.
/// Coordinates remain geometry; rasterization and antialiasing are GPU work.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub contours: Arc<[Arc<[Point]>]>,
    pub inverted: bool,
}
impl Selection {
    pub fn polygon(points: Vec<Point>) -> Result<Self, DocumentError> {
        if points.len() < 3 || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(DocumentError::InvalidLayerOperation(
                "A selection needs a closed area",
            ));
        }
        Ok(Self {
            contours: vec![points.into()].into(),
            inverted: false,
        })
    }
    pub fn translated(&self, delta: Point) -> Self {
        Self {
            contours: self
                .contours
                .iter()
                .map(|path| {
                    path.iter()
                        .map(|p| Point {
                            x: p.x + delta.x,
                            y: p.y + delta.y,
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect::<Vec<_>>()
                .into(),
            inverted: self.inverted,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerMask {
    /// Unique image identity, allocated from the document layer-ID allocator.
    pub id: LayerId,
    pub enabled: bool,
    pub linked: bool,
    pub offset: Point,
    pub initial: Option<Selection>,
    pub default_coverage: f32,
    pub inverted: bool,
    pub strokes: Arc<Vec<StrokeId>>,
    /// Inspection only; never participates in exported color.
    pub show_area: bool,
}

/// Ordered raster mutations retain their source coverage for deterministic undo
/// and device-loss replay. They are not live composition masks after baking.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerOperation {
    pub after_stroke: usize,
    pub coverage: LayerMask,
    pub kind: LayerOperationKind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum LayerOperationKind {
    ApplyMask,
    Fill { color: [f32; 4], alpha_locked: bool },
}
impl LayerMask {
    pub fn reveal_all(id: LayerId, offset: Point) -> Self {
        Self {
            id,
            enabled: true,
            linked: true,
            offset,
            initial: None,
            default_coverage: 1.0,
            inverted: false,
            strokes: Arc::default(),
            show_area: false,
        }
    }
}

impl Document {
    /// Normalize selection so a selected parent owns its descendants once.
    pub fn layer_roots(&self, selected: &std::collections::BTreeSet<LayerId>) -> Vec<LayerId> {
        self.ordered_layers()
            .into_iter()
            .filter(|l| selected.contains(&l.id))
            .filter(|l| {
                let mut parent = l.properties.parent;
                while let Some(id) = parent {
                    if selected.contains(&id) {
                        return false;
                    }
                    parent = self.layer(id).and_then(|l| l.properties.parent);
                }
                true
            })
            .map(|l| l.id)
            .collect()
    }
    pub fn layer_subtrees(&self, roots: &[LayerId]) -> std::collections::BTreeSet<LayerId> {
        let mut members: std::collections::BTreeSet<_> = roots.iter().copied().collect();
        for l in self.ordered_layers() {
            if l.properties.parent.is_some_and(|id| members.contains(&id)) {
                members.insert(l.id);
            }
        }
        members
    }
    pub fn clipping_base(&self, id: LayerId) -> Option<LayerId> {
        let layer = self.layer(id)?;
        let i = self.layers.iter().position(|l| l.id == id)?;
        self.layers[i + 1..]
            .iter()
            .find(|l| l.properties.parent == layer.properties.parent && !l.properties.clipped)
            .filter(|l| matches!(l.kind, LayerKind::Paint | LayerKind::ImportedImage))
            .map(|l| l.id)
    }
    pub fn delete_layers_edit(&self, roots: &[LayerId]) -> Result<Edit, DocumentError> {
        if roots.is_empty() {
            return Err(DocumentError::InvalidLayerOperation("Select layers first"));
        }
        let ids = self.layer_subtrees(roots);
        for &id in &ids {
            let layer = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
            if self.is_locked(id) || layer.kind == LayerKind::Background {
                return Err(DocumentError::ProtectedLayer(id));
            }
        }
        if self.layers.iter().any(|l| {
            l.properties.clipped
                && !ids.contains(&l.id)
                && self
                    .clipping_base(l.id)
                    .is_some_and(|base| ids.contains(&base))
        }) {
            return Err(DocumentError::InvalidLayerOperation(
                "Include the clipped layers above this base",
            ));
        }
        let edit = Edit::Batch(
            self.ordered_layers()
                .into_iter()
                .rev()
                .filter(|l| ids.contains(&l.id))
                .map(|l| Edit::RemoveLayer { id: l.id })
                .collect(),
        );
        self.clone().apply(edit.clone())?;
        Ok(edit)
    }
    /// Group a contiguous sibling range; keep complete clipping relationships.
    pub fn group_layers_edit(
        &self,
        roots: &[LayerId],
        group_id: LayerId,
    ) -> Result<Edit, DocumentError> {
        let first = self
            .layer(
                *roots
                    .first()
                    .ok_or(DocumentError::InvalidLayerOperation("Select layers first"))?,
            )
            .ok_or(DocumentError::InvalidLayerOperation("Unknown layer"))?;
        let parent = first.properties.parent;
        let selected: std::collections::BTreeSet<_> = roots.iter().copied().collect();
        for &id in roots {
            let l = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
            if self.is_locked(id) || l.kind == LayerKind::Background {
                return Err(DocumentError::ProtectedLayer(id));
            }
            if l.properties.parent != parent {
                return Err(DocumentError::InvalidLayerOperation(
                    "Select layers in the same group",
                ));
            }
        }
        let siblings: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.properties.parent == parent)
            .collect();
        let positions: Vec<_> = siblings
            .iter()
            .enumerate()
            .filter(|(_, l)| selected.contains(&l.id))
            .map(|(i, _)| i)
            .collect();
        if positions.last().unwrap() - positions[0] + 1 != roots.len() {
            return Err(DocumentError::InvalidLayerOperation(
                "Select neighboring layers to group",
            ));
        }
        for l in &siblings {
            if l.properties.clipped
                && self
                    .clipping_base(l.id)
                    .is_none_or(|base| selected.contains(&l.id) != selected.contains(&base))
            {
                return Err(DocumentError::InvalidLayerOperation(
                    "Include the complete clipping stack",
                ));
            }
        }
        let top = siblings[positions[0]].id;
        let mut group = Layer::paint(group_id, "Group");
        group.kind = LayerKind::Group;
        group.properties.parent = parent;
        let mut edits = vec![Edit::InsertLayer {
            index: self.layers.iter().position(|l| l.id == top).unwrap(),
            layer: group,
        }];
        for &id in roots {
            let mut l = self.layer(id).unwrap().clone();
            l.properties.parent = Some(group_id);
            edits.push(Edit::ReplaceLayer(Box::new(l)));
        }
        Ok(Edit::Batch(edits))
    }
    /// Remove a neutral group without changing its isolated compositing result.
    pub fn ungroup_layer_edit(&self, id: LayerId) -> Result<Edit, DocumentError> {
        let group = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
        if self.is_locked(id) {
            return Err(DocumentError::ProtectedLayer(id));
        }
        if group.kind != LayerKind::Group
            || group.opacity != 1.
            || group.mask.is_some()
            || group.properties.blend != LayerBlend::Normal
            || group.properties.clipped
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Remove the group mask, blend and opacity effects before ungrouping",
            ));
        }
        let mut edits = Vec::new();
        let mut references = self.reference_layers.clone();
        let reference = references.remove(&id);
        let children: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.properties.parent == Some(id))
            .collect();
        // Place children immediately before the group, in order. Each move is
        // resolved against the preceding edit, not stale flat-storage indices.
        let mut probe = self.clone();
        for child in children {
            if self.is_locked(child.id) {
                return Err(DocumentError::ProtectedLayer(child.id));
            }
            if child.properties.blend != LayerBlend::Normal {
                return Err(DocumentError::InvalidLayerOperation(
                    "Set child layers to Normal before ungrouping",
                ));
            }
            let mut child = child.clone();
            child.properties.parent = group.properties.parent;
            child.properties.offset.x += group.properties.offset.x;
            child.properties.offset.y += group.properties.offset.y;
            if let Some(mask) = &mut child.mask {
                mask.offset.x += group.properties.offset.x;
                mask.offset.y += group.properties.offset.y;
            }
            child.visible &= group.visible;
            if reference {
                references.insert(child.id);
            }
            let from = probe.layers.iter().position(|l| l.id == child.id).unwrap();
            let to = probe.layers.iter().position(|l| l.id == id).unwrap();
            let move_edit = Edit::Batch(vec![
                Edit::ReplaceLayer(Box::new(child.clone())),
                Edit::MoveLayer {
                    id: child.id,
                    to: to.saturating_sub(usize::from(from < to)),
                },
            ]);
            probe.apply(move_edit.clone())?;
            edits.push(move_edit);
        }
        edits.push(Edit::RemoveLayer { id });
        if reference {
            edits.push(Edit::SetReferences(references));
        }
        Ok(Edit::Batch(edits))
    }
    /// Storage order defines sibling order; parents always precede their subtree
    /// in the UI, including after moving a group as a single item.
    pub fn ordered_layers(&self) -> Vec<&Layer> {
        fn visit<'a>(doc: &'a Document, parent: Option<LayerId>, out: &mut Vec<&'a Layer>) {
            for l in doc.layers.iter().filter(|l| l.properties.parent == parent) {
                out.push(l);
                if l.kind == LayerKind::Group {
                    visit(doc, Some(l.id), out);
                }
            }
        }
        let mut out = Vec::with_capacity(self.layers.len());
        visit(self, None, &mut out);
        out
    }
    pub fn target_owner(&self, target: LayerId) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|l| l.id == target || l.mask.as_ref().is_some_and(|m| m.id == target))
    }
    pub(crate) fn target_strokes_mut(&mut self, target: LayerId) -> Option<&mut Vec<StrokeId>> {
        for layer in &mut self.layers {
            if layer.id == target {
                return Some(&mut layer.strokes);
            }
            if let Some(mask) = &mut layer.mask
                && mask.id == target
            {
                return Some(Arc::make_mut(&mut mask.strokes));
            }
        }
        None
    }
    pub fn active_target(&self) -> LayerId {
        self.layer(self.active_layer)
            .and_then(|l| l.mask.as_ref())
            .filter(|_| self.active_mask)
            .map_or(self.active_layer, |m| m.id)
    }
    pub fn layer_offset(&self, id: LayerId) -> Point {
        let Some(owner) = self.target_owner(id) else {
            return Point::default();
        };
        let mut offset = if owner.id == id {
            owner.properties.offset
        } else {
            owner.mask.as_ref().unwrap().offset
        };
        let mut parent = owner.properties.parent;
        for _ in 0..self.layers.len() {
            let Some(layer) = parent.and_then(|id| self.layer(id)) else {
                break;
            };
            offset.x += layer.properties.offset.x;
            offset.y += layer.properties.offset.y;
            parent = layer.properties.parent;
        }
        offset
    }
    pub fn is_locked(&self, id: LayerId) -> bool {
        let mut target = self.target_owner(id);
        for _ in 0..self.layers.len() {
            let Some(layer) = target else {
                return false;
            };
            if layer.properties.locked {
                return true;
            }
            target = layer.properties.parent.and_then(|id| self.layer(id));
        }
        target.is_some()
    }
    pub fn validate_layer(&self, layer: &Layer) -> Result<(), DocumentError> {
        let paper = self.layers.iter().find(|l| l.kind == LayerKind::Background);
        if (layer.kind == LayerKind::Background
            && (paper.is_some_and(|p| p.id != layer.id)
                || layer.properties.parent.is_some()
                || layer.mask.is_some()
                || layer.properties.clipped
                || layer.properties.blend != LayerBlend::Normal))
            || paper.is_some_and(|p| p.id == layer.id && layer.kind != LayerKind::Background)
        {
            return Err(DocumentError::ProtectedLayer(layer.id));
        }
        if !layer.opacity.is_finite()
            || !(0.0..=1.0).contains(&layer.opacity)
            || !layer.properties.offset.x.is_finite()
            || !layer.properties.offset.y.is_finite()
        {
            return Err(DocumentError::InvalidLayerOperation("Invalid layer value"));
        }
        let mut parent = layer.properties.parent;
        for _ in 0..=self.layers.len() {
            let Some(id) = parent else {
                return Ok(());
            };
            if id == layer.id {
                break;
            }
            let node = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
            if node.kind != LayerKind::Group {
                break;
            }
            parent = node.properties.parent;
        }
        Err(DocumentError::InvalidLayerOperation("Invalid layer group"))
    }
    pub fn move_target_edit(&self, delta: Point) -> Result<Edit, DocumentError> {
        let mut layer = self
            .layer(self.active_layer)
            .ok_or(DocumentError::MissingLayer(self.active_layer))?
            .clone();
        if self.is_locked(layer.id) || layer.kind == LayerKind::Background {
            return Err(DocumentError::ProtectedLayer(layer.id));
        }
        let linked = layer.mask.as_ref().is_some_and(|m| m.linked);
        if !self.active_mask || linked {
            layer.properties.offset.x += delta.x;
            layer.properties.offset.y += delta.y;
        }
        if let Some(mask) = &mut layer.mask
            && (self.active_mask || linked)
        {
            mask.offset.x += delta.x;
            mask.offset.y += delta.y;
        }
        Ok(Edit::ReplaceLayer(Box::new(layer)))
    }
}
