use super::*;

fn send(app: &App, value: Value) -> i32 {
    let text = CString::new(value.to_string()).unwrap();
    unsafe { capy_apple_glass_regions(app.0, text.as_ptr()) }
}

#[test]
fn glass_layout_is_validated_bounded_and_wakes_only_on_change() {
    let app = App::new(1);
    let layout = json!({"boxes":[[10,20,300,400,18,18,18,18]],"connections":[]});
    unsafe { &mut *app.0 }.host.dirty = false;
    assert_eq!(send(&app, layout.clone()), 0);
    assert!(unsafe { &*app.0 }.host.dirty, "A new layout wakes the canvas");
    unsafe { &mut *app.0 }.host.dirty = false;
    assert_eq!(send(&app, layout.clone()), 0);
    assert!(!unsafe { &*app.0 }.host.dirty, "An identical layout does not");
    let invalid = [
        json!({"boxes":[[10,20,-1,400,0,0,0,0]],"connections":[]}),
        json!({"boxes":[[10,20,300,400,18,18,18]],"connections":[]}),
        json!({"boxes":[],"connections":[],"extra":1}),
        json!({"boxes":vec![[0,0,1,1,0,0,0,0]; 257],"connections":[]}),
    ];
    for value in invalid {
        assert_eq!(send(&app, value), -1);
        assert_eq!(unsafe { &*app.0 }.metal.glass_regions(&unsafe { &*app.0 }.host).len(), 1, "Failures keep the previous layout");
    }
    let status = unsafe { &*app.0 }.metal.display_status(&unsafe { &*app.0 }.host);
    assert_eq!(status["glass_regions"], 1);
}

#[test]
fn glass_regions_scale_to_surface_pixels_and_include_connector_feet() {
    let app = App::new(0);
    assert_eq!(unsafe { capy_apple_resize(app.0, 2400, 1800, 2.) }, 0);
    let connection = layer_ui::DrawerConnection {
        bounds: layer_ui::Bounds { x: 100., y: 200., width: 60., height: 12. },
        transform: [1., 0., 0., 1., 6., 0.],
        length: 48., depth: 12., radii: [6., 6.], square_corners: [false; 4],
    };
    assert_eq!(send(&app, json!({"boxes":[[10,20,40,30,100,100,100,100]],"connections":[connection]})), 0);
    let regions = unsafe { &*app.0 }.metal.glass_regions(&unsafe { &*app.0 }.host);
    assert_eq!(regions[0].bounds, [20., 40., 80., 60.], "Logical points follow the display scale");
    assert!(regions[0].radii.iter().all(|r| (r - 30.).abs() < 0.01), "Oversized radii clamp like CSS: {:?}", regions[0].radii);
    assert_eq!(regions[0].shape, layer_render_wgpu::BackdropRegion::SQUIRCLE);
    let feet: Vec<_> = regions[1..].iter().filter(|r| r.radii.iter().any(|v| *v < 0.)).collect();
    assert_eq!(regions.len(), 4, "A connector adds its body and both concave feet");
    assert_eq!(feet.len(), 2);
    assert!(feet.iter().all(|r| r.radii.iter().filter(|v| **v < 0.).count() == 1 && r.radii.iter().any(|v| (*v + 12.).abs() < 0.01)));
}
