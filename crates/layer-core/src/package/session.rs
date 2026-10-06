use super::registry::{self,RecordKind};
use super::{archive::{self, Directory, InputMember, Member}, artwork_records, manifest::{Manifest, ManifestLimits, ManifestRead}, resources::{self, ResourceInventory, ResourceReader, PreparedResources}, selection_records, transfer::TransferLayout, transport::BackingReader, ImmutableBacking};
use crate::{authored::{Artwork, ArtworkCapture, CaptureCheckpoint, Handle, OccurrenceHandle, PortableId, SourceTarget, Support, WorkingState}, Document, Edit, Editor, HistoryEntry, ProjectLimits};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, io::{Cursor, Write}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Debug)]
pub struct EditorCapture {
    pub(crate) artwork: ArtworkCapture,
    pub(crate) document: Document,
    pub(crate) undo: Vec<(Edit,u64)>,
    pub(crate) redo: Vec<(Edit,u64)>,
    pub(crate) next_checkpoint: u64,
    pub(crate) captured_checkpoint: CaptureCheckpoint,
}
type HistoryDocuments=Vec<(Document,u64)>;
impl EditorCapture {
    pub fn artwork(&self)->&ArtworkCapture {&self.artwork}
    pub fn artwork_mut(&mut self)->&mut ArtworkCapture {&mut self.artwork}
    pub(crate) fn validate_identity(&self)->Result<(),String> {
        if self.artwork.checkpoint!=self.captured_checkpoint || self.artwork.checkpoint.document!=self.document.artwork.id || self.artwork.artwork.id!=self.document.artwork.id
            || self.artwork.checkpoint.owner!=self.document.owner || self.artwork.checkpoint.artwork_generation!=self.document.revision
            || self.artwork.checkpoint.working_generation!=self.document.working.generation {return Err("Session capture identity changed".into());}
        Ok(())
    }
    fn documents(&self,cancel:&AtomicBool)->Result<(Document,HistoryDocuments,HistoryDocuments),String> {
        active(cancel)?;self.validate_identity()?;
        let mut current=self.document.clone();
        current.artwork=(*self.artwork.artwork).clone();
        fn history(current:&Document,entries:&[(Edit,u64)],cancel:&AtomicBool)->Result<Vec<(Document,u64)>,String> {
            let mut document=current.clone();document.revision=0;document.working.generation=0;let mut states=Vec::with_capacity(entries.len());
            for (edit,checkpoint) in entries.iter().rev() {
                active(cancel)?;document.apply(edit.clone()).map_err(|e|e.to_string())?;
                states.push((document.clone(),*checkpoint));
            }
            Ok(states)
        }
        Ok((current.clone(),history(&current,&self.undo,cancel)?,history(&current,&self.redo,cancel)?))
    }
}
impl Editor {
    pub fn validate_checkpoint(&self,checkpoint:u64)->Result<(),crate::DocumentError> {
        if checkpoint>=self.next_checkpoint {return Err(crate::DocumentError::InvalidLayerOperation("Invalid saved checkpoint"));}
        Ok(())
    }
    pub fn capture_session(&self,capture:ArtworkCapture)->Result<EditorCapture,crate::DocumentError> {
        let Editor {document,undo,redo,checkpoint:edit_checkpoint,next_checkpoint}=self;
        let Document {artwork,working,revision,owner,scene_index:_,next_stroke_id:_}=document;
        let checkpoint=capture.checkpoint;
        if checkpoint.document!=artwork.id || checkpoint.owner!=*owner
            || checkpoint.artwork_generation!=*revision || checkpoint.working_generation!=working.generation
            || checkpoint.edit_checkpoint!=*edit_checkpoint {return Err(crate::DocumentError::InvalidLayerOperation("Session capture is stale"));}
        let history=|entries:&[HistoryEntry]|->Result<Vec<(Edit,u64)>,crate::DocumentError> {
            let mut history:Vec<_>=entries.iter().map(|e|(e.edit.clone(),e.checkpoint)).collect();
            if let Some((edit,_))=history.iter_mut().rev().find(|(edit,_)|edit.changes_project()) {
                let output=crate::RecordChange::replace(&artwork.outputs,artwork.default_output,Some(document.output().clone()))?;
                *edit=Edit::Batch(vec![Edit::Output(output),edit.clone()]);
            }
            Ok(history)
        };
        let undo=history(undo)?;let redo=history(redo)?;
        Ok(EditorCapture {artwork:capture,document:document.clone(),undo,redo,next_checkpoint:*next_checkpoint,captured_checkpoint:checkpoint})
    }
}
fn active(cancel:&AtomicBool)->Result<(),String> {if cancel.load(Ordering::Relaxed) {Err("Session operation cancelled".into())}else{Ok(())}}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkingRecord {
    pub(crate) generation:u64, selection:Option<Value>, selection_overlays:crate::authored::SelectionOverlays,
    layer_selection:BTreeSet<OccurrenceHandle>,layer_anchor:Option<OccurrenceHandle>,solo_visibility:Option<BTreeMap<OccurrenceHandle,bool>>,
    occurrence:Option<OccurrenceHandle>,target:Option<SourceTarget>,inspect_mask:Option<OccurrenceHandle>,
}
impl WorkingRecord {
    pub(crate) fn capture(working:&WorkingState,resources:&mut ResourceInventory)->Result<Self,String> {
        let WorkingState {generation,selection,selection_overlays,layer_selection,layer_anchor,solo_visibility,occurrence,target,inspect_mask}=working;
        Ok(Self {generation:*generation,selection:selection.as_ref().map(|s|selection_records::encode_selection(s,resources)).transpose()?,
            selection_overlays:selection_overlays.clone(),layer_selection:layer_selection.clone(),layer_anchor:*layer_anchor,solo_visibility:solo_visibility.clone(),occurrence:*occurrence,target:*target,inspect_mask:*inspect_mask})
    }
    pub(crate) fn decode(&self,reader:&mut ResourceReader<'_>)->Result<WorkingState,String> {
        Ok(WorkingState {generation:self.generation,selection:self.selection.as_ref().map(|s|selection_records::decode_selection(s,reader)).transpose().map_err(|e|e.to_string())?,
            selection_overlays:self.selection_overlays.clone(),layer_selection:self.layer_selection.clone(),layer_anchor:self.layer_anchor,solo_visibility:self.solo_visibility.clone(),occurrence:self.occurrence,target:self.target,inspect_mask:self.inspect_mask})
    }
}
const RASTER_INDEX_CHUNK:usize=64;
#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObjectVersion {pub(crate) record:Value,tiles:Option<Vec<usize>>,}
impl ObjectVersion {
    pub(crate) fn capture(mut record:Value,chunks:&mut Vec<Vec<Value>>,versions:&mut BTreeMap<Vec<u8>,usize>)->Result<Self,String> {
        let tiles=if matches!(record["type"].as_str().and_then(registry::descriptor).map(|record|record.kind),Some(RecordKind::PaintSource|RecordKind::CoverageSource|RecordKind::Image)) {
            record["data"].as_object_mut().ok_or("Invalid session object data")?.remove("tiles").map(|value| {
                let tiles=value.as_array().ok_or("Invalid captured raster index")?;
                tiles.chunks(RASTER_INDEX_CHUNK).map(|chunk| {
                    let key=serde_json::to_vec(chunk).map_err(|e|e.to_string())?;
                    Ok(if let Some(index)=versions.get(&key) {*index}else {let index=chunks.len();versions.insert(key,index);chunks.push(chunk.to_vec());index})
                }).collect::<Result<Vec<_>,String>>()
            }).transpose()?
        }else{None};
        Ok(Self {record,tiles})
    }
    pub(crate) fn expanded_size(&self,chunks:&[Vec<Value>],limits:ProjectLimits)->Result<usize,String> {
        if !self.record.is_object() || !self.record["data"].is_object() {return Err("Invalid session object data".into());}
        let mut size=crate::json_len(&self.record).saturating_add(32);
        if let Some(indices)=&self.tiles {
            if !matches!(self.record["type"].as_str().and_then(registry::descriptor).map(|record|record.kind),Some(RecordKind::PaintSource|RecordKind::CoverageSource|RecordKind::Image)) || self.record["data"].get("tiles").is_some() || indices.is_empty()
                || indices.len()>limits.tiles.div_ceil(RASTER_INDEX_CHUNK) {return Err("Invalid session raster index".into());}
            let mut tiles=0usize;
            for (index,chunk) in indices.iter().enumerate() {
                let chunk=chunks.get(*chunk).ok_or("Unknown session raster index chunk")?;
                if chunk.is_empty() || chunk.len()>RASTER_INDEX_CHUNK || (index+1!=indices.len() && chunk.len()!=RASTER_INDEX_CHUNK) {return Err("Invalid session raster index chunk".into());}
                tiles=tiles.saturating_add(chunk.len());size=size.saturating_add(crate::json_len(chunk));
            }
            if tiles>limits.tiles {return Err("Session raster index exceeds admission".into());}
        } else if matches!(self.record["type"].as_str().and_then(registry::descriptor).map(|record|record.kind),Some(RecordKind::PaintSource|RecordKind::CoverageSource|RecordKind::Image)) && self.record["data"].get("tiles").is_some() {return Err("Unexpected session raster index".into());}
        if size as u64>limits.metadata_bytes {return Err("Expanded session object exceeds admission".into());}
        Ok(size)
    }
    pub(crate) fn expand(&self,chunks:&[Vec<Value>],limits:ProjectLimits)->Result<Value,String> {
        self.expanded_size(chunks,limits)?;let mut record=self.record.clone();
        if let Some(indices)=&self.tiles {record["data"]["tiles"]=Value::Array(indices.iter().flat_map(|index|chunks[*index].iter().cloned()).collect());}
        Ok(record)
    }
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct StateRecord {objects:Vec<usize>,outputs:Vec<Value>,working:WorkingRecord,checkpoint:u64}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRecord {
    format:String,version:u32,checkpoint:CaptureCheckpoint,next_checkpoint:u64,next_stroke_id:u64,
    layout:TransferLayout,root:Value,default_output:Value,artwork_metadata:Option<Value>,
    metadata:Value,metadata_profiles:Vec<Value>,objects:Vec<ObjectVersion>,tile_chunks:Vec<Vec<Value>>,resources:Vec<Value>,extension_records:BTreeMap<PortableId,Value>,extension_resources:Vec<PortableId>,current:StateRecord,undo:Vec<StateRecord>,redo:Vec<StateRecord>,
}
impl SessionRecord {
    fn manifest(&self,state:&StateRecord,directory:&Directory,limits:ProjectLimits)->Result<Manifest,String> {
        if state.objects.len()>ManifestLimits::default().graph.objects {return Err("Session object graph exceeds admission".into());}
        let mut expanded=0usize;
        for index in &state.objects {
            expanded=expanded.saturating_add(self.objects.get(*index).ok_or("Unknown session object version")?.expanded_size(&self.tile_chunks,limits)?);
            if expanded as u64>limits.metadata_bytes {return Err("Expanded session state exceeds admission".into());}
        }
        let objects=state.objects.iter().map(|index|self.objects.get(*index).ok_or_else(||"Unknown session object version".to_string()).and_then(|object|object.expand(&self.tile_chunks,limits))).collect::<Result<Vec<_>,_>>()?;
        let resources=self.resources.iter().map(|value|Ok((value["id"].as_str().ok_or("Missing session resource identity")?.parse::<PortableId>().map_err(str::to_string)?,value))).collect::<Result<BTreeMap<_,_>,String>>()?;
        let mut needed=std::collections::BTreeSet::new();
        let mut pending=super::references(&Value::Array(objects.clone()),ManifestLimits::default().traversal_nodes)?;
        if let Some(selection)=&state.working.selection {pending.extend(super::references(selection,ManifestLimits::default().traversal_nodes)?);}
        if let Some(metadata)=&self.artwork_metadata {pending.extend(super::references(metadata,ManifestLimits::default().traversal_nodes)?);}
        for profile in &self.metadata_profiles {pending.extend(super::references(profile,ManifestLimits::default().traversal_nodes)?);}
        while let Some(id)=pending.pop() {
            if let Some(resource)=resources.get(&id) && needed.insert(id) {pending.extend(super::references(resource,ManifestLimits::default().traversal_nodes)?);}
        }
        let resource_records=needed.iter().map(|id|resources[id].clone()).collect::<Vec<_>>();
        let mut value=json!({"format":"capy.canvas","version":1,"document":self.checkpoint.document,"root":self.root,
            "objects":objects,"resources":resource_records,"outputs":state.outputs,"default_output":self.default_output});
        if let Some(metadata)=&self.artwork_metadata {value["metadata"]=metadata.clone();}
        let bytes=serde_json::to_vec(&value).map_err(|e|e.to_string())?;
        match Manifest::parse_private(&bytes,directory,ManifestLimits {metadata_bytes:limits.metadata_bytes.min(usize::MAX as u64) as usize,..Default::default()})? {
            ManifestRead::Known(manifest) if matches!(manifest.support,Support::Editable)=>Ok(manifest),
            _=>Err("Unsupported session artwork".into()),
        }
    }
}

#[derive(Default)]
struct StateEncoder {inventory:ResourceInventory,records:Vec<ObjectVersion>,versions:BTreeMap<Vec<u8>,usize>,tile_chunks:Vec<Vec<Value>>,tile_versions:BTreeMap<Vec<u8>,usize>}
impl StateEncoder {
    fn capture(&mut self,document:&Document,checkpoint:u64,cancel:&AtomicBool)->Result<StateRecord,String> {
            active(cancel)?;let art=&document.artwork;
            if art.paint.iter().any(|(_,_,s)|!s.operations.is_empty())||art.coverage.iter().any(|(_,_,s)|!s.operations.is_empty()){return Err("Wait for the current edit before checkpointing".into());}
            let (objects,next)=artwork_records::encode_with_inventory(art,cancel,std::mem::take(&mut self.inventory))?;self.inventory=next;
            let mut indices=Vec::with_capacity(objects.len());
            for record in objects {
                let object=ObjectVersion::capture(record,&mut self.tile_chunks,&mut self.tile_versions)?;
                let key=serde_json::to_vec(&object).map_err(|e|e.to_string())?;
                let index=if let Some(index)=self.versions.get(&key) {*index}else {let index=self.records.len();self.versions.insert(key,index);self.records.push(object);index};
                indices.push(index);
            }
            Ok(StateRecord {objects:indices,outputs:art.outputs.iter().map(|(_,id,_)|resources::reference(id)).collect(),working:WorkingRecord::capture(&document.working,&mut self.inventory)?,checkpoint})
    }
}
#[derive(Clone,Debug)]
pub struct SessionMetadata {pub value:Value,pub profiles:Vec<crate::color::ColorProfile>}
impl From<Value> for SessionMetadata {fn from(value:Value)->Self {Self {value,profiles:Vec::new()}}}
impl SessionMetadata {
    pub(crate) fn encode_profiles(&self,inventory:&mut ResourceInventory)->Result<Vec<Value>,String> {
        if self.profiles.len()>ManifestLimits::default().resources {return Err("Too many session profiles".into());}
        self.profiles.iter().map(|profile| {
            if matches!(profile,crate::color::ColorProfile::Icc(bytes) if bytes.is_empty() || bytes.len()>crate::color::source::MAX_PROFILE_BYTES) {return Err("Invalid session ICC profile size".into());}
            inventory.profile(profile)
        }).collect()
    }
}
pub(crate) fn decode_profiles(profiles:&[Value],reader:&mut ResourceReader<'_>)->Result<Vec<crate::color::ColorProfile>,String> {
    if profiles.len()>ManifestLimits::default().resources {return Err("Too many session profiles".into());}
    profiles.iter().map(|profile|reader.profile(profile).map_err(|error|error.to_string())).collect()
}
pub struct PreparedSession {metadata:Arc<[u8]>,resources:PreparedResources}
impl PreparedSession {
    pub fn prepare(capture:&EditorCapture,metadata:impl Into<SessionMetadata>,cancel:&AtomicBool)->Result<Self,String> {
        let (current,undo,redo)=capture.documents(cancel)?;let mut encoder=StateEncoder::default();encoder.inventory.private=true;
        let opaque=current.artwork.extensions.resources.clone();
        let current_state=encoder.capture(&current,capture.artwork.checkpoint.edit_checkpoint,cancel)?;
        let mut undo_states=Vec::with_capacity(undo.len());let mut redo_states=Vec::with_capacity(redo.len());
        for (history,target) in [(&undo,&mut undo_states),(&redo,&mut redo_states)] {
            for (document,checkpoint) in history {target.push(encoder.capture(document,*checkpoint,cancel)?);}
        }
        let StateEncoder {mut inventory,records,versions:_,tile_chunks,tile_versions:_}=encoder;
        let metadata=metadata.into();let metadata_profiles=metadata.encode_profiles(&mut inventory)?;
        current.artwork.metadata.validate()?;
        let mut artwork_metadata=serde_json::Map::new();
        for (kind,bytes) in ["exif","xmp","iptc"].into_iter().zip(current.artwork.metadata.blocks()) {
            if let Some(bytes)=bytes {artwork_metadata.insert(kind.into(),inventory.bytes("capy.photo-metadata/1",bytes,json!({"kind":kind}))?);}
        }
        let mut resources=inventory.prepare(cancel)?;resources.append_opaque(opaque.into_values().collect(),cancel)?;
        let art=&current.artwork;
        let record=SessionRecord {format:"capy.session".into(),version:1,checkpoint:capture.artwork.checkpoint,next_checkpoint:capture.next_checkpoint,next_stroke_id:current.next_stroke_id,
            layout:TransferLayout::capture(art),root:resources::reference(art.compositions.id(art.root).ok_or("Missing session root")?),
            default_output:resources::reference(art.outputs.id(art.default_output).ok_or("Missing session output")?),
            artwork_metadata:(!artwork_metadata.is_empty()).then_some(Value::Object(artwork_metadata)),metadata:metadata.value,metadata_profiles,objects:records,tile_chunks,resources:resources.entries.iter().map(|r|r.record.clone()).collect(),
            extension_records:art.extensions.records.clone(),extension_resources:art.extensions.resources.keys().copied().collect(),current:current_state,undo:undo_states,redo:redo_states};
        let metadata=serde_json::to_vec(&record).map_err(|e|e.to_string())?;
        if metadata.len()>ProjectLimits::default().metadata_bytes as usize {return Err("Session metadata exceeds admission".into());}
        Ok(Self {metadata:metadata.into(),resources})
    }
    pub fn metadata(&self)->&[u8] {&self.metadata}
    pub fn resources(&self)->&PreparedResources {&self.resources}
    pub fn write(&self,output:&mut impl Write,cancel:&AtomicBool)->Result<(),String> {
        active(cancel)?;let mut mime=Cursor::new(archive::MIMETYPE);let mut metadata=crate::Cancellable {inner:Cursor::new(&*self.metadata),cancelled:||cancel.load(Ordering::Relaxed)};let mut pack=self.resources.reader(cancel);
        let mut members=vec![InputMember{name:"mimetype",length:archive::MIMETYPE.len() as u64,crc32:crc32fast::hash(archive::MIMETYPE),input:&mut mime},
            InputMember{name:"manifest.json",length:self.metadata.len() as u64,crc32:crc32fast::hash(&self.metadata),input:&mut metadata},
            InputMember{name:"data/tiles-1.bin",length:self.resources.length,crc32:self.resources.crc,input:&mut pack}];
        let mut writer=crate::Cancellable {inner:output,cancelled:||cancel.load(Ordering::Relaxed)};
        archive::write_archive(&mut writer,&mut members,ProjectLimits::default().metadata_bytes as usize)
    }
}

pub struct OpenSession {pub editor:Editor,pub metadata:SessionMetadata}
pub(crate) fn validate_history_checkpoints(current:u64,next:u64,undo:impl IntoIterator<Item=u64>,redo:impl IntoIterator<Item=u64>)->Result<(),String> {
    if current>=next {return Err("Invalid session history checkpoint".into());}
    let mut previous=current;
    for checkpoint in undo {if checkpoint>previous {return Err("Invalid Undo checkpoint order".into());}previous=checkpoint;}
    previous=current;
    for checkpoint in redo {if checkpoint<previous || checkpoint>=next {return Err("Invalid Redo checkpoint order".into());}previous=checkpoint;}
    Ok(())
}


fn difference(before:&Document,after:&Document)->Result<Edit,String> {
    for artwork in [&before.artwork,&after.artwork] {
        let Artwork {id:_,root:_,compositions:_,stacks:_,occurrences:_,paint:_,coverage:_,effects:_,object_layers:_,objects:_,selections:_,guides:_,outputs:_,default_output:_,metadata:_,extensions:_}=artwork;
    }
    if before.artwork.id!=after.artwork.id || before.artwork.root!=after.artwork.root || before.artwork.default_output!=after.artwork.default_output
        || before.artwork.metadata!=after.artwork.metadata || before.artwork.extensions!=after.artwork.extensions {return Err("Inconsistent session history roots".into());}
    let mut edits=Vec::new();
    macro_rules! store {
        ($store:ident,$variant:ident)=>{{
            let a=&before.artwork.$store;let b=&after.artwork.$store;
            if a.capacity()!=b.capacity() {return Err("Inconsistent session history layout".into());}
            for index in 0..a.capacity() {
                let handle=Handle::from_index(index as u32);let id=a.id(handle).ok_or("Missing session handle")?;
                if b.id(handle)!=Some(id) {return Err("Conflicting session history identity".into());}
                if a.get(handle)!=b.get(handle) {edits.push(Edit::$variant(crate::RecordChange{handle,id,value:b.get(handle).cloned()}));}
            }
        }};
    }
    store!(compositions,Composition);store!(stacks,Stack);store!(occurrences,Occurrence);store!(paint,Paint);store!(coverage,Coverage);
    store!(effects,Effect);store!(object_layers,ObjectLayer);store!(objects,ImageObject);store!(selections,SavedSelection);store!(guides,Guides);store!(outputs,Output);
    let mut working=after.working.clone();working.generation=before.working.generation;
    if before.working!=working {edits.push(Edit::Working(after.working.clone()));}
    Ok(Edit::Batch(edits))
}
fn intern_artwork(art:&mut Artwork,state:&StateRecord,objects:&[ObjectVersion],documents:&[Document],owners:&mut [Option<usize>])->Result<(),String> {
    for index in &state.objects {
        let object=&objects.get(*index).ok_or("Unknown session object version")?.record;
        let id:PortableId=object["id"].as_str().ok_or("Missing session object identity")?.parse().map_err(str::to_string)?;
        if let Some(owner)=owners[*index] {
            let previous=&documents[owner].artwork;
            macro_rules! reuse {
                ($store:ident)=>{{let h=art.$store.resolve(id).ok_or("Missing interned session record")?;
                    let value=previous.$store.get(previous.$store.resolve(id).ok_or("Missing previous session record")?).ok_or("Missing previous session record")?.clone();
                    *art.$store.get_mut(h).ok_or("Missing session record")?=value;}};
            }
            match object["type"].as_str().and_then(registry::descriptor).map(|record|record.kind) {
                Some(RecordKind::Composition)=>reuse!(compositions),Some(RecordKind::Stack)=>reuse!(stacks),Some(RecordKind::OccurrenceLegacy|RecordKind::Occurrence)=>reuse!(occurrences),
                Some(RecordKind::PaintSource)=>reuse!(paint),Some(RecordKind::CoverageSource)=>reuse!(coverage),Some(RecordKind::Effect)=>reuse!(effects),
                Some(RecordKind::ObjectLayer)=>reuse!(object_layers),Some(RecordKind::ImageObject)=>reuse!(objects),Some(RecordKind::Image)=>{},
                Some(RecordKind::Selection)=>reuse!(selections),Some(RecordKind::Guides)=>reuse!(guides),Some(RecordKind::Output)=>reuse!(outputs),_=>return Err("Unsupported interned session record".into()),
            }
        }else {owners[*index]=Some(documents.len());}
    }
    Ok(())
}

pub fn open_parts(metadata:&[u8],resource_pack:ImmutableBacking,limits:ProjectLimits,cancel:&AtomicBool)->Result<OpenSession,String> {
    active(cancel)?;
    let value=super::parse_json(metadata,limits.metadata_bytes.min(usize::MAX as u64) as usize)?;
    let record:SessionRecord=serde_json::from_value(value).map_err(|e|e.to_string())?;
    if record.format!="capy.session" || record.version!=1 {return Err("Unsupported session format".into());}
    if record.undo.len()>crate::history_budget::ENTRY_BUDGET || record.redo.len()>crate::history_budget::ENTRY_BUDGET
        || record.next_checkpoint==0 || record.next_stroke_id==0 || record.current.checkpoint!=record.checkpoint.edit_checkpoint
        || std::iter::once(&record.current).chain(&record.undo).chain(&record.redo).any(|s|s.checkpoint>=record.next_checkpoint)
        || record.current.working.generation!=record.checkpoint.working_generation {return Err("Invalid session history checkpoint".into());}
    validate_history_checkpoints(record.current.checkpoint,record.next_checkpoint,record.undo.iter().map(|state|state.checkpoint),record.redo.iter().map(|state|state.checkpoint))?;
    let states=std::iter::once(&record.current).chain(&record.undo).chain(&record.redo);
    let used=states.clone().flat_map(|state|state.objects.iter().copied()).collect::<std::collections::BTreeSet<_>>();
    if used.len()!=record.objects.len() || used.iter().any(|index|*index>=record.objects.len()) {return Err("Unindexed session object versions".into());}
    let directory=Directory {members:vec![Member{name:"data/tiles-1.bin".into(),offset:0,length:resource_pack.byte_len(),compressed_length:None,crc32:0}],length:resource_pack.byte_len()};
    let members=directory.members.iter().enumerate().map(|(i,m)|(m.name.as_str(),(i,m))).collect();
    let mut resource_records=BTreeMap::new();
    for value in &record.resources {
        let id=value["id"].as_str().ok_or("Missing session resource identity")?.parse::<PortableId>().map_err(str::to_string)?;
        if resource_records.insert(id,super::manifest::resource(value.clone(),id,&directory,&members)?).is_some() {return Err("Duplicate session resource identity".into());}
    }
    let mut needed=record.extension_resources.iter().copied().collect::<std::collections::BTreeSet<_>>();
    let mut references=Vec::new();
    let mut used_chunks=std::collections::BTreeSet::new();
    for object in &record.objects {
        references.extend(super::references(&object.record,ManifestLimits::default().traversal_nodes)?);
        if let Some(chunks)=&object.tiles {used_chunks.extend(chunks.iter().copied());}
    }
    if used_chunks.len()!=record.tile_chunks.len() || used_chunks.iter().any(|index|*index>=record.tile_chunks.len()) {return Err("Unindexed session raster chunks".into());}
    for chunk in &record.tile_chunks {references.extend(super::references(&Value::Array(chunk.clone()),ManifestLimits::default().traversal_nodes)?);}
    for state in std::iter::once(&record.current).chain(&record.undo).chain(&record.redo) {
        if let Some(selection)=&state.working.selection {references.extend(super::references(selection,ManifestLimits::default().traversal_nodes)?);}
    }
    if let Some(metadata)=&record.artwork_metadata {references.extend(super::references(metadata,ManifestLimits::default().traversal_nodes)?);}
    for profile in &record.metadata_profiles {references.extend(super::references(profile,ManifestLimits::default().traversal_nodes)?);}
    let mut pending=needed.iter().copied().collect::<Vec<_>>();
    for id in references {if resource_records.contains_key(&id) && needed.insert(id) {pending.push(id);}}
    while let Some(id)=pending.pop() {
        let resource=resource_records.get(&id).ok_or("Missing preserved session resource")?;
        for target in super::references(&resource.value,ManifestLimits::default().traversal_nodes)? {
            if resource_records.contains_key(&target) && needed.insert(target) {pending.push(target);}
        }
    }
    if needed.len()!=resource_records.len() {return Err("Unindexed session resources".into());}
    super::manifest::overlaps(&resource_records)?;
    if resource_records.len()>ManifestLimits::default().resources {return Err("Too many session resources".into());}
    let layout=serde_json::to_value(&record.layout).map_err(|e|e.to_string())?;
    let mut identities=std::collections::BTreeSet::from([record.checkpoint.document]);
    let slots=layout.as_object().ok_or("Invalid session handle layout")?.values().map(|value|value.as_array().ok_or("Invalid session handle slots")).collect::<Result<Vec<_>,_>>()?;
    if slots.iter().map(|slots|slots.len()).sum::<usize>()>ManifestLimits::default().graph.objects {return Err("Session handle layout exceeds admission".into());}
    for value in slots.into_iter().flatten() {
        let id=value.as_str().ok_or("Missing session handle identity")?.parse::<PortableId>().map_err(str::to_string)?;
        if !identities.insert(id) {return Err("Conflicting session handle identity".into());}
    }
    for id in resource_records.keys().chain(record.extension_records.keys()) {
        if !identities.insert(*id) {return Err("Conflicting session resource identity".into());}
    }
    let mut ranges=resource_records.values().filter_map(|resource|resource.range).map(|range|(range.offset,range.length)).collect::<Vec<_>>();ranges.sort();ranges.dedup();
    let mut end=0u64;
    for (offset,length) in ranges {
        if offset!=end && length!=0 {return Err("Unindexed session resource bytes".into());}
        if length!=0 {end=end.checked_add(length).ok_or("Session resource length overflow")?;}
    }
    if end!=resource_pack.byte_len() {return Err("Unindexed session resource bytes".into());}
    let mut extensions=crate::authored::Extensions {records:record.extension_records.clone(),resources:BTreeMap::new()};
    for (id,value) in &extensions.records {
        if value["id"]!=json!(id) || value["ancillary"]!=true || value["type"].as_str().is_none_or(str::is_empty) {return Err("Invalid preserved session record".into());}
    }
    for id in &record.extension_resources {
        active(cancel)?;
        let resource=resource_records.get(id).ok_or("Missing preserved session resource")?;
        let range=resource.range.ok_or("Missing preserved session resource range")?;
        let fields=resource.value.as_object().ok_or("Invalid preserved session resource")?;
        let opaque=Arc::new(crate::authored::OpaqueResource {id:*id,kind:fields["type"].as_str().ok_or("Missing preserved resource type")?.into(),
            data:fields["data"].clone(),encoding:fields["encoding"].as_str().ok_or("Missing preserved resource encoding")?.into(),
            extra_fields:fields.iter().filter(|(key,_)|!["id","type","data","encoding","location","bytes","crc32"].contains(&key.as_str())).map(|(key,value)|(key.clone(),value.clone())).collect(),
            backing:resource_pack.clone(),offset:range.offset,length:range.length,crc32:resource.crc32});
        opaque.verify(cancel)?;
        if extensions.resources.insert(*id,opaque).is_some() {return Err("Duplicate preserved session resource".into());}
    }
    let extensions=Arc::new(extensions);
    let mut cache=resources::ResourceCache::default();let mut documents=Vec::new();let mut owners=vec![None;record.objects.len()];let mut profiles=Vec::new();
    let mut resource_limits=limits;resource_limits.raster_bytes=resource_limits.raster_bytes.saturating_add(limits.asset_bytes).saturating_add(crate::history_budget::BYTE_BUDGET as u64);
    for state in std::iter::once(&record.current).chain(&record.undo).chain(&record.redo) {
        active(cancel)?;let manifest=record.manifest(state,&directory,limits)?;
        let mut reader=ResourceReader::new(&manifest,&resource_pack,cancel,resource_limits);reader.private=true;reader.install_cache(cache);
        if documents.is_empty() {profiles=decode_profiles(&record.metadata_profiles,&mut reader)?;}
        let mut artwork=artwork_records::decode_with_layout(&manifest,&mut reader,Some(&record.layout)).map_err(|e|e.to_string())?;
        artwork.extensions=extensions.clone();
        intern_artwork(&mut artwork,state,&record.objects,&documents,&mut owners)?;
        let mut document=Document::from_artwork(artwork).map_err(|e|e.to_string())?;
        document.working=state.working.decode(&mut reader)?;
        let working=document.working.clone();document.repair_working().map_err(|e|e.to_string())?;
        if working!=document.working {return Err("Invalid session editing target".into());}
        document.validate(limits)?;
        cache=reader.into_cache();documents.push(document);
    }
    let mut undo=Vec::with_capacity(record.undo.len());let mut redo=Vec::with_capacity(record.redo.len());
    fn entries(states:&[StateRecord],documents:&[Document],offset:usize,current_checkpoint:u64)->Result<Vec<HistoryEntry>,String> {
        let mut entries=Vec::with_capacity(states.len());let mut previous=&documents[0];let mut previous_checkpoint=current_checkpoint;
        for (index,state) in states.iter().enumerate() {
            let document=&documents[offset+index];let edit=difference(previous,document)?;
            if previous.artwork!=document.artwork && previous_checkpoint==state.checkpoint {return Err("Authored history changed without a checkpoint".into());}
            let mut candidate=previous.clone();candidate.apply(edit.clone()).map_err(|e|e.to_string())?;
            candidate.working.generation=document.working.generation;
            if candidate.artwork!=document.artwork || candidate.working!=document.working {return Err("Invalid session history transition".into());}
            entries.push(HistoryEntry::new(edit,state.checkpoint));previous=document;previous_checkpoint=state.checkpoint;
        }
        entries.reverse();Ok(entries)
    }
    undo.extend(entries(&record.undo,&documents,1,record.current.checkpoint)?);redo.extend(entries(&record.redo,&documents,1+record.undo.len(),record.current.checkpoint)?);
    let mut document=documents.remove(0);document.revision=record.checkpoint.artwork_generation;document.next_stroke_id=record.next_stroke_id;
    let editor=Editor {document,undo,redo,checkpoint:record.checkpoint.edit_checkpoint,next_checkpoint:record.next_checkpoint};
    let mut accounting=crate::history_budget::Accounting::new(editor.document());let mut retained=0usize;
    for entry in editor.undo.iter().rev().chain(editor.redo.iter().rev()) {
        retained=retained.saturating_add(accounting.charge(entry));
        if retained>crate::history_budget::BYTE_BUDGET {return Err("Session history exceeds the Undo/Redo memory limit".into());}
    }
    Ok(OpenSession {editor,metadata:SessionMetadata {value:record.metadata,profiles}})
}

pub fn open(source:ImmutableBacking,limits:ProjectLimits,cancel:&AtomicBool)->Result<OpenSession,String> {
    active(cancel)?;let mut reader=BackingReader::new(&source,cancel);let directory=Directory::read(&mut reader,262_144,limits.metadata_bytes)?;
    if directory.members.iter().any(|m|!matches!(m.name.as_str(),"mimetype"|"manifest.json"|"data/tiles-1.bin")) {return Err("Invalid session archive members".into());}
    let metadata=directory.read_member(&mut reader,directory.member("manifest.json").ok_or("Missing session metadata")?,limits.metadata_bytes.min(usize::MAX as u64) as usize)?;
    let pack=directory.member("data/tiles-1.bin").ok_or("Missing session resources")?;
    struct Pack {source:ImmutableBacking,offset:u64,length:u64}
    impl super::ByteSource for Pack {
        fn byte_len(&self)->u64 {self.length}
        fn resident_bytes(&self)->usize {self.source.resident_bytes()}
        fn poll(&self,offset:u64,length:usize)->Result<super::RangeState,String> {
            if offset.checked_add(length as u64).is_none_or(|end|end>self.length){return Err("Session resource range overflow".into());}
            self.source.poll(self.offset.checked_add(offset).ok_or("Session resource offset overflow")?,length)
        }
    }
    let backing=ImmutableBacking::new(Arc::new(Pack {source:source.clone(),offset:pack.offset,length:pack.length})).map_err(str::to_string)?;
    open_parts(&metadata,backing,limits,cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PaintBase,Image};
    use crate::{DocumentNames, RecordChange, Selection, SelectionPixels};
    fn editor()->Editor {Editor::new(Document::new(PortableId::random(),19,11,DocumentNames{paint:"ink".into(),paper:"paper".into()}))}
    fn rename(editor:&mut Editor,name:&str) {
        let h=editor.document.working.occurrence.unwrap();let mut occurrence=editor.document.artwork.occurrences.get(h).unwrap().clone();occurrence.name=name.into();
        editor.perform(Edit::Occurrence(RecordChange::replace(&editor.document.artwork.occurrences,h,Some(occurrence)).unwrap())).unwrap();
    }
    fn prepared(editor:&Editor)->PreparedSession {
        let capture=editor.capture(7,Default::default()).unwrap();
        PreparedSession::prepare(&editor.capture_session(capture).unwrap(),json!({"name":"drawing","saved_checkpoint":1}),&AtomicBool::new(false)).unwrap()
    }
    fn reopen(prepared:&PreparedSession)->OpenSession {
        let mut bytes=Vec::new();prepared.write(&mut bytes,&AtomicBool::new(false)).unwrap();
        open(ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap(),ProjectLimits::default(),&AtomicBool::new(false)).unwrap()
    }
    #[test]
    fn history_roundtrip_preserves_undo_redo_checkpoint_and_branch_identity() {
        let mut original=editor();rename(&mut original,"first");let saved=original.checkpoint();rename(&mut original,"second");rename(&mut original,"third");original.undo().unwrap();
        let capture=prepared(&original);let mut loaded=reopen(&capture);assert_eq!(loaded.metadata.value["saved_checkpoint"],saved);
        assert_eq!(loaded.editor.checkpoint(),original.checkpoint());assert_eq!(loaded.editor.document.artwork,original.document.artwork);
        assert_eq!(loaded.editor.document.working,original.document.working);assert_eq!(loaded.editor.next_checkpoint,original.next_checkpoint);
        for redo in [false,false,true,true,true,false] {
            let expected=if redo {original.redo()} else {original.undo()}.unwrap();let actual=if redo {loaded.editor.redo()} else {loaded.editor.undo()}.unwrap();
            assert_eq!(actual,expected);assert_eq!(loaded.editor.checkpoint(),original.checkpoint());assert_eq!(loaded.editor.document.artwork,original.document.artwork);
        }
        rename(&mut loaded.editor,"branch");assert!(!loaded.editor.can_redo());assert!(loaded.editor.checkpoint()>saved);
    }
    #[test]
    fn pixel_selection_and_working_only_undo_preserve_modified_checkpoint() {
        let mut original=editor();let initial=original.checkpoint();
        let mut working=original.document.working.clone();working.selection=Some(Selection::pixels(Arc::new(SelectionPixels::new([8,2],[0,0,8,2],vec![0x43214321,0x12341234]).unwrap())));
        original.perform(Edit::Working(working)).unwrap();assert_eq!(original.checkpoint(),initial);
        let mut restored=reopen(&prepared(&original)).editor;assert_eq!(restored.document.working,original.document.working);
        restored.undo().unwrap();assert!(restored.document.working.selection.is_none());assert_eq!(restored.checkpoint(),initial);
        restored.redo().unwrap();assert_eq!(restored.document.working.selection,original.document.working.selection);
        let next=prepared(&restored);assert!(next.resources.entries.len()>0);
    }
    #[test]
    fn unchanged_history_records_are_stored_once_and_interned_after_read() {
        let mut original=editor();let paint=match original.document.working.target.unwrap(){SourceTarget::Paint(h)=>h,_=>unreachable!()};
        let paper=original.document.artwork.occurrences.iter().find(|(handle,_,_)|Some(*handle)!=original.document.working.occurrence).unwrap().0;
        original.document.artwork.occurrences.get_mut(paper).unwrap().placement.mesh=Some(Arc::new(crate::MeshMap::fit(crate::Rect::from_extent([19,11]),[1,1],Some).unwrap()));
        original.document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|x,y|[x as u8,y as u8,80,255]))));
        for i in 0..20 {rename(&mut original,&format!("name {i}"));}
        let capture=prepared(&original);let value:Value=serde_json::from_slice(capture.metadata()).unwrap();
        assert_eq!(value["objects"].as_array().unwrap().iter().filter(|v|v["record"]["type"]=="capy.paint-source/2").count(),1);
        let mut restored=reopen(&capture).editor;let first=restored.document.artwork.paint.get(paint).unwrap().base.clone().unwrap();
        let mesh=restored.document.artwork.occurrences.get(paper).unwrap().placement.mesh.clone().unwrap();
        for _ in 0..20 {restored.undo().unwrap();assert!(first.image.same_owner(&restored.document.artwork.paint.get(paint).unwrap().base.as_ref().unwrap().image));
            assert!(Arc::ptr_eq(&mesh,restored.document.artwork.occurrences.get(paper).unwrap().placement.mesh.as_ref().unwrap()));}
        prepared(&restored);
    }
    #[test]
    fn history_preserves_deleted_handle_slots() {
        let mut original=editor();let stack=original.document.composition().result;
        let paint=RecordChange::insert(&original.document.artwork.paint,original.document.artwork.paint.iter().next().unwrap().2.clone());
        let occurrence=RecordChange::insert(&original.document.artwork.occurrences,crate::Occurrence::new(crate::OccurrenceContent::Paint(paint.handle),"new"));
        let handle=occurrence.handle;let mut entries=original.document.artwork.stacks.get(stack).unwrap().clone();entries.entries.insert(0,handle);
        original.perform(Edit::Batch(vec![Edit::Paint(paint),Edit::Occurrence(occurrence),Edit::Stack(RecordChange::replace(&original.document.artwork.stacks,stack,Some(entries)).unwrap())])).unwrap();original.undo().unwrap();
        let mut restored=reopen(&prepared(&original)).editor;assert!(restored.document.artwork.occurrences.get(handle).is_none());
        restored.redo().unwrap();assert!(restored.document.artwork.occurrences.get(handle).is_some());
        restored.undo().unwrap();assert!(restored.document.artwork.occurrences.get(handle).is_none());
    }
    #[test]
    fn malformed_session_checkpoints_objects_and_working_targets_are_rejected() {
        let mut original=editor();rename(&mut original,"changed");let prepared=prepared(&original);
        let mut pack=Vec::new();std::io::Read::read_to_end(&mut prepared.resources.reader(&AtomicBool::new(false)),&mut pack).unwrap();
        let test=|value:Value|open_parts(&serde_json::to_vec(&value).unwrap(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack.clone()))).unwrap(),ProjectLimits::default(),&AtomicBool::new(false)).is_err();
        let original:Value=serde_json::from_slice(prepared.metadata()).unwrap();
        let mut value=original.clone();value["format"]=json!("old.session");assert!(test(value));
        let mut value=original.clone();value["next_checkpoint"]=json!(0);assert!(test(value));
        let mut value=original.clone();value["current"]["objects"][0]=json!(usize::MAX);assert!(test(value));
        let mut value=original.clone();value["current"]["working"]["occurrence"]=json!(u32::MAX);assert!(test(value));
        let mut value=original.clone();value["undo"][0]["checkpoint"]=value["next_checkpoint"].clone();assert!(test(value));
        let mut value=original.clone();value["undo"][0]["checkpoint"]=value["current"]["checkpoint"].clone();assert!(test(value));
        let mut value=original.clone();value["unexpected"]=json!(true);assert!(test(value));
    }
    #[test]
    fn image_ids_cannot_overlap_resources_in_a_disjoint_history_state() {
        let mut original=editor();let SourceTarget::Paint(handle)=original.document.working.target.unwrap() else {panic!()};
        let old=Image::new(crate::color::source::rgba8_source([19,11],|_,_|[17,29,81,255]));
        original.document.artwork.paint.get_mut(handle).unwrap().base=Some(PaintBase::new(old.clone()));
        let current=Image::new(crate::color::source::rgba8_source([19,11],|_,_|[81,29,17,255]));
        let mut paint=original.document.artwork.paint.get(handle).unwrap().clone();paint.base=Some(PaintBase::new(current.clone()));
        original.perform(Edit::Paint(RecordChange::replace(&original.document.artwork.paint,handle,Some(paint)).unwrap())).unwrap();
        let prepared=prepared(&original);reopen(&prepared);
        let mut value:Value=serde_json::from_slice(prepared.metadata()).unwrap();
        fn replace(value:&mut Value,before:&str,after:&str) {
            match value {
                Value::String(text) if text==before=>*text=after.into(),
                Value::Array(values)=>for value in values {replace(value,before,after);},
                Value::Object(values)=>for value in values.values_mut() {replace(value,before,after);},
                _=>{},
            }
        }
        let current_tile=current.tiles[&[0,0]].resource_id();
        replace(&mut value["objects"],&old.id().to_string(),&current_tile.to_string());
        let mut pack=Vec::new();std::io::Read::read_to_end(&mut prepared.resources.reader(&AtomicBool::new(false)),&mut pack).unwrap();
        let error=open_parts(&serde_json::to_vec(&value).unwrap(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(),ProjectLimits::default(),&AtomicBool::new(false)).err().expect("history namespace collision");
        assert!(error.contains("identity"),"{error}");
    }
    #[test]
    fn corrupted_or_missing_resources_never_adopt_a_partial_drawing() {
        let mut original=editor();let paint=match original.document.working.target.unwrap(){SourceTarget::Paint(h)=>h,_=>unreachable!()};
        original.document.artwork.paint.get_mut(paint).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|_,_|[120,80,20,255]))));
        let prepared=prepared(&original);let mut pack=Vec::new();std::io::Read::read_to_end(&mut prepared.resources.reader(&AtomicBool::new(false)),&mut pack).unwrap();
        pack[0]^=1;
        assert!(open_parts(prepared.metadata(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(),ProjectLimits::default(),&AtomicBool::new(false)).is_err());
        assert!(open_parts(prepared.metadata(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from([]))).unwrap(),ProjectLimits::default(),&AtomicBool::new(false)).is_err());
    }
    #[test]
    fn aliased_payload_owners_survive_disjoint_resource_sets_in_history() {
        let mut original=editor();let h=match original.document.working.target.unwrap(){SourceTarget::Paint(h)=>h,_=>unreachable!()};
        original.document.artwork.paint.get_mut(h).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|_,_|[120,80,20,255]))));
        let before=original.document.artwork.paint.get(h).unwrap().base.clone().unwrap();
        let mut paint=original.document.artwork.paint.get(h).unwrap().clone();paint.base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|_,_|[120,80,20,255]))));
        let after=paint.base.clone().unwrap();let before_id=before.image.tiles.values().next().unwrap().resource_id();let after_id=after.image.tiles.values().next().unwrap().resource_id();assert_ne!(before_id,after_id);
        original.perform(Edit::Paint(RecordChange::replace(&original.document.artwork.paint,h,Some(paint)).unwrap())).unwrap();
        let mut restored=reopen(&prepared(&original)).editor;
        assert_eq!(restored.document.artwork.paint.get(h).unwrap().base.as_ref().unwrap().image.tiles.values().next().unwrap().resource_id(),after_id);
        restored.undo().unwrap();assert_eq!(restored.document.artwork.paint.get(h).unwrap().base.as_ref().unwrap().image.tiles.values().next().unwrap().resource_id(),before_id);
        restored.redo().unwrap();prepared(&restored);
    }
    #[test]
    fn private_sessions_preserve_all_ancillary_data_across_deleted_subject_history() {
        let mut original=editor();let subject=original.document.working.occurrence.unwrap();let subject_id=original.document.artwork.occurrences.id(subject).unwrap();
        let payload_id=PortableId::random();let payload:Arc<[u8]>=Arc::from(b"private retained attachment".as_slice());
        let resource=Arc::new(crate::authored::OpaqueResource {id:payload_id,kind:"future.attachment/1".into(),data:json!({"subject":resources::reference(subject_id)}),encoding:"future.codec/1".into(),
            extra_fields:Default::default(),backing:ImmutableBacking::new(Arc::new(payload.clone())).unwrap(),offset:0,length:payload.len() as u64,crc32:crc32fast::hash(&payload)});
        let mut extensions=crate::authored::Extensions::default();extensions.resources.insert(payload_id,resource);
        for safe in [false,true] {let id=PortableId::random();extensions.records.insert(id,json!({"id":id,"type":"future.note/1","ancillary":true,"copy_safe":safe,
            "data":{"subject":resources::reference(subject_id),"payload":resources::reference(payload_id),"text":"keep every value"}}));}
        original.document.artwork.extensions=Arc::new(extensions);let expected=original.document.artwork.extensions.records.clone();
        original.perform(original.document.delete_layers_edit(&[subject]).unwrap()).unwrap();
        let mut restored=reopen(&prepared(&original)).editor;
        assert!(restored.document.artwork.occurrences.get(subject).is_none());assert_eq!(restored.document.artwork.extensions.records,expected);
        assert_eq!(&*restored.document.artwork.extensions.resources[&payload_id].read_chunk(0,payload.len(),&AtomicBool::new(false)).unwrap(),&*payload);
        restored.undo().unwrap();assert!(restored.document.artwork.occurrences.get(subject).is_some());assert_eq!(restored.document.artwork.extensions.records,expected);
        let again=reopen(&prepared(&restored)).editor;assert_eq!(again.document.artwork.extensions.records,expected);
    }
    #[test]
    fn small_stroke_history_reuses_large_sparse_tile_indexes() {
        use crate::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey,TILE_SIZE};
        let extent=[TILE_SIZE*48;2];let mut document=Document::new(PortableId::random(),extent[0],extent[1],DocumentNames{paint:"ink".into(),paper:"paper".into()});
        let target=document.working.target.unwrap();let SourceTarget::Paint(h)=target else{unreachable!()};
        let descriptor=RasterPlane::Color.descriptor(document.composition().color);let bytes=descriptor.byte_len([TILE_SIZE;2]).unwrap();
        let tile=RasterTile::backed(TileBlob::encode(descriptor,&vec![10;bytes]).unwrap());
        let mut data=RasterData::default();
        for y in 0..48 {for x in 0..48 {data.tiles.insert(TileKey{coordinate:[x,y],plane:RasterPlane::Color},tile.clone());}}
        document.artwork.paint.get_mut(h).unwrap().raster=RasterRevision::backed(data.clone());
        document.artwork.paint.get_mut(h).unwrap().base=Some(PaintBase::new(Image::new(crate::color::source::rgba8_source([19,11],|_,_|[120,80,20,255]))));
        let mut editor=Editor::new(document);let mut expected=Vec::new();
        for index in 0..96 {
            let tile=RasterTile::backed(TileBlob::encode(descriptor,&vec![index as u8+20;bytes]).unwrap());
            let key=TileKey{coordinate:[index%48,index/48],plane:RasterPlane::Color};
            expected.push((key,tile.try_backing().unwrap().unwrap().resource_id()));data.tiles.insert(key,tile);
            editor.perform(Edit::SetRaster{target,revision:RasterRevision::backed(data.clone())}).unwrap();
        }
        let prepared=prepared(&editor);let metadata:Value=serde_json::from_slice(prepared.metadata()).unwrap();
        assert!(prepared.metadata().len()<2*1024*1024,"small strokes must not repeat the full canvas index in every history entry");
        assert!(metadata["tile_chunks"].as_array().unwrap().len()<150);
        let original_descriptors=metadata["objects"].as_array().unwrap().iter().filter(|object|object["record"]["type"]=="capy.image/1").count();
        assert_eq!(original_descriptors,1,"immutable photo indexes must remain shared while painting");
        let mut restored=reopen(&prepared).editor;
        for (key,id) in expected.iter().rev() {
            assert_eq!(restored.document.target_raster(target).unwrap().try_data().unwrap().unwrap().tiles[key].try_backing().unwrap().unwrap().resource_id(),*id);
            restored.undo().unwrap();
        }
        for _ in &expected {restored.redo().unwrap();}
        let actual=restored.document.target_raster(target).unwrap().try_data().unwrap().unwrap();
        for (key,id) in expected {assert_eq!(actual.tiles[&key].try_backing().unwrap().unwrap().resource_id(),id);}
    }
    #[test]
    fn chunk_expansion_refuses_metadata_amplification_before_adoption() {
        for data in [Value::Null,json!(true),json!(3),json!([])] {
            for tiles in [Some(vec![0]),None] {
                let object=ObjectVersion {record:json!({"type":"capy.paint-source/2","data":data}),tiles};
                assert!(object.expand(&[vec![json!({})]],ProjectLimits::default()).is_err());
            }
        }
        let chunk=(0..RASTER_INDEX_CHUNK).map(|index|json!({"coordinate":[index,0],"resource":resources::reference(PortableId::random()),"data":"x".repeat(256)})).collect::<Vec<_>>();
        let object=ObjectVersion {record:json!({"id":PortableId::random(),"type":"capy.paint-source/2","data":{}}),tiles:Some(vec![0;256])};
        let limits=ProjectLimits {metadata_bytes:32*1024,..Default::default()};
        assert!(crate::json_len(&chunk)<limits.metadata_bytes as usize);
        assert!(object.expand(&[chunk],limits).unwrap_err().contains("exceeds admission"));
        let prepared=prepared(&editor());let mut record:SessionRecord=serde_json::from_slice(prepared.metadata()).unwrap();
        let original=json!({"extent":[1,1],"interpretation":{"data":"x".repeat(4096)}});record.tile_chunks=vec![vec![original]];
        let mut current=Vec::new();
        for _ in 0..100 {
            let index=record.objects.len();record.objects.push(ObjectVersion {record:json!({"id":PortableId::random(),"type":"capy.paint-source/2","data":{}}),tiles:Some(vec![0])});current.push(index);
        }
        record.current.objects=current;
        assert!(crate::json_len(&record)<limits.metadata_bytes as usize);
        let directory=Directory {members:vec![Member{name:"data/tiles-1.bin".into(),offset:0,length:prepared.resources.length,compressed_length:None,crc32:0}],length:prepared.resources.length};
        assert!(record.manifest(&record.current,&directory,limits).unwrap_err().contains("Expanded session state exceeds admission"));
    }
    #[test]
    fn undo_redo_checkpoint_order_rejects_corrupt_lineages() {
        assert!(validate_history_checkpoints(3,7,[2,2,0],[3,4,6]).is_ok());
        assert!(validate_history_checkpoints(3,7,[4],[]).is_err());
        assert!(validate_history_checkpoints(3,7,[2,3],[]).is_err());
        assert!(validate_history_checkpoints(3,7,[],[2]).is_err());
        assert!(validate_history_checkpoints(3,7,[],[6,4]).is_err());
        assert!(validate_history_checkpoints(3,7,[],[7]).is_err());
    }
    #[test]
    fn saved_checkpoint_validation_preserves_unreachable_saves_and_refuses_foreign_generations() {
        let mut editor=editor();assert!(editor.validate_checkpoint(0).is_ok());assert!(editor.validate_checkpoint(1).is_err());
        rename(&mut editor,"first");rename(&mut editor,"saved");let saved=editor.checkpoint();editor.undo().unwrap();rename(&mut editor,"branch");
        assert_ne!(editor.checkpoint(),saved);assert!(editor.validate_checkpoint(saved).is_ok());assert!(editor.validate_checkpoint(editor.next_checkpoint).is_err());assert!(editor.validate_checkpoint(u64::MAX).is_err());
    }
    #[test]
    fn capture_rejects_mixed_generations_and_cancelled_preparation() {
        let mut editor=editor();let stale=editor.capture(1,Default::default()).unwrap();rename(&mut editor,"changed");assert!(editor.capture_session(stale).is_err());
        let capture=editor.capture_session(editor.capture(2,Default::default()).unwrap()).unwrap();
        assert!(PreparedSession::prepare(&capture,Value::Null,&AtomicBool::new(true)).is_err());
        let mut changed=capture.clone();changed.artwork_mut().checkpoint.edit_checkpoint+=1;assert!(changed.validate_identity().is_err());
        let mut changed=capture;changed.artwork_mut().checkpoint.session_generation+=1;assert!(changed.validate_identity().is_err());
    }
    #[test]
    fn metadata_profiles_use_bounded_resources_and_keep_identity_through_transfer() {
        use super::super::session_transfer::{PreparedSessionTransfer,SessionTransferReceiver};
        let cancel=AtomicBool::new(false);let editor=editor();let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        for size in [1024*1024,crate::color::source::MAX_PROFILE_BYTES] {
            let bytes=crate::authored::Resource::from((0..size).map(|index|((index*73)^(index>>13)) as u8).collect::<Vec<_>>());let id=bytes.id();
            let profile=crate::color::ColorProfile::Icc(bytes.clone());
            let metadata=SessionMetadata {value:json!({"camera":1,"profile":id}),profiles:vec![profile.clone()]};
            let prepared=PreparedSession::prepare(&capture,metadata.clone(),&cancel).unwrap();assert!(prepared.metadata().len()<16*1024);
            let opened=reopen(&prepared);assert_eq!(opened.metadata.value,metadata.value);
            let crate::color::ColorProfile::Icc(restored)=&opened.metadata.profiles[0] else{panic!()};assert_eq!(restored.id(),id);assert_eq!(restored.as_ref(),bytes.as_ref());
            let transfer=PreparedSessionTransfer::capture(&capture,metadata.clone(),&cancel).unwrap();assert!(crate::json_len(transfer.descriptor())<16*1024);
            let descriptor=serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
            let mut receiver=SessionTransferReceiver::new(descriptor,ProjectLimits::default()).unwrap();
            for index in 0..transfer.payload_count() {let length=transfer.payload_len(index).unwrap();let mut offset=0;
                while offset<length {let count=(length-offset).min(super::super::MAX_RANGE_BYTES as u64) as usize;receiver.push_chunk(index,&transfer.read_chunk(index,offset,count).unwrap()).unwrap();offset+=count as u64;}}
            let opened=receiver.finish().unwrap().adopt_verified(ProjectLimits::default(),&cancel).unwrap();
            let crate::color::ColorProfile::Icc(restored)=&opened.metadata.profiles[0] else{panic!()};assert_eq!(restored.id(),id);assert_eq!(restored.as_ref(),bytes.as_ref());
            let next=PreparedSessionTransfer::capture(&capture,SessionMetadata {value:json!({"camera":2,"profile":id}),profiles:vec![profile]},&cancel).unwrap();
            let reused=SessionTransferReceiver::new_reusing(next.descriptor().clone(),ProjectLimits::default(),&transfer).unwrap();assert!(reused.missing_payloads().is_empty());
            let worker_editor=opened.editor;let worker_capture=worker_editor.capture_session(worker_editor.capture(2,Default::default()).unwrap()).unwrap();
            let prepared=PreparedSession::prepare(&worker_capture,opened.metadata,&cancel).unwrap();assert!(prepared.resources().entries.iter().any(|entry|entry.payload.id()==id));
            let mut invalid:Value=serde_json::from_slice(prepared.metadata()).unwrap();invalid["metadata_profiles"][0]=json!({"resource":resources::reference(PortableId::random())});
            let mut pack=Vec::new();std::io::Read::read_to_end(&mut prepared.resources.reader(&cancel),&mut pack).unwrap();
            assert!(open_parts(&serde_json::to_vec(&invalid).unwrap(),ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(),ProjectLimits::default(),&cancel).is_err());
        }
        let bytes=crate::authored::Resource::from(vec![1;4]);let conflict=crate::authored::Resource::with_id(bytes.id(),Arc::<[u8]>::from(vec![2;4]));
        assert!(PreparedSession::prepare(&capture,SessionMetadata {value:Value::Null,profiles:vec![crate::color::ColorProfile::Icc(bytes),crate::color::ColorProfile::Icc(conflict)]},&cancel).is_err());
        assert!(PreparedSession::prepare(&capture,SessionMetadata {value:Value::Null,profiles:vec![crate::color::ColorProfile::Icc(vec![1;crate::color::source::MAX_PROFILE_BYTES+1].into())]},&cancel).is_err());
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn camera_checkpoints_reuse_metadata_profiles_and_retirement_collects_them() {
        use super::super::session_store::{SessionStore,collect_unreferenced_stores};
        let root=std::env::temp_dir().join(format!("capy-session-profile-{}",PortableId::random()));let path=root.join("drawing");let cancel=AtomicBool::new(false);
        let editor=editor();let capture=editor.capture_session(editor.capture(1,Default::default()).unwrap()).unwrap();
        let bytes=crate::authored::Resource::from(vec![37;crate::color::source::MAX_PROFILE_BYTES]);let id=bytes.id();let profile=crate::color::ColorProfile::Icc(bytes);
        let mut store=SessionStore::open(&path).unwrap();let metadata=|camera|SessionMetadata {value:json!({"camera":camera,"profile":id}),profiles:vec![profile.clone()]};
        store.commit(&PreparedSession::prepare(&capture,metadata(0),&cancel).unwrap(),&cancel).unwrap();
        let resource_path=path.join("resources").join(format!("{id}.bin"));let timestamp=std::time::SystemTime::UNIX_EPOCH+std::time::Duration::from_secs(10);
        std::fs::OpenOptions::new().write(true).open(&resource_path).unwrap().set_times(std::fs::FileTimes::new().set_modified(timestamp)).unwrap();
        for camera in 1..21 {let prepared=PreparedSession::prepare(&capture,metadata(camera),&cancel).unwrap();assert!(prepared.metadata().len()<16*1024);store.commit(&prepared,&cancel).unwrap();}
        assert_eq!(std::fs::metadata(&resource_path).unwrap().modified().unwrap(),timestamp);
        assert_eq!(std::fs::read_dir(path.join("resources")).unwrap().count(),1);
        let opened=store.load(ProjectLimits::default(),&cancel).unwrap().unwrap();assert_eq!(opened.metadata.value["camera"],20);
        let crate::color::ColorProfile::Icc(bytes)=&opened.metadata.profiles[0] else{panic!()};assert_eq!(bytes.id(),id);assert_eq!(bytes.len(),crate::color::source::MAX_PROFILE_BYTES);
        drop(opened);store.retire().unwrap();drop(store);collect_unreferenced_stores(&root,&Default::default(),&cancel).unwrap();
        assert!(!resource_path.exists());std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path="session_tests.rs"]
mod persistence_tests;
