//! Deterministic full/camera snapshot fixtures and CPU-only transport benchmark.
//! No GPU, platform windows, user persistence or document paths are used.
use layer_host::NativeHost;
use layer_ui::{Platform, UiAction, WorkspaceState};
use serde_json::json;
use std::io::Write;

fn host(platform: Platform) -> NativeHost {
    let mut host = NativeHost::new(platform).unwrap();
    host.dispatch(UiAction::RestoreWorkspace {
        workspace: WorkspaceState::for_platform(platform),
    })
    .unwrap();
    host.session.set_document_replacement(true);
    host.resize(2410, 1810, 2.).unwrap();
    host
}

fn main() {
    let stream = std::env::args().any(|a| a == "--stream");
    if std::env::args().any(|a| a == "--benchmark") {
        benchmark();
        return;
    }
    let take = |host: &mut NativeHost| {
        if stream {
            host.take_snapshot_bytes().unwrap()
        } else {
            host.take_snapshot()
                .map(|value| serde_json::to_vec(&value).unwrap())
        }
    };
    let mut fixtures = Vec::new();
    for platform in [Platform::Ios, Platform::Mac] {
        let mut host = host(platform);
        fixtures.push(take(&mut host).unwrap());
        assert!(take(&mut host).is_none());
        for action in [
            json!({"type":"set_theme","theme":"dark"}),
            json!({"type":"set_brush_size","value":37.3}),
            json!({"type":"set_color","rgba":[0.1,0.2,0.7,0.43]}),
            json!({"type":"invoke","command":"zoom_in"}),
            json!({"type":"invoke","command":"fit_canvas"}),
            json!({"type":"open_settings","page":"canvas"}),
            json!({"type":"open_settings","page":"appearance"}),
            json!({"type":"open_settings","page":"input"}),
            json!({"type":"open_settings","page":"shortcuts"}),
            json!({"type":"open_settings","page":"about"}),
            json!({"type":"close_settings"}),
            json!({"type":"move_panel","panel":"toolbar","target":{"kind":"tab","group":6},"viewport":[1205,905]}),
            json!({"type":"customize","action":{"type":"set_column_collapsed","group":6,"collapsed":true}}),
            json!({"type":"customize","action":{"type":"toggle_column_drawer","group":6,"panel":"toolbar"}}),
            json!({"type":"invoke","command":"undo_workspace"}),
            json!({"type":"invoke","command":"redo_workspace"}),
            json!({"type":"invoke","command":"zen_mode"}),
        ] {
            host.dispatch(serde_json::from_value(action).unwrap())
                .unwrap();
            fixtures.push(take(&mut host).unwrap_or_else(|| b"null".to_vec()));
            assert!(take(&mut host).is_none());
        }
        host.error = Some("Synthetic surface error".into());
        fixtures.push(take(&mut host).unwrap());
        host.error = None;
        host.resize(1810, 2410, 2.).unwrap();
        fixtures.push(take(&mut host).unwrap());
    }
    // Preserve the wire bytes: decoding/re-encoding here could itself change
    // floating-point decimal representations before the independent comparison.
    let mut output = std::io::stdout().lock();
    output.write_all(b"[").unwrap();
    for (index, fixture) in fixtures.iter().enumerate() {
        if index != 0 {
            output.write_all(b",").unwrap();
        }
        output.write_all(fixture).unwrap();
    }
    output.write_all(b"]\n").unwrap();
}

fn benchmark() {
    let mut results = Vec::new();
    for platform in [Platform::Ios, Platform::Mac] {
        let mut host = host(platform);
        for round in 0..4 {
            // Alternate ordering to reduce fixed warm-cache bias. This measures
            // transport CPU only, not GPU work, Swift decoding or presentation.
            for stream in if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let mut samples = Vec::new();
                for _ in 0..200 {
                    host.invalidate_snapshot();
                    let started = std::time::Instant::now();
                    let bytes = if stream {
                        host.take_snapshot_bytes().unwrap().unwrap()
                    } else {
                        serde_json::to_vec(&host.take_snapshot().unwrap()).unwrap()
                    };
                    std::hint::black_box(bytes);
                    samples.push(started.elapsed().as_nanos() as u64);
                }
                results.push(json!({"platform":platform,"stream":stream,"round":round,"nanoseconds":samples}));
            }
        }
    }
    println!("{}", json!(results));
}
