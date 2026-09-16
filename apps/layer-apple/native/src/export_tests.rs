//! Actual Apple snapshot jobs: exact retained samples, profiled copies and retry.
use super::*;
use layer_core::color::{ColorProfile, IntegerDepth, RgbSpace, source::*};
use layer_ui::{ExportBackground, ExportFormat, ExportRecipe, ExportResolution, ExportSize};
use std::{io::{Read, Seek, Write}, os::{fd::AsRawFd, unix::fs::OpenOptionsExt}};

fn temporary() -> std::fs::File {
    let path = std::env::temp_dir().join(format!("capy-output-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path).unwrap();
    std::fs::remove_file(path).unwrap(); file
}
fn profile_source(space: RgbSpace) -> SourceImage {
    let builtin = ColorProfile::Builtin(space);
    let profile = if space == RgbSpace::ProPhoto {
        ColorProfile::Icc(layer_color::profile_bytes(&builtin).unwrap().into())
    } else { builtin };
    let mut builder = SourceBuilder::new([17,11], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: IntegerDepth::U16,
        profile, profile_assumed: false,
    }, 1024*1024).unwrap();
    for y in 0..11u16 {
        let row: Vec<u8> = (0..17u16).flat_map(|x| [1234+x*713,54321-y*997,31234,
            if x == 0 {0} else if x%3 == 0 {213} else {65535}].into_iter().flat_map(u16::to_le_bytes)).collect();
        builder.push_row(&row).unwrap();
    }
    builder.finish().unwrap()
}
fn initialize(platform: u32, space: RgbSpace, depth: IntegerDepth) -> (App, SourceImage) {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer()); app.draw_until_idle();
    let source = profile_source(space);
    // Open an untouched photo master, with exactly one visible source layer.
    // Placing into a new drawing adds a second layer and requires compositing,
    // which intentionally discards hidden RGB at zero alpha.
    let mut archive = Vec::new();
    layer_color::photo_project(source.clone(), "Retained original", depth).unwrap().write(&mut archive).unwrap();
    let opened = ProjectJob::new(&app, true);
    assert_eq!(unsafe { capy_project_read_bytes(opened.0, archive.as_ptr(), archive.len(), c"Original.capy".as_ptr()) },0);
    assert_eq!(unsafe { capy_apple_project_adopt(app.0,opened.0,c"Original".as_ptr(),c"".as_ptr()) },0);
    app.draw_until_idle(); app.invoke("export_document");
    (app, source)
}
fn capture(app: &App) -> ProjectJob {
    let id = app.state()["requests"].as_array().unwrap().iter()
        .find(|r| r["kind"]["request"]["type"] == "export").unwrap()["id"].as_u64().unwrap() as u32;
    let mut pointer = std::ptr::null_mut();
    assert_eq!(unsafe { capy_apple_export_task(app.0,id,2_000_000_000,&mut pointer) },1);
    assert!(!pointer.is_null()); ProjectJob(pointer)
}
fn configure(job: &ProjectJob, recipe: &ExportRecipe) -> i32 {
    let recipe = CString::new(serde_json::to_string(recipe).unwrap()).unwrap();
    unsafe { capy_project_export_options(job.0,recipe.as_ptr()) }
}
fn output(job: &ProjectJob) -> SourceImage {
    let mut file = temporary();
    assert_eq!(unsafe { capy_project_write(job.0,file.as_raw_fd()) },0,"{:?}",job.error());
    file.rewind().unwrap();
    layer_color::photo::read_photo(std::io::BufReader::new(file),Default::default()).unwrap()
}
fn exact(actual: &SourceImage, expected: &SourceImage) {
    assert_eq!(actual.extent,expected.extent);
    assert_eq!(actual.interpretation.depth,expected.interpretation.depth);
    assert_eq!(actual.interpretation.channels,expected.interpretation.channels);
    let mut a = actual.rows(); let mut b = expected.rows();
    let mut ar = vec![0;actual.row_bytes()]; let mut br = vec![0;expected.row_bytes()];
    for y in 0..actual.extent[1] { a.read(y,&mut ar).unwrap();b.read(y,&mut br).unwrap();assert_eq!(ar,br,"Exact retained row {y}"); }
    assert_eq!(layer_color::profile_bytes(&actual.interpretation.profile).unwrap(),
        layer_color::profile_bytes(&expected.interpretation.profile).unwrap());
}

#[test]
fn profiled_apple_export_keeps_u16_hidden_rgb_retries_and_survives_owner_closure() {
    for platform in [0,1] {
        for (space,depth) in [(RgbSpace::DisplayP3,IntegerDepth::U8),(RgbSpace::ProPhoto,IntegerDepth::U16)] {
            let (app,source) = initialize(platform,space,depth);
            let master = unsafe { &*app.0 }.host.session.engine().document().clone();
            let file_state = app.state()["document_file"].clone();
            let job = capture(&app); let cancelled = capture(&app);
            let mut recipe = ExportRecipe::further_editing(master.color);
            recipe.profile.profile = source.interpretation.profile.clone();
            recipe.resolution = ExportResolution::Ppi(240);
            let mut invalid = recipe.clone();invalid.format = ExportFormat::Jpeg;
            assert_eq!(configure(&job,&invalid),-1);
            assert_eq!(configure(&job,&recipe),0,"{:?}",job.error());
            assert_eq!(unsafe { capy_project_compare(job.0) },0,"{:?}",job.error());
            for after in [false,true] {
                let mut preview = CapyProjectPreview {width:0,height:0,pixels:std::ptr::null(),count:0};
                assert_eq!(unsafe { capy_project_preview(job.0,after,&mut preview) },0);
                assert!(preview.width > 0 && preview.height > 0 && preview.count > 0);
            }
            assert_project_document(unsafe { &*app.0 }.host.session.engine().document(),&master);
            assert_eq!(app.state()["document_file"],file_state,"Output choices must not save or modify the master");
            unsafe { capy_project_cancel(cancelled.0) };
            let mut untouched = temporary();untouched.write_all(b"untouched").unwrap();untouched.rewind().unwrap();
            assert_eq!(unsafe { capy_project_write(cancelled.0,untouched.as_raw_fd()) },-1);
            let mut bytes = Vec::new();untouched.read_to_end(&mut bytes).unwrap();assert_eq!(bytes,b"untouched");
            assert_eq!(unsafe { capy_project_write(job.0,-1) },-1,"Invalid output must remain retryable");
            app.layer_action(json!({"op":"visibility","id":master.active_layer.0,"value":false}));app.draw_until_idle();
            drop(app); // Snapshot owns the earlier project and device without an editor.
            for format in [ExportFormat::Png,ExportFormat::Tiff] {
                recipe.format = format;assert_eq!(configure(&job,&recipe),0);
                let saved = output(&job);exact(&saved,&source);
                assert_eq!(saved.resolution.unwrap().pixels_per_inch()[0].round(),240.);
                exact(&output(&job),&source); // Reusing the job after writing preserves exact samples.
            }
            recipe.size = ExportSize::Fit {bounds:[9,9],enlarge:false};
            assert_eq!(configure(&job,&recipe),0);assert_eq!(output(&job).extent,[9,6]);
            recipe.format = ExportFormat::Jpeg;recipe.depth = IntegerDepth::U8;recipe.background = ExportBackground::White;
            assert_eq!(configure(&job,&recipe),0);
            let jpeg = output(&job);assert_eq!(jpeg.extent,[9,6]);
            assert_eq!(jpeg.interpretation.channels,SourceChannels::Rgb);
            assert_eq!(jpeg.interpretation.depth,IntegerDepth::U8);
            assert_eq!(layer_color::profile_bytes(&jpeg.interpretation.profile).unwrap(),layer_color::profile_bytes(&source.interpretation.profile).unwrap());
        }
    }
}
