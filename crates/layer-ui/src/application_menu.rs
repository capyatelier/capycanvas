//! Menu identity and contents are shared policy. Hosts only project these models.
use super::*;

pub const NAVIGATOR_COMMANDS: [CommandId; 6] = [CommandId::ZoomOut, CommandId::ZoomIn,
    CommandId::RotateLeft, CommandId::RotateRight, CommandId::FlipHorizontal, CommandId::FlipVertical];

#[derive(Clone, Debug, serde::Serialize)]
pub struct ZoomMenu {
    #[serde(flatten)]
    pub menu: ContextMenu,
    pub rotation_section: usize,
    pub buttons: Vec<CommandState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationLink {
    Website,
    SourceCode,
}
impl ApplicationLink {
    pub fn canonical_label(self) -> std::sync::Arc<str> {
        self.localized_label(&Localizer::shared(UiLanguage::English))
    }
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(match self {
            Self::Website => CommandId::Website.message_id(),
            Self::SourceCode => CommandId::SourceCode.message_id(),
        })
    }
    pub fn display(self) -> &'static str {
        match self {
            Self::Website => "capycanvas.art",
            Self::SourceCode => "github.com/capyatelier/capycanvas",
        }
    }
    pub fn url(self) -> &'static str {
        match self {
            Self::Website => "https://capycanvas.art/",
            Self::SourceCode => "https://github.com/capyatelier/capycanvas",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationMenu {
    File,
    Edit,
    Layer,
    Select,
    Filter,
    View,
    Window,
    Help,
    Primary,
}
impl ApplicationMenu {
    pub const ALL: [Self; 8] = [
        Self::File,
        Self::Edit,
        Self::Layer,
        Self::Select,
        Self::Filter,
        Self::View,
        Self::Window,
        Self::Help,
    ];
    pub fn canonical_label(self) -> std::sync::Arc<str> {
        self.localized_label(&Localizer::shared(UiLanguage::English))
    }
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(match self {
            Self::File => MessageId::MENU_FILE,
            Self::Edit => MessageId::MENU_EDIT,
            Self::Layer => MessageId::MENU_LAYER,
            Self::Select => MessageId::MENU_SELECT,
            Self::Filter => MessageId::MENU_FILTER,
            Self::View => MessageId::MENU_VIEW,
            Self::Window => MessageId::MENU_WINDOW,
            Self::Help => MessageId::MENU_HELP,
            Self::Primary => MessageId::MENU_MAIN_MENU,
        })
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn filter_category_items(&self) -> Vec<ContextMenuItem> {
        self.filter_category_items_for(None)
    }
    fn filter_category_items_for(&self, owner: Option<layer_core::authored::OccurrenceHandle>) -> Vec<ContextMenuItem> {
        let enabled = self.effect_insert_enabled();
        self.effect_catalog
            .categories()
            .iter()
            .filter_map(|category| {
                let items: Vec<_> = self.effect_catalog.filters()
                    .iter()
                    .filter(|f| f.category == category.id && (owner.is_none() || f.program.kind == layer_core::EffectKind::Adjustment))
                    .map(|f| ContextMenuItem { enabled, icon: Some(f.icon.clone()), ..ContextMenuItem::command(effects::resource_label(f.label(), self.localization()).to_string(), UiAction::Effect { action: owner.map_or_else(|| EffectAction::Insert { effect: f.program.id.clone() }, |owner| EffectAction::InsertAttached { effect: f.program.id.clone(), owner: occurrence_token(owner), epoch: self.state.document_file.epoch }) }) })
                    .collect();
                (!items.is_empty()).then(|| ContextMenuItem {
                    icon: Some(effects::category_icon(&category.id).into()),
                    ..ContextMenuItem::submenu(&effects::resource_label(&category.label, self.localization()), vec![items])
                })
            })
            .collect()
    }
    pub(crate) fn layer_filter_menu(&self, id: layer_core::authored::OccurrenceHandle) -> Option<ContextMenu> {
        let doc = self.engine.document(); let scene = doc.scene();
        let owner = scene.effect_owner(id).unwrap_or(id);
        (scene.eligible_target(owner) && !doc.is_locked(owner) && self.effect_insert_enabled()).then(|| ContextMenu {
            title: self.localization().text(MessageId::RESOURCES_LAYER_ADD_FILTER).to_string(),
            sections: vec![self.filter_category_items_for(Some(owner))],
        })
    }
    fn effect_insert_enabled(&self) -> bool {
        self.selection_masks.target().is_none() && self.require_document_idle().is_ok()
    }
    /// Layer › New entries for the bundled fill generators and the Dodge &
    /// Burn layer.
    pub(crate) fn fill_layer_items(&self) -> Vec<ContextMenuItem> {
        let enabled = self.effect_insert_enabled();
        let dodge_burn = self.command(CommandId::NewDodgeBurnLayer);
        [(MessageId::MENU_SOLID_COLOR_FILL, "solid_color"), (MessageId::MENU_GRADIENT_FILL, "gradient_fill")]
            .into_iter()
            .filter(|(_, id)| self.effect_catalog.get(id).is_some())
            .map(|(label, id)| ContextMenuItem {
                enabled,
                ..ContextMenuItem::command(self.localization().text(label).to_string(), UiAction::Effect { action: EffectAction::Insert { effect: id.into() } })
            })
            .chain([ContextMenuItem {
                enabled: dodge_burn.enabled,
                ..ContextMenuItem::command(dodge_burn.label.to_string(), UiAction::Invoke { command: dodge_burn.id })
            }])
            .collect()
    }
    pub(crate) fn proof_panel_command(&self, id: CommandId) -> bool {
        matches!(id, CommandId::SoftProofSetup | CommandId::GamutWarning | CommandId::PreviewSdr)
    }
    fn menu_command(&self, id: CommandId) -> ContextMenuItem {
        let state = self.command(id);
        let mut item = ContextMenuItem::command(state.label.to_string(), UiAction::Invoke { command: id });
        item.enabled = state.enabled;
        item.selected = id.is_toggle().then_some(state.selected);
        item
    }
    pub(crate) fn filter_menu_sections(&self) -> Vec<Vec<ContextMenuItem>> {
        vec![self.filter_category_items(), vec![self.menu_command(CommandId::FrequencySeparation)]]
    }
    pub fn application_menu(&self, menu: ApplicationMenu) -> ContextMenu {
        use ApplicationMenu as M;
        let command = |id: CommandId| self.menu_command(id);
        let mut model = match menu {
            M::Primary => ContextMenu {
                title: menu.localized_label(self.localization()).to_string(),
                sections: vec![
                    M::ALL
                        .into_iter()
                        .map(|id| {
                            ContextMenuItem::submenu(&id.localized_label(self.localization()), if id == M::Layer && self.selection_masks.quick() { self.quick_mask_menu().sections } else if id == M::Layer {
                                self.layer_menu_sections(self.engine.document().working.occurrence.map(super::occurrence_token).unwrap_or(0), self.engine.document().working.target.is_some_and(layer_core::SourceTarget::is_coverage), false).unwrap_or_default()
                            } else { self.application_menu(id).sections })
                        })
                        .collect(),
                ],
            },
            M::Layer if self.selection_masks.quick() => self.quick_mask_menu(),
            M::Edit => ContextMenu { title: menu.localized_label(self.localization()).to_string(), sections: vec![
                vec![command(CommandId::SearchCommands)],
                [CommandId::Undo, CommandId::Redo].map(command).into(),
                [CommandId::Cut, CommandId::Copy, CommandId::CopyMerged, CommandId::PasteImage, CommandId::PasteInPlace, CommandId::PasteInto]
                    .into_iter()
                    .filter(|id| id.available_on(self.state.platform))
                    .map(command)
                    .collect(),
                [CommandId::ApplyTransformPixels, CommandId::RasterizeSource, CommandId::RevertToOriginal, CommandId::FillSelection, CommandId::ClearSelected, CommandId::ClearOutside, CommandId::ClearLayer].map(command).into(),
                vec![command(CommandId::ScaleRotate), command(CommandId::TransformAgain)],
                vec![ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_IMAGE), vec![
                    [CommandId::Crop, CommandId::CropCanvasToSelection, CommandId::CanvasSize, CommandId::ImageSize].map(command).into(),
                    [CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180].map(command).into(),
                    [CommandId::FlipImageHorizontal, CommandId::FlipImageVertical].map(command).into(),
                    [CommandId::Trim, CommandId::RevealAll].map(command).into(),
                ])],
                [CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth].map(command).into_iter()
                    .chain([ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_BLENDING), vec![[CommandId::BlendPerceptual, CommandId::BlendLinear].map(command).into()])])
                    .collect(),
                vec![command(CommandId::Settings)],
            ] },
            M::Select => ContextMenu { title: menu.localized_label(self.localization()).to_string(), sections: vec![
                [CommandId::SelectAll, CommandId::Deselect, CommandId::Reselect, CommandId::InvertSelection].into_iter().map(command).collect(),
                [CommandId::QuickMask, CommandId::NewSelectionLayer, CommandId::SaveSelectionLayer].into_iter().map(command).collect(),
                [CommandId::CopySelectionToLayer, CommandId::CutSelectionToLayer, CommandId::ClearSelected, CommandId::ClearOutside].into_iter().map(command).collect(),
                [
                    CommandId::GrowSelection,
                    CommandId::ShrinkSelection,
                    CommandId::FeatherSelection,
                    CommandId::BorderSelection,
                    CommandId::SmoothSelection,
                    CommandId::TransformSelectionOutline,
                ]
                .into_iter()
                .map(command)
                .collect(),
                self.selection_source_menu_items(),
                vec![ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_LOAD_SELECTION), vec![self.saved_selection_menu_items()]), ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_REPLACE_SELECTION_LAYER_FROM_CURRENT_SELECTION),vec![self.replace_selection_menu_items()])],
                vec![command(CommandId::SelectionOutline)],
            ] },
            M::Layer => self
                .layer_menu_with(
                    self.engine.document().working.occurrence.map(super::occurrence_token).unwrap_or(0),
                    self.engine.document().working.target.is_some_and(layer_core::SourceTarget::is_coverage),
                    false,
                )
                .unwrap_or(ContextMenu {
                    title: menu.localized_label(self.localization()).to_string(),
                    sections: Vec::new(),
                }),
            M::Window => self.workspace_menu(),
            M::Filter => ContextMenu {
                title: menu.localized_label(self.localization()).to_string(),
                sections: self.filter_menu_sections(),
            },
            _ => {
                let sections: &[&[CommandId]] = match menu {
                    M::File => FILE_MENU.sections,
                    M::View => VIEW_MENU.sections,
                    M::Help => &[
                        &[CommandId::KeyboardShortcuts],
                        &[CommandId::Website, CommandId::SourceCode],
                        &[CommandId::About],
                    ],
                    _ => unreachable!(),
                };
                ContextMenu {
                    title: menu.localized_label(self.localization()).to_string(),
                    sections: sections
                        .iter()
                        .map(|section| {
                            section
                                .iter()
                                .copied()
                                .filter(|id| id.available_on(self.state.platform))
                                .filter(|id| !(menu == M::View && self.proof_panel_command(*id)))
                                .filter(|id| !(menu == M::View && self.state.platform == Platform::Windows
                                    && *id == CommandId::SdrRendition))
                                .filter(|id| !matches!(id, CommandId::SdrRendition | CommandId::PreviewSdr) || self.engine.document().composition().color.depth.is_float())
                                .filter(|id| {
                                    !(menu == M::View
                                        && *id == CommandId::ResetLayout
                                        && self.managed_workspace.is_some())
                                })
                                .map(command)
                                .collect()
                        })
                        .filter(|section: &Vec<_>| !section.is_empty())
                        .collect(),
                }
            }
        };
        if menu==M::View {model.sections.push(vec![command(CommandId::SelectionOutline)]);}
        model.title = menu.localized_label(self.localization()).to_string();
        model.with_shortcuts_localized(&self.state.settings, self.state.platform, self.localization())
    }

    pub fn zoom_menu(&self) -> ZoomMenu {
        let idle = self.require_idle().is_ok();
        let command = |id: CommandId| {
            let state = self.command(id);
            ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label.to_string(), UiAction::Invoke { command: id }) }
        };
        let level = |zoom: f32| ContextMenuItem {
            enabled: idle,
            ..ContextMenuItem::command({
                let mut args = localization::FluentArgs::new();
                args.set("percent", (zoom * 100.) as f64);
                self.localization().format(MessageId::MENU_ZOOM_LEVEL, &args)
            }, UiAction::SetZoom { zoom })
        };
        let lock = |label, selected, action| ContextMenuItem {
            selected: Some(selected), enabled: idle,
            ..ContextMenuItem::command(self.localization().text(label).to_string(), action)
        };
        let menu = ContextMenu {
            title: self.localization().text(MessageId::MENU_ZOOM).to_string(),
            sections: vec![
                [CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas, CommandId::ActualPixels].map(command).into(),
                ZOOM_LEVELS.map(level).into(),
                vec![lock(MessageId::MENU_LOCK_ZOOM, self.state.camera.zoom_locked,
                    UiAction::SetZoomLocked { locked: !self.state.camera.zoom_locked })],
                vec![ContextMenuItem { enabled: idle,
                    ..ContextMenuItem::command(self.localization().text(MessageId::MENU_RESET_ROTATION).to_string(),
                        UiAction::SetRotation { rotation: 0.0 }) },
                    lock(MessageId::MENU_LOCK_ROTATION, self.state.camera.rotation_locked,
                        UiAction::SetRotationLocked { locked: !self.state.camera.rotation_locked })],
            ],
        }
        .with_shortcuts_localized(&self.state.settings, self.state.platform, self.localization());
        ZoomMenu { menu, rotation_section: 3, buttons: NAVIGATOR_COMMANDS.into_iter().map(|id| self.command(id)).collect() }
    }
}

const ZOOM_LEVELS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];
