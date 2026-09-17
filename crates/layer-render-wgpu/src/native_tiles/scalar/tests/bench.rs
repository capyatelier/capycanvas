use super::*;

fn wait(r: &WgpuRasterizer) {
    r.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(crate::READBACK_TIMEOUT),
        })
        .unwrap();
}
fn report(label: &str, mut times: Vec<[f64; 2]>) {
    for axis in 0..2 {
        times.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
        println!(
            "NATIVE_SCALAR {label} kind={} p95_ms={:.4} p99_ms={:.4}",
            ["cpu", "complete"][axis],
            times[94][axis],
            times[98][axis]
        );
    }
}
#[test]
#[ignore = "physical scalar writeback/restore workload measurement"]
fn scalar_native_workloads() {
    let mut r = WgpuRasterizer::new_headless().unwrap();
    let start = std::time::Instant::now();
    let encoder = NativeScalarEncoder::with_device(&r.device);
    println!(
        "NATIVE_SCALAR pipeline_prepare_ms={:.4}",
        start.elapsed().as_secs_f64() * 1000.
    );
    let status = NativeEncodeStatus::new(&r.device);
    let working: Vec<_> = (0..16).map(|_| texture(&r)).collect();
    let canonical: Vec<_> = (0..16).map(|_| texture(&r)).collect();
    for depth in [SampleDepth::U8, SampleDepth::U16] {
        let buffers: Vec<_> = (0..16).map(|_| buffer(&r, depth)).collect();
        let descriptor = PixelDescriptor {
            bits_per_channel: depth.bits(),
            ..PixelDescriptor::COVERAGE8
        };
        // Permuted sample codes include every native level without compressing
        // a uniform fixture; the shader's input arithmetic stays Float32.
        let blobs: Vec<_> = (0..16u32)
            .map(|tile| {
                let bytes: Vec<_> = (0..65536u32)
                    .flat_map(|i| {
                        ((i.wrapping_mul(8191) + tile * 31) as u16)
                            .to_le_bytes()
                            .into_iter()
                            .take(depth.bytes())
                    })
                    .collect();
                layer_core::raster::TileBlob::encode(descriptor, &bytes).unwrap()
            })
            .collect();
        for count in [1, 16] {
            let restores: Vec<_> = (0..count)
                .map(|i| NativeScalarRestore {
                    blob: &blobs[i],
                    working: &working[i],
                })
                .collect();
            let mut times = Vec::new();
            for frame in 0..120 {
                let start = std::time::Instant::now();
                r.restore_native_scalars(&restores).unwrap();
                let cpu = start.elapsed().as_secs_f64() * 1000.;
                wait(&r);
                let complete = start.elapsed().as_secs_f64() * 1000.;
                if frame == 0 {
                    println!(
                        "NATIVE_SCALAR restore_cold depth={depth:?} tiles={count} cpu_ms={cpu:.4} complete_ms={complete:.4}"
                    );
                }
                if frame >= 20 {
                    times.push([cpu, complete]);
                }
            }
            report(&format!("restore depth={depth:?} tiles={count}"), times);
            for region in [[0, 0, 256, 256], [17, 31, 63, 65]] {
                let requests: Vec<_> = (0..count)
                    .map(|i| NativeScalarRequest {
                        working: &working[i],
                        encoded: &buffers[i],
                        canonical: &canonical[i],
                        depth,
                        region,
                    })
                    .collect();
                let prepared = encoder.prepare(&r.device, &requests, &status).unwrap();
                for reuse in [false, true] {
                    let mut times = Vec::new();
                    for frame in 0..120 {
                        let start = std::time::Instant::now();
                        let rebuilt = (!reuse)
                            .then(|| encoder.prepare(&r.device, &requests, &status).unwrap());
                        submit(&r, &encoder, &status, rebuilt.as_ref().unwrap_or(&prepared));
                        let cpu = start.elapsed().as_secs_f64() * 1000.;
                        wait(&r);
                        let complete = start.elapsed().as_secs_f64() * 1000.;
                        if frame >= 20 {
                            times.push([cpu, complete]);
                        }
                    }
                    report(
                        &format!(
                            "encode depth={depth:?} tiles={count} region={}x{} reuse={reuse}",
                            region[2], region[3]
                        ),
                        times,
                    );
                }
            }
        }
    }
    println!(
        "NATIVE_SCALAR upload_peak={}",
        r.metrics.source_upload_peak_bytes
    );
}
