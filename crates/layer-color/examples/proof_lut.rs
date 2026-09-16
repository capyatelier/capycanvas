//! Measure cold proof preparation, separate from interactive presentation.
use layer_core::color::{ColorProfile, ProofRecipe, RenderingIntent, RgbSpace};
use std::time::Instant;

fn main() {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(!paths.is_empty(), "supply proof ICC paths");
    let mut failures = 0;
    for path in paths {
        let path = std::path::PathBuf::from(path);
        let profile = ColorProfile::Icc(std::fs::read(&path).unwrap().into());
        for space in RgbSpace::ALL {
            for intent in [
                RenderingIntent::RelativeColorimetric,
                RenderingIntent::Perceptual,
                RenderingIntent::Saturation,
                RenderingIntent::AbsoluteColorimetric,
            ] {
                for bpc in [false, true] {
                    if bpc && intent == RenderingIntent::AbsoluteColorimetric { continue; }
                for simulation in ["adapted", "ink", "paper"] {
                    let mut recipe = ProofRecipe::new(
                        path.file_name().unwrap().to_string_lossy().into(),
                        profile.clone(),
                    );
                    recipe.conversion.intent = intent;
                    recipe.conversion.black_point_compensation = bpc;
                    recipe.simulate_black_ink = simulation != "adapted";
                    recipe.simulate_paper = simulation == "paper";
                    let started = Instant::now();
                    match layer_color::ProofLut::build(space, &recipe, || false) {
                        Ok(lut) => println!(
                            "{} {space:?} {intent:?} bpc={bpc} {simulation}: edge={} dark_grid={} bytes={} cold_ms={:.2}",
                            path.display(),
                            lut.edge(),
                            lut.dark_grid(),
                            lut.byte_len(),
                            started.elapsed().as_secs_f64() * 1000.
                        ),
                        Err(error) => {
                            eprintln!(
                                "{} {space:?} {intent:?} bpc={bpc} {simulation}: ERROR {error}",
                                path.display()
                            );
                            failures += 1;
                        }
                    }
                }
                }
            }
        }
    }
    assert_eq!(
        failures, 0,
        "profiles must meet the declared viewing tolerance"
    );
}
