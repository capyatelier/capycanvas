use super::*;

fn resize(source: [u32; 2], target: [u32; 2], pixels: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut resampler = RowResampler::new(source, target).unwrap();
    let mut result = vec![[0.; 4]; (target[0] * target[1]) as usize];
    for (y, output) in result.chunks_exact_mut(target[0] as usize).enumerate() {
        resampler
            .read_row(y as u32, output, |sy, row| {
                let start = (sy * source[0]) as usize;
                row.copy_from_slice(&pixels[start..start + source[0] as usize]);
                Ok(())
            })
            .unwrap();
    }
    result
}

// Independent reference: integrate pixel rectangles for reductions; evaluate
// the cubic Hermite interpolant (endpoint slopes are centered differences) for
// enlargement. This uses neither the production kernel nor its cached taps.
fn reference_axis(count: u32, target: u32, index: u32, value: impl Fn(i64) -> f64) -> f64 {
    let at = |i: i64| value(i.clamp(0, i64::from(count) - 1));
    if target == count {
        return at(i64::from(index));
    }
    if target < count {
        let left = f64::from(index) * f64::from(count) / f64::from(target);
        let right = f64::from(index + 1) * f64::from(count) / f64::from(target);
        return (0..count)
            .map(|i| {
                let overlap = (right.min(f64::from(i + 1)) - left.max(f64::from(i))).max(0.);
                at(i64::from(i)) * overlap / (right - left)
            })
            .sum();
    }
    let position = (f64::from(index) + 0.5) * f64::from(count) / f64::from(target) - 0.5;
    let i = position.floor() as i64;
    let t = position - i as f64;
    let a = at(i);
    let b = at(i + 1);
    let da = (b - at(i - 1)) / 2.;
    let db = (at(i + 2) - a) / 2.;
    (2. * t.powi(3) - 3. * t.powi(2) + 1.) * a
        + (t.powi(3) - 2. * t.powi(2) + t) * da
        + (-2. * t.powi(3) + 3. * t.powi(2)) * b
        + (t.powi(3) - t.powi(2)) * db
}

#[test]
fn separable_rows_match_independent_area_and_hermite_reference() {
    let source = [7, 5];
    let pixels: Vec<[f32; 4]> = (0..35)
        .map(|i| {
            let alpha = [0., 1. / 65535., 0.25, 0.5, 1.][i % 5];
            [
                alpha * (i % 7) as f32 / 6.,
                alpha * (i % 3) as f32 / 2.,
                -0.1 * alpha,
                alpha,
            ]
        })
        .collect();
    for target in [
        [1, 1],
        [3, 2],
        [7, 5],
        [23, 19],
        [3, 19],
        [23, 2],
        [1, 97],
        [89, 1],
    ] {
        let actual = resize(source, target, &pixels);
        for y in 0..target[1] {
            for x in 0..target[0] {
                let reference: [f64; 4] = std::array::from_fn(|c| {
                    reference_axis(source[1], target[1], y, |sy| {
                        reference_axis(source[0], target[0], x, |sx| {
                            f64::from(pixels[sy as usize * 7 + sx as usize][c])
                        })
                    })
                });
                let alpha = reference[3].clamp(0., 1.);
                let expected = if alpha == 0. {
                    [0.; 4]
                } else {
                    [
                        reference[0] * alpha / reference[3],
                        reference[1] * alpha / reference[3],
                        reference[2] * alpha / reference[3],
                        alpha,
                    ]
                };
                for c in 0..4 {
                    let actual = f64::from(actual[(y * target[0] + x) as usize][c]);
                    assert!(
                        (actual - expected[c]).abs() <= 2e-7,
                        "{target:?} ({x},{y}) channel {c}: {actual} vs {}",
                        expected[c]
                    );
                }
            }
        }
    }
}

#[test]
fn reductions_include_thin_features_and_fractional_edges() {
    let pixels = [
        [0., 0., 0., 1.],
        [0., 0., 0., 1.],
        [1., 1., 1., 1.],
        [0., 0., 0., 1.],
        [0., 0., 0., 1.],
    ];
    assert_eq!(
        resize([5, 1], [2, 1], &pixels),
        vec![[0.2, 0.2, 0.2, 1.]; 2]
    );
    assert_eq!(
        resize([1, 5], [1, 2], &pixels),
        vec![[0.2, 0.2, 0.2, 1.]; 2]
    );
    let pixels = (0..1024)
        .map(|i| {
            if i % 2 == 0 {
                [1.; 4]
            } else {
                [0., 0., 0., 1.]
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(resize([1024, 1], [1, 1], &pixels), [[0.5, 0.5, 0.5, 1.]]);
}

#[test]
fn identity_constants_extended_color_and_transparent_edges_preserve_coverage() {
    let pixels = [
        [0.; 4],
        [1. / 65535., -0.25 / 65535., 0.5 / 65535., 1. / 65535.],
        [1.4, -0.2, 0.1, 1.],
    ];
    assert_eq!(resize([3, 1], [3, 1], &pixels), pixels);
    for pixel in pixels {
        for (source, target) in [([7, 5], [1, 1]), ([1, 1], [63, 61]), ([3, 11], [97, 2])] {
            let result = resize(
                source,
                target,
                &vec![pixel; (source[0] * source[1]) as usize],
            );
            assert!(
                result
                    .iter()
                    .all(|p| p.iter().zip(pixel).all(|(a, b)| (a - b).abs() < 2e-7))
            );
        }
    }
    let red = [[1., 0., 0., 1.], [0.; 4]];
    for target in [[1, 1], [37, 1]] {
        for pixel in resize([2, 1], target, &red) {
            assert!((0. ..=1.).contains(&pixel[3]));
            assert_eq!(
                pixel[0], pixel[3],
                "associated red must not darken against transparency"
            );
            assert_eq!(&pixel[1..3], &[0.; 2]);
        }
    }
}

#[test]
fn extreme_aspect_ratios_keep_four_cached_rows_and_read_sources_once() {
    for (source, target) in [
        ([1, 32768], [1, 1]),
        ([1, 32768], [17, 31]),
        ([1, 7], [1, 32768]),
    ] {
        let mut resampler = RowResampler::new(source, target).unwrap();
        let mut output = vec![[0.; 4]; target[0] as usize];
        let mut reads = Vec::new();
        for y in 0..target[1] {
            resampler
                .read_row(y, &mut output, |sy, row| {
                    reads.push(sy);
                    row.fill([0.25, 0.5, 0.75, 1.]);
                    Ok(())
                })
                .unwrap();
            assert!(resampler.cache.len() <= 4);
            assert_eq!(resampler.row.len(), source[0] as usize);
            assert_eq!(resampler.sum.len(), target[0] as usize);
            assert_eq!(resampler.horizontal.len(), target[0] as usize);
            assert!(output.iter().all(|p| *p == [0.25, 0.5, 0.75, 1.]));
        }
        assert_eq!(reads, (0..source[1]).collect::<Vec<_>>());
    }
}

#[test]
fn invalid_requests_and_provider_failure_do_not_publish_a_row() {
    for bad in [[0, 1], [1, 32769]] {
        assert!(RowResampler::new(bad, [1, 1]).is_err());
        assert!(RowResampler::new([1, 1], bad).is_err());
    }
    let mut resampler = RowResampler::new([1, 32768], [1, 1]).unwrap();
    let mut output = [[42.; 4]];
    assert!(
        resampler
            .read_row(1, &mut output, |_, _| unreachable!())
            .is_err()
    );
    assert!(
        resampler
            .read_row(0, &mut [], |_, _| unreachable!())
            .is_err()
    );
    let error = resampler
        .read_row(0, &mut output, |sy, row| {
            if sy == 17 {
                return Err("cancelled by user".into());
            }
            row.fill([0.; 4]);
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error, "cancelled by user");
    assert_eq!(output, [[42.; 4]]);
    assert!(
        resampler
            .read_row(0, &mut output, |_, _| unreachable!())
            .is_err()
    );
    let mut resampler = RowResampler::new([1, 1], [1, 1]).unwrap();
    assert!(
        resampler
            .read_row(0, &mut output, |_, row| {
                row[0] = [f32::NAN; 4];
                Ok(())
            })
            .unwrap_err()
            .contains("finite")
    );
    assert_eq!(output, [[42.; 4]]);
}
