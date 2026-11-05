use super::*;
use std::{cell::Cell, rc::Weak};

pub(super) struct Projection {
    pub(super) w: Weak<Workspace>,
    pub(super) id: u32,
    dialog: adw::Dialog,
    nav: adw::NavigationView,
    updating: Cell<bool>,
    profile_revision: Cell<u64>,
    preview_revision: Cell<u64>,
    fallback: Cell<Option<bool>>,
    response: Cell<&'static str>,
    comparison: Rc<crate::files::preview::Comparison>,
    profile: ProfileChooser,
    export: gtk::Button,
    cancel: gtk::Button,
    preset: adw::ComboRow,
    size: adw::ComboRow,
    dimensions: [crate::number_control::NumberControl;2],
    enlarge: adw::SwitchRow,
    resolution: adw::ComboRow,
    ppi: crate::number_control::NumberControl,
    size_note: gtk::Label,
    range: adw::ComboRow,
    format: adw::ComboRow,
    flatten: adw::SwitchRow,
    rendition: adw::ToggleGroup,
    depth: adw::ComboRow,
    background: adw::ComboRow,
    quality: crate::number_control::NumberControl,
    metadata: adw::ComboRow,
    remove_location: adw::SwitchRow,
    metadata_note: gtk::Label,
    advanced_group: adw::PreferencesGroup,
    advanced: adw::ExpanderRow,
    clip: adw::SwitchRow,
    intent: adw::ComboRow,
    print: adw::ActionRow,
    dither: adw::SwitchRow,
    jpeg_hint: gtk::Label,
    note: gtk::Label,
    compression: gtk::Label,
    validation: gtk::Label,
    size_link: adw::ActionRow,
    color_link: adw::ActionRow,
    preset_link: adw::ActionRow,
    presets: presets::Presentation,
}

fn labels(row:&adw::ComboRow,title:&str,values:&[&str],selected:u32) {
    row.set_title(title);
    let model=row.model().and_downcast::<gtk::StringList>();
    if model.as_ref().is_none_or(|model|model.n_items() as usize!=values.len()||values.iter().enumerate().any(|(i,value)|model.string(i as u32).as_deref()!=Some(value))) {
        row.set_model(Some(&gtk::StringList::new(values)));
    }
    row.set_selected(selected);
}
fn choices<T:PartialEq>(row:&adw::ComboRow,title:&str,values:&[ExportChoice<T>],selected:&T) {
    labels(row,title,&values.iter().map(|choice|choice.label.as_ref()).collect::<Vec<_>>(),values.iter().position(|choice|&choice.value==selected).unwrap_or(0) as u32);
}
fn number(row:&crate::number_control::NumberControl,value:u32) {
    if row.value()!=f64::from(value) {row.set_value(f64::from(value));}
}

impl Projection {
    pub(super) fn view(&self)->Option<ExportDialogView> {
        let w=self.w.upgrade()?;
        let gpu=w.gpu.borrow();gpu.as_ref()?.session.export_dialog_view(self.id).ok()
    }
    pub(super) fn send(self:&Rc<Self>,action:ExportDialogAction) {
        if self.updating.get(){return;}
        let Some(w)=self.w.upgrade() else{return;};
        let result={let mut gpu=w.gpu.borrow_mut();let Some(gpu)=gpu.as_mut() else{return;};gpu.session.export_dialog_action(self.id,action)};
        match result {Ok(view)=>self.render(view),Err(error)=>{self.validation.set_label(&error);self.validation.set_visible(true);self.export.set_sensitive(false);}}
    }
    fn refresh(self:&Rc<Self>) {if let Some(view)=self.view(){self.render(view);}}
    fn numeric(&self)->Vec<crate::number_control::NumberControl> {self.dimensions.iter().chain([&self.quality,&self.ppi]).cloned().collect()}
    fn request_preview(&self) {
        let Some(w)=self.w.upgrade() else{return;};
        let request={let gpu=w.gpu.borrow();let Some(gpu)=gpu.as_ref() else{return;};
            gpu.session.export_dialog_source(self.id).ok().map(|source|(source,gpu.session.export_dialog_recipe(self.id)))};
        if let Some((source,result))=request {match result {Ok(recipe)=>self.comparison.request_output(&source,recipe),Err(error)=>self.comparison.invalidate_error(error)}}
    }
    fn render(self:&Rc<Self>,view:ExportDialogView) {
        let Some(w)=self.w.upgrade() else{return;};
        let localization=w.localization();let copy=&view.copy;self.updating.set(true);
        self.dialog.set_title(&copy.title);self.export.set_label(&copy.choose_file);self.cancel.set_label(&copy.common.cancel);
        for (tag,title) in [("main",&copy.title),("size",&copy.image_size),("color",&copy.color_transparency),("presets",&copy.presets)] {
            if let Some(page)=self.nav.find_page(tag){page.set_title(title);}
        }
        labels(&self.size,&copy.size,&[&copy.original_size,&copy.fit_bounds],u32::from(view.show_size));
        choices(&self.resolution,&copy.resolution,&view.resolutions,&view.fields.resolution);
        choices(&self.range,&copy.output,&view.outputs,&view.output);self.range.set_visible(view.show_output);
        choices(&self.format,&copy.format,&view.formats,&view.fields.format);self.format.set_visible(view.show_conversion);
        choices(&self.depth,&copy.depth,&view.depths,&view.fields.depth);self.depth.set_visible(view.show_depth);self.depth.set_sensitive(view.depth_enabled);
        choices(&self.background,&copy.background,&view.backgrounds,&view.fields.background);self.background.set_visible(view.show_background);
        choices(&self.intent,&copy.intent,&view.intents,&view.fields.encoding.conversion.intent);
        labels(&self.metadata,&view.metadata.label,&view.metadata.choices.iter().map(|choice|choice.label.as_ref()).collect::<Vec<_>>(),view.metadata.choices.iter().position(|choice|choice.value==view.fields.metadata.keep).unwrap_or(0) as u32);
        self.metadata.set_visible(view.show_metadata);self.remove_location.set_title(&view.metadata.remove_location);
        self.remove_location.set_active(view.fields.metadata.remove_location);self.remove_location.set_visible(view.show_location);
        self.metadata_note.set_label(view.metadata.note.as_deref().unwrap_or_default());self.metadata_note.set_visible(view.show_metadata_note);
        self.profile.row.set_title(&copy.profile);self.profile.row.set_visible(view.show_conversion);
        if self.profile_revision.replace(view.profile_revision)!=view.profile_revision {
            let profile={let gpu=w.gpu.borrow();gpu.as_ref().and_then(|gpu|gpu.session.export_dialog_profile(self.id,view.fields.profile).ok())};
            if let Some(profile)=profile {(self.profile.restore)(profile);}
        }
        for (row,value,title) in [(&self.dimensions[0],view.bounds[0],&copy.maximum_width),(&self.dimensions[1],view.bounds[1],&copy.maximum_height)] {
            number(row,value);row.set_visible(view.show_size);row.set_caption(title,"",localization.clone());
        }
        self.enlarge.set_title(&copy.enlarge);self.enlarge.set_active(view.enlarge);self.enlarge.set_visible(view.show_size);
        number(&self.ppi,view.ppi);self.ppi.set_visible(view.show_ppi);self.ppi.set_caption(&copy.ppi,"",localization.clone());
        number(&self.quality,u32::from(view.fields.jpeg_quality));self.quality.set_visible(view.show_quality);self.quality.set_caption(&copy.quality,"",localization.clone());
        self.flatten.set_title(&copy.flatten);self.flatten.set_active(view.flatten);self.flatten.set_visible(view.show_flatten);
        self.clip.set_title(&copy.clip_hdr);self.clip.set_subtitle(&copy.clip_help);self.clip.set_active(view.clip);self.clip.set_visible(view.show_clip);
        self.dither.set_title(&copy.reduce_banding);self.dither.set_subtitle(&copy.dither_gradients);self.dither.set_active(view.dither);self.dither.set_visible(view.show_dither);
        self.advanced.set_title(&copy.conversion);self.advanced_group.set_visible(view.show_conversion);
        self.print.set_title(&copy.use_print_profile);self.print.set_subtitle(view.print_profile_name.as_deref().unwrap_or_default());self.print.set_visible(view.show_print_profile);self.print.set_sensitive(view.profile_ready);
        self.jpeg_hint.set_label(&copy.jpeg_note);self.jpeg_hint.set_visible(view.jpeg_note);
        self.note.set_label(view.precision_note.as_deref().unwrap_or_default());self.note.set_visible(view.precision_note.is_some());
        self.compression.set_label(&copy.compression_note);self.compression.set_visible(view.compression_note);
        self.size_note.set_label(&view.size_summary);self.size_link.set_title(&copy.size);self.size_link.set_subtitle(&view.size_subtitle);
        self.color_link.set_title(&view.color_title);self.color_link.set_subtitle(&view.color_subtitle);self.color_link.set_visible(view.show_color);
        self.preset_link.set_title(&copy.preset);self.preset_link.set_subtitle(view.preset_names.get(view.preset).map(String::as_str).unwrap_or_default());
        labels(&self.preset,&copy.preset,&view.preset_names.iter().map(String::as_str).collect::<Vec<_>>(),view.preset as u32);
        self.presets.render(&view);self.rendition.set_visible(view.show_rendition);self.rendition.set_active_name(Some(if view.preview_sdr{"sdr"}else{"hdr"}));
        self.validation.set_label(view.error.as_deref().unwrap_or_default());self.validation.set_visible(view.error.is_some());
        self.export.set_sensitive(view.can_export&&numeric_inputs_ready(&self.numeric(),false));
        navigation::project(&self.nav,view.page);
        if self.fallback.replace(Some(view.preview_sdr))!=Some(view.preview_sdr){self.comparison.show_fallback(view.preview_sdr);}
        self.updating.set(false);
        if self.preview_revision.replace(view.preview_revision)!=view.preview_revision {self.request_preview();}
    }
    fn connect_choice<T:Clone+'static>(self:&Rc<Self>,row:&adw::ComboRow,values:fn(&ExportDialogView)->Vec<T>,action:fn(T)->ExportDialogAction) {
        row.connect_selected_notify({let weak=Rc::downgrade(self);move|row|{let Some(p)=weak.upgrade().filter(|p|!p.updating.get())else{return;};
            if let Some(value)=p.view().and_then(|view|values(&view).get(row.selected() as usize).cloned()){p.send(action(value));}
        }});
    }
    fn bind(self:&Rc<Self>) {
        self.connect_choice(&self.range,|view|view.outputs.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::Output {value});
        self.connect_choice(&self.format,|view|view.formats.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::Edit {action:ExportDraftAction::Format(value)});
        self.connect_choice(&self.depth,|view|view.depths.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::Edit {action:ExportDraftAction::Depth(value)});
        self.connect_choice(&self.background,|view|view.backgrounds.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::Edit {action:ExportDraftAction::Background(value)});
        self.connect_choice(&self.intent,|view|view.intents.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::Intent {value});
        self.connect_choice(&self.resolution,|view|view.resolutions.iter().map(|choice|choice.value.clone()).collect(),|value|ExportDialogAction::Edit {action:ExportDraftAction::Resolution(value)});
        self.connect_choice(&self.metadata,|view|view.metadata.choices.iter().map(|choice|choice.value).collect(),|value|ExportDialogAction::MetadataKeep {value});
        self.size.connect_selected_notify({let weak=Rc::downgrade(self);move|row|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Fit {enabled:row.selected()!=0});}});
        self.preset.connect_selected_notify({let weak=Rc::downgrade(self);move|row|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Preset {index:row.selected() as usize});}});
        for (row,action) in [(&self.enlarge,(|enabled|ExportDialogAction::Enlarge {enabled}) as fn(bool)->ExportDialogAction),
            (&self.flatten,|enabled|ExportDialogAction::Flatten {enabled}),(&self.clip,|enabled|ExportDialogAction::Edit {action:ExportDraftAction::ClipHdrRange(enabled)}),
            (&self.dither,|enabled|ExportDialogAction::Dither {enabled}),(&self.remove_location,|enabled|ExportDialogAction::RemoveLocation {enabled})] {
            row.connect_active_notify({let weak=Rc::downgrade(self);move|row|if let Some(p)=weak.upgrade(){p.send(action(row.is_active()));}});
        }
        for (axis,row) in self.dimensions.iter().enumerate(){let weak=Rc::downgrade(self);row.connect_value_changed(move|row|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Bound {axis,value:row.value() as u32});});}
        self.ppi.connect_value_changed({let weak=Rc::downgrade(self);move|row|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Ppi {value:row.value() as u32});}});
        self.quality.connect_value_changed({let weak=Rc::downgrade(self);move|row|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Edit {action:ExportDraftAction::Quality(row.value() as u8)});}});
        for row in self.numeric(){let weak=Rc::downgrade(self);row.connect_input_changed(move|_|if let Some(p)=weak.upgrade().filter(|p|!p.updating.get()){p.refresh();});}
        self.profile.connect_changed({let weak=Rc::downgrade(self);move||if let Some(p)=weak.upgrade().filter(|p|!p.updating.get()){p.send(ExportDialogAction::Profile {result:(p.profile.selected_typed)()});}});
        self.rendition.connect_active_name_notify({let weak=Rc::downgrade(self);move|group|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Rendition {sdr:group.active_name().as_deref()==Some("sdr")});}});
        for (row,page) in [(&self.size_link,ExportDialogPage::Size),(&self.color_link,ExportDialogPage::Color),(&self.preset_link,ExportDialogPage::Presets)] {
            row.connect_activated({let weak=Rc::downgrade(self);move|_|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Navigate {page});}});
        }
        self.nav.connect_visible_page_tag_notify({let weak=Rc::downgrade(self);move|nav|if let Some(p)=weak.upgrade().filter(|p|!p.updating.get()){
            if let Some(page)=nav.visible_page_tag().as_deref().and_then(ExportDialogPage::from_tag){p.send(ExportDialogAction::Navigate {page});}
        }});
        let input=gtk::EventControllerLegacy::new();input.set_propagation_phase(gtk::PropagationPhase::Capture);
        input.connect_event({let weak=Rc::downgrade(self);move|_,event|{if matches!(event.event_type(),gtk::gdk::EventType::ButtonPress|gtk::gdk::EventType::TouchBegin|gtk::gdk::EventType::KeyPress){if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::OutputContact);}}glib::Propagation::Proceed}});self.range.add_controller(input);
        self.export.connect_clicked({let weak=Rc::downgrade(self);move|_|if let Some(p)=weak.upgrade(){if numeric_inputs_ready(&p.numeric(),true)&&p.view().is_some_and(|view|view.can_export){p.response.set("export");crate::page_dialog::close_dialog(&p.dialog);}}});
        self.cancel.connect_clicked({let dialog=self.dialog.downgrade();move|_|if let Some(dialog)=dialog.upgrade(){crate::page_dialog::close_dialog(&dialog);}});
        self.print.connect_activated({let weak=Rc::downgrade(self);move|_|{let Some(p)=weak.upgrade()else{return;};let Some(w)=p.w.upgrade()else{return;};
            let profile={let gpu=w.gpu.borrow();gpu.as_ref().and_then(|gpu|gpu.session.export_dialog_print_profile(p.id).ok()).flatten()};
            if let Some((profile,name,value))=profile{p.send(ExportDialogAction::Intent {value});(p.profile.validated_selection())(profile,name);}
        }});
        *self.comparison.changed.borrow_mut()=Some(Box::new({let weak=Rc::downgrade(self);move|_|if let Some(p)=weak.upgrade(){p.send(ExportDialogAction::Preview {value:ExportDialogPreview {ready:p.comparison.ready.get(),transparent:p.comparison.has_transparency.get(),range_exceeded:p.comparison.range_exceeded.get()}});}}));
        self.dialog.connect_closed({let comparison=self.comparison.clone();move|_|comparison.close()});
        if let Some(w)=self.w.upgrade(){
            self.comparison.follow_display(&w,Rc::new({let weak=Rc::downgrade(self);move||if let Some(p)=weak.upgrade(){p.request_preview();}}));
            w.on_localization({let weak=Rc::downgrade(self);move|_|{let Some(p)=weak.upgrade()else{return false;};p.refresh();true}});
        }
        self.presets.bind(self);
    }
}

pub(super) async fn choose(w:&Rc<Workspace>,id:u32,snapshot:&DocumentExport)->Result<Option<Choice>,String> {
    let library=presets::load(snapshot.composition().color).await.map_err(|error|error.preset_message(&w.localization()))?;
    let capabilities=ExportDialogCapabilities {formats:vec![ExportFormat::Png,ExportFormat::Tiff,ExportFormat::Jpeg,ExportFormat::Webp,ExportFormat::PngHdr,ExportFormat::JpegHdr,ExportFormat::AvifHdr,ExportFormat::Exr],profile_import:true};
    let view={let mut gpu=w.gpu.borrow_mut();gpu.as_mut().ok_or("Canvas unavailable")?.session.begin_export_dialog(id,ExportDialogContext::new(snapshot.clone(),capabilities,Vec::new()),library)?};
    let presentation=view.dialog;let copy=&view.copy;let localization=w.localization();let numeric=&view.numeric;
    let dialog=adw::Dialog::builder().title(copy.title.as_ref()).content_width(480).content_height(680).build();dialog.set_widget_name("export-options");
    let nav=adw::NavigationView::new();nav.set_widget_name("export-navigation");
    let export=gtk::Button::with_label(&copy.choose_file);export.set_widget_name("export-confirm");export.add_css_class("suggested-action");
    let links=adw::PreferencesGroup::new();
    let size_link=navigation::link(&links,&copy.size,"size");let color_link=navigation::link(&links,&copy.color_transparency,"color");let preset_link=navigation::link(&links,&copy.preset,"presets");
    let group=adw::PreferencesGroup::new();let preset_group=adw::PreferencesGroup::new();
    let preset=combo(&preset_group,&copy.preset,"export-preset",&[]);
    let size=combo(&group,&copy.size,"export-size",&[&copy.original_size,&copy.fit_bounds]);
    let dimensions=std::array::from_fn(|i|{let row=crate::number_control::NumberControl::panel(numeric.dimension.clone(),[copy.maximum_width.as_ref(),copy.maximum_height.as_ref()][i],localization.clone());row.set_widget_name(["export-width","export-height"][i]);row.set_value(f64::from(view.bounds[i]));row.set_visible(false);group.add(&row);row});
    let enlarge=adw::SwitchRow::builder().title(copy.enlarge.as_ref()).visible(false).build();enlarge.set_widget_name("export-enlarge");group.add(&enlarge);
    let resolution=combo(&group,&copy.resolution,"export-resolution",&[]);
    let ppi=crate::number_control::NumberControl::panel(numeric.ppi.clone(),&copy.ppi,localization.clone());ppi.set_widget_name("export-ppi");ppi.set_value(300.);group.add(&ppi);
    let size_note=gtk::Label::builder().wrap(true).xalign(0.).build();size_note.set_widget_name("export-size-description");size_note.add_css_class("dim-label");
    let delivery=adw::PreferencesGroup::new();let range=combo(&delivery,&copy.output,"export-output",&[]);let format=combo(&delivery,&copy.format,"export-format",&[]);
    let flatten=adw::SwitchRow::builder().title(copy.flatten.as_ref()).visible(false).build();flatten.set_widget_name("export-flatten");delivery.add(&flatten);
    let rendition=crate::panel_controls::segmented("export-rendition-view",&view.renditions.iter().map(|choice|(if choice.value {"sdr"}else{"hdr"},choice.label.as_ref())).collect::<Vec<_>>());rendition.set_visible(false);
    let color_group=adw::PreferencesGroup::new();let profile=ProfileChooser::new(w,&copy.profile,"export-space",snapshot.composition().color.space,ProfilePurpose::Output);color_group.add(&profile.row);
    let depth=combo(&color_group,&copy.depth,"export-depth",&[]);let background=combo(&color_group,&copy.background,"export-background",&[]);
    let quality=crate::number_control::NumberControl::panel(numeric.quality.clone(),&copy.quality,localization.clone());quality.set_widget_name("export-jpeg-quality");quality.set_value(90.);quality.set_visible(false);delivery.add(&quality);
    let metadata=combo(&delivery,&view.metadata.label,"export-metadata",&[]);let remove_location=adw::SwitchRow::builder().title(view.metadata.remove_location.as_ref()).active(true).build();remove_location.set_widget_name("export-remove-location");delivery.add(&remove_location);
    let metadata_note=gtk::Label::builder().wrap(true).xalign(0.).visible(false).build();metadata_note.set_widget_name("export-metadata-note");metadata_note.add_css_class("dim-label");
    let advanced_group=adw::PreferencesGroup::new();let advanced=adw::ExpanderRow::builder().title(copy.conversion.as_ref()).build();advanced.set_widget_name("export-advanced");advanced_group.add(&advanced);
    let clip=adw::SwitchRow::builder().title(copy.clip_hdr.as_ref()).subtitle(copy.clip_help.as_ref()).visible(false).build();clip.set_widget_name("export-hdr-clip");let clipping_group=adw::PreferencesGroup::new();clipping_group.set_widget_name("export-clipping-group");clipping_group.add(&clip);clip.bind_property("visible",&clipping_group,"visible").sync_create().build();
    let intent=choice(&copy.intent,"export-intent",&[]);advanced.add_row(&intent);
    let print=adw::ActionRow::builder().title(copy.use_print_profile.as_ref()).activatable(true).visible(view.show_print_profile).build();print.set_widget_name("export-print-profile");print.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));color_group.add(&print);
    let dither=adw::SwitchRow::builder().title(copy.reduce_banding.as_ref()).subtitle(copy.dither_gradients.as_ref()).build();dither.set_widget_name("export-dither");color_group.add(&dither);
    let jpeg_hint=gtk::Label::builder().label(copy.jpeg_note.as_ref()).wrap(true).xalign(0.).visible(false).build();jpeg_hint.add_css_class("dim-label");
    let validation=gtk::Label::builder().wrap(true).xalign(0.).visible(false).build();validation.add_css_class("error");validation.set_widget_name("export-validation");
    let note=gtk::Label::builder().wrap(true).xalign(0.).build();note.add_css_class("dim-label");let compression=gtk::Label::builder().wrap(true).xalign(0.).build();compression.set_widget_name("export-preview-compression");compression.add_css_class("dim-label");
    let comparison=crate::files::preview::Comparison::for_output(w.snapshot_gpu()?,snapshot.capture.clone(),w.view_color(),&localization);comparison.set_headroom(w.picker_headroom());
    let content=gtk::Box::new(gtk::Orientation::Vertical,12);for widget in [delivery.upcast_ref::<gtk::Widget>(),metadata_note.upcast_ref(),comparison.widget.upcast_ref(),rendition.upcast_ref(),compression.upcast_ref(),links.upcast_ref(),clipping_group.upcast_ref(),validation.upcast_ref()]{content.append(widget);}
    let (_,header)=navigation::page(&nav,&copy.title,"main",&content);let cancel=gtk::Button::with_label(&copy.common.cancel);cancel.set_widget_name("export-cancel");header.set_show_end_title_buttons(false);header.pack_start(&cancel);header.pack_end(&export);
    let size_body=gtk::Box::new(gtk::Orientation::Vertical,12);size_body.append(&group);size_body.append(&size_note);let (_,size_header)=navigation::page(&nav,&copy.image_size,"size",&size_body);
    let color_body=gtk::Box::new(gtk::Orientation::Vertical,12);for widget in [color_group.upcast_ref::<gtk::Widget>(),jpeg_hint.upcast_ref(),profile.error.upcast_ref(),note.upcast_ref(),advanced_group.upcast_ref()]{color_body.append(widget);}let (_,color_header)=navigation::page(&nav,&copy.color_transparency,"color",&color_body);
    let presets_body=gtk::Box::new(gtk::Orientation::Vertical,12);presets_body.append(&preset_group);let (_,presets_header)=navigation::page(&nav,&copy.presets,"presets",&presets_body);let presets=presets::Presentation::new(&presets_body,&view);
    dialog.set_child(Some(&nav));crate::page_dialog::configure(&dialog,crate::page_dialog::Metadata::new().root_header(&header).primary(&export).cancel(&cancel).navigation_header(&size_header).navigation_header(&color_header).navigation_header(&presets_header));
    let p=Rc::new(Projection {w:Rc::downgrade(w),id,dialog,nav,updating:Cell::new(false),profile_revision:Cell::new(0),preview_revision:Cell::new(0),fallback:Cell::new(None),response:Cell::new("cancel"),comparison,profile,export,cancel,preset,size,dimensions,enlarge,resolution,ppi,size_note,range,format,flatten,rendition,depth,background,quality,metadata,remove_location,metadata_note,advanced_group,advanced,clip,intent,print,dither,jpeg_hint,note,compression,validation,size_link,color_link,preset_link,presets});
    p.bind();p.render(view);let response=navigation::choose(&p.dialog,w.window.upcast_ref(),&p.response,presentation).await;
    p.comparison.close();p.comparison.finish().await;if response!="export"{return Ok(None);}
    let gpu=w.gpu.borrow();let session=&gpu.as_ref().ok_or("Canvas unavailable")?.session;
    Ok(Some(Choice {recipe:session.export_dialog_recipe(id).map_err(|error|error.message(&w.localization()))?,destination:session.export_dialog_view(id)?.destination,library:session.export_dialog_library(id)?}))
}
