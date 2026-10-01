//! The Perceptual composite against the document's own transfer curve, and
//! its readers: export, presentation and exact readback.
use super::*;
use super::blend_modes::{Form, Rgba, blend, combine, held, linear, straight};
use layer_core::color::source::SourceImage;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, Document, EffectInstance, LayerBlend, LayerMask, Project, Selection};

const EXTENT: [u32; 2] = [300, 200];

fn source(depth: SampleDepth, pixel: impl Fn(f32, f32) -> [f32; 4]) -> Arc<SourceImage> {
    crate::test_support::depth_source(EXTENT, depth, RgbSpace::Srgb, 64 * 1024 * 1024, |x, y| pixel(x as f32 / (EXTENT[0] - 1) as f32, y as f32 / (EXTENT[1] - 1) as f32))
}

fn polygon(points: &[[f32; 2]]) -> Selection {
    Selection::polygon(points.iter().map(|&[x, y]| Point { x, y }).collect()).unwrap()
}

fn effect(document: &mut Document, id: &str) -> Layer {
    let mut layer = Layer::paint(document.allocate_layer_id(), id);
    layer.kind = LayerKind::Effect;
    layer.effect = Some(Arc::new(EffectInstance::new(fixture(id).program())));
    layer
}

/// Every blend mode, masks, a group, a clipping stack, pointwise, image and
/// generator effects, an offset layer and a placed photo over the paper.
fn representative(depth: SampleDepth) -> Document {
    let mut document = Document::new("Representative", EXTENT[0], EXTENT[1]);
    document.color = DocumentColor { space: RgbSpace::Srgb, depth };
    let paper = document.layers.pop().unwrap();
    document.layers.clear();
    let paint = |document: &mut Document, name: &str, image: Arc<SourceImage>| {
        let mut layer = Layer::paint(document.allocate_layer_id(), name);
        layer.source = Some(image);
        layer
    };
    let mut layers = Vec::new();

    let mut blur = effect(&mut document, "gaussian_blur");
    blur.opacity = 0.85;
    let mut blur_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    blur_mask.initial = Some(polygon(&[[20., 10.], [280., 30.], [150., 190.]]));
    blur.mask = Some(blur_mask);
    layers.push(blur);

    let mut clipped = paint(&mut document, "Clipped", source(depth, |u, v| [u, 0.3 + 0.5 * v, 1. - u, (0.2 + 0.7 * v).min(1.)]));
    clipped.properties.clipped = true;
    clipped.properties.blend = LayerBlend::Overlay;
    clipped.opacity = 0.7;
    layers.push(clipped);
    let mut clipped_curves = effect(&mut document, "curves");
    clipped_curves.properties.clipped = true;
    clipped_curves.opacity = 0.6;
    layers.push(clipped_curves);
    let mut base = paint(&mut document, "Clip base", source(depth, |u, v| {
        let inside = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
        [0.9 * v, 0.2 + 0.6 * u, 0.4, (1.2 - 2. * inside).clamp(0., 1.)]
    }));
    base.properties.blend = LayerBlend::Screen;
    base.opacity = 0.9;
    layers.push(base);

    let mut group = Layer::paint(document.allocate_layer_id(), "Group");
    group.kind = LayerKind::Group;
    group.properties.blend = LayerBlend::Multiply;
    group.opacity = 0.8;
    let mut group_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    group_mask.default_coverage = 0.75;
    group_mask.initial = Some(polygon(&[[0., 0.], [300., 0.], [300., 120.], [0., 80.]]));
    group.mask = Some(group_mask);
    let group_id = group.id;
    layers.push(group);
    let mut group_curves = effect(&mut document, "curves");
    group_curves.properties.parent = Some(group_id);
    layers.push(group_curves);
    let mut shifted = paint(&mut document, "Shifted", source(depth, |u, v| [1. - v, u * v, 0.5 + 0.4 * u, (0.3 + u).min(1.)]));
    shifted.properties.parent = Some(group_id);
    shifted.properties.offset = Point { x: 37.5, y: 13. };
    let mut shifted_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    shifted_mask.initial = Some(polygon(&[[10., 150.], [290., 20.], [260., 190.]]));
    shifted.mask = Some(shifted_mask);
    layers.push(shifted);

    let modes: Vec<_> = LayerBlend::ALL.into_iter().filter(|m| *m != LayerBlend::PassThrough).collect();
    for (i, &mode) in modes.iter().enumerate() {
        let k = i as f32 / modes.len() as f32;
        let mut layer = paint(&mut document, mode.label(), source(depth, move |u, v| {
            let band = ((u * 7. + k * 3.).fract() - 0.5).abs() * 2.;
            [(u + k).fract(), (v + 0.5 * k).fract(), (1. - u * v + k).fract(), band * (0.35 + 0.5 * v)]
        }));
        layer.properties.blend = mode;
        layer.opacity = 0.55 + 0.4 * k;
        layers.push(layer);
    }

    let mut painted = Layer::paint(document.allocate_layer_id(), "Painted");
    let mut painted_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    painted_mask.initial = Some(polygon(&[[60., 20.], [280., 90.], [120., 190.]]));
    painted.mask = Some(painted_mask);
    painted.opacity = 0.8;
    layers.push(painted);

    let mut placed = paint(&mut document, "Placed", source(depth, |u, v| [0.2 + 0.7 * u, 0.6 * v, 0.9 - 0.5 * u, 1.]));
    placed.properties.placement = layer_core::Affine::around(Point { x: 150., y: 100. }, [0.8, 0.7], 0.35, Point { x: 11., y: -7. });
    placed.opacity = 0.7;
    layers.push(placed);

    let mut masked = paint(&mut document, "Masked", source(depth, |u, v| [0.1, 0.8 * u, 0.9 * v, 0.9]));
    let mut masked_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    masked_mask.initial = Some(polygon(&[[40., 40.], [260., 60.], [200., 180.], [30., 150.]]));
    masked.mask = Some(masked_mask);
    masked.opacity = 0.65;
    layers.push(masked);

    let mut fill = effect(&mut document, "solid_color");
    fill.properties.blend = LayerBlend::Color;
    fill.opacity = 0.5;
    let mut fill_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
    fill_mask.default_coverage = 0.5;
    fill.mask = Some(fill_mask);
    layers.push(fill);

    let mut bottom = paint(&mut document, "Bottom", source(depth, |u, v| [u, v, 0.5, 0.4 + 0.6 * u]));
    bottom.opacity = 0.6;
    layers.push(bottom);

    let mut paper = paper;
    paper.visible = true;
    layers.push(paper);
    document.layers = layers;
    document.active_layer = document.layers[1].id;
    document
}

const BACKGROUND: [f32; 4] = [0.95, 0.9, 0.8, 1.];

fn settle(r: &mut WgpuRasterizer, packet: FramePacket<'_>) {
    r.submit(packet).unwrap();
    for _ in 0..32 {
        if !layer_render::CanvasRenderer::has_pending_work(r) {
            break;
        }
        r.submit(FramePacket { reset_layers: false, composite_all: false, dabs: &[], dab_batches: &[], ..packet }).unwrap();
    }
    assert!(!layer_render::CanvasRenderer::has_pending_work(r), "the composite settles");
}

fn composite(r: &WgpuRasterizer) -> Vec<u8> {
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
}

fn exported(r: &WgpuRasterizer, document: &Document) -> Vec<u8> {
    let mut capture = r
        .snapshot_gpu()
        .capture(Project { document: document.clone() }, BACKGROUND, 0., Default::default())
        .unwrap();
    capture
        .read_region([0, 0, EXTENT[0], EXTENT[1]])
        .unwrap()
        .into_iter()
        .flat_map(|p| p.into_iter().flat_map(f32::to_le_bytes))
        .collect()
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect()
}

/// The encoded composite, decoded on the CPU.
fn decoded(composite: &[f32]) -> Vec<f32> {
    composite
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3];
            let decode = |c: f32| if a > 0. { (RgbSpace::Srgb.decode(f64::from(c / a)) * f64::from(a)) as f32 } else { c };
            [decode(p[0]), decode(p[1]), decode(p[2]), a]
        })
        .collect()
}

#[test]
fn export_and_readback_equal_the_live_composite_in_both_spaces() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for space in BlendSpace::ALL {
            let mut document = representative(depth);
            document.blend_space = space;
            let mut r = WgpuRasterizer::new_native_headless(document.color).expect("physical GPU required");
            let view = ViewState { background_rgba_linear: BACKGROUND, ..crate::test_support::view(EXTENT) };
            settle(&mut r, FramePacket { view, reset_layers: true, blend_space: space, ..packet(&document.layers, EXTENT) });
            let live = floats(&composite(&r));
            let export = floats(&exported(&r, &document));
            if space == BlendSpace::Linear {
                assert_eq!(live, export, "{depth:?} Linear export is the live composite");
                continue;
            }
            for (i, (live, export)) in decoded(&live).into_iter().zip(export).enumerate() {
                assert!((live - export).abs() <= 2e-5, "{depth:?} {space:?} value {i}: decoded composite {live} != export {export}");
            }
            let srgb = r.readback_srgb_rgba8().unwrap();
            for (i, (code, export)) in srgb.iter().zip(floats(&exported(&r, &document)).chunks_exact(4).flat_map(|p| {
                let a = p[3];
                [p[0], p[1], p[2]].map(|c| if a > 0. { layer_core::color::srgb_encode(c / a) } else { 0. }).into_iter().chain([a])
            })).enumerate() {
                if i % 4 != 3 && srgb[i - i % 4 + 3] == 0 { continue; }
                assert!((f32::from(*code) - export * 255.).abs() <= 1., "{depth:?} readback {i}: {code} != {}", export * 255.);
            }
        }
    }
}

/// Opaque `color` over the whole canvas, at `depth`.
fn plain(depth: SampleDepth, color: [f32; 4]) -> Arc<SourceImage> {
    source(depth, move |_, _| color)
}

#[test]
fn black_at_half_opacity_over_white_is_middle_gray_only_when_blending_perceptually() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for (space, expected) in [(BlendSpace::Perceptual, 128.), (BlendSpace::Linear, 188.)] {
            let mut document = Document::new("Gray", EXTENT[0], EXTENT[1]);
            document.color = DocumentColor { space: RgbSpace::Srgb, depth };
            document.blend_space = space;
            document.layers[0].source = Some(plain(depth, [0., 0., 0., 1.]));
            document.layers[0].opacity = 0.5;
            let mut r = WgpuRasterizer::new_native_headless(document.color).expect("physical GPU required");
            let view = ViewState { background_rgba_linear: [1.; 4], ..crate::test_support::view(EXTENT) };
            settle(&mut r, FramePacket { view, reset_layers: true, blend_space: space, ..packet(&document.layers, EXTENT) });
            let center = ((EXTENT[1] / 2 * EXTENT[0] + EXTENT[0] / 2) * 4) as usize;
            let live = floats(&composite(&r))[center];
            assert!((live - 0.5).abs() < 1e-6, "{depth:?} {space:?}: the composite holds {live}");
            let mut capture = r.snapshot_gpu().capture(Project { document: document.clone() }, [1.; 4], 0., Default::default()).unwrap();
            let export = capture.read_region([EXTENT[0] / 2, EXTENT[1] / 2, 1, 1]).unwrap()[0];
            let code = layer_core::color::srgb_encode(export[0]) * 255.;
            assert!((code - expected).abs() <= 1., "{depth:?} {space:?}: exported {code}, not {expected}");
            let srgb = r.readback_srgb_rgba8().unwrap();
            assert!((f32::from(srgb[center]) - expected).abs() <= 1., "{depth:?} {space:?}: read back {}", srgb[center]);
            let target = crate::create_target(&r.device, [EXTENT[0], EXTENT[1]], wgpu::TextureFormat::Rgba8UnormSrgb, "presented gray").0;
            let mut presenter = crate::ViewportPresenter::for_surface(&r, wgpu::TextureFormat::Rgba8UnormSrgb, crate::SdrSurfaceColor::Srgb).unwrap();
            presenter.present(&r, &target.create_view(&Default::default()), view, [0.2, 0.3, 0.4, 1.]).unwrap();
            let presented = crate::layer_tests::page_bytes(&r, &target);
            assert!((f32::from(presented[center]) - expected).abs() <= 1., "{depth:?} {space:?}: presented {}", presented[center]);
        }
    }
}

fn rgba(bytes: &[u8]) -> Vec<Rgba> {
    floats(bytes).chunks_exact(4).map(|p| std::array::from_fn(|c| f64::from(p[c]))).collect()
}

#[test]
fn groups_masks_clips_and_opacity_match_an_encoded_reference() {
    let perceptual = BlendSpace::Perceptual;
    let luma = [0.3, 0.59, 0.11];
    for (depth, tolerance) in [(SampleDepth::U8, 1. / 255.), (SampleDepth::U16, 3. / 65535.)] {
        let mut document = Document::new("Stack", EXTENT[0], EXTENT[1]);
        document.color = DocumentColor { space: RgbSpace::Srgb, depth };
        document.blend_space = perceptual;
        let paper = document.layers.pop().unwrap();
        let paint = |document: &mut Document, name: &str, image| {
            let mut layer = Layer::paint(document.allocate_layer_id(), name);
            layer.source = Some(image);
            layer
        };
        let mut top = paint(&mut document, "Top", source(depth, |u, v| [0.9 * u, 0.2, 1. - v, 0.3 + 0.6 * v]));
        top.opacity = 0.45;
        let mut group = Layer::paint(document.allocate_layer_id(), "Group");
        group.kind = LayerKind::Group;
        group.properties.blend = LayerBlend::Multiply;
        group.opacity = 0.8;
        let mut group_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
        group_mask.default_coverage = 0.75;
        group.mask = Some(group_mask);
        let mut clipped = paint(&mut document, "Clipped", source(depth, |u, v| [v, 1. - u, 0.5, 0.2 + 0.8 * u]));
        clipped.properties.parent = Some(group.id);
        clipped.properties.clipped = true;
        clipped.properties.blend = LayerBlend::SoftLight;
        clipped.opacity = 0.8;
        let mut base = paint(&mut document, "Base", source(depth, |u, v| [0.2 + 0.7 * v, 0.6 * u, 0.9 - 0.8 * u * v, 0.1 + 0.9 * u]));
        base.properties.parent = Some(group.id);
        base.opacity = 0.7;
        let mut base_mask = LayerMask::reveal_all(document.allocate_layer_id(), Point::default());
        base_mask.default_coverage = 0.6;
        base.mask = Some(base_mask);
        let under = paint(&mut document, "Under", source(depth, |u, v| [u, v, 0.3, 0.5 + 0.5 * v]));
        let mut paper = paper;
        paper.visible = true;
        document.layers = vec![top, group, clipped, base, under, paper];
        document.active_layer = document.layers[0].id;
        let mut r = WgpuRasterizer::new_native_headless(document.color).expect("physical GPU required");
        let view = |background| ViewState { background_rgba_linear: background, ..crate::test_support::view(EXTENT) };
        let mut alone = |index: usize| {
            let mut layers = document.layers.clone();
            for (i, layer) in layers.iter_mut().enumerate() {
                layer.visible = i == index;
                layer.opacity = 1.;
                layer.mask = None;
                layer.properties.parent = None;
                layer.properties.clipped = false;
                layer.properties.blend = LayerBlend::Normal;
            }
            settle(&mut r, FramePacket { view: view([0.; 4]), reset_layers: true, ..packet(&layers, EXTENT) });
            rgba(&composite(&r))
        };
        let [top, clipped, base, under] = [0, 2, 3, 4].map(&mut alone);
        settle(&mut r, FramePacket { view: view(BACKGROUND), reset_layers: true, blend_space: perceptual, ..packet(&document.layers, EXTENT) });
        let live = rgba(&composite(&r));
        let export = rgba(&exported(&r, &document));
        let encode = |p: Rgba| {
            let c = held(straight(p), perceptual);
            [c[0] * p[3], c[1] * p[3], c[2] * p[3], p[3]]
        };
        let normal = |layer: Rgba, weight: f64, dst: Rgba| {
            let s = straight(encode(layer));
            combine(Form::Composite, s, layer[3] * weight, dst, s)
        };
        for (i, (live, export)) in live.iter().zip(&export).enumerate() {
            let a = f64::from(BACKGROUND[3]);
            let paper = held([BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]].map(f64::from), perceptual);
            let mut out = normal(under[i], 1., [paper[0] * a, paper[1] * a, paper[2] * a, a]);
            let mut inside = normal(base[i], 0.7 * 0.6, [0.; 4]);
            let s = straight(encode(clipped[i]));
            inside = combine(Form::Clip, s, clipped[i][3] * 0.8, inside, blend(s, straight(inside), LayerBlend::SoftLight, false, luma, perceptual));
            let inside = inside.map(|v| v * 0.75);
            let s = straight(inside);
            out = combine(Form::Composite, s, inside[3] * 0.8, out, blend(s, straight(out), LayerBlend::Multiply, false, luma, perceptual));
            out = normal(top[i], 0.45, out);
            let expected = linear(out, perceptual);
            for c in 0..4 {
                assert!((live[c] - out[c]).abs() <= tolerance, "{depth:?} pixel {i} channel {c}: composite {} != {}", live[c], out[c]);
                assert!((export[c] - expected[c]).abs() <= tolerance, "{depth:?} pixel {i} channel {c}: export {} != {}", export[c], expected[c]);
            }
        }
    }
}
