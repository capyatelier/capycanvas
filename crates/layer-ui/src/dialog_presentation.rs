use crate::{Bounds, UiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogEscape { Back, ClearSearch, Cancel }
pub fn dialog_escape(subpage: bool, search_focused: bool, search_has_text: bool) -> DialogEscape {
    if subpage { DialogEscape::Back }
    else if search_focused && search_has_text { DialogEscape::ClearSearch }
    else { DialogEscape::Cancel }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogRole { Editor, Alert, CanvasPanel }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DialogPresentation {
    Floating,
    Page(Bounds),
    CanvasPanel(Bounds),
}
impl DialogPresentation {
    pub fn single_pane(self) -> bool { matches!(self, Self::Page(_)) }
    pub fn bounds(self, measured: impl FnOnce() -> [f32; 2]) -> Option<Bounds> {
        match self {
            Self::Floating => None,
            Self::Page(bounds) => Some(bounds),
            Self::CanvasPanel(area) => {
                let measured = measured();
                let width = area.width.min(measured[0].max(360.));
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
    fn escape_leaves_subpages_before_clearing_focused_search() {
        for focused in [false, true] { for text in [false, true] {
            assert_eq!(dialog_escape(true, focused, text), DialogEscape::Back);
            assert_eq!(dialog_escape(false, focused, text), if focused && text { DialogEscape::ClearSearch } else { DialogEscape::Cancel });
        } }
    }
    #[test]
    fn only_canvas_panels_measure_their_content() {
        let area = Bounds { x: 8., y: 12., width: 400., height: 600. };
        for (presentation, expected) in [(DialogPresentation::Floating, None),
            (DialogPresentation::Page(area), Some(area))] {
            assert_eq!(presentation.bounds(|| panic!("fixed page bounds never measure content")), expected);
        }
        let mut measurements = 0;
        assert!(DialogPresentation::CanvasPanel(area).bounds(|| {
            measurements += 1;
            [380., 200.]
        }).is_some());
        assert_eq!(measurements, 1);
    }
    #[test]
    fn only_editor_pages_require_one_navigation_pane() {
        for (presentation, single) in [(DialogPresentation::Floating, false),
            (DialogPresentation::Page(Bounds::default()), true),
            (DialogPresentation::CanvasPanel(Bounds::default()), false)] {
            assert_eq!(presentation.single_pane(), single);
        }
    }
    #[test]
    fn canvas_panels_keep_a_useful_width_inside_the_work_area() {
        for available in [396., 770.] {
            let area = Bounds { x: 8., y: 52., width: available, height: 300. };
            for measured in [216_f32, 360., 900.] {
                let bounds = DialogPresentation::CanvasPanel(area).bounds(|| [measured, 100.]).unwrap();
                assert_eq!(bounds.width, available.min(measured.max(360.)));
                assert_eq!(bounds.x, area.x + (available - bounds.width) / 2.);
                assert_eq!(bounds.y + bounds.height, area.y + area.height);
            }
        }
    }
    #[test]
    fn panes_use_their_measured_minimum_widths() {
        assert!(!dialog_panes_fit(399., [190., 200.], 10.));
        assert!(dialog_panes_fit(400., [190., 200.], 10.));
        assert!(dialog_panes_fit(891., [232., 320.], 24.));
        assert!(!dialog_panes_fit(412., [232., 320.], 24.));
        assert!(!dialog_panes_fit(412., [280., 0.], 12.));
    }
}
