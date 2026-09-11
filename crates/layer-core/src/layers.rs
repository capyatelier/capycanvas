//! Layer ownership and coverage, independent of UI widgets and GPU storage.
use super::*;

#[cfg(test)]
mod organization_tests {
    use super::*;
    #[test]
    fn references_preserve_objects_and_ancestors_not_unrelated_siblings() {
        let mut doc = Document::new("references", 128, 128);
        let mut group = Layer::paint(LayerId(10), "Group");
        group.kind = LayerKind::Group;
        group.properties.offset = Point { x: 5., y: 8. };
        let mut line = Layer::paint(LayerId(11), "Line");
        line.properties.parent = Some(group.id);
        let mut clip = line.clone();
        clip.id = LayerId(12);
        clip.properties.clipped = true;
        let mut unrelated = line.clone();
        unrelated.id = LayerId(13);
        doc.layers.splice(0..0, [group, clip, line, unrelated]);
        let visible = |doc: &Document| {
            doc.reference_snapshot()
                .iter()
                .filter(|l| l.visible)
                .map(|l| l.id.0)
                .collect::<Vec<_>>()
        };
        assert!(visible(&doc).is_empty());
        for id in [11, 12] {
            doc.reference_layers = [LayerId(id)].into();
            assert_eq!(visible(&doc), [10, 12, 11]);
            let snapshot = doc.reference_snapshot();
            assert_eq!(snapshot[0].properties, doc.layers[0].properties);
            assert!(
                snapshot
                    .iter()
                    .map(|l| l.id)
                    .eq(doc.layers.iter().map(|l| l.id))
            );
        }
        doc.reference_layers = [LayerId(10)].into();
        assert_eq!(visible(&doc), [10, 12, 11, 13]);
        doc.layers[0].visible = false;
        assert!(
            !doc.reference_snapshot()[0].visible,
            "hidden ancestors remain hidden"
        );
    }

    #[test]
    fn clipping_stack_top_respects_siblings_and_hidden_members() {
        let mut doc = Document::new("stack", 100, 100);
        let mut group = Layer::paint(LayerId(7), "Group");
        group.kind = LayerKind::Group;
        let mut clip = Layer::paint(LayerId(3), "Hidden clip");
        clip.visible = false;
        clip.properties.parent = Some(group.id);
        clip.properties.clipped = true;
        let mut second = clip.clone();
        second.id = LayerId(4);
        let mut base = doc.layers[0].clone();
        base.properties.parent = Some(group.id);
        // Storage can interleave unrelated roots; only sibling order matters.
        doc.layers = vec![
            group,
            clip,
            Layer::paint(LayerId(8), "Other root"),
            second,
            base,
            doc.layers[1].clone(),
        ];
        assert_eq!(doc.clipping_stack_top(LayerId(1)), Some(LayerId(3)));
        assert_eq!(doc.clipping_stack_top(LayerId(4)), Some(LayerId(3)));
        assert_eq!(doc.clipping_stack_top(LayerId(8)), Some(LayerId(8)));
        assert_eq!(doc.clipping_stack_top(LayerId(99)), None);
    }

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

/// Immutable coverage survives subsequent edits, undo and renderer recreation.
/// Each row contains ceil(width / 8) words, with eight 0..4 coverage nibbles.
/// Pixels are produced by the GPU; this type validates and retains their data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionPixels {
    extent: [u32; 2],
    bounds: [u32; 4],
    words: Arc<[u32]>,
}
impl SelectionPixels {
    pub fn new(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: impl Into<Arc<[u32]>>,
    ) -> Result<Self, DocumentError> {
        let words = words.into();
        let [w, h] = extent;
        let [x0, y0, x1, y1] = bounds;
        if w == 0 || h == 0 || x0 > x1 || y0 > y1 || x1 > w || y1 > h
            || u64::from(w.div_ceil(8)) * u64::from(h) != words.len() as u64
            // Reject values >4 with eight parallel nibble comparisons.
            || words.iter().any(|v| v & 0x88888888 != 0 || ((v >> 2) & (v | (v >> 1)) & 0x11111111) != 0)
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection coverage",
            ));
        }
        Ok(Self {
            extent,
            bounds,
            words,
        })
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn bounds(&self) -> [u32; 4] {
        self.bounds
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionShape {
    /// Even/odd interiors support holes and disjoint islands.
    Contours(Arc<[Arc<[Point]>]>),
    Pixels(Arc<SelectionPixels>),
}

/// Geometry or immutable GPU-produced coverage. Translation and inversion are
/// metadata, so layer-local stroke snapshots never duplicate a selection image.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub shape: SelectionShape,
    pub offset: Point,
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
            shape: SelectionShape::Contours(vec![points.into()].into()),
            offset: Point::default(),
            inverted: false,
        })
    }
    pub fn pixels(pixels: Arc<SelectionPixels>) -> Self {
        Self {
            shape: SelectionShape::Pixels(pixels),
            offset: Point::default(),
            inverted: false,
        }
    }
    pub fn contours(&self) -> &[Arc<[Point]>] {
        match &self.shape {
            SelectionShape::Contours(paths) => paths,
            SelectionShape::Pixels(_) => &[],
        }
    }
    /// Conservative local bounds, including one pixel for boundary sampling.
    pub fn bounds(&self) -> Rect {
        let mut bounds = Rect::EMPTY;
        match &self.shape {
            SelectionShape::Contours(paths) => {
                for p in paths.iter().flat_map(|c| c.iter()) {
                    bounds.include_circle(*p, 1.);
                }
            }
            SelectionShape::Pixels(pixels) => {
                let [x0, y0, x1, y1] = pixels.bounds;
                if x0 != x1 && y0 != y1 {
                    bounds.include_circle(
                        Point {
                            x: x0 as f32,
                            y: y0 as f32,
                        },
                        1.,
                    );
                    bounds.include_circle(
                        Point {
                            x: x1 as f32,
                            y: y1 as f32,
                        },
                        1.,
                    );
                }
            }
        }
        bounds.min.x += self.offset.x;
        bounds.min.y += self.offset.y;
        bounds.max.x += self.offset.x;
        bounds.max.y += self.offset.y;
        bounds
    }
    pub fn translated(&self, delta: Point) -> Self {
        Self {
            shape: self.shape.clone(),
            offset: Point {
                x: self.offset.x + delta.x,
                y: self.offset.y + delta.y,
            },
            inverted: self.inverted,
        }
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn packed_coverage_validation_checks_all_nibbles_and_dimensions() {
        for value in 0..16 {
            for shift in (0..32).step_by(4) {
                assert_eq!(
                    SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![value << shift]).is_ok(),
                    value <= 4
                );
            }
        }
        for (extent, bounds, words) in [
            ([0, 1], [0, 0, 0, 1], vec![]),
            ([9, 1], [0, 0, 9, 1], vec![0]),
            ([1, 1], [0, 0, 2, 1], vec![0]),
            ([1, 1], [1, 0, 0, 1], vec![0]),
            ([u32::MAX, u32::MAX], [0, 0, 1, 1], vec![0]),
        ] {
            assert!(SelectionPixels::new(extent, bounds, words).is_err());
        }
    }
    #[test]
    fn translating_and_inverting_share_immutable_selection_storage() {
        let pixels = Arc::new(SelectionPixels::new([8, 1], [2, 0, 4, 1], vec![0x4400]).unwrap());
        let original = Selection::pixels(pixels.clone());
        let mut moved = original.translated(Point { x: -2., y: 7.5 });
        moved.inverted = true;
        let SelectionShape::Pixels(shared) = &moved.shape else {
            panic!("pixels")
        };
        assert!(Arc::ptr_eq(shared, &pixels));
        assert!(!original.inverted);
        assert_eq!(original.offset, Point::default());
        assert_eq!(
            moved.bounds(),
            Rect {
                min: Point { x: -1., y: 6.5 },
                max: Point { x: 3., y: 9.5 }
            }
        );

        let polygon = Selection::polygon(vec![
            Point { x: 1., y: 1. },
            Point { x: 5., y: 1. },
            Point { x: 5., y: 5. },
        ])
        .unwrap();
        let moved = polygon.translated(Point { x: 10., y: -4. });
        assert!(Arc::ptr_eq(&polygon.contours()[0], &moved.contours()[0]));
        assert_eq!(
            moved.bounds(),
            Rect {
                min: Point { x: 10., y: -4. },
                max: Point { x: 16., y: 2. }
            }
        );
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
    Figure(Figure),
    Fill {
        color: [f32; 4],
        alpha_locked: bool,
    },
    Gradient {
        start: Point,
        end: Point,
        colors: [[f32; 4]; 2],
        radial: bool,
        alpha_locked: bool,
    },
}
impl LayerOperation {
    /// Conservative affected area in layer coordinates. Inverted coverage and
    /// applying a mask can change pixels outside the selection's geometry.
    pub fn bounds(&self, extent: [u32; 2]) -> Rect {
        let mut bounds = match &self.kind {
            LayerOperationKind::Figure(figure) => figure.bounds(),
            _ => Rect {
                min: Point::default(),
                max: Point {
                    x: extent[0] as f32,
                    y: extent[1] as f32,
                },
            },
        };
        if self.kind != LayerOperationKind::ApplyMask
            && self.coverage.default_coverage == 0.0
            && !self.coverage.inverted
            && self.coverage.strokes.is_empty()
            && let Some(selection) = &self.coverage.initial
            && !selection.inverted
        {
            let selection = selection.translated(self.coverage.offset).bounds();
            bounds.min.x = bounds.min.x.max(selection.min.x);
            bounds.min.y = bounds.min.y.max(selection.min.y);
            bounds.max.x = bounds.max.x.min(selection.max.x);
            bounds.max.y = bounds.max.y.min(selection.max.y);
        }
        bounds
    }
    fn validate(&self) -> Result<(), DocumentError> {
        let color_ok = |c: &[f32; 4]| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v));
        let valid = match &self.kind {
            LayerOperationKind::ApplyMask => true,
            LayerOperationKind::Figure(figure) => figure.valid(),
            LayerOperationKind::Fill { color, .. } => color_ok(color),
            LayerOperationKind::Gradient {
                start, end, colors, ..
            } => {
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let length2 = dx * dx + dy * dy;
                [start.x, start.y, end.x, end.y]
                    .iter()
                    .all(|v| v.is_finite())
                    && length2.is_finite()
                    && length2 >= 0.000001
                    && colors.iter().all(color_ok)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(DocumentError::InvalidLayerOperation(
                "Invalid paint operation",
            ))
        }
    }
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
    /// Compose reference objects without unrelated artwork. A clipping stack
    /// is one object; a referenced adjustment also needs its input siblings.
    /// Keep original indices for renderer style records, and ancestor groups
    /// for their transforms/masks without including their unrelated children.
    pub fn reference_snapshot(&self) -> Vec<Layer> {
        let mut members = self.reference_layers.clone();
        loop {
            let before = members.len();
            members = self.layer_subtrees(&members.iter().copied().collect::<Vec<_>>());
            for (i, layer) in self.layers.iter().enumerate() {
                if !members.contains(&layer.id) {
                    continue;
                }
                let base = if layer.properties.clipped {
                    self.clipping_base(layer.id).unwrap_or(layer.id)
                } else {
                    layer.id
                };
                members.insert(base);
                let base_index = self.layers.iter().position(|l| l.id == base).unwrap();
                members.extend(
                    self.layers[..base_index]
                        .iter()
                        .rev()
                        .filter(|l| l.properties.parent == layer.properties.parent)
                        .take_while(|l| l.properties.clipped)
                        .map(|l| l.id),
                );
                if layer
                    .effect
                    .as_ref()
                    .is_some_and(|e| e.program.kind == EffectKind::Adjustment)
                    && !layer.properties.clipped
                {
                    members.extend(
                        self.layers[i + 1..]
                            .iter()
                            .filter(|l| l.properties.parent == layer.properties.parent)
                            .map(|l| l.id),
                    );
                }
            }
            if members.len() == before {
                break;
            }
        }
        for id in members.iter().copied().collect::<Vec<_>>() {
            let mut parent = self.layer(id).and_then(|l| l.properties.parent);
            while let Some(id) = parent {
                members.insert(id);
                parent = self.layer(id).and_then(|l| l.properties.parent);
            }
        }
        self.layers
            .iter()
            .map(|layer| {
                let mut snapshot = layer.composite_snapshot();
                snapshot.visible &= members.contains(&layer.id);
                snapshot
            })
            .collect()
    }

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
    /// Insert ordinary adjustments above this sibling stack, never between its
    /// clipping base and attached layers. Hidden clips still own their place.
    pub fn clipping_stack_top(&self, id: LayerId) -> Option<LayerId> {
        let index = self.layers.iter().position(|l| l.id == id)?;
        let parent = self.layers[index].properties.parent;
        Some(
            self.layers[..index]
                .iter()
                .rev()
                .filter(|l| l.properties.parent == parent)
                .take_while(|l| l.properties.clipped)
                .last()
                .map_or(id, |l| l.id),
        )
    }
    /// Capability checks must not clone the document or construct undo edits.
    pub fn can_delete_layers(&self, roots: &[LayerId]) -> bool {
        self.deletable_layer_ids(roots).is_ok()
    }
    fn deletable_layer_ids(&self, roots: &[LayerId]) -> Result<BTreeSet<LayerId>, DocumentError> {
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
        if !self
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Paint && !ids.contains(&l.id))
        {
            return Err(DocumentError::LastPaintLayer);
        }
        Ok(ids)
    }
    pub fn delete_layers_edit(&self, roots: &[LayerId]) -> Result<Edit, DocumentError> {
        let ids = self.deletable_layer_ids(roots)?;
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
        for op in &layer.operations {
            op.validate()?;
        }
        if (layer.kind == LayerKind::Effect) != layer.effect.is_some() {
            return Err(DocumentError::InvalidLayerOperation("Invalid effect layer"));
        }
        if let Some(effect) = &layer.effect {
            effect
                .validate()
                .map_err(DocumentError::InvalidLayerOperation)?;
        }
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
