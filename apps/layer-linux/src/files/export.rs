//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_core::color::{DocumentColor, IntegerDepth, RgbSpace};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotRenderer};
use std::sync::{Arc, Mutex};

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
    snapshot: DocumentExport,
    recipe: ExportRecipe,
    path: &std::path::Path,
    job: &ExportJob,
) -> Result<u64, String> {
    let result = (|| {
        let mut renderer = SnapshotRenderer::with_control(
            snapshot.project,
            snapshot.background,
            snapshot.time,
            Default::default(),
            job.control.clone(),
        )
        .map_err(|e| e.to_string())?;
        let target = recipe.interpretation();
        let mut clipped = 0;
        layer_core::atomic_write_checked(
            path,
            |file| {
                let statistics = match recipe.format {
                    ExportFormat::Png => renderer.write_png(
                        file,
                        &target,
                        Default::default(),
                        recipe.background.matte(),
                    ),
                    ExportFormat::Tiff => renderer.write_tiff(
                        file,
                        &target,
                        Default::default(),
                        recipe.background.matte(),
                    ),
                }?;
                clipped = statistics.clipped_channels;
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
    let row = adw::ComboRow::builder().title(title).build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_use_subtitle(true);
    row.set_model(Some(&gtk::StringList::new(values)));
    row.set_widget_name(name);
    group.add(&row);
    row
}

async fn choose_recipe(w: &Workspace, document: DocumentColor) -> Option<ExportRecipe> {
    let dialog = adw::AlertDialog::builder()
        .heading("Export image")
        .body("Create a profiled copy. Your editable drawing keeps its color space and bit depth.")
        .prefer_wide_layout(true)
        .build();
    dialog.set_widget_name("export-options");
    let group = adw::PreferencesGroup::new();
    let preset = combo(
        &group,
        "Preset",
        "export-preset",
        &[
            "Web / Share",
            "Wide-color image",
            "Further editing",
            "Custom",
        ],
    );
    let format = combo(&group, "Format", "export-format", &["PNG", "TIFF"]);
    let space = combo(
        &group,
        "Color space",
        "export-space",
        &RgbSpace::ALL.map(RgbSpace::name),
    );
    let depth = combo(
        &group,
        "Bit depth",
        "export-depth",
        &["8-bit SDR", "16-bit SDR"],
    );
    let background = combo(
        &group,
        "Transparency",
        "export-background",
        &["Keep transparency", "White background", "Black background"],
    );
    let updating = Rc::new(std::cell::Cell::new(false));
    preset.connect_selected_notify(glib::clone!(
        #[weak]
        format,
        #[weak]
        space,
        #[weak]
        depth,
        #[weak]
        background,
        #[strong]
        updating,
        move |preset| {
            let recipe = match preset.selected() {
                0 => ExportRecipe::web_share(),
                1 => ExportRecipe::wide_color(),
                2 => ExportRecipe::further_editing(document),
                _ => return,
            };
            updating.set(true);
            format.set_selected(u32::from(recipe.format == ExportFormat::Tiff));
            space.set_selected(
                RgbSpace::ALL
                    .iter()
                    .position(|s| *s == recipe.color.space)
                    .unwrap() as u32,
            );
            depth.set_selected(u32::from(recipe.color.depth == IntegerDepth::U16));
            background.set_selected(0);
            updating.set(false);
        }
    ));
    for row in [&format, &space, &depth, &background] {
        row.connect_selected_notify(glib::clone!(
            #[weak]
            preset,
            #[strong]
            updating,
            move |_| {
                if !updating.get() {
                    preset.set_selected(3);
                }
            }
        ));
    }
    let note = gtk::Label::builder().wrap(true).xalign(0.).build();
    note.add_css_class("dim-label");
    let update_note =
        {
            let note = note.clone();
            let space = space.clone();
            move |depth: &adw::ComboRow| {
                note.set_label(if depth.selected() == 0 && document.depth == IntegerDepth::U16 {
                "This copy reduces 16-bit artwork to 8-bit. The master retains its precision."
            } else if depth.selected() == 0 && space.selected() == 3 {
                "16-bit is recommended for ProPhoto RGB gradients and further editing."
            } else {
                "The matching color profile is embedded in the image."
            });
            }
        };
    update_note(&depth);
    depth.connect_selected_notify(update_note);
    space.connect_selected_notify(glib::clone!(
        #[weak]
        depth,
        move |_| {
            depth.notify("selected");
        }
    ));
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&note);
    dialog.set_extra_child(Some(&content));
    dialog.add_responses(&[("cancel", "Cancel"), ("export", "Choose file…")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("export"));
    dialog.set_response_appearance("export", adw::ResponseAppearance::Suggested);
    if dialog.choose_future(Some(&w.window)).await != "export" {
        return None;
    }
    Some(ExportRecipe {
        format: if format.selected() == 0 {
            ExportFormat::Png
        } else {
            ExportFormat::Tiff
        },
        color: DocumentColor {
            space: RgbSpace::ALL[space.selected() as usize],
            depth: if depth.selected() == 0 {
                IntegerDepth::U8
            } else {
                IntegerDepth::U16
            },
        },
        background: match background.selected() {
            1 => ExportBackground::White,
            2 => ExportBackground::Black,
            _ => ExportBackground::Preserve,
        },
    })
}

pub(super) async fn run(w: &Rc<Workspace>, id: u32, name: &str) -> Result<bool, String> {
    let color = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .engine()
        .document()
        .color;
    let Some(recipe) = choose_recipe(w, color).await else {
        return Ok(false);
    };
    let dialog = gtk::FileDialog::builder()
        .title("Export image")
        .accept_label("Export")
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
    filter.set_name(Some(recipe.format.name()));
    filter.add_suffix(recipe.format.extension());
    if recipe.format == ExportFormat::Tiff {
        filter.add_suffix("tiff");
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    dialog.set_default_filter(Some(&filter));
    let file = match dialog.save_future(Some(&w.window)).await {
        Ok(file) => file,
        Err(e)
            if e.matches(gtk::DialogError::Dismissed) || e.matches(gtk::DialogError::Cancelled) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(e.to_string()),
    };
    let path = file.path().ok_or("Choose a file on this device")?;
    if w.gpu
        .borrow()
        .as_ref()
        .and_then(|g| g.session.state().document_file.location.as_ref())
        .is_some_and(|location| file.equal(&gio::File::for_uri(&location.uri)))
    {
        return Err("Choose a different file to keep the editable drawing.".into());
    }
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case(recipe.format.extension())
        && !(recipe.format == ExportFormat::Tiff && extension.eq_ignore_ascii_case("tiff"))
    {
        return Err(format!(
            "Use a .{} filename for this image format.",
            recipe.format.extension()
        ));
    }
    let snapshot = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .capture_project_export(id)?;
    let height = snapshot.project.document.height;
    let job = ExportJob::default();
    let progress = gtk::ProgressBar::builder()
        .show_text(true)
        .text("Preparing export…")
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading("Exporting image")
        .extra_child(&progress)
        .build();
    dialog.set_widget_name("export-progress");
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
    dialog.connect_response(None, {
        let job = job.clone();
        move |_, _| {
            job.cancel();
        }
    });
    dialog.present(Some(&w.window));
    let timer = glib::timeout_add_local(std::time::Duration::from_millis(50), {
        let job = job.clone();
        move || {
            let rows = job.control.output_rows();
            if rows == 0 {
                progress.pulse();
            } else {
                progress.set_fraction(f64::from(rows) / f64::from(height));
                progress.set_text(Some(if rows == height {
                    "Finishing file…"
                } else {
                    "Writing image…"
                }));
            }
            glib::ControlFlow::Continue
        }
    });
    let result = gio::spawn_blocking({
        let job = job.clone();
        move || write_snapshot(snapshot, recipe, &path, &job)
    })
    .await
    .map_err(|_| "Image export worker failed".to_string());
    timer.remove();
    let cancelled = job.cancelled();
    dialog.force_close();
    if cancelled {
        Ok(false)
    } else {
        result??;
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
