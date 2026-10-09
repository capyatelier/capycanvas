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
    objects:Vec<PortableId>, selections: Vec<PortableId>, guides: Vec<PortableId>, outputs: Vec<PortableId>,
}
impl TransferLayout {
    pub(crate) fn capture(art: &Artwork)->Self {
        fn slots<T>(store:&Store<T>)->Vec<PortableId>{(0..store.capacity()).map(|i|store.id(Handle::from_index(i as u32)).unwrap()).collect()}
        Self {compositions:slots(&art.compositions),stacks:slots(&art.stacks),occurrences:slots(&art.occurrences),paint:slots(&art.paint),coverage:slots(&art.coverage),effects:slots(&art.effects),objects:slots(&art.objects),selections:slots(&art.selections),guides:slots(&art.guides),outputs:slots(&art.outputs)}
    }
    pub(crate) fn install(&self,art:&mut Artwork)->super::values::DecodeResult<()> {
        fn slots<T>(store:&mut Store<T>,ids:&[PortableId])->super::values::DecodeResult<()>{for id in ids {store.reserve(*id)?;}Ok(())}
        slots(&mut art.compositions,&self.compositions)?;slots(&mut art.stacks,&self.stacks)?;slots(&mut art.occurrences,&self.occurrences)?;
        slots(&mut art.paint,&self.paint)?;slots(&mut art.coverage,&self.coverage)?;slots(&mut art.effects,&self.effects)?;
        slots(&mut art.objects,&self.objects)?;slots(&mut art.selections,&self.selections)?;slots(&mut art.guides,&self.guides)?;slots(&mut art.outputs,&self.outputs)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct EncodedPayload {encoding:ResourceEncoding,payload:usize}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
enum VerifiedResource {
    Bytes {payload:usize,encoded:Option<EncodedPayload>},
    Code {payload:usize,encoded:Option<EncodedPayload>},
    Tile {payload:usize,fingerprint:Option<[u8;32]>},
    Lut {payload:usize,encoded:Option<EncodedPayload>,digest:[u8;32],spaces:u8},
    Coverage {payload:Option<usize>},
    Opaque {payload:usize},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SelectionTransfer {extent:[u32;2],bounds:[u32;4],bytes:bool,chunks:Vec<PortableId>,payload:usize}
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
    private_resources:BTreeMap<PortableId,(Value,usize)>,
}
#[derive(Clone)]
enum TransferPayload {Bytes(Arc<[u8]>),Tile(Arc<crate::raster::TileBlob>),Code(Resource<str>),Words(Arc<[u32]>),Opaque(Arc<OpaqueResource>)}
impl TransferPayload {
    fn len(&self)->u64 {match self {Self::Bytes(v)=>v.len() as u64,Self::Tile(v)=>v.compressed_len() as u64,Self::Code(v)=>v.len() as u64,Self::Words(v)=>v.len() as u64*4,Self::Opaque(v)=>v.length}}
    fn chunk(&self,offset:u64,length:usize)->Result<Vec<u8>,String>{
        if length>MAX_RANGE_BYTES||offset.checked_add(length as u64).is_none_or(|end|end>self.len()){return Err("Transfer chunk exceeds payload".into());}
        let start=usize::try_from(offset).map_err(|_|"Transfer offset overflow")?;
        Ok(match self {
            Self::Bytes(v)=>v[start..start+length].to_vec(),Self::Tile(v)=>v.compressed()?[start..start+length].to_vec(),Self::Code(v)=>v.as_bytes()[start..start+length].to_vec(),
            Self::Words(v)=>(start..start+length).map(|i|v[i/4].to_le_bytes()[i%4]).collect(),
            Self::Opaque(v)=>v.read_chunk(offset,length,&AtomicBool::new(false))?.to_vec(),
        })
    }
}
pub struct PreparedTransfer {descriptor:TransferDescriptor,payloads:Vec<TransferPayload>}
fn active(cancel:&AtomicBool)->Result<(),String>{if cancel.load(Ordering::Relaxed){Err("Transfer cancelled".into())}else{Ok(())}}
fn manifest(descriptor:&TransferDescriptor,limits:ProjectLimits)->Result<Manifest,String>{
    if crate::json_len(descriptor)>limits.metadata_bytes as usize {return Err("Transfer metadata exceeds admission".into());}
    match Manifest::transfer(&descriptor.manifest,ManifestLimits {metadata_bytes:limits.metadata_bytes.min(usize::MAX as u64) as usize,..Default::default()})? {
        ManifestRead::Known(manifest) if matches!(manifest.support,Support::Editable)=>Ok(manifest),
        _=>Err("Unsupported artwork transfer".into()),
    }
}
impl PreparedTransfer {
    pub fn capture(capture:&ArtworkCapture,cancel:&AtomicBool)->Result<Self,String>{
        Self::capture_with_selection(capture,&None,cancel)
    }
    pub fn capture_with_selection(capture:&ArtworkCapture,selection:&Option<crate::Selection>,cancel:&AtomicBool)->Result<Self,String>{
        Self::capture_with_additional(capture,selection,ResourceInventory::for_transfer(),BTreeMap::new(),cancel)
    }
    pub(crate) fn capture_with_additional(capture:&ArtworkCapture,selection:&Option<crate::Selection>,inventory:ResourceInventory,mut additional_luts:BTreeMap<PortableId,Arc<Lut3d>>,cancel:&AtomicBool)->Result<Self,String>{
        active(cancel)?;let art=&capture.artwork;
        if capture.checkpoint.document!=art.id{return Err("Capture belongs to another artwork".into());}
        if art.paint.iter().any(|(_,_,s)|!s.operations.is_empty())||art.coverage.iter().any(|(_,_,s)|!s.operations.is_empty()){return Err("Wait for the current edit before transferring".into());}
        let (mut objects,mut inventory)=artwork_records::encode_with_inventory(art,cancel,inventory)?;
        let working_selection=selection.as_ref().map(|selection|super::selection_records::encode_selection(selection,&mut inventory)).transpose()?;
        let live=objects.iter().map(|r|r["id"].as_str().ok_or("Missing object ID")?.parse().map_err(str::to_string)).collect::<Result<BTreeSet<_>,String>>()?;
        let (extensions,opaque)=art.extensions.edited_retained(&live)?;objects.extend(extensions);
        let mut roots=crate::RootInventory::default();roots.artwork(art);
        additional_luts.extend(roots.resources.into_iter().filter_map(|lut|lut.resource().map(|r|(r.id(),lut.clone()))));let luts=additional_luts;
        let mut payloads=Vec::new();let mut verification=BTreeMap::new();let mut records=Vec::new();
        fn push(payloads:&mut Vec<TransferPayload>,payload:TransferPayload)->usize{let id=payloads.len();payloads.push(payload);id}
        fn encoded(payloads:&mut Vec<TransferPayload>,bytes:Option<&EncodedBytes>)->Option<EncodedPayload>{bytes.map(|v|EncodedPayload{encoding:v.encoding,payload:push(payloads,TransferPayload::Bytes(v.bytes.clone()))})}
        for (id,entry) in &inventory.entries {
            active(cancel)?;
            let state=match &entry.payload {
                Payload::Tile(tile)=>VerifiedResource::Tile{payload:push(&mut payloads,TransferPayload::Tile(tile.clone())),fingerprint:tile.encoded_fingerprint()},
                Payload::Bytes(bytes) if entry.kind=="capy.lut3d/1"=>{let lut=luts.get(id).ok_or("Missing verified lookup owner")?;
                    VerifiedResource::Lut {payload:push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())),encoded:encoded(&mut payloads,bytes.encoded_if_ready()),digest:lut.digest(),spaces:lut.admitted_spaces()}},
                Payload::Bytes(bytes)=>VerifiedResource::Bytes{payload:push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())),encoded:encoded(&mut payloads,bytes.encoded_if_ready())},
                Payload::Code(code)=>VerifiedResource::Code{payload:push(&mut payloads,TransferPayload::Code(code.clone())),encoded:encoded(&mut payloads,code.encoded_if_ready())},
                Payload::Encoded(bytes)=>VerifiedResource::Coverage{payload:(!bytes.is_empty()).then(||push(&mut payloads,TransferPayload::Bytes(bytes.storage().clone())))},
                Payload::Opaque(resource)=>VerifiedResource::Opaque{payload:push(&mut payloads,TransferPayload::Opaque(resource.clone()))},
            };
            let length=match &state {
                VerifiedResource::Coverage {payload:None}=>0,
                VerifiedResource::Coverage {payload:Some(payload)}|VerifiedResource::Bytes{payload,..}|VerifiedResource::Code{payload,..}|VerifiedResource::Tile{payload,..}|VerifiedResource::Lut{payload,..}|VerifiedResource::Opaque{payload}=>payloads[*payload].len(),
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
                        if resource.encoding.as_ref()=="capy.lz4-bytes/1" {encoded.as_ref().filter(|e|e.encoding==ResourceEncoding::Lz4).map(|e|e.payload)}else{Some(*payload)}
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
        let descriptor=TransferDescriptor {manifest:value,layout:TransferLayout::capture(art),checkpoint:capture.checkpoint,working_selection,lengths:payloads.iter().map(TransferPayload::len).collect(),resources:verification,selections,retained_resources,private_resources:BTreeMap::new()};
        manifest(&descriptor,ProjectLimits::default())?;
        Ok(Self {descriptor,payloads})
    }
    pub(crate) fn append_private_resource(&mut self,resource:Arc<OpaqueResource>)->Result<(),String>{
        if self.descriptor.private_resources.contains_key(&resource.id){return Err("Duplicate private transfer resource".into());}
        let index=self.payloads.len();let record=resource.record(json!({"member":format!("private/{}",resource.id)}));
        self.descriptor.lengths.push(resource.length);self.descriptor.private_resources.insert(resource.id,(record,index));
        self.payloads.push(TransferPayload::Opaque(resource));Ok(())
    }
    pub(crate) fn private_resource(&self,id:PortableId)->Result<Arc<OpaqueResource>,String>{
        let (record,index)=self.descriptor.private_resources.get(&id).ok_or("Missing private transfer resource")?;
        if let Some(TransferPayload::Opaque(resource))=self.payloads.get(*index){return Ok(resource.clone());}
        let bytes=self.bytes(*index)?;let length=bytes.len() as u64;
        if super::manifest::decimal_u64(&record["bytes"]).map_err(|e|e.to_string())?!=length{return Err("Private resource length mismatch".into());}
        let crc32=u32::from_str_radix(record["crc32"].as_str().ok_or("Missing private resource checksum")?,16).map_err(|e|e.to_string())?;
        let mut extra=record.as_object().ok_or("Invalid private resource record")?.clone();for key in ["id","type","data","encoding","location","bytes","crc32"]{extra.remove(key);}
        let backing=ImmutableBacking::new(Arc::new(bytes)).map_err(str::to_string)?;
        Ok(Arc::new(OpaqueResource {id,kind:record["type"].as_str().ok_or("Missing private resource type")?.into(),data:record["data"].clone(),
            encoding:record["encoding"].as_str().ok_or("Missing private resource encoding")?.into(),extra_fields:extra,backing,offset:0,length,crc32}))
    }
    fn payload_keys(&self)->Result<BTreeMap<Vec<u8>,usize>,String> {
        let mut keys=BTreeMap::new();
        let records=self.descriptor.manifest["resources"].as_array().ok_or("Missing transfer resources")?.iter().map(|record|{
            let id:PortableId=record["id"].as_str().ok_or("Missing transfer resource identity")?.parse().map_err(str::to_string)?;Ok((id,record))
        }).collect::<Result<BTreeMap<_,_>,String>>()?;
        let mut add=|id:PortableId,role:&str,payload:usize,extra:Value|->Result<(),String>{
            let mut record=(*records.get(&id).ok_or("Missing transfer resource descriptor")?).clone();
            record.as_object_mut().ok_or("Invalid transfer resource descriptor")?.remove("location");
            let key=serde_json::to_vec(&json!({"resource":record,"role":role,"extra":extra,"bytes":*self.descriptor.lengths.get(payload).ok_or("Missing transfer payload length")?})).map_err(|e|e.to_string())?;
            if keys.insert(key,payload).is_some(){return Err("Ambiguous transfer payload identity".into());}Ok(())
        };
        for (id,state) in &self.descriptor.resources {match state {
            VerifiedResource::Bytes{payload,encoded}|VerifiedResource::Code{payload,encoded}=>{add(*id,"raw",*payload,Value::Null)?;if let Some(encoded)=encoded {add(*id,"encoded",encoded.payload,json!(encoded.encoding))?;}},
            VerifiedResource::Lut{payload,encoded,digest,spaces}=>{add(*id,"raw",*payload,json!([digest,spaces]))?;if let Some(encoded)=encoded {add(*id,"encoded",encoded.payload,json!([encoded.encoding,digest,spaces]))?;}},
            VerifiedResource::Tile{payload,fingerprint}=>add(*id,"raw",*payload,json!(fingerprint))?,
            VerifiedResource::Coverage{payload:Some(payload)}|VerifiedResource::Opaque{payload}=>add(*id,"raw",*payload,Value::Null)?,
            VerifiedResource::Coverage{payload:None}=>(),
        }}
        for (id,payload) in &self.descriptor.retained_resources {add(*id,"retained",*payload,Value::Null)?;}
        for (id,(record,payload)) in &self.descriptor.private_resources {
            let mut record=record.clone();record.as_object_mut().ok_or("Invalid private resource descriptor")?.remove("location");
            let key=serde_json::to_vec(&json!({"private":id,"record":record,"length":self.descriptor.lengths.get(*payload).ok_or("Missing private resource payload")?})).map_err(|e|e.to_string())?;
            if keys.insert(key,*payload).is_some(){return Err("Ambiguous private payload identity".into());}
        }
        for selection in &self.descriptor.selections {
            let key=serde_json::to_vec(&json!({"selection":selection.chunks,"extent":selection.extent,"bounds":selection.bounds,"bytes":selection.bytes,"length":*self.descriptor.lengths.get(selection.payload).ok_or("Missing selection payload length")?})).map_err(|e|e.to_string())?;
            if keys.insert(key,selection.payload).is_some(){return Err("Ambiguous selection payload identity".into());}
        }
        Ok(keys)
    }
    pub fn descriptor(&self)->&TransferDescriptor{&self.descriptor}
    pub fn payload_count(&self)->usize{self.payloads.len()}
    pub fn payload_len(&self,index:usize)->Result<u64,String>{self.payloads.get(index).map(TransferPayload::len).ok_or_else(||"Missing transfer payload".into())}
    pub fn read_chunk(&self,index:usize,offset:u64,length:usize)->Result<Vec<u8>,String>{self.payloads.get(index).ok_or("Missing transfer payload")?.chunk(offset,length)}
    pub fn from_chunks(descriptor:TransferDescriptor,chunks:Vec<Vec<Arc<[u8]>>>)->Result<Self,String>{
        let mut receiver=TransferReceiver::new(descriptor,ProjectLimits::default())?;
        if chunks.len()!=receiver.buffers.len(){return Err("Transfer payload count mismatch".into());}
        for (index,chunks) in chunks.into_iter().enumerate(){for chunk in chunks{receiver.push_chunk(index,&chunk)?;}}
        receiver.finish()
    }
    fn bytes(&self,index:usize)->Result<Arc<[u8]>,String>{match self.payloads.get(index){Some(TransferPayload::Bytes(v))=>Ok(v.clone()),Some(TransferPayload::Tile(v))=>v.compressed(),Some(TransferPayload::Code(v))=>Ok(v.as_bytes().into()),_=>Err("Wrong transfer byte payload".into())}}
    fn encoded(&self,payload:&Option<EncodedPayload>)->Result<Option<EncodedBytes>,String>{payload.as_ref().map(|v|Ok(EncodedBytes{encoding:v.encoding,bytes:self.bytes(v.payload)?})).transpose()}
    pub fn adopt_verified(&self,limits:ProjectLimits,cancel:&AtomicBool)->Result<ArtworkCapture,String>{
        self.adopt_verified_with_selection(limits,cancel).map(|(capture,_)|capture)
    }
    pub fn adopt_verified_with_selection(&self,limits:ProjectLimits,cancel:&AtomicBool)->Result<(ArtworkCapture,Option<crate::Selection>),String>{
        self.adopt_verified_with_retention(limits,cancel,false,|_,_|Ok(())).map(|(capture,selection,())|(capture,selection))
    }
    pub(crate) fn adopt_verified_with<T>(&self,limits:ProjectLimits,cancel:&AtomicBool,additional:impl FnOnce(&Artwork,&mut ResourceReader<'_>)->Result<T,String>)->Result<(ArtworkCapture,Option<crate::Selection>,T),String>{
        self.adopt_verified_with_retention(limits,cancel,true,additional)
    }
    fn adopt_verified_with_retention<T>(&self,limits:ProjectLimits,cancel:&AtomicBool,retained:bool,additional:impl FnOnce(&Artwork,&mut ResourceReader<'_>)->Result<T,String>)->Result<(ArtworkCapture,Option<crate::Selection>,T),String>{
        active(cancel)?;let manifest=manifest(&self.descriptor,limits)?;
        let empty:Arc<[u8]>=Arc::from([]);let backing=ImmutableBacking::new(Arc::new(empty)).map_err(str::to_string)?;
        let mut reader=ResourceReader::new(&manifest,&backing,cancel,limits);reader.private=true;reader.require_verified();
        if manifest.resources.len()!=self.descriptor.resources.len()||self.descriptor.checkpoint.document!=manifest.document{return Err("Transfer identity inventory mismatch".into());}
        let mut decoded=0u64;
        for (id,state) in &self.descriptor.resources {
            active(cancel)?;let record=&manifest.resources.get(id).ok_or("Unknown transfer resource")?.value;
            let kind=record["type"].as_str().ok_or("Missing transfer resource type")?;
            let resource=|index,encoded:&Option<EncodedPayload>|->Result<Resource<[u8]>,String>{let bytes=self.bytes(index)?;Ok(match self.encoded(encoded)?{Some(encoded)=>Resource::with_id_and_encoded(*id,bytes,encoded),None=>Resource::with_id(*id,bytes)})};
            match state {
                VerifiedResource::Bytes{payload,encoded}=>{if !matches!(kind,"capy.icc/1"|"capy.photo-metadata/1"){return Err("Transfer byte resource type mismatch".into());}let value=resource(*payload,encoded)?;let limit=if kind=="capy.icc/1"{crate::color::source::MAX_PROFILE_BYTES}else{crate::PhotoMetadata::MAX_BYTES};if value.len()>limit{return Err("Oversized verified byte resource".into());}decoded=decoded.saturating_add(value.len() as u64);reader.bytes.insert(*id,value);},
                VerifiedResource::Code{payload,encoded}=>{if kind!="capy.wgsl/1"{return Err("Transfer shader type mismatch".into());}let code=match self.payloads.get(*payload){Some(TransferPayload::Code(code))=>code.clone(),_=>{let bytes=self.bytes(*payload)?;let text:Arc<str>=std::str::from_utf8(&bytes).map_err(|_|"Invalid verified shader text")?.into();match self.encoded(encoded)?{Some(encoded)=>Resource::with_id_and_encoded(*id,text,encoded),None=>Resource::with_id(*id,text)}}};if code.len()>4*1024*1024{return Err("Oversized transfer shader".into());}decoded=decoded.saturating_add(code.len() as u64);reader.texts.insert(*id,code);},
                VerifiedResource::Lut{payload,encoded,digest,spaces}=>{if kind!="capy.lut3d/1"{return Err("Transfer lookup type mismatch".into());}let data=&record["data"];let size=super::values::u32_value(&data["size"]).map_err(|e|e.to_string())?;let domain: [[f32;3];2]=serde_json::from_value(data["domain"].clone()).map_err(|e|e.to_string())?;let bytes=resource(*payload,encoded)?;decoded=decoded.saturating_add(bytes.len() as u64);reader.bytes.insert(*id,bytes.clone());reader.luts.insert(*id,Arc::new(Lut3d::from_verified_resource(size,domain,Arc::from(""),*digest,bytes,*spaces).map_err(str::to_string)?));},
                VerifiedResource::Coverage{payload}=>{if kind!="capy.selection-coverage/1"{return Err("Transfer selection type mismatch".into());}let bytes=payload.map(|p|self.bytes(p)).transpose()?.unwrap_or_else(||Arc::from([]));reader.bytes.insert(*id,Resource::with_id(*id,bytes));},
                VerifiedResource::Tile{..}|VerifiedResource::Opaque{..}=>(),
            }
        }
        for (id,state) in &self.descriptor.resources {if let VerifiedResource::Tile{payload,fingerprint}=state {
            let record=&manifest.resources[id].value;if record["type"]!="capy.raster-tile/1"{return Err("Transfer tile type mismatch".into());}
            let descriptor=resources::parse_descriptor(&record["data"]).map_err(|e|e.to_string())?;
            let raw=descriptor.byte_len([super::RASTER_TILE_SIZE;2]).ok_or("Invalid verified tile descriptor")? as u64;
            decoded=decoded.saturating_add(if retained {self.payload_len(*payload)?}else{raw});
            reader.tiles.insert(*id,Arc::new(crate::raster::TileBlob::from_verified_resource_with_encoded_fingerprint(*id,descriptor,self.bytes(*payload)?,*fingerprint)?));
        }}
        for selection in &self.descriptor.selections {
            let words=match self.payloads.get(selection.payload){Some(TransferPayload::Words(words))=>words.clone(),_=>return Err("Missing decoded selection words".into())};decoded=decoded.saturating_add(words.len() as u64*4);
            let chunks=selection.chunks.iter().map(|id|reader.bytes.get(id).cloned().ok_or("Missing selection chunk identity")).collect::<Result<Vec<_>,_>>()?;
            let cached=chunks.iter().all(|c|!c.is_empty()).then_some(chunks);
            let pixels=Arc::new(SelectionPixels::from_verified_words(selection.extent,selection.bounds,selection.bytes,words,selection.chunks.clone(),cached)?);
            reader.selections.insert((selection.extent,selection.bounds,selection.bytes,selection.chunks.clone()),pixels);
        }
        if decoded>limits.raster_bytes.saturating_add(if retained {crate::history_budget::BYTE_BUDGET as u64}else{0}){return Err("Transfer resources exceed decoded memory admission".into());}
        let mut artwork=artwork_records::decode_with_layout(&manifest,&mut reader,Some(&self.descriptor.layout)).map_err(|e|e.to_string())?;
        let working_selection=self.descriptor.working_selection.as_ref().map(|value|super::selection_records::decode_selection(value,&mut reader)).transpose().map_err(|e|e.to_string())?;
        let mut extensions=Extensions {records:manifest.objects.iter().filter(|(_,r)|r["ancillary"]==true).map(|(id,r)|(*id,r.clone())).collect(),..Default::default()};
        let opaque=self.descriptor.resources.iter().filter_map(|(id,state)|if let VerifiedResource::Opaque{payload}=state{Some((*id,*payload))}else{None})
            .chain(self.descriptor.retained_resources.iter().map(|(id,payload)|(*id,*payload)));
        for (id,payload) in opaque {
            let record=manifest.resources.get(&id).ok_or("Missing retained transfer resource")?;
            if self.payloads.get(payload).ok_or("Missing retained payload")?.len()!=record.bytes{return Err("Retained resource length mismatch".into());}
            if let Some(TransferPayload::Opaque(resource))=self.payloads.get(payload){extensions.resources.insert(id,resource.clone());continue;}
            let bytes=self.bytes(payload)?;let length=bytes.len() as u64;let backing=ImmutableBacking::new(Arc::new(bytes)).map_err(str::to_string)?;
            let mut extra=record.value.as_object().ok_or("Invalid opaque transfer record")?.clone();for key in ["id","type","data","encoding","location","bytes","crc32"]{extra.remove(key);}
            extensions.resources.insert(id,Arc::new(OpaqueResource{id,kind:record.value["type"].as_str().ok_or("Missing opaque type")?.into(),data:record.value["data"].clone(),encoding:record.value["encoding"].as_str().ok_or("Missing opaque encoding")?.into(),extra_fields:extra,backing,offset:0,length,crc32:record.crc32}));
        }
        artwork.extensions=Arc::new(extensions);
        crate::Document::from_artwork(artwork.clone()).map_err(|e|e.to_string())?.validate(limits)?;
        let extra=additional(&artwork,&mut reader)?;
        active(cancel)?;
        Ok((ArtworkCapture{artwork:Arc::new(artwork),checkpoint:self.descriptor.checkpoint},working_selection,extra))
    }
}
enum PayloadBuffer {Bytes(Arc<[MaybeUninit<u8>]>),Words(Arc<[MaybeUninit<u32>]>),Ready(TransferPayload) }
pub struct TransferReceiver {descriptor:TransferDescriptor,buffers:Vec<PayloadBuffer>,positions:Vec<u64>}
impl TransferReceiver {
    pub fn new(descriptor:TransferDescriptor,limits:ProjectLimits)->Result<Self,String>{
        Self::new_with_reuse(descriptor,limits,None)
    }
    pub(crate) fn new_reusing(descriptor:TransferDescriptor,limits:ProjectLimits,previous:&PreparedTransfer)->Result<Self,String>{
        Self::new_with_reuse(descriptor,limits,Some(previous))
    }
    fn new_with_reuse(descriptor:TransferDescriptor,limits:ProjectLimits,previous:Option<&PreparedTransfer>)->Result<Self,String>{
        let parsed=manifest(&descriptor,limits)?;
        if !parsed.resources.keys().eq(descriptor.resources.keys()) || parsed.document!=descriptor.checkpoint.document {return Err("Transfer identity inventory mismatch".into());}
        let mut bytes=BTreeSet::new();let mut words=BTreeSet::new();
        for resource in descriptor.resources.values() {match resource {
            VerifiedResource::Bytes{payload,encoded}|VerifiedResource::Code{payload,encoded}|VerifiedResource::Lut{payload,encoded,..}=>{bytes.insert(*payload);if let Some(encoded)=encoded {bytes.insert(encoded.payload);}},
            VerifiedResource::Tile{payload,..}|VerifiedResource::Opaque{payload}|VerifiedResource::Coverage{payload:Some(payload)}=>{bytes.insert(*payload);},
            VerifiedResource::Coverage{payload:None}=>(),
        }}
        bytes.extend(descriptor.retained_resources.values().copied());bytes.extend(descriptor.private_resources.values().map(|(_,index)|*index));
        words.extend(descriptor.selections.iter().map(|selection|selection.payload));
        if !bytes.is_disjoint(&words)||bytes.union(&words).count()!=descriptor.lengths.len()
            ||bytes.union(&words).any(|index|*index>=descriptor.lengths.len()) {return Err("Unindexed or conflicting transfer payloads".into());}
        let mut reuse=BTreeMap::new();
        if let Some(previous)=previous {
            let candidate=PreparedTransfer {descriptor:descriptor.clone(),payloads:Vec::new()};
            let keys=candidate.payload_keys()?;let old=previous.payload_keys()?;
            for (key,index) in keys {if let Some(before)=old.get(&key) {reuse.insert(index,previous.payloads[*before].clone());}}
        }
        for (id,(record,payload)) in &descriptor.private_resources {
            if record["id"]!=json!(id)||super::manifest::decimal_u64(&record["bytes"]).map_err(|e|e.to_string())?!=*descriptor.lengths.get(*payload).ok_or("Missing private resource payload")? {return Err("Private transfer inventory mismatch".into());}
        }
        if descriptor.retained_resources.keys().any(|id|!descriptor.resources.contains_key(id)){return Err("Unknown retained transfer resource".into());}
        let total=descriptor.lengths.iter().try_fold(0u64,|a,b|a.checked_add(*b).ok_or("Transfer length overflow"))?;
        if total>limits.raster_bytes.saturating_add(limits.asset_bytes).saturating_mul(2){return Err("Transfer exceeds memory admission".into());}
        let buffers=descriptor.lengths.iter().enumerate().map(|(i,length)|{
            if let Some(payload)=reuse.remove(&i) {return Ok(PayloadBuffer::Ready(payload));}
            let length=usize::try_from(*length).map_err(|_|"Transfer allocation overflow")?;
            if words.contains(&i){if length%4!=0{return Err("Misaligned selection words".into());}Ok(PayloadBuffer::Words(Arc::<[u32]>::new_uninit_slice(length/4)))}
            else{Ok(PayloadBuffer::Bytes(Arc::<[u8]>::new_uninit_slice(length)))}
        }).collect::<Result<Vec<_>,String>>()?;
        let positions=buffers.iter().enumerate().map(|(i,buffer)|if matches!(buffer,PayloadBuffer::Ready(_)){descriptor.lengths[i]}else{0}).collect();Ok(Self{descriptor,buffers,positions})
    }
    pub fn missing_payloads(&self)->Vec<usize>{self.positions.iter().zip(&self.descriptor.lengths).enumerate().filter_map(|(i,(position,length))|(position!=length).then_some(i)).collect()}
    pub fn push_chunk(&mut self,index:usize,bytes:&[u8])->Result<(),String>{
        let position=*self.positions.get(index).ok_or("Missing transfer payload")?;
        if bytes.len()>MAX_RANGE_BYTES||position.checked_add(bytes.len() as u64).is_none_or(|n|n>self.descriptor.lengths[index]){return Err("Transfer chunk exceeds admission".into());}
        match &mut self.buffers[index] {
            PayloadBuffer::Ready(_)=>return Err("Transfer payload was already reused".into()),
            PayloadBuffer::Bytes(buffer)=>{let start=position as usize;for (out,input) in Arc::get_mut(buffer).unwrap()[start..start+bytes.len()].iter_mut().zip(bytes){out.write(*input);}},
            PayloadBuffer::Words(buffer)=>{if !position.is_multiple_of(4)||!bytes.len().is_multiple_of(4){return Err("Misaligned selection transfer chunk".into());}let start=position as usize/4;for (out,input) in Arc::get_mut(buffer).unwrap()[start..start+bytes.len()/4].iter_mut().zip(bytes.as_chunks::<4>().0){out.write(u32::from_le_bytes(*input));}},
        }
        self.positions[index]+=bytes.len() as u64;Ok(())
    }
    pub fn finish(self)->Result<PreparedTransfer,String>{
        if self.positions!=self.descriptor.lengths{return Err("Incomplete transfer payloads".into());}
        let payloads=self.buffers.into_iter().map(|buffer|match buffer {
            PayloadBuffer::Ready(payload)=>payload,
            PayloadBuffer::Bytes(bytes)=>TransferPayload::Bytes(unsafe{bytes.assume_init()}),
            PayloadBuffer::Words(words)=>TransferPayload::Words(unsafe{words.assume_init()}),
        }).collect();
        Ok(PreparedTransfer{descriptor:self.descriptor,payloads})
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
    fn receive(prepared:&PreparedTransfer)->PreparedTransfer {
        let descriptor=serde_json::from_slice(&serde_json::to_vec(prepared.descriptor()).unwrap()).unwrap();
        let mut receiver=TransferReceiver::new(descriptor,ProjectLimits::default()).unwrap();
        for index in 0..prepared.payload_count(){let mut offset=0;let length=prepared.payload_len(index).unwrap();while offset<length {
            let count=(length-offset).min(MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index,&prepared.read_chunk(index,offset,count).unwrap()).unwrap();offset+=count as u64;
        }}receiver.finish().unwrap()
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
                assert_eq!(tile.compressed().unwrap(),restored.compressed().unwrap());
            }
        }
        assert_eq!(original.artwork.metadata,reopened.artwork.metadata);
        assert_eq!(original.artwork.outputs.get(original.artwork.default_output),reopened.artwork.outputs.get(reopened.artwork.default_output));
        let roundtrip=PreparedTransfer::capture(&reopened,&cancel).unwrap();assert_eq!(prepared.descriptor.manifest,roundtrip.descriptor.manifest);
        assert!(prepared.read_chunk(0,0,MAX_RANGE_BYTES+1).is_err());
    }
    #[test]
    fn captured_transfer_defers_cold_tile_reads_and_retains_spilled_backing() {
        use crate::raster_storage::{TileChunk, prepare_external_spill};
        use std::sync::atomic::AtomicUsize;
        struct Chunk { bytes:Arc<[u8]>, ready:Arc<AtomicBool>, reads:Arc<AtomicUsize> }
        impl TileChunk for Chunk {
            fn len(&self)->usize {self.bytes.len()}
            fn poll(&self)->Result<Option<Arc<[u8]>>,String> {
                self.reads.fetch_add(1,Ordering::Relaxed);
                Ok(self.ready.load(Ordering::Relaxed).then(||self.bytes.clone()))
            }
            fn resident_bytes(&self)->usize {if self.ready.load(Ordering::Relaxed){self.bytes.len()}else{0}}
            fn evict(&self) {self.ready.store(false,Ordering::Relaxed);}
        }
        let cancel=AtomicBool::new(false);
        let original=capture();
        let editor=Editor::new(Document::from_artwork((*original.artwork).clone()).unwrap());
        let retained=editor.retained_tiles();
        let blobs=retained.try_blobs().unwrap().unwrap();
        let tile=blobs[0].clone();
        let id=tile.resource_id();
        let expected=tile.compressed().unwrap();
        let weak_tile=Arc::downgrade(&tile);
        drop(tile);drop(blobs);
        let spill=prepare_external_spill(&retained).unwrap().unwrap();
        let ready=Arc::new(AtomicBool::new(false));
        let reads=Arc::new(AtomicUsize::new(0));
        let chunk=Arc::new(Chunk {bytes:spill.bytes.clone().into(),ready:ready.clone(),reads:reads.clone()});
        let weak_chunk=Arc::downgrade(&chunk);
        spill.commit(chunk.clone()).unwrap();
        let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let VerifiedResource::Tile {payload,..} = prepared.descriptor.resources[&id] else {panic!("tile payload")};
        assert_eq!(prepared.payload_len(payload).unwrap(),expected.len() as u64);
        assert_eq!(reads.load(Ordering::Relaxed),0,"capture and length enumeration must not poll cold backing");
        drop(original);drop(editor);drop(retained);drop(chunk);
        assert!(weak_tile.upgrade().is_some());assert!(weak_chunk.upgrade().is_some());
        assert!(prepared.read_chunk(payload,u64::MAX,1).is_err());
        assert!(prepared.read_chunk(payload,0,MAX_RANGE_BYTES+1).is_err());
        assert_eq!(reads.load(Ordering::Relaxed),0,"invalid ranges must fail before reading");
        assert!(prepared.read_chunk(payload,0,expected.len().min(37)).is_err());
        assert_eq!(reads.load(Ordering::Relaxed),1,"only a requested chunk polls backing");
        ready.store(true,Ordering::Relaxed);
        let mut copied=Vec::new();
        for offset in (0..expected.len()).step_by(37) {
            copied.extend(prepared.read_chunk(payload,offset as u64,(expected.len()-offset).min(37)).unwrap());
        }
        assert_eq!(copied.as_slice(),expected.as_ref());
        for transfer in [&prepared,&receive(&prepared)] {
            let loaded=transfer.adopt_verified(ProjectLimits::default(),&cancel).unwrap();
            let restored=loaded.artwork.paint.iter().next().unwrap().2.base.as_ref().unwrap().image.tiles.values().next().unwrap();
            assert_eq!(restored.resource_id(),id);
            assert_eq!(restored.compressed().unwrap(),expected);
        }
        drop(prepared);
        assert!(weak_tile.upgrade().is_none());assert!(weak_chunk.upgrade().is_none());
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
        assert!(TransferReceiver::new(descriptor,ProjectLimits::default()).is_err());
        let mut descriptor=prepared.descriptor().clone();
        let payload=match descriptor.resources.values().next().unwrap() {
            VerifiedResource::Bytes{payload,..}|VerifiedResource::Code{payload,..}|VerifiedResource::Tile{payload,..}|VerifiedResource::Lut{payload,..}|VerifiedResource::Opaque{payload}=>*payload,
            VerifiedResource::Coverage{payload}=>payload.unwrap(),
        };
        descriptor.selections.push(SelectionTransfer {extent:[8,1],bounds:[0,0,8,1],bytes:false,chunks:vec![PortableId::random()],payload});
        assert!(TransferReceiver::new(descriptor,ProjectLimits::default()).is_err());
        let mut descriptor=prepared.descriptor().clone();let id=*descriptor.resources.keys().next().unwrap();descriptor.resources.remove(&id);
        assert!(TransferReceiver::new(descriptor,ProjectLimits::default()).is_err());
    }
    #[test]
    fn transfer_rejects_incomplete_chunks_missing_verification_and_foreign_layout(){
        let cancel=AtomicBool::new(false);let original=capture();let prepared=PreparedTransfer::capture(&original,&cancel).unwrap();
        let receiver=TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits::default()).unwrap();assert!(receiver.finish().is_err());
        let mut missing=receive(&prepared);missing.descriptor.resources.pop_first();assert!(missing.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        let mut layout=receive(&prepared);layout.descriptor.layout.paint[0]=PortableId::random();assert!(layout.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        let mut receiver=TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits::default()).unwrap();assert!(receiver.push_chunk(0,&vec![0;MAX_RANGE_BYTES+1]).is_err());
        assert!(TransferReceiver::new(prepared.descriptor.clone(),ProjectLimits{raster_bytes:0,asset_bytes:0,..Default::default()}).is_err());
        let mut wrong=receive(&prepared);wrong.descriptor.checkpoint.document=PortableId::random();assert!(wrong.adopt_verified(ProjectLimits::default(),&cancel).is_err());
        cancel.store(true,Ordering::Relaxed);assert!(PreparedTransfer::capture(&original,&cancel).is_err());
    }
}
