use crate::workspace::Workspace;
use adw::prelude::*;
use layer_core::color::source::SourceChannels;
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
            .filter_map(|l| {
                l.source
                    .as_ref()
                    .filter(|s| s.is_original())
                    .map(|s| (l.name.clone(), s))
            })
            .map(|(name, source)| {
                let interpretation = &source.interpretation;
                let profile = layer_color::profile_description(&interpretation.profile)?;
                let channels = match interpretation.channels {
                    SourceChannels::Rgb | SourceChannels::Rgba => "RGB",
                    SourceChannels::Gray | SourceChannels::GrayAlpha => "Grayscale",
                    SourceChannels::Cmyk => "CMYK",
                };
                let assumed = if interpretation.profile_assumed {
                    " (assumed)"
                } else {
                    ""
                };
                Ok::<_, String>((
                    name,
                    format!(
                        "{} × {} px · {}-bit {channels}\n{profile}{assumed}",
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
    let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
    let group = adw::PreferencesGroup::new();
    let add = |group: &adw::PreferencesGroup, title: &str, subtitle: &str| {
        let row = adw::ActionRow::builder()
            .title(title)
            .subtitle(subtitle)
            .subtitle_selectable(true)
            .build();
        row.set_use_markup(false);
        group.add(&row);
    };
    add(
        &group,
        "Canvas size",
        &format!("{} × {} px", extent[0], extent[1]),
    );
    add(&group, "Color space", color.space.name());
    add(
        &group,
        "Resolution",
        &resolution.map_or_else(
            || "Not specified".into(),
            |r| {
                let [x, y] = r.pixels_per_inch().map(|ppi| {
                    format!("{ppi:.2}")
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_owned()
                });
                if x == y {
                    format!("{x} ppi")
                } else {
                    format!("{x} × {y} ppi")
                }
            },
        ),
    );
    add(&group, "Bit depth", &format!("{}-bit", color.depth.bits()));
    body.append(&group);
    if !sources.is_empty() {
        let group = adw::PreferencesGroup::builder()
            .title("Source images")
            .build();
        for (name, description) in sources {
            add(&group, &name, &description);
        }
        body.append(&group);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(450)
        .child(&body)
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
