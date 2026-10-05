use super::*;
use crate::test_support::{complete, packet};
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
    assert!(
        r.request_color_sample(ColorSampleRequest {
            request_id: 1,
            source: ColorSampleSource::Composite,
            position,
            area
        })
        .unwrap()
    );
    complete(r);
    r.take_color_sample().unwrap().unwrap().rgba
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
        let mut mask = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), EXTENT, Point::default());
        mask.source.default_coverage = 0.5;
        let owner = doc.scene().order()[0];
        crate::tests::image_windows::set_mask(&mut doc, owner, mask);
        doc.working.inspect_mask = Some(owner);
        doc.artwork.occurrences.get_mut(owner).unwrap().opacity = 0.7;
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.native_edit.as_mut().unwrap().color_cache_bytes = 0;
        r.submit(frame(&doc)).unwrap();
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
        assert!(
            r.request_region(RegionRequest {
                enclosure: None, contiguous: true,
                selection: None,
                request_id: 2,
                source: RegionSource::Composite,
                position: [20, 20],
                tolerance: 0.,
                refinement: Default::default(),
                limit: None
            })
            .unwrap()
        );
        complete(&r);
        let selected = r.take_region().unwrap().unwrap().pixels;
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
    r.submit(frame(&doc)).unwrap();
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
            .chunks_exact(4)
            .all(|v| f32::from_le_bytes(v.try_into().unwrap()) == 1.)
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
    let (owner,edit)=doc.create_object_layer_edit("Reference",None,1).unwrap();doc.apply(edit).unwrap();
    let mut object=ImageObject::new(Image::new(layer_core::color::source::rgba8_source([512;2],|_,_|[255,0,0,255])),"Photo");
    object.affine=Affine64([1./7.,0.,0.,1./7.,0.,0.]);
    let (_,edit)=doc.add_image_object_edit(owner,object,0).unwrap();doc.apply(edit).unwrap();
    for thumbnail in [false,true] {
        let mut r=WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();r.document_extent=extent;
        let packet=crate::test_support::packet(doc.scene(),extent);
        let frame=Arc::new(Frame::new(packet,r.frame_context(packet)));r.artwork_frame=Some(frame.clone());
        if !thumbnail {
            r.prepare_retouch_sources(Some(&RetouchPreparation {target:doc.scene().source_target(ink).unwrap(),
                retouch:layer_core::Retouch {source:layer_core::RetouchSource::References,references:Arc::new([owner].into()),..Default::default()},
                points:vec![Point {x:32.,y:32.}]}));
            let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
            r.prefetch_retouch(&frame,false,&mut encoder).unwrap();
            r.uploads.finish(&encoder);r.last_submission=Some(encoder.submit(&r.queue));
        }
        let before=r.device.source_samples.stats();
        let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
        let ready=if thumbnail {r.prepare_thumbnail_batch(ThumbnailTarget::Occurrence(owner)).unwrap()}
            else {r.prefetch_retouch(&frame,false,&mut encoder).unwrap();!r.retouch.as_ref().unwrap().pending()};
        assert!(!ready,"a cold exact read remains pending");
        assert_eq!(r.device.source_samples.stats(),before,"the initiating UI query cannot decompress source pixels");
        r.uploads.finish(&encoder);r.last_submission=Some(encoder.submit(&r.queue));r.wait_idle().unwrap();
        let mut ready=false;
        for _ in 0..500 {
            if thumbnail {ready=r.prepare_thumbnail_batch(ThumbnailTarget::Occurrence(owner)).unwrap();}
            else {
                let mut encoder=crate::submission::CommandEncoder::new(&r.device,&Default::default());
                r.prefetch_retouch(&frame,false,&mut encoder).unwrap();
                ready = !r.retouch.as_ref().unwrap().pending();
                r.uploads.finish(&encoder);r.last_submission=Some(encoder.submit(&r.queue));
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
