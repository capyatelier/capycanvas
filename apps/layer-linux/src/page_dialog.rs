use adw::prelude::*;
use gtk::glib;
use crate::workspace::Workspace;
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}};

thread_local! { static WINDOWS: RefCell<Vec<Weak<Workspace>>> = const { RefCell::new(Vec::new()) }; }

#[path = "page_dialog_metadata.rs"]
mod specification;
pub use specification::{Action, Metadata, configure, primary};

fn viewport(w: &Workspace) -> [f32; 2] {
    let parent = w.pages.root.parent().unwrap();
    [parent.width() as f32, parent.height() as f32]
}
pub fn single_pane(w: &Workspace) -> bool {
    w.gpu.borrow().as_ref().is_some_and(|gpu|
        gpu.session.state().dialog_presentation(layer_ui::DialogRole::Editor, viewport(w)).single_pane())
}

struct Page {
    dialog: adw::Dialog,
    body: gtk::Widget,
    frame: gtk::Box,
    controllers: Vec<gtk::EventController>,
    keys: gtk::EventControllerKey,
    hidden: Vec<(gtk::Widget, bool)>,
    responses: Vec<(String, gtk::Button)>,
    headers: Vec<(adw::HeaderBar, bool, bool)>,
    responded: Rc<Cell<bool>>,
    closed: RefCell<Option<glib::SignalHandlerId>>,
    default: Option<glib::WeakRef<gtk::Widget>>,
    dialog_default: Option<gtk::Widget>,
    header: adw::HeaderBar,
    loan: Option<adw::ToolbarView>,
    title: Option<gtk::Widget>,
    projected: Vec<(adw::HeaderBar, gtk::Widget)>,
    bindings: RefCell<Vec<(glib::Object, glib::SignalHandlerId)>>,
    response: RefCell<Option<glib::SignalHandlerId>>,
    focus: Option<glib::WeakRef<gtk::Widget>>,
    closing: Cell<bool>,
    role: layer_ui::DialogRole,
}

pub struct Pages {
    pub root: gtk::Stack,
    empty: gtk::Box,
    pages: RefCell<Vec<Rc<Page>>>,
}

impl Pages {
    pub fn new() -> Self {
        let root = gtk::Stack::builder().hhomogeneous(false).vhomogeneous(false).visible(false)
            .transition_type(gtk::StackTransitionType::SlideDown).transition_duration(200).build();
        root.set_widget_name("dialog-pages");
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_child(&empty);
        Self { root, empty, pages: RefCell::default() }
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        WINDOWS.with_borrow_mut(|windows| { windows.retain(|window| window.strong_count() > 0); windows.push(Rc::downgrade(w)); });
        self.root.connect_transition_running_notify(glib::clone!(#[weak] w, move |stack| {
            if !stack.is_transition_running() { w.pages.finish(); }
        }));
    }
    pub fn visible(&self) -> Option<adw::Dialog> { self.pages.borrow().iter().rev().find(|page| !page.closing.get()).map(|page| page.dialog.clone()) }
    fn contains(&self, widget: &gtk::Widget) -> bool {
        self.pages.borrow().iter().any(|page| page.dialog.upcast_ref::<gtk::Widget>() == widget)
    }
    pub fn allocation(&self, w: &Workspace) -> gtk::gdk::Rectangle {
        let viewport = viewport(w);
        let pages = self.pages.borrow();
        let page = pages.iter().rev().find(|page| !page.closing.get()).or_else(|| pages.last());
        let role = page.map_or(layer_ui::DialogRole::Editor, |page| page.role);
        let presentation = w.gpu.borrow().as_ref().map(|gpu| gpu.session.state().dialog_presentation(role, viewport));
        let measured = || page.map_or(viewport, |page| {
            let width = page.frame.measure(gtk::Orientation::Horizontal, -1).1.min(viewport[0] as i32);
            [width as f32, page.frame.measure(gtk::Orientation::Vertical, width).1 as f32]
        });
        let bounds = presentation.and_then(|presentation| presentation.bounds(measured))
            .unwrap_or(layer_ui::Bounds { x: 0., y: 0., width: viewport[0], height: viewport[1] });
        gtk::gdk::Rectangle::new(bounds.x.round() as i32, bounds.y.round() as i32, bounds.width.round() as i32, bounds.height.round() as i32)
    }
    fn show(&self, w: &Rc<Workspace>, dialog: &adw::Dialog, role: layer_ui::DialogRole, meta: Metadata) {
        if self.pages.borrow().iter().any(|page| page.dialog == *dialog && page.closing.get()) { self.finish(); }
        let shown = self.pages.borrow().iter().find(|page| page.dialog == *dialog).cloned();
        if let Some(page) = shown {
            self.root.set_visible_child(&page.frame);
            return;
        }
        let Some(native_body) = dialog.child() else { return; };
        let dialog_default = dialog.default_widget();
        let headers = meta.headers.iter()
            .map(|header| (header.clone(), header.shows_start_title_buttons(), header.shows_end_title_buttons())).collect::<Vec<_>>();
        let preferred_focus = dialog.focus().filter(|widget| !widget.is::<gtk::Label>());
        let alert = dialog.downcast_ref::<adw::AlertDialog>();
        let body = alert.map(|alert| alert.extra_child().unwrap_or_else(|| gtk::Box::new(gtk::Orientation::Vertical, 0).upcast())).unwrap_or_else(|| native_body.clone());
        let focus = gtk::prelude::GtkWindowExt::focus(&w.window).map(|widget| widget.downgrade());
        w.interact(layer_ui::UiInput::Blur);
        if let Some(alert) = alert { alert.set_extra_child(gtk::Widget::NONE); } else { dialog.set_child(gtk::Widget::NONE); }
        let frame = gtk::Box::builder().orientation(gtk::Orientation::Vertical).accessible_role(gtk::AccessibleRole::Dialog).build();
        frame.set_limit_events(true);
        frame.set_widget_name(&format!("page-{}", dialog.widget_name()));
        frame.add_css_class("phone-dialog-page");
        let page_classes = frame.css_classes();
        dialog.bind_property("css-classes", &frame, "css-classes")
            .transform_to(move |_, mut classes: Vec<String>| {
                classes.extend(page_classes.iter().map(ToString::to_string));
                Some(classes)
            }).sync_create().build();
        frame.update_property(&[gtk::accessible::Property::Modal(meta.intent.is_none_or(|intent| intent.modal))]);
        let navigation = meta.navigation_root.is_some();
        let specification::Header { header, loan, previous_title, projected, hidden, mut responses, page_primary, close, bindings } = meta.project_header(dialog, w, &frame);
        let default = gtk::prelude::GtkWindowExt::default_widget(&w.window).map(|widget| widget.downgrade());
        let page_default = if let Some(alert) = alert {
            if alert.default_response().as_deref() == Some(alert.close_response().as_str()) && close.is_visible() { Some(close.clone()) }
            else { page_primary.clone() }
        } else { page_primary.clone() };
        for (native, _, _) in &headers {
            native.set_show_start_title_buttons(false);
            native.set_show_end_title_buttons(false);
        }
        if !navigation { frame.append(&header); }
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.set_vexpand(true);
        if alert.is_some() { content.set_margin_start(18); content.set_margin_end(18); content.set_margin_bottom(18); }
        if let Some(alert) = alert && !alert.body().is_empty() {
            let description = gtk::Label::builder().wrap(true).xalign(0.).margin_top(12).margin_bottom(12).build();
            alert.bind_property("body", &description, "label").sync_create().build();
            description.set_use_markup(alert.is_body_use_markup());
            content.append(&description);
        }
        if alert.is_some() {
            let holder = gtk::Box::new(gtk::Orientation::Vertical, 0);
            holder.append(&body);
            let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&holder).build());
            content.append(&scroll);
        } else { body.set_vexpand(true); content.append(&body); }
        for action in meta.secondary {
            if let Action::Response(id) = &action && alert.is_none_or(|alert| !alert.has_response(id)) { continue; }
            let button = action.project(dialog, w);
            if let Action::Response(id) = action { responses.push((id, button.clone())); }
            content.append(&button);
        }
        let mut saved = Vec::new();
        for widget in hidden { if !saved.iter().any(|(saved, _)| *saved == widget) { saved.push((widget.clone(), widget.get_visible())); widget.set_visible(false); } }
        frame.append(&content);
        let controllers = dialog.observe_controllers().iter::<glib::Object>().filter_map(|object| object.ok()?.downcast::<gtk::EventController>().ok())
            .filter(|controller| !controller.is::<gtk::ShortcutController>()).collect::<Vec<_>>();
        for controller in &controllers { dialog.remove_controller(controller); frame.add_controller(controller.clone()); }
        let keys = gtk::EventControllerKey::new();
        let cancel = meta.cancel;
        let back = meta.back;
        let handle = Rc::new(glib::clone!(#[weak] dialog, #[weak] frame, #[upgrade_or] glib::Propagation::Proceed, move |key: gtk::gdk::Key, modifiers: gtk::gdk::ModifierType| {
            if !owner(dialog.upcast_ref()).is_some_and(|w| w.pages.visible().as_ref() == Some(&dialog)) { return glib::Propagation::Proceed; }
            if key == gtk::gdk::Key::Tab || key == gtk::gdk::Key::ISO_Left_Tab {
                let direction = if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) { gtk::DirectionType::TabBackward } else { gtk::DirectionType::TabForward };
                if !frame.child_focus(direction) { frame.set_focus_child(gtk::Widget::NONE); frame.child_focus(direction); }
                return glib::Propagation::Stop;
            }
            if key == gtk::gdk::Key::Escape {
                match layer_ui::dialog_escape(back.as_ref().is_some_and(|back| back.is_mapped()),false,false) {
                    layer_ui::DialogEscape::Back => back.as_ref().unwrap().emit_clicked(),
                    _ => cancel.activate(&dialog),
                }
                glib::Propagation::Stop
            } else { glib::Propagation::Proceed }
        }));
        keys.connect_key_pressed(glib::clone!(#[strong] handle, move |_, key, _, modifiers| handle(key, modifiers)));
        frame.add_controller(keys);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(#[weak] w, #[weak] frame, #[upgrade_or] glib::Propagation::Proceed, move |_, key, _, modifiers| {
            if gtk::prelude::GtkWindowExt::focus(&w.window).is_none_or(|focus| focus != frame && !focus.is_ancestor(&frame)) {
                handle(key, modifiers)
            } else { glib::Propagation::Proceed }
        }));
        w.window.add_controller(keys.clone());
        let response = dialog.downcast_ref::<adw::AlertDialog>().map(|alert| alert.connect_response(None,
            glib::clone!(#[weak] w, move |alert, _| { w.pages.close(alert.upcast_ref()); })));
        let responded = Rc::new(Cell::new(false));
        let closed = alert.map(|alert| alert.connect_closed(glib::clone!(#[strong] responded, move |alert| {
            if responded.get() { alert.stop_signal_emission_by_name("closed"); }
        })));
        let page = Rc::new(Page { dialog: dialog.clone(), body, frame, controllers, keys, hidden: saved, responses, headers, responded, closed: RefCell::new(closed), default, dialog_default, response: RefCell::new(response), focus, closing: Cell::new(false), role, header, loan, title: previous_title, projected, bindings: RefCell::new(bindings) });
        self.root.add_child(&page.frame);
        self.pages.borrow_mut().push(page.clone());
        self.root.set_visible(true);
        self.root.set_transition_type(gtk::StackTransitionType::SlideDown);
        self.root.set_visible_child(&page.frame);
        let page_default = page_default.filter(|button| button.is_mapped()).or_else(|| page.projected.iter().find_map(|(_, widget)|
            widget.downcast_ref::<gtk::Button>().filter(|button| button.is_mapped() && button.has_css_class("suggested-action")).cloned()));
        w.window.set_default_widget(page_default.as_ref());
        gtk::prelude::GtkWindowExt::set_focus(&w.window, preferred_focus.as_ref().filter(|widget| widget.is_visible()));
    }
    fn close(&self, dialog: &adw::Dialog) -> bool {
        let Some(index) = self.pages.borrow().iter().position(|page| page.dialog == *dialog) else { return false; };
        let closed = self.pages.borrow()[index..].iter().rev().filter(|page| !page.closing.replace(true)).cloned().collect::<Vec<_>>();
        let focus = closed.last().and_then(|page| page.focus.clone());
        for page in closed {
            if let Some(widget) = page.keys.widget() { widget.remove_controller(&page.keys); }
            if let Some(handler) = page.response.take() { page.dialog.disconnect(handler); }
            if let Some(w) = owner(dialog.upcast_ref()) {
                w.window.set_default_widget(page.default.as_ref().and_then(glib::WeakRef::upgrade).as_ref());
            }
            page.dialog.emit_by_name::<()>("closed", &[]);
        }
        self.root.set_transition_type(gtk::StackTransitionType::SlideUp);
        let next = self.pages.borrow().iter().rev().find(|page| !page.closing.get()).map(|page| page.frame.clone());
        self.root.set_visible_child(next.as_ref().unwrap_or(&self.empty));
        if let Some(focus) = focus.and_then(|focus| focus.upgrade())
            && next.as_ref().is_none_or(|frame| focus.is_ancestor(frame)) { focus.grab_focus(); }
        if !self.root.is_transition_running() { self.finish(); }
        true
    }
    fn finish(&self) {
        let closed = self.pages.borrow_mut().extract_if(.., |page| page.closing.get()).collect::<Vec<_>>();
        for page in closed {
            for (object, handler) in page.bindings.borrow_mut().drain(..) { object.disconnect(handler); }
            if let Some(handler) = page.response.take() { page.dialog.disconnect(handler); }
            if let Some(parent) = page.body.parent().and_downcast::<gtk::Box>() { parent.remove(&page.body); }
            if let Some(alert) = page.dialog.downcast_ref::<adw::AlertDialog>() { alert.set_extra_child(Some(&page.body)); }
            else { page.dialog.set_child(Some(&page.body)); }
            if let Some(handler) = page.closed.take() { page.dialog.disconnect(handler); }
            for controller in &page.controllers { page.frame.remove_controller(controller); page.dialog.add_controller(controller.clone()); }
            for (widget, visible) in &page.hidden { widget.set_visible(*visible); }
            for (header, start, end) in &page.headers { header.set_show_start_title_buttons(*start); header.set_show_end_title_buttons(*end); }
            if let Some(toolbar) = &page.loan {
                page.frame.remove(&page.header);
                page.header.set_title_widget(page.title.as_ref());
                toolbar.add_top_bar(&page.header);
            }
            for (header, widget) in &page.projected { header.remove(widget); }
            page.dialog.set_default_widget(page.dialog_default.as_ref());
            self.root.remove(&page.frame);
        }
        self.root.set_visible(!self.pages.borrow().is_empty());
    }
}

fn owner(widget: &gtk::Widget) -> Option<Rc<Workspace>> {
    WINDOWS.with_borrow(|windows| windows.iter().filter_map(Weak::upgrade).find(|w|
        widget.root().as_ref() == Some(w.window.upcast_ref()) || w.pages.contains(widget)))
}

pub fn parent(widget: &impl IsA<gtk::Widget>) -> gtk::Widget {
    owner(widget.as_ref()).and_then(|w| w.pages.pages.borrow().iter().find(|page|
        page.dialog.upcast_ref::<gtk::Widget>() == widget.as_ref()).map(|page| page.frame.clone().upcast()))
        .unwrap_or_else(|| widget.as_ref().clone())
}

pub fn present(dialog: &impl IsA<adw::Dialog>, parent_widget: &impl IsA<gtk::Widget>) {
    present_role(dialog, parent_widget, layer_ui::DialogRole::Editor);
}

pub fn present_intent(dialog: &impl IsA<adw::Dialog>, parent_widget: &impl IsA<gtk::Widget>, intent: layer_ui::DialogIntent) {
    present_with(dialog, parent_widget, intent.role, specification::get(dialog.as_ref()).intent(intent));
}

pub fn present_role(dialog: &impl IsA<adw::Dialog>, parent_widget: &impl IsA<gtk::Widget>, role: layer_ui::DialogRole) {
    present_with(dialog, parent_widget, role, specification::get(dialog.as_ref()));
}

pub fn present_with(dialog: &impl IsA<adw::Dialog>, parent_widget: &impl IsA<gtk::Widget>, role: layer_ui::DialogRole, metadata: Metadata) {
    let dialog = dialog.as_ref();
    if let Some(w) = owner(parent_widget.as_ref()) {
        let viewport = [w.surface.width() as f32, w.surface.height() as f32];
        let page = w.gpu.borrow().as_ref().is_some_and(|gpu| gpu.session.state().dialog_presentation(role, viewport) != layer_ui::DialogPresentation::Floating);
        if page { w.pages.show(&w, dialog, role, metadata); return; }
    }
    dialog.present(Some(&parent(parent_widget)));
}

pub fn close_dialog(dialog: &impl IsA<adw::Dialog>) -> bool {
    let dialog = dialog.as_ref();
    if !dialog.can_close() { dialog.emit_by_name::<()>("close-attempt", &[]); return false; }
    if let Some(w) = owner(dialog.upcast_ref()) { if w.pages.close(dialog) { return true; } }
    dialog.parent().is_some() && dialog.close()
}

pub fn force_close(dialog: &impl IsA<adw::Dialog>) {
    let dialog = dialog.as_ref();
    if let Some(w) = owner(dialog.upcast_ref()) { if w.pages.close(dialog) { return; } }
    if dialog.parent().is_some() { dialog.force_close(); }
}

fn respond(alert: &adw::AlertDialog, response: &str) {
    if let Some(w) = owner(alert.upcast_ref()) {
        if let Some(page) = w.pages.pages.borrow().iter().find(|page| page.dialog == *alert.upcast_ref::<adw::Dialog>()) { page.responded.set(true); }
        alert.emit_by_name_with_details::<()>("response", glib::Quark::from_str(response), &[&response]);
        w.pages.close(alert.upcast_ref());
    }
}

fn update_response(dialog: &adw::AlertDialog, id: &str, update: impl Fn(&gtk::Button)) {
    let buttons = owner(dialog.upcast_ref()).map(|w| w.pages.pages.borrow().iter()
        .filter(|page| page.dialog == *dialog.upcast_ref::<adw::Dialog>())
        .flat_map(|page| page.responses.iter().filter(|(response, _)| response == id).map(|(_, button)| button.clone())).collect::<Vec<_>>()).unwrap_or_default();
    for button in buttons { update(&button); }
}
pub fn set_response_label(dialog: &adw::AlertDialog, id: &str, label: &str) { dialog.set_response_label(id, label); update_response(dialog, id, |button| button.set_label(label)); }
pub fn set_response_enabled(dialog: &adw::AlertDialog, id: &str, enabled: bool) { dialog.set_response_enabled(id, enabled); update_response(dialog, id, |button| button.set_sensitive(enabled)); }

pub fn close_window(window: &impl IsA<gtk::Widget>) -> bool {
    owner(window.as_ref()).and_then(|w| w.pages.visible()).is_some_and(|dialog| { close_dialog(&dialog); true })
}

pub fn content(dialog: &impl IsA<adw::Dialog>) -> Option<gtk::Widget> {
    let dialog = dialog.as_ref();
    owner(dialog.upcast_ref()).and_then(|w| w.pages.pages.borrow().iter().find(|page| page.dialog == *dialog).map(|page| page.body.clone()))
        .or_else(|| dialog.downcast_ref::<adw::AlertDialog>().and_then(|alert| alert.extra_child()))
        .or_else(|| dialog.child())
}

pub fn hide(dialog: &impl IsA<adw::Dialog>) {
    let dialog = dialog.as_ref();
    if let Some(w) = owner(dialog.upcast_ref()) && w.pages.close(dialog) { w.pages.finish(); }
    else { close_dialog(dialog); }
}
