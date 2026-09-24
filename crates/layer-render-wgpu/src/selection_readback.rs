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
    convert: impl FnOnce(RegionResult, bool) -> T + Send + 'static,
) {
    let ready = readback.clone();
    readback
        .slice(..size)
        .map_async(wgpu::MapMode::Read, move |result| {
            let result = result
                .map_err(|e| GpuRasterError::MapFailed(e.to_string()))
                .and_then(|_| {
                    let bytes = ready
                        .slice(..size)
                        .get_mapped_range()
                        .map_err(|e| GpuRasterError::MapFailed(e.to_string()))?;
                    let read = |offset: usize| {
                        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
                    };
                    let words: std::sync::Arc<[u32]> =
                        (0..extent[0].div_ceil(if byte_coverage { 4 } else { 8 }) as usize
                            * extent[1] as usize)
                            .map(|i| read(32 + i * 4))
                            .collect();
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
                            pixels: std::sync::Arc::new(pixels),
                        },
                        read(coverage_size as usize + 20) != 0,
                    ))
                });
            ready.unmap();
            let _ = tx.send(result);
        });
}
