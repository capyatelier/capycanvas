//! Tonal selection drafts: immutable baseline, pinned source, and one history edit.
//! Hosts present shared fields and forward contacts; sampling never clips location.
use super::*;
use layer_core::{
    Affine, Edit, Point, Selection,
    tonal::{MAX_BANDS, MAX_STOP, MIN_STOP, TonalBand},
};
use layer_render::{RegionRequest, SelectionRefinement, TonalProbe, TonalRequest, TonalSample};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TonalAction {
    ToggleBand { index: usize },
    ActiveBand { index: usize },
    LoadBand { index: usize },
    Source { layer: Option<u64> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TonalOptions {
    pub bands: Vec<TonalBand>,
    pub enabled: Vec<bool>,
    pub active: usize,
    pub linked: bool,
    pub invert: bool,
}
impl Default for TonalOptions {
    fn default() -> Self {
        Self {
            bands: TonalBand::defaults(),
            enabled: vec![false, false, false, false, true, false, false],
            active: 4,
            linked: true,
            invert: false,
        }
    }
}
impl TonalOptions {
    pub fn validate(&self) -> Result<(), String> {
        if self.bands.len() < 7
            || self.bands.len() > MAX_BANDS
            || self.enabled.len() != self.bands.len()
            || self.active >= self.bands.len()
        {
            return Err("Invalid tonal band list".into());
        }
        if self.bands[..7] != TonalBand::defaults() {
            return Err("Built-in tonal bands cannot be changed".into());
        }
        for band in &self.bands {
            band.validate().map_err(str::to_string)?;
        }
        Ok(())
    }
    fn add(&mut self, band: TonalBand) -> Result<(), String> {
        if self.bands.len() == MAX_BANDS {
            return Err("Remove a custom band before adding another (16 bands maximum)".into());
        }
        band.validate().map_err(str::to_string)?;
        self.active = self.bands.len();
        self.bands.push(band);
        self.enabled.push(true);
        Ok(())
    }
    fn editable(&mut self) -> Result<&mut TonalBand, String> {
        if self.active < 7 {
            let mut band = self.bands[self.active].clone();
            band.name = format!("{} copy", band.name);
            let old = self.active;
            self.add(band)?;
            self.enabled[old] = false;
        }
        Ok(&mut self.bands[self.active])
    }
    pub fn controls(&self) -> Vec<ToolSetting> {
        let b = &self.bands[self.active];
        let stops = NumericControl {
            soft_min: -12.,
            soft_max: 6.,
            ..NumericControl::number(MIN_STOP as f64, MAX_STOP as f64, 0.1, 2).unit("stops")
        };
        let falloff = NumericControl::number(0., 16., 0.1, 2).unit("stops");
        let mut fields = Vec::new();
        for (id, label, value, numeric) in [
            ("tonal_lower", "Lower bound", b.lower, stops.clone()),
            ("tonal_upper", "Upper bound", b.upper, stops),
            (
                "tonal_falloff_low",
                if self.linked {
                    "Tonal falloff"
                } else {
                    "Darker falloff"
                },
                Some(b.falloff[0]),
                falloff.clone(),
            ),
            (
                "tonal_falloff_high",
                "Brighter falloff",
                (!self.linked).then_some(b.falloff[1]),
                falloff,
            ),
        ] {
            if let Some(value) = value {
                fields.push(ToolSetting {
                    id,
                    label,
                    group: "Active band",
                    value,
                    numeric,
                });
            }
        }
        fields
    }
    pub fn reset_value(&self, id: &str) -> Result<f32, String> {
        let b = &self.bands[self.active];
        match id {
            "tonal_lower" => Ok((b.upper.unwrap_or(0.) - 1.).max(MIN_STOP)),
            "tonal_upper" => Ok((b.lower.unwrap_or(-1.) + 1.).min(MAX_STOP)),
            "tonal_falloff_low" | "tonal_falloff_high" => Ok(0.5),
            _ => Err("Unknown tonal setting".into()),
        }
    }
    pub fn edit(&mut self, id: &str, value: f32) -> Result<(), String> {
        let control = self
            .controls()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("Unknown tonal setting")?;
        control.numeric.validate(value, control.label)?;
        let mut next = self.clone();
        let linked = next.linked;
        let b = next.editable()?;
        match id {
            "tonal_lower" => b.lower = Some(value),
            "tonal_upper" => b.upper = Some(value),
            "tonal_falloff_low" => {
                b.falloff[0] = value;
                if linked {
                    b.falloff[1] = value;
                }
            }
            "tonal_falloff_high" => b.falloff[1] = value,
            _ => unreachable!(),
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn rename(&mut self, name: String) -> Result<(), String> {
        let mut next = self.clone();
        next.editable()?.name = name.trim().into();
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn sample(&mut self, sample: TonalSample, point: bool) -> Result<(), String> {
        let factory = self.active < 7;
        let b = self.editable()?;
        let width = if factory {
            1.
        } else {
            b.lower.zip(b.upper).map_or(1., |(a, b)| (b - a).max(0.1))
        };
        let [lo, hi] = if point {
            [sample.stops[0] - width / 2., sample.stops[0] + width / 2.]
        } else {
            sample.stops
        };
        b.lower = (lo > MIN_STOP).then_some(lo.min(MAX_STOP));
        b.upper = (hi < MAX_STOP).then_some(hi.max(MIN_STOP));
        self.enabled[self.active] = true;
        Ok(())
    }
}
#[derive(Default)]
pub(super) struct TonalTools {
    pub draft: Option<TonalDraft>,
    pub preview: Option<Selection>,
    pub ready: bool,
    pub source: Option<LayerId>,
    pub probe: Option<TonalProbe>,
    pub hover: Option<f32>,
    pub changed: bool,
    sample_area: Option<layer_render::ColorSampleArea>,
}
pub(super) struct TonalDraft {
    pub revision: u64,
    pub baseline: Option<Selection>,
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn tonal_active(&self) -> bool {
        self.layer_interaction.tool.selection_tool() == Some(SelectionTool::Tonal)
    }
    pub(super) fn cancel_tonal(&mut self) -> bool {
        let active = self.tonal_tools.draft.take().is_some();
        self.tonal_tools.preview = None;
        if active {
            self.engine.set_selection_display(None);
        }
        self.tonal_tools.ready = false;
        self.tonal_tools.probe = None;
        self.tonal_tools.hover = None;
        if let Some(area) = self.tonal_tools.sample_area.take() {
            self.eyedropper.cancel();
            self.eyedropper.area = area;
        }
        if active {
            self.region_tools.cancel();
            self.tonal_tools.changed = true;
        }
        active
    }
    fn tonal_source(&self) -> Result<(layer_render::RegionSource, Affine, [u32; 2]), String> {
        let doc = self.engine.document();
        if let Some(id) = self.tonal_tools.source {
            let layer = doc.layer(id).ok_or("The sampled layer no longer exists")?;
            if matches!(
                layer.kind,
                layer_core::LayerKind::Selection
                    | layer_core::LayerKind::Group
                    | layer_core::LayerKind::Effect
            ) {
                return Err("Choose an artwork layer to sample".into());
            }
            Ok((
                layer_render::RegionSource::Layer(id),
                doc.layer_transform(id),
                doc.target_extent(id),
            ))
        } else {
            Ok((
                layer_render::RegionSource::Composite,
                Affine::IDENTITY,
                [doc.width, doc.height],
            ))
        }
    }
    pub(super) fn queue_tonal(&mut self, probe: Option<TonalProbe>) -> Result<(), String> {
        if !self.tonal_active() {
            return Ok(());
        }
        let (source, basis, _) = self.tonal_source()?;
        let doc = self.engine.document();
        if self
            .tonal_tools
            .draft
            .as_ref()
            .is_some_and(|d| d.revision != doc.revision)
        {
            self.cancel_tonal();
        }
        let doc = self.engine.document();
        let draft = self.tonal_tools.draft.get_or_insert_with(|| TonalDraft {
            revision: doc.revision,
            baseline: doc.selection.clone(),
        });
        let options = &self.selection_tools.options;
        let request = RegionRequest {
            request_id: 0,
            source: layer_render::RegionSource::Tonal(Box::new(TonalRequest {
                source,
                bands: options
                    .tonal
                    .bands
                    .iter()
                    .zip(&options.tonal.enabled)
                    .filter(|(_, on)| **on)
                    .map(|(b, _)| b.clone())
                    .collect(),
                invert: options.tonal.invert,
                probe,
            })),
            contiguous: false,
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
            selection: Some(SelectionRefinement {
                resize: 0,
                mode: options.mode,
                antialias: true,
                feather: options.feather,
                previous: draft.baseline.clone().map(Arc::new),
                source_to_document: basis,
            }),
        };
        self.tonal_tools.ready = false;
        self.tonal_tools.probe = probe;
        self.tonal_tools.changed = true;
        self.queue_tonal_region(request);
        Ok(())
    }
    pub(super) fn tonal_result(
        &mut self,
        result: layer_render::RegionResult,
    ) -> Result<(), String> {
        if !self.tonal_active() || self.tonal_tools.draft.is_none() {
            return Ok(());
        }
        if let Some(probe) = self.tonal_tools.probe.take() {
            if let Some(sample) = result.tonal_sample {
                self.selection_tools
                    .options
                    .tonal
                    .sample(sample, probe.point)?;
                self.queue_tonal(None)?;
            } else {
                self.state.host_error = Some("No visible pixels in the sampled region".into());
                self.queue_tonal(None)?;
            }
        } else {
            self.tonal_tools.preview = Some(Selection::pixels(result.pixels));
            self.tonal_tools.ready = true;
            // Query completion can arrive after this frame was submitted. Publish
            // the display mask now; waiting for another document frame leaves a
            // ready preview showing its predecessor on an otherwise idle canvas.
            self.sync_selection_overlay();
            self.engine
                .backend_mut()
                .set_selection_outline(self.tonal_tools.preview.as_ref())
                .map_err(error)?;
        }
        self.tonal_tools.changed = true;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn finish_tonal(&mut self, apply: bool) -> Result<(), String> {
        if apply
            && (!self.tonal_tools.ready
                || self
                    .tonal_tools
                    .draft
                    .as_ref()
                    .is_none_or(|d| d.revision != self.engine.document().revision))
        {
            return Err("Wait for the current tonal preview".into());
        }
        let selection = self.tonal_tools.preview.clone();
        self.cancel_tonal();
        self.layer_interaction.path.clear();
        if apply {
            self.layer_edit(Edit::SetSelection(selection))?;
        }
        self.sync_selection_overlay();
        let display = self.engine.display_selection().map(|s| s.into_owned());
        self.engine
            .backend_mut()
            .set_selection_outline(display.as_ref())
            .map_err(error)?;
        self.layer_interaction.changed = true;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn tonal_action(&mut self, action: TonalAction) -> Result<(), String> {
        if !self.tonal_active() {
            return Err("Choose Tonal range first".into());
        }
        let options = &mut self.selection_tools.options.tonal;
        let generate = match action {
            TonalAction::ToggleBand { index } => {
                let enabled = options.enabled.get_mut(index).ok_or("Unknown band")?;
                *enabled = !*enabled;
                if *enabled {
                    options.active = index;
                }
                true
            }
            TonalAction::ActiveBand { index } => {
                if index >= options.bands.len() {
                    return Err("Unknown band".into());
                }
                options.active = index;
                false
            }
            TonalAction::LoadBand { index } => {
                options.add(
                    self.state
                        .settings
                        .tonal_bands
                        .get(index)
                        .ok_or("Unknown saved band")?
                        .clone(),
                )?;
                true
            }
            TonalAction::Source { layer } => {
                let previous = self.tonal_tools.source;
                self.tonal_tools.source = layer.map(LayerId);
                if let Err(e) = self.tonal_source() {
                    self.tonal_tools.source = previous;
                    return Err(e);
                }
                self.eyedropper.cancel();
                true
            }
        };
        if generate {
            self.queue_tonal(None)?;
        }
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn tonal_command(&mut self, command: CommandId) -> Result<(), String> {
        if !self.tonal_active() {
            return Err("Choose Tonal range first".into());
        }
        use CommandId::*;
        if matches!(command, ApplyTonalSelection | CancelTonalSelection) {
            return self.finish_tonal(command == ApplyTonalSelection);
        }
        let mut options = self.selection_tools.options.tonal.clone();
        match command {
            TonalNewBand => options.add(TonalBand {
                name: "Custom band".into(),
                lower: Some(-3.),
                upper: Some(-2.),
                falloff: [0.5; 2],
            })?,
            TonalRemoveBand => {
                if options.active < 7 {
                    return Err("Built-in presets cannot be removed".into());
                }
                options.bands.remove(options.active);
                options.enabled.remove(options.active);
                options.active = 4;
            }
            TonalSaveBand => {
                let band = options.bands[options.active].clone();
                band.validate().map_err(str::to_string)?;
                let saved = &mut self.state.settings.tonal_bands;
                if let Some(existing) = saved.iter_mut().find(|b| b.name == band.name) {
                    *existing = band;
                } else if saved.len() < 64 {
                    saved.push(band);
                } else {
                    return Err("At most 64 saved tonal bands are supported".into());
                }
                self.request(HostRequestKind::SaveSettings {
                    settings: Box::new(self.state.settings.clone()),
                })?;
                self.refresh_tools();
                return Ok(());
            }
            TonalInvert => options.invert = !options.invert,
            TonalLinkFalloff => {
                options.linked = !options.linked;
                if options.linked {
                    let b = options.editable()?;
                    b.falloff[1] = b.falloff[0];
                }
            }
            TonalLowerOpen => {
                let b = options.editable()?;
                b.lower = if b.lower.is_some() {
                    None
                } else {
                    Some((b.upper.unwrap_or(0.) - 1.).max(MIN_STOP))
                };
            }
            TonalUpperOpen => {
                let b = options.editable()?;
                b.upper = if b.upper.is_some() {
                    None
                } else {
                    Some((b.lower.unwrap_or(-1.) + 1.).min(MAX_STOP))
                };
            }
            _ => return Err("Unknown tonal command".into()),
        }
        options.validate()?;
        self.selection_tools.options.tonal = options;
        self.queue_tonal(None)?;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn tonal_hover(&mut self, point: Option<Point>) {
        if !self.tonal_active() {
            return;
        }
        let Some(point) = point else {
            self.eyedropper.cancel();
            self.tonal_tools.hover = None;
            self.tonal_tools.changed = true;
            return;
        };
        let Ok((source, basis, extent)) = self.tonal_source() else {
            return;
        };
        let Some(inverse) = basis.inverse() else {
            return;
        };
        let p = inverse.map(point);
        if p.x < 0. || p.y < 0. || p.x >= extent[0] as f32 || p.y >= extent[1] as f32 {
            self.eyedropper.cancel();
            self.tonal_tools.hover = None;
            return;
        }
        self.tonal_tools
            .sample_area
            .get_or_insert(self.eyedropper.area);
        self.eyedropper.area = layer_render::ColorSampleArea::Average5;
        self.eyedropper.queue(
            match source {
                layer_render::RegionSource::Layer(id) => layer_render::ColorSampleSource::Layer(id),
                _ => layer_render::ColorSampleSource::Composite,
            },
            [p.x as u32, p.y as u32],
        );
    }
    pub(super) fn tonal_sampled_color(&mut self, color: layer_core::color::RgbColor) {
        if let Ok(rgb) = color.linear_in(self.engine.document().color.space) {
            let w = self.engine.document().color.space.to_xyz()[1];
            let y = (0..3).map(|i| w[i] * f64::from(rgb[i])).sum::<f64>();
            self.tonal_tools.hover = Some(if y > 0. { y.log2() as f32 } else { MIN_STOP });
            self.tonal_tools.changed = true;
        }
    }
    pub(super) fn tonal_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        match event.phase {
            PenPhase::Down => {
                self.layer_interaction.path = vec![p, p];
            }
            PenPhase::Move => {
                if self.layer_interaction.path.len() == 2 {
                    self.layer_interaction.path[1] = p;
                }
            }
            PenPhase::Cancel => {
                self.layer_interaction.path.clear();
            }
            PenPhase::Hover => self.tonal_hover(Some(p)),
            PenPhase::Up => {
                let points = std::mem::take(&mut self.layer_interaction.path);
                let Some(start) = points.first() else {
                    return Ok(());
                };
                let (_, basis, extent) = self.tonal_source()?;
                let inverse = basis.inverse().ok_or("Invalid sampled layer placement")?;
                let point = (p.x - start.x).hypot(p.y - start.y) * self.state.camera.zoom < 4.;
                let bounds = if point {
                    let q = inverse.map(p);
                    if q.x < 0. || q.y < 0. || q.x >= extent[0] as f32 || q.y >= extent[1] as f32 {
                        return Ok(());
                    }
                    let [x, y] = [q.x as u32, q.y as u32];
                    [
                        x.saturating_sub(2),
                        y.saturating_sub(2),
                        (x + 3).min(extent[0]),
                        (y + 3).min(extent[1]),
                    ]
                } else {
                    let q = [
                        inverse.map(*start),
                        inverse.map(p),
                        inverse.map(Point { x: start.x, y: p.y }),
                        inverse.map(Point { x: p.x, y: start.y }),
                    ];
                    let x = q
                        .iter()
                        .map(|p| p.x)
                        .fold(f32::INFINITY, f32::min)
                        .floor()
                        .max(0.) as u32;
                    let y = q
                        .iter()
                        .map(|p| p.y)
                        .fold(f32::INFINITY, f32::min)
                        .floor()
                        .max(0.) as u32;
                    let r = (q
                        .iter()
                        .map(|p| p.x)
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil()
                        .max(0.) as u32)
                        .min(extent[0]);
                    let b = (q
                        .iter()
                        .map(|p| p.y)
                        .fold(f32::NEG_INFINITY, f32::max)
                        .ceil()
                        .max(0.) as u32)
                        .min(extent[1]);
                    [x, y, r, b]
                };
                if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                    self.queue_tonal(Some(TonalProbe { bounds, point }))?;
                }
            }
        }
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn tonal_extra(&self) -> Vec<ToolOption> {
        if !self.tonal_active() {
            return Vec::new();
        }
        let options = &self.selection_tools.options.tonal;
        let list = |id, label, multiple, items| ToolOption::List {
            id,
            label,
            multiple,
            items,
        };
        let mut sources = vec![ToolListItem {
            label: "Visible composite".into(),
            action: UiAction::Tonal {
                action: TonalAction::Source { layer: None },
            },
            selected: self.tonal_tools.source.is_none(),
        }];
        sources.extend(
            self.engine
                .document()
                .layers
                .iter()
                .filter(|l| {
                    !matches!(
                        l.kind,
                        layer_core::LayerKind::Group
                            | layer_core::LayerKind::Selection
                            | layer_core::LayerKind::Effect
                    )
                })
                .map(|l| ToolListItem {
                    label: format!("Layer: {}", l.name),
                    action: UiAction::Tonal {
                        action: TonalAction::Source {
                            layer: Some(l.id.0),
                        },
                    },
                    selected: self.tonal_tools.source == Some(l.id),
                }),
        );
        let mut result=vec![
            ToolOption::Info {id:"tonal-help",text:"Click to sample a tone; drag to sample a range. Sampling selects matching tones across the image. 0 stops = reference white (203 nits); HDR values remain unclipped.".into()},
            list("tonal-source","Sample from",false,sources),
            list("tonal-bands","Include tones",true,options.bands.iter().enumerate().map(|(index,b)|ToolListItem {label:b.name.clone(),selected:options.enabled[index],action:UiAction::Tonal {action:TonalAction::ToggleBand {index}}}).collect()),
            list("tonal-active","Edit band",false,options.bands.iter().enumerate().map(|(index,b)|ToolListItem {label:b.name.clone(),selected:options.active==index,action:UiAction::Tonal {action:TonalAction::ActiveBand {index}}}).collect()),
            ToolOption::Text {id:"tonal-name",label:"Band name",value:options.bands[options.active].name.clone()},
        ];
        if !self.state.settings.tonal_bands.is_empty() {
            result.push(list(
                "tonal-saved",
                "Add saved band",
                false,
                self.state
                    .settings
                    .tonal_bands
                    .iter()
                    .enumerate()
                    .map(|(index, b)| ToolListItem {
                        label: b.name.clone(),
                        selected: false,
                        action: UiAction::Tonal {
                            action: TonalAction::LoadBand { index },
                        },
                    })
                    .collect(),
            ));
        }
        let status = if self.tonal_tools.draft.is_some() && !self.tonal_tools.ready {
            "Updating preview…"
        } else if self.tonal_tools.ready {
            "Preview ready — Apply selection to keep it"
        } else {
            "Adjust a band or sample the canvas to preview"
        };
        result.push(ToolOption::Info {
            id: "tonal-status",
            text: if let Some(v) = self.tonal_tools.hover {
                format!("Sample: {v:+.2} stops · {status}")
            } else {
                status.into()
            },
        });
        result
    }
}
