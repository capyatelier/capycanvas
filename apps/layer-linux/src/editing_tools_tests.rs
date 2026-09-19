//! Real Wayland contacts, native paint, history and GTK Save/Open.
use super::*;
use super::new_photo::{capture_ui, chooser, finish, invoke, ready};
use layer_core::color::{DocumentColor, RgbColor, RgbSpace, SampleDepth};
use layer_core::raster::{RasterRevision, TileKey};
use std::collections::BTreeMap;

fn raster(w: &Workspace) -> RasterRevision {
    let gpu = w.gpu.borrow();
    let document = gpu.as_ref().unwrap().session.engine().document();
    document.layer(document.active_layer).unwrap().raster.clone()
}
fn pixels(root: &RasterRevision) -> BTreeMap<TileKey, Vec<u8>> {
    root.wait_data().unwrap().tiles.iter().map(|(key, tile)| {
        (*key, tile.wait_backing().unwrap().decode().unwrap())
    }).collect()
}
fn healthy(w: &Workspace) {
    let gpu = w.gpu.borrow();
    let session = &gpu.as_ref().unwrap().session;
    assert!(!session.rendering_suspended(), "canvas stopped: {}", w.status.text());
    assert!(session.state().host_error.is_none(), "{:?}", session.state().host_error);
    assert!(!w.status.is_visible(), "{}", w.status.text());
}
fn committed(w: &Rc<Workspace>, before: &RasterRevision) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(10); healthy(w);
        let current = raster(w);
        if current != *before && current.host_backed() { break; }
        assert!(Instant::now() < deadline, "contact did not commit pixels");
    }
    ready(w);
}

struct Pointer { dir: std::path::PathBuf, step: usize }
impl Pointer {
    fn new() -> Self {
        let dir = std::path::PathBuf::from(std::env::var_os("LAYER_NATIVE_INPUT_DIR").unwrap());
        std::fs::write(dir.join("ready"), "ready").unwrap();
        Self { dir, step: 0 }
    }
    fn stroke(&mut self, w: &Workspace, from: [f32; 2], to: [f32; 2]) {
        let m = state(w).camera.document_to_surface();
        let scale = w.area.scale_factor() as f32;
        let at = |p: [f32; 2]| {
            let p = gtk::graphene::Point::new(
                (m[0]*p[0] + m[2]*p[1] + m[4])/scale,
                (m[1]*p[0] + m[3]*p[1] + m[5])/scale,
            );
            let p = w.area.compute_point(&w.window, &p).unwrap();
            [p.x(), p.y()]
        };
        let events = serde_json::json!([
            {"point": at(from), "down": true}, {"point": at(to)}, {"down": false}
        ]);
        let path = self.dir.join(format!("step-{}.json", self.step));
        std::fs::write(path.with_extension("tmp"), serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(path.with_extension("tmp"), path).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.dir.join(format!("done-{}", self.step)).exists() {
            pump(2); assert!(Instant::now() < deadline, "native pointer step {}", self.step);
        }
        self.step += 1; pump(100);
    }
}

#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_portable_paint_pointer_workflow"]
#[allow(deprecated)]
fn native_portable_paint_pointer_workflow() {
    let app = native_test_app("art.capycanvas.EditingTools");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("capy-editing-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    let mut pointer = Pointer::new();
    for (space, depth) in [
        (RgbSpace::Srgb, SampleDepth::U8),
        (RgbSpace::Srgb, SampleDepth::U16),
        (RgbSpace::DisplayP3, SampleDepth::F16),
    ] {
        let mut project = new_drawing(384, 256).unwrap();
        project.document.color = DocumentColor { space, depth };
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.maximize(); w.window.present(); ready(&w);
        invoke(&w, CommandId::FitCanvas);
        let foreground = if depth.is_float() {
            RgbColor::from_linear(RgbSpace::DisplayP3, [8., -0.125, 2., 0.625]).unwrap()
        } else { RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.625]).unwrap() };
        w.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition { color: foreground } });
        w.dispatch(UiAction::SetBrushOpacity { value: 1. });
        let empty = pixels(&raster(&w));
        // The old bucket validation error arrived from frame() and stopped the
        // renderer. Real GTK contacts must complete asynchronous publication.
        invoke(&w, CommandId::Fill);
        let before = raster(&w);
        pointer.stroke(&w, [96., 128.], [96., 128.]); committed(&w, &before);
        let filled = pixels(&raster(&w)); assert_ne!(filled, empty);
        assert!(filled.keys().any(|k| k.coordinate[0] == 1), "fill crosses page boundary");
        capture_ui(&w, &output, &format!("{depth:?}-bucket.png"));
        invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), empty);
        invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), filled); healthy(&w);
        // Each gradient contact must change stored pixels and be one exactly
        // reversible history edit, including transparent and radial modes.
        for (radial, transparent) in [(false, false), (false, true), (true, false), (true, true)] {
            invoke(&w, CommandId::Gradient);
            w.dispatch(UiAction::Layer { action: LayerAction::Tool {
                tool: LayerCanvasTool::Gradient { radial, transparent },
            } });
            let before = raster(&w); let old = pixels(&before);
            pointer.stroke(&w, [64., 64.], [320., 192.]); committed(&w, &before);
            let edited = pixels(&raster(&w)); assert_ne!(edited, old);
            invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), old);
            invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), edited); healthy(&w);
        }
        invoke(&w, CommandId::Figure);
        w.dispatch(UiAction::Layer { action: LayerAction::Tool {
            tool: LayerCanvasTool::Figure { shape: layer_core::FigureShape::Rectangle, paint: layer_core::FigurePaint::Both },
        } });
        let before = raster(&w);
        pointer.stroke(&w, [100., 80.], [280., 180.]); committed(&w, &before);
        let edited = pixels(&raster(&w));
        capture_ui(&w, &output, &format!("{depth:?}-edited.png"));
        // Actual GTK Save/Open worker and chooser; compare every native sample.
        let name = format!("portable-paint-{depth:?}-{}.capy", std::process::id());
        let path = output.join(&name);
        invoke(&w, CommandId::SaveDocument);
        let save = chooser();
        save.set_current_folder(Some(&gtk::gio::File::for_path(&output))).unwrap();
        save.set_current_name(&name); pump(150); save.response(gtk::ResponseType::Accept); finish(&w);
        assert!(!state(&w).document_file.modified);
        let opened = Rc::new(RefCell::new(None)); let result = opened.clone();
        *w.open_document.borrow_mut() = Some(Rc::new(move |project, location, _| {
            *result.borrow_mut() = Some((project, location));
        }));
        invoke(&w, CommandId::OpenDocument);
        let open = chooser(); open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(150); open.response(gtk::ResponseType::Accept); finish(&w);
        let (project, location) = opened.borrow_mut().take().expect("Open published a document");
        assert_eq!(project.document.color, DocumentColor { space, depth });
        assert_eq!(pixels(&project.document.layer(project.document.active_layer).unwrap().raster), edited);
        w.window.destroy(); pump(100);
        let restored = Workspace::with_project(&app, Some((project, location)));
        restored.window.maximize(); restored.window.present(); ready(&restored);
        invoke(&restored, CommandId::FitCanvas);
        assert_eq!(pixels(&raster(&restored)), edited);
        invoke(&restored, CommandId::Fill);
        restored.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition { color: foreground } });
        let before = raster(&restored);
        pointer.stroke(&restored, [190., 120.], [190., 120.]); committed(&restored, &before);
        assert_ne!(pixels(&raster(&restored)), edited, "continue painting after reopening");
        invoke(&restored, CommandId::Undo); ready(&restored);
        assert_eq!(pixels(&raster(&restored)), edited); healthy(&restored);
        eprintln!("PASS {space:?}/{depth:?}: bucket, four gradients, rectangle, exact undo/redo, GTK Save/Open, continue painting");
        restored.window.destroy(); pump(100);
    }
    std::fs::write(pointer.dir.join("finished"), "done").unwrap();
}
