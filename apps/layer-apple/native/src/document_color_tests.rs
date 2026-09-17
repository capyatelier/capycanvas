use super::*;
use std::{io::{Seek, Read}, os::fd::AsRawFd};

fn initialized(platform: u32) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    let new = ProjectJob::new(&app, true);
    let options = c"{\"extent\":[128,96],\"color\":{\"space\":\"DisplayP3\",\"depth\":\"U16\"},\"background\":\"Transparent\"}";
    assert_eq!(unsafe { capy_project_new(new.0, options.as_ptr()) }, 0);
    adopt(&app, &new);
    app.place_rgba("Retained photograph", 7, 5, &[123, 231, 45, 213].repeat(35));
    // Placement leaves Move selected. Add document-space ink explicitly;
    // moving the retained photo is not a painted color-conversion fixture.
    app.invoke("add_layer");
    app.invoke("pen");
    app.action(json!({"type":"color","action":{"op":"set_slot","slot":"foreground","color":{"space":"DisplayP3","rgba":[0.9,0.23,0.1,0.73]}}}));
    let blank=app.pixels();
    stroke_inside(&app); app.draw_until_idle();
    assert!(app.pixels()!=blank,"Fixture must paint before color preparation");
    app
}
pub(super) fn stroke_inside(app: &App) {
    // Color selection can invalidate a deferred brush pipeline. Like a live
    // owner, prepare it before starting the next admitted contact.
    app.draw_until_idle();
    let camera = &unsafe { &*app.0 }.host.session.state().camera;
    let [a,b,c,d,e,f] = camera.document_to_surface();
    let mut records=Vec::new();
    for (i,[x,y]) in [[30.0,30.0],[80.0,60.0],[100.0,70.0]].into_iter().enumerate() {
        records.extend([f64::from(a*x+c*y+e),f64::from(b*x+d*y+f),1.,0.,0.,0.,0.,1_000_000_000.+i as f64*10_000_000.,i as f64+1.]);
    }
    let revision=unsafe{capy_apple_camera_revision(app.0)};
    assert_eq!(unsafe{capy_apple_pointer(app.0,1,1,0,records.as_ptr(),records.len(),0,revision)},0);
}

fn document(app: &App) -> layer_core::Document { unsafe { &*app.0 }.host.session.engine().document().clone() }
fn check_document(app: &App, mut expected: layer_core::Document) {
    let actual = document(app); expected.revision = actual.revision;
    assert_project_document(&actual, &expected);
}
fn job(app: &App, command: &str) -> ProjectJob {
    app.invoke(command);
    let pointer = unsafe { capy_apple_project_task(app.0, 4, std::ptr::null()) };
    assert!(!pointer.is_null(), "{command}"); ProjectJob(pointer)
}
fn work(job: &ProjectJob, choice: Value, copy: bool) {
    let json = CString::new(choice.to_string()).unwrap();
    assert_eq!(unsafe { capy_project_edit_work(job.0, json.as_ptr(), copy) }, 0, "{:?}", job.error());
}
fn adopt(app: &App, job: &ProjectJob) {
    assert_eq!(unsafe { capy_apple_project_adopt(app.0, job.0, c"Color".as_ptr(), c"".as_ptr()) }, 0,
        "{:?}", unsafe { CStr::from_ptr(capy_apple_error(app.0)) });
    app.draw_until_idle();
}
fn complete(app: &App) {
    let id = app.state()["requests"].as_array().unwrap().iter().find(|r| r["kind"]["type"] == "document").unwrap()["id"].as_u64().unwrap();
    assert_eq!(unsafe { capy_apple_document_complete(app.0, id as u32, 0) }, 0);
}
fn preview(job: &ProjectJob, after: bool) -> Vec<u8> {
    let mut output = CapyProjectPreview { width:0, height:0, pixels:std::ptr::null(), count:0 };
    assert_eq!(unsafe { capy_project_preview(job.0, after, &mut output) }, 0);
    assert!(output.width > 0 && output.width <= 512 && output.height > 0 && output.height <= 384);
    assert_eq!(output.count, (output.width * output.height * 4) as usize);
    unsafe { std::slice::from_raw_parts(output.pixels, output.count) }.to_vec()
}

#[test]
fn apple_color_changes_preview_atomically_and_preserve_exact_history_sources_and_save() {
    for platform in [0,1] {
        let app = initialized(platform);
        let original_source = document(&app).layers.iter().find_map(|l| l.source.clone()).unwrap();
        let mut history = vec![(document(&app), app.pixels())];
        for (command, choice) in [
            ("assign_profile", json!({"Assign":"ProPhoto"})),
            ("convert_color_space", json!({"Convert":{"space":"AdobeRgb","options":{"intent":"RelativeColorimetric","black_point_compensation":false}}})),
            ("change_bit_depth", json!({"Depth":{"depth":"U8","dither":"Stochastic8"}})),
        ] {
            let before = document(&app); let pixels = app.pixels();
            let task = job(&app, command); work(&task, choice, false);
            assert_project_document(&document(&app), &before); assert_eq!(app.pixels(), pixels);
            let a = preview(&task, false); let b = preview(&task, true);
            if command == "assign_profile" { assert!(a != b,"Assignment should change appearance without changing numbers"); }
            adopt(&app, &task);
            assert_eq!(document(&app).layers.iter().find_map(|l| l.source.clone()).unwrap(), original_source);
            if command == "assign_profile" {
                for (a,b) in document(&app).layers.iter().zip(&before.layers) { assert_eq!(raster_samples(&a.raster),raster_samples(&b.raster)); }
            }
            history.push((document(&app), app.pixels()));
        }
        for redo in [false,true] {
            let indices: Vec<usize> = if redo { (1..history.len()).collect() } else { (0..history.len()-1).rev().collect() };
            for i in indices {
                let task = job(&app, if redo {"redo"} else {"undo"}); work(&task, Value::Null, false); adopt(&app,&task);
                check_document(&app, history[i].0.clone()); assert_eq!(app.pixels(), history[i].1);
            }
        }
        let captured = unsafe { &*app.0 }.host.session.capture_project_recovery().unwrap();
        let mut bytes = Vec::new(); captured.write(&mut bytes).unwrap();
        let reopened = layer_core::Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_project_document(&reopened.document, &document(&app));
        stroke_inside(&app);app.draw_until_idle(); let painted=app.pixels();
        app.invoke("undo");app.draw_until_idle();assert_eq!(app.pixels(),history.last().unwrap().1);
        app.invoke("redo");app.draw_until_idle();assert_eq!(app.pixels(),painted);
    }
}

#[test]
fn apple_color_cancel_stale_results_flattened_copy_and_properties_preserve_original() {
    for platform in [0,1] {
        let app=initialized(platform);
        let choice=json!({"Convert":{"space":"Srgb","options":{"intent":"RelativeColorimetric","black_point_compensation":false}}});
        let before=document(&app);let pixels=app.pixels();
        let copy=job(&app,"convert_color_space");work(&copy,choice.clone(),true);
        assert_eq!(preview(&copy,false).len(),preview(&copy,true).len());
        let path=std::env::temp_dir().join(format!("capy-color-copy-{}-{platform}.capy",std::process::id()));
        let mut file=std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        assert_eq!(unsafe {capy_project_write(copy.0,file.as_raw_fd())},0,"{:?}",copy.error());
        file.rewind().unwrap();let mut bytes=Vec::new();file.read_to_end(&mut bytes).unwrap();std::fs::remove_file(path).unwrap();
        let saved=layer_core::Project::read(bytes.as_slice(),Default::default()).unwrap();
        assert_eq!(saved.document.color.space,layer_core::color::RgbSpace::Srgb);
        assert_eq!(saved.document.color.depth,before.color.depth);
        assert_eq!(unsafe{capy_apple_project_adopt(app.0,copy.0,c"Copy".as_ptr(),c"".as_ptr())},-1);
        assert_project_document(&document(&app),&before);assert_eq!(app.pixels(),pixels);complete(&app);
        for failure in ["cancel","edit","device"] {
            let task=job(&app,"convert_color_space");work(&task,choice.clone(),false);
            if failure=="cancel" {unsafe{capy_project_cancel(task.0)}}
            if failure=="edit" {app.action(json!({"type":"set_layer_opacity","opacity":0.5}));app.draw_until_idle();}
            let retired=if failure=="device" {unsafe { &mut *app.0 }.host.session.renderer_mut().0.take()} else {None};
            let expected=document(&app);
            assert_eq!(unsafe{capy_apple_project_adopt(app.0,task.0,c"Change".as_ptr(),c"".as_ptr())},-1,"{failure}");
            assert_project_document(&document(&app),&expected);
            if let Some(renderer)=retired {unsafe{&mut *app.0}.host.session.renderer_mut().0=Some(renderer);}
            complete(&app);
        }
        app.invoke("document_properties");let info=ProjectJob(unsafe{capy_apple_project_task(app.0, 5, std::ptr::null())});assert!(!info.0.is_null());
        let details=unsafe{capy_project_details(info.0)};assert!(!details.is_null());
        let value=unsafe{CStr::from_ptr(details)}.to_string_lossy().into_owned();unsafe{capy_apple_string_free(details)};
        assert!(value.contains("Display P3")&&value.contains("16-bit integer SDR")&&value.contains("Retained photograph")&&value.contains("Original samples"),"{value}");
        complete(&app);
    }
}
