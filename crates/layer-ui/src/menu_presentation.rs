use crate::{ContextTarget, PaletteMenuTarget};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuAnchor { Contact, Widget }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuPresentation {
    pub has_arrow: bool,
    pub anchor: MenuAnchor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuTarget {
    Customization { target: ContextTarget },
    LayerRow,
    Palette { target: PaletteMenuTarget },
    CanvasBar,
    WorkspaceRow,
    WorkspaceOptions,
    PreferenceReset,
    TextEdit,
    ZoomReadout,
    NewDrawingPreset,
    NewDrawingResolution,
    PaperTile,
    NewDrawingColor,
    Button,
}
impl MenuTarget {
    pub const fn presentation(self) -> MenuPresentation {
        use MenuAnchor::{Contact,Widget};
        match self {
            Self::Customization {..}|Self::WorkspaceRow|Self::PreferenceReset|Self::ZoomReadout=>MenuPresentation {has_arrow:true,anchor:Contact},
            Self::LayerRow|Self::WorkspaceOptions|Self::TextEdit=>MenuPresentation {has_arrow:false,anchor:Contact},
            Self::Palette {target:PaletteMenuTarget::Library}|Self::NewDrawingPreset|Self::NewDrawingResolution
                |Self::PaperTile|Self::NewDrawingColor=>MenuPresentation {has_arrow:false,anchor:Widget},
            Self::Palette {..}=>MenuPresentation {has_arrow:false,anchor:Contact},
            Self::CanvasBar|Self::Button=>MenuPresentation {has_arrow:true,anchor:Widget},
        }
    }
}
impl ContextTarget {
    pub const fn presentation(self) -> MenuPresentation { MenuTarget::Customization {target:self}.presentation() }
}
impl PaletteMenuTarget {
    pub const fn presentation(self) -> MenuPresentation { MenuTarget::Palette {target:self}.presentation() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_targets_preserve_native_arrows_and_anchor_owners() {
        let contact=MenuPresentation {has_arrow:true,anchor:MenuAnchor::Contact};
        for target in [ContextTarget::PhoneTile {tile:7},ContextTarget::PhoneToolbar,ContextTarget::ColorWheel] {
            assert_eq!(target.presentation(),contact);
        }
        for target in [MenuTarget::LayerRow,MenuTarget::WorkspaceOptions,MenuTarget::TextEdit,
            MenuTarget::Palette {target:PaletteMenuTarget::Color {id:1}},MenuTarget::Palette {target:PaletteMenuTarget::Palette {id:2}}] {
            assert_eq!(target.presentation(),MenuPresentation {has_arrow:false,anchor:MenuAnchor::Contact});
        }
        for target in [MenuTarget::Palette {target:PaletteMenuTarget::Library},MenuTarget::NewDrawingPreset,
            MenuTarget::NewDrawingResolution,MenuTarget::NewDrawingColor,MenuTarget::PaperTile] {
            assert_eq!(target.presentation(),MenuPresentation {has_arrow:false,anchor:MenuAnchor::Widget});
        }
        for target in [MenuTarget::Button,MenuTarget::CanvasBar] {
            assert_eq!(target.presentation(),MenuPresentation {has_arrow:true,anchor:MenuAnchor::Widget});
        }
        for target in [MenuTarget::WorkspaceRow,MenuTarget::PreferenceReset,MenuTarget::ZoomReadout] {assert_eq!(target.presentation(),contact);}
    }

    #[test]
    fn menu_presentation_roundtrips_target_metadata_without_action_state() {
        let target=MenuTarget::Customization {target:ContextTarget::PhoneTile {tile:7}};
        let wire=serde_json::json!({"kind":"customization","target":{"kind":"phone_tile","tile":7}});
        assert_eq!(serde_json::to_value(target).unwrap(),wire);
        assert_eq!(serde_json::from_value::<MenuTarget>(wire).unwrap(),target);
        assert_eq!(serde_json::to_value(target.presentation()).unwrap(),serde_json::json!({"has_arrow":true,"anchor":"contact"}));
    }
}
