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
    std::fs::create_dir_all(&directory).unwrap();
    let mut settings=Settings::default();crate::keymaps::select(&mut settings,"krita").unwrap();
    settings.shortcuts.insert(CommandId::Undo.shortcut_id(),vec![]);
    settings.gestures.insert("touch.tap.2".into(),CommandId::Redo.shortcut_id());
    settings.hold_keys=Some(settings.default_hold_keys(Platform::Gtk));
    settings.validate().unwrap();
    std::fs::write(directory.join("bindings.capykeys"),crate::keymaps::export(&settings)).unwrap();
    let profile=layer_color::profile_bytes(&layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::DisplayP3)).unwrap();
    let record=crate::profile_library::prepare_profile_import(vec![],&profile).unwrap();
    std::fs::write(directory.join(format!("{}.icc",record.id)),&profile).unwrap();
    write(&directory,"profiles.json",&vec![crate::profile_library::ProfileRecord{id:record.id.clone(),bytes:profile.len() as u64,issue:None}]);
    let hidden=crate::profile_library::ProfileLibraryAction::Visibility {hidden:vec![],id:Some(record.id),visible:Some(false)}.execute(&[]).unwrap();
    write(&directory,"hidden-profiles.json",&hidden);
    let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
    layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(1,1).unwrap();save(&mut source,true);
    source.state.camera.zoom_to(1.75).unwrap();
    source.files.last_export=Some(ExportRepeat{recipe:ExportRecipe::wide_color(),location:DocumentLocation{uri:"private:release.png".into(),name:"Release.png".into()}});
    let save=|key:&str,source:&UiSession<Recorder>| {
        let capture=source.capture_session().unwrap();
        let mut store=layer_core::package::session_store::SessionStore::open(&directory.join(key)).unwrap();
        store.commit(&capture.prepare(&AtomicBool::new(false)).unwrap(),&AtomicBool::new(false)).unwrap();
        drop(store);std::fs::remove_file(directory.join(key).join(".lock")).unwrap();
    };
    save("drawing-a",&source);
    write(&directory,"session.json",&SessionManifest::default().reconcile(vec![SessionDrawing{id:7,key:"drawing-a".into()}],7,false).unwrap());
    let mut recipe=ExportRecipe::wide_color();recipe.profile.profile=layer_core::color::ColorProfile::Icc(profile.to_vec().into());
    let mut presets=crate::ExportPresets::default();presets.save("Release ICC",recipe.clone()).unwrap();presets.remember(0,recipe.clone()).unwrap();
    std::fs::write(directory.join("export-presets-icc"),presets.encode().unwrap()).unwrap();
    source.files.last_export.as_mut().unwrap().recipe=recipe;
    save("drawing-icc",&source);
    write(&directory,"session-icc.json",&SessionManifest::default().reconcile(vec![SessionDrawing{id:9,key:"drawing-icc".into()}],9,false).unwrap());
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
    let directory=root().join(release);let english=Localizer::shared(crate::UiLanguage::English);
    let keymap=std::fs::read_to_string(directory.join("bindings.capykeys")).unwrap();
    let imported=crate::keymaps::import_typed(&Settings::default(),&keymap,Platform::Gtk,&english).unwrap();
    expect(release,"bindings.json",serde_json::from_str(&crate::keymaps::export(&imported.settings)).unwrap());
    let records:Vec<crate::profile_library::ProfileRecord>=serde_json::from_slice(&std::fs::read(directory.join("profiles.json")).unwrap()).unwrap();
    let entries:Vec<_>=records.iter().map(|record| {
        let bytes=std::fs::read(directory.join(format!("{}.icc",record.id))).unwrap();
        assert_eq!(bytes.len() as u64,record.bytes);
        assert_eq!(crate::profile_library::profile_identity(&bytes),record.id);
        let entry=crate::profile_library::inspect_library_entry(record,Ok(&bytes));assert!(entry.issue.is_none());entry
    }).collect();
    let hidden:Vec<String>=serde_json::from_slice(&std::fs::read(directory.join("hidden-profiles.json")).unwrap()).unwrap();
    let hidden=crate::profile_library::ProfileLibraryAction::Visibility{hidden,id:None,visible:None}.execute(&[]).unwrap();
    expect(release,"profiles.json",json!({"entries":entries,"hidden":hidden}));
    let presets=crate::ExportPresets::restore(&std::fs::read(directory.join("export-presets-icc")).unwrap());
    assert_eq!(presets.names().collect::<Vec<_>>(),["Release ICC"]);
    let recipes:Vec<_>=[0,4].into_iter().map(|index| {
        let recipe=presets.recipe(index,Default::default()).unwrap();
        let layer_core::color::ColorProfile::Icc(bytes)=&recipe.profile.profile else {panic!("missing preset ICC")};
        let profile=recipe.profile.clone().with_profile(crate::profile_library::profile_identity(bytes));
        recipe.with_profile(profile)
    }).collect();
    expect(release,"export-presets-icc.json",json!(recipes));
    for name in ["session.json","session-icc.json"] {
        let manifest=SessionManifest::parse(&std::fs::read(directory.join(name)).unwrap()).unwrap();
        expect(release,if name=="session.json" {"membership.json"}else{"membership-icc.json"},serde_json::to_value(&manifest).unwrap());
        let temporary=std::env::temp_dir().join(format!("capy-release-ui-{}",PortableId::random()));
        copy_tree(&directory.join(&manifest.drawings[0].key),&temporary);
        let cancel=AtomicBool::new(false);let mut store=layer_core::package::session_store::SessionStore::open(&temporary).unwrap();
        let restored=SessionRestore::from_core(store.load(ProjectLimits::default(),&cancel).unwrap().unwrap()).unwrap();
        let profile=restored.last_export.as_ref().and_then(|last|match &last.recipe.profile.profile {
            layer_core::color::ColorProfile::Icc(bytes)=>Some(crate::profile_library::profile_identity(bytes)),_=>None,
        });
        let metadata=serde_json::to_value(&restored.state).unwrap();
        let mut target=UiSession::from_project(Recorder::default(),restored.document().clone(),None,[500,400],Platform::Gtk).unwrap();
        target.frame(0,0).unwrap();let observed=restored.state.destination.clone();target.restore_session(restored,false,observed).unwrap();
        assert!(!target.state.document_file.modified);assert!(target.engine.can_undo());
        let mut actual=json!({"metadata":metadata,"camera":target.session_document_state().camera,"checkpoint":target.engine.checkpoint(),"saved":target.files.saved_checkpoint});
        if let Some(profile)=profile {actual["export_profile_sha256"]=profile.into();}
        expect(release,name,actual);
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
