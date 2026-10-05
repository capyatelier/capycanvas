use super::*;
use layer_core::authored::{ImageInterpolation, OccurrenceContent};

fn ready(w: &Rc<Workspace>, presentations: usize) {
    until(|| !w.documents.has_pending_open() && !w.documents.changing.get()
        && !w.documents.paused.get() && w.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            let stats = engine.backend().stats.lock().unwrap();
            engine.backend().startup.complete && engine.backend().frames_idle()
                && !engine.has_pending_document_edits()
                && stats.presented.iter().filter(|sample| sample[3] == 1).count() >= presentations
                && stats.source_transfers.iter().any(|sample| sample[2] > 0)
        }), "object rendering progresses without input");
    pump(100);
    assert!(state(w).host_error.is_none(), "{:?}", state(w).host_error);
}

fn pixels(w: &Rc<Workspace>, id: u64) -> layer_render::ReadbackImage {
    ui_session(w).engine().backend().document_pixels(id).unwrap()
}

fn assert_objects(document: &layer_core::Document, nearest: bool) {
    assert!(document.artwork.paint.is_empty());
    assert!(document.scene().targets().next().is_none());
    assert_eq!(document.scene().order().len(), 1);
    let owner = document.scene().order()[0];
    assert!(matches!(document.scene().occurrence(owner).unwrap().content, OccurrenceContent::Objects(_)));
    let objects = document.artwork.objects.iter().map(|(_, _, object)| object).collect::<Vec<_>>();
    assert_eq!(objects.len(), 3);
    assert!(objects.iter().all(|object| object.visible && object.image.same_owner(&objects[0].image)));
    assert!(objects.iter().any(|object| object.affine.0[4].to_bits() == 16777217.125f64.to_bits()));
    if nearest { assert!(objects.iter().all(|object| object.interpolation == ImageInterpolation::Nearest)); }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
#[allow(deprecated)]
fn native_object_only_shared_images() {
    let app = native_test_app("art.capycanvas.ObjectFoundation");
    let directory = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap()).join("objects");
    std::fs::create_dir_all(&directory).unwrap();
    for (index, kind) in ["nearest", "builtin", "icc"].into_iter().enumerate() {
        eprintln!("Object-only {kind}: cold native startup");
        let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../layer-web/fixtures/shared-image-f64-{kind}.capy"));
        let mut document = open_native_document(std::fs::File::open(input).unwrap());
        let helpers = document.scene().order().iter().copied().filter(|h|
            !matches!(document.scene().occurrence(*h).unwrap().content, OccurrenceContent::Objects(_))).collect::<Vec<_>>();
        document.apply(document.delete_layers_edit(&helpers).unwrap()).unwrap();
        assert_objects(&document, kind == "nearest");
        let expected = document.clone();
        let w = Workspace::with_project(&app, Some((document, None)));
        apply_fixture_theme(&w);
        w.window.present();
        ready(&w, 2);
        if let Ok(theme) = std::env::var("CAPY_NATIVE_TEST_THEME") {
            assert_eq!(state(&w).settings.theme, Some(state(&w).theme));
            assert_eq!(format!("{:?}", state(&w).theme).to_lowercase(), theme);
        }
        let point = [200., expected.artwork.compositions.get(expected.artwork.root).unwrap().size[1] as f32 * 0.5];
        let before = pixels(&w, 100 + index as u64);
        assert!(before.bytes.chunks_exact(4).any(|pixel| pixel[3] > 0), "{kind} objects supply visible pixels");
        let shown = photo_edit::shown(&w, point);
        assert_live_artwork_eq(ui_session(&w).engine().document(), &expected);
        let owner = expected.scene().order()[0];
        w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Visibility {
            id: layer_ui::occurrence_token(owner), value: false,
        }});
        ready(&w, 2);
        let hidden = pixels(&w, 200 + index as u64);
        assert!(hidden.bytes.chunks_exact(4).all(|pixel| pixel[3] == 0), "{kind} has no paint pixels");
        assert_ne!(photo_edit::shown(&w, point), shown, "{kind} objects change the displayed canvas");
        for (command, visible) in [(CommandId::Undo, true), (CommandId::Redo, false), (CommandId::Undo, true)] {
            new_photo::invoke(&w, command);
            ready(&w, 2);
            let image = pixels(&w, 300 + index as u64);
            assert_eq!(image.bytes.as_slice(), if visible { before.bytes.as_slice() } else { hidden.bytes.as_slice() });
        }
        assert_live_artwork_eq(ui_session(&w).engine().document(), &expected);
        new_photo::invoke(&w, CommandId::SaveDocumentAs);
        let save = new_photo::chooser();
        save.set_current_folder(Some(&gtk::gio::File::for_path(&directory))).unwrap();
        let filename = format!("{kind}.capy");
        save.set_current_name(&filename);
        pump(100);
        save.response(gtk::ResponseType::Accept);
        new_photo::finish(&w);
        assert!(!state(&w).document_file.modified);
        let path = directory.join(filename);
        assert!(path.is_file());
        let tab = w.documents.selected();
        new_photo::invoke(&w, CommandId::OpenDocument);
        let open = new_photo::chooser();
        open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(100);
        open.response(gtk::ResponseType::Accept);
        new_photo::finish(&w);
        until(|| w.documents.selected() != tab, "saved objects reopen in a document tab");
        ready(&w, 2);
        let reopened = ui_session(&w).engine().document().clone();
        assert_objects(&reopened, kind == "nearest");
        assert_live_artwork_eq(&reopened, &expected);
        let image = reopened.artwork.objects.iter().next().unwrap().2;
        let original = expected.artwork.objects.iter().next().unwrap().2;
        assert_eq!(image.image.id(), original.image.id());
        assert_source_samples(&image.image, &original.image);
        assert_eq!(pixels(&w, 400 + index as u64).bytes, before.bytes);
        let checkpoint = ui_session(&w).engine().checkpoint();
        w.area.set_visible(false);
        pump(30);
        w.area.unrealize();
        assert!(w.gpu.borrow().is_some());
        w.area.set_visible(true);
        ready(&w, 1);
        assert_eq!(pixels(&w, 500 + index as u64).bytes, before.bytes);
        assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
        assert!(!state(&w).document_file.modified);
        w.recovery().capture(&w);
        glib::MainContext::default().block_on(w.recovery().drain());
        let recovered = glib::MainContext::default().block_on(w.recovery().read_snapshot()).unwrap().document().clone();
        assert_objects(&recovered, kind == "nearest");
        assert_live_artwork_eq(&recovered, &expected);
        let recovered_image = recovered.artwork.objects.iter().next().unwrap().2;
        assert_eq!(recovered_image.image.id(), original.image.id());
        assert_source_samples(&recovered_image.image, &original.image);
        eprintln!("Object-only {kind}: pixels, history, file, surface and recovery passed");
        w.window.destroy();
        pump(100);
    }
}
