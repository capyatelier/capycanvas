use super::*;
use super::workspace_header::Driver;

#[test]
#[ignore = "private compositor: --native-test=native_new_drawing_preview_spacing --native-storage; viewport/theme external"]
fn native_new_drawing_preview_spacing() {
    let mut d = super::phone_motion::setup("art.capycanvas.NewDrawingPreviewSpacing");
    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (_, frame) = dialog_frame(&d);
    let caption = find_named(&frame, "new-drawing-capacity").unwrap();
    let name = find_named(&frame, "new-drawing-name").unwrap();
    let left = caption.parent().unwrap().parent().unwrap();
    let body = left.parent().unwrap().downcast::<gtk::Box>().unwrap();
    let caption = caption.compute_bounds(&frame).unwrap();
    let name = name.compute_bounds(&frame).unwrap();
    if body.orientation() == gtk::Orientation::Vertical {
        let gap = name.y() - caption.y() - caption.height();
        assert!((gap - body.spacing() as f32).abs() <= 1., "stacked preview keeps section spacing: {gap}");
        assert!(!left.compute_expand(gtk::Orientation::Vertical), "stacked preview uses its content height");
    } else {
        assert!(name.x() >= caption.x() + caption.width(), "wide preview stays beside the form");
    }
    super::phone::capture(&mut d, "new-drawing-preview-spacing");
    crate::page_dialog::close_dialog(&d.w.visible_dialog().unwrap());
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_preferences_inline_controls_fit --native-storage; viewport/theme external"]
fn native_preferences_inline_controls_fit() {
    let mut d = super::phone_motion::setup("art.capycanvas.PreferencesInlineFit");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (dialog, frame) = dialog_frame(&d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    let appearance = SettingsPage::Appearance.localized_title(&d.w.localization());
    let point = d.point(&mapped_label(&frame, &appearance).unwrap());
    d.input.perform(serde_json::json!([{"touch":"down","point":point},{"touch":"up"}]));
    until(|| split.shows_content(), "Preferences opens Appearance");
    assert!(dialog.has_css_class("touch-input"), "native touch changes the dialog's input styling");
    assert!(frame.has_css_class("touch-input"), "the presented page mirrors the dialog's touch styling");
    assert!(frame.has_css_class("phone-dialog-page") && frame.has_css_class("vertical"), "mirroring preserves the page's own classes");
    super::phone::capture(&mut d, "preferences-touch-style");
    d.input.perform(serde_json::json!([{"point":[12.,36.]}]));
    until(|| !dialog.has_css_class("touch-input"), "native mouse motion restores pointer styling");
    assert!(!frame.has_css_class("touch-input"), "the page removes the dialog's old touch styling");
    if split.is_collapsed() { page_header(&frame); }
    super::phone::capture(&mut d, "preferences-inline-fit");
    for group in descendants::<adw::PreferencesGroup>(&frame).into_iter().filter(|group| group.is_mapped()) {
        let bounds = group.compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "Appearance group fits: {bounds:?}");
    }
    let choices = widgets(&frame).filter(|widget| widget.is_mapped() && widget.widget_name().starts_with("setting-")
        && (widget.widget_name().contains("-circle-") || widget.widget_name().contains("-swatch-"))).collect::<Vec<_>>();
    assert!(choices.iter().any(|widget| widget.widget_name().contains("-circle-"))
        && choices.iter().any(|widget| widget.widget_name().contains("-swatch-")), "Appearance presents both choice kinds");
    for widget in &choices {
        let bounds = widget.compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "Appearance control fits: {bounds:?}");
    }
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_new_drawing_dimension_controls_fit --native-storage; viewport/theme external"]
fn native_new_drawing_dimension_controls_fit() {
    let mut d = super::phone_motion::setup("art.capycanvas.NewDrawingDimensionFit");
    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (dialog, frame) = dialog_frame(&d);
    super::phone::capture(&mut d, "new-drawing-dimensions-fit");
    for name in ["new-drawing-name", "new-drawing-width", "new-drawing-link", "new-drawing-height", "new-drawing-unit", "new-drawing-orientation",
        "new-drawing-resolution", "new-drawing-size", "new-drawing-space", "new-drawing-depth", "new-drawing-blending", "new-drawing-texture", "new-drawing-paper-color"] {
        let control = find_named(&frame, name).unwrap();
        let bounds = control.compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "New drawing control fits: {name} {bounds:?}");
    }
    let unit = find_named(&frame, "new-drawing-unit").unwrap().downcast::<gtk::DropDown>().unwrap();
    let value = unit.selected_item().and_downcast::<gtk::StringObject>().unwrap().string();
    let label = mapped_label(unit.upcast_ref(), &value).unwrap().downcast::<gtk::Label>().unwrap();
    assert!(!label.layout().is_ellipsized(), "the dimension unit stays readable");
    assert!(!find_named(&frame, "new-drawing-size").unwrap().downcast::<gtk::Label>().unwrap().layout().is_ellipsized(), "the pixel size stays readable");
    super::histogram::scroll_to(&find_named(&frame, "new-drawing-paper-color").unwrap());
    super::phone::capture(&mut d, "new-drawing-dimensions-fit-bottom");
    let window = d.w.window.clone();
    let settled_sheet = |name: &str| {
        let sheet = find_named(&frame, name).unwrap();
        drop(idle_paint(&window));
        until(|| sheet.is_mapped() && sheet.parent().and_then(|parent| sheet.compute_bounds(&parent))
            .is_some_and(|bounds| bounds.y().abs() <= 1.), "the New Drawing sheet reaches its open position");
        drop(idle_paint(&window));
        sheet
    };
    let footer_fits = |name: &str| {
        let action = find_named(&frame, name).unwrap();
        let footer = std::iter::successors(action.parent(), |parent| parent.parent())
            .find(|parent| parent.has_css_class("dialog-footer")).unwrap();
        assert!(footer.is_mapped());
        for child in widgets(&footer).filter(|child| child.is_mapped()) {
            let bounds = child.compute_bounds(&frame).unwrap();
            assert!(bounds.x() >= -1. && bounds.x() + bounds.width() <= frame.width() as f32 + 1.,
                "New Drawing footer child fits: {name} {} {bounds:?}", child.widget_name());
        }
    };
    for (opener, sheet, action, cancel, tag) in [
        ("new-drawing-presets-button", "new-drawing-browser", "new-drawing-select", "new-drawing-browser-cancel", "presets"),
        ("new-drawing-texture", "new-drawing-paper-drawer", "new-drawing-paper-use", "new-drawing-paper-cancel", "paper"),
        ("new-drawing-paper-color", "new-drawing-color-drawer", "new-drawing-color-use", "new-drawing-color-cancel", "paper-color"),
    ] {
        let opener = find_named(&frame, opener).unwrap();
        super::histogram::scroll_to(&opener);
        d.click(&opener);
        let sheet = settled_sheet(sheet);
        capture(&mut d, &format!("new-drawing-footer-{tag}"));
        footer_fits(action);
        if tag == "presets" {
            let summary = named::<gtk::Label>(&frame, "new-drawing-summary-name");
            assert!(summary.is_mapped() && !summary.layout().is_ellipsized(), "the preset summary name stays readable");
            d.click(&find_named(&frame, "new-drawing-save-as").unwrap());
            let name = named::<gtk::Entry>(&frame, "new-drawing-save-name");
            let stack = name.ancestor(gtk::Stack::static_type()).unwrap().downcast::<gtk::Stack>().unwrap();
            until(|| name.is_mapped() && !stack.is_transition_running(), "Save Preset finishes opening");
            drop(idle_paint(&window));
            capture(&mut d, "new-drawing-footer-save-preset");
            footer_fits("new-drawing-save-confirm");
            let natural = name.measure(gtk::Orientation::Horizontal, -1).1;
            let width = name.compute_bounds(&name.parent().unwrap()).unwrap().width();
            assert!(width + 1. >= natural as f32, "Save Preset name keeps its natural width: {width} < {natural}");
            d.click(&find_named(&frame, "new-drawing-save-cancel").unwrap());
            until(|| !name.is_mapped() && !stack.is_transition_running(), "Save Preset returns to browsing");
        } else if tag == "paper" {
            let field = find_named(&frame, "new-drawing-paper-relief-field").unwrap();
            let label = field.parent().unwrap().first_child().unwrap().downcast::<gtk::Label>().unwrap();
            assert!(label.is_mapped() && !label.layout().is_ellipsized(), "Texture visibility stays readable");
        } else if tag == "paper-color" {
            let well = find_named(&frame, "new-drawing-color-mode").unwrap();
            let bounds = well.compute_bounds(&frame).unwrap();
            assert!(bounds.x() >= -1. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "the paper color switch fits: {bounds:?}");
            let labels = descendants::<gtk::Label>(&well).into_iter().filter(|label| label.is_mapped()).collect::<Vec<_>>();
            assert_eq!(labels.len(), 2, "Color and Transparent are both visible");
            assert!(labels.iter().all(|label| !label.layout().is_ellipsized()), "paper color switch labels stay readable");
        }
        d.click(&find_named(&frame, cancel).unwrap());
        until(|| !sheet.is_mapped(), "the New Drawing sheet closes before the next footer");
    }
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_export_narrow_bounds --native-storage; viewport/theme external"]
fn native_export_narrow_bounds() {
    let mut d = super::phone_motion::setup("art.capycanvas.ExportNarrowBounds");
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (dialog, frame) = dialog_frame(&d);
    until(|| find_named(&frame, "color-preview-after").and_then(|widget| widget.downcast::<gtk::Picture>().ok())
        .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its preview");
    super::phone::capture(&mut d, "export-narrow-bounds");
    let headers = descendants::<adw::HeaderBar>(&frame).into_iter().filter(|header| header.is_mapped()).collect::<Vec<_>>();
    assert_eq!(headers.len(), 1, "Export has one mapped navigation header");
    let minimum = headers[0].measure(gtk::Orientation::Horizontal, -1).0;
    assert!(minimum <= d.w.window.width(), "Export header minimum {minimum} fits the window width {}", d.w.window.width());
    for name in ["export-format", "export-open-size", "export-open-color", "color-preview-before", "color-preview-after"] {
        let bounds = find_named(&frame, name).unwrap().compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "Export control fits: {name} {bounds:?}");
    }
    if ui_session(&d.w).phone_layout().is_some() { page(&d); }
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_editor_close_measurement --native-storage; phone viewport/theme external"]
fn native_phone_editor_close_measurement() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditorCloseMeasurement");
    let warnings = bounds::size_warnings();
    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, _) = dialog_frame(&d);
    capture(&mut d, "edit-color-before-close");
    close(&d, &dialog);
    let warnings = warnings.lock().unwrap().clone();
    assert!(warnings.is_empty(), "closing an editor page never measures its body below its minimum: {warnings:?}");
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_edit_color_narrow_bounds --native-storage; viewport/theme external"]
fn native_edit_color_narrow_bounds() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditColorNarrowBounds");
    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, frame) = dialog_frame(&d);
    let last = find_named(&frame, "edit-color-copy-2").unwrap();
    super::histogram::scroll_to(&last);
    super::phone::capture(&mut d, "edit-color-narrow-values");
    let mut controls = vec!["edit-color-hex".to_owned(), "edit-color-hex-copy".to_owned()];
    for row in 0..3 {
        controls.push(format!("edit-color-copy-{row}"));
        for column in 0..3 { controls.push(format!("edit-color-value-{row}-{column}")); }
    }
    for name in controls {
        let widget = find_named(&frame, &name).unwrap();
        let bounds = widget.compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32, "Color control fits: {name} {bounds:?}");
        for label in descendants::<gtk::Label>(&widget).into_iter().filter(|label| label.is_mapped()) {
            assert!(!label.layout().is_ellipsized(), "Color value stays readable: {name}");
        }
    }
    let editor = d.w.color_editors.borrow().last().unwrap().clone();
    let field = &editor.form.rows[0].fields[0];
    super::histogram::scroll_to(field.show.upcast_ref());
    d.click(field.show.upcast_ref());
    until(|| field.editing(), "the color value opens for editing");
    field.entry.set_text("0.25");
    let draft = editor.form.draft.borrow().value();
    let parent = field.stack.parent().unwrap();
    for width in [1000, 264, 1000, 264] {
        editor.form.body.measure(gtk::Orientation::Vertical, width);
        assert_eq!(field.stack.parent().as_ref(), Some(&parent), "reflow keeps the number's parent");
        assert!(field.editing() && field.entry.text() == "0.25", "reflow preserves the unfinished number edit");
        assert_eq!(editor.form.draft.borrow().value(), draft, "reflow never commits the unfinished value");
        assert!(gtk::prelude::RootExt::focus(&d.w.window).is_some_and(|focus| focus == field.entry || focus.is_ancestor(&field.entry)),
            "reflow keeps native focus within the number entry");
    }
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_toolbar_manager_narrow_bounds --native-storage; viewport/theme external"]
fn native_toolbar_manager_narrow_bounds() {
    let mut d = super::phone_motion::setup("art.capycanvas.ToolbarManagerNarrowBounds");
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::ThisWorkspace);
    let (dialog, frame) = dialog_frame(&d);
    super::phone::capture(&mut d, "toolbar-manager-narrow-bounds");
    let header = page_header(&frame).compute_bounds(&d.w.window).unwrap();
    assert!(header.y() >= 0. && header.y() + header.height() <= d.w.window.height() as f32,
        "the manager header stays inside the window: {header:?}");
    let body = crate::page_dialog::content(&dialog).unwrap();
    assert!(body.measure(gtk::Orientation::Horizontal, -1).0 <= frame.width(), "toolbar manager body fits the page");
    for tab in [layer_workspace::ManagerPage::ThisWorkspace, layer_workspace::ManagerPage::ToolbarLibrary] {
        let label = mapped_label(&frame, &tab.label(&d.w.localization())).unwrap();
        let bounds = label.compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32, "toolbar manager tab fits: {bounds:?}");
    }
    page(&d);
    let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
    let row = list.row_at_index(0).unwrap();
    super::histogram::scroll_to(row.upcast_ref());
    d.click(row.upcast_ref());
    let split = descendants::<adw::NavigationSplitView>(&frame).into_iter().find(|view| view.is_mapped()).unwrap();
    until(|| split.shows_content(), "the native manager opens its detail page");
    super::phone::capture(&mut d, "toolbar-manager-detail-minimum");
    let details = find_named(&frame, "workspace-manager-details").unwrap();
    assert!(details.measure(gtk::Orientation::Horizontal, -1).0 <= details.parent().unwrap().width(), "manager detail minimum fits its allocated parent");
    close(&d, &dialog);
    d.finish();
}

fn page_header(frame: &gtk::Widget) -> adw::HeaderBar {
    until(|| descendants::<adw::HeaderBar>(frame).into_iter().filter(|header| header.is_mapped()).count() == 1,
        "native navigation leaves one mapped header");
    let headers = descendants::<adw::HeaderBar>(frame).into_iter().filter(|header| header.is_mapped()).collect::<Vec<_>>();
    assert_eq!(headers.len(), 1, "the page has exactly one mapped header");
    let header = headers.into_iter().next().unwrap();
    let row = std::iter::successors(header.parent(), |widget| widget.parent()).take_while(|widget| widget != frame)
        .find(|widget| widget.has_css_class("top-bar")).unwrap_or_else(|| header.clone().upcast());
    let bounds = row.compute_bounds(frame).unwrap();
    assert!(bounds.y().abs() <= 1., "the page header starts at the top: {bounds:?}");
    header
}

fn icon_button(header: &adw::HeaderBar, icon: &str) -> Option<gtk::Button> {
    descendants::<gtk::Button>(header.upcast_ref()).into_iter().find(|button| button.is_mapped()
        && (button.icon_name().as_deref() == Some(icon)
            || descendants::<gtk::Image>(button.upcast_ref()).iter().any(|image| image.icon_name().as_deref() == Some(icon))))
}

fn navigation_header(d: &Driver, frame: &gtk::Widget, root: bool) -> adw::HeaderBar {
    let header = page_header(frame);
    let close = icon_button(&header, "window-close-symbolic");
    assert_eq!(close.is_some(), root, "only the root navigation header has Close");
    assert_eq!(icon_button(&header, "go-previous-symbolic").is_some(), !root, "subpages keep their native Back button");
    if let Some(close) = close {
        let label = layer_ui::CommonCopy::new(&d.w.localization()).cancel;
        assert_eq!(close.tooltip_text().as_deref(), Some(label.as_ref()));
        assert!(gtk::test_accessible_has_property(&close, gtk::AccessibleProperty::Label));
    }
    if let Some(action) = gtk::prelude::GtkWindowExt::default_widget(&d.w.window) {
        assert!(action.is_mapped() && action.is_ancestor(&header), "the visible header owns the default action");
    }
    header
}

pub(super) fn done_button(d: &Driver, header: &adw::HeaderBar) -> gtk::Button {
    let copy = layer_ui::CommonCopy::new(&d.w.localization());
    let buttons = descendants::<gtk::Button>(header.upcast_ref()).into_iter()
        .filter(|button| button.is_mapped() && button.label().as_deref() == Some(copy.done.as_ref())).collect::<Vec<_>>();
    assert_eq!(buttons.len(), 1, "the header has exactly one Done control");
    let done = buttons.into_iter().next().unwrap();
    assert!(done.has_css_class("suggested-action"));
    assert_eq!(done.accessible_role(), gtk::AccessibleRole::Button);
    assert_eq!(done.child().and_downcast::<gtk::Label>().unwrap().text(), copy.done.as_ref());
    let bounds = done.compute_bounds(header).unwrap();
    assert!(bounds.x() > header.width() as f32 / 2., "Done is at the header end: {bounds:?} in {}x{}", header.width(), header.height());
    done
}

pub(super) fn done_header(d: &Driver, frame: &gtk::Widget, root: bool) -> adw::HeaderBar {
    let header = page_header(frame);
    assert!(icon_button(&header, "window-close-symbolic").is_none(), "Done pages do not duplicate a start X");
    assert_eq!(icon_button(&header, "go-previous-symbolic").is_some(), !root, "Done subpages retain Back");
    done_button(d, &header);
    header
}

pub(super) fn dialog_frame(d: &Driver) -> (adw::Dialog, gtk::Widget) {
    until(|| d.w.visible_dialog().is_some_and(|dialog| {
        let frame = crate::page_dialog::parent(&dialog);
        frame.is_mapped() && frame.width() > 0 && frame.height() > 0
            && !d.w.pages.root.is_transition_running()
    }), "the editor dialog is mapped and settled");
    let dialog = d.w.visible_dialog().unwrap();
    let frame = crate::page_dialog::parent(&dialog);
    (dialog, frame)
}

fn page(d: &Driver) -> (adw::Dialog, gtk::Widget) {
    let (dialog, frame) = dialog_frame(d);
    assert!(!dialog.is_mapped(), "the original native dialog is detached");
    let bounds = frame.compute_bounds(&d.w.window).unwrap();
    assert!(bounds.x().abs() <= 1. && bounds.y().abs() <= 1., "{bounds:?}");
    assert!((bounds.width() - d.w.window.width() as f32).abs() <= 1., "{bounds:?}");
    assert!((bounds.height() - d.w.window.height() as f32).abs() <= 1., "{bounds:?}");
    let header = page_header(&frame);
    assert!(gtk::prelude::GtkWindowExt::focus(&d.w.window).is_none_or(|focus| focus.downcast_ref::<gtk::Button>().is_none_or(|button| button.icon_name().as_deref() != Some("window-close-symbolic"))), "opening never focuses close");
    for button in descendants::<gtk::Button>(header.upcast_ref()).into_iter().filter(|b| b.is_mapped()) {
        if button.has_css_class("suggested-action") { assert!(button.label().is_some_and(|label| !label.is_empty()), "the header primary action has its existing label"); }
        let b = button.compute_bounds(&frame).unwrap();
        assert!(b.x() >= 0. && b.y() >= 0. && b.x() + b.width() <= bounds.width() + 1.);
    }
    if let Some(default) = gtk::prelude::GtkWindowExt::default_widget(&d.w.window) {
        assert!(default.is_mapped() && (default == frame || default.is_ancestor(&frame)), "the page owns its mapped default action: default={} mapped={} parent={:?} frame={} dialog={}", default.widget_name(), default.is_mapped(), default.parent().map(|parent| parent.widget_name()), frame.widget_name(), dialog.widget_name());
    }
    let title = header.ancestor(adw::NavigationPage::static_type()).and_downcast::<adw::NavigationPage>()
        .map_or_else(|| dialog.title(), |page| page.title());
    if !title.is_empty() { assert_eq!(descendants::<gtk::Label>(header.upcast_ref()).into_iter().filter(|label| label.is_mapped() && label.text() == title).count(), 1, "the visible header has its native page title once"); }
    (dialog, frame)
}

pub(super) fn capture(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        let capture_name = format!("pages-{}x{}-{}-{name}",d.w.surface.width(),d.w.surface.height(),format!("{:?}",state(&d.w).theme).to_lowercase());
        d.input.perform(serde_json::json!([{ "capture_window": {
            "name": &capture_name,
            "maximized": d.w.window.is_maximized()
        } }]));
        super::phone_motion::export_allocations(&d.w,name,&capture_name);
    }
}

fn close(d: &Driver, dialog: &adw::Dialog) {
    crate::page_dialog::close_dialog(dialog);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running() && !d.w.servicing.get(), "the page closes");
}

fn keys(d: &mut Driver, key: u32) {
    d.input.perform(serde_json::json!([{ "key": key, "down": true }, { "key": key, "down": false }]));
}

fn export_page(d: &mut Driver, frame: &gtk::Widget, tag: &str) {
    let nav = named::<adw::NavigationView>(frame, "export-navigation");
    let row = named::<adw::ActionRow>(frame, &format!("export-open-{tag}"));
    let scroll = named::<gtk::ScrolledWindow>(frame, "export-main-scroll");
    until(|| find_named(frame, "color-preview-after").and_then(|widget| widget.downcast::<gtk::Picture>().ok())
        .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its preview before native navigation");
    super::histogram::scroll_to(row.upcast_ref());
    drop(idle_paint(&d.w.window));
    until(|| row.compute_bounds(&scroll).is_some_and(|bounds| {
        let visible = bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.;
        row.is_mapped() && bounds.width() > 0. && bounds.height() > 0. && visible
    }), "Export link scrolls into its native viewport");
    d.click(row.upcast_ref());
    until(|| nav.visible_page_tag().as_deref() == Some(tag), &format!("Export opens its native {tag} subpage"));
    page_header(frame);
}

pub(super) fn inventory(d: &mut Driver) {
    inventory_with(d, capture);
}

fn first_editor_pages(d: &mut Driver, visit: &mut impl FnMut(&mut Driver, &str)) {
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (dialog, frame) = dialog_frame(d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    if split.is_collapsed() && split.shows_content() {
        d.click(icon_button(&page_header(&frame), "go-previous-symbolic").unwrap().upcast_ref());
        until(|| !split.shows_content(), "Preferences returns to its root list");
        page_header(&frame);
    }
    visit(d, "preferences-root");
    let appearance = SettingsPage::Appearance.localized_title(&d.w.localization());
    let sidebar = named::<adw::ToolbarView>(&frame,"preferences-sidebar");
    d.click(&mapped_label(sidebar.upcast_ref(), &appearance).unwrap());
    until(|| split.shows_content(), "Preferences opens its page");
    if split.is_collapsed() { page_header(&frame); }
    visit(d, "preferences-subpage");
    close(d, &dialog);

    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (dialog, _) = dialog_frame(d);
    visit(d, "new-drawing");
    close(d, &dialog);
    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, _) = dialog_frame(d);
    visit(d, "color");
    close(d, &dialog);
}

#[test]
#[ignore = "private compositor: --native-test=native_dialog_layout --native-storage; 600/700/800/1024x900 light/dark"]
fn native_dialog_layout() {
    let mut d = super::phone_motion::setup("art.capycanvas.DialogLayout");
    assert!(matches!([d.w.surface.width(), d.w.surface.height()], [600, 900] | [700, 900] | [800, 900] | [1024, 900]));
    first_editor_pages(&mut d, &mut capture);
    d.finish();
}

fn inventory_with(d: &mut Driver, mut visit: impl FnMut(&mut Driver, &str)) {
    first_editor_pages(d, &mut visit);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (dialog, frame) = dialog_frame(d);
    until(|| find_named(&frame, "color-preview-after").and_then(|widget| widget.downcast::<gtk::Picture>().ok())
        .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its inventory preview");
    visit(d, "export");
    let nav = named::<adw::NavigationView>(&frame, "export-navigation");
    for tag in ["size", "color", "presets"] {
        export_page(d, &frame, tag);
        visit(d, &format!("export-{tag}"));
        d.click(icon_button(&page_header(&frame), "go-previous-symbolic").unwrap().upcast_ref());
        until(|| nav.visible_page_tag().as_deref() == Some("main"), "Export Back returns to its draft");
    }
    close(d, &dialog);

    d.w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
    let (dialog, _) = dialog_frame(d);
    visit(d, "canvas-size");
    close(d, &dialog);
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::Workspaces);
    let (dialog, _) = dialog_frame(d);
    visit(d, "workspaces");
    close(d, &dialog);
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::ThisWorkspace);
    let (dialog, frame) = dialog_frame(d);
    visit(d, "workspace-manager-root");
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    if split.is_mapped() {
        let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
        let row = list.row_at_index(0).unwrap();
        super::histogram::scroll_to(row.upcast_ref());
        d.click(row.upcast_ref());
        until(|| split.shows_content(), "the workspace manager opens its inventory details");
        page_header(&frame);
    }
    visit(d, "workspace-manager-detail");
    close(d, &dialog);
}

fn header_state(root: &gtk::Widget) -> Vec<(adw::HeaderBar, gtk::Widget, bool, bool, Option<gtk::Widget>)> {
    descendants::<adw::HeaderBar>(root).into_iter().map(|header|
        (header.clone(), header.parent().unwrap(), header.shows_start_title_buttons(), header.shows_end_title_buttons(), header.title_widget())).collect()
}

fn restored_headers(headers: &[(adw::HeaderBar, gtk::Widget, bool, bool, Option<gtk::Widget>)]) {
    for (header, parent, start, end, title) in headers {
        assert_eq!(header.parent().as_ref(), Some(parent), "the original header stays in its native toolbar");
        assert_eq!(header.shows_start_title_buttons(), *start);
        assert_eq!(header.shows_end_title_buttons(), *end);
        assert_eq!(header.title_widget().as_ref(), title.as_ref(), "the native title widget is restored");
    }
}

fn navigation_headers_settings(mut d: &mut Driver) {
    let warnings = bounds::size_warnings();
    let original = header_state(d.w.preferences.dialog.upcast_ref());
    super::phone::capture(&mut d, "rest");
    d.click(&d.named("zen-capy"));
    until(|| state(&d.w).phone.menu, "Capy opens its icon strip");
    d.click(&d.named("phone-preferences"));
    let (_, frame) = page(&d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    assert!(split.is_collapsed() && !split.shows_content(), "plain Settings opens its page list");
    let root = done_header(&d, &frame, true);
    let close_button = done_button(&d, &root);
    capture(&mut d, "preferences-root");
    let start = descendants::<gtk::Button>(root.upcast_ref()).into_iter().filter(|button| button.is_mapped()
        && button.compute_bounds(&root).is_some_and(|bounds| bounds.x() < root.width() as f32 / 2.)).collect::<Vec<_>>();
    assert_eq!(start.len(), 1, "Preferences root keeps only its search toggle at the start");
    assert_eq!(start[0].widget_name(), "preferences-search-toggle");
    let mut popup_checks = Vec::new();
    for settings in SettingsPage::ALL {
        let title = settings.localized_title(&d.w.localization());
        let row = mapped_label(&frame, &title).unwrap();
        super::histogram::scroll_to(&row);
        d.click(&row);
        until(|| split.shows_content(), "Preferences opens its page from the root list");
        capture(&mut d, &format!("preferences-{}", settings.key()));
        assert!(split.is_collapsed(), "Preferences {} keeps one pane on a phone", settings.key());
        let preferences = ui_session(&d.w).preferences().unwrap();
        let page = preferences.pages.iter().find(|page| page.id == settings).unwrap();
        if settings == SettingsPage::About {
            for row in page.groups.iter().flat_map(|group| &group.rows) {
                let (text, link) = match &row.kind {
                    PreferenceKind::Info { value } => (value.as_str(), false),
                    PreferenceKind::Link { label, .. } => (label.as_str(), true),
                    _ => continue,
                };
                let native = named::<adw::ActionRow>(&frame, &format!("setting-{}", row.id.key()));
                super::histogram::scroll_to(native.upcast_ref());
                drop(idle_paint(&d.w.window));
                let label = descendants::<gtk::Label>(native.upcast_ref()).into_iter().find(|label| label.text() == text).unwrap();
                let layout = label.layout();
                let joined = layout.lines_readonly().iter().map(|line| {
                    let start = line.start_index() as usize;
                    &text[start..start + line.length() as usize]
                }).collect::<String>();
                assert_eq!(joined, text, "About {} preserves its complete text", row.id.key());
                assert!(!layout.is_ellipsized(), "About {} shows every glyph", row.id.key());
                let natural = layout.copy();
                natural.set_width(-1);
                let (x, y) = label.layout_offsets();
                let (ink, logical) = layout.pixel_extents();
                for rect in [ink, logical] {
                    assert!(x + rect.x() >= -1 && y + rect.y() >= -1
                        && x + rect.x() + rect.width() <= label.width() + 1
                        && y + rect.y() + rect.height() <= label.height() + 1,
                        "About {} keeps its text inside the label: {rect:?} in {}x{} at {x},{y}", row.id.key(), label.width(), label.height());
                }
                let scroll = label.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
                let visible = label.compute_bounds(&scroll).unwrap();
                assert!(visible.x() >= -1. && visible.y() >= -1.
                    && visible.x() + visible.width() <= scroll.width() as f32 + 1.
                    && visible.y() + visible.height() <= scroll.height() as f32 + 1.,
                    "About {} keeps its complete value in view: {visible:?}", row.id.key());
                if !text.contains(' ') && (!link || row.id == PreferenceId::Website) && natural.pixel_size().0 <= label.width() {
                    assert_eq!(layout.line_count(), 1, "About {} keeps a fitting token on one line", row.id.key());
                }
                if row.id == PreferenceId::Website { capture(&mut d, "preferences-about-website"); }
                if link {
                    let attributes = label.attributes().unwrap();
                    let hyphens = attributes.iterator().get(gtk::pango::AttrType::InsertHyphens).unwrap().downcast::<gtk::pango::AttrInt>().unwrap();
                    assert_eq!(hyphens.value(), 0, "About links never insert synthetic hyphens");
                    assert!(native.activatable_widget().is_some_and(|widget| widget.is::<gtk::LinkButton>()));
                }
            }
            capture(&mut d, "preferences-about-values");
        }
        for row in page.groups.iter().flat_map(|group| &group.rows).filter(|row| row.visible
            && matches!(&row.kind, PreferenceKind::Choice { icons, presentation: ChoicePresentation::Dropdown, .. } if icons.is_empty())) {
            let native = named::<adw::ComboRow>(&frame, &format!("setting-{}", row.id.key()));
            super::histogram::scroll_to(native.upcast_ref());
            drop(idle_paint(&d.w.window));
            let values = descendants::<gtk::Label>(native.upcast_ref()).into_iter()
                .filter(|label| label.is_mapped() && label.widget_name() == "preference-choice-value").collect::<Vec<_>>();
            assert!(!values.is_empty(), "Preferences {} choice {} presents its selected value", settings.key(), row.id.key());
            for label in &values {
                assert_eq!(label.layout().line_count(), 1, "Preferences {} choice {} stays on one line", settings.key(), row.id.key());
                if frame.width() >= 412 || label.text().chars().count() <= 14 {
                    assert!(!label.layout().is_ellipsized(), "Preferences {} choice {} keeps its whole value: {:?}", settings.key(), row.id.key(), label.text());
                }
            }
            capture(&mut d, &format!("preferences-{}-{}", settings.key(), row.id.key()));
            if frame.width() == 412 && matches!(row.id, PreferenceId::Language | PreferenceId::MissingProfile) {
                d.click(native.upcast_ref());
                until(|| descendants::<gtk::Popover>(native.upcast_ref()).iter().any(|popup| popup.is_mapped()), "the native preference choice popup opens");
                let popup = descendants::<gtk::Popover>(native.upcast_ref()).into_iter().find(|popup| popup.is_mapped()).unwrap();
                let list = descendants::<gtk::ListView>(popup.upcast_ref()).into_iter().find(|list| list.is_mapped()).unwrap();
                drop(idle_paint(&d.w.window));
                capture(&mut d, &format!("preferences-{}-popup", row.id.key()));
                let options = descendants::<gtk::Label>(list.upcast_ref()).into_iter()
                    .filter(|label| label.is_mapped()).collect::<Vec<_>>();
                assert!(!options.is_empty());
                let selected = native.selected_item().and_downcast::<gtk::StringObject>().unwrap().string();
                for label in &options {
                    let list_row = std::iter::successors(label.parent(), |parent| parent.parent())
                        .find(|parent| parent.parent().as_ref() == Some(list.upcast_ref())).unwrap();
                    let bounds = label.compute_bounds(&list_row).unwrap();
                    assert!(bounds.x() >= 0. && bounds.x() <= 24., "the preference popup option starts beside its row: {bounds:?}");
                    if label.text() == selected {
                        popup_checks.push((row.id, descendants::<gtk::Image>(&list_row).iter().any(|icon|
                            icon.is_mapped() && icon.opacity() > 0. && icon.icon_name().as_deref() == Some("object-select-symbolic"))));
                    }
                }
                keys(&mut d, 0xff1b);
                until(|| !popup.is_mapped(), "Escape closes the native preference choice popup");
            }
        }
        if settings == SettingsPage::Color && frame.width() == 412 {
            assert_eq!(popup_checks.len(), 2, "both selected preference options are checked");
            assert!(popup_checks.iter().all(|(_, checked)| *checked), "selected preference options retain their native checkmark: {popup_checks:?}");
        }
        let captured = warnings.lock().unwrap().clone();
        assert!(captured.is_empty(), "Preferences {}: GTK size warnings: {captured:?}", settings.key());
        let subpage = done_header(&d, &frame, false);
        if settings == SettingsPage::Shortcuts {
            let one_back = |header: &adw::HeaderBar| {
                let buttons = descendants::<gtk::Button>(header.upcast_ref()).into_iter().filter(|button| button.is_mapped()
                    && (button.icon_name().as_deref() == Some("go-previous-symbolic")
                        || descendants::<gtk::Image>(button.upcast_ref()).iter().any(|image| image.icon_name().as_deref() == Some("go-previous-symbolic"))))
                    .collect::<Vec<_>>();
                assert_eq!(buttons.len(), 1, "Shortcuts has exactly one mapped Back button in its header");
                buttons.into_iter().next().unwrap()
            };
            let categories = named::<gtk::ListBox>(&frame, "shortcut-categories");
            let first = categories.row_at_index(0).unwrap();
            super::histogram::scroll_to(first.upcast_ref());
            d.click(first.upcast_ref());
            until(|| ui_session(&d.w).preferences().unwrap().shortcut_page.category.is_some()
                && !categories.is_mapped(), "the first Shortcuts category finishes opening");
            let category = named::<adw::PreferencesPage>(&frame, "shortcut-category-page");
            let category_header = done_header(&d, &frame, false);
            let back = one_back(&category_header);
            capture(&mut d, "preferences-shortcuts-category");
            d.click(back.upcast_ref());
            until(|| ui_session(&d.w).preferences().unwrap().shortcut_page.category.is_none()
                && categories.is_mapped() && !category.is_mapped(), "category Back returns to the Shortcuts list");
            one_back(&done_header(&d, &frame, false));
            if state(&d.w).theme == Theme::Dark {
                let nav = named::<adw::NavigationView>(&frame, "shortcut-navigation");
                let content = nav.visible_page().unwrap().child().unwrap();
                let scrolls = descendants::<gtk::ScrolledWindow>(&content).into_iter().filter(|scroll| scroll.is_mapped()).collect::<Vec<_>>();
                let positions = scrolls.iter().map(|scroll| scroll.vadjustment().value()).collect::<Vec<_>>();
                for scroll in &scrolls {
                    let adjustment = scroll.vadjustment();
                    adjustment.set_value((adjustment.upper() - adjustment.page_size()).max(adjustment.lower()));
                }
                drop(idle_paint(&d.w.window));
                capture(&mut d, "preferences-shortcuts-bottom");
                one_back(&page_header(&frame));
                for (scroll, position) in scrolls.iter().zip(positions) { scroll.vadjustment().set_value(position); }
            }
        }
        if settings==SettingsPage::Shortcuts {
            let search=named::<gtk::SearchEntry>(&frame,"shortcuts-search");search.set_text("Escape");search.grab_focus();
            until(|| ui_session(&d.w).preferences().unwrap().shortcut_query=="Escape" && search.delegate().is_some_and(|text| text.is_focus()),"the real Shortcuts search owns its non-empty query");
            keys(&mut d,0xff1b);
            until(|| ui_session(&d.w).preferences().unwrap().shortcut_query.is_empty() && split.shows_content(),"first Escape clears shortcut search before navigating Back");
            keys(&mut d,0xff1b);
        } else { d.click(icon_button(&subpage, "go-previous-symbolic").unwrap().upcast_ref()); }
        until(|| !split.shows_content(), "Preferences Back preserves the dialog");
        assert_eq!(done_header(&d, &frame, true), root);
    }
    d.click(close_button.upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "root Close closes Preferences");
    assert!(!state(&d.w).settings_open && close_button.parent().is_none());
    restored_headers(&original);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (_, reopened) = page(&d);
    done_header(&d, &reopened, true);
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes Preferences from its root list");
    assert!(!state(&d.w).settings_open);
    restored_headers(&original);
    d.w.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts });
    let (_, direct) = page(&d);
    let split = descendants::<adw::NavigationSplitView>(&direct).pop().unwrap();
    assert!(split.shows_content(), "direct Keyboard Shortcuts opens its page");
    capture(&mut d, "preferences-shortcuts-direct");
    let header = done_header(&d, &direct, false);
    for (action, name) in [
        (PreferenceAction::KeymapDetails { open: true }, "keymap-details"),
        (PreferenceAction::OpenActionPicker { trigger: "touch.tap.2".into() }, "action-picker"),
        (PreferenceAction::EditShortcut { id: CommandId::Redo.shortcut_id() }, "shortcut-editor"),
        (PreferenceAction::AddModifierKey, "modifier-key"),
    ] {
        d.w.dispatch(UiAction::Preferences { action });
        let (sheet, frame) = page(&d);
        assert_eq!(sheet.widget_name(), name);
        let header = done_header(&d, &frame, true);
        capture(&mut d, if name == "action-picker" { "choose-action" } else { name });
        d.click(done_button(&d, &header).upcast_ref());
        until(|| d.w.visible_dialog().is_some_and(|dialog| dialog.widget_name() != name) && !d.w.pages.root.is_transition_running(), "Done closes its shortcut sheet");
    }
    d.click(done_button(&d, &header).upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.servicing.get(), "Done closes direct Keyboard Shortcuts");

}

fn w6c_palette_library(d: &mut Driver) {
    let window = d.w.window.clone();
    if !find_named(window.upcast_ref(), "palette-chooser").is_some_and(|widget| widget.is_mapped()) {
        let color = state(&d.w).workspace.layout.phone.as_ref().unwrap().iter()
            .find(|tile| tile.control == ToolbarControl::Color).unwrap().id;
        d.click(&d.named(&format!("phone-tool-{color}")));
    }
    until(|| find_named(window.upcast_ref(), "palette-chooser").is_some_and(|widget| widget.is_mapped() && widget.width() > 0), "Color opens its palette chooser");
    if !find_named(window.upcast_ref(), "palette-library-add").is_some_and(|widget| widget.is_mapped()) {
        let chooser = d.named("palette-chooser");
        super::histogram::scroll_to(&chooser);
        d.click(&chooser);
    }
    until(|| find_named(window.upcast_ref(), "palette-library-add").is_some_and(|widget| widget.is_mapped() && widget.width() > 0), "the palette library opens");
}

fn manager_input(d: &Driver, input: layer_workspace::WorkspaceInput) {
    d.w.workspaces.send(&d.w,input);
    until(|| { let view=d.w.workspaces.view(); view.ready && !view.busy && !view.saving }, "manager input completes");
}
fn manager_page(d: &Driver, page: layer_workspace::ManagerPage, details: bool) {
    d.w.workspaces.ui.show(&d.w,page);
    until(|| { let view=d.w.workspaces.view(); view.ready && !view.busy && !view.saving }, "manager page opens");
    if page==layer_workspace::ManagerPage::ToolbarLibrary && d.w.workspaces.view().rows.is_empty() {
        manager_offered(d,layer_workspace::ManagerPage::ThisWorkspace,"save_toolbar",true);
        manager_input(d,layer_workspace::WorkspaceInput::Submit { name:"Canvas tools".into(),description:None,choice:None });
        manager_input(d,layer_workspace::WorkspaceInput::Open { page });
    }
    if details {
        let id=d.w.workspaces.view().rows.first().expect("offered manager row").id.clone();
        manager_input(d,layer_workspace::WorkspaceInput::Select { id:Some(id) });
        manager_input(d,layer_workspace::WorkspaceInput::OpenDetails);
    }
}
fn manager_close(d: &Driver) {
    manager_input(d,layer_workspace::WorkspaceInput::Cancel);
    manager_input(d,layer_workspace::WorkspaceInput::Dismiss);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "manager surfaces close");
}
fn manager_workspace(d: &Driver) -> String {
    use layer_workspace::{ManagerAction as A,ManagerPage as P,WorkspaceInput as I};
    manager_page(d,P::Workspaces,false);
    if !d.w.workspaces.view().rows.iter().any(|row| row.title=="Canvas study") {
        manager_input(d,I::Form { action:A::New });
        manager_input(d,I::Submit { name:"Canvas study".into(),description:None,choice:None });
    }
    let id=d.w.workspaces.view().rows.iter().find(|row| row.title=="Canvas study").unwrap().id.clone();
    manager_input(d,I::Select { id:Some(id.clone()) });id
}
fn manager_offered(d: &Driver, page: layer_workspace::ManagerPage, kind: &str, form: bool) {
    manager_page(d,page,true);
    let view=d.w.workspaces.view();
    let action=view.details.iter().flat_map(|details| &details.actions).chain(view.rows.iter().flat_map(|row| &row.actions))
        .find(|button| button.enabled && serde_json::to_value(&button.action).unwrap()["type"]==kind).expect("offered manager form").action.clone();
    manager_input(d,if form { layer_workspace::WorkspaceInput::Form { action } } else { layer_workspace::WorkspaceInput::Action { action } });
}
fn w6c_managers(d: &mut Driver) {
    use layer_workspace::{ManagerAction as A,ManagerPage as P,WorkspaceInput as I};
    for (name,page) in [("workspaces",P::Workspaces),("layout-history",P::History),("this-workspace",P::ThisWorkspace),("saved-toolbars",P::ToolbarLibrary)] {
        manager_page(d,page,false);dialog_frame(d);capture(d,name);manager_close(d);
    }
    for (name,page) in [("this-workspace-details",P::ThisWorkspace),("saved-toolbar-details",P::ToolbarLibrary)] {
        manager_page(d,page,true);dialog_frame(d);capture(d,name);manager_close(d);
    }
    for (name,page,action) in [("new-workspace",P::Workspaces,A::New),("new-toolbar",P::ThisWorkspace,A::NewToolbar(None))] {
        manager_page(d,page,false);manager_input(d,I::Form { action });dialog_frame(d);capture(d,name);manager_close(d);
    }
    for (name,page,kind) in [("save-toolbar",P::ThisWorkspace,"save_toolbar"),("replace-toolbar",P::ThisWorkspace,"replace_toolbar"),
        ("rename-saved-toolbar",P::ToolbarLibrary,"rename"),("delete-saved-toolbar",P::ToolbarLibrary,"delete"),("update-saved-toolbar",P::ToolbarLibrary,"update_toolbar")] {
        manager_offered(d,page,kind,true);dialog_frame(d);capture(d,name);manager_close(d);
    }
    for (name,phone) in [("add-tools",false),("insert-tools",true)] {
        let action=if phone { UiAction::Phone { action:PhoneAction::InsertTools { before:Some(state(&d.w).workspace.layout.phone.as_ref().unwrap()[0].id) } } }
            else { UiAction::Customize { action:CustomizationAction::InsertTools { panel:Panel::Toolbar,before:None } } };
        d.w.dispatch(action);dialog_frame(d);capture(d,name);
        d.w.dispatch(UiAction::Customize { action:CustomizationAction::CancelTools });
        until(|| d.w.visible_dialog().is_none(), "tool picker closes");
    }
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(false);
    for (name,command) in [("reset-workspace",WorkspaceCommand::ResetLayout),("reset-brushes",WorkspaceCommand::ResetBrushes)] {
        d.w.dispatch(UiAction::WorkspaceManager { command });dialog_frame(d);capture(d,name);manager_close(d);
    }
    settings.set_gtk_enable_animations(animations);
    for (name,rename) in [("rename-workspace",true),("delete-workspace",false)] {
        let id=manager_workspace(d);
        let offered=d.w.workspaces.view().rows.iter().find(|row| row.id==id).unwrap().actions.iter()
            .find(|button| button.enabled && if rename { matches!(button.action,A::Rename(_)) } else { matches!(button.action,A::Delete(_)) }).unwrap().action.clone();
        manager_input(d,I::Form { action:offered });dialog_frame(d);capture(d,name);manager_close(d);
    }
    manager_input(d,I::Form { action:A::SaveAsNew });dialog_frame(d);capture(d,"save-as-new-workspace");manager_close(d);
    let inactive=manager_workspace(d);manager_close(d);
    manager_input(d,I::Switch { id:"builtin:workspace:painter".into() });manager_page(d,P::Workspaces,false);
    let delete=d.w.workspaces.view().rows.iter().find(|row| row.id==inactive).unwrap().actions.iter()
        .find(|button| button.enabled && matches!(button.action,A::Delete(_))).unwrap().action.clone();
    manager_input(d,I::Form { action:delete });dialog_frame(d);capture(d,"delete-inactive-workspace");manager_close(d);
    d.w.dispatch(UiAction::Invoke { command:CommandId::AddLayer });
    d.w.documents.select(&d.w,d.w.documents.selected(),true);
    dialog_frame(d);capture(d,"unsaved-changes");
    let dialog=d.w.visible_dialog().unwrap();
    d.click(find_button(dialog.upcast_ref(), &layer_ui::new_document_spec(&d.w.localization()).cancel).unwrap().upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.servicing.get(), "unsaved cancellation finishes");
}

fn w6c_catchup(d: &mut Driver) {
    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    page(d);
    for action in [NewDrawingAction::Presets, NewDrawingAction::SaveAs,
        NewDrawingAction::PresetName { text: "Canvas study".into() }, NewDrawingAction::SavePreset] {
        d.w.dispatch(UiAction::NewDrawing { action });
    }
    let index = match state(&d.w).new_drawing.unwrap().drawer.unwrap() {
        NewDrawingDrawerView::Presets(browser) => browser.tiles.iter().position(|tile| tile.own && tile.selected).unwrap(),
        other => panic!("{other:?}"),
    };
    d.w.dispatch(UiAction::NewDrawing { action: NewDrawingAction::RenamePreset { index } });
    capture(d, "rename-drawing-preset");
    d.w.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Cancel });
    until(|| d.w.visible_dialog().is_none(), "New Drawing closes");

    let directory = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    let scan = directory.join("Linen scan.png");
    super::paper::save_scan(&scan);
    let selected = state(&d.w).layer_properties.layer.unwrap();
    let paper = layer_ui::occurrence_token(layer_core::paper::paper_occurrence(ui_session(&d.w).engine().document().scene(), None).unwrap());
    d.w.dispatch(UiAction::SelectLayer { id: paper });
    crate::paper_picker::open(&d.w, paper);
    let (picker, _) = page(d);
    super::paper::import_scan(&d.w, &picker, &scan);
    crate::paper_picker::open(&d.w, paper);
    let (picker, frame) = page(d);
    let grid = named::<gtk::FlowBox>(&frame, "texture-grid");
    super::paper::paper_tile_menu(&grid, layer_core::paper::built_in_papers().len() as i32, "Rename…");
    let (rename, _) = page(d);
    capture(d, "rename-paper");
    crate::page_dialog::close_dialog(&rename);
    until(|| d.w.visible_dialog().as_ref() == Some(&picker), "Rename Paper closes");
    close(d, &picker);
    d.w.dispatch(UiAction::SelectLayer { id: selected });

    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (export, frame) = page(d);
    export_page(d, &frame, "presets");
    named::<adw::ButtonRow>(&frame, "export-preset-save").emit_by_name::<()>("activated", &[]);
    until(|| d.w.visible_dialog().is_some_and(|dialog| find_named(&crate::page_dialog::parent(&dialog), "export-preset-name").is_some()), "Save Export Preset opens");
    capture(d, "save-export-preset");
    crate::page_dialog::close_dialog(&d.w.visible_dialog().unwrap());
    until(|| d.w.visible_dialog().as_ref() == Some(&export), "Save Export Preset closes");
    close(d, &export);

    w6c_palette_library(d);
    d.click(&d.named("palette-library-add"));
    d.click_label(&d.w.localization().text(layer_ui::MessageId::NATIVE_PALETTES_MENU_NEW));
    let (dialog, frame) = page(d);
    capture(d, "new-palette");
    named::<gtk::Entry>(&frame, "palette-library-name").set_text("Canvas study");
    let alert = dialog.downcast_ref::<adw::AlertDialog>().unwrap();
    assert!(alert.is_response_enabled("save"));
    d.click(find_button(&frame, &alert.response_label("save")).unwrap().upcast_ref());
    until(|| d.w.visible_dialog().is_none(), "New Palette saves");

    w6c_palette_library(d);
    let id = state(&d.w).color_library.palettes.iter().find(|palette| palette.name == "Canvas study").unwrap().id;
    for (label, name) in [(layer_ui::MessageId::NATIVE_PALETTES_MENU_RENAME, "rename-palette"),
        (layer_ui::MessageId::NATIVE_PALETTES_MENU_REMOVE, "delete-palette")] {
        let row = d.named(&format!("palette-choice-{id}"));
        super::histogram::scroll_to(&row);
        assert!(row.grab_focus());
        keys(d, 0xff67);
        d.click_label(&d.w.localization().text(label));
        until(|| d.w.visible_dialog().is_some(), "Palette prompt opens");
        capture(d, name);
        crate::page_dialog::close_dialog(&d.w.visible_dialog().unwrap());
        until(|| d.w.visible_dialog().is_none(), "Palette prompt closes");
    }
}

fn w6c_profile_library(d: &mut Driver) {
    d.w.dispatch(UiAction::OpenSettings { page: SettingsPage::Color });
    let (settings, frame) = page(d);
    let manager = named::<gtk::Button>(&frame, "color-profile-library");
    super::histogram::scroll_to(manager.upcast_ref());
    assert!(manager.is_mapped());
    d.click(manager.upcast_ref());
    until(|| d.w.visible_dialog().is_some_and(|dialog| dialog.widget_name() == "profile-library-manager"), "Profile Library opens");
    capture(d, "profile-library");
    crate::page_dialog::close_dialog(&d.w.visible_dialog().unwrap());
    until(|| d.w.visible_dialog().as_ref() == Some(&settings), "Profile Library closes");
    close(d, &settings);
}

fn w6d_canvas(d: &mut Driver) {
    let (layer, [width,height]) = {
        let session=ui_session(&d.w);let document=session.engine().document();
        (layer_ui::occurrence_token(document.working.occurrence.unwrap()),document.composition().size.map(|side| side as f32))
    };
    d.w.dispatch(UiAction::SelectLayer { id:layer });
    d.w.dispatch(UiAction::Invoke { command:CommandId::Lasso });
    native_pen_path(&d.w,&[[width*0.5,height*0.5],[width*0.85,height*0.5],[width*0.85,height*0.85],[width*0.5,height*0.85],[width*0.5,height*0.5]]);
    d.w.dispatch(UiAction::Layer { action:LayerAction::FillSelection });
    d.w.dispatch(UiAction::Invoke { command:CommandId::FeatherSelection });
    until(|| state(&d.w).layer_tools.selection_resize.is_some() && find_named(d.w.window.upcast_ref(),"selection-refine-value").is_some_and(|widget|widget.is_mapped()),"Refine opens its canvas panel");
    capture(d,"selection-refine");
    d.w.dispatch(UiAction::Selection { action:SelectionAction::CancelResize });

    d.w.dispatch(UiAction::Invoke { command:CommandId::BlendPerceptual });
    d.w.dispatch(UiAction::Invoke { command:CommandId::FrequencySeparation });
    until(|| state(&d.w).layer_tools.frequency_separation.is_some() && find_named(d.w.window.upcast_ref(),"frequency-separation-radius").is_some_and(|widget|widget.is_mapped()),"Frequency Separation opens its canvas panel");
    capture(d,"frequency-separation");
    d.w.dispatch(UiAction::FrequencySeparation { action:FrequencySeparationAction::Cancel });

    d.w.dispatch(UiAction::Invoke { command:CommandId::SearchCommands });
    until(|| d.w.command_bar.is_open() && find_named(d.w.window.upcast_ref(),"command-bar").is_some_and(|widget|widget.is_mapped()),"Command Search opens");
    capture(d,"command-search");
    d.w.dispatch(UiAction::CommandSearch { action:CommandSearchAction::Close });
    until(|| !d.w.command_bar.owns_input(),"Command Search releases input before Proof");

    d.w.dispatch(UiAction::Invoke { command:CommandId::SoftProofSetup });
    until(|| state(&d.w).customization.drawer.as_ref().is_some_and(|drawer|drawer.panels().any(|panel|panel==Panel::Proof)) && find_named(d.w.window.upcast_ref(),"drawer-panel-Proof").is_some_and(|panel|panel.is_mapped()&&panel.width()>0&&panel.height()>0),"Soft Proof opens its phone drawer");
    let proof=find_named(d.w.window.upcast_ref(),"drawer-panel-Proof").unwrap();
    let print=find_button(&proof,"Print").unwrap();
    until(|| !d.w.drawer.animation_active(),"Proof drawer reaches its target before Print input");
    d.click(print.upcast_ref());
    until(|| find_named(&proof,"soft-proof-setup").is_some_and(|widget|widget.is_mapped()),"Print settings map in the phone drawer");
    capture(d,"soft-proof");
    let print_body=find_named(&proof,"soft-proof-setup").unwrap();
    let scroll=print_body.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
    assert!(scroll.property::<bool>("propagate-natural-height"),"phone Print propagates its actual natural body height");
    assert!(!proof.property::<bool>("vexpand"),"phone Proof is content-sized");
    let natural=proof.measure(gtk::Orientation::Vertical,proof.width()).1;
    let allocated=proof.height()+proof.margin_top()+proof.margin_bottom();
    assert!((allocated-natural).abs()<=1,"phone Proof allocation plus margins {allocated} matches natural height {natural}");
    let last=find_named(&proof,"proof-gamut-warning").unwrap().compute_bounds(&scroll).unwrap();
    assert!(last.y()>=-1. && last.y()+last.height()<=scroll.height() as f32+1.,"the last Print row fits without scrolling: {last:?}, viewport {}",scroll.height());


    let original=d.w.documents.selected();
    d.w.documents.enqueue(&d.w,(new_drawing_at(1200,1600,layer_core::color::SampleDepth::F32),None));
    until(|| d.w.documents.len()==2 && !d.w.documents.changing.get(),"the float drawing opens");
    super::new_photo::ready(&d.w);
    let temporary=d.w.documents.selected();assert_ne!(temporary,original);
    d.w.dispatch(UiAction::Invoke { command:CommandId::SdrRendition });
    until(|| state(&d.w).customization.drawer.as_ref().is_some_and(|drawer|drawer.panels().any(|panel|panel==Panel::Proof)) && find_named(d.w.window.upcast_ref(),"drawer-panel-Proof").is_some_and(|panel|panel.is_mapped()&&panel.width()>0&&panel.height()>0&&find_named(&panel,"sdr-tone-pad-surface").is_some_and(|widget|widget.is_mapped())),"SDR Rendition opens its phone drawer");
    capture(d,"sdr-rendition");
    d.w.documents.select(&d.w,temporary,true);
    until(|| d.w.documents.len()==1 && d.w.documents.selected()==original && !d.w.documents.changing.get(),"the original drawing returns");
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_dialogs --native-storage; LAYER_DIALOG_BATCH=w6a/w6b/w6c/w6d, phone viewport/theme external"]
fn native_phone_dialogs() {
    let batch = std::env::var("LAYER_DIALOG_BATCH").unwrap();
    let mut d = super::phone_motion::setup("art.capycanvas.PhoneDialogs");
    if batch == "w6d" { w6d_canvas(&mut d); d.finish(); return; }
    if batch == "w6c" {
        w6c_catchup(&mut d); w6c_managers(&mut d); w6c_profile_library(&mut d);
        d.finish(); return;
    }
    if batch == "w6b" {
        inventory_with(&mut d, |d, name| {
            if matches!(name, "new-drawing" | "color" | "export" | "export-size" | "export-color" | "export-presets" | "canvas-size") { capture(d, name); }
        });
        for (command, name) in [(CommandId::ImageSize, "image-size"), (CommandId::DocumentProperties, "properties")] {
            d.w.dispatch(UiAction::Invoke { command });
            let (dialog, _) = page(&d);
            capture(&mut d, name);
            close(&d, &dialog);
        }
        d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
        let (dialog, _) = page(&d);
        d.w.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Paper });
        capture(&mut d, "new-paper");
        keys(&mut d, 0xff1b);
        close(&d, &dialog);
        let selected = state(&d.w).layer_properties.layer.unwrap();
        let paper = layer_ui::occurrence_token(ui_session(&d.w).engine().document().bottom_paper().unwrap());
        d.w.dispatch(UiAction::SelectLayer { id: paper });
        crate::paper_picker::open(&d.w, paper);
        let (dialog, _) = page(&d);
        capture(&mut d, "paper");
        close(&d, &dialog);
        d.w.dispatch(UiAction::SelectLayer { id: selected });
        crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
        let (dialog, frame) = page(&d);
        d.click(named::<gtk::Button>(&frame, "edit-color-swatches").upcast_ref());
        until(|| d.w.color_editors.borrow().last().is_some_and(|editor| editor.slide.is_open()), "All Swatches opens");
        capture(&mut d, "all-swatches");
        close(&d, &dialog);
        d.finish();
        return;
    }
    assert_eq!(batch, "w6a");
    navigation_headers_settings(&mut d);
    d.w.dispatch(UiAction::Invoke { command: CommandId::KeyboardShortcuts });
    let (dialog, _) = page(&d);
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::BeginShortcut { id: CommandId::Hand.shortcut_id() } });
    let (recording, _) = page(&d);
    capture(&mut d,"shortcut-recording");
    keys(&mut d,101);
    until(|| ui_session(&d.w).preferences().unwrap().capture.as_ref().is_some_and(|capture| capture.conflict.is_some()), "existing Eraser chord conflicts with Hand");
    capture(&mut d,"shortcut-conflict");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::CancelShortcut });
    crate::page_dialog::close_dialog(&recording);
    until(|| d.w.visible_dialog().as_ref() == Some(&dialog), "recording returns to Shortcuts");
    let text = serde_json::json!({"format":"capycanvas-keymap","version":1,"keymap":{"id":"photoshop","revision":1}}).to_string();
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ImportKeymap { text } });
    until(|| d.w.window.visible_dialog().is_some_and(|alert| alert.is::<adw::AlertDialog>() && alert.is_mapped()), "keymap import confirmation maps");
    capture(&mut d,"keymap-import");
    let alert = d.w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
    d.click(find_button(alert.upcast_ref(), &alert.response_label("cancel")).unwrap().upcast_ref());
    let key = ui_session(&d.w).preferences().unwrap().shortcut_page.modifiers[0].key.clone();
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::EditModifierKey { key: key.clone() } });
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ModifierKeyPerTool { key, per_tool: true } });
    until(|| ui_session(&d.w).preferences().unwrap().modifier_editor.as_ref().is_some_and(|editor| editor.per_tool), "modifier per-tool page opens");
    capture(&mut d,"modifier-per-tool");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Page { page: SettingsPage::Input } });
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::EditPenButton { trigger: "pen.button.primary".into() } });
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::PenButtonPerTool { trigger: "pen.button.primary".into(), per_tool: true } });
    until(|| ui_session(&d.w).preferences().unwrap().pen_button_editor.as_ref().is_some_and(|editor| editor.per_tool), "pen button per-tool page opens");
    capture(&mut d,"pen-button-per-tool");
    close(&d,&dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_navigation_headers --native-storage; phone viewport, light/dark"]
fn native_phone_navigation_headers() {
    let mut d = super::phone_motion::setup("art.capycanvas.NavigationHeaders");
    assert!(adw::ComboRow::new().factory().is_some(), "a fresh native ComboRow exposes its built-in factory");
    assert!(d.w.surface.width().min(d.w.surface.height()) < 480);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    navigation_headers_settings(&mut d);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (dialog, frame) = page(&d);
    let nav = named::<adw::NavigationView>(&frame, "export-navigation");
    let root = navigation_header(&d, &frame, true);
    let close_button = icon_button(&root, "window-close-symbolic").unwrap();
    let source = find_named(&frame, "export-confirm").unwrap();
    let parent = source.parent();
    let primary = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap().downcast::<gtk::Button>().unwrap();
    for tag in ["size", "color", "presets"] {
        export_page(&mut d, &frame, tag);
        let subpage = navigation_header(&d, &frame, false);
        let action = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap().downcast::<gtk::Button>().unwrap();
        assert_eq!(action.label(), primary.label());
        if tag == "size" { capture(&mut d, "export-size"); }
        let outgoing = nav.visible_page().unwrap();
        d.click(icon_button(&subpage, "go-previous-symbolic").unwrap().upcast_ref());
        until(|| nav.visible_page_tag().as_deref() == Some("main") && !outgoing.is_mapped(), "Export Back returns to its existing draft");
        assert_eq!(navigation_header(&d, &frame, true), root);
    }
    d.click(close_button.upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Export Close invokes its existing Cancel");
    assert_eq!(source.parent(), parent);
    assert!(source.get_visible() && close_button.parent().is_none() && primary.parent().is_none());
    assert!(dialog.child().is_some());

    let original = header_state(d.w.workspaces.ui.dialog.upcast_ref());
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::ThisWorkspace);
    let (dialog, frame) = page(&d);
    let root = done_header(&d, &frame, true);
    let root_title = dialog.title();
    assert_eq!(descendants::<gtk::Label>(root.upcast_ref()).into_iter().filter(|label| label.is_mapped() && label.text() == root_title).count(), 1,
        "the manager root header shows the dialog title");
    capture(&mut d, "workspace-manager-root");
    let close_button = done_button(&d, &root);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    assert!(split.is_mapped() && split.is_collapsed(), "Manage Toolbars root keeps one pane on a phone");
    let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
    d.click(list.row_at_index(0).unwrap().upcast_ref());
    until(|| split.shows_content(), "the workspace manager opens toolbar details");
    assert!(split.is_mapped() && split.is_collapsed(), "Manage Toolbars detail keeps one pane on a phone");
    let subpage = done_header(&d, &frame, false);
    assert_eq!(subpage, root, "the manager keeps its existing header at the top");
    let title = d.w.workspaces.view().details.as_ref().unwrap().title.clone();
    assert_eq!(descendants::<gtk::Label>(&frame).into_iter().filter(|label| label.is_mapped() && label.text().as_str() == title.as_str()).count(), 1,
        "the detail title appears once across its header and body");
    capture(&mut d, "workspace-manager-detail");
    d.click(icon_button(&subpage, "go-previous-symbolic").unwrap().upcast_ref());
    until(|| !split.shows_content(), "manager Back returns to its toolbar list");
    assert_eq!(done_header(&d, &frame, true), root);
    assert_eq!(descendants::<gtk::Label>(root.upcast_ref()).into_iter().filter(|label| label.is_mapped() && label.text() == root_title).count(), 1,
        "manager Back restores the dialog title");
    d.click(close_button.upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "manager Close invokes its existing Cancel");
    assert!(d.w.workspaces.view().page.is_none() && close_button.parent().is_none());
    restored_headers(&original);
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::ThisWorkspace);
    let (_, frame) = page(&d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
    d.click(list.row_at_index(0).unwrap().upcast_ref());
    until(|| split.shows_content(), "the reopened manager opens its detail page");
    done_header(&d, &frame, false);
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes the manager from details");
    assert!(d.w.workspaces.view().page.is_none());
    restored_headers(&original);
    for mode in [layer_workspace::ManagerPage::ThisWorkspace, layer_workspace::ManagerPage::ToolbarLibrary] {
        for detail in [false, true] {
            if mode == layer_workspace::ManagerPage::ToolbarLibrary && detail {
                let toolbar = state(&d.w).workspace.layout.panels.iter()
                    .find(|panel| panel.id.kind() == PanelKind::Tiles).unwrap().id;
                glib::MainContext::default().block_on(
                    d.w.workspaces.manager().unwrap().save_toolbar(toolbar, "Done Test Toolbar", crate::workspace::manager::now_ms()),
                ).unwrap();
            }
            d.w.workspaces.ui.show(&d.w, mode);
            let (_, frame) = page(&d);
            if detail {
                let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
                let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
                let row = list.row_at_index(0).unwrap();
                super::histogram::scroll_to(row.upcast_ref());
                d.click(row.upcast_ref());
                until(|| split.shows_content(), "the manager detail opens before Done");
            }
            let header = done_header(&d, &frame, !detail);
            capture(&mut d, &format!("manager-{mode:?}-{}", if detail { "detail" } else { "root" }).to_lowercase());
            d.click(done_button(&d, &header).upcast_ref());
            until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Done closes the manager from its current level");
        }
    }
    d.w.dispatch(UiAction::Customize { action: CustomizationAction::ManageToolbars });
    let (_, frame) = page(&d);
    let header = done_header(&d, &frame, true);
    capture(&mut d, "toolbar-manager");
    d.click(done_button(&d, &header).upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Done closes the toolbar manager");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_editor_pages --native-storage; 412x891 light"]
fn native_phone_editor_pages() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditorPages");
    let diagnostics = bounds::size_warnings();
    assert!(matches!([d.w.surface.width(), d.w.surface.height()], [412, 891] | [891, 412]));
    d.w.dispatch(UiAction::Invoke { command: CommandId::AddLayer });
    until(|| !ui_session(&d.w).engine().has_pending_document_edits(), "the modal history fixture is committed");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::Edit {
        id: PreferenceId::FingerDrawing,
        value: PreferenceValue::Choice(1),
    } });
    assert!(ui_session(&d.w).finger_drawing());
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let authored = active_raster(ui_session(&d.w).engine().document()).clone();
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (dialog, frame) = page(&d);
    capture(&mut d, "preferences");
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    if split.shows_content() {
        d.click(icon_button(&page_header(&frame), "go-previous-symbolic").unwrap().upcast_ref());
        until(|| !split.shows_content(), "Preferences Back returns to its root list");
    }
    let header = done_header(&d, &frame, true);
    let close_button = done_button(&d, &header);
    assert!(gtk::prelude::GtkWindowExt::focus(&d.w.window).is_none_or(|focus| focus != close_button), "opening does not focus the page's close button");
    assert!(!split.shows_content(), "plain Settings opens the page list");
    close_button.grab_focus();
    for _ in 0..48 {
        keys(&mut d, 0xff09);
        let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
        assert!(focus == frame || focus.is_ancestor(&frame), "Tab stays inside the page");
    }
    close_button.grab_focus();
    d.input.perform(serde_json::json!([
        { "key": 0xffe3u32, "down": true }, { "key": 122, "down": true },
        { "key": 122, "down": false }, { "key": 0xffe3u32, "down": false },
        { "key": 0xffffu32, "down": true }, { "key": 0xffffu32, "down": false }
    ]));
    let p = screen_point(header.upcast_ref(), &d.w.window, [0.5, 0.5]);
    d.input.perform(serde_json::json!([
        { "touch": "down", "point": p }, { "touch": "move", "point": [p[0] + 30., p[1]] },
        { "touch": "up" }
    ]));
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    assert_eq!(active_raster(ui_session(&d.w).engine().document()), &authored);
    d.w.dispatch(UiAction::OpenSettings { page: SettingsPage::Shortcuts });
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::BeginShortcut { id: CommandId::Redo.shortcut_id() } });
    let (recording, _) = page(&d);
    keys(&mut d, 0xffc5);
    assert!(ui_session(&d.w).preferences().unwrap().capture.unwrap().chord.is_some(), "nested shortcut recording receives native keys");
    d.w.dispatch(UiAction::Preferences { action: PreferenceAction::CancelShortcut });
    crate::page_dialog::close_dialog(&recording);
    until(|| d.w.visible_dialog().as_ref() == Some(&dialog) && !d.w.pages.root.is_transition_running(), "the shortcut form returns to Settings");
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(false);
    let original_keymap = state(&d.w).settings.keymap;
    let text = serde_json::json!({"format": "capycanvas-keymap", "version": 1, "keymap": {"id": "photoshop", "revision": 1}}).to_string();
    for response in ["cancel", "import"] {
        d.w.dispatch(UiAction::Preferences { action: PreferenceAction::ImportKeymap { text: text.clone() } });
        until(|| d.w.window.visible_dialog().is_some_and(|alert| alert.is::<adw::AlertDialog>() && alert.is_mapped() && alert.width() > 0 && alert.height() > 0), "keymap import alert maps over Preferences");
        let alert = d.w.window.visible_dialog().unwrap().downcast::<adw::AlertDialog>().unwrap();
        assert_eq!(d.w.pages.visible().as_ref(), Some(&dialog), "Preferences remains the page below the native alert");
        assert!(crate::page_dialog::parent(&dialog).is_mapped());
        capture(&mut d, &format!("keymap-import-{response}"));
        let button = find_button(alert.upcast_ref(), &alert.response_label(response)).unwrap();
        d.click(button.upcast_ref());
        until(|| !alert.is_mapped() && d.w.window.visible_dialog().is_none()
            && ui_session(&d.w).preferences().unwrap().keymap.import.is_none(), "the native keymap response closes its preview");
        assert_eq!(d.w.visible_dialog().as_ref(), Some(&dialog));
        if response == "cancel" { assert_eq!(state(&d.w).settings.keymap, original_keymap, "Cancel preserves the keymap"); }
    }
    assert_eq!(state(&d.w).settings.keymap.as_ref().map(|keymap| keymap.id.as_str()), Some("photoshop"));
    settings.set_gtk_enable_animations(animations);
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes Preferences with its projected Done");
    assert!(!crate::page_dialog::parent(&dialog).is_mapped());

    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (dialog, _) = page(&d);
    d.w.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Width { value: 731. } });
    let draft = state(&d.w).new_drawing.unwrap().setup;
    d.w.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Paper });
    capture(&mut d, "new-paper");
    keys(&mut d, 0xff1b);
    assert_eq!(state(&d.w).new_drawing.unwrap().setup, draft, "Back preserves the New draft");
    assert!(state(&d.w).new_drawing.unwrap().drawer.is_none(), "Escape leaves the subpage even without an initial focus widget");
    page(&d);
    close(&d, &dialog);

    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, frame) = page(&d);
    let rows = find_named(&frame, "edit-color-rows").unwrap();
    let body = rows.ancestor(crate::dialog_content::DialogContent::static_type()).unwrap();
    let bounds = rows.compute_bounds(&body).unwrap();
    assert!(bounds.height() > 0. && bounds.y() + bounds.height() <= body.height() as f32 + 1., "the stacked color rows fit their measured body");
    for button in descendants::<gtk::Button>(&rows) {
        if button.is_mapped() { let b = button.compute_bounds(&frame).unwrap(); assert!(b.x() >= 0. && b.x() + b.width() <= frame.width() as f32 + 1., "color row buttons stay inside the page: {b:?}"); }
    }
    capture(&mut d, "color");
    let pick = find_named(&frame, "edit-color-pick").unwrap();
    d.click(&pick);
    until(|| d.w.visible_dialog().is_none() && state(&d.w).color_picker.editor, "the color page hands input to the canvas picker");
    let p = screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.5, 0.5]);
    d.input.perform(serde_json::json!([
        { "touch": "down", "point": p }, { "touch": "move", "point": [p[0] + 12., p[1]] },
        { "touch": "up" }
    ]));
    let (reopened, _) = page(&d);
    assert_eq!(reopened, dialog, "the picker returns to the same color editor");
    close(&d, &dialog);

    let selected = state(&d.w).layer_properties.layer.unwrap();
    let paper = layer_ui::occurrence_token(ui_session(&d.w).engine().document().bottom_paper().unwrap());
    d.w.dispatch(UiAction::SelectLayer { id: paper });
    crate::paper_picker::open(&d.w, paper);
    let (dialog, _) = page(&d);
    capture(&mut d, "paper");
    close(&d, &dialog);
    d.w.dispatch(UiAction::SelectLayer { id: selected });
    let color_tile = state(&d.w).workspace.layout.phone.as_ref().unwrap().iter()
        .find(|tile| tile.control == ToolbarControl::Color).unwrap().id;
    d.click(&d.named(&format!("phone-tool-{color_tile}")));
    until(|| d.w.drawer.placement().is_some() && !d.w.drawer.animation_active()
        && find_named(d.w.window.upcast_ref(), "palette-chooser").is_some_and(|widget| widget.is_mapped() && widget.width() > 0 && widget.height() > 0), "Color opens its palette chooser");
    let chooser = d.named("palette-chooser");
    super::histogram::scroll_to(&chooser);
    d.click(&chooser);
    until(|| find_named(d.w.window.upcast_ref(), "palette-library-add").is_some_and(|widget| widget.is_mapped() && widget.width() > 0 && widget.height() > 0), "the palette library opens");
    d.click(&d.named("palette-library-add"));
    let new_palette = d.w.localization().text(layer_ui::MessageId::NATIVE_PALETTES_MENU_NEW);
    d.click_label(&new_palette);
    let (dialog, frame) = page(&d);
    let field = named::<gtk::Entry>(&frame, "palette-library-name");
    let alert = dialog.clone().downcast::<adw::AlertDialog>().unwrap();
    for name in ["new-palette", "new-palette-reopened"] {
        let (_, frame) = page(&d);
        assert_eq!(named::<gtk::Entry>(&frame, "palette-library-name"), field, "reopening keeps the same name field");
        let scroll = field.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        drop(idle_paint(&d.w.window));
        capture(&mut d, name);
        let bounds = field.compute_bounds(&scroll).unwrap();
        let natural = field.measure(gtk::Orientation::Vertical, field.width()).1 as f32;
        assert!((bounds.height() - natural).abs() <= 1., "New Palette keeps its name field's natural height: {bounds:?}, natural {natural}");
        assert!(bounds.y() >= -1. && bounds.y() <= 24., "New Palette places the name field at the top of its content: {bounds:?}");
        close(&d, &dialog);
        assert_eq!(alert.extra_child().as_ref(), Some(field.upcast_ref()), "closing restores the alert's extra child");
        if name == "new-palette" { crate::page_dialog::present_role(&alert, &d.w.window, layer_ui::DialogRole::Editor); }
    }
    let browser = d.named("palette-browser");
    assert!(browser.is_visible(), "cancelling New Palette returns to its library chooser");
    keys(&mut d, 0xff1b);
    until(|| !browser.is_visible(), "Escape closes the palette library chooser");
    d.w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
    until(|| d.w.drawer.is_closed(), "Color closes after cancelling New Palette");
    for (command, name) in [
        (CommandId::ImageSize, "image-size"),
        (CommandId::DocumentProperties, "properties"),
        (CommandId::ExportDocument, "export"),
    ] {
        d.w.dispatch(UiAction::Invoke { command });
        let (dialog, frame) = page(&d);
        if command == CommandId::ExportDocument {
            until(|| find_named(&frame, "color-preview-after").and_then(|w| w.downcast::<gtk::Picture>().ok())
                .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its preview before a destination is chosen");
            let action = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap();
            assert!(action.is_ancestor(&page_header(&frame)), "Export keeps Choose File in its only main header");
        }
        if command == CommandId::DocumentProperties { done_header(&d, &frame, true); }
        capture(&mut d, name);
        let scroll = crate::page_dialog::content(&dialog).and_downcast::<gtk::ScrolledWindow>();
        if command == CommandId::DocumentProperties {
            let header = done_header(&d, &frame, true);
            let default = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap();
            assert!(default.is_mapped(), "Properties keeps its declared default response visible");
            assert_eq!(default, done_button(&d, &header).upcast::<gtk::Widget>());
            let alert = dialog.downcast_ref::<adw::AlertDialog>().unwrap();
            let responses = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let observed = responses.clone();
            let handler = alert.connect_response(None, move |_, response| observed.borrow_mut().push(response.to_owned()));
            diagnostics.lock().unwrap().clear();
            d.click(done_button(&d, &header).upcast_ref());
            until(|| d.w.visible_dialog().is_none() && !d.w.servicing.get(), "Properties Done completes its close response");
            assert_eq!(*responses.borrow(), [alert.close_response().to_string()], "Properties Done emits its close response exactly once");
            assert!(!diagnostics.lock().unwrap().iter().any(|message| message.starts_with("Adwaita critical:")), "Properties Done has no Adwaita critical");
            alert.disconnect(handler);
        } else { close(&d, &dialog); }
        if let Some(scroll) = scroll { assert!(scroll.child().is_some(), "closing preserves a scrolled editor body"); }
        assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    }
    d.w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
    let (_, frame) = page(&d);
    let width = ui_session(&d.w).engine().document().composition().size[0];
    d.w.dispatch(UiAction::CanvasSize { action: layer_ui::CanvasSizeAction::Width { value: width as f64 + 1. } });
    let view = state(&d.w).layer_tools.canvas_size.unwrap();
    assert!(view.can_apply);
    let apply = gtk::prelude::GtkWindowExt::default_widget(&d.w.window).unwrap().downcast::<gtk::Button>().unwrap();
    assert_eq!(apply.label().as_deref(), Some(view.apply_label.as_ref()));
    assert!(apply.is_ancestor(&page_header(&frame)), "Apply is the visible header proxy");
    capture(&mut d, "canvas-size");
    d.click(apply.upcast_ref());
    until(|| d.w.visible_dialog().is_none() && ui_session(&d.w).engine().document().composition().size[0] == width + 1,
        "the native header Apply commits Canvas Size");
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    until(|| ui_session(&d.w).engine().checkpoint() == checkpoint, "one Undo restores the canvas checkpoint");
    for (action, name) in [
        (CustomizationAction::ManageToolbars, "manager"),
        (CustomizationAction::NewToolbar { group: None }, "tool-picker"),
    ] {
        d.w.dispatch(UiAction::Customize { action });
        let (dialog, _) = page(&d);
        capture(&mut d, name);
        close(&d, &dialog);
    }
    for (manager_page, name) in [
        (layer_workspace::ManagerPage::Workspaces, "workspaces"),
        (layer_workspace::ManagerPage::History, "history"),
        (layer_workspace::ManagerPage::ThisWorkspace, "this-workspace"),
        (layer_workspace::ManagerPage::ToolbarLibrary, "toolbar-library"),
    ] {
        d.w.workspaces.ui.show(&d.w, manager_page);
        page(&d);
        assert_eq!(d.w.workspaces.view().page, Some(manager_page));
        if manager_page == layer_workspace::ManagerPage::ThisWorkspace {
            let frame = crate::page_dialog::parent(&d.w.visible_dialog().unwrap());
            until(|| descendants::<gtk::ToggleButton>(&frame).into_iter().filter(|tab| tab.is_mapped()).all(|tab| tab.height() > 0), "toolbar tabs have their styled allocation");
            for tab in descendants::<gtk::ToggleButton>(&frame).into_iter().filter(|tab| tab.is_mapped()) {
                let bounds = tab.compute_bounds(&frame).unwrap();
                assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "manager tab stays inside the page: {bounds:?}");
            }
        }
        capture(&mut d, name);
        if manager_page == layer_workspace::ManagerPage::ThisWorkspace {
            let frame = crate::page_dialog::parent(&d.w.visible_dialog().unwrap());
            let navigation = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
            assert!(navigation.is_mapped(), "the narrow toolbar manager presents list/page navigation");
            let list = find_named(&frame, "workspace-manager-items").unwrap().downcast::<gtk::ListBox>().unwrap();
            let new = find_named(&frame, "workspace-manager-new-toolbar").unwrap().compute_bounds(&frame).unwrap();
            assert!(new.y() + new.height() <= list.compute_bounds(&frame).unwrap().y(), "the toolbar list starts after the New Toolbar row");
            d.click(list.row_at_index(0).unwrap().upcast_ref());
            until(|| navigation.shows_content(), "activating a toolbar opens its details");
            assert!(descendants::<gtk::Label>(&frame).into_iter().filter(|label| label.is_mapped()).all(|label| label.selection_bounds().is_none()), "opening details never selects description text");
            capture(&mut d, "this-workspace-details");
        }
        d.w.workspaces.ui.close(&d.w);
        until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "the workspace manager page closes");
    }
    let alert = adw::AlertDialog::builder().heading("Discard this test draft?").build();
    alert.add_response("cancel", "Cancel");
    alert.set_close_response("cancel");
    crate::page_dialog::present_role(&alert, &d.w.window, layer_ui::DialogRole::Alert);
    until(|| alert.is_mapped(), "a compact alert keeps its native presentation");
    assert_eq!(crate::page_dialog::parent(&alert), alert.clone().upcast::<gtk::Widget>());
    alert.close();
    until(|| d.w.visible_dialog().is_none() && !alert.is_mapped(), "the compact alert closes");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    assert_eq!(active_raster(ui_session(&d.w).engine().document()), &authored);
    d.click(&d.named("zen-capy"));
    until(|| state(&d.w).phone.menu && find_named(d.w.window.upcast_ref(), "phone-menu").is_some_and(|widget| widget.is_mapped()), "the root Capy menu opens");
    super::phone::capture(&mut d, "editor-pages-menu");
    keys(&mut d, 0xff1b);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_editor_page_dark_surfaces --native-storage; 412x891 dark"]
fn native_phone_editor_page_dark_surfaces() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditorPageSurfaces");
    assert_eq!(state(&d.w).theme, Theme::Dark);
    for (command, name) in [(CommandId::Settings, "preferences"), (CommandId::ExportDocument, "export"), (CommandId::CanvasSize, "canvas-size")] {
        d.w.dispatch(UiAction::Invoke { command });
        let (dialog, frame) = page(&d);
        if command == CommandId::ExportDocument {
            until(|| find_named(&frame, "color-preview-after").and_then(|w| w.downcast::<gtk::Picture>().ok())
                .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its dark preview");
        }
        capture(&mut d, name);
        close(&d, &dialog);
    }
    d.finish();
}

#[path = "phone_page_bounds_tests.rs"]
pub(super) mod bounds;
