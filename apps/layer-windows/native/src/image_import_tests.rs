//! Generated in-memory fixtures; no user images or private paths are retained.
use super::*;
use std::{fs, path::PathBuf, sync::atomic::AtomicU64};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "capy-image-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn png(
        &self,
        name: &str,
        extent: [u32; 2],
        format: png::ColorType,
        depth: png::BitDepth,
        bytes: &[u8],
    ) -> PathBuf {
        let path = self.0.join(name);
        let mut encoder = png::Encoder::new(fs::File::create(&path).unwrap(), extent[0], extent[1]);
        encoder.set_color(format);
        encoder.set_depth(depth);
        encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(bytes)
            .unwrap();
        path
    }
    fn decode(&self, path: &Path) -> Result<ProjectAsset, String> {
        decode(path, 8192, &AtomicBool::new(false), &AtomicBool::new(false))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for entry in fs::read_dir(&self.0).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            fs::remove_file(entry.path()).unwrap();
        }
        fs::remove_dir(&self.0).unwrap();
    }
}

#[test]
fn windows_decoder_preserves_straight_alpha_and_expands_gray_and_16bit() {
    let f = Fixture::new();
    let rgba = [255, 0, 0, 128, 0, 255, 0, 255, 10, 20, 30, 0];
    let path = f.png(
        "alpha.png",
        [3, 1],
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &rgba,
    );
    let result = f.decode(&path).unwrap();
    assert_eq!(result.extent, [3, 1]);
    assert_eq!(result.format, layer_core::ProjectAssetFormat::Rgba8Srgb);
    assert_eq!(&*result.bytes, &rgba);

    let gray = f.png(
        "gray.png",
        [2, 1],
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Eight,
        &[73, 129, 204, 255],
    );
    assert_eq!(
        &*f.decode(&gray).unwrap().bytes,
        &[73, 73, 73, 129, 204, 204, 204, 255]
    );
    let sixteen = f.png(
        "sixteen.png",
        [1, 1],
        png::ColorType::Rgba,
        png::BitDepth::Sixteen,
        &[255, 255, 0, 0, 128, 128, 255, 255],
    );
    assert_eq!(&*f.decode(&sixteen).unwrap().bytes, &[255, 0, 128, 255]);
}

#[test]
fn windows_decoder_rejects_oversized_corrupt_and_missing_sources_without_paths_in_errors() {
    let f = Fixture::new();
    let wide = f.png(
        "wide.png",
        [8193, 1],
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &vec![255; 8193 * 4],
    );
    assert!(f.decode(&wide).unwrap_err().contains("8192"));
    let corrupt = f.0.join("private-fixture.png");
    fs::write(&corrupt, b"not an image").unwrap();
    for path in [&corrupt, &f.0.join("private-missing.png")] {
        let error = f.decode(path).unwrap_err();
        assert!(error.contains("could not decode"));
        assert!(!error.contains("private"));
        assert!(!error.contains(&f.0.to_string_lossy().to_string()));
    }
    let small = f.png(
        "small.png",
        [4, 1],
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[255; 16],
    );
    assert!(
        decode(&small, 2, &AtomicBool::new(false), &AtomicBool::new(false))
            .unwrap_err()
            .contains("2 × 2")
    );
}

#[test]
fn windows_decoder_cancels_between_native_async_stages() {
    let f = Fixture::new();
    let path = f.png(
        "cancel.png",
        [4, 1],
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[255; 16],
    );
    let checks = AtomicU64::new(0);
    let error =
        windows_decode(&path, 8192, &|| checks.fetch_add(1, Ordering::Relaxed) >= 2).unwrap_err();
    assert!(error.contains("cancelled"));
}

#[test]
fn windows_decoder_applies_exif_orientation_to_pixels_and_extent() {
    let f = Fixture::new();
    // Minimal uncompressed RGB TIFF, 3 × 2, EXIF orientation 6 (90 degrees CW).
    let tags: [(u16, u16, u32, u32); 11] = [
        (256, 4, 1, 3),
        (257, 4, 1, 2),
        (258, 3, 3, 146),
        (259, 3, 1, 1),
        (262, 3, 1, 2),
        (273, 4, 1, 152),
        (274, 3, 1, 6),
        (277, 3, 1, 3),
        (278, 4, 1, 2),
        (279, 4, 1, 18),
        (284, 3, 1, 1),
    ];
    let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    bytes.extend_from_slice(&11u16.to_le_bytes());
    for (tag, kind, count, value) in tags {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[8, 0, 8, 0, 8, 0]);
    bytes.extend_from_slice(&[
        255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 255, 255, 255, 0, 255, 255, 255, 0,
    ]);
    let path = f.0.join("rotated.tif");
    fs::write(&path, bytes).unwrap();
    let image = f.decode(&path).unwrap();
    assert_eq!(image.extent, [2, 3]);
    assert_eq!(
        &*image.bytes,
        &[
            0, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 255, 0, 255, 0, 255, 255, 255, 0, 255,
            0, 0, 255, 255
        ]
    );
}
