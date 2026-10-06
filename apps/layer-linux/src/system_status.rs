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
    pub(crate) clock: gtk::Label,
    pub(crate) battery: gtk::Box,
    percent: gtk::Label,
    drawing: gtk::DrawingArea,
    value: Rc<Cell<Option<Battery>>>,
    fullscreen: Cell<bool>,
    components: Cell<Option<[bool; 2]>>,
    header_size: Cell<Option<layer_ui::HeaderSize>>,
    settings: Option<gio::Settings>,
    clock_proxy: RefCell<Option<gio::DBusProxy>>,
    portal_clock: Cell<Option<bool>>,
    clock_revision: Cell<u64>,
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
        Self::build(true)
    }
    #[cfg(test)]
    pub(crate) fn simulated_power() -> Rc<Self> {
        Self::build(false)
    }
    fn build(observe_power: bool) -> Rc<Self> {
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
            components: Cell::new(None),
            header_size: Cell::new(None),
            settings,
            clock_proxy: RefCell::new(None),
            portal_clock: Cell::new(None),
            clock_revision: Cell::new(0),
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
        this.observe_clock();
        // Synthetic battery tests must not race the host's real UPower reply.
        if !observe_power {
            return this;
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
    fn observe_clock(self: &Rc<Self>) {
        glib::MainContext::default().spawn_local(glib::clone!(#[weak(rename_to = this)] self, async move {
            let Ok(proxy) = gio::DBusProxy::for_bus_future(
                gio::BusType::Session,
                gio::DBusProxyFlags::DO_NOT_AUTO_START | gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
                None, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
            ).await else { return };
            proxy.connect_local("g-signal", false, glib::clone!(#[weak] this, #[upgrade_or] None, move |args| {
                if args[2].get::<String>().ok().as_deref() == Some("SettingChanged")
                    && let Ok(parameters) = args[3].get::<glib::Variant>()
                    && let Some((namespace, key, value)) = parameters.get::<(String, String, glib::Variant)>()
                    && namespace == "org.gnome.desktop.interface" && key == "clock-format"
                {
                    this.set_portal_clock(Some(&value));
                }
                None
            }));
            proxy.connect_notify_local(Some("g-name-owner"), glib::clone!(#[weak] this, move |proxy, _| {
                this.read_portal_clock(proxy);
            }));
            this.read_portal_clock(&proxy);
            *this.clock_proxy.borrow_mut() = Some(proxy);
        }));
    }
    fn read_portal_clock(self: &Rc<Self>, proxy: &gio::DBusProxy) {
        self.set_portal_clock(None);
        let Some(owner) = proxy.name_owner() else { return; };
        let revision = self.clock_revision.get();
        let proxy = proxy.clone();
        glib::MainContext::default().spawn_local(glib::clone!(#[weak(rename_to = this)] self, async move {
            let result = proxy.call_future("ReadOne",
                Some(&("org.gnome.desktop.interface", "clock-format").to_variant()),
                gio::DBusCallFlags::NONE, 3000,
            ).await;
            let value = result.ok().and_then(|reply| reply.get::<(glib::Variant,)>().map(|(value,)| value));
            if this.clock_revision.get() == revision && proxy.name_owner().as_ref() == Some(&owner) {
                this.set_portal_clock(value.as_ref());
            }
        }));
    }
    fn set_portal_clock(&self, value: Option<&glib::Variant>) {
        self.clock_revision.set(self.clock_revision.get().wrapping_add(1));
        self.portal_clock.set(value.and_then(|value| match value.str()? {
            "12h" => Some(true), "24h" => Some(false), _ => None,
        }));
        self.update_clock();
    }
    pub fn set_visibility(&self, fullscreen: bool) {
        self.fullscreen.set(fullscreen);
        self.update_visibility();
    }
    pub fn set_components(&self, clock: bool, battery: bool) {
        self.components.set(Some([clock, battery]));
        self.update_visibility();
    }
    pub fn set_header_size(&self, size: layer_ui::HeaderSize) {
        if self.header_size.replace(Some(size)) != Some(size) {
            self.apply_header_size();
        }
    }
    fn apply_header_size(&self) {
        let Some(size) = self.header_size.get() else {
            return;
        };
        let scale = size.icon() as f32 / 20.;
        self.battery
            .set_size_request(size.tile() as i32, size.tile() as i32);
        self.drawing.set_content_width((26. * scale).round() as i32);
        self.drawing
            .set_content_height((14. * scale).round() as i32);
        let battery = self.value.get();
        let font = if battery.is_some_and(|b| b.percent == 100) {
            10.
        } else {
            11.
        };
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrSize::new_size_absolute(
            (font * scale * gtk::pango::SCALE as f32).round() as i32,
        ));
        self.percent.set_attributes(Some(&attributes));
        self.percent.set_margin_end(
            ((if battery.is_some_and(|b| b.charging) {
                5.
            } else {
                4.
            }) * scale)
                .round() as i32,
        );
    }
    fn update_visibility(&self) {
        let [clock, battery] = self
            .components
            .get()
            .map(|components| components.map(|present| present && self.fullscreen.get()))
            .unwrap_or([self.fullscreen.get(); 2]);
        let battery = battery && self.value.get().is_some();
        self.clock.set_visible(clock);
        self.battery.set_visible(battery);
        self.root.set_visible(clock || battery);
    }
    fn update_style(&self, style: &adw::StyleManager) {
        if style.is_dark() {
            self.root.remove_css_class("light");
            self.battery.remove_css_class("light");
        } else {
            self.root.add_css_class("light");
            self.battery.add_css_class("light");
        }
        self.drawing.queue_draw();
    }
    fn update_clock(&self) {
        let preference = self.settings.as_ref().map(|s| s.string("clock-format"));
        // GTK initializes the process locale; nl_langinfo reflects LC_TIME.
        let locale =
            unsafe { std::ffi::CStr::from_ptr(libc::nl_langinfo(libc::T_FMT)) }.to_string_lossy();
        let twelve = self.portal_clock.get().unwrap_or_else(|| twelve_hour(preference.as_deref(), &locale));
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
            self.apply_header_size();
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

    #[test]
    #[ignore = "private Wayland display and D-Bus session"]
    fn native_clock_portal_preferences() {
        assert!(std::env::var("WAYLAND_DISPLAY").is_ok_and(|display| display.starts_with("layer-bench-")));
        assert_eq!(std::env::var("GSETTINGS_BACKEND").as_deref(), Ok("memory"));
        let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
        let info = gio::DBusNodeInfo::for_xml("<node><interface name='org.freedesktop.portal.Settings'><method name='ReadOne'><arg type='s' direction='in'/><arg type='s' direction='in'/><arg type='v' direction='out'/></method><signal name='SettingChanged'><arg type='s'/><arg type='s'/><arg type='v'/></signal></interface></node>").unwrap();
        let reply = Rc::new(RefCell::new("24h".to_variant()));
        let calls = Rc::new(Cell::new(0));
        let deferred = Rc::new(Cell::new(false));
        let pending = Rc::new(RefCell::new(None::<gio::DBusMethodInvocation>));
        let registration = bus.register_object("/org/freedesktop/portal/desktop", &info.lookup_interface("org.freedesktop.portal.Settings").unwrap())
            .method_call(glib::clone!(#[strong] reply, #[strong] calls, #[strong] deferred, #[strong] pending,
                move |_, _, _, _, method, parameters, invocation| {
                    assert_eq!(method, "ReadOne");
                    assert_eq!(parameters.get::<(String,String)>().unwrap(), ("org.gnome.desktop.interface".into(), "clock-format".into()));
                    calls.set(calls.get() + 1);
                    if deferred.get() { *pending.borrow_mut() = Some(invocation); }
                    else { invocation.return_value(Some(&(reply.borrow().clone(),).to_variant())); }
                })).build().unwrap();
        let own = |acquire| {
            let parameters = if acquire { ("org.freedesktop.portal.Desktop", 4u32).to_variant() }
                else { ("org.freedesktop.portal.Desktop",).to_variant() };
            let answer = bus.call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus",
                if acquire { "RequestName" } else { "ReleaseName" }, Some(&parameters), None,
                gio::DBusCallFlags::NONE, 3000, gio::Cancellable::NONE).unwrap();
            assert_eq!(answer.get::<(u32,)>().unwrap().0, 1);
        };
        own(true);
        let _app = crate::workspace::tests::native_test_app("art.capycanvas.ClockPortalTest");
        let native = SystemStatus::simulated_power();
        let settings = native.settings.as_ref().expect("GNOME clock schema");
        assert!(settings.property::<gio::SettingsBackend>("backend").type_().name().contains("Memory"));
        settings.set_string("clock-format", "12h").unwrap();
        let clock = |twelve: bool| native.clock.text().contains(' ') == twelve;
        crate::workspace::tests::until(|| native.portal_clock.get() == Some(false) && clock(false), "initial portal preference overrides private GSettings");
        let changed = |namespace: &str, key: &str, value: glib::Variant| {
            bus.emit_signal(None, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings", "SettingChanged",
                Some(&(namespace, key, value).to_variant())).unwrap();
        };
        for (value, twelve) in [("12h",true),("24h",false)] {
            changed("org.gnome.desktop.interface", "clock-format", value.to_variant());
            crate::workspace::tests::until(|| native.portal_clock.get() == Some(twelve) && clock(twelve), "live host clock preference");
        }
        changed("unrelated", "clock-format", "12h".to_variant());
        changed("org.gnome.desktop.interface", "unrelated", "12h".to_variant());
        crate::workspace::tests::pump(30);
        assert_eq!(native.portal_clock.get(), Some(false));
        for value in ["unknown".to_variant(), 42u32.to_variant()] {
            changed("org.gnome.desktop.interface", "clock-format", value);
            crate::workspace::tests::until(|| native.portal_clock.get().is_none() && clock(true), "invalid portal value uses private GSettings");
            changed("org.gnome.desktop.interface", "clock-format", "24h".to_variant());
            crate::workspace::tests::until(|| native.portal_clock.get() == Some(false) && clock(false), "valid portal preference returns");
        }
        own(false);
        crate::workspace::tests::until(|| native.portal_clock.get().is_none() && clock(true), "portal owner loss restores fallback");
        settings.set_string("clock-format", "24h").unwrap();
        crate::workspace::tests::until(|| clock(false), "fallback observes private GSettings changes");
        *reply.borrow_mut() = "12h".to_variant();
        own(true);
        crate::workspace::tests::until(|| native.portal_clock.get() == Some(true) && clock(true), "portal restart reloads host preference");
        own(false);
        crate::workspace::tests::until(|| native.portal_clock.get().is_none(), "second portal owner loss");
        deferred.set(true);
        let before = calls.get();
        own(true);
        crate::workspace::tests::until(|| calls.get() > before && pending.borrow().is_some(), "deferred initial portal read");
        changed("org.gnome.desktop.interface", "clock-format", "24h".to_variant());
        crate::workspace::tests::until(|| native.portal_clock.get() == Some(false) && clock(false), "signal supersedes pending read");
        pending.borrow_mut().take().unwrap().return_value(Some(&("12h".to_variant(),).to_variant()));
        crate::workspace::tests::pump(50);
        assert_eq!(native.portal_clock.get(), Some(false), "stale initial reply cannot replace a newer signal");
        assert!(clock(false));
        own(false);
        crate::workspace::tests::until(|| native.portal_clock.get().is_none(), "portal loss after signal");
        let before = calls.get();
        own(true);
        crate::workspace::tests::until(|| calls.get() > before && pending.borrow().is_some(), "deferred read before owner loss");
        own(false);
        crate::workspace::tests::until(|| native.portal_clock.get().is_none(), "owner loss invalidates pending read");
        pending.borrow_mut().take().unwrap().return_value(Some(&("12h".to_variant(),).to_variant()));
        crate::workspace::tests::pump(50);
        assert!(native.portal_clock.get().is_none(), "stale reply cannot restore a lost portal preference");
        assert!(clock(false));
        bus.unregister_object(registration).unwrap();
    }
}
