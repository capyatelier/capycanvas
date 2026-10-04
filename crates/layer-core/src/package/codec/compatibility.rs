use super::*;

const SAVED: &[u8] = include_bytes!("fixtures/authored-v1.capy");

#[test]
fn saved_v1_artwork_retains_authored_values_resources_and_current_builtin_controls() {
    let artwork=editable(SAVED.to_vec());
    let document=crate::Document::from_artwork(artwork.clone()).unwrap();
    assert_eq!(document.composition().color.depth,SampleDepth::F32);
    assert_eq!(document.output().sdr.exposure,0.5);
    let material=artwork.paint.iter().next().unwrap().2.raster.wait_data().unwrap().watercolor.unwrap();
    assert_eq!(material,RasterWatercolor {wet_edge:0.25,burnt_edge:0.75,edge_width:3.5});
    let directory=Directory::read(&mut Cursor::new(SAVED),262144,64*1024*1024).unwrap();
    let manifest:Value=serde_json::from_slice(&directory.read_member(&mut Cursor::new(SAVED),directory.member("manifest.json").unwrap(),64*1024*1024).unwrap()).unwrap();
    assert!(!manifest["resources"].as_array().unwrap().iter().any(|r|r["type"]=="capy.wgsl/1"));
    let mut builtin_ids=BTreeSet::new();
    for (_,id,definition) in artwork.definitions.iter() {
        let builtin=crate::bundled_effect_catalog().get(&definition.program.id).unwrap();
        builtin_ids.insert(builtin.id());
        assert!(Arc::ptr_eq(&definition.program,&builtin.program()));
        let record=manifest["objects"].as_array().unwrap().iter().find(|r|r["id"]==json!(id)).unwrap();
        assert_eq!(record["data"],json!({"builtin":builtin.id(),"version":1}));
    }
    assert_eq!(builtin_ids.len(),52);
    for (_,id,application) in artwork.effects.iter() {
        let program=&artwork.definitions.get(application.definition).unwrap().program;
        let record=manifest["objects"].as_array().unwrap().iter().find(|r|r["id"]==json!(id)).unwrap();
        let gradient_v1=matches!(program.id.as_ref(),"gradient_map"|"gradient_fill");
        assert_eq!(record["data"]["values"].as_object().map_or(0,|v|v.len()),program.parameters.len()+usize::from(gradient_v1));
        if gradient_v1 {
            let Some(EffectValue::Gradient(gradient))=crate::EffectView::new(program,&application.values).value("gradient") else {panic!()};
            assert_eq!(gradient.interpolation,crate::ColorMixSpace::Classic);
        }
        if program.id.as_ref()=="color_lookup" && let Some(EffectValue::Lut3d(Some(lut)))=crate::EffectView::new(program,&application.values).value("resource") {
            assert_eq!(lut.bytes(),96);
            assert!(lut.samples().unwrap().all(|sample|sample==[0.125,0.75,0.5]));
        }
    }
    let reopened=editable(serialize(&prepare(&artwork,false)));
    assert_eq!(prepare(&reopened,false).manifest(),prepare(&artwork,false).manifest());
    super::roundtrip_semantics::exact_rasters(&artwork,&reopened);
    let mut edited=reopened;
    let handles:Vec<_>=edited.effects.iter().map(|(handle,_,_)|handle).collect();
    for handle in handles {
        let application=edited.effects.get_mut(handle).unwrap();
        let program=edited.definitions.get(application.definition).unwrap().program.clone();
        if program.id.as_ref()=="exposure" {
            let mut draft=EffectInstance {program,values:application.values.clone()};
            draft.set("exposure",EffectValue::Number(37.25)).unwrap();application.values=draft.values;
        }
    }
    let reopened=editable(serialize(&prepare(&edited,false)));
    let exposure=reopened.effects.iter().find(|(_,_,e)|reopened.definitions.get(e.definition).unwrap().program.id.as_ref()=="exposure").unwrap().2;
    let program=&reopened.definitions.get(exposure.definition).unwrap().program;
    assert_eq!(crate::EffectView::new(program,&exposure.values).value("exposure"),Some(&EffectValue::Number(37.25)));
}

#[test]
fn saved_numeric_bounds_remain_accepted_independently_of_slider_ranges() {
    let bounds:Value=serde_json::from_str(include_str!("fixtures/parameter-bounds-v1.json")).unwrap();
    for (id,parameters) in bounds.as_object().unwrap() {
        let builtin=crate::bundled_effect_catalog().get(id).unwrap();
        for (key,range) in parameters.as_object().unwrap() {
            let parameter=builtin.program.parameters.iter().find(|p|p.key.as_ref()==key).unwrap();
            for number in range.as_array().unwrap() {parameter.validate(&EffectValue::Number(number.as_f64().unwrap() as f32)).unwrap();}
        }
    }
    assert_eq!(crate::package::RASTER_TILE_SIZE,256);
    assert_eq!(crate::package::SELECTION_CHUNK_BYTES,65536);
}
