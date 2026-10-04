use super::*;
use crate::{
    Affine, BlendSpace, Document, Edit, Editor, LayerPlacement, ProjectLimits, SelectionShape,
    color::{ProofRecipe, source::SourceBuilder},
};

fn native(color: DocumentColor) -> Artwork {
    let mut artwork = Artwork::new([512, 256]).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    let tile = |plane: RasterPlane| {
        let descriptor = plane.descriptor(color);
        let bytes = (0..65536u32)
            .flat_map(|i| {
                (0..descriptor.channels).flat_map(move |c| {
                    let code = if c == 3 { color.depth.maximum() } else { i.wrapping_mul(c as u32 * 112 + 1) };
                    (code as u16).to_le_bytes().into_iter().take(color.depth.bytes())
                })
            })
            .collect::<Vec<_>>();
        RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap())
    };
    let color_tile = tile(RasterPlane::Color);
    let paint = artwork
        .paint
        .insert(
            identity(10),
            PaintSource {
                domain: [512, 256],
                original: None,
                operations: Arc::default(),
                raster: RasterRevision::backed(RasterData {
                    tiles: [
                        (TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, color_tile.clone()),
                        (TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }, color_tile),
                        (TileKey { plane: RasterPlane::Wetness, coordinate: [0, 0] }, tile(RasterPlane::Wetness)),
                        (TileKey { plane: RasterPlane::WatercolorWetness, coordinate: [1, 0] }, tile(RasterPlane::WatercolorWetness)),
                    ]
                    .into(),
                    watercolor: None,
                }),
            },
        )
        .unwrap();
    let mask = artwork
        .coverage
        .insert(
            identity(12),
            CoverageSource {
                domain: [512, 256],
                initial: None,
                default_coverage: 1.,
                operations: Arc::default(),
                raster: RasterRevision::backed(RasterData {
                    tiles: [(TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] }, tile(RasterPlane::Mask))].into(),
                    watercolor: None,
                }),
            },
        )
        .unwrap();
    let mut occurrence = Occurrence::new(OccurrenceContent::Paint(paint), "Current ink");
    occurrence.mask = Some(MaskUse {
        source: mask,
        enabled: true,
        linked: true,
        inverted: false,
        translation: Point::default(),
        placement: crate::Projective::IDENTITY,
    });
    let occurrence = artwork.occurrences.insert(identity(20), occurrence).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    artwork
}
fn exact_rasters(expected: &Artwork, actual: &Artwork) {
    let expected = Document::from_artwork(expected.clone()).unwrap();
    let actual = Document::from_artwork(actual.clone()).unwrap();
    for ((_, id, source), (_, actual_id, restored)) in expected.artwork.paint.iter().zip(actual.artwork.paint.iter()) {
        assert_eq!(id, actual_id);
        exact_tiles(&source.raster, &restored.raster);
    }
    for ((_, id, source), (_, actual_id, restored)) in expected.artwork.coverage.iter().zip(actual.artwork.coverage.iter()) {
        assert_eq!(id, actual_id);
        exact_tiles(&source.raster, &restored.raster);
    }
}
fn exact_tiles(expected: &RasterRevision, actual: &RasterRevision) {
    let expected = expected.wait_data().unwrap();
    let actual = actual.wait_data().unwrap();
    assert_eq!(expected.tiles.len(), actual.tiles.len());
    assert_eq!(expected.watercolor, actual.watercolor);
    for (key, expected) in &expected.tiles {
        let expected = expected.wait_backing().unwrap();
        let actual = actual.tiles[key].wait_backing().unwrap();
        assert_eq!(actual.descriptor, expected.descriptor);
        assert_eq!(actual.content_digest().unwrap(), expected.content_digest().unwrap());
        assert_eq!(actual.decode().unwrap(), expected.decode().unwrap());
    }
}
#[test]
fn literal_choices_and_pixels_resave_unchanged() {
    let mut artwork=fixture(SampleDepth::U16);
    let original_program=crate::bundled_effect_catalog().get("curves").unwrap().program();
    let mut program=(*original_program).clone();program.label=crate::ResourceLabel::from("Curves");
    let parameter=Arc::make_mut(&mut program.parameters).iter_mut().find(|p|p.key.as_ref()=="domain").unwrap();
    let crate::EffectParameterKind::Choice{options}=&mut parameter.kind else{panic!()};
    for option in Arc::make_mut(options){*option=crate::EffectOption::Literal(option.value().into());}
    let mut effect=EffectInstance::new(Arc::new(program));effect.set("domain",EffectValue::Choice(1)).unwrap();
    let definition=artwork.definitions.insert(identity(90),Definition{program:effect.program,dimensions:Default::default()}).unwrap();
    let application=artwork.effects.insert(identity(91),EffectApplication{definition,values:effect.values.clone(),domain:[256;2]}).unwrap();
    let occurrence=artwork.occurrences.insert(identity(92),Occurrence::new(OccurrenceContent::Effect(application),"  My curves { $name } 한글 🎨  ")).unwrap();
    let root=artwork.compositions.get(artwork.root).unwrap().result;artwork.stacks.get_mut(root).unwrap().entries.insert(0,occurrence);
    let bytes=serialize(&prepare(&artwork,false));let reopened=editable(bytes.clone());
    assert_eq!(serialize(&prepare(&reopened,false)),bytes);
    exact_rasters(&artwork,&reopened);
    let reopened=Document::from_artwork(reopened).unwrap();let restored=reopened.artwork.occurrences.resolve(identity(92)).unwrap();
    assert_eq!(reopened.scene().occurrence(restored).unwrap().name,artwork.occurrences.get(occurrence).unwrap().name);
    assert_eq!(reopened.scene().effect(restored).unwrap().values,effect.values);
    let mut explicit=artwork.clone();
    let program=&mut explicit.definitions.get_mut(definition).unwrap().program;
    let parameter=Arc::make_mut(&mut Arc::make_mut(program).parameters).iter_mut().find(|p|p.key.as_ref()=="domain").unwrap();
    parameter.kind=original_program.parameters.iter().find(|p|p.key.as_ref()=="domain").unwrap().kind.clone();
    let bytes=serialize(&prepare(&explicit,false));let reopened=editable(bytes.clone());
    assert_eq!(serialize(&prepare(&reopened,false)),bytes);
    exact_rasters(&explicit,&reopened);
    assert_eq!(reopened.effects.get(reopened.effects.resolve(identity(91)).unwrap()).unwrap().values,effect.values);
}
#[test]
fn native_sdr_archives_preserve_space_depth_every_code_and_scalar_planes() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let color = DocumentColor { space, depth };
            let artwork = native(color);
            let bytes = serialize(&prepare(&artwork, false));
            let restored = editable(bytes.clone());
            assert_eq!(restored.compositions.get(restored.root).unwrap().color, color);
            exact_rasters(&artwork, &restored);
            let data = restored.paint.get(restored.paint.resolve(identity(10)).unwrap()).unwrap().raster.wait_data().unwrap();
            assert!(
                data.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }]
                    .same_capture(&data.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }])
            );
            assert_eq!(serialize(&prepare(&restored, false)), bytes);
        }
    }
}
#[test]
fn native_sdr_archives_reject_depth_mismatch() {
    let mut artwork = native(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    artwork.compositions.get_mut(artwork.root).unwrap().color.depth = SampleDepth::U8;
    assert!(PreparedPackage::prepare(&capture(&artwork), None, &AtomicBool::new(false)).is_err());
    artwork.compositions.get_mut(artwork.root).unwrap().color.depth = SampleDepth::U16;
    let h = artwork.coverage.resolve(identity(12)).unwrap();
    artwork.coverage.get_mut(h).unwrap().raster = RasterRevision::backed(RasterData {
        tiles: [(
            TileKey { plane: RasterPlane::Mask, coordinate: [0, 0] },
            RasterTile::backed(TileBlob::encode(crate::color::PixelDescriptor::COVERAGE8, &vec![0; 65536]).unwrap()),
        )]
        .into(),
        watercolor: None,
    });
    assert!(PreparedPackage::prepare(&capture(&artwork), None, &AtomicBool::new(false)).is_err());
}
#[test]
fn proof_metadata_roundtrips_deduplicates_profile_and_undo_keeps_raster_exact() {
    for (depth, name) in [(SampleDepth::U8, "Printer and paper"), (SampleDepth::U16, "Printer and paper"), (SampleDepth::U8, ""), (SampleDepth::U16, "")] {
        let artwork = native(DocumentColor { space: RgbSpace::ProPhoto, depth });
        let profile = Resource::from((0..1024).map(|i| (i * 13 % 251) as u8).collect::<Vec<_>>());
        let recipe = ProofRecipe::new(name.into(), ColorProfile::Icc(profile.clone()));
        let document = Document::from_artwork(artwork).unwrap();
        let paint = document.artwork.paint.resolve(identity(10)).unwrap();
        let raster = document.artwork.paint.get(paint).unwrap().raster.clone();
        let mut output = document.output().clone();
        output.proof = Some(recipe.clone());
        let edit = Edit::Output(RecordChange::replace(&document.artwork.outputs, document.artwork.default_output, Some(output)).unwrap());
        assert!(!edit.changes_image(&document));
        let mut editor = Editor::new(document);
        editor.perform(edit).unwrap();
        assert_eq!(editor.document().output().proof.as_ref(), Some(&recipe));
        assert_ne!(editor.checkpoint(), 0);
        assert_eq!(editor.document().artwork.paint.get(paint).unwrap().raster.identity(), raster.identity());
        editor.undo().unwrap();
        assert!(editor.document().output().proof.is_none());
        assert_eq!(editor.checkpoint(), 0);
        assert_eq!(editor.document().artwork.paint.get(paint).unwrap().raster.identity(), raster.identity());
        editor.redo().unwrap();
        let mut artwork = editor.document().artwork.clone();
        let mut builder = SourceBuilder::new(
            [1, 1],
            SourceInterpretation { channels: SourceChannels::Rgba, depth, profile: ColorProfile::Icc(profile), profile_assumed: false },
            1024 * 1024,
        )
        .unwrap();
        builder.push_row(&vec![127; 4 * depth.bytes()]).unwrap();
        artwork.paint.get_mut(paint).unwrap().original = Some(Arc::new(builder.finish().unwrap()));
        let prepared = prepare(&artwork, false);
        assert_eq!(prepared.resources().entries.iter().filter(|e| e.record["type"] == "capy.icc/1").count(), 1);
        let bytes = serialize(&prepared);
        let reopened = editable(bytes.clone());
        assert_eq!(reopened.outputs.get(reopened.default_output).unwrap().proof, Some(recipe));
        let ColorProfile::Icc(proof) = &reopened.outputs.get(reopened.default_output).unwrap().proof.as_ref().unwrap().profile else {
            panic!()
        };
        let ColorProfile::Icc(source) =
            &reopened.paint.get(reopened.paint.resolve(identity(10)).unwrap()).unwrap().original.as_ref().unwrap().interpretation.profile
        else {
            panic!()
        };
        assert!(Arc::ptr_eq(proof.storage(), source.storage()));
        exact_rasters(&artwork, &reopened);
        assert_eq!(serialize(&prepare(&reopened, false)), bytes);
    }
}
fn float_artwork(depth: SampleDepth, samples: &[u8]) -> Artwork {
    let mut artwork = Artwork::new([256; 2]).unwrap();
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth };
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    let paint = artwork
        .paint
        .insert(
            identity(10),
            PaintSource {
                domain: [256; 2],
                original: None,
                operations: Arc::default(),
                raster: RasterRevision::backed(RasterData {
                    tiles: [(
                        TileKey { plane: RasterPlane::Color, coordinate: [0, 0] },
                        RasterTile::backed(TileBlob::encode(color.paint_descriptor(), samples).unwrap()),
                    )]
                    .into(),
                    watercolor: None,
                }),
            },
        )
        .unwrap();
    let occurrence = artwork.occurrences.insert(identity(20), Occurrence::new(OccurrenceContent::Paint(paint), "Float master")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    artwork
}
#[test]
fn hdr_archive_and_history_preserve_samples_and_authored_rendition() {
    let samples = (0..65536u32)
        .flat_map(|i| {
            let value = f16::from_bits(i as u16).to_f32();
            let value = if value.is_finite() { value } else { 0. };
            crate::color::hdr::encode_pixel([value, -value, 4., 1.]).unwrap().into_iter().flat_map(u16::to_le_bytes)
        })
        .collect::<Vec<_>>();
    let document = Document::from_artwork(float_artwork(SampleDepth::F16, &samples)).unwrap();
    let rendition = crate::color::hdr::SdrRendition {
        exposure: -1.,
        contrast: 0.8,
        headroom: 4.,
        balance: 0.4,
        highlight_color: 0.35,
    };
    let mut output = document.output().clone();
    output.sdr = rendition;
    let edit = Edit::Output(RecordChange::replace(&document.artwork.outputs, document.artwork.default_output, Some(output)).unwrap());
    let mut editor = Editor::new(document);
    editor.perform(edit).unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.document().output().sdr, Default::default());
    editor.redo().unwrap();
    assert_eq!(editor.document().output().sdr, rendition);
    let bytes = serialize(&prepare(&editor.document().artwork, false));
    let restored = editable(bytes.clone());
    assert_eq!(restored.outputs.get(restored.default_output).unwrap().sdr, rendition);
    exact_rasters(&editor.document().artwork, &restored);
    assert_eq!(serialize(&prepare(&restored, false)), bytes);
}
#[test]
fn float32_archive_history_preserve_every_bit_including_hidden_rgb() {
    let values = [f32::MAX, -f32::MAX, f32::MIN_POSITIVE, f32::from_bits(1), -f32::from_bits(1), -0., 1.0000001, 65505., -1_234_567. - 0.125];
    let samples = (0..65536)
        .flat_map(|i| {
            [
                values[i % values.len()],
                values[(i + 1) % values.len()],
                values[(i + 2) % values.len()],
                [0., f32::from_bits(1), 0.12345679, 1.][i % 4],
            ]
        })
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    let document = Document::from_artwork(float_artwork(SampleDepth::F32, &samples)).unwrap();
    let occurrence = document.artwork.occurrences.resolve(identity(20)).unwrap();
    let mut value = document.artwork.occurrences.get(occurrence).unwrap().clone();
    value.opacity = 0.25;
    let edit = Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, occurrence, Some(value)).unwrap());
    let mut editor = Editor::new(document);
    editor.perform(edit).unwrap();
    editor.undo().unwrap();
    editor.redo().unwrap();
    let restored = editable(serialize(&prepare(&editor.document().artwork, false)));
    exact_rasters(&editor.document().artwork, &restored);
    let descriptor = editor.document().composition().color.paint_descriptor();
    let mut invalid = samples.clone();
    invalid[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(TileBlob::encode(descriptor, &invalid).is_err());
    invalid = samples;
    invalid[12..16].copy_from_slice(&1.0001f32.to_le_bytes());
    assert!(TileBlob::encode(descriptor, &invalid).is_err());
}
fn source_fixture() -> Artwork {
    let mut artwork = Artwork::new([32; 2]).unwrap();
    let interpretation = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U16,
        profile: ColorProfile::Icc(Resource::from((0..256).map(|n| n as u8).collect::<Vec<_>>())),
        profile_assumed: false,
    };
    let mut builder = SourceBuilder::new([257, 259], interpretation, 8 * 1024 * 1024).unwrap();
    for y in 0..259u32 {
        let row = (0..257u32)
            .flat_map(|x| [x.wrapping_mul(255) as u16, y.wrapping_mul(253) as u16, (x ^ y) as u16, if x % 7 == 0 { 0 } else { 65535 }])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        builder.push_row(&row).unwrap();
    }
    let original = Arc::new(builder.finish().unwrap());
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    for n in 0..2 {
        let source = artwork
            .paint
            .insert(
                identity(10 + n),
                PaintSource {
                    domain: [512, 259],
                    original: Some(original.clone()),
                    raster: Default::default(),
                    operations: Arc::default(),
                },
            )
            .unwrap();
        let occurrence = artwork
            .occurrences
            .insert(
                identity(20 + n),
                Occurrence::new(OccurrenceContent::Paint(source), "  Current ink { $name } 漢字 🖌️\u{2068}literal\u{2069}  "),
            )
            .unwrap();
        artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    }
    artwork
}
#[test]
fn persistent_placement_preserves_sources_overrides_and_independent_history() {
    let artwork = source_fixture();
    let document = Document::from_artwork(artwork).unwrap();
    let paint = document.artwork.paint.resolve(identity(10)).unwrap();
    let occurrence = document.artwork.occurrences.resolve(identity(20)).unwrap();
    let original = document.artwork.paint.get(paint).unwrap().original.clone().unwrap();
    let placement = LayerPlacement::from_affine(Affine::around(Point::default(), [1. / 3.; 2], 0.3, Point { x: -45., y: 8. }));
    let mut value = document.artwork.occurrences.get(occurrence).unwrap().clone();
    value.placement = placement.clone();
    let edit = Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, occurrence, Some(value)).unwrap());
    let mut editor = Editor::new(document);
    editor.perform(edit).unwrap();
    assert!(editor.document().artwork.paint.get(paint).unwrap().raster.is_empty());
    assert!(Arc::ptr_eq(editor.document().artwork.paint.get(paint).unwrap().original.as_ref().unwrap(), &original));
    editor.undo().unwrap();
    assert_eq!(editor.document().artwork.occurrences.get(occurrence).unwrap().placement, LayerPlacement::IDENTITY);
    editor.redo().unwrap();
    let mut loaded = editable(serialize(&prepare(&editor.document().artwork, false)));
    let occurrence = loaded.occurrences.resolve(identity(20)).unwrap();
    assert_eq!(loaded.occurrences.get(occurrence).unwrap().placement, placement);
    assert_eq!(loaded.occurrences.get(loaded.occurrences.resolve(identity(21)).unwrap()).unwrap().placement, LayerPlacement::IDENTITY);
    let paint = loaded.paint.resolve(identity(10)).unwrap();
    assert_eq!(loaded.paint.get(paint).unwrap().original.as_ref().unwrap(), &original);
    loaded.occurrences.get_mut(occurrence).unwrap().placement = LayerPlacement::IDENTITY;
    assert_eq!(loaded.paint.get(paint).unwrap().original.as_ref().unwrap(), &original);
    let key = TileKey { plane: RasterPlane::Color, coordinate: [1, 0] };
    let descriptor = loaded.compositions.get(loaded.root).unwrap().color.paint_descriptor();
    let blob = TileBlob::encode(descriptor, &vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()]).unwrap();
    let digest = blob.content_digest().unwrap();
    loaded.paint.get_mut(paint).unwrap().raster =
        RasterRevision::backed(RasterData { tiles: [(key, RasterTile::backed(blob))].into(), watercolor: None });
    let bytes = serialize(&prepare(&loaded, false));
    let reopened = editable(bytes.clone());
    let raster = &reopened.paint.get(reopened.paint.resolve(identity(10)).unwrap()).unwrap().raster;
    assert_eq!(raster.wait_data().unwrap().tiles[&key].wait_backing().unwrap().content_digest().unwrap(), digest);
    assert_eq!(
        reopened.occurrences.get(reopened.occurrences.resolve(identity(20)).unwrap()).unwrap().name,
        loaded.occurrences.get(occurrence).unwrap().name
    );
    assert_eq!(serialize(&prepare(&reopened, false)), bytes);
    let document = Document::from_artwork(reopened).unwrap();
    assert!(document.validate(ProjectLimits { tiles: 1, ..Default::default() }).is_err());
}
#[test]
fn native_sources_preserve_u16_profiles_hidden_rgb_and_shared_ownership() {
    let artwork = source_fixture();
    let document = Document::from_artwork(artwork.clone()).unwrap();
    let original = artwork.paint.get(artwork.paint.resolve(identity(10)).unwrap()).unwrap().original.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        document.snapshot().artwork.paint.get(artwork.paint.resolve(identity(10)).unwrap()).unwrap().original.as_ref().unwrap(),
        original
    ));
    let bytes = serialize(&prepare(&artwork, false));
    let loaded = editable(bytes.clone());
    let a = loaded.paint.get(loaded.paint.resolve(identity(10)).unwrap()).unwrap().original.as_ref().unwrap();
    let b = loaded.paint.get(loaded.paint.resolve(identity(11)).unwrap()).unwrap().original.as_ref().unwrap();
    assert!(Arc::ptr_eq(a, b));
    assert_eq!(a, original);
    for (key, tile) in &a.tiles {
        assert_eq!(tile.decode().unwrap(), original.tiles[key].decode().unwrap());
    }
    assert_eq!(serialize(&prepare(&loaded, false)), bytes);
    let weak = Arc::downgrade(a);
    let mut editor = Editor::new(Document::from_artwork(loaded).unwrap());
    for id in [identity(10), identity(11)] {
        let handle = editor.document().artwork.paint.resolve(id).unwrap();
        let mut value = editor.document().artwork.paint.get(handle).unwrap().clone();
        value.original = None;
        let edit = Edit::Paint(RecordChange::replace(&editor.document().artwork.paint, handle, Some(value)).unwrap());
        editor.perform(edit).unwrap();
    }
    editor.undo().unwrap();
    let paint = editor.document().artwork.paint.resolve(identity(11)).unwrap();
    assert!(Arc::ptr_eq(editor.document().artwork.paint.get(paint).unwrap().original.as_ref().unwrap(), &weak.upgrade().unwrap()));
    editor.redo().unwrap();
    assert!(weak.upgrade().is_some());
    drop(editor);
    assert!(weak.upgrade().is_none());
}
#[test]
fn photo_selection_roundtrips_shared_binary_coverage() {
    let extent = [9504, 6336];
    let mut artwork = Artwork::new(extent).unwrap();
    let pixels = Arc::new(
        SelectionPixels::bytes(extent, [0, 0, extent[0], extent[1]], vec![0xff7f3f01; (extent[0] / 4 * extent[1]) as usize]).unwrap(),
    );
    let selection = Selection { affine: Affine([1., 0., 0., 1., 2., 3.]), inverted: true, ..Selection::pixels(pixels) };
    let saved =
        artwork.selections.insert(identity(13), SavedSelection { selection: selection.clone(), display: Default::default() }).unwrap();
    let occurrence = artwork.occurrences.insert(identity(22), Occurrence::new(OccurrenceContent::Selection(saved), "Saved")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    artwork
        .coverage
        .insert(
            identity(12),
            CoverageSource {
                domain: extent,
                initial: Some(selection.clone()),
                raster: Default::default(),
                default_coverage: 1.,
                operations: Arc::default(),
            },
        )
        .unwrap();
    let bytes = serialize(&prepare(&artwork, false));
    let directory = Directory::read(&mut Cursor::new(&bytes), 262144, 64 * 1024 * 1024).unwrap();
    let metadata_bytes=directory.member("manifest.json").unwrap().length;
    assert!(metadata_bytes < 1024 * 1024, "61MP manifest: {metadata_bytes} bytes");
    eprintln!("61MP authored manifest: {metadata_bytes} bytes; decoded coverage: {} bytes",extent[0] as u64*extent[1] as u64);
    let restored = editable(bytes.clone());
    let saved = &restored.selections.get(restored.selections.resolve(identity(13)).unwrap()).unwrap().selection;
    let mask = restored.coverage.get(restored.coverage.resolve(identity(12)).unwrap()).unwrap().initial.as_ref().unwrap();
    assert_eq!(saved, &selection);
    assert_eq!(mask, &selection);
    let (SelectionShape::Pixels(a), SelectionShape::Pixels(b)) = (&saved.shape, &mask.shape) else { panic!() };
    assert!(Arc::ptr_eq(a, b));
    let limit = a.words().len() as u64 * 4;
    Document::from_artwork(restored).unwrap().validate(ProjectLimits { raster_bytes: limit, ..Default::default() }).unwrap();
    assert!(!matches!(
        open(backing(bytes), ProjectLimits { raster_bytes: 1024, ..Default::default() }, &AtomicBool::new(false)).unwrap(),
        OpenOutcome::Candidate { .. }
    ));
}
#[test]
fn independent_editable_sources_charge_each_raster_instance() {
    let mut artwork = float_artwork(SampleDepth::F32, &paint_samples(SampleDepth::F32));
    let first = artwork.paint.resolve(identity(10)).unwrap();
    let source = artwork.paint.get(first).unwrap().clone();
    let bytes = source.raster.wait_data().unwrap().tiles.values().next().unwrap().descriptor().byte_len([TILE_SIZE; 2]).unwrap() as u64;
    let second = artwork.paint.insert(identity(11), source).unwrap();
    let occurrence = artwork.occurrences.insert(identity(21), Occurrence::new(OccurrenceContent::Paint(second), "Independent")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let document = Document::from_artwork(artwork.clone()).unwrap();
    for limit in [ProjectLimits { raster_bytes: bytes, ..Default::default() }, ProjectLimits { tiles: 1, ..Default::default() }] {
        assert!(document.validate(limit).is_err());
    }
    let restored = Document::from_artwork(editable(serialize(&prepare(&artwork, false)))).unwrap();
    assert!(restored.validate(ProjectLimits { raster_bytes: bytes, ..Default::default() }).is_err());
}
#[test]
fn blend_space_round_trips_and_float_perceptual_is_rejected() {
    let mut artwork = native(DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 });
    artwork.compositions.get_mut(artwork.root).unwrap().blend = BlendSpace::Perceptual;
    let restored = editable(serialize(&prepare(&artwork, false)));
    assert_eq!(restored.compositions.get(restored.root).unwrap().blend, BlendSpace::Perceptual);
    let mut invalid = float_artwork(SampleDepth::F32, &paint_samples(SampleDepth::F32));
    invalid.compositions.get_mut(invalid.root).unwrap().blend = BlendSpace::Perceptual;
    assert!(PreparedPackage::prepare(&capture(&invalid), None, &AtomicBool::new(false)).is_err());
}
#[test]
fn rasterized_image_role_roundtrips_and_rejects_wrong_document_interpretation() {
    let mut artwork = source_fixture();
    let paint = artwork.paint.resolve(identity(10)).unwrap();
    let mut image = (**artwork.paint.get(paint).unwrap().original.as_ref().unwrap()).clone();
    image.kind = SourceKind::Rasterized;
    image.interpretation.profile = ColorProfile::Builtin(RgbSpace::Srgb);
    image.interpretation.profile_assumed = false;
    artwork.compositions.get_mut(artwork.root).unwrap().color.depth = image.interpretation.depth;
    artwork.paint.get_mut(paint).unwrap().original = Some(Arc::new(image.clone()));
    let bytes = serialize(&prepare(&artwork, false));
    let restored = editable(bytes.clone());
    assert_eq!(restored.paint.get(restored.paint.resolve(identity(10)).unwrap()).unwrap().original.as_deref(), Some(&image));
    assert!(restored.paint.get(restored.paint.resolve(identity(11)).unwrap()).unwrap().original.as_ref().unwrap().is_original());
    assert_eq!(serialize(&prepare(&restored, false)), bytes);
    for invalid in [0, 1] {
        let mut changed = artwork.clone();
        let source = changed.paint.get_mut(paint).unwrap().original.as_mut().unwrap();
        let source = Arc::make_mut(source);
        if invalid == 0 {
            source.interpretation.profile_assumed = true;
        } else {
            source.interpretation.depth = SampleDepth::U8;
        }
        assert!(PreparedPackage::prepare(&capture(&changed), None, &AtomicBool::new(false)).is_err());
    }
}
#[test]
fn retained_originals_cannot_bypass_asset_tile_or_dimension_limits() {
    let restored = Document::from_artwork(editable(serialize(&prepare(&source_fixture(), false)))).unwrap();
    for limits in [
        ProjectLimits { asset_bytes: 128, ..Default::default() },
        ProjectLimits { tiles: 3, ..Default::default() },
        ProjectLimits { dimension: 256, ..Default::default() },
    ] {
        assert!(restored.validate(limits).is_err());
    }
}
#[test]
fn filter_blending_space_round_trips() {
    let mut artwork = Artwork::new([256; 2]).unwrap();
    let instance = EffectInstance::new(crate::bundled_effect_catalog().get("gaussian_blur").unwrap().program());
    let definition =
        artwork.definitions.insert(identity(30), Definition { program: instance.program, dimensions: BTreeMap::new() }).unwrap();
    let effect = artwork.effects.insert(identity(40), EffectApplication { definition, values: instance.values, domain: [256; 2] }).unwrap();
    let occurrence = artwork.occurrences.insert(identity(50), Occurrence::new(OccurrenceContent::Effect(effect), "Blur")).unwrap();
    let stack = artwork.compositions.get(artwork.root).unwrap().result;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let restored = editable(serialize(&prepare(&artwork, false)));
    assert_eq!(
        restored.definitions.get(restored.definitions.resolve(identity(30)).unwrap()).unwrap().program.space,
        crate::EffectSpace::Blending
    );
}
