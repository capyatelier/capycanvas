fn transformed_document(linked: bool) -> Document {
    let mut doc = Document::new("retained", 128, 96, crate::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    doc.layers[0].source = Some(std::sync::Arc::new(photo_source([24, 12])));
    doc.layers[0].properties.placement = LayerPlacement::from_affine(Affine([2., 0., 0., 3., -40., 10.]));
    doc.layers[0].properties.extent = Some([768, 512]);
    let mut data = raster(RasterPlane::Color, doc.color, &[[0, 0], [1, 0]]).wait_data().unwrap().as_ref().clone();
    for plane in [RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
        data.tiles.insert(TileKey { plane, coordinate: [1, 0] }, tile(plane, doc.color, 7));
    }
    doc.layers[0].raster = RasterRevision::backed(data);
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: 30., y: -25. });
    mask.linked = linked;
    mask.placement = Projective::from_affine(Affine([1., 0., 0., 2., 7., -20.]));
    mask.raster = raster(RasterPlane::Mask, doc.color, &[[0, 0], [2, 1]]);
    mask.initial = Some(Selection::polygon(vec![Point { x: 5., y: 7. }, Point { x: 13., y: 7. }, Point { x: 13., y: 17. }, Point { x: 5., y: 17. }]).unwrap());
    doc.layers[0].mask = Some(mask);
    doc
}

#[test]
fn transform_pixels_plan_freezes_raw_planes_and_preserves_unlinked_mask_identity() {
    for linked in [false, true] {
        let doc = transformed_document(linked);
        Project { document: doc.clone() }.validate(Default::default()).unwrap();
        let before = doc.clone();
        let id = doc.layers[0].id;
        let plan = doc.transform_pixels_plan(id, Interpolation::Linear, Default::default()).unwrap();
        assert_eq!(doc, before);
        assert_eq!(matches!(plan.scope,TransformPixelsScope::Paint{linked_mask:true}), linked);
        assert_eq!(plan.geometry.placement.interpolation, Interpolation::Linear);
        assert_eq!(plan.input.document.layers.len(), 1);
        let frozen = &plan.input.document.layers[0];
        assert!(std::sync::Arc::ptr_eq(frozen.source.as_ref().unwrap(), doc.layers[0].source.as_ref().unwrap()));
        assert_eq!(frozen.raster, doc.layers[0].raster);
        assert_eq!(frozen.mask.as_ref().unwrap().raster, doc.layers[0].mask.as_ref().unwrap().raster);
        assert_eq!(plan.output.properties.placement, LayerPlacement::IDENTITY);
        assert!(plan.output.source.is_none());
        assert!(plan.output.raster.wait_data().unwrap().tiles.is_empty());
        if linked {
            let mask = plan.output.mask.as_ref().unwrap();
            assert_eq!(mask.placement, Projective::IDENTITY);
            assert_eq!(mask.offset, plan.output.properties.offset);
            assert!(mask.initial.is_none());
            assert!(mask.raster.wait_data().unwrap().tiles.is_empty());
        } else {
            assert_eq!(plan.output.mask, doc.layers[0].mask);
        }
        assert!(plan.input.document.selection.is_none());
        assert!(plan.input.document.reference_layers.is_empty());
    }
}

#[test]
fn transform_pixels_plan_admission_failure_never_changes_source_raster_or_mask() {
    let doc = transformed_document(true);
    let before = doc.clone();
    for limit in [ProjectLimits { dimension: 32, ..Default::default() },
        ProjectLimits { tiles: 1, ..Default::default() },
        ProjectLimits { raster_bytes: 1, ..Default::default() }] {
        assert!(doc.transform_pixels_plan(doc.layers[0].id, Interpolation::Linear, limit).is_err());
        assert_eq!(doc, before);
    }
}

#[test]
fn transform_pixels_plan_refuses_identity_locked_and_nonpaint_targets() {
    let mut doc = transformed_document(true);
    let id = doc.layers[0].id;
    doc.layers[0].properties.placement = LayerPlacement::from_affine(Affine::IDENTITY);
    assert!(doc.transform_pixels_plan(id, Interpolation::Linear, Default::default()).is_err());
    doc.layers[0].properties.placement = LayerPlacement::from_affine(Affine([2., 0., 0., 2., 0., 0.]));
    doc.layers[0].properties.locked = true;
    assert!(doc.transform_pixels_plan(id, Interpolation::Linear, Default::default()).is_err());
    let paper = doc.layers.iter().find(|layer| layer.kind == LayerKind::Background).unwrap();
    assert!(doc.transform_pixels_plan(paper.id, Interpolation::Linear, Default::default()).is_err());
}

#[test]
fn transform_pixels_plan_keeps_nested_color_and_linked_mask_in_one_world_frame() {
    let mut doc = transformed_document(true);
    let owner = doc.layers[0].id;
    let mask = doc.layers[0].mask.as_ref().unwrap().id;
    let group_id = doc.allocate_layer_id();
    let mut group = Layer::paint(group_id, "translated group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 25., y: 35. };
    doc.layers[0].properties.parent = Some(group_id);
    doc.layers[0].properties.offset = Point { x: 7., y: 9. };
    doc.layers.push(group);
    doc.selection = Some(Selection::polygon(vec![Point { x: 3., y: 4. }, Point { x: 17., y: 4. }, Point { x: 17., y: 19. }]).unwrap());
    let before = doc.clone();
    let plan = doc.transform_pixels_plan(owner, Interpolation::Linear, Default::default()).unwrap();
    let mut after = doc.clone();
    *after.layers.iter_mut().find(|layer| layer.id == owner).unwrap() = plan.output.clone();
    let origin = after.layer_offset(owner);
    for target in [owner, mask] {
        for point in [Point { x: 0., y: 0. }, Point { x: 11., y: 23. }, Point { x: 767., y: 511. }] {
            let frozen = plan.input.document.affine_edit_transform(target).unwrap().map(point);
            let original = doc.affine_edit_transform(target).unwrap().map(point);
            assert!((frozen.x + origin.x - original.x).abs() < 0.001);
            assert!((frozen.y + origin.y - original.y).abs() < 0.001);
        }
    }
    assert_eq!(plan.output.properties.parent, Some(group_id));
    assert!(plan.input.document.layers[0].properties.parent.is_none());
    assert!(plan.input.document.selection.is_none());
    assert_eq!(doc, before);
}
