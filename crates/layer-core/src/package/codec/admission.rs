use super::*;

fn paint(artwork: &mut Artwork, domain: [u32;2], original: Option<Arc<SourceImage>>, raster: RasterRevision) {
    let source=artwork.paint.insert(PortableId::random(),PaintSource {domain,original,raster,operations:Default::default()}).unwrap();
    let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),"Paint")).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
}
fn admitted_roundtrip(artwork: &Artwork) {
    let document=crate::Document::from_artwork(artwork.clone()).unwrap();
    document.admit(Default::default()).unwrap();
    let restored=editable(serialize(&prepare(artwork,false)));
    assert_eq!(restored.paint.len(),artwork.paint.len());
    assert_eq!(restored.occurrences.len(),artwork.occurrences.len());
    crate::Document::from_artwork(restored).unwrap().admit(Default::default()).unwrap();
}

#[test]
fn shared_original_references_do_not_consume_evaluation_graph_edges() {
    let mut artwork=Artwork::new([4096;2]).unwrap();
    let original=crate::color::source::rgba8_source([4096;2],|_,_|[32,64,128,255]);
    for _ in 0..1025 {paint(&mut artwork,[4096;2],Some(original.clone()),Default::default());}
    admitted_roundtrip(&artwork);
}

#[test]
fn admitted_layer_tile_and_dependency_boundaries_reopen() {
    let limits=crate::ProjectLimits::default();
    let color=DocumentColor::default();
    let tile=Arc::new(TileBlob::encode(color.paint_descriptor(),&vec![0;256*256*4]).unwrap());
    let raster=RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},RasterTile::backed_shared(tile.clone()))].into(),watercolor:None});
    let mut artwork=Artwork::new([256;2]).unwrap();
    for _ in 0..limits.layers {paint(&mut artwork,[256;2],None,raster.clone());}
    admitted_roundtrip(&artwork);
    paint(&mut artwork,[256;2],None,Default::default());
    assert!(crate::Document::from_artwork(artwork.clone()).unwrap().admit(limits).is_err());
    assert!(PreparedPackage::prepare(&capture(&artwork),None,&AtomicBool::new(false)).is_err());

    let mut artwork=Artwork::new([limits.dimension;2]).unwrap();
    let tile=crate::color::source::rgba8_source([256;2],|_,_|[0;4]).tiles[&[0;2]].clone();
    let original=Arc::new(SourceImage {kind:SourceKind::Original,extent:[limits.dimension;2],resolution:None,
        interpretation:SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},
        tiles:(0..limits.tiles).map(|i|([i as u32%128,i as u32/128],tile.clone())).collect()});
    paint(&mut artwork,[limits.dimension;2],Some(original),Default::default());
    admitted_roundtrip(&artwork);

    let mut artwork=Artwork::new([1;2]).unwrap();
    paint(&mut artwork,[1;2],None,Default::default());
    let root=artwork.compositions.get(artwork.root).unwrap().result;
    for _ in 0..62 {
        let entries=std::mem::take(&mut artwork.stacks.get_mut(root).unwrap().entries);
        let child=artwork.stacks.insert(PortableId::random(),Stack {entries}).unwrap();
        let group=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(child),"Group")).unwrap();
        artwork.stacks.get_mut(root).unwrap().entries.push(group);
    }
    admitted_roundtrip(&artwork);
}

#[test]
fn placement_precision_limits_preserve_but_singular_and_horizon_maps_fail() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for (matrix,preserved) in [([1.,0.,0.,0.,1.,0.,1.,0.,0.0001],true),
        ([0.;9],false),([1.,0.,0.,0.,1.,0.,-1.,0.,1.],false)] {
        let changed=rewrite(&bytes,|manifest| {
            let occurrence=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|r|r["type"]=="capy.occurrence/2").unwrap();
            occurrence["data"]["placement"]=json!({"projective":matrix});
        });
        let outcome=open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap();
        if preserved {assert!(matches!(outcome,OpenOutcome::Preserved {ref outputs,preview:Some(_),..} if outputs.len()==1),"{outcome:?}");}
        else {assert!(matches!(outcome,OpenOutcome::RecoveredView {..}),"{outcome:?}");}
    }
}

#[test]
fn metadata_budget_is_preserved_and_writer_checks_the_same_json_limits() {
    let artwork=Artwork::new([1;2]).unwrap();
    let bytes=serialize(&prepare(&artwork,false));
    assert!(matches!(open(backing(bytes),crate::ProjectLimits {metadata_bytes:128,..Default::default()},&AtomicBool::new(false)).unwrap(),OpenOutcome::Preserved {..}));
    let mut artwork=artwork;
    let mut nested=json!(0);
    for _ in 0..super::super::super::json::MAX_JSON_DEPTH {nested=json!([nested]);}
    Arc::make_mut(&mut artwork.extensions).records.insert(identity(200),json!({"id":identity(200),"type":"example.note/1","ancillary":true,"copy_safe":true,"data":nested}));
    assert!(PreparedPackage::prepare(&capture(&artwork),None,&AtomicBool::new(false)).is_err());
}

#[test]
fn graph_and_record_limits_preserve_known_outputs() {
    let bytes=serialize(&prepare(&Artwork::new([1;2]).unwrap(),true));
    let records=rewrite(&bytes,|manifest| {
        let objects=manifest["objects"].as_array_mut().unwrap();
        for n in 0..crate::authored::GraphLimits::default().objects {
            objects.push(json!({"id":identity(1000+n as u128),"type":"example.future/1","data":{}}));
        }
    });
    let deep=rewrite(&bytes,|manifest| {
        let root=manifest["objects"].as_array().unwrap().iter().find(|r|r["type"]=="capy.stack/1").unwrap()["id"].clone();
        let objects=manifest["objects"].as_array_mut().unwrap();
        let mut stack=root;
        for n in 0..64 {
            let child=identity(1000+2*n);let group=identity(1001+2*n);
            objects.iter_mut().find(|r|r["id"]==stack).unwrap()["data"]["entries"]=json!([resources::reference(group)]);
            objects.push(json!({"id":child,"type":"capy.stack/1","data":{}}));
            objects.push(json!({"id":group,"type":"capy.occurrence/2","data":{"content":{"stack":resources::reference(child)}}}));
            stack=json!(child);
        }
    });
    for changed in [records,deep] {
        let outcome=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap();
        let OpenOutcome::Preserved {source,outputs,preview,..}=outcome else {panic!("{outcome:?}")};
        assert_eq!(outputs.len(),1);assert!(preview.is_some());
        let mut copy=Vec::new();copy_original(&source,&mut copy,&AtomicBool::new(false)).unwrap();assert_eq!(copy,changed);
    }
}

#[test]
fn ruler_mesh_curve_gradient_and_lookup_boundaries_save_and_reopen() {
    let mut artwork=editable(include_bytes!("fixtures/authored-filters.capy").to_vec());
    for handle in artwork.guides.iter().map(|(h,_,_)|h).collect::<Vec<_>>() {
        let guides=artwork.guides.get_mut(handle).unwrap();
        guides.rulers=(0..crate::rulers::MAX_RULERS).map(|n|(identity(5000+n as u128),crate::RulerGeometry::Radial {center:Point {x:n as f32,y:2.}})).collect();
    }
    for handle in artwork.effects.iter().map(|(h,_,_)|h).collect::<Vec<_>>() {
        let application=artwork.effects.get_mut(handle).unwrap();
        for value in &mut application.values {
            match value {
                EffectValue::Curve(points)=>*points=(0..32).map(|n|[n as f32/31.;2]).collect(),
                EffectValue::Gradient(gradient)=>{
                    let color=gradient.stops[0].color;
                    gradient.stops=(0..32).map(|n|crate::GradientStop {position:n as f32/31.,color}).collect();
                },
                EffectValue::Lut3d(Some(lut))=>*lut=Arc::new(Lut3d::from_samples(65,[[0.;3],[1.;3]],"Boundary".into(),vec![[0.25,0.5,0.75];65usize.pow(3)].into()).unwrap()),
                _=>{},
            }
        }
    }
    let occurrence=artwork.occurrences.iter().find(|(_,_,o)|matches!(o.content,OccurrenceContent::Paint(_))).unwrap().0;
    artwork.occurrences.get_mut(occurrence).unwrap().placement=crate::LayerPlacement {mesh:Some(Arc::new(crate::MeshMap::identity(crate::Rect::from_extent([256;2]),[crate::MeshMap::MAX_CELLS;2]).unwrap())),..Default::default()};
    admitted_roundtrip(&artwork);
}

#[test]
fn material_and_ruler_precision_bounds_preserve_future_values() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for (width,length,preserved) in [(17.,1.,true),(0.25,1.,true),(7.,0.0001,true),(0.,1.,false),(7.,0.,false)] {
        let changed=rewrite(&bytes,|manifest| {
            for record in manifest["objects"].as_array_mut().unwrap() {
                if record["type"]=="capy.paint-source/1" && record["data"].get("material").is_some() {
                    record["data"]["material"]["watercolor"]["edge_width"]=json!(width);
                }
                if record["type"]=="capy.guides/1" {
                    record["data"]["rulers"][0]["geometry"]=json!({"kind":"straight","start":[0,0],"end":[length,0]});
                }
            }
        });
        let outcome=open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap();
        if preserved {assert!(matches!(outcome,OpenOutcome::Preserved {ref outputs,..} if outputs.len()==1),"{outcome:?}");}
        else {assert!(matches!(outcome,OpenOutcome::RecoveredView {..}),"{outcome:?}");}
    }
}
