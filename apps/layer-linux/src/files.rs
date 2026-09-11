//! Native file transport and dialogs. Shared Rust authorizes every transition.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{Project, ProjectLimits};
use layer_render::CanvasRenderer;
use layer_ui::*;
use std::{
    io::{BufReader, BufWriter, Write},
    path::Path,
    rc::Rc,
};

pub(crate) type OpenDocument = Rc<dyn Fn(Project, Option<DocumentLocation>)>;

impl Workspace {
    pub(crate) fn install_document_close(self: &Rc<Self>) {
        self.window.connect_close_request(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                let result = {
                    let mut gpu = w.gpu.borrow_mut();
                    let Some(gpu) = gpu.as_mut() else {
                        return glib::Propagation::Proceed;
                    };
                    if gpu.session.state().document_file.close_ready {
                        return glib::Propagation::Proceed;
                    }
                    gpu.session.request_document_close()
                };
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    w,
                    move || w.changed(result)
                ));
                glib::Propagation::Stop
            }
        ));
    }

    pub(crate) fn service_requests(self: &Rc<Self>) {
        if self.servicing.replace(true) {
            return;
        }
        let hold = self.window.application().map(|app| app.hold());
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak(rename_to = w)]
            self,
            async move {
                let _hold = hold;
                loop {
                    let next = w
                        .gpu
                        .borrow()
                        .as_ref()
                        .and_then(|g| g.session.state().requests.first().cloned());
                    let Some(request) = next else {
                        break;
                    };
                    match request.kind {
                        HostRequestKind::Document { request: document } => {
                            if let DocumentRequest::ConfirmClose { .. } = document {
                                let dialog = adw::AlertDialog::builder()
                                    .heading(document.title())
                                    .body(UNSAVED_DESCRIPTION)
                                    .build();
                                dialog.add_responses(&[
                                    ("cancel", CANCEL_DOCUMENT_LABEL),
                                    ("discard", DISCARD_DOCUMENT_LABEL),
                                    ("save", "Save"),
                                ]);
                                dialog.set_close_response("cancel");
                                dialog.set_default_response(Some("save"));
                                dialog.set_response_appearance(
                                    "save",
                                    adw::ResponseAppearance::Suggested,
                                );
                                dialog.set_response_appearance(
                                    "discard",
                                    adw::ResponseAppearance::Destructive,
                                );
                                let response = dialog.choose_future(Some(&w.window)).await;
                                let decision = match response.as_str() {
                                    "save" => CloseDecision::Save,
                                    "discard" => CloseDecision::Discard,
                                    _ => CloseDecision::Cancel,
                                };
                                let result = w.gpu.borrow_mut().as_mut().map(|g| {
                                    g.session.respond_document_close(request.id, decision)
                                });
                                if let Some(result) = result {
                                    w.changed(result);
                                }
                            } else {
                                let outcome = document_request(&w, request.id, &document).await;
                                let result = w.gpu.borrow_mut().as_mut().map(|g| {
                                    g.session.complete_document_request(request.id, outcome)
                                });
                                if let Some(result) = result {
                                    w.changed(result);
                                }
                            }
                        }
                        kind => {
                            let result = match kind {
                                HostRequestKind::NewWindow => w
                                    .window
                                    .application()
                                    .and_then(|app| app.lookup_action("new-window"))
                                    .ok_or("New Window is unavailable".into())
                                    .map(|action| action.activate(None)),
                                HostRequestKind::SaveSettings { settings } => {
                                    crate::preferences::persist(&w, settings).await
                                }
                                HostRequestKind::Document { .. } => unreachable!(),
                            };
                            w.dispatch(UiAction::CompleteRequest {
                                id: request.id,
                                error: result.err(),
                            });
                        }
                    }
                }
                w.servicing.set(false);
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().document_file.close_ready)
                {
                    w.window.close();
                }
            }
        ));
    }
}

async fn document_request(
    w: &Rc<Workspace>,
    id: u32,
    request: &DocumentRequest,
) -> Result<bool, String> {
    if matches!(request, DocumentRequest::New) {
        let dialog = adw::AlertDialog::builder().heading(request.title()).build();
        let group = adw::PreferencesGroup::new();
        let field = |label: &str, value: u32| {
            let row = adw::SpinRow::with_range(1., MAX_NEW_DOCUMENT_DIMENSION as f64, 1.);
            row.set_title(label);
            row.set_snap_to_ticks(true);
            row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
            row.set_value(value as f64);
            group.add(&row);
            row
        };
        let width = field(DOCUMENT_WIDTH_LABEL, DEFAULT_DOCUMENT_EXTENT[0]);
        let height = field(DOCUMENT_HEIGHT_LABEL, DEFAULT_DOCUMENT_EXTENT[1]);
        width.set_widget_name("new-document-width");
        height.set_widget_name("new-document-height");
        dialog.set_extra_child(Some(&group));
        dialog.add_responses(&[
            ("cancel", CANCEL_DOCUMENT_LABEL),
            ("create", request.accept_label()),
        ]);
        dialog.set_close_response("cancel");
        dialog.set_default_response(Some("create"));
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        if dialog.choose_future(Some(&w.window)).await != "create" {
            return Ok(false);
        }
        let project = new_drawing(width.value() as u32, height.value() as u32)?;
        w.open_document
            .borrow()
            .as_ref()
            .ok_or("New drawing window is unavailable")?(project, None);
        return Ok(true);
    }
    let Some(file) = choose_file(w, request).await? else {
        return Ok(false);
    };
    let path = file.path().ok_or("Choose a file on this device")?;
    let location = DocumentLocation {
        uri: file.uri().into(),
        name: path
            .file_name()
            .ok_or("Choose a filename")?
            .to_string_lossy()
            .into_owned(),
    };
    match request {
        DocumentRequest::Open => {
            let project = gio::spawn_blocking(move || {
                let file =
                    std::fs::File::open(path).map_err(|e| format!("Cannot open drawing: {e}"))?;
                Project::read(BufReader::new(file), ProjectLimits::default())
            })
            .await
            .map_err(|_| "Project reader failed")??;
            w.open_document
                .borrow()
                .as_ref()
                .ok_or("New drawing window is unavailable")?(project, Some(location));
        }
        DocumentRequest::Save { .. } => {
            let project = w
                .gpu
                .borrow_mut()
                .as_mut()
                .ok_or("Canvas unavailable")?
                .session
                .capture_project_save(id, location)?;
            gio::spawn_blocking(move || {
                let project = project.pruned()?;
                atomic_write(&path, |file| project.write(file))
            })
            .await
            .map_err(|_| "Project writer failed")??;
        }
        DocumentRequest::Export { .. } => {
            let image = export_pixels(w, id).await?;
            gio::spawn_blocking(move || atomic_write(&path, |file| write_png(file, image)))
                .await
                .map_err(|_| "PNG writer failed")??;
        }
        _ => unreachable!(),
    }
    Ok(true)
}

async fn choose_file(
    w: &Workspace,
    request: &DocumentRequest,
) -> Result<Option<gio::File>, String> {
    if let DocumentRequest::Save {
        location: Some(location),
        ..
    } = request
    {
        return Ok(Some(gio::File::for_uri(&location.uri)));
    }
    let dialog = gtk::FileDialog::builder()
        .title(request.title())
        .accept_label(request.accept_label())
        .modal(true)
        .build();
    if let Some(location) = w
        .gpu
        .borrow()
        .as_ref()
        .and_then(|g| g.session.state().document_file.location.as_ref())
        && let Some(folder) = gio::File::for_uri(&location.uri).parent()
    {
        dialog.set_initial_folder(Some(&folder));
    }
    let (label, extension) = request.filter();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(label));
    filter.add_suffix(extension);
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let result = match request {
        DocumentRequest::Open => dialog.open_future(Some(&w.window)).await,
        DocumentRequest::Save { name, .. } | DocumentRequest::Export { name } => {
            dialog.set_initial_name(Some(name));
            dialog.save_future(Some(&w.window)).await
        }
        _ => unreachable!(),
    };
    match result {
        Ok(file) => Ok(Some(file)),
        Err(error)
            if error.matches(gtk::DialogError::Dismissed)
                || error.matches(gtk::DialogError::Cancelled) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) async fn export_pixels(
    w: &Rc<Workspace>,
    id: u32,
) -> Result<layer_render::ReadbackImage, String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    // The ordered worker queue must receive the final document frame before
    // its readback. Merely changing the model does not update GPU pixels.
    loop {
        let pending = w
            .gpu
            .borrow()
            .as_ref()
            .ok_or("Canvas unavailable")?
            .session
            .engine()
            .has_pending_document_edits();
        if !pending {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("The canvas is not ready to export".into());
        }
        w.wake();
        glib::timeout_future(std::time::Duration::from_millis(16)).await;
    }
    w.gpu
        .borrow_mut()
        .as_mut()
        .ok_or("Canvas unavailable")?
        .session
        .renderer_mut()
        .request_readback(id as u64)
        .map_err(|e| e.to_string())?;
    loop {
        {
            let mut gpu = w.gpu.borrow_mut();
            let renderer = gpu
                .as_mut()
                .ok_or("Canvas unavailable")?
                .session
                .renderer_mut();
            renderer.ready()?;
            while let Some(image) = renderer.take_readback() {
                let image = image.map_err(|e| e.to_string())?;
                if image.request_id == id as u64 {
                    return Ok(image);
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err("The canvas could not be exported".into());
        }
        glib::timeout_future(std::time::Duration::from_millis(16)).await;
    }
}

fn write_png(output: &mut dyn Write, image: layer_render::ReadbackImage) -> Result<(), String> {
    image.write_png(output)
}

/// Local atomic streaming write: the original survives validation, encoding or
/// disk errors. Only a fully flushed sibling temporary file replaces it.
fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut dyn Write) -> Result<(), String>,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let parent = path.parent().ok_or("Choose a destination folder")?;
    let temporary = loop {
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = parent.join(format!(".capy-save-{}-{serial}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot save drawing: {e}")),
        }
    };
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let cleanup = Cleanup(temporary.0);
    let mut file = BufWriter::new(temporary.1);
    write(&mut file)?;
    file.flush()
        .and_then(|()| file.get_ref().sync_all())
        .map_err(|e| format!("Cannot finish saving: {e}"))?;
    std::fs::rename(&cleanup.0, path).map_err(|e| format!("Cannot replace drawing: {e}"))?;
    std::fs::File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("Cannot finish saving: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_writes_preserve_the_original_on_failure_and_png_preserves_alpha() {
        let directory = std::env::temp_dir().join(format!("capy-file-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("drawing.capy");
        let project = new_drawing(32, 24).unwrap();
        atomic_write(&path, |file| project.write(file)).unwrap();
        let before = std::fs::read(&path).unwrap();
        let result = atomic_write(&path, |file| {
            file.write_all(b"incomplete").unwrap();
            Err("Disk full".into())
        });
        assert_eq!(result.unwrap_err(), "Disk full");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        let mut changed = project.clone();
        changed.document.width = 40;
        atomic_write(&path, |file| changed.write(file)).unwrap();
        assert_eq!(
            Project::read(std::fs::File::open(&path).unwrap(), Default::default()).unwrap(),
            changed
        );
        let rgba = vec![0, 0, 0, 0, 255, 80, 30, 128, 25, 90, 240, 255];
        let mut png = Vec::new();
        write_png(
            &mut png,
            layer_render::ReadbackImage {
                request_id: 1,
                width: 3,
                height: 1,
                stride: 12,
                bytes: rgba.clone(),
            },
        )
        .unwrap();
        let mut decoder = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert!(decoder.info().srgb.is_some());
        let mut decoded = vec![0; decoder.output_buffer_size()];
        let info = decoder.next_frame(&mut decoded).unwrap();
        assert_eq!(&decoded[..info.buffer_size()], rgba);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
