use super::*;
use layer_core::{Document, color::{ColorProfile, IntegerDepth, RgbSpace, source::*}};

fn document(app: &App) -> Document { unsafe { &*app.0 }.host.session.engine().document().clone() }
fn adopt(app: &App, task: &ProjectJob) -> i32 {
    unsafe { capy_apple_project_adopt(app.0, task.0, c"Source".as_ptr(), c"".as_ptr()) }
}
fn initialized(platform: u32) -> App {
    let app=App::new(platform);
    unsafe{&mut *app.0}.host.session.renderer_mut().0=Some(native_renderer());
    app.draw_until_idle();
    let task=ProjectJob::new(&app,true);assert_eq!(task.create([128,96]),0);assert_eq!(adopt(&app,&task),0);
    let mut source=SourceBuilder::new([151,103],SourceInterpretation {channels:SourceChannels::Rgba,depth:IntegerDepth::U16,
        profile:ColorProfile::Builtin(RgbSpace::ProPhoto),profile_assumed:true},1024*1024).unwrap();
    let row:Vec<u8>=[23141u16,31788,9234,65535].into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>().repeat(151);
    for _ in 0..103 {source.push_row(&row).unwrap();}
    unsafe{&mut *app.0}.host.session.import_layer_source("Original 16-bit photo",source.finish().unwrap()).unwrap();
    app.draw_until_idle();app
}
fn job(app: &App, command:&str) -> ProjectJob {
    app.invoke(command);let task=unsafe{capy_apple_project_task(app.0,6)};assert!(!task.is_null());ProjectJob(task)
}
fn work(task:&ProjectJob,choice:Value) -> i32 {
    let value=CString::new(choice.to_string()).unwrap();unsafe{capy_project_edit_work(task.0,value.as_ptr(),false)}
}
fn prepare(app:&App,task:&ProjectJob,choice:Value) {
    assert_eq!(work(task,choice),0,"{:?}",task.error());
    assert_eq!(unsafe{capy_apple_project_candidate(app.0,task.0)},0);
    assert_eq!(unsafe{capy_project_compare(task.0)},0,"{:?}",task.error());
}
fn details(task:&ProjectJob) -> Value {
    let text=unsafe{capy_project_details(task.0)};assert!(!text.is_null());
    let value=serde_json::from_slice(unsafe{CStr::from_ptr(text)}.to_bytes()).unwrap();unsafe{capy_apple_string_free(text)};value
}
fn preview(task:&ProjectJob,after:bool) -> Vec<u8> {
    let mut output=CapyProjectPreview {width:0,height:0,pixels:std::ptr::null(),count:0};
    assert_eq!(unsafe{capy_project_preview(task.0,after,&mut output)},0);
    unsafe{std::slice::from_raw_parts(output.pixels,output.count)}.to_vec()
}
fn check(app:&App,mut expected:Document) {
    let actual=document(app);expected.revision=actual.revision;assert_project_document(&actual,&expected);
}
fn history(app:&App,mut before:Document,before_pixels:Vec<u8>) {
    let after=document(app);let after_pixels=app.pixels();
    // Layer IDs are monotonic across ordinary insert Undo; artwork and history
    // restore exactly without reusing an already allocated identity.
    while before.next_layer_id() < after.next_layer_id() { before.allocate_layer_id(); }
    app.invoke("undo");app.draw_until_idle();check(app,before);assert_eq!(app.pixels(),before_pixels);
    app.invoke("redo");app.draw_until_idle();check(app,after);assert_eq!(app.pixels(),after_pixels);
}
fn complete(app:&App) {
    let id=app.state()["requests"].as_array().unwrap().iter().find(|r|r["kind"]["type"]=="document").unwrap()["id"].as_u64().unwrap();
    assert_eq!(unsafe{capy_apple_document_complete(app.0,id as u32,0)},0);
}

#[test]
fn source_repair_and_rasterization_keep_exact_originals_paint_masks_extent_and_history() {
    for platform in [0,1] {
        let app=initialized(platform);let id=document(&app).active_layer;
        app.layer_action(json!({"op":"add_mask","id":id.0,"replace":false}));
        app.layer_action(json!({"op":"select","id":id.0,"mask":false}));app.draw_until_idle();
        let before=document(&app);let pixels=app.pixels();let original=before.layer(id).unwrap().source.as_ref().unwrap();
        let task=job(&app,"repair_source_profile");prepare(&app,&task,json!({"Builtin":"DisplayP3"}));
        check(&app,before.clone());assert_eq!(app.pixels(),pixels);assert_eq!(details(&task)["adds_layer"],false);
        assert!(preview(&task,false)!=preview(&task,true),"Repair must change interpreted appearance");
        assert_eq!(adopt(&app,&task),0);app.draw_until_idle();
        let repaired=document(&app);let source=repaired.layer(id).unwrap().source.as_ref().unwrap();
        assert_eq!(source.extent,original.extent);assert_eq!(source.interpretation.depth,IntegerDepth::U16);
        for (a,b) in source.tiles.values().zip(original.tiles.values()) {assert!(std::sync::Arc::ptr_eq(a,b));}
        assert_eq!(repaired.layer(id).unwrap().mask,before.layer(id).unwrap().mask);
        history(&app,before,pixels);

        document_color::stroke_inside(&app);app.draw_until_idle();
        let painted=document(&app);let painted_pixels=app.pixels();
        assert!(!painted.layer(id).unwrap().raster.is_empty(),"Fixture must contain committed paint");
        let icc=layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::ProPhoto)).unwrap();
        let task=job(&app,"repair_source_profile");prepare(&app,&task,json!({"Icc":icc}));
        assert_eq!(details(&task)["adds_layer"],true);assert_eq!(adopt(&app,&task),0);app.draw_until_idle();
        let added=document(&app);assert_eq!(added.layers.len(),painted.layers.len()+1);
        assert_eq!(added.layer(id),painted.layer(id));assert_ne!(added.active_layer,id);
        assert_eq!(added.layer(added.active_layer).unwrap().source.as_ref().unwrap().interpretation.profile,ColorProfile::Icc(icc.into()));
        history(&app,painted,painted_pixels);

        app.layer_action(json!({"op":"select","id":id.0,"mask":false}));app.draw_until_idle();
        let before=document(&app);let pixels=app.pixels();
        let task=job(&app,"rasterize_source");prepare(&app,&task,Value::Null);assert_eq!(adopt(&app,&task),0);app.draw_until_idle();
        let after=document(&app);let layer=after.layer(id).unwrap();let converted=layer.source.as_ref().unwrap();
        assert_eq!(converted.kind,SourceKind::Rasterized);assert_eq!(converted.extent,[151,103]);
        assert_eq!(converted.interpretation.depth,after.color.depth);
        assert_eq!(converted.interpretation.profile,ColorProfile::Builtin(after.color.space));
        assert_eq!(raster_samples(&layer.raster),raster_samples(&before.layer(id).unwrap().raster));
        assert_eq!(layer.mask,before.layer(id).unwrap().mask);assert_eq!(layer.properties,before.layer(id).unwrap().properties);
        history(&app,before,pixels);
        let project=unsafe{&*app.0}.host.session.capture_project_recovery().unwrap();let mut bytes=Vec::new();project.write(&mut bytes).unwrap();
        let reopened=layer_core::Project::read(bytes.as_slice(),Default::default()).unwrap();assert_project_document(&reopened.document,&document(&app));
        document_color::stroke_inside(&app);app.draw_until_idle();let pixels=app.pixels();
        app.invoke("undo");app.draw_until_idle();app.invoke("redo");app.draw_until_idle();assert_eq!(app.pixels(),pixels);
    }
}

#[test]
fn source_edit_invalid_cancel_and_stale_results_never_publish_partial_work() {
    for platform in [0,1] {
        let app=initialized(platform);
        for condition in ["invalid","cancel_work","cancel_apply","revision","device"] {
            let task=job(&app,"repair_source_profile");
            if condition=="invalid" {assert_eq!(work(&task,json!({"Icc":[]})),-1);}
            else if condition=="cancel_work" {unsafe{capy_project_cancel(task.0)};assert_eq!(work(&task,json!({"Builtin":"Srgb"})),-1);}
            else {prepare(&app,&task,json!({"Builtin":"Srgb"}));}
            if condition=="cancel_apply" {unsafe{capy_project_cancel(task.0)}}
            if condition=="revision" {app.action(json!({"type":"set_layer_opacity","opacity":0.5}));app.draw_until_idle();}
            let retired=if condition=="device" {unsafe{&mut *app.0}.host.session.renderer_mut().0.take()} else {None};
            let before=document(&app);assert_eq!(adopt(&app,&task),-1,"{condition}");check(&app,before);
            if let Some(renderer)=retired {unsafe{&mut *app.0}.host.session.renderer_mut().0=Some(renderer);}
            complete(&app);
        }
    }
}

#[test]
fn icc_inspection_keeps_exact_bytes_supports_summaries_and_rejects_invalid_input() {
    let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
    for summary in [false, true] {
        for input in [bytes.as_slice(), b"Invalid profile"] {
            let text = unsafe { capy_color_profile_inspect(input.as_ptr(), input.len(), summary) };
            assert!(!text.is_null());
            let value: Value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
            unsafe { capy_apple_string_free(text) };
            if input == bytes.as_slice() {
                assert_eq!(value["channels"], "Rgb");
                if summary { assert!(value["profile"].is_null()); }
                else { assert_eq!(value["profile"]["Icc"], json!(bytes)); }
            } else { assert!(value["error"].is_string()); }
        }
    }
}
