//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::sync::{Arc, Mutex};
mod presets;
mod navigation;
mod dialog;

fn numeric_inputs_ready(rows: &[crate::number_control::NumberControl], commit: bool) -> bool {
    rows.iter().filter(|row| row.is_visible()).all(|row| if commit { row.commit_text() } else { row.input_valid() })
}

use super::profile::{ProfileChooser, ProfilePurpose};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Working,
    Cancelled,
    Publishing,
    Finished,
}

#[derive(Clone)]
pub(crate) struct ExportJob {
    control: CaptureControl,
    phase: Arc<Mutex<Phase>>,
}
impl Default for ExportJob {
    fn default() -> Self {
        Self {
            control: CaptureControl::default(),
            phase: Arc::new(Mutex::new(Phase::Working)),
        }
    }
}
impl ExportJob {
    /// The accepted cancellation and publication decision share one lock.
    /// Once publication starts, the completed file takes precedence.
    pub(crate) fn cancel(&self) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if *phase != Phase::Working {
            return false;
        }
        *phase = Phase::Cancelled;
        self.control.cancel();
        true
    }
    fn publish(&self) -> Result<(), String> {
        let mut phase = self.phase.lock().unwrap();
        if *phase != Phase::Working {
            return Err("Export cancelled".into());
        }
        *phase = Phase::Publishing;
        Ok(())
    }
    fn finish(&self) {
        let mut phase = self.phase.lock().unwrap();
        if *phase != Phase::Cancelled {
            *phase = Phase::Finished;
        }
    }
    fn cancelled(&self) -> bool {
        *self.phase.lock().unwrap() == Phase::Cancelled
    }
}

pub(crate) fn write_snapshot(
    gpu: SnapshotGpu,
    snapshot: DocumentExport,
    recipe: ExportRecipe,
    path: &std::path::Path,
    job: &ExportJob,
) -> Result<u64, layer_ui::ColorFeatureError> {
    let result = (|| {
        recipe.validate_for_composition(snapshot.composition())?;
        let extent = recipe.output_extent([
            snapshot.composition().size[0],
            snapshot.composition().size[1],
        ])?;
        let metadata = recipe.delivery_metadata_for_artwork(&snapshot.capture.artwork)?;
        let mut renderer = gpu.capture(
            snapshot.capture,
            job.control.clone(),
        )
        .map_err(|e| e.to_string())?;
        renderer.set_output_extent(extent)?;
        renderer.set_output_metadata(metadata)?;
        let mut clipped = 0;
        layer_core::atomic_write_checked(
            path,
            |file| {
                clipped = layer_host::export::write_recipe(&mut renderer, file, &recipe).map_err(|reason| reason.diagnostic())?
                    .clipped_channels;
                Ok(())
            },
            || job.publish(),
        )?;
        Ok(clipped)
    })();
    job.finish();
    result
}

fn combo(group: &adw::PreferencesGroup, title: &str, name: &str, values: &[&str]) -> adw::ComboRow {
    let row = choice(title, name, values);
    group.add(&row);
    row
}
fn choice(title: &str, name: &str, values: &[&str]) -> adw::ComboRow {
    let row = adw::ComboRow::builder().title(title).build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_use_subtitle(true);
    row.set_model(Some(&gtk::StringList::new(values)));
    row.set_widget_name(name);
    row
}

struct Choice {
    recipe: ExportRecipe,
    destination: usize,
    library: ExportPresets,
}

pub(super) async fn run(w: &Rc<Workspace>, id: u32, name: &str, owner: u64, repeat: Option<&ExportRepeat>) -> Result<bool, String> {
    w.proof_panel.finish_pending(w).await?;
    // The preview and final file share one immutable artwork revision and time.
    let (mut snapshot, context) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (session.capture_project_export(id)?, session.engine().backend().capture_context()?)
    };
    snapshot = gio::spawn_blocking(move || {
        context.install(&mut snapshot.capture)?;
        snapshot.time = snapshot.output().context.elapsed;
        Ok::<_, String>(snapshot)
    }).await.map_err(|_| "Export capture worker failed".to_string())??;
    let choice = if repeat.is_some() { None } else {
        let Some(choice) = dialog::choose(w, id, &snapshot).await? else { return Ok(false); };
        Some(choice)
    };
    let recipe = repeat.map(|last| last.recipe.clone()).or_else(|| choice.as_ref().map(|choice| choice.recipe.clone())).unwrap();
    recipe.validate().map_err(|reason| reason.message(&w.localization()))?;
    let copy = layer_ui::color_feature_copy::ExportCopy::new(&w.localization());
    let dialog = gtk::FileDialog::builder()
        .title(copy.title.as_ref())
        .accept_label(copy.export.as_ref())
        .modal(true)
        .build();
    dialog.set_initial_name(Some(&recipe.filename(name)));
    if let Some(location) = w
        .gpu
        .borrow()
        .as_ref()
        .and_then(|g| g.session.state().document_file.location.as_ref())
        && let Some(folder) = gio::File::for_uri(&location.uri).parent()
    {
        dialog.set_initial_folder(Some(&folder));
    }
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&recipe.format.localized_name(&w.localization())));
    filter.add_suffix(recipe.format.extension());
    if recipe.format == ExportFormat::Tiff {
        filter.add_suffix("tiff");
    }
    if recipe.format == ExportFormat::Jpeg {
        filter.add_suffix("jpeg");
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let retained = if let Some(last) = repeat {
        let file = gio::File::for_uri(&last.location.uri);
        let path = file.path();
        let writable = gio::spawn_blocking(move || path.is_some_and(|path| std::fs::OpenOptions::new().write(true).open(path).is_ok()))
            .await.unwrap_or(false);
        writable.then_some(file)
    } else { None };
    let file = if let Some(file) = retained { file } else { match super::chooser::save(&dialog, &w.window, super::chooser::Folder::Export).await {
        Ok(file) => file,
        Err(e)
            if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(e.to_string()),
    }};
    let path = file.path().ok_or_else(||layer_ui::DocumentHostError::ChooseDeviceFile.message(&w.localization()))?;
    let master = w.gpu.borrow().as_ref().and_then(|g| g.session.state().document_file.location.as_ref())
        .and_then(|location| gio::File::for_uri(&location.uri).path());
    let location = DocumentLocation { uri: file.uri().into(), name: path.file_name().unwrap_or_default().to_string_lossy().into_owned() };
    w.gpu.borrow_mut().as_mut().ok_or("Canvas unavailable")?.session.prepare_export(id, owner, recipe.clone(), location)?;
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case(recipe.format.extension())
        && !(recipe.format == ExportFormat::Tiff && extension.eq_ignore_ascii_case("tiff"))
        && !(recipe.format == ExportFormat::Jpeg && extension.eq_ignore_ascii_case("jpeg"))
    {
        return Err(layer_ui::DocumentDeliveryMessage::ExportExtension {extension: recipe.format.extension().into()}.message(&w.localization()));
    }
    let height = recipe.output_extent([
        snapshot.composition().size[0],
        snapshot.composition().size[1],
    ]).map_err(|reason| reason.message(&w.localization()))?[1];
    let gpu = w.snapshot_gpu()?;
    let job = ExportJob::default();
    let copy = layer_ui::color_feature_copy::ExportCopy::new(&w.localization());
    let progress = gtk::ProgressBar::builder()
        .show_text(true)
        .text(copy.preparing.as_ref())
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading(copy.exporting.as_ref())
        .extra_child(&progress)
        .build();
    dialog.set_widget_name("export-progress");
    dialog.add_response("cancel", copy.common.cancel.as_ref());
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let job = job.clone();
        move |_, _| {
            job.cancel();
        }
    });
    let progress_copy = Rc::new(std::cell::RefCell::new(copy));
    w.on_localization(glib::clone!(#[weak] dialog, #[weak] progress, #[strong] job, #[strong] progress_copy, #[upgrade_or] false, move |localization| {
        *progress_copy.borrow_mut() = layer_ui::color_feature_copy::ExportCopy::new(localization);
        let copy = progress_copy.borrow();
        dialog.set_heading(Some(&copy.exporting));
        dialog.set_response_label("cancel", &copy.common.cancel);
        let rows = job.control.output_rows();
        progress.set_text(Some(if rows == 0 { &copy.preparing } else if rows == height { &copy.finishing_file } else { &copy.writing_image }));
        true
    }));
    crate::page_dialog::present_intent(&dialog, &w.window, progress_copy.borrow().progress_dialog);
    let timer = glib::timeout_add_local(std::time::Duration::from_millis(50), glib::clone!(#[strong] progress_copy, #[strong] job, #[weak] progress, #[upgrade_or] glib::ControlFlow::Break,
        move || {
            let copy = progress_copy.borrow();
            let rows = job.control.output_rows();
            if rows == 0 {
                progress.pulse();
            } else {
                progress.set_fraction(f64::from(rows) / f64::from(height));
                progress.set_text(Some(if rows == height {
                    copy.finishing_file.as_ref()
                } else {
                    copy.writing_image.as_ref()
                }));
            }
            glib::ControlFlow::Continue
        }
    ));
    let remembered = recipe.clone();
    let master_refusal = layer_ui::color_feature_copy::DocumentColorCopy::new(&w.localization()).choose_different.to_string();
    let result = gio::spawn_blocking({
        let job = job.clone();
        move || {
            if master.as_ref().is_some_and(|master| super::open::same_file(master, &path)) { return Err(layer_ui::ColorFeatureError::Diagnostic(master_refusal)); }
            write_snapshot(gpu, snapshot, recipe, &path, &job)
        }
    })
    .await
    .map_err(|_| layer_ui::ColorFeatureError::Diagnostic("Image export worker failed".into()).message(&w.localization()));
    timer.remove();
    let cancelled = job.cancelled();
    dialog.force_close();
    if cancelled {
        Ok(false)
    } else {
        result?.map_err(|reason| reason.message(&w.localization()))?;
        if let Some(choice) = choice {
            let mut next = choice.library.clone();
            let saved = async {
                next.remember(choice.destination.min(3), remembered)?;
                if next != choice.library { presets::save(choice.library, next).await?; }
                Ok::<_, layer_ui::ColorFeatureError>(())
            }.await;
            if let Err(reason) = saved {
                w.changed(Err(layer_ui::DocumentDeliveryMessage::ExportPreferences {detail: reason.preset_message(&w.localization())}.message(&w.localization())));
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_and_publication_have_one_decision_boundary() {
        let cancelled = ExportJob::default();
        assert!(cancelled.cancel());
        assert!(cancelled.control.is_cancelled());
        assert!(cancelled.publish().is_err());
        cancelled.finish();
        assert!(cancelled.cancelled());
        let completed = ExportJob::default();
        completed.publish().unwrap();
        assert!(!completed.cancel());
        assert!(!completed.control.is_cancelled());
        completed.finish();
        assert!(!completed.cancelled());
        assert!(!completed.cancel());
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::*;
    #[test]
    #[ignore = "native GTK display"]
    fn native_export_numeric_commit() {
        gtk::init().unwrap();
        for scheme in [adw::ColorScheme::ForceLight, adw::ColorScheme::ForceDark] {
            adw::StyleManager::default().set_color_scheme(scheme);
            let specs = layer_ui::ExportNumericControls::default();
            for (spec, valid, invalid) in [(specs.dimension.clone(), "2048", "invalid"), (specs.dimension, "1080", "invalid"), (specs.ppi, "300", "１／０"), (specs.quality, "90", "NaN")] {
                let row = crate::number_control::NumberControl::panel(spec, "Export", layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
                row.set_value(32.);
                let window = gtk::Window::builder().child(&row).build();
                window.present();
                fn descendant<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
                    if let Ok(entry) = widget.clone().downcast::<T>() { return Some(entry); }
                    let mut child = widget.first_child();
                    while let Some(widget) = child { if let Some(found) = descendant::<T>(&widget) { return Some(found); } child = widget.next_sibling(); }
                    None
                }
                let input = descendant::<gtk::Entry>(row.upcast_ref()).unwrap();
                let stack = descendant::<gtk::Stack>(row.upcast_ref()).unwrap();
                stack.set_visible_child_name("entry");
                input.set_text(valid);
                assert!(numeric_inputs_ready(std::slice::from_ref(&row), true));
                let accepted = row.value();
                stack.set_visible_child_name("entry");
                input.set_text(&valid.chars().map(|digit| char::from_u32(digit as u32 + 0xfee0).unwrap()).collect::<String>());
                assert!(numeric_inputs_ready(std::slice::from_ref(&row), true));
                assert_eq!(row.value(), accepted);
                stack.set_visible_child_name("entry");
                input.set_text(invalid);
                assert!(!numeric_inputs_ready(std::slice::from_ref(&row), true));
                assert_eq!(row.value(), accepted);
                row.set_visible(false);
                assert!(numeric_inputs_ready(std::slice::from_ref(&row), false));
                row.set_visible(true);
                stack.set_visible_child_name("entry");
                input.set_text(valid);
                input.delegate().and_downcast::<gtk::Text>().unwrap().emit_preedit_changed("にほんご");
                assert!(!numeric_inputs_ready(std::slice::from_ref(&row), true));
                assert_eq!(row.value(), accepted);
                window.close();
            }
        }
    }
}
