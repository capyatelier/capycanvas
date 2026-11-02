use crate::{Bounds, UiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogRole { Editor, Alert, CanvasPanel }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DialogPresentation {
    Floating,
    Page(Bounds),
    CanvasPanel(Bounds),
}
impl DialogPresentation {
    pub fn bounds(self, measured: [f32; 2]) -> Option<Bounds> {
        match self {
            Self::Floating => None,
            Self::Page(bounds) => Some(bounds),
            Self::CanvasPanel(area) => {
                let width = measured[0].min(area.width).max(0.);
                let height = measured[1].min(area.height).max(0.);
                Some(Bounds { x: area.x + (area.width - width) * 0.5, y: area.y + area.height - height, width, height })
            }
        }
    }
}

pub fn dialog_panes_fit(width: f32, minimum: [f32; 2], gap: f32) -> bool {
    minimum.iter().all(|width| *width > 0.) && width >= minimum.iter().sum::<f32>() + gap
}

impl UiState {
    pub fn dialog_presentation(&self, role: DialogRole, viewport: [f32; 2]) -> DialogPresentation {
        let Some(phone) = self.phone_presentation(viewport) else { return DialogPresentation::Floating; };
        match role {
            DialogRole::Alert => DialogPresentation::Floating,
            DialogRole::CanvasPanel => DialogPresentation::CanvasPanel(phone.work_area),
            DialogRole::Editor => {
                let [left, top, right, bottom] = self.phone.insets;
                DialogPresentation::Page(Bounds { x: left, y: top,
                    width: (viewport[0] - left - right).max(0.), height: (viewport[1] - top - bottom).max(0.) })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panes_use_their_measured_minimum_widths() {
        assert!(!dialog_panes_fit(399., [190., 200.], 10.));
        assert!(dialog_panes_fit(400., [190., 200.], 10.));
        assert!(dialog_panes_fit(891., [232., 320.], 24.));
        assert!(!dialog_panes_fit(412., [232., 320.], 24.));
        assert!(!dialog_panes_fit(412., [280., 0.], 12.));
    }
}
