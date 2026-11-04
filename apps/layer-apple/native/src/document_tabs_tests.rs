use super::*;
use layer_core::color::{RgbSpace, SampleDepth};
fn new_drawing(app: &App, depth: SampleDepth) {
    let job = ProjectJob::create_with(app, [96, 64], &[json!({"op": "space", "space": RgbSpace::DisplayP3}), json!({"op": "depth", "depth": depth})]);
    assert_eq!(
        unsafe { capy_apple_project_adopt(app.0, job.0, c"Untitled".as_ptr(), c"".as_ptr()) },
        0,
        "{:?}",
        app.borrow().error
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
    assert!(!(task==0), "{:?}", app.borrow().error);
    if closing { assert_eq!(unsafe { capy_apple_document_close_commit(app.0, task, true) }, 0); }
    assert_eq!(
        unsafe { capy_document_prepare(task) },
        0
    );
    assert_eq!(
        unsafe { capy_apple_document_resume(app.0, task) },
        0,
        "{:?}",
        app.borrow().error
    );
    unsafe { capy_document_free(task) };
    app.draw_until_idle();
}
#[test]
fn apple_tab_close_preparation_can_cancel_without_retiring_source() {
    for platform in [0, 1] {
        let app = App::new(platform);
        app.borrow().host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        new_drawing(&app, SampleDepth::U8);
        let source = app.document().clone();
        assert_eq!(unsafe { capy_apple_document_close(app.0, 0, 0) }, 0);
        super::fixtures::until(&app, "Close preparation", || unsafe { capy_apple_document_prepare_switch(app.0, 2_000_000_000) } == 0);
        let task = unsafe { capy_apple_document_switch(app.0, 0, true) };
        assert!(!(task==0), "{:?}", app.borrow().error);
        assert_eq!(app.borrow().window.documents.selected(), 2);
        assert_eq!(app.borrow().window.documents.order().len(), 2);
        assert_project_document(&app.document(), &source);
        assert_eq!(unsafe { capy_apple_document_close_commit(app.0, task, false) }, 0);
        unsafe { capy_document_free(task) };
        app.draw_until_idle();
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert!(!app.state()["document_file"]["close_ready"].as_bool().unwrap());
        assert_project_document(&app.document(), &source);
    }
}
#[test]
fn apple_new_drawing_takes_the_upcoming_untitled_name_or_the_name_it_was_given() {
    for platform in [0, 1] {
        let app = App::new(platform);
        app.borrow().host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let title = |view: &Value| view["tabs"].as_array().unwrap().iter().find(|tab| tab["id"] == view["selected"]).unwrap()["title"].clone();
        let upcoming = tabs(&app)["upcoming_untitled"].clone();
        assert!(upcoming.as_str().is_some_and(|name| !name.is_empty()));
        new_drawing(&app, SampleDepth::U8);
        assert_eq!(title(&tabs(&app)), upcoming);
        assert_ne!(tabs(&app)["upcoming_untitled"], upcoming);
        let job = ProjectJob::create_with(&app, [96, 64], &[json!({"op": "name", "text": "  Harbor study "})]);
        assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"Untitled".as_ptr(), c"".as_ptr()) }, 0, "{:?}", app.borrow().error);
        app.draw_until_idle();
        assert_eq!(title(&tabs(&app)), "Harbor study");
        assert_eq!(app.state()["document_file"]["unsaved_name"], "Harbor study");
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 3);
    }
}
#[test]
fn apple_tabs_preserve_history_pixels_and_disk_parking() {
    for platform in [0, 1] {
        let app = App::unbound(platform);
        app.borrow().host.session.renderer_mut().0 = Some(native_renderer());
        app.draw_until_idle();
        let root=std::env::temp_dir().join(format!("capy-apple-tabs-{}",layer_core::PortableId::random()));
        let path=CString::new(root.to_str().unwrap()).unwrap();
        let session=unsafe {capy_apple_session_open(app.0,path.as_ptr(),c"tabs".as_ptr(),false,false)};
        assert_ne!(session,0);
        assert_eq!(unsafe {capy_session_work(session,None,std::ptr::null_mut())},0);
        assert_eq!(unsafe {capy_apple_session_adopt(app.0,session)},0,"{:?}",app.borrow().error);
        assert_eq!(unsafe {capy_session_restore_finished(session)},0);
        unsafe {capy_session_free(session)};
        app.stroke();
        app.draw_until_idle();
        let first = app.document().clone();
        new_drawing(&app, SampleDepth::F32);
        assert_eq!(tabs(&app)["tabs"].as_array().unwrap().len(), 2);
        assert_eq!(tabs(&app)["selected"], 2);
        assert_eq!(tabs(&app)["parked_renderers"], 0);
        app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[1.8,1.2,0.4,1.]}}}));
        app.invoke("select_all");
        app.layer_action(json!({"op":"fill_selection"}));
        app.invoke("deselect");
        app.draw_until_idle();
        let second = app.document().clone();
        app.borrow().window.documents.set_inactive_ram_budget(0);
        switch(&app, 1, false);
        assert_project_document(&app.document(), &first);
        assert_eq!(app.document().working, first.working);
        let storage=unsafe {capy_apple_document_trim_wait(app.0)};
        if storage!=0 {
            assert_eq!(unsafe {capy_document_prepare(storage)},0);
            assert_eq!(unsafe {capy_apple_document_resume(app.0,storage)},0,"{:?}",app.borrow().error);
            unsafe {capy_document_free(storage)};
        }
        assert_eq!(tabs(&app)["resident_bytes"], 0);
        app.invoke("undo");
        app.draw_until_idle();
        assert_ne!(
            raster_samples(
                app.document().target_raster(first.working.target.unwrap()).unwrap()
            ),
            raster_samples(active_raster(&first))
        );
        app.invoke("redo");
        app.draw_until_idle();
        assert_project_document(&app.document(), &first);
        assert_eq!(app.document().working, first.working);
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let capture = app.with(|native|native.window.session(&native.host, 2).unwrap().capture_session().unwrap());
        let mut store=pollster::block_on(layer_core::package::session_store::SessionStore::open(&root.join("captured-second"))).unwrap();
        pollster::block_on(capture.checkpoint(&mut store,&cancel)).unwrap();
        let restored=pollster::block_on(store.load(Default::default(),&cancel)).unwrap().unwrap();
        assert_project_document(restored.editor.document(), &second);
        drop((restored,store));
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
        assert!((unsafe { capy_apple_document_switch(app.0, 99, false) }==0));
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
        assert_project_document(&app.document(), &second);
        assert_eq!(app.document().working, second.working);
        let storage=unsafe {capy_apple_document_trim_wait(app.0)};
        if storage!=0 {assert_eq!(unsafe {capy_document_prepare(storage)},0);unsafe {capy_document_free(storage)};}
        drop(app);std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn apple_session_close_keeps_unsaved_drawings_without_document_prompts() {
    for platform in [0, 1] {
        let app = App::new(platform);
        app.invoke("add_layer");
        let before = tabs(&app);
        assert_eq!(app.request(2, json!({"type":"document_tabs","op":"session_close"})).unwrap(), json!(true));
        assert!(app.state()["document_file"]["close_ready"].as_bool().unwrap());
        assert!(app.state()["requests"].as_array().unwrap().iter().all(|r| r["kind"]["request"]["type"] != "confirm_close"));
        assert_eq!(tabs(&app)["tabs"], before["tabs"]);
    }
}
