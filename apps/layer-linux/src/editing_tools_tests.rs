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
fn sample(paint: &BTreeMap<TileKey, Vec<u8>>, x: u32, y: u32) -> [u8; 4] {
    let size = layer_core::raster::TILE_SIZE;
    paint.iter().find(|(key, _)| key.coordinate == [x / size, y / size]).map_or([0; 4], |(_, bytes)| {
        assert_eq!(bytes.len(), (size * size * 4) as usize);
        let offset = ((y % size * size + x % size) * 4) as usize;
        bytes[offset..offset + 4].try_into().unwrap()
    })
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
fn native_fill_texture_controls_and_papers() {
    let app = native_test_app("art.capycanvas.FillTexture");
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let mut input = RemoteInput::new().timeout_secs(30);
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        for paper in [None, Some("bristol"), Some("hot_press")] {
            let mut project = new_drawing(384, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
            if let Some(id) = paper { project.set_paper_texture(layer_core::paper::built_in_paper(id).unwrap().texture.clone()); }
            else { project = project.without_paper().unwrap(); }
            let w = Workspace::with_project(&app, Some((project, None)));
            w.window.maximize(); w.window.present(); ready(&w);
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            let workspace = tool_settings_workspace(&w, &[CommandId::Fill, CommandId::Gradient], true, false);
            w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
            invoke(&w, CommandId::FitCanvas);
            w.dispatch(UiAction::SetColor { rgba: [0.85, 0.1, 0.05, 1.] });
            let empty = pixels(&raster(&w));
            for command_id in [CommandId::Fill, CommandId::LassoFill, CommandId::EncloseFill, CommandId::Gradient] {
                invoke(&w, command_id); ready(&w);
                let panel = w.panel_widget(Panel::ToolSettings);
                let strength = named::<crate::number_control::NumberControl>(&panel, "tool-setting-grain_depth");
                histogram::scroll_to(strength.upcast_ref());
                assert!(strength.is_mapped(), "{command_id:?} exposes Texture strength");
                for value in ["0", "100", "200"] {
                    edit_number(&strength, value);
                    let setting = state(&w).tool_settings.into_iter().find(|field| field.id == "grain_depth").unwrap();
                    assert_eq!(setting.value, value.parse::<f32>().unwrap() / 100.);
                }
            }
            let mut flat = None;
            for strength in ["0", "100"] {
                invoke(&w, CommandId::Fill); ready(&w);
                let control = named::<crate::number_control::NumberControl>(&w.panel_widget(Panel::ToolSettings), "tool-setting-grain_depth");
                histogram::scroll_to(control.upcast_ref()); edit_number(&control, strength);
                let before = raster(&w);
                stroke(&mut input, &w, [192., 128.], [192., 128.]);
                committed(&w, &before);
                let filled = pixels(&raster(&w));
                if strength == "0" { flat = Some(filled.clone()); }
                else {
                    if paper.is_none() { assert_eq!(Some(&filled), flat.as_ref(), "No paper remains bit-exact at 100%"); }
                    capture_ui(&w, &output, &format!("fill-texture-{}-100-{theme:?}.png", paper.unwrap_or("none")));
                }
                invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), empty);
                invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), filled);
                invoke(&w, CommandId::Undo); ready(&w);
            }
            w.window.destroy(); pump(100);
        }
    }
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
                x >= left && x < left + 90 && (50..170).contains(&y)
                    && (x < left + 6 || x >= left + 84 || !(56..164).contains(&y)));
            if ink { [0, 0, 0, 255] } else { [0; 4] }
        });
        ui_session_mut(&w).import_layer_source("Reference ink", std::sync::Arc::unwrap_or_clone(source)).unwrap();
        w.wake(); ready(&w);
        let reference = ui_session(&w).engine().document().working.occurrence.unwrap();
        let reference_pixels = pixels(&raster(&w));
        let reference_source = active_paint(ui_session(&w).engine().document()).clone();
        w.dispatch(UiAction::Layer { action: LayerAction::ReferenceSelection });
        w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
        let workspace = tool_settings_workspace(&w, &[CommandId::Fill], true, false);
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
        invoke(&w, CommandId::FitCanvas);
        let fill = command(&w, CommandId::Fill);
        input.click(screen_point(fill.upcast_ref(), &w.window, [0.5, 0.5]));
        let tools = w.panel_widget(Panel::Brushes);
        let groups = widgets(&tools).find(|widget| widget.has_css_class("tool-groups")).unwrap();
        assert_eq!(state(&w).tool_set.groups.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>(),
            ["Fill", "Lasso fill", "Enclose and Fill"]);
        for label in ["Lasso fill", "Enclose and Fill", "Fill", "Enclose and Fill"] {
            input.click(screen_point(&mapped_label(&groups, label).unwrap(), &w.window, [0.5, 0.5]));
        }
        assert!(ui_session(&w).command(CommandId::EncloseFill).selected);
        for (index, source) in [CommandId::SelectionReference, CommandId::SelectionVisible, CommandId::SelectionEditing, CommandId::SelectionReference].into_iter().enumerate() {
            let panel = w.panel_widget(Panel::ToolSettings);
            let option = named::<gtk::ToggleButton>(&panel, &format!("tool-choice-selection-source-{}", index % 3));
            histogram::scroll_to(option.upcast_ref());
            input.click(screen_point(option.upcast_ref(), &w.window, [0.5, 0.5]));
            assert!(option.is_active());
            assert_eq!(state(&w).tool_extra.iter().find_map(|option| match option { layer_ui::ToolOption::Choice { id: "selection-source", items, .. } => Some(items.iter().map(|item| item.label.as_ref()).collect::<Vec<_>>()), _ => None }).unwrap(),
                ["Reference layers", "All visible layers", "This layer"]);
            assert!(ui_session(&w).command(source).selected);
            assert!(ui_session(&w).command(CommandId::EncloseFill).selected,
                "source choices preserve the Enclose variant");
        }
        let checklist = state(&w).tool_extra.into_iter().find_map(|option| match option {
            layer_ui::ToolOption::Checklist { id: "reference-layers", items, .. } => Some(items), _ => None,
        }).unwrap();
        assert!(!checklist.iter().any(|item| item.label.as_ref() == "Paper"));
        let reference_index = checklist.iter().position(|item| item.label.as_ref() == "Reference ink").unwrap();
        let check = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), &format!("tool-check-reference-layers-{reference_index}"));
        histogram::scroll_to(check.upcast_ref());
        assert!(check.is_active());
        input.click(screen_point(check.upcast_ref(), &w.window, [0.5, 0.5])); ready(&w);
        assert!(!ui_session(&w).engine().document().scene().occurrence(reference).unwrap().reference);
        invoke(&w, CommandId::Undo); ready(&w);
        assert!(ui_session(&w).engine().document().scene().occurrence(reference).unwrap().reference);
        invoke(&w, CommandId::Redo); ready(&w);
        assert!(!ui_session(&w).engine().document().scene().occurrence(reference).unwrap().reference);
        invoke(&w, CommandId::Undo); ready(&w);
        let stop = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), "tool-action-StopAtColors");
        histogram::scroll_to(stop.upcast_ref());
        assert!(stop.is_active() && ui_session(&w).command(CommandId::StopAtColors).selected);
        for selected in [false, true] {
            input.click(screen_point(stop.upcast_ref(), &w.window, [0.5, 0.5]));
            assert_eq!(ui_session(&w).command(CommandId::StopAtColors).selected, selected);
        }
        capture_ui(&w, &output, &format!("enclose-controls-{theme:?}.png"));
        let source = find_named(&w.panel_widget(Panel::ToolSettings), "tool-choice-bar-selection-source").unwrap();
        histogram::scroll_to(&source);
        capture_ui(&w, &output, &format!("enclose-source-{theme:?}.png"));
        for (id, value) in [("grain_depth", "0"), ("tolerance", "15")] {
            let control = named::<crate::number_control::NumberControl>(
                &w.panel_widget(Panel::ToolSettings), &format!("tool-setting-{id}"));
            assert!(control.is_mapped(), "{id} is visible for Enclose and Fill");
            edit_number(&control, value);
        }
        for (id, index) in [("close-gaps", 1), ("region-edges", 2), ("region-edges", 1)] {
            let option = named::<gtk::ToggleButton>(&w.panel_widget(Panel::ToolSettings), &format!("tool-choice-{id}-{index}"));
            histogram::scroll_to(option.upcast_ref());
            input.click(screen_point(option.upcast_ref(), &w.window, [0.5, 0.5]));
            assert!(option.is_active());
            assert_eq!(state(&w).tool_settings.iter().any(|field| field.id == "edge_offset"), id == "region-edges" && index == 2);
        }
        let smooth = named::<gtk::CheckButton>(&w.panel_widget(Panel::ToolSettings), "tool-action-SmoothRegionEdges");
        histogram::scroll_to(smooth.upcast_ref());
        input.click(screen_point(smooth.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(!ui_session(&w).command(CommandId::SmoothRegionEdges).selected);
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
        for [x, y] in [[85, 110], [195, 110]] {
            let rgba = sample(&filled, x, y);
            assert!(rgba[0] > 180 && rgba[1] < 80 && rgba[3] == 255, "enclosed hole {x},{y}: {rgba:?}");
        }
        for [x, y] in [[25, 110], [42, 110], [140, 110], [290, 110], [85, 195]] {
            assert_eq!(sample(&filled, x, y)[3], 0, "only enclosed transparent holes reach the paint destination at {x},{y}");
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
        invoke(&w, CommandId::Undo); ready(&w);
        input.click(screen_point(&mapped_label(&groups, "Fill").unwrap(), &w.window, [0.5, 0.5]));
        let lines = named::<gtk::ToggleButton>(&w.panel_widget(Panel::ToolSettings), "tool-choice-selection-source-0");
        histogram::scroll_to(lines.upcast_ref());
        input.click(screen_point(lines.upcast_ref(), &w.window, [0.5, 0.5]));
        for (index, everywhere) in [(0, false), (1, true)] {
            let scope = named::<gtk::DropDown>(&w.panel_widget(Panel::ToolSettings), "tool-action-group-fill-scope");
            histogram::scroll_to(scope.upcast_ref());
            input.click(screen_point(scope.upcast_ref(), &w.window, [0.5, 0.5]));
            input.key(0xff50);
            for _ in 0..index { input.key(0xff54); }
            input.key(0xff0d);
            let before = raster(&w);
            stroke(&mut input, &w, [85., 110.], [85., 110.]);
            committed(&w, &before);
            let result = pixels(&raster(&w));
            assert_eq!(sample(&result, 85, 110)[3], 255);
            for [x, y] in [[195, 110], [290, 110], [25, 110]] {
                assert_eq!(sample(&result, x, y)[3], if everywhere { 255 } else { 0 });
            }
            invoke(&w, CommandId::Undo); ready(&w); assert_eq!(pixels(&raster(&w)), empty);
            invoke(&w, CommandId::Redo); ready(&w); assert_eq!(pixels(&raster(&w)), result);
            invoke(&w, CommandId::Undo); ready(&w);
        }
        let mut layout = layer_ui::WorkspacePreset::Painter.layout(Platform::Gtk);
        for zone in &mut layout.header.zones {
            zone.retain(|entry| matches!(entry.item, HeaderItem::Capy | HeaderItem::Tool {
                control: ToolbarControl::ToolSlot { slot: layer_ui::ToolSlotId::Fill }
            }));
        }
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
            layout, ..WorkspaceState::default()
        }) });
        invoke(&w, CommandId::EncloseFill); invoke(&w, CommandId::Hand); ready(&w);
        let id = state(&w).workspace.layout.header.entries().find(|entry| entry.item == HeaderItem::Tool {
            control: ToolbarControl::ToolSlot { slot: layer_ui::ToolSlotId::Fill }
        }).unwrap().id;
        let header = find_named(w.window.upcast_ref(), &format!("header-item-{id}")).unwrap();
        let at = screen_point(&header, &w.window, [0.5, 0.5]);
        input.click(at); ready(&w);
        assert!(ui_session(&w).command(CommandId::EncloseFill).selected);
        input.click(at); ready(&w);
        assert_eq!(state(&w).customization.drawer.as_ref().unwrap().columns, [vec![Panel::Tools], vec![Panel::ToolSettings]]);
        let tools = find_named(w.window.upcast_ref(), "drawer-panel-Tools").unwrap();
        for label in ["Fill", "Lasso fill", "Enclose and Fill"] { assert!(mapped_label(&tools, label).is_some()); }
        capture_ui(&w, &output, &format!("sketch-fill-{theme:?}.png"));
        input.click(at); ready(&w);
        let destination = screen_point(w.area.upcast_ref(), &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([
            {"point": at, "down": true}, {"point": destination}, {"down": false}
        ]));
        ready(&w);
        assert_eq!(state(&w).workspace.layout.bubbles.bubbles.len(), 1);
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


#[derive(serde::Deserialize)]
struct ScribbleSheet { size: [u32; 2], cases: Vec<ScribbleCase> }
#[derive(serde::Deserialize)]
struct ScribbleCase {
    name: String, preset: u32, vector: bool, paper: String, bounds: [u32; 4], angle: u32, gap: u32,
    direction: String, size: f32, paths: Vec<Vec<[f32; 2]>>, junctions: Vec<[f32; 2]>, regions: Vec<ScribbleRegion>, eraser: Vec<[f32; 2]>,
}
#[derive(serde::Deserialize)]
struct ScribbleRegion {
    side: String, seed: [f32; 2], polygon: Vec<[f32; 2]>, centres: Vec<[f32; 2]>, empty_probes: Vec<[f32; 2]>, wash_samples: Vec<[u32; 2]>, known_empty_centres: Vec<[f32; 2]>,
    #[serde(default)] known_misses: BTreeMap<String,usize>,
    #[serde(default)] known_leaks: BTreeMap<String,usize>,
    #[serde(default)] known_filled_probes: BTreeMap<String,Vec<usize>>,
}
fn geometry_distance(point: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let d = [b[0] - a[0], b[1] - a[1]];
    let t = (((point[0] - a[0]) * d[0] + (point[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(f32::EPSILON)).clamp(0., 1.);
    ((point[0] - a[0] - d[0] * t).powi(2) + (point[1] - a[1] - d[1] * t).powi(2)).sqrt()
}
fn geometry_inside(point: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
        if (a[1] > point[1]) != (b[1] > point[1]) && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0] { inside = !inside; }
    }
    inside
}
fn scribble_distance(case: &ScribbleCase, region: &ScribbleRegion, point: [f32; 2]) -> f32 {
    let edge = region.polygon.iter().zip(region.polygon.iter().cycle().skip(1)).map(|(a, b)| geometry_distance(point, *a, *b));
    edge.chain(case.paths.iter().flat_map(|path| path.windows(2)).map(|p| geometry_distance(point, p[0], p[1]))).fold(f32::INFINITY, f32::min)
}
fn score_scribble(case: &ScribbleCase, region: &ScribbleRegion, paint: &BTreeMap<TileKey, Vec<u8>>, reference: &BTreeMap<TileKey, Vec<u8>>, outside_cell: usize) {
    let band = case.size * 0.5 + 1.5;
    let mut missed = 0;let mut leaked = outside_cell;
    for y in case.bounds[1]..case.bounds[1] + case.bounds[3] {
        for x in case.bounds[0]..case.bounds[0] + case.bounds[2] {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let line = scribble_distance(case, region, point);
            let alpha = sample(paint, x, y)[3];
            let junction_ink = sample(reference, x, y)[3] > 0 && case.junctions.iter().any(|junction| geometry_distance(point, *junction, *junction) <= 24.);
            if geometry_inside(point, &region.polygon) {
                if line > band && !junction_ink && alpha < 250 { missed += 1; }
            } else if line > band && !junction_ink && alpha != 0 {leaked += 1;}
        }
    }
    for &[x,y] in &region.wash_samples {assert!(sample(paint,x,y)[3] >= 250, "{} missed bottom wash at [{x},{y}]",case.name);}
    let leak_ceiling = region.known_leaks.get("gtk").copied().unwrap_or(0);
    assert!(leaked <= leak_ceiling, "{} {} leaked {leaked} unexpected pixels (ceiling {leak_ceiling})",case.name,region.side);
    if leak_ceiling > 0 {eprintln!("{} {} known leak: {leaked}/{leak_ceiling}; shrink or remove when improved",case.name,region.side);}
    let ceiling = region.known_misses.get("gtk").copied().unwrap_or(4);
    assert!(missed <= ceiling, "{} missed {missed} interior/tip/corner pixels (ceiling {ceiling})", case.name);
    if ceiling > 4 {eprintln!("{} {} known misses: {missed}/{ceiling}; shrink or remove the ceiling when improved",case.name,region.side);}
    let mut known_missing = 0;
    for centre in &region.centres {
        let covered = (-1..=1).any(|dy| (-1..=1).any(|dx| {
            let x = centre[0].round() as i32 + dx;let y = centre[1].round() as i32 + dy;
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            x >= 0 && y >= 0 && geometry_distance(p, *centre, *centre) <= 1.5 && geometry_inside(p, &region.polygon) && sample(paint, x as u32, y as u32)[3] > 0
        }));
        if region.known_empty_centres.contains(centre) {known_missing += usize::from(!covered);}
        else {assert!(covered, "{} has no fill under centre line {centre:?}",case.name);}
    }
    if !region.known_empty_centres.is_empty() {eprintln!("{} {} known centre misses: {known_missing}/{}; remove only after all hosts cover them",case.name,region.side,region.known_empty_centres.len());}
    let known_probes = region.known_filled_probes.get("gtk").map(Vec::as_slice).unwrap_or(&[]);
    assert!(known_probes.iter().all(|index| *index < region.empty_probes.len()));
    for (index,point) in region.empty_probes.iter().enumerate() {assert!(sample(paint,point[0].floor() as u32,point[1].floor() as u32)[3] == 0 || leaked > 0 && leak_ceiling > 0 && known_probes.contains(&index), "{} {} filled the far side at {point:?}",case.name,region.side);}
}
fn capture_scribble(paint: &BTreeMap<TileKey, Vec<u8>>, [left,top,width,height]: [u32;4], path: &std::path::Path) {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in top..top + height {for x in left..left + width {rgba.extend_from_slice(&sample(paint,x,y));}}
    gtk::gdk::MemoryTexture::new(width as i32,height as i32,gtk::gdk::MemoryFormat::R8g8b8a8,&glib::Bytes::from_owned(rgba),width as usize*4).save_to_png(path).unwrap();
}
fn scribble_path(input: &mut RemoteInput, w: &Rc<Workspace>, points: &[[f32; 2]]) {
    ready(w);
    let matrix = state(w).camera.document_to_surface();let scale = w.area.scale_factor() as f32;
    let mut events = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let p = gtk::graphene::Point::new((matrix[0] * point[0] + matrix[2] * point[1] + matrix[4]) / scale, (matrix[1] * point[0] + matrix[3] * point[1] + matrix[5]) / scale);
        let p = w.area.compute_point(&w.window, &p).unwrap();
        if index == 0 { events.push(serde_json::json!({"pen":"move", "point":[p.x(),p.y()]})); }
        events.push(serde_json::json!({"pen": if index == 0 { "down" } else { "move" }, "point": [p.x(), p.y()]}));
    }
    events.push(serde_json::json!({"pen":"up"}));input.perform(serde_json::Value::Array(events));ready(w);
}

#[test]
#[ignore = "private Wayland display, hardware GPU and --tablet"]
fn native_fill_scribble_geometry() {
    let sheet: ScribbleSheet = serde_json::from_str(include_str!("../../../tools/fixtures/fill-scribble.json")).unwrap();
    let app = native_test_app("art.capycanvas.FillScribbles");
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());std::fs::create_dir_all(&output).unwrap();
    let mut input = RemoteInput::new().timeout_secs(30);input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        for paper in ["cold_press", "rough"] {
            let mut project = new_drawing(sheet.size[0], sheet.size[1], &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
            assert_eq!(project.composition().color.depth, SampleDepth::U8);
            project.set_paper_texture(layer_core::paper::built_in_paper(paper).unwrap().texture.clone());project.set_paper_relief(100.);
            let w = Workspace::with_project(&app, Some((project, None)));w.window.maximize();w.window.present();ready(&w);
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });invoke(&w, CommandId::FitCanvas);
            w.dispatch(UiAction::SetColor { rgba: [0., 0., 0., 1.] });
            let mut reference = None;
            for case in sheet.cases.iter().filter(|case| case.paper == paper) {
                if case.vector {
                    reference = Some(pixels(&raster(&w)));
                    w.dispatch(UiAction::Layer { action: LayerAction::ReferenceSelection });
                    w.dispatch(UiAction::Layer { action: LayerAction::NewVector });
                    let session = ui_session(&w);let document = session.engine().document();
                    assert!(document.working.occurrence.is_some_and(|layer| document.scene().vector_layer(layer).is_some()));
                }
                w.dispatch(UiAction::SelectBrush { id: case.preset });w.dispatch(UiAction::SetBrushSize { value: case.size });
                w.dispatch(UiAction::SetToolSetting { id: "smoothing_strength".into(), value: 0. });
                for path in &case.paths { scribble_path(&mut input, &w, path); }
                if case.vector {
                    let session = ui_session(&w);let document = session.engine().document();
                    assert!(document.scene().vector_layer(document.working.occurrence.unwrap()).unwrap().paths().next().is_some());
                }
                if !case.eraser.is_empty() {
                    let before = raster(&w);
                    w.dispatch(UiAction::SelectBrush { id: layer_core::DefaultBrushPreset::Eraser as u32 });
                    w.dispatch(UiAction::SetBrushSize { value: case.gap as f32 });assert!(ui_session(&w).command(CommandId::Eraser).selected);scribble_path(&mut input, &w, &case.eraser);
                    assert_ne!(raster(&w), before, "the real eraser contact cuts the reference ink");
                }
            }
            let reference = reference.unwrap_or_else(|| pixels(&raster(&w)));
            capture_scribble(&reference,[0,0,sheet.size[0],sheet.size[1]],&output.join(format!("scribble-reference-raster-{paper}-{theme:?}.png")));
            capture_ui(&w, &output, &format!("scribble-reference-{paper}-{theme:?}.png"));
            w.dispatch(UiAction::Layer { action: LayerAction::ReferenceSelection });w.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } });
            let workspace = tool_settings_workspace(&w, &[CommandId::Fill], true, false);w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
            invoke(&w, CommandId::FitCanvas);invoke(&w, CommandId::Fill);invoke(&w, CommandId::SelectionReference);
            tool_controls_compact::select_group(&w, "close-gaps", 0);tool_controls_compact::select_group(&w, "region-edges", 0);
            w.dispatch(UiAction::SetToolSetting { id: "grain_depth".into(), value: 0. });w.dispatch(UiAction::SetToolSetting { id: "tolerance".into(), value: 0.05 });
            w.dispatch(UiAction::SetBrushOpacity { value: 1. });
            assert!(state(&w).tool_settings.iter().any(|field| field.id == "grain_depth" && field.value == 0.));
            w.dispatch(UiAction::SetColor { rgba: [0.85, 0.1, 0.05, 1.] });
            let empty = pixels(&raster(&w));
            for case in sheet.cases.iter().filter(|case| case.paper == paper) {
                for region in &case.regions {
                    scribble_path(&mut input, &w, &[region.seed, region.seed]);
                    capture_ui(&w, &output, &format!("scribble-{}-{}-{paper}-{theme:?}.png", case.preset, region.side));
                    let filled = pixels(&raster(&w));
                    let [left, top, width, height] = case.bounds;
                    capture_scribble(&filled,[0,0,sheet.size[0],sheet.size[1]],&output.join(format!("scribble-target-{}-{}-{paper}-{theme:?}.png",case.preset,region.side)));
                    let tile_size = layer_core::raster::TILE_SIZE;let mut outside_cell = 0;
                    for (key, bytes) in &filled {for (index, pixel) in bytes.chunks_exact(4).enumerate() {
                        let x = key.coordinate[0] * tile_size + index as u32 % tile_size;let y = key.coordinate[1] * tile_size + index as u32 / tile_size;
                        if pixel[3] != 0 && !((x >= left && y >= top && x < left + width && y < top + height) || scribble_distance(case,region,[x as f32 + 0.5,y as f32 + 0.5]) <= case.size * 0.5 + 1.5) {outside_cell += 1;}
                    }}
                    score_scribble(case, region, &filled, &reference, outside_cell);
                    invoke(&w, CommandId::Undo);ready(&w);
                    assert_eq!(pixels(&raster(&w)), empty, "{} {} Undo restores exact empty target", case.name, region.side);
                    eprintln!("PASS {theme:?} {paper} {} {}: {}° corner, {} px {} break, checked geometry bounds and exact Undo", case.name, region.side, case.angle, case.gap, case.direction);
                }
            }
            healthy(&w);w.window.destroy();pump(100);
        }
    }
    input.finish();
}
