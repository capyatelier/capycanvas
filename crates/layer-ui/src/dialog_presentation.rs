use crate::{Bounds, UiState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogEscape { Back, ClearSearch, Cancel }
pub fn dialog_escape(subpage: bool, search_focused: bool, search_has_text: bool) -> DialogEscape {
    if search_focused && search_has_text { DialogEscape::ClearSearch }
    else if subpage { DialogEscape::Back }
    else { DialogEscape::Cancel }
}

pub fn alert_responses_stacked(widths: &[f32], gap: f32, available: f32) -> bool {
    widths.iter().copied().fold(0., f32::max) * widths.len() as f32
        + gap * widths.len().saturating_sub(1) as f32 > available
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogRole { Editor, Alert, CanvasPanel }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogPrimary { Action, Close }
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogHeaderAction { Cancel, Back, Primary, Close }
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DialogHeader {
    pub start: Option<DialogHeaderAction>,
    pub end: Option<DialogHeaderAction>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DialogIntent {
    pub role: DialogRole,
    pub modal: bool,
    pub primary: Option<DialogPrimary>,
}
impl DialogIntent {
    pub const fn editor(primary: Option<DialogPrimary>) -> Self { Self { role: DialogRole::Editor, modal: true, primary } }
    pub const fn canvas_panel(modal: bool) -> Self { Self { role: DialogRole::CanvasPanel, modal, primary: Some(DialogPrimary::Action) } }
    pub const fn progress() -> Self { Self { role: DialogRole::Alert, modal: true, primary: None } }
    pub const fn alert() -> Self { Self { role: DialogRole::Alert, modal: true, primary: Some(DialogPrimary::Action) } }
    pub fn header(self, subpage: bool) -> DialogHeader {
        use DialogHeaderAction as A;
        DialogHeader {
            start: if subpage { Some(A::Back) } else if self.primary == Some(DialogPrimary::Close) { None } else { Some(A::Cancel) },
            end: self.primary.map(|primary| if primary == DialogPrimary::Close { A::Close } else { A::Primary }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "bounds", rename_all = "snake_case")]
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

pub fn calibration_extent(area: [f32; 2], natural: [f32; 2]) -> [f32; 2] {
    [natural[0].max(384.).min(area[0]), natural[1].max(360.).min(area[1])]
}

pub fn floating_dialog_width(window: f32, natural: f32) -> f32 {
    let padding = (30. + (window - 720.).max(0.) / 8.).floor();
    natural.max(0.).min((window - 2. * padding).max(0.))
}

pub fn dialog_panes_fit(width: f32, minimum: [f32; 2], gap: f32) -> bool {
    minimum.iter().all(|width| *width > 0.) && width >= minimum.iter().sum::<f32>() + gap
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct ColorEditorLayout { pub split: bool, pub compact_fields: bool }
pub fn color_editor_layout(width: f32, minimum: [f32; 2]) -> ColorEditorLayout {
    ColorEditorLayout { split: dialog_panes_fit(width, minimum, 24.), compact_fields: width < minimum[1] }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct NewDrawingLayout { pub single_pane: bool, pub preview: [i32; 2], pub fit: [i32; 2] }
pub fn new_drawing_layout(width: f32) -> NewDrawingLayout {
    let single_pane = !dialog_panes_fit(width, [232., 320.], 24.);
    NewDrawingLayout { single_pane, preview: [232, if single_pane { 140 } else { 250 }],
        fit: [196, if single_pane { 104 } else { 214 }] }
}

pub fn preferences_content_width(width: f32, minimum: f32) -> f32 {
    let lower = 400_f32.max(minimum + 24.);
    let upper = 600_f32.max(lower);
    let clamped = if width <= lower { width } else if upper == lower { lower } else {
        let t = ((width - lower) / (3. * (upper - lower))).clamp(0., 1.);
        lower + (upper - lower) * (1. - (1. - t).powi(3))
    };
    (clamped.floor() - 24.).max(0.)
}

impl UiState {
    pub fn dialog_presentation(&self, role: DialogRole, viewport: [f32; 2]) -> DialogPresentation {
        self.dialog_presentation_in(role, viewport, Bounds { x: 0., y: 0., width: viewport[0], height: viewport[1] })
    }
    pub fn dialog_presentation_in(&self, role: DialogRole, viewport: [f32; 2], visible: Bounds) -> DialogPresentation {
        let Some(phone) = self.phone_presentation(viewport) else { return DialogPresentation::Floating; };
        match role {
            DialogRole::Alert => DialogPresentation::Floating,
            DialogRole::CanvasPanel => DialogPresentation::CanvasPanel(phone.work_area),
            DialogRole::Editor => {
                let [left, top, right, bottom] = self.phone.insets;
                DialogPresentation::Page(Bounds { x: left, y: top,
                    width: (viewport[0] - left - right).max(0.), height: (viewport[1] - top - bottom).max(0.) }.intersection(visible).unwrap_or_default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_fields_wrap_only_after_the_stacked_form_cannot_fit_them() {
        for (width, split, compact_fields) in [(600.,true,false),(500.,false,false),(320.,false,false),(244.,false,true)] {
            assert_eq!(color_editor_layout(width,[232.,320.]),ColorEditorLayout {split,compact_fields});
        }
    }
    #[test]
    fn floating_editor_width_matches_native_sheet_allocation() {
        for (window, width) in [(600.,540.),(700.,640.),(800.,720.),(1024.,888.)] {
            assert_eq!(floating_dialog_width(window,1000.),width);
            assert_eq!(floating_dialog_width(window,440.),440.);
        }
        assert_eq!(floating_dialog_width(801.,1000.),721.);
        assert_eq!(floating_dialog_width(20.,440.),0.);
    }
    #[test]
    fn canvas_intents_preserve_native_modality_and_progress_is_a_cancel_only_alert() {
        let calibration=DialogIntent::canvas_panel(false);
        assert_eq!(calibration.role,DialogRole::CanvasPanel);
        assert!(!calibration.modal);
        assert!(DialogIntent::canvas_panel(true).modal);
        assert_eq!(DialogIntent::progress(),DialogIntent {role:DialogRole::Alert,modal:true,primary:None});
    }
    #[test]
    fn calibration_minimums_fit_the_measured_native_area() {
        assert_eq!(calibration_extent([412.,891.],[200.,220.]),[384.,360.]);
        assert_eq!(calibration_extent([280.,280.],[200.,220.]),[280.,280.]);
        assert_eq!(calibration_extent([800.,900.],[460.,550.]),[460.,550.]);
    }
    #[test]
    fn close_only_done_and_an_explicit_primary_have_distinct_headers() {
        use DialogHeaderAction as A;
        for (primary, start, end) in [(None, Some(A::Cancel), None),
            (Some(DialogPrimary::Action), Some(A::Cancel), Some(A::Primary)),
            (Some(DialogPrimary::Close), None, Some(A::Close))] {
            let intent = DialogIntent::editor(primary);
            assert_eq!(intent.header(false), DialogHeader { start, end });
            assert_eq!(intent.header(true), DialogHeader { start: Some(A::Back), end });
            assert!(intent.modal);
        }
    }
    #[test]
    fn escape_clears_focused_search_before_leaving_subpages() {
        for focused in [false, true] { for text in [false, true] {
            assert_eq!(dialog_escape(true, focused, text), if focused && text { DialogEscape::ClearSearch } else { DialogEscape::Back });
            assert_eq!(dialog_escape(false, focused, text), if focused && text { DialogEscape::ClearSearch } else { DialogEscape::Cancel });
        } }
    }
    #[test]
    fn alert_responses_match_captured_gtk_homogeneous_widths() {
        assert!(alert_responses_stacked(&[100., 144.], 12., 252.));
        assert!(alert_responses_stacked(&[100., 163., 100.], 12., 252.));
        assert!(!alert_responses_stacked(&[100., 100.], 12., 324.));
        assert!(!alert_responses_stacked(&[100., 100.], 12., 212.));
        assert!(alert_responses_stacked(&[100., 100.], 12., 211.));
        assert!(!alert_responses_stacked(&[100.], 12., 100.));
        assert!(!alert_responses_stacked(&[], 12., 0.));
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
        assert!(!dialog_panes_fit(390., [126., 265.], 0.));
        assert!(dialog_panes_fit(391., [126., 265.], 0.));
    }
    #[test]
    fn drawing_preview_changes_only_when_its_two_panes_stop_fitting() {
        for width in [280.,372.,575.] {
            assert_eq!(new_drawing_layout(width),NewDrawingLayout { single_pane:true,preview:[232,140],fit:[196,104] });
        }
        for width in [576.,851.,1000.] {
            assert_eq!(new_drawing_layout(width),NewDrawingLayout { single_pane:false,preview:[232,250],fit:[196,214] });
        }
    }
    #[test]
    fn preferences_content_width_matches_native_clamp_at_measured_widths() {
        for (available, expected) in [(350.,326.),(400.,376.),(450.,421.),(530.,479.),
            (678.,545.),(1000.,576.)] {
            assert_eq!(preferences_content_width(available,241.),expected,"available {available}");
        }
    }
}
