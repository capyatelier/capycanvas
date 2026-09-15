//! Application-owned ICC files. Imported bytes are exact and content-addressed;
//! removing a library entry cannot remove source files or embedded project data.
use super::*;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 128;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Debug)]
pub(super) struct Entry {
    path: PathBuf,
    name: String,
    channels: Option<ProfileChannels>,
    issue: Option<String>,
    bytes: u64,
}
impl Entry {
    fn description(&self) -> String {
        if let Some(issue) = &self.issue {
            return issue.clone();
        }
        format!(
            "{} · {:.1} KiB",
            match self.channels.unwrap() {
                ProfileChannels::Rgb => "RGB",
                ProfileChannels::Gray => "Grayscale",
                ProfileChannels::Cmyk => "CMYK",
            },
            self.bytes as f64 / 1024.
        )
    }
}
pub(super) fn directory() -> PathBuf {
    if let Some(path) = std::env::var_os("LAYER_SETTINGS_FILE")
        .map(PathBuf::from)
        .and_then(|p| p.parent().map(|p| p.join("color-profiles")))
    {
        return path;
    }
    if cfg!(test) {
        return std::env::temp_dir().join(format!("capy-color-profiles-{}", std::process::id()));
    }
    glib::user_data_dir().join("capycanvas/color-profiles")
}
fn digest(bytes: &[u8]) -> String {
    glib::compute_checksum_for_data(glib::ChecksumType::Sha256, bytes)
        .unwrap()
        .into()
}
fn read_profile(path: &Path) -> Result<(Vec<u8>, String, ProfileChannels), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Choose a profile file".into());
    }
    let mut bytes = Vec::new();
    file.take(layer_color::MAX_ICC_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > layer_color::MAX_ICC_BYTES {
        return Err("ICC profile exceeds the size limit".into());
    }
    let profile = ColorProfile::Icc(bytes.clone().into());
    let channels = layer_color::profile_channels(&profile)?;
    let name = layer_color::profile_description(&profile)?;
    Ok((bytes, name, channels))
}
pub(super) fn list(directory: &Path) -> Result<Vec<Entry>, String> {
    let reader = match std::fs::read_dir(directory) {
        Ok(reader) => reader,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut result = Vec::new();
    let mut total = 0u64;
    for entry in reader {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let valid_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|v| v.is_ascii_hexdigit()));
        if !entry.file_type().map_err(|e| e.to_string())?.is_file()
            || path.extension().is_none_or(|v| v != "icc")
            || !valid_name
        {
            continue;
        }
        let size = entry.metadata().map_err(|e| e.to_string())?.len();
        total = total.saturating_add(size);
        if result.len() >= MAX_ENTRIES {
            break;
        }
        let value = if total > MAX_BYTES {
            Err("Library exceeds 64 MiB; remove unused profiles".into())
        } else {
            read_profile(&path).and_then(|(bytes, name, channels)| {
                if path.file_stem().unwrap().to_str() != Some(&digest(&bytes)) {
                    return Err("Profile changed on disk; remove or reimport it".into());
                }
                Ok((name, channels))
            })
        };
        let (name, channels, issue) = match value {
            Ok((name, channels)) => (name, Some(channels), None),
            Err(error) => (
                format!(
                    "Unavailable profile {}",
                    &path.file_stem().unwrap().to_str().unwrap()[..12]
                ),
                None,
                Some(error),
            ),
        };
        result.push(Entry {
            path,
            name,
            channels,
            issue,
            bytes: size,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    Ok(result)
}
fn import(directory: &Path, source: &Path) -> Result<Vec<Entry>, String> {
    let _lock = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    let (bytes, _, _) = read_profile(source)?;
    let mut entries = list(directory)?;
    let target = directory.join(format!("{}.icc", digest(&bytes)));
    if entries
        .iter()
        .any(|e| e.path == target && e.issue.is_none())
    {
        return Ok(entries);
    }
    let others: Vec<_> = entries.iter().filter(|e| e.path != target).collect();
    if others.len() >= MAX_ENTRIES
        || others
            .iter()
            .map(|e| e.bytes)
            .fold(bytes.len() as u64, u64::saturating_add)
            > MAX_BYTES
    {
        return Err("The profile library limit is 128 profiles and 64 MiB".into());
    }
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    layer_core::atomic_write(&target, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    })?;
    entries = list(directory)?;
    Ok(entries)
}
fn remove(directory: &Path, path: &Path) -> Result<Vec<Entry>, String> {
    let _lock = STORE_LOCK.lock().map_err(|e| e.to_string())?;
    if !list(directory)?.iter().any(|e| e.path == path) {
        return Err("Select an imported profile".into());
    }
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    list(directory)
}
fn rows(list: &gtk::ListBox, entries: &[Entry]) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    for entry in entries {
        let row = adw::ActionRow::builder()
            .title(&entry.name)
            .subtitle(entry.description())
            .use_markup(false)
            .build();
        list.append(&row);
    }
}
fn view(entries: &[Entry]) -> (gtk::ListBox, gtk::ScrolledWindow) {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .build();
    list.add_css_class("boxed-list");
    rows(&list, entries);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(340)
        .min_content_height(100)
        .child(&list)
        .build();
    (list, scroll)
}
pub(super) enum Selection {
    Cancel,
    Browse,
    File(PathBuf),
}
pub(super) fn read_entry(
    path: &Path,
    working: RgbSpace,
    purpose: &ProfilePurpose,
) -> Result<ExportProfile, String> {
    let mut profile = super::read(path, working, purpose)?;
    let ColorProfile::Icc(bytes) = &profile.profile else {
        unreachable!()
    };
    if path.file_stem().and_then(|s| s.to_str()) != Some(&digest(bytes)) {
        return Err("The imported profile changed on disk; reimport it before use".into());
    }
    profile.name = layer_color::profile_description(&profile.profile)?;
    Ok(profile)
}
pub(super) async fn select(parent: &adw::ApplicationWindow, entries: &[Entry]) -> Selection {
    let entries: Vec<_> = entries
        .iter()
        .filter(|e| e.issue.is_none())
        .cloned()
        .collect();
    let (list, scroll) = view(&entries);
    list.set_widget_name("profile-library-choices");
    let dialog = adw::AlertDialog::builder().heading("Choose ICC profile").body("Choose an imported profile or browse for a file. Its compatibility is checked before use.").extra_child(&scroll).build();
    dialog.set_widget_name("profile-library-choose");
    dialog.add_responses(&[
        ("cancel", "Cancel"),
        ("browse", "Browse…"),
        ("use", "Use Profile"),
    ]);
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("use", false);
    list.connect_row_selected(glib::clone!(
        #[weak]
        dialog,
        move |_, row| dialog.set_response_enabled("use", row.is_some())
    ));
    match crate::alert::choose(dialog, parent).await.as_str() {
        "browse" => Selection::Browse,
        "use" => list
            .selected_row()
            .and_then(|row| entries.get(row.index() as usize))
            .map_or(Selection::Cancel, |e| Selection::File(e.path.clone())),
        _ => Selection::Cancel,
    }
}

pub(crate) async fn manage(w: &Rc<Workspace>) -> Result<(), String> {
    let initial = gio::spawn_blocking(|| list(&directory()))
        .await
        .map_err(|_| "Profile library reader failed")??;
    let entries = Rc::new(RefCell::new(initial));
    let (list, scroll) = view(&entries.borrow());
    list.set_widget_name("profile-library-list");
    let error = gtk::Label::builder().wrap(true).xalign(0.).build();
    error.set_widget_name("profile-library-status");
    error.set_label(if entries.borrow().is_empty() {
        "No imported profiles"
    } else {
        ""
    });
    let add = gtk::Button::with_label("Import…");
    add.set_widget_name("profile-library-import");
    let remove_button = gtk::Button::with_label("Remove");
    remove_button.set_widget_name("profile-library-remove");
    remove_button.set_sensitive(false);
    let busy = Rc::new(Cell::new(false));
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.append(&add);
    buttons.append(&remove_button);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&scroll);
    body.append(&buttons);
    body.append(&error);
    let dialog = adw::AlertDialog::builder().heading("Color Profiles").body("Imported ICC profiles are available for source interpretation and profiled export. Removing a library profile leaves source files and profiles embedded in drawings unchanged.").extra_child(&body).content_width(480).build();
    dialog.set_widget_name("profile-library-manager");
    dialog.add_response("close", "Close");
    dialog.set_close_response("close");
    list.connect_row_selected(glib::clone!(
        #[weak]
        remove_button,
        #[strong]
        busy,
        move |_, row| remove_button.set_sensitive(row.is_some() && !busy.get())
    ));
    for (button, importing) in [(&add, true), (&remove_button, false)] {
        button.connect_clicked(glib::clone!(
            #[weak]
            w,
            #[weak]
            list,
            #[weak]
            add,
            #[weak]
            remove_button,
            #[weak]
            error,
            #[strong]
            entries,
            #[strong]
            busy,
            move |_| {
                if busy.replace(true) {
                    return;
                }
                add.set_sensitive(false);
                remove_button.set_sensitive(false);
                let selected = list.selected_row().and_then(|row| {
                    entries
                        .borrow()
                        .get(row.index() as usize)
                        .map(|e| e.path.clone())
                });
                glib::MainContext::default().spawn_local(glib::clone!(
                    #[strong]
                    w,
                    #[strong]
                    list,
                    #[strong]
                    add,
                    #[strong]
                    remove_button,
                    #[strong]
                    error,
                    #[strong]
                    entries,
                    #[strong]
                    busy,
                    async move {
                        let result = if importing {
                            let chooser = gtk::FileDialog::builder()
                                .title("Import ICC profile")
                                .build();
                            let filter = gtk::FileFilter::new();
                            filter.set_name(Some("ICC color profiles"));
                            filter.add_suffix("icc");
                            filter.add_suffix("icm");
                            chooser.set_default_filter(Some(&filter));
                            match chooser.open_future(Some(&w.window)).await {
                                Ok(file) => match file.path() {
                                    Some(path) => {
                                        gio::spawn_blocking(move || import(&directory(), &path))
                                            .await
                                            .map_err(|_| "Profile import worker failed".to_string())
                                            .and_then(|r| r)
                                            .map(Some)
                                    }
                                    None => Err("Choose a local profile file".into()),
                                },
                                Err(e)
                                    if e.matches(gtk::DialogError::Dismissed)
                                        || e.matches(gtk::DialogError::Cancelled) =>
                                {
                                    Ok(None)
                                }
                                Err(e) => Err(e.to_string()),
                            }
                        } else if let Some(path) = selected {
                            gio::spawn_blocking(move || remove(&directory(), &path))
                                .await
                                .map_err(|_| "Profile removal worker failed".to_string())
                                .and_then(|r| r)
                                .map(Some)
                        } else {
                            Ok(None)
                        };
                        match result {
                            Ok(Some(values)) => {
                                rows(&list, &values);
                                *entries.borrow_mut() = values;
                                error.set_label(if entries.borrow().is_empty() {
                                    "No imported profiles"
                                } else {
                                    ""
                                });
                            }
                            Ok(None) => (),
                            Err(message) => error.set_label(&message),
                        }
                        busy.set(false);
                        add.set_sensitive(true);
                        remove_button.set_sensitive(list.selected_row().is_some());
                    }
                ));
            }
        ));
    }
    crate::alert::choose(dialog, &w.window).await;
    while busy.get() {
        glib::timeout_future(std::time::Duration::from_millis(5)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_preserves_bytes_deduplicates_and_never_removes_the_original() {
        let root =
            std::env::temp_dir().join(format!("capy-profile-library-{}", std::process::id()));
        let store = root.join("library");
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("original.icc");
        let bytes =
            layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        std::fs::write(&original, &bytes).unwrap();
        let entries = import(&store, &original).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channels, Some(ProfileChannels::Rgb));
        assert_eq!(std::fs::read(&entries[0].path).unwrap(), bytes);
        assert_eq!(import(&store, &original).unwrap().len(), 1);
        std::fs::write(&entries[0].path, b"damaged").unwrap();
        assert!(list(&store).unwrap()[0].issue.is_some());
        assert!(read_entry(&entries[0].path, RgbSpace::Srgb, &ProfilePurpose::Output).is_err());
        let repaired = import(&store, &original).unwrap();
        assert!(repaired[0].issue.is_none());
        assert_eq!(std::fs::read(&repaired[0].path).unwrap(), bytes);
        assert!(remove(&store, &original).is_err());
        let embedded = read(
            &entries[0].path,
            RgbSpace::ProPhoto,
            &ProfilePurpose::Output,
        )
        .unwrap();
        assert!(remove(&store, &entries[0].path).unwrap().is_empty());
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        assert_eq!(embedded.profile, ColorProfile::Icc(bytes.clone().into()));
        std::fs::write(&original, b"invalid profile").unwrap();
        assert!(import(&store, &original).is_err());
        assert!(list(&store).unwrap().is_empty());
        std::fs::File::create(&original)
            .unwrap()
            .set_len(layer_color::MAX_ICC_BYTES as u64 + 1)
            .unwrap();
        assert!(
            import(&store, &original)
                .unwrap_err()
                .contains("size limit")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
