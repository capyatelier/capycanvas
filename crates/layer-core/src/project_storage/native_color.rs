use super::*;
use crate::color::{DocumentColor, IntegerDepth, RgbSpace};

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
        for depth in [IntegerDepth::U8, IntegerDepth::U16] {
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
        depth: IntegerDepth::U16,
    });
    let mut bytes = Vec::new();
    p.write(&mut bytes).unwrap();
    bytes[10] = 2;
    assert!(Project::read(bytes.as_slice(), Default::default()).is_err());
    p.document.color.depth = IntegerDepth::U8;
    assert!(
        p.write(&mut Vec::new())
            .unwrap_err()
            .contains("representation")
    );
    p.document.color.depth = IntegerDepth::U16;
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
