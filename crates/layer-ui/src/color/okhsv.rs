//! Okhsv ↔ display-encoded sRGB, adapted from Björn Ottosson's reference.
//! https://bottosson.github.io/posts/colorpicker/ (MIT, copyright 2021).
//! See THIRD_PARTY_NOTICES.md. Internal f64 arithmetic keeps gamut-edge and
//! near-neutral conversions stable; the UI boundary uses degrees / percent.

fn linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encoded(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
fn to_lab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}
/// CSS OKLCH units: lightness percent, unscaled chroma, hue degrees.
/// Neutrals retain the picker's hue instead of exposing matrix roundoff.
/// https://www.w3.org/TR/css-color-4/#oklch
pub(super) fn to_oklch(rgb: [f32; 3], previous_hue: f32) -> [f32; 3] {
    let [l, a, b] = to_lab(rgb.map(|v| linear(v as f64)));
    let chroma = a.hypot(b);
    let (chroma, hue) = if chroma <= 0.000004 {
        (0., previous_hue as f64)
    } else {
        (chroma, b.atan2(a).to_degrees().rem_euclid(360.))
    };
    [(l * 100.).clamp(0., 100.) as f32, chroma as f32, hue as f32]
}
fn from_lab([l, a, b]: [f64; 3]) -> [f64; 3] {
    let ll = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let mm = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let ss = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    [
        4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss,
        -1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss,
        -0.0041960863 * ll - 0.7034186147 * mm + 1.7076147010 * ss,
    ]
}
fn toe(x: f64) -> f64 {
    let k = 1.206 / 1.03;
    let t = k * x - 0.206;
    0.5 * (t + (t * t + 0.12 * k * x).sqrt())
}
fn toe_inv(x: f64) -> f64 {
    (x * x + 0.206 * x) / ((1.206 / 1.03) * (x + 0.03))
}
fn max_saturation(a: f64, b: f64) -> f64 {
    let (k, weights) = if -1.88170328 * a - 0.80936493 * b > 1. {
        (
            [1.19086277, 1.76576728, 0.59662641, 0.75515197, 0.56771245],
            [4.0767416621, -3.3077115913, 0.2309699292],
        )
    } else if 1.81444104 * a - 1.19445276 * b > 1. {
        (
            [0.73956515, -0.45954404, 0.08285427, 0.12541070, 0.14503204],
            [-1.2684380046, 2.6097574011, -0.3413193965],
        )
    } else {
        (
            [
                1.35733652,
                -0.00915799,
                -1.15130210,
                -0.50559606,
                0.00692167,
            ],
            [-0.0041960863, -0.7034186147, 1.7076147010],
        )
    };
    let mut s = k[0] + k[1] * a + k[2] * b + k[3] * a * a + k[4] * a * b;
    let slopes = [
        0.3963377774 * a + 0.2158037573 * b,
        -0.1055613458 * a - 0.0638541728 * b,
        -0.0894841775 * a - 1.2914855480 * b,
    ];
    // The reference's single Halley step leaves a larger error around blue.
    // Three iterations make the gamut boundary accurate without per-pixel work.
    for _ in 0..3 {
        let mut f = 0.;
        let mut f1 = 0.;
        let mut f2 = 0.;
        for (k, w) in slopes.into_iter().zip(weights) {
            let t = 1. + s * k;
            f += w * t.powi(3);
            f1 += w * 3. * k * t * t;
            f2 += w * 6. * k * k * t;
        }
        s -= f * f1 / (f1 * f1 - 0.5 * f * f2);
    }
    s
}
/// Hue-only gamut terms are reused across the entire field raster.
pub(super) struct Hue {
    a: f64,
    b: f64,
    s_max: f64,
    t_max: f64,
}
impl Hue {
    pub(super) fn new(degrees: f32) -> Self {
        let angle = (degrees as f64).to_radians();
        Self::from_direction(angle.cos(), angle.sin())
    }
    fn from_direction(a: f64, b: f64) -> Self {
        let s_max = max_saturation(a, b);
        let rgb = from_lab([1., s_max * a, s_max * b]);
        let l = (1. / rgb.into_iter().fold(0., f64::max)).cbrt();
        Self {
            a,
            b,
            s_max,
            t_max: l * s_max / (1. - l),
        }
    }
    #[inline]
    fn linear_rgb(&self, saturation: f32, value: f32) -> [f64; 3] {
        if value <= 0. {
            return [0.; 3];
        }
        let [lv, r, g, b] = self.saturation_curve(saturation as f64);
        let factor = toe_inv(value as f64 * lv).powi(3);
        [r * factor, g * factor, b * factor]
    }
    fn saturation_curve(&self, s: f64) -> [f64; 4] {
        if s <= 0. {
            return [1.; 4];
        }
        let k = 1. - 0.5 / self.s_max;
        let divisor = 0.5 + self.t_max - self.t_max * k * s;
        let lv = 1. - s * 0.5 / divisor;
        let cv = s * self.t_max * 0.5 / divisor;
        // Oklab → linear RGB is homogeneous of degree three. The reference
        // takes a cube root for gamut normalization, then cubes that scale in
        // the inverse matrix. Cancel those operations algebraically: one matrix
        // evaluation and no per-pixel cube root (expensive in scalar Wasm).
        let chroma = cv / lv;
        let rgb = from_lab([1., self.a * chroma, self.b * chroma]);
        let maximum = rgb.into_iter().fold(0., f64::max);
        let factor = 1. / (toe_inv(lv).powi(3) * maximum);
        [lv, rgb[0] * factor, rgb[1] * factor, rgb[2] * factor]
    }
    pub(super) fn rgb(&self, saturation: f32, value: f32) -> [f32; 3] {
        self.linear_rgb(saturation, value)
            .map(|v| encoded(v).clamp(0., 1.) as f32)
    }
}

/// Raster-only, smoothly interpolated saturation curve and sRGB transfer table.
/// Gamut normalization depends on hue/saturation, not value: evaluate that curve
/// once per hue, then reuse it across pixels. Picking retains exact conversion.
pub(super) struct RasterHue {
    curve: Vec<[f64; 4]>,
    transfer: &'static [f32; 4097],
}
impl RasterHue {
    pub(super) fn new(degrees: f32) -> Self {
        let hue = Hue::new(degrees);
        static TRANSFER: std::sync::LazyLock<[f32; 4097]> = std::sync::LazyLock::new(|| {
            std::array::from_fn(|i| (encoded(i as f64 / 4096.) * 255.) as f32)
        });
        Self {
            curve: (0..=1024)
                .map(|i| hue.saturation_curve(i as f64 / 1024.))
                .collect(),
            transfer: &TRANSFER,
        }
    }
    pub(super) fn rgb8(&self, saturation: f32, value: f32) -> [u8; 3] {
        let position = saturation as f64 * 1024.;
        let i = (position as usize).min(1023);
        let t = position - i as f64;
        let a = self.curve[i];
        let b = self.curve[i + 1];
        let lv = a[0] + (b[0] - a[0]) * t;
        let factor = toe_inv(value as f64 * lv).powi(3);
        let transfer = self.transfer;
        let encode = |v: f64| {
            let position = (v.clamp(0., 1.) * 4096.) as f32;
            let i = (position as usize).min(4095);
            // Nonnegative values need only add-half then truncate. Rust's
            // general round() otherwise calls libm three times per Wasm pixel.
            (transfer[i] + (transfer[i + 1] - transfer[i]) * (position - i as f32) + 0.5) as u8
        };
        // Explicit channels avoid out-of-line array-map callbacks in Wasm.
        [
            encode((a[1] + (b[1] - a[1]) * t) * factor),
            encode((a[2] + (b[2] - a[2]) * t) * factor),
            encode((a[3] + (b[3] - a[3]) * t) * factor),
        ]
    }
}
pub(super) fn to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    Hue::new(h).rgb(s / 100., v / 100.)
}
pub(super) fn from_rgb(rgb: [f32; 3], previous_hue: f32) -> [f32; 3] {
    let [l, a, b] = to_lab(rgb.map(|v| linear(v as f64)));
    let c = a.hypot(b);
    // Neutral RGB has no hue. Avoid matrix roundoff inventing a hue/saturation.
    if rgb[0] == rgb[1] && rgb[1] == rgb[2] || c < 1e-12 {
        return [previous_hue, 0., (toe(l) * 100.).clamp(0., 100.) as f32];
    }
    let hue = Hue::from_direction(a / c, b / c);
    let t = hue.t_max / (c + l * hue.t_max);
    let lv = t * l;
    let cv = t * c;
    let lvt = toe_inv(lv);
    let cvt = cv * lvt / lv;
    let scale_rgb = from_lab([lvt, hue.a * cvt, hue.b * cvt]);
    let scale = (1. / scale_rgb.into_iter().fold(0., f64::max)).cbrt();
    let v = toe(l / scale) / lv;
    let k = 1. - 0.5 / hue.s_max;
    let s = (0.5 + hue.t_max) * cv / (hue.t_max * 0.5 + hue.t_max * k * cv);
    [
        b.atan2(a).to_degrees().rem_euclid(360.) as f32,
        (s * 100.).clamp(0., 100.) as f32,
        (v * 100.).clamp(0., 100.) as f32,
    ]
}

/// The maximum-saturation envelope jumps near RGB blue in the reference gamut
/// approximation. A hue guide should not display that saturation discontinuity.
/// Join through RGB blue with monotone cubic ramps in this small neighborhood;
/// the actual Okhsv conversion, field and stored coordinates remain unchanged.
pub(super) fn hue_preview(hue: f32) -> [f32; 3] {
    const LEFT: f32 = 258.;
    const BLUE: f32 = 264.05203;
    const RIGHT: f32 = 268.;
    let hue = hue.rem_euclid(360.);
    if !(LEFT..=RIGHT).contains(&hue) {
        return to_rgb([hue, 100., 100.]);
    }
    struct Anchor {
        color: [f32; 3],
        slope: [f32; 3],
    }
    static ENDS: std::sync::LazyLock<[Anchor; 2]> = std::sync::LazyLock::new(|| {
        [LEFT, RIGHT].map(|h| {
            let before = to_rgb([h - 0.01, 100., 100.]);
            let after = to_rgb([h + 0.01, 100., 100.]);
            Anchor {
                color: to_rgb([h, 100., 100.]),
                slope: std::array::from_fn(|i| (after[i] - before[i]) / 0.02),
            }
        })
    });
    let (a, b, slopes, width, t) = if hue <= BLUE {
        (
            ENDS[0].color,
            [0., 0., 1.],
            [ENDS[0].slope, [0.; 3]],
            BLUE - LEFT,
            (hue - LEFT) / (BLUE - LEFT),
        )
    } else {
        (
            [0., 0., 1.],
            ENDS[1].color,
            [[0.; 3], ENDS[1].slope],
            RIGHT - BLUE,
            (hue - BLUE) / (RIGHT - BLUE),
        )
    };
    std::array::from_fn(|i| {
        let delta = b[i] - a[i];
        if delta.abs() < 1e-6 {
            return a[i];
        }
        let m0 = (slopes[0][i] * width / delta).clamp(0., 3.) * delta;
        let m1 = (slopes[1][i] * width / delta).clamp(0., 3.) * delta;
        ((2. * t * t * t - 3. * t * t + 1.) * a[i]
            + (t * t * t - 2. * t * t + t) * m0
            + (-2. * t * t * t + 3. * t * t) * b[i]
            + (t * t * t - t * t) * m1)
            .clamp(0., 1.)
    })
}

/// Adaptive encoded-sRGB stops follow the hue guide, including its blue ramp.
pub(super) fn hue_stops() -> Vec<super::ColorHueStop> {
    use super::ColorHueStop;
    fn stop(hue: f32) -> ColorHueStop {
        ColorHueStop {
            offset: hue / 360.,
            color: hue_preview(hue),
        }
    }
    fn split(a: ColorHueStop, b: ColorHueStop, depth: u8, output: &mut Vec<ColorHueStop>) {
        let error = [0.25, 0.5, 0.75]
            .into_iter()
            .map(|t| {
                let hue = (a.offset + (b.offset - a.offset) * t) * 360.;
                let actual = hue_preview(hue);
                (0..3)
                    .map(|i| (actual[i] - (a.color[i] + (b.color[i] - a.color[i]) * t)).abs())
                    .fold(0., f32::max)
            })
            .fold(0., f32::max);
        let mid = stop((a.offset + b.offset) * 180.);
        if error > 0.25 / 255. && depth < 12 && mid.offset > a.offset && mid.offset < b.offset {
            split(a, mid, depth + 1, output);
            split(mid, b, depth + 1, output);
        } else {
            output.push(b);
        }
    }
    let mut output = vec![stop(0.)];
    for h in 0..360 {
        split(stop(h as f32), stop(h as f32 + 1.), 0, &mut output);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_author_reference_samples() {
        #[derive(serde::Deserialize)]
        struct Case {
            okhsv: [f32; 3],
            rgb: [f32; 3],
        }
        #[derive(serde::Deserialize)]
        struct Reference {
            cases: Vec<Case>,
        }
        let reference: Reference =
            serde_json::from_str(include_str!("okhsv-reference.json")).unwrap();
        for case in reference.cases {
            let actual = to_rgb(case.okhsv);
            // Independently evaluated author JavaScript, with the documented
            // Halley refinement repeated three times (see fixture provenance).
            for (a, b) in actual.into_iter().zip(case.rgb) {
                assert!(
                    (a - b).abs() < 0.00002,
                    "{:?}: {actual:?} != {:?}",
                    case.okhsv,
                    case.rgb
                );
            }
        }
    }

    #[test]
    fn srgb_grid_roundtrips_including_neutrals_and_gamut_edges() {
        for r in 0..=20 {
            for g in 0..=20 {
                for b in 0..=20 {
                    let rgb = [r, g, b].map(|v| v as f32 / 20.);
                    let hsv = from_rgb(rgb, 137.);
                    assert!(hsv.iter().enumerate().all(|(i, v)| v.is_finite()
                        && (0.0..=if i == 0 { 360. } else { 100. }).contains(v)));
                    let actual = to_rgb(hsv);
                    for (a, b) in actual.into_iter().zip(rgb) {
                        assert!((a - b).abs() < 0.00002, "{rgb:?} -> {hsv:?} -> {actual:?}");
                    }
                    if r == g && g == b {
                        assert_eq!([hsv[0], hsv[1]], [137., 0.]);
                    }
                }
            }
        }
    }

    #[test]
    fn interpolated_raster_matches_exact_colors_within_one_byte() {
        for h in (0..360)
            .map(|h| h as f32 + 0.03)
            .chain([264.05, 264.052, 264.053])
        {
            let hue = Hue::new(h);
            let raster = RasterHue::new(h);
            for s in 0..=101 {
                // Off-table saturation samples, including both endpoints;
                // values cover the sensitive near-black transfer region.
                for v in [
                    0., 0.00001, 0.001, 0.01, 0.03, 0.1, 0.25, 0.5, 0.75, 0.9, 1.,
                ] {
                    let exact = hue
                        .rgb(s as f32 / 101., v)
                        .map(|c| (c * 255.).round() as u8);
                    let pixel = raster.rgb8(s as f32 / 101., v);
                    assert!(
                        exact
                            .into_iter()
                            .zip(pixel)
                            .all(|(a, b)| a.abs_diff(b) <= 1),
                        "h={h} s={s}/101 v={v}: {exact:?} != {pixel:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn hue_preview_removes_the_blue_seam_without_changing_conversion() {
        let mut before = hue_preview(257.99);
        for i in 1..=10020 {
            let hue = 257.99 + i as f32 * 0.001;
            let color = hue_preview(hue);
            assert!(
                color
                    .into_iter()
                    .zip(before)
                    .all(|(a, b)| (a - b).abs() < 0.1 / 255.),
                "hue {hue}: {before:?} -> {color:?}"
            );
            before = color;
        }
        assert_eq!(hue_preview(264.05203), [0., 0., 1.]);
        for hue in [0., 120., 240., 257., 269., 300., 359.] {
            assert_eq!(hue_preview(hue), to_rgb([hue, 100., 100.]));
        }
        // The existing canonical conversion (and saved coordinate meaning) is
        // deliberately retained; the ring is a smoothly varying hue guide.
        assert!(to_rgb([264., 100., 100.])[1] > 0.2);
        assert!(hue_preview(264.)[1] < 0.01);
    }
    #[test]
    fn hue_gradient_matches_preview_around_cusps() {
        let stops = hue_stops();
        assert_eq!(stops.first().unwrap().offset, 0.);
        assert_eq!(stops.last().unwrap().offset, 1.);
        assert!(stops.windows(2).all(|s| s[0].offset < s[1].offset));
        let mut interval = 0;
        for i in 0..36000 {
            let h = (i as f32 + 0.31415) * 0.01;
            let offset = h / 360.;
            while stops[interval + 1].offset < offset {
                interval += 1;
            }
            let (a, b) = (stops[interval], stops[interval + 1]);
            let t = (offset - a.offset) / (b.offset - a.offset);
            let actual = hue_preview(h);
            for c in 0..3 {
                let interpolated = a.color[c] + t * (b.color[c] - a.color[c]);
                assert!(
                    (interpolated - actual[c]).abs() < 1. / 255.,
                    "hue {h}: {interpolated} != {}",
                    actual[c]
                );
            }
        }
    }
    #[test]
    fn black_white_and_near_black_are_finite_at_every_hue() {
        for h in 0..360 {
            assert_eq!(to_rgb([h as f32, 0., 100.]), [1.; 3]);
            for s in [0., 1., 50., 100.] {
                assert_eq!(to_rgb([h as f32, s, 0.]), [0.; 3]);
                for v in [0.000001, 0.01, 50., 100.] {
                    assert!(
                        to_rgb([h as f32, s, v])
                            .into_iter()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
                    );
                }
            }
        }
    }
}
