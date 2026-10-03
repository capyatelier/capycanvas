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
        let mut language = self.language.clone();
        while let Some(root) = window {
            if let Some(active) = unsafe { root.data::<gtk::pango::Language>("capy-text-language") } {
                language = unsafe { active.as_ref() }.clone();
            }
            if root.application().as_ref() == Some(app.upcast_ref()) {
                let context = widget.pango_context();
                if context.language().as_ref() != Some(&language) {
                    context.set_language(Some(&language));
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

pub(crate) fn update(window: &impl IsA<gtk::Window>, localization: &layer_ui::Localizer) {
    let language = gtk::pango::Language::from_string(localization.language().tag());
    unsafe { window.as_ref().set_data("capy-text-language", language.clone()); }
    for window in owned_windows(window) {
    unsafe { window.set_data("capy-text-language", language.clone()); }
    visit(window.upcast_ref(), &mut |widget| {
        let context = widget.pango_context();
        if context.language().as_ref() != Some(&language) {
            context.set_language(Some(&language));
            widget.queue_resize();
        }
    });
    }
}

pub(crate) fn owned_windows(window: &impl IsA<gtk::Window>) -> Vec<gtk::Window> {
    gtk::Window::list_toplevels().into_iter()
        .filter_map(|object| object.downcast::<gtk::Window>().ok())
        .filter(|candidate| {
            let mut parent = Some(candidate.clone());
            while let Some(root) = parent {
                if root == *window.as_ref() { return true; }
                parent = root.transient_for();
            }
            false
        }).collect()
}

pub(crate) fn visit(widget: &gtk::Widget, callback: &mut impl FnMut(&gtk::Widget)) {
    callback(widget);
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        visit(&widget, callback);
    }
}
