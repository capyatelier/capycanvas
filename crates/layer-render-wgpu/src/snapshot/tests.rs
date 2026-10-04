use super::*;
use crate::test_support::packet;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace, source::SourceBuilder};
use layer_core::raster::{RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey};
use layer_core::{Affine, Document, EffectInstance, Point, Selection, SelectionPixels};
use std::io::Cursor;
use layer_core::authored::*;
use crate::artwork_sample_tests::{paint_id,paint_mut,paint_occurrence,refresh,insert_effect,effect_draft,set_effect,add_group};

mod placement;
mod local_adjustments;

fn gpu() -> SnapshotGpu {
    static GPU: std::sync::OnceLock<SnapshotGpu> = std::sync::OnceLock::new();
    GPU.get_or_init(|| WgpuRasterizer::new_native_headless(Default::default()).unwrap().snapshot_gpu()).clone()
}
fn capture(document: Document) -> Result<SnapshotRenderer, GpuRasterError> {
    gpu().capture_scene(document.snapshot(),SceneScope::All,Default::default())
}

#[test]
fn scene_captures_reject_nonfinite_effect_phases_before_preparing_pixels() {
    let document=Document::new(PortableId::random(),16,16,layer_core::DocumentNames {paint:"Layer".into(),paper:"Paper".into()});
    let effect=document.artwork.effects.iter().next().unwrap().0;
    for phase in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
        let context=EvaluationContext {elapsed:1.,phases:vec![(effect,phase)].into()};
        assert!(matches!(gpu().capture_scene(document.snapshot_with_context(context),SceneScope::All,Default::default()),Err(GpuRasterError::Color(message)) if message=="Invalid snapshot viewing state"));
    }
}

fn hide_paper(document:&mut Document) {
    let paper=document.scene().children(None)[1];document.artwork.occurrences.get_mut(paper).unwrap().visible=false;
}
fn roundtrip(document:&Document)->Document {
    use layer_core::package::{codec::{PreparedPackage,OpenOutcome,open},ImmutableBacking};
    let cancelled=std::sync::atomic::AtomicBool::new(false);
    let checkpoint=CaptureCheckpoint {document:document.artwork.id,owner:document.owner,session_generation:0,artwork_generation:0,working_generation:0,edit_checkpoint:0};
    let capture=document.artwork.capture(checkpoint).unwrap();let mut bytes=Vec::new();
    PreparedPackage::prepare(&capture,None,&cancelled).unwrap().write(&mut bytes,&cancelled).unwrap();
    let backing=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    let OpenOutcome::Candidate {artwork,..}=open(backing,Default::default(),&cancelled).unwrap() else {panic!("editable snapshot");};
    Document::from_artwork(artwork).unwrap()
}

#[test]
fn gaussian_all_sigmas_export_png_with_valid_opaque_and_partial_coverage() {
    let extent=[33,17];
    let input=SourceInterpretation{channels:SourceChannels::Rgba,depth:SampleDepth::F32,
        profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false};
    let target=SourceInterpretation{depth:SampleDepth::U16,..input.clone()};
    for depth in [SampleDepth::U16,SampleDepth::F32] {
    for c in [[0.25_f32,0.5,0.75,1.],[4.,-2.,8.,1.],[0.1,0.2,0.4,0.5]] {
        let mut document=Document::new(PortableId::random(),extent[0],extent[1],
            layer_core::DocumentNames{paint:"Original".into(),paper:"Paper".into()});
        let root=document.artwork.root;
        document.artwork.compositions.get_mut(root).unwrap().color.depth=depth;hide_paper(&mut document);
        let mut source=SourceBuilder::new(extent,input.clone(),1<<20).unwrap();
        let row=(0..extent[0]).flat_map(|_|c).flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        for _ in 0..extent[1]{source.push_row(&row).unwrap();}
        paint_mut(&mut document).original=Some(Arc::new(source.finish().unwrap()));
        let filter=insert_effect(&mut document,EffectInstance::new(crate::tests::fixture("gaussian_blur").program()),0);
        for sigma in [0.,0.1,1.,3.,21.,21.1,64.,85.] {
            let mut draft=effect_draft(&document,filter);
            draft.set("sigma",layer_core::EffectValue::Number(sigma)).unwrap();set_effect(&mut document,filter,draft);
            let mut reader=capture(document.clone()).unwrap();let mut png=Vec::new();
            reader.write_png(&mut png,&target,Default::default(),None)
                .unwrap_or_else(|error|panic!("depth={depth:?} sigma={sigma} original={c:?}: {error}"));
            let decoded=layer_color::photo::read_photo(Cursor::new(png),Default::default()).unwrap();
            assert_eq!(decoded.extent,extent);assert_eq!(decoded.interpretation.depth,SampleDepth::U16);
            let mut bytes=vec![0;decoded.row_bytes()];decoded.rows().read(0,&mut bytes).unwrap();
            let alpha=(c[3]*65535.).round() as u16;
            assert!(bytes.chunks_exact(8).all(|p|u16::from_le_bytes(p[6..8].try_into().unwrap()).abs_diff(alpha)<=1));
        }
    }
    }
}

#[test]
fn read_only_capture_does_not_compile_paint_publication_pipelines() {
    let mut document = Document::new(PortableId::random(), 33, 17, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let fill_color=layer_core::color::RgbColor::from_linear(RgbSpace::Srgb,[0.25,0.5,0.75,1.]).unwrap();
    let color=fill_color.linear_in(RgbSpace::Srgb).unwrap();
    hide_paper(&mut document);
    let mut fill=EffectInstance::new(crate::tests::fixture("solid_color").program());
    fill.set("color",layer_core::EffectValue::Color(fill_color)).unwrap();
    insert_effect(&mut document,fill,1);
    let mut capture = gpu().capture_scene(document.snapshot(),SceneScope::All,Default::default()).unwrap();
    assert!(capture.renderer.native_edit.as_ref().unwrap().pipelines().all(|p| !p.ready()));
    assert!(capture.read_region([0, 0, 33, 17]).unwrap().iter().all(|p| *p == color));
    assert!(capture.renderer.native_edit.as_ref().unwrap().pipelines().all(|p| !p.ready()));
}

#[test]
fn animated_speed_edits_keep_canvas_exact_queries_and_export_in_phase() {
    let mut doc = Document::new(PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let program = crate::tests::fixture("domain_warp").program();
    let mut program = (*program).clone();
    // A uniform time signal makes this independent of the filter's appearance.
    program.wgsl = "fn phase(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return vec4<f32>(fract(fx_time(b)/10.),.25,.5,1.);}".into();
    program.entry = "phase".into();
    program.passes = Arc::new([]);
    let mut effect = EffectInstance::new(Arc::new(program));
    effect.set("animate", layer_core::EffectValue::Toggle(true)).unwrap();
    effect.set("speed", layer_core::EffectValue::Number(1.)).unwrap();
    let filter = insert_effect(&mut doc,effect,0);
    let mut live = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let mut before = None;
    for (elapsed, speed, phase) in [(2.,1.,2.),(2.,2.,2.),(3.,2.,4.),(3.,0.,4.),(8.,0.,4.),(8.,2.,4.),(9.,2.,6.)] {
        let mut effect=effect_draft(&doc,filter);
        effect.set("speed",layer_core::EffectValue::Number(speed)).unwrap();set_effect(&mut doc,filter,effect);
        live.submit(FramePacket {
            view: layer_render::ViewState { width_px:32, height_px:32, document_to_surface: [1.,0.,0.,1.,0.,0.], },
            time_seconds: elapsed,
            reset_layers: before.is_none(),
            ..packet(doc.scene(), [32,32])
        }).unwrap();
        let pixels = live.readback_srgb_rgba8().unwrap();
        if let Some((previous_time, previous_pixels)) = &before {
            if *previous_time == elapsed || speed == 0. { assert_eq!(&pixels, previous_pixels, "rate changes do not seek"); }
            else { assert_ne!(&pixels, previous_pixels, "playback advances"); }
        }
        let mut capture = live.snapshot_gpu().capture_scene(doc.snapshot_with_context(live.evaluation_context()),SceneScope::All, Default::default()).unwrap();
        let exported = capture.renderer.effect_clocks.get(&filter).unwrap().1.clone()
            .advance(doc.scene().effect(filter).unwrap(),elapsed);
        assert_eq!(exported, phase);
        let sample = capture.preview_linear_document([32,32]).unwrap().pixels[0];
        assert!((sample[0] - phase/10.).abs()<0.01, "export phase {sample:?}");
        before=Some((elapsed,pixels));
    }
}

#[test]
fn float32_exr_and_deliberate_pq_sdr_delivery_leave_master_unchanged() {
    let mut document = Document::new(PortableId::random(), 3, 1, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F32;
    hide_paper(&mut document);
    let target = SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::F32, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let input = [[100000.125f32, -0.125, 2., 0.5], [4., 2., 1., 1.], [1e-20, -1., 4., 1. / 65536.]];
    let mut builder = SourceBuilder::new([3, 1], target.clone(), 1024 * 1024).unwrap();
    builder.push_row(&input.into_iter().flatten().flat_map(f32::to_le_bytes).collect::<Vec<_>>()).unwrap();
    paint_mut(&mut document).original = Some(Arc::new(builder.finish().unwrap()));
    let project = document;
    let mut renderer = capture(project.clone()).unwrap();
    let before = renderer.preview_linear_document([3, 1]).unwrap().pixels;
    let mut output = Cursor::new(Vec::new());
    renderer.write_exr(&mut output).unwrap();
    let image = layer_color::photo::read_photo(Cursor::new(output.into_inner()), Default::default()).unwrap();
    assert_eq!(image.interpretation, target);
    let mut bytes = vec![0; image.row_bytes()];
    image.rows().read(0, &mut bytes).unwrap();
    for (encoded, expected) in bytes.chunks_exact(16).zip(input) {
        assert_eq!(layer_core::color::hdr::decode_samples(SampleDepth::F32, encoded).unwrap().map(f32::to_bits), expected.map(f32::to_bits));
    }
    let mut pq = Vec::new();
    assert!(renderer.write_hdr_png(&mut pq, false).is_err());
    pq.clear();
    assert!(renderer.write_hdr_png(&mut pq, true).unwrap().clipped_channels > 0);
    assert!(layer_color::photo::read_photo(Cursor::new(pq), Default::default()).unwrap().interpretation.depth.is_float());
    let mut sdr = Vec::new();
    renderer.write_png(&mut sdr, &SourceInterpretation { depth: SampleDepth::U16, ..target }, Default::default(), None).unwrap();
    assert_eq!(layer_color::photo::read_photo(Cursor::new(sdr), Default::default()).unwrap().interpretation.depth, SampleDepth::U16);
    assert_eq!(renderer.preview_linear_document([3, 1]).unwrap().pixels, before);
}

#[test]
fn shared_float32_bands_and_exr_preserve_samples_across_column_boundaries() {
    let extent = [1027, 33];
    let mut document = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::F32 };
    hide_paper(&mut document);
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::F32,
        profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
        profile_assumed: false,
    };
    let mut source = SourceBuilder::new(extent, target.clone(), 1024 * 1024).unwrap();
    let mut straight = Vec::new();
    let mut expected = Vec::new();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            let alpha = [1., 0.5, 1. / 65536.][x as usize % 3];
            let pixel = [100000.125 + x as f32 / 32., -0.125 - y as f32 / 64., 1e-20, alpha];
            row.extend(pixel.into_iter().flat_map(f32::to_le_bytes));
            expected.push([pixel[0] * alpha, pixel[1] * alpha, pixel[2] * alpha, alpha]);
        }
        source.push_row(&row).unwrap();
        straight.extend(row);
    }
    paint_mut(&mut document).original = Some(Arc::new(source.finish().unwrap()));
    let project = document;
    let (live, rendered) = frame(&project);
    assert_eq!(rendered, expected);
    let mut capture = live.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
    let (rows, pixels) = capture.read_band(0).unwrap();
    assert_eq!(rows, extent[1]);
    assert_eq!(pixels, expected, "shared capture must retain signed, low-alpha and above-half-range samples");
    let mut exr = Cursor::new(Vec::new());
    capture.write_exr(&mut exr).unwrap();
    let decoded = decode(exr.into_inner());
    assert_eq!(decoded.interpretation, target);
    assert_eq!(raw_rows(&decoded), straight);
    let budget = capture.planned_pixel_bytes;
    capture.planned_pixel_bytes = 1;
    assert!(matches!(capture.read_band(0), Err(GpuRasterError::CaptureBudget { .. })));
    capture.planned_pixel_bytes = budget;
    capture.control().cancel();
    assert!(capture.read_band(0).is_err());
}

#[test]
fn hdr_flattened_storage_ignores_sdr_rendition() {
    use layer_core::color::hdr;
    let mut document = Document::new(PortableId::random(), 3, 1, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F16;
    hide_paper(&mut document);
    let target = SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::F16, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let input = [[8., -0.125, 2., 0.5], [4., 2., 1., 1.], [1. / 65536., -1., 4., 1. / 65536.]];
    let mut builder = SourceBuilder::new([3, 1], target.clone(), 1024 * 1024).unwrap();
    builder.push_row(&input.into_iter().flat_map(|p| hdr::encode_pixel(p).unwrap()).flat_map(u16::to_le_bytes).collect::<Vec<_>>()).unwrap();
    paint_mut(&mut document).original = Some(Arc::new(builder.finish().unwrap()));
    let mut effect = EffectInstance::new(layer_core::bundled_effect_catalog().get("exposure").unwrap().program());
    effect.set("exposure", layer_core::EffectValue::Number(1.)).unwrap();
    insert_effect(&mut document,effect,0);
    document.artwork.outputs.get_mut(document.artwork.default_output).unwrap().sdr = hdr::SdrRendition { exposure: -4., contrast: 2., headroom: 4., ..Default::default() };
    let mut renderer = capture(document).unwrap();
    let mut bytes = vec![0; 24];
    renderer.write_rows(&target, Default::default(), None, |_, _, read| read(0, &mut bytes)).unwrap();
    let expected: Vec<_> = input.into_iter().flat_map(|p| hdr::encode_pixel([2. * p[0], 2. * p[1], 2. * p[2], p[3]]).unwrap()).flat_map(u16::to_le_bytes).collect();
    assert_eq!(bytes, expected, "flattening a floating master must retain HDR values");
    let master = renderer.preview_document_with_coverage([3, 1], RgbSpace::Srgb, 49.).unwrap().0;
    let linear = renderer.preview_linear_document([3, 1]).unwrap();
    assert_eq!(linear.pixels, master.pixels, "Proof control cache must not bake SDR mapping into HDR samples");
    let reduced = renderer.preview_linear_document([1, 1]).unwrap();
    for c in 0..4 {
        let mean = linear.pixels.iter().map(|p| p[c]).sum::<f32>() / 3.;
        assert!((reduced.pixels[0][c] - mean).abs() < 1e-6, "linear alpha-aware reduction {c}");
    }
    assert!(renderer.preview_linear_document([0, 128]).is_err());
    assert!(renderer.preview_linear_document([1025, 128]).is_err());
    for (actual, p) in master.pixels.iter().zip(input) {
        for c in 0..3 { assert!((actual[c] - 2. * p[c] * p[3]).abs() < 1e-6, "HDR preview changed the master: {actual:?}"); }
        assert_eq!(actual[3], p[3]);
    }
    let sdr = renderer.preview_document([3, 1], RgbSpace::Srgb).unwrap();
    assert!(sdr.pixels[0][0] < 1.);
    assert!(master.pixels[0][0] > 1.);
    renderer.sdr_rendition = Some(hdr::SdrRendition::default());
    assert_eq!(renderer.preview_document_with_coverage([3, 1], RgbSpace::Srgb, 49.).unwrap().0.pixels, master.pixels,
        "saved SDR appearance must not affect the HDR master preview");
    assert!(renderer.preview_document_with_coverage([3, 1], RgbSpace::Srgb, f32::NAN).is_err());
}

fn source_document(color: DocumentColor, extent: [u32; 2]) -> Document {
    let mut document = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
    hide_paper(&mut document);
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: color.depth,
            profile: ColorProfile::Builtin(color.space),
            profile_assumed: false,
        },
        16 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let mut row = Vec::new();
        for x in 0..extent[0] {
            let max = color.depth.maximum();
            let code = (x + y * extent[0]) % (max + 1);
            let values = [
                code,
                max - code,
                (code * 17) % (max + 1),
                match x % 17 {
                    0 => 0,
                    1 => 1,
                    2 => max / 3,
                    _ => max,
                },
            ];
            for value in values {
                match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                    SampleDepth::U8 => row.push(value as u8),
                    SampleDepth::U16 => row.extend((value as u16).to_le_bytes()),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    paint_mut(&mut document).original = Some(Arc::new(builder.finish().unwrap()));
    document
}
fn decode(bytes: Vec<u8>) -> layer_core::color::source::SourceImage {
    layer_color::photo::read_photo(Cursor::new(bytes), Default::default()).unwrap()
}
fn raw_rows(source: &layer_core::color::source::SourceImage) -> Vec<u8> {
    let mut reader = source.rows();
    let mut all = Vec::new();
    let mut row = vec![0; source.extent[0] as usize * source.interpretation.pixel_bytes()];
    for y in 0..source.extent[1] {
        reader.read(y, &mut row).unwrap();
        all.extend_from_slice(&row);
    }
    all
}
fn frame(document: &Document) -> (WgpuRasterizer, Vec<[f32; 4]>) {
    let mut r = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    let extent = document.composition().size;
    r.submit(FramePacket {
        reset_layers: true,
        ..packet(document.scene(), extent)
    })
    .unwrap();
    let bytes = crate::layer_tests::page_bytes(&r, crate::test_support::document_texture(&r));
    let pixels = bytes
        .chunks_exact(16)
        .map(|p| {
            std::array::from_fn(|c| f32::from_le_bytes(p[c * 4..c * 4 + 4].try_into().unwrap()))
        })
        .collect();
    (r, pixels)
}

#[test]
fn snapshot_identity_png_tiff_preserve_every_code_and_hidden_rgb() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let project = source_document(DocumentColor { space, depth }, [257, 256]);
            let source = project.scene().paint(paint_id(&project)).unwrap().original.as_ref().unwrap().clone();
            let expected = raw_rows(&source);
            let target = source.interpretation.clone();
            let mut reader =
                capture(project).unwrap();
            for tiff in [false, true] {
                let mut output = Cursor::new(Vec::new());
                let stats = if tiff {
                    reader.write_tiff(&mut output, &target, Default::default(), None)
                } else {
                    reader.write_png(&mut output, &target, Default::default(), None)
                }
                .unwrap();
                assert_eq!(stats.clipped_channels, 0);
                assert_eq!(reader.control().output_rows(), 256);
                let decoded = decode(output.into_inner());
                assert_eq!(decoded.interpretation.depth, depth);
                assert_eq!(
                    raw_rows(&decoded),
                    expected,
                    "{space:?} {depth:?} tiff={tiff}"
                );
                assert!(reader.renderer.scale_display.is_none());
                assert!(
                    reader
                        .renderer
                        .paint_layers
                        .iter()
                        .all(|l| l.pages.is_empty())
                );
            }
        }
    }
}

#[test]
fn snapshot_gray_identity_and_explicit_matte_keep_their_output_contracts() {
    let mut project = source_document(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        [257, 256],
    );
    let original = project.scene().paint(paint_id(&project)).unwrap().original.as_ref().unwrap().clone();
    let target = SourceInterpretation {
        channels: SourceChannels::GrayAlpha,
        ..original.interpretation.clone()
    };
    let mut builder = SourceBuilder::new(original.extent, target.clone(), 4 * 1024 * 1024).unwrap();
    let mut input = original.rows();
    let mut row = vec![0; 257 * 8];
    for y in 0..256 {
        input.read(y, &mut row).unwrap();
        let gray = row
            .chunks_exact(8)
            .flat_map(|p| [p[0], p[1], p[6], p[7]])
            .collect::<Vec<_>>();
        builder.push_row(&gray).unwrap();
    }
    let gray = Arc::new(builder.finish().unwrap());
    let expected = raw_rows(&gray);
    paint_mut(&mut project).original = Some(gray);
    let mut reader = capture(project).unwrap();
    for tiff in [false, true] {
        let mut file = Cursor::new(Vec::new());
        if tiff {
            reader.write_tiff(&mut file, &target, Default::default(), None)
        } else {
            reader.write_png(&mut file, &target, Default::default(), None)
        }
        .unwrap();
        let decoded = decode(file.into_inner());
        assert_eq!(raw_rows(&decoded), expected);
        assert_eq!(
            layer_color::profile_channels(&decoded.interpretation.profile).unwrap(),
            layer_color::ProfileChannels::Gray
        );
    }
    // An explicit matte defeats the raw-source shortcut even when profile,
    // channels and depth are unchanged. Transparent samples become opaque.
    let project = source_document(
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        },
        [33, 17],
    );
    let target = project.scene().paint(paint_id(&project)).unwrap()
        .original
        .as_ref()
        .unwrap()
        .interpretation
        .clone();
    let mut reader = capture(project).unwrap();
    let matte = [0.2, 0.4, 0.6];
    let mut output = Vec::new();
    reader
        .write_png(&mut output, &target, Default::default(), Some(matte))
        .unwrap();
    let pixels = raw_rows(&decode(output));
    assert!(pixels.chunks_exact(8).all(|p| &p[6..8] == [255, 255]));
    for c in 0..3 {
        let actual = u16::from_le_bytes(pixels[c * 2..c * 2 + 2].try_into().unwrap());
        let expected = (RgbSpace::DisplayP3.encode(matte[c] as f64) * 65535.).round() as u16;
        assert_eq!(actual, expected);
    }
}

fn rich_document(color: DocumentColor, mask_kind: u32) -> Document {
    let mut project = source_document(color, [641, 389]);
    let doc = &mut project;
    let coverage=doc.artwork.coverage.next_handle();
    let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,[641,389],Point{x:13.,y:-7.});
    mask.source.default_coverage = 0.;
    let mut selection = if mask_kind == 0 {
        Selection::polygon(vec![
            Point { x: 10., y: 8. },
            Point { x: 631., y: 45. },
            Point { x: 387., y: 382. },
        ])
        .unwrap()
    } else {
        let words = (0..389)
            .flat_map(|y| {
                (0..641u32.div_ceil(8)).map(move |word| {
                    (0..8).filter(|n| word * 8 + n < 641).fold(0u32, |v, n| {
                        v | (((word * 8 + n) / 17 + y / 11) % 5) << (n * 4)
                    })
                })
            })
            .collect::<Vec<_>>();
        Selection::pixels(Arc::new(
            SelectionPixels::new([641, 389], [0, 0, 641, 389], words).unwrap(),
        ))
        .transformed(if mask_kind == 1 {
            Affine([1., 0., 0., 1., 0.25, -0.3])
        } else {
            Affine([1.1, 0.16, -0.08, 0.94, -5., 8.])
        })
        .unwrap()
    };
    selection.inverted = mask_kind == 2;
    mask.source.initial = Some(selection);
    let scalar = match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
        SampleDepth::U8 => vec![123; 65536],
        SampleDepth::U16 => 32001u16.to_le_bytes().repeat(65536),
    };
    let mut mask_data = RasterData::default();
    mask_data.tiles.insert(
        TileKey {
            plane: RasterPlane::Mask,
            coordinate: [1, 0],
        },
        RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &scalar).unwrap()),
    );
    mask.source.raster = RasterRevision::backed(mask_data);
    let mut data = RasterData {
        watercolor: Some(RasterWatercolor {
            wet_edge: 0.6,
            burnt_edge: 0.3,
            edge_width: 7.,
        }),
        ..Default::default()
    };
    let paint = match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
        SampleDepth::U8 => [92u8, 41, 71, 123].repeat(65536),
        SampleDepth::U16 => [30001u16, 17003, 49117, 32768]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
            .repeat(65536),
    };
    data.tiles.insert(
        TileKey {
            plane: RasterPlane::Color,
            coordinate: [1, 0],
        },
        RasterTile::backed(TileBlob::encode(color.paint_descriptor(), &paint).unwrap()),
    );
    for coordinate in [[1, 0], [2, 0]] {
        data.tiles.insert(
            TileKey {
                plane: RasterPlane::WatercolorWetness,
                coordinate,
            },
            RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &scalar).unwrap()),
        );
    }
    paint_mut(doc).raster = RasterRevision::backed(data);
    assert_eq!(doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap(),coverage);
    let paint=paint_occurrence(doc);
    let occurrence=doc.artwork.occurrences.get_mut(paint).unwrap();
    occurrence.mask=Some(mask.use_);
    occurrence.translation=Point {x:-11.,y:9.};
    let mut program = (*crate::tests::fixture("exposure").program()).clone();
    program.entry = "blur".into();
    program.wgsl="fn blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p+vec2<f32>(3.,0.))+c+fx_sample(p-vec2<f32>(3.,0.)))/3.;}".into();
    program.passes = vec![layer_core::EffectPass {
        entry: "blur".into(),
        sampling: layer_core::EffectSampling::Neighborhood { radius: 3 },
    }]
    .into();
    let effect=insert_effect(doc,EffectInstance::new(Arc::new(program)),0);
    let group=add_group(doc,vec![effect,paint],0);
    let occurrence=doc.artwork.occurrences.get_mut(group).unwrap();
    occurrence.translation=Point{x:3.,y:5.};occurrence.opacity=0.73;
    project
}

#[test]
fn shared_capture_keeps_private_pixels_during_live_frames_and_after_canvas_close() {
    for color in [
        DocumentColor::default(),
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
    ] {
        let project = rich_document(color, 1);
        let (mut live, expected) = frame(&project);
        let cached_before = live.source_sample_cache_stats();
        let expected = Arc::new(expected);
        let [width, height] = project.composition().size;
        let mut capture = live
            .snapshot_gpu()
            .capture_scene(project.clone().snapshot(),SceneScope::All,Default::default())
            .unwrap();
        assert!(
            Arc::ptr_eq(
                &live.device.source_samples,
                &capture.renderer.device.source_samples
            ),
            "file workers must share the live canvas's sample budget"
        );
        let check = |actual: &[[f32; 4]], expected: &[[f32; 4]]| {
            assert_eq!(actual.len(), expected.len());
            for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
                assert!((a - b).abs() <= 2e-6, "shared capture changed: {a} != {b}");
            }
        };
        let saved = expected.clone();
        let worker = std::thread::spawn(move || {
            for _ in 0..4 {
                let pixels = capture.read_region([0, 0, width, height]).unwrap();
                check(&pixels, &saved);
            }
            capture
        });
        let mut changed = project.clone();
        let group=changed.scene().children(None)[0];
        for i in 0..16 {
            changed.artwork.occurrences.get_mut(group).unwrap().opacity=if i%2==0 {0.2}else{0.9};
            live.submit(FramePacket {
                ..packet(changed.scene(),[width,height])
            })
            .unwrap();
            live.wait_idle().unwrap();
        }
        let mut capture = worker.join().unwrap();
        let cached_after = live.source_sample_cache_stats();
        assert!(
            cached_after.hits > cached_before.hits,
            "private GPU captures reuse exact source samples"
        );
        assert!(cached_after.peak_bytes <= cached_after.limit_bytes);
        drop(live);
        // Closing a canvas releases its resources, not the device still owned
        // by an immutable file worker. The snapshot remains exactly its own.
        check(
            &capture.read_region([0, 0, width, height]).unwrap(),
            &expected,
        );
        capture.control().cancel();
        assert!(capture.read_region([0, 0, 1, 1]).is_err());
    }
}

#[test]
fn snapshot_bands_preserve_masked_pixels_and_shrink_before_exceeding_budget() {
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let project = rich_document(color, 1);
    let control = CaptureControl::with_allocation_tracking();
    let mut reader =
        gpu().capture_scene(project.snapshot(),SceneScope::All,control.clone())
            .unwrap();
    let [width, height] = reader.extent();
    let mut reference = Vec::new();
    for y in (0..height).step_by(16) {
        reference.extend(
            reader
                .read_region([0, y, width, 16.min(height - y)])
                .unwrap(),
        );
    }
    let mut actual = Vec::new();
    let mut y = 0;
    let mut bands = 0;
    while y < height {
        let (rows, pixels) = reader.read_band(y).unwrap();
        assert!(pixels.len() * 16 <= 32 * 1024 * 1024);
        actual.extend(pixels);
        y += rows;
        bands += 1;
    }
    assert_eq!(bands, 2);
    assert_eq!(
        actual, reference,
        "band boundaries must not change exact capture pixels"
    );
    let mut expected = layer_core::color::histogram::Histogram::new(color);
    expected.add(&reference).unwrap();
    assert_eq!(reader.histogram().unwrap(), expected);
    let peaks = control.allocation_peaks().unwrap();
    let allocation_reports = reader.renderer.device.generate_allocator_report().is_some();
    assert_eq!(peaks.observations > 0, allocation_reports);
    assert!(peaks.reserved_bytes >= peaks.allocated_bytes);
    if !allocation_reports {
        // Metal does not expose wgpu allocator reports. Absence must not be
        // represented as an observed zero-byte allocation.
        assert_eq!((peaks.allocated_bytes, peaks.reserved_bytes), (0, 0));
    }

    // Budget rejections happen during dependency planning. Let exactly the
    // original 16-row request fit and require the wider band to shrink to it.
    reader.planned_pixel_bytes = 0;
    let required = |error| match error {
        GpuRasterError::CaptureBudget { required, .. } => required,
        other => panic!("unexpected capture error: {other}"),
    };
    let small = required(reader.read_region([0, 0, width, 16]).unwrap_err());
    let large = required(reader.read_region([0, 0, width, 256]).unwrap_err());
    assert!(large > small);
    reader.planned_pixel_bytes = small + u64::from(width) * 16 * 16;
    let observations = control.allocation_peaks().unwrap().observations;
    let (rows, pixels) = reader.read_band(0).unwrap();
    assert_eq!(rows, 16);
    assert_eq!(pixels, reference[..width as usize * 16]);
    assert_eq!(
        control.allocation_peaks().unwrap().observations,
        observations + u64::from(allocation_reports) * u64::from(width.div_ceil(512))
    );
    reader.control().cancel();
    assert!(reader.read_band(0).is_err());
}

#[test]
fn shared_snapshot_chunks_preserve_masked_effect_pixels_across_column_boundaries() {
    let mut project = rich_document(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 }, 1);
    project.artwork.compositions.get_mut(project.artwork.root).unwrap().size[0]=2053;
    let paint=paint_occurrence(&project);
    project.artwork.occurrences.get_mut(paint).unwrap().placement.outer.0[2]+=800.;
    let (live, expected) = frame(&project);
    let mut capture = live.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
    let mut actual = Vec::new();
    let mut y = 0;
    while y < capture.extent()[1] {
        let (rows, pixels) = capture.read_band(y).unwrap();
        actual.extend(pixels); y += rows;
    }
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!((a-b).abs() <= 2e-6, "shared snapshot column seam: {a} != {b}");
    }
}

#[test]
fn gpu_tone_snapshot_matches_composited_masked_filtered_document() {
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let mut project = rich_document(color,1);
    project.artwork.compositions.get_mut(project.artwork.root).unwrap().size[0]=2053;
    let paint=paint_occurrence(&project);
    project.artwork.occurrences.get_mut(paint).unwrap().placement.outer.0[2]+=800.;
    let extent = project.composition().size;
    let (live,pixels) = frame(&project);
    let mut cpu = layer_core::color::hdr::LocalToneBuilder::new(extent,color.space).unwrap();
    for row in pixels.chunks_exact(extent[0] as usize) { cpu.push(row).unwrap(); }
    let expected = cpu.finish(||false).unwrap();
    let mut capture = live.snapshot_gpu().capture_scene(project.snapshot(),SceneScope::All,Default::default()).unwrap();
    let gpu = capture.gpu_local_tone_guide().unwrap();
    assert!(Arc::ptr_eq(&gpu,&capture.gpu_local_tone_guide().unwrap()));
    let actual = capture.local_tone_guide().unwrap();
    assert_eq!(actual.extent,expected.extent);
    for (a,b) in actual.samples.iter().zip(&expected.samples) {
        for c in 0..2 { assert!((a[c]-b[c]).abs() < 0.0003,"masked/filter guide: {a:?} != {b:?}"); }
        let coverage = |p: &[f32; 4]| f64::from(p[2]) * 2f64.powi(p[3] as i32);
        assert!((coverage(a)-coverage(b)).abs() < 0.0003,"masked/filter coverage: {a:?} != {b:?}");
    }
    capture.control().cancel();
    assert!(capture.gpu_local_tone_guide().is_err(),"cancellation also rejects cached output");
    assert!(capture.local_tone_guide().is_err(),"CPU delivery observes the same cancellation");
    capture.control = Default::default();
    capture.gpu_local_tone = None;
    capture.planned_pixel_bytes = 1;
    assert!(capture.gpu_local_tone_guide().unwrap_err().contains("limit is 1"));
}

#[test]
fn snapshot_crops_restore_masked_native_material_and_selection_windows() {
    for color in [
        DocumentColor::default(),
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        },
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
    ] {
        for mask in 0..3 {
            let project = rich_document(color, mask);
            let (_full_renderer, full) = frame(&project);
            let mut project = if mask == 2 {
                roundtrip(&project)
            } else {
                project
            };
            // Viewing the mask area must not change snapshot artwork.
            project.working.inspect_mask=Some(paint_occurrence(&project));
            let mut reader =
                capture(project).unwrap();
            assert!(reader.renderer.scale_display.is_none());
            for rect in [
                [257, 19, 31, 33],
                [0, 0, 97, 79],
                [577, 325, 64, 64],
                [241, 241, 53, 49],
                [17, 300, 65, 33],
                [257, 19, 31, 33],
            ] {
                let pixels = reader.read_region(rect).unwrap();
                for y in 0..rect[3] {
                    for x in 0..rect[2] {
                        for c in 0..4 {
                            let actual = pixels[(y * rect[2] + x) as usize][c];
                            let expected = full[((y + rect[1]) * 641 + x + rect[0]) as usize][c];
                            assert!(
                                (actual - expected).abs() <= 2e-6,
                                "{color:?} mask={mask} rect={rect:?} ({x},{y}) c={c}: {actual} vs {expected}"
                            );
                        }
                    }
                }
                assert!(reader.renderer.scale_display.is_none());
                assert_eq!(reader.renderer.metrics().composite_storage_bytes, 0);
                assert!(reader.renderer.selection_clip.storage_bytes() < 641 * 389 / 2);
            }
        }
    }
}

#[test]
fn snapshot_profiled_composite_rows_match_full_render_and_honor_budget_and_cancel() {
    let project = rich_document(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        2,
    );
    let (_r, full) = frame(&project);
    let before = project.artwork.paint.iter().map(|(_,_,source)|source.raster.identity())
        .collect::<Vec<_>>();
    let mut reader =
        capture(project.clone()).unwrap();
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        for space in [RgbSpace::Srgb, RgbSpace::DisplayP3] {
            for tiff in [false, true] {
                let target = SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth,
                    profile: ColorProfile::Builtin(space),
                    profile_assumed: false,
                };
                let encoder = layer_color::WorkingEncoder::new(
                    reader.color().space,
                    &target,
                    Default::default(),
                )
                .unwrap();
                let mut expected = vec![0; full.len() * target.pixel_bytes()];
                encoder
                    .encode_premultiplied(&full, &mut expected, None, [0, 0])
                    .unwrap();
                let mut output = Cursor::new(Vec::new());
                if tiff {
                    reader.write_tiff(&mut output, &target, Default::default(), None)
                } else {
                    reader.write_png(&mut output, &target, Default::default(), None)
                }
                .unwrap();
                let result = decode(output.into_inner());
                assert_eq!(result.interpretation.depth, depth);
                let actual = raw_rows(&result);
                for (a, b) in actual
                    .chunks_exact(depth.bytes())
                    .zip(expected.chunks_exact(depth.bytes()))
                {
                    let sample = |v: &[u8]| {
                        if depth == SampleDepth::U8 {
                            v[0] as u16
                        } else {
                            u16::from_le_bytes(v.try_into().unwrap())
                        }
                    };
                    assert!(
                        sample(a).abs_diff(sample(b)) <= 1,
                        "profiled row mismatch {space:?} {depth:?} tiff={tiff}"
                    );
                }
            }
        }
    }
    assert_eq!(
        before,
        project.artwork.paint.iter().map(|(_,_,source)|source.raster.identity())
            .collect::<Vec<_>>()
    );
    reader.planned_pixel_bytes = 1;
    assert!(reader.read_region([0, 0, 17, 17]).is_err());
    assert!(reader.renderer.scale_display.is_none());
    reader.control().cancel();
    let mut out = Vec::new();
    assert!(
        reader
            .write_png(
                &mut out,
                &SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth: SampleDepth::U16,
                    profile: Default::default(),
                    profile_assumed: false
                },
                Default::default(),
                None
            )
            .is_err()
    );
    assert!(out.is_empty());
}

#[test]
fn cancelled_snapshot_does_not_initialize_a_device_or_resolve_backing() {
    let control = CaptureControl::default();
    control.cancel();
    let result = gpu().capture_scene(source_document(DocumentColor::default(),[8,8]).snapshot(),SceneScope::All,control.clone());
    assert!(matches!(result, Err(GpuRasterError::Color(e)) if e.contains("cancelled")));
    assert_eq!(control.output_rows(), 0);
}

#[test]
fn snapshot_jpeg_applies_profile_and_linear_matte_before_lossy_encoding() {
    let project = source_document(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        [33, 17],
    );
    let mut reader = capture(project).unwrap();
    for space in RgbSpace::ALL {
        let target = SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(space),
            profile_assumed: false,
        };
        let matte = [0.25, 0.5, 0.75];
        let mut png = Vec::new();
        let expected_stats = reader
            .write_png(&mut png, &target, Default::default(), Some(matte))
            .unwrap();
        let mut jpeg = Vec::new();
        let actual_stats = reader
            .write_jpeg(&mut jpeg, &target, Default::default(), matte, 100)
            .unwrap();
        assert_eq!(expected_stats, actual_stats);
        let expected = decode(png);
        let actual = decode(jpeg);
        assert_eq!(actual.interpretation.channels, SourceChannels::Rgb);
        assert_eq!(actual.interpretation.depth, SampleDepth::U8);
        assert_eq!(
            layer_color::profile_bytes(&actual.interpretation.profile).unwrap(),
            layer_color::profile_bytes(&target.profile).unwrap()
        );
        let a = raw_rows(&expected);
        let b = raw_rows(&actual);
        let max = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(
            max <= 4,
            "{space:?} JPEG quality 100 differs by {max} codes"
        );
    }
}

#[test]
fn snapshot_dither_is_repeatable_across_formats_and_keeps_master_and_identity_samples() {
    use layer_core::color::{OutputDither, OutputEncoding};
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let project = source_document(color, [513, 35]);
    let original = project.clone();
    let mut reader =
        capture(project.clone()).unwrap();
    let target = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(color.space),
        profile_assumed: false,
    };
    let options = OutputEncoding {
        dither: OutputDither::Stochastic8,
        ..Default::default()
    };
    let mut png = Vec::new();
    let mut repeated = Vec::new();
    let mut tiff = Cursor::new(Vec::new());
    let mut normal = Vec::new();
    let stats = reader.write_png(&mut png, &target, options, None).unwrap();
    assert_eq!(
        stats,
        reader
            .write_tiff(&mut tiff, &target, options, None)
            .unwrap()
    );
    assert_eq!(
        stats,
        reader
            .write_png(&mut repeated, &target, options, None)
            .unwrap()
    );
    assert_eq!(png, repeated);
    reader
        .write_png(&mut normal, &target, Default::default(), None)
        .unwrap();
    let dithered = raw_rows(&decode(png));
    let rounded = raw_rows(&decode(normal));
    assert_eq!(dithered, raw_rows(&decode(tiff.into_inner())));
    assert_ne!(dithered, rounded);
    for (a, b) in dithered.chunks_exact(4).zip(rounded.chunks_exact(4)) {
        assert_eq!(a[3], b[3]);
        assert!(a[..3].iter().zip(&b[..3]).all(|(a, b)| a.abs_diff(*b) <= 1));
    }
    assert_eq!(project, original);
    // Dithering never bypasses exact same-depth/source delivery to make noise.
    let project = source_document(
        DocumentColor {
            depth: SampleDepth::U8,
            ..color
        },
        [513, 35],
    );
    let source = project.scene().paint(paint_id(&project)).unwrap().original.as_ref().unwrap().clone();
    let mut reader = capture(project).unwrap();
    let mut bytes = Vec::new();
    reader
        .write_png(&mut bytes, &source.interpretation, options, None)
        .unwrap();
    assert_eq!(raw_rows(&decode(bytes)), raw_rows(&source));
}

mod resized;

#[test]
fn flattened_copy_preserves_complete_composition_precision_extent_and_resolution() {
    let mut original = rich_document(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        2,
    );
    original.artwork.compositions.get_mut(original.artwork.root).unwrap().resolution = Some(layer_core::ImageResolution::ppi(300));
    let (_, full) = frame(&original);
    let mut reader =
        capture(original.clone()).unwrap();
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let result = reader
        .flattened_document(color, Default::default(), 64 * 1024 * 1024)
        .unwrap();
    let copy = result.document;
    assert_eq!(copy.composition().color, color);
    assert_eq!(
        copy.composition().size,
        original.composition().size
    );
    assert_eq!(copy.composition().resolution, original.composition().resolution);
    assert_eq!(copy.scene().order().len(), 1);
    let source = copy.scene().paint(paint_id(&copy)).unwrap().original.as_ref().unwrap();
    assert_eq!(
        source.kind,
        layer_core::color::source::SourceKind::Rasterized
    );
    assert_eq!(source.resolution, original.composition().resolution);
    let encoder = layer_color::WorkingEncoder::new(
        original.composition().color.space,
        &source.interpretation,
        Default::default(),
    )
    .unwrap();
    let mut expected = vec![0; full.len() * 8];
    encoder
        .encode_premultiplied(&full, &mut expected, None, [0, 0])
        .unwrap();
    let actual = raw_rows(source);
    for (a, b) in actual.chunks_exact(2).zip(expected.chunks_exact(2)) {
        assert!(
            u16::from_le_bytes(a.try_into().unwrap())
                .abs_diff(u16::from_le_bytes(b.try_into().unwrap()))
                <= 1
        );
    }
    let reopened = roundtrip(&copy);
    assert_eq!(
        raw_rows(reopened.scene().paint(paint_id(&reopened)).unwrap().original.as_ref().unwrap()),
        actual
    );
    assert_eq!(reopened.composition().resolution, original.composition().resolution);
    reader.control().cancel();
    assert!(
        reader
            .flattened_document(color, Default::default(), 64 * 1024 * 1024)
            .is_err()
    );
}

#[test]
fn applying_projective_pixels_from_linked_coverage_keeps_paired_paint_and_source_handles() {
    let extent = [33, 17];
    let mut document = Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames {
        paint: "Paired paint".into(), paper: "Paper".into(),
    });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F32;
    let paint = paint_id(&document);
    let owner = paint_occurrence(&document);
    let color = document.composition().color;
    let rgba = [0.375_f32, 0.125, 0.0625, 1.];
    let mut data = RasterData::default();
    data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [0; 2] },
        RasterTile::backed(TileBlob::encode(color.paint_descriptor(), &rgba.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>().repeat(65536)).unwrap()));
    document.artwork.paint.get_mut(paint).unwrap().raster = RasterRevision::backed(data);
    let coverage = document.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, extent, Point::default());
    mask.source.default_coverage = 0.;
    let mut data = RasterData::default();
    data.tiles.insert(TileKey { plane: RasterPlane::Mask, coordinate: [0; 2] },
        RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &16384_u16.to_le_bytes().repeat(65536)).unwrap()));
    mask.source.raster = RasterRevision::backed(data);
    assert_eq!(document.artwork.coverage.insert(PortableId::random(), mask.source).unwrap(), coverage);
    let occurrence = document.artwork.occurrences.get_mut(owner).unwrap();
    occurrence.mask = Some(mask.use_);
    occurrence.placement = layer_core::LayerPlacement::from_projective(layer_core::Projective::rect_to_quad(
        layer_core::Rect::from_extent(extent), [[4., 3.], [30., 5.], [29., 15.], [2., 14.]].map(|[x, y]| Point { x, y }),
    ).unwrap());
    refresh(&mut document);
    let target = SourceTarget::Coverage(coverage);
    let plan = document.transform_pixels_plan(target, layer_core::Interpolation::Nearest, Default::default()).unwrap();
    assert_eq!(plan.target, target);
    assert_eq!(plan.paint, Some(paint));
    assert_eq!(plan.coverage, Some(coverage));
    let output = pollster::block_on(gpu().transform_pixels(plan, Default::default())).unwrap();
    document.apply(output).unwrap();
    let bytes = |target, plane| {
        let data = document.target_raster(target).unwrap().wait_data().unwrap();
        data.tiles[&TileKey { plane, coordinate: [0; 2] }].wait_backing().unwrap().decode().unwrap()
    };
    let offset = (8 * 256 + 10) as usize;
    let paint_bytes = bytes(SourceTarget::Paint(paint), RasterPlane::Color);
    let pixel: [f32; 4] = std::array::from_fn(|channel| {
        let start = offset * 16 + channel * 4;
        f32::from_le_bytes(paint_bytes[start..start + 4].try_into().unwrap())
    });
    assert_eq!(pixel, rgba);
    let mask_bytes = bytes(target, RasterPlane::Mask);
    assert_eq!(u16::from_le_bytes(mask_bytes[offset * 2..offset * 2 + 2].try_into().unwrap()), 16384);
    assert_eq!(document.scene().source_target(owner), Some(SourceTarget::Paint(paint)));
    assert_eq!(document.scene().mask(owner).unwrap().0.source, coverage);
}
