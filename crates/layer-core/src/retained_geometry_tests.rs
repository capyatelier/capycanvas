use crate::*;
use std::{sync::Arc,collections::BTreeSet};
fn document()->Document {Document::new("geometry",128,96,DocumentNames{paint:"Ink".into(),paper:"Paper".into()})}
fn perspective()->Projective {Projective::rect_to_quad(Rect::from_extent([128,96]),[[8.,4.],[132.,10.],[115.,103.],[-4.,88.]].map(|[x,y]|Point{x,y})).unwrap()}
fn near(a:Point,b:Point){assert!((a.x-b.x).hypot(a.y-b.y)<0.003,"{a:?} != {b:?}");}
#[test]
fn retained_outer_and_conjugation_preserve_exact_mesh_roots_and_source_coordinates(){
    let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap().move_node(5,Point{x:19.,y:-7.}).unwrap());
    let placement=LayerPlacement{outer:perspective(),mesh:Some(mesh.clone()),interpolation:Interpolation::Lanczos};
    let adapter=Projective::from_affine(Affine::translation(Point{x:4.,y:9.}));
    let map=ImageTransform{placement,source_from_owner:Some(adapter),keep_source:true};
    let to=Affine::around(Point::default(),[1.4,0.7],0.2,Point{x:12.,y:-8.});let moved=map.conjugate(to).unwrap();
    assert!(Arc::ptr_eq(moved.placement.mesh.as_ref().unwrap(),&mesh));assert_eq!(moved.placement.interpolation,Interpolation::Lanczos);assert!(moved.keep_source);
    for y in 0..9 {for x in 0..9 {let p=Point{x:x as f32*12.+4.,y:y as f32*9.+9.};near(moved.map(to.map(p)).unwrap(),to.map(map.map(p).unwrap()));}}
    assert!(moved.as_affine().is_none());
}
#[test]
fn exact_nonuniform_splits_refinement_and_multinode_edits_keep_surface_and_shared_tangents(){
    let mesh=MeshMap::identity(Rect::from_extent([200,120]),[1,1]).unwrap().move_node(0,Point{x:12.,y:17.}).unwrap().move_tangent(3,2,Point{x:80.,y:170.}).unwrap();
    let split=mesh.split(0,0.37).unwrap().split(1,0.61).unwrap();assert_eq!(split.cells(),[2,2]);assert_eq!(split.breakpoints[0].as_ref(),&[0.,0.37,1.]);
    for y in 0..25 {for x in 0..25 {let p=Point{x:x as f32*200./24.,y:y as f32*120./24.};near(mesh.map(p).unwrap(),split.map(p).unwrap());}}
    assert!(split.split(0,0.37).is_none());assert!(split.split(0,0.370001).is_none());assert!(split.refine([3,3]).is_none());
    let uniform=mesh.refine([4,4]).unwrap();assert_eq!(uniform.cells(),[4,4]);
    let nodes=BTreeSet::from([0,1]);let delta=Point{x:3.,y:-4.};let moved=uniform.move_nodes(&nodes,delta).unwrap();
    for node in nodes {let p=uniform.node(node).unwrap();near(moved.node(node).unwrap(),Point{x:p.x+delta.x,y:p.y+delta.y});}
    assert!(mesh.move_nodes(&BTreeSet::from([99]),delta).is_none());assert!(mesh.move_nodes(&BTreeSet::from([0]),Point{x:f32::NAN,y:0.}).is_none());
}
#[test]
fn full_target_geometry_links_homographic_masks_and_refuses_only_nonlinear_writes(){
    let mut doc=document();let id=doc.layers[0].id;doc.layers[0].properties.placement=LayerPlacement::from_projective(perspective());
    let mut mask=LayerMask::reveal_all(doc.allocate_layer_id(),Point{x:6.,y:9.});mask.placement=Projective::from_affine(Affine::translation(Point{x:2.,y:-3.}));mask.extent=Some([64,32]);let mid=mask.id;doc.layers[0].mask=Some(mask);
    let p=Point{x:20.,y:10.};near(doc.layer_geometry(mid).map(p).unwrap(),perspective().map(Point{x:28.,y:16.}).unwrap());
    assert_eq!(doc.target_extent(mid),[64,32]);assert_eq!(doc.try_drawing_target(),Err(DrawingRefusal::NonAffine));assert_eq!(doc.validate_content_write(mid),Err(DrawingRefusal::NonAffine));
    let before=doc.layers[0].mask.clone();let properties=doc.layers[0].properties.clone();assert!(doc.layers[0].mask.as_mut().unwrap().set_linked(false,&properties).is_err());assert_eq!(doc.layers[0].mask,before);
    doc.layers[0].mask.as_mut().unwrap().linked=false;assert!(doc.affine_edit_transform(mid).is_some());assert!(doc.validate_content_write(mid).is_ok());assert!(doc.layer_geometry(id).map(p).is_some());
}
#[test]
fn group_delta_preserves_raw_roots_and_linked_premaps_with_atomic_admission(){
    let mut doc=document();let paint=doc.layers[0].id;let group=doc.allocate_layer_id();let mut g=Layer::paint(group,"Group");g.kind=LayerKind::Group;g.properties.offset=Point{x:10.,y:13.};doc.layers[0].properties.parent=Some(group);
    let mut mask=LayerMask::reveal_all(doc.allocate_layer_id(),Point{x:7.,y:4.});mask.linked=false;g.mask=Some(mask);doc.layers.push(g);
    let old=doc.clone();let edit=doc.retained_transform_edit(&[group,paint],perspective()).unwrap();let mut editor=Editor::new(doc);editor.perform(edit).unwrap();
    for target in [paint,old.layers.last().unwrap().mask.as_ref().unwrap().id] {for p in [Point{x:20.,y:10.},Point{x:50.,y:40.}] {near(editor.document().layer_geometry(target).map(p).unwrap(),perspective().map(old.layer_geometry(target).map(p).unwrap()).unwrap());}}
    assert_eq!(editor.document().layer(paint).unwrap().raster,old.layer(paint).unwrap().raster);editor.undo().unwrap();let mut restored=editor.document().clone();restored.revision=old.revision;assert_eq!(restored,old);editor.redo().unwrap();
    let mut locked=old.clone();locked.layers[0].properties.locked=true;let before=locked.clone();assert!(locked.retained_transform_edit(&[group],perspective()).is_err());assert_eq!(locked,before);
}

#[test]
fn scalar_mask_bake_keeps_owner_tree_and_domain_while_linked_fold_bakes_registered_pair(){
    let mut doc=document();let id=doc.layers[0].id;let mut mask=LayerMask::reveal_all(doc.allocate_layer_id(),Point{x:-11.,y:9.});let mid=mask.id;mask.linked=false;mask.placement=perspective();mask.extent=Some([64,48]);mask.inverted=true;mask.enabled=false;mask.initial=Some(Selection::polygon(Rect::from_extent([20,12]).corners().to_vec()).unwrap());doc.layers[0].mask=Some(mask);
    let original=doc.layers[0].clone();let plan=doc.transform_pixels_plan(mid,Interpolation::Bicubic,Default::default()).unwrap();assert_eq!(plan.target,mid);assert_eq!(plan.scope,TransformPixelsScope::Mask);let mut expected=original.clone();expected.mask=plan.output.mask.clone();assert_eq!(plan.output,expected);assert_eq!(plan.input.document.target_extent(mid),[64,48]);assert!(plan.output.mask.as_ref().unwrap().inverted);assert!(!plan.output.mask.as_ref().unwrap().enabled);assert!(plan.output.mask.as_ref().unwrap().initial.is_none());
    assert_eq!(plan.input.document.layers[0].raster,Default::default());assert!(plan.input.document.layers[0].source.is_none());
    doc.layers[0].mask.as_mut().unwrap().linked=true;doc.layers[0].properties.placement.mesh=Some(Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap().move_node(5,Point{x:30.,y:10.}).unwrap()));
    let pair=doc.transform_pixels_plan(mid,Interpolation::Bicubic,Default::default()).unwrap();assert_eq!(pair.target,mid);assert_eq!(pair.output.id,id);assert_eq!(pair.scope,TransformPixelsScope::Paint{linked_mask:true});assert_eq!(pair.output.properties.placement,LayerPlacement::IDENTITY);assert_eq!(pair.output.mask.as_ref().unwrap().placement,Projective::IDENTITY);
}
#[test]
fn format14_round_trip_keeps_nonuniform_mesh_and_mask_extent_and_rejects_older_headers(){
    let mut doc=document();let mesh=MeshMap::identity(Rect::from_extent([128,96]),[1,1]).unwrap().split(0,0.375).unwrap();doc.layers[0].properties.placement=LayerPlacement{outer:perspective(),mesh:Some(Arc::new(mesh)),interpolation:Interpolation::Lanczos};let mut mask=LayerMask::reveal_all(doc.allocate_layer_id(),Point::default());mask.extent=Some([64,48]);doc.layers[0].mask=Some(mask);let project=Project{document:doc};let mut bytes=Vec::new();project.write(&mut bytes).unwrap();assert_eq!(&bytes[..12],b"CAPYRASTER\x0e\0");let loaded=Project::read(bytes.as_slice(),Default::default()).unwrap();assert_eq!(loaded,project);
    for version in 8..14 {let mut old=bytes.clone();old[10]=version;assert!(Project::read(old.as_slice(),Default::default()).is_err());}
}

#[test]
fn shared_mesh_history_accounting_charges_root_and_arrays_once(){
    let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap());let mut accounting=crate::history_budget::Accounting::default();let charged=accounting.charge_mesh(&mesh);assert!(charged>mesh.net.len()*std::mem::size_of::<Point>());assert_eq!(accounting.charge_mesh(&mesh),0);
    let root=Arc::new((*mesh).clone());assert_eq!(accounting.charge_mesh(&root),std::mem::size_of::<MeshMap>());
    let split=Arc::new(mesh.split(0,0.125).unwrap());assert!(accounting.charge_mesh(&split)>std::mem::size_of::<MeshMap>());
}
#[test]
fn cached_tessellation_hits_use_later_triangle_winner_and_keep_no_hit_distinct(){
    let surface=Tessellation{grid:[2,1],positions:vec![Point{x:0.,y:0.},Point{x:1.,y:0.},Point{x:0.,y:0.},Point{x:0.,y:1.},Point{x:1.,y:1.},Point{x:0.,y:1.}],sources:vec![Point{x:0.,y:0.},Point{x:1.,y:0.},Point{x:2.,y:0.},Point{x:0.,y:1.},Point{x:1.,y:1.},Point{x:2.,y:1.}]};
    near(surface.source_at(Point{x:0.25,y:0.5}).unwrap(),Point{x:1.75,y:0.5});assert_eq!(surface.source_at(Point{x:-1.,y:0.}),None);
}

#[test]
fn affine_mesh_controls_admit_only_exact_grid_refinement() {
    let bounds = Rect { min: Point { x: -27., y: 19. }, max: Point { x: 313., y: 227. } };
    let identity = MeshMap::identity(bounds, [3, 3]).unwrap();
    assert!(identity.is_identity());
    assert!(identity.can_refine([6, 6]));
    assert!(!identity.can_refine([4, 4]));
    assert!(!identity.can_refine([2, 3]));
    assert!(!identity.can_refine([33, 3]));
    assert!(identity.refine([4, 4]).is_none());
    let affine = Affine([1.25, 0.2, -0.3, 0.75, 11., -17.]);
    let mapped = MeshMap::from_affine(bounds, [3, 3], affine).unwrap();
    assert!(!mapped.is_identity());
    for y in 0..=16 {
        for x in 0..=16 {
            let point = identity.frame.map(Point { x: x as f32 / 16., y: y as f32 / 16. });
            let actual = mapped.map(point).unwrap();
            let expected = affine.map(point);
            assert!((actual.x - expected.x).hypot(actual.y - expected.y) < 0.0003);
        }
    }
}

#[test]
fn independent_mask_admission_grows_only_its_authoritative_local_domain() {
    let mut doc = document();
    let owner = doc.layers[0].id;
    doc.layers[0].properties.extent = Some([128, 96]);
    let mask_id = doc.allocate_layer_id();
    let mut mask = LayerMask::reveal_all(mask_id, Point { x: -300., y: -200. });
    mask.linked = false;
    mask.extent = Some([32, 24]);
    doc.layers[0].mask = Some(mask);
    let before = doc.layers[0].clone();
    let edits = doc.paint_extent_plan(&[owner], GeometryLimits { project: Default::default(), device_dimension: 4096 }).unwrap();
    let mut editor = Editor::new(doc.clone());
    editor.perform(Edit::Batch(edits)).unwrap();
    let after = editor.document().layer(owner).unwrap();
    assert_eq!(after.properties, before.properties);
    assert_eq!(after.raster, before.raster);
    assert_eq!(after.source, before.source);
    assert_eq!(editor.document().target_extent(owner), [128, 96]);
    let extent = editor.document().target_extent(mask_id);
    assert!(extent[0] >= 428 && extent[1] >= 296);
    assert_eq!(doc.target_extent(mask_id), [32, 24]);
    let mut virtual_canvas = doc.clone();
    virtual_canvas.width = 1024;
    virtual_canvas.height = 768;
    assert_eq!(virtual_canvas.target_extent(owner), [128, 96]);
    assert_eq!(virtual_canvas.target_extent(mask_id), [32, 24]);
    assert!(editor.undo().unwrap());
    assert_eq!(editor.document().layers, doc.layers);
}

#[test]
fn retained_admission_refuses_adjustment_roots_empty_groups_and_pending_descendants() {
    let mut doc = document();
    let paint = doc.layers[0].id;
    let group_id = doc.allocate_layer_id();
    let mut group = Layer::paint(group_id, "Group");
    group.kind = LayerKind::Group;
    doc.layers.push(group);
    assert!(doc.retained_transform_targets(&[group_id]).is_err());
    let adjustment_id = doc.allocate_layer_id();
    let mut adjustment = Layer::paint(adjustment_id, "Adjustment");
    adjustment.kind = LayerKind::Effect;
    adjustment.properties.parent = Some(group_id);
    doc.layers.push(adjustment);
    assert!(doc.retained_transform_targets(&[adjustment_id]).is_err());
    assert!(doc.retained_transform_targets(&[group_id]).is_err());
    doc.layers[0].properties.parent = Some(group_id);
    assert_eq!(doc.retained_transform_targets(&[group_id, paint]).unwrap(), vec![paint, group_id, adjustment_id]);
    doc.layers[0].pending_operations.push(LayerOperation {
        placement: Affine::IDENTITY,
        coverage: LayerMask::reveal_all(LayerId(0), Point::default()),
        kind: LayerOperationKind::Erase { alpha_locked: false },
    });
    let before = doc.clone();
    assert!(doc.retained_transform_edit(&[group_id], perspective()).is_err());
    assert_eq!(doc, before);
}

#[test]
fn content_bounds_cache_keeps_all_current_targets_but_only_one_time_per_scope() {
    let mut doc = document();
    for index in 0..4 {
        let id = doc.allocate_layer_id();
        doc.layers.insert(index, Layer::paint(id, format!("Paint {index}")));
    }
    let requests: Vec<_> = doc.layers[..4].iter().map(|layer| ContentBoundsRequest::new(&doc, ContentScope::Target(layer.id))).collect();
    let mut cache = ContentBoundsCache::default();
    for request in &requests { cache.insert(request.clone(), Rect::from_extent([24, 16])); }
    for request in &requests { assert_eq!(cache.get(request), Some(Rect::from_extent([24, 16]))); }
    let mut timed = requests[0].clone();
    timed.time = 12.;
    cache.insert(timed.clone(), Rect::from_extent([30, 20]));
    assert!(cache.get(&requests[0]).is_none());
    assert_eq!(cache.get(&timed), Some(Rect::from_extent([30, 20])));
    for request in &requests[1..] { assert!(cache.get(request).is_some()); }
    doc.revision += 1;
    cache.discard_changed(&doc);
    assert!(cache.get(&timed).is_none());
    for request in &requests { assert!(cache.get(request).is_none()); }
}

#[test]
fn bake_member_meshes_and_mask_operations_share_one_retained_geometry_charge() {
    let mesh = Arc::new(MeshMap::identity(Rect::from_extent([128, 96]), [3, 3]).unwrap());
    let mut member = Layer::paint(LayerId(10), "Member");
    member.properties.placement.mesh = Some(mesh.clone());
    let mut output = Layer::paint(LayerId(11), "Baked");
    output.pending_operations.push(LayerOperation {
        placement: Affine::IDENTITY,
        coverage: LayerMask::reveal_all(LayerId(0), Point::default()),
        kind: LayerOperationKind::Bake { members: vec![member.clone(), member].into(), offset: Point::default() },
    });
    let mut mask = LayerMask::reveal_all(LayerId(12), Point::default());
    mask.pending_operations = vec![LayerOperation {
        placement: Affine::IDENTITY,
        coverage: LayerMask::reveal_all(LayerId(0), Point::default()),
        kind: LayerOperationKind::Transform(ImageTransform { placement: LayerPlacement { mesh: Some(mesh.clone()), ..Default::default() }, ..Default::default() }),
    }].into();
    output.mask = Some(mask);
    let mut roots = Vec::new();
    output.mesh_roots(&mut roots);
    assert_eq!(roots.len(), 3);
    assert!(roots.iter().all(|root| Arc::ptr_eq(root, &mesh)));
    let mut doc = document();
    doc.layers.insert(0, output.clone());
    let mut accounting = crate::history_budget::Accounting::new(&doc);
    assert_eq!(accounting.charge_mesh(&mesh), 0, "Bake-only ownership is already charged by the current document");
    let original = output.clone();
    without_shared_payloads(&mut output);
    let mut stripped = Vec::new();
    output.mesh_roots(&mut stripped);
    assert!(stripped.is_empty());
    let mut retained = Vec::new();
    original.mesh_roots(&mut retained);
    assert_eq!(retained.len(), 3);
    assert!(retained.iter().all(|root| Arc::ptr_eq(root, &mesh)));
}

#[test]
fn identity_group_delta_preserves_non_normalized_maps_mesh_roots_masks_and_redo() {
    let mut doc = document();
    let paint = doc.layers[0].id;
    let group_id = doc.allocate_layer_id();
    let mut group = Layer::paint(group_id, "Group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 8192.25, y: -4096.125 };
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: 123.5, y: -45.25 });
    mask.linked = false;
    mask.placement = Projective([2., 0.1, 4., 0., 2., 7., 0., 0., 2.]);
    group.mask = Some(mask);
    let mesh = Arc::new(MeshMap::identity(Rect::from_extent([128, 96]), [3, 3]).unwrap());
    doc.layers[0].properties.parent = Some(group_id);
    doc.layers[0].properties.placement = LayerPlacement {
        outer: Projective([2., 0.2, 7., 0., 2., 11., 0.001, -0.0005, 2.]),
        mesh: Some(mesh.clone()), interpolation: Interpolation::Lanczos,
    };
    doc.layers.push(group);
    let mut editor = Editor::new(doc.clone());
    let mut changed = doc.layer(paint).unwrap().clone();
    changed.opacity = 0.5;
    editor.perform(Edit::ReplaceLayer(Box::new(changed))).unwrap();
    assert!(editor.undo().unwrap());
    let before = editor.document().clone();
    let redo = editor.next_history_edit(true).cloned();
    let edit = editor.document().retained_transform_edit(&[group_id, paint], Projective::IDENTITY).unwrap();
    assert_eq!(edit, Edit::Batch(Vec::new()));
    editor.perform(edit).unwrap();
    assert_eq!(editor.document(), &before);
    assert_eq!(editor.next_history_edit(true), redo.as_ref());
    assert!(Arc::ptr_eq(editor.document().layer(paint).unwrap().properties.placement.mesh.as_ref().unwrap(), &mesh));
}
