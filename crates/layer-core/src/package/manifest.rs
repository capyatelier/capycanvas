use super::{archive::{Directory, Member}, parse_json, references};
use crate::authored::{Content, GraphLimits, GraphShape, PortableId, Shape, Support};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug)]
pub struct ManifestLimits { pub metadata_bytes: usize, pub graph: GraphLimits, pub resources: usize, pub traversal_nodes: usize }
impl Default for ManifestLimits {
    fn default() -> Self { Self { metadata_bytes: 64 * 1024 * 1024, graph: GraphLimits::default(), resources: 262_144, traversal_nodes: 4_194_304 } }
}
#[derive(Debug)]
pub enum ManifestRead { Known(Manifest), UnsupportedEnvelope(Value) }
#[derive(Debug)]
pub struct Manifest {
    pub document: PortableId,
    pub root: PortableId,
    pub objects: BTreeMap<PortableId, Value>,
    pub resources: BTreeMap<PortableId, ResourceRecord>,
    pub outputs: Vec<PortableId>,
    pub default_output: Option<PortableId>,
    pub metadata: Option<Value>,
    pub support: Support,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceRange { pub member: usize, pub offset: u64, pub length: u64 }
#[derive(Debug)]
pub struct ResourceRecord { pub value: Value, pub bytes: u64, pub crc32: u32, pub range: Option<ResourceRange> }

fn object(value: &Value) -> Result<&Map<String, Value>, String> { value.as_object().ok_or_else(|| "Expected package object".into()) }
fn array(value: &Value) -> Result<&[Value], String> { value.as_array().map(Vec::as_slice).ok_or_else(|| "Expected package array".into()) }
fn required<'a>(fields: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> { fields.get(key).ok_or_else(|| format!("Missing package field {key}")) }
fn string(value: &Value) -> Result<&str, String> { value.as_str().ok_or_else(|| "Expected package string".into()) }
fn identity(value: &Value) -> Result<PortableId, String> { string(value)?.parse().map_err(str::to_string) }
fn reference(value: &Value) -> Result<PortableId, String> {
    let fields = object(value)?;
    if fields.len() != 1 { return Err("Invalid reference envelope".into()); }
    identity(required(fields, "ref")?)
}
fn boolean(fields: &Map<String, Value>, key: &str) -> Result<bool, String> {
    fields.get(key).map_or(Ok(false), |v| v.as_bool().ok_or_else(|| format!("Invalid {key} flag")))
}
fn extras(fields: &Map<String, Value>, known: &[&str]) -> bool { fields.keys().any(|key| !known.contains(&key.as_str())) }
pub fn decimal_u64(value: &Value) -> Result<u64, String> {
    let value = string(value)?;
    if value.is_empty() || value.len() > 20 || value.len() > 1 && value.starts_with('0') || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Noncanonical unsigned decimal integer".into());
    }
    value.parse().map_err(|_| "Unsigned decimal integer overflow".into())
}
fn checksum(value: &Value) -> Result<u32, String> {
    let value = string(value)?;
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return Err("Invalid resource checksum".into()); }
    u32::from_str_radix(value, 16).map_err(|_| "Invalid resource checksum".into())
}
fn pack_name(name: &str) -> bool {
    name.strip_prefix("data/tiles-").and_then(|n| n.strip_suffix(".bin")).is_some_and(|n|
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit()))
}
fn data_name(name: &str) -> bool { name.strip_prefix("data/").is_some_and(|id| id.parse::<PortableId>().is_ok()) }
fn namespace(directory: &Directory) -> Result<(), String> {
    if directory.members.iter().any(|member| !matches!(member.name.as_str(), "mimetype" | "manifest.json" | "preview.png")
        && !pack_name(&member.name) && !data_name(&member.name) && !member.name.starts_with("META-INF/")) {
        return Err("Invalid package member namespace".into());
    }
    Ok(())
}
fn endpoint(value: &Value, ports: &[&str], reasons: &mut BTreeSet<&'static str>) -> Result<PortableId, String> {
    let fields = object(value)?;
    if extras(fields, &["object", "port"]) || !ports.contains(&string(required(fields, "port")?)?) { reasons.insert("Unknown evaluation endpoint"); }
    reference(required(fields, "object")?)
}
fn kind(record: &Value) -> Result<&str, String> {
    let name = string(required(object(record)?, "type")?)?;
    if name.is_empty() { return Err("Empty record type".into()); }
    Ok(name)
}
fn known_object(name: &str) -> bool { matches!(name,
    "capy.composition/1" | "capy.stack/1" | "capy.occurrence/1" | "capy.paint-source/1" | "capy.coverage-source/1" |
    "capy.effect/1" | "capy.effect-definition/1" | "capy.selection/1" | "capy.guides/1" | "capy.output/1") }
fn shape(record: &Value, refs: &[PortableId], reasons: &mut BTreeSet<&'static str>) -> Result<Shape, String> {
    let fields = object(record)?;
    let data = object(required(fields, "data")?)?;
    let ancillary = boolean(fields, "ancillary")?;
    let copy_safe = boolean(fields, "copy_safe")?;
    let name = kind(record)?;
    if copy_safe && !ancillary || known_object(name) && (ancillary || copy_safe) { return Err("Invalid ancillary flags".into()); }
    if extras(fields, &["id", "type", "data", "ancillary", "copy_safe"]) && !ancillary { reasons.insert("Unknown object record field"); }
    let unknown = || Shape::Unknown { ancillary, references: refs.to_vec() };
    Ok(match name {
        "capy.composition/1" => Shape::Composition { result: endpoint(required(data, "result")?, &["color"], reasons)? },
        "capy.stack/1" => Shape::Stack { entries: data.get("entries").map_or(Ok(Vec::new()), |v| array(v)?.iter().map(reference).collect())? },
        "capy.occurrence/1" => {
            let content = object(required(data, "content")?)?;
            let recognized: Vec<_> = ["paint", "stack", "effect", "selection"].into_iter().filter(|key| content.contains_key(*key)).collect();
            if content.is_empty() || recognized.len() > 1 { return Err("Invalid occurrence content".into()); }
            if extras(content, &["paint", "stack", "effect", "selection"]) { reasons.insert("Unknown occurrence content"); }
            let content = match recognized.first().copied() {
                Some("paint") => Some(Content::Paint(reference(&content["paint"])?)),
                Some("stack") => Some(Content::Group(reference(&content["stack"])?)),
                Some("effect") => Some(Content::Effect(reference(&content["effect"])?)),
                Some("selection") => Some(Content::Selection(reference(&content["selection"])?)),
                _ => None,
            };
            let mask = data.get("mask").map(|m| reference(required(object(m)?, "source")?)).transpose()?;
            match content { Some(content) => Shape::Occurrence { content, mask }, None => unknown() }
        },
        "capy.paint-source/1" => Shape::Paint,
        "capy.coverage-source/1" => Shape::Coverage,
        "capy.effect/1" => Shape::Effect {
            definition: reference(required(data, "definition")?)?,
            inputs: data.get("inputs").map_or(Ok(Vec::new()), |inputs| object(inputs)?.values()
                .map(|value| endpoint(value, &["color", "coverage"], reasons)).collect())?,
        },
        "capy.effect-definition/1" => Shape::Definition { dependencies: data.get("dependencies").map_or(Ok(Vec::new()), |v| array(v)?.iter().map(reference).collect())? },
        "capy.selection/1" => Shape::Selection,
        "capy.guides/1" => Shape::Guides,
        "capy.output/1" => Shape::Output { composition: endpoint(required(data, "source")?, &["color"], reasons)? },
        _ => unknown(),
    })
}
fn resource_supported(record: &Value) -> Result<bool, String> {
    let fields = object(record)?;
    let data = object(required(fields, "data")?)?;
    let encoding = string(required(fields, "encoding")?)?;
    let name = kind(record)?;
    let compressed = encoding == "capy.lz4-bytes/1";
    if compressed { decimal_u64(required(data, "decoded_bytes")?)?; }
    else if matches!(encoding, "raw" | "utf8" | "capy.lut3d-block/1" | "capy.lz4-tile/1" | "capy.lz4-coverage/1") && data.contains_key("decoded_bytes") {
        return Err("Decoded length on an unwrapped resource".into());
    }
    let (encoding_supported, keys): (bool, &[&str]) = match name {
        "capy.raster-tile/1" => (encoding == "capy.lz4-tile/1", &["channels", "depth", "transfer", "alpha", "profile"]),
        "capy.selection-coverage/1" => (encoding == "capy.lz4-coverage/1", &["depth", "extent", "bounds", "chunk"]),
        "capy.icc/1" | "capy.wgsl/1" => (compressed || encoding == if name == "capy.icc/1" { "raw" } else { "utf8" }, &["decoded_bytes"]),
        "capy.photo-metadata/1" => (compressed || encoding == "raw", &["kind", "decoded_bytes"]),
        "capy.lut3d/1" => (compressed || encoding == "capy.lut3d-block/1", &["size", "domain", "title", "decoded_bytes"]),
        _ => return Ok(false),
    };
    let mut values_supported = true;
    let enums: &[(&str, &[&str])] = match name {
        "capy.raster-tile/1" => &[("channels", &["coverage", "gray", "gray_alpha", "rgb", "rgba", "cmyk"]),
            ("depth", &["u8", "u16", "f16", "f32"]), ("transfer", &["linear", "srgb", "profile"]), ("alpha", &["none", "straight", "premultiplied_linear"])],
        "capy.selection-coverage/1" => &[("depth", &["u4", "u8"])],
        "capy.photo-metadata/1" => &[("kind", &["exif", "xmp", "iptc"])],
        _ => &[],
    };
    for (key, options) in enums { if let Some(value) = data.get(*key) { values_supported &= options.contains(&string(value)?); } }
    Ok(values_supported && encoding_supported && !extras(fields, &["id", "type", "data", "encoding", "location", "bytes", "crc32"]) && !extras(data, keys))
}
fn resource(record: Value, id: PortableId, directory: &Directory, members: &BTreeMap<&str, (usize, &Member)>) -> Result<ResourceRecord, String> {
    let fields = object(&record)?;
    kind(&record)?; object(required(fields, "data")?)?; string(required(fields, "encoding")?)?;
    let bytes = decimal_u64(required(fields, "bytes")?)?;
    let crc32 = checksum(required(fields, "crc32")?)?;
    let location = object(required(fields, "location")?)?;
    if location.contains_key("member") && location.contains_key("pack") || location.is_empty() { return Err("Invalid resource location".into()); }
    let range = if let Some(value) = location.get("member") {
        let name = string(value)?;
        if name != format!("data/{id}") { return Err("Invalid standalone resource member".into()); }
        let (member, stored) = *members.get(name).ok_or("Missing resource member")?;
        if stored.length != bytes || stored.crc32 != crc32 { return Err("Resource member length or checksum mismatch".into()); }
        Some(ResourceRange { member, offset: stored.offset, length: bytes })
    } else if let Some(value) = location.get("pack") {
        let name = string(value)?;
        if !pack_name(name) { return Err("Invalid resource pack name".into()); }
        let (member, stored) = *members.get(name).ok_or("Missing resource pack")?;
        let offset = decimal_u64(required(location, "offset")?)?;
        let end = offset.checked_add(bytes).ok_or("Resource range overflow")?;
        if end > stored.length { return Err("Resource range exceeds pack".into()); }
        Some(ResourceRange { member, offset: stored.offset.checked_add(offset).ok_or("Resource offset overflow")?, length: bytes })
    } else { None };
    if let Some(range) = range { range.offset.checked_add(range.length).filter(|end| *end <= directory.length).ok_or("Resource outside archive")?; }
    Ok(ResourceRecord { value: record, bytes, crc32, range })
}
fn overlaps(resources: &BTreeMap<PortableId, ResourceRecord>) -> Result<(), String> {
    let mut ranges: Vec<_> = resources.values().filter_map(|r| r.range.map(|range| (range, r))).collect();
    ranges.sort_by_key(|(r, _)| (r.member, r.offset, r.length));
    let mut previous: Option<(ResourceRange, &ResourceRecord)> = None;
    for (range, record) in ranges {
        if let Some((before, other)) = previous.filter(|(before, _)| before.member == range.member) {
            if before.offset == range.offset && before.length == range.length {
                let identity_field = |key: &&String| key.as_str() != "id" && key.as_str() != "location";
                if !record.value.as_object().unwrap().iter().filter(|(key, _)| identity_field(key))
                    .eq(other.value.as_object().unwrap().iter().filter(|(key, _)| identity_field(key))) {
                    return Err("Conflicting aliased resource descriptors".into());
                }
                continue;
            }
            if range.length != 0 && before.length != 0 && range.offset < before.offset + before.length { return Err("Overlapping resource ranges".into()); }
            if range.length == 0 && range.offset < before.offset + before.length { continue; }
        }
        previous = Some((range, record));
    }
    Ok(())
}

impl Manifest {
    pub fn parse(bytes: &[u8], directory: &Directory, limits: ManifestLimits) -> Result<ManifestRead, String> {
        let value = parse_json(bytes, limits.metadata_bytes)?;
        Self::from_value(value,Some(directory),limits)
    }
    pub(crate) fn transfer(value:&Value,limits:ManifestLimits)->Result<ManifestRead,String> {
        if crate::json_len(value)>limits.metadata_bytes {return Err("Transfer metadata exceeds admission".into());}
        Self::from_value(value.clone(),None,limits)
    }
    fn from_value(value:Value,directory:Option<&Directory>,limits:ManifestLimits)->Result<ManifestRead,String> {
        let fields = object(&value)?;
        let format = string(required(fields, "format")?)?;
        let version = required(fields, "version")?.as_u64().filter(|v| *v <= u32::MAX as u64).ok_or("Invalid envelope version")?;
        if format != "capy.canvas" || version != 1 || extras(fields, &["format", "version", "document", "root", "objects", "resources", "outputs", "default_output", "metadata"]) {
            return Ok(ManifestRead::UnsupportedEnvelope(value));
        }
        if let Some(directory)=directory {namespace(directory)?;}
        let members = directory.into_iter().flat_map(|d|d.members.iter()).enumerate().map(|(i, m)| (m.name.as_str(), (i, m))).collect();
        let all_refs = references(&value, limits.traversal_nodes)?;
        if all_refs.len() > limits.graph.edges { return Err("Manifest reference limit exceeded".into()); }
        let document = identity(required(fields, "document")?)?;
        let root = reference(required(fields, "root")?)?;
        let outputs = array(required(fields, "outputs")?)?.iter().map(reference).collect::<Result<Vec<_>, _>>()?;
        let default_output = fields.get("default_output").map(reference).transpose()?;
        let objects_count = array(required(fields, "objects")?)?.len();
        let resources_count = array(required(fields, "resources")?)?.len();
        if objects_count > limits.graph.objects || resources_count > limits.resources { return Err("Manifest record limit exceeded".into()); }
        let Value::Object(mut fields) = value else { unreachable!() };
        let metadata = fields.remove("metadata");
        let mut reasons = BTreeSet::new();
        if let Some(metadata) = &metadata {
            let fields = object(metadata)?;
            if extras(fields, &["exif", "xmp", "iptc"]) { reasons.insert("Unknown photo metadata binding"); }
            for key in ["exif", "xmp", "iptc"] { if let Some(value) = fields.get(key) { reference(value)?; } }
        }
        let mut objects = BTreeMap::new();
        let mut resources = BTreeMap::new();
        let mut identities = BTreeSet::from([document]);
        let Value::Array(records) = fields.remove("objects").unwrap() else { unreachable!() };
        for record in records {
            let id = identity(required(object(&record)?, "id")?)?;
            if !identities.insert(id) { return Err("Duplicate package identity".into()); }
            objects.insert(id, record);
        }
        let Value::Array(records) = fields.remove("resources").unwrap() else { unreachable!() };
        for record in records {
            let id = identity(required(object(&record)?, "id")?)?;
            if !identities.insert(id) { return Err("Duplicate package identity".into()); }
            let entry=if let Some(directory)=directory {resource(record,id,directory,&members)?} else {
                let fields=object(&record)?;kind(&record)?;object(required(fields,"data")?)?;string(required(fields,"encoding")?)?;
                let bytes=decimal_u64(required(fields,"bytes")?)?;let crc32=checksum(required(fields,"crc32")?)?;
                let location=object(required(fields,"location")?)?;
                if location.len()!=1||string(required(location,"member")?)?!=format!("data/{id}"){return Err("Invalid transfer resource location".into());}
                ResourceRecord {value:record,bytes,crc32,range:None}
            };
            resources.insert(id,entry);
        }
        if all_refs.iter().any(|id| *id == document || !identities.contains(id)) { return Err("Dangling package reference".into()); }
        let mut graph = GraphShape { outputs: outputs.clone(), default_output, resources: resources.keys().copied().collect(), ..Default::default() };
        let mut required_resources = BTreeSet::new();
        let mut ancillary_resources = BTreeSet::new();
        for (id, record) in &objects {
            let refs = references(record, limits.traversal_nodes)?;
            let shape = shape(record, &refs, &mut reasons)?;
            let ancillary = matches!(shape, Shape::Unknown { ancillary: true, .. });
            let targets = if ancillary { &mut ancillary_resources } else { &mut required_resources };
            targets.extend(refs.iter().filter(|id| resources.contains_key(id)).copied());
            graph.objects.insert(*id, shape);
        }
        for record in objects.values().chain(resources.values().map(|r| &r.value)).chain(metadata.iter()) {
            for target in references(record, limits.traversal_nodes)? {
                if matches!(graph.objects.get(&target), Some(Shape::Unknown { ancillary: true, .. })) { return Err("Reference to ancillary record".into()); }
            }
        }
        if let Some(metadata) = &metadata {
            for target in references(metadata, limits.traversal_nodes)? {
                if !resources.contains_key(&target) { return Err("Photo metadata must reference resources".into()); }
                required_resources.insert(target);
            }
        }
        for roots in [&mut required_resources, &mut ancillary_resources] {
            let mut pending: Vec<_> = roots.iter().copied().collect();
            while let Some(id) = pending.pop() {
                for target in references(&resources[&id].value, limits.traversal_nodes)? {
                    if resources.contains_key(&target) && roots.insert(target) { pending.push(target); }
                }
            }
        }
        for (id, record) in &resources {
            let location = object(required(object(&record.value)?, "location")?)?;
            let location_supported = (directory.is_none() || record.range.is_some()) && !extras(location, if location.contains_key("member") { &["member"] } else { &["pack", "offset"] });
            if !location_supported { reasons.insert("Unknown resource location"); }
            if !resource_supported(&record.value)? && (required_resources.contains(id) || !ancillary_resources.contains(id)) { reasons.insert("Unknown required resource"); }
        }
        for member in directory.into_iter().flat_map(|d|d.members.iter()) {
            if data_name(&member.name) {
                let id: PortableId = member.name[5..].parse().unwrap();
                if !resources.contains_key(&id) { return Err("Unindexed standalone resource member".into()); }
            }
        }
        overlaps(&resources)?;
        if let Support::Preserved(graph_reasons) = graph.validate(root, limits.graph)? { reasons.extend(graph_reasons); }
        let support = if reasons.is_empty() { Support::Editable } else { Support::Preserved(reasons) };
        Ok(ManifestRead::Known(Self { document, root, objects, resources, outputs, default_output, metadata, support }))
    }
}

#[cfg(test)]
mod tests;
