use super::*;
use layer_ui::{CommandId,Platform,PreferenceAction,PreferenceId,PreferenceValue,UiAction,UiSession};

struct Fit {session:UiSession<WgpuRasterizer>,presenter:ViewportPresenter,target:wgpu::TextureView,clock:std::time::Instant}

#[derive(Default,Debug)]
struct Served {frames:u64,busy:u64,presents:u64,first_present:Option<u64>,level_zero_captures:usize,misses:u64,contacts:Vec<u64>,ended:bool}

fn photo_document(extent:[u32;2])->layer_core::Document {
    let mut document=layer_core::Document::new(PortableId::random(),extent[0],extent[1],layer_core::DocumentNames {paint:"Photo".into(),paper:"Paper".into()});
    composition_mut(&mut document).color=DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::U8};
    let image=rgba8_source(extent,|x,y|{
        let h=(x.wrapping_mul(73856093)^y.wrapping_mul(19349663)).wrapping_mul(2654435761);
        [((x/7) as u8).wrapping_add((h&15) as u8),((y/5) as u8).wrapping_add(((h>>4)&15) as u8),((x+y)/11) as u8,255]
    });
    paint_mut(&mut document,0).base=Some(PaintBase::new(image.into()));
    let layer={
        use layer_core::authored::*;
        let artwork=&mut document.artwork;
        let source=artwork.paint.insert(PortableId::random(),PaintSource {color_mode:Default::default(),domain:extent,raster:Default::default(),base:None,operations:Arc::default()}).unwrap();
        let occurrence=artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Paint(source),"Paint")).unwrap();
        let stack=artwork.compositions.get(artwork.root).unwrap().result;
        artwork.stacks.get_mut(stack).unwrap().entries.insert(0,occurrence);
        occurrence
    };
    let mut document=layer_core::Document::from_artwork(document.artwork).unwrap();
    document.working.occurrence=Some(layer);
    document.working.target=document.scene().source_target(layer);
    document
}

impl Fit {
    fn new(document:layer_core::Document)->Self {
        let mut renderer=crate::memory::memory_tests::legacy_scene_renderer(document.composition().color);
        renderer.set_preparation_wake(Arc::new(||{}));
        let presenter=crate::test_support::float_presenter(&renderer);
        let (_texture,target)=create_target(&renderer.device,[2400,1600],wgpu::TextureFormat::Rgba32Float,"reduced settle target");
        let working=document.working.occurrence;
        let mut session=UiSession::new(renderer,document,[2400,1600],Platform::Android).unwrap();
        session.dispatch(UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Feedback,value:PreferenceValue::Bool(false)}}).unwrap();
        if let Some(layer)=working {session.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Select {id:layer_ui::occurrence_token(layer),mask:false}}).unwrap();}
        session.dispatch(UiAction::SelectBrush {id:layer_core::DefaultBrushPreset::GPen as u32}).unwrap();
        session.dispatch(UiAction::SetBrushSize {value:512.}).unwrap();
        session.dispatch(UiAction::Invoke {command:CommandId::FitCanvas}).unwrap();
        Self {session,presenter,target,clock:std::time::Instant::now()}
    }
    fn renderer(&self)->&WgpuRasterizer {self.session.engine().backend()}
    fn frame(&mut self,served:&mut Served) {
        let now=1_000_000_000+self.clock.elapsed().as_nanos() as u64;
        let misses=self.renderer().metrics().source_tile_misses;
        served.busy+=u64::from(!self.renderer().settled());
        self.session.frame(now,now).unwrap();
        crate::test_support::complete(self.session.renderer_mut());
        served.frames+=1;
        let r=self.session.engine().backend();
        served.misses+=r.metrics().source_tile_misses-misses;
        served.level_zero_captures+=r.metrics.frame_composited_pages.iter().filter(|(level,_)|*level==0).count();
        let contact=r.scale_display.as_ref().and_then(|cache|cache.contact_id());
        match (served.contacts.last(),contact) {
            (Some(_),None)=>served.ended|=self.session.engine().has_active_stroke(),
            (last,Some(id)) if last!=Some(&id)=>served.contacts.push(id),
            _=>{},
        }
        let view=self.session.camera().view();
        if self.presenter.needs_present(r,view,[0.;4]) {
            self.presenter.present(r,&self.target,view,[0.;4]).unwrap();
            served.presents+=1;served.first_present.get_or_insert(served.frames);
            crate::test_support::complete(r);
        }
        self.session.renderer_mut().after_present();
    }
    fn settled(&self)->bool {
        let engine=self.session.engine();
        !self.session.holding_input() && !engine.has_pending_input() && !engine.has_pending_document_edits() && !engine.raster_backing_pending()
            && !engine.has_active_stroke() && self.session.require_document_idle().is_ok() && self.renderer().settled() && self.session.frame_demand()==layer_render::FrameDemand::Wait
    }
    fn serve(&mut self,frames:u64,action:Option<UiAction>)->Served {
        let mut served=Served::default();
        if let Some(action)=action {self.session.dispatch(action).unwrap();self.frame(&mut served);}
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(600);
        let mut quiet=0;
        loop {
            let due=match self.session.frame_demand() {
                layer_render::FrameDemand::Wait=>false,
                layer_render::FrameDemand::WaitUntil(at)=>web_time::Instant::now()>=at,
                _=>true,
            };
            if due {self.frame(&mut served);quiet=0;}
            else if self.settled() {quiet+=1;if quiet>20 {return served;}std::thread::sleep(std::time::Duration::from_millis(1));}
            else {std::thread::sleep(std::time::Duration::from_millis(1));}
            assert!(served.frames<=frames,"the view must settle within {frames} frames: {served:?}, {:?}, {}",self.renderer().outstanding_terms(),self.session.rendering_diagnostics());
            assert!(std::time::Instant::now()<deadline,"the view must settle: {}",self.session.rendering_diagnostics());
        }
    }
    fn ellipse(&mut self,first:u64,scale:f32,color:[f32;4])->Served {
        self.session.dispatch(UiAction::SetColor {rgba:color}).unwrap();
        let [w,h]=self.session.engine().document().composition().size.map(|v|v as f32);
        let mut served=Served::default();
        let revision=self.session.engine().document().revision;
        let count=48u64;
        for step in 0..=count {
            let angle=step as f32/count as f32*std::f32::consts::TAU;
            let point=[w*0.5+w*scale*angle.cos(),h*0.5+h*scale*angle.sin()];
            let phase=match step {0=>PenPhase::Down,s if s==count=>PenPhase::Up,_=>PenPhase::Move};
            let [a,b,c,d,x,y]=self.session.camera().document_to_surface();
            let event=PenEvent {timestamp_ns:1_000_000_000+self.clock.elapsed().as_nanos() as u64,view_revision:self.session.camera().revision,
                pressure:if phase==PenPhase::Up {0.}else {1.},
                ..crate::test_support::pen(first+step,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)};
            self.session.pen(event);
            self.frame(&mut served);
        }
        let settle=self.serve(u64::MAX,None);
        served.level_zero_captures+=settle.level_zero_captures;
        assert_ne!(self.session.engine().document().revision,revision,"the stroke commits");
        served
    }
    fn level(&self)->u32 {self.renderer().scale_display.as_ref().unwrap().plan.level}
}

#[test]
fn an_undo_at_fit_composes_no_level_zero_cell_and_shows_its_whole_picture_once() {
    let mut fit=Fit::new(photo_document([9504,6336]));
    fit.serve(u64::MAX,None);
    assert_eq!(fit.level(),2,"the fixture views a 9504 x 6336 photo at Fit on level 2");
    fit.ellipse(1,0.3,[0.9,0.2,0.1,1.]);
    let undo=fit.serve(40,Some(UiAction::Invoke {command:CommandId::Undo}));
    assert_eq!(undo.level_zero_captures,0,"an Undo below 100% settles at the view level: {undo:?}");
    assert_eq!(undo.presents,1,"an Undo shows its whole picture once: {undo:?}");
}

#[test]
fn a_cold_fit_composes_no_level_zero_cell_and_reads_each_present_tile_once() {
    let mut fit=Fit::new(photo_document([9504,6336]));
    fit.serve(u64::MAX,None);
    fit.ellipse(1,0.3,[0.9,0.2,0.1,1.]);
    fit.serve(u64::MAX,Some(UiAction::SetZoom {zoom:1.}));
    {
        let r=fit.session.renderer_mut();
        let scene=r.scene.as_ref().unwrap();for _ in 0..2 {scene.advance_demand_epoch();}scene.trim_pages_by(u64::MAX,crate::residency::Class::Visible);
        crate::test_support::evict_sources(r);
    }
    let cold=fit.serve(60,Some(UiAction::Invoke {command:CommandId::FitCanvas}));
    let tiles=u64::from(9504u32.div_ceil(PAGE_SIZE)*6336u32.div_ceil(PAGE_SIZE));
    assert_eq!(cold.level_zero_captures,0,"a cold Fit below 100% composes each layer's reduced pages: {cold:?}");
    assert!(cold.misses<=2*tiles,"each present tile of the photo and the paint is read once: {cold:?}");
}

#[test]
fn a_blank_panorama_at_fit_settles_in_a_few_frames_without_level_zero_cells() {
    let document=layer_core::Document::new(PortableId::random(),layer_core::MAX_EXTENT,layer_core::MAX_EXTENT/2,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
    let mut fit=Fit::new(document);
    let open=fit.serve(u64::MAX,None);
    assert_eq!(open.level_zero_captures,0,"a blank panorama at Fit settles at the view level: {open:?}");
    assert!(open.busy<=8,"a blank panorama at Fit settles in a few display frames: {open:?}");
}

#[test]
fn a_stroke_at_fit_over_a_photo_builds_view_level_halves_and_keeps_one_contact() {
    let mut fit=Fit::new(photo_document([9504,6336]));
    fit.serve(u64::MAX,None);
    let stroke=fit.ellipse(1,0.3,[0.1,0.3,0.9,1.]);
    assert_eq!(stroke.level_zero_captures,0,"a stroke below 100% builds its halves at the view level: {stroke:?}");
    assert!(!stroke.contacts.is_empty() && !stroke.ended,"the stroke enters contact and keeps it across cells: {stroke:?}");
}

fn web_renderer(session:&UiSession<crate::AttachedRenderer>)->&WgpuRasterizer {session.engine().backend().0.as_deref().unwrap()}

#[test]
fn a_stroke_held_on_its_memory_under_a_web_budget_ends_with_its_commit_or_one_notice() {
    let ellipse=|count:u64,turns:f32,center:[f32;2],radius:[f32;2]|move|step:u64|{
        let angle=step as f32/count as f32*turns*std::f32::consts::TAU;
        [center[0]+radius[0]*angle.cos(),center[1]+radius[1]*angle.sin()]
    };
    let cases:[(&str,[i64;2],layer_core::DefaultBrushPreset,f32,bool,u64,Box<dyn Fn(u64)->[f32;2]>);2]=[
        ("G-Pen on a moved photo",[2112,0],layer_core::DefaultBrushPreset::GPen,2048.,true,250,Box::new(ellipse(250,1.25,[4752.,3168.],[3963.5,1408.]))),
        ("Liquify after a primer",[0,0],layer_core::DefaultBrushPreset::LiquifyPush,512.,false,150,Box::new(ellipse(150,1.,[4752.,3168.],[2851.2,1900.8]))),
    ];
    for (case,offset,brush,size,feedback,count,point) in cases {
        let mut document=photo_document([9504,6336]);
        let photo=document.scene().order()[1];
        document.artwork.occurrences.get_mut(photo).unwrap().offset=offset;
        let mut document=layer_core::Document::from_artwork(document.artwork).unwrap();
        (document.working.occurrence,document.working.target)=(Some(photo),document.scene().source_target(photo));
        let budget=layer_core::MemoryBudget::with_facts(layer_core::MemoryFacts {host_default_bytes:Some(2<<30),override_bytes:Some(2<<30),..Default::default()});
        let mut r=crate::memory::memory_tests::renderer_in_budget(document.composition().color,
            layer_core::DeviceMemoryFacts {unified:true,budget_bytes:Some(2<<30),..Default::default()},budget).unwrap();
        r.set_preparation_wake(Arc::new(||{}));
        let mut presenter=crate::test_support::float_presenter(&r);
        let (_texture,target)=create_target(&r.device,[2880,1590],wgpu::TextureFormat::Rgba32Float,"Web budget stroke target");
        let mut session=UiSession::new(crate::AttachedRenderer(Some(Box::new(r))),document,[2880,1590],Platform::Web).unwrap();
        for action in [UiAction::Layer {action:layer_ui::LayerAction::Select {id:layer_ui::occurrence_token(photo),mask:false}},
            UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Feedback,value:PreferenceValue::Bool(false)}},
            UiAction::SelectBrush {id:brush as u32},UiAction::SetBrushSize {value:size},UiAction::Invoke {command:CommandId::FitCanvas}] {session.dispatch(action).unwrap();}
        let clock=std::time::Instant::now();
        let now=||1_000_000_000+clock.elapsed().as_nanos() as u64;
        let mut frame=|session:&mut UiSession<crate::AttachedRenderer>,complete:bool| {
            let at=now();
            session.frame(at,at).unwrap();
            let r=web_renderer(session);
            if complete {crate::test_support::complete(r);}
            let view=session.camera().view();
            if presenter.needs_present(r,view,[0.;4]) {presenter.present(r,&target,view,[0.;4]).unwrap();if complete {crate::test_support::complete(r);}}
            session.renderer_mut().0.as_deref_mut().unwrap().after_present();
        };
        let idle=|session:&UiSession<crate::AttachedRenderer>|session.frame_demand()==layer_render::FrameDemand::Wait && !session.engine().has_pending_input()
            && !session.engine().has_active_stroke() && web_renderer(session).settled();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
        while !idle(&session) {assert!(std::time::Instant::now()<deadline,"{case}: the photo opens");frame(&mut session,true);}
        session.dispatch(UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Feedback,value:PreferenceValue::Bool(feedback)}}).unwrap();
        let pen=|session:&mut UiSession<crate::AttachedRenderer>,sequence:u64,phase:PenPhase,point:[f32;2]| {
            let [a,b,c,d,x,y]=session.camera().document_to_surface();
            session.pen(PenEvent {timestamp_ns:now(),view_revision:session.camera().revision,pressure:if phase==PenPhase::Up {0.}else {1.},
                ..crate::test_support::pen(sequence,phase,[a*point[0]+c*point[1]+x,b*point[0]+d*point[1]+y],SampleFlags::PRIMARY)});
        };
        let opened=session.engine().document().revision;
        for step in 0..=8u64 {
            pen(&mut session,step+1,match step {0=>PenPhase::Down,8=>PenPhase::Up,_=>PenPhase::Move},[9504.*(0.5+0.025*step as f32/8.),3168.]);
            frame(&mut session,true);
        }
        while session.engine().document().revision==opened {assert!(std::time::Instant::now()<deadline,"{case}: the primer commits");frame(&mut session,true);}
        let (start,told)=(session.engine().document().revision,session.engine().metrics().stroke_memory_refusals);
        for step in 0..=count {
            pen(&mut session,step+10,if step==0 {PenPhase::Down}else if step==count {PenPhase::Up}else {PenPhase::Move},point(step));
            if (step%10==0 || step==count) && session.frame_demand()!=layer_render::FrameDemand::Wait {frame(&mut session,false);}
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ended=|session:&UiSession<crate::AttachedRenderer>|!session.engine().has_pending_input() && !session.engine().has_active_stroke();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
        while !ended(&session) && std::time::Instant::now()<deadline {
            if session.frame_demand()!=layer_render::FrameDemand::Wait {frame(&mut session,false);} else {std::thread::sleep(std::time::Duration::from_millis(5));}
        }
        let (engine,r)=(session.engine(),web_renderer(&session));
        let (told,committed)=(engine.metrics().stroke_memory_refusals-told,engine.document().revision-start);
        assert!(ended(&session),"{case}: the stroke ends instead of waiting with no frame asked: active {}, revision {start} -> {}, notices {told}, {:?}, {}",
            engine.has_active_stroke(),engine.document().revision,r.outstanding_terms(),session.rendering_diagnostics());
        assert!(told<=1 && committed<=1 && (told==1 || committed==1),"{case}: the stroke commits once or the painter is told once: {told} notices, {committed} revisions");
    }
}
