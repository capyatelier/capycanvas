//! Android host boundary. The render Looper exclusively owns `App`; native UI
//! actions use Serde, while pointer history crosses JNI as packed numeric data.
#[cfg(target_os = "android")]
mod android;
#[cfg(any(target_os = "android", test))]
mod app;
#[cfg(any(target_os = "android", test))]
mod renderer;
