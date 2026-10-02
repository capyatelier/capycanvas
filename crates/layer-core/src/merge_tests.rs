use super::*;

const PAPER: LayerId = LayerId(2);

/// Top to bottom: the named paint layers, then the paper.
fn document(names: &[&str]) -> Document {
    let mut doc = Document::new("merge", 600, 400, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.layers.remove(0);
    for (i, name) in names.iter().enumerate() {
        let id = doc.allocate_layer_id();
        doc.layers.insert(i, Layer::paint(id, *name));
    }
    doc.active_layer = doc.layers[0].id;
    doc
}

fn id(doc: &Document, name: &str) -> LayerId {
    doc.layers.iter().find(|l| &*l.name == name).unwrap().id
}

fn layer_mut<'a>(doc: &'a mut Document, name: &str) -> &'a mut Layer {
    doc.layers.iter_mut().find(|l| &*l.name == name).unwrap()
}

fn activate(doc: &mut Document, name: &str) {
    doc.active_layer = id(doc, name);
}

fn effect(doc: &mut Document, name: &str, filter: &str) {
    let layer = layer_mut(doc, name);
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(
        crate::bundled_effect_catalog().get(filter).unwrap().program(),
    )));
}

fn nest(doc: &mut Document, name: &str, children: &[&str]) {
    let group = id(doc, name);
    layer_mut(doc, name).kind = LayerKind::Group;
    for child in children {
        layer_mut(doc, child).properties.parent = Some(group);
    }
}

fn baked(doc: &Document, kind: MergeKind) -> Vec<String> {
    let plan = doc.merge_plan(kind, LayerId(100), LayerId(101)).unwrap();
    let LayerOperationKind::Bake { members, .. } = &plan.operation.kind else { unreachable!() };
    members.iter().map(|l| l.name.to_string()).collect()
}

fn names(doc: &Document) -> Vec<String> {
    doc.layers.iter().map(|l| l.name.to_string()).collect()
}

/// Apply the plan as `insert_with_operations` would, returning its inverse.
fn merged(doc: &mut Document, kind: MergeKind) -> Edit {
    let plan = doc.merge_plan(kind, LayerId(100), LayerId(101)).unwrap();
    let mut edits = plan.edits;
    for edit in &mut edits {
        if let Edit::InsertLayer { layer, .. } = edit {
            layer.pending_operations.push(plan.operation.clone());
        }
    }
    doc.apply(Edit::Batch(edits)).unwrap()
}

#[test]
fn merge_down_replaces_both_layers_in_one_reversible_edit_and_transfers_references() {
    let mut doc = document(&["Top", "Upper", "Lower", "Bottom"]);
    activate(&mut doc, "Upper");
    layer_mut(&mut doc, "Upper").opacity = 0.4;
    layer_mut(&mut doc, "Lower").properties.alpha_locked = true;
    doc.reference_layers = [id(&doc, "Upper"), id(&doc, "Bottom")].into();
    let before = doc.clone();
    assert_eq!(baked(&doc, MergeKind::Down), ["Upper", "Lower"]);
    let undo = merged(&mut doc, MergeKind::Down);
    assert_eq!(names(&doc), ["Top", "Lower", "Bottom", "Paper"]);
    let result = doc.layer(LayerId(100)).unwrap();
    assert_eq!((result.opacity, result.properties.blend, result.mask.is_none()), (1., LayerBlend::Normal, true));
    assert!(result.properties.alpha_locked, "the result keeps the lower layer's alpha lock");
    assert_eq!(result.properties.extent, None);
    assert_eq!(doc.active_layer, LayerId(100));
    assert_eq!(doc.reference_layers, [LayerId(100), id(&before, "Bottom")].into());
    doc.apply(undo).unwrap();
    assert_eq!(doc.layers, before.layers);
    assert_eq!((doc.active_layer, &doc.reference_layers), (before.active_layer, &before.reference_layers));
}

#[test]
fn merge_down_refuses_with_a_reason() {
    use MergeRefusal as R;
    let refusal = |edit: &dyn Fn(&mut Document)| {
        let mut doc = document(&["Upper", "Lower"]);
        edit(&mut doc);
        doc.merge_refusal(MergeKind::Down)
    };
    assert_eq!(refusal(&|_| {}), None);
    assert_eq!(refusal(&|d| drop(d.layers.remove(1))), Some(R::PaperBelow));
    assert_eq!(refusal(&|d| activate(d, "Lower")), Some(R::PaperBelow));
    assert_eq!(refusal(&|d| d.active_layer = PAPER), Some(R::Paper));
    assert_eq!(refusal(&|d| d.active_layer = LayerId(99)), Some(R::NoLayer));
    assert_eq!(refusal(&|d| layer_mut(d, "Upper").visible = false), Some(R::Hidden));
    assert_eq!(refusal(&|d| layer_mut(d, "Upper").properties.blend = LayerBlend::Multiply), Some(R::NotNormal));
    assert_eq!(refusal(&|d| layer_mut(d, "Upper").properties.locked = true), Some(R::Locked));
    assert_eq!(refusal(&|d| layer_mut(d, "Lower").visible = false), Some(R::BelowHidden));
    assert_eq!(refusal(&|d| layer_mut(d, "Lower").properties.locked = true), Some(R::BelowLocked));
    assert_eq!(refusal(&|d| layer_mut(d, "Lower").properties.blend = LayerBlend::Screen), Some(R::BelowNotNormal));
    assert_eq!(refusal(&|d| effect(d, "Lower", "levels")), Some(R::BelowEffect));
    assert_eq!(refusal(&|d| layer_mut(d, "Upper").kind = LayerKind::Selection), Some(R::SelectionLayer));
    let mut doc = document(&["Upper", "Lower"]);
    let group = doc.allocate_layer_id();
    doc.layers.insert(0, Layer { kind: LayerKind::Group, ..Layer::paint(group, "Group") });
    layer_mut(&mut doc, "Upper").properties.parent = Some(group);
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(R::NoLayerBelow), "the layer below is in the same group");
    layer_mut(&mut doc, "Group").properties.locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(R::Locked), "ancestor locks apply");
}

#[test]
fn clipping_bases_bake_their_visible_stack_and_keep_hidden_clips() {
    let mut doc = document(&["Hidden clip", "Shade", "Base", "Below"]);
    for name in ["Hidden clip", "Shade"] {
        layer_mut(&mut doc, name).properties.clipped = true;
    }
    layer_mut(&mut doc, "Hidden clip").visible = false;
    layer_mut(&mut doc, "Shade").properties.blend = LayerBlend::Multiply;
    activate(&mut doc, "Base");
    assert_eq!(doc.merge_down(), MergeDown::ClippingStack);
    assert_eq!(baked(&doc, MergeKind::Down), ["Shade", "Base"]);
    merged(&mut doc, MergeKind::Down);
    assert_eq!(names(&doc), ["Hidden clip", "Base", "Below", "Paper"]);
    assert_eq!(doc.clipping_base(id(&doc, "Hidden clip")), Some(LayerId(100)));
    assert!(!doc.layer(LayerId(100)).unwrap().properties.clipped);

    let mut doc = document(&["Shade", "Base"]);
    layer_mut(&mut doc, "Shade").properties.clipped = true;
    layer_mut(&mut doc, "Shade").visible = false;
    activate(&mut doc, "Base");
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::ClipsHidden));
    layer_mut(&mut doc, "Shade").visible = true;
    layer_mut(&mut doc, "Base").properties.blend = LayerBlend::Multiply;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::NotNormal));
}

#[test]
fn clipped_layers_merge_within_their_stack() {
    let mut doc = document(&["Upper", "Lower", "Base", "Free"]);
    for name in ["Upper", "Lower"] {
        layer_mut(&mut doc, name).properties.clipped = true;
    }
    assert_eq!(doc.merge_down(), MergeDown::Layer);
    let plan = doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap();
    let LayerOperationKind::Bake { members, offset } = &plan.operation.kind else { unreachable!() };
    assert!(
        bake_layers(members, *offset).iter().all(|l| !l.properties.clipped),
        "clips without their base composite as an ordinary stack"
    );
    merged(&mut doc, MergeKind::Down);
    assert!(doc.layer(LayerId(100)).unwrap().properties.clipped, "the result stays clipped to the base");
    assert_eq!(doc.clipping_base(LayerId(100)), Some(id(&doc, "Base")));

    let mut doc = document(&["Upper", "Base", "Other"]);
    layer_mut(&mut doc, "Upper").properties.clipped = true;
    assert_eq!(baked(&doc, MergeKind::Down), ["Upper", "Base"]);
    let mut doc = document(&["Upper", "Clip", "Base"]);
    layer_mut(&mut doc, "Clip").properties.clipped = true;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::BelowClipped));
}

#[test]
fn effects_apply_to_the_layer_below_or_bake_their_clipping_stack() {
    let mut doc = document(&["Levels", "Photo", "Backdrop"]);
    effect(&mut doc, "Levels", "levels");
    assert_eq!(doc.merge_down(), MergeDown::ApplyEffect);
    assert_eq!(baked(&doc, MergeKind::Down), ["Levels", "Photo"]);
    let plan = doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap();
    let result = plan.edits.iter().find_map(|e| match e {
        Edit::InsertLayer { layer, .. } => Some(layer),
        _ => None,
    });
    assert_eq!(result.unwrap().properties.extent, None, "effects bake the canvas only");

    let mut doc = document(&["Levels", "Shade", "Base", "Below"]);
    effect(&mut doc, "Levels", "levels");
    for name in ["Levels", "Shade"] {
        layer_mut(&mut doc, name).properties.clipped = true;
    }
    assert_eq!(doc.merge_down(), MergeDown::ClippingStack);
    assert_eq!(baked(&doc, MergeKind::Down), ["Levels", "Shade", "Base"]);
    layer_mut(&mut doc, "Base").visible = false;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::BaseHidden));
}

#[test]
fn merge_group_keeps_the_groups_blend_and_opacity_and_bakes_its_mask() {
    let mut doc = document(&["Group", "Inside", "Hidden", "Outside"]);
    nest(&mut doc, "Group", &["Inside", "Hidden"]);
    layer_mut(&mut doc, "Hidden").visible = false;
    let mask = doc.allocate_layer_id();
    let group = layer_mut(&mut doc, "Group");
    group.opacity = 0.5;
    group.properties.blend = LayerBlend::Screen;
    group.mask = Some(LayerMask::reveal_all(mask, Point::default()));
    assert_eq!(doc.merge_refusal(MergeKind::Group), None);
    let plan = doc.merge_plan(MergeKind::Group, LayerId(100), LayerId(101)).unwrap();
    let LayerOperationKind::Bake { members, .. } = &plan.operation.kind else { unreachable!() };
    let baked = &members[0];
    assert_eq!((baked.opacity, baked.properties.blend, baked.mask.is_some()), (1., LayerBlend::Normal, true));
    merged(&mut doc, MergeKind::Group);
    assert_eq!(names(&doc), ["Group", "Outside", "Paper"]);
    let result = doc.layer(LayerId(100)).unwrap();
    assert_eq!((result.kind, result.opacity, result.properties.blend), (LayerKind::Paint, 0.5, LayerBlend::Screen));
    activate(&mut doc, "Outside");
    assert_eq!(doc.merge_refusal(MergeKind::Group), Some(MergeRefusal::NotGroup));

    let mut doc = document(&["Group", "Inside", "Outside"]);
    nest(&mut doc, "Group", &["Inside"]);
    layer_mut(&mut doc, "Group").properties.blend = LayerBlend::PassThrough;
    layer_mut(&mut doc, "Group").opacity = 0.5;
    activate(&mut doc, "Group");
    let plan = doc.merge_plan(MergeKind::Group, LayerId(100), LayerId(101)).unwrap();
    let LayerOperationKind::Bake { members, .. } = &plan.operation.kind else { unreachable!() };
    assert_eq!(members[0].properties.blend, LayerBlend::Normal, "the group bakes isolated");
    merged(&mut doc, MergeKind::Group);
    let result = doc.layer(LayerId(100)).unwrap();
    assert_eq!((result.opacity, result.properties.blend), (0.5, LayerBlend::Normal), "a Pass Through group merges into a Normal layer");

    let mut doc = document(&["Group", "Selection"]);
    nest(&mut doc, "Group", &["Selection"]);
    let selection = layer_mut(&mut doc, "Selection");
    selection.kind = LayerKind::Selection;
    assert_eq!(doc.merge_refusal(MergeKind::Group), Some(MergeRefusal::SelectionLayersInside));
}

#[test]
fn merge_visible_keeps_hidden_layers_and_releases_their_clipping() {
    let mut doc = document(&["Top", "Hidden clip", "Base", "Hidden", "Bottom"]);
    layer_mut(&mut doc, "Hidden clip").properties.clipped = true;
    for name in ["Hidden clip", "Hidden"] {
        layer_mut(&mut doc, name).visible = false;
    }
    layer_mut(&mut doc, "Top").properties.blend = LayerBlend::Multiply;
    assert_eq!(baked(&doc, MergeKind::Visible), ["Top", "Base", "Bottom"]);
    merged(&mut doc, MergeKind::Visible);
    assert_eq!(names(&doc), ["Hidden clip", "Hidden", "Bottom", "Paper"]);
    assert!(!doc.layer(id(&doc, "Hidden clip")).unwrap().properties.clipped);
    assert_eq!(doc.layer(LayerId(100)).unwrap().name.as_ref(), "Bottom");

    let mut doc = document(&["Clip", "Base"]);
    layer_mut(&mut doc, "Clip").properties.clipped = true;
    layer_mut(&mut doc, "Base").visible = false;
    assert_eq!(doc.merge_refusal(MergeKind::Visible), Some(MergeRefusal::NothingVisible), "a hidden base hides its clips");
    layer_mut(&mut doc, "Base").visible = true;
    layer_mut(&mut doc, "Clip").properties.locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Visible), Some(MergeRefusal::Locked));
    assert_eq!(doc.merge_refusal(MergeKind::Stamp), None, "stamping never changes the layers");
}

#[test]
fn flatten_discards_hidden_layers_and_keeps_paper_and_selection_layers() {
    let mut doc = document(&["Visible", "Hidden", "Group", "Hidden child", "Shown child", "Saved"]);
    nest(&mut doc, "Group", &["Hidden child", "Shown child"]);
    layer_mut(&mut doc, "Group").visible = false;
    layer_mut(&mut doc, "Hidden").visible = false;
    layer_mut(&mut doc, "Hidden child").visible = false;
    let saved = layer_mut(&mut doc, "Saved");
    saved.kind = LayerKind::Selection;
    saved.selection = Some(Selection::polygon(vec![
        Point { x: 0., y: 0. },
        Point { x: 10., y: 0. },
        Point { x: 10., y: 10. },
    ]).unwrap());
    assert_eq!(doc.flatten_discards(), 2, "a hidden group counts once");
    let before = doc.clone();
    let undo = merged(&mut doc, MergeKind::Flatten);
    assert_eq!(names(&doc), ["Visible", "Saved", "Paper"]);
    assert_eq!(doc.layer(LayerId(100)).unwrap().properties.extent, None);
    doc.apply(undo).unwrap();
    assert_eq!(doc.layers, before.layers);
    layer_mut(&mut doc, "Hidden").properties.locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Flatten), Some(MergeRefusal::Locked), "discarding needs unlocked layers");
    let mut empty = document(&["Hidden"]);
    layer_mut(&mut empty, "Hidden").visible = false;
    assert_eq!(empty.merge_refusal(MergeKind::Flatten), Some(MergeRefusal::NothingVisible));
}

#[test]
fn stamp_visible_adds_a_top_layer_and_keeps_every_member() {
    let mut doc = document(&["Top", "Hidden", "Bottom"]);
    layer_mut(&mut doc, "Hidden").visible = false;
    layer_mut(&mut doc, "Bottom").properties.locked = true;
    doc.reference_layers = [id(&doc, "Top")].into();
    activate(&mut doc, "Bottom");
    assert_eq!(baked(&doc, MergeKind::Stamp), ["Top", "Bottom"]);
    merged(&mut doc, MergeKind::Stamp);
    assert_eq!(names(&doc), ["Visible", "Top", "Hidden", "Bottom", "Paper"]);
    assert_eq!(doc.active_layer, LayerId(100));
    assert_eq!(doc.reference_layers, [id(&doc, "Top")].into());
}

#[test]
fn bakes_keep_pixels_outside_the_canvas_on_whole_pages() {
    let mut doc = document(&["Upper", "Lower"]);
    layer_mut(&mut doc, "Upper").properties.offset = Point { x: -100., y: 30. };
    layer_mut(&mut doc, "Lower").properties.extent = Some([900, 400]);
    let plan = doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap();
    let result = plan.edits.iter().find_map(|e| match e {
        Edit::InsertLayer { layer, .. } => Some(layer.clone()),
        _ => None,
    }).unwrap();
    assert_eq!(result.properties.offset, Point { x: -256., y: 0. });
    assert_eq!(result.properties.extent, Some([1156, 430]));
    let LayerOperationKind::Bake { members, offset } = &plan.operation.kind else { unreachable!() };
    assert_eq!(*offset, Point { x: 256., y: 0. });
    let layers = bake_layers(members, *offset);
    assert_eq!(layers[0].properties.offset, Point { x: 156., y: 30. });
    assert_eq!(layers[1].properties.offset, Point { x: 256., y: 0. });
    let mut after = doc.clone();
    merged(&mut after, MergeKind::Down);
    assert!(after.extents_cover_canvas());
    let flatten = doc.merge_plan(MergeKind::Flatten, LayerId(100), LayerId(101)).unwrap();
    assert!(
        flatten.edits.iter().any(|e| matches!(e, Edit::InsertLayer { layer, .. }
            if layer.properties.extent.is_none() && layer.properties.offset == Point::default())),
        "Flatten covers the canvas only"
    );
}

#[test]
fn bake_bounds_follow_content_photos_and_effects() {
    let descriptor = raster::RasterPlane::Color.descriptor(Default::default());
    let tiles = |coordinates: &[[u32; 2]]| {
        raster::RasterRevision::backed(raster::RasterData {
            tiles: coordinates
                .iter()
                .map(|&coordinate| {
                    (raster::TileKey { plane: raster::RasterPlane::Color, coordinate }, raster::RasterTile::pending(descriptor))
                })
                .collect(),
            watercolor: None,
        })
    };
    let mut upper = Layer::paint(LayerId(5), "Upper");
    upper.raster = tiles(&[[1, 0]]);
    upper.properties.offset = Point { x: 10., y: 20. };
    let lower = Layer::paint(LayerId(6), "Lower");
    let extent = [1024, 768];
    let bounds = |members: &[Layer]| LayerOperation::bounds(&LayerOperation {
        placement: Affine::IDENTITY,
        coverage: LayerMask::reveal_all(LayerId(9), Point::default()),
        kind: LayerOperationKind::Bake { members: members.into(), offset: Point { x: 100., y: 0. } },
    }, extent);
    assert!(bounds(std::slice::from_ref(&lower)).is_empty(), "an empty layer bakes nothing");
    let expected = Rect { min: Point { x: 366., y: 20. }, max: Point { x: 622., y: 276. } };
    assert_eq!(bounds(&[upper.clone(), lower.clone()]), expected);
    let mut hidden = upper.clone();
    hidden.visible = false;
    assert!(bounds(&[hidden]).is_empty());
    let mut pending = lower.clone();
    pending.raster = raster::RasterRevision::pending();
    assert_eq!(bounds(&[pending]), Rect { min: Point { x: 100., y: 0. }, max: Point { x: 1024., y: 768. } });
    let program = |id: &str| Some(Arc::new(EffectInstance::new(crate::bundled_effect_catalog().get(id).unwrap().program())));
    let mut levels = Layer { kind: LayerKind::Effect, ..Layer::paint(LayerId(7), "Levels") };
    levels.effect = program("levels");
    assert_eq!(bounds(&[levels.clone(), upper.clone()]), expected, "adjustments keep coverage");
    let mut blur = levels.clone();
    blur.effect = program("gaussian_blur");
    assert_eq!(bounds(&[blur, upper.clone()]), expected.outset(18.));
    let mut fill = levels;
    fill.effect = program("solid_color");
    assert_eq!(bounds(&[fill, upper]), Rect::from_extent(extent));
}

#[test]
fn bakes_above_the_publication_limit_are_refused_and_bakes_need_history_room() {
    let mut doc = document(&["Upper", "Lower"]);
    doc.width = 16384;
    doc.height = 16384;
    doc.color.depth = color::SampleDepth::F32;
    for name in ["Upper", "Lower"] {
        layer_mut(&mut doc, name).raster = raster::RasterRevision::pending();
    }
    assert_eq!(doc.merge_refusal(MergeKind::Down), None);
    assert_eq!(doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap_err(), MergeRefusal::TooLarge);
    let doc = document(&["Upper", "Lower"]);
    let plan = doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap();
    let mut insert = plan.edits.iter().find(|e| matches!(e, Edit::InsertLayer { .. })).unwrap().clone();
    assert!(!insert.requires_history_admission(&doc));
    if let Edit::InsertLayer { layer, .. } = &mut insert {
        layer.pending_operations.push(plan.operation.clone());
    }
    assert!(insert.requires_history_admission(&doc));
}

#[test]
fn a_merge_that_undo_history_cannot_hold_is_refused_whole() {
    let mut doc = document(&["Upper", "Lower"]);
    doc.width = 8192;
    doc.height = 8192;
    doc.color.depth = color::SampleDepth::F32;
    let descriptor = raster::RasterPlane::Color.descriptor(doc.color);
    layer_mut(&mut doc, "Lower").raster = raster::RasterRevision::backed(raster::RasterData {
        tiles: (0..32u32)
            .flat_map(|x| (0..20u32).map(move |y| [x, y]))
            .map(|coordinate| {
                (raster::TileKey { plane: raster::RasterPlane::Color, coordinate }, raster::RasterTile::pending(descriptor))
            })
            .collect(),
        watercolor: None,
    });
    let mut editor = Editor::new(doc.clone());
    let plan = doc.merge_plan(MergeKind::Down, LayerId(100), LayerId(101)).unwrap();
    let mut edits = plan.edits;
    for edit in &mut edits {
        if let Edit::InsertLayer { layer, .. } = edit {
            layer.raster = raster::RasterRevision::pending();
            layer.pending_operations.push(plan.operation.clone());
        }
    }
    assert_eq!(
        editor.perform(Edit::Batch(edits)),
        Err(DocumentError::InvalidLayerOperation("This edit exceeds the Undo/Redo memory limit"))
    );
    assert_eq!(editor.document().layers, doc.layers, "nothing changes");
}
