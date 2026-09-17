//! Content-addressed ICC library policy. Hosts enumerate app-owned storage and
//! perform bounded reads / atomic writes under their native storage lock.
use layer_core::color::{ColorProfile, ProfileChannels};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROFILE_LIBRARY_ENTRIES: usize = 128;
pub const PROFILE_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;
pub const PROFILE_READ_LIMIT: usize = layer_color::MAX_ICC_BYTES;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileRecord {
    pub id: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProfileEntry {
    pub id: String,
    pub bytes: u64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<ProfileChannels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ColorProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
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
            Some(
                "The profile library limit is 128 profiles and 64 MiB; remove unused profiles"
                    .into(),
            )
        } else if record.bytes > PROFILE_READ_LIMIT as u64 {
            Some("ICC profile exceeds 16 MiB".into())
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
) -> Result<ProfileEntry, String> {
    if !valid_profile_id(id) {
        return Err("Select an imported profile".into());
    }
    if bytes.len() > PROFILE_READ_LIMIT {
        return Err("ICC profile exceeds 16 MiB".into());
    }
    if profile_identity(bytes) != id {
        return Err("Profile changed in storage; remove or reimport it".into());
    }
    let profile = ColorProfile::Icc(bytes.to_vec().into());
    Ok(ProfileEntry {
        id: id.into(),
        bytes: bytes.len() as u64,
        name: layer_color::profile_description(&profile)?,
        channels: Some(layer_color::profile_channels(&profile)?),
        profile: include_bytes.then_some(profile),
        issue: None,
    })
}
pub fn inspect_library_entry(record: &ProfileRecord, bytes: Result<&[u8], String>) -> ProfileEntry {
    let result = record.issue.clone().map_or_else(
        || bytes.and_then(|b| read_library_profile(&record.id, b, false)),
        Err,
    );
    result.unwrap_or_else(|issue| ProfileEntry {
        id: record.id.clone(),
        bytes: record.bytes,
        name: format!(
            "Unavailable profile {}",
            record.id.chars().take(12).collect::<String>()
        ),
        channels: None,
        profile: None,
        issue: Some(issue),
    })
}
pub fn prepare_profile_import(
    records: Vec<ProfileRecord>,
    bytes: &[u8],
) -> Result<ProfileEntry, String> {
    if bytes.len() > PROFILE_READ_LIMIT {
        return Err("ICC profile exceeds 16 MiB".into());
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
        return Err("The profile library limit is 128 profiles and 64 MiB".into());
    }
    Ok(entry)
}

/// The same stateless transport for Wasm workers and JNI. File paths never
/// enter this contract; a Remove result can only name an app-owned object.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileLibraryAction {
    Limits,
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
        error: Option<String>,
    },
    Remove {
        id: String,
    },
}
impl ProfileLibraryAction {
    pub fn execute(self, bytes: &[u8]) -> Result<serde_json::Value, String> {
        use serde_json::json;
        Ok(match self {
            Self::Limits => json!({"read_bytes": PROFILE_READ_LIMIT}),
            Self::Inventory { entries } => json!(profile_inventory(entries)),
            Self::Import { entries } => json!(prepare_profile_import(entries, bytes)?),
            Self::Get { id } => json!(read_library_profile(&id, bytes, true)?),
            Self::Inspect { entry, error } => {
                json!(inspect_library_entry(&entry, error.map_or(Ok(bytes), Err)))
            }
            Self::Remove { id } => {
                if !valid_profile_id(&id) {
                    return Err("Select an imported profile".into());
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
}
