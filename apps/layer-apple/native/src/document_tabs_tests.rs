use super::*;
use super::fixtures::tempfile;
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth};
use std::io::{Read, Seek};
fn new_drawing(app: &App, depth: SampleDepth) {
    app.invoke("new_document");
    let job = ProjectJob::new(app, true);
    let options = layer_ui::NewDocumentOptions {
        extent: [96, 64],
        color: DocumentColor {
            space: RgbSpace::DisplayP3,
            depth,
        },
        ..Default::default()
    };
    let text = CString::new(serde_json::to_string(&options).unwrap()).unwrap();
    assert_eq!(
        unsafe { capy_project_new(job.0, text.as_ptr()) },
        0,
        "{:?}",
        job.error()
    );
    assert_eq!(
        unsafe { capy_apple_project_adopt(app.0, job.0, c"Untitled".as_ptr(), c"".as_ptr()) },
        0,
        "{:?}",
        unsafe { &*app.0 }.error
    );
    app.draw_until_idle();
}
fn tabs(app: &App) -> Value {
    app.request(2, json!({"type":"document_tabs","op":"view","width":800}))
        .unwrap()
}
pub(super) fn switch(app: &App, id: u64, closing: bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while unsafe { capy_apple_document_prepare_switch(app.0, 2_000_000_000) } == 1 {
        assert!(
            std::time::Instant::now() < deadline,
            "Retained history capture did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let task = unsafe { capy_apple_document_switch(app.0, id, closing) };
    assert!(!task.is_null(), "{:?}", unsafe { &*app.0 }.error);
    if closing { assert_eq!(unsafe { capy_apple_document_close_commit(app.0, task, true) }, 0); }
    assert_eq!(
        unsafe { capy_document_prepare(task) },
        0
    );
    assert_eq!(
        unsafe { capy_apple_document_resume(app.0, task) },
        0,
        "{:?}",
        unsafe { &*app.0 }.error
    );
    unsafe { capy_document_free(task) };
    app.draw_until_idle();
}
#[test]
fn apple_tab_close_preparation_can_cancel_without_retiring_source() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        new_drawing(&app, SampleDepth::U8);
        let source = unsafe { &*app.0 }.host.session.engine().document().clone();
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        super::fixtures::until(&app, "Close preparation", || unsafe { capy_apple_document_prepare_switch(app.0, 2_000_000_000) } == 0);
        let task = unsafe { capy_apple_document_switch(app.0, 0, true) };
        assert!(!task.is_null(), "{:?}", unsafe { &*app.0 }.error);
        assert_eq!(unsafe { &*app.0 }.window.documents.selected(), 2);
        assert_eq!(unsafe { &*app.0 }.window.documents.order().len(), 2);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &source);
        assert_eq!(unsafe { capy_apple_document_close_commit(app.0, task, false) }, 0);
        unsafe { capy_document_free(task) };
        app.draw_until_idle();
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert!(!app.state()["document_file"]["close_ready"].as_bool().unwrap());
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &source);
    }
}
#[test]
fn apple_tabs_preserve_history_pixels_and_disk_parking() {
    for platform in [0, 1] {
        let app = App::new(platform);
        unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        app.stroke();
        app.draw_until_idle();
        let first = unsafe { &*app.0 }.host.session.engine().document().clone();
        new_drawing(&app, SampleDepth::F32);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["parked_renderers"], 0);
        app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[1.8,1.2,0.4,1.]}}}));
        app.invoke("select_all");
        app.layer_action(json!({"op":"fill_selection"}));
        app.invoke("deselect");
        app.draw_until_idle();
        let second = unsafe { &*app.0 }.host.session.engine().document().clone();
        unsafe { &mut *app.0 }.window.documents.budget.inactive_ram = 0;
        switch(&app, 1, false);
        assert_eq!(tabs(&app)["resident_bytes"], 0);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &first);
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document().working, first.working);
        app.invoke("undo");
        app.draw_until_idle();
        assert_ne!(
            raster_samples(
                unsafe { &*app.0 }.host.session.engine().document().target_raster(first.working.target.unwrap()).unwrap()
            ),
            raster_samples(active_raster(&first))
        );
        app.invoke("redo");
        app.draw_until_idle();
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &first);
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document().working, first.working);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let capture = unsafe { &*app.0 }.window.session(&unsafe { &*app.0 }.host, 2).unwrap().capture_session().unwrap();
        let prepared = capture.prepare(&cancel).unwrap();
        let mut file = tempfile();
        prepared.write(&mut file, &cancel).unwrap();
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        let bytes: std::sync::Arc<[u8]> = bytes.into();
        let backing = layer_core::package::ImmutableBacking::new(std::sync::Arc::new(bytes)).unwrap();
        let restored = layer_core::package::session::open(backing, Default::default(), &cancel).unwrap();
        assert_project_document(restored.editor.document(), &second);
        app.request(
            2,
            json!({"type":"document_tabs","op":"reorder","id":2,"before":1}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 2);
        app.request(
            2,
            json!({"type":"document_tabs","op":"history","redo":false}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 1);
        app.request(
            2,
            json!({"type":"document_tabs","op":"history","redo":true}),
        );
        assert_eq!(tabs(&app)["tabs"][0]["id"], 2);
        assert!(unsafe { capy_apple_document_switch(app.0, 99, false) }.is_null());
        assert_eq!(tabs(&app)["selected"], 1);
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        let request = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "confirm_close")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_close(app.0, request, 3) }, 0);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert!(
            !app.state()["document_file"]["close_ready"]
                .as_bool()
                .unwrap()
        );
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        let request = app.state()["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["kind"]["request"]["type"] == "confirm_close")
            .unwrap()["id"]
            .as_u64()
            .unwrap() as u32;
        assert_eq!(unsafe { capy_apple_document_close(app.0, request, 2) }, 0);
        switch(&app, 0, true);
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(tabs(&app)["can_undo"], false);
        assert_project_document(unsafe { &*app.0 }.host.session.engine().document(), &second);
        assert_eq!(unsafe { &*app.0 }.host.session.engine().document().working, second.working);
    }
}
