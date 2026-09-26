//! One asynchronous immutable coverage capture shared by region and paint tools.
use super::*;
use layer_render::RegionResult;

pub(super) fn capture_selection<T: Send + 'static>(
    readback: &wgpu::Buffer,
    size: u64,
    coverage_size: u64,
    extent: [u32; 2],
    byte_coverage: bool,
    request_id: u64,
    tx: mpsc::Sender<Result<T, GpuRasterError>>,
    convert: impl FnOnce(RegionResult, bool, &[u8]) -> T + Send + 'static,
) {
    crate::raster::map_then(
        readback,
        size,
        move |bytes| {
            let read =
                |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let count =
                extent[0].div_ceil(if byte_coverage { 4 } else { 8 }) as usize * extent[1] as usize;
            // One bulk copy into immutable history. A per-word iterator
            // also builds an intermediate Vec and performs slow scalar
            // reads from mapped device memory on mobile GPUs.
            #[cfg(target_endian = "little")]
            let words: std::sync::Arc<[u32]> = {
                let source = &bytes[32..32 + count * 4];
                let mut words = std::sync::Arc::<[u32]>::new_uninit_slice(count);
                // SAFETY: the fresh, uniquely owned allocation has count
                // u32 slots. Copy exactly that many initialized bytes;
                // every u32 bit pattern is valid, in native byte order.
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.as_ptr(),
                        std::sync::Arc::get_mut(&mut words)
                            .unwrap()
                            .as_mut_ptr()
                            .cast::<u8>(),
                        source.len(),
                    );
                    words.assume_init()
                }
            };
            #[cfg(target_endian = "big")]
            let words: std::sync::Arc<[u32]> = (0..count).map(|i| read(32 + i * 4)).collect();
            let bounds = if read(coverage_size as usize + 16) == 0 {
                [0; 4]
            } else {
                std::array::from_fn(|i| read(coverage_size as usize + i * 4))
            };
            let pixels = if byte_coverage {
                layer_core::SelectionPixels::bytes(extent, bounds, words)
            } else {
                layer_core::SelectionPixels::new(extent, bounds, words)
            }
            .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
            Ok(convert(
                RegionResult {
                    request_id,
                    tonal_sample: None,
                    pixels: std::sync::Arc::new(pixels),
                },
                read(coverage_size as usize + 20) != 0,
                &bytes[coverage_size as usize + 32..],
            ))
        },
        move |result| {
            let _ = tx.send(result);
        },
    );
}
