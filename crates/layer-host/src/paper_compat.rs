use super::*;

fn root()->PathBuf {Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat")}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory=PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT")).join("papers");
    store(&directory,&photo_paper(),"Release paper 色").unwrap();
}

#[test]
fn release_fixtures_load_without_loss() {
    for entry in std::fs::read_dir(root()).unwrap() {
        let path=entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap()!="expected" {check_release(path.file_name().unwrap().to_str().unwrap());}
    }
}

fn check_release(release:&str) {
    let source=root().join(release).join("papers");
    let directory=std::env::temp_dir().join(format!("capy-release-papers-{}",layer_core::PortableId::random()));
    std::fs::create_dir_all(&directory).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry=entry.unwrap();std::fs::copy(entry.path(),directory.join(entry.file_name())).unwrap();
    }
    let entries=list(&directory).unwrap();assert_eq!(entries.len(),1);
    let values:Vec<_>=entries.iter().map(|entry| {
        let paper=load(&directory,&entry.id).unwrap();
        assert_eq!(paper_identity(&paper).unwrap(),entry.id);
        assert_eq!(layer_core::paper::image_preview(&paper).unwrap().as_slice(),entry.preview.as_ref());
        serde_json::json!({"id":entry.id,"name":entry.name,"bytes":entry.bytes,"preview_sha256":layer_ui::profile_library::profile_identity(&entry.preview)})
    }).collect();
    let expected=root().join("expected").join(release).join("papers.json");let value=serde_json::json!(values);
    if std::env::var_os("CAPY_BLESS").as_deref()==Some(std::ffi::OsStr::new("1")) {
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected,serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
    assert_eq!(value,serde_json::from_slice::<serde_json::Value>(&std::fs::read(expected).unwrap()).unwrap());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saved_documents_tolerate_missing_and_unknown_fields() {
    let bytes=std::fs::read(root().join("1.0.15/papers/index.json")).unwrap();
    let mut value:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
    value["future"]=true.into();value["papers"][0]["future"]=true.into();
    let index:Index=serde_json::from_value(value.clone()).unwrap();
    assert_eq!(index.papers[0].id,serde_json::from_slice::<Index>(&bytes).unwrap().papers[0].id);
    for field in ["id","name"] {
        let mut missing=value.clone();missing["papers"][0].as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<Index>(missing).is_err(),"{field}");
    }
    value.as_object_mut().unwrap().remove("papers");assert!(serde_json::from_value::<Index>(value).is_err());
}
