use super::canvas_bar_tests::canvas_point;
use super::new_photo::{ready, response};
use super::*;
use crate::color_editor::tests::{editor, form, label, type_into, value};
use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
use layer_ui::{ColorAction, ColorEditorTarget, ColorForm, ColorScrubSpeed, ColorSlot, Theme};

const RED: [u8; 3] = [202, 75, 53];

fn hover_strip(w: &Workspace, input: &mut RemoteInput) {
    let strip = w.color_strip.root.clone();
    let before = (strip.margin_start(), strip.margin_top());
    let center = screen_point(strip.upcast_ref(), &w.window, [0.5, 0.5]);
    input.perform(serde_json::json!([{"point": center}]));
    until(|| (strip.margin_start(), strip.margin_top()) != before, "hovering the strip moves it away");
    pump(300);
    let bounds = strip.compute_bounds(&w.window).unwrap();
    assert!(!bounds.contains_point(&gtk::graphene::Point::new(center[0], center[1])), "the strip settles away from the pointer: {bounds:?} {center:?}");
}

fn clipboard(editor: &crate::color_editor::Editor) -> String {
    glib::MainContext::default().block_on(editor.dialog.clipboard().read_text_future()).unwrap().unwrap().to_string()
}
fn labels(row: &crate::color_editor::Row) -> Vec<String> {
    row.fields.iter().map(|field| field.label.text().to_string()).collect()
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native pointer input"]
fn native_color_editor_rows_sheet_and_canvas_pick() {
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    let app = native_test_app("art.capycanvas.ColorEditorJourney");
    let mut project = new_drawing_at(96, 96, SampleDepth::U8);
    paint_at_mut(&mut project, 0).original = Some(layer_core::color::source::rgba8_source([96, 96], |_, _| [RED[0], RED[1], RED[2], 255]));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new().timeout_secs(30);
    input.ready();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::Color { action: ColorAction::Definition { color: RgbColor::WHITE } });
        w.dispatch(UiAction::Color { action: ColorAction::EditorMemory { memory: Default::default() } });
        ready(&w);
        let checkpoint = ui_session(&w).engine().checkpoint();
        crate::color_editor::show(&w, ColorSlot::Foreground);
        let editor = editor(&w);
        save_snapshot(&w, 80, || output.join(format!("color-editor-{theme:?}.png")));
        let (minimum, natural, _, _) = editor.dialog.child().unwrap().measure(gtk::Orientation::Horizontal, -1);
        eprintln!("EDIT_COLOR_WIDTH minimum={minimum} natural={natural} sheet={}", editor.dialog.child().unwrap().width());
        assert!(editor.header.compute_bounds(&editor.left).unwrap().x() > 0., "a wide window keeps the wheel beside the values");
        assert_eq!(editor.rows.iter().map(label).collect::<Vec<_>>(), ["RGB", "HSB", "OKLCH"]);
        assert_eq!(labels(&editor.rows[0]), ["255", "255", "255"]);
        let wheel = editor.wheel.compute_bounds(&editor.dialog).unwrap();
        let title = editor.header.compute_bounds(&editor.dialog).unwrap();
        assert!((wheel.y() - title.y()).abs() <= 2., "the wheel starts level with the title: {wheel:?} {title:?}");
        let rows = [editor.hex_copy.upcast_ref::<gtk::Widget>(), editor.rows[0].copy.upcast_ref(), editor.rows[2].copy.upcast_ref()]
            .map(|button| button.compute_bounds(&editor.dialog).unwrap());
        assert!(rows.iter().all(|bounds| (bounds.x() + bounds.width() - rows[0].x() - rows[0].width()).abs() <= 1.), "copy buttons line up: {rows:?}");
        editor.paste("rgb(202 75 53)");
        assert_eq!(editor.hex.label.text(), "#CA4B35");
        assert_eq!(labels(&editor.rows[0]), ["202", "75", "53"]);
        editor.rows[0].copy.emit_clicked();
        assert_eq!(clipboard(&editor), "rgb(202 75 53)");
        editor.hex_copy.emit_clicked();
        assert_eq!(clipboard(&editor), "#CA4B35");
        form(&w, 1, ColorForm::Hsl);
        assert_eq!(label(&editor.rows[1]), "HSL");
        value(&w, 1, 0, "200");
        assert_eq!(editor.rows[1].fields[0].label.text(), "200°");
        let before = editor.rows[0].fields[2].label.text().parse::<u32>().unwrap();
        let field = editor.rows[0].fields[2].show.clone();
        let start = screen_point(field.upcast_ref(), &w.window, [0.5, 0.5]);
        input.perform(serde_json::json!([{"point": start}, {"down": true}, {"point": [start[0], start[1] - 6.]}, {"point": [start[0], start[1] - 20.]}, {"down": false}]));
        let after = editor.rows[0].fields[2].label.text().parse::<u32>().unwrap();
        assert!(after > before, "dragging a number up raises it: {before} -> {after}");
        assert!(!editor.rows[0].fields[2].editing(), "a drag never opens the field for typing");
        editor.step(ColorEditorTarget::Value { row: 0, index: 2 }, -1., ColorScrubSpeed::Normal);
        assert_eq!(editor.rows[0].fields[2].label.text().parse::<u32>().unwrap(), after - 1);
        editor.current.emit_clicked();
        assert_eq!(editor.hex.label.text(), "#FFFFFF");
        assert!(!editor.draft.borrow().changed());
        editor.swatches.emit_clicked();
        pump(300);
        assert!(editor.sheet_open.get() && !editor.body.is_sensitive() && editor.search.delegate().is_some_and(|text| text.is_focus()));
        editor.search.set_text("zzzz-no-color");
        pump(200);
        assert!(find_named(editor.sheet_body.upcast_ref(), "edit-color-sheet-empty").is_some());
        editor.search.set_text("");
        pump(200);
        save_snapshot(&w, 80, || output.join(format!("color-sheet-{theme:?}.png")));
        named::<gtk::Button>(editor.sheet_body.upcast_ref(), "edit-color-sheet-tile").emit_clicked();
        assert!(editor.draft.borrow().changed());
        editor.search.set_text("Ink");
        pump(200);
        editor.sheet_close.emit_clicked();
        pump(300);
        assert!(!editor.sheet_open.get() && editor.body.is_sensitive());
        response(&w, "cancel");
        assert_eq!(state(&w).colors.definition(), RgbColor::WHITE);
        assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
        crate::color_editor::show(&w, ColorSlot::Foreground);
        let reopened = crate::color_editor::tests::editor(&w);
        assert_eq!(label(&reopened.rows[1]), "HSL", "row formats are remembered");
        reopened.swatches.emit_clicked();
        pump(300);
        assert_eq!(reopened.search.text(), "Ink", "swatch search is remembered");
        reopened.show_sheet(false);
        pump(300);
        reopened.pick.emit_clicked();
        until(|| !reopened.dialog.is_mapped() && w.color_strip.root.is_visible(), "picking strip replaces the dialog");
        assert!(state(&w).color_picker.editor);
        input.perform(serde_json::json!([{"point": canvas_point(&w, [48., 48.])}]));
        pump(200);
        save_snapshot(&w, 80, || output.join(format!("color-strip-{theme:?}.png")));
        let strip_at = || (w.color_strip.root.margin_start(), w.color_strip.root.margin_top());
        let before = strip_at();
        let [x, y] = screen_point(w.color_strip.root.upcast_ref(), &w.window, [0., 0.5]);
        input.perform(serde_json::json!([{"point": [x - 4., y]}]));
        until(|| strip_at() != before, "the strip moves away from the sample");
        hover_strip(&w, &mut input);
        input.click(screen_point(w.color_strip.root.upcast_ref(), &w.window, [0.5, 0.5]));
        until(|| reopened.dialog.is_mapped() && !w.color_strip.root.is_visible(), "the strip goes back without a change");
        assert!(!reopened.draft.borrow().changed());
        reopened.pick.emit_clicked();
        until(|| w.color_strip.root.is_visible(), "picking again");
        input.click(canvas_point(&w, [48., 48.]));
        until(|| reopened.dialog.is_mapped(), "the dialog returns with the picked color");
        assert_eq!(reopened.hex.label.text(), "#CA4B35");
        assert_eq!(state(&w).colors.definition(), RgbColor::WHITE, "picking only changes the draft");
        assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint);
        response(&w, "apply");
        let picked = state(&w).colors.definition().encoded_in(RgbSpace::Srgb).unwrap();
        assert_eq!([0, 1, 2].map(|i| (picked[i] * 255.).round() as u8), RED);
    }
    w.window.unmaximize();
    w.window.set_default_size(620, 900);
    pump(500);
    crate::color_editor::show(&w, ColorSlot::Foreground);
    editor(&w).pick.emit_clicked();
    until(|| w.color_strip.root.is_visible(), "picking in a narrow window");
    pump(300);
    let strip = w.color_strip.root.clone();
    w.color_strip.hover(&w, Some([strip.width() as f64 * 0.5, strip.height() as f64 * 0.5]));
    pump(200);
    w.color_strip.hover(&w, None);
    let area = state(&w).camera.work_area;
    let scale = w.area.scale_factor() as f32;
    assert!(area[2] / scale < 2. * strip.width() as f32 + 32., "the narrow window leaves no room beside the strip: {area:?}");
    assert!(matches!(w.color_strip.corner.get(), layer_ui::ColorStripCorner::BottomLeft | layer_ui::ColorStripCorner::BottomRight), "a narrow work area uses a bottom corner");
    save_snapshot(&w, 80, || output.join("color-strip-narrow.png"));
    w.dispatch(UiAction::ColorPicker { action: layer_ui::ColorPickerAction::Toggle });
    until(|| editor(&w).dialog.is_mapped(), "the dialog returns after cancelling the pick");
    response(&w, "cancel");
    input.finish();
    w.window.destroy();
    pump(100);
}

fn audit(w: &Workspace, editor: &crate::color_editor::Editor, output: &std::path::Path, name: &str) {
    pump(450);
    let sheet = editor.dialog.child().unwrap();
    let bounds = sheet.compute_bounds(&w.window).unwrap();
    println!("EDIT_COLOR_AUDIT {name} {} {} {} {}", bounds.x(), bounds.y(), bounds.width(), bounds.height());
    save_snapshot(w, 60, || output.join(format!("{name}.png")));
}

fn srgb(hex: u32) -> RgbColor {
    RgbColor::new(RgbSpace::Srgb, [16, 8, 0].map(|shift| ((hex >> shift) & 255) as f32 / 255.).into_iter().chain([1.]).collect::<Vec<_>>().try_into().unwrap()).unwrap()
}

fn header_geometry(editor: &crate::color_editor::Editor) {
    let bounds = |widget: &gtk::Widget| widget.compute_bounds(&editor.dialog).unwrap();
    let middle = |b: gtk::graphene::Rect| b.y() + b.height() * 0.5;
    let pair = bounds(&editor.current.parent().unwrap());
    let pick = bounds(editor.pick.upcast_ref());
    assert_eq!(pair.height(), pick.height(), "Current and New match the eyedropper height");
    for (name, widget) in [("eyedropper", editor.pick.upcast_ref::<gtk::Widget>()), ("hex", editor.hex.stack.upcast_ref())] {
        assert!((middle(bounds(widget)) - middle(pair)).abs() <= 0.5, "{name} is centered on Current and New");
    }
    let page = bounds(&editor.dialog.child().unwrap());
    let title = bounds(editor.title.upcast_ref());
    assert!((title.x() + title.width() * 0.5 - page.x() - page.width() * 0.5).abs() <= 0.5, "the title is centered");
    let icon = editor.pick.first_child().unwrap();
    assert_eq!([gtk::Orientation::Horizontal, gtk::Orientation::Vertical].map(|o| icon.measure(o, -1).1), [24, 24], "the eyedropper icon fits its button");
}

fn dialog_size(editor: &crate::color_editor::Editor) -> (i32, i32) {
    let sheet = editor.dialog.child().unwrap();
    (sheet.width(), sheet.height())
}

fn seam_pixels(w: &Workspace, editor: &crate::color_editor::Editor) -> Vec<[u8; 4]> {
    let scale = w.window.surface().map_or(1., |surface| surface.scale()) as f32;
    let texture = crate::with_canvas_snapshot(w, || crate::snapshot_window(&w.window, scale));
    let mut downloader = gtk::gdk::TextureDownloader::new(&texture);
    downloader.set_format(gtk::gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let pair = editor.current.parent().unwrap().compute_bounds(&w.window).unwrap();
    let y = ((pair.y() + pair.height() * 0.5) * scale) as usize;
    let seam = ((pair.x() + pair.width() * 0.5) * scale).round() as usize;
    (seam - 3..seam + 3).map(|x| { let i = y * stride + x * 4; [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]] }).collect()
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native pointer input"]
fn native_color_editor_visual_audit() {
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    let app = native_test_app("art.capycanvas.ColorEditorAudit");
    let mut project = new_drawing_at(96, 96, SampleDepth::F16);
    paint_at_mut(&mut project, 0).original = Some(layer_core::color::source::rgba8_source([96, 96], |_, _| [RED[0], RED[1], RED[2], 255]));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new().timeout_secs(30);
    input.ready();
    for (index, hex) in [0x2F7B9C, 0xE0B84A, 0x6A4C93, 0x1B998B, 0xF46036, 0x2E294E, 0xC5D86D, 0xD7263D].into_iter().enumerate() {
        w.dispatch(UiAction::Color { action: ColorAction::Definition { color: srgb(hex) } });
        input.click(canvas_point(&w, [8. + index as f32 * 10., 8.]));
        pump(80);
    }
    let change = ui_session_mut(&w).reveal_panel(Panel::Palettes);
    w.changed(change);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::Color { action: ColorAction::Definition { color: srgb(0x2F7B9C) } });
        ready(&w);
        save_snapshot(&w, 120, || output.join(format!("reference-{theme:?}.png")));
        crate::color_editor::show(&w, ColorSlot::Foreground);
        let editor = editor(&w);
        audit(&w, &editor, &output, &format!("default-{theme:?}"));
        header_geometry(&editor);
        let size = dialog_size(&editor);
        for (row, family) in layer_ui::COLOR_FORM_FAMILIES.iter().enumerate() {
            for candidate in family.iter().chain(&family[..1]) {
                form(&w, row, *candidate);
                pump(60);
                assert_eq!(dialog_size(&editor), size, "{candidate:?} keeps the dialog size");
                if candidate != &family[0] { audit(&w, &editor, &output, &format!("form-{candidate:?}-{theme:?}")); }
            }
        }
        for (toggle, shape) in editor.shape_toggles.iter().zip(["circle", "square", "triangle"]).rev() {
            toggle.set_active(true);
            pump(60);
            assert_eq!(dialog_size(&editor), size, "{shape} keeps the dialog size");
            audit(&w, &editor, &output, &format!("shape-{shape}-{theme:?}"));
        }
        type_into(&w, "edit-color-hex", "#D7263D");
        let seam = seam_pixels(&w, &editor);
        println!("EDIT_COLOR_SEAM {theme:?} scale={} {seam:?}", w.window.surface().map_or(1., |surface| surface.scale()));
        assert!(seam.windows(2).filter(|pair| pair[0] != pair[1]).count() == 1, "Current and New meet without blended pixels: {seam:?}");
        editor.hex.show.emit_clicked();
        audit(&w, &editor, &output, &format!("hex-edit-{theme:?}"));
        editor.hex.entry.emit_activate();
        editor.rows[0].fields[1].show.emit_clicked();
        audit(&w, &editor, &output, &format!("value-edit-{theme:?}"));
        value(&w, 0, 1, "12x");
        assert_eq!(dialog_size(&editor), size, "an error keeps the dialog size");
        audit(&w, &editor, &output, &format!("error-{theme:?}"));
        editor.rows[0].fields[1].entry.set_text("38");
        editor.rows[0].fields[1].entry.emit_activate();
        let slide = |open: bool| {
            if open { editor.swatches.emit_clicked(); } else { editor.sheet_close.emit_clicked(); }
            let tops: Vec<f32> = (0..16).map(|_| { pump(25); editor.sheet.compute_bounds(&editor.sheet.parent().unwrap()).filter(|_| editor.sheet.is_visible()).map_or(f32::NAN, |b| b.y()) }).collect();
            assert!(tops.windows(2).all(|pair| if open { pair[1] <= pair[0] } else { pair[1] >= pair[0] || pair[1].is_nan() }), "the sheet slides {}: {tops:?}", if open { "up" } else { "down" });
            let mut steps: Vec<i32> = tops.iter().filter(|top| top.is_finite()).map(|top| top.round() as i32).collect();
            steps.dedup();
            assert!(steps.len() >= 3, "the sheet moves through intermediate positions: {tops:?}");
        };
        slide(true);
        let overlay = editor.sheet.parent().unwrap();
        let scroll = editor.sheet_scroll.compute_bounds(&overlay).unwrap();
        assert_eq!(editor.sheet.compute_bounds(&overlay).unwrap().y(), 0.);
        assert_eq!(scroll.y() + scroll.height(), overlay.height() as f32, "the swatch list reaches the divider");
        audit(&w, &editor, &output, &format!("sheet-{theme:?}"));
        slide(false);
        until(|| !editor.sheet.is_visible(), "the closed sheet hides");
        w.window.unmaximize();
        w.window.set_default_size(560, 900);
        pump(400);
        audit(&w, &editor, &output, &format!("narrow-{theme:?}"));
        w.window.maximize();
        pump(400);
        let english = w.localization().language();
        for language in [layer_ui::UiLanguage::German, english] {
            let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
            w.dispatch(UiAction::Preferences { action: layer_ui::PreferenceAction::Edit { id: layer_ui::PreferenceId::Language, value: layer_ui::PreferenceValue::Choice(choice) } });
            until(|| w.localization().language() == language, "audit language");
            pump(200);
            if language != english { audit(&w, &editor, &output, &format!("german-{theme:?}")); }
        }
        response(&w, "cancel");
    }
    let mut project = new_drawing_at(96, 96, SampleDepth::U8);
    paint_at_mut(&mut project, 0).original = Some(layer_core::color::source::rgba8_source([96, 96], |_, _| [RED[0], RED[1], RED[2], 255]));
    let sdr = Workspace::with_project(&app, Some((project, None)));
    sdr.window.maximize();
    sdr.window.present();
    ready(&sdr);
    for theme in [Theme::Light, Theme::Dark] {
        sdr.dispatch(UiAction::SetTheme { theme: Some(theme) });
        sdr.dispatch(UiAction::Color { action: ColorAction::Definition { color: srgb(0x2F7B9C) } });
        ready(&sdr);
        crate::color_editor::show(&sdr, ColorSlot::Foreground);
        let editor = editor(&sdr);
        audit(&sdr, &editor, &output, &format!("sdr-{theme:?}"));
        response(&sdr, "cancel");
    }
    sdr.window.destroy();
    input.finish();
    w.window.destroy();
    pump(100);
}
