use crate::{StoreError, content_id, new_id};
use serde::{Deserialize, Serialize};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const BYTE_BUDGET: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub id: String,
    pub created_ms: u64,
    pub bytes: u64,
}

pub fn prune(mut copies: Vec<SnapshotInfo>) -> Vec<String> {
    copies.sort_by(|a,b| (b.created_ms,&b.id).cmp(&(a.created_ms,&a.id)));
    let mut bytes = 0u64;
    copies.into_iter().enumerate().filter_map(|(index,copy)| {
        bytes = bytes.saturating_add(copy.bytes);
        (index >= 3 && (index >= 10 || bytes > BYTE_BUDGET)).then_some(copy.id)
    }).collect()
}

pub(crate) fn name(version: &str) -> String {
    let version = if !version.is_empty() && version.len() <= 80
        && version.bytes().all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c)) {version} else {"earlier"};
    format!("workspaces.{version}.{}",new_id())
}

pub(crate) fn managed_name(name: &str) -> bool {
    name.strip_prefix("workspaces.").and_then(|name|name.rsplit_once('.')).is_some_and(|(_,id)|uuid::Uuid::parse_str(id).is_ok())
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct VersionMarker {
    pub version: String,
    pub unreadable_hash: Option<String>,
    #[serde(default)]
    pub copies: Vec<SnapshotInfo>,
}

#[derive(Default, Deserialize)]
pub struct SnapshotEnvironment {
    pub marker: Option<VersionMarker>,
    pub unreadable: Option<String>,
}

#[derive(Serialize)]
pub struct SnapshotCopy {
    #[serde(flatten)]
    pub info: SnapshotInfo,
    pub snapshot: String,
}

#[derive(Serialize)]
pub struct SnapshotPlan {
    pub marker: VersionMarker,
    pub copies: Vec<SnapshotCopy>,
    pub remove: Vec<String>,
}

pub fn plan(snapshot: Option<&str>, environment: SnapshotEnvironment, adopted: bool, now: u64) -> Result<SnapshotPlan, StoreError> {
    let previous = environment.marker.unwrap_or_default();
    if previous.copies.iter().any(|copy|!copy.id.strip_prefix("snapshot:").is_some_and(managed_name)) {
        return Err(StoreError::invalid("Invalid workspace snapshot identity."));
    }
    let mut copies = Vec::new();
    let mut copy = |version: &str, text: &str| copies.push(SnapshotCopy {
        info: SnapshotInfo {id:format!("snapshot:{}",name(version)),created_ms:now,bytes:text.len() as u64},
        snapshot:text.into(),
    });
    if previous.version != VERSION && let Some(text) = snapshot {
        let value: serde_json::Value = serde_json::from_str(text)?;
        if value["items"].as_object().is_some_and(|items| !items.is_empty())
            || value["editing"].is_string()
            || value["pending"].as_object().is_some_and(|pending| !pending.is_empty()) {
            copy(&previous.version,text);
        }
    }
    let unreadable_hash = environment.unreadable.as_ref().map(|text| content_id(text.as_bytes())).or_else(||previous.unreadable_hash.clone());
    if unreadable_hash != previous.unreadable_hash && let Some(text) = &environment.unreadable {
        copy("unreadable",text);
    }
    let mut retained = previous.copies;
    retained.extend(copies.iter().map(|copy|copy.info.clone()));
    let remove = if adopted {prune(retained.clone())} else {Vec::new()};
    retained.retain(|copy|!remove.contains(&copy.id));
    Ok(SnapshotPlan {marker:VersionMarker {version:VERSION.into(),unreadable_hash,copies:retained},copies,remove})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_changes_and_unreadable_records_keep_original_bytes_without_repeated_copies() {
        let original = r#"{"items":{"custom":{}},"editing":"released palettes"}"#;
        let old = r#"{"items":{"legacy":{}}}"#;
        let first = plan(Some(original),SnapshotEnvironment {marker:None,unreadable:Some(old.into())},false,100).unwrap();
        assert_eq!(first.copies.len(),2);
        assert_eq!(first.copies[0].snapshot,original);
        assert_eq!(first.copies[1].snapshot,old);
        let mut marker = first.marker.clone();
        assert!(plan(Some(original),SnapshotEnvironment {marker:Some(marker.clone()),unreadable:Some(old.into())},false,101).unwrap().copies.is_empty());
        marker.version = "99.0.0".into();
        let down = plan(Some(original),SnapshotEnvironment {marker:Some(marker),unreadable:Some(old.into())},false,102).unwrap();
        assert_eq!(down.copies.len(),1);
        assert_ne!(first.copies[0].info.id,down.copies[0].info.id);
    }
    #[test]
    fn retention_keeps_the_newest_within_count_and_bytes_and_at_least_three() {
        for (count,size,kept) in [(12,1,10),(8,80*1024*1024,3),(8,64*1024*1024,4),(8,90*1024*1024,3),(2,300*1024*1024,2)] {
            let copies = (0..count).map(|i|SnapshotInfo {id:i.to_string(),created_ms:i,bytes:size}).collect();
            assert_eq!(prune(copies),(0..count-kept).rev().map(|i|i.to_string()).collect::<Vec<_>>());
        }
    }
}
