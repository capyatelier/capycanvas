//! Workspace components contain UTF-8 JSON. Store that text directly instead of
//! expanding each UTF-8 byte into a JSON integer. Accept existing byte arrays.
use serde::{Deserialize, Deserializer, Serializer, ser::SerializeMap};
use std::collections::BTreeMap;

pub fn serialize<S: Serializer>(
    components: &BTreeMap<String, Vec<u8>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(components.len()))?;
    for (id, bytes) in components {
        match std::str::from_utf8(bytes) {
            Ok(text) => map.serialize_entry(id, text)?,
            // Preserve corrupt legacy resources until their owner is opened;
            // one damaged workspace must not block writes to the whole catalog.
            Err(_) => map.serialize_entry(id, bytes)?,
        }
    }
    map.end()
}

pub fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<u8>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Component {
        Text(String),
        Legacy(Vec<u8>),
    }
    Ok(BTreeMap::<String, Component>::deserialize(deserializer)?
        .into_iter()
        .map(|(id, value)| {
            (
                id,
                match value {
                    Component::Text(s) => s.into_bytes(),
                    Component::Legacy(b) => b,
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Serialize, Deserialize)]
    struct Record {
        #[serde(with = "super")]
        components: BTreeMap<String, Vec<u8>>,
    }
    #[test]
    fn legacy_bytes_and_unicode_text_keep_identical_content_hashes() {
        let bytes =
            serde_json::to_vec(&serde_json::json!({"label":"照片 🎨", "range":[-8.0, 1.5]}))
                .unwrap();
        let components = BTreeMap::from([(crate::content_id(&bytes), bytes)]);
        let old = serde_json::json!({"components": components});
        let decoded: Record = serde_json::from_value(old.clone()).unwrap();
        let compact = serde_json::to_vec(&decoded).unwrap();
        assert!(compact.len() < serde_json::to_vec(&old).unwrap().len());
        let read: Record = serde_json::from_slice(&compact).unwrap();
        assert_eq!(read.components, components);
        let corrupt = Record { components: BTreeMap::from([("unopened".into(), vec![255])]) };
        let saved = serde_json::to_vec(&corrupt).unwrap();
        assert_eq!(serde_json::from_slice::<Record>(&saved).unwrap().components, corrupt.components);
    }
}
