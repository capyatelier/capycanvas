mod system;
pub use system::system_facts;

use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, Mutex, MutexGuard, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}}};
use web_time::Instant;

const MIB: u64 = 1024 * 1024;
pub const MEMORY_OWNER_KINDS: usize = 10;
pub const MEMORY_CLASSES: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryFacts {
    pub total_bytes: Option<u64>,
    pub host_default_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub process_limit_bytes: Option<u64>,
    pub process_used_bytes: Option<u64>,
    pub tablet: bool,
    pub override_bytes: Option<u64>,
    pub pressure: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceMemoryFacts {
    pub budget_bytes: Option<u64>,
    pub heap_bytes: Option<u64>,
    pub usage_bytes: Option<u64>,
    pub fixed_bytes: u64,
    pub unified: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryDevice(u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum MemoryOwnerKind { Scene, Objects, Mips, DecodedSamples, RasterPool, Staging, ParkedTabs, Authored, History, Other }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum MemoryClass { Pinned, Visible, Warm }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryReport {
    pub ram_bytes: u64,
    pub gpu_bytes: u64,
    pub pinned_ram_bytes: u64,
    pub pinned_gpu_bytes: u64,
    pub demand_ram_bytes: u64,
    pub demand_gpu_bytes: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryGrant { pub ram_bytes: u64, pub gpu_bytes: u64 }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoryOwnerTotals {
    pub owners: usize,
    pub ram_bytes: u64,
    pub gpu_bytes: u64,
    pub pinned_ram_bytes: u64,
    pub pinned_gpu_bytes: u64,
    pub granted_ram_bytes: u64,
    pub granted_gpu_bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryDeviceSnapshot {
    pub device: MemoryDevice,
    pub unified: bool,
    pub limit_bytes: u64,
    pub physical_bytes: u64,
    pub pinned_bytes: u64,
    pub granted_bytes: u64,
    pub pinned_debt_bytes: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryBudgetSnapshot {
    pub ram_limit_bytes: u64,
    pub ram_physical_bytes: u64,
    pub ram_pinned_bytes: u64,
    pub ram_granted_bytes: u64,
    pub pinned_debt_bytes: u64,
    pub refusals: u64,
    pub reserved_bytes: u64,
    pub pressure: bool,
    pub rows: [[MemoryOwnerTotals; MEMORY_CLASSES]; MEMORY_OWNER_KINDS],
    pub devices: Vec<MemoryDeviceSnapshot>,
}
#[derive(Default)]
struct Grants { ram: AtomicU64, gpu: AtomicU64, wake: OnceLock<Arc<dyn Fn() + Send + Sync>> }
#[derive(Default)]
struct PendingReport { version: AtomicU64, values: [AtomicU64; 6] }
impl PendingReport {
    fn publish(&self, report: MemoryReport) -> bool {
        let version=self.version.load(Ordering::Acquire);
        if version&1!=0 || version>u64::MAX-2 || self.version.compare_exchange(version,version+1,Ordering::AcqRel,Ordering::Relaxed).is_err() {return false;}
        std::sync::atomic::fence(Ordering::Release);
        let values=[report.ram_bytes,report.gpu_bytes,report.pinned_ram_bytes,report.pinned_gpu_bytes,report.demand_ram_bytes,report.demand_gpu_bytes];
        for (field,value) in self.values.iter().zip(values) {field.store(value,Ordering::Relaxed);}
        self.version.store(version+2,Ordering::Release);true
    }
    fn read(&self) -> Option<(u64,MemoryReport)> {
        let version=self.version.load(Ordering::Acquire);if version&1!=0 {return None;}
        let values=self.values.each_ref().map(|value|value.load(Ordering::Relaxed));
        std::sync::atomic::fence(Ordering::Acquire);
        if self.version.load(Ordering::Relaxed)!=version {return None;}
        Some((version,MemoryReport {ram_bytes:values[0],gpu_bytes:values[1],pinned_ram_bytes:values[2],pinned_gpu_bytes:values[3],demand_ram_bytes:values[4],demand_gpu_bytes:values[5]}))
    }
}
struct OwnerRecord {
    kind: MemoryOwnerKind,
    class: MemoryClass,
    device: Option<MemoryDevice>,
    report: MemoryReport,
    grants: Arc<Grants>,
    pending: Arc<PendingReport>,
    applied: u64,
}
#[derive(Default)]
struct State {
    facts: MemoryFacts,
    override_bytes: Option<u64>,
    ram_limit: u64,
    initialized: bool,
    last_growth: u64,
    available_samples: [u64; 3],
    sample_count: usize,
    pressure_trip: bool,
    next: u64,
    devices: BTreeMap<MemoryDevice, DeviceMemoryFacts>,
    retired_devices: BTreeSet<MemoryDevice>,
    owners: BTreeMap<u64, OwnerRecord>,
    reservations: BTreeMap<u64, u64>,
}
#[derive(Default)]
struct BudgetCache { limit: AtomicU64, headroom: AtomicU64, refusals: AtomicU64 }
#[derive(Default)]
struct Manager { state: Mutex<State>, cache: BudgetCache }
#[derive(Clone)]
pub struct MemoryBudget(Arc<Manager>);
impl Default for MemoryBudget { fn default() -> Self { Self::new() } }
#[derive(Clone)]
pub struct MemoryOwner(Arc<Owner>);
struct Owner { budget: MemoryBudget, id: u64, grants: Arc<Grants>, pending: Arc<PendingReport> }
impl Drop for Owner {
    fn drop(&mut self) {
        let mut state = self.budget.0.state.lock().unwrap();
        state.owners.remove(&self.id);
        state.remove_retired_devices();
        self.budget.rebalance(state);
    }
}
impl std::fmt::Debug for MemoryOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("MemoryOwner").field(&self.grant()).finish() }
}
impl MemoryOwner {
    pub fn set_wake(&self, wake:Arc<dyn Fn() + Send + Sync>) {
        let result=self.0.grants.wake.set(wake);debug_assert!(result.is_ok(),"Memory owner wake is already registered");
    }
    pub fn publish(&self, report: MemoryReport) -> bool { self.0.pending.publish(report) }
    pub fn report(&self, mut report: MemoryReport) {
        report.ram_bytes = report.ram_bytes.max(report.pinned_ram_bytes);
        report.gpu_bytes = report.gpu_bytes.max(report.pinned_gpu_bytes);
        let mut state = self.0.budget.0.state.lock().unwrap();
        let owner=state.owners.get_mut(&self.0.id).unwrap();
        owner.report = report;owner.applied=owner.pending.version.load(Ordering::Acquire);
        self.0.budget.rebalance(state);
    }
    pub fn reported(&self) -> MemoryReport { self.0.budget.0.state.lock().unwrap().owners[&self.0.id].report }
    pub fn grant(&self) -> MemoryGrant {
        MemoryGrant { ram_bytes: self.0.grants.ram.load(Ordering::Acquire), gpu_bytes: self.0.grants.gpu.load(Ordering::Acquire) }
    }
}
#[derive(Clone)]
pub struct MemoryReservation(Arc<Reservation>);
struct Reservation { budget: MemoryBudget, id: u64, bytes: AtomicU64, active: AtomicBool }
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut state = self.budget.0.state.lock().unwrap();
        if state.reservations.remove(&self.id).is_some() { self.budget.rebalance(state); }
    }
}
impl std::fmt::Debug for MemoryReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("MemoryReservation").field(&self.reserved_bytes()).finish() }
}
impl MemoryReservation {
    pub fn reserved_bytes(&self) -> u64 { self.0.bytes.load(Ordering::Acquire) }
    pub fn active(&self) -> bool { self.0.active.load(Ordering::Acquire) }
    pub fn merge(&self, other: MemoryReservation) -> Result<(), &'static str> {
        if !Arc::ptr_eq(&self.0.budget.0,&other.0.budget.0) {return Err("Memory reservations belong to different budgets");}
        if Arc::ptr_eq(&self.0,&other.0) {return if self.active() {Ok(())}else {Err("Memory reservation is already complete")};}
        let mut state=self.0.budget.0.state.lock().unwrap();
        let Some(first)=state.reservations.get(&self.0.id).copied() else {return Err("Memory reservation is already complete");};
        let Some(second)=state.reservations.get(&other.0.id).copied() else {return Err("Memory reservation is already complete");};
        let bytes=first.checked_add(second).ok_or("Memory reservation size overflow")?;
        state.reservations.insert(self.0.id,bytes);state.reservations.remove(&other.0.id);
        self.0.bytes.store(bytes,Ordering::Release);other.0.bytes.store(0,Ordering::Release);other.0.active.store(false,Ordering::Release);
        self.0.budget.rebalance(state);Ok(())
    }
    pub fn resize(&self, bytes: u64) -> bool {
        let mut state = self.0.budget.0.state.lock().unwrap();
        let changed = state.apply_reports();
        let Some(previous) = state.reservations.get(&self.0.id).copied() else {
            if changed { self.0.budget.rebalance(state); }
            return false;
        };
        if bytes.saturating_sub(previous) > state.import_headroom() {
            self.0.budget.refused();
            if changed { self.0.budget.rebalance(state); }
            return false;
        }
        state.reservations.insert(self.0.id, bytes);
        self.0.bytes.store(bytes, Ordering::Release);
        self.0.budget.rebalance(state);
        true
    }
    pub fn commit(self, owner: &MemoryOwner, mut report: MemoryReport) -> Result<(), &'static str> {
        if !Arc::ptr_eq(&self.0.budget.0, &owner.0.budget.0) { return Err("Memory reservation belongs to another budget"); }
        let mut state = self.0.budget.0.state.lock().unwrap();
        state.apply_reports();
        if state.reservations.remove(&self.0.id).is_none() { return Err("Memory reservation is already complete"); }
        report.ram_bytes = report.ram_bytes.max(report.pinned_ram_bytes);
        report.gpu_bytes = report.gpu_bytes.max(report.pinned_gpu_bytes);
        let owner=state.owners.get_mut(&owner.0.id).unwrap();
        owner.report = report;owner.applied=owner.pending.version.load(Ordering::Acquire);
        self.0.bytes.store(0, Ordering::Release);
        self.0.active.store(false,Ordering::Release);
        self.0.budget.rebalance(state);
        Ok(())
    }
}
impl std::fmt::Debug for MemoryBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("MemoryBudget").field("limit", &self.ram_limit_bytes()).field("headroom", &self.import_headroom_bytes()).finish() }
}
impl MemoryBudget {
    fn rebalance(&self, state: MutexGuard<'_, State>) {
        let wakes = state.rebalance(&self.0.cache);
        drop(state);
        for wake in wakes { wake(); }
    }
    pub fn new() -> Self { Self::with_facts(MemoryFacts::default()) }
    pub fn with_facts(facts: MemoryFacts) -> Self { let budget = Self(Arc::new(Manager::default())); budget.update_ram(facts, 0); budget }
    pub fn process() -> Self {
        static PROCESS: OnceLock<MemoryBudget> = OnceLock::new();
        PROCESS.get_or_init(||Self::with_facts(system_facts())).clone()
    }
    pub fn now_ms() -> u64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
    pub fn update_ram(&self, facts: MemoryFacts, now_ms: u64) {
        let mut state = self.0.state.lock().unwrap();
        let reserved = reserve(facts);
        let band = reserved / 4;
        let ours = state.ram_physical(false);
        let share = state.override_bytes.or(facts.override_bytes).unwrap_or_else(|| facts.total_bytes.map_or(facts.host_default_bytes.unwrap_or(384 * MIB), |total| fraction(total, if facts.tablet {45} else {60}, 100)));
        if let Some(available) = facts.available_bytes {
            state.available_samples.rotate_right(1);
            state.available_samples[0] = available;
            state.sample_count = (state.sample_count + 1).min(3);
        }
        let samples = &state.available_samples[..state.sample_count];
        let low = samples.iter().min().copied();
        let high = samples.iter().max().copied();
        let sustained = state.sample_count == 3 && high.is_some_and(|available| available < reserved);
        let headroom = high.map_or(share, |available| ours.saturating_add(available).saturating_sub(reserved).saturating_sub(band));
        let process = facts.process_limit_bytes.map_or(u64::MAX, |limit| match facts.process_used_bytes {
            Some(used) => ours.saturating_add(limit.saturating_sub(used)), None => limit,
        });
        let ceiling = share.min(process).max(384 * MIB);
        let target = ceiling.min(headroom).max(384 * MIB);
        if !state.initialized {
            state.ram_limit = target;
            state.last_growth = now_ms;
        } else if facts.pressure || sustained {
            if !state.pressure_trip {
                state.ram_limit = target.min(fraction(state.ram_limit, 3, 4)).max(384 * MIB);
                state.last_growth = now_ms;
                state.pressure_trip = true;
            } else if target < state.ram_limit {
                state.ram_limit = target;
                state.last_growth = now_ms;
            }
        } else {
            state.pressure_trip = false;
            if target < fraction(state.ram_limit, 9, 10) {
                state.ram_limit = target;
                state.last_growth = now_ms;
            } else if now_ms.saturating_sub(state.last_growth) >= 5000 && state.ram_limit < target
                && low.is_some_and(|available| available >= reserved.saturating_add(band)) {
                state.ram_limit = target.min(state.ram_limit.saturating_add((state.ram_limit / 10).max(1)));
                state.last_growth = now_ms;
            }
        }
        state.ram_limit = state.ram_limit.min(ceiling);
        state.initialized = facts.available_bytes.is_some() || facts.total_bytes.is_some() || facts.host_default_bytes.is_some() || state.override_bytes.or(facts.override_bytes).is_some();
        state.facts = facts;
        self.rebalance(state);
    }
    pub fn set_ram_override(&self, bytes: Option<u64>) {
        let facts = {
            let mut state = self.0.state.lock().unwrap();
            state.override_bytes = bytes;
            state.facts
        };
        self.update_ram(facts, Self::now_ms());
    }
    pub fn register_device(&self, facts: DeviceMemoryFacts) -> MemoryDevice {
        let mut state = self.0.state.lock().unwrap();
        state.next = state.next.checked_add(1).expect("Memory owner identity exhausted");
        let id = MemoryDevice(state.next);
        state.devices.insert(id, facts);
        self.rebalance(state);
        id
    }
    pub fn update_device(&self, device: MemoryDevice, facts: DeviceMemoryFacts) {
        let mut state = self.0.state.lock().unwrap();
        if let Some(current) = state.devices.get_mut(&device) { *current = facts; }
        self.rebalance(state);
    }
    pub fn unregister_device(&self, device: MemoryDevice) {
        let mut state = self.0.state.lock().unwrap();
        state.retired_devices.insert(device);
        state.remove_retired_devices();
        self.rebalance(state);
    }
    pub fn register_owner(&self, kind: MemoryOwnerKind, class: MemoryClass, device: Option<MemoryDevice>) -> MemoryOwner {
        let mut state = self.0.state.lock().unwrap();
        state.next = state.next.checked_add(1).expect("Memory owner identity exhausted");
        let id = state.next;
        let grants = Arc::new(Grants::default());let pending=Arc::new(PendingReport::default());
        state.owners.insert(id, OwnerRecord { kind, class, device, report: MemoryReport::default(), grants: grants.clone(), pending: pending.clone(), applied:0 });
        MemoryOwner(Arc::new(Owner { budget: self.clone(), id, grants, pending }))
    }
    pub fn flush_reports(&self) {
        let mut state=self.0.state.lock().unwrap();
        if state.apply_reports() {self.rebalance(state);}
    }
    pub fn ram_limit_bytes(&self) -> u64 { self.0.cache.limit.load(Ordering::Acquire) }
    pub fn import_headroom_bytes(&self) -> u64 { self.0.cache.headroom.load(Ordering::Acquire) }
    fn refused(&self) { let _ = self.0.cache.refusals.try_update(Ordering::Relaxed, Ordering::Relaxed, |count| Some(count.saturating_add(1))); }
    pub fn admit_import(&self, bytes: u64) -> Option<MemoryReservation> {
        let mut state = self.0.state.lock().unwrap();
        let changed = state.apply_reports();
        if bytes > state.import_headroom() {
            self.refused();
            if changed { self.rebalance(state); }
            return None;
        }
        state.next = state.next.checked_add(1).expect("Memory owner identity exhausted");
        let id = state.next;
        state.reservations.insert(id, bytes);
        self.rebalance(state);
        Some(MemoryReservation(Arc::new(Reservation { budget: self.clone(), id, bytes: AtomicU64::new(bytes), active: AtomicBool::new(true) })))
    }
    pub fn snapshot(&self) -> MemoryBudgetSnapshot {
        let state = self.0.state.lock().unwrap();
        let mut snapshot = MemoryBudgetSnapshot { ram_limit_bytes: state.ram_limit, ram_physical_bytes: state.ram_physical(false),
            ram_pinned_bytes: state.ram_physical(true).saturating_add(state.reserved()), reserved_bytes: state.reserved(),
            pressure:state.facts.pressure || state.facts.available_bytes.is_some_and(|available|available<reserve(state.facts)), refusals: self.0.cache.refusals.load(Ordering::Relaxed), ..Default::default() };
        for owner in state.owners.values() {
            let row = &mut snapshot.rows[owner.kind as usize][owner.class as usize];
            let report = owner.report;
            let grant = MemoryGrant { ram_bytes: owner.grants.ram.load(Ordering::Acquire), gpu_bytes: owner.grants.gpu.load(Ordering::Acquire) };
            row.owners += 1;
            row.ram_bytes = row.ram_bytes.saturating_add(report.ram_bytes);
            row.gpu_bytes = row.gpu_bytes.saturating_add(report.gpu_bytes);
            row.pinned_ram_bytes = row.pinned_ram_bytes.saturating_add(report.pinned_ram_bytes);
            row.pinned_gpu_bytes = row.pinned_gpu_bytes.saturating_add(report.pinned_gpu_bytes);
            row.granted_ram_bytes = row.granted_ram_bytes.saturating_add(grant.ram_bytes);
            row.granted_gpu_bytes = row.granted_gpu_bytes.saturating_add(grant.gpu_bytes);
            snapshot.ram_granted_bytes = snapshot.ram_granted_bytes.saturating_add(grant.ram_bytes)
                .saturating_add(if state.unified(owner.device) { grant.gpu_bytes } else { 0 });
        }
        snapshot.ram_granted_bytes = snapshot.ram_granted_bytes.saturating_add(state.reserved());
        snapshot.pinned_debt_bytes = snapshot.ram_pinned_bytes.saturating_sub(state.ram_limit);
        let (mut unified_debt, mut discrete_debt) = (0u64, 0u64);
        for (&device, facts) in &state.devices {
            let (physical, pinned, granted) = state.owners.values().filter(|owner| owner.device == Some(device)).fold((0u64,0u64,0u64), |(physical,pinned,granted),owner| (
                physical.saturating_add(owner.report.gpu_bytes), pinned.saturating_add(owner.report.pinned_gpu_bytes), granted.saturating_add(owner.grants.gpu.load(Ordering::Acquire))));
            let limit = device_limit(*facts, physical).min(if facts.unified { state.ram_limit } else { u64::MAX });
            let debt = pinned.saturating_sub(limit);
            if facts.unified { unified_debt = unified_debt.saturating_add(debt); } else { discrete_debt = discrete_debt.saturating_add(debt); }
            snapshot.devices.push(MemoryDeviceSnapshot { device, unified: facts.unified, limit_bytes: limit,
                physical_bytes: physical, pinned_bytes: pinned, granted_bytes: granted, pinned_debt_bytes: debt });
        }
        snapshot.pinned_debt_bytes = snapshot.pinned_debt_bytes.max(unified_debt).saturating_add(discrete_debt);
        snapshot
    }
}
fn fraction(bytes: u64, numerator: u64, denominator: u64) -> u64 { (u128::from(bytes) * u128::from(numerator) / u128::from(denominator)) as u64 }
fn reserve(facts: MemoryFacts) -> u64 { fraction(facts.total_bytes.unwrap_or(0), if facts.tablet {15} else {10},100).max(if facts.tablet {768 * MIB} else {1024 * MIB}) }
fn device_limit(facts: DeviceMemoryFacts, ours: u64) -> u64 {
    let ceiling = facts.budget_bytes.map_or_else(|| facts.heap_bytes.map_or(if facts.unified { u64::MAX } else { 0 }, |bytes| bytes / 2), |bytes| fraction(bytes,80,100));
    ceiling.saturating_sub(facts.fixed_bytes.max(facts.usage_bytes.unwrap_or(0).saturating_sub(ours)))
}
pub fn distribute(available: u64, requests: &[(usize, u64)]) -> Vec<(usize, u64)> {
    let mut grants = vec![0u64; requests.len()];
    let mut remaining = available;
    loop {
        let count = requests.iter().zip(&grants).filter(|((_, demand), grant)| demand > *grant).count() as u64;
        if count == 0 || remaining == 0 { break; }
        let share = (remaining / count).max(1);
        for ((_, demand), grant) in requests.iter().zip(&mut grants) {
            let added = demand.saturating_sub(*grant).min(share).min(remaining);
            *grant += added;
            remaining -= added;
        }
    }
    requests.iter().zip(grants).map(|((id,_), grant)| (*id,grant)).collect()
}
impl State {
    fn apply_reports(&mut self) -> bool {
        let mut changed=false;
        for owner in self.owners.values_mut() {
            if let Some((version,mut report))=owner.pending.read() && version!=owner.applied {
                report.ram_bytes=report.ram_bytes.max(report.pinned_ram_bytes);report.gpu_bytes=report.gpu_bytes.max(report.pinned_gpu_bytes);
                owner.report=report;owner.applied=version;changed=true;
            }
        }
        changed
    }

    fn reserved(&self) -> u64 { self.reservations.values().copied().fold(0, u64::saturating_add) }
    fn import_headroom(&self) -> u64 { self.ram_limit.saturating_sub(self.ram_physical(true)).saturating_sub(self.reserved()) }
    fn remove_retired_devices(&mut self) {
        self.devices.retain(|device,_| !self.retired_devices.contains(device) || self.owners.values().any(|owner|owner.device == Some(*device)));
        self.retired_devices.retain(|device| self.devices.contains_key(device));
    }
    fn unified(&self, device: Option<MemoryDevice>) -> bool { device.and_then(|device| self.devices.get(&device)).is_some_and(|facts| facts.unified) }
    fn ram_physical(&self, pinned: bool) -> u64 {
        self.owners.values().fold(0u64, |bytes,owner| bytes.saturating_add(if pinned {owner.report.pinned_ram_bytes}else {owner.report.ram_bytes})
            .saturating_add(if self.unified(owner.device) {if pinned {owner.report.pinned_gpu_bytes}else {owner.report.gpu_bytes}}else {0}))
    }
    fn rebalance(&self, cache: &BudgetCache) -> Vec<Arc<dyn Fn() + Send + Sync>> {
        cache.limit.store(self.ram_limit, Ordering::Release);
        let headroom=self.import_headroom();
        let headroom_changed=cache.headroom.swap(headroom, Ordering::AcqRel)!=headroom;
        let owners: Vec<_> = self.owners.values().collect();
        let mut gpu: Vec<_> = owners.iter().map(|owner| owner.report.pinned_gpu_bytes).collect();
        for (&device, facts) in &self.devices {
            let physical = owners.iter().filter(|owner| owner.device == Some(device)).fold(0u64, |sum,owner| sum.saturating_add(owner.report.gpu_bytes));
            let mut remaining = device_limit(*facts,physical).saturating_sub(owners.iter().filter(|owner| owner.device == Some(device)).fold(0u64, |sum,owner|sum.saturating_add(owner.report.pinned_gpu_bytes)));
            for class in 0..MEMORY_CLASSES {
                let requests:Vec<_> = owners.iter().enumerate().filter(|(_,owner)|owner.device==Some(device) && owner.class as usize==class)
                    .map(|(i,owner)|(i,owner.report.demand_gpu_bytes.saturating_sub(gpu[i]))).collect();
                for (i,grant) in distribute(remaining,&requests) { gpu[i]=gpu[i].saturating_add(grant); remaining-=grant; }
            }
        }
        let mut ram: Vec<_> = owners.iter().map(|owner|owner.report.pinned_ram_bytes).collect();
        let mut unified: Vec<_> = owners.iter().enumerate().map(|(i,owner)|if self.unified(owner.device) {owner.report.pinned_gpu_bytes}else {gpu[i]}).collect();
        let mut remaining = self.import_headroom();
        for class in 0..MEMORY_CLASSES {
            let mut requests=Vec::new();
            for (i,owner) in owners.iter().enumerate().filter(|(_,owner)|owner.class as usize==class) {
                requests.push((2*i,owner.report.demand_ram_bytes.saturating_sub(ram[i])));
                if self.unified(owner.device) {requests.push((2*i+1,gpu[i].saturating_sub(unified[i])));}
            }
            for (id,grant) in distribute(remaining,&requests) {
                if id%2==0 {ram[id/2]=ram[id/2].saturating_add(grant);} else {unified[id/2]=unified[id/2].saturating_add(grant);}
                remaining-=grant;
            }
        }
        let mut wakes = Vec::new();
        for (i,owner) in owners.iter().enumerate() {
            let changed=owner.grants.ram.swap(ram[i],Ordering::AcqRel)!=ram[i];
            let gpu_changed=owner.grants.gpu.swap(unified[i],Ordering::AcqRel)!=unified[i];
            if (changed || gpu_changed || headroom_changed) && let Some(wake)=owner.grants.wake.get() {wakes.push(wake.clone());}
        }
        wakes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const GIB: u64 = 1024 * MIB;
    fn facts() -> MemoryFacts { MemoryFacts {total_bytes:Some(16*GIB),available_bytes:Some(12*GIB),..Default::default()} }
    fn device(unified: bool) -> DeviceMemoryFacts { DeviceMemoryFacts {budget_bytes:Some(4*GIB),unified,..Default::default()} }
    fn owner(budget:&MemoryBudget,class:MemoryClass,device:Option<MemoryDevice>,ram:u64,gpu:u64)->MemoryOwner {
        let owner=budget.register_owner(MemoryOwnerKind::Scene,class,device);
        owner.report(MemoryReport {demand_ram_bytes:ram,demand_gpu_bytes:gpu,..Default::default()});owner
    }
    #[test]
    fn grant_wakes_can_complete_pressure_work_and_reenter_the_budget() {
        let budget = MemoryBudget::with_facts(facts());
        let cache = owner(&budget, MemoryClass::Warm, None, u64::MAX, 0);
        let locked = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicU64::new(0));
        let manager = Arc::downgrade(&budget.0);
        let observed = locked.clone();
        let completed = calls.clone();
        cache.set_wake(Arc::new(move || {
            completed.fetch_add(1, Ordering::Relaxed);
            let budget = MemoryBudget(manager.upgrade().unwrap());
            if budget.0.state.try_lock().is_err() {
                observed.store(true, Ordering::Relaxed);
                return;
            }
            budget.flush_reports();
            let _ = budget.snapshot();
        }));
        let report = |bytes| MemoryReport { ram_bytes: bytes, pinned_ram_bytes: bytes,
            demand_ram_bytes: u64::MAX, ..Default::default() };
        cache.report(report(MIB));
        budget.update_ram(MemoryFacts { override_bytes: Some(2 * GIB), ..facts() }, 1000);
        assert!(cache.publish(report(2 * MIB)));
        budget.flush_reports();
        let reservation = budget.admit_import(64 * MIB).unwrap();
        assert!(reservation.resize(128 * MIB));
        let second = budget.admit_import(64 * MIB).unwrap();
        reservation.merge(second).unwrap();
        assert!(cache.publish(report(3 * MIB)));
        assert!(reservation.resize(193 * MIB));
        let authored = budget.register_owner(MemoryOwnerKind::Authored, MemoryClass::Pinned, None);
        reservation.commit(&authored, report(64 * MIB)).unwrap();
        let reservation = budget.admit_import(MIB).unwrap();
        drop(reservation);
        drop(authored);
        cache.report(report(0));
        budget.set_ram_override(Some(GIB));
        assert!(calls.load(Ordering::Relaxed) >= 10);
        assert!(!locked.load(Ordering::Relaxed), "grant wakes must run after releasing the budget lock");
    }
    #[test]
    fn cloned_handles_and_windows_charge_one_physical_owner_until_final_drop() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);
        let first=owner(&budget,MemoryClass::Visible,None,GIB,0);
        first.report(MemoryReport {ram_bytes:700*MIB,demand_ram_bytes:GIB,..Default::default()});
        let second_window=first.clone();
        assert_eq!(budget.snapshot().ram_physical_bytes,700*MIB);
        assert_eq!(budget.snapshot().rows[MemoryOwnerKind::Scene as usize][MemoryClass::Visible as usize].owners,1);
        drop(first);assert_eq!(budget.snapshot().ram_physical_bytes,700*MIB);
        let other=owner(&budget,MemoryClass::Visible,None,GIB,0);
        other.report(MemoryReport {ram_bytes:500*MIB,demand_ram_bytes:GIB,..Default::default()});
        assert_eq!(budget.snapshot().ram_physical_bytes,1200*MIB);
        drop(second_window);assert_eq!(budget.snapshot().ram_physical_bytes,500*MIB);
        drop(other);assert_eq!(budget.snapshot().ram_physical_bytes,0);
    }
    #[test]
    fn unified_gpu_is_counted_once_in_ram_while_discrete_devices_are_independent() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);
        let unified=budget.register_device(device(true));let discrete=budget.register_device(device(false));
        let first=owner(&budget,MemoryClass::Visible,Some(unified),GIB,2*GIB);
        first.report(MemoryReport {ram_bytes:100*MIB,gpu_bytes:200*MIB,demand_ram_bytes:GIB,demand_gpu_bytes:2*GIB,..Default::default()});
        let second=owner(&budget,MemoryClass::Visible,Some(discrete),GIB,2*GIB);
        second.report(MemoryReport {ram_bytes:300*MIB,gpu_bytes:400*MIB,demand_ram_bytes:GIB,demand_gpu_bytes:2*GIB,..Default::default()});
        let snapshot=budget.snapshot();assert_eq!(snapshot.ram_physical_bytes,600*MIB);
        assert_eq!(snapshot.ram_granted_bytes,4*GIB);
        assert_eq!(snapshot.devices[0].physical_bytes,200*MIB);assert_eq!(snapshot.devices[1].physical_bytes,400*MIB);
        assert_eq!(first.grant(),MemoryGrant {ram_bytes:GIB,gpu_bytes:2*GIB});
        assert_eq!(second.grant(),MemoryGrant {ram_bytes:GIB,gpu_bytes:2*GIB});
        budget.unregister_device(unified);
        assert_eq!(budget.snapshot().ram_physical_bytes,600*MIB);
        assert_eq!(budget.snapshot().devices.len(),2);
        drop(first);
        assert_eq!(budget.snapshot().ram_physical_bytes,300*MIB);
        assert_eq!(budget.snapshot().devices.len(),1);
        drop(second);budget.unregister_device(discrete);assert!(budget.snapshot().devices.is_empty());
    }
    #[test]
    fn grants_preserve_visible_priority_and_share_one_process_limit() {
        let budget=MemoryBudget::new();
        budget.update_ram(MemoryFacts {override_bytes:Some(GIB),..facts()},0);
        let first=owner(&budget,MemoryClass::Visible,None,u64::MAX,0);
        let second=owner(&budget,MemoryClass::Visible,None,u64::MAX,0);
        let warm=owner(&budget,MemoryClass::Warm,None,u64::MAX,0);
        assert_eq!(first.grant().ram_bytes,GIB/2);assert_eq!(second.grant().ram_bytes,GIB/2);assert_eq!(warm.grant().ram_bytes,0);
        drop(second);assert_eq!(first.grant().ram_bytes,GIB);assert_eq!(warm.grant().ram_bytes,0);
        first.report(MemoryReport {demand_ram_bytes:300*MIB,..Default::default()});
        assert_eq!(warm.grant().ram_bytes,GIB-300*MIB);
    }
    struct ControllerTrace { limits: Vec<u64>, quarter_cuts: u64 }
    impl ControllerTrace {
        fn steady_score(&self) -> [u64; 4] {
            let values = self.limits.iter().skip(600).map(|bytes| bytes / MIB).collect::<Vec<_>>();
            assert!(!values.is_empty());
            [values.iter().sum::<u64>() / values.len() as u64,
                *values.iter().min().unwrap(), *values.iter().max().unwrap(), self.quarter_cuts]
        }
    }
    fn controller_noise(rng: &mut u64, other_bytes: u64, percent: u64) -> i128 {
        *rng ^= (*rng).wrapping_shl(13);
        *rng ^= *rng >> 7;
        *rng ^= (*rng).wrapping_shl(17);
        let span = 2 * percent * other_bytes / 100 + 1;
        i128::from(*rng % span) - i128::from(percent * other_bytes / 100)
    }
    fn controller_samples(other_mib: u64, noise_percent: u64, seconds: u64,
        external_change: impl Fn(u64) -> (u64, bool)) -> ControllerTrace
    {
        let budget = MemoryBudget::new();
        let cache = owner(&budget, MemoryClass::Visible, None, 8 * GIB, 0);
        let mut resident = 0;
        let mut previous = 0;
        let mut rng = 0x9E3779B97F4A7C15;
        let mut trace = ControllerTrace { limits: Vec::new(), quarter_cuts: 0 };
        for second in 0..seconds {
            let other_bytes = other_mib * MIB;
            let noise = controller_noise(&mut rng, other_bytes, noise_percent);
            let (extra, pressure) = external_change(second);
            let available = (i128::from(8 * GIB) - i128::from(other_bytes) - noise
                - i128::from(extra) - i128::from(resident)).max(0) as u64;
            assert_eq!(budget.snapshot().ram_physical_bytes, resident);
            budget.update_ram(MemoryFacts { total_bytes: Some(8 * GIB),
                available_bytes: Some(available), tablet: true, pressure, ..Default::default() }, second * 1000);
            let limit = budget.ram_limit_bytes();
            assert_eq!(cache.grant().ram_bytes, limit);
            if previous != 0 && limit < previous && limit <= previous * 3 / 4 {
                trace.quarter_cuts += 1;
            }
            previous = limit;
            resident = if resident < limit { resident + (limit - resident) / 2 } else { limit };
            cache.report(MemoryReport { ram_bytes: resident, demand_ram_bytes: 8 * GIB, ..Default::default() });
            trace.limits.push(limit);
        }
        trace
    }
    #[test]
    fn sustained_pressure_does_not_regrow_the_budget() {
        let trace = controller_samples(3584, 0, 638, |second| (0, (600..632).contains(&second)));
        let before = trace.limits[599];
        let cut = trace.limits[600];
        assert_eq!([before / MIB, cut / MIB], [3360, 2520]);
        assert_eq!(cut, before * 3 / 4);
        assert_eq!(trace.quarter_cuts, 1);
        assert!(trace.limits[601..632].iter().all(|&limit| limit == cut));
        let grown = cut + cut / 10;
        assert_eq!(trace.limits[632], grown);
        assert!(trace.limits[633..637].iter().all(|&limit| limit == grown));
        assert_eq!(trace.limits[637], grown + grown / 10);
    }
    #[test]
    fn isolated_low_samples_do_not_collapse_the_budget() {
        let budget = MemoryBudget::new();
        let cache = owner(&budget, MemoryClass::Visible, None, 8 * GIB, 0);
        let tablet = |available| MemoryFacts { total_bytes: Some(8 * GIB),
            available_bytes: Some(available), tablet: true, ..Default::default() };
        budget.update_ram(tablet(6 * GIB), 0);
        let initial = budget.ram_limit_bytes();
        assert_eq!(initial / MIB, 3686);
        for second in 1..=600 {
            cache.report(MemoryReport { ram_bytes: 0, demand_ram_bytes: 8 * GIB, ..Default::default() });
            budget.update_ram(tablet(if second % 30 == 0 { GIB } else { 6 * GIB }), second * 1000);
            assert_eq!(budget.snapshot().ram_physical_bytes, 0);
            assert_eq!(cache.grant().ram_bytes, initial, "sample {second}");
            assert_eq!(budget.ram_limit_bytes(), initial, "sample {second}");
        }
    }
    #[test]
    fn closed_loop_controls_match_scored_limits() {
        for (others, noise, expected) in [
            (3584, 0, [3360, 3360, 3360, 0]),
            (4608, 2, [2174, 2174, 2174, 0]),
            (2560, 2, [3686, 3686, 3686, 0]),
        ] {
            let trace = controller_samples(others, noise, 1800, |_| (0, false));
            assert_eq!(trace.steady_score(), expected, "others={others}, noise={noise}");
        }
    }
    #[test]
    fn noisy_closed_loop_matches_scored_limits() {
        for (noise, expected) in [(2, [3145, 3145, 3145, 1]), (5, [3206, 2433, 3250, 2])] {
            let trace = controller_samples(3584, noise, 1800, |_| (0, false));
            assert_eq!(trace.steady_score(), expected, "noise={noise}");
        }
    }
    #[test]
    fn one_sample_external_loss_keeps_scored_limit() {
        let trace = controller_samples(3584, 0, 400,
            |second| (if second == 200 { 800 * MIB } else { 0 }, false));
        let lowest = trace.limits[200..230].iter().map(|bytes| bytes / MIB).min().unwrap();
        assert_eq!([trace.limits[199] / MIB, lowest, trace.limits[230] / MIB,
            trace.limits[320] / MIB], [3360; 4]);
        assert_eq!(trace.quarter_cuts, 0);
    }
    #[test]
    fn sustained_external_loss_reaches_scored_limit() {
        let trace = controller_samples(3584, 0, 400,
            |second| (if second >= 200 { 2 * GIB } else { 0 }, false));
        let before = trace.limits[199] / MIB;
        let first = (200..400).find(|&second| trace.limits[second] / MIB < before * 9 / 10).unwrap();
        assert_eq!([before, (first - 200) as u64, trace.limits[399] / MIB], [3360, 2, 1024]);
    }
    #[test]
    fn process_and_user_caps_apply_below_the_controller_band() {
        for process_cap in [false, true] {
            let budget = MemoryBudget::with_facts(facts());
            let cap = fraction(budget.ram_limit_bytes(), 99, 100);
            let facts = if process_cap { MemoryFacts { process_limit_bytes: Some(cap), ..facts() } }
                else { MemoryFacts { override_bytes: Some(cap), ..facts() } };
            budget.update_ram(facts, 1000);
            assert_eq!(budget.ram_limit_bytes(), cap);
        }
    }
    #[test]
    fn pressure_and_configuration_cannot_reduce_the_final_target_below_the_floor() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(512*MIB),..facts()});
        budget.update_ram(MemoryFacts {pressure:true,..facts()},1000);
        assert_eq!(budget.ram_limit_bytes(),384*MIB);
        budget.set_ram_override(Some(64*MIB));
        budget.update_ram(MemoryFacts {available_bytes:Some(0),process_limit_bytes:Some(32*MIB),pressure:true,..facts()},2000);
        assert_eq!(budget.ram_limit_bytes(),384*MIB);
        let owner=owner(&budget,MemoryClass::Pinned,None,0,0);
        owner.report(MemoryReport {ram_bytes:512*MIB,pinned_ram_bytes:512*MIB,..Default::default()});
        assert_eq!(budget.snapshot().pinned_debt_bytes,128*MIB);
        assert!(budget.admit_import(1).is_none());
    }
    #[test]
    fn lowered_limit_preserves_pinned_owner_and_reports_debt() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);
        let gpu=budget.register_device(device(true));
        let stroke=owner(&budget,MemoryClass::Pinned,Some(gpu),0,0);
        stroke.report(MemoryReport {ram_bytes:500*MIB,gpu_bytes:800*MIB,pinned_ram_bytes:500*MIB,pinned_gpu_bytes:800*MIB,..Default::default()});
        budget.update_ram(MemoryFacts {override_bytes:Some(384*MIB),..facts()},1000);
        assert_eq!(stroke.grant(),MemoryGrant {ram_bytes:500*MIB,gpu_bytes:800*MIB});
        let snapshot=budget.snapshot();assert_eq!(snapshot.ram_pinned_bytes,1300*MIB);
        assert_eq!(snapshot.pinned_debt_bytes,916*MIB);assert!(budget.admit_import(1).is_none());
    }
    #[test]
    fn import_above_512_mib_is_admitted_and_refusal_does_not_change_existing_reports() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);
        let artwork=owner(&budget,MemoryClass::Pinned,None,0,0);
        artwork.report(MemoryReport {ram_bytes:GIB,pinned_ram_bytes:GIB,..Default::default()});
        assert!(budget.admit_import(700*MIB).is_some());
        let before=budget.snapshot();assert!(budget.admit_import(16*GIB).is_none());let after=budget.snapshot();
        assert_eq!(before.rows,after.rows);assert_eq!(before.ram_physical_bytes,after.ram_physical_bytes);assert_eq!(after.refusals,before.refusals+1);
    }
    #[test]
    fn unknown_and_partial_facts_use_configured_or_host_default_limits() {
        for (facts, expected) in [
            (MemoryFacts::default(),384*MIB),
            (MemoryFacts {host_default_bytes:Some(GIB),..Default::default()},GIB),
            (MemoryFacts {total_bytes:Some(16*GIB),..Default::default()},fraction(16*GIB,60,100)),
            (MemoryFacts {total_bytes:Some(8*GIB),tablet:true,..Default::default()},fraction(8*GIB,45,100)),
            (MemoryFacts {process_limit_bytes:Some(128*MIB),..Default::default()},384*MIB),
            (MemoryFacts {host_default_bytes:Some(64*MIB),override_bytes:Some(128*MIB),process_limit_bytes:Some(32*MIB),process_used_bytes:Some(31*MIB),..Default::default()},384*MIB),
            (MemoryFacts {available_bytes:Some(0),..Default::default()},384*MIB),
        ] {
            let budget=MemoryBudget::with_facts(facts);assert_eq!(budget.ram_limit_bytes(),expected);
            assert_eq!(budget.import_headroom_bytes(),expected);
        }
        let budget=MemoryBudget::with_facts(MemoryFacts {process_limit_bytes:Some(0),..facts()});
        assert_eq!(budget.ram_limit_bytes(),384*MIB);
    }
    #[test]
    fn process_headroom_does_not_subtract_owned_pinned_memory_twice() {
        let budget=MemoryBudget::new();let artwork=owner(&budget,MemoryClass::Pinned,None,0,0);
        artwork.report(MemoryReport {ram_bytes:2*GIB,pinned_ram_bytes:2*GIB,..Default::default()});
        budget.update_ram(MemoryFacts {process_limit_bytes:Some(4*GIB),process_used_bytes:Some(3*GIB),..facts()},0);
        assert_eq!(budget.snapshot().ram_limit_bytes,3*GIB);assert!(budget.admit_import(GIB).is_some());assert!(budget.admit_import(GIB+1).is_none());
    }
    #[test]
    fn operation_reservations_are_atomic_across_concurrent_imports() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(GIB),..facts()});
        let barrier=Arc::new(std::sync::Barrier::new(3));
        std::thread::scope(|scope| {
            let first={let budget=budget.clone();let barrier=barrier.clone();scope.spawn(move||{barrier.wait();budget.admit_import(700*MIB)})};
            let second={let budget=budget.clone();let barrier=barrier.clone();scope.spawn(move||{barrier.wait();budget.admit_import(700*MIB)})};
            barrier.wait();
            let reservations=[first.join().unwrap(),second.join().unwrap()];
            assert_eq!(reservations.iter().filter(|token|token.is_some()).count(),1);
            assert_eq!(budget.snapshot().reserved_bytes,700*MIB);assert_eq!(budget.snapshot().refusals,1);
            drop(reservations);assert_eq!(budget.snapshot().reserved_bytes,0);
        });
    }
    #[test]
    fn evictable_cache_bytes_yield_to_a_pinned_import_reservation() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(GIB),..facts()});
        let cache=owner(&budget,MemoryClass::Warm,None,900*MIB,0);
        cache.report(MemoryReport {ram_bytes:900*MIB,demand_ram_bytes:900*MIB,..Default::default()});
        let artwork=owner(&budget,MemoryClass::Pinned,None,0,0);
        artwork.report(MemoryReport {ram_bytes:100*MIB,pinned_ram_bytes:100*MIB,..Default::default()});
        let token=budget.admit_import(700*MIB).unwrap();
        assert_eq!(budget.snapshot().ram_physical_bytes,1000*MIB);
        assert_eq!(budget.snapshot().ram_pinned_bytes,800*MIB);
        assert_eq!(cache.grant().ram_bytes,224*MIB);
        drop(token);assert_eq!(cache.grant().ram_bytes,900*MIB);
    }
    #[test]
    fn reservation_clones_resize_and_convert_to_one_physical_owner() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(GIB),..facts()});
        let token=budget.admit_import(700*MIB).unwrap();let retained=token.clone();assert!(token.active());
        assert!(!token.resize(2*GIB));assert_eq!(token.reserved_bytes(),700*MIB);
        assert!(token.resize(600*MIB));assert_eq!(retained.reserved_bytes(),600*MIB);
        let artwork=owner(&budget,MemoryClass::Pinned,None,0,0);
        token.commit(&artwork,MemoryReport {ram_bytes:600*MIB,pinned_ram_bytes:600*MIB,..Default::default()}).unwrap();
        assert_eq!(retained.reserved_bytes(),0);assert!(!retained.active());assert!(!retained.resize(MIB));
        assert_eq!(budget.snapshot().reserved_bytes,0);assert_eq!(budget.snapshot().ram_pinned_bytes,600*MIB);
        assert!(retained.commit(&artwork,MemoryReport::default()).is_err());
        assert_eq!(budget.snapshot().ram_pinned_bytes,600*MIB);
    }
    #[test]
    fn startup_budget_has_a_limit_before_any_sampler_tick() {
        let budget=MemoryBudget::with_facts(facts());
        assert_eq!(budget.ram_limit_bytes(),fraction(16*GIB,60,100));
        assert!(budget.admit_import(700*MIB).is_some());
        let facts=system_facts();
        if facts.process_limit_bytes != Some(0) { assert!(MemoryBudget::process().ram_limit_bytes()>0); }
    }
    #[test]
    fn driver_budget_and_heap_fallback_subtract_only_fixed_bytes() {
        assert_eq!(device_limit(DeviceMemoryFacts {budget_bytes:Some(1000),usage_bytes:Some(600),fixed_bytes:50,..Default::default()},400),600);
        assert_eq!(device_limit(DeviceMemoryFacts {heap_bytes:Some(1000),fixed_bytes:100,..Default::default()},0),400);
        assert_eq!(device_limit(DeviceMemoryFacts::default(),0),0);
        let budget=MemoryBudget::new();let device=budget.register_device(DeviceMemoryFacts {budget_bytes:Some(1000),..Default::default()});
        let first=owner(&budget,MemoryClass::Visible,Some(device),0,1000);assert_eq!(first.grant().gpu_bytes,800);
        budget.update_device(device,DeviceMemoryFacts {budget_bytes:Some(100),..Default::default()});assert_eq!(first.grant().gpu_bytes,80);
    }
    #[test]
    fn saturated_reports_and_unknown_gpu_do_not_overflow_or_drop_pins() {
        let budget=MemoryBudget::new();budget.update_ram(MemoryFacts {total_bytes:Some(u64::MAX),available_bytes:Some(u64::MAX),..Default::default()},0);
        let device=budget.register_device(DeviceMemoryFacts {unified:true,..Default::default()});
        let first=owner(&budget,MemoryClass::Pinned,Some(device),0,0);
        first.report(MemoryReport {ram_bytes:u64::MAX,gpu_bytes:u64::MAX,pinned_ram_bytes:u64::MAX,pinned_gpu_bytes:u64::MAX,..Default::default()});
        let second=owner(&budget,MemoryClass::Warm,None,u64::MAX,0);
        assert_eq!(first.grant().ram_bytes,u64::MAX);assert_eq!(first.grant().gpu_bytes,u64::MAX);assert_eq!(second.grant().ram_bytes,0);
        let snapshot=budget.snapshot();assert_eq!(snapshot.ram_physical_bytes,u64::MAX);assert_eq!(snapshot.ram_pinned_bytes,u64::MAX);
        assert_eq!(snapshot.devices[0].pinned_debt_bytes,u64::MAX.saturating_sub(snapshot.ram_limit_bytes));
    }
    #[test]
    fn unified_device_debt_remains_visible_when_the_ram_pool_fits() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);
        let device=budget.register_device(DeviceMemoryFacts {budget_bytes:Some(100*MIB),unified:true,..Default::default()});
        let owner=owner(&budget,MemoryClass::Pinned,Some(device),0,0);
        owner.report(MemoryReport {gpu_bytes:200*MIB,pinned_gpu_bytes:200*MIB,..Default::default()});
        let snapshot=budget.snapshot();assert_eq!(snapshot.ram_pinned_bytes,200*MIB);
        assert_eq!(snapshot.pinned_debt_bytes,120*MIB);assert_eq!(snapshot.devices[0].pinned_debt_bytes,120*MIB);
    }
    #[test]
    fn cached_grants_are_available_while_the_manager_is_locked() {
        let budget=MemoryBudget::new();budget.update_ram(facts(),0);let owner=owner(&budget,MemoryClass::Visible,None,GIB,0);
        let _held=budget.0.state.lock().unwrap();assert_eq!(owner.grant().ram_bytes,GIB);
        assert!(budget.ram_limit_bytes()>GIB);assert!(budget.import_headroom_bytes()>GIB);
        assert!(budget.import_headroom_bytes()<u64::MAX);
    }
    #[test]
    fn published_reports_never_take_the_budget_lock_and_flush_as_coherent_snapshots() {
        let budget=MemoryBudget::with_facts(facts());let owner=owner(&budget,MemoryClass::Visible,None,GIB,0);
        let report=MemoryReport {ram_bytes:100*MIB,gpu_bytes:200*MIB,pinned_ram_bytes:50*MIB,pinned_gpu_bytes:80*MIB,demand_ram_bytes:2*GIB,demand_gpu_bytes:3*GIB};
        {let _held=budget.0.state.lock().unwrap();assert!(owner.publish(report));assert_eq!(owner.grant().ram_bytes,GIB);}
        budget.flush_reports();let row=budget.snapshot().rows[MemoryOwnerKind::Scene as usize][MemoryClass::Visible as usize];
        assert_eq!((row.ram_bytes,row.gpu_bytes,row.pinned_ram_bytes,row.pinned_gpu_bytes),(100*MIB,200*MIB,50*MIB,80*MIB));
        assert_eq!(owner.grant().ram_bytes,2*GIB);
        owner.0.pending.version.store(3,Ordering::Release);owner.0.pending.values[0].store(900*MIB,Ordering::Relaxed);
        assert!(!owner.publish(MemoryReport::default()));budget.flush_reports();assert_eq!(budget.snapshot().ram_physical_bytes,100*MIB);
        for (field,value) in owner.0.pending.values.iter().zip([300*MIB,600*MIB,150*MIB,240*MIB,GIB,2*GIB]) {field.store(value,Ordering::Relaxed);}
        owner.0.pending.version.store(4,Ordering::Release);budget.flush_reports();
        let row=budget.snapshot().rows[MemoryOwnerKind::Scene as usize][MemoryClass::Visible as usize];
        assert_eq!((row.ram_bytes,row.gpu_bytes,row.pinned_ram_bytes,row.pinned_gpu_bytes),(300*MIB,600*MIB,150*MIB,240*MIB));
        assert!(owner.publish(report));owner.report(MemoryReport::default());budget.flush_reports();assert_eq!(budget.snapshot().ram_physical_bytes,0);
    }
    #[test]
    fn merging_reservations_preserves_existing_charge_and_invalidates_other_aliases() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(GIB),..facts()});
        let first=budget.admit_import(600*MIB).unwrap();let second=budget.admit_import(400*MIB).unwrap();let alias=second.clone();
        first.merge(second).unwrap();assert_eq!(first.reserved_bytes(),1000*MIB);assert_eq!(budget.snapshot().reserved_bytes,1000*MIB);
        assert!(!alias.active());assert_eq!(alias.reserved_bytes(),0);drop(alias);assert_eq!(budget.snapshot().reserved_bytes,1000*MIB);
        first.merge(first.clone()).unwrap();assert_eq!(budget.snapshot().reserved_bytes,1000*MIB);drop(first);assert_eq!(budget.snapshot().reserved_bytes,0);
    }
    #[test]
    fn operation_admission_and_resize_apply_pending_pins_before_checking_headroom() {
        let budget=MemoryBudget::with_facts(MemoryFacts {override_bytes:Some(GIB),..facts()});
        let owner=owner(&budget,MemoryClass::Pinned,None,0,0);let token=budget.admit_import(100*MIB).unwrap();
        assert!(owner.publish(MemoryReport {ram_bytes:800*MIB,pinned_ram_bytes:800*MIB,..Default::default()}));
        assert!(budget.admit_import(200*MIB).is_none());assert!(!token.resize(300*MIB));assert_eq!(token.reserved_bytes(),100*MIB);
        assert_eq!(budget.snapshot().ram_pinned_bytes,900*MIB);
    }
    #[test]
    fn zero_byte_operation_reservation_stays_active_until_conversion() {
        let budget=MemoryBudget::new();let token=budget.admit_import(0).unwrap();assert!(token.active());assert_eq!(token.reserved_bytes(),0);
        let owner=owner(&budget,MemoryClass::Pinned,None,0,0);let clone=token.clone();token.commit(&owner,MemoryReport::default()).unwrap();assert!(!clone.active());
    }
    #[test]
    fn internal_override_survives_fresh_sampler_facts() {
        let budget=MemoryBudget::new();budget.set_ram_override(Some(600*MIB));budget.update_ram(facts(),0);
        assert_eq!(budget.snapshot().ram_limit_bytes,600*MIB);
        budget.update_ram(facts(),1000);assert_eq!(budget.snapshot().ram_limit_bytes,600*MIB);
    }
}

#[cfg(test)]
mod unified_unknown_tests {
    use super::*;
    #[test]
    fn browser_gpu_without_driver_facts_shares_the_ram_limit() {
        let budget = MemoryBudget::with_facts(MemoryFacts {host_default_bytes: Some(512 * MIB), ..Default::default()});
        let device = budget.register_device(DeviceMemoryFacts {unified:true, ..Default::default()});
        let owner = budget.register_owner(MemoryOwnerKind::Scene, MemoryClass::Visible, Some(device));
        owner.report(MemoryReport {demand_gpu_bytes: 1024 * MIB, ..Default::default()});
        assert_eq!(owner.grant().gpu_bytes, 512 * MIB);
        assert_eq!(budget.snapshot().devices[0].limit_bytes, 512 * MIB);
        owner.report(MemoryReport {demand_ram_bytes: 1024 * MIB, demand_gpu_bytes: 1024 * MIB, ..Default::default()});
        assert_eq!(owner.grant().gpu_bytes + owner.grant().ram_bytes, 512 * MIB);
    }
}
