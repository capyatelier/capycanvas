//! Admission budgets from remaining memory, shared by every platform.
//! These are snapshots, not reservations or whole-editor memory guarantees.

/// Host-supplied memory must be bytes still available to this process, after
/// existing documents, GPU allocations and host-specific limits are accounted
/// for. Browsers without a reliable query use the same conservative fallback as
/// any native host where the query fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotoMemoryBudget {
    pub source_bytes: usize,
    pub decode_bytes: usize,
    pub encode_bytes: usize,
}

impl PhotoMemoryBudget {
    pub fn from_available_memory(bytes: u64) -> Self {
        // A single Rust allocation may not exceed isize::MAX. This also keeps
        // 32-bit/WASM arithmetic within its address space even on a large host.
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

    pub fn current() -> Self {
        Self::from_available_memory(available_memory().unwrap_or(512 * 1024 * 1024))
    }
}

#[cfg(target_arch = "wasm32")]
fn available_memory() -> Option<u64> {
    // Device RAM and JS heap limits are not available WASM process memory.
    // A host with a trustworthy budget supplies it per operation instead.
    None
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "ios"))]
fn available_memory() -> Option<u64> {
    unsafe extern "C" {
        fn os_proc_available_memory() -> usize;
    }
    // iOS 13+: remaining app allowance, including its memory-pressure limit.
    Some(unsafe { os_proc_available_memory() } as u64)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
fn available_memory() -> Option<u64> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if system.total_memory() == 0 {
        return None;
    }
    // sysinfo 0.37's Apple available-memory calculation subtracts compressed
    // pages from free/inactive pages, even though they are already excluded.
    // Use its total/used readings to count compression once. Do not invent a
    // minimum: true exhaustion must still produce a zero admission budget.
    #[cfg(target_os = "macos")]
    let headroom = system.total_memory().saturating_sub(system.used_memory());
    #[cfg(not(target_os = "macos"))]
    let headroom = system.available_memory();
    let available = process_allowance(
        headroom,
        system.total_memory(),
        system
            .cgroup_limits()
            .map(|c| (c.total_memory, c.free_memory)),
    );
    // sysinfo reports an unlimited root cgroup as physical total minus current
    // usage, including reclaimable cache. That is not a process limit; applying
    // it would incorrectly replace Linux/Android MemAvailable with free pages.
    Some(available)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
fn process_allowance(available: u64, total: u64, cgroup: Option<(u64, u64)>) -> u64 {
    match cgroup {
        Some((limit, free)) if limit < total => available.min(free),
        _ => available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(all(not(target_arch = "wasm32"), not(target_os = "ios")))]
    fn unlimited_cgroup_does_not_discard_reclaimable_headroom() {
        assert_eq!(process_allowance(3000, 12000, Some((12000, 1800))), 3000);
        assert_eq!(process_allowance(3000, 12000, Some((4000, 1800))), 1800);
        assert_eq!(process_allowance(3000, 12000, Some((4000, 0))), 0);
        assert_eq!(process_allowance(0, 12000, None), 0);
    }
    #[test]
    fn budgets_scale_with_headroom_and_never_invent_a_minimum() {
        for available in [0, 1, 1024, 128 << 20, 512 << 20, 4 << 30, 8 << 30, u64::MAX] {
            let b = PhotoMemoryBudget::from_available_memory(available);
            let usable = available.min(isize::MAX as u64) as usize;
            assert_eq!(b.source_bytes, usable / 8);
            assert_eq!(b.decode_bytes, usable / 3);
            assert_eq!(b.encode_bytes, usable / 2);
            assert!(b.source_bytes + b.decode_bytes <= usable / 2);
        }
    }
}
