//! Emit the exact built-in profiles for the independent, offline proof oracle.
use layer_core::color::{ColorProfile, RgbSpace};

fn main() {
    let directory = std::path::PathBuf::from(std::env::args_os().nth(1).expect("output directory"));
    std::fs::create_dir_all(&directory).unwrap();
    for (name, space) in [
        ("srgb", RgbSpace::Srgb),
        ("p3", RgbSpace::DisplayP3),
        ("adobe", RgbSpace::AdobeRgb),
        ("prophoto", RgbSpace::ProPhoto),
    ] {
        std::fs::write(
            directory.join(format!("{name}.icc")),
            layer_color::profile_bytes(&ColorProfile::Builtin(space)).unwrap(),
        )
        .unwrap();
    }
}
