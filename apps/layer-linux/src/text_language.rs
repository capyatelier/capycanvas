use adw::prelude::*;
use gtk::glib::{self, translate::*};

struct LanguageHook {
    signal: u32,
    id: libc::c_ulong,
}
impl Drop for LanguageHook {
    fn drop(&mut self) {
        unsafe { glib::gobject_ffi::g_signal_remove_emission_hook(self.signal, self.id); }
    }
}
struct TextLanguage {
    app: glib::WeakRef<adw::Application>,
    language: gtk::pango::Language,
}
impl TextLanguage {
    fn apply(&self, widget: &gtk::Widget) {
        let Some(app) = self.app.upgrade() else { return; };
        let mut window = widget.root().and_downcast::<gtk::Window>();
        while let Some(root) = window {
            if root.application().as_ref() == Some(app.upcast_ref()) {
                let context = widget.pango_context();
                if context.language().as_ref() != Some(&self.language) {
                    context.set_language(Some(&self.language));
                    widget.queue_resize();
                }
                return;
            }
            window = root.transient_for();
        }
    }
}
unsafe extern "C" fn apply_language(
    _: *mut glib::gobject_ffi::GSignalInvocationHint,
    count: u32,
    values: *const glib::gobject_ffi::GValue,
    data: glib::ffi::gpointer,
) -> glib::ffi::gboolean {
    if count > 0 {
        let widget: glib::translate::Borrowed<gtk::Widget> = unsafe {
            from_glib_borrow(glib::gobject_ffi::g_value_get_object(values) as *mut gtk::ffi::GtkWidget)
        };
        unsafe { &*(data as *const TextLanguage) }.apply(&widget);
    }
    1
}
unsafe extern "C" fn drop_language(data: glib::ffi::gpointer) {
    drop(unsafe { Box::from_raw(data as *mut TextLanguage) });
}
pub fn install(app: &adw::Application, localization: &layer_ui::Localizer) {
    let _class = glib::Class::<gtk::Widget>::from_type(gtk::Widget::static_type()).unwrap();
    let mut hooks = Vec::new();
    for name in [c"realize", c"map"] {
        let data = Box::new(TextLanguage {
            app: app.downgrade(),
            language: gtk::pango::Language::from_string(localization.language().tag()),
        });
        let signal = unsafe { glib::gobject_ffi::g_signal_lookup(name.as_ptr(), gtk::Widget::static_type().into_glib()) };
        let id = unsafe { glib::gobject_ffi::g_signal_add_emission_hook(signal, 0, Some(apply_language), Box::into_raw(data).cast(), Some(drop_language)) };
        assert_ne!(id, 0);
        hooks.push(LanguageHook { signal, id });
    }
    let hooks = std::cell::RefCell::new(Some(hooks));
    app.connect_shutdown(move |_| { hooks.borrow_mut().take(); });
}
