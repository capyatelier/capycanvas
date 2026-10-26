use super::*;
use std::io::{Seek as _, Write as _};
use std::os::fd::AsRawFd;

struct PaperJob(u64);
impl PaperJob {
    fn new(app: &App, request: u32, directory: &std::path::Path) -> Self {
        let directory = CString::new(directory.to_str().unwrap()).unwrap();
        let epoch=app.borrow().host.session.state().document_file.epoch;
        let job = Self(unsafe { capy_apple_paper_task(app.0,request,epoch,directory.as_ptr()) });
        assert!(!(job.0==0)); job
    }
    fn import(app: &App, directory: &std::path::Path) -> Self {
        let directory = CString::new(directory.to_str().unwrap()).unwrap();
        let job = Self(unsafe { capy_apple_paper_import(app.0,directory.as_ptr()) });
        assert!(!(job.0==0)); job
    }
    fn work(&self, file: Option<&std::fs::File>) {
        let pointer = self.0;
        let fd = file.map_or(-1, |file| file.as_raw_fd());
        std::thread::spawn(move || unsafe { capy_paper_work(pointer,fd,c"Scan".as_ptr()) }).join().unwrap();
        let error = unsafe { capy_paper_error(self.0) };
        assert!((error==0), "{}", handles::take_text(error).to_string_lossy());
    }
    fn apply(&self, app: &App) { assert_eq!(unsafe { capy_apple_paper_apply(app.0,self.0) },0); }
}
impl Drop for PaperJob { fn drop(&mut self) { unsafe { capy_paper_free(self.0) }; } }
fn layer(app: &App) -> u64 {
    layer_ui::occurrence_token(layer_core::paper::paper_occurrence(app.document().scene(),None).unwrap())
}
fn paper_request(app: &App) -> u32 {
    app.borrow().host.session.state().requests.iter().find_map(|item|
        matches!(item.kind,layer_ui::HostRequestKind::Paper { .. }).then_some(item.id)).unwrap()
}
fn checkpoint(app: &App) -> u64 {
    app.borrow().host.session.engine().checkpoint()
}
fn document(app: &App) -> layer_core::Document { app.document().clone() }
fn choice(app: &App) -> u32 {
    let native=app.borrow();let state=native.host.session.state();
    let control = state.layer_properties.controls.iter().find(|control|control.key=="texture").unwrap();
    let layer_core::EffectValue::Choice(value) = &control.value else { panic!("paper choice") }; *value
}
fn effect(app: &App, action: Value) { app.action(json!({"type":"effect","action":action})); }
fn gray_png() -> std::fs::File {
    use layer_core::color::source::{SourceBuilder,SourceInterpretation,SourceChannels};
    let mut image = SourceBuilder::new([40,30],SourceInterpretation { channels:SourceChannels::Gray,
        depth:layer_core::color::SampleDepth::U8,profile:Default::default(),profile_assumed:false },1 << 24).unwrap();
    for y in 0..30u8 { image.push_row(&(0..40u8).map(|x|x.wrapping_mul(3).wrapping_add(y)).collect::<Vec<_>>()).unwrap(); }
    let mut file = fixtures::tempfile();
    file.write_all(&layer_core::paper::encode_png(&image.finish().unwrap()).unwrap()).unwrap(); file.rewind().unwrap(); file
}

#[test]
fn apple_paper_library_import_reload_rename_delete_and_add_preserve_authored_history() {
    for platform in [0,1] {
        let directory = std::env::temp_dir().join(format!("capy-apple-paper-{}",layer_workspace::new_id()));
        let app = App::new(platform);
        app.action(json!({"type":"select_layer","id":layer(&app)}));
        let initial = document(&app); let initial_checkpoint = checkpoint(&app);
        let load = PaperJob::new(&app,0,&directory); load.work(None); load.apply(&app);
        assert_project_document(&document(&app),&initial); assert_eq!(checkpoint(&app),initial_checkpoint);
        assert!(!app.state()["document_file"]["modified"].as_bool().unwrap());
        assert!(app.state()["layer_properties"]["controls"].as_array().unwrap().iter()
            .find(|control|control["key"]=="texture").unwrap()["paper"]["import"].is_object());
        effect(&app,json!({"op":"import_paper","layer":layer(&app)}));
        let id = paper_request(&app); let job = PaperJob::new(&app,id,&directory); let input = gray_png();
        job.work(Some(&input));
        assert_project_document(&document(&app),&initial); assert_eq!(checkpoint(&app),initial_checkpoint);
        assert!(app.borrow().host.session.state().requests.iter().any(|item|item.id==id));
        job.apply(&app);
        assert!(!app.borrow().host.session.state().requests.iter().any(|item|item.id==id));
        let imported = document(&app); let imported_checkpoint = checkpoint(&app); assert_ne!(imported_checkpoint,initial_checkpoint);
        assert!(app.state()["document_file"]["modified"].as_bool().unwrap());
        let entries = layer_host::papers::list(&directory).unwrap(); assert_eq!(entries.len(),1); let paper_id = entries[0].id.clone();
        let paper_layer=layer(&app);
        let original = app.borrow().host.session.paper_image(paper_layer).unwrap();
        assert_source_samples(&original,&layer_host::papers::load(&directory,&paper_id).unwrap());
        app.invoke("undo"); assert_eq!(checkpoint(&app),initial_checkpoint); assert_project_document(&document(&app),&initial);
        app.invoke("redo"); assert_eq!(checkpoint(&app),imported_checkpoint); assert_project_document(&document(&app),&imported);
        for (operation,action) in [json!({"op":"rename_paper","id":paper_id,"name":"Cotton rag"}),json!({"op":"delete_paper","id":paper_id}),
            json!({"op":"add_paper","layer":layer(&app)})].into_iter().enumerate() {
            effect(&app,action); let id = paper_request(&app); let job = PaperJob::new(&app,id,&directory);job.work(None);job.apply(&app);
            assert_eq!(checkpoint(&app),imported_checkpoint); assert_project_document(&document(&app),&imported);
            assert!(app.state()["document_file"]["modified"].as_bool().unwrap());
            assert!(!app.borrow().host.session.state().requests.iter().any(|item|item.id==id));
            let entries=layer_host::papers::list(&directory).unwrap();
            match operation {
                0 => { assert_eq!(entries.len(),1);assert_eq!(entries[0].id,paper_id);assert_eq!(entries[0].name,"Cotton rag"); }
                1 => assert!(entries.is_empty()),
                2 => { assert_eq!(entries.len(),1);assert_eq!(entries[0].id,paper_id);assert_source_samples(&layer_host::papers::load(&directory,&paper_id).unwrap(),&original); }
                _ => unreachable!(),
            }
        }
        for _ in 0..2 {
            let pending=app.borrow().host.session.state().requests.iter().find_map(|item|
                matches!(item.kind,layer_ui::HostRequestKind::Paper { .. }).then_some(item.id));
            let Some(id)=pending else { break };
            let job=PaperJob::new(&app,id,&directory);job.work(None);job.apply(&app);
        }
        assert!(!app.borrow().host.session.state().requests.iter().any(|item|matches!(item.kind,layer_ui::HostRequestKind::Paper { .. })));
        assert_eq!(checkpoint(&app),imported_checkpoint);assert_project_document(&document(&app),&imported);
        assert_eq!(layer_host::papers::list(&directory).unwrap().len(),1);
        effect(&app,json!({"op":"reset","layer":layer(&app),"key":"texture"}));
        let reset=checkpoint(&app);
        effect(&app,json!({"op":"set","layer":layer(&app),"key":"texture","value":{"kind":"choice","value":layer_core::paper::built_in_papers().len()}}));
        let pending=app.borrow().host.session.state().requests.iter().find_map(|item|
            matches!(item.kind,layer_ui::HostRequestKind::Paper { .. }).then_some(item.id));
        if let Some(id)=pending {let job=PaperJob::new(&app,id,&directory);job.work(None);job.apply(&app);}
        assert_project_document(&document(&app),&imported);
        app.invoke("undo");assert_eq!(checkpoint(&app),reset);
        app.invoke("redo");assert_project_document(&document(&app),&imported);
        let reopened = App::new(platform); reopened.action(json!({"type":"select_layer","id":layer(&reopened)}));
        let load = PaperJob::new(&reopened,0,&directory);load.work(None);load.apply(&reopened);
        let before = checkpoint(&reopened);
        let index = reopened.borrow().host.session.state().layer_properties.controls.iter()
            .find(|control|control.key=="texture").unwrap().paper.as_ref().unwrap().tiles.len()-1;
        effect(&reopened,json!({"op":"set","layer":layer(&reopened),"key":"texture","value":{"kind":"choice","value":index}}));
        assert_eq!(checkpoint(&reopened),before);
        let job = PaperJob::new(&reopened,paper_request(&reopened),&directory);job.work(None);job.apply(&reopened);
        assert_eq!(choice(&reopened),index as u32);
        let paper_layer=layer(&reopened);
        assert_source_samples(&reopened.borrow().host.session.paper_image(paper_layer).unwrap(),&original);
        assert!(matches!(reopened.borrow().host.session.new_drawing_paper(),layer_core::TextureSource::Image(_)));
        reopened.invoke("undo");assert_eq!(checkpoint(&reopened),before);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn apple_paper_retired_cancelled_and_replaced_requests_do_not_edit_the_current_document() {
    let directory = std::env::temp_dir().join(format!("capy-apple-paper-stale-{}",layer_workspace::new_id()));
    let app = App::new(1);app.action(json!({"type":"select_layer","id":layer(&app)}));
    let load=PaperJob::new(&app,0,&directory);load.work(None);load.apply(&app);
    for cancel in [false,true] {
        effect(&app,json!({"op":"import_paper","layer":layer(&app)}));
        let id=paper_request(&app);let job=PaperJob::new(&app,id,&directory);let input=gray_png();job.work(Some(&input));
        let before=document(&app);let mark=checkpoint(&app);let dirty=app.state()["document_file"]["modified"].clone();
        if cancel { unsafe { capy_paper_cancel(job.0) }; }
        else { app.action(json!({"type":"complete_request","id":id,"error":null})); }
        job.apply(&app);assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
        assert_eq!(app.state()["document_file"]["modified"],dirty);
        assert!(!app.borrow().host.session.state().requests.iter().any(|item|item.id==id));
    }
    effect(&app,json!({"op":"import_paper","layer":layer(&app)}));
    let id=paper_request(&app);let job=PaperJob::new(&app,id,&directory);let input=gray_png();job.work(Some(&input));
    app.action(json!({"type":"complete_request","id":id,"error":null}));
    app.borrow().host.session.renderer_mut().0=Some(native_renderer());app.draw_until_idle();
    let old=document(&app).artwork.id;
    let replacement = ProjectJob::create(&app, [64,64]);
    assert_eq!(unsafe { capy_apple_project_adopt(app.0,replacement.0,c"Replacement".as_ptr(),c"".as_ptr()) },0,"{:?}",app.borrow().error);
    app.action(json!({"type":"select_layer","id":layer(&app)}));
    effect(&app,json!({"op":"import_paper","layer":layer(&app)}));
    let current=paper_request(&app);assert!(current>id);
    let before=document(&app);let mark=checkpoint(&app);assert_ne!(before.artwork.id,old);
    let state=app.state();let requests=state["requests"].clone();let file=state["document_file"].clone();
    let history=app.with(|native| {let engine=native.host.session.engine();(engine.can_undo(),engine.can_redo())});
    job.apply(&app);
    assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
    assert_eq!(document(&app).artwork.id,before.artwork.id);
    assert_eq!(app.state()["requests"],requests);assert_eq!(paper_request(&app),current);
    assert_eq!(app.state()["document_file"],file);
    assert_eq!(app.with(|native| {let engine=native.host.session.engine();(engine.can_undo(),engine.can_redo())}),history);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn apple_paper_preview_leases_outlive_the_editor_and_match_shared_pixels() {
    let app=App::new(1);app.action(json!({"type":"select_layer","id":layer(&app)}));
    let mut ids:Vec<String>=app.with(|native|native.host.session.state().layer_properties.controls.iter().find_map(|control|control.paper.as_ref()).unwrap()
        .tiles.iter().filter_map(|tile|tile.id.as_deref().map(str::to_string)).collect());
    ids.push("missing".into());
    let expected:Vec<_>=ids.iter().map(|id|app.borrow().host.session.paper_preview(id)).collect();
    assert_eq!(expected.iter().filter(|image|image.is_some()).count(),layer_core::paper::built_in_papers().len());
    let json=std::ffi::CString::new(serde_json::to_string(&ids).unwrap()).unwrap();
    assert!((unsafe { capy_apple_paper_previews(app.0,std::ptr::null()) }==0));
    let previews=unsafe { capy_apple_paper_previews(app.0,json.as_ptr()) };assert!(!(previews==0));drop(app);
    assert_eq!(unsafe { capy_paper_previews_count(previews) },expected.len());
    for (index,image) in expected.iter().enumerate() {
        let mut count=0;
        let side=unsafe { capy_paper_previews_read(previews,index,&mut count) };
        if let Some(image)=image { assert_eq!(count,(side * side) as usize);assert_eq!(copied_bytes(count,|offset,output,length|unsafe {capy_paper_previews_copy(previews,index,offset,output,length)}),image.as_ref()); }
        else { assert_eq!(side,0); }
    }
    unsafe { capy_paper_previews_free(previews) };
}

#[test]
fn apple_paper_worker_failure_retires_its_request_without_authored_changes() {
    let directory=std::env::temp_dir().join(format!("capy-apple-paper-error-{}",layer_workspace::new_id()));
    let app=App::new(1);app.action(json!({"type":"select_layer","id":layer(&app)}));
    effect(&app,json!({"op":"import_paper","layer":layer(&app)}));
    let id=paper_request(&app);let job=PaperJob::new(&app,id,&directory);
    let before=document(&app);let mark=checkpoint(&app);let dirty=app.state()["document_file"]["modified"].clone();
    unsafe { capy_paper_work(job.0,-1,c"Invalid input".as_ptr()) };
    let error=unsafe { capy_paper_error(job.0) };assert!(!(error==0));
    capy_apple_string_free(error);
    job.apply(&app);assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
    assert_eq!(app.state()["document_file"]["modified"],dirty);
    assert!(!app.borrow().host.session.state().requests.iter().any(|item|item.id==id));
    assert!(app.state()["host_error"].is_string());
}

fn remembered_launch(directory: &std::path::Path, id: &str) -> String {
    let mut settings=layer_ui::Settings::default();settings.new_document.paper=Some(id.into());
    json!({"saved":serde_json::to_string(&settings).unwrap(),"preferred_languages":["en"],"paper_directory":directory}).to_string()
}

#[test]
fn apple_initial_paper_is_prepared_before_publication_and_later_library_reload_does_not_enqueue_another_load() {
    let directory=std::env::temp_dir().join(format!("capy-apple-paper-initial-{}",layer_workspace::new_id()));
    let mut input=gray_png();let mut encoded=Vec::new();std::io::Read::read_to_end(&mut input,&mut encoded).unwrap();
    let image=layer_core::paper::decode_png(&encoded).unwrap();
    let (id,paper,entries)=layer_host::papers::import(&directory,&image,"Cotton",&||false).unwrap();
    for platform in [0,1] {
        let configuration=CString::new(remembered_launch(&directory,&id)).unwrap();
        let app=App(unsafe {launch_json(platform,configuration.as_ptr(),std::ptr::null_mut(),std::ptr::null_mut())});assert!(!(app.0==0));
        let document=document(&app);
        assert_eq!(document.paper(None).texture,layer_core::paper::built_in_paper("cold_press").unwrap().texture);
        let layer_core::TextureSource::Image(actual)=app.borrow().host.session.new_drawing_paper() else {panic!("initial remembered image must exist before publication")};
        assert_source_samples(actual.storage(),&paper);
        assert_eq!(document.composition().size,[1440,1080]);assert_eq!(document.composition().resolution,Some(layer_core::ImageResolution::ppi(144)));
        assert!(!app.state()["document_file"]["modified"].as_bool().unwrap());
        assert!(!app.borrow().host.session.engine().can_undo());assert!(!app.borrow().host.session.engine().can_redo());
        let job=PaperJob::new(&app,0,&directory);job.work(None);job.apply(&app);
        assert!(!app.borrow().host.session.state().requests.iter().any(|request|matches!(request.kind,
            layer_ui::HostRequestKind::Paper {..}|layer_ui::HostRequestKind::SaveSettings {..})));
        assert_eq!(layer_host::papers::list(&directory).unwrap(),entries);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn apple_new_drawing_resolves_its_uncached_library_paper_on_the_project_worker() {
    let directory=std::env::temp_dir().join(format!("capy-apple-paper-new-{}",layer_workspace::new_id()));
    let mut input=gray_png();let mut encoded=Vec::new();std::io::Read::read_to_end(&mut input,&mut encoded).unwrap();
    let image=layer_core::paper::decode_png(&encoded).unwrap();
    let (id,paper,_)=layer_host::papers::import(&directory,&image,"Cotton",&||false).unwrap();
    for missing in [false,true] {
        if missing { std::fs::write(directory.join(format!("{id}.png")),b"invalid PNG").unwrap(); }
        let configuration=CString::new(json!({"saved":"","preferred_languages":["en"],"paper_directory":directory}).to_string()).unwrap();
        let app=App(unsafe {launch_json(1,configuration.as_ptr(),std::ptr::null_mut(),std::ptr::null_mut())});assert!(!(app.0==0));
        app.borrow().host.session.renderer_mut().0=Some(native_renderer());
        assert_eq!(unsafe {capy_apple_resize(app.0,1200,900,1.)},0);app.draw_frame();
        let list=PaperJob::new(&app,0,&directory);list.work(None);list.apply(&app);
        assert!(matches!(app.borrow().host.session.paper_material(&id),layer_ui::paper_library::PaperMaterial::Stored {..}),"the order begins uncached");
        let before=document(&app);let mark=checkpoint(&app);
        app.order_new_drawing([96,64],&[json!({"op":"paper"}),json!({"op":"pick_paper","id":id}),json!({"op":"use_paper"})]);
        let job=ProjectJob::new(&app,true);let pointer=job.0;
        assert_eq!(std::thread::spawn(move ||unsafe {capy_project_new(pointer)}).join().unwrap(),0);
        assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
        assert_eq!(unsafe {capy_apple_project_adopt(app.0,job.0,c"Untitled".as_ptr(),c"".as_ptr())},0);
        let texture=document(&app).paper(None).texture;
        if missing {
            assert_eq!(texture,layer_core::paper::default_texture(),"A missing library image keeps the built-in fallback");
            assert!(app.state()["notice"]["text"].as_str().unwrap().contains(&format!("{id}.png")),"{}",app.state()["notice"]);
        } else {
            let layer_core::TextureSource::Image(actual)=texture else {panic!("New must use the worker-resolved library image")};
            assert_source_samples(actual.storage(),&paper);
            assert!(app.state()["notice"].is_null());
        }
        assert_eq!(document(&app).composition().size,[96,64]);
        assert!(!app.state()["document_file"]["modified"].as_bool().unwrap());
        assert!(!app.borrow().host.session.engine().can_undo());assert!(!app.borrow().host.session.engine().can_redo());
        assert!(!app.borrow().host.session.state().requests.iter().any(|request|matches!(request.kind,layer_ui::HostRequestKind::SaveSettings {..})));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn apple_new_drawing_imports_paper_into_the_library_without_changing_the_drawing() {
    let directory=std::env::temp_dir().join(format!("capy-apple-paper-library-import-{}",layer_workspace::new_id()));
    for platform in [0,1] {
        let app=App::new(platform);
        app.borrow().host.session.renderer_mut().0=Some(native_renderer());app.draw_until_idle();
        let list=PaperJob::new(&app,0,&directory);list.work(None);list.apply(&app);
        app.invoke("new_document");
        app.new_drawing(json!({"op":"open","screen":null,"untitled":2}));
        app.new_drawing(json!({"op":"paper"}));
        let before=document(&app);let mark=checkpoint(&app);let requests=app.state()["requests"].clone();
        let picked=|app:&App| app.state()["new_drawing"]["drawer"]["tiles"].as_array().unwrap().iter().find(|tile|tile["selected"]==true).map(|tile|tile["id"].clone());
        let drawing=picked(&app);
        let cancelled=PaperJob::import(&app,&directory);unsafe { capy_paper_cancel(cancelled.0) };
        cancelled.work(Some(&gray_png()));cancelled.apply(&app);assert_eq!(picked(&app),drawing);
        let job=PaperJob::import(&app,&directory);job.work(Some(&gray_png()));job.apply(&app);
        let id=picked(&app).and_then(|id|id.as_str().map(str::to_string)).expect("the imported paper is picked");
        assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
        assert!(!app.state()["document_file"]["modified"].as_bool().unwrap());
        assert_eq!(app.state()["requests"],requests);
        assert!(layer_host::papers::list(&directory).unwrap().iter().any(|entry|entry.id==id));
        let tiles=app.state()["new_drawing"]["drawer"]["tiles"].as_array().unwrap().clone();
        assert!(tiles.iter().any(|tile|tile["id"]==id.as_str()),"{tiles:?}");
        assert!(matches!(app.borrow().host.session.paper_material(&id),layer_ui::paper_library::PaperMaterial::Ready(_)));
        app.new_drawing(json!({"op":"use_paper"}));
        assert_eq!(app.state()["new_drawing"]["setup"]["paper"]["texture"],id.as_str());
        let failed=PaperJob::import(&app,&directory);let pointer=failed.0;
        std::thread::spawn(move ||unsafe { capy_paper_work(pointer,-1,c"Missing".as_ptr()) }).join().unwrap();
        let error=unsafe { capy_paper_error(failed.0) };assert!(!(error==0));
        capy_apple_string_free(error);
        assert_eq!(unsafe { capy_apple_paper_apply(app.0,failed.0) },-1);
        assert_project_document(&document(&app),&before);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn apple_new_drawing_texture_visibility_reaches_the_drawing_built_on_the_project_worker() {
    for platform in [0,1] {
        let app=App::new(platform);
        app.borrow().host.session.renderer_mut().0=Some(native_renderer());app.draw_until_idle();
        let dialog=|app:&App| app.state()["new_drawing"].clone();
        app.invoke("new_document");
        app.new_drawing(json!({"op":"open","screen":null,"untitled":2}));
        assert_eq!((dialog(&app)["copy"]["relief"].as_str(),dialog(&app)["copy"]["relief_unit"].as_str()),(Some("Texture visibility"),Some("%")));
        let opened=dialog(&app)["setup"]["paper"]["relief"].as_f64().unwrap();
        assert_eq!(dialog(&app)["page"]["relief"],format!("{opened} %"));
        app.new_drawing(json!({"op":"paper"}));
        let relief=dialog(&app)["drawer"]["relief"].clone();
        assert_eq!((relief["value"].as_f64(),relief["enabled"].as_bool()),(Some(opened),Some(true)));
        let numeric=&relief["numeric"];
        assert_eq!((numeric["kind"].as_str(),numeric["min"].as_f64(),numeric["max"].as_f64(),numeric["step"].as_f64()),(Some("number"),Some(0.),Some(300.),Some(1.)));
        app.new_drawing(json!({"op":"paper_relief","value":250}));
        assert_eq!(dialog(&app)["drawer"]["relief"]["value"].as_f64(),Some(250.));
        app.new_drawing(json!({"op":"close_drawer"}));
        assert_eq!(dialog(&app)["setup"]["paper"]["relief"].as_f64(),Some(opened),"Cancel keeps the sheet's texture visibility");
        app.new_drawing(json!({"op":"paper"}));
        app.new_drawing(json!({"op":"pick_paper","id":null}));
        assert_eq!(dialog(&app)["drawer"]["relief"]["enabled"],false);
        app.new_drawing(json!({"op":"use_paper"}));
        assert!(dialog(&app)["page"]["relief"].is_null());
        app.new_drawing(json!({"op":"paper"}));
        app.new_drawing(json!({"op":"pick_paper","id":"cold_press"}));
        app.new_drawing(json!({"op":"paper_relief","value":149.6}));
        app.new_drawing(json!({"op":"use_paper"}));
        assert_eq!(dialog(&app)["page"]["relief"],"150 %");
        for action in [json!({"op":"unit","unit":"px"}),json!({"op":"width","value":64}),json!({"op":"height","value":48}),json!({"op":"create"})] { app.new_drawing(action); }
        app.save_settings();
        let job=ProjectJob::new(&app,true);
        assert_eq!(unsafe {capy_project_new(job.0)},0,"{:?}",job.error());
        assert_eq!(unsafe {capy_apple_project_adopt(app.0,job.0,c"Untitled".as_ptr(),c"".as_ptr())},0,"{:?} / {:?}",job.error(),app.borrow().error);
        let created=document(&app);
        let paper=created.bottom_paper().unwrap();
        assert_eq!(created.composition().size,[64,48]);
        assert_eq!(layer_core::paper::light(created.scene().effect(paper).unwrap()).map(|light|light.relief),Some(1.5));
    }
}

#[test]
fn apple_failed_initial_material_notifies_once_and_reload_does_not_repeat_the_failed_default_request() {
    let directory=std::env::temp_dir().join(format!("capy-apple-paper-initial-failure-{}",layer_workspace::new_id()));
    let mut input=gray_png();let mut encoded=Vec::new();std::io::Read::read_to_end(&mut input,&mut encoded).unwrap();
    let image=layer_core::paper::decode_png(&encoded).unwrap();
    let (id,_,_)=layer_host::papers::import(&directory,&image,"Cotton",&||false).unwrap();
    std::fs::write(directory.join(format!("{id}.png")),b"invalid PNG").unwrap();
    let configuration=CString::new(remembered_launch(&directory,&id)).unwrap();
    let app=App(unsafe {launch_json(1,configuration.as_ptr(),std::ptr::null_mut(),std::ptr::null_mut())});assert!(!(app.0==0));
    let before=document(&app);let mark=checkpoint(&app);let notice=app.state()["notice"]["id"].clone();assert!(!notice.is_null());
    assert_eq!(before.paper(None).texture,layer_core::paper::built_in_paper("cold_press").unwrap().texture);
    for _ in 0..2 {let job=PaperJob::new(&app,0,&directory);job.work(None);job.apply(&app);}
    assert_project_document(&document(&app),&before);assert_eq!(checkpoint(&app),mark);
    assert_eq!(app.state()["notice"]["id"],notice);
    assert_eq!(app.borrow().host.session.state().settings.new_document.paper.as_deref(),Some(id.as_str()));
    assert!(!app.borrow().host.session.state().requests.iter().any(|request|matches!(request.kind,
        layer_ui::HostRequestKind::Paper {..}|layer_ui::HostRequestKind::SaveSettings {..})));
    app.action(json!({"type":"select_layer","id":layer(&app)}));
    effect(&app,json!({"op":"set","layer":layer(&app),"key":"texture","value":{"kind":"choice","value":layer_core::paper::built_in_papers().len()}}));
    let request=paper_request(&app);let job=PaperJob::new(&app,request,&directory);
    unsafe {capy_paper_work(job.0,-1,c"Cotton".as_ptr())};
    let error=unsafe {capy_paper_error(job.0)};assert!(!(error==0));capy_apple_string_free(error);job.apply(&app);
    assert!(!app.borrow().host.session.state().requests.iter().any(|item|item.id==request));
    assert!(app.state()["host_error"].is_string());assert_project_document(&document(&app),&before);
    let job=PaperJob::new(&app,0,&directory);job.work(None);job.apply(&app);
    assert!(!app.borrow().host.session.state().requests.iter().any(|request|matches!(request.kind,layer_ui::HostRequestKind::Paper {..})));
    std::fs::remove_dir_all(directory).unwrap();
}
