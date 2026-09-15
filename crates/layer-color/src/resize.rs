//! Bounded separable resampling of linear, premultiplied document pixels.
//! Area reduction includes every covered source pixel. Enlargement uses the
//! interpolating Catmull–Rom cubic (B=0, C=1/2), with extended edge pixels.
use std::collections::VecDeque;

struct Taps {
    first: usize,
    len: usize,
    kernel: Kernel,
}
#[derive(Clone, Copy)]
enum Kernel {
    Area { start: u64, end: u64, unit: u64 },
    Cubic([f64; 4]),
}
impl Taps {
    fn weights(&self) -> impl Iterator<Item = f64> + '_ {
        (0..self.len).map(|index| match self.kernel {
            Kernel::Area { start, end, unit } => {
                let i = (self.first + index) as u64;
                (end.min((i + 1) * unit) - start.max(i * unit)) as f64 / (end - start) as f64
            }
            Kernel::Cubic(weights) => weights[index],
        })
    }
}
fn taps(source: u32, target: u32, index: u32) -> Taps {
    if source == target {
        return Taps {
            first: index as usize,
            len: 1,
            kernel: Kernel::Cubic([1., 0., 0., 0.]),
        };
    }
    if target < source {
        // Integral overlap on a common grid avoids rounding the area boundaries.
        let start = u64::from(index) * u64::from(source);
        let end = u64::from(index + 1) * u64::from(source);
        let unit = u64::from(target);
        let first = start / unit;
        let last = (end - 1) / unit;
        return Taps {
            first: first as usize,
            len: (last - first + 1) as usize,
            kernel: Kernel::Area { start, end, unit },
        };
    }
    let center = (f64::from(index) + 0.5) * f64::from(source) / f64::from(target) - 0.5;
    let base = center.floor() as i64;
    let first = (base - 1).clamp(0, i64::from(source) - 1);
    let last = (base + 2).clamp(0, i64::from(source) - 1);
    let mut weights = [0.; 4];
    for i in base - 1..=base + 2 {
        let x = (center - i as f64).abs();
        let weight = if x < 1. {
            1.5 * x * x * x - 2.5 * x * x + 1.
        } else if x < 2. {
            -0.5 * x * x * x + 2.5 * x * x - 4. * x + 2.
        } else {
            0.
        };
        weights[(i.clamp(first, last) - first) as usize] += weight;
    }
    let sum: f64 = weights.iter().sum();
    for weight in &mut weights {
        *weight /= sum;
    }
    Taps {
        first: first as usize,
        len: (last - first + 1) as usize,
        kernel: Kernel::Cubic(weights),
    }
}

/// At most four horizontally filtered rows plus one source row and one output
/// accumulator. Residency depends on width, not photo height or reduction ratio.
pub struct RowResampler {
    source: [u32; 2],
    target: [u32; 2],
    horizontal: Vec<Taps>,
    row: Vec<[f32; 4]>,
    cache: VecDeque<(usize, Vec<[f64; 4]>)>,
    sum: Vec<[f64; 4]>,
    next: u32,
}
impl RowResampler {
    pub fn new(source: [u32; 2], target: [u32; 2]) -> Result<Self, String> {
        for size in [source, target] {
            if size.into_iter().any(|v| v == 0 || v > 32768) {
                return Err("Image dimensions must be between 1 and 32768 pixels".into());
            }
        }
        Ok(Self {
            source,
            target,
            horizontal: (0..target[0])
                .map(|x| taps(source[0], target[0], x))
                .collect(),
            row: vec![[0.; 4]; source[0] as usize],
            cache: VecDeque::with_capacity(4),
            sum: vec![[0.; 4]; target[0] as usize],
            next: 0,
        })
    }
    /// Sequential rows permit bounded reuse. The provider must fill one source
    /// row and propagate cancellation/errors; a failed row cannot be retried.
    pub fn read_row(
        &mut self,
        y: u32,
        output: &mut [[f32; 4]],
        mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
    ) -> Result<(), String> {
        if y != self.next || y >= self.target[1] || output.len() != self.target[0] as usize {
            return Err("Invalid output row sequence or width".into());
        }
        self.next += 1;
        self.sum.fill([0.; 4]);
        let vertical = taps(self.source[1], self.target[1], y);
        for (i, weight) in vertical.weights().enumerate() {
            let sy = vertical.first + i;
            if !self.cache.iter().any(|(row, _)| *row == sy) {
                read(sy as u32, &mut self.row)?;
                if self.row.iter().flatten().any(|v| !v.is_finite()) {
                    return Err("Resampling requires finite document pixels".into());
                }
                let mut row = if self.cache.len() == 4 {
                    self.cache.pop_front().unwrap().1
                } else {
                    vec![[0.; 4]; self.target[0] as usize]
                };
                for (pixel, tap) in row.iter_mut().zip(&self.horizontal) {
                    *pixel = [0.; 4];
                    for (i, weight) in tap.weights().enumerate() {
                        for (out, value) in pixel.iter_mut().zip(self.row[tap.first + i]) {
                            *out += f64::from(value) * weight;
                        }
                    }
                }
                self.cache.push_back((sy, row));
            }
            let row = &self.cache.iter().find(|(row, _)| *row == sy).unwrap().1;
            for (pixel, row) in self.sum.iter_mut().zip(row) {
                for c in 0..4 {
                    pixel[c] += row[c] * weight;
                }
            }
        }
        for (out, sum) in output.iter_mut().zip(&self.sum) {
            let alpha = sum[3].clamp(0., 1.);
            if alpha == 0. {
                *out = [0.; 4];
            } else {
                // Cubic lobes can overshoot coverage. Normalize associated RGB
                // with it, preserving straight extended color rather than clipping.
                let scale = alpha / sum[3];
                *out = [
                    (sum[0] * scale) as f32,
                    (sum[1] * scale) as f32,
                    (sum[2] * scale) as f32,
                    alpha as f32,
                ];
            }
            if out.iter().any(|v| !v.is_finite()) {
                return Err("Resampled color exceeds the finite working range".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
