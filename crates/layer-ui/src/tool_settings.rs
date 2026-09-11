//! Brush controls are a shared schema, not a GTK form. Field access is kept
//! beside its label, constraints and visibility so native hosts never guess.
use crate::NumericControl;
use layer_core::{BrushExecution, BrushSnapshot, BrushTip};
use serde::Serialize;

/// Reusable tool settings actions. Labels, enabled/checked state and shortcuts
/// come from the same command model as menus and toolbar tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ToolSettingAction {
    pub command: crate::CommandId,
    pub checkable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolSetting {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub numeric: NumericControl,
    pub value: f32,
}

struct Definition {
    id: &'static str,
    label: &'static str,
    group: &'static str,
    numeric: fn() -> NumericControl,
    field: fn(&mut BrushSnapshot) -> Option<&mut f32>,
}

fn wet(b: &BrushSnapshot) -> bool {
    matches!(
        b.execution_class(),
        BrushExecution::Wet | BrushExecution::Smudge | BrushExecution::Watercolor
    )
}

const DEFINITIONS: &[Definition] = &[
    Definition {
        id: "size",
        label: "Brush size",
        group: "",
        numeric: NumericControl::brush_size,
        field: |b| Some(&mut b.diameter),
    },
    Definition {
        id: "opacity",
        label: "Opacity",
        group: "",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.opacity),
    },
    Definition {
        id: "flow",
        label: "Flow",
        group: "",
        numeric: NumericControl::percent,
        field: |b| (b.execution != BrushExecution::Liquify).then_some(&mut b.flow),
    },
    Definition {
        id: "hardness",
        label: "Hardness",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| (b.tip == BrushTip::AnalyticEllipse).then_some(&mut b.hardness),
    },
    Definition {
        id: "spacing",
        label: "Spacing",
        group: "Tip",
        numeric: || NumericControl {
            min: 0.005,
            soft_min: 0.005,
            max: 10.0,
            soft_max: 1.0,
            ..NumericControl::percent()
        },
        field: |b| Some(&mut b.spacing),
    },
    Definition {
        id: "angle",
        label: "Angle",
        group: "Tip",
        numeric: || NumericControl {
            scale: 180.0 / std::f64::consts::PI,
            step: std::f64::consts::PI / 180.0,
            resolution: 0.0001,
            digits: 0,
            ..NumericControl::number(-std::f64::consts::PI, std::f64::consts::PI, 0.01, 2).unit("°")
        },
        field: |b| Some(&mut b.angle_radians),
    },
    Definition {
        id: "size_jitter",
        label: "Size variation",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.size_jitter),
    },
    Definition {
        id: "rotation_jitter",
        label: "Rotation variation",
        group: "Tip",
        numeric: NumericControl::percent,
        field: |b| Some(&mut b.shape.rotation_jitter),
    },
    Definition {
        id: "grain_depth",
        label: "Texture strength",
        group: "Texture",
        numeric: NumericControl::percent,
        field: |b| b.grain.as_mut().map(|g| &mut g.depth),
    },
    Definition {
        id: "paint",
        label: "Paint load",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| wet(b).then_some(&mut b.wet_mix.amount_of_paint),
    },
    Definition {
        id: "pull",
        label: "Color pickup",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| wet(b).then_some(&mut b.wet_mix.pull),
    },
    Definition {
        id: "dilution",
        label: "Dilution",
        group: "Mixing",
        numeric: NumericControl::percent,
        field: |b| wet(b).then_some(&mut b.wet_mix.dilution),
    },
    Definition {
        id: "wet_edge",
        label: "Edge strength",
        group: "Watercolor",
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.wet_edge),
    },
    Definition {
        id: "edge_width",
        label: "Edge width",
        group: "Watercolor",
        numeric: || NumericControl::number(0.0, 32.0, 1.0, 1).unit("px"),
        field: |b| {
            (b.execution == BrushExecution::Watercolor).then_some(&mut b.rendering.edge_width)
        },
    },
    Definition {
        id: "wet_flow",
        label: "Wet bleed",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.wet_flow),
    },
    Definition {
        id: "dry_flow",
        label: "Dry bleed",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.dry_flow),
    },
    Definition {
        id: "distance",
        label: "Bleed distance",
        group: "Bleed",
        numeric: || NumericControl::number(0.0, 96.0, 1.0, 1).unit("px"),
        field: |b| b.transport.as_mut().map(|t| &mut t.distance),
    },
    Definition {
        id: "water_load",
        label: "Water load",
        group: "Bleed",
        numeric: NumericControl::percent,
        field: |b| b.transport.as_mut().map(|t| &mut t.water_load),
    },
    Definition {
        id: "strength",
        label: "Strength",
        group: "Liquify",
        numeric: NumericControl::percent,
        field: |b| (b.execution == BrushExecution::Liquify).then_some(&mut b.deform.strength),
    },
];

pub(crate) fn controls(brush: &BrushSnapshot) -> Vec<ToolSetting> {
    let mut brush = brush.clone();
    DEFINITIONS
        .iter()
        .filter_map(|d| {
            Some(ToolSetting {
                id: d.id,
                label: d.label,
                group: d.group,
                numeric: (d.numeric)(),
                value: *(d.field)(&mut brush)?,
            })
        })
        .collect()
}

pub(crate) fn edit(brush: &BrushSnapshot, id: &str, value: f32) -> Result<BrushSnapshot, String> {
    let d = DEFINITIONS
        .iter()
        .find(|d| d.id == id)
        .ok_or("Unknown tool setting")?;
    (d.numeric)().validate(value, d.label)?;
    let mut next = brush.clone();
    *(d.field)(&mut next).ok_or("This setting is not used by the selected tool")? = value;
    next.validate().map_err(|e| e.to_string())?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{DefaultBrushPreset, default_brush};

    #[test]
    fn every_visible_control_accepts_its_default_and_endpoints() {
        for choice in crate::brush_catalog() {
            let brush = default_brush(crate::preset(choice.id).unwrap());
            for control in controls(&brush) {
                assert_eq!(
                    edit(&brush, control.id, control.value).unwrap(),
                    brush,
                    "{}: {}",
                    choice.label,
                    control.id
                );
                for value in [control.numeric.min as f32, control.numeric.max as f32] {
                    edit(&brush, control.id, value).unwrap();
                }
            }
        }
    }

    #[test]
    fn irrelevant_or_invalid_edits_are_rejected() {
        let brush = default_brush(DefaultBrushPreset::GPen);
        for (id, value) in [
            ("water_load", 0.5),
            ("strength", 0.5),
            ("nope", 1.0),
            ("flow", f32::NAN),
            ("size", 3000.0),
        ] {
            assert!(edit(&brush, id, value).is_err());
        }
    }
}
