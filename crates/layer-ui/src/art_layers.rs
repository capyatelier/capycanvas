//! Artwork commands and gestures. Native layer panels only render this model.
use super::*;
use layer_core::{Edit, Layer, LayerMask, Point, Selection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayerCanvasTool {
    #[default]
    Paint,
    Move,
    Transform,
    Select,
    LassoFill,
    Hand,
    PickVisible,
    PickLayer,
    Region {
        fill: bool,
        source: RegionSource,
    },
    Gradient {
        radial: bool,
        transparent: bool,
    },
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
    pub fn picks_color(self) -> bool {
        matches!(self, Self::PickVisible | Self::PickLayer)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayersView {
    pub tool: LayerCanvasTool,
    pub has_selection: bool,
    pub can_reference: bool,
    pub can_delete: bool,
    pub references_selected: bool,
    pub reference_action_label: &'static str,
    /// Header target remains available even inside a collapsed group.
    pub editing_layer: Option<LayerState>,
    pub rename_layer: Option<u64>,
    pub controls: LayerControls,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct LayerControls {
    pub opacity: bool,
    pub blend: bool,
    pub alpha_lock: bool,
    pub edit_lock: bool,
    pub clip: bool,
    pub mask: bool,
    pub move_layer: bool,
    pub fill: bool,
}
impl LayerControls {
    pub(super) fn for_layer(doc: &Document, l: &Layer) -> Self {
        let unlocked = !doc.is_locked(l.id);
        let editable = l.kind != LayerKind::Background;
        Self {
            opacity: unlocked,
            blend: editable && unlocked,
            alpha_lock: l.kind == LayerKind::Paint && unlocked,
            edit_lock: editable && !l.properties.parent.is_some_and(|p| doc.is_locked(p)),
            clip: editable
                && unlocked
                && (l.properties.clipped || doc.clipping_base(l.id).is_some()),
            mask: editable && unlocked,
            move_layer: editable && unlocked,
            fill: l.kind == LayerKind::Paint && unlocked && !doc.active_mask,
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
    Blend {
        id: u64,
        value: u32,
    },
    Reference {
        id: u64,
    },
    Solo {
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
    },
}
#[derive(Default)]
pub(super) struct LayerInteraction {
    pub tool: LayerCanvasTool,
    pub collapsed: BTreeSet<LayerId>,
    pub selected: BTreeSet<LayerId>,
    pub editing: Option<LayerId>,
    clipboard_mask: Option<(LayerMask, Point, Vec<layer_core::Stroke>)>,
    pub path: Vec<Point>,
    original: Option<Layer>,
    solo: Option<Vec<(LayerId, bool)>>,
    pub changed: bool,
    pub gradient: [bool; 2],
    pub figure: (FigureShape, FigurePaint),
}
impl LayerInteraction {
    pub fn depth(&self, doc: &Document, l: &Layer) -> u32 {
        let mut parent = l.properties.parent;
        let mut depth = 0;
        while let Some(p) = parent.and_then(|id| doc.layer(id)) {
            depth += 1;
            if depth > 32 {
                break;
            }
            parent = p.properties.parent;
        }
        depth
    }
    pub fn hidden_by_group(&self, doc: &Document, l: &Layer) -> bool {
        let mut parent = l.properties.parent;
        for _ in 0..doc.layers.len() {
            let Some(id) = parent else { return false };
            if self.collapsed.contains(&id) {
                return true;
            }
            parent = doc.layer(id).and_then(|l| l.properties.parent);
        }
        true
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn reference_action_removes(&self) -> bool {
        let doc = self.engine.document();
        self.layer_interaction.selected.len() == 1
            && self.layer_interaction.selected.contains(&doc.active_layer)
            && doc.reference_layers.contains(&doc.active_layer)
    }
    pub(super) fn reference_selection(&self) -> BTreeSet<LayerId> {
        self.layer_interaction
            .selected
            .iter()
            .copied()
            .filter(|id| {
                self.engine.document().layer(*id).is_some_and(|l| {
                    matches!(
                        l.kind,
                        LayerKind::Paint | LayerKind::ImportedImage | LayerKind::Group
                    )
                })
            })
            .collect()
    }
    /// Hosts decode a file; ownership, placement and undo remain shared policy.
    pub fn import_layer_image(
        &mut self,
        name: &str,
        image: layer_render::HostImage<'_>,
    ) -> Result<(), String> {
        self.require_idle()?;
        if image.format != layer_render::PixelFormat::Rgba8Srgb
            || image.width == 0
            || image.height == 0
            || image.width > 8192
            || image.height > 8192
            || image.stride < image.width * 4
            || image.bytes.len() < image.stride as usize * image.height as usize
        {
            return Err("Import an image up to 8192 × 8192 pixels".into());
        }
        let id = self.engine.allocate_layer_id();
        let asset = layer_core::AssetId::from(format!("document:image/{}", id.0).as_str());
        self.renderer_mut()
            .prepare_asset(&asset, image)
            .map_err(error)?;
        let doc = self.engine.document();
        let current = doc.layer(doc.active_layer).ok_or("Unknown layer")?;
        let index = doc.layers.iter().position(|l| l.id == current.id).unwrap();
        let mut layer = Layer::paint(id, name.trim().chars().take(128).collect::<String>());
        layer.properties.parent = current.properties.parent;
        layer.asset = Some(asset);
        self.layer_edit(Edit::Batch(vec![
            Edit::InsertLayer { index, layer },
            Edit::SetActiveLayer { id },
        ]))?;
        self.refresh_document();
        self.layer_interaction.changed = true;
        Ok(())
    }
    pub(super) fn layer_edit(&mut self, edit: Edit) -> Result<(), String> {
        self.engine.apply_edit(edit).map_err(error)
    }
    pub(super) fn editable_layer(&self, id: u64) -> Result<Layer, String> {
        let layer = self
            .engine
            .document()
            .layer(LayerId(id))
            .ok_or("Unknown layer")?;
        if self.engine.document().is_locked(layer.id) {
            return Err("This layer is locked".into());
        }
        if layer.kind == LayerKind::Background {
            return Err("The background is not editable".into());
        }
        Ok(layer.clone())
    }
    pub(super) fn layer_action(&mut self, action: LayerAction) -> Result<(), String> {
        let hide_selection = matches!(action, LayerAction::MaskSelection { hide: true, .. });
        let action = if let LayerAction::MaskSelection { id, .. } = action {
            if self.engine.document().selection.is_none() {
                return Err("Make a selection first".into());
            }
            LayerAction::AddMask { id, replace: true }
        } else {
            action
        };
        match action {
            LayerAction::Visibility { id, value } => self.layer_edit(Edit::SetLayerVisibility {
                id: LayerId(id),
                visible: value,
            })?,
            LayerAction::ShowParents { id } => {
                let mut parent = Some(LayerId(id));
                let mut edits = Vec::new();
                while let Some(id) = parent {
                    let layer = self.engine.document().layer(id).ok_or("Unknown layer")?;
                    edits.push(Edit::SetLayerVisibility { id, visible: true });
                    parent = layer.properties.parent;
                }
                self.layer_edit(Edit::Batch(edits))?;
            }
            LayerAction::ShowAll => self.layer_edit(Edit::Batch(
                self.engine
                    .document()
                    .layers
                    .iter()
                    .map(|l| Edit::SetLayerVisibility {
                        id: l.id,
                        visible: true,
                    })
                    .collect(),
            ))?,
            LayerAction::Context { id, mask } => {
                let selected = self.layer_interaction.selected.clone();
                self.layer_action(LayerAction::Select { id, mask })?;
                self.layer_interaction.editing = Some(LayerId(id));
                if selected.contains(&LayerId(id)) {
                    self.layer_interaction.selected = selected;
                }
            }
            LayerAction::BeginRename { id } => {
                self.editable_layer(id)?;
                self.state.layer_tools.rename_layer = Some(id);
            }
            LayerAction::CancelRename => self.state.layer_tools.rename_layer = None,
            LayerAction::SelectAllLayers { selected } => {
                self.layer_interaction.selected = self
                    .engine
                    .document()
                    .layers
                    .iter()
                    .filter(|l| selected && l.kind != LayerKind::Background)
                    .map(|l| l.id)
                    .collect();
            }
            LayerAction::GroupSelected => {
                let roots = self
                    .engine
                    .document()
                    .layer_roots(&self.layer_interaction.selected);
                let id = self.engine.allocate_layer_id();
                let edit = self
                    .engine
                    .document()
                    .group_layers_edit(&roots, id)
                    .map_err(error)?;
                self.layer_edit(edit)?;
                self.layer_interaction.selected = BTreeSet::from([id]);
            }
            LayerAction::Ungroup { id } => {
                let doc = self.engine.document();
                let children = doc
                    .layers
                    .iter()
                    .filter(|l| l.properties.parent == Some(LayerId(id)))
                    .map(|l| l.id)
                    .collect();
                let edit = doc.ungroup_layer_edit(LayerId(id)).map_err(error)?;
                self.layer_edit(edit)?;
                self.layer_interaction.editing = Some(self.engine.document().active_layer);
                self.layer_interaction.selected = children;
                self.layer_interaction.collapsed.remove(&LayerId(id));
            }
            LayerAction::DeleteSelected => {
                let doc = self.engine.document();
                let edit = doc
                    .delete_layers_edit(&doc.layer_roots(&self.layer_interaction.selected))
                    .map_err(error)?;
                self.layer_edit(edit)?;
            }
            LayerAction::CopyMask { id } => {
                let doc = self.engine.document();
                let mask = doc
                    .layer(LayerId(id))
                    .and_then(|l| l.mask.clone())
                    .ok_or("No mask")?;
                let strokes = mask
                    .strokes
                    .iter()
                    .map(|id| doc.stroke(*id).cloned().ok_or("Missing mask stroke"))
                    .collect::<Result<Vec<_>, _>>()?;
                self.layer_interaction.clipboard_mask =
                    Some((mask.clone(), doc.layer_offset(mask.id), strokes));
            }
            LayerAction::PasteMask { id } => {
                let mut layer = self.editable_layer(id)?;
                let (mut mask, origin, strokes) = self
                    .layer_interaction
                    .clipboard_mask
                    .clone()
                    .ok_or("Copy a mask first")?;
                let parent = layer.properties.parent.map_or(Point::default(), |id| {
                    self.engine.document().layer_offset(id)
                });
                mask.offset = Point {
                    x: origin.x - parent.x,
                    y: origin.y - parent.y,
                };
                mask.id = self.engine.allocate_layer_id();
                mask.show_area = false;
                mask.strokes = Default::default();
                let target = mask.id;
                layer.mask = Some(mask);
                let mut edits = vec![Edit::ReplaceLayer(Box::new(layer))];
                for mut stroke in strokes {
                    stroke.id = self.engine.allocate_stroke_id();
                    stroke.layer_id = target;
                    edits.push(Edit::InsertStroke(Box::new(stroke)));
                }
                edits.extend([
                    Edit::SetActiveLayer { id: LayerId(id) },
                    Edit::SetMaskTarget(true),
                ]);
                self.layer_edit(Edit::Batch(edits))?;
            }
            LayerAction::FillSelection => {
                let selection = self
                    .engine
                    .document()
                    .selection
                    .clone()
                    .ok_or("Make a selection first")?;
                self.fill_selection(selection)?;
            }
            LayerAction::Drop {
                id,
                target,
                fraction,
            } => {
                if id == target {
                    return Ok(());
                }
                let doc = self.engine.document();
                let row = doc.layer(LayerId(target)).ok_or("Unknown destination")?;
                let into = row.kind == LayerKind::Group && (0.25..0.75).contains(&fraction);
                let parent = if into {
                    Some(row.id.0)
                } else {
                    row.properties.parent.map(|id| id.0)
                };
                let index = doc.layers.iter().position(|l| l.id == row.id).unwrap()
                    + usize::from(into || fraction >= 0.5);
                let from = doc
                    .layers
                    .iter()
                    .position(|l| l.id == LayerId(id))
                    .ok_or("Unknown layer")?;
                self.layer_action(LayerAction::Reparent {
                    id,
                    parent,
                    index: index.saturating_sub(usize::from(from < index)) as u32,
                })?;
                if into {
                    self.layer_interaction.collapsed.remove(&LayerId(target));
                }
            }
            LayerAction::New { group, clipped } => {
                let doc = self.engine.document();
                let active = doc.layer(doc.active_layer).ok_or("Unknown layer")?;
                if clipped && !matches!(active.kind, LayerKind::Paint | LayerKind::ImportedImage) {
                    return Err("Choose a paint layer to clip to".into());
                }
                let parent = if active.kind == LayerKind::Group {
                    Some(active.id)
                } else {
                    active.properties.parent
                };
                if parent.is_some_and(|p| doc.is_locked(p)) {
                    return Err("The destination group is locked".into());
                }
                let index = doc.layers.iter().position(|l| l.id == active.id).unwrap()
                    + usize::from(active.kind == LayerKind::Group);
                let id = self.engine.allocate_layer_id();
                let mut layer = Layer::paint(id, if group { "Group" } else { "Layer" });
                layer.name = format!("{} {}", if group { "Group" } else { "Layer" }, id.0).into();
                if group {
                    layer.kind = LayerKind::Group;
                }
                layer.properties.parent = parent;
                layer.properties.clipped = clipped;
                self.layer_edit(Edit::Batch(vec![
                    Edit::InsertLayer { index, layer },
                    Edit::SetActiveLayer { id },
                ]))?;
            }
            LayerAction::Select { id, mask } => {
                self.state.layer_tools.rename_layer = None;
                self.engine.set_active_layer(LayerId(id)).map_err(error)?;
                self.layer_edit(Edit::SetMaskTarget(mask))?;
                self.layer_interaction.selected = BTreeSet::from([LayerId(id)]);
            }
            LayerAction::ToggleSelection { id } => {
                let id = LayerId(id);
                self.engine.document().layer(id).ok_or("Unknown layer")?;
                if !self.layer_interaction.selected.remove(&id) {
                    self.layer_interaction.selected.insert(id);
                }
            }
            LayerAction::ReferenceSelection => {
                let targets = self.reference_selection();
                let mut references = self.engine.document().reference_layers.clone();
                if self.reference_action_removes() {
                    references.retain(|id| !targets.contains(id));
                } else {
                    references.extend(targets);
                    self.layer_interaction.selected =
                        BTreeSet::from([self.engine.document().active_layer]);
                }
                if references != self.engine.document().reference_layers {
                    self.layer_edit(Edit::SetReferences(references))?;
                }
            }
            LayerAction::Tool { tool } => {
                if tool == LayerCanvasTool::Transform {
                    return self.begin_transform();
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
                if let LayerCanvasTool::Region { fill, source } = tool {
                    self.region_tools.source[usize::from(fill)] = source;
                }
                if let LayerCanvasTool::Gradient {
                    radial,
                    transparent,
                } = tool
                {
                    self.layer_interaction.gradient = [radial, transparent];
                }
                if tool.picks_color() {
                    self.eyedropper.layer = tool == LayerCanvasTool::PickLayer;
                }
                self.layer_interaction.tool = tool;
                self.state.layer_tools.tool = tool;
                self.refresh_tools();
            }
            LayerAction::Deselect => self.layer_edit(Edit::SetSelection(None))?,
            LayerAction::InvertSelection => {
                let mut selection = self
                    .engine
                    .document()
                    .selection
                    .clone()
                    .ok_or("Make a selection first")?;
                selection.inverted = !selection.inverted;
                self.layer_edit(Edit::SetSelection(Some(selection)))?;
            }
            LayerAction::Collapse { id } => {
                let id = LayerId(id);
                if !self.layer_interaction.collapsed.remove(&id) {
                    self.layer_interaction.collapsed.insert(id);
                }
            }
            LayerAction::Lock { id, value } => {
                let mut layer = self
                    .engine
                    .document()
                    .layer(LayerId(id))
                    .ok_or("Unknown layer")?
                    .clone();
                if layer.kind == LayerKind::Background
                    || layer
                        .properties
                        .parent
                        .is_some_and(|p| self.engine.document().is_locked(p))
                {
                    return Err("This layer is protected".into());
                }
                layer.properties.locked = value;
                self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))?;
            }
            LayerAction::Reference { id } => {
                let mut references = self.engine.document().reference_layers.clone();
                if !references.remove(&LayerId(id)) {
                    references.insert(LayerId(id));
                }
                self.layer_edit(Edit::SetReferences(references))?;
            }
            a @ (LayerAction::Solo { .. } | LayerAction::SoloSelected) => {
                let next: Vec<_> = if let Some(previous) = self.layer_interaction.solo.take() {
                    previous
                        .into_iter()
                        .filter(|(id, _)| self.engine.document().layer(*id).is_some())
                        .collect()
                } else {
                    let doc = self.engine.document();
                    let roots = if let LayerAction::Solo { id } = a {
                        vec![LayerId(id)]
                    } else {
                        doc.layer_roots(&self.layer_interaction.selected)
                    };
                    if roots.is_empty() {
                        return Err("Select layers first".into());
                    }
                    self.layer_interaction.solo =
                        Some(doc.layers.iter().map(|l| (l.id, l.visible)).collect());
                    let mut keep = doc.layer_subtrees(&roots);
                    // A clipped layer still needs its base during isolation.
                    for id in keep.clone() {
                        if doc.layer(id).is_some_and(|l| l.properties.clipped)
                            && let Some(base) = doc.clipping_base(id)
                        {
                            keep.insert(base);
                        }
                    }
                    for id in roots {
                        let mut parent = doc.layer(id).and_then(|l| l.properties.parent);
                        while let Some(p) = parent {
                            keep.insert(p);
                            parent = doc.layer(p).and_then(|l| l.properties.parent);
                        }
                    }
                    doc.layers
                        .iter()
                        .map(|l| {
                            (
                                l.id,
                                keep.contains(&l.id) || l.kind == LayerKind::Background,
                            )
                        })
                        .collect()
                };
                self.layer_edit(Edit::Batch(
                    next.into_iter()
                        .map(|(id, visible)| Edit::SetLayerVisibility { id, visible })
                        .collect(),
                ))?;
            }
            a @ (LayerAction::Duplicate { .. } | LayerAction::DuplicateSelected) => {
                let doc = self.engine.document();
                let roots = if let LayerAction::Duplicate { id } = a {
                    vec![LayerId(id)]
                } else {
                    doc.layer_roots(&self.layer_interaction.selected)
                };
                if roots.is_empty() {
                    return Err("Select layers first".into());
                }
                for &id in &roots {
                    if doc.layer(id).ok_or("Unknown layer")?.kind == LayerKind::Background {
                        return Err("The background cannot be duplicated".into());
                    }
                }
                let index = doc
                    .layers
                    .iter()
                    .position(|l| roots.contains(&l.id))
                    .unwrap();
                let members = doc.layer_subtrees(&roots);
                let sources: Vec<_> = self
                    .engine
                    .document()
                    .ordered_layers()
                    .into_iter()
                    .filter(|l| members.contains(&l.id))
                    .cloned()
                    .collect();
                let ids: std::collections::BTreeMap<_, _> = sources
                    .iter()
                    .map(|l| (l.id, self.engine.allocate_layer_id()))
                    .collect();
                let root_id = ids[&roots[0]];
                let mut edits = Vec::new();
                for (offset, source) in sources.iter().enumerate() {
                    let new_id = ids[&source.id];
                    let mut copy = source.clone();
                    copy.id = new_id;
                    if roots.contains(&source.id) {
                        copy.name = format!("{} copy", source.name).into();
                    }
                    copy.properties.parent = copy
                        .properties
                        .parent
                        .map(|p| ids.get(&p).copied().unwrap_or(p));
                    copy.strokes.clear();
                    if let Some(mask) = &mut copy.mask {
                        mask.id = self.engine.allocate_layer_id();
                        mask.strokes = Default::default();
                    }
                    edits.push(Edit::InsertLayer {
                        index: index + offset,
                        layer: copy.clone(),
                    });
                    for (target, ids) in std::iter::once((new_id, &source.strokes)).chain(
                        source
                            .mask
                            .iter()
                            .map(|m| (copy.mask.as_ref().unwrap().id, m.strokes.as_ref())),
                    ) {
                        for id in ids {
                            let mut stroke = self
                                .engine
                                .document()
                                .stroke(*id)
                                .ok_or("Missing stroke")?
                                .clone();
                            stroke.id = self.engine.allocate_stroke_id();
                            stroke.layer_id = target;
                            edits.push(Edit::InsertStroke(Box::new(stroke)));
                        }
                    }
                }
                edits.push(Edit::SetActiveLayer { id: root_id });
                self.layer_edit(Edit::Batch(edits))?;
                self.layer_interaction.editing = Some(root_id);
                self.layer_interaction.selected = roots.iter().map(|id| ids[id]).collect();
            }
            LayerAction::Delete { id } => {
                let edit = self
                    .engine
                    .document()
                    .delete_layers_edit(&[LayerId(id)])
                    .map_err(error)?;
                self.layer_edit(edit)?;
            }
            LayerAction::Reparent { id, parent, index } => {
                let mut layer = self.editable_layer(id)?;
                self.check_dependents(layer.id)?;
                let old_parent = layer
                    .properties
                    .parent
                    .map_or(Point::default(), |p| self.engine.document().layer_offset(p));
                layer.properties.parent = parent.map(LayerId);
                let doc = self.engine.document();
                if let Some(p) = layer.properties.parent
                    && doc.is_locked(p)
                {
                    return Err("The destination group is locked".into());
                }
                doc.validate_layer(&layer).map_err(error)?;
                let new_parent = layer
                    .properties
                    .parent
                    .map_or(Point::default(), |p| doc.layer_offset(p));
                let delta = Point {
                    x: old_parent.x - new_parent.x,
                    y: old_parent.y - new_parent.y,
                };
                layer.properties.offset.x += delta.x;
                layer.properties.offset.y += delta.y;
                if let Some(m) = &mut layer.mask {
                    m.offset.x += delta.x;
                    m.offset.y += delta.y;
                }
                let edit = Edit::Batch(vec![
                    Edit::ReplaceLayer(Box::new(layer)),
                    Edit::MoveLayer {
                        id: LayerId(id),
                        to: index as usize,
                    },
                ]);
                let mut probe = self.engine.document().clone();
                probe.apply(edit.clone()).map_err(error)?;
                for (i, l) in probe
                    .layers
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| l.properties.clipped)
                {
                    if probe.layers[i + 1..]
                        .iter()
                        .find(|next| {
                            next.properties.parent == l.properties.parent
                                && !next.properties.clipped
                        })
                        .is_none_or(|base| {
                            !matches!(base.kind, LayerKind::Paint | LayerKind::ImportedImage)
                        })
                    {
                        return Err(
                            "Keep clipped layers above a paint layer in the same group".into()
                        );
                    }
                }
                self.layer_edit(edit)?;
            }
            other => {
                let id = match &other {
                    LayerAction::Rename { id, .. }
                    | LayerAction::Clear { id }
                    | LayerAction::AlphaLock { id, .. }
                    | LayerAction::Clip { id, .. }
                    | LayerAction::Blend { id, .. }
                    | LayerAction::AddMask { id, .. }
                    | LayerAction::DeleteMask { id }
                    | LayerAction::ApplyMask { id }
                    | LayerAction::EnableMask { id, .. }
                    | LayerAction::LinkMask { id, .. }
                    | LayerAction::ShowMask { id, .. }
                    | LayerAction::InvertMask { id }
                    | LayerAction::ClearMask { id, .. } => *id,
                    _ => unreachable!(),
                };
                let mut layer = if matches!(other, LayerAction::ShowMask { .. }) {
                    self.engine
                        .document()
                        .layer(LayerId(id))
                        .ok_or("Unknown layer")?
                        .clone()
                } else {
                    self.editable_layer(id)?
                };
                match other {
                    LayerAction::Rename { name, .. } => {
                        let name = name.trim();
                        if name.is_empty() || name.chars().count() > 128 {
                            return Err("Use a name of 1–128 characters".into());
                        }
                        layer.name = name.into();
                        self.state.layer_tools.rename_layer = None;
                    }
                    LayerAction::Clear { .. } => {
                        if layer.kind != LayerKind::Paint {
                            return Err("Choose a paint layer".into());
                        }
                        layer.strokes.clear();
                        layer.operations.clear();
                        layer.asset = None;
                    }
                    LayerAction::AlphaLock { value, .. } => {
                        if layer.kind != LayerKind::Paint {
                            return Err("Alpha lock needs a paint layer".into());
                        }
                        layer.properties.alpha_locked = value;
                    }
                    LayerAction::Clip { value, .. } => {
                        if value {
                            let siblings: Vec<_> = self
                                .engine
                                .document()
                                .layers
                                .iter()
                                .filter(|l| l.properties.parent == layer.properties.parent)
                                .collect();
                            let i = siblings.iter().position(|l| l.id == layer.id).unwrap();
                            let base = siblings[i + 1..].iter().find(|l| !l.properties.clipped);
                            if base.is_none_or(|l| {
                                !matches!(l.kind, LayerKind::Paint | LayerKind::ImportedImage)
                            }) {
                                return Err("There is no paint layer below to clip to".into());
                            }
                        }
                        layer.properties.clipped = value;
                    }
                    LayerAction::Blend { value, .. } => {
                        layer.properties.blend = *layer_core::LayerBlend::ALL
                            .get(value as usize)
                            .ok_or("Unknown blend mode")?
                    }
                    LayerAction::AddMask { replace, .. } => {
                        self.layer_interaction.tool = LayerCanvasTool::Paint;
                        self.state.layer_tools.tool = LayerCanvasTool::Paint;
                        self.refresh_tools();
                        if layer.mask.is_none() || replace {
                            let linked = layer.mask.as_ref().is_none_or(|m| m.linked);
                            let offset = layer
                                .mask
                                .as_ref()
                                .map_or(layer.properties.offset, |m| m.offset);
                            let mut mask =
                                LayerMask::reveal_all(self.engine.allocate_layer_id(), offset);
                            mask.linked = linked;
                            if let Some(selection) = &self.engine.document().selection {
                                let mut selection = selection.clone();
                                selection.inverted ^= hide_selection;
                                let parent_offset = self.engine.document().layer_offset(layer.id);
                                mask.initial = Some(selection.translated(Point {
                                    x: -(parent_offset.x - layer.properties.offset.x + offset.x),
                                    y: -(parent_offset.y - layer.properties.offset.y + offset.y),
                                }));
                                mask.default_coverage = f32::from(selection.inverted);
                            }
                            layer.mask = Some(mask);
                            self.layer_edit(Edit::Batch(vec![
                                Edit::ReplaceLayer(Box::new(layer)),
                                Edit::SetSelection(None),
                                Edit::SetActiveLayer { id: LayerId(id) },
                                Edit::SetMaskTarget(true),
                            ]))?;
                            return Ok(());
                        }
                        self.engine.set_active_layer(LayerId(id)).map_err(error)?;
                        self.layer_edit(Edit::SetMaskTarget(true))?;
                        return Ok(());
                    }
                    LayerAction::DeleteMask { .. } => {
                        layer.mask = None;
                    }
                    LayerAction::ApplyMask { .. } => {
                        if layer.kind != LayerKind::Paint {
                            return Err("Apply a group mask by flattening the group first".into());
                        }
                        let mut mask = layer.mask.take().ok_or("No mask")?;
                        if !mask.enabled {
                            return Err("Enable the mask before applying it".into());
                        }
                        mask.show_area = false;
                        mask.offset.x -= layer.properties.offset.x;
                        mask.offset.y -= layer.properties.offset.y;
                        layer.operations.push(layer_core::LayerOperation {
                            after_stroke: layer.strokes.len(),
                            coverage: mask,
                            kind: layer_core::LayerOperationKind::ApplyMask,
                        });
                    }
                    LayerAction::EnableMask { value, .. } => {
                        layer.mask.as_mut().ok_or("No mask")?.enabled = value
                    }
                    LayerAction::LinkMask { value, .. } => {
                        layer.mask.as_mut().ok_or("No mask")?.linked = value
                    }
                    LayerAction::ShowMask { value, .. } => {
                        layer.mask.as_mut().ok_or("No mask")?.show_area = value;
                        if value {
                            self.engine.set_active_layer(LayerId(id)).map_err(error)?;
                            self.layer_edit(Edit::SetMaskTarget(true))?;
                        }
                    }
                    LayerAction::InvertMask { .. } => {
                        let m = layer.mask.as_mut().ok_or("No mask")?;
                        m.inverted = !m.inverted;
                    }
                    LayerAction::ClearMask { reveal, .. } => {
                        let m = layer.mask.as_mut().ok_or("No mask")?;
                        m.id = self.engine.allocate_layer_id();
                        m.strokes = Default::default();
                        m.initial = None;
                        m.inverted = false;
                        m.default_coverage = f32::from(reveal);
                    }
                    _ => unreachable!(),
                }
                self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))?;
            }
        }
        Ok(())
    }
    fn check_dependents(&self, id: LayerId) -> Result<(), String> {
        let doc = self.engine.document();
        let l = doc.layer(id).ok_or("Unknown layer")?;
        let siblings: Vec<_> = doc
            .layers
            .iter()
            .filter(|n| n.properties.parent == l.properties.parent)
            .collect();
        let i = siblings.iter().position(|n| n.id == id).unwrap();
        if i > 0 && siblings[i - 1].properties.clipped && !l.properties.clipped {
            return Err("Release the clipped layers above this base first".into());
        }
        Ok(())
    }
    pub fn layer_menu(&self, id: u64, mask: bool) -> Result<ContextMenu, String> {
        use LayerAction as A;
        let doc = self.engine.document();
        let l = doc.layer(LayerId(id)).ok_or("Unknown layer")?;
        let locked = doc.is_locked(l.id);
        let paint = l.kind == LayerKind::Paint;
        let editable = l.kind != LayerKind::Background;
        let controls = LayerControls::for_layer(doc, l);
        let roots = doc.layer_roots(&self.layer_interaction.selected);
        let multiple = roots.len() > 1;
        let parent = if l.kind == LayerKind::Group {
            Some(l.id)
        } else {
            l.properties.parent
        };
        let item = |label: &str, action: A| {
            let enabled = match &action {
                A::GroupSelected => doc.group_layers_edit(&roots, LayerId(0)).is_ok(),
                A::Ungroup { .. } => doc.ungroup_layer_edit(l.id).is_ok(),
                A::DeleteSelected => doc.can_delete_layers(&roots),
                A::Delete { .. } => doc.can_delete_layers(&[l.id]),
                A::DuplicateSelected => {
                    !roots.is_empty()
                        && roots.iter().all(|id| {
                            doc.layer(*id)
                                .is_some_and(|l| l.kind != LayerKind::Background)
                        })
                }
                A::Duplicate { .. } | A::Select { .. } | A::ShowMask { .. } => editable,
                A::CopyMask { .. } => l.mask.is_some(),
                A::PasteMask { .. } => {
                    editable && !locked && self.layer_interaction.clipboard_mask.is_some()
                }
                A::New { clipped, .. } => {
                    !parent.is_some_and(|p| doc.is_locked(p))
                        && (!clipped
                            || matches!(l.kind, LayerKind::Paint | LayerKind::ImportedImage))
                }
                A::Reference { .. } => matches!(
                    l.kind,
                    LayerKind::Paint | LayerKind::ImportedImage | LayerKind::Group
                ),
                A::ReferenceSelection => !self.reference_selection().is_empty(),
                A::Lock { .. } => controls.edit_lock,
                A::AlphaLock { .. } | A::Clear { .. } => controls.alpha_lock,
                A::Clip { .. } => controls.clip,
                A::MaskSelection { .. } => editable && !locked && doc.selection.is_some(),
                A::ApplyMask { .. } => {
                    paint && !locked && l.mask.as_ref().is_some_and(|m| m.enabled)
                }
                A::InvertSelection | A::Deselect => doc.selection.is_some(),
                A::FillSelection => controls.fill && doc.selection.is_some(),
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
                A::SoloSelected => !roots.is_empty() || self.layer_interaction.solo.is_some(),
                _ => editable && !locked,
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
        let mask_selection = || {
            vec![
                item(
                    if l.mask.is_some() {
                        "Replace mask: reveal selection"
                    } else {
                        "Mask: reveal selection"
                    },
                    A::MaskSelection { id, hide: false },
                ),
                item(
                    if l.mask.is_some() {
                        "Replace mask: hide selection"
                    } else {
                        "Mask: hide selection"
                    },
                    A::MaskSelection { id, hide: true },
                ),
            ]
        };
        let sections = if !editable {
            vec![
                vec![
                    item(
                        "New layer",
                        A::New {
                            group: false,
                            clipped: false,
                        },
                    ),
                    item(
                        "New group",
                        A::New {
                            group: true,
                            clipped: false,
                        },
                    ),
                ],
                vec![
                    check(
                        "Show paper",
                        A::Visibility {
                            id,
                            value: !l.visible,
                        },
                        l.visible,
                    ),
                    item("Show all layers", A::ShowAll),
                    item(
                        "Lasso selection",
                        A::Tool {
                            tool: LayerCanvasTool::Select,
                        },
                    ),
                ],
            ]
        } else if mask {
            let m = l.mask.as_ref().ok_or("No mask")?;
            vec![
                vec![
                    item("Edit layer content", A::Select { id, mask: false }),
                    check(
                        "Show mask area",
                        A::ShowMask {
                            id,
                            value: !m.show_area,
                        },
                        m.show_area,
                    ),
                    check(
                        "Enable mask",
                        A::EnableMask {
                            id,
                            value: !m.enabled,
                        },
                        m.enabled,
                    ),
                    check(
                        "Link mask to layer",
                        A::LinkMask {
                            id,
                            value: !m.linked,
                        },
                        m.linked,
                    ),
                ],
                mask_selection(),
                vec![
                    item("Copy mask", A::CopyMask { id }),
                    item("Replace with copied mask", A::PasteMask { id }),
                    item("Invert mask", A::InvertMask { id }),
                    item("Reveal all", A::ClearMask { id, reveal: true }),
                    item("Hide all", A::ClearMask { id, reveal: false }),
                ],
                vec![
                    item("Apply mask to layer", A::ApplyMask { id }),
                    item("Delete mask", A::DeleteMask { id }),
                ],
            ]
        } else {
            let mut organization = vec![
                item(
                    if l.kind == LayerKind::Group {
                        "Rename group…"
                    } else {
                        "Rename layer…"
                    },
                    A::BeginRename { id },
                ),
                item(
                    if multiple {
                        "Duplicate selected layers"
                    } else {
                        "Duplicate"
                    },
                    if multiple {
                        A::DuplicateSelected
                    } else {
                        A::Duplicate { id }
                    },
                ),
                item("Group selected layers", A::GroupSelected),
            ];
            if l.kind == LayerKind::Group {
                organization.push(item("Ungroup", A::Ungroup { id }));
            }
            let mask_menu = if l.mask.is_some() {
                let mut sections = self.layer_menu(id, true)?.sections;
                sections[0][0] = item("Edit mask", A::Select { id, mask: true });
                sections
            } else {
                vec![
                    vec![item("Add mask", A::AddMask { id, replace: false })],
                    mask_selection(),
                    vec![item("Paste mask", A::PasteMask { id })],
                ]
            };
            let selection = vec![
                vec![
                    item("Select all layers", A::SelectAllLayers { selected: true }),
                    item(
                        "Clear layer selection",
                        A::SelectAllLayers { selected: false },
                    ),
                ],
                vec![
                    item(
                        "Lasso selection",
                        A::Tool {
                            tool: LayerCanvasTool::Select,
                        },
                    ),
                    item(
                        "Lasso Fill",
                        A::Tool {
                            tool: LayerCanvasTool::LassoFill,
                        },
                    ),
                    item("Fill selection", A::FillSelection),
                    item("Invert selection", A::InvertSelection),
                    item("Deselect pixels", A::Deselect),
                ],
            ];
            let visibility = vec![vec![
                check(
                    "Show layer",
                    A::Visibility {
                        id,
                        value: !l.visible,
                    },
                    l.visible,
                ),
                item("Show layer and parent groups", A::ShowParents { id }),
                check(
                    "Isolate selected layers",
                    A::SoloSelected,
                    self.layer_interaction.solo.is_some(),
                ),
                item("Show all layers", A::ShowAll),
            ]];
            let mut protection = Vec::new();
            if paint {
                protection.push(check(
                    "Alpha lock",
                    A::AlphaLock {
                        id,
                        value: !l.properties.alpha_locked,
                    },
                    l.properties.alpha_locked,
                ));
            }
            protection.extend([
                check(
                    "Lock editing",
                    A::Lock {
                        id,
                        value: !l.properties.locked,
                    },
                    l.properties.locked,
                ),
                check(
                    "Clip to layer below",
                    A::Clip {
                        id,
                        value: !l.properties.clipped,
                    },
                    l.properties.clipped,
                ),
                if multiple {
                    item("Use selected layers as references", A::ReferenceSelection)
                } else {
                    check(
                        "Use as reference",
                        A::Reference { id },
                        doc.reference_layers.contains(&l.id),
                    )
                },
            ]);
            let mut destructive = Vec::new();
            if paint {
                destructive.push(item("Clear layer", A::Clear { id }));
            }
            destructive.push(item(
                if multiple {
                    "Delete selected layers"
                } else if l.kind == LayerKind::Group {
                    "Delete group and contents"
                } else {
                    "Delete layer"
                },
                if multiple {
                    A::DeleteSelected
                } else {
                    A::Delete { id }
                },
            ));
            vec![
                vec![
                    item(
                        "New layer",
                        A::New {
                            group: false,
                            clipped: false,
                        },
                    ),
                    item(
                        "New clipping layer",
                        A::New {
                            group: false,
                            clipped: true,
                        },
                    ),
                    item(
                        "New group",
                        A::New {
                            group: true,
                            clipped: false,
                        },
                    ),
                ],
                organization,
                protection,
                vec![
                    ContextMenuItem::submenu("Mask", mask_menu),
                    ContextMenuItem::submenu("Selection", selection),
                    ContextMenuItem::submenu("Visibility", visibility),
                    item(
                        "Move layer / mask",
                        A::Tool {
                            tool: LayerCanvasTool::Move,
                        },
                    ),
                ],
                destructive,
            ]
        };
        Ok(ContextMenu {
            title: format!(
                "{} {}",
                l.name,
                if mask {
                    "mask"
                } else if l.kind == LayerKind::Group {
                    "group"
                } else {
                    "layer"
                }
            ),
            sections,
        }
        .with_shortcuts(&self.state.settings, self.state.platform))
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
        if matches!(self.layer_interaction.tool, LayerCanvasTool::Ruler { .. }) {
            return self.ruler_pen(event, p);
        }
        if self.layer_interaction.tool == LayerCanvasTool::Transform {
            return self.transform_pen(event, p);
        }
        if self.layer_interaction.tool == LayerCanvasTool::Move
            && self.operation_ruler_pen(event, p)?
        {
            return Ok(());
        }
        match event.phase {
            PenPhase::Down => {
                self.layer_interaction.path.clear();
                let doc = self.engine.document();
                let layer = doc.layer(doc.active_layer).ok_or("Unknown layer")?;
                let controls = LayerControls::for_layer(doc, layer);
                match self.layer_interaction.tool {
                    LayerCanvasTool::Move if !controls.move_layer => return Ok(()),
                    LayerCanvasTool::LassoFill if !controls.fill => return Ok(()),
                    LayerCanvasTool::Gradient { .. } | LayerCanvasTool::Figure { .. }
                        if !controls.fill || doc.active_mask =>
                    {
                        return Ok(());
                    }
                    _ => (),
                }
                self.layer_interaction.path.push(p);
                self.layer_interaction.original =
                    (self.layer_interaction.tool == LayerCanvasTool::Move).then(|| layer.clone());
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
                    let original = self.layer_interaction.original.as_ref().unwrap().clone();
                    self.engine
                        .preview_edit(Edit::ReplaceLayer(Box::new(original.clone())))
                        .map_err(error)?;
                    let edit = self
                        .engine
                        .document()
                        .move_target_edit(Point {
                            x: p.x - start.x,
                            y: p.y - start.y,
                        })
                        .map_err(error)?;
                    if event.phase == PenPhase::Up {
                        self.layer_edit(edit)?;
                    } else {
                        self.engine.preview_edit(edit).map_err(error)?;
                    }
                }
                if event.phase == PenPhase::Up {
                    if matches!(self.layer_interaction.tool, LayerCanvasTool::Figure { .. }) {
                        self.commit_figure()?;
                    }
                    if let LayerCanvasTool::Gradient {
                        radial,
                        transparent,
                    } = self.layer_interaction.tool
                    {
                        let start = self.layer_interaction.path[0];
                        if (p.x - start.x).hypot(p.y - start.y) >= 0.5 / self.state.camera.zoom {
                            self.gradient_fill(start, p, radial, transparent)?;
                        }
                    }
                    if self.layer_interaction.tool == LayerCanvasTool::Select {
                        let selection = Selection::polygon(self.layer_interaction.path.clone())
                            .map_err(error)?;
                        self.layer_edit(Edit::SetSelection(Some(selection)))?;
                    }
                    if self.layer_interaction.tool == LayerCanvasTool::LassoFill {
                        let selection = Selection::polygon(self.layer_interaction.path.clone())
                            .map_err(error)?;
                        self.fill_selection(selection)?;
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
        self.layer_interaction.changed = true;
        Ok(())
    }

    pub(super) fn cancel_layer_gesture(&mut self) -> Result<bool, String> {
        let transform = self.cancel_transform()?;
        self.cancel_ruler_gesture();
        let region = self.region_tools.cancellable();
        self.region_tools.cancel();
        if self.layer_interaction.path.is_empty() {
            return Ok(region || transform);
        }
        if let Some(original) = self.layer_interaction.original.take() {
            self.engine
                .preview_edit(Edit::ReplaceLayer(Box::new(original)))
                .map_err(error)?;
        }
        self.layer_interaction.path.clear();
        self.layer_interaction.changed = true;
        Ok(true)
    }

    pub(super) fn fill_selection(&mut self, selection: Selection) -> Result<(), String> {
        self.paint_operation(Some(selection), self.fill_operation())
    }

    pub(super) fn fill_operation(&self) -> layer_core::LayerOperationKind {
        let brush = self.engine.brush();
        let mut color = brush.color_rgba_linear;
        color[3] *= brush.opacity;
        layer_core::LayerOperationKind::Fill {
            color,
            alpha_locked: self
                .engine
                .document()
                .layer(self.engine.document().active_layer)
                .is_some_and(|l| l.properties.alpha_locked),
        }
    }

    fn gradient_fill(
        &mut self,
        start: Point,
        end: Point,
        radial: bool,
        transparent: bool,
    ) -> Result<(), String> {
        let doc = self.engine.document();
        let offset = doc.layer_offset(doc.active_layer);
        let local = |p: Point| Point {
            x: p.x - offset.x,
            y: p.y - offset.y,
        };
        let color = self.state.colors.foreground;
        let mut colors = [
            color,
            if transparent {
                color
            } else {
                self.state.colors.background
            },
        ];
        for c in &mut colors {
            for value in &mut c[..3] {
                *value = srgb_to_linear(*value);
            }
            c[3] *= self.state.brush.opacity;
        }
        if transparent {
            colors[1][3] = 0.0;
        }
        let kind = layer_core::LayerOperationKind::Gradient {
            start: local(start),
            end: local(end),
            colors,
            radial,
            alpha_locked: doc
                .layer(doc.active_layer)
                .is_some_and(|l| l.properties.alpha_locked),
        };
        self.paint_operation(doc.selection.clone(), kind)
    }

    pub(super) fn paint_operation(
        &mut self,
        selection: Option<Selection>,
        kind: layer_core::LayerOperationKind,
    ) -> Result<(), String> {
        let id = self.engine.document().active_layer;
        let layer = self.editable_layer(id.0)?;
        if layer.kind != LayerKind::Paint || self.engine.document().active_mask {
            return Err("Select a paint layer's content to fill".into());
        }
        let offset = self.engine.document().layer_offset(id);
        let mut coverage = LayerMask::reveal_all(self.engine.allocate_layer_id(), Point::default());
        if let Some(selection) = selection {
            coverage.default_coverage = f32::from(selection.inverted);
            coverage.initial = Some(selection.translated(Point {
                x: -offset.x,
                y: -offset.y,
            }));
        }
        self.engine
            .append_layer_operation(
                id,
                layer_core::LayerOperation {
                    after_stroke: layer.strokes.len(),
                    coverage,
                    kind,
                },
            )
            .map_err(error)?;
        self.layer_interaction.changed = true;
        Ok(())
    }

    pub fn append_layer_overlay(&self, segments: &mut Vec<layer_render::CursorSegment>) {
        self.append_ruler_overlay(segments);
        self.append_transform_overlay(segments);
        let matrix = self.state.camera.view().document_to_surface;
        let scale = self
            .logical_viewport
            .map_or(1., |v| self.state.camera.viewport[0] as f32 / v[0]);
        let transform = |p: Point| {
            [
                (matrix[0] * p.x + matrix[2] * p.y + matrix[4]) / scale,
                (matrix[1] * p.x + matrix[3] * p.y + matrix[5]) / scale,
            ]
        };
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
        if let Some(selection) = self.engine.display_selection() {
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
        if let Some(figure) = self.current_figure() {
            let guide = figure
                .shape
                .guide(figure.start, figure.end, self.state.camera.zoom);
            let offset = self
                .engine
                .document()
                .layer_offset(self.engine.document().active_layer);
            path(
                &guide,
                figure.shape != FigureShape::Line,
                layer_core::Affine::translation(offset),
            );
        }
    }
}
