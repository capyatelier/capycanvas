use super::*;
use super::new_photo::ready;
use super::photo_edit::document;
use super::histogram::{choose, scroll_to};
use layer_core::{EffectValue, color::{RgbColor, RgbSpace}};
use layer_ui::EffectAction;
use serde_json::json;

fn start(app: &NativeTestApp) -> Rc<Workspace> {
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    project.document.color.space=RgbSpace::DisplayP3;
    project.document.layers[0].source = Some(layer_core::color::source::rgba8_source([256, 256], |x,y| {
        [x as u8, y as u8, (255-x) as u8, 255]
    }));
    let w = Workspace::with_project(app, Some((project, None)));
    w.window.maximize();w.window.present();ready(&w);
    assert!(matches!(w.window.width(),640|1100));
    for panel in Panel::ALL.into_iter().filter(|panel|!matches!(panel,Panel::Toolbar|Panel::Commands|Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible {panel,visible:false});
    }
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true});
    w.customize(CustomizationAction::CloseExpanded);
    w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[w.window.width() as f32,800.]});
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});ready(&w);w
}
fn insert(w: &Rc<Workspace>, effect: &str) {
    assert!(layer_core::bundled_effect_catalog().filters().iter().any(|filter|filter.id()==effect));
    w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}});ready(w);
}
fn value(w: &Rc<Workspace>, key: &str) -> EffectValue {
    let doc=document(w);doc.layer(doc.active_layer).unwrap().effect.as_ref().unwrap().value(key).unwrap().clone()
}
fn number(w: &Rc<Workspace>, key: &str) -> crate::number_control::NumberControl {
    widgets(w.window.upcast_ref()).find(|widget|widget.widget_name()==format!("property-{key}") && widget.is_mapped())
        .unwrap_or_else(||panic!("mapped generic number {key}")).downcast().unwrap()
}
fn edit(w: &Rc<Workspace>, input: &mut RemoteInput, key: &str, text: &str) {
    let control=number(w,key);scroll_to(control.upcast_ref());
    if let Some(spin)=descendant::<gtk::SpinButton>(&control) {
        input.click(screen_point(spin.upcast_ref(),&w.window,[0.4,0.5]));spin.set_text(text);input.key(0xff0d);
    } else {
        let display=find_css(control.upcast_ref(),"number-value").unwrap();
        input.click(screen_point(&display,&w.window,[0.5,0.5]));
        let entry=descendant::<gtk::Entry>(&control).unwrap();entry.set_text(text);input.key(0xff0d);
    }
    ready(w);assert_eq!(value(w,key),EffectValue::Number(text.parse().unwrap()));
}
fn switch(w: &Rc<Workspace>, key: &str) -> gtk::Switch {
    let label=state(w).layer_properties.controls.iter().find(|control|control.key==key).unwrap().label.clone();
    widgets(w.effects.properties.upcast_ref()).find_map(|widget| {
        let text=widget.downcast_ref::<gtk::Label>()?;
        (text.text()==label).then(||widget.parent().and_then(|row|descendant::<gtk::Switch>(&row))).flatten()
    }).unwrap()
}
fn toggle(w: &Rc<Workspace>, input: &mut RemoteInput, key: &str) {
    let switch=switch(w,key);
    scroll_to(switch.upcast_ref());input.click(screen_point(switch.upcast_ref(),&w.window,[0.5,0.5]));ready(w);
}
fn canvas_pixel(w: &Rc<Workspace>) -> Vec<u8> {
    let texture=crate::snapshot(w);let mut pixels=vec![0;texture.width() as usize*texture.height() as usize*4];texture.download(&mut pixels,texture.width() as usize*4);
    let m=state(w).camera.document_to_surface();let bounds=w.area.compute_bounds(&w.window).unwrap();let scale=w.area.scale_factor() as f32;
    let x=(bounds.x()+(m[0]*64.+m[2]*192.+m[4])/scale) as usize;
    let y=(bounds.y()+(m[1]*64.+m[3]*192.+m[5])/scale) as usize;
    pixels[(y*texture.width() as usize+x)*4..(y*texture.width() as usize+x)*4+4].to_vec()
}

#[test]
#[ignore = "private display and hardware GPU"]
fn native_colorize_threshold_visible_artwork() {
    let app=native_test_app("art.capycanvas.PointwiseSmoke");let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p21-gtk"));
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();let source=document(&w);
    let pixel=||canvas_pixel(&w);
    let mut samples=Vec::new();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);
        let original=pixel();assert!(original[3]>250&&original[..3].iter().max().unwrap()-original[..3].iter().min().unwrap()>60);
        insert(&w,"hue_saturation");toggle(&w,&mut input,"colorize");
        edit(&w,&mut input,"colorize_hue","210");edit(&w,&mut input,"colorize_saturation","55");edit(&w,&mut input,"lightness","12");
        let colorized=pixel();assert_ne!(colorized,original);assert!(colorized[3]>250&&colorized[..3].iter().max().unwrap()-colorized[..3].iter().min().unwrap()>30);
        capture(&w,output,&format!("smoke-colorize-{}-{theme:?}",w.window.width()));
        w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);insert(&w,"threshold");edit(&w,&mut input,"threshold","0.378");
        let threshold=pixel();assert!(threshold.iter().all(|v|*v>250),"visible Threshold pixel {threshold:?}");
        capture(&w,output,&format!("smoke-threshold-{}-{theme:?}",w.window.width()));
        samples.push(json!({"theme":format!("{theme:?}"),"original":original,"colorize":colorized,"threshold":threshold}));
        w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);unchanged_sources(&w,&source);
    }
    std::fs::write(output.join("smoke-pixels.json"),serde_json::to_vec_pretty(&samples).unwrap()).unwrap();
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private display and native keyboard input"]
fn native_property_draft_is_retired_when_document_changes_with_reused_layer_ids() {
    let app=native_test_app("art.capycanvas.PropertyDocumentOwner");let w=start(&app);
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();insert(&w,"hue_saturation");
    let old_epoch=state(&w).document_file.epoch;let layer=document(&w).active_layer;
    let mut next=document(&w);next.id="property-document-owner-second".into();
    std::sync::Arc::make_mut(next.layers.iter_mut().find(|item|item.id==layer).unwrap().effect.as_mut().unwrap()).set("lightness",EffectValue::Number(23.)).unwrap();
    let old=number(&w,"lightness");scroll_to(old.upcast_ref());
    let display=find_css(old.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));
    let entry=descendant::<gtk::Entry>(&old).unwrap();entry.set_text("73.");
    w.documents.enqueue(&w,(layer_core::Project {document:next},None,None));ready(&w);
    assert_ne!(state(&w).document_file.epoch,old_epoch);assert_eq!(document(&w).active_layer,layer);
    assert_ne!(number(&w,"lightness"),old);assert!(old.root().is_none());
    assert_eq!(value(&w,"lightness"),EffectValue::Number(23.));
    input.key(0xff0d);ready(&w);assert_eq!(value(&w,"lightness"),EffectValue::Number(23.));
    w.window.close();pump(100);
}

#[test]
#[ignore = "private display and native keyboard input"]
fn native_hue_colorize_keyboard_focus_and_common_draft() {
    let app=native_test_app("art.capycanvas.HueKeyboard");let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p21-gtk"));
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();insert(&w,"hue_saturation");
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});choose(&w,&mut input,"properties-page",1);
        let native_toggle=switch(&w,"colorize");scroll_to(native_toggle.upcast_ref());native_toggle.grab_focus();pump(100);
        let before=gtk::prelude::RootExt::focus(&w.window);
        assert!(before.as_ref().is_some_and(|focus|focus==native_toggle.upcast_ref::<gtk::Widget>()));
        input.key(32);ready(&w);
        assert_eq!(value(&w,"colorize"),EffectValue::Toggle(true));assert_eq!(state(&w).layer_properties.page.as_deref(),Some("rgb"));
        let after=gtk::prelude::RootExt::focus(&w.window);
        assert_eq!(after.as_ref(),Some(switch(&w,"colorize").upcast_ref::<gtk::Widget>()),"Colorize keyboard focus: before={before:?}, after={after:?}");
        input.key(32);ready(&w);assert_eq!(value(&w,"colorize"),EffectValue::Toggle(false));
        assert_eq!(gtk::prelude::RootExt::focus(&w.window).as_ref(),Some(switch(&w,"colorize").upcast_ref::<gtk::Widget>()));
        choose(&w,&mut input,"properties-page",0);edit(&w,&mut input,"lightness","12");
        let control=number(&w,"lightness");let display=find_css(control.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));
        let entry=descendant::<gtk::Entry>(&control).unwrap();entry.set_text("12.");let focus=gtk::prelude::RootExt::focus(&w.window);
        w.dispatch(UiAction::Effect {action:EffectAction::Set {layer:state(&w).layer_properties.layer.unwrap(),key:"colorize".into(),value:EffectValue::Toggle(true)}});ready(&w);
        assert_eq!(number(&w,"lightness"),control);assert_eq!(entry.text(),"12.");assert_eq!(gtk::prelude::RootExt::focus(&w.window),focus,"common numeric draft survives conditional controls");
        capture(&w,output,&format!("keyboard-focus-{}-{theme:?}",w.window.width()));input.key(0xff1b);
        toggle(&w,&mut input,"colorize");
    }
    input.finish();w.window.destroy();pump(100);
}
fn unchanged_sources(w: &Rc<Workspace>, before: &layer_core::Document) {
    let after=document(w);
    for original in &before.layers {
        let current=after.layer(original.id).unwrap();
        assert_eq!(current.source,original.source);assert_eq!(current.raster.identity(),original.raster.identity());
    }
}
fn persist(w: &Rc<Workspace>, output: &std::path::Path, name: &str) {
    let bytes=super::place_source::snapshot(w);std::fs::write(output.join(format!("{name}.capy")),&bytes).unwrap();
    let reopened=layer_core::Project::read(std::io::Cursor::new(bytes),Default::default()).unwrap();
    let current=document(w);
    assert_eq!(reopened.document.layers,current.layers,"exact values and source payload survive archive reopen");
}
fn capture(w: &Rc<Workspace>, output: &std::path::Path, name: &str) {
    crate::snapshot(w).save_to_png(output.join(format!("{name}.png"))).unwrap();
}
fn motion(w: &Rc<Workspace>, input: &mut RemoteInput, key: &str) -> serde_json::Value {
    let control=number(w,key);scroll_to(control.upcast_ref());
    let scale=descendant::<gtk::Scale>(&control).unwrap();
    let stats=ui_session(w).engine().backend().stats.clone();
    until(||ui_session(w).engine().backend().frames_idle() && w.frame_timer.borrow().is_none(),"preceding filter frames settle");pump(100);
    let baseline=stats.lock().unwrap().camera_views.last().map(|view|view.2);
    *stats.lock().unwrap()=Default::default();
    let point=screen_point(scale.upcast_ref(),&w.window,[0.5,0.5]);
    let mut events=vec![json!({"point":point,"down":true})];
    for index in 1..=625 {
        events.push(json!({"point":[point[0]+scale.width() as f32*0.3*(index as f32*std::f32::consts::TAU/125.).sin(),point[1]]}));
    }
    events.push(json!({"down":false}));let step=input.step;input.perform(json!(events));ready(w);
    until(||ui_session(w).engine().backend().frames_idle() && w.frame_timer.borrow().is_none(),"filter motion settles");
    let trace:serde_json::Value=serde_json::from_slice(&std::fs::read(input.dir.join(format!("trace-{step}.json"))).unwrap()).unwrap();
    let entries=trace.as_array().unwrap();let start=entries[1]["ns"].as_u64().unwrap();let end=entries[entries.len()-2]["ns"].as_u64().unwrap();
    let mut report=super::photo_drop::frames(&stats);let data=stats.lock().unwrap();let mut previous=baseline;let mut moving=Vec::new();
    for frame in data.presented.iter().filter(|frame|frame[3]==1&&frame[1]>=start&&frame[1]<=end) {
        if let Some(view)=data.camera_views.iter().find(|view|view.0==frame[0]) {
            if Some(view.2)!=previous {moving.push(*frame);previous=Some(view.2);}
        }
    }
    assert!(moving.len()>100,"sustained native filter changes produce actual moving presentations");
    report["input_trace"]=trace;report["motion_window_ns"]=json!([start,end]);
    report["moving_presentation_intervals_ms"]=json!(moving.windows(2).map(|pair|pair[1][1].saturating_sub(pair[0][1]) as f64/1e6).collect::<Vec<_>>());
    report["moving_presentations_per_s"]=json!(moving.len() as f64/((end-start) as f64/1e9));report["moving_presentations"]=json!(moving);report["reference_tier_qualification"]=json!(false);report
}

#[test]
#[ignore = "private display and hardware GPU"]
fn native_selective_color_pages_and_persistence() { native_color_pages("selective_color"); }
#[test]
#[ignore = "private display and hardware GPU"]
fn native_channel_mixer_monochrome_retains_values_and_focus() { native_color_pages("channel_mixer"); }
fn native_color_pages(effect: &str) {
    let app=native_test_app(if effect=="selective_color" {"art.capycanvas.SelectiveColor"}else{"art.capycanvas.ChannelMixer"});let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p22-gtk"));
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();let source=document(&w);let width=w.window.width();let mut samples=Vec::new();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);let original=canvas_pixel(&w);insert(&w,effect);
        assert_eq!(canvas_pixel(&w),original,"neutral adjustment retains visible artwork");
        if effect=="selective_color" {
            let pages=["reds","yellows","greens","cyans","blues","magentas","whites","neutrals","blacks"];
            assert_eq!(state(&w).layer_properties.pages.iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),pages);
            assert_eq!(document(&w).layer(document(&w).active_layer).unwrap().effect.as_ref().unwrap().program.parameters.len(),37);
            let before=super::place_source::snapshot(&w);for index in 0..9 {choose(&w,&mut input,"properties-page",index);}assert_eq!(super::place_source::snapshot(&w),before);
            for (index,page) in pages.iter().enumerate() {
                choose(&w,&mut input,"properties-page",index as u32);
                for (ink,text) in [("cyan",((index+1)*2).to_string()),("magenta","-1.25".into()),("yellow","1.5".into()),("black","0.5".into())] {edit(&w,&mut input,&format!("{page}_{ink}"),&text);}
            }
            for (index,page) in pages.iter().enumerate() {choose(&w,&mut input,"properties-page",index as u32);for (ink,expected) in [("cyan",((index+1)*2) as f32),("magenta",-1.25),("yellow",1.5),("black",0.5)] {assert_eq!(value(&w,&format!("{page}_{ink}")),EffectValue::Number(expected));}}
            choose(&w,&mut input,"properties-page",3);let relative=canvas_pixel(&w);assert_ne!(relative,original);capture(&w,output,&format!("selective-relative-{width}-{theme:?}"));
            let before=document(&w).layers;choose(&w,&mut input,"property-mode",1);ready(&w);assert_eq!(value(&w,"mode"),EffectValue::Choice(1));let absolute=canvas_pixel(&w);assert_ne!(absolute,relative);
            let after=document(&w).layers;w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).layers,before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).layers,after);
            capture(&w,output,&format!("selective-absolute-{width}-{theme:?}"));samples.push(json!({"theme":format!("{theme:?}"),"original":original,"relative":relative,"absolute":absolute}));
        } else {
            assert_eq!(state(&w).layer_properties.pages.iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),["red","green","blue"]);
            assert_eq!(document(&w).layer(document(&w).active_layer).unwrap().effect.as_ref().unwrap().program.parameters.len(),17);
            let before=super::place_source::snapshot(&w);for index in 0..3 {choose(&w,&mut input,"properties-page",index);}assert_eq!(super::place_source::snapshot(&w),before);
            for (index,page) in ["red","green","blue"].iter().enumerate() {choose(&w,&mut input,"properties-page",index as u32);for channel in ["red","green","blue"] {edit(&w,&mut input,&format!("{page}_{channel}"),if channel==*page {"85"}else if channel=="red" {"15"}else {"-5"});}edit(&w,&mut input,&format!("{page}_constant"),"1.25");}
            let retained=["red_red","green_green","blue_blue","red_constant"].map(|key|value(&w,key));let rgb=canvas_pixel(&w);assert_ne!(rgb,original);capture(&w,output,&format!("mixer-rgb-{width}-{theme:?}"));
            let mono=switch(&w,"monochrome");scroll_to(mono.upcast_ref());mono.grab_focus();input.key(32);ready(&w);
            assert_eq!(value(&w,"monochrome"),EffectValue::Toggle(true));assert_eq!(state(&w).layer_properties.page.as_deref(),Some("gray"));assert_eq!(state(&w).layer_properties.pages.len(),1);
            assert_eq!(gtk::prelude::RootExt::focus(&w.window).as_ref(),Some(switch(&w,"monochrome").upcast_ref::<gtk::Widget>()));
            assert!(!named::<gtk::DropDown>(w.window.upcast_ref(),"properties-page").is_mapped());
            for (key,text) in [("gray_red","60"),("gray_green","20"),("gray_blue","20"),("gray_constant","1.25")] {edit(&w,&mut input,key,text);}
            let control=number(&w,"gray_constant");let display=find_css(control.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));let entry=descendant::<gtk::Entry>(&control).unwrap();entry.set_text("77.");let focus=gtk::prelude::RootExt::focus(&w.window);
            w.dispatch(UiAction::Effect {action:EffectAction::Set {layer:state(&w).layer_properties.layer.unwrap(),key:"gray_red".into(),value:EffectValue::Number(65.)}});ready(&w);
            assert_eq!(number(&w,"gray_constant"),control);assert_eq!(entry.text(),"77.");assert_eq!(gtk::prelude::RootExt::focus(&w.window),focus);
            let before_cancel=document(&w).layers;input.key(0xff1b);ready(&w);assert_eq!(document(&w).layers,before_cancel);assert_eq!(value(&w,"gray_constant"),EffectValue::Number(1.25));
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gray_red"),EffectValue::Number(60.));w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"gray_red"),EffectValue::Number(65.));
            let gray=canvas_pixel(&w);assert!(gray[3]==255&&gray[..3].iter().max().unwrap()-gray[..3].iter().min().unwrap()<=2);capture(&w,output,&format!("mixer-gray-{width}-{theme:?}"));
            let mono=switch(&w,"monochrome");mono.grab_focus();let before=document(&w).layers;input.key(32);ready(&w);assert_eq!(value(&w,"monochrome"),EffectValue::Toggle(false));assert_eq!(gtk::prelude::RootExt::focus(&w.window).as_ref(),Some(switch(&w,"monochrome").upcast_ref::<gtk::Widget>()));
            assert_eq!(["red_red","green_green","blue_blue","red_constant"].map(|key|value(&w,key)),retained);assert_eq!(value(&w,"gray_red"),EffectValue::Number(65.));
            assert_eq!(["gray_red","gray_green","gray_blue","gray_constant"].map(|key|value(&w,key)),[65.,20.,20.,1.25].map(EffectValue::Number));assert_eq!(canvas_pixel(&w),rgb);
            let after=document(&w).layers;w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).layers,before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).layers,after);
            samples.push(json!({"theme":format!("{theme:?}"),"original":original,"rgb":rgb,"gray":gray}));
        }
        unchanged_sources(&w,&source);persist(&w,output,&format!("{effect}-{width}-{theme:?}"));w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);
    }
    std::fs::write(output.join(format!("{effect}-pixels-{width}.json")),serde_json::to_vec_pretty(&samples).unwrap()).unwrap();input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private display, hardware GPU, native events at8ms"]
fn native_hue_ranges_colorize_retains_values() {
    let app=native_test_app("art.capycanvas.HueRanges");let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p21-gtk"));
    let mut input=RemoteInput::new().settle_ms(100).timeout_secs(30);input.ready();
    let source=document(&w);let width=w.window.width();let mut reports=Vec::new();
    insert(&w,"hue_saturation");
    assert_eq!(document(&w).layer(document(&w).active_layer).unwrap().effect.as_ref().unwrap().program.parameters.len(),42);
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);
        let pages=state(&w).layer_properties.pages;assert_eq!(pages.len(),7);
        assert_eq!(pages.iter().map(|page|page.id.as_str()).collect::<Vec<_>>(),["rgb","reds","yellows","greens","cyans","blues","magentas"]);
        let before_pages=super::place_source::snapshot(&w);
        for index in 1..7 {choose(&w,&mut input,"properties-page",index);assert_eq!(state(&w).layer_properties.page.as_deref(),Some(pages[index as usize].id.as_str()));}
        assert_eq!(super::place_source::snapshot(&w),before_pages,"page navigation creates no stored edit or history");
        choose(&w,&mut input,"properties-page",1);
        edit(&w,&mut input,"reds_hue","27");edit(&w,&mut input,"reds_center","350");edit(&w,&mut input,"reds_width","60");edit(&w,&mut input,"reds_feather","0");
        let retained=["reds_hue","reds_center","reds_width","reds_feather"].map(|key|value(&w,key));
        choose(&w,&mut input,"properties-page",5);edit(&w,&mut input,"blues_saturation","-32");
        choose(&w,&mut input,"properties-page",1);
        assert_eq!(["reds_hue","reds_center","reds_width","reds_feather"].map(|key|value(&w,key)),retained);
        capture(&w,output,&format!("ranges-{width}-{theme:?}"));
        toggle(&w,&mut input,"colorize");
        assert_eq!(state(&w).layer_properties.pages.iter().map(|page|page.id.as_str()).collect::<Vec<_>>(),["rgb"]);
        assert_eq!(state(&w).layer_properties.page.as_deref(),Some("rgb"));
        assert!(!named::<gtk::DropDown>(w.window.upcast_ref(),"properties-page").is_mapped(),"Colorize has no redundant page chooser");
        assert!(state(&w).layer_properties.controls.iter().all(|control|!control.key.starts_with("reds_") && control.key!="hue" && control.key!="saturation"));
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"colorize"),EffectValue::Toggle(false));
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"colorize"),EffectValue::Toggle(true));
        edit(&w,&mut input,"colorize_hue","210");edit(&w,&mut input,"colorize_saturation","55");edit(&w,&mut input,"lightness","12");
        let lightness=number(&w,"lightness");
        let focusable=descendant::<gtk::SpinButton>(&lightness).map(|spin|spin.upcast::<gtk::Widget>()).or_else(||descendant::<gtk::Entry>(&lightness).map(|entry|entry.upcast())).unwrap();focusable.grab_focus();pump(100);
        let focus=gtk::prelude::RootExt::focus(&w.window);assert!(focus.is_some());w.refresh(regions::ALL);pump(100);
        assert_eq!(number(&w,"lightness"),lightness);assert_eq!(gtk::prelude::RootExt::focus(&w.window),focus,"retained visible focus survives publication");
        capture(&w,output,&format!("colorize-{width}-{theme:?}"));
        toggle(&w,&mut input,"colorize");choose(&w,&mut input,"properties-page",1);
        assert_eq!(["reds_hue","reds_center","reds_width","reds_feather"].map(|key|value(&w,key)),retained);
        let before=value(&w,"reds_hue");let report=motion(&w,&mut input,"reds_hue");reports.push(json!({"theme":format!("{theme:?}"),"frames":report}));
        assert_ne!(value(&w,"reds_hue"),before);w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"reds_hue"),before,"native slider gesture is one Undo");
        unchanged_sources(&w,&source);persist(&w,output,&format!("hue-{width}-{theme:?}"));
    }
    std::fs::write(output.join(format!("hue-motion-{width}.json")),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private display and hardware GPU"]
fn native_pointwise_filters_controls_and_persistence() {
    let app=native_test_app("art.capycanvas.PointwiseFilters");let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p21-gtk"));
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();let source=document(&w);let width=w.window.width();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        for effect in ["invert","threshold","desaturate","photo_filter"] {
            insert(&w,effect);
            match effect {
                "threshold"=>{let before=value(&w,"threshold");edit(&w,&mut input,"threshold","0.378");w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"threshold"),before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);},
                "photo_filter"=>{
                    edit(&w,&mut input,"density","37");toggle(&w,&mut input,"preserve_luminance");
                    let button=find_named(w.window.upcast_ref(),"effect-color-color").unwrap();scroll_to(&button);input.click(screen_point(&button,&w.window,[0.5,0.5]));
                    let row=named::<adw::EntryRow>(w.window.visible_dialog().unwrap().upcast_ref(),"edit-color-value-0");row.set_text("0.1234567");
                    super::new_photo::response(&w,"apply");ready(&w);
                    assert!(matches!(value(&w,"color"),EffectValue::Color(RgbColor {space:RgbSpace::DisplayP3,rgba,..}) if rgba[0]==0.1234567));
                },
                _=>assert!(state(&w).layer_properties.controls.is_empty()),
            }
            unchanged_sources(&w,&source);persist(&w,output,&format!("{effect}-{width}-{theme:?}"));capture(&w,output,&format!("{effect}-{width}-{theme:?}"));
            w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);
        }
    }
    input.finish();w.window.destroy();pump(100);
}
