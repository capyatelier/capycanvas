use super::{artwork_records, resources::{ResourceInventory, ResourceReader},
    registry::RecordKind, session::{EditorCapture, OpenSession, SessionMetadata, WorkingRecord, ObjectVersion, validate_history_checkpoints,decode_profiles}, transfer::{PreparedTransfer, TransferDescriptor, TransferReceiver}};
use crate::{authored::*, Document, Edit, Editor, HistoryEntry, ProjectLimits, RecordChange};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::{BTreeMap, BTreeSet}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeRecord {kind:String,handle:u32,id:PortableId,value:Option<ObjectVersion>}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryRecord {changes:Vec<ChangeRecord>,working:Option<WorkingRecord>,checkpoint:u64}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTransferDescriptor {
    artwork:TransferDescriptor,working:WorkingRecord,undo:Vec<EntryRecord>,redo:Vec<EntryRecord>,
    next_checkpoint:u64,next_stroke_id:u64,metadata:Value,metadata_profiles:Vec<Value>,tile_chunks:Vec<Vec<Value>>,extensions:BTreeMap<PortableId,Value>,extension_resources:Vec<PortableId>,
}
pub struct PreparedSessionTransfer {descriptor:SessionTransferDescriptor,artwork:PreparedTransfer}
pub struct SessionTransferReceiver {descriptor:SessionTransferDescriptor,artwork:TransferReceiver}
fn active(cancel:&AtomicBool)->Result<(),String> {if cancel.load(Ordering::Relaxed){Err("Session transfer cancelled".into())}else{Ok(())}}

impl EditorCapture {
    pub fn retained_tiles(&self)->crate::raster_storage::RetainedTiles {
        let editor=Editor {document:self.document.clone(),undo:self.undo.iter().map(|(edit,checkpoint)|HistoryEntry::new(edit.clone(),*checkpoint)).collect(),
            redo:self.redo.iter().map(|(edit,checkpoint)|HistoryEntry::new(edit.clone(),*checkpoint)).collect(),checkpoint:self.artwork.checkpoint.edit_checkpoint,next_checkpoint:self.next_checkpoint};
        editor.retained_tiles()
    }
}
#[derive(Default)]
struct ChangeRecords {records:BTreeMap<PortableId,ChangeRecord>,working:Option<WorkingRecord>}
fn changes(edit:&Edit,context:&Document,resources:&mut ResourceInventory,cancel:&AtomicBool,chunks:&mut Vec<Vec<Value>>,versions:&mut BTreeMap<Vec<u8>,usize>,out:&mut ChangeRecords)->Result<(),String> {
    active(cancel)?;
    macro_rules! record {($change:expr,$kind:ident,$store:ident,$variant:ident)=>{{let change=$change;
        let final_change=RecordChange::replace(&context.artwork.$store,change.handle,context.artwork.$store.get(change.handle).cloned()).map_err(str::to_string)?;
        let value=final_change.value.as_ref().map(|_|artwork_records::encode_change(&context.artwork,&Edit::$variant(final_change.clone()),resources,cancel).and_then(|record|ObjectVersion::capture(record,chunks,versions))).transpose()?;
        let kind=value.as_ref().map_or_else(||RecordKind::$kind.descriptor().type_id.to_owned(),|value|value.record["type"].as_str().unwrap().to_owned());
        out.records.insert(change.id,ChangeRecord {kind,handle:change.handle.index(),id:change.id,value});
    }}}
    match edit {
        Edit::Composition(c)=>record!(c,Composition,compositions,Composition),Edit::Stack(c)=>record!(c,Stack,stacks,Stack),
        Edit::Occurrence(c)=>record!(c,Occurrence,occurrences,Occurrence),Edit::Paint(c)=>record!(c,PaintSource,paint,Paint),
        Edit::Coverage(c)=>record!(c,CoverageSource,coverage,Coverage),Edit::Effect(c)=>record!(c,Effect,effects,Effect),
        Edit::ImageObject(c)=>record!(c,ImageObject,objects,ImageObject),Edit::SavedSelection(c)=>record!(c,Selection,selections,SavedSelection),
        Edit::Guides(c)=>record!(c,Guides,guides,Guides),Edit::Output(c)=>record!(c,Output,outputs,Output),
        Edit::Working(_)=>out.working=Some(WorkingRecord::capture(&context.working,resources)?),
        Edit::Batch(edits)=>for edit in edits {changes(edit,context,resources,cancel,chunks,versions,out)?;},
        Edit::SetRaster {target,revision:_}=>{
            let replacement=match target {
                SourceTarget::Paint(handle)=>Edit::Paint(RecordChange::replace(&context.artwork.paint,*handle,context.artwork.paint.get(*handle).cloned()).map_err(str::to_string)?),
                SourceTarget::Coverage(handle)=>Edit::Coverage(RecordChange::replace(&context.artwork.coverage,*handle,context.artwork.coverage.get(*handle).cloned()).map_err(str::to_string)?),
                SourceTarget::Selection(_)=>return Err("Selection has no raster history target".into()),
            };
            changes(&replacement,context,resources,cancel,chunks,versions,out)?;
        },
    }
    Ok(())
}
impl PreparedSessionTransfer {
    pub fn capture(capture:&EditorCapture,metadata:impl Into<SessionMetadata>,cancel:&AtomicBool)->Result<Self,String> {
        active(cancel)?;capture.validate_identity()?;
        let EditorCapture {artwork,document,undo,redo,next_checkpoint,captured_checkpoint}=capture;
        if *captured_checkpoint!=artwork.checkpoint {return Err("Session capture checkpoint changed".into());}
        let Document {artwork:_,working:_,revision:_,owner:_,scene_index:_,next_stroke_id}=document;
        let mut resources=ResourceInventory::for_transfer();
        let metadata=metadata.into();let metadata_profiles=metadata.encode_profiles(&mut resources)?;
        let working=WorkingRecord::capture(&document.working,&mut resources)?;let mut tile_chunks=Vec::new();let mut chunk_versions=BTreeMap::new();
        fn history(document:&Document,entries:&[(Edit,u64)],resources:&mut ResourceInventory,chunks:&mut Vec<Vec<Value>>,versions:&mut BTreeMap<Vec<u8>,usize>,cancel:&AtomicBool)->Result<Vec<EntryRecord>,String> {
            let mut context=document.clone();let mut result=Vec::with_capacity(entries.len());
            for (edit,checkpoint) in entries.iter().rev() {
                active(cancel)?;context.apply_records(edit.clone()).map_err(|e|e.to_string())?;
                let mut records=ChangeRecords::default();
                changes(edit,&context,resources,cancel,chunks,versions,&mut records)?;
                result.push(EntryRecord {changes:records.records.into_values().collect(),working:records.working,checkpoint:*checkpoint});
            }
            Ok(result)
        }
        let mut current=document.clone();current.artwork=(*artwork.artwork).clone();
        let undo_records=history(&current,undo,&mut resources,&mut tile_chunks,&mut chunk_versions,cancel)?;let redo_records=history(&current,redo,&mut resources,&mut tile_chunks,&mut chunk_versions,cancel)?;
        let mut roots=crate::RootInventory::default();for (edit,_) in undo.iter().chain(redo) {edit.roots(&mut roots);}
        let luts=roots.resources.into_iter().filter_map(|lut|lut.resource().map(|r|(r.id(),lut.clone()))).collect();
        let mut live=artwork.clone();Arc::make_mut(&mut live.artwork).extensions=Arc::default();
        let mut prepared=PreparedTransfer::capture_with_additional(&live,&None,resources,luts,cancel)?;
        for resource in artwork.artwork.extensions.resources.values() {prepared.append_private_resource(resource.clone())?;}
        let descriptor=SessionTransferDescriptor {artwork:prepared.descriptor().clone(),working,undo:undo_records,redo:redo_records,next_checkpoint:*next_checkpoint,next_stroke_id:*next_stroke_id,metadata:metadata.value,metadata_profiles,tile_chunks,
            extensions:artwork.artwork.extensions.records.clone(),extension_resources:artwork.artwork.extensions.resources.keys().copied().collect()};
        validate(&descriptor,ProjectLimits::default())?;active(cancel)?;
        Ok(Self {descriptor,artwork:prepared})
    }
    pub fn descriptor(&self)->&SessionTransferDescriptor {&self.descriptor}
    pub fn payload_count(&self)->usize {self.artwork.payload_count()}
    pub fn payload_len(&self,index:usize)->Result<u64,String> {self.artwork.payload_len(index)}
    pub fn read_chunk(&self,index:usize,offset:u64,length:usize)->Result<Vec<u8>,String> {self.artwork.read_chunk(index,offset,length)}
    pub fn adopt_verified(&self,limits:ProjectLimits,cancel:&AtomicBool)->Result<OpenSession,String> {
        active(cancel)?;validate(&self.descriptor,limits)?;
        let record=&self.descriptor;
        let (capture,_,(working,undo,redo,profiles))=self.artwork.adopt_verified_with(limits,cancel,|art,reader|{
            let profiles=decode_profiles(&record.metadata_profiles,reader)?;
            let working=record.working.decode(reader)?;
            let undo=decode_history(art,&working,record.artwork.checkpoint.edit_checkpoint,&record.undo,&record.tile_chunks,reader,cancel)?;let redo=decode_history(art,&working,record.artwork.checkpoint.edit_checkpoint,&record.redo,&record.tile_chunks,reader,cancel)?;
            Ok((working,undo,redo,profiles))
        })?;
        let mut artwork=(*capture.artwork).clone();
        let mut extensions=Extensions {records:record.extensions.clone(),..Default::default()};
        for id in &record.extension_resources {extensions.resources.insert(*id,self.artwork.private_resource(*id)?);}
        artwork.extensions=Arc::new(extensions);
        let mut document=Document::from_artwork(artwork).map_err(|e|e.to_string())?;
        document.working=working;document.revision=capture.checkpoint.artwork_generation;document.next_stroke_id=record.next_stroke_id;
        let before=document.working.clone();document.repair_working().map_err(|e|e.to_string())?;
        if before!=document.working {return Err("Invalid session transfer editing target".into());}
        document.validate(limits)?;
        let editor=Editor {document,undo,redo,checkpoint:capture.checkpoint.edit_checkpoint,next_checkpoint:record.next_checkpoint};
        let mut accounting=crate::history_budget::Accounting::new(editor.document());let mut bytes=0usize;
        for entry in editor.undo.iter().chain(&editor.redo) {bytes=bytes.saturating_add(accounting.charge(entry));}
        if bytes>crate::history_budget::BYTE_BUDGET {return Err("Session transfer history exceeds admission".into());}
        active(cancel)?;Ok(OpenSession {editor,metadata:SessionMetadata {value:record.metadata.clone(),profiles}})
    }
}
fn decode_history(current:&Artwork,current_working:&WorkingState,mut previous_checkpoint:u64,records:&[EntryRecord],chunks:&[Vec<Value>],reader:&mut ResourceReader<'_>,cancel:&AtomicBool)->Result<Vec<HistoryEntry>,String> {
    let limits=reader.limits;
    let mut context=current.clone();let mut working=current_working.clone();let mut result=Vec::with_capacity(records.len());
    for record in records {
        active(cancel)?;let previous=context.clone();let mut changes_artwork=false;let mut objects=BTreeMap::new();let mut seen=BTreeSet::new();let mut bytes=0usize;
        for change in &record.changes {if let Some(value)=&change.value {
            bytes=bytes.checked_add(value.expanded_size(chunks,limits)?).ok_or("Session history metadata overflow")?;
            if bytes as u64>limits.metadata_bytes {return Err("Expanded session history exceeds admission".into());}
        }}
        for change in &record.changes {
            if !seen.insert(change.id) {return Err("Duplicate session history change".into());}
            if let Some(value)=&change.value {
                if value.record["type"]!=change.kind||value.record["id"]!=serde_json::to_value(change.id).map_err(|e|e.to_string())? {return Err("Session history record identity mismatch".into());}
                objects.insert(change.id,value.expand(chunks,limits)?);
            }
        }
        artwork_records::decode_records_into(&mut context,&objects,reader).map_err(|e|e.to_string())?;
        let mut edits=Vec::with_capacity(record.changes.len()+1);
        macro_rules! change {($record:expr,$store:ident,$variant:ident)=>{{let record=$record;let handle=Handle::from_index(record.handle);
            if context.$store.id(handle)!=Some(record.id) {return Err("Session history handle identity mismatch".into());}
            let value=if record.value.is_some() {Some(context.$store.get(handle).ok_or("Missing session history record")?.clone())}else{context.$store.remove(handle);None};
            if previous.$store.get(handle)!=value.as_ref(){changes_artwork=true;}
            edits.push(Edit::$variant(RecordChange {handle,id:record.id,value}));
        }}}
        for record in &record.changes {match super::registry::descriptor(&record.kind).map(|record|record.kind) {
            Some(RecordKind::Composition)=>change!(record,compositions,Composition),Some(RecordKind::Stack)=>change!(record,stacks,Stack),
            Some(RecordKind::Occurrence)=>change!(record,occurrences,Occurrence),Some(RecordKind::PaintSource)=>change!(record,paint,Paint),
            Some(RecordKind::CoverageSource)=>change!(record,coverage,Coverage),Some(RecordKind::Effect)=>change!(record,effects,Effect),
            Some(RecordKind::ImageObject)=>change!(record,objects,ImageObject),Some(RecordKind::Selection)=>change!(record,selections,SavedSelection),
            Some(RecordKind::Guides)=>change!(record,guides,Guides),Some(RecordKind::Output)=>change!(record,outputs,Output),
            _=>return Err("Unknown session history record type".into()),
        }}
        if changes_artwork && previous_checkpoint==record.checkpoint {return Err("Artwork history changed without a new checkpoint".into());}
        previous_checkpoint=record.checkpoint;
        if let Some(value)=&record.working {working=value.decode(reader)?;edits.push(Edit::Working(working.clone()));}
        let mut candidate=Document::from_artwork(context.clone()).map_err(|error|error.to_string())?;
        candidate.working=working.clone();candidate.repair_working().map_err(|error|error.to_string())?;
        if candidate.working!=working {return Err("Invalid session history editing target".into());}
        candidate.validate(limits)?;
        result.push(HistoryEntry::new(Edit::Batch(edits),record.checkpoint));
    }
    result.reverse();Ok(result)
}
fn validate(record:&SessionTransferDescriptor,limits:ProjectLimits)->Result<(),String> {
    if crate::json_len(record)>limits.metadata_bytes.min(usize::MAX as u64) as usize {return Err("Session transfer metadata exceeds admission".into());}
    if record.extension_resources.iter().collect::<BTreeSet<_>>().len()!=record.extension_resources.len()
        ||record.extensions.iter().any(|(id,value)|value["id"].as_str().and_then(|value|value.parse::<PortableId>().ok())!=Some(*id)) {return Err("Invalid session attachment identity".into());}
    validate_history_checkpoints(record.artwork.checkpoint.edit_checkpoint,record.next_checkpoint,record.undo.iter().map(|entry|entry.checkpoint),record.redo.iter().map(|entry|entry.checkpoint))?;
    if record.undo.len()>crate::history_budget::ENTRY_BUDGET||record.redo.len()>crate::history_budget::ENTRY_BUDGET
        ||record.next_checkpoint==0||record.next_stroke_id==0||record.artwork.checkpoint.edit_checkpoint>=record.next_checkpoint
        ||record.working.generation!=record.artwork.checkpoint.working_generation
        ||record.undo.iter().chain(&record.redo).any(|entry|entry.checkpoint>=record.next_checkpoint) {return Err("Invalid session transfer checkpoint".into());}
    Ok(())
}
impl SessionTransferReceiver {
    pub fn new(descriptor:SessionTransferDescriptor,limits:ProjectLimits)->Result<Self,String> {
        validate(&descriptor,limits)?;let mut transport_limits=limits;transport_limits.raster_bytes=transport_limits.raster_bytes.saturating_add(crate::history_budget::BYTE_BUDGET as u64);
        let artwork=TransferReceiver::new(descriptor.artwork.clone(),transport_limits)?;Ok(Self {descriptor,artwork})
    }
    pub fn new_reusing(descriptor:SessionTransferDescriptor,limits:ProjectLimits,previous:&PreparedSessionTransfer)->Result<Self,String> {
        validate(&descriptor,limits)?;
        if descriptor.artwork.checkpoint.document!=previous.descriptor.artwork.checkpoint.document {return Err("Session reuse belongs to another drawing".into());}
        let mut transport_limits=limits;transport_limits.raster_bytes=transport_limits.raster_bytes.saturating_add(crate::history_budget::BYTE_BUDGET as u64);
        let artwork=TransferReceiver::new_reusing(descriptor.artwork.clone(),transport_limits,&previous.artwork)?;Ok(Self {descriptor,artwork})
    }
    pub fn missing_payloads(&self)->Vec<usize> {self.artwork.missing_payloads()}
    pub fn push_chunk(&mut self,index:usize,bytes:&[u8])->Result<(),String> {self.artwork.push_chunk(index,bytes)}
    pub fn finish(self)->Result<PreparedSessionTransfer,String> {Ok(PreparedSessionTransfer {descriptor:self.descriptor,artwork:self.artwork.finish()?})}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentNames,Selection,SelectionPixels};
    use serde_json::json;
    fn editor()->Editor {
        let document=Document::new(PortableId::random(),19,11,DocumentNames{paint:"ink".into(),paper:"paper".into()});
        Editor::new(document)
    }
    fn rename(editor:&mut Editor,name:&str) {
        let handle=editor.document.working.occurrence.unwrap();let mut value=editor.document.artwork.occurrences.get(handle).unwrap().clone();value.name=name.into();
        editor.perform(Edit::Occurrence(RecordChange::replace(&editor.document.artwork.occurrences,handle,Some(value)).unwrap())).unwrap();
    }
    fn transfer(editor:&Editor)->PreparedSessionTransfer {
        let capture=editor.capture_session(editor.capture(7,Default::default()).unwrap()).unwrap();
        PreparedSessionTransfer::capture(&capture,json!({"saved":1}),&AtomicBool::new(false)).unwrap()
    }
    fn receive(prepared:&PreparedSessionTransfer)->OpenSession {
        let cancel=AtomicBool::new(false);let mut receiver=SessionTransferReceiver::new(prepared.descriptor().clone(),ProjectLimits::default()).unwrap();
        for i in 0..prepared.payload_count() {let mut offset=0;let length=prepared.payload_len(i).unwrap();while offset<length {
            let size=(length-offset).min(super::super::MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(i,&prepared.read_chunk(i,offset,size).unwrap()).unwrap();offset+=size as u64;
        }}
        receiver.finish().unwrap().adopt_verified(ProjectLimits::default(),&cancel).unwrap()
    }
    #[test]
    fn undo_only_image_dependencies_and_occurrence_offsets_survive_incremental_transfer() {
        let mut original=editor();let target=original.document.working.target.unwrap();let SourceTarget::Paint(paint)=target else{unreachable!()};
        let image=Image::new(crate::color::source::rgba8_source([19,11],|_,_|[91,42,71,255]));
        original.document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(image.clone()));
        let occurrence=original.document.working.occurrence.unwrap();
        let mut value=original.document.artwork.occurrences.get(occurrence).unwrap().clone();value.offset=[-3,0];
        original.perform(Edit::Occurrence(RecordChange::replace(&original.document.artwork.occurrences,occurrence,Some(value)).unwrap())).unwrap();
        let mut value=original.document.artwork.paint.get(paint).unwrap().clone();value.base=None;
        original.perform(Edit::Paint(RecordChange::replace(&original.document.artwork.paint,paint,Some(value)).unwrap())).unwrap();
        let prepared=transfer(&original);assert!(prepared.descriptor.artwork.manifest["objects"].as_array().unwrap().iter().any(|r|r["type"]=="capy.image/1"));
        let mut restored=receive(&prepared).editor;assert!(restored.document.artwork.paint.get(paint).unwrap().base.is_none());
        restored.undo().unwrap();let restored_image=restored.document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image.clone();assert_eq!(restored_image.id(),image.id());
        assert_eq!(restored.document.artwork.occurrences.get(occurrence).unwrap().offset,[-3,0]);
        restored.undo().unwrap();assert_eq!(restored.document.artwork.occurrences.get(occurrence).unwrap().offset,[0,0]);
        assert!(restored_image.same_owner(&restored.document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image));
        restored.redo().unwrap();restored.redo().unwrap();assert!(restored.document.artwork.paint.get(paint).unwrap().base.is_none());
    }
    #[test]
    fn custom_program_values_resources_and_spatial_survive_history_transfer() {
        let mut original=editor();let occurrence=original.document.working.occurrence.unwrap();let composition=original.document.composition();
        let program=crate::effect_catalog::custom_program("exposure");let instance=crate::EffectInstance::new(program.clone());
        let application=RecordChange::insert(&original.document.artwork.effects,EffectApplication::new(program,instance.values,composition.size));
        let effect=application.handle;let mut row=original.document.artwork.occurrences.get(occurrence).unwrap().clone();row.content=OccurrenceContent::Effect(effect);row.alpha_locked=false;
        original.perform(Edit::Batch(vec![Edit::Effect(application),Edit::Occurrence(RecordChange::replace(&original.document.artwork.occurrences,occurrence,Some(row)).unwrap())])).unwrap();
        let mut changed=original.document.artwork.effects.get(effect).unwrap().clone();changed.values[0]=crate::EffectValue::Number(0.75);
        original.perform(Edit::Effect(RecordChange::replace(&original.document.artwork.effects,effect,Some(changed)).unwrap())).unwrap();
        let mut restored=receive(&transfer(&original)).editor;
        assert_eq!(restored.document.artwork.effects.get(effect).unwrap().values,original.document.artwork.effects.get(effect).unwrap().values);
        let retained=restored.document.artwork.effects.get(effect).unwrap().program.clone();restored.undo().unwrap();
        assert!(Arc::ptr_eq(&retained,&restored.document.artwork.effects.get(effect).unwrap().program));
        assert_eq!(restored.document.artwork.effects.get(effect).unwrap().spatial,original.document.artwork.effects.get(effect).unwrap().spatial);
        restored.undo().unwrap();assert!(restored.document.artwork.effects.get(effect).is_none());restored.redo().unwrap();restored.redo().unwrap();
    }
    #[test]
    fn incremental_history_rejects_invalid_historical_object_ownership() {
        let mut original=editor();let image=Image::new(crate::color::source::rgba8_source([2,2],|_,_|[71,29,13,255]));
        let (first,edit)=original.document.create_object_layer_edit("First",ImageObject::new(image.clone()),None,0).unwrap();original.perform(edit).unwrap();
        let (second,edit)=original.document.create_object_layer_edit("Second",ImageObject::new(image),None,1).unwrap();original.perform(edit).unwrap();
        original.perform(original.document.delete_layers_edit(&[first]).unwrap()).unwrap();
        let prepared=transfer(&original);let mut descriptor=prepared.descriptor().clone();
        let change=descriptor.undo[0].changes.iter_mut().find(|change|change.kind=="capy.occurrence/3").unwrap();
        let object=original.document.scene().object_handle(second).unwrap();let object_id=original.document.artwork.objects.id(object).unwrap();
        change.value.as_mut().unwrap().record["data"]["content"]=serde_json::json!({"objects":super::super::resources::reference(object_id)});
        let invalid=PreparedSessionTransfer {descriptor,artwork:prepared.artwork};
        assert!(invalid.adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).is_err());
    }
    #[test]
    fn direct_history_transfer_keeps_checkpoints_targets_selection_and_branching() {
        let mut original=editor();rename(&mut original,"first");
        let mut working=original.document.working.clone();working.selection=Some(Selection::pixels(Arc::new(SelectionPixels::new([8,2],[0,0,8,2],vec![0x12341234;2]).unwrap())));
        original.perform(Edit::Working(working)).unwrap();rename(&mut original,"second");rename(&mut original,"third");original.undo().unwrap();
        let prepared=transfer(&original);let mut restored=receive(&prepared).editor;
        assert_eq!(restored.document.artwork,original.document.artwork);assert_eq!(restored.document.working,original.document.working);
        for redo in [false,false,true,true,true,false,false,true] {
            let expected=if redo {original.redo()}else{original.undo()}.unwrap();let actual=if redo {restored.redo()}else{restored.undo()}.unwrap();
            assert_eq!(actual,expected);assert_eq!(restored.checkpoint(),original.checkpoint());assert_eq!(restored.document.artwork,original.document.artwork);assert_eq!(restored.document.working.selection,original.document.working.selection);
        }
        rename(&mut restored,"branch");assert!(!restored.can_redo());assert!(restored.next_checkpoint>original.next_checkpoint);
    }
    #[test]
    fn unchanged_original_is_transferred_once_and_shares_history_owners() {
        let mut original=editor();let paint=match original.document.working.target.unwrap(){SourceTarget::Paint(h)=>h,_=>unreachable!()};
        original.document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|x,y|[x as u8,y as u8,80,255]))));
        let baseline=transfer(&original).payload_count();
        for i in 0..100 {rename(&mut original,&format!("name {i}"));}
        let prepared=transfer(&original);assert_eq!(prepared.payload_count(),baseline);
        let mut restored=receive(&prepared).editor;let first=restored.document.artwork.paint.get(paint).unwrap().base.clone().unwrap();
        for _ in 0..100 {restored.undo().unwrap();assert!(first.image.same_owner(&restored.document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image));}
    }
    #[test]
    fn private_attachments_survive_deleted_subjects_and_undo() {
        let mut original=editor();let subject=original.document.working.occurrence.unwrap();let subject_id=original.document.artwork.occurrences.id(subject).unwrap();
        let resource_id=PortableId::random();let bytes:Arc<[u8]>=Arc::from(b"retained private attachment".as_slice());
        let resource=Arc::new(OpaqueResource {id:resource_id,kind:"future.attachment/1".into(),data:json!({"subject":super::super::resources::reference(subject_id)}),encoding:"future.codec/1".into(),
            extra_fields:Default::default(),backing:super::super::ImmutableBacking::new(Arc::new(bytes.clone())).unwrap(),offset:0,length:bytes.len() as u64,crc32:crc32fast::hash(&bytes)});
        let mut extensions=Extensions::default();extensions.resources.insert(resource_id,resource);
        for safe in [false,true] {let id=PortableId::random();extensions.records.insert(id,json!({"id":id,"type":"future.note/1","ancillary":true,"copy_safe":safe,
            "data":{"subject":super::super::resources::reference(subject_id),"payload":super::super::resources::reference(resource_id),"text":"retain every value"}}));}
        original.document.artwork.extensions=Arc::new(extensions);let expected=original.document.artwork.extensions.records.clone();
        original.perform(original.document.delete_layers_edit(&[subject]).unwrap()).unwrap();
        let prepared=transfer(&original);let mut restored=receive(&prepared).editor;
        assert!(restored.document.artwork.occurrences.get(subject).is_none());assert_eq!(restored.document.artwork.extensions.records,expected);
        assert_eq!(&*restored.document.artwork.extensions.resources[&resource_id].read_chunk(0,bytes.len(),&AtomicBool::new(false)).unwrap(),&*bytes);
        restored.undo().unwrap();assert!(restored.document.artwork.occurrences.get(subject).is_some());assert_eq!(restored.document.artwork.extensions.records,expected);
        let again=receive(&transfer(&restored));assert_eq!(again.editor.document.artwork.extensions.records,expected);
    }
    #[test]
    fn receiver_reuses_immutable_payloads_for_metadata_only_edits() {
        let mut original=editor();let paint=match original.document.working.target.unwrap(){SourceTarget::Paint(h)=>h,_=>unreachable!()};
        original.document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|x,y|[x as u8,y as u8,80,255]))));
        let first=transfer(&original);
        let mut receiver=SessionTransferReceiver::new(first.descriptor().clone(),ProjectLimits::default()).unwrap();
        for index in receiver.missing_payloads() {let length=first.payload_len(index).unwrap() as usize;receiver.push_chunk(index,&first.read_chunk(index,0,length).unwrap()).unwrap();}
        let previous=receiver.finish().unwrap();rename(&mut original,"renamed");let next=transfer(&original);
        let receiver=SessionTransferReceiver::new_reusing(next.descriptor().clone(),ProjectLimits::default(),&previous).unwrap();
        assert!(receiver.missing_payloads().is_empty());
        let restored=receiver.finish().unwrap().adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).unwrap();
        assert_eq!(restored.editor.document.artwork,original.document.artwork);
        let unrelated=transfer(&editor());assert!(SessionTransferReceiver::new_reusing(unrelated.descriptor().clone(),ProjectLimits::default(),&previous).is_err());
        let mut different=next.descriptor().clone();different.artwork.manifest["resources"][0]["crc32"]=Value::String("12345678".into());
        let receiver=SessionTransferReceiver::new_reusing(different,ProjectLimits::default(),&previous).unwrap();
        assert!(!receiver.missing_payloads().is_empty());
    }
    #[test]
    fn compressed_history_metadata_cannot_expand_past_admission() {
        let mut original=editor();rename(&mut original,"edited");let prepared=transfer(&original);let mut descriptor=prepared.descriptor().clone();
        let mut versions=BTreeMap::new();let mut changes=Vec::new();
        for handle in 0..8 {let id=PortableId::random();let record=json!({"id":id,"type":"capy.paint-source/2","data":{"domain":[1,1],"tiles":[{"coordinate":[0,0],"padding":"x".repeat(128*1024)}]}});
            let value=ObjectVersion::capture(record,&mut descriptor.tile_chunks,&mut versions).unwrap();
            changes.push(ChangeRecord {kind:"capy.paint-source/2".into(),handle,id,value:Some(value)});
        }
        descriptor.undo[0].changes=changes;let invalid=PreparedSessionTransfer {descriptor,artwork:prepared.artwork};
        let limits=ProjectLimits {metadata_bytes:256*1024,..Default::default()};
        assert!((crate::json_len(invalid.descriptor()) as u64)<limits.metadata_bytes);
        assert!(invalid.adopt_verified(limits,&AtomicBool::new(false)).err().unwrap().contains("Expanded session history exceeds admission"));
    }
    #[test]
    fn checkpoint_mutations_reject_backwards_and_unmarked_artwork_changes() {
        let mut original=editor();for name in ["one","two","three"] {rename(&mut original,name);}
        let prepared=transfer(&original);let mut descriptor=prepared.descriptor().clone();descriptor.undo[1].checkpoint=descriptor.artwork.checkpoint.edit_checkpoint;
        assert!(SessionTransferReceiver::new(descriptor,ProjectLimits::default()).is_err());
        let mut descriptor=prepared.descriptor().clone();descriptor.undo[0].checkpoint=descriptor.artwork.checkpoint.edit_checkpoint;
        let invalid=PreparedSessionTransfer {descriptor,artwork:prepared.artwork};
        assert!(invalid.adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).err().unwrap().contains("Artwork history changed without a new checkpoint"));
        let mut capture=original.capture_session(original.capture(1,Default::default()).unwrap()).unwrap();capture.artwork_mut().checkpoint.owner+=1;
        assert!(PreparedSessionTransfer::capture(&capture,Value::Null,&AtomicBool::new(false)).is_err());
    }
    #[test]
    fn receiver_rejects_incomplete_invalid_and_cancelled_transfers() {
        let mut original=editor();let mut working=original.document.working.clone();working.selection=Some(Selection::pixels(Arc::new(SelectionPixels::new([8,2],[0,0,8,2],vec![0x12341234;2]).unwrap())));
        original.perform(Edit::Working(working)).unwrap();let prepared=transfer(&original);
        assert!(SessionTransferReceiver::new(prepared.descriptor().clone(),ProjectLimits::default()).unwrap().finish().is_err());
        let mut descriptor=prepared.descriptor().clone();descriptor.next_checkpoint=0;assert!(SessionTransferReceiver::new(descriptor,ProjectLimits::default()).is_err());
        let mut descriptor=prepared.descriptor().clone();descriptor.undo[0].changes.push(ChangeRecord {kind:"future/1".into(),handle:0,id:PortableId::random(),value:None});
        let invalid=PreparedSessionTransfer {descriptor,artwork:prepared.artwork};assert!(invalid.adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).is_err());
        let captured=original.capture_session(original.capture(1,Default::default()).unwrap()).unwrap();
        assert!(PreparedSessionTransfer::capture(&captured,Value::Null,&AtomicBool::new(true)).is_err());
    }
}
