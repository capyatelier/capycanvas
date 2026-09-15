use crate::workspace::Workspace;
use adw::prelude::*;
use layer_core::color::{ColorProfile, source::SourceChannels};
use std::rc::Rc;

pub(crate) async fn show(w: &Rc<Workspace>) -> Result<bool, String> {
    let project = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .capture_project_recovery()?;
    let document = project.document;
    let color = document.color;
    let extent = [document.width, document.height];
    let resolution = document.resolution;
    // Profiles can contain substantial metadata. Parse once off the GTK owner.
    let sources = gtk::gio::spawn_blocking(move || {
        document
            .layers
            .iter()
            .filter_map(|l| l.source.as_ref().filter(|s| s.is_original()).map(|s| (l.name.clone(), s)))
            .map(|(name, source)| {
                let interpretation = &source.interpretation;
                let profile = layer_color::profile_description(&interpretation.profile)?;
                let channels = match interpretation.channels {
                    SourceChannels::Rgb | SourceChannels::Rgba => "RGB",
                    SourceChannels::Gray | SourceChannels::GrayAlpha => "Grayscale",
                    SourceChannels::Cmyk => "CMYK",
                };
                let tag = if interpretation.profile_assumed {
                    "Profile assumed"
                } else {
                    "Source profile"
                };
                let retained = if matches!(interpretation.profile, ColorProfile::Icc(_)) {
                    "Original samples and embedded ICC retained."
                } else {
                    "Original samples and color interpretation retained."
                };
                Ok::<_, String>((
                    name,
                    format!(
                        "{} × {} px · {}-bit {channels}\n{tag}: {profile}\n{retained}",
                        source.extent[0],
                        source.extent[1],
                        interpretation.depth.bits()
                    ),
                ))
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|_| "Cannot read source color details")??;
    let group = adw::PreferencesGroup::new();
    let add = |title: &str, subtitle: &str| {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .subtitle_selectable(true)
            .build();
        row.set_use_markup(false);
        group.add(&row);
    };
    add(
        "Canvas size",
        &format!("{} × {} pixels", extent[0], extent[1]),
    );
    add("Working color space", color.space.name());
    add("Resolution metadata", &resolution.map_or_else(|| "Not specified".into(), |r| {
        let [x, y] = r.pixels_per_inch();
        format!("{x:.2} × {y:.2} pixels per inch")
    }));
    add(
        "Bit depth",
        &format!("{}-bit integer SDR", color.depth.bits()),
    );
    if !sources.is_empty() {
        add(
            "Source preservation",
            "Edits use the working color space. Retained originals keep their source profiles and depth; Save creates an editable master.",
        );
        for (name, description) in sources {
            add(&name, &description);
        }
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(450)
        .child(&group)
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("Document Properties")
        .content_width(440)
        .extra_child(&scroll)
        .build();
    dialog.set_widget_name("document-properties-dialog");
    dialog.add_response("done", "Done");
    dialog.set_close_response("done");
    dialog.set_default_response(Some("done"));
    crate::alert::choose(dialog, &w.window).await;
    Ok(true)
}
