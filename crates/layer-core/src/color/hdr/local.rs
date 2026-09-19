//! Bounded, coverage-aware fast local-Laplacian illumination analysis.
//!
//! Independent implementation of sampled range remapping and Laplacian
//! reconstruction (Paris et al. 2011, Aubry et al. 2014). All arithmetic and
//! stored guide samples are Float32. Analysis is fixed in document space; it
//! never depends on the canvas zoom, viewport, export dimensions or monitor.
//! The bounded guide is an explicit spatial approximation, not pixel storage.
use super::sdr_luminance_weights;
use crate::color::RgbSpace;

pub const LOCAL_GUIDE_EDGE: u32 = 768;
const FLOOR: f32 = -24.;
const SIGMA: f32 = 1.5;

#[derive(Clone, Debug)]
pub struct LocalToneGuide {
    pub extent: [u32; 2],
    pub document_extent: [u32; 2],
    /// log luminance, local illumination, coverage, reserved. GPU-compatible.
    pub samples: Vec<[f32; 4]>,
    pub peak: f32,
}

pub struct LocalToneBuilder {
    document: [u32; 2],
    extent: [u32; 2],
    weights: [f32; 3],
    sums: Vec<[f32; 3]>,
    next_row: u32,
    peak: f32,
}
impl LocalToneBuilder {
    /// Bounded worker handoff after all source rows have been reduced. The
    /// expensive Laplacian reconstruction can run away from the input owner.
    pub fn into_worker_samples(self) -> Result<(Vec<[f32; 3]>, f32), String> {
        if self.next_row != self.document[1] { return Err("Incomplete local tone analysis".into()); }
        Ok((self.sums, self.peak))
    }
    pub fn from_worker_samples(document: [u32; 2], space: RgbSpace, sums: Vec<[f32; 3]>, peak: f32) -> Result<Self, String> {
        let mut builder = Self::new(document, space)?;
        if sums.len() != builder.sums.len() || (!peak.is_finite() || peak<1.) || sums.iter().any(|s|s.iter().any(|v|!v.is_finite())||s[1]<0.||s[2]<s[1]-1e-5) {
            return Err("Invalid local tone worker samples".into());
        }
        builder.sums = sums;
        builder.peak = peak;
        builder.next_row = document[1];
        Ok(builder)
    }
    pub fn new(document: [u32; 2], space: RgbSpace) -> Result<Self, String> {
        if document.contains(&0) || document.iter().any(|&v| v > 32768) {
            return Err("Invalid local tone-map dimensions".into());
        }
        let scale = (document[0].max(document[1]) as f32 / LOCAL_GUIDE_EDGE as f32).max(1.);
        let extent = document.map(|v| (v as f32 / scale).ceil() as u32);
        Ok(Self {
            document,
            extent,
            weights: sdr_luminance_weights(space),
            sums: vec![[0.; 3]; (extent[0] * extent[1]) as usize],
            next_row: 0,
            peak: 1.,
        })
    }
    pub fn push(&mut self, row: &[[f32; 4]]) -> Result<(), String> {
        if row.len() != self.document[0] as usize || self.next_row >= self.document[1] {
            return Err("Invalid local tone-map row".into());
        }
        let sy = self.extent[1] as f32 / self.document[1] as f32;
        let ya = self.next_row as f32 * sy;
        let yb = (self.next_row + 1) as f32 * sy;
        let sx = self.extent[0] as f32 / self.document[0] as f32;
        for (x, p) in row.iter().enumerate() {
            if p.iter().any(|v| !v.is_finite()) {
                return Err("Non-finite HDR sample in local tone analysis".into());
            }
            if !(0. ..=1.).contains(&p[3]) {return Err("Invalid coverage in local tone analysis".into());}
            if p[3] <= 0. {
                continue;
            }
            let y =
                (p[0] * self.weights[0] + p[1] * self.weights[1] + p[2] * self.weights[2]) / p[3];
            self.peak = self.peak.max(y);
            let value = y.max(2f32.powi(-24)).log2();
            let xa = x as f32 * sx;
            let xb = (x + 1) as f32 * sx;
            for gy in ya.floor() as u32..(yb.ceil() as u32).min(self.extent[1]) {
                let wy = (yb.min((gy + 1) as f32) - ya.max(gy as f32)).max(0.);
                for gx in xa.floor() as u32..(xb.ceil() as u32).min(self.extent[0]) {
                    let weight = wy * (xb.min((gx + 1) as f32) - xa.max(gx as f32)).max(0.);
                    let s = &mut self.sums[(gy * self.extent[0] + gx) as usize];
                    s[0] += value * weight * p[3];
                    s[1] += weight * p[3];
                    s[2] += weight;
                }
            }
        }
        self.next_row += 1;
        Ok(())
    }
    pub fn finish(self, cancelled: impl Fn() -> bool) -> Result<LocalToneGuide, String> {
        if self.next_row != self.document[1] {
            return Err("Incomplete local tone analysis".into());
        }
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        let pixels: Vec<_> = self
            .sums
            .iter()
            .map(|s| {
                let v = if s[1] > 0. { s[0] / s[1] } else { FLOOR };
                if s[1] > 0. {
                    low = low.min(v);
                    high = high.max(v);
                }
                [v, s[1].min(1.)]
            })
            .collect();
        if !low.is_finite() {
            low = FLOOR;
            high = FLOOR;
        }
        let original = pyramid(
            Plane {
                extent: self.extent,
                pixels,
            },
            &cancelled,
        )?;
        let mut detail: Vec<Vec<f32>> = original.iter().map(|p| vec![0.; p.pixels.len()]).collect();
        // At most half an EV between samples over the occupied HDR
        // range. Retain one remapped pyramid at a time, not N full pyramids.
        let intervals = ((high - low) / 0.5).ceil().max(1.) as u32;
        let step = ((high - low) / intervals as f32).max(0.00001);
        for index in 0..=intervals {
            check(&cancelled)?;
            let anchor = low + index as f32 * step;
            let pixels = original[0]
                .pixels
                .iter()
                .map(|p| {
                    let d = (p[0] - anchor) / SIGMA;
                    // Smooth bounded detail remapping; unit slope at zero,
                    // tending to +/-sigma at strong edges. Avoid hard thresholds.
                    [SIGMA * d / (1. + d.abs()), p[1]]
                })
                .collect();
            let remapped = pyramid(
                Plane {
                    extent: self.extent,
                    pixels,
                },
                &cancelled,
            )?;
            for level in 0..original.len() - 1 {
                let plane = &original[level];
                for y in 0..plane.extent[1] {
                    check(&cancelled)?;
                    for x in 0..plane.extent[0] {
                        let i = (y * plane.extent[0] + x) as usize;
                        let q = ((plane.pixels[i][0] - low) / step).clamp(0., intervals as f32);
                        let weight = (1. - (q - index as f32).abs()).max(0.);
                        if weight == 0. {
                            continue;
                        }
                        let expanded =
                            sample(&remapped[level + 1], [x as f32 * 0.5, y as f32 * 0.5]);
                        detail[level][i] += weight * (remapped[level].pixels[i][0] - expanded);
                    }
                }
            }
        }
        for level in (0..original.len() - 1).rev() {
            check(&cancelled)?;
            let coarse = Plane {
                extent: original[level + 1].extent,
                pixels: detail[level + 1]
                    .iter()
                    .zip(&original[level + 1].pixels)
                    .map(|(v, p)| [*v, p[1]])
                    .collect(),
            };
            for y in 0..original[level].extent[1] {
                for x in 0..original[level].extent[0] {
                    detail[level][(y * original[level].extent[0] + x) as usize] +=
                        sample(&coarse, [x as f32 * 0.5, y as f32 * 0.5]);
                }
            }
        }
        let samples = original[0]
            .pixels
            .iter()
            .zip(&detail[0])
            .map(|(p, d)| [p[0], p[0] - d, p[1], 0.])
            .collect();
        Ok(LocalToneGuide {
            extent: self.extent,
            document_extent: self.document,
            samples,
            peak: self.peak,
        })
    }
}
impl LocalToneGuide {
    pub fn byte_len(&self) -> usize {
        self.samples.len() * 16 + 16
    }
    /// Full document coordinates, at pixel centers. The guide uses a bilateral
    /// four-point gather: neighboring transparent pixels and unrelated edge
    /// luminance cannot darken covered artwork. Same equations in WGSL.
    pub fn illumination(&self, position: [f32; 2], log_y: f32) -> f32 {
        let q: [f32; 2] = std::array::from_fn(|c| {
            (position[c] * self.extent[c] as f32 / self.document_extent[c] as f32 - 0.5)
                .clamp(0., (self.extent[c] - 1) as f32)
        });
        let low = q.map(|v| v.floor() as u32);
        let t = [q[0] - low[0] as f32, q[1] - low[1] as f32];
        let mut total = 0.;
        let mut value = 0.;
        for dy in 0..2 {
            for dx in 0..2 {
                let x = (low[0] + dx).min(self.extent[0] - 1);
                let y = (low[1] + dy).min(self.extent[1] - 1);
                let p = self.samples[(y * self.extent[0] + x) as usize];
                let delta = (p[0] - log_y) / SIGMA;
                let weight = (if dx == 0 { 1. - t[0] } else { t[0] })
                    * (if dy == 0 { 1. - t[1] } else { t[1] })
                    * p[2]
                    / (1. + delta * delta * delta * delta);
                total += weight;
                value += weight * p[1];
            }
        }
        if total > 1e-12 { value / total } else { log_y }
    }
}

struct Plane {
    extent: [u32; 2],
    pixels: Vec<[f32; 2]>,
}
fn check(cancelled: &impl Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Local tone mapping cancelled".into())
    } else {
        Ok(())
    }
}
fn sample(p: &Plane, q: [f32; 2]) -> f32 {
    let q: [f32; 2] = std::array::from_fn(|c| q[c].clamp(0., (p.extent[c] - 1) as f32));
    let low = q.map(|v| v.floor() as u32);
    let t = [q[0] - low[0] as f32, q[1] - low[1] as f32];
    let mut value = 0.;
    let mut total = 0.;
    for dy in 0..2 {
        for dx in 0..2 {
            let v = p.pixels[((low[1] + dy).min(p.extent[1] - 1) * p.extent[0]
                + (low[0] + dx).min(p.extent[0] - 1)) as usize];
            let w = (if dx == 0 { 1. - t[0] } else { t[0] })
                * (if dy == 0 { 1. - t[1] } else { t[1] })
                * v[1];
            value += w * v[0];
            total += w;
        }
    }
    if total > 0. { value / total } else { 0. }
}
fn pyramid(first: Plane, cancelled: &impl Fn() -> bool) -> Result<Vec<Plane>, String> {
    let mut levels = vec![first];
    while levels.last().unwrap().extent != [1, 1] {
        check(cancelled)?;
        let p = levels.last().unwrap();
        let size = p.extent.map(|v| v.div_ceil(2));
        let kernel = [1., 4., 6., 4., 1.];
        let mut horizontal = vec![[0.; 2]; (size[0] * p.extent[1]) as usize];
        for y in 0..p.extent[1] {
            check(cancelled)?;
            for x in 0..size[0] {
                let mut v = 0.;
                let mut w = 0.;
                for k in 0..5 {
                    let sx = (x as i32 * 2 + k as i32 - 2).clamp(0, p.extent[0] as i32 - 1) as u32;
                    let s = p.pixels[(y * p.extent[0] + sx) as usize];
                    v += s[0] * s[1] * kernel[k];
                    w += s[1] * kernel[k];
                }
                horizontal[(y * size[0] + x) as usize] = [if w > 0. { v / w } else { 0. }, w / 16.];
            }
        }
        let mut pixels = vec![[0.; 2]; (size[0] * size[1]) as usize];
        for y in 0..size[1] {
            check(cancelled)?;
            for x in 0..size[0] {
                let mut v = 0.;
                let mut w = 0.;
                for k in 0..5 {
                    let sy = (y as i32 * 2 + k as i32 - 2).clamp(0, p.extent[1] as i32 - 1) as u32;
                    let s = horizontal[(sy * size[0] + x) as usize];
                    v += s[0] * s[1] * kernel[k];
                    w += s[1] * kernel[k];
                }
                pixels[(y * size[0] + x) as usize] = [if w > 0. { v / w } else { 0. }, w / 16.];
            }
        }
        levels.push(Plane {
            extent: size,
            pixels,
        });
    }
    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn guide(extent: [u32; 2], pixels: &[[f32; 4]]) -> LocalToneGuide {
        let mut b = LocalToneBuilder::new(extent, RgbSpace::Srgb).unwrap();
        for row in pixels.chunks_exact(extent[0] as usize) {
            b.push(row).unwrap();
        }
        b.finish(|| false).unwrap()
    }
    #[test]
    fn bounded_worker_handoff_matches_native_analysis_and_rejects_invalid_data() {
        let pixels:Vec<_>=(0..17*9).map(|i| {let a=if i%7==0 {0.}else{0.5};let v=2f32.powf(i as f32/20.-4.);[v*a,-0.1*a,0.3*a,a]}).collect();
        let expected=guide([17,9],&pixels);
        let mut builder=LocalToneBuilder::new([17,9],RgbSpace::Srgb).unwrap();
        for row in pixels.chunks_exact(17){builder.push(row).unwrap();}
        let (samples,peak)=builder.into_worker_samples().unwrap();
        let actual=LocalToneBuilder::from_worker_samples([17,9],RgbSpace::Srgb,samples.clone(),peak).unwrap().finish(||false).unwrap();
        assert_eq!(actual.samples,expected.samples);assert_eq!(actual.peak,expected.peak);
        assert!(LocalToneBuilder::from_worker_samples([17,9],RgbSpace::Srgb,samples.clone(),f32::NAN).is_err());
        let mut invalid=samples;invalid[0][1]=-1.;
        assert!(LocalToneBuilder::from_worker_samples([17,9],RgbSpace::Srgb,invalid,peak).is_err());
        assert!(LocalToneBuilder::new([17,9],RgbSpace::Srgb).unwrap().into_worker_samples().is_err());
    }
    #[test]
    fn hidden_rgb_does_not_contribute_and_analysis_is_cancellable() {
        let mut pixels = vec![[0.2, 0.2, 0.2, 1.]; 32 * 16];
        for p in &mut pixels[..256] {
            *p = [65000., -65000., 1000., 0.];
        }
        let a = guide([32, 16], &pixels);
        for p in &mut pixels[..256] {
            *p = [0.; 4];
        }
        let b = guide([32, 16], &pixels);
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.peak, b.peak);
        let mut builder = LocalToneBuilder::new([32, 16], RgbSpace::Srgb).unwrap();
        for row in pixels.chunks_exact(32) {
            builder.push(row).unwrap();
        }
        assert!(builder.finish(|| true).unwrap_err().contains("cancelled"));
        assert!(LocalToneBuilder::new([0, 16], RgbSpace::Srgb).is_err());
    }
}
