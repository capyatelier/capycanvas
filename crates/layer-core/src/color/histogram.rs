//! Full-resolution document inspection, independent of output/display transforms.
use super::DocumentColor;

pub const BINS: usize = 256;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    fn add(&mut self, value: f64, linear: f64) {
        self.below += u64::from(linear < 0.);
        self.above += u64::from(linear > 1.);
        self.black += u64::from(linear <= 0.);
        self.white += u64::from(linear >= 1.);
        self.bins[(value.clamp(0., 1.) * BINS as f64)
            .floor()
            .min((BINS - 1) as f64) as usize] += 1;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub color: DocumentColor,
    /// Profile-encoded document RGB followed by linear relative luminance Y.
    pub channels: [Channel; 4],
    /// Each nontransparent pixel counts once, regardless of partial coverage.
    pub pixels: u64,
    pub transparent: u64,
    luminance: [f64; 3],
}
impl Histogram {
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
        for pixel in pixels {
            if pixel.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&pixel[3]) {
                return Err("Invalid histogram pixel");
            }
            if pixel[3] == 0. {
                self.transparent += 1;
                continue;
            }
            self.pixels += 1;
            let rgb = [0, 1, 2].map(|c| f64::from(pixel[c]) / f64::from(pixel[3]));
            for c in 0..3 {
                self.channels[c].add(self.color.space.encode(rgb[c]), rgb[c]);
            }
            // Algebraically equal to dot(Y, RGB), with neutral values exact at
            // the endpoints instead of depending on rounded coefficient sums.
            let y = rgb[1]
                + self.luminance[0] * (rgb[0] - rgb[1])
                + self.luminance[2] * (rgb[2] - rgb[1]);
            self.channels[3].add(y, y);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{IntegerDepth, RgbSpace};

    #[test]
    fn native_codes_transparency_and_strip_partition_do_not_change_distributions() {
        for space in RgbSpace::ALL {
            for depth in [IntegerDepth::U8, IntegerDepth::U16] {
                let color = DocumentColor { space, depth };
                let maximum: u32 = if depth == IntegerDepth::U8 {
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
}
