fn lookup_effect() -> crate::EffectInstance {
    let mut program=(*crate::bundled_effect_catalog().get("color_lookup").unwrap().program()).clone();
    for parameter in Arc::make_mut(&mut program.parameters) {match parameter.key.as_ref() {"resource"=>parameter.key="table".into(),"color_space"=>parameter.key="space".into(),_=>()}}
    program.auxiliary=Some(crate::EffectAuxiliary::Lut3d {resource:"table".into(),color_space:"space".into()});
    crate::EffectInstance::new(Arc::new(program))
}
use crate::{color::RgbSpace, Lut3d};
use sha2::{Digest, Sha256};
use std::sync::Arc;

fn cube() -> String {
    let mut text = String::from("TITLE \"Independent 🎨\"\nDOMAIN_MAX 1 1 1\nLUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\n");
    for b in 0..2 { for g in 0..2 { for r in 0..2 { text.push_str(&format!("{r} {g} {b}\n")); } } }
    text
}
fn descriptor(lut:&Lut3d) -> Lut3d { serde_json::from_value(serde_json::to_value(lut).unwrap()).unwrap() }
fn constant(domain:[[f32;3];2],sample:[f32;3]) -> Result<Lut3d,&'static str> { Lut3d::from_samples(2,domain,"fixture".into(),vec![sample;8].into()) }

#[test]
fn cube_parser_orders_headers_and_samples_and_refuses_malformed_input() {
    let text=cube();let lut=Lut3d::parse_cube(text.as_bytes()).unwrap();
    assert_eq!(lut.title(),"Independent 🎨");assert_eq!(lut.samples().unwrap().collect::<Vec<_>>(),vec![[0.,0.,0.],[1.,0.,0.],[0.,1.,0.],[1.,1.,0.],[0.,0.,1.],[1.,0.,1.],[0.,1.,1.],[1.,1.,1.]]);
    let crlf=format!(" \t# comment\r\n{}",text.replace('\n'," \t\r\n"));assert_eq!(Lut3d::parse_cube(crlf.as_bytes()).unwrap(),lut);
    for invalid in [text.replace("LUT_3D_SIZE 2","LUT_3D_SIZE 1"),text.replace("LUT_3D_SIZE 2","LUT_3D_SIZE 66"),text.replace("LUT_3D_SIZE 2","LUT_3D_SIZE 2\nLUT_3D_SIZE 2"),text.replace("LUT_3D_SIZE 2","LUT_1D_SIZE 2"),text.replace("DOMAIN_MIN 0 0 0","DOMAIN_MIN nan 0 0"),text.replace("DOMAIN_MIN 0 0 0","DOMAIN_MIN -1e38 0 0"),text.replace("DOMAIN_MAX 1 1 1","DOMAIN_MAX 0 1 1"),text.replace("DOMAIN_MAX 1 1 1","DOMAIN_MAX -1 1 1"),text.replace("DOMAIN_MIN 0 0 0","DOMAIN_MIN 0 0 0 # tail"),format!("0 0 0\n{text}"),format!("{text}0 0 0\n"),format!("{text}DOMAIN_MAX 1 1 1\n"),text.replace("1 1 1\n", "1 1 1 0\n"),text[..text.rfind("1 1 1\n").unwrap()].into(),text.replace("Independent 🎨",&"色".repeat(257)),text.replace("Independent 🎨","bad\"quote"),text.replace("Independent 🎨","bad\u{1}"),text.replace("DOMAIN_MIN 0 0 0",&format!("DOMAIN_MIN 0 0 0{}"," ".repeat(4097)))] { assert!(Lut3d::parse_cube(invalid.as_bytes()).is_err(),"{invalid:.80}"); }
    assert!(Lut3d::parse_cube(&vec![b' ';Lut3d::MAX_TEXT_BYTES+1]).is_err());
    let valid=text.replace("Independent 🎨",&"色".repeat(256));assert!(Lut3d::parse_cube(valid.as_bytes()).is_ok());
}

#[test]
fn canonical_payload_checks_all_six_headers_padding_and_digest() {
    let lut=Lut3d::parse_cube(cube().as_bytes()).unwrap();assert_eq!(Lut3d::HEADER_BYTES,96);assert_eq!(lut.bytes(),224);let bytes=lut.payload().unwrap();
    for record in 0..6 { for component in 0..4 { let mut corrupted=bytes.to_vec();corrupted[record*16+component*4]^=1;let mut json=serde_json::to_value(&lut).unwrap();json["digest"]=serde_json::json!(Vec::from(<[u8;32]>::from(Sha256::digest(&corrupted))));let reference:Lut3d=serde_json::from_value(json).unwrap();assert!(reference.with_payload(&corrupted).is_err(),"header {record}/{component}"); } }
    let mut padding=bytes.to_vec();padding[96+12]=1;assert!(lut.with_payload(&padding).is_err());let mut sample=bytes.to_vec();sample[100]^=1;assert!(lut.with_payload(&sample).is_err());assert!(lut.with_payload(&bytes[..bytes.len()-1]).is_err());
    let owned:Arc<[u8]>=bytes.into();let address=owned.as_ptr();assert_eq!(descriptor(&lut).with_owned_payload(owned).unwrap().payload().unwrap().as_ptr(),address);
    let debug=format!("{lut:?}");assert!(!debug.contains("samples"));assert!(debug.len()<512);
}

#[test]
fn aliases_share_verified_storage_but_keep_titles_and_geometry_identity() {
    let donor=Lut3d::parse_cube(cube().as_bytes()).unwrap();let mut json=serde_json::to_value(&donor).unwrap();assert!(json.get("payload").is_none());assert!(json.get("spaces").is_none());json["title"]=serde_json::json!("Alias");let alias:Lut3d=serde_json::from_value(json.clone()).unwrap();assert!(!alias.accepts(RgbSpace::Srgb));let ready=alias.with_shared_payload(&donor).unwrap();assert!(Arc::ptr_eq(ready.storage().unwrap(),donor.storage().unwrap()));assert_eq!(ready.title(),"Alias");assert_eq!(ready.digest(),donor.digest());for space in RgbSpace::ALL {assert_eq!(ready.accepts(space),donor.accepts(space));}
    assert!(alias.with_shared_payload(&descriptor(&donor)).is_err());
    for (key,value) in [("size",serde_json::json!(3)),("domain",serde_json::json!([[0,0,0],[2,1,1]])),("digest",serde_json::json!(vec![0;32])),("title",serde_json::json!("bad\u{1}"))] {let mut changed=json.clone();changed[key]=value;let changed:Lut3d=serde_json::from_value(changed).unwrap();assert!(changed.with_shared_payload(&donor).is_err());}
    let renamed= Lut3d::from_samples(2,donor.domain(),"other".into(),donor.samples().unwrap().collect::<Vec<_>>().into()).unwrap();assert_eq!(renamed.digest(),donor.digest());
}

#[test]
fn domain_admission_preserves_normal_adjacent_ranges_and_rejects_ftz_loss() {
    for minimum in [f32::MIN_POSITIVE,1e-30,1.,1e30,-1e-30] {let maximum=minimum.next_up();let lut=constant([[minimum;3],[maximum;3]],[0.;3]).unwrap();let read=|record:usize| f32::from_le_bytes(lut.payload().unwrap()[record*16..record*16+4].try_into().unwrap());let exponent=read(3) as i32;let scale=2f64.powi(exponent);let low=read(4);let reciprocal=read(5);assert!(reciprocal.is_finite());for (x,expected) in [(minimum,0.),(maximum,1.)] {assert_eq!(((f64::from(x)*scale) as f32-low)*reciprocal,expected);} }
    for domain in [[[0.;3],[1e-40;3]],[[1.;3],[1.;3]],[[-1e-40;3],[1e-40;3]]] {assert!(constant(domain,[0.;3]).is_err());}
    assert!(constant([[-1e37;3],[1e37;3]],[0.;3]).is_ok());
    let collapsed=cube().replace("DOMAIN_MIN 0 0 0","DOMAIN_MIN 1 1 1").replace("DOMAIN_MAX 1 1 1","DOMAIN_MAX 1.00000001 1.00000001 1.00000001");assert!(Lut3d::parse_cube(collapsed.as_bytes()).is_err());
}

#[test]
fn finite_tables_are_distinct_from_safe_selected_space_admission() {
    let ordinary=constant([[0.;3],[1.;3]],[-2.,0.25,3.]).unwrap();assert!(RgbSpace::ALL.into_iter().all(|s|ordinary.accepts(s)));
    let syntax_valid=constant([[0.;3],[1.;3]],[1e37,0.,0.]).unwrap();assert!(syntax_valid.samples().is_some());assert!(RgbSpace::ALL.into_iter().all(|s|!syntax_valid.accepts(s)));
    let overflow=constant([[0.;3],[1.;3]],[1.1458293e16;3]).unwrap();assert!(!overflow.accepts(RgbSpace::DisplayP3));
    let encoded=RgbSpace::AdobeRgb.encode(f64::from(f32::MAX)*0.6) as f32;let samples:Arc<[[f32;3]]>=(0..8).map(|i|[if i%2==0 {encoded}else{-encoded};3]).collect();let correlated=Lut3d::from_samples(2,[[0.;3],[1.;3]],"correlated".into(),samples).unwrap();assert!(!correlated.accepts(RgbSpace::AdobeRgb));
    assert!(constant([[0.;3],[1.;3]],[f32::NAN;3]).is_err());
}

#[test]
fn typed_resource_schema_requires_one_declared_resource_and_atomic_space_admission() {
    use crate::{EffectParameterKind,EffectValue};
    let mut effect=lookup_effect();effect.validate().unwrap();
    let valid=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());effect.set("table",EffectValue::Lut3d(Some(valid.clone()))).unwrap();assert!(Arc::ptr_eq(effect.lut3d().unwrap(),&valid));
    let before=effect.clone();assert!(effect.set("table",EffectValue::Lut3d(Some(Arc::new(descriptor(&valid))))).is_err());assert_eq!(effect,before);
    let mut undeclared=effect.clone();Arc::make_mut(&mut undeclared.program).auxiliary=None;assert!(undeclared.validate().is_err());
    let mut duplicated=effect.clone();let parameter=duplicated.program.parameters[0].clone();let mut parameters=duplicated.program.parameters.to_vec();let mut second=parameter;second.key="second_table".into();parameters.push(second);Arc::make_mut(&mut duplicated.program).parameters=parameters.into();duplicated.values.push(EffectValue::Lut3d(None));assert!(duplicated.validate().is_err());
    let mut wrong_space=effect.clone();let parameters=Arc::make_mut(&mut Arc::make_mut(&mut wrong_space.program).parameters);let EffectParameterKind::Choice {options}=&mut parameters[1].kind else {panic!()};let options=Arc::make_mut(options);options.swap(0,1);assert!(wrong_space.validate().is_err());
}

#[test]
fn maximum_table_has_bounded_canonical_storage_and_descriptor_only_json() {
    let lut=Lut3d::from_samples(65,[[0.;3],[1.;3]],"maximum".into(),vec![[0.25,0.5,0.75];65usize.pow(3)].into()).unwrap();assert_eq!(lut.bytes(),96+65usize.pow(3)*16);assert_eq!(lut.samples().unwrap().count(),65usize.pow(3));assert!(serde_json::to_vec(&lut).unwrap().len()<512);assert!(RgbSpace::ALL.into_iter().all(|s|lut.accepts(s)));assert!(Lut3d::from_samples(65,[[0.;3],[1.;3]],"missing".into(),vec![[0.;3];8].into()).is_err());
}

fn resource_project() -> crate::Project {
    use crate::{Document,DocumentNames,Layer,LayerKind,EffectValue};
    let resource=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());let mut document=Document::new("resources",64,48,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    for title in ["First","Alias"] {let mut json=serde_json::to_value(resource.as_ref()).unwrap();json["title"]=serde_json::json!(title);let alias:Lut3d=serde_json::from_value(json).unwrap();let alias=Arc::new(alias.with_shared_payload(&resource).unwrap());let mut effect=lookup_effect();effect.set("table",EffectValue::Lut3d(Some(alias.clone()))).unwrap();Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[0].default=EffectValue::Lut3d(Some(alias));let mut layer=Layer::paint(document.allocate_layer_id(),title);layer.kind=LayerKind::Effect;layer.effect=Some(Arc::new(effect));document.layers.insert(0,layer);}
    crate::Project {document}
}
fn rewrite_archive(bytes:&[u8],change:impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let length=u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;let mut manifest=serde_json::from_slice(&bytes[52..52+length]).unwrap();change(&mut manifest);let json=serde_json::to_vec(&manifest).unwrap();let mut result=bytes[..12].to_vec();result.extend((json.len() as u64).to_le_bytes());result.extend(Sha256::digest(&json));result.extend(json);result.extend(&bytes[52+length..]);result
}

#[test]
fn archive_and_private_transport_restore_values_defaults_and_deduplicate_aliases() {
    let project=resource_project();project.validate(Default::default()).unwrap();let mut document=project.document.clone();let (index,payloads)=crate::ProjectResources::detach(&mut document).unwrap();assert_eq!(payloads.len(),1);assert_eq!(index.validate(&document,224).unwrap(),224);assert!(index.validate(&document,223).is_err());assert_eq!(serde_json::to_value(&index).unwrap()["bindings"].as_array().unwrap().len(),4);
    index.attach(&mut document,&[payloads[0].as_ref().clone()]).unwrap();assert_eq!(document.layers,project.document.layers);
    let mut bytes=Vec::new();project.write(&mut bytes).unwrap();let reopened=crate::Project::read(bytes.as_slice(),Default::default()).unwrap();assert_eq!(reopened.document.layers,project.document.layers);
    let a=reopened.document.layers[0].effect.as_ref().unwrap();let b=reopened.document.layers[1].effect.as_ref().unwrap();assert!(Arc::ptr_eq(a.lut3d().unwrap().storage().unwrap(),b.lut3d().unwrap().storage().unwrap()));assert_eq!(a.resources().count(),2);for resource in a.resources(){assert!(Arc::ptr_eq(resource.storage().unwrap(),a.lut3d().unwrap().storage().unwrap()));}
    assert!(crate::Project::read(bytes.as_slice(),crate::ProjectLimits {asset_bytes:223,..Default::default()}).is_err());assert!(crate::Project::read(bytes.as_slice(),crate::ProjectLimits {asset_bytes:224,..Default::default()}).is_ok());
    let mut corrupted=bytes.clone();*corrupted.last_mut().unwrap()^=1;assert!(crate::Project::read(corrupted.as_slice(),Default::default()).is_err());assert!(crate::Project::read(&bytes[..bytes.len()-1],Default::default()).is_err());
}

#[test]
fn malformed_resource_bindings_are_rejected_before_payload_hydration() {
    let mut bytes=Vec::new();resource_project().write(&mut bytes).unwrap();
    for mutation in [
        |m:&mut serde_json::Value|{m["resources"]["bindings"][0]["payload"]=999.into();},
        |m:&mut serde_json::Value|{m["resources"]["bindings"][0]["target"]["layer"]=999.into();},
        |m:&mut serde_json::Value|{m["resources"]["bindings"][0]["target"]["key"]="missing".into();},
        |m:&mut serde_json::Value|{m["resources"]["bindings"][1]=m["resources"]["bindings"][0].clone();},
        |m:&mut serde_json::Value|{m["resources"]["bindings"][1]["descriptor"]["domain"]=serde_json::json!([[0,0,0],[2,1,1]]);},
        |m:&mut serde_json::Value|{m["resources"]["payloads"][0]["bytes"]=u64::MAX.into();},
        |m:&mut serde_json::Value|{m["resources"]["payloads"][0]["offset"]=1.into();},
        |m:&mut serde_json::Value|{m["document"]["layers"][0]["effect"]["values"]=serde_json::json!([]);},
    ] {let changed=rewrite_archive(&bytes,mutation);assert!(crate::Project::read(changed.as_slice(),Default::default()).is_err());}
}

#[test]
fn undo_and_pending_operations_retain_and_charge_shared_resources_once() {
    use crate::{Edit,Editor,EffectValue,LayerOperation,LayerOperationKind,LayerMask,LayerId,Point,Affine};
    let mut document=resource_project().document;document.layers.remove(1);let weak=Arc::downgrade(document.layers[0].effect.as_ref().unwrap().lut3d().unwrap().storage().unwrap());let mut editor=Editor::new(document);let mut replacement=editor.document().layers[0].clone();let effect=Arc::make_mut(replacement.effect.as_mut().unwrap());effect.set("table",EffectValue::Lut3d(None)).unwrap();Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[0].default=EffectValue::Lut3d(None);editor.perform(Edit::ReplaceLayer(Box::new(replacement))).unwrap();assert!(weak.upgrade().is_some());assert!(editor.undo().unwrap());assert_eq!(editor.document().layers[0].effect.as_ref().unwrap().lut3d().unwrap().storage().unwrap().as_ptr(),weak.upgrade().unwrap().as_ptr());assert!(editor.redo().unwrap());assert!(weak.upgrade().is_some());drop(editor);assert!(weak.upgrade().is_none());
    let project=resource_project();let layer=project.document.layers[0].clone();let resource=layer.effect.as_ref().unwrap().lut3d().unwrap().clone();
    let mut document=crate::Document::new("pending",64,48,crate::DocumentNames {paint:"Paint".into(),paper:"Paper".into()});document.layers[0].pending_operations.push(LayerOperation {placement:Affine::default(),coverage:LayerMask::reveal_all(LayerId(50),Point::default()),kind:LayerOperationKind::Bake {members:vec![layer.clone(),layer].into(),offset:Point::default()}});let pending=crate::Project {document};assert!(pending.validate(crate::ProjectLimits {asset_bytes:resource.bytes() as u64-1,..Default::default()}).is_err());pending.validate(crate::ProjectLimits {asset_bytes:resource.bytes() as u64,..Default::default()}).unwrap();
}

#[test]
fn resource_ownership_edits_require_admission_but_intensity_and_title_aliases_do_not() {
    use crate::{Edit,EffectValue};
    let project=resource_project();let layer=project.document.layers[0].clone();assert!(Edit::InsertLayer {index:0,layer:Box::new(layer.clone())}.requires_history_admission(&project.document));assert!(Edit::RemoveLayer {id:layer.id}.requires_history_admission(&project.document));
    let mut changed=layer.clone();Arc::make_mut(changed.effect.as_mut().unwrap()).set("intensity",EffectValue::Number(42.)).unwrap();assert!(!Edit::ReplaceLayer(Box::new(changed)).requires_history_admission(&project.document));
    let resource=layer.effect.as_ref().unwrap().lut3d().unwrap();let mut json=serde_json::to_value(resource.as_ref()).unwrap();json["title"]="Renamed resource".into();let alias:Lut3d=serde_json::from_value(json).unwrap();let alias=Arc::new(alias.with_shared_payload(resource).unwrap());let mut changed=layer.clone();Arc::make_mut(changed.effect.as_mut().unwrap()).set("table",EffectValue::Lut3d(Some(alias))).unwrap();assert!(!Edit::ReplaceLayer(Box::new(changed)).requires_history_admission(&project.document));
    let mut changed=layer.clone();Arc::make_mut(changed.effect.as_mut().unwrap()).set("table",EffectValue::Lut3d(None)).unwrap();assert!(Edit::ReplaceLayer(Box::new(changed)).requires_history_admission(&project.document));
    let mut changed=layer;let new=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());Arc::make_mut(changed.effect.as_mut().unwrap()).set("table",EffectValue::Lut3d(Some(new))).unwrap();assert!(Edit::ReplaceLayer(Box::new(changed)).requires_history_admission(&project.document));
}

#[test]
fn asset_budget_deduplicates_physical_resource_and_source_ownership() {
    let mut project=resource_project();assert_eq!(crate::project::asset_bytes(&project.document),224);let source=crate::color::source::rgba8_source([2,1],|_,_|[40,50,60,255]);let source_bytes=std::mem::size_of::<crate::color::source::SourceImage>()+source.tiles.len()*96+source.tiles.values().map(|t|t.compressed_len()).sum::<usize>();project.document.layers[0].source=Some(source.clone());project.document.layers[1].source=Some(source);assert_eq!(crate::project::asset_bytes(&project.document),224+source_bytes as u64);
    let independent=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());let effect=Arc::make_mut(project.document.layers[0].effect.as_mut().unwrap());effect.set("table",crate::EffectValue::Lut3d(Some(independent))).unwrap();assert_eq!(crate::project::asset_bytes(&project.document),448+source_bytes as u64);
}

#[test]
fn filename_fallback_preserves_embedded_titles_and_bounds_unicode() {
    let explicit=Lut3d::parse_cube_named(cube().as_bytes(),"ignored.cube").unwrap();assert_eq!(explicit.title(),"Independent 🎨");let untitled=cube().lines().skip(1).collect::<Vec<_>>().join("\n");let filename=format!("\u{1}{}\n.cube","色".repeat(300));let named=Lut3d::parse_cube_named(untitled.as_bytes(),&filename).unwrap();assert_eq!(named.title(),"色".repeat(256));assert_eq!(named.digest(),Lut3d::parse_cube(untitled.as_bytes()).unwrap().digest());
}

#[test]
fn private_transport_attach_refuses_late_bad_donor_without_partial_mutation() {
    let mut document=resource_project().document;let distinct=Arc::new(constant([[0.;3],[1.;3]],[0.75;3]).unwrap());Arc::make_mut(document.layers[0].effect.as_mut().unwrap()).set("table",crate::EffectValue::Lut3d(Some(distinct))).unwrap();let (index,payloads)=crate::ProjectResources::detach(&mut document).unwrap();assert_eq!(payloads.len(),2);let before=document.clone();let mut resources=payloads.iter().map(|r|r.as_ref().clone()).collect::<Vec<_>>();resources[1]=descriptor(&resources[1]);assert!(index.attach(&mut document,&resources).is_err());assert_eq!(document,before);
}

#[test]
fn resolve_input_range_matches_domain_headers_and_refuses_ambiguous_shapers() {
    let domain=cube().replace("DOMAIN_MAX 1 1 1","DOMAIN_MAX 2 2 2").replace("DOMAIN_MIN 0 0 0","DOMAIN_MIN -1 -1 -1");
    let resolve=cube().replace("DOMAIN_MAX 1 1 1","LUT_3D_INPUT_RANGE -1 2").replace("DOMAIN_MIN 0 0 0\n","");
    let expected=Lut3d::parse_cube(domain.as_bytes()).unwrap();let actual=Lut3d::parse_cube(resolve.as_bytes()).unwrap();assert_eq!(actual,expected);
    let reordered=resolve.replace("LUT_3D_INPUT_RANGE -1 2\n","").replace("LUT_3D_SIZE 2","LUT_3D_SIZE 2\nLUT_3D_INPUT_RANGE -1 2");assert_eq!(Lut3d::parse_cube(reordered.as_bytes()).unwrap(),expected);
    for invalid in [resolve.replace("LUT_3D_INPUT_RANGE -1 2","LUT_3D_INPUT_RANGE -1 2\nLUT_3D_INPUT_RANGE -1 2"),resolve.replace("LUT_3D_INPUT_RANGE -1 2","LUT_3D_INPUT_RANGE -1 2\nDOMAIN_MIN -1 -1 -1"),format!("DOMAIN_MAX 2 2 2\n{resolve}"),format!("{resolve}LUT_3D_INPUT_RANGE -1 2\n"),resolve.replace("-1 2","2 -1"),resolve.replace("-1 2","1 1"),resolve.replace("-1 2","nan 2"),resolve.replace("-1 2","-1 inf"),resolve.replace("-1 2","-1 2 3"),format!("LUT_1D_SIZE 2\n{resolve}"),format!("LUT_1D_INPUT_RANGE 0 1\n{resolve}")] {assert!(Lut3d::parse_cube(invalid.as_bytes()).is_err(),"{invalid}");}
}

#[test]
fn builtin_looks_share_finite_payloads_and_preserve_black_white_endpoints() {
    use crate::lut3d::Look;
    for look in Look::ALL {
        let start=std::time::Instant::now();let resource=look.resource();eprintln!("{look:?} first generation: {:?}",start.elapsed());
        assert!(Arc::ptr_eq(&resource,&look.resource()));assert_eq!(Look::for_resource(&resource),Some(look));
        let samples=resource.samples().unwrap().collect::<Vec<_>>();assert_eq!(samples.first(),Some(&[0.;3]));assert_eq!(samples.last(),Some(&[1.;3]));assert!(samples.iter().flatten().all(|v|v.is_finite() && (0.0..=1.0).contains(v)));
        if look==Look::Monochrome {assert!(samples.iter().all(|v|v[0]==v[1] && v[1]==v[2]));}
        assert!(resource.accepts(RgbSpace::Srgb));let hydrated=descriptor(&resource).with_payload(resource.payload().unwrap()).unwrap();assert_eq!(Look::for_resource(&hydrated),Some(look));
    }
}

#[test]
fn lookup_resource_identity_survives_aliases_and_saved_payload_installation() {
    let original = Lut3d::parse_cube(cube().as_bytes()).unwrap();
    let resource = original.resource().unwrap();
    let id = resource.id();
    let saved = crate::authored::Resource::with_id(id, original.storage().unwrap().clone());
    let restored = Lut3d::from_resource(original.size(), original.domain(), original.title().into(), saved).unwrap();
    assert_eq!(restored, original);
    assert_eq!(restored.resource().unwrap().id(), id);
    assert!(Arc::ptr_eq(restored.storage().unwrap(), original.storage().unwrap()));
    let alias = descriptor(&original).with_shared_payload(&restored).unwrap();
    assert!(alias.resource().unwrap().same_owner(restored.resource().unwrap()));
    let mut damaged = original.payload().unwrap().to_vec();
    damaged[Lut3d::HEADER_BYTES + 12] = 1;
    assert!(Lut3d::from_resource(original.size(), original.domain(), original.title().into(), damaged.into()).is_err());
}
