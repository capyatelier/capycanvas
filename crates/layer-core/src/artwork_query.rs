use crate::{Document, Layer, LayerId, LayerKind};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const ARTWORK_SAMPLE_WIDTHS: [u32; 5] = [1, 5, 15, 51, 101];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArtworkSource {
    Visible,
    LayerContent(LayerId),
    Reference,
    EffectInput(LayerId),
    EffectChannels(LayerId),
    EffectBaseline(Box<Layer>),
}

#[derive(Clone, Debug)]
pub struct ArtworkQuery {
    pub document: Arc<Document>,
    pub source: ArtworkSource,
    pub time: f32,
    pub effect_times: Vec<(LayerId, f32)>,
}

#[derive(Clone, Debug)]
pub struct ArtworkSampleRequest {
    pub query: ArtworkQuery,
    pub position: [f32; 2],
    pub width: u32,
}
impl std::ops::Deref for ArtworkSampleRequest {
    type Target = ArtworkQuery;
    fn deref(&self) -> &ArtworkQuery { &self.query }
}
impl std::ops::DerefMut for ArtworkSampleRequest {
    fn deref_mut(&mut self) -> &mut ArtworkQuery { &mut self.query }
}

#[derive(Clone, Debug)]
pub struct ArtworkStatisticsRequest {
    pub query: ArtworkQuery,
    pub preview: bool,
    pub selection: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArtworkSample {
    Color([f32; 4]),
    Empty,
    Outside,
}

impl ArtworkSampleRequest {
    pub fn new(document: &Document, source: ArtworkSource, position: [f32; 2], width: u32) -> Self {
        Self { query: ArtworkQuery::new(document, source), position, width }
    }
    pub fn validate(&self) -> Result<(), String> {
        if !ARTWORK_SAMPLE_WIDTHS.contains(&self.width) || !self.position.into_iter().all(f32::is_finite) {
            return Err("Invalid artwork sample".into());
        }
        self.query.validate()
    }
}

impl ArtworkQuery {
    pub fn new(document: &Document, source: ArtworkSource) -> Self {
        let mut document = document.clone();
        document.layers = document.layers.iter().map(|layer| {
            let mut snapshot = layer.composite_snapshot();
            snapshot.pending_operations = layer.pending_operations.clone();
            snapshot
        }).collect();
        Self { document: Arc::new(document), source, time: 0., effect_times: Vec::new() }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.time.is_finite() || self.effect_times.iter().any(|(_, time)| !time.is_finite())
            || self.document.width > 32768 || self.document.height > 32768
        { return Err("Invalid artwork sample".into()); }
        if self.document.layers.iter().any(|layer| !layer.pending_operations.is_empty()
            || layer.masks().any(|mask| !mask.pending_operations.is_empty())) {
            return Err("Wait for the current edit before sampling".into());
        }
        match &self.source {
            ArtworkSource::LayerContent(id) if self.document.layer(*id).is_none_or(|layer| layer.kind != LayerKind::Paint) =>
                Err("This layer has no color content to sample".into()),
            ArtworkSource::EffectChannels(id) if self.document.layer(*id).and_then(|layer|layer.effect.as_ref()).is_none_or(|effect|
                !matches!(effect.program.id.as_ref(),"curves"|"levels") || effect.program.kind!=crate::EffectKind::Adjustment) =>
                Err("This adjustment has no channel statistics".into()),
            ArtworkSource::EffectInput(id) if self.document.layer(*id).and_then(|layer| layer.effect.as_ref())
                .is_none_or(|effect| effect.program.kind != crate::EffectKind::Adjustment) =>
                Err("The adjustment to sample was removed".into()),
            ArtworkSource::EffectBaseline(original) if self.document.layer(original.id).is_none()
                || original.kind != LayerKind::Effect => Err("The adjustment to compare was removed".into()),
            _ => Ok(()),
        }
    }

    pub fn matches_artwork(&self, document: &Document) -> bool {self.matches(document,false,true)}

    pub fn matches_source(&self, document:&Document)->bool {self.matches(document,true,true)}

    pub fn matches_source_identity(&self, document:&Document)->bool {self.matches(document,true,false)}

    fn matches(&self, document:&Document,source_only:bool,values:bool)->bool {
        let old = &self.document;
        old.id == document.id && old.color == document.color && old.width == document.width
            && old.height == document.height && old.blend_space == document.blend_space
            && (!matches!(self.source, ArtworkSource::Reference) || old.reference_layers == document.reference_layers)
            && old.layers.len() == document.layers.len()
            && old.layers.iter().zip(&document.layers).all(|(a,b)| {
                if !source_only {return a.same_artwork(b);}
                let channels=match self.source {
                    ArtworkSource::EffectInput(id) if id==a.id=>false,
                    ArtworkSource::EffectChannels(id) if id==a.id=>true,
                    _=>return a.same_backing(b) && if values {a.effect==b.effect}
                        else {a.effect.as_ref().map(|e|&e.program)==b.effect.as_ref().map(|e|&e.program)},
                };
                a.id==b.id && a.kind==b.kind && a.properties.parent==b.properties.parent && a.properties.clipped==b.properties.clipped
                    && match (&a.effect,&b.effect) {
                    (Some(a),Some(b)) if a.program==b.program=>!values || !channels || a.program.parameters.iter().zip(a.values.iter().zip(&b.values))
                        .all(|(parameter,(a,b))| parameter.page.as_deref()==Some("rgb") || a==b),
                    _=>false,
                }
            })
    }
}

impl Layer {
    pub fn same_artwork(&self, other: &Self) -> bool {self.same_backing(other) && self.effect==other.effect}

    fn same_backing(&self,other:&Self)->bool {
        self.id == other.id && self.kind == other.kind && self.visible == other.visible
            && self.opacity == other.opacity && self.raster == other.raster
            && self.properties == other.properties && self.mask == other.mask
            && self.pending_operations == other.pending_operations
            && match (&self.source, &other.source) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

pub fn white_balance_neutral(rgb: [f32; 3], space: crate::color::RgbSpace, preserve_luminance: bool) -> Result<[f32; 2], &'static str> {
    if rgb.iter().any(|v| !v.is_finite() || *v <= 0.) { return Err("Choose a point with positive red, green and blue values"); }
    let [r, g, b] = rgb.map(|v| f64::from(v).log2());
    let values = [62.5 * (b-r), (100. / 1.5) * (2.*g-r-b)];
    if values[0].abs() > 1000. || values[1].abs() > 800. { return Err("This color is outside the White Balance range"); }
    let values = values.map(|v| v as f32);
    let [t, q] = values.map(|v| v / 100.);
    let gain = [0.8*t+0.25*q, -0.5*q, -0.8*t+0.25*q].map(f32::exp2);
    let mut output = std::array::from_fn::<_, 3, _>(|i| rgb[i]*gain[i]);
    if preserve_luminance {
        let weights = space.to_xyz()[1].map(|v| v as f32);
        let luma = |c: [f32; 3]| c[0]*weights[0]+c[1]*weights[1]+c[2]*weights[2];
        let corrected = luma(output);
        if corrected != 0. { output = output.map(|v| v*(luma(rgb)/corrected)); }
    }
    let low = output.into_iter().fold(f32::INFINITY, f32::min);
    let high = output.into_iter().fold(f32::NEG_INFINITY, f32::max);
    if !output.into_iter().all(f32::is_finite) || high-low > 2e-6f32.max(2e-4*high.abs().max(low.abs())) {
        return Err("This color cannot be made neutral with White Balance");
    }
    Ok(values)
}
