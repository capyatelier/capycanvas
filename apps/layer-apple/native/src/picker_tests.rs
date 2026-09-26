use super::*;
use super::fixtures::until;

fn picker_app(platform: u32) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    app
}

fn pointer(app: &App, id: u64, tool: u32, phase: f64, [x, y]: [f64; 2], time: f64) {
    let records = [x, y, 0.6, 0., 0., 0., 0., time, phase];
    let camera = unsafe { capy_apple_camera_revision(app.0) };
    assert_eq!(unsafe { capy_apple_pointer(app.0, id, tool, 0, records.as_ptr(), records.len(), 0, camera) }, 0);
}

fn picking(app: &App) -> bool {
    matches!(app.state()["layer_tools"]["tool"].as_str(), Some("pick_visible" | "pick_layer"))
}

#[test]
fn apple_picker_previews_on_hover_and_contact_and_accepts_by_device() {
    for platform in [0, 1] {
        let app = picker_app(platform);
        let colors = app.state()["colors"].clone();
        let center = [600., 450.];
        app.invoke("eyedropper");
        assert!(picking(&app));
        pointer(&app, 0, 1, 0., center, 3_000_000_000.);
        until(&app, "mouse hover previews", || !app.state()["color_picker"]["preview"].is_null());
        assert!(unsafe { &*app.0 }.host.session.color_picker_overlay().is_some());
        assert_eq!(app.state()["colors"], colors, "Hover never commits");
        app.request(1, json!({"type":"cursor_leave"}));
        assert!(app.state()["color_picker"]["preview"].is_null());
        assert!(picking(&app), "Leaving the canvas keeps the picker");
        pointer(&app, 0, 1, 4., center, 3_010_000_000.);
        assert!(!picking(&app), "A pointer cancel ends picking, so hosts send cursor_leave on exit");
        assert_eq!(app.state()["colors"], colors);

        app.invoke("eyedropper");
        pointer(&app, 1, 1, 1., center, 3_020_000_000.);
        pointer(&app, 1, 1, 3., center, 3_030_000_000.);
        until(&app, "mouse press accepts", || !picking(&app));
        assert_ne!(app.state()["colors"], colors, "The white paper sample replaces black");

        app.action(json!({"type":"set_color","rgba":[0.,0.,0.,1.]}));
        let colors = app.state()["colors"].clone();
        app.invoke("eyedropper");
        pointer(&app, 2, 0, 1., center, 3_040_000_000.);
        until(&app, "pen contact previews", || !app.state()["color_picker"]["preview"].is_null());
        assert!(picking(&app));
        assert_eq!(app.state()["colors"], colors, "Pen contact alone never commits");
        pointer(&app, 2, 0, 3., center, 3_050_000_000.);
        until(&app, "pen lift accepts", || !picking(&app));
        assert_ne!(app.state()["colors"], colors);
    }
}
