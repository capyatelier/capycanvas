//! Native profile selection; parsing and CMM validation stay on a file worker.
use super::*;
use layer_core::color::{ColorProfile, ProfileChannels};
use std::cell::{Cell, RefCell};
use std::io::Read;

pub(super) struct ProfileChooser {
    pub row: adw::ActionRow,
    pub error: gtk::Label,
    pub selected: Rc<dyn Fn(u32) -> Result<ExportProfile, String>>,
}
impl ProfileChooser {
    pub fn new(parent: &adw::ApplicationWindow, space: &adw::ComboRow, working: RgbSpace) -> Self {
        let row = adw::ActionRow::builder()
            .title("ICC profile")
            .subtitle("Choose an RGB, grayscale or CMYK delivery profile")
            .visible(false)
            .build();
        row.set_use_markup(false);
        row.set_widget_name("export-profile-file");
        let button = gtk::Button::with_label("Choose…");
        button.set_widget_name("export-profile-choose");
        button.set_valign(gtk::Align::Center);
        row.add_suffix(&button);
        row.set_activatable_widget(Some(&button));
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.)
            .visible(false)
            .build();
        error.add_css_class("error");
        error.set_widget_name("export-profile-error");
        let profile = Rc::new(RefCell::new(None));
        let loading = Rc::new(Cell::new(false));
        space.connect_selected_notify(glib::clone!(
            #[weak]
            row,
            #[weak]
            error,
            move |space| {
                row.set_visible(space.selected() == 4);
                error.set_visible(space.selected() == 4 && !error.label().is_empty());
            }
        ));
        button.connect_clicked(glib::clone!(
            #[weak]
            parent,
            #[weak]
            row,
            #[weak]
            error,
            #[weak]
            space,
            #[strong]
            profile,
            #[strong]
            loading,
            move |button| {
                if loading.replace(true) {
                    return;
                }
                button.set_sensitive(false);
                space.notify("selected");
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[weak]
                    parent,
                    #[weak]
                    row,
                    #[weak]
                    error,
                    #[weak]
                    space,
                    #[weak]
                    button,
                    #[strong]
                    profile,
                    #[strong]
                    loading,
                    async move {
                        error.set_label("");
                        let result = choose(&parent, working).await;
                        match result {
                            Ok(Some(value)) => {
                                let model = match value.channels {
                                    ProfileChannels::Rgb => "RGB",
                                    ProfileChannels::Gray => "Grayscale",
                                    ProfileChannels::Cmyk => "CMYK",
                                };
                                row.set_subtitle(&format!("{} · {model}", value.name));
                                *profile.borrow_mut() = Some(value);
                            }
                            Ok(None) => (),
                            Err(message) => error.set_label(&message),
                        }
                        loading.set(false);
                        button.set_sensitive(true);
                        space.notify("selected");
                    }
                ));
            }
        ));
        let selected = Rc::new(move |index| {
            if loading.get() {
                return Err("Reading the delivery profile…".into());
            }
            RgbSpace::ALL
                .get(index as usize)
                .map(|space| ExportProfile::builtin(*space))
                .or_else(|| profile.borrow().clone())
                .ok_or_else(|| "Choose an ICC delivery profile".into())
        });
        Self {
            row,
            error,
            selected,
        }
    }
}

async fn choose(
    parent: &adw::ApplicationWindow,
    working: RgbSpace,
) -> Result<Option<ExportProfile>, String> {
    let dialog = gtk::FileDialog::builder()
        .title("Choose delivery profile")
        .modal(true)
        .build();
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("ICC color profiles"));
    filter.add_suffix("icc");
    filter.add_suffix("icm");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let file = match dialog.open_future(Some(parent)).await {
        Ok(file) => file,
        Err(e)
            if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e.to_string()),
    };
    let path = file.path().ok_or("Choose a local ICC profile file")?;
    gio::spawn_blocking(move || read(&path, working))
        .await
        .map_err(|_| "Profile reader failed".to_string())?
        .map(Some)
}

pub(super) fn read(path: &std::path::Path, working: RgbSpace) -> Result<ExportProfile, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(layer_color::MAX_ICC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > layer_color::MAX_ICC_BYTES {
        return Err("ICC profile exceeds the size limit".into());
    }
    let profile = ColorProfile::Icc(bytes.into());
    let channels = layer_color::profile_channels(&profile)?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .take(128)
        .collect();
    let result = ExportProfile {
        profile,
        channels,
        name,
    };
    // An input-only profile is not necessarily usable for delivery. Validate an
    // actual Float32 output transform before making this selection available.
    let recipe = ExportRecipe {
        format: ExportFormat::Tiff,
        profile: result.clone(),
        background: ExportBackground::White,
        ..ExportRecipe::web_share()
    };
    let encoder =
        layer_color::WorkingEncoder::new(working, &recipe.interpretation(), Default::default())?;
    let mut output = vec![0; recipe.interpretation().pixel_bytes() * 3];
    encoder.encode_straight(
        &[[0., 0., 0., 1.], [0.5, 0.5, 0.5, 1.], [1.; 4]],
        &mut output,
        None,
        [0, 0],
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_loading_keeps_bytes_and_rejects_corrupt_or_oversized_files() {
        let dir = std::env::temp_dir().join(format!("capy-output-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, profile, channels) in [
            (
                "RGB.icc",
                ColorProfile::Builtin(RgbSpace::AdobeRgb),
                ProfileChannels::Rgb,
            ),
            (
                "Gray.icc",
                layer_color::gray_profile(RgbSpace::ProPhoto).unwrap(),
                ProfileChannels::Gray,
            ),
        ] {
            let bytes = layer_color::profile_bytes(&profile).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let loaded = read(&path, RgbSpace::DisplayP3).unwrap();
            assert_eq!(loaded.name, name);
            assert_eq!(loaded.channels, channels);
            assert_eq!(loaded.profile, ColorProfile::Icc(bytes.into()));
        }
        let path = dir.join("invalid.icc");
        std::fs::write(&path, b"not a profile").unwrap();
        assert!(read(&path, RgbSpace::Srgb).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(layer_color::MAX_ICC_BYTES as u64 + 1)
            .unwrap();
        assert!(
            read(&path, RgbSpace::Srgb)
                .unwrap_err()
                .contains("size limit")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
