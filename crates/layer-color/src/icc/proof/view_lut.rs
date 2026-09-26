//! Bounded, immutable viewing derivative. Build on a worker and publish the
//! entire value atomically; never substitute it for exact artwork or export.
use super::*;

pub struct ProofLut {
    space: RgbSpace,
    edge: usize,
    dark_grid: bool,
    // R varies fastest, matching a 3D GPU texture. Linear working RGB plus the
    // first and second gamut round-trip distances; alpha belongs to the sampled artwork, never this texture.
    samples: Box<[[f32; 5]]>,
}

impl ProofLut {
    /// Try 65³ then 129³ with an independent set of off-grid quality probes.
    /// A profile which cannot meet the viewing contract fails explicitly.
    pub fn build(
        space: RgbSpace,
        recipe: &ProofRecipe,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self, String> {
        let transform = ProofTransform::new(space, recipe)?;
        let mut reason = String::new();
        for (edge, dark_grid) in [(65, false), (129, false), (129, true)] {
            let result = Self::at_resolution(space, &transform, edge, dark_grid, &cancelled)?;
            match result.quality(&transform, &cancelled) {
                Ok(()) => return Ok(result),
                Err(error) => reason = error,
            }
        }
        Err(format!(
            "This profile exceeds the soft-proof preview's interpolation tolerance: {reason}"
        ))
    }

    pub fn space(&self) -> RgbSpace {
        self.space
    }
    pub fn edge(&self) -> u32 {
        self.edge as u32
    }
    /// A squared encoded grid concentrates samples in steep shadow mappings.
    pub fn dark_grid(&self) -> bool {
        self.dark_grid
    }
    pub fn samples(&self) -> &[[f32; 5]] {
        &self.samples
    }
    pub fn byte_len(&self) -> usize {
        std::mem::size_of_val(self.samples.as_ref())
    }

    /// Host-worker transport only, never a persisted or trusted ICC cache.
    /// Bounds and finite values are checked before GPU publication. The sender
    /// must have built these samples using `build` in the same app version.
    pub fn from_worker_samples(space: RgbSpace, edge: u32, dark_grid: bool, samples: Box<[[f32; 5]]>) -> Result<Self, String> {
        if !matches!(edge, 65 | 129) || (dark_grid && edge != 129) || samples.len() != (edge as usize).pow(3) {
            return Err("Invalid proof worker sample dimensions".into());
        }
        if samples.iter().flatten().any(|v| !v.is_finite()) { return Err("Invalid proof worker samples".into()); }
        Ok(Self { space, edge: edge as usize, dark_grid, samples })
    }

    fn at_resolution(
        space: RgbSpace,
        transform: &ProofTransform,
        edge: usize,
        dark_grid: bool,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Self, String> {
        let from_xyz = builtin(space)?.colorant_matrix().inverse().v;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(edge * edge * edge)
            .map_err(|_| "Not enough memory for the proof preview")?;
        for b in 0..edge {
            if cancelled() {
                return Err("Proof preparation cancelled".into());
            }
            for g in 0..edge {
                for r in 0..edge {
                    let rgb = [r, g, b].map(|v| {
                        let t = v as f32 / (edge - 1) as f32;
                        if dark_grid { t * t } else { t }
                    });
                    let sample = transform.sample(rgb)?;
                    let working = layer_core::color::rgb::apply(from_xyz, sample.xyz);
                    samples.push([
                        working[0] as f32,
                        working[1] as f32,
                        working[2] as f32,
                        sample.gamut_roundtrips[0] as f32,
                        sample.gamut_roundtrips[1] as f32,
                    ]);
                }
            }
        }
        Ok(Self {
            space,
            edge,
            dark_grid,
            samples: samples.into_boxed_slice(),
        })
    }

    /// Tetrahedral lookup in encoded working RGB. Out-of-domain artwork has an
    /// explicit error; a host may clamp for viewing with a disclosed SDR warning.
    pub fn sample(&self, encoded: [f32; 3]) -> Result<[f32; 5], String> {
        if encoded
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Soft proof input is outside the bounded SDR domain".into());
        }
        let coordinate =
            encoded.map(|v| (if self.dark_grid { v.sqrt() } else { v }) * (self.edge - 1) as f32);
        let mut low = coordinate.map(|v| (v as usize).min(self.edge - 2));
        let t: [f32; 3] = std::array::from_fn(|i| coordinate[i] - low[i] as f32);
        let mut axes = [0, 1, 2];
        axes.sort_by(|&a, &b| t[b].total_cmp(&t[a]));
        let at = |p: [usize; 3]| self.samples[(p[2] * self.edge + p[1]) * self.edge + p[0]];
        let mut previous = at(low);
        let mut result = previous;
        for axis in axes {
            low[axis] += 1;
            let next = at(low);
            for c in 0..5 {
                result[c] += t[axis] * (next[c] - previous[c]);
            }
            previous = next;
        }
        Ok(result)
    }

    fn quality(
        &self,
        transform: &ProofTransform,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(), String> {
        let working_to_xyz = builtin(self.space)?.colorant_matrix().v;
        let views = [RgbSpace::Srgb, RgbSpace::DisplayP3].map(|space| {
            let matrix = builtin(space)
                .expect("built-in viewing profile")
                .colorant_matrix();
            (space, matrix.v, matrix.inverse().v)
        });
        let mut errors = Vec::with_capacity(8192);
        let mut random = 0x43505059u32;
        for i in 0..4096 {
            if i % 256 == 0 && cancelled() {
                return Err("Proof preparation cancelled".into());
            }
            let input = if i < 256 {
                [i as f32 / 4097.; 3]
            } else {
                std::array::from_fn(|_| {
                    random ^= random << 13;
                    random ^= random >> 17;
                    random ^= random << 5;
                    let v = (random as f64 / u32::MAX as f64) as f32;
                    if i < 2048 { v * v * v } else { v }
                })
            };
            let direct = transform.sample(input)?;
            let sample = self.sample(input)?;
            let xyz = layer_core::color::rgb::apply(
                working_to_xyz,
                [sample[0] as f64, sample[1] as f64, sample[2] as f64],
            );
            for (space, to_xyz, from_xyz) in views {
                let clipped =
                    |xyz| layer_core::color::rgb::apply(from_xyz, xyz).map(|v| v.clamp(0., 1.));
                let actual = clipped(xyz);
                let expected = clipped(direct.xyz);
                errors.push(distance(
                    layer_core::color::rgb::apply(to_xyz, actual),
                    layer_core::color::rgb::apply(to_xyz, expected),
                ));
                if i < 256
                    && actual
                        .iter()
                        .zip(expected)
                        .any(|(a, b)| (space.encode(*a) - space.encode(b)).abs() > 1. / 255.)
                {
                    return Err(format!(
                        "neutral error at {input:?}: {actual:?} / {expected:?}"
                    ));
                }
            }
            if (direct.gamut_distance - 5.).abs() > 1.
                && direct.gamut_roundtrips.iter().all(|v| (*v - 5.).abs() > 1.)
                && (direct.gamut_distance > 5.) != (gamut_score(sample) > 5.)
            {
                return Err(format!(
                    "gamut error at {input:?}: {} / {}",
                    gamut_score(sample),
                    direct.gamut_distance
                ));
            }
        }
        errors.sort_by(f64::total_cmp);
        let p99 = errors[(errors.len() as f64 * 0.99).ceil() as usize - 1];
        let max = *errors.last().unwrap();
        if p99 <= 0.5 && max <= 2. {
            Ok(())
        } else {
            Err(format!("view DeltaE p99={p99}, max={max}"))
        }
    }
}

fn gamut_score(sample: [f32; 5]) -> f32 {
    if sample[4] < 5. {
        sample[3]
    } else {
        sample[3] / sample[4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_bounded_cancellable_and_preserves_alpha() {
        let recipe = ProofRecipe::new("sRGB".into(), ColorProfile::default());
        assert!(
            ProofLut::build(RgbSpace::Srgb, &recipe, || true)
                .err()
                .unwrap()
                .contains("cancelled")
        );
        let lut = ProofLut::build(RgbSpace::Srgb, &recipe, || false).unwrap();
        assert_eq!(lut.edge(), 65);
        assert_eq!(lut.byte_len(), 65usize.pow(3) * 20);
        let transport = |edge, dark_grid, samples: &[[f32; 5]]| {
            ProofLut::from_worker_samples(RgbSpace::Srgb, edge, dark_grid, samples.into())
        };
        let transported = transport(lut.edge(), lut.dark_grid(), lut.samples()).unwrap();
        assert_eq!(transported.samples(), lut.samples());
        assert!(transport(129, false, lut.samples()).is_err());
        assert!(transport(65, true, lut.samples()).is_err());
        let mut invalid = lut.samples().to_vec();
        invalid[0][0] = f32::NAN;
        assert!(transport(65, false, &invalid).is_err());
        assert!(lut.sample([f32::NAN, 0., 0.]).is_err());
        assert!(lut.sample([2., 0., 0.]).is_err());
    }
}
