//! Signature-based project/photo loading on a file worker. A source photo never
//! grants authority to overwrite that path with a native master.
use adw::prelude::*;
use gtk::{gio, glib};
use layer_core::{Document, Project, ProjectLimits, color::RgbSpace};
use layer_ui::DocumentLocation;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    sync::Arc,
};

/// Shared by Open, Place and Paste. Choosing an interpretation changes no source
/// samples; cancellation occurs before publishing any candidate into a window.
pub(super) async fn interpret(
    w: &std::rc::Rc<crate::workspace::Workspace>,
    mut source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    if !source.interpretation.profile_assumed
        || policy.missing_profile == layer_ui::MissingProfilePolicy::AssumeSrgb
    {
        return Ok(Some(source));
    }
    let space = adw::ComboRow::builder()
        .title("Interpret as")
        .use_subtitle(true)
        .build();
    space.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    space.set_model(Some(&gtk::StringList::new(&[
        "sRGB",
        "Display P3",
        "Adobe RGB (1998)",
        "ProPhoto RGB",
        "Custom ICC",
    ])));
    space.set_widget_name("untagged-profile-space");
    let working = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .engine()
        .document()
        .color
        .space;
    let chooser = super::profile::ProfileChooser::new(
        &w.window,
        &space,
        working,
        super::profile::ProfilePurpose::Source(source.interpretation.clone()),
    );
    let group = adw::PreferencesGroup::new();
    group.add(&space);
    group.add(&chooser.row);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&group);
    body.append(&chooser.error);
    let dialog = adw::AlertDialog::builder().heading("Choose image interpretation").body("This image has no declared color profile. Choose how to interpret its stored values. The original numbers will be retained.").extra_child(&body).content_width(400).prefer_wide_layout(true).build();
    dialog.set_widget_name("untagged-profile-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("use", "Use Profile")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("use"));
    dialog.set_response_appearance("use", adw::ResponseAppearance::Suggested);
    space.connect_selected_notify(glib::clone!(
        #[weak]
        dialog,
        #[strong(rename_to=select)]
        chooser.selected,
        move |space| {
            dialog.set_response_enabled("use", select(space.selected()).is_ok());
        }
    ));
    if crate::alert::choose(dialog, &w.window).await != "use" {
        return Ok(None);
    }
    source.interpretation.profile = (chooser.selected)(space.selected())?.profile;
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
) -> Result<(Project, Option<DocumentLocation>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut reader = BufReader::new(file);
    if reader
        .fill_buf()
        .map_err(|e| e.to_string())?
        .starts_with(b"CAPY")
    {
        return Project::read(reader, ProjectLimits::default()).map(|p| (p, Some(location)));
    }
    let source = layer_color::photo::read_photo(reader, Default::default())?;
    let space = layer_color::suggested_working_space(&source.interpretation.profile)?
        .unwrap_or(RgbSpace::ProPhoto);
    let mut document = Document::new("untitled", source.extent[0], source.extent[1]);
    document.color = layer_core::color::DocumentColor {
        space,
        depth: policy.editing_depth(source.interpretation.depth),
    };
    document.layers[0].name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect::<String>()
        .into();
    if document.layers[0].name.is_empty() {
        document.layers[0].name = "Photo".into();
    }
    document.layers[0].source = Some(Arc::new(source));
    // Retained photo alpha remains visible. A separate Paper layer is available
    // to the user, but does not silently flatten an opened transparent image.
    document.layers[1].visible = false;
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(ProjectLimits::default())?;
    Ok((project, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, IntegerDepth, source::*};
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
