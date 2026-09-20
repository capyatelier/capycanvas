//! Opt-in live-app research harness, not a persisted brush type. Set only before
//! opening a benchmark document; changing this during a stroke is unsupported.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(cfg!(feature = "swept-brush-default"));
static INPUTS: AtomicU64 = AtomicU64::new(0);
static SEGMENTS: AtomicU64 = AtomicU64::new(0);
static BATCHES: AtomicU64 = AtomicU64::new(0);

#[doc(hidden)]
pub fn set_swept_brush_experiment_for_test(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    for counter in [&INPUTS, &SEGMENTS, &BATCHES] {
        counter.store(0, Ordering::Relaxed);
    }
}

#[doc(hidden)]
pub fn swept_brush_experiment_counts() -> [u64; 3] {
    [&INPUTS, &SEGMENTS, &BATCHES].map(|v| v.load(Ordering::Relaxed))
}

#[doc(hidden)]
pub fn swept_brush_experiment_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn supported_contact(contact: Option<layer_core::BrushContact>) -> bool {
    // The installed research build opts in only the tested ink/pencil models.
    // Calligraphy, rough nibs and the other contact media keep their renderer.
    let ink = layer_core::BrushContact::default();
    let pencil = layer_core::BrushContact {
        paper: 1.,
        pressure_gain: 0.9,
        tilt_spread: 2.,
        tilt_shading: 0.7,
        ..ink
    };
    contact == Some(ink) || contact == Some(pencil)
}

pub(super) fn active(style: &layer_render::DabStyle) -> bool {
    swept_brush_experiment_enabled()
        && style.execution == BrushExecution::Dry
        && style.rendering.blend_mode == BrushBlendMode::Normal
        && supported_contact(style.contact)
        && style.dual.is_none()
        && !style.rendering.edge_after_stroke
}

// Merge only constant-material round contacts. Retain the first contact's
// incoming span (which may begin in the previous frame) and never cross batches.
// The deliberately conservative 0.5px bound applies to centers plus radius.
fn merge(input: &[Dab], output: &mut Vec<Dab>) {
    if input.is_empty() {
        return;
    }
    let compatible = input.iter().all(|d| {
        d.color_rgba_linear == input[0].color_rgba_linear
            && d.flow == input[0].flow
            && d.contact[0] == input[0].contact[0]
            && d.contact[1] == 0.
            && d.radii[0] == d.radii[1]
    });
    if !compatible {
        output.extend_from_slice(input);
        return;
    }
    let mut start = input[0];
    start.center.x -= start.motion[0];
    start.center.y -= start.motion[1];
    if start.previous[0] > 0. {
        start.radii = [start.previous[0], start.previous[1]];
        start.contact = start.previous_contact;
    }
    fn fit(start: Dab, input: &[Dab], out: &mut Vec<Dab>, depth: u32) {
        let end = *input.last().unwrap();
        let dx = end.center.x - start.center.x;
        let dy = end.center.y - start.center.y;
        let mut worst = (0.0_f32, 0);
        for (i, d) in input[..input.len() - 1].iter().enumerate() {
            let x = d.center.x - start.center.x;
            let y = d.center.y - start.center.y;
            let t = ((x * dx + y * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0., 1.);
            let error = (x - t * dx).hypot(y - t * dy)
                + (d.radii[0] - (start.radii[0] + t * (end.radii[0] - start.radii[0]))).abs();
            if error > worst.0 {
                worst = (error, i);
            }
        }
        if worst.0 > 0.5 {
            // Bound recursion/work on pathological input. Keeping contacts is safe.
            if depth == 32 {
                out.extend_from_slice(input);
                return;
            }
            let split = worst.1 + 1;
            fit(start, &input[..split], out, depth + 1);
            fit(input[split - 1], &input[split..], out, depth + 1);
        } else {
            let mut segment = end;
            segment.motion = [dx, dy];
            segment.previous = [start.radii[0], start.radii[1], 1., 0.];
            segment.previous_contact = start.contact;
            out.push(segment);
        }
    }
    fit(start, input, output, 0);
}

pub(super) fn prepare(
    packet: FramePacket<'_>,
) -> Result<Option<(Vec<Dab>, Vec<DabBatch>)>, GpuRasterError> {
    if !packet.dab_batches.iter().any(|b| active(&b.style)) {
        return Ok(None);
    }
    let mut dabs = Vec::with_capacity(packet.dabs.len());
    let mut batches = packet.dab_batches.to_vec();
    for batch in &mut batches {
        let from = batch.first_dab as usize;
        let to = from
            .checked_add(batch.dab_count as usize)
            .ok_or(GpuRasterError::InvalidDabRange)?;
        let input = packet
            .dabs
            .get(from..to)
            .ok_or(GpuRasterError::InvalidDabRange)?;
        batch.first_dab = dabs.len() as u32;
        if active(&batch.style) {
            merge(input, &mut dabs);
            // Both new media are continuous stroke coverage. Pencil intentionally
            // does not emulate flow accumulation; separate strokes still layer.
            batch.style.rendering.accumulation = BrushAccumulation::Uniform;
            INPUTS.fetch_add(input.len() as u64, Ordering::Relaxed);
            SEGMENTS.fetch_add(
                dabs.len() as u64 - u64::from(batch.first_dab),
                Ordering::Relaxed,
            );
            BATCHES.fetch_add(1, Ordering::Relaxed);
        } else {
            dabs.extend_from_slice(input);
        }
        batch.dab_count = dabs.len() as u32 - batch.first_dab;
    }
    Ok(Some((dabs, batches)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_experiment_is_limited_to_tested_contact_models() {
        use layer_core::{DefaultBrushPreset, default_brush};
        for preset in layer_core::CONTACT_BRUSH_PRESETS {
            assert_eq!(
                supported_contact(default_brush(preset).contact),
                matches!(
                    preset,
                    DefaultBrushPreset::GPen | DefaultBrushPreset::Pencil
                ),
                "{preset:?}"
            );
        }
    }
    fn contact(x: f32, y: f32, motion: [f32; 2]) -> Dab {
        let mut d = crate::tests::test_dab([x, y], [1.; 4], 1.);
        d.radii = [10.; 2];
        d.previous = [10., 10., 1., 0.];
        d.motion = motion;
        d.contact = [1., 0., 0., 0.];
        d.previous_contact = d.contact;
        d
    }
    #[test]
    fn merging_preserves_incoming_span_and_end() {
        let input = [contact(10., 0., [10., 0.]), contact(20., 0., [10., 0.])];
        let mut output = Vec::new();
        merge(&input, &mut output);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].center.x, 20.);
        assert_eq!(output[0].motion, [20., 0.]);
    }
    #[test]
    fn merging_keeps_corners_pressure_and_empty_boundaries() {
        let mut output = Vec::new();
        merge(&[], &mut output);
        assert!(output.is_empty());
        let mut input = [contact(10., 10., [10., 10.]), contact(20., 0., [10., -10.])];
        merge(&input, &mut output);
        assert_eq!(output.len(), 2);
        output.clear();
        input[1].contact[0] = 0.5;
        merge(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    #[ignore = "process-wide experimental switch: run this GPU test by itself"]
    fn swept_gpu_batching_and_prediction() {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                set_swept_brush_experiment_for_test(false);
            }
        }
        set_swept_brush_experiment_for_test(true);
        let _reset = Reset;
        for preset in [
            layer_core::DefaultBrushPreset::GPen,
            layer_core::DefaultBrushPreset::Pencil,
        ] {
            let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
            let layer = Layer::paint(LayerId(1), "Swept regression");
            let brush = layer_core::default_brush(preset);
            let mut style = crate::tests::test_style(BrushExecution::Dry);
            style.contact = brush.contact;
            style.grain = brush.grain;
            style.rendering = brush.rendering;
            let mut batch = DabBatch {
                material_update: 0,
                stroke_id: StrokeId(1),
                layer_id: layer.id,
                kind: DabBatchKind::Persistent,
                stroke_start: true,
                stroke_end: false,
                first_dab: 0,
                dab_count: 3,
                style,
                damage: layer_core::Rect {
                    min: layer_core::Point { x: 0., y: 0. },
                    max: layer_core::Point { x: 256., y: 256. },
                },
            };
            let dabs = [
                contact(64., 128., [0., 0.]),
                contact(128., 128., [64., 0.]),
                contact(192., 128., [64., 0.]),
            ]
            .map(|mut d| {
                d.color_rgba_linear = [0.8, 0.1, 0.4, 0.6];
                d
            });
            let frame = |r: &mut WgpuRasterizer, dabs: &[Dab], batches: &[DabBatch], reset| {
                r.submit(FramePacket {
                    view: layer_render::ViewState {
                        width_px: 256,
                        height_px: 256,
                        document_to_surface: [1., 0., 0., 1., 0., 0.],
                        background_rgba_linear: [0.; 4],
                    },
                    document_extent: [256; 2],
                    layers: std::slice::from_ref(&layer),
                    dabs,
                    dab_batches: batches,
                    restore_rasters: &[],
                    reset_layers: reset,
                    composite_all: true,
                    time_seconds: 0.,
                })
                .unwrap();
            };
            let pixels = |r: &WgpuRasterizer| {
                crate::layer_tests::page_bytes(r, &r.paint_layers[0].pages[0].active().texture)
            };
            frame(&mut r, &dabs, &[batch.clone()], true);
            let together = pixels(&r);
            batch.dab_count = 1;
            for (i, d) in dabs.iter().enumerate() {
                batch.stroke_start = i == 0;
                frame(&mut r, std::slice::from_ref(d), &[batch.clone()], i == 0);
            }
            let split = pixels(&r);
            let mut painted = 0;
            for (a, b) in together.chunks_exact(4).zip(split.chunks_exact(4)) {
                let a = f32::from_le_bytes(a.try_into().unwrap());
                let b = f32::from_le_bytes(b.try_into().unwrap());
                assert!(
                    a.is_finite() && b.is_finite() && (a - b).abs() < 1e-5,
                    "{preset:?}: {a} != {b}"
                );
                painted += usize::from(a > 0.);
            }
            assert!(painted > 1000);
            let before = r.readback_srgb_rgba8().unwrap();
            batch.kind = DabBatchKind::Preview;
            let prediction = contact(192., 192., [0., 64.]);
            frame(&mut r, &[prediction], &[batch], false);
            assert_eq!(pixels(&r), split, "prediction changed persistent pixels");
            assert_ne!(
                r.readback_srgb_rgba8().unwrap(),
                before,
                "prediction must be visible"
            );
            frame(&mut r, &[], &[], false);
            assert_eq!(
                r.readback_srgb_rgba8().unwrap(),
                before,
                "prediction must retire"
            );
        }
    }
}
