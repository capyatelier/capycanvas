//! One window's drawing collection. Swift owns native contacts and serial/worker
//! scheduling; Rust owns membership, parking, admission and activation validity.
use super::*;
use layer_host::{
    Renderer,
    window::{Activation, DocumentWindow},
};
use layer_ui::UiSession;
use serde_json::{Value, json};
use std::sync::Mutex;

pub(crate) type Window = DocumentWindow<Box<UiSession<Renderer>>>;
struct Job {
    activation: Option<Activation>,
    tiles: Vec<layer_core::raster_storage::RetainedTiles>,
    budget: usize,
    error: Option<CString>,
    storage_error: Option<String>,
}
pub struct CapyDocumentTask(Mutex<Job>);
impl CapyApple {
    pub(crate) fn document_retired(&mut self) {
        self.metal.document_changed();
        self.host.proof = Default::default();
        self.dismissed_contacts.clear();
    }
    pub(crate) fn tabs_request(&mut self, value: Value) -> Result<Value, String> {
        if value["op"] != "open" {
            let request = serde_json::from_value(value).map_err(|e| e.to_string())?;
            return self.window.request(&mut self.host, request);
        }
        if self.host.session.state().document_file.busy
            || !self
                .host
                .session
                .command(layer_ui::CommandId::OpenDocument)
                .enabled
        {
            return Ok(json!(false));
        }
        self.host.dispatch(layer_ui::UiAction::Invoke {
            command: layer_ui::CommandId::OpenDocument,
        })?;
        Ok(json!(true))
    }
    fn document_job(&self, activation: Option<Activation>) -> *mut CapyDocumentTask {
        Box::into_raw(Box::new(CapyDocumentTask(Mutex::new(Job {
            activation,
            tiles: self
                .window
                .documents
                .parked()
                .map(|(_, p)| p.tiles.clone())
                .collect(),
            budget: self.window.documents.budget.inactive_ram,
            error: None,
            storage_error: None,
        }))))
    }
}
/// # Safety
/// Serial owner. Flush native input and wait for every retained undo raster,
/// not just the current recovery snapshot, before retiring the GPU.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_prepare_switch(app: *mut CapyApple, now: u64) -> i32 {
    let ready = unsafe { capy_apple_prepare_recovery(app, now) };
    if ready != 0 {
        return ready;
    }
    let Some(a) = (unsafe { app.as_mut() }) else {
        return -1;
    };
    a.perform(|a| Ok(i32::from(!a.window.park_ready(&a.host)?)))
        .unwrap_or(-1)
}
/// # Safety
/// Serial owner only. The returned job must be prepared on a worker and freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_switch(
    app: *mut CapyApple,
    id: u64,
    closing: bool,
) -> *mut CapyDocumentTask {
    let Some(a) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    a.perform(|a| {
        let options = a.host.renderer_options(a.metal.cache.clone());
        let Some((activation, _)) = a.window.switch(&mut a.host, id, closing, options, |_| {})?
        else {
            return Ok(a.document_job(None));
        };
        a.document_retired();
        Ok(a.document_job(Some(activation)))
    })
    .unwrap_or(std::ptr::null_mut())
}
/// # Safety
/// Serial owner only; schedules inactive backing I/O without changing selection.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_storage(app: *mut CapyApple) -> *mut CapyDocumentTask {
    unsafe { app.as_ref() }.map_or(std::ptr::null_mut(), |a| a.document_job(None))
}
/// # Safety
/// Worker only; directory is UTF-8, job stays alive and has no concurrent caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_prepare(
    task: *mut CapyDocumentTask,
    directory: *const c_char,
) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let path = unsafe { project::read_title(directory) }?;
        for tiles in &job.tiles {
            if layer_core::raster_storage::resident_tile_bytes(job.tiles.iter()) <= job.budget {
                break;
            }
            if let Err(e) =
                layer_core::raster_storage::spill_to_directory(tiles, std::path::Path::new(path))
            {
                job.storage_error = Some(e);
                break;
            }
        }
        match &mut job.activation {
            Some(activation) => activation.work(),
            None => Ok(()),
        }
    }))
    .unwrap_or_else(|_| Err("Drawing activation failed".into()));
    match result {
        Ok(()) => 0,
        Err(e) => {
            job.error = CString::new(e.replace('\0', " ")).ok();
            -1
        }
    }
}
/// # Safety
/// Serial owner after worker completion; preserves the selected document on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_resume(
    app: *mut CapyApple,
    task: *mut CapyDocumentTask,
) -> i32 {
    let (Some(a), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    a.perform(|a| {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        let job = &mut *job;
        a.window
            .documents
            .storage_completed(job.storage_error.clone().map_or(Ok(()), Err));
        let renderer = match &mut job.activation {
            Some(activation) if job.error.is_none() => a.window.resume(&a.host, activation)?,
            _ => None,
        };
        if let Some(e) = &job.error {
            return Err(e.to_string_lossy().into_owned());
        }
        if let Some(gpu) = renderer {
            a.metal.install_renderer(&mut a.host, gpu)?;
            a.host.error = None;
        }
        if job.activation.is_some() {
            a.window.changed(&mut a.host);
        } else {
            a.host.invalidate_snapshot();
        }
        Ok(())
    })
    .map_or(-1, |_| 0)
}
/// # Safety
/// No outstanding job calls. Resource destruction belongs on the file worker.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_free(task: *mut CapyDocumentTask) {
    if !task.is_null() {
        drop(unsafe { Box::from_raw(task) });
    }
}
