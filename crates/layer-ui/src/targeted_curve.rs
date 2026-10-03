use super::*;
use layer_core::{ArtworkQuery,ArtworkSample,ArtworkSampleRequest,ArtworkSource,EffectValue,Layer,Point};

pub(super) struct TargetedCurve {
    layer:LayerId,
    epoch:u64,
    document_epoch:u64,
    page:u8,
    contact:Option<Contact>,
}
struct Contact {
    pointer:(PointerKind,u64),
    request:ArtworkSampleRequest,
    expected:Layer,
    start:f32,
    current:f32,
    scale:f32,
    released:bool,
    submitted:bool,
    point:Option<(Vec<[f32;2]>,usize)>,
}
impl<R:CanvasRenderer> UiSession<R> {
    pub(super) fn targeted_curve_busy(&self)->bool {self.targeted_curve.as_ref().is_some_and(|mode|mode.contact.is_some())}
    pub(super) fn cancel_targeted_contact(&mut self)->Result<(),String> {
        if let Some(contact)=self.targeted_curve.as_mut().and_then(|mode|mode.contact.take()) {
            if contact.submitted {self.engine.backend_mut().cancel_snapshot();}
            if contact.point.is_some() {self.cancel_effect_gesture()?;}
        }
        self.sync_targeted_page();Ok(())
    }
    pub(super) fn sync_targeted_page(&mut self) {
        if let Some(mode)=&mut self.targeted_curve {
            mode.epoch=self.state.layer_properties.epoch;
            mode.page=match self.state.layer_properties.page.as_deref() {Some("red")=>1,Some("green")=>2,Some("blue")=>3,_=>0};
        }
    }
    pub(super) fn start_targeted_curve(&mut self,layer:u64,epoch:u64)->Result<(),String> {
        if self.state.platform!=Platform::Gtk {return Ok(());}
        if self.targeted_curve.is_some() {self.cancel_picker();return Ok(());}
        self.require_idle()?;
        let document=self.engine.document();
        if document.is_locked(LayerId(layer)) || document.layer(LayerId(layer)).and_then(|l|l.effect.as_ref()).is_none_or(|e|e.program.id.as_ref()!="curves") {return Err("Choose a Curves adjustment".into());}
        self.cancel_picker();self.cancel_auto_levels();self.cancel_histogram();self.start_picker()?;
        self.eyedropper.cancel();self.eyedropper.layer=false;
        self.targeted_curve=Some(TargetedCurve {layer:LayerId(layer),epoch,document_epoch:self.state.document_file.epoch,page:0,contact:None});
        self.sync_targeted_page();self.refresh_tools();Ok(())
    }
    pub(super) fn targeted_curve_input(&mut self,input:&UiInput)->Result<Option<UiChange>,String> {
        let changed=regions::DOCUMENT|regions::BRUSH|regions::COMMANDS|regions::COLOR_PREVIEW;
        match *input {
            UiInput::Blur=>{self.cancel_picker();return Ok(None);},
            UiInput::Key {ref key,pressed:true,editing:false,..} if key.eq_ignore_ascii_case("escape")=>{self.cancel_picker();},
            UiInput::ColorPickerHold {..}=>(),
            UiInput::Pointer {id,kind,phase,button,position,..}=> {
                if position.iter().any(|v|!v.is_finite()) {return Ok(Some(UiChange::default()));}
                let Some(mode)=&self.targeted_curve else {return Ok(None);};
                if phase==ContactPhase::Down && button==PointerButton::Primary && mode.contact.is_none() {
                    let document=self.engine.document();let original=document.layer(mode.layer).ok_or("The adjustment was removed")?.clone();
                    let point=self.state.camera.input_transform().map(Point {x:position[0],y:position[1]});
                    let mut query=ArtworkQuery::new(document,ArtworkSource::EffectInput(mode.layer));query.time=self.engine.animation_time();
                    let scale=self.logical_viewport.map_or(1.,|logical|self.state.camera.viewport[0] as f32/logical[0]);
                    self.targeted_curve.as_mut().unwrap().contact=Some(Contact {pointer:(kind,id),request:ArtworkSampleRequest {query,position:[point.x,point.y],width:5},expected:original,start:position[1],current:position[1],scale,released:false,submitted:false,point:None});
                } else if mode.contact.as_ref().is_some_and(|contact|contact.pointer==(kind,id) && !contact.released) {
                    if phase==ContactPhase::Cancel {self.cancel_targeted_contact()?;}
                    else if matches!(phase,ContactPhase::Move|ContactPhase::Up) {
                        let contact=self.targeted_curve.as_mut().unwrap().contact.as_mut().unwrap();contact.current=position[1];contact.released=phase==ContactPhase::Up;
                        self.advance_targeted_curve()?;
                    }
                } else {return Ok(Some(UiChange::default()));}
            },
            _=>return Ok(None),
        }
        Ok(Some(self.changed(changed,true)))
    }
    fn advance_targeted_curve(&mut self)->Result<(),String> {
        let Some(mut mode)=self.targeted_curve.take() else {return Ok(());};
        let outcome=(|| {
            let Some(contact)=mode.contact.as_mut() else {return Ok(());};
            let Some((points,index))=&contact.point else {return Ok(());};
            let mut changed=points.clone();changed[*index][1]=(points[*index][1]+(contact.start-contact.current)/(contact.scale*255.)).clamp(0.,1.);
            let key=format!("curve_{}",mode.page);
            let action=EffectAction::Set {layer:mode.layer.0,key:key.clone(),value:EffectValue::Curve(changed.clone())};
            let started=self.effect_gesture.is_some();
            self.effect_gesture_action(if started {ContactPhase::Move} else {ContactPhase::Down},action.clone())?;
            self.property_editor.select(mode.layer.0,mode.epoch,&key,Some(*index),&changed);
            if contact.released {self.effect_gesture_action(ContactPhase::Up,action)?;mode.contact=None;mode.epoch=self.state.layer_properties.epoch;}
            else {contact.expected=self.engine.document().layer(mode.layer).ok_or("The adjustment was removed")?.clone();}
            Ok(())
        })();
        self.targeted_curve=Some(mode);outcome
    }
    pub(super) fn poll_targeted_curve(&mut self)->u32 {
        let Some(mut mode)=self.targeted_curve.take() else {return 0;};
        let current=self.state.document_file.epoch==mode.document_epoch && self.engine.document().active_layer==mode.layer
            && self.property_editor.accepts(mode.layer.0,mode.epoch)
            && mode.contact.as_ref().is_none_or(|c|c.request.query.matches_source(self.engine.document())
                && self.engine.document().layer(mode.layer).is_some_and(|layer|layer.same_artwork(&c.expected)));
        if !current {self.targeted_curve=Some(mode);self.cancel_picker();return regions::DOCUMENT|regions::BRUSH|regions::COMMANDS;}
        let Some(contact)=mode.contact.as_mut() else {self.targeted_curve=Some(mode);return 0;};
        let mut ready=false;
        let result=if contact.point.is_some() {Ok(())}
        else if contact.submitted {
            if let Some(result)=self.engine.backend_mut().take_snapshot() {
                contact.submitted=false;
                result.map_err(error).and_then(|result| {
                    let layer_render::SnapshotResult::ArtworkSample(ArtworkSample::Color([r,g,b,_]))=result else {return Err(self.localization().text(MessageId::RESOURCES_PICKER_EMPTY).to_string());};
                    let effect=contact.expected.effect.as_ref().ok_or("The adjustment was removed")?;
                    contact.point=Some(layer_core::curves::targeted_curve_point(effect,[r,g,b],contact.request.document.color.space,mode.page).map_err(|_|self.localization().text(MessageId::RESOURCES_CALIBRATION_FAILED).to_string())?);
                    ready=true;Ok(())
                })
            } else {Ok(())}
        } else {
            self.engine.backend_mut().request_snapshot(layer_render::SnapshotRequest::ArtworkSample(contact.request.clone())).map(|submitted|contact.submitted=submitted).map_err(error)
        };
        self.targeted_curve=Some(mode);
        if let Err(reason)=result.and_then(|()|if ready {self.advance_targeted_curve()} else {Ok(())}) {
            let _=self.cancel_targeted_contact();self.raise_notice(reason,None);return regions::DOCUMENT;
        }
        if ready {regions::DOCUMENT|regions::BRUSH|regions::COMMANDS} else {0}
    }
}
