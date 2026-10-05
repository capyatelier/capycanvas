//! Catalog-wide filter rendering, animation and performance gates.
use super::*;
use layer_core::{EffectAlpha, EffectValue, EffectInstance, Document};
use super::native_effects::{empty_document,insert_effect,insert_source,set_effect,mask,refresh};
const EXTENT: [u32; 2] = [384, 256];

#[path = "filter_investigation.rs"]
mod investigation;

fn artwork([width, height]: [u32; 2]) -> Vec<u8> {
    // Original test artwork: gradients, curved silhouettes, bright highlights,
    // fine texture and transparent edges. No external image/licensing inputs.
    (0..width * height)
        .flat_map(|i| {
            let x = i % width;
            let y = i / width;
            let u = x as f32 / width as f32;
            let v = y as f32 / height as f32;
            let mut color = [
                0.15 + 0.5 * u,
                0.25 + 0.45 * (1. - v),
                0.65 + 0.2 * (1. - u),
            ];
            if (u - 0.72).hypot(v - 0.22) < 0.095 {
                color = [1., 0.94, 0.67];
            }
            let ridge = 0.52 + 0.08 * (u * 11.).sin() + 0.04 * (u * 29.).cos();
            if v > ridge {
                color = [0.12 + 0.15 * u, 0.30 + 0.3 * (1. - v), 0.22 + 0.15 * u];
            }
            if v > 0.76 + 0.05 * (u * 8.).sin() {
                color = [0.4 + 0.25 * u, 0.20 + 0.1 * v, 0.09];
            }
            if (u - 0.25).abs() < 0.09 && (0.44..0.78).contains(&v) {
                color = [0.82, 0.23, 0.14];
            }
            if (x / 5 + y / 7) % 13 == 0 {
                for c in &mut color {
                    *c = (*c * 0.75 + 0.1).min(1.);
                }
            }
            let noise =
                ((x.wrapping_mul(1664525) ^ y.wrapping_mul(1013904223)) & 31) as f32 / 255. - 0.06;
            let alpha = if x < 8 || y < 8 || x + 8 >= width || y + 8 >= height {
                0
            } else {
                255
            };
            [
                (255. * (color[0] + noise).clamp(0., 1.)) as u8,
                (255. * (color[1] + noise).clamp(0., 1.)) as u8,
                (255. * (color[2] + noise).clamp(0., 1.)) as u8,
                alpha,
            ]
        })
        .collect()
}
fn setup(extent: [u32;2]) -> Document {
    let bytes=artwork(extent);let mut document=empty_document(extent,Default::default());
    insert_source(&mut document,"Artwork",layer_core::color::source::rgba8_source(extent,|x,y|{let i=(y*extent[0]+x) as usize*4;bytes[i..i+4].try_into().unwrap()}));
    document
}
fn filter(id: &layer_core::EffectDefinition) -> EffectInstance {id.preview().unwrap()}
fn filtered(base: &Document, draft: EffectInstance) -> (Document,OccurrenceHandle) {
    let mut document=base.clone();let h=insert_effect(&mut document,draft);let root=document.composition().result;
    let entries=&mut document.artwork.stacks.get_mut(root).unwrap().entries;entries.pop();entries.insert(0,h);refresh(&mut document);(document,h)
}
fn submit(
    r: &mut WgpuRasterizer,
    extent: [u32; 2],
    document: &Document,
    time: f32,
    reset: bool,
    all: bool,
    paint: Option<([f32; 2], f32)>,
) {
    let mut dabs = Vec::new();
    let mut batches = Vec::new();
    if let Some((center, radius)) = paint {
        let mut dab = test_dab(center, [0.75, 0.06, 0.8, 1.], 1.);
        dab.radii = [radius; 2];
        dabs.push(dab);
        batches.push(DabBatch {
            stroke_id: StrokeId(77),
            ..crate::test_support::dab_batch(
                document.scene().order().iter().find_map(|h|document.scene().source_target(*h)).unwrap(),
                test_style(BrushExecution::Dry),
                Rect { min: Point { x: center[0] - radius - 1., y: center[1] - radius - 1. }, max: Point { x: center[0] + radius + 1., y: center[1] + radius + 1. } },
            )
        });
    }
    let view = ViewState {
        width_px: extent[0],
        height_px: extent[1],
        ..test_view()
    };
    let context=layer_core::EvaluationContext {elapsed:time,phases:Vec::new().into()};
    r.submit(FramePacket {
        view,
        time_seconds: time,
        dabs: &dabs,
        dab_batches: &batches,
        reset_layers: reset,
        composite_all: all,
        ..packet(document.scene().with_context(&context), extent)
    })
    .unwrap();
}
fn image(r: &mut WgpuRasterizer) -> Vec<u8> {
    r.readback_srgb_rgba8().unwrap()
}
fn prepare_analysis(r: &mut WgpuRasterizer, document: &Document, target: OccurrenceHandle, time: f32) {
    if document.scene().effect(target).unwrap().program.analysis().is_none() { return; }
    let mut query = layer_core::ArtworkQuery::new(document, layer_core::ArtworkSource::EffectInput(target));
    query.set_context(layer_core::EvaluationContext {elapsed: time, phases: Vec::new().into()});
    let candidate = pollster::block_on(r.snapshot_gpu().effect_analysis(query, crate::snapshot::CaptureControl::default())).unwrap();
    assert!(candidate.entries.iter().any(|entry| entry.layer() == target));
    r.apply_effect_analysis(candidate);
}

#[test]
fn saved_procedural_patterns_keep_their_noise() {
    fn random([x, y]: [f32; 2], seed: u32) -> f32 {
        let h = (x.floor() as i32 as u32).wrapping_mul(1664525).wrapping_add((y.floor() as i32 as u32).wrapping_mul(1013904223))
            .wrapping_add(seed.wrapping_mul(747796405));
        let h = (h ^ (h >> 15)).wrapping_mul(2246822519);
        (h ^ (h >> 13)) as f32 / 4294967295.
    }
    fn noise(p: [f32; 2], seed: u32) -> f32 {
        let q = p.map(f32::floor);
        let t = p.map(|v| { let f = v - v.floor(); f * f * (3. - 2. * f) });
        let corner = |dx: f32, dy: f32| random([q[0] + dx, q[1] + dy], seed);
        let mix = |a: f32, b: f32, t: f32| a + (b - a) * t;
        mix(mix(corner(0., 0.), corner(1., 0.), t[0]), mix(corner(0., 1.), corner(1., 1.), t[0]), t[1]) * 2. - 1.
    }
    let grain = layer_core::bundled_effect_catalog().get("film_grain").unwrap().program();
    let mut sources = grain.wgsl.sources().unwrap().to_vec();
    sources.push("fn pinned_noise(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fx_random(p,17u),fx_noise(p*.37,3u)*.5+.5,0.,1.)*c.a;}".into());
    let program = std::sync::Arc::new(layer_core::EffectProgram { id: "pinned_noise".into(), label: "Pinned noise".into(),
        wgsl: layer_core::EffectShader::Linked { sources: sources.into() }, entry: "pinned_noise".into(), time: false,
        parameters: Default::default(), pages: Default::default(), constraints: Default::default(), ..(*grain).clone() });
    let extent = [64, 32];
    let (document, _) = filtered(&setup(extent), EffectInstance::new(program));
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    submit(&mut r, extent, &document, 0., true, true, None);
    let pixels = image(&mut r);
    let encoded = |v: f32| (255. * layer_core::color::RgbSpace::Srgb.encode(f64::from(v))).round() as f32;
    for y in 8..extent[1] - 8 {
        for x in 8..extent[0] - 8 {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let i = ((y * extent[0] + x) * 4) as usize;
            for (channel, expected) in [random(p, 17), noise(p.map(|v| v * 0.37), 3) * 0.5 + 0.5].into_iter().enumerate() {
                assert!((f32::from(pixels[i + channel]) - encoded(expected)).abs() <= 1., "noise channel {channel} changed at {x},{y}");
            }
        }
    }
}

#[test]
fn zero_pixel_periods_and_large_lengths_render_finite_pixels() {
    let extent=[32,24];let base=setup(extent);
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for definition in fixtures() {
        let mut effect=EffectInstance::new(definition.program());
        let lengths:Vec<_>=effect.program.parameters.iter().filter(|p|matches!(p.dimension,
            layer_core::authored::Dimension::SourcePixels|layer_core::authored::Dimension::CompositionPixels)).map(|p|p.key.clone()).collect();
        if lengths.is_empty() {continue;}
        for value in [0.,65536.] {
            for key in &lengths {effect.set(key,EffectValue::Number(value)).unwrap();}
            let (document,_)=filtered(&base,effect.clone());
            submit(&mut r,extent,&document,0.,true,true,None);
            let pixels=crate::test_support::float_pixels(&r,crate::test_support::document_texture(&r));
            assert!(pixels.iter().flatten().all(|v|v.is_finite()),"{}: {value}",definition.id());
        }
    }
}

#[test]
fn runtime_manifest_loads_a_new_filter_and_its_preparation() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/filters/tent-blur");
    let manifest = std::fs::read_to_string(directory.join("manifest.json")).unwrap();
    let catalog = layer_core::EffectPackage::parse(&manifest)
        .unwrap()
        .resolve(|name| {
            std::fs::read_to_string(directory.join(name))
                .map(Arc::from)
                .map_err(|e| e.to_string())
        })
        .unwrap();
    let definition = catalog.get("example:tent_blur").unwrap();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let base = setup(EXTENT);
    submit(
        &mut r,
        EXTENT,
        &base,
        0.,
        true,
        true,
        None,
    );
    let original = image(&mut r);
    let (mut document,target)=filtered(&base,filter(definition));
    submit(&mut r, EXTENT, &document, 0., false, true, None);
    assert_ne!(image(&mut r), original);
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 1);
    set_effect(&mut document,target,"radius",EffectValue::Number(0.));
    submit(&mut r, EXTENT, &document, 0., false, true, None);
    assert!(
        image(&mut r)
            .iter()
            .zip(original)
            .all(|(a, b)| a.abs_diff(b) <= 1)
    );
    assert_eq!(r.scene.as_ref().unwrap().effects.preparation_count(), 2);
}

#[test]
fn entire_filter_catalog_renders_masks_freezes_and_animates() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).expect("physical GPU required");
    let base = setup(EXTENT);
    submit(
        &mut r,
        EXTENT,
        &base,
        0.,
        true,
        true,
        None,
    );
    let original = image(&mut r);
    for id in fixtures() {
        let mut draft = filter(id);
        if let Some(layer_core::EffectAuxiliary::Lut3d {resource, ..}) = draft.program.auxiliary.as_ref() {
            let key = resource.clone();
            let samples = (0..8).map(|i| std::array::from_fn(|axis| 1. - ((i >> axis) & 1) as f32)).collect();
            let lut = layer_core::Lut3d::from_samples(2, [[0.; 3], [1.; 3]], "Catalog inversion".into(), samples).unwrap();
            draft.set(&key, EffectValue::Lut3d(Some(Arc::new(lut)))).unwrap();
        }
        let (mut document,target)=filtered(&base,draft);
        prepare_analysis(&mut r, &document, target, 0.);
        submit(&mut r, EXTENT, &document, 0., false, true, None);
        let pages = EXTENT[0].div_ceil(PAGE_SIZE) * EXTENT[1].div_ceil(PAGE_SIZE);
        for step in 0..=pages + 1 {
            if !r.has_pending_work() { break; }
            assert!(step < pages + 1, "{} refinement must finish within one visit per native page", id.id());
            r.wait_idle().unwrap();
            submit(&mut r, EXTENT, &document, 0., false, false, None);
        }
        assert!(!r.has_pending_work(), "{} must settle before testing frozen work", id.id());
        r.wait_idle().unwrap();
        let output = image(&mut r);
        assert_ne!(
            output,
            original,
            "{} preview must demonstrate its effect",
            id.id()
        );
        assert!(
            output.chunks_exact(4).any(|p| p[3] > 0),
            "{} must not erase the image",
            id.id()
        );
        let program = document.scene().effect(target).unwrap().program;
        if program.kind == layer_core::EffectKind::Adjustment && program.alpha == EffectAlpha::Preserve {
            assert!(
                output
                    .chunks_exact(4)
                    .zip(original.chunks_exact(4))
                    .all(|(a, b)| a[3] == b[3]),
                "{} preserves alpha",
                id.id()
            );
        }
        let before = r.scene.as_ref().map_or([0, 0], |s| s.image_work());
        let pixels_before = r.metrics.composited_pixels;
        submit(&mut r, EXTENT, &document, 20., false, false, None);
        assert_eq!(image(&mut r), output, "{} frozen result", id.id());
        assert_eq!(
            r.scene.as_ref().map_or([0, 0], |s| s.image_work()),
            before,
            "{} frozen frame does no image work",
            id.id()
        );
        assert_eq!(r.metrics.composited_pixels, pixels_before, "{} frozen frame composites no pixels", id.id());
        document.artwork.occurrences.get_mut(target).unwrap().opacity = 0.;
        submit(&mut r, EXTENT, &document, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero opacity is identity",
            id.id()
        );
        document.artwork.occurrences.get_mut(target).unwrap().opacity = 1.;
        mask(&mut document,target,0.);
        submit(&mut r, EXTENT, &document, 20., false, true, None);
        assert_eq!(
            image(&mut r),
            original,
            "{} zero mask is identity",
            id.id()
        );
        document.artwork.occurrences.get_mut(target).unwrap().mask = None;
        let source = document.scene().children(None)[1];
        let layer_core::authored::OccurrenceContent::Paint(paint) = document.scene().occurrence(source).unwrap().content else { panic!("paint source"); };
        let original_source = document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.storage().clone();
        let copy = insert_source(&mut document, "Clipped source", original_source);
        let stack = document.artwork.stacks.insert(layer_core::authored::PortableId::random(), layer_core::authored::Stack { entries: vec![target, copy] }).unwrap();
        let mut group = layer_core::authored::Occurrence::new(layer_core::authored::OccurrenceContent::Stack(stack), "Clipped filter");
        group.attachment = layer_core::Attachment::Clip;
        let group = document.artwork.occurrences.insert(layer_core::authored::PortableId::random(), group).unwrap();
        let root = document.composition().result;
        document.artwork.stacks.get_mut(root).unwrap().entries = vec![group, source];
        refresh(&mut document);
        prepare_analysis(&mut r, &document, target, 20.);
        submit(&mut r, EXTENT, &document, 20., false, true, None);
        assert!(
            image(&mut r)
                .chunks_exact(4)
                .zip(original.chunks_exact(4))
                .all(|(a, b)| a[3] == b[3]),
            "{} clipping preserves base coverage",
            id.id()
        );
        if document.scene().effect(target).unwrap().program.time {
            set_effect(&mut document,target,"animate",EffectValue::Toggle(true));
            submit(&mut r, EXTENT, &document, 0., false, true, None);
            let first = image(&mut r);
            submit(&mut r, EXTENT, &document, 1., false, false, None);
            let second = image(&mut r);
            assert_ne!(first, second, "{} animation must change pixels", id.id());
        }
    }
}
