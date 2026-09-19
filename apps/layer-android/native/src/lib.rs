//! Android host boundary. The render Looper exclusively owns `App`; native UI
//! actions use Serde, while pointer history crosses JNI as packed numeric data.
#[cfg(any(target_os = "android", test))]
mod display;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod app;
#[cfg(target_os = "android")]
mod workspaces;

#[cfg(target_os = "android")]
mod documents;

#[cfg(target_os = "android")]
mod image_import;

#[cfg(target_os = "android")]
mod inspection;

#[cfg(target_os = "android")]
mod color_edit;
#[cfg(target_os = "android")]
mod source_edit;

#[cfg(target_os = "android")]
mod color_preferences;
#[cfg(target_os = "android")]
mod proof;

#[cfg(target_os = "android")]
mod hdr;
