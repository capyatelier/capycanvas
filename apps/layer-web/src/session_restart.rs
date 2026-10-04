use super::*;
use layer_core::{package::{session_transfer::{PreparedSessionTransfer,SessionTransferDescriptor,SessionTransferReceiver},session::{EditorCapture,SessionMetadata},ByteRange,ByteSource,ImmutableBacking,RangeState,MAX_RANGE_BYTES},ProjectLimits};
use std::{collections::{BTreeMap,BTreeSet},sync::{Arc,atomic::AtomicBool}};
use wasm_bindgen_futures::{future_to_promise,JsFuture};

const BLOCK:usize=4*1024*1024;
#[derive(Serialize,Deserialize)]
struct Envelope {descriptor:SessionTransferDescriptor,chunks:Vec<usize>}

async fn wait_capture(capture:&EditorCapture)->Result<(),JsValue> {
    let start=js_sys::Date::now();
    loop {
        if capture.retained_tiles().try_blobs().map_err(js)?.is_some(){return Ok(());}
        if js_sys::Date::now()-start>30_000. {return Err(js("Drawing checkpoint backing timed out"));}
        documents::yield_browser().await?;
    }
}
async fn pack(capture:EditorCapture,metadata:SessionMetadata)->Result<JsValue,JsValue> {
    wait_capture(&capture).await?;
    let prepared=PreparedSessionTransfer::capture(&capture,metadata,&AtomicBool::new(false)).map_err(js)?;
    let indices=(0..prepared.payload_count()).collect::<Vec<_>>();
    pack_prepared(&prepared,&indices).await
}
async fn pack_prepared(prepared:&PreparedSessionTransfer,indices:&[usize])->Result<JsValue,JsValue> {
    let buffers=js_sys::Array::new();let mut chunks=vec![0;prepared.payload_count()];
    for &index in indices {
        let length=usize::try_from(prepared.payload_len(index).map_err(js)?).map_err(js)?;
        for offset in (0..length).step_by(BLOCK) {
            let bytes=prepared.read_chunk(index,offset as u64,(length-offset).min(BLOCK)).map_err(js)?;
            buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));chunks[index]+=1;
            documents::yield_browser().await?;
        }
    }
    let result=js_sys::Object::new();
    js_sys::Reflect::set(&result,&js("metadata"),&js(serde_json::to_string(&Envelope{descriptor:prepared.descriptor().clone(),chunks}).map_err(js)?))?;
    js_sys::Reflect::set(&result,&js("buffers"),&buffers)?;Ok(result.into())
}
async fn unpack(metadata:&str,buffers:js_sys::Array)->Result<layer_core::package::session::OpenSession,JsValue> {
    let envelope:Envelope=serde_json::from_str(metadata).map_err(js)?;
    let mut receiver=SessionTransferReceiver::new(envelope.descriptor,ProjectLimits::default()).map_err(js)?;
    let count=envelope.chunks.iter().try_fold(0usize,|sum,count|sum.checked_add(*count)).ok_or_else(||js("Session transfer count overflow"))?;
    if count!=buffers.length() as usize{return Err(js("Incomplete session transfer"));}
    let mut index=0;let mut total=0usize;
    for (payload,count) in envelope.chunks.into_iter().enumerate() {
        for _ in 0..count {
            let bytes=buffers.get(index).dyn_into::<js_sys::Uint8Array>().map_err(|_|js("Missing session resource block"))?;
            if bytes.length() as usize>MAX_RANGE_BYTES{return Err(js("Oversized session resource block"));}
            total=total.checked_add(bytes.length() as usize).filter(|n|*n<=1024*1024*1024).ok_or_else(||js("Session transfer exceeds admission"))?;
            receiver.push_chunk(payload,&bytes.to_vec()).map_err(js)?;buffers.set(index,JsValue::UNDEFINED);index+=1;
            documents::yield_browser().await?;
        }
    }
    receiver.finish().map_err(js)?.adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).map_err(js)
}
fn parts(value:&JsValue)->Result<(String,js_sys::Array),JsValue> {
    Ok((js_sys::Reflect::get(value,&js("metadata"))?.as_string().ok_or_else(||js("Missing session transfer metadata"))?,js_sys::Reflect::get(value,&js("buffers"))?.dyn_into()?))
}

#[wasm_bindgen]
pub struct WebSessionCapture(Option<layer_ui::SessionCapture>);
#[wasm_bindgen]
impl WebSessionCapture {
    pub fn write(&mut self,key:String,generation:u64,base_generation:u64,existing:JsValue,handles:JsValue)->Result<js_sys::Promise,JsValue> {
        let capture=self.0.take().ok_or_else(||js("Session checkpoint was already written"))?;
        let existing:Vec<String>=serde_wasm_bindgen::from_value(existing).map_err(js)?;
        Ok(future_to_promise(async move {
            wait_capture(&capture.editor).await?;
            let prepared=PreparedSessionTransfer::capture(&capture.editor,capture.metadata().map_err(js)?,&AtomicBool::new(false)).map_err(js)?;
            let envelope=serde_json::to_string(&Envelope{descriptor:prepared.descriptor().clone(),chunks:Vec::new()}).map_err(js)?;
            let beginning=serde_json::to_string(&serde_json::json!({"key":key,"project":envelope})).map_err(js)?;
            let missing=JsFuture::from(raster_worker::call("restart-begin",&beginning,&js_sys::Array::new())?).await?;
            let missing:Vec<usize>=serde_wasm_bindgen::from_value(missing).map_err(js)?;
            let wire=pack_prepared(&prepared,&missing).await?;
            let (project,buffers)=parts(&wire)?;
            let metadata=serde_json::to_string(&serde_json::json!({"key":key,"generation":generation,"base_generation":base_generation,"existing":existing,"project":project})).map_err(js)?;
            let result=js_sys::Object::new();
            js_sys::Reflect::set(&result,&js("operation"),&js("restart-write"))?;
            js_sys::Reflect::set(&result,&js("metadata"),&js(metadata))?;
            js_sys::Reflect::set(&result,&js("buffers"),&buffers)?;
            js_sys::Reflect::set(&result,&js("handles"),&handles)?;
            JsFuture::from(raster_worker::request(&result)?).await
        }))
    }
}
#[wasm_bindgen]
pub struct WebSessionProject {session:Option<Box<UiSession<AttachedRenderer>>>,lost:Arc<std::sync::Mutex<Option<String>>>}

#[derive(Serialize)]
struct SessionAdoption {id:u64,change:layer_ui::UiChange}

#[wasm_bindgen]
impl WebApp {
    pub fn reserve_session_ids(&mut self,ids:JsValue)->Result<(),JsValue> {
        self.documents.reserve_identities(&serde_wasm_bindgen::from_value::<Vec<u64>>(ids).map_err(js)?).map_err(js)
    }
    pub fn capture_tab_session(&self,id:u64)->Result<WebSessionCapture,JsValue> {
        Ok(WebSessionCapture(Some(self.document_session(id)?.capture_session().map_err(js)?)))
    }
    pub fn prepare_session_restart(&self,checkpoint:JsValue,buffers:js_sys::Array,recovered:bool,observe:js_sys::Function)->Result<js_sys::Promise,JsValue> {
        let metadata=js_sys::JSON::stringify(&checkpoint)?.as_string().ok_or_else(||js("Missing drawing checkpoint"))?;
        let renderer=self.session.engine().backend().0.as_ref().ok_or_else(||js("Wait for the canvas"))?;
        let (adapter,device,queue)=(renderer.adapter().clone(),renderer.device().clone(),renderer.queue().clone());
        let lost=self.gpu_owner().ok_or_else(||js("Wait for the canvas"))?;
        let viewport=self.session.state().camera.viewport;let localization=self.session.localization().clone();
        let brush=self.session.engine().configured_brush().clone();
        let admission=self.documents.admission(&self.session.retained_document_tiles());
        Ok(future_to_promise(async move {
            let wire=JsFuture::from(raster_worker::call("restart-open",&metadata,&buffers)?).await?;
            let (metadata,buffers)=parts(&wire)?;
            let restore=layer_ui::SessionRestore::from_core(unpack(&metadata,buffers).await?).map_err(js)?;
            let observed=observe.call1(&JsValue::NULL,&serialize(&restore.state.location)?)?;
            let observed:Option<layer_ui::DestinationFingerprint>=serde_wasm_bindgen::from_value(JsFuture::from(js_sys::Promise::resolve(&observed)).await?).map_err(js)?;
            admission.admit(restore.document()).map_err(|reason|js(reason.message(&localization)))?;
            let mut candidate=documents::prepare_session(restore.document().clone(),adapter,device,queue,&brush,viewport,localization,None).await?;
            candidate.restore_session(restore,recovered,observed).map_err(js)?;
            Ok(WebSessionProject{session:Some(Box::new(candidate)),lost}.into())
        }))
    }
    pub fn adopt_session_restart(&mut self,mut project:WebSessionProject,initial:JsValue,identity:u64,activate:bool)->Result<JsValue,JsValue> {
        let lost=self.gpu_owner().ok_or_else(||js("Canvas unavailable"))?;
        if !Arc::ptr_eq(&lost,&project.lost)||lost.lock().unwrap().is_some(){return Err(js("The canvas changed while restoring the drawing"));}
        self.documents.reserve_identities(&[identity]).map_err(js)?;
        let initial=serde_wasm_bindgen::from_value(initial).map_err(js)?;
        let replace=self.documents.order().len()==1&&self.session.can_replace_startup_session(&initial);
        let mut candidate=*project.session.take().ok_or_else(||js("Drawing checkpoint was already adopted"))?;
        candidate.set_document_replacement(false);candidate.inherit_window_state(&self.session).map_err(js)?;
        let config=&self.surface.as_ref().ok_or_else(||js("Canvas unavailable"))?.config;
        candidate.renderer_mut().resize_surface(config.width,config.height).map_err(js)?;
        if !activate || !replace {
            let tiles=candidate.park_document().map_err(js)?;candidate.renderer_mut().0.take();
            let id=if self.documents.order().contains(&identity){self.documents.append_parked(candidate,tiles,self.session.localization())}else{
                self.documents.append_parked_with_id(identity,candidate,tiles,self.session.localization()).map_err(|(reason,_)|js(reason))?;identity
            };
            return serialize(&SessionAdoption{id,change:layer_ui::UiChange{revision:self.session.state().revision,regions:layer_ui::regions::DOCUMENT,canvas_wake:false}});
        }
        self.session.park_document().map_err(js)?;
        self.session.renderer_mut().0.take();
        self.session=candidate;
        self.documents.restore_identity(identity).map_err(js)?;
        if let Some(control)=self.tone.pending.take(){control.cancel();}
        self.tone=Default::default();self.proof=Default::default();self.reset_document_views();
        serialize(&SessionAdoption{id:identity,change:layer_ui::UiChange{revision:self.session.state().revision,regions:layer_ui::regions::ALL,canvas_wake:true}})
    }
    pub fn restore_session_order(&mut self,order:JsValue)->Result<JsValue,JsValue> {
        let order:Vec<u64>=serde_wasm_bindgen::from_value(order).map_err(js)?;
        self.documents.restore_order(&order,self.documents.selected()).map_err(js)?;
        self.document_changed()
    }
}

struct CachedTransfer {prepared:PreparedSessionTransfer,bytes:u64}
thread_local! {
    static TRANSFERS:std::cell::RefCell<BTreeMap<String,CachedTransfer>>=const {std::cell::RefCell::new(BTreeMap::new())};
    static PENDING_TRANSFER:std::cell::RefCell<BTreeMap<String,SessionTransferReceiver>>=const {std::cell::RefCell::new(BTreeMap::new())};
}
fn trim_transfer_cache(current:&str) {
    TRANSFERS.with(|slot|{
        let mut cache=slot.borrow_mut();
        let budget=layer_ui::DocumentBudget::default().inactive_ram as u64;
        loop {
            let bytes=cache.values().map(|transfer|transfer.bytes).fold(0u64,u64::saturating_add);
            if bytes<=budget {break;}
            let key=cache.keys().find(|key|key.as_str()!=current).cloned().or_else(||cache.keys().next().cloned());
            let Some(key)=key else {break;};cache.remove(&key);
        }
    });
}
#[wasm_bindgen]
pub fn raster_worker_restart_begin(metadata:&str)->Result<JsValue,JsValue> {
    #[derive(Deserialize)]struct Begin {key:String,project:String}
    let begin:Begin=serde_json::from_str(metadata).map_err(js)?;
    let envelope:Envelope=serde_json::from_str(&begin.project).map_err(js)?;
    let receiver=TRANSFERS.with(|cache|{
        let cache=cache.borrow();
        match cache.get(&begin.key) {
            Some(previous)=>SessionTransferReceiver::new_reusing(envelope.descriptor,ProjectLimits::default(),&previous.prepared),
            None=>SessionTransferReceiver::new(envelope.descriptor,ProjectLimits::default()),
        }
    }).map_err(js)?;
    let missing=receiver.missing_payloads();
    PENDING_TRANSFER.with(|slot|{let mut pending=slot.borrow_mut();pending.clear();pending.insert(begin.key,receiver);});
    serialize(&missing)
}
#[wasm_bindgen]
pub fn raster_worker_restart_retain(metadata:&str)->Result<(),JsValue> {
    let retained:BTreeSet<String>=serde_json::from_str(metadata).map_err(js)?;
    TRANSFERS.with(|cache|cache.borrow_mut().retain(|key,_|retained.contains(key)));Ok(())
}

#[derive(Deserialize)]
struct WriteOptions {key:String,generation:u64,base_generation:u64,existing:Vec<String>,project:String}
#[wasm_bindgen]
pub async fn raster_worker_restart_write(metadata:&str,buffers:js_sys::Array)->Result<JsValue,JsValue> {
    let options:WriteOptions=serde_json::from_str(metadata).map_err(js)?;
    let envelope:Envelope=serde_json::from_str(&options.project).map_err(js)?;
    let mut receiver=PENDING_TRANSFER.with(|slot|slot.borrow_mut().remove(&options.key)).ok_or_else(||js("Checkpoint transfer expired"))?;
    let mut index=0;
    for (payload,count) in envelope.chunks.into_iter().enumerate() {
        for _ in 0..count {
            let bytes=buffers.get(index).dyn_into::<js_sys::Uint8Array>().map_err(|_|js("Missing checkpoint resource block"))?;
            if bytes.length() as usize>BLOCK{return Err(js("Oversized checkpoint resource block"));}
            receiver.push_chunk(payload,&bytes.to_vec()).map_err(js)?;buffers.set(index,JsValue::UNDEFINED);index+=1;
            documents::yield_browser().await?;
        }
    }
    if index!=buffers.length(){return Err(js("Trailing checkpoint resource blocks"));}
    let transfer=receiver.finish().map_err(js)?;
    let opened=transfer.adopt_verified(ProjectLimits::default(),&AtomicBool::new(false)).map_err(js)?;
    layer_ui::SessionRestore::validate_core(&opened).map_err(js)?;
    let metadata_bytes=(options.project.len() as u64).saturating_mul(2);
    let bytes=(0..transfer.payload_count()).map(|index|transfer.payload_len(index).unwrap_or(u64::MAX)).fold(metadata_bytes,u64::saturating_add);
    TRANSFERS.with(|cache|{cache.borrow_mut().insert(options.key.clone(),CachedTransfer{prepared:transfer,bytes});});
    trim_transfer_cache(&options.key);
    let capture=opened.editor.capture_session(opened.editor.capture(1,Default::default()).map_err(js)?).map_err(js)?;
    let prepared=layer_core::package::session::PreparedSession::prepare(&capture,opened.metadata,&AtomicBool::new(false)).map_err(js)?;
    let mut ids=Vec::new();let mut resources=Vec::new();let result_buffers=js_sys::Array::new();
    let existing:BTreeSet<_>=options.existing.into_iter().collect();
    for entry in &prepared.resources().entries {
        let id=entry.payload.id().to_string();
        for offset in (0..entry.bytes).step_by(BLOCK) {
            let part=format!("{id}-{}",offset/BLOCK as u64);resources.push(part.clone());
            if existing.contains(&part){continue;}
            let size=(entry.bytes-offset).min(BLOCK as u64) as usize;
            let bytes=match &entry.payload {
                layer_core::package::resources::Payload::Opaque(resource)=>resource.read_chunk(offset,size,&AtomicBool::new(false)).map_err(js)?.to_vec(),
                payload=>payload.encoded().map_err(js)?.bytes[offset as usize..offset as usize+size].to_vec(),
            };
            ids.push(part);result_buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            documents::yield_browser().await?;
        }
    }
    let checkpoint=serde_json::json!({"generation":options.generation,"resources":resources,"metadata":String::from_utf8(prepared.metadata().to_vec()).map_err(js)?});
    let result=js_sys::Object::new();
    js_sys::Reflect::set(&result,&js("metadata"),&js(serde_json::to_string(&serde_json::json!({"key":options.key,"checkpoint":checkpoint,"ids":ids,"base_generation":options.base_generation})).map_err(js)?))?;
    js_sys::Reflect::set(&result,&js("buffers"),&result_buffers)?;Ok(result.into())
}

struct Pack {segments:BTreeMap<u64,Vec<Arc<[u8]>>>,length:u64}
impl ByteSource for Pack {
    fn byte_len(&self)->u64 {self.length}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        let mut output=Vec::with_capacity(length);let mut cursor=offset;
        while output.len()<length {
            let (&start,blocks)=self.segments.range(..=cursor).next_back().ok_or("Missing session resource range")?;
            let relative=usize::try_from(cursor-start).map_err(|_|"Session offset exceeds admission")?;
            let block=blocks.get(relative/BLOCK).ok_or("Incomplete session resource range")?;
            let position=relative%BLOCK;
            let count=(length-output.len()).min(block.len().checked_sub(position).ok_or("Session resource range overflow")?);
            if count==0{return Err("Incomplete session resource range".into());}
            output.extend_from_slice(&block[position..position+count]);cursor+=count as u64;
        }
        Ok(RangeState::Ready(ByteRange::new(output.into(),0..length).map_err(str::to_string)?))
    }
}
#[wasm_bindgen]
pub async fn raster_worker_restart_open(checkpoint:&str,buffers:js_sys::Array)->Result<JsValue,JsValue> {
    #[derive(Deserialize)]struct Checkpoint {generation:u64,resources:Vec<String>,metadata:String}
    let checkpoint:Checkpoint=serde_json::from_str(checkpoint).map_err(js)?;
    let _=checkpoint.generation;
    if checkpoint.resources.len()!=buffers.length() as usize{return Err(js("Missing session resource blocks"));}
    let mut blocks=BTreeMap::new();let mut total=0usize;
    for (index,id) in checkpoint.resources.into_iter().enumerate() {
        let bytes=buffers.get(index as u32).dyn_into::<js_sys::Uint8Array>().map_err(|_|js("Missing session resource block"))?;
        total=total.checked_add(bytes.length() as usize).filter(|n|*n<=1024*1024*1024).ok_or_else(||js("Session resources exceed admission"))?;
        if bytes.length() as usize>BLOCK{return Err(js("Oversized session resource block"));}
        if blocks.insert(id,Arc::<[u8]>::from(bytes.to_vec())).is_some(){return Err(js("Duplicate session resource block"));}
        buffers.set(index as u32,JsValue::UNDEFINED);documents::yield_browser().await?;
    }
    let metadata:serde_json::Value=serde_json::from_str(&checkpoint.metadata).map_err(js)?;
    let mut segments=BTreeMap::new();let mut length=0u64;
    for resource in metadata["resources"].as_array().ok_or_else(||js("Missing session resource inventory"))? {
        let offset=resource["location"]["offset"].as_str().and_then(|s|s.parse::<u64>().ok()).ok_or_else(||js("Invalid session resource offset"))?;
        let size=resource["bytes"].as_str().and_then(|s|s.parse::<u64>().ok()).ok_or_else(||js("Invalid session resource length"))?;
        length=length.max(offset.checked_add(size).ok_or_else(||js("Session pack length overflow"))?);
        if segments.contains_key(&offset)||size==0{continue;}
        let id=resource["id"].as_str().ok_or_else(||js("Missing session resource identity"))?;
        let mut resource_blocks=Vec::new();let mut actual=0u64;
        for part in (0..size).step_by(BLOCK) {
            let bytes=blocks.get(&format!("{id}-{}",part/BLOCK as u64)).ok_or_else(||js("Missing saved session resource"))?;
            if bytes.len() as u64!=(size-part).min(BLOCK as u64){return Err(js("Incomplete saved session resource"));}
            actual+=bytes.len() as u64;resource_blocks.push(bytes.clone());
        }
        if actual!=size{return Err(js("Incomplete session resource payload"));}
        segments.insert(offset,resource_blocks);
    }
    let backing=ImmutableBacking::new(Arc::new(Pack{segments,length})).map_err(js)?;
    let opened=layer_core::package::session::open_parts(checkpoint.metadata.as_bytes(),backing,ProjectLimits::default(),&AtomicBool::new(false)).map_err(js)?;
    layer_ui::SessionRestore::validate_core(&opened).map_err(js)?;
    let capture=opened.editor.capture_session(opened.editor.capture(1,Default::default()).map_err(js)?).map_err(js)?;
    pack(capture,opened.metadata).await
}

#[wasm_bindgen]
pub struct WebFileFingerprint(Option<layer_ui::FingerprintWriter<std::io::Sink>>);
#[wasm_bindgen]
impl WebFileFingerprint {
    #[wasm_bindgen(constructor)]
    pub fn new()->Self {Self(Some(layer_ui::FingerprintWriter::new(std::io::sink())))}
    pub fn update(&mut self,bytes:&[u8])->Result<(),JsValue> {
        use std::io::Write;
        self.0.as_mut().ok_or_else(||js("File fingerprint is complete"))?.write_all(bytes).map_err(js)
    }
    pub fn finish(&mut self)->Result<JsValue,JsValue> {
        serialize(&self.0.take().ok_or_else(||js("File fingerprint is complete"))?.finish())
    }
}
