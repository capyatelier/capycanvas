use super::*;
use crate::test_support::{packet, submit_settled};
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, CoverageSnapshot, Point, authored::{PortableId,SourceTarget}};
use layer_render::{
    ColorSampleArea, ColorSampleRequest, ColorSampleSource, RegionRequest, RegionSource,
};

const EXTENT: [u32; 2] = [513, 273];
fn codes(x: u32) -> [u16; 4] {
    [10000 + (x / 256) as u16 * 16000, 32123, 51007, 40000]
}
fn document(color: DocumentColor) -> Document {
    let mut doc = Document::new(PortableId::random(), EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let root = doc.artwork.root;
    doc.artwork.compositions.get_mut(root).unwrap().color = color;
    let paper = doc.scene().order()[1];
    doc.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    let mut data = RasterData::default();
    for y in 0..2 {
        for x in 0..3 {
            let values = codes(x * PAGE_SIZE);
            let bytes = match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                SampleDepth::U16 => values
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
                SampleDepth::U8 => values.map(|v| (v >> 8) as u8).to_vec(),
            };
            data.tiles.insert(
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [x, y],
                },
                RasterTile::backed(
                    TileBlob::encode(color.paint_descriptor(), &bytes.repeat(256 * 256)).unwrap(),
                ),
            );
        }
    }
    let SourceTarget::Paint(target) = doc.scene().source_target(doc.scene().order()[0]).unwrap() else { unreachable!() };
    doc.artwork.paint.get_mut(target).unwrap().raster = RasterRevision::backed(data);
    doc
}
fn frame<'a>(doc: &'a Document) -> FramePacket<'a> {
    FramePacket { view: crate::test_support::view([640, 480]), inspect_mask: doc.working.inspect_mask, ..packet(doc.scene(), EXTENT) }
}
fn sample(r: &mut WgpuRasterizer, position: [u32; 2], area: ColorSampleArea) -> [f32; 4] {
    let idle = r.raster_tiers.paint.used() - r.color_sampler.storage_bytes();
    let rgba = crate::test_support::color_sample(r, ColorSampleRequest { request_id: 1, source: ColorSampleSource::Composite, position, area }).rgba;
    assert_eq!(r.raster_tiers.paint.used() - idle, r.color_sampler.storage_bytes(), "the sample buffer and capture target are charged to the paint tier");
    rgba
}
fn close(a: [f32; 4], b: [f32; 4]) {
    for (a, b) in a.into_iter().zip(b) {
        assert!((a - b).abs() <= 3e-6, "{a} != {b}");
    }
}

#[test]
fn composite_queries_ignore_inspection_and_need_no_display_texture() {
    for color in [
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: SampleDepth::U16,
        },
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U8,
        },
    ] {
        let mut doc = document(color);
        let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), EXTENT, [0, 0]);
        mask.source.default_coverage = 0.5;
        let owner = doc.scene().order()[0];
        crate::tests::image_windows::set_mask(&mut doc, owner, mask);
        doc.working.inspect_mask = Some(owner);
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.7;
        let mut r = crate::memory::memory_tests::renderer(color, layer_core::DeviceMemoryFacts {
            budget_bytes: Some(4 << 30), unified: true, ..Default::default()
        }).unwrap();
        crate::memory::memory_tests::cold_color_pressure(&mut r);
        submit_settled(&mut r, frame(&doc));
        let visible = r.scale_display.as_ref().unwrap().texture().clone();
        let before = crate::layer_tests::page_bytes(&r, &visible);
        r.scale_display = None;
        for position in [[20u32, 20], [255, 255], [512, 272]] {
            for area in [ColorSampleArea::Point, ColorSampleArea::Average5] {
                let radius = area.width() / 2;
                let mut sum = [0.; 4];
                let mut count = 0.;
                for _y in
                    position[1].saturating_sub(radius)..(position[1] + radius + 1).min(EXTENT[1])
                {
                    for x in position[0].saturating_sub(radius)
                        ..(position[0] + radius + 1).min(EXTENT[0])
                    {
                        let values = codes(x).map(|v| match color.depth {
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                            SampleDepth::U16 => f64::from(v) / 65535.,
                            SampleDepth::U8 => f64::from(v >> 8) / 255.,
                        });
                        let alpha = values[3] as f32 * 0.5 * 0.7;
                        for c in 0..3 {
                            sum[c] += color.space.decode(values[c]) as f32 * alpha;
                        }
                        sum[3] += alpha;
                        count += 1.;
                    }
                }
                close(
                    sample(&mut r, position, area),
                    [
                        sum[0] / sum[3],
                        sum[1] / sum[3],
                        sum[2] / sum[3],
                        sum[3] / count,
                    ],
                );
            }
        }
        let selected = crate::test_support::receive_request(&mut r, RegionRequest { fill_areas: None, colour_mode: Default::default(), fill: None, reference_count: None, paper: None,
            enclosure: None, contiguous: true,
            selection: None,
            request_id: 2,
            source: RegionSource::Composite,
            position: [20, 20],
            tolerance: 0.,
            refinement: layer_render::RegionRefinement { close_gaps: false, edges: layer_core::RegionEdges::Exact, smooth_edges: false },
            limit: None
        }).pixels;
        assert_eq!(selected.bounds(), [0, 0, 256, 273]);
        assert_eq!(
            crate::layer_tests::page_bytes(&r, &visible),
            before,
            "queries preserve displayed pixels"
        );
        assert!(
            r.paint_layers[0].pages.is_empty(),
            "queries do not materialize cold paint"
        );
        assert!(r.scale_display.is_none());
    }
}

#[test]
fn filtered_query_crops_match_full_resolution_and_reject_excessive_dependencies() {
    let mut doc = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    for _ in 0..2 { crate::tests::image_windows::insert_effect(&mut doc, crate::tests::image_windows::program(false, false)); }
    let mut r = WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    submit_settled(&mut r, frame(&doc));
    let visible = crate::test_support::document_texture(&r).clone();
    let full = crate::layer_tests::page_bytes(&r, &visible);
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    r.encode_clear_value(
        &mut encoder,
        r.scale_display.as_ref().unwrap().view(),
        "poison display only",
        1.,
    );
    encoder.submit(&r.queue);
    for position in [[0u32, 0], [253, 255], [256, 257], [512, 272]] {
        let mut sum = [0.; 4];
        let mut count = 0.;
        for y in position[1].saturating_sub(2)..(position[1] + 3).min(EXTENT[1]) {
            for x in position[0].saturating_sub(2)..(position[0] + 3).min(EXTENT[0]) {
                let i = (y * EXTENT[0] + x) as usize * 16;
                for c in 0..4 {
                    sum[c] +=
                        f32::from_le_bytes(full[i + c * 4..i + c * 4 + 4].try_into().unwrap());
                }
                count += 1.;
            }
        }
        close(
            sample(&mut r, position, ColorSampleArea::Average5),
            [
                sum[0] / sum[3],
                sum[1] / sum[3],
                sum[2] / sum[3],
                sum[3] / count,
            ],
        );
    }
    let mut capture = Capture {
        image_limit: Some(1),
        ..Default::default()
    };
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    assert!(
        capture
            .region(
                &mut r,
                frame(&doc),
                PixelRect::new(255, 255, 260, 260),
                [5; 2],
                &mut encoder
            )
            .is_err()
    );
    assert!(
        capture.target.is_none(),
        "preflight precedes query image allocation"
    );
    let after = crate::layer_tests::page_bytes(&r, &visible);
    assert!(
        after
            .as_chunks::<4>().0.iter()
            .all(|v| f32::from_le_bytes(*v) == 1.)
    );
}

#[test]
fn cold_object_thumbnails_and_retouch_reads_retry_until_exact_pixels_are_ready() {
    use layer_core::authored::{Affine64,Image,ImageObject};
    use layer_render::{RetouchPreparation,ThumbnailTarget};
    let extent=[256;2];
    let mut doc=Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let ink=doc.scene().order()[0];let paper=doc.scene().order()[1];
    doc.artwork.occurrences.get_mut(paper).unwrap().visible=false;
    let mut object=ImageObject::new(Image::new(layer_core::color::source::rgba8_source([512;2],|_,_|[255,0,0,255])));
    object.affine=Affine64([1./7.,0.,0.,1./7.,0.,0.]);
    let (owner,edit)=doc.create_object_layer_edit("Reference",object,None,1).unwrap();doc.apply(edit).unwrap();
    for thumbnail in [false,true] {
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=extent;
        let packet=crate::test_support::packet(doc.scene(),extent);
        let frame=Arc::new(Frame::new(packet,r.frame_context(packet)));r.artwork_frame=Some(frame.clone());
        if !thumbnail {
            r.prepare_retouch_sources(Some(&RetouchPreparation {execution:layer_core::BrushExecution::Clone,target:doc.scene().source_target(ink).unwrap(),
                retouch:layer_core::Retouch {source:layer_core::RetouchSource::References,references:Arc::new([owner].into()),..Default::default()},
                points:vec![Point {x:32.,y:32.}]}));
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.prefetch_retouch(&frame,false,&mut encoder).unwrap();
            r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);
        }
        let before=r.device.source_samples.stats();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let ready=if thumbnail {r.prepare_thumbnail_batch(ThumbnailTarget::Occurrence(owner)).unwrap()}
            else {r.prefetch_retouch(&frame,false,&mut encoder).unwrap();!r.retouch.as_ref().unwrap().pending()};
        assert!(!ready,"a cold exact read remains pending");
        assert_eq!(r.device.source_samples.stats(),before,"the initiating UI query cannot decompress source pixels");
        r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);r.wait_idle().unwrap();
        let mut ready=false;
        for _ in 0..500 {
            if thumbnail {ready=r.prepare_thumbnail_batch(ThumbnailTarget::Occurrence(owner)).unwrap();}
            else {
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                r.prefetch_retouch(&frame,false,&mut encoder).unwrap();
                ready = !r.retouch.as_ref().unwrap().pending();
                r.uploads.finish(&encoder);r.last_submission=encoder.submit(&r.queue);
            }
            r.wait_idle().unwrap();if ready {break;}std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(ready,"prepared object pixels must eventually publish");
        if thumbnail {
            r.request_thumbnail(91,ThumbnailTarget::Occurrence(owner)).unwrap();crate::test_support::complete(&r);
            let thumbnail=r.take_thumbnail().unwrap().unwrap();assert_eq!(thumbnail.request_id,91);
            let offset=(16*thumbnail.stride+16*4) as usize;
            assert_eq!(&thumbnail.bytes[offset..offset+4],&[255,0,0,255]);
        } else {
            let (target,view)=create_color_target(&r.device,extent,"prepared retouch read regression");
            assert!(r.draw_retouch_source(&view,[0;2],extent,[1.;2],[0.;2]).unwrap());r.wait_idle().unwrap();
            let pixels=crate::test_support::floats(&crate::layer_tests::page_bytes(&r,&target));
            close(pixels[32*256+32],[1.,0.,0.,1.]);
            assert_eq!(r.retouch.as_ref().unwrap().cached_pages(),1);
        }
        drop(r);
    }
    crate::startup::finish_shader_compiler_shutdown();
}

fn exact_preview_after_mask_upload(execution: BrushExecution) {
    use crate::layer_tests::{dab, placement, preset_style};
    let extent = [256; 2];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut doc = placement::paint_document(extent, "exact preview after mask upload");
    placement::set_source(&mut doc, layer_core::color::source::rgba8_source(extent,
        |x, y| [(x % 256) as u8, (y % 256) as u8, 80, 255]));
    crate::test_support::submit_ready(&mut r, FramePacket { reset_layers: true, ..packet(doc.scene(), extent) }).unwrap();
    let view = layer_render::ViewState { width_px: 32, height_px: 32,
        document_to_surface: [0.125, 0., 0., 0.125, 0., 0.] };
    crate::test_support::submit_ready(&mut r, FramePacket { view, ..packet(doc.scene(), extent) }).unwrap();
    let style = if execution == BrushExecution::Liquify {
        let mut style = preset_style(layer_core::DefaultBrushPreset::LiquifyPush);
        style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(placement::paint(&doc), false)));
        style
    } else { crate::tests::test_style(execution) };
    placement::paint_mut(&mut doc).raster = RasterRevision::pending();
    let mut real = dab([0.8, 0.2, 0.1, 1.]);
    real.center = Point { x: 128., y: 128. };
    real.radii = [60.; 2];
    real.material[3] = 1.;
    real.motion = [20., 0.];
    let mut predicted = real;
    predicted.center.x += 8.;
    let mut persistent = crate::test_support::dab_batch(placement::target(&doc), style, real.bounds());
    persistent.stroke_id = StrokeId(320);
    persistent.stroke_end = false;
    let mut preview = persistent.clone();
    preview.kind = DabBatchKind::Preview;
    preview.first_dab = 1;
    preview.damage = predicted.bounds();
    crate::test_support::submit_ready(&mut r, FramePacket { view, dabs: &[real, predicted], dab_batches: &[persistent, preview],
        ..packet(doc.scene(), extent) }).unwrap();
    assert_eq!(r.preview_level, 2);
    assert_eq!(r.artwork_frame.as_ref().unwrap().previews.len(), 1);
    assert!(!r.texture_sets.is_empty());
    r.prepare_asset(&AssetId(Arc::from("exact-preview-new-mask")), HostImage {
        width: 1, height: 1, stride: 1, format: PixelFormat::R8Unorm, bytes: &[255],
    }).unwrap();
    assert!(r.texture_sets.is_empty());
    let (destination, _) = create_color_target(&r.device, extent, "exact preview capture");
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    let quality=(r.preview_level,r.preview_contribution,r.preview_requires_base);
    let result=Capture::default().region_into(&mut r, packet(doc.scene(), extent), PixelRect::full(extent),
        &destination, &mut encoder);
    if result.is_err() {
        assert_eq!((r.preview_level,r.preview_contribution,r.preview_requires_base),quality,
            "a refused exact preview must preserve its published quality");
    }
    result.unwrap();
    r.uploads.finish(&encoder);
    encoder.submit(&r.queue);
    r.wait_idle().unwrap();
    assert_eq!(r.preview_level, 0);
    assert!(!r.texture_sets.is_empty());
    assert!(crate::layer_tests::page_bytes(&r, &destination).iter().any(|&byte| byte != 0));
}

#[test]
fn liquify_exact_preview_reprepares_resources_after_mask_upload() {
    exact_preview_after_mask_upload(BrushExecution::Liquify);
}

#[test]
fn dry_exact_preview_reprepares_resources_after_mask_upload() {
    exact_preview_after_mask_upload(BrushExecution::Dry);
}
