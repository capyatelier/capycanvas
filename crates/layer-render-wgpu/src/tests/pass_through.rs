//! Pass Through groups against the documents they are equivalent to: the same
//! layers ungrouped, a fade between the composites without and with the
//! group, and an isolated Normal group once clipped.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, Document, Edit, EffectInstance, LayerBlend, LayerMask, Project, Selection};

const EXTENT: [u32; 2] = [300, 280];
const COLOR: DocumentColor = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
const PAPER: [f32; 4] = [0.9, 0.85, 0.8, 1.];

fn source(seed: u32, alpha: impl Fn(u32, u32) -> u16) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        EXTENT,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..EXTENT[1] {
        let mut row = Vec::with_capacity(EXTENT[0] as usize * 8);
        for x in 0..EXTENT[0] {
            for v in [
                ((x * 211 + y * seed * 37) % 60000) as u16,
                ((x * seed * 29 + y * 173) % 55000) as u16,
                (4000 + (x * 3 + y * 5 + seed * 997) % 58000) as u16,
                alpha(x, y),
            ] {
                row.extend_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}

struct Builder(Document);
impl Builder {
    fn new() -> Self {
        let mut document = Document::new("Pass Through oracle", EXTENT[0], EXTENT[1]);
        document.color = COLOR;
        document.layers.remove(0);
        Self(document)
    }
    fn push(&mut self, mut layer: Layer, parent: Option<LayerId>) -> LayerId {
        layer.properties.parent = parent;
        let id = layer.id;
        let paper = self.0.layers.len() - 1;
        self.0.layers.insert(paper, layer);
        id
    }
    fn paint(&mut self, name: &str, parent: Option<LayerId>, blend: LayerBlend, seed: u32, alpha: impl Fn(u32, u32) -> u16) -> LayerId {
        let mut layer = Layer::paint(self.0.allocate_layer_id(), name);
        layer.source = Some(source(seed, alpha));
        layer.properties.blend = blend;
        self.push(layer, parent)
    }
    fn group(&mut self, name: &str, parent: Option<LayerId>, blend: LayerBlend) -> LayerId {
        let mut layer = Layer::paint(self.0.allocate_layer_id(), name);
        layer.kind = LayerKind::Group;
        layer.properties.blend = blend;
        self.push(layer, parent)
    }
    fn effect(&mut self, name: &str, parent: Option<LayerId>, filter: &str) -> LayerId {
        let mut layer = Layer::paint(self.0.allocate_layer_id(), name);
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(EffectInstance::new(fixture(filter).program())));
        self.push(layer, parent)
    }
    fn layer(&mut self, id: LayerId) -> &mut Layer {
        self.0.layers.iter_mut().find(|l| l.id == id).unwrap()
    }
}

fn disc(cx: u32, cy: u32, r: u32) -> impl Fn(u32, u32) -> u16 {
    move |x, y| {
        let d = ((x.abs_diff(cx).pow(2) + y.abs_diff(cy).pow(2)) as f32).sqrt();
        (((r as f32 - d) / 24.).clamp(0., 1.) * 65535.) as u16
    }
}

/// Top to bottom: a Screen layer, a Pass Through group holding an
/// adjustment, a Multiply layer, a nested Pass Through group with an image
/// filter and a clipping stack, and an isolated group around a third Pass
/// Through group with a Difference layer; then an opaque backdrop and paper.
/// Returns the document and the outer group.
fn nested() -> (Document, LayerId) {
    let mut b = Builder::new();
    b.paint("Screen", None, LayerBlend::Screen, 3, disc(60, 70, 50));
    let outer = b.group("Outer", None, LayerBlend::PassThrough);
    let adjustment = b.effect("Black & White", Some(outer), "black_white");
    b.layer(adjustment).opacity = 0.8;
    b.paint("Multiply", Some(outer), LayerBlend::Multiply, 5, disc(200, 90, 80));
    let inner = b.group("Inner", Some(outer), LayerBlend::PassThrough);
    b.layer(inner).properties.offset = Point { x: 13., y: -7. };
    b.effect("Blur", Some(inner), "gaussian_blur");
    let clip = b.paint("Overlay clip", Some(inner), LayerBlend::Overlay, 7, |x, _| if x % 90 < 60 { 65535 } else { 0 });
    b.layer(clip).properties.clipped = true;
    let base = b.paint("Base", Some(inner), LayerBlend::Normal, 11, disc(150, 180, 90));
    b.layer(base).opacity = 0.8;
    let isolated = b.group("Isolated", Some(outer), LayerBlend::Normal);
    let deepest = b.group("Deepest", Some(isolated), LayerBlend::PassThrough);
    b.paint("Difference", Some(deepest), LayerBlend::Difference, 13, disc(240, 220, 70));
    b.paint("Inside isolated", Some(isolated), LayerBlend::Normal, 17, disc(210, 200, 60));
    b.paint("Backdrop", None, LayerBlend::Normal, 19, |_, _| 65535);
    b.0.active_layer = b.0.layers[0].id;
    (b.0, outer)
}

/// The live composite of `document`, in its blend space.
fn render(r: &mut WgpuRasterizer, document: &Document) -> Vec<[f32; 4]> {
    let view = ViewState { background_rgba_linear: PAPER, ..crate::test_support::view(EXTENT) };
    let frame = FramePacket { view, blend_space: document.blend_space, ..packet(&document.layers, EXTENT) };
    r.submit(FramePacket { reset_layers: true, ..frame }).unwrap();
    for _ in 0..16 {
        if !layer_render::CanvasRenderer::has_pending_work(r) {
            break;
        }
        r.submit(frame).unwrap();
    }
    assert!(!layer_render::CanvasRenderer::has_pending_work(r), "the composite settles");
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
        .chunks_exact(16)
        .map(|p| std::array::from_fn(|c| f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap())))
        .collect()
}

fn exported(r: &WgpuRasterizer, document: &Document) -> Vec<[f32; 4]> {
    let mut capture = r
        .snapshot_gpu()
        .capture(Project { document: document.clone() }, PAPER, 0., Default::default())
        .unwrap();
    capture.read_region([0, 0, EXTENT[0], EXTENT[1]]).unwrap()
}

/// `composite` as linear pixels, decoded from a Perceptual composite.
fn linear(document: &Document, composite: &[[f32; 4]]) -> Vec<[f32; 4]> {
    composite
        .iter()
        .map(|&[red, green, blue, alpha]| {
            if document.blend_space == BlendSpace::Linear || alpha <= 0. {
                return [red, green, blue, alpha];
            }
            let decode = |c: f32| (RgbSpace::Srgb.decode(f64::from(c / alpha)) * f64::from(alpha)) as f32;
            [decode(red), decode(green), decode(blue), alpha]
        })
        .collect()
}

fn largest_difference(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b).flat_map(|(a, b)| (0..4).map(move |c| (a[c] - b[c]).abs())).fold(0., f32::max)
}

fn assert_close(actual: &[[f32; 4]], expected: &[[f32; 4]], tolerance: f32, what: &str) {
    let largest = largest_difference(actual, expected);
    assert!(largest <= tolerance, "{what}: largest difference {largest}");
}

/// Every Pass Through group ungrouped, innermost last, through Ungroup.
fn ungrouped(document: &Document) -> Document {
    let mut document = document.clone();
    while let Some(group) = document.layers.iter().find(|l| l.passes_through()).map(|l| l.id) {
        document.apply(document.ungroup_layer_edit(group).unwrap()).unwrap();
    }
    document
}

fn set(document: &Document, id: LayerId, change: impl Fn(&mut Layer)) -> Document {
    let mut document = document.clone();
    change(document.layers.iter_mut().find(|l| l.id == id).unwrap());
    document
}

#[test]
fn pass_through_groups_composite_as_their_layers_ungrouped() {
    let mut r = WgpuRasterizer::new_native_headless(COLOR).expect("physical GPU required");
    for blend_space in BlendSpace::ALL {
        let (mut document, outer) = nested();
        document.blend_space = blend_space;
        let passing = render(&mut r, &document);
        let flat = ungrouped(&document);
        assert!(flat.layers.iter().all(|l| l.kind != LayerKind::Group || !l.passes_through()));
        assert_close(&passing, &render(&mut r, &flat), 2e-5, &format!("{blend_space:?}: nested groups against their layers ungrouped"));
        let isolated = render(&mut r, &set(&document, outer, |g| g.properties.blend = LayerBlend::Normal));
        assert!(
            largest_difference(&passing, &isolated) > 0.1,
            "{blend_space:?}: the adjustment and Multiply layer inside reach the backdrop below the group"
        );
        let backdrop = set(&document, outer, |g| g.visible = false);
        let without = render(&mut r, &backdrop);
        let corner = (270 + 10 * EXTENT[0]) as usize;
        assert!(
            (0..3).any(|c| (passing[corner][c] - without[corner][c]).abs() > 0.05),
            "{blend_space:?}: the Black & White adjustment desaturates the backdrop where no grouped layer draws"
        );
        assert_close(&exported(&r, &document), &linear(&document, &passing), 2e-5, &format!("{blend_space:?}: export"));

        let mut moved = document.clone();
        let inner = moved.layers.iter().find(|l| &*l.name == "Inner").unwrap().id;
        let last_child = moved.layers.iter().rposition(|l| l.properties.parent == Some(inner)).unwrap();
        moved.apply(Edit::MoveLayer { id: inner, to: last_child }).unwrap();
        assert!(
            moved.layers.iter().position(|l| l.id == inner).unwrap()
                > moved.layers.iter().position(|l| l.properties.parent == Some(inner)).unwrap()
        );
        assert_close(&render(&mut r, &moved), &passing, 2e-5, &format!("{blend_space:?}: a group stored after its layers"));
    }
}

/// The mask's coverage at each pixel, from an isolated group that holds one
/// opaque layer.
fn coverage(r: &mut WgpuRasterizer, mask: &LayerMask) -> Vec<f32> {
    let mut b = Builder::new();
    let probe = b.group("Mask", None, LayerBlend::Normal);
    b.layer(probe).mask = Some(mask.clone());
    b.paint("Opaque", Some(probe), LayerBlend::Normal, 1, |_, _| 65535);
    let paper = b.0.layers.len() - 1;
    b.0.layers[paper].visible = false;
    let view = ViewState { background_rgba_linear: [0.; 4], ..crate::test_support::view(EXTENT) };
    r.submit(FramePacket { view, reset_layers: true, ..packet(&b.0.layers, EXTENT) }).unwrap();
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
        .chunks_exact(16)
        .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
        .collect()
}

#[test]
fn opacity_and_mask_fade_between_the_backdrop_and_the_groups_result() {
    let mut r = WgpuRasterizer::new_native_headless(COLOR).expect("physical GPU required");
    let (mut document, outer) = nested();
    document.layers[0].properties.blend = LayerBlend::Normal;
    document.layers[0].opacity = 0.7;
    let mut mask = LayerMask::reveal_all(document.allocate_layer_id(), Point { x: 9., y: 4. });
    mask.default_coverage = 0.25;
    mask.initial = Some(
        Selection::polygon(vec![
            Point { x: 20., y: 10. },
            Point { x: 290., y: 40. },
            Point { x: 240., y: 270. },
            Point { x: 30., y: 200. },
        ])
        .unwrap(),
    );
    let covered = coverage(&mut r, &mask);
    assert!(covered.iter().any(|c| *c > 0.99) && covered.iter().any(|c| (*c - 0.25).abs() < 1e-3));
    for (nested, opacity, masked) in [(false, 0.6, false), (false, 1., true), (true, 0.45, true)] {
        let mut faded = set(&document, outer, |g| {
            g.opacity = opacity;
            g.mask = masked.then(|| mask.clone());
        });
        if nested {
            let mut b = Builder(faded);
            let around = b.group("Around", None, LayerBlend::Normal);
            b.layer(around).opacity = 0.7;
            let group = b.0.layers.remove(b.0.layers.iter().position(|l| l.id == around).unwrap());
            let at = b.0.layers.iter().position(|l| l.id == outer).unwrap();
            b.0.layers.insert(at, group);
            b.layer(outer).properties.parent = Some(around);
            faded = b.0;
        }
        let without = render(&mut r, &set(&faded, outer, |g| g.visible = false));
        let full = render(&mut r, &set(&faded, outer, |g| {
            g.opacity = 1.;
            g.mask = None;
        }));
        let expected: Vec<[f32; 4]> = without
            .iter()
            .zip(&full)
            .zip(&covered)
            .map(|((a, b), m)| {
                let k = opacity * if masked { *m } else { 1. };
                std::array::from_fn(|c| a[c] + k * (b[c] - a[c]))
            })
            .collect();
        let actual = render(&mut r, &faded);
        let what = format!("nested {nested}, opacity {opacity}, mask {masked}");
        assert_close(&actual, &expected, 5e-5, &what);
        assert_close(&exported(&r, &faded), &actual, 2e-5, &format!("export, {what}"));
    }
}

#[test]
fn a_clipped_pass_through_group_composites_isolated() {
    let mut r = WgpuRasterizer::new_native_headless(COLOR).expect("physical GPU required");
    let mut b = Builder::new();
    let group = b.group("Clipped", None, LayerBlend::PassThrough);
    b.layer(group).properties.clipped = true;
    b.layer(group).opacity = 0.75;
    b.effect("Black & White", Some(group), "black_white");
    b.paint("Multiply", Some(group), LayerBlend::Multiply, 5, disc(150, 140, 110));
    b.paint("Base", None, LayerBlend::Normal, 11, disc(140, 150, 120));
    b.paint("Backdrop", None, LayerBlend::Normal, 19, |_, _| 65535);
    let clipped = render(&mut r, &b.0);
    let normal = render(&mut r, &set(&b.0, group, |g| g.properties.blend = LayerBlend::Normal));
    assert_close(&clipped, &normal, 1e-6, "a clipped Pass Through group against a clipped Normal group");
    let unclipped = render(&mut r, &set(&b.0, group, |g| g.properties.clipped = false));
    assert!(largest_difference(&clipped, &unclipped) > 0.1, "unclipped, the group passes through");
}
