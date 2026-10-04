use crate::authored::PortableId;
use serde::{Deserialize, Deserializer, de::{Error, MapAccess, SeqAccess, Visitor}};
use serde_json::{Map, Number, Value};
use std::{collections::BTreeMap, fmt};
use super::values::{DecodeError, DecodeResult};

pub const MAX_JSON_DEPTH: usize = 128;

struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("JSON without duplicate keys") }
            fn visit_bool<E: Error>(self, v: bool) -> Result<Unique, E> { Ok(Unique(Value::Bool(v))) }
            fn visit_i64<E: Error>(self, v: i64) -> Result<Unique, E> { Ok(Unique(Value::Number(v.into()))) }
            fn visit_u64<E: Error>(self, v: u64) -> Result<Unique, E> { Ok(Unique(Value::Number(v.into()))) }
            fn visit_f64<E: Error>(self, v: f64) -> Result<Unique, E> {
                Number::from_f64(v).map(|v| Unique(Value::Number(v))).ok_or_else(|| E::custom("Non-finite JSON number"))
            }
            fn visit_str<E: Error>(self, v: &str) -> Result<Unique, E> { Ok(Unique(Value::String(v.into()))) }
            fn visit_string<E: Error>(self, v: String) -> Result<Unique, E> { Ok(Unique(Value::String(v))) }
            fn visit_unit<E: Error>(self) -> Result<Unique, E> { Ok(Unique(Value::Null)) }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = seq.next_element()? { values.push(value); }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Unique, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) { return Err(A::Error::custom("Duplicate JSON key")); }
                    let Unique(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}

pub fn parse_json(bytes: &[u8], limit: usize) -> DecodeResult<Value> {
    if bytes.len() > limit { return Err(DecodeError::Unsupported("Package metadata exceeds limit".into())); }
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for byte in bytes {
        if quoted {
            if escaped { escaped = false; } else if *byte == b'\\' { escaped = true; } else if *byte == b'"' { quoted = false; }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => { depth += 1; if depth >= MAX_JSON_DEPTH { return Err(DecodeError::Unsupported("Package JSON depth exceeds limit".into())); } },
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {},
            }
        }
    }
    serde_json::from_slice::<Unique>(bytes).map(|value| value.0).map_err(|e| DecodeError::Invalid(e.to_string()))
}

pub fn references(value: &Value, limit: usize) -> DecodeResult<Vec<PortableId>> {
    let mut pending = vec![(value, 0)];
    let mut result = Vec::new();
    let mut visited = 0usize;
    while let Some((value, depth)) = pending.pop() {
        visited = visited.checked_add(1).ok_or("Reference traversal overflow")?;
        if visited > limit || depth >= MAX_JSON_DEPTH || depth == MAX_JSON_DEPTH-1 && matches!(value,Value::Object(_)|Value::Array(_)) { return Err(DecodeError::Unsupported("Reference traversal exceeds limit".into())); }
        match value {
            Value::Object(fields) => {
                if let Some(reference) = fields.get("ref") {
                    if fields.len() != 1 { return Err("Invalid reference envelope".into()); }
                    result.push(reference.as_str().ok_or("Invalid reference identity")?.parse()?);
                } else {
                    if fields.len() > limit.saturating_sub(visited + pending.len()) { return Err(DecodeError::Unsupported("Reference traversal exceeds limit".into())); }
                    pending.extend(fields.values().map(|value| (value, depth + 1)));
                }
            }
            Value::Array(values) => {
                if values.len() > limit.saturating_sub(visited + pending.len()) { return Err(DecodeError::Unsupported("Reference traversal exceeds limit".into())); }
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            _ => (),
        }
    }
    Ok(result)
}

pub fn remap_references(value: &mut Value, identities: &BTreeMap<PortableId, PortableId>, limit: usize) -> DecodeResult<()> {
    references(value, limit)?;
    fn remap(value: &mut Value, identities: &BTreeMap<PortableId, PortableId>) {
        match value {
            Value::Object(fields) => {
                if let Some(reference) = fields.get_mut("ref") {
                    if let Some(replacement) = reference.as_str().and_then(|s| s.parse().ok()).and_then(|id| identities.get(&id)) {
                        *reference = Value::String(replacement.to_string());
                    }
                } else { for value in fields.values_mut() { remap(value, identities); } }
            }
            Value::Array(values) => { for value in values { remap(value, identities); } }
            _ => (),
        }
    }
    remap(value, identities);
    Ok(())
}
