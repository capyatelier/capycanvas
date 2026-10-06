use super::*;

fn paint(artwork: &mut Artwork, domain: [u32;2], original: Option<Image>, raster: RasterRevision) {
    let source=artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain,base:original.map(PaintBase::new),raster,operations:Default::default()}).unwrap();
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
    let original=Image::new(crate::color::source::rgba8_source([4096;2],|_,_|[32,64,128,255]));
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
    let original=Arc::new(SourceImage {extent:[limits.dimension;2],resolution:None,
        interpretation:SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false},
        tiles:(0..limits.tiles).map(|i|([i as u32%128,i as u32/128],tile.clone())).collect()});
    paint(&mut artwork,[limits.dimension;2],Some(Image::new(original)),Default::default());
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
fn image_layer_object_and_name_boundaries_save_and_reopen() {
    let limit=crate::authored::GraphLimits::default().layer_objects;
    let image=Image::new(crate::color::source::rgba8_source([4;2],|_,_|[1,2,3,255]));
    let mut artwork=Artwork::new([64;2]).unwrap();
    let children:Vec<_>=(0..limit).map(|n|artwork.objects.insert(PortableId::random(),
        ImageObject::new(image.clone(),if n==0 {"x".repeat(crate::MAX_NAME_BYTES)} else {String::new()})).unwrap()).collect();
    let layer=artwork.object_layers.insert(PortableId::random(),ObjectLayer {children:children.clone()}).unwrap();
    let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(layer),"é".repeat(crate::MAX_NAME_BYTES/2))).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let document=crate::Document::from_artwork(artwork.clone()).unwrap();
    document.admit(Default::default()).unwrap();
    let bytes=serialize(&prepare(&artwork,false));
    let restored=editable(bytes.clone());
    assert_eq!(restored.objects.len(),limit);
    crate::Document::from_artwork(restored).unwrap().admit(Default::default()).unwrap();

    assert!(document.add_image_object_edit(occurrence,ImageObject::new(image.clone(),""),0).is_err(),"editing refuses one image past the limit");
    let mut over=artwork.clone();
    let extra=over.objects.insert(PortableId::random(),ImageObject::new(image.clone(),"")).unwrap();
    over.object_layers.get_mut(layer).unwrap().children.push(extra);
    assert!(crate::Document::from_artwork(over.clone()).is_err());
    assert!(PreparedPackage::prepare(&capture(&over),None,&AtomicBool::new(false)).is_err());
    let added=rewrite(&bytes,|manifest|{
        let objects=manifest["objects"].as_array_mut().unwrap();
        let mut record=objects.iter().find(|r|r["type"]=="capy.image-object/1").unwrap().clone();
        record["id"]=json!(identity(900_000));
        objects.iter_mut().find(|r|r["type"]=="capy.object-layer/1").unwrap()["data"]["children"].as_array_mut().unwrap().push(resources::reference(identity(900_000)));
        objects.push(record);
    });
    assert!(matches!(open(backing(added),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::Preserved {..}));

    let long="x".repeat(crate::MAX_NAME_BYTES+1);
    assert!(document.rename_image_object_edit(children[1],&long).is_err());
    let mut long_layer=document.scene().occurrence(occurrence).unwrap().clone();long_layer.name=long.as_str().into();
    assert!(document.clone().apply(crate::Edit::Occurrence(crate::RecordChange::replace(&artwork.occurrences,occurrence,Some(long_layer)).unwrap())).is_err(),"editing refuses a layer name past the limit");
    let mut renamed=artwork.clone();renamed.objects.get_mut(children[1]).unwrap().name=long.as_str().into();
    assert!(crate::Document::from_artwork(renamed).is_err());
    let mut layer_named=artwork;layer_named.occurrences.get_mut(occurrence).unwrap().name=long.as_str().into();
    assert!(crate::Document::from_artwork(layer_named).is_err());
    for kind in ["capy.occurrence/3","capy.image-object/1"] {
        let named=rewrite(&bytes,|manifest|{
            manifest["objects"].as_array_mut().unwrap().iter_mut().find(|r|r["type"]==kind).unwrap()["data"]["name"]=json!(long);
        });
        let outcome=open(backing(named),Default::default(),&AtomicBool::new(false)).unwrap();
        assert!(!matches!(outcome,OpenOutcome::Candidate {..}),"{kind}: {outcome:?}");
    }
}

#[test]
fn removed_placement_records_and_fields_are_preserved_as_unsupported() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for legacy in [true,false] {
        let changed=rewrite(&bytes,|manifest| {
            let occurrence=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|r|r["type"]=="capy.occurrence/3" && r["data"].get("mask").is_some()).unwrap();
            let data=occurrence["data"].as_object_mut().unwrap();data.remove("offset");
            data.insert("placement".into(),json!({"translation":[0.25,0],"projective":[1,0,0,0,1,0,0,0,1]}));
            data.get_mut("mask").unwrap().as_object_mut().unwrap().insert("placement".into(),json!({"translation":[3,1]}));
            if legacy {occurrence["type"]="capy.occurrence/2".into();}
        });
        let outcome=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap();
        let OpenOutcome::Preserved {source,ref outputs,preview:Some(_),..}=outcome else {panic!("{outcome:?}")};
        assert_eq!(outputs.len(),1);
        let mut copied=Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();assert_eq!(copied,changed);
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
            objects.push(json!({"id":group,"type":"capy.occurrence/3","data":{"content":{"stack":resources::reference(child)}}}));
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
    artwork.occurrences.get_mut(occurrence).unwrap().offset=[-crate::offsets::MAX_OFFSET,crate::offsets::MAX_OFFSET];
    admitted_roundtrip(&artwork);
}

#[test]
fn material_and_ruler_precision_bounds_preserve_future_values() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for (width,length,preserved) in [(17.,1.,true),(0.25,1.,true),(7.,0.0001,true),(0.,1.,false),(7.,0.,false)] {
        let changed=rewrite(&bytes,|manifest| {
            for record in manifest["objects"].as_array_mut().unwrap() {
                if record["type"]=="capy.paint-source/2" && record["data"].get("material").is_some() {
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

#[test]
fn object_ancestors_preserve_offsets_beyond_admission_and_reject_noncanonical_offsets() {
    let bytes=include_bytes!("fixtures/shared-image-objects.capy");
    for (field,value,supported,invalid) in [
        ("offset",json!(["8","-4"]),true,false),
        ("offset",json!(["16777217","0"]),false,false),
        ("placement",json!({"translation":[8,4]}),false,false),
        ("offset",json!(["08","4"]),false,true),
        ("offset",json!([8,4]),false,true),
    ] {
        let changed=rewrite(bytes,|manifest| {
            let objects=manifest["objects"].as_array_mut().unwrap();
            let stack=objects.iter_mut().find(|record|record["type"]=="capy.stack/1").unwrap();
            let entries=stack["data"]["entries"].clone();
            stack["data"]["entries"]=json!([resources::reference(identity(1001))]);
            objects.push(json!({"id":identity(1000),"type":"capy.stack/1","data":{"entries":entries}}));
            objects.push(json!({"id":identity(1001),"type":"capy.occurrence/3","data":{"content":{"stack":resources::reference(identity(1000))},field:value}}));
        });
        let outcome=open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap();
        if supported {assert!(matches!(outcome,OpenOutcome::Candidate {..}),"{outcome:?}");}
        else if invalid {assert!(matches!(outcome,OpenOutcome::Failure {..}),"{outcome:?}");}
        else {assert!(matches!(outcome,OpenOutcome::Preserved {..}),"{outcome:?}");}
    }
}

#[test]
fn unknown_drawable_children_preserve_the_complete_package() {
    let bytes=include_bytes!("fixtures/shared-image-objects.capy");
    let changed=rewrite(bytes,|manifest| {
        manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["type"]=="capy.image-object/1").unwrap()["type"]=json!("example.path-object/1");
    });
    let OpenOutcome::Preserved {source,..}=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap() else {panic!("Unknown drawable must preserve its owning object layer")};
    let mut copied=Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();assert_eq!(copied,changed);
}

fn collide_image_and_tile(manifest:&mut Value) {
    let tile=manifest["resources"].as_array().unwrap().iter().find(|record|record["type"]=="capy.raster-tile/1").unwrap()["id"].clone();
    let image=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["type"]=="capy.image/1").unwrap();
    let previous=resources::reference_id(&json!({"ref":image["id"]})).unwrap();
    image["id"]=tile.clone();
    let tile=resources::reference_id(&json!({"ref":tile})).unwrap();
    crate::package::remap_references(manifest,&[(previous,tile)].into(),4_194_304).unwrap();
}

#[test]
fn image_identity_cannot_alias_a_tile_in_portable_or_private_transfer() {
    let bytes=include_bytes!("fixtures/shared-image-objects.capy");
    let changed=rewrite(bytes,collide_image_and_tile);
    assert!(matches!(open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::Failure {..}));
    let artwork=editable(bytes.to_vec());
    let transfer=crate::package::transfer::PreparedTransfer::capture(&capture(&artwork),&AtomicBool::new(false)).unwrap();
    let mut descriptor=transfer.descriptor().clone();collide_image_and_tile(&mut descriptor.manifest);
    assert!(crate::package::transfer::TransferReceiver::new(descriptor,Default::default()).is_err());
}

#[test]
fn ancillary_only_images_are_dropped_and_unplaced_hidden_authored_uses_are_kept() {
    let bytes=include_bytes!("fixtures/shared-image-objects.capy");
    for authored_use in [false,true] {
        let changed=rewrite(bytes,|manifest| {
            let records=manifest["objects"].as_array_mut().unwrap();
            let mut image=records.iter().find(|record|record["type"]=="capy.image/1").unwrap().clone();image["id"]=json!(identity(900));records.push(image);
            records.push(json!({"id":identity(901),"type":"example.note/1","ancillary":true,"copy_safe":true,"data":{"image":resources::reference(identity(900))}}));
            if authored_use {
                records.push(json!({"id":identity(903),"type":"capy.image-object/1","data":{"image":resources::reference(identity(900)),"visible":false}}));
                records.push(json!({"id":identity(904),"type":"capy.object-layer/1","data":{"children":[resources::reference(identity(903))]}}));
                records.push(json!({"id":identity(905),"type":"capy.occurrence/3","data":{"content":{"objects":resources::reference(identity(904))}}}));
            }
        });
        let artwork=editable(changed);let prepared=prepare(&artwork,false);
        let manifest:Value=serde_json::from_slice(prepared.manifest()).unwrap();
        let records=manifest["objects"].as_array().unwrap();
        assert_eq!(records.iter().any(|record|record["id"]==json!(identity(900))),authored_use);
        assert_eq!(records.iter().any(|record|record["id"]==json!(identity(901))),authored_use);
        let reopened=editable(serialize(&prepared));
        assert_eq!(reopened.objects.resolve(identity(903)).is_some(),authored_use);
        if authored_use {assert!(!reopened.objects.get(reopened.objects.resolve(identity(903)).unwrap()).unwrap().visible);}
    }
}

#[test]
fn linked_integer_mask_offsets_preserve_packages_beyond_runtime_sum_precision() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for (owner,relative) in [("16777216","1"),("2147483520","128"),("-2147483648","-1")] {
        let changed=rewrite(&bytes,|manifest| {
            let occurrence=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record|record["type"]=="capy.occurrence/3" && record["data"].get("mask").is_some()).unwrap();
            let data=occurrence["data"].as_object_mut().unwrap();data.insert("offset".into(),json!([owner,"0"]));
            let mask=data.get_mut("mask").unwrap().as_object_mut().unwrap();mask.insert("linked".into(),true.into());mask.insert("offset".into(),json!([relative,"0"]));
        });
        let outcome=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap();
        let OpenOutcome::Preserved {source,preview:Some(_),..}=outcome else {panic!("exact mask offset must be preserved: {outcome:?}");};
        let cancel=AtomicBool::new(false);let mut copied=Vec::new();copy_original(&source,&mut copied,&cancel).unwrap();assert_eq!(copied,changed);
    }
}
