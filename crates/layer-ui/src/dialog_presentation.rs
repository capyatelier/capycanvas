use crate::{Bounds, UiState};

pub fn dialog_ui(source: &str) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    enum Request {
        Header { intent: DialogIntent, subpage: bool },
        Escape { subpage: bool, search_focused: bool, search_has_text: bool },
        AlertShort { height: f32 },
        AlertLayout { viewport: [f32; 2], metrics: AlertMetrics, font_pixels: f32, prefer_wide: bool },
        FloatingWidth { window: f32, natural: f32 },
        CalibrationExtent { area: [f32; 2], natural: [f32; 2] },
        PanesFit { width: f32, minimum: [f32; 2], gap: f32 },
        DocumentColorSuggestedDepth { depth: layer_core::color::SampleDepth },
        DocumentColorDither { depth: layer_core::color::SampleDepth, active: bool },
        PreferencesContentWidth { width: f32, minimum: f32 },
        NewDrawingLayout { width: f32 },
        ColorEditorLayout { width: f32, minimum: [f32; 2] },
        NoticeAbove { notice: crate::NoticePlacement, bar: Option<Bounds>, bubble: Option<Bounds> },
        FingerLoupeOffset { #[serde(default)] display: Option<[f32; 2]>, scale: f32 },
        PhoneWindow { viewport: [f32; 2] },
        MenuPresentation { target: crate::MenuTarget },
    }
    use serde_json::json;
    Ok(match serde_json::from_str(source).map_err(|error| error.to_string())? {
        Request::Header { intent, subpage } => json!(intent.header(subpage)),
        Request::Escape { subpage, search_focused, search_has_text } => json!(dialog_escape(subpage, search_focused, search_has_text)),
        Request::AlertShort { height } => json!(alert_dialog_short(height)),
        Request::AlertLayout { viewport, metrics, font_pixels, prefer_wide } => json!(alert_dialog_layout(viewport, &metrics, font_pixels / (crate::UI_TEXT_PT as f32 * 4. / 3.), prefer_wide)),
        Request::FloatingWidth { window, natural } => json!(floating_dialog_width(window, natural)),
        Request::CalibrationExtent { area, natural } => json!(calibration_extent(area, natural)),
        Request::PanesFit { width, minimum, gap } => json!(dialog_panes_fit(width, minimum, gap)),
        Request::DocumentColorSuggestedDepth { depth } => json!(crate::document_color_suggested_depth(depth)),
        Request::DocumentColorDither { depth, active } => json!(crate::document_color_dither(depth, active)),
        Request::PreferencesContentWidth { width, minimum } => json!(preferences_content_width(width, minimum)),
        Request::NewDrawingLayout { width } => json!(new_drawing_layout(width)),
        Request::ColorEditorLayout { width, minimum } => json!(color_editor_layout(width, minimum)),
        Request::NoticeAbove { notice, bar, bubble } => json!(notice.above(bar).above_notice(bubble)),
        Request::FingerLoupeOffset { display, scale } => json!(crate::finger_loupe_offset(display, scale)),
        Request::PhoneWindow { viewport } => json!(crate::phone_window(viewport)),
        Request::MenuPresentation { target } => json!(target.presentation()),
    })
}

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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DialogFrame { Editor, Alert { prefer_wide: bool } }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DialogIntent {
    pub frame: DialogFrame,
    pub role: DialogRole,
    pub modal: bool,
    pub primary: Option<DialogPrimary>,
}
impl DialogIntent {
    pub const fn editor(primary: Option<DialogPrimary>) -> Self { Self { frame: DialogFrame::Editor, role: DialogRole::Editor, modal: true, primary } }
    pub const fn alert_editor(primary: Option<DialogPrimary>, prefer_wide: bool) -> Self { Self { frame: DialogFrame::Alert { prefer_wide }, ..Self::editor(primary) } }
    pub const fn canvas_panel(modal: bool) -> Self { Self { frame: DialogFrame::Editor, role: DialogRole::CanvasPanel, modal, primary: Some(DialogPrimary::Action) } }
    pub const fn progress() -> Self { Self { frame: DialogFrame::Alert { prefer_wide: false }, role: DialogRole::Alert, modal: true, primary: None } }
    pub const fn alert() -> Self { Self { frame: DialogFrame::Alert { prefer_wide: false }, role: DialogRole::Alert, modal: true, primary: Some(DialogPrimary::Action) } }
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

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct AlertMetrics {
    pub minimum: f32,
    pub natural: f32,
    pub heading: f32,
    pub responses: Vec<f32>,
}
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct AlertLayout { pub width: f32, pub short: bool, pub stacked: bool }
pub fn alert_dialog_short(height: f32) -> bool { height <= 360. }

pub fn alert_dialog_layout(viewport: [f32; 2], metrics: &AlertMetrics, text_scale: f32, prefer_wide: bool) -> AlertLayout {
    let short = alert_dialog_short(viewport[1]);
    let padding = if short { 36. } else { 48. };
    let minimum = metrics.minimum.max(300.);
    let preferred = (300. * text_scale).floor();
    let regular_maximum = (372. * text_scale).floor();
    let wide_maximum = (600. * text_scale).floor();
    let button = metrics.responses.iter().copied().fold(0., f32::max);
    let wide = button * metrics.responses.len() as f32 + 12. * metrics.responses.len().saturating_sub(1) as f32 + padding;
    let maximum = if metrics.responses.len() > 1 && wide > minimum {
        if prefer_wide || short {
            if wide > wide_maximum { if short { wide } else { regular_maximum } } else { wide }
        } else {
            let narrow = (button + padding).max(preferred);
            let heading = if metrics.heading > wide { wide } else { metrics.heading.max(narrow) };
            heading.min(regular_maximum)
        }
    } else { regular_maximum }.max(minimum);
    let natural = metrics.natural.clamp(minimum, maximum);
    let width = floating_dialog_width(viewport[0], natural).max(minimum);
    AlertLayout { width, short, stacked: !short && alert_responses_stacked(&metrics.responses, 12., width - padding) }
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
            DialogRole::CanvasPanel => DialogPresentation::CanvasPanel(phone.work_area.intersection(visible).unwrap_or(phone.work_area)),
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
    fn dialog_ui_projects_every_stateless_request() {
        use serde_json::json;
        let notice = crate::NoticePlacement::within(Bounds { x:0.,y:0.,width:300.,height:500. });
        for (request, expected) in [
            (json!({"kind":"header","intent":DialogIntent::editor(Some(DialogPrimary::Close)),"subpage":false}),json!({"start":null,"end":"close"})),
            (json!({"kind":"escape","subpage":true,"search_focused":true,"search_has_text":true}),json!("clear_search")),
            (json!({"kind":"alert_short","height":320.}),json!(true)),
            (json!({"kind":"alert_layout","viewport":[412.,891.],"metrics":{"minimum":48.,"natural":800.,"heading":200.,"responses":[100.,100.]},"font_pixels":crate::UI_TEXT_PT as f32*4./3.,"prefer_wide":false}),json!({"width":352.,"short":false,"stacked":false})),
            (json!({"kind":"floating_width","window":1024.,"natural":1000.}),json!(888.)),
            (json!({"kind":"calibration_extent","area":[280.,280.],"natural":[100.,100.]}),json!([280.,280.])),
            (json!({"kind":"panes_fit","width":500.,"minimum":[200.,320.],"gap":8.}),json!(false)),
            (json!({"kind":"document_color_suggested_depth","depth":"U16"}),json!("U8")),
            (json!({"kind":"document_color_dither","depth":"U8","active":true}),json!({"available":true,"value":"Stochastic8"})),
            (json!({"kind":"preferences_content_width","width":280.,"minimum":200.}),json!(256.)),
            (json!({"kind":"new_drawing_layout","width":280.}),json!({"single_pane":true,"preview":[232,140],"fit":[196,104]})),
            (json!({"kind":"color_editor_layout","width":244.,"minimum":[232.,320.]}),json!({"split":false,"compact_fields":true})),
            (json!({"kind":"notice_above","notice":notice,"bar":{"x":0.,"y":450.,"width":300.,"height":40.}}),json!(crate::NoticePlacement { y:438.,..notice })),
            (json!({"kind":"notice_above","notice":notice,"bar":{"x":0.,"y":450.,"width":300.,"height":40.},"bubble":{"x":20.,"y":300.,"width":260.,"height":80.}}),json!(crate::NoticePlacement { y:288.,..notice })),
            (json!({"kind":"finger_loupe_offset","scale":2.}),json!(88.)),
            (json!({"kind":"finger_loupe_offset","display":[800.,127.],"scale":1.}),json!(crate::finger_loupe_offset(Some([800.,127.]),1.))),
            (json!({"kind":"finger_loupe_offset","display":[800.,127.],"scale":2.}),json!(crate::finger_loupe_offset(Some([800.,127.]),2.))),
            (json!({"kind":"finger_loupe_offset","display":[800.,50.],"scale":1.}),json!(64.)),
            (json!({"kind":"finger_loupe_offset","display":[800.,500.],"scale":1.}),json!(36.)),
            (json!({"kind":"phone_window","viewport":[479.,800.]}),json!(true)),
            (json!({"kind":"menu_presentation","target":{"kind":"layer_row"}}),json!({"has_arrow":false,"anchor":"contact"})),
        ] {
            assert_eq!(dialog_ui(&request.to_string()).unwrap(),expected,"{request}");
        }
        for viewport in [[480.,800.],[800.,480.],[0.,300.],[-1.,300.],[f32::NAN,300.],[f32::INFINITY,300.]] { assert!(!crate::phone_window(viewport)); }
        assert!(dialog_ui("{\"kind\":\"unknown\"}").is_err());
    }
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
    fn native_alert_sizing_preserves_response_caps_and_the_absolute_minimum() {
        let mut metrics = AlertMetrics { minimum: 48., natural: 800., heading: 200., responses: vec![100.,144.,100.] };
        assert_eq!(alert_dialog_layout([891.,412.], &metrics, 1., false), AlertLayout { width:300., short:false, stacked:true });
        metrics.responses = vec![100.,100.];
        for (viewport, width) in [([891.,412.],372.),([412.,891.],352.),([280.,560.],300.),([320.,320.],300.)] {
            assert_eq!(alert_dialog_layout(viewport,&metrics,1.,false).width,width);
        }
        metrics.responses = vec![140.,140.];
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,1.,false).width,300.);
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,1.,true).width,340.);
        assert_eq!(alert_dialog_layout([891.,360.],&metrics,1.,false),AlertLayout {width:328.,short:true,stacked:false});
        assert!(!alert_dialog_layout([891.,361.],&metrics,1.,false).short);
        metrics.responses = vec![320.,320.];
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,1.,true).width,372.);
        assert_eq!(alert_dialog_layout([891.,360.],&metrics,1.,false).width,688.);
        metrics.minimum = 400.;metrics.responses.clear();
        assert_eq!(alert_dialog_layout([280.,560.],&metrics,1.,false).width,400.);
        metrics.minimum = 48.;metrics.natural = 48.;
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,1.,false).width,300.);
    }
    #[test]
    fn alert_sp_limits_scale_while_the_pixel_minimum_and_content_natural_remain_authoritative() {
        let mut metrics = AlertMetrics { minimum:48., natural:800., heading:200., responses:vec![100.,100.] };
        let scale = 16. / 11.;
        assert_eq!(alert_dialog_layout([320.,320.],&metrics,scale,false),AlertLayout {width:300.,short:true,stacked:false});
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,scale,false).width,541.);
        metrics.responses = vec![140.,140.];
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,scale,false).width,436.);
        metrics.heading = 350.;
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,scale,false).width,340.);
        metrics.heading = 200.;metrics.responses = vec![320.,320.];metrics.natural = 310.;
        assert_eq!(alert_dialog_layout([891.,412.],&metrics,1.,false).width,310.);
    }
    #[test]
    fn canvas_intents_preserve_native_modality_and_progress_is_a_cancel_only_alert() {
        let calibration=DialogIntent::canvas_panel(false);
        assert_eq!(calibration.role,DialogRole::CanvasPanel);
        assert!(!calibration.modal);
        assert!(DialogIntent::canvas_panel(true).modal);
        assert_eq!(DialogIntent::progress(),DialogIntent {frame:DialogFrame::Alert {prefer_wide:false},role:DialogRole::Alert,modal:true,primary:None});
    }
    #[test]
    fn editor_alert_frames_preserve_editor_routing_and_header_actions() {
        for primary in [None,Some(DialogPrimary::Action),Some(DialogPrimary::Close)] {
            let editor = DialogIntent::editor(primary);
            for prefer_wide in [false,true] {
                let alert = DialogIntent::alert_editor(primary,prefer_wide);
                assert_eq!(alert.frame,DialogFrame::Alert {prefer_wide});
                assert_eq!(alert.role,editor.role);
                assert_eq!(alert.modal,editor.modal);
                for subpage in [false,true] { assert_eq!(alert.header(subpage),editor.header(subpage)); }
            }
        }
        assert_eq!(DialogIntent::alert().frame,DialogFrame::Alert {prefer_wide:false});
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
