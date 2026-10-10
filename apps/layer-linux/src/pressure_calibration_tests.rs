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
        native.perform(json!([contact(device,"down",at),contact(device,"up",at)]));
        until(|| !w.preferences.dialog.is_mapped(),"preferences closes before calibration");pump(250);
        let panel = w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
        assert!(panel.is_mapped()); assert!(!state(&w).settings_open);
        assert_eq!(state(&w).pressure_calibration.unwrap().editor.points.len(),3);
        let snapshot=gtk::Snapshot::new();w.surface.snapshot_child(panel,&snapshot);
        let mut glass=Vec::new();crate::glass::collect(&snapshot.to_node().unwrap(),&state(&w).palette.glass.surfaces(),&mut glass);
        assert!(glass.is_empty(),"opaque utility owns its background: {glass:?}");
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
        let close=find_named(panel,"pen-pressure-close").unwrap(); let header=title.parent().unwrap();
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
    if std::env::var("LAYER_PRESSURE_MOTION").is_ok_and(|value|value=="1") {
        w.dispatch(UiAction::Invoke{command:CommandId::PenPressure});pump(300);
        w.dispatch(UiAction::CurveEditor {target:layer_ui::CurveEditorTarget::Pressure,action:layer_ui::CurveEditorAction::Reset});pump(300);
        native.settle_ms=0;native.timeout=Duration::from_secs(30);
        measure_motion(&w,&mut native,&output);
    }
    native.finish();w.window.close();
}

fn graph_point(w: &Workspace, graph: &gtk::Widget, point: [f32;2]) -> [f32;2] {
    let inset=state(w).pressure_calibration.unwrap().editor.controls.inset;
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

fn measure_motion(w: &Rc<Workspace>, input: &mut RemoteInput, output: &std::path::Path) {
    let mut reports=Vec::new();
    let panel=w.pressure_calibration.root.upcast_ref::<gtk::Widget>();
    let title=find_named(panel,"pen-pressure-title").unwrap();
    let bounds=state(w).pressure_calibration.unwrap().bounds;
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
            let target=find_named(panel,if scenario=="panel" {"pen-pressure-title"} else {"pen-pressure-graph"}).unwrap();
            let view=state(w).pressure_calibration.unwrap();
            let point=if scenario=="panel" {screen_point(&target,&w.window,[0.5,0.5])}
                else {let p=view.editor.points[1];graph_point(w,&target,[p[0],1.-p[1]])};
            input.perform(json!([{"point":point},{"wait_ms":100}]));
            let hit=w.window.pick(f64::from(point[0]),f64::from(point[1]),gtk::PickFlags::DEFAULT);
            assert!(hit.as_ref().is_some_and(|v|v==&target || v.is_ancestor(&target)),"{scenario} motion target hit {:?} at {point:?}",hit.map(|v|v.widget_name()));
            let current=move |w:&Workspace| {
                if scenario=="panel" {
                    let b=w.pressure_calibration.root.compute_bounds(&w.surface).unwrap();
                    return [b.x(),b.y()];
                }
                let gpu=w.gpu.borrow();let v=gpu.as_ref().unwrap().session.state().pressure_calibration.as_ref().unwrap();
                v.editor.points[1]
            };
            let observed=Rc::new(RefCell::new(Vec::new()));let observations=observed.clone();let owner=w.clone();let mut previous=current(w);
            let initial_native=previous;
            let shared=Rc::new(RefCell::new(Vec::new()));let shared_observations=shared.clone();let mut previous_shared=view.bounds;
            let observer=glib::timeout_add_local(Duration::from_millis(1),move || {
                let value=current(&owner);
                if value!=previous {observations.borrow_mut().push((glib::monotonic_time(),value));previous=value;}
                if scenario=="panel" {
                    let gpu=owner.gpu.borrow();let value=gpu.as_ref().unwrap().session.state().pressure_calibration.as_ref().unwrap().bounds;
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
                std::fs::write(output.join("pressure-motion-failure.json"),serde_json::to_vec_pretty(&json!({
                    "scenario":scenario,"gesture":gesture,"moving_updates":observed.len(),
                    "initial_native_position":initial_native,"initial_shared_bounds":view.bounds,
                    "moving_positions":observed.iter().map(|(timestamp,position)|json!({"timestamp_us":timestamp,"position":position})).collect::<Vec<_>>(),
                    "shared_positions":shared.borrow().iter().map(|(timestamp,bounds)|json!({"timestamp_us":timestamp,"bounds":bounds})).collect::<Vec<_>>(),
                    "shared_bounds":state(w).pressure_calibration.unwrap().bounds,
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
    std::fs::write(output.join("pressure-motion.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
}
