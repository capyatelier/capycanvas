use std::sync::{Arc, atomic::{AtomicU32, AtomicU64, Ordering::SeqCst, fence}};
#[cfg(not(target_arch = "wasm32"))]
use crate::device_poll::RetirementHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class { Authored, Visible, Fallback, Navigator, Prefetch, Refinement }
impl Class {
    pub(crate) fn floor_reserved(self) -> bool { self <= Self::Fallback }
    pub(crate) fn required(self) -> bool { self <= Self::Visible }
    pub(crate) fn demanded(self) -> bool { self <= Self::Navigator }
}

pub(crate) struct Tier {
    used: AtomicU64,
    yielding: AtomicU64,
    limit: AtomicU64,
    floor: AtomicU64,
    grant: AtomicU64,
    frame: AtomicU64,
    refusals: std::sync::Mutex<Vec<(std::sync::Weak<AtomicU64>, u64)>>,
    retiring: Option<Arc<Tier>>,
    parent: Option<Arc<Tier>>,
    ledger: Option<layer_core::MemoryOwner>,
    heap: Arc<super::Heap>,
    #[cfg(not(target_arch = "wasm32"))]
    edge: std::sync::OnceLock<RetirementHandle>,
}
impl Default for Tier {
    fn default() -> Self {
        Self { used: AtomicU64::new(0), yielding: AtomicU64::new(0), limit: AtomicU64::new(0), floor: AtomicU64::new(0), grant: AtomicU64::new(u64::MAX), frame: AtomicU64::new(u64::MAX), refusals: Default::default(), retiring: None, parent: None, ledger: None, heap: Default::default(),
            #[cfg(not(target_arch = "wasm32"))] edge: Default::default() }
    }
}
impl Tier {
    pub(crate) fn retiring_into(retiring: &Arc<Self>) -> Arc<Self> { Arc::new(Self { retiring: Some(retiring.clone()), ..Default::default() }) }
    pub(crate) fn limited_by(ledger: layer_core::MemoryOwner) -> Arc<Self> { Arc::new(Self { ledger: Some(ledger), ..Default::default() }) }
    pub(crate) fn within(parent: &Arc<Self>) -> Arc<Self> { Arc::new(Self { parent: Some(parent.clone()), limit: AtomicU64::new(u64::MAX), heap: parent.heap.clone(), ..Default::default() }) }
    pub(crate) fn heap(&self) -> &Arc<super::Heap> { &self.heap }
    fn root_used(&self) -> u64 { self.parent.as_ref().map_or_else(|| self.used(), |parent| parent.root_used()) }
    fn root_evictable(&self, class: Class) -> u64 { self.parent.as_ref().map_or_else(|| self.evictable(class), |parent| parent.root_evictable(class)) }
    pub(crate) fn yielding(&self) -> u64 { self.yielding.load(SeqCst) }
    fn offer(&self, bytes: u64) {
        self.yielding.fetch_add(bytes, SeqCst);
        self.release_edge();
        if let Some(parent) = &self.parent { parent.offer(bytes); }
    }
    fn withdraw(&self, bytes: u64) {
        self.yielding.fetch_sub(bytes, SeqCst);
        if let Some(parent) = &self.parent { parent.withdraw(bytes); }
    }
    fn add(&self, bytes: u64) { self.used.fetch_add(bytes, SeqCst); if let Some(parent) = &self.parent { parent.add(bytes); } }
    fn release(&self, bytes: u64) {
        self.used.fetch_sub(bytes, SeqCst);
        self.release_edge();
        if let Some(parent) = &self.parent { parent.release(bytes); }
    }
    pub(crate) fn used(&self) -> u64 { self.used.load(SeqCst) }
    pub(crate) fn begin_frame(&self) { self.frame.store(self.used(), SeqCst); }
    pub(crate) fn end_frame(&self) {
        let used = self.used();
        self.refusals.lock().unwrap().retain_mut(|(ahead, at)| {
            let Some(ahead) = ahead.upgrade() else { return false; };
            if *at != u64::MAX { ahead.store(at.saturating_sub(used), SeqCst); *at = u64::MAX; }
            true
        });
    }
    pub(crate) fn settle(&self) -> bool {
        let mut settled = false;
        self.refusals.lock().unwrap().retain(|(ahead, at)| {
            let Some(ahead) = ahead.upgrade() else { return false; };
            if *at == u64::MAX { settled |= ahead.swap(0, SeqCst) != 0; }
            true
        });
        if settled { self.release_edge(); }
        settled
    }
    fn refusal(self: &Arc<Self>, class: Class, bytes: u64, ledger: Option<layer_core::MemoryOwner>) -> Blocked {
        let (start, used) = (self.frame.load(SeqCst), self.used());
        let ahead = Arc::new(AtomicU64::new(if start == u64::MAX { 0 } else { used.saturating_sub(start) }));
        if start != u64::MAX { self.refusals.lock().unwrap().push((Arc::downgrade(&ahead), used)); }
        Blocked { tier: self.clone(), class, bytes, ahead, ledger }
    }
    pub(crate) fn is_root(&self) -> bool { self.parent.is_none() }
    pub(crate) fn limit(&self) -> u64 { self.ledger.as_ref().map_or_else(|| self.limit.load(SeqCst), layer_core::MemoryOwner::published_gpu_working_limit) }
    fn root_ledger(&self) -> Option<&layer_core::MemoryOwner> { self.parent.as_ref().map_or(self.ledger.as_ref(), |parent| parent.root_ledger()) }
    pub(crate) fn floor(&self) -> u64 { self.floor.load(SeqCst) }
    pub(crate) fn set_limit(&self, limit: u64, floor: u64) {
        if (self.limit.swap(limit, SeqCst) < limit) | (self.floor.swap(floor, SeqCst) > floor) { self.release_edge(); }
    }
    pub(crate) fn above_optional(&self) -> u64 { self.used().saturating_sub(self.available(Class::Refinement)) }
    pub(crate) fn headroom(&self, class: Class) -> u64 {
        let own = self.available(class).saturating_sub(self.used());
        self.parent.as_ref().map_or(own, |parent| own.min(parent.headroom(class)))
    }
    pub(crate) fn update_limit(&self,limit:u64) {
        if self.limit.swap(limit,SeqCst)<limit {self.release_edge();}
    }
    pub(crate) fn grant(&self) -> u64 { self.grant.load(SeqCst) }
    pub(crate) fn set_grant(&self, grant: u64) {
        if self.grant.swap(grant, SeqCst) < grant { self.release_edge(); }
    }
    fn available(&self, class: Class) -> u64 {
        let limit = self.limit();
        if class.floor_reserved() { return limit; }
        let floor = self.floor.load(SeqCst);
        let retiring = self.retiring.as_ref().map_or(0, |retiring| retiring.used()).min(floor);
        limit.min(self.grant().saturating_add(retiring)).saturating_sub(floor)
    }
    fn fits_after(&self, used: u64, class: Class, bytes: u64) -> bool {
        bytes == 0 || used.checked_add(bytes).is_some_and(|used| used <= self.available(class))
    }
    fn fits(&self, class: Class, bytes: u64) -> bool { self.fits_after(self.used(), class, bytes) }
    fn evictable(&self, class: Class) -> u64 { if class == Class::Refinement { 0 } else { self.yielding() } }
    fn fits_through(&self, class: Class, bytes: u64) -> bool {
        self.fits_after(self.used().saturating_sub(self.evictable(class)), class, bytes) && self.parent.as_ref().is_none_or(|parent| parent.fits_through(class, bytes))
    }
    pub(crate) fn charge(self: &Arc<Self>, bytes: u64) -> Charge {
        self.add(bytes);
        Charge { tier: self.clone(), bytes }
    }
    fn reserve(self: &Arc<Self>, bytes: u64, fits: impl Fn(u64) -> bool) -> Option<Charge> {
        self.used.try_update(SeqCst, SeqCst, |used| fits(used).then(|| used + bytes)).ok()?;
        if let Some(parent) = &self.parent { parent.add(bytes); }
        Some(Charge { tier: self.clone(), bytes })
    }
    pub(crate) fn charge_within(self: &Arc<Self>, limit: u64, bytes: u64) -> Option<Charge> {
        self.reserve(bytes, |used| used.checked_add(bytes).is_some_and(|used| used <= limit))
    }
    pub(crate) fn blocked(self: &Arc<Self>, class: Class, bytes: u64) -> Option<Blocked> {
        (!self.fits(class, bytes)).then(|| self.refusal(class, bytes, None))
    }
    fn reserve_admission(self: &Arc<Self>, class: Class, bytes: u64) -> Result<(), Blocked> {
        self.used.try_update(SeqCst, SeqCst, |used|self.fits_after(used,class,bytes).then(||used+bytes))
            .map_err(|_|self.refusal(class,bytes,None))?;
        if let Some(parent)=&self.parent && let Err(blocked)=parent.reserve_admission(class,bytes) {
            self.used.fetch_sub(bytes,SeqCst);self.release_edge();return Err(blocked);
        }
        Ok(())
    }
    pub(crate) fn refused_by(self: &Arc<Self>, ledger: layer_core::MemoryOwner, class: Class, bytes: u64) -> Blocked { self.refusal(class, bytes, Some(ledger)) }
    pub(crate) fn admit(self: &Arc<Self>, class: Class, bytes: u64) -> Result<Reservation, Blocked> {
        self.reserve_admission(class,bytes).inspect_err(|blocked| { blocked.tier.heap.evict(blocked.deficit(), class); })?;
        Ok(Reservation(Charge {tier:self.clone(),bytes},class))
    }
    fn release_edge(&self) {
        fence(SeqCst);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(edge) = self.edge.get() { edge.signal_release(); }
    }
}

pub(crate) struct Charge { tier: Arc<Tier>, bytes: u64 }
impl Charge {
    pub(crate) fn bytes(&self) -> u64 { self.bytes }
    pub(crate) fn tier(&self) -> &Arc<Tier> { &self.tier }
    pub(crate) fn resize(&mut self, bytes: u64) { if bytes != self.bytes { *self = self.tier.charge(bytes); } }
    pub(crate) fn merge(&mut self, mut charge: Charge) {
        assert!(Arc::ptr_eq(&self.tier,&charge.tier));
        self.bytes+=charge.bytes;charge.bytes=0;
    }
    pub(crate) fn split(&mut self, bytes: u64) -> Option<Charge> {
        self.bytes = self.bytes.checked_sub(bytes)?;
        Some(Charge { tier: self.tier.clone(), bytes })
    }
}
impl std::fmt::Debug for Charge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_tuple("Charge").field(&self.bytes).finish() }
}
impl Drop for Charge {
    fn drop(&mut self) { self.tier.release(self.bytes); }
}

pub(crate) struct Reservation(Charge, Class);
impl Reservation {
    pub(crate) fn bytes(&self)->u64 {self.0.bytes()}
    pub(crate) fn class(&self)->Class {self.1}
    pub(crate) fn split(&mut self, bytes:u64)->Option<Self> {self.0.split(bytes).map(|charge|Self(charge,self.1))}
    pub(crate) fn grow(&mut self, bytes:u64) {self.0.tier.add(bytes);self.0.bytes+=bytes;}
    pub(crate) fn charge(self, bytes: u64) -> Charge {
        let Self(mut charge, _) = self;
        if bytes < charge.bytes { charge.tier.release(charge.bytes - bytes); } else { charge.tier.add(bytes - charge.bytes); }
        charge.bytes = bytes;
        charge
    }
}

#[derive(Clone)]
pub(crate) struct Blocked { tier: Arc<Tier>, class: Class, bytes: u64, ahead: Arc<AtomicU64>, ledger: Option<layer_core::MemoryOwner> }
impl std::fmt::Debug for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blocked").field("class", &self.class).field("bytes", &self.bytes).field("ahead", &self.ahead.load(SeqCst)).field("used", &self.tier.used()).field("ledger", &self.ledger.is_some()).finish()
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl Blocked {
    pub(crate) fn wake_on_release(self, edge: &RetirementHandle, wake: Arc<dyn Fn() + Send + Sync>) { edge.wait_for_release(&self, wake); }
    pub(crate) fn bind(&self, edge: &RetirementHandle) -> RetirementHandle {
        let bound = self.tier.edge.get_or_init(|| edge.clone()).clone();
        let mut parent = self.tier.parent.as_ref();
        while let Some(tier) = parent { tier.edge.get_or_init(|| bound.clone()); parent = tier.parent.as_ref(); }
        fence(SeqCst);
        bound
    }
}
impl Blocked {
    pub(crate) fn class(&self)->Class {self.class}
    pub(crate) fn covering(self, bytes: u64) -> Self { Self { bytes: self.bytes.max(bytes), ..self } }
    pub(crate) fn alone(self) -> Self { Self { ahead: Default::default(), ..self } }
    pub(crate) fn fits(&self) -> bool {
        if let Some(ledger) = self.tier.root_ledger() { ledger.flush(); }
        let bytes = self.need();
        self.tier.fits_through(self.class, bytes)
            && self.ledger.as_ref().is_none_or(|ledger| ledger.fits_working_through(self.tier.root_used().saturating_sub(self.tier.root_evictable(self.class)).saturating_add(bytes)))
    }
    pub(crate) fn need(&self) -> u64 { self.bytes.saturating_add(self.ahead.load(SeqCst)) }
    pub(crate) fn same(&self, other: &Self) -> bool { Arc::ptr_eq(&self.ahead, &other.ahead) }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn same_wait(&self, other: &Self) -> bool { Arc::ptr_eq(&self.tier, &other.tier) && self.class == other.class && self.ledger.is_some() == other.ledger.is_some() }
    pub(crate) fn deficit(&self) -> u64 { self.tier.used().saturating_add(self.bytes).saturating_sub(self.tier.available(self.class)) }
}
#[cfg(target_arch = "wasm32")]
impl Blocked {
    pub(crate) fn overcommit(self) -> Reservation { Reservation(self.tier.charge(self.bytes), self.class) }
}

pub(crate) struct Retention { state: AtomicU32, bytes: u64, tier: Arc<Tier> }
impl Retention {
    const OPTIONAL: u32 = 1 << 30;
    const REMOVED: u32 = 1 << 31;
    fn yields(state: u32) -> bool { state == Self::OPTIONAL }
    fn update(&self, change: impl Fn(u32) -> u32) {
        let before = self.state.try_update(SeqCst, SeqCst, |state| Some(change(state))).unwrap();
        match (Self::yields(before), Self::yields(change(before))) {
            (false, true) => self.tier.offer(self.bytes),
            (true, false) => self.tier.withdraw(self.bytes),
            _ => {},
        }
    }
}
pub(crate) struct Retained(Arc<Retention>);
impl Retained {
    pub(crate) fn new(tier: &Arc<Tier>, bytes: u64) -> Self { Self(Arc::new(Retention { state: AtomicU32::new(0), bytes, tier: tier.clone() })) }
    pub(crate) fn set_optional(&self, optional: bool) {
        self.0.update(|state| if optional { state | Retention::OPTIONAL } else { state & !Retention::OPTIONAL });
    }
    pub(crate) fn pin(&self, charge: Charge) -> RetentionPin {
        self.0.update(|state| state + 1);
        RetentionPin { retention: self.0.clone(), _charge: charge }
    }
}
impl Drop for Retained {
    fn drop(&mut self) { self.0.update(|state| state | Retention::REMOVED); }
}
pub(crate) struct RetentionPin { retention: Arc<Retention>, _charge: Charge }
impl Drop for RetentionPin {
    fn drop(&mut self) { self.retention.update(|state| state - 1); }
}

#[cfg(test)]
#[path = "tier_tests.rs"]
mod tests;
