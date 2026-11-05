use crate::{color_feature_copy::ExportCopy, *};
use layer_core::color::{ColorProfile, OutputDither, RenderingIntent, RgbSpace, SampleDepth};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportDialogCapabilities {
    pub formats: Vec<ExportFormat>,
    pub profile_import: bool,
}
impl ExportDialogCapabilities {
    fn supports(&self, format: ExportFormat) -> bool {
        self.formats.iter().any(|value| value.with_hdr_range_mapping(false) == format.with_hdr_range_mapping(false))
    }
}

#[derive(Clone)]
pub struct ExportDialogContext {
    pub source: DocumentExport,
    pub capabilities: ExportDialogCapabilities,
    pub profiles: Vec<ExportProfile>,
}
impl ExportDialogContext {
    pub fn new(source: DocumentExport, capabilities: ExportDialogCapabilities, profiles: Vec<ExportProfile>) -> Self {
        Self { source, capabilities, profiles }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDialogPage { Root, Size, Color, Presets, SavePreset }
impl ExportDialogPage {
    pub fn from_tag(tag:&str)->Option<Self> {[Self::Root,Self::Size,Self::Color,Self::Presets].into_iter().find(|page|page.tag()==tag)}
    pub fn tag(self) -> &'static str {
        match self { Self::Root=>"main", Self::Size=>"size", Self::Color=>"color", Self::Presets|Self::SavePreset=>"presets" }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDialogOutput { Sdr, Native, Jpeg, Avif, Exr }
impl ExportDialogOutput {
    fn format(self) -> Option<ExportFormat> {
        match self { Self::Sdr=>None, Self::Native=>Some(ExportFormat::PngHdr), Self::Jpeg=>Some(ExportFormat::JpegHdr), Self::Avif=>Some(ExportFormat::AvifHdr), Self::Exr=>Some(ExportFormat::Exr) }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDialogPresetOperation { Save, Update, Remove, Reset }

#[derive(Serialize, Deserialize)]
pub struct ExportDialogPresetWrite {
    pub expected: ExportPresets,
    pub next: ExportPresets,
    pub selected: usize,
    pub operation: ExportDialogPresetOperation,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ExportDialogPreview {
    pub ready: bool,
    pub transparent: Option<bool>,
    pub range_exceeded: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportDialogAction {
    Edit { action: ExportDraftAction },
    Output { value: ExportDialogOutput },
    OutputContact,
    Fit { enabled: bool },
    Bound { axis: usize, value: u32 },
    Enlarge { enabled: bool },
    Ppi { value: u32 },
    Flatten { enabled: bool },
    Dither { enabled: bool },
    Intent { value: RenderingIntent },
    MetadataKeep { value: MetadataKeep },
    RemoveLocation { enabled: bool },
    Rendition { sdr: bool },
    Profile { result: Result<ExportProfile, ColorFeatureError> },
    Preview { value: ExportDialogPreview },
    Navigate { page: ExportDialogPage },
    Back,
    Preset { index: usize },
    Name { value: String },
    BeginPreset { operation: ExportDialogPresetOperation },
    PresetCancel,
    PresetResult { result: Result<ExportDialogPresetWrite, ColorFeatureError> },
}

pub struct ExportDialogState {
    pub(crate) context: ExportDialogContext,
    pub(crate) library: ExportPresets,
    profiles: Vec<ExportProfile>,
    fields: ExportRecipe<usize>,
    page: ExportDialogPage,
    output: ExportDialogOutput,
    bounds: [u32; 2],
    enlarge: bool,
    ppi: u32,
    flatten: bool,
    dither: bool,
    clip: bool,
    preset: usize,
    destination: usize,
    name: String,
    preset_busy: bool,
    preset_outcome: Option<Result<ExportDialogPresetOperation, ColorFeatureError>>,
    profile_error: Option<ColorFeatureError>,
    profile_revision: u64,
    preview_revision: u64,
    preview: ExportDialogPreview,
    recommend: bool,
    preview_sdr: bool,
}

#[derive(Serialize)]
pub struct ExportDialogView {
    pub dialog: DialogIntent,
    pub copy: ExportCopy,
    pub numeric: ExportNumericControls,
    pub page: ExportDialogPage,
    pub fields: ExportRecipe<usize>,
    pub profile_revision: u64,
    pub original_profiles: Vec<ExportChoice<usize>>,
    pub preview_revision: u64,
    pub output: ExportDialogOutput,
    pub preview_sdr: bool,
    pub renditions: [ExportChoice<bool>; 2],
    pub comparison_labels: [Arc<str>; 2],
    pub outputs: Vec<ExportChoice<ExportDialogOutput>>,
    pub formats: Vec<ExportChoice<ExportFormat>>,
    pub backgrounds: Vec<ExportChoice<ExportBackground>>,
    pub depths: Vec<ExportChoice<SampleDepth>>,
    pub intents: Vec<ExportChoice<RenderingIntent>>,
    pub resolutions: Vec<ExportChoice<ExportResolution>>,
    pub metadata: ExportMetadataView,
    pub bounds: [u32; 2],
    pub enlarge: bool,
    pub ppi: u32,
    pub flatten: bool,
    pub dither: bool,
    pub clip: bool,
    pub show_output: bool,
    pub show_size: bool,
    pub show_ppi: bool,
    pub show_color: bool,
    pub show_background: bool,
    pub show_depth: bool,
    pub depth_enabled: bool,
    pub show_conversion: bool,
    pub show_dither: bool,
    pub show_quality: bool,
    pub show_flatten: bool,
    pub show_rendition: bool,
    pub show_clip: bool,
    pub show_metadata: bool,
    pub show_location: bool,
    pub show_metadata_note: bool,
    pub show_print_profile: bool,
    pub print_profile_name: Option<String>,
    pub profile_ready: bool,
    pub profile_import: bool,
    pub jpeg_note: bool,
    pub precision_note: Option<Arc<str>>,
    pub compression_note: bool,
    pub size_summary: String,
    pub size_subtitle: String,
    pub color_title: Arc<str>,
    pub color_subtitle: String,
    pub preset: usize,
    pub destination: usize,
    pub preset_names: Vec<String>,
    pub preset_enabled: Vec<bool>,
    pub preset_labels: [String; 4],
    pub preset_save: bool,
    pub preset_update: bool,
    pub preset_remove: bool,
    pub preset_reset: bool,
    pub preset_busy: bool,
    pub preset_status: Option<String>,
    pub preset_error: bool,
    pub name: String,
    pub name_valid: bool,
    pub error: Option<String>,
    pub can_export: bool,
}

impl ExportDialogState {
    pub fn new(context: ExportDialogContext, library: ExportPresets) -> Result<Self, ColorFeatureError> {
        let mut profiles=context.profiles.clone();
        for space in RgbSpace::ALL {
            let profile = ExportProfile::builtin(space);
            if !profiles.contains(&profile) { profiles.push(profile); }
        }
        let recipe = library.recipe(0, context.source.composition().color)?;
        let extent = context.source.composition().size;
        let mut state = Self {
            context, library, profiles, fields: recipe.clone().with_profile(0), page: ExportDialogPage::Root,
            output: ExportDialogOutput::Sdr, bounds: extent, enlarge: false, ppi: 300,
            flatten: false, dither: false, clip: false, preset: 0, destination: 0, name: String::new(),
            preset_busy: false, preset_outcome: None, profile_error: None,
            profile_revision: 0, preview_revision: 1, preview: Default::default(), recommend: false, preview_sdr: false,
        };
        state.load(recipe);
        if state.context.source.composition().color.depth.is_float() && state.supports_output(ExportDialogOutput::Native) {
            state.output = ExportDialogOutput::Native;
            state.preset = 3;
            state.recommend = true;
        }
        Ok(state)
    }
    fn intern(&mut self, profile: ExportProfile) -> usize {
        if let Some(index) = self.profiles.iter().position(|value| value == &profile) { return index; }
        let index = self.profiles.len(); self.profiles.push(profile); index
    }
    pub fn profile(&self, index: usize) -> Option<&ExportProfile> { self.profiles.get(index) }
    fn supports_output(&self, output: ExportDialogOutput) -> bool {
        output.format().is_none_or(|format| self.context.source.composition().color.depth.is_float() && self.context.capabilities.supports(format))
    }
    fn load(&mut self, recipe: ExportRecipe) {
        self.output = match recipe.format { ExportFormat::Exr=>ExportDialogOutput::Exr, format=>match format.gainmap() {
            Some(layer_color::photo::GainMapFormat::Jpeg)=>ExportDialogOutput::Jpeg,
            Some(layer_color::photo::GainMapFormat::Avif)=>ExportDialogOutput::Avif,
            None=>if format.is_hdr() { ExportDialogOutput::Native } else { ExportDialogOutput::Sdr },
        }};
        self.clip = recipe.format.maps_hdr_range();
        self.flatten = self.output == ExportDialogOutput::Jpeg && recipe.background != ExportBackground::Preserve;
        self.dither = recipe.encoding.dither != OutputDither::None;
        if let ExportSize::Fit { bounds, enlarge } = recipe.size { self.bounds = bounds; self.enlarge = enlarge; }
        if let ExportResolution::Ppi(value) = recipe.resolution { self.ppi = value; }
        let index = self.intern(recipe.profile.clone());
        let background = if recipe.format.is_hdr() && recipe.format.gainmap().is_none() { self.fields.background } else { recipe.background };
        self.fields = ExportRecipe { format: if recipe.format.is_hdr() { ExportFormat::Png } else { recipe.format },
            depth: if recipe.depth == SampleDepth::U16 { SampleDepth::U16 } else { SampleDepth::U8 }, background,
            ..recipe.with_profile(index) };
        self.profile_error = None;
        self.profile_revision = self.profile_revision.wrapping_add(1);
        self.recommend = false;
    }
    pub fn recipe(&self) -> Result<ExportRecipe, ColorFeatureError> {
        if self.output == ExportDialogOutput::Sdr {
            if let Some(error) = &self.profile_error { return Err(error.clone()); }
        }
        let profile = self.profile(self.fields.profile).ok_or(ColorFeatureError::ProfileMissing)?.clone();
        let mut recipe = self.fields.clone().with_profile(profile);
        recipe.encoding.dither = if recipe.depth == SampleDepth::U8 && self.dither { OutputDither::Stochastic8 } else { OutputDither::None };
        if let Some(format) = self.output.format() {
            recipe.format = format.with_hdr_range_mapping(self.clip);
            recipe.profile = ExportProfile::builtin(if format == ExportFormat::Exr { self.context.source.composition().color.space } else { RgbSpace::Srgb });
            if self.output == ExportDialogOutput::Jpeg {
                recipe.background = if !self.flatten { ExportBackground::Preserve } else if recipe.background == ExportBackground::Preserve { ExportBackground::White } else { recipe.background };
            }
            recipe = recipe.draft_for_color_canonical(self.context.source.composition().color, ExportDraftAction::Refresh).recipe;
        }
        if !self.supports_output(self.output) || !self.context.capabilities.supports(recipe.format) { return Err(ColorFeatureError::ExportCombination); }
        recipe.validate()?;
        recipe.output_extent(self.context.source.composition().size)?;
        recipe.output_resolution(self.context.source.composition().resolution)?;
        Ok(recipe)
    }
    fn edit(&mut self, action: ExportDraftAction) {
        let recipe = self.fields.clone().with_profile(self.profiles[self.fields.profile].clone()).draft_canonical(action).recipe;
        let profile = self.intern(recipe.profile.clone());
        self.fields = recipe.with_profile(profile);
    }
    fn size(&mut self) {
        if matches!(self.fields.size, ExportSize::Fit { .. }) {
            self.fields.size = ExportSize::Fit { bounds: self.bounds, enlarge: self.enlarge };
        }
    }
    pub fn apply(&mut self, action: ExportDialogAction) -> Result<(), ColorFeatureError> {
        use ExportDialogAction as A;
        let mut edited = true;
        let mut preview = true;
        match action {
            A::Edit { action } => {
                preview = match &action { ExportDraftAction::Metadata(_)|ExportDraftAction::Resolution(_)=>false,
                    ExportDraftAction::Quality(_)=>matches!(self.output,ExportDialogOutput::Jpeg|ExportDialogOutput::Avif), _=>true };
                match action {
                    ExportDraftAction::ClipHdrRange(value)=>self.clip=value,
                    ExportDraftAction::Resolution(ExportResolution::Ppi(value))=>{self.ppi=value;self.edit(ExportDraftAction::Resolution(ExportResolution::Ppi(value)));},
                    action=>self.edit(action),
                }
            }
            A::Output { value } => { if !self.supports_output(value) { return Err(ColorFeatureError::ExportCombination); } self.recommend=false;if value==self.output {edited=false;preview=false;}else{self.output=value;} }
            A::OutputContact=>{self.recommend=false;edited=false;preview=false;}
            A::Fit { enabled }=>{self.fields.size=if enabled {ExportSize::Fit {bounds:self.bounds,enlarge:self.enlarge}}else{ExportSize::Original};}
            A::Bound { axis,value }=>{if axis>=2 {return Err(ColorFeatureError::ExportDimensions);}self.bounds[axis]=value;self.size();}
            A::Enlarge { enabled }=>{self.enlarge=enabled;self.size();}
            A::Ppi { value }=>{self.ppi=value;if matches!(self.fields.resolution,ExportResolution::Ppi(_)){self.fields.resolution=ExportResolution::Ppi(value);}preview=false;}
            A::Flatten { enabled }=>self.flatten=enabled,
            A::Dither { enabled }=>self.dither=enabled,
            A::Intent { value }=>self.fields.encoding.conversion.intent=value,
            A::MetadataKeep { value }=>{self.fields.metadata.keep=value;preview=false;}
            A::RemoveLocation { enabled }=>{self.fields.metadata.remove_location=enabled;preview=false;}
            A::Rendition { sdr }=>{self.preview_sdr=sdr&&matches!(self.output,ExportDialogOutput::Jpeg|ExportDialogOutput::Avif);edited=false;preview=false;}
            A::Profile { result }=>match result {
                Ok(profile)=>{self.edit(ExportDraftAction::Profile(profile));self.profile_error=None;self.profile_revision=self.profile_revision.wrapping_add(1);}
                Err(error)=>self.profile_error=Some(error),
            },
            A::Preview { value }=>{
                self.preview=value;edited=false;preview=false;
                if let Some(transparent)=value.transparent && self.recommend {
                    self.recommend=false;
                    let output=if transparent {ExportDialogOutput::Avif}else{ExportDialogOutput::Jpeg};
                    if self.supports_output(output) {self.output=output;edited=true;preview=true;}
                }
            }
            A::Navigate { page }=>{self.page=page;edited=false;preview=false;}
            A::Back=>{self.page=if self.page==ExportDialogPage::SavePreset {ExportDialogPage::Presets}else{ExportDialogPage::Root};edited=false;preview=false;}
            A::Preset { index }=>{edited=false;if index==self.preset {preview=false;}else{let recipe=self.library.recipe(index,self.context.source.composition().color)?;if !self.context.capabilities.supports(recipe.format){return Err(ColorFeatureError::ExportCombination);}self.load(recipe);self.preset=index;self.destination=index;}}
            A::Name { value }=>{self.name=value;edited=false;preview=false;}
            A::BeginPreset { operation }=>{
                edited=false;preview=false;
                if self.preset_busy {return Ok(());}
                if matches!(operation,ExportDialogPresetOperation::Save|ExportDialogPresetOperation::Update) && let Err(error)=self.recipe() {
                    self.preset_outcome=Some(Err(error));
                } else {
                    self.preset_busy=true;
                    if operation==ExportDialogPresetOperation::Save {self.page=ExportDialogPage::SavePreset;self.name.clear();}
                }
            }
            A::PresetCancel=>{self.preset_busy=false;self.page=ExportDialogPage::Presets;edited=false;preview=false;}
            A::PresetResult { result }=>{
                self.preset_busy=false;edited=false;preview=false;
                if self.page==ExportDialogPage::SavePreset {self.page=ExportDialogPage::Presets;}
                match result {
                    Ok(write)=>{let recipe=write.next.recipe(write.selected,self.context.source.composition().color)?;self.library=write.next;self.load(recipe);self.preset=write.selected;self.destination=write.selected;self.preset_outcome=Some(Ok(write.operation));preview=true;}
                    Err(error)=>self.preset_outcome=Some(Err(error)),
                }
            }
        }
        if edited {self.preset=3;}
        if !matches!(self.output,ExportDialogOutput::Jpeg|ExportDialogOutput::Avif) {self.preview_sdr=false;}
        if preview {self.preview_revision=self.preview_revision.wrapping_add(1);self.preview.ready=false;self.preview.range_exceeded=false;}
        Ok(())
    }
    pub fn preset_write(&self, operation: ExportDialogPresetOperation) -> Result<ExportDialogPresetWrite, ColorFeatureError> {
        let mut next = self.library.clone();
        let action = match operation {
            ExportDialogPresetOperation::Save=>ExportPresetAction::Save {name:self.name.clone(),recipe:self.recipe()?},
            ExportDialogPresetOperation::Update=>ExportPresetAction::Update {index:self.destination,recipe:self.recipe()?},
            ExportDialogPresetOperation::Remove=>ExportPresetAction::Remove {index:self.destination},
            ExportDialogPresetOperation::Reset=>ExportPresetAction::Reset {index:self.destination},
        };
        let view=next.operate(action,self.context.source.composition().color)?;
        let selected=if operation==ExportDialogPresetOperation::Remove {3}else{view.index.unwrap_or(self.destination)};
        Ok(ExportDialogPresetWrite {expected:self.library.clone(),next,selected,operation})
    }
    pub fn print_profile(&self) -> Option<(ColorProfile, String, RenderingIntent)> {
        self.context.source.output().proof.as_ref().map(|proof|(proof.profile.clone(),proof.name.clone(),proof.conversion.intent))
    }
    pub fn view(&self, localizer: &Localizer) -> ExportDialogView {
        use crate::MessageId as M;
        let copy=ExportCopy::new(localizer);
        let color=self.context.source.composition().color;
        let hdr=self.output!=ExportDialogOutput::Sdr;
        let profile=&self.profiles[self.fields.profile];
        let original_profiles=self.context.profiles.iter().filter(|profile|!matches!(profile.profile,ColorProfile::Builtin(_)))
            .filter_map(|profile|self.profiles.iter().position(|value|value==profile).map(|value|ExportChoice {
                value,label:ExportProfileCaption::Original {name:profile.name.clone()}.message(localizer).into(),
            })).collect();
        let draft=self.fields.clone().with_profile(profile.clone()).draft_localized(ExportDraftAction::Refresh,localizer);
        let formats=draft.formats.into_iter().filter(|format|self.context.capabilities.supports(*format))
            .map(|value|ExportChoice {value,label:match value {ExportFormat::Png=>"PNG".into(),ExportFormat::Tiff=>"TIFF".into(),ExportFormat::Jpeg=>"JPEG".into(),format=>format.localized_name(localizer)}}).collect();
        let outputs=[(ExportDialogOutput::Sdr,Arc::<str>::from("SDR")),(ExportDialogOutput::Native,copy.hdr_native.clone()),
            (ExportDialogOutput::Jpeg,copy.format_jpeg_hdr.clone()),(ExportDialogOutput::Avif,copy.format_avif_hdr.clone()),(ExportDialogOutput::Exr,copy.format_exr.clone())]
            .into_iter().filter(|(value,_)|self.supports_output(*value)).map(|(value,label)|ExportChoice {value,label}).collect::<Vec<_>>();
        let show_output=color.depth.is_float()&&outputs.len()>1;
        let jpeg=self.output==ExportDialogOutput::Jpeg;
        let backgrounds=if jpeg {
            vec![(ExportBackground::Preserve,copy.keep_transparency.clone()),(ExportBackground::White,copy.white.clone()),(ExportBackground::Black,copy.black.clone())]
                .into_iter().map(|(value,label)|ExportChoice {value,label}).collect()
        }else{draft.choices.backgrounds};
        let format=self.output.format().map_or(self.fields.format,|format|format.with_hdr_range_mapping(self.clip));
        let metadata=ExportMetadataView::localized_for(format,self.fields.metadata.keep,localizer);
        let physical=match self.fields.resolution {ExportResolution::Master=>self.context.source.composition().resolution,
            ExportResolution::Ppi(value)=>Some(layer_core::ImageResolution::ppi(value)),ExportResolution::Omit=>None};
        let (size_summary,size_subtitle)=match self.fields.size.extent(self.context.source.composition().size) {
            Ok([width,height])=>{
                let resolution=physical.map_or_else(||copy.no_resolution.to_string(),|resolution|{
                    let [x,y]=resolution.pixels_per_inch();let mut args=FluentArgs::new();
                    args.set("width",format!("{:.2}",f64::from(width)/x*2.54));args.set("height",format!("{:.2}",f64::from(height)/y*2.54));
                    args.set("x_ppi",format!("{x:.2}"));args.set("y_ppi",format!("{y:.2}"));
                    localizer.format(M::COLOR_FEATURES_EXPORT_PHYSICAL_RESOLUTION,&args)
                });
                let mut args=FluentArgs::new();args.set("width",width.to_string());args.set("height",height.to_string());args.set("resolution",resolution);
                (localizer.format(M::COLOR_FEATURES_EXPORT_SIZE_SUMMARY,&args),format!("{width} × {height} px"))
            }
            Err(error)=>(error.message(localizer),String::new()),
        };
        let (color_title,color_subtitle)=if jpeg {
            (copy.background.clone(),if !self.flatten {copy.no_flatten.to_string()}else if self.fields.background==ExportBackground::Black {copy.black.to_string()}else{copy.white.to_string()})
        }else{
            (copy.color_transparency.clone(),format!("{} · {}-bit · {}",profile.display_name(localizer),if self.fields.depth==SampleDepth::U8 {8}else{16},
                match self.fields.background {ExportBackground::Preserve=>copy.transparent.as_ref(),ExportBackground::White=>copy.white.as_ref(),ExportBackground::Black=>copy.black.as_ref()}))
        };
        let preset_names=self.library.localized_names(color,localizer);
        let preset_enabled=(0..preset_names.len()).map(|index|self.library.recipe(index,color).is_ok_and(|recipe|self.context.capabilities.supports(recipe.format))).collect();
        let name=preset_names.get(self.destination).map(String::as_str).unwrap_or_default();
        let preset_labels=[copy.save_new_preset.to_string(),
            color_feature_copy::named(localizer,M::COLOR_FEATURES_EXPORT_UPDATE_NAMED,name),
            color_feature_copy::named(localizer,M::COLOR_FEATURES_EXPORT_REMOVE_NAMED,name),
            color_feature_copy::named(localizer,M::COLOR_FEATURES_EXPORT_RESET_NAMED,name)];
        let preset_status=self.preset_outcome.as_ref().map(|result|match result {
            Ok(ExportDialogPresetOperation::Remove)=>copy.preset_removed.to_string(),
            Ok(ExportDialogPresetOperation::Reset)=>copy.preset_reset.to_string(),
            Ok(_)=>copy.preset_saved.to_string(),Err(error)=>error.preset_message(localizer),
        });
        let precision_note=if hdr||self.fields.depth!=SampleDepth::U8 {None}else if color.depth==SampleDepth::U16 {Some(copy.precision_note.clone())}
            else if self.profile_error.is_none()&&profile.profile==ColorProfile::Builtin(RgbSpace::ProPhoto) {Some(localizer.text(M::DOCUMENTS_PROPHOTO_NOTE))}else{None};
        let result=self.recipe();
        let can_export=result.is_ok()&&(!hdr||(self.preview.ready&&!self.preview.range_exceeded));
        let error=result.err().map(|error|error.message(localizer));
        let show_metadata=!self.context.source.metadata().is_empty();
        let transparent=self.preview.transparent.unwrap_or(true);
        ExportDialogView {
            dialog:copy.dialog,numeric:ExportNumericControls::default(),page:self.page,fields:self.fields.clone(),
            profile_revision:self.profile_revision,original_profiles,preview_revision:self.preview_revision,output:self.output,preview_sdr:self.preview_sdr,
            comparison_labels:color_feature_copy::ComparisonCopy::new(localizer).output_labels(color.depth.is_float(),hdr,false,self.preview_sdr).map(Clone::clone),
            renditions:[ExportChoice {value:false,label:"HDR".into()},ExportChoice {value:true,label:"SDR".into()}],outputs,formats,backgrounds,
            depths:draft.choices.depths,intents:draft.choices.intents,
            resolutions:vec![ExportChoice {value:ExportResolution::Master,label:copy.keep_resolution.clone()},ExportChoice {value:ExportResolution::Ppi(self.ppi),label:copy.custom.clone()},ExportChoice {value:ExportResolution::Omit,label:copy.omit.clone()}],
            bounds:self.bounds,enlarge:self.enlarge,ppi:self.ppi,flatten:self.flatten,dither:self.dither,clip:self.clip,
            show_output,show_size:matches!(self.fields.size,ExportSize::Fit {..}),show_ppi:matches!(self.fields.resolution,ExportResolution::Ppi(_)),
            show_color:!hdr||(jpeg&&(transparent||self.flatten)),show_background:!hdr||jpeg,show_depth:!hdr&&draft.depths.len()>1,depth_enabled:draft.depths.len()>1,show_conversion:!hdr,
            show_dither:!hdr&&self.fields.depth==SampleDepth::U8,show_quality:matches!(self.output,ExportDialogOutput::Jpeg|ExportDialogOutput::Avif)||(!hdr&&self.fields.format==ExportFormat::Jpeg),
            show_flatten:jpeg&&(transparent||self.flatten),show_rendition:matches!(self.output,ExportDialogOutput::Jpeg|ExportDialogOutput::Avif),
            show_clip:hdr&&self.output!=ExportDialogOutput::Exr&&(self.clip||self.preview.range_exceeded),
            show_metadata:show_metadata&&metadata.available,show_location:show_metadata&&metadata.location,show_metadata_note:show_metadata&&metadata.note.is_some(),metadata,
            show_print_profile:!hdr&&self.context.source.output().proof.is_some(),print_profile_name:self.context.source.output().proof.as_ref().map(|proof|proof.name.clone()),
            profile_ready:self.profile_error.is_none(),profile_import:self.context.capabilities.profile_import,
            jpeg_note:!hdr&&self.fields.format==ExportFormat::Jpeg,precision_note,compression_note:!hdr&&self.fields.format==ExportFormat::Jpeg,
            size_summary,size_subtitle,color_title,color_subtitle,preset:self.preset,destination:self.destination,preset_names,preset_enabled,preset_labels,
            preset_save:true,preset_update:self.destination>=4,preset_remove:self.destination>=4,preset_reset:self.destination<4,
            preset_busy:self.preset_busy,preset_status,preset_error:self.preset_outcome.as_ref().is_some_and(Result::is_err),
            name:self.name.clone(),name_valid:ExportPresets::validate_name(&self.name).is_ok(),error,can_export,copy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::Recorder;
    fn state(color:layer_core::color::DocumentColor) -> ExportDialogState {
        let mut session=UiSession::blank(Recorder::default(),[513,257],Platform::Gtk).unwrap();
        session.dispatch(UiAction::Invoke {command:CommandId::ExportDocument}).unwrap();
        let id=session.state().requests.last().unwrap().id;
        let mut source=session.capture_project_export(id).unwrap();
        let artwork=Arc::make_mut(&mut source.capture.artwork);
        let root=artwork.root;
        artwork.compositions.get_mut(root).unwrap().color=color;
        ExportDialogState::new(ExportDialogContext::new(source,ExportDialogCapabilities {
            formats:vec![ExportFormat::Png,ExportFormat::Tiff,ExportFormat::Jpeg,ExportFormat::Webp,ExportFormat::PngHdr,ExportFormat::JpegHdr,ExportFormat::AvifHdr,ExportFormat::Exr],profile_import:true,
        },Vec::new()),ExportPresets::default()).unwrap()
    }
    fn view(state:&ExportDialogState)->ExportDialogView {state.view(&Localizer::shared(UiLanguage::English))}
    #[test]
    fn export_dialog_original_profiles_keep_localized_captions_and_catalog_indices() {
        let seed=state(Default::default());
        let builtin=ExportProfile::builtin(RgbSpace::Srgb);
        let original=ExportProfile {profile:ColorProfile::Icc(layer_color::profile_bytes(&builtin.profile).unwrap().into()),name:"Source photo".into(),channels:builtin.channels};
        let context=ExportDialogContext::new(seed.context.source,seed.context.capabilities,vec![builtin,original.clone()]);
        let mut state=ExportDialogState::new(context,ExportPresets::default()).unwrap();
        for language in [UiLanguage::English,UiLanguage::French] {
            let localization=Localizer::shared(language);let view=state.view(&localization);
            assert_eq!(view.original_profiles.len(),1);
            let choice=&view.original_profiles[0];
            assert_eq!(state.profile(choice.value),Some(&original));
            assert_eq!(choice.label.as_ref(),ExportProfileCaption::Original {name:original.name.clone()}.message(&localization));
            assert!(!serde_json::to_string(&view).unwrap().contains("Icc"));
        }
        state.apply(ExportDialogAction::Profile {result:Ok(ExportProfile::builtin(RgbSpace::DisplayP3))}).unwrap();
        assert_eq!(view(&state).original_profiles.len(),1);
    }
    #[test]
    fn export_dialog_preview_readiness_belongs_to_the_current_recipe() {
        let mut state=state(layer_core::color::DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::F32});
        state.apply(ExportDialogAction::OutputContact).unwrap();
        state.apply(ExportDialogAction::Output {value:ExportDialogOutput::Native}).unwrap();
        state.apply(ExportDialogAction::Preview {value:ExportDialogPreview {ready:true,transparent:Some(false),range_exceeded:true}}).unwrap();
        assert!(!view(&state).can_export);
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::ClipHdrRange(true)}).unwrap();
        assert!(!view(&state).can_export);
        state.apply(ExportDialogAction::Preview {value:ExportDialogPreview {ready:true,transparent:Some(false),range_exceeded:false}}).unwrap();
        assert!(view(&state).can_export);
        state.apply(ExportDialogAction::Fit {enabled:true}).unwrap();
        assert!(!view(&state).can_export);
    }
    #[test]
    fn export_dialog_fields_survive_navigation_and_disabled_size_resolution_and_color_controls() {
        let mut state=state(Default::default());
        state.apply(ExportDialogAction::Fit {enabled:true}).unwrap();
        state.apply(ExportDialogAction::Bound {axis:0,value:300}).unwrap();
        state.apply(ExportDialogAction::Bound {axis:1,value:257}).unwrap();
        state.apply(ExportDialogAction::Enlarge {enabled:true}).unwrap();
        state.apply(ExportDialogAction::Fit {enabled:false}).unwrap();
        state.apply(ExportDialogAction::Ppi {value:144}).unwrap();
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Resolution(ExportResolution::Ppi(144))}).unwrap();
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Resolution(ExportResolution::Master)}).unwrap();
        state.apply(ExportDialogAction::Dither {enabled:true}).unwrap();
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Depth(SampleDepth::U16)}).unwrap();
        for page in [ExportDialogPage::Size,ExportDialogPage::Color,ExportDialogPage::Presets] {
            state.apply(ExportDialogAction::Navigate {page}).unwrap();state.apply(ExportDialogAction::Back).unwrap();
        }
        assert_eq!(view(&state).page,ExportDialogPage::Root);
        assert_eq!(view(&state).bounds,[300,257]);assert_eq!(view(&state).ppi,144);assert!(view(&state).enlarge);
        assert_eq!(state.recipe().unwrap().size,ExportSize::Original);
        assert_eq!(state.recipe().unwrap().encoding.dither,OutputDither::None);
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Depth(SampleDepth::U8)}).unwrap();
        assert_eq!(state.recipe().unwrap().encoding.dither,OutputDither::Stochastic8);
        assert_eq!(view(&state).preset,3);assert_eq!(view(&state).destination,0);
    }
    #[test]
    fn export_dialog_capabilities_filter_outputs_and_presets_do_not_publish_before_storage_succeeds() {
        let mut state=state(layer_core::color::DocumentColor {depth:SampleDepth::F16,..Default::default()});
        state.context.capabilities.formats=vec![ExportFormat::Png,ExportFormat::Tiff];state.context.capabilities.profile_import=false;
        state.apply(ExportDialogAction::Output {value:ExportDialogOutput::Sdr}).unwrap();
        assert_eq!(view(&state).outputs.len(),1);assert!(!view(&state).show_output);assert!(!view(&state).profile_import);
        assert!(!view(&state).preset_enabled[2]);assert!(state.apply(ExportDialogAction::Preset {index:2}).is_err());
        assert!(state.apply(ExportDialogAction::Output {value:ExportDialogOutput::Avif}).is_err());
        state.apply(ExportDialogAction::Name {value:"Print delivery".into()}).unwrap();
        let write=state.preset_write(ExportDialogPresetOperation::Save).unwrap();
        assert_eq!(state.library.names().count(),0);assert_eq!(write.next.names().count(),1);
        state.apply(ExportDialogAction::BeginPreset {operation:ExportDialogPresetOperation::Save}).unwrap();assert!(view(&state).preset_busy);
        state.apply(ExportDialogAction::PresetResult {result:Err(ColorFeatureError::PresetChanged)}).unwrap();
        assert_eq!(state.library.names().count(),0);assert!(!view(&state).preset_busy);
        state.apply(ExportDialogAction::PresetResult {result:Ok(write)}).unwrap();
        assert_eq!(state.library.names().count(),1);assert_eq!(view(&state).destination,4);
    }
    #[test]
    fn export_dialog_preview_recommendation_yields_to_contact_and_quality_only_previews_gain_maps() {
        let mut state=state(layer_core::color::DocumentColor {depth:SampleDepth::F16,..Default::default()});
        state.apply(ExportDialogAction::OutputContact).unwrap();
        state.apply(ExportDialogAction::Preview {value:ExportDialogPreview {ready:true,transparent:Some(true),range_exceeded:true}}).unwrap();
        assert_eq!(view(&state).output,ExportDialogOutput::Native);assert!(view(&state).show_clip);
        let revision=view(&state).preview_revision;
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Quality(73)}).unwrap();assert_eq!(view(&state).preview_revision,revision);
        state.apply(ExportDialogAction::Output {value:ExportDialogOutput::Jpeg}).unwrap();
        let revision=view(&state).preview_revision;
        state.apply(ExportDialogAction::Edit {action:ExportDraftAction::Quality(74)}).unwrap();assert_eq!(view(&state).preview_revision,revision+1);
        assert_eq!(state.recipe().unwrap().jpeg_quality,74);
    }
    #[test]
    fn export_dialog_released_presets_select_the_same_recipe_and_keep_all_saved_values() {
        let seed=state(layer_core::color::DocumentColor {space:RgbSpace::ProPhoto,depth:SampleDepth::U16});
        let library=ExportPresets::restore(include_bytes!("../tests/compat/1.0.15/export-presets-complete"));
        assert!(library.names().count()>0);
        for index in 0..4+library.names().count() {
            let expected=library.recipe(index,seed.context.source.composition().color).unwrap();
            let mut state=ExportDialogState::new(seed.context.clone(),library.clone()).unwrap();state.apply(ExportDialogAction::Preset {index}).unwrap();
            assert_eq!(state.recipe().unwrap(),expected,"released preset {index}");
        }
    }
    #[test]
    fn export_dialog_view_references_profiles_without_serializing_their_bytes() {
        let mut state=state(Default::default());
        let profile=ExportProfile {profile:ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap().into()),channels:layer_core::color::ProfileChannels::Rgb,name:"Literal 日本語 {profile}".into()};
        state.apply(ExportDialogAction::Profile {result:Ok(profile.clone())}).unwrap();
        let json=serde_json::to_value(view(&state)).unwrap();
        assert!(json["fields"]["profile"].is_number());
        assert!(json["color_subtitle"].as_str().unwrap().contains(&profile.name));
        assert!(!json.to_string().contains("Icc"));
        assert_eq!(state.recipe().unwrap().profile,profile);
    }
    #[test]
    fn export_dialog_belongs_to_its_request_and_retires_with_completion() {
        let mut session=UiSession::blank(Recorder::default(),[412,891],Platform::Gtk).unwrap();
        session.dispatch(UiAction::Invoke {command:CommandId::ExportDocument}).unwrap();
        let id=session.state().requests.last().unwrap().id;
        let source=session.capture_project_export(id).unwrap();
        let context=ExportDialogContext::new(source,ExportDialogCapabilities {formats:vec![ExportFormat::Png],profile_import:false},Vec::new());
        session.begin_export_dialog(id,context,ExportPresets::default()).unwrap();
        session.export_dialog_action(id,ExportDialogAction::Navigate {page:ExportDialogPage::Size}).unwrap();
        assert_eq!(session.export_dialog_view(id).unwrap().page,ExportDialogPage::Size);
        assert!(session.export_dialog_view(id.wrapping_add(1)).is_err());
        session.complete_document_request(id,Ok(false)).unwrap();
        assert!(session.export_dialog_view(id).is_err());
        assert!(session.export_dialog_recipe(id).is_err());
    }
    #[test]
    fn export_dialog_save_prompt_starts_empty_and_requires_an_admitted_recipe() {
        let mut state=state(Default::default());
        state.apply(ExportDialogAction::Navigate {page:ExportDialogPage::Presets}).unwrap();
        state.apply(ExportDialogAction::BeginPreset {operation:ExportDialogPresetOperation::Save}).unwrap();
        state.apply(ExportDialogAction::Name {value:"Abandoned draft".into()}).unwrap();
        state.apply(ExportDialogAction::PresetCancel).unwrap();
        state.apply(ExportDialogAction::BeginPreset {operation:ExportDialogPresetOperation::Save}).unwrap();
        assert_eq!(view(&state).page,ExportDialogPage::SavePreset);assert!(view(&state).name.is_empty());
        state.apply(ExportDialogAction::PresetCancel).unwrap();
        state.apply(ExportDialogAction::Profile {result:Err(ColorFeatureError::ProfileMissing)}).unwrap();
        state.apply(ExportDialogAction::BeginPreset {operation:ExportDialogPresetOperation::Save}).unwrap();
        assert_eq!(view(&state).page,ExportDialogPage::Presets);assert!(!view(&state).preset_busy);assert!(view(&state).preset_error);
    }
    #[test]
    fn export_dialog_selecting_the_current_output_or_custom_preset_keeps_the_draft_and_preview() {
        let mut state=state(Default::default());
        state.apply(ExportDialogAction::Fit {enabled:true}).unwrap();
        state.apply(ExportDialogAction::Bound {axis:0,value:240}).unwrap();
        let recipe=state.recipe().unwrap();let revision=view(&state).preview_revision;
        state.apply(ExportDialogAction::Preset {index:3}).unwrap();
        state.apply(ExportDialogAction::Output {value:ExportDialogOutput::Sdr}).unwrap();
        assert_eq!(state.recipe().unwrap(),recipe);assert_eq!(view(&state).preview_revision,revision);
    }
}
