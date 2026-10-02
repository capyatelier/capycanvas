use std::{collections::BTreeSet, env, fs, path::PathBuf};
use fluent_syntax::{ast::Entry, parser};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/locales");
    let languages = ["en", "ja", "zh-Hans", "zh-Hant", "ko"];
    let domains = ["common", "commands", "settings", "tools", "documents", "workspace"];
    let mut keys = BTreeSet::new();
    let mut constants = BTreeSet::new();
    for language in languages {
        for domain in domains {
            let path = root.join(language).join(format!("{domain}.ftl"));
            println!("cargo:rerun-if-changed={}", path.display());
            if language != "en" { continue; }
            let source = fs::read_to_string(&path).expect("English catalog must exist");
            let resource = parser::parse(source.as_str()).unwrap_or_else(|(_, errors)| panic!("{}: {errors:?}", path.display()));
            for entry in resource.body {
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
    let mut generated = String::from("#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]\npub struct MessageId(&'static str);\nimpl MessageId {\npub const fn key(self) -> &'static str { self.0 }\n");
    for key in &keys { generated.push_str(&format!("pub const {}: Self = Self({key:?});\n", key.replace('-', "_").to_ascii_uppercase())); }
    generated.push_str("pub const ALL: &'static [Self] = &[\n");
    for key in &keys { generated.push_str(&format!("Self({key:?}),\n")); }
    generated.push_str("];\n}\npub(crate) const CATALOGS: &[(&str, &[(&str, &str)])] = &[\n");
    for language in languages {
        generated.push_str(&format!("({language:?}, &[\n"));
        for domain in domains {
            let path = root.join(language).join(format!("{domain}.ftl")).canonicalize().unwrap();
            generated.push_str(&format!("({domain:?}, include_str!({:?})),\n", path.to_str().unwrap()));
        }
        generated.push_str("]),\n");
    }
    generated.push_str("];\n");
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("localization_catalogs.rs"), generated).unwrap();
}
