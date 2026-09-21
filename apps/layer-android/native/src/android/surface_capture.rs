//! Instrumentation reads the retained image itself: PixelCopy cannot acquire an
//! already acquired shared swapchain. Never used by the production render loop.
use super::*;
use jni::sys::jbyteArray;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);

fn pixels(a: &App) -> Result<Vec<u8>, String> {
    if !a.profiling {
        return Err("Surface capture requires a debug test session".into());
    }
    let surface = a.surface.as_ref().ok_or("Missing test surface")?;
    let gpu = a
        .host
        .session
        .engine()
        .backend()
        .0
        .as_ref()
        .ok_or("Missing test GPU")?;
    let device = gpu.device();
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(TIMEOUT),
        })
        .map_err(error)?;
    let start = Instant::now();
    let target = loop {
        match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(target) | wgpu::CurrentSurfaceTexture::Suboptimal(target) => break target,
            wgpu::CurrentSurfaceTexture::Timeout if start.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(1))
            }
            other => return Err(format!("Test surface acquisition failed: {other:?}")),
        }
    };
    let bytes_per_pixel = match surface.config.format {
        wgpu::TextureFormat::Rgba16Float => 8,
        wgpu::TextureFormat::Rgba8Unorm
        | wgpu::TextureFormat::Rgba8UnormSrgb
        | wgpu::TextureFormat::Bgra8Unorm
        | wgpu::TextureFormat::Bgra8UnormSrgb => 4,
        other => return Err(format!("Unsupported test surface format: {other:?}")),
    };
    let stride = surface.config.width * bytes_per_pixel;
    let padded =
        stride.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test retained surface readback"),
        size: u64::from(padded) * u64::from(surface.config.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        target.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        target.texture.size(),
    );
    let submission = gpu.queue().submit([encoder.finish()]);
    gpu.queue().present(target);
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(TIMEOUT),
        })
        .map_err(error)?;
    receive
        .recv_timeout(TIMEOUT)
        .map_err(error)?
        .map_err(error)?;
    let mapped = buffer.slice(..).get_mapped_range().map_err(error)?;
    let mut bytes = vec![0; stride as usize * surface.config.height as usize];
    for (source, destination) in mapped
        .chunks_exact(padded as usize)
        .zip(bytes.chunks_exact_mut(stride as usize))
    {
        destination.copy_from_slice(&source[..stride as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(bytes)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_art_capycanvas_Native_surfacePixelsForTest(
    mut env: JNIEnv,
    _: JClass,
    handle: jlong,
) -> jbyteArray {
    match pixels(unsafe { app(handle) })
        .and_then(|bytes| env.byte_array_from_slice(&bytes).map_err(error))
    {
        Ok(array) => array.into_raw(),
        Err(e) => {
            fail(&mut env, Err(e));
            std::ptr::null_mut()
        }
    }
}
