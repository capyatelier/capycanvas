//! Layer ownership and coverage, independent of UI widgets and GPU storage.
use super::*;

/// World translation of a paint target or its independently placed mask.
pub fn target_offset(layers: &[Layer], id: LayerId) -> Point {
    let Some(owner) = layers
        .iter()
        .find(|l| l.id == id || l.mask.as_ref().is_some_and(|m| m.id == id))
    else {
        return Point::default();
    };
    let mut offset = if owner.id == id {
        owner.properties.offset
    } else {
        owner.mask.as_ref().unwrap().offset
    };
    let mut parent = owner.properties.parent;
    for _ in 0..layers.len() {
        let Some(layer) = parent.and_then(|id| layers.iter().find(|l| l.id == id)) else {
            break;
        };
        offset.x += layer.properties.offset.x;
        offset.y += layer.properties.offset.y;
        parent = layer.properties.parent;
    }
    offset
}

/// Why the active layer has no target for brushes or content tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingRefusal {
    NoLayer,
    Locked,
    /// An effect layer draws on the layer below it, and that layer is locked.
    BaseLocked,
    Group,
    Paper,
    SelectionLayer,
    EffectWithoutBase,
    /// Content tools draw on artwork, and the layer's mask is being edited.
    Mask,
    /// Content tools draw on artwork, and an effect layer draws on its mask.
    EffectMask,
}

/// Paint/mask-local pixels to document pixels. Groups retain their existing
/// translation semantics. A linked mask follows the owner's placement while an
/// unlinked mask stays in its independently translated document position.
pub fn target_transform(layers: &[Layer], id: LayerId) -> Affine {
    let Some(owner) = layers.iter().find(|l| {
        l.id == id || l.mask.as_ref().is_some_and(|m| m.id == id)
    }) else {
        return Affine::IDENTITY;
    };
    if owner.id == id {
        return owner.properties.placement.then(Affine::translation(target_offset(layers, id)));
    }
    let mask = owner.mask.as_ref().unwrap();
    let world = target_offset(layers, owner.id);
    mask.transform_in_parent(&owner.properties).then(Affine::translation(Point {
        x: world.x - owner.properties.offset.x,
        y: world.y - owner.properties.offset.y,
    }))
}

/// The group whose own composite holds the layers below a layer whose parent
/// is `parent`: the nearest of those groups that does not pass through, or
/// None for the document.
pub fn isolated_scope(layers: &[Layer], mut parent: Option<LayerId>) -> Option<LayerId> {
    for _ in 0..layers.len() {
        match parent.and_then(|id| layers.iter().find(|l| l.id == id)) {
            Some(group) if group.passes_through() => parent = group.properties.parent,
            _ => break,
        }
    }
    parent
}

/// The layers composited before `layers[index]` onto the backdrop it draws
/// over: its later siblings and, through each Pass Through group around it,
/// that group's later siblings, all with their subtrees. Storage order is
/// sibling order; a group need not precede its subtree.
pub fn backdrop_layers(layers: &[Layer], index: usize) -> Vec<usize> {
    let position = |id: LayerId| layers.iter().position(|l| l.id == id);
    let mut levels = vec![(layers[index].properties.parent, index)];
    while let Some(&(Some(group), _)) = levels.last()
        && levels.len() <= layers.len()
        && let Some(group) = position(group).filter(|&g| layers[g].passes_through())
    {
        levels.push((layers[group].properties.parent, group));
    }
    (0..layers.len())
        .filter(|&i| {
            let mut root = i;
            for _ in 0..=layers.len() {
                let parent = layers[root].properties.parent;
                if let Some(&(_, own)) = levels.iter().find(|(level, _)| *level == parent) {
                    return root > own;
                }
                let Some(next) = parent.and_then(position) else {
                    return false;
                };
                root = next;
            }
            false
        })
        .collect()
}

/// Whether `layer` lies inside `scope`, where None is the document.
pub fn descends_from(layers: &[Layer], layer: &Layer, scope: Option<LayerId>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    let mut parent = layer.properties.parent;
    for _ in 0..layers.len() {
        match parent {
            Some(id) if id == scope => return true,
            Some(id) => parent = layers.iter().find(|l| l.id == id).and_then(|l| l.properties.parent),
            None => return false,
        }
    }
    false
}

impl Layer {
    /// A Pass Through group composites its layers onto the layers below it;
    /// clipped, it composites isolated like a Normal group.
    pub fn passes_through(&self) -> bool {
        self.kind == LayerKind::Group && self.properties.blend == LayerBlend::PassThrough && !self.properties.clipped
    }
    /// Finite editable local extent: the canvas, the extent a canvas change
    /// left behind and a retained photo's size, whichever is larger. Pixels
    /// beyond the canvas stay hidden until the canvas grows over them again;
    /// placement never changes this extent or crops its backing.
    pub fn local_extent(&self, canvas: [u32; 2]) -> [u32; 2] {
        let stored = self.properties.extent.unwrap_or_default();
        let source = self.source.as_ref().map_or([0; 2], |source| source.extent);
        std::array::from_fn(|i| canvas[i].max(stored[i]).max(source[i]))
    }
    /// Pending Apply mask keeps coverage alive until its submission completes.
    pub fn masks(&self) -> impl Iterator<Item = &LayerMask> {
        self.mask.iter().chain(
            self.pending_operations
                .iter()
                .filter(|op| !matches!(op.kind, LayerOperationKind::Transform(_)))
                .map(|op| &op.coverage),
        )
    }
    pub fn target_operations(&self, id: LayerId) -> Option<&[LayerOperation]> {
        if id == self.id {
            return Some(&self.pending_operations);
        }
        self.masks()
            .find(|m| m.id == id)
            .map(|m| m.pending_operations.as_slice())
    }
    pub fn target_operations_mut(&mut self, id: LayerId) -> Option<&mut Vec<LayerOperation>> {
        if id == self.id {
            return Some(&mut self.pending_operations);
        }
        self.mask
            .as_mut()
            .filter(|m| m.id == id)
            .map(|m| Arc::make_mut(&mut m.pending_operations))
    }
}

#[cfg(test)]
mod organization_tests {
    use super::*;
    #[test]
    fn paint_operations_accept_extended_rgb_and_reject_invalid_coverage() {
        let operations = |color| {
            [
                LayerOperationKind::Fill { color, alpha_locked: false },
                LayerOperationKind::Gradient {
                    start: Point { x: 10., y: 20. },
                    end: Point { x: 80., y: 60. },
                    colors: [color; 2], radial: false, alpha_locked: false,
                },
                LayerOperationKind::Figure(crate::Figure {
                    shape: crate::FigureShape::Rectangle, paint: crate::FigurePaint::Both,
                    start: Point { x: 10., y: 20. }, end: Point { x: 80., y: 60. },
                    width: 4., colors: [color; 2], alpha_locked: false, erase: false,
                }),
            ].map(|kind| LayerOperation {
                placement: Affine::IDENTITY,
                coverage: LayerMask::reveal_all(LayerId(20), Point::default()),
                kind,
            })
        };
        // Portable P3 red is outside sRGB even in an ordinary SDR document.
        let p3 = color::RgbColor::new(color::RgbSpace::DisplayP3, [1., 0., 0., 0.8])
            .unwrap().linear_in(color::RgbSpace::Srgb).unwrap();
        assert!(p3[0] > 1. && p3[1] < 0.);
        for color in [p3, [8., -0.125, 2., 0.25], [-0.01, 1.01, 0., 0.], [1.; 4]] {
            for operation in operations(color) {
                operation.validate().unwrap();
            }
        }
        for channel in 0..4 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut color = [0.5; 4]; color[channel] = value;
                for operation in operations(color) {
                    assert!(operation.validate().is_err(), "channel {channel}: {value}");
                }
            }
        }
        for alpha in [-0.001, 1.001] {
            for operation in operations([0.5, 0.5, 0.5, alpha]) {
                assert!(operation.validate().is_err());
            }
        }
    }

    #[test]
    fn placement_composes_group_offsets_and_linked_masks() {
        let mut doc = Document::new("geometry", 2000, 1500, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let mut group = Layer::paint(LayerId(10), "group");
        group.kind = LayerKind::Group;
        group.properties.offset = Point { x: 20., y: -30. };
        let layer = &mut doc.layers[0];
        let id = layer.id;
        layer.properties.parent = Some(group.id);
        layer.properties.offset = Point { x: 6., y: 9. };
        layer.properties.placement = Affine([0.5, 0., 0., 0.5, -100., 50.]);
        let mut mask = LayerMask::reveal_all(LayerId(11), layer.properties.offset);
        mask.offset.x += 4.;
        layer.mask = Some(mask);
        doc.layers.push(group);
        let p = Point { x: 100., y: 200. };
        assert_eq!(doc.layer_transform(id).map(p), Point { x: -24., y: 129. });
        assert_eq!(doc.layer_transform(LayerId(11)).map(p), Point { x: -22., y: 129. });
        doc.layers[0].mask.as_mut().unwrap().linked = false;
        assert_eq!(doc.layer_transform(LayerId(11)).map(p), Point { x: 130., y: 179. });
        let mut invalid = doc.layers[0].clone();
        invalid.properties.placement = Affine([0.; 6]);
        assert!(doc.validate_layer(&invalid).is_err());
    }
    #[test]
    fn pending_mask_operations_validate_before_submission() {
        let op = LayerOperation {
            placement: crate::Affine::IDENTITY,
            coverage: LayerMask::reveal_all(LayerId(20), Point::default()),
            kind: LayerOperationKind::Transform(ImageTransform::default()),
        };
        let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
        mask.pending_operations = Arc::new(vec![op.clone()]);
        assert!(mask.validate().is_ok());
        let mut snapshot = LayerOperation {
            placement: crate::Affine::IDENTITY,
            coverage: mask.clone(),
            kind: LayerOperationKind::ApplyMask,
        };
        assert!(snapshot.validate().is_ok());
        // These operations already encode geometry in their transform/coverage.
        // A second placement would be ignored by rendering and must not persist.
        for mut operation in [op.clone(), snapshot.clone()] {
            operation.placement = Affine::translation(Point { x: 12., y: -7. });
            assert!(operation.validate().is_err());
        }
        snapshot.kind = op.kind.clone();
        assert!(
            snapshot.validate().is_err(),
            "transform coverage must not recursively contain transforms"
        );
        mask.pending_operations = Arc::default();
        mask.default_coverage = f32::NAN;
        assert!(mask.validate().is_err());
        mask.default_coverage = 1.;
        mask.offset.x = f32::INFINITY;
        assert!(mask.validate().is_err());
    }
    #[test]
    fn references_preserve_objects_and_ancestors_not_unrelated_siblings() {
        let mut doc = Document::new("references", 128, 128, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
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
        let mut doc = Document::new("stack", 100, 100, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
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
        let mut doc = Document::new("groups", 100, 100, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::paint(LayerId(3), "Texture"),
        })
        .unwrap();
        doc.layers[0].mask = Some(LayerMask::reveal_all(LayerId(99), Point { x: 7., y: 9. }));
        let roots = doc.layer_roots(&BTreeSet::from([LayerId(1), LayerId(3)]));
        let name = "  Group { $name }「グループ」🖌️\u{2068}literal\u{2069}  ";
        doc.apply(doc.group_layers_edit(&roots, LayerId(10), LayerBlend::Normal, name).unwrap())
            .unwrap();
        assert_eq!(doc.layer(LayerId(10)).unwrap().name.as_ref(), name);
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
    fn bulk_edits_protect_clipping_stacks_and_locks() {
        let mut doc = Document::new("clipping", 100, 100, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let mut clip = Layer::paint(LayerId(3), "Shade");
        clip.properties.clipped = true;
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: clip,
        })
        .unwrap();
        assert!(doc.delete_layers_edit(&[LayerId(1)]).is_err());
        assert!(doc.group_layers_edit(&[LayerId(1)], LayerId(10), LayerBlend::Normal, "Group").is_err());
        assert!(doc.delete_layers_edit(&[LayerId(1), LayerId(3)]).is_ok());
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

    #[test]
    fn blend_codes_are_stable_and_every_mode_has_one_menu_place() {
        let shipped = ["Normal", "Multiply", "Screen", "Add", "Overlay", "SoftLight", "Color"];
        for (code, name) in shipped.into_iter().enumerate() {
            let blend = LayerBlend::from_code(code as u32).unwrap();
            assert_eq!(serde_json::to_value(blend).unwrap(), name);
            assert_eq!(serde_json::from_value::<LayerBlend>(name.into()).unwrap(), blend);
        }
        for (code, blend) in LayerBlend::ALL.into_iter().enumerate() {
            assert_eq!(blend.code(), code as u32);
            assert_eq!(LayerBlend::from_code(blend.code()), Some(blend));
        }
        assert_eq!(LayerBlend::from_code(LayerBlend::ALL.len() as u32), None);
        let menu: Vec<_> = LayerBlend::MENU.into_iter().flatten().copied().collect();
        assert_eq!(menu.len(), LayerBlend::ALL.len());
        assert!(LayerBlend::ALL.iter().all(|b| menu.contains(b)));
        assert_eq!(LayerBlend::MENU[0], [LayerBlend::PassThrough, LayerBlend::Normal]);
        let float: Vec<_> = menu.iter().filter(|b| b.offered(LayerKind::Paint, true)).map(|b| b.label()).collect();
        assert!(!float.contains(&"Overlay") && !float.contains(&"Hard Mix") && float.contains(&"Linear Light"));
        assert!(menu.iter().all(|b| b.offered(LayerKind::Group, false)));
        let paint: Vec<_> = menu.iter().filter(|b| b.offered(LayerKind::Paint, false)).collect();
        assert_eq!(paint.len(), menu.len() - 1);
        assert!(!paint.contains(&&LayerBlend::PassThrough));
        assert!(LayerBlend::PassThrough.offered(LayerKind::Group, true));
    }

    fn pass_through_document() -> Document {
        let mut doc = Document::new("pass through", 64, 64, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let mut group = Layer::paint(LayerId(10), "Group");
        group.kind = LayerKind::Group;
        group.properties.blend = LayerBlend::PassThrough;
        let mut top = Layer::paint(LayerId(11), "Top");
        top.properties.parent = Some(group.id);
        top.properties.blend = LayerBlend::Multiply;
        let mut adjustment = top.clone();
        adjustment.id = LayerId(12);
        adjustment.kind = LayerKind::Effect;
        adjustment.properties.blend = LayerBlend::Normal;
        adjustment.effect = Some(Arc::new(EffectInstance::new(
            crate::bundled_effect_catalog().get("exposure").unwrap().program(),
        )));
        let mut inner = top.clone();
        inner.id = LayerId(13);
        let above = Layer::paint(LayerId(14), "Above");
        let below = Layer::paint(LayerId(15), "Below");
        doc.layers.splice(0..0, [above, top, adjustment, group, inner, below]);
        for layer in &doc.layers {
            doc.validate_layer(layer).unwrap();
        }
        doc
    }

    #[test]
    fn pass_through_is_for_groups_and_is_saved_by_a_new_name() {
        let mut doc = pass_through_document();
        assert_eq!(serde_json::to_value(LayerBlend::PassThrough).unwrap(), "PassThrough");
        assert_eq!(LayerBlend::PassThrough.label(), "Pass Through");
        let mut paint = doc.layer(LayerId(14)).unwrap().clone();
        paint.properties.blend = LayerBlend::PassThrough;
        assert_eq!(
            doc.apply(Edit::ReplaceLayer(Box::new(paint))).unwrap_err().to_string(),
            "Only groups can use Pass Through"
        );
        let group = doc.layer(LayerId(10)).unwrap();
        assert!(group.passes_through());
        let mut clipped = group.clone();
        clipped.properties.clipped = true;
        assert!(!clipped.passes_through(), "a clipped group composites isolated");
    }

    #[test]
    fn backdrops_widen_through_pass_through_groups_to_the_nearest_isolated_one() {
        let mut doc = pass_through_document();
        let index = |doc: &Document, id: u64| doc.layers.iter().position(|l| l.id == LayerId(id)).unwrap();
        let ids = |doc: &Document, indices: Vec<usize>| indices.into_iter().map(|i| doc.layers[i].id.0).collect::<Vec<_>>();
        assert_eq!(isolated_scope(&doc.layers, Some(LayerId(10))), None);
        assert_eq!(ids(&doc, backdrop_layers(&doc.layers, index(&doc, 12))), [13, 15, 1, 2]);
        assert_eq!(ids(&doc, backdrop_layers(&doc.layers, index(&doc, 11))), [12, 13, 15, 1, 2]);
        assert_eq!(ids(&doc, backdrop_layers(&doc.layers, index(&doc, 14))), [11, 12, 10, 13, 15, 1, 2]);
        doc.reference_layers = [LayerId(12)].into();
        let visible: Vec<_> = doc.reference_snapshot().iter().filter(|l| l.visible).map(|l| l.id.0).collect();
        assert_eq!(visible, [12, 10, 13, 15, 1, 2], "a referenced adjustment keeps what lies below its group");
        let mut group = doc.layer(LayerId(10)).unwrap().clone();
        group.properties.blend = LayerBlend::Normal;
        doc.apply(Edit::ReplaceLayer(Box::new(group))).unwrap();
        assert_eq!(isolated_scope(&doc.layers, Some(LayerId(10))), Some(LayerId(10)));
        assert_eq!(ids(&doc, backdrop_layers(&doc.layers, index(&doc, 12))), [13]);
        let visible: Vec<_> = doc.reference_snapshot().iter().filter(|l| l.visible).map(|l| l.id.0).collect();
        assert_eq!(visible, [12, 10, 13]);
        assert!(descends_from(&doc.layers, doc.layer(LayerId(13)).unwrap(), Some(LayerId(10))));
        assert!(!descends_from(&doc.layers, doc.layer(LayerId(15)).unwrap(), Some(LayerId(10))));
    }

    #[test]
    fn pass_through_groups_ungroup_with_any_layers_at_full_opacity_without_a_mask() {
        let mut doc = pass_through_document();
        let before = doc.clone();
        let undo = doc.apply(doc.ungroup_layer_edit(LayerId(10)).unwrap()).unwrap();
        assert!(doc.layers.iter().all(|l| l.properties.parent.is_none()));
        assert_eq!(doc.layer(LayerId(11)).unwrap().properties.blend, LayerBlend::Multiply);
        doc.apply(undo).unwrap();
        assert_eq!(doc.layers, before.layers);
        let refused = |change: &dyn Fn(&mut Layer)| {
            let mut doc = before.clone();
            let mut group = doc.layer(LayerId(10)).unwrap().clone();
            change(&mut group);
            doc.apply(Edit::ReplaceLayer(Box::new(group))).unwrap();
            doc.ungroup_layer_edit(LayerId(10)).is_err()
        };
        assert!(refused(&|g| g.opacity = 0.5));
        assert!(refused(&|g| g.mask = Some(LayerMask::reveal_all(LayerId(90), Point::default()))));
        assert!(refused(&|g| g.properties.blend = LayerBlend::Normal), "an isolated group keeps its Normal-layer rule");
    }
}

/// How a layer combines with the pixels below it. The discriminant is the
/// renderer's mode code and the flat `ALL` order; documents store the name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum LayerBlend {
    #[default]
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Add = 3,
    Overlay = 4,
    SoftLight = 5,
    Color = 6,
    Darken = 7,
    Lighten = 8,
    ColorBurn = 9,
    LinearBurn = 10,
    ColorDodge = 11,
    HardLight = 12,
    VividLight = 13,
    LinearLight = 14,
    PinLight = 15,
    HardMix = 16,
    Difference = 17,
    Exclusion = 18,
    Subtract = 19,
    Divide = 20,
    Hue = 21,
    Saturation = 22,
    Luminosity = 23,
    /// Groups only: the group's layers composite onto the layers below it, as
    /// if they were not grouped, and its opacity and mask fade between that
    /// result and what lies below.
    PassThrough = 24,
}
/// The operands a blend mode is defined on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendRange {
    /// Defined for extended float values.
    Unbounded,
    /// Defined on [0, 1]; operands are clamped, and float documents do not
    /// offer the mode.
    Unit,
}
/// The values a document's layers combine on. Layer pixels stay linear; in
/// Perceptual documents the composite holds the document's encoded values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlendSpace {
    #[default]
    Linear,
    Perceptual,
}
impl BlendSpace {
    pub const ALL: [Self; 2] = [Self::Perceptual, Self::Linear];
    pub fn label(self) -> &'static str {
        match self {
            Self::Perceptual => "Perceptual",
            Self::Linear => "Linear light",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Perceptual => "Like Photoshop and Clip Studio Paint",
            Self::Linear => "Physically based",
        }
    }
    /// Why a document of this depth cannot blend perceptually.
    pub fn unavailable_reason(depth: color::SampleDepth) -> Option<&'static str> {
        depth.is_float().then_some("Float documents blend in linear light")
    }
    /// The space a document of `depth` uses when this one is chosen.
    pub fn for_depth(self, depth: color::SampleDepth) -> Self {
        if depth.is_float() { Self::Linear } else { self }
    }
    /// Premultiplied linear `rgba` as the composite of this space holds it,
    /// with the transfer curve of `rgb`.
    pub fn composite(self, rgb: color::RgbSpace, rgba: [f32; 4]) -> [f32; 4] {
        if self == Self::Linear || rgba[3] <= 0. {
            return rgba;
        }
        let a = rgba[3];
        let encode = |c: f32| (rgb.encode(f64::from(c / a)) * f64::from(a)) as f32;
        [encode(rgba[0]), encode(rgba[1]), encode(rgba[2]), a]
    }
}
impl LayerBlend {
    pub const ALL: [Self; 25] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Add,
        Self::Overlay,
        Self::SoftLight,
        Self::Color,
        Self::Darken,
        Self::Lighten,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::ColorDodge,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Luminosity,
        Self::PassThrough,
    ];
    /// Menu groups in Photoshop's order: Pass Through and Normal, darken,
    /// lighten, contrast, inversion and component modes.
    pub const MENU: [&'static [Self]; 6] = [
        &[Self::PassThrough, Self::Normal],
        &[Self::Darken, Self::Multiply, Self::ColorBurn, Self::LinearBurn],
        &[Self::Lighten, Self::Screen, Self::ColorDodge, Self::Add],
        &[
            Self::Overlay,
            Self::SoftLight,
            Self::HardLight,
            Self::VividLight,
            Self::LinearLight,
            Self::PinLight,
            Self::HardMix,
        ],
        &[Self::Difference, Self::Exclusion, Self::Subtract, Self::Divide],
        &[Self::Hue, Self::Saturation, Self::Color, Self::Luminosity],
    ];
    pub fn code(self) -> u32 {
        self as u32
    }
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }
    pub fn range(self) -> BlendRange {
        match self {
            Self::Overlay
            | Self::SoftLight
            | Self::HardLight
            | Self::ColorBurn
            | Self::ColorDodge
            | Self::VividLight
            | Self::HardMix
            | Self::Exclusion => BlendRange::Unit,
            _ => BlendRange::Unbounded,
        }
    }
    /// Whether a layer of this kind, in a document of this depth, offers the
    /// mode in its menus.
    pub fn offered(self, kind: LayerKind, float: bool) -> bool {
        (!float || self.range() == BlendRange::Unbounded) && (self != Self::PassThrough || kind == LayerKind::Group)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Add => "Add",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::Color => "Color",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::ColorDodge => "Color Dodge",
            Self::HardLight => "Hard Light",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Subtract => "Subtract",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Luminosity => "Luminosity",
            Self::PassThrough => "Pass Through",
        }
    }
}
impl From<BrushBlendMode> for LayerBlend {
    fn from(mode: BrushBlendMode) -> Self {
        match mode {
            BrushBlendMode::Normal => Self::Normal,
            BrushBlendMode::Multiply => Self::Multiply,
            BrushBlendMode::Screen => Self::Screen,
            BrushBlendMode::Add => Self::Add,
            BrushBlendMode::Subtract => Self::Subtract,
            BrushBlendMode::Darken => Self::Darken,
            BrushBlendMode::Lighten => Self::Lighten,
            BrushBlendMode::Overlay => Self::Overlay,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerProperties {
    pub parent: Option<LayerId>,
    pub offset: Point,
    /// Persistent placement of local source AND raster pixels, before offset.
    /// Apply never resamples backing.
    pub placement: Affine,
    pub alpha_locked: bool,
    pub locked: bool,
    pub clipped: bool,
    pub blend: LayerBlend,
    /// None uses the canvas's default paper color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_color: Option<color::RgbColor>,
    /// Only Selection Layers store these display and painting settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_mask: Option<SelectionMaskProperties>,
    /// Local extent kept when the canvas shrinks, so cropped pixels survive.
    /// It never shrinks with the canvas; see `Layer::local_extent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<[u32; 2]>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerMask {
    /// Unique image identity, allocated from the document layer-ID allocator.
    pub id: LayerId,
    #[serde(skip)]
    pub raster: raster::RasterRevision,
    pub enabled: bool,
    pub linked: bool,
    /// Independent geometry preserves the visible mask when linking changes.
    pub placement: Affine,
    pub offset: Point,
    pub initial: Option<Selection>,
    pub default_coverage: f32,
    pub inverted: bool,
    /// Commands awaiting submission share the paint-layer operation format.
    /// Transform selections cannot contain nested commands.
    #[serde(skip)]
    pub pending_operations: Arc<Vec<LayerOperation>>,
    /// Inspection only; never participates in exported color.
    pub show_area: bool,
}

/// Transient raster commands retain source coverage until their GPU submission.
/// Raster revisions own the resulting pixels, undo states, and recovery data.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerOperation {
    /// Figure/gradient coordinates to local pixels. Coverage is already local.
    pub placement: Affine,
    pub coverage: LayerMask,
    pub kind: LayerOperationKind,
}
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LayerOperationKind {
    ApplyMask,
    /// Erases where the coverage is set: ApplyMask with complemented coverage.
    /// Alpha-locked content keeps its transparency, so nothing changes.
    Erase {
        alpha_locked: bool,
    },
    Transform(crate::ImageTransform),
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
    /// Composite `members`, isolated and in document order, into the empty
    /// target: see `bake_layers`. `offset` places the coordinates of the
    /// members' parent in the target's pixels.
    Bake {
        members: Arc<[Layer]>,
        offset: Point,
    },
    FrequencyDetail {
        members: Arc<[Layer]>,
        offset: Point,
        low: LayerId,
    },
}
impl LayerOperation {
    /// Conservative affected area in layer coordinates. Inverted coverage and
    /// applying a mask can change pixels outside the selection's geometry.
    pub fn bounds(&self, extent: [u32; 2]) -> Rect {
        let mut bounds = match &self.kind {
            LayerOperationKind::Figure(figure) => self.placement.bounds(figure.bounds()),
            LayerOperationKind::Bake { members, offset }
            | LayerOperationKind::FrequencyDetail { members, offset, .. } => crate::merge::bake_bounds(members, *offset, extent),
            _ => Rect::from_extent(extent),
        };
        if self.kind != LayerOperationKind::ApplyMask
            && self.coverage.default_coverage == 0.0
            && !self.coverage.inverted
            && self.coverage.raster.is_empty()
            && let Some(selection) = &self.coverage.initial
            && !selection.inverted
        {
            let selection = selection.translated(self.coverage.offset).bounds();
            bounds.min.x = bounds.min.x.max(selection.min.x);
            bounds.min.y = bounds.min.y.max(selection.min.y);
            bounds.max.x = bounds.max.x.min(selection.max.x);
            bounds.max.y = bounds.max.y.min(selection.max.y);
        }
        if let LayerOperationKind::Transform(transform) = &self.kind {
            transform.affected_bounds(bounds)
        } else {
            bounds
        }
    }
    fn validate(&self) -> Result<(), DocumentError> {
        if self.placement.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation("Invalid paint operation placement"));
        }
        if !self.coverage.pending_operations.is_empty()
            && self.kind != LayerOperationKind::ApplyMask
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Nested coverage operations",
            ));
        }
        self.coverage.validate()?;
        if self
            .coverage
            .initial
            .as_ref()
            .is_some_and(|s| s.affine.inverse().is_none())
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection transform",
            ));
        }
        // Paint is straight linear RGB, like BrushSnapshot. Portable colors can
        // leave the document gamut and HDR can exceed reference white. Only
        // coverage is a unit interval; native storage owns quantization/range.
        let color_ok = |c: &[f32; 4]| c.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&c[3]);
        let valid = match &self.kind {
            LayerOperationKind::ApplyMask | LayerOperationKind::Erase { .. } => {
                self.placement == Affine::IDENTITY
            }
            LayerOperationKind::Bake { offset, .. } | LayerOperationKind::FrequencyDetail { offset, .. } => {
                self.placement == Affine::IDENTITY
                    && offset.x.is_finite()
                    && offset.y.is_finite()
                    && self.coverage.default_coverage == 1.
                    && self.coverage.initial.is_none()
                    && self.coverage.raster.is_empty()
            }
            LayerOperationKind::Transform(transform) => {
                self.placement == Affine::IDENTITY
                    && transform.validate().is_ok()
                    && self.coverage.raster.is_empty()
                    && self.coverage.offset == Point::default()
                    && self.coverage.enabled
                    && !self.coverage.inverted
                    && if self.coverage.initial.is_some() {
                        self.coverage.default_coverage == 0.
                    } else {
                        self.coverage.default_coverage == 1.
                    }
            }
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
    pub fn transform_in_parent(&self, owner: &LayerProperties) -> Affine {
        if self.linked {
            self.placement.then(Affine::translation(Point {
                x: self.offset.x - owner.offset.x,
                y: self.offset.y - owner.offset.y,
            })).then(owner.placement).then(Affine::translation(owner.offset))
        } else {
            self.placement.then(Affine::translation(self.offset))
        }
    }
    /// Change future following behavior without moving/resampling current pixels.
    pub fn set_linked(&mut self, linked: bool, owner: &LayerProperties) -> Result<(), DocumentError> {
        if self.linked == linked { return Ok(()); }
        let current = self.transform_in_parent(owner);
        let mut next = self.clone();
        next.linked = linked;
        next.placement = Affine::IDENTITY;
        next.placement = current.then(next.transform_in_parent(owner).inverse()
            .ok_or(DocumentError::InvalidLayerOperation("Invalid mask placement"))?);
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn validate(&self) -> Result<(), DocumentError> {
        if !self.default_coverage.is_finite()
            || !(0.0..=1.0).contains(&self.default_coverage)
            || !self.offset.x.is_finite()
            || !self.offset.y.is_finite()
            || self.placement.inverse().is_none()
            || self
                .initial
                .as_ref()
                .is_some_and(|s| s.affine.inverse().is_none())
        {
            return Err(DocumentError::InvalidLayerOperation("Invalid mask value"));
        }
        for op in self.pending_operations.iter() {
            if !matches!(op.kind, LayerOperationKind::Transform(_)) {
                return Err(DocumentError::InvalidLayerOperation(
                    "Unsupported mask operation",
                ));
            }
            op.validate()?;
        }
        Ok(())
    }
    pub fn reveal_all(id: LayerId, offset: Point) -> Self {
        Self {
            id,
            enabled: true,
            linked: true,
            placement: Affine::IDENTITY,
            offset,
            initial: None,
            default_coverage: 1.0,
            inverted: false,
            raster: Default::default(),
            pending_operations: Arc::default(),
            show_area: false,
        }
    }
}

impl Document {
    /// Compose reference objects without unrelated artwork. A clipping stack
    /// is one object; a referenced adjustment also needs the layers below it,
    /// through the Pass Through groups around it. Keep original indices for
    /// renderer style records, and ancestor groups for their transforms/masks
    /// without including their unrelated children.
    pub fn reference_snapshot(&self) -> Vec<Layer> {
        let members = self.reference_members(|_| true);
        self.layers
            .iter()
            .map(|layer| {
                let mut snapshot = layer.composite_snapshot();
                snapshot.visible &= members.contains(&layer.id);
                snapshot
            })
            .collect()
    }

    /// The reference objects a retouching stroke on `target` samples beneath
    /// it: the members of [`Self::reference_snapshot`] that composite below
    /// `target`, with their ancestors. Never `target` or anything above it,
    /// so a reference adjustment above the target is ignored.
    pub fn references_below(&self, target: LayerId) -> std::collections::BTreeSet<LayerId> {
        let Some(index) = self.layers.iter().position(|l| l.id == target) else {
            return Default::default();
        };
        self.reference_members(|id| self.layers.iter().position(|l| l.id == id).is_some_and(|i| i > index))
    }

    /// Reference objects expanded to whole clipping stacks, subtrees and
    /// adjustment inputs, those `keep` accepts, and their ancestors.
    fn reference_members(&self, keep: impl Fn(LayerId) -> bool) -> std::collections::BTreeSet<LayerId> {
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
                    members.extend(backdrop_layers(&self.layers, i).into_iter().map(|j| self.layers[j].id));
                }
            }
            if members.len() == before {
                break;
            }
        }
        members.retain(|id| keep(*id));
        for id in members.iter().copied().collect::<Vec<_>>() {
            let mut parent = self.layer(id).and_then(|l| l.properties.parent);
            while let Some(id) = parent {
                members.insert(id);
                parent = self.layer(id).and_then(|l| l.properties.parent);
            }
        }
        members
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
            .find(|l| l.is_artwork() && l.properties.parent == layer.properties.parent && !l.properties.clipped)
            .filter(|l| l.kind == LayerKind::Paint)
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
                .filter(|l| l.is_artwork() && l.properties.parent == parent)
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
            self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
            if self.is_locked(id) {
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
    /// Group a contiguous sibling range in a group that blends with `blend`;
    /// keep complete clipping relationships.
    pub fn group_layers_edit(
        &self,
        roots: &[LayerId],
        group_id: LayerId,
        blend: LayerBlend,
        name: impl Into<std::sync::Arc<str>>,
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
        let mut group = Layer::paint(group_id, name);
        group.kind = LayerKind::Group;
        group.properties.parent = parent;
        group.properties.blend = blend;
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
    /// Remove a neutral group without changing how its layers composite: a
    /// Pass Through group with any layers, or an isolated Normal group whose
    /// layers are Normal.
    pub fn ungroup_layer_edit(&self, id: LayerId) -> Result<Edit, DocumentError> {
        let group = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
        if self.is_locked(id) {
            return Err(DocumentError::ProtectedLayer(id));
        }
        let passes_through = group.passes_through();
        if group.kind != LayerKind::Group
            || group.opacity != 1.
            || group.mask.is_some()
            || !(passes_through || group.properties.blend == LayerBlend::Normal)
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
            if !passes_through && child.properties.blend != LayerBlend::Normal {
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
    pub fn active_target(&self) -> LayerId {
        self.layer(self.active_layer)
            .and_then(|l| l.mask.as_ref())
            .filter(|_| self.active_mask)
            .map_or(self.active_layer, |m| m.id)
    }
    /// Drawing can pass through filters while selection and property editing
    /// stay on the selected layer. Only that layer's own mask takes precedence.
    pub fn drawing_target(&self) -> Option<LayerId> {
        self.try_drawing_target().ok()
    }

    /// The brush target, or why the active layer has none.
    pub fn try_drawing_target(&self) -> Result<LayerId, DrawingRefusal> {
        let mut layer = self.layer(self.active_layer).ok_or(DrawingRefusal::NoLayer)?;
        if self.is_locked(layer.id) {
            return Err(DrawingRefusal::Locked);
        }
        if let Some(mask) = &layer.mask
            && (self.active_mask || layer.kind == LayerKind::Effect)
        {
            return Ok(mask.id);
        }
        while layer.kind == LayerKind::Effect {
            let base = if layer.properties.clipped {
                self.clipping_base(layer.id).and_then(|id| self.layer(id))
            } else {
                self.layers.iter()
                    .skip_while(|next| next.id != layer.id).skip(1)
                    .find(|next| next.is_artwork() && next.properties.parent == layer.properties.parent)
            };
            layer = base.ok_or(DrawingRefusal::EffectWithoutBase)?;
        }
        match layer.kind {
            LayerKind::Paint if self.is_locked(layer.id) => Err(DrawingRefusal::BaseLocked),
            LayerKind::Paint => Ok(layer.id),
            LayerKind::Group => Err(DrawingRefusal::Group),
            LayerKind::Background => Err(DrawingRefusal::Paper),
            LayerKind::Selection => Err(DrawingRefusal::SelectionLayer),
            LayerKind::Effect => Err(DrawingRefusal::EffectWithoutBase),
        }
    }

    /// Fill/figure tools currently operate on ordinary content, not masks.
    pub fn drawing_content(&self) -> Option<LayerId> {
        self.try_drawing_content().ok()
    }

    /// The content target, or why fill, gradient, figure and lasso-fill tools
    /// have none.
    pub fn try_drawing_content(&self) -> Result<LayerId, DrawingRefusal> {
        let target = self.try_drawing_target()?;
        match self.layer(target) {
            Some(_) => Ok(target),
            None if self.active_mask => Err(DrawingRefusal::Mask),
            None => Err(DrawingRefusal::EffectMask),
        }
    }

    /// Why the content tools cannot draw on the active layer.
    pub fn drawing_refusal(&self) -> Option<DrawingRefusal> {
        self.try_drawing_content().err()
    }
    pub fn target_raster(&self, target: LayerId) -> Option<&raster::RasterRevision> {
        let owner = self.target_owner(target)?;
        if !owner.is_artwork() { return None; }
        if owner.id == target {
            Some(&owner.raster)
        } else {
            owner.mask.as_ref().map(|m| &m.raster)
        }
    }
    pub fn target_raster_mut(&mut self, target: LayerId) -> Option<&mut raster::RasterRevision> {
        for layer in &mut self.layers {
            if layer.id == target {
                return layer.is_artwork().then_some(&mut layer.raster);
            }
            if let Some(mask) = &mut layer.mask
                && mask.id == target
            {
                return Some(&mut mask.raster);
            }
        }
        None
    }
    pub fn layer_offset(&self, id: LayerId) -> Point {
        target_offset(&self.layers, id)
    }
    pub fn layer_transform(&self, id: LayerId) -> Affine {
        target_transform(&self.layers, id)
    }
    pub fn target_extent(&self, id: LayerId) -> [u32; 2] {
        let canvas = [self.width, self.height];
        self.target_owner(id).map_or(canvas, |l| l.local_extent(canvas))
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
        if (layer.kind == LayerKind::Selection) != layer.selection.is_some() {
            return Err(DocumentError::InvalidLayerOperation("Invalid Selection Layer coverage"));
        }
        if let Some(selection) = &layer.selection {
            selection.validate()?;
            if layer.mask.is_some() || !layer.raster.is_empty()
                || !layer.pending_operations.is_empty() || layer.properties.clipped
                || layer.properties.alpha_locked || layer.properties.blend != LayerBlend::Normal
                || layer.opacity != 1.
            {
                return Err(DocumentError::InvalidLayerOperation("Selection Layers cannot contain artwork"));
            }
        }
        if let Some(color) = layer.properties.paper_color {
            if layer.kind != LayerKind::Background || color.validate_working_spaces().is_err() {
                return Err(DocumentError::InvalidLayerOperation("Invalid paper color"));
            }
        }
        if let Some(mask) = &layer.properties.selection_mask {
            if layer.kind != LayerKind::Selection { return Err(DocumentError::InvalidLayerOperation("Mask properties require a Selection Layer")); }
            mask.validate()?;
        }
        if layer.kind != LayerKind::Paint && layer.source.is_some() {
            return Err(DocumentError::InvalidLayerOperation("Invalid layer source"));
        }
        if layer.source.as_ref().is_some_and(|s| !s.is_original()
            && (s.interpretation.depth != self.color.depth
                || s.interpretation.profile != crate::color::ColorProfile::Builtin(self.color.space)))
        {
            return Err(DocumentError::InvalidLayerOperation("Rasterized image interpretation differs from the document"));
        }
        if layer.source.as_ref().is_some_and(|s| s.validate().is_err()) {
            return Err(DocumentError::InvalidLayerOperation("Invalid tiled source"));
        }
        for op in &layer.pending_operations {
            op.validate()?;
        }
        if let Some(mask) = &layer.mask {
            mask.validate()?;
        }
        if layer.properties.blend == LayerBlend::PassThrough && layer.kind != LayerKind::Group {
            return Err(DocumentError::InvalidLayerOperation("Only groups can use Pass Through"));
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
        if layer.properties.extent.is_some_and(|e| e.iter().any(|v| *v > crate::MAX_EXTENT)) {
            return Err(DocumentError::InvalidLayerOperation("Layer extent exceeds the image limit"));
        }
        if !layer.opacity.is_finite()
            || !(0.0..=1.0).contains(&layer.opacity)
            || !layer.properties.offset.x.is_finite()
            || !layer.properties.offset.y.is_finite()
            || layer.properties.placement.inverse().is_none()
            || (layer.properties.placement != Affine::IDENTITY
                && !matches!(layer.kind, LayerKind::Paint | LayerKind::Selection))
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
