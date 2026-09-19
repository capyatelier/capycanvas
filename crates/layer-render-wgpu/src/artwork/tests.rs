use super::*;
use layer_core::color::{DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
use layer_core::{Document, LayerMask, Point};
use layer_render::{
    ColorSampleArea, ColorSampleRequest, ColorSampleSource, RegionRequest, RegionSource,
};

const EXTENT: [u32; 2] = [513, 273];
fn codes(x: u32) -> [u16; 4] {
    [10000 + (x / 256) as u16 * 16000, 32123, 51007, 40000]
}
fn document(color: DocumentColor) -> Document {
    let mut doc = Document::new("exact query", EXTENT[0], EXTENT[1]);
    doc.color = color;
    doc.layers[1].visible = false;
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
    doc.layers[0].raster = RasterRevision::backed(data);
    doc
}
fn frame<'a>(doc: &'a Document) -> FramePacket<'a> {
    FramePacket {
        layers: &doc.layers,
        document_extent: EXTENT,
        view: ViewState {
            width_px: 640,
            height_px: 480,
            document_to_surface: [1., 0., 0., 1., 0., 0.],
            background_rgba_linear: [0.; 4],
        },
        time_seconds: 0.,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: false,
        composite_all: true,
    }
}
fn complete(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
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
        let mut mask = LayerMask::reveal_all(LayerId(99), Point::default());
        mask.default_coverage = 0.5;
        mask.show_area = true;
        doc.layers[0].mask = Some(mask);
        doc.layers[0].opacity = 0.7;
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.native_edit.as_mut().unwrap().color_cache_bytes = 0;
        r.submit(frame(&doc)).unwrap();
        let visible = r.composite_texture.clone().unwrap();
        let before = crate::layer_tests::page_bytes(&r, &visible);
        // Display storage can be removed/replaced without changing query input.
        r.composite_texture = None;
        r.composite_view = None;
        r.composite_bind_group = None;
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
        assert!(r.composite_texture.is_none());
    }
}

#[test]
fn filtered_query_crops_match_full_resolution_and_reject_excessive_dependencies() {
    let mut doc = document(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    doc.layers
        .insert(0, crate::tests::image_windows::effect(20, false, false));
    doc.layers
        .insert(0, crate::tests::image_windows::effect(21, false, false));
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.submit(frame(&doc)).unwrap();
    let visible = r.composite_texture.clone().unwrap();
    let full = crate::layer_tests::page_bytes(&r, &visible);
    let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
    r.encode_clear_value(
        &mut encoder,
        r.composite_view.as_ref().unwrap(),
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
fn projected_watercolor_queries_keep_layer_style_identity_and_prediction() {
    let mut doc = document(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    let mut unrelated = Layer::paint(LayerId(8), "unrelated style slot");
    unrelated.visible = false;
    doc.layers.insert(0, unrelated);
    let id = doc.layers[1].id;
    let mut r = WgpuRasterizer::new_native_headless(doc.color).unwrap();
    r.submit(frame(&doc)).unwrap();
    let mut style = crate::tests::test_style(BrushExecution::Watercolor);
    style.rendering.wet_edge = 0.9;
    style.rendering.burnt_edge = 0.7;
    style.rendering.edge_width = 7.;
    style.wet_mix.wetness = 0.8;
    style.wet_mix.amount_of_paint = 0.7;
    let mut dab = crate::tests::test_dab([257., 249.], [0.8, 0.1, 0.3, 0.7], 0.8);
    dab.material = [0., 0.8, 0.9, 0.7];
    let mut batch = DabBatch {
        material_update: 0,
        stroke_id: layer_core::StrokeId(1),
        layer_id: id,
        kind: DabBatchKind::Persistent,
        stroke_start: true,
        stroke_end: false,
        first_dab: 0,
        dab_count: 1,
        style,
        damage: layer_core::Rect {
            min: Point { x: 225., y: 215. },
            max: Point { x: 295., y: 273. },
        },
    };
    let crop = PixelRect::new(224, 224, 289, 273);
    let mut capture = Capture::default();
    for preview in [false, true] {
        if preview {
            batch.kind = DabBatchKind::Preview;
            batch.stroke_start = false;
            dab.center.x += 9.;
        }
        r.submit(FramePacket {
            dabs: &[dab],
            dab_batches: std::slice::from_ref(&batch),
            ..frame(&doc)
        })
        .unwrap();
        let displayed = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
        let mut projected = (**r.artwork_frame.as_ref().unwrap()).clone();
        projected.layers = vec![doc.layers[1].clone()];
        let mut encoder = submission::CommandEncoder::new(&r.device, &Default::default());
        let output = capture
            .region(
                &mut r,
                projected.packet(EXTENT),
                crop,
                [crop.width(), crop.height()],
                &mut encoder,
            )
            .unwrap();
        r.uploads.finish(&encoder);
        encoder.submit(&r.queue);
        let pixels = crate::layer_tests::page_bytes(&r, &output.texture);
        for y in 0..crop.height() {
            for x in 0..crop.width() {
                let i = ((y + crop.min_y()) * EXTENT[0] + x + crop.min_x()) as usize * 16;
                let j = (y * crop.width() + x) as usize * 16;
                close(
                    std::array::from_fn(|c| {
                        f32::from_le_bytes(pixels[j + c * 4..j + c * 4 + 4].try_into().unwrap())
                    }),
                    std::array::from_fn(|c| {
                        f32::from_le_bytes(displayed[i + c * 4..i + c * 4 + 4].try_into().unwrap())
                    }),
                );
            }
        }
    }
}
