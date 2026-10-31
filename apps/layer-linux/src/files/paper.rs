use crate::workspace::Workspace;
use super::reader::cancellable_file;
use adw::prelude::*;
use gtk::{gio, glib};
use layer_ui::paper_library::{PaperLibraryError, PaperRequest};
use std::{ path::PathBuf, rc::Rc, sync::{Arc, atomic::AtomicBool}};

fn directory(w: &Workspace) -> Result<PathBuf, String> {
    crate::storage::papers().map(PathBuf::from).ok_or_else(|| w.localization().text(layer_ui::MessageId::DOCUMENTS_ERROR_CANVAS_UNAVAILABLE).to_string())
}

fn apply(w: &Rc<Workspace>, update: impl FnOnce(&mut layer_ui::UiSession<crate::render_thread::RenderWorker>) -> Result<layer_ui::UiChange, String>) -> Result<(), String> {
    let change = w.gpu.borrow_mut().as_mut().map(|gpu| update(&mut gpu.session)).transpose()?;
    if let Some(change) = change { w.changed(Ok(change)); }
    Ok(())
}

pub(crate) fn load_library(w: &Rc<Workspace>) {
    let Some(directory) = crate::storage::papers().map(PathBuf::from) else { return };
    glib::MainContext::default().spawn_local(glib::clone!(#[weak] w, async move {
        match gio::spawn_blocking(move || layer_host::papers::list(&directory)).await {
            Ok(Ok(entries)) => { let _ = apply(&w, |session| session.set_paper_library(entries)); }
            Ok(Err(error)) => eprintln!("Paper library: {error}"),
            Err(_) => eprintln!("Paper library reader failed"),
        }
    }));
}

async fn choose_image(w: &Rc<Workspace>) -> Result<Option<PathBuf>, String> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&layer_ui::DocumentDeliveryCopy::new(&w.localization()).images));
    for suffix in layer_color::photo::extensions() { filter.add_suffix(suffix); }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title(w.localization().text(layer_ui::MessageId::RESOURCES_PAPER_IMPORT).as_ref())
        .accept_label(w.localization().text(layer_ui::MessageId::DOCUMENTS_IMPORT_ACCEPT).as_ref())
        .filters(&filters)
        .default_filter(&filter)
        .build();
    match super::chooser::open(&dialog, &w.window, super::chooser::Folder::Artwork).await {
        Ok(file) => file.path().map(Some).ok_or_else(|| "Choose an image on this device".into()),
        Err(e) if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn import(directory: PathBuf, path: PathBuf) -> Result<(String, layer_core::color::source::SourceImage, Vec<layer_ui::paper_library::PaperEntry>), PaperLibraryError> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut images = layer_ui::ImageImportBatch::new(Default::default(), layer_core::color::RgbSpace::ProPhoto, Default::default());
    let reader = cancellable_file(&path, cancelled.clone()).map_err(|e| e.to_string())?;
    images.read_with_decoder(reader,&name,&cancelled,crate::photo_reader::read).map_err(|e| format!("{name}: {e}"))?;
    let (_, source) = images.take_sources(false)?.into_iter().next().ok_or_else(|| "No image to import".to_string())?;
    layer_host::papers::import(&directory, &source, &name, &|| false)
}

pub(crate) async fn run(w: &Rc<Workspace>, request: PaperRequest) -> Result<(), String> {
    let directory = directory(w)?;
    let failed = |w: &Rc<Workspace>, error: PaperLibraryError| error.message(&w.localization());
    let worker = || "Paper library worker failed".to_string();
    let retained=if matches!(&request,PaperRequest::Load { .. } | PaperRequest::Import { .. }) {w.gpu.borrow().as_ref().map(|gpu|gpu.session.paper_sources()).unwrap_or_default()}else{Vec::new()};
    match request {
        PaperRequest::Import { layer } => {
            let Some(path) = choose_image(w).await? else { return Ok(()) };
            let (id, image, entries) = gio::spawn_blocking(move || {
                let (id,image,entries)=import(directory,path)?;
                let image=layer_host::papers::canonical_source(&id,image,&retained)?;
                Ok::<_,PaperLibraryError>((id,image,entries))
            }).await.map_err(|_| worker())?.map_err(|e| failed(w, e))?;
            apply(w, |session| session.paper_imported(layer, entries, &id, image))
        }
        PaperRequest::Load { id } => {
            let read = id.clone();
            let image = gio::spawn_blocking(move || {
                layer_host::papers::canonical_source(&read,layer_host::papers::load(&directory,&read)?,&retained)
            }).await.map_err(|_| worker())??;
            apply(w, |session| session.paper_loaded(&id, image))
        }
        PaperRequest::Add { layer, name } => {
            let image = w.gpu.borrow().as_ref().and_then(|gpu| gpu.session.paper_image(layer)).ok_or_else(worker)?;
            let (_, entries) = gio::spawn_blocking(move || layer_host::papers::store(&directory, &image, &name)).await.map_err(|_| worker())?.map_err(|e| failed(w, e))?;
            apply(w, |session| session.set_paper_library(entries))
        }
        PaperRequest::Rename { id, name } => {
            let entries = gio::spawn_blocking(move || layer_host::papers::rename(&directory, &id, &name)).await.map_err(|_| worker())??;
            apply(w, |session| session.set_paper_library(entries))
        }
        PaperRequest::Delete { id } => {
            let entries = gio::spawn_blocking(move || layer_host::papers::delete(&directory, &id)).await.map_err(|_| worker())??;
            apply(w, |session| session.set_paper_library(entries))
        }
    }
}

pub(crate) async fn rename(w: &Rc<Workspace>, id: String, name: String) {
    let localization = w.localization();
    let entry = adw::EntryRow::builder().title(localization.text(layer_ui::MessageId::COMMON_NAME).as_ref()).text(&name).build();
    entry.set_widget_name("paper-rename-name");
    let group = adw::PreferencesGroup::new();
    group.add(&entry);
    let dialog = adw::AlertDialog::builder()
        .heading(localization.text(layer_ui::MessageId::RESOURCES_PAPER_RENAME_TITLE).as_ref())
        .extra_child(&group)
        .build();
    dialog.set_widget_name("paper-rename-dialog");
    dialog.add_responses(&[("cancel", &layer_ui::CommonCopy::new(&localization).cancel), ("rename", &localization.text(layer_ui::MessageId::WORKSPACE_RENAME))]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("rename"));
    dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
    entry.connect_changed(glib::clone!(#[weak] dialog, move |entry| {
        dialog.set_response_enabled("rename", layer_ui::paper_library::paper_name(&entry.text()).is_some());
    }));
    if crate::alert::choose(dialog, &w.window).await == "rename" {
        w.dispatch(layer_ui::UiAction::Effect { action: layer_ui::EffectAction::RenamePaper { id, name: entry.text().into() } });
    }
}
