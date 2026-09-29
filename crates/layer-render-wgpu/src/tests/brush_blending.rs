//! How dabs lay over paint follows the document's Blending: soft edges and
//! opacity build up on encoded values in Perceptual documents, checked against
//! a CPU reference, and a brush blend mode equals the layer blend mode.
use super::*;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, BrushBlendMode, LayerBlend, StrokeId};

const EXTENT: [u32; 2] = [128, 128];
const WHITE: [f32; 4] = [1.; 4];

struct Canvas {
    r: WgpuRasterizer,
    layers: Vec<Layer>,
    space: BlendSpace,
    strokes: u64,
}

impl Canvas {
    fn new(color: DocumentColor, space: BlendSpace, layers: Vec<Layer>) -> Self {
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.submit(FramePacket { reset_layers: true, blend_space: space, ..packet(&layers, EXTENT) }).unwrap();
        Self { r, layers, space, strokes: 0 }
    }

    fn paint(&mut self, layer: u64, mut style: DabStyle, dab: Dab) {
        style.blend_space = self.space;
        self.strokes += 1;
        let damage = Rect { min: Point { x: 0., y: 0. }, max: Point { x: EXTENT[0] as f32, y: EXTENT[1] as f32 } };
        let batch = DabBatch { stroke_id: StrokeId(self.strokes), ..crate::test_support::dab_batch(LayerId(layer), style, damage) };
        self.r
            .submit(FramePacket { dabs: &[dab], dab_batches: &[batch], blend_space: self.space, ..packet(&self.layers, EXTENT) })
            .unwrap();
    }

    fn fill(&mut self, layer: u64, color: [f32; 4]) {
        let mut dab = test_dab([64., 64.], color, 1.);
        dab.radii = [200.; 2];
        self.paint(layer, test_style(BrushExecution::Dry), dab);
    }

    /// The composite, as the document's blend space holds it.
    fn composite(&mut self) -> Vec<[f64; 4]> {
        self.r.submit(FramePacket { blend_space: self.space, ..packet(&self.layers, EXTENT) }).unwrap();
        crate::layer_tests::page_bytes(&self.r, self.r.composite_texture.as_ref().unwrap())
            .chunks_exact(16)
            .map(|p| std::array::from_fn(|c| f64::from(f32::from_le_bytes(p[c * 4..][..4].try_into().unwrap()))))
            .collect()
    }
}

fn soft(center: [f32; 2], color: [f32; 4], radius: f32, hardness: f32) -> Dab {
    let mut dab = test_dab(center, color, 1.);
    dab.radii = [radius; 2];
    dab.hardness = hardness;
    dab
}

/// `analytic_coverage` of a round dab at the center of pixel `[x, y]`.
fn coverage(dab: &Dab, [x, y]: [usize; 2]) -> f64 {
    let radius = f64::from(dab.radii[0]);
    let local = [(x as f64 + 0.5 - f64::from(dab.center.x)) / radius, (y as f64 + 0.5 - f64::from(dab.center.y)) / radius];
    let squared = local[0] * local[0] + local[1] * local[1];
    if squared >= 1. {
        return 0.;
    }
    let edge = (1. - f64::from(dab.hardness)).max(1. / radius);
    let solid = (1. - edge).max(0.);
    if squared <= solid * solid { 1. } else { ((1. - squared.sqrt()) / edge).clamp(0., 1.) }
}

#[test]
fn a_soft_black_edge_over_white_fades_on_the_documents_values() {
    for space in BlendSpace::ALL {
        let mut canvas = Canvas::new(DocumentColor::default(), space, vec![Layer::paint(LayerId(1), "Paint")]);
        canvas.fill(1, WHITE);
        let dab = soft([61.3, 66.7], [0., 0., 0., 1.], 50., 0.1);
        canvas.paint(1, test_style(BrushExecution::Dry), dab);
        let composite = canvas.composite();
        let mut faded = 0;
        for y in 0..EXTENT[1] as usize {
            for x in 0..EXTENT[0] as usize {
                let alpha = coverage(&dab, [x, y]);
                let expected = 1. - alpha;
                let actual = composite[y * EXTENT[0] as usize + x];
                assert!((actual[1] - expected).abs() < 2e-4, "{space:?} at {x},{y}: {} != {expected}", actual[1]);
                faded += usize::from(alpha > 0.1 && alpha < 0.9);
            }
        }
        assert!(faded > 1000, "{space:?}: the profile crosses {faded} pixels of the soft edge");
    }
}

#[test]
fn black_at_half_opacity_over_white_paint_is_middle_gray_when_blending_perceptually() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for (space, expected) in [(BlendSpace::Perceptual, 128.), (BlendSpace::Linear, 188.)] {
            let color = DocumentColor { space: RgbSpace::Srgb, depth };
            let mut canvas = Canvas::new(color, space, vec![Layer::paint(LayerId(1), "Paint")]);
            canvas.fill(1, WHITE);
            canvas.paint(1, test_style(BrushExecution::Dry), soft([64., 64.], [0., 0., 0., 0.5], 40., 1.));
            canvas.composite();
            let image = canvas.r.readback_srgb_rgba8().unwrap();
            let center = (64 * EXTENT[0] as usize + 64) * 4;
            let code = f32::from(image[center]);
            assert!((code - expected).abs() <= 1., "{depth:?} {space:?}: {code}, not {expected}");
        }
    }
}

#[test]
fn a_brush_blend_mode_equals_the_layer_blend_mode_in_both_spaces() {
    let base = |canvas: &mut Canvas, layer| {
        canvas.fill(layer, [0.3, 0.55, 0.2, 1.]);
        canvas.paint(layer, test_style(BrushExecution::Dry), soft([40., 60.], [0.9, 0.1, 0.6, 1.], 70., 0.));
        canvas.paint(layer, test_style(BrushExecution::Dry), soft([100., 90.], [0.05, 0.08, 0.5, 0.8], 50., 0.3));
    };
    let stroke = soft([70., 64.], [0.85, 0.7, 0.15, 0.75], 55., 0.2);
    for space in BlendSpace::ALL {
        for mode in [BrushBlendMode::Overlay, BrushBlendMode::Multiply, BrushBlendMode::Screen, BrushBlendMode::Darken, BrushBlendMode::Lighten] {
            let mut brush = Canvas::new(DocumentColor::default(), space, vec![Layer::paint(LayerId(1), "Paint")]);
            base(&mut brush, 1);
            let mut style = test_style(BrushExecution::Dry);
            style.rendering.blend_mode = mode;
            brush.paint(1, style, stroke);
            let mut over = Layer::paint(LayerId(2), "Blend");
            over.properties.blend = LayerBlend::from(mode);
            let mut layered = Canvas::new(DocumentColor::default(), space, vec![over, Layer::paint(LayerId(1), "Paint")]);
            base(&mut layered, 1);
            layered.paint(2, test_style(BrushExecution::Dry), stroke);
            for (i, (a, b)) in brush.composite().into_iter().zip(layered.composite()).enumerate() {
                for c in 0..4 {
                    assert!((a[c] - b[c]).abs() < 2e-4, "{space:?} {mode:?} pixel {i} channel {c}: brush {} != layer {}", a[c], b[c]);
                }
            }
        }
    }
}

#[test]
fn erasing_is_the_same_in_both_spaces() {
    let erased = |space| {
        let mut canvas = Canvas::new(DocumentColor::default(), space, vec![Layer::paint(LayerId(1), "Paint")]);
        canvas.fill(1, [0.7, 0.2, 0.4, 0.9]);
        let mut style = test_style(BrushExecution::Dry);
        style.mode = DabMode::Erase;
        canvas.paint(1, style, soft([64., 64.], WHITE, 45., 0.1));
        canvas.r.readback_srgb_rgba8().unwrap()
    };
    assert_eq!(erased(BlendSpace::Perceptual), erased(BlendSpace::Linear));
}

#[test]
fn edged_brushes_lay_dabs_in_the_material_pass_in_perceptual_documents() {
    let mut style = test_style(BrushExecution::Dry);
    style.rendering.wet_edge = 0.6;
    assert!(BrushPassPlan::for_style(&style).direct.is_some());
    style.blend_space = BlendSpace::Perceptual;
    let plan = BrushPassPlan::for_style(&style);
    assert!(plan.direct.is_none() && plan.material == MaterialOperation::Deposit);
}
