//! Full-resolution document inspection, independent of output/display transforms.
use super::DocumentColor;
mod encoded_bins;

pub const BINS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Channel {
    pub bins: Vec<u64>,
    /// Values outside the SDR range, before histogram bin clamping.
    pub below: u64,
    pub above: u64,
    /// Values at or beyond the endpoints, including `below` / `above`.
    pub black: u64,
    pub white: u64,
}
impl Channel {
    fn new() -> Self {
        Self {
            bins: vec![0; BINS],
            ..Default::default()
        }
    }
    fn add(&mut self, bin: usize, linear: f64) {
        self.below += u64::from(linear < 0.);
        self.above += u64::from(linear > 1.);
        self.black += u64::from(linear <= 0.);
        self.white += u64::from(linear >= 1.);
        self.bins[bin] += 1;
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Histogram {
    pub color: DocumentColor,
    /// Profile-encoded document RGB followed by linear relative luminance Y.
    pub channels: [Channel; 4],
    /// Each nontransparent pixel counts once, regardless of partial coverage.
    pub pixels: u64,
    pub transparent: u64,
    #[serde(skip)]
    luminance: [f64; 3],
}
impl Histogram {
    /// Positive HDR bins shown by an inspector. Keep white and nearby stops in
    /// view; expand to include occupied tails. Bin zero has a separate count.
    pub fn hdr_bin(&self, linear: f64) -> usize {
        if self.color.depth == super::SampleDepth::F32 { bin_between(linear, -149., 128.) } else { hdr_bin(linear) }
    }
    pub fn hdr_bin_stops(&self, bin: usize) -> f64 {
        if self.color.depth == super::SampleDepth::F32 { (bin.saturating_sub(1) as f64 / 254.) * 277. - 149. } else { hdr_bin_stops(bin) }
    }
    pub fn plot_bins(&self) -> std::ops::Range<usize> {
        if !self.color.depth.is_float() { return 0..BINS; }
        let occupied = |i| self.channels.iter().any(|c| c.bins[i] > 0);
        let first = (1..BINS).find(|&i| occupied(i)).unwrap_or(self.hdr_bin(1.));
        let last = (1..BINS).rev().find(|&i| occupied(i)).unwrap_or(self.hdr_bin(1.));
        let low = self.hdr_bin_stops(first).floor().min(-4.).max(if self.color.depth == super::SampleDepth::F32 { -149. } else { -12. });
        let high = (self.hdr_bin_stops(last).ceil() + 1.).max(2.).min(if self.color.depth == super::SampleDepth::F32 { 128. } else { 16. });
        self.hdr_bin(2f64.powf(low))..(self.hdr_bin(2f64.powf(high)) + 1).min(BINS)
    }
    pub fn new(color: DocumentColor) -> Self {
        Self {
            color,
            channels: std::array::from_fn(|_| Channel::new()),
            pixels: 0,
            transparent: 0,
            luminance: color.space.to_xyz()[1],
        }
    }

    /// Consume linear-premultiplied working pixels in bounded strips. Alpha zero
    /// contributes no RGB; partial alpha is unassociated before counting. Never
    /// feed a downsampled preview: averaging changes the distribution.
    pub fn add(&mut self, pixels: &[[f32; 4]]) -> Result<(), &'static str> {
        let encoded = encoded_bins::for_space(self.color.space);
        for pixel in pixels {
            if pixel.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&pixel[3]) {
                return Err("Invalid histogram pixel");
            }
            if pixel[3] == 0. {
                self.transparent += 1;
                continue;
            }
            self.pixels += 1;
            let rgb = if pixel[3] == 1. {
                [0, 1, 2].map(|c| f64::from(pixel[c]))
            } else {
                [0, 1, 2].map(|c| f64::from(pixel[c]) / f64::from(pixel[3]))
            };
            for c in 0..3 {
                let bin = if self.color.depth.is_float() { self.hdr_bin(rgb[c]) } else { encoded.index(rgb[c]) };
                self.channels[c].add(bin, rgb[c]);
            }
            // Algebraically equal to dot(Y, RGB), with neutral values exact at
            // the endpoints instead of depending on rounded coefficient sums.
            let y = rgb[1]
                + self.luminance[0] * (rgb[0] - rgb[1])
                + self.luminance[2] * (rgb[2] - rgb[1]);
            self.channels[3].add(
                if self.color.depth.is_float() { self.hdr_bin(y) } else { (y.clamp(0., 1.) * BINS as f64)
                    .floor()
                    .min((BINS - 1) as f64) as usize },
                y,
            );
        }
        Ok(())
    }
}

/// Bin 0 counts nonpositive channels; remaining bins cover -12..+16 stops
/// relative to portable reference white. `above` still reports above-white data.
pub fn hdr_bin(linear: f64) -> usize {
    bin_between(linear, -12., 16.)
}
fn bin_between(linear: f64, low: f64, high: f64) -> usize {
    if linear <= 0. { 0 } else { 1 + (((linear.log2()-low)/(high-low)).clamp(0.,1.)*254.).floor() as usize }
}
/// Lower stop boundary of a positive HDR bin.
pub fn hdr_bin_stops(bin: usize) -> f64 { (bin.saturating_sub(1) as f64 / 254.) * 28. - 12. }
#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{SampleDepth, RgbSpace};

    #[test]
    fn fitted_hdr_axis_contains_all_positive_bins_and_white() {
        let mut h = Histogram::new(DocumentColor { depth: SampleDepth::F16, ..Default::default() });
        for values in [[-2., 0., 1., 1.], [0.000001, 8., 65504., 1.]] {
            h.add(&[values]).unwrap();
            let range = h.plot_bins();
            assert!(range.contains(&hdr_bin(1.)));
            for channel in &h.channels { for (i, count) in channel.bins.iter().enumerate().skip(1) {
                if *count > 0 { assert!(range.contains(&i)); }
            }}
        }
        assert_eq!(Histogram::new(DocumentColor::default()).plot_bins(), 0..BINS);
    }
    #[test]
    fn native_codes_transparency_and_strip_partition_do_not_change_distributions() {
        for space in RgbSpace::ALL {
            for depth in [SampleDepth::U8, SampleDepth::U16] {
                let color = DocumentColor { space, depth };
                let maximum: u32 = if depth == SampleDepth::U8 {
                    255
                } else {
                    65535
                };
                let mut pixels: Vec<_> = (0..=maximum)
                    .map(|code| {
                        let v = space.decode(f64::from(code) / f64::from(maximum)) as f32;
                        [v, v, v, 1.]
                    })
                    .collect();
                pixels.push([100., -100., 3., 0.]);
                let mut all = Histogram::new(color);
                all.add(&pixels).unwrap();
                let mut strips = Histogram::new(color);
                for strip in pixels.chunks(137) {
                    strips.add(strip).unwrap();
                }
                assert_eq!(all, strips);
                assert_eq!(all.transparent, 1);
                assert_eq!(all.pixels, maximum as u64 + 1);
                for channel in &all.channels[..3] {
                    assert!(
                        channel
                            .bins
                            .iter()
                            .all(|v| *v == u64::from(maximum + 1) / 256)
                    );
                    assert_eq!(
                        (channel.black, channel.white, channel.below, channel.above),
                        (1, 1, 0, 0)
                    );
                }
                assert_eq!(all.channels[3].bins.iter().sum::<u64>(), all.pixels);
                assert_eq!((all.channels[3].black, all.channels[3].white), (1, 1));
            }
        }
    }

    #[test]
    fn alpha_range_and_luminance_are_document_values() {
        let mut h = Histogram::new(DocumentColor::default());
        h.add(&[
            [0.5, 0., 0., 0.5],
            [-0.25, 0.25, 0.75, 0.5],
            [0., 0., 0., 0.],
        ])
        .unwrap();
        assert_eq!((h.pixels, h.transparent), (2, 1));
        assert_eq!(
            (
                h.channels[0].black,
                h.channels[0].white,
                h.channels[0].below
            ),
            (1, 1, 1)
        );
        assert_eq!(h.channels[2].above, 1);
        // Published sRGB red luminance is 0.2126: Y bin 54, not encoded RGB 255.
        assert_eq!(h.channels[3].bins[54], 1);
        let mut other = Histogram::new(DocumentColor {
            space: RgbSpace::ProPhoto,
            ..Default::default()
        });
        other.add(&[[1., 0., 0., 1.]]).unwrap();
        // ProPhoto red Y is about 0.288: the different document space matters.
        assert_eq!(other.channels[3].bins[73], 1);
        assert!(h.add(&[[f32::NAN, 0., 0., 1.]]).is_err());
        assert!(h.add(&[[0., 0., 0., 1.1]]).is_err());
    }

    #[test]
    fn complete_distributions_match_direct_transfer_with_partial_and_subnormal_alpha() {
        let mut random = 0x3174abcdu32;
        let mut pixels = Vec::new();
        for alpha in [0., f32::from_bits(1), 1. / 65535., 0.1, 0.3, 0.7, 1.] {
            for _ in 0..32768 {
                let mut pixel = [0., 0., 0., alpha];
                for c in &mut pixel[..3] {
                    random ^= random << 13;
                    random ^= random >> 17;
                    random ^= random << 5;
                    *c = ((random % 131072) as f32 / 65535. - 0.25) * alpha;
                }
                pixels.push(pixel);
            }
        }
        for space in RgbSpace::ALL {
            let color = DocumentColor {
                space,
                depth: SampleDepth::U16,
            };
            let mut actual = Histogram::new(color);
            actual.add(&pixels).unwrap();
            let mut expected = Histogram::new(color);
            let coefficients = space.to_xyz()[1];
            for p in &pixels {
                if p[3] == 0. {
                    expected.transparent += 1;
                    continue;
                }
                expected.pixels += 1;
                let rgb = [0, 1, 2].map(|i| f64::from(p[i]) / f64::from(p[3]));
                let y = rgb[1]
                    + coefficients[0] * (rgb[0] - rgb[1])
                    + coefficients[2] * (rgb[2] - rgb[1]);
                for (i, linear) in rgb.into_iter().chain([y]).enumerate() {
                    let encoded = if i < 3 { space.encode(linear) } else { linear };
                    let channel = &mut expected.channels[i];
                    let bin = (encoded.clamp(0., 1.) * 256.).floor().min(255.) as usize;
                    channel.bins[bin] += 1;
                    if linear < 0. {
                        channel.below += 1;
                    }
                    if linear > 1. {
                        channel.above += 1;
                    }
                    if linear <= 0. {
                        channel.black += 1;
                    }
                    if linear >= 1. {
                        channel.white += 1;
                    }
                }
            }
            assert_eq!(actual, expected, "{space:?}");
        }
    }
}

#[cfg(test)]
mod float32_tests {
    use super::*;
    #[test]
    fn float32_histogram_separates_extended_range_and_subnormal_bins() {
        let mut histogram = Histogram::new(DocumentColor { depth: super::super::SampleDepth::F32, ..Default::default() });
        histogram.add(&[[f32::from_bits(1),1e-20,1.,1.],[-1.,100000.,1e30,1.]]).unwrap();
        let plot = histogram.plot_bins();
        for value in [f64::from(f32::from_bits(1)),1e-20,1.,100000.,1e30] { assert!(plot.contains(&histogram.hdr_bin(value))); }
        assert_ne!(histogram.hdr_bin(100000.), histogram.hdr_bin(1e30));
        assert_eq!(histogram.channels[0].below, 1);
    }
}
