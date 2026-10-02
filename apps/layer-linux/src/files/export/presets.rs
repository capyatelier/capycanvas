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
    Ok(ExportPresets::restore(&bytes))
}
pub(super) async fn load(
    document: layer_core::color::DocumentColor,
    localization: &layer_ui::Localizer,
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
                return Err(layer_ui::ColorFeatureError::ProfileChannels);
            }
            ProfilePurpose::Output.validate(&recipe.profile, document.space)?;
            checked.push(recipe.profile);
        }
        Ok::<_,layer_ui::ColorFeatureError>(library)
    })
    .await
    .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Export preset reader failed".into()).preset_message(localization))?.map_err(|reason|reason.preset_message(localization))
}
fn write(
    path: &std::path::Path,
    expected: &ExportPresets,
    next: &ExportPresets,
) -> Result<(), layer_ui::ColorFeatureError> {
    let _lock = LOCK.lock().map_err(|e| e.to_string())?;
    if read(path)? != *expected {
        return Err(layer_ui::ColorFeatureError::PresetChanged);
    }
    let bytes = next.encode()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    layer_core::atomic_write(path, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    }).map_err(layer_ui::ColorFeatureError::from)
}
pub(super) async fn save(expected: ExportPresets, next: ExportPresets, localization: &layer_ui::Localizer) -> Result<(), String> {
    gio::spawn_blocking(move || write(&path(), &expected, &next))
        .await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Export preset writer failed".into()).preset_message(localization))?.map_err(|reason|reason.preset_message(localization))
}

/// Editing a destination changes temporary controls; saving a named preset is an
/// explicit application preference action and does not depend on exporting a file.
pub(super) fn install(
    parent: &adw::ApplicationWindow,
    localization: &std::sync::Arc<layer_ui::Localizer>,
    hdr_document: bool,
    group: &gtk::Box,
    preset: &adw::ComboRow,
    library: Rc<std::cell::RefCell<ExportPresets>>,
    destination: Rc<std::cell::Cell<usize>>,
    updating: Rc<std::cell::Cell<bool>>,
    read_recipe: Rc<dyn Fn() -> Result<ExportRecipe, String>>,
) {
    let copy = std::rc::Rc::new(layer_ui::color_feature_copy::ExportCopy::new(localization));
    let localization = localization.clone();
    let buttons = adw::PreferencesGroup::new();
    group.append(&buttons);
    let status = adw::ActionRow::builder()
        .use_markup(false)
        .visible(false)
        .build();
    status.set_widget_name("export-presets-status");
    buttons.add(&status);
    let refresh: Rc<dyn Fn(usize)> = Rc::new(glib::clone!( #[strong] localization,
        #[weak]
        preset,
        #[strong]
        library,
        #[strong]
        updating,
        move |selected| {
            let library = library.borrow();
            let color = layer_core::color::DocumentColor { depth: if hdr_document {layer_core::color::SampleDepth::F16} else {layer_core::color::SampleDepth::U8}, ..Default::default() };
            let names = library.localized_names(color, &localization);
            let names: Vec<_> = names.iter().map(String::as_str).collect();
            updating.set(true);
            preset.set_model(Some(&gtk::StringList::new(&names)));
            preset.set_selected(selected as u32);
            updating.set(false);
        }
    ));
    refresh(0);
    for (operation, label) in [(0, copy.save_new_preset.as_ref()), (1, copy.update_saved_preset.as_ref()), (2, copy.remove_saved_preset.as_ref()), (3, copy.restore_original.as_ref())] {
        let button = adw::ButtonRow::builder().title(label).use_markup(false).build();
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
                copy.save_preset_help.as_ref(),
                copy.update_preset_help.as_ref(),
                copy.remove_preset_help.as_ref(),
                copy.reset_preset_help.as_ref(),
            ][operation],
        ));
        if operation == 2 { button.add_css_class("destructive-action"); }
        buttons.add(&button);
        let refresh_button = glib::clone!( #[strong] copy, #[strong] localization,
            #[weak]
            button,
            #[strong]
            destination,
            move |preset: &adw::ComboRow| {
                if let Some(value) = preset.model().and_then(|model| model.item(destination.get() as u32)).and_downcast::<gtk::StringObject>() {
                    let name = value.string();
                    button.set_title(&match operation {
                        1 => layer_ui::color_feature_copy::named(&localization, layer_ui::MessageId::COLOR_FEATURES_EXPORT_UPDATE_NAMED, &name),
                        2 => layer_ui::color_feature_copy::named(&localization, layer_ui::MessageId::COLOR_FEATURES_EXPORT_REMOVE_NAMED, &name),
                        3 => layer_ui::color_feature_copy::named(&localization, layer_ui::MessageId::COLOR_FEATURES_EXPORT_RESET_NAMED, &name),
                        _ => copy.save_new_preset.to_string(),
                    });
                }
                button.set_visible(match operation {
                    1 | 2 => destination.get() >= 4,
                    3 => destination.get() < 4,
                    _ => true,
                });
            }
        );
        refresh_button(preset);
        preset.connect_selected_notify(refresh_button);
        button.connect_activated(glib::clone!( #[strong] copy,
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
            #[strong]
            localization,
            move |_| {
                buttons.set_sensitive(false);
                let index = destination.get();
                let recipe = read_recipe();
                glib::MainContext::default().spawn_local(glib::clone!( #[strong] copy,
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
                    #[strong]
                    localization,
                    async move {
                        let result: Result<Option<(ExportPresets, usize)>, String> = async {
                            let name = if operation == 0 {
                                // Validate first; an unavailable ICC is never saved as a fallback.
                                recipe.as_ref().map_err(Clone::clone)?;
                                let entry = adw::EntryRow::builder().title(copy.preset_name.as_ref()).build();
                                entry.set_widget_name("export-preset-name");
                                let group = adw::PreferencesGroup::new();
                                group.add(&entry);
                                let dialog = adw::AlertDialog::builder()
                                    .heading(copy.save_preset_title.as_ref())
                                    .extra_child(&group)
                                    .build();
                                dialog.set_widget_name("export-preset-name-dialog");
                                dialog.add_responses(&[("cancel", copy.common.cancel.as_ref()), ("save", copy.common.save.as_ref())]);
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
                                0 => next.save(name.as_deref().unwrap(), recipe?).map_err(|reason|reason.preset_message(&localization))?,
                                1 => {
                                    next.update(index, recipe?).map_err(|reason|reason.preset_message(&localization))?;
                                    index
                                }
                                2 => {
                                    next.remove(index).map_err(|reason|reason.preset_message(&localization))?;
                                    3
                                }
                                _ => {
                                    next.reset_destination(index).map_err(|reason|reason.preset_message(&localization))?;
                                    index
                                }
                            };
                            save(expected, next.clone(), &localization).await?;
                            Ok(Some((next, selected)))
                        }
                        .await;
                        match result {
                            Ok(Some((next, selected))) => {
                                *library.borrow_mut() = next;
                                refresh(selected);
                                preset.notify("selected");
                                status.set_title(match operation {
                                    2 => copy.preset_removed.as_ref(),
                                    3 => copy.preset_reset.as_ref(),
                                    _ => copy.preset_saved.as_ref(),
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
        assert_eq!(write(&path, &empty, &empty).unwrap_err(), layer_ui::ColorFeatureError::PresetChanged);
        assert_eq!(read(&path).unwrap(), next);
        std::fs::write(&path, b"broken").unwrap();
        assert_eq!(read(&path).unwrap(), empty);
        assert!(write(&path, &next, &empty).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        write(&path, &empty, &next).unwrap();
        assert_eq!(read(&path).unwrap(), next);
        std::fs::remove_file(path).unwrap();
    }
}
