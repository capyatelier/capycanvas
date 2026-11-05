//! Content-addressed ICC library policy. Hosts enumerate app-owned storage and
//! perform bounded reads / atomic writes under their native storage lock.
use layer_core::color::{ColorProfile, ProfileChannels};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::ColorFeatureError;

pub const PROFILE_LIBRARY_ENTRIES: usize = 128;
pub const PROFILE_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;
pub const PROFILE_READ_LIMIT: usize = layer_color::MAX_ICC_BYTES;
pub const LIBRARY_SIZE: [i32; 2] = [480, 500];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "source", rename_all = "snake_case")]
pub enum ProfilePurpose {
    Proof,
    Output,
    Source(layer_core::color::source::SourceInterpretation),
}
impl ProfilePurpose {
    pub fn picker_purpose(&self) -> ProfilePickerPurpose {
        match self { Self::Proof=>ProfilePickerPurpose::Proof, Self::Output=>ProfilePickerPurpose::Output,
            Self::Source(source)=>ProfilePickerPurpose::Source(source.channels) }
    }

    pub fn validate(
        &self,
        profile: &crate::ExportProfile,
        working: layer_core::color::RgbSpace,
    ) -> Result<(), ColorFeatureError> {
        match self {
            Self::Proof => {
                let recipe = layer_core::color::ProofRecipe::new(
                    profile.name.clone(),
                    profile.profile.clone(),
                );
                layer_color::ProofTransform::new(working, &recipe)?;
            }
            Self::Output => {
                let recipe = crate::ExportRecipe {
                    format: crate::ExportFormat::Tiff,
                    profile: profile.clone(),
                    background: crate::ExportBackground::White,
                    ..crate::ExportRecipe::web_share()
                };
                let encoder = layer_color::WorkingEncoder::new(
                    working,
                    &recipe.interpretation(),
                    Default::default(),
                )?;
                let mut output = vec![0; recipe.interpretation().pixel_bytes() * 3];
                encoder.encode_straight(
                    &[[0., 0., 0., 1.], [0.5, 0.5, 0.5, 1.], [1.; 4]],
                    &mut output,
                    None,
                    [0, 0],
                )?;
            }
            Self::Source(source) => {
                validate_source_profile(profile,source,working)?;
            }
        }
        Ok(())
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channels", rename_all = "snake_case")]
pub enum ProfilePickerPurpose { Output, Proof, Source(layer_core::color::source::SourceChannels) }
impl ProfilePickerPurpose {
    pub fn accepts(&self, channels: ProfileChannels) -> bool {
        use layer_core::color::source::SourceChannels;
        match self {
            Self::Output => true,
            Self::Proof => channels != ProfileChannels::Gray,
            Self::Source(source) => channels == match source {
                SourceChannels::Rgb | SourceChannels::Rgba => ProfileChannels::Rgb,
                SourceChannels::Gray | SourceChannels::GrayAlpha => ProfileChannels::Gray,
                SourceChannels::Cmyk => ProfileChannels::Cmyk,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfilePickerSaved {
    pub id: String,
    pub visible: bool,
    pub channels: Option<ProfileChannels>,
    pub valid: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfilePickerInventory {
    pub document: bool,
    pub current: bool,
    pub saved: Vec<usize>,
    pub standard: bool,
}
pub fn picker_inventory(purpose: &ProfilePickerPurpose, document: Option<&ColorProfile>, current: Option<&ColorProfile>, saved: &[ProfilePickerSaved]) -> ProfilePickerInventory {
    let identity = |profile: Option<&ColorProfile>| match profile { Some(ColorProfile::Icc(bytes))=>Some(profile_identity(bytes)), _=>None };
    let document = identity(document);
    let current = identity(current);
    ProfilePickerInventory {
        current: current.as_ref().is_some_and(|id| Some(id)!=document.as_ref() && !saved.iter().any(|entry|entry.valid && &entry.id==id)),
        saved: saved.iter().enumerate().filter(|(_,entry)|Some(&entry.id)!=document.as_ref() && entry.visible && entry.valid && entry.channels.is_some_and(|channels|purpose.accepts(channels))).map(|(index,_)|index).collect(),
        document: document.is_some(),
        standard: purpose.accepts(ProfileChannels::Rgb),
    }
}

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
    pub fn matches(&self, query: &str, localizer: &crate::Localizer) -> bool {
        let query = crate::search::normalize(query);
        [&self.name, &self.id, &self.display_name(localizer), &self.details(localizer)]
            .iter().any(|text| crate::search::normalize(text).contains(&query))
    }
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
        let id = self.id.chars().take(12).collect::<String>();
        args.set("channels", channels); args.set("bytes", self.bytes); args.set("id", id.as_str());
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
    InspectMetadata {
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
            Self::Limits => json!({"read_bytes": PROFILE_READ_LIMIT, "entries": PROFILE_LIBRARY_ENTRIES}),
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
            Self::InspectMetadata { entry, error } => Self::Inspect { entry, error }.execute(bytes)?,
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
    fn entry_search_matches_current_captions_and_normalized_literal_names() {
        let mut entry = ProfileEntry { id: "a".repeat(64), name: String::new(), bytes: 2602,
            channels: Some(ProfileChannels::Gray), profile: None, issue: None };
        for language in crate::UiLanguage::ALL {
            let localizer = crate::Localizer::shared(language);
            assert!(entry.matches(&entry.display_name(&localizer), &localizer));
            assert!(entry.matches(&entry.details(&localizer), &localizer));
            entry.channels = None;
            assert!(entry.matches(&entry.display_name(&localizer), &localizer));
            entry.channels = Some(ProfileChannels::Gray);
        }
        let localizer = crate::Localizer::shared(crate::UiLanguage::English);
        for (name, query) in [("Işık İçe aktar", "IŞIK İÇE AKTAR"), ("Größe", "GRÖSSE"),
            ("Tiếng Việt", "Tie\u{302}\u{301}ng Vie\u{323}\u{302}t")] {
            entry.name = name.into();
            assert!(entry.matches(query, &localizer));
            assert_eq!(entry.name, name);
        }
        assert!(!entry.matches("Tieng Viet", &localizer));
    }
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
    fn retained_profile_metadata_reprojects_without_icc_bytes_or_losing_raw_reasons() {
        use crate::{ColorUiRequest, Localizer, UiLanguage, color_ui_localized};
        let literal = "Embedded ICC profile 私の色 { $name }";
        let diagnostic = "Missing file 私の色 { $name }";
        let mut entries = vec![
            ProfileEntry { id: "a".repeat(64), bytes: 2602, name: String::new(), channels: Some(ProfileChannels::Gray), profile: None, issue: None },
            ProfileEntry { id: "b".repeat(64), bytes: 128, name: literal.into(), channels: Some(ProfileChannels::Rgb), profile: None, issue: None },
        ];
        for issue in [ColorFeatureError::ProfileMissing, ColorFeatureError::Diagnostic(diagnostic.into())] {
            let raw = ProfileLibraryAction::InspectMetadata {
                entry: ProfileRecord { id: "c".repeat(64), bytes: 17, issue: None }, error: Some(issue.clone()),
            }.execute_localized(&[], &Localizer::shared(UiLanguage::Japanese)).unwrap();
            let entry: ProfileEntry = serde_json::from_value(raw).unwrap();
            assert_eq!(entry.issue, Some(issue)); assert!(entry.profile.is_none());
            entries.push(entry);
        }
        let payload = serde_json::to_value(&entries).unwrap();
        assert_eq!(payload[0]["name"], ""); assert_eq!(payload[1]["name"], literal);
        let mut views = Vec::new();
        for language in [UiLanguage::English, UiLanguage::Japanese, UiLanguage::English] {
            let localizer = Localizer::shared(language);
            let request: ColorUiRequest = serde_json::from_value(serde_json::json!({"type":"profile_entries", "entries":payload})).unwrap();
            let view = color_ui_localized(request, &localizer).unwrap();
            for (entry, row) in entries.iter().zip(view.as_array().unwrap()) {
                assert_eq!(*row, entry.localized_view(&localizer));
                assert!(row["profile"].is_null());
            }
            assert_eq!(view[1]["name"], literal);
            assert!(view[3]["issue"].as_str().unwrap().ends_with(diagnostic));
            views.push(view);
        }
        assert_ne!(views[0][0]["name"], views[1][0]["name"]);
        assert_ne!(views[0][0]["details"], views[1][0]["details"]);
        assert_ne!(views[0][2]["issue"], views[1][2]["issue"]);
        assert_eq!(views[0], views[2]);
        assert_eq!(serde_json::to_value(&entries).unwrap(), payload);
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

#[cfg(test)]
mod picker_tests {
    use super::*;
    use layer_core::color::source::SourceChannels;
    #[test]
    fn picker_sections_match_document_current_saved_and_channel_ownership() {
        let embedded = ColorProfile::Icc(vec![1,2,3].into());
        let other = ColorProfile::Icc(vec![4,5,6].into());
        let builtin = ColorProfile::Builtin(layer_core::color::RgbSpace::Srgb);
        let key = profile_identity(&[1,2,3]);
        let mut saved = vec![ProfilePickerSaved {id:key,visible:true,channels:Some(ProfileChannels::Rgb),valid:true}];
        let purpose = ProfilePickerPurpose::Proof;
        let view = picker_inventory(&purpose,Some(&embedded),Some(&embedded),&saved);
        assert!(view.document && !view.current && view.saved.is_empty() && view.standard);
        let view = picker_inventory(&purpose,Some(&builtin),Some(&other),&saved);
        assert!(!view.document && view.current && view.saved==[0]);
        saved[0].visible=false;
        assert!(!picker_inventory(&purpose,None,Some(&embedded),&saved).current);
        saved[0].valid=false;
        assert!(picker_inventory(&purpose,None,Some(&embedded),&saved).current);
        for channels in [SourceChannels::Gray,SourceChannels::Cmyk] {
            let view=picker_inventory(&ProfilePickerPurpose::Source(channels),None,Some(&builtin),&saved);
            assert!(!view.document && !view.current && view.saved.is_empty() && !view.standard);
        }
    }

    #[test]
    fn source_picker_validates_imported_profiles_against_actual_sample_channels() {
        use layer_core::color::{RgbSpace,SampleDepth,source::SourceInterpretation};
        let rgb = ColorProfile::Builtin(RgbSpace::DisplayP3);
        let gray = layer_color::gray_profile(RgbSpace::Srgb).unwrap();
        for (channels, profile, valid) in [(SourceChannels::Rgba,&rgb,true),(SourceChannels::Rgba,&gray,false),
            (SourceChannels::GrayAlpha,&gray,true),(SourceChannels::GrayAlpha,&rgb,false),(SourceChannels::Cmyk,&rgb,false)] {
            let purpose = ProfilePurpose::Source(SourceInterpretation {channels,depth:SampleDepth::U16,profile:rgb.clone(),profile_assumed:false});
            let profile = crate::ExportProfile {profile:profile.clone(),name:"Imported".into(),channels:layer_color::profile_channels(profile).unwrap()};
            assert_eq!(purpose.validate(&profile,RgbSpace::ProPhoto).is_ok(),valid);
            assert_eq!(purpose.picker_purpose().accepts(profile.channels),valid);
        }
    }

    #[test]
    fn picker_channels_follow_source_samples_and_proof_requires_non_gray() {
        for channels in [ProfileChannels::Rgb,ProfileChannels::Gray,ProfileChannels::Cmyk] {
            assert!(ProfilePickerPurpose::Output.accepts(channels));
            assert_eq!(ProfilePickerPurpose::Proof.accepts(channels),channels!=ProfileChannels::Gray);
            for (source,expected) in [(SourceChannels::Rgb,ProfileChannels::Rgb),(SourceChannels::Rgba,ProfileChannels::Rgb),
                (SourceChannels::Gray,ProfileChannels::Gray),(SourceChannels::GrayAlpha,ProfileChannels::Gray),(SourceChannels::Cmyk,ProfileChannels::Cmyk)] {
                assert_eq!(ProfilePickerPurpose::Source(source).accepts(channels),channels==expected);
            }
        }
    }
}
