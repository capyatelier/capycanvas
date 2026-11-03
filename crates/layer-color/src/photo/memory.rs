//! Temporary source, decode and encode working-set limits for photo operations.

/// Concrete operation allowance supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotoMemoryBudget {
    pub source_bytes: usize,
    pub decode_bytes: usize,
    pub encode_bytes: usize,
}

impl PhotoMemoryBudget {
    pub fn facts() -> layer_core::MemoryFacts { layer_core::memory_budget::system_facts() }
    pub fn conversion_encode_limit() -> usize {
        Self::from_limit(layer_core::MemoryBudget::process().import_headroom_bytes()).encode_bytes
    }
    pub fn from_limit(bytes: u64) -> Self {
        let bytes = bytes.min(isize::MAX as u64) as usize;
        Self {
            source_bytes: bytes / 8,
            decode_bytes: bytes / 3,
            encode_bytes: bytes / 2,
        }
    }

    /// Current process/system headroom when the host can measure it.
    pub fn available_memory() -> Option<u64> {
        available_memory()
    }
}

fn available_memory() -> Option<u64> {
    remaining(PhotoMemoryBudget::facts())
}

fn remaining(facts: layer_core::MemoryFacts) -> Option<u64> {
    let process = facts.process_limit_bytes.zip(facts.process_used_bytes)
        .map(|(limit, used)| limit.saturating_sub(used));
    match (facts.available_bytes, process) {
        (Some(available), Some(process)) => Some(available.min(process)),
        (available, process) => available.or(process),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_constraints_preserve_unknown_and_actual_exhaustion() {
        let facts = |available, limit, used| layer_core::MemoryFacts {
            available_bytes: available, process_limit_bytes: limit, process_used_bytes: used, ..Default::default()
        };
        assert_eq!(remaining(facts(Some(3000), None, None)), Some(3000));
        assert_eq!(remaining(facts(Some(3000), Some(4000), Some(2200))), Some(1800));
        assert_eq!(remaining(facts(Some(3000), Some(4000), Some(4000))), Some(0));
        assert_eq!(remaining(facts(None, Some(4000), Some(2200))), Some(1800));
        assert_eq!(remaining(facts(None, None, None)), None);
        assert_eq!(remaining(facts(Some(0), None, None)), Some(0));
    }
    #[test]
    fn operation_limits_scale_with_the_caller_allowance_without_a_minimum() {
        for available in [0, 1, 1024, 128 << 20, 512 << 20, 4 << 30, 8 << 30, u64::MAX] {
            let b = PhotoMemoryBudget::from_limit(available);
            let usable = available.min(isize::MAX as u64) as usize;
            assert_eq!(b.source_bytes, usable / 8);
            assert_eq!(b.decode_bytes, usable / 3);
            assert_eq!(b.encode_bytes, usable / 2);
            assert!(b.source_bytes + b.decode_bytes <= usable / 2);
        }
    }
}
