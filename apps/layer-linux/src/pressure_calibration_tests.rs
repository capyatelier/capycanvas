use super::*;
use super::new_photo::ready;
use super::photo_edit::window_point;
use layer_core::color::SampleDepth;
use serde_json::json;

#[test]
#[ignore = "private Mutter display and tablet proxy; workspace-motion.sh gtk --native-test=native_pressure_calibration --tablet"]
fn native_pressure_calibration() {
    let app = native_test_app("art.capycanvas.PressureCalibration");
    opaque_surfaces_own_their_translucent_controls();
    let w = Workspace::with_project(&app, Some((new_drawing_at(256,256,SampleDepth::F32), None)));
    w.window.set_default_size(1100,800); w.window.maximize(); w.window.present(); ready(&w);
    set_transparency(&w, "LAYER_MOTION_TRANSPARENCY");
    let theme = if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|v|v=="dark") { Theme::Dark } else { Theme::Light };
    w.dispatch(UiAction::SetTheme{theme:Some(theme)});
    w.dispatch(UiAction::SetBrushSize{value:8.});
    let mut native = RemoteInput::new(); native.ready();
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap()); std::fs::create_dir_all(&output).unwrap();
    let layout = serde_json::to_value(&state(&w).workspace).unwrap();
    let original = state(&w).settings.pressure_curve;
    for device in ["mouse","touch","pen"] {
        w.dispatch(UiAction::OpenSettings{page:SettingsPage::Input});pump(300);
        let adjust=find_named(w.preferences.dialog.upcast_ref(),"setting-pen-pressure-action").unwrap();
        super::histogram::scroll_to(&adjust);pump(100);
        let at=screen_point(&adjust,&w.window,[0.5,0.5]);
        let css = gtk::CssProvider::new();
        css.load_from_string(".painted-calibration-opening { background-color: #ff00ff; background-image: none; }");
        w.pressure_calibration.root.add_css_class("painted-calibration-opening");
        gtk::style_context_add_provider_for_display(&w.surface.display(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
        let frames = sample_after_paint(&w.surface, || ());
        native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        until(|| !w.preferences.dialog.is_mapped(),"preferences closes before calibration");pump(250);
        let frames = frames.finish();
        let final_bounds = frames.last().and_then(|frame| frame.color_bounds(&w.window, [255, 0, 255, 255]))
            .expect("opening pressure calibration paints its panel");
        for frame in frames {
            if let Some(bounds) = frame.color_bounds(&w.window, [255, 0, 255, 255]) {
                assert_eq!(bounds, final_bounds, "pressure calibration first paint has its measured extent in frame {}", frame.time);
            }
        }
        w.pressure_calibration.root.remove_css_class("painted-calibration-opening");
        gtk::style_context_remove_provider_for_display(&w.surface.display(), &css);
        let panel = w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
        assert!(panel.is_mapped()); assert!(!state(&w).settings_open);
        if device=="mouse" {
            let bounds=panel.compute_bounds(&w.surface).unwrap();let draft=response_view(&w,false).editor.points;
            w.dispatch(UiAction::Invoke {command:CommandId::Lasso});pump(100);
            native.perform(json!([{"pen":"down","point":window_point(&w,[40.,180.]),"tilt":[30.,0.]},{"pen":"up"},{"pen":"leave"}]));
            assert!(panel.is_mapped(),"first Lasso pen contact keeps pressure visible");
            assert_eq!(panel.compute_bounds(&w.surface).unwrap(),bounds);
            assert_eq!(response_view(&w,false).editor.points,draft);
            w.dispatch(UiAction::Invoke {command:CommandId::Pencil});pump(100);
            let before=artwork_history(&w);
            let title=find_named(panel,"pen-pressure-title").unwrap();let at=screen_point(&title,&w.window,[0.5,0.5]);
            native.perform(json!([{"pen":"move","point":at,"tilt":[30.,0.]},{"pen":"down","point":at,"tilt":[0.,40.]},{"pen":"up"},{"pen":"leave"}]));
            assert_eq!(artwork_history(&w),before,"pressure dialog contacts never paint or add history");
            measured_extent_recovers(&w,false);
        }
        assert_eq!(state(&w).pressure_calibration.unwrap().editor.points.len(),3);
        let snapshot=gtk::Snapshot::new();w.surface.snapshot_child(panel,&snapshot);
        let mut glass=Vec::new();crate::glass::collect(&snapshot.to_node().unwrap(),&state(&w).palette.glass.surfaces(),&mut glass);
        assert!(glass.is_empty(),"opaque utility owns its background: {glass:?}");
        assert_dialog_footer(panel,"pen-pressure");
        let graph = find_named(panel,"pen-pressure-graph").unwrap();
        assert!(find_named(panel,"pen-pressure-input").is_none() && find_named(panel,"pen-pressure-output").is_none());
        let input_axis=find_named(panel,"pen-pressure-input-axis").unwrap();
        let output_axis=find_named(panel,"pen-pressure-output-axis").unwrap();
        let input_bounds=input_axis.compute_bounds(&graph).unwrap(); let output_bounds=output_axis.compute_bounds(&graph).unwrap();
        assert!(input_bounds.y()>=graph.height() as f32);
        assert!((input_bounds.x()+input_bounds.width()*0.5-graph.width() as f32*0.5).abs()<1.);
        assert!(output_bounds.x()+output_bounds.width()<=0.);
        assert!((output_bounds.y()+output_bounds.height()*0.5-graph.height() as f32*0.5).abs()<1.);
        assert!(output_bounds.height()>output_bounds.width(),"output caption is sideways");
        let title = find_named(panel,"pen-pressure-title").unwrap();
        assert_eq!(title.pango_context().font_description().unwrap().weight(),gtk::pango::Weight::Bold);
        let close=find_named(panel,"pen-pressure-close").unwrap(); let header=close.parent().unwrap();
        let close_bounds=close.compute_bounds(&header).unwrap();
        assert!(close_bounds.y()>=4. && close_bounds.y()+close_bounds.height()<=header.height() as f32-4.);
        assert!(close_bounds.x()+close_bounds.width()<=header.width() as f32-6.);
        let lighter = find_named(panel,"pen-pressure-lighter").unwrap();
        let start = state(&w).pressure_calibration.unwrap().editor.points[0][1];
        crate::snapshot(&w).save_to_png(output.join(format!("pressure-{theme:?}-{device}-initial.png"))).unwrap();
        if device=="mouse" && std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
            native.perform(json!([{"wait_ms":250},{"capture":"pressure-default"}]));
            let at=screen_point(&close,&w.window,[0.5,0.5]);
            native.perform(json!([{"point":at},{"wait_ms":100},{"capture":"pressure-close-hover"}]));
            assert!(close.state_flags().contains(gtk::StateFlags::PRELIGHT));
        }
        let at = screen_point(&lighter,&w.window,[0.5,0.5]);
        let hit = w.window.pick(f64::from(at[0]),f64::from(at[1]),gtk::PickFlags::DEFAULT);
        assert!(hit.as_ref().is_some_and(|v|v==&lighter || v.is_ancestor(&lighter)), "pressure button hit {:?} at {at:?}",hit.map(|v|v.widget_name()));
        native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        assert!((state(&w).pressure_calibration.unwrap().editor.points[0][1]-start-0.025).abs()<1e-6,"{device}: starting output {start}, current {}",state(&w).pressure_calibration.unwrap().editor.points[0][1]);
        assert_eq!(state(&w).settings.pressure_curve,original);
        let point = state(&w).pressure_calibration.unwrap().editor.points[1];
        let at = graph_point(&w,&graph,[point[0],1.-point[1]]);
        let to = graph_point(&w,&graph,[point[0]+0.05,1.-point[1]+0.15]);
        native.perform(json!([contact(device,"down",at),contact(device,"move",to),contact(device,"up",to)]));
        let selected = state(&w).pressure_calibration.unwrap().editor.controls.selected;
        assert_eq!(selected,Some(1),"{device} selects the actual control point");
        let point_after = state(&w).pressure_calibration.unwrap().editor.points[1];
        assert!(point_after[0]>point[0]+0.02 && point_after[1]<point[1]-0.1,"{device} moves the control");
        let at=graph_point(&w,&graph,[point_after[0],1.-point_after[1]]);
        let to=graph_point(&w,&graph,[point_after[0],-0.25]);
        native.perform(json!([contact(device,"down",at),contact(device,"move",to)]));
        assert_eq!(state(&w).pressure_calibration.unwrap().editor.points.len(),2,"{device} removes an interior point before release outside the graph");
        native.perform(json!([contact(device,"up",to)]));
        assert_eq!(state(&w).pressure_calibration.unwrap().editor.points.len(),2,"{device} release keeps the point removed");
        let reset=find_named(panel,"pen-pressure-reset").unwrap();native.click(screen_point(&reset,&w.window,[0.5,0.5]));
        assert_eq!(state(&w).pressure_calibration.unwrap().editor.points,original.points());
        let at = screen_point(&title,&w.window,[0.5,0.5]); let to=[at[0]-140.,at[1]+40.];
        let before=state(&w).pressure_calibration.unwrap().bounds;
        native.perform(json!([contact(device,"down",at),contact(device,"move",to),contact(device,"up",to)]));
        let after=state(&w).pressure_calibration.unwrap().bounds;
        assert!(after.x<before.x-100. && after.y>before.y+20.,"{device} drags the utility");
        assert_eq!(serde_json::to_value(&state(&w).workspace).unwrap(),layout,"utility drag never changes workspace");
        if device != "touch" {
            let before = super::editing_tools::raster(&w);
            let pixels = super::editing_tools::pixels(&before);
            let a = window_point(&w,if device=="pen" {[150.,235.]} else {[40.,210.]});
            let b=window_point(&w,if device=="pen" {[210.,245.]} else {[100.,220.]});
            let hit=w.window.pick(f64::from(a[0]),f64::from(a[1]),gtk::PickFlags::DEFAULT).unwrap();
            assert!(hit==w.area || hit.is_ancestor(&w.area),"native drawing starts on canvas, got {}",hit.widget_name());
            native.perform(json!([contact(device,"down",a),contact(device,"move",b)]));
            if device=="pen" {
                let marker=state(&w).pressure_calibration.unwrap().editor.marker.unwrap();
                assert!((marker[0]-32768./65535.).abs()<1e-6,"native pressure marker {marker:?}");
                if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                    native.perform(json!([{"wait_ms":250},{"capture":"pressure-live"}]));
                }
            }
            native.perform(json!([contact(device,"up",b)]));
            if device=="pen" { assert!(state(&w).pressure_calibration.unwrap().editor.marker.is_none(),"pen lift clears live pressure"); }
            super::editing_tools::committed(&w,&before);
            assert_ne!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),pixels,"{device} paints with utility open");
            assert!(state(&w).pressure_calibration.is_some());
        }
        crate::snapshot(&w).save_to_png(output.join(format!("pressure-{theme:?}-{device}.png"))).unwrap();
        record_layout(&w,panel,"pen-pressure",&output,theme);
        let cancel=find_named(panel,"pen-pressure-cancel").unwrap();
        native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
        assert!(state(&w).pressure_calibration.is_none()); assert_eq!(state(&w).settings.pressure_curve,original);
    }
    w.dispatch(UiAction::Invoke{command:CommandId::PenPressure}); pump(200);
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let lighter=find_named(panel,"pen-pressure-lighter").unwrap(); native.click(screen_point(&lighter,&w.window,[0.5,0.5]));
    let chosen=state(&w).pressure_calibration.unwrap().editor.points;
    let apply=find_named(panel,"pen-pressure-apply").unwrap(); native.click(screen_point(&apply,&w.window,[0.5,0.5]));
    assert_eq!(state(&w).settings.pressure_curve.points(),chosen);
    w.dispatch(UiAction::Invoke{command:CommandId::PenPressure}); pump(200);
    let reset=find_named(panel,"pen-pressure-reset").unwrap();native.click(screen_point(&reset,&w.window,[0.5,0.5]));
    assert_eq!(state(&w).pressure_calibration.unwrap().editor.points,original.points());
    let close=find_named(panel,"pen-pressure-close").unwrap();native.click(screen_point(&close,&w.window,[0.5,0.5]));
    assert_eq!(state(&w).settings.pressure_curve.points(),chosen);
    w.dispatch(UiAction::Invoke{command:CommandId::PenPressure}); pump(200);
    let graph=find_named(panel,"pen-pressure-graph").unwrap();graph.grab_focus();
    native.key(0xff1b);assert!(state(&w).pressure_calibration.is_none());
    utility_over_column(&w,&mut native,&output,theme,false);
    if std::env::var("LAYER_PRESSURE_MOTION").is_ok_and(|value|value=="1") {
        w.dispatch(UiAction::Invoke{command:CommandId::PenPressure});pump(300);
        w.dispatch(UiAction::CurveEditor {target:layer_ui::CurveEditorTarget::Pressure,action:layer_ui::CurveEditorAction::Reset});pump(300);
        native.settle_ms=0;native.timeout=Duration::from_secs(30);
        measure_motion(&w,&mut native,&output,false);
    }
    native.finish();w.window.close();
}

fn graph_point(w: &Workspace, graph: &gtk::Widget, point: [f32;2]) -> [f32;2] {
    graph_point_for(w,graph,point,false)
}

fn response_view(w:&Workspace,tilt:bool)->layer_ui::PressureCalibrationView {
    with_response(w,tilt,Clone::clone)
}
fn with_response<T>(w:&Workspace,tilt:bool,f:impl FnOnce(&layer_ui::PressureCalibrationView)->T)->T {
    let gpu=w.gpu.borrow();let state=gpu.as_ref().unwrap().session.state();
    f(if tilt {&state.tilt_calibration.as_ref().unwrap().panel}else{state.pressure_calibration.as_ref().unwrap()})
}
fn graph_point_for(w:&Workspace,graph:&gtk::Widget,point:[f32;2],tilt:bool)->[f32;2] {
    let inset=response_view(w,tilt).editor.controls.inset;
    let at=[(inset+point[0]*(graph.width() as f32-2.*inset))/graph.width() as f32,
        (inset+point[1]*(graph.height() as f32-2.*inset))/graph.height() as f32];
    screen_point(graph,&w.window,at)
}

fn opaque_surfaces_own_their_translucent_controls() {
    use gtk::{gdk,gsk,graphene};
    let bounds=graphene::Rect::new(0.,0.,100.,100.);
    let surface=layer_ui::GlassColor([0.4,0.4,0.4,0.5]);
    let child=gsk::ColorNode::new(&gdk::RGBA::new(0.4,0.4,0.4,0.5),&graphene::Rect::new(10.,10.,40.,40.));
    for alpha in [1.,0.5] {
        let background=gsk::ColorNode::new(&gdk::RGBA::new(0.2,0.2,0.2,alpha),&bounds);
        let node=gsk::ContainerNode::new(&[background.upcast(),child.clone().upcast()]);
        let mut regions=Vec::new();crate::glass::collect(node.upcast_ref(),&[surface],&mut regions);
        assert_eq!(regions.len(),usize::from(alpha!=1.));
        let node=gsk::OpacityNode::new(node.upcast_ref(),0.5);
        let mut regions=Vec::new();crate::glass::collect(node.upcast_ref(),&[surface],&mut regions);
        assert_eq!(regions.len(),1);
    }
}

fn measure_motion(w: &Rc<Workspace>, input: &mut RemoteInput, output: &std::path::Path, tilt:bool) {
    let mut reports=Vec::new();let prefix=if tilt {"pen-tilt"}else{"pen-pressure"};
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let title=find_named(panel,&format!("{prefix}-title")).unwrap();
    let bounds=response_view(w,tilt).bounds;
    let viewport=[w.surface.width() as f32,w.surface.height() as f32];
    let origin=[(viewport[0]-bounds.width)*0.5,(viewport[1]-bounds.height+200.)*0.5];
    assert!(origin[0]>=200. && origin[0]+bounds.width+200.<=viewport[0]);
    assert!(origin[1]>=200. && origin[1]+bounds.height<=viewport[1]);
    let point=screen_point(&title,&w.window,[0.5,0.5]);
    input.perform(json!([{"point":point},{"wait_ms":100}]));
    input.perform(json!([{"down":true},
        {"point":[point[0]+origin[0]-bounds.x,point[1]+origin[1]-bounds.y]}, {"down":false}]));
    pump(200);
    for scenario in ["panel","curve"] {
        for gesture in 0..3 {
            let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
            let target=find_named(panel,&format!("{prefix}-{}",if scenario=="panel" {"title"}else{"graph"})).unwrap();
            let view=response_view(w,tilt);
            let point=if scenario=="panel" {screen_point(&target,&w.window,[0.5,0.5])}
                else {let p=view.editor.points[1];graph_point_for(w,&target,[p[0],1.-p[1]],tilt)};
            input.perform(json!([{"point":point},{"wait_ms":100}]));
            let hit=w.window.pick(f64::from(point[0]),f64::from(point[1]),gtk::PickFlags::DEFAULT);
            assert!(hit.as_ref().is_some_and(|v|v==&target || v.is_ancestor(&target)),"{scenario} motion target hit {:?} at {point:?}",hit.map(|v|v.widget_name()));
            let current=move |w:&Workspace| {
                if scenario=="panel" {
                    let b=w.pressure_calibration.root.compute_bounds(&w.surface).unwrap();
                    return [b.x(),b.y()];
                }
                with_response(w,tilt,|v|v.editor.points[1])
            };
            let observed=Rc::new(RefCell::new(Vec::new()));let observations=observed.clone();let owner=w.clone();let mut previous=current(w);
            let initial_native=previous;
            let shared=Rc::new(RefCell::new(Vec::new()));let shared_observations=shared.clone();let mut previous_shared=view.bounds;
            let observer=glib::timeout_add_local(Duration::from_millis(1),move || {
                let value=current(&owner);
                if value!=previous {observations.borrow_mut().push((glib::monotonic_time(),value));previous=value;}
                if scenario=="panel" {
                    let value=with_response(&owner,tilt,|v|v.bounds);
                    if value!=previous_shared {shared_observations.borrow_mut().push((glib::monotonic_time(),value));previous_shared=value;}
                }
                glib::ControlFlow::Continue
            });
            let clock=w.surface.frame_clock().unwrap();let timings=Rc::new(RefCell::new(Vec::new()));
            let after=clock.connect_after_paint(glib::clone!(#[strong] timings,move |clock| if let Some(t)=clock.current_timings(){timings.borrow_mut().push(t);}));
            let mut events=vec![json!({"down":true})];
            for i in 1..=625 {
                let t=i as f32/625.*std::f32::consts::TAU;
                let [x,y]=if scenario=="panel" {[200.*t.sin(),100.*(t.cos()-1.)]} else {[4.*t.sin(),20.*(1.-t.cos())]};
                events.push(json!({"point":[point[0]+x,point[1]+y]}));
            }
            events.push(json!({"down":false}));input.perform(json!(events));pump(300);observer.remove();clock.disconnect(after);
            let observed=observed.borrow();
            if observed.len()<=100 {
                std::fs::write(output.join(if tilt {"tilt-motion-failure.json"}else{"pressure-motion-failure.json"}),serde_json::to_vec_pretty(&json!({
                    "scenario":scenario,"gesture":gesture,"moving_updates":observed.len(),
                    "initial_native_position":initial_native,"initial_shared_bounds":view.bounds,
                    "moving_positions":observed.iter().map(|(timestamp,position)|json!({"timestamp_us":timestamp,"position":position})).collect::<Vec<_>>(),
                    "shared_positions":shared.borrow().iter().map(|(timestamp,bounds)|json!({"timestamp_us":timestamp,"bounds":bounds})).collect::<Vec<_>>(),
                    "shared_bounds":response_view(w,tilt).bounds,
                })).unwrap()).unwrap();
            }
            assert!(observed.len()>100,"measure actual moving state: {scenario} gesture {gesture}, {} updates, first {:?}, last {:?}",observed.len(),observed.first(),observed.last());
            if scenario=="panel" {
                let scale=w.surface.scale_factor() as f32;
                assert!(observed.iter().all(|(_,position)|position.iter().all(|v|(v*scale-(v*scale).round()).abs()<1e-3)),"native placement is snapped to the device-pixel grid");
            }
            let start=observed.first().unwrap().0;let end=observed.last().unwrap().0;
            let mut presented:Vec<_>=timings.borrow().iter().filter(|t|t.is_complete()).map(|t|t.presentation_time()).filter(|t|*t>=start && *t<=end).collect();
            presented.sort_unstable();presented.dedup();assert!(presented.len()>100,"native presentation timestamps");
            let mut intervals:Vec<_>=presented.windows(2).map(|p|(p[1]-p[0]) as f64/1000.).collect();intervals.sort_by(f64::total_cmp);
            let hz=(presented.len()-1) as f64*1e6/(presented.last().unwrap()-presented.first().unwrap()) as f64;
            let p99=intervals[intervals.len()*99/100];
            let rate_floor=114.;let gap_limit=2000./120.;
            reports.push(json!({"scenario":scenario,"gesture":gesture,"duration_s":(end-start) as f64/1e6,"presentation_hz":hz,
                "presentation_gap_p99_ms":p99,"moving_updates":observed.len(),"presented_frames":presented.len(),
                "rate_floor_hz":rate_floor,"gap_p99_limit_ms":gap_limit,"rate_met":hz>=rate_floor,"gap_met":p99<=gap_limit,
                "observation":if scenario=="panel" {"native_allocation"} else {"authored_control"},
                "path_amplitude":if scenario=="panel" {[200.,100.]} else {[4.,20.]},
                "moving_positions":observed.iter().map(|(timestamp,position)|json!({"timestamp_us":timestamp,"position":position})).collect::<Vec<_>>()}));
        }
    }
    std::fs::write(output.join(if tilt {"tilt-motion.json"}else{"pressure-motion.json"}),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}

#[test]
#[ignore="private Mutter display and tablet proxy; workspace-motion.sh gtk --native-test=native_tilt_calibration --tablet"]
fn native_tilt_calibration() {
    let app=native_test_app("art.capycanvas.TiltCalibration");
    let mut native=RemoteInput::new();
    untouched_tilt_first_contact(&app,&mut native,&std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap()));
    let w=Workspace::with_project(&app,Some((new_drawing_at(256,256,SampleDepth::F32),None)));
    w.window.set_default_size(1100,800);w.window.maximize();w.window.present();ready(&w);
    set_transparency(&w, "LAYER_MOTION_TRANSPARENCY");
    let theme=if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|v|v=="dark"){Theme::Dark}else{Theme::Light};
    w.dispatch(UiAction::SetTheme{theme:Some(theme)});
    native.ready();
    let output=std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());std::fs::create_dir_all(&output).unwrap();
    let mut settings=state(&w).settings;settings.cursor=layer_ui::CursorMode::None;
    w.dispatch(UiAction::RestoreSettings{settings:Box::new(settings)});
    w.dispatch(UiAction::OpenSettings{page:SettingsPage::Input});pump(200);
    let adjust=find_named(w.preferences.dialog.upcast_ref(),"setting-pen-tilt-action").unwrap();super::histogram::scroll_to(&adjust);pump(100);
    native.click(screen_point(&adjust,&w.window,[0.5,0.5]));until(||!w.preferences.dialog.is_mapped(),"first tilt editor opens");pump(250);
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    assert!(!state(&w).tilt_calibration.unwrap().ready);
    for name in ["pen-tilt-graph","pen-tilt-firmer","pen-tilt-lighter","pen-tilt-reset","pen-tilt-cancel","pen-tilt-reverse-horizontal","pen-tilt-reverse-vertical","pen-tilt-swap-axes"] {
        assert!(find_named(panel,name).unwrap().is_sensitive(),"{name} works before first pen input");
    }
    let apply=find_named(panel,"pen-tilt-apply").unwrap();assert!(!apply.is_sensitive());
    let graph=find_named(panel,"pen-tilt-graph").unwrap();let at=graph_point_for(&w,&graph,[0.5,0.5],true);native.click(at);
    assert_eq!(response_view(&w,true).editor.points.len(),3);
    let more=find_named(panel,"pen-tilt-lighter").unwrap();native.click(screen_point(&more,&w.window,[0.5,0.5]));
    let reverse=find_named(panel,"pen-tilt-reverse-horizontal").unwrap();native.click(screen_point(&reverse,&w.window,[0.5,0.5]));
    let swap=find_named(panel,"pen-tilt-swap-axes").unwrap();native.click(screen_point(&swap,&w.window,[0.5,0.5]));
    assert!(state(&w).tilt_calibration.as_ref().unwrap().axes.reverse_horizontal);
    assert!(state(&w).tilt_calibration.as_ref().unwrap().axes.swap,"the icon bar preserves combined corrections");
    let reset=find_named(panel,"pen-tilt-reset").unwrap();native.click(screen_point(&reset,&w.window,[0.5,0.5]));
    assert_eq!(response_view(&w,true).editor.points,[[0.,0.],[1.,1.]]);assert_eq!(state(&w).tilt_calibration.unwrap().axes,Default::default());
    native.click(screen_point(&more,&w.window,[0.5,0.5]));let before=response_view(&w,true).editor.points;
    crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-before-pen.png"))).unwrap();
    pump(500);
    let canvas=window_point(&w,[60.,140.]);
    w.dispatch(UiAction::Invoke {command:CommandId::Lasso});pump(100);
    let bounds=panel.compute_bounds(&w.surface).unwrap();
    native.perform(json!([{"pen":"down","point":canvas,"tilt":[30.,0.]}]));
    crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-first-lasso-down.png"))).unwrap();
    assert!(panel.is_mapped(),"first Lasso canvas penDown keeps the tilt utility visible");
    assert_eq!(panel.compute_bounds(&w.surface).unwrap(),bounds,"first pen identification preserves native utility bounds");
    assert_eq!(response_view(&w,true).editor.points,before,"first Lasso penDown keeps the edited curve");
    native.perform(json!([{"pen":"up"},{"pen":"leave"}]));
    assert!(panel.is_mapped(),"first Lasso pen release keeps the tilt utility visible");
    assert!(apply.is_sensitive(),"first pen contact identifies the pen with the brush cursor hidden");
    assert_eq!(ui_session(&w).engine().metrics().committed_strokes,0,"no stroke is needed to activate the editor");
    native.click(screen_point(&more,&w.window,[0.5,0.5]));
    assert_ne!(response_view(&w,true).editor.points,before,"controls remain usable after first canvas contact");
    let cancel=find_named(panel,"pen-tilt-cancel").unwrap();native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
    assert!(state(&w).tilt_calibration.is_none());
    w.dispatch(UiAction::Invoke {command:CommandId::Pencil});pump(100);

    for device in ["mouse","touch","pen"] {
        w.dispatch(UiAction::OpenSettings{page:SettingsPage::Input});pump(200);
        let adjust=find_named(w.preferences.dialog.upcast_ref(),"setting-pen-tilt-action").unwrap();super::histogram::scroll_to(&adjust);pump(100);
        let at=screen_point(&adjust,&w.window,[0.5,0.5]);native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        until(||!w.preferences.dialog.is_mapped(),"tilt closes preferences");pump(200);
        let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();assert!(panel.is_mapped());
        assert!(state(&w).tilt_calibration.as_ref().unwrap().ready,"native tablet identity binds the editor");
        if device=="mouse" {measured_extent_recovers(&w,true);}
        assert_dialog_footer(panel,"pen-tilt");
        assert_axis_bar(panel,&w);
        let graph=find_named(panel,"pen-tilt-graph").unwrap();
        let less=find_named(panel,"pen-tilt-firmer").unwrap();let at=screen_point(&less,&w.window,[0.5,0.5]);
        native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        assert_eq!(response_view(&w,true).editor.points,[[0.,0.],[1.,0.975]],"{device} lowers sensitivity from neutral");
        let reverse=find_named(panel,"pen-tilt-reverse-horizontal").unwrap();let at=screen_point(&reverse,&w.window,[0.5,0.5]);
        native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        assert!(state(&w).tilt_calibration.unwrap().axes.reverse_horizontal);
        let at=graph_point_for(&w,&graph,[0.5,0.5],true);let to=graph_point_for(&w,&graph,[0.55,0.4],true);
        native.perform(json!([contact(device,"down",at),contact(device,"move",to),contact(device,"up",to)]));
        assert_eq!(response_view(&w,true).editor.points.len(),3,"{device} edits a shared curve control");
        let title=find_named(panel,"pen-tilt-title").unwrap();let at=screen_point(&title,&w.window,[0.5,0.5]);
        let before=response_view(&w,true).bounds;let to=[at[0]-100.,at[1]+20.];
        native.perform(json!([contact(device,"down",at),contact(device,"move",to),contact(device,"up",to)]));
        assert!(response_view(&w,true).bounds.x<before.x-70.,"{device} moves the tilt utility");
        native.perform(json!([{"pen":"move","point":canvas,"tilt":[30.,0.]}]));
        let tilt=state(&w).tilt_calibration.unwrap();assert!((tilt.input_direction.unwrap()[0]-1./3.).abs()<1e-5);
        assert!(tilt.adjusted_direction.unwrap()[0]<0.);
        let before=super::editing_tools::raster(&w);let pixels=super::editing_tools::pixels(&before);
        let to=window_point(&w,[100.,150.]);native.perform(json!([{"pen":"down","point":canvas,"tilt":[30.,0.]},{"pen":"move","point":to,"tilt":[30.,0.]},{"pen":"up"}]));
        super::editing_tools::committed(&w,&before);assert_ne!(super::editing_tools::pixels(&super::editing_tools::raster(&w)),pixels,"draw while editing tilt");
        crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-{device}.png"))).unwrap();
        record_layout(&w,panel,"pen-tilt",&output,theme);
        let cancel=find_named(panel,"pen-tilt-cancel").unwrap();native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
        assert!(state(&w).tilt_calibration.is_none());assert!(state(&w).settings.tilt_calibrations.is_empty());
    }
    w.dispatch(UiAction::Invoke{command:CommandId::PenTilt});pump(200);
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let more=find_named(panel,"pen-tilt-lighter").unwrap();native.click(screen_point(&more,&w.window,[0.5,0.5]));
    let chosen=response_view(&w,true).editor.points;
    let apply=find_named(panel,"pen-tilt-apply").unwrap();native.click(screen_point(&apply,&w.window,[0.5,0.5]));
    assert_eq!(state(&w).settings.tilt_calibrations.values().next().unwrap().response.points(),chosen);
    w.dispatch(UiAction::Invoke{command:CommandId::PenTilt});pump(200);
    let reset=find_named(panel,"pen-tilt-reset").unwrap();native.click(screen_point(&reset,&w.window,[0.5,0.5]));
    assert_eq!(response_view(&w,true).editor.points,[[0.,0.],[1.,1.]]);
    let close=find_named(panel,"pen-tilt-close").unwrap();native.click(screen_point(&close,&w.window,[0.5,0.5]));
    assert_eq!(state(&w).settings.tilt_calibrations.values().next().unwrap().response.points(),chosen);
    w.dispatch(UiAction::Invoke{command:CommandId::PenTilt});pump(200);
    let graph=find_named(panel,"pen-tilt-graph").unwrap();let tilt_plot=[graph.width(),graph.height()];graph.grab_focus();native.key(0xff1b);assert!(state(&w).tilt_calibration.is_none());
    w.dispatch(UiAction::Invoke{command:CommandId::PenPressure});pump(200);
    let pressure_graph=find_named(panel,"pen-pressure-graph").unwrap();
    assert_eq!([pressure_graph.width(),pressure_graph.height()],tilt_plot,"pressure and tilt reserve the same font-sized axis gutters");
    let cancel=find_named(panel,"pen-pressure-cancel").unwrap();native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
    utility_over_column(&w,&mut native,&output,theme,true);
    if std::env::var("LAYER_TILT_MOTION").is_ok_and(|v|v=="1") {
        w.dispatch(UiAction::Invoke{command:CommandId::PenTilt});pump(200);
        w.dispatch(UiAction::CurveEditor{target:layer_ui::CurveEditorTarget::Tilt,action:layer_ui::CurveEditorAction::Reset});
        let epoch=response_view(&w,true).editor.controls.epoch;
        for phase in [layer_ui::ContactPhase::Down,layer_ui::ContactPhase::Up]{
            w.dispatch(UiAction::CurveEditor{target:layer_ui::CurveEditorTarget::Tilt,action:layer_ui::CurveEditorAction::Contact{epoch,phase,point:[50.,50.],extent:[100.,100.]}});
        }
        pump(200);native.settle_ms=0;native.timeout=Duration::from_secs(30);measure_motion(&w,&mut native,&output,true);
    }
    tilt_dialog_preview(&app,&mut native,&output,theme);
    native.finish();w.window.close();
}

#[test]
#[ignore="private Mutter display and tablet proxy; LAYER_MOTION_VIEWPORT=640x480 workspace-motion.sh gtk --native-test=native_short_tilt_calibration --tablet"]
fn native_short_tilt_calibration() {
    let app=native_test_app("art.capycanvas.ShortTiltCalibration");
    let mut native=RemoteInput::new();native.ready();
    let w=Workspace::with_project(&app,Some((new_drawing_at(256,256,SampleDepth::F32),None)));
    w.window.maximize();w.window.present();ready(&w);
    let theme=if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|v|v=="dark"){Theme::Dark}else{Theme::Light};
    w.dispatch(UiAction::SetTheme{theme:Some(theme)});
    w.dispatch(UiAction::Invoke{command:CommandId::PenTilt});pump(250);
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    assert_dialog_footer(panel,"pen-tilt");assert_axis_bar(panel,&w);
    let before=artwork_history(&w);
    for name in ["reverse-horizontal","reverse-vertical","swap-axes"] {
        let button=find_named(panel,&format!("pen-tilt-{name}")).unwrap().downcast::<gtk::ToggleButton>().unwrap();
        let at=screen_point(button.upcast_ref(),&w.window,[0.5,0.5]);
        native.perform(json!([contact("pen","down",at),contact("pen","up",at)]));
        assert!(button.is_active(),"short viewport pen activates {name}");
    }
    let lighter=find_named(panel,"pen-tilt-lighter").unwrap();let at=screen_point(&lighter,&w.window,[0.5,0.5]);
    native.perform(json!([contact("pen","down",at),contact("pen","up",at)]));
    assert_ne!(response_view(&w,true).editor.points,[[0.,0.],[1.,1.]],"short viewport pen changes sensitivity");
    assert_eq!(artwork_history(&w),before,"utility pen controls leave artwork unchanged");
    let output=std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());std::fs::create_dir_all(&output).unwrap();
    crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-short.png"))).unwrap();
    native.finish();w.window.close();pump(100);
}

fn assert_dialog_footer(panel:&gtk::Widget,prefix:&str) {
    let buttons=["reset","cancel","apply"].map(|name|find_named(panel,&format!("{prefix}-{name}")).unwrap());
    let footer=buttons[0].parent().unwrap();assert!(footer.has_css_class("dialog-actions"));
    assert!(!buttons[0].has_css_class("flat"));
    let reset_bounds=buttons[0].compute_bounds(panel).unwrap();
    let title=find_named(panel,&format!("{prefix}-title")).unwrap().compute_bounds(panel).unwrap();
    let sensitivity=find_named(panel,&format!("{prefix}-firmer")).unwrap().compute_bounds(panel).unwrap();
    let apply=buttons[2].compute_bounds(panel).unwrap();
    assert!((reset_bounds.x()-title.x()).abs()<1.5,"title and footer share an inset");
    assert!((panel.width() as f32-apply.x()-apply.width()-16.).abs()<1.5,"balanced footer end inset");
    let graph=find_named(panel,&format!("{prefix}-graph")).unwrap().compute_bounds(panel).unwrap();
    assert!((graph.width()-graph.height()).abs()<=1.,"equal input/output scales use a square plot: {graph:?}");
    assert!((graph.x()-(panel.width() as f32-graph.x()-graph.width())).abs()<1.5,"axis gutters keep the plot centered");
    let close=find_named(panel,&format!("{prefix}-close")).unwrap().compute_bounds(panel).unwrap();
    assert!((panel.width() as f32-close.x()-close.width()-16.).abs()<1.5,"the header shares the same end inset");
    assert!((title.y()+title.height()*0.5-close.y()-close.height()*0.5).abs()<1.,"title and Close share a vertical center");
    assert!((sensitivity.height()-reset_bounds.height()).abs()<1.,"editing and footer actions use the same height");
    let bounds=buttons.each_ref().map(|button|button.compute_bounds(&footer).unwrap());
    assert!(bounds.iter().all(|b|b.height()==TILE_SIZE),"tile-height dialog buttons: {bounds:?}");
    assert!(bounds.iter().all(|b|(b.y()-bounds[0].y()).abs()<1.));
    assert!((bounds[2].x()-bounds[1].x()-bounds[1].width()-8.).abs()<1.);
    let reset=buttons[0].clone().downcast::<gtk::Button>().unwrap();assert!(reset.child().is_some_and(|c|c.is::<gtk::Label>()));
}

fn assert_axis_bar(panel:&gtk::Widget,w:&Workspace) {
    let bar=find_named(panel,"pen-tilt-axes").unwrap();assert!(bar.has_css_class("linked"));
    assert!(bar.has_css_class("selection-modes") && !bar.has_css_class("dialog-actions"));
    let buttons=["reverse-horizontal","reverse-vertical","swap-axes"].map(|name|find_named(panel,&format!("pen-tilt-{name}")).unwrap().downcast::<gtk::ToggleButton>().unwrap());
    let view=state(w).tilt_calibration.unwrap();
    for (button,label) in buttons.iter().zip([view.reverse_horizontal,view.reverse_vertical,view.swap_axes]) {
        assert!(button.child().is_some_and(|c|c.is::<gtk::Image>()));assert_eq!(button.tooltip_text().as_deref(),Some(label.as_str()));
        assert_eq!(button.child().unwrap().downcast::<gtk::Image>().unwrap().pixel_size(),16);
    }
    let preview=find_named(panel,"pen-tilt-direction").unwrap();
    let preview_bounds=preview.compute_bounds(panel).unwrap();let bar_bounds=bar.compute_bounds(panel).unwrap();
    let graph=find_named(panel,"pen-tilt-graph").unwrap().compute_bounds(panel).unwrap();
    let placed=panel.compute_bounds(&w.surface).unwrap();
    assert!(placed.x()>=-1. && placed.y()>=-1. && placed.x()+placed.width()<=w.surface.width() as f32+1. && placed.y()+placed.height()<=w.surface.height() as f32+1.,"utility stays inside the viewport: {placed:?}");
    let footer=find_named(panel,"pen-tilt-reset").unwrap().compute_bounds(panel).unwrap();
    assert!(bar_bounds.y()>=graph.y()+graph.height() && bar_bounds.y()+bar_bounds.height()<=footer.y(),"direction controls stay visible between plot and footer");
    assert!(preview_bounds.x()>=graph.x()+graph.width()*0.5 && preview_bounds.y()>=graph.y()+graph.height()*0.5,"direction preview overlays the lower-right plot quadrant");
    assert!(preview_bounds.x()+preview_bounds.width()<=graph.x()+graph.width()+1. && preview_bounds.y()+preview_bounds.height()<=graph.y()+graph.height()+1.,"direction preview stays inside the graph");
    assert!((graph.x()+graph.width()-preview_bounds.x()-preview_bounds.width()-8.).abs()<1. && (graph.y()+graph.height()-preview_bounds.y()-preview_bounds.height()-8.).abs()<1.,"direction preview keeps an eight-pixel plot inset");
    let sensitivity=find_named(panel,"pen-tilt-firmer").unwrap().compute_bounds(panel).unwrap();
    assert!((bar_bounds.y()-sensitivity.y()).abs()<1.,"sensitivity and direction controls share a compact row");
    let labels=["pen-tilt-sensitivity-title","pen-tilt-direction-title"].map(|name|find_named(panel,name).unwrap().downcast::<gtk::Label>().unwrap());
    let text=labels.each_ref().map(|label| {
        let bounds=label.compute_bounds(panel).unwrap();
        let width=label.layout().pixel_size().0 as f32;
        assert!((bounds.width()-width).abs()<1.,"compact label keeps its intrinsic text width");
        [bounds.x()+label.layout_offsets().0 as f32,width]
    });
    let lighter=find_named(panel,"pen-tilt-lighter").unwrap().compute_bounds(panel).unwrap();
    assert!([&sensitivity,&lighter].iter().all(|bounds|bounds.width()==TILE_SIZE && bounds.height()==TILE_SIZE),"sensitivity controls keep square tile targets");
    assert!((sensitivity.x()-text[0][0]-text[0][1]-4.).abs()<1.,"Sensitivity text stays four pixels from its bar");
    assert!((bar_bounds.x()-text[1][0]-text[1][1]-4.).abs()<1.,"Direction text stays four pixels from its bar");
    assert!(text[1][0]-lighter.x()-lighter.width()>=7.,"compact groups stay at least eight pixels apart");
    assert!((text[0][0]-16.).abs()<1.,"Sensitivity starts at the left content gutter");
    assert!((panel.width() as f32-bar_bounds.x()-bar_bounds.width()-16.).abs()<1.,"Direction ends at the right content gutter");
    assert!(buttons.iter().all(|button|button.width()==TILE_SIZE as i32 && button.height()==TILE_SIZE as i32));
    let bounds=buttons.each_ref().map(|button|button.compute_bounds(&bar).unwrap());
    assert!(bounds.windows(2).all(|pair|(pair[0].x()+pair[0].width()-pair[1].x()).abs()<1.));
}

fn artwork_history(w:&Workspace)->(u64,bool,bool,u64) {
    let session=ui_session(w);let engine=session.engine();
    (engine.document().revision,engine.can_undo(),engine.can_redo(),engine.metrics().committed_strokes)
}

fn measured_extent_recovers(w:&Rc<Workspace>,tilt:bool) {
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let original_bounds=response_view(w,tilt).bounds;
    let dispatch=|action| w.dispatch(if tilt {UiAction::TiltCalibration {action:layer_ui::TiltCalibrationAction::Panel {action}}}else{UiAction::PressureCalibration {action}});
    let css = gtk::CssProvider::new();
    css.load_from_string(".painted-calibration-panel { background-color: #ff00ff; background-image: none; }");
    panel.add_css_class("painted-calibration-panel");
    gtk::style_context_add_provider_for_display(&w.surface.display(), &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    assert!(idle_paint(panel).color_bounds(&w.window, [255, 0, 255, 255]).is_some(),
        "calibration panel paints before resizing");
    let maximized = w.window.is_maximized();
    let previous = [w.window.width(), w.window.height()];
    let frames = sample_after_paint(panel, glib::clone!(#[weak] w, #[upgrade_or_default] move ||
        ([w.surface.width(), w.surface.height()], w.pressure_calibration.root.compute_bounds(&w.surface))));
    w.window.unmaximize();
    w.window.set_default_size(900, 650);
    pump(300);
    let frames = frames.finish();
    let size = [w.surface.width(), w.surface.height()];
    assert_ne!(size, previous, "calibration resize changes the native viewport");
    let final_bounds = frames.last().and_then(|frame| frame.color_bounds(&w.window, [255, 0, 255, 255]))
        .expect("resized calibration panel paints its marked background");
    let placement = panel.compute_bounds(&w.surface).unwrap();
    assert!(placement.x() >= 0. && placement.y() >= 0.
        && placement.x() + placement.width() <= size[0] as f32 + 1.
        && placement.y() + placement.height() <= size[1] as f32 + 1., "resized calibration fits its viewport");
    for frame in frames.iter().filter(|frame| frame.state.0 == size) {
        assert_eq!(frame.color_bounds(&w.window, [255, 0, 255, 255]), Some(final_bounds),
            "calibration real resize paints its measured extent immediately: frame {}, viewport {size:?}", frame.time);
        assert_eq!(frame.state.1, Some(placement), "painted calibration uses its current viewport placement: frame {}", frame.time);
    }
    if maximized { w.window.maximize(); } else { w.window.set_default_size(previous[0], previous[1]); }
    pump(300);
    let native=panel.compute_bounds(&w.surface).unwrap();let view=response_view(w,tilt);
    let action=layer_ui::PressureCalibrationAction::Measure {
        extent:[native.width()-8.,native.height()-8.],viewport:[w.surface.width() as f32,w.surface.height() as f32],
    };
    let reference = idle_paint(panel).color_bounds(&w.window, [255, 0, 255, 255])
        .expect("calibration panel paints its marked background");
    let frames = sample_after_paint(panel, || ());
    dispatch(action);
    pump(300);
    let frames = frames.finish();
    assert!(!frames.is_empty(), "calibration remeasurement paints a frame");
    for frame in frames {
        let bounds = frame.color_bounds(&w.window, [255, 0, 255, 255]);
        assert_eq!(bounds, Some(reference), "calibration remeasurement preserves its painted extent in frame {}", frame.time);
    }
    panel.remove_css_class("painted-calibration-panel");
    gtk::style_context_remove_provider_for_display(&w.surface.display(), &css);
    assert!(panel.is_mapped(),"cached native extent heals a mismatched shared measurement");
    assert_eq!(panel.compute_bounds(&w.surface).unwrap(),native,"healing preserves native extent and placement");
    assert_eq!(response_view(w,tilt).bounds,view.bounds,"native remeasurement restores the actual shared extent");
    assert_eq!(response_view(w,tilt).editor.points,view.editor.points);
    let position=[original_bounds.x,original_bounds.y];
    let viewport=[w.surface.width() as f32,w.surface.height() as f32];
    for (phase,position) in [(ContactPhase::Down,[view.bounds.x,view.bounds.y]),(ContactPhase::Move,position),(ContactPhase::Up,position)] {
        dispatch(layer_ui::PressureCalibrationAction::Drag {phase,position,viewport});
    }
    pump(100);
    assert_eq!(response_view(w,tilt).bounds,original_bounds,"resize probe restores the utility placement");
}

fn untouched_tilt_first_contact(app:&NativeTestApp,native:&mut RemoteInput,output:&std::path::Path) {
    let w=Workspace::with_project(app,Some((new_drawing_at(256,256,SampleDepth::F32),None)));
    w.window.maximize();w.window.present();ready(&w);native.ready();
    let theme=if std::env::var("CAPY_NATIVE_TEST_THEME").is_ok_and(|v|v=="dark"){Theme::Dark}else{Theme::Light};
    w.dispatch(UiAction::SetTheme {theme:Some(theme)});
    let mut settings=state(&w).settings;settings.cursor=layer_ui::CursorMode::None;
    w.dispatch(UiAction::RestoreSettings {settings:Box::new(settings)});
    w.dispatch(UiAction::Invoke {command:CommandId::Lasso});
    w.dispatch(UiAction::Invoke {command:CommandId::PenTilt});pump(250);
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let title=find_named(panel,"pen-tilt-title").unwrap();let at=screen_point(&title,&w.window,[0.5,0.5]);
    native.perform(json!([{"point":at,"down":true},{"point":[at[0]-80.,at[1]+20.]},{"down":false}]));
    let native_bounds=panel.compute_bounds(&w.surface).unwrap();let view=response_view(&w,true);
    assert!(!state(&w).tilt_calibration.unwrap().ready);
    native.perform(json!([{"pen":"down","point":window_point(&w,[40.,180.]),"tilt":[30.,0.]}]));
    std::fs::create_dir_all(output).unwrap();
    crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-untouched-first-lasso-down.png"))).unwrap();
    assert!(panel.is_mapped(),"untouched tilt utility survives first Lasso canvas penDown");
    assert_eq!(panel.compute_bounds(&w.surface).unwrap(),native_bounds,"first device identification keeps measured native placement");
    assert_eq!(response_view(&w,true).bounds,view.bounds,"first device identification keeps shared placement");
    assert_eq!(response_view(&w,true).editor.points,view.editor.points);
    native.perform(json!([{"pen":"up"},{"pen":"leave"}]));
    let more=find_named(panel,"pen-tilt-lighter").unwrap();native.click(screen_point(&more,&w.window,[0.5,0.5]));
    assert_ne!(response_view(&w,true).editor.points,view.editor.points,"surviving utility remains editable");
    let cancel=find_named(panel,"pen-tilt-cancel").unwrap();native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
    w.dispatch(UiAction::Invoke {command:CommandId::PenTilt});pump(250);
    assert!(panel.is_mapped() && state(&w).tilt_calibration.as_ref().unwrap().ready,"utility reopens after first canvas contact");
    assert_eq!(ui_session(&w).engine().metrics().committed_strokes,0);
    w.window.close();pump(100);
}

fn tilt_dialog_preview(app:&NativeTestApp,native:&mut RemoteInput,output:&std::path::Path,theme:Theme) {
    native.settle_ms=100;native.timeout=Duration::from_secs(5);
    let w=Workspace::with_project(app,Some((new_drawing_at(256,256,SampleDepth::F32),None)));
    w.window.maximize();w.window.present();ready(&w);w.dispatch(UiAction::SetTheme {theme:Some(theme)});
    let mut settings=state(&w).settings;settings.cursor=layer_ui::CursorMode::None;settings.tilt_calibrations.clear();
    w.dispatch(UiAction::RestoreSettings {settings:Box::new(settings)});
    for bound in [false,true] {
        w.dispatch(UiAction::Invoke {command:CommandId::PenTilt});pump(250);
        let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
        assert_eq!(state(&w).tilt_calibration.as_ref().unwrap().ready,bound);
        let bounds=panel.compute_bounds(&w.surface).unwrap();let history=artwork_history(&w);
        let draft=response_view(&w,true).editor.points;
        let circle=find_named(panel,"pen-tilt-direction").unwrap();
        let at=screen_point(&circle,&w.window,[0.5,0.5]);
        native.perform(json!([{"pen":"move","point":at,"tilt":[30.,0.]}]));
        assert!(panel.is_mapped(),"dialog hover keeps tilt mapped");
        assert_eq!(panel.compute_bounds(&w.surface).unwrap(),bounds,"dialog identification preserves placement");
        let view=state(&w).tilt_calibration.unwrap();assert!(view.ready);
        assert!((view.input_direction.unwrap()[0]-1./3.).abs()<1e-5,"circle hover senses native tilt");
        native.perform(json!([{"pen":"down","point":at,"tilt":[0.,40.]},{"pen":"move","point":[at[0]+4.,at[1]+4.],"tilt":[0.,60.]}]));
        let view=state(&w).tilt_calibration.unwrap();
        assert!((view.input_direction.unwrap()[1]-2./3.).abs()<1e-5,"circle contact updates native tilt");
        assert!(view.adjusted_direction.is_some() && view.panel.editor.marker.is_some());
        crate::snapshot(&w).save_to_png(output.join(format!("tilt-{theme:?}-dialog-preview-bound-{bound}.png"))).unwrap();
        native.perform(json!([{"pen":"up"},{"pen":"leave"}]));
        let title=find_named(panel,"pen-tilt-title").unwrap();let at=screen_point(&title,&w.window,[0.5,0.5]);
        native.perform(json!([{"pen":"move","point":at,"tilt":[-30.,0.]}]));
        assert!((state(&w).tilt_calibration.unwrap().input_direction.unwrap()[0]+1./3.).abs()<1e-5,"dialog header hover senses tilt");
        native.perform(json!([{"pen":"leave"}]));
        assert_eq!(response_view(&w,true).editor.points,draft,"sensing keeps the response draft");
        assert_eq!(artwork_history(&w),history,"dialog tilt preview never paints or changes document history");
        let apply=find_named(panel,"pen-tilt-apply").unwrap();assert!(apply.is_sensitive());native.click(screen_point(&apply,&w.window,[0.5,0.5]));
        assert!(state(&w).tilt_calibration.is_none());
    }
    w.window.close();pump(100);
}

fn utility_over_column(w:&Rc<Workspace>,native:&mut RemoteInput,output:&std::path::Path,theme:Theme,tilt:bool) {
    let saved=state(w).workspace;
    let group=saved.layout.panel_group(Panel::Layers).unwrap();
    let column=saved.layout.column_for_group(group).unwrap();
    w.customize(CustomizationAction::SetColumnCollapsed {group,collapsed:true});
    enable_individual_column_panels(w,group);pump(200);
    let icon=find_named(w.surface.upcast_ref(),"column-icon-Layers").unwrap();
    native.click(screen_point(&icon,&w.window,[0.5,0.5]));
    let drawer=find_named(w.surface.upcast_ref(),&format!("column-drawer-{column}")).unwrap();
    until(||drawer.is_mapped(),"collapsed Layers column expands");pump(200);
    let drawer_bounds=drawer.compute_bounds(&w.surface).unwrap();
    w.dispatch(UiAction::Invoke {command:if tilt {CommandId::PenTilt}else{CommandId::PenPressure}});pump(250);
    let prefix=if tilt {"pen-tilt"}else{"pen-pressure"};
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let initial=panel.compute_bounds(&w.surface).unwrap();let area=w.resolved().work_area;
    crate::snapshot(w).save_to_png(output.join(format!("{prefix}-{theme:?}-clear-canvas.png"))).unwrap();
    let title=find_named(panel,&format!("{prefix}-title")).unwrap();
    let start=screen_point(&title,&w.window,[0.5,0.5]);
    let origin=[drawer_bounds.x()-80.,(drawer_bounds.y()+8.).min(w.surface.height() as f32-panel.height() as f32-16.)];
    let end=[start[0]+origin[0]-initial.x(),start[1]+origin[1]-initial.y()];
    native.perform(json!([{"point":start,"down":true},{"point":end},{"down":false}]));pump(200);
    let actual=panel.compute_bounds(&w.surface).unwrap();
    crate::snapshot(w).save_to_png(output.join(format!("{prefix}-{theme:?}-over-column.png"))).unwrap();
    std::fs::write(output.join(format!("{prefix}-{theme:?}-column-placement.json")),serde_json::to_vec_pretty(&json!({
        "initial":{"x":initial.x(),"y":initial.y(),"width":initial.width(),"height":initial.height()},
        "work_area":area,"drawer":{"x":drawer_bounds.x(),"y":drawer_bounds.y(),"width":drawer_bounds.width(),"height":drawer_bounds.height()},
        "overlap":{"x":actual.x(),"y":actual.y(),"width":actual.width(),"height":actual.height()},
    })).unwrap()).unwrap();
    assert!(drawer.is_mapped(),"utility keeps the expanded column open");
    for target in [&title,&find_named(panel,&format!("{prefix}-lighter")).unwrap()] {
        let at=screen_point(target,&w.window,[0.5,0.5]);
        let b=target.compute_bounds(&w.surface).unwrap();
        assert!(b.x()+b.width()*0.5>drawer_bounds.x() && b.x()+b.width()*0.5<drawer_bounds.x()+drawer_bounds.width() && b.y()+b.height()*0.5>drawer_bounds.y() && b.y()+b.height()*0.5<drawer_bounds.y()+drawer_bounds.height(),"utility target overlaps expanded column");
        let hit=w.window.pick(f64::from(at[0]),f64::from(at[1]),gtk::PickFlags::DEFAULT).unwrap();
        assert!(hit==*target || hit.is_ancestor(target),"{prefix} stays above expanded column at {at:?}: hit {}",hit.widget_name());
    }
    let with_drawer=crate::snapshot(w);
    w.customize(CustomizationAction::CloseExpanded);pump(200);
    let without_drawer=crate::snapshot(w);
    without_drawer.save_to_png(output.join(format!("{prefix}-{theme:?}-column-hidden.png"))).unwrap();
    assert_eq!(panel.compute_bounds(&w.surface).unwrap(),actual,"closing column keeps the manually placed utility");
    let width=with_drawer.width() as usize;let height=with_drawer.height() as usize;
    assert_eq!((width,height),(without_drawer.width() as usize,without_drawer.height() as usize));
    let mut front=vec![0;width*height*4];let mut reference=vec![0;width*height*4];
    with_drawer.download(&mut front,width*4);without_drawer.download(&mut reference,width*4);
    let b=panel.compute_bounds(&w.window).unwrap();let scale=width as f32/w.window.width() as f32;
    let mut different=Vec::new();
    for y in ((b.y()+8.)*scale).ceil() as usize..((b.y()+b.height()-8.)*scale).floor() as usize {
        for x in ((b.x()+8.)*scale).ceil() as usize..((b.x()+b.width()-8.)*scale).floor() as usize {
            let i=(y*width+x)*4;
            if front[i..i+4]!=reference[i..i+4] {different.push([x,y]);}
        }
    }
    assert!(different.is_empty(),"expanded column cannot paint or clip the opaque utility: {} changed pixels, first {:?}",different.len(),different.first());
    w.customize(CustomizationAction::ToggleColumnDrawer {group,panel:Panel::Layers});pump(200);
    let before=response_view(w,tilt).editor.points;
    let lighter=find_named(panel,&format!("{prefix}-lighter")).unwrap();native.click(screen_point(&lighter,&w.window,[0.5,0.5]));
    assert_ne!(response_view(w,tilt).editor.points,before,"utility receives native clicks above expanded content");
    assert!(initial.x()>=area.x+15. && initial.y()>=area.y+15. && initial.x()+initial.width()<=area.x+area.width-15. && initial.y()+initial.height()<=area.y+area.height-15.,"default utility stays inset in clear canvas: {initial:?}, {area:?}");
    assert!(initial.x()+initial.width()<=drawer_bounds.x() || initial.x()>=drawer_bounds.x()+drawer_bounds.width() || initial.y()+initial.height()<=drawer_bounds.y() || initial.y()>=drawer_bounds.y()+drawer_bounds.height(),"default utility avoids expanded column: {initial:?}, {drawer_bounds:?}");
    let cancel=find_named(panel,&format!("{prefix}-cancel")).unwrap();native.click(screen_point(&cancel,&w.window,[0.5,0.5]));
    w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(saved)});pump(200);
}

fn record_layout(w:&Workspace,panel:&gtk::Widget,prefix:&str,output:&std::path::Path,theme:Theme) {
    let mut widgets=serde_json::Map::new();
    for suffix in ["title","close","graph","input-axis","output-axis","firmer","lighter","reset","cancel","apply","device","direction-title","direction","axes"] {
        if let Some(widget)=find_named(panel,&format!("{prefix}-{suffix}")) && widget.is_mapped() {
            let b=widget.compute_bounds(panel).unwrap();widgets.insert(suffix.into(),json!({"x":b.x(),"y":b.y(),"width":b.width(),"height":b.height()}));
        }
    }
    let view=response_view(w,prefix=="pen-tilt");
    std::fs::write(output.join(format!("{prefix}-{theme:?}-layout.json")),serde_json::to_vec_pretty(&json!({"width":panel.width(),"height":panel.height(),"shared_bounds":view.bounds,"widgets":widgets})).unwrap()).unwrap();
}
