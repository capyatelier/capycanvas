//! Windows image decoding runs on the document worker, never the canvas owner.
//! Only packed, straight-alpha sRGB source pixels leave this module.
use layer_core::ProjectAsset;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) const MAX_DIMENSION: u32 = 8192;

fn byte_count(extent: [u32; 2], limit: u32) -> Result<usize, String> {
    if extent
        .iter()
        .any(|&n| n == 0 || n > limit.min(MAX_DIMENSION))
    {
        return Err(format!(
            "Import an image up to {} × {} pixels",
            limit.min(MAX_DIMENSION),
            limit.min(MAX_DIMENSION)
        ));
    }
    Ok(extent[0] as usize * extent[1] as usize * 4)
}

pub(crate) fn decode(
    path: &Path,
    limit: u32,
    stopping: &AtomicBool,
    cancelled: &AtomicBool,
) -> Result<ProjectAsset, String> {
    let cancelled = || stopping.load(Ordering::Acquire) || cancelled.load(Ordering::Acquire);
    if cancelled() {
        return Err("Image import cancelled".into());
    }
    #[cfg(target_os = "windows")]
    {
        windows_decode(path, limit, &cancelled)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (path, byte_count([1, 1], limit));
        Err("Image decoding requires Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn windows_decode(
    path: &Path,
    limit: u32,
    cancelled: &impl Fn() -> bool,
) -> Result<ProjectAsset, String> {
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use windows::{
        Graphics::Imaging::{
            BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform,
            ColorManagementMode, ExifOrientationMode,
        },
        Storage::StorageFile,
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        core::{HSTRING, RuntimeType},
    };
    use windows_future::{AsyncStatus, IAsyncOperation};

    // A worker apartment owns all WinRT objects and is balanced even on error.
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(decode_error)?;
    let _apartment = Apartment;
    fn wait<T: RuntimeType>(
        operation: IAsyncOperation<T>,
        cancelled: &impl Fn() -> bool,
    ) -> Result<T, String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while operation.Status().map_err(decode_error)? == AsyncStatus::Started {
            if cancelled() || Instant::now() >= deadline {
                let _ = operation.Cancel();
                return Err(if cancelled() {
                    "Image import cancelled"
                } else {
                    "Image decoding timed out"
                }
                .into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        if cancelled() {
            return Err("Image import cancelled".into());
        }
        operation.GetResults().map_err(decode_error)
    }

    let file = wait(
        StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))
            .map_err(decode_error)?,
        cancelled,
    )?;
    let stream = wait(file.OpenReadAsync().map_err(decode_error)?, cancelled)?;
    let decoder = wait(
        BitmapDecoder::CreateAsync(&stream).map_err(decode_error)?,
        cancelled,
    )?;
    // Check both source and oriented bounds before asking the codec for pixels.
    byte_count(
        [
            decoder.PixelWidth().map_err(decode_error)?,
            decoder.PixelHeight().map_err(decode_error)?,
        ],
        limit,
    )?;
    let extent = [
        decoder.OrientedPixelWidth().map_err(decode_error)?,
        decoder.OrientedPixelHeight().map_err(decode_error)?,
    ];
    let expected = byte_count(extent, limit)?;
    let pixels = wait(
        decoder
            .GetPixelDataTransformedAsync(
                BitmapPixelFormat::Rgba8,
                BitmapAlphaMode::Straight,
                &BitmapTransform::new().map_err(decode_error)?,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::ColorManageToSRgb,
            )
            .map_err(decode_error)?,
        cancelled,
    )?
    .DetachPixelData()
    .map_err(decode_error)?;
    if pixels.len() != expected {
        return Err("The image decoder returned incomplete pixels".into());
    }
    if cancelled() {
        return Err("Image import cancelled".into());
    }
    // This one source-pixel copy also runs on the worker. The owner receives an Arc.
    Ok(ProjectAsset {
        extent,
        format: layer_core::ProjectAssetFormat::Rgba8Srgb,
        bytes: Arc::from(pixels.as_slice()),
    })
}

#[cfg(target_os = "windows")]
fn decode_error(error: windows::core::Error) -> String {
    // Windows error messages can contain private file paths; report only the code.
    format!(
        "Windows could not decode this image ({:#010x}).",
        error.code().0 as u32
    )
}

#[cfg(all(test, target_os = "windows"))]
#[path = "image_import_tests.rs"]
mod windows_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn size_limits_and_cancel_are_checked_without_decoding() {
        assert_eq!(byte_count([8192, 8192], 16384).unwrap(), 256 * 1024 * 1024);
        for extent in [[0, 10], [10, 0], [8193, 1], [1, u32::MAX]] {
            assert!(byte_count(extent, 16384).is_err());
        }
        assert!(byte_count([2049, 1], 2048).is_err());
        for (stop, cancel) in [(true, false), (false, true)] {
            assert!(
                decode(
                    Path::new("not-opened"),
                    8192,
                    &AtomicBool::new(stop),
                    &AtomicBool::new(cancel)
                )
                .unwrap_err()
                .contains("cancelled")
            );
        }
    }
}
