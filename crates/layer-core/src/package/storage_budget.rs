use std::{collections::BTreeMap, sync::{Arc, Mutex, OnceLock}};

pub const STORAGE_UNAVAILABLE: &str = "Drawing storage does not have enough available space";
#[cfg(not(target_arch="wasm32"))]
pub(crate) fn storage_error(error:std::io::Error)->String {
    let full=error.kind()==std::io::ErrorKind::StorageFull;
    #[cfg(unix)]
    let full=full || error.raw_os_error()==Some(libc::EDQUOT);
    if full {STORAGE_UNAVAILABLE.into()}else {error.to_string()}
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageFacts {
    pub scope: String,
    pub available_bytes: Option<u64>,
    pub epoch: u64,
}

#[derive(Clone)]
pub struct StorageRefusal {pub facts:Option<StorageFacts>,pub epoch:u64}
impl StorageRefusal {
    pub fn allows(&self,facts:&StorageFacts)->bool {
        StorageBudget::process().removal(&facts.scope).1>self.epoch
            || facts.available_bytes.is_some_and(|available|available>0 && self.facts.as_ref().map_or(facts.epoch>self.epoch,|old|facts.epoch>old.epoch && available>old.available_bytes.unwrap_or(0)))
    }
}

#[derive(Clone, Default)]
pub struct StorageBudget(Arc<Mutex<State>>);
#[derive(Default)]
struct State { epoch: u64, volumes: BTreeMap<String, Volume> }
struct Volume { facts: StorageFacts, reserved: u64, written: Vec<(u64, u64)>, removed_bytes:u64, removal_epoch:u64 }

pub enum StorageAdmission { Granted(StorageReservation), NeedsFacts, Refused }
pub struct StorageReservation { budget: StorageBudget, scope: String, bytes: u64, written: u64 }

impl StorageBudget {
    pub fn process() -> Self {
        static BUDGET: OnceLock<StorageBudget> = OnceLock::new();
        BUDGET.get_or_init(Self::default).clone()
    }
    pub fn sample_epoch(&self) -> u64 {
        let mut state = self.0.lock().unwrap();
        state.epoch = state.epoch.checked_add(1).expect("storage epoch exhausted");
        state.epoch
    }
    pub fn event_epoch(&self)->u64 {self.0.lock().unwrap().epoch}
    pub fn removal(&self,scope:&str)->(u64,u64) {
        self.0.lock().unwrap().volumes.get(scope).map_or((0,0),|volume|(volume.removed_bytes,volume.removal_epoch))
    }
    fn removed(&self,scope:&str,bytes:u64) {
        if bytes==0 {return;}
        let mut state=self.0.lock().unwrap();state.epoch=state.epoch.checked_add(1).expect("storage epoch exhausted");let epoch=state.epoch;
        let volume=state.volumes.entry(scope.into()).or_insert_with(||Volume {facts:StorageFacts {scope:scope.into(),available_bytes:None,epoch:0},reserved:0,written:Vec::new(),removed_bytes:0,removal_epoch:0});volume.removed_bytes=volume.removed_bytes.saturating_add(bytes);volume.removal_epoch=epoch;
    }
    pub fn publish(&self, facts: StorageFacts) {
        let mut state = self.0.lock().unwrap();
        state.epoch = state.epoch.max(facts.epoch);
        match state.volumes.get_mut(&facts.scope) {
            Some(volume) if facts.epoch > volume.facts.epoch => {
                volume.written.retain(|(epoch, _)| *epoch >= facts.epoch);
                volume.facts = facts;
            },
            None => { state.volumes.insert(facts.scope.clone(), Volume { facts, reserved: 0, written: Vec::new(), removed_bytes:0, removal_epoch:0 }); },
            _ => {},
        }
    }
    pub fn admit(&self, scope: &str, bytes: u64, headroom: u64) -> StorageAdmission {
        let mut state = self.0.lock().unwrap();
        let Some(volume) = state.volumes.get_mut(scope) else { return StorageAdmission::NeedsFacts; };
        let Some(available) = volume.facts.available_bytes else { return StorageAdmission::NeedsFacts; };
        let written = volume.written.iter().fold(0u64, |total, (_, bytes)| total.saturating_add(*bytes));
        if bytes.checked_add(headroom).is_none_or(|need| need > available.saturating_sub(written).saturating_sub(volume.reserved)) {
            return StorageAdmission::Refused;
        }
        volume.reserved += bytes;
        StorageAdmission::Granted(StorageReservation { budget: self.clone(), scope: scope.into(), bytes, written: 0 })
    }
    pub fn facts(&self,scope:&str)->Option<StorageFacts> {self.0.lock().unwrap().volumes.get(scope).map(|volume|volume.facts.clone())}
    pub fn available_bytes(&self, scope: &str) -> Option<u64> {
        let state = self.0.lock().unwrap();let volume = state.volumes.get(scope)?;
        let written = volume.written.iter().fold(0u64, |total, (_, bytes)| total.saturating_add(*bytes));
        Some(volume.facts.available_bytes?.saturating_sub(written).saturating_sub(volume.reserved))
    }
}
impl StorageReservation {
    pub fn written(&mut self, bytes: u64) -> Result<(), String> {
        let written = self.written.checked_add(bytes).filter(|written| *written <= self.bytes).ok_or("Drawing storage reservation exceeded")?;
        let mut state=self.budget.0.lock().unwrap();state.epoch=state.epoch.checked_add(1).expect("storage epoch exhausted");let epoch=state.epoch;
        let volume=state.volumes.get_mut(&self.scope).unwrap();volume.reserved-=bytes;
        if bytes!=0 {volume.written.push((epoch,bytes));}self.written=written;
        Ok(())
    }
}
pub(crate) async fn remove(io:&dyn super::store_io::FileTransport,name:&str)->Result<(),String> {
    let removed=io.remove(name).await?;
    StorageBudget::process().removed(&removed.scope,removed.bytes);
    Ok(())
}
pub async fn reserve(io: &dyn super::store_io::FileTransport, bytes: u64) -> Result<StorageReservation, String> {
    let facts = io.storage_facts().await?;
    let budget = StorageBudget::process();
    let scope = facts.scope.clone(); budget.publish(facts);
    match budget.admit(&scope, bytes, super::session_store::PACK_BYTES + super::MAX_RANGE_BYTES as u64) {
        StorageAdmission::Granted(token) => Ok(token),
        StorageAdmission::NeedsFacts => Err(STORAGE_UNAVAILABLE.into()),
        StorageAdmission::Refused => Err(STORAGE_UNAVAILABLE.into()),
    }
}
impl Drop for StorageReservation {
    fn drop(&mut self) {
        let mut state = self.budget.0.lock().unwrap();
        let volume = state.volumes.get_mut(&self.scope).unwrap();
        volume.reserved -= self.bytes-self.written;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn facts(budget: &StorageBudget, available: Option<u64>) {
        budget.publish(StorageFacts { scope: "volume".into(), available_bytes: available, epoch: budget.sample_epoch() });
    }
    fn granted(budget: &StorageBudget, bytes: u64) -> StorageReservation {
        match budget.admit("volume", bytes, 10) { StorageAdmission::Granted(token) => token, _ => panic!("expected admission") }
    }
    #[test]
    fn removal_before_any_sample_keeps_capacity_unknown() {
        let budget=StorageBudget::default();budget.removed("volume",60);
        assert_eq!(budget.available_bytes("volume"),None);
        assert!(matches!(budget.admit("volume",1,0),StorageAdmission::NeedsFacts));
        let evidence=budget.removal("volume");assert_eq!(evidence.0,60);
        facts(&budget,Some(100));assert_eq!(budget.available_bytes("volume"),Some(100));
        assert_eq!(budget.removal("volume"),evidence);
    }
    #[test]
    fn removal_evidence_survives_samples_without_crediting_capacity() {
        let budget=StorageBudget::default();facts(&budget,Some(100));
        let mut write=granted(&budget,60);write.written(60).unwrap();drop(write);
        let overlapping=budget.sample_epoch();let refusal=budget.event_epoch();
        budget.removed("volume",40);
        let evidence=budget.removal("volume");assert_eq!(evidence.0,40);assert!(evidence.1>refusal);
        assert_eq!(budget.available_bytes("volume"),Some(40));
        budget.publish(StorageFacts {scope:"volume".into(),available_bytes:Some(40),epoch:overlapping});
        assert_eq!(budget.available_bytes("volume"),Some(40));assert_eq!(budget.removal("volume"),evidence);
        facts(&budget,Some(80));assert_eq!(budget.available_bytes("volume"),Some(80));
        assert_eq!(budget.removal("volume"),evidence);
        let next_refusal=budget.event_epoch();assert!(evidence.1<=next_refusal);
        budget.removed("volume",0);assert_eq!(budget.removal("volume"),evidence);
        budget.publish(StorageFacts {scope:"volume".into(),available_bytes:Some(1000),epoch:overlapping});
        assert_eq!(budget.available_bytes("volume"),Some(80));
        budget.removed("volume",10);assert_eq!(budget.removal("volume").0,50);assert!(budget.removal("volume").1>next_refusal);
    }
    #[test]
    fn unknown_facts_and_shrinking_space_never_invent_capacity() {
        let budget = StorageBudget::default();
        assert!(matches!(budget.admit("volume", 1, 0), StorageAdmission::NeedsFacts));
        facts(&budget, None);
        assert!(matches!(budget.admit("volume", 1, 0), StorageAdmission::NeedsFacts));
        facts(&budget, Some(100));
        let first = granted(&budget, 60);
        facts(&budget, Some(50));
        assert!(matches!(budget.admit("volume", 1, 0), StorageAdmission::Refused));
        drop(first);
        assert!(matches!(budget.admit("volume", u64::MAX, 1), StorageAdmission::Refused));
    }
    #[test]
    fn drawings_share_reservations_and_written_bytes_until_a_new_sample() {
        let budget = StorageBudget::default(); facts(&budget, Some(100));
        let mut first = granted(&budget, 60);
        assert!(matches!(budget.admit("volume", 31, 10), StorageAdmission::Refused));
        first.written(40).unwrap(); drop(first);
        let second = granted(&budget, 50); drop(second);
        assert!(matches!(budget.admit("volume", 51, 10), StorageAdmission::Refused));
        facts(&budget, Some(60));
        drop(granted(&budget, 50));
    }
    #[test]
    fn samples_overlapping_writes_do_not_release_capacity_twice() {
        let budget = StorageBudget::default(); facts(&budget, Some(100));
        let sample = budget.sample_epoch();
        let mut first = granted(&budget, 60); first.written(60).unwrap(); drop(first);
        budget.publish(StorageFacts { scope: "volume".into(), available_bytes: Some(100), epoch: sample });
        assert!(matches!(budget.admit("volume", 31, 10), StorageAdmission::Refused));
        facts(&budget, Some(40));
        drop(granted(&budget, 30));
        budget.publish(StorageFacts { scope: "volume".into(), available_bytes: Some(100), epoch: sample });
        assert!(matches!(budget.admit("volume", 31, 10), StorageAdmission::Refused));
    }
    #[test]
    fn a_sample_after_append_does_not_count_written_bytes_as_an_outstanding_reservation() {
        let budget=StorageBudget::default();facts(&budget,Some(100));let mut first=granted(&budget,60);
        first.written(60).unwrap();facts(&budget,Some(40));
        drop(granted(&budget,30));drop(first);
    }
}
