use crate::localization::MessageId;

#[derive(Clone, Copy)]
pub(crate) enum Placement {
    Primary,
    Section(MessageId),
}

pub(crate) struct ControlPlacement {
    pub setting: &'static str,
    pub placement: Placement,
}

pub(crate) struct ControlProfile(pub &'static [ControlPlacement]);

impl ControlProfile {
    pub fn placement(&self, setting: &str) -> Option<(usize, Placement)> {
        self.0.iter().enumerate().find(|(_, control)| control.setting == setting)
            .map(|(index, control)| (index, control.placement))
    }

    pub fn primary(&self) -> impl Iterator<Item = &'static str> {
        self.0.iter().filter(|control| matches!(control.placement, Placement::Primary)).map(|control| control.setting)
    }
}

const fn primary(setting: &'static str) -> ControlPlacement {
    ControlPlacement { setting, placement: Placement::Primary }
}

const fn secondary(setting: &'static str) -> ControlPlacement {
    ControlPlacement { setting, placement: Placement::Section(MessageId::TOOL_SETTING_GROUP_BLENDER) }
}

pub(crate) const SOFT: ControlProfile = ControlProfile(&[
    primary("pull"), primary("spread"), secondary("streaks"), secondary("grain_depth"),
]);
pub(crate) const BLUR: ControlProfile = ControlProfile(&[primary("spread")]);
pub(crate) const GRAINY: ControlProfile = ControlProfile(&[
    primary("spread"), primary("grain_depth"), secondary("pull"),
]);
pub(crate) const BRISTLE: ControlProfile = ControlProfile(&[
    primary("pull"), primary("streaks"), primary("spread"), secondary("grain_depth"),
]);
pub(crate) const KNIFE: ControlProfile = ControlProfile(&[
    primary("pull"), primary("streaks"), secondary("grain_depth"),
]);
pub(crate) const TEXTURE: ControlProfile = ControlProfile(&[
    primary("texture_kept"), primary("detail_size"), primary("spread"), secondary("pull"),
]);
pub(crate) const HUE: ControlProfile = ControlProfile(&[
    primary("blend_lightness"), primary("spread"), secondary("pull"),
]);
pub(crate) const WET: ControlProfile = ControlProfile(&[
    primary("pull"), primary("dilution"), primary("average_pickup"),
    secondary("paint"), secondary("pressure_refill"), secondary("persistence"),
]);
pub(crate) const COLOR_MIXER: ControlProfile = ControlProfile(&[
    primary("pull"), primary("average_pickup"), primary("persistence"),
    secondary("paint"), secondary("dilution"), secondary("pressure_refill"),
]);
pub(crate) const PAINTERLY: ControlProfile = ControlProfile(&[
    primary("spread"), primary("stretch"), secondary("pull"),
]);
pub(crate) const COMB: ControlProfile = ControlProfile(&[
    primary("edge_keeping"), primary("spread"),
]);
pub(crate) const SCATTER: ControlProfile = ControlProfile(&[
    primary("spread"), primary("scatter_cell_size"), primary("grit"), secondary("pull"),
]);
pub(crate) const SHARPEN: ControlProfile = ControlProfile(&[
    primary("sharpness"), primary("spread"),
]);
pub(crate) const MARBLING: ControlProfile = ControlProfile(&[
    primary("swirl"), primary("swirl_size"), secondary("pull"),
]);
pub(crate) const CRYSTAL: ControlProfile = ControlProfile(&[
    primary("cell_size"), primary("stretch"), primary("spread"),
]);
pub(crate) const DITHER: ControlProfile = ControlProfile(&[
    primary("spread"), primary("dither_cell_size"),
]);
