//! Retained file/clipboard image import. Transfer uses a bounded buffer and a
//! private temporary file; decode/CMM work runs outside the GTK owner.
use crate::workspace::Workspace;
use super::reader::cancellable_file;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    io::BufReader,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const CLIPBOARD_FILE_LIMIT: usize = 512 * 1024 * 1024;

struct TemporaryImage(PathBuf);
impl Drop for TemporaryImage {
    fn drop(&mut self) {
        let path = self.0.clone();
        gio::spawn_blocking(move || { let _ = std::fs::remove_file(path); });
    }
}

async fn spool(clipboard: &gdk::Clipboard, mime: &str) -> Result<TemporaryImage, String> {
    let (input, _) = clipboard
        .read_future(&[mime], glib::Priority::DEFAULT)
        .await
        .map_err(|e| format!("Copy a supported image ({}) to paste: {e}", layer_color::photo::format_names()))?;
    let temporary = gio::spawn_blocking(|| {
        use std::os::unix::fs::OpenOptionsExt;
        let directory = layer_core::temp_files::directory()?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        let path = directory.join(format!("clipboard-image-{}", layer_core::PortableId::random()));
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| e.to_string())?;
        Ok::<_, String>(TemporaryImage(path))
    }).await.map_err(|_| "Clipboard transfer failed")??;
    let output = gio::File::for_path(&temporary.0).append_to_future(gio::FileCreateFlags::NONE, glib::Priority::DEFAULT).await.map_err(|e| e.to_string())?;
    let mut total = 0usize;
    loop {
        let bytes = input
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await
            .map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            break;
        }
        total = total
            .checked_add(bytes.len())
            .filter(|n| *n <= CLIPBOARD_FILE_LIMIT)
            .ok_or("The clipboard image file exceeds 512 MiB")?;
        let length = bytes.len();
        let (_, written, error) = output
            .write_all_future(bytes, glib::Priority::DEFAULT)
            .await
            .map_err(|(_, e)| e.to_string())?;
        if let Some(error) = error {
            return Err(error.to_string());
        }
        if written != length {
            return Err("Incomplete clipboard image transfer".into());
        }
    }
    output
        .close_future(glib::Priority::DEFAULT)
        .await
        .map_err(|e| e.to_string())?;
    input
        .close_future(glib::Priority::DEFAULT)
        .await
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

/// Import chosen or dropped images, or with `mode` paste an image another
/// application put on the clipboard.
pub(super) async fn run(w: &Rc<Workspace>, mode: Option<layer_ui::PasteMode>) -> Result<bool, String> {
    let paste = mode.is_some();
    let request = mode.map_or(layer_ui::DocumentRequest::Place, |mode| layer_ui::DocumentRequest::Paste { mode });
    let incoming = if paste { None } else { w.image_drop.borrow_mut().take() };
    let (context, policy, working) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        let context = if let Some(d) = &incoming {
            layer_ui::ImagePlacementContext { epoch: d.epoch, revision: d.revision, target: d.target, center: d.center, destination: d.destination }
        } else { session.image_placement_context(None, None)? };
        (context, session.state().settings.photo_open, session.engine().document().composition().color.space)
    };
    let paths = if paste {
        Vec::new()
    } else if let Some(incoming) = incoming {
        incoming.files.into_iter().map(|file| file.path().ok_or("Drop images stored on this device"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&layer_ui::DocumentDeliveryCopy::new(&w.localization()).images));
        for suffix in layer_color::photo::extensions() {
            filter.add_suffix(suffix);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(request.title(&w.localization()).as_ref())
            .accept_label(request.accept_label(&w.localization()).as_ref())
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let weak = dialog.downgrade(); let filter = filter.downgrade(); let request = request.clone();
        w.on_localization(move |localization| {
            let Some(dialog) = weak.upgrade() else { return false };
            dialog.set_title(&request.title(localization)); dialog.set_accept_label(Some(&request.accept_label(localization)));
            if let Some(filter) = filter.upgrade() { filter.set_name(Some(&layer_ui::DocumentDeliveryCopy::new(localization).images)); }
            true
        });
        match super::chooser::open_multiple(&dialog, &w.window, super::chooser::Folder::Artwork).await {
            Ok(files) => files.iter::<gio::File>().map(|file|
                file.map_err(|e| e.to_string())?.path().ok_or("Choose images on this device".into())
            ).collect::<Result<Vec<_>, String>>()?,
            Err(e)
                if e.matches(gtk::DialogError::Dismissed)
                    || e.matches(gtk::DialogError::Cancelled) =>
            {
                return Ok(false);
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let dialog = adw::AlertDialog::builder()
        .heading(request.title(&w.localization()).as_ref())
        .body(layer_ui::bootstrap_view(&w.localization()).preparing_document.as_ref())
        .build();
    dialog.set_widget_name("image-import-progress");
    dialog.add_response("cancel", &layer_ui::CommonCopy::new(&w.localization()).cancel);
    dialog.set_close_response("cancel");
    let weak = dialog.downgrade();
    w.on_localization(move |localization| {
        let Some(dialog) = weak.upgrade() else { return false };
        dialog.set_heading(Some(&request.title(localization))); dialog.set_body(&layer_ui::bootstrap_view(localization).preparing_document);
        dialog.set_response_label("cancel", &layer_ui::CommonCopy::new(localization).cancel);
        true
    });
    let cancelled = Arc::new(AtomicBool::new(false));
    let transfer = gio::Cancellable::new();
    let signal = dialog.connect_response(
        Some("cancel"),
        glib::clone!(
            #[strong]
            cancelled,
            #[strong]
            transfer,
            move |_, _| {
                cancelled.store(true, Ordering::Release);
                transfer.cancel();
            }
        ),
    );
    dialog.present(Some(&w.window));
    let result = async {
        let sources = if paste {
            let clipboard = w.window.clipboard();
            let mut result = Err(format!("Copy a supported image ({}) to paste", layer_color::photo::format_names()));
            if clipboard.formats().union_deserialize_types().contains_type(gdk::FileList::static_type()) {
                result = async {
                    let value = gio::CancellableFuture::new(clipboard.read_value_future(gdk::FileList::static_type(), glib::Priority::DEFAULT), transfer.clone())
                        .await.map_err(|_| "Image import cancelled")?.map_err(|e| e.to_string())?;
                    let paths = value.get::<gdk::FileList>().map_err(|e| e.to_string())?.files().iter()
                        .map(|file| file.path().ok_or_else(|| "Choose images stored on this device".to_string())).collect::<Result<Vec<_>, _>>()?;
                    let control = cancelled.clone();
                    gio::spawn_blocking(move || read_sources(paths, true, control)).await.map_err(|_| "Image reader failed")?
                }.await;
            }
            for mime in layer_color::photo::mime_types() {
                if result.is_ok() || cancelled.load(Ordering::Acquire) { break; }
                if !clipboard.formats().contain_mime_type(mime) { continue; }
                result = async {
                    let temporary = gio::CancellableFuture::new(spool(&clipboard, mime), transfer.clone()).await.map_err(|_| "Image import cancelled")??;
                    let control = cancelled.clone();
                    gio::spawn_blocking(move || read_sources(vec![temporary.0.clone()], true, control)).await.map_err(|_| "Image reader failed")?
                }.await;
            }
            result?
        } else {
            let control = cancelled.clone();
            gio::spawn_blocking(move || read_sources(paths, false, control)).await.map_err(|_| "Image reader failed")??
        };
        Ok::<_, String>(sources)
    }
    .await;
    dialog.disconnect(signal);
    if cancelled.load(Ordering::Acquire) {
        // AlertDialog already dismisses itself when Cancel/close responds.
        return Ok(false);
    }
    dialog.close();
    let sources = result?;
    let mut interpreted = layer_ui::ImageImportBatch::new(policy, working, Default::default());
    for (name, source) in sources {
        let Some(source) = super::open::interpret(w, source, policy).await? else { return Ok(false); };
        interpreted.append(name, source, false)?;
    }
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
    let sources = interpreted.take_sources(false)?;
    if mode == Some(layer_ui::PasteMode::NewImage) {
        session.validate_image_placement(&context)?;
        drop(gpu);
        let localization = w.localization();
        let project = gio::spawn_blocking(move || layer_ui::clipboard_document(sources, policy, &localization)).await.map_err(|_| "The paste failed")??;
        w.documents.enqueue_imported(w, layer_ui::ImportedDocument::new(project, layer_ui::ImportSource::Photo), None);
        return Ok(true);
    }
    match mode {
        Some(mode) => session.paste_layer_sources(sources, mode, &context)?,
        None => {
            session.validate_image_placement(&context)?;
            session.place_layer_sources(sources, context.center, context.destination)?;
        }
    }
    drop(gpu);
    w.wake();
    Ok(true)
}

/// One worker and one aggregate source allowance for the entire batch. Failure
/// drops every prepared source before any provisional layer can be published.
fn read_sources(
    paths: Vec<PathBuf>, paste: bool, cancelled: Arc<AtomicBool>,
) -> Result<Vec<(String, layer_core::color::source::SourceImage)>, String> {
    if paths.is_empty() { return Err("No images to import".into()); }
    let mut images = layer_ui::ImageImportBatch::new(Default::default(), layer_core::color::RgbSpace::ProPhoto, Default::default());
    for path in paths {
        if cancelled.load(Ordering::Acquire) { return Err("Image import cancelled".into()); }
        let name = if paste { "Clipboard image".into() } else {
            path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
        };
        let reader = cancellable_file(&path, cancelled.clone()).map_err(|e| e.to_string())?;
        images.read(BufReader::new(reader), &name, &cancelled)
            .map_err(|e| format!("{name}: {e}. No images were imported."))?;
    }
    if cancelled.load(Ordering::Acquire) { return Err("Image import cancelled".into()); }
    images.take_sources(cancelled.load(Ordering::Acquire))
}
