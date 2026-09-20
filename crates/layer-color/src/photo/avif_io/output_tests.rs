use super::*;
use std::io::Cursor;

fn sample(x: u32, y: u32) -> [f32; 4] {
    let a = ((x * 389 + y * 601) % 4096) as f32 / 4095.;
    let r = if x % 7 < 3 { 4. } else { 0.025 };
    [
        r * a,
        (0.03 + (y % 13) as f32 * 0.04) * a,
        (0.1 + (x % 5) as f32 * 0.01) * a,
        a,
    ]
}
fn rows(y: u32, row: &mut [[f32; 4]]) -> Result<(), String> {
    for (x, p) in row.iter_mut().enumerate() {
        *p = sample(x as u32, y);
    }
    Ok(())
}

#[test]
fn rust_avif_export_reconstructs_compressed_base_and_preserves_alpha() {
    let cancel = AtomicBool::new(false);
    for extent in [[23, 17], [263, 65], [32, 24]] {
        for quality in [25, 90, 100] {
            let (bytes, stats) = encode(
                extent,
                RgbSpace::Srgb,
                Default::default(),
                None,
                quality,
                Some(layer_core::ImageResolution::ppi(300)),
                None,
                false,
                128 * 1024 * 1024,
                &cancel,
                rows,
            )
            .unwrap();
            assert_eq!(stats.clipped_channels, 0);
            let hdr = super::super::read(Cursor::new(&bytes), Default::default(), &cancel)
                .unwrap()
                .source;
            let base = read_rendition(Cursor::new(&bytes), Default::default(), &cancel, false)
                .unwrap()
                .source;
            assert_eq!(hdr.extent, extent);
            assert_eq!(base.extent, extent);
            assert_eq!(hdr.interpretation.depth, SampleDepth::F16);
            assert_eq!(base.interpretation.depth, SampleDepth::U16);
            assert_eq!(hdr.resolution, Some(layer_core::ImageResolution::ppi(300)));
            let mut hdr_rows = hdr.rows();
            let mut base_rows = base.rows();
            let mut decoded = vec![0; hdr.row_bytes()];
            let mut sdr = vec![0; base.row_bytes()];
            let mut maximum = 0f32;
            for y in 0..extent[1] {
                hdr_rows.read(y, &mut decoded).unwrap();
                base_rows.read(y, &mut sdr).unwrap();
                for (x, p) in decoded.chunks_exact(8).enumerate() {
                    let actual = hdr::decode_pixel(std::array::from_fn(|c| {
                        u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])
                    }))
                    .unwrap();
                    let expected = sample(x as u32, y);
                    if expected[3] > 0. {
                        for c in 0..3 {
                            let error = (actual[c] - expected[c] / expected[3]).abs();
                            maximum = maximum.max(error);
                            assert!(
                                error < 0.01,
                                "{extent:?} q={quality} ({x},{y}) {actual:?} expected {expected:?}"
                            );
                        }
                    }
                    assert!((actual[3] - expected[3]).abs() <= 0.00025);
                    let code = (expected[3] * 4095.).round() as u32;
                    let alpha = u16::from_le_bytes([sdr[x * 8 + 6], sdr[x * 8 + 7]]);
                    assert_eq!(
                        u32::from(alpha),
                        (code * 65535 + 2047) / 4095,
                        "lossless alpha"
                    );
                }
            }
            eprintln!(
                "Rust AVIF {extent:?} quality={quality}: {} bytes, HDR max error={maximum}",
                bytes.len()
            );
            if let Some(directory) = std::env::var_os("LAYER_AVIF_OUTPUT") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let stem = format!("{}x{}-q{quality}", extent[0], extent[1]);
                std::fs::write(directory.join(format!("{stem}.avif")), &bytes).unwrap();
                for (name, source) in [("hdr", &hdr), ("base", &base)] {
                    let mut full = Vec::new();
                    let mut row = vec![0; source.row_bytes()];
                    let mut rows = source.rows();
                    for y in 0..extent[1] {
                        rows.read(y, &mut row).unwrap();
                        full.extend_from_slice(&row);
                    }
                    std::fs::write(directory.join(format!("{stem}.{name}")), full).unwrap();
                }
            }
        }
    }
}

#[test]
fn rust_avif_export_checks_admission_cancellation_and_pixel_errors() {
    let cancel = AtomicBool::new(false);
    let attempt = |budget, read| {
        encode(
            [23, 17],
            RgbSpace::Srgb,
            Default::default(),
            None,
            90,
            None,
            None,
            false,
            budget,
            &cancel,
            read,
        )
    };
    assert!(attempt(1024, rows).unwrap_err().contains("memory budget"));
    cancel.store(true, Ordering::Release);
    assert!(
        attempt(128 * 1024 * 1024, rows)
            .unwrap_err()
            .contains("cancelled")
    );
    cancel.store(false, Ordering::Release);
    assert!(attempt(128 * 1024 * 1024, rows).is_ok());
    let bad = |_: u32, row: &mut [[f32; 4]]| {
        row.fill([f32::NAN, 0., 0., 1.]);
        Ok(())
    };
    assert!(
        encode(
            [8, 8],
            RgbSpace::Srgb,
            Default::default(),
            None,
            90,
            None,
            None,
            false,
            128 * 1024 * 1024,
            &cancel,
            bad
        )
        .is_err()
    );
}

#[test]
fn rust_avif_preview_preserves_hdr_when_the_authored_sdr_changes() {
    let cancel = AtomicBool::new(false);
    let extent = [23, 17];
    let capture = |exposure| {
        preview(
            extent,
            [12, 12],
            RgbSpace::Srgb,
            hdr::SdrRendition {
                exposure,
                ..Default::default()
            },
            None,
            50,
            None,
            &cancel,
            rows,
        )
        .unwrap()
    };
    let (preview_extent, hdr, sdr, _) = capture(0.);
    let (other_extent, other_hdr, other_sdr, _) = capture(-2.);
    assert_eq!(preview_extent, [12, 9]);
    assert_eq!(other_extent, preview_extent);
    for (a, b) in hdr.iter().zip(&other_hdr) {
        for c in 0..4 {
            assert!((a[c] - b[c]).abs() < 0.004);
        }
    }
    assert!(sdr.iter().zip(&other_sdr).any(|(a, b)| a[0] > b[0] + 0.04));
    assert!(hdr.iter().zip(&sdr).any(|(h, s)| h[0] > s[0] + 0.2));
}

#[cfg(all(feature = "heif", target_os = "linux"))]
#[test]
#[ignore = "independent libavif/dav1d reference bundle via CAPY_PHOTO_CODEC_DIR"]
fn rust_avif_output_interoperates_with_native_libavif() {
    let cancel = AtomicBool::new(false);
    for extent in [[23, 17], [263, 65], [32, 24]] {
        for quality in [25, 90, 100] {
            let (bytes, _) = encode(
                extent,
                RgbSpace::Srgb,
                Default::default(),
                None,
                quality,
                Some(layer_core::ImageResolution::ppi(300)),
                None,
                false,
                128 * 1024 * 1024,
                &cancel,
                rows,
            )
            .unwrap();
            let rust = super::super::read(Cursor::new(&bytes), Default::default(), &cancel)
                .unwrap()
                .source;
            let native = crate::photo::gainmap::native::read_gainmap(
                Cursor::new(&bytes),
                crate::photo::GainMapFormat::Avif,
                Default::default(),
                &cancel,
            )
            .unwrap();
            assert_eq!(rust.extent, native.extent);
            assert_eq!(rust.resolution, native.resolution);
            let mut a = vec![0; rust.row_bytes()];
            let mut b = vec![0; native.row_bytes()];
            let mut ar = rust.rows();
            let mut br = native.rows();
            let mut maximum = 0f32;
            for y in 0..extent[1] {
                ar.read(y, &mut a).unwrap();
                br.read(y, &mut b).unwrap();
                for (a, b) in a.chunks_exact(8).zip(b.chunks_exact(8)) {
                    let pixel = |p: &[u8]| {
                        hdr::decode_pixel(std::array::from_fn(|c| {
                            u16::from_le_bytes([p[c * 2], p[c * 2 + 1]])
                        }))
                        .unwrap()
                    };
                    for (a, b) in pixel(a).into_iter().zip(pixel(b)) {
                        maximum = maximum.max((a - b).abs());
                    }
                }
            }
            eprintln!(
                "Independent libavif {extent:?} q={quality} HDR maximum difference {maximum}"
            );
            assert!(maximum <= 0.004);
        }
    }
}
