//! Stateless toolbar presentation queries shared by browser and native hosts.
//! Measurements are native; fitting, numeric formatting and icons stay here.
use crate::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolbarUiRequest {
    SliderPreview {
        control: ToolbarControl,
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
}

pub fn toolbar_ui(request: ToolbarUiRequest) -> Result<serde_json::Value, String> {
    use serde_json::json;
    Ok(match request {
        ToolbarUiRequest::SliderPreview {
            control,
            value,
            length,
            extent,
        } => json!(slider_preview_layout(control, value, length, extent)?),
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
            json!({"size":style.size(), "icon":style.icon_size(), "labeled":style.label_lines()>0})
        }
    })
}
