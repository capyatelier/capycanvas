use super::Class;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Demand(Option<(u64, Class)>);
impl Demand {
    pub(crate) fn stamp(&mut self, epoch: u64, class: Class) {
        self.0 = Some((epoch, self.0.filter(|(stamped, _)| *stamped == epoch).map_or(class, |(_, demanded)| demanded.min(class))));
    }
    pub(crate) fn protects(self, epoch: u64, class: Class) -> bool {
        self.0.is_some_and(|(demanded_at, demanded)| demanded_at.saturating_add(1) >= epoch && demanded <= class)
    }
    pub(crate) fn current(self, epoch: u64, class: Class) -> bool { self.class_at(epoch).is_some_and(|demanded| demanded <= class) }
    pub(crate) fn class_at(self, epoch: u64) -> Option<Class> { self.0.filter(|(demanded_at, _)| *demanded_at == epoch).map(|(_, demanded)| demanded) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_stamp_protects_its_class_and_lower_priority_requesters_for_one_further_epoch() {
        let mut demand = Demand::default();
        assert!(!demand.protects(0, Class::Refinement));
        demand.stamp(4, Class::Prefetch);
        demand.stamp(4, Class::Visible);
        assert!(demand.protects(5, Class::Visible) && demand.protects(5, Class::Prefetch));
        assert!(!demand.protects(5, Class::Authored), "a more important requester may take it");
        assert!(!demand.protects(6, Class::Refinement), "demand expires after the following epoch");
        demand.stamp(6, Class::Prefetch);
        assert!(!demand.protects(6, Class::Fallback), "a new epoch restarts from the new class");
    }
    #[test]
    fn current_demand_protects_only_within_its_own_epoch() {
        let mut demand = Demand::default();
        demand.stamp(4, Class::Visible);
        assert!(demand.current(4, Class::Fallback) && !demand.current(4, Class::Authored));
        assert!(!demand.current(5, Class::Refinement), "an earlier epoch's demand protects nothing");
    }
}
