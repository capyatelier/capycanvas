use super::{ImmutableBacking, RangeState, MAX_RANGE_BYTES, manifest::{Manifest, decimal_u64}, values::{self, DecodeError, DecodeResult}};
use crate::{authored::{PortableId, Resource, ResourceEncoding, EncodedBytes}, color::{PixelDescriptor, SampleType, TransferEncoding, AlphaAssociation, ColorProfile}, raster::{TileBlob, TILE_SIZE}, Lut3d};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub fn reference(id: PortableId) -> Value { json!({"ref":id}) }
pub fn reference_id(value: &Value) -> DecodeResult<PortableId> {
    values::string(values::required(values::object(value, &["ref"])?, "ref")?)?.parse().map_err(DecodeError::from)
}
fn compress(bytes: &[u8], raw: impl FnOnce() -> Arc<[u8]>) -> EncodedBytes {
    let compressed = lz4_flex::block::compress(bytes);
    if compressed.len() < bytes.len() { EncodedBytes { encoding: ResourceEncoding::Lz4, bytes: compressed.into() } }
    else { EncodedBytes { encoding: ResourceEncoding::Raw, bytes: raw() } }
}

#[derive(Clone, Debug)]
pub enum Payload { Tile(Arc<TileBlob>), Bytes(Resource<[u8]>), Code(Resource<str>), Encoded(Resource<[u8]>), Opaque(Arc<crate::authored::OpaqueResource>) }
impl Payload {
    pub fn id(&self) -> PortableId { match self { Self::Tile(tile) => tile.resource_id(), Self::Bytes(bytes) | Self::Encoded(bytes) => bytes.id(), Self::Code(code) => code.id(), Self::Opaque(resource) => resource.id } }
    pub fn same_owner(&self, other: &Self) -> bool { match (self, other) {
        (Self::Tile(a),Self::Tile(b)) => Arc::ptr_eq(a,b), (Self::Bytes(a),Self::Bytes(b)) | (Self::Encoded(a),Self::Encoded(b)) => a.same_owner(b),
        (Self::Code(a),Self::Code(b)) => a.same_owner(b), (Self::Opaque(a),Self::Opaque(b)) => Arc::ptr_eq(a,b), _ => false,
    } }
    pub fn encoded(&self) -> Result<EncodedBytes, String> { Ok(match self {
        Self::Opaque(_) => return Err("Range resources use bounded reads".into()),
        Self::Tile(tile) => EncodedBytes { encoding:ResourceEncoding::Raw, bytes:tile.compressed()? },
        Self::Encoded(bytes) => EncodedBytes { encoding:ResourceEncoding::Raw, bytes:bytes.storage().clone() },
        Self::Bytes(bytes) => bytes.encoded(|data| compress(data, || bytes.storage().clone())).clone(),
        Self::Code(code) => code.encoded(|data| compress(data.as_bytes(), || Arc::from(data.as_bytes()))).clone(),
    }) }
}
#[derive(Clone, Debug)]
pub struct ResourceEntry { pub kind: &'static str, pub data: Value, pub raw_encoding: &'static str, pub payload: Payload }
#[derive(Clone, Debug, Default)]
pub struct ResourceInventory { pub entries: BTreeMap<PortableId, ResourceEntry> }
impl ResourceInventory {
    pub fn insert(&mut self, entry: ResourceEntry) -> Result<Value, String> {
        let id = entry.payload.id();
        if let Some(previous) = self.entries.get(&id) {
            if previous.kind != entry.kind || previous.data != entry.data || previous.raw_encoding != entry.raw_encoding || !previous.payload.same_owner(&entry.payload) {
                return Err("Conflicting immutable resource identity".into());
            }
        } else { self.entries.insert(id, entry); }
        Ok(reference(id))
    }
    pub fn bytes(&mut self, kind: &'static str, bytes: &Resource<[u8]>, data: Value) -> Result<Value, String> {
        self.insert(ResourceEntry { kind, data, raw_encoding:"raw", payload:Payload::Bytes(bytes.clone()) })
    }
    pub fn profile(&mut self, profile: &ColorProfile) -> Result<Value, String> { Ok(match profile {
        ColorProfile::Builtin(space) => json!({"builtin":values::encode_rgb_space(*space)}),
        ColorProfile::Icc(bytes) => json!({"resource":self.bytes("capy.icc/1",bytes,json!({}))?}),
    }) }
    pub fn tile(&mut self, tile: Arc<TileBlob>) -> Result<Value, String> {
        let mut data = encode_descriptor(tile.descriptor)?;
        if let Some(profile)=tile.resource_profile() {data["profile"]=self.bytes("capy.icc/1",profile,json!({}))?;}
        self.insert(ResourceEntry { kind:"capy.raster-tile/1", data, raw_encoding:"capy.lz4-tile/1", payload:Payload::Tile(tile) })
    }
    pub fn prepare(&self, cancelled: &AtomicBool) -> Result<PreparedResources, String> {
        let mut entries = Vec::with_capacity(self.entries.len());
        let mut length = 0u64;
        let mut combined = crc32fast::Hasher::new();
        for (id, entry) in &self.entries {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let encoded = entry.payload.encoded()?;
            let mut data = entry.data.clone();
            let encoding = if encoded.encoding == ResourceEncoding::Lz4 {
                let decoded = match &entry.payload { Payload::Bytes(bytes) => bytes.len(), Payload::Code(code) => code.len(), _ => return Err("Invalid resource compression owner".into()) };
                data.as_object_mut().ok_or("Invalid resource descriptor")?.insert("decoded_bytes".into(), decoded.to_string().into());
                "capy.lz4-bytes/1"
            } else { entry.raw_encoding };
            let bytes = encoded.bytes.len() as u64;
            let crc = crc32fast::hash(&encoded.bytes);
            let record = json!({"id":id,"type":entry.kind,"data":data,"encoding":encoding,
                "location":{"pack":"data/tiles-1.bin","offset":length.to_string()},"bytes":bytes.to_string(),"crc32":format!("{crc:08x}")});
            combined.combine(&crc32fast::Hasher::new_with_initial_len(crc, bytes));
            length = length.checked_add(bytes).ok_or("Package resource length overflow")?;
            entries.push(PreparedResource { record, payload:entry.payload.clone(), bytes, crc });
        }
        Ok(PreparedResources { entries, length, crc:combined.finalize() })
    }
}
#[derive(Clone, Debug)]
pub struct PreparedResource { pub record: Value, pub payload: Payload, pub bytes: u64, pub crc: u32 }
#[derive(Clone, Debug)]
pub struct PreparedResources { pub entries: Vec<PreparedResource>, pub length: u64, pub crc: u32 }
pub struct PackReader<'a> { entries: &'a [PreparedResource], index: usize, current: Option<Arc<[u8]>>, position: u64, hasher: crc32fast::Hasher, cancelled: &'a AtomicBool }
impl PreparedResources {
    pub fn reader<'a>(&'a self, cancelled: &'a AtomicBool) -> PackReader<'a> { PackReader { entries:&self.entries,index:0,current:None,position:0,hasher:crc32fast::Hasher::new(),cancelled } }
}
impl std::io::Read for PackReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) { return Err(std::io::Error::other("Package operation cancelled")); }
        if output.is_empty() { return Ok(0); }
        loop {
            if let Some(PreparedResource {payload:Payload::Opaque(resource),..})=self.entries.get(self.index) {
                let count=output.len().min(MAX_RANGE_BYTES).min(resource.length.saturating_sub(self.position).min(usize::MAX as u64) as usize);
                if count==0 {
                    let crc=std::mem::replace(&mut self.hasher,crc32fast::Hasher::new()).finalize();
                    if crc!=resource.crc32 {return Err(std::io::Error::other(resource.backing.fail("Opaque resource checksum failed".into())));}
                    self.index+=1;self.position=0;continue;
                }
                let bytes=resource.read_chunk(self.position,count,self.cancelled).map_err(std::io::Error::other)?;
                self.hasher.update(&bytes);output[..count].copy_from_slice(&bytes);self.position+=count as u64;return Ok(count);
            }
            if self.current.is_none() {
                let Some(entry) = self.entries.get(self.index) else { return Ok(0) };
                let bytes = entry.payload.encoded().map_err(std::io::Error::other)?.bytes;
                if bytes.len() as u64 != entry.bytes || crc32fast::hash(&bytes) != entry.crc { return Err(std::io::Error::other("Immutable resource changed during save")); }
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

pub fn encode_descriptor(descriptor: PixelDescriptor) -> Result<Value, String> {
    if descriptor.byte_len([TILE_SIZE;2]).is_none() { return Err("Invalid tile descriptor".into()); }
    let channels = match (descriptor.channels,descriptor.alpha,descriptor.encoding) {
        (1,_,TransferEncoding::Linear) => "coverage", (1,_,_) => "gray", (2,_,_) => "gray_alpha", (3,_,_) => "rgb", (4,AlphaAssociation::None,_) => "cmyk", (4,_,_) => "rgba", _ => return Err("Unknown raster channels".into()),
    };
    let depth = match (descriptor.sample,descriptor.bits_per_channel) { (SampleType::Unsigned,8)=>"u8",(SampleType::Unsigned,16)=>"u16",(SampleType::Float,16)=>"f16",(SampleType::Float,32)=>"f32",_=>return Err("Invalid raster sample depth".into()) };
    Ok(json!({"channels":channels,"depth":depth,"transfer":match descriptor.encoding {TransferEncoding::Linear=>"linear",TransferEncoding::Srgb=>"srgb",TransferEncoding::Profile=>"profile"},
        "alpha":match descriptor.alpha {AlphaAssociation::None=>"none",AlphaAssociation::Straight=>"straight",AlphaAssociation::PremultipliedLinear=>"premultiplied_linear"}}))
}
pub fn parse_descriptor(value: &Value) -> DecodeResult<PixelDescriptor> {
    let data = values::object(value,&["channels","depth","transfer","alpha","profile"])?;
    let name = values::string(values::required(data,"channels")?)?;
    let channels = match name { "coverage"|"gray"=>1,"gray_alpha"=>2,"rgb"=>3,"rgba"|"cmyk"=>4,unknown=>return Err(DecodeError::Unsupported(format!("Unknown raster channels {unknown}"))) };
    let depth = values::parse_depth(values::required(data,"depth")?)?;
    let encoding = match values::string(values::required(data,"transfer")?)? {"linear"=>TransferEncoding::Linear,"srgb"=>TransferEncoding::Srgb,"profile"=>TransferEncoding::Profile,unknown=>return Err(DecodeError::Unsupported(format!("Unknown transfer {unknown}")))};
    let alpha = match values::string(values::required(data,"alpha")?)? {"none"=>AlphaAssociation::None,"straight"=>AlphaAssociation::Straight,"premultiplied_linear"=>AlphaAssociation::PremultipliedLinear,unknown=>return Err(DecodeError::Unsupported(format!("Unknown alpha {unknown}")))};
    let descriptor = PixelDescriptor { channels, bits_per_channel:depth.bits(), sample:if depth.is_float(){SampleType::Float}else{SampleType::Unsigned}, encoding, alpha };
    if descriptor.byte_len([TILE_SIZE;2]).is_none() || name == "coverage" && (depth.is_float() || encoding != TransferEncoding::Linear || alpha != AlphaAssociation::None)
        || matches!(name,"cmyk"|"gray"|"rgb") && alpha != AlphaAssociation::None || matches!(name,"rgba"|"gray_alpha") && alpha == AlphaAssociation::None {
        return Err("Invalid raster sample interpretation".into());
    }
    if let Some(profile)=data.get("profile") {
        reference_id(profile)?;
        if descriptor.encoding!=TransferEncoding::Profile {return Err("Profile binding on non-profile samples".into());}
    }
    Ok(descriptor)
}

pub struct ResourceReader<'a> {
    pub manifest: &'a Manifest, pub backing: &'a ImmutableBacking, pub cancelled: &'a AtomicBool,
    pub limits: crate::ProjectLimits,
    bytes: BTreeMap<PortableId, Resource<[u8]>>, texts: BTreeMap<PortableId, Resource<str>>,
    tiles: BTreeMap<PortableId, Arc<TileBlob>>, luts: BTreeMap<PortableId, Arc<Lut3d>>, decoded: u64,
}
impl<'a> ResourceReader<'a> {
    pub fn new(manifest: &'a Manifest, backing: &'a ImmutableBacking, cancelled: &'a AtomicBool, limits: crate::ProjectLimits) -> Self {
        Self {manifest,backing,cancelled,limits,bytes:BTreeMap::new(),texts:BTreeMap::new(),tiles:BTreeMap::new(),luts:BTreeMap::new(),decoded:0}
    }
    fn charge(&self, bytes: u64) -> DecodeResult<()> {
        let decoded = self.decoded.checked_add(bytes).ok_or("Decoded resource size overflow")?;
        if decoded > self.limits.raster_bytes { return Err(DecodeError::Unsupported("Drawing exceeds resource admission budget".into())); }
        Ok(())
    }
    pub fn record(&self, reference: &Value, kind: &str) -> DecodeResult<(PortableId, &Value)> {
        let id = reference_id(reference)?;
        let record = &self.manifest.resources.get(&id).ok_or("Missing resource")?.value;
        if record["type"].as_str() != Some(kind) { return Err("Resource type does not match binding".into()); }
        Ok((id,record))
    }
    pub fn stored(&self, id: PortableId, limit: usize) -> DecodeResult<Arc<[u8]>> {
        let record = self.manifest.resources.get(&id).ok_or("Missing resource")?;
        let range = record.range.ok_or_else(|| DecodeError::Unsupported("Unknown resource location".into()))?;
        if record.bytes > limit as u64 { return Err("Resource exceeds encoded size bound".into()); }
        if range.length != record.bytes {return Err("Resource range length disagrees with descriptor".into());}
        let mut bytes = Vec::new(); bytes.try_reserve_exact(record.bytes as usize).map_err(|_| "Resource allocation failed")?;
        while bytes.len() < record.bytes as usize {
            if self.cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let length = (record.bytes as usize - bytes.len()).min(MAX_RANGE_BYTES);
            match self.backing.poll(range.offset.checked_add(bytes.len() as u64).ok_or("Resource offset overflow")?, length)? {
                RangeState::Pending => return Err("Package bytes are pending".into()),
                RangeState::Ready(chunk) => bytes.extend_from_slice(&chunk),
            }
        }
        if crc32fast::hash(&bytes) != record.crc32 { return Err(self.backing.fail("Resource checksum failed".into()).into()); }
        Ok(bytes.into())
    }
    pub fn bytes(&mut self, reference: &Value, kind: &str, limit: usize) -> DecodeResult<Resource<[u8]>> {
        let (id,record) = self.record(reference,kind)?;
        if let Some(bytes) = self.bytes.get(&id) { return Ok(bytes.clone()); }
        let encoding = record["encoding"].as_str().ok_or("Missing resource encoding")?;
        let compressed = encoding == "capy.lz4-bytes/1";
        let expected_raw = match kind {"capy.wgsl/1"=>"utf8","capy.lut3d/1"=>"capy.lut3d-block/1",_=>"raw"};
        if !compressed && encoding != expected_raw { return Err(DecodeError::Unsupported("Unknown resource encoding".into())); }
        let expected = if compressed { decimal_u64(&record["data"]["decoded_bytes"])? } else { self.manifest.resources[&id].bytes };
        if expected > limit as u64 { return Err("Resource exceeds decoded size bound".into()); }
        self.charge(expected)?;
        let stored = self.stored(id, lz4_flex::block::get_maximum_output_size(limit))?;
        let decoded: Arc<[u8]> = if compressed {
            let mut bytes = vec![0;expected as usize];
            let written = lz4_flex::block::decompress_into(&stored,&mut bytes).map_err(|_| self.backing.fail("Invalid compressed resource".into()))?;
            if written != bytes.len() { return Err(self.backing.fail("Incomplete compressed resource".into()).into()); }
            bytes.into()
        } else { stored.clone() };
        let result = Resource::with_id_and_encoded(id,decoded,EncodedBytes { encoding:if compressed {ResourceEncoding::Lz4}else{ResourceEncoding::Raw},bytes:stored });
        self.decoded+=expected;
        self.bytes.insert(id,result.clone()); Ok(result)
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
        if record["encoding"]!="capy.lz4-tile/1" {return Err(DecodeError::Unsupported("Unknown tile encoding".into()));}
        let descriptor=parse_descriptor(&record["data"])?;
        let profile=record["data"].get("profile").cloned();
        let profile=profile.as_ref().map(|reference|self.bytes(reference,"capy.icc/1",crate::color::source::MAX_PROFILE_BYTES)).transpose()?;
        let size=descriptor.byte_len([TILE_SIZE;2]).ok_or("Invalid tile descriptor")?;
        self.charge(size as u64)?;
        let bytes=self.stored(id,lz4_flex::block::get_maximum_output_size(size))?;
        let tile=Arc::new(match profile {
            Some(profile)=>TileBlob::from_profiled_package(id,descriptor,bytes,profile),
            None=>TileBlob::from_package(id,descriptor,bytes),
        }.map_err(|e|self.backing.fail(e))?);
        self.decoded+=size as u64;
        self.tiles.insert(id,tile.clone()); Ok(tile)
    }
}

impl super::effect_records::ResourceWriter for ResourceInventory {
    fn code(&mut self, code: &Resource<str>) -> Result<Value, String> {
        self.insert(ResourceEntry {kind:"capy.wgsl/1",data:json!({}),raw_encoding:"utf8",payload:Payload::Code(code.clone())})
    }
    fn lut(&mut self, lut: &Lut3d) -> Result<Value, String> {
        let payload=lut.resource().ok_or("Unresolved color lookup resource")?;
        self.insert(ResourceEntry {kind:"capy.lut3d/1",data:json!({"size":lut.size(),"domain":lut.domain(),"title":lut.title()}),raw_encoding:"capy.lut3d-block/1",payload:Payload::Bytes(payload.clone())})
    }
}
impl super::effect_records::ResourceReader for ResourceReader<'_> {
    fn code(&mut self, value: &Value) -> DecodeResult<Resource<str>> {
        let (id,record)=self.record(value,"capy.wgsl/1")?;
        values::object(&record["data"],&["decoded_bytes"])?;
        if let Some(code)=self.texts.get(&id) {return Ok(code.clone());}
        let bytes=self.bytes(value,"capy.wgsl/1",4*1024*1024)?;
        let code: Resource<str>=Resource::with_id_and_encoded(id,std::str::from_utf8(&bytes).map_err(|_|"Invalid UTF-8 shader resource")?.into(),bytes.encoded_if_ready().unwrap().clone());
        self.texts.insert(id,code.clone()); self.bytes.remove(&id); Ok(code)
    }
    fn lut(&mut self, value: &Value) -> DecodeResult<Arc<Lut3d>> {
        let (id,record)=self.record(value,"capy.lut3d/1")?;
        if let Some(lut)=self.luts.get(&id) {return Ok(lut.clone());}
        let data=values::object(&record["data"],&["size","domain","title","decoded_bytes"])?;
        let size=values::u32_value(values::required(data,"size")?)?;
        let domain=values::array(values::required(data,"domain")?,2)?;
        let mut bounds=[[0.;3];2];
        for (out,row) in bounds.iter_mut().zip(domain) { for (component,value) in out.iter_mut().zip(values::array(row,3)?) { *component=values::finite_f32(value)?; } }
        let title:Arc<str>=data.get("title").map_or(Ok(""),values::string)?.into();
        let bytes=self.bytes(value,"capy.lut3d/1",Lut3d::HEADER_BYTES+(Lut3d::MAX_SIZE as usize).pow(3)*16)?;
        let lut=Arc::new(Lut3d::from_resource(size,bounds,title,bytes).map_err(|e|self.backing.fail(e.into()))?);
        self.luts.insert(id,lut.clone()); Ok(lut)
    }
}
impl super::selection_records::SelectionResourceWriter for ResourceInventory {
    fn chunk(&mut self, bytes:&Resource<[u8]>, data:Value) -> Result<Value,String> {
        self.insert(ResourceEntry {kind:"capy.selection-coverage/1",data,raw_encoding:"capy.lz4-coverage/1",payload:Payload::Encoded(bytes.clone())})
    }
}
impl super::selection_records::SelectionResourceReader for ResourceReader<'_> {
    fn limits(&self) -> crate::ProjectLimits {self.limits}
    fn chunk(&mut self, value:&Value, expected:&Value) -> DecodeResult<Resource<[u8]>> {
        let (id,record)=self.record(value,"capy.selection-coverage/1")?;
        if record["encoding"]!="capy.lz4-coverage/1" {return Err(DecodeError::Unsupported("Unknown selection compression".into()));}
        if record["data"]!=*expected {return Err("Selection resource descriptor disagrees with source".into());}
        if let Some(bytes)=self.bytes.get(&id) {return Ok(bytes.clone());}
        let extent=values::array(&expected["extent"],2)?;
        let width=values::u32_value(&extent[0])?;
        let height=values::u32_value(&extent[1])?;
        let words=u64::from(width.div_ceil(if expected["depth"]=="u8" {4}else{8}))*u64::from(height);
        let start=u64::from(values::u32_value(&expected["chunk"])?)*65536;
        let retained=(words*4).saturating_sub(start).min(65536);
        self.charge(retained)?;
        let bytes=Resource::with_id(id,self.stored(id,lz4_flex::block::get_maximum_output_size(65536))?);
        self.decoded+=retained;
        self.bytes.insert(id,bytes.clone()); Ok(bytes)
    }
}

impl PreparedResources {
    pub fn append_opaque(&mut self, resources:Vec<Arc<crate::authored::OpaqueResource>>, cancelled:&AtomicBool) -> Result<(),String> {
        let mut combined=crc32fast::Hasher::new_with_initial_len(self.crc,self.length);
        let ids:BTreeMap<_,_>=self.entries.iter().enumerate().map(|(index,entry)|(entry.payload.id(),index)).collect();
        for resource in resources {
            if cancelled.load(Ordering::Relaxed) {return Err("Package operation cancelled".into());}
            if let Some(index)=ids.get(&resource.id) {
                let existing=&self.entries[*index];
                let candidate=resource.record(existing.record["location"].clone());
                if candidate!=existing.record {return Err("Opaque resource conflicts with authored resource".into());}
                let bytes=existing.payload.encoded()?.bytes;
                let mut offset=0;
                while offset<bytes.len() {
                    let count=(bytes.len()-offset).min(MAX_RANGE_BYTES);
                    if *resource.read_chunk(offset as u64,count,cancelled)?!=bytes[offset..offset+count] {return Err("Conflicting resource bytes".into());}
                    offset+=count;
                }
                continue;
            }
            let record=resource.record(json!({"pack":"data/tiles-1.bin","offset":self.length.to_string()}));
            combined.combine(&crc32fast::Hasher::new_with_initial_len(resource.crc32,resource.length));
            self.length=self.length.checked_add(resource.length).ok_or("Opaque package size overflow")?;
            self.entries.push(PreparedResource {record,bytes:resource.length,crc:resource.crc32,payload:Payload::Opaque(resource)});
        }
        self.crc=combined.finalize(); Ok(())
    }
}

#[cfg(test)]
mod tests;
