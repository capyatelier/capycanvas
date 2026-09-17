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
const IMAGE_MIMES: [&str; 3] = ["image/tiff", "image/png", "image/jpeg"];

struct TemporaryImage(PathBuf);
impl Drop for TemporaryImage {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

async fn spool(clipboard: &gdk::Clipboard) -> Result<TemporaryImage, String> {
    // GDK prefers MIME types in order. Ask for lossless/deeper source formats
    // before JPEG, without downloading a display texture as untagged RGBA8.
    let (input, _) = clipboard
        .read_future(&IMAGE_MIMES, glib::Priority::DEFAULT)
        .await
        .map_err(|e| format!("Copy a PNG, TIFF or JPEG image to paste: {e}"))?;
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
    let (epoch, revision, target) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (
            session.state().document_file.epoch,
            session.engine().document().revision,
            session.engine().document().active_target(),
        )
    };
    let path = if paste {
        None
    } else {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PNG, JPEG and TIFF images"));
        for suffix in ["png", "jpg", "jpeg", "tif", "tiff"] {
            filter.add_suffix(suffix);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Import image as layer")
            .accept_label("Import")
            .filters(&filters)
            .default_filter(&filter)
            .build();
        match super::chooser::open(&dialog, &w.window, super::chooser::Folder::Artwork).await {
            Ok(file) => Some(file.path().ok_or("Choose an image on this device")?),
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
            "Importing image…"
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
        let path = path
            .or_else(|| temporary.as_ref().map(|t| t.0.clone()))
            .ok_or("No image to import")?;
        let name = if paste {
            "Clipboard image".into()
        } else {
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect::<String>();
            if name.trim().is_empty() {
                "Image".into()
            } else {
                name
            }
        };
        let cancelled = cancelled.clone();
        // Keep the request reserved until this worker acknowledges cancellation;
        // repeated Cancel/Import cannot leave an unbounded set of decode workers.
        let source = gio::spawn_blocking(move || {
            let reader = CancelRead::new(&path, cancelled).map_err(|e| e.to_string())?;
            layer_color::photo::read_photo(BufReader::new(reader), Default::default())
        })
        .await
        .map_err(|_| "Image reader failed")??;
        drop(temporary);
        Ok::<_, String>((name, source))
    }
    .await;
    dialog.disconnect(signal);
    if cancelled.load(Ordering::Acquire) {
        // AlertDialog already dismisses itself when Cancel/close responds.
        return Ok(false);
    }
    dialog.close();
    let (name, source) = result?;
    let policy = w.gpu.borrow().as_ref().ok_or("Canvas unavailable")?.session.state().settings.photo_open;
    let Some(source) = super::open::interpret(w, source, policy).await? else { return Ok(false); };
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
    session.import_layer_source(&name, source)?;
    drop(gpu);
    w.wake();
    Ok(true)
}
