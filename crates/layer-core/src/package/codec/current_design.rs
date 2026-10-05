use super::*;

const SAVED: &[u8] = include_bytes!("fixtures/authored-filters.capy");

#[test]
fn saved_lookup_admission_preserves_original_bytes_without_latching_corruption() {
    for (field, value) in [("domain", json!(([[1e-40;3],[2e-40;3]]))),
        ("domain", json!(([[-1e38;3],[1e38;3]]))), ("size", json!(66))] {
        let changed = rewrite(SAVED, |manifest| {
            let record = manifest["resources"].as_array_mut().unwrap().iter_mut().find(|r| r["type"] == "capy.lut3d/1").unwrap();
            record["data"][field] = value;
        });
        let original = backing(changed.clone());
        let outcome = open(original.clone(), Default::default(), &AtomicBool::new(false)).unwrap();
        assert!(matches!(outcome, OpenOutcome::Preserved {..}), "{outcome:?}");
        let mut copied = Vec::new();
        copy_original(&original, &mut copied, &AtomicBool::new(false)).unwrap();
        assert_eq!(copied, changed);
    }
    for (field, value) in [("domain", json!(([[1.;3],[1.;3]]))), ("size", json!(3)), ("size", json!(1))] {
        let changed = rewrite(SAVED, |manifest| {
            let record = manifest["resources"].as_array_mut().unwrap().iter_mut().find(|r| r["type"] == "capy.lut3d/1").unwrap();
            record["data"][field] = value;
        });
        assert!(matches!(open(backing(changed), Default::default(), &AtomicBool::new(false)).unwrap(), OpenOutcome::Failure {..}));
    }
}

#[test]
fn saved_artwork_retains_authored_values_resources_and_current_builtin_controls() {
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
        assert_eq!(record["data"],json!({"builtin":builtin.id(),"version":if matches!(builtin.id(),"gradient_map"|"gradient_fill"|"denoise"|"domain_warp"|"posterize"|"kaleidoscope"){2}else{1}}));
    }
    assert_eq!(builtin_ids.len(),52);
    let objects=manifest["objects"].as_array().unwrap();
    let occurrence=|name:&str|&objects.iter().find(|r|r["type"]=="capy.occurrence/2" && r["data"]["name"]==name).unwrap()["data"];
    assert_eq!(objects.iter().filter_map(|r|r["data"]["blend"].as_str()).collect::<BTreeSet<_>>().len(),24);
    assert_eq!([&occurrence("Original source")["attachment"],&occurrence("color_lookup")["attachment"],&occurrence("Fills")["blend"]],["clip","effect","pass_through"]);
    let placement=&occurrence("Independent copy")["placement"];
    assert_eq!([placement["interpolation"].as_str(),placement["mesh"]["frame"].as_array().map(|_|"mesh")],[Some("bicubic"),Some("mesh")]);
    let kinds=objects.iter().filter(|r|r["type"]=="capy.guides/1").flat_map(|r|r["data"]["rulers"].as_array().unwrap()).map(|r|r["geometry"]["kind"].as_str().unwrap()).collect::<BTreeSet<_>>();
    assert_eq!(kinds,["parallel","radial","straight"].into());
    assert!(objects.iter().any(|r|r["data"]["shape"]["contours"].is_array() && r["data"]["inverted"]==true));
    let output=&objects.iter().find(|r|r["type"]=="capy.output/1").unwrap()["data"];
    assert_eq!([&output["proof"]["intent"],&output["sdr"]["balance"]],[&json!("perceptual"),&json!(-0.25)]);
    let curves=artwork.effects.iter().map(|(_,_,e)|crate::EffectView::new(&artwork.definitions.get(e.definition).unwrap().program,&e.values)).find(|e|e.program.id.as_ref()=="curves").unwrap();
    assert_eq!(curves.choice("domain"),Some("log_hdr"));
    assert!(crate::CURVE_KEYS.iter().all(|key|matches!(curves.value(key),Some(EffectValue::Curve(_)))));
    for (_,id,application) in artwork.effects.iter() {
        let program=&artwork.definitions.get(application.definition).unwrap().program;
        let record=manifest["objects"].as_array().unwrap().iter().find(|r|r["id"]==json!(id)).unwrap();
        assert_eq!(record["data"]["values"].as_object().map_or(0,|v|v.len()),program.parameters.len());
        if let Some(expected)=match program.id.as_ref() {"gradient_map"=>Some(crate::ColorMixSpace::LinearRgb),"gradient_fill"=>Some(crate::ColorMixSpace::Oklab),_=>None} {
            let Some(EffectValue::Gradient(gradient))=crate::EffectView::new(program,&application.values).value("gradient") else {panic!()};
            assert_eq!(gradient.interpolation,expected);
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
    let contracts:Value=serde_json::from_str(include_str!("fixtures/builtin-contracts.json")).unwrap();
    for (id,contract) in contracts.as_object().unwrap() {
        let builtin=crate::bundled_effect_catalog().get(id).unwrap();
        for (key,saved) in contract["parameters"].as_object().unwrap() {
            if saved["kind"]["kind"]!="number" {continue;}
            let parameter=builtin.program.parameters.iter().find(|p|p.key.as_ref()==key).unwrap();
            for bound in ["min","max"] {parameter.validate(&EffectValue::Number(saved["kind"][bound].as_f64().unwrap() as f32)).unwrap();}
        }
    }
    assert_eq!(crate::package::RASTER_TILE_SIZE,256);
    assert_eq!(crate::package::SELECTION_CHUNK_BYTES,65536);
}

#[test]
fn pixel_lengths_reopen_and_evaluate_without_catalog_clamping() {
    let mut artwork=editable(SAVED.to_vec());
    let handles:Vec<_>=artwork.effects.iter().map(|(handle,_,_)|handle).collect();
    for handle in handles {
        let application=artwork.effects.get_mut(handle).unwrap();
        let program=&artwork.definitions.get(application.definition).unwrap().program;
        for (parameter,value) in program.parameters.iter().zip(&mut application.values) {
            if matches!(parameter.dimension,Dimension::SourcePixels|Dimension::CompositionPixels) {*value=EffectValue::Number(120.);}
        }
    }
    let reopened=editable(serialize(&prepare(&artwork,false)));
    for (_,_,application) in reopened.effects.iter() {
        let mut program=reopened.definitions.get(application.definition).unwrap().program.clone();
        for parameter in Arc::make_mut(&mut Arc::make_mut(&mut program).parameters) {
            if matches!(parameter.dimension,Dimension::SourcePixels|Dimension::CompositionPixels) {
                if let crate::EffectParameterKind::Number {max,..}=&mut parameter.kind {*max=100.;}
                parameter.soft_bounds=None;
            }
        }
        let view=crate::EffectView::new(&program,&application.values);
        let mut slot=1;
        for (parameter,value) in program.parameters.iter().zip(&application.values) {
            if matches!(parameter.dimension,Dimension::SourcePixels|Dimension::CompositionPixels) {
                assert_eq!(*value,EffectValue::Number(120.));
                assert_eq!(view.gpu_parameters(RgbSpace::Srgb).unwrap()[slot][0],120.);
            }
            slot+=match parameter.kind {crate::EffectParameterKind::Curve|crate::EffectParameterKind::Gradient=>crate::EFFECT_TABLE_VECTORS,_=>1};
        }
    }
}

#[test]
fn saved_reduced_color_layers_keep_modes_channel_counts_and_samples() {
    let artwork = editable(include_bytes!("fixtures/layer-color-modes.capy").to_vec());
    let modes: Vec<_> = artwork.paint.iter().map(|(_, _, p)| p.color_mode).collect();
    assert_eq!(modes, [crate::color::LayerColorMode::Grayscale, crate::color::LayerColorMode::TwoTone]);
    for (_, _, source) in artwork.paint.iter() {
        let data = source.raster.wait_data().unwrap();
        let tile = data.tiles.iter().find(|(k, _)| k.plane == RasterPlane::Color).unwrap().1;
        assert_eq!(tile.descriptor().channels, 2);
        assert_eq!(&tile.wait_backing().unwrap().decode().unwrap()[..4], if source.color_mode == crate::color::LayerColorMode::Grayscale { &[0, 64, 0, 128] } else { &[255; 4] });
    }
    let loaded = editable(serialize(&prepare(&artwork, false)));
    assert_eq!(loaded.paint.iter().map(|(_, _, p)| p.color_mode).collect::<Vec<_>>(), modes);
    for ((_, _, a), (_, _, b)) in artwork.paint.iter().zip(loaded.paint.iter()) {
        assert_eq!(a.raster.wait_data().unwrap().tiles.keys().collect::<Vec<_>>(), b.raster.wait_data().unwrap().tiles.keys().collect::<Vec<_>>());
        for (key, tile) in &a.raster.wait_data().unwrap().tiles { assert_eq!(tile.wait_backing().unwrap().decode().unwrap(), b.raster.wait_data().unwrap().tiles[key].wait_backing().unwrap().decode().unwrap()); }
    }
}
