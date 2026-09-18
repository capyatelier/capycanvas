//! GTK output choices and a cancellable, immutable document worker.
use super::*;
use layer_core::color::{
    ConversionOptions, SampleDepth, OutputDither, OutputEncoding, RenderingIntent,
};
use layer_render_wgpu::snapshot::{CaptureControl, SnapshotGpu};
use std::sync::{Arc, Mutex};
mod presets;
mod navigation;
const FORMATS: [ExportFormat; 3] = [ExportFormat::Png, ExportFormat::Tiff, ExportFormat::Jpeg];

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
                    ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => renderer.write_gainmap(file,recipe.format.gainmap().unwrap(),recipe.jpeg_quality,recipe.background.matte(),recipe.format.maps_hdr_range()),
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
    edit_appearance: bool,
    recipe: ExportRecipe,
    destination: usize,
    library: ExportPresets,
}

async fn choose_recipe(w: &Rc<Workspace>, snapshot: &DocumentExport, initial: Option<&ExportRecipe>) -> Result<Option<Choice>, String> {
    let document = snapshot.project.document.color;
    let master_resolution = snapshot.project.document.resolution;
    let library = Rc::new(std::cell::RefCell::new(presets::load(document).await?));
    let destination = Rc::new(std::cell::Cell::new(0usize));
    let extent = [
        snapshot.project.document.width,
        snapshot.project.document.height,
    ];
    let dialog = adw::Dialog::builder().title("Export image")
        .content_width(480).content_height(680).build();
    let nav = adw::NavigationView::new();
    nav.set_widget_name("export-navigation");
    let response = Rc::new(std::cell::Cell::new("cancel"));
    let export = gtk::Button::with_label("Choose file…");
    export.set_widget_name("export-confirm");
    export.add_css_class("suggested-action");
    export.connect_clicked(glib::clone!(#[weak] dialog, #[strong] response, move |_| {
        response.set("export"); dialog.close();
    }));
    let appearance = adw::ActionRow::builder().title("Proof")
        .subtitle("SDR appearance").activatable(true).build();
    appearance.set_widget_name("export-appearance");
    appearance.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    appearance.connect_activated(glib::clone!(#[weak] dialog, #[strong] response, move |_| {
        response.set("appearance"); dialog.close();
    }));
    let links = adw::PreferencesGroup::new();
    let size_link = navigation::link(&links, &nav, "Size", "size");
    let color_link = navigation::link(&links, &nav, "Color & transparency", "color");
    let preset_link = navigation::link(&links, &nav, "Preset", "presets");
    links.add(&appearance);
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
        let size_link = size_link.downgrade();
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
                    size_note.set_label(&format!("Output: {width} × {height} px\n{metadata}"));
                    if let Some(link) = size_link.upgrade() { link.set_subtitle(&format!("{width} × {height} px")); }
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
    let delivery_group = adw::PreferencesGroup::new();
    let gainmaps=layer_color::photo::gainmap_available();
    let range = combo(&delivery_group, "Output", "export-output", if gainmaps { &["SDR", "HDR native · PNG", "HDR JPEG", "HDR with transparency · AVIF"] } else { &["SDR", "HDR native · PNG"] });
    range.set_visible(document.depth.is_float());
    let format = combo(&delivery_group, "Format", "export-format", &["PNG", "TIFF", "JPEG"]);
    let output_hint=gtk::Label::builder().wrap(true).xalign(0.).build();
    output_hint.add_css_class("dim-label");
    let flatten=adw::SwitchRow::builder().title("Flatten transparency").visible(false).build();
    flatten.set_widget_name("export-flatten");delivery_group.add(&flatten);
    let rendition_view=crate::panel_controls::segmented("export-rendition-view",&[("hdr","HDR"),("sdr","SDR")]);
    rendition_view.set_visible(false);
    let color_group = adw::PreferencesGroup::new();
    let profile = ProfileChooser::new(w, "Delivery profile", "export-space", document.space, ProfilePurpose::Output);
    let space = profile.row.clone();
    color_group.add(&space);
    let selected_profile = profile.selected.clone();
    let depth = combo(
        &color_group,
        "Bit depth",
        "export-depth",
        &["8-bit", "16-bit"],
    );
    let background = combo(
        &color_group,
        "Background",
        "export-background",
        &["Keep transparency", "White background", "Black background"],
    );
    let backgrounds = Rc::new(std::cell::RefCell::new(vec![ExportBackground::Preserve, ExportBackground::White, ExportBackground::Black]));
    let read_background: Rc<dyn Fn() -> ExportBackground> = Rc::new(glib::clone!(
        #[weak] background, #[strong] backgrounds, #[upgrade_or] ExportBackground::Preserve,
        move || backgrounds.borrow().get(background.selected() as usize).copied().unwrap_or(ExportBackground::Preserve)
    ));
    let apply_background: Rc<dyn Fn(&layer_ui::ExportDraft)> = Rc::new(glib::clone!(
        #[weak] background, #[strong] backgrounds,
        move |draft: &layer_ui::ExportDraft| {
            if *backgrounds.borrow() != draft.backgrounds {
                *backgrounds.borrow_mut() = draft.backgrounds.clone();
                let names: Vec<_> = draft.backgrounds.iter().map(|b| match b {
                    ExportBackground::Preserve => "Keep transparency", ExportBackground::White => "White", ExportBackground::Black => "Black",
                }).collect();
                background.set_model(Some(&gtk::StringList::new(&names)));
            }
            background.set_selected(draft.backgrounds.iter().position(|b| *b == draft.recipe.background).unwrap_or(0) as u32);
        }
    ));
    let quality = adw::SpinRow::with_range(1., 100., 1.);
    quality.set_title("Quality");
    quality.set_widget_name("export-jpeg-quality");
    quality.set_value(90.);
    quality.set_snap_to_ticks(true);
    quality.set_update_policy(gtk::SpinButtonUpdatePolicy::IfValid);
    quality.set_visible(false);
    delivery_group.add(&quality);
    let advanced_group = adw::PreferencesGroup::new();
    let advanced = adw::ExpanderRow::builder().title("Color conversion").build();
    let clip_hdr = adw::SwitchRow::builder().title("Clip out-of-range colors").subtitle("May lose highlight or color detail in the exported copy.").visible(false).build();
    clip_hdr.set_widget_name("export-hdr-clip");
    let clipping_group = adw::PreferencesGroup::new();
    clipping_group.add(&clip_hdr);
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
    let bpc=adw::SwitchRow::builder().title("Black point compensation").build();
    bpc.set_widget_name("export-bpc");advanced.add_row(&bpc);
    intent.connect_selected_notify(glib::clone!(#[weak] bpc,move |intent|{bpc.set_sensitive(intent.selected()!=3);if intent.selected()==3{bpc.set_active(false);}}));
    let print_delivery=adw::ActionRow::builder().title("Use print profile").activatable(true).visible(snapshot.project.document.proof.is_some()).build();
    print_delivery.set_widget_name("export-print-profile");
    print_delivery.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    color_group.add(&print_delivery);
    if let Some(proof)=snapshot.project.document.proof.clone(){
        print_delivery.set_subtitle(&proof.name);
        let restore=profile.restore.clone();
        print_delivery.connect_activated(glib::clone!(#[weak] print_delivery, #[weak] intent, #[weak] bpc, #[weak(rename_to=validation_error)] profile.error, move |_|{
            print_delivery.set_sensitive(false);
            let proof=proof.clone();let restore=restore.clone();let intent=intent.clone();let bpc=bpc.clone();let row=print_delivery.clone();let error=validation_error.clone();
            glib::spawn_future_local(async move{
                let result=gio::spawn_blocking(move || {
                    let p=ExportProfile{channels:layer_color::profile_channels(&proof.profile)?,profile:proof.profile,name:proof.name};
                    ProfilePurpose::Output.validate(&p,document.space)?;
                    Ok::<_,String>((p,proof.conversion))
                }).await.map_err(|_|"Print profile validation failed".to_string()).and_then(|r|r);
                match result {Ok((p,conversion))=>{restore(p);intent.set_selected(match conversion.intent {RenderingIntent::RelativeColorimetric=>0,RenderingIntent::Perceptual=>1,RenderingIntent::Saturation=>2,RenderingIntent::AbsoluteColorimetric=>3});bpc.set_active(conversion.black_point_compensation);},Err(e)=>{error.set_label(&e);error.set_visible(true);}}
                row.set_sensitive(true);
            });
        }));
    }
    let dither = adw::SwitchRow::builder()
        .title("Reduce banding")
        .subtitle("Dither 8-bit gradients")
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
        quality,
        #[weak]
        jpeg_hint,
        #[strong] read_background, #[strong] apply_background, #[strong] selected_profile,
        move |format| {
            let recipe = ExportRecipe {
                depth: if depth.selected() == 0 { SampleDepth::U8 } else { SampleDepth::U16 },
                profile: selected_profile().unwrap_or_else(|_| ExportRecipe::web_share().profile),
                background: read_background(),
                ..ExportRecipe::web_share()
            };
            let draft = recipe.draft(ExportDraftAction::Format(FORMATS[format.selected().min(2) as usize]));
            quality.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            jpeg_hint.set_visible(draft.recipe.format == ExportFormat::Jpeg);
            depth.set_sensitive(draft.depths.len() > 1);
            depth.set_selected(u32::from(draft.recipe.depth == SampleDepth::U16));
            apply_background(&draft);
        }
    ));
    let sync_range: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[weak] range, #[weak] format, #[weak] output_hint, #[weak] flatten, #[weak] rendition_view, #[weak] space,
        #[weak] depth, #[weak] background, #[weak] quality, #[weak] jpeg_hint,
        #[weak] intent, #[weak] dither, #[weak] appearance, #[weak] color_link, #[weak] advanced_group, #[weak] print_delivery,
        move || {
            let choice=range.selected();
            let hdr = choice != 0;
            advanced_group.set_visible(!hdr); print_delivery.set_visible(!hdr && print_delivery.subtitle().is_some());
            for widget in [format.upcast_ref::<gtk::Widget>(), space.upcast_ref(), depth.upcast_ref(), background.upcast_ref(), intent.upcast_ref()] { widget.set_visible(!hdr); }
            output_hint.set_label(match choice {1=>"HDR pixels for HDR-aware software.",2=>"HDR on supported devices; SDR on older viewers.",3=>"Keeps HDR and transparency. Requires a compatible viewer.",_=>"Standard image for everyday use."});
            output_hint.set_visible(document.depth.is_float());
            rendition_view.set_visible(choice>=2);
            flatten.set_visible(choice==2);
            appearance.set_visible(document.depth.is_float() && choice!=1);
            color_link.set_visible(!hdr || choice==2);
            background.set_visible(!hdr || choice==2);
            depth.set_visible(!hdr && format.selected() != 2);
            dither.set_visible(!hdr && depth.selected() == 0);
            quality.set_visible(choice>=2 || (!hdr && format.selected() == 2));
            jpeg_hint.set_visible(!hdr && format.selected() == 2);
        }
    ));
    for row in [&range, &format] { let sync = sync_range.clone(); row.connect_selected_notify(move |_| sync()); }
    sync_range();
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
        #[strong] read_background, #[strong] apply_background,
        move |_| {
            if let Ok(profile) = selected_profile() {
                let recipe = ExportRecipe {
                    format: FORMATS[format.selected().min(2) as usize],
                    background: read_background(),
                    ..ExportRecipe::web_share()
                };
                let draft = recipe.draft(ExportDraftAction::Profile(profile));
                format.set_selected(FORMATS.iter().position(|f| *f == draft.recipe.format).unwrap() as u32);
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
            bpc,
            #[weak]
            enlarge,
            #[strong]
            updating,
            #[strong] apply_background,
            move |recipe: &ExportRecipe| {
                updating.set(true);
                range.set_selected(match recipe.format.gainmap(){Some(layer_color::photo::GainMapFormat::Jpeg)=>2,Some(layer_color::photo::GainMapFormat::Avif)=>3,None=>u32::from(recipe.format.is_hdr())});
                flatten.set_active(recipe.format.gainmap()==Some(layer_color::photo::GainMapFormat::Jpeg)&&recipe.background!=ExportBackground::Preserve);
                clip_hdr.set_active(recipe.format.maps_hdr_range());
                format.set_selected(match recipe.format {
                    ExportFormat::Png => 0,
                    ExportFormat::Tiff => 1,
                    ExportFormat::Jpeg => 2,
                    ExportFormat::PngHdr | ExportFormat::PngHdrMapped | ExportFormat::JpegHdr | ExportFormat::JpegHdrMapped | ExportFormat::AvifHdr | ExportFormat::AvifHdrMapped => 0,
                });
                restore_profile(recipe.profile.clone());
                depth.set_selected(u32::from(recipe.depth == SampleDepth::U16));
                if !recipe.format.is_hdr() || recipe.format.gainmap().is_some() { apply_background(&recipe.clone().draft(ExportDraftAction::Refresh)); }
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
                bpc.set_active(recipe.encoding.conversion.black_point_compensation);
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
    for row in [&dither, &bpc, &enlarge, &clip_hdr, &flatten] {
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
                ""
            });
                note.set_visible(!note.text().is_empty());
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
        range,
        #[weak]
        clip_hdr,
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
        #[weak]
        bpc,
        #[strong]
        selected_profile,
        #[strong]
        output_size,
        #[strong]
        chosen_resolution,
        #[strong] read_background,
        #[upgrade_or]
        Err("Export options closed".into()),
        move || {
            let recipe = ExportRecipe {
                size: output_size(),
                resolution: chosen_resolution(),
                format: match (range.selected(),clip_hdr.is_active()) {
                    (1,false)=>ExportFormat::PngHdr,(1,true)=>ExportFormat::PngHdrMapped,
                    (2,false)=>ExportFormat::JpegHdr,(2,true)=>ExportFormat::JpegHdrMapped,
                    (3,false)=>ExportFormat::AvifHdr,(3,true)=>ExportFormat::AvifHdrMapped,
                    _=>FORMATS[format.selected().min(2) as usize],
                },
                profile: if range.selected() != 0 { ExportProfile::builtin(layer_core::color::RgbSpace::Srgb) } else { selected_profile()? },
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
                        black_point_compensation: bpc.is_active(),
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
    let comparison =
        super::preview::Comparison::for_output(w.snapshot_gpu()?, snapshot.project.clone(), w.view_color());
    comparison.set_headroom(super::preview::display_headroom(w));
    rendition_view.connect_active_name_notify(glib::clone!(#[weak] comparison, move |group| comparison.show_fallback(group.active_name().as_deref()==Some("sdr"))));
    let recommend=Rc::new(std::cell::Cell::new(false));
    range.connect_selected_notify(glib::clone!(#[strong] recommend, move |_| recommend.set(false)));
    range.connect_selected_notify(glib::clone!(#[weak] rendition_view, #[weak] comparison, move |range| {
        if range.selected()<2 {rendition_view.set_active_name(Some("hdr"));comparison.show_fallback(false);}
    }));
    let validate: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[weak]
        export,
        #[weak]
        validation,
        #[strong]
        read_recipe,
        #[weak] comparison,
        move || {
            let result = read_recipe();
            export.set_sensitive(result.as_ref().is_ok_and(|recipe| !recipe.format.is_hdr() || comparison.ready.get()));
            validation.set_label(result.as_ref().err().map_or("", String::as_str));
            validation.set_visible(result.is_err());
        }
    ));
    *comparison.changed.borrow_mut() = Some(Box::new(glib::clone!(
        #[strong] validate, #[strong] recommend, #[weak] comparison, #[weak] clip_hdr, #[weak] range, #[weak] flatten, #[weak] color_link,
        move |_| {
            if let Some(transparent)=comparison.has_transparency.get(){
                if recommend.replace(false) && gainmaps {range.set_selected(if transparent{3}else{2});}
                flatten.set_visible(range.selected()==2&&(transparent||flatten.is_active()));
                color_link.set_visible(range.selected()==0||(range.selected()==2&&(transparent||flatten.is_active())));
            }
            clip_hdr.set_visible(range.selected() != 0 && (clip_hdr.is_active() || comparison.range_exceeded.get()));
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
    ] {
        row.connect_selected_notify({
            let validate = validate.clone();
            move |_| validate()
        });
    }
    space.connect_subtitle_notify({ let validate = validate.clone(); move |_| validate() });
    for row in [&dither, &bpc, &enlarge, &clip_hdr, &flatten] {
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
    let compression_note = gtk::Label::builder().wrap(true).xalign(0.).build();
    compression_note.set_widget_name("export-preview-compression");
    compression_note.add_css_class("dim-label");
    let preview_note = glib::clone!(#[weak] range, #[weak] format, #[weak] compression_note, #[weak] note, move || {
        let hdr = range.selected() != 0;
        compression_note.set_label("JPEG compression artifacts are not previewed.");
        compression_note.set_visible(!hdr && format.selected() == 2);
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
                    Err(error) => comparison.invalidate(&error),
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
    space.connect_subtitle_notify({ let refresh = refresh_preview.clone(); move |_| refresh() });
    for row in [&dither, &bpc, &enlarge, &clip_hdr, &flatten] {
        row.connect_active_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
    // Gain-map previews decode the actual selected quality.
    for row in dimensions.iter().chain([&quality]) {
        row.connect_value_notify({
            let refresh = refresh_preview.clone();
            move |_| refresh()
        });
    }
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
    content.append(&output_hint);
    content.append(&comparison.widget);
    content.append(&rendition_view);
    content.append(&compression_note);
    content.append(&links);
    content.append(&clipping_group);
    content.append(&validation);
    let header = navigation::page(&nav, "Export image", "main", &content);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.set_widget_name("export-cancel");
    cancel.connect_clicked(glib::clone!(#[weak] dialog, move |_| { dialog.close(); }));
    header.set_show_end_title_buttons(false);
    header.pack_start(&cancel);
    header.pack_end(&export);
    let size_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    size_body.append(&group);
    size_body.append(&size_note);
    navigation::page(&nav, "Image size", "size", &size_body);
    let color_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    color_body.append(&color_group);
    color_body.append(&jpeg_hint);
    color_body.append(&profile.error);
    color_body.append(&note);
    color_body.append(&advanced_group);
    navigation::page(&nav, "Color & transparency", "color", &color_body);
    let presets_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    presets_body.append(&preset_group);
    navigation::page(&nav, "Presets", "presets", &presets_body);
    dialog.set_child(Some(&nav));
    preset.connect_selected_notify(glib::clone!(#[weak] preset_link, move |preset| {
        if let Some(value) = preset.selected_item().and_downcast::<gtk::StringObject>() { preset_link.set_subtitle(&value.string()); }
    }));
    let summarize_color = glib::clone!(#[weak] color_link, #[weak] depth, #[weak] range, #[weak] flatten, #[strong] read_background, #[strong] selected_profile, move || {
        if range.selected()==2 {color_link.set_title("Background"); color_link.set_subtitle(if flatten.is_active(){match read_background(){ExportBackground::Black=>"Black",_=>"White"}}else{"No flattening"});return;}
        color_link.set_title("Color & transparency");
        if let Ok(profile) = selected_profile() {
            color_link.set_subtitle(&format!("{} · {}-bit · {}", profile.name, if depth.selected() == 0 { 8 } else { 16 },
                match read_background() { ExportBackground::Preserve => "Transparent", ExportBackground::White => "White", ExportBackground::Black => "Black" }));
        }
    });
    for row in [&depth, &background, &range] { let update = summarize_color.clone(); row.connect_selected_notify(move |_| update()); }
    space.connect_subtitle_notify(move |_| summarize_color());
    presets::install(
        &w.window,
        document.depth.is_float(),
        &presets_body,
        &preset,
        library.clone(),
        destination.clone(),
        updating.clone(),
        read_recipe.clone(),
    );
    // Loading the destination follows the same control and preview path as a click.
    preset.notify("selected");
    if let Some(recipe) = initial { apply_recipe(recipe); updating.set(true); preset.set_selected(3); updating.set(false); }
    else if document.depth.is_float() && preset.selected()==0 { range.set_selected(1); recommend.set(true); }
    refresh_preview();
    let response = navigation::choose(&dialog, &w.window, &response).await;
    comparison.close();
    comparison.finish().await;
    if response != "export" && response != "appearance" {
        return Ok(None);
    }
    Ok(Some(Choice {
        edit_appearance: response == "appearance",        recipe: read_recipe()?,
        destination: destination.get(),
        library: library.borrow().clone(),
    }))
}

pub(super) async fn run(w: &Rc<Workspace>, id: u32, name: &str) -> Result<bool, String> {
    w.proof_panel.finish_pending(w).await?;
    // The preview and final file share one immutable artwork revision and time.
    let mut snapshot = w
        .gpu
        .borrow()
        .as_ref()
        .ok_or("Canvas unavailable")?
        .session
        .capture_project_export(id)?;
    let mut initial = None;
    let choice = loop {
        let Some(choice) = choose_recipe(w, &snapshot, initial.as_ref()).await? else { return Ok(false); };
        if !choice.edit_appearance { break choice; }
        initial = Some(choice.recipe);
        crate::hdr::configure(w).await?;
        snapshot = w.gpu.borrow().as_ref().ok_or("Canvas unavailable")?.session.capture_project_export(id)?;
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
