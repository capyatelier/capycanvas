use layer_core::color::SampleDepth;
use layer_core::color::source::SourceImage;

pub(crate) fn rows(source: &SourceImage) -> Vec<Vec<u8>> {
    let mut reader = source.rows();
    (0..source.extent[1])
        .map(|y| {
            let mut row = vec![0; source.row_bytes()];
            reader.read(y, &mut row).unwrap();
            row
        })
        .collect()
}

pub(crate) fn f16_pixels(bytes: &[u8]) -> Vec<[f32; 4]> {
    bytes
        .chunks_exact(8)
        .map(|p| {
            layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| {
                u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])
            }))
            .unwrap()
        })
        .collect()
}

pub(crate) fn hdr_pixels(source: &SourceImage) -> Vec<[f32; 4]> {
    f16_pixels(&rows(source).concat())
}

pub(crate) fn assert_rgba16_reference(source: &SourceImage, expected: &[u8], name: &str) {
    let bytes = rows(source).concat();
    let actual: Vec<u16> = if source.interpretation.depth == SampleDepth::U16 {
        bytes
            .chunks_exact(2)
            .map(|v| u16::from_le_bytes([v[0], v[1]]))
            .collect()
    } else {
        bytes.iter().map(|&v| u16::from(v) * 257).collect()
    };
    assert_eq!(expected.len(), actual.len() * 2, "{name}");
    for (i, (actual, expected)) in actual.iter().zip(expected.chunks_exact(2)).enumerate() {
        assert_eq!(
            *actual,
            u16::from_le_bytes([expected[0], expected[1]]),
            "{name} sample={i}"
        );
    }
}
