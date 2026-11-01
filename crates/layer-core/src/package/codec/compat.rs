use super::*;
use crate::ProjectLimits;
use std::path::{Path, PathBuf};
use sha2::{Digest,Sha256};

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat") }
fn digest(bytes:&[u8])->String {Sha256::digest(bytes).iter().map(|b|format!("{b:02x}")).collect()}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory = PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    std::fs::create_dir_all(&directory).unwrap();
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        std::fs::write(directory.join(format!("samples-{depth:?}.capy")),serialize(&prepare(&fixture(depth),true))).unwrap();
    }
    for name in ["authored-filters.capy","shared-image-objects.capy","vector-objects.capy","illustration-conversions.capy","layer-color-modes.capy","papers.capy","unsupported-pass-through.capy"] {
        let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/package/codec/fixtures").join(name)).unwrap();
        let bytes = if name.starts_with("unsupported") {bytes} else {serialize(&prepare(&editable(bytes),true))};
        std::fs::write(directory.join(name),bytes).unwrap();
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn truncated_released_files_fail_without_changing_the_source() {
    let fixture = root().join("1.0.15/samples-U8.capy");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&fixture).unwrap()).unwrap();
    let resource = archive.by_name("data/tiles-1.bin").unwrap();
    let cuts = [resource.central_header_start()+16, resource.data_start().unwrap()+resource.size()/2];
    let directory = std::env::temp_dir().join(format!("capy-truncated-package-{}",PortableId::random()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("drawing.capy");
    let cancel = AtomicBool::new(false);
    for cut in cuts {
        std::fs::copy(&fixture,&path).unwrap();
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(cut).unwrap();
        let original = std::fs::read(&path).unwrap();
        let source = crate::package::transport::spool(&mut std::fs::File::open(&path).unwrap(),&directory,cut,&cancel).unwrap();
        assert_eq!(source.resident_bytes(),0);
        let opened = open(source,ProjectLimits::default(),&cancel);
        assert!(matches!(&opened,Err(_) | Ok(OpenOutcome::Failure {..} | OpenOutcome::RecoveredView {..})), "cut {cut}: {opened:?}");
        drop(opened);
        assert_eq!(std::fs::read(&path).unwrap(),original,"cut {cut}");
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn release_fixtures_load_without_loss() {
    for entry in std::fs::read_dir(root()).unwrap() {
        let path=entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap()!="expected" {check_release(path.file_name().unwrap().to_str().unwrap());}
    }
}

fn check_release(release:&str) {
    for path in std::fs::read_dir(root().join(release)).unwrap().map(|entry|entry.unwrap().path()) {
        if path.extension().and_then(|value|value.to_str()) != Some("capy") {continue;}
        let bytes = std::fs::read(&path).unwrap();
        let actual = match open(backing(bytes.clone()),ProjectLimits::default(),&AtomicBool::new(false)).unwrap() {
            OpenOutcome::Candidate {artwork,preview,..} => {
                if path.file_name().unwrap()=="app-preview.capy" {assert!(preview.is_some());}
                let prepared = prepare(&artwork,false);
                let manifest = serde_json::from_slice::<Value>(prepared.manifest()).unwrap();
                let reopened=editable(serialize(&prepared));
                assert_eq!(prepare(&reopened,false).manifest(),prepared.manifest());
                super::roundtrip_semantics::exact_rasters(&artwork,&reopened);
                let resources:std::collections::BTreeMap<_,_>=prepared.resources().entries.iter()
                    .map(|entry|(entry.record["id"].as_str().unwrap().to_owned(),digest(&entry.payload.encoded().unwrap().bytes))).collect();
                json!({"manifest":manifest,"resources":resources,"preview":preview.map(|p|json!({"size":p.size(),"pixels":digest(p.pixels())}))})
            }
            OpenOutcome::Preserved {source,..} if path.file_name().unwrap() == "unsupported-pass-through.capy" => {
                let mut copied = Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();
                assert_eq!(copied,bytes);json!({"preserved":digest(&bytes)})
            }
            outcome => panic!("{}: {outcome:?}",path.display()),
        };
        let expected = root().join("expected").join(release).join(format!("{}.json",path.file_name().unwrap().to_str().unwrap()));
        if std::env::var_os("CAPY_BLESS").as_deref() == Some(std::ffi::OsStr::new("1")) {
            std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
            std::fs::write(&expected,serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        }
        assert_eq!(actual,serde_json::from_slice::<Value>(&std::fs::read(&expected).unwrap()).unwrap(),"{}",expected.display());
    }
}
