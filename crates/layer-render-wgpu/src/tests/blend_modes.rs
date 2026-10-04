//! Every layer blend mode against an independent f64 reference, through each
//! compositor path that applies a layer's blend, at every document depth and
//! in both blend spaces.
use super::*;
use layer_core::color::source::SourceImage;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::authored::*;
use layer_core::{BlendRange, BlendSpace, Document, EffectInstance, LayerBlend, SceneView, SceneScope};

const EXTENT: [u32; 2] = [64, 32];
const DIVISOR: f64 = 1. / 16384.;
const BACKDROP: [f32; 4] = [0.25, 0.5, 0.125, 0.75];

pub(super) type Rgb = [f64; 3];
pub(super) type Rgba = [f64; 4];

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
pub(super) fn blend(s: Rgb, d: Rgb, mode: LayerBlend, float: bool, w: Rgb, space: BlendSpace) -> Rgb {
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
        B::SoftLight if space == BlendSpace::Perceptual => each(&|s, d| {
            if s > 0.5 { 2. * d * (1. - s) + d.sqrt() * (2. * s - 1.) } else { 2. * d * s + d * d * (1. - 2. * s) }
        }),
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
pub(super) enum Form {
    /// Source over backdrop.
    Composite,
    /// Inside the backdrop's coverage, keeping its alpha.
    Clip,
}
pub(super) fn straight(p: Rgba) -> Rgb {
    if p[3] > 0. { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [0.; 3] }
}
pub(super) fn combine(form: Form, s: Rgb, sa: f64, dst: Rgba, b: Rgb) -> Rgba {
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
#[allow(clippy::too_many_arguments)] // The reference's operands.
fn expected(form: Form, s: Rgb, sa: f64, dst: Rgba, mode: LayerBlend, float: bool, w: Rgb, space: BlendSpace) -> [Rgba; 2] {
    let d = straight(dst);
    let nudge = |c: Rgb, k: f64| c.map(|v| v + k * (v.abs() * 2e-6 + 1e-7));
    let mut range = [[f64::INFINITY; 4], [f64::NEG_INFINITY; 4]];
    for i in [-1., 0., 1.] {
        for j in [-1., 0., 1.] {
            let result = combine(form, s, sa, dst, blend(nudge(s, i), nudge(d, j), mode, float, w, space));
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
    crate::test_support::depth_source(EXTENT, depth, RgbSpace::Srgb, 16 * 1024 * 1024, pixel)
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
/// Straight linear `c` as the composite of `space` holds it.
pub(super) fn held(c: Rgb, space: BlendSpace) -> Rgb {
    if space == BlendSpace::Linear { c } else { c.map(|v| RgbSpace::Srgb.encode(v)) }
}
/// A composite pixel of `space` as linear premultiplied values.
pub(super) fn linear(p: Rgba, space: BlendSpace) -> Rgba {
    if space == BlendSpace::Linear || p[3] <= 0. {
        return p;
    }
    let rgb = straight(p).map(|v| RgbSpace::Srgb.decode(v) * p[3]);
    [rgb[0], rgb[1], rgb[2], p[3]]
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
    /// The layer whose blend varies.
    blended: usize,
}
fn case(depth: SampleDepth, path: Path, space: BlendSpace) -> Case {
    let mut document = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let root = document.artwork.root;
    let composition = document.artwork.compositions.get_mut(root).unwrap();
    composition.color = DocumentColor { space: RgbSpace::Srgb, depth };
    composition.blend = space;
    let paper = *document.scene().order().last().unwrap();
    let paint = |document: &mut Document, name: &str, image| {
        let source = document.artwork.paint.insert(PortableId::random(), PaintSource { domain: EXTENT, original: Some(image), raster: Default::default(), operations: Arc::default() }).unwrap();
        document.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(source), name)).unwrap()
    };
    let effect = |document: &mut Document, image: bool| {
        let instance = Arc::unwrap_or_clone(probe_effect(depth, image));
        let definition = document.artwork.definitions.insert(PortableId::random(), Definition { program: instance.program }).unwrap();
        let application = document.artwork.effects.insert(PortableId::random(), EffectApplication { definition, values: instance.values, domain: EXTENT }).unwrap();
        document.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), "Probe")).unwrap()
    };
    let top_layer = paint(&mut document, "Top", top(depth));
    let bottom_layer = paint(&mut document, "Bottom", bottom(depth));
    let (mut entries, background) = match path {
        Path::Layer => {
            document.artwork.occurrences.get_mut(top_layer).unwrap().opacity = OPACITY;
            (vec![top_layer, bottom_layer], [0.; 4])
        }
        Path::Clip => {
            let owner = document.artwork.occurrences.get_mut(top_layer).unwrap();
            owner.opacity = OPACITY;
            owner.clipped = true;
            (vec![top_layer, bottom_layer], [0.; 4])
        }
        Path::Effect => {
            let adjustment = effect(&mut document, false);
            document.artwork.occurrences.get_mut(adjustment).unwrap().opacity = OPACITY;
            (vec![adjustment, bottom_layer], [0.; 4])
        }
        Path::Folded => {
            let adjustment = effect(&mut document, false);
            document.artwork.occurrences.get_mut(adjustment).unwrap().clipped = true;
            document.artwork.occurrences.get_mut(bottom_layer).unwrap().opacity = OPACITY;
            (vec![adjustment, bottom_layer], BACKDROP)
        }
        Path::ImageComposition => {
            let filter = effect(&mut document, true);
            document.artwork.occurrences.get_mut(filter).unwrap().clipped = true;
            document.artwork.occurrences.get_mut(bottom_layer).unwrap().opacity = OPACITY;
            (vec![filter, bottom_layer, top_layer], [0.; 4])
        }
    };
    let blended = match path {
        Path::Layer | Path::Clip | Path::Effect => 0,
        Path::Folded | Path::ImageComposition => 1,
    };
    super::native_effects::set_effect(&mut document, paper, "color", layer_core::EffectValue::Color(
        layer_core::color::RgbColor::from_linear(RgbSpace::Srgb, background).unwrap()));
    document.artwork.occurrences.get_mut(paper).unwrap().visible = background[3] > 0.;
    entries.push(paper);
    let root = document.composition().result;
    let stack = RecordChange::replace(&document.artwork.stacks, root, Some(Stack { entries })).unwrap();
    document.apply(layer_core::Edit::Stack(stack)).unwrap();
    Case { document, blended }
}

fn live(r: &mut WgpuRasterizer, scene: SceneView<'_>, blend_space: BlendSpace) -> Vec<Rgba> {
    live_at(r, scene, 0, blend_space, true)
}
fn live_at(r: &mut WgpuRasterizer, scene: SceneView<'_>, level: u32, blend_space: BlendSpace, settled: bool) -> Vec<Rgba> {
    let scale = 1. / (1 << level) as f32;
    let view = ViewState {
        document_to_surface: [scale, 0., 0., scale, 0., 0.],
        ..crate::test_support::view(EXTENT)
    };
    r.submit(FramePacket { view, reset_layers: true, blend_space, ..packet(scene, EXTENT) }).unwrap();
    for _ in 0..16 {
        if !settled || !layer_render::CanvasRenderer::has_pending_work(r) {
            break;
        }
        r.submit(FramePacket { view, blend_space, composite_all: false, ..packet(scene, EXTENT) }).unwrap();
    }
    assert!(!settled || !layer_render::CanvasRenderer::has_pending_work(r), "the composite settles");
    let texture = if level == 0 {
        crate::test_support::document_texture(r)
    } else {
        let display = r.scale_display.as_ref().unwrap();
        assert_eq!(display.plan.level, level);
        display.texture()
    };
    crate::layer_tests::page_bytes(r, texture)
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|c| f64::from(f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap()))))
        .collect()
}
fn exported(r: &WgpuRasterizer, document: &Document) -> Vec<Rgba> {
    let mut capture = r
        .snapshot_gpu()
        .capture_scene(document.snapshot_with_context(r.evaluation_context()), SceneScope::All, Default::default())
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
    let mut isolated = document.clone();
    let handles = isolated.scene().order().to_vec();
    for (i, handle) in handles.into_iter().enumerate() {
        let occurrence = isolated.artwork.occurrences.get_mut(handle).unwrap();
        occurrence.visible = i == index;
        occurrence.opacity = 1.;
        occurrence.blend = LayerBlend::Normal;
        occurrence.clipped = false;
    }
    live(r, isolated.scene(), document.composition().blend)
}

#[test]
fn every_blend_mode_matches_the_reference_on_every_path_and_depth() {
    for (depth, space) in [
        (SampleDepth::U8, BlendSpace::Linear),
        (SampleDepth::U8, BlendSpace::Perceptual),
        (SampleDepth::U16, BlendSpace::Linear),
        (SampleDepth::U16, BlendSpace::Perceptual),
        (SampleDepth::F16, BlendSpace::Linear),
        (SampleDepth::F32, BlendSpace::Linear),
    ] {
        let w = if space == BlendSpace::Perceptual { [0.3, 0.59, 0.11] } else { RgbSpace::Srgb.to_xyz()[1] };
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth })
            .expect("physical GPU required");
        let float = depth.is_float();
        for path in [Path::Layer, Path::Clip, Path::Effect, Path::Folded, Path::ImageComposition] {
            let Case { mut document, blended } = case(depth, path, space);
            let inputs: Vec<_> = (0..document.scene().order().len() - 1).map(|i| alone(&mut r, &document, i)).collect();
            let a = f64::from(BACKDROP[3]);
            let backdrop = held([BACKDROP[0], BACKDROP[1], BACKDROP[2]].map(f64::from), space);
            let backdrop = [backdrop[0] * a, backdrop[1] * a, backdrop[2] * a, a];
            let opacity = f64::from(OPACITY);
            let probe = |xy| held(probe(depth, xy), space);
            for mode in LayerBlend::ALL.into_iter().filter(|m| *m != LayerBlend::PassThrough) {
                let owner = document.scene().order()[blended];
                document.artwork.occurrences.get_mut(owner).unwrap().blend = mode;
                let composite = live(&mut r, document.scene(), space);
                let export = exported(&r, &document);
                for (i, (actual, export)) in composite.iter().zip(&export).enumerate() {
                    let xy = [i as u32 % EXTENT[0], i as u32 / EXTENT[0]];
                    let [low, high] = match path {
                        Path::Layer => {
                            let t = inputs[0][i];
                            expected(Form::Composite, straight(t), t[3] * opacity, inputs[1][i], mode, float, w, space)
                        }
                        Path::Clip => {
                            let t = inputs[0][i];
                            expected(Form::Clip, straight(t), t[3] * opacity, inputs[1][i], mode, float, w, space)
                        }
                        Path::Effect => expected(Form::Clip, probe(xy), opacity, inputs[1][i], mode, float, w, space),
                        Path::Folded => {
                            let a = inputs[1][i][3];
                            expected(Form::Composite, probe(xy), a * opacity, backdrop, mode, float, w, space)
                        }
                        Path::ImageComposition => {
                            let a = inputs[1][i][3];
                            expected(Form::Composite, probe(xy), a * opacity, inputs[2][i], mode, float, w, space)
                        }
                    };
                    let linear = linear(*actual, space);
                    for c in 0..4 {
                        let tol = tolerance(depth, high[c]);
                        assert!(
                            actual[c] >= low[c] - tol && actual[c] <= high[c] + tol,
                            "{depth:?} {space:?} {path:?} {mode:?} at {xy:?} channel {c}: {} outside [{}, {}]",
                            actual[c], low[c], high[c]
                        );
                        assert!(
                            (export[c] - linear[c]).abs() <= tol,
                            "{depth:?} {space:?} {path:?} {mode:?} at {xy:?} channel {c}: export {} != live {}",
                            export[c], linear[c]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn reduced_blend_modes_match_the_reference_at_every_depth() {
    for (depth, space) in [
        (SampleDepth::U8, BlendSpace::Linear), (SampleDepth::U8, BlendSpace::Perceptual),
        (SampleDepth::U16, BlendSpace::Linear), (SampleDepth::U16, BlendSpace::Perceptual),
        (SampleDepth::F16, BlendSpace::Linear), (SampleDepth::F32, BlendSpace::Linear),
    ] {
        let weights = if space == BlendSpace::Perceptual { [0.3, 0.59, 0.11] } else { RgbSpace::Srgb.to_xyz()[1] };
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor { space: RgbSpace::Srgb, depth }).unwrap();
        for path in [Path::Layer, Path::Clip] {
            let Case { mut document, blended } = case(depth, path, space);
            let inputs = [alone(&mut r, &document, 0), alone(&mut r, &document, 1)];
            for level in [1, 3] {
                let step = 1 << level;
                let extent = EXTENT.map(|size| size / step);
                let average = |input: &[Rgba], x: u32, y: u32| -> Rgba {
                    let mut sum = [0.; 4];
                    for dy in 0..step {
                        for dx in 0..step {
                            let p = input[((y * step + dy) * EXTENT[0] + x * step + dx) as usize];
                            for c in 0..4 { sum[c] += p[c] / f64::from(step * step); }
                        }
                    }
                    sum
                };
                for mode in LayerBlend::ALL.into_iter().filter(|m| *m != LayerBlend::PassThrough) {
                    let owner = document.scene().order()[blended];
                document.artwork.occurrences.get_mut(owner).unwrap().blend = mode;
                    let form = if matches!(path, Path::Clip) { Form::Clip } else { Form::Composite };
                    let native: Vec<_> = inputs[0].iter().zip(&inputs[1]).map(|(top, bottom)|
                        expected(form, straight(*top), top[3] * f64::from(OPACITY), *bottom, mode, depth.is_float(), weights, space)).collect();
                    let native_low: Vec<_> = native.iter().map(|range| range[0]).collect();
                    let native_high: Vec<_> = native.iter().map(|range| range[1]).collect();
                    for settled in [false, true] {
                        let actual = live_at(&mut r, document.scene(), level, document.composition().blend, settled);
                        assert_eq!(actual.len(), (extent[0] * extent[1]) as usize);
                        for (i, actual) in actual.iter().enumerate() {
                            let (x, y) = (i as u32 % extent[0], i as u32 / extent[0]);
                            let top = average(&inputs[0], x, y);
                            let bottom = average(&inputs[1], x, y);
                            let [low, high] = if settled {
                                [average(&native_low, x, y), average(&native_high, x, y)]
                            } else { expected(form, straight(top), top[3] * f64::from(OPACITY), bottom, mode, depth.is_float(), weights, space) };
                            for c in 0..4 {
                                let tol = tolerance(depth, high[c]);
                                assert!(actual[c] >= low[c] - tol && actual[c] <= high[c] + tol,
                                    "{depth:?} {space:?} {path:?} {mode:?} level={level} settled={settled} at ({x}, {y}) channel {c}: {} outside [{}, {}]",
                                    actual[c], low[c], high[c]);
                            }
                        }
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
        let extent = [128, 128];
        let mut artwork = Artwork::new(extent).unwrap();
        artwork.compositions.get_mut(artwork.root).unwrap().color = DocumentColor { space: RgbSpace::Srgb, depth };
        let (_, target) = crate::test_support::add_paint(&mut artwork, "Paint", extent);
        let document = Document::from_artwork(artwork).unwrap();
        let center = 64 * 128 + 64;
        let read = |r: &mut WgpuRasterizer| -> Rgba {
            let p = crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r));
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
                let mut batch = crate::layer_tests::batch(target);
                batch.style.rendering.blend_mode = blend;
                let dabs = [crate::layer_tests::dab(color)];
                r.submit(FramePacket { dabs: &dabs, dab_batches: &[batch], reset_layers: reset, ..packet(document.scene(), extent) })
                    .unwrap();
            };
            frame(&mut r, backdrop, layer_core::BrushBlendMode::Normal, true);
            let d = read(&mut r);
            frame(&mut r, source, mode, false);
            let actual = read(&mut r);
            let s = source.map(f64::from);
            let expected = blend([s[0], s[1], s[2]], straight(d), mode.into(), depth.is_float(), w, BlendSpace::Linear);
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
