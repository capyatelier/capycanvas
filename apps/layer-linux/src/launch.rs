use gtk::prelude::*;
use std::{cell::{Cell, RefCell}, rc::Rc};

pub(crate) struct LaunchSplash {
    pub root: gtk::Revealer,
    label: gtk::Label,
    paint: Rc<RefCell<Option<gtk::TickCallbackId>>>,
}
impl LaunchSplash {
    pub fn new(localization: &layer_ui::Localizer, capy: layer_ui::ZenIcon) -> Self {
        let root = gtk::Revealer::new();
        root.set_widget_name("launch-splash");
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        root.set_transition_duration(0);
        root.set_opacity(254. / 255.);
        let background = gtk::Overlay::new();
        background.add_css_class("launch-splash");
        let sizing = gtk::DrawingArea::new();
        background.set_child(Some(&sizing));
        let mark = capy.launch_mark();
        let icon = crate::icons::image(&format!("layer-{}-symbolic", mark.icon));
        icon.set_widget_name("launch-mark");
        icon.set_pixel_size(0);
        icon.set_halign(gtk::Align::Center);
        icon.set_valign(gtk::Align::Center);
        background.add_overlay(&icon);
        let copy = layer_ui::bootstrap_view(localization);
        let label = gtk::Label::new(Some(&copy.launch_caption));
        label.add_css_class("launch-caption");
        let attributes = gtk::pango::AttrList::new();
        attributes.insert(gtk::pango::AttrSize::new_size_absolute((copy.launch_caption_size * gtk::pango::SCALE as f32).round() as i32));
        label.set_attributes(Some(&attributes));
        let dark = adw::StyleManager::default().is_dark();
        let theme = if dark { layer_ui::Theme::Dark } else { layer_ui::Theme::Light };
        let palette = layer_ui::Settings::default().palette(theme, layer_ui::Platform::Gtk, None);
        label.set_wrap(true);
        label.set_justify(gtk::Justification::Center);
        label.set_max_width_chars(48);
        let caption = gtk::Revealer::new();
        caption.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        caption.set_transition_duration(150);
        caption.set_child(Some(&label));
        caption.set_halign(gtk::Align::Fill);
        caption.set_valign(gtk::Align::Start);
        caption.set_margin_start(layer_ui::LAUNCH_CAPTION_GAP as i32);
        caption.set_margin_end(layer_ui::LAUNCH_CAPTION_GAP as i32);
        background.add_overlay(&caption);
        let image = icon.downgrade();
        let text = caption.downgrade();
        let ink = label.downgrade();
        sizing.connect_resize(move |_, width, height| {
            let size = layer_ui::launch_mark_size([width as f32, height as f32]);
            if let Some(image) = image.upgrade() { image.set_pixel_size(size.round() as i32); }
            if let Some(text) = text.upgrade() {
                let padding = ink.upgrade().map_or(0, |label| label.layout().pixel_extents().0.y());
                text.set_margin_top((height as f32 * 0.5 + size * (mark.ink_bottom - 0.5)
                    + layer_ui::LAUNCH_CAPTION_GAP).round() as i32 - padding);
            }
        });
        root.set_child(Some(&background));
        root.set_reveal_child(true);
        let weak = caption.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(layer_ui::LAUNCH_CAPTION_DELAY_MS), move || {
            if let Some(caption) = weak.upgrade() { caption.set_reveal_child(true); }
        });
        let splash = Self {root, label, paint: Rc::new(RefCell::new(None))};
        splash.set_palette(&palette);
        splash
    }
    pub fn set_palette(&self, palette: &layer_ui::ThemePalette) {
        let [r, g, b] = palette.settings_secondary.0.map(|channel| u16::from(channel) * 257);
        let attributes = self.label.attributes().unwrap();
        attributes.change(gtk::pango::AttrColor::new_foreground(r, g, b));
        attributes.change(gtk::pango::AttrInt::new_foreground_alpha(u16::MAX));
        self.label.set_attributes(Some(&attributes));
    }
    pub fn update(&self, view: layer_ui::LaunchView, localization: &layer_ui::Localizer) {
        if !view.visible && !self.root.reveals_child() { return; }
        self.root.set_can_target(view.visible);
        if view.visible {
            self.cancel_paint();
            self.label.set_text(&layer_ui::bootstrap_view(localization).launch_caption);
        }
        if view.visible {
            self.root.set_transition_duration(0);
            self.root.set_reveal_child(true);
        } else if !view.finished {
            self.bypass();
        } else if self.paint.borrow().is_none() {
            self.root.set_transition_duration(layer_ui::LAUNCH_REVEAL_MS as u32);
            let pending = Rc::downgrade(&self.paint);
            let frame = Cell::new(None);
            let callback = self.root.add_tick_callback(move |root, clock| {
                let Some(pending) = pending.upgrade() else { return glib::ControlFlow::Break; };
                if let Some(frame) = frame.get() {
                    if clock.frame_counter() <= frame { return glib::ControlFlow::Continue; }
                    pending.borrow_mut().take();
                    root.set_reveal_child(false);
                    return glib::ControlFlow::Break;
                }
                frame.set(Some(clock.frame_counter()));
                root.queue_draw();
                glib::ControlFlow::Continue
            });
            *self.paint.borrow_mut() = Some(callback);
        }
    }
    fn cancel_paint(&self) {
        if let Some(callback) = self.paint.borrow_mut().take() { callback.remove(); }
    }
    pub fn bypass(&self) {
        self.cancel_paint();
        self.root.set_transition_duration(0);
        self.root.set_reveal_child(false);
        self.root.set_can_target(false);
    }
    #[cfg(test)]
    pub fn covering(&self) -> bool { self.root.reveals_child() || self.root.is_child_revealed() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::tests::{native_test_app, pump, until};

    #[test]
    fn launch_reads_the_language_and_capy_from_one_saved_text() {
        let saved = serde_json::to_string(&layer_ui::Settings {
            language: layer_ui::LanguagePreference::Explicit(layer_ui::UiLanguage::Japanese),
            zen_icon: layer_ui::ZenIcon::Sleeping,
            ..layer_ui::Settings::default()
        }).unwrap();
        let launch = crate::LaunchPreferences::read(&saved, &["fr_FR"]);
        assert_eq!(launch.localization.language(), layer_ui::UiLanguage::Japanese);
        assert_eq!(launch.capy, layer_ui::ZenIcon::Sleeping);
        let unread = crate::LaunchPreferences::read("{", &["en_US"]);
        assert_eq!(unread.localization.language(), layer_ui::UiLanguage::English);
        assert_eq!(unread.capy, layer_ui::ZenIcon::LookingUp);
    }

    #[test]
    #[ignore = "private Wayland display: splash caption, reveal and error bypass"]
    fn native_launch_splash_timing_and_bypass() {
        let app = native_test_app("art.capycanvas.LaunchSplash");
        adw::StyleManager::default().set_color_scheme(if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|theme| theme == "dark") {
            adw::ColorScheme::ForceDark
        } else { adw::ColorScheme::ForceLight });
        crate::icons::register();
        let localization = layer_ui::Localizer::shared(layer_ui::UiLanguage::English);
        for (size, prefix, capy) in [([400, 300], "small", layer_ui::ZenIcon::LookingUp), ([1300, 900], "large", layer_ui::ZenIcon::Sleeping)] {
        let started = std::time::Instant::now();
        let mark = capy.launch_mark();
        let splash = LaunchSplash::new(&localization, capy);
        let window = gtk::ApplicationWindow::new(&*app);
        window.set_default_size(size[0], size[1]);
        window.set_child(Some(&splash.root));
        window.present();
        pump(100);
        let caption = splash.label.parent().unwrap().downcast::<gtk::Revealer>().unwrap();
        let capture = |name: &str| {
            if let Some(directory) = std::env::var_os("LAYER_TEST_ARTIFACTS") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                crate::snapshot_window(&window, 1.).save_to_png(directory.join(format!("{prefix}-{name}"))).unwrap();
            }
        };
        assert!(splash.covering());
        assert!((splash.root.opacity() - 254. / 255.).abs() < 1e-9);
        let icon = caption.prev_sibling().unwrap().downcast::<gtk::Image>().unwrap();
        assert_eq!(icon.paintable(), crate::icons::paintable(&format!("layer-{}-symbolic", mark.icon), None).map(|svg| svg.upcast()),
            "the splash draws the chosen capy");
        let geometry = || {
            let bounds = icon.compute_bounds(&window).unwrap();
            let surface = splash.root.compute_bounds(&window).unwrap();
            assert!((bounds.x() + bounds.width() * 0.5 - (surface.x() + surface.width() * 0.5)).abs() <= 1.);
            assert!((bounds.y() + bounds.height() * 0.5 - (surface.y() + surface.height() * 0.5)).abs() <= 1.);
            assert_eq!(icon.pixel_size(), layer_ui::launch_mark_size([surface.width(), surface.height()]).round() as i32);
            assert!((bounds.width() - icon.pixel_size() as f32).abs() <= 1.);
            assert!((bounds.height() - icon.pixel_size() as f32).abs() <= 1.);
            if caption.reveals_child() {
                let text = caption.compute_bounds(&window).unwrap();
                let ink = splash.label.layout().pixel_extents().0;
                let label = splash.label.compute_bounds(&window).unwrap();
                assert!((label.y() + ink.y() as f32 - (bounds.y() + bounds.height() * mark.ink_bottom)
                    - layer_ui::LAUNCH_CAPTION_GAP).abs() <= 1.);
                assert!(text.y() + text.height() <= surface.y() + surface.height());
            }
            bounds
        };
        let bounds = geometry();
        if prefix == "small" { assert!(icon.pixel_size() <= 120); }
        else { assert_eq!(icon.pixel_size(), 252); }
        assert!(!caption.reveals_child());
        let delay = std::time::Duration::from_millis(layer_ui::LAUNCH_CAPTION_DELAY_MS);
        let before = delay.saturating_sub(std::time::Duration::from_millis(100));
        pump(before.saturating_sub(started.elapsed()).as_millis() as u64);
        assert!(started.elapsed() < delay);
        assert!(!caption.reveals_child(), "only the mark appears before the shared caption deadline");
        capture("launch-before-caption.png");
        pump(delay.saturating_sub(started.elapsed()).as_millis() as u64 + 200);
        assert!(caption.reveals_child());
        assert_eq!(icon.compute_bounds(&window).unwrap(), bounds, "caption reveal keeps the mark centered");
        geometry();
        capture("launch-after-caption.png");
        let view = layer_ui::LaunchView {visible: false, finished: true, frame: None};
        splash.update(view, &localization);
        assert!(splash.root.reveals_child(), "readiness does not reveal an unpainted editor");
        assert!(!splash.root.can_target(), "a finished launch passes input before its cover fades");
        assert!(splash.paint.borrow().is_some());
        splash.update(layer_ui::LaunchView {visible: true, finished: false, ..view}, &localization);
        assert!(splash.paint.borrow().is_none());
        pump(layer_ui::LAUNCH_REVEAL_MS + 100);
        assert!(splash.root.reveals_child() && splash.root.can_target(), "renewed loading cancels the pending reveal");
        splash.update(view, &localization);
        assert_eq!(splash.root.transition_duration(), layer_ui::LAUNCH_REVEAL_MS as u32);
        assert!(splash.covering());
        until(|| !splash.root.reveals_child(), "covered frame precedes reveal");
        assert!(!splash.root.can_target());
        pump(layer_ui::LAUNCH_REVEAL_MS / 2);
        assert!(splash.covering(), "the shared reveal remains in progress halfway through");
        capture("launch-reveal-midpoint.png");
        pump(layer_ui::LAUNCH_REVEAL_MS / 2 + 100);
        assert!(!splash.covering());
        splash.update(layer_ui::LaunchView {visible: true, finished: false, ..view}, &localization);
        assert!(splash.covering());
        splash.update(view, &localization);
        assert!(splash.paint.borrow().is_some());
        splash.update(layer_ui::LaunchView {finished: false, ..view}, &localization);
        assert!(splash.paint.borrow().is_none());
        assert_eq!(splash.root.transition_duration(), 0);
        pump(10);
        assert!(!splash.covering());
        window.destroy();
        pump(20);
        }
    }
}
