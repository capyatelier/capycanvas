//! Native file transport and dialogs. Shared Rust authorizes every transition.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::Project;
use layer_ui::*;
use std::rc::Rc;

pub(crate) mod export;
pub(crate) mod open;
pub(crate) mod launch;
mod properties;
mod color;
mod proof;
mod place;
pub(crate) mod drop;
pub(crate) mod profile;
mod source;
mod preview;
mod rasterize;
mod reader;
mod chooser;

pub(crate) type OpenDocument =
    Rc<dyn Fn(Project, Option<DocumentLocation>, Option<std::path::PathBuf>)>;

impl Workspace {
    pub(crate) fn install_document_close(self: &Rc<Self>) {
        self.window.connect_close_request(glib::clone!(
            #[weak(rename_to = w)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| {
                if w.gpu
                    .borrow()
                    .as_ref()
                    .is_some_and(|g| g.session.state().document_file.close_ready)
                {
                    return if w.workspaces.request_close(&w) {
                        glib::Propagation::Stop
                    } else {
                        w.recovery.discard();
                        glib::Propagation::Proceed
                    };
                }
                let result = {
                    let mut gpu = w.gpu.borrow_mut();
                    let Some(gpu) = gpu.as_mut() else {
                        return glib::Propagation::Proceed;
                    };
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
                                let response = crate::alert::choose(dialog, &w.window).await;
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
                                HostRequestKind::SoftProofSetup => proof::run(&w).await,
                                HostRequestKind::Histogram => {
                                    crate::histogram::show(&w);
                                    Ok(())
                                }
                                HostRequestKind::SetFullscreen { fullscreen } => {
                                    if fullscreen {
                                        w.window.fullscreen();
                                    } else {
                                        w.window.unfullscreen();
                                    }
                                    Ok(())
                                }
                                HostRequestKind::NewWindow => w
                                    .window
                                    .application()
                                    .and_then(|app| app.lookup_action("new-window"))
                                    .ok_or("New Window is unavailable".into())
                                    .map(|action| action.activate(None)),
                                HostRequestKind::SaveSettings { settings } => {
                                    crate::preferences::persist(&w, settings).await
                                }
                                HostRequestKind::Workspace { command } => {
                                    w.workspaces.command(&w, command).await
                                }
                                HostRequestKind::OpenLink { link } => {
                                    gtk::UriLauncher::new(link.url())
                                        .launch_future(Some(&w.window))
                                        .await
                                        .map_err(|e| e.to_string())
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
    if let DocumentRequest::ChangeColor { operation } = request {
        return color::run(w, id, *operation).await;
    }
    if let DocumentRequest::ColorHistory { .. } = request {
        return color::history(w, id).await;
    }
    if matches!(request, DocumentRequest::Place | DocumentRequest::Paste) {
        return place::run(w, matches!(request, DocumentRequest::Paste)).await;
    }
    if let DocumentRequest::RasterizeSource { .. } = request {
        return rasterize::run(w, id).await;
    }
    if let DocumentRequest::RepairSourceProfile { .. } = request {
        return source::repair(w, id).await;
    }
    if matches!(request, DocumentRequest::Properties) {
        return properties::show(w).await;
    }
    if matches!(request, DocumentRequest::New) {
        let Some(project) = crate::new_document::run(w).await? else { return Ok(false); };
        w.open_document
            .borrow()
            .as_ref()
            .ok_or("New drawing window is unavailable")?(project, None, None);
        return Ok(true);
    }
    if let DocumentRequest::Export { name } = request {
        return export::run(w, id, name).await;
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
            let (policy, working) = {
                let gpu = w.gpu.borrow();
                let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
                (session.state().settings.photo_open, session.engine().document().color.space)
            };
            let Some((project, location)) = open::prepare(&w.window, file, policy, working).await? else {
                return Ok(false);
            };
            w.open_document
                .borrow()
                .as_ref()
                .ok_or("New drawing window is unavailable")?(
                project, location, None
            );
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
    if matches!(request, DocumentRequest::Open)
        && let Some(incoming) = w.image_drop.borrow_mut().take()
    {
        return Ok(Some(incoming.files.into_iter().next().ok_or("No drawing to open")?));
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
    if matches!(request, DocumentRequest::Open) {
        filter.set_name(Some("Drawings and photos"));
        for suffix in layer_color::photo::extensions() { filter.add_suffix(suffix); }
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let result = match request {
        DocumentRequest::Open => chooser::open(&dialog, &w.window, chooser::Folder::Artwork).await,
        DocumentRequest::Save { name, .. } => {
            dialog.set_initial_name(Some(name));
            chooser::save(&dialog, &w.window, chooser::Folder::Save).await
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

pub(crate) use layer_core::atomic_write;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
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
        layer_render::ReadbackImage {
            request_id: 1,
            width: 3,
            height: 1,
            stride: 12,
            bytes: rgba.clone(),
        }
        .write_png(&mut png)
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
