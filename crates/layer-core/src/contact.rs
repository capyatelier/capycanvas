//! Parameters of one GPU contact/deposition model for graphite and ink.
//!
//! The contact is evaluated in document space against stationary paper. Its
//! pose is interpolated between consecutive dabs; texture and strand identity
//! survive that interpolation. These are material parameters, not per-preset
//! shader choices. More elaborate GPU brush-state solvers can supply the same
//! resolved contact records later.

use crate::BrushError;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushDryPose {
    pub radii: [f32; 2],
    pub relief: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushDryMedia {
    pub poses: [BrushDryPose; 3],
    pub softness: f32,
    pub loading: f32,
    pub corner: f32,
    pub relief_offset: f32,
}

impl BrushDryMedia {
    pub const WRITING_TILT_RADIANS: f32 = std::f32::consts::PI * (2. / 9.);
    pub const SIDE_TILT_RADIANS: f32 = std::f32::consts::PI / 3.;

    pub fn extent(self) -> f32 {
        self.poses.iter().map(|p| if self.corner > 0. { p.radii[0].hypot(p.radii[1]) }
            else { p.radii[0].max(p.radii[1]) }).fold(0., f32::max)
    }

    pub fn minor_extent(self, aspect: f32) -> f32 {
        self.poses.windows(2).map(|pair| pair[0].radii[0].max(pair[1].radii[0])
            .min(pair[0].radii[1].max(pair[1].radii[1]) / aspect)).fold(0., f32::max)
    }

    pub fn without_tilt(self) -> Self {
        let mut poses = self.poses;
        let upright = poses[0];
        let long = upright.radii[0].max(upright.radii[1]);
        poses[0].radii = upright.radii.map(|radius| radius / long);
        Self { poses, ..self }
    }

    pub fn pose(self, inclination_radians: f32) -> BrushDryPose {
        let (i, t) = if inclination_radians < Self::WRITING_TILT_RADIANS {
            (0, inclination_radians / Self::WRITING_TILT_RADIANS)
        } else {
            (1, (inclination_radians - Self::WRITING_TILT_RADIANS)
                / (Self::SIDE_TILT_RADIANS - Self::WRITING_TILT_RADIANS))
        };
        let t = t.clamp(0., 1.);
        let w = t * t * (3. - 2. * t);
        let a = self.poses[i];
        let b = self.poses[i + 1];
        BrushDryPose {
            radii: std::array::from_fn(|j| a.radii[j] + (b.radii[j] - a.radii[j]) * w),
            relief: std::array::from_fn(|j| a.relief[j] + (b.relief[j] - a.relief[j]) * w),
        }
    }

    fn validate(self) -> Result<(), BrushError> {
        if !(0.005..=0.8).contains(&self.softness)
            || !(0.0..=1.0).contains(&self.loading)
            || !(0.0..=1.0).contains(&self.corner)
            || !self.relief_offset.is_finite() || self.relief_offset.abs() > 1.
            || (self.corner > 0. && self.relief_offset != 0.)
            || self.poses.iter().any(|p| p.radii.iter().any(|v| !v.is_finite() || !(0.02..=8.).contains(v))
                || p.relief.iter().any(|v| !v.is_finite() || !(0.0..=4.).contains(v)))
        {
            return Err(BrushError::InvalidAdvanced);
        }
        Ok(())
    }
}

/// A flat fan of hairs swept across the paper. Hair spacing is relative to the
/// configured diameter, independently of instantaneous pressure and zoom.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushBristles {
    /// Hair spacing relative to the default for the configured diameter.
    pub texture_scale: f32,
    /// How far the fan narrows as pressure is released.
    pub splay: f32,
    /// Paint carried at the start of each stroke.
    pub load: f32,
    /// A second paint on the hairs, in straight linear document RGB. Thick
    /// ridges of paint show it. Painting tools load the color swatch that is
    /// not painting.
    pub streak_rgba_linear: [f32; 4],
}

impl Default for BrushBristles {
    fn default() -> Self {
        Self {
            texture_scale: 1.,
            splay: 0.65,
            load: 0.8,
            streak_rgba_linear: [0.02, 0.02, 0.018, 1.],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushContact {
    pub bristles: Option<BrushBristles>,
    pub dry: Option<BrushDryMedia>,
    /// Strength of paper tooth in the contact threshold, 0 for a solid nib.
    pub paper: f32,
    /// How far pressure pushes the contact into the paper's tooth.
    pub pressure_gain: f32,
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
    /// Coverage falls linearly from the solid core to the rim, as an analytic
    /// stamp's does, instead of along a smooth curve.
    #[serde(default)]
    pub linear_edge: bool,
}

impl Default for BrushContact {
    fn default() -> Self {
        Self {
            bristles: None,
            dry: None,
            paper: 0.0,
            pressure_gain: 0.65,
            edge_roughness: 0.0,
            edge_scale: 2.0,
            fibers: 24.0,
            fiber_strength: 0.0,
            pooling: 0.0,
            depletion: 0.0,
            linear_edge: false,
        }
    }
}

impl BrushContact {
    pub(crate) fn validate(self) -> Result<(), BrushError> {
        if let Some(dry) = self.dry {
            dry.validate()?;
            if self.bristles.is_some() { return Err(BrushError::InvalidAdvanced); }
        }
        if let Some(b) = self.bristles
            && (!(0.25..=4.).contains(&b.texture_scale)
                || [b.splay, b.load, b.streak_rgba_linear[3]].iter().any(|v| !(0.0..=1.0).contains(v))
                || b.streak_rgba_linear[..3].iter().any(|v| !v.is_finite()))
        {
            return Err(BrushError::InvalidAdvanced);
        }
        let unit = [
            self.pressure_gain,
            self.edge_roughness,
            self.fiber_strength,
            self.pooling,
            self.depletion,
        ];
        if unit
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !self.paper.is_finite() || !(0.0..=if self.dry.is_some() { 3. } else { 1. }).contains(&self.paper)
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
