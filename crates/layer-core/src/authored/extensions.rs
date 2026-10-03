use super::PortableId;
use crate::package::{ByteRange, ImmutableBacking, RangeState, MAX_RANGE_BYTES, manifest::Manifest, references};
use serde_json::{Map, Value, json};
use std::{collections::{BTreeMap, BTreeSet}, fmt, sync::{Arc, atomic::{AtomicBool, Ordering}}};

const REFERENCE_NODES: usize = 4_194_304;

#[derive(Clone, Debug, Default)]
pub struct Extensions {
    pub records: BTreeMap<PortableId, Value>,
    pub resources: BTreeMap<PortableId, Arc<OpaqueResource>>,
}
#[derive(Clone)]
pub struct OpaqueResource {
    pub id: PortableId,
    pub kind: Arc<str>,
    pub data: Value,
    pub encoding: Arc<str>,
    pub extra_fields: Map<String, Value>,
    pub backing: ImmutableBacking,
    pub offset: u64,
    pub length: u64,
    pub crc32: u32,
}
impl fmt::Debug for OpaqueResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpaqueResource").field("id", &self.id).field("kind", &self.kind)
            .field("encoding", &self.encoding).field("owner", &self.backing.identity())
            .field("offset", &self.offset).field("length", &self.length).field("crc32", &self.crc32).finish()
    }
}
fn check(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) { Err("Package operation cancelled".into()) } else { Ok(()) }
}
impl OpaqueResource {
    pub fn record(&self, location: Value) -> Value {
        let mut fields = self.extra_fields.clone();
        for (key, value) in [("id", json!(self.id)), ("type", json!(self.kind)), ("data", self.data.clone()),
            ("encoding", json!(self.encoding)), ("location", location), ("bytes", self.length.to_string().into()),
            ("crc32", format!("{:08x}", self.crc32).into())] { fields.insert(key.into(), value); }
        Value::Object(fields)
    }
    pub fn read_chunk(&self, offset: u64, length: usize, cancelled: &AtomicBool) -> Result<ByteRange, String> {
        check(cancelled)?;
        if offset.checked_add(length as u64).is_none_or(|end| end > self.length) { return Err("Opaque resource read exceeds range".into()); }
        let absolute = self.offset.checked_add(offset).ok_or("Opaque resource offset overflow")?;
        let bytes = match self.backing.poll(absolute, length)? {
            RangeState::Pending => return Err("Package bytes are pending".into()),
            RangeState::Ready(bytes) => bytes,
        };
        check(cancelled)?;
        Ok(bytes)
    }
    pub fn verify(&self, cancelled: &AtomicBool) -> Result<(), String> {
        check(cancelled)?;
        self.offset.checked_add(self.length).filter(|end| *end <= self.backing.byte_len()).ok_or("Opaque resource exceeds backing")?;
        if self.length == 0 { self.read_chunk(0, 0, cancelled)?; }
        let mut offset = 0;
        let mut checksum = crc32fast::Hasher::new();
        while offset < self.length {
            let length = (self.length - offset).min(MAX_RANGE_BYTES as u64) as usize;
            checksum.update(&self.read_chunk(offset, length, cancelled)?);
            offset += length as u64;
        }
        if checksum.finalize() != self.crc32 { return Err(self.backing.fail("Opaque resource checksum failed".into())); }
        check(cancelled)
    }
    fn references(&self) -> Result<Vec<PortableId>, String> {
        let mut result = references(&self.data, REFERENCE_NODES)?;
        for value in self.extra_fields.values() { result.extend(references(value, REFERENCE_NODES)?); }
        if result.len() > REFERENCE_NODES { return Err("Opaque resource references exceed limit".into()); }
        Ok(result)
    }
}
impl Extensions {
    pub fn load(manifest: &Manifest, backing: &ImmutableBacking, cancelled: &AtomicBool) -> Result<Self, String> {
        check(cancelled)?;
        let records: BTreeMap<_, _> = manifest.objects.iter().filter(|(_, record)| record["ancillary"] == true)
            .map(|(id, record)| (*id, record.clone())).collect();
        let mut needed = BTreeSet::new();
        let mut pending = Vec::new();
        for record in records.values() {
            for id in references(record, REFERENCE_NODES)? {
                if manifest.resources.contains_key(&id) && needed.insert(id) { pending.push(id); }
            }
        }
        while let Some(id) = pending.pop() {
            check(cancelled)?;
            for target in references(&manifest.resources[&id].value, REFERENCE_NODES)? {
                if manifest.resources.contains_key(&target) && needed.insert(target) { pending.push(target); }
            }
        }
        let mut resources = BTreeMap::new();
        for id in needed {
            check(cancelled)?;
            let record = &manifest.resources[&id];
            let range = record.range.ok_or("Unknown ancillary resource location")?;
            let fields = record.value.as_object().ok_or("Invalid ancillary resource")?;
            let kind = fields.get("type").and_then(Value::as_str).ok_or("Missing ancillary resource type")?;
            let encoding = fields.get("encoding").and_then(Value::as_str).ok_or("Missing ancillary resource encoding")?;
            let data = fields.get("data").ok_or("Missing ancillary resource descriptor")?.clone();
            let extra_fields = fields.iter().filter(|(key, _)| !["id", "type", "data", "encoding", "location", "bytes", "crc32"].contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone())).collect();
            let resource = Arc::new(OpaqueResource { id, kind:kind.into(), data, encoding:encoding.into(), extra_fields,
                backing:backing.clone(), offset:range.offset, length:record.bytes, crc32:record.crc32 });
            resource.verify(cancelled)?;
            resources.insert(id, resource);
        }
        check(cancelled)?;
        Ok(Self { records, resources })
    }
    pub fn edited_retained(&self, live_object_ids: &BTreeSet<PortableId>) -> Result<(Vec<Value>, Vec<Arc<OpaqueResource>>), String> {
        let mut records = Vec::new();
        let mut resources = BTreeSet::new();
        for record in self.records.values().filter(|record| record["copy_safe"] == true) {
            let mut closure = BTreeSet::new();
            let mut pending = references(record, REFERENCE_NODES)?;
            let mut resolves = true;
            while let Some(id) = pending.pop() {
                if live_object_ids.contains(&id) { continue; }
                let Some(resource) = self.resources.get(&id) else { resolves = false; break; };
                if closure.insert(id) { pending.extend(resource.references()?); }
            }
            if resolves { records.push(record.clone()); resources.extend(closure); }
        }
        Ok((records, resources.into_iter().map(|id| self.resources[&id].clone()).collect()))
    }
}

#[cfg(test)]
mod tests;
