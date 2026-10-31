//! Stateless toolbar presentation queries shared by browser and native hosts.
//! Measurements are native; fitting, numeric formatting and icons stay here.
use crate::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolbarUiRequest {
    Metrics,
    SliderPreview {
        control: ToolbarControl,
        style: TileStyle,
        value: f32,
        length: f32,
        extent: f32,
    },
    SliderBookmarkValue {
        control: ToolbarControl,
        values: Vec<f32>,
        position: f64,
        travel: f64,
    },
    OptionsLayout {
        width: f32,
        height: f32,
        axis: Axis,
        sizes: Vec<[f32; 2]>,
        button: [f32; 2],
        gap: f32,
    },
    SliderLayout {
        width: f32,
        height: f32,
        axis: Axis,
        style: TileStyle,
        slider_style: ToolbarSliderStyle,
    },
    SliderReadout {
        style: TileStyle,
        axis: Axis,
        control: NumericControl,
        value: f64,
    },
    SliderSpec {
        control: ToolbarControl,
    },
    NumericInfo {
        id: String,
        control: NumericControl,
        compact: bool,
        units: bool,
    },
    Number {
        request: NumericRequest,
        compact: bool,
        units: bool,
    },
    Style {
        style: TileStyle,
    },
    ChoiceStyle {
        labeled: bool,
        #[serde(default)]
        style: TileStyle,
    },
    DrawerSourceCorners {
        anchor: Bounds,
        direction: Edge,
        container: Bounds,
    },
}

pub fn toolbar_number(request: NumericRequest, compact: bool, units: bool) -> Result<NumericValue, NumericError> {
    let control = &request.control;
    let mut result = control.resolve(request.value, request.operation)?;
    if compact {
        result.text = if units { control.compact_text(result.value) } else { control.compact_value(result.value) };
    }
    Ok(result)
}

pub fn toolbar_metrics() -> serde_json::Value {
    serde_json::json!({
        "surface_radius": SURFACE_RADIUS,
        "panel": PANEL_CONTROL_METRICS,
        "layer_edge_inset": LAYER_EDGE_INSET,
        "divider": {"size": TOOLBAR_DIVIDER_SIZE, "length": DIVIDER_LENGTH, "inset": TOOLBAR_DIVIDER_INSET},
    })
}

pub fn toolbar_ui(request: ToolbarUiRequest, localizer: &Localizer) -> Result<serde_json::Value, String> {
    use serde_json::json;
    Ok(match request {
        ToolbarUiRequest::Metrics => toolbar_metrics(),
        ToolbarUiRequest::SliderPreview {
            control,
            style,
            value,
            length,
            extent,
        } => json!(slider_preview_layout(
            control, style, value, length, extent, localizer
        )?),
        ToolbarUiRequest::SliderBookmarkValue {
            control,
            values,
            position,
            travel,
        } => json!(slider_bookmark_value(
            control, &values, position, travel, localizer
        )?),
        ToolbarUiRequest::OptionsLayout {
            width,
            height,
            axis,
            sizes,
            button,
            gap,
        } => {
            json!(tool_options_layout(
                width, height, axis, &sizes, button, gap
            ))
        }
        ToolbarUiRequest::SliderLayout {
            width,
            height,
            axis,
            style,
            slider_style,
        } => {
            json!(toolbar_slider_styled_layout(width, height, axis, style, slider_style))
        }
        ToolbarUiRequest::SliderReadout { style, axis, control, value } => {
            let text = control.compact_value(value);
            json!({"value":text, "unit":control.unit, "metrics":toolbar_slider_readout_metrics(style, axis, &text)})
        }
        ToolbarUiRequest::SliderSpec { control } => {
            json!(control.slider().ok_or("Not a slider")?.numeric())
        }
        ToolbarUiRequest::NumericInfo {
            id,
            mut control,
            compact,
            units,
        } => {
            if !units {
                control.unit.clear();
            }
            json!({"icon": tool_setting_icon(&id), "samples": control.width_samples(compact)})
        }
        ToolbarUiRequest::Number {
            request,
            compact,
            units,
        } => {
            json!(toolbar_number(request, compact, units).map_err(|reason| reason.message(localizer))?)
        }
        ToolbarUiRequest::Style { style } => {
            json!({"size":style.size(), "gap":style.gap(), "icon":style.icon_size(), "labeled":style.label_lines()>0})
        }
        ToolbarUiRequest::ChoiceStyle { labeled, style } => toolbar_ui(
            ToolbarUiRequest::Style {style: tool_choice_style(labeled, style)}, localizer
        )?,
        ToolbarUiRequest::DrawerSourceCorners { anchor, direction, container } => json!(
            DrawerPlacement {tab_presentations:Vec::new(), bounds: anchor, anchor, direction, columns: Vec::new() }.source_corners(container)
        ),
    })
}

#[test]
fn slider_queries_publish_styled_geometry_and_unit_placement() {
    let localizer = Localizer::shared(UiLanguage::English);
    let control = ToolbarNumericBinding::BrushSize.numeric();
    for style in [TileStyle::Small, TileStyle::Medium, TileStyle::Large, TileStyle::MediumLabeled, TileStyle::Labeled] {
        for axis in [Axis::Horizontal, Axis::Vertical] {
            for value in [24., 99., 100., 430., 2048.] {
                let readout = toolbar_ui(ToolbarUiRequest::SliderReadout { style, axis, control: control.clone(), value }, &localizer).unwrap();
                assert_eq!(readout["value"], control.compact_value(value));
                assert_eq!(readout["unit"], "px");
                assert_eq!(readout["metrics"]["units_below"], axis == Axis::Vertical
                    && (style == TileStyle::Small || (style == TileStyle::Medium && value >= 100.)));
            }
            let slider_style = ToolbarSliderStyle { show_value: true, expand: true };
            let bounds = toolbar_ui(ToolbarUiRequest::SliderLayout { width: 400., height: 400., axis, style, slider_style }, &localizer).unwrap();
            assert_eq!(bounds, serde_json::to_value(toolbar_slider_styled_layout(400., 400., axis, style, slider_style)).unwrap());
        }
    }
}

#[test]
fn drawer_source_corners_are_a_stateless_toolbar_query() {
    let corners = toolbar_ui(
        serde_json::from_value(serde_json::json!({
            "type": "drawer_source_corners",
            "anchor": {"x": 10., "y": 10., "width": 36., "height": 36.},
            "direction": "bottom",
            "container": {"x": 0., "y": 4., "width": 60., "height": 42.},
        }))
        .unwrap(),
     &Localizer::shared(UiLanguage::English))
    .unwrap();
    assert_eq!(corners, serde_json::json!([false, false, true, true]));
}


#[test]
fn labeled_choices_keep_compact_icons_and_unlabeled_choices_keep_the_workspace_style() {
    let localizer = Localizer::shared(UiLanguage::English);
    let query = |labeled, style| toolbar_ui(serde_json::from_value(serde_json::json!({
        "type":"choice_style", "labeled":labeled, "style":style,
    })).unwrap(), &localizer).unwrap();
    let compact = serde_json::json!({"size":[36.,36.],"gap":2.,"icon":16,"labeled":false});
    for (style, size, gap, icon, labeled) in [
        (TileStyle::Small, [36.,36.], 2., 16, false),
        (TileStyle::Medium, [54.,54.], 2., 24, false),
        (TileStyle::Large, [72.,72.], 4., 32, false),
        (TileStyle::MediumLabeled, [108.,54.], 2., 16, true),
        (TileStyle::Labeled, [108.,72.], 4., 16, true),
    ] {
        assert_eq!(query(true, style), compact);
        assert_eq!(query(false, style), serde_json::json!({"size":size,"gap":gap,"icon":icon,"labeled":labeled}));
    }
    for labeled in [false, true] {
        let request = serde_json::from_value(serde_json::json!({"type":"choice_style","labeled":labeled})).unwrap();
        assert_eq!(toolbar_ui(request, &localizer).unwrap(), compact);
    }
}

#[test]
fn native_metrics_expose_the_shared_layout_roles() {
    let metrics = toolbar_ui(serde_json::from_value(serde_json::json!({"type":"metrics"})).unwrap(),
        &Localizer::shared(UiLanguage::English)).unwrap();
    assert_eq!(metrics["surface_radius"], serde_json::json!(SURFACE_RADIUS));
    assert_eq!(metrics["panel"], serde_json::json!(PANEL_CONTROL_METRICS));
    assert_eq!(metrics["layer_edge_inset"], serde_json::json!(LAYER_EDGE_INSET));
    assert_eq!(metrics["divider"], serde_json::json!({
        "size": TOOLBAR_DIVIDER_SIZE, "length": DIVIDER_LENGTH, "inset": TOOLBAR_DIVIDER_INSET,
    }));
}
