use super::*;
use layer_core::{ColorTransition, Document, Editor, Edit, Point, authored::*};

const LIMIT: usize = 16 * 1024 * 1024;

fn changes(document: &Document) -> (Vec<RecordChange<PaintSource>>, Vec<RecordChange<CoverageSource>>) {
    let art = &document.artwork;
    (art.paint.iter().map(|(handle,id,value)| RecordChange {handle,id,value:Some(value.clone())}).collect(),
     art.coverage.iter().map(|(handle,id,value)| RecordChange {handle,id,value:Some(value.clone())}).collect())
}
fn edit(before: &Document, prepared: &PreparedDocumentColor) -> Edit {
    let (paint, coverage) = changes(&prepared.document);
    before.color_edit(prepared.document.composition().color, paint, coverage).unwrap()
}
fn paint(document: &Document, index: usize) -> &PaintSource {
    document.scene().paint_source(document.scene().order()[index]).unwrap()
}
fn paint_mut(document: &mut Document, index: usize) -> &mut PaintSource {
    let SourceTarget::Paint(handle) = document.scene().source_target(document.scene().order()[index]).unwrap() else {panic!("paint source required")};
    document.artwork.paint.get_mut(handle).unwrap()
}
fn coverage(document: &Document, index: usize) -> &CoverageSource {
    document.scene().mask(document.scene().order()[index]).unwrap().1
}

fn key(plane: RasterPlane) -> TileKey {
    TileKey {
        plane,
        coordinate: [0, 0],
    }
}

fn samples(depth: SampleDepth, channels: usize) -> Vec<u8> {
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

fn fixture(color: DocumentColor) -> Document {
    let document = Document::new(PortableId::random(), TILE_SIZE, TILE_SIZE, layer_core::DocumentNames {paint:"Current ink".into(),paper:"Paper".into()});
    let mut artwork = document.artwork.clone();
    let ink = document.working.occurrence.unwrap();
    let SourceTarget::Paint(ink_source) = document.working.target.unwrap() else {unreachable!()};
    let paper = document.scene().order()[1];
    let composition = artwork.compositions.get_mut(artwork.root).unwrap();
    composition.color = color; composition.blend = composition.blend.for_depth(color.depth);
    let stack = composition.result;
    let rgba = Arc::new(
        TileBlob::encode(color.paint_descriptor(), &samples(color.depth, 4)).unwrap(),
    );
    let scalar = Arc::new(
        TileBlob::encode(color.coverage_descriptor(), &samples(color.depth, 1)).unwrap(),
    );
    let root = RasterRevision::backed(RasterData {
        tiles: [
            (
                key(RasterPlane::Color),
                RasterTile::backed_shared(rgba.clone()),
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
    let coverage = artwork.coverage.insert(PortableId::random(), CoverageSource {
        domain:[TILE_SIZE;2], initial:None, default_coverage:1., operations:Default::default(),
        raster:RasterRevision::backed(RasterData {tiles:[(key(RasterPlane::Mask),RasterTile::backed_shared(scalar))].into(),watercolor:None}),
    }).unwrap();
    let mask = MaskUse {source:coverage, enabled:true, linked:true, inverted:true, translation:Point{x:2.5,y:-1.}, placement:layer_core::Projective::IDENTITY};
    let rasterized = Arc::new(SourceImage {
        resolution: None,
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
    artwork.paint.get_mut(ink_source).unwrap().raster = root;
    let occurrence = artwork.occurrences.get_mut(ink).unwrap(); occurrence.mask=Some(mask); occurrence.opacity=0.75;
    let base_source = artwork.paint.insert(PortableId::random(), PaintSource { color_mode:Default::default(),domain:[512,256],raster:Default::default(),base:Some(PaintBase {image:rasterized.clone().into(),offset:[0;2],policy:PaintBasePolicy::WorkingPixels}),operations:Default::default()}).unwrap();
    let base = artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(base_source),"Rasterized full image")).unwrap();
    let mut original = rasterized.as_ref().clone();
    original.interpretation.profile = ColorProfile::Icc(
        crate::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3))
            .unwrap()
            .into(),
    );
    let original_source=artwork.paint.insert(PortableId::random(),PaintSource { color_mode:Default::default(),domain:[512,256],raster:Default::default(),base:Some(PaintBase::new(Arc::new(original).into())),operations:Default::default()}).unwrap();
    let original=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(original_source),"Independent original")).unwrap();
    let mut entries=vec![ink,base,original];
    use layer_core::{EffectInstance, EffectValue, GradientStop, color::RgbColor};
    let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0.234567, 123. / 65535.]).unwrap();
    for (id, key, value) in [
        ("black_white", "tint_color", EffectValue::Color(color)),
        ("gradient_map", "gradient", EffectValue::Gradient(layer_core::GradientDefinition {stops:vec![
            GradientStop { position: 0., color },
            GradientStop { position: 1., color: RgbColor::new(RgbSpace::ProPhoto, [0.123456, 0.75, 0.5, 0.37]).unwrap() },
        ],interpolation:layer_core::ColorMixSpace::Classic})),
    ] {
        let mut effect = EffectInstance::new(layer_core::bundled_effect_catalog().get(id).unwrap().program());
        effect.set(key, value).unwrap();

        let application=artwork.effects.insert(PortableId::random(),EffectApplication::new(effect.program, effect.values, document.composition().size)).unwrap();
        entries.push(artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),id)).unwrap());
    }
    entries.push(paper); artwork.stacks.get_mut(stack).unwrap().entries=entries;
    let mut result=Document::from_artwork(artwork).unwrap(); result.working=document.working;
    result.validate(Default::default()).unwrap(); result
}

fn backing(root: &RasterRevision, plane: RasterPlane) -> Arc<TileBlob> {
    root.wait_data().unwrap().tiles[&key(plane)]
        .wait_backing()
        .unwrap()
}

fn assert_original_and_properties(a: &Document, b: &Document) {
    assert_eq!((a.composition().size,&a.working),(b.composition().size,&b.working));
    assert_eq!(a.artwork.occurrences,b.artwork.occurrences);
    assert_eq!(a.artwork.stacks,b.artwork.stacks);
    assert_eq!(a.artwork.effects,b.artwork.effects);
    assert!(Arc::ptr_eq(paint(a,2).base.as_ref().unwrap().image.storage(),paint(b,2).base.as_ref().unwrap().image.storage()));
    assert_eq!(paint(a,1).base.as_ref().unwrap().image.storage().extent,paint(b,1).base.as_ref().unwrap().image.storage().extent);
    for (handle,id,old) in a.artwork.paint.iter() {
        assert_eq!(b.artwork.paint.id(handle),Some(id));
        let mut metadata=b.artwork.paint.get(handle).unwrap().clone(); metadata.raster=old.raster.clone();metadata.base=old.base.clone();assert_eq!(*old,metadata);
    }
    for (handle,id,old) in a.artwork.coverage.iter() {
        assert_eq!(b.artwork.coverage.id(handle),Some(id));
        let mut metadata=b.artwork.coverage.get(handle).unwrap().clone(); metadata.raster=old.raster.clone();assert_eq!(*old,metadata);
    }
}

#[test]
fn assignment_preserves_every_code_and_shared_backing_in_all_eight_modes() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
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
                    prepared.document.composition().color,
                    DocumentColor {
                        space: target,
                        depth
                    }
                );
                assert_eq!(prepared.statistics.clipped_channels, 0);
                assert_original_and_properties(&project, &prepared.document);
                let a = &project;
                let b = &prepared.document;
                assert_eq!(paint(a,0).raster.identity(), paint(b,0).raster.identity());
                assert_eq!(
                    coverage(a,0).raster.identity(),
                    coverage(b,0).raster.identity()
                );
                let source = paint(b,1).base.as_ref().unwrap().image.storage();
                assert_eq!(source.interpretation.profile, ColorProfile::Builtin(target));
                for (coordinate, blob) in &paint(a,1).base.as_ref().unwrap().image.storage().tiles {
                    assert!(Arc::ptr_eq(blob, &source.tiles[coordinate]));
                }
            }
        }
    }
}

#[test]
fn depth_changes_rescale_all_codes_in_color_alpha_mask_and_both_wetness_planes() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let project = fixture(DocumentColor { space, depth });
            let target = if depth == SampleDepth::U8 {
                SampleDepth::U16
            } else {
                SampleDepth::U8
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
            assert_original_and_properties(&project, &prepared.document);
            assert_eq!(prepared.statistics.clipped_channels, 0);
            let document = &prepared.document;
            for plane in [
                RasterPlane::Color,
                RasterPlane::Mask,
                RasterPlane::WatercolorWetness,
            ] {
                let root = if plane == RasterPlane::Mask {
                    &coverage(document,0).raster
                } else {
                    &paint(document,0).raster
                };
                let blob = backing(root, plane);
                let bytes = blob.decode().unwrap();
                let original = samples(depth, if plane == RasterPlane::Color { 4 } else { 1 });
                for (a, b) in original
                    .chunks_exact(depth.bytes())
                    .zip(bytes.chunks_exact(target.bytes()))
                {
                    if target == SampleDepth::U16 {
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
            let rgba = backing(&paint(document,0).raster, RasterPlane::Color);
            let source = paint(document,1).base.as_ref().unwrap().image.storage();
            assert_eq!(source.interpretation.depth, target);
            assert!(source.tiles.values().all(|blob| Arc::ptr_eq(blob, &rgba)));
            let wetness = backing(&paint(document,0).raster, RasterPlane::WatercolorWetness);
            assert!(Arc::ptr_eq(
                &wetness,
                &backing(&coverage(document,0).raster, RasterPlane::Mask)
            ));
        }
    }
}

#[test]
fn conversion_matches_f64_coordinates_within_one_code_and_keeps_exact_alpha() {
    for depth in [SampleDepth::U8, SampleDepth::U16] {
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
                assert_original_and_properties(&project, &prepared.document);
                let bytes = backing(
                    &paint(&prepared.document,0).raster,
                    RasterPlane::Color,
                )
                .decode()
                .unwrap();
                let read = |bytes: &[u8]| {
                    if depth == SampleDepth::U8 {
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
                    coverage(&project,0).raster
                        .identity(),
                    coverage(&prepared.document,0).raster
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
        depth: SampleDepth::U16,
    });
    let change = DocumentColorChange::Depth {
        depth: SampleDepth::U8,
        dither: OutputDither::Stochastic8,
    };
    let a = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let b = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let source_a = paint(&a.document,1).base.as_ref().unwrap().image.storage();
    let source_b = paint(&b.document,1).base.as_ref().unwrap().image.storage();
    assert_eq!(source_a.extent,source_b.extent);
    assert_eq!(source_a.resolution,source_b.resolution);
    assert_eq!(source_a.interpretation,source_b.interpretation);
    assert_eq!(source_a.tiles.keys().collect::<Vec<_>>(),source_b.tiles.keys().collect::<Vec<_>>());
    for (coordinate,a) in &source_a.tiles {
        let b=&source_b.tiles[coordinate];
        assert_ne!(a.resource_id(),b.resource_id());
        assert_eq!(a.descriptor,b.descriptor);
        assert_eq!(a.decode().unwrap(),b.decode().unwrap());
    }
    assert_ne!(
        source_a.tiles[&[0, 0]].content_digest().unwrap(),
        source_a.tiles[&[1, 0]].content_digest().unwrap()
    );
    assert!(Arc::ptr_eq(
        &source_a.tiles[&[0, 0]],
        &backing(&paint(&a.document,0).raster, RasterPlane::Color)
    ));
    let plain = prepare_document_color(
        &project,
        DocumentColorChange::Depth {
            depth: SampleDepth::U8,
            dither: OutputDither::None,
        },
        LIMIT,
        || false,
    )
    .unwrap();
    assert_eq!(
        backing(&paint(&a.document,0).raster, RasterPlane::WatercolorWetness).content_digest().unwrap(),
        backing(&paint(&plain.document,0).raster, RasterPlane::WatercolorWetness).content_digest().unwrap()
    );
    let bytes = backing(&paint(&plain.document,0).raster, RasterPlane::Color)
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
        depth: SampleDepth::U16,
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
        &paint(&relative.document,0).raster,
        RasterPlane::Color,
    )
    .decode()
    .unwrap();
    let absolute = backing(
        &paint(&absolute.document,0).raster,
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
fn apply_and_history_restore_exact_roots_sources_properties_and_checkpoints() {
    let project = fixture(DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    });
    for change in [
        DocumentColorChange::Assign(RgbSpace::AdobeRgb),
        DocumentColorChange::Convert {
            space: RgbSpace::Srgb,
            options: Default::default(),
        },
        DocumentColorChange::Depth {
            depth: SampleDepth::U8,
            dither: OutputDither::Stochastic8,
        },
    ] {
        let prepared = prepare_document_color(&project, change, LIMIT, || false).unwrap();
        let mut editor = Editor::new(project.clone());
        editor.validate_edit(&edit(&project, &prepared)).unwrap();
        assert_eq!(editor.document(), &project);
        assert!(!editor.can_undo());
        let transition = editor
            .prepare_color_transition(ColorTransition::Apply {
                edit: Box::new(edit(&project, &prepared)),
            })
            .unwrap();
        assert_eq!(transition.document(), &prepared.document);
        editor
            .commit_color_transition::<layer_core::DocumentError>(transition, |candidate| {
                assert_eq!(candidate, &prepared.document);
                Ok(())
            })
            .unwrap();
        assert_eq!(editor.document(), &prepared.document);
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
            restored.revision = project.revision;
            assert_eq!(restored, project);
            let transition = editor
                .prepare_color_transition(ColorTransition::Redo)
                .unwrap();
            editor
                .commit_color_transition::<layer_core::DocumentError>(transition, |_| Ok(()))
                .unwrap();
            assert!(!editor.can_redo());
            assert_eq!(editor.checkpoint(), checkpoint);
            let mut restored = editor.document().clone();
            restored.revision = prepared.document.revision;
            assert_eq!(restored, prepared.document);
        }
    }
}

#[test]
fn cancellation_limits_and_invalid_candidates_leave_document_and_history_intact() {
    let project = fixture(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let snapshot = project.clone();
    let change = DocumentColorChange::Depth {
        depth: SampleDepth::U8,
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
    assert_eq!(project, snapshot);
    let prepared = prepare_document_color(&project, change, LIMIT, || false).unwrap();
    let editor = Editor::new(project.clone());
    for bad in 0..7 {
        let (mut paint_changes, coverage_changes) = changes(&prepared.document);
        let result = if bad == 0 {
            let handle = project.scene().order()[0];
            let mut occurrence = project.scene().occurrence(handle).unwrap().clone();
            occurrence.opacity = 0.25;
            let supplied = Edit::Batch(vec![edit(&project, &prepared), Edit::Occurrence(
                RecordChange::replace(&project.artwork.occurrences, handle, Some(occurrence)).unwrap()
            )]);
            editor.prepare_color_transition(ColorTransition::Apply {edit:Box::new(supplied)}).map(|_| ())
        } else {
            match bad {
                1 => {paint_changes.remove(1);}
                2 => paint_changes[0].value.as_mut().unwrap().raster = paint(&project,0).raster.clone(),
                3 => paint_changes[0].value.as_mut().unwrap().raster = RasterRevision::pending(),
                4 => paint_changes[0].value.as_mut().unwrap().raster = RasterRevision::default(),
                5 => {
                    let value = paint_changes[0].value.as_mut().unwrap();
                    let mut data = (*value.raster.wait_data().unwrap()).clone();
                    data.watercolor = None;
                    value.raster = RasterRevision::backed(data);
                }
                6 => paint_changes[2].value.as_mut().unwrap().base = paint_changes[1].value.as_ref().unwrap().base.clone(),
                _ => unreachable!(),
            }
            project.color_edit(prepared.document.composition().color,paint_changes,coverage_changes).map(|_| ())
        };
        assert!(result.is_err(), "case {bad}");
        assert_eq!(editor.document(), &snapshot);
        assert_eq!(editor.checkpoint(), 0);
        assert!(!editor.can_undo());
        assert!(!editor.can_redo());
    }
    // Cancellation must interrupt a not-yet-published raster, too.
    let mut pending = project.clone();
    paint_mut(&mut pending,0).raster = RasterRevision::pending();
    let mut checks = 0;
    let error = prepare_document_color(&pending, change, LIMIT, || {
        checks += 1;
        checks == 5
    })
    .err()
    .unwrap();
    assert!(error.contains("cancelled"), "{error}");
    assert!(paint(&pending,0).raster.try_data().is_none());
}

#[test]
fn float32_depth_promotion_is_exact_demotion_and_cancel_are_atomic() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F16 };
    let mut document = Document::new(PortableId::random(), 256, 256, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }); let composition=document.artwork.compositions.get_mut(document.artwork.root).unwrap(); composition.color=color; composition.blend=composition.blend.for_depth(color.depth);
    let half: Vec<_> = (0..65536u32).flat_map(|i| {
        let v = layer_core::color::f16::from_bits(i as u16).to_f32();
        let v = if v.is_finite() { v } else { 0. };
        layer_core::color::hdr::encode_pixel([v,-v,0.12345, if i%2==0 {0.} else {1.}]).unwrap()
    }).flat_map(u16::to_le_bytes).collect();
    let rgba = Arc::new(TileBlob::encode(color.paint_descriptor(), &half).unwrap());
    let mask = Arc::new(TileBlob::encode(color.coverage_descriptor(), &vec![123; 65536*2]).unwrap());
    paint_mut(&mut document,0).raster = RasterRevision::backed(RasterData { tiles: [(key(RasterPlane::Color), RasterTile::backed_shared(rgba)), (key(RasterPlane::WatercolorWetness), RasterTile::backed_shared(mask.clone()))].into(), watercolor: Some(RasterWatercolor { wet_edge: 0.25, burnt_edge: 0.125, edge_width: 3. }) });
    let project = document;
    let promote = DocumentColorChange::Depth { depth: SampleDepth::F32, dither: OutputDither::None };
    let result = prepare_document_color(&project, promote, LIMIT, || false).unwrap();
    let root = paint(&result.document,0).raster.wait_data().unwrap();
    let output = root.tiles[&key(RasterPlane::Color)].wait_backing().unwrap().decode().unwrap();
    for (input, output) in half.chunks_exact(2).zip(output.chunks_exact(4)) {
        assert_eq!(layer_core::color::f16::from_bits(u16::from_le_bytes(input.try_into().unwrap())).to_f32().to_bits(), u32::from_le_bytes(output.try_into().unwrap()));
    }
    assert!(Arc::ptr_eq(&root.tiles[&key(RasterPlane::WatercolorWetness)].wait_backing().unwrap(), &mask));
    let mut editor = Editor::new(project.clone());
    editor.perform(edit(&project, &result)).unwrap(); editor.undo().unwrap();
    assert_eq!(editor.document().composition().color, color); editor.redo().unwrap();
    assert_eq!(editor.document().composition().color.depth, SampleDepth::F32);
    let demote = DocumentColorChange::Depth { depth: SampleDepth::F16, dither: OutputDither::None };
    let narrowed = prepare_document_color(&result.document, demote, LIMIT, || false).unwrap();
    assert_eq!(paint(&narrowed.document,0).raster.wait_data().unwrap().tiles[&key(RasterPlane::Color)].wait_backing().unwrap().decode().unwrap(), half);
    assert!(prepare_document_color(&project, promote, LIMIT, || true).is_err());
    let mut wide = result.document.clone();
    let bytes: Vec<_> = (0..65536).flat_map(|_| [100000.,-100000.,1.,1.]).flat_map(f32::to_le_bytes).collect();
    let descriptor=wide.composition().color.paint_descriptor();
    paint_mut(&mut wide,0).raster = RasterRevision::backed(RasterData { tiles: [(key(RasterPlane::Color), RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()))].into(), watercolor: None });
    assert!(prepare_document_color(&wide, demote, LIMIT, || false).err().unwrap().contains("range"));
}

#[test]
fn converting_to_float_blends_in_linear_light_in_the_same_step() {
    let color = DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::U16 };
    let mut project = fixture(color);
    project.artwork.compositions.get_mut(project.artwork.root).unwrap().blend = layer_core::BlendSpace::Perceptual;
    let promote = DocumentColorChange::Depth { depth: SampleDepth::F32, dither: OutputDither::None };
    let result = prepare_document_color(&project, promote, LIMIT, || false).unwrap();
    assert_eq!(result.document.composition().blend, layer_core::BlendSpace::Linear);
    let mut editor = Editor::new(project.clone());
    editor.perform(edit(&project, &result)).unwrap();
    assert_eq!(editor.document().composition().color.depth, SampleDepth::F32);
    assert_eq!(editor.document().composition().blend, layer_core::BlendSpace::Linear);
    assert!(editor.undo().unwrap());
    assert_eq!((editor.document().composition().color, editor.document().composition().blend), (color, layer_core::BlendSpace::Perceptual));
    assert!(!editor.can_undo());
    assert!(editor.redo().unwrap());
    assert_eq!(editor.document().composition().blend, layer_core::BlendSpace::Linear);
    let mut composition=editor.document().composition().clone(); composition.blend=layer_core::BlendSpace::Perceptual;
    let change=RecordChange::replace(&editor.document().artwork.compositions,editor.document().artwork.root,Some(composition)).unwrap();
    assert!(editor.perform(Edit::Composition(change)).unwrap_err().to_string().contains("linear light"));
}

#[test]
fn editable_color_admission_rejects_invalid_original_profiles_without_mutation() {
    let mut document=fixture(DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8});
    let base=paint_mut(&mut document,2).base.as_mut().unwrap();
    let mut source=base.image.as_ref().clone();
    source.interpretation.profile=ColorProfile::Icc(vec![0;128].into());
    base.image=Arc::new(source).into();
    let before=document.clone();
    assert!(validate_document_color(&document).is_err());
    assert!(prepare_document_color(&document,DocumentColorChange::Assign(RgbSpace::DisplayP3),LIMIT,||false).is_err());
    assert_eq!(document,before);
    assert!(Arc::ptr_eq(paint(&document,2).base.as_ref().unwrap().image.storage(),paint(&before,2).base.as_ref().unwrap().image.storage()));
}

#[test]
fn conversion_uses_unplaced_source_domain_outside_the_composition() {
    let mut document=fixture(DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8});
    let color=document.composition().color;
    let coordinate=[2,0];
    let blob=Arc::new(TileBlob::encode(color.paint_descriptor(),&samples(SampleDepth::U8,4)).unwrap());
    let handle=document.artwork.paint.insert(PortableId::random(),PaintSource { color_mode:Default::default(),
        domain:[768,256], base:None, operations:Default::default(),
        raster:RasterRevision::backed(RasterData {watercolor:None,tiles:[(TileKey {plane:RasterPlane::Color,coordinate},RasterTile::backed_shared(blob))].into()}),
    }).unwrap();
    let result=prepare_document_color(&document,DocumentColorChange::Depth {depth:SampleDepth::U16,dither:OutputDither::None},LIMIT,||false).unwrap();
    let converted=result.document.artwork.paint.get(handle).unwrap();
    assert_eq!(converted.domain,[768,256]);
    let data=converted.raster.wait_data().unwrap();
    let tile=data.tiles[&TileKey {plane:RasterPlane::Color,coordinate}].wait_backing().unwrap();
    assert_eq!(tile.descriptor,result.document.composition().color.paint_descriptor());
    let output=tile.decode().unwrap();
    for (input,output) in samples(SampleDepth::U8,4).iter().zip(output.chunks_exact(2)) {
        assert_eq!(u16::from_le_bytes(output.try_into().unwrap()),u16::from(*input)*257);
    }
}

#[test]
fn fixed_reduced_color_layers_preserve_modes_and_alpha_through_document_depth_changes() {
    let bytes: Arc<[u8]> = Arc::from(include_bytes!("../../../layer-core/src/package/codec/fixtures/layer-color-modes.capy").as_slice());
    let source = layer_core::package::ImmutableBacking::new(Arc::new(bytes)).unwrap();
    let layer_core::package::codec::OpenOutcome::Candidate { artwork, .. } = layer_core::package::codec::open(source, Default::default(), &std::sync::atomic::AtomicBool::new(false)).unwrap() else { panic!("editable fixture") };
    let doc = Document::from_artwork(artwork).unwrap();
    for depth in [SampleDepth::U8, SampleDepth::F16, SampleDepth::F32] {
        let prepared = prepare_document_color(&doc, DocumentColorChange::Depth { depth, dither: OutputDither::None }, LIMIT, || false).unwrap();
        let mut edited = doc.clone(); edited.apply(edit(&doc, &prepared)).unwrap();
        for ((_, _, before), (_, _, after)) in doc.artwork.paint.iter().zip(edited.artwork.paint.iter()) {
            assert_eq!(before.color_mode, after.color_mode);
            let tile = after.raster.wait_data().unwrap().tiles[&key(RasterPlane::Color)].wait_backing().unwrap();
            assert_eq!(tile.descriptor.channels, 2); assert_eq!(tile.descriptor.depth(), depth);
            let data = tile.decode().unwrap();
            if depth == SampleDepth::U8 { assert_eq!(&data[..2], if after.color_mode == LayerColorMode::TwoTone { &[255,255] } else { &[64,128] }); }
            else { let values = hdr::decode_samples(depth, &data[..2 * depth.bytes()]).unwrap(); assert_eq!(values[0], values[1]); assert!((values[3] - if after.color_mode == LayerColorMode::TwoTone { 1. } else { 32768. / 65535. }).abs() < 0.001); }
        }
    }
}

#[test]
fn working_base_conversion_preserves_shared_object_image_and_base_dither_phase() {
    let mut document=fixture(DocumentColor {space:RgbSpace::ProPhoto,depth:SampleDepth::U16});
    let shared=paint(&document,1).base.as_ref().unwrap().image.clone();
    let object=document.artwork.objects.insert(PortableId::random(),ImageObject::new(shared.clone(),"Shared photo")).unwrap();
    let layer=document.artwork.object_layers.insert(PortableId::random(),ObjectLayer {children:vec![object]}).unwrap();
    let occurrence=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Objects(layer),"Object")).unwrap();
    let stack=document.composition().result;
    document.artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let shifted=PaintSource { color_mode:Default::default(),domain:[544,272],raster:Default::default(),base:Some(PaintBase {image:shared.clone(),offset:[17,9],policy:PaintBasePolicy::WorkingPixels}),operations:Default::default()};
    let shifted=document.artwork.paint.insert(PortableId::random(),shifted).unwrap();
    let occurrence=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(shifted),"Shifted base")).unwrap();
    document.artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let document=Document::from_artwork(document.artwork).unwrap();
    for change in [
        DocumentColorChange::Assign(RgbSpace::Srgb),
        DocumentColorChange::Convert {space:RgbSpace::Srgb,options:Default::default()},
        DocumentColorChange::Depth {depth:SampleDepth::U8,dither:OutputDither::Stochastic8},
    ] {
        let prepared=prepare_document_color(&document,change,LIMIT,||false).unwrap();
        let artwork=&prepared.document.artwork;
        let original=&artwork.objects.get(object).unwrap().image;
        assert_eq!(original.id(),shared.id());
        assert!(original.same_owner(&shared));
        let base=paint(&prepared.document,1).base.as_ref().unwrap();
        let shifted=artwork.paint.get(shifted).unwrap().base.as_ref().unwrap();
        assert_eq!(shifted.offset,[17,9]);
        assert_ne!(base.image.id(),shared.id());
        if matches!(change,DocumentColorChange::Depth {dither:OutputDither::Stochastic8,..}) {
            assert_ne!(base.image.id(),shifted.image.id());
            assert_ne!(base.image.tiles[&[0,0]].decode().unwrap(),shifted.image.tiles[&[0,0]].decode().unwrap());
            let old=shared.tiles[&[0,0]].decode().unwrap();
            let mut expected=vec![0;prepared.document.composition().color.paint_descriptor().byte_len([TILE_SIZE;2]).unwrap()];
            let decoder=WorkingDecoder::new(&shared.interpretation,document.composition().color.space,Default::default()).unwrap();
            let destination=&shifted.image.interpretation;
            let encoder=WorkingEncoder::new(document.composition().color.space,destination,change.encoding()).unwrap();
            let mut linear=vec![[0.;4];TILE_SIZE as usize];
            let old_stride=TILE_SIZE as usize*shared.interpretation.pixel_bytes();
            for y in 0..TILE_SIZE as usize {
                decoder.decode_pixels(&old[y*old_stride..(y+1)*old_stride],&mut linear).unwrap();
                encoder.encode_straight(&linear,&mut expected[y*TILE_SIZE as usize*4..(y+1)*TILE_SIZE as usize*4],None,[17,9+y as u32]).unwrap();
            }
            assert_eq!(shifted.image.tiles[&[0,0]].decode().unwrap(),expected);
        } else {
            assert_eq!(base.image.id(),shifted.image.id());
            assert!(base.image.same_owner(&shifted.image));
        }
    }
}

#[test]
fn worker_color_roundtrip_interns_mixed_immutable_image_closure() {
    use layer_core::package::transfer::{PreparedTransfer,TransferReceiver};
    use std::sync::atomic::AtomicBool;
    let cancelled=AtomicBool::new(false);
    let source=|value:u8| {
        let interpretation=SourceInterpretation {channels:SourceChannels::Rgba,depth:SampleDepth::U8,profile:ColorProfile::Builtin(RgbSpace::Srgb),profile_assumed:false};
        let bytes=[value,29,81,255].repeat((TILE_SIZE*TILE_SIZE) as usize);
        Arc::new(SourceImage {extent:[19,11],resolution:None,tiles:[([0,0],Arc::new(TileBlob::encode(interpretation.descriptor(),&bytes).unwrap()))].into(),interpretation})
    };
    let roundtrip=|artwork:layer_core::Artwork| {
        let capture=Editor::new(Document::from_artwork(artwork).unwrap()).capture(0,Default::default()).unwrap();
        let prepared=PreparedTransfer::capture(&capture,&cancelled).unwrap();
        let descriptor=serde_json::from_slice(&serde_json::to_vec(prepared.descriptor()).unwrap()).unwrap();
        let mut receiver=TransferReceiver::new(descriptor,Default::default()).unwrap();
        for index in 0..prepared.payload_count() {
            let mut offset=0;let length=prepared.payload_len(index).unwrap();
            while offset<length {
                let size=(length-offset).min(layer_core::package::MAX_RANGE_BYTES as u64) as usize;
                receiver.push_chunk(index,&prepared.read_chunk(index,offset,size).unwrap()).unwrap();offset+=size as u64;
            }
        }
        receiver.finish().unwrap().adopt_verified(Default::default(),&cancelled).unwrap().artwork.as_ref().clone()
    };
    let mut original=Document::new(PortableId::random(),19,11,layer_core::DocumentNames {paint:"paint".into(),paper:"paper".into()});
    let layer_core::SourceTarget::Paint(paint)=original.working.target.unwrap() else {panic!("paint")};
    let image=Image::new(source(17));
    original.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(image.clone()));
    let (layer,edit)=original.create_object_layer_edit("objects",None,0).unwrap();original.apply(edit).unwrap();
    for index in 0..2 {let (_,edit)=original.add_image_object_edit(layer,ImageObject::new(image.clone(),"shared"),index).unwrap();original.apply(edit).unwrap();}
    original.artwork=roundtrip(original.artwork.clone());
    let image=original.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.clone();
    for policy in [PaintBasePolicy::SourceProfile,PaintBasePolicy::WorkingPixels] {
      let mut original=original.clone();original.artwork.paint.get_mut(paint).unwrap().base.as_mut().unwrap().policy=policy;
      for change in [DocumentColorChange::Assign(RgbSpace::Srgb),DocumentColorChange::Assign(RgbSpace::DisplayP3),DocumentColorChange::Convert {space:RgbSpace::DisplayP3,options:Default::default()}] {
        let outgoing=color_job_artwork(&original.artwork);
        assert_eq!(outgoing.paint.get(paint).unwrap().base.is_none(),policy==PaintBasePolicy::SourceProfile);
        let worker=Document::from_artwork(roundtrip(outgoing)).unwrap();
        let converted=prepare_document_color(&worker,change,LIMIT,||false).unwrap();
        let candidate=adopt_color_job_artwork(&original,roundtrip(converted.document.artwork)).unwrap();
        assert_eq!(candidate.owner,original.owner);assert_eq!(candidate.revision,original.revision);assert_eq!(candidate.working,original.working);
        let incoming=candidate.artwork.clone();
        for (_,_,object) in incoming.objects.iter() {assert!(object.image.same_owner(&image));}
        let base=&incoming.paint.get(paint).unwrap().base.as_ref().unwrap().image;
        if policy==PaintBasePolicy::SourceProfile || candidate.composition().color==original.composition().color {assert!(base.same_owner(&image));assert_eq!(incoming.images().unwrap().len(),1);}
        else {assert_ne!(base.id(),image.id());assert!(!base.same_owner(&image));assert_eq!(incoming.images().unwrap().len(),2);}
        PreparedTransfer::capture(&Editor::new(candidate).capture(0,Default::default()).unwrap(),&cancelled).unwrap();
        let mut replacement=incoming.clone();
        let handle=replacement.objects.iter().next().unwrap().0;
        let fresh=Image::new(image.storage().clone());let fresh_id=fresh.id();replacement.objects.get_mut(handle).unwrap().image=fresh;
        replacement.intern_images_from(&original.artwork).unwrap();assert_eq!(replacement.objects.get(handle).unwrap().image.id(),fresh_id);
        let mut conflict=incoming;let handle=conflict.objects.iter().next().unwrap().0;
        conflict.objects.get_mut(handle).unwrap().image=Image::with_id(image.id(),source(0));
        assert!(conflict.intern_images_from(&original.artwork).is_err());
    }
    }
}
