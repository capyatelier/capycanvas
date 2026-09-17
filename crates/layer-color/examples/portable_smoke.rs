use layer_core::color::source::{SourceBuilder, SourceChannels, SourceInterpretation};
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace};
#[unsafe(no_mangle)]
pub extern "C" fn portable_smoke() -> u32 {
    let profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    let bytes = layer_color::profile_bytes(&profile).unwrap();
    let embedded = ColorProfile::Icc(bytes.into());
    let transform =
        layer_color::RgbTransform::new(&embedded, &ColorProfile::default(), Default::default())
            .unwrap();
    let mut values = [[0.25, 0.5, 0.75, 0.375]];
    transform.apply(&mut values);
    assert_eq!(values[0][3], 0.375);
    assert!(values[0].iter().all(|v| v.is_finite()));
    let mut builder = SourceBuilder::new(
        [16, 8],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: embedded.clone(),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    for _ in 0..8 {
        builder.push_row(&[40, 100, 170].repeat(16)).unwrap();
    }
    let source = builder.finish().unwrap();
    let mut jpeg = Vec::new();
    layer_color::photo::write_jpeg(&mut jpeg, &source, 100).unwrap();
    let restored =
        layer_color::photo::read_jpeg(std::io::Cursor::new(jpeg), Default::default()).unwrap();
    assert_eq!(restored.extent, source.extent);
    assert_eq!(restored.interpretation.profile, embedded);
    let mut row = [0; 48];
    restored.rows().read(0, &mut row).unwrap();
    assert!(row.chunks_exact(3).all(|p| {
        p.iter()
            .zip([40u8, 100, 170])
            .all(|(a, b)| a.abs_diff(b) <= 2)
    }));
    let gray = layer_color::gray_profile(RgbSpace::Srgb).unwrap();
    let transform = layer_color::InputTransform::new(&gray, &profile, Default::default()).unwrap();
    transform.gray(&[[0.5]], &mut values).unwrap();
    assert!(values[0][0] > 0.49 && values[0][0] < 0.51);
    1
}
