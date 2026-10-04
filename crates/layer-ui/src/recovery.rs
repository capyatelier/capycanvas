use serde::{Deserialize,Serialize};
use crate::SessionStamp;

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
pub struct RecoveryDocument {
    pub epoch:u64,
    pub revision:u64,
    pub modified:bool,
    pub busy:bool,
    pub session:SessionStamp,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum RecoveryWorkKind {Capture,Retire}
#[derive(Clone,Debug)]
pub struct RecoveryWork {pub token:u64,pub kind:RecoveryWorkKind,document:Option<RecoveryDocument>}
#[derive(Default)]
pub struct RecoveryState {
    checkpoint:Option<RecoveryDocument>,document:Option<RecoveryDocument>,owned:bool,closed:bool,
    next_token:u64,pending:Option<RecoveryWork>,retire:bool,
}
pub enum RecoveryEvent {
    Observe {document:RecoveryDocument,owned:bool},Ownership {owned:bool},Retire,
    Complete {token:u64,success:bool},Close,Resume,
}
#[derive(Default)]
pub struct RecoveryUpdate {pub work:Option<RecoveryWork>,pub busy:bool,pub current:bool}
impl RecoveryState {
    fn next(&mut self)->Result<Option<RecoveryWork>,String> {
        if self.pending.is_some() || !self.owned {return Ok(None);}
        let kind=if self.retire {RecoveryWorkKind::Retire} else {
            let Some(document)=self.document.as_ref() else {return Ok(None)};
            if self.closed || document.busy || self.checkpoint.as_ref()==Some(document) {return Ok(None);}
            RecoveryWorkKind::Capture
        };
        self.next_token=self.next_token.checked_add(1).ok_or("Recovery ticket identity exhausted")?;
        let work=RecoveryWork {token:self.next_token,kind,document:self.document.clone()};
        self.pending=Some(work.clone());Ok(Some(work))
    }
    pub fn event(&mut self,event:RecoveryEvent)->Result<RecoveryUpdate,String> {
        let mut advance=true;
        match event {
            RecoveryEvent::Observe{document,owned}=>{self.document=Some(document);self.owned=owned;}
            RecoveryEvent::Ownership{owned}=>self.owned=owned,
            RecoveryEvent::Retire=>{self.retire=true;self.checkpoint=None;}
            RecoveryEvent::Close=>self.closed=true,
            RecoveryEvent::Resume=>{
                if self.pending.is_some() {return Err("Recovery work is still pending".into());}
                self.closed=false;self.retire=false;self.checkpoint=None;
            }
            RecoveryEvent::Complete{token,success}=>{
                if self.pending.as_ref().is_none_or(|pending|pending.token!=token) {return Err("Stale recovery completion".into());}
                let work=self.pending.take().ok_or("Recovery ticket is missing")?;
                advance=success || (self.retire && work.kind==RecoveryWorkKind::Capture);
                match (work.kind,success) {
                    (RecoveryWorkKind::Capture,true) if !self.retire=>self.checkpoint=work.document,
                    (RecoveryWorkKind::Retire,true)=>{self.retire=false;self.checkpoint=work.document;}
                    _=>(),
                }
            }
        }
        let work=if advance {self.next()?}else{None};
        let busy=self.pending.is_some();
        Ok(RecoveryUpdate {work,busy,current:!busy && !self.retire && (self.closed || (self.document.is_some() && self.checkpoint==self.document))})
    }
}
impl<R:layer_render::CanvasRenderer> crate::UiSession<R> {
    pub fn recovery_document(&self)->RecoveryDocument {
        RecoveryDocument {epoch:self.state().document_file.epoch,revision:self.engine().document().revision,
            modified:self.state().document_file.modified,busy:self.recovery_file_busy() || self.require_raster_snapshot().is_err(),session:self.session_stamp()}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::session;
    fn document()->RecoveryDocument {session(crate::Platform::Gtk).recovery_document()}
    fn observe(state:&mut RecoveryState,document:&RecoveryDocument)->RecoveryUpdate {
        state.event(RecoveryEvent::Observe {document:document.clone(),owned:true}).unwrap()
    }
    fn complete(state:&mut RecoveryState,work:&RecoveryWork,success:bool)->RecoveryUpdate {
        state.event(RecoveryEvent::Complete {token:work.token,success}).unwrap()
    }
    #[test]
    fn clean_drawings_persist_and_unchanged_observations_do_not_write() {
        let mut state=RecoveryState::default();let document=document();assert!(!document.modified);
        let first=observe(&mut state,&document).work.unwrap();assert_eq!(first.kind,RecoveryWorkKind::Capture);
        assert!(complete(&mut state,&first,true).current);
        assert!(observe(&mut state,&document).work.is_none());
        for field in 0..5 {
            let mut next=document.clone();
            match field {0=>next.session.state.camera.zoom*=2.,1=>next.session.working_generation+=1,2=>next.session.state.saved_checkpoint+=1,
                3=>next.session.state.unsaved_name=Some("A different name".into()),4=>next.session.state.recovered=true,_=>unreachable!()}
            let ticket=observe(&mut state,&next).work.unwrap();assert!(complete(&mut state,&ticket,true).current);
        }
    }
    #[test]
    fn older_completions_never_acknowledge_later_edits_and_failures_retry() {
        let mut state=RecoveryState::default();let mut document=document();
        let first=observe(&mut state,&document).work.unwrap();document.session.revision+=1;
        assert!(observe(&mut state,&document).work.is_none());
        let update=complete(&mut state,&first,true);assert!(!update.current);
        let latest=update.work.unwrap();assert!(!complete(&mut state,&latest,false).current);
        let retry=observe(&mut state,&document).work.unwrap();
        assert!(state.event(RecoveryEvent::Complete{token:first.token,success:true}).is_err());
        assert!(complete(&mut state,&retry,true).current);
    }
    #[test]
    fn close_waits_for_accepted_work_and_retirement_failure_never_reports_safe() {
        for captured in [false,true] {
            let mut state=RecoveryState::default();let document=document();let capture=observe(&mut state,&document).work.unwrap();
            assert!(state.event(RecoveryEvent::Retire).unwrap().work.is_none());state.event(RecoveryEvent::Close).unwrap();
            let retire=complete(&mut state,&capture,captured).work.unwrap();assert_eq!(retire.kind,RecoveryWorkKind::Retire);
            assert!(!complete(&mut state,&retire,false).current);
            let retry=state.event(RecoveryEvent::Retire).unwrap().work.unwrap();assert!(state.event(RecoveryEvent::Resume).is_err());
            assert!(complete(&mut state,&retry,true).current);
            assert_eq!(state.event(RecoveryEvent::Resume).unwrap().work.unwrap().kind,RecoveryWorkKind::Capture);
        }
    }
    #[test]
    fn busy_or_unowned_drawings_never_start_storage_and_tickets_do_not_wrap() {
        let mut state=RecoveryState::default();let mut document=document();document.busy=true;
        assert!(observe(&mut state,&document).work.is_none());document.busy=false;
        assert!(state.event(RecoveryEvent::Observe{document,owned:false}).unwrap().work.is_none());
        state.next_token=u64::MAX;
        assert!(state.event(RecoveryEvent::Ownership{owned:true}).is_err());assert!(state.pending.is_none());
    }
}
