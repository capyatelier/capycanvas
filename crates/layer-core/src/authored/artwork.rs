use super::{Handle, PortableId, Store};
use crate::{BlendSpace, EffectProgram, EffectValue, ImageResolution, LayerBlend, LayerPlacement,
    PhotoMetadata, Point, Projective, RulerGeometry, Selection, SelectionMaskProperties,
    color::{DocumentColor, ProofRecipe, hdr::SdrRendition, source::SourceImage}, raster::RasterRevision};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

pub type CompositionHandle = Handle<Composition>;
pub type StackHandle = Handle<Stack>;
pub type OccurrenceHandle = Handle<Occurrence>;
pub type PaintHandle = Handle<PaintSource>;
pub type CoverageHandle = Handle<CoverageSource>;
pub type EffectHandle = Handle<EffectApplication>;
pub type DefinitionHandle = Handle<Definition>;
pub type SelectionHandle = Handle<SavedSelection>;
pub type OutputHandle = Handle<Output>;

#[derive(Clone, Debug, PartialEq)]
pub struct Artwork {
    pub id: PortableId,
    pub root: CompositionHandle,
    pub compositions: Store<Composition>,
    pub stacks: Store<Stack>,
    pub occurrences: Store<Occurrence>,
    pub paint: Store<PaintSource>,
    pub coverage: Store<CoverageSource>,
    pub effects: Store<EffectApplication>,
    pub definitions: Store<Definition>,
    pub selections: Store<SavedSelection>,
    pub guides: Store<Guides>,
    pub outputs: Store<Output>,
    pub default_output: OutputHandle,
    pub metadata: Arc<PhotoMetadata>,
    pub extensions: Arc<super::Extensions>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    pub size: [u32; 2],
    pub origin: Point,
    pub color: DocumentColor,
    pub blend: BlendSpace,
    pub resolution: Option<ImageResolution>,
    pub result: StackHandle,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack { pub entries: Vec<OccurrenceHandle> }
#[derive(Clone, Debug, PartialEq)]
pub enum OccurrenceContent {
    Paint(PaintHandle), Stack(StackHandle), Effect(EffectHandle), Selection(SelectionHandle),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Attachment { #[default] None, Clip, Effect }
impl Attachment {
    pub fn is_clip(self) -> bool { self == Self::Clip }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub content: OccurrenceContent,
    pub name: Arc<str>,
    pub visible: bool,
    pub opacity: f32,
    pub blend: LayerBlend,
    pub locked: bool,
    pub alpha_locked: bool,
    pub reference: bool,
    pub attachment: Attachment,
    pub isolated_blend: LayerBlend,
    pub translation: Point,
    pub placement: LayerPlacement,
    pub mask: Option<MaskUse>,
}
impl Occurrence {
    pub fn new(content: OccurrenceContent, name: impl Into<Arc<str>>) -> Self {
        Self { content, name: name.into(), visible: true, opacity: 1., blend: LayerBlend::Normal,
            locked:false, alpha_locked:false, reference:false, attachment:Attachment::None, isolated_blend:LayerBlend::Normal,
            translation:Point::default(), placement:LayerPlacement::default(), mask:None }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct PaintSource {
    pub domain: [u32; 2],
    pub raster: RasterRevision,
    pub original: Option<Arc<SourceImage>>,
    pub operations: Arc<Vec<crate::RasterOperation>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageSource {
    pub domain: [u32; 2],
    pub raster: RasterRevision,
    pub initial: Option<Selection>,
    pub default_coverage: f32,
    pub operations: Arc<Vec<crate::RasterOperation>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MaskUse {
    pub source: CoverageHandle,
    pub enabled: bool,
    pub linked: bool,
    pub inverted: bool,
    pub translation: Point,
    pub placement: Projective,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EffectApplication {
    pub definition: DefinitionHandle,
    pub values: Vec<EffectValue>,
    pub domain: [u32; 2],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension { #[default] Scalar, Count, Angle, Time, SourcePixels, CompositionPixels, Normalized }
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub program: Arc<EffectProgram>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SavedSelection { pub selection: Selection }
#[derive(Clone, Debug, PartialEq)]
pub struct Guides { pub rulers: Vec<(PortableId, RulerGeometry)> }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvaluationContext { pub elapsed: f32, pub phases: Arc<Vec<(EffectHandle, f32)>> }
impl EvaluationContext {
    pub fn retain_effects(&mut self, artwork: &Artwork) {
        if self.phases.iter().any(|(handle, _)| artwork.effects.get(*handle).is_none()) {
            Arc::make_mut(&mut self.phases).retain(|(handle, _)| artwork.effects.get(*handle).is_some());
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub composition: CompositionHandle,
    pub name: Arc<str>,
    pub context: EvaluationContext,
    pub frame: Option<(Point, [u32; 2])>,
    pub scale: [f32; 2],
    pub sdr: SdrRendition,
    pub proof: Option<ProofRecipe>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum SourceTarget { Paint(PaintHandle), Coverage(CoverageHandle), Selection(SelectionHandle) }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkingState {
    pub generation: u64,
    pub selection: Option<Selection>,
    pub selection_overlays: SelectionOverlays,
    pub layer_selection: BTreeSet<OccurrenceHandle>,
    pub layer_anchor: Option<OccurrenceHandle>,
    pub solo_visibility: Option<BTreeMap<OccurrenceHandle,bool>>,
    pub occurrence: Option<OccurrenceHandle>,
    pub target: Option<SourceTarget>,
    pub inspect_mask: Option<OccurrenceHandle>,
}
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionOverlays {
    pub visibility: BTreeMap<OccurrenceHandle,bool>,
    pub properties: BTreeMap<SelectionHandle,SelectionMaskProperties>,
}
impl SelectionOverlays {
    pub fn metadata_bytes(&self) -> usize {
        self.visibility.len().saturating_mul(64).saturating_add(self.properties.len().saturating_mul(128))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureCheckpoint {
    pub owner: u64,
    pub document: PortableId,
    pub session_generation: u64,
    pub artwork_generation: u64,
    pub working_generation: u64,
    pub edit_checkpoint: u64,
}
#[derive(Clone, Debug)]
pub struct ArtworkCapture { pub artwork: Arc<Artwork>, pub checkpoint: CaptureCheckpoint }

impl Artwork {
    pub fn new(size: [u32; 2]) -> Result<Self, &'static str> {
        if size.contains(&0) || size.iter().any(|n| *n > crate::MAX_EXTENT) { return Err("Invalid composition size"); }
        let mut stacks = Store::default();
        let stack = stacks.insert(PortableId::random(), Stack::default())?;
        let mut compositions = Store::default();
        let root = compositions.insert(PortableId::random(), Composition { size, origin:Point::default(), color:DocumentColor::default(),
            blend:BlendSpace::Linear, resolution:None, result:stack })?;
        let mut outputs = Store::default();
        let default_output = outputs.insert(PortableId::random(), Output { composition:root, name:Arc::from(""), context:EvaluationContext::default(),
            frame:None, scale:[1.;2], sdr:SdrRendition::default(), proof:None })?;
        Ok(Self { id:PortableId::random(), root, compositions, stacks, outputs, default_output,
            occurrences:Store::default(), paint:Store::default(), coverage:Store::default(), effects:Store::default(),
            definitions:Store::default(), selections:Store::default(), guides:Store::default(), metadata:Arc::new(PhotoMetadata::default()), extensions:Arc::default() })
    }
    pub fn capture(&self, checkpoint: CaptureCheckpoint) -> Result<ArtworkCapture, &'static str> {
        if checkpoint.document != self.id { return Err("Capture belongs to a different drawing"); }
        Ok(ArtworkCapture { artwork: Arc::new(self.clone()), checkpoint })
    }
}

impl Artwork {
    pub fn topology(&self) -> Result<super::GraphShape, String> {
        use super::{Shape, Content, GraphShape};
        fn id<T>(store:&Store<T>,handle:Handle<T>) -> Result<PortableId,String> {
            store.get(handle).ok_or("Missing authored reference")?;
            store.id(handle).ok_or_else(||"Missing authored identity".into())
        }
        let mut shape=GraphShape {objects:BTreeMap::new(),resources:Default::default(),outputs:Vec::new(),default_output:Some(id(&self.outputs,self.default_output)?)};
        let mut insert=|identity,record| {
            if identity==self.id || shape.objects.insert(identity,record).is_some() {Err("Duplicate artwork identity".to_string())} else {Ok(())}
        };
        id(&self.compositions,self.root)?;
        for (_,identity,composition) in self.compositions.iter() {insert(identity,Shape::Composition {result:id(&self.stacks,composition.result)?})?;}
        for (_,identity,stack) in self.stacks.iter() {insert(identity,Shape::Stack {entries:stack.entries.iter().map(|entry|id(&self.occurrences,*entry)).collect::<Result<_,_>>()?})?;}
        for (_,identity,occurrence) in self.occurrences.iter() {
            let content=match occurrence.content {
                OccurrenceContent::Paint(handle)=>Content::Paint(id(&self.paint,handle)?),
                OccurrenceContent::Stack(handle)=>Content::Group(id(&self.stacks,handle)?),
                OccurrenceContent::Effect(handle)=>Content::Effect(id(&self.effects,handle)?),
                OccurrenceContent::Selection(handle)=>Content::Selection(id(&self.selections,handle)?),
            };
            insert(identity,Shape::Occurrence {content,mask:occurrence.mask.as_ref().map(|mask|id(&self.coverage,mask.source)).transpose()?})?;
        }
        for (_,identity,_) in self.paint.iter() {insert(identity,Shape::Paint)?;}
        for (_,identity,_) in self.coverage.iter() {insert(identity,Shape::Coverage)?;}
        for (_,identity,effect) in self.effects.iter() {insert(identity,Shape::Effect {definition:id(&self.definitions,effect.definition)?,inputs:Vec::new()})?;}
        for (_,identity,_) in self.definitions.iter() {insert(identity,Shape::Definition {dependencies:Vec::new()})?;}
        for (_,identity,_) in self.selections.iter() {insert(identity,Shape::Selection)?;}
        for (_,identity,_) in self.guides.iter() {insert(identity,Shape::Guides)?;}
        for (_,identity,output) in self.outputs.iter() {insert(identity,Shape::Output {composition:id(&self.compositions,output.composition)?})?;}
        shape.outputs=self.outputs.iter().map(|(_,identity,_)|identity).collect();
        shape.validate(id(&self.compositions,self.root)?,Default::default())?;
        Ok(shape)
    }
}

impl ArtworkCapture {
    pub fn composition(&self)->&Composition{self.artwork.compositions.get(self.artwork.root).expect("Captured composition")}
    pub fn output(&self)->&Output{self.artwork.outputs.get(self.artwork.default_output).expect("Captured output")}
    pub fn metadata(&self)->&PhotoMetadata{&self.artwork.metadata}
}
