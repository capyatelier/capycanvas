use super::*;
use crate::package::{ByteSource, archive::{Directory, Member, MIMETYPE}, manifest::{ManifestLimits, ManifestRead}, transport::ChunkedBytes};
use std::sync::{Mutex, atomic::AtomicUsize};

fn id(n: u8) -> PortableId { let mut bytes = [0; 16]; bytes[15] = n; PortableId::from_bytes(bytes) }
fn reference(n: u8) -> Value { json!({"ref":id(n)}) }
fn live() -> BTreeSet<PortableId> { [id(1),id(2),id(5)].into() }
fn note(n: u8, safe: bool, payload: u8) -> Value {
    json!({"id":id(n),"type":"future.note/1","ancillary":true,"copy_safe":safe,
        "data":{"subject":reference(2),"payload":reference(payload)}})
}
fn resource(n: u8, offset: u64, bytes: &[u8], data: Value) -> Value {
    json!({"id":id(n),"type":"future.attachment/1","data":data,"encoding":"future.codec/1",
        "location":{"pack":"data/tiles-1.bin","offset":offset.to_string()},
        "bytes":bytes.len().to_string(),"crc32":format!("{:08x}",crc32fast::hash(bytes))})
}
fn manifest(data: &[u8], resources: Vec<Value>, notes: Vec<Value>) -> (Manifest, ImmutableBacking) {
    let mut value: Value = serde_json::from_str(include_str!("../../../tests/fixtures/capy/empty.json")).unwrap();
    value["objects"].as_array_mut().unwrap().extend(notes);
    value["resources"] = resources.into();
    let mut bytes = Vec::new();
    let members = [("mimetype",MIMETYPE),("manifest.json",b"{}".as_slice()),("data/tiles-1.bin",data)].into_iter()
        .map(|(name,data)| { let member=Member {name:name.into(),offset:bytes.len() as u64,length:data.len() as u64,compressed_length:None,crc32:crc32fast::hash(data)};
            bytes.extend_from_slice(data); member }).collect();
    let directory = Directory { members, length:bytes.len() as u64 };
    let ManifestRead::Known(manifest) = Manifest::parse(&serde_json::to_vec(&value).unwrap(),&directory,ManifestLimits::default()).unwrap()
        else { panic!("unknown envelope") };
    let backing = ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    (manifest,backing)
}
fn fixture() -> (Manifest, ImmutableBacking) {
    let mut first=resource(10,0,b"abc",json!({"dictionary":reference(11)}));
    first["future_descriptor"] = json!({"profile":reference(12),"value":[1,2,3]});
    let mut known=resource(12,6,b"\xff\xfe\xfd",json!({}));
    known["type"]="capy.wgsl/1".into(); known["encoding"]="utf8".into();
    manifest(b"abcdef\xff\xfe\xfdjkl",vec![first,resource(11,3,b"def",json!({"subject":reference(1)})),known,
        resource(13,9,b"jkl",json!({}))],vec![note(7,true,10),note(8,false,13)])
}

#[test]
fn opaque_descriptors_bytes_and_identity_survive_capture_without_decoding() {
    let (manifest,backing)=fixture(); let cancelled=AtomicBool::new(false);
    let extensions=Extensions::load(&manifest,&backing,&cancelled).unwrap();
    assert_eq!(extensions.records.len(),2); assert_eq!(extensions.resources.len(),4);
    for (id,resource) in &extensions.resources {
        let original=&manifest.resources[id];
        assert_eq!(resource.backing.identity(),backing.identity());
        let location=json!({"member":format!("data/{id}")});
        let mut expected=original.value.clone();expected["location"]=location.clone();
        assert_eq!(resource.record(location),expected);
        assert_eq!(crc32fast::hash(&resource.read_chunk(0,resource.length as usize,&cancelled).unwrap()),resource.crc32);
    }
    assert_eq!(&*extensions.resources[&id(12)].read_chunk(0,3,&cancelled).unwrap(),b"\xff\xfe\xfd");
    let (records,resources)=extensions.edited_retained(&live()).unwrap();
    assert_eq!(records,vec![manifest.objects[&id(7)].clone()]);
    assert_eq!(resources.iter().map(|r|r.id).collect::<Vec<_>>(),vec![id(10),id(11),id(12)]);
    assert!(Arc::ptr_eq(&resources[0],&extensions.resources[&id(10)]));
    let captured=extensions.clone();drop(extensions);drop(backing);drop(manifest);
    assert_eq!(&*captured.resources[&id(10)].read_chunk(0,3,&cancelled).unwrap(),b"abc");
}

#[test]
fn edited_capture_drops_deleted_or_unresolved_records_and_exclusive_resources() {
    let (manifest,backing)=fixture();let extensions=Extensions::load(&manifest,&backing,&AtomicBool::new(false)).unwrap();
    for deleted in [id(1),id(2)] {
        let mut ids=live();ids.remove(&deleted);
        let (records,resources)=extensions.edited_retained(&ids).unwrap();assert!(records.is_empty());assert!(resources.is_empty());
    }
    let mut missing=extensions.clone();missing.resources.remove(&id(11));
    let (records,resources)=missing.edited_retained(&live()).unwrap();assert!(records.is_empty());assert!(resources.is_empty());
    let mut unknown=extensions.clone();unknown.records.get_mut(&id(7)).unwrap()["data"]["future_subject"]=reference(99);
    let (records,resources)=unknown.edited_retained(&live()).unwrap();assert!(records.is_empty());assert!(resources.is_empty());
    let mut shared=extensions;shared.records.insert(id(9),note(9,true,13));
    let (records,resources)=shared.edited_retained(&live()).unwrap();
    assert_eq!(records.iter().map(|r|r["id"].clone()).collect::<Vec<_>>(),vec![json!(id(7)),json!(id(9))]);
    assert_eq!(resources.iter().map(|r|r.id).collect::<Vec<_>>(),vec![id(10),id(11),id(12),id(13)]);
}

#[test]
fn resource_cycles_terminate_and_retain_the_complete_reachable_closure() {
    let (manifest,backing)=manifest(b"abcdef",vec![resource(10,0,b"abc",json!({"next":reference(11)})),
        resource(11,3,b"def",json!({"next":reference(10)}))],vec![note(7,true,10)]);
    let extensions=Extensions::load(&manifest,&backing,&AtomicBool::new(false)).unwrap();
    let (records,resources)=extensions.edited_retained(&live()).unwrap();
    assert_eq!(records.len(),1);assert_eq!(resources.len(),2);
}

#[test]
fn resource_checksum_failure_poisons_every_borrower_of_the_backing() {
    let (mut manifest,backing)=fixture();
    manifest.resources.get_mut(&id(11)).unwrap().crc32 ^= 1;
    let other_owner=backing.clone();
    let error=Extensions::load(&manifest,&backing,&AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("checksum"));assert_eq!(other_owner.poll(0,1).unwrap_err(),error);
}

struct DeferredSource { bytes:Arc<[u8]>, pending:AtomicBool, cancel:Arc<AtomicBool>, polls:AtomicUsize }
impl ByteSource for DeferredSource {
    fn byte_len(&self) -> u64 { self.bytes.len() as u64 }
    fn poll(&self, offset:u64, length:usize) -> Result<RangeState,String> {
        self.polls.fetch_add(1,Ordering::Relaxed);
        if self.pending.load(Ordering::Relaxed) { return Ok(RangeState::Pending); }
        self.cancel.store(true,Ordering::Relaxed);
        self.bytes.poll(offset,length)
    }
}
fn opaque(backing:ImmutableBacking, length:u64, crc32:u32) -> OpaqueResource {
    OpaqueResource {id:id(10),kind:"future.attachment/1".into(),data:json!({}),encoding:"future.codec/1".into(),
        extra_fields:Map::new(),backing,offset:0,length,crc32}
}

#[test]
fn pending_and_cancellation_stop_reads_without_poisoning_the_backing() {
    let cancel=Arc::new(AtomicBool::new(true));
    let source=Arc::new(DeferredSource {bytes:Arc::from(b"abc".as_slice()),pending:AtomicBool::new(true),cancel:cancel.clone(),polls:AtomicUsize::new(0)});
    let backing=ImmutableBacking::new(source.clone()).unwrap();let resource=opaque(backing.clone(),3,crc32fast::hash(b"abc"));
    assert!(resource.verify(&cancel).unwrap_err().contains("cancelled"));assert_eq!(source.polls.load(Ordering::Relaxed),0);
    cancel.store(false,Ordering::Relaxed);
    assert!(resource.verify(&cancel).unwrap_err().contains("pending"));
    source.pending.store(false,Ordering::Relaxed);
    assert!(resource.verify(&cancel).unwrap_err().contains("cancelled"));
    assert!(matches!(backing.poll(0,3).unwrap(),RangeState::Ready(_)));
    assert!(resource.read_chunk(2,2,&AtomicBool::new(false)).unwrap_err().contains("exceeds range"));
    assert!(resource.read_chunk(u64::MAX,2,&AtomicBool::new(false)).is_err());
    assert!(resource.verify(&AtomicBool::new(false)).is_ok());
}

struct CountingMemory { bytes:ChunkedBytes, reads:Mutex<Vec<(u64,usize)>> }
impl ByteSource for CountingMemory {
    fn byte_len(&self) -> u64 { self.bytes.byte_len() }
    fn poll(&self, offset:u64, length:usize) -> Result<RangeState,String> {
        self.reads.lock().unwrap().push((offset,length));self.bytes.poll(offset,length)
    }
}

#[test]
fn integrity_walk_is_bounded_and_does_not_decode_or_materialize_large_resources() {
    let first:Arc<[u8]>=vec![0x91;MAX_RANGE_BYTES].into();let last:Arc<[u8]>=vec![0x36;31].into();
    let mut checksum=crc32fast::Hasher::new();checksum.update(&first);checksum.update(&last);
    let source=Arc::new(CountingMemory {bytes:ChunkedBytes::new(vec![first,last]).unwrap(),reads:Mutex::new(Vec::new())});
    let backing=ImmutableBacking::new(source.clone()).unwrap();let resource=opaque(backing,MAX_RANGE_BYTES as u64+31,checksum.finalize());
    resource.verify(&AtomicBool::new(false)).unwrap();
    assert_eq!(*source.reads.lock().unwrap(),vec![(0,MAX_RANGE_BYTES),(MAX_RANGE_BYTES as u64,31)]);
    let range=resource.read_chunk(MAX_RANGE_BYTES as u64-2,4,&AtomicBool::new(false)).unwrap();
    assert_eq!(&*range,&[0x91,0x91,0x36,0x36]);assert!(range.retained_bytes()<=MAX_RANGE_BYTES);
    assert!(resource.read_chunk(0,MAX_RANGE_BYTES+1,&AtomicBool::new(false)).is_err());
}

#[test]
fn empty_resources_observe_pending_and_owner_failure() {
    let source=Arc::new(DeferredSource {bytes:Arc::from([]),pending:AtomicBool::new(true),cancel:Arc::new(AtomicBool::new(false)),polls:AtomicUsize::new(0)});
    let backing=ImmutableBacking::new(source.clone()).unwrap();let resource=opaque(backing.clone(),0,crc32fast::hash(&[]));
    let cancelled=AtomicBool::new(false);
    assert!(resource.verify(&cancelled).unwrap_err().contains("pending"));
    source.pending.store(false,Ordering::Relaxed);resource.verify(&cancelled).unwrap();
    let error=backing.fail("Another resource was corrupt".into());
    assert_eq!(resource.verify(&cancelled).unwrap_err(),error);
}
