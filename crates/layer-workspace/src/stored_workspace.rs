use crate::*;
use layer_ui::{DockLayout, LayoutEdit, LayoutHead, LayoutHistory, LayoutRevision};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue;
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, StoreError>;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Workspace,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredHistory {
    current: String,
    #[serde(with = "counter")]
    generation: u64,
    #[serde(default, deserialize_with = "layer_ui::lenient::vec")]
    redo: Vec<String>,
    revisions: BTreeMap<String, Box<RawValue>>,
    #[serde(default, deserialize_with = "layer_ui::lenient::vec")]
    undo: Vec<String>,
    #[serde(default, deserialize_with = "layer_ui::lenient::or_default")]
    version: u32,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredWorkspace {
    baseline: Box<RawValue>,
    history: StoredHistory,
    #[serde(rename = "type")]
    kind: Kind,
}

fn decode<T: DeserializeOwned>(raw: &RawValue, read: &mut impl FnMut(&str) -> Result<String>) -> Result<T> {
    Ok(serde_json::from_value(protocol::resolve(raw.get(), &mut *read)?)?)
}
fn no_longer_retained(error: StoreError) -> StoreError {
    if error.kind == ErrorKind::InvalidData {
        StoreError::known(ErrorKind::InvalidData, WorkspaceRefusal::ThisLayoutVersionIsNoLongerRetained)
    } else {
        error
    }
}

impl StoredHistory {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
    pub(crate) fn delta(edit: &LayoutEdit) -> Result<LayoutHistory> {
        edit.validate().map_err(StoreError::invalid)?;
        Ok(LayoutHistory {
            version: edit.head.version,
            generation: edit.head.generation,
            current: edit.head.current.id.clone(),
            undo: edit.head.undo.clone(),
            redo: edit.head.redo.clone(),
            revisions: edit.unsaved.clone(),
        })
    }
    fn within_limits(&self) -> Result<()> {
        if self.generation == u64::MAX || self.revisions.len() > 100_000 || self.undo.len() > 100 || self.redo.len() > 100 {
            return Err(StoreError::invalid("Layout history exceeds supported limits"));
        }
        Ok(())
    }
    pub(crate) fn validate(&self, mut read: impl FnMut(&str) -> Result<String>) -> Result<()> {
        self.within_limits()?;
        for (id, raw) in &self.revisions {
            decode::<LayoutRevision>(raw, &mut read)?.validate(id).map_err(StoreError::invalid)?;
        }
        Ok(())
    }
    pub(crate) fn resolved(mut self, mut read: impl FnMut(&str) -> Result<String>) -> Result<Self> {
        for raw in self.revisions.values_mut() {
            *raw = RawValue::from_string(serde_json::to_string(&protocol::resolve(raw.get(), &mut read)?)?)?;
        }
        Ok(self)
    }
    fn apply(&mut self, edit: StoredHistory) -> Result<()> {
        for (id, revision) in edit.revisions {
            match self.revisions.get(&id) {
                Some(stored) if serde_json::from_str::<serde_json::Value>(stored.get())?
                    != serde_json::from_str::<serde_json::Value>(revision.get())? => return Err(StoreError::conflict()),
                Some(_) => {}
                None => {
                    self.revisions.insert(id, revision);
                }
            }
        }
        self.version = edit.version;
        self.generation = edit.generation;
        self.current = edit.current;
        self.undo = edit.undo;
        self.redo = edit.redo;
        self.within_limits()?;
        if std::iter::once(&self.current).chain(&self.undo).chain(&self.redo).any(|id| !self.revisions.contains_key(id)) {
            return Err(StoreError::invalid("Missing layout history revision"));
        }
        Ok(())
    }
}
pub(crate) fn applied(history: &LayoutHistory, mut edit: StoredHistory) -> Result<LayoutHistory> {
    let mut stored = StoredHistory::parse(&serde_json::to_string(history)?)?;
    let readable = |id: &String| stored.revisions.contains_key(id) || edit.revisions.contains_key(id);
    let (undo, redo) = (edit.undo.iter().filter(|id| readable(id)).cloned().collect(), edit.redo.iter().filter(|id| readable(id)).cloned().collect());
    (edit.undo, edit.redo) = (undo, redo);
    stored.apply(edit)?;
    let mut history: LayoutHistory = serde_json::from_str(&serde_json::to_string(&stored)?)?;
    history.retain_admitted();
    history.validate().map_err(StoreError::invalid)?;
    Ok(history)
}
pub(crate) fn applied_edit(history: &LayoutHistory, edit: &LayoutEdit) -> Result<LayoutHistory> {
    applied(history, StoredHistory::parse(&serde_json::to_string(&StoredHistory::delta(edit)?)?)?)
}

impl StoredWorkspace {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        Ok(serde_json::from_str(text)?)
    }
    pub(crate) fn encode(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }
    pub(crate) fn baseline(text: &str, mut read: impl FnMut(&str) -> Result<String>) -> Result<DockLayout> {
        #[derive(Deserialize)]
        struct Baseline {
            baseline: Box<RawValue>,
        }
        let mut baseline: DockLayout = decode(&serde_json::from_str::<Baseline>(text)?.baseline, &mut read)?;
        baseline.retain_admitted();
        Ok(baseline)
    }
    pub(crate) fn head(&self, read: &mut impl FnMut(&str) -> Result<String>) -> Result<(LayoutHead, DockLayout)> {
        let history = &self.history;
        history.within_limits()?;
        let retained = |id: &&String| history.revisions.contains_key(*id);
        let head = LayoutHead {
            version: history.version,
            generation: history.generation,
            current: self.admitted(&history.current, read)?,
            undo: history.undo.iter().filter(retained).cloned().collect(),
            redo: history.redo.iter().filter(retained).cloned().collect(),
        };
        let mut baseline: DockLayout = decode(&self.baseline, read)?;
        baseline.retain_admitted();
        Ok((head, baseline))
    }
    pub(crate) fn revision(&self, id: &str, read: &mut impl FnMut(&str) -> Result<String>) -> Result<LayoutRevision> {
        let missing = || no_longer_retained(StoreError::invalid("Missing layout history revision"));
        let revision: LayoutRevision = decode(self.history.revisions.get(id).ok_or_else(missing)?, read).map_err(no_longer_retained)?;
        if revision.id != id {
            return Err(missing());
        }
        Ok(revision)
    }
    fn admitted(&self, id: &str, read: &mut impl FnMut(&str) -> Result<String>) -> Result<LayoutRevision> {
        let mut revision = self.revision(id, read)?;
        model::admit_revision(&mut revision)?;
        Ok(revision)
    }
    pub(crate) fn versions(&self) -> Vec<LayoutVersion> {
        self.history
            .revisions
            .iter()
            .filter_map(|(id, raw)| {
                let mut version: LayoutVersion = serde_json::from_str(raw.get()).ok()?;
                if version.description.validate().is_err() {
                    version.description = Default::default();
                }
                (&version.id == id).then_some(version)
            })
            .collect()
    }
    pub(crate) fn complete(&self, mut read: impl FnMut(&str) -> Result<String>) -> Result<(LayoutHistory, DockLayout)> {
        let (head, baseline) = self.head(&mut read)?;
        let mut revisions = BTreeMap::new();
        for id in self.history.revisions.keys() {
            match self.admitted(id, &mut read) {
                Ok(revision) => {
                    revisions.insert(id.clone(), revision);
                }
                Err(error) if error.kind == ErrorKind::InvalidData => {}
                Err(error) => return Err(error),
            }
        }
        let retained = |id: &String| revisions.contains_key(id);
        let history = LayoutHistory {
            version: head.version,
            generation: head.generation,
            undo: head.undo.into_iter().filter(retained).collect(),
            redo: head.redo.into_iter().filter(retained).collect(),
            current: head.current.id,
            revisions,
        };
        Ok((history, baseline))
    }
    pub(crate) fn apply(&mut self, edit: StoredHistory) -> Result<()> {
        self.history.apply(edit)
    }
    pub(crate) fn unreachable(&self) -> impl Iterator<Item = (&String, &RawValue)> {
        let history = &self.history;
        history
            .revisions
            .iter()
            .filter(|(id, _)| **id != history.current && !history.undo.contains(id) && !history.redo.contains(id))
            .map(|(id, raw)| (id, raw.as_ref()))
    }
    pub(crate) fn remove(&mut self, id: &str) {
        self.history.revisions.remove(id);
    }
}

#[cfg(feature = "native")]
pub(crate) fn resolved_len(text: &str, read: &mut impl FnMut(&str) -> Option<String>, lengths: &mut BTreeMap<String, u64>) -> u64 {
    const REFERENCE: &str = "{\"$workspace_component\":\"";
    let mut total = text.len() as u64;
    let mut rest = text;
    while let Some(start) = rest.find(REFERENCE) {
        let after = &rest[start + REFERENCE.len()..];
        let Some(end) = after.find("\"}") else { break };
        let id = &after[..end];
        let length = match lengths.get(id) {
            Some(length) => Some(*length),
            None => read(id).map(|json| {
                let length = resolved_len(&json, read, lengths);
                lengths.insert(id.into(), length);
                length
            }),
        };
        if let Some(length) = length {
            total = total.saturating_sub((REFERENCE.len() + end + 2) as u64).saturating_add(length);
        }
        rest = &after[end + 2..];
    }
    total
}
