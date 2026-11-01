use layer_render::{MemoryPeak, MemorySnapshot, ProcessMemory, ProcessMemoryKind};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub(super) struct MemorySampler {
    snapshot: Arc<Mutex<Option<MemorySnapshot>>>,
    _lifetime: mpsc::Sender<()>,
}
impl MemorySampler {
    pub fn new(device: wgpu::Device) -> Self {
        let mut peak = 0;
        Self::with_query(move || {
            let mut process = process_memory();
            if let Some(memory) = &mut process {
                peak = peak.max(memory.bytes);
                if matches!(memory.peak, MemoryPeak::Sampled(_)) { memory.peak = MemoryPeak::Sampled(peak); }
            }
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
            let gpu = crate::display_memory::usage(&device);
            #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
            let gpu = { let _ = &device; None };
            MemorySnapshot { sampled_at: Instant::now(), process, gpu }
        })
    }
    fn with_query(mut query: impl FnMut() -> MemorySnapshot + Send + 'static) -> Self {
        let snapshot = Arc::new(Mutex::new(None));
        let publish = Arc::downgrade(&snapshot);
        let (lifetime, stopped) = mpsc::channel();
        let _ = std::thread::Builder::new().name("memory samples".into()).spawn(move || loop {
            let sample = query();
            let Some(snapshot) = publish.upgrade() else { break; };
            *snapshot.lock().unwrap() = Some(sample);
            drop(snapshot);
            match stopped.recv_timeout(Duration::from_secs(2)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {},
                _ => break,
            }
        });
        Self { snapshot, _lifetime: lifetime }
    }
    pub fn snapshot(&self) -> Option<MemorySnapshot> { self.snapshot.try_lock().ok().and_then(|snapshot| *snapshot) }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_memory() -> Option<ProcessMemory> {
    status_memory(&std::fs::read_to_string("/proc/self/status").ok()?)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn status_memory(status: &str) -> Option<ProcessMemory> {
    let bytes = |key| {
        let mut fields = status.lines().find_map(|line| line.strip_prefix(key))?.split_whitespace();
        let value = fields.next()?.parse::<u64>().ok()?;
        if fields.next()? != "kB" || fields.next().is_some() { return None; }
        value.checked_mul(1024)
    };
    Some(ProcessMemory { kind: ProcessMemoryKind::Resident, bytes: bytes("VmRSS:")?, peak: MemoryPeak::Lifetime(bytes("VmHWM:")?) })
}

#[cfg(target_os = "windows")]
fn process_memory() -> Option<ProcessMemory> {
    use windows::Win32::System::{ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX}, Threading::GetCurrentProcess};
    let mut memory = PROCESS_MEMORY_COUNTERS_EX::default();
    memory.cb = std::mem::size_of_val(&memory) as u32;
    let valid = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), (&raw mut memory).cast(), memory.cb) }.as_bool();
    valid.then_some(ProcessMemory { kind: ProcessMemoryKind::PrivateCommit, bytes: memory.PrivateUsage as u64,
        peak: MemoryPeak::Lifetime(memory.PeakPagefileUsage as u64) })
}

#[cfg(target_vendor = "apple")]
fn process_memory() -> Option<ProcessMemory> {
    #[repr(C)]
    #[derive(Default)]
    struct TaskVmInfoRev1 { virtual_bytes: u64, regions: i32, page_bytes: i32, accounting: [u64; 16], footprint_bytes: u64 }
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    let mut info = TaskVmInfoRev1::default();
    let mut count = (std::mem::size_of_val(&info) / std::mem::size_of::<i32>()) as u32;
    let expected = count;
    let result = unsafe { task_info(mach_task_self_, 22, (&raw mut info).cast(), &mut count) };
    (result == 0 && count >= expected).then_some(ProcessMemory { kind: ProcessMemoryKind::PhysicalFootprint,
        bytes: info.footprint_bytes, peak: MemoryPeak::Sampled(info.footprint_bytes) })
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
fn process_memory() -> Option<ProcessMemory> { None }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn resident_memory_keeps_rss_and_os_peak_separate_from_virtual_bytes() {
        let status = "Name:\tcapy\nVmSize:\t999999 kB\nVmRSS:\t1234 kB\nVmHWM:\t5678 kB\n";
        assert_eq!(status_memory(status), Some(ProcessMemory { kind: ProcessMemoryKind::Resident,
            bytes: 1234 * 1024, peak: MemoryPeak::Lifetime(5678 * 1024) }));
        assert_eq!(status_memory("VmRSS: 2 MB\nVmHWM: 3 kB"), None);
        assert_eq!(status_memory("VmSize: 1 kB\nVmHWM: 2 kB"), None);
        assert_eq!(status_memory("VmRSS: 18446744073709551615 kB\nVmHWM: 2 kB"), None);
        assert!(process_memory().is_some_and(|memory| memory.bytes > 0));
    }

    #[test]
    fn memory_readers_never_wait_for_a_blocked_query_or_publisher() {
        let (started, running) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let sampler = MemorySampler::with_query(move || {
            started.send(()).unwrap();
            held.recv().unwrap();
            MemorySnapshot { sampled_at: Instant::now(), process: None, gpu: None }
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(sampler.snapshot().is_none());
        let guard = sampler.snapshot.lock().unwrap();
        assert!(sampler.snapshot().is_none());
        drop(guard);
        release.send(()).unwrap();
    }

    #[test]
    fn retired_sampler_cannot_publish_into_a_replacement() {
        struct Completed(mpsc::Sender<()>);
        impl Drop for Completed { fn drop(&mut self) { let _ = self.0.send(()); } }
        let (finished, done) = mpsc::channel();
        let (started, running) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let completed = Completed(finished);
        let retired = MemorySampler::with_query(move || {
            let _ = &completed;
            started.send(()).unwrap();
            held.recv().unwrap();
            MemorySnapshot { sampled_at: Instant::now(), process: Some(ProcessMemory {
                kind: ProcessMemoryKind::Resident, bytes: 999, peak: MemoryPeak::Lifetime(999) }), gpu: None }
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        let old_state = Arc::downgrade(&retired.snapshot);
        drop(retired);
        let replacement = MemorySampler { snapshot: Arc::new(Mutex::new(None)), _lifetime: mpsc::channel().0 };
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(old_state.upgrade().is_none());
        assert!(replacement.snapshot().is_none());
    }
}
