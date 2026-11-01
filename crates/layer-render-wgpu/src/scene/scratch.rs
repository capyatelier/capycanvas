use std::sync::{Arc, Weak};

#[derive(Clone, Default)]
pub(super) enum Usage {
    #[default]
    Free,
    Reserved(Arc<()>),
    Leased(Weak<()>),
}

#[derive(Clone)]
pub(super) struct Lease {
    pub index: usize,
    _owner: Arc<()>,
}
impl Lease {
    pub fn into_owner(self) -> Arc<()> { self._owner }
}

impl Usage {
    pub fn occupied(&self) -> bool {
        match self {
            Self::Free => false,
            Self::Reserved(_) => true,
            Self::Leased(owner) => owner.strong_count() != 0,
        }
    }

    pub fn reset(&mut self) {
        if let Self::Reserved(owner) = self { *self = Self::Leased(Arc::downgrade(owner)); }
        if !self.occupied() { *self = Self::Free; }
    }

    pub fn lease(&mut self, index: usize) -> Lease {
        let owner = match self {
            Self::Reserved(owner) => owner.clone(),
            Self::Leased(owner) => owner.upgrade().expect("scratch reservation expired"),
            Self::Free => panic!("scratch page is not reserved"),
        };
        *self = Self::Leased(Arc::downgrade(&owner));
        Lease { index, _owner: owner }
    }

    pub fn reserve(&mut self) { *self = Self::Reserved(Arc::new(())); }

    pub fn keep(&mut self, lease: Lease) -> usize {
        let index = lease.index;
        *self = Self::Reserved(lease._owner);
        index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_release_on_unwind_and_keep_cloned_consumers_reserved() {
        let mut usage = Usage::default();
        usage.reserve();
        let lease = usage.lease(0);
        let consumer = lease.clone();
        usage.reset();
        assert!(usage.occupied());
        drop(lease);
        assert!(usage.occupied());
        drop(consumer);
        assert!(!usage.occupied());
        let result: Result<(), ()> = (|| {
            usage.reserve();
            let _lease = usage.lease(0);
            Err(())?;
            Ok(())
        })();
        assert!(result.is_err());
        assert!(!usage.occupied());
    }
}
