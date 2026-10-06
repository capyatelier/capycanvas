//! Frame timings of the GTK native renderer on an isolated compositor.
use super::*;
use std::sync::{Arc, Mutex};

fn distribution(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    if values.is_empty() {
        return Value::Null;
    }
    json!({"count": values.len(), "p50": values[values.len() / 2],
        "p95": values[((values.len() as f64 * 0.95).ceil() as usize - 1).min(values.len() - 1)],
        "max": values.last().unwrap()})
}
pub(crate) fn frames(stats: &Arc<Mutex<crate::timing::Stats>>) -> Value {
    let stats = stats.lock().unwrap();
    let presented: Vec<_> = stats.presented.iter().filter(|v| v[3] == 1).collect();
    let mut previous_pose = None;
    let mut latency = Vec::new();
    let mut changed_inputs = 0;
    for (at, layer, _, pose) in &stats.photo_inputs {
        if previous_pose.as_ref() == Some(&(layer, pose)) {
            continue;
        }
        previous_pose = Some((layer, pose));
        changed_inputs += 1;
        if let Some(presented_at) = stats
            .photo_frames
            .iter()
            .filter(|(_, id, geometry)| id == layer && geometry == pose)
            .filter_map(|(frame, _, _)| {
                presented
                    .iter()
                    .find(|p| p[0] == *frame && p[1] >= *at)
                    .map(|p| p[1])
            })
            .min()
        {
            latency.push((presented_at - *at) as f64 / 1e6);
        }
    }
    let mut camera_frames = Vec::new();
    let mut previous_camera = None;
    for p in &presented {
        if let Some((_, camera, _)) = stats.camera_views.iter().find(|v| v.0 == p[0])
            && previous_camera != Some(*camera)
        {
            camera_frames.push(p[1]);
            previous_camera = Some(*camera);
        }
    }
    json!({
        "cpu_total_ms": distribution(stats.cpu.iter().map(|v| v[3]).collect()),
        "gpu_ms": distribution(stats.gpu.iter().map(|v| v[1]).collect()),
        "owner_input_ms": distribution(stats.input_handler_cpu.clone()),
        "presentation_gap_ms": distribution(presented.windows(2)
            .map(|v| v[1][1].saturating_sub(v[0][1]) as f64 / 1e6).collect()),
        "cpu": stats.cpu, "cpu_stages": stats.cpu_stages, "thread_cpu": stats.thread_cpu,
        "gpu": stats.gpu, "presented": stats.presented,
        "camera_views": stats.camera_views, "photo_inputs": stats.photo_inputs,
        "material_samples": stats.material_samples,
        "renderer_phases": stats.renderer_phases, "material_phases": stats.material_phases,
        "source_transfers": stats.source_transfers,
        "pen_routes": stats.pen_routes,
        "photo_frames": stats.photo_frames, "changed_pose_inputs": changed_inputs,
        "gtk_pose_to_presentation_ms": distribution(latency),
        "distinct_camera_presentations": camera_frames.len(),
        "distinct_camera_gap_ms": distribution(camera_frames.windows(2)
            .map(|v| v[1].saturating_sub(v[0]) as f64 / 1e6).collect()),
        "camera_work": stats.camera_work, "raster_commits": stats.raster_commits,
    })
}

