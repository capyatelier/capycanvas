use super::*;
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};

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
fn thumbnail(r: &mut WgpuRasterizer, id: LayerId) -> Vec<u8> {
    r.start_thumbnail(7, id).unwrap();
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
    let mut layer = Layer::paint(LayerId(1), "tiny photo");
    layer.source = Some(source);
    let mut r = WgpuRasterizer::new_headless().unwrap();
    r.ensure_document([24, 48], &[layer]).unwrap();
    let bytes = thumbnail(&mut r, LayerId(1));
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

