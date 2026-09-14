use super::*;
use layer_core::color::{ColorProfile, DocumentColor, IntegerDepth, RgbSpace};
use layer_core::raster::{RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey};
use layer_core::{Affine, Document, EffectInstance, LayerMask, Point, Selection, SelectionPixels};
use std::io::Cursor;

fn source_project(color: DocumentColor, extent: [u32; 2]) -> Project {
    let mut document = Document::new("snapshot fixture", extent[0], extent[1]);
    document.color = color;
    document.layers[1].visible = false;
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
                    IntegerDepth::U8 => row.push(value as u8),
                    IntegerDepth::U16 => row.extend((value as u16).to_le_bytes()),
                }
            }
        }
        builder.push_row(&row).unwrap();
    }
    document.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    Project {
        document,
        assets: Default::default(),
    }
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
fn frame(project: &Project) -> (WgpuRasterizer, Vec<[f32; 4]>) {
    let mut r = WgpuRasterizer::new_native_headless(project.document.color).unwrap();
    let extent = [project.document.width, project.document.height];
    r.submit(FramePacket {
        view: layer_render::ViewState {
            width_px: extent[0],
            height_px: extent[1],
            background_rgba_linear: [0.; 4],
            document_to_surface: [1., 0., 0., 1., 0., 0.],
        },
        document_extent: extent,
        layers: &project.document.layers,
        dabs: &[],
        dab_batches: &[],
        restore_rasters: &[],
        reset_layers: true,
        composite_all: true,
        time_seconds: 0.,
    })
    .unwrap();
    let bytes = crate::layer_tests::page_bytes(&r, r.composite_texture.as_ref().unwrap());
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
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let project = source_project(DocumentColor { space, depth }, [257, 256]);
            let source = project.document.layers[0].source.as_ref().unwrap().clone();
            let expected = raw_rows(&source);
            let target = source.interpretation.clone();
            let mut reader =
                SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
            for tiff in [false, true] {
                let mut output = Cursor::new(Vec::new());
                let stats = if tiff {
                    reader.write_tiff(&mut output, &target, Default::default(), None)
                } else {
                    reader.write_png(&mut output, &target, Default::default(), None)
                }
                .unwrap();
                assert_eq!(stats.clipped_channels, 0);
                let decoded = decode(output.into_inner());
                assert_eq!(decoded.interpretation.depth, depth);
                assert_eq!(
                    raw_rows(&decoded),
                    expected,
                    "{space:?} {depth:?} tiff={tiff}"
                );
                assert!(reader.renderer.composite_texture.is_none());
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
    let mut project = source_project(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        },
        [257, 256],
    );
    let original = project.document.layers[0].source.as_ref().unwrap().clone();
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
    project.document.layers[0].source = Some(gray);
    let mut reader = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
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
    let project = source_project(
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U16,
        },
        [33, 17],
    );
    let target = project.document.layers[0]
        .source
        .as_ref()
        .unwrap()
        .interpretation
        .clone();
    let mut reader = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
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

#[test]
fn snapshot_legacy_project_images_use_native_primary_conversion_without_full_upload() {
    let mut project = source_project(DocumentColor::default(), [33, 17]);
    let source = project.document.layers[0].source.take().unwrap();
    let bytes = raw_rows(&source);
    let asset = layer_core::AssetId("test:legacy snapshot image".into());
    project.document.layers[0].asset = Some(asset.clone());
    project.document.color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    };
    project.assets.insert(
        asset,
        layer_core::ProjectAsset {
            extent: [33, 17],
            format: ProjectAssetFormat::Rgba8Srgb,
            bytes: bytes.clone().into(),
        },
    );
    let mut reader = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
    let actual = reader.read_region([0, 0, 33, 17]).unwrap();
    let decoder = layer_color::WorkingDecoder::new(
        &source.interpretation,
        RgbSpace::ProPhoto,
        Default::default(),
    )
    .unwrap();
    let mut expected = vec![[0.; 4]; 33 * 17];
    decoder.decode_pixels(&bytes, &mut expected).unwrap();
    for (actual, expected) in actual.iter().zip(expected) {
        for c in 0..4 {
            let expected = if c == 3 {
                expected[3]
            } else {
                expected[c] * expected[3]
            };
            assert!((actual[c] - expected).abs() <= 2e-6);
        }
    }
    assert!(reader.renderer.composite_texture.is_none());
    assert!(reader.renderer.image_sources.is_empty());
    assert!(
        reader
            .renderer
            .paint_layers
            .iter()
            .all(|l| l.pages.is_empty())
    );
}

fn rich_project(color: DocumentColor, mask_kind: u32) -> Project {
    let mut project = source_project(color, [641, 389]);
    let doc = &mut project.document;
    let mask_id = doc.allocate_layer_id();
    let group_id = doc.allocate_layer_id();
    let effect_id = doc.allocate_layer_id();
    let mut mask = LayerMask::reveal_all(mask_id, Point { x: 13., y: -7. });
    mask.default_coverage = 0.;
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
                    (0..8).fold(0u32, |v, n| {
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
    mask.initial = Some(selection);
    let scalar = match color.depth {
        IntegerDepth::U8 => vec![123; 65536],
        IntegerDepth::U16 => 32001u16.to_le_bytes().repeat(65536),
    };
    let mut mask_data = RasterData::default();
    mask_data.tiles.insert(
        TileKey {
            plane: RasterPlane::Mask,
            coordinate: [1, 0],
        },
        RasterTile::backed(TileBlob::encode(color.coverage_descriptor(), &scalar).unwrap()),
    );
    mask.raster = RasterRevision::backed(mask_data);
    let mut data = RasterData {
        watercolor: Some(RasterWatercolor {
            wet_edge: 0.6,
            burnt_edge: 0.3,
            edge_width: 7.,
        }),
        ..Default::default()
    };
    let paint = match color.depth {
        IntegerDepth::U8 => [92u8, 41, 71, 123].repeat(65536),
        IntegerDepth::U16 => [30001u16, 17003, 49117, 32768]
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
    doc.layers[0].raster = RasterRevision::backed(data);
    doc.layers[0].mask = Some(mask);
    doc.layers[0].properties.offset = Point { x: -11., y: 9. };
    doc.layers[0].properties.parent = Some(group_id);
    let mut effect = Layer::paint(effect_id, "blur");
    effect.kind = LayerKind::Effect;
    effect.properties.parent = Some(group_id);
    let mut program = (*crate::tests::fixture("exposure").program()).clone();
    program.entry = "blur".into();
    program.wgsl="fn blur(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{return (fx_sample(p+vec2<f32>(3.,0.))+c+fx_sample(p-vec2<f32>(3.,0.)))/3.;}".into();
    program.passes = vec![layer_core::EffectPass {
        entry: "blur".into(),
        sampling: layer_core::EffectSampling::Neighborhood { radius: 3 },
    }]
    .into();
    effect.effect = Some(Arc::new(EffectInstance::new(Arc::new(program))));
    let mut group = Layer::paint(group_id, "group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 3., y: 5. };
    group.opacity = 0.73;
    doc.layers.insert(0, effect);
    doc.layers.insert(0, group);
    project
}

#[test]
fn snapshot_crops_restore_masked_native_material_and_selection_windows() {
    for color in [
        DocumentColor::default(),
        DocumentColor {
            space: RgbSpace::DisplayP3,
            depth: IntegerDepth::U16,
        },
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        },
    ] {
        for mask in 0..3 {
            let project = rich_project(color, mask);
            let (_full_renderer, full) = frame(&project);
            let mut project = if mask == 2 {
                let mut archive = Vec::new();
                project.write(&mut archive).unwrap();
                Project::read(Cursor::new(archive), Default::default()).unwrap()
            } else {
                project
            };
            // Viewing the mask area must not change snapshot artwork.
            for layer in &mut project.document.layers {
                if let Some(mask) = &mut layer.mask {
                    mask.show_area = true;
                }
            }
            let mut reader =
                SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
            assert!(reader.renderer.composite_texture.is_none());
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
                assert!(reader.renderer.composite_texture.is_none());
                assert_eq!(reader.renderer.metrics().composite_storage_bytes, 0);
                assert!(reader.renderer.selection_clip.storage_bytes() < 641 * 389 / 2);
            }
        }
    }
}

#[test]
fn snapshot_profiled_composite_rows_match_full_render_and_honor_budget_and_cancel() {
    let project = rich_project(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: IntegerDepth::U16,
        },
        2,
    );
    let (_r, full) = frame(&project);
    let before = project
        .document
        .layers
        .iter()
        .map(|l| l.raster.identity())
        .collect::<Vec<_>>();
    let mut reader =
        SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
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
                    .encode_premultiplied(&full, &mut expected, None)
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
                        if depth == IntegerDepth::U8 {
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
        project
            .document
            .layers
            .iter()
            .map(|l| l.raster.identity())
            .collect::<Vec<_>>()
    );
    reader.limits.planned_pixel_bytes = 1;
    assert!(reader.read_region([0, 0, 17, 17]).is_err());
    assert!(reader.renderer.composite_texture.is_none());
    reader.cancellation().store(true, Ordering::Relaxed);
    let mut out = Vec::new();
    assert!(
        reader
            .write_png(
                &mut out,
                &SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth: IntegerDepth::U16,
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
