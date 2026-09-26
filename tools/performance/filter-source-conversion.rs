//! Standalone CPU control; link the release layer_color library (see report).
//! The built-in profile arm is a timing control, not a color-correct replacement.
fn main() {
    let path = std::env::var("CAPY_FILTER_SOURCE_JPEG").unwrap();
    let source = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(path).unwrap()),
        Default::default(),
    )
    .unwrap();
    let encoded: Vec<_> = source
        .tiles
        .values()
        .map(|tile| tile.decode().unwrap())
        .collect();
    let mut output = vec![[0.; 4]; 256 * 256];
    for kind in ["icc", "assume_srgb"] {
        let mut interpretation = source.interpretation.clone();
        if kind == "assume_srgb" {
            interpretation.profile = Default::default();
        }
        let decoder = layer_color::WorkingDecoder::new(
            &interpretation,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        for i in 0..6 {
            let start = std::time::Instant::now();
            for tile in &encoded {
                decoder.decode_pixels(tile, &mut output).unwrap();
                std::hint::black_box(&output);
            }
            println!(
                "{kind},{i},{},{:.3}",
                encoded.len(),
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
}
