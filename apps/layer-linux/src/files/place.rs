//! Retained file/clipboard image import. Transfer uses a bounded buffer and a
//! private temporary file; decode/CMM work runs outside the GTK owner.
use crate::workspace::Workspace;
use super::reader::CancelRead;
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
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn spool(clipboard: &gdk::Clipboard) -> Result<TemporaryImage, String> {
    // GDK prefers MIME types in order. Ask for lossless/deeper source formats
    // before JPEG, without downloading a display texture as untagged RGBA8.
    let mimes: Vec<_> = layer_color::photo::mime_types().collect();
    let (input, _) = clipboard
        .read_future(&mimes, glib::Priority::DEFAULT)
        .await
        .map_err(|e| format!("Copy a supported image ({}) to paste: {e}", layer_color::photo::format_names()))?;
    // Establish the unlink guard before the first cancellable write. Async
    // creation could finish after its future is dropped and orphan the file.
    // This is one private-file creation; all payload I/O stays asynchronous.
    let (file, stream) =
        gio::File::new_tmp(Some("capy-image-XXXXXX")).map_err(|e| e.to_string())?;
    let temporary = TemporaryImage(file.path().ok_or("Temporary image path is unavailable")?);
    let output = stream.output_stream();
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
    stream
        .close_future(glib::Priority::DEFAULT)
        .await
        .map_err(|e| e.to_string())?;
    input
        .close_future(glib::Priority::DEFAULT)
        .await
        .map_err(|e| e.to_string())?;
    Ok(temporary)
}

pub(super) async fn run(w: &Rc<Workspace>, paste: bool) -> Result<bool, String> {
    let incoming = if paste { None } else { w.image_drop.borrow_mut().take() };
    let (epoch, revision, target) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        incoming.as_ref().map_or_else(|| (
            session.state().document_file.epoch,
            session.engine().document().revision,
            session.engine().document().active_target(),
        ), |d| (d.epoch, d.revision, d.target))
    };
    let center = incoming.as_ref().and_then(|d| d.center);
    let destination = incoming.as_ref().and_then(|d| d.destination);
    let paths = if paste {
        Vec::new()
    } else if let Some(incoming) = incoming {
        incoming.files.into_iter().map(|file| file.path().ok_or("Drop images stored on this device"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&format!("Images ({})", layer_color::photo::format_names())));
        for suffix in layer_color::photo::extensions() {
            filter.add_suffix(suffix);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Import images as layers")
            .accept_label("Import")
            .filters(&filters)
            .default_filter(&filter)
            .build();
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
        .heading(if paste {
            "Pasting image…"
        } else {
            "Importing images…"
        })
        .body("Keeping the original color profile and bit depth.")
        .build();
    dialog.set_widget_name("image-import-progress");
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
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
        let temporary = if paste {
            Some(
                gio::CancellableFuture::new(spool(&w.window.clipboard()), transfer)
                    .await
                    .map_err(|_| "Image import cancelled")??,
            )
        } else {
            None
        };
        let paths = temporary.as_ref().map_or(paths, |t| vec![t.0.clone()]);
        let cancelled = cancelled.clone();
        // Keep the request reserved until this worker acknowledges cancellation;
        // repeated Cancel/Import cannot leave an unbounded set of decode workers.
        let sources = gio::spawn_blocking(move || read_sources(paths, paste, cancelled))
        .await
        .map_err(|_| "Image reader failed")??;
        drop(temporary);
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
    let policy = w.gpu.borrow().as_ref().ok_or("Canvas unavailable")?.session.state().settings.photo_open;
    let mut interpreted = Vec::with_capacity(sources.len());
    for (name, source) in sources {
        let Some(source) = super::open::interpret(w, source, policy).await? else { return Ok(false); };
        interpreted.push((name, source));
    }
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
    let document = session.engine().document();
    if session.state().document_file.epoch != epoch
        || document.revision != revision
        || document.active_target() != target
    {
        return Err(
            "The document or target layer changed while importing; import the image again".into(),
        );
    }
    session.place_layer_sources(interpreted, center, destination)?;
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
    let mut limits = layer_color::photo::DecodeLimits::default();
    let mut sources = Vec::with_capacity(paths.len());
    for path in paths {
        if cancelled.load(Ordering::Acquire) { return Err("Image import cancelled".into()); }
        let name = if paste { "Clipboard image".into() } else {
            path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
        };
        let photo = (|| {
            let reader = CancelRead::new(&path, cancelled.clone()).map_err(|e| e.to_string())?;
            layer_color::photo::read_photo_detailed_with_cancel(BufReader::new(reader), limits, &cancelled)
        })().map_err(|e| format!("{name}: {e}. No images were imported."))?;
        limits.source_bytes = limits.source_bytes.checked_sub(photo.source.resident_bytes())
            .ok_or("The image batch exceeds the source memory budget. No images were imported.")?;
        sources.push((photo.display_name(&name), photo.source));
    }
    if cancelled.load(Ordering::Acquire) { return Err("Image import cancelled".into()); }
    Ok(sources)
}
