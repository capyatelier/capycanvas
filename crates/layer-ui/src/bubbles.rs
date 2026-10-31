use crate::*;
use serde::{Deserialize, Serialize};

#[path = "bubble_desk.rs"]
mod desk;
pub(crate) use desk::{Desk, Shape};

pub const TOOL_BUBBLE: f32 = 56.;
pub const HISTORY_BUBBLE: [f32; 2] = [104., 56.];
pub const CONTROL_RINGS: [ToolbarNumericBinding; 2] = [ToolbarNumericBinding::BrushSize, ToolbarNumericBinding::BrushStrength];
pub const HISTORY_COMMANDS: [CommandId; 2] = [CommandId::Undo, CommandId::Redo];
pub const COLOR_RING_SIZES: [f32; 3] = [136., 216., 344.];
pub const COLOR_RING_RANGE: [f32; 2] = [128., 352.];
pub const RING_RIM: f32 = 7.;
pub const BUBBLE_BEAD: f32 = 24.;
pub const PALETTE_BEAD: f32 = 26.;
pub const BEAD_STEP: f32 = 28.;
pub const RECENT_BEADS: usize = 8;
pub const MAX_RING_SETTINGS: usize = 6;
pub const RING_TRACK: f32 = 4.;
pub const HALO_RINGS: [f32; 2] = [53., 88.];
pub const HALO_BANDS: [f32; 2] = [28., 28.];
pub const RING_ICON: f32 = 21.;
pub const CHOOSER_ITEM: f32 = 44.;
pub const CHOOSER_RING: f32 = 84.;
pub const BRUSH_PILL: [f32; 2] = [152., 48.];
pub const CHOOSER_TAG: [f32; 2] = [96., 26.];
const TAG_GAP: f32 = 4.;
pub const PALETTE_PILL: [f32; 2] = [184., 30.];
pub const STROKE_FADE: f32 = 48.;
pub const FADED_BUBBLE: f32 = 0.2;
pub const SECOND_FINGER_REACH: f32 = 90.;
const BRUSH_COLUMN: f32 = 170.;
const PILL_PITCH: f32 = BRUSH_PILL[1] + 4.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BubbleSource {
    Control { control: ToolbarControl },
    Medium { group: ToolGroup },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleKind {
    Dial,
    Brush,
    Medium,
    Controls,
    History,
    Color,
    Layers,
}

impl BubbleSource {
    pub fn for_control(control: ToolbarControl) -> Option<Self> {
        let control = match control {
            ToolbarControl::Command { command: CommandId::Redo } => ToolbarControl::Command { command: CommandId::Undo },
            ToolbarControl::Command { command: CommandId::Undo }
            | ToolbarControl::Color
            | ToolbarControl::ColorPicker
            | ToolbarControl::Brush { .. }
            | ToolbarControl::Panel { panel: Panel::Layers } => control,
            control if control.tool_group().is_some() => control,
            _ => return None,
        };
        Some(Self::Control { control })
    }
    pub fn tears_without_hold(control: ToolbarControl) -> bool {
        Self::for_control(control).is_some() && !control.is_component()
    }
    pub fn for_variant(variant: ToolVariant) -> Option<Self> {
        match variant {
            ToolVariant::BrushGroup { group } => Some(Self::Medium { group }),
            ToolVariant::BrushPreset { id } => Some(Self::Control { control: ToolbarControl::Brush { id } }),
            _ => None,
        }
    }
    pub fn kind(self) -> BubbleKind {
        match self {
            Self::Medium { .. } => BubbleKind::Medium,
            Self::Control { control: ToolbarControl::Brush { .. } } => BubbleKind::Brush,
            Self::Control { control: ToolbarControl::Color } => BubbleKind::Color,
            Self::Control { control: ToolbarControl::Panel { .. } } => BubbleKind::Layers,
            Self::Control { control: ToolbarControl::ColorPicker } => BubbleKind::Controls,
            Self::Control { control: ToolbarControl::Command { command: CommandId::Undo } } => BubbleKind::History,
            Self::Control { .. } => BubbleKind::Dial,
        }
    }
    pub fn control(self) -> Option<ToolbarControl> {
        match self {
            Self::Control { control } => Some(control),
            Self::Medium { .. } => None,
        }
    }
    pub fn label(self, localization: &Localizer) -> String {
        match self {
            Self::Control { control } => customization::tool_choice_localized(control, localization).label,
            Self::Medium { group } => group.localized_label(localization).to_string(),
        }
    }
    pub(crate) fn validate(self) -> Result<(), String> {
        let valid = match self {
            Self::Control { control } => {
                control.validate().is_ok()
                    && Self::for_control(control) == Some(self)
                    && !matches!(control, ToolbarControl::Brush { id } if preset(id).is_err())
            }
            Self::Medium { .. } => true,
        };
        valid.then_some(()).ok_or_else(|| "Invalid tool bubble".into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BubbleOptions {
    pub size: f32,
    pub recent: bool,
    pub palette: bool,
    pub pair: bool,
    pub full: bool,
    pub fold: bool,
    pub swap: bool,
}
impl Default for BubbleOptions {
    fn default() -> Self {
        Self { size: COLOR_RING_SIZES[1], recent: false, palette: false, pair: false, full: false, fold: true, swap: false }
    }
}
impl BubbleOptions {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Corner {
    fn point(self, viewport: [f32; 2]) -> [f32; 2] {
        match self {
            Self::TopLeft => [0., 0.],
            Self::TopRight => [viewport[0], 0.],
            Self::BottomLeft => [0., viewport[1]],
            Self::BottomRight => viewport,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolBubble {
    pub id: u32,
    pub source: BubbleSource,
    pub corner: Corner,
    pub offset: [f32; 2],
    #[serde(default, skip_serializing_if = "BubbleOptions::is_default")]
    pub options: BubbleOptions,
}

pub fn pinched_size(size: f32, scale: f32) -> f32 {
    (size * scale).clamp(COLOR_RING_RANGE[0], COLOR_RING_RANGE[1])
}

impl ToolBubble {
    pub(crate) fn new(id: u32, source: BubbleSource, center: [f32; 2], viewport: [f32; 2]) -> Self {
        let mut bubble = Self { id, source, corner: Corner::TopLeft, offset: center, options: BubbleOptions::default() };
        bubble.place(center, viewport);
        bubble
    }
    pub fn center(&self, viewport: [f32; 2]) -> [f32; 2] {
        let corner = self.corner.point(viewport);
        [corner[0] + self.offset[0], corner[1] + self.offset[1]]
    }
    pub(crate) fn place(&mut self, center: [f32; 2], viewport: [f32; 2]) {
        let corner = match (center[0] * 2. > viewport[0], center[1] * 2. > viewport[1]) {
            (false, false) => Corner::TopLeft,
            (true, false) => Corner::TopRight,
            (false, true) => Corner::BottomLeft,
            (true, true) => Corner::BottomRight,
        };
        let point = corner.point(viewport);
        self.corner = corner;
        self.offset = [center[0] - point[0], center[1] - point[1]];
    }
    pub fn diameter(&self, narrow: bool, show_sliders: bool) -> f32 {
        match self.source.kind() {
            BubbleKind::Controls if show_sliders => halo_disc(CONTROL_RINGS.len()),
            BubbleKind::History => HISTORY_BUBBLE[0],
            BubbleKind::Color if narrow => self.options.size.min(COLOR_RING_SIZES[0]),
            BubbleKind::Color => self.options.size,
            _ => TOOL_BUBBLE,
        }
    }
    pub(crate) fn shape(&self, narrow: bool, pair: PairBeads, show_sliders: bool) -> Shape {
        match self.source.kind() {
            BubbleKind::Color => Shape::circle(color_reach(self.diameter(narrow, show_sliders), self.options, pair)),
            BubbleKind::Layers => Shape::rect([TOOL_BUBBLE; 2]),
            BubbleKind::History => Shape::rect(HISTORY_BUBBLE),
            _ => Shape::circle(self.diameter(narrow, show_sliders) * 0.5),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RingSettings {
    pub brush: u32,
    pub settings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptOptions {
    pub source: BubbleSource,
    pub options: BubbleOptions,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BubbleLayout {
    pub show_sliders: bool,
    pub bubbles: Vec<ToolBubble>,
    pub rings: Vec<RingSettings>,
    pub kept: Vec<KeptOptions>,
}
impl Default for BubbleLayout {
    fn default() -> Self {
        Self { show_sliders: true, bubbles: Vec::new(), rings: Vec::new(), kept: Vec::new() }
    }
}
impl BubbleLayout {
    pub fn is_empty(&self) -> bool {
        self.show_sliders && self.bubbles.is_empty() && self.rings.is_empty() && self.kept.is_empty()
    }
    pub(crate) fn put_back(&mut self, gone: impl Fn(&ToolBubble) -> bool) {
        let (removed, staying): (Vec<_>, Vec<_>) = std::mem::take(&mut self.bubbles).into_iter().partition(|b| gone(b));
        self.bubbles = staying;
        for bubble in removed {
            self.kept.retain(|k| k.source != bubble.source);
            if bubble.options != BubbleOptions::default() {
                self.kept.push(KeptOptions { source: bubble.source, options: bubble.options });
            }
        }
    }
    pub(crate) fn options_for(&self, source: BubbleSource) -> BubbleOptions {
        self.kept.iter().find(|k| k.source == source).map_or_else(BubbleOptions::default, |k| k.options)
    }
    pub fn get(&self, id: u32) -> Option<&ToolBubble> {
        self.bubbles.iter().find(|b| b.id == id)
    }
    pub(crate) fn get_mut(&mut self, id: u32) -> Result<&mut ToolBubble, String> {
        self.bubbles.iter_mut().find(|b| b.id == id).ok_or_else(|| "The bubble no longer exists".into())
    }
    pub fn find(&self, source: BubbleSource) -> Option<&ToolBubble> {
        self.bubbles.iter().find(|b| b.source == source)
    }
    pub fn ring(&self, brush: u32) -> Option<&[String]> {
        self.rings.iter().find(|r| r.brush == brush).map(|r| r.settings.as_slice())
    }
    pub(crate) fn set_ring(&mut self, brush: u32, settings: Vec<String>) {
        self.rings.retain(|r| r.brush != brush);
        self.rings.push(RingSettings { brush, settings });
    }
    pub(crate) fn validate(&self, ids: &mut std::collections::BTreeSet<u32>) -> Result<(), String> {
        let mut sources = Vec::new();
        for bubble in &self.bubbles {
            bubble.source.validate()?;
            if !ids.insert(bubble.id)
                || sources.contains(&bubble.source)
                || !bubble.offset.into_iter().all(f32::is_finite)
                || !(COLOR_RING_RANGE[0]..=COLOR_RING_RANGE[1]).contains(&bubble.options.size)
            {
                return Err("Invalid tool bubble".into());
            }
            sources.push(bubble.source);
        }
        let mut kept = Vec::new();
        for k in &self.kept {
            k.source.validate()?;
            if kept.contains(&k.source) || !(COLOR_RING_RANGE[0]..=COLOR_RING_RANGE[1]).contains(&k.options.size) {
                return Err("Invalid kept bubble options".into());
            }
            kept.push(k.source);
        }
        let mut brushes = Vec::new();
        for ring in &self.rings {
            let mut seen = Vec::new();
            if preset(ring.brush).is_err()
                || brushes.contains(&ring.brush)
                || ring.settings.len() > MAX_RING_SETTINGS
                || ring.settings.iter().any(|s| seen.contains(&s) || { seen.push(s); !tool_settings::defined(s) })
            {
                return Err("Invalid bubble ring settings".into());
            }
            brushes.push(ring.brush);
        }
        Ok(())
    }
}

fn degrees(point: [f32; 2]) -> f32 {
    point[1].atan2(point[0]).to_degrees().rem_euclid(360.)
}

fn polar(radius: f32, degrees: f32) -> [f32; 2] {
    let a = degrees.to_radians();
    [radius * a.cos(), radius * a.sin()]
}

fn turn(from: f32, to: f32) -> f32 {
    (to - from + 540.).rem_euclid(360.) - 180.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bead {
    Recent { index: usize },
    Swatch { index: usize },
    Foreground,
    Background,
    Transparent,
    Palettes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeadStrip {
    Recent,
    Palette,
}

impl Bead {
    pub fn strip(self) -> Option<BeadStrip> {
        match self {
            Self::Recent { .. } => Some(BeadStrip::Recent),
            Self::Swatch { .. } => Some(BeadStrip::Palette),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeadPlacement {
    pub bead: Bead,
    pub center: [f32; 2],
    pub size: f32,
    pub alpha: f32,
}

fn orbit(size: f32, row: usize) -> f32 {
    size * 0.5 + 19. + row as f32 * BEAD_STEP
}

fn bead_degrees(size: f32) -> f32 {
    (BEAD_STEP / orbit(size, 0)).to_degrees()
}

fn strip_columns(strip: BeadStrip, size: f32) -> usize {
    let step = bead_degrees(size);
    match strip {
        BeadStrip::Recent => ((104. / step) as usize + 1).min(RECENT_BEADS),
        BeadStrip::Palette => (112. / step) as usize + 1,
    }
}

fn strip_angle(strip: BeadStrip, size: f32, column: f32, last: f32) -> f32 {
    let (middle, direction) = match strip {
        BeadStrip::Recent => (270., 1.),
        BeadStrip::Palette => (90., -1.),
    };
    middle + direction * (column - last * 0.5) * bead_degrees(size)
}

fn strip_rows(strip: BeadStrip) -> usize {
    match strip {
        BeadStrip::Recent => 1,
        BeadStrip::Palette => 2,
    }
}

pub(crate) fn bead_scroll_limit(strip: BeadStrip, size: f32, count: usize) -> f32 {
    count.div_ceil(strip_rows(strip)).saturating_sub(strip_columns(strip, size)) as f32
}

pub(crate) fn bead_travel(strip: BeadStrip, size: f32, from: [f32; 2], to: [f32; 2]) -> f32 {
    let beads = turn(degrees(from), degrees(to)) / bead_degrees(size);
    match strip {
        BeadStrip::Recent => -beads,
        BeadStrip::Palette => beads,
    }
}

const FADE_COLUMNS: f32 = 0.8;
const HINT_COLUMN: f32 = 0.65;
pub const HINT_DOT: f32 = 5.;
const PALETTE_ROOM: f32 = 6.;
const PALETTE_GAP: f32 = 1.35;
const BEAD_CLEARANCE: f32 = 3.;

fn strip_last(strip: BeadStrip, size: f32, count: usize) -> f32 {
    strip_columns(strip, size).min(count.div_ceil(strip_rows(strip))) as f32 - 1.
}

fn faded(column: f32, last: f32) -> f32 {
    let excess = (-column).max(column - last).max(0.);
    (1. - excess / FADE_COLUMNS).clamp(0., 1.)
}

fn strip_point(layout: ColorBubbleLayout, strip: BeadStrip, row: usize, column: f32, last: f32) -> [f32; 2] {
    if strip == BeadStrip::Palette && layout.intensity.is_some() {
        let inner = layout.orbit_from_bottom((last * 0.5 - column) * BEAD_STEP, 19.);
        let distance = inner[0].hypot(inner[1]);
        let scale = layout.radius_at(inner, 19. + row as f32 * BEAD_STEP) / distance;
        inner.map(|v| v * scale)
    } else {
        layout.orbit(polar(orbit(layout.diameter, row), strip_angle(strip, layout.diameter, column, last)))
    }
}

fn strip_beads(beads: &mut Vec<BeadPlacement>, strip: BeadStrip, layout: ColorBubbleLayout, count: usize, scroll: f32) {
    let size = layout.diameter;
    let rows = strip_rows(strip);
    let last = strip_last(strip, size, count);
    for index in 0..count {
        let column = (index / rows) as f32 - scroll;
        let alpha = faded(column, last);
        if alpha > 0. {
            let bead = if strip == BeadStrip::Palette { Bead::Swatch { index } } else { Bead::Recent { index } };
            let center = strip_point(layout, strip, index % rows, column, last);
            beads.push(BeadPlacement { bead, center, size: BUBBLE_BEAD * (0.6 + 0.4 * alpha), alpha });
        }
    }
}

fn strip_edges(strip: BeadStrip, layout: ColorBubbleLayout, count: usize) -> Vec<([f32; 2], f32)> {
    let size = layout.diameter;
    let last = strip_last(strip, size, count);
    let columns = (0..=8).flat_map(|i| [-(i as f32) * 0.1, last + i as f32 * 0.1]);
    columns
        .flat_map(|column| (0..strip_rows(strip)).map(move |row| (column, row)))
        .map(|(column, row)| {
            let radius = BUBBLE_BEAD * 0.5 * (0.6 + 0.4 * faded(column, last));
            (strip_point(layout, strip, row, column, last), radius)
        })
        .collect()
}

fn clear(center: [f32; 2], radius: f32, obstacles: &[([f32; 2], f32)]) -> bool {
    obstacles.iter().all(|(at, r)| (center[0] - at[0]).hypot(center[1] - at[1]) >= radius + r + BEAD_CLEARANCE)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PairBeads {
    pub foreground: f32,
    pub background: f32,
    pub gap: f32,
}

impl PairBeads {
    const SMALLEST: Self = Self { foreground: 30., background: 24., gap: 10. };

    pub fn for_window(width: f32) -> Self {
        let stage = Panel::Color.default_width(width) - 2. * PANEL_CONTENT_INSET;
        let Some((layout, wheel)) = ColorPanelLayout::new(stage).and_then(|l| Some((l, ColorWheelGeometry::new(l.wheel[2])?))) else {
            return Self::SMALLEST;
        };
        let center = [layout.wheel[0] + layout.wheel[2] * 0.5, layout.wheel[1] + layout.wheel[3] * 0.5];
        let gap = [layout.foreground, layout.background, layout.transparent]
            .map(|[x, y, w, _]| (x + w * 0.5 - center[0]).hypot(y + w * 0.5 - center[1]) - wheel.outer - w * 0.5)
            .into_iter()
            .fold(f32::MAX, f32::min);
        Self { foreground: layout.foreground[2], background: layout.background[2], gap }
    }

    fn for_bubble(self, bubble: ColorBubbleLayout) -> Self {
        if bubble.intensity.is_none() { return self; }
        let layout = bubble.panel();
        let wheel = ColorWheelGeometry::new(bubble.wheel_size()).unwrap();
        let sdr = ColorPanelLayout::new(bubble.stage).unwrap();
        let gap = [sdr.foreground, sdr.background, sdr.transparent].into_iter().map(|[x, y, w, _]|
            (x + w * 0.5 - bubble.stage * 0.5).hypot(y + w * 0.5 - bubble.stage * 0.5) - wheel.outer - w * 0.5)
            .fold(f32::MAX, f32::min);
        Self { foreground: layout.foreground[2], background: layout.background[2], gap: (gap - bubble.margin()).max(4.) }
    }

    fn offset(self) -> [f32; 2] {
        let (fg, bg) = (self.foreground, self.background);
        [0.04 * fg + 0.5 * bg, 0.5 * fg - 0.24 * bg]
    }
}

impl ColorBubbleLayout {
    fn pair_angle(self) -> f32 {
        if self.intensity.is_none() { return 140.; }
        let p = self.panel().foreground;
        degrees([p[0] + p[2] * 0.5 - self.stage * 0.5, p[1] + p[3] * 0.5 - self.stage * 0.5])
    }
}

fn pair_at(layout: ColorBubbleLayout, angle: f32, pair: PairBeads) -> [[f32; 2]; 2] {
    let radius = layout.diameter * 0.5 + pair.gap;
    let offset = if layout.intensity.is_some() {
        let p = layout.panel();
        [p.background[0] + p.background[2] * 0.5 - p.foreground[0] - p.foreground[2] * 0.5,
            p.background[1] + p.background[3] * 0.5 - p.foreground[1] - p.foreground[3] * 0.5]
    } else { pair.offset() };
    let u = polar(1., angle);
    let along = offset[0] * u[0] + offset[1] * u[1];
    let reach = (radius + pair.background * 0.5).powi(2) - (offset[0] * offset[0] + offset[1] * offset[1]) + along * along;
    let distance = (radius + pair.foreground * 0.5).max(reach.max(0.).sqrt() - along);
    let distance = if layout.intensity.is_some() {
        let (mut low, mut high) = (distance, distance + layout.diameter);
        for _ in 0..20 {
            let mid = (low + high) * 0.5;
            let f = polar(mid, angle);
            let b = [f[0] + offset[0], f[1] + offset[1]];
            if [(f, pair.foreground), (b, pair.background)].into_iter().all(|(p, size)|
                p[0].hypot(p[1]) >= layout.radius_at(p, pair.gap + size * 0.5)) { high = mid; } else { low = mid; }
        }
        high
    } else { distance };
    let foreground = polar(distance, angle);
    [foreground, [foreground[0] + offset[0], foreground[1] + offset[1]]]
}

pub fn color_beads(layout: ColorBubbleLayout, options: BubbleOptions, pair: PairBeads, recent: usize, swatches: usize, scroll: [f32; 2], front: Bead) -> Vec<BeadPlacement> {
    let size = layout.diameter;
    let pair = pair.for_bubble(layout);
    let mut beads = Vec::new();
    let mut obstacles = Vec::new();
    if options.recent {
        strip_beads(&mut beads, BeadStrip::Recent, layout, recent, scroll[0]);
        obstacles.extend(strip_edges(BeadStrip::Recent, layout, recent));
    }
    let mut palette = None;
    if options.palette {
        strip_beads(&mut beads, BeadStrip::Palette, layout, swatches, scroll[1]);
        obstacles.extend(strip_edges(BeadStrip::Palette, layout, swatches));
        let last = strip_last(BeadStrip::Palette, size, swatches);
        let angle = degrees(strip_point(layout, BeadStrip::Palette, 0, last + PALETTE_GAP, last));
        let ends = [0, 1].map(|row| strip_point(layout, BeadStrip::Palette, row, last, last));
        let middle = (ends[0][0].hypot(ends[0][1]) + ends[1][0].hypot(ends[1][1])) * 0.5;
        let center = polar(middle / (angle - degrees(ends[0])).to_radians().cos(), angle);
        palette = Some(BeadPlacement { bead: Bead::Palettes, center, size: PALETTE_BEAD, alpha: 1. });
    }
    if let Some(bead) = palette {
        obstacles.push((bead.center, bead.size * 0.5 + PALETTE_ROOM));
    }
    if options.pair {
        let last = strip_last(BeadStrip::Palette, size, swatches);
        let start = if options.palette { degrees(strip_point(layout, BeadStrip::Palette, 0, -FADE_COLUMNS, last)) } else { layout.pair_angle() };
        let place = |angle: f32| pair_at(layout, angle, pair);
        let fits = |[f, b]: [[f32; 2]; 2]| clear(f, pair.foreground * 0.5, &obstacles) && clear(b, pair.background * 0.5, &obstacles);
        let angle = (0..90).map(|i| start + i as f32).find(|a| fits(place(*a))).unwrap_or(start);
        let [foreground, background] = place(angle);
        let beads_in_order = [(Bead::Background, background, pair.background), (Bead::Foreground, foreground, pair.foreground)];
        let order = if front == Bead::Background { [beads_in_order[1], beads_in_order[0]] } else { beads_in_order };
        for (bead, center, size) in order {
            beads.push(BeadPlacement { bead, center, size, alpha: 1. });
            obstacles.push((center, size * 0.5));
        }
        let radius = size * 0.5 + pair.gap + pair.background * 0.5;
        let start = palette.map_or(45., |p| degrees(p.center) - 10.);
        let place = |angle| layout.orbit(polar(radius, angle));
        let angle = (0..90).map(|i| start - i as f32).find(|a| clear(place(*a), pair.background * 0.5, &obstacles)).unwrap_or(start);
        beads.push(BeadPlacement { bead: Bead::Transparent, center: place(angle), size: pair.background, alpha: 1. });
    }
    beads.extend(palette);
    beads
}

pub fn bead_hints(layout: ColorBubbleLayout, options: BubbleOptions, recent: usize, swatches: usize, scroll: [f32; 2]) -> Vec<([f32; 2], f32)> {
    let size = layout.diameter;
    let mut hints = Vec::new();
    for (shown, strip, count, scroll) in [(options.recent, BeadStrip::Recent, recent, scroll[0]), (options.palette, BeadStrip::Palette, swatches, scroll[1])] {
        if !shown {
            continue;
        }
        let (rows, last) = (strip_rows(strip), strip_last(strip, size, count));
        let beyond = count.div_ceil(rows) as f32 - 1. - last - scroll;
        let fade = |hidden: f32| ((hidden - FADE_COLUMNS) / (1. - FADE_COLUMNS)).clamp(0., 1.);
        for (column, alpha) in [(-HINT_COLUMN, fade(scroll)), (last + HINT_COLUMN, fade(beyond))] {
            if alpha > 0. {
                hints.extend((0..rows).map(|row| (strip_point(layout, strip, row, column, last), alpha)));
            }
        }
    }
    hints
}

pub fn bead_at(beads: &[BeadPlacement], point: [f32; 2]) -> Option<Bead> {
    beads
        .iter()
        .rev()
        .filter(|b| b.alpha > 0.6)
        .find(|b| (point[0] - b.center[0]).hypot(point[1] - b.center[1]) <= b.size * 0.5 + 2.)
        .map(|b| b.bead)
}

pub fn palette_pill(size: f32) -> Bounds {
    Bounds { x: -PALETTE_PILL[0] * 0.5, y: size * 0.5 + 70., width: PALETTE_PILL[0], height: PALETTE_PILL[1] }
}

pub fn color_drag_out(size: f32, point: [f32; 2]) -> bool {
    point[0].hypot(point[1]) > size
}

pub fn color_reach(size: f32, options: BubbleOptions, pair: PairBeads) -> f32 {
    let [x, y] = pair.offset();
    [
        (true, size * 0.5),
        (options.recent, orbit(size, 0) + BUBBLE_BEAD * 0.5),
        (options.palette, orbit(size, 1) + BUBBLE_BEAD * 0.5),
        (options.pair, size * 0.5 + pair.gap + (pair.foreground + pair.background) * 0.5 + x.hypot(y)),
    ]
    .into_iter()
    .filter(|(shown, _)| *shown)
    .fold(0., |reach, (_, r)| f32::max(reach, r))
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RingArc {
    pub radius: f32,
    pub start: f32,
    pub sweep: f32,
    pub band: f32,
}
impl RingArc {
    pub fn point(&self, fraction: f32) -> [f32; 2] {
        polar(self.radius, self.start + self.sweep * fraction.clamp(0., 1.))
    }
    pub fn icon_point(&self) -> [f32; 2] {
        polar(self.radius, self.start - self.sweep.signum() * (RING_ICON / self.radius).to_degrees())
    }
    pub fn covers(&self, point: [f32; 2]) -> bool {
        let icon = (RING_ICON / self.radius).to_degrees();
        let direction = self.sweep.signum();
        let from = self.start - direction * icon;
        let span = self.sweep.abs() + icon;
        let offset = (direction * (degrees(point) - from)).rem_euclid(360.);
        let along = if offset <= span { offset } else if offset - span < 360. - offset { span } else { 0. };
        let [x, y] = polar(self.radius, from + direction * along);
        (point[0] - x).hypot(point[1] - y) <= self.band * 0.5
    }
    pub fn fraction(&self, point: [f32; 2]) -> f32 {
        let span = self.sweep.abs();
        let angle = degrees(point);
        let offset = (if self.sweep >= 0. { angle - self.start } else { self.start - angle }).rem_euclid(360.);
        if offset <= span {
            offset / span
        } else if offset - span < 360. - offset {
            1.
        } else {
            0.
        }
    }
}

pub fn ring_at(arcs: &[RingArc], point: [f32; 2]) -> Option<usize> {
    arcs.iter().position(|arc| arc.covers(point))
}

pub fn bubble_face(kind: BubbleKind, diameter: f32) -> [f32; 2] {
    match kind {
        BubbleKind::Controls => [TOOL_BUBBLE; 2],
        BubbleKind::History => HISTORY_BUBBLE,
        _ => [diameter; 2],
    }
}

pub fn history_center(index: usize) -> [f32; 2] {
    let [width, height] = HISTORY_BUBBLE;
    [(width - height) * if index == 0 { -0.5 } else { 0.5 }, 0.]
}

pub fn history_button(point: [f32; 2]) -> Option<usize> {
    let index = usize::from(point[0] >= 0.);
    let [x, y] = history_center(index);
    ((point[0] - x).hypot(point[1] - y) <= HISTORY_BUBBLE[1] * 0.5).then_some(index)
}

fn on_capsule(point: [f32; 2], [width, height]: [f32; 2]) -> bool {
    (point[0].abs() - (width - height) * 0.5).max(0.).hypot(point[1]) <= height * 0.5
}

fn halo_ring(count: usize, ring: usize) -> Vec<RingArc> {
    let slots: &[(f32, f32)] = match (count, ring) {
        (1, _) => &[(270., 272.)],
        (2, _) => &[(180., 138.), (0., 138.)],
        (_, 0) => &[(270., 112.), (30., 112.), (150., 112.)],
        _ => &[(90., 112.), (210., 112.), (330., 112.)],
    };
    let (radius, band) = (HALO_RINGS[ring], HALO_BANDS[ring]);
    let icon = (RING_ICON / radius).to_degrees();
    let reach = |span: f32| span * 0.5 - (band * 0.5 / radius).to_degrees();
    slots
        .iter()
        .map(|&(middle, span)| {
            let (sin, cos) = f32::to_radians(middle).sin_cos();
            let direction = if cos.abs() > 0.7 { -cos.signum() } else { -sin.signum() };
            RingArc {
                radius,
                start: middle - direction * (reach(span) - icon),
                sweep: direction * (reach(span) * 2. - icon),
                band,
            }
        })
        .collect()
}

pub fn halo_arcs(count: usize) -> Vec<RingArc> {
    let (inner, outer) = match count {
        0..=3 => (count, 0),
        4 => (2, 2),
        _ => (3, count.min(MAX_RING_SETTINGS) - 3),
    };
    let mut arcs = if inner > 0 { halo_ring(inner, 0) } else { Vec::new() };
    if outer > 0 {
        arcs.extend(halo_ring(outer, 1));
    }
    arcs
}

pub fn halo_disc(count: usize) -> f32 {
    let ring = usize::from(count > 3);
    (HALO_RINGS[ring] + HALO_BANDS[ring] * 0.5) * 2.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChooserKind {
    Medium,
    Tool,
    Brush,
}

fn fits(bounds: Bounds, point: [f32; 2], half: f32) -> bool {
    point[0] - half >= bounds.x
        && point[1] - half >= bounds.y
        && point[0] + half <= bounds.x + bounds.width
        && point[1] + half <= bounds.y + bounds.height
}

fn free_arc(center: [f32; 2], radius: f32, bounds: Bounds) -> Option<(f32, f32)> {
    let free: Vec<bool> = (0..72)
        .map(|i| {
            let p = polar(radius, i as f32 * 5.);
            fits(bounds, [center[0] + p[0], center[1] + p[1]], CHOOSER_ITEM * 0.5 + 2.)
        })
        .collect();
    if free.iter().all(|f| *f) {
        return Some((0., 360.));
    }
    (0..72)
        .filter(|&i| free[i] && !free[(i + 71) % 72])
        .map(|i| (i, (0..72).take_while(|n| free[(i + n) % 72]).count()))
        .max_by_key(|run| run.1)
        .map(|(start, run)| (start as f32 * 5., (run - 1) as f32 * 5.))
}

pub fn chooser_ring(count: usize, center: [f32; 2], bounds: Bounds) -> Vec<[f32; 2]> {
    let pitch = CHOOSER_ITEM + 6.;
    let base = CHOOSER_RING.max(pitch * count as f32 / std::f32::consts::TAU);
    if free_arc(center, base, bounds) == Some((0., 360.)) {
        return (0..count).map(|i| polar(base, -90. + i as f32 * 360. / count as f32)).collect();
    }
    let mut items = Vec::new();
    for radius in (0..8).map(|row| base + row as f32 * pitch) {
        let Some((start, span)) = free_arc(center, radius, bounds).filter(|(_, span)| *span < 360.) else { continue };
        let step = (pitch / radius).to_degrees();
        let room = (span / step) as usize + 1;
        let take = room.min(count - items.len());
        let middle = start + span * 0.5;
        items.extend((0..take).map(|i| polar(radius, middle + (i as f32 - (take - 1) as f32 * 0.5) * step)));
        if items.len() == count {
            break;
        }
    }
    items.resize(count, polar(base, -90.));
    items
}

pub fn chooser_column(count: usize, center: [f32; 2], medium: Option<([f32; 2], f32, f32)>, bounds: Bounds) -> Vec<[f32; 2]> {
    let n = count as f32;
    let (radius, side, middle) = match medium {
        Some((item, reach, tag)) => {
            let tag = tag_bounds(item, tag);
            let label = (tag.x.abs().max((tag.x + tag.width).abs()) - item[0].abs()).max(CHOOSER_ITEM * 0.5);
            let radius = |beside: f32| (n * PILL_PITCH * 0.5 + 20.).max(BRUSH_COLUMN).max(reach + beside + BRUSH_PILL[0] * 0.5 + 8.);
            let room = |side: f32, radius: f32| {
                if side > 0. { bounds.x + bounds.width - (center[0] + radius + BRUSH_PILL[0] * 0.5) } else { center[0] - radius - BRUSH_PILL[0] * 0.5 - bounds.x }
            };
            let side = if item[0] > -1. { 1. } else { -1. };
            let (near, far) = (radius(label), radius(CHOOSER_ITEM * 0.5));
            if room(side, near) < 0. && room(-side, far) > room(side, near) { (far, -side, item[1]) } else { (near, side, item[1]) }
        }
        None => {
            let radius = (n * PILL_PITCH * 0.5 + 18.).max(TOOL_BUBBLE * 0.5 + BRUSH_PILL[0].hypot(BRUSH_PILL[1]) * 0.5 + WORKSPACE_SPACING);
            let room = |right: bool| {
                if right { bounds.x + bounds.width - (center[0] + radius + BRUSH_PILL[0] * 0.5) } else { center[0] - radius - BRUSH_PILL[0] * 0.5 - bounds.x }
            };
            let right = if room(true) >= 0. && (center[0] <= bounds.x + bounds.width * 0.5 || room(false) < 0.) {
                true
            } else if room(false) >= 0. {
                false
            } else {
                room(true) > room(false)
            };
            (radius, if right { 1. } else { -1. }, 0.)
        }
    };
    let half = n.max(1.) * PILL_PITCH * 0.5 - PILL_PITCH * 0.5;
    let top = bounds.y + BRUSH_PILL[1] * 0.5 - center[1];
    let bottom = bounds.y + bounds.height - BRUSH_PILL[1] * 0.5 - center[1];
    let mut first = (middle - half).max(top);
    if first + 2. * half > bottom {
        first = (bottom - 2. * half).max(top);
    }
    (0..count)
        .map(|i| {
            let y = first + i as f32 * PILL_PITCH;
            let dy = y - middle;
            let x = (radius * radius - dy * dy).max(0.).sqrt().max(BRUSH_PILL[0] * 0.5 + 16.);
            [side * x, y]
        })
        .collect()
}

pub fn tag_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 8. + 24.).min(CHOOSER_TAG[0])
}

pub fn tag_bounds(item: [f32; 2], w: f32) -> Bounds {
    let length = item[0].hypot(item[1]);
    let d = if length > 1e-3 { [item[0] / length, item[1] / length] } else { [0., -1.] };
    let h = CHOOSER_TAG[1];
    let out = CHOOSER_ITEM * 0.5 + TAG_GAP + d[0].abs() * w * 0.5 + d[1].abs() * h * 0.5;
    Bounds { x: item[0] + d[0] * out - w * 0.5, y: item[1] + d[1] * out - h * 0.5, width: w, height: h }
}

pub fn layers_card(viewport: [f32; 2], layers: usize, measured: Option<[f32; 2]>, properties: bool) -> [f32; 2] {
    let guess = PanelScrollMeasurement { fixed_height: 92., unit_height: 40. };
    let heights = measured.unwrap_or_else(|| LayerDrawerSizing::pages(guess, layers, viewport[1], 160.));
    [Panel::Layers.default_width(viewport[0]), heights[usize::from(properties)]]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BubbleGrab {
    Bubble { id: u32 },
    Rim { id: u32 },
    Tile { panel: Panel, tile: u32 },
    Header { id: u32 },
    Variant { variant: ToolVariant },
}

impl BubbleGrab {
    pub fn for_choice(action: &UiAction) -> Option<Self> {
        let variant = match action {
            UiAction::SelectBrush { id } => ToolVariant::BrushPreset { id: *id },
            UiAction::SelectToolGroup { group } | UiAction::SelectBrushSet { group } => ToolVariant::BrushGroup { group: *group },
            UiAction::ChooseToolVariant { variant, .. } => *variant,
            _ => return None,
        };
        BubbleSource::for_variant(variant).map(|_| Self::Variant { variant })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BubbleAction {
    Drag { grab: BubbleGrab, phase: ContactPhase, position: [f32; 2], viewport: [f32; 2] },
    Tap { id: u32, position: [f32; 2] },
    Ring { id: u32, phase: ContactPhase, position: [f32; 2] },
    Wheel { id: u32, phase: ContactPhase, part: ColorWheelPart, position: [f32; 2] },
    Intensity { id: u32, phase: ContactPhase, position: [f32; 2] },
    SetIntensity { id: u32, stops: f32 },
    ResetRing { id: u32, position: [f32; 2] },
    Beads { id: u32, phase: ContactPhase, position: [f32; 2] },
    Choose { id: u32, phase: ContactPhase, position: [f32; 2] },
    PutBack { id: u32 },
    PutBackAll,
    SetOptions { id: u32, options: BubbleOptions },
    ToggleSliders { id: u32 },
    Option { id: u32, option: BubbleOption },
    ToggleRingSetting { brush: u32, setting: String },
    Palette { id: u32, step: i32 },
    Resize { id: u32, scale: f32 },
    Properties { id: u32 },
    Close { id: u32 },
    MeasureCard { id: u32, measurement: PanelScrollMeasurement, properties_height: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BubbleOption {
    Recent,
    Palette,
    Pair,
    Swap,
    Fold,
    Full { full: bool },
    Size { size: f32 },
}

impl BubbleOption {
    pub(crate) fn apply(self, mut options: BubbleOptions) -> BubbleOptions {
        match self {
            Self::Recent => options.recent = !options.recent,
            Self::Palette => options.palette = !options.palette,
            Self::Pair => options.pair = !options.pair,
            Self::Swap => options.swap = !options.swap,
            Self::Fold => options.fold = !options.fold,
            Self::Full { full } => options.full = full,
            Self::Size { size } => options.size = size,
        }
        options
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BubbleButton {
    pub label: String,
    pub icon: &'static str,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BubbleStamp {
    pub center: [f32; 2],
    pub diameter: f32,
    pub color: layer_core::color::RgbColor,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BubbleRing {
    pub arc: RingArc,
    pub setting: &'static str,
    pub label: std::sync::Arc<str>,
    pub icon: &'static str,
    pub fill: f32,
    pub text: String,
    pub active: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BubbleBead {
    pub placement: BeadPlacement,
    pub color: Option<layer_core::color::RgbColor>,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChooserItem {
    pub kind: ChooserKind,
    pub center: [f32; 2],
    pub label: String,
    pub icon: &'static str,
    pub preview: Option<u32>,
    pub selected: bool,
    pub shown: bool,
    pub enabled: bool,
    pub tag: Bounds,
}
pub fn chooser_tag(center: [f32; 2], width: f32) -> Bounds {
    tag_bounds(center, width.min(CHOOSER_TAG[0]))
}

impl ChooserItem {
    pub fn size(&self) -> [f32; 2] {
        if self.kind == ChooserKind::Brush { BRUSH_PILL } else { [CHOOSER_ITEM; 2] }
    }
    pub fn contains(&self, point: [f32; 2]) -> bool {
        on_capsule([point[0] - self.center[0], point[1] - self.center[1]], self.size())
    }
    pub fn tag_center(&self, width: f32) -> [f32; 2] {
        let tag = chooser_tag(self.center, width);
        [tag.x + tag.width * 0.5, tag.y + tag.height * 0.5]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BubbleCard {
    pub size: [f32; 2],
    pub metrics: Option<(PanelScrollMeasurement, f32)>,
    pub full: bool,
    pub properties: bool,
    pub disclosure: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BubbleView {
    pub id: u32,
    pub source: BubbleSource,
    pub kind: BubbleKind,
    pub diameter: f32,
    pub face: [f32; 2],
    pub pair: PairBeads,
    pub color: Option<ColorBubbleLayout>,
    pub label: String,
    pub icon: &'static str,
    pub preview: Option<u32>,
    pub variants: bool,
    pub dots: Vec<bool>,
    pub selected: bool,
    pub enabled: bool,
    pub open: bool,
    pub halo: Option<f32>,
    pub rings: Vec<BubbleRing>,
    pub readout: Option<String>,
    pub stamp: Option<BubbleStamp>,
    pub buttons: Vec<BubbleButton>,
    pub hints: Vec<([f32; 2], f32)>,
    pub beads: Vec<BubbleBead>,
    pub pill: Option<String>,
    pub chooser: Vec<ChooserItem>,
    pub card: Option<BubbleCard>,
    pub layer: Option<u64>,
    pub layers: usize,
    pub options: BubbleOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BubbleHit {
    Card,
    Item(usize),
    Ring,
    Bead,
    Pill,
    Wheel,
    Intensity,
    Face,
    Half(usize),
}

pub fn count_badge(text: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    ([TOOL_BUBBLE * 0.5 - 4., -TOOL_BUBBLE * 0.5 + 4.], [(text[0] + 10.).max(text[1]), text[1]])
}

impl BubbleView {
    pub fn dot_centers(&self) -> Vec<[f32; 2]> {
        let count = self.dots.len().min(9);
        if count < 2 { return Vec::new(); }
        (0..count).map(|i| polar(19., 90. + ((count - 1) as f32 * 0.5 - i as f32) * 12.)).collect()
    }
    pub fn readout_center(&self) -> [f32; 2] {
        [0., self.halo.unwrap_or(self.diameter) * 0.5 + 16.]
    }
    pub fn palette_bounds(&self) -> Bounds {
        self.color.map_or_else(|| palette_pill(self.diameter), ColorBubbleLayout::palette_bounds)
    }
    pub fn reach(&self) -> f32 {
        let corner = |at: [f32; 2], size: [f32; 2]| (at[0].abs() + size[0] * 0.5).max(at[1].abs() + size[1] * 0.5);
        let beads = self.beads.iter().map(|b| corner(b.placement.center, [b.placement.size; 2]));
        let items = self.chooser.iter().map(|i| corner(i.center, i.size()) + if i.kind == ChooserKind::Brush { 0. } else { 60. });
        let pill = self.pill.as_ref().map(|_| {
            let b = self.palette_bounds();
            corner([0., b.y + b.height * 0.5], [b.width, b.height])
        });
        let card = self.card.as_ref().map(|c| corner([0.; 2], c.size));
        let halo = self.halo.map(|size| size * 0.5 + 40.);
        beads.chain(items).chain(pill).chain(card).chain(halo).fold(self.color.map_or(self.diameter * 0.5, |c| c.bounds().height - c.diameter * 0.5), f32::max)
    }
    pub fn hit(&self, point: [f32; 2]) -> Option<BubbleHit> {
        hit_geometry((self.kind, self.diameter, self.face), self.color, (self.card.as_ref().map(|card| card.size), self.pill.is_some()),
            self.chooser.iter().map(|item| (item.center, item.size())),
            self.rings.iter().filter(|ring| ring.enabled).map(|ring| ring.arc),
            self.beads.iter().map(|bead| bead.placement), point)
    }
}

#[derive(Deserialize)]
pub struct BubbleHitGeometry {
    kind: BubbleKind,
    diameter: f32,
    face: [f32; 2],
    #[serde(default)]
    color: Option<ColorBubbleLayout>,
    card: Option<HitCard>,
    chooser: Vec<HitItem>,
    rings: Vec<HitRing>,
    beads: Vec<HitBead>,
    pill: Option<String>,
}
#[derive(Deserialize)]
struct HitCard { size: [f32; 2] }
#[derive(Deserialize)]
struct HitItem { center: [f32; 2], kind: ChooserKind }
#[derive(Deserialize)]
struct HitRing { arc: RingArc, enabled: bool }
#[derive(Deserialize)]
struct HitBead { placement: BeadPlacement }

impl BubbleHitGeometry {
    pub fn hit(&self, point: [f32; 2]) -> Option<BubbleHit> {
        hit_geometry((self.kind, self.diameter, self.face), self.color, (self.card.as_ref().map(|card| card.size), self.pill.is_some()),
            self.chooser.iter().map(|item| (item.center, if item.kind == ChooserKind::Brush { BRUSH_PILL } else { [CHOOSER_ITEM; 2] })),
            self.rings.iter().filter(|ring| ring.enabled).map(|ring| ring.arc),
            self.beads.iter().map(|bead| bead.placement), point)
    }
}

fn hit_geometry((kind, diameter, face): (BubbleKind, f32, [f32; 2]), color: Option<ColorBubbleLayout>, (card, pill): (Option<[f32; 2]>, bool),
    mut items: impl Iterator<Item = ([f32; 2], [f32; 2])>, mut rings: impl Iterator<Item = RingArc>,
    mut beads: impl Iterator<Item = BeadPlacement>, point: [f32; 2]) -> Option<BubbleHit> {
    let within = |[w, h]: [f32; 2]| point[0].abs() <= w * 0.5 && point[1].abs() <= h * 0.5;
    if card.is_some_and(within) { return Some(BubbleHit::Card); }
    if let Some(index) = items.position(|(center, size)| on_capsule([point[0] - center[0], point[1] - center[1]], size)) {
        return Some(BubbleHit::Item(index));
    }
    if rings.any(|arc| arc.covers(point)) { return Some(BubbleHit::Ring); }
    if beads.any(|bead| bead_at(&[bead], point).is_some()) { return Some(BubbleHit::Bead); }
    let pill_bounds = color.map_or_else(|| palette_pill(diameter), ColorBubbleLayout::palette_bounds);
    if pill && pill_bounds.contains(point[0], point[1]) { return Some(BubbleHit::Pill); }
    let radius = point[0].hypot(point[1]);
    if let Some(color) = color {
        if color.intensity.is_some_and(|arc| arc.contains(point)) { return Some(BubbleHit::Intensity); }
        if radius <= color.wheel_size() * 0.5 { return Some(BubbleHit::Wheel); }
        return color.contains(point).then_some(BubbleHit::Face);
    }
    match kind {
        BubbleKind::Color if radius <= diameter * 0.5 - RING_RIM => Some(BubbleHit::Wheel),
        BubbleKind::Color => (radius <= diameter * 0.5).then_some(BubbleHit::Face),
        BubbleKind::History => history_button(point).map(BubbleHit::Half).or_else(|| on_capsule(point, face).then_some(BubbleHit::Face)),
        BubbleKind::Layers => within(face).then_some(BubbleHit::Face),
        _ => (radius <= face[0] * 0.5).then_some(BubbleHit::Face),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct BubblePlacement {
    pub id: u32,
    pub center: [f32; 2],
    pub lifted: bool,
    pub homing: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BubbleRingGeometry {
    pub start: [f32; 2],
    pub end: [f32; 2],
    pub thumb: [f32; 2],
    pub icon: [f32; 2],
}

#[derive(Clone, Debug, Serialize)]
pub struct BubbleColorPresentation {
    pub bounds: Bounds,
    pub outline: Vec<crate::color::ColorBubbleArc>,
    pub glass: [f32; 4],
    pub intensity: BubbleIntensityPresentation,
}

#[derive(Clone, Debug, Serialize)]
pub struct BubbleIntensityPresentation {
    pub geometry: crate::color::HdrIntensityArc,
    pub path: Vec<[f32; 2]>,
    pub point: [f32; 2],
    pub zero: [[f32; 2]; 2],
    pub range: [f32; 2],
    pub stops: f32,
    pub ramp: Vec<[f32; 4]>,
    pub marker: [f32; 4],
    pub label: String,
    pub text: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct BubblePresentation {
    pub color: Option<BubbleColorPresentation>,
    pub view: BubbleView,
    pub reach: f32,
    pub readout: [f32; 2],
    pub pill: Bounds,
    pub history: [[f32; 2]; 2],
    pub dots: Vec<[f32; 2]>,
    pub bead_colors: Vec<Option<[f32; 4]>>,
    pub stamp_color: Option<[f32; 4]>,
    pub rings: Vec<BubbleRingGeometry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BubbleMotion {
    pub moving: bool,
    pub placements: Vec<BubblePlacement>,
    pub fade: (bool, Vec<u32>),
}

#[cfg(test)]
#[path = "bubble_tests.rs"]
mod tests;
