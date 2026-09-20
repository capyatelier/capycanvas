use super::{RawImage, container::Result, properties::Color};
use layer_core::color::{ColorProfile, RgbSpace};

pub(super) fn profile(color: Color, icc: Option<&[u8]>) -> Result<(ColorProfile, bool)> {
    if matches!(color.cicp[1], 16 | 18) {
        return Err("HDR HEIF/AVIF needs an explicit SDR conversion before import".into());
    }
    if let Some(icc) = icc {
        return Ok((ColorProfile::Icc(icc.into()), false));
    }
    let [primaries, transfer, _] = color.cicp;
    if primaries == 2 || transfer == 2 {
        return Ok((ColorProfile::default(), true));
    }
    Ok((
        match (primaries, transfer) {
            (1, 13) => ColorProfile::Builtin(RgbSpace::Srgb),
            (12, 13) => ColorProfile::Builtin(RgbSpace::DisplayP3),
            _ => crate::icc::nclx_profile(chromaticities(primaries)?, transfer as u32)?,
        },
        false,
    ))
}
fn chromaticities(primaries: u16) -> Result<[f32; 8]> {
    // ITU-T H.273 Table 2, ordered R/G/B/white xy, as used by the ICC builder.
    Ok(match primaries {
        1 => [0.64, 0.33, 0.30, 0.60, 0.15, 0.06, 0.3127, 0.3290],
        4 => [0.67, 0.33, 0.21, 0.71, 0.14, 0.08, 0.310, 0.316],
        5 => [0.64, 0.33, 0.29, 0.60, 0.15, 0.06, 0.3127, 0.3290],
        6 | 7 => [0.630, 0.340, 0.310, 0.595, 0.155, 0.070, 0.3127, 0.3290],
        8 => [0.681, 0.319, 0.243, 0.692, 0.145, 0.049, 0.310, 0.316],
        9 => [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290],
        11 => [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.314, 0.351],
        12 => [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.3127, 0.3290],
        22 => [0.630, 0.340, 0.295, 0.605, 0.155, 0.077, 0.3127, 0.3290],
        _ => return Err("Unsupported HEIF/AVIF color primaries".into()),
    })
}
pub(super) struct Converter {
    color: Color,
    kr: f32,
    kb: f32,
}
impl Converter {
    pub fn new(color: Color, layout: u32) -> Result<Self> {
        let (kr, kb) = match color.cicp[2] {
            0 if layout == 0 || layout == 3 => (0., 0.),
            1 => (0.2126, 0.0722),
            2 | 5 | 6 => (0.299, 0.114),
            4 => (0.30, 0.11),
            7 => (0.212, 0.087),
            8 if color.full_range => (0., 0.),
            9 => (0.2627, 0.0593),
            _ => return Err("Unsupported AVIF YUV matrix".into()),
        };
        Ok(Self { color, kr, kb })
    }
    pub fn pixel(&self, p: &RawImage, x: u32, y: u32) -> [u16; 4] {
        self.pixel_at_depth(p, x, y, p.depth)
    }
    pub fn pixel_at_depth(&self, p: &RawImage, x: u32, y: u32, output_depth: u8) -> [u16; 4] {
        let max = (1u32 << p.depth) - 1;
        let shift = p.depth - 8;
        let bias = if self.color.full_range {
            0.
        } else {
            (16 << shift) as f32
        };
        let range_y = if self.color.full_range {
            max
        } else {
            219 << shift
        } as f32;
        let range_uv = if self.color.full_range {
            max
        } else {
            224 << shift
        } as f32;
        let identity = self.color.cicp[2] == 0;
        let luma = (p.sample(0, x, y) as f32 - bias) / range_y;
        let chroma = |plane| {
            let [w, h] = p.plane_extent(plane);
            let sub_x = matches!(p.layout, 1 | 2);
            let sub_y = p.layout == 1;
            let (xx, yy) = (if sub_x { x / 2 } else { x }, if sub_y { y / 2 } else { y });
            let adj = |pos: u32, pixel: u32, full: u32, sub: bool| {
                if !sub {
                    pos
                } else if pixel % 2 == 0 {
                    pos.saturating_sub(1)
                } else {
                    (pos + 1).min(full - 1)
                }
            };
            let (ax, ay) = (adj(xx, x, w, sub_x), adj(yy, y, h, sub_y));
            let sample = |x, y| {
                let v = p.sample(plane, x, y) as f32;
                if identity {
                    (v - bias) / range_y
                } else {
                    (v - (1u32 << (p.depth - 1)) as f32) / range_uv
                }
            };
            if p.chroma_location != 1 {
                // HEVC locations 0–5 describe left/center horizontally and
                // center/top/bottom vertically. AVIF retains its established
                // centered path below; a grid is joined before interpolation.
                let [dx, dy] = match p.chroma_location {
                    0 => [0., 0.5],
                    2 => [0., 0.],
                    3 => [0.5, 0.],
                    4 => [0., 1.],
                    5 => [0.5, 1.],
                    _ => unreachable!(),
                };
                let axis = |pixel: u32, size: u32, sub: bool, offset: f32| {
                    let v = if sub {
                        (pixel as f32 - offset) / 2.
                    } else {
                        pixel as f32
                    };
                    let v = v.clamp(0., (size - 1) as f32);
                    (v as u32, (v as u32 + 1).min(size - 1), v - v.floor())
                };
                let (x0, x1, fx) = axis(x, w, sub_x, dx);
                let (y0, y1, fy) = axis(y, h, sub_y, dy);
                return (sample(x0, y0) * (1. - fx) + sample(x1, y0) * fx) * (1. - fy)
                    + (sample(x0, y1) * (1. - fx) + sample(x1, y1) * fx) * fy;
            }
            // Center-sited bilinear upsampling, matching the existing portable
            // import policy; duplicate edge samples give the correct weights.
            sample(xx, yy) * (9. / 16.)
                + sample(ax, yy) * (3. / 16.)
                + sample(xx, ay) * (3. / 16.)
                + sample(ax, ay) * (1. / 16.)
        };
        let rgb = if p.layout == 0 {
            [luma; 3]
        } else {
            let (cb, cr) = (chroma(1), chroma(2));
            if identity {
                [cr, luma, cb]
            } else if self.color.cicp[2] == 8 {
                [luma - cb + cr, luma + cb, luma - cb - cr]
            } else {
                [
                    luma + 2. * (1. - self.kr) * cr,
                    luma - 2. * (self.kr * (1. - self.kr) * cr + self.kb * (1. - self.kb) * cb)
                        / (1. - self.kr - self.kb),
                    luma + 2. * (1. - self.kb) * cb,
                ]
            }
        };
        let max = (1u32 << output_depth) - 1;
        [
            (rgb[0].clamp(0., 1.) * max as f32).round() as u16,
            (rgb[1].clamp(0., 1.) * max as f32).round() as u16,
            (rgb[2].clamp(0., 1.) * max as f32).round() as u16,
            max as u16,
        ]
    }
}
