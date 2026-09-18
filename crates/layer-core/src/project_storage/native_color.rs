use super::*;
use crate::color::{DocumentColor, SampleDepth, RgbSpace};

fn project(color: DocumentColor) -> Project {
    let mut document = Document::new("native color archive", 512, 256);
    document.color = color;
    let tile = |plane| {
        let descriptor = RasterPlane::descriptor(plane, color);
        let bytes: Vec<_> = (0..65536u32)
            .flat_map(|i| {
                (0..descriptor.channels).flat_map(move |c| {
                    let code = if c == 3 {
                        color.depth.maximum()
                    } else {
                        i.wrapping_mul(c as u32 * 112 + 1)
                    };
                    (code as u16)
                        .to_le_bytes()
                        .into_iter()
                        .take(color.depth.bytes())
                })
            })
            .collect();
        RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap())
    };
    let color_tile = tile(RasterPlane::Color);
    document.layers[0].raster = RasterRevision::backed(RasterData {
        tiles: BTreeMap::from([
            (
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0],
                },
                color_tile.clone(),
            ),
            (
                TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [1, 0],
                },
                color_tile,
            ),
            (
                TileKey {
                    plane: RasterPlane::Wetness,
                    coordinate: [0, 0],
                },
                tile(RasterPlane::Wetness),
            ),
            (
                TileKey {
                    plane: RasterPlane::WatercolorWetness,
                    coordinate: [1, 0],
                },
                tile(RasterPlane::WatercolorWetness),
            ),
        ]),
        watercolor: None,
    });
    let mut mask = LayerMask::reveal_all(LayerId(document.next_layer_id), Point::default());
    mask.raster = RasterRevision::backed(RasterData {
        tiles: BTreeMap::from([(
            TileKey {
                plane: RasterPlane::Mask,
                coordinate: [0, 0],
            },
            tile(RasterPlane::Mask),
        )]),
        watercolor: None,
    });
    document.layers[0].mask = Some(mask);
    document.next_layer_id += 1;
    Project {
        document,
        assets: Default::default(),
    }
}

#[test]
fn native_sdr_archives_preserve_space_depth_every_code_and_scalar_planes() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16] {
            let color = DocumentColor { space, depth };
            let original = project(color);
            let mut bytes = Vec::new();
            original.write(&mut bytes).unwrap();
            let restored = Project::read(bytes.as_slice(), Default::default()).unwrap();
            assert_eq!(restored.document.color, color);
            let a = &original.document.layers[0];
            let b = &restored.document.layers[0];
            for (old, new) in [
                (&a.raster, &b.raster),
                (
                    &a.mask.as_ref().unwrap().raster,
                    &b.mask.as_ref().unwrap().raster,
                ),
            ] {
                let old = old.wait_data().unwrap();
                let new = new.wait_data().unwrap();
                assert_eq!(old.tiles.len(), new.tiles.len());
                for (key, tile) in &old.tiles {
                    let expected = tile.wait_backing().unwrap();
                    let actual = new.tiles[key].wait_backing().unwrap();
                    assert_eq!(actual.descriptor, key.plane.descriptor(color));
                    assert_eq!(actual.digest, expected.digest);
                    assert!(
                        actual.decode().unwrap() == expected.decode().unwrap(),
                        "{space:?}/{depth:?}/{key:?}"
                    );
                }
            }
            let data = b.raster.wait_data().unwrap();
            assert!(
                data.tiles[&TileKey {
                    plane: RasterPlane::Color,
                    coordinate: [0, 0]
                }]
                    .same_capture(
                        &data.tiles[&TileKey {
                            plane: RasterPlane::Color,
                            coordinate: [1, 0]
                        }]
                    )
            );
            let mut saved_again = Vec::new();
            restored.write(&mut saved_again).unwrap();
            assert!(saved_again == bytes);
        }
    }
}

#[test]
fn native_sdr_archives_reject_depth_mismatch_and_previous_semantics() {
    let mut p = project(DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    });
    let mut bytes = Vec::new();
    p.write(&mut bytes).unwrap();
    bytes[10] = 2;
    assert!(Project::read(bytes.as_slice(), Default::default()).is_err());
    p.document.color.depth = SampleDepth::U8;
    assert!(
        p.write(&mut Vec::new())
            .unwrap_err()
            .contains("representation")
    );
    p.document.color.depth = SampleDepth::U16;
    let mask = p.document.layers[0].mask.as_mut().unwrap();
    mask.raster = RasterRevision::backed(RasterData {
        tiles: BTreeMap::from([(
            TileKey {
                plane: RasterPlane::Mask,
                coordinate: [0, 0],
            },
            RasterTile::backed(
                TileBlob::encode(color::PixelDescriptor::COVERAGE8, &vec![0; 65536]).unwrap(),
            ),
        )]),
        watercolor: None,
    });
    assert!(
        p.write(&mut Vec::new())
            .unwrap_err()
            .contains("representation")
    );
}

#[test]
fn proof_metadata_roundtrips_deduplicates_profile_and_undo_keeps_raster_exact() {
    use crate::color::{ColorProfile, ProofRecipe, source::*};
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let mut p = project(DocumentColor { space: RgbSpace::ProPhoto, depth });
        // Core storage owns exact bytes; CMM validation belongs to layer-color.
        let profile: Arc<[u8]> = (0..1024).map(|i| (i * 13 % 251) as u8).collect::<Vec<_>>().into();
        let recipe = ProofRecipe::new("Printer and paper".into(), ColorProfile::Icc(profile.clone()));
        let original = p.document.clone();
        let raster = original.layers[0].raster.clone();
        let mut editor = Editor::new(original.clone());
        let edit = Edit::SetProof(Some(recipe.clone()));
        assert!(!edit.changes_image());
        editor.perform(edit).unwrap();
        assert_eq!(editor.document().proof.as_ref(), Some(&recipe));
        assert_ne!(editor.checkpoint(), 0);
        assert_eq!(editor.document().layers[0].raster.identity(), raster.identity());
        editor.undo().unwrap();
        assert!(editor.document().proof.is_none());
        assert_eq!(editor.checkpoint(), 0);
        assert_eq!(editor.document().layers, original.layers);
        editor.redo().unwrap();
        p.document = editor.document().clone();
        let mut source = SourceBuilder::new([1, 1], SourceInterpretation {
            channels: SourceChannels::Rgba, depth,
            profile: ColorProfile::Icc(profile.clone()), profile_assumed: false,
        }, 1024 * 1024).unwrap();
        source.push_row(&vec![127; 4 * depth.bytes()]).unwrap();
        p.document.layers[0].source = Some(Arc::new(source.finish().unwrap()));
        let mut bytes = Vec::new();
        p.write(&mut bytes).unwrap();
        assert_eq!(bytes.windows(profile.len()).filter(|v| *v == profile.as_ref()).count(), 1,
            "proof and source share one binary profile payload");
        let reopened = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(reopened.document.proof, p.document.proof);
        let ColorProfile::Icc(proof_profile) = &reopened.document.proof.as_ref().unwrap().profile else { panic!() };
        let ColorProfile::Icc(source_profile) = &reopened.document.layers[0].source.as_ref().unwrap().interpretation.profile else { panic!() };
        assert!(Arc::ptr_eq(proof_profile, source_profile));
        let a = raster.wait_data().unwrap();
        let b = reopened.document.layers[0].raster.wait_data().unwrap();
        for (key, tile) in &a.tiles {
            assert_eq!(tile.wait_backing().unwrap().digest, b.tiles[key].wait_backing().unwrap().digest);
        }
        let mut again = Vec::new();
        reopened.write(&mut again).unwrap();
        assert_eq!(bytes, again);
    }
}

#[test]
fn hdr_archive_and_history_preserve_samples_and_authored_rendition() {
    let color=DocumentColor {space:RgbSpace::DisplayP3,depth:SampleDepth::F16};
    let mut document=Document::new("HDR master",256,256);document.color=color;
    let samples:Vec<_>=(0..65536u32).flat_map(|i| {
        let value=crate::color::f16::from_bits(i as u16).to_f32();
        let value=if value.is_finite(){value}else{0.};
        crate::color::hdr::encode_pixel([value,-value,4.,1.]).unwrap().into_iter().flat_map(u16::to_le_bytes)
    }).collect();
    let key=TileKey{plane:RasterPlane::Color,coordinate:[0,0]};
    document.layers[0].raster=RasterRevision::backed(RasterData{tiles:BTreeMap::from([(key,RasterTile::backed(TileBlob::encode(color.paint_descriptor(),&samples).unwrap()))]),watercolor:None});
    let mut editor=Editor::new(document);
    let rendition=crate::color::hdr::SdrRendition{exposure:-1.,contrast:0.8,headroom:4.,method:crate::color::hdr::SdrMethod::Photographic,highlight_color:0.35};
    editor.perform(Edit::SetSdrRendition(rendition)).unwrap();
    editor.undo().unwrap();assert_eq!(editor.document().sdr_rendition,Default::default());
    editor.redo().unwrap();assert_eq!(editor.document().sdr_rendition,rendition);
    let project=Project{document:editor.document().clone(),assets:Default::default()};
    let mut encoded=Vec::new();project.write(&mut encoded).unwrap();
    let restored=Project::read(encoded.as_slice(),Default::default()).unwrap();
    assert_eq!(restored.document.color,color);assert_eq!(restored.document.sdr_rendition,rendition);
    assert_eq!(restored.document.layers[0].raster.wait_data().unwrap().tiles[&key].wait_backing().unwrap().decode().unwrap(),samples);
    let mut again=Vec::new();restored.write(&mut again).unwrap();assert_eq!(again,encoded);
}
