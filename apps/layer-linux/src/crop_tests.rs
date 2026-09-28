//! Crop journeys with real Mutter delivery: a ratio from the crop bar, a
//! handle drag and Apply with the mouse, a finger and the pen; Straighten by
//! drawing a line; and Delete Cropped Pixels.
use super::photo_edit::{choose, document, labelled, shown, start, window_point};
use super::*;
use serde_json::json;

#[derive(Clone, Copy, Debug)]
enum Device {
    Mouse,
    Touch,
    Pen,
}

fn tap(input: &mut RemoteInput, device: Device, at: [f32; 2]) {
    match device {
        Device::Mouse => input.click(at),
        Device::Touch => input.perform(json!([{"touch": "down", "point": at}, {"wait_ms": 40}, {"touch": "up"}])),
        Device::Pen => {
            input.perform(json!([{"pen": "down", "point": at}, {"wait_ms": 40}, {"pen": "up"}, {"pen": "leave"}]))
        }
    }
}

fn drag(input: &mut RemoteInput, device: Device, from: [f32; 2], to: [f32; 2]) {
    let points: Vec<_> = (1..=10)
        .map(|i| {
            let t = i as f32 / 10.;
            [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t]
        })
        .collect();
    let mut events = match device {
        Device::Mouse => vec![json!({"point": from}), json!({"down": true})],
        Device::Touch => vec![json!({"touch": "down", "point": from})],
        Device::Pen => vec![json!({"pen": "down", "point": from})],
    };
    events.push(json!({"wait_ms": 30}));
    for point in points {
        events.push(match device {
            Device::Mouse => json!({"point": point}),
            Device::Touch => json!({"touch": "move", "point": point}),
            Device::Pen => json!({"pen": "move", "point": point}),
        });
        events.push(json!({"wait_ms": 16}));
    }
    events.extend(match device {
        Device::Mouse => vec![json!({"down": false})],
        Device::Touch => vec![json!({"touch": "up"})],
        Device::Pen => vec![json!({"pen": "up"}), json!({"pen": "leave"})],
    });
    input.perform(json!(events));
}

/// The action of the item labelled `label` in a native menu model. The
/// isolated tablet's synthetic serials cannot grab a popup, so a pen opens the
/// menu and its item runs through this action.
fn menu_action(model: &gtk::gio::MenuModel, label: &str) -> Option<String> {
    (0..model.n_items()).find_map(|i| {
        if let Some(section) = model.item_link(i, "section") {
            return menu_action(&section, label);
        }
        (model.item_attribute_value(i, "label", None)?.get::<String>()? == label)
            .then(|| model.item_attribute_value(i, "action", None)?.get::<String>())
            .flatten()
    })
}

fn center(w: &Workspace, widget: &gtk::Widget) -> [f32; 2] {
    let b = widget.compute_bounds(&w.window).expect("mapped widget");
    [b.x() + b.width() * 0.5, b.y() + b.height() * 0.5]
}

fn bar_widget(w: &Workspace, name: &str) -> gtk::Widget {
    let mut found = None;
    until(
        || {
            found = find_named(w.canvas_bar.root.upcast_ref(), name).filter(|b| b.is_mapped());
            found.is_some()
        },
        name,
    );
    found.unwrap()
}

fn cropping(w: &Workspace) -> bool {
    state(w).layer_tools.tool == LayerCanvasTool::Crop
        && state(w).canvas_bar.is_some_and(|b| b.context.kind == layer_ui::CanvasBarKind::Crop)
        && w.canvas_bar.root.is_mapped()
}

fn selected(w: &Workspace, command: CommandId) -> bool {
    state(w).commands.iter().any(|c| c.id == command && c.selected)
}

fn setting(w: &Workspace, id: &str) -> f32 {
    state(w).tool_settings.iter().find(|c| c.id == id).map_or(f32::NAN, |c| c.value)
}

/// A filled rectangle over the middle of the canvas, then the Crop tool from
/// Edit › Image.
fn crop_ready(id: &str) -> (NativeTestApp, Rc<Workspace>, RemoteInput) {
    let (app, w, mut input) = start(id);
    w.dispatch(UiAction::SetColor { rgba: [0.1, 0.3, 0.8, 1.] });
    let doc = document(&w);
    let [width, height] = [doc.width as f32, doc.height as f32];
    let paint = doc.active_layer;
    w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    let [x0, y0, x1, y1] = [width * 0.2, height * 0.2, width * 0.8, height * 0.8];
    native_pen_path(&w, &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]]);
    w.dispatch(UiAction::Layer { action: LayerAction::FillSelection });
    until(|| !document(&w).layer(paint).unwrap().raster.is_empty(), "the fill paints the selection");
    w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    choose(&w, &mut input, "Edit", &["Image", "Crop"]);
    until(|| cropping(&w), "Edit › Image › Crop opens the crop bar");
    (app, w, input)
}

fn ratio_handle_and_apply(device: Device, id: &str) {
    let (_app, w, mut input) = crop_ready(id);
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    let ratio = bar_widget(&w, "canvas-bar-choice-crop-ratio");
    let popover = ratio.downcast_ref::<gtk::MenuButton>().and_then(|b| b.popover()).expect("the Ratio menu");
    tap(&mut input, device, center(&w, &ratio));
    if let Device::Pen = device {
        let menu = popover.downcast_ref::<gtk::PopoverMenu>().expect("a menu popover").clone();
        let mut action = None;
        until(
            || {
                action = menu.is_visible().then(|| menu.menu_model().and_then(|m| menu_action(&m, "1:1"))).flatten();
                action.is_some()
            },
            "the pen opens the Ratio menu",
        );
        menu.activate_action(&action.unwrap(), None).unwrap();
    } else {
        let mut square = None;
        until(
            || {
                square = labelled(popover.upcast_ref(), "1:1");
                square.is_some()
            },
            "the Ratio menu lists 1:1",
        );
        tap(&mut input, device, center(&w, &square.unwrap()));
    }
    until(|| selected(&w, CommandId::CropRatioSquare), "1:1 constrains the crop");
    let side = width.min(height);
    assert_eq!(setting(&w, "crop_width"), side);
    let corner = window_point(&w, [(width - side) * 0.5, (height - side) * 0.5]);
    let target = [corner[0] + 120., corner[1] + 60.];
    drag(&mut input, device, corner, target);
    until(|| setting(&w, "crop_width") < side - 20., &format!("{device:?} drags the corner handle"));
    let cropped = setting(&w, "crop_width");
    assert!((cropped - setting(&w, "crop_height")).abs() < 0.01, "the ratio holds");
    until(|| cropping(&w), "the crop bar returns after the drag");
    if let Some(dir) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
        pump(200);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&dir).join(format!("crop-{device:?}.png"))).unwrap();
    }
    tap(&mut input, device, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
    until(|| state(&w).layer_tools.tool != LayerCanvasTool::Crop, "Apply finishes the crop");
    let after = document(&w);
    assert_eq!(after.width, after.height, "a square canvas");
    assert_eq!(after.width, cropped.round() as u32);
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).width == before.width, "one undo step restores the canvas");
    assert_eq!(document(&w).height, before.height);
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_crop_ratio_handle_and_apply_with_the_mouse() {
    ratio_handle_and_apply(Device::Mouse, "art.capycanvas.CropMouse");
}

#[test]
#[ignore = "isolated compositor, GPU and native touch delivery"]
fn native_crop_ratio_handle_and_apply_with_a_finger() {
    ratio_handle_and_apply(Device::Touch, "art.capycanvas.CropTouch");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_crop_ratio_handle_and_apply_with_the_pen --tablet"]
fn native_crop_ratio_handle_and_apply_with_the_pen() {
    ratio_handle_and_apply(Device::Pen, "art.capycanvas.CropPen");
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_crop_straighten_by_drawing_a_line() {
    let (_app, w, mut input) = crop_ready("art.capycanvas.CropStraighten");
    let before = document(&w);
    let paint = before.active_layer;
    let [width, height] = [before.width as f32, before.height as f32];
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-CropStraighten")));
    until(|| selected(&w, CommandId::CropStraighten), "Straighten arms line drawing");
    let angle = 0.1f32;
    let from = [width * 0.3, height * 0.5];
    let to = [from[0] + width * 0.4 * angle.cos(), from[1] + width * 0.4 * angle.sin()];
    drag(&mut input, Device::Mouse, window_point(&w, from), window_point(&w, to));
    until(|| !selected(&w, CommandId::CropStraighten), "the line levels the crop");
    let turned = setting(&w, "crop_angle");
    assert!((turned - angle).abs() < 0.01, "the crop follows the line: {turned}");
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
    until(|| state(&w).layer_tools.tool != LayerCanvasTool::Crop, "Apply straightens the image");
    until(|| document(&w).layer(paint).is_some_and(|l| l.raster.try_data().is_some()), "the resampled pixels are captured");
    let after = document(&w);
    assert!(after.width < before.width && after.height < before.height, "the level crop fits inside the old canvas");
    assert_ne!(after.layer(paint).unwrap().raster, before.layer(paint).unwrap().raster, "the pixels were resampled");
    pump(300);
    let middle = shown(&w, [after.width as f32 * 0.5, after.height as f32 * 0.5]);
    assert!(middle[2] > 150 && middle[0] < 100, "the fill stays in the middle: {middle:?}");
    assert!(state(&w).host_error.is_none(), "{:?}", state(&w).host_error);
    w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| document(&w).width == before.width, "one undo step restores the drawing");
    input.finish();
    w.window.close();
    pump(50);
}

#[test]
#[ignore = "isolated compositor, GPU and native mouse delivery"]
fn native_crop_deleting_cropped_pixels_leaves_nothing_to_reveal() {
    let (_app, w, mut input) = crop_ready("art.capycanvas.CropDelete");
    let before = document(&w);
    let [width, height] = [before.width as f32, before.height as f32];
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-CropDeleteCroppedPixels")));
    until(|| selected(&w, CommandId::CropDeleteCroppedPixels), "Delete Cropped Pixels turns on");
    drag(&mut input, Device::Mouse, window_point(&w, [0., 0.]), window_point(&w, [width * 0.4, height * 0.4]));
    until(|| setting(&w, "crop_width") < width * 0.7, "the top-left handle moves");
    tap(&mut input, Device::Mouse, center(&w, &bar_widget(&w, "canvas-bar-ApplyTransform")));
    until(|| state(&w).layer_tools.tool != LayerCanvasTool::Crop, "Apply crops");
    let cropped = document(&w);
    let origin = [before.width - cropped.width, before.height - cropped.height];
    w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
    for action in [
        layer_ui::CanvasSizeAction::Anchor { anchor: layer_ui::CanvasAnchor::BottomRight },
        layer_ui::CanvasSizeAction::Width { value: f64::from(before.width) },
        layer_ui::CanvasSizeAction::Height { value: f64::from(before.height) },
        layer_ui::CanvasSizeAction::Apply,
    ] {
        w.dispatch(UiAction::CanvasSize { action });
    }
    until(|| document(&w).width == before.width, "Canvas Size grows the canvas back");
    w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });
    pump(400);
    let deleted = shown(&w, [origin[0] as f32 * 0.75, height * 0.5]);
    assert!(deleted.iter().take(3).all(|v| *v > 200), "the deleted fill does not come back: {deleted:?}");
    let kept = shown(&w, [width * 0.6, height * 0.6]);
    assert!(kept[2] > 150 && kept[0] < 100, "the cropped fill stays: {kept:?}");
    input.finish();
    w.window.close();
    pump(50);
}
