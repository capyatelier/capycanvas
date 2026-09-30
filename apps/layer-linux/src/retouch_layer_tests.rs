//! Retouching layers with real Mutter delivery: a Dodge & Burn layer leaves
//! the image as it was until white and black airbrush strokes dodge and burn
//! it, and Frequency Separation previews its blur in its dialog, recombines
//! into the photo, and takes a small brush on High, each one undo step.
use super::clone_stamp::{fill, finish, stroke};
use super::photo_edit::{choose, document, labelled, shown, start, window_point};
use super::*;
use serde_json::json;

fn near(a: [u8; 4], b: [u8; 4], within: u8) -> bool {
    a.iter().zip(b).take(3).all(|(a, b)| a.abs_diff(b) <= within)
}

fn brightness(pixel: [u8; 4]) -> u32 {
    pixel.iter().take(3).map(|c| u32::from(*c)).sum()
}

/// The canvas's own composite at document point `at`, before the display's
/// color conversion.
fn composite(w: &Workspace, [x, y]: [f32; 2]) -> [u8; 4] {
    let image = ui_session(&w).engine().backend().capture().unwrap();
    let m = state(w).camera.document_to_surface();
    let [x, y] = [(m[0] * x + m[2] * y + m[4]) as usize, (m[1] * x + m[3] * y + m[5]) as usize];
    image.bytes[y * image.stride as usize + x * 4..][..4].try_into().unwrap()
}

fn active(w: &Workspace) -> layer_core::Layer {
    let doc = document(w);
    doc.layer(doc.active_layer).unwrap().clone()
}

/// Run `command` from the menu `path` with the mouse, or dispatch it where
/// the tablet proxy cannot open popups.
fn run(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str, command: CommandId, path: &[&str]) {
    if device == "pen" {
        w.dispatch(UiAction::Invoke { command });
    } else {
        choose(w, input, path[0], &path[1..]);
    }
}

fn paint(w: &Rc<Workspace>, input: &mut RemoteInput, device: &str, rgba: [f32; 4], from: [f32; 2], to: [f32; 2]) {
    let strokes = || ui_session(&w).engine().metrics().committed_strokes;
    let before = strokes();
    w.dispatch(UiAction::SetColor { rgba });
    until(
        || {
            w.gpu.borrow().as_ref().is_some_and(|g| {
                let engine = g.session.engine();
                engine.backend().paint_ready(engine.document(), engine.brush(), false)
            })
        },
        "the brush is ready before pen-down",
    );
    stroke(input, device, window_point(w, from), window_point(w, to));
    until(|| strokes() > before, "the stroke ends");
    until(|| !ui_session(&w).engine().has_pending_document_edits(), "the stroke is captured");
    pump(200);
}

fn snapshots(w: &Rc<Workspace>, name: &str) {
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(300);
        if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
            save_snapshot(w, 100, || std::path::Path::new(&dir).join(format!("{name}-{theme:?}.png")));
        }
    }
}

fn retouch_layers_journey(id: &str, device: &str) {
    let (_app, w, mut input) = start(id);
    run(&w, &mut input, device, CommandId::BlendPerceptual, &["Edit", "Blending", "Perceptual Blending"]);
    until(|| document(&w).blend_space == layer_core::BlendSpace::Perceptual, "the drawing blends perceptually");
    fill(&w, [0.25, 0.35, 0.3, 1.], [0.15, 0.15, 0.85, 0.85]);
    fill(&w, [0.8, 0.7, 0.3, 1.], [0.35, 0.4, 0.5, 0.6]);
    let photo = document(&w).active_layer;
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let points = [[0.3, 0.3], [0.6, 0.45], [0.7, 0.7], [0.502, 0.5], [0.498, 0.5]].map(|[x, y]| [width * x, height * y]);
    pump(300);
    let original = points.map(|p| shown(&w, p));

    run(&w, &mut input, device, CommandId::NewDodgeBurnLayer, &["Layer", "New", "New Dodge & Burn Layer"]);
    until(|| &*active(&w).name == "Dodge & Burn", "a Dodge & Burn layer is added and active");
    until(|| !ui_session(&w).engine().has_pending_document_edits(), "the gray fill is captured");
    pump(300);
    assert_eq!(active(&w).properties.blend, layer_core::LayerBlend::SoftLight);
    for (p, before) in points.iter().zip(original) {
        assert!(near(shown(&w, *p), before, 1), "the neutral layer leaves {p:?} unchanged");
    }
    w.dispatch(UiAction::Invoke { command: CommandId::Airbrush });
    w.dispatch(UiAction::SetBrushSize { value: width * 0.08 });
    w.dispatch(UiAction::SetBrushOpacity { value: 0.3 });
    let [dodge, burn] = [[width * 0.5, height * 0.3], [width * 0.5, height * 0.7]];
    let [light, dark] = [dodge, burn].map(|p| shown(&w, p));
    paint(&w, &mut input, device, [1., 1., 1., 1.], [width * 0.3, height * 0.3], [width * 0.7, height * 0.3]);
    paint(&w, &mut input, device, [0., 0., 0., 1.], [width * 0.3, height * 0.7], [width * 0.7, height * 0.7]);
    assert!(brightness(shown(&w, dodge)) > brightness(light) + 6, "white dodges: {:?} -> {:?}", light, shown(&w, dodge));
    assert!(brightness(shown(&w, burn)) + 6 < brightness(dark), "black burns: {:?} -> {:?}", dark, shown(&w, burn));
    snapshots(&w, &format!("dodge-burn-{device}"));

    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: photo.0, mask: false } });
    let painted = points.map(|p| shown(&w, p));
    let composed = points.map(|p| composite(&w, p));
    run(&w, &mut input, device, CommandId::FrequencySeparation, &["Filter", "Frequency Separation…"]);
    until(|| state(&w).layer_tools.frequency_separation.is_some(), "the Frequency Separation dialog opens");
    let field = find_named(w.window.upcast_ref(), "frequency-separation-radius").expect("the radius field");
    until(|| field.is_mapped(), "the radius field shows");
    if device == "pen" {
        w.dispatch(UiAction::FrequencySeparation { action: FrequencySeparationAction::Radius { radius: 12. } });
    } else {
        input.click(screen_point(&find_css(&field, "number-value").unwrap(), &w.window, [0.5, 0.5]));
        input.perform(json!([{ "key": 0xffe3, "down": true }, { "key": 0x61, "down": true }, { "key": 0x61, "down": false }, { "key": 0xffe3, "down": false }]));
        for c in "12".chars() {
            input.key(c as u32);
        }
        input.key(0xff0d);
    }
    until(|| state(&w).layer_tools.frequency_separation.as_ref().is_some_and(|v| v.radius == 12.), "the typed radius");
    let edge = points[3];
    until(|| !near(shown(&w, edge), painted[3], 2), "the canvas previews the blur at the photo's edge");
    assert_eq!(document(&w).layers.len(), 3, "the preview adds no layer");
    snapshots(&w, &format!("frequency-separation-{device}"));
    if device == "pen" {
        w.dispatch(UiAction::FrequencySeparation { action: FrequencySeparationAction::Apply });
    } else {
        let apply = labelled(w.window.upcast_ref(), "Apply").expect("Apply");
        input.click(screen_point(&apply, &w.window, [0.5, 0.5]));
    }
    until(|| state(&w).layer_tools.frequency_separation.is_none(), "Apply closes the dialog");
    until(|| &*active(&w).name == "High", "High is active");
    until(|| !ui_session(&w).engine().has_pending_document_edits(), "Low and High are baked");
    pump(300);
    let doc = document(&w);
    assert_eq!(doc.layers.iter().map(|l| l.name.to_string()).collect::<Vec<_>>()[1..4], ["Frequency Separation", "High", "Low"]);
    assert!(!doc.layer(photo).unwrap().visible, "the photo stays below, hidden");
    for (p, before) in points.iter().zip(composed) {
        assert!(near(composite(&w, *p), before, 2), "Low and High recombine at {p:?}: {:?} {before:?}", composite(&w, *p));
    }
    w.dispatch(UiAction::Invoke { command: CommandId::Pen });
    w.dispatch(UiAction::SetBrushSize { value: 4. });
    w.dispatch(UiAction::SetBrushOpacity { value: 1. });
    let mark = points[1];
    paint(&w, &mut input, device, [0.9, 0.1, 0.1, 1.], [width * 0.55, height * 0.45], [width * 0.65, height * 0.45]);
    until(|| !near(shown(&w, mark), painted[1], 8), "a small brush paints on High");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| near(composite(&w, mark), composed[1], 2), "one undo removes the stroke");
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).layers.len() == 3 && document(&w).layer(photo).unwrap().visible, "one undo removes Frequency Separation");
    finish(&w, &input);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse and keyboard delivery"]
fn native_dodge_burn_and_frequency_separation_with_the_mouse() {
    retouch_layers_journey("art.capycanvas.RetouchLayersMouse", "mouse");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_dodge_burn_and_frequency_separation_with_the_pen --tablet"]
fn native_dodge_burn_and_frequency_separation_with_the_pen() {
    retouch_layers_journey("art.capycanvas.RetouchLayersPen", "pen");
}

/// Timing, not a gate: New Dodge & Burn Layer and Frequency Separation on a
/// 16-bit 24 MP photo, and the longest main-loop stall meanwhile.
#[test]
#[ignore = "hardware 24 MP timing: workspace-motion.sh gtk --native-test=native_retouch_layers_timing"]
fn native_retouch_layers_timing() {
    let app = native_test_app("art.capycanvas.RetouchLayersTiming");
    let mut project = native_navigation::photo([6000, 4000]);
    project.document.blend_space = layer_core::BlendSpace::Perceptual;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    pump(1500);
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    let photo = document(&w).layers.iter().find(|l| l.source.is_some()).unwrap().id;
    w.dispatch(UiAction::Layer { action: LayerAction::Select { id: photo.0, mask: false } });
    let idle = |w: &Workspace| {
        let gpu = w.gpu.borrow();
        let engine = gpu.as_ref().unwrap().session.engine();
        !engine.has_pending_document_edits()
            && engine.document().layers.iter().all(|l| l.raster.try_data().is_some_and(|d| d.is_ok_and(|d| d.host_backed())))
    };
    until(|| idle(&w), "the photo is ready");
    let count = document(&w).layers.len();
    let separate = |action| w.dispatch(UiAction::FrequencySeparation { action });
    let steps: [(&str, usize, &dyn Fn(), UiAction); 2] = [
        ("New Dodge & Burn Layer", 1, &|| {}, UiAction::Invoke { command: CommandId::NewDodgeBurnLayer }),
        (
            "Frequency Separation, radius 8",
            3,
            &|| {
                w.dispatch(UiAction::Invoke { command: CommandId::FrequencySeparation });
                separate(FrequencySeparationAction::Radius { radius: 8. });
            },
            UiAction::FrequencySeparation { action: FrequencySeparationAction::Apply },
        ),
    ];
    for (name, added, prepare, action) in steps {
        prepare();
        pump(1500);
        let stall = Rc::new(Cell::new(Duration::ZERO));
        let last = Rc::new(Cell::new(Instant::now()));
        let tick = glib::timeout_add_local(Duration::from_millis(1), glib::clone!(#[strong] stall, #[strong] last, move || {
            let now = Instant::now();
            stall.set(stall.get().max(now - last.replace(now)));
            glib::ControlFlow::Continue
        }));
        let start = Instant::now();
        w.dispatch(action);
        let dispatch = start.elapsed();
        assert_eq!(document(&w).layers.len(), count + added, "{name} adds its layers");
        until(|| idle(&w), "the new layers are captured");
        let captured = start.elapsed();
        tick.remove();
        eprintln!(
            "{name} on 24 MP: dispatch {:.1} ms, result captured {:.0} ms, longest main-loop gap {:.1} ms",
            dispatch.as_secs_f64() * 1e3,
            captured.as_secs_f64() * 1e3,
            stall.get().as_secs_f64() * 1e3,
        );
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        until(|| document(&w).layers.len() == count, "undo removes the layers");
        until(|| idle(&w), "the photo is ready again");
    }
    w.window.close();
    pump(50);
}
