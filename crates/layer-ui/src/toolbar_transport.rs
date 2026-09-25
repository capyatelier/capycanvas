//! Stateless toolbar presentation queries shared by browser and native hosts.
//! Measurements are native; fitting, numeric formatting and icons stay here.
use crate::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolbarUiRequest {
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
    AutomaticTabNames {
        available: f32,
        widths: Vec<[f32; 2]>,
    },
    DrawerSourceCorners {
        anchor: Bounds,
        direction: Edge,
        container: Bounds,
    },
}

pub fn toolbar_ui(request: ToolbarUiRequest) -> Result<serde_json::Value, String> {
    use serde_json::json;
    Ok(match request {
        ToolbarUiRequest::SliderPreview {
            control,
            style,
            value,
            length,
            extent,
        } => json!(slider_preview_layout(
            control, style, value, length, extent
        )?),
        ToolbarUiRequest::SliderBookmarkValue {
            control,
            values,
            position,
            travel,
        } => json!(slider_bookmark_value(
            control, &values, position, travel
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
        } => {
            json!(toolbar_slider_layout(width, height, axis))
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
            let control = &request.control;
            let mut result = control.resolve(request.value, request.operation)?;
            if compact {
                result.text = if units {
                    control.compact_text(result.value)
                } else {
                    control.compact_value(result.value)
                };
            }
            json!(result)
        }
        ToolbarUiRequest::Style { style } => {
            json!({"size":style.size(), "gap":style.gap(), "icon":style.icon_size(), "labeled":style.label_lines()>0})
        }
        ToolbarUiRequest::AutomaticTabNames { available, widths } => {
            json!(TabStyle::automatic_names(available, &widths))
        }
        ToolbarUiRequest::DrawerSourceCorners { anchor, direction, container } => json!(
            DrawerPlacement { bounds: anchor, anchor, direction, columns: Vec::new() }.source_corners(container)
        ),
    })
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
    )
    .unwrap();
    assert_eq!(corners, serde_json::json!([false, false, true, true]));
}

#[test]
fn automatic_tab_names_are_a_stateless_toolbar_query() {
    let request = |available: f32| {
        toolbar_ui(
            serde_json::from_value(serde_json::json!({
                "type": "automatic_tab_names",
                "available": available,
                "widths": [[120., 36.], [90., 36.], [140., 36.]],
            }))
            .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(request(108.), serde_json::json!([false, false, false]));
    assert_eq!(request(192.), serde_json::json!([true, false, false]));
    assert_eq!(request(246.), serde_json::json!([true, true, false]));
    assert_eq!(request(400.), serde_json::json!([true, true, true]));
}
