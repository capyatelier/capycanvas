use super::*;
use crate::authored::Support;
use super::super::{manifest::{ResourceRecord,ResourceRange}, effect_records::{ResourceWriter as _,ResourceReader as _}};
use std::io::Read;

fn loaded(prepared:&PreparedResources, bytes:Arc<[u8]>) -> (Manifest,ImmutableBacking) {
    let mut resources=BTreeMap::new();
    for entry in &prepared.entries {
        let id=entry.payload.id();
        let offset=decimal_u64(&entry.record["location"]["offset"]).unwrap();
        resources.insert(id,ResourceRecord {value:entry.record.clone(),bytes:entry.bytes,crc32:entry.crc,range:Some(ResourceRange {member:0,offset,length:entry.bytes})});
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
#[test]
fn verified_selection_transfer_keeps_words_and_ids_without_compressing_or_decoding_on_adoption() {
    use super::super::selection_records::{encode_selection,decode_selection};
    let cancelled=AtomicBool::new(false);
    let pixels=Arc::new(crate::SelectionPixels::new([1033,257],[0,0,1033,257],vec![0;1033u32.div_ceil(8) as usize*257]).unwrap());
    let selection=crate::Selection::pixels(pixels.clone());
    assert!(pixels.transfer_chunks().is_none());
    let mut transfer=ResourceInventory::for_transfer();
    let wire=encode_selection(&selection,&mut transfer).unwrap();
    assert_eq!(encode_selection(&selection,&mut transfer).unwrap(),wire);
    assert!(pixels.transfer_chunks().is_none());
    assert_eq!(transfer.transfer_selections.len(),1);
    assert!(transfer.entries.values().all(|entry|matches!(&entry.payload,Payload::Encoded(bytes) if bytes.is_empty())));
    assert!(transfer.prepare(&cancelled).is_err());
    let ids=pixels.transfer_chunk_ids().to_vec();
    let verified=Arc::new(crate::SelectionPixels::from_verified_words(pixels.extent(),pixels.bounds(),false,pixels.transfer_words().clone(),ids.clone(),None).unwrap());
    assert!(Arc::ptr_eq(verified.transfer_words(),pixels.transfer_words()));
    let mut inventory=ResourceInventory::default();
    assert_eq!(encode_selection(&selection,&mut inventory).unwrap(),wire);
    assert_eq!(pixels.package_chunks().unwrap().iter().map(Resource::id).collect::<Vec<_>>(),ids);
    let prepared=inventory.prepare(&cancelled).unwrap();
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.into());
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());reader.require_verified();
    assert!(decode_selection(&wire,&mut reader).is_err());
    for chunk in pixels.package_chunks().unwrap(){reader.bytes.insert(chunk.id(),chunk.clone());}
    assert!(decode_selection(&wire,&mut reader).is_err());
    reader.selections.insert((pixels.extent(),pixels.bounds(),false,ids),verified.clone());
    let restored=decode_selection(&wire,&mut reader).unwrap();
    let crate::SelectionShape::Pixels(restored)=restored.shape else{panic!()};
    assert!(Arc::ptr_eq(&restored,&verified));
    assert!(restored.transfer_chunks().is_none());
    let raw=crate::SelectionPixels::new([8,1],[1,0,8,1],vec![0x11111111]).unwrap();
    let incoming=crate::SelectionPixels::from_verified_words(raw.extent(),raw.bounds(),false,raw.transfer_words().clone(),raw.transfer_chunk_ids().to_vec(),None).unwrap();
    assert!(incoming.package_chunks().is_err());
}

fn tile(value:u8) -> Arc<TileBlob> {
    let descriptor=crate::color::DocumentColor::default().paint_descriptor();
    Arc::new(TileBlob::encode(descriptor,&vec![value;descriptor.byte_len([TILE_SIZE;2]).unwrap()]).unwrap())
}
fn tile_inventory() -> (ResourceInventory,[Value;2]) {
    let mut inventory=ResourceInventory::default();
    let references=[inventory.tile(tile(42)).unwrap(),inventory.tile(tile(42)).unwrap()];
    assert_ne!(references[0],references[1]);
    (inventory,references)
}
#[test]
fn equal_tile_payloads_keep_ids_but_read_validate_and_charge_one_physical_range() {
    use super::super::ByteSource;
    use std::sync::atomic::AtomicUsize;
    struct Counted {bytes:Arc<[u8]>,reads:Arc<AtomicUsize>}
    impl ByteSource for Counted {
        fn byte_len(&self)->u64 {self.bytes.len() as u64}
        fn poll(&self,offset:u64,len:usize)->Result<RangeState,String> {
            self.reads.fetch_add(1,Ordering::Relaxed);self.bytes.poll(offset,len)
        }
    }
    let cancelled=AtomicBool::new(false);
    let (inventory,references)=tile_inventory();
    let prepared=inventory.prepare(&cancelled).unwrap();
    assert_eq!(prepared.entries.len(),2);
    assert_eq!(prepared.entries[0].record["location"],prepared.entries[1].record["location"]);
    assert_eq!(prepared.length,prepared.entries[0].bytes);
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    assert_eq!(packed.len() as u64,prepared.length);assert_eq!(crc32fast::hash(&packed),prepared.crc);
    let (manifest,_)=loaded(&prepared,packed.clone().into());
    let reads=Arc::new(AtomicUsize::new(0));
    let backing=ImmutableBacking::new(Arc::new(Counted {bytes:packed.into(),reads:reads.clone()})).unwrap();
    let decoded=crate::color::DocumentColor::default().paint_descriptor().byte_len([TILE_SIZE;2]).unwrap() as u64;
    let mut limits=crate::ProjectLimits::default();limits.raster_bytes=decoded;
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,limits);
    let first=reader.tile(&references[0]).unwrap();let second=reader.tile(&references[1]).unwrap();
    assert_eq!(reads.load(Ordering::Relaxed),1);
    assert_eq!(reader.decoded,decoded,"aliases have one validated sample allocation");
    assert_ne!(first.resource_id(),second.resource_id());assert!(!Arc::ptr_eq(&first,&second));
    assert_eq!(first.owner_identity(),second.owner_identity());
    assert!(Arc::ptr_eq(&first.compressed,&second.compressed));
    assert!(Arc::ptr_eq(&first.compressed().unwrap(),&second.compressed().unwrap()));
    assert_eq!(first.decode().unwrap(),vec![42;decoded as usize]);
    let mut independently_opened=ResourceReader::new(&manifest,&backing,&cancelled,limits);
    let independent=independently_opened.tile(&references[0]).unwrap();
    assert_eq!(first.resource_id(),independent.resource_id());
    assert_ne!(first.owner_identity(),independent.owner_identity());
    let mut saved=ResourceInventory::default();saved.tile(first).unwrap();saved.tile(second).unwrap();
    let again=saved.prepare(&cancelled).unwrap();
    assert_eq!(again.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>(),prepared.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>());
}
#[test]
fn aliased_tiles_share_spill_state_and_release_the_last_owner() {
    use crate::raster_storage::{RetainedTiles,TileChunk,prepare_external_spill};
    struct Chunk {bytes:Arc<[u8]>,ready:AtomicBool}
    impl TileChunk for Chunk {
        fn len(&self)->usize {self.bytes.len()}
        fn poll(&self)->Result<Option<Arc<[u8]>>,String> {Ok(self.ready.load(Ordering::Relaxed).then(||self.bytes.clone()))}
        fn resident_bytes(&self)->usize {if self.ready.load(Ordering::Relaxed){self.bytes.len()}else{0}}
        fn evict(&self) {self.ready.store(false,Ordering::Relaxed);}
    }
    let cancelled=AtomicBool::new(false);let (inventory,references)=tile_inventory();
    let prepared=inventory.prepare(&cancelled).unwrap();
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.into());
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    let first=reader.tile(&references[0]).unwrap();let second=reader.tile(&references[1]).unwrap();drop(reader);
    let retained=RetainedTiles {sources:vec![first.clone(),second.clone()],..Default::default()};
    assert_eq!(retained.resident_bytes(),first.compressed_len());
    let spill=prepare_external_spill(&retained).unwrap().unwrap();
    assert_eq!(spill.bytes.len(),first.compressed_len(),"one shared owner writes one spill payload");
    let chunk=Arc::new(Chunk {bytes:spill.bytes.clone().into(),ready:AtomicBool::new(false)});
    let weak_chunk=Arc::downgrade(&chunk);let weak_storage=Arc::downgrade(&first.compressed);
    spill.commit(chunk.clone()).unwrap();
    assert_eq!(retained.resident_bytes(),0);
    assert!(!first.compressed_ready().unwrap());assert!(!second.compressed_ready().unwrap());
    assert!(second.compressed().is_err());
    chunk.ready.store(true,Ordering::Relaxed);
    assert_eq!(first.decode().unwrap(),second.decode().unwrap());
    assert_eq!(retained.resident_bytes(),first.compressed_len());
    assert!(prepare_external_spill(&retained).unwrap().is_none());
    assert!(!first.compressed_ready().unwrap());assert!(!second.compressed_ready().unwrap());
    chunk.ready.store(true,Ordering::Relaxed);
    drop(retained);drop(chunk);drop(first);
    assert!(weak_storage.upgrade().is_some());assert!(weak_chunk.upgrade().is_some());
    assert_eq!(second.decode().unwrap(),vec![42;second.descriptor.byte_len([TILE_SIZE;2]).unwrap()]);
    drop(second);assert!(weak_storage.upgrade().is_none());assert!(weak_chunk.upgrade().is_none());
}
#[test]
fn nonimage_aliases_share_decoded_storage_and_keep_descriptors_and_titles() {
    let cancelled=AtomicBool::new(false);
    let first=Resource::<[u8]>::from(vec![9;4096]);let second=Resource::<[u8]>::from(first.to_vec());
    let first_code=Resource::<str>::from("fn exact_shader() {}\n".repeat(100));let second_code=Resource::<str>::from(first_code.to_string());
    let first_lut=Lut3d::from_samples(2,[[0.;3],[1.;3]],"First".into(),vec![[0.125,0.75,0.5];8].into()).unwrap();
    let second_lut=Lut3d::from_samples(2,[[0.;3],[1.;3]],"Second".into(),vec![[0.125,0.75,0.5];8].into()).unwrap();
    let mut inventory=ResourceInventory::default();
    let metadata=[inventory.bytes("capy.photo-metadata/1",&first,json!({"kind":"exif"})).unwrap(),inventory.bytes("capy.photo-metadata/1",&second,json!({"kind":"exif"})).unwrap()];
    let shaders=[inventory.code(&first_code).unwrap(),inventory.code(&second_code).unwrap()];
    let tables=[inventory.lut(&first_lut).unwrap(),inventory.lut(&second_lut).unwrap()];
    let distinct=Resource::<[u8]>::from(first.to_vec());
    inventory.bytes("capy.photo-metadata/1",&distinct,json!({"kind":"iptc"})).unwrap();
    let prepared=inventory.prepare(&cancelled).unwrap();
    assert_eq!(prepared.entries.len(),7);assert_eq!(prepared.entries.iter().filter(|entry|entry.physical).count(),4);
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.clone().into());let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    let metadata=metadata.map(|value|reader.bytes(&value,"capy.photo-metadata/1",4096).unwrap());
    let shaders=shaders.map(|value|reader.code(&value).unwrap());let tables=tables.map(|value|reader.lut(&value).unwrap());
    assert_ne!(metadata[0].id(),metadata[1].id());assert!(Arc::ptr_eq(metadata[0].storage(),metadata[1].storage()));
    assert_ne!(shaders[0].id(),shaders[1].id());assert!(Arc::ptr_eq(shaders[0].storage(),shaders[1].storage()));
    assert_ne!(tables[0].resource().unwrap().id(),tables[1].resource().unwrap().id());
    assert!(Arc::ptr_eq(tables[0].storage().unwrap(),tables[1].storage().unwrap()));
    assert_eq!(tables[0].title(),"First");assert_eq!(tables[1].title(),"Second");
    assert_eq!(reader.decoded,first.len() as u64+first_code.len() as u64+first_lut.bytes() as u64);
    let mut saved=ResourceInventory::default();
    for bytes in &metadata {saved.bytes("capy.photo-metadata/1",bytes,json!({"kind":"exif"})).unwrap();}
    for code in &shaders {saved.code(code).unwrap();}for lut in &tables {saved.lut(lut).unwrap();}
    saved.bytes("capy.photo-metadata/1",&distinct,json!({"kind":"iptc"})).unwrap();
    let saved=saved.prepare(&cancelled).unwrap();let mut restored=Vec::new();saved.reader(&cancelled).read_to_end(&mut restored).unwrap();
    assert_eq!(restored,packed);assert_eq!(saved.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>(),prepared.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>());
}
#[test]
fn alias_corruption_conflicting_descriptors_and_cancelled_reads_are_rejected() {
    let cancelled=AtomicBool::new(false);let (inventory,references)=tile_inventory();let prepared=inventory.prepare(&cancelled).unwrap();
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (mut manifest,backing)=loaded(&prepared,packed.clone().into());
    manifest.resources.get_mut(&reference_id(&references[1]).unwrap()).unwrap().value["data"]["alpha"]=json!("premultiplied_linear");
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());reader.tile(&references[0]).unwrap();
    assert!(reader.tile(&references[1]).unwrap_err().to_string().contains("Conflicting aliased"));
    let (manifest,backing)=loaded(&prepared,packed.clone().into());let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    reader.tile(&references[0]).unwrap();cancelled.store(true,Ordering::Relaxed);
    assert!(reader.tile(&references[1]).is_err());assert!(prepared.reader(&cancelled).read(&mut [0;1]).is_err());
    cancelled.store(false,Ordering::Relaxed);packed[0]^=1;
    let (manifest,backing)=loaded(&prepared,packed.into());let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    assert!(reader.tile(&references[0]).is_err());assert!(reader.tile(&references[1]).is_err());assert!(backing.poll(0,1).is_err());
}

#[test]
fn aliased_selection_chunks_retain_ids_and_share_validated_words() {
    use super::super::selection_records::{encode_selection,decode_selection};
    let cancelled=AtomicBool::new(false);
    let selections=[0,1].map(|_|crate::Selection::pixels(Arc::new(crate::SelectionPixels::new([16,4],[0,0,16,4],vec![0x11111111;8]).unwrap())));
    let mut inventory=ResourceInventory::default();let records=selections.map(|selection|encode_selection(&selection,&mut inventory).unwrap());
    let prepared=inventory.prepare(&cancelled).unwrap();assert_eq!(prepared.entries.len(),2);assert_eq!(prepared.entries.iter().filter(|entry|entry.physical).count(),1);
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.into());
    let mut limits=crate::ProjectLimits::default();limits.raster_bytes=32;
    let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,limits);
    let selections=records.each_ref().map(|record|decode_selection(record,&mut reader).unwrap());
    let pixels=selections.each_ref().map(|selection|match &selection.shape {crate::SelectionShape::Pixels(pixels)=>pixels,_=>panic!()});
    assert_ne!(pixels[0].transfer_chunk_ids(),pixels[1].transfer_chunk_ids());
    assert!(Arc::ptr_eq(pixels[0].transfer_words(),pixels[1].transfer_words()));
    assert_eq!(reader.decoded,32);
    let mut saved=ResourceInventory::default();
    for (selection,record) in selections.iter().zip(records) {assert_eq!(encode_selection(selection,&mut saved).unwrap(),record);}
    assert_eq!(saved.prepare(&cancelled).unwrap().length,prepared.length);
}
#[test]
fn aliased_archive_sources_remain_independently_editable_and_resave_exactly() {
    use crate::{authored::*, raster::{RasterRevision,RasterData,RasterTile,RasterPlane,TileKey},Document,Editor,Edit};
    use super::super::codec::{PreparedPackage,OpenOutcome,open};
    fn revision(tile:Arc<TileBlob>)->RasterRevision {
        RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0,0]},RasterTile::backed_shared(tile))].into(),watercolor:None})
    }
    fn archive(artwork:&Artwork)->Vec<u8> {
        let capture=artwork.capture(CaptureCheckpoint {owner:1,document:artwork.id,session_generation:0,artwork_generation:0,working_generation:0,edit_checkpoint:0}).unwrap();
        let cancel=AtomicBool::new(false);let prepared=PreparedPackage::prepare(&capture,None,&cancel).unwrap();
        let mut bytes=Vec::new();prepared.write(&mut bytes,&cancel).unwrap();bytes
    }
    let mut artwork=Artwork::new([TILE_SIZE;2]).unwrap();
    let stack=artwork.compositions.get(artwork.root).unwrap().result;
    let tiles=[tile(42),tile(42)];let tile_ids=tiles.each_ref().map(|tile|tile.resource_id());
    let sources=tiles.map(|tile| {
        let source=artwork.paint.insert(PortableId::random(),PaintSource {domain:[TILE_SIZE;2],raster:revision(tile),original:None,operations:Default::default()}).unwrap();
        let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),"Independent paint")).unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);artwork.paint.id(source).unwrap()
    });
    assert_ne!(sources[0],sources[1]);assert_ne!(tile_ids[0],tile_ids[1]);
    let bytes=archive(&artwork);let cancelled=AtomicBool::new(false);
    let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes.clone()))).unwrap();
    let OpenOutcome::Candidate {artwork:loaded,..}=open(backing,Default::default(),&cancelled).unwrap() else {panic!("expected editable independent sources")};
    assert_eq!(archive(&loaded),bytes);
    let handles=sources.map(|id|loaded.paint.resolve(id).unwrap());
    let blobs=handles.map(|handle|loaded.paint.get(handle).unwrap().raster.wait_data().unwrap().tiles.values().next().unwrap().wait_backing().unwrap());
    assert_eq!(blobs.each_ref().map(|tile|tile.resource_id()),tile_ids);
    assert_eq!(blobs[0].owner_identity(),blobs[1].owner_identity());
    let mut editor=Editor::new(Document::from_artwork(loaded).unwrap());
    editor.perform(Edit::SetRaster {target:SourceTarget::Paint(handles[0]),revision:revision(tile(77))}).unwrap();
    let samples=|editor:&Editor,handle|editor.document().artwork.paint.get(handle).unwrap().raster.wait_data().unwrap().tiles.values().next().unwrap().wait_backing().unwrap().decode().unwrap();
    assert!(samples(&editor,handles[0]).iter().all(|sample|*sample==77));
    assert!(samples(&editor,handles[1]).iter().all(|sample|*sample==42));
    editor.undo().unwrap();assert_eq!(archive(&editor.document().artwork),bytes);
    editor.redo().unwrap();assert!(samples(&editor,handles[0]).iter().all(|sample|*sample==77));
    assert!(samples(&editor,handles[1]).iter().all(|sample|*sample==42));
}

#[test]
fn unique_crc_buckets_skip_sha_and_collisions_keep_distinct_exact_payloads() {
    let cancelled=AtomicBool::new(false);
    let first=Resource::<[u8]>::from(vec![78,56,72,77,180,220,248,124]);
    let second=Resource::<[u8]>::from(vec![65,11,137,195,3,223,87,218]);
    assert_ne!(*first,*second);assert_eq!(crc32fast::hash(&first),crc32fast::hash(&second));
    let mut single=ResourceInventory::default();single.bytes("capy.icc/1",&first,json!({})).unwrap();single.prepare(&cancelled).unwrap();
    let mut reads=0;
    first.encoded_digest(&cancelled,||{reads+=1;Ok(first.encoded_if_ready().unwrap().bytes.clone())}).unwrap();
    assert_eq!(reads,1,"a unique CRC/length bucket must not compute SHA");
    let duplicate=Resource::<[u8]>::from(first.to_vec());
    single.bytes("capy.icc/1",&second,json!({})).unwrap();single.bytes("capy.icc/1",&duplicate,json!({})).unwrap();
    let prepared=single.prepare(&cancelled).unwrap();assert_eq!(prepared.entries.len(),3);assert_eq!(prepared.length,16);
    assert_eq!(prepared.entries.iter().filter(|entry|entry.physical).count(),2);
    let digest=second.encoded_digest(&cancelled,||panic!("collision SHA must be retained")).unwrap();
    assert_ne!(digest,first.encoded_digest(&cancelled,||panic!("retained SHA")).unwrap());
    let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.into());let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    for expected in [&first,&second,&duplicate] {
        let actual=reader.bytes(&reference(expected.id()),"capy.icc/1",8).unwrap();
        assert_eq!(&*actual,&**expected);assert_eq!(actual.id(),expected.id());
        assert_eq!(actual.encoded_integrity(&cancelled,||panic!("opening must retain mandatory CRC")).unwrap().crc32,crc32fast::hash(expected));
    }
}
#[test]
fn cached_alias_preparation_does_not_read_cold_tiles_but_writing_still_checks_backing() {
    use crate::raster_storage::{RetainedTiles,TileChunk,prepare_external_spill};
    use std::sync::atomic::AtomicUsize;
    struct Chunk {bytes:Arc<[u8]>,ready:AtomicBool,reads:AtomicUsize}
    impl TileChunk for Chunk {
        fn len(&self)->usize {self.bytes.len()}
        fn poll(&self)->Result<Option<Arc<[u8]>>,String> {
            self.reads.fetch_add(1,Ordering::Relaxed);Ok(self.ready.load(Ordering::Relaxed).then(||self.bytes.clone()))
        }
        fn resident_bytes(&self)->usize {if self.ready.load(Ordering::Relaxed){self.bytes.len()}else{0}}
        fn evict(&self) {self.ready.store(false,Ordering::Relaxed);}
    }
    let cancelled=AtomicBool::new(false);let first=tile(42);let second=Arc::new(first.alias(PortableId::random()));
    let mut inventory=ResourceInventory::default();inventory.tile(first.clone()).unwrap();inventory.tile(second.clone()).unwrap();
    let prepared=inventory.prepare(&cancelled).unwrap();let expected_crc=prepared.crc;
    let retained=RetainedTiles {sources:vec![first,second],..Default::default()};let spill=prepare_external_spill(&retained).unwrap().unwrap();
    let chunk=Arc::new(Chunk {bytes:spill.bytes.clone().into(),ready:AtomicBool::new(false),reads:AtomicUsize::new(0)});spill.commit(chunk.clone()).unwrap();
    let again=inventory.prepare(&cancelled).unwrap();assert_eq!(again.length,prepared.length);assert_eq!(again.crc,expected_crc);
    assert_eq!(chunk.reads.load(Ordering::Relaxed),0,"cached CRC and SHA plus shared owner equality need no backing read");
    assert!(again.reader(&cancelled).read(&mut [0;1]).is_err());assert_eq!(chunk.reads.load(Ordering::Relaxed),1);
    chunk.ready.store(true,Ordering::Relaxed);let mut packed=Vec::new();again.reader(&cancelled).read_to_end(&mut packed).unwrap();
    assert_eq!(crc32fast::hash(&packed),expected_crc);assert_eq!(packed.as_slice(),&*chunk.bytes);
}

#[test]
fn reopened_catalog_shader_ids_reconcile_reinsertion_and_other_shared_definitions() {
    use crate::authored::Definition;
    use super::super::effect_records::{encode_definition,decode_definition};
    let cancelled=AtomicBool::new(false);
    let catalog=crate::bundled_effect_catalog();
    let first=Definition {program:crate::effect_catalog::custom_program("color_lookup")};
    let shared=Definition {program:crate::effect_catalog::custom_program("hue_saturation")};
    let mut inventory=ResourceInventory::default();let record=encode_definition(&first,&mut inventory).unwrap();
    let shared_record=encode_definition(&shared,&mut inventory).unwrap();
    let prepared=inventory.prepare(&cancelled).unwrap();let mut packed=Vec::new();prepared.reader(&cancelled).read_to_end(&mut packed).unwrap();
    let (manifest,backing)=loaded(&prepared,packed.clone().into());let mut reader=ResourceReader::new(&manifest,&backing,&cancelled,Default::default());
    let reopened=decode_definition(&record,&mut reader).unwrap();
    let reopened_shared=decode_definition(&shared_record,&mut reader).unwrap();
    for (original,loaded) in first.program.wgsl.sources().unwrap().iter().zip(reopened.program.wgsl.sources().unwrap()) {
        assert_eq!(original.id(),loaded.id());assert!(!original.same_owner(loaded));assert_eq!(original.as_ref(),loaded.as_ref());
    }
    let mut reused=ResourceInventory::default();
    assert_eq!(encode_definition(&reopened,&mut reused).unwrap(),record);
    assert_eq!(encode_definition(&reopened_shared,&mut reused).unwrap(),shared_record);
    assert_eq!(encode_definition(&first,&mut reused).unwrap(),record);
    let saved=reused.prepare(&cancelled).unwrap();let mut saved_bytes=Vec::new();saved.reader(&cancelled).read_to_end(&mut saved_bytes).unwrap();
    assert_eq!(saved_bytes,packed);
    assert_eq!(saved.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>(),prepared.entries.iter().map(|entry|&entry.record).collect::<Vec<_>>());
    let ids:std::collections::BTreeSet<_>=inventory.entries.keys().copied().collect();
    let other=catalog.filters().iter().filter(|filter|filter.id()!="color_lookup"&&filter.id()!="hue_saturation").find(|filter| {
        filter.program().wgsl.sources().unwrap().iter().any(|source|ids.contains(&source.id()))
    }).expect("another bundled definition shares shader helpers");
    let second=Definition {program:crate::effect_catalog::custom_program(other.id())};
    encode_definition(&second,&mut reused).unwrap();
    let mut transfer=ResourceInventory::for_transfer();
    encode_definition(&reopened,&mut transfer).unwrap();encode_definition(&reopened_shared,&mut transfer).unwrap();
    encode_definition(&first,&mut transfer).unwrap();encode_definition(&second,&mut transfer).unwrap();
    for source in reopened.program.wgsl.sources().unwrap().iter().chain(reopened_shared.program.wgsl.sources().unwrap()) {
        let Payload::Code(saved)=&reused.entries[&source.id()].payload else {panic!()};
        assert!(saved.same_owner(source),"the reopened encoded owner remains authoritative");
    }
    let source=&first.program.wgsl.sources().unwrap()[0];
    let changed=Resource::<str>::with_id(source.id(),format!("{}\n// different module",source.as_ref()).into());
    let error=reused.code(&changed).unwrap_err();
    assert!(error.contains(&source.id().to_string()));assert!(error.contains("capy.wgsl/1"));assert!(error.contains("shader bytes differ"));
    assert!(transfer.code(&changed).unwrap_err().contains("shader bytes differ"));
}
#[test]
fn shader_reconciliation_preserves_stored_encoding_and_rejects_conflicting_descriptors_or_owners() {
    let id=PortableId::random();let text:Arc<str>="fn exact_shader() {}".into();
    let unencoded=Resource::<str>::with_id(id,text.clone());
    let stored:Arc<[u8]>=text.as_bytes().into();
    let encoded=Resource::<str>::with_id_and_encoded(id,text.clone(),EncodedBytes {encoding:ResourceEncoding::Raw,bytes:stored.clone()});
    for loaded_first in [false,true] {
        let mut inventory=ResourceInventory::default();
        if loaded_first {inventory.code(&encoded).unwrap();inventory.code(&unencoded).unwrap();}
        else {inventory.code(&unencoded).unwrap();inventory.code(&encoded).unwrap();}
        let Payload::Code(retained)=&inventory.entries[&id].payload else {panic!()};
        assert!(retained.same_owner(&encoded));assert!(Arc::ptr_eq(&retained.encoded_if_ready().unwrap().bytes,&stored));
        let different_encoding=Resource::<str>::with_id_and_encoded(id,text.clone(),EncodedBytes {encoding:ResourceEncoding::Lz4,bytes:lz4_flex::block::compress(text.as_bytes()).into()});
        assert!(inventory.code(&different_encoding).unwrap_err().contains("encoded shader bytes differ"));
        assert!(inventory.insert(ResourceEntry {kind:"capy.wgsl/1",data:json!({"future":true}),raw_encoding:"utf8",payload:Payload::Code(encoded.clone())}).unwrap_err().contains("descriptor differs"));
    }
    assert!(unencoded.encoded_if_ready().is_none(),"inventory reconciliation never encodes shader text");
    let first=Resource::<[u8]>::with_id(PortableId::random(),Arc::from([7,13]));
    let unrelated=Resource::<[u8]>::with_id(first.id(),first.storage().clone());
    let mut inventory=ResourceInventory::default();inventory.bytes("capy.icc/1",&first,json!({})).unwrap();
    assert!(inventory.bytes("capy.icc/1",&unrelated,json!({})).unwrap_err().contains("owner differs"));
    let first=tile(42);let unrelated=Arc::new(TileBlob::from_package(first.resource_id(),first.descriptor,first.compressed().unwrap()).unwrap());
    inventory.tile(first).unwrap();assert!(inventory.tile(unrelated).unwrap_err().contains("owner differs"));
}
