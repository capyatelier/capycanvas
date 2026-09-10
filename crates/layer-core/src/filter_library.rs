//! Built-in declarations only. Execution, caching and generated controls do not
//! branch on these IDs. Algorithms are original WGSL, not third-party ports.
use super::*;

pub(super) fn source() -> Arc<str> {
    static SOURCE: std::sync::OnceLock<Arc<str>> = std::sync::OnceLock::new();
    SOURCE
        .get_or_init(|| {
            concat!(
                include_str!("effects.wgsl"),
                "\n",
                include_str!("filter_library.wgsl")
            )
            .into()
        })
        .clone()
}
fn n(
    key: &str,
    label: &str,
    [min, max, value]: [f32; 3],
    step: f32,
    decimals: u8,
    unit: &str,
) -> EffectParameter {
    EffectParameter {
        key: key.into(),
        label: label.into(),
        section: None,
        kind: EffectParameterKind::Number {
            min,
            max,
            step,
            decimals,
            unit: unit.into(),
        },
        default: EffectValue::Number(value),
    }
}
fn percent(key: &str, label: &str, value: f32) -> EffectParameter {
    n(key, label, [0., 100., value], 1., 0, "%")
}
fn px(key: &str, label: &str, max: f32, value: f32) -> EffectParameter {
    n(key, label, [0., max, value], 0.1, 1, "px")
}
fn color(key: &str, label: &str, rgb: [f32; 3]) -> EffectParameter {
    EffectParameter {
        key: key.into(),
        label: label.into(),
        section: None,
        kind: EffectParameterKind::Color,
        default: EffectValue::Color([rgb[0], rgb[1], rgb[2], 1.]),
    }
}
fn toggle(key: &str, label: &str, value: bool) -> EffectParameter {
    EffectParameter {
        key: key.into(),
        label: label.into(),
        section: None,
        kind: EffectParameterKind::Toggle,
        default: EffectValue::Toggle(value),
    }
}
fn angle() -> EffectParameter {
    n("angle", "Angle", [-180., 180., 0.], 1., 0, "°")
}
fn speed(value: f32) -> EffectParameter {
    n("speed", "Speed", [0., 4., value], 0.05, 2, "")
}
fn radius(key: &str, scale: f32, padding: u32) -> EffectSampling {
    EffectSampling::Parameter {
        key: key.into(),
        scale,
        padding,
    }
}
fn image(p: &mut EffectProgram, sampling: EffectSampling, alpha: EffectAlpha) {
    p.passes = Arc::from([EffectPass {
        entry: p.entry.clone(),
        sampling,
    }]);
    p.alpha = alpha;
}

pub(super) fn program(id: BuiltinEffect) -> EffectProgram {
    use BuiltinEffect as B;
    let mut p = EffectProgram {
        abi: EFFECT_ABI,
        id: id.id().into(),
        label: id.label().into(),
        kind: EffectKind::Adjustment,
        alpha: EffectAlpha::Preserve,
        wgsl: source(),
        entry: format!("capy_{}", id.id()).into(),
        passes: Arc::from([]),
        time: false,
        lookups: Arc::from([]),
        parameters: Arc::from([]),
        constraints: Arc::from([]),
    };
    let mut animated = false;
    let parameters = match id {
        B::GaussianBlur | B::UnsharpMask | B::HighPass | B::Bloom | B::SoftFocus | B::Pencil => {
            let sigma = match id {
                B::GaussianBlur => 3.,
                B::UnsharpMask => 1.5,
                B::HighPass => 4.,
                B::Bloom => 6.,
                B::SoftFocus => 5.,
                _ => 2.,
            };
            let mut values = vec![px("sigma", "Radius", 21., sigma)];
            match id {
                B::UnsharpMask => values.extend([
                    n("amount", "Amount", [0., 300., 100.], 1., 0, "%"),
                    percent("threshold", "Threshold", 2.),
                ]),
                B::HighPass => values.push(n("amount", "Strength", [0., 300., 100.], 1., 0, "%")),
                B::Bloom => values.extend([
                    n("amount", "Strength", [0., 200., 60.], 1., 0, "%"),
                    percent("threshold", "Threshold", 60.),
                ]),
                B::SoftFocus => values.push(percent("amount", "Strength", 40.)),
                B::Pencil => values.extend([
                    percent("contrast", "Contrast", 40.),
                    color("ink", "Ink", [0.07, 0.06, 0.05]),
                    color("paper", "Paper", [0.97, 0.95, 0.90]),
                ]),
                _ => {}
            }
            p.lookups = Arc::from([EffectLookup {
                wgsl: include_str!("gaussian-prepare.wgsl").into(),
                entry: "capy_prepare_gaussian".into(),
                dependencies: Arc::from([Arc::from("sigma")]),
                values: 33,
                workgroup_size: [64, 1, 1],
                workgroups: [1, 1, 1],
            }]);
            p.passes = Arc::from([
                EffectPass {
                    entry: if id == B::Bloom {
                        "capy_bloom_h"
                    } else {
                        "capy_blur_h"
                    }
                    .into(),
                    sampling: radius("sigma", 3., 0),
                },
                EffectPass {
                    entry: p.entry.clone(),
                    sampling: radius("sigma", 3., 0),
                },
            ]);
            if matches!(id, B::GaussianBlur | B::Bloom) {
                p.alpha = EffectAlpha::Filter;
            }
            values
        }
        B::MotionBlur => {
            image(&mut p, radius("distance", 0.5, 1), EffectAlpha::Filter);
            vec![px("distance", "Distance", 64., 12.), angle()]
        }
        B::Denoise => {
            image(&mut p, radius("radius", 1., 0), EffectAlpha::Preserve);
            vec![
                n("radius", "Radius", [1., 3., 2.], 1., 0, "px"),
                percent("strength", "Strength", 25.),
            ]
        }
        B::EdgeDetect => {
            image(&mut p, radius("radius", 1., 1), EffectAlpha::Preserve);
            vec![
                n("radius", "Width", [0.5, 8., 1.], 0.1, 1, "px"),
                n("strength", "Strength", [0., 400., 100.], 1., 0, "%"),
                toggle("invert", "Invert", false),
            ]
        }
        B::WhiteBalance => vec![
            n("temperature", "Temperature", [-100., 100., 0.], 1., 0, "")
                .in_section("White balance"),
            n("tint", "Tint", [-100., 100., 0.], 1., 0, ""),
            toggle("preserve_luminance", "Preserve luminosity", true),
        ],
        B::SplitTone => vec![
            color("shadows", "Shadows", [0.16, 0.33, 0.58]),
            color("highlights", "Highlights", [0.96, 0.68, 0.34]),
            n("balance", "Balance", [-100., 100., 0.], 1., 0, ""),
            percent("strength", "Strength", 30.),
        ],
        B::Vignette => vec![
            percent("strength", "Strength", 40.),
            n("radius", "Radius", [10., 150., 95.], 1., 0, "%"),
            percent("softness", "Softness", 55.),
            percent("center_x", "Center X", 50.).in_section("Position"),
            percent("center_y", "Center Y", 50.),
        ],
        B::FilmGrain => {
            animated = true;
            vec![
                percent("amount", "Amount", 18.),
                n("size", "Size", [0.5, 8., 1.], 0.1, 1, "px"),
                toggle("color", "Color grain", false),
                speed(1.),
            ]
        }
        B::Halftone => {
            image(&mut p, radius("size", 2., 1), EffectAlpha::Preserve);
            let mut a = angle();
            a.default = EffectValue::Number(15.);
            vec![
                n("size", "Dot spacing", [3., 48., 9.], 0.5, 1, "px"),
                a,
                percent("contrast", "Contrast", 30.),
                color("ink", "Ink", [0.05, 0.07, 0.09]),
                color("paper", "Paper", [0.96, 0.94, 0.87]),
            ]
        }
        B::Crosshatch => vec![
            n("spacing", "Spacing", [3., 32., 8.], 0.5, 1, "px"),
            n("width", "Line width", [0.25, 4., 1.], 0.1, 1, "px"),
            angle(),
            color("ink", "Ink", [0.07, 0.08, 0.09]),
            color("paper", "Paper", [0.97, 0.95, 0.91]),
        ],
        B::Emboss => {
            image(&mut p, radius("radius", 1., 1), EffectAlpha::Preserve);
            let mut a = angle();
            a.default = EffectValue::Number(135.);
            vec![
                n("radius", "Width", [0.5, 8., 1.5], 0.1, 1, "px"),
                a,
                n("strength", "Depth", [0., 400., 100.], 1., 0, "%"),
            ]
        }
        B::PixelMosaic => {
            image(&mut p, radius("size", 0.5, 1), EffectAlpha::Filter);
            vec![n("size", "Cell size", [1., 96., 12.], 1., 0, "px")]
        }
        B::ChromaticAberration => {
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![px("distance", "Separation", 32., 3.), angle()]
        }
        B::Painterly => {
            image(&mut p, radius("radius", 1., 1), EffectAlpha::Preserve);
            vec![
                n("radius", "Radius", [1., 16., 5.], 0.5, 1, "px"),
                percent("strength", "Strength", 100.),
            ]
        }
        B::Solarize => vec![
            percent("threshold", "Threshold", 50.),
            percent("strength", "Strength", 100.),
        ],
        B::Kaleidoscope => {
            image(&mut p, EffectSampling::Document, EffectAlpha::Filter);
            vec![
                n("segments", "Segments", [2., 24., 6.], 1., 0, ""),
                angle(),
                percent("center_x", "Center X", 50.).in_section("Position"),
                percent("center_y", "Center Y", 50.),
            ]
        }
        B::Swirl => {
            image(&mut p, EffectSampling::Document, EffectAlpha::Filter);
            vec![
                n("turn", "Twist", [-720., 720., 120.], 1., 0, "°"),
                n("radius", "Radius", [1., 150., 70.], 1., 0, "%"),
                percent("center_x", "Center X", 50.).in_section("Position"),
                percent("center_y", "Center Y", 50.),
            ]
        }
        B::Ripple => {
            animated = true;
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![
                px("distance", "Amplitude", 48., 12.),
                n("wavelength", "Wavelength", [8., 256., 64.], 1., 0, "px"),
                speed(0.5),
                percent("center_x", "Center X", 50.).in_section("Position"),
                percent("center_y", "Center Y", 50.),
            ]
        }
        B::Glass => {
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![
                px("distance", "Distortion", 48., 12.),
                n("scale", "Texture size", [4., 160., 24.], 1., 0, "px"),
                percent("roughness", "Roughness", 35.),
            ]
        }
        B::RainyGlass => {
            animated = true;
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![
                px("distance", "Refraction", 32., 8.),
                n("scale", "Drop size", [12., 120., 48.], 1., 0, "px"),
                percent("amount", "Rain", 65.),
                speed(0.5),
            ]
        }
        B::Vhs => {
            animated = true;
            image(&mut p, radius("distance", 1.5, 1), EffectAlpha::Filter);
            vec![
                px("distance", "Tracking", 32., 5.),
                percent("noise", "Noise", 12.),
                percent("lines", "Scanlines", 20.),
                speed(1.),
            ]
        }
        B::Crt => {
            animated = true;
            image(&mut p, EffectSampling::Document, EffectAlpha::Filter);
            vec![
                n("curvature", "Curvature", [0., 30., 8.], 1., 0, "%"),
                percent("lines", "Scanlines", 35.),
                percent("mask", "Pixel mask", 25.),
                px("separation", "Separation", 5., 1.),
            ]
        }
        B::HeatHaze => {
            animated = true;
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![
                px("distance", "Distortion", 48., 8.),
                n("scale", "Wave size", [10., 240., 90.], 1., 0, "px"),
                speed(0.6),
                percent("detail", "Detail", 50.),
            ]
        }
        B::Iridescence => {
            animated = true;
            vec![
                percent("strength", "Strength", 55.),
                n("scale", "Film size", [8., 240., 64.], 1., 0, "px"),
                speed(0.3),
            ]
        }
        B::DomainWarp => {
            animated = true;
            image(&mut p, radius("distance", 1., 1), EffectAlpha::Filter);
            vec![
                px("distance", "Distortion", 64., 24.),
                n("scale", "Pattern size", [8., 256., 96.], 1., 0, "px"),
                n("octaves", "Detail", [1., 5., 3.], 1., 0, ""),
                speed(0.25),
            ]
        }
        _ => unreachable!("tone adjustments are declared in effects.rs"),
    };
    p.parameters = parameters.into();
    if animated {
        p = p.with_time_controls();
    }
    p
}
