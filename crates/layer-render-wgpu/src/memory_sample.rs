use layer_render::{MemoryBudgetSummary, MemoryPeak, MemorySnapshot, ProcessMemory, ProcessMemoryKind};
use layer_core::{MemoryBudget, MemoryDevice, MemoryFacts, MemoryOwner, DeviceMemoryFacts};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use web_time::Instant;

pub(super) struct MemorySampler {
    snapshot: Arc<Mutex<Option<Sample>>>,
    updates: Option<mpsc::SyncSender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for MemorySampler {
    fn drop(&mut self) {
        drop(self.updates.take());
        if let Some(worker)=self.worker.take() { let _=worker.join(); }
    }
}
#[derive(Clone)]
struct Sample { memory: MemorySnapshot, budget: Arc<MemoryBudgetSummary> }
#[cfg(test)]
pub(super) fn fixture_sampler(budget: MemoryBudget, id: MemoryDevice) -> MemorySampler {
    let refresh = budget.clone();
    MemorySampler::with_updates(move || {
        budget.flush_reports();
        let snapshot = budget.snapshot();
        Sample { memory: MemorySnapshot { sampled_at: Instant::now(), process: None, gpu: None },
            budget: Arc::new(MemoryBudgetSummary::new(&snapshot, id)) }
    }, |sample| sample, move |sample| {
        refresh.flush_reports();
        let snapshot = refresh.snapshot();
        sample.budget = Arc::new(MemoryBudgetSummary::new(&snapshot, id));
    })
}
impl MemorySampler {
    pub fn new(device: wgpu::Device, budget: MemoryBudget, id: MemoryDevice, allocator: MemoryOwner) -> Self {
        let mut peak = 0;
        let refresh = budget.clone();
        Self::with_updates(move || {
            let mut process = process_memory();
            if let Some(memory) = &mut process {
                peak = peak.max(memory.bytes);
                if matches!(memory.peak, MemoryPeak::Sampled(_)) { memory.peak = MemoryPeak::Sampled(peak); }
            }
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
            let gpu = crate::display_memory::usage(&device);
            #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
            let gpu = { let _ = &device; None };
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple"))]
            let facts = crate::display_memory::facts(&device, gpu);
            #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
            let facts = DeviceMemoryFacts::default();
            let capacity = device.generate_allocator_report().map(|report| report.total_reserved_bytes);
            (MemorySnapshot { sampled_at: Instant::now(), process, gpu },
                layer_core::memory_budget::system_facts(), facts, capacity)
        }, move |reading| publish(&budget, id, &allocator, reading), move |sample| {
            refresh.flush_reports();
            let snapshot=refresh.snapshot();
            sample.budget=Arc::new(MemoryBudgetSummary::new(&snapshot, id));
        })
    }
    fn with_updates<T: Send + 'static>(mut query: impl FnMut() -> T + Send + 'static,
        mut finish: impl FnMut(T) -> Sample + Send + 'static, mut refresh: impl FnMut(&mut Sample) + Send + 'static) -> Self {
        let snapshot = Arc::new(Mutex::new(None));
        let publish = Arc::downgrade(&snapshot);
        let (updates, received) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new().name("memory samples".into()).spawn(move || {
            let mut sample=None;
            let mut next=Instant::now();
            loop {
                if publish.strong_count()==0 {break;}
                let reading=(sample.is_none() || Instant::now()>=next).then(&mut query);
                let Some(snapshot)=publish.upgrade() else {break;};
                if let Some(reading)=reading {sample=Some(finish(reading));next=Instant::now()+Duration::from_secs(1);}
                else {refresh(sample.as_mut().unwrap());}
                *snapshot.lock().unwrap()=sample.clone();drop(snapshot);
                match received.recv_timeout(next.saturating_duration_since(Instant::now())) {
                    Ok(())|Err(mpsc::RecvTimeoutError::Timeout)=>{},
                    Err(mpsc::RecvTimeoutError::Disconnected)=>break,
                }
            }
        }).ok();
        Self { snapshot, updates: Some(updates), worker }
    }
    pub fn request_update(&self) { if let Some(updates)=&self.updates { let _=updates.try_send(()); } }
    pub fn snapshot(&self) -> Option<MemorySnapshot> { self.snapshot.try_lock().ok().and_then(|snapshot| snapshot.as_ref().map(|sample| sample.memory)) }
    pub fn budget_snapshot(&self) -> Option<Arc<MemoryBudgetSummary>> {
        self.snapshot.try_lock().ok().and_then(|snapshot| snapshot.as_ref().map(|sample| sample.budget.clone()))
    }
}

type Reading = (MemorySnapshot, MemoryFacts, DeviceMemoryFacts, Option<u64>);
fn publish(budget: &MemoryBudget, id: MemoryDevice, allocator: &MemoryOwner, (memory, ram, gpu, capacity): Reading) -> Sample {
    budget.flush_reports();
    budget.update_ram(ram, MemoryBudget::now_ms());
    if let Some(capacity) = capacity {
        let charged = budget.snapshot().devices.into_iter().find(|device| device.device == id).map_or(0, |device| device.physical_bytes);
        let owners = charged.saturating_sub(allocator.reported().gpu_bytes);
        allocator.report(layer_core::MemoryReport { gpu_bytes: capacity.saturating_sub(owners), ..Default::default() });
    }
    budget.update_device(id, gpu);
    let snapshot = budget.snapshot();
    Sample { memory, budget: Arc::new(MemoryBudgetSummary::new(&snapshot, id)) }
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

    fn sample(memory: MemorySnapshot) -> Sample {
        let budget = MemoryBudget::new();
        let device = budget.register_device(DeviceMemoryFacts::default());
        Sample { memory, budget: Arc::new(MemoryBudgetSummary::new(&budget.snapshot(), device)) }
    }

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
        let sampler = MemorySampler::with_updates(move || {
            started.send(()).unwrap();
            held.recv().unwrap();
            MemorySnapshot { sampled_at: Instant::now(), process: None, gpu: None }
        }, sample, |_| {});
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(sampler.snapshot().is_none());
        let guard = sampler.snapshot.lock().unwrap();
        assert!(sampler.snapshot().is_none());
        assert!(sampler.budget_snapshot().is_none());
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
        let retired = MemorySampler::with_updates(move || {
            let _ = &completed;
            started.send(()).unwrap();
            held.recv().unwrap();
            MemorySnapshot { sampled_at: Instant::now(), process: Some(ProcessMemory {
                kind: ProcessMemoryKind::Resident, bytes: 999, peak: MemoryPeak::Lifetime(999) }), gpu: None }
        }, sample, |_| {});
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        let old_state = Arc::downgrade(&retired.snapshot);
        let retired=std::thread::spawn(move || drop(retired));
        let replacement = MemorySampler { snapshot: Arc::new(Mutex::new(None)), updates: Some(mpsc::sync_channel(1).0), worker: None };
        release.send(()).unwrap();
        retired.join().unwrap();
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(old_state.upgrade().is_none());
        assert!(replacement.snapshot().is_none());
    }

    #[test]
    fn dropping_sampler_joins_worker_and_releases_query_captures() {
        let owner=Arc::new(());
        let captured=owner.clone();
        let (queried,ready)=mpsc::channel();
        let sampler=MemorySampler::with_updates(move || {
            let _=&captured;
            let _=queried.send(());
            MemorySnapshot {sampled_at:Instant::now(),process:None,gpu:None}
        },sample,|_| {});
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(Arc::strong_count(&owner),2);
        drop(sampler);
        assert_eq!(Arc::strong_count(&owner),1);
    }

    #[test]
    fn queued_budget_updates_coalesce_without_polling_os_facts_again() {
        use std::sync::atomic::{AtomicUsize,Ordering};
        let queries=Arc::new(AtomicUsize::new(0));let count=queries.clone();
        let budget=MemoryBudget::new();let owner=budget.register_owner(layer_core::MemoryOwnerKind::Scene,layer_core::MemoryClass::Visible,None);
        let device=budget.register_device(DeviceMemoryFacts::default());
        let (started,running)=mpsc::channel();let (release,held)=mpsc::channel();let (updated,done)=mpsc::channel();
        let refresh=budget.clone();
        let sampler=MemorySampler::with_updates(move||{
            let number=count.fetch_add(1,Ordering::Relaxed);if number==0 {started.send(()).unwrap();held.recv().unwrap();}
            MemorySnapshot {sampled_at:Instant::now(),process:None,gpu:None}
        },sample,move|sample|{
            refresh.flush_reports();let snapshot=refresh.snapshot();
            sample.budget=Arc::new(MemoryBudgetSummary::new(&snapshot,device));
            updated.send((snapshot.ram_physical_bytes,sample.memory.sampled_at)).unwrap();
        });
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(owner.publish(layer_core::MemoryReport {ram_bytes:1234,demand_ram_bytes:1234,..Default::default()}));
        for _ in 0..1024 {sampler.request_update();}
        assert!(sampler.snapshot().is_none());release.send(()).unwrap();
        let (bytes,sampled_at)=done.recv_timeout(Duration::from_secs(5)).unwrap();assert_eq!(bytes,1234);
        assert_eq!(queries.load(Ordering::Relaxed),1);
        assert!(done.try_recv().is_err());
        sampler.request_update();let (_,second_sampled_at)=done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(second_sampled_at,sampled_at);assert_eq!(queries.load(Ordering::Relaxed),1);
        drop(sampler);
    }

    #[test]
    fn budget_facts_update_without_a_stats_reader() {
        let budget = MemoryBudget::new();
        let id = budget.register_device(Default::default());
        let observed = budget.clone();
        let (published, done) = mpsc::channel();
        let allocator = budget.register_owner(layer_core::MemoryOwnerKind::Other, layer_core::MemoryClass::Pinned, Some(id));
        let sampler = MemorySampler::with_updates(|| (
            MemorySnapshot { sampled_at: Instant::now(), process: None, gpu: None },
            MemoryFacts { total_bytes: Some(8 << 30), available_bytes: Some(6 << 30), ..Default::default() },
            DeviceMemoryFacts { budget_bytes: Some(1 << 30), ..Default::default() },
            None,
        ), move |reading| {
            let result = publish(&budget, id, &allocator, reading);
            published.send(()).unwrap();
            result
        }, |_| {});
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        let snapshot = observed.snapshot();
        assert!(snapshot.ram_limit_bytes > 0);
        assert!(snapshot.devices.iter().any(|device| device.device == id && device.limit_bytes > 0));
        drop(sampler);
    }

    #[test]
    fn evicting_into_allocator_slack_neither_lowers_the_device_limit_nor_frees_room() {
        use layer_core::{MemoryClass, MemoryOwnerKind, MemoryReport};
        const MIB: u64 = 1 << 20;
        for reported in [true, false] {
            let facts = |capacity: u64| DeviceMemoryFacts { budget_bytes: reported.then_some(4096 * MIB), heap_bytes: Some(8192 * MIB),
                usage_bytes: reported.then_some(capacity + 193 * MIB), ..Default::default() };
            let budget = MemoryBudget::new();
            let id = budget.register_device(facts(0));
            let scene = budget.register_owner(MemoryOwnerKind::Scene, MemoryClass::Visible, Some(id));
            let allocator = budget.register_owner(MemoryOwnerKind::Other, MemoryClass::Pinned, Some(id));
            let reading = |capacity| (MemorySnapshot { sampled_at: Instant::now(), process: None, gpu: None }, MemoryFacts::default(), facts(capacity), Some(capacity));
            let limit = || budget.snapshot().devices[0].limit_bytes;
            scene.report(MemoryReport { gpu_bytes: 1536 * MIB, demand_gpu_bytes: 8192 * MIB, ..Default::default() });
            publish(&budget, id, &allocator, reading(1536 * MIB));
            let before = limit();
            scene.report(MemoryReport { gpu_bytes: 768 * MIB, demand_gpu_bytes: 8192 * MIB, ..Default::default() });
            publish(&budget, id, &allocator, reading(1536 * MIB));
            assert_eq!(limit(), before, "reported usage: {reported}");
            assert_eq!(allocator.reported().gpu_bytes, 768 * MIB, "reported usage: {reported}");
            let room = limit() - 1536 * MIB;
            assert!(scene.grant().gpu_bytes <= 768 * MIB + room, "reported usage: {reported}: {} > {}", scene.grant().gpu_bytes >> 20, (768 * MIB + room) >> 20);
            assert!(scene.grant().gpu_bytes >= 768 * MIB, "reported usage: {reported}");
            scene.report(MemoryReport { gpu_bytes: 1536 * MIB, demand_gpu_bytes: 8192 * MIB, ..Default::default() });
            publish(&budget, id, &allocator, reading(1536 * MIB));
            assert_eq!((limit(), allocator.reported().gpu_bytes), (before, 0), "reported usage: {reported}");
        }
    }
}
