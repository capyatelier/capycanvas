//! Native transport for portable delivery preferences. Disk/CMM work is off-thread.
use super::*;
use std::{
    io::{Read, Write},
    path::PathBuf,
};
static LOCK: Mutex<()> = Mutex::new(());

pub(super) fn path() -> PathBuf {
    if let Some(path) = std::env::var_os("LAYER_SETTINGS_FILE").map(PathBuf::from) {
        return path.with_file_name("export-presets.json");
    }
    if cfg!(test) {
        return std::env::temp_dir()
            .join(format!("capy-export-presets-{}.json", std::process::id()));
    }
    glib::user_data_dir().join("capycanvas/export-presets.json")
}
fn read(path: &std::path::Path) -> Result<ExportPresets, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ExportPresets::default()),
        Err(e) => return Err(e.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(ExportPresets::MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    ExportPresets::decode(&bytes)
        .map_err(|e| format!("Cannot read export presets at {}: {e}", path.display()))
}
pub(super) async fn load(
    document: layer_core::color::DocumentColor,
) -> Result<ExportPresets, String> {
    gio::spawn_blocking(move || {
        let library = read(&path())?;
        let mut checked = Vec::new();
        for index in 0..4 + library.names().count() {
            let recipe = library.recipe(index, document)?;
            if checked.contains(&recipe.profile) {
                continue;
            }
            let actual = layer_color::profile_channels(&recipe.profile.profile)?;
            if actual != recipe.profile.channels {
                return Err("Saved export profile channels do not match its ICC data".into());
            }
            ProfilePurpose::Output.validate(&recipe.profile, document.space)?;
            checked.push(recipe.profile);
        }
        Ok(library)
    })
    .await
    .map_err(|_| "Export preset reader failed".to_string())?
}
fn write(
    path: &std::path::Path,
    expected: &ExportPresets,
    next: &ExportPresets,
) -> Result<(), String> {
    let _lock = LOCK.lock().map_err(|e| e.to_string())?;
    if read(path)? != *expected {
        return Err(
            "Export presets changed in another window. Reopen Export to use the latest choices."
                .into(),
        );
    }
    let bytes = next.encode()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    layer_core::atomic_write(path, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    })
}
pub(super) async fn save(expected: ExportPresets, next: ExportPresets) -> Result<(), String> {
    gio::spawn_blocking(move || write(&path(), &expected, &next))
        .await
        .map_err(|_| "Export preset writer failed".to_string())?
}

/// Editing a destination changes temporary controls; saving a named preset is an
/// explicit application preference action and does not depend on exporting a file.
pub(super) fn install(
    parent: &adw::ApplicationWindow,
    hdr_document: bool,
    group: &adw::PreferencesGroup,
    preset: &adw::ComboRow,
    library: Rc<std::cell::RefCell<ExportPresets>>,
    destination: Rc<std::cell::Cell<usize>>,
    updating: Rc<std::cell::Cell<bool>>,
    read_recipe: Rc<dyn Fn() -> Result<ExportRecipe, String>>,
) {
    let row = adw::PreferencesRow::new();
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    buttons.set_valign(gtk::Align::Center);
    buttons.set_halign(gtk::Align::Center);
    buttons.set_margin_top(8);
    buttons.set_margin_bottom(8);
    let expander = gtk::Expander::builder().label("Manage presets").child(&buttons).build();
    expander.set_widget_name("export-manage-presets");
    row.set_child(Some(&expander));
    group.add(&row);
    let status = adw::ActionRow::builder()
        .use_markup(false)
        .visible(false)
        .build();
    status.set_widget_name("export-presets-status");
    group.add(&status);
    let refresh: Rc<dyn Fn(usize)> = Rc::new(glib::clone!(
        #[weak]
        preset,
        #[strong]
        library,
        #[strong]
        updating,
        move |selected| {
            let library = library.borrow();
            let names: Vec<_> = ExportPresets::DESTINATIONS
                .iter()
                .copied()
                .map(|name| if hdr_document && name == "Further editing" { "Further editing (SDR)" } else { name })
                .chain(library.names())
                .collect();
            updating.set(true);
            preset.set_model(Some(&gtk::StringList::new(&names)));
            preset.set_selected(selected as u32);
            updating.set(false);
        }
    ));
    refresh(0);
    for (operation, label) in [(0, "Save as…"), (1, "Update"), (2, "Remove"), (3, "Reset")] {
        let button = gtk::Button::with_label(label);
        button.set_widget_name(
            [
                "export-preset-save",
                "export-preset-update",
                "export-preset-remove",
                "export-preset-reset",
            ][operation],
        );
        button.set_tooltip_text(Some(
            [
                "Save these choices as a new named preset",
                "Replace the selected saved preset with these choices",
                "Remove the selected saved preset",
                "Restore this destination's original choices",
            ][operation],
        ));
        buttons.append(&button);
        let refresh_button = glib::clone!(
            #[weak]
            button,
            #[strong]
            destination,
            move |_: &adw::ComboRow| {
                button.set_sensitive(match operation {
                    1 | 2 => destination.get() >= 4,
                    3 => destination.get() < 4,
                    _ => true,
                });
            }
        );
        refresh_button(preset);
        preset.connect_selected_notify(refresh_button);
        button.connect_clicked(glib::clone!(
            #[weak]
            parent,
            #[weak]
            preset,
            #[weak]
            buttons,
            #[weak]
            status,
            #[strong]
            library,
            #[strong]
            destination,
            #[strong]
            read_recipe,
            #[strong]
            refresh,
            move |_| {
                buttons.set_sensitive(false);
                let index = destination.get();
                let recipe = read_recipe();
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[weak]
                    parent,
                    #[weak]
                    preset,
                    #[weak]
                    buttons,
                    #[weak]
                    status,
                    #[strong]
                    library,
                    #[strong]
                    refresh,
                    async move {
                        let result: Result<Option<(ExportPresets, usize)>, String> = async {
                            let name = if operation == 0 {
                                // Validate first; an unavailable ICC is never saved as a fallback.
                                recipe.as_ref().map_err(Clone::clone)?;
                                let entry = adw::EntryRow::builder().title("Preset name").build();
                                entry.set_widget_name("export-preset-name");
                                let group = adw::PreferencesGroup::new();
                                group.add(&entry);
                                let dialog = adw::AlertDialog::builder()
                                    .heading("Save export preset")
                                    .extra_child(&group)
                                    .build();
                                dialog.set_widget_name("export-preset-name-dialog");
                                dialog.add_responses(&[("cancel", "Cancel"), ("save", "Save")]);
                                dialog.set_close_response("cancel");
                                dialog.set_default_response(Some("save"));
                                dialog.set_response_appearance(
                                    "save",
                                    adw::ResponseAppearance::Suggested,
                                );
                                dialog.set_response_enabled("save", false);
                                entry.connect_changed(glib::clone!(
                                    #[weak]
                                    dialog,
                                    move |entry| dialog.set_response_enabled(
                                        "save",
                                        !entry.text().trim().is_empty()
                                    )
                                ));
                                if crate::alert::choose(dialog, &parent).await != "save" {
                                    return Ok(None);
                                }
                                Some(entry.text().to_string())
                            } else {
                                None
                            };
                            let expected = library.borrow().clone();
                            let mut next = expected.clone();
                            let selected = match operation {
                                0 => next.save(name.as_deref().unwrap(), recipe?)?,
                                1 => {
                                    next.update(index, recipe?)?;
                                    index
                                }
                                2 => {
                                    next.remove(index)?;
                                    3
                                }
                                _ => {
                                    next.reset_destination(index)?;
                                    index
                                }
                            };
                            save(expected, next.clone()).await?;
                            Ok(Some((next, selected)))
                        }
                        .await;
                        match result {
                            Ok(Some((next, selected))) => {
                                *library.borrow_mut() = next;
                                refresh(selected);
                                preset.notify("selected");
                                status.set_title(match operation {
                                    2 => "Preset removed",
                                    3 => "Original choices restored",
                                    _ => "Preset saved",
                                });
                                status.set_visible(true);
                                status.remove_css_class("error");
                            }
                            Ok(None) => (),
                            Err(error) => {
                                status.set_title(&error);
                                status.set_visible(true);
                                status.add_css_class("error");
                            }
                        }
                        buttons.set_sensitive(true);
                    }
                ));
            }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_presets_detect_stale_windows_and_keep_embedded_profile_bytes() {
        let path =
            std::env::temp_dir().join(format!("capy-export-store-{}.json", std::process::id()));
        let empty = ExportPresets::default();
        let mut next = empty.clone();
        let mut recipe = ExportRecipe::web_share();
        recipe.profile.profile = layer_core::color::ColorProfile::Icc(
            layer_color::profile_bytes(&recipe.profile.profile)
                .unwrap()
                .into(),
        );
        next.save("My delivery", recipe.clone()).unwrap();
        write(&path, &empty, &next).unwrap();
        let readback = read(&path).unwrap();
        assert_eq!(readback.recipe(4, Default::default()).unwrap(), recipe);
        assert!(
            write(&path, &empty, &empty)
                .unwrap_err()
                .contains("another window")
        );
        assert_eq!(read(&path).unwrap(), next);
        std::fs::write(&path, b"broken").unwrap();
        assert!(write(&path, &next, &empty).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        std::fs::remove_file(path).unwrap();
    }
}
