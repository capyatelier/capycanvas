use std::{collections::{BTreeMap, BTreeSet}, env, fs, path::PathBuf};
use fluent_syntax::{ast::{Entry, Resource}, parser, serializer};
#[path = "src/localization_inventory.rs"]
mod localization_inventory;

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/locales");
    let languages = ["en", "ja", "zh-Hans", "zh-Hant", "ko"];
    let domains = ["common", "commands", "settings", "tools", "documents", "workspace", "shortcuts", "toolbar", "resources", "creation", "color-features"];
    let mut english_sources = Vec::new();
    let mut chunks = BTreeMap::new();
    let mut keys = BTreeSet::new();
    let mut constants = BTreeSet::new();
    for language in languages {
        for domain in domains {
            let path = root.join(language).join(format!("{domain}.ftl"));
            println!("cargo:rerun-if-changed={}", path.display());
            let source = fs::read_to_string(&path).expect("Catalog must exist");
            let resource = parser::parse(source.as_str()).unwrap_or_else(|(_, errors)| panic!("{}: {errors:?}", path.display()));
            let mut groups = Vec::new();
            let mut group = String::new();
            let mut entries = 0;
            for entry in &resource.body {
                let serialized = serializer::serialize(&Resource { body: vec![entry.clone()] });
                assert!(serialized.len() <= 4096, "Catalog entry exceeds preparation chunk: {}", path.display());
                if entries == 32 || group.len() + serialized.len() > 4096 {
                    groups.push(std::mem::take(&mut group));
                    entries = 0;
                }
                group.push_str(&serialized);
                entries += 1;
            }
            if !group.is_empty() { groups.push(group); }
            chunks.insert((language, domain), groups);
            if language != "en" { continue; }
            english_sources.push((domain, source.clone()));
            for entry in &resource.body {
                if let Entry::Message(message) = entry {
                    assert!(message.value.is_some(), "English message needs a value: {}", message.id.name);
                    let key = message.id.name.to_owned();
                    let constant = key.replace('-', "_").to_ascii_uppercase();
                    assert!(key.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'), "Invalid message key: {key}");
                    assert!(keys.insert(key.clone()), "Duplicate English message: {key}");
                    assert!(constants.insert(constant), "Conflicting generated message constant: {key}");
                }
            }
        }
    }
    assert!(keys.contains("common-error"), "Generic failure message is required");
    let files: Vec<_> = english_sources.iter().map(|(domain, source)| (*domain, source.as_str())).collect();
    let inventory = localization_inventory::inventory(&files).expect("Valid canonical catalog inventory");
    let static_keys: Vec<_> = keys.iter().filter(|key| localization_inventory::requirements(key, &inventory, &mut BTreeSet::new()).expect("Valid canonical message references").is_empty()).collect();
    let mut generated = String::from("#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]\npub struct MessageId(&'static str);\nimpl MessageId {\npub const fn key(self) -> &'static str { self.0 }\n");
    for key in &keys { generated.push_str(&format!("pub const {}: Self = Self({key:?});\n", key.replace('-', "_").to_ascii_uppercase())); }
    generated.push_str("pub const ALL: &'static [Self] = &[\n");
    for key in &keys { generated.push_str(&format!("Self({key:?}),\n")); }
    generated.push_str("];\npub const STATIC: &'static [Self] = &[\n");
    for key in static_keys { generated.push_str(&format!("Self({key:?}),\n")); }
    generated.push_str("];\n}\nimpl std::borrow::Borrow<str> for MessageId { fn borrow(&self) -> &str { self.0 } }\n#[cfg(test)]\npub(crate) const CATALOGS: &[(&str, &[(&str, &str)])] = &[\n");
    for language in languages {
        generated.push_str(&format!("({language:?}, &[\n"));
        for domain in domains {
            let path = root.join(language).join(format!("{domain}.ftl")).canonicalize().unwrap();
            generated.push_str(&format!("({domain:?}, include_str!({:?})),\n", path.to_str().unwrap()));
        }
        generated.push_str("]),\n");
    }
    generated.push_str("];\n");
    generated.push_str("pub(crate) const CATALOG_CHUNKS: &[(&str, &[(&str, &str)])] = &[\n");
    for language in languages {
        generated.push_str(&format!("({language:?}, &[\n"));
        for domain in domains {
            for chunk in &chunks[&(language, domain)] { generated.push_str(&format!("({domain:?}, {chunk:?}),\n")); }
        }
        generated.push_str("]),\n");
    }
    generated.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("localization_catalogs.rs"), generated).unwrap();
}
