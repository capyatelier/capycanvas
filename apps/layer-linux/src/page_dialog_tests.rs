use super::*;
use super::workspace_header::Driver;

#[test]
#[ignore = "private compositor: --native-test=native_new_drawing_preview_spacing --native-storage; viewport/theme external"]
fn native_new_drawing_preview_spacing() {
    let mut d = super::phone_motion::setup("art.capycanvas.NewDrawingPreviewSpacing");
    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (_, frame) = page_frame(&d);
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
    let (dialog, frame) = page_frame(&d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    let appearance = SettingsPage::Appearance.localized_title(&d.w.localization());
    d.click(&mapped_label(&frame, &appearance).unwrap());
    until(|| split.shows_content(), "Preferences opens Appearance");
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
    let (dialog, frame) = page_frame(&d);
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
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_export_narrow_bounds --native-storage; viewport/theme external"]
fn native_export_narrow_bounds() {
    let mut d = super::phone_motion::setup("art.capycanvas.ExportNarrowBounds");
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (dialog, frame) = page_frame(&d);
    until(|| find_named(&frame, "color-preview-after").and_then(|widget| widget.downcast::<gtk::Picture>().ok())
        .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its preview");
    for name in ["export-format", "export-open-size", "export-open-color", "color-preview-before", "color-preview-after"] {
        let bounds = find_named(&frame, name).unwrap().compute_bounds(&frame).unwrap();
        assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32 + 1., "Export control fits: {name} {bounds:?}");
    }
    page(&d);
    super::phone::capture(&mut d, "export-narrow-bounds");
    close(&d, &dialog);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_edit_color_narrow_bounds --native-storage; viewport/theme external"]
fn native_edit_color_narrow_bounds() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditColorNarrowBounds");
    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, frame) = page_frame(&d);
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
    let (dialog, frame) = page_frame(&d);
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

fn page_frame(d: &Driver) -> (adw::Dialog, gtk::Widget) {
    until(|| d.w.visible_dialog().is_some_and(|dialog| {
        let frame = crate::page_dialog::parent(&dialog);
        frame != dialog.clone().upcast::<gtk::Widget>() && frame.is_mapped()
            && !d.w.pages.root.is_transition_running()
    }), "the editor page is mapped and settled");
    let dialog = d.w.visible_dialog().unwrap();
    let frame = crate::page_dialog::parent(&dialog);
    (dialog, frame)
}

fn page(d: &Driver) -> (adw::Dialog, gtk::Widget) {
    let (dialog, frame) = page_frame(d);
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
        assert!(default.is_mapped() && (default == frame || default.is_ancestor(&frame)), "the page owns its mapped default action");
    }
    let title = header.ancestor(adw::NavigationPage::static_type()).and_downcast::<adw::NavigationPage>()
        .map_or_else(|| dialog.title(), |page| page.title());
    if !title.is_empty() { assert_eq!(descendants::<gtk::Label>(header.upcast_ref()).into_iter().filter(|label| label.is_mapped() && label.text() == title).count(), 1, "the visible header has its native page title once"); }
    (dialog, frame)
}

fn capture(d: &mut Driver, name: &str) {
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
        d.input.perform(serde_json::json!([{ "capture_window": {
            "name": format!("pages-{}x{}-{}-{name}", d.w.surface.width(), d.w.surface.height(), format!("{:?}", state(&d.w).theme).to_lowercase()),
            "maximized": d.w.window.is_maximized()
        } }]));
    }
}

fn close(d: &Driver, dialog: &adw::Dialog) {
    crate::page_dialog::close_dialog(dialog);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "the page closes");
}

fn keys(d: &mut Driver, key: u32) {
    d.input.perform(serde_json::json!([{ "key": key, "down": true }, { "key": key, "down": false }]));
}

fn export_page(d: &mut Driver, frame: &gtk::Widget, tag: &str) {
    let nav = named::<adw::NavigationView>(frame, "export-navigation");
    let row = named::<adw::ActionRow>(frame, &format!("export-open-{tag}"));
    let scroll = named::<gtk::ScrolledWindow>(frame, "export-main-scroll");
    assert!(row.grab_focus());
    until(|| row.compute_bounds(&scroll).is_some_and(|bounds| {
        let visible = bounds.y() >= -1. && bounds.y() + bounds.height() <= scroll.height() as f32 + 1.;
        row.is_mapped() && bounds.width() > 0. && bounds.height() > 0. && visible
    }), "Export link scrolls into its native viewport");
    d.click(row.upcast_ref());
    until(|| nav.visible_page_tag().as_deref() == Some(tag), "Export opens its native subpage");
    page_header(frame);
}

pub(super) fn inventory(d: &mut Driver) {
    inventory_with(d, capture);
}

fn inventory_with(d: &mut Driver, mut visit: impl FnMut(&mut Driver, &str)) {
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (dialog, frame) = page_frame(d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    if split.is_collapsed() && split.shows_content() {
        d.click(icon_button(&page_header(&frame), "go-previous-symbolic").unwrap().upcast_ref());
        until(|| !split.shows_content(), "Preferences returns to its root list");
        page_header(&frame);
    }
    visit(d, "preferences-root");
    let appearance = SettingsPage::Appearance.localized_title(&d.w.localization());
    d.click(&mapped_label(&frame, &appearance).unwrap());
    until(|| split.shows_content(), "Preferences opens its page");
    if split.is_collapsed() { page_header(&frame); }
    visit(d, "preferences-subpage");
    close(d, &dialog);

    d.w.dispatch(UiAction::Invoke { command: CommandId::NewDocument });
    let (dialog, _) = page_frame(d);
    visit(d, "new-drawing");
    close(d, &dialog);
    d.w.dispatch(UiAction::Invoke { command: CommandId::ExportDocument });
    let (dialog, frame) = page_frame(d);
    until(|| find_named(&frame, "color-preview-after").and_then(|widget| widget.downcast::<gtk::Picture>().ok())
        .is_some_and(|picture| picture.paintable().is_some()), "Export publishes its inventory preview");
    visit(d, "export");
    export_page(d, &frame, "size");
    visit(d, "export-size");
    close(d, &dialog);

    d.w.dispatch(UiAction::Invoke { command: CommandId::CanvasSize });
    let (dialog, _) = page_frame(d);
    visit(d, "canvas-size");
    close(d, &dialog);
    crate::color_editor::show(&d.w, layer_ui::ColorSlot::Foreground);
    let (dialog, _) = page_frame(d);
    visit(d, "color");
    close(d, &dialog);
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::Workspaces);
    let (dialog, _) = page_frame(d);
    visit(d, "workspaces");
    close(d, &dialog);
    d.w.workspaces.ui.show(&d.w, layer_workspace::ManagerPage::ThisWorkspace);
    let (dialog, frame) = page_frame(d);
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

#[test]
#[ignore = "private compositor: --native-test=native_phone_navigation_headers --native-storage; phone viewport, light/dark"]
fn native_phone_navigation_headers() {
    let mut d = super::phone_motion::setup("art.capycanvas.NavigationHeaders");
    assert!(d.w.surface.width().min(d.w.surface.height()) < 480);
    let checkpoint = ui_session(&d.w).engine().checkpoint();
    let original = header_state(d.w.preferences.dialog.upcast_ref());
    d.click(&d.named("zen-capy"));
    until(|| state(&d.w).phone.menu, "Capy opens its icon strip");
    d.click(&d.named("phone-preferences"));
    let (_, frame) = page(&d);
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    assert!(split.is_collapsed() && split.shows_content());
    d.click(icon_button(&navigation_header(&d, &frame, false), "go-previous-symbolic").unwrap().upcast_ref());
    until(|| !split.shows_content(), "Preferences Back opens its root list");
    let root = navigation_header(&d, &frame, true);
    let close_button = icon_button(&root, "window-close-symbolic").unwrap();
    capture(&mut d, "preferences-root");
    for settings in SettingsPage::ALL {
        let title = settings.localized_title(&d.w.localization());
        let row = mapped_label(&frame, &title).unwrap();
        super::histogram::scroll_to(&row);
        d.click(&row);
        until(|| split.shows_content(), "Preferences opens its page from the root list");
        capture(&mut d, &format!("preferences-{}", settings.key()));
        assert!(split.is_collapsed(), "Preferences {} keeps one pane on a phone", settings.key());
        let subpage = navigation_header(&d, &frame, false);
        assert!(gtk::prelude::GtkWindowExt::default_widget(&d.w.window).is_none(), "Preferences has no synthesized primary");
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
            let category_header = navigation_header(&d, &frame, false);
            let back = one_back(&category_header);
            capture(&mut d, "preferences-shortcuts-category");
            d.click(back.upcast_ref());
            until(|| ui_session(&d.w).preferences().unwrap().shortcut_page.category.is_none()
                && categories.is_mapped() && !category.is_mapped(), "category Back returns to the Shortcuts list");
            one_back(&navigation_header(&d, &frame, false));
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
        d.click(icon_button(&subpage, "go-previous-symbolic").unwrap().upcast_ref());
        until(|| !split.shows_content(), "Preferences Back preserves the dialog");
        assert_eq!(navigation_header(&d, &frame, true), root);
    }
    d.click(close_button.upcast_ref());
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "root Close closes Preferences");
    assert!(!state(&d.w).settings_open && close_button.parent().is_none());
    restored_headers(&original);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Settings });
    let (_, reopened) = page(&d);
    navigation_header(&d, &reopened, false);
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes Preferences from a subpage");
    assert!(!state(&d.w).settings_open);
    restored_headers(&original);

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
    let root = navigation_header(&d, &frame, true);
    let root_title = dialog.title();
    assert_eq!(descendants::<gtk::Label>(root.upcast_ref()).into_iter().filter(|label| label.is_mapped() && label.text() == root_title).count(), 1,
        "the manager root header shows the dialog title");
    capture(&mut d, "workspace-manager-root");
    let close_button = icon_button(&root, "window-close-symbolic").unwrap();
    let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
    assert!(split.is_mapped() && split.is_collapsed(), "Manage Toolbars root keeps one pane on a phone");
    let list = named::<gtk::ListBox>(&frame, "workspace-manager-items");
    d.click(list.row_at_index(0).unwrap().upcast_ref());
    until(|| split.shows_content(), "the workspace manager opens toolbar details");
    assert!(split.is_mapped() && split.is_collapsed(), "Manage Toolbars detail keeps one pane on a phone");
    let subpage = navigation_header(&d, &frame, false);
    assert_eq!(subpage, root, "the manager keeps its existing header at the top");
    let title = d.w.workspaces.view().details.as_ref().unwrap().title.clone();
    assert_eq!(descendants::<gtk::Label>(&frame).into_iter().filter(|label| label.is_mapped() && label.text().as_str() == title.as_str()).count(), 1,
        "the detail title appears once across its header and body");
    capture(&mut d, "workspace-manager-detail");
    d.click(icon_button(&subpage, "go-previous-symbolic").unwrap().upcast_ref());
    until(|| !split.shows_content(), "manager Back returns to its toolbar list");
    assert_eq!(navigation_header(&d, &frame, true), root);
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
    navigation_header(&d, &frame, false);
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes the manager from details");
    assert!(d.w.workspaces.view().page.is_none());
    restored_headers(&original);
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    d.finish();
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_editor_pages --native-storage; 412x891 light"]
fn native_phone_editor_pages() {
    let mut d = super::phone_motion::setup("art.capycanvas.EditorPages");
    assert_eq!([d.w.surface.width(), d.w.surface.height()], [412, 891]);
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
    let header = navigation_header(&d, &frame, true);
    let close_button = icon_button(&header, "window-close-symbolic").unwrap();
    assert!(gtk::prelude::GtkWindowExt::focus(&d.w.window).is_none_or(|focus| focus != close_button), "opening does not focus the page's close button");
    assert!(gtk::prelude::GtkWindowExt::default_widget(&d.w.window).is_none(), "Preferences declares no primary action");
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
    keys(&mut d, 0xff1b);
    until(|| d.w.visible_dialog().is_none() && !d.w.pages.root.is_transition_running(), "Escape closes Preferences without a synthesized Done");
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
        if command == CommandId::DocumentProperties { assert!(descendants::<gtk::Button>(page_header(&frame).upcast_ref()).into_iter().all(|button| !button.is_mapped() || !button.has_css_class("suggested-action")), "Document Properties declares no primary action"); }
        capture(&mut d, name);
        let scroll = crate::page_dialog::content(&dialog).and_downcast::<gtk::ScrolledWindow>();
        close(&d, &dialog);
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
    until(|| d.w.visible_dialog().is_none(), "the compact alert closes");
    assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
    assert_eq!(active_raster(ui_session(&d.w).engine().document()), &authored);
    d.click(&d.named("zen-capy"));
    until(|| d.named("phone-menu-column-0").is_mapped(), "the root Capy menu opens");
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
mod bounds;
