//! Every layer blend mode against an independent f64 reference, through each
//! compositor path that applies a layer's blend, at every document depth.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendRange, Document, EffectInstance, LayerBlend, Project};

const EXTENT: [u32; 2] = [64, 32];
const DIVISOR: f64 = 1. / 16384.;
const BACKDROP: [f32; 4] = [0.25, 0.5, 0.125, 0.75];

type Rgb = [f64; 3];
type Rgba = [f64; 4];

fn lum(c: Rgb, w: Rgb) -> f64 {
    c[0] * w[0] + c[1] * w[1] + c[2] * w[2]
}
fn set_lum(c: Rgb, l: f64, w: Rgb, float: bool) -> Rgb {
    let shift = l - lum(c, w);
    let mut r = c.map(|v| v + shift);
    let low = r.into_iter().fold(f64::INFINITY, f64::min);
    let high = r.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if low < 0. {
        r = r.map(|v| l + (v - l) * l / (l - low).max(1e-6));
    }
    if !float && high > 1. {
        r = r.map(|v| l + (v - l) * (1. - l) / (high - l).max(1e-6));
    }
    r
}
fn sat(c: Rgb) -> f64 {
    c.into_iter().fold(f64::NEG_INFINITY, f64::max) - c.into_iter().fold(f64::INFINITY, f64::min)
}
fn set_sat(c: Rgb, s: f64) -> Rgb {
    let low = c.into_iter().fold(f64::INFINITY, f64::min);
    let high = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if high <= low { [0.; 3] } else { c.map(|v| (v - low) * s / (high - low)) }
}
fn burn(s: f64, d: f64) -> f64 {
    if d >= 1. { 1. } else if s <= 0. { 0. } else { 1. - ((1. - d) / s.max(DIVISOR)).min(1.) }
}
fn dodge(s: f64, d: f64) -> f64 {
    if d <= 0. { 0. } else if s >= 1. { 1. } else { (d / (1. - s).max(DIVISOR)).min(1.) }
}
fn hard_light(s: f64, d: f64) -> f64 {
    if s > 0.5 { 1. - 2. * (1. - s) * (1. - d) } else { 2. * s * d }
}

/// The blend of straight source `s` over straight backdrop `d`.
fn blend(s: Rgb, d: Rgb, mode: LayerBlend, float: bool, w: Rgb) -> Rgb {
    use LayerBlend as B;
    let unit = |c: Rgb| c.map(|v| v.clamp(0., 1.));
    let (s, d) = if mode.range() == BlendRange::Unit { (unit(s), unit(d)) } else { (s, d) };
    let each = |f: &dyn Fn(f64, f64) -> f64| -> Rgb { std::array::from_fn(|i| f(s[i], d[i])) };
    let bounded = |v: f64| if float { v } else { v.clamp(0., 1.) };
    match mode {
        B::Normal => s,
        B::Multiply => each(&|s, d| s * d),
        B::Screen => each(&|s, d| if float { s + d - s.min(1.) * d.min(1.) } else { s + d - s * d }),
        B::Add => each(&|s, d| if float { s + d } else { (s + d).min(1.) }),
        B::Overlay => each(&|s, d| hard_light(d, s)),
        B::SoftLight => each(&|s, d| {
            let curve = if d > 0.25 { d.sqrt() } else { ((16. * d - 12.) * d + 4.) * d };
            if s > 0.5 { d + (2. * s - 1.) * (curve - d) } else { d - (1. - 2. * s) * d * (1. - d) }
        }),
        B::Color => set_lum(s, lum(d, w), w, float),
        B::Darken => each(&f64::min),
        B::Lighten => each(&f64::max),
        B::ColorBurn => each(&burn),
        B::LinearBurn => each(&|s, d| (s + d - 1.).max(0.)),
        B::ColorDodge => each(&dodge),
        B::HardLight => each(&hard_light),
        B::VividLight => each(&|s, d| if s < 0.5 { burn(2. * s, d) } else { dodge(2. * s - 1., d) }),
        B::LinearLight => each(&|s, d| bounded((d + 2. * s - 1.).max(0.))),
        B::PinLight => each(&|s, d| bounded(if s < 0.5 { d.min(2. * s) } else { d.max(2. * s - 1.) })),
        B::HardMix => each(&|s, d| if s + d >= 1. { 1. } else { 0. }),
        B::Difference => each(&|s, d| (d - s).abs()),
        B::Exclusion => each(&|s, d| s + d - 2. * s * d),
        B::Subtract => each(&|s, d| (d - s).max(0.)),
        B::Divide => each(&|s, d| {
            let divided = d / s.max(DIVISOR);
            if float { divided } else if s <= 0. { if d > 0. { 1. } else { 0. } } else { divided.min(1.) }
        }),
        B::Hue => set_lum(set_sat(s, sat(d)), lum(d, w), w, float),
        B::Saturation => set_lum(set_sat(d, sat(s)), lum(d, w), w, float),
        B::Luminosity => set_lum(d, lum(s, w), w, float),
        B::PassThrough => unreachable!("only groups pass through"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Form {
    /// Source over backdrop.
    Composite,
    /// Inside the backdrop's coverage, keeping its alpha.
    Clip,
}
fn straight(p: Rgba) -> Rgb {
    if p[3] > 0. { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [0.; 3] }
}
fn combine(form: Form, s: Rgb, sa: f64, dst: Rgba, b: Rgb) -> Rgba {
    let da = dst[3];
    match form {
        Form::Composite => {
            let rgb: Rgb = std::array::from_fn(|i| (1. - sa) * dst[i] + (1. - da) * s[i] * sa + sa * da * b[i]);
            [rgb[0], rgb[1], rgb[2], sa + da * (1. - sa)]
        }
        Form::Clip => {
            let rgb: Rgb = std::array::from_fn(|i| dst[i] + (b[i] * da - dst[i]) * sa);
            [rgb[0], rgb[1], rgb[2], da]
        }
    }
}
/// The reference result and its range under operand perturbations of a few
/// f32 ulps, which is where branches and divisions are ill-conditioned.
fn expected(form: Form, s: Rgb, sa: f64, dst: Rgba, mode: LayerBlend, float: bool, w: Rgb) -> [Rgba; 2] {
    let d = straight(dst);
    let nudge = |c: Rgb, k: f64| c.map(|v| v + k * (v.abs() * 2e-6 + 1e-7));
    let mut range = [[f64::INFINITY; 4], [f64::NEG_INFINITY; 4]];
    for i in [-1., 0., 1.] {
        for j in [-1., 0., 1.] {
            let result = combine(form, s, sa, dst, blend(nudge(s, i), nudge(d, j), mode, float, w));
            for c in 0..4 {
                range[0][c] = range[0][c].min(result[c]);
                range[1][c] = range[1][c].max(result[c]);
            }
        }
    }
    range
}

fn tolerance(depth: SampleDepth, value: f64) -> f64 {
    match depth {
        SampleDepth::U8 => 1. / 255.,
        SampleDepth::U16 => 1. / 65535.,
        _ => 1e-5 * value.abs().max(1.),
    }
}

/// A layer from straight RGBA, quantized to the document's depth.
fn source(depth: SampleDepth, pixel: impl Fn(u32, u32) -> [f32; 4]) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let mut row = Vec::new();
        for x in 0..EXTENT[0] {
            for v in pixel(x, y) {
                match depth {
                    SampleDepth::U8 => row.push((v.clamp(0., 1.) * 255.).round() as u8),
                    SampleDepth::U16 => row.extend_from_slice(&((v.clamp(0., 1.) * 65535.).round() as u16).to_le_bytes()),
                    SampleDepth::F16 => row.extend_from_slice(&layer_core::color::f16::from_f32(v).to_bits().to_le_bytes()),
                    SampleDepth::F32 => row.extend_from_slice(&v.to_le_bytes()),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

/// Straight probe colors: extended in float documents.
fn scale(depth: SampleDepth) -> f32 {
    if depth.is_float() { 3. } else { 1. }
}
fn top(depth: SampleDepth) -> Arc<SourceImage> {
    let k = scale(depth);
    source(depth, |x, y| {
        let (u, v) = (x as f32 / 63., y as f32 / 31.);
        [(1. - v) * k, ((x * 7 % 64) as f32 / 63.) * k * 0.8, (0.5 + 0.5 * (u - v)) * k, ((y as f32 - 2.) / 24.).clamp(0., 1.)]
    })
}
fn bottom(depth: SampleDepth) -> Arc<SourceImage> {
    let k = scale(depth);
    source(depth, |x, y| {
        let (u, v) = (x as f32 / 63., y as f32 / 31.);
        [u * k, ((y * 5 % 32) as f32 / 31.) * k * 0.9, (1. - 0.5 * (u + v)) * k, ((x % 32) as f32 / 20.).clamp(0., 1.)]
    })
}
/// The straight color the probe filters write at a pixel center.
fn probe(depth: SampleDepth, [x, y]: [u32; 2]) -> Rgb {
    let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
    let k = f64::from(scale(depth)) * 0.9;
    [p[0] / 64. * k, p[1] / 32. * k, (1. - (p[0] + p[1]) / 96.) * k]
}
fn probe_effect(depth: SampleDepth, image: bool) -> Arc<EffectInstance> {
    let mut program = (*fixture("exposure").program()).clone();
    let k = f64::from(scale(depth)) * 0.9;
    let name = if image { "blend_probe_image" } else { "blend_probe" };
    program.id = format!("{name}_{}", depth.bytes()).into();
    program.wgsl = format!(
        "fn {name}(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return vec4<f32>(vec3<f32>(p.x/64.,p.y/32.,1.-(p.x+p.y)/96.)*{k:.3}*c.a,c.a);}}"
    )
    .into();
    program.entry = name.into();
    program.parameters = Arc::new([]);
    program.lookups = Arc::new([]);
    program.constraints = Arc::new([]);
    program.passes = if image {
        Arc::new([layer_core::EffectPass {
            entry: name.into(),
            sampling: layer_core::EffectSampling::Neighborhood { radius: 1 },
        }])
    } else {
        Arc::new([])
    };
    Arc::new(EffectInstance::new(Arc::new(program)))
}

#[derive(Clone, Copy, Debug)]
enum Path {
    /// A layer over the layers below it (scene op 4).
    Layer,
    /// A layer clipped to the base below it.
    Clip,
    /// An adjustment layer over the composite below it.
    Effect,
    /// A clipping stack's final composite over a constant backdrop, folded
    /// into its adjustment.
    Folded,
    /// A clipping stack ending in an image filter, composed from cached images.
    ImageComposition,
}
const OPACITY: f32 = 0.8;

struct Case {
    document: Document,
    background: [f32; 4],
    /// The layer whose blend varies.
    blended: usize,
}
fn case(depth: SampleDepth, path: Path) -> Case {
    let mut document = Document::new("Blend oracle", EXTENT[0], EXTENT[1]);
    document.color = DocumentColor { space: RgbSpace::Srgb, depth };
    let paper = document.layers.pop().unwrap();
    document.layers.clear();
    let paint = |document: &mut Document, name: &str, image| {
        let mut layer = Layer::paint(document.allocate_layer_id(), name);
        layer.source = Some(image);
        layer
    };
    let effect = |document: &mut Document, image: bool| {
        let mut layer = Layer::paint(document.allocate_layer_id(), "Probe");
        layer.kind = LayerKind::Effect;
        layer.effect = Some(probe_effect(depth, image));
        layer
    };
    let mut top_layer = paint(&mut document, "Top", top(depth));
    let mut bottom_layer = paint(&mut document, "Bottom", bottom(depth));
    let (layers, background) = match path {
        Path::Layer => {
            top_layer.opacity = OPACITY;
            (vec![top_layer, bottom_layer], [0.; 4])
        }
        Path::Clip => {
            top_layer.opacity = OPACITY;
            top_layer.properties.clipped = true;
            (vec![top_layer, bottom_layer], [0.; 4])
        }
        Path::Effect => {
            let mut adjustment = effect(&mut document, false);
            adjustment.opacity = OPACITY;
            (vec![adjustment, bottom_layer], [0.; 4])
        }
        Path::Folded => {
            let mut adjustment = effect(&mut document, false);
            adjustment.properties.clipped = true;
            bottom_layer.opacity = OPACITY;
            (vec![adjustment, bottom_layer], BACKDROP)
        }
        Path::ImageComposition => {
            let mut filter = effect(&mut document, true);
            filter.properties.clipped = true;
            bottom_layer.opacity = OPACITY;
            (vec![filter, bottom_layer, top_layer], [0.; 4])
        }
    };
    let blended = match path {
        Path::Layer | Path::Clip | Path::Effect => 0,
        Path::Folded | Path::ImageComposition => 1,
    };
    document.layers = layers;
    let mut paper = paper;
    paper.visible = background[3] > 0.;
    document.layers.push(paper);
    document.active_layer = document.layers[0].id;
    Case { document, background, blended }
}

fn live(r: &mut WgpuRasterizer, layers: &[Layer], background: [f32; 4]) -> Vec<Rgba> {
    let view = ViewState { background_rgba_linear: background, ..crate::test_support::view(EXTENT) };
    r.submit(FramePacket { view, reset_layers: true, ..packet(layers, EXTENT) }).unwrap();
    for _ in 0..16 {
        if !layer_render::CanvasRenderer::has_pending_work(r) {
            break;
        }
        r.submit(FramePacket { view, ..packet(layers, EXTENT) }).unwrap();
    }
    assert!(!layer_render::CanvasRenderer::has_pending_work(r), "the composite settles");
    crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap())
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|c| f64::from(f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap()))))
        .collect()
}
fn exported(r: &WgpuRasterizer, document: &Document, background: [f32; 4]) -> Vec<Rgba> {
    let mut capture = r
        .snapshot_gpu()
        .capture(Project { document: document.clone() }, background, 0., Default::default())
        .unwrap();
    capture
        .read_region([0, 0, EXTENT[0], EXTENT[1]])
        .unwrap()
        .into_iter()
        .map(|p| p.map(f64::from))
        .collect()
}

/// The premultiplied pixels of `layers[index]` drawn alone over nothing.
fn alone(r: &mut WgpuRasterizer, document: &Document, index: usize) -> Vec<Rgba> {
    let mut layers = document.layers.clone();
    for (i, layer) in layers.iter_mut().enumerate() {
        layer.visible = i == index;
        layer.opacity = 1.;
        layer.properties.blend = LayerBlend::Normal;
        layer.properties.clipped = false;
    }
    live(r, &layers, [0.; 4])
}

#[test]
fn every_blend_mode_matches_the_reference_on_every_path_and_depth() {
    let w = RgbSpace::Srgb.to_xyz()[1];
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth })
            .expect("physical GPU required");
        let float = depth.is_float();
        for path in [Path::Layer, Path::Clip, Path::Effect, Path::Folded, Path::ImageComposition] {
            let Case { mut document, background, blended } = case(depth, path);
            let inputs: Vec<_> = (0..document.layers.len() - 1).map(|i| alone(&mut r, &document, i)).collect();
            let backdrop = BACKDROP.map(f64::from);
            let backdrop = [backdrop[0] * backdrop[3], backdrop[1] * backdrop[3], backdrop[2] * backdrop[3], backdrop[3]];
            let opacity = f64::from(OPACITY);
            for mode in LayerBlend::ALL.into_iter().filter(|m| *m != LayerBlend::PassThrough) {
                document.layers[blended].properties.blend = mode;
                let composite = live(&mut r, &document.layers, background);
                let export = exported(&r, &document, background);
                for (i, (actual, export)) in composite.iter().zip(&export).enumerate() {
                    let xy = [i as u32 % EXTENT[0], i as u32 / EXTENT[0]];
                    let [low, high] = match path {
                        Path::Layer => {
                            let t = inputs[0][i];
                            expected(Form::Composite, straight(t), t[3] * opacity, inputs[1][i], mode, float, w)
                        }
                        Path::Clip => {
                            let t = inputs[0][i];
                            expected(Form::Clip, straight(t), t[3] * opacity, inputs[1][i], mode, float, w)
                        }
                        Path::Effect => expected(Form::Clip, probe(depth, xy), opacity, inputs[1][i], mode, float, w),
                        Path::Folded => {
                            let a = inputs[1][i][3];
                            expected(Form::Composite, probe(depth, xy), a * opacity, backdrop, mode, float, w)
                        }
                        Path::ImageComposition => {
                            let a = inputs[1][i][3];
                            expected(Form::Composite, probe(depth, xy), a * opacity, inputs[2][i], mode, float, w)
                        }
                    };
                    for c in 0..4 {
                        let tol = tolerance(depth, high[c]);
                        assert!(
                            actual[c] >= low[c] - tol && actual[c] <= high[c] + tol,
                            "{depth:?} {path:?} {mode:?} at {xy:?} channel {c}: {} outside [{}, {}]",
                            actual[c], low[c], high[c]
                        );
                        assert!(
                            (export[c] - actual[c]).abs() <= tol,
                            "{depth:?} {path:?} {mode:?} at {xy:?} channel {c}: export {} != live {}",
                            export[c], actual[c]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn brush_blend_modes_use_the_layer_formulas() {
    let w = RgbSpace::Srgb.to_xyz()[1];
    for (depth, tolerance, source, backdrop) in [
        (SampleDepth::U16, 3. / 65535., [0.7, 0.3, 0.6, 1.], [0.4, 0.2, 0.8, 1.]),
        (SampleDepth::F32, 1e-5, [2.5, 0.3, 0.6, 1.], [1.5, 0.2, 0.8, 1.]),
    ] {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth })
            .expect("physical GPU required");
        let layer = Layer::paint(LayerId(1), "Paint");
        let extent = [128, 128];
        let center = 64 * 128 + 64;
        let read = |r: &mut WgpuRasterizer| -> Rgba {
            let p = crate::layer_tests::page_bytes(r, r.composite_texture.as_ref().unwrap());
            std::array::from_fn(|c| f64::from(f32::from_le_bytes(p[center * 16 + c * 4..][..4].try_into().unwrap())))
        };
        for mode in [
            layer_core::BrushBlendMode::Normal,
            layer_core::BrushBlendMode::Multiply,
            layer_core::BrushBlendMode::Screen,
            layer_core::BrushBlendMode::Add,
            layer_core::BrushBlendMode::Subtract,
            layer_core::BrushBlendMode::Darken,
            layer_core::BrushBlendMode::Lighten,
            layer_core::BrushBlendMode::Overlay,
        ] {
            let frame = |r: &mut WgpuRasterizer, color: [f32; 4], blend, reset| {
                let mut batch = crate::layer_tests::batch(1);
                batch.style.rendering.blend_mode = blend;
                let dabs = [crate::layer_tests::dab(color)];
                let layers = [layer.clone()];
                r.submit(FramePacket { dabs: &dabs, dab_batches: &[batch], reset_layers: reset, ..packet(&layers, extent) })
                    .unwrap();
            };
            frame(&mut r, backdrop, layer_core::BrushBlendMode::Normal, true);
            let d = read(&mut r);
            frame(&mut r, source, mode, false);
            let actual = read(&mut r);
            let s = source.map(f64::from);
            let expected = blend([s[0], s[1], s[2]], straight(d), mode.into(), depth.is_float(), w);
            for c in 0..3 {
                assert!(
                    (actual[c] - expected[c]).abs() <= tolerance * expected[c].abs().max(1.),
                    "{depth:?} {mode:?} channel {c}: {} != {}",
                    actual[c],
                    expected[c]
                );
            }
        }
    }
}
