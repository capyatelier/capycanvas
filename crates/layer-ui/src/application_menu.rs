//! Menu identity and contents are shared policy. Hosts only project these models.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationLink {
    Website,
    SourceCode,
}
impl ApplicationLink {
    pub fn label(self) -> &'static str {
        match self {
            Self::Website => "Website",
            Self::SourceCode => "Source code",
        }
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
    pub fn label(self) -> &'static str {
        match self {
            Self::File => FILE_MENU.label,
            Self::Edit => "Edit",
            Self::Layer => "Layer",
            Self::Select => "Select",
            Self::Filter => "Filter",
            Self::View => VIEW_MENU.label,
            Self::Window => WORKSPACE_MENU_LABEL,
            Self::Help => "Help",
            Self::Primary => "Main Menu",
        }
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// One submenu per filter category, shared by the Filter menu and the
    /// selection bar's Adjust menu. It ignores the panel's search and category,
    /// leaves fill generators to Layer › New and never requests thumbnails.
    pub(crate) fn filter_category_items(&self) -> Vec<ContextMenuItem> {
        let choices = effects::catalog(&self.effect_catalog, &Default::default());
        let enabled = self.effect_insert_enabled();
        let generators: Vec<_> = self.effect_catalog.filters().iter()
            .filter(|d| d.program.kind == layer_core::EffectKind::Generator)
            .map(|d| d.program.id.clone())
            .collect();
        self.effect_catalog
            .categories()
            .iter()
            .filter_map(|category| {
                let items: Vec<_> = choices
                    .iter()
                    .filter(|f| f.category == category.id && !generators.contains(&f.id))
                    .map(|f| ContextMenuItem { enabled, ..ContextMenuItem::command(f.label.to_string(), f.action.clone()) })
                    .collect();
                (!items.is_empty()).then(|| ContextMenuItem::submenu(&category.label, vec![items]))
            })
            .collect()
    }
    fn effect_insert_enabled(&self) -> bool {
        self.selection_masks.target().is_none() && self.require_document_idle().is_ok()
    }
    /// Layer › New entries for the bundled fill generators.
    pub(crate) fn fill_layer_items(&self) -> Vec<ContextMenuItem> {
        let enabled = self.effect_insert_enabled();
        [("Solid Color Fill", "solid_color"), ("Gradient Fill", "gradient_fill")]
            .into_iter()
            .filter(|(_, id)| self.effect_catalog.get(id).is_some())
            .map(|(label, id)| ContextMenuItem {
                enabled,
                ..ContextMenuItem::command(label, UiAction::Effect { action: EffectAction::Insert { effect: id.into() } })
            })
            .collect()
    }
    pub(crate) fn proof_panel_command(&self, id: CommandId) -> bool {
        matches!(id, CommandId::SoftProofSetup | CommandId::GamutWarning | CommandId::PreviewSdr)
    }
    pub fn application_menu(&self, menu: ApplicationMenu) -> ContextMenu {
        use ApplicationMenu as M;
        let command = |id: CommandId| {
            let state = self.command(id);
            let mut item = ContextMenuItem::command(state.label, UiAction::Invoke { command: id });
            item.enabled = state.enabled;
            item.selected = id.is_toggle().then_some(state.selected);
            item
        };
        let mut model = match menu {
            M::Primary => ContextMenu {
                title: menu.label().into(),
                sections: vec![
                    M::ALL
                        .into_iter()
                        .map(|id| {
                            ContextMenuItem::submenu(id.label(), self.application_menu(id).sections)
                        })
                        .collect(),
                ],
            },
            M::Layer if self.selection_masks.quick() => self.quick_mask_menu(),
            M::Edit => ContextMenu { title: menu.label().into(), sections: vec![
                vec![command(CommandId::SearchCommands)],
                [CommandId::Undo, CommandId::Redo].map(command).into(),
                [CommandId::Cut, CommandId::Copy, CommandId::CopyMerged, CommandId::PasteImage, CommandId::PasteInPlace, CommandId::PasteInto]
                    .into_iter()
                    .filter(|id| id.available_on(self.state.platform))
                    .map(command)
                    .collect(),
                [CommandId::RasterizeSource, CommandId::RevertToOriginal, CommandId::FillSelection, CommandId::ClearSelected, CommandId::ClearOutside, CommandId::ClearLayer].map(command).into(),
                vec![command(CommandId::ScaleRotate)],
                vec![ContextMenuItem::submenu("Image", vec![
                    [CommandId::Crop, CommandId::CropCanvasToSelection, CommandId::CanvasSize, CommandId::ImageSize].map(command).into(),
                    [CommandId::RotateImageLeft, CommandId::RotateImageRight, CommandId::RotateImage180].map(command).into(),
                    [CommandId::FlipImageHorizontal, CommandId::FlipImageVertical].map(command).into(),
                    [CommandId::Trim, CommandId::RevealAll].map(command).into(),
                ])],
                [CommandId::AssignProfile, CommandId::ConvertColorSpace, CommandId::ChangeBitDepth].map(command).into_iter()
                    .chain([ContextMenuItem::submenu("Blending", vec![[CommandId::BlendPerceptual, CommandId::BlendLinear].map(command).into()])])
                    .collect(),
                vec![command(CommandId::Settings)],
            ] },
            M::Select => ContextMenu { title: menu.label().into(), sections: vec![
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
                vec![ContextMenuItem::submenu("Load Selection", vec![self.saved_selection_menu_items()]), ContextMenuItem::submenu("Replace Selection Layer from Current Selection",vec![self.replace_selection_menu_items()])],
                vec![command(CommandId::SelectionOutline)],
            ] },
            M::Layer => self
                .layer_menu(
                    self.engine.document().active_layer.0,
                    self.engine.document().active_mask,
                )
                .unwrap_or(ContextMenu {
                    title: menu.label().into(),
                    sections: Vec::new(),
                }),
            M::Window => self.workspace_menu(),
            M::Filter => ContextMenu {
                title: menu.label().into(),
                sections: vec![self.filter_category_items()],
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
                    title: menu.label().into(),
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
                                .filter(|id| !matches!(id, CommandId::SdrRendition | CommandId::PreviewSdr) || self.engine.document().color.depth.is_float())
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
        model.title = menu.label().into();
        model.with_shortcuts(&self.state.settings, self.state.platform)
    }

    /// The zoom readout's menu: the view commands, then fixed percentages.
    pub fn zoom_menu(&self) -> ContextMenu {
        let idle = self.require_idle().is_ok();
        let command = |id: CommandId| {
            let state = self.command(id);
            ContextMenuItem { enabled: state.enabled, ..ContextMenuItem::command(state.label, UiAction::Invoke { command: id }) }
        };
        let level = |zoom: f32| ContextMenuItem {
            enabled: idle,
            ..ContextMenuItem::command(format!("{}%", zoom * 100.), UiAction::SetZoom { zoom })
        };
        ContextMenu {
            title: "Zoom".into(),
            sections: vec![
                [CommandId::ZoomIn, CommandId::ZoomOut, CommandId::FitCanvas, CommandId::ActualPixels].map(command).into(),
                ZOOM_LEVELS.map(level).into(),
            ],
        }
        .with_shortcuts(&self.state.settings, self.state.platform)
    }
}

const ZOOM_LEVELS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];
