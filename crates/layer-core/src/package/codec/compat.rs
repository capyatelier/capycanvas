use super::*;
use crate::ProjectLimits;
use std::path::{Path, PathBuf};
use sha2::{Digest,Sha256};

#[path="sweep.rs"]
mod sweep;

const VIEW_ONLY:&str="unsupported-pass-through.capy";
fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compat") }
fn digest(bytes:&[u8])->String {Sha256::digest(bytes).iter().map(|b|format!("{b:02x}")).collect()}
fn resource_digest(payload:&resources::Payload)->String {
    let mut hash=crate::package::trusted_programs::HashWriter::default();
    if let resources::Payload::Opaque(resource)=payload {
        let mut offset=0;
        while offset<resource.length {
            let count=(resource.length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            hash.write_all(&resource.read_chunk(offset,count,&AtomicBool::new(false)).unwrap()).unwrap();
            offset+=count as u64;
        }
    } else {hash.write_all(&payload.encoded().unwrap().bytes).unwrap();}
    hash.hex()
}

#[test]
#[ignore]
fn write_compat_fixtures() {
    let directory = PathBuf::from(std::env::var_os("CAPY_COMPAT_OUT").expect("CAPY_COMPAT_OUT"));
    generate(&directory);
}

pub(crate) fn generate(directory:&Path) {
    std::fs::create_dir_all(directory.join("newer")).unwrap();
    for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        std::fs::write(directory.join(format!("samples-{depth:?}.capy")),serialize(&prepare(&fixture(depth),true))).unwrap();
    }
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/package/codec/fixtures");
    for name in ["authored-filters.capy","shared-image-objects.capy","vector-objects.capy","vector-future.capy","illustration-conversions.capy","layer-color-modes.capy","papers.capy"] {
        std::fs::write(directory.join(name),serialize(&prepare(&editable(std::fs::read(fixtures.join(name)).unwrap()),true))).unwrap();
    }
    std::fs::copy(fixtures.join(VIEW_ONLY),directory.join("newer").join(VIEW_ONLY)).unwrap();
    sweep::generate(directory);
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
    check_inputs(&root().join(release),|name,actual| {
        let expected = root().join("expected").join(release).join(format!("{name}.json"));
        if std::env::var_os("CAPY_BLESS").as_deref() == Some(std::ffi::OsStr::new("1")) {
            std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
            std::fs::write(&expected,serde_json::to_vec_pretty(&actual).unwrap()).unwrap();
        }
        assert_eq!(actual,serde_json::from_slice::<Value>(&std::fs::read(&expected).unwrap()).unwrap(),"{}",expected.display());
    });
}

#[test]
fn external_compat_inputs_load() {
    let Some(directory)=std::env::var_os("CAPY_COMPAT_IN") else {return;};
    let directory=PathBuf::from(directory);
    check_inputs(&directory,|_,_|{});
    crate::package::session::persistence_tests::compat::check_inputs(&directory,|_,_|{});
}

fn check_inputs(directory:&Path,check:impl Fn(&str,Value)) {
    let marked=directory.join("newer");
    for path in std::fs::read_dir(directory).unwrap().chain(std::fs::read_dir(&marked).into_iter().flatten()).map(|entry|entry.unwrap().path()) {
        if path.extension().and_then(|value|value.to_str()) != Some("capy") {continue;}
        let newer=path.parent()==Some(marked.as_path());
        let file=path.file_name().unwrap().to_str().unwrap();
        let name=if newer {format!("newer/{file}")} else {file.to_owned()};
        let bytes = std::fs::read(&path).unwrap();
        let actual = match open(backing(bytes.clone()),ProjectLimits::default(),&AtomicBool::new(false)).unwrap() {
            OpenOutcome::Candidate {artwork,preview,..} => {
                if name=="app-preview.capy" {assert!(preview.is_some());}
                let prepared = prepare(&artwork,false);
                let manifest = serde_json::from_slice::<Value>(prepared.manifest()).unwrap();
                let reopened=editable(serialize(&prepared));
                assert_eq!(prepare(&reopened,false).manifest(),prepared.manifest());
                super::roundtrip_semantics::exact_rasters(&artwork,&reopened);
                let resources:std::collections::BTreeMap<_,_>=prepared.resources().entries.iter()
                    .map(|entry|(entry.record["id"].as_str().unwrap().to_owned(),resource_digest(&entry.payload))).collect();
                json!({"manifest":manifest,"resources":resources,"preview":preview.map(|p|json!({"size":p.size(),"pixels":digest(p.pixels())}))})
            }
            OpenOutcome::Preserved {source,..} if newer || name == VIEW_ONLY => {
                let mut copied = Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();
                assert_eq!(copied,bytes);json!({"preserved":digest(&bytes)})
            }
            outcome => panic!("{}: {outcome:?}",path.display()),
        };
        check(&name,actual);
    }
}

#[test]
fn generated_compat_fixtures_load_without_expectations() {
    let directory=std::env::temp_dir().join(format!("capy-generated-compat-{}",PortableId::random()));
    generate(&directory);crate::package::session::persistence_tests::compat::generate(&directory);
    let view_only=std::cell::RefCell::new(Vec::new());
    check_inputs(&directory,|name,actual|if actual.get("preserved").is_some() {view_only.borrow_mut().push(name.to_owned());});
    assert_eq!(*view_only.borrow(),[format!("newer/{VIEW_ONLY}")]);
    crate::package::session::persistence_tests::compat::check_inputs(&directory,|_,_|{});
    std::fs::copy(directory.join("newer").join(VIEW_ONLY),directory.join("unmarked.capy")).unwrap();
    assert!(std::panic::catch_unwind(||check_inputs(&directory,|_,_|{})).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn generated_fixtures_cover_every_saved_value() {
    let directory=std::env::temp_dir().join(format!("capy-record-coverage-{}",PortableId::random()));
    let codec=directory.join("codec");let sessions=directory.join("sessions");
    std::fs::create_dir_all(&codec).unwrap();std::fs::create_dir_all(&sessions).unwrap();
    generate(&codec);crate::package::session::persistence_tests::compat::generate(&sessions);
    check_inputs(&codec,|_,_|{});
    sweep::check(&directory);
    std::fs::remove_dir_all(directory).unwrap();
}
