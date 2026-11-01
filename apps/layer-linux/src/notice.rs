//! Native projection of the shared canvas notice: a transient bubble over the
//! canvas with the core's text and ordered actions. The core owns the text,
//! the actions, their availability and stale-id validation; this widget owns
//! the timeout and hides at the next canvas contact.
use crate::workspace::Workspace;
use gtk::{gdk, glib, prelude::*};
use layer_ui::{Notice, NoticeAction, NoticePlacement, UiAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(4);
const GAP: i32 = layer_ui::TILE_GAP as i32;

pub struct NoticeBubble {
    pub root: gtk::Box,
    text: gtk::Label,
    actions: gtk::Box,
    published: RefCell<Vec<NoticeAction>>,
    placement: Cell<NoticePlacement>,
    shown: Cell<Option<u64>>,
    timeout: RefCell<Option<glib::SourceId>>,
}

impl NoticeBubble {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, GAP);
        root.set_widget_name("canvas-notice");
        root.add_css_class("dock-panel");
        root.add_css_class("canvas-action-bar");
        root.add_css_class("canvas-notice");
        root.set_visible(false);
        let text = gtk::Label::new(None);
        text.add_css_class("canvas-action-bar-label");
        text.set_wrap(true);
        text.set_natural_wrap_mode(gtk::NaturalWrapMode::None);
        text.set_xalign(0.);
        text.set_hexpand(true);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, GAP);
        actions.set_halign(gtk::Align::End);
        actions.set_valign(gtk::Align::Center);
        actions.set_widget_name("canvas-notice-actions");
        actions.set_visible(false);
        root.append(&text);
        root.append(&actions);
        Rc::new(Self {
            root,
            text,
            actions,
            published: RefCell::new(Vec::new()),
            placement: Cell::new(NoticePlacement::default()),
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
            button.add_css_class(if action.accent { "suggested-action" } else { "flat" });
            button.add_css_class("text-tile");
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
        if actions.is_empty() { self.root.add_css_class("text-only"); } else { self.root.remove_css_class("text-only"); }
        *self.published.borrow_mut() = actions.to_vec();
    }

    /// Show a notice the first time its id is published at `placement`; a
    /// cleared notice hides.
    pub fn publish(self: &Rc<Self>, workspace: &Rc<Workspace>, notice: Option<&Notice>, placement: NoticePlacement) {
        let Some(notice) = notice else {
            self.hide();
            self.shown.set(None);
            return;
        };
        self.text.set_text(&notice.text);
        self.root.update_property(&[gtk::accessible::Property::Label(&notice.name)]);
        self.rebuild_actions(workspace, &notice.actions);
        self.place(placement);
        if self.shown.replace(Some(notice.id)) == Some(notice.id) {
            return;
        }
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

    /// Lay the actions beside the text or on their own row for `placement`.
    pub fn place(&self, placement: NoticePlacement) {
        let natural = |widget: &gtk::Widget| widget.measure(gtk::Orientation::Horizontal, -1).1 as f32;
        let content = placement.max_width;
        let actions = if self.actions.get_visible() { natural(self.actions.upcast_ref()) } else { 0. };
        let beside = placement.actions_beside(natural(self.text.upcast_ref()), content, actions, GAP as f32);
        let orientation = if beside { gtk::Orientation::Horizontal } else { gtk::Orientation::Vertical };
        if self.root.orientation() != orientation { self.root.set_orientation(orientation); }
        if beside { self.root.remove_css_class("stacked"); } else { self.root.add_css_class("stacked"); }
        if self.placement.replace(placement) != placement { self.root.queue_resize(); }
    }

    /// The bubble's allocation in the overlay: centred on the placement's
    /// anchor, no wider than its maximum width.
    pub fn allocation(&self) -> gdk::Rectangle {
        let placement = self.placement.get();
        let width = self.root.measure(gtk::Orientation::Horizontal, -1).1.min(placement.max_width.floor() as i32).max(1);
        let height = self.root.measure(gtk::Orientation::Vertical, width).1;
        gdk::Rectangle::new((placement.x - width as f32 * 0.5).round() as i32, (placement.y - height as f32).round() as i32, width, height)
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
