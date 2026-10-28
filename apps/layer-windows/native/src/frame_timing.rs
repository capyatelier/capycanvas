use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct FrameTiming {
    pub pacing_ns: u64,
    pub acquisition_ns: u64,
    pub prepared_ns: u64,
    pub composed_ns: u64,
    pub submitted_ns: u64,
    pub presented_ns: u64,
}

pub(crate) fn now() -> u64 {
    static FREQUENCY: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut value = 0;
        unsafe { QueryPerformanceFrequency(&mut value) }.expect("Windows performance frequency");
        value as u64
    });
    let mut value = 0;
    unsafe { QueryPerformanceCounter(&mut value) }.expect("Windows performance counter");
    let ticks = value as u64;
    ticks / frequency * 1_000_000_000 + ticks % frequency * 1_000_000_000 / frequency
}
