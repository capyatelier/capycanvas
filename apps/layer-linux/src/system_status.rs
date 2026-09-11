//! Native clock preferences and UPower observation, independent of GPU startup.
use adw::prelude::*;
use gtk::{gio, glib};
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
    charging: gtk::Label,
    percent: gtk::Label,
    drawing: gtk::DrawingArea,
    value: Rc<Cell<Option<Battery>>>,
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
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        root.set_widget_name("system-status");
        root.add_css_class("system-status");
        root.set_visible(false);
        let clock = gtk::Label::new(None);
        clock.set_widget_name("system-clock");
        let battery = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        battery.set_widget_name("system-battery");
        battery.add_css_class("system-battery");
        battery.set_visible(false);
        let charging = gtk::Label::new(Some("ϟ"));
        let overlay = gtk::Overlay::new();
        let drawing = gtk::DrawingArea::new();
        drawing.set_content_width(42);
        drawing.set_content_height(22);
        drawing.set_valign(gtk::Align::Center);
        let percent = gtk::Label::new(None);
        percent.add_css_class("battery-percent");
        percent.set_margin_end(4);
        overlay.set_child(Some(&drawing));
        overlay.add_overlay(&percent);
        battery.append(&charging);
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
                let color = area.color();
                let (w, h) = (width as f64 - 4., height as f64);
                let rounded = |cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64| {
                    let r = 3.;
                    cr.new_sub_path();
                    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.);
                    cr.arc(x + w - r, y + h - r, r, 0., std::f64::consts::FRAC_PI_2);
                    cr.arc(
                        x + r,
                        y + h - r,
                        r,
                        std::f64::consts::FRAC_PI_2,
                        std::f64::consts::PI,
                    );
                    cr.arc(
                        x + r,
                        y + r,
                        r,
                        std::f64::consts::PI,
                        3. * std::f64::consts::FRAC_PI_2,
                    );
                    cr.close_path();
                };
                cr.set_source_rgba(
                    color.red() as f64,
                    color.green() as f64,
                    color.blue() as f64,
                    1.,
                );
                rounded(cr, 0.75, 0.75, w - 1.5, h - 1.5);
                cr.set_line_width(1.5);
                let _ = cr.stroke();
                cr.rectangle(w + 2., h * 0.3, 2., h * 0.4);
                let _ = cr.fill();
                let _ = cr.save();
                rounded(cr, 1.5, 1.5, w - 3., h - 3.);
                cr.clip();
                cr.set_source_rgba(
                    color.red() as f64,
                    color.green() as f64,
                    color.blue() as f64,
                    0.18,
                );
                cr.rectangle(1.5, 1.5, (w - 3.) * battery.percent as f64 / 100., h - 3.);
                let _ = cr.fill();
                let _ = cr.restore();
            }
        ));
        let this = Rc::new(Self {
            root,
            clock,
            battery,
            charging,
            percent,
            drawing,
            value,
            settings,
            proxy: RefCell::new(None),
            timer: RefCell::new(None),
        });
        this.update_clock();
        this.root.connect_map(glib::clone!(
            #[weak]
            this,
            move |_| this.schedule_clock()
        ));
        this.root.connect_unmap(glib::clone!(
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
        if !self.root.is_mapped() {
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
        self.battery.set_visible(value.is_some());
        if let Some(b) = value {
            self.percent.set_text(&b.percent.to_string());
            self.charging.set_visible(b.charging);
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
