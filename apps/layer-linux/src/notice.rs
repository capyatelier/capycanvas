//! Native projection of the shared canvas notice: a transient bubble over the
//! canvas with the core's text and ordered actions. The core owns the text,
//! the actions, their availability and stale-id validation; this widget owns
//! the timeout and hides at the next canvas contact.
use crate::workspace::Workspace;
use gtk::{glib, prelude::*};
use layer_ui::{Notice, NoticeAction, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(4);

pub struct NoticeBubble {
    pub root: gtk::Box,
    text: gtk::Label,
    actions: gtk::Box,
    published: RefCell<Vec<NoticeAction>>,
    shown: Cell<Option<u64>>,
    timeout: RefCell<Option<glib::SourceId>>,
}

impl NoticeBubble {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        root.set_widget_name("canvas-notice");
        root.add_css_class("workspace-notice");
        root.add_css_class("canvas-notice");
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::End);
        root.set_visible(false);
        root.update_property(&[gtk::accessible::Property::Label("Canvas notice")]);
        let text = gtk::Label::new(None);
        text.set_wrap(true);
        text.set_xalign(0.);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        actions.set_widget_name("canvas-notice-actions");
        actions.set_visible(false);
        root.append(&text);
        root.append(&actions);
        Rc::new(Self {
            root,
            text,
            actions,
            published: RefCell::new(Vec::new()),
            shown: Cell::new(None),
            timeout: RefCell::new(None),
        })
    }

    fn rebuild_actions(self: &Rc<Self>, workspace: &Rc<Workspace>, actions: &[NoticeAction]) {
        if *self.published.borrow() == actions { return; }
        while let Some(child) = self.actions.first_child() { self.actions.remove(&child); }
        for action in actions {
            let button = gtk::Button::with_label(&action.label);
            button.set_widget_name("canvas-notice-action");
            button.add_css_class("flat");
            button.set_focus_on_click(false);
            button.set_sensitive(action.enabled);
            button.set_tooltip_text(action.reason.as_deref());
            let token = action.id;
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = bubble)]
                self,
                #[weak]
                workspace,
                move |_| {
                    if let Some(id) = bubble.shown.get() {
                        bubble.hide();
                        workspace.dispatch(UiAction::Notice { id, accept: true, action: Some(token) });
                    }
                }
            ));
            self.actions.append(&button);
        }
        self.actions.set_visible(!actions.is_empty());
        *self.published.borrow_mut() = actions.to_vec();
    }

    /// Show a notice the first time its id is published, `bottom` pixels above
    /// the window's bottom edge; a cleared notice hides.
    pub fn publish(self: &Rc<Self>, workspace: &Rc<Workspace>, notice: Option<&Notice>, bottom: i32) {
        let Some(notice) = notice else {
            self.hide();
            self.shown.set(None);
            return;
        };
        self.text.set_text(&notice.text);
        self.rebuild_actions(workspace, &notice.actions);
        if self.shown.replace(Some(notice.id)) == Some(notice.id) {
            return;
        }
        self.place(bottom);
        self.root.set_visible(true);
        self.cancel_timeout();
        let id = notice.id;
        let source = glib::timeout_add_local_once(
            TIMEOUT,
            glib::clone!(
                #[weak(rename_to = bubble)]
                self,
                #[weak]
                workspace,
                move || {
                    bubble.timeout.borrow_mut().take();
                    bubble.root.set_visible(false);
                    workspace.decline_notice(id);
                }
            ),
        );
        *self.timeout.borrow_mut() = Some(source);
    }

    pub fn place(&self, bottom: i32) {
        self.root.set_margin_bottom(bottom);
    }

    /// A canvas contact dismisses the notice without answering its action.
    pub fn hide(&self) {
        self.cancel_timeout();
        self.root.set_visible(false);
    }

    pub fn cancel_timeout(&self) {
        if let Some(source) = self.timeout.borrow_mut().take() {
            source.remove();
        }
    }
}
