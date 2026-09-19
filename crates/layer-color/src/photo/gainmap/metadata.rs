//! Validated per-channel gain metadata shared by JPEG and AVIF.
const INVALID: &str = "Invalid gain-map metadata";

#[derive(Clone, Debug)]
pub(in crate::photo) struct Metadata {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub gamma: [f32; 3],
    pub base_offset: [f32; 3],
    pub alternate_offset: [f32; 3],
    pub base_headroom: f32,
    pub alternate_headroom: f32,
    pub use_base_space: bool,
}
impl Metadata {
    pub fn validate(self) -> Result<Self, String> {
        for c in 0..3 {
            if !self.min[c].is_finite()
                || !self.max[c].is_finite()
                || self.min[c] > self.max[c]
                || self.min[c] < -64.
                || self.max[c] > 64.
                || !self.gamma[c].is_finite()
                || self.gamma[c] <= 0.
                || !self.base_offset[c].is_finite()
                || self.base_offset[c] < 0.
                || !self.alternate_offset[c].is_finite()
                || self.alternate_offset[c] < 0.
            {
                return Err(INVALID.into());
            }
        }
        if !self.base_headroom.is_finite()
            || !self.alternate_headroom.is_finite()
            || self.base_headroom < 0.
            || self.alternate_headroom <= self.base_headroom
            || self.alternate_headroom > 64.
        {
            return Err(INVALID.into());
        }
        Ok(self)
    }
    pub fn reconstruct(&self, base: [f32; 3], gain: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|c| {
            let log = self.min[c]
                + gain[c].clamp(0., 1.).powf(1. / self.gamma[c]) * (self.max[c] - self.min[c]);
            (base[c] + self.base_offset[c]) * log.exp2() - self.alternate_offset[c]
        })
    }
}
