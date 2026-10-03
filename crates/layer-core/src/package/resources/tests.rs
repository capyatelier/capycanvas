use super::*;
use crate::authored::Support;
use super::super::{manifest::{ResourceRecord,ResourceRange}, effect_records::{ResourceWriter as _,ResourceReader as _}};
use std::io::Read;

fn loaded(prepared:&PreparedResources, bytes:Arc<[u8]>) -> (Manifest,ImmutableBacking) {
    let mut resources=BTreeMap::new();let mut offset=0;
    for entry in &prepared.entries {
        let id=entry.payload.id();
        resources.insert(id,ResourceRecord {value:entry.record.clone(),bytes:entry.bytes,crc32:entry.crc,range:Some(ResourceRange {member:0,offset,length:entry.bytes})});
        offset+=entry.bytes;
    }
    (Manifest {document:PortableId::random(),root:PortableId::random(),objects:BTreeMap::new(),resources,outputs:Vec::new(),default_output:None,metadata:None,support:Support::Editable},ImmutableBacking::new(Arc::new(bytes)).unwrap())
}
#[test]
fn heavy_payloads_compress_losslessly_and_reuse_encoding_across_captures_and_open() {
    let cancelled=AtomicBool::new(false);
    let bytes=Resource::<[u8]>::from(b"exact camera metadata \0\xff".repeat(16384));
    let code=Resource::<str>::from("fn shade() -> vec4<f32> { return vec4<f32>(1.); }\n".repeat(2048));
    let lut=Lut3d::from_samples(33,[[0.;3],[1.;3]],"Large exact cube".into(),(0..33u32.pow(3)).map(|i|[(i%33) as f32/32.,(i/33%33) as f32/32.,(i/33/33) as f32/32.]).collect::<Vec<_>>().into()).unwrap();
    let mut inventory=ResourceInventory::default();
    let metadata=inventory.bytes("capy.photo-metadata/1",&bytes,json!({"kind":"xmp"})).unwrap();
    let shader=inventory.code(&code).unwrap();
    let lookup=inventory.lut(&lut).unwrap();
    let first=inventory.prepare(&cancelled).unwrap();
    assert!(first.length<(bytes.len()+code.len()+lut.bytes()) as u64/2);
    assert!(first.entries.iter().all(|entry|entry.record["encoding"]=="capy.lz4-bytes/1"));
    let before=bytes.encoded_if_ready().unwrap().bytes.clone();
    let second=inventory.clone().prepare(&cancelled).unwrap();
    assert!(Arc::ptr_eq(&before,&bytes.encoded_if_ready().unwrap().bytes));
    let mut packed=Vec::new();first.reader(&cancelled).read_to_end(&mut packed).unwrap();
    assert_eq!(crc32fast::hash(&packed),first.crc);
    let (manifest,backing)=loaded(&first,packed.clone().into());
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    let restored=reader.bytes(&metadata,"capy.photo-metadata/1",crate::PhotoMetadata::MAX_BYTES).unwrap();
    let restored_code=reader.code(&shader).unwrap();
    let restored_lut=reader.lut(&lookup).unwrap();
    assert_eq!(restored,bytes);assert_eq!(restored.id(),bytes.id());
    assert_eq!(restored_code,code);assert_eq!(restored_code.id(),code.id());
    assert_eq!(restored_lut.payload(),lut.payload());assert_eq!(restored_lut.resource().unwrap().id(),lut.resource().unwrap().id());
    let mut reopened=ResourceInventory::default();
    reopened.bytes("capy.photo-metadata/1",&restored,json!({"kind":"xmp"})).unwrap();reopened.code(&restored_code).unwrap();reopened.lut(&restored_lut).unwrap();
    let third=reopened.prepare(&cancelled).unwrap();
    for candidate in [&second,&third] {
        let mut again=Vec::new();candidate.reader(&cancelled).read_to_end(&mut again).unwrap();
        assert_eq!(again,packed);
        assert_eq!(candidate.entries.iter().map(|e|&e.record).collect::<Vec<_>>(),first.entries.iter().map(|e|&e.record).collect::<Vec<_>>());
    }
}
#[test]
fn heavy_payload_bounds_corruption_and_cancellation_are_independent() {
    let cancelled=AtomicBool::new(false);
    let bytes=Resource::<[u8]>::from(vec![81;1024]);
    let mut inventory=ResourceInventory::default();let reference=inventory.bytes("capy.icc/1",&bytes,json!({})).unwrap();
    let prepared=inventory.prepare(&cancelled).unwrap();let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.clone().into());
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    assert!(reader.bytes(&reference,"capy.icc/1",1023).is_err());
    cancelled.store(true,Ordering::Relaxed);assert!(reader.bytes(&reference,"capy.icc/1",1024).is_err());
    cancelled.store(false,Ordering::Relaxed);assert_eq!(reader.bytes(&reference,"capy.icc/1",1024).unwrap(),bytes);
    packed[0]^=1;
    let (manifest,corrupted)=loaded(&prepared,packed.into());
    let mut reader=ResourceReader::new(&manifest,&corrupted,&cancelled,Default::default());
    assert!(reader.bytes(&reference,"capy.icc/1",1024).is_err());
    assert!(corrupted.poll(0,1).is_err());assert!(backing.poll(0,1).is_ok());
}
