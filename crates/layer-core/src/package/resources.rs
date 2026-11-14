#[cfg(test)]
use super::RangeState;
use super::{ImmutableBacking, MAX_RANGE_BYTES, manifest::{Manifest, decimal_u64}, values::{self, DecodeError, DecodeResult}};
use super::RASTER_TILE_SIZE as TILE_SIZE;
use crate::{authored::{OpaqueResource, PortableId, Resource, ResourceEncoding, EncodedBytes, EncodedIntegrity, vector::{Slot, VectorChunk, VectorObject}}, color::{PixelDescriptor, SampleType, TransferEncoding, AlphaAssociation, ColorProfile}, raster::{TileBlob}, Lut3d};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub(crate) const WRAPPERS: [&str; 2] = ["capy.lz4-bytes/1", "capy.deflate-bytes/1"];
pub(crate) fn stored_decode_bound(record:&Value)->DecodeResult<Option<usize>> {
    if !super::manifest::resource_supported(record)? {return Ok(None);}
    let limit=match record["type"].as_str() {
        Some("capy.icc/1")=>crate::color::source::MAX_PROFILE_BYTES,
        Some("capy.wgsl/1")=>4*1024*1024,
        Some("capy.lut3d/1")=>(Lut3d::MAX_SIZE as usize).pow(3)*12,
        Some("capy.vector-items/1")=>super::vector_records::MAX_CHUNK_BYTES,
        Some("capy.photo-metadata/1")=>crate::PhotoMetadata::MAX_BYTES,
        Some("capy.selection-coverage/1")=>65536,
        Some("capy.raster-tile/1")=>parse_descriptor(&record["data"])?.byte_len([TILE_SIZE;2]).ok_or("Invalid tile descriptor")?,
        _=>return Ok(None),
    };
    let encoded=decimal_u64(&record["bytes"])?;
    let decoded=if WRAPPERS.contains(&record["encoding"].as_str().unwrap()) {decimal_u64(&record["data"]["decoded_bytes"])?}else {encoded};
    let tile_or_coverage=matches!(record["type"].as_str(),Some("capy.raster-tile/1"|"capy.selection-coverage/1"));
    if !tile_or_coverage && decoded>limit as u64 {return Err(DecodeError::Unsupported("Resource exceeds decoded size admission".into()));}
    if encoded>lz4_flex::block::get_maximum_output_size(limit) as u64 {return Err(DecodeError::Unsupported("Resource exceeds encoded size bound".into()));}
    Ok(Some(limit))
}
pub fn reference(id: PortableId) -> Value { json!({"ref":id}) }
pub fn reference_id(value: &Value) -> DecodeResult<PortableId> {
    values::string(values::required(values::object(value, &["ref"])?, "ref")?)?.parse().map_err(DecodeError::from)
}
fn encode_vector(chunk: &Resource<VectorChunk>) -> &EncodedBytes {
    chunk.encoded(|data| { let raw: Arc<[u8]> = super::vector_records::encode_chunk(data).into(); compress(&raw, || raw.clone()) })
}
fn compress(bytes: &[u8], raw: impl FnOnce() -> Arc<[u8]>) -> EncodedBytes {
    let compressed = lz4_flex::block::compress(bytes);
    if compressed.len() < bytes.len() { EncodedBytes { encoding: ResourceEncoding::Lz4, bytes: compressed.into() } }
    else { EncodedBytes { encoding: ResourceEncoding::Raw, bytes: raw() } }
}

#[derive(Clone, Debug)]
pub enum Payload { Tile(Arc<TileBlob>), Bytes(Resource<[u8]>), Code(Resource<str>), Encoded(Resource<[u8]>), Vector(Resource<VectorChunk>), Opaque(Arc<crate::authored::OpaqueResource>) }
impl Payload {
    pub fn id(&self) -> PortableId { match self { Self::Tile(tile) => tile.resource_id(), Self::Bytes(bytes) | Self::Encoded(bytes) => bytes.id(), Self::Code(code) => code.id(), Self::Vector(chunk) => chunk.id(), Self::Opaque(resource) => resource.id } }
    pub fn same_owner(&self, other: &Self) -> bool { match (self, other) {
        (Self::Tile(a),Self::Tile(b)) => Arc::ptr_eq(&a.compressed,&b.compressed), (Self::Bytes(a),Self::Bytes(b)) | (Self::Encoded(a),Self::Encoded(b)) => a.same_owner(b),
        (Self::Code(a),Self::Code(b)) => a.same_owner(b), (Self::Vector(a),Self::Vector(b)) => a.same_owner(b), (Self::Opaque(a),Self::Opaque(b)) => Arc::ptr_eq(a,b), _ => false,
    } }
    pub(crate) fn weak_owner(&self)->std::sync::Weak<dyn Send+Sync> {
        match self {
            Self::Tile(tile)=>{let owner:Arc<dyn Send+Sync>=tile.compressed.clone();Arc::downgrade(&owner)},
            Self::Bytes(value)|Self::Encoded(value)=>value.weak_owner(),Self::Code(value)=>value.weak_owner(),Self::Vector(value)=>value.weak_owner(),
            Self::Opaque(value)=>{let owner:Arc<dyn Send+Sync>=value.clone();Arc::downgrade(&owner)},
        }
    }
    pub(crate) fn prepared(&self, cancelled:&AtomicBool) -> Result<(ResourceEncoding,EncodedIntegrity),String> {
        match self {
            Self::Opaque(_)=>Err("Range resources use bounded reads".into()),
            Self::Tile(tile)=>Ok((ResourceEncoding::Raw,tile.compressed.integrity(cancelled)?)),
            Self::Encoded(bytes)=>Ok((ResourceEncoding::Raw,bytes.encoded_integrity(cancelled,||Ok(bytes.storage().clone()))?)),
            Self::Bytes(bytes)=>{
                let encoded=bytes.encoded(|data|compress(data,||bytes.storage().clone()));
                Ok((encoded.encoding,bytes.encoded_integrity(cancelled,||Ok(encoded.bytes.clone()))?))
            },
            Self::Code(code)=>{
                let encoded=code.encoded(|data|compress(data.as_bytes(),||Arc::from(data.as_bytes())));
                Ok((encoded.encoding,code.encoded_integrity(cancelled,||Ok(encoded.bytes.clone()))?))
            },
            Self::Vector(chunk)=>{
                let encoded=encode_vector(chunk);
                Ok((encoded.encoding,chunk.encoded_integrity(cancelled,||Ok(encoded.bytes.clone()))?))
            },
        }
    }
    fn digest(&self,cancelled:&AtomicBool)->Result<[u8;32],String> {
        match self {
            Self::Opaque(_)=>Err("Range resources use bounded reads".into()),
            Self::Tile(tile)=>tile.compressed.digest(cancelled),
            Self::Bytes(bytes)|Self::Encoded(bytes)=>bytes.encoded_digest(cancelled,||Ok(self.encoded()?.bytes)),
            Self::Code(code)=>code.encoded_digest(cancelled,||Ok(self.encoded()?.bytes)),
            Self::Vector(chunk)=>chunk.encoded_digest(cancelled,||Ok(self.encoded()?.bytes)),
        }
    }
    pub fn encoded(&self) -> Result<EncodedBytes, String> { Ok(match self {
        Self::Opaque(_) => return Err("Range resources use bounded reads".into()),
        Self::Tile(tile) => {
            #[cfg(not(target_arch="wasm32"))]
            let bytes=tile.compressed_on_worker()?.into_arc();
            #[cfg(target_arch="wasm32")]
            let bytes=tile.memory_compressed()?.ok_or("Package input bytes were not supplied to the worker")?;
            EncodedBytes {encoding:ResourceEncoding::Raw,bytes}
        },
        Self::Encoded(bytes) => EncodedBytes { encoding:ResourceEncoding::Raw, bytes:bytes.storage().clone() },
        Self::Bytes(bytes) => bytes.encoded(|data| compress(data, || bytes.storage().clone())).clone(),
        Self::Code(code) => code.encoded(|data| compress(data.as_bytes(), || Arc::from(data.as_bytes()))).clone(),
        Self::Vector(chunk) => encode_vector(chunk).clone(),
    }) }
}
pub(crate) trait ResourceInput {
    async fn prepared(payload:&Payload,cancelled:&AtomicBool)->Result<(ResourceEncoding,EncodedIntegrity),String>;
    async fn encoded(payload:&Payload)->Result<super::ByteRange,String>;
    async fn digest(payload:&Payload,cancelled:&AtomicBool)->Result<[u8;32],String>;
    async fn opaque(resource:&OpaqueResource,offset:u64,length:usize,cancelled:&AtomicBool)->Result<super::ByteRange,String>;
}
pub(crate) struct ReadyResources;
pub(crate) struct LeasedResources;
impl ResourceInput for ReadyResources {
    async fn prepared(payload:&Payload,cancelled:&AtomicBool)->Result<(ResourceEncoding,EncodedIntegrity),String> {payload.prepared(cancelled)}
    async fn encoded(payload:&Payload)->Result<super::ByteRange,String> {let bytes=payload.encoded()?.bytes;let length=bytes.len();super::ByteRange::new(bytes,0..length).map_err(str::to_string)}
    async fn digest(payload:&Payload,cancelled:&AtomicBool)->Result<[u8;32],String> {payload.digest(cancelled)}
    async fn opaque(resource:&OpaqueResource,offset:u64,length:usize,cancelled:&AtomicBool)->Result<super::ByteRange,String> {resource.read_chunk(offset,length,cancelled).map_err(|error|error.to_string())}
}
impl ResourceInput for LeasedResources {
    async fn prepared(payload:&Payload,cancelled:&AtomicBool)->Result<(ResourceEncoding,EncodedIntegrity),String> {
        if let Payload::Tile(tile)=payload && tile.compressed.needs_preparation() {tile.read_compressed().await?;}
        payload.prepared(cancelled)
    }
    async fn encoded(payload:&Payload)->Result<super::ByteRange,String> {
        match payload {Payload::Tile(tile)=>tile.read_compressed().await,_=>ReadyResources::encoded(payload).await}
    }
    async fn digest(payload:&Payload,cancelled:&AtomicBool)->Result<[u8;32],String> {
        match payload {Payload::Tile(tile)=>tile.compressed.digest_async(cancelled).await,_=>payload.digest(cancelled)}
    }
    async fn opaque(resource:&OpaqueResource,offset:u64,length:usize,cancelled:&AtomicBool)->Result<super::ByteRange,String> {resource.read_chunk_async(offset,length,cancelled).await.map_err(|error|error.to_string())}
}
#[derive(Clone, Debug)]
pub struct ResourceEntry { pub kind: &'static str, pub data: Value, pub raw_encoding: &'static str, pub payload: Payload }
impl ResourceEntry {
    pub(crate) fn store_record(&self,cancelled:&AtomicBool)->Result<(Value,EncodedIntegrity),String> {
        let (encoded,integrity)=self.payload.prepared(cancelled)?;
        let mut data=self.data.clone();
        let encoding=if encoded==ResourceEncoding::Lz4 {
            let length=match &self.payload {
                Payload::Bytes(bytes)=>bytes.len(),Payload::Code(code)=>code.len(),
                Payload::Vector(chunk)=>chunk.encoded_len().ok_or("Missing vector chunk length")?,
                _=>return Err("Invalid resource compression owner".into()),
            };
            data.as_object_mut().ok_or("Invalid resource descriptor")?.insert("decoded_bytes".into(),length.to_string().into());
            "capy.lz4-bytes/1"
        }else {self.raw_encoding};
        Ok((json!({"id":self.payload.id(),"type":self.kind,"data":data,"encoding":encoding,
            "bytes":integrity.bytes.to_string(),"crc32":format!("{:08x}",integrity.crc32)}),integrity))
    }
}
#[derive(Clone, Debug, Default)]
pub struct ResourceInventory {
    pub entries: BTreeMap<PortableId, ResourceEntry>,
    pub(crate) transfer: bool,
    pub(crate) private: bool,
    pub(crate) images: BTreeMap<PortableId, Value>,
    pub(crate) transfer_selections: BTreeMap<SelectionKey, Arc<crate::SelectionPixels>>,
    pub(crate) opaque: BTreeMap<PortableId, Arc<OpaqueResource>>,
}
impl ResourceInventory {
    pub(crate) fn for_transfer() -> Self { Self { transfer: true, private: true, ..Default::default() } }
    pub fn insert(&mut self, entry: ResourceEntry) -> Result<Value, String> {
        let id = entry.payload.id();
        if self.images.contains_key(&id) || self.opaque.contains_key(&id) {return Err("Object and resource identities overlap".into());}
        if let Some(previous) = self.entries.get(&id) {
            let conflict=|reason:&str|format!("Conflicting immutable resource identity {id} ({}): {reason}",entry.kind);
            if previous.kind != entry.kind || previous.data != entry.data || previous.raw_encoding != entry.raw_encoding {
                return Err(conflict("descriptor differs"));
            }
            if !previous.payload.same_owner(&entry.payload) {
                let (Payload::Code(before),Payload::Code(after))=(&previous.payload,&entry.payload) else {return Err(conflict("owner differs"));};
                if before.as_ref()!=after.as_ref() {return Err(conflict("shader bytes differ"));}
                match (before.encoded_if_ready(),after.encoded_if_ready()) {
                    (Some(before),Some(after)) if before.encoding!=after.encoding || before.bytes!=after.bytes=>return Err(conflict("encoded shader bytes differ")),
                    (None,Some(_))=>{self.entries.insert(id,entry);},
                    _=>{},
                }
            }
        } else { self.entries.insert(id, entry); }
        Ok(reference(id))
    }
    pub(crate) fn image(&mut self,id:PortableId,record:Value)->Result<Value,String> {
        if self.entries.contains_key(&id) || self.opaque.contains_key(&id) {return Err("Object and resource identities overlap".into());}
        if let Some(previous)=self.images.get(&id) {
            if previous!=&record {return Err("Conflicting immutable image identity".into());}
        } else {self.images.insert(id,record);}
        Ok(reference(id))
    }
    pub(crate) fn slot(&mut self, slot: &Slot) -> Result<Value, String> {
        match slot {
            Slot::Image(image) => super::artwork_records::encode_image(image, self),
            Slot::Resource(resource) => {
                if self.entries.contains_key(&resource.id) || self.images.contains_key(&resource.id) {return Err("Object and resource identities overlap".into());}
                if self.opaque.insert(resource.id, resource.clone()).is_some_and(|previous| previous != *resource) {return Err("Conflicting slot resource identity".into());}
                Ok(reference(resource.id))
            }
        }
    }
    pub(crate) fn with_opaque(&self, retained: impl IntoIterator<Item = Arc<OpaqueResource>>) -> Result<Vec<Arc<OpaqueResource>>, String> {
        let mut all = self.opaque.clone();
        for resource in retained {
            if all.get(&resource.id).is_some_and(|previous| *previous != resource) { return Err("Conflicting opaque resource identity".into()); }
            all.insert(resource.id, resource);
        }
        Ok(all.into_values().collect())
    }
    pub fn bytes(&mut self, kind: &'static str, bytes: &Resource<[u8]>, data: Value) -> Result<Value, String> {
        self.insert(ResourceEntry { kind, data, raw_encoding:"raw", payload:Payload::Bytes(bytes.clone()) })
    }
    pub fn profile(&mut self, profile: &ColorProfile) -> Result<Value, String> { Ok(match profile {
        ColorProfile::Builtin(space) => json!({"builtin":values::encode_rgb_space(*space)}),
        ColorProfile::Icc(bytes) => json!({"resource":self.bytes("capy.icc/1",bytes,json!({}))?}),
    }) }
    pub fn vector_bytes(&mut self, bytes: &Resource<[u8]>, data: Value) -> Result<Value, String> {
        self.insert(ResourceEntry { kind:"capy.vector-items/1", data, raw_encoding:"capy.vector-records/1", payload:Payload::Bytes(bytes.clone()) })
    }
    pub fn vector_chunk(&mut self, chunk: &Resource<VectorChunk>) -> Result<Value, String> {
        self.insert(ResourceEntry { kind:"capy.vector-items/1", data:json!({"records":chunk.records().len()}), raw_encoding:"capy.vector-records/1", payload:Payload::Vector(chunk.clone()) })
    }
    pub fn tile(&mut self, tile: Arc<TileBlob>) -> Result<Value, String> {
        let data = encode_descriptor(tile.descriptor)?;
        self.insert(ResourceEntry { kind:"capy.raster-tile/1", data, raw_encoding:"capy.lz4-tile/1", payload:Payload::Tile(tile) })
    }
    pub(crate) fn validate_opaque_alias(&self,resource:&crate::authored::OpaqueResource)->Result<bool,String> {
        let Some(entry)=self.entries.get(&resource.id) else {return Ok(false)};
        let mut data=resource.data.clone();
        let wrapped=WRAPPERS.contains(&resource.encoding.as_ref());
        if wrapped {data.as_object_mut().ok_or("Invalid wrapped resource descriptor")?.remove("decoded_bytes");}
        if resource.kind.as_ref()!=entry.kind || data!=entry.data || !resource.extra_fields.is_empty()
            || (resource.encoding.as_ref()!=entry.raw_encoding && !wrapped) {return Err("Opaque resource conflicts with authored resource".into());}
        Ok(true)
    }
    pub fn prepare(&self,cancelled:&AtomicBool)->Result<PreparedResources,String> {super::archive::ready(self.prepare_using::<ReadyResources>(cancelled))}
    pub async fn prepare_async(&self,cancelled:&AtomicBool)->Result<PreparedResources,String> {self.prepare_using::<LeasedResources>(cancelled).await}
    pub(crate) async fn prepare_using<I:ResourceInput>(&self, cancelled: &AtomicBool) -> Result<PreparedResources, String> {
        if self.transfer { return Err("Worker transfer inventory cannot be written as a package".into()); }
        let mut entries: Vec<PreparedResource> = Vec::with_capacity(self.entries.len());
        enum Candidates { Single(usize), Hashed(BTreeMap<[u8;32],Vec<usize>>) }
        let mut candidates = BTreeMap::<(u64,u32),Candidates>::new();
        let mut length = 0u64;
        let mut combined = crc32fast::Hasher::new();
        for (id, entry) in &self.entries {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let (encoded,integrity) = I::prepared(&entry.payload,cancelled).await?;
            let mut data = entry.data.clone();
            let encoding = if encoded == ResourceEncoding::Lz4 {
                let decoded = match &entry.payload { Payload::Bytes(bytes) => bytes.len(), Payload::Code(code) => code.len(),
                    Payload::Vector(chunk) => chunk.encoded_len().ok_or("Missing vector chunk length")?, _ => return Err("Invalid resource compression owner".into()) };
                data.as_object_mut().ok_or("Invalid resource descriptor")?.insert("decoded_bytes".into(), decoded.to_string().into());
                "capy.lz4-bytes/1"
            } else { entry.raw_encoding };
            let EncodedIntegrity {bytes,crc32:crc}=integrity;
            let mut alias = None;
            match candidates.entry((bytes,crc)) {
                std::collections::btree_map::Entry::Vacant(bucket)=>{bucket.insert(Candidates::Single(entries.len()));},
                std::collections::btree_map::Entry::Occupied(mut bucket)=>{
                    if let Candidates::Single(index)=*bucket.get() {
                        let digest=I::digest(&entries[index].payload,cancelled).await?;
                        *bucket.get_mut()=Candidates::Hashed([(digest,vec![index])].into());
                    }
                    let digest=I::digest(&entry.payload,cancelled).await?;
                    let Candidates::Hashed(hashes)=bucket.get_mut() else {unreachable!()};
                    let indices=hashes.entry(digest).or_default();
                    for &index in indices.iter() {
                        let previous=&entries[index];
                        if previous.record["type"]==entry.kind && previous.record["data"]==data && previous.record["encoding"]==encoding
                            && (previous.payload.same_owner(&entry.payload) || *I::encoded(&previous.payload).await?==*I::encoded(&entry.payload).await?) {
                            alias=Some(index);break;
                        }
                    }
                    if alias.is_none() {indices.push(entries.len());}
                },
            }
            let offset = alias.map_or_else(||length.to_string(),|index|entries[index].record["location"]["offset"].as_str().unwrap().to_owned());
            let record = json!({"id":id,"type":entry.kind,"data":data,"encoding":encoding,
                "location":{"pack":"data/tiles-1.bin","offset":offset},"bytes":bytes.to_string(),"crc32":format!("{crc:08x}")});
            if alias.is_none() {
                combined.combine(&crc32fast::Hasher::new_with_initial_len(crc, bytes));
                length = length.checked_add(bytes).ok_or("Package resource length overflow")?;
            }
            entries.push(PreparedResource { record, payload:entry.payload.clone(), bytes, crc, physical:alias.is_none() });
        }
        Ok(PreparedResources { entries, length, crc:combined.finalize() })
    }
}
#[derive(Clone, Debug)]
pub struct PreparedResource { pub record: Value, pub payload: Payload, pub bytes: u64, pub crc: u32, physical: bool }
#[derive(Clone, Debug)]
pub struct PreparedResources { pub entries: Vec<PreparedResource>, pub length: u64, pub crc: u32 }
pub struct PackReader<'a> { entries: &'a [PreparedResource], index: usize, current: Option<super::ByteRange>, position: u64, hasher: crc32fast::Hasher, cancelled: &'a AtomicBool }
impl PreparedResources {
    pub fn reader<'a>(&'a self, cancelled: &'a AtomicBool) -> PackReader<'a> { PackReader { entries:&self.entries,index:0,current:None,position:0,hasher:crc32fast::Hasher::new(),cancelled } }
}
impl std::io::Read for PackReader<'_> {
    fn read(&mut self,output:&mut [u8])->std::io::Result<usize> {super::archive::ready(self.read_using::<ReadyResources>(output))}
}
impl super::archive::ReadCursor for PackReader<'_> {
    fn read_exact<'a>(&'a mut self,output:&'a mut [u8])->super::store_io::FileFuture<'a,()> {
        Box::pin(async move {
            let mut position=0;
            while position<output.len() {
                let count=self.read_using::<LeasedResources>(&mut output[position..]).await.map_err(|error|format!("Package I/O failed: {error}"))?;
                if count==0 {return Err("Package I/O failed: failed to fill whole buffer".into());}position+=count;
            }Ok(())
        })
    }
}
impl PackReader<'_> {
    async fn read_using<I:ResourceInput>(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) { return Err(std::io::Error::other("Package operation cancelled")); }
        if output.is_empty() { return Ok(0); }
        loop {
            if self.entries.get(self.index).is_some_and(|entry|!entry.physical) { self.index+=1; continue; }
            if let Some(PreparedResource {payload:Payload::Opaque(resource),..})=self.entries.get(self.index) {
                let count=output.len().min(MAX_RANGE_BYTES).min(resource.length.saturating_sub(self.position).min(usize::MAX as u64) as usize);
                if count==0 {
                    let crc=std::mem::replace(&mut self.hasher,crc32fast::Hasher::new()).finalize();
                    if crc!=resource.crc32 {return Err(std::io::Error::other(resource.backing.fail("Opaque resource checksum failed".into())));}
                    self.index+=1;self.position=0;continue;
                }
                let bytes=I::opaque(resource,self.position,count,self.cancelled).await.map_err(std::io::Error::other)?;
                self.hasher.update(&bytes);output[..count].copy_from_slice(&bytes);self.position+=count as u64;return Ok(count);
            }
            if self.current.is_none() {
                let Some(entry) = self.entries.get(self.index) else { return Ok(0) };
                let bytes = I::encoded(&entry.payload).await.map_err(std::io::Error::other)?;
                let (_,integrity)=I::prepared(&entry.payload,self.cancelled).await.map_err(std::io::Error::other)?;
                if bytes.len() as u64 != entry.bytes || integrity.bytes != entry.bytes || integrity.crc32 != entry.crc { return Err(std::io::Error::other("Immutable resource changed during save")); }
                self.current = Some(bytes); self.position = 0;
            }
            let bytes = self.current.as_ref().unwrap();
            let count = output.len().min(bytes.len() - self.position as usize);
            if count == 0 { self.index += 1; self.current = None; self.position=0; continue; }
            output[..count].copy_from_slice(&bytes[self.position as usize..self.position as usize+count]); self.position += count as u64;
            return Ok(count);
        }
    }
}

#[derive(serde::Serialize)]
pub(crate) struct DescriptorFields {alpha:&'static str,channels:&'static str,depth:&'static str,transfer:&'static str}
pub(crate) fn descriptor_fields(descriptor:PixelDescriptor)->Result<DescriptorFields,String> {
    if descriptor.byte_len([TILE_SIZE;2]).is_none() { return Err("Invalid tile descriptor".into()); }
    let channels = match (descriptor.channels,descriptor.alpha,descriptor.encoding) {
        (1,_,TransferEncoding::Linear) => "coverage", (1,_,_) => "gray", (2,_,_) => "gray_alpha", (3,_,_) => "rgb", (4,AlphaAssociation::None,_) => "cmyk", (4,_,_) => "rgba", _ => return Err("Unknown raster channels".into()),
    };
    let depth = match (descriptor.sample,descriptor.bits_per_channel) { (SampleType::Unsigned,8)=>"u8",(SampleType::Unsigned,16)=>"u16",(SampleType::Float,16)=>"f16",(SampleType::Float,32)=>"f32",_=>return Err("Invalid raster sample depth".into()) };
    let transfer = match descriptor.encoding { TransferEncoding::Linear=>"linear", TransferEncoding::Profile=>"profile", TransferEncoding::Srgb=>return Err("Unsupported raster transfer".into()) };
    let alpha = match descriptor.alpha { AlphaAssociation::None=>"none", AlphaAssociation::Straight=>"straight", AlphaAssociation::PremultipliedLinear=>return Err("Unsupported raster alpha".into()) };
    Ok(DescriptorFields {alpha,channels,depth,transfer})
}
pub fn encode_descriptor(descriptor:PixelDescriptor)->Result<Value,String> {
    serde_json::to_value(descriptor_fields(descriptor)?).map_err(|error|error.to_string())
}
pub fn parse_descriptor(value: &Value) -> DecodeResult<PixelDescriptor> {
    let data = values::object(value,&["channels","depth","transfer","alpha"])?;
    let name = values::string(values::required(data,"channels")?)?;
    let channels = match name { "coverage"|"gray"=>1,"gray_alpha"=>2,"rgb"=>3,"rgba"|"cmyk"=>4,unknown=>return Err(DecodeError::Unsupported(format!("Unknown raster channels {unknown}"))) };
    let depth = values::parse_depth(values::required(data,"depth")?)?;
    let encoding = match values::string(values::required(data,"transfer")?)? {"linear"=>TransferEncoding::Linear,"profile"=>TransferEncoding::Profile,unknown=>return Err(DecodeError::Unsupported(format!("Unknown transfer {unknown}")))};
    let alpha = match values::string(values::required(data,"alpha")?)? {"none"=>AlphaAssociation::None,"straight"=>AlphaAssociation::Straight,unknown=>return Err(DecodeError::Unsupported(format!("Unknown alpha {unknown}")))};
    let descriptor = PixelDescriptor { channels, bits_per_channel:depth.bits(), sample:if depth.is_float(){SampleType::Float}else{SampleType::Unsigned}, encoding, alpha };
    if descriptor.byte_len([TILE_SIZE;2]).is_none() || name == "coverage" && (depth.is_float() || encoding != TransferEncoding::Linear || alpha != AlphaAssociation::None)
        || matches!(name,"cmyk"|"gray"|"rgb") && alpha != AlphaAssociation::None || matches!(name,"rgba"|"gray_alpha") && alpha == AlphaAssociation::None {
        return Err("Invalid raster sample interpretation".into());
    }
    Ok(descriptor)
}

pub(crate) type SelectionKey = ([u32; 2], [u32; 4], bool, Vec<PortableId>);

pub struct ResourceReader<'a> {
    pub(crate) admission: Option<crate::MemoryReservation>,
    pub manifest: &'a Manifest, pub backing: &'a ImmutableBacking, pub cancelled: &'a AtomicBool,
    pub limits: crate::ProjectLimits,
    pub(crate) private: bool,
    pub(crate) waits: bool,
    pub(crate) store:Option<&'a super::session_store::StoreResources>,
    pub(crate) trusted_programs: super::TrustedPrograms,
    pub(crate) bytes: BTreeMap<PortableId, Resource<[u8]>>, pub(crate) texts: BTreeMap<PortableId, Resource<str>>,
    pub(crate) tiles: BTreeMap<PortableId, Arc<TileBlob>>, pub(crate) luts: BTreeMap<PortableId, Arc<Lut3d>>, decoded: u64,
    verified: bool,
    aliases: BTreeMap<(usize,u64,u64), (PortableId,Value)>,
    rasters: BTreeMap<PortableId, crate::raster::RasterTile>,
    pub(crate) selections: BTreeMap<SelectionKey, Arc<crate::SelectionPixels>>,
    pub(crate) images: BTreeMap<PortableId, (Value, crate::authored::Image)>,
    pub(crate) programs: BTreeMap<Vec<u8>, Arc<crate::EffectProgram>>,
    pub(crate) vectors: BTreeMap<PortableId, (u32, u32, usize, u64, Resource<VectorChunk>)>,
    pub(crate) opaque: BTreeMap<PortableId, Arc<OpaqueResource>>,
    package:super::PackageLease,
    prepared:BTreeMap<PortableId,Arc<[u8]>>,prepared_bytes:u64,
}
#[derive(Default)]
pub(crate) struct ResourceCache {
    pub(crate) admission: Option<crate::MemoryReservation>,
    bytes: BTreeMap<PortableId, Resource<[u8]>>, texts: BTreeMap<PortableId, Resource<str>>,
    tiles: BTreeMap<PortableId, Arc<TileBlob>>, luts: BTreeMap<PortableId, Arc<Lut3d>>, decoded: u64,
    aliases: BTreeMap<(usize,u64,u64), (PortableId,Value)>, rasters: BTreeMap<PortableId, crate::raster::RasterTile>,
    selections: BTreeMap<SelectionKey, Arc<crate::SelectionPixels>>, images: BTreeMap<PortableId, (Value, crate::authored::Image)>,
    programs: BTreeMap<Vec<u8>, Arc<crate::EffectProgram>>,
    vectors: BTreeMap<PortableId, (u32, u32, usize, u64, Resource<VectorChunk>)>,
    opaque: BTreeMap<PortableId, Arc<OpaqueResource>>,
}
impl<'a> ResourceReader<'a> {
    pub fn new(manifest: &'a Manifest, backing: &'a ImmutableBacking, cancelled: &'a AtomicBool, limits: crate::ProjectLimits) -> Self {
        Self::with_budget(manifest, backing, cancelled, limits, crate::MemoryBudget::process())
    }
    pub(crate) fn with_budget(manifest: &'a Manifest, backing: &'a ImmutableBacking, cancelled: &'a AtomicBool, limits: crate::ProjectLimits, budget: crate::MemoryBudget) -> Self {
        Self {admission:budget.admit_import(0),package:super::PackageLease::new(backing.clone()),prepared:Default::default(),prepared_bytes:0,manifest,backing,cancelled,limits,private:false,waits:false,
            store:None,trusted_programs:Default::default(),bytes:BTreeMap::new(),texts:BTreeMap::new(),tiles:BTreeMap::new(),luts:BTreeMap::new(),decoded:0,verified:false,aliases:BTreeMap::new(),rasters:BTreeMap::new(),selections:BTreeMap::new(),images:BTreeMap::new(),programs:BTreeMap::new(),vectors:BTreeMap::new(),opaque:BTreeMap::new()}
    }
    pub(crate) async fn prepare_metadata(&mut self)->DecodeResult<()> {
        for (&id,record) in &self.manifest.resources {
            if record.value["type"]=="capy.raster-tile/1" {continue;}
            let Some(bound)=stored_decode_bound(&record.value)? else {continue;};
            if record.bytes>lz4_flex::block::get_maximum_output_size(bound) as u64 {return Err(DecodeError::Unsupported("Resource exceeds encoded size bound".into()));}
            self.prepared_bytes=self.prepared_bytes.checked_add(record.bytes).ok_or("Metadata range size overflow")?;
            self.charge_covered(0).await?;
            let range=record.range.ok_or("Missing metadata resource range")?;
            let mut bytes=Vec::with_capacity(record.bytes as usize);
            while bytes.len()<record.bytes as usize {
                if self.cancelled.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
                let chunk=self.backing.wait(range.offset+bytes.len() as u64,(record.bytes as usize-bytes.len()).min(MAX_RANGE_BYTES)).await.map_err(DecodeError::Io)?;
                bytes.extend_from_slice(&chunk);
            }
            if crc32fast::hash(&bytes)!=record.crc32 {return Err(self.backing.fail("Resource checksum failed".into()).into());}
            self.prepared.insert(id,bytes.into());
        }
        let mut slots=std::collections::BTreeSet::new();
        for object in self.manifest.objects.values() {
            if let Some(values)=object["data"]["slots"].as_array() {for value in values {
                let id=reference_id(value)?;if self.manifest.resources.contains_key(&id) {slots.insert(id);}
            }}
        }
        for id in slots {
            let resource=self.opaque_location(id)?;self.charge_covered(resource.length).await?;
            resource.verify_async(self.cancelled).await?;self.decoded+=resource.length;self.opaque.insert(id,resource);
        }
        Ok(())
    }
    pub(crate) fn install_cache(&mut self, cache: ResourceCache) {
        self.admission=cache.admission.or_else(||self.admission.take());
        self.bytes=cache.bytes;self.texts=cache.texts;self.tiles=cache.tiles;self.luts=cache.luts;self.decoded=cache.decoded;
        self.aliases=cache.aliases;self.rasters=cache.rasters;self.selections=cache.selections;self.images=cache.images;self.programs=cache.programs;self.vectors=cache.vectors;self.opaque=cache.opaque;
    }
    pub(crate) fn into_cache(self)->ResourceCache {
        ResourceCache {admission:self.admission,bytes:self.bytes,texts:self.texts,tiles:self.tiles,luts:self.luts,decoded:self.decoded,
            aliases:self.aliases,rasters:self.rasters,selections:self.selections,images:self.images,programs:self.programs,vectors:self.vectors,opaque:self.opaque}
    }
    pub(crate) fn require_verified(&mut self) { self.verified = true; }
    fn alias(&self, id:PortableId) -> DecodeResult<Option<PortableId>> {
        if self.cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        let record=self.manifest.resources.get(&id).ok_or("Missing resource")?;
        let Some(range)=record.range else { return Ok(None) };
        let Some((alias,previous))=self.aliases.get(&(range.member,range.offset,range.length)) else { return Ok(None) };
        let identity_field=|key:&&String| key.as_str()!="id"&&key.as_str()!="location";
        if !record.value.as_object().ok_or("Invalid resource record")?.iter().filter(|(key,_)|identity_field(key))
                .eq(previous.as_object().ok_or("Invalid resource record")?.iter().filter(|(key,_)|identity_field(key))) {
            return Err("Conflicting aliased resource descriptors".into());
        }
        Ok(Some(*alias))
    }
    fn remember_alias(&mut self,id:PortableId) {
        if let Some(range)=self.manifest.resources[&id].range {
            self.aliases.entry((range.member,range.offset,range.length)).or_insert_with(||(id,self.manifest.resources[&id].value.clone()));
        }
    }
    fn charge(&self, bytes: u64) -> DecodeResult<()> {
        let decoded = self.decoded.checked_add(bytes).ok_or("Decoded resource size overflow")?;
        if decoded > self.limits.raster_bytes.saturating_add(self.limits.asset_bytes) { return Err(DecodeError::MemoryBudgetExceeded); }
        if self.admission.as_ref().is_some_and(|admission| !admission.resize(decoded.saturating_add(self.prepared_bytes).max(admission.reserved_bytes()))) { return Err(DecodeError::MemoryBudgetExceeded); }
        if self.waits && self.admission.as_ref().is_some_and(|admission| !admission.covered()) { return Err(DecodeError::Uncovered); }
        Ok(())
    }
    async fn charge_covered(&self, bytes: u64) -> DecodeResult<()> {
        loop { match self.charge(bytes) { Err(DecodeError::Uncovered) => self.covering().await, result => return result } }
    }
    pub(crate) async fn covering(&self) { if let Some(admission) = &self.admission { admission.covering().await; } }
    pub fn raster_tile(&mut self, reference:&Value) -> DecodeResult<crate::raster::RasterTile> {
        let id = reference_id(reference)?;
        if let Some(tile) = self.rasters.get(&id) { return Ok(tile.clone()); }
        let tile = crate::raster::RasterTile::backed_shared(self.tile(reference)?);
        self.rasters.insert(id,tile.clone());
        Ok(tile)
    }
    pub fn record(&self, reference: &Value, kind: &str) -> DecodeResult<(PortableId, &Value)> {
        let id = reference_id(reference)?;
        let record = &self.manifest.resources.get(&id).ok_or("Missing resource")?.value;
        let actual=values::string(&record["type"])?;
        if !super::manifest::known_resource(actual) {return Err(DecodeError::Unsupported(format!("Unknown resource type {actual}")));}
        if actual != kind { return Err(DecodeError::Unsupported("Resource type does not match binding".into())); }
        if !super::manifest::resource_supported(record)? {return Err(DecodeError::Unsupported("Unknown resource interpretation".into()));}
        Ok((id,record))
    }
    fn stored(&self, id: PortableId, limit: usize) -> DecodeResult<(Arc<[u8]>,EncodedIntegrity)> {
        if self.verified { return Err("Missing verified resource payload".into()); }
        let record = self.manifest.resources.get(&id).ok_or("Missing resource")?;
        let range = record.range.ok_or_else(|| DecodeError::Unsupported("Unknown resource location".into()))?;
        if record.bytes > limit as u64 { return Err(DecodeError::Unsupported("Resource exceeds encoded size bound".into())); }
        if range.length != record.bytes {return Err("Resource range length disagrees with descriptor".into());}
            if let Some(store)=self.store {
            let bytes=store.loaded.get(&id).ok_or("Stored resource bytes are not prepared")?.clone();
            return Ok((bytes,EncodedIntegrity {bytes:record.bytes,crc32:record.crc32}));
        }
        if let Some(bytes)=self.prepared.get(&id) {return Ok((bytes.clone(),EncodedIntegrity {bytes:record.bytes,crc32:record.crc32}));}
        let mut bytes = Vec::new(); bytes.try_reserve_exact(record.bytes as usize).map_err(|_| DecodeError::Unsupported("Resource allocation failed".into()))?;
        while bytes.len() < record.bytes as usize {
            if self.cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let length = (record.bytes as usize - bytes.len()).min(MAX_RANGE_BYTES);
            let chunk=self.backing.read_on_worker(range.offset.checked_add(bytes.len() as u64).ok_or("Resource offset overflow")?,length).map_err(DecodeError::Io)?;
            bytes.extend_from_slice(&chunk);
        }
        let integrity=EncodedIntegrity::calculate(&bytes,self.cancelled)?;
        if integrity.crc32 != record.crc32 { return Err(self.backing.fail("Resource checksum failed".into()).into()); }
        Ok((bytes.into(),integrity))
    }
    pub fn bytes(&mut self, reference: &Value, kind: &str, limit: usize) -> DecodeResult<Resource<[u8]>> {
        let (id,record) = self.record(reference,kind)?;
        if let Some(bytes) = self.bytes.get(&id) { return Ok(bytes.clone()); }
        let encoding = record["encoding"].as_str().ok_or("Missing resource encoding")?;
        let wrapper = WRAPPERS.iter().position(|name| *name == encoding);
        let expected_raw = match kind {"capy.wgsl/1"=>"utf8","capy.lut3d/1"=>"capy.rgb-f32/1","capy.vector-items/1"=>"capy.vector-records/1",_=>"raw"};
        if wrapper.is_none() && encoding != expected_raw { return Err(DecodeError::Unsupported("Unknown resource encoding".into())); }
        let expected = if wrapper.is_some() { decimal_u64(&record["data"]["decoded_bytes"])? } else { self.manifest.resources[&id].bytes };
        if expected > limit as u64 { return Err(DecodeError::Unsupported("Resource exceeds decoded size admission".into())); }
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.bytes.get(&alias)) {
            let result=previous.alias(id);
            self.bytes.insert(id,result.clone()); return Ok(result);
        }
        self.charge(expected)?;
        let (stored,integrity) = self.stored(id, lz4_flex::block::get_maximum_output_size(limit))?;
        let result = match wrapper {
            Some(0) => {
                let mut bytes = vec![0;expected as usize];
                let written = lz4_flex::block::decompress_into(&stored,&mut bytes).map_err(|_| self.backing.fail("Invalid compressed resource".into()))?;
                if written != bytes.len() { return Err(self.backing.fail("Incomplete compressed resource".into()).into()); }
                Resource::with_id_and_encoded(id,bytes.into(),EncodedBytes { encoding:ResourceEncoding::Lz4,bytes:stored })
            }
            Some(_) => Resource::with_id(id,super::archive::inflate(&stored,expected as usize,true).map_err(|e|self.backing.fail(e))?.into()),
            None => Resource::with_id_and_encoded(id,stored.clone(),EncodedBytes { encoding:ResourceEncoding::Raw,bytes:stored }),
        };
        if result.encoded_if_ready().is_some() { result.set_encoded_integrity(integrity)?; }
        self.decoded+=expected; self.remember_alias(id);
        self.bytes.insert(id,result.clone()); Ok(result)
    }
    pub(crate) fn slots(&mut self, value: Option<&Value>) -> DecodeResult<Arc<[Slot]>> {
        let Some(value) = value else { return Ok(Arc::default()) };
        let list = value.as_array().ok_or("Expected vector slots")?;
        list.iter().map(|reference| {
            let id = reference_id(reference)?;
            if self.manifest.resources.contains_key(&id) { return self.opaque_resource(id).map(Slot::Resource); }
            match self.manifest.objects.get(&id).map(|record| &record["type"]) {
                Some(kind) if kind == "capy.image/1" => super::artwork_records::image_reference(reference, self).map(Slot::Image),
                _ => Err(DecodeError::Unsupported("Unsupported vector slot target".into())),
            }
        }).collect()
    }
    pub(crate) fn resource_backing(&self,_id:PortableId,offset:u64)->DecodeResult<(ImmutableBacking,u64)> {
            if let Some(store)=self.store {return Ok((store.locations.get(&_id).ok_or("Missing stored resource location")?.backing()?,0));}
        Ok((self.backing.clone(),offset))
    }
    fn opaque_location(&self, id:PortableId) -> DecodeResult<Arc<OpaqueResource>> {
        let record = &self.manifest.resources[&id];
        if !super::references(&record.value, super::manifest::ManifestLimits::default().traversal_nodes)?.is_empty() {
            return Err(DecodeError::Unsupported("A slot resource references other records".into()));
        }
        let range = record.range.ok_or("Missing slot resource range")?;
        let (backing,offset)=self.resource_backing(id,range.offset)?;
        let resource = Arc::new(OpaqueResource::from_record(id, &record.value, backing, offset, record.bytes, record.crc32).map_err(DecodeError::from)?);
        Ok(resource)
    }
    fn opaque_resource(&mut self,id:PortableId)->DecodeResult<Arc<OpaqueResource>> {
        if let Some(resource)=self.opaque.get(&id) {return Ok(resource.clone());}
        let resource=self.opaque_location(id)?;
        self.charge(resource.length)?;
        if let Some(store)=self.store {store.verify_opaque(id);}else {resource.verify(self.cancelled)?;}
        self.decoded+=resource.length;self.opaque.insert(id,resource.clone());Ok(resource)
    }
    pub fn vector_object(&mut self, items: &[Value], slots: Arc<[Slot]>, next_item: Option<u64>) -> DecodeResult<VectorObject> {
        let mut reading = super::vector_records::Reading::slots(slots.len());
        let mut records = 0usize;
        let mut chunks = Vec::with_capacity(items.len());
        for item in items {
            let (id, record) = self.record(item, "capy.vector-items/1")?;
            let count = usize::try_from(record["data"]["records"].as_u64().ok_or("Vector items require a record count")?).map_err(|_| DecodeError::Unsupported("Vector record count exceeds admission".into()))?;
            records = records.saturating_add(count);
            if records > super::vector_records::MAX_RECORDS { return Err(DecodeError::Unsupported("Vector object exceeds the admitted record count".into())); }
            let chunk = match self.vectors.get(&id) {
                Some((start, end, count, highest, chunk)) if *start == reading.open && *count == slots.len() => { reading.open = *end; reading.highest = reading.highest.max(*highest); chunk.clone() }
                _ => {
                    let (start, highest) = (reading.open, std::mem::take(&mut reading.highest));
                    let bytes = self.bytes(item, "capy.vector-items/1", super::vector_records::MAX_CHUNK_BYTES)?;
                    let chunk = super::vector_records::decode_chunk(&bytes, count, &mut reading)?;
                    let own = std::mem::replace(&mut reading.highest, highest);
                    reading.highest = reading.highest.max(own);
                    let resource = match bytes.encoded_if_ready().cloned() {
                        Some(encoded) => Resource::with_id_and_encoded(id, Arc::new(chunk), encoded),
                        None => Resource::with_id(id, Arc::new(chunk)),
                    };
                    let resident = crate::history_budget::Accounting::default().charge_vector(&VectorObject::from_chunks(vec![resource.clone()])) as u64;
                    self.charge(resident)?;
                    self.decoded += resident;
                    if let Some(integrity) = bytes.encoded_integrity_if_ready() { resource.set_encoded_integrity(integrity)?; }
                    self.vectors.insert(id, (start, reading.open, slots.len(), own, resource.clone()));
                    resource
                }
            };
            chunks.push(chunk);
        }
        let object = match next_item {
            Some(next) if next > reading.highest => VectorObject::with_slots(chunks, slots).with_next_item(next),
            None if chunks.is_empty() => VectorObject::with_slots(chunks, slots),
            _ => return Err("Invalid vector data: the next item must follow every item identity".into()),
        };
        object.validate().map_err(|e| format!("Invalid vector data: {e}"))?;
        Ok(object)
    }
    pub fn profile(&mut self, value: &Value) -> DecodeResult<ColorProfile> {
        let fields = values::object(value,&["builtin","resource"])?;
        if fields.len()!=1 {return Err("Invalid profile binding".into());}
        if let Some(value)=fields.get("builtin") {return Ok(ColorProfile::Builtin(values::parse_rgb_space(value)?));}
        let bytes=self.bytes(values::required(fields,"resource")?,"capy.icc/1",crate::color::source::MAX_PROFILE_BYTES)?;
        if bytes.is_empty() {return Err("Empty ICC profile".into());}
        Ok(ColorProfile::Icc(bytes))
    }
    pub fn tile(&mut self, reference: &Value) -> DecodeResult<Arc<TileBlob>> {
        let (id,record)=self.record(reference,"capy.raster-tile/1")?;
        if let Some(tile)=self.tiles.get(&id) {return Ok(tile.clone());}
        let filtered=match record["encoding"].as_str() {Some("capy.lz4-tile/1")=>false,Some("capy.deflate-tile/1")=>true,_=>return Err(DecodeError::Unsupported("Unknown tile encoding".into()))};
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.tiles.get(&alias)) {
            let tile=Arc::new(previous.alias(id));
            self.tiles.insert(id,tile.clone()); return Ok(tile);
        }
        let descriptor=parse_descriptor(&record["data"])?;
        descriptor.byte_len([TILE_SIZE;2]).ok_or("Invalid tile descriptor")?;
            if let Some(store)=self.store && !filtered {
            let location=store.locations.get(&id).ok_or("Missing stored tile location")?.clone();
            let tile=Arc::new(TileBlob::from_store(id,descriptor,location)?);
            self.remember_alias(id);self.tiles.insert(id,tile.clone());return Ok(tile);
        }
        let record=&self.manifest.resources[&id];let range=record.range.ok_or("Missing tile range")?;
        let range=self.package.range(range.offset,record.bytes,record.crc32,descriptor,filtered)?;
        let tile=Arc::new(TileBlob::from_package_range(id,descriptor,range)?);
        self.remember_alias(id);self.tiles.insert(id,tile.clone());Ok(tile)
    }
}

impl super::effect_records::ResourceWriter for ResourceInventory {
    fn code(&mut self, code: &Resource<str>) -> Result<Value, String> {
        self.insert(ResourceEntry {kind:"capy.wgsl/1",data:json!({}),raw_encoding:"utf8",payload:Payload::Code(code.clone())})
    }
    fn lut(&mut self, lut: &Lut3d) -> Result<Value, String> {
        let payload=lut.resource().ok_or("Unresolved color lookup resource")?;
        let resource=self.insert(ResourceEntry {kind:"capy.lut3d/1",data:json!({"size":lut.size(),"domain":lut.domain()}),raw_encoding:"capy.rgb-f32/1",payload:Payload::Bytes(payload.clone())})?;
        Ok(json!({"resource":resource,"title":lut.title()}))
    }
    fn image(&mut self, image: &crate::authored::Image) -> Result<Value, String> {
        super::artwork_records::encode_image(image,self)
    }
}
impl super::effect_records::ResourceReader for ResourceReader<'_> {
    fn resolve_program(&self, program: &Arc<crate::EffectProgram>) -> DecodeResult<Arc<crate::EffectProgram>> { self.trusted_programs.resolve(program) }
    fn code(&mut self, value: &Value) -> DecodeResult<Resource<str>> {
        let (id,record)=self.record(value,"capy.wgsl/1")?;
        values::object(&record["data"],&["decoded_bytes"])?;
        if let Some(code)=self.texts.get(&id) {return Ok(code.clone());}
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.texts.get(&alias)) {
            let code=previous.alias(id);
            self.texts.insert(id,code.clone()); return Ok(code);
        }
        let bytes=self.bytes(value,"capy.wgsl/1",4*1024*1024)?;
        let text:Arc<str>=std::str::from_utf8(&bytes).map_err(|_|"Invalid UTF-8 shader resource")?.into();
        let code: Resource<str>=match bytes.encoded_if_ready() {Some(encoded)=>Resource::with_id_and_encoded(id,text,encoded.clone()),None=>Resource::with_id(id,text)};
        if let Some(integrity)=bytes.encoded_integrity_if_ready() {code.set_encoded_integrity(integrity)?;}
        self.texts.insert(id,code.clone()); self.bytes.remove(&id); Ok(code)
    }
    fn lut(&mut self, value: &Value) -> DecodeResult<Arc<Lut3d>> {
        let binding=values::object(value,&["resource","title"])?;
        let title:Arc<str>=values::string(values::required(binding,"title")?)?.into();
        if title.chars().count()>256 {return Err(DecodeError::Unsupported("Color lookup title exceeds the current limit".into()));}
        let resource=values::required(binding,"resource")?;
        let (id,record)=self.record(resource,"capy.lut3d/1")?;
        if let Some(lut)=self.luts.get(&id) {return Ok(Arc::new(lut.with_title(title)?));}
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.luts.get(&alias)) {
            let lut=Arc::new(previous.alias_resource(id)?.with_title(title)?);
            self.luts.insert(id,lut.clone()); return Ok(lut);
        }
        let data=values::object(&record["data"],&["size","domain","decoded_bytes"])?;
        let size=values::u32_value(values::required(data,"size")?)?;
        if size>Lut3d::MAX_SIZE {return Err(DecodeError::Unsupported("Color lookup size exceeds the current limit".into()));}
        let domain=values::array(values::required(data,"domain")?,2)?;
        let mut bounds=[[0.;3];2];
        for (out,row) in bounds.iter_mut().zip(domain) { for (component,value) in out.iter_mut().zip(values::array(row,3)?) { *component=values::finite_f32(value)?; } }
        let bytes=self.bytes(resource,"capy.lut3d/1",(Lut3d::MAX_SIZE as usize).pow(3)*12)?;
        let lut=Arc::new(Lut3d::from_resource(size,bounds,title,bytes).map_err(|e|self.backing.fail(e.into()))?);
        lut.validate_gpu_domain().map_err(|e|DecodeError::Unsupported(e.into()))?;
        self.luts.insert(id,lut.clone()); Ok(lut)
    }
    fn image(&mut self, value: &Value) -> DecodeResult<crate::authored::Image> {
        super::artwork_records::image_reference(value,self)
    }
}
impl super::selection_records::SelectionResourceWriter for ResourceInventory {
    fn validate_selection(&self, selection:&crate::Selection) -> Result<(),String> {
        if self.transfer {super::selection_records::validate_selection_metadata(selection)}
        else {selection.validate().map_err(|e|e.to_string())}
    }
    fn pixels(&mut self, pixels:&crate::SelectionPixels) -> Result<Value,String> {
        let ids = pixels.transfer_chunk_ids();
        let key = (pixels.extent(), pixels.bounds(), pixels.coverage_format() == 2, ids.to_vec());
        let refs = if self.transfer {
            if !self.transfer_selections.contains_key(&key) {
                for (index, id) in ids.iter().enumerate() {
                    let bytes = pixels.transfer_chunks().map(|chunks| chunks[index].clone())
                        .unwrap_or_else(||Resource::with_id(*id,Arc::from([])));
                    self.chunk(&bytes, super::selection_records::chunk_descriptor(pixels.extent(),pixels.bounds(),pixels.coverage_format()==2,index))?;
                }
                self.transfer_selections.insert(key, Arc::new(pixels.clone()));
            }
            ids.iter().copied().map(reference).collect::<Vec<_>>()
        } else {
            pixels.package_chunks()?.iter().enumerate().map(|(index,chunk)|self.chunk(chunk,
                super::selection_records::chunk_descriptor(pixels.extent(),pixels.bounds(),pixels.coverage_format()==2,index))).collect::<Result<Vec<_>,_>>()?
        };
        Ok(json!({"extent":pixels.extent(),"bounds":pixels.bounds(),"depth":if pixels.coverage_format()==2 {"u8"}else{"u4"},"chunks":refs}))
    }
    fn chunk(&mut self, bytes:&Resource<[u8]>, data:Value) -> Result<Value,String> {
        self.insert(ResourceEntry {kind:"capy.selection-coverage/1",data,raw_encoding:"capy.lz4-coverage/1",payload:Payload::Encoded(bytes.clone())})
    }
}
impl super::selection_records::SelectionResourceReader for ResourceReader<'_> {
    fn validate_selection(&self, selection:&crate::Selection) -> DecodeResult<()> {
        if self.verified {super::selection_records::validate_selection_metadata(selection).map_err(DecodeError::from)}
        else {selection.validate().map_err(|e|DecodeError::from(e.to_string()))}
    }
    fn pixels(&mut self, extent: [u32; 2], bounds: [u32; 4], bytes: bool, chunks: Vec<Resource<[u8]>>) -> DecodeResult<Arc<crate::SelectionPixels>> {
        let key = (extent, bounds, bytes, chunks.iter().map(Resource::id).collect());
        if let Some(pixels) = self.selections.get(&key) { return Ok(pixels.clone()); }
        if self.verified { return Err("Missing verified selection pixels".into()); }
        let canonical=(extent,bounds,bytes,chunks.iter().map(|chunk|self.alias(chunk.id()).map(|alias|alias.unwrap_or(chunk.id()))).collect::<DecodeResult<Vec<_>>>()?);
        let pixels=if let Some(previous)=self.selections.get(&canonical) {
            Arc::new(crate::SelectionPixels::from_verified_words(extent,bounds,bytes,previous.transfer_words().clone(),chunks.iter().map(Resource::id).collect(),Some(chunks))?)
        } else {
            let retained=u64::from(extent[0].div_ceil(if bytes {4}else{8}))*u64::from(extent[1])*4;
            self.charge(retained)?;
            let pixels=Arc::new(crate::SelectionPixels::from_package_chunks(extent,bounds,bytes,chunks)?);
            self.decoded+=retained;
            self.selections.insert(canonical,pixels.clone()); pixels
        };
        self.selections.insert(key, pixels.clone());
        Ok(pixels)
    }
    fn limits(&self) -> crate::ProjectLimits {self.limits}
    fn chunk(&mut self, value:&Value, expected:&Value) -> DecodeResult<Resource<[u8]>> {
        let (id,record)=self.record(value,"capy.selection-coverage/1")?;
        if record["encoding"]!="capy.lz4-coverage/1" {return Err(DecodeError::Unsupported("Unknown selection compression".into()));}
        if record["data"]!=*expected {return Err("Selection resource descriptor disagrees with source".into());}
        if let Some(bytes)=self.bytes.get(&id) {return Ok(bytes.clone());}
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.bytes.get(&alias)) {
            let bytes=previous.alias(id);
            self.bytes.insert(id,bytes.clone()); return Ok(bytes);
        }
        let (stored,integrity)=self.stored(id,lz4_flex::block::get_maximum_output_size(65536))?;
        let bytes=Resource::with_id(id,stored);bytes.set_encoded_integrity(integrity)?;
        self.remember_alias(id);
        self.bytes.insert(id,bytes.clone()); Ok(bytes)
    }
}

impl PreparedResources {
    pub fn append_opaque(&mut self,resources:Vec<Arc<OpaqueResource>>,cancelled:&AtomicBool)->Result<(),String> {super::archive::ready(self.append_opaque_using::<ReadyResources>(resources,cancelled))}
    pub(crate) async fn append_opaque_using<I:ResourceInput>(&mut self, resources:Vec<Arc<crate::authored::OpaqueResource>>, cancelled:&AtomicBool) -> Result<(),String> {
        let mut combined=crc32fast::Hasher::new_with_initial_len(self.crc,self.length);
        let ids:BTreeMap<_,_>=self.entries.iter().enumerate().map(|(index,entry)|(entry.payload.id(),index)).collect();
        for resource in resources {
            if cancelled.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
            if let Some(index)=ids.get(&resource.id) {
                let existing=&self.entries[*index];
                let candidate=resource.record(existing.record["location"].clone());
                if candidate!=existing.record {return Err("Opaque resource conflicts with authored resource".into());}
                let bytes=I::encoded(&existing.payload).await?;
                let mut offset=0;
                while offset<bytes.len() {
                    let count=(bytes.len()-offset).min(MAX_RANGE_BYTES);
                    if *I::opaque(&resource,offset as u64,count,cancelled).await?!=bytes[offset..offset+count] {return Err("Conflicting resource bytes".into());}
                    offset+=count;
                }
                continue;
            }
            let record=resource.record(json!({"pack":"data/tiles-1.bin","offset":self.length.to_string()}));
            combined.combine(&crc32fast::Hasher::new_with_initial_len(resource.crc32,resource.length));
            self.length=self.length.checked_add(resource.length).ok_or("Opaque package size overflow")?;
            self.entries.push(PreparedResource {record,bytes:resource.length,crc:resource.crc32,payload:Payload::Opaque(resource),physical:true});
        }
        self.crc=combined.finalize(); Ok(())
    }
}

#[cfg(test)]
mod tests;
