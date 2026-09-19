//! ABI types shared with the C interface without requiring libc on WebAssembly.
#![allow(non_camel_case_types)]
pub type intptr_t = isize;
pub type uintptr_t = usize;
pub type ptrdiff_t = isize;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use libc::{off_t, ENOENT, EIO, EAGAIN, ENOMEM, EINVAL, ERANGE, ENOPROTOOPT};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use wasm::*;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod wasm {
    // No system errno on wasm-unknown; retain conventional Linux error numbers
    // for the exported dav1d-compatible result values. These are not syscalls.
    pub type off_t = i64;
    pub const ENOENT: u8 = 2;
    pub const EIO: u8 = 5;
    pub const EAGAIN: u8 = 11;
    pub const ENOMEM: u8 = 12;
    pub const EINVAL: u8 = 22;
    pub const ERANGE: u8 = 34;
    pub const ENOPROTOOPT: u8 = 92;
}
