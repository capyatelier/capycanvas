//! Menu identity and contents are shared policy. Hosts only project these models.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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
            Self::Edit => EDIT_MENU.label,
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
            M::Filter => {
                // Ignore the panel's search/category selection; the menu always
                // exposes the whole live catalog and never requests thumbnails.
                let choices = effects::catalog(&self.effect_catalog, &Default::default());
                ContextMenu {
                    title: menu.label().into(),
                    sections: vec![
                        self.effect_catalog
                            .categories()
                            .iter()
                            .map(|category| {
                                let items = choices
                                    .iter()
                                    .filter(|f| f.category == category.id)
                                    .map(|f| {
                                        let mut item = ContextMenuItem::command(
                                            f.label.to_string(),
                                            f.action.clone(),
                                        );
                                        item.enabled = self.require_document_idle().is_ok();
                                        item
                                    })
                                    .collect();
                                ContextMenuItem::submenu(&category.label, vec![items])
                            })
                            .collect(),
                    ],
                }
            }
            _ => {
                let sections: &[&[CommandId]] = match menu {
                    M::File => FILE_MENU.sections,
                    M::Edit => EDIT_MENU.sections,
                    M::Select => &[
                        &[
                            CommandId::SelectAll,
                            CommandId::Deselect,
                            CommandId::InvertSelection,
                        ],
                        &[CommandId::Lasso, CommandId::AutoSelect],
                    ],
                    M::View => VIEW_MENU.sections,
                    M::Help => &[
                        &[CommandId::KeyboardShortcuts],
                        &[CommandId::Website, CommandId::SourceCode],
                        &[CommandId::About],
                    ],
                    M::Primary => PRIMARY_MENU,
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
                                .map(command)
                                .collect()
                        })
                        .filter(|section: &Vec<_>| !section.is_empty())
                        .collect(),
                }
            }
        };
        model.title = menu.label().into();
        model.with_shortcuts(&self.state.settings, self.state.platform)
    }
}
