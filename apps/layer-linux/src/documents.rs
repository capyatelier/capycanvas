//! One window, one live canvas, retained document sessions with reclaimable tiles.
use crate::{canvas::GpuCanvas, recovery::Recovery, workspace::Workspace};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_core::{Project, raster_storage::RetainedTiles};
use layer_ui::{DocumentLocation, DocumentTabs, DocumentSessions, DocumentTabLabel as Label};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::Rc,
    time::{Duration, Instant},
};

type Prepared = (
    Project,
    Option<DocumentLocation>,
    Option<std::path::PathBuf>,
);
struct Queued {
    prepared: Prepared,
    // Claim synchronously while the recovery offer still holds its lease.
    _recovery_lease: Option<Rc<Recovery>>,
}
struct TabDrag {
    id: u64,
    origin: [f32; 2],
    sequence: Option<gdk::EventSequence>,
    device: Option<gdk::Device>,
    button: gtk::Widget,
    width: i32,
}
pub(crate) struct Parked {
    canvas: GpuCanvas,
    recovery: Rc<Recovery>,
}
pub(crate) struct Documents {
    pub root: gtk::Stack,
    strip: gtk::Box,
    title: gtk::Label,
    selector: gtk::MenuButton,
    selector_label: gtk::Label,
    pub model: RefCell<DocumentSessions<Parked>>,
    labels: RefCell<Vec<Label>>,
    pending: RefCell<VecDeque<Queued>>,
    draining: Cell<bool>,
    selecting: Cell<bool>,
    pub changing: Cell<bool>,
    pub loading: Cell<bool>,
    pub cancel_open: Cell<bool>,
    pub close_pending: Cell<bool>,
    pub paused: Cell<bool>,
    pub closing_window: Cell<bool>,
    pub closing_tab: Cell<bool>,
    dragging: Cell<bool>,
    drag: RefCell<Option<TabDrag>>,
    #[cfg(test)]
    pub ram_budget: Cell<usize>,
}
impl Documents {
    fn reset_button(button: &gtk::Widget) {
        // The captured controller consumes motion/release. Retire ancestor
        // context holds too, so they cannot open a menu during a touch drag.
        let mut current = Some(button.clone());
        while let Some(widget) = current {
            let controllers = widget.observe_controllers();
            for i in 0..controllers.n_items() {
                if let Some(gesture) = controllers.item(i).and_downcast::<gtk::Gesture>() {
                    gesture.reset();
                }
            }
            current = widget.parent();
        }
    }
    fn cancel_drag(&self) {
        if let Some(drag) = self.drag.borrow_mut().take() {
            Self::reset_button(&drag.button);
        }
        self.dragging.set(false);
        self.drop_at(None);
    }
    // Return an insertion point only over a visible tab. The root controller
    // keeps the native implicit contact grab stable while GTK buttons arbitrate
    // clicks. Like the existing workspace tabs, every device uses native slop.
    fn drop_at(&self, point: Option<[f32; 2]>) -> Option<Option<u64>> {
        let mut result = None;
        let mut child = self.strip.first_child();
        for &id in self.model.borrow().order() {
            let Some(row) = child else {
                break;
            };
            child = row.next_sibling();
            row.remove_css_class("drop-before");
            row.remove_css_class("drop-after");
            if let Some(p) = point
                && let Some(b) = row.compute_bounds(&self.root)
                && b.contains_point(&gtk::graphene::Point::new(p[0], p[1]))
            {
                result = self.model.borrow().drop_target(&[layer_ui::DocumentTabHit {
                    id,
                    bounds: layer_ui::Bounds { x: b.x(), y: b.y(), width: b.width(), height: b.height() },
                }], p, false);
                row.add_css_class(if result == Some(Some(id)) { "drop-before" } else { "drop-after" });
            }
        }
        result
    }
    fn bind_input(&self, w: &Rc<Workspace>) {
        let input = gtk::EventControllerLegacy::new();
        input.set_propagation_phase(gtk::PropagationPhase::Capture);
        input.connect_event(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                use gdk::EventType::*;
                let kind = event.event_type();
                if !matches!(
                    kind,
                    ButtonPress
                        | ButtonRelease
                        | MotionNotify
                        | TouchBegin
                        | TouchUpdate
                        | TouchEnd
                        | TouchCancel
                ) {
                    return glib::Propagation::Proceed;
                }
                if matches!(kind, ButtonPress | ButtonRelease)
                    && event
                        .downcast_ref::<gdk::ButtonEvent>()
                        .is_none_or(|e| e.button() != 1)
                {
                    return glib::Propagation::Proceed;
                }
                let docs = &w.documents;
                let touch = matches!(kind, TouchBegin | TouchUpdate | TouchEnd | TouchCancel);
                let sequence = touch.then(|| event.event_sequence());
                let point = event
                    .position()
                    .and_then(|(x, y)| crate::input::widget_point(&docs.root, x, y))
                    .map(|p| [p.x(), p.y()]);
                if matches!(kind, ButtonPress | TouchBegin) {
                    if docs.drag.borrow().is_some()
                        || docs.changing.get()
                        || docs.loading.get()
                        || w.servicing.get()
                        || w.window.visible_dialog().is_some()
                        || event.surface() != w.window.surface()
                        || !docs.root.is_sensitive()
                        || !docs.root.is_mapped()
                        || docs.root.visible_child_name().as_deref() != Some("tabs")
                    {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(p) = point {
                        let mut child = docs.strip.first_child();
                        for &id in docs.model.borrow().order() {
                            let Some(row) = child else {
                                break;
                            };
                            child = row.next_sibling();
                            let Some(button) = row.first_child() else {
                                continue;
                            };
                            if button.compute_bounds(&docs.root).is_some_and(|b| {
                                b.contains_point(&gtk::graphene::Point::new(p[0], p[1]))
                            }) {
                                *docs.drag.borrow_mut() = Some(TabDrag {
                                    id,
                                    origin: p,
                                    sequence,
                                    device: event.device(),
                                    button,
                                    width: docs.root.width(),
                                });
                                break;
                            }
                        }
                    }
                    return glib::Propagation::Proceed;
                }
                let held = docs.drag.borrow();
                let Some(drag) = held
                    .as_ref()
                    .filter(|d| d.sequence == sequence && d.device == event.device())
                else {
                    return glib::Propagation::Proceed;
                };
                let id = drag.id;
                let cancelled = kind == TouchCancel
                    || drag.width != docs.root.width()
                    || !drag.button.is_mapped()
                    || !docs.root.is_sensitive()
                    || docs.root.visible_child_name().as_deref() != Some("tabs");
                if !cancelled
                    && let Some(p) = point
                    && !docs.dragging.get()
                    && docs.root.drag_check_threshold(
                        drag.origin[0] as i32,
                        drag.origin[1] as i32,
                        p[0] as i32,
                        p[1] as i32,
                    )
                {
                    docs.dragging.set(true);
                    Self::reset_button(&drag.button);
                }
                let started = docs.dragging.get();
                // Release the RefCell guard before cancellation takes ownership.
                drop(held);
                if cancelled || matches!(kind, ButtonRelease | TouchEnd) {
                    let before = if started && !cancelled {
                        docs.drop_at(point)
                    } else {
                        None
                    };
                    if started || cancelled {
                        docs.cancel_drag();
                    } else {
                        docs.drag.borrow_mut().take();
                    }
                    if let Some(before) = before {
                        docs.model.borrow_mut().reorder(id, before);
                    }
                    if started {
                        docs.refresh(&w);
                    }
                } else if started {
                    docs.drop_at(point);
                }
                if started {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        w.window.add_controller(input);
        w.window.connect_is_active_notify(glib::clone!(
            #[weak]
            w,
            move |window| {
                if !window.is_active() {
                    w.documents.cancel_drag();
                }
            }
        ));
        self.root.connect_unmap(glib::clone!(
            #[weak]
            w,
            move |_| w.documents.cancel_drag()
        ));
    }
    pub fn new() -> Self {
        let root = gtk::Stack::new();
        root.set_widget_name("document-tabs");
        root.set_hhomogeneous(false);
        root.set_vhomogeneous(false);
        root.set_hexpand(true);
        let title = gtk::Label::new(Some(layer_ui::APP_NAME));
        title.set_widget_name("single-document-title");
        title.add_css_class("document-title");
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_width_chars(1);
        let title_handle = gtk::WindowHandle::new();
        title_handle.add_css_class("header-readout");
        title_handle.set_child(Some(&title));
        root.add_named(&title_handle, Some("title"));
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.set_homogeneous(true);
        strip.add_css_class("document-tabs");
        let selector = gtk::MenuButton::new();
        selector.set_widget_name("document-selector");
        selector.set_hexpand(true);
        selector.add_css_class("flat");
        selector.set_tooltip_text(Some("Switch drawing · Ctrl+Shift+A"));
        let selector_label = gtk::Label::new(None);
        selector_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        selector_label.set_width_chars(1);
        selector_label.set_hexpand(true);
        let selector_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        selector_content.append(&selector_label);
        selector_content.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
        selector.set_child(Some(&selector_content));
        root.add_named(&strip, Some("tabs"));
        root.add_named(&selector, Some("selector"));
        Self {
            root,
            strip,
            title,
            selector,
            selector_label,
            model: RefCell::new(DocumentSessions::default()),
            labels: Default::default(),
            pending: Default::default(),
            draining: Cell::new(false),
            selecting: Cell::new(false),
            changing: Cell::new(false),
            loading: Cell::new(false),
            cancel_open: Cell::new(false),
            close_pending: Cell::new(false),
            paused: Cell::new(false),
            closing_window: Cell::new(false),
            closing_tab: Cell::new(false),
            dragging: Cell::new(false),
            drag: Default::default(),
            #[cfg(test)]
            ram_budget: Cell::new(layer_ui::DocumentBudget::default().inactive_ram),
        }
    }
    pub fn len(&self) -> usize {
        self.model.borrow().order().len()
    }
    pub fn selected(&self) -> u64 {
        self.model.borrow().selected()
    }
    pub fn has_pending_open(&self) -> bool {
        self.draining.get() || !self.pending.borrow().is_empty()
    }
    pub fn bind(&self, w: &Rc<Workspace>) {
        self.bind_input(w);
        self.selector.set_create_popup_func(glib::clone!(
            #[weak]
            w,
            move |button| {
                button.set_popover(Some(&w.documents.popup(&w)));
            }
        ));
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        target.connect_drop(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                let Ok(files) = value.get::<gdk::FileList>() else {
                    return false;
                };
                crate::files::launch::open_in(&w, files.files());
                true
            }
        ));
        self.root.add_controller(target);
    }
    pub fn allocate(&self, width: f32) {
        self.root.set_visible_child_name(if self.len() == 1 {
            "title"
        } else if DocumentTabs::compact(width, self.len()) {
            "selector"
        } else {
            "tabs"
        });
    }
    fn label(id: u64, canvas: &GpuCanvas) -> Label {
        Label::new(id, &canvas.session.state().document_file)
    }
    pub fn refresh(&self, w: &Rc<Workspace>) {
        let Some(current) = w.gpu.borrow().as_ref().map(|g| {
            let state = g.session.state();
            if let Some(tab) = state.tabs.first() {
                self.title.set_label(&format!(
                    "{}{} · {} × {}",
                    if state.document_file.modified {
                        "• "
                    } else {
                        ""
                    },
                    tab.title,
                    tab.width,
                    tab.height
                ));
            }
            Self::label(self.selected(), g)
        }) else {
            return;
        };
        let mut labels = BTreeMap::from([(current.id, current.clone())]);
        for (&id, tab) in self.model.borrow().parked() {
            labels.insert(id, Self::label(id, &tab.owner.canvas));
        }
        let labels: Vec<_> = self
            .model
            .borrow()
            .order()
            .iter()
            .filter_map(|id| labels.remove(id))
            .collect();
        self.selector_label.set_label(&format!(
            "{}{}",
            if current.modified { "• " } else { "" },
            current.title
        ));
        if *self.labels.borrow() == labels && self.strip.first_child().is_some() {
            // Selection can change while two drawings have identical names.
            self.mark_selected();
            return;
        }
        if self.dragging.get() {
            return;
        }
        *self.labels.borrow_mut() = labels.clone();
        while let Some(child) = self.strip.first_child() {
            self.strip.remove(&child);
        }
        for label in labels {
            let id = label.id;
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.set_widget_name(&format!("document-tab-{id}"));
            row.add_css_class("document-tab");
            row.set_hexpand(true);
            let title = gtk::Label::new(Some(&format!(
                "{}{}",
                if label.modified { "• " } else { "" },
                label.title
            )));
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.set_width_chars(1);
            // Balance the close control so the title is centered in the tab.
            title.set_margin_start(24);
            let select = gtk::Button::builder().child(&title).hexpand(true).build();
            select.add_css_class("flat");
            select.add_css_class("document-tab-select");
            select.set_focus_on_click(false);
            select.connect_state_flags_changed(glib::clone!(
                #[weak]
                row,
                move |button, _| {
                    // The fill belongs to the whole tab, including the area
                    // behind its separate close button.
                    if button.state_flags().contains(gtk::StateFlags::ACTIVE) {
                        row.set_state_flags(gtk::StateFlags::ACTIVE, false);
                    } else {
                        row.unset_state_flags(gtk::StateFlags::ACTIVE);
                    }
                }
            ));
            select.set_tooltip_text(Some(&format!("{}\n{}", label.title, label.location)));
            select.update_property(&[gtk::accessible::Property::Label(&format!(
                "Switch to {}",
                label.title
            ))]);
            select.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| {
                    if !w.documents.dragging.get() {
                        w.documents.select(&w, id, false);
                    }
                }
            ));
            row.append(&select);
            let close = gtk::Button::from_icon_name("window-close-symbolic");
            close.add_css_class("flat");
            close.add_css_class("document-tab-close");
            close.set_valign(gtk::Align::Center);
            // Like AdwTabBar, one keyboard stop per tab; Ctrl+W closes it.
            close.set_can_focus(false);
            close.set_tooltip_text(Some(&format!("Close {}", label.title)));
            close.connect_clicked(glib::clone!(
                #[weak]
                w,
                move |_| w.documents.select(&w, id, true)
            ));
            row.append(&close);
            self.strip.append(&row);
        }
        self.mark_selected();
        self.root.queue_resize();
    }
    fn mark_selected(&self) {
        let selected = format!("document-tab-{}", self.selected());
        let mut child = self.strip.first_child();
        while let Some(row) = child {
            child = row.next_sibling();
            if row.widget_name() == selected {
                row.add_css_class("selected");
            } else {
                row.remove_css_class("selected");
            }
        }
    }
    pub fn popup(&self, w: &Rc<Workspace>) -> gtk::Popover {
        let popover = gtk::Popover::new();
        popover.set_widget_name("drawing-selector-popup");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for label in self.labels.borrow().iter() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            let title = gtk::Label::new(Some(&format!(
                "{}{}{}\n{}",
                if label.id == self.selected() {
                    "✓ "
                } else {
                    ""
                },
                if label.modified { "• " } else { "" },
                label.title,
                label.location
            )));
            title.set_xalign(0.);
            title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            title.set_max_width_chars(48);
            let button = gtk::Button::builder().child(&title).hexpand(true).build();
            button.add_css_class("flat");
            let id = label.id;
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    w.documents.select(&w, id, false);
                }
            ));
            row.append(&button);
            let close = gtk::Button::from_icon_name("window-close-symbolic");
            close.set_tooltip_text(Some(&format!("Close {}", label.title)));
            close.add_css_class("flat");
            close.connect_clicked(glib::clone!(
                #[weak]
                w,
                #[weak]
                popover,
                move |_| {
                    popover.popdown();
                    w.documents.select(&w, id, true);
                }
            ));
            row.append(&close);
            list.append(&row);
        }
        for redo in [false, true] {
            let button = gtk::Button::with_label(if redo {
                "Redo Tab Reorder"
            } else {
                "Undo Tab Reorder"
            });
            button.set_sensitive(if redo {
                self.model.borrow().can_redo()
            } else {
                self.model.borrow().can_undo()
            });
            button.connect_clicked(glib::clone!(
                #[weak]
                w,
                #[weak]
                popover,
                move |_| {
                    if redo {
                        w.documents.model.borrow_mut().redo();
                    } else {
                        w.documents.model.borrow_mut().undo();
                    }
                    popover.popdown();
                    w.documents.refresh(&w);
                }
            ));
            list.append(&button);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(400)
            .propagate_natural_height(true)
            .child(&list)
            .build();
        popover.set_child(Some(&scroll));
        w.watch_popover(&popover);
        popover
    }
    pub fn show_selector(&self, w: &Rc<Workspace>) {
        let popup = self.popup(w);
        popup.set_parent(&w.area);
        popup.set_position(gtk::PositionType::Bottom);
        popup.set_pointing_to(Some(&gdk::Rectangle::new(w.area.width() / 2, 0, 1, 1)));
        popup.connect_closed(|p| p.unparent());
        popup.popup();
    }
    pub fn key(&self, w: &Rc<Workspace>, key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
        if key == gdk::Key::Escape && self.drag.borrow().is_some() {
            self.cancel_drag();
            return true;
        }
        if !modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            || modifiers.intersects(gdk::ModifierType::ALT_MASK | gdk::ModifierType::SUPER_MASK)
        {
            return false;
        }
        let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
        match key {
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab | gdk::Key::Page_Down | gdk::Key::Page_Up => {
                let forward = !shift && key != gdk::Key::Page_Up && key != gdk::Key::ISO_Left_Tab;
                let id = self.model.borrow().adjacent(forward);
                if let Some(id) = id {
                    self.select(w, id, false);
                }
            }
            gdk::Key::a | gdk::Key::A if shift => self.show_selector(w),
            gdk::Key::w | gdk::Key::W if shift => w.window.close(),
            _ => return false,
        }
        true
    }
    pub fn enqueue(&self, w: &Rc<Workspace>, prepared: Prepared) {
        // File-launch producer awaits each admission; native file requests can
        // have at most one prepared successor. Never accumulate decoded images.
        if !self.pending.borrow().is_empty() || self.closing_window.get() {
            w.changed(Err(
                "Finish opening or closing the current drawing first".into()
            ));
            return;
        }
        let lease = if let Some(origin) = &prepared.2 {
            let lease = Rc::new(Recovery::default());
            if let Err(error) = lease.set_origin(Some(origin.clone())) {
                w.changed(Err(error));
                return;
            }
            Some(lease)
        } else {
            None
        };
        self.pending.borrow_mut().push_back(Queued {
            prepared,
            _recovery_lease: lease,
        });
        self.drain(w);
    }
    pub fn drain(&self, w: &Rc<Workspace>) {
        if self.draining.get()
            || self.pending.borrow().is_empty()
            || w.servicing.get()
            || self.changing.get()
        {
            return;
        }
        self.draining.set(true);
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let prepared = w.documents.pending.borrow_mut().pop_front();
                if let Some(queued) = prepared {
                    let _lease = queued._recovery_lease;
                    if let Err(error) = w.documents.open(&w, queued.prepared).await {
                        w.changed(Err(error));
                    }
                }
                w.documents.draining.set(false);
            }
        ));
    }
    pub fn select(&self, w: &Rc<Workspace>, id: u64, close: bool) {
        if self.changing.get()
            || self.loading.get()
            || w.servicing.get()
            || w.window.visible_dialog().is_some()
            || self.closing_tab.get()
        {
            return;
        }
        if self.selecting.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                let result = w.documents.activate(&w, id).await;
                w.documents.selecting.set(false);
                if let Err(error) = result {
                    w.changed(Err(error));
                    return;
                }
                if close {
                    w.documents.closing_tab.set(true);
                    let result = w
                        .gpu
                        .borrow_mut()
                        .as_mut()
                        .map(|g| g.session.request_document_close());
                    if let Some(result) = result {
                        w.changed(result);
                    }
                }
            }
        ));
    }
    async fn prepare_switch(&self, w: &Rc<Workspace>) -> Result<(), String> {
        if w.gpu.borrow().is_none() {
            return Err("Wait for the drawing canvas to finish opening".into());
        }
        if self.changing.replace(true) {
            return Err("Drawing switch already in progress".into());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        // Alert responses precede their closing animation. A completed New/Open
        // or Save/Discard can still have a visible native sheet for a few frames.
        while w.servicing.get() || w.window.visible_dialog().is_some() {
            if Instant::now() >= deadline || !w.window.is_visible() {
                self.changing.set(false);
                return Err("Finish the current dialog before switching drawings".into());
            }
            glib::timeout_future(Duration::from_millis(8)).await;
        }
        while w
            .gpu
            .borrow()
            .as_ref()
            .is_some_and(|g| !g.session.can_park_document())
        {
            if Instant::now() >= deadline || !w.window.is_visible() {
                self.changing.set(false);
                return Err("Finish the current canvas operation before switching drawings".into());
            }
            w.wake();
            glib::timeout_future(Duration::from_millis(8)).await;
        }
        w.proof.pause().await;
        w.local_tone.pause().await;
        let histogram = w.histogram.borrow_mut().take();
        if let Some(histogram) = histogram {
            histogram.retire().await;
        }
        let capture = w.gpu.borrow().as_ref().and_then(|g| {
            (!g.session.rendering_suspended() && !g.session.state().document_file.close_ready)
                .then(|| g.session.retained_document_tiles())
        });
        if let Some(tiles) = capture {
            // A submitted frame may still be mapping exact tile backing. Wait
            // for every history root too: an immediate Undo can move the newest
            // pending capture out of the current document and into redo.
            let result = gio::spawn_blocking(move || tiles.blobs().map(|_| ()))
                .await
                .map_err(|_| "Drawing capture worker stopped".to_string())
                .and_then(|r| r);
            if let Err(error) = result {
                self.changing.set(false);
                w.proof.resume(w);
                w.local_tone.resume();
                return Err(error);
            }
        }
        let recovery = w.recovery();
        recovery.capture(w);
        recovery.drain().await;
        if !w.window.is_visible() {
            self.changing.set(false);
            return Err("The drawing window was closed".into());
        }
        self.paused.set(true);
        let parked = w.gpu.borrow_mut().as_mut().map(|g| {
            g.session.park_document()?;
            g.session.renderer_mut().stop();
            Ok::<_, String>(())
        }).unwrap_or(Ok(()));
        if let Err(error) = parked {
            self.paused.set(false);
            self.changing.set(false);
            w.proof.resume(w);
            w.local_tone.resume();
            return Err(error);
        }
        Ok(())
    }
    fn park(&self, w: &Workspace) -> Option<GpuCanvas> {
        w.gpu.borrow_mut().take()
    }
    fn finish_switch(&self, w: &Rc<Workspace>, error: Option<String>) {
        self.paused.set(false);
        self.changing.set(false);
        w.layer_panel.document_changed();
        w.refresh_document_view();
        w.proof.resume(w);
        w.local_tone.resume();
        let storage_error = self.model.borrow().storage_error().map(str::to_owned);
        if let Some(error) = error {
            w.document_canvas_error(&error);
        } else if let Some(error) = storage_error {
            w.changed(Err(format!(
                "{error}\nThe drawing is retained in memory. Free disk space or close some tabs."
            )));
        }
        self.refresh(w);
        w.area.grab_focus();
        w.wake();
        if self.close_pending.replace(false) {
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                w,
                move || w.window.close()
            ));
        }
    }
    pub async fn activate(&self, w: &Rc<Workspace>, id: u64) -> Result<(), String> {
        if self.selected() == id {
            return Ok(());
        }
        if !self.model.borrow().contains_parked(id) {
            return Err("Drawing tab is no longer open".into());
        }
        self.prepare_switch(w).await?;
        let previous = self.park(w).ok_or("Canvas unavailable")?;
        let tiles = previous.session.retained_document_tiles();
        let error = {
            let mut model = self.model.borrow_mut();
            let next = model.parked_owner_mut(id).unwrap();
            next.canvas.session.inherit_window_state(&previous.session).err()
                .or_else(|| next.canvas.reattach(&w.area).err())
        };
        let next = self.model.borrow_mut().exchange(id, Parked { canvas: previous, recovery: w.recovery() }, tiles)
            .unwrap_or_else(|_| unreachable!("validated drawing target"));
        *w.gpu.borrow_mut() = Some(next.canvas);
        *w.recovery.borrow_mut() = next.recovery;
        self.trim().await;
        self.finish_switch(w, error);
        Ok(())
    }
    pub async fn open(
        &self,
        w: &Rc<Workspace>,
        (project, location, origin): Prepared,
    ) -> Result<(), String> {
        if !w.window.is_visible() || self.closing_window.get() {
            return Err("The drawing window was closed".into());
        }
        let retry_storage = self.model.borrow().storage_error().is_some();
        if retry_storage {
            self.trim().await;
        }
        let active = w.gpu.borrow().as_ref().map(|g| g.session.retained_document_tiles()).unwrap_or_default();
        self.model.borrow().admit(&active, &project)?;
        self.prepare_switch(w).await?;
        let mut previous = self.park(w).ok_or("Canvas unavailable")?;
        if self.closing_window.get() || self.cancel_open.get() {
            let error = previous.reattach(&w.area).err();
            *w.gpu.borrow_mut() = Some(previous);
            self.finish_switch(w, error);
            return Err("Opening was cancelled because the window is closing".into());
        }
        let candidate = GpuCanvas::with_project(&w.area, Some((project, location)));
        let mut next = match candidate {
            Ok(next) => next,
            Err(error) => {
                let restart_error = previous.reattach(&w.area).err();
                *w.gpu.borrow_mut() = Some(previous);
                self.finish_switch(w, restart_error);
                return Err(error);
            }
        };
        let error = next.session.inherit_window_state(&previous.session).err();
        let recovery = Rc::new(Recovery::default());
        recovery.recovered.set(origin.is_some());
        if recovery.recovered.get() {
            next.session.mark_recovered();
        }
        if let Err(e) = recovery.set_origin(origin) {
            eprintln!("Recovery ownership: {e}");
        }
        let tiles = previous.session.retained_document_tiles();
        self.model.borrow_mut().append(Parked { canvas: previous, recovery: w.recovery() }, tiles);
        *w.gpu.borrow_mut() = Some(next);
        *w.recovery.borrow_mut() = recovery;
        self.trim().await;
        self.finish_switch(w, error);
        Ok(())
    }
    async fn trim(&self) {
        #[cfg(test)]
        { self.model.borrow_mut().budget.inactive_ram = self.ram_budget.get(); }
        self.model.borrow_mut().storage_completed(Ok(()));
        loop {
            let candidate = self.model.borrow().spill_candidate();
            let Some(tiles) = candidate else {
                break;
            };
            let result = gio::spawn_blocking(move || spill(tiles))
                .await
                .map_err(|_| "Drawing storage worker stopped".to_string())
                .and_then(|r| r);
            if let Err(error) = result {
                self.model.borrow_mut().storage_completed(Err(error));
                break;
            }
        }
    }
    /// A completed close always belongs to the active session; service_requests
    /// calls this only after its Save/Discard/Cancel state machine has settled.
    pub fn close_completed(&self, w: &Rc<Workspace>) {
        let ready = w
            .gpu
            .borrow()
            .as_ref()
            .is_some_and(|g| g.session.state().document_file.close_ready);
        if !ready {
            self.closing_tab.set(false);
            self.closing_window.set(false);
            if self.close_pending.replace(false) {
                self.pending.borrow_mut().clear();
                self.cancel_open.set(false);
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    w,
                    move || w.window.close()
                ));
                return;
            }
            if !self.loading.get() {
                self.cancel_open.set(false);
            }
            self.drain(w);
            return;
        }
        if self.len() == 1 {
            w.window.close();
            return;
        }
        if self.changing.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            w,
            async move {
                w.documents.changing.set(false);
                // close_ready still allows a normal parked boundary.
                if let Err(error) = w.documents.prepare_switch(&w).await {
                    w.documents.closing_window.set(false);
                    w.documents.closing_tab.set(false);
                    w.changed(Err(error));
                    return;
                }
                let mut next = w.documents.model.borrow_mut().close_selected().unwrap();
                let previous = w.gpu.borrow_mut().take().unwrap();
                let recovery = w.recovery();
                recovery.discard();
                let error = next
                    .canvas
                    .session
                    .inherit_window_state(&previous.session)
                    .err()
                    .or_else(|| next.canvas.reattach(&w.area).err());
                *w.gpu.borrow_mut() = Some(next.canvas);
                *w.recovery.borrow_mut() = next.recovery;
                drop(previous);
                w.documents.closing_tab.set(false);
                w.documents.finish_switch(&w, error);
                if w.documents.closing_window.get() {
                    w.window.close();
                }
            }
        ));
    }
    #[cfg(test)]
    pub fn drag_active(&self) -> bool {
        self.dragging.get()
    }
    #[cfg(test)]
    pub fn parked_memory(&self) -> (usize, bool) {
        let p = self.model.borrow();
        (
            p.resident_bytes(),
            p.parked().all(|(_, p)| p.owner.canvas.session.engine().backend().worker_is_joined()),
        )
    }
}

fn spill(tiles: RetainedTiles) -> Result<(), String> {
    let directory = std::env::var_os("CAPY_TAB_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| glib::user_cache_dir().join("capycanvas/tabs"));
    layer_core::raster_storage::spill_to_directory(&tiles, &directory)
}
