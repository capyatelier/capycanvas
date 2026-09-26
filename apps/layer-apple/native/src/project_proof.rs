//! Shared print-view preparation on a worker; adoption stays on the editor owner.
use super::*;
use layer_ui::proof_workflow::ProofPreparation;
use std::sync::Arc;

pub(super) struct Task {
    job: ProofPreparation,
    lut: Option<Arc<layer_color::ProofLut>>,
}

/// # Safety
/// Editor owner only. Recipe is optional ProofRecipe JSON; request 0 restores a
/// view without changing the document. The returned task never borrows the app.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_task(
    app: *mut CapyApple,
    request: u32,
    recipe: *const c_char,
) -> *mut CapyProjectTask {
    let Some(app) = (unsafe { app.as_mut() }) else {
        return std::ptr::null_mut();
    };
    app.perform(|app| {
        let recipe = if recipe.is_null() {
            None
        } else {
            serde_json::from_str(unsafe { read_title(recipe) }?).map_err(|e| e.to_string())?
        };
        let session = &app.host.session;
        let job = ProofPreparation::begin(session, (request != 0).then_some(request), recipe)?;
        Ok(CapyProjectTask::new(
            Payload::Proof(Box::new(Task { job, lut: None })),
            session.state().document_file.epoch,
            session.engine().document().revision,
            None,
        ))
    })
    .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// Worker only; cancellation may run concurrently. No session or GPU access.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_proof_build(task: *const CapyProjectTask) -> i32 {
    let Some(task) = (unsafe { task.as_ref() }) else {
        return -1;
    };
    task.perform(|payload| {
        let Payload::Proof(proof) = payload else {
            return Err("Not a proof task".into());
        };
        let start = Instant::now();
        proof.lut = Some(proof.job.build(|| {
            task.control.is_cancelled() || start.elapsed() >= Duration::from_secs(120)
        })?);
        Ok(())
    })
}

/// # Safety
/// Worker only, after build. Borrowed bytes remain valid until task release;
/// copy them before another task operation. A null result means no preservation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_project_proof_preservation(
    task: *const CapyProjectTask,
    bytes: *mut *const u8,
    count: *mut usize,
) -> i32 {
    let (Some(task), Some(bytes), Some(count)) = (
        unsafe { task.as_ref() },
        unsafe { bytes.as_mut() },
        unsafe { count.as_mut() },
    ) else {
        return -1;
    };
    *bytes = std::ptr::null();
    *count = 0;
    task.perform_inner(|payload| {
        let Payload::Proof(proof) = payload else {
            return Err("Not a proof task".into());
        };
        if let Some(value) = proof.job.preservation() {
            *bytes = value.as_ptr();
            *count = value.len();
        }
        Ok(())
    })
}

/// # Safety
/// Editor owner only, after the worker finishes. Check again before writing the
/// replaced ICC; apply repeats the same shared stale-document validation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_check(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    app.perform(|app| {
        task.check_cancelled()?;
        let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        let Payload::Proof(proof) = &state.payload else {
            return Err("Not a proof task".into());
        };
        proof.job.validate(&app.host.session)
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Editor owner only, after preparation and any required durable ICC copy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_apply(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    preserved: bool,
) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    app.perform(|app| {
        task.check_cancelled()?;
        let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        let Payload::Proof(proof) = &state.payload else {
            return Err("Not a proof task".into());
        };
        let lut = proof.lut.clone().ok_or("Proof preview is not prepared")?;
        proof.job.validate(&app.host.session)?;
        if proof.job.preservation().is_some() && !preserved {
            return Err("Save the original in Saved Profiles before replacing it".into());
        }
        if unsafe { capy_project_begin_commit(task) } < 0 {
            return Err("Proof preparation cancelled".into());
        }
        let previous = app.host.session.state().revision;
        let change = proof.job.apply(&mut app.host.session, preserved)?;
        app.host.proof.retain(&proof.job, lut)?;
        app.host.apply_change(previous, change);
        app.host.dirty = true;
        Ok(())
    })
    .map_or(-1, |_| 0)
}

/// # Safety
/// Editor owner only. Report a terminal background preparation error; setup
/// errors remain in the form and do not poison the previously valid view.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_proof_failed(
    app: *mut CapyApple,
    task: *const CapyProjectTask,
    message: *const c_char,
) -> i32 {
    let (Some(app), Some(task)) = (unsafe { app.as_mut() }, unsafe { task.as_ref() }) else {
        return -1;
    };
    app.perform(|app| {
        let state = task.state.lock().unwrap_or_else(|e| e.into_inner());
        let Payload::Proof(proof) = &state.payload else {
            return Err("Not a proof task".into());
        };
        app.host.proof.fail(
            &app.host.session,
            &proof.job,
            unsafe { read_title(message) }?.into(),
        );
        Ok(())
    })
    .map_or(-1, |_| 0)
}
