//! Allocation-free Android trace scopes for shared renderer attribution. No
//! scheduling or GPU waits; other hosts compile these probes away. Optional
//! GPU phase readbacks are managed separately by telemetry.
pub(crate) struct Span;
pub(crate) fn enabled() -> bool {
    #[cfg(target_os = "android")]
    unsafe {
        ATrace_isEnabled()
    }
    #[cfg(not(target_os = "android"))]
    {
        false
    }
}
impl Span {
    pub fn new(name: &'static std::ffi::CStr) -> Self {
        #[cfg(target_os = "android")]
        unsafe {
            ATrace_beginSection(name.as_ptr());
        }
        #[cfg(not(target_os = "android"))]
        let _ = name;
        Self
    }
    pub fn next(&mut self, name: &'static std::ffi::CStr) {
        #[cfg(target_os = "android")]
        unsafe {
            ATrace_endSection();
            ATrace_beginSection(name.as_ptr());
        }
        #[cfg(not(target_os = "android"))]
        let _ = name;
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        #[cfg(target_os = "android")]
        unsafe {
            ATrace_endSection();
        }
    }
}
pub(crate) fn counter(name: &'static std::ffi::CStr, value: u64) {
    #[cfg(target_os = "android")]
    unsafe {
        ATrace_setCounter(name.as_ptr(), value as i64);
    }
    #[cfg(not(target_os = "android"))]
    let _ = (name, value);
}
#[cfg(target_os = "android")]
#[link(name = "android")]
unsafe extern "C" {
    fn ATrace_isEnabled() -> bool;
    fn ATrace_beginSection(name: *const std::ffi::c_char);
    fn ATrace_endSection();
    fn ATrace_setCounter(name: *const std::ffi::c_char, value: i64);
}
