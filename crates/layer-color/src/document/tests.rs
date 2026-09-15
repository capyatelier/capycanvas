use super::*;
use layer_core::{ColorTransition, Document, Editor, Layer, LayerKind, LayerMask, Point};

const LIMIT: usize = 16 * 1024 * 1024;

fn key(plane: RasterPlane) -> TileKey {
    TileKey {
        plane,
        coordinate: [0, 0],
    }
}

fn samples(depth: IntegerDepth, channels: usize) -> Vec<u8> {
    (0..TILE_SIZE * TILE_SIZE)
        .flat_map(|i| {
            let n = i % (depth.maximum() + 1);
            [n, depth.maximum() - n, n / 3, n]
                .into_iter()
                .take(channels)
        })
        .flat_map(|n| n.to_le_bytes().into_iter().take(depth.bytes()))
        .collect()
}

fn fixture(color: DocumentColor) -> Project {
    let mut document = Document::new("document-color", TILE_SIZE, TILE_SIZE);
    document.color = color;
    let rgba = Arc::new(
        TileBlob::encode_source(color.paint_descriptor(), &samples(color.depth, 4)).unwrap(),
    );
    let scalar = Arc::new(
        TileBlob::encode_source(color.coverage_descriptor(), &samples(color.depth, 1)).unwrap(),
    );
    let root = RasterRevision::backed(RasterData {
        tiles: [
            (
                key(RasterPlane::Color),
                RasterTile::backed_shared(rgba.clone()),
            ),
            (
                key(RasterPlane::Wetness),
                RasterTile::backed_shared(scalar.clone()),
            ),
            (
                key(RasterPlane::WatercolorWetness),
                RasterTile::backed_shared(scalar.clone()),
            ),
        ]
        .into(),
        watercolor: Some(RasterWatercolor {
            wet_edge: 0.25,
            burnt_edge: 0.125,
            edge_width: 3.,
        }),
    });
    let mut mask = LayerMask::reveal_all(document.allocate_layer_id(), Point { x: 2.5, y: -1. });
    mask.inverted = true;
    mask.raster = RasterRevision::backed(RasterData {
        tiles: [(key(RasterPlane::Mask), RasterTile::backed_shared(scalar))].into(),
        watercolor: None,
    });
    let rasterized = Arc::new(SourceImage {
        resolution: None,
        kind: SourceKind::Rasterized,
        // Includes a complete tile outside the document, shared with paint.
        extent: [512, 256],
        interpretation: SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: color.depth,
            profile: ColorProfile::Builtin(color.space),
            profile_assumed: false,
        },
        tiles: [([0, 0], rgba.clone()), ([1, 0], rgba)].into(),
    });
    document.layers[0].raster = root;
    document.layers[0].mask = Some(mask);
    document.layers[0].opacity = 0.75;
    let mut base = Layer::paint(document.allocate_layer_id(), "Rasterized full image");
    base.kind = LayerKind::ImportedImage;
    base.source = Some(rasterized.clone());
    document.layers.insert(1, base);
    let mut original = rasterized.as_ref().clone();
    original.kind = SourceKind::Original;
    original.interpretation.profile = ColorProfile::Icc(
        crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3))
            .unwrap()
            .into(),
    );
    let mut source = Layer::paint(document.allocate_layer_id(), "Independent original");
    source.kind = LayerKind::ImportedImage;
    source.source = Some(Arc::new(original));
    document.layers.insert(2, source);
    // Live color definitions are independent of document assignment/conversion.
    // All existing sample, metadata and history checks also cover these layers.
    use layer_core::{EffectInstance, EffectValue, GradientStop, color::RgbColor};
    let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0.234567, 123. / 65535.]).unwrap();
    for (id, key, value) in [
        ("black_white", "tint_color", EffectValue::Color(color)),
        ("gradient_map", "gradient", EffectValue::Gradient(vec![
            GradientStop { position: 0., color },
            GradientStop { position: 1., color: RgbColor::new(RgbSpace::ProPhoto, [0.123456, 0.75, 0.5, 0.37]).unwrap() },
        ])),
    ] {
        let mut effect = EffectInstance::new(layer_core::bundled_effect_catalog().get(id).unwrap().program());
        effect.set(key, value).unwrap();
        let mut layer = Layer::paint(document.allocate_layer_id(), id);
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(effect));
        document.layers.insert(document.layers.len() - 1, layer);
    }
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(Default::default()).unwrap();
    project
}

fn backing(root: &RasterRevision, plane: RasterPlane) -> Arc<TileBlob> {
    root.wait_data().unwrap().tiles[&key(plane)]
        .wait_backing()
        .unwrap()
}

fn assert_original_and_properties(old: &Project, new: &Project) {
    let a = &old.document;
    let b = &new.document;
    assert_eq!(
        (a.width, a.height, a.active_layer, a.active_mask),
        (b.width, b.height, b.active_layer, b.active_mask)
    );
    assert!(Arc::ptr_eq(
        a.layers[2].source.as_ref().unwrap(),
        b.layers[2].source.as_ref().unwrap()
    ));
    assert_eq!(
        a.layers[1].source.as_ref().unwrap().extent,
        b.layers[1].source.as_ref().unwrap().extent
    );
    for (a, b) in a.layers.iter().zip(&b.layers) {
        let mut metadata = b.clone();
        metadata.raster = a.raster.clone();
        metadata.source = a.source.clone();
        if let (Some(a), Some(b)) = (&a.mask, &mut metadata.mask) {
            b.raster = a.raster.clone();
        }
        assert_eq!(*a, metadata);
    }
}

#[test]
fn assignment_preserves_every_code_and_shared_backing_in_all_eight_modes() {
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        for space in RgbSpace::ALL {
            let project = fixture(DocumentColor { space, depth });
            for target in RgbSpace::ALL {
                let prepared = prepare_document_color(
                    &project,
                    DocumentColorChange::Assign(target),
                    LIMIT,
                    || false,
                )
                .unwrap();
                assert_eq!(
                    prepared.project.document.color,
                    DocumentColor {
                        space: target,
                        depth
                    }
                );
                assert_eq!(prepared.statistics.clipped_channels, 0);
                assert_original_and_properties(&project, &prepared.project);
                let a = &project.document.layers;
                let b = &prepared.project.document.layers;
                assert_eq!(a[0].raster.identity(), b[0].raster.identity());
                assert_eq!(
                    a[0].mask.as_ref().unwrap().raster.identity(),
                    b[0].mask.as_ref().unwrap().raster.identity()
                );
                let source = b[1].source.as_ref().unwrap();
                assert_eq!(source.interpretation.profile, ColorProfile::Builtin(target));
                for (coordinate, blob) in &a[1].source.as_ref().unwrap().tiles {
                    assert!(Arc::ptr_eq(blob, &source.tiles[coordinate]));
                }
                if target == space {
                    assert_eq!(prepared.allocated_bytes, 0);
                }
            }
        }
    }
}

#[test]
fn depth_changes_rescale_all_codes_in_color_alpha_mask_and_both_wetness_planes() {
    for space in RgbSpace::ALL {
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
            let project = fixture(DocumentColor { space, depth });
            let target = if depth == IntegerDepth::U8 {
                IntegerDepth::U16
            } else {
                IntegerDepth::U8
            };
            let prepared = prepare_document_color(
                &project,
                DocumentColorChange::Depth {
                    depth: target,
                    dither: OutputDither::None,
                },
                LIMIT,
                || false,
            )
            .unwrap();
            assert_original_and_properties(&project, &prepared.project);
            assert_eq!(prepared.statistics.clipped_channels, 0);
            let layers = &prepared.project.document.layers;
            for plane in [
                RasterPlane::Color,
                RasterPlane::Mask,
                RasterPlane::Wetness,
                RasterPlane::WatercolorWetness,
            ] {
                let root = if plane == RasterPlane::Mask {
                    &layers[0].mask.as_ref().unwrap().raster
                } else {
                    &layers[0].raster
                };
                let blob = backing(root, plane);
                let bytes = blob.decode().unwrap();
                let original = samples(depth, if plane == RasterPlane::Color { 4 } else { 1 });
                for (a, b) in original
                    .chunks_exact(depth.bytes())
                    .zip(bytes.chunks_exact(target.bytes()))
                {
                    if target == IntegerDepth::U16 {
                        assert_eq!(
                            u16::from_le_bytes([b[0], b[1]]),
                            a[0] as u16 * 257,
                            "{space:?} {plane:?}"
                        );
                    } else {
                        let code = u16::from_le_bytes([a[0], a[1]]) as u32;
                        assert_eq!(
                            b[0] as u32,
                            (code + 128) / 257,
                            "{space:?} {plane:?} {code}"
                        );
                    }
                }
            }
            let rgba = backing(&layers[0].raster, RasterPlane::Color);
            let source = layers[1].source.as_ref().unwrap();
            assert_eq!(source.interpretation.depth, target);
            assert!(source.tiles.values().all(|blob| Arc::ptr_eq(blob, &rgba)));
            let wetness = backing(&layers[0].raster, RasterPlane::Wetness);
            assert!(Arc::ptr_eq(
                &wetness,
                &backing(&layers[0].raster, RasterPlane::WatercolorWetness)
            ));
            assert!(Arc::ptr_eq(
                &wetness,
                &backing(&layers[0].mask.as_ref().unwrap().raster, RasterPlane::Mask)
            ));
        }
    }
}

#[test]
fn conversion_matches_f64_coordinates_within_one_code_and_keeps_exact_alpha() {
    for depth in [IntegerDepth::U8, IntegerDepth::U16] {
        for space in RgbSpace::ALL {
            let project = fixture(DocumentColor { space, depth });
            let original = samples(depth, 4);
            let maximum = depth.maximum() as f64;
            for target in RgbSpace::ALL {
                let prepared = prepare_document_color(
                    &project,
                    DocumentColorChange::Convert {
                        space: target,
                        options: Default::default(),
                    },
                    LIMIT,
                    || false,
                )
                .unwrap();
                assert_original_and_properties(&project, &prepared.project);
                let bytes = backing(
                    &prepared.project.document.layers[0].raster,
                    RasterPlane::Color,
                )
                .decode()
                .unwrap();
                let read = |bytes: &[u8]| {
                    if depth == IntegerDepth::U8 {
                        bytes[0] as u32
                    } else {
                        u16::from_le_bytes([bytes[0], bytes[1]]) as u32
                    }
                };
                let mut expected_clipped = 0;
                for (a, b) in original
                    .chunks_exact(4 * depth.bytes())
                    .zip(bytes.chunks_exact(4 * depth.bytes()))
                {
                    let a: Vec<_> = a.chunks_exact(depth.bytes()).map(read).collect();
                    let b: Vec<_> = b.chunks_exact(depth.bytes()).map(read).collect();
                    let rgb = space.convert(target, [a[0], a[1], a[2]].map(|v| v as f64 / maximum));
                    for c in 0..3 {
                        let reference = (rgb[c] * maximum).round();
                        expected_clipped += u64::from(reference < 0. || reference > maximum);
                        assert!(
                            (b[c] as f64 - reference.clamp(0., maximum)).abs() <= 1.,
                            "{space:?} → {target:?}, {depth:?}, {a:?} {b:?} {rgb:?}"
                        );
                    }
                    assert_eq!(a[3], b[3]);
                }
                assert_eq!(prepared.statistics.clipped_channels, expected_clipped);
                assert_eq!(
                    project.document.layers[0]
                        .mask
                        .as_ref()
                        .unwrap()
                        .raster
                        .identity(),
                    prepared.project.document.layers[0]
                        .mask
                        .as_ref()
                        .unwrap()
                        .raster
                        .identity()
                );
            }
        }
    }
}

#[test]
fn dither_is_repeatable_coordinate_dependent_and_never_changes_coverage() {
    let project = fixture(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    let change = DocumentColorChange::Depth {
        depth: IntegerDepth::U8,
        dither: OutputDither::Stochastic8,
    };
    let a = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let b = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let source_a = a.project.document.layers[1].source.as_ref().unwrap();
    let source_b = b.project.document.layers[1].source.as_ref().unwrap();
    assert_eq!(source_a, source_b);
    assert_ne!(
        source_a.tiles[&[0, 0]].digest,
        source_a.tiles[&[1, 0]].digest
    );
    assert!(Arc::ptr_eq(
        &source_a.tiles[&[0, 0]],
        &backing(&a.project.document.layers[0].raster, RasterPlane::Color)
    ));
    let plain = prepare_document_color(
        &project,
        DocumentColorChange::Depth {
            depth: IntegerDepth::U8,
            dither: OutputDither::None,
        },
        LIMIT,
        || false,
    )
    .unwrap();
    for plane in [RasterPlane::Wetness, RasterPlane::WatercolorWetness] {
        assert_eq!(
            backing(&a.project.document.layers[0].raster, plane).digest,
            backing(&plain.project.document.layers[0].raster, plane).digest
        );
    }
    let bytes = backing(&plain.project.document.layers[0].raster, RasterPlane::Color)
        .decode()
        .unwrap();
    for tile in source_a.tiles.values() {
        let dithered = tile.decode().unwrap();
        for (a, b) in bytes.chunks_exact(4).zip(dithered.chunks_exact(4)) {
            assert_eq!(a[3], b[3]);
            assert!(a[..3].iter().zip(&b[..3]).all(|(a, b)| a.abs_diff(*b) <= 1));
        }
    }
}

#[test]
fn absolute_intent_keeps_the_white_point_difference_and_preserves_alpha() {
    let project = fixture(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    let convert = |intent| {
        prepare_document_color(
            &project,
            DocumentColorChange::Convert {
                space: RgbSpace::Srgb,
                options: ConversionOptions {
                    intent,
                    black_point_compensation: false,
                },
            },
            LIMIT,
            || false,
        )
        .unwrap()
    };
    let relative = convert(RenderingIntent::RelativeColorimetric);
    let absolute = convert(RenderingIntent::AbsoluteColorimetric);
    let relative = backing(
        &relative.project.document.layers[0].raster,
        RasterPlane::Color,
    )
    .decode()
    .unwrap();
    let absolute = backing(
        &absolute.project.document.layers[0].raster,
        RasterPlane::Color,
    )
    .decode()
    .unwrap();
    assert_ne!(relative, absolute);
    for (a, b) in relative.chunks_exact(8).zip(absolute.chunks_exact(8)) {
        assert_eq!(&a[6..], &b[6..]);
    }
}

#[test]
fn explicit_attachment_assignment_recovers_declared_straight_codes_before_retagging() {
    let mut project = fixture(DocumentColor::default());
    let bytes: Vec<_> = (0..256)
        .flat_map(|alpha| {
            (0..256).flat_map(move |code| [code as u8, code as u8, code as u8, alpha as u8])
        })
        .collect();
    let mut data = (*project.document.layers[0].raster.wait_data().unwrap()).clone();
    data.tiles.insert(
        key(RasterPlane::Color),
        RasterTile::backed(TileBlob::encode_source(PixelDescriptor::SRGB8_PAINT, &bytes).unwrap()),
    );
    project.document.layers[0].raster = RasterRevision::backed(data);
    let prepared = prepare_document_color(
        &project,
        DocumentColorChange::Assign(RgbSpace::DisplayP3),
        LIMIT,
        || false,
    )
    .unwrap();
    let blob = backing(
        &prepared.project.document.layers[0].raster,
        RasterPlane::Color,
    );
    assert_eq!(
        blob.descriptor,
        prepared.project.document.color.paint_descriptor()
    );
    let result = blob.decode().unwrap();
    for (a, b) in bytes.chunks_exact(4).zip(result.chunks_exact(4)) {
        let reference = if a[3] == 0 {
            0.
        } else {
            let linear = RgbSpace::Srgb.decode(a[0] as f64 / 255.) / (a[3] as f64 / 255.);
            (RgbSpace::Srgb.encode(linear) * 255.)
                .round()
                .clamp(0., 255.)
        };
        assert!((b[0] as f64 - reference).abs() <= 1.);
        assert_eq!(b[0], b[1]);
        assert_eq!(b[1], b[2]);
        assert_eq!(a[3], b[3]);
    }
    let mut editor = Editor::new(project.document.clone());
    editor.perform(prepared.edit()).unwrap();
    editor.undo().unwrap();
    assert_eq!(
        editor.document().layers[0].raster,
        project.document.layers[0].raster
    );
}

#[test]
fn apply_and_history_restore_exact_roots_sources_properties_and_checkpoints() {
    let project = fixture(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: IntegerDepth::U16,
    });
    for change in [
        DocumentColorChange::Assign(RgbSpace::AdobeRgb),
        DocumentColorChange::Convert {
            space: RgbSpace::Srgb,
            options: Default::default(),
        },
        DocumentColorChange::Depth {
            depth: IntegerDepth::U8,
            dither: OutputDither::Stochastic8,
        },
    ] {
        let prepared = prepare_document_color(&project, change, LIMIT, || false).unwrap();
        let mut editor = Editor::new(project.document.clone());
        editor.validate_edit(&prepared.edit()).unwrap();
        assert_eq!(editor.document(), &project.document);
        assert!(!editor.can_undo());
        let transition = editor
            .prepare_color_transition(ColorTransition::Apply {
                color: prepared.project.document.color,
                layers: prepared.project.document.layers.clone(),
            })
            .unwrap();
        assert_eq!(transition.document(), &prepared.project.document);
        editor
            .commit_color_transition::<layer_core::DocumentError>(transition, |candidate| {
                assert_eq!(candidate, &prepared.project.document);
                Ok(())
            })
            .unwrap();
        assert_eq!(editor.document(), &prepared.project.document);
        let checkpoint = editor.checkpoint();
        for _ in 0..3 {
            let transition = editor
                .prepare_color_transition(ColorTransition::Undo)
                .unwrap();
            editor
                .commit_color_transition::<layer_core::DocumentError>(transition, |_| Ok(()))
                .unwrap();
            assert!(!editor.can_undo());
            assert_eq!(editor.checkpoint(), 0);
            let mut restored = editor.document().clone();
            restored.revision = project.document.revision;
            assert_eq!(restored, project.document);
            let transition = editor
                .prepare_color_transition(ColorTransition::Redo)
                .unwrap();
            editor
                .commit_color_transition::<layer_core::DocumentError>(transition, |_| Ok(()))
                .unwrap();
            assert!(!editor.can_redo());
            assert_eq!(editor.checkpoint(), checkpoint);
            let mut restored = editor.document().clone();
            restored.revision = prepared.project.document.revision;
            assert_eq!(restored, prepared.project.document);
        }
    }
}

#[test]
fn cancellation_limits_and_invalid_candidates_leave_document_and_history_intact() {
    let project = fixture(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: IntegerDepth::U16,
    });
    let snapshot = project.document.clone();
    let change = DocumentColorChange::Depth {
        depth: IntegerDepth::U8,
        dither: OutputDither::None,
    };
    for when in [1, 12, 200] {
        let mut checks = 0;
        let error = prepare_document_color(&project, change, LIMIT, || {
            checks += 1;
            checks == when
        })
        .err()
        .unwrap();
        assert!(error.contains("cancelled"), "{error}");
        assert_eq!(checks, when);
    }
    let error = prepare_document_color(&project, change, 1, || false)
        .err()
        .unwrap();
    assert!(error.contains("memory limit"), "{error}");
    assert_eq!(project.document, snapshot);
    let prepared = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let mut editor = Editor::new(project.document.clone());
    for bad in 0..7 {
        let mut layers = prepared.project.document.layers.clone();
        match bad {
            0 => layers[0].opacity = 0.25,
            1 => {
                layers.remove(1);
            }
            2 => layers[0].raster = project.document.layers[0].raster.clone(),
            3 => layers[0].raster = RasterRevision::pending(),
            4 => layers[0].raster = RasterRevision::default(),
            5 => {
                let mut data = (*layers[0].raster.wait_data().unwrap()).clone();
                data.watercolor = None;
                layers[0].raster = RasterRevision::backed(data);
            }
            6 => layers[2].source = layers[1].source.clone(),
            _ => unreachable!(),
        }
        assert!(
            editor
                .perform(Edit::SetColor {
                    color: prepared.project.document.color,
                    layers
                })
                .is_err(),
            "case {bad}"
        );
        assert_eq!(editor.document(), &snapshot);
        assert_eq!(editor.checkpoint(), 0);
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());
    }
    // Cancellation must interrupt a not-yet-published raster, too.
    let mut pending = project.clone();
    pending.document.layers[0].raster = RasterRevision::pending();
    let mut checks = 0;
    let error = prepare_document_color(&pending, change, LIMIT, || {
        checks += 1;
        checks == 5
    })
    .err()
    .unwrap();
    assert!(error.contains("cancelled"), "{error}");
    assert!(pending.document.layers[0].raster.try_data().is_none());
}
