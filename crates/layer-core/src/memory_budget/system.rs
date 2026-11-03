#[cfg(target_arch = "wasm32")]
pub fn system_facts() -> super::MemoryFacts { Default::default() }

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn system_facts() -> super::MemoryFacts {
    let info = std::fs::read_to_string("/proc/meminfo").ok();
    let mut facts = super::MemoryFacts {
        total_bytes: info.as_deref().and_then(|s| proc_bytes(s, "MemTotal:")),
        available_bytes: info.as_deref().and_then(|s| proc_bytes(s, "MemAvailable:")),
        tablet: cfg!(target_os = "android"), ..Default::default()
    };
    let constraint = std::fs::read_to_string("/proc/self/cgroup").ok()
        .zip(std::fs::read_to_string("/proc/self/mountinfo").ok())
        .ok_or(()).and_then(|(member, mounts)| cgroup_constraint(&member, &mounts, |path| std::fs::read_to_string(path)));
    match constraint {
        Ok(Some((limit, used, pressure))) => { facts.process_limit_bytes = Some(limit); facts.process_used_bytes = Some(used); facts.pressure = pressure; }
        Ok(None) => {},
        Err(()) => facts.available_bytes = None,
    }
    facts
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_bytes(source: &str, key: &str) -> Option<u64> {
    let mut fields = source.lines().find_map(|line| line.strip_prefix(key))?.split_whitespace();
    let number = fields.next()?.parse::<u64>().ok()?;
    (fields.next()? == "kB" && fields.next().is_none()).then(|| number.checked_mul(1024)).flatten()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn mount_path(value: &str) -> Option<std::path::PathBuf> {
    let mut source = value.bytes();
    let mut bytes = Vec::with_capacity(value.len());
    while let Some(byte) = source.next() {
        bytes.push(if byte == b'\\' {
            let digits = [source.next()?, source.next()?, source.next()?];
            if !digits.iter().all(|n| (b'0'..=b'7').contains(n)) { return None; }
            let number = u16::from(digits[0] - b'0') * 64 + u16::from(digits[1] - b'0') * 8 + u16::from(digits[2] - b'0');
            u8::try_from(number).ok()?
        } else { byte });
    }
    String::from_utf8(bytes).ok().map(std::path::PathBuf::from)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn cgroup_constraint(member: &str, mounts: &str, mut read: impl FnMut(&std::path::Path) -> std::io::Result<String>) -> Result<Option<(u64, u64, bool)>, ()> {
    use std::path::{Component, Path};
    let Some(member) = member.lines().find_map(|line| line.strip_prefix("0::")) else { return Ok(None); };
    let member = Path::new(member);
    if !member.is_absolute() || member.components().any(|c| matches!(c, Component::ParentDir)) { return Err(()); }
    let (root, mount) = mounts.lines().filter_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        let split = fields.iter().position(|field| *field == "-")?;
        if fields.get(split + 1)? != &"cgroup2" { return None; }
        let root = mount_path(fields.get(3)?)?;
        let mount = mount_path(fields.get(4)?)?;
        member.starts_with(&root).then_some((root, mount))
    }).max_by_key(|(root, _)| root.components().count()).ok_or(())?;
    let mut current = mount.join(member.strip_prefix(root).map_err(|_| ())?);
    let mut tightest: Option<(u64, u64)> = None;
    let mut pressure = false;
    loop {
        let mut limit: Option<u64> = None;
        let mut high = None;
        for name in ["memory.max", "memory.high"] {
            match read(&current.join(name)) {
                Ok(value) if value.trim() == "max" => {},
                Ok(value) => {
                    let bytes = value.trim().parse::<u64>().map_err(|_| ())?;
                    if name == "memory.high" { high = Some(bytes); }
                    limit = Some(limit.map_or(bytes, |previous| previous.min(bytes)));
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(_) => return Err(()),
            }
        }
        if let Some(limit) = limit {
            let used = read(&current.join("memory.current")).map_err(|_| ())?.trim().parse::<u64>().map_err(|_| ())?;
            pressure |= high.is_some_and(|high| used >= high);
            if tightest.is_none_or(|(old_limit, old_used)| limit.saturating_sub(used) < old_limit.saturating_sub(old_used)) {
                tightest = Some((limit, used));
            }
        }
        if current == mount { break; }
        if !current.pop() || !current.starts_with(&mount) { return Err(()); }
    }
    Ok(tightest.map(|(limit, used)| (limit, used, pressure)))
}

#[cfg(target_os = "windows")]
pub fn system_facts() -> super::MemoryFacts {
    use windows::Win32::System::{SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        ProcessStatus::{K32GetPerformanceInfo, K32GetProcessMemoryInfo, PERFORMANCE_INFORMATION, PROCESS_MEMORY_COUNTERS_EX},
        Threading::GetCurrentProcess, JobObjects::{IsProcessInJob, QueryInformationJobObject,
            JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOBOBJECTINFOCLASS,
            JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_PROCESS_MEMORY}};
    #[repr(C)]
    #[derive(Default)]
    struct JobMemoryUsage { current: u64, peak: u64 }
    const JOB_OBJECT_MEMORY_USAGE_INFORMATION: JOBOBJECTINFOCLASS = JOBOBJECTINFOCLASS(28);
    let mut facts = super::MemoryFacts::default();
    let mut memory = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    if unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok() {
        facts.total_bytes = Some(memory.ullTotalPhys);
        facts.available_bytes = Some(memory.ullAvailPhys);
    }
    let mut performance = PERFORMANCE_INFORMATION { cb: std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    if unsafe { K32GetPerformanceInfo(&mut performance, performance.cb) }.as_bool()
        && let Some((limit, used)) = (performance.CommitLimit as u64).checked_mul(performance.PageSize as u64)
            .zip((performance.CommitTotal as u64).checked_mul(performance.PageSize as u64)) {
        constrain(&mut facts, limit, used);
    }
    let mut counters = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    let used = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut counters).cast(), counters.cb) }.as_bool()
        .then_some(counters.PrivateUsage as u64);
    let mut in_job = windows::core::BOOL::default();
    if unsafe { IsProcessInJob(GetCurrentProcess(), None, &mut in_job) }.is_ok() && in_job.as_bool() {
        let mut job = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if unsafe { QueryInformationJobObject(None, JobObjectExtendedLimitInformation, (&raw mut job).cast(),
            std::mem::size_of_val(&job) as u32, None) }.is_ok() {
            if job.BasicLimitInformation.LimitFlags.contains(JOB_OBJECT_LIMIT_PROCESS_MEMORY)
                && let Some(used) = used { constrain(&mut facts, job.ProcessMemoryLimit as u64, used); }
            if job.BasicLimitInformation.LimitFlags.contains(JOB_OBJECT_LIMIT_JOB_MEMORY) {
                let mut usage = JobMemoryUsage::default();
                let current = unsafe { QueryInformationJobObject(None, JOB_OBJECT_MEMORY_USAGE_INFORMATION,
                    (&raw mut usage).cast(), std::mem::size_of_val(&usage) as u32, None) }.is_ok()
                    .then_some(usage.current);
                constrain_job(&mut facts, job.JobMemoryLimit as u64, current, used);
            }
        }
    }
    facts
}

#[cfg(any(target_os = "windows", test))]
fn constrain_job(facts: &mut super::MemoryFacts, limit: u64, current: Option<u64>, private: Option<u64>) {
    if let Some(used) = current.or(private) { constrain(facts, limit, used); }
}

#[cfg(any(target_os = "windows", test))]
fn constrain(facts: &mut super::MemoryFacts, limit: u64, used: u64) {
    if facts.process_limit_bytes.zip(facts.process_used_bytes)
        .is_none_or(|(old_limit, old_used)| limit.saturating_sub(used) < old_limit.saturating_sub(old_used)) {
        facts.process_limit_bytes = Some(limit);
        facts.process_used_bytes = Some(used);
    }
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "ios", not(target_env = "sim")))]
pub fn system_facts() -> super::MemoryFacts {
    unsafe extern "C" {
        fn os_proc_available_memory() -> usize;
        fn sysctlbyname(name: *const std::ffi::c_char, old: *mut std::ffi::c_void, len: *mut usize,
            new: *mut std::ffi::c_void, new_len: usize) -> std::ffi::c_int;
    }
    let mut total = 0u64;
    let mut len = std::mem::size_of_val(&total);
    let valid = unsafe { sysctlbyname(c"hw.memsize".as_ptr(), (&mut total as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    super::MemoryFacts { total_bytes: (valid && len == std::mem::size_of_val(&total) && total > 0).then_some(total),
        available_bytes: Some(unsafe { os_proc_available_memory() } as u64), tablet: true, ..Default::default() }
}

#[cfg(all(not(target_arch = "wasm32"), not(any(target_os = "linux", target_os = "android", target_os = "windows")), any(not(target_os = "ios"), target_env = "sim")))]
pub fn system_facts() -> super::MemoryFacts {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if system.total_memory() == 0 { return Default::default(); }
    #[cfg(any(target_os = "macos", all(target_os = "ios", target_env = "sim")))]
    let available = system.total_memory().saturating_sub(system.used_memory());
    #[cfg(not(any(target_os = "macos", all(target_os = "ios", target_env = "sim"))))]
    let available = system.available_memory();
    super::MemoryFacts { total_bytes: Some(system.total_memory()), available_bytes: Some(available),
        tablet: cfg!(target_os = "ios"), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_allowance_recovers_with_current_usage_and_falls_back_to_private_usage() {
        let sample = |current, private| {
            let mut facts = super::super::MemoryFacts::default();
            constrain_job(&mut facts, 2000, current, private);
            facts.process_limit_bytes.zip(facts.process_used_bytes).map(|(limit, used)| limit.saturating_sub(used))
        };
        assert_eq!(sample(Some(1800), Some(100)), Some(200));
        assert_eq!(sample(Some(400), Some(100)), Some(1600));
        assert_eq!(sample(None, Some(300)), Some(1700));
        assert_eq!(sample(Some(0), Some(300)), Some(2000));
        assert_eq!(sample(None, None), None);
    }

    #[test]
    fn tightest_commit_or_job_constraint_keeps_its_matching_usage() {
        let mut facts = super::super::MemoryFacts::default();
        constrain(&mut facts, 10000, 1000);
        constrain(&mut facts, 4000, 3500);
        constrain(&mut facts, 1000, 100);
        assert_eq!((facts.process_limit_bytes, facts.process_used_bytes), (Some(4000), Some(3500)));
        constrain(&mut facts, 200, 300);
        assert_eq!((facts.process_limit_bytes, facts.process_used_bytes), (Some(200), Some(300)));
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn proc_memory_accepts_only_checked_kernel_kilobytes() {
        let info = "MemTotal: 1234 kB\nMemAvailable: 50 kB\nVmSize: 9999 kB\n";
        assert_eq!(proc_bytes(info, "MemTotal:"), Some(1234 * 1024));
        assert_eq!(proc_bytes(info, "MemAvailable:"), Some(50 * 1024));
        assert_eq!(proc_bytes("MemAvailable: 0 kB", "MemAvailable:"), Some(0));
        assert_eq!(proc_bytes("MemAvailable: 2 MB", "MemAvailable:"), None);
        assert_eq!(proc_bytes("MemAvailable: 2 kB extra", "MemAvailable:"), None);
        assert_eq!(proc_bytes("MemAvailable: 18446744073709551615 kB", "MemAvailable:"), None);
        assert_eq!(proc_bytes(info, "Missing:"), None);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn files(entries: &[(&str, &str)]) -> impl FnMut(&std::path::Path) -> std::io::Result<String> + use<> {
        let files: std::collections::BTreeMap<_, _> = entries.iter()
            .map(|(path, value)| (std::path::PathBuf::from(path), value.to_string())).collect();
        move |path| files.get(path).cloned().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn cgroup_membership_and_ancestors_bound_the_remaining_allowance() {
        let mount = "29 1 0:2 / /cg rw - cgroup2 cgroup rw";
        let read = files(&[("/cg/team/app/memory.max", "900"), ("/cg/team/app/memory.current", "200"),
            ("/cg/team/memory.max", "2000"), ("/cg/team/memory.high", "1000"), ("/cg/team/memory.current", "900")]);
        assert_eq!(cgroup_constraint("0::/team/app\n", mount, read), Ok(Some((1000, 900, false))));
        let read = files(&[("/cg/team/memory.high", "800"), ("/cg/team/memory.current", "900")]);
        assert_eq!(cgroup_constraint("0::/team/app\n", mount, read), Ok(Some((800, 900, true))));
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn cgroup_mount_root_and_escaped_paths_are_resolved_without_leaving_the_mount() {
        let mount = "29 1 0:2 /tenant /cg\\040pool rw shared:7 - cgroup2 cgroup rw";
        let read = files(&[("/cg pool/child/memory.max", "200"), ("/cg pool/child/memory.current", "10"),
            ("/cg pool/memory.max", "500"), ("/cg pool/memory.current", "400")]);
        assert_eq!(cgroup_constraint("0::/tenant/child", mount, read), Ok(Some((500, 400, false))));
        assert_eq!(mount_path("/cg\\999pool"), None);
        assert_eq!(cgroup_constraint("0::/tenant/../outside", mount, files(&[])), Err(()));
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn cgroup_unlimited_missing_and_failed_constraints_stay_distinct() {
        let mount = "29 1 0:2 / /cg rw - cgroup2 cgroup rw";
        assert_eq!(cgroup_constraint("0::/app", mount, files(&[("/cg/app/memory.max", "max")])), Ok(None));
        assert_eq!(cgroup_constraint("1:cpu:/app", mount, files(&[])), Ok(None));
        assert_eq!(cgroup_constraint("0::/app", "", files(&[])), Err(()));
        assert_eq!(cgroup_constraint("0::/app", mount, files(&[("/cg/app/memory.max", "100")])), Err(()));
        assert_eq!(cgroup_constraint("0::/app", mount, |_| Err(std::io::ErrorKind::PermissionDenied.into())), Err(()));
    }
}
