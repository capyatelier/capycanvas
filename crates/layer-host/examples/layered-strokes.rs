//! Renderer-only reproduction of the Apple layered 4K drawing workload.
//! Reports preparation/completion and upload work, not screen presentation.
use layer_host::{NativeHost, Renderer};
use layer_render_wgpu::WgpuRasterizer;
use layer_ui::{Platform, UiSession};
use serde_json::json;
use std::time::Instant;

fn action(host: &mut NativeHost, value: serde_json::Value) {
    host.dispatch(serde_json::from_value(value).unwrap())
        .unwrap();
    host.session.frame(0, 0).unwrap();
    host.session
        .renderer_mut()
        .0
        .as_mut()
        .unwrap()
        .wait_idle()
        .unwrap();
}

fn main() {
    let (brush, diameter) = match std::env::args().nth(1).as_deref() {
        None | Some("ink") => (1, 24),
        Some("wet-watercolor") => (21, 320),
        _ => panic!("usage: layered-strokes [ink|wet-watercolor] [frames]"),
    };
    let frames: usize = std::env::args().nth(2).map(|n| n.parse().unwrap()).unwrap_or(256);
    assert!((1..=100_000).contains(&frames));
    let project = layer_ui::new_drawing(4096, 4096).unwrap();
    let gpu = WgpuRasterizer::new_native_headless(project.document.color).unwrap();
    let mut host = NativeHost::new(Platform::Mac).unwrap();
    host.session =
        UiSession::from_project(Renderer(Some(gpu)), project, None, [1600, 1200]).unwrap();
    host.session.set_platform(Platform::Mac);
    host.resize(1600, 1200, 1.).unwrap();
    for layer in 0..7 {
        action(
            &mut host,
            json!({"type":"set_color","rgba":[(layer % 3) as f64 * 0.3 + 0.1,0.25,0.55,0.2]}),
        );
        for command in ["select_all", "fill_selection", "deselect", "add_layer"] {
            action(&mut host, json!({"type":"invoke","command":command}));
        }
    }
    action(&mut host, json!({"type":"select_brush","id":brush}));
    action(&mut host, json!({"type":"set_brush_size","value":diameter}));
    action(
        &mut host,
        json!({"type":"set_color","rgba":[0.08,0.2,0.55,1.]}),
    );
    action(&mut host, json!({"type":"invoke","command":"fit_canvas"}));
    assert_eq!(host.session.engine().document().layers.len(), 9);
    let camera = host.session.state().camera.clone();
    println!(
        "frame,stroke_sample,prepare_ms,complete_ms,composited_pixels,source_misses,upload_submissions,display_submissions,paint_pages"
    );
    for frame in 0..frames {
        let before = host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .unwrap()
            .metrics();
        let mut records = Vec::new();
        let mut contact = 0;
        for index in frame * 3..(frame + 1) * 3 {
            let point = index % 384;
            if point >= 360 {
                continue;
            }
            contact = index / 384 + 1;
            let t = point as f64 / 359.;
            let shift = (contact - 1) as f64 * 0.37;
            let x = 4096. * (0.5 + 0.32 * (t * std::f64::consts::TAU + shift).sin());
            let y = 4096. * (0.5 + 0.27 * (t * std::f64::consts::PI * 3. + shift * 0.7).sin());
            records.extend_from_slice(&[
                camera.translation[0] as f64 + x * camera.zoom as f64,
                camera.translation[1] as f64 + y * camera.zoom as f64,
                0.25 + 0.75 * (t * std::f64::consts::PI).sin(),
                0.2,
                0.1,
                0.3,
                0.,
                1e9 + index as f64 / 240. * 1e9,
                if point == 0 {
                    1.
                } else if point == 359 {
                    3.
                } else {
                    2.
                },
            ]);
        }
        let now = 1_000_000_000 + ((frame + 1) as u64 * 3 * 1_000_000_000 / 240);
        if !records.is_empty() {
            host.pointer(contact as u64, 0, 0, &records, false).unwrap();
        }
        let start = Instant::now();
        host.session.frame(now, now + 11_111_111).unwrap();
        let prepare = start.elapsed().as_secs_f64() * 1000.;
        host.session
            .renderer_mut()
            .0
            .as_mut()
            .unwrap()
            .wait_idle()
            .unwrap();
        let complete = start.elapsed().as_secs_f64() * 1000.;
        let after = host
            .session
            .engine()
            .backend()
            .0
            .as_ref()
            .unwrap()
            .metrics();
        println!(
            "{frame},{},{prepare:.3},{complete:.3},{},{},{},{},{}",
            frame * 3 % 384,
            after.composited_pixels - before.composited_pixels,
            after.source_tile_misses - before.source_tile_misses,
            after.source_upload_submissions - before.source_upload_submissions,
            after.display_composition_submissions - before.display_composition_submissions,
            after.paint_pages
        );
    }
}
