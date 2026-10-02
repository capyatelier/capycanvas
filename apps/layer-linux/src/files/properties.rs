use crate::workspace::Workspace;
use adw::prelude::*;
use std::rc::Rc;

pub(crate) async fn show(w: &Rc<Workspace>) -> Result<bool, String> {
    let project = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .capture_project_recovery()?;
    let info = layer_color::DocumentInfo::capture(&project.document);
    let inspected = gtk::gio::spawn_blocking(move || info.inspect()).await
        .map_err(|_| "Cannot read source color details")??;
    let view = layer_ui::document_properties(&inspected, &w.localization);
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
    for (title, subtitle) in &view.rows { add(&group, title, subtitle); }
    body.append(&group);
    if !view.sources.is_empty() {
        let group = adw::PreferencesGroup::builder().title(view.source_images.as_ref()).build();
        for (name, description) in &view.sources { add(&group, name, description); }
        body.append(&group);
    }
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(450)
        .child(&body)
        .build());
    let dialog = adw::AlertDialog::builder()
        .heading(view.title.as_ref())
        .content_width(440)
        .extra_child(&scroll)
        .build();
    dialog.set_widget_name("document-properties-dialog");
    dialog.add_response("done", view.done.as_ref());
    dialog.add_response("appearance", view.proof.as_ref());
    dialog.set_close_response("done");
    dialog.set_default_response(Some("done"));
    if crate::alert::choose(dialog, &w.window).await == "appearance" { super::proof::run(w)?; }
    Ok(true)
}
