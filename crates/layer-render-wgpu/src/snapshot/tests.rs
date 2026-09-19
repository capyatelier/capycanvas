use super::*;
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbSpace};
use layer_core::raster::{RasterRevision, RasterTile, RasterWatercolor, TileBlob, TileKey};
use layer_core::{Affine, Document, EffectInstance, LayerMask, Point, Selection, SelectionPixels};
use std::io::Cursor;

mod placement;

#[test]
fn float32_exr_and_deliberate_pq_sdr_delivery_leave_master_unchanged() {
    let mut document = Document::new("Float32 delivery", 3, 1);
    document.color.depth = SampleDepth::F32;
    document.layers[1].visible = false;
    let target = SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::F32, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let input = [[100000.125f32, -0.125, 2., 0.5], [4., 2., 1., 1.], [1e-20, -1., 4., 1. / 65536.]];
    let mut builder = SourceBuilder::new([3, 1], target.clone(), 1024 * 1024).unwrap();
    builder.push_row(&input.into_iter().flatten().flat_map(f32::to_le_bytes).collect::<Vec<_>>()).unwrap();
    document.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    let project = Project { document, assets: Default::default() };
    let mut renderer = SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
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
fn hdr_flattened_storage_ignores_sdr_rendition() {
    use layer_core::color::hdr;
    let mut document = Document::new("HDR flattened copy", 3, 1);
    document.color.depth = SampleDepth::F16;
    document.layers[1].visible = false;
    let target = SourceInterpretation { channels: SourceChannels::Rgba,
        depth: SampleDepth::F16, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let input = [[8., -0.125, 2., 0.5], [4., 2., 1., 1.], [1. / 65536., -1., 4., 1. / 65536.]];
    let mut builder = SourceBuilder::new([3, 1], target.clone(), 1024 * 1024).unwrap();
    builder.push_row(&input.into_iter().flat_map(|p| hdr::encode_pixel(p).unwrap()).flat_map(u16::to_le_bytes).collect::<Vec<_>>()).unwrap();
    document.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    let id = document.allocate_layer_id();
    let mut layer = Layer::paint(id, "+1 EV");
    layer.kind = LayerKind::Effect;
    let mut effect = EffectInstance::new(layer_core::bundled_effect_catalog().get("exposure").unwrap().program());
    effect.set("exposure", layer_core::EffectValue::Number(1.)).unwrap();
    layer.effect = Some(Arc::new(effect));
    document.layers.insert(0, layer);
    document.sdr_rendition = hdr::SdrRendition { exposure: -4., contrast: 2., headroom: 4., ..Default::default() };
    let mut renderer = SnapshotRenderer::new(Project { document, assets: Default::default() }, [0.; 4], 0., Default::default()).unwrap();
    let mut bytes = vec![0; 24];
    renderer.write_rows(&target, Default::default(), None, |_, _, read| read(0, &mut bytes)).unwrap();
    let expected: Vec<_> = input.into_iter().flat_map(|p| hdr::encode_pixel([2. * p[0], 2. * p[1], 2. * p[2], p[3]]).unwrap()).flat_map(u16::to_le_bytes).collect();
    assert_eq!(bytes, expected, "flattening a floating master must retain HDR values");
    let master = renderer.preview_document_for_display([3, 1], RgbSpace::Srgb, 49.).unwrap();
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
    assert_eq!(renderer.preview_document_for_display([3, 1], RgbSpace::Srgb, 49.).unwrap().pixels, master.pixels,
        "saved SDR appearance must not affect the HDR master preview");
    assert!(renderer.preview_document_for_display([3, 1], RgbSpace::Srgb, f32::NAN).is_err());
}

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
                SampleDepth::F16 | SampleDepth::F32 => unreachable!("SDR-only fixture"),
                    SampleDepth::U8 => row.push(value as u8),
                    SampleDepth::U16 => row.extend((value as u16).to_le_bytes()),
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
        for depth in [SampleDepth::U8, SampleDepth::U16] {
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
                assert_eq!(reader.control().output_rows(), 256);
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
            depth: SampleDepth::U16,
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
            depth: SampleDepth::U16,
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
        depth: SampleDepth::U16,
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
fn shared_capture_keeps_private_pixels_during_live_frames_and_after_canvas_close() {
    for color in [
        DocumentColor::default(),
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
    ] {
        let project = rich_project(color, 1);
        let (mut live, expected) = frame(&project);
        let cached_before = live.source_sample_cache_stats();
        let expected = Arc::new(expected);
        let [width, height] = [project.document.width, project.document.height];
        let mut capture = live
            .snapshot_gpu()
            .capture(
                project.clone(),
                [0.; 4],
                0.,
                Default::default(),
                Default::default(),
            )
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
        let mut layers = project.document.layers.clone();
        for i in 0..16 {
            layers[0].opacity = if i % 2 == 0 { 0.2 } else { 0.9 };
            live.submit(FramePacket {
                view: layer_render::ViewState {
                    width_px: width,
                    height_px: height,
                    background_rgba_linear: [0.; 4],
                    document_to_surface: [1., 0., 0., 1., 0., 0.],
                },
                document_extent: [width, height],
                layers: &layers,
                dabs: &[],
                dab_batches: &[],
                restore_rasters: &[],
                reset_layers: false,
                composite_all: true,
                time_seconds: 0.,
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
    let project = rich_project(color, 1);
    let control = CaptureControl::with_allocation_tracking();
    let mut reader =
        SnapshotRenderer::with_control(project, [0.; 4], 0., Default::default(), control.clone())
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
    reader.limits.planned_pixel_bytes = 0;
    let required = |error| match error {
        GpuRasterError::CaptureBudget { required, .. } => required,
        other => panic!("unexpected capture error: {other}"),
    };
    let small = required(reader.read_region([0, 0, width, 16]).unwrap_err());
    let large = required(reader.read_region([0, 0, width, 256]).unwrap_err());
    assert!(large > small);
    reader.limits.planned_pixel_bytes = small;
    let observations = control.allocation_peaks().unwrap().observations;
    let (rows, pixels) = reader.read_band(0).unwrap();
    assert_eq!(rows, 16);
    assert_eq!(pixels, reference[..width as usize * 16]);
    assert_eq!(
        control.allocation_peaks().unwrap().observations,
        observations + u64::from(allocation_reports)
    );
    reader.control().cancel();
    assert!(reader.read_band(0).is_err());
}

#[test]
fn shared_snapshot_chunks_preserve_masked_effect_pixels_across_column_boundaries() {
    let mut project = rich_project(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 }, 1);
    project.document.width = 2053;
    for layer in &mut project.document.layers {
        if layer.kind == layer_core::LayerKind::Paint { layer.properties.placement.0[4] += 800.; }
    }
    let (live, expected) = frame(&project);
    let mut capture = live.snapshot_gpu().capture(project, [0.; 4], 0., Default::default(), Default::default()).unwrap();
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
            depth: SampleDepth::U16,
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
    let result = SnapshotRenderer::with_control(
        source_project(DocumentColor::default(), [8, 8]),
        [0.; 4],
        0.,
        Default::default(),
        control.clone(),
    );
    assert!(matches!(result, Err(GpuRasterError::Color(e)) if e.contains("cancelled")));
    assert_eq!(control.output_rows(), 0);
}

#[test]
fn snapshot_jpeg_applies_profile_and_linear_matte_before_lossy_encoding() {
    let project = source_project(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        [33, 17],
    );
    let mut reader = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
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
    let project = source_project(color, [513, 35]);
    let original = project.clone();
    let mut reader =
        SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
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
    let project = source_project(
        DocumentColor {
            depth: SampleDepth::U8,
            ..color
        },
        [513, 35],
    );
    let source = project.document.layers[0].source.as_ref().unwrap().clone();
    let mut reader = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
    let mut bytes = Vec::new();
    reader
        .write_png(&mut bytes, &source.interpretation, options, None)
        .unwrap();
    assert_eq!(raw_rows(&decode(bytes)), raw_rows(&source));
}

mod resized;

#[test]
fn flattened_copy_preserves_complete_composition_precision_extent_and_resolution() {
    let mut original = rich_project(
        DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        },
        2,
    );
    original.document.resolution = Some(layer_core::ImageResolution::ppi(300));
    let (_, full) = frame(&original);
    let mut reader =
        SnapshotRenderer::new(original.clone(), [0.; 4], 0., Default::default()).unwrap();
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let result = reader
        .flattened_document(color, Default::default(), 64 * 1024 * 1024)
        .unwrap();
    let copy = result.project;
    assert_eq!(copy.document.color, color);
    assert_eq!(
        [copy.document.width, copy.document.height],
        [original.document.width, original.document.height]
    );
    assert_eq!(copy.document.resolution, original.document.resolution);
    assert_eq!(copy.document.layers.len(), 1);
    let source = copy.document.layers[0].source.as_ref().unwrap();
    assert_eq!(
        source.kind,
        layer_core::color::source::SourceKind::Rasterized
    );
    assert_eq!(source.resolution, original.document.resolution);
    let encoder = layer_color::WorkingEncoder::new(
        original.document.color.space,
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
    let mut bytes = Vec::new();
    copy.write(&mut bytes).unwrap();
    let reopened = Project::read(Cursor::new(bytes), Default::default()).unwrap();
    assert_eq!(
        raw_rows(reopened.document.layers[0].source.as_ref().unwrap()),
        actual
    );
    assert_eq!(reopened.document.resolution, original.document.resolution);
    reader.control().cancel();
    assert!(
        reader
            .flattened_document(color, Default::default(), 64 * 1024 * 1024)
            .is_err()
    );
}
