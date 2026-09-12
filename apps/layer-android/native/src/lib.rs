//! Android host boundary. The render Looper exclusively owns `App`; native UI
//! actions use Serde, while pointer history crosses JNI as packed numeric data.
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod app;
#[cfg(target_os = "android")]
mod workspaces;

#[cfg(target_os = "android")]
mod documents;
