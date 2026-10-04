//! Real GPU readback: circular coverage, perceptual averaging and exact points.
use super::*;
use layer_core::{
    Document, authored::{PortableId,SourceTarget},
    color::{DocumentColor, RgbSpace, SampleDepth},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey},
};
use layer_render::{ColorSampleArea, ColorSampleRequest, ColorSampleSource};

#[test]
fn color_picker_circular_oklab_averaging_keeps_points_alpha_and_extended_values() {
    for space in [RgbSpace::Srgb, RgbSpace::DisplayP3] {
        let color = DocumentColor {
            space,
            depth: SampleDepth::F32,
        };
        let mut doc = Document::new(PortableId::random(), 128, 128, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let root = doc.artwork.root;
        doc.artwork.compositions.get_mut(root).unwrap().color = color;
        let paper = doc.scene().order()[1];
        doc.artwork.occurrences.get_mut(paper).unwrap().visible = false;
        let mut bytes = Vec::with_capacity(256 * 256 * 16);
        for y in 0..256i32 {
            for x in 0..256i32 {
                let d = [x - 64, y - 64];
                let v = if x < 64 {
                    0.
                } else if x > 64 {
                    1.
                } else {
                    0.125
                };
                let rgba: [f32; 4] = if x < 14 && y < 14 {
                    if (x + y) % 2 == 0 {
                        [4., -0.1, 0.5, 0.25]
                    } else {
                        [0.; 4]
                    }
                } else if d[0] * d[0] + d[1] * d[1] > 2550 {
                    [0., 1., 0., 1.]
                } else {
                    [v, v, v, 1.]
                };
                bytes.extend(rgba.into_iter().flat_map(f32::to_le_bytes));
            }
        }
        let mut data = RasterData::default();
        data.tiles.insert(
            TileKey {
                plane: RasterPlane::Color,
                coordinate: [0, 0],
            },
            RasterTile::backed(TileBlob::encode(color.paint_descriptor(), &bytes).unwrap()),
        );
        let target = doc.scene().source_target(doc.scene().order()[0]).unwrap();
        let SourceTarget::Paint(handle) = target else { unreachable!() };
        doc.artwork.paint.get_mut(handle).unwrap().raster = RasterRevision::backed(data);
        let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
        r.submit(FramePacket {
            ..packet(doc.scene(), [128, 128])
        })
        .unwrap();
        for area in [ColorSampleArea::Point, ColorSampleArea::Circle5] {
            assert!(
                r.request_color_sample(ColorSampleRequest {
                    request_id: 2,
                    source: ColorSampleSource::Source(target),
                    position: [8, 8],
                    area
                })
                .unwrap()
            );
            r.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(READBACK_TIMEOUT),
                })
                .unwrap();
            let rgba = r.take_color_sample().unwrap().unwrap().rgba;
            for (got, expected) in rgba[..3].iter().zip([4., -0.1, 0.5]) {
                assert!(
                    (*got - expected).abs() < 2e-5,
                    "{space:?} {area:?} {rgba:?}"
                );
            }
            let alpha = if area == ColorSampleArea::Point {
                0.25
            } else {
                0.25 * 9. / 21.
            };
            assert!((rgba[3] - alpha).abs() < 1e-6, "{rgba:?}");
        }
        for source in [
            ColorSampleSource::Source(target),
            ColorSampleSource::Composite,
        ] {
            for (area, position, expected) in [
                (ColorSampleArea::Circle101, [64, 64], 0.125),
                (ColorSampleArea::Circle51, [64, 64], 0.125),
                (ColorSampleArea::Circle15, [64, 64], 0.125),
                (ColorSampleArea::Circle5, [64, 64], 0.125),
                (ColorSampleArea::Point, [65, 64], 1.),
            ] {
                assert!(
                    r.request_color_sample(ColorSampleRequest {
                        request_id: 1,
                        source,
                        position,
                        area
                    })
                    .unwrap()
                );
                r.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(READBACK_TIMEOUT),
                    })
                    .unwrap();
                let rgba = r.take_color_sample().unwrap().unwrap().rgba;
                assert!(
                    rgba[..3].iter().all(|c| (*c - expected).abs() < 2e-5),
                    "{space:?} {source:?} {area:?}: {rgba:?}"
                );
                assert_eq!(rgba[3], 1.);
            }
        }
    }
}
