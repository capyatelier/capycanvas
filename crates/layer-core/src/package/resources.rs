use super::{ImmutableBacking, RangeState, MAX_RANGE_BYTES, manifest::{Manifest, decimal_u64}, values::{self, DecodeError, DecodeResult}};
use crate::{authored::{PortableId, Resource, ResourceEncoding, EncodedBytes, EncodedIntegrity}, color::{PixelDescriptor, SampleType, TransferEncoding, AlphaAssociation, ColorProfile}, raster::{TileBlob, TILE_SIZE}, Lut3d};
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
        (Self::Tile(a),Self::Tile(b)) => Arc::ptr_eq(&a.compressed,&b.compressed), (Self::Bytes(a),Self::Bytes(b)) | (Self::Encoded(a),Self::Encoded(b)) => a.same_owner(b),
        (Self::Code(a),Self::Code(b)) => a.same_owner(b), (Self::Opaque(a),Self::Opaque(b)) => Arc::ptr_eq(a,b), _ => false,
    } }
    fn prepared(&self, cancelled:&AtomicBool) -> Result<(ResourceEncoding,EncodedIntegrity),String> {
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
        }
    }
    fn digest(&self,cancelled:&AtomicBool)->Result<[u8;32],String> {
        match self {
            Self::Opaque(_)=>Err("Range resources use bounded reads".into()),
            Self::Tile(tile)=>tile.compressed.digest(cancelled),
            Self::Bytes(bytes)|Self::Encoded(bytes)=>bytes.encoded_digest(cancelled,||Ok(self.encoded()?.bytes)),
            Self::Code(code)=>code.encoded_digest(cancelled,||Ok(self.encoded()?.bytes)),
        }
    }
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
pub struct ResourceInventory {
    pub entries: BTreeMap<PortableId, ResourceEntry>,
    pub(crate) transfer: bool,
    pub(crate) transfer_selections: BTreeMap<SelectionKey, Arc<crate::SelectionPixels>>,
}
impl ResourceInventory {
    pub(crate) fn for_transfer() -> Self { Self { transfer: true, ..Default::default() } }
    pub fn insert(&mut self, entry: ResourceEntry) -> Result<Value, String> {
        let id = entry.payload.id();
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
        if self.transfer { return Err("Worker transfer inventory cannot be written as a package".into()); }
        let mut entries: Vec<PreparedResource> = Vec::with_capacity(self.entries.len());
        enum Candidates { Single(usize), Hashed(BTreeMap<[u8;32],Vec<usize>>) }
        let mut candidates = BTreeMap::<(u64,u32),Candidates>::new();
        let mut length = 0u64;
        let mut combined = crc32fast::Hasher::new();
        for (id, entry) in &self.entries {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let (encoded,integrity) = entry.payload.prepared(cancelled)?;
            let mut data = entry.data.clone();
            let encoding = if encoded == ResourceEncoding::Lz4 {
                let decoded = match &entry.payload { Payload::Bytes(bytes) => bytes.len(), Payload::Code(code) => code.len(), _ => return Err("Invalid resource compression owner".into()) };
                data.as_object_mut().ok_or("Invalid resource descriptor")?.insert("decoded_bytes".into(), decoded.to_string().into());
                "capy.lz4-bytes/1"
            } else { entry.raw_encoding };
            let EncodedIntegrity {bytes,crc32:crc}=integrity;
            let mut alias = None;
            match candidates.entry((bytes,crc)) {
                std::collections::btree_map::Entry::Vacant(bucket)=>{bucket.insert(Candidates::Single(entries.len()));},
                std::collections::btree_map::Entry::Occupied(mut bucket)=>{
                    if let Candidates::Single(index)=*bucket.get() {
                        let digest=entries[index].payload.digest(cancelled)?;
                        *bucket.get_mut()=Candidates::Hashed([(digest,vec![index])].into());
                    }
                    let digest=entry.payload.digest(cancelled)?;
                    let Candidates::Hashed(hashes)=bucket.get_mut() else {unreachable!()};
                    let indices=hashes.entry(digest).or_default();
                    for &index in indices.iter() {
                        let previous=&entries[index];
                        if previous.record["type"]==entry.kind && previous.record["data"]==data && previous.record["encoding"]==encoding
                            && (previous.payload.same_owner(&entry.payload) || previous.payload.encoded()?.bytes==entry.payload.encoded()?.bytes) {
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
pub struct PackReader<'a> { entries: &'a [PreparedResource], index: usize, current: Option<Arc<[u8]>>, position: u64, hasher: crc32fast::Hasher, cancelled: &'a AtomicBool }
impl PreparedResources {
    pub fn reader<'a>(&'a self, cancelled: &'a AtomicBool) -> PackReader<'a> { PackReader { entries:&self.entries,index:0,current:None,position:0,hasher:crc32fast::Hasher::new(),cancelled } }
}
impl std::io::Read for PackReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
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
                let bytes=resource.read_chunk(self.position,count,self.cancelled).map_err(std::io::Error::other)?;
                self.hasher.update(&bytes);output[..count].copy_from_slice(&bytes);self.position+=count as u64;return Ok(count);
            }
            if self.current.is_none() {
                let Some(entry) = self.entries.get(self.index) else { return Ok(0) };
                let bytes = entry.payload.encoded().map_err(std::io::Error::other)?.bytes;
                let (_,integrity)=entry.payload.prepared(self.cancelled).map_err(std::io::Error::other)?;
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

pub(crate) type SelectionKey = ([u32; 2], [u32; 4], bool, Vec<PortableId>);

pub struct ResourceReader<'a> {
    pub manifest: &'a Manifest, pub backing: &'a ImmutableBacking, pub cancelled: &'a AtomicBool,
    pub limits: crate::ProjectLimits,
    pub(crate) bytes: BTreeMap<PortableId, Resource<[u8]>>, pub(crate) texts: BTreeMap<PortableId, Resource<str>>,
    pub(crate) tiles: BTreeMap<PortableId, Arc<TileBlob>>, pub(crate) luts: BTreeMap<PortableId, Arc<Lut3d>>, decoded: u64,
    verified: bool,
    aliases: BTreeMap<(usize,u64,u64), PortableId>,
    rasters: BTreeMap<PortableId, crate::raster::RasterTile>,
    pub(crate) selections: BTreeMap<SelectionKey, Arc<crate::SelectionPixels>>,
    pub(crate) originals: BTreeMap<String, Arc<crate::color::source::SourceImage>>,
}
impl<'a> ResourceReader<'a> {
    pub fn new(manifest: &'a Manifest, backing: &'a ImmutableBacking, cancelled: &'a AtomicBool, limits: crate::ProjectLimits) -> Self {
        Self {manifest,backing,cancelled,limits,bytes:BTreeMap::new(),texts:BTreeMap::new(),tiles:BTreeMap::new(),luts:BTreeMap::new(),decoded:0,verified:false,aliases:BTreeMap::new(),rasters:BTreeMap::new(),selections:BTreeMap::new(),originals:BTreeMap::new()}
    }
    pub(crate) fn require_verified(&mut self) { self.verified = true; }
    fn alias(&self, id:PortableId) -> DecodeResult<Option<PortableId>> {
        if self.cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        let record=self.manifest.resources.get(&id).ok_or("Missing resource")?;
        let Some(range)=record.range else { return Ok(None) };
        let Some(&alias)=self.aliases.get(&(range.member,range.offset,range.length)) else { return Ok(None) };
        let previous=&self.manifest.resources[&alias];
        let identity_field=|key:&&String| key.as_str()!="id"&&key.as_str()!="location";
        if record.bytes!=previous.bytes || record.crc32!=previous.crc32
            || !record.value.as_object().ok_or("Invalid resource record")?.iter().filter(|(key,_)|identity_field(key))
                .eq(previous.value.as_object().ok_or("Invalid resource record")?.iter().filter(|(key,_)|identity_field(key))) {
            return Err("Conflicting aliased resource descriptors".into());
        }
        Ok(Some(alias))
    }
    fn remember_alias(&mut self,id:PortableId) {
        if let Some(range)=self.manifest.resources[&id].range {
            self.aliases.entry((range.member,range.offset,range.length)).or_insert(id);
        }
    }
    fn charge(&self, bytes: u64) -> DecodeResult<()> {
        let decoded = self.decoded.checked_add(bytes).ok_or("Decoded resource size overflow")?;
        if decoded > self.limits.raster_bytes { return Err(DecodeError::Unsupported("Drawing exceeds resource admission budget".into())); }
        Ok(())
    }
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
        if record["type"].as_str() != Some(kind) { return Err("Resource type does not match binding".into()); }
        Ok((id,record))
    }
    fn stored(&self, id: PortableId, limit: usize) -> DecodeResult<(Arc<[u8]>,EncodedIntegrity)> {
        if self.verified { return Err("Missing verified resource payload".into()); }
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
        let integrity=EncodedIntegrity::calculate(&bytes,self.cancelled)?;
        if integrity.crc32 != record.crc32 { return Err(self.backing.fail("Resource checksum failed".into()).into()); }
        Ok((bytes.into(),integrity))
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
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.bytes.get(&alias)) {
            let result=previous.alias(id);
            self.bytes.insert(id,result.clone()); return Ok(result);
        }
        self.charge(expected)?;
        let (stored,integrity) = self.stored(id, lz4_flex::block::get_maximum_output_size(limit))?;
        let decoded: Arc<[u8]> = if compressed {
            let mut bytes = vec![0;expected as usize];
            let written = lz4_flex::block::decompress_into(&stored,&mut bytes).map_err(|_| self.backing.fail("Invalid compressed resource".into()))?;
            if written != bytes.len() { return Err(self.backing.fail("Incomplete compressed resource".into()).into()); }
            bytes.into()
        } else { stored.clone() };
        let result = Resource::with_id_and_encoded(id,decoded,EncodedBytes { encoding:if compressed {ResourceEncoding::Lz4}else{ResourceEncoding::Raw},bytes:stored });
        result.set_encoded_integrity(integrity)?;
        self.decoded+=expected; self.remember_alias(id);
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
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.tiles.get(&alias)) {
            let tile=Arc::new(previous.alias(id));
            self.tiles.insert(id,tile.clone()); return Ok(tile);
        }
        let descriptor=parse_descriptor(&record["data"])?;
        let profile=record["data"].get("profile").cloned();
        let profile=profile.as_ref().map(|reference|self.bytes(reference,"capy.icc/1",crate::color::source::MAX_PROFILE_BYTES)).transpose()?;
        let size=descriptor.byte_len([TILE_SIZE;2]).ok_or("Invalid tile descriptor")?;
        self.charge(size as u64)?;
        let (bytes,integrity)=self.stored(id,lz4_flex::block::get_maximum_output_size(size))?;
        let tile=Arc::new(match profile {
            Some(profile)=>TileBlob::from_profiled_package(id,descriptor,bytes,profile),
            None=>TileBlob::from_package(id,descriptor,bytes),
        }.map_err(|e|self.backing.fail(e))?);
        tile.compressed.set_integrity(integrity)?;
        self.decoded+=size as u64; self.remember_alias(id);
        self.tiles.insert(id,tile.clone()); Ok(tile)
    }
}

impl super::effect_records::ResourceWriter for ResourceInventory {
    fn code(&mut self, code: &Resource<str>) -> Result<Value, String> {
        self.insert(ResourceEntry {kind:"capy.wgsl/1",data:json!({}),raw_encoding:"utf8",payload:Payload::Code(code.clone())})
    }
    fn lut(&mut self, lut: &Lut3d) -> Result<Value, String> {
        let payload=lut.resource().ok_or("Unresolved color lookup resource")?;
        let resource=self.insert(ResourceEntry {kind:"capy.lut3d/1",data:json!({"size":lut.size(),"domain":lut.domain()}),raw_encoding:"capy.lut3d-block/1",payload:Payload::Bytes(payload.clone())})?;
        Ok(json!({"resource":resource,"title":lut.title()}))
    }
}
impl super::effect_records::ResourceReader for ResourceReader<'_> {
    fn code(&mut self, value: &Value) -> DecodeResult<Resource<str>> {
        let (id,record)=self.record(value,"capy.wgsl/1")?;
        values::object(&record["data"],&["decoded_bytes"])?;
        if let Some(code)=self.texts.get(&id) {return Ok(code.clone());}
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.texts.get(&alias)) {
            let code=previous.alias(id);
            self.texts.insert(id,code.clone()); return Ok(code);
        }
        let bytes=self.bytes(value,"capy.wgsl/1",4*1024*1024)?;
        let code: Resource<str>=Resource::with_id_and_encoded(id,std::str::from_utf8(&bytes).map_err(|_|"Invalid UTF-8 shader resource")?.into(),bytes.encoded_if_ready().unwrap().clone());
        if let Some(integrity)=bytes.encoded_integrity_if_ready() {code.set_encoded_integrity(integrity)?;}
        self.texts.insert(id,code.clone()); self.bytes.remove(&id); Ok(code)
    }
    fn lut(&mut self, value: &Value) -> DecodeResult<Arc<Lut3d>> {
        let binding=values::object(value,&["resource","title"])?;
        let title:Arc<str>=values::string(values::required(binding,"title")?)?.into();
        let resource=values::required(binding,"resource")?;
        let (id,record)=self.record(resource,"capy.lut3d/1")?;
        if let Some(lut)=self.luts.get(&id) {return Ok(Arc::new(lut.with_title(title)?));}
        if let Some(previous)=self.alias(id)?.and_then(|alias|self.luts.get(&alias)) {
            let lut=Arc::new(previous.alias_resource(id)?.with_title(title)?);
            self.luts.insert(id,lut.clone()); return Ok(lut);
        }
        let data=values::object(&record["data"],&["size","domain","decoded_bytes"])?;
        let size=values::u32_value(values::required(data,"size")?)?;
        let domain=values::array(values::required(data,"domain")?,2)?;
        let mut bounds=[[0.;3];2];
        for (out,row) in bounds.iter_mut().zip(domain) { for (component,value) in out.iter_mut().zip(values::array(row,3)?) { *component=values::finite_f32(value)?; } }
        let bytes=self.bytes(resource,"capy.lut3d/1",Lut3d::HEADER_BYTES+(Lut3d::MAX_SIZE as usize).pow(3)*16)?;
        let lut=Arc::new(Lut3d::from_resource(size,bounds,title,bytes).map_err(|e|self.backing.fail(e.into()))?);
        self.luts.insert(id,lut.clone()); Ok(lut)
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
            self.entries.push(PreparedResource {record,bytes:resource.length,crc:resource.crc32,payload:Payload::Opaque(resource),physical:true});
        }
        self.crc=combined.finalize(); Ok(())
    }
}

#[cfg(test)]
mod tests;
