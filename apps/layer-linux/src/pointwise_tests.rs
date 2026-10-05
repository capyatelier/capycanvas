use super::*;
use super::new_photo::ready;
use super::photo_edit::document;
use super::histogram::{choose, scroll_to};
use layer_core::{EffectValue, color::{RgbColor, RgbSpace}};
use layer_ui::EffectAction;
use serde_json::json;

fn start(app: &NativeTestApp) -> Rc<Workspace> {
    let mut project = new_drawing(256, 256, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut project).color.space=RgbSpace::DisplayP3;
    let layer_core::authored::SourceTarget::Paint(paint) = project.working.target.unwrap() else { unreachable!() };
    project.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::PaintBase::new((layer_core::color::source::rgba8_source([256, 256], |x,y| {
        [x as u8, y as u8, (255-x) as u8, 255]
    })).into()));
    let w = Workspace::with_project(app, Some((project, None)));
    w.window.maximize();w.window.present();ready(&w);
    assert!(matches!(w.window.width(),640|1100));
    configure_properties(&w);w
}
pub(super) fn configure_properties(w:&Rc<Workspace>) {
    for panel in Panel::ALL.into_iter().filter(|panel|!matches!(panel,Panel::Toolbar|Panel::Commands|Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible {panel,visible:false});
    }
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true});
    w.customize(CustomizationAction::CloseExpanded);
    w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[w.window.width() as f32,800.]});
    w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});ready(w);
}
fn insert(w: &Rc<Workspace>, effect: &str) {
    assert!(layer_core::bundled_effect_catalog().filters().iter().any(|filter|filter.id()==effect));
    w.dispatch(UiAction::Effect {action:EffectAction::Insert {effect:effect.into()}});ready(w);
}
pub(super) fn value(w: &Rc<Workspace>, key: &str) -> EffectValue {
    let doc=document(w);doc.scene().effect(doc.working.occurrence.unwrap()).unwrap().value(key).unwrap().clone()
}
pub(super) fn number(w: &Rc<Workspace>, key: &str) -> crate::number_control::NumberControl {
    widgets(w.window.upcast_ref()).find(|widget|widget.widget_name()==format!("property-{key}") && widget.is_mapped())
        .unwrap_or_else(||panic!("mapped generic number {key}")).downcast().unwrap()
}
pub(super) fn edit(w: &Rc<Workspace>, input: &mut RemoteInput, key: &str, text: &str) {
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
    let old_epoch=state(&w).document_file.epoch;let layer=document(&w).working.occurrence.unwrap();
    let mut next=document(&w);next.artwork.id=layer_core::authored::PortableId::random();
    let effect = next.scene().effect_handle(layer).unwrap();
    let view = next.scene().effect(layer).unwrap();
    let parameter = view.program.parameters.iter().position(|parameter| parameter.key.as_ref() == "lightness").unwrap();
    next.artwork.effects.get_mut(effect).unwrap().values[parameter] = EffectValue::Number(23.);
    let old=number(&w,"lightness");scroll_to(old.upcast_ref());
    let display=find_css(old.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));
    let entry=descendant::<gtk::Entry>(&old).unwrap();entry.set_text("73.");
    w.documents.enqueue(&w,(next,None));ready(&w);
    assert_ne!(state(&w).document_file.epoch,old_epoch);assert_eq!(document(&w).working.occurrence.unwrap(),layer);
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
    for (_, id, original) in before.artwork.paint.iter() {
        let current=after.artwork.paint.get(after.artwork.paint.resolve(id).unwrap()).unwrap();
        assert_eq!(current.base,original.base);assert_eq!(current.raster.identity(),original.raster.identity());
    }
}
pub(super) fn saved_artwork(w: &Rc<Workspace>) -> (layer_core::authored::ArtworkCapture, Vec<u8>) {
    let capture = ui_session(w).capture_artwork().unwrap();
    let mut bytes = Vec::new();
    write_capture(&capture, &mut bytes).unwrap();
    (capture, bytes)
}
pub(super) fn assert_saved_artwork(capture: &layer_core::authored::ArtworkCapture, reopened: &layer_core::Document) {
    let restored = reopened.artwork.capture(capture.checkpoint).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let before = layer_core::package::codec::PreparedPackage::prepare(capture,None,&cancel).unwrap();
    let after = layer_core::package::codec::PreparedPackage::prepare(&restored,None,&cancel).unwrap();
    assert_eq!(before.manifest(),after.manifest(),"exact values and source payload survive package reopen");
}
fn assert_source_scene(w: &Rc<Workspace>, before: &layer_core::Document) {
    let after=document(w);
    let before_order: Vec<_> = before.scene().order().iter().map(|h| before.artwork.occurrences.id(*h).unwrap()).collect();
    let after_order: Vec<_> = after.scene().order().iter().map(|h| after.artwork.occurrences.id(*h).unwrap()).collect();
    assert_eq!(before_order,after_order);
    let original:serde_json::Value=serde_json::from_slice(&artwork_manifest(before)).unwrap();
    let current:serde_json::Value=serde_json::from_slice(&artwork_manifest(&after)).unwrap();
    for object in original["objects"].as_array().unwrap().iter().filter(|object|object["type"]!="capy.output/1") {
        assert_eq!(current["objects"].as_array().unwrap().iter().find(|entry|entry["id"]==object["id"]),Some(object),"original authored source/scene object retains exact portable content");
    }
    assert_eq!(after.artwork.paint.iter().count(),before.artwork.paint.iter().count());
    for (_, id, original) in before.artwork.paint.iter() {
        let current=after.artwork.paint.get(after.artwork.paint.resolve(id).unwrap()).unwrap();
        if let (Some(current),Some(original))=(&current.base,&original.base) {assert_source_samples(&current.image,&original.image);} else {assert_eq!(current.base,original.base);}
        assert_eq!(super::editing_tools::pixels(&current.raster),super::editing_tools::pixels(&original.raster),"original authored raster payload is unchanged");
    }
}
fn persist(w: &Rc<Workspace>, output: &std::path::Path, name: &str) {
    let (saved,bytes)=saved_artwork(w);std::fs::write(output.join(format!("{name}.capy")),&bytes).unwrap();
    let reopened=open_native_document(std::io::Cursor::new(bytes));
    assert_saved_artwork(&saved,&reopened);
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
            assert_eq!(document(&w).scene().effect(document(&w).working.occurrence.unwrap()).unwrap().program.parameters.len(),37);
            let before=super::place_source::snapshot(&w);for index in 0..9 {choose(&w,&mut input,"properties-page",index);}assert_eq!(super::place_source::snapshot(&w),before);
            for (index,page) in pages.iter().enumerate() {
                choose(&w,&mut input,"properties-page",index as u32);
                for (ink,text) in [("cyan",((index+1)*2).to_string()),("magenta","-1.25".into()),("yellow","1.5".into()),("black","0.5".into())] {edit(&w,&mut input,&format!("{page}_{ink}"),&text);}
            }
            for (index,page) in pages.iter().enumerate() {choose(&w,&mut input,"properties-page",index as u32);for (ink,expected) in [("cyan",((index+1)*2) as f32),("magenta",-1.25),("yellow",1.5),("black",0.5)] {assert_eq!(value(&w,&format!("{page}_{ink}")),EffectValue::Number(expected));}}
            choose(&w,&mut input,"properties-page",3);let relative=canvas_pixel(&w);assert_ne!(relative,original);capture(&w,output,&format!("selective-relative-{width}-{theme:?}"));
            let before=document(&w).artwork;choose(&w,&mut input,"property-mode",1);ready(&w);assert_eq!(value(&w,"mode"),EffectValue::Choice(1));let absolute=canvas_pixel(&w);assert_ne!(absolute,relative);
            let after=document(&w).artwork;w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).artwork,before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).artwork,after);
            capture(&w,output,&format!("selective-absolute-{width}-{theme:?}"));samples.push(json!({"theme":format!("{theme:?}"),"original":original,"relative":relative,"absolute":absolute}));
        } else {
            assert_eq!(state(&w).layer_properties.pages.iter().map(|p|p.id.as_str()).collect::<Vec<_>>(),["red","green","blue"]);
            assert_eq!(document(&w).scene().effect(document(&w).working.occurrence.unwrap()).unwrap().program.parameters.len(),17);
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
            let before_cancel=document(&w).artwork;input.key(0xff1b);ready(&w);assert_eq!(document(&w).artwork,before_cancel);assert_eq!(value(&w,"gray_constant"),EffectValue::Number(1.25));
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(value(&w,"gray_red"),EffectValue::Number(60.));w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(value(&w,"gray_red"),EffectValue::Number(65.));
            let gray=canvas_pixel(&w);assert!(gray[3]==255&&gray[..3].iter().max().unwrap()-gray[..3].iter().min().unwrap()<=2);capture(&w,output,&format!("mixer-gray-{width}-{theme:?}"));
            let mono=switch(&w,"monochrome");mono.grab_focus();let before=document(&w).artwork;input.key(32);ready(&w);assert_eq!(value(&w,"monochrome"),EffectValue::Toggle(false));assert_eq!(gtk::prelude::RootExt::focus(&w.window).as_ref(),Some(switch(&w,"monochrome").upcast_ref::<gtk::Widget>()));
            assert_eq!(["red_red","green_green","blue_blue","red_constant"].map(|key|value(&w,key)),retained);assert_eq!(value(&w,"gray_red"),EffectValue::Number(65.));
            assert_eq!(["gray_red","gray_green","gray_blue","gray_constant"].map(|key|value(&w,key)),[65.,20.,20.,1.25].map(EffectValue::Number));assert_eq!(canvas_pixel(&w),rgb);
            let after=document(&w).artwork;w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).artwork,before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).artwork,after);
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
    assert_eq!(document(&w).scene().effect(document(&w).working.occurrence.unwrap()).unwrap().program.parameters.len(),42);
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
                    crate::color_editor::tests::form(&w,0,layer_ui::ColorForm::RgbUnit);crate::color_editor::tests::value(&w,0,0,"0.1234567");
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

#[allow(deprecated)]
pub(super) fn import_lookup(w: &Rc<Workspace>, path: Option<&std::path::Path>, input: &mut RemoteInput) {
    if let Some(notice)=state(w).notice {w.dispatch(UiAction::Notice {id:notice.id,accept:false});pump(100);}
    lookup_action(w,input,|action|matches!(action,EffectAction::ImportLookup {..}));let dialog=super::new_photo::chooser();assert!(state(w).host_error.is_none(),"new native import clears preceding error");
    if let Some(path)=path {dialog.set_file(&gtk::gio::File::for_path(path)).unwrap();pump(250);dialog.response(gtk::ResponseType::Accept);} else {dialog.response(gtk::ResponseType::Cancel);}
    until(||!state(w).document_file.busy,"lookup worker finishes");if state(w).host_error.is_none() {ready(w);}
}

fn lookup_action(w:&Rc<Workspace>,input:&mut RemoteInput,select:impl Fn(&EffectAction)->bool) {
    let view=state(w).layer_properties;
    let index=view.actions.iter().position(|action|select(&action.action)).unwrap();
    if matches!(view.actions[index].action,EffectAction::LookupPreset {..}) {
        let item=view.actions[..index].iter().filter(|action|matches!(action.action,EffectAction::LookupPreset {..})).count();
        choose(w,input,"property-resource-choice",item as u32);
    } else {
        let button=named::<gtk::Button>(w.effects.properties.upcast_ref(),"property-picker");scroll_to(button.upcast_ref());assert!(button.is_mapped());
        let bounds=button.compute_bounds(&w.window).unwrap();let picked=w.window.pick((bounds.x()+bounds.width()/2.) as f64,(bounds.y()+bounds.height()/2.) as f64,gtk::PickFlags::DEFAULT).unwrap();
        assert!(picked==button||picked.is_ancestor(&button),"Import button is covered by {picked:?}");
        input.click(screen_point(button.upcast_ref(),&w.window,[0.5,0.5]));
    }
}

#[test]
#[ignore = "private display, native input and hardware GPU"]
fn native_lookup_builtin_looks_and_discoverable_import() {
    use layer_core::lut3d::Look;
    let app=native_test_app("art.capycanvas.LookupLooks");let w=super::histogram::photo_workspace(&app);w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(layer_ui::WorkspaceState {layout:layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk),..Default::default()})});ready(&w);w.dispatch(UiAction::Invoke {command:CommandId::FitCanvas});
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk"));let width=w.window.width();let mut input=RemoteInput::new().timeout_secs(30);input.ready();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);let source=document(&w);let original=canvas_pixel(&w);insert(&w,"color_lookup");
        assert_eq!(state(&w).layer_properties.resource_name.as_deref(),Some("Original"));assert!(!state(&w).layer_properties.controls.iter().any(|control|control.key=="color_space"));assert_eq!(canvas_pixel(&w),original);
        capture(&w,output,&format!("lookup-original-{width}-{theme:?}"));
        let choice=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice");assert_eq!(choice.model().unwrap().n_items(),4);assert_eq!(choice.selected(),0);
        let group=w.groups.borrow().iter().find(|group|group.panels.contains(&Panel::Properties)).unwrap().root.clone();
        for name in ["property-resource-choice","property-picker","property-intensity"] {let widget=find_named(w.effects.properties.upcast_ref(),name).unwrap();let bounds=widget.compute_bounds(&group).unwrap();assert!(bounds.x()>=-1. && bounds.y()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1. && bounds.y()+bounds.height()<=group.height() as f32+1.,"Photo workspace {name} is fully visible: {bounds:?} in {}x{}",group.width(),group.height());}

        assert!(!widgets(w.effects.properties.upcast_ref()).any(|widget|widget.widget_name()=="property-resource-help" || widget.widget_name()=="property-picker-menu"));
        for look in Look::ALL {
            let before=document(&w).artwork;lookup_action(&w,&mut input,|action|matches!(action,EffectAction::LookupPreset {preset:Some(value),..} if *value==look));ready(&w);
            let after=document(&w).artwork;let doc=document(&w);let resource=doc.scene().effect(doc.working.occurrence.unwrap()).unwrap().lut3d().unwrap();assert_eq!(Look::for_resource(resource),Some(look));let choice=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice");let label=choice.selected_item().unwrap().downcast::<gtk::StringObject>().unwrap().string();assert_eq!(label.lines().count(),1,"preset label stays on one line");assert!(choice.height()<=40,"ordinary LUT selector has one-row height");assert_ne!(canvas_pixel(&w),original);assert!(!state(&w).layer_properties.controls.iter().any(|control|control.key=="color_space"));
            let checkpoint=ui_session(&w).engine().checkpoint();lookup_action(&w,&mut input,|action|matches!(action,EffectAction::LookupPreset {preset:Some(value),..} if *value==look));ready(&w);assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint,"choosing the current Look is inert");
            w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).artwork,before);w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).artwork,after);unchanged_sources(&w,&source);
            capture(&w,output,&format!("lookup-{look:?}-{width}-{theme:?}"));
        }
        edit(&w,&mut input,"intensity","50");let adjusted=canvas_pixel(&w);
        let authored=document(&w);let occurrence_id=authored.artwork.occurrences.id(authored.working.occurrence.unwrap()).unwrap();
        let (saved,bytes)=saved_artwork(&w);let reopened=open_native_document(std::io::Cursor::new(bytes));assert_saved_artwork(&saved,&reopened);
        w.documents.enqueue(&w,(reopened,None));ready(&w);assert_eq!(canvas_pixel(&w),adjusted);
        let restored=document(&w).artwork.occurrences.resolve(occurrence_id).unwrap();
        w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(restored)});ready(&w);
        lookup_action(&w,&mut input,|action|matches!(action,EffectAction::LookupPreset {preset:None,..}));ready(&w);assert_eq!(canvas_pixel(&w),original);assert_eq!(value(&w,"intensity"),EffectValue::Number(50.));
        import_lookup(&w,None,&mut input);assert_eq!(canvas_pixel(&w),original);assert!(state(&w).host_error.is_none());capture(&w,output,&format!("lookup-original-reopened-{width}-{theme:?}"));
        w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);
    }
    input.finish();w.window.destroy();pump(100);
}

pub(super) fn lookup_cube(title:&str,swap:bool) -> String {
        let mut text=format!("TITLE \"{title}\"\nLUT_3D_SIZE 2\n");
        for b in 0..2 {for g in 0..2 {for r in 0..2 {let v=if swap {[b,g,r]} else {[1-r,1-g,1-b]};text.push_str(&format!("{} {} {}\n",v[0],v[1],v[2]));}}}text
}

#[test]
#[ignore = "private display, native file chooser and hardware GPU"]
#[allow(deprecated)]
fn native_color_lookup_import_replace_and_persistence() {
    glib::set_prgname(Some("capy-canvas-test"));glib::set_application_name(APP_NAME);
    let app=native_test_app("art.capycanvas.ColorLookup");let w=start(&app);
    let output=std::path::Path::new(artifact_dir("../../artifacts/photo-editing-color/p23-gtk")).canonicalize().unwrap();
    let mut input=RemoteInput::new().timeout_secs(30);input.ready();
    let import=|path:Option<&std::path::Path>,input:&mut RemoteInput| import_lookup(&w,path,input);
    let lut=||document(&w).scene().effect(document(&w).working.occurrence.unwrap()).unwrap().lut3d().cloned();
    let cube=lookup_cube;
    let mut pixels=Vec::new();
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);let source=document(&w);let original=canvas_pixel(&w);insert(&w,"color_lookup");
        assert!(lut().is_none());assert_eq!(document(&w).scene().effect(document(&w).working.occurrence.unwrap()).unwrap().program.resolution,layer_core::EffectResolution::Native);assert_eq!(canvas_pixel(&w),original);
        capture(&w,&output,&format!("lookup-empty-{}-{theme:?}",w.window.width()));let unchanged=document(&w).artwork;let checkpoint=ui_session(&w).engine().checkpoint();import(None,&mut input);assert_eq!(document(&w).artwork,unchanged);assert!(state(&w).host_error.is_none());
        let malformed=output.join("malformed.cube");std::fs::write(&malformed,b"LUT_3D_SIZE 2\n0 0 0\n").unwrap();
        import(Some(&malformed),&mut input);assert!(state(&w).host_error.is_some());assert_eq!(document(&w).artwork,unchanged);
        let oversized=output.join("oversized.cube");std::fs::File::create(&oversized).unwrap().set_len(layer_core::Lut3d::MAX_TEXT_BYTES as u64+1).unwrap();
        import(Some(&oversized),&mut input);assert!(state(&w).host_error.is_some());assert_eq!(document(&w).artwork,unchanged);std::fs::remove_file(oversized).unwrap();assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint,"cancelled and rejected imports create no undo entry");
        let title="夕空の色彩調整と深い青の階調".repeat(10);let first=output.join("first.cube");let second=output.join("second.cube");std::fs::write(&first,cube("Inverse gradient",false)).unwrap();std::fs::write(&second,cube(&title,true)).unwrap();
        import(Some(&first),&mut input);let inverse=lut().unwrap();assert!(inverse.payload().is_some());assert_eq!(state(&w).layer_properties.description,"Inverse gradient");let choice=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice");assert!(choice.is_mapped());assert_eq!(choice.selected_item().unwrap().downcast::<gtk::StringObject>().unwrap().string(),"Inverse gradient");assert_eq!(choice.tooltip_text().as_deref(),Some("Inverse gradient"));
        assert!(state(&w).layer_properties.actions.iter().any(|action|matches!(action.action,EffectAction::ImportLookup {..})));
        let first_pixel=canvas_pixel(&w);assert_ne!(first_pixel,original);assert_eq!(first_pixel[3],255);
        edit(&w,&mut input,"intensity","65");choose(&w,&mut input,"property-color_space",1);assert_eq!(value(&w,"color_space"),EffectValue::Choice(1));capture(&w,&output,&format!("lookup-import-{}-{theme:?}",w.window.width()));
        let before=document(&w).artwork;import(Some(&second),&mut input);let replacement=lut().unwrap();assert_ne!(replacement.digest(),inverse.digest());let after=document(&w).artwork;
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});ready(&w);assert_eq!(document(&w).artwork,before);assert_eq!(lut().unwrap().digest(),inverse.digest());
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});ready(&w);assert_eq!(document(&w).artwork,after);assert_eq!(lut().unwrap().digest(),replacement.digest());
        let choice=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice");assert_eq!(state(&w).layer_properties.description,title);assert_eq!(choice.selected_item().unwrap().downcast::<gtk::StringObject>().unwrap().string(),title);assert_eq!(choice.tooltip_text().as_deref(),Some(title.as_str()));assert!(choice.width()<=w.effects.properties.width());let replaced_pixel=canvas_pixel(&w);assert_ne!(replaced_pixel,first_pixel);
        std::fs::remove_file(first).unwrap();std::fs::remove_file(second).unwrap();unchanged_sources(&w,&source);
        let authored=document(&w);let selected=authored.working.occurrence.unwrap();
        let occurrence_id=authored.artwork.occurrences.id(selected).unwrap();
        let effect_id=authored.artwork.effects.id(authored.scene().effect_handle(selected).unwrap()).unwrap();
        let (saved,bytes)=saved_artwork(&w);let archive=output.join(format!("lookup-{}-{theme:?}.capy",w.window.width()));std::fs::write(&archive,&bytes).unwrap();
        let reopened=open_native_document(std::io::Cursor::new(bytes));assert_saved_artwork(&saved,&reopened);
        let restored=reopened.artwork.occurrences.resolve(occurrence_id).unwrap();
        assert_eq!(reopened.scene().effect_handle(restored),reopened.artwork.effects.resolve(effect_id));
        let loaded=reopened.scene().effect(restored).unwrap().lut3d().unwrap();assert_eq!(loaded.payload(),replacement.payload());
        w.documents.enqueue(&w,(reopened,None));ready(&w);assert_eq!(canvas_pixel(&w),replaced_pixel);capture(&w,&output,&format!("lookup-reopened-{}-{theme:?}",w.window.width()));
        let restored=document(&w).artwork.occurrences.resolve(occurrence_id).unwrap();
        w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(restored)});ready(&w);
        assert_eq!(document(&w).working.occurrence,Some(restored));
        pixels.push(json!({"theme":format!("{theme:?}"),"original":original,"inverse":first_pixel,"reopened":replaced_pixel}));
        w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);
    }
    std::fs::write(output.join(format!("lookup-pixels-{}.json",w.window.width())),serde_json::to_vec_pretty(&pixels).unwrap()).unwrap();
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private display and hardware GPU"]
fn native_local_adjustments_analysis_history_and_recreation() {
    let app=native_test_app("art.capycanvas.LocalAdjustments");let w=start(&app);
    let output=std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_|"../../artifacts/photo-editing-color/p26-gtk".into()));std::fs::create_dir_all(&output).unwrap();let output=output.as_path();
    let sample=|w:&Rc<Workspace>| {let camera=state(w).camera;let m=camera.document_to_surface();let mut published=None;until(||{match ui_session(w).engine().backend().capture_in(w.view_color()) {Ok(image)=>{published=Some(image);true},Err(error) if error=="Canvas has not rendered"=>false,Err(error)=>panic!("local-analysis readback: {error}")}},"local-analysis artwork frame publication");let image=published.unwrap();let scale=[image.width as f32/camera.viewport[0] as f32,image.height as f32/camera.viewport[1] as f32];std::fs::write(output.join("readback-extent.json"),serde_json::to_vec_pretty(&json!({"image":[image.width,image.height],"viewport":camera.viewport,"scale":scale})).unwrap()).unwrap();[[32.,32.],[64.,192.],[192.,64.],[224.,224.]].map(|[dx,dy]| {let x=((m[0]*dx+m[2]*dy+m[4])*scale[0]) as usize;let y=((m[1]*dx+m[3]*dy+m[5])*scale[1]) as usize;let offset=y*image.stride as usize+x*4;image.bytes[offset..offset+4].to_vec()})};
    let width=w.window.width();let mut input=RemoteInput::new().timeout_secs(30);input.ready();let mut samples=Vec::new();
    let settled=|w:&Rc<Workspace>| {
        let deadline=Instant::now()+Duration::from_secs(60);
        loop {
            pump(20);let view=state(w);
            assert!(view.host_error.is_none(),"analysis host error {:?}",view.host_error);
            assert_ne!(view.layer_properties.description,"Could not update this adjustment.");
            if view.layer_properties.description!="Updating…" && !ui_session(w).wants_continuous_frames() {break;}
            assert!(Instant::now()<deadline,"local analysis timeout: {}",view.layer_properties.description);
        }
        ready(w);until(||ui_session(w).engine().backend().frames_idle() && w.frame_timer.borrow().is_none(),"published local-analysis canvas frames");pump(100);
    };
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);let source=document(&w);let original=sample(&w);
        insert(&w,"hue_saturation");let lower=document(&w).working.occurrence.unwrap();
        insert(&w,"shadows_highlights");let shadows=document(&w).working.occurrence.unwrap();let view=state(&w).layer_properties;let title=w.effects.properties.first_child().unwrap().downcast::<gtk::Label>().unwrap();assert_eq!(title.text().as_str(),view.title,"native Properties heading follows shared effect/analysis title");if view.description=="Updating…" {assert!(title.text().contains("Updating"));capture(&w,output,&format!("analysis-pending-{width}-{theme:?}"));}settled(&w);assert_eq!(title.text().as_str(),state(&w).layer_properties.title);
        assert_eq!(state(&w).layer_properties.controls.iter().map(|c|c.key.as_str()).collect::<Vec<_>>(),["shadows","highlights"]);
        let before=document(&w).artwork;edit(&w,&mut input,"shadows","65");edit(&w,&mut input,"highlights","45");settled(&w);
        let adjusted=sample(&w);assert_ne!(adjusted,original);capture(&w,output,&format!("shadows-highlights-{width}-{theme:?}"));
        let after=document(&w).artwork;w.dispatch(UiAction::Invoke {command:CommandId::Undo});settled(&w);assert_eq!(value(&w,"highlights"),EffectValue::Number(0.));
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});settled(&w);assert_eq!(document(&w).artwork,after);assert_ne!(before,after);
        insert(&w,"clarity");let clarity=document(&w).working.occurrence.unwrap();settled(&w);assert_eq!(state(&w).layer_properties.controls.iter().map(|c|c.key.as_str()).collect::<Vec<_>>(),["amount"]);
        edit(&w,&mut input,"amount","55");settled(&w);let positive=sample(&w);capture(&w,output,&format!("clarity-positive-{width}-{theme:?}"));
        let control=number(&w,"amount");scroll_to(control.upcast_ref());let display=find_css(control.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));
        let entry=descendant::<gtk::Entry>(&control).unwrap();entry.set_text("99.");let before_cancel=document(&w).artwork;input.key(0xff1b);settled(&w);assert_eq!(document(&w).artwork,before_cancel);
        edit(&w,&mut input,"amount","-55");settled(&w);let negative=sample(&w);capture(&w,output,&format!("clarity-negative-{width}-{theme:?}"));assert_ne!(positive,negative);
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});settled(&w);assert_eq!(value(&w,"amount"),EffectValue::Number(55.));
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});settled(&w);assert_eq!(value(&w,"amount"),EffectValue::Number(-55.));
        insert(&w,"dehaze");let dehaze=document(&w).working.occurrence.unwrap();settled(&w);
        assert_eq!(state(&w).layer_properties.controls.iter().map(|c|c.key.as_str()).collect::<Vec<_>>(),["amount"]);
        assert_eq!(value(&w,"amount"),EffectValue::Number(0.));assert_eq!(sample(&w),negative,"zero Dehaze preserves the composed source");
        let amount=number(&w,"amount");scroll_to(amount.upcast_ref());let bounds=amount.compute_bounds(&w.effects.properties).unwrap();assert!(bounds.width()>50. && bounds.x()>=-1. && bounds.x()+bounds.width()<=w.effects.properties.width() as f32+1.,"Dehaze ordinary Amount fits Properties: {bounds:?}");
        edit(&w,&mut input,"amount","55");settled(&w);let dehaze_positive=sample(&w);assert_ne!(dehaze_positive,negative);capture(&w,output,&format!("dehaze-positive-{width}-{theme:?}"));
        edit(&w,&mut input,"amount","-55");settled(&w);let dehaze_negative=sample(&w);assert_ne!(dehaze_negative,dehaze_positive);capture(&w,output,&format!("dehaze-negative-{width}-{theme:?}"));
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});settled(&w);assert_eq!(value(&w,"amount"),EffectValue::Number(55.));assert_eq!(sample(&w),dehaze_positive);
        w.dispatch(UiAction::Invoke {command:CommandId::Redo});settled(&w);assert_eq!(value(&w,"amount"),EffectValue::Number(-55.));assert_eq!(sample(&w),dehaze_negative);
        w.dispatch(UiAction::Effect {action:EffectAction::Set {layer:layer_ui::occurrence_token(lower),key:"lightness".into(),value:EffectValue::Number(-20.)}});
        let resize_history=ui_session(&w).engine().checkpoint();w.window.unmaximize();
        for index in 0..8 {w.window.set_default_size(width-80+(index%2)*40,700+(index%3)*20);pump(15);assert!(state(&w).host_error.is_none(),"surface resize during source-aware GPU publication");}
        w.window.maximize();settled(&w);assert_eq!(ui_session(&w).engine().checkpoint(),resize_history,"surface reconfiguration never enters document history");
        let changed=sample(&w);assert_ne!(changed,dehaze_negative);capture(&w,output,&format!("dehaze-stacked-{width}-{theme:?}"));unchanged_sources(&w,&source);
        w.dispatch(UiAction::Invoke {command:CommandId::Undo});settled(&w);assert_eq!(sample(&w),dehaze_negative,"Undo lower source edit restores Dehaze analysis");w.dispatch(UiAction::Invoke {command:CommandId::Redo});settled(&w);assert_eq!(sample(&w),changed);
        let deleted=[dehaze,clarity,shadows,lower].map(|h|document(&w).artwork.occurrences.id(h).unwrap());
        let (saved,bytes)=saved_artwork(&w);std::fs::write(output.join(format!("local-adjustments-{width}-{theme:?}.capy")),&bytes).unwrap();
        let reopened=open_native_document(std::io::Cursor::new(bytes));assert_saved_artwork(&saved,&reopened);
        let epoch=state(&w).document_file.epoch;w.documents.enqueue(&w,(reopened,None));until(||state(&w).document_file.epoch!=epoch,"local-adjustment archive owner replacement");ready(&w);settled(&w);assert_eq!(sample(&w),changed);
        w.restart_gpu();ready(&w);settled(&w);assert_eq!(sample(&w),changed,"recreated GPU rebuilds live analysis");capture(&w,output,&format!("local-recreated-{width}-{theme:?}"));
        samples.push(json!({"theme":format!("{theme:?}"),"original":original,"shadows_highlights":adjusted,"clarity_positive":positive,"clarity_negative":negative,"dehaze_positive":dehaze_positive,"dehaze_negative":dehaze_negative,"lower_changed":changed}));
        for id in deleted {let handle=document(&w).artwork.occurrences.resolve(id).unwrap();w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Select {id:layer_ui::occurrence_token(handle),mask:false}});w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);}assert_source_scene(&w,&source);
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Adjustments,visible:true});w.customize(CustomizationAction::CloseExpanded);
        w.dispatch(UiAction::MovePanel {panel:Panel::Adjustments,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[width as f32,800.]});
        w.dispatch(UiAction::FilterPicker {action:layer_ui::FilterPickerAction::Category {category:None}});
        for (id,query) in [("shadows_highlights","Shadows"),("clarity","Clarity"),("dehaze","Dehaze")] {
            w.dispatch(UiAction::FilterPicker {action:layer_ui::FilterPickerAction::Search {query:query.into()}});pump(100);
            let row=named::<gtk::Button>(w.window.upcast_ref(),&format!("adjustment-{id}"));scroll_to(row.upcast_ref());
            let picture=descendant::<gtk::Picture>(&row).unwrap();until(||picture.paintable().is_some(),"local adjustment catalog thumbnail");
            let texture=picture.paintable().unwrap().downcast::<gtk::gdk::Texture>().unwrap();let mut bytes=vec![0;texture.width() as usize*texture.height() as usize*4];texture.download(&mut bytes,texture.width() as usize*4);
            assert!(bytes.chunks_exact(4).any(|p|p[..3].iter().max().unwrap()-p[..3].iter().min().unwrap()>20),"source-aware {id} thumbnail contains gradient colors");
            capture(&w,output,&format!("catalog-{id}-{width}-{theme:?}"));
        }
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Adjustments,visible:false});assert_source_scene(&w,&source);
    }
    std::fs::write(output.join(format!("local-pixels-{width}.json")),serde_json::to_vec_pretty(&samples).unwrap()).unwrap();input.finish();w.window.destroy();pump(100);
}
