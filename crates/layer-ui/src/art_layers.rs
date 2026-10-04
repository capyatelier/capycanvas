//! Artwork commands and gestures. Native layer panels only render this model.
use super::*;
use layer_core::{Attachment, Edit, LayerBlend, Point, Selection, CoverageSnapshot};
use layer_core::authored::{Occurrence, OccurrenceContent, OccurrenceHandle, PaintSource, RecordChange, SourceTarget, Stack};
use super::session::{occurrence_handle, occurrence_token};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

type PreviewGeometry = (layer_core::Projective, layer_core::Interpolation, Point, [u32; 2],
    Option<(layer_core::authored::CoverageHandle, layer_core::Projective, Point, [u32; 2], bool)>);
type GeneratorPreview = (layer_core::authored::EffectApplication, Arc<layer_core::EffectProgram>,
    ([u32; 2], layer_core::BlendSpace, layer_core::color::DocumentColor));
type PreviewRevision = (Option<std::sync::Weak<layer_core::color::source::SourceImage>>, Option<std::sync::Weak<layer_core::MeshMap>>,
    Option<GeneratorPreview>, PreviewGeometry, u64);

#[derive(Default)]
pub(super) struct PreviewRevisions {
    occurrences: std::collections::BTreeMap<OccurrenceHandle, PreviewRevision>,
    next: u64,
}
impl PreviewRevisions {
    pub(super) fn update(&mut self, document: &Document) {
        let scene = document.scene();
        let composition = document.composition();
        for &handle in scene.order() {
            let occurrence = scene.occurrence(handle).unwrap();
            let source = scene.paint_source(handle).and_then(|paint| paint.original.as_ref()).map(Arc::downgrade);
            let placement = &occurrence.placement;
            let mesh = placement.mesh.as_ref().map(Arc::downgrade);
            let generator = match occurrence.content {
                OccurrenceContent::Effect(effect) => document.artwork.effects.get(effect).and_then(|application| {
                    let definition = document.artwork.definitions.get(application.definition)?;
                    (definition.program.kind == layer_core::EffectKind::Generator).then_some((application, &definition.program))
                }),
                _ => None,
            };
            let environment = (composition.size, composition.blend, composition.color);
            let geometry = (placement.outer, placement.interpolation, occurrence.translation, scene.local_extent(handle),
                occurrence.mask.as_ref().map(|mask| (mask.source, mask.placement, mask.translation, document.artwork.coverage.get(mask.source).unwrap().domain, mask.linked)));
            let same = self.occurrences.get(&handle).is_some_and(|(a, b, c, previous, _)| {
                let source = match (a, &source) { (Some(a), Some(b)) => a.ptr_eq(b), (None, None) => true, _ => false };
                let mesh = match (b, &mesh) { (Some(a), Some(b)) => a.ptr_eq(b), (None, None) => true, _ => false };
                let generator = match (c, generator) { (Some(a), Some((application, program))) => a.0 == *application && (Arc::ptr_eq(&a.1, program) || a.1 == *program) && a.2 == environment, (None, None) => true, _ => false };
                source && mesh && generator && *previous == geometry
            });
            if !same {
                self.next = self.next.wrapping_add(1);
                let generator = generator.map(|(application, program)| (application.clone(), program.clone(), environment));
                self.occurrences.insert(handle, (source, mesh, generator, geometry, self.next));
            }
        }
        self.occurrences.retain(|handle, _| scene.position(*handle).is_some());
    }
    pub(super) fn id(&self, handle: OccurrenceHandle) -> u64 { self.occurrences.get(&handle).map_or(0, |value| value.4) }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerCanvasTool {
    #[default]
    Paint,
    Move,
    Transform,
    Crop,
    Select,
    Selection { kind: SelectionTool },
    SelectColor { source: RegionSource },
    LassoFill,
    Hand,
    PickVisible,
    PickLayer,
    Region {
        fill: bool,
        source: RegionSource,
    },
    Gradient {shape:layer_core::GradientShape},
    Figure {
        shape: FigureShape,
        paint: FigurePaint,
    },
    Ruler {
        kind: RulerKind,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionSource {
    #[default]
    Visible,
    Editing,
    Reference,
}
impl LayerCanvasTool {
    pub fn region(self) -> Option<(bool, RegionSource, bool)> {
        match self {
            Self::Region { fill, source } => Some((fill, source, true)),
            Self::SelectColor { source } => Some((false, source, false)),
            _ => None,
        }
    }
    pub fn selection_tool(self) -> Option<SelectionTool> {
        match self {
            Self::Select => Some(SelectionTool::Lasso),
            Self::Selection { kind } => Some(kind),
            Self::Region { fill: false, .. } => Some(SelectionTool::Wand),
            Self::SelectColor { .. } => Some(SelectionTool::Color),
            _ => None,
        }
    }
    pub fn picks_color(self) -> bool {
        matches!(self, Self::PickVisible | Self::PickLayer)
    }
    pub fn draws(self) -> bool {
        matches!(self, Self::Paint | Self::LassoFill | Self::Region { fill: true, .. }
            | Self::Gradient { .. } | Self::Figure { .. })
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayersView {
    pub tool: LayerCanvasTool,
    pub has_selection: bool,
    pub quick_mask: bool,
    pub mask_editing: Option<MaskEditingView>,
    pub selection_resize: Option<super::selection_refine::SelectionRefineView>,
    pub canvas_size: Option<super::canvas_size::CanvasSizeView>,
    pub image_size: Option<super::image_size::ImageSizeView>,
    pub frequency_separation: Option<super::retouch_layers::FrequencySeparationView>,
    pub can_reference: bool,
    pub can_delete: bool,
    pub references_selected: bool,
    pub reference_action_label: std::sync::Arc<str>,
    /// Header target remains available even inside a collapsed group.
    pub editing_layer: Option<LayerState>,
    pub rename_layer: Option<u64>,
    pub controls: LayerControls,
    pub attachment: LayerAttachmentControl,
    pub connections: Vec<LayerConnection>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct LayerControls {
    pub opacity: bool,
    pub blend: bool,
    pub alpha_lock: bool,
    pub edit_lock: bool,
    pub mask: bool,
    pub fill: bool,
}
/// Only a paint layer's mask can be baked into its pixels.
pub(super) fn apply_mask_refusal(kind: LayerKind, l: &Localizer) -> Option<std::sync::Arc<str>> {
    match kind {
        LayerKind::Paint => None,
        LayerKind::Group => Some(l.text(MessageId::COMMANDS_REFUSAL_ART_LAYERS_A_GROUP_S_MASK_CAN_T_BE_APPLIED_ON_ITS_OWN_MERGE_GROUP_APPLIES_IT)),
        LayerKind::Effect => Some(l.text(MessageId::COMMANDS_REFUSAL_ART_LAYERS_AN_EFFECT_LAYER_S_MASK_SETS_WHERE_THE_EFFECT_SHOWS_IT_CAN_T_BE_APPLIED)),
        LayerKind::Selection => Some(l.text(MessageId::COMMANDS_REFUSAL_ART_LAYERS_ONLY_A_PAINT_LAYER_S_MASK_CAN_BE_APPLIED)),
    }
}
impl LayerControls {
    pub(super) fn for_layer(doc: &Document, id: OccurrenceHandle, l: &Occurrence) -> Self {
        let unlocked = !doc.is_locked(id);
        let editable = l.kind() != LayerKind::Selection;
        Self {
            opacity: editable && unlocked,
            blend: editable && unlocked,
            alpha_lock: l.kind() == LayerKind::Paint && unlocked,
            edit_lock: !doc.scene().parent(id).is_some_and(|p| doc.is_locked(p)),
            mask: editable && unlocked,
            fill: l.kind() == LayerKind::Paint && unlocked && !matches!(doc.working.target, Some(SourceTarget::Coverage(_))),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum LayerAction {
    New {
        group: bool,
        clipped: bool,
    },
    Select {
        id: u64,
        mask: bool,
    },
    SelectRow {
        id: u64,
        extend: bool,
        toggle: bool,
    },
    /// Toggle row selection without changing the content/mask editing target.
    ToggleSelection {
        id: u64,
    },
    Context {
        id: u64,
        mask: bool,
    },
    BeginRename {
        id: u64,
    },
    CancelRename,
    SelectAllLayers {
        selected: bool,
    },
    GroupSelected,
    Ungroup {
        id: u64,
    },
    /// Flatten Image without asking, as accepted from its notice.
    Flatten,
    DeleteSelected,
    DuplicateSelected,
    Visibility {
        id: u64,
        value: bool,
    },
    ShowParents {
        id: u64,
    },
    ShowAll,
    SoloSelected,
    Clear {
        id: u64,
    },
    RepairSourceProfile { id: u64 },
    RasterizeSource { id: u64 },
    CopyMask {
        id: u64,
    },
    PasteMask {
        id: u64,
    },
    MaskSelection {
        id: u64,
        hide: bool,
    },
    /// Checked selections add references; the lone editing target toggles off.
    ReferenceSelection,
    Rename {
        id: u64,
        name: String,
    },
    Duplicate {
        id: u64,
    },
    Delete {
        id: u64,
    },
    ToggleAlphaLock {
        id: u64,
    },
    AlphaLock {
        id: u64,
        value: bool,
    },
    Lock {
        id: u64,
        value: bool,
    },
    Clip {
        id: u64,
        value: bool,
    },
    AttachEffect {
        id: u64,
        owner: u64,
    },
    TogglePassThrough {
        id: u64,
    },
    Blend {
        id: u64,
        value: u32,
    },
    Reference {
        id: u64,
    },
    AddMask {
        id: u64,
        replace: bool,
    },
    DeleteMask {
        id: u64,
    },
    ApplyMask {
        id: u64,
    },
    FillSelection,
    EnableMask {
        id: u64,
        value: bool,
    },
    LinkMask {
        id: u64,
        value: bool,
    },
    ShowMask {
        id: u64,
        value: bool,
    },
    InvertMask {
        id: u64,
    },
    ClearMask {
        id: u64,
        reveal: bool,
    },
    Tool {
        tool: LayerCanvasTool,
    },
    Deselect,
    InvertSelection,
    Collapse {
        id: u64,
    },
    Reparent {
        id: u64,
        parent: Option<u64>,
        index: u32,
    },
    Drop {
        id: u64,
        target: u64,
        fraction: f32,
        #[serde(default)]
        surface: LayerDropSurface,
    },
}
pub use layer_core::OccurrenceDropPosition as LayerDropPosition;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerDropSurface { #[default] Row, Thumbnail }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerDropHint { pub target: u64, pub position: LayerDropPosition }
/// Captured destination for an external image insertion. Resolve and validate
/// again against the same document revision when prepared sources arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageLayerDestination {
    #[serde(serialize_with = "serialize_occurrence_token", deserialize_with = "deserialize_occurrence_token")]
    pub target: OccurrenceHandle,
    pub position: LayerDropPosition,
}
fn serialize_occurrence_token<S: serde::Serializer>(handle: &OccurrenceHandle, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(occurrence_token(*handle))
}
fn deserialize_occurrence_token<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<OccurrenceHandle, D::Error> {
    occurrence_handle(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}
/// Identity and document-space destination captured before a host yields to a
/// picker, provider or decoder. Camera changes never retarget an accepted drop.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ImagePlacementContext {
    pub epoch: u64,
    pub revision: u64,
    pub target: Option<SourceTarget>,
    pub center: Option<Point>,
    pub destination: Option<ImageLayerDestination>,
}
#[derive(Clone,Debug,Default,PartialEq,Serialize,Deserialize)]
pub struct GradientToolSettings {
    pub definition:Option<layer_core::GradientDefinition>,
    pub shape:layer_core::GradientShape,
}
#[derive(Default)]
pub(super) struct LayerInteraction {
    pub tool: LayerCanvasTool,
    pub collapsed: BTreeSet<OccurrenceHandle>,
    pub clipboard_mask: Option<(CoverageSnapshot, layer_core::ImageTransform)>,
    pub path: Vec<Point>,
    original: Option<(OccurrenceHandle, Occurrence)>,
    pub changed: bool,
    pub gradient: GradientToolSettings,
    pub gradient_before: Option<GradientToolSettings>,
    pub figure: (FigureShape, FigurePaint),
}
impl LayerInteraction {
    pub fn depth(&self, doc: &Document, id: OccurrenceHandle) -> u32 {
        let scene = doc.scene();
        let mut parent = scene.parent(id);
        let mut depth = 0;
        while let Some(id) = parent {
            depth += 1;
            parent = scene.parent(id);
        }
        depth
    }
    pub fn hidden_by_group(&self, doc: &Document, id: OccurrenceHandle) -> bool {
        let scene = doc.scene();
        let mut parent = scene.parent(id);
        while let Some(id) = parent {
            if self.collapsed.contains(&id) { return true; }
            parent = scene.parent(id);
        }
        false
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn selected_layers(&self) -> &BTreeSet<OccurrenceHandle> {
        &self.engine.document().working.layer_selection
    }
    pub(super) fn set_selected_layers(&mut self, selected: BTreeSet<OccurrenceHandle>) -> Result<(), String> {
        if self.selected_layers() == &selected { return Ok(()); }
        let mut working = self.engine.document().working.clone();
        working.layer_selection = selected;
        self.layer_edit(Edit::Working(working))
    }
    fn select_layer_row(&mut self, id: u64, extend: bool, toggle: bool) -> Result<(), String> {
        let handle = occurrence_handle(id)?;
        let doc = self.engine.document();
        if doc.scene().occurrence(handle).is_none() || self.layer_interaction.hidden_by_group(doc, handle) {
            return Err(self.localization().text(MessageId::RESOURCES_ERROR_UNKNOWN_LAYER).to_string());
        }
        let mut working = doc.working.clone();
        if toggle && !extend {
            if !working.layer_selection.remove(&handle) { working.layer_selection.insert(handle); }
            working.layer_anchor = Some(handle);
        } else if extend {
            let visible: Vec<_> = doc.ordered_layers().iter().copied()
                .filter(|h| !self.layer_interaction.hidden_by_group(doc, *h)).collect();
            let end = visible.iter().position(|h| *h == handle).unwrap();
            let start = working.layer_anchor.and_then(|h| visible.iter().position(|v| *v == h)).unwrap_or(end);
            working.layer_selection = visible[start.min(end)..=start.max(end)].iter().copied().collect();
            working.layer_anchor = Some(visible[start]);
        } else {
            if working.occurrence == Some(handle) && working.layer_selection.contains(&handle) { return Ok(()); }
            let selected = working.layer_selection;
            self.layer_action(LayerAction::Select { id, mask: false })?;
            if selected.contains(&handle) { self.set_selected_layers(selected)?; }
            return Ok(());
        }
        self.layer_edit(Edit::Working(working))
    }
    fn occurrence_change(&self, id: OccurrenceHandle, value: Occurrence) -> Result<Edit, String> {
        Ok(Edit::Occurrence(RecordChange::replace(&self.engine.document().artwork.occurrences, id, Some(value))?))
    }
    fn visibility_edit(&self, values: impl IntoIterator<Item = (OccurrenceHandle, bool)>) -> Result<Edit, String> {
        let doc = self.engine.document();
        let mut working = doc.working.clone();
        working.solo_visibility = None;
        let mut edits = Vec::new();
        for (id, visible) in values {
            let mut occurrence = doc.scene().occurrence(id).ok_or("Unknown layer")?.clone();
            if occurrence.visible != visible {
                occurrence.visible = visible;
                edits.push(self.occurrence_change(id, occurrence)?);
            }
            working.selection_visibility.remove(&id);
        }
        edits.push(Edit::Working(working));
        Ok(Edit::Batch(edits))
    }
    fn layer_deletion_ids(&self, selected: &BTreeSet<OccurrenceHandle>) -> Vec<OccurrenceHandle> {
        let doc = self.engine.document();
        let mut ids = selected.clone();
        for &id in selected.intersection(&self.layer_interaction.collapsed) {
            ids.extend(doc.scene().order().iter().copied().filter(|h| layer_core::descends_from(doc.scene(), *h, Some(id))));
        }
        ids.into_iter().collect()
    }
    pub(super) fn can_delete_layer_rows(&self, selected: &BTreeSet<OccurrenceHandle>) -> bool {
        self.engine.document().can_delete_layers(&self.layer_deletion_ids(selected))
    }
    pub(super) fn layer_deletion_edit(&self, selected: &BTreeSet<OccurrenceHandle>) -> Result<Edit, layer_core::DocumentError> {
        self.engine.document().delete_layers_edit(&self.layer_deletion_ids(selected))
    }
    pub(super) fn layer_step_edit(&self, raise: bool) -> Result<Edit, layer_core::DocumentError> {
        use layer_core::DocumentError::InvalidLayerOperation;
        let doc = self.engine.document();
        let roots = doc.layer_roots(self.selected_layers());
        let first = *roots.first().ok_or(InvalidLayerOperation("Select layers first"))?;
        let parent = doc.scene().parent(first);
        if roots.iter().any(|id| doc.scene().parent(*id) != parent) { return Err(InvalidLayerOperation("Select layers in the same group")); }
        let moving = doc.relationship_roots(&roots);
        let siblings = doc.scene().children(parent);
        let at = siblings.iter().position(|id| moving.contains(id)).unwrap();
        let end = siblings.iter().rposition(|id| moving.contains(id)).unwrap();
        let target = if raise { at.checked_sub(1).and_then(|at| siblings.get(at)) } else { siblings.get(end + 1) }.ok_or(InvalidLayerOperation("Invalid layer position"))?;
        doc.drop_layers_edit(&roots, *target, if raise { LayerDropPosition::Above } else { LayerDropPosition::Below })
            .map(|plan| plan.edit)
    }
    fn layer_reparent_edit(&self, id: u64, parent: Option<u64>, index: u32) -> Result<Option<Edit>, String> {
        let doc = self.engine.document();
        let edit = doc.reparent_occurrence_edit(occurrence_handle(id)?, parent.map(occurrence_handle).transpose()?, index as usize).map_err(error)?;
        let mut probe = doc.clone();
        probe.apply(edit.clone()).map_err(error)?;
        Ok((probe.artwork != doc.artwork).then_some(edit))
    }
    fn layer_drop_edit(&self,id:u64,target:u64,fraction:f32,surface:LayerDropSurface)->Result<Option<(Edit,LayerDropHint)>,String> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction){return Err("Invalid layer drop position".into());}
        let doc=self.engine.document();let scene=doc.scene();let handle=occurrence_handle(id)?;let target=occurrence_handle(target)?;
        let row=scene.occurrence(target).ok_or("Unknown destination")?;
        let attach=surface==LayerDropSurface::Thumbnail&&scene.effect(handle).is_some_and(|e|e.program.kind==layer_core::EffectKind::Adjustment);
        let position=if attach {LayerDropPosition::Attach}else if row.kind()==LayerKind::Group&&(0.25..0.75).contains(&fraction){LayerDropPosition::Into}else if fraction<0.5{LayerDropPosition::Above}else{LayerDropPosition::Below};
        let roots = if self.selected_layers().contains(&handle) { doc.layer_roots(self.selected_layers()) } else { vec![handle] };
        let position = if position == LayerDropPosition::Attach && roots.iter().any(|id| !scene.effect(*id).is_some_and(|effect| effect.program.kind == layer_core::EffectKind::Adjustment)) {
            if fraction < 0.5 { LayerDropPosition::Above } else { LayerDropPosition::Below }
        } else { position };
        let plan=doc.drop_layers_edit(&roots,target,position).map_err(error)?;
        let mut probe=doc.clone();probe.apply(plan.edit.clone()).map_err(error)?;
        Ok((probe.artwork!=doc.artwork).then_some((plan.edit,LayerDropHint{target:occurrence_token(plan.target),position:plan.position})))
    }
    pub fn layer_drop_preview(&self,id:u64,target:u64,fraction:f32,surface:LayerDropSurface)->Option<LayerDropHint> {
        self.layer_drop_edit(id,target,fraction,surface).ok().flatten().map(|(_,hint)|hint)
    }
    /// Shared external-image feedback, independent of a dragged internal layer.
    pub fn image_placement_context(
        &self,
        screen: Option<Point>,
        destination: Option<ImageLayerDestination>,
    ) -> Result<ImagePlacementContext, String> {
        self.require_document_idle()?;
        self.image_layer_destination(destination)?;
        Ok(ImagePlacementContext {
            epoch: self.state.document_file.epoch,
            revision: self.engine.document().revision,
            target: self.engine.document().active_target(),
            center: screen.map(|p| self.state.camera.input_transform().map(p)),
            destination,
        })
    }
    pub fn validate_image_placement(&self, context: &ImagePlacementContext) -> Result<(), String> {
        self.require_document_idle()?;
        if self.state.document_file.epoch != context.epoch
            || self.engine.document().revision != context.revision
            || self.engine.document().active_target() != context.target
        {
            return Err("The document or selected layer changed while importing; try again".into());
        }
        self.image_layer_destination(context.destination)?;
        Ok(())
    }
    pub fn image_layer_drop_hint(&self, target: u64, fraction: f32) -> Option<LayerDropPosition> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction)
            || self.operation.active() || !self.engine.backend().supports_tiled_sources()
        { return None; }
        let handle = occurrence_handle(target).ok()?;
        let row = self.engine.document().scene().occurrence(handle)?;
        let position = if row.kind() == LayerKind::Group && (0.25..0.75).contains(&fraction) { LayerDropPosition::Into }
            else if fraction < 0.5 { LayerDropPosition::Above } else { LayerDropPosition::Below };
        self.image_layer_destination(Some(ImageLayerDestination { target: handle, position })).ok()?;
        Some(position)
    }
    pub(super) fn image_layer_destination(&self, destination: Option<ImageLayerDestination>) -> Result<(usize, Option<OccurrenceHandle>), String> {
        let doc = self.engine.document();
        let scene = doc.scene();
        let Some(id) = destination.map(|d| d.target).or(doc.working.occurrence) else { return Ok((0, None)); };
        let row = scene.occurrence(id).ok_or("The destination layer was removed")?;
        let position = destination.map_or(if row.kind() == LayerKind::Group { LayerDropPosition::Into } else { LayerDropPosition::Above }, |d| d.position);
        if position==LayerDropPosition::Attach{return Err("Images can be inserted into a group".into());}
        let parent = if position == LayerDropPosition::Into {
            if row.kind() != LayerKind::Group { return Err("Images can be inserted into a group".into()); }
            Some(id)
        } else { scene.parent(id) };
        if parent.is_some_and(|id| doc.is_locked(id)) { return Err("The destination group is locked".into()); }
        let siblings = scene.children(parent);
        let mut index = if position == LayerDropPosition::Into { 0 }
            else { siblings.iter().position(|h| *h == id).ok_or("Unknown destination")? + usize::from(position == LayerDropPosition::Below) };
        if destination.is_none() && position == LayerDropPosition::Above {
            while index > 0 && scene.occurrence(siblings[index - 1]).is_some_and(|l| l.attachment != Attachment::None) { index -= 1; }
        }
        index = doc.content_insertion(parent, index).0;
        Ok((index, parent))
    }
    pub(super) fn reference_action_removes(&self) -> bool {
        let doc = self.engine.document();
        self.selected_layers().len() == 1 && doc.working.occurrence.is_some_and(|id|
            self.selected_layers().contains(&id) && doc.scene().occurrence(id).is_some_and(|l| l.reference))
    }
    pub(super) fn set_references(&mut self, references: BTreeSet<OccurrenceHandle>) -> Result<(), String> {
        let doc = self.engine.document();
        for id in &references {
            if !doc.scene().occurrence(*id).is_some_and(|l| matches!(l.kind(), LayerKind::Paint | LayerKind::Group)) {
                return Err("Choose a paint layer or group as a reference".into());
            }
        }
        let edits = doc.scene().order().iter().filter_map(|id| {
            let mut occurrence = doc.scene().occurrence(*id)?.clone();
            let reference = references.contains(id);
            if occurrence.reference == reference { return None; }
            occurrence.reference = reference;
            Some(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, *id, Some(occurrence)).expect("placed occurrence")))
        }).collect::<Vec<_>>();
        if edits.is_empty() { return Ok(()); }
        self.layer_edit(Edit::Batch(edits))
    }
    pub(super) fn reference_selection(&self) -> BTreeSet<OccurrenceHandle> {
        self.selected_layers().iter().copied().filter(|id|
            self.engine.document().scene().occurrence(*id).is_some_and(|l| matches!(l.kind(), LayerKind::Paint | LayerKind::Group))).collect()
    }
    /// The host decodes/validates source color off-thread. Retain those original
    /// samples at their own depth/profile; rendering converts into this document
    /// only when a working tile is needed. Import itself never rasterizes pixels.
    pub fn import_layer_source(
        &mut self,
        name: &str,
        source: layer_core::color::source::SourceImage,
    ) -> Result<(), String> {
        self.import_sources(vec![(name.into(), source)], Default::default(), false, None, None)
    }
    /// Prepared photo placement. The supplied center is captured in document
    /// pixels at drop time; menu/clipboard imports default to the canvas center.
    pub fn place_layer_source(
        &mut self,
        name: &str,
        source: layer_core::color::source::SourceImage,
        center: Option<Point>,
    ) -> Result<(), String> {
        self.place_layer_sources(vec![(name.into(), source)], center, None)
    }
    pub fn place_layer_sources(
        &mut self,
        sources: Vec<(String, layer_core::color::source::SourceImage)>,
        center: Option<Point>,
        destination: Option<ImageLayerDestination>,
    ) -> Result<(), String> {
        self.import_sources(sources, Default::default(), true, center, destination)
    }
    pub(super) fn import_sources(
        &mut self,
        sources: Vec<(String, layer_core::color::source::SourceImage)>,
        limits: layer_core::ProjectLimits,
        interactive: bool,
        center: Option<Point>,
        destination: Option<ImageLayerDestination>,
    ) -> Result<(), String> {
        self.require_document_idle()?;
        if !self.engine.backend().supports_tiled_sources() {
            return Err("This renderer does not support tiled photo layers".into());
        }
        if sources.is_empty() { return Err("Choose at least one image".into()); }
        let (index, parent) = self.image_layer_destination(destination)?;
        let doc = self.engine.document();
        let attachment = if destination.is_some() { doc.insertion_attachment(parent, index) } else { Attachment::None };
        let [width, height] = doc.composition().size;
        let center = center.unwrap_or(Point { x: width as f32 * 0.5, y: height as f32 * 0.5 });
        if !center.x.is_finite() || !center.y.is_finite() { return Err("Invalid drop position".into()); }
        let parent_offset = parent.map_or(Point::default(), |id| doc.layer_offset(id));
        let stack_handle = match parent {
            Some(id) => match doc.scene().occurrence(id).ok_or("Unknown group")?.content { OccurrenceContent::Stack(h) => h, _ => return Err("Choose a group".into()) },
            None => doc.composition().result,
        };
        let mut artwork = doc.artwork.clone();
        let mut stack = artwork.stacks.get(stack_handle).ok_or("Unknown stack")?.clone();
        let mut edits = Vec::with_capacity(sources.len() * 2 + 2);
        let mut ids = Vec::with_capacity(sources.len());
        for (offset, (name, source)) in sources.into_iter().enumerate() {
            source.validate()?;
            if name.trim().is_empty() || name.chars().count() > 128 || name.chars().any(char::is_control) {
                return Err("Use an image name with 1 to 128 characters".into());
            }
            let [w, h] = source.extent.map(|v| v as f32);
            let paint = RecordChange::insert(&artwork.paint, PaintSource { domain: std::array::from_fn(|axis| doc.composition().size[axis].max(source.extent[axis])), raster: Default::default(), original: Some(Arc::new(source)), operations: Arc::default() });
            let mut occurrence = Occurrence::new(OccurrenceContent::Paint(paint.handle), name);
            occurrence.attachment = attachment;
            if interactive {
                let scale = 1_f32.min(width as f32 / w).min(height as f32 / h);
                occurrence.placement = layer_core::LayerPlacement::from_affine(layer_core::Affine([
                    scale, 0., 0., scale, center.x - parent_offset.x - w * scale * 0.5, center.y - parent_offset.y - h * scale * 0.5,
                ]));
            }
            artwork.paint.change(paint.handle, paint.id, paint.value.clone())?;
            let occurrence = RecordChange::insert(&artwork.occurrences, occurrence);
            artwork.occurrences.change(occurrence.handle, occurrence.id, occurrence.value.clone())?;
            stack.entries.insert(index + offset, occurrence.handle);
            ids.push(occurrence.handle);
            edits.extend([Edit::Paint(paint), Edit::Occurrence(occurrence)]);
        }
        edits.push(Edit::Stack(RecordChange::replace(&artwork.stacks, stack_handle, Some(stack))?));
        let mut working = doc.working.clone();
        working.occurrence = Some(ids[0]);
        working.layer_selection = ids.iter().copied().collect();
        working.layer_anchor = Some(ids[0]);
        working.target = match artwork.occurrences.get(ids[0]).unwrap().content { OccurrenceContent::Paint(h) => Some(SourceTarget::Paint(h)), _ => unreachable!() };
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        let edit = Edit::Batch(edits);
        self.source_edit_candidates(&edit, limits)?;
        if interactive {
            let rollback = self.engine.document().clone().apply(edit.clone()).map_err(error)?;
            let selected = self.selected_layers().clone();
            self.engine.preview_edit(edit).map_err(error)?;
            if let Err(error) = self.begin_layer_placement(Some(super::operation::PlacementInsertion {
                index, rollback: rollback.clone(), ids, selected,
            })) {
                self.engine.preview_edit(rollback).map_err(super::error)?;
                self.refresh_document();
                self.refresh_tools();
                self.refresh_commands();
                return Err(error);
            }
        } else {
            self.layer_edit(edit)?;
        }
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn layer_edit(&mut self, edit: Edit) -> Result<(), String> {
        if self.operation.placing() {
            return Err(self.localization().text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST).to_string());
        }
        let previous = self.engine.document().working.selection.clone();
        self.engine.apply_edit(edit).map_err(error)?;
        if previous.is_some() && self.engine.document().working.selection.is_none() { self.selection_masks.reselect = previous; }
        Ok(())
    }
    pub(super) fn set_layer_opacity(
        &mut self,
        id: Option<u64>,
        opacity: f32,
    ) -> Result<(), String> {
        NumericControl::percent().validate(opacity, MessageId::TOOL_SETTING_OPACITY).map_err(|reason| reason.message(self.localization()))?;
        if id.is_none() && self.selection_masks.quick() {
            return Err("Return to artwork before changing layer opacity".into());
        }
        let id = id.map(occurrence_handle).transpose()?.or(self.engine.document().working.occurrence).ok_or("Unknown layer")?;
        let mut occurrence = self.engine.document().scene().occurrence(id).ok_or("Unknown layer")?.clone();
        if occurrence.kind() == LayerKind::Selection { return Err("Selection layers have coverage, not artwork opacity".into()); }
        if self.engine.document().is_locked(id) { return Err("This layer is locked".into()); }
        if self.effect_gesture.is_some() {
            occurrence.opacity = opacity;
            self.engine.preview_edit(self.occurrence_change(id, occurrence)?).map_err(error)
        } else { self.engine.set_layer_opacity(id, opacity).map_err(error) }
    }
    pub(super) fn copied_mask_use(&self, handle: OccurrenceHandle) -> Result<layer_core::authored::MaskUse, String> {
        use layer_core::{Affine, Projective};
        let (snapshot, geometry) = self.layer_interaction.clipboard_mask.as_ref().ok_or_else(||
            self.localization().text(MessageId::COMMANDS_COPY_A_LAYER_MASK_FIRST).to_string())?;
        let doc = self.engine.document();
        let owner = doc.scene().occurrence(handle).ok_or("Unknown layer")?;
        let invalid = || self.localization().text(MessageId::COMMANDS_APPLY_TRANSFORM_BEFORE_EDITING).to_string();
        let mut mask = snapshot.use_.clone();
        mask.translation = if mask.linked { owner.translation } else { Point::default() };
        let world = doc.layer_offset(handle);
        let parent = Point { x: world.x - owner.translation.x, y: world.y - owner.translation.y };
        let destination = if mask.linked {
            owner.placement.post(Projective::from_affine(Affine::translation(world))).ok_or_else(invalid)?
        } else { layer_core::LayerPlacement::from_affine(Affine::translation(parent)) };
        mask.placement = if geometry.placement.mesh.is_none() && destination.mesh.is_none() {
            geometry.projective().and_then(|map| map.then(destination.outer.inverse()?)).ok_or_else(invalid)?
        } else if geometry.placement.mesh == destination.mesh && geometry.placement.outer == destination.outer {
            geometry.source_from_owner.map_or(Some(Projective::IDENTITY), Projective::inverse).ok_or_else(invalid)?
        } else { return Err(invalid()); };
        mask.validate().map_err(error)?;
        Ok(mask)
    }

    pub(super) fn editable_layer(&self, id: u64) -> Result<Occurrence, String> {
        let id = occurrence_handle(id)?;
        let layer = self.engine.document().scene().occurrence(id).ok_or("Unknown layer")?;
        if self.engine.document().is_locked(id) { return Err("This layer is locked".into()); }
        Ok(layer.clone())
    }
    pub(super) fn layer_action(&mut self, action: LayerAction) -> Result<(), String> {
        if self.selection_masks.quick() {
            match action {
                LayerAction::Visibility { id: 0, value } => { self.selection_masks.quick_visible = value; return Ok(()); }
                LayerAction::Select { id: 0, .. } | LayerAction::SelectRow { id: 0, .. } | LayerAction::Context { id: 0, .. } | LayerAction::ToggleSelection { id: 0 } => return Ok(()),
                LayerAction::Delete { id: 0 } | LayerAction::DeleteSelected => return self.return_to_artwork(),
                _ => (),
            }
        }
        if self.selection_masks.target().is_some() && matches!(action,
            LayerAction::Clear {..}|LayerAction::FillSelection|LayerAction::ApplyMask {..}|LayerAction::AddMask {..}|LayerAction::PasteMask {..}|LayerAction::MaskSelection {..}|LayerAction::ClearMask {..}|LayerAction::InvertMask {..}|LayerAction::RasterizeSource {..}|LayerAction::RepairSourceProfile {..}) {
            return Err("Return to artwork before changing artwork pixels or layer masks".into());
        }
        if self.selection_masks.quick() && matches!(action,LayerAction::Delete {..}|LayerAction::DeleteSelected) {return Err("Return to artwork before deleting artwork layers".into());}

        if self.operation.placing() && !matches!(action, LayerAction::Tool { .. }) {
            return Err(self.localization().text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST).to_string());
        }
        let hide_selection = matches!(action, LayerAction::MaskSelection { hide: true, .. });
        let action = if let LayerAction::MaskSelection { id, .. } = action {
            if self.engine.document().working.selection.is_none() {
                return Err("Make a selection first".into());
            }
            LayerAction::AddMask { id, replace: true }
        } else {
            action
        };
        match action {
            LayerAction::RasterizeSource { id } => self.request_source_edit(occurrence_handle(id)?, DocumentRequest::RasterizeSource { layer: id })?,
            LayerAction::RepairSourceProfile { id } => self.request_source_edit(occurrence_handle(id)?, DocumentRequest::RepairSourceProfile { layer: id })?,
            LayerAction::Visibility { id, value } => {
                self.layer_edit(self.visibility_edit([(occurrence_handle(id)?, value)])?)?;
            }
            LayerAction::ShowParents { id } => {
                let doc = self.engine.document();
                let mut parent = Some(occurrence_handle(id)?);
                let mut values = Vec::new();
                while let Some(id) = parent {
                    values.push((id, true));
                    parent = doc.scene().parent(id);
                }
                self.layer_edit(self.visibility_edit(values)?)?;
            }
            LayerAction::ShowAll => {
                let values = self.engine.document().scene().order().iter().map(|id| (*id, true)).collect::<Vec<_>>();
                self.layer_edit(self.visibility_edit(values)?)?;
            }
            LayerAction::Context { id, mask } => {
                let selected = self.selected_layers().clone();
                let handle = occurrence_handle(id)?;
                if self.engine.document().working.occurrence != Some(handle) || mask {
                    self.layer_action(LayerAction::Select { id, mask })?;
                }
                if selected.contains(&handle) { self.set_selected_layers(selected)?; }
                else { self.set_selected_layers(BTreeSet::from([handle]))?; }
            }
            LayerAction::BeginRename { id } => { self.editable_layer(id)?; self.state.layer_tools.rename_layer = Some(id); }
            LayerAction::CancelRename => self.state.layer_tools.rename_layer = None,
            LayerAction::SelectAllLayers { selected } => {
                let doc = self.engine.document();
                let checks = doc.ordered_layers().iter().copied()
                    .filter(|id| selected && !self.layer_interaction.hidden_by_group(doc, *id)).collect();
                self.set_selected_layers(checks)?;
            }
            LayerAction::GroupSelected => {
                let doc = self.engine.document();
                let roots = doc.layer_roots(self.selected_layers());
                let id = doc.artwork.occurrences.next_handle();
                let edit = doc.group_layers_edit(&roots, self.new_group_blend(), self.numbered_document_name(MessageId::DOCUMENTS_GROUP_NAME, occurrence_token(id))).map_err(error)?;
                let mut working = doc.working.clone();
                working.occurrence = Some(id); working.target = None; working.inspect_mask = None;
                working.layer_selection = BTreeSet::from([id]); working.layer_anchor = Some(id);
                self.layer_edit(Edit::Batch(vec![edit, Edit::Working(working)]))?;
            }
            LayerAction::Ungroup { id } => {
                let id = occurrence_handle(id)?;
                let doc = self.engine.document();
                let edit = doc.ungroup_layer_edit(id).map_err(error)?;
                self.layer_edit(edit)?;
                self.layer_interaction.collapsed.remove(&id);
            }
            LayerAction::Flatten => self.bake(layer_core::MergeKind::Flatten)?,
            LayerAction::DeleteSelected => {
                self.layer_edit(self.layer_deletion_edit(self.selected_layers()).map_err(error)?)?;
            }
            LayerAction::CopyMask { id } => {
                let doc = self.engine.document();
                let (mask, source) = doc.scene().mask(occurrence_handle(id)?).ok_or("No mask")?;
                let snapshot = CoverageSnapshot { target: mask.source, source: source.clone(), use_: mask.clone() };
                self.layer_interaction.clipboard_mask = Some((snapshot, doc.target_geometry(SourceTarget::Coverage(mask.source))));
            }
            LayerAction::PasteMask { id } => {
                let handle = occurrence_handle(id)?;
                let mut layer = self.editable_layer(id)?;
                let mask = self.copied_mask_use(handle)?;
                let (snapshot, _) = self.layer_interaction.clipboard_mask.as_ref().ok_or("Copy a mask first")?;
                let doc = self.engine.document();
                let source = RecordChange::insert(&doc.artwork.coverage, snapshot.source.clone());
                layer.mask = Some(layer_core::authored::MaskUse { source: source.handle, ..mask });
                let mut working = doc.working.clone();
                working.occurrence = Some(handle); working.target = Some(SourceTarget::Coverage(source.handle)); working.inspect_mask = None;
                working.layer_selection = BTreeSet::from([handle]); working.layer_anchor = Some(handle);
                let mut edits = vec![Edit::Coverage(source), self.occurrence_change(handle, layer)?, Edit::Working(working)];
                if let Some(previous) = doc.scene().occurrence(handle).and_then(|o| o.mask.as_ref())
                    && !doc.artwork.occurrences.iter().any(|(h, _, o)| h != handle && o.mask.as_ref().is_some_and(|mask| mask.source == previous.source)) {
                    edits.push(Edit::Coverage(RecordChange::remove(&doc.artwork.coverage, previous.source)?));
                }
                self.layer_edit(Edit::Batch(edits))?;
            }
            LayerAction::FillSelection => {
                let selection = self.engine.document().working.selection.clone().ok_or("Make a selection first")?;
                self.fill_selection(selection)?;
            }
            LayerAction::Drop { id, target, fraction, surface } => {
                if let Some((edit, hint)) = self.layer_drop_edit(id, target, fraction, surface)? {
                    self.layer_edit(edit)?;
                    if hint.position == LayerDropPosition::Into { self.layer_interaction.collapsed.remove(&occurrence_handle(hint.target)?); }
                }
            }
            LayerAction::New { group, clipped } => {
                if group && !clipped && self.selected_layers().len() > 1 {
                    return self.layer_action(LayerAction::GroupSelected);
                }
                self.return_to_artwork()?;
                let doc = self.engine.document();
                let scene = doc.scene();
                let active = doc.working.occurrence;
                if clipped && !active.and_then(|h| scene.occurrence(h)).is_some_and(|l| l.kind() == LayerKind::Paint || l.kind() == LayerKind::Group && !l.passes_through()) {
                    return Err("Choose a paint layer or isolated group to clip to".into());
                }
                let parent = active.and_then(|h| if !clipped && scene.occurrence(h).is_some_and(|l| l.kind() == LayerKind::Group) { Some(h) } else { scene.parent(h) });
                if parent.is_some_and(|p| doc.is_locked(p)) { return Err("The destination group is locked".into()); }
                let stack_handle = match parent { Some(h) => match scene.occurrence(h).unwrap().content { OccurrenceContent::Stack(h) => h, _ => unreachable!() }, None => doc.composition().result };
                let mut stack = doc.artwork.stacks.get(stack_handle).ok_or("Unknown stack")?.clone();
                let index = active.and_then(|h| {
                    let top = if clipped { scene.attached_effects(h).last().copied().unwrap_or(h) } else { doc.clipping_stack_top(h).unwrap_or(h) };
                    stack.entries.iter().position(|id| *id == top)
                }).unwrap_or(0);
                let mut edits = Vec::new();
                let content = if group {
                    let change = RecordChange::insert(&doc.artwork.stacks, Stack::default());
                    let content = OccurrenceContent::Stack(change.handle); edits.push(Edit::Stack(change)); content
                } else {
                    let change = RecordChange::insert(&doc.artwork.paint, PaintSource { domain: doc.composition().size, raster: Default::default(), original: None, operations: Arc::default() });
                    let content = OccurrenceContent::Paint(change.handle); edits.push(Edit::Paint(change)); content
                };
                let id = doc.artwork.occurrences.next_handle();
                let mut layer = Occurrence::new(content, self.numbered_document_name(if group { MessageId::DOCUMENTS_GROUP_NAME } else { MessageId::DOCUMENTS_LAYER_NAME }, occurrence_token(id)));
                if group { layer.blend = self.new_group_blend(); }
                layer.attachment = if clipped { Attachment::Clip } else { Attachment::None };
                let mut working = doc.working.clone();
                working.occurrence = Some(id); working.target = match layer.content { OccurrenceContent::Paint(h) => Some(SourceTarget::Paint(h)), _ => None }; working.inspect_mask = None;
                working.layer_selection = BTreeSet::from([id]); working.layer_anchor = Some(id);
                stack.entries.insert(index, id);
                edits.extend([Edit::Occurrence(RecordChange::insert(&doc.artwork.occurrences, layer)), Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack_handle, Some(stack))?), Edit::Working(working)]);
                self.layer_edit(Edit::Batch(edits))?;
                if let Some(parent) = parent { self.layer_interaction.collapsed.remove(&parent); }
            }
            LayerAction::Select { id, mask } => {
                let handle = occurrence_handle(id)?;
                if self.engine.document().scene().occurrence(handle).is_some_and(|l| l.kind() == LayerKind::Selection) {
                    return self.begin_selection_mask(layer_core::SelectionTarget::Saved(handle));
                }
                self.return_to_artwork()?;
                self.state.layer_tools.rename_layer = None;
                let doc = self.engine.document();
                let layer = doc.scene().occurrence(handle).ok_or("Unknown layer")?;
                let mut working = doc.working.clone();
                working.occurrence = Some(handle);
                working.target = if mask { Some(SourceTarget::Coverage(layer.mask.as_ref().ok_or("No mask")?.source)) } else { doc.scene().source_target(handle) };
                working.inspect_mask = None;
                working.layer_selection = BTreeSet::from([handle]);
                working.layer_anchor = Some(handle);
                self.layer_edit(Edit::Working(working))?;
            }
            LayerAction::SelectRow { id, extend, toggle } => self.select_layer_row(id, extend, toggle)?,
            LayerAction::ToggleSelection { id } => {
                self.select_layer_row(id, false, true)?;
            }
            LayerAction::ReferenceSelection => {
                let targets = self.reference_selection();
                let mut references = self.engine.document().scene().references();
                if self.reference_action_removes() { references.retain(|id| !targets.contains(id)); }
                else { references.extend(targets); self.set_selected_layers(self.engine.document().working.occurrence.into_iter().collect())?; }
                self.set_references(references)?;
            }
            LayerAction::Tool { tool } => {
                if tool.selection_tool().is_some() && tool.selection_tool()!=Some(SelectionTool::Tonal) { self.return_to_artwork()?; }
                if self.selection_masks.target().is_some() && matches!(tool, LayerCanvasTool::Transform | LayerCanvasTool::Crop | LayerCanvasTool::Move | LayerCanvasTool::LassoFill | LayerCanvasTool::Figure { .. }) {
                    return Err("Return to artwork to use this tool".into());
                }
                if tool.picks_color() {
                    self.eyedropper.layer = tool == LayerCanvasTool::PickLayer;
                    if self.eyedropper.picking.previous.is_none() { self.start_picker()?; }
                    else { self.configure_picker(ColorPickerAction::Source { layer: self.eyedropper.layer })?; }
                    return Ok(());
                }
                if let LayerCanvasTool::Selection { kind } = tool
                    && !matches!(kind, SelectionTool::Rectangle | SelectionTool::Ellipse | SelectionTool::Polygon | SelectionTool::Brush | SelectionTool::Tonal) {
                    return Err("Invalid geometric selection tool".into());
                }
                if tool == LayerCanvasTool::Transform {
                    return self.begin_transform();
                }
                if tool == LayerCanvasTool::Crop {
                    return self.begin_crop(None);
                }
                if let LayerCanvasTool::Ruler { kind } = tool {
                    self.rulers.kind = kind;
                }
                if let LayerCanvasTool::Figure { shape, paint } = tool {
                    if shape == FigureShape::Line && paint != FigurePaint::Outline {
                        return Err("Lines only support an outline".into());
                    }
                    self.layer_interaction.figure = (shape, paint);
                }
                self.cancel_layer_gesture()?;
                if let Some(kind) = tool.selection_tool() {
                    self.selection_tools.options.tool = kind;
                }
                if let Some((fill, source, _)) = tool.region() {
                    self.region_tools.source[usize::from(fill)] = source;
                }
                if let LayerCanvasTool::Gradient {shape}=tool {
                    self.layer_interaction.gradient.shape=shape;
                }
                self.layer_interaction.tool = tool;
                self.state.layer_tools.tool = tool;
                self.refresh_tools();
            }
            LayerAction::Deselect => {
                self.return_to_artwork()?;
                let mut working = self.engine.document().working.clone(); working.selection = None;
                self.layer_edit(Edit::Working(working))?;
            }
            LayerAction::InvertSelection => {
                if !self.selection_masks.quick() { self.return_to_artwork()?; }
                let mut selection = self.current_selection().ok_or("Make a selection first")?;
                selection.inverted = !selection.inverted;
                let mut working = self.engine.document().working.clone(); working.selection = Some(selection);
                self.layer_edit(Edit::Working(working))?;
            }
            LayerAction::Collapse { id } => {
                let id = occurrence_handle(id)?;
                if !self.engine.document().scene().occurrence(id).is_some_and(|o| o.kind() == LayerKind::Group) { return Err("Choose a group".into()); }
                if !self.layer_interaction.collapsed.remove(&id) {
                    self.layer_interaction.collapsed.insert(id);
                    let doc = self.engine.document();
                    let mut working = doc.working.clone();
                    let concealed = working.layer_selection.iter().any(|h| self.layer_interaction.hidden_by_group(doc, *h));
                    working.layer_selection.retain(|h| !self.layer_interaction.hidden_by_group(doc, *h));
                    if working.occurrence.is_some_and(|h| self.layer_interaction.hidden_by_group(doc, h)) {
                        working.occurrence = Some(id); working.target = None; working.inspect_mask = None;
                    }
                    if concealed || working.occurrence == Some(id) { working.layer_selection.insert(id); working.layer_anchor = Some(id); }
                    self.layer_edit(Edit::Working(working))?;
                }
            }
            LayerAction::Lock { id, value } => {
                let id = occurrence_handle(id)?;
                let doc = self.engine.document();
                if doc.scene().parent(id).is_some_and(|p| doc.is_locked(p)) { return Err("This layer is protected".into()); }
                let mut layer = doc.scene().occurrence(id).ok_or("Unknown layer")?.clone();
                layer.locked = value;
                self.layer_edit(self.occurrence_change(id, layer)?)?;
            }
            LayerAction::Reference { id } => {
                let mut references = self.engine.document().scene().references();
                let handle = occurrence_handle(id)?;
                if !references.remove(&handle) { references.insert(handle); }
                self.set_references(references)?;
            }
            LayerAction::SoloSelected => {
                let doc = self.engine.document();
                let scene = doc.scene();
                let mut snapshot = None;
                let next: Vec<_> = if let Some(previous) = &doc.working.solo_visibility {
                    previous.iter().filter(|(id, _)| scene.occurrence(**id).is_some()).map(|(id, value)| (*id, *value)).collect()
                } else {
                    let roots = doc.layer_roots(self.selected_layers());
                    if roots.is_empty() { return Err("Select layers first".into()); }
                    snapshot = Some(scene.order().iter().map(|id| (*id, scene.occurrence(*id).unwrap().visible)).collect());
                    let mut keep = doc.composition_members(&roots);
                    let mut reveal: BTreeSet<_> = roots.iter().copied().collect();
                    for id in roots {
                        let mut parent = scene.parent(id);
                        while let Some(p) = parent { keep.insert(p); reveal.insert(p); parent = scene.parent(p); }
                    }
                    scene.order().iter().map(|id| (*id, keep.contains(id) && (reveal.contains(id) || scene.occurrence(*id).unwrap().visible))).collect()
                };
                let Edit::Batch(mut edits) = self.visibility_edit(next)? else { unreachable!() };
                let Some(Edit::Working(working)) = edits.last_mut() else { unreachable!() };
                working.solo_visibility = snapshot;
                self.layer_edit(Edit::Batch(edits))?;
            }
            a @ (LayerAction::Duplicate { .. } | LayerAction::DuplicateSelected) => {
                let doc = self.engine.document();
                let roots = if let LayerAction::Duplicate { id } = a { vec![occurrence_handle(id)?] } else { doc.layer_roots(self.selected_layers()) };
                let (edit, copies) = doc.duplicate_layers_edit(&roots).map_err(error)?;
                let mut candidate = doc.clone(); candidate.apply(edit.clone()).map_err(error)?;
                let mut edits = vec![edit];
                for id in &copies {
                    let mut copy = candidate.scene().occurrence(*id).ok_or("Unknown copied layer")?.clone();
                    let mut args = FluentArgs::new(); args.set("name", copy.name.as_ref());
                    copy.name = self.localization().format(MessageId::DOCUMENTS_COPY_NAME, &args).into();
                    edits.push(Edit::Occurrence(RecordChange::replace(&candidate.artwork.occurrences, *id, Some(copy))?));
                }
                let root = *copies.first().ok_or("Select layers first")?;
                let mut working = candidate.working.clone();
                working.occurrence = Some(root); working.target = candidate.scene().source_target(root); working.inspect_mask = None;
                working.layer_selection = copies.into_iter().collect(); working.layer_anchor = Some(root);
                edits.push(Edit::Working(working));
                self.layer_edit(Edit::Batch(edits))?;
            }
            LayerAction::Delete { id } => {
                let edit = self.layer_deletion_edit(&BTreeSet::from([occurrence_handle(id)?])).map_err(error)?;
                self.layer_edit(edit)?;
            }
            LayerAction::Reparent { id, parent, index } => {
                if let Some(edit) = self.layer_reparent_edit(id, parent, index)? { self.layer_edit(edit)?; }
            }
            LayerAction::Clip { id, value } => {
                let edit = self.engine.document().attachment_edit(occurrence_handle(id)?, value, false).map_err(error)?;
                self.layer_edit(edit)?;
            }
            LayerAction::AttachEffect { id, owner } => {
                let doc = self.engine.document();
                let id = occurrence_handle(id)?;
                let owner = occurrence_handle(owner)?;
                let index = doc.scene().attached_effects(owner).iter().filter(|h| **h != id).count();
                let edit = doc.attach_effect_edit(id, owner, index, false).map_err(error)?;
                self.layer_edit(edit)?;
            }
            LayerAction::TogglePassThrough { id } => {
                let id = occurrence_handle(id)?;
                let doc = self.engine.document();
                let group = doc.scene().occurrence(id).ok_or("Unknown group")?;
                let blend = if group.passes_through() { group.isolated_blend } else { LayerBlend::PassThrough };
                self.layer_edit(doc.group_blend_edit(id, blend).map_err(error)?)?;
            }
            LayerAction::Blend { id, value } => {
                let blend = LayerBlend::from_code(value).ok_or("Unknown blend mode")?;
                let handle = occurrence_handle(id)?;
                let doc = self.engine.document();
                if doc.scene().occurrence(handle).is_some_and(|o| o.kind() == LayerKind::Group) {
                    self.layer_edit(doc.group_blend_edit(handle, blend).map_err(error)?)?;
                } else {
                    let mut layer = self.editable_layer(id)?;
                    if blend == layer.blend { return Ok(()); }
                    layer.blend = blend;
                    self.layer_edit(self.occurrence_change(handle, layer)?)?;
                }
            }
            other => {
                let token = match &other {
                    LayerAction::Rename { id, .. } | LayerAction::Clear { id } | LayerAction::AlphaLock { id, .. }
                    | LayerAction::ToggleAlphaLock { id }
                    | LayerAction::AddMask { id, .. } | LayerAction::DeleteMask { id } | LayerAction::ApplyMask { id }
                    | LayerAction::EnableMask { id, .. } | LayerAction::LinkMask { id, .. } | LayerAction::ShowMask { id, .. }
                    | LayerAction::InvertMask { id } | LayerAction::ClearMask { id, .. } => *id,
                    _ => unreachable!(),
                };
                let id = occurrence_handle(token)?;
                let mut layer = if matches!(other, LayerAction::ShowMask { .. }) { self.engine.document().scene().occurrence(id).ok_or("Unknown layer")?.clone() } else { self.editable_layer(token)? };
                let original_mask = layer.mask.as_ref().map(|mask| mask.source);
                let mut edits = Vec::new();
                match other {
                    LayerAction::Rename { name, .. } => {
                        if name.trim().is_empty() || name.chars().count() > 128 { return Err("Use a name of 1–128 characters".into()); }
                        layer.name = name.into(); self.state.layer_tools.rename_layer = None;
                    }
                    LayerAction::Clear { .. } => {
                        let OccurrenceContent::Paint(handle) = layer.content else { return Err("Choose a paint layer".into()); };
                        let mut source = self.engine.document().artwork.paint.get(handle).ok_or("Unknown source")?.clone();
                        source.raster = Default::default(); source.operations = Arc::default(); source.original = None;
                        edits.push(Edit::Paint(RecordChange::replace(&self.engine.document().artwork.paint, handle, Some(source))?));
                    }
                    lock @ (LayerAction::AlphaLock { .. } | LayerAction::ToggleAlphaLock { .. }) => {
                        if layer.kind() != LayerKind::Paint { return Err("Alpha lock needs a paint layer".into()); }
                        layer.alpha_locked = if let LayerAction::AlphaLock { value, .. } = lock { value } else { !layer.alpha_locked };
                    }
                    LayerAction::AddMask { replace, .. } => {
                        self.layer_interaction.tool = LayerCanvasTool::Paint; self.state.layer_tools.tool = LayerCanvasTool::Paint; self.refresh_tools();
                        if let Some(mask) = &layer.mask && !replace {
                            let mut working = self.engine.document().working.clone();
                            working.occurrence = Some(id); working.target = Some(SourceTarget::Coverage(mask.source)); working.inspect_mask = None;
                            self.layer_edit(Edit::Working(working))?;
                            return Ok(());
                        }
                        if layer.mask.is_none() || replace {
                            let doc = self.engine.document();
                            let (source, mask) = self.selection_mask(&layer, hide_selection, doc.scene().parent(id), doc.scene().local_extent(id))?;
                            layer.mask = Some(mask); edits.push(Edit::Coverage(source));
                        }
                        let mut working = self.engine.document().working.clone(); working.selection = None; working.occurrence = Some(id);
                        working.target = Some(SourceTarget::Coverage(layer.mask.as_ref().unwrap().source)); working.inspect_mask = None;
                        edits.push(Edit::Working(working));
                    }
                    LayerAction::DeleteMask { .. } => { layer.mask = None; }
                    LayerAction::ApplyMask { .. } => {
                        if let Some(reason) = apply_mask_refusal(layer.kind(), self.localization()) { return Err(reason.to_string()); }
                        let mut mask = layer.mask.take().ok_or("No mask")?;
                        if !mask.enabled { return Err("Enable the mask before applying it".into()); }
                        let doc = self.engine.document();
                        let target = doc.scene().source_target(id).ok_or("Choose a paint layer")?;
                        let mask_target = SourceTarget::Coverage(mask.source);
                        for target in [target, mask_target] { doc.validate_content_write(target).map_err(|reason| notices::drawing_refusal_text(reason, self.localization()).to_string())?; }
                        let to = doc.affine_edit_transform(mask_target).ok_or("Invalid mask placement")?
                            .then(doc.affine_edit_transform(target).ok_or("Invalid layer placement")?.inverse().ok_or("Invalid layer placement")?);
                        mask.placement = layer_core::Projective::from_affine(to); mask.translation = Point::default(); mask.linked = false;
                        let coverage = CoverageSnapshot { target: mask.source, source: doc.artwork.coverage.get(mask.source).ok_or("Unknown mask")?.clone(), use_: mask };
                        let OccurrenceContent::Paint(handle) = layer.content else { unreachable!() };
                        let mut source = doc.artwork.paint.get(handle).ok_or("Unknown source")?.clone();
                        Arc::make_mut(&mut source.operations).push(layer_core::RasterOperation { placement: layer_core::Affine::IDENTITY, coverage, kind: layer_core::RasterOperationKind::ApplyMask });
                        edits.push(Edit::Paint(RecordChange::replace(&doc.artwork.paint, handle, Some(source))?));
                    }
                    LayerAction::EnableMask { value, .. } => { layer.mask.as_mut().ok_or("No mask")?.enabled = value; }
                    LayerAction::LinkMask { value, .. } => {
                        let owner = layer.clone(); layer.mask.as_mut().ok_or("No mask")?.set_linked(value, &owner).map_err(error)?;
                    }
                    LayerAction::ShowMask { value, .. } => {
                        let mask = layer.mask.as_ref().ok_or("No mask")?;
                        let mut working = self.engine.document().working.clone();
                        working.inspect_mask = value.then_some(id);
                        if value { working.occurrence = Some(id); working.target = Some(SourceTarget::Coverage(mask.source)); }
                        self.layer_edit(Edit::Working(working))?; return Ok(());
                    }
                    LayerAction::InvertMask { .. } => { let mask = layer.mask.as_mut().ok_or("No mask")?; mask.inverted = !mask.inverted; }
                    LayerAction::ClearMask { reveal, .. } => {
                        let mask = layer.mask.as_mut().ok_or("No mask")?;
                        let mut source = self.engine.document().artwork.coverage.get(mask.source).ok_or("Unknown mask")?.clone();
                        source.raster = Default::default(); source.operations = Arc::default(); source.initial = None; source.default_coverage = f32::from(reveal); mask.inverted = false;
                        edits.push(Edit::Coverage(RecordChange::replace(&self.engine.document().artwork.coverage, mask.source, Some(source))?));
                    }
                    _ => unreachable!(),
                }
                if let Some(source) = original_mask && layer.mask.as_ref().map(|mask| mask.source) != Some(source)
                    && !self.engine.document().artwork.occurrences.iter().any(|(h, _, o)| h != id && o.mask.as_ref().is_some_and(|mask| mask.source == source)) {
                    edits.push(Edit::Coverage(RecordChange::remove(&self.engine.document().artwork.coverage, source)?));
                }
                if self.engine.document().scene().occurrence(id) != Some(&layer) { edits.push(self.occurrence_change(id, layer)?); }
                self.layer_edit(Edit::Batch(edits))?;
            }
        }
        Ok(())
    }
    /// How a new group blends: Pass Through when the setting asks for it.
    pub(super) fn new_group_blend(&self) -> LayerBlend {
        if self.state.settings.pass_through_groups { LayerBlend::PassThrough } else { LayerBlend::Normal }
    }
    /// The layer's blend modes in `LayerBlend::MENU` groups, as check items.
    /// Float documents offer only modes defined above 1, and only groups offer
    /// Pass Through, plus the current one.
    fn blend_sections(&self, handle: OccurrenceHandle, layer: &Occurrence) -> Vec<Vec<ContextMenuItem>> {
        let doc = self.engine.document();
        let enabled = LayerControls::for_layer(doc, handle, layer).blend;
        let current = layer.blend;
        let float = doc.composition().color.depth.is_float();
        LayerBlend::MENU
            .iter()
            .map(|group| {
                group
                    .iter()
                    .filter(|b| b.offered(layer.kind(), float) || **b == current)
                    .map(|b| ContextMenuItem {
                        selected: Some(*b == current),
                        enabled,
                        ..ContextMenuItem::command(
                            super::effects::blend_label(*b, self.localization()).as_ref(),
                            UiAction::Layer { action: LayerAction::Blend { id: occurrence_token(handle), value: b.code() } },
                        )
                    })
                    .collect()
            })
            .filter(|items: &Vec<_>| !items.is_empty())
            .collect()
    }
    /// The grouped menu that the layer header's blend control opens.
    pub fn layer_blend_menu(&self, id: u64) -> Result<ContextMenu, String> {
        let handle = occurrence_handle(id)?;
        let layer = self.engine.document().scene().occurrence(handle).ok_or("Unknown layer")?;
        Ok(ContextMenu { title: self.localization().text(MessageId::RESOURCES_LAYER_MENU_BLEND_MODE).to_string(), sections: self.blend_sections(handle, layer) })
    }
    pub fn layer_menu(&self, id: u64, mask: bool) -> Result<ContextMenu, String> {
        if id == 0 && self.selection_masks.quick() { return Ok(self.quick_mask_menu()); }
        let handle = occurrence_handle(id)?;
        let layer = self.engine.document().scene().occurrence(handle).ok_or("Unknown layer")?;
        if layer.kind() == LayerKind::Selection { return self.selection_layer_menu(handle); }
        let mut args = FluentArgs::new(); args.set("name", layer.name.as_ref());
        let title = self.localization().format(if mask { MessageId::RESOURCES_LAYER_MENU_MASK_TITLE } else if layer.kind() == LayerKind::Group { MessageId::RESOURCES_LAYER_MENU_GROUP_TITLE } else { MessageId::RESOURCES_LAYER_MENU_LAYER_TITLE }, &args);
        Ok(ContextMenu { title, sections: self.layer_menu_sections(id, mask)? }
            .with_shortcuts_localized(&self.state.settings, self.state.platform, self.localization()))
    }
    pub(super) fn layer_menu_sections(&self, id: u64, mask: bool) -> Result<Vec<Vec<ContextMenuItem>>, String> {
        use LayerAction as A;
        if id == 0 && self.selection_masks.quick() { return Ok(self.quick_mask_menu().sections); }
        let doc = self.engine.document();
        let handle = occurrence_handle(id)?;
        let l = doc.scene().occurrence(handle).ok_or("Unknown layer")?;
        if l.kind() == LayerKind::Selection { return self.selection_layer_menu(handle).map(|menu| menu.sections); }
        let locked = doc.is_locked(handle);
        let paint = l.kind() == LayerKind::Paint;
        let controls = LayerControls::for_layer(doc, handle, l);
        let roots = doc.layer_roots(self.selected_layers());
        let multiple = roots.len() > 1;
        let parent = if l.kind() == LayerKind::Group {
            Some(handle)
        } else {
            doc.scene().parent(handle)
        };
        let item = |label: &str, action: A| {
            let enabled = match &action {
                A::RepairSourceProfile { .. } | A::RasterizeSource { .. } => self.can_edit_original(handle) && !self.state.document_file.busy,
                A::GroupSelected => doc.group_layers_edit(&roots, LayerBlend::Normal, "").is_ok(),
                A::Ungroup { .. } => doc.ungroup_layer_edit(handle).is_ok(),
                A::DeleteSelected => self.can_delete_layer_rows(self.selected_layers()),
                A::Delete { .. } => self.can_delete_layer_rows(&BTreeSet::from([handle])),
                A::DuplicateSelected => doc.duplicate_layers_edit(&roots).is_ok(),
                A::Duplicate { .. } => doc.duplicate_layers_edit(&[handle]).is_ok(),
                A::Select { .. } | A::ShowMask { .. } => true,
                A::CopyMask { .. } => l.mask.is_some(),
                A::PasteMask { .. } => {
                    !locked && self.copied_mask_use(handle).is_ok()
                }
                A::New { group: true, clipped: false } if self.selected_layers().len() > 1 => doc.group_layers_edit(&roots, self.new_group_blend(), "").is_ok(),
                A::New { clipped, .. } => {
                    !parent.is_some_and(|p| doc.is_locked(p))
                        && (!clipped
                            || l.kind() == LayerKind::Paint || l.kind() == LayerKind::Group && !l.passes_through())
                }
                A::Reference { .. } => matches!(l.kind(), LayerKind::Paint | LayerKind::Group),
                A::ReferenceSelection => !self.reference_selection().is_empty(),
                A::Lock { .. } => controls.edit_lock,
                A::AlphaLock { .. } | A::Clear { .. } => controls.alpha_lock,
                A::MaskSelection { .. } => !locked && doc.working.selection.is_some(),
                A::ApplyMask { .. } => {
                    paint && !locked && l.mask.as_ref().is_some_and(|m| m.enabled)
                }
                A::InvertSelection | A::Deselect => doc.working.selection.is_some(),
                A::FillSelection => controls.fill && doc.working.selection.is_some(),
                A::Tool {
                    tool: LayerCanvasTool::LassoFill,
                } => controls.fill,
                A::Tool {
                    tool: LayerCanvasTool::Select,
                } => true,
                A::Visibility { .. }
                | A::ShowParents { .. }
                | A::ShowAll
                | A::SelectAllLayers { .. } => true,
                A::SoloSelected => !roots.is_empty() || doc.working.solo_visibility.is_some(),
                _ => !locked,
            };
            let mut item = ContextMenuItem::command(label, UiAction::Layer { action });
            item.enabled = enabled;
            item
        };
        let check = |label: &str, action, checked| {
            let mut item = item(label, action);
            item.selected = Some(checked);
            item
        };
        let routed = |label: &str, command: CommandId, action: A| {
            if Some(handle) != doc.working.occurrence {
                return item(label, action);
            }
            let state = self.command(command);
            ContextMenuItem {
                enabled: state.enabled,
                selected: command.is_toggle().then_some(state.selected),
                ..ContextMenuItem::command(label, UiAction::Invoke { command })
            }
        };
        let mask_selection = || {
            vec![
                item(
                    self.localization().text(if l.mask.is_some() { MessageId::RESOURCES_LAYER_MENU_REPLACE_MASK_REVEAL_SELECTION } else { MessageId::RESOURCES_LAYER_MENU_MASK_REVEAL_SELECTION }).as_ref(),
                    A::MaskSelection { id, hide: false },
                ),
                item(
                    self.localization().text(if l.mask.is_some() { MessageId::RESOURCES_LAYER_MENU_REPLACE_MASK_HIDE_SELECTION } else { MessageId::RESOURCES_LAYER_MENU_MASK_HIDE_SELECTION }).as_ref(),
                    A::MaskSelection { id, hide: true },
                ),
            ]
        };
        let mut sections = if mask {
            let m = l.mask.as_ref().ok_or("No mask")?;
            vec![
                vec![
                    routed(self.localization().text(MessageId::RESOURCES_LAYER_MENU_EDIT_LAYER_CONTENT).as_ref(), CommandId::EditLayerContent, A::Select { id, mask: false }),
                    check(
                        self.localization().text(MessageId::RESOURCES_LAYER_MENU_SHOW_MASK_AREA).as_ref(),
                        A::ShowMask {
                            id,
                            value: !(doc.working.inspect_mask == Some(handle)),
                        },
                        doc.working.inspect_mask == Some(handle),
                    ),
                    ContextMenuItem {
                        selected: Some(m.enabled),
                        ..routed(
                            self.localization().text(MessageId::RESOURCES_LAYER_MENU_ENABLE_MASK).as_ref(),
                            CommandId::LayerMaskEnabled,
                            A::EnableMask {
                                id,
                                value: !m.enabled,
                            },
                        )
                    },
                    check(
                        self.localization().text(MessageId::RESOURCES_LAYER_MENU_LINK_MASK_TO_LAYER).as_ref(),
                        A::LinkMask {
                            id,
                            value: !m.linked,
                        },
                        m.linked,
                    ),
                ],
                mask_selection(),
                vec![
                    item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_COPY_MASK).as_ref(), A::CopyMask { id }),
                    item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_REPLACE_WITH_COPIED_MASK).as_ref(), A::PasteMask { id }),
                    routed(self.localization().text(MessageId::RESOURCES_LAYER_MENU_INVERT_MASK).as_ref(), CommandId::InvertLayerMask, A::InvertMask { id }),
                    item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_REVEAL_ALL).as_ref(), A::ClearMask { id, reveal: true }),
                    item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_HIDE_ALL).as_ref(), A::ClearMask { id, reveal: false }),
                ],
                vec![
                    routed(self.localization().text(MessageId::RESOURCES_LAYER_MENU_APPLY_MASK_TO_LAYER).as_ref(), CommandId::ApplyLayerMask, A::ApplyMask { id }),
                    item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_DELETE_MASK).as_ref(), A::DeleteMask { id }),
                ],
            ]
        } else {
            let mut organization = vec![
                item(
                    self.localization().text(if l.kind() == LayerKind::Group { MessageId::RESOURCES_LAYER_MENU_RENAME_GROUP } else { MessageId::RESOURCES_LAYER_MENU_RENAME_LAYER }).as_ref(),
                    A::BeginRename { id },
                ),
                item(
                    self.localization().text(if multiple { MessageId::RESOURCES_LAYER_MENU_DUPLICATE_SELECTED_LAYERS } else { MessageId::RESOURCES_LAYER_MENU_DUPLICATE }).as_ref(),
                    if multiple {
                        A::DuplicateSelected
                    } else {
                        A::Duplicate { id }
                    },
                ),
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_GROUP_SELECTED_LAYERS).as_ref(), A::GroupSelected),
            ];
            if l.kind() == LayerKind::Group {
                organization.push(item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_UNGROUP).as_ref(), A::Ungroup { id }));
            }
            let mask_menu = if l.mask.is_some() {
                let mut sections = self.layer_menu_sections(id, true)?;
                sections[0][0] = routed(self.localization().text(MessageId::RESOURCES_LAYER_MENU_EDIT_MASK).as_ref(), CommandId::EditLayerMask, A::Select { id, mask: true });
                sections
            } else {
                vec![
                    vec![item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_ADD_MASK).as_ref(), A::AddMask { id, replace: false })],
                    mask_selection(),
                    vec![item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_PASTE_MASK).as_ref(), A::PasteMask { id })],
                ]
            };
            let rows = vec![vec![
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_SELECT_ALL_LAYER_ROWS).as_ref(), A::SelectAllLayers { selected: true }),
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_CLEAR_LAYER_ROW_SELECTION).as_ref(), A::SelectAllLayers { selected: false }),
            ]];
            let mut selection = vec![vec![
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_FILL_SELECTION).as_ref(), A::FillSelection),
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_INVERT_SELECTION).as_ref(), A::InvertSelection), item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_DESELECT_PIXELS).as_ref(), A::Deselect),
            ]];
            if l.kind() == LayerKind::Paint {
                selection.insert(0,self.coverage_menu_items(id,false));
            }
            let visibility = vec![vec![
                check(
                    self.localization().text(MessageId::RESOURCES_LAYER_MENU_SHOW_LAYER).as_ref(),
                    A::Visibility {
                        id,
                        value: !doc.effective_visibility(handle),
                    },
                    doc.effective_visibility(handle),
                ),
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_SHOW_LAYER_AND_PARENT_GROUPS).as_ref(), A::ShowParents { id }),
                check(
                    self.localization().text(MessageId::RESOURCES_LAYER_MENU_ISOLATE_SELECTED_LAYERS).as_ref(),
                    A::SoloSelected,
                    doc.working.solo_visibility.is_some(),
                ),
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_SHOW_ALL_LAYERS).as_ref(), A::ShowAll),
            ]];
            let mut protection = Vec::new();
            if paint {
                protection.push(check(
                    self.localization().text(MessageId::RESOURCES_LAYER_MENU_ALPHA_LOCK).as_ref(),
                    A::AlphaLock {
                        id,
                        value: !l.alpha_locked,
                    },
                    l.alpha_locked,
                ));
            }
            protection.push(check(
                self.localization().text(MessageId::RESOURCES_LAYER_MENU_LOCK_EDITING).as_ref(),
                A::Lock { id, value: !l.locked },
                l.locked,
            ));
            if let Some(mode) = crate::layer_relationships::group_mode_menu(doc, handle, self.localization()) {
                protection.push(mode);
            }
            protection.push(crate::layer_relationships::attachment_control(doc, Some(handle), self.localization()).menu_item());
            protection.push(if multiple {
                item(self.localization().text(MessageId::RESOURCES_LAYER_MENU_USE_SELECTED_LAYERS_AS_REFERENCES).as_ref(), A::ReferenceSelection)
            } else {
                check(self.localization().text(MessageId::RESOURCES_LAYER_MENU_USE_AS_REFERENCE).as_ref(), A::Reference { id }, l.reference)
            });
            if Some(handle) == doc.working.occurrence {
                let state = self.command(CommandId::ApplyTransformPixels);
                let mut apply = ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command: state.id });
                apply.enabled = state.enabled;
                protection.push(apply);
                let state = self.command(CommandId::UseReferenceBelow);
                let mut below = ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command: state.id });
                below.enabled = state.enabled;
                protection.push(below);
            }
            if doc.scene().paint_source(handle).and_then(|s| s.original.as_ref()).is_some_and(|s| s.is_original()) {
                protection.push(item(self.localization().text(MessageId::COMMAND_REPAIR_SOURCE_PROFILE).as_ref(), A::RepairSourceProfile { id }));
                protection.push(item(self.localization().text(MessageId::COMMAND_RASTERIZE_SOURCE).as_ref(), A::RasterizeSource { id }));
                if Some(handle) == doc.working.occurrence {
                    let state = self.command(CommandId::RevertToOriginal);
                    let mut revert = ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command: state.id });
                    revert.enabled = state.enabled;
                    protection.push(revert);
                }
            }
            let mut destructive = Vec::new();
            if paint {
                destructive.push(item(CommandId::ClearLayer.localized_label(self.localization()).as_ref(), A::Clear { id }));
            }
            destructive.push(item(
                self.localization().text(if multiple { MessageId::RESOURCES_LAYER_MENU_DELETE_SELECTED_LAYERS } else if l.kind() == LayerKind::Group && self.layer_interaction.collapsed.contains(&handle) { MessageId::RESOURCES_LAYER_MENU_DELETE_GROUP_AND_CONTENTS } else { MessageId::COMMAND_DELETE_LAYER }).as_ref(),
                if multiple {
                    A::DeleteSelected
                } else {
                    A::Delete { id }
                },
            ));
            let mut new = vec![vec![
                item(
                    self.localization().text(MessageId::COMMAND_ADD_LAYER).as_ref(),
                    A::New {
                        group: false,
                        clipped: false,
                    },
                ),
                item(
                    self.localization().text(MessageId::RESOURCES_LAYER_MENU_NEW_CLIPPING_LAYER).as_ref(),
                    A::New {
                        group: false,
                        clipped: true,
                    },
                ),
                item(
                    self.localization().text(MessageId::RESOURCES_LAYER_MENU_NEW_GROUP).as_ref(),
                    A::New {
                        group: true,
                        clipped: false,
                    },
                ),
            ]];
            if Some(handle) == doc.working.occurrence {
                new.push(self.fill_layer_items());
                new.push(
                    [CommandId::CopySelectionToLayer, CommandId::CutSelectionToLayer]
                        .map(|command| {
                            let state = self.command(command);
                            let mut item = ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command });
                            item.enabled = state.enabled;
                            item
                        })
                        .into(),
                );
            }
            vec![
                vec![ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_NEW).as_ref(), new)],
                vec![
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_ORGANIZE).as_ref(), vec![organization]),
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_BLEND_MODE).as_ref(), self.blend_sections(handle, l)),
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_LAYER_SETTINGS).as_ref(), vec![protection]),
                ],
                vec![
                    ContextMenuItem::submenu(self.localization().text(MessageId::TOOLBAR_MASK).as_ref(), mask_menu),
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_PIXEL_SELECTION).as_ref(), selection),
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_LAYER_ROW_SELECTION).as_ref(), rows),
                    ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_VISIBILITY).as_ref(), visibility),
                    item(
                        self.localization().text(MessageId::RESOURCES_LAYER_MENU_MOVE_LAYER_MASK).as_ref(),
                        A::Tool {
                            tool: LayerCanvasTool::Move,
                        },
                    ),
                ],
                self.merge_menu_items(handle),
                destructive,
            ]
        };
        if mask { sections.push(vec![ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_LAYER_MENU_PIXEL_SELECTION).as_ref(),vec![self.coverage_menu_items(id,true)])]); }
        if l.kind() == LayerKind::Group {
            sections.insert(0, vec![
                ContextMenuItem { enabled: !locked, ..ContextMenuItem::command(self.localization().text(MessageId::RESOURCES_LAYER_MENU_NEW_SELECTION_LAYER_IN_GROUP).as_ref(), UiAction::Selection { action: SelectionAction::NewLayer { parent: Some(id), save_current: false } }) },
                ContextMenuItem { enabled: !locked && self.current_selection().is_some(), ..ContextMenuItem::command(self.localization().text(MessageId::RESOURCES_LAYER_MENU_SAVE_CURRENT_SELECTION_IN_GROUP).as_ref(), UiAction::Selection { action: SelectionAction::NewLayer { parent: Some(id), save_current: true } }) },
            ]);
        }
        Ok(sections)
    }
    pub(super) fn layer_pen(&mut self, event: PenEvent) -> Result<(), String> {
        let p = self
            .state
            .camera
            .input_transform()
            .map(event.surface_position);
        if event.phase != PenPhase::Cancel && (!p.x.is_finite() || !p.y.is_finite()) {
            return Err("Invalid canvas point".into());
        }
        if let Some(moving) = &mut self.content_bounds.moving {
            match event.phase {
                PenPhase::Move | PenPhase::Up => { moving.latest = Some((event, p)); return Ok(()); }
                PenPhase::Cancel => { self.cancel_content_bounds(); return Ok(()); }
                PenPhase::Down => { self.cancel_content_bounds(); }
                PenPhase::Hover => return Ok(()),
            }
        }
        if self.tonal_active() {return self.tonal_pen(event,p);}
        if let LayerCanvasTool::Selection { kind } = self.layer_interaction.tool {
            return self.selection_pen(event, p, kind);
        }
        if matches!(self.layer_interaction.tool, LayerCanvasTool::Ruler { .. }) {
            return self.ruler_pen(event, p);
        }
        if self.layer_interaction.tool == LayerCanvasTool::Transform || self.operation.moving_pixels() || self.operation.moving_layer() {
            return self.transform_pen(event, p);
        }
        if self.layer_interaction.tool == LayerCanvasTool::Crop {
            return self.crop_pen(event, p);
        }
        if self.layer_interaction.tool == LayerCanvasTool::Move
            && self.operation_ruler_pen(event, p)?
        {
            return Ok(());
        }
        if event.phase == PenPhase::Down && self.layer_interaction.tool == LayerCanvasTool::Move {
            if let Some(reason) = self.move_refusal() {
                self.notify(reason);
                return Ok(());
            }
            if self.moves_selected_pixels() || !matches!(self.engine.document().working.target, Some(SourceTarget::Coverage(_))) {
                let keep_source = self.operation.leave_copy != self.interaction.modifiers.alt;
                return self.begin_move_transform(p, keep_source);
            }
        }
        match event.phase {
            PenPhase::Down => {
                self.layer_interaction.path.clear();
                let doc = self.engine.document();
                let id = doc.working.occurrence.ok_or("Unknown layer")?;
                let layer = doc.scene().occurrence(id).ok_or("Unknown layer")?;
                match self.layer_interaction.tool {
                    LayerCanvasTool::LassoFill | LayerCanvasTool::Gradient { .. } | LayerCanvasTool::Figure { .. }
                        if doc.drawing_content().is_none() && self.selection_masks.target().is_none() =>
                    {
                        self.notify_drawing_refusal();
                        return Ok(());
                    }
                    _ => (),
                }
                self.layer_interaction.path.push(p);
                self.layer_interaction.original =
                    (self.layer_interaction.tool == LayerCanvasTool::Move).then(|| (id, layer.clone()));
            }
            PenPhase::Move | PenPhase::Up => {
                if self.layer_interaction.path.is_empty() {
                    return Ok(());
                }
                if matches!(
                    self.layer_interaction.tool,
                    LayerCanvasTool::Gradient { .. } | LayerCanvasTool::Figure { .. }
                ) && self.layer_interaction.path.len() == 2
                {
                    self.layer_interaction.path[1] = p;
                } else {
                    self.layer_interaction.path.push(p);
                }
                if self.layer_interaction.tool == LayerCanvasTool::Move {
                    let start = self.layer_interaction.path[0];
                    let (id, original) = self.layer_interaction.original.as_ref().unwrap().clone();
                    let rollback = self.occurrence_change(id, original.clone())?;
                    let mut candidate = self.engine.document().clone();
                    candidate.apply(rollback.clone()).map_err(error)?;
                    let targets: Vec<_> = candidate.layer_subtrees(&[id]).into_iter().flat_map(|h| {
                        let scene = candidate.scene();
                        scene.source_target(h).into_iter().chain(scene.mask(h).map(|(mask, _)| SourceTarget::Coverage(mask.source)))
                    }).collect();
                    let planned = (|| {
                        let edit = candidate.move_target_edit(Point { x: p.x - start.x, y: p.y - start.y }).map_err(error)?;
                        candidate.apply(edit.clone()).map_err(error)?;
                        candidate.validate_paint_extents(&targets, self.engine.geometry_limits()).map_err(error)?;
                        Ok::<_, String>(edit)
                    })();
                    let edit = match planned {
                        Ok(edit) => edit,
                        Err(cause) if event.phase == PenPhase::Up => {
                            self.notify(cause);
                            candidate = self.engine.document().clone();
                            self.occurrence_change(id, candidate.scene().occurrence(id).ok_or("Unknown layer")?.clone())?
                        }
                        Err(cause) => return Err(cause),
                    };
                    let moved = candidate.scene().occurrence(id) != Some(&original);
                    let edit = if moved && event.phase == PenPhase::Up {
                        let mut edits = vec![edit];
                        edits.extend(candidate.paint_extent_plan(&targets, self.engine.geometry_limits()).map_err(error)?);
                        Edit::Batch(edits)
                    } else { edit };
                    if event.phase == PenPhase::Up || candidate.artwork != self.engine.document().artwork {
                        if self.engine.document().scene().occurrence(id) != Some(&original) {
                            self.engine.preview_edit(rollback).map_err(error)?;
                        }
                        if moved {
                            if event.phase == PenPhase::Up { self.layer_edit(edit)?; }
                            else { self.engine.preview_edit(edit).map_err(error)?; }
                        }
                    }
                }
                if event.phase == PenPhase::Up {
                    if matches!(self.layer_interaction.tool, LayerCanvasTool::Figure { .. }) {
                        self.commit_figure()?;
                    }
                    if let LayerCanvasTool::Gradient {..}=self.layer_interaction.tool {
                        let start=self.layer_interaction.path[0];
                        if start!=p {self.gradient_fill(start,p)?;}
                    }
                    if matches!(
                        self.layer_interaction.tool,
                        LayerCanvasTool::Select | LayerCanvasTool::LassoFill
                    ) {
                        let mut points = std::mem::take(&mut self.layer_interaction.path);
                        points.dedup();
                        // A click (including repeated stationary samples) does
                        // not enclose an area or replace the current selection.
                        if points.len() >= 3 {
                            let selection = Selection::polygon(points).map_err(error)?;
                            if self.layer_interaction.tool == LayerCanvasTool::Select {
                                self.commit_tool_selection(selection)?;
                            } else {
                                self.fill_selection(selection)?;
                            }
                        }
                    }
                    self.layer_interaction.path.clear();
                    self.layer_interaction.original = None;
                }
            }
            PenPhase::Cancel => {
                self.cancel_layer_gesture()?;
            }
            PenPhase::Hover => {}
        }
        if !matches!(event.phase, PenPhase::Move | PenPhase::Hover) {
            self.layer_interaction.changed = true;
        }
        Ok(())
    }

    /// Cancel the contact in progress. An open transform or placement session
    /// survives; only its current handle drag is rolled back.
    pub(super) fn cancel_layer_contact(&mut self) -> Result<bool, String> {
        if self.operation.active() {
            return self.cancel_transform_drag();
        }
        self.cancel_layer_gesture()
    }

    pub(super) fn cancel_layer_gesture(&mut self) -> Result<bool, String> {
        if self.cancel_selection_contact() { return Ok(true); }
        let tonal=self.cancel_tonal();
        let effect = self.cancel_effect_gesture()?;
        let sdr = self.cancel_sdr_gesture()?;
        let transform = self.cancel_transform()?;
        self.cancel_ruler_gesture();
        let selection = self.selection_tools.cancel();
        let region = self.region_tools.cancellable();
        self.region_tools.cancel();
        if self.layer_interaction.path.is_empty() {
            return Ok(tonal || selection || region || transform || effect || sdr);
        }
        if let Some((id, original)) = self.layer_interaction.original.take() {
            self.engine
                .preview_edit(self.occurrence_change(id, original)?)
                .map_err(error)?;
        }
        self.layer_interaction.path.clear();
        self.layer_interaction.changed = true;
        Ok(true)
    }

    pub(super) fn fill_selection(&mut self, selection: Selection) -> Result<(), String> {
        self.paint_operation(
            Some(selection),
            self.fill_operation(),
            &[self.state.colors.definition()],
        )
    }

    pub(super) fn fill_operation(&self) -> layer_core::RasterOperationKind {
        let brush = self.engine.brush();
        let mut color = brush.color_rgba_linear;
        color[3] *= brush.opacity;
        layer_core::RasterOperationKind::Fill {
            color,
            alpha_locked: self
                .engine
                .document()
                .drawing_content().and_then(|target| self.engine.document().target_owner(target))
                .and_then(|id| self.engine.document().scene().occurrence(id)).is_some_and(|l| l.alpha_locked),
        }
    }

    fn gradient_fill(&mut self,start:Point,end:Point)->Result<(),String> {
        let gradient=self.tool_gradient();
        let shape=self.layer_interaction.gradient.shape;
        let reverse=false;
        if let Some(target)=self.selection_masks.target() {
            let stops=gradient.stops.iter().map(|stop|layer_core::ScalarGradientStop {position:stop.position,
                value:if self.grayscale_masks() {selection_masks::mask_gray(stop.color)} else {1.},opacity:stop.color.rgba[3]}).collect();
            return self.queue_mask_gradient(target,layer_render::SelectionGradient {start,end,shape,reverse,
                gradient:layer_core::ScalarGradient {stops}});
        }
        let doc=self.engine.document();
        let target=doc.drawing_content().ok_or("Select a drawing layer")?;
        let colors:Vec<_>=gradient.stops.iter().map(|stop|stop.color).collect();
        let kind=layer_core::RasterOperationKind::Gradient {start,end,gradient,shape,reverse,
            opacity:self.state.brush.opacity,alpha_locked:doc.target_owner(target).and_then(|id|doc.scene().occurrence(id)).is_some_and(|layer|layer.alpha_locked)};
        self.paint_operation(doc.working.selection.clone(),kind,&colors)
    }

    pub(super) fn paint_operation(
        &mut self,
        selection: Option<Selection>,
        kind: layer_core::RasterOperationKind,
        colors: &[layer_core::color::RgbColor],
    ) -> Result<(), String> {
        let id = self.engine.document().drawing_content().ok_or("Select a drawing layer")?;
        let owner = self.engine.document().target_owner(id).ok_or("Unknown layer")?;
        let layer = self.editable_layer(occurrence_token(owner))?;
        if layer.kind() != LayerKind::Paint {
            return Err("Select a paint layer's content to fill".into());
        }
        let offset = self.engine.document().target_offset(id);
        let inverse = self.engine.document().affine_edit_transform(id).and_then(layer_core::Affine::inverse).ok_or("Invalid layer placement")?;
        let placement = if matches!(kind,layer_core::RasterOperationKind::Gradient {..}) {inverse} else {layer_core::Affine::translation(offset).then(inverse)};
        let domain = self.engine.document().target_extent(id);
        let mut coverage = CoverageSnapshot::reveal_all(self.engine.allocate_coverage_handle(), domain, Point::default());
        let doc = self.engine.document();
        let [w, h] = doc.composition().size.map(|v| v as f32);
        let hidden = doc.target_extent(id) != doc.composition().size || doc.affine_edit_transform(id) != Some(layer_core::Affine::IDENTITY);
        let selection = match selection {
            None if hidden => Some(
                Selection::polygon([[0., 0.], [w, 0.], [w, h], [0., h]].map(|[x, y]| Point { x, y }).to_vec())
                    .map_err(error)?,
            ),
            selection => selection,
        };
        if let Some(selection) = selection {
            coverage.source.default_coverage = f32::from(selection.inverted);
            coverage.source.initial = Some(selection.transformed(inverse).map_err(error)?);
        }
        let paints = match &kind {
            layer_core::RasterOperationKind::Fill { color, .. } => color[3] > 0.,
            layer_core::RasterOperationKind::Gradient { gradient, opacity, .. } => {
                *opacity>0. && gradient.stops.iter().any(|stop| stop.color.rgba[3]>0.)
            }
            layer_core::RasterOperationKind::Figure(figure) => {
                !figure.erase && figure.colors.iter().any(|c| c[3] > 0.)
            }
            _ => false,
        };
        self.engine.append_raster_operation(id, layer_core::RasterOperation { placement, coverage, kind }).map_err(error)?;
        if paints {
            for color in colors.iter().rev() {
                self.state.color_library.record_use(*color);
            }
        }
        self.layer_interaction.changed = true;
        Ok(())
    }

    pub fn append_layer_overlay(&self, segments: &mut Vec<layer_render::CursorSegment>) {
        self.append_ruler_overlay(segments);
        self.append_transform_overlay(segments);
        self.append_crop_overlay(segments);
        self.append_clone_overlay(segments);
        let transform = self.document_to_logical();
        let mut path = |points: &[Point], closed: bool, affine: layer_core::Affine| {
            let mut distance = 0.;
            for (a, b) in points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .take(if closed {
                    points.len()
                } else {
                    points.len().saturating_sub(1)
                })
            {
                let from = transform(affine.map(*a));
                let to = transform(affine.map(*b));
                segments.push(layer_render::CursorSegment {
                    from,
                    to,
                    distance,
                    marker: 0.,
                    scale: 1.,
                });
                distance += (to[0] - from[0]).hypot(to[1] - from[1]);
            }
        };
        if !self.selection_brush_active() && self.selection_masks.target().is_none()
            && (self.selection_tools.options.display.outline || self.operation.outline())
            && let Some(selection) = self.engine.display_selection() {
            for contour in selection.contours() {
                path(contour, true, selection.affine);
            }
        }
        if matches!(
            self.layer_interaction.tool,
            LayerCanvasTool::Select | LayerCanvasTool::LassoFill | LayerCanvasTool::Gradient { .. }
        ) {
            path(
                &self.layer_interaction.path,
                false,
                layer_core::Affine::IDENTITY,
            );
        }
        let outline = self.selection_outline();
        if !outline.is_empty() {
            path(&outline, true, layer_core::Affine::IDENTITY);
        }
        if let Some(figure) = self.current_figure() {
            let guide = figure
                .shape
                .guide(figure.start, figure.end, self.state.camera.zoom);
            let offset = self
                .engine
                .document()
                .working.occurrence.map(|id| self.engine.document().layer_offset(id)).unwrap_or_default();
            path(
                &guide,
                figure.shape != FigureShape::Line,
                layer_core::Affine::translation(offset),
            );
        }
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn image_drop_destinations_use_panel_tokens_and_reserve_zero_for_quick_mask() {
        let destination = ImageLayerDestination { target: OccurrenceHandle::from_index(0), position: LayerDropPosition::Above };
        let value = serde_json::to_value(destination).unwrap();
        assert_eq!(value, serde_json::json!({"target":1,"position":"above"}));
        assert_eq!(serde_json::from_value::<ImageLayerDestination>(value).unwrap(), destination);
        assert!(serde_json::from_value::<ImageLayerDestination>(serde_json::json!({"target":0,"position":"above"})).is_err());
    }
}
