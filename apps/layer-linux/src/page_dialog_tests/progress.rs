use super::*;

fn pipe(name: &str) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap()).join(name);
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0, "{}", std::io::Error::last_os_error());
        path
    })).unwrap()
}

pub(super) fn opening(d: &mut Driver) {
    let _windows = photo_launcher(d);
    for (name, import, widget) in [("document-opening-progress", false, "document-open-progress"),
        ("image-import-progress", true, "image-import-progress")] {
        let path = pipe(&format!("{name}.png"));
        let original = d.w.documents.selected();
        let count = d.w.documents.len();
        let before = super::super::place_source::authored_snapshot(&d.w);
        let file = gtk::gio::File::for_path(&path);
        if import {
            assert!(crate::files::drop::receive(&d.w, vec![file], None, None));
        } else {
            crate::files::launch::open_in(&d.w, vec![file]);
        }
        let (dialog, _) = dialog_frame_named(d, widget);
        capture(d, name);
        respond(d, &dialog, "cancel");
        let held = path.clone();
        let writer = glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
            std::fs::OpenOptions::new().read(true).write(true).open(&held).unwrap()
        })).unwrap();
        until(|| !d.w.documents.loading.get() && !d.w.servicing.get()
            && d.w.visible_dialog().is_none(), "Cancel acknowledges the held image provider");
        assert_eq!(d.w.documents.selected(), original);
        assert_eq!(d.w.documents.len(), count);
        assert_eq!(super::super::place_source::authored_snapshot(&d.w), before,
            "cancelled image preparation preserves authored data");
        glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
            drop(writer);
            std::fs::remove_file(path).unwrap();
        })).unwrap();
    }
}

fn cancel_held(d: &mut Driver, name: &str, widget: &str, mut hold: super::super::providers::Hold) {
    until(|| hold.started(), "the selected native provider starts");
    let (dialog, _) = dialog_frame_named(d, widget);
    capture(d, name);
    respond(d, &dialog, "cancel");
    drop(hold);
    until(|| !d.w.servicing.get() && d.w.visible_dialog().is_none(), "Cancel waits for native provider acknowledgement");
}

pub(super) fn workers(d: &mut Driver) {
    use super::super::providers::{hold, Provider};
    let before = super::super::place_source::authored_snapshot(&d.w);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ChangeBitDepth });
    let (dialog, frame) = dialog_frame(d);
    let depth = named::<adw::ComboRow>(&frame, "document-color-depth");
    depth.set_selected((depth.selected() + 1) % depth.model().unwrap().n_items());
    until(|| ["color-preview-before", "color-preview-after"].iter().all(|name| find_named(&frame, name)
        .and_then(|widget| widget.downcast::<gtk::Picture>().ok()).is_some_and(|picture| picture.paintable().is_some())),
        "the selected depth has both real comparison previews");
    let gate = hold(Provider::Color);
    respond(d, &dialog, "apply");
    cancel_held(d, "document-color-progress", "document-color-progress", gate);
    assert_eq!(super::super::place_source::authored_snapshot(&d.w), before);

    let path = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap()).join("held-export.png");
    crate::files::choose_next_save(path.clone());
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (_, frame) = dialog_frame(d);
    let gate = hold(Provider::Export);
    let confirm = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap();
    assert!(confirm.is_mapped() && confirm.is_ancestor(&frame));
    d.click(&confirm);
    cancel_held(d, "export-progress", "export-progress", gate);
    let exists = glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || path.exists())).unwrap();
    assert!(!exists, "Cancel prevents publishing the owned export");
    assert_eq!(super::super::place_source::authored_snapshot(&d.w), before);

    let original = d.w.documents.selected();
    let project = new_drawing(2048, 1025, &d.w.localization()).unwrap();
    glib::MainContext::default().block_on(d.w.documents.open(&d.w, (project, None))).unwrap();
    super::super::new_photo::ready(&d.w);
    let temporary = d.w.documents.selected();
    let gate = hold(Provider::Clipboard);
    d.w.dispatch(UiAction::Invoke { command: CommandId::CopyMerged });
    cancel_held(d, "clipboard-progress", "clipboard-progress", gate);
    assert_eq!(d.w.documents.selected(), temporary);
    d.w.documents.select(&d.w, temporary, true);
    until(|| d.w.documents.selected() == original && !d.w.documents.changing.get(), "the unchanged clipboard fixture closes");
}

#[allow(deprecated)]
pub(super) fn recording(d: &mut Driver) {
    let button = named::<gtk::Button>(d.w.effects.stats.upcast_ref(), "stroke-recording");
    button.emit_clicked();
    assert!(ui_session_mut(&d.w).stroke_recording().status().recording);
    let from = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.45, 0.4]);
    let to = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.55, 0.45]);
    d.input.perform(serde_json::json!([{"pen":"down","point":from},{"pen":"move","point":to},{"pen":"up"},{"pen":"leave"}]));
    button.emit_clicked();
    let chooser = super::super::canvas_bar_tests::until_some(|| gtk::Window::list_toplevels().into_iter()
        .filter_map(|window| window.downcast::<gtk::FileChooserDialog>().ok())
        .find(|window| window.is_visible() && window.transient_for().as_ref() == Some(d.w.window.upcast_ref())),
        "the recording Stop button opens its owned save chooser");
    chooser.response(gtk::ResponseType::Cancel);
    until(|| !chooser.is_visible() && button.is_sensitive() && ui_session_mut(&d.w).stroke_recording().status().ready,
        "Cancel acknowledges the recording chooser and retains its data");
    let bytes = {
        let mut session = ui_session_mut(&d.w);
        let mut recorder = session.stroke_recording();
        assert!(recorder.status().raw_events > 0);
        recorder.snapshot().unwrap()
    };
    let parent = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap()).join("recording-parent-file");
    let parent = glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        std::fs::write(&parent, b"owned nondirectory").unwrap();
        parent
    })).unwrap();
    glib::MainContext::default().block_on(d.w.effects.save_recording_file(&d.w, gtk::gio::File::for_path(parent.join("stroke.capystrokes"))));
    let (dialog, _) = dialog_frame(d);
    assert!(!dialog.downcast_ref::<adw::AlertDialog>().unwrap().body().is_empty());
    capture(d, "stroke-recording-failure");
    close(d, &dialog);
    assert_eq!(ui_session_mut(&d.w).stroke_recording().snapshot().unwrap(), bytes, "a failed delivery retains the recording");
    let path = parent.with_file_name("recording-retry.capystrokes");
    glib::MainContext::default().block_on(d.w.effects.save_recording_file(&d.w, gtk::gio::File::for_path(&path)));
    assert!(!ui_session_mut(&d.w).stroke_recording().status().ready, "a successful retry clears only the delivered recording");
    glib::MainContext::default().block_on(gtk::gio::spawn_blocking(move || {
        let decoded = layer_engine::recording::read(std::fs::File::open(&path).unwrap()).unwrap();
        assert!(decoded.iter().any(|record| matches!(record, layer_engine::recording::Record::Raw { .. })));
        std::fs::remove_file(parent).unwrap();
        std::fs::remove_file(path).unwrap();
    })).unwrap();
}

fn respond(d: &mut Driver, dialog: &adw::Dialog, id: &str) {
    let alert = dialog.downcast_ref::<adw::AlertDialog>().unwrap();
    assert!(alert.is_response_enabled(id));
    let frame = crate::page_dialog::parent(dialog);
    let button = find_button(&frame, &alert.response_label(id)).unwrap();
    d.click(button.upcast_ref());
}
