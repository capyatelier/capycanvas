//! Signature-based project/photo loading on a file worker. A source photo never
//! grants authority to overwrite that path with a native master.
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::Project;
use layer_ui::DocumentLocation;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

/// Shared preparation for menu Open and application file launches. The latter
/// has a native dialog parent before it has a document or rendering device.
pub(super) async fn prepare(
    window: &adw::ApplicationWindow,
    file: gio::File,
    policy: layer_ui::PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
) -> Result<Option<(Project, Option<DocumentLocation>)>, String> {
    let path = file.path().ok_or("Choose a file on this device")?;
    let location = DocumentLocation {
        uri: file.uri().into(),
        name: path.file_name().ok_or("Choose a filename")?.to_string_lossy().into_owned(),
    };
    let Some((mut project, location)) = run(window, path, location, policy).await? else {
        return Ok(None);
    };
    if !window.is_visible() { return Ok(None); }
    if location.is_none() {
        let source = Arc::unwrap_or_clone(project.document.layers[0].source.take().ok_or("Photo source unavailable")?);
        let Some(source) = interpret_window(window, source, policy, working).await? else { return Ok(None); };
        let name = project.document.layers[0].name.to_string();
        project = gio::spawn_blocking(move || policy.photo_project(source, &name))
            .await.map_err(|_| "Profile reader failed")??;
    }
    Ok(window.is_visible().then_some((project, location)))
}

async fn run(
    window: &adw::ApplicationWindow,
    path: PathBuf,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
) -> Result<Option<(Project, Option<DocumentLocation>)>, String> {
    let dialog = adw::AlertDialog::builder()
        .heading("Opening image or project…")
        .body("Reading the file and its color information.")
        .build();
    dialog.set_widget_name("document-open-progress");
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = dialog.connect_response(Some("cancel"), {
        let cancelled = cancelled.clone();
        move |_, _| cancelled.store(true, Ordering::Release)
    });
    dialog.present(Some(window));
    let control = cancelled.clone();
    // Await acknowledgement even after Cancel. A successor cannot overlap a
    // detached decoder, and a cancelled candidate never reaches a new window.
    let result = gio::spawn_blocking(move || read(&path, location, policy, control))
        .await
        .map_err(|_| "Project reader failed".to_string())
        .and_then(|result| result);
    dialog.disconnect(signal);
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    dialog.close();
    result.map(Some)
}

/// Shared by Open, Place and Paste. Choosing an interpretation changes no source
/// samples; cancellation occurs before publishing any candidate into a window.
pub(super) async fn interpret(
    w: &std::rc::Rc<crate::workspace::Workspace>,
    source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    let working = w.gpu.borrow().as_ref().ok_or("Canvas unavailable")?
        .session.engine().document().color.space;
    interpret_window(&w.window, source, policy, working).await
}

async fn interpret_window(
    window: &adw::ApplicationWindow,
    mut source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    if !policy.needs_interpretation(&source)
    {
        return Ok(Some(source));
    }
    let chooser = super::profile::ProfileChooser::for_window(
        window, "Interpret as", "untagged-profile-space", working,
        super::profile::ProfilePurpose::Source(source.interpretation.clone()),
    );
    let space = chooser.row.clone();
    let group = adw::PreferencesGroup::new();
    group.add(&space);
    let current = source.interpretation.profile.clone();
    let profile = gio::spawn_blocking(move || super::profile::describe(current)).await
        .map_err(|_| "Profile reader failed")??;
    (chooser.restore)(profile);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&group);
    body.append(&chooser.error);
    let dialog = adw::AlertDialog::builder().heading("Choose image interpretation").body("This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained.").extra_child(&body).content_width(400).prefer_wide_layout(true).build();
    dialog.set_widget_name("untagged-profile-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("use", "Use Profile")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("use"));
    dialog.set_response_appearance("use", adw::ResponseAppearance::Suggested);
    space.connect_subtitle_notify(glib::clone!(
        #[weak]
        dialog,
        #[strong(rename_to=select)]
        chooser.selected,
        move |_| {
            dialog.set_response_enabled("use", select().is_ok());
        }
    ));
    if crate::alert::choose(dialog, window).await != "use" {
        return Ok(None);
    }
    source.interpretation.profile = (chooser.selected)()?.profile;
    source.interpretation.profile_assumed = false;
    // Validate the chosen source transform on a worker before adoption.
    gio::spawn_blocking(move || {
        layer_color::WorkingDecoder::new(&source.interpretation, working, Default::default())?;
        source.validate()?;
        Ok(Some(source))
    })
    .await
    .map_err(|_| "Profile validation worker failed".to_string())?
}

pub(crate) fn read(
    path: &Path,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
    cancelled: Arc<AtomicBool>,
) -> Result<(Project, Option<DocumentLocation>), String> {
    let file = super::reader::CancelRead::new(path, cancelled.clone())
        .map_err(|e| format!("Cannot open file: {e}"))?;
    let imported = layer_ui::read_import(file, layer_ui::ImportIntent::Open, policy,
        &path.file_name().unwrap_or_default().to_string_lossy(), Default::default(), Default::default(), &cancelled)?;
    Ok((imported.project, imported.source.adoption_location(Some(location))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace, source::*};
    #[test]
    fn photo_open_preserves_source_depth_profile_and_master_separation() {
        let directory =
            std::env::temp_dir().join(format!("capy-open-photo-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                let profile = ColorProfile::Icc(
                    layer_color::profile_bytes(&ColorProfile::Builtin(space))
                        .unwrap()
                        .into(),
                );
                let mut builder = SourceBuilder::new(
                    [3, 1],
                    SourceInterpretation {
                        channels: SourceChannels::Rgba,
                        depth,
                        profile,
                        profile_assumed: false,
                    },
                    1024 * 1024,
                )
                .unwrap();
                let codes = [
                    65535u16, 0, 32767, 1, 12345, 54321, 1001, 50000, 7654, 65535, 12456, 0,
                ];
                let row: Vec<u8> = if depth == IntegerDepth::U8 {
                    codes.map(|v| (v / 257) as u8).to_vec()
                } else {
                    codes.into_iter().flat_map(u16::to_le_bytes).collect()
                };
                builder.push_row(&row).unwrap();
                let source = builder.finish().unwrap();
                // Misleading extension cannot change file interpretation.
                let path = directory.join(format!("Photo-{space:?}-{depth:?}.capy"));
                layer_color::photo::write_png(std::fs::File::create(&path).unwrap(), &source)
                    .unwrap();
                let bytes = std::fs::read(&path).unwrap();
                let (project, location) = read(
                    &path,
                    DocumentLocation {
                        uri: "source".into(),
                        name: "Photo.png".into(),
                    },
                    Default::default(),
                    Default::default(),
                )
                .unwrap();
                assert!(location.is_none());
                assert_eq!(project.document.color.space, space);
                assert_eq!(project.document.color.depth, depth);
                assert!(!project.document.layers[1].visible);
                assert_eq!(project.document.layers[0].source.as_deref(), Some(&source));
                let policy = layer_ui::PhotoOpenPolicy {
                    promote_to_16: true,
                    missing_profile: layer_ui::MissingProfilePolicy::Ask,
                };
                let (promoted, _) = read(
                    &path,
                    DocumentLocation {
                        uri: "source".into(),
                        name: "Photo.png".into(),
                    },
                    policy,
                    Default::default(),
                )
                .unwrap();
                assert_eq!(promoted.document.color.depth, IntegerDepth::U16);
                assert_eq!(promoted.document.layers[0].source.as_deref(), Some(&source));
                let mut master = Vec::new();
                project.write(&mut master).unwrap();
                assert_eq!(
                    Project::read(std::io::Cursor::new(master), Default::default()).unwrap(),
                    project
                );
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                let native = directory.join(format!("master-{space:?}-{depth:?}.capy"));
                project
                    .write(std::fs::File::create(&native).unwrap())
                    .unwrap();
                let (reopened, location) = read(
                    &native,
                    DocumentLocation {
                        uri: "master".into(),
                        name: "master.capy".into(),
                    },
                    policy,
                    Default::default(),
                )
                .unwrap();
                assert!(location.is_some());
                assert_eq!(
                    reopened, project,
                    "photo policies never reinterpret a native master"
                );
            }
        }
        let path = directory.join("Ordinary.jpg");
        let mut builder = SourceBuilder::new(
            [3, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgb,
                depth: IntegerDepth::U8,
                profile: ColorProfile::default(),
                profile_assumed: true,
            },
            1024,
        )
        .unwrap();
        builder
            .push_row(&[230, 75, 100, 245, 180, 100, 25, 190, 110])
            .unwrap();
        let mut jpeg = Vec::new();
        layer_color::photo::write_jpeg(&mut jpeg, &builder.finish().unwrap(), 95).unwrap();
        // Strip APP2 ICC segments to exercise an ordinary untagged JPEG.
        let mut untagged = jpeg[..2].to_vec();
        let mut at = 2;
        while jpeg[at + 1] != 0xda {
            let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]])) + 2;
            if jpeg[at + 1] != 0xe2 {
                untagged.extend_from_slice(&jpeg[at..at + len]);
            }
            at += len;
        }
        untagged.extend_from_slice(&jpeg[at..]);
        std::fs::write(&path, untagged).unwrap();
        let (project, location) = read(
            &path,
            DocumentLocation {
                uri: "source".into(),
                name: "Ordinary.jpg".into(),
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert!(location.is_none());
        assert_eq!(project.document.color, Default::default());
        assert!(
            project.document.layers[0]
                .source
                .as_ref()
                .unwrap()
                .interpretation
                .profile_assumed
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
