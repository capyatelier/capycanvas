use super::*;
use serde::{Serializer,ser::{SerializeMap,Error}};
use serde_json::Value;

#[derive(Clone)]
pub(super) struct StoredRecord {pub id:PortableId,data:RecordData}
#[derive(Clone)]
enum RecordData {Tile(crate::color::PixelDescriptor),Other(Arc<Descriptor>)}
struct Descriptor {kind:String,data:Value,encoding:String,extra:serde_json::Map<String,Value>}
impl StoredRecord {
    pub fn tile(id:PortableId,descriptor:crate::color::PixelDescriptor)->Result<Self,String> {
        super::super::resources::descriptor_fields(descriptor)?;Ok(Self {id,data:RecordData::Tile(descriptor)})
    }
    pub fn parse(mut value:Value)->Result<Self,String> {
        let fields=value.as_object_mut().ok_or("Invalid recovery resource")?;
        let id=fields.remove("id").and_then(|id|id.as_str().and_then(|id|id.parse().ok())).ok_or("Invalid recovery resource identity")?;
        fields.remove("location");fields.remove("bytes");fields.remove("crc32");
        let data=fields.remove("data").ok_or("Missing recovery resource descriptor")?;
        let kind=fields.remove("type").and_then(|kind|kind.as_str().map(str::to_owned)).ok_or("Missing recovery resource type")?;
        let encoding=fields.remove("encoding").and_then(|encoding|encoding.as_str().map(str::to_owned)).ok_or("Missing recovery resource encoding")?;
        if kind=="capy.raster-tile/1" && encoding=="capy.lz4-tile/1" && fields.is_empty() {
            return Self::tile(id,super::super::resources::parse_descriptor(&data).map_err(|error|error.to_string())?);
        }
        Ok(Self {id,data:RecordData::Other(Arc::new(Descriptor {kind,data,encoding,extra:std::mem::take(fields)}))})
    }
    fn fields<M:SerializeMap>(&self,map:&mut M,length:u64,crc:u32)->Result<(),M::Error> {
        map.serialize_entry("bytes",&Decimal(length))?;map.serialize_entry("crc32",&Hex(crc))?;
        match &self.data {
            RecordData::Tile(descriptor)=>{
                let data=super::super::resources::descriptor_fields(*descriptor).map_err(M::Error::custom)?;
                map.serialize_entry("data",&data)?;map.serialize_entry("encoding","capy.lz4-tile/1")?;map.serialize_entry("type","capy.raster-tile/1")?;
            },
            RecordData::Other(descriptor)=>{
                map.serialize_entry("data",&descriptor.data)?;map.serialize_entry("encoding",&descriptor.encoding)?;map.serialize_entry("type",&descriptor.kind)?;
                for (key,value) in &descriptor.extra {map.serialize_entry(key,value)?;}
            },
        }Ok(())
    }
    pub fn fingerprint(&self,length:u64,crc:u32)->Result<[u8;32],String> {
        struct Header<'a>(&'a StoredRecord,u64,u32);
        impl Serialize for Header<'_> {
            fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
                let mut map=serializer.serialize_map(None)?;self.0.fields(&mut map,self.1,self.2)?;map.end()
            }
        }
        if matches!(self.data,RecordData::Tile(_)) {
            return Ok(Sha256::digest(serde_json::to_vec(&Header(self,length,crc)).map_err(|error|error.to_string())?).into());
        }
        let value=serde_json::to_value(Header(self,length,crc)).map_err(|error|error.to_string())?;resource_descriptor(&value)
    }
}
impl Serialize for IndexedResource {
    fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
        let mut map=serializer.serialize_map(None)?;
        self.record.fields(&mut map,self.location.length,self.location.crc)?;
        map.serialize_entry("id",&self.record.id)?;map.serialize_entry("location",&Location(&self.location))?;map.end()
    }
}
struct Decimal(u64);
impl Serialize for Decimal {fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {serializer.collect_str(&self.0)}}
struct Hex(u32);
impl std::fmt::Display for Hex {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"{:08x}",self.0)}}
impl Serialize for Hex {fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {serializer.collect_str(self)}}
struct PackName(PortableId);
impl std::fmt::Display for PackName {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {write!(f,"packs/{}.bin",self.0)}}
impl Serialize for PackName {fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {serializer.collect_str(self)}}
struct Location<'a>(&'a StoreLocation);
impl Serialize for Location<'_> {
    fn serialize<S:Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
        let range=self.0.range();let mut map=serializer.serialize_map(Some(2))?;map.serialize_entry("pack",&PackName(range.pack.id))?;map.serialize_entry("offset",&Decimal(range.offset))?;map.end()
    }
}
