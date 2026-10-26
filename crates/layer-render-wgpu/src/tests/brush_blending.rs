//! How dabs lay over paint follows the document's Blending: soft edges and
//! opacity build up on encoded values in Perceptual documents, checked against
//! a CPU reference, and a brush blend mode equals the layer blend mode.
use super::*;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, BrushBlendMode, Document, LayerBlend, StrokeId, authored::{Artwork, SourceTarget}};

const EXTENT: [u32; 2] = [128, 128];
const WHITE: [f32; 4] = [1.; 4];

pub(super) struct Canvas {
    pub(super) r: WgpuRasterizer,
    document: Document,
    pub(super) base: SourceTarget,
    over: Option<SourceTarget>,
    space: BlendSpace,
    strokes: u64,
    extent: [u32;2],
}

impl Canvas {
    pub(super) fn new(color: DocumentColor, space: BlendSpace, blend: Option<LayerBlend>) -> Self {
        Self::at(color,space,blend,EXTENT)
    }

    pub(super) fn at(color:DocumentColor,space:BlendSpace,blend:Option<LayerBlend>,extent:[u32;2])->Self {
        let mut artwork = Artwork::new(extent).unwrap();
        let composition = artwork.compositions.get_mut(artwork.root).unwrap();
        composition.color = color;
        composition.blend = space;
        let over = blend.map(|blend| {
            let (owner, target) = crate::test_support::add_paint(&mut artwork, "Blend", extent);
            artwork.occurrences.get_mut(owner).unwrap().blend = blend;
            target
        });
        let (_, base) = crate::test_support::add_paint(&mut artwork, "Paint", extent);
        let document = Document::from_artwork(artwork).unwrap();
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.submit(FramePacket { reset_layers: true, blend_space: space, ..packet(document.scene(), extent) }).unwrap();
        Self { r, document, base, over, space, strokes: 0, extent }
    }

    pub(super) fn paint(&mut self, target: SourceTarget, mut style: DabStyle, dab: Dab) {
        style.blend_space = self.space;
        self.strokes += 1;
        let damage = Rect { min: Point { x: 0., y: 0. }, max: Point { x: self.extent[0] as f32, y: self.extent[1] as f32 } };
        let batch = DabBatch { stroke_id: StrokeId(self.strokes), ..crate::test_support::dab_batch(target, style, damage) };
        self.r
            .submit(FramePacket { dabs: &[dab], dab_batches: &[batch], blend_space: self.space, ..packet(self.document.scene(), self.extent) })
            .unwrap();
    }

    pub(super) fn stroke(&mut self, id: u64, brush: &layer_core::BrushSnapshot, dabs: &[Dab], chunk: usize, erase: bool) {
        if dabs.is_empty() {return;}
        let style=DabStyle::for_brush(brush,if erase {layer_core::StrokeTool::Eraser} else {layer_core::StrokeTool::Brush});
        self.styled_stroke(id,style,dabs,chunk);
    }

    pub(super) fn styled_stroke(&mut self,id:u64,mut style:DabStyle,dabs:&[Dab],chunk:usize) {
        style.blend_space=self.space;
        for (i,contacts) in dabs.chunks(chunk).enumerate() {
            let damage=contacts.iter().fold(Rect::EMPTY,|r,d|r.union(d.bounds()));
            let batch=DabBatch {stroke_id:StrokeId(id),dab_count:contacts.len() as u32,
                stroke_start:i==0,stroke_end:(i+1)*chunk>=dabs.len(),
                ..crate::test_support::dab_batch(self.base,style.clone(),damage)};
            crate::test_support::submit_ready(&mut self.r, FramePacket {dabs:contacts,dab_batches:std::slice::from_ref(&batch),
                blend_space:self.space,..packet(self.document.scene(),self.extent)}).unwrap();
        }
    }

    fn reset(&mut self) {
        crate::test_support::submit_ready(&mut self.r, FramePacket { reset_layers: true, blend_space: self.space, ..packet(self.document.scene(), self.extent) }).unwrap();
    }

    /// One frame of a stroke whose contacts arrive in batches of `chunk`, and
    /// its composite.
    fn frame(&mut self, mut style: DabStyle, dabs: &[Dab], chunk: usize, kind: DabBatchKind) -> Vec<[f64; 4]> {
        style.blend_space = self.space;
        self.strokes += 1;
        let batches: Vec<_> = dabs.chunks(chunk).enumerate().map(|(i, contacts)| DabBatch {
            stroke_id: StrokeId(self.strokes), kind, first_dab: (i * chunk) as u32, dab_count: contacts.len() as u32,
            stroke_start: i == 0, stroke_end: kind == DabBatchKind::Persistent && (i + 1) * chunk >= dabs.len(),
            ..crate::test_support::dab_batch(self.base, style.clone(), contacts.iter().fold(Rect::EMPTY, |r, d| r.union(d.bounds())))
        }).collect();
        self.r.submit(FramePacket { dabs, dab_batches: &batches, blend_space: self.space, ..packet(self.document.scene(), self.extent) }).unwrap();
        self.pixels()
    }

    pub(super) fn fill(&mut self, target: SourceTarget, color: [f32; 4]) {
        let mut dab = test_dab([64., 64.], color, 1.);
        dab.radii = [200.; 2];
        self.paint(target, test_style(BrushExecution::Dry), dab);
    }

    /// The composite, as the document's blend space holds it.
    pub(super) fn composite(&mut self) -> Vec<[f64; 4]> {
        self.r.submit(FramePacket { blend_space: self.space, ..packet(self.document.scene(), self.extent) }).unwrap();
        self.pixels()
    }

    fn pixels(&self) -> Vec<[f64;4]> {
        crate::layer_tests::page_bytes(&self.r, crate::test_support::document_texture(&self.r))
            .as_chunks::<16>().0.iter()
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
fn uniform_stroke_colors_and_coverage_do_not_depend_on_contact_batching() {
    for space in BlendSpace::ALL {
        for (mode, blend) in [(DabMode::Paint, BrushBlendMode::Normal), (DabMode::Paint, BrushBlendMode::Overlay), (DabMode::Erase, BrushBlendMode::Normal)] {
            for varying in [false, true] {
                let dabs: Vec<_> = [0.2, 1., 0.4, 1.].into_iter().enumerate().map(|(i, alpha)| {
                    soft([40. + i as f32 * 11., 48. + i as f32 * 6.],
                        [0.2 + if varying { i as f32 * 0.2 } else { 0. }, 0.3, 0.8, alpha], 52., 0.3)
                }).collect();
                let draw = |chunk: usize| {
                    let mut canvas = Canvas::new(DocumentColor::default(), space, None);
                    canvas.fill(canvas.base, [0.3, 0.5, 0.1, 0.8]);
                    let mut style = test_style(BrushExecution::Dry);
                    style.blend_space = space;
                    style.mode = mode;
                    style.rendering.blend_mode = blend;
                    style.rendering.accumulation = BrushAccumulation::Uniform;
                    let damage = dabs.iter().fold(Rect::default(), |r, d| r.union(d.bounds()));
                    let mut batch = DabBatch { stroke_id: StrokeId(2), ..crate::test_support::dab_batch(canvas.base, style, damage) };
                    for (i, contacts) in dabs.chunks(chunk).enumerate() {
                        batch.dab_count = contacts.len() as u32;
                        batch.stroke_start = i == 0;
                        batch.stroke_end = (i + 1) * chunk >= dabs.len();
                        canvas.r.submit(FramePacket { dabs: contacts, dab_batches: std::slice::from_ref(&batch),
                            blend_space: space, ..packet(canvas.document.scene(), EXTENT) }).unwrap();
                    }
                    canvas.composite()
                };
                for (pixel, (a, b)) in draw(1).into_iter().zip(draw(dabs.len())).enumerate() {
                    for channel in 0..4 {
                        assert!((a[channel] - b[channel]).abs() < 2e-4,
                            "{space:?} {mode:?} {blend:?} varying={varying} pixel={pixel} channel={channel}: {a:?} != {b:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn dry_flow_batch_dose_matches_ordered_contacts_and_closed_form() {
    use layer_core::{BrushContact, BrushDryMedia, BrushDryPose, Selection, SelectionPixels};
    for underpaint in [[-0.12f32,0.18,1.5,0.6],[0.;4],[-0.15*0.6,0.7*0.6,1.9*0.6,0.6]] {
    let bytes: Vec<_> = (0..PAGE_SIZE*PAGE_SIZE).flat_map(|_| underpaint.into_iter().flat_map(f32::to_le_bytes)).collect();
    let selected = Arc::new(Selection::pixels(Arc::new(SelectionPixels::new(EXTENT,
        [0, 0, EXTENT[0], EXTENT[1]], vec![0x22222222; (EXTENT[0]*EXTENT[1]/8) as usize]).unwrap())));
    for space in BlendSpace::ALL {
        let mut canvas = Canvas::new(DocumentColor { depth: if space == BlendSpace::Linear { SampleDepth::F32 } else { SampleDepth::U8 }, ..Default::default() }, space, None);
        let r = &mut canvas.r;
        let base = r.create_page_surface("dose oracle underpaint");
        crate::test_support::upload_page(r, &base.texture, &bytes);
        let output = r.create_page_surface("dose oracle output");
        for mode in [DabMode::Paint, DabMode::Erase] {
            for (varying, locked, selection) in [(false, false, false), (true, false, false),
                (false, true, false), (false, false, true)] {
                let mut style = test_style(BrushExecution::Dry);
                style.mode = mode; style.blend_space = space; style.alpha_locked = locked;
                style.selection = selection.then(|| selected.clone());
                style.rendering.accumulation = BrushAccumulation::Flow;
                style.contact = Some(BrushContact { pressure_gain: 1., dry: Some(BrushDryMedia {
                    poses: [BrushDryPose { radii: [1.; 2], relief: [0.; 3] }; 3],
                    softness: 0.05, loading: 0., corner: 0., relief_offset: 0.,
                }), ..Default::default() });
                let dabs: Vec<_> = [(0., 0.1), (0.15, 0.02), (0.65, 0.3), (1., 0.12)]
                    .into_iter().enumerate().map(|(i, (alpha, flow))| {
                        let mut dab = test_dab([64.5; 2], [-0.15 + if varying { i as f32*0.12 } else { 0. }, 0.7, 1.9, alpha], flow);
                        dab.radii = [200.; 2]; dab.contact = [0.7, 0., 0., 0.]; dab.previous_contact = dab.contact;
                        dab
                    }).collect();
                let batch = DabBatch { dab_count: dabs.len() as u32, kind: DabBatchKind::Preview,
                    ..crate::test_support::dab_batch(canvas.base, style, Rect::from_extent(EXTENT)) };
                let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
                r.prepare_uploads(FramePacket { dabs: &dabs, dab_batches: std::slice::from_ref(&batch),
                    ..packet(canvas.document.scene(), EXTENT) }, &mut [Vec::new()], &mut encoder).unwrap();
                r.prepare_target_selection(&mut encoder, &batch.style, EXTENT).unwrap();
                r.prepare_records(std::iter::once(material_sources::dry_record(PixelRect::full(EXTENT), 0..dabs.len() as u32, [0; 16])), &mut encoder).unwrap();
                let key=r.ensure_texture_set(&batch.style).unwrap();
                r.uploads.finish(&encoder);
                encoder.submit(&r.queue);
                r.queue.write_buffer(&r.target_buffer, 0, target_bytes(&TargetGpu::new([0; 2], EXTENT, EXTENT)));
                let marker = mem::offset_of!(StyleGpu, material_a)+12;
                let mut record = r.style_upload[..mem::size_of::<StyleGpu>()].to_vec();
                let enabled = !locked && !selection && (mode == DabMode::Erase || !varying);
                assert_eq!(f32::from_ne_bytes(record[marker..marker+4].try_into().unwrap()), f32::from(enabled),
                    "{space:?}/{mode:?} varying={varying} locked={locked} selection={selection}: actual batch marker");
                let sources = create_material_bind_group(&r.device, &r.material_layout, &[&base.view; 9],
                    &r.dab_buffer, &r.empty_scalar_view, &r.empty_view,
                    wgpu::BindingResource::Buffer(r.material_records.binding()));
                let binding = r.pipelines.dry_material.output(r, &output, None, None);
                let mut rendered = Vec::new();
                for reference in [false, true] {
                    if reference { record[marker..marker+4].copy_from_slice(&0f32.to_ne_bytes()); }
                    r.queue.write_buffer(&r.style_buffer, 0, &record);
                    let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
                    base.copy_to(&output, &mut encoder);
                    { let mut pass = encoder.begin_compute_pass(&Default::default());
                        let pipeline = r.pipelines.dry_material.selected_kernel(&batch.style, MaterialOperation::Deposit, false, if enabled && !reference {8192} else {0});
                        pass.set_pipeline(pipeline);
                        pass.set_bind_group(0, &binding, &[0]);
                        pass.set_bind_group(1, r.paint_target_binding(&batch.style), &[0]);
                        pass.set_bind_group(2, &sources, &[0]);
                        pass.set_bind_group(3, &r.texture_sets.iter().find(|t|t.key==key).unwrap().bind_group, &[]);
                        pass.dispatch_workgroups(EXTENT[0].div_ceil(32), EXTENT[1].div_ceil(2), 1);
                    }
                    encoder.submit(&r.queue);
                    rendered.push(crate::test_support::float_pixels(r, &output.texture));
                }
                assert!(crate::test_support::max_error(&rendered[0], &rendered[1]) < 1e-6,
                    "{space:?}/{mode:?} varying={varying} locked={locked} selection={selection}: aggregate vs ordered contacts");
                let mut expected = underpaint.map(f64::from);
                if mode == DabMode::Paint && space == BlendSpace::Perceptual {
                    for c in 0..3 { expected[c] = if expected[3]>0. {RgbSpace::Srgb.encode(expected[c]/expected[3])*expected[3]} else {0.}; }
                }
                for dab in &dabs {
                    let alpha = (1.-(-6.*f64::from(dab.flow)*f64::from(dab.color_rgba_linear[3])).exp())*if selection {0.5} else {1.};
                    if mode == DabMode::Erase { if !locked { for value in &mut expected { *value *= 1.-alpha; } } }
                    else {
                        for c in 0..3 {
                            let color = if space == BlendSpace::Perceptual { RgbSpace::Srgb.encode(f64::from(dab.color_rgba_linear[c])) } else { f64::from(dab.color_rgba_linear[c]) };
                            expected[c] = expected[c]*(1.-alpha)+color*alpha*if locked {expected[3]} else {1.};
                        }
                        if !locked { expected[3] = alpha+expected[3]*(1.-alpha); }
                    }
                }
                if mode == DabMode::Paint && space == BlendSpace::Perceptual {
                    for c in 0..3 { expected[c] = if expected[3]>0. {RgbSpace::Srgb.decode(expected[c]/expected[3])*expected[3]} else {0.}; }
                }
                for y in 0..EXTENT[1] { for x in 0..EXTENT[0] {
                    let pixel = rendered[0][(y*PAGE_SIZE+x) as usize];
                    for c in 0..4 { assert!((f64::from(pixel[c])-expected[c]).abs() < 1e-6,
                        "{space:?}/{mode:?} varying={varying} locked={locked} selection={selection} channel={c}: GPU {pixel:?}, closed form {expected:?}"); }
                }}
            }
        }
    }
    }
}

#[test]
fn a_soft_black_edge_over_white_fades_on_the_documents_values() {
    for space in BlendSpace::ALL {
        let mut canvas = Canvas::new(DocumentColor::default(), space, None);
        canvas.fill(canvas.base, WHITE);
        let dab = soft([61.3, 66.7], [0., 0., 0., 1.], 50., 0.1);
        canvas.paint(canvas.base, test_style(BrushExecution::Dry), dab);
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
            let mut canvas = Canvas::new(color, space, None);
            canvas.fill(canvas.base, WHITE);
            canvas.paint(canvas.base, test_style(BrushExecution::Dry), soft([64., 64.], [0., 0., 0., 0.5], 40., 1.));
            canvas.composite();
            let image = canvas.r.readback_srgb_rgba8().unwrap();
            let center = (64 * EXTENT[0] as usize + 64) * 4;
            let code = f32::from(image[center]);
            assert!((code - expected).abs() <= 1., "{depth:?} {space:?}: {code}, not {expected}");
        }
    }
}

#[test]
fn a_brush_blend_mode_equals_the_layer_blend_mode_when_predicted_or_committed_and_erases_like_normal() {
    let extent = [384, 128];
    let base = |canvas: &mut Canvas, layer| {
        canvas.paint(layer, test_style(BrushExecution::Dry), soft([192., 64.], [0.3, 0.55, 0.2, 1.], 400., 1.));
        canvas.paint(layer, test_style(BrushExecution::Dry), soft([100., 60.], [0.9, 0.1, 0.6, 1.], 90., 0.));
        canvas.paint(layer, test_style(BrushExecution::Dry), soft([300., 90.], [0.05, 0.08, 0.5, 0.8], 70., 0.3));
    };
    let custom = layer_core::BrushContact { pooling: 0.25, linear_edge: true, ..Default::default() };
    layer_core::BrushSnapshot { contact: Some(custom), ..Default::default() }.validate().unwrap();
    for space in BlendSpace::ALL {
        let mut brush = Canvas::at(DocumentColor::default(), space, None, extent);
        for mode in [BrushBlendMode::Normal, BrushBlendMode::Overlay, BrushBlendMode::Multiply, BrushBlendMode::Screen, BrushBlendMode::Darken, BrushBlendMode::Lighten] {
            let mut layered = Canvas::at(DocumentColor::default(), space, Some(LayerBlend::from(mode)), extent);
            for (contact, accumulation) in [None, Some(custom)].into_iter()
                .flat_map(|contact| [BrushAccumulation::Flow, BrushAccumulation::Uniform].map(|accumulation| (contact, accumulation))) {
                let mut stroke = [soft([226., 64.], [0.85, 0.7, 0.15, 0.75], 40., 0.2), soft([320., 70.], [0.2, 0.6, 0.9, 0.9], 30., 0.5)];
                if contact.is_some() {
                    for dab in &mut stroke {
                        dab.previous = [dab.radii[0], dab.radii[1], 1., 0.];
                        dab.contact = [0.8, 0., 0., 0.];
                        dab.previous_contact = dab.contact;
                    }
                }
                let mut style = test_style(BrushExecution::Dry);
                style.contact = contact;
                style.rendering.accumulation = accumulation;
                let normal = style.clone();
                style.rendering.blend_mode = mode;
                layered.reset();
                let target = layered.base;
                base(&mut layered, target);
                for dab in stroke { layered.paint(layered.over.unwrap(), normal.clone(), dab); }
                let expected = layered.composite();
                brush.reset();
                let target = brush.base;
                base(&mut brush, target);
                let before = brush.composite();
                for (phase, kind, chunk) in [("one predicted batch", DabBatchKind::Preview, 2), ("two predicted batches", DabBatchKind::Preview, 1),
                    ("two committed batches", DabBatchKind::Persistent, 1)] {
                    if kind == DabBatchKind::Persistent {
                        assert_eq!(brush.composite(), before, "{space:?} {mode:?} {accumulation:?} contact={contact:?}: predictions leave committed paint unchanged");
                    }
                    let actual = brush.frame(style.clone(), &stroke, chunk, kind);
                    assert!(actual.iter().zip(&before).any(|(a, b)| (0..3).any(|c| (a[c] - b[c]).abs() > 0.01)),
                        "{space:?} {mode:?} {accumulation:?} contact={contact:?} {phase}: brush must change visible paint");
                    for (i, (a, b)) in actual.into_iter().zip(&expected).enumerate() {
                        for c in 0..4 {
                            assert!((a[c] - b[c]).abs() < 2e-4, "{space:?} {mode:?} {accumulation:?} contact={contact:?} {phase} pixel {i} channel {c}: brush {} != layer {}", a[c], b[c]);
                        }
                    }
                }
                let erased = [BrushBlendMode::Normal, mode].map(|blend| {
                    brush.reset();
                    let target = brush.base;
                    base(&mut brush, target);
                    let mut eraser = style.clone();
                    eraser.mode = DabMode::Erase;
                    eraser.rendering.blend_mode = blend;
                    brush.frame(eraser, &stroke, 1, DabBatchKind::Persistent)
                });
                assert!(erased[1].iter().zip(&before).any(|(a, b)| (a[3] - b[3]).abs() > 0.01), "{space:?} {mode:?} {accumulation:?} contact={contact:?}: erasing removes paint");
                for (i, (a, b)) in erased[1].iter().zip(&erased[0]).enumerate() {
                    for c in 0..4 {
                        assert!((a[c] - b[c]).abs() < 2e-4, "{space:?} {mode:?} {accumulation:?} contact={contact:?} pixel {i} channel {c}: erasing {} != erasing with Normal {}", a[c], b[c]);
                    }
                }
            }
        }
    }
}

#[test]
fn erasing_is_the_same_in_both_spaces() {
    let erased = |space| {
        let mut canvas = Canvas::new(DocumentColor::default(), space, None);
        canvas.fill(canvas.base, [0.7, 0.2, 0.4, 0.9]);
        let mut style = test_style(BrushExecution::Dry);
        style.mode = DabMode::Erase;
        canvas.paint(canvas.base, style, soft([64., 64.], WHITE, 45., 0.1));
        canvas.r.readback_srgb_rgba8().unwrap()
    };
    assert_eq!(erased(BlendSpace::Perceptual), erased(BlendSpace::Linear));
}

#[test]
#[ignore = "Requires hardware float32 blending for direct painting"]
fn direct_contact_paint_and_erase_match_destination_deposition() {
    let contact = layer_core::BrushContact { pooling: 0.25, linear_edge: true, ..Default::default() };
    layer_core::BrushSnapshot { contact: Some(contact), ..Default::default() }.validate().unwrap();
    for mode in [DabMode::Paint, DabMode::Erase] {
        let mut direct = Canvas::new(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None);
        let mut destination = Canvas::new(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None);
        direct.fill(direct.base, [0.3, 0.55, 0.2, 1.]);
        destination.fill(destination.base, [0.3, 0.55, 0.2, 1.]);
        let before = direct.composite();
        let mut style = test_style(BrushExecution::Dry);
        style.mode = mode;
        style.contact = Some(contact);
        style.rendering.accumulation = BrushAccumulation::Flow;
        style.rendering.wet_edge = 0.6;
        style.rendering.burnt_edge = 0.3;
        assert!(BrushPassPlan::for_device(&style, &direct.r.device).direct.is_some(), "{mode:?}: actual direct route");
        let mut reference = style.clone();
        if mode == DabMode::Paint { reference.alpha_locked = true; }
        else { reference.rendering.blend_mode = BrushBlendMode::Multiply; }
        assert!(BrushPassPlan::for_device(&reference, &destination.r.device).direct.is_none(), "{mode:?}: destination oracle");
        let mut dab = soft([70., 64.], [0.85, 0.7, 0.15, 0.75], 55., 0.2);
        dab.previous = [55., 55., 1., 0.];
        dab.contact = [0.8, 0., 0., 0.];
        dab.previous_contact = dab.contact;
        direct.paint(direct.base, style, dab);
        destination.paint(destination.base, reference, dab);
        let actual = direct.composite();
        assert!(actual.iter().zip(before).any(|(a, b)| (0..4).any(|c| (a[c] - b[c]).abs() > 0.01)),
            "{mode:?}: direct contact must change visible paint");
        for (pixel, (a, b)) in actual.into_iter().zip(destination.composite()).enumerate() {
            for channel in 0..4 {
                assert!((a[channel] - b[channel]).abs() < 2e-4,
                    "{mode:?} pixel={pixel} channel={channel}: direct {a:?} != destination {b:?}");
            }
        }
    }
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

#[test]
fn cropped_dry_dispatch_preserves_every_untouched_hdr_pixel() {
    use layer_core::{BrushContact, BrushDryMedia, BrushDryPose};
    let source_color=[-0.15f32,0.7,1.9,0.6];
    let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
    let r=&mut canvas.r;
    let base=r.create_page_surface("cropped oracle source");
    let output=r.create_page_surface("cropped oracle destination");
    for underpaint in [[-0.12f32,0.18,1.5,0.6],[0.;4]] {
    let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|underpaint.into_iter().flat_map(f32::to_le_bytes)).collect();
    crate::test_support::upload_page(r,&base.texture,&bytes);
    for dirty in [PixelRect::new(37,41,91,84),PixelRect::new(0,0,19,17),
        PixelRect::new(0,41,91,42),PixelRect::new(37,0,38,84),PixelRect::new(31,33,50,50)] {
    for mode in [DabMode::Paint,DabMode::Erase] {
        let mut style=test_style(BrushExecution::Dry);
        style.mode=mode;style.blend_space=BlendSpace::Linear;style.rendering.accumulation=BrushAccumulation::Flow;
        style.contact=Some(BrushContact {pressure_gain:1.,dry:Some(BrushDryMedia {
            poses:[BrushDryPose {radii:[1.;2],relief:[0.;3]};3],softness:0.05,loading:0.,corner:0.,relief_offset:0.,
        }),..Default::default()});
        let mut dab=test_dab([64.5;2],source_color,0.2);
        dab.radii=[200.;2];dab.contact=[0.7,0.,0.,0.];dab.previous_contact=dab.contact;
        for inplace in [false,true] {
            let mut batch=crate::test_support::dab_batch(canvas.base,style.clone(),dirty.to_rect());
            if !inplace {batch.kind=DabBatchKind::Preview;}
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.prepare_uploads(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),
                ..packet(canvas.document.scene(),EXTENT)},&mut [Vec::new()],&mut encoder).unwrap();
            r.prepare_records(std::iter::once(material_sources::dry_record(dirty,0..1,[0;16])),&mut encoder).unwrap();
            r.ensure_texture_set(&batch.style).unwrap();
            r.uploads.finish(&encoder);encoder.submit(&r.queue);
            r.queue.write_buffer(&r.target_buffer,0,target_bytes(&TargetGpu::new([0;2],EXTENT,EXTENT)));
            assert_eq!(r.in_place_dry_material(&batch),inplace,"reference hardware supports in-place storage");
            let stale:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).flat_map(|_|[9f32,-7.,3.,0.9].into_iter().flat_map(f32::to_le_bytes)).collect();
            crate::test_support::upload_page(r,&output.texture,if inplace {&bytes} else {&stale});
            let source=if underpaint==[0.;4] {&r.empty_view} else {&base.view};
            let sources=create_material_bind_group(&r.device,&r.material_layout,&[source;9],&r.dab_buffer,
                &r.empty_scalar_view,&r.empty_view,wgpu::BindingResource::Buffer(r.material_records.binding()));
            let job=crate::dry_material::Job {output:r.pipelines.dry_material.output(r,&output,None,None),source:sources,display:None,
                coordinate:[0;2],coverage:false,record_offset:0,dirty};
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.encode_dry_material_jobs(&mut encoder,0,&batch,&[job]);
            encoder.submit(&r.queue);
            let actual=crate::test_support::float_pixels(r,&output.texture);
            let remaining=(-6f64*f64::from(dab.flow)*f64::from(source_color[3])).exp();
            for y in 0..PAGE_SIZE {for x in 0..PAGE_SIZE {
                let pixel=actual[(y*PAGE_SIZE+x) as usize];
                let inside=x>=dirty.min_x()&&x<dirty.max_x()&&y>=dirty.min_y()&&y<dirty.max_y();
                if !inside {assert_eq!(pixel.map(f32::to_bits),underpaint.map(f32::to_bits),
                    "{mode:?} inplace={inplace} underpaint={underpaint:?} dirty={dirty:?}: unchanged HDR pixel ({x},{y})");continue;}
                let expected=if mode==DabMode::Erase {underpaint.map(|v|f64::from(v)*remaining)} else {
                    std::array::from_fn(|c|f64::from(underpaint[c])*remaining+
                        (if c==3 {1.} else {f64::from(source_color[c])})*(1.-remaining))};
                for c in 0..4 {assert!((f64::from(pixel[c])-expected[c]).abs()<1e-6,
                    "{mode:?} inplace={inplace} underpaint={underpaint:?} dirty={dirty:?}: dirty HDR pixel ({x},{y}) channel{c}: {pixel:?} != {expected:?}");}
            }}
        }
    }}}
}

#[test]
fn cropped_display_restores_hdr_blocks_at_partial_document_edges() {
    use layer_core::{BrushContact,BrushDryMedia,BrushDryPose};
    let extent=[131,129];
    let underpaint=|x:u32,y:u32|[-0.12+x as f32*0.002,0.18+y as f32*0.003,1.5+(x+y) as f32*0.001,0.6];
    let bytes:Vec<_>=(0..PAGE_SIZE).flat_map(|y|(0..PAGE_SIZE).flat_map(move|x|
        underpaint(x,y).into_iter().flat_map(f32::to_le_bytes))).collect();
    let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,extent);
    let r=&mut canvas.r;
    let base=r.create_page_surface("display HDR source");
    crate::test_support::upload_page(r,&base.texture,&bytes);
    let dirty=PixelRect::new(37,41,extent[0],extent[1]);
    let color=[-0.15f32,0.7,1.9,0.6];
    for block in [2u32,4] {for (mode,contribution) in [(DabMode::Paint,false),(DabMode::Erase,false),(DabMode::Paint,true)] {
        r.preview_level=block.trailing_zeros();r.preview_contribution=contribution;
        let mut style=test_style(BrushExecution::Dry);style.mode=mode;style.blend_space=BlendSpace::Linear;
        style.rendering.accumulation=BrushAccumulation::Flow;
        style.contact=Some(BrushContact {pressure_gain:1.,dry:Some(BrushDryMedia {
            poses:[BrushDryPose {radii:[1.;2],relief:[0.;3]};3],softness:0.05,loading:0.,corner:0.,relief_offset:0.,
        }),..Default::default()});
        let mut dab=test_dab([64.5;2],color,0.2);dab.radii=[200.;2];
        dab.contact=[0.7,0.,0.,0.];dab.previous_contact=dab.contact;
        let mut batch=crate::test_support::dab_batch(canvas.base,style,dirty.to_rect());batch.kind=DabBatchKind::Preview;
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.prepare_uploads(FramePacket {dabs:std::slice::from_ref(&dab),dab_batches:std::slice::from_ref(&batch),
            ..packet(canvas.document.scene(),extent)},&mut [Vec::new()],&mut encoder).unwrap();
        r.prepare_records(std::iter::once(material_sources::dry_record(dirty,0..1,[0;16])),&mut encoder).unwrap();
        r.ensure_texture_set(&batch.style).unwrap();
        r.uploads.finish(&encoder);encoder.submit(&r.queue);
        r.queue.write_buffer(&r.target_buffer,0,target_bytes(&TargetGpu::new([0;2],extent,extent)));
        let size=[PAGE_SIZE/block;2];
        let output=create_page_surface(&r.device,&r.float_texture_layout,&r.sampler,size,r.device.working_format(),"stale display HDR output");
        let stale:Vec<_>=(0..size[0]*size[1]).flat_map(|_|[9f32,-7.,3.,0.9].into_iter().flat_map(f32::to_le_bytes)).collect();
        r.queue.write_texture(output.texture.as_image_copy(),&stale,wgpu::TexelCopyBufferLayout {
            offset:0,bytes_per_row:Some(size[0]*16),rows_per_image:Some(size[1]),
        },output.texture.size());
        let sources=create_material_bind_group(&r.device,&r.material_layout,&[&base.view;9],&r.dab_buffer,
            &r.empty_scalar_view,&r.empty_view,wgpu::BindingResource::Buffer(r.material_records.binding()));
        let output_binding=r.pipelines.dry_material.output(r,&output,None,None);
        let display=crate::test_support::dry_display(r,&batch,[&base.view;9],&output);
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.encode_dry_material_jobs(&mut encoder,0,&batch,&[crate::dry_material::Job {output:output_binding,source:sources,display,coordinate:[0;2],
            coverage:false,record_offset:0,dirty}]);
        encoder.submit(&r.queue);
        let actual=crate::test_support::float_pixels(r,&output.texture);
        let remaining=(-6f64*f64::from(dab.flow)*f64::from(color[3])).exp();
        for y in 0..size[1] {for x in 0..size[0] {
            let start=[x*block,y*block];
            let end=[(start[0]+block).min(extent[0]),(start[1]+block).min(extent[1])];
            let mut expected=[0f64;4];
            if !contribution && start[0]<end[0] && start[1]<end[1] {
                for py in start[1]..end[1] {for px in start[0]..end[0] {
                    let pixel=underpaint(px,py);for c in 0..4 {expected[c]+=f64::from(pixel[c]);}
                }}
                let count=f64::from((end[0]-start[0])*(end[1]-start[1]));for v in &mut expected {*v/=count;}
            }
            let center=[start[0]+block/2,start[1]+block/2];
            if center[0]>=dirty.min_x() && center[0]<dirty.max_x() && center[1]>=dirty.min_y() && center[1]<dirty.max_y() {
                expected=if mode==DabMode::Erase {expected.map(|v|v*remaining)} else {
                    std::array::from_fn(|c|expected[c]*remaining+(if c==3 {1.} else {f64::from(color[c])})*(1.-remaining))};
            }
            let pixel=actual[(y*size[0]+x) as usize];
            for c in 0..4 {assert!((f64::from(pixel[c])-expected[c]).abs()<1e-6,
                "block{block} {mode:?} contribution={contribution} HDR({x},{y}) channel{c}: {pixel:?} != {expected:?}");}
        }}
    }}
}


#[test]
fn independent_full_page_dispatch_preserves_contact_hull_pixels() {
    use layer_core::DefaultBrushPreset as P;
    for (name,preset,center,radii,motion,angle,old_angle,large) in [
        ("tangent",P::Pencil,[255.49,256.49],[4.1,4.1],[180.,0.],0.37,0.37,false),
        ("thin-rotating",P::RoughGPen,[260.,258.],[41.,1.2],[180.,160.],1.1,-0.7,false),
        ("pooled-offcanvas",P::BlottyInk,[1.,260.],[3.,49.],[-140.,180.],-0.8,0.6,false),
        ("fine-AA",P::Pencil,[257.,255.],[2.6,1.3],[119.,-85.],0.87,-0.42,false),
        ("rounded",P::PastelBlock,[256.7,257.1],[72.,23.],[150.,100.],0.73,-0.37,false),
        ("wide-turn",P::Charcoal,[253.,260.],[132.,28.],[210.,-140.],0.94,-0.9,false),
        ("large-coordinates",P::Pencil,[16128.49,256.49],[11.,3.],[180.,-90.],0.51,-0.66,true),
    ] {
        let angle:f32=angle;let old_angle:f32=old_angle;
        let extent=if large {[16384,512]} else {[512,512]};
        let mut canvas=Canvas::at(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None,extent);
        let mut style=crate::layer_tests::preset_style(preset);
        style.rendering.accumulation=BrushAccumulation::Flow;
        if style.contact.unwrap().dry.is_none() {let c=style.contact.as_mut().unwrap();c.edge_roughness=0.9;c.pooling=1.;}
        let mut dab=test_dab(center,[-0.15,0.7,1.9,0.6],0.41);
        dab.radii=radii;dab.motion=motion;dab.rotation=[angle.cos(),angle.sin()];
        dab.previous=[radii[0]*0.82,radii[1]*1.17,old_angle.cos(),old_angle.sin()];
        dab.contact=[0.73,0.2,0.4,17.];dab.previous_contact=[0.41,0.7,0.1,17.];
        if let Some(d)=style.contact.unwrap().dry {let v=d.pose(0.2).relief;let old=d.pose(0.7).relief;
            dab.contact=[0.73,v[0],v[1],v[2]];dab.previous_contact=[0.41,old[0],old[1],old[2]];}
        canvas.paint(canvas.base,style.clone(),dab);
        let batch=DabBatch {kind:DabBatchKind::Preview,..crate::test_support::dab_batch(canvas.base,style.clone(),Rect {min:Point{x:0.,y:0.},max:Point{x:extent[0] as f32,y:512.}})};
        let r=&mut canvas.r;let base=r.create_page_surface("independent zero source");let output=r.create_page_surface("uncropped oracle");
        crate::test_support::upload_page(r,&base.texture,&vec![0;256*256*16]);
        let coordinates:Vec<_>=if large {(62..64).flat_map(|x|(0..2).map(move|y|[x,y])).collect()}
            else {(0..2).flat_map(|x|(0..2).map(move|y|[x,y])).collect()};
        let actual:Vec<_>=coordinates.iter().map(|coord|r.paint_layers.iter().find(|l|l.id==canvas.base)
            .and_then(|l|l.pages.iter().find(|p|p.coordinate==*coord)).map(|p|crate::layer_tests::page_bytes(r,&p.active().texture))
            .unwrap_or_else(||vec![0;256*256*16])).collect();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        r.prepare_uploads(FramePacket {dabs:&[dab],dab_batches:std::slice::from_ref(&batch),..packet(canvas.document.scene(),extent)},&mut [Vec::new()],&mut encoder).unwrap();
        r.prepare_records(std::iter::once(material_sources::dry_record(PixelRect::full([256;2]),0..1,[0;16])),&mut encoder).unwrap();
        let key=r.ensure_texture_set(&style).unwrap();r.uploads.finish(&encoder);encoder.submit(&r.queue);
        let marker=mem::offset_of!(StyleGpu,material_a)+12;
        let aggregate=f32::from_ne_bytes(r.style_upload[marker..marker+4].try_into().unwrap())>0.5&&style.contact.unwrap().dry.is_some();
        let fine=style.contact.unwrap().dry.is_some_and(|d|d.corner==0.)&&(0..2).any(|i|0.5*(dab.radii[i]+dab.previous[i])<8.);
        let mut painted=0usize;let mut maximum=0f32;
        for (i,coord) in coordinates.iter().enumerate() {
            r.queue.write_buffer(&r.target_buffer,0,target_bytes(&TargetGpu::new([coord[0]*256,coord[1]*256],extent,extent)));
            let sources=create_material_bind_group(&r.device,&r.material_layout,&[&base.view;9],&r.dab_buffer,&r.empty_scalar_view,&r.empty_view,wgpu::BindingResource::Buffer(r.material_records.binding()));
            let kernels=&r.pipelines.dry_material;let binding=kernels.output(r,&output,None,None);
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            base.copy_to(&output,&mut encoder);
            {let mut pass=encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {label:Some("full independent page"),timestamp_writes:None});
                let pipeline = kernels.selected_kernel(&style,MaterialOperation::Deposit,false,if aggregate {8192}else{0}|if fine{16384}else{0});
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0,&binding,&[0]);pass.set_bind_group(1,r.paint_target_binding(&style),&[0]);pass.set_bind_group(2,&sources,&[0]);
                pass.set_bind_group(3,&r.texture_sets.iter().find(|s|s.key==key).unwrap().bind_group,&[]);pass.dispatch_workgroups(8,128,1);}
            encoder.submit(&r.queue);let reference=crate::test_support::float_pixels(r,&output.texture);
            painted+=reference.iter().filter(|p|p[3]>0.).count();
            for (pixel,(a,b)) in actual[i].as_chunks::<16>().0.iter().zip(reference).enumerate() {let a:[f32;4]=std::array::from_fn(|c|f32::from_le_bytes(a[c*4..c*4+4].try_into().unwrap()));
                assert_eq!(a[3]==0.,b[3]==0.,"{name} tile{coord:?} pixel{pixel}: strict zero support");
                for c in 0..4 {maximum=maximum.max((a[c]-b[c]).abs());assert!((a[c]-b[c]).abs()<=1e-6,"{name} tile{coord:?} pixel{pixel} channel{c}: clipped{a:?} != uncropped{b:?}");}}
        }
        assert!(painted>16,"{name}: nonempty independent reference");println!("{name}: painted={painted} maximum_RGBA_error={maximum}");
    }
}

fn opacity_brushes() -> [(&'static str,layer_core::BrushSnapshot);3] {
    use layer_core::DefaultBrushPreset as P;
    let pencil=layer_core::default_brush(P::Pencil);
    let mut uniform=layer_core::default_brush(P::GPen);
    uniform.rendering.accumulation=BrushAccumulation::Uniform;
    let mut analytic=uniform.clone();analytic.contact=None;analytic.grain=None;
    analytic.rendering.accumulation=BrushAccumulation::Flow;analytic.hardness=1.;
    analytic.mappings=Arc::from([]);
    [("Pencil",pencil),("analytic",analytic),("Uniform G-Pen",uniform)].map(|(name,mut b)| {
        b.diameter=64.;b.opacity=0.4;b.flow=1.;b.color_rgba_linear=[0.7,0.2,0.4,1.];(name,b)
    })
}

fn opacity_contacts(brush:&layer_core::BrushSnapshot,retraces:usize)->Vec<Dab> {
    use layer_core::{Stroke,StrokePoint,StrokeTool,authored::PaintHandle};
    let points:Vec<StrokePoint>=(0..=retraces*2).map(|i|StrokePoint {
        position:Point {x:if i%2==0 {32.5} else {95.5},y:64.5},pressure:1.,
        tilt:[std::f32::consts::FRAC_PI_4,0.],twist:0.,elapsed_micros:i as u32*16_000,
    }).collect();
    let stroke=Stroke::new(StrokeId(7),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),points).unwrap();
    let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,RgbSpace::Srgb,&mut dabs);
    if brush.flow>0. && brush.opacity>0. {assert!(!dabs.is_empty());}dabs
}

#[test]
fn engine_generated_opacity_caps_retraces_and_restarts_for_new_strokes() {
    let center=(64*EXTENT[0]+64) as usize;
    for (name,brush) in opacity_brushes() {for space in BlendSpace::ALL {
        let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
        let dabs=opacity_contacts(&brush,64);
        canvas.stroke(7,&brush,&dabs,dabs.len(),false);let once=canvas.composite();
        for (i,p) in once.iter().enumerate() {assert!(p[3]<=0.40005,"{name}/{space:?}: pixel{i} exceeded stroke ceiling: {p:?}");}
        assert!((once[center][3]-0.4).abs()<0.005,"{name}/{space:?}: repeated stroke reaches ceiling: {:?}",once[center]);
        canvas.stroke(8,&brush,&dabs,dabs.len(),false);let twice=canvas.composite();
        assert!((twice[center][3]-0.64).abs()<0.005,"{name}/{space:?}: separate strokes build beyond ceiling: {:?}",twice[center]);
        let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
        canvas.fill(canvas.base,[0.1,0.6,0.3,0.6]);let before=canvas.composite();
        canvas.stroke(7,&brush,&dabs,dabs.len(),false);let painted=canvas.composite();
        assert!((painted[center][3]-0.76).abs()<0.005,"{name}/{space:?}: opacity is relative to existing paint: {:?}",painted[center]);
        if space==BlendSpace::Linear {for c in 0..3 {
            let expected=before[center][c]*0.6+f64::from(brush.color_rgba_linear[c])*0.4;
            assert!((painted[center][c]-expected).abs()<0.005,"{name}: underpaint channel{c}: {:?}, expected{expected}",painted[center]);
        }}
        assert_eq!(painted[0],before[0],"{name}/{space:?}: distant underpaint");
        let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
        canvas.fill(canvas.base,[0.1,0.6,0.3,0.6]);let before=canvas.composite();
        canvas.stroke(7,&brush,&dabs,dabs.len(),true);let erased=canvas.composite();
        for (i,(a,b)) in before.iter().zip(&erased).enumerate() {assert!(b[3]+0.00005>=a[3]*0.6,"{name}/{space:?}: erase exceeded ceiling at{i}: {a:?}->{b:?}");}
        for c in 0..4 {assert!((erased[center][c]-before[center][c]*0.6).abs()<0.005,"{name}/{space:?}: erase channel{c}: {:?}",erased[center]);}
        canvas.stroke(8,&brush,&dabs,dabs.len(),true);let erased_twice=canvas.composite();
        assert!((erased_twice[center][3]-before[center][3]*0.36).abs()<0.005,"{name}/{space:?}: separate erases build: {:?}",erased_twice[center]);
    }}
}

#[test]
fn engine_generated_strength_changes_buildup_without_changing_opacity_ceiling() {
    let center=(64*EXTENT[0]+64) as usize;
    for (name,mut brush) in opacity_brushes() {
        brush.diameter=32.;
        let render=|b:&layer_core::BrushSnapshot,retraces| {
            let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
            let dabs=opacity_contacts(b,retraces);canvas.stroke(7,b,&dabs,dabs.len(),false);canvas.composite()
        };
        let mut weak=brush.clone();weak.flow=0.03;
        let first_weak=render(&weak,1);let first_strong=render(&brush,1);
        assert!(first_weak[center][3]+0.01<first_strong[center][3],"{name}: Strength changes buildup: {:?} vs {:?}",first_weak[center],first_strong[center]);
        let built=render(&weak,128);
        if brush.contact.is_some_and(|c|c.dry.is_some()) {
            let lost=first_strong.iter().zip(&first_weak).filter(|(a,b)|a[3]>0. && b[3]==0.).count();
            assert!(lost>0,"{name}: lower material pressure removes eligible contacts");
            assert!(first_strong.iter().zip(&built).any(|(a,b)|a[3]>0. && b[3]==0.),"{name}: repeated low Strength still leaves ineligible contacts empty");
            for (i,after) in built.iter().enumerate() {
                assert!(after[3]<=0.40005,"{name}: low Strength retrace exceeds Opacity at{i}");
            }
        } else {
            assert!((built[center][3]-0.4).abs()<0.005,"{name}: low Strength can reach the same ceiling: {:?}",built[center]);
        }
        let mut zero=brush.clone();zero.flow=0.;let empty=render(&zero,8);
        assert!(empty.iter().all(|p|*p==[0.;4]),"{name}: zero Strength paints nothing");
    }
    use layer_core::{Stroke,StrokePoint,StrokeTool,authored::PaintHandle};
    let mut pencil=layer_core::default_brush(layer_core::DefaultBrushPreset::Pencil);
    pencil.diameter=36.;pencil.opacity=1.;pencil.color_rgba_linear=[0.,0.,0.,1.];
    let sweep=|strength| {
        let mut brush=pencil.clone();brush.flow=strength;
        let points:Vec<StrokePoint>=[16.5,112.5].into_iter().enumerate().map(|(i,x)|StrokePoint {
            position:Point {x,y:64.5},pressure:1.,tilt:[std::f32::consts::FRAC_PI_4,0.],
            twist:0.,elapsed_micros:i as u32*16_000,
        }).collect();
        let stroke=Stroke::new(StrokeId(7),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),points).unwrap();
        let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,RgbSpace::Srgb,&mut dabs);
        let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
        canvas.stroke(7,&brush,&dabs,dabs.len(),false);
        (canvas.composite(),dabs)
    };
    let (empty,zero_contacts)=sweep(0.);
    assert!(zero_contacts.is_empty() && empty.iter().all(|p|*p==[0.;4]),"Pencil: zero Strength endpoint");
    let (middle,middle_contacts)=sweep(0.75);let (full,full_contacts)=sweep(1.);
    assert!(!full_contacts.is_empty() && full_contacts.iter().all(|d|d.flow==1.),"Pencil: full Strength keeps the original contact feed");
    assert_eq!(middle_contacts.len(),full_contacts.len());
    let mut reference_contacts=full_contacts.clone();
    for ((middle,full),reference) in middle_contacts.iter().zip(&full_contacts).zip(&mut reference_contacts) {
        assert_eq!(middle.flow,1.,"Pencil: Strength never scales pigment feed");
        assert_eq!(middle.radii,full.radii,"Pencil: Strength does not change physical pen geometry");
        assert_eq!(middle.previous,full.previous);assert_eq!(middle.rotation,full.rotation);
        reference.contact[0]*=0.75;reference.previous_contact[0]*=0.75;
    }
    let mut reference_brush=pencil.clone();reference_brush.flow=1.;
    let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
    canvas.stroke(7,&reference_brush,&reference_contacts,reference_contacts.len(),false);let reference=canvas.composite();
    for (i,(a,b)) in middle.iter().zip(&reference).enumerate() {for c in 0..4 {
        assert!((a[c]-b[c]).abs()<0.00002,"Pencil: Strength equals pressure-only reference pixel{i} channel{c}: {a:?} vs {b:?}");
    }}
    assert!(full[center][3]>0.95,"Pencil: full Strength retains its original endpoint: {:?}",full[center]);
    assert!(middle.iter().chain(&full).all(|p|p[..3]==[0.;3]),"Pencil: black pigment stays black in premultiplied pixels");
    pencil.flow=0.75;let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
    let dabs=opacity_contacts(&pencil,16);canvas.stroke(7,&pencil,&dabs,dabs.len(),false);let built=canvas.composite();
    assert!(built[center][3]>0.98,"Pencil: eligible contacts still build on retracing: {:?}",built[center]);
    println!("Pencil black sweep: Strength75 alpha={}, Strength100 alpha={}, retraced Strength75 alpha={}",middle[center][3],full[center][3],built[center][3]);
}

#[test]
fn engine_generated_uniform_contacts_lay_one_even_coat_however_the_sweep_is_divided() {
    use layer_core::{DefaultBrushPreset as P,Stroke,StrokePoint,StrokeTool,authored::PaintHandle};
    let glaze=layer_core::default_brush(P::TransparentGlaze);
    let mut opaque_glaze=glaze.clone();opaque_glaze.opacity=1.;
    let mut weak_pen=layer_core::default_brush(P::GPen);weak_pen.flow=0.3;
    for (name,mut brush) in [("Transparent Glaze",glaze),("opaque Transparent Glaze",opaque_glaze),
        ("Marker",layer_core::default_brush(P::Marker)),("weak G-Pen",weak_pen)] {
        brush.diameter=42.;brush.color_rgba_linear=[0.1,0.2,0.6,1.];brush.stabilization.strength=0.;
        let contacts=|inputs:u32| {
            let points:Vec<StrokePoint>=(0..=inputs).map(|i|StrokePoint {
                position:Point {x:8.5+111.*i as f32/inputs as f32,y:64.5},pressure:1.,tilt:[0.;2],
                twist:0.,elapsed_micros:i*8_000,
            }).collect();
            let stroke=Stroke::new(StrokeId(7),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),points).unwrap();
            let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,RgbSpace::Srgb,&mut dabs);dabs
        };
        let (swept,divided)=(contacts(1),contacts(40));
        assert!(swept.len()==2 && divided.len()>4,"{name}: {} and {} contacts",swept.len(),divided.len());
        for space in BlendSpace::ALL {
            let (depth,tolerance)=if space==BlendSpace::Linear {(SampleDepth::F32,2e-4)} else {(SampleDepth::U8,1.2/255.)};
            let draw=|dabs:&[Dab]| {
                let mut canvas=Canvas::new(DocumentColor {depth,..Default::default()},space,None);
                canvas.stroke(7,&brush,dabs,dabs.len(),false);canvas.composite()
            };
            let (once,coat)=(draw(&swept),draw(&divided));
            let centre=coat[(64*EXTENT[0]+64) as usize];
            assert!(centre[3]>0.1,"{name}/{space:?}: the stroke paints: {centre:?}");
            for (i,(a,b)) in once.iter().zip(&coat).enumerate() {for c in 0..4 {
                assert!((a[c]-b[c]).abs()<tolerance,"{name}/{space:?}: pixel{i} channel{c} depends on contact spacing: {a:?} vs {b:?}");
            }}
            for y in 0..EXTENT[1] as usize {
                let row=&coat[y*EXTENT[0] as usize..][..EXTENT[0] as usize];
                for x in 40..=88 {for c in 0..4 {
                    assert!((row[x][c]-row[64][c]).abs()<tolerance,"{name}/{space:?}: uneven coat at ({x},{y}) channel{c}: {:?} vs {:?}",row[x],row[64]);
                }}
            }
        }
    }
}

#[test]
fn engine_generated_opacity_ceiling_is_invariant_to_contact_batch_partition() {
    for (name,brush) in opacity_brushes() {for space in BlendSpace::ALL {
        let mut brush=brush.clone();brush.flow=0.07;let dabs=opacity_contacts(&brush,4);
        let draw=|chunk| {
            let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
            canvas.fill(canvas.base,[0.1,0.6,0.3,0.6]);canvas.stroke(7,&brush,&dabs,chunk,false);canvas.composite()
        };
        let separate=draw(1);let together=draw(dabs.len());
        for (i,(a,b)) in separate.iter().zip(together).enumerate() {for c in 0..4 {
            assert!((a[c]-b[c]).abs()<0.00002,"{name}/{space:?}: batch partition pixel{i} channel{c}: {a:?} vs {b:?}");
        }}
    }}
}

#[test]
fn engine_generated_smudge_strength_zero_is_noop_and_opacity_bounds_retraces() {
    let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::Smudge);
    brush.diameter=64.;brush.flow=1.;brush.wet_mix.attack=1.;brush.wet_mix.pull=1.;
    brush.wet_mix.blur=0.;brush.wet_mix.mix_space=layer_core::ColorMixSpace::LinearRgb;
    let draw=|strength:f32,opacity:f32,chunk:usize| {
        let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
        canvas.fill(canvas.base,[0.,0.,0.,1.]);
        canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([1066.,64.],[1.;4],1000.,1.));
        let before=canvas.composite();let mut brush=brush.clone();brush.flow=strength;brush.opacity=opacity;
        let dabs=opacity_contacts(&brush,8);canvas.stroke(7,&brush,&dabs,chunk.min(dabs.len()).max(1),false);
        (before,canvas.composite())
    };
    for (strength,opacity) in [(0.,1.),(0.,0.4),(1.,0.)] {for chunk in [1,usize::MAX] {
        let mut cursor=layer_engine::DabGenerator::default();let mut cursor_brush=brush.clone();
        cursor_brush.flow=strength;cursor_brush.opacity=opacity;
        let contacts=cursor.cursor_contacts(layer_core::StrokePoint {position:Point {x:64.5,y:64.5},
            pressure:1.,tilt:[0.;2],twist:0.,elapsed_micros:0},&cursor_brush);
        assert!(contacts.iter().any(|d|d.radii.iter().all(|r|*r>0.)),"zero brush keeps cursor geometry");
        let (before,after)=draw(strength,opacity,chunk);
        assert_eq!(before,after,"Smudge Strength={strength} Opacity={opacity} chunk={chunk}: noop");
    }}
    for chunk in [1,usize::MAX] {
        let (before,full)=draw(1.,1.,chunk);let (_,limited)=draw(1.,0.4,chunk);
        let mut largest_full=0f64;let mut largest_limited=0f64;
        for (i,((before,full),limited)) in before.iter().zip(&full).zip(&limited).enumerate() {
            assert!((limited[3]-before[3]).abs()<0.00002,"Smudge chunk={chunk}: opaque underpaint at{i}");
            for c in 0..3 {
                let full=(full[c]-before[c]).abs();let limited=(limited[c]-before[c]).abs();
                largest_full=largest_full.max(full);largest_limited=largest_limited.max(limited);
                assert!(limited<=0.40002,"Smudge chunk={chunk}: stroke budget pixel{i} channel{c}: {limited}");
            }
        }
        assert!(largest_full>0.5,"Smudge chunk={chunk}: the uncapped stroke visibly moves underpaint ({largest_full})");
        assert!(largest_limited>0.01 && largest_limited+0.1<largest_full,
            "Smudge chunk={chunk}: partial Opacity preserves a smaller real effect ({largest_limited} vs {largest_full})");
    }
}

#[test]
fn dither_sparse_contact_pages_match_rectangular_evaluation() {
    for bristles in [false, true] {
        let mut reference = None;
        for rectangular in [false, true] {
            let mut canvas = Canvas::at(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None, [512, 512]);
            canvas.paint(canvas.base, test_style(BrushExecution::Dry), soft([256.; 2], [0.1, 0.6, 0.3, 1.], 10000., 1.));
            canvas.paint(canvas.base, test_style(BrushExecution::Dry), soft([1256., 256.], [0.9, 0.1, 0.05, 1.], 1000., 1.));
            let before = canvas.composite();
            let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
            brush.blender.cell_size = 1.;
            brush.diameter = 128.;
            brush.blender.reach = if bristles { 0.06 } else { 0.25 };
            brush.contact = Some(layer_core::BrushContact { edge_roughness: 0.9, pooling: 1.,
                bristles: bristles.then(Default::default), ..Default::default() });
            brush.rendering.edge_after_stroke = rectangular;
            brush.validate().unwrap();
            let dabs: Vec<_> = [[260., 258.], [90., 160.], [252., 264.]].into_iter().enumerate().map(|(i, center)| {
                let mut dab = soft(center, [1.; 4], 90., 0.7);
                let angle = 0.8 + i as f32 * 0.2;
                dab.radii = [90., 8.];
                dab.rotation = [angle.cos(), angle.sin()];
                dab.previous = [76., 10.2, (-0.6f32).cos(), (-0.6f32).sin()];
                dab.motion = if bristles { [300., -100.] } else { [150., -100.] };
                if bristles { dab.center.x += 160.; }
                dab.contact = [0.73, 0.2, 0.4, 17.];
                dab.previous_contact = [0.41, 0.7, 0.1, 17.];
                dab
            }).collect();
            canvas.stroke(100, &brush, &dabs, dabs.len(), false);
            let actual = canvas.composite();
            assert!(actual.iter().zip(&before).any(|(a, b)| a != b), "the evolving/bristle fixture must move paint, bristles={bristles}");
            if let Some(expected) = &reference { assert_eq!(&actual, expected, "sparse bounds must include every evolving/bristle contact, bristles={bristles}"); }
            else { reference = Some(actual); }
        }
    }
}

#[test]
fn dither_and_uniform_brushes_can_share_a_queued_frame() {
    let mut brush = layer_core::default_brush(layer_core::DefaultBrushPreset::DitherBlender);
    brush.diameter = 96.;
    brush.opacity = 0.4;
    brush.blender.reach = 0.5;
    let dither = DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
    let mut uniform = test_style(BrushExecution::Dry);
    uniform.rendering.accumulation = BrushAccumulation::Uniform;
    let mut edged = dither.clone();
    edged.rendering.edge_after_stroke = true;
    edged.rendering.wet_edge = 0.;
    edged.rendering.burnt_edge = 0.;
    let styles = [uniform.clone(), dither.clone(), edged.clone(), uniform, dither, edged];
    let dabs: Vec<_> = (0..styles.len()).map(|i| {
        let mut dab = soft([244. + i as f32 * 6., 64.], [0.1, 0.2, 0.9, 0.4], 48., 0.7);
        dab.motion = [96., 0.];
        dab
    }).collect();
    let mut reference = None;
    for queued in [false, true] {
        let mut canvas = Canvas::at(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None, [512, 128]);
        canvas.fill(canvas.base, [0.1, 0.6, 0.3, 0.6]);
        canvas.paint(canvas.base, test_style(BrushExecution::Dry), soft([1066., 64.], [0.9, 0.1, 0.05, 1.], 1000., 1.));
        if queued {
            let batches: Vec<_> = styles.iter().zip(&dabs).enumerate().map(|(i, (style, dab))| DabBatch {
                stroke_id: StrokeId(100 + i as u64), first_dab: i as u32,
                ..crate::test_support::dab_batch(canvas.base, style.clone(), dab.bounds())
            }).collect();
            canvas.r.submit(FramePacket { dabs: &dabs, dab_batches: &batches, ..packet(canvas.document.scene(), canvas.extent) }).unwrap();
        } else {
            for (i, (style, dab)) in styles.iter().zip(&dabs).enumerate() {
                canvas.styled_stroke(100 + i as u64, style.clone(), &[*dab], 1);
            }
        }
        let actual = canvas.composite();
        if let Some(expected) = &reference { assert_eq!(&actual, expected, "queued brush changes must match separate frames"); }
        else { reference = Some(actual); }
    }
}

#[test]
fn dither_prediction_forks_committed_exposure_and_preserves_prior_tiles() {
    for (preset,cell_size) in [(layer_core::DefaultBrushPreset::DitherBlender,1.),
        (layer_core::DefaultBrushPreset::DitherBlender,3.),(layer_core::DefaultBrushPreset::ScatterBlender,4.)] {
    let mut brush = layer_core::default_brush(preset);
    brush.blender.cell_size = cell_size;
    brush.diameter = 48.;
    brush.opacity = 0.4;
    brush.blender.reach = 0.5;
    brush.flow = 0.08;
    let mut dabs = opacity_contacts(&brush, 2);
    for (i, dab) in dabs.iter_mut().enumerate() {
        let scale = [1., 0.3, 1.4, 0.6][i % 4];
        dab.radii = [dab.radii[0] * scale, dab.radii[1] * scale * 0.5];
        let angle = i as f32 * 0.13;
        dab.rotation = [angle.cos(), angle.sin()];
        if i % 7 == 3 { dab.center.x = 300.; }
    }
    let split = dabs.len() / 2;
    let send = |canvas: &mut Canvas, contacts: &[Dab], kind, start, end| {
        let style = DabStyle::for_brush(&brush, layer_core::StrokeTool::Brush);
        let damage = contacts.iter().fold(Rect::EMPTY, |r, d| r.union(d.bounds()));
        let batch = DabBatch { stroke_id: StrokeId(100), dab_count: contacts.len() as u32,
            kind, stroke_start: start, stroke_end: end, ..crate::test_support::dab_batch(canvas.base, style, damage) };
        canvas.r.submit(FramePacket { dabs: contacts, dab_batches: &[batch], ..packet(canvas.document.scene(), canvas.extent) }).unwrap();
    };
    let mut preview = Canvas::at(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None, [512, 128]);
    let mut reference = Canvas::at(DocumentColor { depth: SampleDepth::F32, ..Default::default() }, BlendSpace::Linear, None, [512, 128]);
    for canvas in [&mut preview, &mut reference] {
        canvas.fill(canvas.base, [0.1, 0.6, 0.3, 0.6]);
        canvas.paint(canvas.base, test_style(BrushExecution::Dry), soft([1066., 64.], [0.9, 0.1, 0.05, 1.], 1000., 1.));
    }
    let untouched = preview.composite();
    send(&mut preview, &dabs[..split], DabBatchKind::Preview, true, false);
    let predicted_initial = preview.pixels();
    assert_ne!(predicted_initial, untouched, "first-touch prediction transports actual pigment");
    assert_eq!(preview.composite(), untouched, "cancel first-touch prediction restores untouched paint");
    send(&mut reference, &dabs[..split], DabBatchKind::Persistent, true, false);
    assert_eq!(predicted_initial, reference.pixels(), "first-touch prediction equals the same committed segment");
    send(&mut preview, &dabs[..split], DabBatchKind::Persistent, true, false);
    let committed = preview.pixels();
    send(&mut preview, &dabs[split..], DabBatchKind::Preview, false, false);
    let predicted = preview.pixels();
    send(&mut reference, &dabs[split..], DabBatchKind::Persistent, false, true);
    let expected = reference.pixels();
    assert_ne!(predicted, committed, "prediction transports actual pigment");
    assert_eq!(predicted, expected, "prediction equals the same committed continuation");
    assert_eq!(preview.composite(), committed, "cancel prediction restores committed paint");
    send(&mut preview, &dabs[split..], DabBatchKind::Persistent, false, true);
    assert_eq!(preview.pixels(), expected, "prediction leaves committed exposure untouched");
    }
}

#[test]
fn engine_generated_partial_opacity_prediction_forks_and_preserves_committed_budget() {
    let (_,mut brush)=opacity_brushes()[0].clone();brush.flow=0.08;
    let dabs=opacity_contacts(&brush,2);let split=2;
    let send=|canvas:&mut Canvas,contacts:&[Dab],kind,start,end| {
        let mut style=DabStyle::for_brush(&brush,layer_core::StrokeTool::Brush);style.blend_space=canvas.space;
        let damage=contacts.iter().fold(Rect::EMPTY,|r,d|r.union(d.bounds()));
        let batch=DabBatch {stroke_id:StrokeId(7),dab_count:contacts.len() as u32,kind,stroke_start:start,stroke_end:end,
            ..crate::test_support::dab_batch(canvas.base,style,damage)};
        canvas.r.submit(FramePacket {dabs:contacts,dab_batches:std::slice::from_ref(&batch),
            ..packet(canvas.document.scene(),canvas.extent)}).unwrap();
    };
    let mut preview=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
    let mut reference=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
    for canvas in [&mut preview,&mut reference] {send(canvas,&dabs[..split],DabBatchKind::Persistent,true,false);}
    let committed=preview.pixels();assert!(committed.iter().any(|p|p[3]>0. && p[3]<0.39));
    send(&mut preview,&dabs[split..],DabBatchKind::Preview,false,false);let predicted=preview.pixels();
    send(&mut reference,&dabs[split..],DabBatchKind::Persistent,false,true);let expected=reference.pixels();
    assert!(predicted.iter().zip(&committed).any(|(a,b)|a[3]>b[3]+0.0001),"prediction paints a real continuation");
    for (i,(a,b)) in predicted.iter().zip(&expected).enumerate() {for c in 0..4 {
        assert!((a[c]-b[c]).abs()<0.00002,"partial Opacity prediction pixel{i} channel{c}: {a:?} vs {b:?}");
    }}
    assert_eq!(preview.composite(),committed,"cancel prediction preserves committed pixels");
    send(&mut preview,&dabs[split..],DabBatchKind::Persistent,false,true);
    assert_eq!(preview.pixels(),expected,"prediction cannot consume the real stroke's budget");
}

#[test]
fn engine_generated_opacity_keeps_later_opaque_pigment_changes_inside_the_stroke() {
    use layer_core::{Stroke,StrokePoint,StrokeTool,authored::PaintHandle};
    let (_,mut brush)=opacity_brushes()[1].clone();brush.spacing=0.1;
    brush.color_rgba_linear=[1.,0.,0.,1.];brush.color_dynamics.stamp_hue_jitter=0.4;
    let stroke=Stroke::new(StrokeId(7),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),
        [56.5,72.5].into_iter().enumerate().map(|(i,x)|StrokePoint {position:Point {x,y:64.5},
            pressure:1.,tilt:[0.;2],twist:0.,elapsed_micros:i as u32*16_000}).collect::<Vec<_>>()).unwrap();
    let mut generated=Vec::new();layer_engine::DabGenerator::generate(&stroke,RgbSpace::Srgb,&mut generated);
    let first=*generated.iter().find(|d|coverage(d,[64,64])==1.).unwrap();
    let last=*generated.iter().find(|d|coverage(d,[64,64])==1. && d.color_rgba_linear!=first.color_rgba_linear).unwrap();
    for dab in [first,last] {assert_eq!(dab.flow*dab.color_rgba_linear[3],1.);}
    let dabs=[first,last];let center=(64*EXTENT[0]+64) as usize;let mut rows=Vec::new();
    for space in BlendSpace::ALL {for chunk in [1,2] {for background in [None,Some([0.1,0.6,0.3,0.6]),Some(first.color_rgba_linear)] {for opacity in [1f32,0.4,0.01,0.001] {
        let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
        if let Some(color)=background {canvas.fill(canvas.base,color);}
        let before=canvas.composite()[center];brush.opacity=opacity;
        let mut style=DabStyle::for_brush(&brush,StrokeTool::Brush);assert_eq!(style.execution,BrushExecution::Dry);
        style.wet_mix.dilution=1.;canvas.styled_stroke(7,style,&dabs,chunk);let actual=canvas.composite()[center];
        let expected:[f64;4]=std::array::from_fn(|c|before[c]*(1.-f64::from(opacity))+
            if c==3 {f64::from(opacity)} else {
                let color=f64::from(last.color_rgba_linear[c]);
                (if space==BlendSpace::Perceptual {RgbSpace::Srgb.encode(color)} else {color})*f64::from(opacity)
            });
        eprintln!("pigment-opacity background={background:?} opacity={opacity} first={:?} later={:?} actual={actual:?} expected={expected:?}",
            first.color_rgba_linear,last.color_rgba_linear);
        rows.push((space,chunk,background,opacity,actual,expected));
        if chunk==1 && opacity==0.4 {
            let mut preview=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
            if let Some(color)=background {preview.fill(preview.base,color);}
            let send=|canvas:&mut Canvas,dab:&Dab,kind,start,end| {
                let mut style=DabStyle::for_brush(&brush,StrokeTool::Brush);style.blend_space=space;style.wet_mix.dilution=1.;
                let batch=DabBatch {stroke_id:StrokeId(7),dab_count:1,kind,stroke_start:start,stroke_end:end,
                    ..crate::test_support::dab_batch(canvas.base,style,dab.bounds())};
                canvas.r.submit(FramePacket {dabs:std::slice::from_ref(dab),dab_batches:std::slice::from_ref(&batch),
                    blend_space:space,..packet(canvas.document.scene(),canvas.extent)}).unwrap();
            };
            send(&mut preview,&first,DabBatchKind::Persistent,true,false);let committed=preview.pixels();
            send(&mut preview,&last,DabBatchKind::Preview,false,false);let predicted=preview.pixels();
            for c in 0..4 {assert!((predicted[center][c]-actual[c]).abs()<0.00002,"{space:?}: pigment prediction {:?} vs {actual:?}",predicted[center]);}
            assert_eq!(preview.composite(),committed,"{space:?}: cancel pigment prediction");
            send(&mut preview,&last,DabBatchKind::Persistent,false,true);
            assert_eq!(preview.pixels()[center],actual,"{space:?}: prediction preserves stroke-start underpaint");
        }
    }}}}
    for (space,chunk,background,opacity,actual,expected) in rows {
        for c in 0..4 {assert!((actual[c]-expected[c]).abs()<if space==BlendSpace::Linear {0.00001} else {0.004},
            "{space:?} chunk={chunk}: later opaque pigment must repaint within the stroke without increasing its Opacity ceiling: background={background:?}, opacity={opacity}, channel{c}, actual={actual:?}, expected={expected:?}");}
    }

}

#[test]
fn engine_generated_bristle_partial_opacity_matches_full_stroke_group_composite() {
    let mut brush=layer_core::default_brush(layer_core::DefaultBrushPreset::BristlePaintbrush);
    brush.diameter=64.;brush.flow=0.3;brush.color_rgba_linear=[0.7,0.2,0.4,1.];
    let dabs=opacity_contacts(&brush,4);
    for chunk in [1,dabs.len()] {
        let draw=|opacity| {
            let mut canvas=Canvas::new(DocumentColor {depth:SampleDepth::F32,..Default::default()},BlendSpace::Linear,None);
            canvas.fill(canvas.base,[0.1,0.6,0.3,0.6]);let before=canvas.composite();
            let mut brush=brush.clone();brush.opacity=opacity;canvas.stroke(7,&brush,&dabs,chunk,false);
            (before,canvas.composite())
        };
        let (before,full)=draw(1.);let (_,limited)=draw(0.4);
        assert!(full.iter().zip(&before).any(|(a,b)|(a[3]-b[3]).abs()>0.01),"Bristle paints a real stroke");
        for (i,((b,g),f)) in before.iter().zip(&full).zip(&limited).enumerate() {for c in 0..4 {
            let expected=b[c]*0.6+g[c]*0.4;
            assert!((f[c]-expected).abs()<0.00002,"Bristle group Opacity chunk={chunk} pixel{i} channel{c}: {f:?} expected={expected} full={g:?} before={b:?}");
        }}
    }
}

#[test]
fn partial_opacity_group_reference_covers_non_normal_blends_and_alpha_lock() {
    let (_,mut brush)=opacity_brushes()[1].clone();brush.flow=0.3;
    brush.color_dynamics.stamp_hue_jitter=0.4;let dabs=opacity_contacts(&brush,2);
    assert!(dabs.iter().any(|d|d.color_rgba_linear!=dabs[0].color_rgba_linear));
    for space in BlendSpace::ALL {for (mode,locked) in [(BrushBlendMode::Multiply,false),
        (BrushBlendMode::Overlay,false),(BrushBlendMode::Normal,true)] {for chunk in [1,dabs.len()] {
        let draw=|opacity| {
            let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
            canvas.fill(canvas.base,[0.1,0.6,0.3,0.6]);let before=canvas.composite();
            let mut style=DabStyle::for_brush(&brush,layer_core::StrokeTool::Brush);
            style.opacity=opacity;style.alpha_locked=locked;style.rendering.blend_mode=mode;
            canvas.styled_stroke(7,style,&dabs,chunk);(before,canvas.composite())
        };
        let (before,full)=draw(1.);let (_,limited)=draw(0.4);
        assert!(full.iter().zip(&before).any(|(a,b)|(a[0]-b[0]).abs()>0.01),"{space:?}/{mode:?}: real pigment change");
        for (i,((b,g),f)) in before.iter().zip(&full).zip(&limited).enumerate() {for c in 0..4 {
            let expected=b[c]*0.6+g[c]*0.4;
            assert!((f[c]-expected).abs()<0.00002,"{space:?}/{mode:?} locked={locked} chunk={chunk} pixel{i} channel{c}: {f:?} expected={expected} full={g:?} before={b:?}");
            if locked && c==3 {assert!((f[c]-b[c]).abs()<0.00002,"alpha lock preserves partial underpaint");}
        }}
    }}}
}

#[test]
fn partial_opacity_clone_matches_full_stroke_group_over_colored_translucent_source() {
    let (_,mut brush)=opacity_brushes()[1].clone();brush.flow=0.3;let dabs=opacity_contacts(&brush,2);
    for accumulation in [BrushAccumulation::Flow,BrushAccumulation::Uniform] {
    for space in BlendSpace::ALL {for chunk in [1,dabs.len()] {
        let draw=|opacity,offset| {
            let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
            canvas.fill(canvas.base,[0.1,0.6,0.3,0.3]);
            canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([20.,64.],[0.8,0.1,0.2,0.3],12.,1.));
            canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([44.,64.],[0.1,0.2,0.9,0.8],12.,1.));
            let before=canvas.composite();
            assert!((before[64*128+20][3]-before[64*128+44][3]).abs()>0.2,"Clone source has varying alpha");
            assert!((0..3).any(|c|(before[64*128+20][c]-before[64*128+44][c]).abs()>0.1),"Clone source has varying color");
            let mut style=DabStyle::for_brush(&brush,layer_core::StrokeTool::Brush);style.opacity=opacity;
            style.execution=BrushExecution::Clone;style.rendering.accumulation=accumulation;
            style.retouch=Some(layer_core::Retouch {source:layer_core::RetouchSource::Editing,offset,
                ..Default::default()});
            canvas.styled_stroke(7,style,&dabs,chunk);(before,canvas.composite())
        };
        let (before,full)=draw(1.,[-48.,0.]);let (_,limited)=draw(0.4,[-48.,0.]);
        assert!(full.iter().zip(&before).any(|(a,b)|(a[3]-b[3]).abs()>0.01),"{space:?}: Clone copies source alpha");
        for (i,((b,g),f)) in before.iter().zip(&full).zip(&limited).enumerate() {for c in 0..4 {
            let expected=b[c]*0.6+g[c]*0.4;
            assert!((f[c]-expected).abs()<0.00002,"{accumulation:?}/{space:?}: Clone group chunk={chunk} pixel{i} channel{c}: {f:?} expected={expected} full={g:?} before={b:?}");
        }}
        let (before,empty)=draw(0.4,[-256.,0.]);
        assert_eq!(before,empty,"{space:?}: transparent out-of-domain Clone source is a noop");
    }}}
}

#[test]
fn saturated_dry_retraces_preserve_exact_rgba_with_varying_pressure() {
    use layer_core::{Stroke,StrokePoint,StrokeTool,authored::PaintHandle};
    for preset in [layer_core::DefaultBrushPreset::Pencil,layer_core::DefaultBrushPreset::GPen] {
    let mut brush=layer_core::default_brush(preset);
    brush.diameter=64.;brush.flow=1.;brush.color_rgba_linear=[0.,1.,0.,1.];
    let points=(0..9).map(|i|StrokePoint {position:Point {x:if i%2==0 {32.5} else {95.5},y:64.5},
        pressure:[0.3,0.6,0.95][i%3],tilt:[std::f32::consts::FRAC_PI_4,0.],twist:0.,
        elapsed_micros:i as u32*16_000}).collect::<Vec<_>>();
    let stroke=Stroke::new(StrokeId(7),SourceTarget::Paint(PaintHandle::from_index(1)),StrokeTool::Brush,brush.clone(),points).unwrap();
    let mut dabs=Vec::new();layer_engine::DabGenerator::generate(&stroke,RgbSpace::Srgb,&mut dabs);
    assert!(dabs.iter().any(|d|d.contact[0]!=dabs[0].contact[0]),"{preset:?}: real contacts vary pressure");
    for space in BlendSpace::ALL {for chunk in [1,dabs.len()] {for opacity in [1.,0.4] {for erase in [false,true] {
        let mut canvas=Canvas::new(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None);
        if !erase {canvas.fill(canvas.base,brush.color_rgba_linear);}
        let before=canvas.composite();brush.opacity=opacity;
        let mut style=DabStyle::for_brush(&brush,if erase {StrokeTool::Eraser} else {StrokeTool::Brush});
        assert_eq!(style.execution,BrushExecution::Dry);style.wet_mix.dilution=1.;
        canvas.styled_stroke(7,style,&dabs,chunk);let after=canvas.composite();
        assert_eq!(after,before,"{preset:?}/{space:?}: saturated same-pigment/transparent erase Opacity={opacity} erase={erase} chunk={chunk}");
    }}}}
    }
}

#[test]
fn partial_analytic_display_averages_current_and_stroke_start_over_the_same_footprint() {
    let extent=[131,129];let pigment=[0.8f32,0.1,0.4,0.7];
    for space in BlendSpace::ALL {
        let mut canvas=Canvas::at(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None,extent);
        let r=&mut canvas.r;
        let convert=|mut p:[f64;4],encode:bool| {
            if space==BlendSpace::Perceptual && p[3]>0. {for c in 0..3 {
                p[c]=(if encode {RgbSpace::Srgb.encode(p[c]/p[3])} else {RgbSpace::Srgb.decode(p[c]/p[3])})*p[3];
            }}p
        };
        let background=r.create_page_surface("varying translucent stroke start");
        let current=r.create_page_surface("partially painted current pixels");
        for opacity in [0.4f32,0.01,0.001] {for mode in [DabMode::Paint,DabMode::Erase] {
            let mut b=Vec::new();let mut f=Vec::new();
            for y in 0..PAGE_SIZE {for x in 0..PAGE_SIZE {
                if x>=extent[0] || y>=extent[1] {b.push([0.;4]);f.push([0.;4]);continue;}
                let a=if (x+y)%2==0 {0.2} else {0.8};
                let original=[(0.1+0.3*f64::from(x%3))*a,(0.2+0.2*f64::from(y%3))*a,0.5*a,a];
                let bg=convert(original,true);let source=if mode==DabMode::Erase {[0.;4]} else {
                    [if space==BlendSpace::Perceptual {RgbSpace::Srgb.encode(0.8)} else {0.8},
                    if space==BlendSpace::Perceptual {RgbSpace::Srgb.encode(0.1)} else {0.1},
                    if space==BlendSpace::Perceptual {RgbSpace::Srgb.encode(0.4)} else {0.4},1.]};
                let painted=convert(std::array::from_fn(|c|bg[c]*(1.-f64::from(opacity)*0.3)+source[c]*f64::from(opacity)*0.3),false);
                b.push(original.map(|v|v as f32));f.push(painted.map(|v|v as f32));
            }}
            for (surface,pixels) in [(&background,&b),(&current,&f)] {
                let bytes:Vec<_>=pixels.iter().flat_map(|p|p.iter().flat_map(|v|v.to_le_bytes())).collect();
                crate::test_support::upload_page(r,&surface.texture,&bytes);
            }
            for block in [1u32,2,4] {
                r.preview_level=block.trailing_zeros();r.preview_contribution=true;
                let mut style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
                style.opacity=opacity;style.mode=mode;style.blend_space=space;
                assert!(crate::dry_material::display_preview_eligible(&style));
                let mut dab=soft([64.5;2],pigment,200.,1.);dab.flow=0.25;
                let mut batch=crate::test_support::dab_batch(canvas.base,style,Rect::from_extent(extent));batch.kind=DabBatchKind::Preview;
                assert!(!r.compact_preview_contribution(&batch),"partial Opacity needs complete RGBA");
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                r.prepare_uploads(FramePacket {dabs:&[dab],dab_batches:std::slice::from_ref(&batch),..packet(canvas.document.scene(),extent)},&mut [Vec::new()],&mut encoder).unwrap();
                r.prepare_records(std::iter::once(material_sources::dry_record(PixelRect::full(extent),0..1,[0;16])),&mut encoder).unwrap();
                r.ensure_texture_set(&batch.style).unwrap();r.uploads.finish(&encoder);encoder.submit(&r.queue);
                r.queue.write_buffer(&r.target_buffer,0,target_bytes(&TargetGpu::new([0;2],extent,extent)));
                let size=[PAGE_SIZE/block;2];let output=create_page_surface(&r.device,&r.float_texture_layout,&r.sampler,size,r.device.working_format(),"partial display reference output");
                let sources=create_material_bind_group(&r.device,&r.material_layout,&[&current.view;9],&r.dab_buffer,&r.empty_scalar_view,&background.view,wgpu::BindingResource::Buffer(r.material_records.binding()));
                let binding=r.pipelines.dry_material.output(r,&output,None,None);
                let display=crate::test_support::dry_display(r,&batch,[&current.view;9],&output);
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                r.encode_dry_material_jobs(&mut encoder,0,&batch,&[crate::dry_material::Job {output:binding,source:sources,display,coordinate:[0;2],
                    coverage:false,record_offset:0,dirty:PixelRect::full(extent)}]);encoder.submit(&r.queue);
                let actual=crate::test_support::float_pixels(r,&output.texture);
                for y in 0..size[1] {for x in 0..size[0] {
                    let start=[x*block,y*block];let end=[(start[0]+block).min(extent[0]),(start[1]+block).min(extent[1])];
                    let mut original=[0.;4];let mut painted=[0.;4];
                    if start[0]<end[0] && start[1]<end[1] {
                        for py in start[1]..end[1] {for px in start[0]..end[0] {let i=(py*PAGE_SIZE+px) as usize;
                            for c in 0..4 {original[c]+=f64::from(b[i][c]);painted[c]+=f64::from(f[i][c]);}
                        }}let n=f64::from((end[0]-start[0])*(end[1]-start[1]));for c in 0..4 {original[c]/=n;painted[c]/=n;}
                    }
                    let center=[start[0]+block/2,start[1]+block/2];let mut expected=painted;
                    if center[0]<extent[0] && center[1]<extent[1] {
                        let bg=convert(original,true);let old=convert(painted,true);
                        let q=f64::from(dab.flow*dab.color_rgba_linear[3]);
                        expected=convert(std::array::from_fn(|c| {
                            let source=if mode==DabMode::Erase {0.} else if c==3 {1.} else if space==BlendSpace::Perceptual {RgbSpace::Srgb.encode(f64::from(pigment[c]))} else {f64::from(pigment[c])};
                            old[c]*(1.-q)+(bg[c]*(1.-f64::from(opacity))+source*f64::from(opacity))*q
                        }),false);
                    }
                    let a=actual[(y*size[0]+x) as usize];for c in 0..4 {assert!((f64::from(a[c])-expected[c]).abs()<0.000002,
                        "{space:?}/{mode:?} O={opacity} block={block} ({x},{y}) channel{c}: {a:?} expected={expected:?}");}
                }}
            }
        }}
    }
}

#[test]
fn partial_gpen_scaled_prediction_cancels_and_exact_queries_match_commit() {
    for space in BlendSpace::ALL {for opacity in [0.4,0.001] {for erase in [false,true] {
        let create=||Canvas::at(DocumentColor {depth:if space==BlendSpace::Linear {SampleDepth::F32} else {SampleDepth::U8},..Default::default()},space,None,[1024;2]);
        let mut preview=create();let mut reference=create();
        let send=|canvas:&mut Canvas,dabs:&[Dab],batches:&[DabBatch]| {
            let mut frame=packet(canvas.document.scene(),canvas.extent);frame.composite_all=false;
            frame.view.document_to_surface=[0.125,0.,0.,0.125,0.,0.];frame.view.width_px=128;frame.view.height_px=128;
            canvas.r.submit(FramePacket {dabs,dab_batches:batches,blend_space:space,..frame}).unwrap();
        };
        for canvas in [&mut preview,&mut reference] {
            canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([512.;2],[0.1,0.6,0.3,0.6],2000.,1.));
            canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([480.,512.],[0.8,0.1,0.2,0.4],38.,1.));
            canvas.paint(canvas.base,test_style(BrushExecution::Dry),soft([544.,512.],[0.1,0.2,0.9,0.8],27.,1.));
        }
        let mut style=crate::layer_tests::preset_style(layer_core::DefaultBrushPreset::GPen);
        style.opacity=opacity;style.blend_space=space;style.mode=if erase {DabMode::Erase} else {DabMode::Paint};
        let mut first=soft([500.,512.],[0.7,0.2,0.4,1.],110.,1.);first.flow=0.08;
        let mut last=first;last.center.x=550.;last.flow=0.3;last.contact[0]=0.7;
        let mut batch=DabBatch {stroke_id:StrokeId(7),dab_count:1,stroke_start:true,stroke_end:false,
            ..crate::test_support::dab_batch(preview.base,style,first.bounds().union(last.bounds()))};
        for canvas in [&mut preview,&mut reference] {send(canvas,&[first],std::slice::from_ref(&batch));}
        let committed=preview.r.readback_srgb_rgba8().unwrap();
        send(&mut preview,&[],&[]);let display_before=crate::scene::scale::tests::display_pixels(&preview.r);
        batch.stroke_start=false;batch.kind=DabBatchKind::Preview;
        send(&mut preview,&[last],std::slice::from_ref(&batch));
        assert!(preview.r.preview_level>0,"partial G-Pen uses display-sized prediction");
        assert!(!preview.r.preview_contribution,"partial Opacity prediction retains complete RGBA");
        assert!(preview.r.preview_pages.iter().all(|p|p.active().texture.width()<PAGE_SIZE));
        let predicted=crate::scene::scale::tests::display_pixels(&preview.r);
        assert_ne!(predicted,display_before,"prediction makes a visible continuation");
        let exact_prediction=preview.r.readback_srgb_rgba8().unwrap();assert_eq!(preview.r.preview_level,0);
        send(&mut preview,&[],&[]);assert_eq!(preview.r.readback_srgb_rgba8().unwrap(),committed,"cancel prediction keeps committed pixels");
        assert_eq!(crate::scene::scale::tests::display_pixels(&preview.r),display_before,"cancel prediction restores scaled scene");
        batch.kind=DabBatchKind::Persistent;batch.stroke_end=true;
        for canvas in [&mut preview,&mut reference] {send(canvas,&[last],std::slice::from_ref(&batch));}
        let accepted=preview.r.readback_srgb_rgba8().unwrap();
        assert_eq!(accepted,exact_prediction,"exact query replays the same contacts as commit");
        assert_eq!(accepted,reference.r.readback_srgb_rgba8().unwrap(),"prediction cannot alter committed group background");
    }}}
}
