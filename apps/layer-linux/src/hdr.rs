//! Native presentation of the shared authored SDR rendition transaction.
use crate::workspace::Workspace;
use adw::prelude::*;
use layer_core::color::hdr::SdrRendition;
use std::rc::Rc;

pub(crate) async fn configure(w: &Rc<Workspace>) -> Result<(), String> {
    let (epoch, revision, recipe) = {
        let gpu = w.gpu.borrow();
        let s = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        s.require_document_idle()?;
        let d = s.engine().document();
        if !d.color.depth.is_float() {
            return Err("SDR rendition requires HDR artwork".into());
        }
        (s.state().document_file.epoch, d.revision, d.sdr_rendition)
    };
    let group = adw::PreferencesGroup::new();
    let rows = [
        ("Exposure", -12., 12., 0.1, recipe.exposure),
        ("Contrast", 0.25, 4., 0.05, recipe.contrast),
        ("Highlight shoulder", 0.25, 0.95, 0.05, recipe.knee),
    ]
    .map(|(name, min, max, step, value)| {
        let row = adw::SpinRow::with_range(min, max, step);
        row.set_title(name);
        row.set_digits(2);
        row.set_value(f64::from(value));
        row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
        group.add(&row);
        row
    });
    for (row, name) in rows.iter().zip([
        "sdr-rendition-exposure",
        "sdr-rendition-contrast",
        "sdr-rendition-knee",
    ]) {
        row.set_widget_name(name);
    }
    let dialog=adw::AlertDialog::builder().heading("SDR Rendition")
        .body("These saved settings control mapped SDR viewing, SDR export and print proofing. HDR artwork stays at its original range. Undo restores the previous rendition.")
        .extra_child(&group).build();
    dialog.set_widget_name("sdr-rendition-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Apply")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("apply"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    if crate::alert::choose(dialog, &w.window).await != "apply" {
        return Ok(());
    }
    let recipe = SdrRendition {
        exposure: rows[0].value() as f32,
        contrast: rows[1].value() as f32,
        knee: rows[2].value() as f32,
    };
    let result = {
        let mut gpu = w.gpu.borrow_mut();
        let s = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
        if s.state().document_file.epoch != epoch || s.engine().document().revision != revision {
            return Err(
                "The drawing changed while configuring its rendition; reopen the settings".into(),
            );
        }
        s.set_sdr_rendition(recipe)
    };
    w.changed(result);
    Ok(())
}
