//! Detect black from profile transforms, not untrusted bkpt tags. The published
//! ICC/Adobe ramp method is described in the GTK milestone 3 design document.
use super::*;

pub(super) fn source_black(
    profile: &Profile,
    pcs: &Pcs,
    intent: RenderingIntent,
) -> Result<[f64; 3], String> {
    if profile.version() >= moxcms::ProfileVersion::V4_0
        && matches!(
            intent,
            RenderingIntent::Perceptual | RenderingIntent::Saturation
        )
        && !matrix_only(profile)
    {
        return Ok([0.00336, 0.0034731, 0.00287]);
    }
    let cmyk = channels(profile)? == ProfileChannels::Cmyk;
    let xyz = if cmyk
        && profile.profile_class == moxcms::ProfileClass::OutputDevice
        && intent == RenderingIntent::RelativeColorimetric
    {
        let perceptual = Pcs::new(profile, RenderingIntent::Perceptual)?;
        // The v4 perceptual connection maps Lab's zero black into the reference
        // medium black before entering the inverse table, even with BPC off.
        let input = if profile.version() >= moxcms::ProfileVersion::V4_0 {
            [0.00336, 0.0034731, 0.00287]
        } else { [0.; 3] };
        pcs.roundtrip(&perceptual, input)
    } else {
        pcs.to_xyz([if cmyk { 1. } else { 0. }; 4])
    };
    let l = xyz_to_lab(xyz)[0];
    if !l.is_finite() || !(0. ..=50.).contains(&l) {
        return Err("The proof profile has an invalid black endpoint".into());
    }
    Ok(lab_to_xyz([l, 0., 0.]))
}

pub(super) fn destination_black(
    profile: &Profile,
    reverse: &Pcs,
    relative: &Pcs,
    intent: RenderingIntent,
) -> Result<[f64; 3], String> {
    let initial = source_black(profile, reverse, intent)?;
    if matrix_only(profile)
        || (profile.version() >= moxcms::ProfileVersion::V4_0
            && matches!(
                intent,
                RenderingIntent::Perceptual | RenderingIntent::Saturation
            ))
    {
        return Ok(initial);
    }
    let mut ramp = [0.; 256];
    for (i, value) in ramp.iter_mut().enumerate() {
        let xyz = lab_to_xyz([i as f64 * 100. / 255., 0., 0.]);
        *value = xyz_to_lab(relative.roundtrip(reverse, xyz))[0];
    }
    for i in (1..255).rev() {
        ramp[i] = ramp[i].min(ramp[i + 1]);
    }
    let (low, high) = (ramp[0], ramp[255]);
    if !low.is_finite() || !high.is_finite() || low >= high {
        return Err("Cannot estimate the proof profile's black response".into());
    }
    let relative_intent = intent == RenderingIntent::RelativeColorimetric;
    if relative_intent
        && ramp.iter().enumerate().all(|(i, y)| {
            let x = i as f64 * 100. / 255.;
            x <= low + 0.2 * (high - low) || (x - y).abs() < 4.
        })
    {
        return Ok(initial);
    }
    let (min, max) = if relative_intent {
        (0.1, 0.5)
    } else {
        (0.03, 0.25)
    };
    let points: Vec<_> = ramp
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let y = (v - low) / (high - low);
            (min..max)
                .contains(&y)
                .then_some((i as f64 * 100. / 255., y))
        })
        .collect();
    if points.len() < 4 {
        return Err("Insufficient shadow response for black compensation".into());
    }
    // Least squares c + bx + ax² from the normal equations.
    let mut ata = [[0.; 3]; 3];
    let mut atb = [0.; 3];
    for (x, y) in points {
        let basis = [1., x, x * x];
        for i in 0..3 {
            for j in 0..3 {
                ata[i][j] += basis[i] * basis[j];
            }
            atb[i] += basis[i] * y;
        }
    }
    let [c, b, a] = layer_core::color::rgb::apply(layer_core::color::rgb::inverse(ata), atb);
    if [c, b, a].iter().any(|v| !v.is_finite()) {
        return Err("Degenerate profile black response".into());
    }
    let discriminant = b * b - 4. * a * c;
    let l = if a.abs() < 1e-10 {
        if b.abs() < 1e-10 { 0. } else { -c / b }
    } else if discriminant <= 0. {
        0.
    } else {
        (-b + discriminant.sqrt()) / (2. * a)
    };
    Ok(lab_to_xyz([l.clamp(0., 50.), 0., 0.]))
}

pub(super) fn compensate(xyz: [f64; 3], source: [f64; 3], destination: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| {
        let scale = (D50[i] - destination[i]) / (D50[i] - source[i]);
        xyz[i] * scale + D50[i] * (1. - scale)
    })
}
