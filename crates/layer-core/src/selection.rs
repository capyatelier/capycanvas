//! Selection coverage, validation, and saved selection targets.
use super::*;

/// Painting changes coverage independently of artwork colors and compositing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionPaintBehavior {
    #[default]
    ColorTransparency,
    BlackWhite,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SelectionMaskProperties {
    pub color: color::RgbColor,
    pub opacity: f32,
}
impl Default for SelectionMaskProperties {
    fn default() -> Self {
        Self {
            color: color::RgbColor::new(color::RgbSpace::Srgb, [1., 0., 0., 1.]).unwrap(),
            opacity: 0.5,
        }
    }
}
impl SelectionMaskProperties {
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.color.validate_working_spaces().is_err()
            || !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection mask properties",
            ));
        }
        Ok(())
    }
}

/// A paintable selection destination. Artwork and visibility masks have their
/// own targets; a saved mask is never clipped by the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionTarget {
    Current,
    Saved(LayerId),
}

impl Layer {
    pub fn selection(id: LayerId, name: impl Into<Arc<str>>, coverage: Selection) -> Self {
        let mut layer = Self::paint(id, name);
        layer.kind = LayerKind::Selection;
        layer.selection = Some(coverage);
        layer
    }
    pub fn is_artwork(&self) -> bool {
        self.kind != LayerKind::Selection
    }
}

impl Document {
    /// Visibility through groups, also used for display-only selection previews.
    pub fn layer_is_visible(&self, id: LayerId) -> bool {
        let mut current = Some(id);
        while let Some(id) = current {
            let Some(layer) = self.layer(id) else {
                return false;
            };
            if !layer.visible {
                return false;
            }
            current = layer.properties.parent;
        }
        true
    }

    /// Resolve stored placement into a working snapshot without copying pixels.
    pub fn saved_selection(&self, id: LayerId) -> Result<Selection, DocumentError> {
        let layer = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
        layer
            .selection
            .as_ref()
            .filter(|_| layer.kind == LayerKind::Selection)
            .ok_or(DocumentError::InvalidLayerOperation(
                "Choose a Selection Layer",
            ))?
            .transformed(self.layer_transform(id))
    }

    /// Construct a validated edit from document-space coverage. Loading/painting
    /// the current selection never overwrites a saved source. History replay uses
    /// the resulting edit directly, so a later lock cannot prevent undo.
    pub fn selection_edit(
        &self,
        target: SelectionTarget,
        coverage: Selection,
    ) -> Result<Edit, DocumentError> {
        coverage.validate()?;
        match target {
            SelectionTarget::Current => Ok(Edit::SetSelection(Some(coverage))),
            SelectionTarget::Saved(id) => {
                self.saved_selection(id)?;
                if self.is_locked(id) {
                    return Err(DocumentError::ProtectedLayer(id));
                }
                let inverse = self.layer_transform(id).inverse().ok_or(
                    DocumentError::InvalidLayerOperation("Invalid selection placement"),
                )?;
                Ok(Edit::SetSavedSelection {
                    id,
                    selection: coverage.transformed(inverse)?,
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    #[default]
    New,
    Add,
    Subtract,
    Intersect,
}

/// Immutable coverage survives subsequent edits, undo and renderer recreation.
/// Legacy masks pack eight 0..4 coverage samples per word; refined masks pack
/// four 0..255 coverage bytes. Rows pad their final word with zero coverage.
/// Pixels are produced by the GPU; this type validates and retains their data.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectionPixels {
    /// Older projects store four coverage samples in each nibble. Feathered
    /// selections retain full 8-bit coverage, four pixels per word.
    #[serde(default)]
    byte_coverage: bool,
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
        Self::with_coverage(extent, bounds, words.into(), false)
    }
    pub fn bytes(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: impl Into<Arc<[u32]>>,
    ) -> Result<Self, DocumentError> {
        Self::with_coverage(extent, bounds, words.into(), true)
    }
    fn with_coverage(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: Arc<[u32]>,
        byte_coverage: bool,
    ) -> Result<Self, DocumentError> {
        let value = Self {
            extent,
            bounds,
            words,
            byte_coverage,
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<(), DocumentError> {
        let [w, h] = self.extent;
        let [x0, y0, x1, y1] = self.bounds;
        let words = &self.words;
        if w == 0 || h == 0 || x0 > x1 || y0 > y1 || x1 > w || y1 > h
            || u64::from(w.div_ceil(self.pixels_per_word())) * u64::from(h) != words.len() as u64
            // Reject values >4 with eight parallel nibble comparisons.
            || (!self.byte_coverage && words.iter().any(|v| v & 0x88888888 != 0 || ((v >> 2) & (v | (v >> 1)) & 0x11111111) != 0))
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection coverage",
            ));
        }
        Ok(())
    }
    pub fn pixels_per_word(&self) -> u32 {
        if self.byte_coverage { 4 } else { 8 }
    }
    pub fn coverage_format(&self) -> u32 {
        if self.byte_coverage { 2 } else { 1 }
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SelectionShape {
    /// Even/odd interiors support holes and disjoint islands.
    Contours(Arc<[Arc<[Point]>]>),
    Pixels(Arc<SelectionPixels>),
}

/// Geometry or immutable GPU-produced coverage. Affine placement and inversion are
/// metadata, so layer-local stroke snapshots never duplicate a selection image.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    pub shape: SelectionShape,
    pub affine: crate::Affine,
    pub inverted: bool,
}
impl Selection {
    pub fn empty() -> Self {
        Self {
            shape: SelectionShape::Contours(Arc::default()),
            affine: Affine::IDENTITY,
            inverted: false,
        }
    }
    pub fn full() -> Self {
        Self {
            inverted: true,
            ..Self::empty()
        }
    }

    /// Shape validation without reading back or resampling coverage. Pixel bounds
    /// are additionally checked against their contents at the project boundary.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.affine.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection transform",
            ));
        }
        match &self.shape {
            SelectionShape::Contours(paths) => {
                if paths
                    .iter()
                    .any(|p| p.len() < 3 || p.iter().any(|v| !v.x.is_finite() || !v.y.is_finite()))
                {
                    return Err(DocumentError::InvalidLayerOperation(
                        "Invalid selection contour",
                    ));
                }
            }
            SelectionShape::Pixels(pixels) => pixels.validate()?,
        }
        Ok(())
    }

    pub fn polygon(points: Vec<Point>) -> Result<Self, DocumentError> {
        if points.len() < 3 || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(DocumentError::InvalidLayerOperation(
                "A selection needs a closed area",
            ));
        }
        Ok(Self {
            shape: SelectionShape::Contours(vec![points.into()].into()),
            affine: crate::Affine::IDENTITY,
            inverted: false,
        })
    }
    pub fn pixels(pixels: Arc<SelectionPixels>) -> Self {
        Self {
            shape: SelectionShape::Pixels(pixels),
            affine: crate::Affine::IDENTITY,
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
        self.affine.bounds(bounds)
    }
    pub fn translated(&self, delta: Point) -> Self {
        Self {
            shape: self.shape.clone(),
            affine: self.affine.then(crate::Affine::translation(delta)),
            inverted: self.inverted,
        }
    }
    /// Compose placement without modifying geometry or resampling coverage.
    /// GPU consumers sample the immutable source only when they need pixels.
    pub fn transformed(&self, affine: crate::Affine) -> Result<Self, DocumentError> {
        let affine = self.affine.then(affine);
        if affine.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection transform",
            ));
        }
        Ok(Self {
            shape: self.shape.clone(),
            affine,
            inverted: self.inverted,
        })
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn legacy_layer_mode_is_ignored_in_favor_of_global_preferences() {
        let p: SelectionMaskProperties = serde_json::from_str(r#"{"painting":"color_transparency","protected":true}"#).unwrap();
        assert_eq!(p, SelectionMaskProperties::default());
        let p: SelectionMaskProperties = serde_json::from_str(r#"{"painting":"black_white","protected":false}"#).unwrap();
        assert_eq!(p, SelectionMaskProperties::default());
        assert!(!serde_json::to_string(&p).unwrap().contains("painting"));
        assert!(!serde_json::to_string(&p).unwrap().contains("protected"));
    }

    fn soft_mask() -> Selection {
        Selection::pixels(Arc::new(
            SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xff804020]).unwrap(),
        ))
    }

    #[test]
    fn saved_selection_roundtrip_and_working_copy_are_independent() {
        let mut editor = Editor::new(Document::new("saved coverage", 64, 64));
        let id = editor.allocate_layer_id();
        let original = soft_mask();
        let mut layer = Layer::selection(id, "Hair", original.clone());
        let properties = SelectionMaskProperties {
            opacity: 0.35,
            ..Default::default()
        };
        layer.properties.selection_mask = Some(properties.clone());
        editor
            .perform(Edit::InsertLayer { index: 0, layer })
            .unwrap();
        let saved_checkpoint = editor.checkpoint();
        let loaded = editor.document().saved_selection(id).unwrap();
        let (SelectionShape::Pixels(saved), SelectionShape::Pixels(copy)) =
            (&original.shape, &loaded.shape)
        else {
            panic!("pixels")
        };
        assert!(Arc::ptr_eq(saved, copy));
        editor
            .perform(Edit::SetSelection(Some(loaded.clone())))
            .unwrap();
        assert_eq!(editor.checkpoint(), saved_checkpoint);
        let edit = editor
            .document()
            .selection_edit(SelectionTarget::Saved(id), Selection::full())
            .unwrap();
        assert!(!edit.changes_image());
        editor.perform(edit).unwrap();
        assert_ne!(editor.checkpoint(), saved_checkpoint);
        assert_eq!(editor.document().selection, Some(loaded));
        editor.undo().unwrap();
        assert_eq!(editor.document().saved_selection(id).unwrap(), original);
        editor.redo().unwrap();
        let mut bytes = Vec::new();
        Project::snapshot(editor.document(), &BTreeMap::new())
            .unwrap()
            .write(&mut bytes)
            .unwrap();
        let restored = Project::read(bytes.as_slice(), ProjectLimits::default())
            .unwrap()
            .document;
        assert_eq!(
            restored.layer(id).unwrap().selection,
            Some(Selection::full())
        );
        assert_eq!(restored.layer(id).unwrap().name.as_ref(), "Hair");
        assert_eq!(
            restored.layer(id).unwrap().properties.selection_mask,
            Some(properties)
        );
        assert_eq!(restored.selection, Some(original.clone()));
        editor.perform(Edit::RemoveLayer { id }).unwrap();
        assert_eq!(editor.document().selection, Some(original));
        editor.undo().unwrap();
        assert!(editor.document().layer(id).is_some());
    }

    #[test]
    fn saved_selection_resolves_group_placement_and_checks_ancestor_locks() {
        let mut doc = Document::new("group coverage", 64, 64);
        let group_id = doc.allocate_layer_id();
        let mut group = Layer::paint(group_id, "Character");
        group.kind = LayerKind::Group;
        group.properties.offset = Point { x: 12., y: 8. };
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: group.clone(),
        })
        .unwrap();
        let id = doc.allocate_layer_id();
        let mut layer = Layer::selection(id, "Hair", soft_mask());
        layer.properties.parent = Some(group_id);
        layer.properties.placement =
            Affine::around(Point::default(), [2., 2.], 0., Point::default());
        doc.apply(Edit::InsertLayer { index: 1, layer }).unwrap();
        let world = doc.saved_selection(id).unwrap();
        assert_eq!(world.affine, doc.layer_transform(id));
        let replacement = world.translated(Point { x: 2., y: 4. });
        doc.apply(
            doc.selection_edit(SelectionTarget::Saved(id), replacement.clone())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(doc.saved_selection(id).unwrap(), replacement);
        group.properties.locked = true;
        doc.apply(Edit::ReplaceLayer(Box::new(group))).unwrap();
        assert!(
            doc.selection_edit(SelectionTarget::Saved(id), Selection::empty())
                .is_err()
        );
        assert!(
            doc.selection_edit(SelectionTarget::Current, Selection::empty())
                .is_ok()
        );
        assert!(
            doc.saved_selection(id).is_ok(),
            "locked masks remain loadable"
        );
        assert!(doc.saved_selection(LayerId(1)).is_err());
    }

    #[test]
    fn selection_nodes_reject_artwork_and_project_limits_include_saved_coverage() {
        let mut doc = Document::new("validation", 64, 64);
        let id = doc.allocate_layer_id();
        let layer = Layer::selection(id, "Region", soft_mask());
        for mutate in [
            |l: &mut Layer| l.opacity = 0.5,
            |l: &mut Layer| l.properties.clipped = true,
            |l: &mut Layer| l.properties.alpha_locked = true,
            |l: &mut Layer| l.mask = Some(LayerMask::reveal_all(LayerId(99), Point::default())),
            |l: &mut Layer| l.selection = None,
        ] {
            let mut invalid = layer.clone();
            mutate(&mut invalid);
            assert!(doc.validate_layer(&invalid).is_err());
        }
        doc.apply(Edit::InsertLayer { index: 0, layer }).unwrap();
        let project = Project::snapshot(&doc, &BTreeMap::new()).unwrap();
        assert!(
            project
                .validate(ProjectLimits {
                    raster_bytes: 3,
                    ..Default::default()
                })
                .is_err()
        );
        let mut old = serde_json::to_value(Document::new("old", 64, 64)).unwrap();
        for layer in old["layers"].as_array_mut().unwrap() {
            layer.as_object_mut().unwrap().remove("selection");
        }
        let old: Document = serde_json::from_value(old).unwrap();
        assert!(old.layers.iter().all(|l| l.selection.is_none()));
        assert_ne!(Selection::empty(), Selection::full());
        assert!(Selection::empty().validate().is_ok());
    }

    #[test]
    fn saved_rows_do_not_interrupt_artwork_clipping_or_accept_raster_edits() {
        let mut doc = Document::new("clipping", 64, 64);
        let saved = doc.allocate_layer_id();
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::selection(saved, "Region", Selection::empty()),
        })
        .unwrap();
        let clip = doc.allocate_layer_id();
        let mut layer = Layer::paint(clip, "Highlights");
        layer.properties.clipped = true;
        doc.apply(Edit::InsertLayer { index: 0, layer }).unwrap();
        assert_eq!(doc.clipping_base(clip), Some(LayerId(1)));
        assert_eq!(doc.clipping_stack_top(LayerId(1)), Some(clip));
        assert!(doc.target_raster(saved).is_none());
        assert!(
            doc.apply(Edit::SetRaster {
                target: saved,
                revision: Default::default()
            })
            .is_err()
        );
        assert!(
            doc.apply(Edit::SetLayerOpacity {
                id: saved,
                opacity: 0.5
            })
            .is_err()
        );
    }
    #[test]
    fn affine_placement_keeps_source_and_composes_with_local_offsets() {
        let pixels = Arc::new(SelectionPixels::new([8, 1], [2, 0, 4, 1], vec![0x4400]).unwrap());
        let original = Selection::pixels(pixels.clone());
        let transform = crate::Affine::around(
            Point { x: 2., y: 3. },
            [2., -3.],
            0.4,
            Point { x: 5., y: 7. },
        );
        let placed = original
            .transformed(transform)
            .unwrap()
            .translated(Point { x: -12., y: 21. });
        let SelectionShape::Pixels(shared) = &placed.shape else {
            panic!("pixels")
        };
        assert!(Arc::ptr_eq(shared, &pixels));
        assert_eq!(
            placed.bounds(),
            transform
                .then(crate::Affine::translation(Point { x: -12., y: 21. }))
                .bounds(original.bounds())
        );
        let restored = placed
            .transformed(placed.affine.inverse().unwrap())
            .unwrap();
        for (a, b) in restored.affine.0.into_iter().zip(crate::Affine::IDENTITY.0) {
            assert!((a - b).abs() < 0.0001);
        }
        assert!(original.transformed(crate::Affine([0.; 6])).is_err());
        assert_eq!(original.affine, crate::Affine::IDENTITY);
    }
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
        assert_eq!(original.affine, crate::Affine::IDENTITY);
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

#[cfg(test)]
mod refinement_tests {
    use super::*;
    fn mask(word:u32)->Selection {
        Selection::pixels(Arc::new(SelectionPixels::bytes([4,1],[0,0,4,1],vec![word]).unwrap()))
    }
    #[test]
    fn refined_selections_keep_original_undo_and_final_redo() {
        for target in [SelectionTarget::Current,SelectionTarget::Saved(LayerId(3))] {
            let mut doc=Document::new("refine",4,1);
            doc.layers.push(Layer::selection(LayerId(3),"Mask",Selection::empty()));
            let original=doc.clone();let mut editor=Editor::new(doc);
            editor.perform(editor.document().selection_edit(target,mask(0xff000000)).unwrap()).unwrap();
            let checkpoint=editor.checkpoint();
            for word in [0xff800000,0xff808000] {
                editor.refine_selection(target,mask(word),editor.document().revision).unwrap();
            }
            assert_eq!(editor.checkpoint()==checkpoint,target==SelectionTarget::Current);
            editor.undo().unwrap();
            assert_eq!(editor.document().selection,original.selection);
            assert_eq!(editor.document().saved_selection(LayerId(3)).unwrap(),Selection::empty());
            assert!(!editor.can_undo());
            editor.redo().unwrap();
            let coverage=match target {SelectionTarget::Current=>editor.document().selection.clone().unwrap(),SelectionTarget::Saved(id)=>editor.document().saved_selection(id).unwrap()};
            assert_eq!(coverage,mask(0xff808000));
        }
    }
    #[test]
    fn refinement_rejects_stale_revision_and_undo_branches() {
        let mut editor=Editor::new(Document::new("refine",4,1));
        editor.perform(Edit::SetSelection(Some(mask(0xff)))).unwrap();
        let revision=editor.document().revision;
        editor.perform(Edit::SetSelection(None)).unwrap();
        assert!(editor.refine_selection(SelectionTarget::Current,mask(0xff00),revision).is_err());
        assert!(editor.document().selection.is_none());
        editor.undo().unwrap();
        assert!(editor.refine_selection(SelectionTarget::Current,mask(0xff00),editor.document().revision).is_err());
        assert!(editor.can_redo());
    }
}
