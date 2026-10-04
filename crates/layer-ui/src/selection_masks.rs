//! Selection destinations and lifecycle. Temporary editing never creates a
//! document layer; stored masks use ordinary tree identity and shared history.
use super::*;
use layer_core::{
    Edit, Selection, SelectionMaskProperties, SelectionTarget,
    authored::{OccurrenceHandle, OccurrenceContent, Occurrence, RecordChange, SourceTarget, SavedSelection},
};
use std::sync::Arc;
use super::session::{occurrence_token, occurrence_handle};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMenu {
    Selection,
    QuickMask,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionDisplayOptions {
    pub outline: bool,
    pub overlay: bool,
    pub color: [f32; 4],
}
impl Default for SelectionDisplayOptions {
    fn default() -> Self {
        Self {
            outline: true,
            overlay: true,
            color: [1., 0., 0., 0.5],
        }
    }
}
impl SelectionDisplayOptions {
    pub fn validate(&self) -> Result<(), NumericError> {
        for value in self.color {
            NumericControl::percent().validate(value, MessageId::RESOURCES_MASK_OVERLAY_COLOR)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SelectionAction {
    /// Open the Refine dialog for Quick Mask (`layer` 0), a Selection Layer,
    /// or the current selection (`None`).
    BeginRefine {
        kind: super::selection_refine::RefineKind,
        layer: Option<u64>,
    },
    ResizeRadius {
        radius: f32,
    },
    ApplyResize,
    CancelResize,
    NewLayer {
        parent: Option<u64>,
        save_current: bool,
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
    artwork: Option<OccurrenceHandle>,
    tool: LayerCanvasTool,
}
pub(super) struct SelectionMasks {
    editing: Option<Editing>,
    pub(super) refine: Option<super::selection_refine::RefineDraft>,
    pub(super) refine_changed: bool,
    /// Refine values received, and results drawn on the canvas before the
    /// document held them.
    pub(super) refine_values: u64,
    pub(super) refine_previews: u64,
    pub reselect: Option<Selection>,
    pub(super) colors: ColorState,
    pub quick_properties: SelectionMaskProperties,
    pub quick_visible: bool,
    pub quick_property_gesture: Option<(String, SelectionMaskProperties)>,
    previews: std::collections::BTreeMap<Option<OccurrenceHandle>, (Selection, u64)>,
    preview_revision: u64,
}
impl Default for SelectionMasks {
    fn default() -> Self {
        let mut colors = ColorState::default();
        colors.foreground = layer_core::color::RgbColor::BLACK;
        colors.background = layer_core::color::RgbColor::WHITE;
        colors
            .set_document_depth(layer_core::color::SampleDepth::U8)
            .expect("scalar mask color depth");
        Self {
            editing: None,
            refine: None,
            refine_changed: false,
            refine_values: 0,
            refine_previews: 0,
            reselect: None,
            colors,
            quick_properties: Default::default(),
            quick_visible: true,
            quick_property_gesture: None,
            previews: Default::default(),
            preview_revision: 0,
        }
    }
}
/// Mask coverage uses display-encoded luminance; the picker retains the color.
pub(super) fn mask_gray(color: layer_core::color::RgbColor) -> f32 {
    let c = color.encoded_in(layer_core::color::RgbSpace::Srgb).expect("validated mask color");
    (c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722).clamp(0., 1.)
}

impl SelectionMasks {
    pub fn preview_revision(&self, id: Option<OccurrenceHandle>) -> u64 {
        self.previews.get(&id).map_or(0, |(_, revision)| *revision)
    }
    fn update_previews(&mut self, doc: &layer_core::Document) {
        self.previews.retain(|id,_| id.is_none() || id.is_some_and(|id|matches!(doc.scene().source_target(id),Some(SourceTarget::Selection(_)))));
        if self.quick() {
            let mask=doc.working.selection.clone().unwrap_or_else(Selection::empty);
            if self.previews.get(&None).is_none_or(|(old,_)|*old!=mask) {
                self.preview_revision=self.preview_revision.wrapping_add(1);
                self.previews.insert(None,(mask,self.preview_revision));
            }
        } else {self.previews.remove(&None);}
        for &id in doc.scene().order() {
            if !matches!(doc.scene().source_target(id),Some(SourceTarget::Selection(_))) {continue;}
            let Ok(mask)=doc.saved_selection(id) else {continue;};
            if self.previews.get(&Some(id)).is_none_or(|(old,_)|*old!=mask) {
                self.preview_revision=self.preview_revision.wrapping_add(1);
                self.previews.insert(Some(id),(mask,self.preview_revision));
            }
        }
    }
    pub fn refine_view(&self) -> Option<super::selection_refine::SelectionRefineView> {
        self.refine.as_ref().and_then(|d| d.view())
    }
    pub fn target(&self) -> Option<SelectionTarget> {
        self.editing.as_ref().map(|e| e.target)
    }
    pub fn quick(&self) -> bool {
        self.target() == Some(SelectionTarget::Current)
    }
    pub fn gray(&self) -> f32 {
        mask_gray(self.colors.definition())
    }
    pub fn erases(&self) -> bool {
        self.colors.transparent()
    }
    pub fn artwork(&self) -> Option<OccurrenceHandle> {
        self.editing.as_ref().and_then(|e| e.artwork)
    }
}

fn load_selection_items(id: u64, localization: &Localizer) -> Vec<ContextMenuItem> {
    [
        (MessageId::RESOURCES_SELECTION_MENU_LOAD_SELECTION, SelectionMode::New, false),
        (MessageId::RESOURCES_SELECTION_MENU_ADD_TO_SELECTION, SelectionMode::Add, false),
        (MessageId::RESOURCES_SELECTION_MENU_SUBTRACT_FROM_SELECTION, SelectionMode::Subtract, false),
        (MessageId::RESOURCES_SELECTION_MENU_INTERSECT_WITH_SELECTION, SelectionMode::Intersect, false),
        (MessageId::RESOURCES_SELECTION_MENU_LOAD_INVERTED_SELECTION, SelectionMode::New, true),
    ]
    .into_iter()
    .map(|(label, mode, inverted)| {
        ContextMenuItem::command(localization.text(label).as_ref(), UiAction::Selection { action: SelectionAction::LoadLayer { id, mode, inverted } })
    })
    .collect()
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn selection_menu(&self, kind: SelectionMenu) -> ContextMenu {
        match kind {
            SelectionMenu::Selection => self.application_menu(ApplicationMenu::Select),
            SelectionMenu::QuickMask => self.quick_mask_menu(),
        }
    }

    fn selection_command_item(&self, command: CommandId) -> ContextMenuItem {
        let state = self.command(command);
        let mut item = ContextMenuItem::command(state.label.as_ref(), UiAction::Invoke { command });
        item.enabled = state.enabled;
        item.selected = command.is_toggle().then_some(state.selected);
        item
    }
    /// Grow… to Smooth… with their short labels, as commands or for one Selection Layer.
    pub(super) fn refine_items(&self, layer: Option<OccurrenceHandle>) -> Vec<ContextMenuItem> {
        use super::selection_refine::RefineKind;
        RefineKind::ALL
            .into_iter()
            .map(|kind| {
                let label = canvas_bar::short_label(kind.command(), self.localization());
                match layer {
                    None => ContextMenuItem { label: label.to_string(), ..self.selection_command_item(kind.command()) },
                    Some(id) => ContextMenuItem {
                        enabled: self.require_document_idle().is_ok() && !self.engine.document().is_locked(id),
                        ..ContextMenuItem::command(
                            label.as_ref(),
                            UiAction::Selection { action: SelectionAction::BeginRefine { kind, layer: Some(occurrence_token(id)) } },
                        )
                    },
                }
            })
            .collect()
    }
    pub fn quick_mask_menu(&self) -> ContextMenu {
        ContextMenu {
            title: self.localization().text(MessageId::RESOURCES_SELECTION_MENU_QUICK_MASK).to_string(),
            sections: vec![
                vec![
                    self.selection_command_item(CommandId::ReturnToArtwork),
                    self.selection_command_item(CommandId::SaveSelectionLayer),
                ],
                vec![ContextMenuItem::submenu(
                    self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MODIFY).as_ref(),
                    vec![
                        [
                            CommandId::InvertSelection,
                            CommandId::SelectAll,
                            CommandId::ClearSelectionMask,
                            CommandId::FillSelectionMask,
                        ]
                        .map(|c| self.selection_command_item(c))
                        .into(),
                        self.refine_items(None),
                    ],
                )],
            ],
        }
    }
    pub(super) fn selection_layer_menu(&self, id: OccurrenceHandle) -> Result<ContextMenu, String> {
        let doc = self.engine.document();
        let layer = doc.scene().occurrence(id).ok_or("Unknown selection layer")?;
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
        let editing = self.selection_masks.target() == Some(SelectionTarget::Saved(id));
        let edited = |label: &str, command: CommandId, item: ContextMenuItem| {
            if !editing {
                return item;
            }
            ContextMenuItem {
                enabled: self.command(command).enabled,
                ..ContextMenuItem::command(label, UiAction::Invoke { command })
            }
        };
        let mut load = load_selection_items(occurrence_token(id), self.localization()).into_iter();
        let load: Vec<_> = load
            .next()
            .map(|item| edited(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_LOAD_SELECTION).as_ref(), CommandId::LoadSelectionLayer, item))
            .into_iter()
            .chain(load)
            .collect();
        let roots = doc.layer_roots(&self.layer_interaction.selected);
        let multiple = roots.len() > 1 && self.layer_interaction.selected.contains(&id);
        let mut organize_items = vec![
            organize(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_RENAME).as_ref(), LayerAction::BeginRename { id: occurrence_token(id) }, unlocked),
            organize(
                self.localization().text(if multiple { MessageId::RESOURCES_SELECTION_MENU_DUPLICATE_SELECTED_LAYERS } else { MessageId::RESOURCES_SELECTION_MENU_DUPLICATE }).as_ref(),
                if multiple {
                    LayerAction::DuplicateSelected
                } else {
                    LayerAction::Duplicate { id: occurrence_token(id) }
                },
                true,
            ),
            organize(
                self.localization().text(if multiple { MessageId::RESOURCES_SELECTION_MENU_DELETE_SELECTED_LAYERS } else { MessageId::RESOURCES_SELECTION_MENU_DELETE }).as_ref(),
                if multiple {
                    LayerAction::DeleteSelected
                } else {
                    LayerAction::Delete { id: occurrence_token(id) }
                },
                doc.can_delete_layers(if multiple {
                    &roots
                } else {
                    std::slice::from_ref(&id)
                }),
            ),
            organize(
                self.localization().text(if layer.locked { MessageId::RESOURCES_SELECTION_MENU_UNLOCK_EDITING } else { MessageId::RESOURCES_SELECTION_MENU_LOCK_EDITING }).as_ref(),
                LayerAction::Lock {
                    id: occurrence_token(id),
                    value: !layer.locked,
                },
                !doc.scene().parent(id).is_some_and(|p| doc.is_locked(p)),
            ),
            organize(
                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_GROUP_SELECTED_LAYERS).as_ref(),
                LayerAction::GroupSelected,
                doc.group_layers_edit(
                    &doc.layer_roots(&self.layer_interaction.selected),
                    layer_core::LayerBlend::Normal,
                    "",
                )
                .is_ok(),
            ),
            organize(
                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MOVE_TO_ROOT).as_ref(),
                LayerAction::Reparent {
                    id: occurrence_token(id),
                    parent: None,
                    index: 0,
                },
                unlocked && doc.scene().parent(id).is_some(),
            ),
        ];
        let siblings=doc.scene().children(doc.scene().parent(id));
        let position=siblings.iter().position(|h|*h==id).unwrap();
        for (label,up) in [(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MOVE_UP),true),(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MOVE_DOWN),false)] {
            let neighbor=if up {position.checked_sub(1)} else {(position+1<siblings.len()).then_some(position+1)};
            organize_items.push(organize(label.as_ref(),LayerAction::Reparent {
                id:occurrence_token(id),parent:doc.scene().parent(id).map(occurrence_token),index:neighbor.unwrap_or(position) as u32,
            },unlocked && neighbor.is_some()));
        }
        let groups=doc.scene().order().iter().copied().filter(|h|doc.scene().occurrence(*h).is_some_and(|o|o.kind()==LayerKind::Group)).map(|group| {
            organize(&doc.scene().occurrence(group).unwrap().name,LayerAction::Reparent {id:occurrence_token(id),parent:Some(occurrence_token(group)),index:0},unlocked && !doc.is_locked(group))
        }).collect();
        organize_items.push(ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MOVE_INTO_GROUP).as_ref(), vec![groups]));
        Ok(ContextMenu {
            title: layer.name.to_string(),
            sections: vec![
                vec![ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_LOAD_SELECTION).as_ref(), vec![load])],
                vec![ContextMenuItem::submenu(
                    self.localization().text(MessageId::RESOURCES_SELECTION_MENU_MODIFY).as_ref(),
                    vec![
                        vec![
                            action(
                                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_REPLACE_FROM_CURRENT_SELECTION).as_ref(),
                                SelectionAction::ReplaceLayer { id: occurrence_token(id) },
                                unlocked && self.current_selection().is_some(),
                            ),
                            edited(
                                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_INVERT).as_ref(),
                                CommandId::InvertSelectionLayer,
                                action(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_INVERT).as_ref(), SelectionAction::InvertLayer { id: occurrence_token(id) }, unlocked),
                            ),
                            action(
                                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_SELECT_ALL).as_ref(),
                                SelectionAction::ClearLayer {
                                    id: occurrence_token(id),
                                    full: true,
                                },
                                unlocked,
                            ),
                            action(
                                self.localization().text(MessageId::RESOURCES_SELECTION_MENU_CLEAR).as_ref(),
                                SelectionAction::ClearLayer {
                                    id: occurrence_token(id),
                                    full: false,
                                },
                                unlocked,
                            ),
                            action(self.localization().text(MessageId::RESOURCES_SELECTION_MENU_FILL).as_ref(), SelectionAction::FillLayer { id: occurrence_token(id) }, unlocked),
                        ],
                        self.refine_items(Some(id)),
                    ],
                )],
                vec![ContextMenuItem::submenu(
                    self.localization().text(MessageId::RESOURCES_SELECTION_MENU_ORGANIZE).as_ref(),
                    vec![organize_items.split_off(4)],
                )],
                organize_items,
            ],
        })
    }

    fn saved_selection_label(&self, id: OccurrenceHandle) -> String {
        let scene=self.engine.document().scene();
        let mut names=Vec::new();let mut current=Some(id);
        while let Some(id)=current {
            let Some(o)=scene.occurrence(id) else {break;};names.push(o.name.to_string());current=scene.parent(id);
        }
        names.reverse();names.join(" / ")
    }
    pub(super) fn replace_selection_menu_items(&self) -> Vec<ContextMenuItem> {
        let doc=self.engine.document();
        doc.scene().order().iter().copied().filter(|h|matches!(doc.scene().source_target(*h),Some(SourceTarget::Selection(_)))).map(|id| {
            let mut item=ContextMenuItem::command(self.saved_selection_label(id),UiAction::Selection {action:SelectionAction::ReplaceLayer {id:occurrence_token(id)}});
            item.enabled=self.current_selection().is_some() && !doc.is_locked(id);item
        }).collect()
    }
    pub(super) fn selection_source_menu_items(&self) -> Vec<ContextMenuItem> {
        let doc=self.engine.document();
        let Some(id)=self.selection_masks.artwork().or(doc.working.occurrence) else {return Vec::new();};
        let Some(layer)=doc.scene().occurrence(id) else {return Vec::new();};let mut items=Vec::new();
        if layer.kind()==LayerKind::Paint {items.push(ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_COVERAGE_FROM_LAYER_OPACITY).as_ref(),vec![self.coverage_menu_items(occurrence_token(id),false)]));}
        if layer.mask.is_some() {items.push(ContextMenuItem::submenu(self.localization().text(MessageId::RESOURCES_COVERAGE_FROM_LAYER_MASK).as_ref(),vec![self.coverage_menu_items(occurrence_token(id),true)]));}
        items
    }
    pub(super) fn saved_selection_menu_items(&self) -> Vec<ContextMenuItem> {
        let scene=self.engine.document().scene();
        scene.order().iter().copied().filter(|h|matches!(scene.source_target(*h),Some(SourceTarget::Selection(_)))).map(|id| {
            ContextMenuItem::submenu(&self.saved_selection_label(id),vec![load_selection_items(occurrence_token(id),self.localization())])
        }).collect()
    }
    pub(super) fn coverage_menu_items(&self, id: u64, mask: bool) -> Vec<ContextMenuItem> {
        let labels = if mask {
            [
                MessageId::RESOURCES_SELECTION_MENU_LOAD_MASK_AS_SELECTION,
                MessageId::RESOURCES_SELECTION_MENU_ADD_MASK_TO_SELECTION,
                MessageId::RESOURCES_SELECTION_MENU_SUBTRACT_MASK_FROM_SELECTION,
                MessageId::RESOURCES_SELECTION_MENU_INTERSECT_WITH_MASK,
            ]
        } else {
            [
                MessageId::RESOURCES_SELECTION_MENU_SELECT_LAYER_OPACITY,
                MessageId::RESOURCES_SELECTION_MENU_ADD_OPACITY_TO_SELECTION,
                MessageId::RESOURCES_SELECTION_MENU_SUBTRACT_OPACITY_FROM_SELECTION,
                MessageId::RESOURCES_SELECTION_MENU_INTERSECT_WITH_LAYER_OPACITY,
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
                    self.localization().text(label).as_ref(),
                    UiAction::Selection {
                        action: SelectionAction::LoadCoverage { id, mask, mode },
                    },
                )
            })
            .collect()
    }
    /// `current_selection().is_some()` without copying the selection.
    pub(super) fn has_selection(&self) -> bool {
        self.engine.document().working.selection.is_some() || self.selection_masks.quick()
    }
    pub(super) fn current_selection(&self) -> Option<Selection> {
        self.engine
            .document()
            .working.selection
            .clone()
            .or_else(|| self.selection_masks.quick().then(Selection::empty))
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
        self.selection_masks.target()?;
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
        let tonal = self.tonal_active();
        let keep_draft = tonal && target == SelectionTarget::Current
            && self.tonal_tools.draft.as_ref().is_some_and(|d| d.target == target);
        if !keep_draft { self.cancel_layer_gesture()?; }
        if self.selection_masks.target().is_none() {
            self.selection_masks.colors = self.state.colors.clone();
            self.selection_masks
                .colors
                .set_document_depth(layer_core::color::SampleDepth::U8)?;
        }
        let previous = self.selection_masks.editing.take();
        let artwork = previous
            .as_ref()
            .map_or(self.engine.document().working.occurrence, |e| e.artwork);
        let tool = previous.map_or(self.layer_interaction.tool, |e| e.tool);
        self.selection_masks.editing = Some(Editing {
            target,
            artwork,
            tool,
        });
        if let SelectionTarget::Saved(id) = target {
            self.engine.set_active_layer(id).map_err(error)?;
            self.layer_interaction.selected = std::collections::BTreeSet::from([id]);
        }
        self.layer_interaction.tool = if tonal {LayerCanvasTool::Selection {kind:SelectionTool::Tonal}} else {LayerCanvasTool::Paint};
        self.refresh_document();
        self.sync_selection_overlay();
        Ok(())
    }
    pub(super) fn reconcile_selection_mask(&mut self) {
        let doc=self.engine.document();self.selection_masks.update_previews(doc);
        if let Some(SelectionTarget::Saved(id))=self.selection_masks.target()
            && (doc.working.occurrence!=Some(id) || !matches!(doc.scene().source_target(id),Some(SourceTarget::Selection(_)))) {
            self.selection_masks.editing=None;
        }
        if self.selection_masks.target().is_none()
            && let Some(id)=doc.working.occurrence.filter(|id|matches!(doc.scene().source_target(*id),Some(SourceTarget::Selection(_)))) {
            let artwork=doc.scene().order().iter().copied().find(|h|matches!(doc.scene().source_target(*h),Some(SourceTarget::Paint(_))));
            self.selection_masks.editing=Some(Editing {target:SelectionTarget::Saved(id),artwork,tool:LayerCanvasTool::Paint});
            self.layer_interaction.tool=LayerCanvasTool::Paint;
        }
    }
    pub(super) fn return_to_artwork(&mut self) -> Result<(), String> {
        let Some(editing)=self.selection_masks.editing.take() else {return Ok(());};
        let tonal=self.tonal_active();if tonal && editing.target!=SelectionTarget::Current {self.cancel_tonal();}
        self.cancel_selection_contact();
        let doc=self.engine.document();
        let artwork=editing.artwork.filter(|h|doc.scene().occurrence(*h).is_some_and(Occurrence::is_artwork)).or_else(||
            doc.scene().order().iter().copied().find(|h|matches!(doc.scene().source_target(*h),Some(SourceTarget::Paint(_)))));
        if let Some(id)=artwork.filter(|id|Some(*id)!=doc.working.occurrence) {self.engine.set_active_layer(id).map_err(error)?;}
        let doc=self.engine.document();
        if matches!(doc.working.target,Some(SourceTarget::Coverage(_))) {
            let mut working=doc.working.clone();working.target=working.occurrence.and_then(|h|doc.scene().source_target(h));working.inspect_mask=None;
            self.engine.apply_edit(Edit::Working(working)).map_err(error)?;
        }
        self.layer_interaction.tool=if tonal {LayerCanvasTool::Selection {kind:SelectionTool::Tonal}} else {editing.tool};
        self.engine.set_selection_display(None);self.refresh_document();self.sync_selection_overlay();Ok(())
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
            CommandId::LoadSelectionLayer | CommandId::InvertSelectionLayer => {
                let Some(SelectionTarget::Saved(id)) = self.selection_masks.target() else {
                    return Err("Edit a Selection Layer first".into());
                };
                self.selection_action(if command == CommandId::LoadSelectionLayer {
                    SelectionAction::LoadLayer { id: occurrence_token(id), mode: SelectionMode::New, inverted: false }
                } else {
                    SelectionAction::InvertLayer { id: occurrence_token(id) }
                })?
            }
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
                self.set_mask_coverage(SelectionTarget::Current,selection)?;
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
                // Keep old customized buttons/shortcuts readable, but make
                // their toggle select the coupled mode, not a second polarity.
                let layer = match self.selection_masks.target().ok_or("Select a mask first")? {
                    SelectionTarget::Current => 0,
                    SelectionTarget::Saved(id) => occurrence_token(id),
                };
                self.effect_action(EffectAction::Set {
                    layer,
                    key: "mask_mode".into(),
                    value: layer_core::EffectValue::Choice(u32::from(
                        !self.grayscale_masks(),
                    )),
                })?;
            }
            CommandId::ResetMaskColors => {
                self.selection_masks.colors.apply(ColorAction::SetSlot {
                    slot: ColorSlot::Foreground,
                    color: layer_core::color::RgbColor::BLACK,
                }).map_err(|reason|reason.message(crate::ColorInputModel::DocumentRgb, self.localization()))?;
                self.selection_masks.colors.apply(ColorAction::SetSlot {
                    slot: ColorSlot::Background,
                    color: layer_core::color::RgbColor::WHITE,
                }).map_err(|reason|reason.message(crate::ColorInputModel::DocumentRgb, self.localization()))?;
                self.selection_masks.colors.apply(ColorAction::Select {
                    slot: ColorSlot::Foreground,
                }).map_err(|reason|reason.message(crate::ColorInputModel::DocumentRgb, self.localization()))?;
            }
            CommandId::SwapMaskColors => self.selection_masks.colors.apply(ColorAction::Swap).map_err(|reason|reason.message(crate::ColorInputModel::DocumentRgb, self.localization()))?,
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
        if self.selection_masks.quick() {
            match &action {
                SelectionAction::LoadLayer { id: 0, .. }
                | SelectionAction::LoadThumbnail { id: 0, .. } => return self.return_to_artwork(),
                _ => (),
            }
        }
        match action {
            SelectionAction::BeginRefine { kind, layer } => self.begin_refine(kind, layer)?,
            SelectionAction::ResizeRadius { radius } => return self.set_refine_radius(radius),
            SelectionAction::ApplyResize => self.apply_refine()?,
            SelectionAction::CancelResize => self.cancel_refine()?,
            SelectionAction::NewLayer {
                parent,
                save_current,
            } => {
                let mut selection = if save_current {
                    self.current_selection().ok_or("Make a selection first")?
                } else {
                    Selection::empty()
                };
                let parent = parent.map(occurrence_handle).transpose()?;
                if let Some(id) = parent {
                    let doc = self.engine.document();
                    let group = doc.scene().occurrence(id).ok_or("Unknown group")?;
                    if group.kind() != LayerKind::Group || doc.is_locked(id) {
                        return Err("Choose an unlocked group".into());
                    }
                    selection = selection
                        .transformed(
                            group.placement.as_affine().map(|map|map.then(layer_core::Affine::translation(doc.layer_offset(id))))
                                .and_then(layer_core::Affine::inverse)
                                .ok_or("Invalid group placement")?,
                        )
                        .map_err(error)?;
                }
                let doc=self.engine.document();
                let saved=RecordChange::insert(&doc.artwork.selections,SavedSelection {selection,display:if save_current && self.selection_masks.target().is_some() {self.mask_properties()} else {Default::default()}});
                let occurrence=RecordChange::insert(&doc.artwork.occurrences,Occurrence::new(OccurrenceContent::Selection(saved.handle),
                    self.numbered_document_name(MessageId::DOCUMENTS_SELECTION_NAME,u64::from(doc.artwork.occurrences.next_handle().index())+1)));
                let id=occurrence.handle;
                let stack=match parent {Some(h)=>match doc.scene().occurrence(h).map(|o|o.content.clone()) {Some(OccurrenceContent::Stack(s))=>s,_=>return Err("Choose a group".into())},None=>doc.composition().result};
                let mut entries=doc.artwork.stacks.get(stack).ok_or("Missing stack")?.clone();entries.entries.insert(0,id);
                let stack=RecordChange::replace(&doc.artwork.stacks,stack,Some(entries)).map_err(error)?;
                self.layer_edit(Edit::Batch(vec![Edit::SavedSelection(saved),Edit::Occurrence(occurrence),Edit::Stack(stack)]))?;
                self.begin_selection_mask(SelectionTarget::Saved(id))?;
                self.state.layer_tools.rename_layer=Some(occurrence_token(id));
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
                    .scene().occurrence(occurrence_handle(id)?)
                    .is_some_and(|l| l.kind() == LayerKind::Selection);
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
                let layer = doc.scene().occurrence(occurrence_handle(id)?).ok_or("Unknown layer")?;
                let target = if mask {
                    SourceTarget::Coverage(layer.mask.as_ref().ok_or("This layer has no mask")?.source)
                } else {
                    if layer.kind() != LayerKind::Paint {
                        return Err("Choose a drawable layer with content alpha".into());
                    }
                    doc.scene().source_target(occurrence_handle(id)?).ok_or("Missing paint source")?
                };
                self.return_to_artwork()?;
                let options = layer_render::SelectionRefinement {
                    resize: 0,
                    mode,
                    previous: self.engine.document().working.selection.clone().map(Arc::new),
                    antialias: true,
                    feather: 0.,
                    source_to_document: layer_core::Affine::IDENTITY,
                    keep_canvas_edges: false,
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
                    .saved_selection(occurrence_handle(id)?)
                    .map_err(error)?;
                selection.inverted ^= inverted;
                self.return_to_artwork()?;
                if mode == SelectionMode::New {
                    self.set_mask_coverage(SelectionTarget::Current,selection)?;
                } else {
                    let options = layer_render::SelectionRefinement {
                        resize: 0,
                        mode,
                        previous: self.engine.document().working.selection.clone().map(Arc::new),
                        antialias: true,
                        feather: 0.,
                        source_to_document: layer_core::Affine::IDENTITY,
                        keep_canvas_edges: false,
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
                self.set_mask_coverage(SelectionTarget::Saved(occurrence_handle(id)?), selection)?;
            }
            SelectionAction::InvertLayer { id } => {
                let target = SelectionTarget::Saved(occurrence_handle(id)?);
                let mut selection = self.mask_coverage(target)?;
                selection.inverted = !selection.inverted;
                self.set_mask_coverage(target, selection)?;
            }
            SelectionAction::ClearLayer { id, full } => self.set_mask_coverage(
                SelectionTarget::Saved(occurrence_handle(id)?),
                if full {
                    Selection::full()
                } else {
                    Selection::empty()
                },
            )?,
            SelectionAction::FillLayer { id } => {
                self.queue_mask_fill(SelectionTarget::Saved(occurrence_handle(id)?), Selection::full())?
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
            layer: layer.map(occurrence_token),
            label: layer
                .and_then(|id| self.engine.document().scene().occurrence(id))
                .map_or_else(|| "Quick Mask".into(), |l| l.name.to_string()),
            gray: self.selection_masks.colors.foreground.rgba[0],
            background: self.selection_masks.colors.background.rgba[0],
            colors: self.selection_masks.colors.clone(),
            overlay: self.selection_tools.options.display.overlay,
            reason: self.mask_brush_reason(),
        })
    }
}

impl UiState {
    /// Mask painting keeps independent colors; artwork definitions remain intact.
    pub fn display_colors(&self) -> &ColorState {
        self.layer_tools
            .mask_editing
            .as_ref()
            .map_or(&self.colors, |m| &m.colors)
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn mask_color_action(&mut self, action: ColorAction) -> Result<(), String> {
        self.selection_masks.colors.apply(action).map_err(|reason|reason.message(crate::ColorInputModel::DocumentRgb, self.localization()))?;
        self.refresh_tools();
        Ok(())
    }
}
