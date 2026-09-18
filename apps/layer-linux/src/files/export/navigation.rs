//! Native, reversible navigation around one export draft.
use super::*;
use std::{cell::{Cell, RefCell}, task::{Poll, Waker}};

pub(super) fn page(nav: &adw::NavigationView, title: &str, tag: &str, body: &gtk::Box) -> adw::HeaderBar {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.set_margin_top(18);
    body.set_margin_bottom(18);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(body).build();
    scroll.set_widget_name(&format!("export-{tag}-scroll"));
    toolbar.set_content(Some(&scroll));
    let page = adw::NavigationPage::builder().title(title).tag(tag).child(&toolbar).build();
    nav.add(&page);
    header
}

pub(super) fn link(group: &adw::PreferencesGroup, nav: &adw::NavigationView, title: &str, tag: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).use_markup(false).activatable(true).build();
    row.set_widget_name(&format!("export-open-{tag}"));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let tag = tag.to_string();
    row.connect_activated(glib::clone!(#[weak] nav, move |_| nav.push_by_tag(&tag)));
    group.add(&row);
    row
}

// Cancellation of the file request must release the dialog and preview worker.
struct Guard(adw::Dialog, Option<glib::SignalHandlerId>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(handler) = self.1.take() { self.0.disconnect(handler); }
        self.0.force_close();
    }
}
pub(super) async fn choose(dialog: &adw::Dialog, parent: &adw::ApplicationWindow, response: &Cell<&'static str>) -> &'static str {
    let closed = Rc::new(Cell::new(false));
    let waker: Rc<RefCell<Option<Waker>>> = Rc::new(RefCell::new(None));
    let handler = dialog.connect_closed({
        let closed = closed.clone(); let waker = waker.clone();
        move |_| { closed.set(true); if let Some(waker) = waker.borrow_mut().take() { waker.wake(); } }
    });
    let _guard = Guard(dialog.clone(), Some(handler));
    dialog.present(Some(parent));
    std::future::poll_fn(|cx| {
        if closed.get() { Poll::Ready(()) } else { *waker.borrow_mut() = Some(cx.waker().clone()); Poll::Pending }
    }).await;
    response.get()
}
