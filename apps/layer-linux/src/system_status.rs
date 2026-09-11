//! Native clock preferences and UPower observation, independent of GPU startup.
use adw::prelude::*;
use gtk::{gio, glib};
#[path = "battery_font.rs"]
mod battery_font;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Battery {
    pub percent: u32,
    pub charging: bool,
    pub low: bool,
}
pub(crate) struct SystemStatus {
    pub root: gtk::Box,
    clock: gtk::Label,
    battery: gtk::Box,
    percent: gtk::Label,
    drawing: gtk::DrawingArea,
    value: Rc<Cell<Option<Battery>>>,
    fullscreen: Cell<bool>,
    show_clock: Cell<layer_ui::ClockVisibility>,
    settings: Option<gio::Settings>,
    proxy: RefCell<Option<gio::DBusProxy>>,
    timer: RefCell<Option<glib::SourceId>>,
}
fn twelve_hour(preference: Option<&str>, locale_format: &str) -> bool {
    match preference {
        Some("12h") => true,
        Some("24h") => false,
        _ => ["%I", "%l", "%p", "%r"]
            .iter()
            .any(|p| locale_format.contains(p)),
    }
}
impl SystemStatus {
    pub fn new() -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.set_widget_name("system-status");
        root.add_css_class("system-status");
        root.set_visible(false);
        let clock = gtk::Label::new(None);
        clock.set_widget_name("system-clock");
        clock.add_css_class("header-clock");
        let battery = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        battery.set_widget_name("system-battery");
        battery.add_css_class("system-battery");
        battery.set_size_request(layer_ui::TILE_SIZE as i32, layer_ui::TILE_SIZE as i32);
        battery.set_hexpand(false);
        battery.set_valign(gtk::Align::Center);
        battery.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_hexpand(true);
        overlay.set_halign(gtk::Align::Center);
        overlay.set_valign(gtk::Align::Center);
        let drawing = gtk::DrawingArea::new();
        drawing.set_content_width(26);
        drawing.set_content_height(14);
        drawing.set_valign(gtk::Align::Center);
        let percent = gtk::Label::new(None);
        percent.add_css_class("battery-percent");
        battery_font::load(&percent.pango_context());
        percent.pango_context().set_round_glyph_positions(false);
        percent.set_margin_end(4);
        overlay.set_child(Some(&drawing));
        overlay.add_overlay(&percent);
        battery.append(&overlay);
        root.append(&clock);
        root.append(&battery);
        let settings = gio::SettingsSchemaSource::default()
            .and_then(|s| s.lookup("org.gnome.desktop.interface", true))
            .filter(|s| s.has_key("clock-format"))
            .map(|s| gio::Settings::new_full(&s, None::<&gio::SettingsBackend>, None));
        let value = Rc::new(Cell::new(None::<Battery>));
        drawing.set_draw_func(glib::clone!(
            #[strong]
            value,
            move |area, cr, width, height| {
                let Some(battery) = value.get() else {
                    return;
                };
                let dark = adw::StyleManager::for_display(&area.display()).is_dark();
                let (fill, track, ink) = match (battery.charging, battery.low, dark) {
                    (true, _, _) => (0x91b89d, 0xc4c9cf, 0x13251a),
                    (_, true, true) => (0xbc9996, 0xa3a8b0, 0x202226),
                    (_, true, false) => (0xa15d59, 0x707479, 0xffffff),
                    (_, _, true) => (0xe5e7eb, 0xa3a8b0, 0x202226),
                    _ => (0x3f4246, 0x707479, 0xffffff),
                };
                let color = |rgb: u32| {
                    cr.set_source_rgb(
                        ((rgb >> 16) & 255) as f64 / 255.,
                        ((rgb >> 8) & 255) as f64 / 255.,
                        (rgb & 255) as f64 / 255.,
                    )
                };
                let _ = cr.save();
                // Use the same 22 × 14 body and terminal geometry as Android/web.
                let u = (height as f64 / 14.).min(width as f64 / 26.);
                cr.translate(0., (height as f64 - 14. * u) / 2.);
                cr.scale(u, u);
                let body = || {
                    cr.new_sub_path();
                    cr.arc(18., 4., 4., -std::f64::consts::FRAC_PI_2, 0.);
                    cr.arc(18., 10., 4., 0., std::f64::consts::FRAC_PI_2);
                    cr.arc(
                        4.,
                        10.,
                        4.,
                        std::f64::consts::FRAC_PI_2,
                        std::f64::consts::PI,
                    );
                    cr.arc(
                        4.,
                        4.,
                        4.,
                        std::f64::consts::PI,
                        3. * std::f64::consts::FRAC_PI_2,
                    );
                    cr.close_path();
                };
                body();
                color(track);
                let _ = cr.fill_preserve();
                let _ = cr.save();
                cr.clip();
                color(fill);
                cr.rectangle(0., 0., 22. * battery.percent as f64 / 100., 14.);
                let _ = cr.fill();
                let _ = cr.restore();
                color(track);
                if battery.charging {
                    cr.move_to(23., 2.);
                    cr.line_to(18.5, 8.);
                    cr.line_to(21.2, 8.);
                    cr.line_to(20., 12.);
                    cr.line_to(25., 5.9);
                    cr.line_to(22.7, 5.9);
                    cr.line_to(24.1, 2.);
                    cr.close_path();
                    cr.set_line_width(1.75);
                    cr.set_line_join(gtk::cairo::LineJoin::Miter);
                    cr.set_miter_limit(4.);
                    color(if dark { 0x202226 } else { track });
                    let _ = cr.stroke_preserve();
                    color(if dark { 0xe5e7eb } else { ink });
                    let _ = cr.fill();
                } else {
                    cr.new_sub_path();
                    cr.arc(24., 5., 1., std::f64::consts::PI, 2. * std::f64::consts::PI);
                    cr.arc(24., 9., 1., 0., std::f64::consts::PI);
                    cr.close_path();
                    let _ = cr.fill();
                }
                let _ = cr.restore();
            }
        ));
        let this = Rc::new(Self {
            root,
            clock,
            battery,
            percent,
            drawing,
            value,
            fullscreen: Cell::new(false),
            show_clock: Cell::new(layer_ui::ClockVisibility::default()),
            settings,
            proxy: RefCell::new(None),
            timer: RefCell::new(None),
        });
        let style = adw::StyleManager::for_display(&this.root.display());
        this.update_style(&style);
        style.connect_dark_notify(glib::clone!(
            #[weak]
            this,
            move |style| this.update_style(style)
        ));
        this.update_clock();
        this.clock.connect_map(glib::clone!(
            #[weak]
            this,
            move |_| this.schedule_clock()
        ));
        this.clock.connect_unmap(glib::clone!(
            #[weak]
            this,
            move |_| this.stop_clock()
        ));
        if let Some(settings) = &this.settings {
            settings.connect_changed(
                Some("clock-format"),
                glib::clone!(
                    #[weak]
                    this,
                    move |_, _| this.update_clock()
                ),
            );
        }
        glib::MainContext::default().spawn_local(glib::clone!(
            #[weak]
            this,
            async move {
                let Ok(proxy) = gio::DBusProxy::for_bus_future(
                    gio::BusType::System,
                    gio::DBusProxyFlags::DO_NOT_AUTO_START
                        | gio::DBusProxyFlags::GET_INVALIDATED_PROPERTIES,
                    None,
                    "org.freedesktop.UPower",
                    "/org/freedesktop/UPower/devices/DisplayDevice",
                    "org.freedesktop.UPower.Device",
                )
                .await
                else {
                    return;
                };
                proxy.connect_local(
                    "g-properties-changed",
                    false,
                    glib::clone!(
                        #[weak]
                        this,
                        #[upgrade_or]
                        None,
                        move |_| {
                            this.update_power();
                            None
                        }
                    ),
                );
                proxy.connect_notify_local(
                    Some("g-name-owner"),
                    glib::clone!(
                        #[weak]
                        this,
                        move |_, _| this.update_power()
                    ),
                );
                *this.proxy.borrow_mut() = Some(proxy);
                this.update_power();
            }
        ));
        this
    }
    pub fn set_visibility(&self, fullscreen: bool, show_clock: layer_ui::ClockVisibility) {
        self.fullscreen.set(fullscreen);
        self.show_clock.set(show_clock);
        self.update_visibility();
    }
    fn update_visibility(&self) {
        let clock = self.show_clock.get().visible(self.fullscreen.get());
        let battery = clock && self.value.get().is_some();
        self.clock.set_visible(clock);
        self.battery.set_visible(battery);
        self.root.set_visible(clock || battery);
    }
    fn update_style(&self, style: &adw::StyleManager) {
        if style.is_dark() {
            self.root.remove_css_class("light");
        } else {
            self.root.add_css_class("light");
        }
        self.drawing.queue_draw();
    }
    fn update_clock(&self) {
        let preference = self.settings.as_ref().map(|s| s.string("clock-format"));
        // GTK initializes the process locale; nl_langinfo reflects LC_TIME.
        let locale =
            unsafe { std::ffi::CStr::from_ptr(libc::nl_langinfo(libc::T_FMT)) }.to_string_lossy();
        let twelve = twelve_hour(preference.as_deref(), &locale);
        if let Ok(now) = glib::DateTime::now_local()
            && let Ok(text) = now.format(if twelve { "%I:%M %p" } else { "%H:%M" })
        {
            let text = if twelve {
                text.trim_start_matches('0')
            } else {
                &text
            };
            if self.clock.text() != text {
                self.clock.set_text(text);
            }
        }
    }
    fn schedule_clock(self: &Rc<Self>) {
        self.stop_clock();
        self.update_clock();
        if !self.clock.is_mapped() {
            return;
        }
        let delay = 60_000 - (glib::real_time() / 1000).rem_euclid(60_000) + 20;
        *self.timer.borrow_mut() = Some(glib::timeout_add_local_once(
            Duration::from_millis(delay as u64),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move || {
                    this.timer.borrow_mut().take();
                    this.schedule_clock();
                }
            ),
        ));
    }
    fn stop_clock(&self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
    }
    fn update_power(&self) {
        let proxy = self.proxy.borrow();
        let battery = proxy.as_ref().and_then(|p| {
            if p.name_owner().is_none() || !p.cached_property("IsPresent")?.get::<bool>()? {
                return None;
            }
            let percent = p.cached_property("Percentage")?.get::<f64>()?;
            if !percent.is_finite() || !(0. ..=100.).contains(&percent) {
                return None;
            }
            let state = p
                .cached_property("State")
                .and_then(|v| v.get::<u32>())
                .unwrap_or(0);
            let warning = p
                .cached_property("WarningLevel")
                .and_then(|v| v.get::<u32>())
                .unwrap_or(0);
            Some(Battery {
                percent: percent.round() as u32,
                charging: matches!(state, 1 | 4 | 5),
                low: warning >= 3,
            })
        });
        self.show_battery(battery);
    }
    pub(crate) fn show_battery(&self, value: Option<Battery>) {
        if self.value.replace(value) == value {
            return;
        }
        self.update_visibility();
        if let Some(b) = value {
            self.percent.set_text(&b.percent.to_string());
            self.percent.set_margin_end(if b.charging { 5 } else { 4 });
            if b.percent == 100 {
                self.battery.add_css_class("full");
            } else {
                self.battery.remove_css_class("full");
            }
            if b.charging {
                self.battery.add_css_class("charging");
            } else {
                self.battery.remove_css_class("charging");
            }
            if b.low && !b.charging {
                self.battery.add_css_class("low");
            } else {
                self.battery.remove_css_class("low");
            }
            let description = format!(
                "Battery {}%{}",
                b.percent,
                if b.charging {
                    ", charging"
                } else if b.low {
                    ", low"
                } else {
                    ""
                }
            );
            self.battery.set_tooltip_text(Some(&description));
            self.battery
                .update_property(&[gtk::accessible::Property::Label(&description)]);
        }
        self.drawing.queue_draw();
    }
}
impl Drop for SystemStatus {
    fn drop(&mut self) {
        self.stop_clock();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_preference_overrides_locale_and_missing_schema_uses_locale() {
        assert!(twelve_hour(Some("12h"), "%H:%M:%S"));
        assert!(!twelve_hour(Some("24h"), "%r"));
        assert!(twelve_hour(None, "%I:%M:%S %p"));
        assert!(twelve_hour(None, "%r"));
        assert!(!twelve_hour(None, "%T"));
    }
}
