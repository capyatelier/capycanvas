use layer_host::{ModelUpdates, handles::{self, Handle}};
use serde_json::value::RawValue;
use std::{collections::BTreeMap, ffi::{CString, c_char}};

#[unsafe(no_mangle)]
pub extern "C" fn capy_model_updates_new()->u64 {
    handles::insert(ModelUpdates::default()).map_or(0,Handle::raw)
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_model_updates_free(updates:u64) {
    let _=Handle::from_raw(updates).and_then(handles::release::<ModelUpdates>);
}

/// # Safety
/// Input is readable, unchanged NUL-terminated UTF-8 model JSON during this worker call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_model_updates_publish(updates:u64,input:*const c_char)->u64 {
    std::panic::catch_unwind(||unsafe {crate::input::with_text(input,|text| {
        let lease=Handle::from_raw(updates).and_then(handles::lease::<ModelUpdates>).map_err(|error|error.to_string())?;
        let mut encoder=lease.borrow().map_err(|error|error.to_string())?;
        let mut fields:BTreeMap<&str,&RawValue>=serde_json::from_str(text).map_err(|error|error.to_string())?;
        let localization=fields.remove("localization");
        let bytes=if localization.is_some(){serde_json::to_vec(&fields).map_err(|error|error.to_string())?}else{text.as_bytes().to_vec()};
        let bytes=encoder.publish(bytes).map_err(|error|error.to_string())?;
        let bytes=match localization {
            Some(value)=>layer_host::extend_update(bytes,&serde_json::to_vec(&BTreeMap::from([("localization",value)])).map_err(|error|error.to_string())?),
            None=>bytes,
        };
        CString::new(bytes).map(crate::owned::text).map_err(|error|error.to_string())
    })}).ok().and_then(Result::ok).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value,json};
    fn publish(encoder:u64,value:Value)->Value {
        let input=CString::new(value.to_string()).unwrap();
        serde_json::from_str(crate::owned::take_text(unsafe {capy_model_updates_publish(encoder,input.as_ptr())}).to_str().unwrap()).unwrap()
    }
    #[test]
    fn worker_encoder_retains_fields_across_threads_and_separates_localization() {
        let encoder=capy_model_updates_new();assert_ne!(encoder,0);
        let first=publish(encoder,json!({"state":{"revision":1},"windows_workspace":{"busy":false},"localization":{"generation":1}}));
        assert_eq!(first["state"]["revision"],1);
        let next=std::thread::spawn(move||publish(encoder,json!({"state":{"revision":1},"windows_workspace":{"busy":true}}))).join().unwrap();
        assert_eq!(next,json!({"model_update":[[["windows_workspace","busy"],true]]}));
        let translated=publish(encoder,json!({"state":{"revision":1},"windows_workspace":{"busy":true},"localization":{"generation":2}}));
        assert_eq!(translated,json!({"model_update":[],"localization":{"generation":2}}));
        let stable=publish(encoder,json!({"state":{"revision":1},"windows_workspace":{"busy":true}}));
        assert_eq!(stable,json!({"model_update":[]}));
        capy_model_updates_free(encoder);
        let input=CString::new("{}").unwrap();
        assert_eq!(unsafe {capy_model_updates_publish(encoder,input.as_ptr())},0);
    }
    #[test]
    fn invalid_model_does_not_consume_the_worker_baseline() {
        let encoder=capy_model_updates_new();
        publish(encoder,json!({"state":{"revision":1}}));
        for input in [std::ptr::null(),c"[".as_ptr()] {assert_eq!(unsafe {capy_model_updates_publish(encoder,input)},0);}
        assert_eq!(publish(encoder,json!({"state":{"revision":2}})),json!({"model_update":[[["state","revision"],2]]}));
        capy_model_updates_free(encoder);
    }
}
