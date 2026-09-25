//! Stroke recordings leave the owner as one raw copy; compression and file
//! writes run on a worker.
use super::*;
use std::io::Write;
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;

/// # Safety
/// Valid handle and writable length. Returns an owned raw capture released
/// with `capy_bytes_free`, or null with the error set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_stroke_recording_data(app: *mut CapyApple, length: *mut usize) -> *mut u8 {
    let Some(app) = (unsafe { app.as_mut() }) else { return std::ptr::null_mut() };
    let Some(bytes) = app.perform(|a| a.host.session.stroke_recording().snapshot().map_err(|e| e.to_string())) else {
        return std::ptr::null_mut();
    };
    let bytes = bytes.into_boxed_slice();
    if let Some(length) = unsafe { length.as_mut() } { *length = bytes.len(); }
    Box::into_raw(bytes) as *mut u8
}

/// # Safety
/// Bytes must be an unreleased result of `capy_apple_stroke_recording_data`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_bytes_free(bytes: *mut u8, length: usize) {
    if !bytes.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes, length)) });
    }
}

/// # Safety
/// Worker only. Borrows count readable bytes and a descriptor open for writing.
/// Returns null on success or owned error JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_stroke_recording_write(bytes: *const u8, count: usize, output: i32) -> *mut c_char {
    let result = (|| -> Result<(), String> {
        let raw = if count == 0 { &[][..] } else {
            if bytes.is_null() { return Err("Recording bytes are unavailable".into()); }
            unsafe { std::slice::from_raw_parts(bytes, count) }
        };
        let compressed = layer_engine::recording::compress(raw).map_err(|e| e.to_string())?;
        let mut file = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(output) });
        file.write_all(&compressed).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => std::ptr::null_mut(),
        Err(error) => CString::new(serde_json::json!({"error": error}).to_string()).unwrap().into_raw(),
    }
}
