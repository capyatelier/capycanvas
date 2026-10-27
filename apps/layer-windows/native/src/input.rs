use serde::de::DeserializeOwned;
use std::ffi::{CStr, c_char};

/// # Safety
/// `raw` must be null or readable, unchanged NUL-terminated storage throughout the call.
///
/// ```compile_fail
/// mod input { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/input.rs")); }
/// let bytes = std::ffi::CString::new("borrowed").unwrap();
/// let escaped = unsafe { input::with_text(bytes.as_ptr(), |text| Ok(text)) };
/// ```
pub(crate) unsafe fn with_text<T>(raw: *const c_char, use_text: impl FnOnce(&str) -> Result<T, String>) -> Result<T, String> {
    if raw.is_null() { return Err("Null JSON".into()); }
    // SAFETY: The caller retains readable, unchanged storage throughout use_text.
    let text = unsafe { CStr::from_ptr(raw) }.to_str().map_err(|error| error.to_string())?;
    use_text(text)
}

/// # Safety
/// `raw` has the storage requirements of `with_text`.
///
/// ```compile_fail
/// mod input { include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/input.rs")); }
/// let bytes = std::ffi::CString::new("\"borrowed\"").unwrap();
/// let escaped: &str = unsafe { input::read_json(bytes.as_ptr()) }.unwrap();
/// ```
pub(crate) unsafe fn read_json<T: DeserializeOwned>(raw: *const c_char) -> Result<T, String> {
    // SAFETY: The caller retains the input storage through owned deserialization.
    unsafe { with_text(raw, |text| serde_json::from_str(text).map_err(|error| error.to_string())) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn json_owns_its_strings_after_input_is_released() {
        let bytes = CString::new("[\"paint\",\"色\"]").unwrap();
        // SAFETY: bytes owns initialized NUL-terminated storage through this call.
        let values: Vec<String> = unsafe { read_json(bytes.as_ptr()) }.unwrap();
        drop(bytes);
        assert_eq!(values, ["paint", "色"]);
    }

    #[test]
    fn text_validation_preserves_input_and_transport_errors() {
        // SAFETY: Null is explicitly accepted and rejected before dereference.
        assert_eq!(unsafe { read_json::<serde_json::Value>(std::ptr::null()) }.unwrap_err(), "Null JSON");
        for bytes in [CString::new(vec![0xff]).unwrap(), CString::new("[").unwrap()] {
            // SAFETY: bytes remains live, including its terminating NUL.
            assert!(unsafe { read_json::<serde_json::Value>(bytes.as_ptr()) }.is_err());
        }
        let bytes = CString::new("folder/色").unwrap();
        // SAFETY: bytes remains live through the callback; only an owned length escapes.
        assert_eq!(unsafe { with_text(bytes.as_ptr(), |text| Ok(text.len())) }.unwrap(), "folder/色".len());
        // SAFETY: bytes remains live through the callback.
        assert_eq!(unsafe { with_text::<()>(bytes.as_ptr(), |_| Err("transport".into())) }.unwrap_err(), "transport");
    }

    #[test]
    fn touch_contact_json_preserves_full_unsigned_fields() {
        for timestamp in [0, 1_101_152_828_577_000, (1_u64 << 53) - 1, (1_u64 << 53) + 1, u64::MAX] {
            let surface = (1_u64 << 53) + 1;
            let bytes = CString::new(format!(r#"{{"type":"color_touch","phase":"down","position":[1.5,2.5],"surface":{surface},"id":4294967295,"time_ns":{timestamp}}}"#)).unwrap();
            let input: layer_ui::UiInput = unsafe { read_json(bytes.as_ptr()) }.unwrap();
            let layer_ui::UiInput::ColorTouch { surface: actual_surface, id, time_ns, .. } = input else { panic!("Wrong contact type"); };
            assert_eq!((actual_surface, id, time_ns), (surface, u64::from(u32::MAX), timestamp));
            let bytes = CString::new(format!(r#"{{"type":"layer_touch","phase":"up","position":[1.5,2.5],"surface":{surface},"id":7,"row":{surface},"time_ns":{timestamp}}}"#)).unwrap();
            let input: layer_ui::UiInput = unsafe { read_json(bytes.as_ptr()) }.unwrap();
            let layer_ui::UiInput::LayerTouch { row, time_ns, .. } = input else { panic!("Wrong contact type"); };
            assert_eq!((row, time_ns), (Some(surface), timestamp));
        }
        let malformed = CString::new(r#"{"type":"color_touch","phase":"down","position":[1.5,2.5],"surface":7,"id":1,"time_ns":1101152828577000.0}"#).unwrap();
        assert!(unsafe { read_json::<layer_ui::UiInput>(malformed.as_ptr()) }.is_err());
        let next = CString::new(r#"{"type":"blur"}"#).unwrap();
        assert!(matches!(unsafe { read_json::<layer_ui::UiInput>(next.as_ptr()) }.unwrap(), layer_ui::UiInput::Blur));
    }
}
