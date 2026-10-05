//! Real Wayland contacts, native paint, history and GTK Save/Open.
use super::*;
use super::new_photo::{capture_ui, chooser, finish, invoke, ready};
use layer_core::color::{DocumentColor, RgbColor, RgbSpace, SampleDepth};
use layer_core::raster::{RasterRevision, TileKey};
use std::collections::BTreeMap;

pub(super) fn raster(w: &Workspace) -> RasterRevision {
    let gpu = w.gpu.borrow();
    let document = gpu.as_ref().unwrap().session.engine().document();
    active_raster(document).clone()
}
pub(super) fn pixels(root: &RasterRevision) -> BTreeMap<TileKey, Vec<u8>> {
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
pub(super) fn committed(w: &Rc<Workspace>, before: &RasterRevision) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(10); healthy(w);
        let current = raster(w);
        if current != *before && current.host_backed() { break; }
        assert!(Instant::now() < deadline, "contact did not commit pixels");
    }
    ready(w);
}

pub(super) fn stroke(input: &mut RemoteInput, w: &Workspace, from: [f32; 2], to: [f32; 2]) {
    let m = state(w).camera.document_to_surface();
    let scale = w.area.scale_factor() as f32;
    let at = |p: [f32; 2]| {
        let p = gtk::graphene::Point::new(
            (m[0] * p[0] + m[2] * p[1] + m[4]) / scale,
            (m[1] * p[0] + m[3] * p[1] + m[5]) / scale,
        );
        let p = w.area.compute_point(&w.window, &p).unwrap();
        [p.x(), p.y()]
    };
    input.perform(serde_json::json!([
        {"point": at(from), "down": true}, {"point": at(to)}, {"down": false}
    ]));
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_enclose_fill_pointer_workflow() {
    let app = native_test_app("art.capycanvas.EncloseFill");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("capy-enclose-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    let mut input = RemoteInput::new().timeout_secs(30);
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        let project = new_drawing(384, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        let w = Workspace::with_project(&app, Some((project, None)));
        w.window.maximize(); w.window.present(); ready(&w);
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let source = layer_core::color::source::rgba8_source([384, 256], |x, y| {
            let ink = [40, 150, 270].into_iter().any(|left|
                x >= left && x < left + 90 && y >= 50 && y < 170
                    && (x < left + 6 || x >= left + 84 || y < 56 || y >= 164));
            if ink { [0, 0, 0, 255] } else { [0; 4] }
        });
        ui_session_mut(&w).import_layer_source("Reference ink", std::sync::Arc::unwrap_or_clone(source)).unwrap();
        w.wake(); ready(&w);
        let reference = ui_session(&w).engine().document().working.occurrence.unwrap();
        let reference_pixels = pixels(&raster(&w));
        let reference_source = active_paint(ui_session(&w).engine().document()).clone();
        w.dispatch(UiAction::Layer { action: LayerAction::ReferenceSelection });
        w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
        let workspace = tool_settings_workspace(&w, &[CommandId::EncloseFill], true, false);
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
        invoke(&w, CommandId::FitCanvas);
        let enclose = command(&w, CommandId::EncloseFill);
        assert!(enclose.is_mapped() && enclose.is_sensitive());
        input.click(screen_point(enclose.upcast_ref(), &w.window, [0.5, 0.5]));
        invoke(&w, CommandId::SelectionReference);
        for (id, value) in [("tolerance", "15"), ("gap_closing", "0"), ("expansion", "0"), ("smoothing", "0")] {
            let control = named::<crate::number_control::NumberControl>(
                &w.panel_widget(Panel::ToolSettings), &format!("tool-setting-{id}"));
            assert!(control.is_mapped(), "{id} is visible for Enclose and Fill");
            edit_number(&control, value);
        }
        w.dispatch(UiAction::SetColor { rgba: [0.85, 0.1, 0.05, 1.] });
        ready(&w);
        let empty = pixels(&raster(&w));
        let camera = state(&w).camera;
        let scale = w.area.scale_factor() as f32;
        let matrix = camera.document_to_surface();
        let at = |p: [f32; 2]| {
            let p = gtk::graphene::Point::new(
                (matrix[0] * p[0] + matrix[2] * p[1] + matrix[4]) / scale,
                (matrix[1] * p[0] + matrix[3] * p[1] + matrix[5]) / scale);
            let p = w.area.compute_point(&w.window, &p).unwrap();
            [p.x(), p.y()]
        };
        let before = raster(&w);
        input.perform(serde_json::json!([
            {"point":at([20.,30.]),"down":true}, {"point":at([315.,30.])},
            {"point":at([315.,190.])}, {"point":at([20.,190.])},
            {"point":at([20.,30.])}, {"down":false}
        ]));
        committed(&w, &before);
        let filled = pixels(&raster(&w));
        let sample = |x: u32, y: u32| {
            let tile_size = layer_core::raster::TILE_SIZE;
            let tile = filled.iter().find(|(key, _)| key.coordinate == [x / tile_size, y / tile_size]);
            tile.map_or([0; 4], |(_, bytes)| {
                assert_eq!(bytes.len(), (tile_size * tile_size * 4) as usize);
                let offset = ((y % tile_size * tile_size + x % tile_size) * 4) as usize;
                bytes[offset..offset + 4].try_into().unwrap()
            })
        };
        for [x, y] in [[85, 110], [195, 110]] {
            let rgba = sample(x, y);
            assert!(rgba[0] > 180 && rgba[1] < 80 && rgba[3] == 255, "enclosed hole {x},{y}: {rgba:?}");
        }
        for [x, y] in [[25, 110], [42, 110], [140, 110], [290, 110], [85, 195]] {
            assert_eq!(sample(x, y)[3], 0, "only enclosed transparent holes reach the paint destination at {x},{y}");
        }
        let reference_now = || {
            let gpu = w.gpu.borrow();
            let document = gpu.as_ref().unwrap().session.engine().document();
            let source = document.scene().paint_source(reference).unwrap();
            assert_eq!(source, &reference_source, "reference originals and operations stay exact");
            pixels(&source.raster)
        };
        assert_eq!(reference_now(), reference_pixels);
        capture_ui(&w, &output, &format!("enclose-{theme:?}.png"));
        invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), empty);
        invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), filled);
        invoke(&w, CommandId::Undo); ready(&w);
        input.perform(serde_json::json!([
            {"point":at([20.,30.]),"down":true}, {"point":at([315.,30.])},
            {"point":at([315.,190.])}, {"point":at([20.,190.])},
            {"key":0xff1b,"down":true}, {"key":0xff1b,"down":false}, {"down":false}
        ]));
        ready(&w);
        assert_eq!(pixels(&raster(&w)), empty, "Escape cancels an active enclosure");
        invoke(&w, CommandId::Redo); ready(&w);
        assert_eq!(pixels(&raster(&w)), filled, "cancel preserves the redo entry");
        assert_eq!(reference_now(), reference_pixels);
        healthy(&w);
        eprintln!("PASS {theme:?}: reference holes, separate paint source, untouched exterior/ink/reference, one undo/redo and native Escape");
        w.window.destroy(); pump(100);
    }
    input.finish();
}

#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_portable_paint_pointer_workflow"]
#[allow(deprecated)]
fn native_portable_paint_pointer_workflow() {
    let app = native_test_app("art.capycanvas.EditingTools");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("capy-editing-{}", std::process::id())));
    std::fs::create_dir_all(&output).unwrap();
    let mut input = RemoteInput::new().timeout_secs(20);
    input.ready();
    for (space, depth) in [
        (RgbSpace::Srgb, SampleDepth::U8),
        (RgbSpace::Srgb, SampleDepth::U16),
        (RgbSpace::DisplayP3, SampleDepth::F16),
    ] {
        let mut project = new_drawing(384, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
        composition_mut(&mut project).color = DocumentColor { space, depth };
        composition_mut(&mut project).blend = project.composition().blend.for_depth(depth);
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
        stroke(&mut input, &w, [96., 128.], [96., 128.]); committed(&w, &before);
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
                tool: LayerCanvasTool::Gradient { shape:if radial {layer_core::GradientShape::Radial} else {layer_core::GradientShape::Linear} },
            } });
            let mut color=state(&w).colors.background;color.rgba[3]=if transparent {0.} else {1.};
            w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Gradient {target:layer_ui::GradientDestination::Tool {epoch:state(&w).document_file.epoch},edit:layer_ui::GradientEdit::Stop {index:Some(1),position:1.,color:Some(color),remove:false}}});
            let before = raster(&w); let old = pixels(&before);
            stroke(&mut input, &w, [64., 64.], [320., 192.]); committed(&w, &before);
            let edited = pixels(&raster(&w)); assert_ne!(edited, old);
            invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), old);
            invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), edited); healthy(&w);
        }
        invoke(&w, CommandId::Figure);
        w.dispatch(UiAction::Layer { action: LayerAction::Tool {
            tool: LayerCanvasTool::Figure { shape: layer_core::FigureShape::Rectangle, paint: layer_core::FigurePaint::Both },
        } });
        let before = raster(&w);
        stroke(&mut input, &w, [100., 80.], [280., 180.]); committed(&w, &before);
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
        *w.open_document.borrow_mut() = Some(Rc::new(move |project, location| {
            *result.borrow_mut() = Some((project, location));
        }));
        invoke(&w, CommandId::OpenDocument);
        let open = chooser(); open.set_file(&gtk::gio::File::for_path(&path)).unwrap();
        pump(150); open.response(gtk::ResponseType::Accept); finish(&w);
        let (project, location) = opened.borrow_mut().take().expect("Open published a document");
        assert_eq!(project.composition().color, DocumentColor { space, depth });
        assert_eq!(pixels(&paint_at(&project, 0).raster), edited);
        w.window.destroy(); pump(100);
        let restored = Workspace::with_project(&app, Some((project, location)));
        restored.window.maximize(); restored.window.present(); ready(&restored);
        invoke(&restored, CommandId::FitCanvas);
        assert_eq!(pixels(&raster(&restored)), edited);
        invoke(&restored, CommandId::Fill);
        restored.dispatch(UiAction::Color { action: layer_ui::ColorAction::Definition { color: foreground } });
        let before = raster(&restored);
        stroke(&mut input, &restored, [190., 120.], [190., 120.]); committed(&restored, &before);
        assert_ne!(pixels(&raster(&restored)), edited, "continue painting after reopening");
        invoke(&restored, CommandId::Undo); ready(&restored);
        assert_eq!(pixels(&raster(&restored)), edited); healthy(&restored);
        eprintln!("PASS {space:?}/{depth:?}: bucket, four gradients, rectangle, exact undo/redo, GTK Save/Open, continue painting");
        restored.window.destroy(); pump(100);
    }
    input.finish();
}
