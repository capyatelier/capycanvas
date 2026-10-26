use super::CapyApple;
use layer_host::handles::{self,Handle,LocalHandles,LocalLease,LocalBorrowed,Borrowed};
use std::{ffi::CString,sync::Arc};

thread_local! {static APPS:LocalHandles=Default::default();}

pub(crate) fn store_app(value:CapyApple)->Result<u64,String> {
    APPS.with(|apps|apps.insert(LocalLease::new(value))).map(Handle::raw).map_err(|error|error.to_string())
}
pub(crate) fn app(raw:u64)->Option<LocalBorrowed<CapyApple>> {
    let handle=Handle::from_raw(raw).ok()?;
    APPS.with(|apps|apps.with::<LocalLease<CapyApple>,_>(handle,|lease|lease.clone())).ok()?.borrow().ok()
}
pub(crate) fn free_app(raw:u64) {
    let _=APPS.try_with(|apps|Handle::from_raw(raw).and_then(|handle|apps.take::<LocalLease<CapyApple>>(handle)));
}
pub(crate) fn store<T:Send+Sync+'static>(value:T)->u64 {
    handles::insert_shared(Arc::new(value)).map_or(0,Handle::raw)
}
pub(crate) fn shared<T:Send+Sync+'static>(raw:u64)->Option<Arc<T>> {
    Handle::from_raw(raw).and_then(handles::shared::<T>).ok()
}
pub(crate) fn mutable<T:Send+'static>(value:T)->u64 {handles::insert(value).map_or(0,Handle::raw)}
pub(crate) fn borrow<T:Send+'static>(raw:u64)->Option<Borrowed<T>> {
    Handle::from_raw(raw).and_then(handles::lease::<T>).and_then(|lease|lease.borrow()).ok()
}
pub(crate) fn free<T:'static>(raw:u64) {let _=Handle::from_raw(raw).and_then(handles::release::<T>);}
pub(crate) fn take<T:Send+'static>(raw:u64)->Option<T> {Handle::from_raw(raw).and_then(handles::take::<T>).ok()}
pub(crate) fn text(value:CString)->u64 {layer_host::ffi::text(value).map_or(0,Handle::raw)}

#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_string_length(raw:u64)->usize {
    Handle::from_raw(raw).and_then(layer_host::ffi::text_length).unwrap_or(0)
}
/// # Safety
/// Nonempty output is writable and exclusive for length bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_string_copy(raw:u64,offset:usize,output:*mut u8,length:usize)->bool {
    Handle::from_raw(raw).and_then(|handle|unsafe {layer_host::ffi::copy_text(handle,offset,output,length)}).unwrap_or(false)
}
#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_string_free(raw:u64) {let _=Handle::from_raw(raw).and_then(layer_host::ffi::free_text);}

#[cfg(test)]
pub(crate) fn take_text(raw:u64)->CString {
    let mut bytes=vec![0;capy_apple_string_length(raw)];
    for offset in (0..bytes.len()).step_by(layer_core::package::MAX_RANGE_BYTES) {
        let count=(bytes.len()-offset).min(layer_core::package::MAX_RANGE_BYTES);
        assert!(unsafe {capy_apple_string_copy(raw,offset,bytes.as_mut_ptr().add(offset),count)});
    }
    assert!(unsafe {capy_apple_string_copy(raw,bytes.len(),std::ptr::null_mut(),0)});
    capy_apple_string_free(raw);CString::new(bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn native_handles_reject_wrong_families_and_initialize_failed_outputs() {
        let bytes=store(vec![17u8,29,43]);
        let text=text(CString::new("日本語🎨").unwrap());
        for raw in [0,1,u64::MAX,bytes,text] {
            assert_eq!(unsafe {capy_session_failures(raw)},0);
            let mut preview=CapyProjectPreview {width:91,height:91,count:91};
            assert_eq!(unsafe {capy_project_preview_at(raw,0,&mut preview)},-1);
            assert_eq!((preview.width,preview.height,preview.count),(0,0,0));
            let mut filter=CapyFilterPreviewInfo {request:91,width:91,height:91,stride:91,count:91,filters:91};
            assert!(!unsafe {capy_filter_previews_read(raw,&mut filter)});
            assert_eq!((filter.request,filter.width,filter.height,filter.stride,filter.count,filter.filters),(0,0,0,0,0,0));
            let mut scopes=CapyScopeInfo {plots:91,count:91};
            assert!(!unsafe {capy_scopes_read(raw,&mut scopes)});
            assert_eq!((scopes.plots,scopes.count),(0,0));
            let mut count=91;
            assert_eq!(unsafe {capy_paper_previews_read(raw,0,&mut count)},0);assert_eq!(count,0);
            count=91;assert_eq!(unsafe {capy_project_clip_png(raw,&mut count)},-1);assert_eq!(count,0);
            count=91;assert_eq!(unsafe {capy_project_proof_preservation(raw,&mut count)},-1);assert_eq!(count,0);
            unsafe {
                capy_apple_destroy(raw);capy_project_free(raw);capy_document_free(raw);
                capy_session_free(raw);capy_session_destination_free(raw);capy_language_free(raw);
                capy_paper_free(raw);capy_paper_previews_free(raw);capy_filter_previews_free(raw);capy_scopes_free(raw);
            }
        }
        assert_eq!(capy_bytes_length(bytes),3);
        assert_eq!(take_text(text).to_str().unwrap(),"日本語🎨");
        let mut output=[91;3];
        assert!(!unsafe {capy_bytes_copy(bytes,usize::MAX,output.as_mut_ptr(),1)});
        assert_eq!(output,[91;3]);
        assert!(unsafe {capy_bytes_copy(bytes,0,output.as_mut_ptr(),3)});
        assert_eq!(output,[17,29,43]);capy_bytes_free(bytes);capy_bytes_free(bytes);
        assert!(!unsafe {capy_bytes_copy(bytes,0,output.as_mut_ptr(),0)});
    }

    #[test]
    fn editor_handles_reject_reentry_wrong_thread_and_retired_generations() {
        let raw=capy_apple_create(1);assert_ne!(raw,0);
        let active=app(raw).unwrap();assert!(app(raw).is_none());
        std::thread::spawn(move || {
            assert!(app(raw).is_none());unsafe {capy_apple_destroy(raw)};
        }).join().unwrap();
        drop(active);assert!(app(raw).is_some());
        let active=app(raw).unwrap();let epoch=active.host.session.state().document_file.epoch;unsafe {capy_apple_destroy(raw)};
        assert!(app(raw).is_none());assert_eq!(active.host.session.state().document_file.epoch,epoch);
        drop(active);unsafe {capy_apple_destroy(raw)};
        let replacement=capy_apple_create(1);assert_ne!(replacement,0);assert_ne!(raw,replacement);
        assert!(app(raw).is_none());unsafe {capy_apple_destroy(replacement)};
    }

    #[test]
    fn destination_observations_transfer_the_fingerprint_once() {
        use std::io::{Seek,Write};
        use std::os::fd::AsRawFd;
        let path=std::env::temp_dir().join(format!("apple-destination-{}",layer_core::PortableId::random()));
        let mut file=std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        file.write_all(b"retained source").unwrap();file.rewind().unwrap();
        let raw=unsafe {capy_session_destination_read(file.as_raw_fd())};assert_ne!(raw,0);
        assert!(take::<CapySessionDestination>(raw).is_some());
        assert!(take::<CapySessionDestination>(raw).is_none());
        unsafe {capy_session_destination_free(raw)};drop(file);std::fs::remove_file(path).unwrap();
    }
}
