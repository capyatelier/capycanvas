//! Android host boundary. The render Looper exclusively owns `App`; native UI
//! actions use Serde, while pointer history crosses JNI as packed numeric data.
#[cfg(any(target_os = "android", test))]
macro_rules! jni_entry {
    ($env:ident, $handle:ident, $owner:ident, $body:block) => {{
        let result=crate::android::with_app($handle,|$owner| {Ok($body)});
        match result {Ok(value)=>value,Err(cause)=>crate::android::refuse(&mut $env,cause)}
    }};
    ($env:ident, $body:block) => {{
        crate::jni_entry!(result => {
            match result {Ok(value)=>value,Err(cause)=>crate::android::refuse(&mut $env,cause)}
        }; { Ok($body) })
    }};
    ($result:ident => $finish:block; $body:block) => {{
        let $result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<_,String> { $body }))
            .unwrap_or_else(|payload|Err(layer_host::panic_diagnostic(&*payload)));
        $finish
    }};
}
#[cfg(any(target_os = "android", test))]
pub(crate) use jni_entry;
#[cfg(any(target_os = "android", test))]
mod display;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
mod app;
#[cfg(any(target_os = "android", test))]
mod launch;
#[cfg(any(target_os = "android", test))]
mod document_close;
#[cfg(target_os = "android")]
mod workspaces;

#[cfg(target_os = "android")]
mod documents;
#[cfg(target_os = "android")]
mod session_recovery;
#[cfg(target_os = "android")]
mod document_tabs;

#[cfg(target_os = "android")]
mod image_import;
#[cfg(target_os = "android")]
mod clipboard;

#[cfg(target_os = "android")]
mod inspection;
#[cfg(target_os = "android")]
mod scopes;

#[cfg(target_os = "android")]
mod color_edit;
#[cfg(target_os = "android")]
mod source_edit;
#[cfg(target_os = "android")]
mod image_objects;

#[cfg(target_os = "android")]
mod color_preferences;
#[cfg(target_os = "android")]
mod proof;
#[cfg(target_os = "android")]
mod papers;
#[cfg(target_os = "android")]
mod photo_reader;

#[cfg(target_os = "android")]
mod hdr;

#[cfg(test)]
mod launch_tests;

#[cfg(target_os = "android")]
mod port_controls;
