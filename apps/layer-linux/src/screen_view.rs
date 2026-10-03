use crate::canvas::GpuCanvas;
use crate::panel_controls::SPACING;
use crate::workspace::Workspace;
use gtk::prelude::*;
use gtk::{gdk, glib};
use layer_color::screen::edid::{self, Edid};
use layer_color::screen::ScreenReport;
use layer_core::color::RgbSpace;
use layer_ui::{MessageId, ScreenChip, ScreenDetails, UiAction};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const DETAILS_WIDTH: i32 = 340;

struct Monitor {
    monitor: gdk::Monitor,
    name: Option<String>,
    edid: Option<Edid>,
}

pub(crate) struct ScreenView {
    pub(crate) button: gtk::Button,
    icon: gtk::Image,
    label: gtk::Label,
    popover: gtk::Popover,
    title: gtk::Label,
    headline: gtk::Label,
    body: gtk::Label,
    mark: gtk::CheckButton,
    updating: Cell<bool>,
    details: RefCell<Option<ScreenDetails>>,
    monitor: RefCell<Option<Monitor>>,
    #[cfg(test)]
    pub(crate) forced: RefCell<Option<ScreenReport>>,
}

impl ScreenView {
    pub(crate) fn new() -> Rc<Self> {
        let icon = gtk::Image::from_icon_name("layer-warning-symbolic");
        icon.add_css_class("warning");
        let label = gtk::Label::new(None);
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        chip.append(&icon);
        chip.append(&label);
        let button = gtk::Button::builder().child(&chip).visible(false).build();
        button.add_css_class("flat");
        button.add_css_class("status-bubble");
        button.set_widget_name("screen-status");
        let title = text("");
        title.add_css_class("caption");
        title.add_css_class("dim-label");
        let headline = text("");
        headline.add_css_class("heading");
        let heading = gtk::Box::new(gtk::Orientation::Vertical, SPACING / 2);
        heading.append(&title);
        heading.append(&headline);
        let body = text("");
        let mark = crate::panel_controls::check("");
        mark.set_widget_name("screen-mark-clipped");
        let content = gtk::Box::new(gtk::Orientation::Vertical, SPACING * 2);
        content.set_widget_name("screen-details");
        content.set_width_request(DETAILS_WIDTH);
        for part in [heading.upcast_ref::<gtk::Widget>(), body.upcast_ref(), mark.upcast_ref()] {
            content.append(part);
        }
        let popover = gtk::Popover::builder().child(&content).position(gtk::PositionType::Top).build();
        popover.set_parent(&button);
        Rc::new(Self {
            button,
            icon,
            label,
            popover,
            title,
            headline,
            body,
            mark,
            updating: Cell::new(false),
            details: RefCell::default(),
            monitor: RefCell::default(),
            #[cfg(test)]
            forced: RefCell::default(),
        })
    }

    pub(crate) fn bind(self: &Rc<Self>, w: &Rc<Workspace>) {
        w.on_localization(glib::clone!(#[weak(rename_to = this)] self, #[upgrade_or] false, move |localization| {
            this.button.set_tooltip_text(Some(localization.text(MessageId::NATIVE_SCREEN_DETAILS).as_ref()));
            this.mark.set_label(Some(localization.text(MessageId::NATIVE_HIGHLIGHT_CLIPPED_COLORS).as_ref()));
            true
        }));
        self.button.connect_clicked(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            w,
            move |button| {
                let Some(details) = w.gpu.borrow().as_ref().and_then(|g| g.session.screen_details()) else { return };
                this.show_details(details);
                if button.root().is_some_and(|root| root.focus().is_none()) {
                    button.grab_focus();
                }
                this.popover.popup();
            }
        ));
        self.mark.connect_toggled(glib::clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            w,
            move |mark| {
                if !this.updating.get() {
                    w.dispatch(UiAction::ShowClippedColors { visible: mark.is_active() });
                }
            }
        ));
    }

    #[cfg(test)]
    pub(crate) fn popover(&self) -> &gtk::Popover {
        &self.popover
    }

    #[cfg(test)]
    pub(crate) fn chip_label(&self) -> Option<String> {
        self.button.is_visible().then(|| self.label.label().to_string())
    }

    pub(crate) fn sync(self: &Rc<Self>, w: &Rc<Workspace>, g: &mut GpuCanvas) {
        let (name, monitor) = self.monitor(w);
        let backend = g.session.engine().backend();
        let hdr_capable = monitor.as_ref().map(|m| m.pq_signal);
        let report = ScreenReport { name, color: backend.screen_color, monitor, hdr_capable, wide_color_off: false };
        #[cfg(test)]
        let report = self.forced.borrow().clone().unwrap_or(report);
        let hdr_surface = backend.display_encoding.is_some();
        let view = if hdr_surface { RgbSpace::Srgb } else { backend.view_color.space() };
        g.session.set_screen_report(report);
        let screen = &g.session.state().screen;
        let check = layer_render_wgpu::ScreenCheck::for_view(&screen.assessment, view, hdr_surface, screen.show_clipped);
        let headroom = screen.assessment.headroom();
        if let Err(e) = g.session.renderer_mut().set_screen(headroom, check) {
            eprintln!("Screen check: {e}");
        }
        let clipped = g.session.engine().backend().screen_clipped;
        g.session.set_screen_clipped(clipped);
        self.show_chip(g.session.screen_chip());
        if self.popover.is_visible() {
            match g.session.screen_details().filter(|_| self.button.is_visible()) {
                Some(details) => self.show_details(details),
                None => self.popover.popdown(),
            }
        }
    }

    fn show_chip(&self, chip: Option<ScreenChip>) {
        self.button.set_visible(chip.is_some());
        if let Some(chip) = chip {
            self.label.set_label(&chip.label);
            self.icon.set_visible(chip.warning);
        }
    }

    fn show_details(&self, details: ScreenDetails) {
        if self.details.borrow().as_ref() == Some(&details) {
            return;
        }
        self.title.set_label(&details.title);
        self.headline.set_label(&details.headline);
        if details.warning {
            self.headline.add_css_class("warning");
        } else {
            self.headline.remove_css_class("warning");
        }
        self.body.set_label(details.body.as_deref().unwrap_or_default());
        self.body.set_visible(details.body.is_some());
        self.updating.set(true);
        self.mark.set_active(details.show_clipped == Some(true));
        self.updating.set(false);
        self.mark.set_visible(details.show_clipped.is_some());
        *self.details.borrow_mut() = Some(details);
    }

    fn monitor(&self, w: &Workspace) -> (Option<String>, Option<Edid>) {
        let current = w.window.surface().and_then(|s| s.display().monitor_at_surface(&s));
        let mut cached = self.monitor.borrow_mut();
        if cached.as_ref().map(|c| &c.monitor) != current.as_ref() {
            *cached = current.map(|monitor| {
                let edid = monitor_edid(&monitor);
                let name = edid
                    .as_ref()
                    .and_then(|e| e.name.clone())
                    .or_else(|| monitor.model().map(String::from))
                    .or_else(|| monitor.description().map(String::from));
                Monitor { monitor, name, edid }
            });
        }
        cached.as_ref().map_or((None, None), |c| (c.name.clone(), c.edid.clone()))
    }
}

impl Drop for ScreenView {
    fn drop(&mut self) {
        self.popover.unparent();
    }
}

fn text(value: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_max_width_chars(1);
    label
}

fn monitor_edid(monitor: &gdk::Monitor) -> Option<Edid> {
    let connector = monitor.connector()?;
    let model = monitor.model();
    let candidates: Vec<Edid> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.split_once('-'))
                .is_some_and(|(_, name)| name == connector.as_str())
        })
        .filter(|path| std::fs::read_to_string(path.join("status")).is_ok_and(|s| s.trim() == "connected"))
        .filter_map(|path| edid::parse(&std::fs::read(path.join("edid")).ok()?).ok())
        .collect();
    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }
    candidates.into_iter().find(|e| e.name.is_some() && e.name.as_deref() == model.as_deref())
}
