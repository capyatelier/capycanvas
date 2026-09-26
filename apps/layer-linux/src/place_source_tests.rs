use super::new_photo::{chooser, finish, invoke, ready};
use super::*;
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};

pub(super) fn source() -> SourceImage {
    let mut builder = SourceBuilder::new(
        [128, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
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

pub(super) fn wait_layer_thumbnail(w: &Rc<Workspace>, id: u64) -> gtk::gdk::Texture {
    // A second window can own Sketch while Paint is leased by the first. Its
    // Layers panel is intentionally hidden until the user opens the drawer.
    if !w.layer_panel.root.is_mapped()
        && !w.drawers().iter().filter_map(|d| d.layers()).any(|v| v.root.is_mapped())
    {
        let layout = state(w).workspace.layout;
        let control = ToolbarControl::Panel { panel: Panel::Layers };
        let button = if let Some(entry) = layout.header.zones.iter().flatten()
            .find(|entry| entry.item == (layer_ui::HeaderItem::Tool { control }))
        {
            find_named(w.header.root.upcast_ref(), &format!("header-item-{}", entry.id))
                .and_then(|root| root.first_child()).unwrap().downcast::<gtk::Button>().unwrap()
        } else {
            let tile = layout.panels.iter().flat_map(|p| p.tiles())
                .find(|tile| tile.control == control).expect("Layers control in the test workspace");
            find_named(w.surface.upcast_ref(), &format!("tile-{}", tile.id))
                .unwrap().downcast::<gtk::Button>().unwrap()
        };
        click(&button);
    }
    let started = Instant::now();
    loop {
        pump(10);
        let state = state(w);
        let revision = state.layers.iter().find(|l| l.id == id).unwrap().paint_revision;
        let extra: Vec<_> = w.drawers().iter().filter_map(|d| d.layers()).collect();
        if let Some(texture) = w.layer_panel.preview_texture(id, revision, &extra) {
            assert_eq!([texture.width(), texture.height()], [32, 32]);
            return texture;
        }
        assert!(state.host_error.is_none(), "{:?}", state.host_error);
        if started.elapsed() >= Duration::from_secs(10) {
            if let Some(output) = std::env::var_os("LAYER_RASTER_UI_OUTPUT") {
                let output = std::path::PathBuf::from(output);
                std::fs::create_dir_all(&output).unwrap();
                super::new_photo::capture_ui(w, &output, &format!("thumbnail-timeout-{id}.png"));
            }
            panic!("visible layer {id} thumbnail revision {revision} timed out: {}", w.layer_panel.preview_debug(&extra));
        }
    }
}

#[test]
#[ignore = "private Wayland display, hardware GPU and LAYER_RASTER_FIXTURES"]
#[allow(deprecated)]
fn native_common_raster_open_import_and_paste() {
    native_raster_open_import_and_paste(&[("Profiled.bmp", "image/bmp"), ("Animation.gif", "image/gif"), ("Profiled.webp", "image/webp")]);
}

#[test]
#[ignore = "private Wayland display, GPU and LAYER_RASTER_FIXTURES"]
fn native_heif_avif_open_import_and_paste() {
    native_raster_open_import_and_paste(&[("Photo.heic", "image/heic"), ("P3.avif", "image/avif"), ("ICC.avif", "image/avif")]);
}

#[allow(deprecated)]
fn native_raster_open_import_and_paste(cases: &[(&str, &str)]) {
    let directory = std::path::PathBuf::from(std::env::var_os("LAYER_RASTER_FIXTURES").expect("codec fixtures"));
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.CommonRaster");
    let w = Workspace::with_project(&app, Some((new_drawing(256, 128).unwrap(), None)));
    let opened = Rc::new(RefCell::new(None));
    let result = opened.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, location, _| {
        result.replace(Some((project, location)));
    }));
    w.window.present();
    ready(&w);
    for &(name, mime) in cases {
        let path = directory.join(name);
        let bytes = std::fs::read(&path).unwrap();
        let photo = layer_color::photo::read_photo_detailed(std::io::Cursor::new(&bytes), Default::default()).unwrap();
        invoke(&w, CommandId::ImportImage);
        let file = chooser();
        file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(100);
        file.response(gtk::ResponseType::Accept);
        finish(&w);
        ready(&w);
        source_is(&w, &photo.source);
        {
            let gpu = w.gpu.borrow();
            let document = gpu.as_ref().unwrap().session.engine().document();
            assert_eq!(document.layer(document.active_layer).unwrap().name.contains("first frame"), photo.first_frame);
            assert_eq!(document.layer(document.active_layer).unwrap().name.contains("primary image"), photo.primary_image);
        }
        invoke(&w, CommandId::ApplyTransform);
        ready(&w);
        let active = w.gpu.borrow().as_ref().unwrap().session.engine().document().active_layer.0;
        wait_layer_thumbnail(&w, active);
        let saved = snapshot(&w);
        let reopened = layer_core::Project::read(std::io::Cursor::new(saved), Default::default()).unwrap();
        assert_eq!(reopened.document.layers[0].source.as_deref(), Some(&photo.source));
        invoke(&w, CommandId::Undo);
        ready(&w);
        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().layers.len(), 2);

        let provider = gtk::gdk::ContentProvider::for_bytes(mime, &glib::Bytes::from_owned(bytes));
        w.window.clipboard().set_content(Some(&provider)).unwrap();
        invoke(&w, CommandId::PasteImage);
        finish(&w);
        ready(&w);
        source_is(&w, &photo.source);
        invoke(&w, CommandId::CancelTransform);
        ready(&w);
        assert_eq!(w.gpu.borrow().as_ref().unwrap().session.engine().document().layers.len(), 2);

        invoke(&w, CommandId::OpenDocument);
        let file = chooser();
        file.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(100);
        file.response(gtk::ResponseType::Accept);
        finish(&w);
        let (project, location) = opened.borrow_mut().take().expect("photo Open publishes a document");
        assert!(location.is_none(), "Open must not make the original image the master target");
        assert_eq!([project.document.width, project.document.height], photo.source.extent);
        assert_eq!(project.document.layers[0].source.as_deref(), Some(&photo.source));
        assert_eq!(project.document.layers[0].name.contains("first frame"), photo.first_frame);
        assert_eq!(project.document.layers[0].name.contains("primary image"), photo.primary_image);
        let photo_id = project.document.layers[0].id.0;
        let photo_window = Workspace::with_project(&app, Some((project, location)));
        let shown = Instant::now();
        photo_window.window.present();
        ready(&photo_window);
        wait_layer_thumbnail(&photo_window, photo_id);
        println!("{name}: source-sized Open canvas and visible layer thumbnail ready in {:.2} ms", shown.elapsed().as_secs_f64() * 1000.);
        if let Some(output) = std::env::var_os("LAYER_RASTER_UI_OUTPUT") {
            let output = std::path::PathBuf::from(output);
            std::fs::create_dir_all(&output).unwrap();
            super::new_photo::capture_ui(&photo_window, &output, &format!("{name}.png"));
        }
        photo_window.window.destroy();
        println!("{name}: native Open/Import/Paste and retained save/history passed");
    }
    w.window.destroy();
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
    assert!(state(&w).commands.iter().any(|c| c.id == CommandId::ApplyTransform && c.enabled));
    invoke(&w, CommandId::ApplyTransform);
    ready(&w);
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
    layer_color::photo::write_png(
        &mut preview,
        &layer_core::color::source::rgba8_source([1, 1], |_, _| [0, 0, 0, 255]),
    )
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
    invoke(&w, CommandId::ApplyTransform);
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
            .is_some_and(|e| e.contains("Copy a supported image"))
    );
    assert_eq!(snapshot(&w), saved);
    w.window.destroy();
    pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_unsupported_hdr_and_multiple_picture_inputs_preserve_the_document() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.SdrInputPolicy");
    let w = Workspace::with_project(&app, Some((new_drawing(96, 64).unwrap(), None)));
    w.window.present();
    ready(&w);
    let before = snapshot(&w);
    let mut builder = SourceBuilder::new(
        [3, 2],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for _ in 0..2 {
        builder.push_row(&[100; 9]).unwrap();
    }
    let mut jpeg = Vec::new();
    layer_color::photo::write_jpeg(&mut jpeg, &builder.finish().unwrap(), 95).unwrap();
    let directory = std::env::temp_dir().join(format!("capy-input-policy-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    for (command, marker, reason) in [
        (
            CommandId::OpenDocument,
            b"urn:iso:std:iso:ts:21496:-1\0".as_slice(),
            "JPEG gain map has no MPF directory",
        ),
        (
            CommandId::ImportImage,
            b"MPF\0".as_slice(),
            "Invalid JPEG MPF directory",
        ),
        (
            CommandId::PasteImage,
            b"urn:iso:std:iso:ts:21496:-1\0".as_slice(),
            "JPEG gain map has no MPF directory",
        ),
    ] {
        // A valid ordinary JPEG with a recognized richer-container declaration.
        // This tests rejection, not gain-map reconstruction or MPF conformance.
        let mut bytes = jpeg[..2].to_vec();
        bytes.extend_from_slice(&[0xff, 0xe2]);
        bytes.extend_from_slice(&((marker.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(marker);
        bytes.extend_from_slice(&jpeg[2..]);
        if command == CommandId::PasteImage {
            let provider =
                gtk::gdk::ContentProvider::for_bytes("image/jpeg", &glib::Bytes::from_owned(bytes));
            w.window.clipboard().set_content(Some(&provider)).unwrap();
            invoke(&w, command);
        } else {
            let path = directory.join("unsupported.jpg");
            std::fs::write(&path, &bytes).unwrap();
            invoke(&w, command);
            let file = chooser();
            file.set_file(&gtk::gio::File::for_path(path)).unwrap();
            pump(200);
            file.response(gtk::ResponseType::Accept);
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while state(&w).document_file.busy || !state(&w).requests.is_empty() {
            pump(20);
            assert!(Instant::now() < deadline, "input rejection");
        }
        assert!(
            state(&w)
                .host_error
                .as_deref()
                .is_some_and(|e| e.contains(reason)),
            "{:?}",
            state(&w).host_error
        );
        assert_eq!(snapshot(&w), before);
    }
    w.window.destroy();
    pump(100);
    std::fs::remove_dir_all(directory).unwrap();
}
