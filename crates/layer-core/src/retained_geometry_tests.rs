use crate::*;
use std::{sync::Arc,collections::BTreeSet};
use crate::operation_test_support as fixture;
fn document()->Document {fixture::document([128,96], &["Ink"])}
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
fn full_target_geometry_links_homographic_masks_and_refuses_only_nonlinear_writes() {
    let mut doc=document();let id=fixture::id(&doc,"Ink");let target=fixture::target(&doc,"Ink");fixture::occurrence_mut(&mut doc,"Ink").placement=LayerPlacement::from_projective(perspective());
    let mid=fixture::add_mask(&mut doc,id,[64,32],Point{x:6.,y:9.});let mask_target=SourceTarget::Coverage(mid);fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().placement=Projective::from_affine(Affine::translation(Point{x:2.,y:-3.}));
    let p=Point{x:20.,y:10.};near(doc.target_geometry(mask_target).map(p).unwrap(),perspective().map(Point{x:28.,y:16.}).unwrap());
    assert_eq!(doc.target_extent(mask_target),[64,32]);assert_eq!(doc.try_drawing_target(),Err(DrawingRefusal::NonAffine));assert_eq!(doc.validate_content_write(mask_target),Err(DrawingRefusal::NonAffine));
    let before=fixture::occurrence(&doc,"Ink").mask.clone();let owner=fixture::occurrence(&doc,"Ink").clone();assert!(fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().set_linked(false,&owner).is_err());assert_eq!(fixture::occurrence(&doc,"Ink").mask,before);
    fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=false;assert!(doc.affine_edit_transform(mask_target).is_some());assert!(doc.validate_content_write(mask_target).is_ok());assert!(doc.target_geometry(target).map(p).is_some());
}

#[test]
fn group_delta_preserves_raw_roots_and_linked_premaps_with_atomic_admission() {
    for linked in [false,true] {
    let mut doc=fixture::document([128,96],&["Group","Ink"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&["Ink"]);fixture::occurrence_mut(&mut doc,"Group").translation=Point{x:10.,y:13.};
    let mask=fixture::add_mask(&mut doc,group,[128,96],Point{x:7.,y:4.});fixture::occurrence_mut(&mut doc,"Group").mask.as_mut().unwrap().linked=linked;
    let old=doc.clone();let edit=doc.retained_transform_edit(&[group,paint],perspective()).unwrap();let mut editor=Editor::new(doc);editor.perform(edit).unwrap();
    for target in [fixture::target(&old,"Ink"),SourceTarget::Coverage(mask)] {for p in [Point{x:20.,y:10.},Point{x:50.,y:40.}] {let before=old.target_geometry(target).map(p).unwrap();near(editor.document().target_geometry(target).map(p).unwrap(),if target==SourceTarget::Coverage(mask)&&!linked{before}else{perspective().map(before).unwrap()});}}
    assert_eq!(fixture::paint(editor.document(),"Ink").raster,fixture::paint(&old,"Ink").raster);editor.undo().unwrap();let mut restored=editor.document().clone();restored.revision=old.revision;assert_eq!(restored,old);editor.redo().unwrap();
    let mut locked=old.clone();fixture::occurrence_mut(&mut locked,"Ink").locked=true;let before=locked.clone();assert!(locked.retained_transform_edit(&[group],perspective()).is_err());assert_eq!(locked,before);
    }
}

#[test]
fn retained_single_multi_and_nested_group_transforms_preserve_unlinked_masks() {
    for mode in 0..3 {for linked in [false,true] {
        let mut doc=fixture::document([128,96],&["Group","Ink","Other"]);
        let paint=fixture::id(&doc,"Ink");let other=fixture::id(&doc,"Other");
        let group=fixture::nest(&mut doc,"Group",&["Ink"]);
        fixture::occurrence_mut(&mut doc,"Group").translation=Point{x:19.,y:-11.};
        let mask=fixture::add_mask(&mut doc,paint,[64,48],Point{x:7.,y:4.});
        fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=linked;
        fixture::occurrence_mut(&mut doc,"Ink").placement.mesh=Some(Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap().move_node(5,Point{x:13.,y:8.}).unwrap()));
        let original=doc.clone();let roots=match mode{0=>vec![paint],1=>vec![paint,other],_=>vec![group]};
        let edit=doc.retained_transform_edit(&roots,perspective()).unwrap();let mut editor=Editor::new(doc);editor.perform(edit).unwrap();
        for p in [Point{x:20.,y:10.},Point{x:50.,y:40.}] {
            let before=original.target_geometry(SourceTarget::Coverage(mask)).map(p).unwrap();
            near(editor.document().target_geometry(SourceTarget::Coverage(mask)).map(p).unwrap(),if linked{perspective().map(before).unwrap()}else{before});
        }
        if !linked{assert_eq!(fixture::occurrence(editor.document(),"Ink").mask,fixture::occurrence(&original,"Ink").mask);}
        if mode!=1{assert_eq!(fixture::occurrence(editor.document(),"Other"),fixture::occurrence(&original,"Other"));}
        editor.undo().unwrap();let mut restored=editor.document().clone();restored.revision=original.revision;assert_eq!(restored,original);editor.redo().unwrap();
    }}
}

#[test]
fn scalar_mask_bake_keeps_owner_tree_and_domain_while_linked_fold_bakes_registered_pair() {
    let mut doc=document();let id=fixture::id(&doc,"Ink");let mid=fixture::add_mask(&mut doc,id,[64,48],Point{x:-11.,y:9.});let target=SourceTarget::Coverage(mid);let mask=fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap();mask.linked=false;mask.placement=perspective();mask.inverted=true;mask.enabled=false;doc.artwork.coverage.get_mut(mid).unwrap().initial=Some(Selection::polygon(Rect::from_extent([20,12]).corners().to_vec()).unwrap());
    let original=fixture::occurrence(&doc,"Ink").clone();let plan=doc.transform_pixels_plan(target,Interpolation::Bicubic,Default::default()).unwrap();assert_eq!(plan.target,target);assert_eq!(plan.scope,TransformPixelsScope::Mask);assert_eq!(plan.scene.view().target_extent(target),[64,48]);let mut editor=Editor::new(doc.clone());editor.perform(plan.output.clone()).unwrap();let after=fixture::occurrence(editor.document(),"Ink");let mut expected=original.clone();expected.mask=after.mask.clone();assert_eq!(*after,expected);assert!(after.mask.as_ref().unwrap().inverted);assert!(!after.mask.as_ref().unwrap().enabled);assert!(editor.document().artwork.coverage.get(mid).unwrap().initial.is_none());assert_eq!(fixture::paint(editor.document(),"Ink"),fixture::paint(&doc,"Ink"));
    assert_eq!(plan.scene.view().paint_source(id).unwrap().raster,Default::default());assert!(plan.scene.view().paint_source(id).unwrap().base.is_none());
    fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=true;fixture::occurrence_mut(&mut doc,"Ink").placement.mesh=Some(Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap().move_node(5,Point{x:30.,y:10.}).unwrap()));
    let pair=doc.transform_pixels_plan(target,Interpolation::Bicubic,Default::default()).unwrap();assert_eq!(pair.target,target);assert_eq!(pair.scope,TransformPixelsScope::Paint{linked_mask:true});let mut editor=Editor::new(doc);editor.perform(pair.output).unwrap();let owner=fixture::occurrence(editor.document(),"Ink");assert_eq!(fixture::id(editor.document(),"Ink"),id);assert_eq!(owner.placement,LayerPlacement::IDENTITY);assert_eq!(owner.mask.as_ref().unwrap().placement,Projective::IDENTITY);
}

#[test]
fn current_format_round_trip_keeps_nonuniform_mesh_and_mask_extent_and_preserves_unsupported_envelopes() {
    let mut doc=document();let mesh=MeshMap::identity(Rect::from_extent([128,96]),[1,1]).unwrap().split(0,0.375).unwrap();fixture::occurrence_mut(&mut doc,"Ink").placement=LayerPlacement{outer:perspective(),mesh:Some(Arc::new(mesh)),interpolation:Interpolation::Lanczos};let owner=fixture::id(&doc,"Ink");fixture::add_mask(&mut doc,owner,[64,48],Point::default());let bytes=fixture::encoded(&doc);let loaded=fixture::decoded(bytes.clone());assert_eq!(fixture::occurrence(&loaded,"Ink").placement,fixture::occurrence(&doc,"Ink").placement);let mask=fixture::occurrence(&loaded,"Ink").mask.as_ref().unwrap();assert_eq!(loaded.target_extent(SourceTarget::Coverage(mask.source)),[64,48]);
    let mut input=std::io::Cursor::new(bytes);let directory=package::archive::Directory::read(&mut input,262144,64*1024*1024).unwrap();let member=directory.member("manifest.json").unwrap();let manifest=directory.read_member(&mut input,member,64*1024*1024).unwrap();let mut value:serde_json::Value=serde_json::from_slice(&manifest).unwrap();assert_eq!(value["format"],"capy.canvas");assert_eq!(value["version"],1);
    for version in 2..9 {value["version"]=version.into();let unsupported=serde_json::to_vec(&value).unwrap();assert!(matches!(package::manifest::Manifest::parse(&unsupported,&directory,Default::default()).unwrap(),package::manifest::ManifestRead::UnsupportedEnvelope(_)));}
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
    let mut doc=document();let owner=fixture::id(&doc,"Ink");let target=fixture::target(&doc,"Ink");let mask_id=fixture::add_mask(&mut doc,owner,[32,24],Point{x:-300.,y:-200.});fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=false;let before=fixture::occurrence(&doc,"Ink").clone();
    let edits=doc.paint_extent_plan(&[target],GeometryLimits{project:Default::default(),device_dimension:4096}).unwrap();let mut editor=Editor::new(doc.clone());editor.perform(Edit::Batch(edits)).unwrap();let mut after=fixture::occurrence(editor.document(),"Ink").clone();after.mask=before.mask.clone();assert_eq!(after,before);assert_eq!(fixture::paint(editor.document(),"Ink"),fixture::paint(&doc,"Ink"));assert_eq!(editor.document().target_extent(target),[128,96]);let extent=editor.document().target_extent(SourceTarget::Coverage(mask_id));assert!(extent[0]>=428&&extent[1]>=296);assert_eq!(doc.target_extent(SourceTarget::Coverage(mask_id)),[32,24]);
    let mut virtual_canvas=doc.clone();virtual_canvas.artwork.compositions.get_mut(virtual_canvas.artwork.root).unwrap().size=[1024,768];assert_eq!(virtual_canvas.target_extent(target),[128,96]);assert_eq!(virtual_canvas.target_extent(SourceTarget::Coverage(mask_id)),[32,24]);assert!(editor.undo().unwrap());let mut restored=editor.document().clone();restored.revision=doc.revision;assert_eq!(restored,doc);
}

#[test]
fn retained_admission_refuses_adjustment_roots_empty_groups_and_pending_descendants() {
    let mut doc=fixture::document([128,96],&["Ink","Group"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&[]);assert!(doc.retained_transform_targets(&[group]).is_err());let adjustment=fixture::insert_paint(&mut doc,"Adjustment",0,Some(group));fixture::effect(&mut doc,"Adjustment","gaussian_blur");assert!(doc.retained_transform_targets(&[adjustment]).is_err());assert!(doc.retained_transform_targets(&[group]).is_err());fixture::nest(&mut doc,"Group",&["Ink","Adjustment"]);assert_eq!(doc.retained_transform_targets(&[group,paint]).unwrap(),vec![group,paint,adjustment]);
    let coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),[128,96],Point::default());Arc::make_mut(&mut fixture::paint_mut(&mut doc,"Ink").operations).push(RasterOperation{placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Erase{alpha_locked:false}});let before=doc.clone();assert!(doc.retained_transform_edit(&[group],perspective()).is_err());assert_eq!(doc,before);
}

#[test]
fn content_bounds_cache_keeps_all_current_targets_but_only_one_time_per_scope() {
    let mut doc=document();let mut targets=Vec::new();for index in 0..4 {let h=fixture::insert_paint(&mut doc,format!("Paint {index}"),index,None);targets.push(doc.scene().source_target(h).unwrap());}let requests:Vec<_>=targets.iter().map(|target|ContentBoundsRequest::new(&doc,ContentScope::Target(*target))).collect();let mut cache=ContentBoundsCache::default();for request in &requests{cache.insert(request.clone(),Rect::from_extent([24,16]));}for request in &requests{assert_eq!(cache.get(request),Some(Rect::from_extent([24,16])));}let mut timed=requests[0].clone();Arc::make_mut(&mut timed.snapshot).context.elapsed=12.;cache.insert(timed.clone(),Rect::from_extent([30,20]));assert!(cache.get(&requests[0]).is_none());assert_eq!(cache.get(&timed),Some(Rect::from_extent([30,20])));for request in &requests[1..]{assert!(cache.get(request).is_some());}doc.revision+=1;cache.discard_changed(&doc);assert!(cache.get(&timed).is_none());for request in &requests{assert!(cache.get(request).is_none());}
}

#[test]
fn bake_member_meshes_and_mask_operations_share_one_retained_geometry_charge() {
    let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap());let mut members=fixture::document([128,96],&["Member 1","Member 2"]);for name in ["Member 1","Member 2"]{fixture::occurrence_mut(&mut members,name).placement.mesh=Some(mesh.clone());}let scene=members.snapshot();let scope=SceneScope::Members(vec![fixture::id(&members,"Member 1"),fixture::id(&members,"Member 2")].into());let mut doc=document();let owner=fixture::id(&doc,"Ink");let coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),[128,96],Point::default());Arc::make_mut(&mut fixture::paint_mut(&mut doc,"Ink").operations).push(RasterOperation{placement:Affine::IDENTITY,coverage:coverage.clone(),kind:RasterOperationKind::Bake{scene,scope,offset:Point::default()}});let mask=fixture::add_mask(&mut doc,owner,[128,96],Point::default());doc.artwork.coverage.get_mut(mask).unwrap().operations=vec![RasterOperation{placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Transform(ImageTransform{placement:LayerPlacement{mesh:Some(mesh.clone()),..Default::default()},..Default::default()})}].into();
    let mut roots=RootInventory::default();roots.document(&doc);assert_eq!(roots.meshes.len(),3);assert!(roots.meshes.iter().all(|root|Arc::ptr_eq(root,&mesh)));let mut accounting=crate::history_budget::Accounting::new(&doc);assert_eq!(accounting.charge_mesh(&mesh),0,"Bake-only ownership is already charged by the current document");let original=doc.clone();fixture::paint_mut(&mut doc,"Ink").operations=Arc::default();doc.artwork.coverage.get_mut(mask).unwrap().operations=Arc::default();let mut stripped=RootInventory::default();stripped.document(&doc);assert!(stripped.meshes.is_empty());let mut retained=RootInventory::default();retained.document(&original);assert_eq!(retained.meshes.len(),3);assert!(retained.meshes.iter().all(|root|Arc::ptr_eq(root,&mesh)));
}

#[test]
fn identity_group_delta_preserves_non_normalized_maps_mesh_roots_masks_and_redo() {
    let mut doc=fixture::document([128,96],&["Group","Ink"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&["Ink"]);fixture::occurrence_mut(&mut doc,"Group").translation=Point{x:8192.25,y:-4096.125};fixture::add_mask(&mut doc,group,[128,96],Point{x:123.5,y:-45.25});let mask=fixture::occurrence_mut(&mut doc,"Group").mask.as_mut().unwrap();mask.linked=false;mask.placement=Projective([2.,0.1,4.,0.,2.,7.,0.,0.,2.]);let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap());fixture::occurrence_mut(&mut doc,"Ink").placement=LayerPlacement{outer:Projective([2.,0.2,7.,0.,2.,11.,0.001,-0.0005,2.]),mesh:Some(mesh.clone()),interpolation:Interpolation::Lanczos};
    let mut editor=Editor::new(doc.clone());let mut changed=fixture::occurrence(&doc,"Ink").clone();changed.opacity=0.5;editor.perform(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,paint,Some(changed)).unwrap())).unwrap();assert!(editor.undo().unwrap());let before=editor.document().clone();let redo=editor.next_history_edit(true).cloned();let edit=editor.document().retained_transform_edit(&[group,paint],Projective::IDENTITY).unwrap();assert_eq!(edit,Edit::Batch(Vec::new()));editor.perform(edit).unwrap();assert_eq!(editor.document(),&before);assert_eq!(editor.next_history_edit(true),redo.as_ref());assert!(Arc::ptr_eq(fixture::occurrence(editor.document(),"Ink").placement.mesh.as_ref().unwrap(),&mesh));
}
