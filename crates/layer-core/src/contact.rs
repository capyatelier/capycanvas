//! Parameters of one GPU contact/deposition model for graphite and ink.
//!
//! The contact is evaluated in document space against stationary paper. Its
//! pose is interpolated between consecutive dabs; texture and strand identity
//! survive that interpolation. These are material parameters, not per-preset
//! shader choices. More elaborate GPU brush-state solvers can supply the same
//! resolved contact records later.

use crate::BrushError;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BrushContact {
    /// Strength of paper tooth in the contact threshold, 0 for a solid nib.
    pub paper: f32,
    /// How far pressure pushes the contact into the paper's tooth.
    pub pressure_gain: f32,
    /// Density concentrated on the tip-facing side of the contact.
    pub tip_bias: f32,
    /// Broadening of the contact along the stylus azimuth when tilted.
    pub tilt_spread: f32,
    /// Additional directional density falloff when tilted.
    pub tilt_shading: f32,
    /// Maximum irregularity as a fraction of the contact radius.
    pub edge_roughness: f32,
    /// Document-space size of edge irregularities, in pixels.
    pub edge_scale: f32,
    /// Number of coherent strand bands across the contact.
    pub fibers: f32,
    /// Strength of strand separation, 0 for an unbroken contact.
    pub fiber_strength: f32,
    /// Peripheral ink deposition, using the same paper/contact threshold.
    pub pooling: f32,
    /// Ink-load decay per nominal brush diameter of travel.
    pub depletion: f32,
}

impl Default for BrushContact {
    fn default() -> Self {
        Self {
            paper: 0.0,
            pressure_gain: 0.65,
            tip_bias: 0.0,
            tilt_spread: 0.0,
            tilt_shading: 0.0,
            edge_roughness: 0.0,
            edge_scale: 2.0,
            fibers: 24.0,
            fiber_strength: 0.0,
            pooling: 0.0,
            depletion: 0.0,
        }
    }
}

impl BrushContact {
    pub(crate) fn validate(self) -> Result<(), BrushError> {
        let unit = [
            self.paper,
            self.pressure_gain,
            self.tip_bias,
            self.tilt_shading,
            self.edge_roughness,
            self.fiber_strength,
            self.pooling,
            self.depletion,
        ];
        if unit
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !self.tilt_spread.is_finite()
            || !(0.0..=8.0).contains(&self.tilt_spread)
            || !self.edge_scale.is_finite()
            || !(0.25..=256.0).contains(&self.edge_scale)
            || !self.fibers.is_finite()
            || !(1.0..=128.0).contains(&self.fibers)
        {
            return Err(BrushError::InvalidAdvanced);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    #[test]
    fn contact_snapshots_are_versioned_and_legacy_snapshots_keep_their_semantics() {
        let old = BrushSnapshot::default();
        let mut json = serde_json::to_value(&old).unwrap();
        json["taper"]
            .as_object_mut()
            .unwrap()
            .remove("tip_sharpness");
        json["stabilization"]
            .as_object_mut()
            .unwrap()
            .remove("pressure_fall_micros");
        // Old experimental release fields are ignored, not reinterpreted.
        json["taper"]["release_micros"] = serde_json::json!(16_000);
        let restored: BrushSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(old, restored);
        restored.validate().unwrap();
        let mut invalid = default_brush(DefaultBrushPreset::GPen);
        assert_eq!(invalid.stabilization.pressure_fall_micros, 34_133);
        invalid.stabilization.pressure_fall_micros = 1_000_001;
        assert!(invalid.validate().is_err());
        for preset in CONTACT_BRUSH_PRESETS {
            let brush = default_brush(preset);
            let decoded: BrushSnapshot =
                serde_json::from_slice(&serde_json::to_vec(&brush).unwrap()).unwrap();
            assert_eq!(decoded, brush);
            assert_eq!(decoded.schema_version, 5);
            decoded.validate().unwrap();
            let mut incorrectly_versioned = brush;
            incorrectly_versioned.schema_version = 4;
            assert!(incorrectly_versioned.validate().is_err());
        }
    }
}
