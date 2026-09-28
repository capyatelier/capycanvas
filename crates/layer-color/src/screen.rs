pub mod edid;

use layer_core::color::RgbSpace;
use layer_core::color::rgb::{self, Matrix3};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Chromaticities {
    pub primaries: [[f64; 2]; 3],
    pub white: [f64; 2],
}

impl Chromaticities {
    pub const BT2020: Self = Self {
        primaries: [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        white: [0.3127, 0.3290],
    };

    pub fn of(space: RgbSpace) -> Self {
        Self { primaries: space.primaries(), white: space.white() }
    }

    pub fn near(self, other: Self, tolerance: f64) -> bool {
        let points = |c: Self| [c.primaries[0], c.primaries[1], c.primaries[2], c.white];
        points(self)
            .iter()
            .zip(points(other))
            .all(|(a, b)| (a[0] - b[0]).abs() <= tolerance && (a[1] - b[1]).abs() <= tolerance)
    }

    pub fn plausible(self) -> bool {
        let valid = |[x, y]: [f64; 2]| x > 0. && y > 0. && x + y < 1.;
        let [wx, wy] = self.white;
        self.primaries.into_iter().all(valid)
            && valid(self.white)
            && (wx - 0.3127).abs() < 0.03
            && (wy - 0.3290).abs() < 0.03
            && area(&self.primaries) > 0.5 * area(&RgbSpace::Srgb.primaries())
            && inside(&counterclockwise(self.primaries), self.white)
    }

    pub fn coverage(self, target: Self) -> f64 {
        let target_area = area(&target.primaries);
        if target_area <= 0. {
            return 0.;
        }
        let edges = counterclockwise(self.primaries);
        let mut polygon = target.primaries.to_vec();
        for i in 0..3 {
            polygon = clip(&polygon, edges[i], edges[(i + 1) % 3]);
        }
        (area(&polygon) / target_area).min(1.)
    }

    pub fn from_space(self, space: RgbSpace) -> Matrix3 {
        rgb::multiply(
            rgb::inverse(rgb::primaries_to_xyz(self.primaries, self.white)),
            rgb::multiply(rgb::bradford(space.white(), self.white), space.to_xyz()),
        )
    }
}

fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

fn area(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n).map(|i| cross([0., 0.], points[i], points[(i + 1) % n])).sum::<f64>().abs() / 2.
}

fn counterclockwise(mut triangle: [[f64; 2]; 3]) -> [[f64; 2]; 3] {
    if cross(triangle[0], triangle[1], triangle[2]) < 0. {
        triangle.swap(1, 2);
    }
    triangle
}

fn inside(triangle: &[[f64; 2]; 3], point: [f64; 2]) -> bool {
    (0..3).all(|i| cross(triangle[i], triangle[(i + 1) % 3], point) >= 0.)
}

fn clip(polygon: &[[f64; 2]], a: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    let mut kept = Vec::with_capacity(polygon.len() + 1);
    for (i, &current) in polygon.iter().enumerate() {
        let previous = polygon[(i + polygon.len() - 1) % polygon.len()];
        let (c, p) = (cross(a, b, current), cross(a, b, previous));
        if (c >= 0.) != (p >= 0.) {
            let t = p / (p - c);
            kept.push([
                previous[0] + t * (current[0] - previous[0]),
                previous[1] + t * (current[1] - previous[1]),
            ]);
        }
        if c >= 0. {
            kept.push(current);
        }
    }
    kept
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub enum Transfer {
    Srgb,
    Gamma(f32),
    Bt1886,
    Pq,
    Hlg,
    Linear,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct CompositorDescription {
    pub primaries: Chromaticities,
    pub target: Chromaticities,
    pub transfer: Transfer,
    pub reference_white: f32,
    pub target_peak: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub enum ScreenColor {
    #[default]
    Pending,
    Unmanaged,
    Unreported,
    Described(CompositorDescription),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScreenReport {
    pub name: Option<String>,
    pub color: ScreenColor,
    pub monitor: Option<edid::Edid>,
    pub hdr_capable: Option<bool>,
}

impl ScreenReport {
    pub fn managed(
        name: Option<String>,
        gamut: Chromaticities,
        hdr_on: bool,
        hdr_peak: Option<f32>,
        hdr_capable: Option<bool>,
    ) -> Self {
        Self {
            name,
            color: ScreenColor::Described(CompositorDescription {
                primaries: gamut,
                target: gamut,
                transfer: if hdr_on { Transfer::Pq } else { Transfer::Srgb },
                reference_white: ARTWORK_WHITE,
                target_peak: if hdr_on {
                    hdr_peak.filter(|peak| peak.is_finite() && *peak > ARTWORK_WHITE)
                } else {
                    Some(ARTWORK_WHITE)
                },
            }),
            monitor: None,
            hdr_capable,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Basis {
    #[default]
    Pending,
    System,
    Monitor,
    Unknown,
    Unmanaged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct ScreenAssessment {
    pub gamut: Option<Chromaticities>,
    pub basis: Basis,
    pub reference_white: Option<f32>,
    pub peak: Option<f32>,
    pub signal_peak: Option<f32>,
    pub hdr_signal: bool,
    pub srgb_on_wide_monitor: bool,
}

const PQ_SIGNAL_PEAK: f32 = 10_000.;
const ARTWORK_WHITE: f32 = 203.;

impl ScreenAssessment {
    pub fn headroom(&self) -> f32 {
        let Some(white) = self.reference_white.filter(|w| *w > 0.) else { return 1. };
        self.peak
            .or(self.signal_peak)
            .map_or(1., |peak| (peak / white).clamp(1., PQ_SIGNAL_PEAK / ARTWORK_WHITE))
    }

    pub fn white_at_peak(&self) -> bool {
        self.hdr_signal && matches!((self.peak, self.reference_white), (Some(peak), Some(white)) if peak <= white * 1.02)
    }

    pub fn approximate(&self) -> bool {
        self.basis != Basis::System || self.srgb_on_wide_monitor || self.white_at_peak()
    }

    pub fn from_view(&self, view: RgbSpace) -> Option<Matrix3> {
        self.gamut.map(|gamut| gamut.from_space(view))
    }
}

pub fn assess(report: &ScreenReport) -> ScreenAssessment {
    let description = match report.color {
        ScreenColor::Pending => return ScreenAssessment::default(),
        ScreenColor::Unmanaged => return ScreenAssessment { basis: Basis::Unmanaged, ..Default::default() },
        ScreenColor::Unreported => return ScreenAssessment { basis: Basis::Unknown, ..Default::default() },
        ScreenColor::Described(description) => description,
    };
    let monitor = report.monitor.as_ref().filter(|m| m.chromaticities.plausible());
    let srgb = Chromaticities::of(RgbSpace::Srgb);
    let signal_only = description.transfer == Transfer::Pq
        && description.target.near(Chromaticities::BT2020, 0.002)
        && description.target_peak.is_some_and(|peak| peak >= PQ_SIGNAL_PEAK);
    let (gamut, basis) = if signal_only {
        match monitor.filter(|m| !(m.bt2020_signal && m.chromaticities.near(srgb, 0.004))) {
            Some(monitor) => (Some(monitor.chromaticities), Basis::Monitor),
            None => (None, Basis::Unknown),
        }
    } else {
        (Some(description.target), Basis::System)
    };
    let peak = if signal_only { monitor.and_then(|m| m.max_luminance) } else { description.target_peak };
    let wide_monitor = monitor.is_some_and(|m| m.chromaticities.coverage(Chromaticities::of(RgbSpace::DisplayP3)) > 0.85);
    ScreenAssessment {
        gamut,
        basis,
        reference_white: Some(description.reference_white),
        peak,
        signal_peak: description.target_peak,
        hdr_signal: matches!(description.transfer, Transfer::Pq | Transfer::Hlg)
            || description.target_peak.is_some_and(|peak| peak > description.reference_white),
        srgb_on_wide_monitor: basis == Basis::System && description.target.near(srgb, 0.004) && wide_monitor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(file: &[u8]) -> Option<edid::Edid> {
        Some(edid::parse(file).unwrap())
    }
    fn cintiq() -> Option<edid::Edid> {
        monitor(include_bytes!("../tests/fixtures/edid/cintiq-pro-27.bin"))
    }
    fn television() -> Option<edid::Edid> {
        monitor(include_bytes!("../tests/fixtures/edid/lg-tv.bin"))
    }
    fn described(target: Chromaticities, transfer: Transfer, white: f32, peak: f32) -> ScreenColor {
        ScreenColor::Described(CompositorDescription {
            primaries: target,
            target,
            transfer,
            reference_white: white,
            target_peak: Some(peak),
        })
    }
    fn gnome_hdr() -> ScreenColor {
        described(Chromaticities::BT2020, Transfer::Pq, 416., 10_000.)
    }
    fn gnome_default() -> ScreenColor {
        described(Chromaticities::of(RgbSpace::Srgb), Transfer::Gamma(2.2), 80., 80.)
    }

    #[test]
    fn coverage_of_nested_and_overlapping_gamuts() {
        let srgb = Chromaticities::of(RgbSpace::Srgb);
        let p3 = Chromaticities::of(RgbSpace::DisplayP3);
        assert!((p3.coverage(srgb) - 1.).abs() < 1e-9);
        assert!((srgb.coverage(srgb) - 1.).abs() < 1e-9);
        let partial = srgb.coverage(p3);
        assert!(partial > 0.7 && partial < 0.8, "{partial}");
        assert!(Chromaticities::BT2020.coverage(Chromaticities::of(RgbSpace::AdobeRgb)) > 0.999);
    }

    #[test]
    fn screen_conversion_keeps_white_and_flags_outside_colors() {
        let srgb = Chromaticities::of(RgbSpace::Srgb);
        let m = srgb.from_space(RgbSpace::DisplayP3);
        let white = rgb::apply(m, [1., 1., 1.]);
        assert!(white.iter().all(|v| (v - 1.).abs() < 1e-6));
        let green = rgb::apply(m, [0., 1., 0.]);
        assert!(green[0] < -0.2);
    }

    #[test]
    fn placeholder_chromaticities_are_rejected() {
        let blank = Chromaticities { primaries: [[0., 0.]; 3], white: [0., 0.] };
        assert!(!blank.plausible());
        assert!(Chromaticities::of(RgbSpace::Srgb).plausible());
        assert!(cintiq().unwrap().chromaticities.plausible());
    }

    #[test]
    fn hdr_signal_uses_the_monitors_gamut_and_peak() {
        let a = assess(&ScreenReport { color: gnome_hdr(), monitor: cintiq(), ..Default::default() });
        assert_eq!(a.basis, Basis::Monitor);
        assert_eq!(a.gamut, Some(cintiq().unwrap().chromaticities));
        assert_eq!(a.peak, Some(400.));
        assert!(a.white_at_peak() && a.approximate());
        assert_eq!(a.headroom(), 1.);
        assert!(!a.srgb_on_wide_monitor);
    }

    #[test]
    fn hdr_signal_without_monitor_data_leaves_gamut_and_peak_unknown() {
        for monitor in [None, television()] {
            let a = assess(&ScreenReport { color: gnome_hdr(), monitor, ..Default::default() });
            assert_eq!((a.basis, a.gamut, a.peak), (Basis::Unknown, None, None));
            assert!((a.headroom() - 10_000. / 416.).abs() < 1e-3);
        }
    }

    #[test]
    fn system_srgb_on_a_wide_gamut_monitor_is_flagged() {
        let a = assess(&ScreenReport { color: gnome_default(), monitor: cintiq(), ..Default::default() });
        assert_eq!((a.basis, a.gamut), (Basis::System, Some(Chromaticities::of(RgbSpace::Srgb))));
        assert!(a.srgb_on_wide_monitor && a.approximate());
        assert_eq!(a.headroom(), 1.);
        let tv = assess(&ScreenReport { color: gnome_default(), monitor: television(), ..Default::default() });
        assert!(!tv.srgb_on_wide_monitor && !tv.approximate());
    }

    #[test]
    fn native_primaries_from_the_system_are_trusted() {
        let native = cintiq().unwrap().chromaticities;
        let color = described(native, Transfer::Gamma(2.2), 80., 80.);
        let a = assess(&ScreenReport { color, monitor: cintiq(), ..Default::default() });
        assert_eq!((a.basis, a.gamut), (Basis::System, Some(native)));
        assert!(!a.approximate());
    }

    #[test]
    fn managed_platforms_describe_gamut_and_an_optional_hdr_peak() {
        let p3 = Chromaticities::of(RgbSpace::DisplayP3);
        let sdr = assess(&ScreenReport::managed(None, p3, false, Some(1000.), Some(true)));
        assert_eq!((sdr.basis, sdr.gamut, sdr.hdr_signal), (Basis::System, Some(p3), false));
        assert_eq!(sdr.headroom(), 1.);
        let hdr = assess(&ScreenReport::managed(None, p3, true, None, Some(true)));
        assert_eq!((hdr.basis, hdr.peak, hdr.hdr_signal), (Basis::System, None, true));
        assert!(!hdr.approximate());
        let measured = assess(&ScreenReport::managed(None, p3, true, Some(1000.), Some(true)));
        assert_eq!(measured.peak, Some(1000.));
        assert!((measured.headroom() - 1000. / 203.).abs() < 1e-4);
        for implausible in [0., 150., f32::NAN] {
            assert_eq!(assess(&ScreenReport::managed(None, p3, true, Some(implausible), Some(true))).peak, None);
        }
    }

    #[test]
    fn unmanaged_and_unreported_screens_have_no_gamut() {
        for (color, basis) in [(ScreenColor::Unmanaged, Basis::Unmanaged), (ScreenColor::Unreported, Basis::Unknown)] {
            let a = assess(&ScreenReport { color, monitor: cintiq(), ..Default::default() });
            assert_eq!((a.basis, a.gamut), (basis, None));
            assert!(a.approximate());
        }
    }
}
