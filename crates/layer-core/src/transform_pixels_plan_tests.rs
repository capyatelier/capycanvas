fn transformed_document(linked: bool) -> Document {
    let mut doc = fixture::document([128, 96], &["Ink"]);
    let owner = fixture::id(&doc, "Ink");
    let color = doc.composition().color;
    let p = fixture::paint_mut(&mut doc, "Ink");
    p.base = Some(PaintBase::new(Arc::new(photo_source([24, 12])).into()));
    p.domain = [768, 512];
    let mut data = raster(RasterPlane::Color, color, &[[0, 0], [1, 0]]).wait_data().unwrap().as_ref().clone();
    for plane in [RasterPlane::WatercolorWetness] {
        data.tiles.insert(TileKey { plane, coordinate: [1, 0] }, tile(plane, color, 7));
    }
    fixture::paint_mut(&mut doc, "Ink").raster = RasterRevision::backed(data);
    fixture::occurrence_mut(&mut doc, "Ink").offset = [-40, 10];
    let h = fixture::add_mask(&mut doc, owner, [768, 512], [30, -25]);
    doc.artwork.occurrences.get_mut(owner).unwrap().mask.as_mut().unwrap().linked = linked;
    let c = doc.artwork.coverage.get_mut(h).unwrap();
    c.raster = raster(RasterPlane::Mask, color, &[[0, 0], [2, 1]]);
    doc
}
fn stretch() -> LayerPlacement { LayerPlacement::from_affine(Affine([2., 0., 0., 3., 0., 0.])) }
#[test]
fn layer_transform_plan_needs_a_perspective_valid_only_over_the_content_it_moves() {
    let horizon = LayerPlacement::from_projective(Projective([1., 0., 0., 0., 1., 0., -1. / 300., 0., 1.]));
    let stroke = Rect { min: Point { x: 100., y: 20. }, max: Point { x: 160., y: 60. } };
    for linked in [false, true] {
        let doc = transformed_document(linked);
        let target = fixture::target(&doc, "Ink");
        assert!(doc.layer_transform_plan(target, &horizon, None, Default::default()).is_err(), "the whole layer reaches the horizon");
        let plan = doc.layer_transform_plan(target, &horizon, Some(stroke), Default::default()).unwrap();
        for corner in stroke.corners() {
            let placed = plan.geometry.map(corner).unwrap();
            assert!(placed.x >= 0. && placed.y >= 0. && placed.x <= plan.extent[0] as f32 && placed.y <= plan.extent[1] as f32,
                "{corner:?} lands at {placed:?} inside {:?}", plan.extent);
        }
    }
}
#[test]
fn layer_transform_plan_freezes_raw_planes_and_preserves_unlinked_mask_identity() {
    for linked in [false, true] {
        let doc = transformed_document(linked);
        let before = doc.clone();
        let target = fixture::target(&doc, "Ink");
        let owner = fixture::id(&doc, "Ink");
        let old = fixture::occurrence(&doc, "Ink");
        let mask = old.mask.as_ref().unwrap();
        let plan = doc.layer_transform_plan(target, &stretch(), None, Default::default()).unwrap();
        assert_eq!(doc, before);
        assert_eq!(matches!(plan.scope, TransformPixelsScope::Paint { linked_mask: true }), linked);
        assert_eq!(plan.geometry.placement.interpolation, Interpolation::Linear);
        let frozen = plan.scene.view().paint_source(owner).unwrap();
        assert!(Arc::ptr_eq(frozen.base.as_ref().unwrap().image.storage(), fixture::paint(&doc, "Ink").base.as_ref().unwrap().image.storage()));
        assert_eq!(frozen.raster, fixture::paint(&doc, "Ink").raster);
        assert_eq!(plan.scene.artwork.coverage.get(mask.source).unwrap().raster, doc.artwork.coverage.get(mask.source).unwrap().raster);
        let mut after = doc.clone();
        after.apply(plan.output.clone()).unwrap();
        let output = fixture::occurrence(&after, "Ink");
        let p = fixture::paint(&after, "Ink");
        assert_eq!(output.offset, plan.origin);
        assert!(p.base.is_none());
        assert!(p.raster.wait_data().unwrap().tiles.is_empty());
        for point in [Point { x: 0., y: 0. }, Point { x: 7., y: 5. }] {
            let placed = plan.geometry.map(point).unwrap();
            let expected = Affine([2., 0., 0., 3., (-40 - plan.origin[0]) as f32, (10 - plan.origin[1]) as f32]).map(point);
            assert!((placed.x - expected.x).abs() < 0.001 && (placed.y - expected.y).abs() < 0.001);
        }
        if linked {
            let mask = output.mask.as_ref().unwrap();
            let c = after.artwork.coverage.get(mask.source).unwrap();
            assert_eq!(mask.offset, [0, 0]);
            assert!(c.raster.wait_data().unwrap().tiles.is_empty());
        } else {
            assert_eq!(output.mask, old.mask);
            assert_eq!(after.artwork.coverage.get(mask.source), doc.artwork.coverage.get(mask.source));
        }
        assert_eq!(plan.scene.artwork, doc.artwork);
    }
}
#[test]
fn layer_transform_plan_admission_failure_never_changes_source_raster_or_mask() {
    let doc = transformed_document(true);
    let before = doc.clone();
    for limit in [
        ProjectLimits { dimension: 32, ..Default::default() },
        ProjectLimits { tiles: 1, ..Default::default() },
        ProjectLimits { raster_bytes: 1, ..Default::default() },
    ] {
        assert!(doc.layer_transform_plan(fixture::target(&doc, "Ink"), &stretch(), None, limit).is_err());
        assert_eq!(doc, before);
    }
}
#[test]
fn layer_transform_plan_refuses_locked_pending_and_nonpaint_targets() {
    let mut doc = transformed_document(true);
    let target = fixture::target(&doc, "Ink");
    fixture::occurrence_mut(&mut doc, "Ink").locked = true;
    assert!(doc.layer_transform_plan(target, &stretch(), None, Default::default()).is_err());
    assert_eq!(doc.layer_transform_refusal(SourceTarget::Paint(PaintHandle::INVALID)), Some(TransformPixelsRefusal::Target));
    fixture::occurrence_mut(&mut doc, "Ink").locked = false;
    let coverage = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), [768, 512], [0; 2]);
    Arc::make_mut(&mut fixture::paint_mut(&mut doc, "Ink").operations).push(RasterOperation { placement: Affine::IDENTITY, coverage, kind: RasterOperationKind::Erase { alpha_locked: false } });
    assert_eq!(doc.layer_transform_refusal(target), Some(TransformPixelsRefusal::Pending));
    let fill = fixture::id(&doc, "Paper");
    assert!(doc.scene().effect(fill).is_some_and(|effect| effect.constant_color().is_some()));
    assert_eq!(doc.scene().source_target(fill), None);
}
#[test]
fn layer_transform_plan_keeps_nested_color_and_linked_mask_in_one_world_frame() {
    let mut doc = transformed_document(true);
    let owner = fixture::id(&doc, "Ink");
    let target = fixture::target(&doc, "Ink");
    let mask = SourceTarget::Coverage(fixture::occurrence(&doc, "Ink").mask.as_ref().unwrap().source);
    fixture::insert_paint(&mut doc, "Group", 0, None);
    let group = fixture::nest(&mut doc, "Group", &["Ink"]);
    fixture::occurrence_mut(&mut doc, "Group").offset = [25, 35];
    fixture::occurrence_mut(&mut doc, "Ink").offset = [7, 9];
    let before = doc.clone();
    let plan = doc.layer_transform_plan(target, &stretch(), None, Default::default()).unwrap();
    let mut after = doc.clone();
    after.apply(plan.output.clone()).unwrap();
    assert_eq!(after.scene().layer_origin(Some(owner)), plan.origin);
    assert_eq!(after.scene().target_origin(mask), after.scene().target_origin(target));
    assert_eq!(after.scene().occurrence(owner).unwrap().offset, offsets::checked_sub(plan.origin, [25, 35]).unwrap());
    assert_eq!(after.scene().parent(owner), Some(group));
    assert_eq!(plan.scene.view().parent(owner), Some(group));
    assert_eq!(doc, before);
}
#[test]
fn scalar_mask_plan_keeps_owner_paint_and_moves_only_the_mask_origin() {
    for linked in [false, true] {
        let mut doc = transformed_document(linked);
        fixture::paint_mut(&mut doc, "Ink").raster = Default::default();
        let mask = fixture::occurrence(&doc, "Ink").mask.clone().unwrap();
        let target = SourceTarget::Coverage(mask.source);
        let use_ = fixture::occurrence_mut(&mut doc, "Ink").mask.as_mut().unwrap();
        use_.inverted = true;
        use_.enabled = false;
        let original = fixture::occurrence(&doc, "Ink").clone();
        let plan = doc.layer_transform_plan(target, &stretch(), None, Default::default()).unwrap();
        assert_eq!(plan.scope, TransformPixelsScope::Mask);
        let mut after = doc.clone();
        after.apply(plan.output.clone()).unwrap();
        let output = fixture::occurrence(&after, "Ink");
        assert_eq!(output.offset, original.offset);
        assert!(output.mask.as_ref().unwrap().inverted && !output.mask.as_ref().unwrap().enabled);
        assert_eq!(after.scene().target_origin(target), plan.origin);
        assert_eq!(fixture::paint(&after, "Ink"), fixture::paint(&doc, "Ink"));
    }
}
