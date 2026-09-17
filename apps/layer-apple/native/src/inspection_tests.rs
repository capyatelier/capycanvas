use super::*;
use layer_core::color::{ColorProfile, SampleDepth, RgbSpace, source::*};

fn initialized(platform: u32, space: RgbSpace, depth: SampleDepth) -> App {
    let app = App::new(platform);
    unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
    app.draw_until_idle();
    let task = ProjectJob::new(&app, true);
    let options = CString::new(json!({"extent":[5,5],"color":{"space":space,"depth":depth},"background":"Transparent"}).to_string()).unwrap();
    assert_eq!(unsafe { capy_project_new(task.0, options.as_ptr()) }, 0);
    assert_eq!(unsafe { capy_apple_project_adopt(app.0, task.0, c"Inspection".as_ptr(), c"".as_ptr()) }, 0);
    let mut source = SourceBuilder::new([5,5], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U16,
        profile: ColorProfile::Builtin(space), profile_assumed: false,
    }, 1024*1024).unwrap();
    // Nineteen opaque red pixels, one half-alpha blue, five transparent green.
    for y in 0..5 {
        let row: Vec<u8> = (0..5).flat_map(|x| {
            let rgba: [u16;4] = if y == 4 { [0,65535,0,0] } else if x == 2 && y == 2 { [0,0,65535,32768] } else { [65535,0,0,65535] };
            rgba.into_iter().flat_map(u16::to_le_bytes)
        }).collect();
        source.push_row(&row).unwrap();
    }
    unsafe { &mut *app.0 }.host.session.import_layer_source("Known pixels", source.finish().unwrap()).unwrap();
    app.draw_until_idle(); app
}
fn capture(app: &App) -> ProjectJob {
    let task = unsafe { capy_apple_project_task(app.0, 7, std::ptr::null()) };
    assert!(!task.is_null(), "{:?}", unsafe { CStr::from_ptr(capy_apple_error(app.0)) }); ProjectJob(task)
}
fn histogram(task: &ProjectJob) -> Value {
    let text = unsafe { capy_project_details(task.0) };
    assert!(!text.is_null(), "{:?}", task.error());
    let value = serde_json::from_slice(unsafe { CStr::from_ptr(text) }.to_bytes()).unwrap();
    unsafe { capy_apple_string_free(text) }; value
}

#[test]
fn apple_histogram_keeps_full_resolution_precision_snapshot_identity_and_cancellation() {
    for platform in [0,1] {
        for (space, depth) in [(RgbSpace::DisplayP3,SampleDepth::U8), (RgbSpace::ProPhoto,SampleDepth::U16)] {
            let app = initialized(platform, space, depth);
            app.invoke("histogram");
            let request = app.state()["requests"].as_array().unwrap().iter().find(|r| r["kind"]["type"] == "histogram").unwrap().clone();
            app.action(json!({"type":"complete_request","id":request["id"],"error":null}));
            let original = unsafe { &*app.0 }.host.session.engine().document().clone();
            let job = capture(&app);
            let cancelled = capture(&app);
            unsafe { capy_project_cancel(cancelled.0) };
            assert!(unsafe { capy_project_details(cancelled.0) }.is_null());
            assert!(cancelled.error().unwrap().contains("cancelled"));
            // Inspection captured before a later edit must still report its old revision/pixels.
            let layer = original.active_layer;
            app.layer_action(json!({"op":"visibility","id":layer.0,"value":false}));
            app.draw_until_idle();
            let output = histogram(&job);
            assert_eq!(output["revision"], original.revision);
            assert_eq!(output["histogram"]["color"], json!({"space":space,"depth":depth}));
            assert_eq!(output["histogram"]["pixels"],20);
            assert_eq!(output["histogram"]["transparent"],5);
            assert!(output["sampled_time"].is_null());
            for (channel, black, white) in [(0,1,19),(1,20,0),(2,19,1)] {
                let c = &output["histogram"]["channels"][channel];
                assert_eq!(c["bins"][0],black); assert_eq!(c["bins"][255],white);
                assert_eq!(c["below"],0); assert_eq!(c["above"],0);
            }
            assert_eq!(histogram(&capture(&app))["histogram"]["pixels"],0);
            app.invoke("undo"); app.draw_until_idle();
            assert_eq!(histogram(&capture(&app))["histogram"], output["histogram"]);
            // Visible paper participates, transparent pixels do not.
            let paper = original.layers.iter().find(|l| l.name.as_ref() == "Paper").unwrap().id;
            app.layer_action(json!({"op":"visibility","id":paper.0,"value":true})); app.draw_until_idle();
            assert_eq!(histogram(&capture(&app))["histogram"]["pixels"],25);
        }
    }
}

#[test]
fn apple_point_and_area_sampling_use_document_linear_coverage_without_changing_artwork_or_opacity() {
    for platform in [0,1] {
        let app = initialized(platform, RgbSpace::DisplayP3, SampleDepth::U16);
        app.action(json!({"type":"set_brush_opacity","value":0.37}));
        let original = unsafe { &*app.0 }.host.session.engine().document().clone();
        let pixels = app.pixels();
        app.invoke("eyedropper");
        for source in ["pick_visible", "pick_layer"] {
            app.layer_action(json!({"op":"tool","tool":source}));
            for (width, red) in [(1,0.),(3,8.),(5,19.)] {
                app.action(json!({"type":"set_color_sample_size","width":width}));
                app.action(json!({"type":"set_color","rgba":[0.,1.,0.,1.]}));
                app.draw_until_idle();
                let [a,b,c,d,e,f] = unsafe { &*app.0 }.host.session.state().camera.document_to_surface();
                let [x,y] = [a*2.5+c*2.5+e,b*2.5+d*2.5+f];
                let records = [f64::from(x),f64::from(y),1.,0.,0.,0.,0.,3_000_000_000.,1.,
                    f64::from(x),f64::from(y),1.,0.,0.,0.,0.,3_010_000_000.,3.];
                let camera = unsafe { capy_apple_camera_revision(app.0) };
                assert_eq!(unsafe { capy_apple_pointer(app.0,1,1,0,records.as_ptr(),records.len(),0,camera) },0);
                let alpha = 32768. / 65535.;
                let expected = [red/(red+alpha),0.,alpha/(red+alpha)];
                let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
                loop {
                    app.draw_frame();
                    let color = unsafe { &*app.0 }.host.session.state().colors.definition();
                    let actual = color.linear_in(RgbSpace::DisplayP3).unwrap();
                    if actual[..3].iter().zip(expected).all(|(a,b)| (*a as f64-b).abs()<0.0005) {
                        assert_eq!(color.space,RgbSpace::DisplayP3); assert_eq!(color.rgba[3],1.); break;
                    }
                    assert!(std::time::Instant::now()<deadline,"{source} {width}: {actual:?} != {expected:?}");
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                assert_eq!(app.state()["brush"]["opacity"],json!(0.37f32));
                assert_project_document(unsafe { &*app.0 }.host.session.engine().document(),&original);
                assert_eq!(app.pixels(),pixels);
            }
        }
    }
}
