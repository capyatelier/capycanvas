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
    fn android_cached_apps_leave_room_for_a_twelve_megapixel_jpeg() {
        use layer_core::memory_budget::{AndroidMemory, android_facts};
        let total = 8_070_660 * 1024;
        let budget = layer_core::MemoryBudget::with_facts(android_facts(
            layer_core::MemoryFacts { total_bytes: Some(total), available_bytes: Some(1_187_912 * 1024), tablet: true, ..Default::default() },
            Some(AndroidMemory { total_bytes: total, available_bytes: (142_964 + 1_166_128) * 1024, threshold_bytes: 216 << 20,
                low_memory: false, trim_levels: Vec::new() })));
        let session = budget.register_owner(layer_core::MemoryOwnerKind::Authored, layer_core::MemoryClass::Pinned, None);
        session.report(layer_core::MemoryReport { ram_bytes: 308 << 20, pinned_ram_bytes: 308 << 20, ..Default::default() });
        let limits = crate::photo::DecodeLimits::from_memory_budget(PhotoMemoryBudget::from_limit(budget.import_headroom_bytes()));
        assert!(limits.codec_bytes >= 72_182_016, "estimated decode memory 72182016 exceeds limit {}", limits.codec_bytes);
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
