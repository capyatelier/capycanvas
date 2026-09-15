//! Correct retained source interpretation without changing its original samples.
use super::profile::{ProfileChooser, ProfilePurpose};
use super::*;
use layer_core::{LayerId, color::RgbSpace};

pub(super) async fn repair(w: &Rc<Workspace>, id: u64) -> Result<bool, String> {
    let (epoch, revision, layer, working) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        let document = session.engine().document();
        (
            session.state().document_file.epoch,
            document.revision,
            document
                .layer(LayerId(id))
                .ok_or("Unknown source layer")?
                .clone(),
            document.color.space,
        )
    };
    let original = layer
        .source
        .clone()
        .ok_or("This layer has no retained source")?;
    let baked =
        !layer.raster.is_empty() || !layer.pending_operations.is_empty() || layer.asset.is_some();
    let current = original.interpretation.profile.clone();
    let description = gio::spawn_blocking(move || layer_color::profile_description(&current))
        .await
        .map_err(|_| "Profile reader failed")??;
    let group = adw::PreferencesGroup::new();
    let current = adw::ActionRow::builder()
        .title("Current source profile")
        .subtitle(&format!(
            "{description}{}",
            if original.interpretation.profile_assumed {
                " (assumed)"
            } else {
                ""
            }
        ))
        .subtitle_selectable(true)
        .build();
    current.set_use_markup(false);
    group.add(&current);
    let space = adw::ComboRow::builder()
        .title("Correct source profile")
        .model(&gtk::StringList::new(&[
            "sRGB",
            "Display P3",
            "Adobe RGB (1998)",
            "ProPhoto RGB",
            "Custom ICC",
        ]))
        .use_subtitle(true)
        .build();
    space.set_widget_name("source-profile-space");
    group.add(&space);
    let chooser = ProfileChooser::new(
        &w.window,
        &space,
        working,
        ProfilePurpose::Source(original.interpretation.clone()),
    );
    group.add(&chooser.row);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&chooser.error);
    let hint = gtk::Label::builder().wrap(true).xalign(0.).build();
    hint.set_widget_name("source-profile-hint");
    content.append(&hint);
    let dialog = adw::AlertDialog::builder().heading("Repair Source Profile")
        .body(if baked {
            "This layer has pixel edits. Add a corrected original as a new layer at the same position. The existing layer keeps its edits, masks and adjustments."
        } else {
            "Change how the original image’s color numbers are interpreted. Original samples and depth stay intact. You can undo the change."
        }).prefer_wide_layout(true).content_width(440).extra_child(&content).build();
    dialog.set_widget_name("source-profile-dialog");
    dialog.add_responses(&[
        ("cancel", "Cancel"),
        (
            "apply",
            if baked {
                "Add Corrected Source"
            } else {
                "Apply Profile"
            },
        ),
    ]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    let selected = chooser.selected.clone();
    space.connect_selected_notify(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        hint,
        move |space| {
            let result = selected(space.selected());
            dialog.set_response_enabled("apply", result.is_ok());
            let message = result.err().unwrap_or_default();
            hint.set_label(&message);
            hint.set_visible(!message.is_empty());
        }
    ));
    let index = match original.interpretation.profile {
        layer_core::color::ColorProfile::Builtin(space) => {
            RgbSpace::ALL.iter().position(|s| *s == space).unwrap() as u32
        }
        _ => 4,
    };
    space.set_selected(index);
    space.notify("selected");
    if dialog.choose_future(Some(&w.window)).await != "apply" {
        return Ok(false);
    }
    let profile = (chooser.selected)(space.selected())?;
    let mut corrected = (*original).clone();
    corrected.interpretation.profile = profile.profile;
    corrected.interpretation.profile_assumed = false;
    // Also validate builtins on the worker. ICC selections were already tested
    // against these exact source channels, rather than an output-profile role.
    let corrected = gio::spawn_blocking(move || {
        layer_color::WorkingDecoder::new(&corrected.interpretation, working, Default::default())?;
        Ok::<_, String>(corrected)
    })
    .await
    .map_err(|_| "Source profile validation failed")??;
    let mut gpu = w.gpu.borrow_mut();
    let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
    if session.state().document_file.epoch != epoch
        || session.engine().document().revision != revision
    {
        return Err("The document changed while choosing the source profile; try again".into());
    }
    session.repair_layer_source(layer.id, &original, corrected)?;
    drop(gpu);
    w.wake();
    Ok(true)
}
