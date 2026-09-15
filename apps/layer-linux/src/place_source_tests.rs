use super::new_photo::{chooser, finish, invoke, ready};
use super::*;
use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace, source::*};

pub(super) fn source() -> SourceImage {
    let mut builder = SourceBuilder::new(
        [128, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U16,
            profile: ColorProfile::Icc(
                layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3))
                    .unwrap()
                    .into(),
            ),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for y in 0..64 {
        let row: Vec<u8> = (0..128)
            .flat_map(|x| [65535u16, (x * 37) as u16, (y * 311) as u16, 65535])
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}
pub(super) fn snapshot(w: &Rc<Workspace>) -> Vec<u8> {
    let project = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .capture_project_recovery()
        .unwrap();
    let mut bytes = Vec::new();
    project.write(&mut bytes).unwrap();
    bytes
}
fn source_is(w: &Rc<Workspace>, expected: &SourceImage) {
    let gpu = w.gpu.borrow();
    let document = gpu.as_ref().unwrap().session.engine().document();
    assert_eq!(document.color, Default::default());
    let layer = document.layer(document.active_layer).unwrap();
    assert_eq!(layer.source.as_deref(), Some(expected));
    assert!(layer.asset.is_none());
    assert!(
        layer.raster.is_empty(),
        "retained import must not quantize to canvas precision"
    );
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_profiled_place_paste_and_source_history() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.PlaceSource");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 128).unwrap(), None)));
    w.window.present();
    ready(&w);
    let source = source();
    let output = std::path::Path::new("../../artifacts/color-m2/place-source-ui");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let path = output.join(format!("P3 reference-{}.png", std::process::id()));
    layer_color::photo::write_png(std::fs::File::create(&path).unwrap(), &source).unwrap();
    let original = snapshot(&w);
    invoke(&w, CommandId::ImportImage);
    let file = chooser();
    file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
    pump(200);
    file.response(gtk::ResponseType::Accept);
    finish(&w);
    ready(&w);
    source_is(&w, &source);
    let placed = snapshot(&w);
    let before = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9891))
        .unwrap();
    assert!(
        state(&w)
            .commands
            .iter()
            .any(|c| c.id == CommandId::ScaleRotate && c.enabled)
    );
    invoke(&w, CommandId::ScaleRotate);
    invoke(&w, CommandId::CancelTransform);
    ready(&w);
    assert_eq!(snapshot(&w), placed);
    invoke(&w, CommandId::ClearLayer);
    ready(&w);
    assert!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers[0]
            .source
            .is_none()
    );
    let cleared = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9892))
        .unwrap();
    assert_ne!(cleared.bytes, before.bytes);
    invoke(&w, CommandId::Undo);
    ready(&w);
    source_is(&w, &source);
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&w, 9893))
            .unwrap()
            .bytes,
        before.bytes
    );
    invoke(&w, CommandId::Undo);
    ready(&w);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers
            .len(),
        2
    );
    // Revision/counter identities can advance through history; source payload
    // restoration remains exact and redo is a single layer-import history step.
    assert_ne!(snapshot(&w), original);
    invoke(&w, CommandId::Redo);
    ready(&w);
    source_is(&w, &source);

    let mut tiff = std::io::Cursor::new(Vec::new());
    layer_color::photo::write_tiff(&mut tiff, &source).unwrap();
    let tiff = tiff.into_inner();
    // A clipboard can offer a rich TIFF and a lower-depth PNG rendition.
    // The same retained import route must prefer the TIFF source.
    let mut preview = Vec::new();
    layer_render::ReadbackImage {
        request_id: 0,
        width: 1,
        height: 1,
        stride: 4,
        bytes: vec![0, 0, 0, 255],
    }
    .write_png(&mut preview)
    .unwrap();
    let providers = [
        gtk::gdk::ContentProvider::for_bytes("image/png", &glib::Bytes::from_owned(preview)),
        gtk::gdk::ContentProvider::for_bytes("image/tiff", &glib::Bytes::from_owned(tiff.clone())),
    ];
    let provider = gtk::gdk::ContentProvider::new_union(&providers);
    w.window.clipboard().set_content(Some(&provider)).unwrap();
    invoke(&w, CommandId::PasteImage);
    finish(&w);
    ready(&w);
    source_is(&w, &source);
    assert_eq!(
        w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .document()
            .layers
            .len(),
        4
    );
    let saved = snapshot(&w);
    let reopened =
        layer_core::Project::read(std::io::Cursor::new(saved.clone()), Default::default()).unwrap();
    assert_eq!(reopened.document.layers[0].source.as_deref(), Some(&source));
    assert_eq!(reopened.document.layers[1].source.as_deref(), Some(&source));
    let restored = Workspace::with_project(&app, Some((reopened, None)));
    restored.window.present();
    ready(&restored);
    let expected = glib::MainContext::default()
        .block_on(read_canvas_pixels(&w, 9894))
        .unwrap();
    assert_eq!(
        glib::MainContext::default()
            .block_on(read_canvas_pixels(&restored, 9895))
            .unwrap()
            .bytes,
        expected.bytes
    );
    restored.window.destroy();
    w.window.present();
    ready(&w);

    // TIFF permits unused trailing storage. Keep a transfer pending long enough
    // to cancel through the visible native progress dialog, without decoding it.
    let mut padded = tiff;
    padded.resize(32 * 1024 * 1024, 0);
    let provider =
        gtk::gdk::ContentProvider::for_bytes("image/tiff", &glib::Bytes::from_owned(padded));
    w.window.clipboard().set_content(Some(&provider)).unwrap();
    w.dispatch(UiAction::Invoke {
        command: CommandId::PasteImage,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while w.window.visible_dialog().is_none() {
        pump(1);
        assert!(Instant::now() < deadline);
    }
    let dialog = w.window.visible_dialog().unwrap();
    click(&find_button(dialog.upcast_ref(), "Cancel").unwrap());
    finish(&w);
    assert_eq!(
        snapshot(&w),
        saved,
        "cancelled paste changes no document content"
    );
    w.window.clipboard().set_text("Ordinary text");
    w.dispatch(UiAction::Invoke {
        command: CommandId::PasteImage,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while state(&w).document_file.busy {
        pump(20);
        assert!(Instant::now() < deadline);
    }
    assert!(
        state(&w)
            .host_error
            .as_deref()
            .is_some_and(|e| e.contains("Copy a PNG, TIFF or JPEG"))
    );
    assert_eq!(snapshot(&w), saved);
    w.window.destroy();
    pump(100);
}
