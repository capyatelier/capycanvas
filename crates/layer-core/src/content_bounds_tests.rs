use super::*;
use crate::color::{DocumentColor, SampleDepth};
use crate::raster::{RasterData, RasterTile};

/// A tile whose pixels inside `covered` (tile-local x0, y0, x1, y1) have full
/// alpha, or full coverage on a mask.
fn tile(plane: RasterPlane, color: DocumentColor, covered: Option<[u32; 4]>) -> RasterTile {
    let descriptor = plane.descriptor(color);
    let stride = descriptor.bytes_per_pixel().unwrap();
    let size = usize::from(descriptor.bits_per_channel / 8);
    let one: Vec<u8> = match (descriptor.sample, size) {
        (SampleType::Float, 4) => 1f32.to_le_bytes().to_vec(),
        (SampleType::Float, _) => 0x3c00u16.to_le_bytes().to_vec(),
        _ => vec![0xff; size],
    };
    let mut bytes = vec![0; descriptor.byte_len([TILE_SIZE; 2]).unwrap()];
    if let Some([x0, y0, x1, y1]) = covered {
        for y in y0..y1 {
            for x in x0..x1 {
                let pixel = (y * TILE_SIZE + x) as usize * stride;
                for channel in 0..usize::from(descriptor.channels) {
                    bytes[pixel + channel * size..pixel + (channel + 1) * size].copy_from_slice(&one);
                }
            }
        }
    }
    RasterTile::backed(TileBlob::encode(descriptor, &bytes).unwrap())
}

fn raster(plane: RasterPlane, color: DocumentColor, tiles: &[([u32; 2], Option<[u32; 4]>)]) -> RasterRevision {
    RasterRevision::backed(RasterData {
        tiles: tiles.iter().map(|(c, covered)| (TileKey { plane, coordinate: *c }, tile(plane, color, *covered))).collect(),
        watercolor: None,
    })
}

fn photo_source(extent: [u32; 2]) -> color::source::SourceImage {
    use color::source::*;
    let mut builder = SourceBuilder::new(extent, SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U8,
        profile: Default::default(),
        profile_assumed: false,
    }, 8 * 1024 * 1024).unwrap();
    for _ in 0..extent[1] {
        builder.push_row(&vec![200; extent[0] as usize * 4]).unwrap();
    }
    builder.finish().unwrap()
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect { min: Point { x: x0, y: y0 }, max: Point { x: x1, y: y1 } }
}

fn bounds(doc: &Document, scope: ContentScope) -> Rect {
    ContentBoundsRequest::new(doc, scope).scan(&ContentBoundsCache::default(), ScanBudget::Worker).unwrap().unwrap()
}

/// A 600×400 drawing without paper whose paint layer holds a few pixels in
/// the middle of its grid, transparent tiles around them, and pixels past
/// the canvas's left edge.
fn drawing(color: DocumentColor) -> Document {
    let mut doc = Document::new("bounds", 600, 400);
    doc.color = color;
    doc.layers.retain(|l| l.kind != LayerKind::Background);
    doc.layers[0].raster = raster(RasterPlane::Color, color, &[
        ([0, 0], None),
        ([1, 0], Some([10, 20, 30, 256])),
        ([2, 0], None),
        ([1, 1], Some([0, 0, 200, 5])),
        ([2, 1], Some([0, 40, 1, 41])),
        ([0, 1], None),
    ]);
    doc.layers[0].properties.offset = Point { x: -300., y: 0. };
    doc
}

#[test]
fn bounds_are_pixel_tight_at_every_depth_and_include_hidden_pixels() {
    for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
        let color = DocumentColor { depth, ..DocumentColor::default() };
        let doc = drawing(color);
        let expected = rect(256. - 300., 20., 513. - 300., 297.);
        assert_eq!(bounds(&doc, ContentScope::Visible), expected, "{depth:?}");
        assert_eq!(bounds(&doc, ContentScope::All), expected, "{depth:?}");
    }
}

#[test]
fn canvas_bounds_count_only_the_pixels_on_the_canvas() {
    for depth in [SampleDepth::U8, SampleDepth::F32] {
        let doc = drawing(DocumentColor { depth, ..DocumentColor::default() });
        assert_eq!(bounds(&doc, ContentScope::Canvas), rect(0., 256., 213., 297.), "{depth:?}");
    }
    let mut doc = drawing(DocumentColor::default());
    doc.layers[0].properties.offset = Point { x: -600., y: 0. };
    assert!(bounds(&doc, ContentScope::Canvas).is_empty(), "every pixel lies beyond the canvas");
    let mut paper = Document::new("paper", 600, 400);
    paper.layers[0].properties.offset = Point { x: -5000., y: 0. };
    assert_eq!(bounds(&paper, ContentScope::Canvas), rect(0., 0., 600., 400.));
}

#[test]
fn only_edge_tiles_are_decoded_and_a_second_scan_decodes_nothing() {
    let color = DocumentColor::default();
    let mut doc = Document::new("edges", 2048, 2048);
    doc.layers.retain(|l| l.kind != LayerKind::Background);
    let tiles: Vec<_> = (0..8).flat_map(|y| (0..8).map(move |x| ([x, y], Some([0, 0, 256, 256])))).collect();
    doc.layers[0].raster = raster(RasterPlane::Color, color, &tiles);
    let request = ContentBoundsRequest::new(&doc, ContentScope::Visible);
    let cache = ContentBoundsCache::default();
    let mut scan = Scan { cache: &cache, budget: ScanBudget::Worker, decoded: 0 };
    let part = &request.parts[0].content;
    assert_eq!(scan.bounds(part).unwrap(), Some(rect(0., 0., 2048., 2048.)));
    assert_eq!(scan.decoded, 1, "identical tiles share one decode");
    assert_eq!(request.scan(&cache, ScanBudget::Tiles(0)).unwrap(), Some(rect(0., 0., 2048., 2048.)));

    let distinct: Vec<_> = (0..8).flat_map(|y| (0..8).map(move |x| ([x, y], Some([x, y, 256, 256])))).collect();
    doc.layers[0].raster = raster(RasterPlane::Color, color, &distinct);
    let request = ContentBoundsRequest::new(&doc, ContentScope::Visible);
    let cache = ContentBoundsCache::default();
    let mut scan = Scan { cache: &cache, budget: ScanBudget::Worker, decoded: 0 };
    assert_eq!(scan.bounds(&request.parts[0].content).unwrap(), Some(rect(0., 0., 2048., 2048.)));
    assert_eq!(scan.decoded, 28, "only the ring of edge tiles");
}

#[test]
fn a_tile_budget_pauses_the_scan_and_resumes_from_the_cache() {
    let doc = drawing(DocumentColor::default());
    let request = ContentBoundsRequest::new(&doc, ContentScope::Visible);
    let cache = ContentBoundsCache::default();
    let mut rounds = 0;
    let found = loop {
        rounds += 1;
        if let Some(found) = request.scan(&cache, ScanBudget::Tiles(1)).unwrap() {
            break found;
        }
    };
    assert!(rounds > 2);
    assert_eq!(found, rect(-44., 20., 213., 297.));
}

#[test]
fn photos_count_by_placement_and_masks_limit_only_visible_content() {
    let color = DocumentColor::default();
    let mut doc = drawing(color);
    let photo = doc.allocate_layer_id();
    let mut layer = Layer::paint(photo, "Photo");
    layer.source = Some(Arc::new(photo_source([100, 50])));
    layer.properties.placement = Affine([2., 0., 0., 2., 0., 0.]);
    layer.properties.offset = Point { x: 500., y: 380. };
    doc.layers.insert(0, layer);
    let all = rect(-44., 20., 700., 480.);
    assert_eq!(bounds(&doc, ContentScope::Visible), all);

    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point { x: 500., y: 380. });
    mask.default_coverage = 0.;
    mask.raster = raster(RasterPlane::Mask, color, &[([0, 0], Some([0, 0, 20, 10]))]);
    doc.layers[0].mask = Some(mask);
    assert_eq!(bounds(&doc, ContentScope::Visible), rect(-44., 20., 540., 400.));
    assert_eq!(bounds(&doc, ContentScope::All), all, "masked pixels still count for Reveal All");

    doc.layers[0].mask.as_mut().unwrap().enabled = false;
    assert_eq!(bounds(&doc, ContentScope::Visible), all);
    doc.layers[0].visible = false;
    assert_eq!(bounds(&doc, ContentScope::Visible), rect(-44., 20., 213., 297.));
    assert_eq!(bounds(&doc, ContentScope::All), all, "hidden layers count for Reveal All");
}

#[test]
fn group_masks_limit_their_children_and_paper_covers_the_canvas() {
    let color = DocumentColor::default();
    let mut doc = drawing(color);
    let group = doc.allocate_layer_id();
    let mut layer = Layer::paint(group, "Group");
    layer.kind = LayerKind::Group;
    let mut mask = LayerMask::reveal_all(doc.allocate_layer_id(), Point::default());
    mask.default_coverage = 0.;
    mask.initial = Some(Selection::polygon(rect(0., 0., 100., 100.).corners().to_vec()).unwrap());
    layer.mask = Some(mask);
    doc.layers.insert(1, layer);
    doc.layers[0].properties.parent = Some(group);
    doc.layers[0].properties.offset = Point { x: -300., y: 0. };
    assert_eq!(bounds(&doc, ContentScope::Visible), rect(0., 20., 100., 100.));
    assert_eq!(bounds(&doc, ContentScope::All), rect(-44., 20., 213., 297.));

    let mut paper = Document::new("paper", 600, 400);
    paper.layers[0].raster = raster(RasterPlane::Color, color, &[([0, 0], Some([5, 5, 6, 6]))]);
    assert_eq!(bounds(&paper, ContentScope::Visible), rect(0., 0., 600., 400.));
    paper.layers[1].visible = false;
    assert_eq!(bounds(&paper, ContentScope::Visible), rect(5., 5., 6., 6.));
}

#[test]
fn empty_documents_have_empty_bounds() {
    let mut doc = Document::new("empty", 300, 200);
    doc.layers.retain(|l| l.kind != LayerKind::Background);
    assert!(bounds(&doc, ContentScope::Visible).is_empty());
    doc.layers[0].raster = raster(RasterPlane::Color, doc.color, &[([0, 0], None)]);
    assert!(bounds(&doc, ContentScope::All).is_empty());
}
