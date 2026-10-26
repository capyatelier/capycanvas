use super::*;
use layer_host::papers::PaperTask;
use layer_ui::paper_library::{PaperLibraryError, PaperResult};
use std::{fs::File, mem::ManuallyDrop, os::fd::FromRawFd, path::PathBuf,
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};

pub struct CapyPaperTask {
    task: PaperTask,
    cancelled: AtomicBool,
    result: Mutex<Option<Result<PaperResult, String>>>,
}
impl CapyPaperTask {
    fn new(task: PaperTask) -> Self {
        Self { task, cancelled: AtomicBool::new(false), result: Mutex::new(None) }
    }
    fn capture(app: &CapyApple, request: u32, expected_epoch: u64, directory: PathBuf) -> Result<Self, String> {
        let session = &app.host.session;
        if request != 0 && expected_epoch != session.state().document_file.epoch {
            return Err(layer_ui::DocumentDeliveryCopy::new(session.localization()).change_in_progress.to_string());
        }
        Ok(Self::new(PaperTask::capture(session, (request != 0).then_some(request), directory)?))
    }
    fn work(&self, fd: i32, name: &str) -> Result<PaperResult, String> {
        let file = (fd >= 0).then(|| ManuallyDrop::new(unsafe { File::from_raw_fd(fd) }));
        self.task.work(file.as_deref().map(|file| (file, name)), &self.cancelled, layer_host::photo_reader::local)
    }
    fn apply(&self, app: &mut CapyApple) -> Result<(), String> {
        let result = self.result.lock().unwrap_or_else(|e| e.into_inner()).take()
            .ok_or_else(|| PaperLibraryError::Unsupported.message(app.host.session.localization()))?;
        self.task.apply(&mut app.host, result, self.cancelled.load(Ordering::Relaxed))
    }
}

/// # Safety
/// app is exclusive on its serial owner; directory is readable NUL-terminated UTF-8. Return/free one independent job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_paper_task(app: u64, request: u32, epoch: u64, directory: *const c_char) -> u64 {
    let Some(mut lease_app)=handles::app(app) else { return 0 };
    let app=&mut *lease_app;
    app.perform(|app| CapyPaperTask::capture(app, request, epoch, PathBuf::from(unsafe { project::read_title(directory) }?))
        .map(handles::store)).unwrap_or(0)
}
/// # Safety
/// app is exclusive on its serial owner; directory is readable NUL-terminated UTF-8. Return/free one independent job.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_paper_import(app: u64, directory: *const c_char) -> u64 {
    let Some(mut lease_app)=handles::app(app) else { return 0 };
    let app=&mut *lease_app;
    app.perform(|app| Ok(handles::store(CapyPaperTask::new(PaperTask::library_import(&app.host.session, PathBuf::from(unsafe { project::read_title(directory) }?))))))
        .unwrap_or(0)
}
/// # Safety
/// task is live on one file worker; fd and NUL-terminated name remain readable through this call. Do not run twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_work(task: u64, fd: i32, name: *const c_char) {
    let Some(lease_task)=handles::shared::<CapyPaperTask>(task) else { return };
    let task=&*lease_task;
    let result = catch_unwind(AssertUnwindSafe(|| task.work(fd, &unsafe { project::read_title(name) }?)))
        .unwrap_or_else(|error| Err(layer_host::panic_diagnostic(&*error)));
    *task.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
}
/// # Safety
/// app is exclusive on its serial owner; task work finished. Adopt a job at most once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_paper_apply(app: u64, task: u64) -> i32 {
    let (Some(mut lease_app),Some(lease_task))=(handles::app(app),handles::shared::<CapyPaperTask>(task)) else { return -1 };
    let (app,task)=(&mut *lease_app,&*lease_task);
    app.perform(|app| task.apply(app).map(|_| 0)).unwrap_or(-1)
}
/// # Safety
/// task remains live throughout this thread-safe cancellation call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_cancel(task: u64) {
    if let Some(lease_task)=handles::shared::<CapyPaperTask>(task) {let task=&*lease_task; task.cancelled.store(true, Ordering::Relaxed); }
}
/// # Safety
/// task is live on its file worker; message is readable NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_failed(task: u64, message: *const c_char) {
    if let Some(lease_task)=handles::shared::<CapyPaperTask>(task) {let task=&*lease_task;
        *task.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(unsafe { project::read_title(message) }.unwrap_or_default().to_string()));
    }
}
/// # Safety
/// task work finished and task remains live; free returned owned text with capy_apple_string_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_error(task: u64) -> u64 {
    let Some(lease_task)=handles::shared::<CapyPaperTask>(task) else { return 0 };
    let task=&*lease_task;
    if task.cancelled.load(Ordering::Relaxed) { return 0; }
    let result = task.result.lock().unwrap_or_else(|e| e.into_inner());
    result.as_ref().and_then(|result| result.as_ref().err())
        .and_then(|error| CString::new(error.as_str()).ok()).map_or(0, handles::text)
}
/// # Safety
/// task is owned exactly once and no calls/borrowed data remain; drop on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_free(task: u64) {
    handles::free::<CapyPaperTask>(task);
}

pub struct CapyPaperPreviews { images: Vec<Option<Arc<[u8]>>> }
/// # Safety
/// app is exclusive on its serial owner; ids is a readable NUL-terminated JSON array of paper ids.
/// Returned previews own immutable allocations independent of app, in the order of ids.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_paper_previews(app: u64, ids: *const c_char) -> u64 {
    let Some(mut lease_app)=handles::app(app) else { return 0 };
    let app=&mut *lease_app;
    let Some(ids) = (!ids.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(ids) }.to_str().ok()).flatten()
        .and_then(|ids| serde_json::from_str::<Vec<String>>(ids).ok()) else { return 0 };
    handles::store(CapyPaperPreviews { images: ids.iter().map(|id| app.host.session.paper_preview(id)).collect() })
}
/// # Safety
/// previews remains live throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_previews_count(previews: u64) -> usize {
    handles::shared::<CapyPaperPreviews>(previews).map_or(0, |previews| previews.images.len())
}
/// # Safety
/// previews is live; output pointers are writable. Borrowed bytes remain valid until previews_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_previews_read(previews: u64, index: usize, count: *mut usize) -> u32 {
    if count.is_null() {return 0;}
    unsafe {count.write(0);}
    let Some(lease_previews)=handles::shared::<CapyPaperPreviews>(previews) else { return 0 };
    let previews=&*lease_previews;
    let Some(Some(image)) = previews.images.get(index) else { return 0 };
    unsafe { count.write(image.len()); } layer_core::paper::PREVIEW_SIZE as u32
}

/// # Safety
/// Nonempty output is writable and exclusive for length bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_previews_copy(previews:u64,index:usize,offset:usize,output:*mut u8,length:usize)->bool {
    handles::shared::<CapyPaperPreviews>(previews).and_then(|previews|previews.images.get(index).cloned().flatten())
        .is_some_and(|bytes|unsafe {layer_host::ffi::copy_bytes(&bytes,offset,output,length)})
}
/// # Safety
/// previews is owned exactly once; all readers finished. Drop on the image/file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_paper_previews_free(previews: u64) {
    handles::free::<CapyPaperPreviews>(previews);
}
