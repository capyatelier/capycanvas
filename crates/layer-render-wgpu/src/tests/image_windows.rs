//! Cropped captures must be the same document, including halos and clipping.
use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::authored::*;
use layer_core::{EffectInstance, EffectKind, EffectPass, EffectSampling, Document, CoverageSnapshot, Selection};

pub(crate) fn document(extent: [u32; 2], color: DocumentColor) -> Document {
    let mut artwork = Artwork::new(extent).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    Document::from_artwork(artwork).unwrap()
}
pub(crate) fn effect(doc: &mut Document, generator: bool, global: bool) -> OccurrenceHandle {
    let handle = add_effect(doc, program(generator, global));
    if !generator { return handle; }
    let stack = doc.artwork.stacks.insert(PortableId::random(),Stack { entries:vec![handle] }).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"generated content")).unwrap()
}
pub(crate) fn add_effect(doc: &mut Document, instance: EffectInstance) -> OccurrenceHandle {
    let size=doc.composition().size;
    let application=doc.artwork.effects.insert(PortableId::random(),EffectApplication::new(instance.program,instance.values,size)).unwrap();
    doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Effect(application), "window fixture")).unwrap()
}
pub(crate) fn set_entries(doc: &mut Document, entries: Vec<OccurrenceHandle>) {
    let root = doc.composition().result;
    let edit = RecordChange::replace(&doc.artwork.stacks, root, Some(Stack { entries })).unwrap();
    doc.apply(layer_core::Edit::Stack(edit)).unwrap();
}
pub(crate) fn set_mask(doc: &mut Document, owner: OccurrenceHandle, mask: CoverageSnapshot) {
    doc.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().mask = Some(mask.use_);
    let entries = doc.artwork.stacks.get(doc.composition().result).unwrap().entries.clone();
    set_entries(doc, entries);
}

pub(crate) fn insert_effect(doc: &mut Document, instance: EffectInstance) -> OccurrenceHandle {
    let owner = add_effect(doc, instance);
    let mut entries = doc.artwork.stacks.get(doc.composition().result).unwrap().entries.clone();
    entries.insert(0, owner);
    set_entries(doc, entries);
    owner
}

pub(crate) fn program(generator: bool, global: bool) -> EffectInstance {
    let mut p = (*fixture("exposure").program()).clone();
    p.kind = if generator {
        EffectKind::Generator
    } else {
        EffectKind::Adjustment
    };
    if generator {
        p.entry = "pattern".into();
        p.wgsl = "fn pattern(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{let a=select(.31,.73,(u32(p.x)/13u+u32(p.y)/11u)%2u==0u);return vec4<f32>(vec3<f32>(fract(p.x/37.),fract(p.y/29.),.27)*a,a);}".into();
        p.passes = Arc::from([]);
    } else {
        p.entry = "horizontal".into();
        p.wgsl = if global {
            "fn horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return fx_sample(fx_extent()-p);}".into()
        } else {
            "fn horizontal(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(3.,0.))+c+fx_sample(p+vec2<f32>(3.,0.)))/3.;} fn vertical(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p-vec2<f32>(0.,2.))+c+fx_sample(p+vec2<f32>(0.,2.)))/3.;}".into()
        };
        p.passes = if global {
            vec![EffectPass {
                entry: "horizontal".into(),
                sampling: EffectSampling::Document,
            }]
        } else {
            vec![
                EffectPass {
                    entry: "horizontal".into(),
                    sampling: EffectSampling::Neighborhood { radius: 3 },
                },
                EffectPass {
                    entry: "vertical".into(),
                    sampling: EffectSampling::Neighborhood { radius: 2 },
                },
            ]
        }
        .into();
    }
    EffectInstance::new(Arc::new(p))
}

fn capture(
    r: &mut WgpuRasterizer,
    scene: &mut scene::Scene,
    packet: FramePacket<'_>,
    crop: PixelRect,
) -> Vec<u8> {
    let (target, _) =
        create_color_target(&r.device, [crop.width(), crop.height()], "window oracle");
    let mut encoder = crate::submission::CommandEncoder::new(
        &r.device,
        &wgpu::CommandEncoderDescriptor::default(),
    );
    scene
        .capture_region(r, packet, &target, crop, scene::Output::Artwork(None), &mut encoder)
        .unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    crate::layer_tests::page_bytes(r, &target)
}

#[test]
fn image_windows_match_full_composition_with_halos_masks_and_clipping() {
    let extent = [777, 533];
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: SampleDepth::U16,
        })
        .unwrap();
        for clipped in [false, true] {
            let mut document = document(extent, DocumentColor { space, depth: SampleDepth::U16 });
            let first = effect(&mut document, false, false);
            let second = effect(&mut document, false, false);
            for owner in [first, second] {
                let occurrence = document.artwork.occurrences.get_mut(owner).unwrap();
                occurrence.opacity = 0.63;
                occurrence.attachment = if clipped { layer_core::Attachment::Effect } else { layer_core::Attachment::None };
            }
            let mut mask = CoverageSnapshot::reveal_all(document.artwork.coverage.next_handle(), extent, [7, -9]);
            mask.source.default_coverage = 0.;
            crate::test_support::materialize_mask(&mut mask.source, Selection::polygon(vec![Point { x: 0., y: 0. }, Point { x: 760., y: 99. }, Point { x: 440., y: 533. }]).unwrap(), document.composition().color);
            document.artwork.coverage.insert(PortableId::random(), mask.source).unwrap();
            document.artwork.occurrences.get_mut(first).unwrap().mask = Some(mask.use_);
            let inside = effect(&mut document, true, false);
            let outside = effect(&mut document, true, false);
            let stack = document.artwork.stacks.insert(PortableId::random(), Stack { entries: vec![first, second, inside] }).unwrap();
            let mut group = Occurrence::new(OccurrenceContent::Stack(stack), "isolated");
            group.opacity = 0.79;
            let group = document.artwork.occurrences.insert(PortableId::random(), group).unwrap();
            set_entries(&mut document, vec![group, outside]);
            let packet = crate::test_support::packet(document.scene(), extent);
            r.submit(packet).unwrap(); // Initializes real mask pages and renderer metadata.
            let mut scene = scene::Scene::new(&r);
            let full = capture(&mut r, &mut scene, packet, PixelRect::full(extent));
            let full_cache = scene.image_cache_bytes();
            for crop in [
                PixelRect::new(249, 251, 279, 281),
                PixelRect::new(17, 33, 49, 89),
                PixelRect::new(752, 511, 777, 533),
                PixelRect::new(0, 0, 23, 27),
            ] {
                let pixels = capture(&mut r, &mut scene, packet, crop);
                for y in 0..crop.height() as usize {
                    for x in 0..crop.width() as usize {
                        let src = ((y + crop.min_y() as usize) * extent[0] as usize
                            + x
                            + crop.min_x() as usize)
                            * 16;
                        let dst = (y * crop.width() as usize + x) * 16;
                        for c in 0..4 {
                            let a = f32::from_le_bytes(pixels[dst + c * 4..dst + c * 4 + 4].try_into().unwrap());
                            let b = f32::from_le_bytes(full[src + c * 4..src + c * 4 + 4].try_into().unwrap());
                            assert!(
                                (a - b).abs() <= 2e-6,
                                "{space:?} clipped={clipped} crop={crop:?} ({x},{y}) c={c}: {a} != {b}"
                            );
                        }
                    }
                }
                assert!(
                    scene.image_cache_bytes() < full_cache / 20,
                    "cropped boundaries must allocate only their window"
                );
            }
            // Reusing the same Scene must restore full bounds, without stale pixels.
            assert_eq!(
                capture(&mut r, &mut scene, packet, PixelRect::full(extent)),
                full
            );
        }
    }
}

#[test]
fn image_windows_keep_document_sampler_dependencies_complete() {
    let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    })
    .unwrap();
    let extent = [333, 291];
    let mut document = document(extent, DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    let entries = vec![effect(&mut document, false, true), effect(&mut document, false, false), effect(&mut document, true, false)];
    set_entries(&mut document, entries);
    let packet = FramePacket { view: test_view(), ..crate::test_support::packet(document.scene(), extent) };
    r.submit(packet).unwrap();
    let mut scene = scene::Scene::new(&r);
    let full = capture(&mut r, &mut scene, packet, PixelRect::full(extent));
    let cache = scene.image_cache_bytes();
    let crop = PixelRect::new(271, 3, 295, 19);
    let pixels = capture(&mut r, &mut scene, packet, crop);
    assert_eq!(
        scene.image_cache_bytes(),
        cache,
        "global remapping must retain the declared full input"
    );
    for y in 0..crop.height() as usize {
        let a = y * crop.width() as usize * 16;
        let b = ((y + crop.min_y() as usize) * extent[0] as usize + crop.min_x() as usize) * 16;
        assert_eq!(
            &pixels[a..a + crop.width() as usize * 16],
            &full[b..b + crop.width() as usize * 16]
        );
    }
}

#[test]
fn retained_off_frame_source_blurs_into_frame_and_matches_larger_reference() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F32 };
    let source = crate::test_support::depth_source([64, 32], SampleDepth::F32, color.space, 1 << 20,
        |x, y| if (20..32).contains(&x) && (7..25).contains(&y) { [0.7, 0.2, 0.1, 1.] } else { [0.; 4] });
    let make = |extent, offset| {
        let mut doc = crate::tests::native_effects::empty_document(extent, color);
        let mut blur = EffectInstance::new(layer_core::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
        blur.set("sigma", layer_core::EffectValue::Number(4.)).unwrap();
        crate::tests::native_effects::insert_effect(&mut doc, blur);
        let paint = crate::tests::native_effects::insert_source(&mut doc, "retained source", source.clone());
        doc.artwork.occurrences.get_mut(paint).unwrap().offset = offset;
        crate::tests::native_effects::refresh(&mut doc);
        doc
    };
    let document = make([32, 32], [-32, 0]);
    let reference = make([96, 64], [0, 16]);
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    let mut scene = scene::Scene::new(&r);
    let actual = capture(&mut r, &mut scene, crate::test_support::packet(document.scene(), [32, 32]), PixelRect::full([32, 32]));
    assert!(scene::Scene::capture_window(document.scene(), PixelRect::full([32, 32]), [32, 32]).min[0] < 0);
    let expected = capture(&mut r, &mut scene, crate::test_support::packet(reference.scene(), [96, 64]), PixelRect::new(32, 16, 64, 48));
    let actual = crate::test_support::floats(&actual);
    let expected = crate::test_support::floats(&expected);
    assert!(actual.iter().any(|pixel| pixel[3] > 0.01), "retained marks outside the frame must contribute inside it");
    assert!(crate::test_support::max_error(&actual, &expected) < 3e-5);
}
