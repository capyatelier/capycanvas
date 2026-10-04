use super::*;

const SWAP: &[u8] = b"LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";

fn import(app: &App, bytes: &[u8]) -> (ProjectJob, i32) {
    let view = app.state()["layer_properties"].clone();
    app.action(json!({"type":"effect","action":{"op":"import_lookup","layer":view["layer"],"epoch":view["epoch"]}}));
    let task = ProjectJob(unsafe { capy_apple_project_task(app.0, 7, std::ptr::null()) });
    assert!(!task.0.is_null(), "{:?}", unsafe { CStr::from_ptr(capy_apple_error(app.0)) });
    let read = unsafe { capy_project_read_bytes(task.0, bytes.as_ptr(), bytes.len(), c"Swap.cube".as_ptr()) };
    (task, read)
}

#[test]
fn apple_lookup_import_parses_on_the_worker_and_applies_one_undo_step() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        app.action(json!({"type":"effect","action":{"op":"insert","effect":"color_lookup"}}));
        app.draw_until_idle();
        let original = app.state()["layer_properties"]["resource_name"].clone();
        let (malformed, read) = import(&app, b"LUT_3D_SIZE 2\n0 0 0\n");
        assert_eq!(read, -1, "A malformed table fails on the worker");
        let request = app.state()["requests"][0]["id"].as_u64().unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_complete(app.0, request, 0) }, 0);
        drop(malformed);
        assert!(app.state()["requests"].as_array().unwrap().is_empty());
        assert_eq!(app.state()["layer_properties"]["resource_name"], original);

        let (task, read) = import(&app, SWAP);
        assert_eq!(read, 0, "{:?}", task.error());
        assert_eq!(unsafe { capy_apple_project_adopt(app.0, task.0, c"Swap.cube".as_ptr(), c"".as_ptr()) }, 0,
            "{:?}", unsafe { CStr::from_ptr(capy_apple_error(app.0)) });
        assert!(app.state()["requests"].as_array().unwrap().is_empty(), "Adoption completes the import request");
        assert_eq!(app.state()["layer_properties"]["resource_name"], "Swap.cube");
        app.invoke("undo");
        assert_eq!(app.state()["layer_properties"]["resource_name"], original);
        app.invoke("redo");
        assert_eq!(app.state()["layer_properties"]["resource_name"], "Swap.cube");
    }
}
