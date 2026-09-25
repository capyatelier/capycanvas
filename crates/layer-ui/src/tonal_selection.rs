//! Direct tonal selection: visible artwork in, current selection or mask out.
use super::*;
use layer_core::{
    Affine, Point, Selection, SelectionTarget,
    tonal::{MAX_STOP, MIN_STOP, TonalBand},
};
use layer_render::{RegionRequest, SelectionRefinement, TonalProbe, TonalRequest, TonalSample};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TonalAction {
    Preset { index: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredTonalOptions")]
pub struct TonalOptions {
    pub tone: usize,
    pub custom: [f32; 2],
    pub softness: f32,
}
impl Default for TonalOptions {
    fn default() -> Self {
        Self {
            tone: 4,
            custom: [-3.5, -1.5],
            softness: 1.,
        }
    }
}
// Read workspaces from the original band editor without retaining its editing
// machinery. A legacy multi-band recipe migrates to its active included band.
#[derive(Default, Deserialize)]
#[serde(default)]
struct StoredTonalOptions {
    tone: Option<usize>,
    custom: Option<[f32; 2]>,
    softness: Option<f32>,
    bands: Vec<TonalBand>,
    enabled: Vec<bool>,
    active: usize,
}
impl From<StoredTonalOptions> for TonalOptions {
    fn from(old: StoredTonalOptions) -> Self {
        let mut options = Self::default();
        options.softness = old.softness.unwrap_or(1.);
        if let Some(tone) = old.tone {
            options.tone = tone;
            options.custom = old.custom.unwrap_or(options.custom);
        } else if !old.bands.is_empty() {
            let index = if old.enabled.get(old.active) == Some(&true) {
                old.active
            } else {
                old.enabled.iter().position(|on| *on).unwrap_or(4)
            };
            if let Some(band) = old.bands.get(index) {
                options.tone = index.min(7);
                if index >= 7 {
                    options.custom = [
                        band.lower.unwrap_or(MIN_STOP),
                        band.upper.unwrap_or(MAX_STOP),
                    ];
                }
            }
        }
        options
    }
}
impl TonalOptions {
    pub const NAMES: [&'static str; 8] = [
        "Shadows",
        "Mid-shadows",
        "Midtones",
        "Mid-highlights",
        "Highlights",
        "Deep shadows",
        "Bright HDR",
        "Custom",
    ];
    pub fn validate(&self) -> Result<(), String> {
        if self.tone >= Self::NAMES.len() {
            return Err("Unknown tone".into());
        }
        self.softness_control()
            .numeric
            .validate(self.softness, "Softness")?;
        if self
            .custom
            .iter()
            .any(|v| !v.is_finite() || !(MIN_STOP..=MAX_STOP).contains(v))
            || self.custom[0] > self.custom[1]
        {
            return Err("Invalid tonal range".into());
        }
        Ok(())
    }
    fn band(&self) -> TonalBand {
        let mut band = if self.tone < 7 {
            TonalBand::defaults().remove(self.tone)
        } else {
            TonalBand {
                name: "Custom".into(),
                lower: (self.custom[0] > MIN_STOP).then_some(self.custom[0]),
                upper: (self.custom[1] < MAX_STOP).then_some(self.custom[1]),
                falloff: [0.5; 2],
            }
        };
        band.falloff = [self.softness * 0.5; 2];
        band
    }
    pub fn softness_control(&self) -> ToolSetting {
        ToolSetting {
            id: "tonal_softness",
            label: "Softness",
            group: "",
            value: self.softness,
            numeric: NumericControl {
                max: 2.,
                soft_max: 2.,
                digits: 0,
                resolution: 0.01,
                ..NumericControl::percent()
            },
        }
    }
    pub fn controls(&self) -> Vec<ToolSetting> {
        let mut controls = Vec::new();
        if self.tone == 7 {
            let numeric = NumericControl {
                soft_min: -12.,
                soft_max: 6.,
                ..NumericControl::number(MIN_STOP as f64, MAX_STOP as f64, 0.1, 2).unit("stops")
            };
            for (id, label, value) in [
                ("tonal_lower", "From", self.custom[0]),
                ("tonal_upper", "To", self.custom[1]),
            ] {
                controls.push(ToolSetting {
                    id,
                    label,
                    value,
                    group: "",
                    numeric: numeric.clone(),
                });
            }
        }
        controls.push(self.softness_control());
        controls
    }
    pub fn reset_value(&self, id: &str) -> Result<f32, String> {
        match id {
            "tonal_softness" => Ok(1.),
            "tonal_lower" => Ok((-3.5f32).min(self.custom[1])),
            "tonal_upper" => Ok((-1.5f32).max(self.custom[0])),
            _ => Err("Unknown tonal setting".into()),
        }
    }
    pub fn edit(&mut self, id: &str, value: f32) -> Result<(), String> {
        let mut next = self.clone();
        match id {
            "tonal_softness" => next.softness = value,
            "tonal_lower" if self.tone == 7 => {
                next.custom[0] = value;
                next.custom[1] = next.custom[1].max(value);
            }
            "tonal_upper" if self.tone == 7 => {
                next.custom[1] = value;
                next.custom[0] = next.custom[0].min(value);
            }
            _ => return Err("Unknown tonal setting".into()),
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn sample(&mut self, sample: TonalSample, point: bool) {
        let width = if self.tone == 7 {
            (self.custom[1] - self.custom[0]).max(0.1)
        } else {
            1.
        };
        self.custom = if point {
            [sample.stops[0] - width / 2., sample.stops[0] + width / 2.]
        } else {
            sample.stops
        };
        self.custom = self.custom.map(|v| v.clamp(MIN_STOP, MAX_STOP));
        self.tone = 7;
    }
}
#[derive(Default)]
pub(super) struct TonalTools {
    pub draft: Option<TonalDraft>,
    pub probe: Option<TonalProbe>,
    pub changed: bool,
}
pub(super) struct TonalDraft {
    pub revision: u64,
    pub baseline: Option<Selection>,
    pub mode: SelectionMode,
    pub target: SelectionTarget,
    committed: bool,
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn tonal_active(&self) -> bool {
        self.layer_interaction.tool.selection_tool() == Some(SelectionTool::Tonal)
    }
    pub(super) fn cancel_tonal(&mut self) -> bool {
        let active = self.tonal_tools.draft.take().is_some();
        self.tonal_tools.probe = None;
        if active {
            self.region_tools.cancel();
            self.tonal_tools.changed = true;
        }
        active
    }
    pub(super) fn queue_tonal(&mut self, probe: Option<TonalProbe>) -> Result<(), String> {
        if !self.tonal_active() {
            return Ok(());
        }
        let target = self
            .selection_masks
            .target()
            .unwrap_or(SelectionTarget::Current);
        if self
            .tonal_tools
            .draft
            .as_ref()
            .is_some_and(|d| d.revision != self.engine.document().revision || d.target != target)
        {
            self.cancel_tonal();
        }
        let baseline = if target == SelectionTarget::Current {
            self.current_selection()
        } else {
            Some(self.mask_coverage(target)?)
        };
        let mode = self.effective_selection_mode();
        let doc = self.engine.document();
        doc.selection_edit(target, baseline.clone().unwrap_or_else(Selection::empty))
            .map_err(error)?;
        let draft = self.tonal_tools.draft.get_or_insert_with(|| TonalDraft {
            revision: doc.revision,
            baseline,
            mode,
            target,
            committed: false,
        });
        if probe.is_some() {
            draft.mode = mode;
        }
        let request = RegionRequest {
            request_id: 0,
            source: layer_render::RegionSource::Tonal(Box::new(TonalRequest {
                source: layer_render::RegionSource::Composite,
                bands: vec![self.selection_tools.options.tonal.band()],
                invert: false,
                probe,
            })),
            contiguous: false,
            position: [0, 0],
            tolerance: 0.,
            refinement: Default::default(),
            limit: None,
            selection: Some(SelectionRefinement {
                resize: 0,
                mode: draft.mode,
                antialias: true,
                feather: self.selection_tools.options.feather,
                previous: draft.baseline.clone().map(Arc::new),
                source_to_document: Affine::IDENTITY,
            }),
        };
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
                    .sample(sample, probe.point);
                self.queue_tonal(None)?;
            } else {
                self.cancel_tonal();
                self.state.host_error = Some("No visible pixels in the sampled region".into());
            }
        } else {
            let selection = Selection::pixels(result.pixels);
            let draft = self.tonal_tools.draft.as_ref().unwrap();
            if draft.committed {
                self.engine
                    .refine_selection(draft.target, selection, draft.revision)
                    .map_err(error)?;
            } else {
                self.set_mask_coverage(draft.target, selection)?;
            }
            let draft = self.tonal_tools.draft.as_mut().unwrap();
            draft.committed = true;
            draft.revision = self.engine.document().revision;
            self.sync_selection_overlay();
            let display = self.engine.display_selection().map(|s| s.into_owned());
            self.engine
                .backend_mut()
                .set_selection_outline(display.as_ref())
                .map_err(error)?;
            self.refresh_document();
        }
        self.tonal_tools.changed = true;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn tonal_action(&mut self, action: TonalAction) -> Result<(), String> {
        if !self.tonal_active() {
            return Err("Choose Tonal range first".into());
        }
        let TonalAction::Preset { index } = action;
        if index >= TonalOptions::NAMES.len() {
            return Err("Unknown tone".into());
        }
        self.cancel_tonal();
        self.selection_tools.options.tonal.tone = index;
        self.queue_tonal(None)?;
        self.refresh_tools();
        Ok(())
    }
    pub(super) fn tonal_pen(&mut self, event: PenEvent, p: Point) -> Result<(), String> {
        match event.phase {
            PenPhase::Down => {
                self.cancel_tonal();
                self.layer_interaction.path = vec![p, p];
            }
            PenPhase::Move => {
                if self.layer_interaction.path.len() == 2 {
                    self.layer_interaction.path[1] = p;
                }
            }
            PenPhase::Cancel => self.layer_interaction.path.clear(),
            PenPhase::Hover => (),
            PenPhase::Up => {
                let points = std::mem::take(&mut self.layer_interaction.path);
                let Some(start) = points.first() else {
                    return Ok(());
                };
                let doc = self.engine.document();
                let [w, h] = [doc.width, doc.height];
                let point = (p.x - start.x).hypot(p.y - start.y) * self.state.camera.zoom < 4.;
                let bounds = if point {
                    if p.x < 0. || p.y < 0. || p.x >= w as f32 || p.y >= h as f32 {
                        return Ok(());
                    }
                    let [x, y] = [p.x as u32, p.y as u32];
                    [
                        x.saturating_sub(2),
                        y.saturating_sub(2),
                        (x + 3).min(w),
                        (y + 3).min(h),
                    ]
                } else {
                    [
                        (start.x.min(p.x).floor().max(0.) as u32).min(w),
                        (start.y.min(p.y).floor().max(0.) as u32).min(h),
                        (start.x.max(p.x).ceil().max(0.) as u32).min(w),
                        (start.y.max(p.y).ceil().max(0.) as u32).min(h),
                    ]
                };
                if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                    self.queue_tonal(Some(TonalProbe {
                        bounds,
                        point,
                        quad: None,
                    }))?;
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
        vec![ToolOption::Choice {
            id: "tonal-tones",
            label: "Tones",
            segmented: false,
            items: [5, 0, 1, 2, 3, 4, 6, 7]
                .into_iter()
                .map(|index| ToolSetItem {
                    label: TonalOptions::NAMES[index],
                    icon: "tonal-select",
                    action: UiAction::Tonal {
                        action: TonalAction::Preset { index },
                    },
                    selected: self.tonal_tools.draft.is_some()
                        && self.selection_tools.options.tonal.tone == index,
                    preview: None,
                })
                .collect(),
        }]
    }
}
