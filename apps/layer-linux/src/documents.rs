//! One window, one live canvas, retained document sessions with reclaimable tiles.
use crate::{
    canvas::GpuCanvas,
    recovery::Recovery,
    workspace::{NativeTabSlide, SlidingTab, Workspace},
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use layer_core::{Document, raster_storage::RetainedTiles};
use layer_ui::{DocumentLocation, DocumentTabs, DocumentSessions, DocumentTabLabel as Label};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

type Prepared = (
    Document,
    Option<DocumentLocation>,
);
struct Queued {
    prepared: Prepared,
    imported: Option<layer_ui::ImportedDocument>,
}
struct TabDrag {
    id: u64,
    origin: [f32; 2],
    sequence: Option<gdk::EventSequence>,
    device: Option<gdk::Device>,
    button: gtk::Widget,
    width: i32,
    slide: Option<TabSlide>,
}
struct TabSlide {
    policy: layer_ui::DocumentTabDrag,
    view: NativeTabSlide,
    surface: gtk::Widget,
}
pub(crate) struct Parked {
    pub(crate) canvas: GpuCanvas,
    pub(crate) recovery: Rc<Recovery>,
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
    pub exit_ready: Cell<bool>,
    pub exit_flushing: Cell<bool>,
    dragging: Cell<bool>,
    drag: RefCell<Option<TabDrag>>,
    close_inset: Cell<i32>,
    #[cfg(test)]
    pub ram_budget: Cell<usize>,
}
const TAB_CLOSE_SIZE: i32 = 24;
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
            if let Some(slide) = drag.slide {
                slide.view.restore();
                slide.surface.queue_draw();
            }
        }
        self.dragging.set(false);
    }
    fn begin_slide(&self, w: &Workspace) {
        let mut held = self.drag.borrow_mut();
        let Some(drag) = held.as_mut() else {
            return;
        };
        let surface: &gtk::Widget = w.surface.upcast_ref();
        let palette = w.gpu.borrow().as_ref().map(|g| g.session.state().palette);
        let Some(clip) = self.strip.compute_bounds(surface) else {
            return;
        };
        let order = self.model.borrow().order().to_vec();
        let mut tabs = Vec::new();
        let mut hits = Vec::new();
        let mut child = self.strip.first_child();
        for &id in &order {
            let Some(row) = child else {
                return;
            };
            child = row.next_sibling();
            let backing = palette.filter(|_| id == drag.id).map(|p| {
                let [r, g, b] = if row.has_css_class("selected") {
                    p.panel
                } else {
                    p.tabbar
                }
                .0;
                gdk::RGBA::new(r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.)
            });
            let Some(tab) = SlidingTab::capture(row, surface, |snapshot, rect| {
                if let Some(color) = backing {
                    let shape = gtk::gsk::RoundedRect::from_rect(
                        *rect,
                        rect.height() / 2. * crate::squircle::CORNER_FIT,
                    );
                    snapshot.push_rounded_clip(&shape);
                    snapshot.append_color(&color, rect);
                    snapshot.pop();
                }
            }) else {
                return;
            };
            hits.push(layer_ui::DocumentTabHit {
                id,
                bounds: tab.bounds,
            });
            tabs.push(tab);
        }
        let clip = layer_ui::Bounds {
            x: clip.x(),
            y: clip.y(),
            width: clip.width(),
            height: clip.height(),
        };
        let Some(policy) = self.model.borrow().drag(drag.id, drag.origin, &hits, clip) else {
            return;
        };
        let Some(view) = order
            .iter()
            .position(|&id| id == drag.id)
            .and_then(|source| NativeTabSlide::new(tabs, source, clip, 0))
        else {
            return;
        };
        view.hide();
        surface.queue_draw();
        drag.slide = Some(TabSlide {
            policy,
            view,
            surface: surface.clone(),
        });
    }
    fn slide_to(&self, point: [f32; 2]) {
        if let Some(slide) = self
            .drag
            .borrow_mut()
            .as_mut()
            .and_then(|d| d.slide.as_mut())
            && let Some(preview) = slide.policy.preview(point)
        {
            slide
                .view
                .retarget(&slide.surface, preview.bounds, |index| {
                    preview.offsets[index]
                });
            slide.surface.queue_draw();
        }
    }
    fn drop_at(&self, point: [f32; 2]) -> Option<Option<u64>> {
        let held = self.drag.borrow();
        let policy = &held.as_ref()?.slide.as_ref()?.policy;
        if !policy.is_current(self.model.borrow().order()) {
            return None;
        }
        policy
            .preview(point)
            .filter(|preview| preview.attached)
            .map(|preview| preview.before)
    }
    pub fn snapshot_drag(&self, snapshot: &gtk::Snapshot, now: i64, scale: f32) {
        if let Some(slide) = self.drag.borrow().as_ref().and_then(|d| d.slide.as_ref()) {
            slide.view.snapshot(snapshot, now, scale);
        }
    }
    // The root controller keeps the native implicit contact grab stable while
    // GTK buttons arbitrate clicks. Like the existing workspace tabs, every
    // device uses native slop.
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
                    .and_then(|(x, y)| crate::input::widget_point(&w.surface, x, y))
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
                            if button.compute_bounds(&w.surface).is_some_and(|b| {
                                b.contains_point(&gtk::graphene::Point::new(p[0], p[1]))
                            }) {
                                *docs.drag.borrow_mut() = Some(TabDrag {
                                    id,
                                    origin: p,
                                    sequence,
                                    device: event.device(),
                                    button,
                                    width: docs.root.width(),
                                    slide: None,
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
                let pickup = !cancelled
                    && !docs.dragging.get()
                    && point.is_some_and(|p| {
                        docs.root.drag_check_threshold(
                            drag.origin[0] as i32,
                            drag.origin[1] as i32,
                            p[0] as i32,
                            p[1] as i32,
                        )
                    });
                if pickup {
                    docs.dragging.set(true);
                    Self::reset_button(&drag.button);
                }
                let started = docs.dragging.get();
                // Release the RefCell guard before cancellation takes ownership.
                drop(held);
                if pickup {
                    docs.begin_slide(&w);
                }
                if cancelled || matches!(kind, ButtonRelease | TouchEnd) {
                    let before = point
                        .filter(|_| started && !cancelled)
                        .and_then(|p| docs.drop_at(p));
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
                } else if started && let Some(p) = point {
                    docs.slide_to(p);
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
    pub fn new_localized(localization: &layer_ui::Localizer) -> Self {
        let root = gtk::Stack::new();
        root.set_widget_name("document-tabs");
        root.set_hhomogeneous(false);
        root.set_vhomogeneous(false);
        root.set_hexpand(true);
        let title = gtk::Label::new(Some(localization.language().app_name()));
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
            model: RefCell::new(DocumentSessions::localized(localization)),
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
            exit_ready: Cell::new(false),
            exit_flushing: Cell::new(false),
            dragging: Cell::new(false),
            drag: Default::default(),
            close_inset: Cell::new(0),
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
    /// Close controls keep equal side and vertical insets, concentric with the tab.
    pub fn set_header_size(&self, size: layer_ui::HeaderSize) {
        self.strip.set_spacing(size.gap() as i32);
        let inset = (size.tile() as i32 - TAB_CLOSE_SIZE) / 2;
        if self.close_inset.replace(inset) != inset {
            let mut row = self.strip.first_child();
            while let Some(tab) = row {
                row = tab.next_sibling();
                Self::inset_close(&tab, inset);
            }
        }
    }
    fn inset_close(tab: &gtk::Widget, inset: i32) {
        if let Some(balance) = tab
            .first_child()
            .and_downcast::<gtk::Button>()
            .and_then(|select| select.child())
            .and_then(|content| content.first_child())
        {
            balance.set_size_request(TAB_CLOSE_SIZE + inset, -1);
        }
        if let Some(close) = tab.last_child() {
            close.set_margin_end(inset);
        }
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
    pub(crate) fn set_localization(&self, localization: &layer_ui::Localizer) {
        self.model.borrow_mut().set_localization(localization);
    }

    pub fn refresh(&self, w: &Rc<Workspace>) {
        let gpu = w.gpu.borrow();
        let Some(g) = gpu.as_ref() else { return; };
        let state = g.session.state();
        if let Some(tab) = state.tabs.first() {
            self.title.set_label(&format!(
                "{}{} · {} × {}",
                if state.document_file.modified { "• " } else { "" },
                tab.title,
                tab.width,
                tab.height
            ));
        }
        let labels = self.model.borrow().labels(
            &state.document_file,
            |parked| &parked.canvas.session.state().document_file,
            &w.localization(),
        );
        let Some(current) = labels.iter().find(|label| label.id == self.selected()).cloned() else { return; };
        drop(gpu);
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
            title.set_hexpand(true);
            // Balance the close control so the title is centered in the tab.
            let balance = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            content.append(&balance);
            content.append(&title);
            let select = gtk::Button::builder().child(&content).hexpand(true).build();
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
            Self::inset_close(row.upcast_ref(), self.close_inset.get());
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
        let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .max_content_height(400)
            .propagate_natural_height(true)
            .child(&list)
            .build());
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
        self.enqueue_prepared(w, prepared, None);
    }
    pub fn enqueue_imported(&self, w: &Rc<Workspace>, imported: layer_ui::ImportedDocument, location: Option<DocumentLocation>) {
        self.enqueue_prepared(w, (imported.project.clone(), location), Some(imported));
    }
    fn enqueue_prepared(&self, w: &Rc<Workspace>, prepared: Prepared, imported: Option<layer_ui::ImportedDocument>) {
        // File-launch producer awaits each admission; native file requests can
        // have at most one prepared successor. Never accumulate decoded images.
        if !self.pending.borrow().is_empty() || self.closing_window.get() {
            w.changed(Err(
                "Finish opening or closing the current drawing first".into()
            ));
            return;
        }
        self.pending.borrow_mut().push_back(Queued {
            prepared,
            imported,
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
                    if let Err(error) = w.documents.open_prepared(&w, queued.prepared, queued.imported, false, None, None).await {
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
    async fn prepare_switch(&self, w: &Rc<Workspace>, checkpoint:bool) -> Result<(), String> {
        if w.gpu.borrow().is_none() {
            return Err("Wait for the drawing canvas to finish opening".into());
        }
        if self.changing.replace(true) {
            return Err(layer_ui::DocumentTransportRefusal::ChangeInProgress.message(&w.localization()).to_string());
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        // Alert responses precede their closing animation. A completed New/Open
        // or Save/Discard can still have a visible native sheet for a few frames.
        while w.servicing.get() || w.window.visible_dialog().is_some() {
            if Instant::now() >= deadline || !w.window.is_visible() {
                self.changing.set(false);
                return Err(layer_ui::DocumentTransportRefusal::SwitchDialog.message(&w.localization()).to_string());
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
                return Err(layer_ui::DocumentTransportRefusal::SwitchOperation.message(&w.localization()).to_string());
            }
            w.wake();
            glib::timeout_future(Duration::from_millis(8)).await;
        }
        w.proof.pause().await;
        w.local_tone.pause().await;
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
        if checkpoint&&!w.gpu.borrow().as_ref().is_some_and(|gpu|gpu.session.state().document_file.close_ready) {recovery.capture(w);}
        recovery.drain().await;
        if !w.window.is_visible() {
            self.changing.set(false);
            return Err(layer_ui::bootstrap_view(&w.localization()).editor_closed.to_string());
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
            w.changed(Err(layer_ui::document_storage_retained(&w.localization(), &error)));
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
            return Err(layer_ui::DocumentSessionError::TabClosed.message(&w.localization()));
        }
        self.prepare_switch(w,true).await?;
        let previous = self.park(w).ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?;
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
    pub async fn open_imported(&self, w: &Rc<Workspace>, imported: layer_ui::ImportedDocument, location: Option<DocumentLocation>) -> Result<(), String> {
        self.open_prepared(w, (imported.project.clone(), location), Some(imported), false, None, None).await
    }
    pub async fn open_initial_imported(&self, w: &Rc<Workspace>, imported: layer_ui::ImportedDocument, location: Option<DocumentLocation>) -> Result<(), String> {
        let startup=w.gpu.borrow().as_ref().map(|gpu|gpu.session.session_stamp());
        self.open_prepared(w, (imported.project.clone(), location), Some(imported), true, None, startup).await
    }
    pub async fn open_restored(&self,w:&Rc<Workspace>,restored:layer_ui::session_recovery::SessionRestore,recovery:Rc<Recovery>,observed:Option<layer_ui::DestinationFingerprint>,startup:layer_ui::SessionStamp,id:u64)->Result<u64,String> {
        let project=restored.document().clone();
        self.open_prepared(w,(project,None),None,true,Some((restored,recovery,observed)),Some(startup)).await?;
        if !self.model.borrow().order().contains(&id)||self.selected()==id {self.model.borrow_mut().restore_identity(id)?;}
        Ok(self.selected())
    }
    pub async fn restore_inactive(&self,w:&Rc<Workspace>,restored:layer_ui::SessionRestore,recovery:Rc<Recovery>,observed:Option<layer_ui::DestinationFingerprint>,id:u64)->Result<(),String> {
        let project=restored.document().clone();
        let active=w.gpu.borrow().as_ref().map(|gpu|gpu.session.retained_document_tiles()).unwrap_or_default();
        self.model.borrow().admit(&active,&project).map_err(|reason|reason.message(&w.localization()))?;
        let settings=w.gpu.borrow().as_ref().map(|gpu|gpu.session.state().settings.clone());
        let mut candidate=GpuCanvas::with_project_localized(&w.area,Some((project,None)),w.localization(),settings)?;
        candidate.prepare_import(||!w.window.is_visible()||self.closing_window.get()).await?;
        if let Some(active)=w.gpu.borrow().as_ref() {candidate.session.inherit_window_state(&active.session)?;}
        candidate.session.renderer_mut().geometry=Some(crate::wayland::Geometry::of(&w.area));
        candidate.session.restore_session(restored,recovery.recovered.get(),observed)?;
        let deadline=Instant::now()+Duration::from_secs(120);
        while !candidate.session.can_park_document() {
            if !w.window.is_visible()||self.closing_window.get() {return Err("Opening cancelled".into());}
            if Instant::now()>=deadline {return Err("Project canvas preparation timed out".into());}
            let now=glib::monotonic_time() as u64*1000;
            candidate.session.frame(now,now)?;
            glib::timeout_future(Duration::from_millis(2)).await;
        }
        recovery.capture_session(&candidate.session);
        recovery.flush().await?;
        candidate.session.park_document()?;
        let tiles=candidate.session.retained_document_tiles();
        candidate.session.renderer_mut().stop();
        self.model.borrow_mut().append_parked_with_id(id,Parked {canvas:candidate,recovery},tiles,&w.localization()).map_err(|(error,_)|error)?;
        self.trim().await;
        self.refresh(w);
        Ok(())
    }
    async fn open_prepared(&self, w: &Rc<Workspace>, (project, location): Prepared, imported: Option<layer_ui::ImportedDocument>, replace_initial: bool, restored: Option<(layer_ui::session_recovery::SessionRestore,Rc<Recovery>,Option<layer_ui::DestinationFingerprint>)>, startup:Option<layer_ui::SessionStamp>) -> Result<(), String> {
        let original = location.as_ref().and_then(|location| gio::File::for_uri(&location.uri).path());
        let destination=if let (Some(imported),Some(location))=(imported.as_ref(),location.clone()) {
            let imported=imported.clone();
            gio::spawn_blocking(move || imported.destination_fingerprint(&std::sync::atomic::AtomicBool::new(false)).map(|fingerprint|fingerprint.map(|fingerprint|(location,fingerprint))))
                .await.map_err(|_|"Saved drawing verification stopped".to_string())??
        } else {None};
        if imported.is_some() || restored.is_some() {
            let deadline = Instant::now() + Duration::from_secs(120);
            while w.gpu.borrow().is_none() {
                if !w.window.is_visible() || self.closing_window.get() || self.cancel_open.get() { return Err("Opening cancelled".into()); }
                if Instant::now() >= deadline {
                    return self.show_unsupported(w, imported.as_ref(), original, "Project canvas preparation timed out".into()).await;
                }
                glib::timeout_future(Duration::from_millis(20)).await;
            }
        }
        if !w.window.is_visible() || self.closing_window.get() {
            return Err(layer_ui::bootstrap_view(&w.localization()).editor_closed.to_string());
        }
        let retry_storage = self.model.borrow().storage_error().is_some();
        if retry_storage {
            self.trim().await;
        }
        let active = w.gpu.borrow().as_ref().map(|g| g.session.retained_document_tiles()).unwrap_or_default();
        let admission = self.model.borrow().admit(&active, &project).map_err(|reason| reason.message(&w.localization()));
        if let Err(reason) = admission {
            return self.show_unsupported(w, imported.as_ref(), original, reason).await;
        }
        let mut replace_initial=replace_initial&&w.gpu.borrow().as_ref().is_some_and(|gpu|startup.as_ref().is_some_and(|stamp|gpu.session.can_replace_startup_session(stamp)));
        self.prepare_switch(w,!replace_initial).await?;
        let mut previous = self.park(w).ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?;
        if self.closing_window.get() || self.cancel_open.get() {
            let error = previous.reattach(&w.area).err();
            *w.gpu.borrow_mut() = Some(previous);
            self.finish_switch(w, error);
            return Err("Opening was cancelled because the window is closing".into());
        }
        let candidate = GpuCanvas::with_project_localized(&w.area, Some((project, location)), w.localization(), Some(previous.session.state().settings.clone()));
        let candidate = match candidate {
            Ok(mut next) if imported.is_some() || restored.is_some() => match next.prepare_import(|| !w.window.is_visible() || self.closing_window.get() || self.cancel_open.get()).await {
                Ok(()) => Ok(next), Err(error) => Err(error),
            },
            candidate => candidate,
        };
        let mut next = match candidate {
            Ok(next) => next,
            Err(error) => {
                let restart_error = previous.reattach(&w.area).err();
                *w.gpu.borrow_mut() = Some(previous);
                self.finish_switch(w, restart_error);
                return self.show_unsupported(w, imported.as_ref(), original, error).await;
            }
        };
        replace_initial&=startup.as_ref().is_some_and(|stamp|previous.session.can_replace_startup_session(stamp));
        let error = next.session.inherit_window_state(&previous.session).err()
            .or_else(|| next.session.inherit_initial_drawing_tools(&previous.session).err());
        let recovery = if let Some((restored,recovery,observed))=restored {
            if let Err(error)=next.session.restore_session(restored,recovery.recovered.get(),observed) {
                let restart_error=previous.reattach(&w.area).err();
                *w.gpu.borrow_mut()=Some(previous);
                self.finish_switch(w,restart_error);
                return Err(error);
            }
            recovery
        } else {
            let recovery=Rc::new(Recovery::default());
            recovery
        };
        if let Some((location,fingerprint))=destination {next.session.record_destination_fingerprint(&location,fingerprint)?;}
        if replace_initial {
            w.recovery().discard();
            drop(previous);
        } else {
            let tiles = previous.session.retained_document_tiles();
            self.model.borrow_mut().append(Parked { canvas: previous, recovery: w.recovery() }, tiles, &w.localization());
        }
        *w.gpu.borrow_mut() = Some(next);
        *w.recovery.borrow_mut() = recovery;
        self.trim().await;
        self.finish_switch(w, error);
        Ok(())
    }
    async fn show_unsupported(&self, w: &Rc<Workspace>, imported: Option<&layer_ui::ImportedDocument>, original: Option<std::path::PathBuf>, reason: String) -> Result<(), String> {
        if !w.window.is_visible() || self.closing_window.get() || self.cancel_open.get() { return Err(reason); }
        let Some(outcome) = imported.and_then(|imported| imported.preserve_unsupported(reason.clone())) else { return Err(reason); };
        crate::files::open::show_package(&w.window, layer_ui::PackageView::new(outcome)?, original, &w.localization()).await
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
        if self.closing_window.get() && !self.closing_tab.get() {
            if self.close_pending.replace(false) && !self.changing.get() {
                glib::idle_add_local_once(glib::clone!(#[weak] w,move ||w.window.close()));
            }
            return;
        }
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
            if self.exit_flushing.replace(true) {return;}
            glib::spawn_future_local(glib::clone!(#[weak] w, async move {
                let result=async {w.recovery().prepare_retirement().await?;w.restart.remove(&w,w.documents.selected()).await}.await;
                if let Err(error)=result {
                    w.documents.exit_flushing.set(false);w.documents.closing_tab.set(false);w.documents.closing_window.set(false);
                    if let Some(gpu)=w.gpu.borrow_mut().as_mut() {gpu.session.reset_document_close();}
                    w.changed(Err(error));return;
                }
                let recovery=w.recovery();recovery.discard();recovery.drain().await;
                w.documents.exit_flushing.set(false);w.documents.exit_ready.set(true);w.window.close();
            }));
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
                if let Err(error)=w.documents.prepare_switch(&w,true).await {
                    w.documents.closing_tab.set(false);w.documents.closing_window.set(false);
                    if let Some(gpu)=w.gpu.borrow_mut().as_mut() {gpu.session.reset_document_close();}
                    w.changed(Err(error));return;
                }
                let removal=async {w.recovery().prepare_retirement().await?;w.restart.remove(&w,w.documents.selected()).await}.await;
                if let Err(error)=removal {
                    let resume=w.gpu.borrow_mut().as_mut().and_then(|gpu| {gpu.session.reset_document_close();gpu.reattach(&w.area).err()});
                    w.documents.closing_tab.set(false);w.documents.closing_window.set(false);
                    w.documents.finish_switch(&w,resume);w.changed(Err(error));return;
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
    pub fn slide(&self) -> Option<(f32, Vec<f32>)> {
        let held = self.drag.borrow();
        let view = &held.as_ref()?.slide.as_ref()?.view;
        assert!(view.tabs.iter().all(|t| t.widget.opacity() == 0.));
        Some((view.bounds.x, view.tabs.iter().map(|t| t.to).collect()))
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

#[cfg(test)]
impl Documents {
    pub async fn open(
        &self,
        w: &Rc<Workspace>,
        prepared: Prepared,
    ) -> Result<(), String> {
        self.open_prepared(w, prepared, None, false, None, None).await
    }
}

fn spill(tiles: RetainedTiles) -> Result<(), String> {
    layer_core::raster_storage::spill_to_directory(&tiles, layer_core::temp_files::directory()?)
}
