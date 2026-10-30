//! Whole-drawing setups, their presets and the New drawing dialog's rules.
use crate::{DocumentBackground, FluentArgs, Localizer, MAX_NEW_DOCUMENT_RESOLUTION, MessageId, NewDocumentError, NewDocumentOptions, NewDocumentSettings};
use layer_core::{
    BlendSpace, Document, TextureSource,
    authored::PortableId,
    color::{DocumentColor, RgbColor, RgbSpace, SampleDepth},
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Px,
    In,
    Cm,
    Mm,
}
impl LengthUnit {
    pub const ALL: [Self; 4] = [Self::Mm, Self::Cm, Self::In, Self::Px];
    pub fn per_inch(self) -> Option<f64> {
        match self { Self::Px => None, Self::In => Some(1.), Self::Cm => Some(2.54), Self::Mm => Some(25.4) }
    }
    pub fn digits(self) -> u32 {
        match self { Self::Px => 0, Self::In => 3, Self::Cm => 2, Self::Mm => 1 }
    }
    pub fn step(self) -> f64 {
        match self { Self::Px => 10., Self::Mm => 1., Self::Cm => 0.1, Self::In => 0.125 }
    }
    pub fn snap(self) -> f64 {
        match self { Self::Px => 1., unit => unit.step() }
    }
    pub fn label(self, l: &Localizer) -> Arc<str> {
        l.text(match self {
            Self::Px => MessageId::DOCUMENTS_UNIT_PX,
            Self::In => MessageId::DOCUMENTS_UNIT_IN,
            Self::Cm => MessageId::DOCUMENTS_UNIT_CM,
            Self::Mm => MessageId::DOCUMENTS_UNIT_MM,
        })
    }
}

/// A length in thousandths of its unit, exact so setups compare equal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Measure(pub u32);
impl Measure {
    pub fn new(value: f64) -> Self { Self((value * 1000.).round().clamp(1., f64::from(u32::MAX)) as u32) }
    pub fn get(self) -> f64 { f64::from(self.0) / 1000. }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaperSetup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Arc<str>>,
    pub color: RgbColor,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transparent: bool,
    #[serde(default)]
    pub relief: u32,
}
impl PaperSetup {
    pub fn shown(&self) -> bool { self.texture.is_some() && !self.transparent }
}

/// A complete New drawing choice. `blend_space` is the blending the drawing
/// gets, so float depths always store linear light.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingSetup {
    pub unit: LengthUnit,
    pub width: Measure,
    pub height: Measure,
    pub resolution: u32,
    pub color: DocumentColor,
    pub blend_space: BlendSpace,
    pub paper: PaperSetup,
}
impl DrawingSetup {
    fn pixels(&self, length: Measure) -> u32 {
        let value = match self.unit.per_inch() {
            None => length.get(),
            Some(per) => length.get() / per * f64::from(self.resolution),
        };
        value.round().clamp(1., f64::from(u32::MAX)) as u32
    }
    pub fn extent(&self) -> [u32; 2] { [self.pixels(self.width), self.pixels(self.height)] }
    pub fn inches(&self) -> [f64; 2] {
        let inches = |length: Measure| self.unit.per_inch().map_or(length.get() / f64::from(self.resolution), |per| length.get() / per);
        [inches(self.width), inches(self.height)]
    }
    pub fn options(&self) -> NewDocumentOptions {
        NewDocumentOptions {
            extent: self.extent(),
            color: self.color,
            background: if self.paper.shown() { DocumentBackground::White } else { DocumentBackground::Transparent },
            blend_space: self.blend_space,
            resolution: self.resolution,
        }
    }
    pub fn validate(&self, limits: CanvasLimits) -> Result<(), SetupProblem> {
        if !(1..=MAX_NEW_DOCUMENT_RESOLUTION).contains(&self.resolution) { return Err(SetupProblem::Resolution); }
        let extent = self.extent();
        if extent.iter().any(|side| *side > limits.side) { return Err(SetupProblem::Side(limits.side)); }
        if Capacity::new(extent, self.color.depth, limits).layers == Some(0) { return Err(SetupProblem::Memory); }
        Ok(())
    }
    pub fn project(&self, id: PortableId, paper: TextureSource, limits: CanvasLimits, l: &Localizer) -> Result<Document, String> {
        self.validate(limits).map_err(|problem| problem.message(l))?;
        let mut document = self.options().document_on(id, paper, l);
        document.set_paper_color(self.paper.color);
        document.set_paper_relief(self.paper.relief as f32);
        if self.paper.texture.is_some() { return Ok(document); }
        document.without_paper().map_err(|error| error.to_string())
    }
    pub fn orientation(&self) -> Orientation {
        let [width, height] = self.extent();
        match width.cmp(&height) {
            std::cmp::Ordering::Equal => Orientation::Square,
            std::cmp::Ordering::Greater => Orientation::Landscape,
            std::cmp::Ordering::Less => Orientation::Portrait,
        }
    }
    fn turned(&self) -> Self {
        Self { width: self.height, height: self.width, ..self.clone() }
    }
    pub fn same_sheet(&self, other: &Self) -> bool { self == other || *self == other.turned() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupProblem {
    Resolution,
    Side(u32),
    Memory,
}
impl SetupProblem {
    pub fn message(self, l: &Localizer) -> String {
        match self {
            Self::Resolution => NewDocumentError::Resolution.message(l),
            Self::Side(maximum) => {
                let mut args = FluentArgs::new();
                args.set("maximum", group(u64::from(maximum)));
                l.format(MessageId::DOCUMENTS_TOO_LARGE_SIDE, &args)
            }
            Self::Memory => l.text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    Landscape,
    Square,
}

/// What this device can hold: the longest side the GPU accepts and the memory
/// left for layers, when the host knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasLimits {
    pub side: u32,
    pub memory: Option<u64>,
}
impl Default for CanvasLimits {
    fn default() -> Self { Self { side: layer_core::MAX_EXTENT, memory: None } }
}
impl CanvasLimits {
    pub fn new(side: u32, memory: Option<u64>) -> Self { Self { side: side.min(layer_core::MAX_EXTENT), memory } }
}

const OVERHEAD_BYTES_PER_PIXEL: u64 = 20;

fn bytes_per_pixel(depth: SampleDepth) -> u64 {
    match depth { SampleDepth::U8 => 4, SampleDepth::U16 | SampleDepth::F16 => 8, SampleDepth::F32 => 16 }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capacity {
    pub megapixels: f64,
    pub layer_bytes: u64,
    pub layers: Option<u64>,
    pub available: Option<u64>,
}
impl Capacity {
    pub fn new(extent: [u32; 2], depth: SampleDepth, limits: CanvasLimits) -> Self {
        let pixels = u64::from(extent[0]) * u64::from(extent[1]);
        let layer_bytes = pixels * bytes_per_pixel(depth);
        let layers = limits.memory.map(|memory| memory.saturating_sub(pixels * OVERHEAD_BYTES_PER_PIXEL) / layer_bytes.max(1));
        Self { megapixels: pixels as f64 / 1e6, layer_bytes, layers, available: limits.memory }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetGroup {
    Yours,
    Sketch,
    Watercolor,
    Pastel,
    Canvas,
    MixedMedia,
    Comics,
    Prints,
    Digital,
}
impl PresetGroup {
    pub const ALL: [Self; 9] = [Self::Yours, Self::Sketch, Self::Watercolor, Self::Pastel, Self::Canvas, Self::MixedMedia, Self::Comics, Self::Prints, Self::Digital];
    pub fn label(self, l: &Localizer) -> Arc<str> {
        l.text(match self {
            Self::Yours => MessageId::DOCUMENTS_YOUR_PRESETS,
            Self::Sketch => MessageId::DOCUMENTS_GROUP_SKETCH,
            Self::Watercolor => MessageId::DOCUMENTS_GROUP_WATERCOLOR,
            Self::Pastel => MessageId::DOCUMENTS_GROUP_PASTEL,
            Self::Canvas => MessageId::DOCUMENTS_GROUP_CANVAS,
            Self::MixedMedia => MessageId::DOCUMENTS_GROUP_MIXED_MEDIA,
            Self::Comics => MessageId::DOCUMENTS_GROUP_COMICS,
            Self::Prints => MessageId::DOCUMENTS_GROUP_PRINTS,
            Self::Digital => MessageId::DOCUMENTS_GROUP_DIGITAL,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum PresetKey {
    BuiltIn(String),
    Saved(String),
    Last,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedSetup {
    pub name: String,
    pub group: PresetGroup,
    pub setup: DrawingSetup,
}

pub struct BuiltInPreset {
    pub id: &'static str,
    pub name: MessageId,
    pub group: PresetGroup,
    pub setup: DrawingSetup,
}

const fn srgb(r: u8, g: u8, b: u8) -> RgbColor {
    RgbColor { space: RgbSpace::Srgb, rgba: [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.], linear_rgb: None }
}
const WHITE: RgbColor = srgb(0xff, 0xff, 0xff);
const NATURAL: RgbColor = srgb(0xf4, 0xf1, 0xe8);
const CREAM: RgbColor = srgb(0xf0, 0xe6, 0xce);
const OYSTER: RgbColor = srgb(0xd9, 0xc9, 0xa8);
const TONED_TAN: RgbColor = srgb(0xc9, 0xb7, 0x93);
const KRAFT: RgbColor = srgb(0xa9, 0x8d, 0x67);
const PEARL: RgbColor = srgb(0xd3, 0xd3, 0xd0);
const TONED_GRAY: RgbColor = srgb(0xa5, 0xa3, 0x9e);
const TONED_BLUE: RgbColor = srgb(0xa9, 0xb4, 0xc2);
const STEEL: RgbColor = srgb(0x5f, 0x5f, 0x5d);
const BLACK: RgbColor = srgb(0x1e, 0x1e, 0x1e);
const NEWSPRINT: RgbColor = srgb(0xe6, 0xe1, 0xd3);
const CHALKBOARD: RgbColor = srgb(0x2b, 0x32, 0x30);
const BLUEPRINT: RgbColor = srgb(0x1f, 0x4b, 0x7a);
const SAND: RgbColor = srgb(0xd6, 0xc1, 0x96);
const EARTH: RgbColor = srgb(0xb5, 0x9a, 0x74);
const LEAF_GREEN: RgbColor = srgb(0x7d, 0x9a, 0x5b);
const WEATHERED_WOOD: RgbColor = srgb(0x8d, 0x85, 0x74);
const CONCRETE: RgbColor = srgb(0x9b, 0x9a, 0x95);
const ASPHALT: RgbColor = srgb(0x3c, 0x3c, 0x3e);
pub const PAPER_COLORS: [(MessageId, RgbColor); 20] = [
    (MessageId::DOCUMENTS_WHITE, WHITE),
    (MessageId::DOCUMENTS_PAPER_NATURAL_WHITE, NATURAL),
    (MessageId::DOCUMENTS_PAPER_CREAM, CREAM),
    (MessageId::DOCUMENTS_PAPER_OYSTER, OYSTER),
    (MessageId::DOCUMENTS_PAPER_TONED_TAN, TONED_TAN),
    (MessageId::DOCUMENTS_PAPER_KRAFT, KRAFT),
    (MessageId::DOCUMENTS_PAPER_PEARL_GRAY, PEARL),
    (MessageId::DOCUMENTS_PAPER_TONED_GRAY, TONED_GRAY),
    (MessageId::DOCUMENTS_PAPER_TONED_BLUE, TONED_BLUE),
    (MessageId::DOCUMENTS_PAPER_STEEL_GRAY, STEEL),
    (MessageId::DOCUMENTS_PAPER_BLACK, BLACK),
    (MessageId::DOCUMENTS_PAPER_NEWSPRINT, NEWSPRINT),
    (MessageId::DOCUMENTS_PAPER_CHALKBOARD, CHALKBOARD),
    (MessageId::DOCUMENTS_PAPER_BLUEPRINT, BLUEPRINT),
    (MessageId::DOCUMENTS_PAPER_SAND, SAND),
    (MessageId::DOCUMENTS_PAPER_EARTH, EARTH),
    (MessageId::DOCUMENTS_PAPER_LEAF_GREEN, LEAF_GREEN),
    (MessageId::DOCUMENTS_PAPER_WEATHERED_WOOD, WEATHERED_WOOD),
    (MessageId::DOCUMENTS_PAPER_CONCRETE, CONCRETE),
    (MessageId::DOCUMENTS_PAPER_ASPHALT, ASPHALT),
];
pub const MAX_PAPER_COLORS: usize = 64;
pub const SCREEN_PPI: u32 = 144;
pub const THIS_SCREEN: &str = "this-screen";
pub const DEFAULT_PRESET: &str = "sketchbook-a4";
pub const MAX_RECENT: usize = 8;

#[allow(clippy::too_many_arguments)]
fn sheet(unit: LengthUnit, width: f64, height: f64, resolution: u32, space: RgbSpace, depth: SampleDepth, texture: &str, relief: u32, color: RgbColor, transparent: bool) -> DrawingSetup {
    DrawingSetup {
        unit,
        width: Measure::new(width),
        height: Measure::new(height),
        resolution,
        color: DocumentColor { space, depth },
        blend_space: BlendSpace::Perceptual.for_depth(depth),
        paper: PaperSetup { texture: Some(texture.into()), color, transparent, relief },
    }
}

pub fn built_in_presets() -> &'static [BuiltInPreset] {
    static PRESETS: OnceLock<Vec<BuiltInPreset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        use LengthUnit::{Cm, In, Mm, Px};
        use MessageId as M;
        use PresetGroup::*;
        use RgbSpace::{AdobeRgb as Adobe, DisplayP3 as P3, Srgb};
        use SampleDepth::{F16, U8, U16};
        let p = |id, name, group, setup| BuiltInPreset { id, name, group, setup };
        vec![
            p("pocket-book", M::DOCUMENTS_PRESET_POCKET_BOOK, Sketch, sheet(In, 3.5, 5.5, 300, Srgb, U8, "cold_press", 100, NATURAL, false)),
            p("sketchbook-a5", M::DOCUMENTS_PRESET_SKETCHBOOK_A5, Sketch, sheet(Mm, 210., 148., 300, Srgb, U8, "cold_press", 100, NATURAL, false)),
            p("sketchbook-a4", M::DOCUMENTS_PRESET_SKETCHBOOK_A4, Sketch, sheet(Mm, 297., 210., 300, Srgb, U8, "cold_press", 100, NATURAL, false)),
            p("sketch-pad", M::DOCUMENTS_PRESET_SKETCH_PAD, Sketch, sheet(In, 12., 9., 300, Srgb, U8, "cold_press", 100, WHITE, false)),
            p("square-book", M::DOCUMENTS_PRESET_SQUARE_BOOK, Sketch, sheet(In, 8., 8., 300, Srgb, U8, "cold_press", 100, CREAM, false)),
            p("toned-tan", M::DOCUMENTS_PRESET_TONED_TAN, Sketch, sheet(In, 12., 9., 300, Srgb, U8, "cold_press", 100, TONED_TAN, false)),
            p("toned-gray", M::DOCUMENTS_PRESET_TONED_GRAY, Sketch, sheet(In, 12., 9., 300, Srgb, U8, "cold_press", 100, TONED_GRAY, false)),
            p("toned-blue", M::DOCUMENTS_PRESET_TONED_BLUE, Sketch, sheet(In, 12., 9., 300, Srgb, U8, "cold_press", 100, TONED_BLUE, false)),
            p("kraft-paper", M::DOCUMENTS_PRESET_KRAFT_PAPER, Sketch, sheet(In, 11., 8.5, 300, Srgb, U8, "cold_press", 100, KRAFT, false)),
            p("black-book", M::DOCUMENTS_PRESET_BLACK_BOOK, Sketch, sheet(Mm, 210., 148., 300, Srgb, U8, "cold_press", 100, BLACK, false)),
            p("bristol-pad", M::DOCUMENTS_PRESET_BRISTOL_PAD, Sketch, sheet(In, 14., 11., 300, Srgb, U8, "bristol", 100, WHITE, false)),
            p("newsprint-pad", M::DOCUMENTS_PRESET_NEWSPRINT_PAD, Sketch, sheet(In, 24., 18., 300, Srgb, U8, "newsprint", 50, NEWSPRINT, false)),
            p("drafting-film", M::DOCUMENTS_PRESET_DRAFTING_FILM, Sketch, sheet(In, 12., 9., 300, Srgb, U8, "drafting_film", 100, WHITE, false)),
            p("trading-card", M::DOCUMENTS_PRESET_TRADING_CARD, Sketch, sheet(In, 2.5, 3.5, 300, Srgb, U8, "bristol", 100, CREAM, false)),
            p("postcard", M::DOCUMENTS_PRESET_POSTCARD, Watercolor, sheet(In, 6., 4., 300, Srgb, U16, "cold_press", 100, NATURAL, false)),
            p("bookmark", M::DOCUMENTS_PRESET_BOOKMARK, Watercolor, sheet(In, 2., 6., 300, Srgb, U16, "cold_press", 100, WHITE, false)),
            p("greeting-card", M::DOCUMENTS_PRESET_GREETING_CARD, Watercolor, sheet(In, 5., 7., 300, Srgb, U16, "cold_press", 100, CREAM, false)),
            p("paper-block", M::DOCUMENTS_PRESET_PAPER_BLOCK, Watercolor, sheet(In, 12., 9., 300, Srgb, U16, "cold_press", 100, NATURAL, false)),
            p("quarter-sheet", M::DOCUMENTS_PRESET_QUARTER_SHEET, Watercolor, sheet(In, 15., 11., 300, Srgb, U16, "cold_press", 100, NATURAL, false)),
            p("half-sheet", M::DOCUMENTS_PRESET_HALF_SHEET, Watercolor, sheet(In, 22., 15., 300, Srgb, U16, "cold_press", 100, NATURAL, false)),
            p("full-sheet", M::DOCUMENTS_PRESET_FULL_SHEET, Watercolor, sheet(In, 30., 22., 300, Srgb, U16, "rough", 150, NATURAL, false)),
            p("extra-rough", M::DOCUMENTS_PRESET_EXTRA_ROUGH, Watercolor, sheet(In, 22., 15., 300, Srgb, U16, "torchon", 150, NATURAL, false)),
            p("botanical", M::DOCUMENTS_PRESET_BOTANICAL, Watercolor, sheet(In, 11., 14., 300, Srgb, U16, "hot_press", 100, WHITE, false)),
            p("journal", M::DOCUMENTS_PRESET_JOURNAL, Watercolor, sheet(In, 6., 8., 300, Srgb, U16, "handmade_paper", 150, CREAM, false)),
            p("washi-sheet", M::DOCUMENTS_PRESET_WASHI_SHEET, Watercolor, sheet(In, 18., 12., 300, Srgb, U16, "washi", 100, NATURAL, false)),
            p("ink-scroll", M::DOCUMENTS_PRESET_INK_SCROLL, Watercolor, sheet(Cm, 30., 90., 300, Srgb, U16, "unryu", 100, CREAM, false)),
            p("pastel-pad", M::DOCUMENTS_PRESET_PASTEL_PAD, Pastel, sheet(In, 12., 9., 300, Srgb, U8, "pastel_paper", 100, TONED_GRAY, false)),
            p("pearl-gray", M::DOCUMENTS_PRESET_PEARL_GRAY, Pastel, sheet(In, 16., 12., 300, Srgb, U8, "pastel_paper", 100, PEARL, false)),
            p("pastel-sheet", M::DOCUMENTS_PRESET_PASTEL_SHEET, Pastel, sheet(In, 25.5, 19.5, 300, Srgb, U8, "pastel_paper", 100, TONED_TAN, false)),
            p("dark-gray", M::DOCUMENTS_PRESET_DARK_GRAY, Pastel, sheet(In, 16., 12., 300, Srgb, U8, "pastel_paper", 100, STEEL, false)),
            p("black-paper", M::DOCUMENTS_PRESET_BLACK_PAPER, Pastel, sheet(In, 12., 9., 300, Srgb, U8, "pastel_paper", 100, BLACK, false)),
            p("sanded-pastel", M::DOCUMENTS_PRESET_SANDED_PASTEL, Pastel, sheet(In, 18., 12., 300, Srgb, U8, "sanded_pastel", 100, OYSTER, false)),
            p("sanded-pastel-large", M::DOCUMENTS_PRESET_SANDED_PASTEL, Pastel, sheet(In, 24., 18., 300, Srgb, U8, "sanded_pastel", 100, TONED_GRAY, false)),
            p("pastel-board", M::DOCUMENTS_PRESET_PASTEL_BOARD, Pastel, sheet(In, 16., 12., 300, Srgb, U8, "coarse_sanded", 100, TONED_GRAY, false)),
            p("charcoal-pad", M::DOCUMENTS_PRESET_CHARCOAL_PAD, Pastel, sheet(In, 24., 18., 300, Srgb, U8, "ingres", 100, NATURAL, false)),
            p("chalkboard", M::DOCUMENTS_PRESET_CHALKBOARD, Pastel, sheet(In, 36., 24., 150, Srgb, U8, "chalkboard", 50, CHALKBOARD, false)),
            p("sidewalk", M::DOCUMENTS_PRESET_SIDEWALK, Pastel, sheet(In, 48., 36., 150, Srgb, U8, "asphalt", 150, ASPHALT, false)),
            p("blueprint", M::DOCUMENTS_PRESET_BLUEPRINT, Pastel, sheet(In, 24., 18., 150, Srgb, U8, "hot_press", 0, BLUEPRINT, false)),
            p("canvas-panel", M::DOCUMENTS_PRESET_CANVAS_PANEL, Canvas, sheet(In, 10., 8., 300, Srgb, U16, "canvas", 100, WHITE, false)),
            p("canvas-panel-large", M::DOCUMENTS_PRESET_CANVAS_PANEL, Canvas, sheet(In, 14., 11., 300, Srgb, U16, "canvas", 100, WHITE, false)),
            p("plein-air", M::DOCUMENTS_PRESET_PLEIN_AIR, Canvas, sheet(In, 12., 9., 300, Srgb, U16, "linen", 100, OYSTER, false)),
            p("toned-canvas", M::DOCUMENTS_PRESET_TONED_CANVAS, Canvas, sheet(In, 20., 16., 300, Srgb, U16, "canvas", 100, TONED_TAN, false)),
            p("canvas", M::DOCUMENTS_PRESET_CANVAS, Canvas, sheet(In, 24., 18., 300, Srgb, U16, "canvas", 100, WHITE, false)),
            p("gray-linen", M::DOCUMENTS_PRESET_GRAY_LINEN, Canvas, sheet(Cm, 40., 30., 300, Srgb, U16, "linen", 100, PEARL, false)),
            p("linen", M::DOCUMENTS_PRESET_LINEN, Canvas, sheet(In, 36., 24., 300, Srgb, U16, "linen", 100, WHITE, false)),
            p("gesso-panel", M::DOCUMENTS_PRESET_GESSO_PANEL, Canvas, sheet(In, 12., 9., 300, Srgb, U16, "gesso_panel", 100, WHITE, false)),
            p("gesso-across", M::DOCUMENTS_PRESET_GESSO_ACROSS, Canvas, sheet(In, 12., 9., 300, Srgb, U16, "gesso_across", 100, WHITE, false)),
            p("gesso-down", M::DOCUMENTS_PRESET_GESSO_DOWN, Canvas, sheet(In, 12., 9., 300, Srgb, U16, "gesso_down", 100, WHITE, false)),
            p("primed-board", M::DOCUMENTS_PRESET_PRIMED_BOARD, Canvas, sheet(In, 16., 12., 300, Srgb, U16, "brushed_gesso", 100, WHITE, false)),
            p("rough-across", M::DOCUMENTS_PRESET_ROUGH_ACROSS, Canvas, sheet(In, 16., 12., 300, Srgb, U16, "rough_gesso_across", 100, WHITE, false)),
            p("rough-down", M::DOCUMENTS_PRESET_ROUGH_DOWN, Canvas, sheet(In, 16., 12., 300, Srgb, U16, "rough_gesso_down", 100, WHITE, false)),
            p("rough-linen", M::DOCUMENTS_PRESET_ROUGH_LINEN, Canvas, sheet(In, 30., 24., 300, Srgb, U16, "rough_linen", 100, OYSTER, false)),
            p("canvas-pad", M::DOCUMENTS_PRESET_CANVAS_PAD, Canvas, sheet(In, 12., 9., 300, Srgb, U16, "canvas_paper", 100, WHITE, false)),
            p("barn-board", M::DOCUMENTS_PRESET_BARN_BOARD, MixedMedia, sheet(In, 24., 12., 300, Srgb, U16, "weathered_wood", 150, WEATHERED_WOOD, false)),
            p("concrete-wall", M::DOCUMENTS_PRESET_CONCRETE_WALL, MixedMedia, sheet(In, 48., 36., 150, Srgb, U8, "concrete", 100, CONCRETE, false)),
            p("cracked-earth", M::DOCUMENTS_PRESET_CRACKED_EARTH, MixedMedia, sheet(In, 16., 16., 300, Srgb, U16, "cracked_earth", 150, EARTH, false)),
            p("sand-ground", M::DOCUMENTS_PRESET_SAND_GROUND, MixedMedia, sheet(In, 16., 12., 300, Srgb, U16, "sand", 100, SAND, false)),
            p("papyrus", M::DOCUMENTS_PRESET_PAPYRUS, MixedMedia, sheet(Cm, 40., 30., 300, Srgb, U16, "papyrus", 100, SAND, false)),
            p("leaf", M::DOCUMENTS_PRESET_LEAF, MixedMedia, sheet(In, 6., 9., 300, Srgb, U16, "leaf", 100, LEAF_GREEN, false)),
            p("crumpled-page", M::DOCUMENTS_PRESET_CRUMPLED_PAGE, MixedMedia, sheet(Mm, 297., 210., 300, Srgb, U8, "crumpled_paper", 150, NATURAL, false)),
            p("burlap", M::DOCUMENTS_PRESET_BURLAP, MixedMedia, sheet(In, 20., 16., 300, Srgb, U16, "burlap", 100, KRAFT, false)),
            p("reed-mat", M::DOCUMENTS_PRESET_REED_MAT, MixedMedia, sheet(In, 24., 12., 300, Srgb, U16, "reeds_across", 100, TONED_TAN, false)),
            p("reed-screen", M::DOCUMENTS_PRESET_REED_SCREEN, MixedMedia, sheet(In, 12., 24., 300, Srgb, U16, "reeds_down", 100, TONED_TAN, false)),
            p("toilet-paper", M::DOCUMENTS_PRESET_TOILET_PAPER, MixedMedia, sheet(In, 4., 4.5, 300, Srgb, U8, "toilet", 150, WHITE, false)),
            p("manga-b5", M::DOCUMENTS_PRESET_MANGA_B5, Comics, sheet(Mm, 182., 257., 600, Srgb, U8, "bristol", 0, WHITE, false)),
            p("manga-b4", M::DOCUMENTS_PRESET_MANGA_B4, Comics, sheet(Mm, 257., 364., 350, Srgb, U8, "bristol", 0, WHITE, false)),
            p("us-comic", M::DOCUMENTS_PRESET_US_COMIC, Comics, sheet(In, 6.875, 10.5, 600, Srgb, U8, "bristol", 0, WHITE, false)),
            p("comic-board", M::DOCUMENTS_PRESET_COMIC_BOARD, Comics, sheet(In, 11., 17., 600, Srgb, U8, "bristol", 0, WHITE, false)),
            p("four-panel-strip", M::DOCUMENTS_PRESET_FOUR_PANEL_STRIP, Comics, sheet(In, 4., 13., 600, Srgb, U8, "bristol", 0, WHITE, false)),
            p("daily-strip", M::DOCUMENTS_PRESET_DAILY_STRIP, Comics, sheet(In, 13., 4., 600, Srgb, U8, "bristol", 0, WHITE, false)),
            p("zine-page", M::DOCUMENTS_PRESET_ZINE_PAGE, Comics, sheet(In, 5.5, 8.5, 300, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("picture-book", M::DOCUMENTS_PRESET_PICTURE_BOOK, Comics, sheet(In, 16., 10., 300, Srgb, U16, "hot_press", 100, NATURAL, false)),
            p("scroll-comic", M::DOCUMENTS_PRESET_SCROLL_COMIC, Comics, sheet(Px, 1600., 2560., SCREEN_PPI, Srgb, U8, "bristol", 0, WHITE, false)),
            p("art-print-5x7", M::DOCUMENTS_PRESET_ART_PRINT, Prints, sheet(In, 7., 5., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("art-print-8x10", M::DOCUMENTS_PRESET_ART_PRINT, Prints, sheet(In, 10., 8., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("art-print-11x14", M::DOCUMENTS_PRESET_ART_PRINT, Prints, sheet(In, 14., 11., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("art-print-16x20", M::DOCUMENTS_PRESET_ART_PRINT, Prints, sheet(In, 20., 16., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("poster", M::DOCUMENTS_PRESET_POSTER, Prints, sheet(In, 18., 24., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("a4", M::DOCUMENTS_PRESET_A4, Prints, sheet(Mm, 297., 210., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("a3", M::DOCUMENTS_PRESET_A3, Prints, sheet(Mm, 420., 297., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("letter", M::DOCUMENTS_PRESET_LETTER, Prints, sheet(In, 11., 8.5, 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("tabloid", M::DOCUMENTS_PRESET_TABLOID, Prints, sheet(In, 17., 11., 300, Adobe, U16, "hot_press", 0, WHITE, false)),
            p("sticker", M::DOCUMENTS_PRESET_STICKER, Prints, sheet(In, 3., 3., 300, Srgb, U8, "drawing_paper", 0, WHITE, true)),
            p(THIS_SCREEN, M::DOCUMENTS_PRESET_THIS_SCREEN, Digital, sheet(Px, 2560., 1600., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("full-hd", M::DOCUMENTS_PRESET_FULL_HD, Digital, sheet(Px, 1920., 1080., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("uhd", M::DOCUMENTS_PRESET_UHD, Digital, sheet(Px, 3840., 2160., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("uhd-hdr", M::DOCUMENTS_PRESET_UHD_HDR, Digital, sheet(Px, 3840., 2160., SCREEN_PPI, Srgb, F16, "drawing_paper", 0, WHITE, false)),
            p("cinematic", M::DOCUMENTS_PRESET_CINEMATIC, Digital, sheet(Px, 3840., 1608., SCREEN_PPI, P3, U16, "drawing_paper", 0, PEARL, false)),
            p("phone", M::DOCUMENTS_PRESET_PHONE, Digital, sheet(Px, 1290., 2796., SCREEN_PPI, P3, U8, "drawing_paper", 0, WHITE, false)),
            p("tablet", M::DOCUMENTS_PRESET_TABLET, Digital, sheet(Px, 2048., 2732., SCREEN_PPI, P3, U8, "drawing_paper", 0, WHITE, false)),
            p("square-post", M::DOCUMENTS_PRESET_SQUARE_POST, Digital, sheet(Px, 2160., 2160., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("portrait-post", M::DOCUMENTS_PRESET_PORTRAIT_POST, Digital, sheet(Px, 2160., 2700., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
            p("story", M::DOCUMENTS_PRESET_STORY, Digital, sheet(Px, 2160., 3840., SCREEN_PPI, Srgb, U8, "drawing_paper", 0, WHITE, false)),
        ]
    })
}

/// The current display, in device pixels and optional physical pixels per inch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenInfo {
    pub extent: [u32; 2],
    #[serde(default)]
    pub ppi: Option<u32>,
}

pub fn built_in(id: &str, screen: Option<ScreenInfo>) -> Option<DrawingSetup> {
    let mut setup = built_in_presets().iter().find(|preset| preset.id == id)?.setup.clone();
    if let (THIS_SCREEN, Some(screen)) = (id, screen) {
        setup.width = Measure::new(f64::from(screen.extent[0].max(1)));
        setup.height = Measure::new(f64::from(screen.extent[1].max(1)));
        setup.resolution = screen.ppi.unwrap_or(SCREEN_PPI).clamp(1, MAX_NEW_DOCUMENT_RESOLUTION);
    }
    Some(setup)
}

pub fn default_setup() -> DrawingSetup { built_in(DEFAULT_PRESET, None).expect("default preset") }

/// The sheet a new window opens with.
pub fn startup_setup(screen: Option<ScreenInfo>) -> DrawingSetup {
    let screen = screen.unwrap_or(ScreenInfo { extent: [1440, 1080], ppi: None });
    let height = screen.extent[1].max(1);
    let width = (f64::from(height) * 4. / 3.).round();
    let resolution = screen.ppi.unwrap_or(SCREEN_PPI).clamp(SCREEN_PPI, MAX_NEW_DOCUMENT_RESOLUTION);
    sheet(LengthUnit::Px, width, f64::from(height), resolution, RgbSpace::Srgb, SampleDepth::U8, "cold_press", 100, NATURAL, false)
}

pub fn startup_drawing(screen: Option<ScreenInfo>, l: &Localizer) -> Result<Document, String> {
    let setup = startup_setup(screen);
    let paper = setup.paper.texture.as_deref().and_then(layer_core::paper::built_in_paper).expect("startup paper").texture.clone();
    setup.project(PortableId::random(), paper, CanvasLimits::default(), l)
}

#[derive(Clone, Debug, PartialEq)]
pub enum NewDrawingEdit {
    Width(f64),
    Height(f64),
    Resolution(u32),
    Unit(LengthUnit),
    Orientation(Orientation),
    Linked(bool),
    Space(RgbSpace),
    Depth(SampleDepth),
    Blending(BlendSpace),
    Setup(DrawingSetup),
    Texture { texture: Option<Arc<str>>, relief: u32 },
    PaperColor { color: RgbColor, transparent: bool },
}

/// The dialog's working setup. Physical units keep the sheet size when the
/// resolution changes; pixels keep the pixel size.
#[derive(Clone, Debug, PartialEq)]
pub struct NewDrawingDraft {
    pub setup: DrawingSetup,
    pub linked: bool,
    print_unit: LengthUnit,
    blend_choice: BlendSpace,
}
impl NewDrawingDraft {
    pub fn new(setup: DrawingSetup) -> Self {
        let mut draft = Self { linked: false, print_unit: LengthUnit::Mm, blend_choice: BlendSpace::Perceptual, setup: setup.clone() };
        draft.apply(NewDrawingEdit::Setup(setup));
        draft
    }
    pub fn print_unit(&self) -> LengthUnit { self.print_unit }
    pub fn apply(&mut self, edit: NewDrawingEdit) {
        let setup = &mut self.setup;
        match edit {
            NewDrawingEdit::Width(value) => self.resize(true, value),
            NewDrawingEdit::Height(value) => self.resize(false, value),
            NewDrawingEdit::Resolution(value) => setup.resolution = value.clamp(1, MAX_NEW_DOCUMENT_RESOLUTION),
            NewDrawingEdit::Unit(unit) => {
                let inches = setup.inches();
                let convert = |inches: f64| match unit.per_inch() {
                    None => Measure::new((inches * f64::from(setup.resolution)).round().max(1.)),
                    Some(per) => Measure::new(round_to(inches * per, unit.digits())),
                };
                (setup.width, setup.height) = (convert(inches[0]), convert(inches[1]));
                setup.unit = unit;
                if unit != LengthUnit::Px { self.print_unit = unit; }
            }
            NewDrawingEdit::Orientation(orientation) => {
                let current = setup.orientation();
                if orientation != Orientation::Square && current != Orientation::Square && current != orientation { *setup = setup.turned(); }
            }
            NewDrawingEdit::Linked(linked) => self.linked = linked,
            NewDrawingEdit::Space(space) => setup.color.space = space,
            NewDrawingEdit::Depth(depth) => {
                setup.color.depth = depth;
                setup.blend_space = self.blend_choice.for_depth(depth);
            }
            NewDrawingEdit::Blending(blending) => if !setup.color.depth.is_float() {
                self.blend_choice = blending;
                setup.blend_space = blending;
            },
            NewDrawingEdit::Setup(next) => {
                if next.unit != LengthUnit::Px { self.print_unit = next.unit; }
                if !next.color.depth.is_float() { self.blend_choice = next.blend_space; }
                *setup = DrawingSetup { blend_space: next.blend_space.for_depth(next.color.depth), ..next };
            }
            NewDrawingEdit::Texture { texture, relief } => (setup.paper.texture, setup.paper.relief) = (texture, relief),
            NewDrawingEdit::PaperColor { color, transparent } => {
                setup.paper.color = RgbColor { rgba: [color.rgba[0], color.rgba[1], color.rgba[2], 1.], ..color };
                setup.paper.transparent = transparent;
            }
        }
    }
    fn resize(&mut self, width: bool, value: f64) {
        let setup = &mut self.setup;
        if !value.is_finite() || value <= 0. { return; }
        let pixels = setup.unit == LengthUnit::Px;
        let fit = |value: f64| Measure::new(if pixels { value.round().max(1.) } else { value });
        let (changed, other) = if width { (setup.width, setup.height) } else { (setup.height, setup.width) };
        let other = if self.linked { fit(other.get() * value / changed.get()) } else { other };
        let value = fit(value);
        if width { (setup.width, setup.height) = (value, other) } else { (setup.height, setup.width) = (value, other) }
    }
}

fn round_to(value: f64, digits: u32) -> f64 {
    let scale = 10f64.powi(digits as i32);
    (value * scale).round() / scale
}

fn trimmed(value: f64, digits: u32) -> String {
    let text = format!("{value:.*}", digits as usize);
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text }
}

pub fn format_length(value: f64, unit: LengthUnit) -> String {
    if unit == LengthUnit::Px { return group(value.round() as u64); }
    trimmed(value, unit.digits())
}

pub fn group(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, c) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) { out.push(','); }
        out.push(c);
    }
    out
}

pub fn length_control(unit: LengthUnit, limits: CanvasLimits) -> crate::NumericControl {
    let max = match unit.per_inch() {
        None => f64::from(limits.side),
        Some(per) => f64::from(limits.side) * per,
    };
    let min = if unit == LengthUnit::Px { 1. } else { 10f64.powi(-(unit.digits() as i32)) };
    crate::NumericControl { kind: crate::NumericKind::Number, scrub_snap: Some(unit.snap()), ..crate::NumericControl::number(min, max, unit.step(), unit.digits()) }
}

pub fn resolution_control() -> crate::NumericControl {
    crate::NumericControl { kind: crate::NumericKind::Number, ..crate::NumericControl::number(1., f64::from(MAX_NEW_DOCUMENT_RESOLUTION), 1., 0) }
}

pub fn relief_control() -> crate::NumericControl {
    let program = layer_core::bundled_effect_catalog().get(layer_core::paper::PAPER_PROGRAM).expect("paper program").program();
    let relief = program.parameters.iter().find(|p| p.key.as_ref() == layer_core::paper::RELIEF).expect("paper relief");
    crate::NumericControl { kind: crate::NumericKind::Number, unit: String::new(), ..crate::session::effects::number_control(relief).expect("numeric relief") }
}

pub const RESOLUTION_CHOICES: [(u32, MessageId); 5] = [
    (72, MessageId::DOCUMENTS_RESOLUTION_SCREEN),
    (150, MessageId::DOCUMENTS_RESOLUTION_DRAFT),
    (300, MessageId::DOCUMENTS_RESOLUTION_PRINT),
    (350, MessageId::DOCUMENTS_RESOLUTION_COLOR_COMICS),
    (600, MessageId::DOCUMENTS_RESOLUTION_LINE_ART),
];

pub fn resolution_text(resolution: u32, l: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("value", resolution);
    l.format(MessageId::DOCUMENTS_PPI, &args)
}

fn bytes(value: u64, l: &Localizer) -> String {
    let mut args = FluentArgs::new();
    let megabytes = value as f64 / 1_048_576.;
    if megabytes >= 1024. {
        args.set("value", format!("{:.1}", megabytes / 1024.));
        l.format(MessageId::DOCUMENTS_GIGABYTES, &args)
    } else {
        args.set("value", group(megabytes.round().max(1.) as u64));
        l.format(MessageId::DOCUMENTS_MEGABYTES, &args)
    }
}

pub fn paper_color_name(color: RgbColor, l: &Localizer) -> String {
    PAPER_COLORS.iter().find(|(_, named)| *named == color).map_or_else(|| hex(color), |(name, _)| l.text(*name).to_string())
}

pub fn hex(color: RgbColor) -> String {
    let rgb = color.encoded_in(RgbSpace::Srgb).unwrap_or(color.rgba).map(|v| (v.clamp(0., 1.) * 255.).round() as u8);
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// An imported paper as the library names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NamedPaper {
    pub id: Arc<str>,
    pub label: Arc<str>,
}

pub fn texture_name(texture: Option<&str>, imported: &[NamedPaper], l: &Localizer) -> Arc<str> {
    let Some(id) = texture else { return l.text(MessageId::DOCUMENTS_NO_PAPER) };
    layer_core::paper::built_in_paper(id).map(|paper| crate::session::effects::resource_label(&paper.label, l))
        .or_else(|| imported.iter().find(|paper| paper.id.as_ref() == id).map(|paper| paper.label.clone()))
        .unwrap_or_else(|| l.text(MessageId::RESOURCES_PAPER_OWN))
}

fn relief_text(relief: u32) -> String { format!("{relief} %") }

pub fn dimensions_text(setup: &DrawingSetup, l: &Localizer) -> String {
    let unit = setup.unit;
    format!("{} × {} {}", format_length(setup.width.get(), unit), format_length(setup.height.get(), unit), unit.label(l))
}

/// The main page's derived text: caption, size line, capacity and its tooltip.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewDrawingView {
    pub preset: String,
    pub orientation: Orientation,
    pub orientation_label: Arc<str>,
    pub size: String,
    pub problem: bool,
    pub capacity: String,
    pub memory: String,
    pub texture: Arc<str>,
    pub relief: Option<String>,
    pub paper_color: String,
    pub resolution: String,
}
impl NewDrawingView {
    pub fn new(draft: &NewDrawingDraft, catalog: &PresetCatalog, limits: CanvasLimits) -> Self {
        let l = catalog.localization;
        let setup = &draft.setup;
        let extent = setup.extent();
        let capacity = Capacity::new(extent, setup.color.depth, limits);
        let mut args = FluentArgs::new();
        args.set("megapixels", format!("{:.1}", capacity.megapixels));
        args.set("size", bytes(capacity.layer_bytes, l));
        let memory = match capacity.available {
            Some(available) => {
                args.set("available", bytes(available, l));
                l.format(MessageId::DOCUMENTS_LAYER_MEMORY_AVAILABLE, &args)
            }
            None => l.format(MessageId::DOCUMENTS_LAYER_MEMORY, &args),
        };
        let problem = setup.validate(limits).err();
        let capacity = match capacity.layers.filter(|_| problem.is_none()) {
            Some(layers) => {
                args.set("layers", layers);
                args.set("count", group(layers));
                l.format(MessageId::DOCUMENTS_CAPACITY, &args)
            }
            None => l.format(MessageId::DOCUMENTS_MEGAPIXELS, &args),
        };
        let size = match problem {
            Some(problem) => problem.message(l),
            None if setup.unit == LengthUnit::Px => {
                let unit = draft.print_unit();
                let per = unit.per_inch().unwrap_or(25.4);
                let digits = unit.digits().saturating_sub(1);
                let [width, height] = setup.inches();
                args.set("width", trimmed(width * per, digits));
                args.set("height", trimmed(height * per, digits));
                args.set("unit", unit.label(l).to_string());
                l.format(MessageId::DOCUMENTS_PRINTS_AT, &args)
            }
            None => {
                args.set("width", group(u64::from(extent[0])));
                args.set("height", group(u64::from(extent[1])));
                l.format(MessageId::DOCUMENTS_PIXEL_SIZE, &args)
            }
        };
        let orientation = setup.orientation();
        Self {
            preset: catalog.matching(setup, false).map_or_else(|| l.text(MessageId::DOCUMENTS_CUSTOM_PRESET).to_string(), |view| view.name),
            orientation,
            orientation_label: l.text(match orientation {
                Orientation::Portrait => MessageId::DOCUMENTS_PORTRAIT,
                Orientation::Landscape => MessageId::DOCUMENTS_LANDSCAPE,
                Orientation::Square => MessageId::DOCUMENTS_SQUARE,
            }),
            size,
            problem: problem.is_some(),
            capacity,
            memory,
            texture: texture_name(setup.paper.texture.as_deref(), catalog.imported, l),
            relief: setup.paper.texture.is_some().then(|| relief_text(setup.paper.relief)),
            paper_color: if setup.paper.texture.is_none() { l.text(MessageId::DOCUMENTS_PAPER_NONE).to_string() }
                else if setup.paper.transparent { l.text(MessageId::DOCUMENTS_TRANSPARENT).to_string() }
                else { paper_color_name(setup.paper.color, l) },
            resolution: resolution_text(setup.resolution, l),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetView {
    pub key: PresetKey,
    pub name: String,
    pub group: PresetGroup,
    pub setup: DrawingSetup,
    pub dimensions: String,
    pub details: String,
    pub own: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BrowserTab {
    Recent,
    Group(PresetGroup),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetSummary {
    pub name: String,
    pub size: String,
    pub details: String,
}

pub struct PresetCatalog<'a> {
    pub settings: &'a NewDocumentSettings,
    pub screen: Option<ScreenInfo>,
    pub imported: &'a [NamedPaper],
    pub localization: &'a Localizer,
}
impl PresetCatalog<'_> {
    fn view(&self, key: PresetKey, name: String, group: PresetGroup, setup: DrawingSetup, own: bool) -> PresetView {
        let l = self.localization;
        let mut args = FluentArgs::new();
        args.set("resolution", setup.resolution);
        args.set("paper", texture_name(setup.paper.texture.as_deref(), self.imported, l).to_string());
        let mut details = l.format(MessageId::DOCUMENTS_PRESET_TILE_DETAILS, &args);
        if setup.paper.transparent { details = format!("{details} · {}", l.text(MessageId::DOCUMENTS_TRANSPARENT)); }
        if setup.color.depth.is_float() { details = format!("{details} · {}", l.text(MessageId::DOCUMENTS_PRESET_HDR)); }
        PresetView { key, name, group, dimensions: dimensions_text(&setup, l), details, setup, own }
    }
    pub fn get(&self, key: &PresetKey) -> Option<PresetView> {
        let l = self.localization;
        match key {
            PresetKey::BuiltIn(id) => {
                let preset = built_in_presets().iter().find(|preset| preset.id == id)?;
                Some(self.view(key.clone(), l.text(preset.name).to_string(), preset.group, built_in(id, self.screen)?, false))
            }
            PresetKey::Saved(name) => {
                let saved = self.settings.setups.iter().find(|saved| saved.name == *name)?;
                Some(self.view(key.clone(), saved.name.clone(), saved.group, saved.setup.clone(), true))
            }
            PresetKey::Last => self.settings.last.clone()
                .map(|setup| self.view(PresetKey::Last, l.text(MessageId::DOCUMENTS_LAST_USED).to_string(), PresetGroup::Yours, setup, false)),
        }
    }
    fn all(&self) -> impl Iterator<Item = PresetView> + '_ {
        let saved = self.settings.setups.iter().map(|saved| PresetKey::Saved(saved.name.clone()));
        let built_in = built_in_presets().iter().map(|preset| PresetKey::BuiltIn(preset.id.into()));
        saved.chain(built_in).filter_map(|key| self.get(&key))
    }
    pub fn recent(&self) -> Vec<PresetView> {
        self.settings.recent.iter().filter_map(|key| self.get(key)).collect()
    }
    pub fn matching(&self, setup: &DrawingSetup, exact: bool) -> Option<PresetView> {
        let matches = |view: &PresetView| if exact { view.setup == *setup } else { view.setup.same_sheet(setup) };
        self.all().find(matches).or_else(|| self.get(&PresetKey::Last).filter(matches))
    }
    pub fn tabs(&self) -> Vec<(BrowserTab, Arc<str>)> {
        let l = self.localization;
        let recent = (!self.recent().is_empty()).then(|| (BrowserTab::Recent, l.text(MessageId::DOCUMENTS_RECENT_PRESETS)));
        let groups = PresetGroup::ALL.into_iter()
            .filter(|group| *group != PresetGroup::Yours || self.settings.setups.iter().any(|saved| saved.group == PresetGroup::Yours))
            .map(|group| (BrowserTab::Group(group), group.label(l)));
        recent.into_iter().chain(groups).collect()
    }
    pub fn default_tab(&self) -> BrowserTab {
        if self.recent().iter().any(|view| view.key != PresetKey::Last) { return BrowserTab::Recent; }
        if self.settings.setups.iter().any(|saved| saved.group == PresetGroup::Yours) { return BrowserTab::Group(PresetGroup::Yours); }
        BrowserTab::Group(PresetGroup::Sketch)
    }
    pub fn tiles(&self, tab: BrowserTab, query: &str) -> Vec<PresetView> {
        let query = query.trim().to_lowercase();
        if !query.is_empty() {
            return self.get(&PresetKey::Last).into_iter().chain(self.all())
                .filter(|view| format!("{} {} {}", view.name, view.dimensions, view.details).to_lowercase().contains(&query)).collect();
        }
        match tab {
            BrowserTab::Recent => self.recent(),
            BrowserTab::Group(group) => {
                let built_in = built_in_presets().iter().filter(|preset| preset.group == group).map(|preset| PresetKey::BuiltIn(preset.id.into()));
                let saved = self.settings.setups.iter().filter(|saved| saved.group == group).map(|saved| PresetKey::Saved(saved.name.clone()));
                built_in.chain(saved).filter_map(|key| self.get(&key)).collect()
            }
        }
    }
    pub fn no_match(&self, query: &str) -> String {
        let mut args = FluentArgs::new();
        args.set("query", query.trim().to_string());
        self.localization.format(MessageId::DOCUMENTS_NO_PRESETS_MATCH, &args)
    }
    pub fn summary(&self, name: &str, setup: &DrawingSetup) -> PresetSummary {
        let l = self.localization;
        let mut args = FluentArgs::new();
        args.set("size", dimensions_text(setup, l));
        args.set("resolution", setup.resolution);
        let size = l.format(MessageId::DOCUMENTS_PRESET_SIZE, &args);
        let paper = match &setup.paper.texture {
            None => l.text(MessageId::DOCUMENTS_NO_PAPER).to_string(),
            Some(texture) => {
                args.set("texture", format!("{} {}", texture_name(Some(texture), self.imported, l), relief_text(setup.paper.relief)));
                let color = if setup.paper.transparent { l.text(MessageId::DOCUMENTS_TRANSPARENT).to_string() } else { paper_color_name(setup.paper.color, l) };
                args.set("color", color);
                l.format(MessageId::DOCUMENTS_PRESET_PAPER, &args)
            }
        };
        args.set("color", format!("{} {}", setup.color.space.name(), crate::document_creation::depth_label(setup.color.depth, l)));
        args.set("paper", paper);
        PresetSummary { name: name.into(), size, details: l.format(MessageId::DOCUMENTS_PRESET_DETAILS, &args) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetNameCheck {
    Save,
    Replace,
    Rename,
    Invalid,
}
impl NewDocumentSettings {
    pub fn check_preset_name(&self, name: &str, renaming: Option<&str>) -> PresetNameCheck {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) { return PresetNameCheck::Invalid; }
        let taken = self.setups.iter().find(|saved| saved.name.to_lowercase() == name.to_lowercase());
        match renaming {
            Some(from) if taken.is_some_and(|saved| saved.name != from) => PresetNameCheck::Invalid,
            Some(_) => PresetNameCheck::Rename,
            None if taken.is_some() => PresetNameCheck::Replace,
            None => PresetNameCheck::Save,
        }
    }
    pub fn opening_setup(&self, screen: Option<ScreenInfo>) -> DrawingSetup {
        match self.recent.first() {
            Some(PresetKey::BuiltIn(id)) => built_in(id, screen),
            Some(PresetKey::Saved(name)) => self.setups.iter().find(|saved| saved.name == *name).map(|saved| saved.setup.clone()),
            Some(PresetKey::Last) => self.last.clone(),
            None => None,
        }.unwrap_or_else(default_setup)
    }
    pub fn paper_palette(&self) -> Vec<RgbColor> {
        self.paper_colors.clone().unwrap_or_else(|| PAPER_COLORS.iter().map(|(_, color)| *color).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewDocumentAction, UiLanguage};
    fn english() -> Arc<Localizer> { Localizer::shared(UiLanguage::English) }
    fn catalog<'a>(settings: &'a NewDocumentSettings, l: &'a Localizer) -> PresetCatalog<'a> {
        PresetCatalog { settings, screen: None, imported: &[], localization: l }
    }

    #[test]
    fn built_in_presets_are_unique_valid_and_use_every_built_in_paper() {
        let presets = built_in_presets();
        assert_eq!(presets.len(), 93);
        let mut ids: Vec<_> = presets.iter().map(|preset| preset.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), presets.len());
        for preset in presets {
            assert_eq!(preset.setup.validate(CanvasLimits::default()), Ok(()), "{}", preset.id);
            assert!(english().text(preset.name).chars().count() <= 13, "{}", preset.id);
            let texture = preset.setup.paper.texture.as_deref().unwrap();
            assert!(layer_core::paper::built_in_paper(texture).is_some(), "{}", preset.id);
        }
        for paper in layer_core::paper::built_in_papers() {
            assert!(presets.iter().any(|preset| preset.setup.paper.texture.as_deref() == Some(&*paper.id)), "{}", paper.id);
        }
        for (_, color) in PAPER_COLORS {
            assert!(presets.iter().any(|preset| preset.setup.paper.color == color), "{color:?}");
        }
        for preset in presets.iter().filter(|preset| preset.group == PresetGroup::Digital || preset.setup.unit == LengthUnit::Px) {
            assert_eq!(preset.setup.paper.relief, 0, "{} is a screen, not a sheet", preset.id);
        }
        assert!(presets.iter().all(|preset| preset.setup.paper.relief <= 300));
        assert_eq!(built_in("sketchbook-a4", None).unwrap().extent(), [3508, 2480]);
    }

    #[test]
    fn general_drawing_presets_are_landscape_and_format_orientations_are_retained() {
        for preset in built_in_presets() {
            let expected = match preset.id {
                "pocket-book" | "trading-card" | "bookmark" | "greeting-card" | "botanical"
                | "journal" | "ink-scroll" | "leaf" | "reed-screen" | "toilet-paper"
                | "manga-b5" | "manga-b4" | "us-comic" | "comic-board" | "four-panel-strip"
                | "zine-page" | "scroll-comic" | "poster" | "phone" | "tablet" | "portrait-post" | "story" => Orientation::Portrait,
                "square-book" | "cracked-earth" | "sticker" | "square-post" => Orientation::Square,
                _ => Orientation::Landscape,
            };
            assert_eq!(preset.setup.orientation(), expected, "{}", preset.id);
        }
        for extent in [[1800, 2880], [2880, 1800], [1800, 1800]] {
            let setup = built_in(THIS_SCREEN, Some(ScreenInfo { extent, ppi: Some(220) })).unwrap();
            assert_eq!(setup.extent(), extent);
        }
    }

    #[test]
    fn physical_sizes_keep_the_sheet_and_pixels_keep_the_pixels_when_resolution_changes() {
        let mut draft = NewDrawingDraft::new(default_setup());
        draft.apply(NewDrawingEdit::Resolution(600));
        assert_eq!(draft.setup.extent(), [7016, 4961]);
        assert_eq!(dimensions_text(&draft.setup, &english()), "297 × 210 mm");
        draft.apply(NewDrawingEdit::Unit(LengthUnit::Px));
        assert_eq!(dimensions_text(&draft.setup, &english()), "7,016 × 4,961 px");
        draft.apply(NewDrawingEdit::Resolution(300));
        assert_eq!(draft.setup.extent(), [7016, 4961]);
        let view = NewDrawingView::new(&draft, &catalog(&NewDocumentSettings::default(), &english()), CanvasLimits::default());
        assert_eq!(view.size, "Prints at 594 × 420 mm");
        draft.apply(NewDrawingEdit::Unit(LengthUnit::In));
        assert_eq!(dimensions_text(&draft.setup, &english()), "23.387 × 16.537 in");
        draft.apply(NewDrawingEdit::Unit(LengthUnit::Px));
        assert_eq!(draft.setup.extent(), [7016, 4961]);
    }

    #[test]
    fn dragging_lengths_lands_on_whole_pixels_and_millimetres_tenths_of_centimetres_and_eighths_of_inches() {
        use crate::NumericOperation;
        let dragged = |unit, origin: f64, pixels| length_control(unit, CanvasLimits::default()).resolve(origin, NumericOperation::Scrub { origin, pixels, fine: false }).unwrap();
        let drag = |unit, origin: f64, pixels| dragged(unit, origin, pixels).value;
        let typed = |unit, value| length_control(unit, CanvasLimits::default()).resolve(value, NumericOperation::Value { value }).unwrap().value;
        assert_eq!(drag(LengthUnit::Px, 2480., 3.), 2488.);
        assert_eq!(drag(LengthUnit::Mm, 210.4, 3.), 211.);
        assert!((drag(LengthUnit::Cm, 21.03, 3.) - 21.1).abs() < 1e-9);
        assert_eq!(drag(LengthUnit::In, 8.5, 5.), 8.625);
        let shown = [(LengthUnit::Px, 2480.), (LengthUnit::Mm, 210.4), (LengthUnit::Cm, 21.03), (LengthUnit::In, 8.5)].map(|(unit, origin)| dragged(unit, origin, 3.).text);
        assert_eq!(shown, ["2488", "211", "21.1", "8.625"]);
        assert_eq!(typed(LengthUnit::Mm, 210.4), 210.4);
        assert_eq!(typed(LengthUnit::In, 6.875), 6.875);
    }

    #[test]
    fn linked_sides_keep_the_ratio_and_orientation_turns_the_sheet() {
        let mut draft = NewDrawingDraft::new(built_in("sketch-pad", None).unwrap());
        draft.apply(NewDrawingEdit::Orientation(Orientation::Portrait));
        draft.apply(NewDrawingEdit::Width(18.));
        assert_eq!(dimensions_text(&draft.setup, &english()), "18 × 12 in");
        draft.apply(NewDrawingEdit::Linked(true));
        draft.apply(NewDrawingEdit::Height(24.));
        assert_eq!(dimensions_text(&draft.setup, &english()), "36 × 24 in");
        assert_eq!(draft.setup.orientation(), Orientation::Landscape);
        draft.apply(NewDrawingEdit::Orientation(Orientation::Portrait));
        assert_eq!(dimensions_text(&draft.setup, &english()), "24 × 36 in");
        draft.apply(NewDrawingEdit::Orientation(Orientation::Portrait));
        assert_eq!(dimensions_text(&draft.setup, &english()), "24 × 36 in");
        draft.apply(NewDrawingEdit::Unit(LengthUnit::Px));
        draft.apply(NewDrawingEdit::Width(1001.));
        assert_eq!(draft.setup.extent(), [1001, 1502]);
        draft.apply(NewDrawingEdit::Width(f64::NAN));
        draft.apply(NewDrawingEdit::Height(0.));
        assert_eq!(draft.setup.extent(), [1001, 1502]);
    }

    #[test]
    fn float_depths_blend_linearly_and_return_to_the_chosen_blending() {
        let mut draft = NewDrawingDraft::new(default_setup());
        draft.apply(NewDrawingEdit::Blending(BlendSpace::Linear));
        draft.apply(NewDrawingEdit::Blending(BlendSpace::Perceptual));
        draft.apply(NewDrawingEdit::Depth(SampleDepth::F16));
        assert_eq!(draft.setup.blend_space, BlendSpace::Linear);
        draft.apply(NewDrawingEdit::Blending(BlendSpace::Perceptual));
        assert_eq!(draft.setup.blend_space, BlendSpace::Linear);
        draft.apply(NewDrawingEdit::Depth(SampleDepth::U16));
        assert_eq!(draft.setup.blend_space, BlendSpace::Perceptual);
        draft.apply(NewDrawingEdit::Setup(built_in("uhd-hdr", None).unwrap()));
        draft.apply(NewDrawingEdit::Depth(SampleDepth::U8));
        assert_eq!(draft.setup.blend_space, BlendSpace::Perceptual);
    }

    #[test]
    fn limits_report_capacity_and_refuse_sheets_the_device_cannot_hold() {
        let l = english();
        let settings = NewDocumentSettings::default();
        let limits = CanvasLimits::new(16384, Some(6 * 1024 * 1024 * 1024));
        let draft = NewDrawingDraft::new(default_setup());
        let view = NewDrawingView::new(&draft, &catalog(&settings, &l), limits);
        assert_eq!(view.preset, "Sketchbook A4");
        assert_eq!(view.capacity, "8.7 MP · up to 180 layers");
        let roomy = NewDrawingView::new(&draft, &catalog(&settings, &l), CanvasLimits::new(16384, Some(400 * 1024 * 1024 * 1024)));
        assert_eq!(roomy.capacity, "8.7 MP · up to 12,337 layers");
        assert_eq!(view.memory, "33 MB per layer · 6.0 GB available");
        assert_eq!(view.size, "3,508 × 2,480 px");
        assert!(!view.problem);
        let mut wide = draft.clone();
        wide.apply(NewDrawingEdit::Width(1500.));
        let view = NewDrawingView::new(&wide, &catalog(&settings, &l), limits);
        assert!(view.problem);
        assert_eq!(view.size, "Too large for this device: sides stop at 16,384 px");
        assert_eq!(view.preset, "Custom");
        let mut heavy = draft.clone();
        heavy.apply(NewDrawingEdit::Depth(SampleDepth::F32));
        let tight = CanvasLimits::new(16384, Some(200 * 1024 * 1024));
        assert_eq!(heavy.setup.validate(tight), Err(SetupProblem::Memory));
        assert_eq!(heavy.setup.project(PortableId::random(), layer_core::paper::default_texture(), tight, &l).unwrap_err(), "Too large for this device’s available memory");
    }

    #[test]
    fn created_drawings_get_their_paper_color_texture_or_no_paper() {
        let l = english();
        let mut setup = built_in("kraft-paper", None).unwrap();
        let bristol = layer_core::paper::built_in_paper("bristol").unwrap().texture.clone();
        let project = setup.project(PortableId::random(), bristol.clone(), CanvasLimits::default(), &l).unwrap();
        let paper = project.bottom_paper().unwrap();
        assert_eq!(project.scene().effect(paper).unwrap().constant_color(), Some(KRAFT));
        assert_eq!(layer_core::paper::light(project.scene().effect(paper).unwrap()).map(|light| light.relief), Some(1.));
        assert_eq!(project.paper(None).texture, bristol);
        assert!(project.scene().occurrence(paper).unwrap().visible);
        assert_eq!(project.composition().resolution, Some(layer_core::ImageResolution::ppi(300)));
        setup.paper.transparent = true;
        let project = setup.project(PortableId::random(), layer_core::paper::default_texture(), CanvasLimits::default(), &l).unwrap();
        assert!(!project.scene().occurrence(project.bottom_paper().unwrap()).unwrap().visible);
        setup.paper.texture = None;
        let project = setup.project(PortableId::random(), layer_core::paper::default_texture(), CanvasLimits::default(), &l).unwrap();
        assert_eq!(project.bottom_paper(), None);
        assert_eq!(project.scene().order().len(), 1);
        assert_eq!(project.revision, 0);
        assert!(project.working.target.is_some());
    }

    #[test]
    fn recents_remember_presets_or_the_last_custom_setup_and_choose_the_opening_tab() {
        let l = english();
        let mut settings = NewDocumentSettings::default();
        let tabs = |settings: &NewDocumentSettings| catalog(settings, &l).tabs().into_iter().map(|(_, label)| label.to_string()).collect::<Vec<_>>();
        let created = |settings: &mut NewDocumentSettings, setup: DrawingSetup| {
            let preset = catalog(settings, &l).matching(&setup, true).map(|view| view.key).filter(|key| *key != PresetKey::Last);
            settings.apply(NewDocumentAction::Created { setup, preset }).unwrap();
        };
        assert!(catalog(&settings, &l).recent().is_empty());
        assert_eq!(catalog(&settings, &l).default_tab(), BrowserTab::Group(PresetGroup::Sketch));
        assert_eq!(tabs(&settings)[0], "Sketch");
        let mut custom = default_setup();
        custom.resolution = 450;
        created(&mut settings, custom.clone());
        assert_eq!(settings.recent, [PresetKey::Last]);
        assert_eq!(settings.opening_setup(None), custom);
        assert_eq!(catalog(&settings, &l).default_tab(), BrowserTab::Group(PresetGroup::Sketch));
        assert_eq!(tabs(&settings)[0], "Recent");
        created(&mut settings, built_in("chalkboard", None).unwrap());
        assert_eq!(settings.recent, [PresetKey::BuiltIn("chalkboard".into()), PresetKey::Last]);
        assert_eq!(catalog(&settings, &l).default_tab(), BrowserTab::Recent);
        assert_eq!(catalog(&settings, &l).recent().iter().map(|view| view.name.as_str()).collect::<Vec<_>>(), ["Chalkboard", "Last used"]);
        created(&mut settings, custom);
        assert_eq!(settings.recent, [PresetKey::Last, PresetKey::BuiltIn("chalkboard".into())]);
        let read: NewDocumentSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(read, settings);
    }

    #[test]
    fn saved_presets_replace_rename_delete_and_follow_their_category() {
        let l = english();
        let mut settings = NewDocumentSettings::default();
        let setup = built_in("toned-blue", None).unwrap();
        assert_eq!(settings.check_preset_name(" Studio ", None), PresetNameCheck::Save);
        settings.apply(NewDocumentAction::SaveSetup { name: " Studio ".into(), group: PresetGroup::Yours, setup: setup.clone() }).unwrap();
        assert_eq!(settings.check_preset_name("studio", None), PresetNameCheck::Replace);
        assert_eq!(settings.check_preset_name("", None), PresetNameCheck::Invalid);
        assert_eq!(catalog(&settings, &l).default_tab(), BrowserTab::Group(PresetGroup::Yours));
        assert_eq!(catalog(&settings, &l).tabs()[0].1.as_ref(), "Your presets");
        let preset = catalog(&settings, &l).matching(&setup, true).map(|view| view.key);
        assert_eq!(preset, Some(PresetKey::Saved("Studio".into())));
        settings.apply(NewDocumentAction::Created { setup, preset }).unwrap();
        assert_eq!(settings.recent, [PresetKey::Saved("Studio".into())]);
        settings.apply(NewDocumentAction::SaveSetup { name: "STUDIO".into(), group: PresetGroup::Yours, setup: default_setup() }).unwrap();
        assert_eq!(settings.setups.len(), 1);
        assert!(settings.recent.is_empty());
        settings.apply(NewDocumentAction::RenameSetup { from: "STUDIO".into(), to: "Comics desk".into(), group: PresetGroup::Comics }).unwrap();
        let tiles = catalog(&settings, &l).tiles(BrowserTab::Group(PresetGroup::Comics), "");
        assert_eq!(tiles.last().map(|view| (view.name.as_str(), view.own)), Some(("Comics desk", true)));
        assert_eq!(catalog(&settings, &l).tabs()[0].1.as_ref(), "Sketch");
        assert_eq!(settings.check_preset_name("comics DESK", Some("Comics desk")), PresetNameCheck::Rename);
        settings.apply(NewDocumentAction::DeleteSetup { name: "Comics desk".into() }).unwrap();
        assert!(settings.setups.is_empty());
        assert!(settings.apply(NewDocumentAction::DeleteSetup { name: "Comics desk".into() }).is_err());
    }

    #[test]
    fn startup_sheet_uses_screen_height_and_density_with_cold_press_paper() {
        for (screen, extent, ppi) in [
            (None, [1440, 1080], 144),
            (Some(ScreenInfo { extent: [2880, 1800], ppi: Some(220) }), [2400, 1800], 220),
            (Some(ScreenInfo { extent: [1920, 1080], ppi: Some(96) }), [1440, 1080], 144),
            (Some(ScreenInfo { extent: [1080, 1920], ppi: None }), [2560, 1920], 144),
            (Some(ScreenInfo { extent: [1920, 1081], ppi: Some(0) }), [1441, 1081], 144),
        ] {
            let opened = startup_drawing(screen, &english()).unwrap();
            assert_eq!((opened.composition().size, opened.composition().resolution), (extent, Some(layer_core::ImageResolution::ppi(ppi))));
            assert_eq!(opened.composition().color, DocumentColor::default());
            let paper = opened.bottom_paper().unwrap();
            assert!(opened.scene().occurrence(paper).unwrap().visible);
            assert_eq!(opened.scene().effect(paper).unwrap().constant_color(), Some(NATURAL));
            let surface = opened.paper(None);
            assert_eq!(surface.texture, layer_core::paper::built_in_paper("cold_press").unwrap().texture);
            assert_eq!(layer_core::paper::light(opened.scene().effect(paper).unwrap()).unwrap().relief, 1.);
        }
    }

    #[test]
    fn browser_search_summary_and_this_screen_follow_the_catalog() {
        let l = english();
        let settings = NewDocumentSettings::default();
        let catalog = PresetCatalog { screen: Some(ScreenInfo { extent: [2880, 1800], ppi: Some(220) }), ..catalog(&settings, &l) };
        let found = catalog.tiles(BrowserTab::Recent, "  CHALK ");
        assert_eq!(found.iter().map(|view| view.name.as_str()).collect::<Vec<_>>(), ["Chalkboard"]);
        assert_eq!(found[0].details, "150 ppi · Chalkboard");
        assert!(catalog.tiles(BrowserTab::Recent, "zzz").is_empty());
        assert_eq!(catalog.no_match("zzz"), "No presets match “zzz”.");
        let screen = catalog.get(&PresetKey::BuiltIn(THIS_SCREEN.into())).unwrap();
        assert_eq!((screen.setup.extent(), screen.setup.resolution), ([2880, 1800], 220));
        let summary = catalog.summary("Kraft paper", &built_in("kraft-paper", None).unwrap());
        assert_eq!((summary.size.as_str(), summary.details.as_str()), ("11 × 8.5 in · 300 ppi", "sRGB 8-bit SDR · Cold press 100 %, Kraft"));
        assert_eq!(catalog.get(&PresetKey::BuiltIn("uhd-hdr".into())).unwrap().details, "144 ppi · Fine tooth · HDR");
        assert_eq!(catalog.tiles(BrowserTab::Group(PresetGroup::Digital), "").len(), 10);
    }
}
