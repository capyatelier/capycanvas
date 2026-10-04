use super::*;
use super::new_photo::ready;
use super::photo_edit::{document, shown, window_point};
use super::histogram::{choose, scroll_to};
use layer_core::{EffectValue, color::{ColorProfile, RgbSpace, SampleDepth, source::*}, levels::CalibrationRole};
use layer_ui::{CanvasBarKind, EffectAction};
use serde_json::json;

fn fixture() -> layer_core::Project {
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut source = SourceBuilder::new([256, 256], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 1024 * 1024).unwrap();
    let row: Vec<u8> = (0..256).flat_map(|x| {
        let value = 35 + x * 170 / 255;
        [value as u8, (value + 15) as u8, (value + 30) as u8, 255]
    }).collect();
    for _ in 0..256 {source.push_row(&row).unwrap();}
    project.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));project
}

fn start(app: &NativeTestApp, effect: &str) -> (Rc<Workspace>, std::path::PathBuf) {
    let w = Workspace::with_project(app, Some((fixture(), None)));
    w.window.maximize();w.window.present();ready(&w);
    assert!(matches!(w.window.width(), 640 | 1100));
    for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible {panel, visible:false});
    }
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true});
    w.dispatch(UiAction::Customize {action:CustomizationAction::CloseExpanded});
    w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[w.window.width() as f32,800.]});
    w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}});ready(&w);
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});pump(200);
    let output = artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk");std::fs::create_dir_all(&output).unwrap();
    (w,output.into())
}

fn action_button(w: &Workspace, input: &mut RemoteInput, accepts: impl Fn(&EffectAction)->bool) -> gtk::Button {
    let action = state(w).layer_properties.actions.iter().find(|action| accepts(&action.action)).unwrap().clone();let label = action.label;
    if action.group.is_some() {let menu=named::<gtk::MenuButton>(w.effects.properties.upcast_ref(),"property-picker-menu");scroll_to(menu.upcast_ref());input.click(screen_point(menu.upcast_ref(),&w.window,[0.5,0.5]));until(|| menu.popover().is_some_and(|p|p.is_mapped()),"native grouped picker choices");}
    let button = widgets(w.effects.properties.upcast_ref()).filter_map(|widget|widget.downcast::<gtk::Button>().ok())
        .find(|button|button.is_mapped() && button.widget_name()=="property-picker" && (button.label().as_deref()==Some(label.as_str()) || button.tooltip_text().as_deref()==Some(label.as_str()))).unwrap();
    scroll_to(button.upcast_ref());button
}

fn canvas_point(w: &Workspace, at: [f32;2]) -> [f32;2] {
    let point = window_point(w,at);
    let hit = w.window.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
    assert!(hit==w.area || hit.is_ancestor(&w.area),"native contact hits canvas, got {}",hit.widget_name());point
}

fn sample(input: &mut RemoteInput, kind: &str, point: [f32;2]) {
    match kind {
        "mouse"=>input.click(point),
        "pen"=>input.perform(json!([{"pen":"down","point":point},{"pen":"up"},{"pen":"leave"}])),
        _=>input.perform(json!([{"touch":"down","point":point},{"wait_ms":700},{"touch":"up"}])),
    }
}

fn assert_source_unchanged(w: &Workspace, before: &layer_core::Document) {
    let after = document(w);
    for old in &before.layers {
        let new = after.layer(old.id).unwrap();assert_eq!(new.source,old.source);assert_eq!(new.raster.identity(),old.raster.identity());
        if old.id!=before.active_layer {assert_eq!(new,old);}
    }
}

fn assert_persisted(w: &Rc<Workspace>, path: &std::path::Path) {
    std::fs::write(path, super::place_source::snapshot(w)).unwrap();
    let reopened = layer_core::Project::read(std::fs::File::open(path).unwrap(), Default::default()).unwrap();
    let current = document(w);
    assert_eq!(reopened.document.layer(current.active_layer).unwrap().effect, current.layer(current.active_layer).unwrap().effect, "saved adjustment values reopen exactly");
    for layer in &current.layers {assert_eq!(reopened.document.layer(layer.id).unwrap().source, layer.source);}
}

fn tonal_calibration(effect: &str) {
    let app = native_test_app("art.capycanvas.TonalControls");
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(30);
    let (w,output) = start(&app,effect);input.ready();
        let width=w.window.width();
        for theme in [Theme::Light,Theme::Dark] {
            if std::env::var("LAYER_TONAL_THEME").is_ok_and(|wanted| wanted != format!("{theme:?}")) {continue;}
            w.dispatch(UiAction::SetTheme {theme:Some(theme)});pump(150);
            if effect=="levels" {
                let before=document(&w);let foreground=state(&w).colors.clone();let pixel=shown(&w,[128.,128.]);
                let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::AutoLevels {..}));
                crate::snapshot(&w).save_to_png(output.join(format!("levels-actions-{width}-{theme:?}.png"))).unwrap();
                input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
                until(||document(&w).layers!=before.layers,"native Auto Levels publishes correction");ready(&w);
                assert_source_unchanged(&w,&before);assert_eq!(state(&w).colors,foreground);pump(200);assert_ne!(shown(&w,[128.,128.]),pixel);
                crate::snapshot(&w).save_to_png(output.join(format!("levels-auto-{width}-{theme:?}.png"))).unwrap();
                assert_persisted(&w,&output.join(format!("levels-auto-{width}-{theme:?}.capy")));
                w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).layers,before.layers,"one Undo restores Auto Levels");
            }
            for role in [CalibrationRole::Black,CalibrationRole::Gray,CalibrationRole::White] {
                if std::env::var("LAYER_TONAL_ROLE").is_ok_and(|wanted|wanted!=format!("{role:?}")) {continue;}
                for kind in ["mouse","pen","touch"] {
                    if std::env::var("LAYER_TONAL_CONTACT").unwrap_or_else(|_|"mouse".into())!=kind {continue;}
                    if kind!="mouse" {assert!(std::env::var("LAYER_TONAL_ROLE").is_ok() && std::env::var("LAYER_TONAL_THEME").is_ok(),"tablet popup journeys require one role/theme per private input stream");}
                    let before=document(&w);let foreground=state(&w).colors.clone();let tool=state(&w).layer_tools.tool;
                    let at=[match role {CalibrationRole::Black=>30.,CalibrationRole::White=>220.,_=>128.},128.];
                    let pixel=shown(&w,at);
                    let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::Calibrate {role:current,..} if *current==role));
                    input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));input.key(0xff1b);assert_eq!(document(&w).layers,before.layers);assert_eq!(state(&w).colors,foreground);
                    let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::Calibrate {role:current,..} if *current==role));
                    input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
                    until(||state(&w).canvas_bar.as_ref().is_some_and(|bar|bar.context.kind==CanvasBarKind::Picker),"calibration picker armed");
                    sample(&mut input,kind,canvas_point(&w,at));
                    until(||state(&w).canvas_bar.as_ref().is_none_or(|bar|bar.context.kind!=CanvasBarKind::Picker),"calibration accepted");ready(&w);
                    assert_ne!(document(&w).layers,before.layers,"{effect} {role:?} {kind} changes adjustment");
                    assert_source_unchanged(&w,&before);assert_eq!(state(&w).colors,foreground);assert_eq!(state(&w).layer_tools.tool,tool);
                    pump(200);assert_ne!(shown(&w,at),pixel,"{effect} {role:?} {kind} changes presented photo");
                    crate::snapshot(&w).save_to_png(output.join(format!("{effect}-{role:?}-{kind}-{width}-{theme:?}.png"))).unwrap();
                    if kind=="mouse" {assert_persisted(&w,&output.join(format!("{effect}-{role:?}-{width}-{theme:?}.capy")));}
                    w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).layers,before.layers,"one Undo restores complete correction");

                }
            }
        }
    w.window.destroy();pump(100);input.finish();
}

#[test]
#[ignore = "private compositor, hardware GPU and native-input.js --tablet"]
fn native_levels_auto_and_calibration_atomic_history() {tonal_calibration("levels");}

#[test]
#[ignore = "private compositor, hardware GPU and native-input.js --tablet"]
fn native_curves_calibration_atomic_history() {tonal_calibration("curves");}

fn targeted_curves(page: u32) {
    let app=native_test_app("art.capycanvas.TargetedCurves");
    let (w,output)=start(&app,"curves");let width=w.window.width();
    let mut input=RemoteInput::new().settle_ms(150).timeout_secs(30);input.ready();
    choose(&w,&mut input,"properties-page",page);
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
            for kind in ["mouse","pen","touch"] {
                let before=document(&w);let foreground=state(&w).colors.clone();let pixel=shown(&w,[128.,128.]);
                let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::TargetCurve {..}));
                crate::snapshot(&w).save_to_png(output.join(format!("curves-actions-{page}-{width}-{theme:?}.png"))).unwrap();
                input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
                let point=canvas_point(&w,[128.,128.]);let end=[point[0],point[1]-30.];
                match kind {
                    "mouse"=>input.perform(json!([{"point":point,"down":true},{"point":end},{"down":false}])),
                    "pen"=>input.perform(json!([{"pen":"down","point":point},{"pen":"move","point":end},{"pen":"up"},{"pen":"leave"}])),
                    _=>input.perform(json!([{"touch":"down","point":point},{"wait_ms":700},{"touch":"move","point":end},{"touch":"up"}])),
                }
                until(||document(&w).layers!=before.layers,"native targeted drag publishes curve");ready(&w);pump(200);
                assert_source_unchanged(&w,&before);assert_eq!(state(&w).colors,foreground);assert_ne!(shown(&w,[128.,128.]),pixel);
                let after=document(&w);let key=format!("curve_{page}");
                assert!(matches!(after.layer(after.active_layer).unwrap().effect.as_ref().unwrap().value(&key),Some(EffectValue::Curve(points)) if points.len()==3));
                crate::snapshot(&w).save_to_png(output.join(format!("targeted-{page}-{kind}-{width}-{theme:?}.png"))).unwrap();
                if kind=="mouse" {assert_persisted(&w,&output.join(format!("targeted-{page}-{width}-{theme:?}.capy")));}
                input.key(0xff1b);w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).layers,before.layers,"one Undo restores targeted drag");
                let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::TargetCurve {..}));
                input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
                let point=canvas_point(&w,[128.,128.]);input.perform(json!([{"point":point,"down":true},{"point":[point[0],point[1]+25.]},{"wait_ms":500}]));
                until(||document(&w).layers!=before.layers,"targeted preview before Escape");
                input.key(0xff1b);input.perform(json!([{"down":false}]));ready(&w);
                assert_eq!(document(&w).layers,before.layers,"Escape restores an active targeted preview");assert_eq!(state(&w).colors,foreground);
            }
    }
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private compositor, hardware GPU and native-input.js --tablet"]
fn native_targeted_curves_rgb_contacts_and_cancel() {targeted_curves(0);}

#[test]
#[ignore = "private compositor, hardware GPU and native-input.js --tablet"]
fn native_targeted_curves_red_contacts_and_cancel() {targeted_curves(1);}


#[test]
#[ignore = "private compositor, hardware GPU, LAYER_NATIVE_INPUT_TRACE=1 and LAYER_NATIVE_EVENT_MS=8"]
fn native_targeted_curves_motion_and_latency() {
    let app=native_test_app("art.capycanvas.TargetedCurveTiming");
    let (w,output)=start(&app,"curves");
    let mut input=RemoteInput::new().settle_ms(0).timeout_secs(30);input.ready();
    let stats=ui_session(&w).engine().backend().stats.clone();
    let mut reports=Vec::new();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::TargetCurve {..}));
        input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
        let point=canvas_point(&w,[128.,128.]);
        input.perform(json!([{"point":point,"down":true},{"point":[point[0],point[1]-20.]},{"wait_ms":400},{"down":false}]));
        until(||document(&w).layer(document(&w).active_layer).unwrap().effect.as_ref().unwrap().value("curve_0").is_some_and(|value|matches!(value,EffectValue::Curve(points) if points.len()==3)),"priming targeted gesture applies");
        input.key(0xff1b);w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);
        for gesture in 0..3 {
            let button=action_button(&w,&mut input,|action|matches!(action,EffectAction::TargetCurve {..}));
            input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
            until(||ui_session(&w).engine().backend().frames_idle() && w.frame_timer.borrow().is_none(),"preceding targeted frames settle");pump(100);
            let baseline=stats.lock().unwrap().camera_views.last().map(|entry|entry.2);
            *stats.lock().unwrap()=Default::default();
            let original=ui_session(&w).engine().document().layer(ui_session(&w).engine().document().active_layer).unwrap().effect.as_ref().unwrap().value("curve_0").unwrap().clone();
            let applications=Rc::new(RefCell::new(Vec::new()));
            let observations=applications.clone();let observed=w.clone();let mut previous=original;
            let observer=glib::timeout_add_local(Duration::from_millis(1),move || {
                let session=ui_session(&observed);let doc=session.engine().document();
                let current=doc.layer(doc.active_layer).unwrap().effect.as_ref().unwrap().value("curve_0").unwrap();
                if *current!=previous {observations.borrow_mut().push(glib::monotonic_time().max(0) as u64*1000);previous=current.clone();}
                glib::ControlFlow::Continue
            });
            let point=canvas_point(&w,[128.,128.]);
            let mut events=vec![json!({"point":point,"down":true})];
            for index in 1..=625 {
                let y=point[1]-30.*(index as f32*std::f32::consts::TAU/125.).sin();
                events.push(json!({"point":[point[0],y]}));
            }
            events.push(json!({"down":false}));
            let step=input.step;input.perform(json!(events));observer.remove();ready(&w);
            until(||ui_session(&w).engine().backend().frames_idle() && w.frame_timer.borrow().is_none(),"targeted motion renderer settles");pump(300);
            let trace:serde_json::Value=serde_json::from_slice(&std::fs::read(input.dir.join(format!("trace-{step}.json"))).unwrap()).unwrap();
            let entries=trace.as_array().unwrap();
            let down=entries.first().unwrap()["ns"].as_u64().unwrap();
            let start=entries[1]["ns"].as_u64().unwrap();
            let end=entries[entries.len()-2]["ns"].as_u64().unwrap();
            let mut report=super::photo_drop::frames(&stats);
            let data=stats.lock().unwrap();let mut revision=baseline;let mut moving=Vec::new();
            for presentation in data.presented.iter().filter(|p|p[3]==1 && p[1]>=start && p[1]<=end) {
                if let Some(view)=data.camera_views.iter().find(|view|view.0==presentation[0]) {
                    if Some(view.2)!=revision {moving.push(*presentation);revision=Some(view.2);}
                }
            }
            drop(data);
            let intervals:Vec<f64>=moving.windows(2).map(|pair|pair[1][1].saturating_sub(pair[0][1]) as f64/1e6).collect();
            let first=applications.borrow().first().copied();
            report["theme"]=json!(format!("{theme:?}"));report["gesture"]=json!(gesture);
            report["input_trace"]=trace;report["application_observations_ns"]=json!(*applications.borrow());
            report["native_down_to_first_curve_change_observed_ms"]=json!(first.map(|at|at.saturating_sub(down) as f64/1e6));
            report["native_down_to_first_new_preview_presentation_ms"]=json!(moving.first().map(|frame|frame[1].saturating_sub(down) as f64/1e6));
            report["motion_window_ns"]=json!([start,end]);report["moving_presentations"]=json!(moving);
            report["moving_presentation_intervals_ms"]=json!(intervals);
            report["moving_presentations_per_s"]=json!(moving.len() as f64/((end-start) as f64/1e9));
            assert!(first.is_some() && moving.len()>100,"actual curve application and moving presentations recorded");
            reports.push(report);
            if gesture==2 {crate::snapshot(&w).save_to_png(output.join(format!("targeted-timing-{}-{theme:?}.png",w.window.width()))).unwrap();}
            input.key(0xff1b);w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);
        }
    }
    std::fs::write(output.join(format!("targeted-timing-{}.json",w.window.width())),serde_json::to_vec_pretty(&json!({"canvas":[256,256],"source":"opaque U8 SRGB horizontal colored gradient","viewport":[w.window.width(),w.window.height()],"refresh_hz":120,"event_interval_ms":8,"gesture_count_per_theme":3,"application_observer_interval_ms":1,"reference_tier_qualification":false,"reports":reports})).unwrap()).unwrap();
    input.finish();w.window.destroy();pump(100);
}
