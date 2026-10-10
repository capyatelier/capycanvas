//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_core::color::{
    ConversionOptions, SampleDepth, OutputDither, OutputEncoding, RenderingIntent,
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::sync::{Arc, Mutex};
mod presets;
mod navigation;

fn format_label(format: ExportFormat, localizer: &layer_ui::Localizer) -> Arc<str> {
    match format {
        ExportFormat::Png => "PNG".into(),
        ExportFormat::Tiff => "TIFF".into(),
        ExportFormat::Jpeg => "JPEG".into(),
        format => format.localized_name(localizer),
    }
}

fn numeric_inputs_ready(rows: &[crate::number_control::NumberControl], commit: bool) -> bool {
    rows.iter().filter(|row| row.is_visible()).all(|row| if commit { row.commit_text() } else { row.input_valid() })
}

fn fixed_depth(format: ExportFormat, localization: &layer_ui::Localizer) -> bool {
    ExportRecipe::web_share().draft_localized(ExportDraftAction::Format(format), localization).depths.len() == 1
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

fn relabel_choice(row: &adw::ComboRow, title: &str, values: &[&str]) {
    let selected = row.selected();
    let frozen = row.freeze_notify();
    row.set_title(title);
    if let Some(model) = row.model().and_downcast::<gtk::StringList>() {
        model.splice(0, model.n_items(), values);
    }
    row.set_selected(selected);
    drop(frozen);
}

struct Choice {
    recipe: ExportRecipe,
    destination: usize,
    library: ExportPresets,
}

async fn choose_recipe(w: &Rc<Workspace>, snapshot: &DocumentExport) -> Result<Option<Choice>, String> {
    let document = snapshot.composition().color;
    let master_resolution = snapshot.composition().resolution;
    let library = Rc::new(std::cell::RefCell::new(presets::load(document).await.map_err(|reason| reason.preset_message(&w.localization()))?));
    let localization = w.localization();
    let copy = layer_ui::color_feature_copy::ExportCopy::new(&localization);
    let updating = Rc::new(std::cell::Cell::new(false));
    let destination = Rc::new(std::cell::Cell::new(0usize));
    let extent = [
        snapshot.composition().size[0],
        snapshot.composition().size[1],
    ];
    let dialog = adw::Dialog::builder().title(copy.title.as_ref())
        .content_width(480).content_height(680).build();
    let nav = adw::NavigationView::new();
    nav.set_widget_name("export-navigation");
    let response = Rc::new(std::cell::Cell::new("cancel"));
    let export = gtk::Button::with_label(copy.choose_file.as_ref());
    export.set_widget_name("export-confirm");
    export.add_css_class("suggested-action");

    let links = adw::PreferencesGroup::new();
    let size_link = navigation::link(&links, &nav, copy.size.as_ref(), "size");
    let color_link = navigation::link(&links, &nav, copy.color_transparency.as_ref(), "color");
    let preset_link = navigation::link(&links, &nav, copy.preset.as_ref(), "presets");
    dialog.set_widget_name("export-options");
    let group = adw::PreferencesGroup::new();
    let preset_group = adw::PreferencesGroup::new();
    let preset = combo(
        &preset_group,
        copy.preset.as_ref(),
        "export-preset",
        &[
            copy.web_share.as_ref(),
            copy.wide_color.as_ref(),
            copy.further_editing.as_ref(),
            copy.custom.as_ref(),
        ],
    );
    let size = combo(
        &group,
        copy.size.as_ref(),
        "export-size",
        &[copy.original_size.as_ref(), copy.fit_bounds.as_ref()],
    );
    let numeric = layer_ui::ExportNumericControls::default();
    let dimensions: [crate::number_control::NumberControl; 2] = std::array::from_fn(|i| {
        let row = crate::number_control::NumberControl::panel(numeric.dimension.clone(), [copy.maximum_width.as_ref(), copy.maximum_height.as_ref()][i], localization.clone());
        row.set_widget_name(["export-width", "export-height"][i]);
        row.set_value(f64::from(extent[i]));
        row.set_visible(false);
        group.add(&row);
        row
    });
    let enlarge = adw::SwitchRow::builder()
        .title(copy.enlarge.as_ref())
        .visible(false)
        .build();
    enlarge.set_widget_name("export-enlarge");
    group.add(&enlarge);
    let resolution = combo(
        &group,
        copy.resolution.as_ref(),
        "export-resolution",
        &[copy.keep_resolution.as_ref(), copy.custom.as_ref(), copy.omit.as_ref()],
    );
    let ppi = crate::number_control::NumberControl::panel(numeric.ppi.clone(), copy.ppi.as_ref(), localization.clone());
    ppi.set_widget_name("export-ppi");
    ppi.set_value(300.);
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
        let workspace = Rc::downgrade(w);
        let size_note = size_note.downgrade();
        let size_link = size_link.downgrade();
        let output_size = output_size.clone();
        let chosen_resolution = chosen_resolution.clone();
        let ppi = ppi.downgrade();
        let dimensions = dimensions.each_ref().map(|row| row.downgrade());
        let enlarge = enlarge.downgrade();
        move || {
            let Some(w) = workspace.upgrade() else { return; };
            let localization = w.localization();
            let copy = layer_ui::color_feature_copy::ExportCopy::new(&localization);
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
                        || copy.no_resolution.as_ref().into(),
                        |r| {
                            let [x, y] = r.pixels_per_inch();
                            let width = format!("{:.2}", f64::from(width) / x * 2.54);
                            let height = format!("{:.2}", f64::from(height) / y * 2.54);
                            let x = format!("{x:.2}"); let y = format!("{y:.2}");
                            let mut args = layer_ui::FluentArgs::new();
                            args.set("width", width.as_str()); args.set("height", height.as_str()); args.set("x_ppi", x.as_str()); args.set("y_ppi", y.as_str());
                            localization.format(layer_ui::MessageId::COLOR_FEATURES_EXPORT_PHYSICAL_RESOLUTION, &args)
                        },
                    );
                    let mut args = layer_ui::FluentArgs::new();
                    let width_text = width.to_string(); let height_text = height.to_string();
                    args.set("width", width_text.as_str()); args.set("height", height_text.as_str()); args.set("resolution", metadata.as_str());
                    size_note.set_label(&localization.format(layer_ui::MessageId::COLOR_FEATURES_EXPORT_SIZE_SUMMARY, &args));
                    if let Some(link) = size_link.upgrade() { link.set_subtitle(&format!("{width} × {height} px")); }
                }
                Err(error) => size_note.set_label(&error.message(&localization)),
            }
        }
    });
    size.connect_selected_notify({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    for row in &dimensions {
        row.connect_value_changed({
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
    ppi.connect_value_changed({
        let refresh = refresh_size.clone();
        move |_| refresh()
    });
    refresh_size();
    let delivery_group = adw::PreferencesGroup::new();
    let exr_index = 4;
    let range = combo(&delivery_group, copy.output.as_ref(), "export-output", &["SDR", copy.hdr_native.as_ref(), copy.format_jpeg_hdr.as_ref(), copy.format_avif_hdr.as_ref(), copy.format_exr.as_ref()]);
    range.set_visible(document.depth.is_float());
    let formats = Rc::new(ExportRecipe::web_share().draft_localized(ExportDraftAction::Refresh, &localization).formats);
    let format_labels: Vec<_> = formats.iter().map(|f| format_label(*f, &localization)).collect();
    let format = combo(&delivery_group, copy.format.as_ref(), "export-format", &format_labels.iter().map(|label|label.as_ref()).collect::<Vec<_>>());
    let read_format: Rc<dyn Fn() -> ExportFormat> = Rc::new(glib::clone!(
        #[weak] format, #[strong] formats, #[upgrade_or] ExportFormat::Png,
        move || formats.get(format.selected() as usize).copied().unwrap_or(ExportFormat::Png)
    ));
    let select_format: Rc<dyn Fn(ExportFormat)> = Rc::new(glib::clone!(
        #[weak] format, #[strong] formats,
        move |value: ExportFormat| format.set_selected(formats.iter().position(|f| *f == value).unwrap_or(0) as u32)
    ));
    let flatten=adw::SwitchRow::builder().title(copy.flatten.as_ref()).visible(false).build();
    flatten.set_widget_name("export-flatten");delivery_group.add(&flatten);
    let rendition_view=crate::panel_controls::segmented("export-rendition-view",&[("hdr","HDR"),("sdr","SDR")]);
    rendition_view.set_visible(false);
    let color_group = adw::PreferencesGroup::new();
    let profile = ProfileChooser::new(w, copy.profile.as_ref(), "export-space", document.space, ProfilePurpose::Output);
    let space = profile.row.clone();
    color_group.add(&space);
    let selected_profile = profile.selected.clone();
    let selected_profile_typed = profile.selected_typed.clone();
    let depth = combo(
        &color_group,
        copy.depth.as_ref(),
        "export-depth",
        &[copy.depth_8.as_ref(), copy.depth_16.as_ref()],
    );
    let background = combo(
        &color_group,
        copy.background.as_ref(),
        "export-background",
        &[copy.keep_transparency.as_ref(), copy.white_background.as_ref(), copy.black_background.as_ref()],
    );
    let backgrounds = Rc::new(std::cell::RefCell::new(vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black]));
    let read_background: Rc<dyn Fn() -> ExportBackground> = Rc::new(glib::clone!(
        #[weak] background, #[strong] backgrounds, #[upgrade_or] ExportBackground::Preserve,
        move || backgrounds.borrow().get(background.selected() as usize).copied().unwrap_or(ExportBackground::Preserve)
    ));
    let apply_background: Rc<dyn Fn(&layer_ui::ExportDraft)> = Rc::new(glib::clone!( #[weak] w,
        #[weak] background, #[strong] backgrounds,
        move |draft: &layer_ui::ExportDraft| {
            let copy = layer_ui::color_feature_copy::ExportCopy::new(&w.localization());
            if *backgrounds.borrow() != draft.backgrounds {
                *backgrounds.borrow_mut() = draft.backgrounds.clone();
                let names: Vec<_> = draft.backgrounds.iter().map(|b| match b {
                    ExportBackground::Preserve => copy.keep_transparency.as_ref(), ExportBackground::White => copy.white.as_ref(), ExportBackground::Black => copy.black.as_ref(),
                }).collect();
                background.set_model(Some(&gtk::StringList::new(&names)));
            }
            background.set_selected(draft.backgrounds.iter().position(|b| *b == draft.recipe.background).unwrap_or(0) as u32);
        }
    ));
    let quality = crate::number_control::NumberControl::panel(numeric.quality, copy.quality.as_ref(), localization.clone());
    quality.set_widget_name("export-jpeg-quality");
    quality.set_value(90.);
    quality.set_visible(false);
    delivery_group.add(&quality);
    let photo_metadata = !snapshot.metadata().is_empty();
    let metadata_view = ExportRecipe::web_share().draft_localized(ExportDraftAction::Refresh, &localization).metadata;
    let metadata_choices: Rc<Vec<MetadataKeep>> = Rc::new(metadata_view.choices.iter().map(|c| c.value).collect());
    let metadata = combo(&delivery_group, &metadata_view.label, "export-metadata",
        &metadata_view.choices.iter().map(|c| c.label.as_ref()).collect::<Vec<_>>());
    let remove_location = adw::SwitchRow::builder().title(metadata_view.remove_location.as_ref()).active(true).build();
    remove_location.set_widget_name("export-remove-location");
    delivery_group.add(&remove_location);
    let metadata_note = gtk::Label::builder().wrap(true).xalign(0.).visible(false).build();
    metadata_note.set_widget_name("export-metadata-note");
    metadata_note.add_css_class("dim-label");
    let read_metadata: Rc<dyn Fn() -> ExportMetadata> = Rc::new(glib::clone!(
        #[weak] metadata, #[weak] remove_location, #[strong] metadata_choices, #[upgrade_or_default]
        move || ExportMetadata {
            keep: metadata_choices.get(metadata.selected() as usize).copied().unwrap_or_default(),
            remove_location: remove_location.is_active(),
        }
    ));
    let advanced_group = adw::PreferencesGroup::new();
    let advanced = adw::ExpanderRow::builder().title(copy.conversion.as_ref()).build();
    let clip_hdr = adw::SwitchRow::builder().title(copy.clip_hdr.as_ref()).subtitle(copy.clip_help.as_ref()).visible(false).build();
    clip_hdr.set_widget_name("export-hdr-clip");
    let clipping_group = adw::PreferencesGroup::new();
    clipping_group.set_widget_name("export-clipping-group");
    clipping_group.add(&clip_hdr);
    // An empty boxed group still paints its list shadow. Hide the container
    // whenever its only row is hidden, rather than leaving a dark bottom stripe.
    clip_hdr.bind_property("visible", &clipping_group, "visible").sync_create().build();
    advanced.set_widget_name("export-advanced");
    advanced_group.add(&advanced);
    let intent = choice(
        copy.intent.as_ref(),
        "export-intent",
        &[
            copy.relative.as_ref(),
            copy.perceptual.as_ref(),
            copy.saturation.as_ref(),
            copy.absolute.as_ref(),
        ],
    );
    advanced.add_row(&intent);
    let print_delivery=adw::ActionRow::builder().title(copy.use_print_profile.as_ref()).activatable(true).visible(snapshot.output().proof.is_some()).build();
    print_delivery.set_widget_name("export-print-profile");
    print_delivery.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    color_group.add(&print_delivery);
    if let Some(proof)=snapshot.output().proof.clone(){
        print_delivery.set_subtitle(&proof.name);
        let choose=profile.validated_selection();
        print_delivery.connect_activated(glib::clone!(#[weak] intent, move |_|{
            intent.set_selected(match proof.conversion.intent {RenderingIntent::RelativeColorimetric=>0,RenderingIntent::Perceptual=>1,RenderingIntent::Saturation=>2,RenderingIntent::AbsoluteColorimetric=>3});
            choose(proof.profile.clone(),proof.name.clone());
        }));
        profile.connect_changed(glib::clone!(#[weak] print_delivery,#[strong] selected_profile,move ||print_delivery.set_sensitive(selected_profile().is_ok())));
    }
    let dither = adw::SwitchRow::builder()
        .title(copy.reduce_banding.as_ref())
        .subtitle(copy.dither_gradients.as_ref())
        .build();
    dither.set_widget_name("export-dither");
    color_group.add(&dither);
    depth.connect_selected_notify(glib::clone!(
        #[weak]
        dither,
        move |depth| {
            dither.set_visible(depth.selected() == 0);
        }
    ));
    let jpeg_hint = gtk::Label::builder()
        .label(copy.jpeg_note.as_ref())
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    jpeg_hint.add_css_class("dim-label");
    format.connect_selected_notify(glib::clone!( #[weak] w, #[strong] updating,
        #[weak]
        depth,
        #[weak]
        quality,
        #[weak]
        jpeg_hint,
        #[strong] read_background, #[strong] apply_background, #[strong] selected_profile, #[strong] read_format,
        move |_| {
            if updating.get() { return; }
            let localization = w.localization();
            let recipe = ExportRecipe {
                depth: if depth.selected() == 0 { SampleDepth::U8 } else { SampleDepth::U16 },
                profile: selected_profile().unwrap_or_else(|_| ExportRecipe::web_share().profile),
                background: read_background(),
                ..ExportRecipe::web_share()
            };
            let draft = recipe.draft_localized(ExportDraftAction::Format(read_format()), &localization);
            quality.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            jpeg_hint.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            depth.set_sensitive(draft.depths.len() > 1);
            depth.set_selected(u32::from(draft.recipe.depth == SampleDepth::U16));
            apply_background(&draft);
        }
    ));
    let sync_range: Rc<dyn Fn()> = Rc::new(glib::clone!( #[weak] w,
        #[weak] range, #[weak] format, #[weak] flatten, #[weak] rendition_view, #[weak] space,
        #[weak] depth, #[weak] background, #[weak] quality, #[weak] jpeg_hint,
        #[weak] intent, #[weak] dither, #[weak] color_link, #[weak] advanced_group, #[weak] print_delivery,
        #[strong] read_format,
        move || {
            let localization = w.localization();
            let choice=range.selected();
            let hdr = choice != 0;
            let jpeg = read_format() == ExportFormat::Jpeg;
            advanced_group.set_visible(!hdr); print_delivery.set_visible(!hdr && print_delivery.subtitle().is_some());
            for widget in [format.upcast_ref::<gtk::Widget>(), space.upcast_ref(), depth.upcast_ref(), background.upcast_ref(), intent.upcast_ref()] { widget.set_visible(!hdr); }
            rendition_view.set_visible(choice>=2 && choice != exr_index);
            flatten.set_visible(choice==2 && choice != exr_index);
            color_link.set_visible(!hdr || (choice==2 && choice != exr_index));
            background.set_visible(!hdr || (choice==2 && choice != exr_index));
            depth.set_visible(!hdr && !fixed_depth(read_format(), &localization));
            dither.set_visible(!hdr && depth.selected() == 0);
            quality.set_visible((choice>=2 && choice != exr_index) || (!hdr && jpeg));
            jpeg_hint.set_visible(!hdr && jpeg);
        }
    ));
    for row in [&range, &format] { let sync = sync_range.clone(); row.connect_selected_notify(move |_| sync()); }
    sync_range();
    let read_output_format: Rc<dyn Fn() -> ExportFormat> = Rc::new(glib::clone!(
        #[weak] range, #[weak] clip_hdr, #[strong] read_format, #[upgrade_or] ExportFormat::Png,
        move || match (range.selected(), clip_hdr.is_active()) {
            (v, _) if v == exr_index => ExportFormat::Exr,
            (1, false) => ExportFormat::PngHdr, (1, true) => ExportFormat::PngHdrMapped,
            (2, false) => ExportFormat::JpegHdr, (2, true) => ExportFormat::JpegHdrMapped,
            (3, false) => ExportFormat::AvifHdr, (3, true) => ExportFormat::AvifHdrMapped,
            _ => read_format(),
        }
    ));
    let sync_metadata: Rc<dyn Fn()> = Rc::new(glib::clone!( #[weak] w,
        #[weak] metadata, #[weak] remove_location, #[weak] metadata_note, #[strong] read_output_format, #[strong] read_metadata,
        move || {
            let localization = w.localization();
            let recipe = ExportRecipe { format: read_output_format(), metadata: read_metadata(), ..ExportRecipe::web_share() };
            let view = recipe.draft_localized(ExportDraftAction::Refresh, &localization).metadata;
            metadata.set_visible(photo_metadata && view.available);
            remove_location.set_visible(photo_metadata && view.location);
            metadata_note.set_label(view.note.as_deref().unwrap_or_default());
            metadata_note.set_visible(photo_metadata && view.note.is_some());
        }
    ));
    for row in [&range, &format, &metadata] { let sync = sync_metadata.clone(); row.connect_selected_notify(move |_| sync()); }
    sync_metadata();
    let validation = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .visible(false)
        .build();
    validation.add_css_class("error");
    validation.set_widget_name("export-validation");
    profile.connect_changed(glib::clone!( #[weak] w, #[strong] updating,
        #[weak]
        background,
        #[strong]
        selected_profile,
        #[strong] read_background, #[strong] apply_background, #[strong] read_format, #[strong] select_format,
        move || {
            if updating.get() { return; }
            let localization = w.localization();
            if let Ok(profile) = selected_profile() {
                let recipe = ExportRecipe {
                    format: read_format(),
                    background: read_background(),
                    ..ExportRecipe::web_share()
                };
                let draft = recipe.draft_localized(ExportDraftAction::Profile(profile), &localization);
                select_format(draft.recipe.format);
                apply_background(&draft);
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
    let apply_recipe: Rc<dyn Fn(&ExportRecipe)> = Rc::new({
        let restore_profile = profile.restore.clone();
        let dimensions = dimensions.each_ref().map(|r| r.downgrade());
        glib::clone!( #[weak] w,
            #[weak]
            resolution,
            #[weak]
            ppi,
            #[strong]
            select_format,
            #[weak]
            range,
            #[weak]
            clip_hdr,
            #[weak]
            flatten,
            #[weak]
            size,
            #[weak]
            depth,
            #[weak]
            quality,
            #[weak]
            intent,
            #[weak]
            dither,
            #[weak]
            enlarge,
            #[strong]
            updating,
            #[strong] apply_background,
            #[weak] metadata,
            #[weak] remove_location,
            #[strong] metadata_choices,
            move |recipe: &ExportRecipe| {
                let localization = w.localization();
                updating.set(true);
                metadata.set_selected(metadata_choices.iter().position(|k| *k == recipe.metadata.keep).unwrap_or(0) as u32);
                remove_location.set_active(recipe.metadata.remove_location);
                range.set_selected(if recipe.format == ExportFormat::Exr { exr_index } else { match recipe.format.gainmap(){Some(layer_color::photo::GainMapFormat::Jpeg)=>2,Some(layer_color::photo::GainMapFormat::Avif)=>3,None=>u32::from(recipe.format.is_hdr())}});
                flatten.set_active(recipe.format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg)&&recipe.background!=ExportBackground::Preserve);
                clip_hdr.set_active(recipe.format.maps_hdr_range());
                select_format(if recipe.format.is_hdr() { ExportFormat::Png } else { recipe.format });
                restore_profile(recipe.profile.clone());
                depth.set_selected(u32::from(recipe.depth == SampleDepth::U16));
                if !recipe.format.is_hdr() || recipe.format.gainmap().is_some() { apply_background(&recipe.clone().draft_localized(ExportDraftAction::Refresh, &localization)); }
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
        &range,
        &depth,
        &background,
        &intent,
        &size,
        &resolution,
        &metadata,
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
    profile.connect_changed(glib::clone!(#[weak] preset, #[strong] updating, move || {
        if !updating.get() { updating.set(true); preset.set_selected(3); updating.set(false); }
    }));
    for row in [&dither, &enlarge, &clip_hdr, &flatten, &remove_location] {
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
        row.connect_value_changed(glib::clone!(
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
        glib::clone!( #[weak] w,
            #[weak]
            note,
            #[strong]
            selected_profile,
            move |depth: &adw::ComboRow| {
                let localization = w.localization();
                let copy = layer_ui::color_feature_copy::ExportCopy::new(&localization);
                let prophoto_note = localization.text(layer_ui::MessageId::DOCUMENTS_PROPHOTO_NOTE);
                note.set_label(if depth.selected() == 0 && document.depth == SampleDepth::U16 {
                copy.precision_note.as_ref()
            } else if depth.selected() == 0 && selected_profile().is_ok_and(|p| p.profile == layer_core::color::ColorProfile::Builtin(layer_core::color::RgbSpace::ProPhoto)) {
                prophoto_note.as_ref()
            } else {
                ""
            });
                note.set_visible(!note.text().is_empty());
            }
        );
    update_note(&depth);
    depth.connect_selected_notify(update_note.clone());
    profile.connect_changed(glib::clone!(
        #[weak]
        depth,
        move || {
            depth.notify("selected");
        }
    ));
    let read_recipe: Rc<dyn Fn() -> Result<ExportRecipe, layer_ui::ColorFeatureError>> = Rc::new(glib::clone!( #[weak] w,
        #[weak]
        range,
        #[weak]
        flatten,
        #[weak]
        depth,
        #[weak]
        quality,
        #[weak]
        intent,
        #[weak]
        dither,
        #[strong]
        selected_profile_typed,
        #[strong]
        output_size,
        #[strong]
        chosen_resolution,
        #[strong] read_background,
        #[strong] read_output_format,
        #[strong] read_metadata,
        #[upgrade_or]
        Err(layer_ui::ColorFeatureError::Diagnostic("Export options closed".into())),
        move || {
            let localization = w.localization();
            let recipe = ExportRecipe {
                size: output_size(),
                resolution: chosen_resolution(),
                format: read_output_format(),
                metadata: read_metadata(),
                profile: if range.selected() == exr_index { ExportProfile::builtin(document.space) } else if range.selected() != 0 { ExportProfile::builtin(layer_core::color::RgbSpace::Srgb) } else { selected_profile_typed()? },
                depth: if depth.selected() == 0 {
                    SampleDepth::U8
                } else {
                    SampleDepth::U16
                },
                background: if range.selected()==2 {if flatten.is_active(){match read_background(){ExportBackground::Preserve=>ExportBackground::White,b=>b}}else{ExportBackground::Preserve}} else {read_background()},
                jpeg_quality: quality.value() as u8,
                encoding: OutputEncoding {
                    conversion: ConversionOptions {
                        intent: match intent.selected() {
                            1 => RenderingIntent::Perceptual,
                            2 => RenderingIntent::Saturation,
                            3 => RenderingIntent::AbsoluteColorimetric,
                            _ => RenderingIntent::RelativeColorimetric,
                        },
                        black_point_compensation: false,
                    },
                    dither: if depth.selected() == 0 && dither.is_active() {
                        OutputDither::Stochastic8
                    } else {
                        OutputDither::None
                    },
                },
            };
            let recipe = if recipe.format.is_hdr() {
                if !document.depth.is_float() { return Err(layer_ui::ColorFeatureError::HdrDocument); }
                recipe.draft_localized(ExportDraftAction::Refresh, &localization).recipe
            } else { recipe };
            recipe.validate()?;
            recipe.output_extent(extent)?;
            recipe.output_resolution(master_resolution)?;
            Ok(recipe)
        }
    ));
    let comparison =
        super::preview::Comparison::for_output(w.snapshot_gpu()?, snapshot.capture.clone(), w.view_color(), &w.localization());
    comparison.set_headroom(w.picker_headroom());
    rendition_view.connect_active_name_notify(glib::clone!(#[weak] comparison, move |group| comparison.show_fallback(group.active_name().as_deref()==Some("sdr"))));
    let recommend=Rc::new(std::cell::Cell::new(false));
    range.connect_selected_notify(glib::clone!(#[strong] recommend, #[strong] updating, move |_| if !updating.get() { recommend.set(false); }));
    // A user may explicitly choose the already selected HDR-native entry while
    // coverage is still being analysed. That produces no selected notification.
    // Any interaction with this chooser takes precedence over late recommendations.
    let output_input=gtk::EventControllerLegacy::new();
    output_input.set_propagation_phase(gtk::PropagationPhase::Capture);
    output_input.connect_event(glib::clone!(#[strong] recommend, move |_,event| {
        if matches!(event.event_type(),gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin | gtk::gdk::EventType::KeyPress) {recommend.set(false);}
        glib::Propagation::Proceed
    }));
    range.add_controller(output_input);

    range.connect_selected_notify(glib::clone!(#[weak] rendition_view, #[weak] comparison, move |range| {
        if range.selected()<2 || range.selected()==exr_index {rendition_view.set_active_name(Some("hdr"));comparison.show_fallback(false);}
    }));
    let numeric_rows = dimensions.iter().chain([&quality, &ppi]).cloned().collect::<Vec<_>>();
    export.connect_clicked(glib::clone!(#[weak] dialog, #[strong] response, #[strong] numeric_rows, move |_| {
        if numeric_inputs_ready(&numeric_rows, true) { response.set("export"); dialog.close(); }
    }));
    let validate: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[weak] w,
        #[strong] numeric_rows,
        #[weak]
        export,
        #[weak]
        validation,
        #[strong]
        read_recipe,
        #[weak] comparison,
        move || {
            let result = read_recipe();
            export.set_sensitive(numeric_inputs_ready(&numeric_rows, false) && result.as_ref().is_ok_and(|recipe| !recipe.format.is_hdr() || comparison.ready.get()));
            validation.set_label(&result.as_ref().err().map(|error| error.message(&w.localization())).unwrap_or_default());
            validation.set_visible(result.is_err());
        }
    ));
    *comparison.changed.borrow_mut() = Some(Box::new(glib::clone!(
        #[strong] validate, #[strong] recommend, #[weak] comparison, #[weak] clip_hdr, #[weak] range, #[weak] flatten, #[weak] color_link,
        move |_| {
            if let Some(transparent)=comparison.has_transparency.get(){
                if recommend.replace(false) {range.set_selected(if transparent{3}else{2});}
                flatten.set_visible(range.selected()==2&&range.selected()!=exr_index&&(transparent||flatten.is_active()));
                color_link.set_visible(range.selected()==0||(range.selected()==2&&range.selected()!=exr_index&&(transparent||flatten.is_active())));
            }
            clip_hdr.set_visible(range.selected() != 0 && range.selected() != exr_index && (clip_hdr.is_active() || comparison.range_exceeded.get()));
            validate();
        }
    )));
    for row in [
        &format,
        &range,
        &depth,
        &background,
        &intent,
        &size,
        &resolution,
        &metadata,
    ] {
        row.connect_selected_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    profile.connect_changed({ let validate = validate.clone(); move || validate() });
    for row in [&dither, &enlarge, &clip_hdr, &flatten, &remove_location] {
        row.connect_active_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    for row in dimensions.iter().chain([&quality, &ppi]) {
        row.connect_value_changed({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    for row in &numeric_rows {
        let validate = validate.clone();
        row.connect_input_changed(move |_| validate());
    }
    let compression_note = gtk::Label::builder().wrap(true).xalign(0.).build();
    compression_note.set_widget_name("export-preview-compression");
    compression_note.add_css_class("dim-label");
    let preview_note = glib::clone!( #[weak] w,#[weak] range, #[strong] read_format, #[weak] compression_note, #[weak] note, move || {
        let copy = layer_ui::color_feature_copy::ExportCopy::new(&w.localization());
        let hdr = range.selected() != 0;
        compression_note.set_label(copy.compression_note.as_ref());
        compression_note.set_visible(!hdr && read_format() == ExportFormat::Jpeg);
        note.set_visible(!hdr && !note.text().is_empty());
    });
    for row in [&range, &format] { let update = preview_note.clone(); row.connect_selected_notify(move |_| update()); }
    preview_note();
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
                    Err(error) => comparison.invalidate_error(error),
                }
            }
        )
    });
    comparison.follow_display(w, refresh_preview.clone());
    for row in [
        &preset,
        &size,
        &range,
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
    profile.connect_changed({ let refresh = refresh_preview.clone(); move || refresh() });
    for row in [&dither, &enlarge, &clip_hdr, &flatten] {
        row.connect_active_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    for row in &dimensions {
        row.connect_value_changed({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    // Gain-map previews decode the actual selected quality.
    quality.connect_value_changed({
        let refresh = refresh_preview.clone();
        let read_recipe = read_recipe.clone();
        move |_| {
            if read_recipe().is_ok_and(|recipe| recipe.format.gainmap().is_some()) {
                refresh();
            }
        }
    });
    dialog.connect_closed(
        glib::clone!(
            #[weak]
            comparison,
            move |_| comparison.close()
        ),
    );
    // One draft survives Back navigation; only the main page delivers it.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&delivery_group);
    content.append(&metadata_note);
    content.append(&comparison.widget);
    content.append(&rendition_view);
    content.append(&compression_note);
    content.append(&links);
    content.append(&clipping_group);
    content.append(&validation);
    let header = navigation::page(&nav, copy.title.as_ref(), "main", &content);
    let cancel = gtk::Button::with_label(copy.common.cancel.as_ref());
    cancel.set_widget_name("export-cancel");
    cancel.connect_clicked(glib::clone!(#[weak] dialog, move |_| { dialog.close(); }));
    header.set_show_end_title_buttons(false);
    header.pack_start(&cancel);
    header.pack_end(&export);
    let size_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    size_body.append(&group);
    size_body.append(&size_note);
    navigation::page(&nav, copy.image_size.as_ref(), "size", &size_body);
    let color_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    color_body.append(&color_group);
    color_body.append(&jpeg_hint);
    color_body.append(&profile.error);
    color_body.append(&note);
    color_body.append(&advanced_group);
    navigation::page(&nav, copy.color_transparency.as_ref(), "color", &color_body);
    let presets_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    presets_body.append(&preset_group);
    navigation::page(&nav, copy.presets.as_ref(), "presets", &presets_body);
    dialog.set_child(Some(&nav));
    preset.connect_selected_notify(glib::clone!(#[weak] preset_link, move |preset| {
        if let Some(value) = preset.selected_item().and_downcast::<gtk::StringObject>() { preset_link.set_subtitle(&value.string()); }
    }));
    let summarize_color = glib::clone!( #[weak] w,#[weak] color_link, #[weak] depth, #[weak] range, #[weak] flatten, #[strong] read_background, #[strong] selected_profile, move || {
        let copy = layer_ui::color_feature_copy::ExportCopy::new(&w.localization());
        if range.selected()==2 && range.selected()!=exr_index {color_link.set_title(copy.background.as_ref()); color_link.set_subtitle(if flatten.is_active(){match read_background(){ExportBackground::Black=>copy.black.as_ref(),_=>copy.white.as_ref()}}else{copy.no_flatten.as_ref()});return;}
        color_link.set_title(copy.color_transparency.as_ref());
        if let Ok(profile) = selected_profile() {
            color_link.set_subtitle(&format!("{} · {}-bit · {}", profile.display_name(&w.localization()), if depth.selected() == 0 { 8 } else { 16 },
                match read_background() { ExportBackground::Preserve => copy.transparent.as_ref(), ExportBackground::White => copy.white.as_ref(), ExportBackground::Black => copy.black.as_ref() }));
        }
    });
    for row in [&depth, &background, &range] { let update = summarize_color.clone(); row.connect_selected_notify(move |_| update()); }
    profile.connect_changed({ let update = summarize_color.clone(); move || update() });
    presets::install(
        w,
        document.depth.is_float(),
        &presets_body,
        &preset,
        library.clone(),
        destination.clone(),
        updating.clone(),
        read_recipe.clone(),
    );
    let dimension_labels = dimensions.each_ref().map(|row| row.downgrade());
    w.on_localization(glib::clone!(
        #[weak] dialog, #[weak] nav, #[weak] export, #[weak] cancel,
        #[weak] size, #[weak] resolution, #[weak] range, #[weak] format,
        #[weak] space, #[weak] depth, #[weak] background, #[weak] metadata,
        #[weak] intent, #[weak] enlarge, #[weak] flatten, #[weak] clip_hdr,
        #[weak] remove_location, #[weak] advanced, #[weak] print_delivery,
        #[weak] dither, #[weak] jpeg_hint, #[weak] quality, #[weak] ppi,
        #[weak] size_link, #[weak] preset_link,
        #[strong] formats, #[strong] backgrounds, #[strong] updating,
        #[strong] read_output_format, #[strong] read_metadata, #[strong] refresh_size,
        #[strong] sync_metadata, #[strong] preview_note, #[strong] summarize_color,
        #[strong] update_note, #[strong] validate,
        #[upgrade_or] false,
        move |localization| {
            let copy = layer_ui::color_feature_copy::ExportCopy::new(localization);
            updating.set(true);
            dialog.set_title(&copy.title);
            export.set_label(&copy.choose_file);
            cancel.set_label(&copy.common.cancel);
            for (tag, title) in [("main", &copy.title), ("size", &copy.image_size), ("color", &copy.color_transparency), ("presets", &copy.presets)] {
                if let Some(page) = nav.find_page(tag) { page.set_title(title); }
            }
            size_link.set_title(&copy.size);
            preset_link.set_title(&copy.preset);
            relabel_choice(&size, &copy.size, &[&copy.original_size, &copy.fit_bounds]);
            relabel_choice(&resolution, &copy.resolution, &[&copy.keep_resolution, &copy.custom, &copy.omit]);
            relabel_choice(&range, &copy.output, &["SDR", &copy.hdr_native, &copy.format_jpeg_hdr, &copy.format_avif_hdr, &copy.format_exr]);
            let labels = formats.iter().map(|format| format_label(*format, localization)).collect::<Vec<_>>();
            relabel_choice(&format, &copy.format, &labels.iter().map(AsRef::as_ref).collect::<Vec<_>>());
            relabel_choice(&depth, &copy.depth, &[&copy.depth_8, &copy.depth_16]);
            let values = backgrounds.borrow();
            let labels = values.iter().map(|value| match value { ExportBackground::Preserve => copy.keep_transparency.as_ref(), ExportBackground::White => copy.white.as_ref(), ExportBackground::Black => copy.black.as_ref() }).collect::<Vec<_>>();
            relabel_choice(&background, &copy.background, &labels);
            drop(values);
            relabel_choice(&intent, &copy.intent, &[&copy.relative, &copy.perceptual, &copy.saturation, &copy.absolute]);
            let view = ExportRecipe { format: read_output_format(), metadata: read_metadata(), ..ExportRecipe::web_share() }.draft_localized(ExportDraftAction::Refresh, localization).metadata;
            relabel_choice(&metadata, &view.label, &view.choices.iter().map(|choice| choice.label.as_ref()).collect::<Vec<_>>());
            remove_location.set_title(&view.remove_location);
            space.set_title(&copy.profile);
            enlarge.set_title(&copy.enlarge);
            flatten.set_title(&copy.flatten);
            clip_hdr.set_title(&copy.clip_hdr);
            clip_hdr.set_subtitle(&copy.clip_help);
            advanced.set_title(&copy.conversion);
            print_delivery.set_title(&copy.use_print_profile);
            dither.set_title(&copy.reduce_banding);
            dither.set_subtitle(&copy.dither_gradients);
            jpeg_hint.set_label(&copy.jpeg_note);
            for (row, title) in dimension_labels.iter().zip([&copy.maximum_width, &copy.maximum_height]) {
                if let Some(row) = row.upgrade() { row.set_caption(title, "", localization.clone()); }
            }
            quality.set_caption(&copy.quality, "", localization.clone());
            ppi.set_caption(&copy.ppi, "", localization.clone());
            updating.set(false);
            refresh_size(); sync_metadata(); preview_note(); summarize_color(); update_note(&depth); validate();
            true
        }
    ));
    // Loading the destination follows the same control and preview path as a click.
    preset.notify("selected");
    if document.depth.is_float() && preset.selected()==0 { range.set_selected(1); recommend.set(true); }
    refresh_preview();
    let response = navigation::choose(&dialog, &w.window, &response).await;
    comparison.close();
    comparison.finish().await;
    if response != "export" {
        return Ok(None);
    }
    Ok(Some(Choice {
        recipe: read_recipe().map_err(|error| error.message(&w.localization()))?,
        destination: destination.get(),
        library: library.borrow().clone(),
    }))
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
        let Some(choice) = choose_recipe(w, &snapshot).await? else { return Ok(false); };
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
    dialog.present(Some(&w.window));
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
                assert!(numeric_inputs_ready(&[row.clone()], true));
                let accepted = row.value();
                stack.set_visible_child_name("entry");
                input.set_text("１２３");
                assert!(!numeric_inputs_ready(&[row.clone()], true));
                assert_eq!(row.value(), accepted);
                stack.set_visible_child_name("entry");
                input.set_text(invalid);
                assert!(!numeric_inputs_ready(&[row.clone()], true));
                assert_eq!(row.value(), accepted);
                row.set_visible(false);
                assert!(numeric_inputs_ready(&[row.clone()], false));
                row.set_visible(true);
                stack.set_visible_child_name("entry");
                input.set_text(valid);
                input.delegate().and_downcast::<gtk::Text>().unwrap().emit_preedit_changed("にほんご");
                assert!(!numeric_inputs_ready(&[row.clone()], true));
                assert_eq!(row.value(), accepted);
                window.close();
            }
        }
    }
}
