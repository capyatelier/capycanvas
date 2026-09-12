//! GTK 4.22 re-queries the seat's mouse position after its tooltip timeout,
//! even for tablet events. Use the pen's picked widget without moving the mouse.
use adw::prelude::*;
use gtk::{gdk, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

type Text = Rc<dyn Fn(&gtk::Widget) -> Option<String>>;
#[derive(Default)]
pub(crate) struct PenTooltips {
    pending: RefCell<Option<glib::SourceId>>,
    source: RefCell<Option<(gtk::Widget, gtk::Widget)>>,
    popup: RefCell<Option<gtk::Popover>>,
    providers: RefCell<Vec<(glib::WeakRef<gtk::Widget>, Text)>>,
    suppressed: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
    pen_active: Cell<bool>,
    watchers: RefCell<Vec<(gtk::Widget, glib::SignalHandlerId)>>,
}

impl PenTooltips {
    pub fn bind(
        &self,
        widget: &impl IsA<gtk::Widget>,
        text: impl Fn(&gtk::Widget) -> Option<String> + 'static,
    ) {
        let widget = widget.as_ref();
        let text: Text = Rc::new(text);
        widget.set_has_tooltip(true);
        widget.connect_query_tooltip({
            let text = text.clone();
            move |widget, _, _, _, tooltip| {
                let Some(text) = text(widget) else {
                    return false;
                };
                tooltip.set_text(Some(&text));
                true
            }
        });
        let mut providers = self.providers.borrow_mut();
        providers.retain(|(w, _)| w.upgrade().is_some_and(|w| w != *widget));
        providers.push((widget.downgrade(), text));
    }

    pub fn install(self: &Rc<Self>, window: &impl IsA<gtk::Window>) {
        let window = window.as_ref();
        let events = gtk::EventControllerLegacy::new();
        events.set_name(Some("pen-tooltips"));
        events.set_propagation_phase(gtk::PropagationPhase::Capture);
        events.set_propagation_limit(gtk::PropagationLimit::None);
        events.connect_event(glib::clone!(
            #[weak(rename_to = tips)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, event| {
                // GTK synthesizes mouse motion after layout changes. It must
                // not retire the actual pen's hover (native tooltips skip it too).
                if event.event_type() == gdk::EventType::MotionNotify
                    && event.time() == gdk::CURRENT_TIME
                {
                    return glib::Propagation::Proceed;
                }
                let pen = event.device_tool().is_some()
                    || event
                        .device()
                        .is_some_and(|d| d.source() == gdk::InputSource::Pen);
                let pressed = event.modifier_state().intersects(
                    gdk::ModifierType::BUTTON1_MASK
                        | gdk::ModifierType::BUTTON2_MASK
                        | gdk::ModifierType::BUTTON3_MASK
                        | gdk::ModifierType::BUTTON4_MASK
                        | gdk::ModifierType::BUTTON5_MASK,
                );
                if pen
                    && !pressed
                    && matches!(
                        event.event_type(),
                        gdk::EventType::MotionNotify
                            | gdk::EventType::EnterNotify
                            | gdk::EventType::ProximityIn
                    )
                {
                    if let Some((native, point)) = event
                        .surface()
                        .and_then(|surface| gtk::Native::for_surface(&surface))
                        .and_then(|native| {
                            let (x, y) = event.position()?;
                            let (dx, dy) = native.surface_transform();
                            Some((native.dynamic_cast::<gtk::Widget>().ok()?, [x - dx, y - dy]))
                        })
                    {
                        tips.hover(&native, point);
                    }
                } else if !matches!(
                    event.event_type(),
                    gdk::EventType::MotionNotify | gdk::EventType::EnterNotify
                ) || !pen
                    || pressed
                {
                    tips.hide();
                    if !pen || event.event_type() == gdk::EventType::ProximityOut {
                        tips.restore_mouse();
                    }
                }
                glib::Propagation::Proceed
            }
        ));
        window.add_controller(events);
        window.connect_is_active_notify(glib::clone!(
            #[weak(rename_to = tips)]
            self,
            move |window| {
                if !window.is_active() {
                    tips.hide();
                    tips.restore_mouse();
                }
            }
        ));
        window.connect_unmap(glib::clone!(
            #[weak(rename_to = tips)]
            self,
            move |_| {
                tips.hide();
                tips.restore_mouse();
            }
        ));
    }

    fn text(&self, widget: &gtk::Widget) -> Option<String> {
        let provider = self
            .providers
            .borrow()
            .iter()
            .find(|(w, _)| w.upgrade().as_ref() == Some(widget))
            .map(|(_, p)| p.clone());
        if let Some(provider) = provider {
            provider(widget)
        } else {
            widget.tooltip_text().map(Into::into)
        }
    }

    // A parked mouse must not show a second, unrelated native tooltip while
    // the pen is browsing. Restore native mouse tooltips on mouse input/exit.
    fn suppress_mouse(&self, native: &gtk::Widget) {
        let Some(device) = native.display().default_seat().and_then(|s| s.pointer()) else {
            return;
        };
        let (Some(surface), x, y) = device.surface_at_position() else {
            return;
        };
        let Some(root) = gtk::Native::for_surface(&surface) else {
            return;
        };
        if root.root() != native.root() {
            return;
        }
        let (dx, dy) = root.surface_transform();
        let root = root.dynamic_cast::<gtk::Widget>().unwrap();
        let mut picked = root.pick(x - dx, y - dy, gtk::PickFlags::INSENSITIVE);
        while let Some(widget) = picked {
            if widget.has_tooltip() {
                self.suppressed.borrow_mut().push(widget.downgrade());
                widget.set_has_tooltip(false);
            }
            picked = widget.parent();
        }
        root.trigger_tooltip_query();
    }

    fn restore_mouse(&self) {
        self.pen_active.set(false);
        for widget in self
            .suppressed
            .take()
            .into_iter()
            .filter_map(|w| w.upgrade())
        {
            widget.set_has_tooltip(true);
        }
    }

    pub(crate) fn hover(self: &Rc<Self>, native: &gtk::Widget, point: [f64; 2]) {
        if !self.pen_active.replace(true) {
            self.suppress_mouse(native);
        }
        let mut picked = native.pick(point[0], point[1], gtk::PickFlags::INSENSITIVE);
        let target = loop {
            let Some(widget) = picked else {
                self.hide();
                return;
            };
            let suppressed = self
                .suppressed
                .borrow()
                .iter()
                .any(|w| w.upgrade().as_ref() == Some(&widget));
            if (widget.has_tooltip() || suppressed)
                && self.text(&widget).is_some_and(|s| !s.is_empty())
            {
                break widget;
            }
            picked = widget.parent();
        };
        if self
            .source
            .borrow()
            .as_ref()
            .is_some_and(|(s, p)| *s == target && s.parent().as_ref() == Some(p))
        {
            return;
        }
        self.hide();
        self.suppress_mouse(native);
        let Some(parent) = target.parent() else {
            return;
        };
        // Retire both pending and visible tooltips when their anchor changes.
        let unmapped = target.connect_unmap(glib::clone!(
            #[weak(rename_to = tips)]
            self,
            move |_| tips.hide()
        ));
        let reparented = target.connect_parent_notify(glib::clone!(
            #[weak(rename_to = tips)]
            self,
            move |_| tips.hide()
        ));
        self.watchers.borrow_mut().extend(
            [unmapped, reparented]
                .into_iter()
                .map(|id| (target.clone(), id)),
        );
        *self.source.borrow_mut() = Some((target, parent));
        *self.pending.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(500),
            glib::clone!(
                #[weak(rename_to = tips)]
                self,
                move || {
                    tips.pending.borrow_mut().take();
                    tips.show();
                }
            ),
        ));
    }

    fn show(self: &Rc<Self>) {
        let Some((source, parent)) = self.source.borrow().clone() else {
            return;
        };
        if !source.is_mapped() || source.parent().as_ref() != Some(&parent) {
            self.hide();
            return;
        }
        let Some(text) = self.text(&source) else {
            self.hide();
            return;
        };
        let Some(native) = source
            .native()
            .and_then(|n| n.dynamic_cast::<gtk::Widget>().ok())
        else {
            return;
        };
        let Some(bounds) = source.compute_bounds(&native) else {
            return;
        };
        self.suppress_mouse(&native);
        let label = gtk::Label::new(Some(&text));
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_max_width_chars(40);
        let popup = gtk::Popover::builder()
            .accessible_role(gtk::AccessibleRole::Tooltip)
            .autohide(false)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .child(&label)
            .build();
        popup.add_css_class("pen-tooltip");
        popup.set_widget_name("pen-tooltip");
        popup.set_can_target(false);
        popup.set_focusable(false);
        popup.set_offset(0, 4);
        popup.set_pointing_to(Some(&gdk::Rectangle::new(
            bounds.x().floor() as i32,
            bounds.y().floor() as i32,
            bounds.width().ceil() as i32,
            bounds.height().ceil() as i32,
        )));
        popup.set_parent(&native);
        *self.popup.borrow_mut() = Some(popup.clone());
        popup.popup();
    }

    pub(crate) fn hide(&self) {
        if let Some(timer) = self.pending.take() {
            timer.remove();
        }
        for (widget, id) in self.watchers.take() {
            widget.disconnect(id);
        }
        self.source.borrow_mut().take();
        if let Some(popup) = self.popup.take() {
            popup.popdown();
            popup.unparent();
        }
    }
}

impl Drop for PenTooltips {
    fn drop(&mut self) {
        self.hide();
        self.restore_mouse();
    }
}
