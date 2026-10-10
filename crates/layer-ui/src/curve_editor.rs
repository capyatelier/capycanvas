use crate::{ContactPhase, CurveAxis, CurveControls, EffectAction, Modifiers, NumericOperation};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurveEditorView {
    pub controls: CurveControls,
    pub points: Vec<[f32; 2]>,
    pub plot: Vec<[f32; 2]>,
    pub marker: Option<[f32; 2]>,
    pub modified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveEditorTarget { Effect { layer: u64, key: String }, Pressure }

pub(crate) fn dragged_outside(point: [f32; 2], extent: [f32; 2]) -> bool {
    point.iter().zip(extent).any(|(position, length)| *position < -24. || *position > length + 24.)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveEditorAction {
    Contact { epoch: u64, phase: ContactPhase, point: [f32; 2], extent: [f32; 2] },
    RemoveAt { epoch: u64, point: [f32; 2], extent: [f32; 2], point_count: Option<usize> },
    Key { epoch: u64, key_event: String, pressed: bool, repeat: bool, modifiers: Modifiers },
    Number { epoch: u64, axis: CurveAxis, operation: NumericOperation },
    Reset,
    Gesture { phase: ContactPhase, action: Box<CurveEditorAction> },
}

impl CurveEditorAction {
    pub(crate) fn effect(self, layer: u64, key: &str) -> EffectAction {
        let key = key.to_string();
        match self {
            Self::Contact { epoch, phase, point, extent } => EffectAction::CurveContact { layer, key, epoch, phase, point, extent },
            Self::RemoveAt { epoch, point, extent, point_count } => EffectAction::CurveRemoveAt { layer, key, epoch, point, extent, point_count },
            Self::Key { epoch, key_event, pressed, repeat, modifiers } => EffectAction::CurveKey { layer, key, epoch, key_event, pressed, repeat, modifiers },
            Self::Number { epoch, axis, operation } => EffectAction::CurveNumber { layer, key, epoch, axis, operation },
            Self::Reset => EffectAction::Reset { layer, key },
            Self::Gesture { phase, action } => EffectAction::Gesture { phase, action: Box::new(action.effect(layer, &key)) },
        }
    }
}

impl crate::PropertyControl {
    pub fn curve_editor(&self) -> Option<CurveEditorView> {
        let layer_core::EffectValue::Curve(points) = &self.value else { return None; };
        Some(CurveEditorView { controls: self.curve.clone()?, points: points.clone(), plot: self.plot.clone(),
            marker: None, modified: self.modified })
    }
}
