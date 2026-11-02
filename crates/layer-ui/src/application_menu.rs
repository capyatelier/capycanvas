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
    Image,
    Layer,
    Select,
    Filter,
    View,
    Window,
    Primary,
}
impl ApplicationMenu {
    pub const ALL: [Self; 8] = [
        Self::File,
        Self::Edit,
        Self::Image,
        Self::Layer,
        Self::Select,
        Self::Filter,
        Self::View,
        Self::Window,
    ];
    pub fn switches_on_hover(open: Option<Self>, hovered: Self) -> bool {
        open.is_some_and(|open| open != hovered && Self::ALL.contains(&open))
            && Self::ALL.contains(&hovered)
    }
    pub fn canonical_label(self) -> std::sync::Arc<str> {
        self.localized_label(&Localizer::shared(UiLanguage::English))
    }
    pub fn localized_label(self, localization: &Localizer) -> std::sync::Arc<str> {
        localization.text(match self {
            Self::File => MessageId::MENU_FILE,
            Self::Edit => MessageId::MENU_EDIT,
            Self::Image => MessageId::MENU_IMAGE,
            Self::Layer => MessageId::MENU_LAYER,
            Self::Select => MessageId::MENU_SELECT,
            Self::Filter => MessageId::MENU_FILTER,
            Self::View => MessageId::MENU_VIEW,
            Self::Window => MessageId::MENU_WINDOW,
            Self::Primary => MessageId::MENU_MAIN_MENU,
        })
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub(crate) fn filter_category_items(&self) -> Vec<ContextMenuItem> {
        self.filter_category_items_for(None, true)
    }
    /// Adjustments attach to the editing layer; fills and textures become layers of their own.
    pub(crate) fn selection_filter_items(&self) -> Vec<ContextMenuItem> {
        let doc = self.engine.document();
        let scene = doc.scene();
        let owner = doc.working.occurrence.map(|id| scene.effect_owner(id).unwrap_or(id)).filter(|&id| scene.eligible_target(id) && !doc.is_locked(id));
        self.filter_category_items_for(owner, true)
    }
    fn filter_category_items_for(&self, owner: Option<layer_core::authored::OccurrenceHandle>, standalone: bool) -> Vec<ContextMenuItem> {
        let enabled = self.effect_insert_enabled();
        self.effect_catalog
            .categories()
            .iter()
            .filter_map(|category| {
                let items: Vec<_> = self.effect_catalog.filters()
                    .iter()
                    .filter(|f| f.category == category.id && (standalone || f.program.kind == layer_core::EffectKind::Adjustment))
                    .map(|f| ContextMenuItem { enabled, icon: Some(f.icon.clone()), ..ContextMenuItem::command(effects::resource_label(f.label(), self.localization()).to_string(), UiAction::Effect { action: owner.filter(|_| f.program.kind == layer_core::EffectKind::Adjustment).map_or_else(|| EffectAction::Insert { effect: f.program.id.clone() }, |owner| EffectAction::InsertAttached { effect: f.program.id.clone(), owner: occurrence_token(owner), epoch: self.state.document_file.epoch }) }) })
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
            sections: vec![self.filter_category_items_for(Some(owner), false)],
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
        [(MessageId::MENU_SOLID_COLOR_FILL, "solid_color"), (MessageId::MENU_GRADIENT_FILL, "gradient_fill"), (MessageId::RESOURCES_FILTER_PAPER, "paper")]
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
                                self.application_layer_sections()
                            } else { self.application_menu(id).sections })
                        })
                        .collect(),
                    vec![command(CommandId::Settings)],
                ],
            },
            M::Layer if self.selection_masks.quick() => self.quick_mask_menu(),
            M::Edit => ContextMenu { title: menu.localized_label(self.localization()).to_string(), sections: vec![
                [CommandId::Undo, CommandId::Redo].map(command).into(),
                [CommandId::Cut, CommandId::Copy, CommandId::CopyMerged, CommandId::CopyPixels, CommandId::PasteImage]
                    .map(command).into_iter().chain([ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_PASTE_SPECIAL), vec![
                        [CommandId::PasteInPlace, CommandId::PasteAtView, CommandId::PasteAtCursor].map(command).into(),
                        [CommandId::PasteInto, CommandId::PasteAsNewImage].map(command).into(),
                    ])]).collect(),
                [CommandId::FillSelection, CommandId::ClearSelected, CommandId::ClearOutside].map(command).into(),
                vec![command(CommandId::ScaleRotate), command(CommandId::TransformAgain), ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_PATH), vec![
                    [CommandId::JoinPaths, CommandId::ClosePath, CommandId::SplitPath, CommandId::ReversePath].map(command).into(),
                    [CommandId::MakeCompoundPath, CommandId::ReleaseCompoundPath].map(command).into(),
                ]), ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_ARRANGE), vec![
                    [CommandId::BringToFront, CommandId::BringForward, CommandId::SendBackward, CommandId::SendToBack].map(command).into(),
                ])],
                vec![command(CommandId::SearchCommands), command(CommandId::Settings)],
            ] },
            M::Image => ContextMenu { title: menu.localized_label(self.localization()).to_string(), sections: vec![
                [CommandId::ImageSize, CommandId::CanvasSize].map(command).into(),
                [CommandId::Crop, CommandId::CropCanvasToSelection, CommandId::Trim, CommandId::RevealAll].map(command).into(),
                vec![ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_ROTATE_AND_FLIP), vec![
                    [CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180].map(command).into(),
                    [CommandId::FlipImageHorizontal, CommandId::FlipImageVertical].map(command).into(),
                ])],
                vec![ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_COLOR_MANAGEMENT), vec![
                    [CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth].map(command).into(),
                ]), ContextMenuItem::submenu(&self.localization().text(MessageId::MENU_BLENDING), vec![
                    [CommandId::BlendPerceptual, CommandId::BlendLinear].map(command).into(),
                ])],
            ] },
            M::Select => ContextMenu { title: menu.localized_label(self.localization()).to_string(), sections: vec![
                [CommandId::SelectAll, CommandId::Deselect, CommandId::Reselect, CommandId::InvertSelection].into_iter().map(command).collect(),
                [CommandId::QuickMask, CommandId::NewSelectionLayer, CommandId::SaveSelectionLayer].into_iter().map(command).collect(),
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
            M::Layer => ContextMenu { title: String::new(), sections: self.application_layer_sections() },
            M::Window => self.workspace_menu(),
            M::Filter => ContextMenu {
                title: menu.localized_label(self.localization()).to_string(),
                sections: self.filter_menu_sections(),
            },
            _ => {
                let sections: &[&[CommandId]] = match menu {
                    M::File => FILE_MENU.sections,
                    M::View => VIEW_MENU.sections,
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
        if menu == M::Primary && self.phone_layout().is_some() {
            model.sections.pop();
            let mut swap = ContextMenuItem::command(self.localization().text(MessageId::WORKSPACE_PHONE_SWAP_SIDES).to_string(), UiAction::Phone { action: PhoneAction::SwapSides });
            swap.selected = Some(self.state.settings.phone_swap_sides);
            model.sections.insert(0, vec![command(CommandId::SearchCommands)]);
            model.sections.insert(1, vec![command(CommandId::ZenMode), swap]);
        }
        model.title = menu.localized_label(self.localization()).to_string();
        model.with_shortcuts_localized(&self.state.settings, self.state.platform, self.localization())
    }

    pub fn zoom_menu(&self) -> ZoomMenu {
        let idle = self.navigation_idle();
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
                vec![command(CommandId::ResetRotation),
                    lock(MessageId::MENU_SNAP_ROTATION, self.state.settings.snap_rotation,
                        UiAction::SetRotationSnapping { enabled: !self.state.settings.snap_rotation }),
                    lock(MessageId::MENU_LOCK_ROTATION, self.state.camera.rotation_locked,
                        UiAction::SetRotationLocked { locked: !self.state.camera.rotation_locked })],
            ],
        }
        .with_shortcuts_localized(&self.state.settings, self.state.platform, self.localization());
        ZoomMenu { menu, rotation_section: 3, buttons: NAVIGATOR_COMMANDS.into_iter().map(|id| self.command(id)).collect() }
    }
}

const ZOOM_LEVELS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::session;

    #[test]
    fn compact_menu_ends_with_preferences_and_preserves_edit_access_on_every_platform() {
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Ios, Platform::Mac, Platform::Windows] {
            let mut session = session(platform);
            let primary = session.application_menu(ApplicationMenu::Primary);
            assert_eq!(primary.sections.len(), 2, "{platform:?}");
            assert_eq!(primary.sections[0].iter().map(|item| item.label.as_str()).collect::<Vec<_>>(),
                ApplicationMenu::ALL.map(|menu| menu.localized_label(session.localization()).to_string()));
            assert_eq!(primary.sections[1].len(), 1, "{platform:?}");
            let preferences = &primary.sections[1][0];
            assert_eq!(preferences.action, Some(UiAction::Invoke { command: CommandId::Settings }));
            let edit = session.application_menu(ApplicationMenu::Edit);
            assert_eq!(edit.sections.last().unwrap().last(), Some(preferences), "{platform:?}");
            assert!(!session.state().settings_open);
            session.dispatch(preferences.action.clone().unwrap()).unwrap();
            assert!(session.state().settings_open, "{platform:?}");
        }
    }

    #[test]
    fn edit_and_image_menus_preserve_live_commands_on_every_platform() {
        fn check<R: CanvasRenderer>(session: &UiSession<R>, sections: &[Vec<ContextMenuItem>], commands: &mut Vec<CommandId>) {
            for item in sections.iter().flatten() {
                if let Some(UiAction::Invoke { command }) = item.action {
                    assert!(!commands.contains(&command), "duplicate {command:?}");
                    commands.push(command);
                    let state = session.command(command);
                    assert_eq!(item.label, state.label.as_ref());
                    assert_eq!(item.enabled, state.enabled, "{command:?}");
                    let keys = session.state.settings.action_keys(&UiAction::Invoke { command }, session.state.platform);
                    let bindings: Vec<_> = keys.iter().map(|key| key.native_binding(session.state.platform)).collect();
                    assert_eq!(item.bindings, bindings, "{command:?}");
                }
                check(session, &item.sections, commands);
            }
        }
        for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Ios, Platform::Mac, Platform::Windows] {
            let session = session(platform);
            let edit = session.application_menu(ApplicationMenu::Edit);
            assert_eq!(edit.sections.iter().map(Vec::len).sum::<usize>(), 17);
            assert_eq!(edit.sections[0].iter().map(|item| item.action.clone()).collect::<Vec<_>>(),
                [CommandId::Undo, CommandId::Redo].map(|command| Some(UiAction::Invoke { command })));
            let mut commands = Vec::new();
            check(&session, &edit.sections, &mut commands);
            for command in [CommandId::ClearLayer, CommandId::RasterizeSource, CommandId::DiscardPaintEdits, CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth] {
                assert!(!commands.contains(&command), "{platform:?}: {command:?} belongs outside Edit");
            }
            check(&session, &session.application_menu(ApplicationMenu::Image).sections, &mut commands);
            assert_eq!(commands.len(), 45, "{platform:?}: all remaining Edit/Image commands survive exactly once");
        }
    }

    #[test]
    fn application_menu_hover_tracks_only_open_menu_bar_neighbors() {
        let menus = ApplicationMenu::ALL.into_iter().chain([ApplicationMenu::Primary]);
        for hovered in menus.clone() {
            assert!(!ApplicationMenu::switches_on_hover(None, hovered));
            for open in menus.clone() {
                assert_eq!(ApplicationMenu::switches_on_hover(Some(open), hovered),
                    open != hovered && open != ApplicationMenu::Primary && hovered != ApplicationMenu::Primary);
            }
        }
    }
}
