//! Content-addressed ICC library policy. Hosts enumerate app-owned storage and
//! perform bounded reads / atomic writes under their native storage lock.
use layer_core::color::{ColorProfile, ProfileChannels};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::ColorFeatureError;

pub const PROFILE_LIBRARY_ENTRIES: usize = 128;
pub const PROFILE_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;
pub const PROFILE_READ_LIMIT: usize = layer_color::MAX_ICC_BYTES;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub id: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<ColorFeatureError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub id: String,
    pub bytes: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<ProfileChannels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ColorProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<ColorFeatureError>,
}
impl ProfileEntry {
    pub fn localized_view(&self, localizer: &crate::Localizer) -> serde_json::Value {
        let mut value = serde_json::json!(self);
        value["name"] = self.display_name(localizer).into();
        value["details"] = self.details(localizer).into();
        if let Some(reason) = &self.issue { value["issue"] = reason.profile_message(localizer).into(); }
        value
    }
    pub fn display_name(&self, localizer: &crate::Localizer) -> String {
        if self.channels.is_some() {
            return if self.name.is_empty() { localizer.text(crate::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string() } else { self.name.clone() };
        }
        let id = self.id.chars().take(12).collect::<String>();
        let mut args = crate::FluentArgs::new(); args.set("id", id.as_str());
        localizer.format(crate::MessageId::COLOR_FEATURES_PROFILE_UNAVAILABLE, &args)
    }
    pub fn details(&self, localizer: &crate::Localizer) -> String {
        let mut args = crate::FluentArgs::new();
        let gray = localizer.text(crate::MessageId::COLOR_FEATURES_PROFILE_GRAYSCALE);
        let channels = match self.channels { Some(ProfileChannels::Rgb) => "RGB", Some(ProfileChannels::Cmyk) => "CMYK", Some(ProfileChannels::Gray) => gray.as_ref(), None => "" };
        let bytes = self.bytes.to_string(); let id = self.id.chars().take(12).collect::<String>();
        args.set("channels", channels); args.set("bytes", bytes.as_str()); args.set("id", id.as_str());
        localizer.format(crate::MessageId::COLOR_FEATURES_PROFILE_DETAILS, &args)
    }
}
pub fn profile_description_name(description: Option<String>, localizer: &crate::Localizer) -> String {
    description.unwrap_or_else(|| localizer.text(crate::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).to_string())
}
pub fn profile_display_name(profile: &ColorProfile, localizer: &crate::Localizer) -> Result<String, String> {
    Ok(profile_description_name(layer_color::profile_description_optional(profile)?, localizer))
}
pub fn validate_source_profile(profile: &crate::ExportProfile, source: &layer_core::color::source::SourceInterpretation, working: layer_core::color::RgbSpace) -> Result<(), ColorFeatureError> {
    use layer_core::color::source::SourceChannels;
    let expected = match source.channels { SourceChannels::Rgb | SourceChannels::Rgba => ProfileChannels::Rgb, SourceChannels::Gray | SourceChannels::GrayAlpha => ProfileChannels::Gray, SourceChannels::Cmyk => ProfileChannels::Cmyk };
    if profile.channels != expected { return Err(ColorFeatureError::SourceChannels(expected)); }
    let mut source = source.clone(); source.profile = profile.profile.clone();
    layer_color::WorkingDecoder::new(&source, working, Default::default())?;
    Ok(())
}
pub fn valid_profile_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn profile_identity(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn profile_inventory(mut records: Vec<ProfileRecord>) -> Vec<ProfileRecord> {
    records.retain(|r| valid_profile_id(&r.id));
    records.sort_by(|a, b| a.id.cmp(&b.id));
    records.dedup_by(|a, b| a.id == b.id);
    let mut total = 0u64;
    for (index, record) in records.iter_mut().enumerate() {
        total = total.saturating_add(record.bytes);
        record.issue = if index >= PROFILE_LIBRARY_ENTRIES || total > PROFILE_LIBRARY_BYTES {
            Some(ColorFeatureError::ProfileLibraryLimit)
        } else if record.bytes > PROFILE_READ_LIMIT as u64 {
            Some(ColorFeatureError::ProfileReadLimit)
        } else {
            None
        };
    }
    // Entries beyond the limit remain visible and removable, never silently
    // excluded from quota accounting or made impossible to repair.
    records
}
pub fn read_library_profile(
    id: &str,
    bytes: &[u8],
    include_bytes: bool,
) -> Result<ProfileEntry, ColorFeatureError> {
    if !valid_profile_id(id) {
        return Err(ColorFeatureError::SelectImportedProfile);
    }
    if bytes.len() > PROFILE_READ_LIMIT {
        return Err(ColorFeatureError::ProfileReadLimit);
    }
    if profile_identity(bytes) != id {
        return Err(ColorFeatureError::ProfileChanged);
    }
    let profile = ColorProfile::Icc(bytes.to_vec().into());
    Ok(ProfileEntry {
        id: id.into(),
        bytes: bytes.len() as u64,
        name: layer_color::profile_description_optional(&profile)?.unwrap_or_default(),
        channels: Some(layer_color::profile_channels(&profile)?),
        profile: include_bytes.then_some(profile),
        issue: None,
    })
}
pub fn inspect_library_entry(record: &ProfileRecord, bytes: Result<&[u8], ColorFeatureError>) -> ProfileEntry {
    let result = record.issue.clone().map_or_else(
        || bytes.and_then(|b| read_library_profile(&record.id, b, false)),
        Err,
    );
    result.unwrap_or_else(|issue| ProfileEntry {
        id: record.id.clone(),
        bytes: record.bytes,
        name: String::new(),
        channels: None,
        profile: None,
        issue: Some(issue),
    })
}
pub fn prepare_profile_import(
    records: Vec<ProfileRecord>,
    bytes: &[u8],
) -> Result<ProfileEntry, ColorFeatureError> {
    if bytes.len() > PROFILE_READ_LIMIT {
        return Err(ColorFeatureError::ProfileReadLimit);
    }
    let id = profile_identity(bytes);
    let entry = read_library_profile(&id, bytes, true)?;
    let records = profile_inventory(records);
    let other: Vec<_> = records.iter().filter(|r| r.id != id).collect();
    if other.len() >= PROFILE_LIBRARY_ENTRIES
        || other
            .iter()
            .fold(bytes.len() as u64, |total, r| total.saturating_add(r.bytes))
            > PROFILE_LIBRARY_BYTES
    {
        return Err(ColorFeatureError::ProfileLibraryLimit);
    }
    Ok(entry)
}

/// Hidden ids as any build saved them; entries this build cannot read are dropped.
fn saved_ids<'de, D: serde::Deserializer<'de>>(saved: D) -> Result<Vec<String>, D::Error> {
    let saved = serde_json::Value::deserialize(saved)?;
    Ok(saved
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(Into::into))
        .collect())
}
/// The same stateless transport for Wasm workers and JNI. File paths never
/// enter this contract; a Remove result can only name an app-owned object.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileLibraryAction {
    Limits,
    Visibility {
        #[serde(deserialize_with = "saved_ids")]
        hidden: Vec<String>,
        id: Option<String>,
        visible: Option<bool>,
    },
    Inventory {
        entries: Vec<ProfileRecord>,
    },
    Import {
        entries: Vec<ProfileRecord>,
    },
    Get {
        id: String,
    },
    Inspect {
        entry: ProfileRecord,
        error: Option<ColorFeatureError>,
    },
    Remove {
        id: String,
    },
}
impl ProfileLibraryAction {
    pub fn execute_localized(self, bytes: &[u8], localizer: &crate::Localizer) -> Result<serde_json::Value, ColorFeatureError> {
        self.execute_impl(bytes, Some(localizer))
    }
    pub fn execute(self, bytes: &[u8]) -> Result<serde_json::Value, ColorFeatureError> {
        self.execute_impl(bytes, None)
    }
    fn execute_impl(self, bytes: &[u8], localizer: Option<&crate::Localizer>) -> Result<serde_json::Value, ColorFeatureError> {
        use serde_json::json;
        let view = |entry: ProfileEntry| localizer.map_or_else(||json!(&entry), |localizer|entry.localized_view(localizer));
        Ok(match self {
            Self::Limits => json!({"read_bytes": PROFILE_READ_LIMIT}),
            Self::Visibility { mut hidden, id, visible } => {
                hidden.retain(|id| valid_profile_id(id));
                hidden.sort(); hidden.dedup();
                if let Some(id) = id {
                    if !valid_profile_id(&id) { return Err(ColorFeatureError::SelectImportedProfile); }
                    hidden.retain(|key| key != &id);
                    if visible == Some(false) { hidden.push(id); }
                }
                if hidden.len() > PROFILE_LIBRARY_ENTRIES { return Err(ColorFeatureError::ProfileHiddenLimit); }
                json!(hidden)
            }
            Self::Inventory { entries } => json!(profile_inventory(entries)),
            Self::Import { entries } => view(prepare_profile_import(entries, bytes)?),
            Self::Get { id } => view(read_library_profile(&id, bytes, true)?),
            Self::Inspect { entry, error } => {
                view(inspect_library_entry(&entry, error.map_or(Ok(bytes), Err)))
            }
            Self::Remove { id } => {
                if !valid_profile_id(&id) {
                    return Err(ColorFeatureError::SelectImportedProfile);
                }
                json!({"id":id})
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artist_entry_projection_keeps_literal_names_and_localizes_absence() {
        let localizer = crate::Localizer::shared(crate::UiLanguage::English);
        let entry = ProfileEntry { id: "a".repeat(64), name: "Embedded ICC profile 私の色".into(), bytes: 128, channels: Some(ProfileChannels::Gray), profile: None, issue: None };
        let view = entry.localized_view(&localizer);
        assert_eq!(view["name"], entry.name);
        assert_eq!(view["details"], entry.details(&localizer));
        let missing = ProfileEntry { channels: None, name: String::new(), issue: Some(ColorFeatureError::ProfileMissing), ..entry };
        let view = missing.localized_view(&localizer);
        assert_eq!(view["name"], missing.display_name(&localizer));
        assert_eq!(view["issue"], ColorFeatureError::ProfileMissing.profile_message(&localizer));
    }
    #[test]
    fn import_deduplicates_accounts_all_entries_and_preserves_exact_owned_bytes() {
        let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(
            layer_core::color::RgbSpace::DisplayP3,
        ))
        .unwrap();
        let entry = prepare_profile_import(vec![], &bytes).unwrap();
        assert_eq!(entry.profile, Some(ColorProfile::Icc(bytes.clone().into())));
        let record = ProfileRecord {
            id: entry.id.clone(),
            bytes: bytes.len() as u64,
            issue: None,
        };
        assert_eq!(
            prepare_profile_import(vec![record.clone()], &bytes)
                .unwrap()
                .id,
            entry.id
        );
        assert!(read_library_profile(&entry.id, b"corrupt", true).is_err());
        assert!(
            inspect_library_entry(&record, Err("Missing object".into()))
                .issue
                .is_some()
        );
        let records: Vec<_> = (0..128)
            .map(|i| ProfileRecord {
                id: format!("{i:064x}"),
                bytes: 1,
                issue: None,
            })
            .collect();
        assert!(prepare_profile_import(records.clone(), &bytes).is_err());
        let mut oversized = records.clone();
        oversized[0].bytes = PROFILE_LIBRARY_BYTES;
        assert!(prepare_profile_import(oversized.clone(), &bytes).is_err());
        assert!(profile_inventory(oversized).last().unwrap().issue.is_some());
        let mut records = records;
        records.push(record.clone());
        assert_eq!(profile_inventory(records).len(), 129);
        assert!(
            prepare_profile_import(
                vec![ProfileRecord {
                    bytes: PROFILE_LIBRARY_BYTES,
                    ..record
                }],
                &bytes
            )
            .is_ok(),
            "repair replaces an existing corrupt entry"
        );
        assert!(
            ProfileLibraryAction::Remove {
                id: "../source.icc".into()
            }
            .execute(&[])
            .is_err()
        );
        assert!(!valid_profile_id("original.icc"));
        assert!(prepare_profile_import(vec![], &vec![0; PROFILE_READ_LIMIT + 1]).is_err());
    }
    #[test]
    fn hidden_profiles_saved_by_any_build_read_what_this_build_can() {
        let id = "a".repeat(64);
        for (hidden, expected) in [
            (serde_json::json!([id, 7, {"id": id}, "original.icc"]), serde_json::json!([id])),
            (serde_json::json!({"hidden": [id]}), serde_json::json!([])),
            (serde_json::json!("broken"), serde_json::json!([])),
        ] {
            let action: ProfileLibraryAction = serde_json::from_value(
                serde_json::json!({"type": "visibility", "hidden": hidden, "id": null, "visible": null}),
            )
            .unwrap();
            assert_eq!(action.execute(&[]).unwrap(), expected);
        }
    }
}

#[cfg(test)]
mod localized_label_tests {
    use super::*;
    #[test]
    fn absent_profile_name_uses_only_explicit_absence() {
        let context = crate::Localizer::shared(crate::UiLanguage::Japanese);
        assert_eq!(profile_description_name(None, &context), context.text(crate::MessageId::COLOR_FEATURES_PROFILE_EMBEDDED).as_ref());
        for name in ["Embedded ICC profile", "埋め込みICCプロファイル", "내 프로파일", ""] {
            assert_eq!(profile_description_name(Some(name.into()), &context), name);
        }
    }
    #[test]
    fn imported_profile_descriptions_remain_literal() {
        let context = crate::Localizer::shared(crate::UiLanguage::Japanese);
        for name in ["Embedded ICC profile", "Unavailable profile", "埋め込みICCプロファイル", "내 프로파일"] {
            let entry = ProfileEntry { id: "literal-profile-id".into(), name: name.into(), channels: Some(ProfileChannels::Gray), bytes: 256, profile: None, issue: None };
            assert_eq!(entry.display_name(&context), name);
            assert!(entry.details(&context).contains("256"));
            assert_eq!(entry.id, "literal-profile-id");
        }
    }
}
