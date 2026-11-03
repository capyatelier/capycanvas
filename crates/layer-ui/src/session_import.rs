use layer_core::{PortableId,package::session_store::check_store_key};
use serde::{Deserialize,Serialize};
use std::collections::BTreeMap;

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
pub struct SourceVersion {pub key:String,pub generation:u64,pub sha256:String}
impl SourceVersion {
    pub fn capture(key:String,generation:u64,metadata:&str)->Result<Self,String> {
        use sha2::{Digest,Sha256};
        let sha256=Sha256::digest(metadata.as_bytes()).iter().map(|byte|format!("{byte:02x}")).collect();
        let source=Self {key,generation,sha256};source.validate()?;Ok(source)
    }
    pub fn validate(&self)->Result<(),String> {
        check_store_key(&self.key)?;
        if self.generation==0 || self.generation>super::MAX_SESSION_DRAWING_ID
            || self.sha256.len()!=64 || !self.sha256.bytes().all(|byte|byte.is_ascii_digit()||(b'a'..=b'f').contains(&byte)) {return Err("Invalid imported session identity".into());}Ok(())
    }
}
#[derive(Clone,Debug,Serialize,Deserialize)]
pub struct ImportRecord {
    pub source:SourceVersion,pub drawing:String,pub window:String,pub baseline:Option<PortableId>,pub complete:bool,
}
#[derive(Default,Serialize,Deserialize)]
pub struct ImportLedger {pub records:BTreeMap<String,ImportRecord>,pub windows:BTreeMap<String,String>}
impl ImportLedger {
    pub fn validate(&self)->Result<(),String> {
        for (source,window) in &self.windows {check_store_key(source)?;check_store_key(window)?;}
        for (key,record) in &self.records {
            record.source.validate()?;check_store_key(&record.drawing)?;check_store_key(&record.window)?;
            if *key!=record.source.key || record.complete&&record.baseline.is_none() {return Err("Invalid imported session index".into());}
        }Ok(())
    }
    pub fn window(&mut self,source:&str)->Result<String,String> {
        check_store_key(source)?;Ok(self.windows.entry(source.into()).or_insert_with(||PortableId::random().to_string()).clone())
    }
    pub fn plan(&self,source:SourceVersion,current:Option<PortableId>,window:String)->Result<Option<ImportRecord>,String> {
        source.validate()?;
        let previous=self.records.get(&source.key);
        if previous.is_some_and(|previous|previous.complete&&previous.source==source) {return Ok(None);}
        let reuse=previous.is_some_and(|previous|previous.baseline==current);
        let drawing=if reuse {previous.unwrap().drawing.clone()}else {PortableId::random().to_string()};
        check_store_key(&window)?;
        Ok(Some(ImportRecord {source,drawing,window,baseline:if reuse {current}else {None},complete:false}))
    }
    pub fn stage(&mut self,record:ImportRecord) {self.records.insert(record.source.key.clone(),record);}
    pub fn retry_missing(&mut self,source:&str)->Result<(),String> {
        self.records.get_mut(source).ok_or("Missing imported session identity")?.complete=false;Ok(())
    }
    pub fn published(&mut self,source:&str,generation:PortableId,complete:bool)->Result<(),String> {
        let record=self.records.get_mut(source).ok_or("Missing imported session identity")?;
        record.baseline=Some(generation);record.complete=complete;Ok(())
    }
}
pub fn merge_window(previous:&super::SessionManifest,source:&super::SessionManifest,bindings:&BTreeMap<u64,String>)->Result<super::SessionManifest,String> {
    previous.validate()?;source.validate()?;
    let mut used:std::collections::BTreeSet<_>=previous.drawings.iter().map(|drawing|drawing.id).chain(source.drawings.iter().map(|drawing|drawing.id)).collect();
    let mut next=1;let mut imported=Vec::new();let mut mapping=BTreeMap::new();
    for drawing in &source.drawings {
        let Some(key)=bindings.get(&drawing.id) else {continue;};check_store_key(key)?;
        let id=if let Some(existing)=previous.drawings.iter().find(|existing|existing.key==*key) {existing.id}
            else if previous.drawings.iter().any(|existing|existing.id==drawing.id) {
                while used.contains(&next) {next=next.checked_add(1).ok_or("Drawing identities are exhausted")?;}used.insert(next);next
            }else {drawing.id};
        mapping.insert(drawing.id,id);imported.push(super::SessionDrawing {id,key:key.clone()});
    }
    let active=mapping.get(&source.active).copied().filter(|id|*id!=0).or_else(||(previous.active!=0).then_some(previous.active)).or_else(||imported.first().map(|drawing|drawing.id)).unwrap_or(0);
    let mut result=previous.stage(imported,active)?;
    for id in source.blocked.iter().copied().chain(source.restoring.iter().map(|attempt|attempt.id)) {
        if let Some(id)=mapping.get(&id) && !result.blocked.contains(id) {result.restoring.retain(|attempt|attempt.id!=*id);result.blocked.push(*id);}
    }
    if result.blocked!=previous.blocked && result.generation==previous.generation {result=result.next()?;}
    result.validate()?;Ok(result)
}
pub fn block_copy(previous:&super::SessionManifest,key:&str)->Result<super::SessionManifest,String> {
    let id=previous.drawings.iter().find(|drawing|drawing.key==key).ok_or("Missing imported drawing")?.id;
    if previous.blocked.contains(&id) {return Ok(previous.clone());}
    let pending=if previous.restoring.iter().any(|attempt|attempt.id==id) {previous.clone()}else {previous.begin_restore(id)?};
    let attempt=*pending.restoring.iter().find(|attempt|attempt.id==id).unwrap();pending.finish_restore(attempt,false)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn version(generation:u64)->SourceVersion {SourceVersion {key:"released".into(),generation,sha256:format!("{generation:064x}")}}
    #[test]
    fn unchanged_imports_stay_closed_and_two_edited_branches_keep_separate_drawings() {
        let mut ledger=ImportLedger::default();let record=ledger.plan(version(1),None,"window".into()).unwrap().unwrap();let key=record.drawing.clone();
        ledger.stage(record);let baseline=PortableId::random();ledger.published("released",baseline,true).unwrap();
        assert!(ledger.plan(version(1),None,"window".into()).unwrap().is_none());
        assert_eq!(ledger.plan(version(2),Some(baseline),"window".into()).unwrap().unwrap().drawing,key);
        let branch=ledger.plan(version(2),Some(PortableId::random()),"window".into()).unwrap().unwrap();assert_ne!(branch.drawing,key);
        assert_ne!(ledger.plan(version(2),None,"window".into()).unwrap().unwrap().drawing,key);
    }
    #[test]
    fn interrupted_copy_reuses_its_staged_identity_and_a_durable_prefix() {
        let mut ledger=ImportLedger::default();let record=ledger.plan(version(1),None,"window".into()).unwrap().unwrap();
        let (key,window)=(record.drawing.clone(),record.window.clone());ledger.stage(record);
        let retry=ledger.plan(version(1),None,"window".into()).unwrap().unwrap();
        assert_eq!(retry.drawing,key);assert_eq!(retry.window,window);
        let copied=PortableId::random();
        assert_ne!(ledger.plan(version(1),Some(copied),"window".into()).unwrap().unwrap().drawing,key);
        ledger.published("released",copied,false).unwrap();
        assert_eq!(ledger.plan(version(1),Some(copied),"window".into()).unwrap().unwrap().drawing,key);
        assert_ne!(ledger.plan(version(1),Some(PortableId::random()),"window".into()).unwrap().unwrap().drawing,key);
    }
    #[test]
    fn a_missing_destination_can_be_reimported_without_replacing_an_edited_branch() {
        let mut ledger=ImportLedger::default();let record=ledger.plan(version(1),None,"window".into()).unwrap().unwrap();
        let key=record.drawing.clone();ledger.stage(record);ledger.published("released",PortableId::random(),true).unwrap();
        ledger.retry_missing("released").unwrap();
        let retry=ledger.plan(version(1),None,"window".into()).unwrap().unwrap();
        assert_ne!(retry.drawing,key);assert_eq!(retry.baseline,None);assert!(!retry.complete);
        let mut value=serde_json::to_value(&ledger).unwrap();value["future_option"]=serde_json::json!({"enabled":true});
        value["records"]["released"]["source"]["future_option"]=serde_json::json!(1);
        serde_json::from_value::<ImportLedger>(value).unwrap().validate().unwrap();
    }
    #[test]
    fn importing_one_drawing_preserves_other_branches_and_blocks_only_the_failed_copy() {
        use super::super::{SessionManifest,SessionDrawing};
        let source=SessionManifest::default().stage(vec![SessionDrawing {id:1,key:"released".into()},SessionDrawing {id:2,key:"closed".into()}],2).unwrap();
        let previous=SessionManifest::default().stage(vec![SessionDrawing {id:1,key:"edited".into()}],1).unwrap();
        let bindings=[(1,"copy".into())].into_iter().collect();
        let merged=merge_window(&previous,&source,&bindings).unwrap();
        assert_eq!(merged.drawings.len(),2);assert!(merged.drawings.contains(&previous.drawings[0]));
        let copy=merged.drawings.iter().find(|drawing|drawing.key=="copy").unwrap();assert_ne!(copy.id,1);
        assert!(merged.drawings.iter().all(|drawing|drawing.key!="closed"));
        let failed=block_copy(&merged,"copy").unwrap();assert_eq!(failed.blocked,vec![copy.id]);
        assert!(failed.restoring.is_empty());assert_eq!(block_copy(&failed,"copy").unwrap(),failed);
        let repeated=merge_window(&failed,&source,&bindings).unwrap();assert_eq!(repeated.drawings,failed.drawings);
    }
}
