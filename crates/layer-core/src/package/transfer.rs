use super::{artwork_records, manifest::{Manifest, ManifestLimits, ManifestRead}, resources::{self, Payload, ResourceInventory, ResourceReader}, ImmutableBacking, MAX_RANGE_BYTES};
use crate::{authored::*, ProjectLimits, SelectionPixels, Lut3d};
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, mem::MaybeUninit, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferLayout {
    compositions: Vec<PortableId>, stacks: Vec<PortableId>, occurrences: Vec<PortableId>,
    paint: Vec<PortableId>, coverage: Vec<PortableId>, effects: Vec<PortableId>,
    objects:Vec<PortableId>, #[serde(default)] vectors:Vec<PortableId>, selections: Vec<PortableId>, guides: Vec<PortableId>, outputs: Vec<PortableId>,
}
impl TransferLayout {
    pub(crate) fn capture(art: &Artwork)->Self {
        fn slots<T>(store:&Store<T>)->Vec<PortableId>{(0..store.capacity()).map(|i|store.id(Handle::from_index(i as u32)).unwrap()).collect()}
        Self {compositions:slots(&art.compositions),stacks:slots(&art.stacks),occurrences:slots(&art.occurrences),paint:slots(&art.paint),coverage:slots(&art.coverage),effects:slots(&art.effects),objects:slots(&art.objects),vectors:slots(&art.vectors),selections:slots(&art.selections),guides:slots(&art.guides),outputs:slots(&art.outputs)}
    }
    pub(crate) fn install(&self,art:&mut Artwork)->super::values::DecodeResult<()> {
        fn slots<T>(store:&mut Store<T>,ids:&[PortableId])->super::values::DecodeResult<()>{for id in ids {store.reserve(*id)?;}Ok(())}
        slots(&mut art.compositions,&self.compositions)?;slots(&mut art.stacks,&self.stacks)?;slots(&mut art.occurrences,&self.occurrences)?;
        slots(&mut art.paint,&self.paint)?;slots(&mut art.coverage,&self.coverage)?;slots(&mut art.effects,&self.effects)?;
        slots(&mut art.objects,&self.objects)?;slots(&mut art.vectors,&self.vectors)?;slots(&mut art.selections,&self.selections)?;slots(&mut art.guides,&self.guides)?;slots(&mut art.outputs,&self.outputs)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EncodedPayload {encoding:ResourceEncoding,payload:usize}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
enum VerifiedResource {
    Bytes {payload:usize,encoded:Option<EncodedPayload>},
    Code {payload:usize,encoded:Option<EncodedPayload>},
    Tile {payload:usize,identity:Option<crate::raster::TileContentIdentity>},
    TileRange {backing:usize,offset:u64,original:EncodedIntegrity,filtered:bool,canonical:Option<EncodedIntegrity>,identity:Option<crate::raster::TileContentIdentity>},
    Lut {payload:usize,encoded:Option<EncodedPayload>,digest:[u8;32],spaces:u8},
    Coverage {payload:Option<usize>},
    Vector {payload:usize},
    Opaque {payload:usize},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SelectionTransfer {extent:[u32;2],bounds:[u32;4],bytes:bool,chunks:Vec<PortableId>,payload:usize}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackingReference {pub key:String,pub generation:PortableId,pub length:u64}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferDescriptor {
    pub manifest: Value,
    pub layout: TransferLayout,
    pub checkpoint: CaptureCheckpoint,
    working_selection: Option<Value>,
    lengths: Vec<u64>,
    resources: BTreeMap<PortableId,VerifiedResource>,
    selections: Vec<SelectionTransfer>,
    retained_resources: BTreeMap<PortableId,usize>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    backings:Vec<BackingReference>,
}
impl TransferDescriptor {pub fn backings(&self)->&[BackingReference] {&self.backings}}
#[derive(Clone)]
enum TransferPayload {Bytes(Arc<[u8]>),Tile(Arc<crate::raster::TileBlob>),Code(Resource<str>),Words(Arc<[u32]>),Vector(Resource<vector::VectorChunk>,Arc<[u8]>),Opaque(Arc<OpaqueResource>)}
impl TransferPayload {
    fn len(&self)->u64 {match self {Self::Bytes(v)=>v.len() as u64,Self::Tile(v)=>v.compressed_len() as u64,Self::Code(v)=>v.len() as u64,Self::Words(v)=>v.len() as u64*4,Self::Vector(_,v)=>v.len() as u64,Self::Opaque(v)=>v.length}}
    fn memory_chunk(&self,start:usize,length:usize)->Vec<u8> {
        match self {
            Self::Bytes(bytes)|Self::Vector(_,bytes)=>bytes[start..start+length].to_vec(),
            Self::Code(code)=>code.as_bytes()[start..start+length].to_vec(),
            Self::Words(words)=>(start..start+length).map(|index|words[index/4].to_le_bytes()[index%4]).collect(),
            Self::Tile(_)|Self::Opaque(_)=>unreachable!(),
        }
    }
    #[cfg(not(target_arch="wasm32"))]
    fn chunk(&self,offset:u64,length:usize)->Result<Vec<u8>,String>{
        if length>MAX_RANGE_BYTES||offset.checked_add(length as u64).is_none_or(|end|end>self.len()){return Err("Transfer chunk exceeds payload".into());}
        let start=usize::try_from(offset).map_err(|_|"Transfer offset overflow")?;
        Ok(match self {
            Self::Tile(tile)=>tile.compressed.read_on_worker()?[start..start+length].to_vec(),
            Self::Opaque(resource)=>resource.read_chunk(offset,length,&AtomicBool::new(false))?.to_vec(),
            _=>self.memory_chunk(start,length),
        })
    }

}
pub struct PreparedTransfer {memory:std::sync::OnceLock<crate::DocumentMemory>,admission:Option<crate::MemoryReservation>,descriptor:TransferDescriptor,payloads:Vec<TransferPayload>,backings:Vec<ImmutableBacking>,pub(crate) trusted_programs:super::TrustedPrograms}
fn active(cancel:&AtomicBool)->Result<(),String>{if cancel.load(Ordering::Relaxed){Err("Transfer cancelled".into())}else{Ok(())}}
fn manifest(descriptor:&TransferDescriptor,limits:ProjectLimits)->Result<Manifest,String>{
    if crate::json_len(descriptor)>limits.metadata_bytes as usize {return Err("Transfer metadata exceeds admission".into());}
    match Manifest::transfer(&descriptor.manifest,ManifestLimits {metadata_bytes:limits.metadata_bytes.min(usize::MAX as u64) as usize,..Default::default()})? {
        ManifestRead::Known(manifest) if matches!(manifest.support,Support::Editable)=>Ok(manifest),
        _=>Err("Unsupported artwork transfer".into()),
    }
}
impl PreparedTransfer {
    pub async fn capture_async(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool,budget:&crate::MemoryBudget)->Result<Self,String> {
        Self::capture_using(capture,selection,cancel,budget,false).await
    }
    pub async fn capture_referenced(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool,budget:&crate::MemoryBudget)->Result<Self,String> {
        Self::capture_using(capture,selection,cancel,budget,true).await
    }
    async fn capture_using(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool,budget:&crate::MemoryBudget,references:bool)->Result<Self,String> {
        let inventory=Self::inventory(capture,cancel)?;
        let mut ranges=BTreeMap::new();
        if references {for (id,entry) in &inventory.1.entries {
            if let Payload::Tile(tile)=&entry.payload && let Some(range)=tile.compressed.transfer_range()? {ranges.insert(*id,range);}
        }}
        let mut bytes=0u64;let mut owners=BTreeSet::new();
        for entry in inventory.1.entries.values() {
            if let Payload::Tile(tile)=&entry.payload && !ranges.contains_key(&tile.resource_id()) && tile.compressed.package_origin() && tile.compressed.needs_store()? && owners.insert(Arc::as_ptr(&tile.compressed) as usize) {
                let length=if tile.compressed.needs_preparation() {crate::raster::TileBlob::max_compressed_len(tile.descriptor).ok_or("Invalid package tile descriptor")?}else {tile.compressed_len()};
                bytes=bytes.checked_add(length as u64).ok_or("Transfer length overflow")?;
            }
        }
        let admission=if bytes==0 {None}else {Some(budget.admit_import(bytes).ok_or(crate::MEMORY_BUDGET_EXCEEDED)?)};
        if let Some(admission)=&admission {admission.covering().await;}
        let mut tiles=inventory.1.entries.values().filter_map(|entry|match &entry.payload {
            Payload::Tile(tile) if tile.compressed.needs_preparation() && !ranges.contains_key(&tile.resource_id())=>Some(tile),_=>None,
        });
        async fn prepare(tile:&crate::raster::TileBlob)->Result<(),String> {
            tile.read_compressed().await.map(|_|())
        }
        let mut window=std::collections::VecDeque::new();
        std::future::poll_fn(|cx|loop {
            active(cancel)?;
            while window.len()<crate::tile_io::QUEUED_READS {
                let Some(tile)=tiles.next() else {break;};window.push_back(Some(Box::pin(prepare(tile))));
            }
            if window.is_empty() {return std::task::Poll::Ready(Ok::<_,String>(()));}
            for slot in &mut window {
                if let Some(read)=slot && let std::task::Poll::Ready(result)=read.as_mut().poll(cx) {result?;*slot=None;}
            }
            if window.front().unwrap().is_some() {return std::task::Poll::Pending;}
            while window.front().is_some_and(Option::is_none) {window.pop_front();}
        }).await?;
        drop(window);drop(tiles);
        let mut transfer=Self::from_inventory(capture,selection,cancel,inventory,ranges)?;
        transfer.admission=admission;Ok(transfer)
    }
    pub fn capture(capture:&ArtworkCapture,cancel:&AtomicBool)->Result<Self,String>{
        Self::capture_with_selection(capture,&None,cancel)
    }
    pub fn capture_with_selection(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool)->Result<Self,String>{
        Self::from_inventory(capture,selection,cancel,Self::inventory(capture,cancel)?,BTreeMap::new())
    }
    fn inventory(capture:&ArtworkCapture,cancel:&AtomicBool)->Result<(Vec<Value>,ResourceInventory),String> {
        active(cancel)?;let art=&capture.artwork;
        if capture.checkpoint.document!=art.id{return Err("Capture belongs to another artwork".into());}
        if art.paint.iter().any(|(_,_,s)|!s.operations.is_empty())||art.coverage.iter().any(|(_,_,s)|!s.operations.is_empty()){return Err("Wait for the current edit before transferring".into());}
        let inventory=artwork_records::encode_with_inventory(art,cancel,ResourceInventory::for_transfer())?;
        Ok(inventory)
    }
    fn from_inventory(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool,(mut objects,mut inventory):(Vec<Value>,ResourceInventory),mut ranges:BTreeMap<PortableId,super::lease::TransferRange>)->Result<Self,String> {
        let art=&capture.artwork;
        let working_selection=selection.as_ref().map(|selection|super::selection_records::encode_selection(selection,&mut inventory)).transpose()?;
        let live=objects.iter().map(|r|r["id"].as_str().ok_or("Missing object ID")?.parse().map_err(str::to_string)).collect::<Result<BTreeSet<_>,String>>()?;
        let (extensions,retained)=art.extensions.edited_retained(&live)?;objects.extend(extensions);
        let opaque=inventory.with_opaque(retained)?;
        let mut roots=crate::RootInventory::default();roots.artwork(art);
        let trusted_programs=super::TrustedPrograms::from_roots(&roots)?;
        let luts:BTreeMap<_,_>=roots.resources.into_iter().filter_map(|lut|lut.resource().map(|r|(r.id(),lut.clone()))).collect();
        let mut backings:Vec<ImmutableBacking>=Vec::new();let mut backing_refs:Vec<BackingReference>=Vec::new();
        let mut payloads=Vec::new();let mut verification=BTreeMap::new();let mut records=Vec::new();
        fn push(payloads:&mut Vec<TransferPayload>,payload:TransferPayload)->usize{let id=payloads.len();payloads.push(payload);id}
        fn encoded(payloads:&mut Vec<TransferPayload>,bytes:Option<&EncodedBytes>)->Option<EncodedPayload>{bytes.map(|v|EncodedPayload{encoding:v.encoding,payload:push(payloads,TransferPayload::Bytes(v.bytes.clone()))})}
        for (id,entry) in &inventory.entries {
            active(cancel)?;
            if let Payload::Tile(tile)=&entry.payload && tile.compressed.needs_preparation() && !ranges.contains_key(id) {return Err("Package normalization must finish before transfer capture".into());}
            let state=match &entry.payload {
                Payload::Tile(tile)=>match ranges.remove(id) {
                    Some(super::lease::TransferRange {backing:source,offset,original,filtered,canonical})=> {
                        let reference=source.transfer_reference().ok_or("Missing transfer backing reference")?;
                        let backing=if let Some(index)=backing_refs.iter().position(|r|r.key==reference.key && r.generation==reference.generation) {
                            if reference.length>backing_refs[index].length {backing_refs[index]=reference;backings[index]=source;}index
                        }else {let index=backings.len();backing_refs.push(reference);backings.push(source);index};
                        VerifiedResource::TileRange {backing,offset,original,filtered,canonical,identity:tile.content_identity()}
                    },
                    None=>VerifiedResource::Tile{payload:push(&mut payloads,TransferPayload::Tile(tile.clone())),identity:tile.content_identity()},
                },
                Payload::Bytes(bytes) if entry.kind=="capy.lut3d/1"=>{let lut=luts.get(id).ok_or("Missing verified lookup owner")?;
                    VerifiedResource::Lut {payload:push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())),encoded:encoded(&mut payloads,bytes.encoded_if_ready()),digest:lut.digest(),spaces:lut.admitted_spaces()}},
                Payload::Bytes(bytes)=>VerifiedResource::Bytes{payload:push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())),encoded:encoded(&mut payloads,bytes.encoded_if_ready())},
                Payload::Code(code)=>VerifiedResource::Code{payload:push(&mut payloads,TransferPayload::Code(code.clone())),encoded:encoded(&mut payloads,code.encoded_if_ready())},
                Payload::Encoded(bytes)=>VerifiedResource::Coverage{payload:(!bytes.is_empty()).then(||push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())))},
                Payload::Vector(chunk)=>VerifiedResource::Vector{payload:push(&mut payloads,TransferPayload::Vector(chunk.clone(),super::vector_records::encode_chunk(chunk).into()))},
                Payload::Opaque(resource)=>VerifiedResource::Opaque{payload:push(&mut payloads,TransferPayload::Opaque(resource.clone()))},
            };
            let length=match &state {
                VerifiedResource::TileRange {original,canonical,..}=>canonical.unwrap_or(*original).bytes,
                VerifiedResource::Coverage {payload:None}=>0,
                VerifiedResource::Coverage {payload:Some(payload)}|VerifiedResource::Bytes{payload,..}|VerifiedResource::Code{payload,..}|VerifiedResource::Tile{payload,..}|VerifiedResource::Lut{payload,..}|VerifiedResource::Vector{payload}|VerifiedResource::Opaque{payload}=>payloads[*payload].len(),
            };
            records.push(json!({"id":id,"type":entry.kind,"data":entry.data,"encoding":entry.raw_encoding,"location":{"member":format!("data/{id}")},"bytes":length.to_string(),"crc32":"00000000"}));
            verification.insert(*id,state);
        }
        let mut retained_resources=BTreeMap::new();
        for resource in opaque {
            if inventory.validate_opaque_alias(&resource)? {
                let record=records.iter_mut().find(|record|record["id"]==json!(resource.id)).unwrap();
                *record=resource.record(json!({"member":format!("data/{}",resource.id)}));
                let shared=match &verification[&resource.id] {
                    VerifiedResource::Bytes{payload,encoded}|VerifiedResource::Code{payload,encoded}|VerifiedResource::Lut{payload,encoded,digest:_,spaces:_}=>{
                        match resource.encoding.as_ref() {"capy.lz4-bytes/1"=>encoded.as_ref().filter(|e|e.encoding==ResourceEncoding::Lz4).map(|e|e.payload),"capy.deflate-bytes/1"=>None,_=>Some(*payload)}
                    },
                    VerifiedResource::Tile{payload,..}|VerifiedResource::Coverage{payload:Some(payload)}=>Some(*payload),
                    _=>None,
                }.filter(|payload|payloads[*payload].len()==resource.length);
                let payload=shared.unwrap_or_else(||push(&mut payloads,TransferPayload::Opaque(resource.clone())));
                retained_resources.insert(resource.id,payload);
                continue;
            }
            records.push(resource.record(json!({"member":format!("data/{}",resource.id)})));
            verification.insert(resource.id,VerifiedResource::Opaque{payload:push(&mut payloads,TransferPayload::Opaque(resource))});
        }
        let selections=inventory.transfer_selections.values().map(|pixels|SelectionTransfer {extent:pixels.extent(),bounds:pixels.bounds(),bytes:pixels.coverage_format()==2,chunks:pixels.transfer_chunk_ids().to_vec(),payload:push(&mut payloads,TransferPayload::Words(pixels.transfer_words().clone()))}).collect();
        let mut value=json!({"format":"capy.canvas","version":1,"document":art.id,"root":resources::reference(art.compositions.id(art.root).ok_or("Missing root")?),"objects":objects,"resources":records,
            "outputs":art.outputs.iter().map(|(_,id,_)|resources::reference(id)).collect::<Vec<_>>(),"default_output":resources::reference(art.outputs.id(art.default_output).ok_or("Missing output")?)});
        let metadata:serde_json::Map<_,_>=["exif","xmp","iptc"].into_iter().zip(art.metadata.blocks()).filter_map(|(kind,bytes)|bytes.as_ref().map(|b|(kind.to_string(),resources::reference(b.id())))).collect();
        if !metadata.is_empty(){value["metadata"]=Value::Object(metadata);}
        let descriptor=TransferDescriptor {manifest:value,layout:TransferLayout::capture(art),checkpoint:capture.checkpoint,working_selection,lengths:payloads.iter().map(TransferPayload::len).collect(),resources:verification,selections,retained_resources,backings:backing_refs};
        manifest(&descriptor,ProjectLimits::default())?;
        Ok(Self {memory:capture.memory.clone().map(std::sync::OnceLock::from).unwrap_or_default(),admission:None,descriptor,payloads,backings,trusted_programs})
    }
    pub fn reserve_copy(&self,budget:&crate::MemoryBudget,overhead:u64)->Result<crate::MemoryReservation,String> {
        let bytes=self.descriptor.lengths.iter().try_fold(overhead,|total,length|total.checked_add(*length).ok_or("Transfer length overflow"))?;
        budget.admit_import(bytes).ok_or_else(||crate::MEMORY_BUDGET_EXCEEDED.into())
    }
    pub fn descriptor(&self)->&TransferDescriptor{&self.descriptor}
    pub fn backings(&self)->&[ImmutableBacking] {&self.backings}
    pub fn payload_count(&self)->usize{self.payloads.len()}
    pub fn payload_len(&self,index:usize)->Result<u64,String>{self.payloads.get(index).map(TransferPayload::len).ok_or_else(||"Missing transfer payload".into())}
    #[cfg(not(target_arch="wasm32"))]
    pub fn read_chunk(&self,index:usize,offset:u64,length:usize)->Result<Vec<u8>,String>{self.payloads.get(index).ok_or("Missing transfer payload")?.chunk(offset,length)}
    pub async fn read_chunk_async(&self,index:usize,offset:u64,length:usize)->Result<Vec<u8>,String> {
        let payload=self.payloads.get(index).ok_or("Missing transfer payload")?;
        if length>MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end|end>payload.len()) {return Err("Transfer chunk exceeds payload".into());}
        match payload {
            TransferPayload::Tile(tile)=>{let bytes=tile.read_compressed().await?;Ok(bytes[offset as usize..offset as usize+length].to_vec())},
            TransferPayload::Opaque(resource)=>Ok(resource.read_chunk_async(offset,length,&AtomicBool::new(false)).await?.to_vec()),
            _=>Ok(payload.memory_chunk(offset as usize,length)),
        }
    }
    pub async fn from_chunks(descriptor:TransferDescriptor,chunks:Vec<Vec<Arc<[u8]>>>)->Result<Self,String>{
        let mut receiver=TransferReceiver::new(descriptor,ProjectLimits::default()).await?;
        if chunks.len()!=receiver.buffers.len(){return Err("Transfer payload count mismatch".into());}
        for (index,chunks) in chunks.into_iter().enumerate(){for chunk in chunks{receiver.push_chunk(index,&chunk)?;}}
        receiver.finish()
    }
    fn bytes(&self,index:usize)->Result<Arc<[u8]>,String>{match self.payloads.get(index){Some(TransferPayload::Bytes(v)|TransferPayload::Vector(_,v))=>Ok(v.clone()),Some(TransferPayload::Tile(tile))=>{
            #[cfg(not(target_arch="wasm32"))]
            {tile.compressed_on_worker().map(|range|range.into_arc())}
            #[cfg(target_arch="wasm32")]
            {tile.memory_compressed()?.ok_or_else(||"Transfer input bytes were not supplied to the worker".into())}
        },Some(TransferPayload::Code(v))=>Ok(v.as_bytes().into()),_=>Err("Wrong transfer byte payload".into())}}
    fn encoded(&self,payload:&Option<EncodedPayload>)->Result<Option<EncodedBytes>,String>{payload.as_ref().map(|v|Ok(EncodedBytes{encoding:v.encoding,bytes:self.bytes(v.payload)?})).transpose()}
    pub fn adopt_verified(&self,limits:ProjectLimits,cancel:&AtomicBool)->Result<ArtworkCapture,String>{
        self.adopt_verified_with_selection(limits,cancel).map(|(capture,_)|capture)
    }
    pub fn adopt_verified_with_selection(&self,limits:ProjectLimits,cancel:&AtomicBool)->Result<(ArtworkCapture,Option<crate::Selection>),String>{
        self.adopt(limits,cancel,&self.trusted_programs)
    }
    pub fn adopt_with_programs(&self,limits:ProjectLimits,cancel:&AtomicBool,programs:&super::TrustedPrograms)->Result<ArtworkCapture,String> {
        self.adopt(limits,cancel,programs).map(|(capture,_)|capture)
    }
    fn adopt(&self,limits:ProjectLimits,cancel:&AtomicBool,programs:&super::TrustedPrograms)->Result<(ArtworkCapture,Option<crate::Selection>),String>{
        active(cancel)?;let manifest=manifest(&self.descriptor,limits)?;
        let empty:Arc<[u8]>=Arc::from([]);let backing=ImmutableBacking::new(Arc::new(empty)).map_err(str::to_string)?;
        let mut reader=ResourceReader::new(&manifest,&backing,cancel,limits);reader.admission=self.admission.clone().or(reader.admission);reader.private=true;reader.trusted_programs=programs.clone();reader.require_verified();
        if manifest.resources.len()!=self.descriptor.resources.len()||self.descriptor.checkpoint.document!=manifest.document{return Err("Transfer identity inventory mismatch".into());}
        let mut decoded=0u64;
        let slot_counts:BTreeMap<PortableId,usize>=manifest.objects.values().filter(|record|record["type"]=="capy.vector-object/1").flat_map(|record|{
            let count=record["data"]["slots"].as_array().map_or(0,Vec::len);
            record["data"]["items"].as_array().into_iter().flatten().filter_map(move |item|Some((resources::reference_id(item).ok()?,count)))
        }).collect();
        for (id,state) in &self.descriptor.resources {
            active(cancel)?;let record=&manifest.resources.get(id).ok_or("Unknown transfer resource")?.value;
            let kind=record["type"].as_str().ok_or("Missing transfer resource type")?;
            let resource=|index,encoded:&Option<EncodedPayload>|->Result<Resource<[u8]>,String>{let bytes=self.bytes(index)?;Ok(match self.encoded(encoded)?{Some(encoded)=>Resource::with_id_and_encoded(*id,bytes,encoded),None=>Resource::with_id(*id,bytes)})};
            match state {
                VerifiedResource::Bytes{payload,encoded}=>{let limit=match kind {"capy.icc/1"=>crate::color::source::MAX_PROFILE_BYTES,"capy.photo-metadata/1"=>crate::PhotoMetadata::MAX_BYTES,"capy.vector-items/1"=>super::vector_records::MAX_CHUNK_BYTES,_=>return Err("Transfer byte resource type mismatch".into())};let value=resource(*payload,encoded)?;if value.len()>limit{return Err("Oversized verified byte resource".into());}decoded=decoded.saturating_add(value.len() as u64);reader.bytes.insert(*id,value);},
                VerifiedResource::Code{payload,encoded}=>{if kind!="capy.wgsl/1"{return Err("Transfer shader type mismatch".into());}let code=match self.payloads.get(*payload){Some(TransferPayload::Code(code))=>code.clone(),_=>{let bytes=self.bytes(*payload)?;let text:Arc<str>=std::str::from_utf8(&bytes).map_err(|_|"Invalid verified shader text")?.into();match self.encoded(encoded)?{Some(encoded)=>Resource::with_id_and_encoded(*id,text,encoded),None=>Resource::with_id(*id,text)}}};if code.len()>4*1024*1024{return Err("Oversized transfer shader".into());}decoded=decoded.saturating_add(code.len() as u64);reader.texts.insert(*id,code);},
                VerifiedResource::Lut{payload,encoded,digest,spaces}=>{if kind!="capy.lut3d/1"{return Err("Transfer lookup type mismatch".into());}let data=&record["data"];let size=super::values::u32_value(&data["size"]).map_err(|e|e.to_string())?;let domain: [[f32;3];2]=serde_json::from_value(data["domain"].clone()).map_err(|e|e.to_string())?;let bytes=resource(*payload,encoded)?;decoded=decoded.saturating_add(bytes.len() as u64);reader.bytes.insert(*id,bytes.clone());reader.luts.insert(*id,Arc::new(Lut3d::from_verified_resource(size,domain,Arc::from(""),*digest,bytes,*spaces).map_err(str::to_string)?));},
                VerifiedResource::Coverage{payload}=>{if kind!="capy.selection-coverage/1"{return Err("Transfer selection type mismatch".into());}let bytes=payload.map(|p|self.bytes(p)).transpose()?.unwrap_or_else(||Arc::from([]));reader.bytes.insert(*id,Resource::with_id(*id,bytes));},
                VerifiedResource::Vector{payload}=>{
                    if kind!="capy.vector-items/1"{return Err("Transfer vector type mismatch".into());}
                    let bytes=self.bytes(*payload)?;decoded=decoded.saturating_add(bytes.len() as u64);
                    if let Some(TransferPayload::Vector(chunk,_))=self.payloads.get(*payload) && chunk.records().iter().all(|r|matches!(r,vector::Record::Path(_))) {reader.vectors.insert(*id,(0,0,slot_counts.get(id).copied().unwrap_or(0),chunk.summary().next_item-1,chunk.clone()));}
                    reader.bytes.insert(*id,Resource::with_id_and_encoded(*id,bytes.clone(),EncodedBytes{encoding:ResourceEncoding::Raw,bytes}));
                },
                VerifiedResource::Tile{..}|VerifiedResource::TileRange{..}|VerifiedResource::Opaque{..}=>(),
            }
        }
        let leases:Vec<_>=self.backings.iter().cloned().map(super::PackageLease::new).collect();
        for (id,state) in &self.descriptor.resources {
            if !matches!(state,VerifiedResource::Tile {..}|VerifiedResource::TileRange {..}) {continue;}
            let record=&manifest.resources[id].value;if record["type"]!="capy.raster-tile/1"{return Err("Transfer tile type mismatch".into());}
            let descriptor=resources::parse_descriptor(&record["data"]).map_err(|e|e.to_string())?;
            let raw=descriptor.byte_len([super::RASTER_TILE_SIZE;2]).ok_or("Invalid verified tile descriptor")? as u64;
            decoded=decoded.saturating_add(raw);
            let tile=match state {
                VerifiedResource::Tile {payload,identity}=>crate::raster::TileBlob::from_verified_resource_with_content_identity(*id,descriptor,self.bytes(*payload)?,*identity)?,
                VerifiedResource::TileRange {backing,offset,original,filtered,canonical,identity}=> {
                    let range=leases.get(*backing).ok_or("Missing transfer backing")?.range(*offset,original.bytes,original.crc32,descriptor,*filtered)?;
                    if let Some(integrity)=canonical {range.seed_integrity(*integrity)?;}
                    crate::raster::TileBlob::from_transferred_range(*id,descriptor,range,*identity)?
                },
                _=>unreachable!(),
            };
            reader.tiles.insert(*id,Arc::new(tile));
        }
        for selection in &self.descriptor.selections {
            let words=match self.payloads.get(selection.payload){Some(TransferPayload::Words(words))=>words.clone(),_=>return Err("Missing decoded selection words".into())};decoded=decoded.saturating_add(words.len() as u64*4);
            let chunks=selection.chunks.iter().map(|id|reader.bytes.get(id).cloned().ok_or("Missing selection chunk identity")).collect::<Result<Vec<_>,_>>()?;
            let cached=chunks.iter().all(|c|!c.is_empty()).then_some(chunks);
            let pixels=Arc::new(SelectionPixels::from_verified_words(selection.extent,selection.bounds,selection.bytes,words,selection.chunks.clone(),cached)?);
            reader.selections.insert((selection.extent,selection.bounds,selection.bytes,selection.chunks.clone()),pixels);
        }
        if decoded>limits.raster_bytes{return Err(crate::MEMORY_BUDGET_EXCEEDED.into());}
        let opaque=self.descriptor.resources.iter().filter_map(|(id,state)|if let VerifiedResource::Opaque{payload}=state{Some((*id,*payload))}else{None})
            .chain(self.descriptor.retained_resources.iter().map(|(id,payload)|(*id,*payload)));
        for (id,payload) in opaque {
            let record=manifest.resources.get(&id).ok_or("Missing retained transfer resource")?;
            if self.payloads.get(payload).ok_or("Missing retained payload")?.len()!=record.bytes{return Err("Retained resource length mismatch".into());}
            if let Some(TransferPayload::Opaque(resource))=self.payloads.get(payload){reader.opaque.insert(id,resource.clone());continue;}
            let bytes=self.bytes(payload)?;let length=bytes.len() as u64;let backing=ImmutableBacking::new(Arc::new(bytes)).map_err(str::to_string)?;
            reader.opaque.insert(id,Arc::new(OpaqueResource::from_record(id,&record.value,backing,0,length,record.crc32)?));
        }
        let mut artwork=artwork_records::decode_with_layout(&manifest,&mut reader,Some(&self.descriptor.layout)).map_err(|e|e.to_string())?;
        let working_selection=self.descriptor.working_selection.as_ref().map(|value|super::selection_records::decode_selection(value,&mut reader)).transpose().map_err(|e|e.to_string())?;
        let slotted=artwork_records::slot_resources(&artwork);
        let extensions=Extensions {records:manifest.objects.iter().filter(|(_,r)|r["ancillary"]==true).map(|(id,r)|(*id,r.clone())).collect(),
            resources:std::mem::take(&mut reader.opaque).into_iter().filter(|(id,_)|!slotted.contains(id)).collect()};
        artwork.extensions=Arc::new(extensions);
        crate::Document::from_artwork(artwork.clone()).map_err(|e|e.to_string())?.validate(limits)?;
        active(cancel)?;
        #[cfg_attr(target_arch="wasm32",expect(clippy::arc_with_non_send_sync,reason="Shared artwork ownership includes single-threaded Wasm Liquify fields"))]
        let artwork=Arc::new(artwork);
        Ok((ArtworkCapture{artwork,checkpoint:self.descriptor.checkpoint,memory:Some(self.memory.get_or_init(||crate::DocumentMemory::new(crate::MemoryBudget::process(),reader.admission.clone())).clone())},working_selection))
    }
}
enum PayloadBuffer {Bytes(Arc<[MaybeUninit<u8>]>),Words(Arc<[MaybeUninit<u32>]>) }
pub struct TransferReceiver {admission:crate::MemoryReservation,descriptor:TransferDescriptor,buffers:Vec<PayloadBuffer>,positions:Vec<u64>,backings:Vec<ImmutableBacking>}
impl TransferReceiver {
    pub async fn new(descriptor:TransferDescriptor,limits:ProjectLimits)->Result<Self,String>{
        Self::with_sources(descriptor,limits,|_|Err("Transfer backing resolver is missing".into())).await
    }
    pub async fn with_sources(descriptor:TransferDescriptor,limits:ProjectLimits,source:impl FnMut(&BackingReference)->Result<Arc<dyn super::ByteSource>,String>)->Result<Self,String> {
        Self::receive(descriptor,limits,source,crate::MemoryBudget::process()).await
    }
    async fn receive(descriptor:TransferDescriptor,limits:ProjectLimits,mut source:impl FnMut(&BackingReference)->Result<Arc<dyn super::ByteSource>,String>,budget:crate::MemoryBudget)->Result<Self,String> {
        let parsed=manifest(&descriptor,limits)?;
        if !parsed.resources.keys().eq(descriptor.resources.keys()) || parsed.document!=descriptor.checkpoint.document {return Err("Transfer identity inventory mismatch".into());}
        let mut bytes=BTreeSet::new();let mut words=BTreeSet::new();
        for resource in descriptor.resources.values() {match resource {
            VerifiedResource::Bytes{payload,encoded}|VerifiedResource::Code{payload,encoded}|VerifiedResource::Lut{payload,encoded,..}=>{bytes.insert(*payload);if let Some(encoded)=encoded {bytes.insert(encoded.payload);}},
            VerifiedResource::Tile{payload,..}|VerifiedResource::Vector{payload}|VerifiedResource::Opaque{payload}|VerifiedResource::Coverage{payload:Some(payload)}=>{bytes.insert(*payload);},
            VerifiedResource::TileRange {..}|VerifiedResource::Coverage{payload:None}=>(),
        }}
        bytes.extend(descriptor.retained_resources.values().copied());
        words.extend(descriptor.selections.iter().map(|selection|selection.payload));
        if !bytes.is_disjoint(&words)||bytes.union(&words).count()!=descriptor.lengths.len()
            ||bytes.union(&words).any(|index|*index>=descriptor.lengths.len()) {return Err("Unindexed or conflicting transfer payloads".into());}
        if descriptor.retained_resources.keys().any(|id|!descriptor.resources.contains_key(id)){return Err("Unknown retained transfer resource".into());}
        let mut referenced=BTreeSet::new();
        for resource in descriptor.resources.values() {if let VerifiedResource::TileRange {backing,offset,original,filtered,canonical,..}=resource {
            let owner=descriptor.backings.get(*backing).ok_or("Missing transfer backing reference")?;
            if original.bytes==0 || original.bytes>MAX_RANGE_BYTES as u64 || offset.checked_add(original.bytes).is_none_or(|end|end>owner.length)
                || !filtered && *canonical!=Some(*original) || canonical.is_some_and(|value|value.bytes==0 || value.bytes>MAX_RANGE_BYTES as u64) {return Err("Invalid transfer tile range".into());}
            referenced.insert(*backing);
        }}
        if referenced.len()!=descriptor.backings.len() {return Err("Unreferenced transfer backing".into());}
        let mut identities=BTreeSet::new();
        let backings=descriptor.backings.iter().map(|reference| {
            if reference.key.is_empty() || reference.key.len()>1024 || reference.length>9_007_199_254_740_991 || !identities.insert((&reference.key,reference.generation)) {return Err("Invalid transfer backing identity".into());}
            let source=source(reference)?;
            if source.byte_len()!=reference.length || source.transfer_key().as_ref()!=Some(&reference.key) {return Err("Transfer backing identity mismatch".into());}
            ImmutableBacking::with_generation(source,reference.generation).map_err(str::to_string)
        }).collect::<Result<Vec<_>,String>>()?;
        let total=descriptor.lengths.iter().try_fold(0u64,|a,b|a.checked_add(*b).ok_or("Transfer length overflow"))?;
        if total>limits.raster_bytes.saturating_add(limits.asset_bytes).saturating_mul(2){return Err(crate::MEMORY_BUDGET_EXCEEDED.into());}
        let admission=budget.admit_import(total).ok_or(crate::MEMORY_BUDGET_EXCEEDED)?;
        admission.covering().await;
        let buffers=descriptor.lengths.iter().enumerate().map(|(i,length)|{
            let length=usize::try_from(*length).map_err(|_|"Transfer allocation overflow")?;
            if words.contains(&i){if length%4!=0{return Err("Misaligned selection words".into());}Ok(PayloadBuffer::Words(Arc::<[u32]>::new_uninit_slice(length/4)))}
            else{Ok(PayloadBuffer::Bytes(Arc::<[u8]>::new_uninit_slice(length)))}
        }).collect::<Result<Vec<_>,String>>()?;
        let positions=vec![0;buffers.len()];Ok(Self{admission,descriptor,buffers,positions,backings})
    }
    pub fn missing_payloads(&self)->Vec<usize>{self.positions.iter().zip(&self.descriptor.lengths).enumerate().filter_map(|(i,(position,length))|(position!=length).then_some(i)).collect()}
    pub fn push_chunk(&mut self,index:usize,bytes:&[u8])->Result<(),String>{
        let position=*self.positions.get(index).ok_or("Missing transfer payload")?;
        if bytes.len()>MAX_RANGE_BYTES||position.checked_add(bytes.len() as u64).is_none_or(|n|n>self.descriptor.lengths[index]){return Err("Transfer chunk exceeds admission".into());}
        match &mut self.buffers[index] {
            PayloadBuffer::Bytes(buffer)=>{let start=position as usize;for (out,input) in Arc::get_mut(buffer).unwrap()[start..start+bytes.len()].iter_mut().zip(bytes){out.write(*input);}},
            PayloadBuffer::Words(buffer)=>{if !position.is_multiple_of(4)||!bytes.len().is_multiple_of(4){return Err("Misaligned selection transfer chunk".into());}let start=position as usize/4;for (out,input) in Arc::get_mut(buffer).unwrap()[start..start+bytes.len()/4].iter_mut().zip(bytes.as_chunks::<4>().0){out.write(u32::from_le_bytes(*input));}},
        }
        self.positions[index]+=bytes.len() as u64;Ok(())
    }
    pub fn finish(self)->Result<PreparedTransfer,String>{
        if self.positions!=self.descriptor.lengths{return Err("Incomplete transfer payloads".into());}
        let payloads=self.buffers.into_iter().map(|buffer|match buffer {
            PayloadBuffer::Bytes(bytes)=>TransferPayload::Bytes(unsafe{bytes.assume_init()}),
            PayloadBuffer::Words(words)=>TransferPayload::Words(unsafe{words.assume_init()}),
        }).collect();
        Ok(PreparedTransfer{memory:std::sync::OnceLock::new(),admission:Some(self.admission),descriptor:self.descriptor,payloads,backings:self.backings,trusted_programs:Default::default()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Editor, Selection, SelectionShape, EffectInstance, EffectValue};
    use super::super::codec::{PreparedPackage, OpenOutcome};
    fn capture()->ArtworkCapture {
        let mut document=Document::new(PortableId::random(),19,11,crate::DocumentNames{paint:"ink".into(),paper:"paper".into()});
        document.artwork.paint.reserve(PortableId::random()).unwrap();
        document.artwork.occurrences.reserve(PortableId::random()).unwrap();
        let source=document.working.target.unwrap();let SourceTarget::Paint(handle)=source else{unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|x,y|[x as u8,y as u8,80,255]))));
        document.artwork.metadata=Arc::new(crate::PhotoMetadata{xmp:Some(Resource::from(b"<x:xmpmeta/>".to_vec())),..Default::default()});
        let mut output=document.artwork.outputs.get(document.artwork.default_output).unwrap().clone();
        output.proof=Some(crate::color::ProofRecipe {name:"proof".into(),profile:crate::color::ColorProfile::Icc(Resource::from(vec![1,2,3,4])),conversion:Default::default(),simulate_paper:false,simulate_black_ink:false});
        document.artwork.outputs.get_mut(document.artwork.default_output).unwrap().clone_from(&output);
        Editor::new(document).capture(7,EvaluationContext{elapsed:0.,phases:Vec::new().into()}).unwrap()
    }
    #[test]
    fn a_received_transfer_allocates_only_into_room_another_owner_has_released() {
        use std::{future::Future,task::{Context,Poll,Waker}};
        let prepared=PreparedTransfer::capture(&capture(),&AtomicBool::new(false)).unwrap();
        let total=prepared.descriptor.lengths.iter().sum::<u64>();
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(1<<30),..Default::default()});
        let limit=budget.ram_limit_bytes();
        let cache=budget.register_owner(crate::MemoryOwnerKind::Other,crate::MemoryClass::Warm,None);
        cache.report(crate::MemoryReport {ram_bytes:limit,demand_ram_bytes:limit,..Default::default()});
        let physical=|allocated:bool|budget.snapshot().ram_physical_bytes+if allocated {total}else {0};
        let mut receiving=Box::pin(TransferReceiver::receive(prepared.descriptor.clone(),ProjectLimits::default(),|_|Err("No backing".into()),budget.clone()));
        let mut context=Context::from_waker(Waker::noop());
        let first=receiving.as_mut().poll(&mut context);
        assert!(physical(first.is_ready())<=limit,"the receiver allocated into room another owner still holds: {} > {limit}",physical(first.is_ready()));
        cache.report(crate::MemoryReport {ram_bytes:cache.grant().ram_bytes,demand_ram_bytes:limit,..Default::default()});
        let Poll::Ready(receiver)=receiving.as_mut().poll(&mut context) else {panic!("the release lets the receiver allocate")};
        let mut receiver=receiver.unwrap();
        assert!(physical(true)<=limit);
        for index in 0..prepared.payload_count() {receiver.push_chunk(index,&prepared.read_chunk(index,0,prepared.payload_len(index).unwrap() as usize).unwrap()).unwrap();}
        assert!(receiver.finish().is_ok());
    }
    fn receive(prepared:&PreparedTransfer)->PreparedTransfer {
        let descriptor=serde_json::from_slice(&serde_json::to_vec(prepared.descriptor()).unwrap()).unwrap();
        let mut receiver=pollster::block_on(TransferReceiver::new(descriptor,ProjectLimits::default())).unwrap();
        for index in 0..prepared.payload_count(){let mut offset=0;let length=prepared.payload_len(index).unwrap();while offset<length {
            let count=(length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index,&prepared.read_chunk(index,offset,count).unwrap()).unwrap();offset+=count as u64;
        }}receiver.finish().unwrap()
    }
    fn lazy_capture(bytes:Arc<[u8]>,reads:Arc<std::sync::atomic::AtomicUsize>)->ArtworkCapture {
        struct Source {bytes:Arc<[u8]>,reads:Arc<std::sync::atomic::AtomicUsize>}
        impl super::super::ByteSource for Source {
            fn byte_len(&self)->u64 {self.bytes.len() as u64}
            fn resident_bytes(&self)->usize {0}
            fn poll(&self,offset:u64,length:usize)->Result<super::super::RangeState,String> {
                self.reads.fetch_add(1,Ordering::SeqCst);
                Ok(super::super::RangeState::Ready(super::super::ByteRange::new(self.bytes.clone(),offset as usize..offset as usize+length).unwrap()))
            }
        }
        let mut capture=capture();let artwork=Arc::make_mut(&mut capture.artwork);
        let handle=artwork.paint.iter().next().unwrap().0;let base=artwork.paint.get_mut(handle).unwrap().base.as_mut().unwrap();
        let mut image=(**base.image.storage()).clone();let (coordinate,tile)=image.tiles.iter().next().unwrap();
        let lease=super::super::PackageLease::new(ImmutableBacking::new(Arc::new(Source {bytes:bytes.clone(),reads})).unwrap());
        let range=lease.range(0,bytes.len() as u64,crc32fast::hash(&bytes),tile.descriptor,false).unwrap();
        image.tiles.insert(*coordinate,Arc::new(crate::raster::TileBlob::from_package_range(tile.resource_id(),tile.descriptor,range).unwrap()));
        base.image=Image::with_id(base.image.id(),Arc::new(image));capture
    }
    struct ExportedFile {bytes:Arc<[u8]>,reads:std::sync::atomic::AtomicUsize}
    impl super::super::ByteSource for ExportedFile {
        fn byte_len(&self)->u64 {self.bytes.len() as u64}
        fn resident_bytes(&self)->usize {0}
        fn transfer_key(&self)->Option<String> {Some("exported-file".into())}
        fn poll(&self,offset:u64,length:usize)->Result<super::super::RangeState,String> {
            self.reads.fetch_add(1,Ordering::SeqCst);
            Ok(super::super::RangeState::Ready(super::super::ByteRange::new(self.bytes.clone(),offset as usize..offset as usize+length)?))
        }
    }
    fn reference_fixture(filtered:bool)->(ArtworkCapture,Arc<ExportedFile>) {
        let mut capture=capture();let artwork=Arc::make_mut(&mut capture.artwork);
        let paint=artwork.paint.iter().next().unwrap().0;let base=artwork.paint.get_mut(paint).unwrap().base.as_mut().unwrap();
        let mut image=(**base.image.storage()).clone();let (&coordinate,tile)=image.tiles.iter().next().unwrap();
        let bytes=if filtered {
            use std::io::Write;
            let mut planes=vec![0;256*256*4];for row in planes.chunks_mut(256) {row[0]=21;}
            let mut encoder=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::fast());encoder.write_all(&planes).unwrap();Arc::from(encoder.finish().unwrap())
        }else {tile.memory_compressed().unwrap().unwrap()};
        let file=Arc::new(ExportedFile {bytes,reads:Default::default()});let lease=super::super::PackageLease::new(ImmutableBacking::new(file.clone()).unwrap());
        let range=lease.range(0,file.bytes.len() as u64,crc32fast::hash(&file.bytes),tile.descriptor,filtered).unwrap();
        image.tiles.insert(coordinate,Arc::new(crate::raster::TileBlob::from_package_range(tile.resource_id(),tile.descriptor,range).unwrap()));
        base.image=Image::new(Arc::new(image));(capture,file)
    }
    fn receive_references(transfer:&PreparedTransfer,file:Arc<ExportedFile>)->Result<PreparedTransfer,String> {
        let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
        let mut receiver=pollster::block_on(TransferReceiver::with_sources(descriptor,ProjectLimits::default(),|_|Ok(file.clone())))?;
        for index in 0..transfer.payload_count() {receiver.push_chunk(index,&transfer.read_chunk(index,0,transfer.payload_len(index)? as usize)?)?;}
        receiver.finish()
    }
    #[test]
    fn reference_transfer_does_not_reserve_the_tile_payload() {
        let (capture,file)=reference_fixture(false);let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(384*1024*1024),..Default::default()});
        let buffered=PreparedTransfer::capture(&capture,&AtomicBool::new(false)).unwrap();
        let metadata_payloads=buffered.descriptor.lengths.iter().sum::<u64>()-file.bytes.len() as u64;
        assert!(metadata_payloads<file.bytes.len() as u64);
        let owner=budget.register_owner(crate::MemoryOwnerKind::Authored,crate::MemoryClass::Pinned,None);
        owner.report(crate::MemoryReport {ram_bytes:budget.ram_limit_bytes()-metadata_payloads,pinned_ram_bytes:budget.ram_limit_bytes()-metadata_payloads,..Default::default()});
        assert!(buffered.reserve_copy(&budget,0).is_err());
        let transfer=pollster::block_on(PreparedTransfer::capture_referenced(&capture,&None,&AtomicBool::new(false),&budget)).unwrap();
        assert_eq!(transfer.descriptor.lengths.iter().sum::<u64>(),metadata_payloads);
        let reservation=transfer.reserve_copy(&budget,0).unwrap();assert_eq!(reservation.reserved_bytes(),transfer.descriptor.lengths.iter().sum::<u64>());
        assert_eq!(file.reads.load(Ordering::SeqCst),0);
        receive_references(&transfer,file).unwrap().adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).unwrap();
    }
    #[test]
    fn reference_transfer_rejects_missing_forged_and_overflowing_ranges_without_reads() {
        let (capture,file)=reference_fixture(false);let cancel=AtomicBool::new(false);
        let transfer=pollster::block_on(PreparedTransfer::capture_referenced(&capture,&None,&cancel,&crate::MemoryBudget::process())).unwrap();
        assert!(pollster::block_on(TransferReceiver::new(transfer.descriptor.clone(),ProjectLimits::default())).is_err());
        for mutation in 0..4 {
            let mut descriptor=transfer.descriptor.clone();
            match mutation {
                0=>descriptor.backings.clear(),
                1=>descriptor.backings[0].key="forged-file".into(),
                2=>descriptor.backings[0].length+=1,
                3=>if let Some(VerifiedResource::TileRange {offset,..})=descriptor.resources.values_mut().find(|r|matches!(r,VerifiedResource::TileRange {..})) {*offset=u64::MAX;},
                _=>unreachable!(),
            }
            assert!(pollster::block_on(TransferReceiver::with_sources(descriptor,ProjectLimits::default(),|_|Ok(file.clone()))).is_err());
        }
        let mut descriptor=transfer.descriptor.clone();
        if let Some(VerifiedResource::TileRange {identity:Some(crate::raster::TileContentIdentity::Package {descriptor,..}),..})=descriptor.resources.values_mut().find(|r|matches!(r,VerifiedResource::TileRange {..})) {descriptor[0]^=1;}else {panic!("Missing package identity");}
        let mut receiver=pollster::block_on(TransferReceiver::with_sources(descriptor,ProjectLimits::default(),|_|Ok(file.clone()))).unwrap();
        for index in 0..transfer.payload_count() {receiver.push_chunk(index,&transfer.read_chunk(index,0,transfer.payload_len(index).unwrap() as usize).unwrap()).unwrap();}
        assert!(receiver.finish().unwrap().adopt_verified(ProjectLimits::default(),&cancel).is_err());
        assert_eq!(file.reads.load(Ordering::SeqCst),0);
    }
    #[test]
    #[cfg(not(target_arch="wasm32"))]
    fn filtered_package_tiles_recover_after_temporary_memory_refusal() {
        const CHILD: &str = "CAPY_TEST_PACKAGE_MEMORY_REFUSAL";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "package::transfer::tests::filtered_package_tiles_recover_after_temporary_memory_refusal",
                    "--nocapture", "--test-threads=1"])
                .env(CHILD, "1").output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success() && stdout.contains("1 passed; 0 failed"),
                "{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
            return;
        }
        let (capture, file) = reference_fixture(true);
        let tile = capture.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap()
            .image.tiles.values().next().unwrap();
        let budget = crate::MemoryBudget::process();
        budget.flush_reports();
        let held = budget.admit_import(budget.import_headroom_bytes()).unwrap();
        assert_eq!(tile.decode_on_worker().unwrap_err(), crate::MEMORY_BUDGET_EXCEEDED);
        assert!(tile.compressed.healthy().is_ok(), "a budget refusal is not damaged source data");
        drop(held);
        budget.flush_reports();
        assert_eq!(tile.decode_on_worker().unwrap(), vec![21; 256 * 256 * 4]);
        assert!(tile.compressed.healthy().is_ok());
        assert_eq!(file.reads.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn filtered_references_normalize_lazily_and_keep_their_generation() {
        let (capture,file)=reference_fixture(true);let cancel=AtomicBool::new(false);
        let transfer=pollster::block_on(PreparedTransfer::capture_referenced(&capture,&None,&cancel,&crate::MemoryBudget::process())).unwrap();
        let received=receive_references(&transfer,file.clone()).unwrap();let adopted=received.adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        let tile=adopted.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles.values().next().unwrap();
        assert_eq!(file.reads.load(Ordering::SeqCst),0);
        assert_eq!(tile.decode_on_worker().unwrap(),vec![21;256*256*4]);
        let original=capture.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles.values().next().unwrap();
        original.decode_on_worker().unwrap();assert!(original.same_content(tile));
        let second=pollster::block_on(PreparedTransfer::capture_referenced(&adopted,&None,&cancel,&crate::MemoryBudget::process())).unwrap();
        assert_eq!(second.descriptor.backings,transfer.descriptor.backings);
        let again=receive_references(&second,file).unwrap().adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        assert!(tile.same_content(again.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles.values().next().unwrap()));
    }
    #[test]
    fn file_backed_transfer_keeps_unread_tiles_after_parser_release() {
        struct File {
            bytes:Arc<[u8]>,reads:Arc<std::sync::atomic::AtomicUsize>,
        }
        impl super::super::ByteSource for File {
            fn byte_len(&self)->u64 {self.bytes.len() as u64}
            fn transfer_key(&self)->Option<String> {Some("test-file".into())}
            fn resident_bytes(&self)->usize {0}
            fn poll(&self,offset:u64,length:usize)->Result<super::super::RangeState,String> {
                self.reads.fetch_add(1,Ordering::SeqCst);
                Ok(super::super::RangeState::Ready(super::super::ByteRange::new(self.bytes.clone(),offset as usize..offset as usize+length)?))
            }
        }
        let image=crate::color::source::rgba8_source([768,256],|x,y| {
            let mut bits=x.wrapping_mul(0x9e37_79b9)^y.wrapping_mul(0x85eb_ca6b);bits^=bits>>16;bits=bits.wrapping_mul(0x7feb_352d);bits^=bits>>15;
            [bits as u8,(bits>>8) as u8,(bits>>16) as u8,255]
        });
        let expected=image.tiles[&[2,0]].decode_on_worker().unwrap();let reads=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut bytes=Vec::new();let mut ranges=Vec::new();
        for (&coordinate,tile) in &image.tiles {
            let encoded=tile.memory_compressed().unwrap().unwrap();let offset=bytes.len() as u64;bytes.extend_from_slice(&encoded);
            ranges.push((coordinate,tile.resource_id(),tile.descriptor,offset,encoded.len() as u64,crc32fast::hash(&encoded)));
        }
        let file=Arc::new(File {bytes:bytes.into(),reads:reads.clone()});let alive=Arc::downgrade(&file);
        let backing=ImmutableBacking::new(file.clone()).unwrap();let lease=super::super::PackageLease::new(backing);
        let mut source=(*image).clone();
        for (coordinate,id,descriptor,offset,length,crc) in ranges {
            source.tiles.insert(coordinate,Arc::new(crate::raster::TileBlob::from_package_range(id,descriptor,lease.range(offset,length,crc,descriptor,false).unwrap()).unwrap()));
        }
        let mut document=Document::new(PortableId::random(),768,256,crate::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let SourceTarget::Paint(paint)=document.working.target.unwrap() else {unreachable!()};
        document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(Arc::new(source))));
        let parser=Editor::new(document).capture(1,Default::default()).unwrap();let cancel=AtomicBool::new(false);
        let transfer=pollster::block_on(PreparedTransfer::capture_referenced(&parser,&None,&cancel,&crate::MemoryBudget::process())).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst),0);
        assert_eq!(transfer.payloads.iter().filter(|payload|matches!(payload,TransferPayload::Tile(_))).count(),0,"file-backed tiles must cross the worker boundary as references");
        let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
        let received=pollster::block_on(TransferReceiver::with_sources(descriptor,ProjectLimits::default(),|_|Ok(file.clone()))).unwrap().finish().unwrap();let adopted=received.adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        let tile=adopted.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles[&[2,0]].clone();
        drop(parser);drop(file);drop(lease);drop(transfer);drop(received);drop(adopted);drop(image);
        assert!(alive.upgrade().is_some());assert_eq!(reads.load(Ordering::SeqCst),0);
        assert_eq!(std::thread::spawn(move ||tile.decode_on_worker()).join().unwrap().unwrap(),expected);
        assert!(reads.load(Ordering::SeqCst)>0);assert!(alive.upgrade().is_none());
    }
    #[test]
    fn lazy_transfer_refuses_materialization_before_reading_any_tile() {
        let reads=Arc::new(std::sync::atomic::AtomicUsize::new(0));let bytes:Arc<[u8]>=vec![7;1024].into();
        let capture=lazy_capture(bytes.clone(),reads.clone());let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(384*1024*1024),..Default::default()});
        let owner=budget.register_owner(crate::MemoryOwnerKind::Authored,crate::MemoryClass::Pinned,None);
        let pinned=budget.ram_limit_bytes()-bytes.len() as u64;
        let report=|bytes|crate::MemoryReport {ram_bytes:bytes,pinned_ram_bytes:bytes,..Default::default()};
        owner.report(report(pinned+1));let cancel=AtomicBool::new(false);
        assert!(matches!(pollster::block_on(PreparedTransfer::capture_async(&capture,&None,&cancel,&budget)),Err(error) if error==crate::MEMORY_BUDGET_EXCEEDED));
        assert_eq!(reads.load(Ordering::SeqCst),0);
        owner.report(report(pinned));
        let transfer=pollster::block_on(PreparedTransfer::capture_async(&capture,&None,&cancel,&budget)).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst),0);assert_eq!(budget.import_headroom_bytes(),0);
        assert!(transfer.reserve_copy(&budget,0).is_err());
        drop(transfer);assert_eq!(budget.import_headroom_bytes(),bytes.len() as u64);
    }
    #[test]
    fn compressed_package_transfer_defers_sample_validation_until_decode() {
        let reads=Arc::new(std::sync::atomic::AtomicUsize::new(0));let bytes:Arc<[u8]>=vec![7;1024].into();
        let capture=lazy_capture(bytes.clone(),reads.clone());let cancel=AtomicBool::new(false);
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(384*1024*1024),..Default::default()});
        let transfer=pollster::block_on(PreparedTransfer::capture_async(&capture,&None,&cancel,&budget)).unwrap();assert_eq!(reads.load(Ordering::SeqCst),0);
        let tile=transfer.payloads.iter().position(|payload|matches!(payload,TransferPayload::Tile(_))).unwrap();
        assert_eq!(&*pollster::block_on(transfer.read_chunk_async(tile,0,bytes.len())).unwrap(),&*bytes);
        assert!(reads.load(Ordering::SeqCst)>0);
        let TransferPayload::Tile(tile)=&transfer.payloads[tile] else {unreachable!()};
        assert!(tile.decode_bytes_on_worker(&bytes).is_err());assert!(tile.compressed.healthy().is_err());
    }
    #[test]
    fn lazy_transfer_materialization_charges_shared_tile_owners_once() {
        let bytes:Arc<[u8]>=vec![7;1024].into();let original=lazy_capture(bytes.clone(),Default::default());
        let mut document=Document::from_artwork((*original.artwork).clone()).unwrap();
        let source=document.artwork.paint.iter().find_map(|(_,_,paint)|paint.base.as_ref()).unwrap().image.storage();
        let mut image=(**source).clone();
        for tile in image.tiles.values_mut() {*tile=Arc::new(tile.alias(PortableId::random()));}
        let (_,edit)=document.create_object_layer_edit("Alias",ImageObject::new(Image::new(Arc::new(image))),None,1).unwrap();document.apply(edit).unwrap();
        let capture=Editor::new(document).capture(0,Default::default()).unwrap();
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(384*1024*1024),..Default::default()});
        let owner=budget.register_owner(crate::MemoryOwnerKind::Authored,crate::MemoryClass::Pinned,None);let pinned=budget.ram_limit_bytes()-bytes.len() as u64;
        owner.report(crate::MemoryReport {ram_bytes:pinned,pinned_ram_bytes:pinned,..Default::default()});
        let transfer=pollster::block_on(PreparedTransfer::capture_async(&capture,&None,&AtomicBool::new(false),&budget)).unwrap();
        assert_eq!(transfer.admission.as_ref().unwrap().reserved_bytes(),bytes.len() as u64);assert_eq!(budget.import_headroom_bytes(),0);
    }
    #[cfg(any(unix,windows))]
    #[test]
    fn wire_copy_reserves_durable_payloads_and_scratch_before_reading() {
        let original=capture();
        let sources=original.artwork.paint.iter().flat_map(|(_,_,paint)|paint.base.iter().flat_map(|base|base.image.tiles.values().cloned())).collect::<Vec<_>>();
        assert!(!sources.is_empty());
        let store=crate::raster_storage::tests::Store::new(&crate::raster_storage::RetainedTiles {sources:sources.clone(),..Default::default()});
        assert!(sources.iter().all(|tile|tile.memory_compressed().unwrap().is_none()));
        let transfer=PreparedTransfer::capture(&original,&AtomicBool::new(false)).unwrap();
        std::fs::write(store.pack(),b"unreadable payload").unwrap();
        let overhead=MAX_RANGE_BYTES as u64;
        let required=transfer.descriptor.lengths.iter().sum::<u64>()+overhead;
        let budget=crate::MemoryBudget::with_facts(crate::MemoryFacts {override_bytes:Some(384*1024*1024),..Default::default()});
        let owner=budget.register_owner(crate::MemoryOwnerKind::Authored,crate::MemoryClass::Pinned,None);
        let pinned=budget.ram_limit_bytes()-required;
        let report=|bytes|crate::MemoryReport {ram_bytes:bytes,pinned_ram_bytes:bytes,..Default::default()};
        owner.report(report(pinned+1));
        assert!(transfer.reserve_copy(&budget,overhead).is_err());
        assert_eq!(budget.import_headroom_bytes(),required-1);
        owner.report(report(pinned));
        let reservation=transfer.reserve_copy(&budget,overhead).unwrap();
        assert_eq!(reservation.reserved_bytes(),required);
        assert_eq!(budget.import_headroom_bytes(),0);
        assert!(transfer.reserve_copy(&budget,0).is_err());
        drop(reservation);
        assert_eq!(budget.import_headroom_bytes(),required);
        assert!(transfer.reserve_copy(&budget,u64::MAX).is_err());
        assert_eq!(budget.import_headroom_bytes(),required);
    }
    #[test]
    fn capture_rejects_image_resource_identity_collisions_before_publication() {
        let mut capture=capture();let art=Arc::make_mut(&mut capture.artwork);
        let (_,_,paint)=art.paint.iter().next().unwrap();let samples=paint.base.as_ref().unwrap().image.storage().clone();
        let collision=samples.tiles.values().next().unwrap().resource_id();
        let paint=art.paint.iter().next().unwrap().0;art.paint.get_mut(paint).unwrap().base.as_mut().unwrap().image=Image::with_id(collision,samples);
        assert!(PreparedTransfer::capture(&capture,&AtomicBool::new(false)).is_err());
    }
    #[test]
    fn verified_transfer_preserves_final_records_resources_and_runtime_tombstone_slots(){
        let cancel=AtomicBool::new(false);let original=capture();let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let received=receive(&prepared);let reopened=received.adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        assert_eq!(reopened.checkpoint,original.checkpoint);
        assert_eq!(original.artwork.paint.capacity(),reopened.artwork.paint.capacity());
        assert_eq!(original.artwork.occurrences.capacity(),reopened.artwork.occurrences.capacity());
        for (h,id,_) in original.artwork.occurrences.iter(){assert_eq!(reopened.artwork.occurrences.resolve(id),Some(h));}
        for (h,id,paint) in original.artwork.paint.iter(){
            let loaded=reopened.artwork.paint.get(reopened.artwork.paint.resolve(id).unwrap()).unwrap();
            assert_eq!(reopened.artwork.paint.resolve(id),Some(h));
            for (coordinate,tile) in &paint.base.as_ref().unwrap().image.tiles {
                let restored=&loaded.base.as_ref().unwrap().image.tiles[coordinate];
                assert_eq!(tile.resource_id(),restored.resource_id());
                assert_eq!(tile.compressed_on_worker().map(|range| range.into_arc()).unwrap(),restored.compressed_on_worker().map(|range| range.into_arc()).unwrap());
            }
        }
        assert_eq!(original.artwork.metadata,reopened.artwork.metadata);
        assert_eq!(original.artwork.outputs.get(original.artwork.default_output),reopened.artwork.outputs.get(reopened.artwork.default_output));
        let roundtrip=PreparedTransfer::capture(&reopened,&cancel).unwrap();assert_eq!(prepared.descriptor.manifest,roundtrip.descriptor.manifest);
        assert!(prepared.read_chunk(0,0,MAX_RANGE_BYTES+1).is_err());
    }
    #[test]
    fn captured_transfer_defers_store_reads_and_retains_exact_input() {
        use crate::raster_storage::tests::Store;
        let cancel=AtomicBool::new(false);
        let original=capture();
        let editor=Editor::new(Document::from_artwork((*original.artwork).clone()).unwrap());
        let retained=editor.retained_tiles();
        let blobs=retained.try_blobs().unwrap().unwrap();
        let tile=blobs[0].clone();let id=tile.resource_id();let expected=tile.compressed_on_worker().map(|range| range.into_arc()).unwrap();let samples=tile.decode_on_worker().unwrap();
        let weak_tile=Arc::downgrade(&tile);
        let mut store=Store::new(&retained);let location=Store::location(&tile);
        let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let VerifiedResource::Tile {payload,..}=prepared.descriptor.resources[&id] else {panic!("tile payload")};
        assert_eq!(prepared.payload_len(payload).unwrap(),expected.len() as u64);
        assert_eq!(retained.resident_bytes(),0);
        assert!(prepared.read_chunk(payload,u64::MAX,1).is_err());
        assert!(prepared.read_chunk(payload,0,MAX_RANGE_BYTES+1).is_err());
        let input=Store::range(&tile);
        drop(tile);drop(blobs);drop(original);drop(editor);drop(retained);
        assert!(weak_tile.upgrade().is_some());assert!(location.upgrade().is_some());
        let mut copied=Vec::new();
        for offset in (0..expected.len()).step_by(37) {
            copied.extend(prepared.read_chunk(payload,offset as u64,(expected.len()-offset).min(37)).unwrap());
        }
        assert_eq!(copied.as_slice(),expected.as_ref());
        for transfer in [&prepared,&receive(&prepared)] {
            let loaded=transfer.adopt_verified(ProjectLimits::default(),&cancel).unwrap();
            let restored=loaded.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles.values().next().unwrap();
            assert_eq!(restored.resource_id(),id);
            assert_eq!(restored.decode_on_worker().unwrap(),samples);
        }
        drop(prepared);assert!(weak_tile.upgrade().is_none());
        pollster::block_on(store.store.collect_garbage()).unwrap();
        assert!(location.upgrade().is_none());
        assert_eq!(&*input,expected.as_ref());
        drop(input);drop(store);
    }
    #[test]
    fn decoded_selection_transfer_is_transient_compression_free_and_package_compatible(){
        let cancel=AtomicBool::new(false);let original=capture();
        let pixels=Arc::new(SelectionPixels::new([9,2],[0,0,9,2],vec![0x43214321,4,0x12341234,1]).unwrap());
        let selection=Some(Selection::pixels(pixels.clone()));assert!(pixels.transfer_chunks().is_none());
        let first=PreparedTransfer::capture_with_selection(&original,&selection,&cancel).unwrap();
        let second=PreparedTransfer::capture_with_selection(&original,&selection,&cancel).unwrap();
        assert_eq!(first.descriptor.manifest,second.descriptor.manifest);
        assert_eq!(first.descriptor.working_selection,second.descriptor.working_selection);
        assert!(pixels.transfer_chunks().is_none(),"main capture must not compress selection coverage");
        assert!(!first.descriptor.manifest["objects"].as_array().unwrap().iter().any(|o|o["type"]=="capy.selection/1"));
        let (restored,transient)=receive(&first).adopt_verified_with_selection(ProjectLimits::default(),&cancel).unwrap();
        assert_eq!(transient,selection);assert_eq!(restored.artwork.selections.len(),0);
        let SelectionShape::Pixels(transient)=transient.unwrap().shape else{unreachable!()};
        assert_eq!(pixels.transfer_chunk_ids(),transient.transfer_chunk_ids());
        assert!(transient.transfer_chunks().is_none(),"main adoption must retain decoded words without compression");
        let mut artwork=restored.artwork.as_ref().clone();
        let saved=artwork.selections.insert(PortableId::random(),SavedSelection{selection:Selection::pixels(transient.clone()),}).unwrap();
        let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Selection(saved),"saved")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;artwork.stacks.get_mut(stack).unwrap().entries.insert(0,occurrence);
        let saved_capture=artwork.capture(restored.checkpoint).unwrap();
        let mut output=std::io::Cursor::new(Vec::new());PreparedPackage::prepare(&saved_capture,None,&cancel).unwrap().write(&mut output,&cancel).unwrap();
        let data:Arc<[u8]>=output.into_inner().into();let backing=ImmutableBacking::new(Arc::new(data)).unwrap();
        let OpenOutcome::Candidate{artwork,..}=crate::package::codec::open(backing,ProjectLimits::default(),&cancel).unwrap() else{panic!("transfer output must reopen through final package codec")};
        let loaded=&artwork.selections.iter().next().unwrap().2.selection;
        assert_eq!(*loaded,Selection::pixels(transient.clone()));
        let SelectionShape::Pixels(loaded)=&loaded.shape else{unreachable!()};assert_eq!(loaded.transfer_chunk_ids(),pixels.transfer_chunk_ids());
    }
    #[test]
    fn lookup_alias_titles_keep_one_verified_owner_and_admitted_spaces(){
        let cancel=AtomicBool::new(false);let mut original=capture();let art=Arc::make_mut(&mut original.artwork);
        let lut=Arc::new(Lut3d::from_samples(2,[[0.;3],[1.;3]],Arc::from("first"),vec![[0.25;3];8].into()).unwrap());
        let alias=Arc::new(lut.with_title(Arc::from("second")).unwrap());let mut occurrences=Vec::new();
        let program=crate::bundled_effect_catalog().get("color_lookup").unwrap().program();

        for lookup in [lut.clone(),alias.clone()]{let mut effect=EffectInstance::new(program.clone());effect.set("resource",EffectValue::Lut3d(Some(lookup))).unwrap();
            let application=art.effects.insert(PortableId::random(),EffectApplication::new(program.clone(),effect.values,[19,11])).unwrap();
            occurrences.push(art.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),"lookup")).unwrap());
        }
        let stack=art.compositions.get(art.root).unwrap().result;art.stacks.get_mut(stack).unwrap().entries.splice(0..0,occurrences);
        let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();assert_eq!(prepared.descriptor.resources.values().filter(|r|matches!(r,VerifiedResource::Lut{..})).count(),1);
        let loaded=receive(&prepared).adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        let mut resources=crate::RootInventory::default();resources.artwork(&loaded.artwork);let resources=resources.resources;
        assert_eq!(resources.len(),2);assert_eq!(resources[0].title(),"first");assert_eq!(resources[1].title(),"second");
        assert_eq!(resources[0].admitted_spaces(),lut.admitted_spaces());assert!(resources[0].resource().unwrap().same_owner(resources[1].resource().unwrap()));
        assert_eq!(resources[0].resource().unwrap().id(),lut.resource().unwrap().id());assert_eq!(resources[0].digest(),lut.digest());
    }
    #[test]
    fn ancillary_references_to_live_resources_preserve_one_identity_and_original_encoding(){
        let cancel=AtomicBool::new(false);let mut original=capture();
        let artwork=Arc::make_mut(&mut original.artwork);
        let id=PortableId::random();let bytes:Arc<[u8]>=vec![0x41;16384].into();
        let compressed:Arc<[u8]>=lz4_flex::block::compress(&bytes).into();
        let resource=Resource::with_id_and_encoded(id,bytes.clone(),EncodedBytes{encoding:ResourceEncoding::Lz4,bytes:compressed.clone()});
        artwork.metadata=Arc::new(crate::PhotoMetadata{xmp:Some(resource),..Default::default()});
        let opaque=Arc::new(OpaqueResource{id,kind:"capy.photo-metadata/1".into(),data:json!({"kind":"xmp","decoded_bytes":bytes.len().to_string()}),
            encoding:"capy.lz4-bytes/1".into(),extra_fields:Default::default(),backing:ImmutableBacking::new(Arc::new(compressed.clone())).unwrap(),
            offset:0,length:compressed.len() as u64,crc32:crc32fast::hash(&compressed)});
        let ancillary=PortableId::random();let extensions=Arc::make_mut(&mut artwork.extensions);
        extensions.resources.insert(id,opaque);
        extensions.records.insert(ancillary,json!({"id":ancillary,"type":"future.metadata/1","ancillary":true,"copy_safe":true,"data":{"payload":resources::reference(id)}}));
        let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        assert_eq!(prepared.descriptor.manifest["resources"].as_array().unwrap().iter().filter(|r|r["id"]==json!(id)).count(),1);
        let VerifiedResource::Bytes{encoded:Some(encoded),..}=&prepared.descriptor.resources[&id]else{panic!("Verified encoding must be retained")};
        assert_eq!(prepared.descriptor.retained_resources[&id],encoded.payload);
        let loaded=receive(&prepared).adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        assert_eq!(loaded.artwork.metadata.xmp.as_ref().unwrap().id(),id);
        assert_eq!(&**loaded.artwork.metadata.xmp.as_ref().unwrap(),&*bytes);
        assert_eq!(loaded.artwork.extensions.resources[&id].read_chunk(0,compressed.len(),&cancel).unwrap().to_vec(),compressed.to_vec());
        let package=PreparedPackage::prepare(&loaded,None,&cancel).unwrap();let mut saved=Vec::new();package.write(&mut saved,&cancel).unwrap();
        let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(saved))).unwrap();
        let OpenOutcome::Candidate{artwork,..}=crate::package::codec::open(backing,ProjectLimits::default(),&cancel).unwrap()else{panic!("Shared ancillary resource must remain editable")};
        assert_eq!(artwork.extensions.records[&ancillary],original.artwork.extensions.records[&ancillary]);
        assert_eq!(artwork.metadata.xmp.as_ref().unwrap().id(),id);
    }
    #[test]
    fn large_opaque_attachments_survive_worker_transfer_save_and_reopen(){
        let cancel=AtomicBool::new(false);let mut original=capture();
        let artwork=Arc::make_mut(&mut original.artwork);
        let id=PortableId::random();let bytes:Arc<[u8]>=(0..MAX_RANGE_BYTES+1).map(|i|(i%251) as u8).collect::<Vec<_>>().into();
        let chunks=super::super::transport::ChunkedBytes::new(bytes.chunks(MAX_RANGE_BYTES).map(Arc::from).collect()).unwrap();
        let resource=Arc::new(OpaqueResource{id,kind:"future.bytes/1".into(),data:json!({}),encoding:"future.raw/1".into(),extra_fields:Default::default(),
            backing:ImmutableBacking::new(Arc::new(chunks)).unwrap(),offset:0,length:bytes.len() as u64,crc32:crc32fast::hash(&bytes)});
        let ancillary=PortableId::random();let extensions=Arc::make_mut(&mut artwork.extensions);
        extensions.resources.insert(id,resource);
        extensions.records.insert(ancillary,json!({"id":ancillary,"type":"future.attachment/1","ancillary":true,"copy_safe":true,"data":{"payload":resources::reference(id)}}));
        let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let loaded=receive(&prepared).adopt_verified(ProjectLimits::default(),&cancel).unwrap();
        let package=PreparedPackage::prepare(&loaded,None,&cancel).unwrap();let mut saved=Vec::new();package.write(&mut saved,&cancel).unwrap();
        let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(saved))).unwrap();
        let OpenOutcome::Candidate{artwork,..}=crate::package::codec::open(backing,ProjectLimits::default(),&cancel).unwrap()else{panic!("Transferred attachment must reopen")};
        assert_eq!(artwork.extensions.records[&ancillary],original.artwork.extensions.records[&ancillary]);
        for (index,expected) in bytes.chunks(MAX_RANGE_BYTES).enumerate(){
            let actual=artwork.extensions.resources[&id].read_chunk((index*MAX_RANGE_BYTES) as u64,expected.len(),&cancel).unwrap();
            assert_eq!(&*actual,expected);assert!(actual.retained_bytes()<=MAX_RANGE_BYTES);
        }
    }
    #[test]
    fn receiver_rejects_unindexed_and_conflicting_payload_roles_before_allocation() {
        let prepared=PreparedTransfer::capture(&capture(),&AtomicBool::new(false)).unwrap();
        let mut descriptor=prepared.descriptor().clone();descriptor.lengths.push(1);
        assert!(pollster::block_on(TransferReceiver::new(descriptor,ProjectLimits::default())).is_err());
        let mut descriptor=prepared.descriptor().clone();
        let payload=match descriptor.resources.values().next().unwrap() {
            VerifiedResource::Bytes{payload,..}|VerifiedResource::Code{payload,..}|VerifiedResource::Tile{payload,..}|VerifiedResource::Lut{payload,..}|VerifiedResource::Vector{payload}|VerifiedResource::Opaque{payload}=>*payload,
            VerifiedResource::Coverage{payload}=>payload.unwrap(),
            VerifiedResource::TileRange {..}=>unreachable!(),
        };
        descriptor.selections.push(SelectionTransfer {extent:[8,1],bounds:[0,0,8,1],bytes:false,chunks:vec![PortableId::random()],payload});
        assert!(pollster::block_on(TransferReceiver::new(descriptor,ProjectLimits::default())).is_err());
        let mut descriptor=prepared.descriptor().clone();let id=*descriptor.resources.keys().next().unwrap();descriptor.resources.remove(&id);
        assert!(pollster::block_on(TransferReceiver::new(descriptor,ProjectLimits::default())).is_err());
    }
    #[test]
    fn transfer_rejects_incomplete_chunks_missing_verification_and_foreign_layout(){
        let cancel=AtomicBool::new(false);let original=capture();let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let receiver=pollster::block_on(TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits::default())).unwrap();assert!(receiver.finish().is_err());
        let mut missing=receive(&prepared);missing.descriptor.resources.pop_first();assert!(missing.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        let mut layout=receive(&prepared);layout.descriptor.layout.paint[0]=PortableId::random();assert!(layout.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        let mut receiver=pollster::block_on(TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits::default())).unwrap();assert!(receiver.push_chunk(0,&vec![0;MAX_RANGE_BYTES+1]).is_err());
        assert!(pollster::block_on(TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits{raster_bytes:0,asset_bytes:0,..Default::default()})).is_err());
        let mut wrong=receive(&prepared);wrong.descriptor.checkpoint.document=PortableId::random();assert!(wrong.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        cancel.store(true,Ordering::Relaxed);assert!(PreparedTransfer::capture(&original,&cancel).is_err());
    }
}
