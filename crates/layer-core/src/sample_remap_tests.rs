use super::*;
use crate::color::{ColorProfile, SampleDepth, source::*};
use crate::raster::RasterPlane;

fn numbered(extent: [u32; 2]) -> SourceImage {
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::U16, profile: ColorProfile::Icc(vec![7; 64].into()), profile_assumed: false };
    let mut builder = SourceBuilder::new(extent, interpretation, 1 << 28).unwrap();
    for y in 0..extent[1] {
        let row: Vec<u8> = (0..extent[0]).flat_map(|x| [x as u16, y as u16, (x * 7 + y) as u16, 65535].into_iter().flat_map(u16::to_le_bytes)).collect();
        builder.push_row(&row).unwrap();
    }
    let mut image = builder.finish().unwrap();
    image.resolution = Some(crate::ImageResolution::ppi(300));
    image
}

fn sample(image: &SourceImage, [x, y]: [u32; 2]) -> [u16; 4] {
    let mut rows = image.rows();
    let mut row = vec![0; image.row_bytes()];
    rows.read(y, &mut row).unwrap();
    let at = x as usize * 8;
    std::array::from_fn(|i| u16::from_le_bytes([row[at + 2 * i], row[at + 2 * i + 1]]))
}

fn never() -> AtomicBool { AtomicBool::new(false) }

#[test]
fn every_orientation_moves_each_sample_exactly_in_its_own_interpretation() {
    let source = numbered([300, 270]);
    for code in 1..=8 {
        let (map, extent) = GridMap::exif(code, source.extent).unwrap();
        let result = remap_image(&source, extent, map, 1 << 28, &never()).unwrap();
        assert_eq!(result.interpretation, source.interpretation);
        assert_eq!(result.extent, if code >= 5 { [270, 300] } else { [300, 270] });
        for p in [[0, 0], [299, 0], [0, 269], [256, 255], [143, 77], [extent[0] - 1, extent[1] - 1]] {
            if p[0] >= extent[0] || p[1] >= extent[1] { continue; }
            let [sx, sy] = map.source(p.map(i64::from));
            assert_eq!(sample(&result, p), [sx as u16, sy as u16, (sx * 7 + sy) as u16, 65535], "code {code} at {p:?}");
            assert_eq!(map.result([sx, sy]), p.map(i64::from));
        }
        let mut rows = result.rows();
        let mut row = vec![0; result.row_bytes()];
        let mut seen = std::collections::BTreeSet::new();
        for y in 0..extent[1] {
            rows.read(y, &mut row).unwrap();
            for x in 0..extent[0] as usize {
                let v: [u16; 2] = std::array::from_fn(|i| u16::from_le_bytes([row[x * 8 + 2 * i], row[x * 8 + 2 * i + 1]]));
                seen.insert(v);
            }
        }
        assert_eq!(seen.len(), 300 * 270, "code {code} is a permutation");
        let resolution = result.resolution.unwrap();
        assert_eq!(resolution, if code >= 5 { source.resolution.unwrap().swapped() } else { source.resolution.unwrap() });
    }
}

#[test]
fn a_crop_shares_whole_tiles_and_rebuilds_edges_without_excluded_samples() {
    let source = numbered([700, 600]);
    let result = remap_image(&source, [400, 300], GridMap::translation([256, 256]), 1 << 28, &never()).unwrap();
    assert!(Arc::ptr_eq(&result.tiles[&[0, 0]], &source.tiles[&[1, 1]]), "an interior tile is shared");
    assert!(!Arc::ptr_eq(&result.tiles[&[1, 0]], &source.tiles[&[2, 1]]), "a narrower edge tile is rebuilt");
    let edge = result.tiles[&[1, 0]].decode().unwrap();
    let column = 400 - 256;
    assert!(edge[column * 8..256 * 8].iter().all(|v| *v == 0), "excluded samples are removed from the edge tile");
    assert_eq!(sample(&result, [399, 299]), [655, 555, (655 * 7 + 555) as u16, 65535]);
    let unaligned = remap_image(&source, [10, 10], GridMap::translation([3, 5]), 1 << 28, &never()).unwrap();
    assert_eq!(sample(&unaligned, [9, 9]), [12, 14, 12 * 7 + 14, 65535]);
    let trailing = remap_image(&source, [700 - 512, 600 - 512], GridMap::translation([512, 512]), 1 << 28, &never()).unwrap();
    assert!(Arc::ptr_eq(&trailing.tiles[&[0, 0]], &source.tiles[&[2, 2]]), "the source's own edge tile is shared");
}

#[test]
fn image_remaps_refuse_beyond_their_budget_and_cancel() {
    let source = numbered([300, 270]);
    let (map, extent) = GridMap::exif(6, source.extent).unwrap();
    assert!(remap_image(&source, extent, map, 64, &never()).is_err());
    assert!(remap_image(&source, extent, map, 1 << 28, &AtomicBool::new(true)).is_err());
}

fn raster(tiles: &[([u32; 2], u8)]) -> RasterData {
    let color = crate::color::DocumentColor::default();
    let descriptor = RasterPlane::Color.descriptor(color);
    let mut data = RasterData::default();
    for (coordinate, seed) in tiles {
        let bytes: Vec<u8> = (0..TILE_SIZE * TILE_SIZE).flat_map(|i| [*seed, (i % 251) as u8, (i / 256) as u8, 255]).collect();
        data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: *coordinate }, RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap()));
    }
    data
}

fn pixel(data: &RasterData, plane: RasterPlane, [x, y]: [u32; 2]) -> Option<[u8; 4]> {
    let tile = data.tiles.get(&TileKey { plane, coordinate: [x / TILE_SIZE, y / TILE_SIZE] })?;
    let bytes = tile.wait_backing().unwrap().decode().unwrap();
    let at = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize * 4;
    Some(std::array::from_fn(|i| bytes[at + i]))
}

#[test]
fn whole_tile_turns_move_raster_tiles_one_to_one() {
    let data = raster(&[([0, 0], 10), ([2, 1], 20)]);
    let domain = [768, 512];
    let (map, extent) = GridMap::exif(6, domain).unwrap();
    assert!(map.tile_aligned());
    let turned = remap_raster(&data, map, extent, [[0; 2], extent.map(i64::from)], &never()).unwrap();
    assert_eq!(turned.tiles.len(), 2);
    for (x, y) in [(0u32, 0u32), (511, 767), (300, 5), (17, 600)] {
        let [sx, sy] = map.source([i64::from(x), i64::from(y)]);
        assert_eq!(pixel(&turned, RasterPlane::Color, [x, y]), pixel(&data, RasterPlane::Color, [sx as u32, sy as u32]), "{x}, {y}");
    }
    let unaligned = GridMap::exif(6, [512, 700]).unwrap().0;
    assert!(!unaligned.tile_aligned());
    assert!(remap_raster(&data, unaligned, [700, 512], [[0; 2], [700, 512]], &never()).is_err());
}

#[test]
fn trimming_clears_outside_the_kept_window_and_shares_inner_tiles() {
    let data = raster(&[([0, 0], 1), ([1, 0], 2), ([3, 0], 3)]);
    let trimmed = remap_raster(&data, GridMap::translation([256, 0]), [512, 256], [[0, 0], [300, 200]], &never()).unwrap();
    assert_eq!(trimmed.tiles.len(), 1, "tiles outside the window are dropped");
    let kept = &trimmed.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }];
    assert!(!kept.same_capture(&data.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }]));
    assert_eq!(pixel(&trimmed, RasterPlane::Color, [255, 199]).unwrap()[0], 2);
    assert_eq!(pixel(&trimmed, RasterPlane::Color, [10, 200]), Some([0; 4]));
    let shared = remap_raster(&data, GridMap::translation([-256, 0]), [1024, 256], [[0, 0], [1024, 256]], &never()).unwrap();
    assert!(shared.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [1, 0] }].same_capture(&data.tiles[&TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }]));
}
