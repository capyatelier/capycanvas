//! Side-button transitions through Wayland, GDK and the real canvas gestures.
use super::*;

#[test]
#[ignore = "isolated native-input.js --native-test=native_canvas_pen_buttons --tablet"]
fn native_canvas_pen_buttons() {
    let mut d = Driver::new("art.capycanvas.CanvasPenButtons");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let ready = d.w.gpu.borrow().as_ref().is_some_and(|g| {
            let engine = g.session.engine();
            engine
                .backend()
                .paint_ready(engine.document(), engine.brush(), false)
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "active brush startup");
        pump(20);
    }
    let area = d.w.resolved().work_area;
    let p = [area.x + area.width * 0.4, area.y + area.height * 0.5];
    let q = [p[0] + 50., p[1] + 20.];
    let r = [p[0] + 100., p[1]];
    let camera = state(&d.w).camera;
    let stats =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .backend()
            .stats
            .clone();
    let initial =
        d.w.gpu
            .borrow()
            .as_ref()
            .unwrap()
            .session
            .engine()
            .metrics()
            .committed_strokes;
    let raster = |w: &Workspace| {
        let gpu = w.gpu.borrow();
        let document = gpu.as_ref().unwrap().session.engine().document();
        document
            .layers
            .iter()
            .find(|l| l.id == document.active_layer)
            .unwrap()
            .raster
            .clone()
    };
    let paper = raster(&d.w);
    let mut expected = initial;
    // BTN_STYLUS, BTN_STYLUS2 and BTN_STYLUS3 map to middle, right and back.
    for button in [331, 332, 329] {
        for held_before in [false, true] {
            stats.lock().unwrap().pen_routes.clear();
            if held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"move","point":p},
                    {"pen":"button","button":button,"down":true}
                ]));
            }
            d.input.perform(serde_json::json!([
                {"pen":"down","point":p}, {"pen":"move","point":q}
            ]));
            if !held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"button","button":button,"down":true},
                    {"pen":"move","point":r},
                    {"pen":"button","button":button,"down":false}
                ]));
            }
            assert_eq!(
                d.w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .metrics()
                    .committed_strokes,
                expected,
                "side buttons cannot finish the stroke"
            );
            d.input
                .perform(serde_json::json!([{"pen":"move","point":r}, {"pen":"up"}]));
            if held_before {
                d.input.perform(serde_json::json!([
                    {"pen":"move","point":p},
                    {"pen":"button","button":button,"down":false}
                ]));
            }
            expected += 1;
            assert_eq!(
                d.w.gpu
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .session
                    .engine()
                    .metrics()
                    .committed_strokes,
                expected
            );
            assert_eq!(state(&d.w).camera, camera, "pen buttons must not pan");
            let routes = stats.lock().unwrap().pen_routes.clone();
            let phases: Vec<_> = routes
                .iter()
                .filter(|r| r.2 == "send")
                .map(|r| r.1.as_str())
                .collect();
            assert_eq!(phases.first(), Some(&"Down"));
            assert_eq!(phases.last(), Some(&"Up"));
            assert!(
                phases[1..phases.len() - 1].iter().all(|p| *p == "Move"),
                "{phases:?}"
            );
            let painted = raster(&d.w);
            assert_ne!(painted, paper);
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            pump(180);
            assert_eq!(raster(&d.w), paper, "one Undo removes the whole stroke");
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Redo,
            });
            pump(180);
            assert_eq!(raster(&d.w), painted);
            d.w.dispatch(UiAction::Invoke {
                command: CommandId::Undo,
            });
            pump(180);
        }
    }
    d.input.perform(serde_json::json!([{"pen":"leave"}]));
    // The actual mouse still owns middle/right navigation.
    for button in [274, 273] {
        let before = state(&d.w).camera;
        d.input.perform(serde_json::json!([
            {"point":p}, {"down":true,"button":button},
            {"point":q}, {"down":false,"button":button}
        ]));
        assert_ne!(state(&d.w).camera.translation, before.translation);
    }
    d.finish();
}
