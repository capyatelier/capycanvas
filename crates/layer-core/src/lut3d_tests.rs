fn lookup_effect() -> crate::EffectInstance {
    let mut program=(*crate::effect_catalog::custom_program("color_lookup")).clone();
    for parameter in Arc::make_mut(&mut program.parameters) {match parameter.key.as_ref() {"resource"=>parameter.key="table".into(),"color_space"=>parameter.key="space".into(),_=>()}}
    program.auxiliary=Some(crate::EffectAuxiliary::Lut3d {resource:"table".into(),color_space:"space".into()});
    crate::EffectInstance::new(Arc::new(program))
}
use crate::{color::RgbSpace, Lut3d};
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
fn canonical_payload_is_tightly_packed_rgb_and_checks_digest() {
    let lut=Lut3d::parse_cube(cube().as_bytes()).unwrap();
    assert_eq!(lut.bytes(),8*3*4);
    let bytes=lut.payload().unwrap();
    let expected:Vec<_>=lut.samples().unwrap().flatten().flat_map(f32::to_le_bytes).collect();
    assert_eq!(bytes,expected);
    let mut sample=bytes.to_vec();sample[4]^=1;
    assert!(lut.with_payload(&sample).is_err());
    assert!(lut.with_payload(&bytes[..bytes.len()-1]).is_err());
    sample[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(Lut3d::from_resource(2,lut.domain(),"bad".into(),sample.into()).is_err());
    let owned:Arc<[u8]>=bytes.into();let address=owned.as_ptr();
    assert_eq!(descriptor(&lut).with_owned_payload(owned).unwrap().payload().unwrap().as_ptr(),address);
}

#[test]
fn aliases_share_verified_storage_but_keep_titles_and_geometry_identity() {
    let donor=Lut3d::parse_cube(cube().as_bytes()).unwrap();let mut json=serde_json::to_value(&donor).unwrap();assert!(json.get("payload").is_none());assert!(json.get("spaces").is_none());json["title"]=serde_json::json!("Alias");let alias:Lut3d=serde_json::from_value(json.clone()).unwrap();assert!(!alias.accepts(RgbSpace::Srgb));let ready=alias.with_shared_payload(&donor).unwrap();assert!(Arc::ptr_eq(ready.storage().unwrap(),donor.storage().unwrap()));assert_eq!(ready.title(),"Alias");assert_eq!(ready.digest(),donor.digest());for space in RgbSpace::ALL {assert_eq!(ready.accepts(space),donor.accepts(space));}
    assert!(alias.with_shared_payload(&descriptor(&donor)).is_err());
    for (key,value) in [("size",serde_json::json!(3)),("domain",serde_json::json!([[0,0,0],[0,1,1]])),("digest",serde_json::json!(vec![0;32])),("title",serde_json::json!("bad\u{1}"))] {let mut changed=json.clone();changed[key]=value;let changed:Lut3d=serde_json::from_value(changed).unwrap();assert!(changed.with_shared_payload(&donor).is_err());}
    let renamed= Lut3d::from_samples(2,donor.domain(),"other".into(),donor.samples().unwrap().collect::<Vec<_>>().into()).unwrap();assert_eq!(renamed.digest(),donor.digest());
}

#[test]
fn domain_admission_preserves_normal_adjacent_ranges_and_rejects_ftz_loss() {
    for minimum in [f32::MIN_POSITIVE,1e-30,1.,1e30,-1e-30] {
        let domain=[[minimum;3],[minimum.next_up();3]];
        assert_eq!(constant(domain,[0.;3]).unwrap().domain(),domain);
    }
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
    let mut wrong_space=effect.clone();let parameters=Arc::make_mut(&mut Arc::make_mut(&mut wrong_space.program).parameters);let EffectParameterKind::Choice {options}=&mut parameters[1].kind else {panic!()};let options=Arc::make_mut(options);options.swap(0,1);wrong_space.validate().unwrap();
    assert_eq!(wrong_space.gpu_parameters(RgbSpace::Srgb).unwrap()[2][0],RgbSpace::DisplayP3.shader_code() as f32);
}

#[test]
fn maximum_table_has_bounded_canonical_storage_and_descriptor_only_json() {
    let lut=Lut3d::from_samples(65,[[0.;3],[1.;3]],"maximum".into(),vec![[0.25,0.5,0.75];65usize.pow(3)].into()).unwrap();assert_eq!(lut.bytes(),65usize.pow(3)*12);assert_eq!(lut.samples().unwrap().count(),65usize.pow(3));assert!(serde_json::to_vec(&lut).unwrap().len()<512);assert!(RgbSpace::ALL.into_iter().all(|s|lut.accepts(s)));assert!(Lut3d::from_samples(65,[[0.;3],[1.;3]],"missing".into(),vec![[0.;3];8].into()).is_err());
}

use crate::{Document, DocumentNames, Edit, Editor, EffectValue, Point};
use crate::authored::*;
use crate::package::{codec::{PreparedPackage, OpenOutcome}, ImmutableBacking};
use std::{io::Cursor, sync::atomic::AtomicBool};

fn resource_document() -> Document {
    let resource=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());
    let mut document=Document::new(PortableId::random(),64,48,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    let stack=document.composition().result;
    for title in ["First","Alias"] {
        let mut json=serde_json::to_value(resource.as_ref()).unwrap();json["title"]=serde_json::json!(title);
        let alias:Lut3d=serde_json::from_value(json).unwrap();let alias=Arc::new(alias.with_shared_payload(&resource).unwrap());
        let mut effect=lookup_effect();effect.set("table",EffectValue::Lut3d(Some(alias.clone()))).unwrap();
        Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[0].default=EffectValue::Lut3d(Some(alias));
        let definition=document.artwork.definitions.insert(PortableId::random(),Definition {program:effect.program}).unwrap();
        let effect=document.artwork.effects.insert(PortableId::random(),EffectApplication {definition,values:effect.values}).unwrap();
        let occurrence=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(effect),title)).unwrap();
        document.artwork.stacks.get_mut(stack).unwrap().entries.insert(0,occurrence);
    }
    let working=document.working;let mut rebuilt=Document::from_artwork(document.artwork).unwrap();rebuilt.working=working;rebuilt
}
fn prepared(document:&Document)->PreparedPackage {
    let capture=Editor::new(document.clone()).capture(0,document.output().context.clone()).unwrap();
    PreparedPackage::prepare(&capture,None,&AtomicBool::new(false)).unwrap()
}
fn archive(document:&Document)->Vec<u8> {
    let mut bytes=Vec::new();prepared(document).write(&mut bytes,&AtomicBool::new(false)).unwrap();bytes
}
fn open_document(bytes:&[u8],limits:crate::ProjectLimits)->Result<Document,String> {
    let chunks=bytes.chunks(crate::package::MAX_RANGE_BYTES).map(Arc::<[u8]>::from).collect();
    let source=ImmutableBacking::new(Arc::new(crate::package::transport::ChunkedBytes::new(chunks)?))?;
    match crate::package::codec::open(source,limits,&AtomicBool::new(false))? {
        OpenOutcome::Candidate {artwork,..}=>{let document=Document::from_artwork(artwork).map_err(|e|e.to_string())?;document.validate(limits)?;Ok(document)}
        _=>Err("Package did not produce editable artwork".into()),
    }
}
fn code_bytes(document:&Document)->u64 {
    let mut seen=std::collections::BTreeSet::new();let mut bytes=0;
    for (_,_,definition) in document.artwork.definitions.iter() {for code in definition.program.wgsl.sources().unwrap() {if seen.insert(code.as_ptr() as usize){bytes+=code.len() as u64;}}}
    bytes
}
fn effect(document:&Document,index:usize)->crate::EffectView<'_> {document.scene().effect(document.scene().order()[index]).unwrap()}
fn application_edit(document:&Document,index:usize,value:EffectValue)->Edit {
    let handle=document.scene().effect_handle(document.scene().order()[index]).unwrap();
    let mut application=document.artwork.effects.get(handle).unwrap().clone();
    let program=&document.artwork.definitions.get(application.definition).unwrap().program;
    let index=program.parameters.iter().position(|p|p.key.as_ref()=="table").unwrap();application.values[index]=value;
    Edit::Effect(RecordChange::replace(&document.artwork.effects,handle,Some(application)).unwrap())
}
fn rewrite_archive(bytes:&[u8],change:impl FnOnce(&mut serde_json::Value))->Vec<u8> {
    use crate::package::archive::{Directory,InputMember};
    let directory=Directory::read(&mut Cursor::new(bytes),262144,64*1024*1024).unwrap();
    let mut manifest=serde_json::from_slice(&directory.read_member(&mut Cursor::new(bytes),directory.member("manifest.json").unwrap(),64*1024*1024).unwrap()).unwrap();change(&mut manifest);
    let changed=serde_json::to_vec(&manifest).unwrap();
    let mut inputs=directory.members.iter().map(|member| {let data=if member.name=="manifest.json" {changed.clone()} else {directory.read_member(&mut Cursor::new(bytes),member,crate::package::MAX_RANGE_BYTES).unwrap()};(member.name.clone(),Cursor::new(data))}).collect::<Vec<_>>();
    let mut members=inputs.iter_mut().map(|(name,input)|InputMember {name,length:input.get_ref().len() as u64,crc32:crc32fast::hash(input.get_ref()),input}).collect::<Vec<_>>();
    let mut output=Vec::new();crate::package::archive::write_archive(&mut output,&mut members,64*1024*1024).unwrap();output
}

#[test]
fn archive_restores_values_defaults_and_deduplicates_aliases() {
    let document=resource_document();document.validate(Default::default()).unwrap();let prepared=prepared(&document);
    let manifest:serde_json::Value=serde_json::from_slice(prepared.manifest()).unwrap();
    let resources=manifest["resources"].as_array().unwrap();assert_eq!(resources.iter().filter(|r|r["type"]=="capy.lut3d/1").count(),1);
    let bytes=archive(&document);let reopened=open_document(&bytes,Default::default()).unwrap();
    assert_eq!(self::prepared(&reopened).manifest(),prepared.manifest());
    let a=effect(&reopened,0);let b=effect(&reopened,1);assert_ne!(a.lut3d().unwrap().title(),b.lut3d().unwrap().title());
    assert!(Arc::ptr_eq(a.lut3d().unwrap().storage().unwrap(),b.lut3d().unwrap().storage().unwrap()));assert_eq!(a.resources().count(),2);
    for resource in a.resources(){assert!(Arc::ptr_eq(resource.storage().unwrap(),a.lut3d().unwrap().storage().unwrap()));}
    let budget=code_bytes(&document)+96;
    assert!(open_document(&bytes,crate::ProjectLimits {asset_bytes:budget-1,..Default::default()}).is_err());
    assert!(open_document(&bytes,crate::ProjectLimits {asset_bytes:budget,..Default::default()}).is_ok());
    let mut corrupted=bytes.clone();*corrupted.last_mut().unwrap()^=1;assert!(open_document(&corrupted,Default::default()).is_err());assert!(open_document(&bytes[..bytes.len()-1],Default::default()).is_err());
}

#[test]
fn malformed_resource_bindings_are_rejected_before_editable_adoption() {
    let bytes=archive(&resource_document());
    for bad in 0..8 {
        let changed=rewrite_archive(&bytes,|m| {
            let lut=m["resources"].as_array().unwrap().iter().position(|r|r["type"]=="capy.lut3d/1").unwrap();
            let application=m["objects"].as_array().unwrap().iter().position(|o|o["type"]=="capy.effect/1").unwrap();
            match bad {
                0=>{m["resources"][lut]["id"]=serde_json::json!(PortableId::random());}
                1=>{m["objects"][application]["data"]["definition"]=serde_json::json!({"ref":PortableId::random()});}
                2=>{m["objects"][application]["data"]["values"]["missing"]=serde_json::json!({"kind":"number","value":0});}
                3=>{let duplicate=m["resources"][lut].clone();m["resources"].as_array_mut().unwrap().push(duplicate);}
                4=>{m["resources"][lut]["data"]["domain"]=serde_json::json!([[0,0,0],[0,1,1]]);}
                5=>{m["resources"][lut]["bytes"]=u64::MAX.to_string().into();}
                6=>{m["resources"][lut]["location"]["offset"]=u64::MAX.to_string().into();}
                _=>{m["objects"][application]["data"]["values"]=serde_json::json!([]);}
            }
        });
        assert!(open_document(&changed,Default::default()).is_err(),"case {bad}");
    }
}

#[test]
fn undo_and_pending_operations_retain_and_charge_shared_resources_once() {
    let mut document=resource_document();
    let extra=document.scene().order()[1];let extra_effect=document.scene().effect_handle(extra).unwrap();let extra_definition=document.artwork.effects.get(extra_effect).unwrap().definition;
    let stack=document.composition().result;document.artwork.stacks.get_mut(stack).unwrap().entries.remove(1);
    document.artwork.occurrences.remove(extra);document.artwork.effects.remove(extra_effect);document.artwork.definitions.remove(extra_definition);
    let working=document.working;let mut document=Document::from_artwork(document.artwork).unwrap();document.working=working;
    let weak=Arc::downgrade(effect(&document,0).lut3d().unwrap().storage().unwrap());let mut editor=Editor::new(document);
    let effect_handle=editor.document().scene().effect_handle(editor.document().scene().order()[0]).unwrap();let definition_handle=editor.document().artwork.effects.get(effect_handle).unwrap().definition;
    let mut definition=editor.document().artwork.definitions.get(definition_handle).unwrap().clone();Arc::make_mut(&mut Arc::make_mut(&mut definition.program).parameters)[0].default=EffectValue::Lut3d(None);
    let change=Edit::Batch(vec![application_edit(editor.document(),0,EffectValue::Lut3d(None)),Edit::Definition(RecordChange::replace(&editor.document().artwork.definitions,definition_handle,Some(definition)).unwrap())]);
    editor.perform(change).unwrap();assert!(weak.upgrade().is_some());assert!(editor.undo().unwrap());assert_eq!(effect(editor.document(),0).lut3d().unwrap().storage().unwrap().as_ptr(),weak.upgrade().unwrap().as_ptr());assert!(editor.redo().unwrap());assert!(weak.upgrade().is_some());drop(editor);assert!(weak.upgrade().is_none());
    let captured=resource_document();let budget=code_bytes(&captured)+96;
    let mut pending=Document::new(PortableId::random(),64,48,DocumentNames {paint:"Paint".into(),paper:"Paper".into()});
    pending.target_operations_mut(pending.working.target.unwrap()).unwrap().push(crate::RasterOperation {placement:crate::Affine::default(),coverage:crate::CoverageSnapshot::reveal_all(CoverageHandle::from_index(50),[64,48],Point::default()),kind:crate::RasterOperationKind::Bake {scene:captured.snapshot(),scope:SceneScope::Members(captured.scene().order()[..2].to_vec().into()),offset:Point::default()}});
    assert!(pending.validate(crate::ProjectLimits {asset_bytes:budget-1,..Default::default()}).is_err());pending.validate(crate::ProjectLimits {asset_bytes:budget,..Default::default()}).unwrap();
}

#[test]
fn resource_ownership_edits_require_admission_but_intensity_and_title_aliases_do_not() {
    let document=resource_document();let handle=document.scene().effect_handle(document.scene().order()[0]).unwrap();let application=document.artwork.effects.get(handle).unwrap();
    assert!(Edit::Effect(RecordChange::insert(&document.artwork.effects,application.clone())).requires_history_admission(&document));
    assert!(Edit::Effect(RecordChange::replace(&document.artwork.effects,handle,None).unwrap()).requires_history_admission(&document));
    let mut changed=application.clone();let program=effect(&document,0).program;let intensity=program.parameters.iter().position(|p|p.key.as_ref()=="intensity").unwrap();changed.values[intensity]=EffectValue::Number(42.);
    assert!(!Edit::Effect(RecordChange::replace(&document.artwork.effects,handle,Some(changed)).unwrap()).requires_history_admission(&document));
    let resource=effect(&document,0).lut3d().unwrap();let mut json=serde_json::to_value(resource.as_ref()).unwrap();json["title"]="Renamed resource".into();let alias:Lut3d=serde_json::from_value(json).unwrap();let alias=Arc::new(alias.with_shared_payload(resource).unwrap());
    assert!(!application_edit(&document,0,EffectValue::Lut3d(Some(alias))).requires_history_admission(&document));
    assert!(application_edit(&document,0,EffectValue::Lut3d(None)).requires_history_admission(&document));
    let new=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());assert!(application_edit(&document,0,EffectValue::Lut3d(Some(new))).requires_history_admission(&document));
}

#[test]
fn asset_budget_deduplicates_physical_resource_and_source_ownership() {
    let mut document=resource_document();let base=code_bytes(&document)+96;
    assert!(document.validate(crate::ProjectLimits {asset_bytes:base-1,..Default::default()}).is_err());document.validate(crate::ProjectLimits {asset_bytes:base,..Default::default()}).unwrap();
    let source=crate::color::source::rgba8_source([2,1],|_,_|[40,50,60,255]);let source_bytes=std::mem::size_of::<crate::color::source::SourceImage>()+source.tiles.len()*96+source.tiles.values().map(|t|t.compressed_len()).sum::<usize>();
    for _ in 0..2 {document.artwork.paint.insert(PortableId::random(),PaintSource {domain:[64,48],original:Some(source.clone()),raster:Default::default(),operations:Default::default()}).unwrap();}
    let total=base+source_bytes as u64;assert!(document.validate(crate::ProjectLimits {asset_bytes:total-1,..Default::default()}).is_err());document.validate(crate::ProjectLimits {asset_bytes:total,..Default::default()}).unwrap();
    let independent=Arc::new(Lut3d::parse_cube(cube().as_bytes()).unwrap());document.apply(application_edit(&document,0,EffectValue::Lut3d(Some(independent)))).unwrap();
    assert!(document.validate(crate::ProjectLimits {asset_bytes:total+95,..Default::default()}).is_err());document.validate(crate::ProjectLimits {asset_bytes:total+96,..Default::default()}).unwrap();
}

#[test]
fn filename_fallback_preserves_embedded_titles_and_bounds_unicode() {
    let explicit=Lut3d::parse_cube_named(cube().as_bytes(),"ignored.cube").unwrap();assert_eq!(explicit.title(),"Independent 🎨");let untitled=cube().lines().skip(1).collect::<Vec<_>>().join("\n");let filename=format!("\u{1}{}\n.cube","色".repeat(300));let named=Lut3d::parse_cube_named(untitled.as_bytes(),&filename).unwrap();assert_eq!(named.title(),"色".repeat(256));assert_eq!(named.digest(),Lut3d::parse_cube(untitled.as_bytes()).unwrap().digest());
}

#[test]
fn resource_candidate_refuses_late_bad_payload_without_partial_mutation() {
    let mut document=resource_document();let distinct=Arc::new(constant([[0.;3],[1.;3]],[0.75;3]).unwrap());document.apply(application_edit(&document,0,EffectValue::Lut3d(Some(distinct)))).unwrap();
    let before=document.clone();let prepared=prepared(&document);let manifest:serde_json::Value=serde_json::from_slice(prepared.manifest()).unwrap();
    assert_eq!(manifest["resources"].as_array().unwrap().iter().filter(|r|r["type"]=="capy.lut3d/1").count(),2);
    let bytes=archive(&document);let damaged=rewrite_archive(&bytes,|m| {let index=m["resources"].as_array().unwrap().iter().enumerate().filter(|(_,r)|r["type"]=="capy.lut3d/1").last().unwrap().0;m["resources"][index]["crc32"]="00000000".into();});
    assert!(open_document(&damaged,Default::default()).is_err());assert_eq!(document,before);
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
    damaged[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(Lut3d::from_resource(original.size(), original.domain(), original.title().into(), damaged.into()).is_err());
}

#[test]
fn builtin_look_matching_includes_domain_as_well_as_samples() {
    let original=crate::lut3d::Look::Warm.resource();
    let changed=Lut3d::from_samples(original.size(),[[0.;3],[2.;3]],original.title().into(),original.samples().unwrap().collect::<Vec<_>>().into()).unwrap();
    assert_eq!(changed.digest(),original.digest());
    assert_eq!(crate::lut3d::Look::for_resource(&changed),None);
    assert_eq!(crate::lut3d::Look::for_resource(&original),Some(crate::lut3d::Look::Warm));
}
