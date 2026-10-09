use super::{Handle, PortableId, Store, Image, ImageObject, PaintBase};
use crate::{BlendSpace, EffectProgram, EffectValue, ImageResolution, LayerBlend,
    PhotoMetadata, RulerGeometry, Selection, SelectionMaskProperties,
    color::{DocumentColor, ProofRecipe, hdr::SdrRendition}, raster::RasterRevision};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

pub type CompositionHandle = Handle<Composition>;
pub type StackHandle = Handle<Stack>;
pub type OccurrenceHandle = Handle<Occurrence>;
pub type PaintHandle = Handle<PaintSource>;
pub type ImageObjectHandle = Handle<ImageObject>;

pub type CoverageHandle = Handle<CoverageSource>;
pub type EffectHandle = Handle<EffectApplication>;
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
    pub objects: Store<ImageObject>,
    pub coverage: Store<CoverageSource>,
    pub effects: Store<EffectApplication>,
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
    pub color: DocumentColor,
    pub blend: BlendSpace,
    pub resolution: Option<ImageResolution>,
    pub result: StackHandle,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack { pub entries: Vec<OccurrenceHandle> }
#[derive(Clone, Debug, PartialEq)]
pub enum OccurrenceContent {
    Paint(PaintHandle), Objects(ImageObjectHandle), Stack(StackHandle), Effect(EffectHandle), Selection(SelectionHandle),
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
    pub offset: [i64; 2],
    pub mask: Option<MaskUse>,
}
impl Occurrence {
    pub fn new(content: OccurrenceContent, name: impl Into<Arc<str>>) -> Self {
        Self { content, name: name.into(), visible: true, opacity: 1., blend: LayerBlend::Normal,
            locked:false, alpha_locked:false, reference:false, attachment:Attachment::None,
            offset:[0;2], mask:None }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct PaintSource {
    pub color_mode: crate::color::LayerColorMode,
    pub domain: [u32; 2],
    pub raster: RasterRevision,
    pub base: Option<PaintBase>,
    pub operations: Arc<Vec<crate::RasterOperation>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CoverageSource {
    pub domain: [u32; 2],
    pub raster: RasterRevision,
    pub default_coverage: f32,
    pub operations: Arc<Vec<crate::RasterOperation>>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MaskUse {
    pub source: CoverageHandle,
    pub enabled: bool,
    pub linked: bool,
    pub inverted: bool,
    pub offset: [i64; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub struct EffectApplication {
    pub program: Arc<EffectProgram>,
    pub values: Vec<EffectValue>,
    pub spatial: Option<EffectSpatialReference>,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSpatialReference {
    pub mapping: super::Affine64,
    pub extent: [f64; 2],
}
impl EffectSpatialReference {
    pub fn inverse(self) -> Option<super::Affine64> { self.mapping.inverse() }
    pub fn map(self, point: [f64; 2]) -> [f64; 2] { self.mapping.map(point) }
    pub fn validate(self) -> Result<(), &'static str> {
        self.mapping.validate().map_err(|_| "Invalid effect spatial mapping")?;
        if self.extent.iter().any(|v| !v.is_finite() || *v <= 0. || *v > crate::MAX_EXTENT as f64) {
            return Err("Invalid effect reference extent");
        }
        Ok(())
    }
}
impl EffectApplication {
    pub fn new(program: Arc<EffectProgram>, values: Vec<EffectValue>, size: [u32; 2]) -> Self {
        let spatial = program.uses_spatial_reference().then_some(EffectSpatialReference {
            mapping: super::Affine64::default(),
            extent: size.map(f64::from),
        });
        Self { program, values, spatial }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        crate::EffectView::new(&self.program, &self.values).validate()?;
        if self.program.uses_spatial_reference() != self.spatial.is_some() { return Err("Missing or unexpected effect spatial reference"); }
        if let Some(spatial) = self.spatial { spatial.validate()?; }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension { #[default] Scalar, Count, Angle, Time, SourcePixels, CompositionPixels, Normalized }
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
    pub view_origin: [i64; 2],
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
        let root = compositions.insert(PortableId::random(), Composition { size, color:DocumentColor::default(),
            blend:BlendSpace::Linear, resolution:None, result:stack })?;
        let mut outputs = Store::default();
        let default_output = outputs.insert(PortableId::random(), Output { composition:root, name:Arc::from(""), context:EvaluationContext::default(),
            sdr:SdrRendition::default(), proof:None })?;
        Ok(Self { id:PortableId::random(), root, compositions, stacks, outputs, default_output,
            occurrences:Store::default(), paint:Store::default(), objects:Store::default(), coverage:Store::default(), effects:Store::default(),
            selections:Store::default(), guides:Store::default(), metadata:Arc::new(PhotoMetadata::default()), extensions:Arc::default() })
    }
    pub fn capture(&self, checkpoint: CaptureCheckpoint) -> Result<ArtworkCapture, &'static str> {
        if checkpoint.document != self.id { return Err("Capture belongs to a different drawing"); }
        Ok(ArtworkCapture { artwork: Arc::new(self.clone()), checkpoint })
    }
}

impl Artwork {
    pub fn images(&self) -> Result<BTreeMap<PortableId, &Image>, String> {
        let mut images = BTreeMap::new();
        let mut roots=crate::RootInventory::default();roots.artwork(self);
        for image in roots.images {collect_image(&mut images,image)?;}
        Ok(images)
    }
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
                OccurrenceContent::Objects(handle)=>Content::Objects(id(&self.objects,handle)?),
                OccurrenceContent::Stack(handle)=>Content::Group(id(&self.stacks,handle)?),
                OccurrenceContent::Effect(handle)=>Content::Effect(id(&self.effects,handle)?),
                OccurrenceContent::Selection(handle)=>Content::Selection(id(&self.selections,handle)?),
            };
            insert(identity,Shape::Occurrence {content,mask:occurrence.mask.as_ref().map(|mask|id(&self.coverage,mask.source)).transpose()?})?;
        }
        let mut images = BTreeMap::<PortableId, &Image>::new();
        for (_,identity,paint) in self.paint.iter() {
            if let Some(base) = &paint.base { collect_image(&mut images, &base.image)?; }
            insert(identity,Shape::Paint { image:paint.base.as_ref().map(|base|base.image.id()) })?;
        }
        for (_,identity,object) in self.objects.iter() { collect_image(&mut images,&object.image)?; insert(identity,Shape::ImageObject { image:object.image.id() })?; }
        for identity in images.keys() { insert(*identity,Shape::Image)?; }
        for (_,identity,_) in self.coverage.iter() {insert(identity,Shape::Coverage)?;}
        for (_,identity,_) in self.effects.iter() {insert(identity,Shape::Effect)?;}
        for (_,identity,_) in self.selections.iter() {insert(identity,Shape::Selection)?;}
        for (_,identity,_) in self.guides.iter() {insert(identity,Shape::Guides)?;}
        for (_,identity,output) in self.outputs.iter() {insert(identity,Shape::Output {composition:id(&self.compositions,output.composition)?})?;}
        shape.outputs=self.outputs.iter().map(|(_,identity,_)|identity).collect();
        let mut roots=crate::RootInventory {authored_only:true,..Default::default()};roots.artwork(self);
        shape.resources=roots.resource_ids();
        if shape.resources.contains(&self.id) || self.extensions.records.keys().any(|id|shape.resources.contains(id)) {return Err("Object and resource identities overlap".into());}
        shape.validate(id(&self.compositions,self.root)?,Default::default())?;
        Ok(shape)
    }
}

impl ArtworkCapture {
    pub fn composition(&self)->&Composition{self.artwork.compositions.get(self.artwork.root).expect("Captured composition")}
    pub fn output(&self)->&Output{self.artwork.outputs.get(self.artwork.default_output).expect("Captured output")}
    pub fn metadata(&self)->&PhotoMetadata{&self.artwork.metadata}
}

fn collect_image<'a>(images:&mut BTreeMap<PortableId,&'a Image>,image:&'a Image)->Result<(),String> {
    if let Some(previous)=images.insert(image.id(),image) && !previous.identity_matches(image) { return Err("Conflicting immutable image identity".into()); }
    Ok(())
}
