use super::*;
use layer_core::{authored::ArtworkCapture, package::{ImmutableBacking, session::{EditorCapture, PreparedSession, OpenSession, SessionMetadata}}, PortableId, ProjectLimits};
use std::{collections::BTreeSet, sync::atomic::AtomicBool};

pub const CHECKPOINT_INTERVAL_MS: u64 = 2_000;
pub const MAX_SESSION_DRAWINGS: usize = 1024;
pub const MAX_SESSION_DRAWING_ID: u64 = (1u64<<53)-1;
const MAX_MANIFEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCamera {
    pub center: [f32; 2],
    pub zoom: f32,
    pub rotation: f32,
    pub flipped: [bool; 2],
    pub zoom_locked: bool,
    pub rotation_locked: bool,
}
impl SessionCamera {
    pub(super) fn capture(camera: &Camera) -> Self {
        let Camera {revision:_,viewport:_,zoom,rotation,zoom_locked,rotation_locked,flipped,translation:_,work_area:_}=camera;
        let [x,y] = camera.work_area_center();
        let center = camera.input_transform().map(layer_core::Point {x,y});
        Self {center:[center.x,center.y],zoom:*zoom,rotation:*rotation,
            flipped:*flipped,zoom_locked:*zoom_locked,rotation_locked:*rotation_locked}
    }
    pub(super) fn restore(&self, current: &Camera) -> Result<Camera,String> {
        if !self.center.into_iter().chain([self.zoom,self.rotation]).all(f32::is_finite)
            || !(crate::camera::MIN_ZOOM..=crate::camera::MAX_ZOOM).contains(&self.zoom) {
            return Err("Invalid session camera".into());
        }
        let mut camera=current.clone();
        camera.zoom=self.zoom;camera.rotation=self.rotation;camera.flipped=self.flipped;
        camera.zoom_locked=self.zoom_locked;camera.rotation_locked=self.rotation_locked;
        camera.center_on(self.center);
        if !camera.document_to_surface().into_iter().all(f32::is_finite) {return Err("Invalid session camera".into());}
        camera.revision=current.revision.checked_add(1).ok_or("Camera generation exhausted")?;
        Ok(camera)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationFingerprint {pub bytes:u64,pub sha256:String}
impl DestinationFingerprint {
    pub fn validate(&self)->Result<(),String> {
        if self.sha256.len()!=64 || !self.sha256.bytes().all(|c|c.is_ascii_hexdigit()) {return Err("Invalid saved-file fingerprint".into());}
        Ok(())
    }
}

pub type SessionExport = ExportRepeat<ExportProfile<layer_core::color::ProfileReference<PortableId>>>;
pub(super) fn detach_export(last:&ExportRepeat)->SessionExport {
    use layer_core::color::{ColorProfile,ProfileReference};
    let profile=match &last.recipe.profile.profile {ColorProfile::Builtin(space)=>ProfileReference::Builtin(*space),ColorProfile::Icc(bytes)=>ProfileReference::Embedded(bytes.id())};
    ExportRepeat {recipe:last.recipe.clone().with_profile(last.recipe.profile.clone().with_profile(profile)),location:last.location.clone()}
}
fn resolve_export(last:&SessionExport,profiles:&[layer_core::color::ColorProfile])->Result<ExportRepeat,String> {
    use layer_core::color::{ColorProfile,ProfileReference};
    let profile=match &last.recipe.profile.profile {
        ProfileReference::Builtin(space) if profiles.is_empty()=>ColorProfile::Builtin(*space),
        ProfileReference::Embedded(id)=>match profiles {
            [ColorProfile::Icc(bytes)] if bytes.id()==*id && !bytes.is_empty() && bytes.len()<=layer_core::color::source::MAX_PROFILE_BYTES=>ColorProfile::Icc(bytes.clone()),
            _=>return Err("Missing or conflicting session export profile".into()),
        },
        _=>return Err("Unused session export profile".into()),
    };
    let resolved=ExportRepeat {recipe:last.recipe.clone().with_profile(last.recipe.profile.clone().with_profile(profile)),location:last.location.clone()};
    Ok(resolved)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDocumentState {
    pub camera: SessionCamera,
    pub location: Option<DocumentLocation>,
    pub unsaved_name: Option<String>,
    pub saved_checkpoint: u64,
    pub unpublished: bool,
    pub recovered: bool,
    pub destination: Option<DestinationFingerprint>,
    #[serde(deserialize_with="deserialize_last_export")]
    pub last_export: Option<SessionExport>,
}
fn deserialize_last_export<'de,D:serde::Deserializer<'de>>(deserializer:D)->Result<Option<SessionExport>,D::Error> {
    let value=Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value)=value else{return Ok(None)};
    let decoded:SessionExport=serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
    if serde_json::to_value(&decoded).map_err(serde::de::Error::custom)?!=value {
        return Err(serde::de::Error::custom("Invalid session export destination"));
    }
    Ok(Some(decoded))
}
impl SessionDocumentState {
    fn validate(&self)->Result<(),String> {
        if let Some(location)=&self.location {location.validate().map_err(|_|"Invalid session save destination")?;}
        if self.unsaved_name.as_ref().is_some_and(|name|name.len()>1024 || name.chars().any(char::is_control)) {return Err("Invalid session drawing name".into());}
        if let Some(destination)=&self.destination {destination.validate()?;}
        if let Some(last)=&self.last_export {
            last.location.validate().map_err(|_|"Invalid session export destination")?;
            if last.recipe.profile.name.len()>1024 || last.recipe.profile.name.chars().any(char::is_control) {
                return Err("Invalid session export profile".into());
            }

        }
        self.camera.restore(&Camera::new([1,1],[1,1]))?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionStamp {
    pub artwork:PortableId,
    pub camera_navigation:u64,
    pub epoch:u64,
    pub revision:u64,
    pub working_generation:u64,
    pub checkpoint:u64,
    pub state:SessionDocumentState,
}
impl SessionStamp {
    pub fn same_editor(&self,other:&Self)->bool {
        let Self {artwork,camera_navigation:_,epoch,revision,working_generation,checkpoint,state:_}=self;
        *artwork==other.artwork && *epoch==other.epoch && *revision==other.revision && *working_generation==other.working_generation && *checkpoint==other.checkpoint
    }
}

#[derive(Clone, Debug)]
pub struct SessionCapture {pub editor:EditorCapture,pub state:SessionDocumentState,profiles:Vec<layer_core::color::ColorProfile>}
impl SessionCapture {
    pub fn artwork(&self)->&ArtworkCapture {self.editor.artwork()}
    pub fn artwork_mut(&mut self)->&mut ArtworkCapture {self.editor.artwork_mut()}
    pub fn metadata(&self)->Result<SessionMetadata,String> {
        self.state.validate()?;
        if let Some(last)=&self.state.last_export {resolve_export(last,&self.profiles)?;} else if !self.profiles.is_empty() {return Err("Unused session export profile".into());}
        Ok(SessionMetadata {value:serde_json::to_value(&self.state).map_err(|e|e.to_string())?,profiles:self.profiles.clone()})
    }
    pub fn prepare(&self,cancel:&AtomicBool)->Result<PreparedSession,String> {
        let metadata=self.metadata()?;
        if let Some(last)=&self.state.last_export {resolve_export(last,&self.profiles)?.recipe.validate().map_err(|_|"Invalid session export recipe")?;}
        PreparedSession::prepare(&self.editor,metadata,cancel)
    }
}

pub struct SessionRestore {pub editor:layer_core::Editor,pub state:SessionDocumentState,last_export:Option<ExportRepeat>}
impl SessionRestore {
    fn decode_metadata(metadata:SessionMetadata)->Result<(SessionDocumentState,Option<ExportRepeat>),String> {
        let SessionMetadata {value,profiles}=metadata;
        let state:SessionDocumentState=serde_json::from_value(value).map_err(|e|e.to_string())?;
        state.validate()?;
        let last_export=state.last_export.as_ref().map(|last|resolve_export(last,&profiles)).transpose()?;
        if last_export.is_none() && !profiles.is_empty() {return Err("Unused session export profile".into());}
        if let Some(last)=&last_export {last.recipe.validate().map_err(|_|"Invalid session export recipe")?;}
        Ok((state,last_export))
    }
    pub fn validate_core(open:&OpenSession)->Result<(),String> {
        let OpenSession {editor,metadata}=open;
        let (state,_)=Self::decode_metadata(metadata.clone())?;
        editor.validate_checkpoint(state.saved_checkpoint).map_err(error)
    }
    pub fn from_core(open:OpenSession)->Result<Self,String> {
        let OpenSession {editor,metadata}=open;
        let (state,last_export)=Self::decode_metadata(metadata)?;
        editor.validate_checkpoint(state.saved_checkpoint).map_err(error)?;
        Ok(Self {editor,state,last_export})
    }
    pub fn open(backing:ImmutableBacking,limits:ProjectLimits,cancel:&AtomicBool)->Result<Self,String> {
        Self::from_core(layer_core::package::session::open(backing,limits,cancel)?)
    }
    pub fn document(&self)->&Document {self.editor.document()}
}

impl<R:CanvasRenderer> UiSession<R> {
    pub(super) fn camera_navigation_revision(&self)->u64 {
        self.state.camera.revision.checked_sub(self.automatic_camera_revision).expect("Automatic camera revision exceeds camera revision")
    }
    fn session_document_state(&self)->SessionDocumentState {
        let UiState {camera,document_file,tool_slots:_,histogram:_,waveform:_,tonal_histogram:_,localization:_,command_search:_,soft_proof:_,preview_sdr:_,hdr_display_available:_,screen:_,gamut_warning:_,revision:_,fullscreen:_,workspace:_,brush:_,colors:_,color_library:_,color_picker:_,tool_settings:_,tool_extra:_,toolbar_context_generation:_,tool_actions:_,tool_set:_,tool_panels:_,canvas_bar:_,layers:_,layer_tools:_,adjustments:_,filter_picker:_,filter_categories:_,filter_catalog_revision:_,filter_load:_,layer_properties:_,tabs:_,commands:_,settings:_,theme:_,palette:_,settings_open:_,preferences:_,customization:_,platform:_,requests:_,host_error:_,notice:_}=&self.state;
        self.files.session_state(document_file,SessionCamera::capture(camera))
    }
    pub fn session_stamp(&self)->SessionStamp {
        SessionStamp {artwork:self.engine.document().artwork.id,camera_navigation:self.camera_navigation_revision(),epoch:self.state.document_file.epoch,revision:self.engine.document().revision,
            working_generation:self.engine.document().working.generation,checkpoint:self.engine.checkpoint(),state:self.session_document_state()}
    }
    pub fn capture_session(&self)->Result<SessionCapture,String> {
        let Self {engine,state,files:_,screen_headroom:_,histogram_captions:_,histogram:_,effect_analyses:_,
            tonal_histogram:_,auto_levels:_,targeted_curve:_,localization_generation:_,renderer_generation:_,preferences_revision:_,
            panel_copy:_,customization_copy:_,command_search:_,last_toolbar_context:_,canvas_bar:_,notices:_,
            pen:_,input_pending:_,host_requests_changed:_,pen_contact:_,input_held:_,rendering_suspended:_,touch:_,navigator_drag:_,
            effect_gesture:_,object_motion:_,property_editor:_,sdr_gesture:_,last_proof_mode:_,proof_setup_pending:_,filter_previews:_,
            eyedropper:_,region_tools:_,selection_tools:_,tonal_tools:_,painted_selections:_,deferred_edits:_,selection_masks:_,
            canvas_size:_,image_size:_,frequency_separation:_,content_bounds:_,conversion:_,rulers:_,retouch:_,operation:_,objects:_,
            system_theme:_,system_accent:_,platform_prediction_available:_,logical_viewport:_,initial_fit:_,
            automatic_camera_revision:_,divider_drag:_,floating_resize:_,workspace_drag:_,workspace_tab_drag:_,
            workspace_drag_tabs:_,workspace_model_revision:_,workspace_content_revision:_,workspace_history:_,
            workspace_transition:_,workspace_read_only:_,workspace_preview:_,managed_workspace:_,interaction:_,
            cursor:_,next_request:_,layer_interaction:_,layer_preview_revisions:_,effect_catalog:_,pending_filters:_,
            tools:_,tool_origin:_,pending_tool_drawer:_}=self;
        self.require_raster_snapshot()?;
        let profiles=self.files.last_export.as_ref().and_then(|last|match &last.recipe.profile.profile {layer_core::color::ColorProfile::Icc(bytes)=>Some(layer_core::color::ColorProfile::Icc(bytes.clone())),_=>None}).into_iter().collect();
        Ok(SessionCapture {editor:engine.capture_session(state.document_file.epoch).map_err(error)?,state:self.session_document_state(),profiles})
    }
    pub fn restore_session(&mut self,restore:SessionRestore,recovered:bool,observed:Option<DestinationFingerprint>)->Result<UiChange,String> {
        self.require_document_snapshot_idle()?;
        restore.state.validate()?;
        restore.editor.validate_checkpoint(restore.state.saved_checkpoint).map_err(error)?;
        let camera=restore.state.camera.restore(&self.state.camera)?;
        self.engine.restore_editor(restore.editor).map_err(error)?;
        self.state.camera=camera;
        self.initial_fit=false;
        self.files.saved_checkpoint=restore.state.saved_checkpoint;
        self.files.startup=false;
        self.files.unpublished=restore.state.unpublished || (restore.state.location.is_some()
            && !restore.state.destination.as_ref().zip(observed.as_ref()).is_some_and(|(expected,actual)|expected==actual));
        self.files.destination=restore.state.destination;
        self.files.check_destination=restore.state.location.is_some();
        self.state.document_file.export_uri=restore.state.last_export.as_ref().map(|last|last.location.uri.clone());
        self.files.last_export=restore.last_export;
        self.state.document_file.location=restore.state.location;
        self.state.document_file.unsaved_name=restore.state.unsaved_name;
        self.state.document_file.recovered=restore.state.recovered || recovered;
        self.sync_camera();self.refresh_document();self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT|regions::COMMANDS|regions::HOST|regions::CAMERA,true))
    }
    pub fn can_replace_startup_session(&self,stamp:&SessionStamp)->bool {
        let mut current=self.session_stamp();
        current.state.camera=stamp.state.camera.clone();
        self.document_park_interaction_idle() && current==*stamp && !self.state.document_file.modified && !self.state.document_file.busy
            && self.state.document_file.location.is_none() && !self.engine.can_undo() && !self.engine.can_redo()
    }
    pub fn request_session_close(&mut self)->Result<UiChange,String> {
        self.require_document_snapshot_idle()?;
        if self.files.pending.is_some() {return Err("Wait for the current file operation".into());}
        self.state.document_file.close_ready=true;
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT|regions::COMMANDS|regions::HOST,false))
    }
    pub fn record_destination_fingerprint(&mut self,location:&DocumentLocation,fingerprint:DestinationFingerprint)->Result<(),String> {
        fingerprint.validate()?;
        if self.state.document_file.location.as_ref()!=Some(location) {return Err("The save destination changed".into());}
        self.files.destination=Some(fingerprint);self.files.check_destination=false;
        Ok(())
    }
    pub fn destination_matches(&self,observed:Option<&DestinationFingerprint>)->bool {
        match (&self.files.destination,observed) {
            (Some(expected),Some(actual))=>expected==actual,
            (None,_)=>!self.files.check_destination,
            _=>false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionDrawing {pub id:u64,pub key:String}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRestoreAttempt {pub id:u64,pub generation:u64}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionManifest {
    pub generation:u64,
    pub drawings:Vec<SessionDrawing>,
    pub active:u64,
    pub clean_exit:bool,
    pub restoring:Vec<SessionRestoreAttempt>,
    pub blocked:Vec<u64>,
}
impl SessionManifest {
    pub fn validate(&self)->Result<(),String> {
        let mut ids=BTreeSet::new();let mut keys=BTreeSet::new();
        if self.generation>MAX_SESSION_DRAWING_ID || self.drawings.len()>MAX_SESSION_DRAWINGS || self.drawings.iter().any(|drawing|drawing.id==0 || drawing.id>MAX_SESSION_DRAWING_ID
            || drawing.key.is_empty() || drawing.key.len()>128 || !drawing.key.bytes().all(|c|c.is_ascii_alphanumeric()||c==b'-'||c==b'_')
            || !ids.insert(drawing.id) || !keys.insert(&drawing.key))
            || (self.drawings.is_empty() && self.active!=0) || (!self.drawings.is_empty() && !ids.contains(&self.active))
            || self.restoring.iter().any(|attempt|!ids.contains(&attempt.id) || self.blocked.contains(&attempt.id) || attempt.generation==0 || attempt.generation>self.generation)
            || self.restoring.iter().map(|attempt|attempt.id).collect::<BTreeSet<_>>().len()!=self.restoring.len()
            || self.blocked.iter().any(|id|!ids.contains(id))
            || self.blocked.iter().collect::<BTreeSet<_>>().len()!=self.blocked.len() {
            return Err("Invalid saved session".into());
        }
        Ok(())
    }
    pub fn parse(bytes:&[u8])->Result<Self,String> {
        if bytes.len()>MAX_MANIFEST_BYTES {return Err("Saved session is too large".into());}
        let manifest:Self=serde_json::from_slice(bytes).map_err(|e|e.to_string())?;manifest.validate()?;Ok(manifest)
    }
    fn next(&self)->Result<Self,String> {
        let mut next=self.clone();next.generation=next.generation.checked_add(1).filter(|generation|*generation<=MAX_SESSION_DRAWING_ID).ok_or("Session generation exhausted")?;Ok(next)
    }
    pub fn reconcile(&self,drawings:Vec<SessionDrawing>,active:u64,clean_exit:bool)->Result<Self,String> {
        if self.drawings==drawings && self.active==active && self.clean_exit==clean_exit {self.validate()?;return Ok(self.clone());}
        if self.drawings.iter().any(|previous|!drawings.iter().any(|next|next==previous)) {
            return Err("Close drawings explicitly before removing them from the saved session".into());
        }
        let mut next=self.next()?;
        next.drawings=drawings;next.active=active;next.clean_exit=clean_exit;
        next.blocked.retain(|id|next.drawings.iter().any(|drawing|drawing.id==*id));
        next.validate()?;Ok(next)
    }
    pub fn stage(&self,drawings:Vec<SessionDrawing>,active:u64)->Result<Self,String> {
        self.validate()?;
        let mut members=self.drawings.clone();
        let mut seen=BTreeSet::new();
        for drawing in drawings {
            if !seen.insert(drawing.id) {return Err("Duplicate staged drawing identity".into());}
            match members.iter().find(|member|member.id==drawing.id) {
                Some(member) if member!=&drawing=>return Err("Saved drawing identity cannot be rebound".into()),
                Some(_)=>{},
                None=>members.push(drawing),
            }
        }
        self.reconcile(members,if active==0 {self.active}else{active},false)
    }
    pub fn remove(&self,id:u64)->Result<Self,String> {
        if id==0 || id>MAX_SESSION_DRAWING_ID {return Err("Invalid drawing identity".into());}
        self.validate()?;
        if !self.drawings.iter().any(|drawing|drawing.id==id) {return Ok(self.clone());}
        let mut next=self.next()?;
        if next.active==id {next.active=crate::document_tabs::drawing_after_close(next.drawings.iter().map(|drawing|drawing.id),id).unwrap_or(0);}
        next.drawings.retain(|drawing|drawing.id!=id);next.blocked.retain(|blocked|*blocked!=id);
        next.restoring.retain(|pending|pending.id!=id);
        next.validate()?;Ok(next)
    }
    pub fn remap(&self,mapping:&[(u64,u64)])->Result<Self,String> {
        self.validate()?;
        let mut sources=BTreeSet::new();let mut targets=BTreeSet::new();
        if mapping.iter().any(|(from,to)|!sources.insert(*from) || !targets.insert(*to) || *to==0 || *to>MAX_SESSION_DRAWING_ID || !self.drawings.iter().any(|drawing|drawing.id==*from)) {
            return Err("Invalid restored drawing identity mapping".into());
        }
        if mapping.iter().all(|(from,to)|from==to) {return Ok(self.clone());}
        let identity=|id:u64|mapping.iter().find_map(|(from,to)|(*from==id).then_some(*to)).unwrap_or(id);
        let mut next=self.next()?;
        for drawing in &mut next.drawings {drawing.id=identity(drawing.id);}
        next.active=identity(next.active);
        for attempt in &mut next.restoring {attempt.id=identity(attempt.id);}
        for id in &mut next.blocked {*id=identity(*id);}
        next.validate()?;Ok(next)
    }
    pub fn recover_interrupted(&self)->Result<Self,String> {
        let mut next=self.next()?;
        for attempt in std::mem::take(&mut next.restoring) {if !next.blocked.contains(&attempt.id) {next.blocked.push(attempt.id);}}
        next.validate()?;Ok(next)
    }
    pub fn begin_restore(&self,id:u64)->Result<Self,String> {
        if self.restoring.iter().any(|attempt|attempt.id==id) || self.blocked.contains(&id) || !self.drawings.iter().any(|drawing|drawing.id==id) {return Err("Drawing cannot be restored automatically".into());}
        let mut next=self.next()?;next.restoring.push(SessionRestoreAttempt{id,generation:next.generation});next.validate()?;Ok(next)
    }
    pub fn finish_restore(&self,attempt:SessionRestoreAttempt,success:bool)->Result<Self,String> {
        if !self.restoring.contains(&attempt) {return Err("Stale session restore completion".into());}
        let mut next=self.next()?;next.restoring.retain(|pending|*pending!=attempt);
        if !success && !next.blocked.contains(&attempt.id) {next.blocked.push(attempt.id);}
        next.validate()?;Ok(next)
    }
    pub fn retry_restore(&self,id:u64)->Result<Self,String> {
        let mut next=self.clone();next.blocked.retain(|blocked|*blocked!=id);next.begin_restore(id)
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn read(path:&std::path::Path)->Result<Option<Self>,String> {
        use std::io::Read;
        let file=match std::fs::File::open(path) {Ok(file)=>file,Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(e)=>return Err(e.to_string())};
        let mut bytes=Vec::new();file.take(MAX_MANIFEST_BYTES as u64+1).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        Self::parse(&bytes).map(Some)
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn publish(&self,path:&std::path::Path)->Result<(),String> {
        self.publish_checked(path).map_err(|failure|failure.error)
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn publish_checked(&self,path:&std::path::Path)->Result<(),layer_core::package::session_store::AtomicReplaceError> {
        use layer_core::package::session_store::{AtomicReplaceError,atomic_replace_checked,sync_directory};
        let unpublished=|error:String|AtomicReplaceError{published:false,error};
        self.validate().map_err(unpublished)?;
        if let Some(previous)=Self::read(path).map_err(unpublished)? {
            if previous==*self {
                let parent=path.parent().ok_or_else(||unpublished("Session has no parent directory".into()))?;
                return sync_directory(parent).map_err(|error|AtomicReplaceError{published:true,error});
            }
            if self.generation<=previous.generation {return Err(unpublished("Stale session publication".into()));}
        }
        let bytes=serde_json::to_vec(self).map_err(|e|unpublished(e.to_string()))?;
        if bytes.len()>MAX_MANIFEST_BYTES {return Err(unpublished("Saved session is too large".into()));}
        atomic_replace_checked(path,&bytes)
    }
}

#[derive(Deserialize)]
#[serde(tag="type",rename_all="snake_case")]
pub enum SessionManifestEvent {
    Stage {drawings:Vec<SessionDrawing>,active:u64},
    Remap {mapping:Vec<(u64,u64)>},
    Reconcile {drawings:Vec<SessionDrawing>,active:u64,clean_exit:bool},
    Remove {id:u64},
    Interrupted,
    BeginRestore {id:u64},
    FinishRestore {attempt:SessionRestoreAttempt,success:bool},
    RetryRestore {id:u64},
}
pub fn session_manifest_update(state:&str,event:SessionManifestEvent)->Result<SessionManifest,String> {
    let manifest=if state.is_empty(){SessionManifest::default()}else{SessionManifest::parse(state.as_bytes())?};
    match event {
        SessionManifestEvent::Stage{drawings,active}=>manifest.stage(drawings,active),
        SessionManifestEvent::Remap{mapping}=>manifest.remap(&mapping),
        SessionManifestEvent::Reconcile{drawings,active,clean_exit}=>manifest.reconcile(drawings,active,clean_exit),
        SessionManifestEvent::Remove{id}=>manifest.remove(id),
        SessionManifestEvent::Interrupted=>manifest.recover_interrupted(),
        SessionManifestEvent::BeginRestore{id}=>manifest.begin_restore(id),
        SessionManifestEvent::FinishRestore{attempt,success}=>manifest.finish_restore(attempt,success),
        SessionManifestEvent::RetryRestore{id}=>manifest.retry_restore(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::{session,layer,invoke,Recorder};
    use std::sync::Arc;
    fn manifest()->SessionManifest {
        SessionManifest::default().reconcile(vec![SessionDrawing{id:7,key:"drawing-a".into()},SessionDrawing{id:2,key:"drawing-b".into()}],2,false).unwrap()
    }
    fn attempt(manifest:&SessionManifest,id:u64)->SessionRestoreAttempt {*manifest.restoring.iter().find(|attempt|attempt.id==id).unwrap()}
    fn encode(session:&UiSession<Recorder>)->Vec<u8> {
        let cancel=AtomicBool::new(false);let mut bytes=Vec::new();
        session.capture_session().unwrap().prepare(&cancel).unwrap().write(&mut bytes,&cancel).unwrap();bytes
    }
    fn restore(bytes:&[u8],interrupted:bool)->UiSession<Recorder> {
        let source=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
        let restored=SessionRestore::open(source,ProjectLimits::default(),&AtomicBool::new(false)).unwrap();
        let mut session=UiSession::from_project(Recorder::default(),restored.document().clone(),None,[500,400],Platform::Gtk).unwrap();
        session.frame(0,0).unwrap();let observed=restored.state.destination.clone();session.restore_session(restored,interrupted,observed).unwrap();session
    }
    fn save(session:&mut UiSession<Recorder>,success:bool) {
        invoke(session,CommandId::SaveDocument);
        let request=session.state().requests.iter().find(|request|matches!(request.kind,HostRequestKind::Document{request:DocumentRequest::Save{..}})).unwrap().id;
        session.capture_project_save(request,DocumentLocation{uri:"private:painting.capy".into(),name:"Painting.capy".into()}).unwrap();
        session.complete_document_request(request,Ok(success)).unwrap();
        if success {let location=session.state.document_file.location.clone().unwrap();session.record_destination_fingerprint(&location,DestinationFingerprint::read(&b"saved master"[..]).unwrap()).unwrap();}
    }
    fn restore_observed(bytes:&[u8],observed:Option<DestinationFingerprint>)->UiSession<Recorder> {
        let source=ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
        let restored=SessionRestore::open(source,ProjectLimits::default(),&AtomicBool::new(false)).unwrap();
        let mut session=UiSession::from_project(Recorder::default(),restored.document().clone(),None,[500,400],Platform::Gtk).unwrap();
        session.frame(0,0).unwrap();session.restore_session(restored,false,observed).unwrap();session
    }
    #[test]
    fn restored_saved_drawing_requires_matching_original_before_clean_close() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(1,1).unwrap();save(&mut source,true);
        let saved=source.files.saved_checkpoint;let expected=source.files.destination.clone().unwrap();let bytes=encode(&source);
        let mut intact=restore_observed(&bytes,Some(expected.clone()));assert!(!intact.state.document_file.modified);
        intact.request_document_close().unwrap();assert!(intact.state.document_file.close_ready);assert!(intact.state.requests.is_empty());
        let changed=DestinationFingerprint::read(&b"other master"[..]).unwrap();assert_eq!(changed.bytes,expected.bytes);
        for observed in [None,Some(changed)] {
            let mut protected=restore_observed(&bytes,observed.clone());assert!(protected.state.document_file.modified);
            assert_eq!(protected.files.saved_checkpoint,saved);assert_eq!(protected.files.destination,Some(expected.clone()));
            assert!(!protected.destination_matches(observed.as_ref()));
            protected.request_document_close().unwrap();let request=protected.state.requests.last().unwrap().id;
            protected.respond_document_close(request,CloseDecision::Cancel).unwrap();assert!(!protected.state.document_file.close_ready);
            layer(&mut protected,LayerAction::New{group:false,clipped:false});protected.frame(2,2).unwrap();
            invoke(&mut protected,CommandId::Undo);protected.frame(3,3).unwrap();assert_eq!(protected.engine.checkpoint(),saved);assert!(protected.state.document_file.modified);
            save(&mut protected,false);assert!(protected.state.document_file.modified);
            save(&mut protected,true);assert!(!protected.state.document_file.modified);
        }
        source.files.destination=None;let no_expected=restore_observed(&encode(&source),Some(expected));assert!(no_expected.state.document_file.modified);
        let mut blank=session(Platform::Gtk);blank.frame(0,0).unwrap();assert!(!restore_observed(&encode(&blank),None).state.document_file.modified);
    }
    #[test]
    fn restored_export_again_retains_recipe_target_and_saved_checkpoint() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();save(&mut source,true);
        let remembered=ExportRepeat {recipe:ExportRecipe::web_share(),location:DocumentLocation {uri:"private:export.png".into(),name:"Export.png".into()}};
        source.files.last_export=Some(remembered.clone());
        let mut restored=restore(&encode(&source),false);let checkpoint=restored.engine.checkpoint();let saved=restored.files.saved_checkpoint;
        assert!(!restored.state.document_file.modified);assert_eq!(restored.state.document_file.export_uri.as_deref(),Some("private:export.png"));
        for result in [Ok(false),Err("private export failed".into())] {
            invoke(&mut restored,CommandId::ExportAgain);let request=restored.state.requests.last().unwrap().id;
            let Ok(DocumentRequest::Export {repeat:Some(repeat),..})=restored.document_request(request) else {panic!("missing export repeat")};
            assert_eq!(repeat,&remembered);restored.complete_document_request(request,result).unwrap();
            assert_eq!(restored.files.last_export,Some(remembered.clone()));assert_eq!(restored.engine.checkpoint(),checkpoint);
            assert_eq!(restored.files.saved_checkpoint,saved);assert!(!restored.state.document_file.modified);
        }
        let capture=source.capture_session().unwrap();let original=capture.metadata().unwrap().value;
        let mut malformed=capture.clone();malformed.state.last_export.as_mut().unwrap().recipe.jpeg_quality=0;
        let metadata=malformed.metadata().unwrap();
        let opened=OpenSession {editor:layer_core::Editor::new(source.engine.document().clone()),metadata};
        assert_eq!(SessionRestore::validate_core(&opened),Err("Invalid session export recipe".into()));assert!(malformed.prepare(&AtomicBool::new(false)).is_err());
        for kind in 0..4 {
            let mut value=original.clone();match kind {
                0=>{value.as_object_mut().unwrap().remove("last_export");},
                1=>{value["last_export"]["recipe"].as_object_mut().unwrap().remove("metadata");},
                2=>{value["last_export"]["recipe"]["profile"]["unknown"]=true.into();},
                _=>{value["last_export"]["recipe"]["jpeg_quality"]=0.into();},
            }
            let state=serde_json::from_value::<SessionDocumentState>(value);
            assert!(state.as_ref().map_or(true,|state|state.validate().is_err() || state.last_export.as_ref().is_some_and(|last|resolve_export(last,&[]).and_then(|last|last.recipe.validate().map_err(|_|"Invalid session export recipe".into())).is_err())));
        }
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn maximum_export_profile_is_detached_from_stamps_and_reused_on_disk() {
        use layer_core::color::ColorProfile;
        let cancel=AtomicBool::new(false);let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        let mut recipe=ExportRecipe::web_share();let profile=layer_core::authored::Resource::new(Arc::<[u8]>::from(vec![7;layer_core::color::source::MAX_PROFILE_BYTES]));
        let id=profile.id();recipe.profile.profile=ColorProfile::Icc(profile);
        source.files.last_export=Some(ExportRepeat {recipe,location:DocumentLocation {uri:"private:export.png".into(),name:"Export.png".into()}});
        let capture=source.capture_session().unwrap();assert!(serde_json::to_vec(&source.session_stamp()).unwrap().len()<4096);
        assert!(serde_json::to_vec(&capture.metadata().unwrap().value).unwrap().len()<4096);
        let prepared=capture.prepare(&cancel).unwrap();assert!(prepared.metadata().len()<8192);
        let root=std::env::temp_dir().join(format!("capy-export-profile-{id}"));let mut store=layer_core::package::session_store::SessionStore::open(&root).unwrap();
        store.commit(&prepared,&cancel).unwrap();let count=std::fs::read_dir(root.join("resources")).unwrap().count();assert_eq!(count,1);
        source.state.camera.zoom_to(1.25).unwrap();store.commit(&source.capture_session().unwrap().prepare(&cancel).unwrap(),&cancel).unwrap();
        assert_eq!(std::fs::read_dir(root.join("resources")).unwrap().count(),count);
        let restored=SessionRestore::from_core(store.load(ProjectLimits::default(),&cancel).unwrap().unwrap()).unwrap();
        let ColorProfile::Icc(bytes)=&restored.last_export.as_ref().unwrap().recipe.profile.profile else {panic!("missing ICC")};assert_eq!(bytes.id(),id);assert_eq!(bytes.len(),layer_core::color::source::MAX_PROFILE_BYTES);
        drop(restored);drop(store);drop(source);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pending_manual_save_can_checkpoint_history_without_acknowledging_the_save() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        layer(&mut source,LayerAction::New {group:false,clipped:false});source.frame(1,1).unwrap();save(&mut source,true);
        let saved=source.files.saved_checkpoint;
        layer(&mut source,LayerAction::New {group:false,clipped:false});source.frame(2,2).unwrap();
        invoke(&mut source,CommandId::SaveDocument);let request=source.state.requests.last().unwrap().id;
        source.capture_project_save(request,source.state.document_file.location.clone().unwrap()).unwrap();
        assert!(source.state.document_file.busy);assert!(!source.recovery_document().busy);
        let mut expected=source.capture_session().unwrap().artwork().artwork.as_ref().clone();
        expected.outputs.get_mut(expected.default_output).unwrap().context.elapsed=0.;
        let mut restored=restore(&encode(&source),false);assert!(!restored.state.document_file.busy);
        assert_eq!(restored.engine.document().artwork,expected);assert_eq!(restored.files.saved_checkpoint,saved);
        assert!(restored.state.document_file.modified);invoke(&mut restored,CommandId::Undo);restored.frame(3,3).unwrap();
        assert_eq!(restored.engine.checkpoint(),saved);assert!(!restored.state.document_file.modified);
        let capture=source.capture_session().unwrap();assert!(capture.prepare(&AtomicBool::new(true)).is_err());
        assert!(!source.recovery_document().busy);assert_eq!(source.files.saved_checkpoint,saved);
        source.sdr_gesture=Some(Default::default());assert!(source.recovery_document().busy);assert!(source.capture_session().is_err());source.sdr_gesture=None;
        source.complete_document_request(request,Err("private checkpoint failed".into())).unwrap();
        assert_eq!(source.files.saved_checkpoint,saved);assert!(source.state.document_file.modified);assert!(!source.recovery_document().busy);
        invoke(&mut source,CommandId::ExportDocument);assert!(source.state.document_file.busy);assert!(source.recovery_document().busy);
    }
    #[test]
    fn checkpoints_wait_until_a_raster_edit_is_submitted() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        crate::session::test_support::select(&mut source,crate::session::test_support::rectangle([4.,4.,20.,20.]));
        assert!(!source.recovery_document().busy);
        source.dispatch(UiAction::Layer {action:LayerAction::FillSelection}).unwrap();
        assert!(source.engine.raster_edit_pending());
        assert!(source.recovery_document().busy);assert!(source.capture_session().is_err());
        source.frame(1,1).unwrap();
        assert!(!source.recovery_document().busy);assert!(source.capture_session().is_ok());
    }
    #[test]
    fn session_history_preserves_manual_save_checkpoint_and_recovered_label_until_success() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(1,1).unwrap();save(&mut source,true);
        let saved=source.engine.checkpoint();
        layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(2,2).unwrap();
        let bytes=encode(&source);
        let ordinary=restore(&bytes,false);assert!(!ordinary.state.document_file.recovered);assert!(ordinary.state.document_file.modified);
        let mut recovered=restore(&bytes,true);assert!(recovered.state.document_file.recovered);assert_eq!(recovered.files.saved_checkpoint,saved);
        assert_eq!(recovered.state.document_file.location,source.state.document_file.location);
        assert_eq!(recovered.engine.document().working,source.engine.document().working);
        invoke(&mut recovered,CommandId::Undo);recovered.frame(3,3).unwrap();assert!(!recovered.state.document_file.modified);
        invoke(&mut recovered,CommandId::Redo);recovered.frame(4,4).unwrap();assert!(recovered.state.document_file.modified);
        save(&mut recovered,false);assert!(recovered.state.document_file.recovered);
        save(&mut recovered,true);assert!(!recovered.state.document_file.recovered);assert!(!recovered.state.document_file.modified);
        let saved=encode(&recovered);
        let ordinary=restore(&saved,false);assert!(!ordinary.state.document_file.modified);assert!(!ordinary.state.document_file.recovered);
        let mut clean=restore(&saved,true);assert!(!clean.state.document_file.modified);assert!(clean.state.document_file.recovered);
        assert!(clean.state.commands.iter().any(|command|command.id==CommandId::SaveDocument && command.enabled));
        save(&mut clean,true);assert!(!clean.state.document_file.modified);assert!(!clean.state.document_file.recovered);
    }
    /// Undo then redo `session`'s last edit, checking each step rebuilds the
    /// renderer and moves the camera by the document's change of origin.
    fn navigate_rebuilding(session: &mut UiSession<Recorder>, sizes: [[u32; 2]; 2], shift: [i64; 2]) {
        for (step, (command, size)) in [(CommandId::Undo, sizes[0]), (CommandId::Redo, sizes[1])].into_iter().enumerate() {
            let camera = session.state.camera.clone();
            session.renderer_mut().rebuilds = 0;
            invoke(session, command);
            session.frame(10 + step as u64, 10 + step as u64).unwrap();
            assert_eq!(session.engine.document().composition().size, size);
            assert!(session.renderer_mut().rebuilds > 0, "{command:?} rebuilds the canvas");
            let mut followed = camera.clone();
            let sign = if command == CommandId::Undo { -1 } else { 1 };
            followed.follow_document_origin(shift.map(|v| (sign * v) as f32));
            assert_eq!(session.state.camera.translation, followed.translation, "{command:?} keeps the artwork in place on screen");
        }
    }
    #[test]
    fn crops_on_every_edge_and_storage_rebases_rebuild_on_undo_and_redo_after_recovery() {
        use layer_core::{CanvasGeometry, CanvasRect};
        let crops = [([0, 0], [900, 1000]), ([0, 0], [1000, 850]), ([70, 0], [930, 1000]), ([0, 45], [1000, 955])];
        for (origin, size) in crops {
            let mut source = session(Platform::Gtk); source.frame(0, 0).unwrap();
            source.apply_canvas_geometry(&CanvasGeometry::crop(CanvasRect { origin, size }), Vec::new()).unwrap();
            source.frame(1, 1).unwrap();
            let shift = origin.map(i64::from);
            for mut session in [restore(&encode(&source), true), source] {
                navigate_rebuilding(&mut session, [[1000; 2], size], shift);
            }
        }
        let mut source = session(Platform::Gtk); source.frame(0, 0).unwrap();
        let target = source.engine.document().working.target.unwrap();
        let mut moved = source.engine.document().clone();
        let edit = moved.translate_target_edit(target, [300, 0]).unwrap();
        moved.apply(edit.clone()).unwrap();
        let mut edits = vec![edit];
        edits.extend(moved.paint_extent_plan(&[target], source.engine.geometry_limits()).unwrap());
        source.layer_edit(layer_core::Edit::Batch(edits)).unwrap(); source.frame(1, 1).unwrap();
        assert_ne!(source.engine.document().target_extent(target), [1000; 2], "the paint domain grows by whole tiles");
        assert_eq!(source.engine.document().working.view_origin, [0, 0]);
        for mut session in [restore(&encode(&source), true), source] {
            navigate_rebuilding(&mut session, [[1000; 2]; 2], [0, 0]);
        }
    }
    #[test]
    fn restored_camera_centers_document_point_for_new_viewport_and_rejects_unknown_state() {
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();
        source.state.camera.zoom_to(2.).unwrap();source.state.camera.rotation=0.7;source.state.camera.flipped=[true,false];source.state.camera.center_on([122.,345.]);
        let expected=source.session_stamp().state.camera;
        let restored=restore(&encode(&source),false);let actual=restored.session_stamp().state.camera;
        assert!((expected.center[0]-actual.center[0]).abs()<0.0001);assert!((expected.center[1]-actual.center[1]).abs()<0.0001);
        assert_eq!(expected.zoom,actual.zoom);assert_eq!(expected.rotation,actual.rotation);assert_eq!(expected.flipped,actual.flipped);
        let mut resized=restored;let mut expected_resize=resized.state.camera.clone();expected_resize.resize([1440,1080]);
        resized.set_viewport([720.,540.],[1440,1080]).unwrap();resized.frame(2,2).unwrap();
        let after=resized.session_stamp().state.camera;
        assert_eq!(after.zoom,expected.zoom);assert_eq!(after.rotation,expected.rotation);assert_eq!(after.flipped,expected.flipped);
        assert_eq!(resized.state.camera.translation,expected_resize.translation);
        let mut state=serde_json::to_value(source.session_document_state()).unwrap();state["new_unclassified_field"]=true.into();
        assert!(serde_json::from_value::<SessionDocumentState>(state).is_err());
        let mut camera=expected;camera.center[0]=f32::MAX;assert!(camera.restore(&source.state.camera).is_err());
        let mut capture=source.capture_session().unwrap();capture.state.camera=camera;
        assert!(capture.prepare(&AtomicBool::new(false)).is_err());
    }
    #[test]
    fn quit_does_not_close_drawing_and_explicit_close_still_requires_decision() {
        let mut session=session(Platform::Gtk);layer(&mut session,LayerAction::New{group:false,clipped:false});session.frame(0,0).unwrap();
        session.request_session_close().unwrap();assert!(session.state.document_file.close_ready);assert!(session.state.document_file.modified);assert!(session.state.requests.is_empty());
        session.reset_document_close();assert!(!session.state.document_file.close_ready);
        session.request_document_close().unwrap();assert!(session.state.requests.iter().any(|request|matches!(request.kind,HostRequestKind::Document{request:DocumentRequest::ConfirmClose{..}})));
    }
    #[test]
    fn membership_changes_are_explicit_and_identities_cannot_be_rebound() {
        let original=manifest();assert_eq!(original.reconcile(original.drawings.clone(),original.active,false).unwrap(),original);
        assert!(original.reconcile(vec![original.drawings[1].clone()],2,false).is_err());
        let mut rebound=original.drawings.clone();rebound[0].key="different".into();assert!(original.reconcile(rebound,2,false).is_err());
        let removed=original.remove(7).unwrap();assert_eq!(removed.drawings,vec![original.drawings[1].clone()]);assert!(removed.generation>original.generation);
        assert_eq!(removed.remove(7).unwrap(),removed);assert!(removed.remove(0).is_err());
        assert!(SessionManifest{generation:u64::MAX,..original.clone()}.remove(7).is_err());
        let exhausted=SessionManifest{generation:MAX_SESSION_DRAWING_ID,..original.clone()};
        assert!(exhausted.validate().is_ok());assert!(exhausted.remove(7).is_err());
        assert!(SessionManifest{generation:MAX_SESSION_DRAWING_ID+1,..original.clone()}.validate().is_err());
        let mut invalid=original;invalid.drawings[0].id=u64::MAX;assert!(invalid.validate().is_err());
        invalid.drawings[0].id=7;invalid.drawings[0].key="../other-session".into();assert!(invalid.validate().is_err());
    }
    #[test]
    fn durable_close_selects_the_same_neighbor_as_live_tabs() {
        let mut tabs=crate::document_tabs::DocumentTabs::default();tabs.add();tabs.add();
        for active in [1,2,3] {
            for closed in [1,2,3] {
                let mut live=tabs.clone();assert!(live.select(active));
                let saved=SessionManifest{drawings:live.order().iter().map(|&id|SessionDrawing{id,key:format!("drawing-{id}")}).collect(),active,..Default::default()};
                assert!(live.close(closed));let restored=saved.remove(closed).unwrap();
                assert_eq!(restored.active,live.selected());
                assert_eq!(restored.drawings.iter().map(|drawing|drawing.id).collect::<Vec<_>>(),live.order());
            }
        }
    }
    #[test]
    fn unfinished_restores_are_quarantined_and_late_completion_cannot_remove_a_new_attempt() {
        let original=manifest();let pending=original.begin_restore(7).unwrap();
        let interrupted=SessionManifest::parse(&serde_json::to_vec(&pending).unwrap()).unwrap().recover_interrupted().unwrap();
        assert_eq!(interrupted.blocked,vec![7]);assert!(interrupted.begin_restore(7).is_err());
        let next=interrupted.begin_restore(2).unwrap();assert!(next.finish_restore(attempt(&pending,7),true).is_err());
        let failed=next.finish_restore(attempt(&next,2),false).unwrap();assert_eq!(failed.blocked,vec![7,2]);assert_eq!(failed.drawings,original.drawings);
        let retried=failed.retry_restore(7).unwrap();assert!(retried.finish_restore(attempt(&pending,7),true).is_err());
        let retried=retried.finish_restore(attempt(&retried,7),true).unwrap();assert_eq!(retried.blocked,vec![2]);
        assert_eq!(retried.remove(2).unwrap().blocked,Vec::<u64>::new());
    }
    #[test]
    fn batch_restore_remains_pending_until_each_owner_adopts() {
        let pending=manifest().begin_restore(7).unwrap().begin_restore(2).unwrap();
        assert!(pending.begin_restore(7).is_err());
        let interrupted=pending.recover_interrupted().unwrap();
        assert_eq!(interrupted.blocked,vec![7,2]);assert!(interrupted.restoring.is_empty());
        let adopted=pending.finish_restore(attempt(&pending,2),true).unwrap().recover_interrupted().unwrap();
        assert_eq!(adopted.blocked,vec![7]);
        assert_eq!(adopted.drawings,pending.drawings);
    }
    #[test]
    fn staging_preserves_unresolved_drawings_and_rejects_identity_reuse() {
        let original=manifest().begin_restore(7).unwrap();
        let added=SessionDrawing{id:3,key:"drawing-c".into()};
        let staged=original.stage(vec![original.drawings[1].clone(),added.clone()],3).unwrap();
        assert_eq!(staged.drawings,vec![original.drawings[0].clone(),original.drawings[1].clone(),added.clone()]);
        assert_eq!(staged.restoring,original.restoring);assert!(!staged.clean_exit);
        assert_eq!(staged.stage(vec![added.clone()],3).unwrap(),staged);
        assert!(staged.stage(vec![added.clone(),added],3).is_err());
        assert!(staged.stage(vec![SessionDrawing{id:7,key:"different".into()}],7).is_err());
    }
    #[test]
    fn restored_identity_remapping_preserves_keys_and_pending_failures() {
        let pending=manifest().begin_restore(7).unwrap().begin_restore(2).unwrap();
        let original=pending.finish_restore(attempt(&pending,2),false).unwrap();
        let remapped=original.remap(&[(7,2),(2,7)]).unwrap();
        assert_eq!(remapped.drawings[0].id,2);assert_eq!(remapped.drawings[0].key,original.drawings[0].key);
        assert_eq!(remapped.active,7);assert_eq!(remapped.blocked,vec![7]);assert_eq!(remapped.restoring,vec![SessionRestoreAttempt{id:2,generation:attempt(&original,7).generation}]);
        assert!(original.remap(&[(7,2)]).is_err());assert!(original.remap(&[(7,9),(2,9)]).is_err());
        assert!(original.remap(&[(7,9),(7,10)]).is_err());assert!(original.remap(&[(12,3)]).is_err());
        assert_eq!(original.remap(&[(7,7)]).unwrap(),original);
    }
    #[test]
    fn startup_stamp_distinguishes_new_empty_documents_without_replacement_policy() {
        let mut original=session(Platform::Gtk);original.frame(0,0).unwrap();original.set_document_replacement(false);
        let stamp=original.session_stamp();assert!(original.can_replace_startup_session(&stamp));
        let mut another=session(Platform::Gtk);another.frame(0,0).unwrap();assert!(!another.can_replace_startup_session(&stamp));
        assert!(!stamp.same_editor(&another.session_stamp()));
    }
    #[test]
    fn startup_replacement_ignores_pending_gpu_but_waits_for_live_pointer() {
        let mut original=session(Platform::Gtk);let stamp=original.session_stamp();
        assert!(original.engine.has_pending_document_edits());assert!(!original.can_park_document());
        assert!(original.can_replace_startup_session(&stamp));original.frame(0,0).unwrap();
        let stamp=original.session_stamp();
        original.input(UiInput::Pointer {id:1,phase:ContactPhase::Down,kind:PointerKind::Mouse,button:PointerButton::Pan,position:[200.,200.],time_ns:1}).unwrap();
        assert!(!original.state.document_file.modified);assert!(!original.can_replace_startup_session(&stamp));
        original.input(UiInput::Pointer {id:1,phase:ContactPhase::Cancel,kind:PointerKind::Mouse,button:PointerButton::Pan,position:[200.,200.],time_ns:2}).unwrap();
        assert!(original.can_replace_startup_session(&stamp));
    }
    #[test]
    fn startup_allows_automatic_first_layout_fit_but_preserves_manual_navigation() {
        let mut original=session(Platform::Gtk);let stamp=original.session_stamp();assert!(original.initial_fit);
        original.set_viewport([900.,700.],[1800,1400]).unwrap();original.frame(0,0).unwrap();
        assert!(!original.initial_fit);assert!(original.can_replace_startup_session(&stamp));
        original.set_viewport([840.,640.],[1680,1280]).unwrap();original.frame(1,1).unwrap();assert!(original.can_replace_startup_session(&stamp));
        original.gesture([100.,100.],[120.,105.],1.2,0.1).unwrap();
        assert!(!original.can_replace_startup_session(&stamp));
    }
    #[test]
    fn restored_camera_keeps_its_document_center_when_adopted_into_window_layout() {
        let mut source=session(Platform::Gtk);source.set_viewport([1200.,900.],[2400,1800]).unwrap();source.frame(0,0).unwrap();
        source.state.camera.zoom_to(0.37).unwrap();source.state.camera.rotation=0.2;source.state.camera.center_on([123.,456.]);
        let expected=source.session_stamp().state.camera;
        let mut restored=restore(&encode(&source),false);
        let mut window=session(Platform::Gtk);window.set_viewport([900.,700.],[1800,1400]).unwrap();window.frame(0,0).unwrap();
        restored.inherit_window_state(&window).unwrap();restored.set_viewport([900.,700.],[1800,1400]).unwrap();
        let actual=restored.session_stamp().state.camera;
        assert_eq!(actual.zoom,expected.zoom);assert_eq!(actual.rotation,expected.rotation);
        assert!((actual.center[0]-expected.center[0]).abs()<0.001);assert!((actual.center[1]-expected.center[1]).abs()<0.001);
    }
    #[test]
    fn impossible_saved_checkpoint_is_rejected_before_adopting_document() {
        let mut original=session(Platform::Gtk);original.frame(0,0).unwrap();
        let before=original.session_stamp();
        let mut state=original.session_document_state();state.saved_checkpoint=u64::MAX;
        let candidate=SessionRestore{editor:layer_core::Editor::new(original.engine.document().clone()),state,last_export:None};
        assert!(original.restore_session(candidate,true,None).is_err());assert_eq!(original.session_stamp(),before);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn first_checkpoint_is_indexed_before_any_durable_artwork_can_be_written() {
        use layer_core::package::session_store::{SessionStore,collect_unreferenced_stores};
        let root=std::env::temp_dir().join(format!("capy-first-checkpoint-{}",layer_core::PortableId::random()));std::fs::create_dir(&root).unwrap();
        let cancel=AtomicBool::new(false);let path=root.join("session.json");
        let staged=SessionManifest::default().stage(vec![SessionDrawing{id:1,key:"drawing-a".into()}],1).unwrap();staged.publish(&path).unwrap();
        assert_eq!(SessionManifest::read(&path).unwrap().unwrap(),staged);
        let mut store=SessionStore::open(&root.join("drawing-a")).unwrap();assert!(store.load(ProjectLimits::default(),&cancel).unwrap().is_none());
        let mut source=session(Platform::Gtk);source.frame(0,0).unwrap();layer(&mut source,LayerAction::New{group:false,clipped:false});source.frame(1,1).unwrap();
        store.commit(&source.capture_session().unwrap().prepare(&cancel).unwrap(),&cancel).unwrap();drop(store);
        let surviving=SessionManifest::read(&path).unwrap().unwrap();let reachable=surviving.drawings.iter().map(|drawing|drawing.key.clone()).collect();
        collect_unreferenced_stores(&root,&reachable,&cancel).unwrap();
        let mut reopened=SessionStore::open(&root.join(&surviving.drawings[0].key)).unwrap();let copy=SessionRestore::from_core(reopened.load(ProjectLimits::default(),&cancel).unwrap().unwrap()).unwrap();
        assert_eq!(copy.document().artwork,source.engine.document().artwork);assert!(copy.editor.can_undo());drop(copy);drop(reopened);drop(source);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn durable_membership_rejects_stale_publication_and_preserves_unreadable_index() {
        let root=std::env::temp_dir().join(format!("capy-session-ui-{}",layer_core::PortableId::random()));std::fs::create_dir(&root).unwrap();let path=root.join("session.json");
        let original=manifest();original.publish(&path).unwrap();let closed=original.remove(7).unwrap();closed.publish(&path).unwrap();
        assert!(!original.publish_checked(&path).unwrap_err().published);assert_eq!(SessionManifest::read(&path).unwrap().unwrap(),closed);
        closed.publish_checked(&path).unwrap();
        std::fs::write(&path,b"incomplete session").unwrap();assert!(closed.publish(&path).is_err());assert_eq!(std::fs::read(&path).unwrap(),b"incomplete session");
        std::fs::remove_dir_all(root).unwrap();
    }
}
