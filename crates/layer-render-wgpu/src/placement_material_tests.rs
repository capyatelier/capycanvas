use super::*;
use layer_core::{Affine, ImageTransform, Interpolation, MeshMap, Projective, LayerPlacement, raster::*};
use std::collections::BTreeMap;
use std::sync::Arc;

const EXTENT: [u32; 2] = [512; 2];

fn field(plane: RasterPlane, x: f32, y: f32) -> Vec<u8> {
    let inside = (232. ..272.).contains(&x) && (120. ..160.).contains(&y);
    match plane {
        RasterPlane::Color => if inside { vec![76, 38, 19, 128] } else { vec![0; 4] },
        RasterPlane::Wetness => vec![if inside { 37 + (x.floor() as u32 % 8) as u8 * 19 } else { 0 }],
        RasterPlane::WatercolorWetness => vec![if inside { 89 + (y.floor() as u32 % 8) as u8 * 17 } else { 0 }],
        RasterPlane::Mask => unreachable!(),
    }
}

fn fixture(map: Affine, width: f32) -> RasterData {
    let inverse = map.inverse().unwrap();
    let mut data = RasterData {
        watercolor: Some(RasterWatercolor { wet_edge: 0.9, burnt_edge: 0.6, edge_width: width }),
        ..Default::default()
    };
    for plane in [RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
        for coordinate in [[0, 0], [1, 0], [0, 1], [1, 1]] {
            let mut bytes = Vec::new();
            for y in 0..PAGE_SIZE {
                for x in 0..PAGE_SIZE {
                    let point = inverse.map(Point {
                        x: (coordinate[0] * PAGE_SIZE + x) as f32 + 0.5,
                        y: (coordinate[1] * PAGE_SIZE + y) as f32 + 0.5,
                    });
                    bytes.extend(field(plane, point.x, point.y));
                }
            }
            if bytes.iter().any(|v| *v != 0) {
                data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(
                    TileBlob::encode(plane.descriptor(Default::default()), &bytes).unwrap()));
            }
        }
    }
    data
}

fn layer(data: RasterData, map: Affine) -> Layer {
    let mut layer = Layer::paint(LayerId(1), "independent wet rectangle");
    layer.raster = RasterRevision::backed(data);
    layer.properties.placement = layer_core::LayerPlacement::from_affine(map);
    layer
}

fn render(r: &mut WgpuRasterizer, layer: &Layer, batches: &[DabBatch], reset: bool) -> Vec<u8> {
    r.submit(FramePacket {
        dab_batches: batches,
        reset_layers: reset,
        ..packet(std::slice::from_ref(layer), EXTENT)
    }).unwrap();
    r.readback_srgb_rgba8().unwrap()
}

fn assert_pixels(actual: &[u8], expected: &[u8], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}");
    let changed = actual.chunks_exact(4).zip(expected.chunks_exact(4)).filter(|(a, b)| a != b).count();
    let error = actual.iter().zip(expected).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(changed == 0, "{context}: {changed} differing pixels, maximum channel error {error}");
}

fn live_planes(r: &WgpuRasterizer) -> std::collections::BTreeMap<TileKey, Vec<u8>> {
    let layer = &r.paint_layers[0];
    layer.pages.iter().map(|p| (TileKey { plane: RasterPlane::Color, coordinate: p.coordinate },
        page_bytes(r, &p.active().texture)))
        .chain(layer.material_pages.iter().map(|p| (TileKey { plane: RasterPlane::Wetness, coordinate: p.coordinate },
            page_bytes(r, &p.wetness.texture))))
        .chain(layer.watercolor_wetness_pages.iter().map(|p| (TileKey { plane: RasterPlane::WatercolorWetness, coordinate: p.coordinate },
            page_bytes(r, &p.active().texture))))
        .collect()
}

fn maps() -> [Affine; 2] {
    [Affine([0.5, 0., 0., 0.5, 128., 176.]), Affine([0.5, 0., 0., 1., 128., 112.])]
}

#[test]
fn affine_material_transform_maps_raw_planes_before_edges_and_keeps_tile_neighbors() {
    let mut transformed = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for map in maps() {
        for width in [1., 4., 16.] {
            let mut source = layer(fixture(Affine::IDENTITY, width), Affine::IDENTITY);
            render(&mut transformed, &source, &[], true);
            let destination = layer(fixture(map, width), Affine::IDENTITY);
            let expected_pixels = render(&mut expected, &destination, &[], true);
            let mut coverage = LayerMask::reveal_all(LayerId(9), Point::default());
            coverage.default_coverage = 1.;
            let operation = LayerOperation {
                placement: Affine::IDENTITY,
                coverage,
                kind: LayerOperationKind::Transform(ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(map) },
                    ..Default::default()
                }),
            };
            let operation_batch = DabBatch {
                kind: DabBatchKind::LayerOperation(0),
                dab_count: 0,
                damage: operation.bounds(EXTENT),
                ..batch(1)
            };
            source.pending_operations.push(operation);
            let actual = render(&mut transformed, &source, &[operation_batch], false);
            let actual_planes = live_planes(&transformed);
            for (key, bytes) in live_planes(&expected) {
                assert!(actual_planes.get(&key) == Some(&bytes), "{map:?} width={width} {key:?}: mapped raw plane differs");
            }
            for (key, bytes) in &actual_planes {
                if !destination.raster.wait_data().unwrap().tiles.contains_key(key) {
                    assert!(bytes.iter().all(|v| *v == 0), "unmapped {map:?} width={width} {key:?}");
                }
            }
            assert_eq!(transformed.paint_layers[0].watercolor, expected.paint_layers[0].watercolor);
            assert_pixels(&actual, &expected_pixels, &format!("{map:?} width={width}: destination halo/style parity"));
        }
    }
}

#[test]
fn retained_affine_material_evaluates_mapped_raw_planes_in_document_coordinates() {
    let mut retained = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for map in maps() {
        for width in [1., 4., 16.] {
            let source = fixture(Affine::IDENTITY, width);
            let digests: Vec<_> = source.tiles.values().map(|tile| tile.wait_backing().unwrap().digest).collect();
            let mut placed = layer(source.clone(), map);
            placed.properties.extent = Some([280, 168]);
            let identity = placed.raster.identity();
            let mut original = placed.clone();
            original.properties.placement = layer_core::LayerPlacement::from_affine(Affine::IDENTITY);
            render(&mut expected, &original, &[], true);
            let original_planes = live_planes(&expected);
            let actual = render(&mut retained, &placed, &[], true);
            assert!(live_planes(&retained) == original_planes,
                "{map:?} width={width}: retained material must preserve all original working planes");
            let expected_pixels = render(&mut expected, &layer(fixture(map, width), Affine::IDENTITY), &[], true);
            assert_pixels(&actual, &expected_pixels, &format!("{map:?} width={width}: retained destination material parity"));
            let at = |pixels: &[u8], x: usize, y: usize| pixels[(y * EXTENT[0] as usize + x) * 4 + 3];
            if width == 4. {
                assert!(at(&expected_pixels, 269, 250) > 0, "document-space halo reaches six pixels beyond pigment");
                assert_eq!(at(&actual, 269, 250), at(&expected_pixels, 269, 250), "retained material keeps document-space halo width");
                assert_eq!(at(&expected_pixels, 273, 250), 0, "halo stops outside two style widths");
                assert_eq!(at(&actual, 273, 250), 0);
            }
            assert_eq!(source.tiles.values().map(|tile| tile.wait_backing().unwrap().digest).collect::<Vec<_>>(), digests);
            assert_eq!(placed.raster.identity(), identity);
        }
    }
}

#[test]
fn retained_material_neighbor_cache_rejects_changed_placement_style_and_raster_roots() {
    let mut retained = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (frame, (map, width, wet)) in [
        (maps()[0], 4., true),
        (maps()[0], 16., true),
        (maps()[1], 16., true),
        (maps()[1], 16., false),
        (maps()[0], 4., true),
    ].into_iter().enumerate() {
        let mut original = fixture(Affine::IDENTITY, width);
        let mut destination = fixture(map, width);
        if !wet {
            original.tiles.retain(|key, _| key.plane != RasterPlane::WatercolorWetness);
            destination.tiles.retain(|key, _| key.plane != RasterPlane::WatercolorWetness);
        }
        let placed = layer(original, map);
        let actual = render(&mut retained, &placed, &[], frame == 0);
        let reference = render(&mut expected, &layer(destination, Affine::IDENTITY), &[], true);
        assert_pixels(&actual, &reference, &format!("frame {frame}: changed placement/style/raw root"));
    }
}

#[test]
fn cold_backed_scalar_planes_survive_destructive_affine_mapping_without_live_pages() {
    let mut transformed = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let map = maps()[1];
    let mut source = layer(fixture(Affine::IDENTITY, 4.), Affine::IDENTITY);
    let immutable = source.raster.clone();
    render(&mut transformed, &source, &[], true);
    assert!(!transformed.paint_layers[0].material_pages.is_empty());
    assert!(!transformed.paint_layers[0].watercolor_wetness_pages.is_empty());
    transformed.paint_layers[0].material_pages.clear();
    transformed.paint_layers[0].watercolor_wetness_pages.clear();
    for plane in [RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
        assert!(transformed.native_plane_tile(source.id, plane, [1, 0]).unwrap().is_some());
    }
    let destination = layer(fixture(map, 4.), Affine::IDENTITY);
    let expected_pixels = render(&mut expected, &destination, &[], true);
    let operation = LayerOperation {
        placement: Affine::IDENTITY,
        coverage: LayerMask::reveal_all(LayerId(9), Point::default()),
        kind: LayerOperationKind::Transform(ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Nearest, ..LayerPlacement::from_affine(map) }, ..Default::default()
        }),
    };
    let operation_batch = DabBatch { kind: DabBatchKind::LayerOperation(0), dab_count: 0,
        damage: operation.bounds(EXTENT), ..batch(1) };
    source.pending_operations.push(operation);
    let actual = render(&mut transformed, &source, &[operation_batch], false);
    let actual_planes = live_planes(&transformed);
    for (key, bytes) in live_planes(&expected) {
        let mapped = actual_planes.get(&key).unwrap_or_else(|| panic!("missing cold mapped {key:?}"));
        if key.plane == RasterPlane::Color {
            assert!(mapped == &bytes, "cold mapped pigment must match independent raw planes");
        } else {
            let error = crate::test_support::max_error_bytes(mapped, &bytes);
            assert!(error <= f32::EPSILON, "cold mapped {key:?}: Float32 error {error}");
            assert!(mapped.chunks_exact(4).zip(bytes.chunks_exact(4)).all(|(a, b)| {
                let code = |v: &[u8]| (f32::from_le_bytes(v.try_into().unwrap()) * 255.).round() as u8;
                code(a) == code(b)
            }), "cold mapped {key:?}: exact scalar codes");
        }
    }
    assert_pixels(&actual, &expected_pixels, "cold scalar destination material parity");
    assert_eq!(source.raster.identity(), immutable.identity());
}

#[test]
fn retained_affine_material_low_zoom_matches_independently_mapped_destination_planes() {
    let mut retained = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for map in maps() {
        for width in [4., 16.] {
            let mut placed = layer(fixture(Affine::IDENTITY, width), map);
            placed.properties.extent = Some([280, 168]);
            let mut destination = layer(fixture(map, width), Affine::IDENTITY);
            let identity = placed.raster.identity();
            for (zoom, blend, masked) in [
                (0.5, layer_core::BlendSpace::Linear, false),
                (0.5, layer_core::BlendSpace::Perceptual, false),
                (0.125, layer_core::BlendSpace::Linear, false),
                (0.125, layer_core::BlendSpace::Perceptual, false),
                (0.125, layer_core::BlendSpace::Perceptual, true),
            ] {
                if masked {
                    for source in [&mut placed, &mut destination] {
                        source.opacity = 0.65;
                        let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
                        mask.default_coverage = 0.63;
                        source.mask = Some(mask);
                    }
                }
                for (r, source) in [(&mut retained, &placed), (&mut expected, &destination)] {
                    let mut frame = packet(std::slice::from_ref(source), EXTENT);
                    frame.view.document_to_surface = [zoom, 0., 0., zoom, 0., 0.];
                    frame.blend_space = blend;
                    frame.reset_layers = true;
                    r.submit(frame).unwrap();
                    assert!(r.scale_display.as_ref().unwrap().plan.level > 0);
                }
                let a = retained.scale_display.as_ref().unwrap();
                let b = expected.scale_display.as_ref().unwrap();
                assert_eq!(a.plan, b.plan);
                let actual = crate::test_support::float_pixels(&retained, a.texture());
                let reference = crate::test_support::float_pixels(&expected, b.texture());
                let error = crate::test_support::max_error(&actual, &reference);
                let witness = actual.iter().zip(&reference).enumerate().max_by_key(|(_, (a, b))|
                    a.iter().zip(b.iter()).map(|(a, b)| (a - b).abs().to_bits()).max().unwrap()).unwrap();
                let coordinate = [witness.0 as u32 % a.plan.size[0], witness.0 as u32 / a.plan.size[0]];
                assert!(error < 1e-6, "{map:?} width={width} zoom={zoom} blend={blend:?} masked={masked}: display material maximum error {error}, texel={coordinate:?}, actual={:?}, expected={:?}", witness.1.0, witness.1.1);
                assert_eq!(placed.raster.identity(), identity);
            }
        }
    }
}

#[test]
fn minified_material_split_pieces_clear_color_scalar_and_clip_mask_coverage() {
    let data = |mapped: bool| {
        let mut data = RasterData { watercolor: Some(RasterWatercolor {
            wet_edge: 0.9, burnt_edge: 0.6, edge_width: 4.,
        }), ..Default::default() };
        let coordinates = if mapped { vec![[1, 0]] } else { vec![[8, 4], [9, 4], [8, 5], [9, 5]] };
        for plane in [RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
            for &coordinate in &coordinates {
                let mut bytes = Vec::new();
                for y in 0..PAGE_SIZE {
                    for x in 0..PAGE_SIZE {
                        let point = Point { x: (coordinate[0] * PAGE_SIZE + x) as f32 + 0.5,
                            y: (coordinate[1] * PAGE_SIZE + y) as f32 + 0.5 };
                        let inside = !mapped || ((256. ..320.).contains(&point.x) && (128. ..192.).contains(&point.y));
                        match plane {
                            RasterPlane::Color => bytes.extend(if inside { [76, 38, 19, 128] } else { [0; 4] }),
                            RasterPlane::Wetness => bytes.push(if inside { 103 } else { 0 }),
                            RasterPlane::WatercolorWetness => bytes.push(if inside { 127 } else { 0 }),
                            _ => unreachable!(),
                        }
                    }
                }
                data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(TileBlob::encode(plane.descriptor(Default::default()), &bytes).unwrap()));
            }
        }
        data
    };
    let mut source = layer(data(false), Affine([0.125, 0., 0., 0.125, 0., 0.]));
    source.properties.extent = Some([8192; 2]);
    let mut destination = layer(data(true), Affine::IDENTITY);
    for (layer, mapped) in [(&mut source, false), (&mut destination, true)] {
        let mut mask = LayerMask::reveal_all(LayerId(9), Point::default());
        let bytes = if mapped {
            (0..PAGE_SIZE * PAGE_SIZE).map(|i| {
                let x = PAGE_SIZE + i % PAGE_SIZE;
                let y = i / PAGE_SIZE;
                if (256..288).contains(&x) && (128..160).contains(&y) { 0 } else { 255 }
            }).collect()
        } else { vec![0; (PAGE_SIZE * PAGE_SIZE) as usize] };
        mask.raster = RasterRevision::backed(RasterData {
            tiles: [(TileKey { plane: RasterPlane::Mask, coordinate: if mapped { [1, 0] } else { [8, 4] } },
                RasterTile::backed(TileBlob::encode(RasterPlane::Mask.descriptor(Default::default()), &bytes).unwrap()))].into(),
            ..Default::default()
        });
        layer.mask = Some(mask);
    }
    let mut pieces = Vec::new();
    crate::paint_transform::snapshot::Splitter::new(PixelRect::full([8192; 2]),
        &ImageTransform { placement: source.properties.placement.clone(), ..Default::default() }, None, |_| true).unwrap()
        .split(PixelRect::full([PAGE_SIZE; 2]), &mut pieces).unwrap();
    assert!(pieces.len() > 1, "the compute clear must survive several clipped pieces");
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    render(&mut renderer, &layer(fixture(Affine::IDENTITY, 16.), maps()[1]), &[], true);
    let reference = render(&mut expected, &destination, &[], true);
    let actual = render(&mut renderer, &source, &[], true);
    assert!(actual.chunks_exact(4).filter(|pixel| pixel[3] > 0).count() > 100);
    assert_pixels(&actual, &reference, "split minification reuses color/scalar targets with clipped default-one mask");
}

#[test]
fn mapped_semitransparent_material_over_nontransparent_backdrop_matches_premultiplied_over() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut front = layer(fixture(Affine::IDENTITY, 16.), maps()[1]);
    front.opacity = 0.65;
    let mut background = Layer::paint(LayerId(2), "semitransparent backdrop");
    let mut data = RasterData::default();
    let bytes: Vec<_> = [20, 70, 130, 128].into_iter().cycle().take((PAGE_SIZE * PAGE_SIZE * 4) as usize).collect();
    for coordinate in [[0, 0], [1, 0], [0, 1], [1, 1]] {
        data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate }, RasterTile::backed(
            TileBlob::encode(RasterPlane::Color.descriptor(Default::default()), &bytes).unwrap()));
    }
    background.raster = RasterRevision::backed(data);
    let mut pixels = |layers: &[Layer]| {
        renderer.submit(FramePacket { reset_layers: true, ..packet(layers, EXTENT) }).unwrap();
        crate::scene::scale::tests::display_pixels(&renderer)
    };
    let source = pixels(std::slice::from_ref(&front));
    let base = pixels(std::slice::from_ref(&background));
    let actual = pixels(&[front, background]);
    let expected: Vec<[f32; 4]> = source.iter().zip(&base)
        .map(|(front, back)| std::array::from_fn(|channel| front[channel] + back[channel] * (1. - front[3])))
        .collect();
    assert!(source.iter().filter(|pixel| pixel[3] > 0. && pixel[3] < 1.).count() > 100);
    assert!(base.iter().all(|pixel| pixel[3] > 0. && pixel[3] < 1.));
    assert!(crate::test_support::max_error(&actual, &expected) < 1e-6, "native material over nontransparent pixels");
}

#[test]
fn dry_anisotropic_low_zoom_matches_independently_mapped_pigment_edges() {
    let map = Affine([0.5, 0., 0., 1., 128., 112.]);
    let pigment = |map| {
        let mut data = fixture(map, 4.);
        data.watercolor = None;
        data.tiles.retain(|key, _| key.plane == RasterPlane::Color);
        data
    };
    let mut placed = layer(pigment(Affine::IDENTITY), map);
    placed.properties.extent = Some([280, 168]);
    let destination = layer(pigment(map), Affine::IDENTITY);
    let mut retained = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (r, source) in [(&mut retained, &placed), (&mut expected, &destination)] {
        let mut frame = packet(std::slice::from_ref(source), EXTENT);
        frame.view.document_to_surface = [0.5, 0., 0., 0.5, 0., 0.];
        frame.blend_space = layer_core::BlendSpace::Linear;
        frame.reset_layers = true;
        r.submit(frame).unwrap();
    }
    assert_eq!(retained.scale_display.as_ref().unwrap().plan, expected.scale_display.as_ref().unwrap().plan);
    let actual = crate::scene::scale::tests::display_pixels(&retained);
    let reference = crate::scene::scale::tests::display_pixels(&expected);
    assert_eq!(crate::test_support::max_error(&actual, &reference), 0., "dry pigment edges under a 4 by 2 source footprint");
}

#[test]
fn perceptual_low_zoom_sparse_material_keeps_dry_photo_pixels_across_halo_page_edges() {
    let extent = [2048; 2];
    let map = Affine([0.5, 0., 0., 0.5, 256., 128.]);
    let photo = layer_core::color::source::rgba8_source(extent, |x, y|
        if (x / 4 + y / 4) % 2 == 0 { [230, 220, 210, 255] } else { [24, 32, 40, 255] });
    let mut data = fixture(Affine::IDENTITY, 16.);
    data.tiles.retain(|key, _| key.plane != RasterPlane::Color);
    let mut wet = layer(data.clone(), map);
    wet.source = Some(photo);
    data.watercolor = None;
    let mut dry = wet.clone();
    dry.raster = RasterRevision::backed(data);
    let mut actual = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (r, source) in [(&mut actual, &wet), (&mut expected, &dry)] {
        let mut frame = packet(std::slice::from_ref(source), extent);
        frame.reset_layers = true;
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        frame.view.width_px = 512;
        frame.view.height_px = 512;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 0., 0.];
        r.submit(frame).unwrap();
    }
    let a = actual.scale_display.as_ref().unwrap();
    let b = expected.scale_display.as_ref().unwrap();
    assert_eq!(a.plan, b.plan);
    assert!(a.plan.level > 0);
    let pixels = crate::scene::scale::tests::display_pixels(&actual);
    let reference = crate::scene::scale::tests::display_pixels(&expected);
    let wet_bounds = Rect { min: Point { x: 232., y: 120. }, max: Point { x: 272., y: 160. } };
    let halo = map.bounds(wet_bounds).outset(32.);
    assert!(halo.max.x < 600. && halo.max.y < 320.);
    let step = (1 << a.plan.level) as f32;
    let mut checked = 0;
    let mut error = 0f32;
    for (i, (pixel, reference)) in pixels.iter().zip(&reference).enumerate() {
        let point = Point {
            x: a.plan.bounds.min_x() as f32 + (i as u32 % a.plan.size[0]) as f32 * step + step * 0.5,
            y: a.plan.bounds.min_y() as f32 + (i as u32 / a.plan.size[0]) as f32 * step + step * 0.5,
        };
        if (600. ..850.).contains(&point.x) && (320. ..450.).contains(&point.y) {
            checked += 1;
            error = error.max(crate::test_support::max_error(std::slice::from_ref(pixel), std::slice::from_ref(reference)));
        }
    }
    assert!(checked > 100, "dry photo samples span native page edges");
    assert_eq!(error, 0., "{checked} samples far from wetness across halo page edges: maximum float error");
}

type Planes = BTreeMap<TileKey, Vec<u8>>;

fn assert_baked_codes(layer: &Layer, nonzero_planes: &[RasterPlane], expected: impl Fn(RasterPlane, Point) -> Vec<u8>) {
    let data = layer.raster.wait_data().unwrap();
    let mut planes = std::collections::BTreeSet::new();
    for (key, tile) in &data.tiles {
        if key.plane == RasterPlane::Mask { continue; }
        planes.insert(key.plane);
        let blob = tile.wait_backing().unwrap();
        let bytes = blob.decode().unwrap();
        assert!(bytes.iter().any(|value| *value != 0), "empty native {key:?} must stay sparse");
        let size = blob.descriptor.bytes_per_pixel().unwrap();
        for (i, actual) in bytes.chunks_exact(size).enumerate() {
            let point = Point {
                x: (key.coordinate[0] * PAGE_SIZE + i as u32 % PAGE_SIZE) as f32 + 0.5 + layer.properties.offset.x,
                y: (key.coordinate[1] * PAGE_SIZE + i as u32 / PAGE_SIZE) as f32 + 0.5 + layer.properties.offset.y,
            };
            assert!(actual == expected(key.plane, point), "baked raw {:?} at {point:?}", key.plane);
        }
    }
    assert_eq!(planes, nonzero_planes.iter().copied().collect());
}

#[test]
fn snapshot_affine_material_bake_preserves_raw_codes_masks_and_world_registration() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let map = Affine([0.5, 0., 0., 1., -80., -40.]);
    for linked in [true, false] {
        let mut document = layer_core::Document::new("material bake", EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.layers = vec![layer(fixture(Affine::IDENTITY, 4.), map)];
        document.active_layer = document.layers[0].id;
        let mut mask = LayerMask::reveal_all(document.allocate_layer_id(), Point { x: 24., y: 6. });
        mask.linked = linked;
        mask.inverted = true;
        mask.default_coverage = 0.;
        let [x, y] = if linked { [208., 114.] } else { [16., 80.] };
        mask.initial = Some(Selection::polygon(vec![Point { x, y }, Point { x: x + 16., y },
            Point { x: x + 16., y: y + 24. }, Point { x, y: y + 24. }]).unwrap());
        document.layers[0].mask = Some(mask.clone());
        document.layers[0].opacity = 0.7;
        let before = document.clone();
        let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Nearest, Default::default()).unwrap();
        assert_eq!(plan.output.properties.offset, Point { x: -80., y: -40. });
        assert_eq!(plan.output.properties.extent, Some([592, 552]));
        let cancelled = crate::snapshot::CaptureControl::default();
        cancelled.cancel();
        let cancellation = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan.clone(), cancelled)).unwrap_err();
        assert!(cancellation.contains("cancel"), "{cancellation}");
        assert_eq!(document, before);
        let control = crate::snapshot::CaptureControl::with_allocation_tracking();
        let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, control.clone())).unwrap();
        assert!(control.allocation_peaks().unwrap().observations > 0);
        assert_eq!(baked.id, document.active_layer);
        assert_eq!(baked.properties.placement, layer_core::LayerPlacement::IDENTITY);
        assert_eq!(baked.opacity, before.layers[0].opacity);
        assert!(baked.source.is_none());
        assert_eq!(baked.raster.wait_data().unwrap().watercolor, before.layers[0].raster.wait_data().unwrap().watercolor);
        let inverse = map.inverse().unwrap();
        assert_baked_codes(&baked, &[RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness], |plane, point| {
            let source = inverse.map(point);
            field(plane, source.x.floor() + 0.5, source.y.floor() + 0.5)
        });
        if linked {
            let baked_mask = baked.mask.as_ref().unwrap();
            assert!(baked_mask.initial.is_none());
            assert_eq!(baked_mask.placement, layer_core::Projective::IDENTITY);
            assert_eq!(baked_mask.offset, baked.properties.offset);
            assert_eq!(baked_mask.inverted, mask.inverted);
            assert!(!baked_mask.raster.is_empty());
        } else {
            assert_eq!(baked.mask, Some(mask));
        }
        let preview = render(&mut renderer, &before.layers[0], &[], true);
        let actual = render(&mut renderer, &baked, &[], true);
        assert_pixels(&actual, &preview, "raw material bake appearance and mask parity");
        assert_eq!(document, before);
    }
}


#[test]
fn boundary_material_bake_and_visible_bounds_match_independent_expanded_capture() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut failures = Vec::new();
    for (size, map) in [(128, Affine([1., 0., 0., 1., 128., 0.])),
        (512, Affine([0.5, 0., 0., 0.5, 0., 0.]))] {
        let mut data = RasterData {
            watercolor: Some(RasterWatercolor { wet_edge: 0.9, burnt_edge: 0.6, edge_width: 16. }),
            ..Default::default()
        };
        let coordinate = [(size - 1) / PAGE_SIZE, 0];
        for plane in [RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
            let mut bytes = Vec::new();
            for y in 0..PAGE_SIZE {
                for x in 0..PAGE_SIZE {
                    let world_x = coordinate[0] * PAGE_SIZE + x;
                    let inside = (size - 16..size).contains(&world_x)
                        && (40..56).contains(&y);
                    bytes.extend(match plane {
                        RasterPlane::Color => if inside { vec![76, 38, 19, 128] } else { vec![0; 4] },
                        RasterPlane::Wetness => vec![if inside { 127 } else { 0 }],
                        RasterPlane::WatercolorWetness => vec![if inside { 255 } else { 0 }],
                        RasterPlane::Mask => unreachable!(),
                    });
                }
            }
            data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(
                TileBlob::encode(plane.descriptor(Default::default()), &bytes).unwrap()));
        }
        let mut document = layer_core::Document::new("boundary watercolor", size, size,
            layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.layers = vec![layer(data, map)];
        document.active_layer = document.layers[0].id;
        let request = layer_core::ContentBoundsRequest::new(&document, layer_core::ContentScope::Target(document.active_layer));
        let raw = pollster::block_on(renderer.snapshot_gpu().content_bounds(request, Default::default())).unwrap();
        assert_eq!(raw, Rect { min: Point { x: (size - 16) as f32, y: 40. }, max: Point { x: size as f32, y: 56. } },
            "material appearance support must not change raw Target bounds");
        let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Nearest, Default::default()).unwrap();
        if size == 128 {
            assert_eq!(plan.output.properties.offset, Point::default());
            assert_eq!(plan.output.properties.extent, Some([256, 128]));
        }
        let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, Default::default())).unwrap();
        let capture = |layer: &Layer| {
            let mut expanded = document.clone();
            expanded.width = 640;
            expanded.height = 640;
            let mut shifted = layer.clone();
            shifted.properties.offset.x += 64.;
            shifted.properties.offset.y += 64.;
            expanded.layers = vec![shifted];
            let mut snapshot = renderer.snapshot_gpu().capture(layer_core::Project { document: expanded },
                [0.; 4], 0., Default::default()).unwrap();
            snapshot.read_region([0, 0, 640, 640]).unwrap()
        };
        let original = capture(&document.layers[0]);
        let actual = capture(&baked);
        let error = original.iter().zip(&actual).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
            .fold(0f32, f32::max);
        assert!(error < 1e-6, "{size}: expanded retained/baked material at the mapped domain edge: {error}");
        let alpha_bounds = original.iter().enumerate().filter(|(_, pixel)| pixel[3] > 0.)
            .fold(Rect::EMPTY, |bounds, (i, _)| {
                let x = (i % 640) as f32 - 64.;
                let y = (i / 640) as f32 - 64.;
                bounds.union(Rect { min: Point { x, y }, max: Point { x: x + 1., y: y + 1. } })
            });
        assert!(alpha_bounds.max.x > 256., "independent capture must include a destination halo: {alpha_bounds:?}");
        for (name, target) in [("retained", document.layers[0].clone()), ("baked", baked)] {
            let mut measured = document.clone();
            measured.layers = vec![target];
            for scope in [layer_core::ContentScope::Visible, layer_core::ContentScope::All] {
                let request = layer_core::ContentBoundsRequest::new(&measured, scope);
                let bounds = pollster::block_on(renderer.snapshot_gpu().content_bounds(request, Default::default())).unwrap();
                if bounds != alpha_bounds { failures.push(format!("{size} {name} {scope:?}: {bounds:?} != {alpha_bounds:?}")); }
            }
        }
    }
    assert!(failures.is_empty(), "Visible material bounds: {}", failures.join("; "));
}

#[test]
fn snapshot_photo_bake_keeps_original_source_and_honors_erased_base_overrides() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = layer_core::Document::new("erased photo bake", EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.layers.retain(|layer| layer.kind == layer_core::LayerKind::Paint);
    let source = layer_core::color::source::rgba8_source([512, 256], |_, _| [20, 180, 80, 255]);
    let original = &mut document.layers[0];
    original.source = Some(source.clone());
    original.properties.placement = layer_core::LayerPlacement::from_affine(Affine([0.5, 0., 0., 0.5, -20., 30.]));
    original.raster = RasterRevision::backed(RasterData {
        tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] },
            RasterTile::backed(TileBlob::encode(document.color.paint_descriptor(), &vec![0; 256 * 256 * 4]).unwrap()))].into(),
        ..Default::default()
    });
    let before = document.clone();
    let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Nearest, Default::default()).unwrap();
    let inverse = before.layers[0].properties.placement.as_affine().unwrap().inverse().unwrap();
    let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, Default::default())).unwrap();
    assert_baked_codes(&baked, &[RasterPlane::Color], |plane, point| {
        let local = inverse.map(point);
        if plane == RasterPlane::Color && (256. ..512.).contains(&local.x) && (0. ..256.).contains(&local.y) {
            vec![20, 180, 80, 255]
        } else { vec![0; if plane == RasterPlane::Color { 4 } else { 1 }] }
    });
    let preview = render(&mut renderer, &before.layers[0], &[], true);
    let actual = render(&mut renderer, &baked, &[], true);
    assert_pixels(&actual, &preview, "source plus erased override bake parity");
    assert!(baked.source.is_none());
    assert_eq!(document, before);
    assert!(Arc::ptr_eq(document.layers[0].source.as_ref().unwrap(), &source));
}

#[test]
fn snapshot_magnified_nonuniform_photo_bake_matches_retained_bicubic_edges() {
    let mut document = layer_core::Document::new("magnified checker", EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.color.depth = layer_core::color::SampleDepth::F32;
    document.layers.retain(|layer| layer.kind == layer_core::LayerKind::Paint);
    let original = &mut document.layers[0];
    original.source = Some(layer_core::color::source::rgba8_source([128; 2], |x, y| {
        if x < 3 || y < 3 || x >= 125 || y >= 125 { [0; 4] }
        else if (x / 4 + y / 4) % 2 == 0 { [220, 28, 90, 255] }
        else { [24, 170, 210, 128] }
    }));
    original.properties.placement = layer_core::LayerPlacement { interpolation:Interpolation::Bicubic, ..LayerPlacement::from_affine(Affine([1.6, 0.1, 0., 0.8, 19.2, 23.4])) };
    let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Bicubic, Default::default()).unwrap();
    let mut renderer = WgpuRasterizer::new_native_headless(document.color).unwrap();
    renderer.test.reference = true;
    let expected = render(&mut renderer, &document.layers[0], &[], true);
    let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, Default::default())).unwrap();
    let actual = render(&mut renderer, &baked, &[], true);
    assert_pixels(&actual, &expected, "magnified nonuniform checker: retained and baked Bicubic appearance");
    assert!(baked.source.is_none());
}

#[test]
fn snapshot_bake_rejects_corrupt_native_tile_after_metadata_admission() {
    let mut document = layer_core::Document::new("corrupt backing", EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.layers.retain(|layer| layer.kind == layer_core::LayerKind::Paint);
    let mut data = fixture(Affine::IDENTITY, 4.);
    let key = TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [1, 0] };
    let mut blob = TileBlob::encode(key.plane.descriptor(document.color), &vec![180; 256 * 256]).unwrap();
    blob.digest[0] ^= 1;
    data.tiles.insert(key, RasterTile::backed(blob));
    document.layers[0].raster = RasterRevision::backed(data);
    document.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(maps()[0]);
    let before = document.clone();
    let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Nearest, Default::default()).unwrap();
    plan.input.validate(Default::default()).unwrap();
    let renderer = WgpuRasterizer::new_native_headless(document.color).unwrap();
    let result = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, Default::default()));
    assert!(result.is_err(), "native decode must reject corrupted backing admitted by metadata validation");
    assert_eq!(document, before);
}

#[test]
fn snapshot_bake_enlargement_preserves_mask_defaults_and_finite_native_overrides() {
    let mut document = layer_core::Document::new("finite mask domain", EXTENT[0], EXTENT[1], layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.layers.retain(|layer| layer.kind == layer_core::LayerKind::Paint);
    let mask_id = document.allocate_layer_id();
    let map = Affine([0.5, 0., 0., 1., -80., -40.]);
    document.layers[0].properties.placement = layer_core::LayerPlacement::from_affine(map);
    document.layers[0].source = Some(layer_core::color::source::rgba8_source(EXTENT, |_, _| [80, 120, 160, 255]));
    let mut mask = LayerMask::reveal_all(mask_id, Point::default());
    mask.initial = Some(Selection::polygon(vec![
        Point { x: 0., y: 500. }, Point { x: 512., y: 500. },
        Point { x: 512., y: 550. }, Point { x: 0., y: 550. },
    ]).unwrap());
    mask.raster = RasterRevision::backed(RasterData { tiles: [[0, 1], [1, 1]].map(|coordinate|
        (TileKey { plane: RasterPlane::Mask, coordinate }, RasterTile::backed(
            TileBlob::encode(RasterPlane::Mask.descriptor(document.color), &vec![0; 256 * 256]).unwrap()))).into(),
        ..Default::default() });
    document.layers[0].mask = Some(mask);
    let plan = document.transform_pixels_plan(document.active_layer, Interpolation::Nearest, Default::default()).unwrap();
    assert_eq!(plan.input.document.layers[0].properties.extent, Some(EXTENT));
    assert_eq!([plan.input.document.width, plan.input.document.height], [592, 552]);
    let renderer = WgpuRasterizer::new_native_headless(document.color).unwrap();
    let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan, Default::default())).unwrap();
    let mask = baked.mask.as_ref().unwrap();
    let inverse = map.inverse().unwrap();
    assert_baked_codes(&baked, &[RasterPlane::Color], |plane, world| {
        assert_eq!(plane, RasterPlane::Color);
        let local = inverse.map(world);
        if (0. ..512.).contains(&local.x) && (0. ..512.).contains(&local.y) { vec![80, 120, 160, 255] }
        else { vec![0; 4] }
    });
    let mut covered = 0;
    let mut zero_overrides = 0;
    for (key, tile) in &mask.raster.wait_data().unwrap().tiles {
        assert_eq!(key.plane, RasterPlane::Mask);
        let bytes = tile.wait_backing().unwrap().decode().unwrap();
        zero_overrides += usize::from(bytes.iter().all(|value| *value == 0));
        for (i, value) in bytes.into_iter().enumerate() {
            let world = Point {
                x: (key.coordinate[0] * PAGE_SIZE + i as u32 % PAGE_SIZE) as f32 + 0.5 + mask.offset.x,
                y: (key.coordinate[1] * PAGE_SIZE + i as u32 / PAGE_SIZE) as f32 + 0.5 + mask.offset.y,
            };
            let local = inverse.map(world);
            let expected = if (0. ..512.).contains(&local.x) && (256. ..512.).contains(&local.y) { 0 } else { 255 };
            assert_eq!(value, expected, "raw default mask at {world:?}, original local {local:?}");
            covered += usize::from(value != 0);
        }
    }
    assert!(covered > 0);
    assert!(zero_overrides > 0, "default-one mask must retain explicit empty native overrides");
}

#[test]
fn moving_watercolor_crosses_identity_without_rebuilding_the_photo_source() {
    let extent = [2048; 2];
    let photo_pixel = |x: u32, y: u32| [40 + (x % 160) as u8, 30 + (y % 180) as u8, 80, 255];
    let mut original = layer(fixture(Affine::IDENTITY, 16.), Affine::IDENTITY);
    original.source = Some(layer_core::color::source::rgba8_source(extent, photo_pixel));
    let overrides: Vec<_> = original.raster.wait_data().unwrap().tiles.keys()
        .filter(|key| key.plane == RasterPlane::Color).map(|key| key.coordinate).collect();
    let mut moving = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut oracle = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    moving.prepare_moving_layer(Some(original.id));
    oracle.prepare_moving_layer(Some(original.id));
    let mut prepared = None;
    for x in [0., 16., 0.] {
        original.properties.placement = layer_core::LayerPlacement::from_affine(Affine::translation(Point { x, y: 0. }));
        let mut expected = layer(fixture(original.properties.placement.as_affine().unwrap(), 16.), Affine::IDENTITY);
        expected.source = Some(layer_core::color::source::rgba8_source(extent, |px, py| {
            if px < x as u32 { return [0; 4]; }
            let px = px - x as u32;
            if overrides.contains(&[px / PAGE_SIZE, py / PAGE_SIZE]) { [0; 4] } else { photo_pixel(px, py) }
        }));
        let frame = |layers| FramePacket {
            blend_space: layer_core::BlendSpace::Perceptual,
            view: ViewState { width_px: 640, height_px: 480,
                document_to_surface: [0.125, 0., 0., 0.125, 64., 48.], ..packet(layers, extent).view },
            ..packet(layers, extent)
        };
        moving.submit(frame(std::slice::from_ref(&original))).unwrap();
        let (texture, updates, level) = moving.scene.as_ref().unwrap().placement_cache(original.id).unwrap();
        if let Some((first_texture, previous_updates, first_level)) = &mut prepared {
            assert_eq!(&texture, first_texture, "crossing identity keeps the raw photo LOD allocation");
            assert_eq!(level, *first_level);
            assert!(updates - *previous_updates <= 16,
                "only the sparse wet region may update: {} source pages", updates - *previous_updates);
            *previous_updates = updates;
        } else {
            assert!(updates >= 64, "the fixture must prepare a complete 64-page photo source");
            prepared = Some((texture, updates, level));
        }
        oracle.submit(FramePacket { reset_layers: true, ..frame(std::slice::from_ref(&expected)) }).unwrap();
        let actual = crate::scene::scale::tests::display_pixels(&moving);
        let expected = crate::scene::scale::tests::display_pixels(&oracle);
        assert_eq!(actual.len(), expected.len());
        let witness = actual.iter().zip(&expected).enumerate().find(|(_, (a, b))|
            a.iter().zip(*b).any(|(a, b)| (a - b).abs() >= 1e-6));
        let error = actual.iter().zip(&expected).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
            .fold(0f32, f32::max);
        assert!(error < 1e-6, "CPU-shifted raw photo/material oracle at x={x}: {error}, witness={witness:?}, plan={:?}", moving.scale_display.as_ref().unwrap().plan);
    }
    moving.prepare_moving_layer(None);
    oracle.prepare_moving_layer(None);
    let mut frame = packet(std::slice::from_ref(&original), extent);
    frame.blend_space = layer_core::BlendSpace::Perceptual;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 64., 48.];
    moving.submit(frame).unwrap();
    oracle.submit(FramePacket { reset_layers: true, ..frame }).unwrap();
    assert_eq!(crate::scene::scale::tests::display_pixels(&moving),
        crate::scene::scale::tests::display_pixels(&oracle), "Cancel restores the ordinary identity rendering");
}

#[test]
fn accepted_photo_keeps_raw_lod_for_sparse_watercolor_and_transform_reopen() {
    for initial in [Affine::IDENTITY, Affine([0.5, 0., 0., 0.5, 0., 0.])] {
    let extent = [2048; 2];
    let mut photo = layer(RasterData::default(), initial);
    photo.source = Some(layer_core::color::source::rgba8_source(extent, |x, y|
        [40 + (x % 160) as u8, 30 + (y % 180) as u8, 80, 255]));
    let mut cached = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let submit = |r: &mut WgpuRasterizer, layer: &Layer| {
        let mut frame = packet(std::slice::from_ref(layer), extent);
        frame.blend_space = layer_core::BlendSpace::Perceptual;
        frame.view.width_px = 640;
        frame.view.height_px = 480;
        frame.view.document_to_surface = [0.125, 0., 0., 0.125, 64., 48.];
        r.submit(frame).unwrap();
    };
    let parity = |r: &WgpuRasterizer, layer: &Layer, moving: bool| {
        let mut fresh = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        fresh.prepare_moving_layer(moving.then_some(layer.id));
        submit(&mut fresh, layer);
        let actual = crate::scene::scale::tests::display_pixels(r);
        let expected = crate::scene::scale::tests::display_pixels(&fresh);
        assert_eq!(actual.len(), expected.len());
        let error = actual.iter().zip(&expected).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
            .fold(0f32, f32::max);
        assert!(error < 1e-6, "cached versus fresh photo/material appearance: {error}");
    };
    cached.prepare_moving_layer(Some(photo.id));
    submit(&mut cached, &photo);
    let raw_level = if initial == Affine::IDENTITY { 2 } else {
        cached.scene.as_ref().unwrap().placement_cache(photo.id).unwrap().2
    };
    let (raw_texture, _, _) = cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap();
    eprintln!("{initial:?} initial updates {}", cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap().1);
    let raw_pixels = crate::test_support::float_pixels(&cached, &raw_texture);
    cached.prepare_moving_layer(None);
    photo.properties.placement = layer_core::LayerPlacement::from_affine(Affine::IDENTITY);
    submit(&mut cached, &photo);
    let (accepted_texture, _, accepted_level) = cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap();
    assert_eq!(accepted_level, raw_level);
    assert_eq!(accepted_texture, raw_texture);
    assert_eq!(crate::test_support::float_pixels(&cached, &raw_texture), raw_pixels,
        "accepting a photo must preserve its prepared raw finer pixels");
    parity(&cached, &photo, false);
    photo.raster = RasterRevision::backed(fixture(Affine::IDENTITY, 16.));
    submit(&mut cached, &photo);
    parity(&cached, &photo, false);
    eprintln!("{initial:?} wet restored updates {}", cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap().1);
    let (_, before_reopen, _) = cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap();
    cached.prepare_moving_layer(Some(photo.id));
    submit(&mut cached, &photo);
    let (reopened_texture, updates, level) = cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap();
    eprintln!("{initial:?} reopened updates {}", cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap().1);
    assert_eq!(reopened_texture, raw_texture);
    assert_eq!(level, raw_level);
    assert!(updates - before_reopen <= 16,
        "reopening Transform repairs sparse dirty pages, not the 64-page photo: {}", updates - before_reopen);
    parity(&cached, &photo, true);
    cached.prepare_moving_layer(None);
    submit(&mut cached, &photo);
    parity(&cached, &photo, false);
    eprintln!("{initial:?} wet accepted updates {}", cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap().1);
    let wet = photo.raster.clone();
    let mut dry = (*wet.wait_data().unwrap()).clone();
    for (key, tile) in &mut dry.tiles {
        if key.plane != RasterPlane::WatercolorWetness { continue; }
        let blob = tile.wait_backing().unwrap();
        let mut bytes = blob.decode().unwrap();
        bytes.fill(0);
        *tile = RasterTile::backed(TileBlob::encode(blob.descriptor, &bytes).unwrap());
    }
    photo.raster = RasterRevision::backed(dry);
    submit(&mut cached, &photo);
    eprintln!("accepted {initial:?}: isolated cold WatercolorWetness removal with unchanged pigment/style");
    parity(&cached, &photo, false);
    eprintln!("{initial:?} dry accepted updates {}", cached.scene.as_ref().unwrap().placement_cache_at(photo.id, raw_level).unwrap().1);
    photo.raster = wet;
    submit(&mut cached, &photo);
    parity(&cached, &photo, false);
    cached.prepare_moving_layer(Some(photo.id));
    submit(&mut cached, &photo);
    parity(&cached, &photo, true);
    let mut changed_style = fixture(Affine::IDENTITY, 4.);
    photo.raster = RasterRevision::backed(changed_style.clone());
    submit(&mut cached, &photo);
    parity(&cached, &photo, true);
    let key = TileKey { plane: RasterPlane::Color, coordinate: [0, 0] };
    let blob = changed_style.tiles[&key].wait_backing().unwrap();
    let mut bytes = blob.decode().unwrap();
    for pixel in bytes.chunks_exact_mut(4).filter(|pixel| pixel[3] != 0) { pixel[..3].copy_from_slice(&[20, 60, 100]); }
    changed_style.tiles.insert(key, RasterTile::backed(TileBlob::encode(blob.descriptor, &bytes).unwrap()));
    photo.raster = RasterRevision::backed(changed_style);
    submit(&mut cached, &photo);
    parity(&cached, &photo, true);
    photo.source = Some(layer_core::color::source::rgba8_source(extent, |x, y|
        [80, 50 + (x % 100) as u8, 30 + (y % 120) as u8, 255]));
    submit(&mut cached, &photo);
    parity(&cached, &photo, true);
    let mut appearance = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut frame = packet(std::slice::from_ref(&photo), extent);
    frame.blend_space = layer_core::BlendSpace::Linear;
    frame.view.document_to_surface = [0.125, 0., 0., 0.125, 64., 48.];
    appearance.submit(frame).unwrap();
    let (_, _, appearance_level) = appearance.scene.as_ref().unwrap().placement_cache(photo.id).unwrap();
    assert!(appearance.scene.as_ref().unwrap().reduced_layer(&appearance, &photo, extent, appearance_level).is_none(),
        "linear watercolor appearance must never become the transform's original raw pigment");
    appearance.prepare_moving_layer(Some(photo.id));
    appearance.submit(frame).unwrap();
    let (_, _, pigment_level) = appearance.scene.as_ref().unwrap().placement_cache(photo.id).unwrap();
    assert!(appearance.scene.as_ref().unwrap().reduced_layer(&appearance, &photo, extent, pigment_level).is_some(),
        "a complete raw pigment LOD remains eligible for reduced transforms");
    }
}

#[derive(Debug, PartialEq)]
struct RawTexel([Vec<u8>; 3]);

const PLANES: [RasterPlane; 3] = [RasterPlane::Color, RasterPlane::Wetness, RasterPlane::WatercolorWetness];

fn raw_texel(planes: &Planes, sizes: [usize; 3], at: [i32; 2]) -> RawTexel {
    RawTexel(std::array::from_fn(|i| {
        if at.iter().any(|v| *v < 0) { return vec![0; sizes[i]]; }
        let coordinate = at.map(|v| v as u32 / PAGE_SIZE);
        let key = TileKey { plane: PLANES[i], coordinate };
        let pixel = (at[1] as u32 % PAGE_SIZE * PAGE_SIZE + at[0] as u32 % PAGE_SIZE) as usize;
        planes.get(&key).map_or_else(|| vec![0; sizes[i]], |p| p[pixel * sizes[i]..][..sizes[i]].to_vec())
    }))
}

fn plane_sizes(planes: &Planes) -> [usize; 3] {
    PLANES.map(|plane| planes.iter().find(|(k, _)| k.plane == plane).unwrap().1.len()
        / (PAGE_SIZE * PAGE_SIZE) as usize)
}

fn nonlinear_maps() -> Vec<(&'static str, LayerPlacement)> {
    let domain = Rect { min: Point { x: 224., y: 112. }, max: Point { x: 280., y: 168. } };
    let affine = Affine([0.55, 0.07, 0.03, 0.8, 113.137, 125.263]);
    let base = MeshMap::from_affine(domain, [2, 2], affine).unwrap();
    vec![
        ("perspective", LayerPlacement::from_projective(Projective([
            0.8, 0.02, 117., 0.02, 0.8, 204., 0.0009, 0.0002, 1.,
        ]))),
        ("nonuniform warp", layer_core::LayerPlacement { mesh: Some(Arc::new(base.move_node(4, Point { x: 7., y: -6. }).unwrap())), ..Default::default() }),
        ("folded warp", layer_core::LayerPlacement { mesh: Some(Arc::new(base.move_node(4, Point { x: 40., y: -20. }).unwrap())), ..Default::default() }),
        ("outer perspective on folded warp", layer_core::LayerPlacement {
            outer: Projective([1.05,0.02,-16.,-0.02,1.04,-6.,0.00015,-0.0001,1.]),
            mesh: Some(Arc::new(base.move_node(4, Point { x:40., y:-20. }).unwrap())),
            ..Default::default()
        }),
    ]
}

type SourcePositions = Vec<Vec<Option<[f64; 2]>>>;

fn source_positions(map: &LayerPlacement) -> SourcePositions {
    let size = (EXTENT[0] * EXTENT[1]) as usize;
    let mut positions = vec![vec![None]; size];
    if let Some(h) = map.projective() {
        for y in 0..EXTENT[1] { for x in 0..EXTENT[0] {
            let p = crate::test_support::preimage(h.0.map(f64::from), [x as f64 + 0.5, y as f64 + 0.5]);
            let i = (y * EXTENT[0] + x) as usize;
            positions[i] = vec![p];
        }}
        return positions;
    }
    let mesh = map.mesh.as_ref().unwrap();
    let geometry = crate::paint_transform::mesh::MeshGeometry::new(mesh, map.outer, None);
    let inverse_outer = Projective::invert(map.outer.0.map(f64::from)).unwrap();
    for t in geometry.triangles() {
        let [a, b, c] = t.map(|i| geometry.vertices[i as usize].map(f64::from));
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 { continue; }
        let low = [0, 1].map(|k| a[k].min(b[k]).min(c[k]).floor().max(0.) as u32);
        let high = [0, 1].map(|k| (a[k].max(b[k]).max(c[k]).ceil().max(0.) as u32).min(EXTENT[k]));
        for y in low[1]..high[1] { for x in low[0]..high[0] {
            let p = [x as f64 + 0.5, y as f64 + 0.5];
            let weight = |u: [f64; 4], v: [f64; 4]| {
                ((u[0] - p[0]) * (v[1] - p[1]) - (u[1] - p[1]) * (v[0] - p[0])) / area
            };
            let l = [weight(b, c), weight(c, a), weight(a, b)];
            let edges = [(b, c), (c, a), (a, b)].map(|(u, v)| (u[0] - v[0]).hypot(u[1] - v[1]));
            let nearest = (0..3).map(|i| l[i] * area.abs() / edges[i]).fold(f64::INFINITY, f64::min);
            let homogeneous = [a,b,c].map(|v| inverse_outer[6]*v[0]+inverse_outer[7]*v[1]+inverse_outer[8]);
            let denominator = (0..3).map(|n| l[n]*homogeneous[n]).sum::<f64>();
            let source = [2,3].map(|k| (l[0]*homogeneous[0]*a[k]+l[1]*homogeneous[1]*b[k]+l[2]*homogeneous[2]*c[k])/denominator);
            let i = (y * EXTENT[0] + x) as usize;
            if nearest > 0.02 { positions[i].clear(); }
            if nearest > -0.02 { positions[i].insert(0,Some(source)); }
        }}
    }
    positions
}

fn nearest_candidates(positions: &SourcePositions, i: usize) -> Vec<[i32; 2]> {
    let mut candidates = Vec::new();
    for position in &positions[i] {
        match *position {
            None => if !candidates.contains(&[-1, -1]) { candidates.push([-1, -1]); },
            Some([x, y]) => {
                let center = [x.floor() as i32,y.floor() as i32];
                if !candidates.contains(&center) { candidates.push(center); }
                for sx in [(x - 0.005).floor() as i32, (x + 0.005).floor() as i32] {
                    for sy in [(y - 0.005).floor() as i32, (y + 0.005).floor() as i32] {
                        if !candidates.contains(&[sx, sy]) { candidates.push([sx, sy]); }
                    }
                }
            }
        }
    }
    candidates
}

fn destination_fixture(samples: &[[i32; 2]], width: f32, source: &RasterData) -> RasterData {
    let mut data = RasterData {
        watercolor: Some(RasterWatercolor { wet_edge: 0.9, burnt_edge: 0.6, edge_width: width }),
        ..Default::default()
    };
    for plane in PLANES {
        for coordinate in [[0, 0], [1, 0], [0, 1], [1, 1]] {
            let mut bytes = Vec::new();
            for y in 0..PAGE_SIZE { for x in 0..PAGE_SIZE {
                let [sx, sy] = samples[((coordinate[1] * PAGE_SIZE + y) * EXTENT[0] + coordinate[0] * PAGE_SIZE + x) as usize];
                let present = sx >= 0 && sy >= 0 && source.tiles.contains_key(&TileKey {
                    plane, coordinate: [sx as u32 / PAGE_SIZE, sy as u32 / PAGE_SIZE],
                });
                bytes.extend(if present { field(plane, sx as f32 + 0.5, sy as f32 + 0.5) }
                    else { vec![0; if plane == RasterPlane::Color { 4 } else { 1 }] });
            }}
            if bytes.iter().any(|b| *b != 0) {
                data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed(
                    TileBlob::encode(plane.descriptor(Default::default()), &bytes).unwrap()));
            }
        }
    }
    data
}

fn check_raw_mapping(source: &Planes, actual: &Planes, positions: &SourcePositions, label: &str) -> Vec<[i32; 2]> {
    let sizes = plane_sizes(source);
    let mut samples = Vec::with_capacity((EXTENT[0] * EXTENT[1]) as usize);
    let mut boundary_choices = 0;
    let mut pigment = 0;
    for y in 0..EXTENT[1] { for x in 0..EXTENT[0] {
        let i = (y * EXTENT[0] + x) as usize;
        let value = raw_texel(actual, sizes, [x as i32, y as i32]);
        let choices = nearest_candidates(positions, i);
        let accepted = choices.iter().position(|p| raw_texel(source, sizes, *p) == value)
            .unwrap_or_else(|| panic!("{label} at {x},{y}: {value:?}, source candidates {choices:?}"));
        boundary_choices += usize::from(accepted != 0);
        pigment += usize::from(value.0[0].iter().any(|v| *v != 0));
        samples.push(choices[accepted]);
    }}
    assert!(pigment > 250, "{label}: enough mapped wet pigment: {pigment}");
    assert!(boundary_choices < 128, "{label}: Float32 boundary choices remain bounded: {boundary_choices}");
    println!("{label}: {pigment} pigment pixels, {boundary_choices} alternate nearest-boundary samples");
    samples
}

fn assert_destination_halo(pixels: &[u8], samples: &[[i32; 2]], width: f32, label: &str) {
    let pigment: Vec<_> = samples.iter().enumerate().filter(|(_, p)| field(RasterPlane::Color, p[0] as f32 + 0.5, p[1] as f32 + 0.5)[3] != 0)
        .map(|(i, _)| [i as u32 % EXTENT[0], i as u32 / EXTENT[0]]).collect();
    let low = [0, 1].map(|axis| pigment.iter().map(|p| p[axis]).min().unwrap());
    let high = [0, 1].map(|axis| pigment.iter().map(|p| p[axis]).max().unwrap());
    let radius = (width.clamp(1., 16.) * 2.).ceil() as u32;
    let mut halo = 0;
    for (i, _) in pixels.chunks_exact(4).enumerate().filter(|(_, p)| p[3] != 0) {
        let p = [i as u32 % EXTENT[0], i as u32 / EXTENT[0]];
        assert!((0..2).all(|axis| p[axis] >= low[axis].saturating_sub(radius) && p[axis] <= high[axis] + radius),
            "{label}: material at {p:?} exceeds {radius}px destination halo of {low:?}..{high:?}");
        halo += usize::from(!pigment.contains(&p));
    }
    assert!(halo > 0, "{label}: the fixture exercises a material halo");
    assert!(low[0] < PAGE_SIZE && high[0] >= PAGE_SIZE, "{label}: pigment crosses x tile seam");
    assert!(low[1] < PAGE_SIZE && high[1] >= PAGE_SIZE, "{label}: pigment crosses y tile seam");
    println!("{label}: {halo} material-only pixels within {radius}px destination halo");
}

#[test]
fn nonlinear_material_transform_maps_raw_planes_with_preview_commit_and_destination_style_parity() {
    let mut transformed = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut expected = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    for (label, map) in nonlinear_maps() {
        let positions = source_positions(&map);
        for (width, missing) in [(1., None), (4., None), (16., None),
            (4., Some(RasterPlane::Wetness)), (4., Some(RasterPlane::WatercolorWetness))] {
            let mut data = fixture(Affine::IDENTITY, width);
            if let Some(plane) = missing { data.tiles.remove(&TileKey { plane, coordinate: [1, 0] }); }
            let context = format!("{label} width={width} missing={missing:?}");
            let mut source = layer(data.clone(), Affine::IDENTITY);
            let immutable = source.raster.clone();
            let digests: Vec<_> = immutable.wait_data().unwrap().tiles.values().map(|t| t.wait_backing().unwrap().digest).collect();
            render(&mut transformed, &source, &[], true);
            let source_planes = live_planes(&transformed);
            let transform = ImageTransform { placement: layer_core::LayerPlacement { interpolation: Interpolation::Nearest, ..map.clone() }, ..Default::default() };
            transformed.set_transform_preview(Some(&layer_render::TransformPreview {
                transaction: 1, layer: source.id, moving: false, selection: None, transform: transform.clone(),
            })).unwrap();
            let preview = render(&mut transformed, &source, &[], false);
            let mut retained = source.clone();
            retained.properties.placement = transform.placement.clone();
            let retained_pixels = render(&mut expected, &retained, &[], true);
            assert_pixels(&retained_pixels, &preview, &format!("retained {context}"));
            transformed.set_transform_preview(None).unwrap();
            render(&mut transformed, &source, &[], false);
            let mut coverage = LayerMask::reveal_all(LayerId(9), Point::default());
            coverage.default_coverage = 1.;
            let operation = LayerOperation { placement: Affine::IDENTITY, coverage,
                kind: LayerOperationKind::Transform(transform) };
            let operation_batch = DabBatch { kind: DabBatchKind::LayerOperation(0), dab_count: 0,
                damage: operation.bounds(EXTENT), ..batch(1) };
            source.pending_operations.push(operation);
            let actual = render(&mut transformed, &source, &[operation_batch], false);
            let samples = check_raw_mapping(&source_planes, &live_planes(&transformed), &positions, &context);
            let destination = layer(destination_fixture(&samples, width, &data), Affine::IDENTITY);
            let expected_pixels = render(&mut expected, &destination, &[], true);
            assert_eq!(transformed.paint_layers[0].watercolor, expected.paint_layers[0].watercolor, "{label} width={width}");
            assert_pixels(&actual, &expected_pixels, &format!("{label} width={width}: map raw planes, then evaluate style once"));
            assert_pixels(&preview, &actual, &format!("{label} width={width}: still preview and commit agree"));
            assert_destination_halo(&actual, &samples, width, &context);
            assert_eq!(source.raster.identity(), immutable.identity());
            assert_eq!(immutable.wait_data().unwrap().tiles.values().map(|t| t.wait_backing().unwrap().digest).collect::<Vec<_>>(), digests);
        }
    }
}

#[test]
fn nonlinear_material_source_footprints_cover_destination_neighbors_with_portable_binding_limits() {
    use crate::paint_transform::{mesh::MeshGeometry, snapshot::Splitter};
    use crate::pixel_rect::PixelRect;
    let source_extent = [8192; 2];
    let mesh = MeshMap::from_affine(Rect::from_extent(source_extent), [2, 2],
        Affine([0.04, 0., 0., 0.04, 100.137, 120.263])).unwrap()
        .move_node(4, Point { x: 40., y: -20. }).unwrap();
    let maps = [
        ("minified perspective", LayerPlacement::from_projective(Projective([
            0.04, 0.0008, 100., 0.0008, 0.04, 120., 0.00001, 0.000008, 1.,
        ]))),
        ("minified warp", layer_core::LayerPlacement { mesh: Some(Arc::new(mesh)), ..Default::default() }),
    ];
    for (label, map) in maps {
        let positions = source_positions(&map);
        let geometry = map.mesh.as_ref().map(|mesh| Arc::new(MeshGeometry::new(mesh, map.outer, None)));
        let slots = 16 - usize::from(geometry.is_some());
        for interpolation in [Interpolation::Nearest, Interpolation::Linear, Interpolation::Bicubic, Interpolation::Lanczos] {
            let transform = ImageTransform { placement: layer_core::LayerPlacement { interpolation: interpolation, ..map.clone() }, ..Default::default() };
            let splitter = Splitter::new(PixelRect::full(source_extent), &transform, geometry.clone(), |c| c[0] < 32 && c[1] < 32).unwrap();
            let mut jobs = Vec::new();
            splitter.split(PixelRect::new(192, 192, 320, 320), &mut jobs).unwrap();
            assert!(jobs.len() > 1 && jobs.len() <= 256, "{label} {interpolation:?}: a wide source footprint splits into bounded regions");
            println!("{label} {interpolation:?}: {} bounded source neighborhoods, {slots} bindings each", jobs.len());
            for job in jobs {
                assert!(job.sources.len() <= slots, "{label} {interpolation:?}: portable texture-binding limit");
                for y in job.region.min_y()..job.region.max_y() { for x in job.region.min_x()..job.region.max_x() {
                    let i = (y * EXTENT[0] + x) as usize;
                    let mut sampled: Vec<_> = positions[i].iter().filter_map(|p| *p).collect();
                    if interpolation != Interpolation::Nearest && let Some(h) = map.projective() {
                        for dy in [0.01, 0.99] { for dx in [0.01, 0.99] {
                            sampled.extend(crate::test_support::preimage(h.0.map(f64::from), [x as f64 + dx, y as f64 + dy]));
                        }}
                    }
                    for source in sampled {
                        let support = interpolation.support().max(1) as i32;
                        for sy in (source[1].floor() as i32 - support)..=(source[1].floor() as i32 + support) {
                            for sx in (source[0].floor() as i32 - support)..=(source[0].floor() as i32 + support) {
                                if (0..source_extent[0] as i32).contains(&sx) && (0..source_extent[1] as i32).contains(&sy) {
                                    let page = [sx as u32 / PAGE_SIZE, sy as u32 / PAGE_SIZE];
                                    assert!(job.sources.contains(&page), "{label} {interpolation:?}: {x},{y} needs source page {page:?}");
                                }
                            }
                        }
                    }
                }}
            }
        }
    }
}

#[test]
fn independent_projective_group_mask_bake_preserves_owner_and_default_domain() {
    let renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = layer_core::Document::new("independent mask", 512,512,
        layer_core::DocumentNames { paint:"Ink".into(),paper:"Paper".into() });
    let mut owner = Layer::paint(LayerId(1),"group");
    owner.kind = LayerKind::Group;
    owner.properties.offset = Point { x:17.,y:23. };
    owner.opacity = 0.4;
    let mut mask = LayerMask::reveal_all(LayerId(2),Point { x:-32.,y:-14. });
    mask.extent = Some([256;2]);
    mask.linked = false;
    mask.enabled = false;
    mask.inverted = true;
    mask.default_coverage = 0.63;
    mask.placement = Projective([0.9,0.07,0.,-0.03,1.1,0.,0.0003,0.0001,1.]);
    mask.initial = Some(Selection::polygon(vec![Point {x:300.,y:300.},Point {x:340.,y:300.},
        Point {x:340.,y:340.},Point {x:300.,y:340.}]).unwrap());
    let mut bytes = Vec::new();
    for y in 0..PAGE_SIZE { for x in 0..PAGE_SIZE { bytes.push(if (80..160).contains(&x) && (90..190).contains(&y) { 51 } else { 204 }); }}
    let mut data = RasterData::default();
    data.tiles.insert(TileKey {plane:RasterPlane::Mask,coordinate:[0;2]},RasterTile::backed(
        TileBlob::encode(RasterPlane::Mask.descriptor(document.color),&bytes).unwrap()));
    mask.raster = RasterRevision::backed(data);
    let source_mask = mask.clone();
    owner.mask = Some(mask);
    let mut child = Layer::paint(LayerId(3),"child");
    child.properties.parent = Some(owner.id);
    document.layers = vec![child,owner.clone()];
    document.active_layer = owner.id;
    document.active_mask = true;
    let geometry = document.layer_geometry(source_mask.id).projective().unwrap().0.map(f64::from);
    let plan = document.transform_pixels_plan(source_mask.id,Interpolation::Nearest,Default::default()).unwrap();
    assert_eq!(plan.scope,layer_core::TransformPixelsScope::Mask);
    let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan,Default::default())).unwrap();
    assert_eq!(baked.properties,owner.properties);
    assert_eq!(baked.raster,owner.raster);
    assert_eq!(baked.source,owner.source);
    assert_eq!(baked.opacity,owner.opacity);
    let mask = baked.mask.as_ref().unwrap();
    assert_eq!(mask.inverted,source_mask.inverted);
    assert_eq!(mask.enabled,source_mask.enabled);
    assert_eq!(mask.default_coverage,source_mask.default_coverage);
    assert_eq!(mask.placement,Projective::IDENTITY);
    assert!(mask.initial.is_none());
    let mut checked = 0;
    let mut defaults = 0;
    for (key,tile) in &mask.raster.wait_data().unwrap().tiles {
        assert_eq!(key.plane,RasterPlane::Mask);
        let bytes = tile.wait_backing().unwrap().decode().unwrap();
        for (i,actual) in bytes.into_iter().enumerate() {
            let world = [mask.offset.x as f64+(key.coordinate[0]*PAGE_SIZE+i as u32%PAGE_SIZE) as f64+0.5,
                mask.offset.y as f64+(key.coordinate[1]*PAGE_SIZE+i as u32/PAGE_SIZE) as f64+0.5];
            let expected = crate::test_support::preimage(geometry,world).filter(|p| p.iter().all(|v| *v>=0. && *v<256.)).map_or_else(
                || {defaults+=1;161}, |p| if (80. ..160.).contains(&p[0].floor()) && (90. ..190.).contains(&p[1].floor()) {51}else{204});
            assert_eq!(actual,expected,"mask raw sample at {world:?}");
            checked+=1;
        }
    }
    assert!(checked>256*256 && defaults>256*256);
}

#[test]
fn linked_outer_mesh_mask_bake_uses_winning_owner_uv_and_preserves_no_hit_default() {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_,mut placement) = nonlinear_maps().pop().unwrap();
    placement.interpolation = Interpolation::Nearest;
    let positions = source_positions(&placement);
    for mask_placement in [Projective([1.,0.03,0.,-0.02,1.,0.,0.0001,0.00005,1.]),
        Projective([1.,0.,0.,0.,1.,0.,0.004,0.,1.])] {
    for inverted in [false,true] {
        let mut document = layer_core::Document::new("linked folded mask",512,512,
            layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        let mut owner = layer(fixture(Affine::IDENTITY,4.),Affine::IDENTITY);
        owner.properties.placement = placement.clone();
        let mut mask = LayerMask::reveal_all(LayerId(2),Point {x:7.,y:-3.});
        mask.extent = Some([256;2]);
        mask.default_coverage = 161./255.;
        mask.inverted = inverted;
        mask.placement = mask_placement;
        let mut data = RasterData::default();
        let bytes: Vec<_> = (0..PAGE_SIZE*PAGE_SIZE).map(|i| if i%PAGE_SIZE>220 && i/PAGE_SIZE>100 {51}else{204}).collect();
        data.tiles.insert(TileKey {plane:RasterPlane::Mask,coordinate:[0;2]},RasterTile::backed(
            TileBlob::encode(RasterPlane::Mask.descriptor(document.color),&bytes).unwrap()));
        mask.raster = RasterRevision::backed(data);
        let adapter = mask.placement.then(Projective::from_affine(Affine::translation(mask.offset))).unwrap().inverse().unwrap();
        owner.mask = Some(mask.clone());
        document.layers = vec![owner.clone()];
        document.active_layer = owner.id;
        document.active_mask = true;
        let original = render(&mut renderer,&owner,&[],true);
        let source: Planes = owner.raster.wait_data().unwrap().tiles.iter().map(|(key,tile)|
            (*key,tile.wait_backing().unwrap().decode().unwrap())).collect();
        let plan = document.transform_pixels_plan(mask.id,Interpolation::Nearest,Default::default()).unwrap();
        assert_eq!(plan.target,mask.id);
        assert_eq!(plan.scope,layer_core::TransformPixelsScope::Paint {linked_mask:true});
        let baked = pollster::block_on(renderer.snapshot_gpu().transform_pixels(plan,Default::default())).unwrap();
        let actual: Planes = baked.raster.wait_data().unwrap().tiles.iter().map(|(key,tile)|
            (*key,tile.wait_backing().unwrap().decode().unwrap())).collect();
        check_raw_mapping(&source,&actual,&positions,"linked outer mesh bake");
        let baked_mask = baked.mask.as_ref().unwrap();
        assert_eq!(baked_mask.inverted,inverted);
        let masks: Planes = baked_mask.raster.wait_data().unwrap().tiles.iter().map(|(key,tile)|
            (*key,tile.wait_backing().unwrap().decode().unwrap())).collect();
        let mut no_hit = 0;
        for (i,sources) in positions.iter().enumerate() {
            let codes: Vec<u8> = sources.iter().map(|source| source.and_then(|[x,y]|
                adapter.map(Point {x:x as f32,y:y as f32})).filter(|p| p.x>=0. && p.y>=0. && p.x<256. && p.y<256.)
                .map_or(161,|p| if p.x.floor()>220. && p.y.floor()>100. {51}else{204})).collect();
            let coordinate = [i as u32%512/PAGE_SIZE,i as u32/512/PAGE_SIZE];
            let pixel = (i as u32/512%PAGE_SIZE*PAGE_SIZE+i as u32%512%PAGE_SIZE) as usize;
            let actual = masks[&TileKey {plane:RasterPlane::Mask,coordinate}][pixel];
            assert!(codes.contains(&actual),"linked mask at {},{}: {actual} expected {codes:?}",i%512,i/512);
            no_hit+=usize::from(sources==&vec![None]);
        }
        assert!(no_hit>200_000);
        let restored = render(&mut renderer,&baked,&[],true);
        assert_pixels(&restored,&original,"linked outer mesh preview/bake appearance");
    }
    }
}

#[test]
fn retained_outer_mesh_document_reads_and_linked_mask_thumbnail_match_independent_bake() {
    use layer_render::{ColorSampleRequest,ColorSampleArea,ColorSampleSource,RegionRequest,RegionSource};
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_,mut placement)=nonlinear_maps().pop().unwrap();
    placement.interpolation=Interpolation::Nearest;
    let mut doc=layer_core::Document::new("placed reads",512,512,
        layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let mut owner=layer(fixture(Affine::IDENTITY,4.),Affine::IDENTITY);
    owner.properties.placement=placement;
    let mut mask=LayerMask::reveal_all(LayerId(2),Point {x:7.,y:-3.});
    mask.extent=Some([256;2]);mask.default_coverage=161./255.;
    mask.placement=Projective([1.,0.03,0.,-0.02,1.,0.,0.0001,0.00005,1.]);
    let mut data=RasterData::default();
    let bytes:Vec<_>=(0..PAGE_SIZE*PAGE_SIZE).map(|i| if i%PAGE_SIZE>110 && i/PAGE_SIZE>100 {51}else{204}).collect();
    data.tiles.insert(TileKey {plane:RasterPlane::Mask,coordinate:[0;2]},RasterTile::backed(
        TileBlob::encode(RasterPlane::Mask.descriptor(doc.color),&bytes).unwrap()));
    mask.raster=RasterRevision::backed(data);owner.mask=Some(mask);
    doc.layers=vec![owner.clone()];
    let plan=doc.transform_pixels_plan(owner.id,Interpolation::Nearest,Default::default()).unwrap();
    let baked=pollster::block_on(r.snapshot_gpu().transform_pixels(plan,Default::default())).unwrap();
    let coverage=|r:&mut WgpuRasterizer,id| crate::test_support::receive_request(r,RegionRequest {
        request_id:1,source:RegionSource::Coverage(id),position:[0;2],contiguous:false,
        tolerance:0.,refinement:Default::default(),selection:Some(layer_render::SelectionRefinement {
            resize:0,mode:layer_core::SelectionMode::New,antialias:true,feather:0.,previous:None,
            source_to_document:Affine::IDENTITY,keep_canvas_edges:false
        }),limit:None
    }).pixels;
    let points=|r:&mut WgpuRasterizer,id| {
        [[0,0],[60,60],[170,120],[240,130],[270,150],[255,255],[320,280],[511,511]]
            .into_iter().flat_map(|position| [ColorSampleArea::Point,ColorSampleArea::Circle5].map(|area| {
                assert!(r.request_color_sample(ColorSampleRequest {request_id:1,source:ColorSampleSource::Layer(id),position,area}).unwrap());
                crate::test_support::complete(r);r.take_color_sample().unwrap().unwrap().rgba
            })).collect::<Vec<_>>()
    };
    for inverted in [false,true] {
        owner.mask.as_mut().unwrap().inverted=inverted;
        let mut reference=baked.clone();reference.mask.as_mut().unwrap().inverted=inverted;
        render(&mut r,&owner,&[],true);
        let color=points(&mut r,owner.id);
        let alpha=coverage(&mut r,owner.id);
        let mask=coverage(&mut r,LayerId(2));
        let thumb=crate::source_thumbnails::tests::thumbnail(&mut r,LayerId(2));
        render(&mut r,&reference,&[],true);
        assert_eq!(color,points(&mut r,owner.id));
        assert_eq!(alpha.words(),coverage(&mut r,owner.id).words());
        assert_eq!(mask.words(),coverage(&mut r,LayerId(2)).words());
        assert_pixels(&thumb,&crate::source_thumbnails::tests::thumbnail(&mut r,LayerId(2)),"placed linked mask thumbnail");
    }
}

#[test]
fn folded_selection_copy_bake_applies_soft_coverage_after_the_source_mask() {
    let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (_,mut placement)=nonlinear_maps().pop().unwrap();placement.interpolation=Interpolation::Nearest;
    let mut owner=layer(fixture(Affine::IDENTITY,4.),Affine::IDENTITY);
    owner.properties.placement=placement;
    let mut mask=LayerMask::reveal_all(LayerId(2),Point {x:7.,y:-3.});
    mask.default_coverage=0.63;
    mask.initial=Some(layer_core::Selection::polygon(vec![Point {x:160.,y:80.},Point {x:290.,y:80.},
        Point {x:280.,y:180.},Point {x:170.,y:180.}]).unwrap());owner.mask=Some(mask);
    render(&mut r,&owner,&[],true);
    let original=crate::test_support::float_pixels(&r,crate::test_support::document_texture(&r));
    let occupied:Vec<_>=original.iter().enumerate().filter(|(_,p)|p[3]>0.).map(|(i,_)|[i%512,i/512]).collect();
    let bounds=[occupied.iter().map(|p|p[0]).min().unwrap(),occupied.iter().map(|p|p[1]).min().unwrap(),
        occupied.iter().map(|p|p[0]).max().unwrap()+1,occupied.iter().map(|p|p[1]).max().unwrap()+1];
    let [left,top,right,bottom]=[(bounds[0]+bounds[2])/2,bounds[1],bounds[2],bounds[3]];
    let mut words=vec![0u32;512*128];
    for y in top..bottom {for x in left..right {words[y*128+x/4]|=127u32<<((x%4)*8);}}
    let mut coverage=LayerMask::reveal_all(LayerId(4),Point::default());coverage.default_coverage=0.;
    coverage.initial=Some(layer_core::Selection::pixels(Arc::new(layer_core::SelectionPixels::bytes(
        EXTENT,[left as u32,top as u32,right as u32,bottom as u32],words).unwrap())));
    let operation=LayerOperation {placement:Affine::IDENTITY,coverage,
        kind:LayerOperationKind::Bake {members:vec![owner.clone()].into(),offset:Point::default()}};
    let mut copied=Layer::paint(LayerId(3),"selection copy");copied.properties.extent=Some(EXTENT);
    let damage=operation.bounds(EXTENT);copied.pending_operations.push(operation);
    let batch=DabBatch {layer_id:copied.id,kind:DabBatchKind::LayerOperation(0),dab_count:0,damage,..batch(3)};
    owner.visible=false;
    r.submit(FramePacket {dab_batches:&[batch],..packet(&[copied.clone(),owner],EXTENT)}).unwrap();
    let actual=crate::test_support::float_pixels(&r,crate::test_support::document_texture(&r));
    let mut occupied=0;
    for (i,(actual,source)) in actual.iter().zip(&original).enumerate() {
        let x=i%512;let y=i/512;
        let coverage=if (left..right).contains(&x)&&(top..bottom).contains(&y){127./255.}else{0.};
        occupied+=usize::from(actual[3]>0.);
        for c in 0..4 {assert!((actual[c]-source[c]*coverage).abs()<1e-6,
            "copy {x},{y} channel{c}: {actual:?}, source {source:?}, coverage{coverage}");}
    }
    assert!(occupied>100,"folded soft selection must retain actual paint");
    assert!(r.paint_layers.iter().find(|layer|layer.id==copied.id).unwrap().pages.iter()
        .all(|page|!page_rect(page.coordinate).intersect(pixel_rect(damage,EXTENT)).is_empty()),"copy must allocate only selection pages");
}
