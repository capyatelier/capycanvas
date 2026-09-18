//! Native editor for a shared, transient SDR appearance draft.
use crate::{number_control::NumberControl, workspace::Workspace};
use adw::prelude::*;
use gtk::glib;
use layer_core::color::hdr::SdrRendition;
use layer_ui::NumericControl;
use std::{
    cell::Cell,
    rc::{Rc, Weak},
    time::Duration,
};

struct Preview {
    workspace: Weak<Workspace>,
    window: adw::Window,
}
impl Drop for Preview {
    fn drop(&mut self) {
        self.window.destroy();
        if let Some(w) = self.workspace.upgrade() {
            let result = w
                .gpu
                .borrow_mut()
                .as_mut()
                .map(|g| g.session.preview_sdr_appearance(None));
            if let Some(result) = result {
                w.changed(result);
            }
        }
    }
}

pub(crate) async fn configure(w: &Rc<Workspace>) -> Result<(), String> {
    let (epoch, revision, original) = {
        let gpu = w.gpu.borrow();
        let s = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        s.require_document_idle()?;
        let d = s.engine().document();
        if !d.color.depth.is_float() {
            return Err("SDR appearance requires HDR artwork".into());
        }
        (s.state().document_file.epoch, d.revision, d.sdr_rendition)
    };
    // A native utility window keeps the canvas visible without an alert scrim.
    let window = adw::Window::builder()
        .title("SDR Appearance")
        .transient_for(&w.window)
        .modal(true)
        .destroy_with_parent(true)
        .default_width(360)
        .resizable(false)
        .build();
    window.set_widget_name("sdr-appearance-window");
    let guard = Preview {
        workspace: Rc::downgrade(w),
        window: window.clone(),
    };
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    let cancel = gtk::Button::with_label("Cancel");
    cancel.set_widget_name("sdr-appearance-cancel");
    let apply = gtk::Button::with_label("Apply");
    apply.set_widget_name("sdr-appearance-apply");
    apply.add_css_class("suggested-action");
    header.pack_start(&cancel);
    header.pack_end(&apply);
    toolbar.add_top_bar(&header);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 14);
    for side in [gtk::PositionType::Top, gtk::PositionType::Bottom] {
        if side == gtk::PositionType::Top {
            body.set_margin_top(18);
        } else {
            body.set_margin_bottom(18);
        }
    }
    body.set_margin_start(18);
    body.set_margin_end(18);
    let description = gtk::Label::builder()
        .label("Used for SDR viewing, export and print proofing.")
        .wrap(true)
        .xalign(0.)
        .build();
    description.add_css_class("dim-label");
    body.append(&description);
    let controls = [
        ("Exposure", "exposure", -12., 12., 0.1, 2, "EV", 1.),
        ("Contrast", "contrast", 0.25, 4., 0.01, 2, "%", 100.),
        ("Highlights", "highlights", -100., 100., 1., 0, "%", 1.),
    ]
    .map(|(label, name, min, max, step, digits, unit, scale)| {
        let mut spec = NumericControl::number(min, max, step, digits);
        spec.scale = scale;
        spec.unit = unit.into();
        let control = NumberControl::new(spec, label, "");
        control.set_widget_name(&format!("sdr-appearance-{name}"));
        body.append(&control);
        control
    });
    for (control, value) in
        controls
            .iter()
            .zip([original.exposure, original.contrast, original.highlights()])
    {
        control.set_value(f64::from(value));
    }
    let compare = gtk::CheckButton::with_label("Compare saved appearance");
    compare.set_widget_name("sdr-appearance-compare");
    body.append(&compare);
    let reset = gtk::Button::with_label("Reset");
    reset.set_halign(gtk::Align::Start);
    reset.set_widget_name("sdr-appearance-reset");
    body.append(&reset);
    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    error.add_css_class("error");
    body.append(&error);
    let draft = Rc::new(Cell::new(original));
    let refresh: Rc<dyn Fn()> = Rc::new({
        let controls = controls.each_ref().map(|c| c.downgrade());
        let draft = draft.clone();
        glib::clone!(
            #[weak]
            w,
            #[weak]
            compare,
            #[weak]
            apply,
            #[weak]
            error,
            move || {
                let Some(controls) = controls
                    .iter()
                    .map(|c| c.upgrade())
                    .collect::<Option<Vec<_>>>()
                else {
                    return;
                };
                let result = SdrRendition::from_appearance(
                    controls[0].value() as f32,
                    controls[1].value() as f32,
                    controls[2].value() as f32,
                )
                .map_err(str::to_string)
                .and_then(|recipe| {
                    draft.set(recipe);
                    let mut gpu = w.gpu.borrow_mut();
                    let s = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
                    if s.state().document_file.epoch != epoch
                        || s.engine().document().revision != revision
                    {
                        return Err("The drawing changed; reopen SDR Appearance.".into());
                    }
                    s.preview_sdr_appearance(Some(if compare.is_active() {
                        original
                    } else {
                        recipe
                    }))
                });
                apply.set_sensitive(result.is_ok());
                error.set_visible(result.is_err());
                error.set_label(result.as_ref().err().map_or("", String::as_str));
                if result.is_ok() {
                    w.changed(result);
                }
            }
        )
    });
    for control in &controls {
        let refresh = refresh.clone();
        control.connect_value_changed(move |_| refresh());
    }
    compare.connect_toggled({
        let refresh = refresh.clone();
        move |_| refresh()
    });
    reset.connect_clicked({
        let controls = controls.clone();
        let refresh = refresh.clone();
        let compare = compare.clone();
        move |_| {
            let d = SdrRendition::default();
            for (control, value) in controls
                .iter()
                .zip([d.exposure, d.contrast, d.highlights()])
            {
                control.set_value(f64::from(value));
            }
            compare.set_active(false);
            refresh();
        }
    });
    let accepted = Rc::new(Cell::new(false));
    apply.connect_clicked(glib::clone!(
        #[weak]
        window,
        #[strong]
        accepted,
        move |_| {
            accepted.set(true);
            window.close();
        }
    ));
    cancel.connect_clicked(glib::clone!(
        #[weak]
        window,
        move |_| window.close()
    ));
    let key = gtk::EventControllerKey::new();
    key.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(key);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(480)
        .child(&body)
        .build();
    toolbar.set_content(Some(&scroll));
    window.set_content(Some(&toolbar));
    window.set_default_widget(Some(&apply));
    refresh();
    window.present();
    controls[0].grab_focus();
    while window.is_visible() {
        glib::timeout_future(Duration::from_millis(20)).await;
    }
    drop(guard);
    if accepted.get() {
        let result = {
            let mut gpu = w.gpu.borrow_mut();
            let s = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
            if s.state().document_file.epoch != epoch || s.engine().document().revision != revision
            {
                return Err("The drawing changed; reopen SDR Appearance.".into());
            }
            s.set_sdr_rendition(draft.get())
        };
        w.changed(result);
    }
    Ok(())
}

pub(crate) fn display_details(w: &Rc<Workspace>) {
    let text = {
        let gpu = w.gpu.borrow();
        let Some(gpu) = gpu.as_ref() else {
            return;
        };
        let headroom = gpu.session.engine().backend().display_headroom;
        let state = if headroom > 1. {
            format!("HDR presentation available · {headroom:.1}× headroom")
        } else {
            "Showing the saved SDR appearance. HDR presentation is unavailable on the current canvas surface. If this drawing was converted from SDR, save and reopen it to retry HDR presentation.".into()
        };
        format!(
            "{state}\n\nReference white: 203 cd/m². The HDR master is preserved.\n\n{}",
            w.display_description()
        )
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Display Details")
        .body(&text)
        .build();
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    dialog.present(Some(&w.window));
}
