//! Signature-based project/photo loading on a file worker. A source photo never
//! grants authority to overwrite that path with a native master.
use adw::prelude::*;
use gtk::{gio, glib};
use layer_ui::DocumentLocation;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

/// Shared preparation for menu Open and application file launches. The latter
/// has a native dialog parent before it has a document or rendering device.
pub(super) async fn prepare(
    window: &adw::ApplicationWindow,
    workspace: Option<&std::rc::Rc<crate::workspace::Workspace>>,
    file: gio::File,
    policy: layer_ui::PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
    names: layer_core::DocumentNames,
    localization: &Arc<layer_ui::Localizer>,
) -> Result<Option<(layer_ui::ImportedDocument, Option<DocumentLocation>)>, String> {
    let path = file.path().ok_or_else(|| layer_ui::DocumentHostError::ChooseDeviceFile.message(localization))?;
    let location = DocumentLocation {
        uri: file.uri().into(),
        name: path.file_name().ok_or_else(|| layer_ui::DocumentHostError::ChooseFilename.message(localization))?.to_string_lossy().into_owned(),
    };
    let Some((outcome, location)) = run(window, workspace, path, location, policy, names, localization).await? else {
        return Ok(None);
    };
    if !window.is_visible() { return Ok(None); }
    let mut imported = match outcome {
        layer_ui::ImportOutcome::Editable(imported) => imported,
        layer_ui::ImportOutcome::Package(outcome) => {
            show_package(window, layer_ui::PackageView::new(outcome)?, file.path(), localization).await?;
            return Ok(None);
        }
    };
    if location.is_none() {
        let project = &imported.project;
        let source = project.artwork.paint.iter().find_map(|(_,_,p)| p.original.clone())
            .ok_or_else(|| localization.text(layer_ui::MessageId::DOCUMENTS_ERROR_MISSING_SOURCE).to_string())?;
        let source = Arc::unwrap_or_clone(source);
        let metadata = (*project.artwork.metadata).clone();
        let names = layer_core::DocumentNames {
            paint: project.scene().order().first().and_then(|h| project.scene().occurrence(*h)).map_or_else(|| "Photo".into(), |o| o.name.clone()),
            paper: project.scene().constant_backdrop().first().and_then(|h| project.scene().occurrence(*h)).map(|o| o.name.clone()).unwrap_or_default(),
        };
        let Some(source) = interpret_window(window, workspace, source, policy, working, localization).await? else { return Ok(None); };
        imported.project = gio::spawn_blocking(move || policy.photo_project(source, metadata, names))
            .await.map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))?
            .map_err(|detail| layer_ui::ColorFeatureError::Diagnostic(detail).profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))?;
    }
    Ok(window.is_visible().then_some((imported, location)))
}

async fn run(
    window: &adw::ApplicationWindow,
    workspace: Option<&std::rc::Rc<crate::workspace::Workspace>>,
    path: PathBuf,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
    names: layer_core::DocumentNames,
    localization: &Arc<layer_ui::Localizer>,
) -> Result<Option<(layer_ui::ImportOutcome, Option<DocumentLocation>)>, String> {
    let copy = layer_ui::bootstrap_view(localization);
    let failure_name = location.name.clone();
    let dialog = adw::AlertDialog::builder()
        .heading(copy.opening_files.as_ref())
        .body(copy.preparing_document.as_ref())
        .build();
    dialog.set_widget_name("document-open-progress");
    dialog.add_response("cancel", copy.common.cancel.as_ref());
    dialog.set_close_response("cancel");
    {
        let weak = dialog.downgrade();
        crate::on_window_localization(window, workspace, localization, move |localization| {
            let Some(dialog) = weak.upgrade() else { return false };
            let copy = layer_ui::bootstrap_view(localization);
            dialog.set_heading(Some(&copy.opening_files)); dialog.set_body(&copy.preparing_document);
            dialog.set_response_label("cancel", &copy.common.cancel);
            true
        });
    }
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
        .map_err(|_| "Document reader failed".to_string())
        .and_then(|result| result);
    dialog.disconnect(signal);
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    dialog.close();
    let localization = crate::window_localization(window, workspace, localization);
    result.map(Some).map_err(|detail| format!("{}\n{detail}", layer_ui::file_open_failure(&localization, &failure_name)))
}

/// Shared by Open, Place and Paste. Choosing an interpretation changes no source
/// samples; cancellation occurs before publishing any candidate into a window.
pub(super) async fn interpret(
    w: &std::rc::Rc<crate::workspace::Workspace>,
    source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    let working = w.gpu.borrow().as_ref().ok_or_else(|| layer_ui::NewDocumentError::CanvasUnavailable.message(&w.localization()))?
        .session.engine().document().composition().color.space;
    interpret_window(&w.window, Some(w), source, policy, working, &w.localization()).await
}

async fn interpret_window(
    window: &adw::ApplicationWindow,
    workspace: Option<&std::rc::Rc<crate::workspace::Workspace>>,
    mut source: layer_core::color::source::SourceImage,
    policy: layer_ui::PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
    localization: &Arc<layer_ui::Localizer>,
) -> Result<Option<layer_core::color::source::SourceImage>, String> {
    if !policy.needs_interpretation(&source)
    {
        return Ok(Some(source));
    }
    let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
    let purpose = super::profile::ProfilePurpose::Source(source.interpretation.clone());
    let chooser = match workspace {
        Some(w) => super::profile::ProfileChooser::new(w, &copy.interpret_as, "untagged-profile-space", working, purpose),
        None => super::profile::ProfileChooser::for_window(window, &copy.interpret_as, "untagged-profile-space", working, purpose, localization.clone()),
    };
    let space = chooser.row.clone();
    let group = adw::PreferencesGroup::new();
    group.add(&space);
    let current = source.interpretation.profile.clone();
    let profile = gio::spawn_blocking(move || super::profile::describe(current)).await
        .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile reader failed".into()).profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))?.map_err(|reason| reason.profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))?;
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
    {
        let weak = dialog.downgrade(); let space = space.downgrade();
        crate::on_window_localization(window, workspace, localization, move |localization| {
            let Some(dialog) = weak.upgrade() else { return false };
            let copy = layer_ui::color_feature_copy::ProfileCopy::new(localization);
            dialog.set_heading(Some(&copy.interpret_title)); dialog.set_body(&copy.interpret_help);
            dialog.set_response_label("cancel", &copy.common.cancel); dialog.set_response_label("use", &copy.use_profile);
            if let Some(space) = space.upgrade() { space.set_title(&copy.interpret_as); }
            true
        });
    }
    chooser.connect_changed(glib::clone!(
        #[weak]
        dialog,
        #[strong(rename_to=select)]
        chooser.selected,
        move || {
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
    .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Profile validation worker failed".into()).profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))?
    .map_err(|detail| layer_ui::ColorFeatureError::Diagnostic(detail).profile_message(workspace.map(|w| w.localization()).as_deref().unwrap_or(localization)))
}

pub(crate) fn read(
    path: &Path,
    location: DocumentLocation,
    policy: layer_ui::PhotoOpenPolicy,
    names: layer_core::DocumentNames,
    cancelled: Arc<AtomicBool>,
) -> Result<(layer_ui::ImportOutcome, Option<DocumentLocation>), String> {
    let file = super::reader::cancellable_file(path, cancelled.clone())
        .map_err(|e| e.to_string())?;
    let imported = layer_ui::read_import(file, layer_ui::ImportIntent::Open, policy,
        names, Default::default(), Default::default(), &cancelled)?;
    let location = match &imported {
        layer_ui::ImportOutcome::Editable(imported) => imported.source.adoption_location(Some(location)),
        layer_ui::ImportOutcome::Package(_) => None,
    };
    Ok((imported, location))
}

pub(crate) async fn show_package(
    window: &adw::ApplicationWindow,
    package: layer_ui::PackageView,
    original: Option<PathBuf>,
    localization: &layer_ui::Localizer,
) -> Result<(), String> {
    loop {
        let summary = package.summary(localization);
        let dialog = adw::AlertDialog::builder().heading(summary.status.as_ref())
            .body(&summary.reason).prefer_wide_layout(true).build();
        dialog.set_widget_name("preserved-package-preview");
        if let Some(preview) = package.preview() {
            let [width,height] = preview.size();
            let texture = gtk::gdk::MemoryTexture::new(width as i32,height as i32,
                gtk::gdk::MemoryFormat::R8g8b8a8,&glib::Bytes::from_owned(preview.pixels().clone()),width as usize*4);
            let picture = gtk::Picture::for_paintable(&texture);
            picture.set_can_shrink(true);
            picture.set_size_request(320,240);
            dialog.set_extra_child(Some(&picture));
        }
        dialog.add_responses(&[("close",summary.close.as_ref()),("copy",summary.copy_original.as_ref())]);
        if summary.capabilities.export { dialog.add_response("export", &summary.export_preview); }
        dialog.set_close_response("close");
        let response = crate::alert::choose(dialog,window).await;
        let preview = response == "export";
        if response != "copy" && !preview { return Ok(()); }
        let title = if preview { &summary.export_preview } else { &summary.copy_original };
        let chooser = gtk::FileDialog::builder().title(title.as_ref()).initial_name(if preview { "Preview.png" } else { "Copy.capy" }).build();
        let folder = if preview { super::chooser::Folder::Export } else { super::chooser::Folder::Save };
        let file = match super::chooser::save(&chooser,window,folder).await {
            Ok(file) => file, Err(_) => continue,
        };
        let path = file.path().ok_or_else(|| layer_ui::DocumentHostError::ChooseDeviceFile.message(localization))?;
        let copy = package.clone();
        let original = original.clone();
        let refusal = summary.destination_error.clone();
        gio::spawn_blocking(move || write_package(&copy, original.as_deref(), &path, preview, &refusal))
            .await.map_err(|_| layer_ui::DocumentHostError::ProjectWriterFailed.message(localization))??;
    }
}

fn write_package(package: &layer_ui::PackageView, original: Option<&Path>, destination: &Path, preview: bool, refusal: &str) -> Result<(), String> {
    if preview && original.is_some_and(|source| same_file(source, destination)) {
        return Err(refusal.to_string());
    }
    super::atomic_write(destination,|file| {
        let cancelled = AtomicBool::new(false);
        if preview { package.export_preview(file, &cancelled) } else { package.copy_original(file, &cancelled) }
    })
}

fn same_file(source: &Path, destination: &Path) -> bool {
    if source == destination { return true; }
    if let (Ok(source), Ok(destination)) = (source.canonicalize(), destination.canonicalize()) {
        if source == destination { return true; }
    }
    use std::os::unix::fs::MetadataExt;
    match (source.metadata(), destination.metadata()) {
        (Ok(source), Ok(destination)) => source.dev() == destination.dev() && source.ino() == destination.ino(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::{ColorProfile, SampleDepth, source::*};
    #[test]
    fn preview_destination_identity_detects_source_aliases() {
        let directory = std::env::temp_dir().join(format!("capy-preview-destination-{}", layer_core::PortableId::random()));
        std::fs::create_dir_all(&directory).unwrap();
        let original = directory.join("Original.capy");
        std::fs::write(&original, b"retained source package").unwrap();
        let symbolic = directory.join("Symbolic.png");
        let linked = directory.join("Linked.png");
        std::os::unix::fs::symlink(&original, &symbolic).unwrap();
        std::fs::hard_link(&original, &linked).unwrap();
        let backing = layer_core::package::ImmutableBacking::new(Arc::new(layer_core::package::transport::ChunkedBytes::new(vec![Arc::from(b"retained source package".as_slice())]).unwrap())).unwrap();
        let preview = layer_core::package::preview::Preview::from_rgba([1,1], Arc::from([10,20,30,255])).unwrap();
        let package = layer_ui::PackageView::new(layer_core::package::codec::OpenOutcome::Preserved {
            source: backing, preview: Some(preview.clone()), outputs: Vec::new(), reason: "unsupported artwork".into(),
        }).unwrap();
        let refusal = package.summary(&layer_ui::Localizer::new(layer_ui::UiLanguage::English)).destination_error;
        for destination in [&original, &symbolic, &linked] {
            assert_eq!(write_package(&package, Some(&original), destination, true, &refusal), Err(refusal.to_string()));
            assert_eq!(std::fs::read(&original).unwrap(), b"retained source package");
        }
        let separate = directory.join("Preview.png");
        write_package(&package, Some(&original), &separate, true, &refusal).unwrap();
        assert_eq!(std::fs::read(separate).unwrap(), preview.encoded().as_ref());
        let copy = directory.join("Copy.capy");
        write_package(&package, Some(&original), &copy, false, &refusal).unwrap();
        assert_eq!(std::fs::read(copy).unwrap(), b"retained source package");
        assert_eq!(std::fs::read(&original).unwrap(), b"retained source package");
        std::fs::remove_dir_all(directory).unwrap();
    }
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
        let layer_ui::ImportOutcome::Editable(imported) = project else { panic!("Photo was not editable") };
        assert_eq!(imported.project.composition().color, Default::default());
        assert!(imported.project.artwork.paint.iter().find_map(|(_, _, source)| source.original.as_ref()).unwrap().interpretation.profile_assumed);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
