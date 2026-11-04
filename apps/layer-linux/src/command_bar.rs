//! Native focus, IME and input capture around the shared command search model.
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, glib};
use layer_ui::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

type Contact = (Option<gdk::Device>, Option<gdk::EventSequence>, u32);

pub struct CommandBar {
    copy: RefCell<layer_ui::native_copy::SearchCopy>,
    close: gtk::Button,
    pub(crate) root: gtk::Overlay,
    card: gtk::Box,
    entry: gtk::SearchEntry,
    unit: gtk::Label,
    list: gtk::ListBox,
    viewport: gtk::Viewport,
    detail: gtk::Label,
    empty: gtk::Label,
    view: RefCell<Option<CommandSearchView>>,
    signature: RefCell<String>,
    updating: Cell<bool>,
    composition: Rc<crate::input::EntryComposition>,
    previous_focus: RefCell<Option<glib::WeakRef<gtk::Widget>>>,
    closing: Cell<Option<Closing>>,
    paint: RefCell<Option<(gdk::FrameClock, glib::SignalHandlerId)>>,
    contacts: RefCell<Vec<Contact>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Closing { Requested(Option<u64>), Selected(i64), Painted }
impl Closing {
    fn selected(self, requested: u64, shown: u64, frame: i64) -> Self {
        match self {
            Self::Requested(revision) => {
                let revision = revision.unwrap_or(requested);
                if shown >= revision { Self::Selected(frame) } else { Self::Requested(Some(revision)) }
            }
            _ => self,
        }
    }
    fn painted(self, frame: i64) -> Self {
        match self { Self::Selected(selected) if frame >= selected => Self::Painted, _ => self }
    }
}

impl CommandBar {
    pub(crate) fn composition_busy(&self) -> bool { self.composition.busy() }

    pub fn is_open(&self) -> bool {
        self.view.borrow().is_some()
    }
    pub fn new() -> Self {
        Self::with_copy(NativeCopy::new(crate::launch_localization()).search)
    }

    fn with_copy(copy: layer_ui::native_copy::SearchCopy) -> Self {
        let root = gtk::Overlay::new();
        root.set_widget_name("command-search-capture");
        root.set_focusable(true);
        root.set_visible(false);
        root.set_overflow(gtk::Overflow::Hidden);
        let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical)
            .spacing(COMMAND_SEARCH_STYLE.gap).accessible_role(gtk::AccessibleRole::Dialog).build();
        body.set_widget_name("command-bar");
        body.add_css_class("dialog-part-page");
        body.add_css_class("command-bar");
        body.set_halign(gtk::Align::Center);
        body.set_valign(gtk::Align::Start);
        body.update_property(&[gtk::accessible::Property::Label(copy.title.as_ref()), gtk::accessible::Property::Modal(CommandSearchView::DIALOG.modal)]);
        root.add_overlay(&body);
        let header = gtk::Box::new(gtk::Orientation::Horizontal, COMMAND_SEARCH_STYLE.gap);
        header.set_widget_name("dialog-header");
        let entry = gtk::SearchEntry::new();
        entry.set_widget_name("command-search");
        entry.set_placeholder_text(Some(copy.search_commands.as_ref()));
        entry.set_hexpand(true);
        entry.update_property(&[gtk::accessible::Property::Label(copy.search_commands.as_ref())]);
        let unit = gtk::Label::new(None);
        unit.add_css_class("dim-label");
        unit.set_visible(false);
        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.set_widget_name("command-search-close");
        close.add_css_class("dialog-part-end");
        close.set_tooltip_text(Some(copy.close_search.as_ref()));
        close.update_property(&[gtk::accessible::Property::Label(copy.close_search.as_ref())]);
        close.add_css_class("flat");
        header.append(&entry);
        header.append(&unit);
        header.append(&close);
        body.append(&header);
        let list = gtk::ListBox::new();
        list.set_widget_name("command-results");
        list.update_property(&[gtk::accessible::Property::Label(copy.commands.as_ref())]);
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        list.add_css_class("navigation-sidebar");
        let viewport = gtk::Viewport::builder().child(&list).build();
        let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic).propagate_natural_height(true).child(&viewport).build();
        scroll.set_vexpand(true);
        body.append(&scroll);
        let empty = gtk::Label::new(Some(copy.no_matches.as_ref()));
        empty.add_css_class("dim-label");
        empty.set_margin_top(12);
        empty.set_margin_bottom(12);
        body.append(&empty);
        let detail = gtk::Label::new(None);
        detail.set_widget_name("command-detail");
        detail.add_css_class("dim-label");
        detail.add_css_class("caption");
        detail.set_xalign(0.);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_margin_start(COMMAND_SEARCH_STYLE.inset);
        detail.set_margin_end(COMMAND_SEARCH_STYLE.inset);
        detail.set_height_request(20);
        body.append(&detail);
        let composition = crate::input::guard_editable_activation(&entry);
        Self {
            copy: RefCell::new(copy),
            close,
            root,
            card: body,
            entry,
            unit,
            list,
            viewport,
            detail,
            empty,
            view: Default::default(),
            signature: Default::default(),
            updating: Cell::new(false),
            composition,
            previous_focus: Default::default(),
            closing: Cell::new(None),
            paint: RefCell::default(),
            contacts: RefCell::default(),
        }
    }

    pub(crate) fn set_localization(&self, localization: std::sync::Arc<Localizer>) {
        let copy = NativeCopy::new(&localization).search;
        self.card.update_property(&[gtk::accessible::Property::Label(&copy.title)]);
        self.close.set_tooltip_text(Some(&copy.close_search));
        self.close.update_property(&[gtk::accessible::Property::Label(&copy.close_search)]);
        self.list.update_property(&[gtk::accessible::Property::Label(&copy.commands)]);
        self.empty.set_text(&copy.no_matches);
        *self.copy.borrow_mut() = copy;
        self.signature.borrow_mut().clear();
        self.update_entry_copy(None);
    }

    fn update_entry_copy(&self, parameter: Option<&str>) {
        let copy = self.copy.borrow();
        self.entry.update_property(&[gtk::accessible::Property::Label(parameter.unwrap_or(copy.search_commands.as_ref()))]);
        self.entry.set_placeholder_text(Some(if parameter.is_some() { copy.enter_value.as_ref() } else { copy.search_commands.as_ref() }));
    }

    pub(crate) fn owns_input(&self) -> bool { self.is_open() || self.closing.get().is_some() }

    pub(crate) fn allocate(&self, width: i32, height: i32, area: Bounds, phone: bool) {
        if !self.root.is_visible() { return; }
        let viewport = [width as f32, height as f32];
        let bounds = COMMAND_SEARCH_STYLE.placement(viewport, area, phone);
        self.card.set_halign(gtk::Align::Start);
        self.card.set_margin_start(bounds.x.round() as i32);
        self.card.set_width_request(bounds.width.round() as i32);
        self.card.set_margin_top(bounds.y.round() as i32);
        if let Some(scroll) = self.viewport.parent().and_downcast::<gtk::ScrolledWindow>() {
            let reserved = self.card.measure(gtk::Orientation::Vertical, bounds.width as i32).0
                - scroll.measure(gtk::Orientation::Vertical, bounds.width as i32).0;
            scroll.set_max_content_height((bounds.height as i32 - reserved).max(1));
        }
    }

    pub(crate) fn selected(&self, w: &Workspace, requested: u64, shown: u64) {
        if let Some(closing) = self.closing.get() && let Some(clock) = w.surface.frame_clock() {
            self.closing.set(Some(closing.selected(requested, shown, clock.frame_counter())));
        }
    }

    fn disconnect_paint(&self) {
        if let Some((clock, handler)) = self.paint.borrow_mut().take() { clock.disconnect(handler); }
    }

    fn retire(&self, w: &Rc<Workspace>, restore: bool) {
        self.closing.set(None);
        self.disconnect_paint();
        let focus = self.previous_focus.take().and_then(|focus| focus.upgrade()).filter(|focus| focus.is_mapped());
        self.root.set_visible(false);
        if restore && w.window.is_mapped() && w.visible_dialog().is_none() {
            if let Some(focus) = focus { focus.grab_focus(); } else { w.area.grab_focus(); }
        }
    }

    fn finish_close(&self, w: &Rc<Workspace>) {
        if self.closing.get() == Some(Closing::Painted) && self.contacts.borrow().is_empty() { self.retire(w, true); }
    }

    pub(crate) fn capture(&self, w: &Rc<Workspace>, event: &gdk::Event, point: Option<[f32; 2]>) -> bool {
        if !self.owns_input() { return false; }
        let kind = event.event_type();
        let touch = matches!(kind, gdk::EventType::TouchBegin | gdk::EventType::TouchUpdate | gdk::EventType::TouchEnd | gdk::EventType::TouchCancel);
        let key = (event.device(), touch.then(|| event.event_sequence()), event.downcast_ref::<gdk::ButtonEvent>().map_or(0, |event| event.button()));
        let terminal = matches!(kind, gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd | gdk::EventType::TouchCancel);
        let claimed = self.contacts.borrow().iter().position(|contact| if touch { *contact == key } else {
            contact.0 == key.0 && contact.1.is_none() && (!terminal || contact.2 == key.2)
        });
        if let Some(index) = claimed {
            if kind == gdk::EventType::ButtonPress && !self.contacts.borrow().contains(&key) { self.contacts.borrow_mut().push(key); }
            if terminal { self.contacts.borrow_mut().remove(index); self.finish_close(w); }
            return true;
        }
        if w.visible_dialog().is_some() || event.surface() != w.window.surface() { return false; }
        let pointer = matches!(kind, gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease | gdk::EventType::MotionNotify
            | gdk::EventType::TouchBegin | gdk::EventType::TouchUpdate | gdk::EventType::TouchEnd | gdk::EventType::TouchCancel | gdk::EventType::Scroll);
        if !pointer { return false; }
        let inside = self.is_open() && point.is_some_and(|[x,y]| self.card.compute_bounds(&w.surface)
            .is_some_and(|bounds| bounds.contains_point(&gtk::graphene::Point::new(x,y))));
        if inside { return false; }
        if matches!(kind, gdk::EventType::ButtonPress | gdk::EventType::TouchBegin) {
            self.contacts.borrow_mut().push(key);
            w.close_command_search();
        }
        true
    }

    pub fn bind(&self, w: &Rc<Workspace>) {
        self.close.connect_clicked(glib::clone!(#[weak] w, move |_| w.close_command_search()));
        self.card.connect_unmap(glib::clone!(#[weak] w, move |_| w.command_bar.composition.clear()));
        self.root.connect_unmap(glib::clone!(#[weak] w, move |_| {
            let open = w.command_bar.view.take().is_some();
            w.command_bar.contacts.borrow_mut().clear();
            w.command_bar.retire(&w, false);
            if open { w.close_command_search(); }
        }));
        self.entry.connect_changed(glib::clone!(
            #[weak]
            w,
            move |entry| {
                if !w.command_bar.updating.get()
                    && w.command_bar
                        .view
                        .borrow()
                        .as_ref()
                        .is_some_and(|v| v.parameter.is_none())
                {
                    w.dispatch(UiAction::CommandSearch {
                        action: CommandSearchAction::Query {
                            text: entry.text().into(),
                        },
                    });
                }
            }
        ));
        crate::input::stop_search(&self.entry, glib::clone!(#[weak] w, move |_| w.command_bar.cancel(&w)));
        self.entry.connect_activate(glib::clone!(
            #[weak]
            w,
            move |_| {
                if !w.command_bar.composition.active() { w.command_bar.execute(&w, None); }
            }
        ));
        self.list.connect_row_activated(glib::clone!(
            #[weak]
            w,
            move |_, row| w.command_bar.execute(&w, Some(row.index() as usize))
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_name(Some("command-search-navigation"));
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(#[weak] w, #[upgrade_or] glib::Propagation::Proceed, move |_, _, keycode, _| {
            w.command_bar.composition.capture(keycode);
            glib::Propagation::Proceed
        }));
        let composed = gtk::EventControllerKey::new();
        composed.set_name(Some("command-search-composed-key"));
        composed.set_propagation_phase(gtk::PropagationPhase::Bubble);
        composed.connect_key_pressed(glib::clone!(
            #[weak]
            w,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, key, keycode, modifiers| {
                if w.command_bar.composition.bubble(controller, keycode) == glib::Propagation::Stop { return glib::Propagation::Stop; }
                if !w.command_bar.is_open() { return glib::Propagation::Stop; }
                if matches!(key, gdk::Key::Tab | gdk::Key::ISO_Left_Tab) {
                    let direction = if key == gdk::Key::ISO_Left_Tab || modifiers.contains(gdk::ModifierType::SHIFT_MASK) {
                        gtk::DirectionType::TabBackward
                    } else { gtk::DirectionType::TabForward };
                    if !w.command_bar.card.child_focus(direction) {
                        gtk::prelude::GtkWindowExt::set_focus(&w.window, None::<&gtk::Widget>);
                        w.command_bar.card.child_focus(direction);
                    }
                    return glib::Propagation::Stop;
                }
                let action = match key {
                    gdk::Key::Up => Some(CommandSearchAction::Move { delta: -1 }),
                    gdk::Key::Down => Some(CommandSearchAction::Move { delta: 1 }),
                    gdk::Key::Escape => {
                        w.command_bar.cancel(&w);
                        return glib::Propagation::Stop;
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    w.dispatch(UiAction::CommandSearch { action });
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak]
            w,
            move |_, key, keycode, modifiers| {
                let preedit = w.command_bar.composition.release(keycode);
                if preedit || w.command_bar.composition.marked() { return; }
                w.interact(crate::input::key_input(key, keycode, false, modifiers, true, None));
            }
        ));
        self.root.add_controller(keys);
        self.root.add_controller(composed);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(#[weak] w, move |_| w.command_bar.composition.clear()));
        self.root.add_controller(focus);
    }

    fn cancel(&self, w: &Rc<Workspace>) {
        w.dispatch(UiAction::CommandSearch { action: CommandSearchAction::Dismiss {
            search_focused: self.entry.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN), text: self.entry.text().to_string(),
        } });
    }

    fn execute(&self, w: &Rc<Workspace>, index: Option<usize>) {
        if index.is_none() {
            w.dispatch(UiAction::CommandSearch {
                action: CommandSearchAction::Commit {
                    text: self.entry.text().into(),
                },
            });
            return;
        }
        let request = self.view.borrow().as_ref().and_then(|v| {
            let d = v
                .parameter
                .as_ref()
                .or_else(|| v.results.get(index.unwrap_or(v.selected)))?;
            Some(CommandSearchAction::Execute {
                id: d.id.clone(),
                value: v.parameter.as_ref().map(|_| self.entry.text().into()),
            })
        });
        if let Some(action) = request {
            w.dispatch(UiAction::CommandSearch { action });
        }
    }

    pub fn refresh(&self, w: &Rc<Workspace>, view: Option<CommandSearchView>) {
        self.updating.set(true);
        let was_open = self.view.borrow().is_some();
        let old_parameter = self
            .view
            .borrow()
            .as_ref()
            .and_then(|v| v.parameter.as_ref().map(|p| p.id.clone()));
        *self.view.borrow_mut() = view.clone();
        let Some(view) = view else {
            if was_open {
                self.closing.set(Some(Closing::Requested(None)));
                self.card.set_visible(false);
                if self.root.is_mapped() {
                    if w.visible_dialog().is_none() { self.root.grab_focus(); }
                    let clock = self.root.frame_clock().unwrap();
                    let handler = clock.connect_after_paint(glib::clone!(#[weak] w, move |clock| {
                        if let Some(closing) = w.command_bar.closing.get() {
                            w.command_bar.closing.set(Some(closing.painted(clock.frame_counter())));
                            w.command_bar.finish_close(&w);
                        }
                    }));
                    *self.paint.borrow_mut() = Some((clock, handler));
                    w.surface.queue_draw();
                } else {
                    self.contacts.borrow_mut().clear();
                    self.retire(w, false);
                }
            }
            self.updating.set(false);
            return;
        };
        let parameter = view.parameter.as_ref();
        let unit = parameter
            .and_then(|p| p.parameter.as_ref())
            .map(|p| p.numeric.unit.as_str())
            .unwrap_or("");
        self.unit.set_text(unit);
        self.unit.set_visible(!unit.is_empty());
        self.update_entry_copy(parameter.map(|p| p.label.as_str()));
        if !was_open || parameter.map(|p| &p.id) != old_parameter.as_ref() {
            self.entry.set_text(
                parameter
                    .and_then(|p| p.parameter.as_ref())
                    .map(|p| p.text.as_str())
                    .unwrap_or(&view.query),
            );
            if parameter.is_some() {
                self.entry.select_region(0, -1);
            }
        }
        if parameter.is_none() && self.entry.text().as_str() != view.query { self.entry.set_text(&view.query); }
        let signature = serde_json::to_string(&view.results).unwrap();
        if *self.signature.borrow() != signature {
            *self.signature.borrow_mut() = signature;
            while let Some(row) = self.list.first_child() {
                self.list.remove(&row);
            }
            for (i, command) in view.results.iter().enumerate() {
                let row = gtk::ListBoxRow::new();
                row.set_height_request(COMMAND_SEARCH_STYLE.row_height);
                row.set_widget_name(&format!("command-result-{i}"));
                let content =
                    gtk::Box::new(gtk::Orientation::Horizontal, COMMAND_SEARCH_STYLE.inset);
                let label = gtk::Label::new(Some(&command.label));
                label.set_xalign(0.);
                label.set_hexpand(true);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                if !command.enabled {
                    label.add_css_class("dim-label");
                }
                content.append(&label);
                if command.selected {
                    let check = crate::icons::image("layer-check-symbolic");
                    check.set_pixel_size(16);
                    content.append(&check);
                }
                let shortcut = gtk::Label::new(Some(&command.shortcut));
                shortcut.add_css_class("dim-label");
                shortcut.add_css_class("caption");
                content.append(&shortcut);
                row.set_child(Some(&content));
                row.update_property(&[gtk::accessible::Property::Description(
                    command
                        .disabled_reason
                        .as_deref()
                        .unwrap_or(&command.description),
                )]);
                let motion = gtk::EventControllerMotion::new();
                let id = command.id.clone();
                motion.connect_motion(glib::clone!(
                    #[weak]
                    w,
                    move |_, _, _| {
                        let selected = w
                            .command_bar
                            .view
                            .borrow()
                            .as_ref()
                            .and_then(|v| v.results.get(v.selected))
                            .is_some_and(|d| d.id == id);
                        if !selected {
                            w.dispatch(UiAction::CommandSearch {
                                action: CommandSearchAction::Select { id: id.clone() },
                            });
                        }
                    }
                ));
                row.add_controller(motion);
                self.list.append(&row);
            }
        }
        self.list.set_visible(parameter.is_none());
        self.empty
            .set_visible(parameter.is_none() && view.results.is_empty());
        if let Some(row) = self.list.row_at_index(view.selected as i32) {
            self.list.select_row(Some(&row));
            if parameter.is_none() {
                self.viewport.scroll_to(&row, None);
                self.entry
                    .update_relation(&[gtk::accessible::Relation::ActiveDescendant(
                        row.upcast_ref(),
                    )]);
            }
        }
        if parameter.is_some() || view.results.is_empty() {
            self.entry
                .reset_relation(gtk::AccessibleRelation::ActiveDescendant);
        }
        self.detail.set_text(&view.detail);
        self.detail
            .set_tooltip_text((!view.detail.is_empty()).then_some(view.detail.as_str()));
        if !was_open {
            if self.closing.take().is_none() {
                *self.previous_focus.borrow_mut() = gtk::prelude::GtkWindowExt::focus(&w.window).map(|focus| focus.downgrade());
            }
            self.disconnect_paint();
            if let Some(adjustment) = self.viewport.vadjustment() { adjustment.set_value(0.); }
            self.card.set_visible(true);
            self.root.set_visible(true);
            let viewport = [w.surface.width() as f32, w.surface.height() as f32];
            let layout = w.gpu.borrow().as_ref().map(|gpu| gpu.session.layout(viewport)).unwrap();
            self.allocate(w.surface.width(), w.surface.height(), layout.work_area, layout.phone.is_some());
            w.surface.queue_allocate();
            self.entry.grab_focus();
        }
        self.updating.set(false);
    }
}

impl Drop for CommandBar {
    fn drop(&mut self) {
        self.disconnect_paint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_search_waits_for_selection_and_its_painted_frame() {
        let requested = Closing::Requested(None).selected(7, 6, 11);
        assert_eq!(requested, Closing::Requested(Some(7)));
        assert_eq!(requested.painted(12), requested);
        let selected = requested.selected(9, 7, 13);
        assert_eq!(selected, Closing::Selected(13));
        assert_eq!(selected.painted(12), selected);
        assert_eq!(selected.painted(13), Closing::Painted);
    }

    #[test]
    fn closing_search_accepts_an_unconstrained_generation_without_waiting_for_gpu_visibility() {
        assert_eq!(Closing::Requested(None).selected(4, 4, 8).painted(8), Closing::Painted);
    }

    #[test]
    #[ignore = "private display native_command_search_copy"]
    fn native_command_search_copy() {
        adw::init().unwrap();
        for theme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            adw::StyleManager::default().set_color_scheme(theme);
            for language in UiLanguage::ALL {
                let context = Localizer::shared(language);
                let expected = NativeCopy::new(&context).search;
                let bar = CommandBar::with_copy(expected.clone());
                assert_eq!(bar.entry.placeholder_text().as_deref(), Some(expected.search_commands.as_ref()));
                assert_eq!(bar.empty.text().as_str(), expected.no_matches.as_ref());
                bar.update_entry_copy(Some("literal parameter & name"));
                assert_eq!(bar.entry.placeholder_text().as_deref(), Some(expected.enter_value.as_ref()));
                bar.update_entry_copy(None);
                assert_eq!(bar.entry.placeholder_text().as_deref(), Some(expected.search_commands.as_ref()));
                assert!(std::sync::Arc::ptr_eq(&bar.copy.borrow().search_commands, &expected.search_commands));
            }
        }
        let removed = Rc::new(Cell::new(false));
        let signal = removed.clone();
        let content = gtk::Label::new(Some("literal layer"));
        let row = crate::swipe_row::SwipeRow::new(&content, move || signal.set(true), || {}, || {}, || {}, |_| {}, |_| {});
        let copy = layer_ui::CommonCopy::new(crate::launch_localization());
        assert_eq!(row.delete_button().child().unwrap().downcast::<gtk::Label>().unwrap().text().as_str(), copy.delete.as_ref());
        row.delete_button().emit_clicked();
        assert!(removed.get());
    }
}
