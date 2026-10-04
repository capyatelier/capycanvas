//! Pass Through groups against the documents they are equivalent to: the same
//! layers ungrouped, a fade between the composites without and with the
//! group, and an isolated Normal group once clipped.
use super::*;
use layer_core::color::source::{SourceBuilder, SourceChannels, SourceImage, SourceInterpretation};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth};
use layer_core::{BlendSpace, Document, Edit, EffectInstance, LayerBlend, CoverageSnapshot, Selection};
use layer_core::authored::*;

const EXTENT: [u32; 2] = [300, 280];
const COLOR: DocumentColor = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };

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
        let mut document = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = COLOR;
        let paint = document.scene().order()[0];
        document.apply(document.delete_layers_edit(&[paint]).unwrap()).unwrap();
        Self(document)
    }
    fn push(&mut self, content: OccurrenceContent, name: &str, parent: Option<OccurrenceHandle>, edits: Vec<Edit>) -> OccurrenceHandle {
        let owner = RecordChange::insert(&self.0.artwork.occurrences, Occurrence::new(content, name));
        let id = owner.handle;
        let stack = parent.map(|h| match self.layer(h).content { OccurrenceContent::Stack(s) => s, _ => panic!("group") }).unwrap_or(self.0.composition().result);
        let mut entries = self.0.artwork.stacks.get(stack).unwrap().clone();
        let at = if parent.is_none() { entries.entries.len() - 1 } else { entries.entries.len() };
        entries.entries.insert(at, id);
        let stack = RecordChange::replace(&self.0.artwork.stacks, stack, Some(entries)).unwrap();
        let mut edits = edits;
        edits.extend([Edit::Occurrence(owner), Edit::Stack(stack)]);
        self.0.apply(Edit::Batch(edits)).unwrap();
        id
    }
    fn paint(&mut self, name: &str, parent: Option<OccurrenceHandle>, blend: LayerBlend, seed: u32, alpha: impl Fn(u32, u32) -> u16) -> OccurrenceHandle {
        let paint = RecordChange::insert(&self.0.artwork.paint, PaintSource { domain: EXTENT, original: Some(source(seed, alpha)), raster: Default::default(), operations: Arc::default() });
        let id = self.push(OccurrenceContent::Paint(paint.handle), name, parent, vec![Edit::Paint(paint)]);
        self.layer(id).blend = blend;
        id
    }
    fn group(&mut self, name: &str, parent: Option<OccurrenceHandle>, blend: LayerBlend) -> OccurrenceHandle {
        let stack = RecordChange::insert(&self.0.artwork.stacks, Stack::default());
        let id = self.push(OccurrenceContent::Stack(stack.handle), name, parent, vec![Edit::Stack(stack)]);
        self.layer(id).blend = blend;
        id
    }
    fn effect(&mut self, name: &str, parent: Option<OccurrenceHandle>, filter: &str) -> OccurrenceHandle {
        let draft = EffectInstance::new(fixture(filter).program());
        let definition = RecordChange::insert(&self.0.artwork.definitions, Definition { program: draft.program });
        let effect = RecordChange::insert(&self.0.artwork.effects, EffectApplication { definition: definition.handle, values: draft.values, domain: EXTENT });
        self.push(OccurrenceContent::Effect(effect.handle), name, parent, vec![Edit::Definition(definition), Edit::Effect(effect)])
    }
    fn layer(&mut self, id: OccurrenceHandle) -> &mut Occurrence {
        self.0.artwork.occurrences.get_mut(id).unwrap()
    }
    fn mask(&mut self, id: OccurrenceHandle, mask: &CoverageSnapshot) {
        let source = RecordChange::insert(&self.0.artwork.coverage, mask.source.clone());
        let mut owner = self.layer(id).clone();
        let mut use_ = mask.use_.clone(); use_.source = source.handle;
        owner.mask = Some(use_);
        let owner = RecordChange::replace(&self.0.artwork.occurrences, id, Some(owner)).unwrap();
        self.0.apply(Edit::Batch(vec![Edit::Coverage(source), Edit::Occurrence(owner)])).unwrap();
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
fn nested() -> (Document, OccurrenceHandle) {
    let mut b = Builder::new();
    b.paint("Screen", None, LayerBlend::Screen, 3, disc(60, 70, 50));
    let outer = b.group("Outer", None, LayerBlend::PassThrough);
    let adjustment = b.effect("Black & White", Some(outer), "black_white");
    b.layer(adjustment).opacity = 0.8;
    b.paint("Multiply", Some(outer), LayerBlend::Multiply, 5, disc(200, 90, 80));
    let inner = b.group("Inner", Some(outer), LayerBlend::PassThrough);
    b.layer(inner).translation = Point { x: 13., y: -7. };
    b.effect("Blur", Some(inner), "gaussian_blur");
    let clip = b.paint("Overlay clip", Some(inner), LayerBlend::Overlay, 7, |x, _| if x % 90 < 60 { 65535 } else { 0 });
    b.layer(clip).clipped = true;
    let base = b.paint("Base", Some(inner), LayerBlend::Normal, 11, disc(150, 180, 90));
    b.layer(base).opacity = 0.8;
    let isolated = b.group("Isolated", Some(outer), LayerBlend::Normal);
    let deepest = b.group("Deepest", Some(isolated), LayerBlend::PassThrough);
    b.paint("Difference", Some(deepest), LayerBlend::Difference, 13, disc(240, 220, 70));
    b.paint("Inside isolated", Some(isolated), LayerBlend::Normal, 17, disc(210, 200, 60));
    b.paint("Backdrop", None, LayerBlend::Normal, 19, |_, _| 65535);
    b.0.working.occurrence = b.0.scene().order().first().copied();
    (b.0, outer)
}

/// The live composite of `document`, in its blend space.
fn render(r: &mut WgpuRasterizer, document: &Document) -> Vec<[f32; 4]> {
    let view = crate::test_support::view(EXTENT);
    let frame = FramePacket { view, blend_space: document.composition().blend, ..packet(document.scene(), EXTENT) };
    r.submit(FramePacket { reset_layers: true, ..frame }).unwrap();
    for _ in 0..16 {
        if !layer_render::CanvasRenderer::has_pending_work(r) {
            break;
        }
        r.submit(FramePacket { composite_all: false, ..frame }).unwrap();
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
        .capture(layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap(), Default::default())
        .unwrap();
    capture.read_region([0, 0, EXTENT[0], EXTENT[1]]).unwrap()
}

/// `composite` as linear pixels, decoded from a Perceptual composite.
fn linear(document: &Document, composite: &[[f32; 4]]) -> Vec<[f32; 4]> {
    composite
        .iter()
        .map(|&[red, green, blue, alpha]| {
            if document.composition().blend == BlendSpace::Linear || alpha <= 0. {
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
    while let Some(group) = document.scene().order().iter().copied().find(|h| document.scene().occurrence(*h).unwrap().passes_through()) {
        document.apply(document.ungroup_layer_edit(group).unwrap()).unwrap();
    }
    document
}

fn set(document: &Document, id: OccurrenceHandle, change: impl Fn(&mut Occurrence)) -> Document {
    let mut document = document.clone();
    change(document.artwork.occurrences.get_mut(id).unwrap());
    document
}

#[test]
fn pass_through_groups_composite_as_their_layers_ungrouped() {
    let mut r = WgpuRasterizer::new_native_headless(COLOR).expect("physical GPU required");
    for blend_space in BlendSpace::ALL {
        let (mut document, outer) = nested();
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend = blend_space;
        let passing = render(&mut r, &document);
        let flat = ungrouped(&document);
        assert!(flat.scene().order().iter().all(|h| !flat.scene().occurrence(*h).unwrap().passes_through()));
        assert_close(&passing, &render(&mut r, &flat), 2e-5, &format!("{blend_space:?}: nested groups against their layers ungrouped"));
        let isolated = render(&mut r, &set(&document, outer, |g| g.blend = LayerBlend::Normal));
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
        let inner = moved.scene().order().iter().copied().find(|h| &*moved.scene().occurrence(*h).unwrap().name == "Inner").unwrap();
        let children = moved.scene().children(Some(inner)).to_vec();
        let replacement = RecordChange::insert(&moved.artwork.occurrences, moved.scene().occurrence(inner).unwrap().clone());
        let new_inner = replacement.handle;
        let containing = moved.scene().stack(inner).unwrap();
        let mut stack = moved.artwork.stacks.get(containing).unwrap().clone();
        *stack.entries.iter_mut().find(|h| **h == inner).unwrap() = new_inner;
        moved.apply(Edit::Batch(vec![Edit::Occurrence(replacement),
            Edit::Occurrence(RecordChange::replace(&moved.artwork.occurrences, inner, None).unwrap()),
            Edit::Stack(RecordChange::replace(&moved.artwork.stacks, containing, Some(stack)).unwrap())])).unwrap();
        assert!(children.iter().all(|h| new_inner.index() > h.index()));
        assert_close(&render(&mut r, &moved), &passing, 2e-5, &format!("{blend_space:?}: a group stored after its layers"));
    }
}

/// The mask's coverage at each pixel, from an isolated group that holds one
/// opaque layer.
fn coverage(r: &mut WgpuRasterizer, mask: &CoverageSnapshot) -> Vec<f32> {
    let mut b = Builder::new();
    let probe = b.group("Mask", None, LayerBlend::Normal);
    b.mask(probe, mask);
    b.paint("Opaque", Some(probe), LayerBlend::Normal, 1, |_, _| 65535);
    let paper = *b.0.scene().order().last().unwrap();
    b.layer(paper).visible = false;
    let view = crate::test_support::view(EXTENT);
    r.submit(FramePacket { view, reset_layers: true, ..packet(b.0.scene(), EXTENT) }).unwrap();
    crate::layer_tests::page_bytes(r, crate::test_support::document_texture(r))
        .chunks_exact(16)
        .map(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()))
        .collect()
}

#[test]
fn opacity_and_mask_fade_between_the_backdrop_and_the_groups_result() {
    let mut r = WgpuRasterizer::new_native_headless(COLOR).expect("physical GPU required");
    let (mut document, outer) = nested();
    let first = document.scene().order()[0];
    document.artwork.occurrences.get_mut(first).unwrap().blend = LayerBlend::Normal;
    document.artwork.occurrences.get_mut(first).unwrap().opacity = 0.7;
    let mut mask = CoverageSnapshot::reveal_all(CoverageHandle::from_index(0), EXTENT, Point { x: 9., y: 4. });
    mask.source.default_coverage = 0.25;
    mask.source.initial = Some(
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
            g.mask = None;
        });
        if masked { let mut b = Builder(faded); b.mask(outer, &mask); faded = b.0; }
        if nested {
            let mut b = Builder(faded);
            let around = b.group("Around", None, LayerBlend::Normal);
            b.layer(around).opacity = 0.7;
            let root = b.0.composition().result;
            let mut stack = b.0.artwork.stacks.get(root).unwrap().clone();
            stack.entries.retain(|h| *h != around);
            let at = stack.entries.iter().position(|h| *h == outer).unwrap();
            stack.entries[at] = around;
            let OccurrenceContent::Stack(children) = b.layer(around).content else { panic!("group") };
            b.0.apply(Edit::Batch(vec![
                Edit::Stack(RecordChange::replace(&b.0.artwork.stacks, root, Some(stack)).unwrap()),
                Edit::Stack(RecordChange::replace(&b.0.artwork.stacks, children, Some(Stack { entries: vec![outer] })).unwrap())])).unwrap();
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
    b.layer(group).clipped = true;
    b.layer(group).opacity = 0.75;
    b.effect("Black & White", Some(group), "black_white");
    b.paint("Multiply", Some(group), LayerBlend::Multiply, 5, disc(150, 140, 110));
    b.paint("Base", None, LayerBlend::Normal, 11, disc(140, 150, 120));
    b.paint("Backdrop", None, LayerBlend::Normal, 19, |_, _| 65535);
    let clipped = render(&mut r, &b.0);
    let normal = render(&mut r, &set(&b.0, group, |g| g.blend = LayerBlend::Normal));
    assert_close(&clipped, &normal, 1e-6, "a clipped Pass Through group against a clipped Normal group");
    let unclipped = render(&mut r, &set(&b.0, group, |g| g.clipped = false));
    assert!(largest_difference(&clipped, &unclipped) > 0.1, "unclipped, the group passes through");
}
