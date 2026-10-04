use super::*;
use layer_core::{ArtworkQuery, ArtworkSource, ArtworkStatisticsRequest, color::histogram::Histogram};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HistogramAction {
    Source { index: u8 },
    Channel { index: u8 },
    WaveformChannel { index: u8 },
    WaveformLogarithmic { enabled: bool },
    Logarithmic { enabled: bool },
    Shadows { enabled: bool },
    Highlights { enabled: bool },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct HistogramView {
    pub source: u8,
    pub channel: u8,
    pub logarithmic: bool,
    pub shadows: bool,
    pub highlights: bool,
    pub data: Option<Arc<Histogram>>,
    pub captured_time: Option<f32>,
    pub captured_source: Option<ArtworkSource>,
    pub status: Arc<str>,
    #[serde(skip)]
    status_message: Option<MessageId>,
    #[serde(skip)]
    status_language: Option<UiLanguage>,
    pub description: String,
    pub range: String,
    pub axis: [String; 2],
    pub sources: Vec<Arc<str>>,
    pub channels: Vec<Arc<str>>,
    pub labels: Vec<Arc<str>>,
}

pub(super) struct HistogramCaptionKey {
    language: UiLanguage,
    sample: Option<std::sync::Weak<Histogram>>,
    channel: u8,
    float: bool,
}
impl HistogramCaptionKey {
    fn matches(&self, view: &HistogramView, language: UiLanguage, float: bool) -> bool {
        self.language == language && self.channel == view.channel && self.float == float
            && match (&self.sample, &view.data) {
                (None, None) => true,
                (Some(cached), Some(current)) => std::ptr::eq(cached.as_ptr(), Arc::as_ptr(current)),
                _ => false,
            }
    }
}

impl HistogramView {
    pub fn same_publication(&self, other:&Self) -> bool {
        self.data.as_ref().map(Arc::as_ptr)==other.data.as_ref().map(Arc::as_ptr)
            && (self.source,self.channel,self.logarithmic,self.shadows,self.highlights,self.captured_time)
                ==(other.source,other.channel,other.logarithmic,other.shadows,other.highlights,other.captured_time)
            && (&self.captured_source,&self.status,&self.description,&self.range,&self.axis,&self.sources,&self.channels,&self.labels)
                ==(&other.captured_source,&other.status,&other.description,&other.range,&other.axis,&other.sources,&other.channels,&other.labels)
    }
    pub fn histogram_plot(&self) -> Vec<(usize,Vec<f32>)> {
        let Some(data)=&self.data else {return Vec::new();};
        let channels=self.plotted_channels();let bins=data.plot_bins();
        let scale=|count:u64| if self.logarithmic {(count as f64).ln_1p()} else {count as f64};
        let maximum=scale(channels.iter().flat_map(|&channel|&data.channels[channel].bins[bins.clone()]).copied().max().unwrap_or(1).max(1));
        channels.iter().map(|&channel|(channel,data.channels[channel].bins[bins.clone()].iter().map(|&count|(scale(count)/maximum) as f32).collect())).collect()
    }

    fn set_status(&mut self, message: MessageId, l: &Localizer) {
        if self.status_message == Some(message) && self.status_language == Some(l.language()) { return; }
        self.status_message = Some(message);self.status_language = Some(l.language());self.status = l.text(message);
    }
    fn refresh_copy(&mut self, cached: &mut Option<HistogramCaptionKey>, l: &Localizer, float: bool) {
        if let Some(message) = self.status_message { self.set_status(message, l); }
        if cached.as_ref().is_some_and(|key| key.matches(self, l.language(), float)) { return; }
        let sources = [MessageId::RESOURCES_HISTOGRAM_VISIBLE, MessageId::TOOLBAR_SELECTED_LAYER,
            MessageId::RESOURCES_HISTOGRAM_REFERENCE, MessageId::RESOURCES_HISTOGRAM_SELECTION].map(|id| l.text(id)).to_vec();
        let channels = [MessageId::RESOURCES_PARAMETER_CURVES_CURVE_0, MessageId::RESOURCES_PARAMETER_CURVES_CURVE_1, MessageId::RESOURCES_PARAMETER_CURVES_CURVE_2, MessageId::RESOURCES_PARAMETER_CURVES_CURVE_3].map(|id| l.text(id)).to_vec();
        let labels = [MessageId::NATIVE_COLOR_LOG_COUNTS,
            if float {MessageId::RESOURCES_HISTOGRAM_SHADOWS_SDR} else {MessageId::RESOURCES_HISTOGRAM_SHADOWS},
            if float {MessageId::RESOURCES_HISTOGRAM_HIGHLIGHTS_SDR} else {MessageId::RESOURCES_HISTOGRAM_HIGHLIGHTS}].map(|id| l.text(id)).to_vec();
        let luminance = l.text(MessageId::NATIVE_COLOR_LUMINANCE);
        let description = self.data.as_ref().map(|data| NativeCaption::InspectionPixels {
            sampled: data.pixels, transparent: data.transparent,
        }.message(l)).unwrap_or_default();
        let axis = self.data.as_ref().map_or_else(|| ["0".into(),"1".into()], |data| {
            let bins = data.plot_bins();
            match data.domain {
                layer_core::color::histogram::HistogramDomain::CurveLog {stops} => ["0".into(),format!("{}",stops.exp2())],
                layer_core::color::histogram::HistogramDomain::Artwork if data.color.depth.is_float() =>
                    [format!("{:+.0} EV",data.hdr_bin_stops(bins.start)),format!("{:+.0} EV",data.hdr_bin_stops(bins.end-1))],
                _ => ["0".into(),"1".into()],
            }
        });
        let range = self.data.as_ref().map(|data| {
            let indices: &[usize] = match self.channel {1=>&[0],2=>&[1],3=>&[2],4=>&[3],_=>&[0,1,2]};
            indices.iter().map(|&index| {
                let channel = &data.channels[index];
                let label = if index==3 {luminance.as_ref()} else {channels[index+1].as_ref()};
                format!("{label}: {}", NativeCaption::InspectionChannel {below:channel.below,above:channel.above,black:channel.black,white:channel.white}.message(l))
            }).collect::<Vec<_>>().join("\n")
        }).unwrap_or_default();
        self.range = range;self.axis = axis;
        self.sources = sources;self.channels = channels;
        self.channels.push(luminance);self.labels = labels;
        self.description = description;
        *cached = Some(HistogramCaptionKey { language: l.language(), sample: self.data.as_ref().map(Arc::downgrade), channel: self.channel, float });
    }
    pub(crate) fn clear(&mut self) {self.data=None;self.captured_time=None;self.captured_source=None;self.status=Arc::from("");self.status_message=None;self.status_language=None;}
    pub fn plotted_channels(&self) -> &'static [usize] {
        match self.channel {1=>&[0],2=>&[1],3=>&[2],4=>&[3],_=>&[0,1,2]}
    }
    pub fn waveform_premultiplied_rgba(&self, colors:[[u8;3];4]) -> Option<([u32;2], Vec<u8>)> { self.waveform_pixels(colors, true) }
    pub fn waveform_rgba(&self, colors:[[u8;3];4]) -> Option<([u32;2], Vec<u8>)> { self.waveform_pixels(colors, false) }
    fn waveform_pixels(&self, colors:[[u8;3];4], premultiplied:bool) -> Option<([u32;2], Vec<u8>)> {
        let data=self.data.as_ref()?;let waveform=data.waveform.as_ref()?;
        let bins=data.plot_bins();let height=bins.len();let channels=self.plotted_channels();
        let mut counts=vec![[0u32;3];256*height];
        for (component,&channel) in channels.iter().enumerate() {
            for (i,&count) in waveform.channel(channel).iter().enumerate() {
                let row=height-1-((i/256).clamp(bins.start,bins.end-1)-bins.start);
                counts[row*256+i%256][component]+=count;
            }
        }
        let scale=|count:u32| if self.logarithmic {f64::from(count).ln_1p()} else {f64::from(count)};
        let maximum=scale(counts.iter().flatten().copied().max().unwrap_or(1).max(1));
        let rgba=counts.into_iter().flat_map(|counts| {
            let mut rgb=[0.;3];let mut alpha:f64=0.;
            for (component,&channel) in channels.iter().enumerate() {
                let density=scale(counts[component])/maximum;alpha=alpha.max(density);
                for c in 0..3 {rgb[c]+=f64::from(colors[channel][c])*density;}
            }
            if !premultiplied && alpha>0. {for c in &mut rgb {*c/=alpha;}}
            let ceiling=if premultiplied {255.*alpha} else {255.};
            [rgb[0].min(ceiling).round() as u8,rgb[1].min(ceiling).round() as u8,rgb[2].min(ceiling).round() as u8,(alpha*255.).round() as u8]
        }).collect();
        Some(([256,height as u32],rgba))
    }
}

#[derive(Default)]
pub(super) struct Statistics {
    query: Option<ArtworkQuery>,
    observed: Option<ArtworkQuery>,
    active: bool,
    preview: bool,
    preview_ready: bool,
    changed: u64,
    started: u64,
    epoch: u64,
    pub demand: bool,
    pub settled: bool,
    waveform: bool,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn panel_is_presented(&self, panel: Panel) -> bool {
        if self.rendering_suspended { return false; }
        let layout = &self.state.workspace.layout;
        self.state.customization.expanded == Some(panel)
            || (layout.active_panel(panel) == Some(panel) && layout.panel_group(panel).is_none_or(|group|
                layout.collapsed_column_for_group(group).is_none_or(|column| layout.column_stack(column).open_column == Some(column))))
            || self.state.customization.drawer.iter().chain(self.state.customization.column_drawers.iter())
                .any(|drawer| drawer.columns.iter().any(|column| column.contains(&panel)))
    }
    pub(super) fn histogram_visibility_changed(&self)->bool {
        if !Panel::Histogram.available_on(self.state.platform) {return false;}
        let tonal=self.engine.document().layer(self.engine.document().active_layer).and_then(|layer|layer.effect.as_ref())
            .is_some_and(|effect|matches!(effect.program.id.as_ref(),"curves"|"levels"));
        let waveform=self.panel_is_presented(Panel::Waveform);
        self.histogram.demand!=(self.panel_is_presented(Panel::Histogram) || waveform) || self.histogram.waveform!=waveform
            || self.tonal_histogram.demand!=(tonal && self.panel_is_presented(Panel::Properties))
    }
    pub(super) fn cancel_histogram(&mut self) {
        if self.histogram.active || self.tonal_histogram.active { self.engine.backend_mut().cancel_snapshot(); }
        self.histogram = Statistics::default();self.tonal_histogram = Statistics::default();
        self.state.histogram.clear();self.state.waveform.clear();self.state.tonal_histogram.clear();
        self.histogram_copy();
    }
    pub(super) fn histogram_action(&mut self, action: HistogramAction) -> Result<(), String> {
        match action {
            HistogramAction::Source { index } if index < 4 => {
                self.cancel_histogram();self.state.histogram.source = index;self.state.histogram.clear();
            }
            HistogramAction::Channel { index } if index < 5 => self.state.histogram.channel = index,
            HistogramAction::WaveformChannel { index } if index < 5 => self.state.waveform.channel = index,
            HistogramAction::WaveformLogarithmic { enabled } => self.state.waveform.logarithmic = enabled,
            HistogramAction::Logarithmic { enabled } => self.state.histogram.logarithmic = enabled,
            HistogramAction::Shadows { enabled } => self.state.histogram.shadows = enabled,
            HistogramAction::Highlights { enabled } => self.state.histogram.highlights = enabled,
            _ => return Err("Invalid histogram control".into()),
        }
        self.engine.backend_mut().set_clipping_preview(self.state.histogram.shadows, self.state.histogram.highlights);
        self.histogram_copy();
        Ok(())
    }
    pub(super) fn histogram_copy(&mut self) {
        let l = &self.state.localization;
        let float = self.engine.document().color.depth.is_float();
        self.state.histogram.refresh_copy(&mut self.histogram_captions[0],l,float);
        self.state.tonal_histogram.refresh_copy(&mut self.histogram_captions[1],l,float);
        let waveform=&mut self.state.waveform;let histogram=&self.state.histogram;
        waveform.data=histogram.data.clone();waveform.source=histogram.source;
        waveform.status=histogram.status.clone();waveform.status_message=histogram.status_message;waveform.status_language=histogram.status_language;
        waveform.captured_time=histogram.captured_time;waveform.captured_source=histogram.captured_source.clone();
        waveform.shadows=histogram.shadows;waveform.highlights=histogram.highlights;
        if !self.histogram.waveform {waveform.clear();}
        waveform.refresh_copy(&mut self.histogram_captions[2],l,float);
    }
    pub(super) fn poll_histogram(&mut self, now:u64) -> u32 {
        let properties = &self.state.layer_properties;
        let tonal = properties.layer.and_then(|id| self.engine.document().layer(LayerId(id))).filter(|layer|
            layer.effect.as_ref().is_some_and(|effect| matches!(effect.program.id.as_ref(),"curves"|"levels")));
        let channel = match properties.page.as_deref() {Some("red")=>1,Some("green")=>2,Some("blue")=>3,_=>0};
        let source = tonal.map(|layer| if channel==0 {ArtworkSource::EffectChannels(layer.id)} else {ArtworkSource::EffectInput(layer.id)});
        let demand = Panel::Histogram.available_on(self.state.platform) && source.is_some() && self.panel_is_presented(Panel::Properties);
        let mut task=std::mem::take(&mut self.tonal_histogram);
        let mut view=std::mem::take(&mut self.state.tonal_histogram);
        if view.channel!=channel { if task.active {self.engine.backend_mut().cancel_snapshot();}task=Statistics::default();view.clear(); }
        view.channel=channel;
        if demand && !task.settled && self.histogram.active {
            self.engine.backend_mut().cancel_snapshot();self.histogram.active=false;self.histogram.query=None;
        }
        let mut updates=self.poll_statistics(now,source.unwrap_or(ArtworkSource::Visible),false,demand,!self.histogram.active,(false,&mut task,&mut view));
        self.tonal_histogram=task;self.state.tonal_histogram=view;
        let mut task=std::mem::take(&mut self.histogram);
        let mut view=std::mem::take(&mut self.state.histogram);
        let source = match view.source {1=>ArtworkSource::LayerContent(self.engine.document().active_layer),2=>ArtworkSource::Reference,_=>ArtworkSource::Visible};
        let waveform=Panel::Histogram.available_on(self.state.platform) && self.panel_is_presented(Panel::Waveform);
        let demand=Panel::Histogram.available_on(self.state.platform) && (self.panel_is_presented(Panel::Histogram) || waveform);
        let admitted=!self.tonal_histogram.active && (!self.tonal_histogram.demand || self.tonal_histogram.settled);
        updates|=self.poll_statistics(now,source,view.source==3,demand,admitted,(waveform,&mut task,&mut view));
        self.histogram=task;self.state.histogram=view;
        if updates!=0 || self.state.histogram.sources.is_empty() {self.histogram_copy();}
        updates
    }
    fn poll_statistics(&mut self, now:u64, source:ArtworkSource, selection:bool, demand:bool, admitted:bool,
        state:(bool, &mut Statistics, &mut HistogramView)) -> u32 {
        let (waveform, task, view) = state;
        if !demand || self.targeted_curve.is_some() || self.auto_levels.is_some() || self.eyedropper.calibration.is_some() || self.content_bounds.busy() || self.state.host_error.is_some() {
            if task.demand || task.active {
                if task.active {self.engine.backend_mut().cancel_snapshot();}
                *task = Statistics::default();view.clear();return regions::HISTOGRAM;
            }
            return 0;
        }
        task.demand = true;
        let document = self.engine.document();
        let matches = |query:&ArtworkQuery, document:&Document| query.source==source && query.matches_source(document)
            && (!selection || query.document.selection==document.selection);
        let identity_changed = task.waveform!=waveform || task.epoch != self.state.document_file.epoch
            || task.observed.as_ref().is_some_and(|query| query.source != source || !query.matches_source_identity(document)
                || (selection && query.document.selection != document.selection)
                || match source {
                    ArtworkSource::EffectInput(id) | ArtworkSource::EffectChannels(id) => {
                        let domain = |doc:&Document| doc.layer(id).and_then(|layer|layer.effect.as_ref()).map(|effect|
                            (effect.value("domain").cloned(),effect.value("hdr_stops").cloned()));
                        domain(&query.document)!=domain(document)
                    },
                    _ => false,
                });
        let animated = document.has_animated_effects();
        let changed = task.observed.as_ref().is_none_or(|query| !matches(query,self.engine.document())
            || (animated && query.time != self.engine.animation_time()));
        if identity_changed {
            if task.active {self.engine.backend_mut().cancel_snapshot();}
            *task = Statistics::default();task.demand = true;task.waveform=waveform;view.clear();
        }
        let mut updates = 0;
        if changed || identity_changed {
            let mut observed = ArtworkQuery::new(self.engine.document(), source.clone());observed.time = self.engine.animation_time();
            task.observed = Some(observed);
            task.epoch = self.state.document_file.epoch;task.changed = now;
            task.preview_ready = false;task.settled = false;
            view.set_status(MessageId::RESOURCES_HISTOGRAM_UPDATING,self.localization());
            updates = regions::HISTOGRAM;
            if task.active && !task.preview {
                self.engine.backend_mut().cancel_snapshot();task.active = false;task.query = None;
            }
        }
        if task.active && let Some(result) = self.engine.backend_mut().take_snapshot() {
            task.active = false;
            let current = task.query.as_ref().is_some_and(|query| matches(query,self.engine.document()) && (!animated || query.time == self.engine.animation_time()));
            match result {
                Ok(layer_render::SnapshotResult::ArtworkStatistics(data)) => {
                    let empty = selection && data.pixels+data.transparent==0 && !task.preview;
                    view.data = (!empty).then(|| Arc::new(data));
                    view.captured_time = task.query.as_ref().map(|query|query.time);
                    view.captured_source = task.query.as_ref().map(|query|query.source.clone());
                    task.settled = current && !task.preview;
                    task.preview_ready = current;
                    view.set_status(if empty {MessageId::RESOURCES_HISTOGRAM_UNAVAILABLE} else if !current { MessageId::RESOURCES_HISTOGRAM_UPDATING }
                        else if task.preview { MessageId::RESOURCES_HISTOGRAM_PREVIEW } else { MessageId::RESOURCES_HISTOGRAM_EXACT },self.localization());
                }
                _ => {
                    task.settled = current;
                    view.set_status(MessageId::RESOURCES_HISTOGRAM_ERROR,self.localization());
                }
            }
            task.query = None;updates = regions::HISTOGRAM;
        }
        if admitted && !task.active && !task.settled && now.saturating_sub(task.started) >= 100_000_000
            && (!task.preview_ready || now.saturating_sub(task.changed) >= 200_000_000)
            && !self.engine.has_pending_document_edits() {
            let mut query = task.observed.as_ref().unwrap().clone();query.time = self.engine.animation_time();
            task.preview = !task.preview_ready;
            let request = ArtworkStatisticsRequest { waveform, query: query.clone(), preview: task.preview, selection };
            if query.validate().is_err() || (request.selection && query.document.selection.is_none()) {
                task.settled = true;view.set_status(MessageId::RESOURCES_HISTOGRAM_UNAVAILABLE,self.localization());
                updates = regions::HISTOGRAM;
            } else { match self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::ArtworkStatistics(request)) {
                Ok(true) => {task.active = true;task.query = Some(query);task.started = now;}
                Ok(false) => (),
                Err(_) => {task.settled = true;view.set_status(MessageId::RESOURCES_HISTOGRAM_ERROR,self.localization());updates = regions::HISTOGRAM;}
            }}
        }
        if view.data.is_none() {view.captured_time=None;view.captured_source=None;}
        updates
    }
}
