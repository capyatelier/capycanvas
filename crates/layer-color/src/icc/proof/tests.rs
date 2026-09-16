use super::*;

#[test]
fn matrix_proof_keeps_neutral_endpoints_and_detects_target_gamut() {
    for space in RgbSpace::ALL {
        let recipe = ProofRecipe::new("sRGB".into(), ColorProfile::default());
        let proof = ProofTransform::new(space, &recipe).unwrap();
        for code in 0..=255 {
            let gray = code as f32 / 255.;
            let output = proof.sample([gray; 3]).unwrap();
            let expected = space.decode(f64::from(gray));
            assert!(
                (output.xyz[1] - expected).abs() < 0.0002,
                "{space:?} {code}"
            );
            assert!(output.gamut_distance < 0.1);
        }
        if space != RgbSpace::Srgb {
            assert!(proof.sample([0., 1., 0.]).unwrap().gamut_distance > 5.);
        }
    }
}

#[test]
fn conflicting_policy_invalid_profile_and_outside_domain_fail_explicitly() {
    let mut recipe = ProofRecipe::new("sRGB".into(), ColorProfile::default());
    recipe.conversion.intent = RenderingIntent::AbsoluteColorimetric;
    assert!(
        ProofTransform::new(RgbSpace::Srgb, &recipe)
            .err()
            .unwrap()
            .contains("Absolute")
    );
    recipe.conversion.black_point_compensation = false;
    recipe.simulate_paper = true;
    recipe.simulate_black_ink = false;
    assert!(
        ProofTransform::new(RgbSpace::Srgb, &recipe)
            .err()
            .unwrap()
            .contains("Paper")
    );
    recipe.simulate_black_ink = true;
    let proof = ProofTransform::new(RgbSpace::Srgb, &recipe).unwrap();
    for rgb in [[-0.1, 0., 0.], [1.01, 0., 0.], [f32::NAN, 0., 0.]] {
        assert!(proof.sample(rgb).is_err());
    }
    recipe.profile = ColorProfile::Icc(vec![0; 132].into());
    assert!(ProofTransform::new(RgbSpace::Srgb, &recipe).is_err());
    recipe.profile = gray_profile(RgbSpace::Srgb).unwrap();
    assert!(
        ProofTransform::new(RgbSpace::Srgb, &recipe)
            .err()
            .unwrap()
            .contains("RGB or CMYK")
    );
}

#[test]
fn proof_requires_complete_directions_matching_channels_and_an_image_profile() {
    use moxcms::{LutDataType, LutStore, LutType, LutWarehouse};
    let lut = LutWarehouse::Lut(LutDataType {
        num_input_channels: 3, num_output_channels: 3, num_clut_grid_points: 2,
        matrix: builtin(RgbSpace::Srgb).unwrap().colorant_matrix(),
        num_input_table_entries: 2, num_output_table_entries: 2,
        input_table: LutStore::Store16(vec![0, 65535].repeat(3)),
        output_table: LutStore::Store16(vec![0, 65535].repeat(3)),
        clut_table: LutStore::Store16(vec![0; 24]), lut_type: LutType::Lut16,
    });
    let mut profile = builtin(RgbSpace::Srgb).unwrap();
    profile.lut_a_to_b_perceptual = Some(lut.clone());
    assert!(Pcs::new(&profile, RenderingIntent::Perceptual).err().unwrap().contains("PCS-to-device"));
    profile.lut_a_to_b_perceptual = None;
    profile.lut_b_to_a_perceptual = Some(lut.clone());
    assert!(Pcs::new(&profile, RenderingIntent::Perceptual).err().unwrap().contains("device-to-PCS"));
    profile.lut_a_to_b_perceptual = Some(lut);
    let Some(LutWarehouse::Lut(lut)) = &mut profile.lut_a_to_b_perceptual else { panic!() };
    lut.num_input_channels = 4;
    assert!(Pcs::new(&profile, RenderingIntent::Perceptual).err().unwrap().contains("channel count"));
    let mut profile = builtin(RgbSpace::Srgb).unwrap();
    profile.pcs = DataColorSpace::Lab;
    assert!(Pcs::new(&profile, RenderingIntent::Perceptual).is_err());
    let mut bytes = profile_bytes(&ColorProfile::default()).unwrap();
    bytes[12..16].copy_from_slice(b"link");
    let recipe = ProofRecipe::new("Device link".into(), ColorProfile::Icc(bytes.into()));
    assert!(ProofTransform::new(RgbSpace::Srgb, &recipe).err().unwrap().contains("image color space"));
}

#[test]
fn paper_scales_media_white_and_viewing_black_mapping_keeps_white() {
    let mut profile = builtin(RgbSpace::Srgb).unwrap();
    profile.media_white_point = Some(moxcms::Xyzd {
        x: 0.88,
        y: 0.92,
        z: 0.65,
    });
    let mut recipe = ProofRecipe::new(
        "Warm paper".into(),
        ColorProfile::Icc(profile.encode().unwrap().into()),
    );
    recipe.simulate_paper = true;
    let proof = ProofTransform::new(RgbSpace::Srgb, &recipe).unwrap();
    let white = proof.sample([1.; 3]).unwrap();
    recipe.simulate_paper = false;
    let relative = ProofTransform::new(RgbSpace::Srgb, &recipe)
        .unwrap()
        .sample([1.; 3])
        .unwrap();
    // The matrix's actual endpoint need not equal the rounded ICC D50 constant.
    for i in 0..3 {
        let expected = relative.xyz[i] * [0.88, 0.92, 0.65][i] / D50[i];
        assert!((white.xyz[i] - expected).abs() < 0.0002);
    }
    let dark = [0.02, 0.025, 0.01];
    let mapped_white = black::compensate(D50, dark, [0.; 3]);
    let mapped_black = black::compensate(dark, dark, [0.; 3]);
    for i in 0..3 {
        assert!((mapped_white[i] - D50[i]).abs() < 1e-12);
        assert!(mapped_black[i].abs() < 1e-12);
    }
}

#[test]
#[ignore = "requires independent LittleCMS fixtures from tools/validation/proof_reference.py"]
fn proof_matches_independent_cmm() {
    let directory = std::path::PathBuf::from(
        std::env::var("LAYER_PROOF_REFERENCE").expect("reference directory"),
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("reference.json")).unwrap()).unwrap();
    let floats = |record: &serde_json::Value| -> Vec<f32> {
        std::fs::read(directory.join(record["file"].as_str().unwrap()))
            .unwrap()
            .chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
            .collect()
    };
    let intents = [
        RenderingIntent::Perceptual,
        RenderingIntent::RelativeColorimetric,
        RenderingIntent::Saturation,
        RenderingIntent::AbsoluteColorimetric,
    ];
    let mut failures = Vec::new();
    for (file, record) in manifest["profiles"].as_object().unwrap() {
        let profile = open(&ColorProfile::Icc(
            std::fs::read(directory.join(file)).unwrap().into(),
        ))
        .unwrap();
        let relative = Pcs::new(&profile, RenderingIntent::RelativeColorimetric).unwrap();
        for (i, intent) in intents[..3].iter().enumerate() {
            let pcs = Pcs::new(&profile, *intent).unwrap();
            for (label, actual) in [
                (
                    "source",
                    black::source_black(&profile, &pcs, *intent).unwrap(),
                ),
                (
                    "destination",
                    black::destination_black(&profile, &pcs, &relative, *intent).unwrap(),
                ),
            ] {
                assert!(record["black"][i][label]["valid"].as_bool().unwrap());
                let xyz = record["black"][i][label]["xyz"].as_array().unwrap();
                let maximum = actual
                    .iter()
                    .zip(xyz)
                    .map(|(a, b)| (a - b.as_f64().unwrap()).abs())
                    .fold(0f64, f64::max);
                eprintln!("black {file} {intent:?} {label}: max_xyz={maximum:.8} {actual:?}");
                if maximum > 0.0002 {
                    failures.push(format!("black {file} {intent:?} {label}"));
                }
            }
        }
    }
    for case in manifest["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let space = match source {
            "srgb.icc" => RgbSpace::Srgb,
            "p3.icc" => RgbSpace::DisplayP3,
            "adobe.icc" => RgbSpace::AdobeRgb,
            "prophoto.icc" => RgbSpace::ProPhoto,
            _ => panic!("unknown reference source"),
        };
        let target = case["target"].as_str().unwrap();
        let bytes = std::fs::read(directory.join(target)).unwrap();
        let mut recipe = ProofRecipe::new(target.into(), ColorProfile::Icc(bytes.into()));
        recipe.conversion = ConversionOptions {
            intent: intents[case["intent"].as_u64().unwrap() as usize],
            black_point_compensation: case["bpc"].as_bool().unwrap(),
        };
        let simulation = case["simulation"].as_str().unwrap();
        recipe.simulate_black_ink = simulation != "adapted";
        recipe.simulate_paper = simulation == "paper";
        let proof = ProofTransform::new(space, &recipe).unwrap();
        let input = floats(&case["input"]);
        let expected = floats(&case["xyz"]);
        let gamut = floats(&case["gamut"]);
        assert_eq!(input.len(), expected.len());
        assert_eq!(input.len(), gamut.len());
        let mut errors = Vec::new();
        let mut max_xyz = 0f64;
        let mut mismatches = 0;
        let mut boundary = 0;
        let mut raw_disagreements = 0;
        let mut distance_error = 0f64;
        for ((rgb, xyz), gamut) in input
            .chunks_exact(3)
            .zip(expected.chunks_exact(3))
            .zip(gamut.chunks_exact(3))
        {
            let result = proof.sample(rgb.try_into().unwrap()).unwrap();
            let xyz = [xyz[0] as f64, xyz[1] as f64, xyz[2] as f64];
            errors.push(distance(result.xyz, xyz));
            for i in 0..3 {
                max_xyz = max_xyz.max((result.xyz[i] - xyz[i]).abs());
            }
            let ambiguous = gamut.iter().any(|v| (*v - 5.).abs() <= 1.);
            boundary += usize::from(ambiguous);
            let disagree = (gamut[0] > 5.) != (result.gamut_distance > 5.);
            raw_disagreements += usize::from(disagree);
            mismatches += usize::from(disagree && !ambiguous);
            for i in 0..2 { distance_error = distance_error.max((f64::from(gamut[i+1]) - result.gamut_roundtrips[i]).abs()); }
        }
        let mut subset_report = Vec::new();
        for (name, range) in case["subsets"].as_object().unwrap() {
            let mut e = errors
                [range[0].as_u64().unwrap() as usize..range[1].as_u64().unwrap() as usize]
                .to_vec();
            e.sort_by(f64::total_cmp);
            subset_report.push(format!(
                "{name}:p99={:.4},max={:.4}",
                e[(e.len() as f64 * 0.99).ceil() as usize - 1],
                e.last().unwrap()
            ));
        }
        errors.sort_by(f64::total_cmp);
        let p99 = errors[(errors.len() as f64 * 0.99).ceil() as usize - 1];
        let max = *errors.last().unwrap();
        let label = format!(
            "{source}->{target} depth={} {:?} bpc={} {simulation}",
            case["depth"], recipe.conversion.intent, recipe.conversion.black_point_compensation
        );
        eprintln!(
            "{label}: xyz={max_xyz:.6} dE99={p99:.4} dEmax={max:.4} gamut_mismatch={mismatches} raw_disagreement={raw_disagreements} boundary={boundary}/{} distance_error={distance_error:.5}; {}",
            gamut.len() / 3,
            subset_report.join(" ")
        );
        if max_xyz > 0.02 || p99 > 2. || max > 4. || mismatches > 0 {
            failures.push(label);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failing proof cases: {failures:?}",
        failures.len()
    );
}
