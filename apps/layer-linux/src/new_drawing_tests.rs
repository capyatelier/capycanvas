//! The New drawing dialog on the private display: main page, presets, saving,
//! paper and paper color, in the light and dark themes.
use super::new_photo::{capture_ui, invoke, ready};
use super::*;

fn dialog(w: &Workspace) -> gtk::Widget { w.window.visible_dialog().expect("New drawing").upcast() }
pub(super) fn part<T: IsA<gtk::Widget>>(w: &Workspace, name: &str) -> T { named::<T>(&dialog(w), name) }
pub(super) fn open_new_drawing(w: &Rc<Workspace>) {
    invoke(w, CommandId::NewDocument);
    until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "new-drawing-dialog" && dialog.is_mapped()), "New drawing");
    pump(300);
}
pub(super) fn new_drawing_action(w: &Rc<Workspace>, action: NewDrawingAction) {
    w.dispatch(UiAction::NewDrawing { action });
    pump(150);
}
pub(super) fn close_new_drawing(w: &Rc<Workspace>, button: &str) {
    click(&part::<gtk::Button>(w, button));
    until(|| w.window.visible_dialog().is_none(), "New drawing closes");
}
fn press(w: &Workspace, name: &str) {
    click(&part::<gtk::Button>(w, name));
    pump(350);
}
fn tab(w: &Workspace, label: &str) {
    let tabs = part::<gtk::Widget>(w, "new-drawing-tabs");
    let toggle = widgets(tabs.upcast_ref()).filter_map(|widget| widget.downcast::<gtk::ToggleButton>().ok())
        .find(|toggle| toggle.label().as_deref() == Some(label)).unwrap_or_else(|| panic!("{label} tab"));
    toggle.set_active(true);
    pump(200);
}
fn pick(grid: &gtk::FlowBox, index: i32) {
    grid.select_child(&grid.child_at_index(index).expect("tile"));
    pump(150);
}
fn label(w: &Workspace, name: &str) -> String { part::<gtk::Label>(w, name).text().to_string() }
fn third_row_shows(grid: &gtk::FlowBox) -> bool {
    let scroller = grid.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
    let last = grid.child_at_index(12).unwrap();
    last.compute_bounds(&scroller).is_some_and(|bounds| bounds.y() >= 0. && bounds.y() + bounds.height() <= scroller.height() as f32)
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_new_drawing_search_composition_snapshots() { new_drawing_composition_snapshots(false); }

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_new_drawing_preset_name_composition_snapshots() { new_drawing_composition_snapshots(true); }

fn new_drawing_composition_snapshots(saving: bool) {
    let app = native_test_app("art.capycanvas.NewDrawingComposition");
    crate::text_language::install(&app, &Localizer::shared(UiLanguage::English));
    gtk::Settings::default().unwrap().set_gtk_enable_animations(false);
    let w = Workspace::with_project(&app, Some((new_drawing_at(256, 256, layer_core::color::SampleDepth::U8), None)));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new();
    input.ready();
    let query = || match state(&w).new_drawing.unwrap().drawer.unwrap() {
        NewDrawingDrawerView::Presets(browser) => browser.query,
        _ => panic!("Preset browser"),
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        open_new_drawing(&w);
        new_drawing_action(&w, NewDrawingAction::Presets);
        if saving { new_drawing_action(&w, NewDrawingAction::SaveAs); }
        let name = if saving { "new-drawing-save-name" } else { "new-drawing-search" };
        let field = part::<gtk::Widget>(&w, name);
        let edit = field.clone().dynamic_cast::<gtk::Editable>().unwrap();
        let text = descendant::<gtk::Text>(&field).unwrap();
        input.click(screen_point(&field, &w.window, [0.5, 0.5]));
        assert!(text.grab_focus());
        until(|| text.has_focus(), "Native text focus");
        edit.set_text("한");
        if !saving { until(|| query() == "한", "Committed preset query"); }
        edit.set_position(1);
        text.emit_preedit_changed("글");
        let before = w.localization().language();
        let target = if before == UiLanguage::Japanese { UiLanguage::English } else { UiLanguage::Japanese };
        let choice = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|language| *language == target).unwrap() as u32;
        w.dispatch(UiAction::SetBrushSize { value: state(&w).brush.diameter + 1. });
        w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit { id: PreferenceId::Language, value: PreferenceValue::Choice(choice) } });
        pump(100);
        assert_eq!(w.localization().language(), before);
        assert!(text.has_focus(), "{theme:?} {name}: snapshot moved focus during preedit");
        assert_eq!(edit.text(), "한");
        assert_eq!(edit.position(), 1);
        text.emit_preedit_changed("");
        let mut position = 1;
        edit.insert_text("글", &mut position);
        edit.set_position(2);
        if !saving { assert_eq!(query(), "한", "Native query is still debounced"); }
        w.new_drawing.borrow().upgrade().unwrap().render();
        assert_eq!(edit.text(), "한글", "{theme:?} {name}: snapshot replaced the pending native commit");
        until(|| w.localization().language() == target, "Deferred language publication");
        if !saving { until(|| query() == "한글", "Complete committed preset query"); }
        assert_eq!(part::<gtk::Widget>(&w, name), field);
        assert_eq!(edit.text(), "한글");
        assert_eq!(edit.position(), 2);
        assert_eq!(edit.selection_bounds(), None);
        assert!(text.has_focus(), "{theme:?} {name}: language publication moved focus");
        if saving {
            let model = part::<gtk::DropDown>(&w, "new-drawing-save-group").model().unwrap().downcast::<gtk::StringList>().unwrap();
            let labels: Vec<_> = (0..model.n_items()).map(|index| model.string(index).unwrap().to_string()).collect();
            let expected: Vec<_> = match state(&w).new_drawing.unwrap().drawer.unwrap() {
                NewDrawingDrawerView::Presets(browser) => browser.save.unwrap().groups.into_iter().map(|group| group.label.to_string()).collect(),
                _ => panic!("Preset browser"),
            };
            assert_eq!(labels, expected);
        }
        close_new_drawing(&w, "new-drawing-cancel");
        super::new_photo::finish(&w);
    }
    w.window.destroy();
    pump(100);
}

fn assert_startup_sheet(w: &Workspace) {
    let opened = ui_session(w).engine().document().clone();
    let screen = crate::new_drawing::screen(&w.window.surface().unwrap()).unwrap();
    let expected = layer_ui::startup_setup(Some(screen));
    assert_eq!(opened.composition().size, expected.extent());
    assert_eq!(opened.composition().size[1], screen.extent[1]);
    assert_eq!(opened.composition().resolution, Some(layer_core::ImageResolution::ppi(expected.resolution)));
    let paper = opened.bottom_paper().unwrap();
    assert_eq!(opened.scene().effect(paper).unwrap().constant_color(), Some(expected.paper.color));
    assert_eq!(layer_core::paper::light(opened.scene().effect(paper).unwrap()).unwrap().relief, 1.);
    assert_eq!(opened.paper(None).texture, layer_core::paper::built_in_paper("cold_press").unwrap().texture);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_startup_canvas_uses_current_monitor() {
    let app = native_test_app("art.capycanvas.StartupCanvas");
    let output = std::path::Path::new("../../artifacts/startup-defaults/gtk");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        let w = Workspace::new(&app);
        w.window.present();
        ready(&w);
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        assert_startup_sheet(&w);
        assert_eq!((state(&w).brush.preset, state(&w).brush.diameter), (layer_core::DefaultBrushPreset::Pencil as u32, 9.));
        assert_eq!(state(&w).colors.definition().rgba, [46. / 255., 46. / 255., 46. / 255., 1.]);
        capture_ui(&w, &output, &format!("{theme:?}.png"));
        w.window.destroy();
        pump(100);
    }
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_new_drawing_dialog_review() {
    let app = native_test_app("art.capycanvas.NewDrawingReview");
    let output = std::path::Path::new("../../artifacts/new-drawing-dialog/gtk");
    std::fs::create_dir_all(output).unwrap();
    let output = output.canonicalize().unwrap();
    let w = Workspace::with_project(&app, Some((new_drawing_at(256, 256, layer_core::color::SampleDepth::U8), None)));
    let created = Rc::new(RefCell::new(None));
    let result = created.clone();
    *w.open_document.borrow_mut() = Some(Rc::new(move |project, location| { result.replace(Some((project, location))); }));
    w.window.maximize();
    w.window.present();
    ready(&w);
    let mut input = RemoteInput::new().settle_ms(200).timeout_secs(30);
    input.ready();
    let paper = layer_ui::occurrence_token(ui_session(&w).engine().document().bottom_paper().unwrap());
    w.dispatch(UiAction::SelectLayer { id: paper });
    ready(&w);
    crate::paper_picker::open(&w, paper);
    until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "texture-dialog" && dialog.is_mapped()), "Choose Texture");
    pump(400);
    assert!(third_row_shows(&part::<gtk::FlowBox>(&w, "texture-grid")), "Choose Texture shows three full rows");
    w.window.visible_dialog().unwrap().close();
    until(|| w.window.visible_dialog().is_none(), "Choose Texture closes");
    let compositor = std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        let theme = format!("{theme:?}").to_lowercase();
        let shot = |step: &str| capture_ui(&w, &output, &format!("{theme}-{step}.png"));
        invoke(&w, CommandId::NewDocument);
        until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "new-drawing-dialog"), "New drawing");
        pump(500);
        shot("1-main");
        assert_eq!(state(&w).new_drawing.as_ref().unwrap().page.orientation, Orientation::Landscape);
        let height = |name: &str| {
            let widget = part::<gtk::Widget>(&w, name);
            (name.to_string(), widget.compute_bounds(&widget).map_or(0, |bounds| bounds.height().round() as i32))
        };
        let inputs: Vec<_> = ["new-drawing-name", "new-drawing-width", "new-drawing-height", "new-drawing-unit", "new-drawing-orientation",
            "new-drawing-resolution-field", "new-drawing-space", "new-drawing-depth", "new-drawing-blending", "new-drawing-presets-button", "new-drawing-cancel", "new-drawing-create"].map(height).into();
        assert!(inputs.iter().all(|(_, size)| *size == 34), "inputs and buttons share one height: {inputs:?}");
        let tiles = ["new-drawing-texture", "new-drawing-paper-color"].map(height);
        assert!(tiles.iter().all(|(_, size)| *size == 44), "paper tiles: {tiles:?}");
        let widths = ["new-drawing-width", "new-drawing-height"].map(|name| part::<gtk::Widget>(&w, name).width());
        assert_eq!(widths[0], widths[1], "W and H fields match");
        let bounds = |name: &str| part::<gtk::Widget>(&w, name).compute_bounds(&dialog(&w)).unwrap();
        let [width, link, height] = ["new-drawing-width", "new-drawing-link", "new-drawing-height"].map(bounds);
        assert_eq!((link.x(), link.x() + link.width(), link.height()), (width.x() + width.width(), height.x(), width.height()), "the link fills the gap between W and H");
        let field = part::<gtk::Widget>(&w, "new-drawing-resolution-field");
        let closed = (field.width(), field.height());
        input.click(screen_point(&part::<gtk::Widget>(&w, "new-drawing-resolution-choices"), &w.window, [0.5, 0.5]));
        pump(300);
        assert!(part::<gtk::Popover>(&w, "new-drawing-resolution-menu").is_visible(), "a click opens common resolutions");
        assert_eq!((field.width(), field.height()), closed, "opening common resolutions keeps the field's size");
        if compositor { input.perform(serde_json::json!([{ "wait_ms": 250 }, { "capture": format!("{theme}-1b-resolution-menu") }])); }
        part::<gtk::MenuButton>(&w, "new-drawing-resolution-choices").popdown();
        pump(200);

        press(&w, "new-drawing-preview");
        assert!(part::<gtk::Widget>(&w, "new-drawing-browser").is_visible(), "the preview opens all presets");
        press(&w, "new-drawing-browser-cancel");
        press(&w, "new-drawing-presets-button");
        let tiles = part::<gtk::FlowBox>(&w, "new-drawing-presets");
        let across = (0..8).filter_map(|index| tiles.child_at_index(index)).filter(|tile| tile.compute_bounds(&tiles).is_some_and(|bounds| bounds.y() < 1.)).count();
        assert_eq!(across, 5, "five presets across on desktop");
        shot("2-presets");
        tab(&w, "Digital");
        let digital = part::<gtk::FlowBox>(&w, "new-drawing-presets");
        assert_eq!(digital.observe_children().n_items(), 10);
        let full_hd = digital.child_at_index(1).unwrap();
        input.click(screen_point(full_hd.upcast_ref(), &w.window, [0.5, 0.5]));
        input.click(screen_point(part::<gtk::Button>(&w, "new-drawing-select").upcast_ref(), &w.window, [0.5, 0.5]));
        pump(200);
        assert!(state(&w).new_drawing.as_ref().unwrap().setup.paper.texture.is_none());
        shot("2-digital-no-paper");
        input.click(screen_point(part::<gtk::Button>(&w, "new-drawing-presets-button").upcast_ref(), &w.window, [0.5, 0.5]));
        pump(200);
        tab(&w, "Mixed media");
        shot("2-presets-mixed-media");
        tab(&w, "Pastel");
        let grid = part::<gtk::FlowBox>(&w, "new-drawing-presets");
        pick(&grid, 9);
        assert_eq!(label(&w, "new-drawing-summary-name"), "Chalkboard");
        shot("3-presets-pastel");
        press(&w, "new-drawing-save-as");
        part::<gtk::Entry>(&w, "new-drawing-save-name").set_text("Studio chalk");
        pump(150);
        shot("4-save-as");
        press(&w, "new-drawing-save-cancel");
        press(&w, "new-drawing-select");
        assert!(label(&w, "new-drawing-caption").is_empty() || part::<gtk::Label>(&w, "new-drawing-caption").label().contains("Chalkboard"));
        assert_eq!(label(&w, "new-drawing-texture-relief"), "50 %");
        assert_eq!(label(&w, "new-drawing-summary-details"), "sRGB 8-bit SDR · Chalkboard 50 %, Chalkboard");
        shot("5-chalkboard");

        press(&w, "new-drawing-texture");
        let papers = part::<gtk::FlowBox>(&w, "new-drawing-papers");
        let import = part::<gtk::FlowBoxChild>(&w, "new-drawing-paper-import");
        assert_eq!(import.index(), layer_core::paper::built_in_papers().len() as i32 + 1, "Import follows the last paper");
        let field = part::<gtk::Widget>(&w, "new-drawing-paper-relief-field");
        assert_eq!(field.compute_bounds(&field).map(|bounds| bounds.height().round()), Some(34.), "the visibility field matches the footer buttons");
        assert_eq!(part::<crate::number_control::NumberControl>(&w, "new-drawing-paper-relief").value(), 50., "Chalkboard's texture visibility");
        shot("6-paper");
        papers.select_child(&import);
        pump(150);
        let marked = match state(&w).new_drawing.and_then(|view| view.drawer) { Some(NewDrawingDrawerView::Paper { tiles, .. }) => tiles.iter().position(|tile| tile.selected), _ => None };
        assert_eq!(papers.selected_children().first().map(|tile| tile.index() as usize), marked, "Import is an action, so the paper stays marked");
        new_drawing_action(&w, NewDrawingAction::PaperRelief { value: 150. });
        pick(&papers, 0);
        assert!(!part::<gtk::Widget>(&w, "new-drawing-paper-relief-field").is_sensitive(), "No paper has no texture to show");
        shot("6b-no-paper");
        let scroller = papers.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>().unwrap();
        scroller.vadjustment().set_value(scroller.vadjustment().upper());
        pump(200);
        shot("6c-import");
        let chooser = || gtk::Window::list_toplevels().into_iter().find(|window| window.type_().name() == "GtkFileChooserDialog" && window.is_visible())
            .and_downcast::<gtk::Window>();
        let marked = papers.selected_children().first().map(|tile| tile.index());
        input.click(screen_point(import.upcast_ref(), &w.window, [0.5, 0.3]));
        until(|| chooser().is_some(), "a click on Import opens the image chooser");
        assert_eq!(papers.selected_children().first().map(|tile| tile.index()), marked, "Import never becomes the marked paper");
        chooser().unwrap().close();
        until(|| chooser().is_none(), "the image chooser closes");
        pump(200);
        pick(&papers, 9);
        press(&w, "new-drawing-paper-use");
        assert_eq!(state(&w).new_drawing.as_ref().map(|view| view.setup.paper.relief), Some(150));
        press(&w, "new-drawing-paper-color");
        shot("7-paper-color");
        let layout = |w: &Workspace| ["edit-color-wheel", "edit-color-hex", "edit-color-value-0-0", "edit-color-value-1-2", "edit-color-value-2-0", "edit-color-value-2-2", "new-drawing-color-mode"]
            .map(|name| {
                let bounds = part::<gtk::Widget>(w, name).compute_bounds(&dialog(w)).unwrap();
                (name, bounds.x().round() as i32, bounds.width().round() as i32)
            });
        let before = layout(&w);
        click(&part::<gtk::Button>(&w, "new-drawing-paper-color-0"));
        pump(200);
        assert_eq!(layout(&w), before, "values with more digits keep every field in place");
        let visible = |widget: gtk::Widget| {
            let bounds = widget.compute_bounds(&widget).unwrap();
            (bounds.width().round() as i32, bounds.height().round() as i32)
        };
        let sizes = [visible(part::<gtk::Widget>(&w, "new-drawing-paper-color-1").first_child().unwrap()), visible(part::<gtk::Widget>(&w, "new-drawing-add-paper-color"))];
        assert_eq!(sizes[0], sizes[1], "the add tile matches the swatches");
        assert_eq!(sizes[0].0, sizes[0].1, "swatches are square");
        part::<gtk::ToggleButton>(&w, "new-drawing-color-mode-transparent").set_active(true);
        pump(200);
        shot("8-transparent");
        press(&w, "new-drawing-color-cancel");

        part::<gtk::DropDown>(&w, "new-drawing-depth").set_selected(2);
        pump(150);
        assert!(!part::<gtk::DropDown>(&w, "new-drawing-blending").is_sensitive());
        part::<gtk::DropDown>(&w, "new-drawing-unit").set_selected(3);
        pump(150);
        shot("9-pixels-hdr");
        part::<gtk::DropDown>(&w, "new-drawing-depth").set_selected(0);
        click(&part::<gtk::Button>(&w, "new-drawing-cancel"));
        until(|| w.window.visible_dialog().is_none(), "New drawing closes");
    }

    invoke(&w, CommandId::NewDocument);
    until(|| w.window.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "new-drawing-dialog"), "New drawing");
    pump(300);
    part::<gtk::Entry>(&w, "new-drawing-name").set_text("Harbor study");
    press(&w, "new-drawing-create");
    until(|| created.borrow().is_some(), "the new drawing");
    let (project, location) = created.borrow_mut().take().unwrap();
    assert!(location.is_none());
    assert_eq!(project.composition().resolution, Some(layer_core::ImageResolution::ppi(300)));
    assert_eq!(state(&w).settings.new_document.recent.first(), Some(&layer_ui::PresetKey::BuiltIn("sketchbook-a4".into())));

    w.window.unmaximize();
    w.window.set_default_size(620, 860);
    pump(300);
    invoke(&w, CommandId::NewDocument);
    until(|| w.window.visible_dialog().is_some(), "New drawing");
    pump(500);
    capture_ui(&w, &output, "narrow.png");
    press(&w, "new-drawing-texture");
    capture_ui(&w, &output, "narrow-paper.png");
    press(&w, "new-drawing-paper-cancel");
    click(&part::<gtk::Button>(&w, "new-drawing-cancel"));
    let fresh = Workspace::new(&app);
    fresh.window.present();
    ready(&fresh);
    assert_startup_sheet(&fresh);
    fresh.window.destroy();
    input.finish();
    w.window.destroy();
    pump(100);
}
