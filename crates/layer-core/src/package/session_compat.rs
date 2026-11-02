use super::*;
use crate::package::session_store::SessionStore;
use std::path::{Path,PathBuf};
use sha2::{Digest,Sha256};

fn prepared(editor:&Editor,metadata:impl Into<crate::package::session::SessionMetadata>)->PreparedSession {
    let capture=editor.capture_session(editor.capture(11,editor.document().output().context.clone()).unwrap()).unwrap();
    PreparedSession::prepare(&capture,metadata,&AtomicBool::new(false)).unwrap()
}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory=PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    generate(&directory);
}

pub(crate) fn generate(directory:&Path) {
    let mut unsupported=fixture();perform_all(&mut unsupported);unsupported.undo().unwrap();unsupported.undo().unwrap();
    SessionStore::open(&directory.join("unsupported-session")).unwrap()
        .commit(&prepared(&unsupported,json!({"tab":"Release 色"})),&AtomicBool::new(false)).unwrap();
    std::fs::remove_file(directory.join("unsupported-session/.lock")).unwrap();
    let root=directory.join("session");
    let mut editor=fixture();
    for marker in [37,59,71] {
        let target=editor.document().working.target.unwrap();
        editor.perform(Edit::SetRaster{target,revision:raster(RasterPlane::Mask,marker,false)}).unwrap();
    }
    editor.undo().unwrap();
    let mut store=SessionStore::open(&root).unwrap();
    store.commit(&prepared(&editor,json!({"tab":"Release 色"})),&AtomicBool::new(false)).unwrap();
    drop(store);std::fs::remove_file(root.join(".lock")).unwrap();
}

fn copy_tree(source:&Path,destination:&Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry=entry.unwrap();let target=destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {copy_tree(&entry.path(),&target);} else {std::fs::copy(entry.path(),target).unwrap();}
    }
}

fn snapshot(editor:&Editor,metadata:serde_json::Value)->serde_json::Value {
    let prepared=prepared(editor,metadata);
    let mut value:serde_json::Value=serde_json::from_slice(prepared.metadata()).unwrap();
    value["checkpoint"]["owner"]=0.into();
    let resources:std::collections::BTreeMap<_,_>=prepared.resources().entries.iter().map(|entry| {
        let encoded=entry.payload.encoded().unwrap();
        (entry.record["id"].as_str().unwrap().to_owned(),Sha256::digest(&encoded.bytes).iter().map(|b|format!("{b:02x}")).collect::<String>())
    }).collect();
    json!({"metadata":value,"resources":resources})
}
fn history_snapshot(editor:&mut Editor,metadata:&serde_json::Value)->serde_json::Value {
    let original=snapshot(editor,metadata.clone());let mut states=Vec::new();
    while editor.can_undo() {editor.undo().unwrap();states.push(Sha256::digest(serde_json::to_vec(&snapshot(editor,metadata.clone())).unwrap()).iter().map(|b|format!("{b:02x}")).collect::<String>());}
    while editor.can_redo() {editor.redo().unwrap();states.push(Sha256::digest(serde_json::to_vec(&snapshot(editor,metadata.clone())).unwrap()).iter().map(|b|format!("{b:02x}")).collect::<String>());}
    json!({"metadata":original,"history":states})
}

#[test]
fn release_fixtures_load_without_loss() {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat");
    for entry in std::fs::read_dir(root).unwrap() {
        let path=entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap()!="expected" {check_release(path.file_name().unwrap().to_str().unwrap());}
    }
}

fn check_release(release:&str) {
    let root=Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat");
    check_inputs(&root.join(release),|name,actual| {
        let expected=root.join("expected").join(release).join(name);
        if std::env::var_os("CAPY_BLESS").as_deref()==Some(std::ffi::OsStr::new("1")) {
            std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
            std::fs::write(&expected,serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        }
        assert_eq!(actual,serde_json::from_slice::<serde_json::Value>(&std::fs::read(expected).unwrap()).unwrap());
    });
}

pub(crate) fn check_inputs(directory:&Path,check:impl Fn(&str,serde_json::Value)) {
    let temporary=std::env::temp_dir().join(format!("capy-release-session-{}",PortableId::random()));
    copy_tree(&directory.join("session"),&temporary);
    let cancel=AtomicBool::new(false);let mut store=SessionStore::open(&temporary).unwrap();
    let loaded=store.load(ProjectLimits::default(),&cancel).unwrap().unwrap();
    let mut editor=loaded.editor;let metadata=loaded.metadata.value;
    let actual=history_snapshot(&mut editor,&metadata);
    check("session.json",actual.clone());
    let loaded=store.load(ProjectLimits::default(),&cancel).unwrap().unwrap();
    store.commit(&prepared(&loaded.editor,loaded.metadata),&cancel).unwrap();
    drop(loaded.editor);drop(editor);drop(store);
    let mut upgraded=SessionStore::open(&temporary).unwrap();
    let mut loaded=upgraded.load(ProjectLimits::default(),&cancel).unwrap().unwrap();
    assert_eq!(history_snapshot(&mut loaded.editor,&loaded.metadata.value),actual);
    drop(loaded);drop(upgraded);std::fs::remove_dir_all(&temporary).unwrap();
    let source=directory.join("unsupported-session");copy_tree(&source,&temporary);
    let mut store=SessionStore::open(&temporary).unwrap();
    assert!(store.load(ProjectLimits::default(),&cancel).err().unwrap().contains("not approved"));
    drop(store);
    fn unchanged(source:&Path,copy:&Path) {
        for entry in std::fs::read_dir(source).unwrap() {
            let entry=entry.unwrap();let target=copy.join(entry.file_name());
            if entry.file_type().unwrap().is_dir(){unchanged(&entry.path(),&target);}else{assert_eq!(std::fs::read(entry.path()).unwrap(),std::fs::read(target).unwrap());}
        }
    }
    unchanged(&source,&temporary);std::fs::remove_dir_all(temporary).unwrap();
}
