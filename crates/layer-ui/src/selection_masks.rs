//! Selection destinations and lifecycle. Temporary editing never creates a
//! document layer; stored masks use ordinary tree identity and shared history.
use super::*;
use layer_core::{Edit, Layer, Selection, SelectionTarget};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMenu {
    Selection,
    QuickMask,
    Overlay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionDisplayOptions {
    pub outline: bool,
    pub overlay: bool,
    pub protected: bool,
    pub color: [f32; 4],
}
impl Default for SelectionDisplayOptions {
    fn default() -> Self {
        Self {
            outline: true,
            overlay: true,
            protected: true,
            color: [1., 0., 0., 0.5],
        }
    }
}
impl SelectionDisplayOptions {
    pub fn validate(&self) -> Result<(), String> {
        for value in self.color {
            NumericControl::percent().validate(value, "Overlay color")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SelectionAction {
    NewLayer {
        parent: Option<u64>,
        save_current: bool,
    },
    EditLayer {
        id: u64,
    },
    LoadLayer {
        id: u64,
        mode: SelectionMode,
        inverted: bool,
    },
    LoadThumbnail {
        id: u64,
        mask: bool,
        shift: bool,
        alt: bool,
    },
    LoadCoverage {
        id: u64,
        mask: bool,
        mode: SelectionMode,
    },
    ReplaceLayer {
        id: u64,
    },
    InvertLayer {
        id: u64,
    },
    ClearLayer {
        id: u64,
        full: bool,
    },
    FillLayer {
        id: u64,
    },
    OverlayColor {
        color: [f32; 4],
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MaskEditingView {
    pub layer: Option<u64>,
    pub label: String,
    pub gray: f32,
    pub background: f32,
    pub overlay: bool,
    pub reason: Option<&'static str>,
    pub colors: ColorState,
}

struct Editing {
    target: SelectionTarget,
    artwork: LayerId,
    tool: LayerCanvasTool,
}
pub(super) struct SelectionMasks {
    editing: Option<Editing>,
    pub reselect: Option<Selection>,
    colors: ColorState,
    previews: std::collections::BTreeMap<LayerId, (Selection, u64)>,
    preview_revision: u64,
}
impl Default for SelectionMasks {
    fn default() -> Self {
        let mut colors = ColorState::default();
        colors.foreground = layer_core::color::RgbColor::BLACK;
        colors.background = layer_core::color::RgbColor::WHITE;
        colors.set_document_depth(layer_core::color::SampleDepth::U8)
            .expect("scalar mask color depth");
        Self {
            editing: None,
            reselect: None,
            colors,
            previews: Default::default(),
            preview_revision: 0,
        }
    }
}
impl SelectionMasks {
    pub fn preview_revision(&self, id: LayerId) -> u64 {
        self.previews.get(&id).map_or(0, |(_, revision)| *revision)
    }
    fn update_previews(&mut self, doc: &layer_core::Document) {
        self.previews.retain(|id, _| {
            doc.layer(*id)
                .is_some_and(|l| l.kind == LayerKind::Selection)
        });
        for layer in doc.layers.iter().filter(|l| l.kind == LayerKind::Selection) {
            let Ok(mask) = doc.saved_selection(layer.id) else {
                continue;
            };
            if self
                .previews
                .get(&layer.id)
                .is_none_or(|(old, _)| *old != mask)
            {
                self.preview_revision = self.preview_revision.wrapping_add(1);
                self.previews
                    .insert(layer.id, (mask, self.preview_revision));
            }
        }
    }
    pub fn target(&self) -> Option<SelectionTarget> {
        self.editing.as_ref().map(|e| e.target)
    }
    pub fn quick(&self) -> bool {
        self.target() == Some(SelectionTarget::Current)
    }
    pub fn gray(&self) -> f32 {
        self.colors.definition().rgba[0]
    }
    pub fn background(&self) -> f32 {
        self.colors.background.rgba[0]
    }
    pub fn erases(&self) -> bool {
        self.colors.transparent()
    }
    pub fn artwork(&self) -> Option<LayerId> {
        self.editing.as_ref().map(|e| e.artwork)
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn selection_menu(&self, kind: SelectionMenu) -> ContextMenu {
        match kind {
            SelectionMenu::Selection => self.application_menu(ApplicationMenu::Select),
            SelectionMenu::QuickMask => self.quick_mask_menu(),
            SelectionMenu::Overlay => self.selection_overlay_menu(),
        }
    }

    fn selection_command_item(&self, command: CommandId) -> ContextMenuItem {
        let state = self.command(command);
        let mut item = ContextMenuItem::command(state.label, UiAction::Invoke { command });
        item.enabled = state.enabled;
        item.selected = command.is_toggle().then_some(state.selected);
        item
    }
    pub fn quick_mask_menu(&self) -> ContextMenu {
        ContextMenu {
            title: "Quick Mask · Temporary".into(),
            sections: vec![
                [CommandId::ReturnToArtwork, CommandId::SaveSelectionLayer]
                    .map(|c| self.selection_command_item(c))
                    .into(),
                [
                    CommandId::InvertSelection,
                    CommandId::SelectAll,
                    CommandId::ClearSelectionMask,
                    CommandId::FillSelectionMask,
                ]
                .map(|c| self.selection_command_item(c))
                .into(),
                vec![ContextMenuItem::submenu(
                    "Overlay",
                    self.selection_overlay_menu().sections,
                )],
            ],
        }
    }
    pub fn selection_overlay_menu(&self) -> ContextMenu {
        let palette = [
            ("Red", [1., 0., 0.]),
            ("Blue", [0., 0.4, 1.]),
            ("Green", [0., 0.8, 0.3]),
            ("Magenta", [1., 0., 1.]),
        ];
        ContextMenu {
            title: "Mask Overlay".into(),
            sections: vec![
                [CommandId::MaskOverlay, CommandId::MaskOverlayProtected]
                    .map(|c| self.selection_command_item(c))
                    .into(),
                palette
                    .into_iter()
                    .map(|(label, rgb)| {
                        let mut item = ContextMenuItem::command(
                            label,
                            UiAction::Selection {
                                action: SelectionAction::OverlayColor {
                                    color: [
                                        rgb[0],
                                        rgb[1],
                                        rgb[2],
                                        self.selection_tools.options.display.color[3],
                                    ],
                                },
                            },
                        );
                        item.selected =
                            Some(self.selection_tools.options.display.color[..3] == rgb);
                        item
                    })
                    .collect(),
            ],
        }
    }
    pub(super) fn selection_layer_menu(&self, id: LayerId) -> Result<ContextMenu, String> {
        let doc = self.engine.document();
        let layer = doc.layer(id).ok_or("Unknown selection layer")?;
        let unlocked = !doc.is_locked(id);
        let action = |label: &str, action, enabled| {
            let mut item = ContextMenuItem::command(label, UiAction::Selection { action });
            item.enabled = enabled;
            item
        };
        let organize = |label: &str, action, enabled| {
            let mut item = ContextMenuItem::command(label, UiAction::Layer { action });
            item.enabled = enabled;
            item
        };
        let mut uses: Vec<_> = [
            ("Load Selection", SelectionMode::New),
            ("Add to Selection", SelectionMode::Add),
            ("Subtract from Selection", SelectionMode::Subtract),
            ("Intersect with Selection", SelectionMode::Intersect),
        ]
        .into_iter()
        .map(|(label, mode)| {
            action(
                label,
                SelectionAction::LoadLayer {
                    id: id.0,
                    mode,
                    inverted: false,
                },
                true,
            )
        })
        .collect();
        uses.push(action(
            "Load Inverted Selection",
            SelectionAction::LoadLayer {
                id: id.0,
                mode: SelectionMode::New,
                inverted: true,
            },
            true,
        ));
        let roots = doc.layer_roots(&self.layer_interaction.selected);
        let multiple = roots.len() > 1 && self.layer_interaction.selected.contains(&id);
        let mut organize_items = vec![
            organize("Rename…", LayerAction::BeginRename { id: id.0 }, unlocked),
            organize(
                if multiple {
                    "Duplicate Selected Layers"
                } else {
                    "Duplicate"
                },
                if multiple {
                    LayerAction::DuplicateSelected
                } else {
                    LayerAction::Duplicate { id: id.0 }
                },
                true,
            ),
            organize(
                if multiple {
                    "Delete Selected Layers"
                } else {
                    "Delete"
                },
                if multiple {
                    LayerAction::DeleteSelected
                } else {
                    LayerAction::Delete { id: id.0 }
                },
                doc.can_delete_layers(if multiple {
                    &roots
                } else {
                    std::slice::from_ref(&id)
                }),
            ),
            organize(
                if layer.properties.locked {
                    "Unlock Editing"
                } else {
                    "Lock Editing"
                },
                LayerAction::Lock {
                    id: id.0,
                    value: !layer.properties.locked,
                },
                !layer.properties.parent.is_some_and(|p| doc.is_locked(p)),
            ),
            organize(
                "Group Selected Layers",
                LayerAction::GroupSelected,
                doc.group_layers_edit(
                    &doc.layer_roots(&self.layer_interaction.selected),
                    LayerId(0),
                )
                .is_ok(),
            ),
            organize(
                "Move to Root",
                LayerAction::Reparent {
                    id: id.0,
                    parent: None,
                    index: 0,
                },
                unlocked && layer.properties.parent.is_some(),
            ),
        ];
        let position = doc.layers.iter().position(|l| l.id == id).unwrap();
        for (label, up) in [("Move Up", true), ("Move Down", false)] {
            let neighbor = if up {
                doc.layers[..position]
                    .iter()
                    .rposition(|l| l.properties.parent == layer.properties.parent)
            } else {
                doc.layers[position + 1..]
                    .iter()
                    .position(|l| {
                        l.properties.parent == layer.properties.parent
                            && l.kind != LayerKind::Background
                    })
                    .map(|i| position + 1 + i)
            };
            organize_items.push(organize(
                label,
                LayerAction::Reparent {
                    id: id.0,
                    parent: layer.properties.parent.map(|p| p.0),
                    index: neighbor.unwrap_or(position) as u32,
                },
                unlocked && neighbor.is_some(),
            ));
        }
        let groups = doc
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Group)
            .map(|l| {
                organize(
                    &l.name,
                    LayerAction::Reparent {
                        id: id.0,
                        parent: Some(l.id.0),
                        index: 0,
                    },
                    unlocked && !doc.is_locked(l.id),
                )
            })
            .collect();
        organize_items.push(ContextMenuItem::submenu("Move into Group", vec![groups]));
        Ok(ContextMenu {
            title: format!("{} · Selection Layer", layer.name),
            sections: vec![
                vec![
                    action(
                        "Edit Selection Layer",
                        SelectionAction::EditLayer { id: id.0 },
                        true,
                    ),
                    self.selection_command_item(CommandId::ReturnToArtwork),
                ],
                uses,
                vec![action(
                    "Replace from Current Selection",
                    SelectionAction::ReplaceLayer { id: id.0 },
                    unlocked && self.current_selection().is_some(),
                )],
                vec![
                    action(
                        "Invert Stored Mask",
                        SelectionAction::InvertLayer { id: id.0 },
                        unlocked,
                    ),
                    action(
                        "Select Entire Canvas in Mask",
                        SelectionAction::ClearLayer {
                            id: id.0,
                            full: true,
                        },
                        unlocked,
                    ),
                    action(
                        "Clear Stored Mask",
                        SelectionAction::ClearLayer {
                            id: id.0,
                            full: false,
                        },
                        unlocked,
                    ),
                    action(
                        "Fill Mask",
                        SelectionAction::FillLayer { id: id.0 },
                        unlocked,
                    ),
                ],
                vec![
                    organize(
                        if layer.visible {
                            "Hide Overlay"
                        } else {
                            "Show Overlay"
                        },
                        LayerAction::Visibility {
                            id: id.0,
                            value: !layer.visible,
                        },
                        true,
                    ),
                    ContextMenuItem::submenu(
                        "Overlay Settings",
                        self.selection_overlay_menu().sections,
                    ),
                ],
                organize_items,
            ],
        })
    }
    fn saved_selection_label(&self, layer: &Layer) -> String {
        let mut names = vec![layer.name.to_string()];
        let mut parent = layer.properties.parent;
        while let Some(id) = parent {
            let Some(layer) = self.engine.document().layer(id) else {
                break;
            };
            names.push(layer.name.to_string());
            parent = layer.properties.parent;
        }
        names.reverse();
        names.join(" / ")
    }
    pub(super) fn replace_selection_menu_items(&self) -> Vec<ContextMenuItem> {
        let doc = self.engine.document();
        doc.layers
            .iter()
            .filter(|l| l.kind == LayerKind::Selection)
            .map(|l| {
                let mut item = ContextMenuItem::command(
                    &self.saved_selection_label(l),
                    UiAction::Selection {
                        action: SelectionAction::ReplaceLayer { id: l.id.0 },
                    },
                );
                item.enabled = self.current_selection().is_some() && !doc.is_locked(l.id);
                item
            })
            .collect()
    }
    pub(super) fn selection_source_menu_items(&self) -> Vec<ContextMenuItem> {
        let id = self
            .selection_masks
            .artwork()
            .unwrap_or(self.engine.document().active_layer);
        let Some(layer) = self.engine.document().layer(id) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if matches!(layer.kind, LayerKind::Paint | LayerKind::ImportedImage) {
            items.push(ContextMenuItem::submenu(
                "From Layer Opacity",
                vec![self.coverage_menu_items(id.0, false)],
            ));
        }
        if layer.mask.is_some() {
            items.push(ContextMenuItem::submenu(
                "From Layer Mask",
                vec![self.coverage_menu_items(id.0, true)],
            ));
        }
        items
    }
    pub(super) fn saved_selection_menu_items(&self) -> Vec<ContextMenuItem> {
        self.engine
            .document()
            .layers
            .iter()
            .filter(|l| l.kind == LayerKind::Selection)
            .map(|l| {
                ContextMenuItem::submenu(
                    &self.saved_selection_label(l),
                    vec![
                        [
                            ("Load Selection", SelectionMode::New, false),
                            ("Add to Selection", SelectionMode::Add, false),
                            ("Subtract from Selection", SelectionMode::Subtract, false),
                            ("Intersect with Selection", SelectionMode::Intersect, false),
                            ("Load Inverted Selection", SelectionMode::New, true),
                        ]
                        .into_iter()
                        .map(|(label, mode, inverted)| {
                            ContextMenuItem::command(
                                label,
                                UiAction::Selection {
                                    action: SelectionAction::LoadLayer {
                                        id: l.id.0,
                                        mode,
                                        inverted,
                                    },
                                },
                            )
                        })
                        .collect(),
                    ],
                )
            })
            .collect()
    }
    pub(super) fn coverage_menu_items(&self, id: u64, mask: bool) -> Vec<ContextMenuItem> {
        let labels = if mask {
            [
                "Load Mask as Selection",
                "Add Mask to Selection",
                "Subtract Mask from Selection",
                "Intersect with Mask",
            ]
        } else {
            [
                "Select Layer Opacity",
                "Add Opacity to Selection",
                "Subtract Opacity from Selection",
                "Intersect with Layer Opacity",
            ]
        };
        labels
            .into_iter()
            .zip([
                SelectionMode::New,
                SelectionMode::Add,
                SelectionMode::Subtract,
                SelectionMode::Intersect,
            ])
            .map(|(label, mode)| {
                ContextMenuItem::command(
                    label,
                    UiAction::Selection {
                        action: SelectionAction::LoadCoverage { id, mask, mode },
                    },
                )
            })
            .collect()
    }
    pub(super) fn current_selection(&self) -> Option<Selection> {
        self.engine
            .document()
            .selection
            .clone()
            .or_else(|| self.selection_masks.quick().then(Selection::full))
    }
    pub(super) fn mask_coverage(&self, target: SelectionTarget) -> Result<Selection, String> {
        match target {
            SelectionTarget::Current => {
                Ok(self.current_selection().unwrap_or_else(Selection::empty))
            }
            SelectionTarget::Saved(id) => self.engine.document().saved_selection(id).map_err(error),
        }
    }
    pub(super) fn mask_brush_reason(&self) -> Option<&'static str> {
        if self.selection_masks.target().is_none() {
            return None;
        }
        if let Some(SelectionTarget::Saved(id)) = self.selection_masks.target()
            && self.engine.document().is_locked(id)
        {
            return Some("This selection layer is locked");
        }
        (self.layer_interaction.tool == LayerCanvasTool::Paint
            && self.engine.configured_brush().execution_class() != layer_core::BrushExecution::Dry)
            .then_some(
                "Selection masks support dry brushes. Choose an ink, pencil, airbrush, or eraser.",
            )
    }
    pub(super) fn begin_selection_mask(&mut self, target: SelectionTarget) -> Result<(), String> {
        self.require_document_idle()?;
        if let SelectionTarget::Saved(id) = target {
            self.engine.document().saved_selection(id).map_err(error)?;
        }
        self.cancel_layer_gesture()?;
        let previous = self.selection_masks.editing.take();
        let artwork = previous
            .as_ref()
            .map_or(self.engine.document().active_layer, |e| e.artwork);
        let tool = previous.map_or(self.layer_interaction.tool, |e| e.tool);
        self.selection_masks.editing = Some(Editing {
            target,
            artwork,
            tool,
        });
        if let SelectionTarget::Saved(id) = target {
            self.engine.set_layer_visibility(id, true).map_err(error)?;
            self.engine.set_active_layer(id).map_err(error)?;
            self.layer_interaction.selected = std::collections::BTreeSet::from([id]);
        }
        self.layer_interaction.tool = LayerCanvasTool::Paint;
        self.refresh_document();
        Ok(())
    }
    pub(super) fn reconcile_selection_mask(&mut self) {
        let doc = self.engine.document();
        self.selection_masks.update_previews(doc);
        if let Some(SelectionTarget::Saved(id)) = self.selection_masks.target()
            && (doc.active_layer != id
                || doc.layer(id).is_none_or(|l| l.kind != LayerKind::Selection))
        {
            self.selection_masks.editing = None;
        }
        if self.selection_masks.target().is_none()
            && doc
                .layer(doc.active_layer)
                .is_some_and(|l| l.kind == LayerKind::Selection)
        {
            let artwork = doc
                .layers
                .iter()
                .find(|l| l.kind == LayerKind::Paint)
                .map_or(doc.active_layer, |l| l.id);
            self.selection_masks.editing = Some(Editing {
                target: SelectionTarget::Saved(doc.active_layer),
                artwork,
                tool: LayerCanvasTool::Paint,
            });
            self.layer_interaction.tool = LayerCanvasTool::Paint;
        }
    }
    pub(super) fn return_to_artwork(&mut self) -> Result<(), String> {
        let Some(editing) = self.selection_masks.editing.take() else {
            return Ok(());
        };
        self.cancel_selection_contact();
        let doc = self.engine.document();
        let artwork = doc
            .layer(editing.artwork)
            .filter(|l| l.is_artwork())
            .map(|l| l.id)
            .or_else(|| {
                doc.layers
                    .iter()
                    .find(|l| l.kind == LayerKind::Paint)
                    .map(|l| l.id)
            });
        if let Some(id) = artwork {
            self.engine.set_active_layer(id).map_err(error)?;
        }
        self.engine
            .apply_edit(Edit::SetMaskTarget(false))
            .map_err(error)?;
        self.layer_interaction.tool = editing.tool;
        self.engine.set_selection_display(None);
        self.refresh_document();
        Ok(())
    }
    pub(super) fn selection_mask_command(&mut self, command: CommandId) -> Result<bool, String> {
        match command {
            CommandId::QuickMask => {
                if self.selection_masks.quick() {
                    self.return_to_artwork()?;
                } else {
                    self.return_to_artwork()?;
                    self.begin_selection_mask(SelectionTarget::Current)?;
                }
            }
            CommandId::ReturnToArtwork => self.return_to_artwork()?,
            CommandId::NewSelectionLayer | CommandId::SaveSelectionLayer => {
                self.selection_action(SelectionAction::NewLayer {
                    parent: None,
                    save_current: command == CommandId::SaveSelectionLayer,
                })?
            }
            CommandId::Reselect => {
                let selection = self
                    .selection_masks
                    .reselect
                    .clone()
                    .ok_or("No selection to restore")?;
                self.return_to_artwork()?;
                self.layer_edit(Edit::SetSelection(Some(selection)))?;
            }
            CommandId::SelectionOutline => {
                self.selection_tools.options.display.outline =
                    !self.selection_tools.options.display.outline
            }
            CommandId::MaskOverlay => {
                self.selection_tools.options.display.overlay =
                    !self.selection_tools.options.display.overlay
            }
            CommandId::MaskOverlayProtected => {
                self.selection_tools.options.display.protected =
                    !self.selection_tools.options.display.protected
            }
            CommandId::ResetMaskColors => {
                self.selection_masks.colors.apply(ColorAction::SetSlot {
                    slot: ColorSlot::Foreground,
                    color: layer_core::color::RgbColor::BLACK,
                })?;
                self.selection_masks.colors.apply(ColorAction::SetSlot {
                    slot: ColorSlot::Background,
                    color: layer_core::color::RgbColor::WHITE,
                })?;
                self.selection_masks.colors.apply(ColorAction::Select {
                    slot: ColorSlot::Foreground,
                })?;
            }
            CommandId::SwapMaskColors => self.selection_masks.colors.apply(ColorAction::Swap)?,
            CommandId::ClearSelectionMask | CommandId::FillSelectionMask => {
                let target = self
                    .selection_masks
                    .target()
                    .ok_or("Choose a selection mask first")?;
                if command == CommandId::ClearSelectionMask {
                    self.set_mask_coverage(target, Selection::empty())?;
                } else {
                    self.queue_mask_fill(target, Selection::full())?;
                }
            }
            _ => return Ok(false),
        }
        self.refresh_document();
        Ok(true)
    }
    pub(super) fn set_mask_coverage(
        &mut self,
        target: SelectionTarget,
        selection: Selection,
    ) -> Result<(), String> {
        let edit = self
            .engine
            .document()
            .selection_edit(target, selection)
            .map_err(error)?;
        self.layer_edit(edit)
    }
    pub(super) fn selection_action(&mut self, action: SelectionAction) -> Result<(), String> {
        match action {
            SelectionAction::NewLayer {
                parent,
                save_current,
            } => {
                let mut selection = if save_current {
                    self.current_selection().ok_or("Make a selection first")?
                } else {
                    Selection::empty()
                };
                let parent = parent.map(LayerId);
                if let Some(id) = parent {
                    let doc = self.engine.document();
                    let group = doc.layer(id).ok_or("Unknown group")?;
                    if group.kind != LayerKind::Group || doc.is_locked(id) {
                        return Err("Choose an unlocked group".into());
                    }
                    selection = selection
                        .transformed(
                            doc.layer_transform(id)
                                .inverse()
                                .ok_or("Invalid group placement")?,
                        )
                        .map_err(error)?;
                }
                let id = self.engine.allocate_layer_id();
                let mut layer = Layer::selection(id, format!("Selection {}", id.0), selection);
                layer.properties.parent = parent;
                layer.visible = !save_current;
                self.layer_edit(Edit::InsertLayer { index: 0, layer })?;
                if !save_current {
                    self.begin_selection_mask(SelectionTarget::Saved(id))?;
                }
                self.state.layer_tools.rename_layer = Some(id.0);
            }
            SelectionAction::EditLayer { id } => {
                self.begin_selection_mask(SelectionTarget::Saved(LayerId(id)))?
            }
            SelectionAction::LoadThumbnail {
                id,
                mask,
                shift,
                alt,
            } => {
                let mode = match (shift, alt) {
                    (false, false) => SelectionMode::New,
                    (true, false) => SelectionMode::Add,
                    (false, true) => SelectionMode::Subtract,
                    (true, true) => SelectionMode::Intersect,
                };
                let saved = self
                    .engine
                    .document()
                    .layer(LayerId(id))
                    .is_some_and(|l| l.kind == LayerKind::Selection);
                self.selection_action(if saved && !mask {
                    SelectionAction::LoadLayer {
                        id,
                        mode,
                        inverted: false,
                    }
                } else {
                    SelectionAction::LoadCoverage { id, mask, mode }
                })?;
            }
            SelectionAction::LoadCoverage { id, mask, mode } => {
                let doc = self.engine.document();
                let layer = doc.layer(LayerId(id)).ok_or("Unknown layer")?;
                let target = if mask {
                    layer.mask.as_ref().ok_or("This layer has no mask")?.id
                } else {
                    if !matches!(layer.kind, LayerKind::Paint | LayerKind::ImportedImage) {
                        return Err("Choose a drawable layer with content alpha".into());
                    }
                    layer.id
                };
                let basis = doc.layer_transform(target);
                self.return_to_artwork()?;
                let options = layer_render::SelectionRefinement {
                    mode,
                    previous: self.engine.document().selection.clone().map(Arc::new),
                    antialias: true,
                    feather: 0.,
                    source_to_document: basis,
                };
                self.queue_mask_region(
                    SelectionTarget::Current,
                    layer_render::RegionRequest {
                        request_id: 0,
                        contiguous: false,
                        source: layer_render::RegionSource::Coverage(target),
                        position: [0, 0],
                        tolerance: 0.,
                        refinement: Default::default(),
                        limit: None,
                        selection: Some(options),
                    },
                    layer_core::Affine::IDENTITY,
                    false,
                )?;
            }
            SelectionAction::LoadLayer { id, mode, inverted } => {
                let mut selection = self
                    .engine
                    .document()
                    .saved_selection(LayerId(id))
                    .map_err(error)?;
                selection.inverted ^= inverted;
                self.return_to_artwork()?;
                if mode == SelectionMode::New {
                    self.layer_edit(Edit::SetSelection(Some(selection)))?;
                } else {
                    let options = layer_render::SelectionRefinement {
                        mode,
                        previous: self.engine.document().selection.clone().map(Arc::new),
                        antialias: true,
                        feather: 0.,
                        source_to_document: layer_core::Affine::IDENTITY,
                    };
                    self.queue_mask_region(
                        SelectionTarget::Current,
                        layer_render::RegionRequest {
                            request_id: 0,
                            contiguous: false,
                            source: layer_render::RegionSource::Selection(Arc::new(selection)),
                            position: [0, 0],
                            tolerance: 0.,
                            refinement: Default::default(),
                            limit: None,
                            selection: Some(options),
                        },
                        layer_core::Affine::IDENTITY,
                        false,
                    )?;
                }
            }
            SelectionAction::ReplaceLayer { id } => {
                let selection = self.current_selection().ok_or("Make a selection first")?;
                self.set_mask_coverage(SelectionTarget::Saved(LayerId(id)), selection)?;
            }
            SelectionAction::InvertLayer { id } => {
                let target = SelectionTarget::Saved(LayerId(id));
                let mut selection = self.mask_coverage(target)?;
                selection.inverted = !selection.inverted;
                self.set_mask_coverage(target, selection)?;
            }
            SelectionAction::ClearLayer { id, full } => self.set_mask_coverage(
                SelectionTarget::Saved(LayerId(id)),
                if full {
                    Selection::full()
                } else {
                    Selection::empty()
                },
            )?,
            SelectionAction::FillLayer { id } => {
                self.queue_mask_fill(SelectionTarget::Saved(LayerId(id)), Selection::full())?
            }
            SelectionAction::OverlayColor { color } => {
                let mut display = self.selection_tools.options.display.clone();
                display.color = color;
                display.validate()?;
                self.selection_tools.options.display = display;
            }
        }
        self.refresh_document();
        Ok(())
    }
    pub(super) fn mask_editing_view(&self) -> Option<MaskEditingView> {
        let target = self.selection_masks.target()?;
        let layer = match target {
            SelectionTarget::Current => None,
            SelectionTarget::Saved(id) => Some(id),
        };
        Some(MaskEditingView {
            layer: layer.map(|id| id.0),
            label: layer
                .and_then(|id| self.engine.document().layer(id))
                .map_or_else(
                    || "Editing selection · Quick Mask".into(),
                    |l| format!("Editing selection layer: {}", l.name),
                ),
            gray: self.selection_masks.colors.foreground.rgba[0],
            background: self.selection_masks.colors.background.rgba[0],
            colors: self.selection_masks.colors.clone(),
            overlay: self.selection_tools.options.display.overlay,
            reason: self.mask_brush_reason(),
        })
    }
    pub(super) fn edit_mask_control(&mut self, id: &str, value: f32) -> Result<(), String> {
        NumericControl::percent().validate(value, "Mask value")?;
        match id {
            "mask_gray" | "mask_background" => {
                self.selection_masks.colors.apply(ColorAction::SetSlot {
                    slot: if id == "mask_gray" {
                        ColorSlot::Foreground
                    } else {
                        ColorSlot::Background
                    },
                    color: layer_core::color::RgbColor::new(
                        layer_core::color::RgbSpace::Srgb,
                        [value, value, value, 1.],
                    )?,
                })?
            }
            "mask_overlay_opacity" => self.selection_tools.options.display.color[3] = value,
            _ => return Err("Unknown mask control".into()),
        }
        self.refresh_tools();
        Ok(())
    }
}

impl UiState {
    /// Mask colors are a separate scalar pair; artwork definitions remain intact.
    pub fn display_colors(&self) -> &ColorState {
        self.layer_tools
            .mask_editing
            .as_ref()
            .map_or(&self.colors, |m| &m.colors)
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn mask_color_action(&mut self, action: ColorAction) -> Result<(), String> {
        self.selection_masks.colors.apply(action)?;
        self.selection_masks.colors.constrain_grayscale()?;
        self.refresh_tools();
        Ok(())
    }
}
