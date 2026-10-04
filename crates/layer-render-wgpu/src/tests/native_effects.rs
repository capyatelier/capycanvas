use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::{EffectInstance, EffectKind, EffectPass, EffectSampling, EffectValue};

#[path = "native_effects/tone.rs"]
mod tone;
#[path = "native_effects/fills.rs"]
mod fills;
#[path = "native_effects/color.rs"]
mod color;

use layer_core::authored::{Artwork, PortableId, Occurrence, OccurrenceContent, OccurrenceHandle, PaintSource, Definition, EffectApplication};
use layer_core::Document;

pub(crate) fn empty_document(extent: [u32;2], color: DocumentColor) -> Document {
    let mut artwork = Artwork::new(extent).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    Document::from_artwork(artwork).unwrap()
}
pub(crate) fn refresh(document: &mut Document) {
    let owner = document.owner; let revision = document.revision; let working = document.working.clone();
    *document = Document::from_artwork(document.artwork.clone()).unwrap();
    document.owner = owner; document.revision = revision; document.working = working;
}
pub(crate) fn insert_effect(document: &mut Document, effect: EffectInstance) -> OccurrenceHandle {
    let name = effect.program.id.clone();
    let definition = document.artwork.definitions.insert(PortableId::random(), Definition {program:effect.program}).unwrap();
    let application = document.artwork.effects.insert(PortableId::random(), EffectApplication {definition,values:effect.values,domain:document.composition().size}).unwrap();
    let occurrence = document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),name)).unwrap();
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(occurrence);
    refresh(document);
    occurrence
}
pub(crate) fn insert_source(document: &mut Document, name: &str, source: Arc<layer_core::color::source::SourceImage>) -> OccurrenceHandle {
    let source = document.artwork.paint.insert(PortableId::random(),PaintSource {domain:source.extent,original:Some(source),raster:Default::default(),operations:Default::default()}).unwrap();
    let occurrence = document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),name)).unwrap();
    document.artwork.stacks.get_mut(document.composition().result).unwrap().entries.push(occurrence);
    refresh(document);
    occurrence
}
pub(crate) fn effect_document(effects: &[EffectInstance], extent: [u32;2], color: DocumentColor) -> Document {
    let mut document = empty_document(extent,color);
    let mut adjustment = false;
    for effect in effects {
        let handle = insert_effect(&mut document,effect.clone());
        if effect.program.kind == EffectKind::Generator && adjustment {
            let stack = document.artwork.stacks.insert(PortableId::random(),layer_core::authored::Stack {entries:vec![handle]}).unwrap();
            let group = document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Stack(stack),"source")).unwrap();
            let root = document.composition().result;
            let entry = document.artwork.stacks.get_mut(root).unwrap().entries.iter_mut().find(|h|**h==handle).unwrap();
            *entry = group;
            refresh(&mut document);
        }
        adjustment |= effect.program.kind == EffectKind::Adjustment;
    }
    document
}
pub(crate) fn set_effect(document: &mut Document, occurrence: OccurrenceHandle, key: &str, value: EffectValue) {
    let view = document.scene().effect(occurrence).unwrap();
    let mut draft = EffectInstance {program: document.artwork.definitions.get(document.scene().effect_application(occurrence).unwrap().definition).unwrap().program.clone(), values:view.values.to_vec()};
    draft.set(key,value).unwrap();
    let handle = document.scene().effect_handle(occurrence).unwrap();
    document.artwork.effects.get_mut(handle).unwrap().values = draft.values;
}
pub(crate) fn mask(document: &mut Document, owner: OccurrenceHandle, default: f32) {
    let coverage = document.artwork.coverage.next_handle();
    let mut snapshot = layer_core::CoverageSnapshot::reveal_all(coverage,document.composition().size,Default::default());
    snapshot.source.default_coverage = default;
    document.artwork.coverage.insert(PortableId::random(),snapshot.source).unwrap();
    document.artwork.occurrences.get_mut(owner).unwrap().mask = Some(snapshot.use_);
    refresh(document);
}
pub(crate) fn roundtrip(document: Document) -> Document {
    let capture = layer_core::Editor::new(document).capture(0,Default::default()).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut bytes = Vec::new();
    layer_core::package::codec::PreparedPackage::prepare(&capture,None,&cancel).unwrap().write(&mut bytes,&cancel).unwrap();
    read_document(std::io::Cursor::new(bytes))
}
pub(crate) fn read_document(mut input: impl std::io::Read + std::io::Seek) -> Document {
    let mut bytes = Vec::new(); input.read_to_end(&mut bytes).unwrap();
    let backing = layer_core::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    let layer_core::package::codec::OpenOutcome::Candidate {artwork,..} = layer_core::package::codec::open(backing,Default::default(),&std::sync::atomic::AtomicBool::new(false)).unwrap() else { panic!("Expected editable effect fixture") };
    let document = Document::from_artwork(artwork).unwrap();
    document.validate(Default::default()).unwrap();
    layer_color::validate_document_color(&document).unwrap();
    document
}
fn effect(_id: u64, name: &str, image: bool) -> EffectInstance {
    let mut program = (*fixture(name).program()).clone();
    if image {program.passes = vec![EffectPass {entry:program.entry.clone(),sampling:EffectSampling::Neighborhood {radius:0}}].into();}
    EffectInstance::new(Arc::new(program))
}
fn set(effect: &mut EffectInstance, name: &str, value: EffectValue) {effect.set(name,value).unwrap();}
fn source(rgb: [f32;3], alpha: f32) -> EffectInstance {
    let mut effect = effect(1,"exposure",false);
    let program = Arc::make_mut(&mut effect.program);
    program.kind = EffectKind::Generator; program.entry = "fixture".into();
    program.wgsl = format!("fn fixture(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{return vec4<f32>({:?},{:?},{:?},{:?});}}",rgb[0]*alpha,rgb[1]*alpha,rgb[2]*alpha,alpha).into();
    effect
}
fn frame_document(r: &mut WgpuRasterizer, document: &Document) -> [f32;4] {
    let document = Document::from_artwork(document.artwork.clone()).unwrap();
    r.submit(packet(document.scene().with_owner(0,0),document.composition().size)).unwrap();
    let bytes = crate::layer_tests::page_bytes(r,crate::test_support::document_texture(r));
    std::array::from_fn(|c| f32::from_le_bytes(bytes[c*4..c*4+4].try_into().unwrap()))
}
fn frame(r: &mut WgpuRasterizer, effects: &[EffectInstance]) -> [f32;4] {
    let document = effect_document(effects,[256;2],r.document_color);
    frame_document(r,&document)
}
fn close(actual: [f32; 4], rgb: [f32; 3], alpha: f32, context: &str) {
    assert_eq!(actual[3], alpha, "coverage {context}");
    for c in 0..3 {
        let straight = if alpha == 0. {
            actual[c]
        } else {
            actual[c] / alpha
        };
        let expected = if alpha == 0. { 0. } else { rgb[c] };
        assert!(
            (straight - expected).abs() <= 2e-6,
            "{context}: channel {c}: {straight} vs {expected}"
        );
    }
}

#[test]
fn native_exposure_retains_extended_low_alpha_through_fused_and_physical_chains() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let mut r =
                WgpuRasterizer::new_native_headless(DocumentColor { space, depth }).unwrap();
            for image in [false, true] {
                for alpha in [0., 0.00000008, 1. / 65535., 0.5, 1.] {
                    let rgb = [-0.125, 0.234567, 1.5];
                    let mut layers = vec![source(rgb, alpha)];
                    for i in 0..24 {
                        let mut ev = effect(2 + i, "exposure", image);
                        set(
                            &mut ev,
                            "exposure",
                            EffectValue::Number(if i % 2 == 0 { 5. } else { -5. }),
                        );
                        layers.insert(0, ev);
                    }
                    let out = frame(&mut r, &layers);
                    close(
                        out,
                        rgb,
                        alpha,
                        &format!("{space:?} {depth:?} image={image}"),
                    );
                    // A slider evaluates the retained input, and restoring it
                    // recovers the same result without baking prior outputs.
                    set(&mut layers[0], "exposure", EffectValue::Number(-4.));
                    close(
                        frame(&mut r, &layers),
                        rgb.map(|v| v * 2.),
                        alpha,
                        &format!("slider {space:?} {depth:?} image={image} alpha={alpha}"),
                    );
                    set(&mut layers[0], "exposure", EffectValue::Number(-5.));
                    assert_eq!(frame(&mut r, &layers), out);
                }
            }
        }
    }
}

#[test]
fn native_white_balance_and_encoded_tone_helpers_use_document_primaries_and_transfer() {
    for space in RgbSpace::ALL {
        let mut r = WgpuRasterizer::new_native_headless(DocumentColor {
            space,
            depth: SampleDepth::U16,
        })
        .unwrap();
        for image in [false, true] {
            let alpha = 0.00000008;
            let rgb = [0.75, 0.2, 0.05];
            let mut wb = effect(2, "white_balance", image);
            set(&mut wb, "temperature", EffectValue::Number(50.));
            set(&mut wb, "tint", EffectValue::Number(-30.));
            set(&mut wb, "preserve_luminance", EffectValue::Toggle(false));
            let mut layers = vec![wb, source(rgb, alpha)];
            let adjusted = [
                0.5f64 * 0.8 - 0.3 * 0.25,
                0.3 * 0.5,
                -0.5 * 0.8 - 0.3 * 0.25,
            ];
            let raw: [f64; 3] = std::array::from_fn(|c| f64::from(rgb[c]) * adjusted[c].exp2());
            close(
                frame(&mut r, &layers),
                raw.map(|v| v as f32),
                alpha,
                "white balance gains",
            );
            set(
                &mut layers[0],
                "preserve_luminance",
                EffectValue::Toggle(true),
            );
            let w = space.to_xyz()[1];
            let before: f64 = (0..3).map(|c| w[c] * f64::from(rgb[c])).sum();
            let after: f64 = (0..3).map(|c| w[c] * raw[c]).sum();
            close(
                frame(&mut r, &layers),
                raw.map(|v| (v * before / after) as f32),
                alpha,
                "profile luminance",
            );
            // Brightness is deliberately encoded-domain; its +10 control adds
            // 0.1 in this document's transfer curve, without an SDR range clamp.
            layers[0] = effect(2, "brightness_contrast", image);
            set(&mut layers[0], "brightness", EffectValue::Number(10.));
            let expected = rgb.map(|v| space.decode(space.encode(f64::from(v)) + 0.1) as f32);
            close(
                frame(&mut r, &layers),
                expected,
                alpha,
                "profile tone curve",
            );
        }
    }
}

#[test]
fn native_photo_adjustments_and_masks_remain_editable_after_save_reopen() {
    use layer_core::color::ColorProfile;
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8,SampleDepth::U16] {
            let color = DocumentColor {space,depth};
            let mut document = empty_document([256;2],color);
            let mut effects = Vec::new();
            for name in ["exposure","white_balance","levels","curves","hue_saturation","color_balance"] {
                let mut draft = effect(0,name,false);
                match name {
                    "exposure" => set(&mut draft,"exposure",EffectValue::Number(0.75)),
                    "white_balance" => set(&mut draft,"temperature",EffectValue::Number(25.)),
                    "hue_saturation" => set(&mut draft,"hue",EffectValue::Number(10.)),
                    "levels" => {set(&mut draft,"black",EffectValue::Number(0.03));set(&mut draft,"gamma",EffectValue::Number(0.9));set(&mut draft,"clamp_input",EffectValue::Toggle(true));},
                    "curves" => set(&mut draft,"curve_0",EffectValue::Curve(vec![[0.,0.],[0.213,0.13],[0.79,0.9],[1.,1.]])),
                    "color_balance" => {set(&mut draft,"midtones_red",EffectValue::Number(12.));set(&mut draft,"shadows_blue",EffectValue::Number(-5.));},
                    _ => unreachable!(),
                }
                effects.push(draft);
            }
            for draft in effects.into_iter().rev() {let h=insert_effect(&mut document,draft);mask(&mut document,h,0.5);}
            let original = crate::test_support::depth_source([256;2],depth,space,4*1024*1024,|x,y| {
                let codes = [x*257,y*257,(x*101+y*237)%65536,65535];
                codes.map(|code| if depth==SampleDepth::U8 {(code/257) as f32/255.} else {code as f32/65535.})
            });
            let source = insert_source(&mut document,"retained original",original);
            document.working.occurrence = Some(source);document.working.target=document.scene().source_target(source);
            let mut r=WgpuRasterizer::new_native_headless(color).unwrap();frame_document(&mut r,&document);
            let before=crate::layer_tests::page_bytes(&r,crate::test_support::document_texture(&r));
            let mut loaded=roundtrip(document);assert_eq!(loaded.composition().color,color);
            assert!(loaded.scene().order()[..6].iter().all(|h|loaded.scene().effect(*h).is_some()&&loaded.scene().mask(*h).is_some()));
            let mut fresh=WgpuRasterizer::new_native_headless(color).unwrap();frame_document(&mut fresh,&loaded);
            assert_eq!(crate::layer_tests::page_bytes(&fresh,crate::test_support::document_texture(&fresh)),before);
            let exposure=loaded.scene().order().iter().copied().find(|h|loaded.scene().occurrence(*h).unwrap().name.as_ref()=="exposure").unwrap();
            set_effect(&mut loaded,exposure,"exposure",EffectValue::Number(-1.));frame_document(&mut fresh,&loaded);
            assert_ne!(crate::layer_tests::page_bytes(&fresh,crate::test_support::document_texture(&fresh)),before);
            set_effect(&mut loaded,exposure,"exposure",EffectValue::Number(0.75));frame_document(&mut fresh,&loaded);
            assert_eq!(crate::layer_tests::page_bytes(&fresh,crate::test_support::document_texture(&fresh)),before);
            let source=loaded.scene().paint_source(*loaded.scene().order().last().unwrap()).unwrap();
            assert_eq!(source.original.as_ref().unwrap().interpretation.profile,ColorProfile::Builtin(space));
            assert_eq!(source.original.as_ref().unwrap().interpretation.depth,depth);assert!(source.raster.is_empty());
        }
    }
}
