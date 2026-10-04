use super::*;
use crate::operation_test_support as fixture;
use fixture::*;
fn document(names: &[&str]) -> Document {
    fixture::document([600, 400], names)
}
fn baked(doc: &Document, kind: MergeKind) -> Vec<String> {
    let plan = doc.merge_plan(kind).unwrap();
    let RasterOperationKind::Bake { scene, scope, .. } = &plan.operation.kind else { panic!("Bake") };
    let SceneScope::Members(members) = scope else { panic!("Members") };
    members.iter().map(|h| scene.view().occurrence(*h).unwrap().name.to_string()).collect()
}
fn merged(doc: &mut Document, kind: MergeKind) -> Edit {
    let plan = doc.merge_plan(kind).unwrap();
    let mut edits = plan.edits;
    for edit in &mut edits {
        if let Edit::Paint(change) = edit {
            if Some(SourceTarget::Paint(change.handle)) == Some(plan.target) {
                Arc::make_mut(&mut change.value.as_mut().unwrap().operations).push(plan.operation.clone());
            }
        }
    }
    doc.apply(Edit::Batch(edits)).unwrap()
}
fn references(doc: &Document) -> BTreeSet<OccurrenceHandle> {
    doc.scene().references()
}
#[test]
fn merge_down_replaces_both_layers_in_one_reversible_edit_and_transfers_references() {
    let mut doc = document(&["Top", "Upper", "Lower", "Bottom"]);
    activate(&mut doc, "Upper");
    occurrence_mut(&mut doc, "Upper").opacity = 0.4;
    occurrence_mut(&mut doc, "Lower").alpha_locked = true;
    for name in ["Upper", "Bottom"] {
        occurrence_mut(&mut doc, name).reference = true;
    }
    let before = doc.clone();
    assert_eq!(baked(&doc, MergeKind::Down), ["Upper", "Lower"]);
    let undo = merged(&mut doc, MergeKind::Down);
    assert_eq!(names(&doc), ["Top", "Lower", "Bottom", "Paper"]);
    let result = doc.working.occurrence.unwrap();
    let o = doc.scene().occurrence(result).unwrap();
    assert_eq!((o.opacity, o.blend, o.mask.is_none()), (1., LayerBlend::Normal, true));
    assert!(o.alpha_locked);
    assert_eq!(doc.scene().local_extent(result), doc.composition().size);
    assert_eq!(references(&doc), [result, id(&before, "Bottom")].into());
    doc.apply(undo).unwrap();
    restored(&before, &doc);
    assert_eq!(references(&doc), references(&before));
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
    assert_eq!(
        refusal(&|d| {
            let h = id(d, "Lower");
            let root = d.composition().result;
            d.artwork.stacks.get_mut(root).unwrap().entries.retain(|v| *v != h);
            refresh(d);
        }),
        None
    );
    assert_eq!(refusal(&|d| activate(d, "Lower")), None);
    assert_eq!(refusal(&|d| activate(d, "Paper")), Some(R::NoLayerBelow));
    assert_eq!(refusal(&|d| d.working.occurrence = Some(OccurrenceHandle::INVALID)), Some(R::NoLayer));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Upper").visible = false), Some(R::Hidden));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Upper").blend = LayerBlend::Multiply), Some(R::NotNormal));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Upper").locked = true), Some(R::Locked));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Lower").visible = false), Some(R::BelowHidden));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Lower").locked = true), Some(R::BelowLocked));
    assert_eq!(refusal(&|d| occurrence_mut(d, "Lower").blend = LayerBlend::Screen), Some(R::BelowNotNormal));
    assert_eq!(refusal(&|d| effect(d, "Lower", "levels")), Some(R::BelowEffect));
    assert_eq!(refusal(&|d| saved(d, "Upper", Selection::empty())), Some(R::SelectionLayer));
    let mut doc = document(&["Upper", "Lower"]);
    insert_paint(&mut doc, "Group", 0, None);
    nest(&mut doc, "Group", &["Upper"]);
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(R::NoLayerBelow));
    occurrence_mut(&mut doc, "Group").locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(R::Locked));
}
#[test]
fn clipping_bases_bake_their_visible_stack_and_keep_hidden_clips() {
    let mut doc = document(&["Hidden clip", "Shade", "Base", "Below"]);
    for name in ["Hidden clip", "Shade"] {
        occurrence_mut(&mut doc, name).clipped = true;
    }
    occurrence_mut(&mut doc, "Hidden clip").visible = false;
    occurrence_mut(&mut doc, "Shade").blend = LayerBlend::Multiply;
    activate(&mut doc, "Base");
    assert_eq!(doc.merge_down(), MergeDown::ClippingStack);
    assert_eq!(baked(&doc, MergeKind::Down), ["Shade", "Base"]);
    merged(&mut doc, MergeKind::Down);
    assert_eq!(names(&doc), ["Hidden clip", "Base", "Below", "Paper"]);
    let result = doc.working.occurrence.unwrap();
    assert_eq!(doc.clipping_base(id(&doc, "Hidden clip")), Some(result));
    assert!(!doc.scene().occurrence(result).unwrap().clipped);
    let mut doc = document(&["Shade", "Base"]);
    occurrence_mut(&mut doc, "Shade").clipped = true;
    occurrence_mut(&mut doc, "Shade").visible = false;
    activate(&mut doc, "Base");
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::ClipsHidden));
    occurrence_mut(&mut doc, "Shade").visible = true;
    occurrence_mut(&mut doc, "Base").blend = LayerBlend::Multiply;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::NotNormal));
}
#[test]
fn clipped_layers_merge_within_their_stack() {
    let mut doc = document(&["Upper", "Lower", "Base", "Free"]);
    for name in ["Upper", "Lower"] {
        occurrence_mut(&mut doc, name).clipped = true;
    }
    assert_eq!(doc.merge_down(), MergeDown::Layer);
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let RasterOperationKind::Bake { scene, scope, .. } = &plan.operation.kind else { panic!("Bake") };
    let SceneScope::Members(members) = scope else { panic!("members") };
    assert_eq!(members.iter().map(|h| scene.view().occurrence(*h).unwrap().name.as_ref()).collect::<Vec<_>>(), ["Upper", "Lower"]);
    assert!(members.iter().all(|h| !scene.view().with_scope(scope).effective_clipped(*h)));
    assert!(!members.contains(&id(&doc, "Base")));
    merged(&mut doc, MergeKind::Down);
    let result = doc.working.occurrence.unwrap();
    assert!(doc.scene().occurrence(result).unwrap().clipped);
    assert_eq!(doc.clipping_base(result), Some(id(&doc, "Base")));
    let mut doc = document(&["Upper", "Base", "Other"]);
    occurrence_mut(&mut doc, "Upper").clipped = true;
    assert_eq!(baked(&doc, MergeKind::Down), ["Upper", "Base"]);
    let mut doc = document(&["Upper", "Clip", "Base"]);
    occurrence_mut(&mut doc, "Clip").clipped = true;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::BelowClipped));
}
#[test]
fn effects_apply_to_the_layer_below_or_bake_their_clipping_stack() {
    let mut doc = document(&["Levels", "Photo", "Backdrop"]);
    effect(&mut doc, "Levels", "levels");
    assert_eq!(doc.merge_down(), MergeDown::ApplyEffect);
    assert_eq!(baked(&doc, MergeKind::Down), ["Levels", "Photo"]);
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let source = plan
        .edits
        .iter()
        .find_map(|e| match e {
            Edit::Paint(c) => c.value.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(source.domain, doc.composition().size);
    let mut doc = document(&["Levels", "Shade", "Base", "Below"]);
    effect(&mut doc, "Levels", "levels");
    for name in ["Levels", "Shade"] {
        occurrence_mut(&mut doc, name).clipped = true;
    }
    assert_eq!(doc.merge_down(), MergeDown::ClippingStack);
    assert_eq!(baked(&doc, MergeKind::Down), ["Levels", "Shade", "Base"]);
    occurrence_mut(&mut doc, "Base").visible = false;
    assert_eq!(doc.merge_refusal(MergeKind::Down), Some(MergeRefusal::BaseHidden));
}
#[test]
fn merge_group_keeps_the_groups_blend_and_opacity_and_bakes_its_mask() {
    let mut doc = document(&["Group", "Inside", "Hidden", "Outside"]);
    let group = nest(&mut doc, "Group", &["Inside", "Hidden"]);
    occurrence_mut(&mut doc, "Hidden").visible = false;
    add_mask(&mut doc, group, [600, 400], Point::default());
    occurrence_mut(&mut doc, "Group").opacity = 0.5;
    occurrence_mut(&mut doc, "Group").blend = LayerBlend::Screen;
    assert_eq!(doc.merge_refusal(MergeKind::Group), None);
    let plan = doc.merge_plan(MergeKind::Group).unwrap();
    let RasterOperationKind::Bake { scene, .. } = &plan.operation.kind else { panic!("Bake") };
    let baked = scene.view().occurrence(group).unwrap();
    assert_eq!((baked.opacity, baked.blend, baked.mask.is_some()), (1., LayerBlend::Normal, true));
    merged(&mut doc, MergeKind::Group);
    assert_eq!(names(&doc), ["Group", "Outside", "Paper"]);
    let result = occurrence(&doc, "Group");
    assert_eq!((result.kind(), result.opacity, result.blend), (LayerKind::Paint, 0.5, LayerBlend::Screen));
    activate(&mut doc, "Outside");
    assert_eq!(doc.merge_refusal(MergeKind::Group), Some(MergeRefusal::NotGroup));
    let mut doc = document(&["Group", "Inside", "Outside"]);
    let group = nest(&mut doc, "Group", &["Inside"]);
    occurrence_mut(&mut doc, "Group").blend = LayerBlend::PassThrough;
    occurrence_mut(&mut doc, "Group").opacity = 0.5;
    activate(&mut doc, "Group");
    let plan = doc.merge_plan(MergeKind::Group).unwrap();
    let RasterOperationKind::Bake { scene, .. } = &plan.operation.kind else { panic!("Bake") };
    assert_eq!(scene.view().occurrence(group).unwrap().blend, LayerBlend::Normal);
    merged(&mut doc, MergeKind::Group);
    let result = occurrence(&doc, "Group");
    assert_eq!((result.opacity, result.blend), (0.5, LayerBlend::Normal));
    let mut doc = document(&["Group", "Selection"]);
    nest(&mut doc, "Group", &["Selection"]);
    saved(&mut doc, "Selection", Selection::empty());
    assert_eq!(doc.merge_refusal(MergeKind::Group), Some(MergeRefusal::SelectionLayersInside));
}
#[test]
fn merge_visible_keeps_hidden_layers_and_releases_their_clipping() {
    let mut doc = document(&["Top", "Hidden clip", "Base", "Hidden", "Bottom"]);
    occurrence_mut(&mut doc, "Hidden clip").clipped = true;
    for name in ["Hidden clip", "Hidden"] {
        occurrence_mut(&mut doc, name).visible = false;
    }
    occurrence_mut(&mut doc, "Top").blend = LayerBlend::Multiply;
    assert_eq!(baked(&doc, MergeKind::Visible), ["Top", "Base", "Bottom", "Paper"]);
    merged(&mut doc, MergeKind::Visible);
    assert_eq!(names(&doc), ["Hidden clip", "Hidden", "Paper"]);
    assert!(!occurrence(&doc, "Hidden clip").clipped);
    assert_eq!(doc.scene().occurrence(doc.working.occurrence.unwrap()).unwrap().name.as_ref(), "Paper");
    let mut doc = document(&["Clip", "Base"]);
    occurrence_mut(&mut doc, "Clip").clipped = true;
    occurrence_mut(&mut doc, "Base").visible = false;
    occurrence_mut(&mut doc, "Paper").visible = false;
    assert_eq!(doc.merge_refusal(MergeKind::Visible), Some(MergeRefusal::NothingVisible), "a hidden base hides its clips");
    occurrence_mut(&mut doc, "Base").visible = true;
    occurrence_mut(&mut doc, "Clip").locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Visible), Some(MergeRefusal::Locked));
    assert_eq!(doc.merge_refusal(MergeKind::Stamp), None);
}
#[test]
fn flatten_bakes_fill_layers_and_keeps_selection_layers() {
    let mut doc = document(&["Visible", "Hidden", "Group", "Hidden child", "Shown child", "Saved"]);
    nest(&mut doc, "Group", &["Hidden child", "Shown child"]);
    for name in ["Group", "Hidden", "Hidden child"] {
        occurrence_mut(&mut doc, name).visible = false;
    }
    saved(&mut doc, "Saved", Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: 10., y: 0. }, Point { x: 10., y: 10. }]).unwrap());
    assert_eq!(doc.flatten_discards(), 2);
    let before = doc.clone();
    let undo = merged(&mut doc, MergeKind::Flatten);
    assert_eq!(names(&doc), ["Saved", "Paper"]);
    assert_eq!(doc.target_extent(doc.working.target.unwrap()), doc.composition().size);
    doc.apply(undo).unwrap();
    restored(&before, &doc);
    occurrence_mut(&mut doc, "Hidden").locked = true;
    assert_eq!(doc.merge_refusal(MergeKind::Flatten), Some(MergeRefusal::Locked));
    let mut empty = document(&["Hidden"]);
    occurrence_mut(&mut empty, "Hidden").visible = false;
    occurrence_mut(&mut empty, "Paper").visible = false;
    assert_eq!(empty.merge_refusal(MergeKind::Flatten), Some(MergeRefusal::NothingVisible));
}
#[test]
fn stamp_visible_adds_a_top_layer_and_keeps_every_member() {
    let mut doc = document(&["Top", "Hidden", "Bottom"]);
    occurrence_mut(&mut doc, "Hidden").visible = false;
    occurrence_mut(&mut doc, "Bottom").locked = true;
    occurrence_mut(&mut doc, "Top").reference = true;
    activate(&mut doc, "Bottom");
    assert_eq!(baked(&doc, MergeKind::Stamp), ["Top", "Bottom", "Paper"]);
    merged(&mut doc, MergeKind::Stamp);
    assert_eq!(names(&doc), ["Visible", "Top", "Hidden", "Bottom", "Paper"]);
    assert_eq!(doc.working.occurrence, Some(id(&doc, "Visible")));
    assert_eq!(references(&doc), [id(&doc, "Top")].into());
}
#[test]
fn bakes_keep_pixels_outside_the_canvas_on_whole_pages() {
    let mut doc = document(&["Upper", "Lower"]);
    occurrence_mut(&mut doc, "Upper").translation = Point { x: -100., y: 30. };
    paint_mut(&mut doc, "Lower").domain = [900, 400];
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let result = plan
        .edits
        .iter()
        .find_map(|e| match e {
            Edit::Occurrence(c) if c.handle == plan.result => c.value.as_ref(),
            _ => None,
        })
        .unwrap();
    let p = plan
        .edits
        .iter()
        .find_map(|e| match e {
            Edit::Paint(c) => c.value.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.translation, Point { x: -256., y: 0. });
    assert_eq!(p.domain, [1156, 430]);
    let RasterOperationKind::Bake { scene, offset, .. } = &plan.operation.kind else { panic!("Bake") };
    assert_eq!(*offset, Point { x: 256., y: 0. });
    assert_eq!(Affine::translation(*offset).map(scene.view().target_offset(target(&doc, "Upper"))), Point { x: 156., y: 30. });
    assert_eq!(Affine::translation(*offset).map(scene.view().target_offset(target(&doc, "Lower"))), Point { x: 256., y: 0. });
    let mut after = doc.clone();
    merged(&mut after, MergeKind::Down);
    assert!(after.extents_cover_canvas());
    let flatten = doc.merge_plan(MergeKind::Flatten).unwrap();
    assert!(flatten.edits.iter().any(|e| matches!(e,Edit::Paint(c) if c.value.as_ref().is_some_and(|p|p.domain==doc.composition().size))));
    assert!(flatten.edits.iter().any(
        |e| matches!(e,Edit::Occurrence(c) if c.handle==flatten.result&&c.value.as_ref().is_some_and(|o|o.translation==Point::default()))
    ));
}
#[test]
fn bake_bounds_follow_content_photos_and_effects() {
    let extent = [1024, 768];
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
    let bounds = |doc: &Document, members: &[&str]| {
        let scope = SceneScope::Members(members.iter().map(|n| id(doc, n)).collect::<Vec<_>>().into());
        RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), extent, Point::default()),
            kind: RasterOperationKind::Bake { scene: doc.snapshot(), scope, offset: Point { x: 100., y: 0. } },
        }
        .bounds(extent)
    };
    let mut doc = fixture::document(extent, &["Upper", "Lower"]);
    paint_mut(&mut doc, "Upper").raster = tiles(&[[1, 0]]);
    occurrence_mut(&mut doc, "Upper").translation = Point { x: 10., y: 20. };
    assert!(bounds(&doc, &["Lower"]).is_empty());
    let expected = Rect { min: Point { x: 366., y: 20. }, max: Point { x: 622., y: 276. } };
    assert_eq!(bounds(&doc, &["Upper", "Lower"]), expected);
    occurrence_mut(&mut doc, "Upper").visible = false;
    assert!(bounds(&doc, &["Upper"]).is_empty());
    occurrence_mut(&mut doc, "Upper").visible = true;
    paint_mut(&mut doc, "Lower").raster = raster::RasterRevision::pending();
    assert_eq!(bounds(&doc, &["Lower"]), Rect { min: Point { x: 100., y: 0. }, max: Point { x: 1024., y: 768. } });
    insert_paint(&mut doc, "Levels", 0, None);
    effect(&mut doc, "Levels", "levels");
    assert_eq!(bounds(&doc, &["Levels", "Upper"]), expected);
    effect(&mut doc, "Levels", "gaussian_blur");
    assert_eq!(bounds(&doc, &["Levels", "Upper"]), expected.outset(18.));
    effect(&mut doc, "Levels", "solid_color");
    assert_eq!(bounds(&doc, &["Levels", "Upper"]), Rect::from_extent(extent));
}
#[test]
fn bakes_above_the_publication_limit_are_refused_and_bakes_need_history_room() {
    let mut doc = document(&["Upper", "Lower"]);
    let composition = doc.artwork.compositions.get_mut(doc.artwork.root).unwrap();
    composition.size = [16384; 2];
    composition.color.depth = color::SampleDepth::F32;
    for name in ["Upper", "Lower"] {
        let p = paint_mut(&mut doc, name);
        p.domain = [16384; 2];
        p.raster = raster::RasterRevision::pending();
    }
    assert_eq!(doc.merge_refusal(MergeKind::Down), None);
    assert_eq!(doc.merge_plan(MergeKind::Down).unwrap_err(), MergeRefusal::TooLarge);
    let doc = document(&["Upper", "Lower"]);
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let mut insert = plan.edits.iter().find(|e| matches!(e,Edit::Paint(c) if c.value.is_some())).unwrap().clone();
    assert!(!insert.requires_history_admission(&doc));
    if let Edit::Paint(c) = &mut insert {
        Arc::make_mut(&mut c.value.as_mut().unwrap().operations).push(plan.operation);
    }
    assert!(insert.requires_history_admission(&doc));
}
#[test]
fn a_merge_that_undo_history_cannot_hold_is_refused_whole() {
    let mut doc = document(&["Upper", "Lower"]);
    let composition = doc.artwork.compositions.get_mut(doc.artwork.root).unwrap();
    composition.size = [8192; 2];
    composition.color.depth = color::SampleDepth::F32;
    let descriptor = raster::RasterPlane::Color.descriptor(doc.composition().color);
    paint_mut(&mut doc, "Lower").domain = [8192; 2];
    paint_mut(&mut doc, "Lower").raster = raster::RasterRevision::backed(raster::RasterData {
        tiles: (0..32u32)
            .flat_map(|x| (0..20u32).map(move |y| [x, y]))
            .map(|coordinate| (raster::TileKey { plane: raster::RasterPlane::Color, coordinate }, raster::RasterTile::pending(descriptor)))
            .collect(),
        watercolor: None,
    });
    let mut editor = Editor::new(doc.clone());
    let plan = doc.merge_plan(MergeKind::Down).unwrap();
    let mut edits = plan.edits;
    for edit in &mut edits {
        if let Edit::Paint(c) = edit {
            if c.value.is_some() {
                let p = c.value.as_mut().unwrap();
                p.raster = raster::RasterRevision::pending();
                Arc::make_mut(&mut p.operations).push(plan.operation.clone());
            }
        }
    }
    assert_eq!(
        editor.perform(Edit::Batch(edits)),
        Err(DocumentError::InvalidLayerOperation("This edit exceeds the Undo/Redo memory limit"))
    );
    assert_eq!(editor.document(), &doc);
}
