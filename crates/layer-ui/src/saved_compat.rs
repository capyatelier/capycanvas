use super::*;
use serde_json::{json,Value};
use std::path::{Path,PathBuf};

fn root()->PathBuf {Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat")}

fn write(directory:&Path,name:&str,value:&impl Serialize) {
    std::fs::write(directory.join(name),serde_json::to_vec_pretty(value).unwrap()).unwrap();
}
fn expect(release:&str,name:&str,value:Value) {
    let directory=root().join("expected").join(release);
    if std::env::var_os("CAPY_BLESS").as_deref()==Some(std::ffi::OsStr::new("1")) {
        std::fs::create_dir_all(&directory).unwrap();write(&directory,name,&value);
    }
    assert_eq!(value,serde_json::from_slice::<Value>(&std::fs::read(directory.join(name)).unwrap()).unwrap(),"{name}");
}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory=PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    generate(&directory);
}

fn generate(directory:&Path)->SessionDocumentState {
    std::fs::create_dir_all(directory).unwrap();
    let english=Localizer::shared(UiLanguage::English);
    let editing=complete_editing(&english);
    write(directory,"editing.json",&editing);
    let settings=complete_settings();
    write(directory,"settings.json",&settings);
    std::fs::write(directory.join("bindings.capykeys"),crate::keymaps::export(&settings)).unwrap();
    let profile=layer_color::profile_bytes(&layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3)).unwrap();
    let record=crate::profile_library::prepare_profile_import(vec![],&profile).unwrap();
    std::fs::write(directory.join(format!("{}.icc",record.id)),&profile).unwrap();
    write(directory,"profiles.json",&vec![crate::profile_library::ProfileRecord{id:record.id.clone(),bytes:profile.len() as u64,issue:None}]);
    let hidden=crate::profile_library::ProfileLibraryAction::Visibility {hidden:vec![],id:Some(record.id),visible:Some(false)}.execute(&[]).unwrap();
    write(directory,"hidden-profiles.json",&hidden);
    let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
    layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(1,1).unwrap();save(&mut source,true);
    source.state.camera.zoom_to(1.75).unwrap();
    source.files.last_export=Some(ExportRepeat{recipe:ExportRecipe::wide_color(),location:DocumentLocation{uri:"private:release.png".into(),name:"Release.png".into()}});
    let save=|key:&str,source:&UiSession<Recorder>| {
        let mut capture=source.capture_session().unwrap();
        capture.state.camera.center=[12.,34.];capture.state.camera.rotation=0.25;capture.state.camera.flipped=[true,true];
        capture.state.camera.zoom_locked=true;capture.state.camera.rotation_locked=true;
        capture.state.unsaved_name=Some("Coverage drawing".into());capture.state.unpublished=true;capture.state.recovered=true;
        let mut store=pollster::block_on(layer_core::package::session_store::SessionStore::open(&directory.join(key))).unwrap();
        pollster::block_on(capture.checkpoint(&mut store,&AtomicBool::new(false))).unwrap();
        drop(store);std::fs::remove_file(directory.join(key).join(".lock")).unwrap();
        capture.state
    };
    let authored=save("drawing-a",&source);
    write(directory,"session.json",&SessionManifest::default().reconcile(vec![SessionDrawing{id:7,key:"drawing-a".into()}],7,false).unwrap());
    let mut recipe=ExportRecipe::wide_color();recipe.profile.profile=layer_core::color::ColorProfile::Icc(profile.to_vec().into());
    let mut presets=crate::ExportPresets::default();presets.save("Release ICC",recipe.clone()).unwrap();presets.remember(0,recipe.clone()).unwrap();
    std::fs::write(directory.join("export-presets-icc"),presets.encode().unwrap()).unwrap();
    source.files.last_export.as_mut().unwrap().recipe=recipe;
    save("drawing-icc",&source);
    write(directory,"session-icc.json",&SessionManifest::default().reconcile(vec![SessionDrawing{id:9,key:"drawing-icc".into()}],9,false).unwrap());
    write(directory,"saved-names.json",&saved_names(&english));
    write(directory,"workspace-layout.json",&complete_history(&english));
    authored
}

fn copy_tree(source:&Path,destination:&Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry=entry.unwrap();let target=destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir(){copy_tree(&entry.path(),&target);}else{std::fs::copy(entry.path(),target).unwrap();}
    }
}

#[test]
fn release_fixtures_load_without_loss() {
    for entry in std::fs::read_dir(root()).unwrap() {
        let path=entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap()!="expected" {check_release(path.file_name().unwrap().to_str().unwrap());}
    }
}

fn check_release(release:&str) {
    check_inputs(&root().join(release),|name,value|expect(release,name,value));
}

#[test]
fn external_compat_inputs_load() {
    let Some(directory)=std::env::var_os("CAPY_COMPAT_IN") else {return;};
    check_inputs(&PathBuf::from(directory),|_,_|{});
}

fn check_inputs(directory:&Path,check:impl Fn(&str,Value)) {
    let english=Localizer::shared(crate::UiLanguage::English);
    for entry in std::fs::read_dir(directory).unwrap() {
        let path=entry.unwrap().path();let name=path.file_name().unwrap().to_str().unwrap();
        if name.starts_with("editing") && name.ends_with(".json") {
            let saved=std::fs::read_to_string(&path).unwrap();check(name,serde_json::to_value(EditingState::restore(&saved,&english)).unwrap());
        } else if name.starts_with("settings") && name.ends_with(".json") {
            let saved=std::fs::read_to_string(&path).unwrap();check(name,serde_json::to_value(Settings::restore(&saved)).unwrap());
        } else if name=="workspace-layout.json" {
            let mut history:LayoutHistory=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            history.retain_admitted();history.validate().unwrap();check(name,serde_json::to_value(history).unwrap());
        } else if name=="export-presets-complete" {
            check("export-presets-complete.json",serde_json::to_value(crate::ExportPresets::restore(&std::fs::read(path).unwrap())).unwrap());
        } else if name=="bindings-complete.capykeys" {
            let imported=crate::keymaps::import_typed(&Settings::default(),&std::fs::read_to_string(path).unwrap(),Platform::Gtk,&english).unwrap();
            check("bindings-complete.json",serde_json::from_str(&crate::keymaps::export(&imported.settings)).unwrap());
        }
    }
    let keymap=std::fs::read_to_string(directory.join("bindings.capykeys")).unwrap();
    let imported=crate::keymaps::import_typed(&Settings::default(),&keymap,Platform::Gtk,&english).unwrap();
    check("bindings.json",serde_json::from_str(&crate::keymaps::export(&imported.settings)).unwrap());
    let records:Vec<crate::profile_library::ProfileRecord>=serde_json::from_slice(&std::fs::read(directory.join("profiles.json")).unwrap()).unwrap();
    let entries:Vec<_>=records.iter().map(|record| {
        let bytes=std::fs::read(directory.join(format!("{}.icc",record.id))).unwrap();
        assert_eq!(bytes.len() as u64,record.bytes);
        assert_eq!(crate::profile_library::profile_identity(&bytes),record.id);
        let entry=crate::profile_library::inspect_library_entry(record,Ok(&bytes));assert!(entry.issue.is_none());entry
    }).collect();
    let hidden:Vec<String>=serde_json::from_slice(&std::fs::read(directory.join("hidden-profiles.json")).unwrap()).unwrap();
    let hidden=crate::profile_library::ProfileLibraryAction::Visibility{hidden,id:None,visible:None}.execute(&[]).unwrap();
    check("profiles.json",json!({"entries":entries,"hidden":hidden}));
    let presets=crate::ExportPresets::restore(&std::fs::read(directory.join("export-presets-icc")).unwrap());
    assert_eq!(presets.names().collect::<Vec<_>>(),["Release ICC"]);
    let recipes:Vec<_>=[0,4].into_iter().map(|index| {
        let recipe=presets.recipe(index,Default::default()).unwrap();
        let layer_core::color::ColorProfile::Icc(bytes)=&recipe.profile.profile else {panic!("missing preset ICC")};
        let profile=recipe.profile.clone().with_profile(crate::profile_library::profile_identity(bytes));
        recipe.with_profile(profile)
    }).collect();
    check("export-presets-icc.json",json!(recipes));
    for name in ["session.json","session-icc.json"] {
        let manifest=SessionManifest::parse(&std::fs::read(directory.join(name)).unwrap()).unwrap();
        check(if name=="session.json" {"membership.json"}else{"membership-icc.json"},serde_json::to_value(&manifest).unwrap());
        let temporary=std::env::temp_dir().join(format!("capy-release-ui-{}",PortableId::random()));
        copy_tree(&directory.join(&manifest.drawings[0].key),&temporary);
        let cancel=AtomicBool::new(false);let mut store=pollster::block_on(layer_core::package::session_store::SessionStore::open(&temporary)).unwrap();
        let restored=SessionRestore::from_core(pollster::block_on(store.load(ProjectLimits::default(),&cancel)).unwrap().unwrap()).unwrap();
        let profile=restored.last_export.as_ref().and_then(|last|match &last.recipe.profile.profile {
            layer_core::color::ColorProfile::Icc(bytes)=>Some(crate::profile_library::profile_identity(bytes)),_=>None,
        });
        let metadata=serde_json::to_value(&restored.state).unwrap();
        let modified=restored.state.unpublished || restored.editor.checkpoint()!=restored.state.saved_checkpoint
            || (restored.state.location.is_some() && restored.state.destination.is_none());
        let mut target=UiSession::from_project(Recorder::default(),restored.document().clone(),None,[500,400],Platform::Gtk).unwrap();
        target.frame(0,0).unwrap();let observed=restored.state.destination.clone();target.restore_session(restored,false,observed).unwrap();
        assert_eq!(target.state.document_file.modified,modified);assert!(target.engine.can_undo());
        let mut actual=json!({"metadata":metadata,"camera":target.session_document_state().camera,"checkpoint":target.engine.checkpoint(),"saved":target.files.saved_checkpoint});
        if let Some(profile)=profile {actual["export_profile_sha256"]=profile.into();}
        check(name,actual);
        drop(target);drop(store);std::fs::remove_dir_all(temporary).unwrap();
    }
}

#[test]
fn saved_documents_tolerate_missing_and_unknown_fields() {
    let directory=root().join("1.0.15");let english=Localizer::shared(crate::UiLanguage::English);
    let mut keymap:Value=serde_json::from_slice(&std::fs::read(directory.join("bindings.capykeys")).unwrap()).unwrap();
    let import=|value:&Value|crate::keymaps::import_typed(&Settings::default(),&value.to_string(),Platform::Gtk,&english);
    let baseline=crate::keymaps::export(&import(&keymap).unwrap().settings);
    keymap["future"]=true.into();keymap["keymap"]["future"]=true.into();
    for hold in keymap["modifiers"].as_array_mut().unwrap() {hold["future"]=true.into();hold["key"]["future"]=true.into();}
    assert_eq!(crate::keymaps::export(&import(&keymap).unwrap().settings),baseline);
    for field in ["keymap","shortcuts","gestures","modifiers","pen_buttons"] {
        let mut missing=keymap.clone();missing.as_object_mut().unwrap().remove(field);
        let imported=import(&missing).unwrap();imported.settings.validate().unwrap();
        let remaining:Value=serde_json::from_str(&crate::keymaps::export(&imported.settings)).unwrap();
        let expected:Value=serde_json::from_str(&baseline).unwrap();
        for (key,value) in expected.as_object().unwrap() {if key!=field {assert_eq!(&remaining[key],value,"missing {field}, retained {key}");}}
    }
    for field in ["format","version"] {
        let mut missing=keymap.clone();missing.as_object_mut().unwrap().remove(field);assert!(import(&missing).is_err());
    }
    let mut membership:Value=serde_json::from_slice(&std::fs::read(directory.join("session.json")).unwrap()).unwrap();
    let original=SessionManifest::parse(&serde_json::to_vec(&membership).unwrap()).unwrap();
    membership["future"]=true.into();membership["drawings"][0]["future"]=true.into();
    assert_eq!(SessionManifest::parse(&serde_json::to_vec(&membership).unwrap()).unwrap(),original);
    for field in ["id","key"] {
        let mut missing=membership.clone();missing["drawings"][0].as_object_mut().unwrap().remove(field);
        assert!(SessionManifest::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    }
    let generation=std::fs::read_dir(directory.join("drawing-a/generations")).unwrap().next().unwrap().unwrap().path();
    let envelope:Value=serde_json::from_slice(&std::fs::read(generation).unwrap()).unwrap();
    let original=envelope["metadata"].clone();let mut metadata=original.clone();
    for pointer in ["","/camera","/destination","/last_export","/last_export/recipe","/last_export/recipe/profile","/last_export/location"] {
        metadata.pointer_mut(pointer).unwrap().as_object_mut().unwrap().insert("future".into(),true.into());
    }
    let (state,_)=SessionRestore::decode_metadata(SessionMetadata{value:metadata,profiles:vec![]}).unwrap();
    assert_eq!(serde_json::to_value(state).unwrap(),original);
    for pointer in ["/camera/center","/camera/zoom","/destination/sha256","/saved_checkpoint"] {
        let mut missing=original.clone();let (parent,field)=pointer.rsplit_once('/').unwrap();
        missing.pointer_mut(parent).unwrap().as_object_mut().unwrap().remove(field);
        assert!(SessionRestore::decode_metadata(SessionMetadata{value:missing,profiles:vec![]}).is_err(),"{pointer}");
    }
    let records:Value=serde_json::from_slice(&std::fs::read(directory.join("profiles.json")).unwrap()).unwrap();
    let mut unknown=records[0].clone();unknown["future"]=true.into();
    assert!(serde_json::from_value::<crate::profile_library::ProfileRecord>(unknown.clone()).is_ok());
    for field in ["id","bytes"] {
        let mut missing=unknown.clone();missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<crate::profile_library::ProfileRecord>(missing).is_err());
    }
}

fn struct_fields<T:serde::de::DeserializeOwned>()->&'static [&'static str] {
    struct Fields<'a>(&'a std::cell::Cell<&'static [&'static str]>);
    impl<'de> serde::Deserializer<'de> for Fields<'_> {
        type Error=serde::de::value::Error;
        fn deserialize_any<V:serde::de::Visitor<'de>>(self,_:V)->Result<V::Value,Self::Error> {Err(serde::de::Error::custom("fields"))}
        fn deserialize_struct<V:serde::de::Visitor<'de>>(self,_:&'static str,fields:&'static [&'static str],_:V)->Result<V::Value,Self::Error> {
            self.0.set(fields);Err(serde::de::Error::custom("fields"))
        }
        serde::forward_to_deserialize_any! {bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any}
    }
    let fields=std::cell::Cell::new(&[][..]);
    let _=T::deserialize(Fields(&fields));
    assert!(!fields.get().is_empty(),"{} has no serde struct fields",std::any::type_name::<T>());
    fields.get()
}
fn exempt_path(path:&str,exempt:&[&str])->bool {
    exempt.iter().any(|entry|path==*entry || path.strip_prefix(entry).is_some_and(|rest|rest.starts_with('/')))
}
fn pointer_child(parent:&str,key:&str)->String {format!("{parent}/{}",key.replace('~',"~0").replace('/',"~1"))}
fn assert_every_field_differs(name:&str,generated:&Value,default:&Value,exempt:&[&str]) {
    fn walk(path:&str,generated:Option<&Value>,default:Option<&Value>,exempt:&[&str],paths:&mut Vec<String>) {
        if exempt_path(path,exempt) {return;}
        match (generated.and_then(Value::as_object),default.and_then(Value::as_object)) {
            (Some(g),Some(d)) if !g.is_empty() && !d.is_empty()=> {
                let keys:std::collections::BTreeSet<_>=g.keys().chain(d.keys()).collect();
                for key in keys {walk(&pointer_child(path,key),g.get(key),d.get(key),exempt,paths);}
            }
            _=>if generated.is_none() || generated==default {paths.push(path.into());},
        }
    }
    let mut paths=Vec::new();walk("",Some(generated),Some(default),exempt,&mut paths);
    assert!(paths.is_empty(),"{name} keeps default values at {paths:?}; give them non-default values in write_compat_fixtures");
}
type StructPair=(&'static str,&'static str,fn()->&'static [&'static str]);
fn assert_struct_fields(name:&str,generated:&Value,default:&Value,pairs:&[StructPair],exempt:&[&str]) {
    for &(kind,pointer,fields) in pairs {
        for field in fields() {
            let path=pointer_child(pointer,field);
            if exempt_path(&path,exempt) {continue;}
            let value=generated.pointer(&path);
            assert!(value.is_some() && value!=default.pointer(&path),"{name}: {kind} field {field} is missing or default at {pointer}; set it in write_compat_fixtures");
        }
    }
}

#[test]
fn generated_saved_documents_set_every_field() {
    let directory=std::env::temp_dir().join(format!("capy-complete-{}",PortableId::random()));
    let authored=generate(&directory);
    check_inputs(&directory,|_,_|{});
    let english=Localizer::shared(UiLanguage::English);
    let editing=serde_json::to_value(serde_json::from_slice::<EditingState>(&std::fs::read(directory.join("editing.json")).unwrap()).unwrap()).unwrap();
    let restored=EditingState::restore(&editing.to_string(),&english);
    assert_eq!(serde_json::to_value(restored).unwrap(),editing);
    assert_every_field_differs("editing",&editing,&serde_json::to_value(EditingState::default()).unwrap(),&["/version"]);
    let settings=serde_json::to_value(serde_json::from_slice::<Settings>(&std::fs::read(directory.join("settings.json")).unwrap()).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(Settings::restore(&settings.to_string())).unwrap(),settings);
    assert_every_field_differs("settings",&settings,&serde_json::to_value(Settings::default()).unwrap(),&["/version"]);
    let settings=Settings::restore(&settings.to_string());
    let imported=crate::keymaps::import_typed(&settings,&std::fs::read_to_string(directory.join("bindings.capykeys")).unwrap(),Platform::Gtk,&english).unwrap().settings;
    assert_eq!(imported,settings);
    let file=std::fs::read_to_string(directory.join("bindings.capykeys")).unwrap();
    let fresh=crate::keymaps::import_typed(&Settings::default(),&file,Platform::Gtk,&english).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&crate::keymaps::export(&fresh.settings)).unwrap(),serde_json::from_str::<Value>(&file).unwrap());
    let defaults=serde_json::to_value(Settings::default()).unwrap();
    assert_every_field_differs("keymap settings",&serde_json::to_value(&imported).unwrap(),&defaults,&["/version"]);
    for (name,value,default,pairs) in [
        ("editing",&editing,serde_json::to_value(EditingState::default()).unwrap(),EDITING_STRUCTS),
        ("settings",&serde_json::to_value(&settings).unwrap(),defaults.clone(),SETTINGS_STRUCTS),
        ("keymap settings",&serde_json::to_value(&imported).unwrap(),defaults,KEYMAP_STRUCTS),
    ] {assert_struct_fields(name,value,&default,pairs,&["/version"]);}
    let presets=crate::ExportPresets::restore(&std::fs::read(directory.join("export-presets-icc")).unwrap());
    assert_eq!(crate::ExportPresets::restore(&presets.encode().unwrap()),presets);
    let generated=serde_json::to_value(presets).unwrap();let default=serde_json::to_value(crate::ExportPresets::<layer_core::color::ColorProfile>::default()).unwrap();
    assert_every_field_differs("export presets",&generated,&default,&["/version"]);
    assert_struct_fields("export presets",&generated,&default,EXPORT_STRUCTS,&["/version"]);
    let mut baseline=session(Platform::Gtk);baseline.frame(0,0).unwrap();
    let default=serde_json::to_value(baseline.capture_session().unwrap().state).unwrap();
    {
        let mut store=pollster::block_on(layer_core::package::session_store::SessionStore::open(&directory.join("drawing-a"))).unwrap();
        let opened=pollster::block_on(store.load(ProjectLimits::default(),&AtomicBool::new(false))).unwrap().unwrap();
        let restored=SessionRestore::from_core(opened).unwrap();
        assert_eq!(serde_json::to_value(restored.state).unwrap(),serde_json::to_value(&authored).unwrap());
    }
    let generated=serde_json::to_value(&authored).unwrap();
    assert_every_field_differs("session UI",&generated,&default,&["/version"]);
    assert_struct_fields("session UI",&generated,&default,SESSION_STRUCTS,&["/version"]);
    for id in brush_ids() {
        let brush=layer_core::default_brush(preset(id).unwrap());
        for control in crate::tool_settings::controls(id,&brush,&english) {
            assert!(editing["tools"]["overrides"][id.to_string()].get(control.id).is_some(),"brush {id} missing {}",control.id);
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

fn complete_colors()->ColorState {
    let mut colors=ColorState::default();
    colors.set_rgb_space(layer_core::color::RgbSpace::DisplayP3).unwrap();
    for (index,slot) in [ColorSlot::Foreground,ColorSlot::Background,ColorSlot::Temporary].into_iter().enumerate() {
        colors.apply(ColorAction::Select {slot}).unwrap();
        let rgba=[0.13+index as f32*0.1,0.27,0.63,0.7];
        let mut color=layer_core::color::RgbColor::from_linear(layer_core::color::RgbSpace::DisplayP3,rgba).unwrap();
        color.linear_rgb=Some([rgba[0],rgba[1],rgba[2]]);
        colors.set_color(color).unwrap();
    }
    colors.apply(ColorAction::Select {slot:ColorSlot::Background}).unwrap();
    colors.shape=ColorShape::Triangle;colors.readout=ColorReadout::Rgb;
    colors.set_document_depth(layer_core::color::SampleDepth::F32).unwrap();colors.set_hdr_enabled(true).unwrap();
    let mut value=serde_json::to_value(colors).unwrap();
    value["editor"]=json!({"forms":["linear_rgb","hsl","oklab"],"search":"Coverage"});
    serde_json::from_value(value).unwrap()
}
fn complete_editing(english:&Localizer)->EditingState {
    let mut editing=EditingState::default();
    editing.region_gap_reach=Some(layer_render::RegionGapMasks::STOPS[1]);
    for id in brush_ids() {
        let defaults=layer_core::default_brush(preset(id).unwrap());
        for control in crate::tool_settings::controls(id,&defaults,english) {
            let value=if control.numeric.min as f32==control.value {control.numeric.max}else{control.numeric.min} as f32;
            editing.tools.set_override(id,control.id,value,false,english).unwrap();
        }
        editing.tools.remember(id);
    }
    editing.preset=brush_ids().last().unwrap();
    for slot in ToolSlotId::ALL {editing.tool_slots.choices.insert(slot,*slot.variants().last().unwrap());}
    editing.colors=complete_colors();editing.mask_colors=complete_colors();
    let color=editing.colors.foreground;
    let mut value=serde_json::to_value(&editing).unwrap();
    value["color_library"]=json!({
        "palettes":[{"id":400,"name":"Coverage","swatches":[{"id":401,"name":"Ink","color":color}]}],
        "active":400,"selected":401,"history":[color],"pending_name":[color,"Named paint"],
        "starters_installed":false,"next_id":402,
    });
    value["canvas_tool"]="hand".into();
    value["selection"]=json!({
        "tool":"ellipse","tonal":{"tone":7,"custom":[-2.,2.],"softness":0.5},
        "brush":{"subtract":true,"pressure_size":true,"size":19.,"hardness":0.4,"opacity":0.6},
        "display":{"outline":false,"overlay":false,"color":[0.2,0.3,0.8,0.4]},
        "constraint":"ratio","from_center":true,"mode":"add","antialias":false,
        "feather":2.,"constrain_angles":true,"ratio":[2.,3.],"size":[123.,234.],
    });
    value["region_values"]=json!({"edge_offset":2.,"grain_depth":0.5,"tolerance":0.4,"unfilled_size":19.});
    value["fill_areas"]="any_colour".into();
    value["region_sources"]=json!(["visible","reference","editing"]);
    for field in ["region_stop_at_colors","region_close_gaps","region_smooth_edges"] {value[field]=false.into();}
    value["region_edges"]=serde_json::to_value(layer_core::RegionEdges::GrowShrink(2)).unwrap();
    value["fill_scope"]="everywhere".into();
    value["gradient"]=json!({"definition":layer_core::GradientDefinition::new(vec![layer_core::GradientStop{position:0.,color},layer_core::GradientStop{position:1.,color}]),"shape":"Radial"});
    value["figure"]=json!(["ellipse","fill"]);
    let editing:EditingState=serde_json::from_value(value).unwrap();editing.validate().unwrap();editing
}
fn complete_settings()->Settings {
    let mut settings=Settings::default();crate::keymaps::select(&mut settings,"krita").unwrap();
    settings.shortcuts.insert(CommandId::Undo.shortcut_id(),vec![]);
    settings.gestures.insert("touch.tap.2".into(),CommandId::Redo.shortcut_id());
    settings.hold_keys=Some(settings.default_hold_keys(Platform::Gtk));
    let mut setup=crate::new_drawing::default_setup();
    setup.paper.texture=Some("coverage".into());setup.paper.transparent=true;setup.paper.color=complete_colors().foreground;
    let mut value=serde_json::to_value(settings).unwrap();
    let options=json!({
        "theme":"dark","language":{"Explicit":"ja"},"transparency":"high",
        "dark_base":"#444444","light_base":"#cccccc","accent":"#b85050",
        "zen_icon":"sleeping","zen_show_capy":false,"phone_swap_sides":true,"finger_drawing":"off","pen_seen":true,"selection_painting":"black_white",
        "pressure_curve":[[0.,0.],[1.,1.]],"tilt_calibrations":{"coverage":layer_core::TiltCalibration::default()},
        "cursor":"cross","hide_cursor_while_drawing":false,"pan_speed":1.5,"zoom_speed":0.75,
        "zoom_tool":{"zoom_out":true,"drag":"area","direction":"vertical","center_clicked_point":true},
        "wheel_zoom":true,"rotate_with_two_fingers":false,"quick_pinch_fit":false,
        "quick_pinch_restore":false,"snap_rotation":false,"pass_through_groups":true,
        "feedback":false,"platform_prediction":false,"prediction_ms":12.,
        "eraser_end":{"tool":"pencil","erase":false},"vector_eraser":"whole_paths","keep_outline_widths":true,
        "slider_bookmarks":{"2":{"size":[19.],"strength":[0.4]}},
        "new_document":{
            "paper":"coverage","setups":[{"name":"Coverage","group":"yours","setup":setup}],
            "recent":[crate::PresetKey::Saved("Coverage".into())],"last":setup,
            "paper_colors":[complete_colors().foreground],
        },
        "photo_open":{"promote_to_16":true,"missing_profile":"ask"},
        "pen_buttons":{"pen.button.primary":{"drawing":"command.Redo"}},
    });
    for (key,v) in options.as_object().unwrap() {value[key]=v.clone();}
    let settings:Settings=serde_json::from_value(value).unwrap();settings.validate().unwrap();settings
}

const EDITING_STRUCTS:&[StructPair]=&[
    ("EditingState","",struct_fields::<EditingState>),
    ("ToolMemory","/tools",struct_fields::<ToolMemory>),
    ("ColorLibrary","/color_library",struct_fields::<ColorLibrary>),
    ("ColorState","/colors",struct_fields::<ColorState>),
    ("ColorState","/mask_colors",struct_fields::<ColorState>),
    ("RgbColor","/colors/foreground",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/colors/background",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/colors/temporary",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/colors/hdr_picker/0/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/colors/hdr_picker/1/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/colors/hdr_picker/2/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/foreground",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/background",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/temporary",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/hdr_picker/0/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/hdr_picker/1/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/mask_colors/hdr_picker/2/base",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/color_library/history/0",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/color_library/pending_name/0",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/color_library/palettes/0/swatches/0/color",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/gradient/definition/stops/0/color",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/gradient/definition/stops/1/color",struct_fields::<layer_core::color::RgbColor>),
];
const SETTINGS_STRUCTS:&[StructPair]=&[
    ("Settings","",struct_fields::<Settings>),
    ("NewDocumentSettings","/new_document",struct_fields::<NewDocumentSettings>),
    ("RgbColor","/new_document/paper_colors/0",struct_fields::<layer_core::color::RgbColor>),
    ("PaperSetup","/new_document/last/paper",struct_fields::<PaperSetup>),
    ("PaperSetup","/new_document/setups/0/setup/paper",struct_fields::<PaperSetup>),
    ("RgbColor","/new_document/last/paper/color",struct_fields::<layer_core::color::RgbColor>),
    ("RgbColor","/new_document/setups/0/setup/paper/color",struct_fields::<layer_core::color::RgbColor>),
];
const EXPORT_STRUCTS:&[StructPair]=&[("ExportPresets","",struct_fields::<crate::ExportPresets>)];
const SESSION_STRUCTS:&[StructPair]=&[("SessionDocumentState","",struct_fields::<SessionDocumentState>)];

const KEYMAP_STRUCTS:&[StructPair]=SETTINGS_STRUCTS;

fn tool_setting_ids(english:&Localizer)->std::collections::BTreeSet<&'static str> {
    brush_ids().flat_map(|id|crate::tool_settings::controls(id,&layer_core::default_brush(preset(id).unwrap()),english)).map(|control|control.id).collect()
}
fn saved_names(english:&Localizer)->Value {
    use ToolbarControl as C;
    let brush=brush_ids().next().unwrap();
    let controls=[C::ToolSlot{slot:ToolSlotId::Fill},C::Command{command:CommandId::Undo},C::Brush{id:brush},C::Size{tenths:120},C::Color,C::ColorPicker,
        C::BrushSizeSlider,C::BrushStrengthSlider,C::ToolOptions{style:Default::default()},C::Panel{panel:Panel::Layers},C::Divider];
    let control_kind=|control:&ToolbarControl|match control {C::ToolSlot{..}=>0,C::Command{..}=>1,C::Brush{..}=>2,C::Size{..}=>3,C::Color=>4,C::ColorPicker=>5,
        C::BrushSizeSlider=>6,C::BrushStrengthSlider=>7,C::ToolOptions{..}=>8,C::Panel{..}=>9,C::Divider=>10};
    assert_eq!(controls.iter().map(control_kind).collect::<std::collections::BTreeSet<_>>(),(0..=10).collect());
    let mut items=HeaderItem::COMPONENTS.to_vec();items.push(HeaderItem::Tool{control:controls[1]});
    let item_kind=|item:&HeaderItem|match item {HeaderItem::Capy=>0,HeaderItem::Menu=>1,HeaderItem::MenuLabels=>2,HeaderItem::Settings=>3,HeaderItem::Fullscreen=>4,
        HeaderItem::Workspaces=>5,HeaderItem::DocumentTitle=>6,HeaderItem::Clock=>7,HeaderItem::Battery=>8,HeaderItem::Space=>9,HeaderItem::Tool{..}=>10};
    assert_eq!(items.iter().map(item_kind).collect::<std::collections::BTreeSet<_>>(),(0..=10).collect());
    let sources=[BubbleSource::Control{control:C::Color},BubbleSource::Medium{group:ToolGroup::ALL[0]}];
    let source_kind=|source:&BubbleSource|match source {BubbleSource::Control{..}=>0,BubbleSource::Medium{..}=>1};
    assert_eq!(sources.iter().map(source_kind).collect::<std::collections::BTreeSet<_>>(),(0..=1).collect());
    let name=|panel,custom_name:Option<&str>|LayoutPanelName{panel,custom_name:custom_name.map(Into::into)};
    let changes=[LayoutChange::Starting,LayoutChange::HeaderSize{size:HeaderSize::Large},LayoutChange::HeaderAdded{item:HeaderItem::Clock},
        LayoutChange::HeaderRemoved{item:HeaderItem::Clock},LayoutChange::HeaderRearranged,LayoutChange::CanvasInfo,LayoutChange::CanvasBar,LayoutChange::Restored,
        LayoutChange::RestoredEarlier,LayoutChange::panels(LayoutPanelAction::Moved,vec![name(Panel::Layers,None)]),
        LayoutChange::Renamed{before:name(Panel::Toolbar,None),after:name(Panel::Toolbar,Some("Tools"))},LayoutChange::Bubble{action:LayoutPanelAction::Added,source:sources[0]}];
    let change_kind=|change:&LayoutChange|match change {LayoutChange::Automatic=>usize::MAX,LayoutChange::Starting=>0,LayoutChange::HeaderSize{..}=>1,LayoutChange::HeaderAdded{..}=>2,
        LayoutChange::HeaderRemoved{..}=>3,LayoutChange::HeaderRearranged=>4,LayoutChange::CanvasInfo=>5,LayoutChange::CanvasBar=>6,LayoutChange::Restored=>7,
        LayoutChange::RestoredEarlier=>8,LayoutChange::Panels{..}=>9,LayoutChange::Renamed{..}=>10,LayoutChange::Bubble{..}=>11};
    assert_eq!(changes.iter().map(change_kind).collect::<std::collections::BTreeSet<_>>(),(0..=11).collect());
    let mut panels=Panel::ALL.to_vec();panels.push(Panel::CustomToolbar(1));
    json!({
        "command":CommandId::ALL.to_vec(),"tool_slot":ToolSlotId::ALL.to_vec(),"tool_group":ToolGroup::ALL.to_vec(),"panel":panels,"panel_control":crate::PanelControl::ALL.to_vec(),
        "tile_style":TileStyle::ALL.to_vec(),"tab_style":crate::TabStyle::ALL.to_vec(),"header_size":HeaderSize::ALL.to_vec(),"edge":Edge::ALL.to_vec(),"axis":Axis::ALL.to_vec(),
        "edge_alignment":crate::EdgeAlignment::ALL.to_vec(),"floating_toolbar_layout":crate::FloatingToolbarLayout::ALL.to_vec(),"layout_panel_action":LayoutPanelAction::ALL.to_vec(),
        "corner":crate::Corner::ALL.to_vec(),"header_item":items,"toolbar_control":controls,"bubble_source":sources,"layout_change":changes,
        "brush":brush_ids().collect::<Vec<_>>(),"tool_setting":tool_setting_ids(english),
    })
}
fn check_saved_names(names:&Value) {
    fn read<T:serde::de::DeserializeOwned>(names:&Value,key:&str)->Vec<T> {
        names[key].as_array().unwrap_or_else(||panic!("saved names lack {key}")).iter()
            .map(|name|serde_json::from_value(name.clone()).unwrap_or_else(|error|panic!("{key} {name} no longer reads: {error}"))).collect()
    }
    read::<CommandId>(names,"command");read::<ToolSlotId>(names,"tool_slot");read::<ToolGroup>(names,"tool_group");read::<Panel>(names,"panel");
    read::<crate::PanelControl>(names,"panel_control");read::<TileStyle>(names,"tile_style");read::<crate::TabStyle>(names,"tab_style");
    read::<HeaderSize>(names,"header_size");read::<Edge>(names,"edge");read::<Axis>(names,"axis");read::<crate::EdgeAlignment>(names,"edge_alignment");
    read::<crate::FloatingToolbarLayout>(names,"floating_toolbar_layout");read::<LayoutPanelAction>(names,"layout_panel_action");
    read::<crate::Corner>(names,"corner");read::<HeaderItem>(names,"header_item");
    for control in read::<ToolbarControl>(names,"toolbar_control") {assert!(control.validate_admission().is_ok(),"toolbar control {control:?} no longer opens");}
    for source in read::<BubbleSource>(names,"bubble_source") {assert!(source.validate().is_ok(),"bubble {source:?} no longer opens");}
    for change in read::<LayoutChange>(names,"layout_change") {assert!(change.validate().is_ok(),"history entry {change:?} no longer opens");}
    for id in read::<u32>(names,"brush") {assert!(preset(id).is_ok(),"brush {id} no longer opens");}
    for id in read::<String>(names,"tool_setting") {assert!(crate::tool_settings::defined(&id),"tool setting {id} no longer opens");}
}
fn complete_layout(english:&Localizer)->DockLayout {
    use ToolbarControl as C;
    let mut layout=serde_json::to_value(crate::WorkspacePreset::Illustrator.layout(Platform::Gtk)).unwrap();
    let mut next=layout["next_id"].as_u64().unwrap();let mut tile=layout["next_tile_id"].as_u64().unwrap();
    let mut id=||{next+=1;next-1};
    let mut tiles=|controls:Vec<ToolbarControl>|controls.into_iter().map(|control|{tile+=1;json!({"id":tile-1,"control":control})}).collect::<Vec<_>>();
    for config in layout["panels"].as_array_mut().unwrap().iter_mut().filter(|config|config["content"]["kind"]=="controls") {
        let panel:Panel=serde_json::from_value(config["id"].clone()).unwrap();
        config["content"]["visible"]=json!(crate::PanelControl::available(panel));
    }
    let mut controls:Vec<ToolbarControl>=serde_json::from_value(saved_names(english)["toolbar_control"].clone()).unwrap();
    controls.extend(CommandId::ALL.map(|command|C::Command{command}));
    controls.extend(ToolSlotId::ALL.map(|slot|C::ToolSlot{slot}));
    controls.extend(Panel::ALL.into_iter().map(|panel|C::Panel{panel}).filter(|control|control.validate_admission().is_ok()));
    controls.extend(brush_ids().map(|id|C::Brush{id}));
    let mut toolbars=Vec::new();
    for (index,style) in TileStyle::ALL.iter().cycle().take(7).enumerate() {
        let panel=Panel::CustomToolbar(id() as u32);
        let content=tiles(if index==0 {controls.clone()} else {vec![C::Color]});
        toolbars.push((panel,json!({"id":panel,"hide_tab":false,"tile_style":style,"content":{"kind":"toolbar","name":format!("Coverage {index}"),"tiles":content}})));
    }
    let tab_styles=crate::TabStyle::ALL;
    let tabs=|index:usize,id:u64|json!({"kind":"tabs","id":id,"panels":[toolbars[index].0],"active":toolbars[index].0,"tab_style":tab_styles[index%tab_styles.len()]});
    let bands=[
        json!({"alignment":"start","id":id(),"edge":"bottom","extent":0.,"root":tabs(0,id())}),
        json!({"alignment":"center","id":id(),"edge":"bottom","extent":0.,"root":{"kind":"split","id":id(),"axis":"horizontal","fraction":0.5,"first":tabs(1,id()),"second":tabs(2,id())}}),
        json!({"alignment":"end","id":id(),"edge":"bottom","extent":0.,"root":tabs(3,id())}),
    ];
    layout["bands"].as_array_mut().unwrap().extend(bands);
    for (index,toolbar_layout) in crate::FloatingToolbarLayout::ALL.into_iter().enumerate() {
        layout["floating"].as_array_mut().unwrap().push(json!({"root":tabs(4+index,id()),"position":[100.+index as f32*60.,120.],"width":120.,"default_width":null,"height":null,"toolbar_layout":toolbar_layout}));
    }
    layout["panels"].as_array_mut().unwrap().extend(toolbars.iter().map(|(_,config)|config.clone()));
    let present:Vec<HeaderItem>=layout["header"]["zones"].as_array().unwrap().iter().flat_map(|zone|zone.as_array().unwrap().iter().map(|entry|serde_json::from_value(entry["item"].clone()).unwrap())).collect();
    let mut header_id=layout["header"]["next_id"].as_u64().unwrap();
    let mut added=Vec::new();
    for item in HeaderItem::COMPONENTS.into_iter().filter(|item|!present.contains(item)).chain([HeaderItem::Tool{control:C::Command{command:CommandId::Undo}}]) {
        added.push(json!({"id":header_id,"item":item}));header_id+=1;
    }
    layout["header"]["zones"][2].as_array_mut().unwrap().extend(added);
    layout["header"]["next_id"]=json!(header_id);
    let mut sources=vec![BubbleSource::Control{control:C::Color},BubbleSource::Control{control:C::ColorPicker},
        BubbleSource::Control{control:C::Command{command:CommandId::Undo}},BubbleSource::Control{control:C::Panel{panel:Panel::Layers}},
        BubbleSource::Control{control:C::Brush{id:brush_ids().next().unwrap()}}];
    sources.extend(ToolGroup::ALL.map(|group|BubbleSource::Medium{group}));
    let bubbles:Vec<_>=sources.iter().zip(crate::Corner::ALL.iter().cycle()).map(|(source,corner)|json!({"id":id(),"source":source,"corner":corner,"offset":[80.,80.]})).collect();
    let mut settings=tool_setting_ids(english).into_iter();
    let rings:Vec<_>=brush_ids().map_while(|brush|{let ring:Vec<_>=settings.by_ref().take(crate::MAX_RING_SETTINGS).collect();(!ring.is_empty()).then(||json!({"brush":brush,"settings":ring}))}).collect();
    layout["bubbles"]=json!({"show_sliders":false,"bubbles":bubbles,"rings":rings,"kept":[]});
    layout["next_id"]=json!(next);layout["next_tile_id"]=json!(tile);
    let layout:DockLayout=serde_json::from_value(layout).unwrap();
    layout.validate().unwrap();
    layout
}
fn complete_history(english:&Localizer)->LayoutHistory {
    let mut layout=complete_layout(english);
    let mut edit=crate::LayoutEdit::starting(&layout);
    let changes:Vec<LayoutChange>=serde_json::from_value(saved_names(english)["layout_change"].clone()).unwrap();
    let actions=LayoutPanelAction::ALL.into_iter().map(|action|LayoutChange::panels(action,vec![LayoutPanelName{panel:Panel::Layers,custom_name:None}]));
    for (index,change) in changes.into_iter().chain(actions).enumerate() {
        layout.canvas_bar = !layout.canvas_bar;layout.header.size=HeaderSize::ALL[index%HeaderSize::ALL.len()];
        edit.append(&layout,change);
    }
    let history=edit.into_history();
    history.validate().unwrap();
    history
}

#[test]
fn released_saved_names_still_read() {
    let releases:Vec<_>=std::fs::read_dir(root()).unwrap().map(|entry|entry.unwrap().path().join("saved-names.json")).filter(|path|path.is_file()).collect();
    assert!(!releases.is_empty());
    for path in releases {check_saved_names(&serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap());}
    check_saved_names(&saved_names(&Localizer::shared(UiLanguage::English)));
}

#[test]
fn generated_fixtures_cover_every_saved_name() {
    type Found=std::collections::BTreeMap<&'static str,std::collections::BTreeSet<String>>;
    fn name(value:&Value)->String {
        let value=["kind","change"].into_iter().find_map(|tag|value.get(tag)).unwrap_or(value);
        if value.as_str().is_some_and(|name|name.starts_with("toolbar:")) {"toolbar:*".into()} else {value.to_string()}
    }
    fn add(found:&mut Found,key:&'static str,value:&Value) {found.entry(key).or_default().insert(name(value));}
    fn collect(value:&Value,found:&mut Found) {
        match value {
            Value::Object(fields)=>for (key,field) in fields {
                match key.as_str() {
                    "command"=>add(found,"command",field),"edge"=>add(found,"edge",field),"axis"=>add(found,"axis",field),
                    "tile_style"=>add(found,"tile_style",field),"tab_style"=>add(found,"tab_style",field),"corner"=>add(found,"corner",field),
                    "slot"=>add(found,"tool_slot",field),"group" if field.is_string()=>add(found,"tool_group",field),"alignment"=>add(found,"edge_alignment",field),
                    "toolbar_layout"=>add(found,"floating_toolbar_layout",field),"action" if field.is_string()=>add(found,"layout_panel_action",field),
                    "size" if field.is_string()=>add(found,"header_size",field),"brush"=>add(found,"brush",field),
                    "visible"=>for control in field.as_array().into_iter().flatten() {add(found,"panel_control",control);},
                    "settings"=>for setting in field.as_array().into_iter().flatten() {add(found,"tool_setting",setting);},
                    "panels" if field.as_array().is_some_and(|panels|panels.iter().all(Value::is_string))=>for panel in field.as_array().unwrap() {add(found,"panel",panel);},
                    "control"=>{
                        add(found,"toolbar_control",field);
                        if field["kind"]=="brush" {add(found,"brush",&field["id"]);}
                        if field["kind"]=="panel" {add(found,"panel",&field["panel"]);}
                    }
                    "item"=>add(found,"header_item",field),"source"=>add(found,"bubble_source",field),
                    "description"=>add(found,"layout_change",field),
                    _=>{},
                }
                collect(field,found);
            },
            Value::Array(items)=>for item in items {collect(item,found);},
            _=>{},
        }
    }
    let directory=std::env::temp_dir().join(format!("capy-saved-names-{}",PortableId::random()));
    generate(&directory);
    let names:Value=serde_json::from_slice(&std::fs::read(directory.join("saved-names.json")).unwrap()).unwrap();
    assert_eq!(names,saved_names(&Localizer::shared(UiLanguage::English)));
    let mut found=std::collections::BTreeMap::new();
    collect(&serde_json::from_slice(&std::fs::read(directory.join("workspace-layout.json")).unwrap()).unwrap(),&mut found);
    let mut missing=Vec::new();
    for (key,expected) in names.as_object().unwrap() {
        for value in expected.as_array().unwrap() {
            if !found.get(key.as_str()).is_some_and(|seen|seen.contains(&name(value))) {missing.push(format!("{key} {value}"));}
        }
    }
    assert!(missing.is_empty(),"generated workspace fixtures miss saved names: {missing:?}");
    std::fs::remove_dir_all(directory).unwrap();
}
