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
    Select,
    LassoFill,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LayersView {
    pub tool: LayerCanvasTool,
    pub has_selection: bool,
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
    pub path: Vec<Point>,
    original: Option<Layer>,
    solo: Option<Vec<(LayerId, bool)>>,
    pub changed: bool,
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
    fn layer_edit(&mut self, edit: Edit) -> Result<(), String> {
        self.engine.apply_edit(edit).map_err(error)
    }
    fn editable_layer(&self, id: u64) -> Result<Layer, String> {
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
        match action {
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
                return self.layer_action(LayerAction::Reparent {
                    id,
                    parent,
                    index: index.saturating_sub(usize::from(from < index)) as u32,
                });
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
                self.engine.set_active_layer(LayerId(id)).map_err(error)?;
                self.layer_edit(Edit::SetMaskTarget(mask))?;
            }
            LayerAction::Tool { tool } => {
                self.layer_interaction.tool = tool;
                self.state.layer_tools.tool = tool;
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
                layer.properties.locked = value;
                self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))?;
            }
            LayerAction::Reference { id } => self.layer_edit(Edit::SetReference(
                if self.engine.document().reference_layer == Some(LayerId(id)) {
                    None
                } else {
                    Some(LayerId(id))
                },
            ))?,
            LayerAction::Solo { id } => {
                let next = if let Some(previous) = self.layer_interaction.solo.take() {
                    previous
                } else {
                    let doc = self.engine.document();
                    self.layer_interaction.solo =
                        Some(doc.layers.iter().map(|l| (l.id, l.visible)).collect());
                    let mut keep = BTreeSet::from([LayerId(id)]);
                    for l in doc.ordered_layers() {
                        if l.properties.parent.is_some_and(|p| keep.contains(&p)) {
                            keep.insert(l.id);
                        }
                    }
                    let mut parent = doc.layer(LayerId(id)).and_then(|l| l.properties.parent);
                    while let Some(p) = parent {
                        keep.insert(p);
                        parent = doc.layer(p).and_then(|l| l.properties.parent);
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
            LayerAction::Duplicate { id } => {
                let source = self.editable_layer(id)?;
                let index = self
                    .engine
                    .document()
                    .layers
                    .iter()
                    .position(|l| l.id == source.id)
                    .unwrap();
                let mut members = BTreeSet::from([source.id]);
                let sources: Vec<_> = self
                    .engine
                    .document()
                    .ordered_layers()
                    .into_iter()
                    .filter_map(|l| {
                        if l.id == source.id
                            || l.properties.parent.is_some_and(|p| members.contains(&p))
                        {
                            members.insert(l.id);
                            Some(l.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                let ids: std::collections::BTreeMap<_, _> = sources
                    .iter()
                    .map(|l| (l.id, self.engine.allocate_layer_id()))
                    .collect();
                let root_id = ids[&source.id];
                let mut edits = Vec::new();
                for (offset, source) in sources.iter().enumerate() {
                    let new_id = ids[&source.id];
                    let mut copy = source.clone();
                    copy.id = new_id;
                    if source.id.0 == id {
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
            }
            LayerAction::Delete { id } => {
                let layer = self.editable_layer(id)?;
                self.check_dependents(layer.id)?;
                let mut ids = BTreeSet::from([layer.id]);
                for _ in 0..self.engine.document().layers.len() {
                    for l in &self.engine.document().layers {
                        if l.properties.parent.is_some_and(|p| ids.contains(&p)) {
                            ids.insert(l.id);
                        }
                    }
                }
                if ids.iter().any(|id| self.engine.document().is_locked(*id)) {
                    return Err("A layer in this group is locked".into());
                }
                let edits = self
                    .engine
                    .document()
                    .ordered_layers()
                    .into_iter()
                    .rev()
                    .filter(|l| ids.contains(&l.id))
                    .map(|l| Edit::RemoveLayer { id: l.id })
                    .collect();
                self.layer_edit(Edit::Batch(edits))?;
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
        let l = self
            .engine
            .document()
            .layer(LayerId(id))
            .ok_or("Unknown layer")?;
        let item = |label: &str, action: LayerAction| {
            ContextMenuItem::command(label, UiAction::Layer { action })
        };
        let check = |label: &str, action: LayerAction, value: bool| {
            let mut i = item(label, action);
            i.selected = Some(value);
            i
        };
        let mut sections = if mask {
            let m = l.mask.as_ref().ok_or("No mask")?;
            vec![
                vec![
                    check(
                        "Show mask area",
                        LayerAction::ShowMask {
                            id,
                            value: !m.show_area,
                        },
                        m.show_area,
                    ),
                    check(
                        "Enable mask",
                        LayerAction::EnableMask {
                            id,
                            value: !m.enabled,
                        },
                        m.enabled,
                    ),
                    check(
                        "Link mask to layer",
                        LayerAction::LinkMask {
                            id,
                            value: !m.linked,
                        },
                        m.linked,
                    ),
                ],
                vec![
                    item(
                        "Replace mask from selection",
                        LayerAction::AddMask { id, replace: true },
                    ),
                    item("Invert mask", LayerAction::InvertMask { id }),
                    item("Reveal all", LayerAction::ClearMask { id, reveal: true }),
                    item("Hide all", LayerAction::ClearMask { id, reveal: false }),
                ],
                vec![
                    item("Apply mask to layer", LayerAction::ApplyMask { id }),
                    item("Delete mask", LayerAction::DeleteMask { id }),
                ],
            ]
        } else {
            vec![
                vec![
                    item(
                        "New layer",
                        LayerAction::New {
                            group: false,
                            clipped: false,
                        },
                    ),
                    item(
                        "New clipping layer",
                        LayerAction::New {
                            group: false,
                            clipped: true,
                        },
                    ),
                    item(
                        "New group",
                        LayerAction::New {
                            group: true,
                            clipped: false,
                        },
                    ),
                    item("Duplicate", LayerAction::Duplicate { id }),
                ],
                vec![
                    check(
                        "Alpha lock",
                        LayerAction::AlphaLock {
                            id,
                            value: !l.properties.alpha_locked,
                        },
                        l.properties.alpha_locked,
                    ),
                    check(
                        "Lock",
                        LayerAction::Lock {
                            id,
                            value: !l.properties.locked,
                        },
                        l.properties.locked,
                    ),
                    check(
                        "Clip to layer below",
                        LayerAction::Clip {
                            id,
                            value: !l.properties.clipped,
                        },
                        l.properties.clipped,
                    ),
                    check(
                        "Use as fill reference",
                        LayerAction::Reference { id },
                        self.engine.document().reference_layer == Some(l.id),
                    ),
                    item("Solo / restore", LayerAction::Solo { id }),
                ],
                vec![
                    item("Add mask", LayerAction::AddMask { id, replace: false }),
                    item(
                        "Move content / mask",
                        LayerAction::Tool {
                            tool: LayerCanvasTool::Move,
                        },
                    ),
                ],
                vec![
                    item(
                        "Lasso selection",
                        LayerAction::Tool {
                            tool: LayerCanvasTool::Select,
                        },
                    ),
                    item(
                        "Lasso Fill",
                        LayerAction::Tool {
                            tool: LayerCanvasTool::LassoFill,
                        },
                    ),
                    item("Fill selection", LayerAction::FillSelection),
                    item("Deselect", LayerAction::Deselect),
                ],
                vec![item("Delete layer", LayerAction::Delete { id })],
            ]
        };
        let locked = self.engine.document().is_locked(l.id);
        let paint = l.kind == LayerKind::Paint;
        for item in sections.iter_mut().flatten() {
            if let Some(UiAction::Layer { action }) = &item.action {
                item.enabled = match action {
                    LayerAction::ShowMask { .. }
                    | LayerAction::Solo { .. }
                    | LayerAction::Lock { .. } => l.kind != LayerKind::Background,
                    LayerAction::New { .. } => !locked,
                    LayerAction::Deselect
                    | LayerAction::FillSelection
                    | LayerAction::InvertSelection => {
                        self.engine.document().selection.is_some() && !locked
                    }
                    LayerAction::ApplyMask { .. } => {
                        paint && !locked && l.mask.as_ref().is_some_and(|m| m.enabled)
                    }
                    LayerAction::AlphaLock { .. } | LayerAction::Reference { .. } => {
                        paint && !locked
                    }
                    LayerAction::AddMask { replace: true, .. } => {
                        !locked && self.engine.document().selection.is_some()
                    }
                    LayerAction::Delete { .. } => {
                        !locked
                            && l.kind != LayerKind::Background
                            && self.check_dependents(l.id).is_ok()
                            && (!paint
                                || self
                                    .engine
                                    .document()
                                    .layers
                                    .iter()
                                    .filter(|l| l.kind == LayerKind::Paint)
                                    .count()
                                    > 1)
                    }
                    _ => !locked && l.kind != LayerKind::Background,
                };
            }
        }
        Ok(ContextMenu {
            title: format!("{} {}", l.name, if mask { "mask" } else { "layer" }),
            sections,
        })
    }
    pub(super) fn layer_pen(&mut self, event: PenEvent) -> Result<(), String> {
        let p = self
            .state
            .camera
            .input_transform()
            .map(event.surface_position);
        match event.phase {
            PenPhase::Down => {
                self.layer_interaction.path.clear();
                self.layer_interaction.path.push(p);
                self.layer_interaction.original =
                    Some(self.editable_layer(self.engine.document().active_layer.0)?);
            }
            PenPhase::Move | PenPhase::Up => {
                if self.layer_interaction.path.is_empty() {
                    return Ok(());
                }
                self.layer_interaction.path.push(p);
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
                if let Some(original) = self.layer_interaction.original.take() {
                    self.engine
                        .preview_edit(Edit::ReplaceLayer(Box::new(original)))
                        .map_err(error)?;
                }
                self.layer_interaction.path.clear();
            }
            PenPhase::Hover => {}
        }
        self.layer_interaction.changed = true;
        Ok(())
    }

    fn fill_selection(&mut self, selection: Selection) -> Result<(), String> {
        let id = self.engine.document().active_layer;
        let mut layer = self.editable_layer(id.0)?;
        if layer.kind != LayerKind::Paint || self.engine.document().active_mask {
            return Err("Select a paint layer's content to fill".into());
        }
        let offset = self.engine.document().layer_offset(id);
        let mut coverage = LayerMask::reveal_all(self.engine.allocate_layer_id(), Point::default());
        coverage.default_coverage = f32::from(selection.inverted);
        coverage.initial = Some(selection.translated(Point {
            x: -offset.x,
            y: -offset.y,
        }));
        let brush = self.engine.brush();
        let mut color = brush.color_rgba_linear;
        color[3] *= brush.opacity;
        layer.operations.push(layer_core::LayerOperation {
            after_stroke: layer.strokes.len(),
            coverage,
            kind: layer_core::LayerOperationKind::Fill {
                color,
                alpha_locked: layer.properties.alpha_locked,
            },
        });
        self.layer_edit(Edit::ReplaceLayer(Box::new(layer)))
    }

    pub fn append_layer_overlay(&self, segments: &mut Vec<layer_render::CursorSegment>) {
        let matrix = self.state.camera.view().document_to_surface;
        let transform = |p: Point| {
            [
                matrix[0] * p.x + matrix[2] * p.y + matrix[4],
                matrix[1] * p.x + matrix[3] * p.y + matrix[5],
            ]
        };
        let mut path = |points: &[Point], closed: bool| {
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
                let from = transform(*a);
                let to = transform(*b);
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
        if let Some(selection) = &self.engine.document().selection {
            for contour in selection.contours.iter() {
                path(contour, true);
            }
        }
        if matches!(
            self.layer_interaction.tool,
            LayerCanvasTool::Select | LayerCanvasTool::LassoFill
        ) {
            path(&self.layer_interaction.path, false);
        }
    }
}
