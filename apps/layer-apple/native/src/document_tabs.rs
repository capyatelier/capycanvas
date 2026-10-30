//! One window's drawing collection. Swift owns native contacts and serial/worker
//! scheduling; Rust owns membership, parking, admission and activation validity.
use super::*;
use layer_host::{
    Renderer,
    window::{Activation, DocumentWindow, PreparedClose},
};
use layer_ui::UiSession;
use serde_json::{Value, json};
use std::sync::Mutex;

pub(crate) type Window = DocumentWindow<Box<UiSession<Renderer>>>;
struct Job {
    activation: Option<Activation>,
    close: Option<PreparedClose>,
    retired: Option<Box<UiSession<Renderer>>>,
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
        if value["op"] == "session_close" {
            let previous = self.host.session.state().revision;
            let change = self.host.session.request_session_close()?;
            self.host.apply_change(previous, change);
            return Ok(json!(true));
        }
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
    fn document_job(&self, activation: Option<Activation>) -> Box<CapyDocumentTask> {
        Box::new(CapyDocumentTask(Mutex::new(Job {
            activation,
            close: None,
            retired: None,
            tiles: self
                .window
                .documents
                .parked()
                .map(|(_, p)| p.tiles.clone())
                .collect(),
            budget: self.window.documents.budget.inactive_ram,
            error: None,
            storage_error: None,
        })))
    }
}
/// # Safety
/// Serial owner. Flush native input and wait for every retained undo raster,
/// not just the current recovery snapshot, before retiring the GPU.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_prepare_switch(app: u64, now: u64) -> i32 {
    let Some(mut owner) = handles::app(app) else { return -1; };
    if owner.perform(|a| {
        a.host.session.prepare_document_action()?;
        if !a.host.session.can_switch_document() { return Err(layer_ui::DocumentTransportRefusal::SwitchOperation.message(a.host.session.localization()).to_string()); }
        Ok(())
    }).is_none() { return -1; }
    drop(owner);
    let ready = unsafe { capy_apple_prepare_recovery(app, now) };
    if ready != 0 {
        return ready;
    }
    let Some(mut lease_a)=handles::app(app) else {
        return -1;
    };
    let a=&mut *lease_a;
    a.perform(|a| Ok(i32::from(!a.window.park_ready(&a.host)?)))
        .unwrap_or(-1)
}
/// # Safety
/// Serial owner only. The returned job must be prepared on a worker and freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_switch(
    app: u64,
    id: u64,
    closing: bool,
) -> u64 {
    let Some(mut lease_a)=handles::app(app) else {
        return 0;
    };
    let a=&mut *lease_a;
    a.perform(|a| {
        let options = a.host.renderer_options();
        if closing {
            let close = a.window.prepare_close(&mut a.host, options)?;
            let task = a.document_job(None);
            task.0.lock().unwrap_or_else(|e| e.into_inner()).close = Some(close);
            return Ok(handles::store(*task));
        }
        let Some((activation, _)) = a.window.switch(&mut a.host, id, closing, options, |_| {})?
        else {
            return Ok(handles::store(*a.document_job(None)));
        };
        a.document_retired();
        Ok(handles::store(*a.document_job(Some(activation))))
    })
    .unwrap_or(0)
}
/// # Safety
/// Serial owner; every prepared close must be committed or cancelled before free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_close_commit(app: u64, task: u64, accept: bool) -> i32 {
    let (Some(mut lease_a),Some(lease_task))=(handles::app(app),handles::shared::<CapyDocumentTask>(task)) else { return -1; };
    let (a,task)=(&mut *lease_a,&*lease_task);
    a.perform(|a| {
        let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        let close = job.close.take().ok_or("The drawing close is unavailable")?;
        if accept {
            let (activation, retired) = a.window.commit_close(&mut a.host, close, |_| {})?;
            job.activation = Some(activation);
            job.retired = retired;
            a.document_retired();
        } else { a.window.cancel_close(&mut a.host, close)?; }
        Ok(())
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Serial owner before publishing the session membership removal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_close_validate(app: u64, task: u64) -> i32 {
    let (Some(mut lease_a),Some(lease_task))=(handles::app(app),handles::shared::<CapyDocumentTask>(task)) else { return -1; };
    let (a,task)=(&mut *lease_a,&*lease_task);
    a.perform(|a| {
        let job = task.0.lock().unwrap_or_else(|e| e.into_inner());
        a.window.validate_close(&a.host, job.close.as_ref().ok_or("The drawing close is unavailable")?)
    }).map_or(-1, |_| 0)
}
/// # Safety
/// Serial owner only; schedules inactive backing I/O without changing selection.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_document_storage(app: u64) -> u64 {
    handles::app(app).map_or(0, |a| handles::store(*a.document_job(None)))
}
/// # Safety
/// Worker only; job stays alive and has no concurrent caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_document_prepare(task: u64) -> i32 {
    let Some(lease_task)=handles::shared::<CapyDocumentTask>(task) else {
        return -1;
    };
    let task=&*lease_task;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("capy-drawing".into())
            .stack_size(8 * 1024 * 1024)
            .spawn_scoped(scope, || prepare_document(task))
            .map_or(-1, |worker| worker.join().unwrap_or(-1))
    })
}
fn prepare_document(task: &CapyDocumentTask) -> i32 {
    let mut job = task.0.lock().unwrap_or_else(|e| e.into_inner());
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        for tiles in &job.tiles {
            if layer_core::raster_storage::resident_tile_bytes(job.tiles.iter()) <= job.budget {
                break;
            }
            if let Err(e) = layer_core::temp_files::directory()
                .and_then(|directory| layer_core::raster_storage::spill_to_directory(tiles, directory))
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
    app: u64,
    task: u64,
) -> i32 {
    let (Some(mut lease_a),Some(lease_task))=(handles::app(app),handles::shared::<CapyDocumentTask>(task)) else {
        return -1;
    };
    let (a,task)=(&mut *lease_a,&*lease_task);
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
pub unsafe extern "C" fn capy_document_free(task: u64) {
    handles::free::<CapyDocumentTask>(task);
}
