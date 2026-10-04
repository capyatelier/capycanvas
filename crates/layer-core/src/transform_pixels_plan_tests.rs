fn transformed_document(linked: bool) -> Document {
    let mut doc = fixture::document([128, 96], &["Ink"]);
    let owner = fixture::id(&doc, "Ink");
    let color = doc.composition().color;
    let p = fixture::paint_mut(&mut doc, "Ink");
    p.original = Some(Arc::new(photo_source([24, 12])));
    p.domain = [768, 512];
    let mut data = raster(RasterPlane::Color, color, &[[0, 0], [1, 0]]).wait_data().unwrap().as_ref().clone();
    for plane in [RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
        data.tiles.insert(TileKey { plane, coordinate: [1, 0] }, tile(plane, color, 7));
    }
    fixture::paint_mut(&mut doc, "Ink").raster = RasterRevision::backed(data);
    fixture::occurrence_mut(&mut doc, "Ink").placement = LayerPlacement::from_affine(Affine([2., 0., 0., 3., -40., 10.]));
    let h = fixture::add_mask(&mut doc, owner, [768, 512], Point { x: 30., y: -25. });
    let mask = doc.artwork.occurrences.get_mut(owner).unwrap().mask.as_mut().unwrap();
    mask.linked = linked;
    mask.placement = Projective::from_affine(Affine([1., 0., 0., 2., 7., -20.]));
    let c = doc.artwork.coverage.get_mut(h).unwrap();
    c.raster = raster(RasterPlane::Mask, color, &[[0, 0], [2, 1]]);
    c.initial = Some(
        Selection::polygon(vec![Point { x: 5., y: 7. }, Point { x: 13., y: 7. }, Point { x: 13., y: 17. }, Point { x: 5., y: 17. }])
            .unwrap(),
    );
    doc
}
#[test]
fn transform_pixels_plan_freezes_raw_planes_and_preserves_unlinked_mask_identity() {
    for linked in [false, true] {
        let doc = transformed_document(linked);
        let before = doc.clone();
        let target = fixture::target(&doc, "Ink");
        let owner = fixture::id(&doc, "Ink");
        let old = fixture::occurrence(&doc, "Ink");
        let mask = old.mask.as_ref().unwrap();
        let plan = doc.transform_pixels_plan(target, Interpolation::Linear, Default::default()).unwrap();
        assert_eq!(doc, before);
        assert_eq!(matches!(plan.scope, TransformPixelsScope::Paint { linked_mask: true }), linked);
        assert_eq!(plan.geometry.placement.interpolation, Interpolation::Linear);
        let frozen = plan.scene.view().paint_source(owner).unwrap();
        assert!(Arc::ptr_eq(frozen.original.as_ref().unwrap(), fixture::paint(&doc, "Ink").original.as_ref().unwrap()));
        assert_eq!(frozen.raster, fixture::paint(&doc, "Ink").raster);
        assert_eq!(plan.scene.artwork.coverage.get(mask.source).unwrap().raster, doc.artwork.coverage.get(mask.source).unwrap().raster);
        let mut after = doc.clone();
        after.apply(plan.output.clone()).unwrap();
        let output = fixture::occurrence(&after, "Ink");
        let p = fixture::paint(&after, "Ink");
        assert_eq!(output.placement, LayerPlacement::IDENTITY);
        assert!(p.original.is_none());
        assert!(p.raster.wait_data().unwrap().tiles.is_empty());
        if linked {
            let mask = output.mask.as_ref().unwrap();
            let c = after.artwork.coverage.get(mask.source).unwrap();
            assert_eq!(mask.placement, Projective::IDENTITY);
            assert_eq!(mask.translation, output.translation);
            assert!(c.initial.is_none());
            assert!(c.raster.wait_data().unwrap().tiles.is_empty());
        } else {
            assert_eq!(output.mask, old.mask);
            assert_eq!(after.artwork.coverage.get(mask.source), doc.artwork.coverage.get(mask.source));
        }
        assert_eq!(plan.scene.artwork, doc.artwork);
    }
}
#[test]
fn transform_pixels_plan_admission_failure_never_changes_source_raster_or_mask() {
    let doc = transformed_document(true);
    let before = doc.clone();
    for limit in [
        ProjectLimits { dimension: 32, ..Default::default() },
        ProjectLimits { tiles: 1, ..Default::default() },
        ProjectLimits { raster_bytes: 1, ..Default::default() },
    ] {
        assert!(doc.transform_pixels_plan(fixture::target(&doc, "Ink"), Interpolation::Linear, limit).is_err());
        assert_eq!(doc, before);
    }
}
#[test]
fn transform_pixels_plan_refuses_identity_locked_and_nonpaint_targets() {
    let mut doc = transformed_document(true);
    let target = fixture::target(&doc, "Ink");
    fixture::occurrence_mut(&mut doc, "Ink").placement = LayerPlacement::from_affine(Affine::IDENTITY);
    assert!(doc.transform_pixels_plan(target, Interpolation::Linear, Default::default()).is_err());
    fixture::occurrence_mut(&mut doc, "Ink").placement = LayerPlacement::from_affine(Affine([2., 0., 0., 2., 0., 0.]));
    fixture::occurrence_mut(&mut doc, "Ink").locked = true;
    assert!(doc.transform_pixels_plan(target, Interpolation::Linear, Default::default()).is_err());
    assert_eq!(doc.transform_pixels_refusal(SourceTarget::Paint(PaintHandle::INVALID)), Some(TransformPixelsRefusal::Target));
    let fill = fixture::id(&doc, "Paper");
    assert!(doc.scene().effect(fill).is_some_and(|effect| effect.constant_color().is_some()));
    assert_eq!(doc.scene().source_target(fill), None);
}
#[test]
fn transform_pixels_plan_keeps_nested_color_and_linked_mask_in_one_world_frame() {
    let mut doc = transformed_document(true);
    let owner = fixture::id(&doc, "Ink");
    let target = fixture::target(&doc, "Ink");
    let mask = SourceTarget::Coverage(fixture::occurrence(&doc, "Ink").mask.as_ref().unwrap().source);
    fixture::insert_paint(&mut doc, "Group", 0, None);
    let group = fixture::nest(&mut doc, "Group", &["Ink"]);
    fixture::occurrence_mut(&mut doc, "Group").translation = Point { x: 25., y: 35. };
    fixture::occurrence_mut(&mut doc, "Ink").translation = Point { x: 7., y: 9. };
    doc.working.selection =
        Some(Selection::polygon(vec![Point { x: 3., y: 4. }, Point { x: 17., y: 4. }, Point { x: 17., y: 19. }]).unwrap());
    let before = doc.clone();
    let plan = doc.transform_pixels_plan(target, Interpolation::Linear, Default::default()).unwrap();
    let mut after = doc.clone();
    after.apply(plan.output.clone()).unwrap();
    let origin = after.layer_offset(owner);
    for t in [target, mask] {
        for point in [Point { x: 0., y: 0. }, Point { x: 11., y: 23. }, Point { x: 767., y: 511. }] {
            let frozen = plan.scene.view().target_geometry(t).as_affine().unwrap().map(point);
            let original = doc.affine_edit_transform(t).unwrap().map(point);
            assert!((frozen.x - plan.origin.x + origin.x - original.x).abs() < 0.001);
            assert!((frozen.y - plan.origin.y + origin.y - original.y).abs() < 0.001);
        }
    }
    assert_eq!(after.scene().parent(owner), Some(group));
    assert_eq!(plan.scene.view().parent(owner), Some(group));
    assert_eq!(doc, before);
}
