//! The pixel clipboard on GTK. Copies are composed on a worker and kept for
//! the whole application; the system clipboard receives a PNG and a private
//! nonce, so pasting this application's own copy reads the full-depth clip.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_render_wgpu::snapshot::CaptureControl;
use layer_ui::{PasteMode, PixelClip};
use std::{cell::RefCell, rc::Rc};

pub(crate) const CLIP_MIME: &str = "application/x-capycanvas-clip";

thread_local! {
    static CLIP: RefCell<Option<PixelClip>> = const { RefCell::new(None) };
}

pub(crate) fn current() -> Option<PixelClip> {
    CLIP.with_borrow(Clone::clone)
}

pub(super) async fn copy(w: &Rc<Workspace>, id: u32, request: &layer_ui::DocumentRequest) -> Result<bool, String> {
    let task = {
        let gpu = w.snapshot_gpu()?;
        let mut owner = w.gpu.borrow_mut();
        let session = &mut owner.as_mut().ok_or("Canvas unavailable")?.session;
        layer_host::clipboard::ClipTask::new(session, id, gpu)?
    };
    let details = task.capture_details();
    let progress = details.large.then(|| {
        let dialog = adw::AlertDialog::builder().heading(request.title(&w.localization()).as_ref()).build();
        dialog.set_widget_name("clipboard-progress");
        dialog.add_response("cancel", &layer_ui::CommonCopy::new(&w.localization()).cancel);
        dialog.set_close_response("cancel");
        dialog
    });
    if let Some(dialog) = &progress {
        let weak = dialog.downgrade(); let request = request.clone();
        w.on_localization(move |localization| {
            let Some(dialog) = weak.upgrade() else { return false };
            dialog.set_heading(Some(&request.title(localization)));
            dialog.set_response_label("cancel", &layer_ui::CommonCopy::new(localization).cancel);
            true
        });
    }
    let control = CaptureControl::default();
    let signal = progress.as_ref().map(|dialog| {
        let control = control.clone();
        dialog.connect_response(Some("cancel"), move |_, _| control.cancel())
    });
    if let Some(dialog) = &progress {
        dialog.present(Some(&w.window));
    }
    let nonce = glib::uuid_string_random().to_string();
    let worker = control.clone();
    let result = gio::spawn_blocking(move || task.run(nonce, worker)).await.map_err(|_| "The copy failed".to_string());
    if let (Some(dialog), Some(signal)) = (&progress, signal) {
        dialog.disconnect(signal);
        if !control.is_cancelled() {
            dialog.close();
        }
    }
    if control.is_cancelled() {
        return Ok(false);
    }
    publish(w, result??)?;

    Ok(true)
}

/// Keep the clip for every window and offer its PNG and nonce to other applications.
fn publish(w: &Workspace, clip: PixelClip) -> Result<(), String> {
    let provider = gdk::ContentProvider::new_union(&[
        gdk::ContentProvider::for_bytes("image/png", &glib::Bytes::from(&clip.png[..])),
        gdk::ContentProvider::for_bytes(CLIP_MIME, &glib::Bytes::from(clip.nonce.as_bytes())),
    ]);
    CLIP.set(Some(clip));
    w.window.clipboard().set_content(Some(&provider)).map_err(|e| e.to_string())
}

/// The nonce the system clipboard carries, when it holds a copy from this application.
async fn clipboard_nonce(clipboard: &gdk::Clipboard) -> Option<String> {
    if !clipboard.formats().contain_mime_type(CLIP_MIME) {
        return None;
    }
    let (input, _) = clipboard.read_future(&[CLIP_MIME], glib::Priority::DEFAULT).await.ok()?;
    let bytes = input.read_bytes_future(256, glib::Priority::DEFAULT).await.ok()?;
    String::from_utf8(bytes.to_vec()).ok()
}

pub(super) async fn paste(w: &Rc<Workspace>, mode: PasteMode) -> Result<bool, String> {
    let nonce = clipboard_nonce(&w.window.clipboard()).await;
    if let Some(clip) = current().filter(|clip| nonce.as_deref() == Some(clip.nonce.as_str())) {
        let mut owner = w.gpu.borrow_mut();
        owner.as_mut().ok_or("Canvas unavailable")?.session.paste_clip(&clip, mode)?;
        drop(owner);
        w.wake();
        return Ok(true);
    }
    super::place::run(w, Some(mode)).await
}
