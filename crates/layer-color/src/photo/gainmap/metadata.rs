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
    /// ISO 21496-1 fields from the minimum version onward. `None` marks a newer
    /// minimum version. Only SDR-base maps are supported.
    pub fn parse_iso(
        mut bytes: &[u8],
        allow_common_denominator: bool,
    ) -> Result<Option<Self>, String> {
        fn take<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], String> {
            let (head, tail) = bytes.split_at_checked(N).ok_or(INVALID)?;
            *bytes = tail;
            Ok(head.try_into().unwrap())
        }
        if u16::from_be_bytes(take(&mut bytes)?) != 0 {
            return Ok(None);
        }
        let writer = u16::from_be_bytes(take(&mut bytes)?);
        let flags = take::<1>(&mut bytes)?[0];
        if flags & 4 != 0 {
            return Err("HDR-base gain maps are not supported".into());
        }
        if flags & 0x33 != 0 || flags & 8 != 0 && !allow_common_denominator {
            return Err(INVALID.into());
        }
        let common = if flags & 8 != 0 {
            Some(u32::from_be_bytes(take(&mut bytes)?))
        } else {
            None
        };
        let mut fraction = |signed| -> Result<f32, String> {
            let bits = take(&mut bytes)?;
            let n = if signed {
                f64::from(i32::from_be_bytes(bits))
            } else {
                f64::from(u32::from_be_bytes(bits))
            };
            let d = match common {
                Some(d) => d,
                None => u32::from_be_bytes(take(&mut bytes)?),
            };
            if d == 0 {
                return Err(INVALID.into());
            }
            Ok((n / f64::from(d)) as f32)
        };
        let mut m = Self {
            min: [0.; 3],
            max: [0.; 3],
            gamma: [1.; 3],
            base_offset: [0.; 3],
            alternate_offset: [0.; 3],
            base_headroom: fraction(false)?,
            alternate_headroom: fraction(false)?,
            use_base_space: flags & 0x40 != 0,
        };
        let channels = if flags & 0x80 != 0 { 3 } else { 1 };
        for c in 0..channels {
            m.min[c] = fraction(true)?;
            m.max[c] = fraction(true)?;
            m.gamma[c] = fraction(false)?;
            m.base_offset[c] = fraction(true)?;
            m.alternate_offset[c] = fraction(true)?;
        }
        if channels == 1 {
            for v in [
                &mut m.min,
                &mut m.max,
                &mut m.gamma,
                &mut m.base_offset,
                &mut m.alternate_offset,
            ] {
                *v = [v[0]; 3];
            }
        }
        if writer == 0 && !bytes.is_empty() {
            return Err(INVALID.into());
        }
        m.validate().map(Some)
    }
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
