//! Windows application adapter. After UI-thread surface initialization, the
//! canvas thread exclusively owns this object; UI callbacks only enqueue work.
#![deny(unsafe_op_in_unsafe_fn)]
#![recursion_limit = "256"]
#[cfg(any(target_os = "windows", test))]
mod actions;
#[cfg(any(target_os = "windows", test))]
mod document_io;
#[cfg(any(target_os = "windows", test))]
mod documents;
#[cfg(any(target_os = "windows", test))]
mod document_workflows;
#[cfg(any(target_os = "windows", test))]
mod proof;
#[cfg(any(target_os = "windows", test))]
mod tone;
#[cfg(target_os = "windows")]
mod display;
#[cfg(any(target_os = "windows", test))]
mod color_storage;
#[cfg(any(target_os = "windows", test))]
mod recovery;
mod events;
#[cfg(any(target_os = "windows", test))]
mod filter_packages;
#[cfg(any(target_os = "windows", test))]
mod navigator;
#[cfg(any(target_os = "windows", test))]
mod previews;
#[cfg(any(target_os = "windows", test))]
mod workspace;
#[cfg(any(target_os = "windows", test))]
mod workspace_async;
#[cfg(any(target_os = "windows", test))]
mod workspace_service;
#[cfg(any(target_os = "windows", test))]
pub use navigator::{capy_navigator_aspect, capy_navigator_image};
#[cfg(any(target_os = "windows", test))]
mod settings;
pub use events::CapyPointer;
#[cfg(target_os = "windows")]
mod device;
#[cfg(target_os = "windows")]
mod host;
mod glass;
mod palette_files;
#[cfg(target_os = "windows")]
pub use host::*;

mod shared_controls;
pub use shared_controls::*;
mod color;
pub use color::*;

#[cfg(all(test, target_os = "windows"))]
mod gpu_recovery_tests;
