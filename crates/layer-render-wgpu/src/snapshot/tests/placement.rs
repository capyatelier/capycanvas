//! Cropped file-worker capture must restore source-local backing off the canvas.
use super::*;

#[test]
fn snapshot_placed_photo_crops_restore_off_canvas_paint_and_linked_mask() {
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let extent = [1025, 769];
    let mut project = source_project(color, extent);
    let interpretation = project.document.layers[0]
        .source
        .as_ref()
        .unwrap()
        .interpretation
        .clone();
    let mut builder = SourceBuilder::new(extent, interpretation, 16 * 1024 * 1024).unwrap();
    for _ in 0..extent[1] {
        builder
            .push_row(&vec![255; extent[0] as usize * 8])
            .unwrap();
    }
    project.document.layers[0].source = Some(Arc::new(builder.finish().unwrap()));
    project.document.width = 321;
    project.document.height = 257;
    let group_id = project.document.allocate_layer_id();
    let mask_id = project.document.allocate_layer_id();
    let mut group = Layer::paint(group_id, "translated group");
    group.kind = LayerKind::Group;
    group.properties.offset = Point { x: 15., y: -9. };
    let layer = &mut project.document.layers[0];
    layer.properties.parent = Some(group_id);
    layer.properties.offset = Point { x: -5., y: 11. };
    let mut paint = RasterData::default();
    let codes: Vec<_> = [32768u16, 0, 0, 65535]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    paint.tiles.insert(
        TileKey {
            plane: RasterPlane::Color,
            coordinate: [3, 2],
        },
        RasterTile::backed(
            TileBlob::encode(color.paint_descriptor(), &codes.repeat(65536)).unwrap(),
        ),
    );
    layer.raster = RasterRevision::backed(paint);
    let mut mask = LayerMask::reveal_all(mask_id, layer.properties.offset);
    let mut coverage = RasterData::default();
    coverage.tiles.insert(
        TileKey {
            plane: RasterPlane::Mask,
            coordinate: [3, 2],
        },
        RasterTile::backed(
            TileBlob::encode(
                color.coverage_descriptor(),
                &16384u16.to_le_bytes().repeat(65536),
            )
            .unwrap(),
        ),
    );
    mask.raster = RasterRevision::backed(coverage);
    layer.mask = Some(mask);
    project.document.layers.insert(0, group);

    for pose in [
        Affine([0.25, 0., 0., 0.25, 30., 4.]),
        Affine([0.27, 0.04, -0.03, 0.24, 30., 4.]),
        Affine([-0.25, 0., 0., 0.25, 290., 4.]),
    ] {
        project.document.layers[1].properties.placement = pose;
        project.validate(Default::default()).unwrap();
        let source = project.document.layers[1].source.clone();
        let inverse = pose
            .then(Affine::translation(Point { x: 10., y: 2. }))
            .inverse()
            .unwrap();
        let mut capture =
            SnapshotRenderer::new(project.clone(), [0.; 4], 0., Default::default()).unwrap();
        let mut painted = 0;
        // Visit distant windows and then return to force retirement/restoration.
        for rect in [
            [0, 0, 321, 257],
            [240, 145, 65, 75],
            [20, 125, 70, 110],
            [0, 0, 37, 41],
            [240, 145, 65, 75],
        ] {
            let pixels = capture.read_region(rect).unwrap();
            for y in 0..rect[3] {
                for x in 0..rect[2] {
                    let local = inverse.map(Point {
                        x: (x + rect[0]) as f32 + 0.5,
                        y: (y + rect[1]) as f32 + 0.5,
                    });
                    // The independent constant-color oracle excludes the edge
                    // footprints of a minified pixel's bilinear taps, but includes
                    // both sides of source/tile boundaries.
                    if [0., 768., 1024., 1025.]
                        .iter()
                        .any(|v| (local.x - v).abs() < 3.)
                        || [0., 512., 768., 769.]
                            .iter()
                            .any(|v| (local.y - v).abs() < 3.)
                    {
                        continue;
                    }
                    let inside = local.x > 0. && local.y > 0. && local.x < 1025. && local.y < 769.;
                    let overridden = inside
                        && local.x > 768.
                        && local.x < 1024.
                        && local.y > 512.
                        && local.y < 768.;
                    let expected = if overridden {
                        painted += 1;
                        let coverage = 16384. / 65535.;
                        // Native U16 paint stores encoded document RGB. Display
                        // P3 uses the sRGB transfer; capture returns linear RGB.
                        let red = ((32768.0_f32 / 65535. + 0.055) / 1.055).powf(2.4);
                        [red * coverage, 0., 0., coverage]
                    } else if inside {
                        [1.; 4]
                    } else {
                        [0.; 4]
                    };
                    let actual = pixels[(y * rect[2] + x) as usize];
                    assert!(
                        actual
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| (*a - b).abs() < 2e-6),
                        "pose={pose:?} rect={rect:?} local={local:?}: {actual:?} vs {expected:?}"
                    );
                }
            }
        }
        assert!(painted > 100);
        assert_eq!(project.document.layers[1].source, source);
        assert!(capture.renderer.composite_texture.is_none());
        assert!(capture.renderer.scene.as_ref().unwrap()
            .placement_cache(project.document.layers[1].id).is_none());
    }
}

#[test]
fn snapshot_export_does_not_bypass_placement_when_source_matches_canvas_extent() {
    let color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let mut project = source_project(color, [33, 17]);
    let target = project.document.layers[0]
        .source
        .as_ref()
        .unwrap()
        .interpretation
        .clone();
    project.document.layers[0].properties.placement = Affine::translation(Point { x: 7., y: -3. });
    let mut capture = SnapshotRenderer::new(project, [0.; 4], 0., Default::default()).unwrap();
    assert!(
        capture.identity_source(&target).is_none(),
        "source passthrough must honor placement"
    );
    let mut png = Vec::new();
    capture
        .write_png(&mut png, &target, Default::default(), None)
        .unwrap();
    let exported = decode(png);
    let bytes = raw_rows(&exported);
    let alpha = |x: usize, y: usize| {
        u16::from_le_bytes(
            bytes[(y * 33 + x) * 8 + 6..(y * 33 + x) * 8 + 8]
                .try_into()
                .unwrap(),
        )
    };
    assert_eq!(exported.extent, [33, 17]);
    assert_eq!(
        alpha(3, 8),
        0,
        "translation reveals transparent pixels on the left"
    );
    assert_eq!(
        alpha(15, 15),
        0,
        "translation reveals transparent pixels below the source"
    );
    assert_eq!(alpha(15, 8), 65535, "the placed image remains visible");
}
