//! Parameters of one GPU contact/deposition model for graphite and ink.
//!
//! The contact is evaluated in document space against stationary paper. Its
//! pose is interpolated between consecutive dabs; texture and strand identity
//! survive that interpolation. These are material parameters, not per-preset
//! shader choices. More elaborate GPU brush-state solvers can supply the same
//! resolved contact records later.

use crate::BrushError;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Strand count at 128 px diameter; larger tools retain fine bristles.
    pub fibers: f32,
    /// Strength of strand separation, 0 for an unbroken contact.
    pub fiber_strength: f32,
    /// Wet-contact broadening.
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
