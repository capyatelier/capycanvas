use adw::prelude::*;
use gtk::glib;
use crate::workspace::Workspace;
use super::{close_dialog, respond};

#[derive(Clone, Default)]
pub enum Action { #[default] Close, Button(gtk::Button), Response(String) }
#[derive(Clone, Default)]
pub struct Metadata {
    pub(super) intent: Option<layer_ui::DialogIntent>,
    pub(super) title: Option<(gtk::Widget, &'static str)>,
    pub(super) header: Option<(adw::ToolbarView, adw::HeaderBar)>,
    pub(super) navigation_root: Option<adw::HeaderBar>,
    pub(super) top_navigation: Option<(adw::NavigationSplitView, adw::NavigationPage)>,
    pub(super) primary: Option<Action>,
    pub(super) cancel: Action,
    pub(super) back: Option<gtk::Button>,
    pub(super) secondary: Vec<Action>,
    pub(super) hidden: Vec<gtk::Widget>,
    pub(super) headers: Vec<adw::HeaderBar>,
}
impl Metadata {
    pub fn new() -> Self { Self::default() }
    pub fn intent(mut self, intent: layer_ui::DialogIntent) -> Self {
        if intent.primary == Some(layer_ui::DialogPrimary::Close) { self.primary = Some(Action::Close); }
        for header in &self.headers { header.set_widget_name("dialog-header"); }
        self.intent = Some(intent); self
    }
    pub fn title(mut self, widget: &impl IsA<gtk::Widget>, property: &'static str) -> Self { self.title = Some((widget.clone().upcast(), property)); self.hide(widget) }
    pub fn hide(mut self, widget: &impl IsA<gtk::Widget>) -> Self { self.hidden.push(widget.clone().upcast()); self }
    pub fn header(mut self, view: &adw::ToolbarView, header: &adw::HeaderBar) -> Self { self.header = Some((view.clone(), header.clone())); self.navigation_header(header) }
    pub fn root_header(mut self, header: &adw::HeaderBar) -> Self { self.navigation_root = Some(header.clone()); self.navigation_header(header) }
    pub fn top_navigation(mut self, split: &adw::NavigationSplitView, page: &adw::NavigationPage, inner: &adw::HeaderBar) -> Self {
        self.top_navigation = Some((split.clone(), page.clone())); self.navigation_header(inner).hide(inner)
    }
    pub fn navigation_header(mut self, header: &adw::HeaderBar) -> Self { self.headers.push(header.clone()); self }
    pub fn primary(mut self, button: &gtk::Button) -> Self { self.primary = Some(Action::Button(button.clone())); self }
    pub fn done(mut self) -> Self { self.primary = Some(Action::Close); self }
    pub fn cancel(mut self, button: &gtk::Button) -> Self { self.cancel = Action::Button(button.clone()); self.hide(button) }
    pub fn back(mut self, button: &gtk::Button) -> Self { self.back = Some(button.clone()); self }
    pub fn responses(primary: Option<&str>, cancel: &str, secondary: &[&str]) -> Self {
        Self { primary: primary.map(|id| Action::Response(id.into())), cancel: Action::Response(cancel.into()),
            secondary: secondary.iter().map(|id| Action::Response((*id).into())).collect(), ..Self::default() }
    }
}
pub fn configure(dialog: &impl IsA<adw::Dialog>, metadata: Metadata) { unsafe { dialog.as_ref().set_data("capy-page-metadata", metadata); } }
pub(super) fn get(dialog: &adw::Dialog) -> Metadata { unsafe { dialog.data::<Metadata>("capy-page-metadata").map(|data| data.as_ref().clone()).unwrap_or_default() } }
pub fn intent(dialog: &impl IsA<adw::Dialog>, intent: layer_ui::DialogIntent) { configure(dialog, get(dialog.as_ref()).intent(intent)); }
pub fn primary(dialog: &impl IsA<adw::Dialog>, action: Option<Action>) { let mut meta = get(dialog.as_ref()); meta.primary = action; configure(dialog, meta); }
impl Action {
    pub(super) fn activate(&self, dialog: &adw::Dialog) {
        match self { Self::Close => { close_dialog(dialog); }, Self::Button(button) => button.emit_clicked(),
            Self::Response(id) => respond(dialog.downcast_ref::<adw::AlertDialog>().unwrap(), id) }
    }
    pub(super) fn project(&self, dialog: &adw::Dialog, w: &Workspace) -> gtk::Button {
        let button = gtk::Button::new();
        match self {
            Self::Close => button.set_label(&layer_ui::CommonCopy::new(&w.localization()).done),
            Self::Button(source) => {
                source.bind_property("label", &button, "label").sync_create().build();
                source.bind_property("sensitive", &button, "sensitive").sync_create().build();
                button.set_use_underline(source.uses_underline());
            }
            Self::Response(id) => {
                let alert = dialog.downcast_ref::<adw::AlertDialog>().unwrap();
                button.set_label(&alert.response_label(id)); button.set_sensitive(alert.is_response_enabled(id)); button.set_use_underline(true);
            }
        }
        let action = self.clone();
        button.connect_clicked(glib::clone!(#[weak] dialog, move |_| action.activate(&dialog)));
        button
    }
}

pub(super) struct Header {
    pub header: adw::HeaderBar,
    pub loan: Option<adw::ToolbarView>,
    pub previous_title: Option<gtk::Widget>,
    pub projected: Vec<(adw::HeaderBar, gtk::Widget)>,
    pub hidden: Vec<gtk::Widget>,
    pub responses: Vec<(String, gtk::Button)>,
    pub page_primary: Option<gtk::Button>,
    pub close: gtk::Button,
    pub bindings: Vec<(glib::Object, glib::SignalHandlerId)>,
}
impl Metadata {
    fn dialog_intent(&self) -> layer_ui::DialogIntent {
        self.intent.unwrap_or_else(|| layer_ui::DialogIntent::editor(self.primary.as_ref().map(|action|
            if matches!(action, Action::Close) { layer_ui::DialogPrimary::Close } else { layer_ui::DialogPrimary::Action })))
    }
    pub(super) fn project_header(&self, dialog: &adw::Dialog, w: &std::rc::Rc<Workspace>, frame: &gtk::Box) -> Header {
        let navigation = self.navigation_root.is_some();
        let header = self.navigation_root.clone().or_else(|| self.header.as_ref().map(|(_, header)| header.clone())).unwrap_or_default();
        header.set_widget_name("dialog-header");
        let loan = self.header.as_ref().map(|(view, _)| view.clone());
        if let Some(toolbar) = &loan { toolbar.remove(&header); }
        let previous_title = header.title_widget();
        let mut projected = Vec::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        let title = gtk::Label::new(None);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_width_chars(1);
        if self.top_navigation.is_none() {
            if let Some((source, property)) = &self.title { source.bind_property(property, &title, "label").sync_create().build(); }
            else { dialog.bind_property("title", &title, "label").sync_create().build(); }
        }
        if navigation { frame.update_property(&[gtk::accessible::Property::Label(&dialog.title())]); }
        else {
            frame.update_relation(&[gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()])]);
            header.set_title_widget(Some(&title));
        }
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        close.set_widget_name("dialog-start");
        let intent = self.dialog_intent();
        close.set_visible(intent.header(false).start.is_some());
        let cancel_label = layer_ui::CommonCopy::new(&w.localization()).cancel;
        close.set_tooltip_text(Some(&cancel_label));
        close.update_property(&[gtk::accessible::Property::Label(&cancel_label)]);
        let cancel = self.cancel.clone();
        let top_navigation = self.top_navigation.as_ref().map(|(split, _)| split.clone());
        close.connect_clicked(glib::clone!(#[weak] dialog, move |_| {
            if let Some(split) = top_navigation.as_ref().filter(|split| split.is_mapped() && split.shows_content()) { split.set_show_content(false); }
            else { cancel.activate(&dialog); }
        }));
        let bindings = self.top_navigation.as_ref().map_or_else(Vec::new, |(split, page)| bind_navigation(split, page, dialog, &title, &close, intent, w));
        header.pack_start(&close);
        if let Some(row) = close.parent().and_downcast::<gtk::Box>() { row.reorder_child_after(&close, gtk::Widget::NONE); }
        projected.push((header.clone(), close.clone().upcast()));
        let mut hidden = self.hidden.clone();
        let mut responses = Vec::new();
        let mut page_primary = None;
        if let Some(action) = self.primary.as_ref().filter(|_| intent.header(false).end.is_some()) {
            let destinations = if navigation { self.headers.clone() } else { vec![header.clone()] };
            for destination in destinations {
                if let Action::Button(button) = action {
                    if !navigation && button.is_ancestor(&destination) { page_primary = Some(button.clone()); continue; }
                    hidden.push(button.clone().upcast());
                }
                let button = action.project(dialog, w);
                button.add_css_class("suggested-action");
                button.set_widget_name("dialog-end");
                destination.pack_end(&button);
                if let Action::Response(id) = action { responses.push((id.clone(), button.clone())); }
                button.connect_map(glib::clone!(#[weak] w, move |button| w.window.set_default_widget(Some(button))));
                projected.push((destination, button.clone().upcast()));
                page_primary.get_or_insert(button);
            }
        }
        Header { header, loan, previous_title, projected, hidden, responses, page_primary, close, bindings }
    }
}

fn bind_navigation(split: &adw::NavigationSplitView, page: &adw::NavigationPage, dialog: &adw::Dialog, title: &gtk::Label, close: &gtk::Button, intent: layer_ui::DialogIntent, w: &Workspace) -> Vec<(glib::Object, glib::SignalHandlerId)> {
    let copy = layer_ui::CommonCopy::new(&w.localization());
    let update = std::rc::Rc::new(glib::clone!(#[weak] split, #[weak] page, #[weak] dialog, #[weak] title, #[weak] close, move |mapped: bool| {
        let subpage = mapped && split.shows_content();
        let caption = if subpage { page.title() } else { dialog.title() };
        title.set_label(&caption);
        let action = intent.header(subpage).start;
        close.set_visible(action.is_some());
        let back = action == Some(layer_ui::DialogHeaderAction::Back);
        close.set_icon_name(if back { "go-previous-symbolic" } else { "window-close-symbolic" });
        let label = if back { &copy.back } else { &copy.cancel };
        close.set_tooltip_text(Some(label));
        close.update_property(&[gtk::accessible::Property::Label(label)]);
    }));
    update(split.is_mapped());
    vec![
        (split.clone().upcast(), split.connect_show_content_notify(glib::clone!(#[strong] update, move |split| update(split.is_mapped())))),
        (split.clone().upcast(), split.connect_map(glib::clone!(#[strong] update, move |_| update(true)))),
        (split.clone().upcast(), split.connect_unmap(glib::clone!(#[strong] update, move |_| update(false)))),
        (page.clone().upcast(), page.connect_title_notify(glib::clone!(#[strong] update, #[weak] split, move |_| update(split.is_mapped())))),
        (dialog.clone().upcast(), dialog.connect_title_notify(glib::clone!(#[strong] update, #[weak] split, move |_| update(split.is_mapped())))),
    ]
}
