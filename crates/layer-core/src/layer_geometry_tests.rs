use crate::*;
use std::{sync::Arc,collections::BTreeSet};
use crate::operation_test_support as fixture;
fn document()->Document {fixture::document([128,96], &["Ink"])}
fn perspective()->Projective {Projective::rect_to_quad(Rect::from_extent([128,96]),[[8.,4.],[132.,10.],[115.,103.],[-4.,88.]].map(|[x,y]|Point{x,y})).unwrap()}
fn near(a:Point,b:Point){assert!((a.x-b.x).hypot(a.y-b.y)<0.003,"{a:?} != {b:?}");}
#[test]
fn conjugation_preserves_exact_mesh_roots_and_source_coordinates(){
    let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap().move_node(5,Point{x:19.,y:-7.}).unwrap());
    let placement=LayerPlacement{outer:perspective(),mesh:Some(mesh.clone()),interpolation:Interpolation::Lanczos};
    let adapter=Projective::from_affine(Affine::translation(Point{x:4.,y:9.}));
    let map=ImageTransform{placement,source_from_owner:Some(adapter),keep_source:true,source_base:None};
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
    let mut doc=document();let owner=fixture::id(&doc,"Ink");let target=fixture::target(&doc,"Ink");let mask_id=fixture::add_mask(&mut doc,owner,[32,24],[-300,-200]);fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=false;let before=fixture::occurrence(&doc,"Ink").clone();
    let edits=doc.paint_extent_plan(&[target],GeometryLimits{project:Default::default(),device_dimension:4096}).unwrap();let mut editor=Editor::new(doc.clone());editor.perform(Edit::Batch(edits)).unwrap();let mut after=fixture::occurrence(editor.document(),"Ink").clone();after.mask=before.mask.clone();assert_eq!(after,before);assert_eq!(fixture::paint(editor.document(),"Ink"),fixture::paint(&doc,"Ink"));assert_eq!(editor.document().target_extent(target),[128,96]);let extent=editor.document().target_extent(SourceTarget::Coverage(mask_id));assert!(extent[0]>=428&&extent[1]>=296);assert_eq!(doc.target_extent(SourceTarget::Coverage(mask_id)),[32,24]);
    let mut virtual_canvas=doc.clone();virtual_canvas.artwork.compositions.get_mut(virtual_canvas.artwork.root).unwrap().size=[1024,768];assert_eq!(virtual_canvas.target_extent(target),[128,96]);assert_eq!(virtual_canvas.target_extent(SourceTarget::Coverage(mask_id)),[32,24]);assert!(editor.undo().unwrap());let mut restored=editor.document().clone();restored.revision=doc.revision;assert_eq!(restored,doc);
}

#[test]
fn content_bounds_cache_keeps_all_current_targets_but_only_one_time_per_scope() {
    let mut doc=document();let mut targets=Vec::new();for index in 0..4 {let h=fixture::insert_paint(&mut doc,format!("Paint {index}"),index,None);targets.push(doc.scene().source_target(h).unwrap());}let requests:Vec<_>=targets.iter().map(|target|ContentBoundsRequest::new(&doc,ContentScope::Target(*target))).collect();let mut cache=ContentBoundsCache::default();for request in &requests{cache.insert(request.clone(),Rect::from_extent([24,16]));}for request in &requests{assert_eq!(cache.get(request),Some(Rect::from_extent([24,16])));}let mut timed=requests[0].clone();Arc::make_mut(&mut timed.snapshot).context.elapsed=12.;cache.insert(timed.clone(),Rect::from_extent([30,20]));assert!(cache.get(&requests[0]).is_none());assert_eq!(cache.get(&timed),Some(Rect::from_extent([30,20])));for request in &requests[1..]{assert!(cache.get(request).is_some());}doc.revision+=1;cache.discard_changed(&doc);assert!(cache.get(&timed).is_none());for request in &requests{assert!(cache.get(request).is_none());}
}

#[test]
fn integer_masks_follow_the_linkage_table_for_moves_relinking_and_owners_without_offsets() {
    let mut doc=fixture::document([128,96],&["Group","Ink","Shade"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&["Ink","Shade"]);
    fixture::effect(&mut doc,"Shade","exposure");let effect=fixture::id(&doc,"Shade");
    fixture::occurrence_mut(&mut doc,"Group").offset=[10,-13];fixture::occurrence_mut(&mut doc,"Group").blend=LayerBlend::PassThrough;fixture::occurrence_mut(&mut doc,"Ink").offset=[-256,7];
    let mask=SourceTarget::Coverage(fixture::add_mask(&mut doc,paint,[64,48],[3,4]));let effect_mask=SourceTarget::Coverage(fixture::add_mask(&mut doc,effect,[64,48],[5,6]));
    assert_eq!(doc.scene().target_origin(mask),[-243,-2]);assert_eq!(doc.scene().target_origin(effect_mask),[15,-7]);
    let owner=fixture::occurrence(&doc,"Ink").clone();let mut unlinked=owner.mask.clone().unwrap();unlinked.set_linked(false,&owner).unwrap();assert_eq!(unlinked.offset,[-253,11]);
    fixture::occurrence_mut(&mut doc,"Ink").mask=Some(unlinked.clone());assert_eq!(doc.scene().target_origin(mask),[-243,-2]);
    let mut relinked=unlinked;relinked.set_linked(true,&owner).unwrap();assert_eq!(relinked.offset,[3,4]);
    fixture::activate(&mut doc,"Ink");let before=doc.clone();
    doc.apply(doc.move_target_edit([2,-5]).unwrap()).unwrap();assert_eq!(fixture::occurrence(&doc,"Ink").offset,[-254,2]);assert_eq!(doc.scene().target_origin(mask),[-243,-2]);
    doc=before.clone();doc.working.target=Some(mask);doc.apply(doc.move_target_edit([2,-5]).unwrap()).unwrap();
    assert_eq!(fixture::occurrence(&doc,"Ink").offset,[-256,7]);assert_eq!(doc.scene().target_origin(mask),[-241,-7]);
    doc=before.clone();fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=true;fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().offset=[3,4];doc.working.target=Some(mask);
    doc.apply(doc.move_target_edit([2,-5]).unwrap()).unwrap();assert_eq!(fixture::occurrence(&doc,"Ink").offset,[-254,2]);assert_eq!(fixture::occurrence(&doc,"Ink").mask.as_ref().unwrap().offset,[3,4]);
    doc.apply(doc.translate_target_edit(mask,[1,1]).unwrap()).unwrap();assert_eq!(fixture::occurrence(&doc,"Ink").mask.as_ref().unwrap().offset,[3,4]);assert_eq!(fixture::occurrence(&doc,"Ink").offset,[-253,3]);
    doc.apply(doc.translate_target_edit(effect_mask,[-5,-6]).unwrap()).unwrap();assert_eq!(doc.scene().target_origin(effect_mask),[10,-13]);assert_eq!(fixture::occurrence(&doc,"Shade").offset,[0,0]);
    doc.apply(doc.ungroup_layer_edit(group).unwrap()).unwrap();assert_eq!(fixture::occurrence(&doc,"Ink").offset,[-243,-10]);assert_eq!(doc.scene().target_origin(mask),[-240,-6]);
    assert_eq!(fixture::occurrence(&doc,"Shade").offset,[0,0]);assert_eq!(doc.scene().target_origin(effect_mask),[10,-13]);
    let mut invalid=fixture::occurrence(&doc,"Shade").clone();invalid.offset=[1,0];assert!(doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,fixture::id(&doc,"Shade"),Some(invalid)).unwrap())).is_err());
}

#[test]
fn group_and_multi_layer_moves_change_root_offsets_atomically_and_keep_unlinked_masks() {
    for linked in [false,true] {
        let mut doc=fixture::document([128,96],&["Group","Ink","Other"]);let paint=fixture::id(&doc,"Ink");let other=fixture::id(&doc,"Other");let group=fixture::nest(&mut doc,"Group",&["Ink"]);
        fixture::occurrence_mut(&mut doc,"Group").offset=[19,-11];let mask=SourceTarget::Coverage(fixture::add_mask(&mut doc,paint,[64,48],[7,4]));
        let group_mask=SourceTarget::Coverage(fixture::add_mask(&mut doc,group,[64,48],[1,2]));
        fixture::occurrence_mut(&mut doc,"Ink").mask.as_mut().unwrap().linked=linked;fixture::occurrence_mut(&mut doc,"Group").mask.as_mut().unwrap().linked=linked;
        let original=doc.clone();
        for roots in [vec![paint],vec![paint,other],vec![group]] {
            let edit=original.move_layers_edit(&roots,[-300,45]).unwrap();let mut editor=Editor::new(original.clone());editor.perform(edit).unwrap();let moved=editor.document();
            let paint_moved=roots.contains(&paint)||roots.contains(&group);
            assert_eq!(moved.scene().target_origin(fixture::target(moved,"Ink")),offsets::checked_add(original.scene().target_origin(fixture::target(&original,"Ink")),if paint_moved {[-300,45]} else {[0,0]}).unwrap());
            let follows=paint_moved&&(linked||roots.contains(&group));
            assert_eq!(moved.scene().target_origin(mask),offsets::checked_add(original.scene().target_origin(mask),if follows {[-300,45]} else {[0,0]}).unwrap());
            let group_follows=roots.contains(&group)&&linked;
            assert_eq!(moved.scene().target_origin(group_mask),offsets::checked_add(original.scene().target_origin(group_mask),if group_follows {[-300,45]} else {[0,0]}).unwrap());
            if !roots.contains(&other) {assert_eq!(fixture::occurrence(moved,"Other"),fixture::occurrence(&original,"Other"));}
            assert_eq!(fixture::paint(moved,"Ink").raster,fixture::paint(&original,"Ink").raster);
            editor.undo().unwrap();let mut restored=editor.document().clone();restored.revision=original.revision;assert_eq!(restored,original);
        }
        let mut locked=original.clone();fixture::occurrence_mut(&mut locked,"Ink").locked=true;let before=locked.clone();assert!(locked.move_layers_edit(&[group],[1,1]).is_err());assert_eq!(locked,before);
        let mut overflow=original.clone();fixture::occurrence_mut(&mut overflow,"Group").offset=[i64::MAX,0];assert!(overflow.move_layers_edit(&[group],[1,0]).is_err());
    }
}

#[test]
fn layer_move_admission_refuses_adjustment_roots_empty_groups_and_pending_descendants() {
    let mut doc=fixture::document([128,96],&["Ink","Group"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&[]);assert!(doc.layer_move_targets(&[group]).is_err());let adjustment=fixture::insert_paint(&mut doc,"Adjustment",0,Some(group));fixture::effect(&mut doc,"Adjustment","gaussian_blur");assert!(doc.layer_move_targets(&[adjustment]).is_err());assert!(doc.layer_move_targets(&[group]).is_err());fixture::nest(&mut doc,"Group",&["Ink","Adjustment"]);assert_eq!(doc.layer_move_targets(&[group,paint]).unwrap(),vec![group,paint,adjustment]);
    let coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),[128,96],[0;2]);Arc::make_mut(&mut fixture::paint_mut(&mut doc,"Ink").operations).push(RasterOperation{placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Erase{alpha_locked:false}});let before=doc.clone();assert!(doc.move_layers_edit(&[group],[4,0]).is_err());assert_eq!(doc,before);
}

#[test]
fn identity_moves_add_no_history_and_keep_redo() {
    let mut doc=fixture::document([128,96],&["Group","Ink"]);let paint=fixture::id(&doc,"Ink");let group=fixture::nest(&mut doc,"Group",&["Ink"]);fixture::occurrence_mut(&mut doc,"Group").offset=[8192,-4096];
    let mut editor=Editor::new(doc.clone());let mut changed=fixture::occurrence(&doc,"Ink").clone();changed.opacity=0.5;editor.perform(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,paint,Some(changed)).unwrap())).unwrap();assert!(editor.undo().unwrap());
    let before=editor.document().clone();let redo=editor.next_history_edit(true).cloned();let edit=editor.document().move_layers_edit(&[group,paint],[0,0]).unwrap();assert_eq!(edit,Edit::Batch(Vec::new()));
    editor.perform(edit).unwrap();assert_eq!(editor.document(),&before);assert_eq!(editor.next_history_edit(true).cloned(),redo);
}

#[test]
fn transform_operation_meshes_share_one_history_charge() {
    let mesh=Arc::new(MeshMap::identity(Rect::from_extent([128,96]),[3,3]).unwrap());let mut doc=document();let owner=fixture::id(&doc,"Ink");
    let transform=|coverage:CoverageSnapshot|RasterOperation{placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Transform(ImageTransform{placement:LayerPlacement{mesh:Some(mesh.clone()),..Default::default()},..Default::default()})};
    let coverage=CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(),[128,96],[0;2]);Arc::make_mut(&mut fixture::paint_mut(&mut doc,"Ink").operations).push(transform(coverage.clone()));
    let mask=fixture::add_mask(&mut doc,owner,[128,96],[0;2]);doc.artwork.coverage.get_mut(mask).unwrap().operations=vec![transform(coverage)].into();
    let mut roots=RootInventory::default();roots.document(&doc);assert_eq!(roots.meshes.len(),2);assert!(roots.meshes.iter().all(|root|Arc::ptr_eq(root,&mesh)));
    let mut accounting=crate::history_budget::Accounting::new(&doc);assert_eq!(accounting.charge_mesh(&mesh),0);
    fixture::paint_mut(&mut doc,"Ink").operations=Arc::default();doc.artwork.coverage.get_mut(mask).unwrap().operations=Arc::default();let mut stripped=RootInventory::default();stripped.document(&doc);assert!(stripped.meshes.is_empty());
}

