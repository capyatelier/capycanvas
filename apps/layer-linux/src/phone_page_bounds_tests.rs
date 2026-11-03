use super::*;
use gtk::glib;
use std::sync::{Arc, Mutex};

pub(crate) fn size_warnings() -> Arc<Mutex<Vec<String>>> {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let captured = warnings.clone();
    glib::log_set_writer_func(move |level, fields| {
        let field = |key| fields.iter().find(|field| field.key() == key).and_then(glib::LogField::value_str);
        if level == glib::LogLevel::Warning && field("GLIB_DOMAIN") == Some("Gtk")
            && let Some(message) = field("MESSAGE")
            && ["Trying to measure", "size_allocate", "minimum", "min width", "min height", "natural width", "natural height"]
                .iter().any(|text| message.contains(text)) {
            captured.lock().unwrap().push(message.to_owned());
        }
        if level == glib::LogLevel::Critical && field("GLIB_DOMAIN") == Some("Adwaita")
            && let Some(message) = field("MESSAGE") {
            captured.lock().unwrap().push(format!("Adwaita critical: {message}"));
        }
        glib::log_writer_default(level, fields)
    });
    warnings
}

fn inside_page(widget: &gtk::Widget, frame: &gtk::Widget, name: &str) {
    let bounds = widget.compute_bounds(frame).unwrap();
    assert!(bounds.x() >= 0. && bounds.x() + bounds.width() <= frame.width() as f32,
        "{name}: {} {} stays inside the page: {bounds:?} in {} px", widget.type_().name(), widget.widget_name(), frame.width());
}

fn check_page(d: &mut Driver, name: &str) {
    capture(d, name);
    let (dialog, frame) = page_frame(d);
    assert!(!dialog.is_mapped(), "{name}: the original native dialog is detached");
    inside_page(&frame, d.w.window.upcast_ref(), name);
    let bounds = frame.compute_bounds(&d.w.window).unwrap();
    assert!(bounds.y() >= 0. && bounds.y() + bounds.height() <= d.w.window.height() as f32,
        "{name}: the page stays vertically inside the window: {bounds:?}");
    let body = crate::page_dialog::content(&dialog).unwrap();
    let mut bodies = vec![body.clone()];
    for widget in widgets(&body).filter(|widget| widget.is_mapped()) {
        if widget.is::<crate::dialog_content::DialogContent>() {
            bodies.push(widget.first_child().unwrap());
        }
        if let Some(scroll) = widget.downcast_ref::<gtk::ScrolledWindow>() && let Some(child) = scroll.child() {
            bodies.push(child.downcast_ref::<gtk::Viewport>().and_then(|viewport| viewport.child()).unwrap_or(child));
        }
    }
    for body in bodies {
        let minimum = body.measure(gtk::Orientation::Horizontal, -1).0;
        assert!(minimum <= frame.width(), "{name}: actual body {} {} minimum {minimum} exceeds page width {}",
            body.type_().name(), body.widget_name(), frame.width());
        inside_page(&body, &frame, name);
    }
    let header = page_header(&frame);
    let close = icon_button(&header, "window-close-symbolic");
    let back = icon_button(&header, "go-previous-symbolic");
    assert!(close.is_some() || back.is_some(), "{name}: the page has its visible Close or Back");
    let buttons = descendants::<gtk::Button>(header.upcast_ref()).into_iter().filter(|button| button.is_mapped()).collect::<Vec<_>>();
    for button in &buttons {
        inside_page(button.upcast_ref(), &frame, name);
        let bounds = button.compute_bounds(&frame).unwrap();
        assert!(bounds.y() >= 0. && bounds.y() + bounds.height() <= frame.height() as f32,
            "{name}: the complete header button fits vertically: {bounds:?}");
        let natural = button.measure(gtk::Orientation::Horizontal, -1).1;
        assert!(bounds.width() >= natural as f32, "{name}: a header action keeps its natural width {natural}, got {}", bounds.width());
        for label in descendants::<gtk::Label>(button.upcast_ref()).into_iter().filter(|label| label.is_mapped()) {
            assert!(!label.layout().is_ellipsized(), "{name}: the header action stays readable: {}", label.text());
        }
    }
    let title = header.title_widget();
    let text = title.is_none().then(|| header.ancestor(adw::NavigationPage::static_type()).and_downcast::<adw::NavigationPage>()
        .map_or_else(|| dialog.title(), |page| page.title()));
    let labels = descendants::<gtk::Label>(title.as_ref().unwrap_or(header.upcast_ref())).into_iter()
        .filter(|label| label.is_mapped() && !label.text().is_empty() && text.as_ref().is_none_or(|text| label.text() == *text)).collect::<Vec<_>>();
    assert_eq!(labels.len(), 1, "{name}: the visible header title is present once");
    let title = &labels[0];
    inside_page(title.upcast_ref(), &frame, name);
    assert!(title.width() > 0 && title.layout().pixel_size().0 <= title.width(), "{name}: the title fits its text allocation");
    let natural = title.layout().copy();
    natural.set_width(-1);
    natural.set_ellipsize(gtk::pango::EllipsizeMode::None);
    if title.width() < natural.pixel_size().0 {
        assert!(title.ellipsize() != gtk::pango::EllipsizeMode::None && title.layout().is_ellipsized(),
            "{name}: the title ellipsizes before a header action shrinks");
    }
    let title = title.compute_bounds(&frame).unwrap();
    for button in buttons {
        let button = button.compute_bounds(&frame).unwrap();
        assert!(title.x() + title.width() <= button.x() || button.x() + button.width() <= title.x(),
            "{name}: the title does not overlap a header action: {title:?} {button:?}");
    }
    if name == "workspace-manager-detail" {
        let split = descendants::<adw::NavigationSplitView>(&frame).pop().unwrap();
        assert!(split.is_mapped() && split.shows_content(), "the inventory opens the actual manager detail page");
    }
}

#[test]
#[ignore = "private compositor: --native-test=native_phone_page_bounds --native-storage; 280x280/280x560/320x568 light/dark with captures"]
fn native_phone_page_bounds() {
    assert!(std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some(), "page bounds require compositor captures before measurement");
    let warnings = size_warnings();
    let mut d = super::super::phone_motion::setup("art.capycanvas.PhonePageBounds");
    assert!([[280, 280], [280, 560], [320, 568]].contains(&[d.w.surface.width(), d.w.surface.height()]));
    inventory_with(&mut d, |d, name| {
        check_page(d, name);
        let warnings = warnings.lock().unwrap().clone();
        assert!(warnings.is_empty(), "{name}: GTK size warnings: {warnings:?}");
    });
    d.finish();
    let warnings = warnings.lock().unwrap().clone();
    assert!(warnings.is_empty(), "GTK size warnings: {warnings:?}");
}
