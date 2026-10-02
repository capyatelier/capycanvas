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
    names: layer_core::DocumentNames,
    localization: &layer_ui::Localizer,
) -> Result<Option<(Project, Option<DocumentLocation>)>, String> {
    let path = file.path().ok_or_else(|| layer_ui::DocumentHostError::ChooseDeviceFile.message(localization))?;
    let location = DocumentLocation {
        uri: file.uri().into(),
        name: path.file_name().ok_or_else(|| layer_ui::DocumentHostError::ChooseFilename.message(localization))?.to_string_lossy().into_owned(),
    };
    let Some((mut project, location)) = run(window, path, location, policy, names, localization).await? else {
        return Ok(None);
    };
    if !window.is_visible() { return Ok(None); }
    if location.is_none() {
        let source = Arc::unwrap_or_clone(project.document.layers[0].source.take().ok_or_else(|| localization.text(layer_ui::MessageId::DOCUMENTS_ERROR_MISSING_SOURCE).to_string())?);
        let metadata = std::mem::take(&mut project.document.metadata);
        let Some(source) = interpret_window(window, source, policy, working, localization).await? else { return Ok(None); };
        let names = layer_core::DocumentNames { paint: project.document.layers[0].name.clone(),
            paper: project.document.layers[1].name.clone() };
        project = gio::spawn_blocking(move || policy.photo_project(source, metadata, names))
            .await.map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(localization))?
            .map_err(|detail| layer_ui::ColorFeatureError::Diagnostic(detail).profile_message(localization))?;
    }
    Ok(window.is_visible().then_some((project, location)))
}

async fn run(
    window: &adw::ApplicationWindow,
    path: PathBuf,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
    names: layer_core::DocumentNames,
    localization: &layer_ui::Localizer,
) -> Result<Option<(Project, Option<DocumentLocation>)>, String> {
    let copy = layer_ui::bootstrap_view(localization);
    let failure = layer_ui::file_open_failure(localization, &location.name);
    let dialog = adw::AlertDialog::builder()
        .heading(copy.opening_files.as_ref())
        .body(copy.preparing_document.as_ref())
        .build();
    dialog.set_widget_name("document-open-progress");
    dialog.add_response("cancel", copy.common.cancel.as_ref());
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
    let result = gio::spawn_blocking(move || read(&path, location, policy, names, control))
        .await
        .map_err(|_| "Project reader failed".to_string())
        .and_then(|result| result);
    dialog.disconnect(signal);
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    dialog.close();
    result.map(Some).map_err(|detail| format!("{failure}\n{detail}"))
}

/// Shared by Open, Place and Paste. Choosing an interpretation changes no source
/// samples; cancellation occurs before publishing any candidate into a window.
pub(super) async fn interpret(
    w: &std::rc::Rc<crate::workspace::Workspace>,
    source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    let working = w.gpu.borrow().as_ref().ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization))?
        .session.engine().document().color.space;
    interpret_window(&w.window, source, policy, working, &w.localization).await
}

async fn interpret_window(
    window: &adw::ApplicationWindow,
    mut source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
    localization: &layer_ui::Localizer,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    if !policy.needs_interpretation(&source)
    {
        return Ok(Some(source));
    }
    let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
    let chooser = super::profile::ProfileChooser::for_window(
        window, copy.interpret_as.as_ref(), "untagged-profile-space", working,
        super::profile::ProfilePurpose::Source(source.interpretation.clone()),
    );
    let space = chooser.row.clone();
    let group = adw::PreferencesGroup::new();
    group.add(&space);
    let current = source.interpretation.profile.clone();
    let profile = gio::spawn_blocking(move || super::profile::describe(current)).await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(localization))?.map_err(|reason| reason.profile_message(localization))?;
    (chooser.restore)(profile);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.append(&group);
    body.append(&chooser.error);
    let dialog = adw::AlertDialog::builder().heading(copy.interpret_title.as_ref()).body(copy.interpret_help.as_ref()).extra_child(&body).content_width(400).prefer_wide_layout(true).build();
    dialog.set_widget_name("untagged-profile-dialog");
    dialog.add_responses(&[("cancel", copy.common.cancel.as_ref()), ("use", copy.use_profile.as_ref())]);
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
    .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile validation worker failed".into()).profile_message(localization))?
    .map_err(|detail| layer_ui::ColorFeatureError::Diagnostic(detail).profile_message(localization))
}

pub(crate) fn read(
    path: &Path,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
    names: layer_core::DocumentNames,
    cancelled: Arc<AtomicBool>,
) -> Result<(Project, Option<DocumentLocation>), String> {
    let file = super::reader::cancellable_file(path, cancelled.clone())
        .map_err(|e| e.to_string())?;
    let imported = layer_ui::read_import(file, layer_ui::ImportIntent::Open, policy,
        names, Default::default(), Default::default(), &cancelled)?;
    Ok((imported.project, imported.source.adoption_location(Some(location))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, SampleDepth, source::*};
    #[test]
    fn untagged_jpeg_opens_with_the_assumed_default_profile() {
        let directory =
            std::env::temp_dir().join(format!("capy-open-photo-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("Ordinary.jpg");
        let mut builder = SourceBuilder::new(
            [3, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgb,
                depth: SampleDepth::U8,
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
            layer_ui::photo_document_names("Ordinary.jpg", &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)),
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
