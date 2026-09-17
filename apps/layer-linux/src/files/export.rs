//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_core::color::{
    ConversionOptions, SampleDepth, OutputDither, OutputEncoding, RenderingIntent,
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::sync::{Arc, Mutex};
mod presets;
const FORMATS: [ExportFormat; 5] = [ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg, ExportFormat::PngHdr, ExportFormat::PngHdrMapped];

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
) -> Result<u64, String> {
    let result = (|| {
        recipe.validate()?;
        let extent = recipe.size.extent([
            snapshot.project.document.width,
            snapshot.project.document.height,
        ])?;
        let resolution = recipe.output_resolution(snapshot.project.document.resolution)?;
        let mut renderer = gpu.capture(
            snapshot.project,
            snapshot.background,
            snapshot.time,
            Default::default(),
            job.control.clone(),
        )
        .map_err(|e| e.to_string())?;
        renderer.set_output_extent(extent)?;
        renderer.set_output_resolution(resolution)?;
        let target = recipe.interpretation();
        let mut clipped = 0;
        layer_core::atomic_write_checked(
            path,
            |file| {
                let statistics = match recipe.format {
                    ExportFormat::PngHdr | ExportFormat::PngHdrMapped => renderer.write_hdr_png(file, recipe.format.maps_hdr_range()),
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

struct Choice {
    recipe: ExportRecipe,
    destination: usize,
    library: ExportPresets,
}

async fn choose_recipe(w: &Rc<Workspace>, snapshot: &DocumentExport) -> Result<Option<Choice>, String> {
    let document = snapshot.project.document.color;
    let master_resolution = snapshot.project.document.resolution;
    let library = Rc::new(std::cell::RefCell::new(presets::load(document).await?));
    let destination = Rc::new(std::cell::Cell::new(0usize));
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
    let preset_group = adw::PreferencesGroup::new();
    let preset = combo(
        &preset_group,
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
    let resolution = combo(
        &group,
        "Resolution metadata",
        "export-resolution",
        &["From master", "Custom", "Omit"],
    );
    let ppi = adw::SpinRow::with_range(1., 65535., 1.);
    ppi.set_title("Pixels per inch");
    ppi.set_widget_name("export-ppi");
    ppi.set_value(300.);
    ppi.set_snap_to_ticks(true);
    ppi.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    group.add(&ppi);
    let chosen_resolution: Rc<dyn Fn() -> ExportResolution> = Rc::new(glib::clone!(
        #[weak]
        resolution,
        #[weak]
        ppi,
        #[upgrade_or]
        ExportResolution::Omit,
        move || match resolution.selected() {
            1 => ExportResolution::Ppi(ppi.value() as u32),
            2 => ExportResolution::Omit,
            _ => ExportResolution::Master,
        }
    ));
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
        let chosen_resolution = chosen_resolution.clone();
        let ppi = ppi.downgrade();
        let dimensions = dimensions.each_ref().map(|row| row.downgrade());
        let enlarge = enlarge.downgrade();
        move || {
            let Some(size_note) = size_note.upgrade() else {
                return;
            };
            let choice = output_size();
            let resolution = chosen_resolution();
            if let Some(ppi) = ppi.upgrade() {
                ppi.set_visible(matches!(resolution, ExportResolution::Ppi(_)));
            }
            let physical = match resolution {
                ExportResolution::Master => master_resolution,
                ExportResolution::Ppi(value) => Some(layer_core::ImageResolution::ppi(value)),
                ExportResolution::Omit => None,
            };
            let fitted = matches!(choice, layer_ui::ExportSize::Fit { .. });
            for row in dimensions.iter().filter_map(|row| row.upgrade()) {
                row.set_visible(fitted);
            }
            if let Some(enlarge) = enlarge.upgrade() {
                enlarge.set_visible(fitted);
            }
            match choice.extent(extent) {
                Ok([width, height]) => {
                    let metadata = physical.map_or_else(
                        || "No physical resolution specified".into(),
                        |r| {
                            let [x, y] = r.pixels_per_inch();
                            format!(
                                "{:.2} × {:.2} cm · {:.2} × {:.2} ppi",
                                f64::from(width) / x * 2.54,
                                f64::from(height) / y * 2.54,
                                x,
                                y
                            )
                        },
                    );
                    size_note.set_label(&format!("Output: {width} × {height} px\n{metadata}"))
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
    resolution.connect_selected_notify({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    ppi.connect_value_notify({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    refresh_size();
    let format = combo(&group, "Format", "export-format", if document.depth.is_float() { &["SDR PNG", "SDR TIFF", "SDR JPEG", "HDR PNG · preserve PQ range", "HDR PNG · map to PQ range"] } else { &["PNG", "TIFF", "JPEG"] });
    let profile = ProfileChooser::new(w, "Delivery profile", "export-space", document.space, ProfilePurpose::Output);
    let space = profile.row.clone();
    group.add(&space);
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
        .subtitle("Currently unavailable")
        .active(false)
        .sensitive(false)
        .build();
    bpc.set_widget_name("export-bpc");
    advanced.add_row(&bpc);
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
    format.connect_selected_notify(glib::clone!(#[weak] space, #[weak] depth, #[weak] background, #[weak] advanced,
        move |format| { let hdr=format.selected()>=3; space.set_visible(!hdr); depth.set_visible(!hdr); background.set_visible(!hdr); advanced.set_visible(!hdr); }));
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
            let recipe = ExportRecipe {
                depth: if depth.selected() == 0 { SampleDepth::U8 } else { SampleDepth::U16 },
                background: [ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black][background.selected() as usize],
                ..ExportRecipe::web_share()
            };
            let draft = recipe.draft(ExportDraftAction::Format(FORMATS[format.selected().min(4) as usize]));
            quality.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            jpeg_hint.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            depth.set_sensitive(draft.depths.len() > 1);
            depth.set_selected(u32::from(draft.recipe.depth == SampleDepth::U16));
            background.set_selected(match draft.recipe.background { ExportBackground::Preserve => 0, ExportBackground::White => 1, ExportBackground::Black => 2 });
        }
    ));
    let validation = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    validation.add_css_class("error");
    validation.set_widget_name("export-validation");
    space.connect_subtitle_notify(glib::clone!(
        #[weak]
        background,
        #[weak]
        format,
        #[strong]
        selected_profile,
        move |_| {
            if let Ok(profile) = selected_profile() {
                let recipe = ExportRecipe {
                    format: FORMATS[format.selected().min(4) as usize],
                    background: [ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black][background.selected() as usize],
                    ..ExportRecipe::web_share()
                };
                let draft = recipe.draft(ExportDraftAction::Profile(profile));
                format.set_selected(FORMATS.iter().position(|f| *f == draft.recipe.format).unwrap() as u32);
                background.set_selected(match draft.recipe.background { ExportBackground::Preserve => 0, ExportBackground::White => 1, ExportBackground::Black => 2 });
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
    let apply_recipe: Rc<dyn Fn(&ExportRecipe)> = Rc::new({
        let restore_profile = profile.restore.clone();
        let dimensions = dimensions.each_ref().map(|r| r.downgrade());
        glib::clone!(
            #[weak]
            resolution,
            #[weak]
            ppi,
            #[weak]
            format,
            #[weak]
            size,
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
            #[weak]
            enlarge,
            #[strong]
            updating,
            move |recipe: &ExportRecipe| {
                updating.set(true);
                format.set_selected(match recipe.format {
                    ExportFormat::Png => 0,
                    ExportFormat::Tiff => 1,
                    ExportFormat::Jpeg => 2,
                    ExportFormat::PngHdr => 3,
                    ExportFormat::PngHdrMapped => 4,
                });
                restore_profile(recipe.profile.clone());
                depth.set_selected(u32::from(recipe.depth == SampleDepth::U16));
                background.set_selected(match recipe.background {
                    ExportBackground::Preserve => 0,
                    ExportBackground::White => 1,
                    ExportBackground::Black => 2,
                });
                quality.set_value(f64::from(recipe.jpeg_quality));
                match recipe.resolution {
                    ExportResolution::Master => resolution.set_selected(0),
                    ExportResolution::Ppi(value) => {
                        ppi.set_value(f64::from(value));
                        resolution.set_selected(1);
                    }
                    ExportResolution::Omit => resolution.set_selected(2),
                }
                intent.set_selected(match recipe.encoding.conversion.intent {
                    RenderingIntent::RelativeColorimetric => 0,
                    RenderingIntent::Perceptual => 1,
                    RenderingIntent::Saturation => 2,
                    RenderingIntent::AbsoluteColorimetric => 3,
                });
                bpc.set_active(false);
                dither.set_active(recipe.encoding.dither != OutputDither::None);
                match recipe.size {
                    ExportSize::Original => size.set_selected(0),
                    ExportSize::Fit {
                        bounds,
                        enlarge: allow,
                    } => {
                        for (row, value) in dimensions.iter().zip(bounds) {
                            if let Some(row) = row.upgrade() {
                                row.set_value(f64::from(value));
                            }
                        }
                        enlarge.set_active(allow);
                        size.set_selected(1);
                    }
                }
                updating.set(false);
            }
        )
    });
    preset.connect_selected_notify(glib::clone!(
        #[strong]
        library,
        #[strong]
        destination,
        #[strong]
        updating,
        #[strong]
        apply_recipe,
        move |preset| {
            if updating.get() {
                return;
            }
            let index = preset.selected() as usize;
            if let Ok(recipe) = library.borrow().recipe(index, document) {
                destination.set(index);
                apply_recipe(&recipe);
            }
        }
    ));
    for row in [
        &format,
        &depth,
        &background,
        &intent,
        &size,
        &resolution,
    ] {
        row.connect_selected_notify(glib::clone!(
            #[weak]
            preset,
            #[strong]
            updating,
            move |_| {
                if !updating.get() {
                    updating.set(true);
                    preset.set_selected(3);
                    updating.set(false);
                }
            }
        ));
    }
    space.connect_subtitle_notify(glib::clone!(#[weak] preset, #[strong] updating, move |_| {
        if !updating.get() { updating.set(true); preset.set_selected(3); updating.set(false); }
    }));
    for row in [&bpc, &dither, &enlarge] {
        row.connect_active_notify(glib::clone!(
            #[weak]
            preset,
            #[strong]
            updating,
            move |_| {
                if !updating.get() {
                    updating.set(true);
                    preset.set_selected(3);
                    updating.set(false);
                }
            }
        ));
    }
    for row in dimensions.iter().chain([&quality, &ppi]) {
        row.connect_value_notify(glib::clone!(
            #[weak]
            preset,
            #[strong]
            updating,
            move |_| {
                if !updating.get() {
                    updating.set(true);
                    preset.set_selected(3);
                    updating.set(false);
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
            #[strong]
            selected_profile,
            move |depth: &adw::ComboRow| {
                note.set_label(if depth.selected() == 0 && document.depth == SampleDepth::U16 {
                "This copy reduces 16-bit artwork to 8-bit. The master retains its precision."
            } else if depth.selected() == 0 && selected_profile().is_ok_and(|p| p.profile == layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::ProPhoto)) {
                "16-bit is recommended for ProPhoto RGB gradients and further editing."
            } else {
                "The matching color profile is embedded in the image."
            });
            }
        );
    update_note(&depth);
    depth.connect_selected_notify(update_note);
    space.connect_subtitle_notify(glib::clone!(
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
        #[strong]
        chosen_resolution,
        #[upgrade_or]
        Err("Export options closed".into()),
        move || {
            let recipe = ExportRecipe {
                size: output_size(),
                resolution: chosen_resolution(),
                format: match format.selected() {
                    1 => ExportFormat::Tiff,
                    2 => ExportFormat::Jpeg,
                    3 => ExportFormat::PngHdr,
                    4 => ExportFormat::PngHdrMapped,
                    _ => ExportFormat::Png,
                },
                profile: selected_profile()?,
                depth: if depth.selected() == 0 {
                    SampleDepth::U8
                } else {
                    SampleDepth::U16
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
            let recipe = if recipe.format.is_hdr() {
                if !document.depth.is_float() { return Err("HDR delivery requires an HDR document".into()); }
                recipe.draft(ExportDraftAction::Refresh).recipe
            } else { recipe };
            recipe.validate()?;
            recipe.output_resolution(master_resolution)?;
            Ok(recipe)
        }
    ));
    let validate: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        validation,
        #[strong]
        read_recipe,
        move || {
            let result = read_recipe();
            dialog.set_response_enabled("export", result.is_ok());
            validation.set_label(result.as_ref().err().map_or("", String::as_str));
            validation.set_visible(result.is_err());
        }
    ));
    for row in [
        &format,
        &depth,
        &background,
        &intent,
        &size,
        &resolution,
    ] {
        row.connect_selected_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    space.connect_subtitle_notify({ let validate = validate.clone(); move |_| validate() });
    for row in [&bpc, &dither, &enlarge] {
        row.connect_active_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    for row in dimensions.iter().chain([&quality, &ppi]) {
        row.connect_value_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    let comparison =
        super::preview::Comparison::for_output(w.snapshot_gpu()?, snapshot.project.clone(), w.view_color());
    let compression_note = gtk::Label::builder()
        .label(
            "SDR output uses the saved rendition. HDR preview shows that rendition; PQ range is checked on export. Mapping to PQ range deliberately clips colors outside BT.2020 or 0–10000 cd/m². JPEG compression artifacts are excluded.",
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
        &depth,
        &background,
        &intent,
    ] {
        row.connect_selected_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    space.connect_subtitle_notify({ let refresh = refresh_preview.clone(); move |_| refresh() });
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
    content.append(&preset_group);
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
    presets::install(
        &w.window,
        &preset_group,
        &preset,
        library.clone(),
        destination.clone(),
        updating.clone(),
        read_recipe.clone(),
    );
    // Loading the destination follows the same control and preview path as a click.
    preset.notify("selected");
    refresh_preview();
    let response = crate::alert::choose(dialog, &w.window).await;
    comparison.close();
    comparison.finish().await;
    if response != "export" {
        return Ok(None);
    }
    Ok(Some(Choice {
        recipe: read_recipe()?,
        destination: destination.get(),
        library: library.borrow().clone(),
    }))
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
    let Some(choice) = choose_recipe(w, &snapshot).await? else {
        return Ok(false);
    };
    let recipe = choice.recipe;
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
    let file = match super::chooser::save(&dialog, &w.window, super::chooser::Folder::Export).await {
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
    let gpu = w.snapshot_gpu()?;
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
    let remembered = recipe.clone();
    let result = gio::spawn_blocking({
        let job = job.clone();
        move || write_snapshot(gpu, snapshot, recipe, &path, &job)
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
        let mut next = choice.library.clone();
        next.remember(choice.destination.min(3), remembered)?;
        if next != choice.library {
            presets::save(choice.library, next).await.map_err(|e| {
                format!("The image was exported, but its choices could not be remembered: {e}")
            })?;
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
