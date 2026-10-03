use super::*;
use layer_core::color::{ColorProfile, RgbSpace};

#[test]
fn shared_icc_library_bridge_keeps_bytes_summaries_and_invalid_entry_errors() {
    let call = |action: Value, bytes: &[u8]| {
        fixture_localization();
        let action = CString::new(json!({"language":"en","request":action}).to_string()).unwrap();
        let text = unsafe { capy_profile_library(action.as_ptr(), bytes.as_ptr(), bytes.len()) };
        assert!(!text.is_null());
        let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
        unsafe { capy_apple_string_free(text) };
        value
    };
    let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    let imported = call(json!({"type":"import","entries":[]}), &bytes);
    assert_eq!(imported["profile"]["Icc"], json!(bytes));
    let id = imported["id"].as_str().unwrap();
    let record = json!({"id":id,"bytes":bytes.len()});
    let summary = call(json!({"type":"inspect","entry":record}), &bytes);
    assert_eq!(summary["channels"], "Rgb"); assert!(summary["profile"].is_null());
    assert_eq!(call(json!({"type":"get","id":id}), &bytes), imported);
    assert!(call(json!({"type":"get","id":id}), b"Invalid profile")["error"].is_string());
    assert!(call(json!({"type":"inspect","entry":record}), b"Invalid profile")["issue"].is_string());
    let limit = call(json!({"type":"limits"}), &[])["read_bytes"].as_u64().unwrap() as usize;
    assert!(call(json!({"type":"inspect","entry":record}), &vec![0;limit+1])["issue"].is_string());
    assert!(call(json!({"type":"remove","id":"../Original.icc"}), &[])["error"].is_string());
}
