//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_core::color::{
    ConversionOptions, IntegerDepth, OutputDither, OutputEncoding, ProfileChannels,
    RenderingIntent, RgbSpace,
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotRenderer};
use std::sync::{Arc, Mutex};

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
    snapshot: DocumentExport,
    recipe: ExportRecipe,
    path: &std::path::Path,
    job: &ExportJob,
) -> Result<u64, String> {
    let result = (|| {
        recipe.validate()?;
        let extent = recipe.size.extent([
            snapshot.project.document.width,
            snapshot.project.document.height,
        ])?;
        let mut renderer = SnapshotRenderer::with_control(
            snapshot.project,
            snapshot.background,
            snapshot.time,
            Default::default(),
            job.control.clone(),
        )
        .map_err(|e| e.to_string())?;
        renderer.set_output_extent(extent)?;
        let target = recipe.interpretation();
        let mut clipped = 0;
        layer_core::atomic_write_checked(
            path,
            |file| {
                let statistics = match recipe.format {
                    ExportFormat::Png => renderer.write_png(
                        file,
                        &target,
                        recipe.encoding,
                        recipe.background.matte(),
                    ),
                    ExportFormat::Tiff => renderer.write_tiff(
                        file,
                        &target,
                        recipe.encoding,
                        recipe.background.matte(),
                    ),
                    ExportFormat::Jpeg => renderer.write_jpeg(
                        file,
                        &target,
                        recipe.encoding,
                        recipe
                            .background
                            .matte()
                            .ok_or("Choose a JPEG background")?,
                        recipe.jpeg_quality,
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

async fn choose_recipe(w: &Workspace, snapshot: &DocumentExport) -> Option<ExportRecipe> {
    let document = snapshot.project.document.color;
    let extent = [
        snapshot.project.document.width,
        snapshot.project.document.height,
    ];
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
    let size = combo(
        &group,
        "Size",
        "export-size",
        &["Original size", "Fit within"],
    );
    let dimensions: [adw::SpinRow; 2] = std::array::from_fn(|i| {
        let row = adw::SpinRow::with_range(1., 32768., 1.);
        row.set_title(["Maximum width (px)", "Maximum height (px)"][i]);
        row.set_widget_name(["export-width", "export-height"][i]);
        row.set_value(f64::from(extent[i]));
        row.set_snap_to_ticks(true);
        row.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
        row.set_visible(false);
        group.add(&row);
        row
    });
    let enlarge = adw::SwitchRow::builder()
        .title("Allow enlargement")
        .visible(false)
        .build();
    enlarge.set_widget_name("export-enlarge");
    group.add(&enlarge);
    let size_note = gtk::Label::builder().wrap(true).xalign(0.).build();
    size_note.set_widget_name("export-size-description");
    size_note.add_css_class("dim-label");
    let output_size: Rc<dyn Fn() -> layer_ui::ExportSize> = Rc::new({
        let size = size.downgrade();
        let dimensions = dimensions.each_ref().map(|row| row.downgrade());
        let enlarge = enlarge.downgrade();
        move || {
            if size.upgrade().is_none_or(|size| size.selected() == 0) {
                layer_ui::ExportSize::Original
            } else {
                layer_ui::ExportSize::Fit {
                    bounds: dimensions
                        .each_ref()
                        .map(|row| row.upgrade().map_or(1, |r| r.value() as u32)),
                    enlarge: enlarge.upgrade().is_some_and(|row| row.is_active()),
                }
            }
        }
    });
    let refresh_size: Rc<dyn Fn()> = Rc::new({
        let size_note = size_note.downgrade();
        let output_size = output_size.clone();
        let dimensions = dimensions.each_ref().map(|row| row.downgrade());
        let enlarge = enlarge.downgrade();
        move || {
            let Some(size_note) = size_note.upgrade() else {
                return;
            };
            let choice = output_size();
            let fitted = matches!(choice, layer_ui::ExportSize::Fit { .. });
            for row in dimensions.iter().filter_map(|row| row.upgrade()) {
                row.set_visible(fitted);
            }
            if let Some(enlarge) = enlarge.upgrade() {
                enlarge.set_visible(fitted);
            }
            match choice.extent(extent) {
                Ok([width, height]) => {
                    size_note.set_label(&format!("Output: {width} × {height} px"))
                }
                Err(error) => size_note.set_label(&error),
            }
        }
    });
    size.connect_selected_notify({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    for row in &dimensions {
        row.connect_value_notify({
            let refresh = refresh_size.clone();
            move |_| refresh()
        });
    }
    enlarge.connect_active_notify({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    refresh_size();
    let format = combo(&group, "Format", "export-format", &["PNG", "TIFF", "JPEG"]);
    let space = combo(
        &group,
        "Color space",
        "export-space",
        &[
            "sRGB",
            "Display P3",
            "Adobe RGB (1998)",
            "ProPhoto RGB",
            "Custom ICC",
        ],
    );
    let profile = ProfileChooser::new(&w.window, &space, document.space, ProfilePurpose::Output);
    group.add(&profile.row);
    let selected_profile = profile.selected.clone();
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
    let quality = adw::SpinRow::with_range(1., 100., 1.);
    quality.set_title("JPEG quality");
    quality.set_widget_name("export-jpeg-quality");
    quality.set_value(90.);
    quality.set_snap_to_ticks(true);
    quality.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    quality.set_visible(false);
    group.add(&quality);
    let advanced_group = adw::PreferencesGroup::new();
    let advanced = adw::ExpanderRow::builder().title("Advanced color").build();
    advanced.set_widget_name("export-advanced");
    advanced_group.add(&advanced);
    let intent = choice(
        "Rendering intent",
        "export-intent",
        &[
            "Relative colorimetric",
            "Perceptual",
            "Saturation",
            "Absolute colorimetric",
        ],
    );
    advanced.add_row(&intent);
    let bpc = adw::SwitchRow::builder()
        .title("Black point compensation")
        .active(true)
        .build();
    bpc.set_widget_name("export-bpc");
    advanced.add_row(&bpc);
    intent.connect_selected_notify(glib::clone!(
        #[weak]
        bpc,
        move |intent| {
            // ICC absolute intent preserves media white/black instead of adapting
            // to the destination's endpoints. Remember the switch for other intents.
            bpc.set_sensitive(intent.selected() != 3);
        }
    ));
    let dither = adw::SwitchRow::builder()
        .title("Reduce banding")
        .subtitle("Dither 8-bit gradients")
        .build();
    dither.set_widget_name("export-dither");
    advanced.add_row(&dither);
    depth.connect_selected_notify(glib::clone!(
        #[weak]
        dither,
        move |depth| {
            dither.set_sensitive(depth.selected() == 0);
        }
    ));
    let jpeg_hint = gtk::Label::builder()
        .label("JPEG uses 8-bit color and needs an opaque background.")
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    jpeg_hint.add_css_class("dim-label");
    format.connect_selected_notify(glib::clone!(
        #[weak]
        depth,
        #[weak]
        background,
        #[weak]
        quality,
        #[weak]
        jpeg_hint,
        move |format| {
            let jpeg = format.selected() == 2;
            quality.set_visible(jpeg);
            jpeg_hint.set_visible(jpeg);
            depth.set_sensitive(!jpeg);
            if jpeg {
                depth.set_selected(0);
                if background.selected() == 0 {
                    background.set_selected(1);
                }
            }
        }
    ));
    let validation = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    validation.add_css_class("error");
    validation.set_widget_name("export-validation");
    background.connect_selected_notify(glib::clone!(
        #[weak]
        format,
        #[weak]
        space,
        #[weak]
        validation,
        #[weak]
        dialog,
        #[strong]
        selected_profile,
        move |background| {
            let result = selected_profile(space.selected()).and_then(|profile| {
                if format.selected() == 0 && profile.channels == ProfileChannels::Cmyk {
                    Err("Choose TIFF or JPEG for a CMYK profile".into())
                } else if background.selected() == 0
                    && (format.selected() == 2 || profile.channels == ProfileChannels::Cmyk)
                {
                    Err("Choose a background for this output".into())
                } else {
                    Ok(())
                }
            });
            dialog.set_response_enabled("export", result.is_ok());
            validation.set_label(result.as_ref().err().map_or("", String::as_str));
            validation.set_visible(result.is_err());
        }
    ));
    space.connect_selected_notify(glib::clone!(
        #[weak]
        background,
        #[weak]
        format,
        #[strong]
        selected_profile,
        move |space| {
            if selected_profile(space.selected()).is_ok_and(|p| p.channels == ProfileChannels::Cmyk)
            {
                if format.selected() == 0 {
                    format.set_selected(1);
                }
                if background.selected() == 0 {
                    background.set_selected(1);
                }
            }
            background.notify("selected");
        }
    ));
    format.connect_selected_notify(glib::clone!(
        #[weak]
        background,
        move |_| {
            background.notify("selected");
        }
    ));
    let updating = Rc::new(std::cell::Cell::new(false));
    preset.connect_selected_notify(glib::clone!(
        #[weak]
        format,
        #[weak]
        size,
        #[weak]
        space,
        #[weak]
        depth,
        #[weak]
        background,
        #[weak]
        intent,
        #[weak]
        bpc,
        #[weak]
        dither,
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
            size.set_selected(0);
            format.set_selected(u32::from(recipe.format == ExportFormat::Tiff));
            space.set_selected(
                RgbSpace::ALL
                    .iter()
                    .position(|s| {
                        recipe.profile.profile == layer_core::color::ColorProfile::Builtin(*s)
                    })
                    .unwrap() as u32,
            );
            depth.set_selected(u32::from(recipe.depth == IntegerDepth::U16));
            background.set_selected(0);
            intent.set_selected(0);
            bpc.set_active(true);
            dither.set_active(false);
            updating.set(false);
        }
    ));
    for row in [&format, &space, &depth, &background, &intent, &size] {
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
    for row in [&bpc, &dither] {
        row.connect_active_notify(glib::clone!(
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
        glib::clone!(
            #[weak]
            note,
            #[weak]
            space,
            move |depth: &adw::ComboRow| {
                note.set_label(if depth.selected() == 0 && document.depth == IntegerDepth::U16 {
                "This copy reduces 16-bit artwork to 8-bit. The master retains its precision."
            } else if depth.selected() == 0 && space.selected() == 3 {
                "16-bit is recommended for ProPhoto RGB gradients and further editing."
            } else {
                "The matching color profile is embedded in the image."
            });
            }
        );
    update_note(&depth);
    depth.connect_selected_notify(update_note);
    space.connect_selected_notify(glib::clone!(
        #[weak]
        depth,
        move |_| {
            depth.notify("selected");
        }
    ));
    let read_recipe: Rc<dyn Fn() -> Result<ExportRecipe, String>> = Rc::new(glib::clone!(
        #[weak]
        format,
        #[weak]
        space,
        #[weak]
        depth,
        #[weak]
        background,
        #[weak]
        quality,
        #[weak]
        intent,
        #[weak]
        bpc,
        #[weak]
        dither,
        #[strong]
        selected_profile,
        #[strong]
        output_size,
        #[upgrade_or]
        Err("Export options closed".into()),
        move || {
            let recipe = ExportRecipe {
                size: output_size(),
                format: match format.selected() {
                    1 => ExportFormat::Tiff,
                    2 => ExportFormat::Jpeg,
                    _ => ExportFormat::Png,
                },
                profile: selected_profile(space.selected())?,
                depth: if depth.selected() == 0 {
                    IntegerDepth::U8
                } else {
                    IntegerDepth::U16
                },
                background: match background.selected() {
                    1 => ExportBackground::White,
                    2 => ExportBackground::Black,
                    _ => ExportBackground::Preserve,
                },
                jpeg_quality: quality.value() as u8,
                encoding: OutputEncoding {
                    conversion: ConversionOptions {
                        intent: match intent.selected() {
                            1 => RenderingIntent::Perceptual,
                            2 => RenderingIntent::Saturation,
                            3 => RenderingIntent::AbsoluteColorimetric,
                            _ => RenderingIntent::RelativeColorimetric,
                        },
                        black_point_compensation: bpc.is_active() && intent.selected() != 3,
                    },
                    dither: if depth.selected() == 0 && dither.is_active() {
                        OutputDither::Stochastic8
                    } else {
                        OutputDither::None
                    },
                },
            };
            recipe.validate()?;
            Ok(recipe)
        }
    ));
    let comparison =
        super::preview::Comparison::for_output(snapshot.project.clone(), w.view_color());
    let compression_note = gtk::Label::builder()
        .label(
            "JPEG preview includes size, color and background. Compression artifacts are excluded.",
        )
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    compression_note.set_widget_name("export-preview-compression");
    compression_note.add_css_class("dim-label");
    format.connect_selected_notify(glib::clone!(
        #[weak]
        compression_note,
        move |row| {
            compression_note.set_visible(row.selected() == 2);
        }
    ));
    let refresh_preview: Rc<dyn Fn()> = Rc::new({
        let snapshot = DocumentExport::clone(snapshot);
        glib::clone!(
            #[weak]
            comparison,
            #[strong]
            read_recipe,
            #[strong]
            updating,
            move || {
                if updating.get() {
                    return;
                }
                match read_recipe() {
                    Ok(recipe) => comparison.request_output(&snapshot, recipe),
                    Err(error) => comparison.invalidate(&error),
                }
            }
        )
    });
    for row in [
        &preset,
        &size,
        &format,
        &space,
        &depth,
        &background,
        &intent,
    ] {
        row.connect_selected_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    for row in [&bpc, &dither, &enlarge] {
        row.connect_active_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    // Quality affects excluded JPEG compression, so it cannot change these pixels.
    for row in &dimensions {
        row.connect_value_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    dialog.connect_response(
        None,
        glib::clone!(
            #[weak]
            comparison,
            move |_, _| comparison.close()
        ),
    );
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&jpeg_hint);
    content.append(&profile.error);
    content.append(&validation);
    content.append(&note);
    content.append(&advanced_group);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(430)
        .child(&content)
        .build();
    scroll.set_widget_name("export-scroll");
    let extra = gtk::Box::new(gtk::Orientation::Vertical, 8);
    extra.append(&comparison.widget);
    extra.append(&compression_note);
    extra.append(&scroll);
    extra.append(&size_note);
    dialog.set_extra_child(Some(&extra));
    dialog.add_responses(&[("cancel", "Cancel"), ("export", "Choose file…")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("export"));
    dialog.set_response_appearance("export", adw::ResponseAppearance::Suggested);
    refresh_preview();
    let response = crate::alert::choose(dialog, &w.window).await;
    comparison.close();
    comparison.finish().await;
    if response != "export" {
        return None;
    }
    read_recipe().ok()
}

pub(super) async fn run(w: &Rc<Workspace>, id: u32, name: &str) -> Result<bool, String> {
    // The preview and final file share one immutable artwork revision and time.
    let snapshot = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .capture_project_export(id)?;
    let Some(recipe) = choose_recipe(w, &snapshot).await else {
        return Ok(false);
    };
    recipe.validate()?;
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
    if recipe.format == ExportFormat::Jpeg {
        filter.add_suffix("jpeg");
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
        && !(recipe.format == ExportFormat::Jpeg && extension.eq_ignore_ascii_case("jpeg"))
    {
        return Err(format!(
            "Use a .{} filename for this image format.",
            recipe.format.extension()
        ));
    }
    let height = recipe.size.extent([
        snapshot.project.document.width,
        snapshot.project.document.height,
    ])?[1];
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
