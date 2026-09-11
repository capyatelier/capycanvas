//! Windows application adapter. After UI-thread surface initialization, the
//! canvas thread exclusively owns this object; UI callbacks only enqueue work.
#![deny(unsafe_op_in_unsafe_fn)]
mod events;
#[cfg(any(target_os = "windows", test))]
mod settings;
pub use events::CapyPointer;
#[cfg(target_os = "windows")]
mod host;
#[cfg(target_os = "windows")]
pub use host::*;
