use super::*;
use layer_core::{Document, authored::{Artwork, PaintSource, SourceTarget}};
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};
use layer_render::ThumbnailTarget;

fn codes(x: u32, y: u32) -> [u16; 4] {
    [
        ((x * 8191 + y * 31) % 65536) as u16,
        ((x * 17 + y * 16381) % 65536) as u16,
        if (x / 173 + y / 111) % 2 == 0 {
            65535
        } else {
            0
        },
        [0, 1, 257, 32768, 65535][((x + y * 3) % 5) as usize],
    ]
}
fn photo(extent: [u32; 2]) -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
            profile_assumed: false,
        },
        512 * 1024 * 1024,
    )
    .unwrap();
    for y in 0..extent[1] {
        let row: Vec<_> = (0..extent[0])
            .flat_map(|x| codes(x, y))
            .flat_map(u16::to_le_bytes)
            .collect();
        builder.push_row(&row).unwrap();
    }
    Arc::new(builder.finish().unwrap())
}
pub(crate) fn thumbnail(r: &mut WgpuRasterizer, target: ThumbnailTarget) -> Vec<u8> {
    r.start_thumbnail(7, target).unwrap();
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(READBACK_TIMEOUT),
        })
        .unwrap();
    r.thumbnails.take().unwrap().unwrap().bytes
}

#[test]
fn tiny_portrait_photo_thumbnail_has_color_and_checkered_letterbox() {
    let source = photo([24, 48]);
    let mut artwork = Artwork::new([24, 48]).unwrap();
    let (_, SourceTarget::Paint(target)) = crate::test_support::add_paint(&mut artwork, "tiny photo", [24, 48]) else { unreachable!() };
    artwork.paint.get_mut(target).unwrap().base = Some(layer_core::authored::PaintBase::new((source).into()));
    let document = Document::from_artwork(artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    r.submit(crate::test_support::packet(document.scene(), [24, 48])).unwrap();
    let bytes = thumbnail(&mut r, ThumbnailTarget::Source(SourceTarget::Paint(target)));
    assert!(bytes.chunks_exact(4).all(|p| p[3] == 255));
    assert!(
        bytes
            .chunks_exact(4)
            .filter(|p| p[0].abs_diff(p[2]) > 20)
            .count()
            > 100
    );
    assert_ne!(bytes[0], bytes[16], "letterbox checker survives");
}

#[test]
fn photo_thumbnail_batches_survive_interleaved_layers_edits_and_discarded_commands() {
    use layer_core::raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey};
    let extent = [768, 256];
    let source = photo(extent);
    let color = layer_core::color::DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let paint = |rgba: [u16; 4]| {
        let pixels: Vec<_> = rgba.into_iter().flat_map(u16::to_le_bytes).collect();
        let mut data = RasterData::default();
        for x in 0..3 {
            data.tiles.insert(TileKey { plane: RasterPlane::Color, coordinate: [x, 0] },
                RasterTile::backed(TileBlob::encode(color.paint_descriptor(), &pixels.repeat(256 * 256)).unwrap()));
        }
        PaintSource { color_mode: Default::default(), domain: extent, base: Some(layer_core::authored::PaintBase::new((source.clone()).into())), raster: RasterRevision::backed(data), operations: Arc::default() }
    };
    let mut artwork = Artwork::new(extent).unwrap();
    artwork.compositions.get_mut(artwork.root).unwrap().color = color;
    let targets = [[45000, 1000, 1000, 65535], [1000, 45000, 1000, 65535]].map(|rgba| {
        let (_, target) = crate::test_support::add_paint(&mut artwork, "photo", extent);
        let SourceTarget::Paint(handle) = target else { unreachable!() };
        *artwork.paint.get_mut(handle).unwrap() = paint(rgba);
        target
    });
    let mut document = Document::from_artwork(artwork).unwrap();
    let mut r = WgpuRasterizer::new_native_headless(color).unwrap();
    r.submit(crate::test_support::packet(document.scene(), extent)).unwrap();
    let mut gpu = SourceThumbnails::new(&r);
    let prepare = |gpu: &mut SourceThumbnails, r: &mut WgpuRasterizer, id, discard: bool| {
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let ready = gpu.prepare(r, id, &mut encoder, 1).unwrap();
        if !discard { r.uploads.finish(&encoder); encoder.submit(&r.queue); }
        ready
    };
    while !prepare(&mut gpu, &mut r, targets[0], false) {}
    while !prepare(&mut gpu, &mut r, targets[1], false) {}
    assert_eq!(gpu.cache.len(), 1, "layers share the integrated original");
    r.thumbnails.sources = Some(gpu);
    let before = thumbnail(&mut r, ThumbnailTarget::Source(targets[0]));
    let other = thumbnail(&mut r, ThumbnailTarget::Source(targets[1]));
    assert_ne!(before, other);
    let SourceTarget::Paint(handle) = targets[0] else { unreachable!() };
    *document.artwork.paint.get_mut(handle).unwrap() = paint([1000, 1000, 45000, 65535]);
    r.submit(crate::test_support::packet(document.scene(), extent)).unwrap();
    let mut gpu = r.thumbnails.sources.take().unwrap();
    assert!(!prepare(&mut gpu, &mut r, targets[0], true));
    let mut ready = [false; 2];
    for _ in 0..8 {
        for i in 0..2 { ready[i] = prepare(&mut gpu, &mut r, targets[i], false); }
        if ready == [true; 2] { break; }
    }
    assert_eq!(ready, [true; 2]);
    assert_eq!(gpu.cache.len(), 1);
    r.thumbnails.sources = Some(gpu);
    let after = thumbnail(&mut r, ThumbnailTarget::Source(targets[0]));
    assert_ne!(before, after, "an edit invalidates the prepared sums");
    assert_eq!(other, thumbnail(&mut r, ThumbnailTarget::Source(targets[1])));
    r.thumbnails.sources = None;
    assert_eq!(after, thumbnail(&mut r, ThumbnailTarget::Source(targets[0])), "discarded work cannot contaminate the completed image");
}
