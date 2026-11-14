use super::*;
use layer_core::{Artwork,ArtworkCapture,CaptureCheckpoint,ProjectLimits,package::{MAX_RANGE_BYTES,transfer::{PreparedTransfer,TransferDescriptor,TransferReceiver}}};
use std::sync::atomic::AtomicBool;
use wasm_bindgen_futures::JsFuture;

const BLOCK:usize=4*1024*1024;
type CopyAdmission=std::rc::Rc<std::cell::RefCell<Option<layer_core::MemoryReservation>>>;
#[wasm_bindgen]
pub struct WebTransferLease {admission:CopyAdmission}
#[wasm_bindgen]
impl WebTransferLease {pub fn release(&self) {self.admission.borrow_mut().take();}}
pub(super) struct PackedArtwork {value:JsValue,admission:Option<CopyAdmission>,offer:Option<String>}
impl std::ops::Deref for PackedArtwork {type Target=JsValue;fn deref(&self)->&JsValue {&self.value}}
impl Drop for PackedArtwork {fn drop(&mut self) {if let Some(admission)=&self.admission {admission.borrow_mut().take();}if let Some(ticket)=&self.offer {crate::store_transport::cancel_sources(ticket);}}}
impl PackedArtwork {
    pub(super) fn empty()->Self {Self {value:js_sys::Object::new().into(),admission:None,offer:None}}
    pub(super) fn push_buffer(&mut self,bytes:&[u8])->Result<u32,JsValue> {
        let buffers=js_sys::Reflect::get(&self.value,&js("buffers"))?.dyn_into::<js_sys::Array>()?;
        if let Some(admission)=&self.admission {
            let mut held=admission.borrow_mut();let held=held.as_mut().ok_or_else(||js("Artwork transfer was already posted"))?;
            let required=held.reserved_bytes().checked_add(bytes.len() as u64).ok_or_else(||js(layer_core::MEMORY_BUDGET_EXCEEDED))?;
            if !held.resize(required) {return Err(js(layer_core::MEMORY_BUDGET_EXCEEDED));}
        }else {
            let held=layer_core::MemoryBudget::process().admit_import(bytes.len() as u64).ok_or_else(||js(layer_core::MEMORY_BUDGET_EXCEEDED))?;
            let admission=std::rc::Rc::new(std::cell::RefCell::new(Some(held)));self.admission=Some(admission.clone());
            js_sys::Reflect::set(&buffers,&js("admission"),&JsValue::from(WebTransferLease {admission}))?;
        }
        let index=buffers.length();buffers.push(&js_sys::Uint8Array::from(bytes));Ok(index)
    }
    pub(super) fn into_js(mut self)->JsValue {self.admission=None;self.offer=None;self.value.clone()}
}
pub(super) fn limits(dimension:u32)->ProjectLimits {seed_memory_budget();ProjectLimits {dimension,tiles:8192,..ProjectLimits::for_import()}}

#[derive(Serialize,Deserialize)]
struct Envelope {transfer:TransferDescriptor,chunks:Vec<usize>,#[serde(default,skip_serializing_if="Option::is_none")]offer:Option<String>}
pub(super) struct SourceOffer(pub(super) Option<String>);
impl Drop for SourceOffer {fn drop(&mut self) {if let Some(ticket)=&self.0 {crate::store_transport::cancel_sources(ticket);}}}
pub(super) fn received_offer(value:&JsValue)->SourceOffer {
    SourceOffer(js_sys::Reflect::get(value,&js("metadata")).ok().and_then(|metadata|metadata.as_string()).and_then(|metadata|serde_json::from_str::<serde_json::Value>(&metadata).ok()).and_then(|metadata|metadata["offer"].as_str().map(str::to_owned)))
}

pub(super) async fn wait_backing(artwork:&Artwork)->Result<(),JsValue>{wait_backing_cancellable(artwork,None).await}
pub(super) async fn wait_backing_cancellable(artwork:&Artwork,control:Option<&layer_render_wgpu::snapshot::CaptureControl>)->Result<(),JsValue>{
    for raster in artwork.paint.iter().map(|(_,_,paint)|&paint.raster).chain(artwork.coverage.iter().map(|(_,_,coverage)|&coverage.raster)) {
        if control.is_some_and(|control|control.is_cancelled()) {return Err(js("Operation cancelled"));}
        let data=raster.await_data().await.map_err(js)?;
        for tile in data.tiles.values() {
            if control.is_some_and(|control|control.is_cancelled()) {return Err(js("Operation cancelled"));}
            tile.await_backing().await.map_err(js)?;
        }
    }Ok(())
}

pub(super) async fn pack(artwork:Artwork)->Result<PackedArtwork,JsValue>{pack_scene(artwork,None).await}
fn scene_capture(artwork:Artwork)->Result<ArtworkCapture,JsValue> {
    let checkpoint=CaptureCheckpoint {document:artwork.id,owner:0,session_generation:0,artwork_generation:0,working_generation:0,edit_checkpoint:0};
    artwork.capture(checkpoint).map_err(js)
}
pub(super) async fn pack_scene(artwork:Artwork,selection:Option<layer_core::Selection>)->Result<PackedArtwork,JsValue>{
    pack_with_selection(scene_capture(artwork)?,selection).await
}
async fn pack_capture(capture:ArtworkCapture)->Result<PackedArtwork,JsValue>{pack_with_selection(capture,None).await}
async fn pack_with_selection(capture:ArtworkCapture,selection:Option<layer_core::Selection>)->Result<PackedArtwork,JsValue>{
    seed_memory_budget();
    wait_backing(&capture.artwork).await?;
    crate::store_transport::configure_reads();
    let transfer=PreparedTransfer::capture_referenced(&capture,&selection,&AtomicBool::new(false),&layer_core::MemoryBudget::process()).await.map_err(js)?;
    let chunks=(0..transfer.payload_count()).map(|index|usize::try_from(transfer.payload_len(index).map_err(js)?).map(|length|length.div_ceil(BLOCK)).map_err(js)).collect::<Result<Vec<_>,JsValue>>()?;
    let offer=crate::store_transport::offer_sources(transfer.descriptor().backings()).await.map_err(js)?;
    let mut packed=PackedArtwork {value:js_sys::Object::new().into(),admission:None,offer:offer.clone()};
    let metadata=serde_json::to_string(&Envelope{transfer:transfer.descriptor().clone(),chunks,offer}).map_err(js)?;
    let overhead=(metadata.len() as u64).checked_mul(3).and_then(|bytes|bytes.checked_add(BLOCK as u64)).ok_or_else(||js(layer_core::MEMORY_BUDGET_EXCEEDED))?;
    let reservation=transfer.reserve_copy(&layer_core::MemoryBudget::process(),overhead).map_err(js)?;
    let admission=std::rc::Rc::new(std::cell::RefCell::new(Some(reservation)));
    packed.admission=Some(admission.clone());
    let buffers=js_sys::Array::new();
    for index in 0..transfer.payload_count(){
        let len=usize::try_from(transfer.payload_len(index).map_err(js)?).map_err(js)?;
        for offset in (0..len).step_by(BLOCK){
            let bytes=transfer.read_chunk_async(index,offset as u64,(len-offset).min(BLOCK)).await.map_err(js)?;
            buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));
            documents::yield_browser().await?;
        }
    }
    js_sys::Reflect::set(&buffers,&js("admission"),&JsValue::from(WebTransferLease {admission}))?;
    js_sys::Reflect::set(&packed,&js("metadata"),&js(metadata))?;js_sys::Reflect::set(&packed,&js("buffers"),&buffers)?;Ok(packed)
}
pub(super) async fn unpack(metadata:&str,buffers:js_sys::Array)->Result<Artwork,JsValue>{Ok(unpack_capture(metadata,buffers).await?.artwork.as_ref().clone())}
pub(super) async fn unpack_scene(metadata:&str,buffers:js_sys::Array)->Result<(Artwork,Option<layer_core::Selection>),JsValue>{let(capture,selection)=unpack_with_selection(metadata,buffers).await?;Ok((capture.artwork.as_ref().clone(),selection))}
async fn unpack_capture(metadata:&str,buffers:js_sys::Array)->Result<ArtworkCapture,JsValue>{Ok(unpack_with_selection(metadata,buffers).await?.0)}
async fn unpack_with_selection(metadata:&str,buffers:js_sys::Array)->Result<(ArtworkCapture,Option<layer_core::Selection>),JsValue>{
    if metadata.len()>64*1024*1024{return Err(js("Oversized artwork metadata"));}
    let envelope:Envelope=serde_json::from_str(metadata).map_err(js)?;
    let mut offer=SourceOffer(envelope.offer.clone());
    let count=envelope.chunks.iter().try_fold(0usize,|sum,n|sum.checked_add(*n)).ok_or_else(||js("Transfer chunk count overflow"))?;
    if count!=buffers.length() as usize{return Err(js("Incomplete artwork transfer"));}
    let admitted_limits=ProjectLimits {tiles:8192,..Default::default()};
    let sources=crate::store_transport::adopt_sources(envelope.offer.as_deref(),envelope.transfer.backings()).await.map_err(js)?;
    offer.0=None;
    let mut sources=sources.into_iter();
    let mut receiver=TransferReceiver::with_sources(envelope.transfer,admitted_limits,|_|sources.next().ok_or_else(||"Missing backing owner".into())).await.map_err(js)?;let mut index=0;let mut total=0usize;
    for (payload,count) in envelope.chunks.into_iter().enumerate() {
        for _ in 0..count {
            let buffer=buffers.get(index).dyn_into::<js_sys::Uint8Array>().map_err(|_|js("Missing artwork transfer chunk"))?;
            if buffer.length() as usize>MAX_RANGE_BYTES{return Err(js("Oversized artwork transfer chunk"));}
            total=total.checked_add(buffer.length() as usize).filter(|n|*n as u64<=admitted_limits.raster_bytes.saturating_add(admitted_limits.asset_bytes)).ok_or_else(||js("Artwork transfer exceeds budget"))?;
            receiver.push_chunk(payload,&buffer.to_vec()).map_err(js)?;buffers.set(index,JsValue::UNDEFINED);index+=1;
            documents::yield_browser().await?;
        }
    }
    receiver.finish().map_err(js)?.adopt_verified_with_selection(admitted_limits,&AtomicBool::new(false)).map_err(js)
}
fn parts(wire:&JsValue)->Result<(String,js_sys::Array),JsValue>{Ok((js_sys::Reflect::get(wire,&js("metadata"))?.as_string().ok_or_else(||js("Missing artwork transfer metadata"))?,js_sys::Reflect::get(wire,&js("buffers"))?.dyn_into()?))}
pub(super) async fn convert_color(original: &layer_core::Document, change: layer_color::DocumentColorChange, control: &layer_render_wgpu::snapshot::CaptureControl) -> Result<(layer_core::Document, u64), JsValue> {
    output::cancelled(control)?;
    let wire = pack(layer_color::color_job_artwork(&original.artwork)).await?;
    let (metadata, buffers) = parts(&wire)?;
    let request = serde_json::to_string(&serde_json::json!({"project":metadata,"change":change})).map_err(js)?;
    let result = JsFuture::from(raster_worker::call("color-convert", &request, &buffers)?).await?;
    let _offer=received_offer(&result);
    output::cancelled(control)?;
    let (metadata, buffers) = parts(&result)?;
    let clipped = js_sys::Reflect::get(&result, &js("clipped"))?.as_f64().unwrap_or(0.) as u64;
    let artwork = unpack(&metadata, buffers).await?;
    Ok((layer_color::adopt_color_job_artwork(original, artwork).map_err(js)?, clipped))
}

pub(super) async fn save(capture:ArtworkCapture)->Result<JsValue,JsValue>{let wire=pack_capture(capture).await?;let (metadata,buffers)=parts(&wire)?;JsFuture::from(raster_worker::call("write",&metadata,&buffers)?).await}
#[derive(Serialize,Deserialize)]
pub(super) struct OpenOptions {pub dimension:u32,pub photo_policy:layer_ui::PhotoOpenPolicy,pub name:std::sync::Arc<str>,pub intent:layer_ui::ImportIntent,#[serde(default)]pub source_bytes:Option<usize>}
pub(super) type Preserved = (layer_ui::PackagePresentation,Option<js_sys::Object>);
pub(super) enum Opened {Editable {document:layer_ui::ImportedDocument,fallback:Option<Preserved>},Package {presentation:layer_ui::PackagePresentation,preview:Option<js_sys::Object>}}
pub(super) async fn open(file:web_sys::Blob,options:OpenOptions,cancelled:Option<js_sys::Function>)->Result<Opened,JsValue>{
    let source=crate::store_transport::register_blob(&file).await.map_err(js)?;
    let request=js_sys::Object::new();
    js_sys::Reflect::set(&request,&js("operation"),&js("read"))?;
    js_sys::Reflect::set(&request,&js("metadata"),&js(serde_json::to_string(&options).map_err(js)?))?;
    js_sys::Reflect::set(&request,&js("buffers"),&js_sys::Array::new())?;
    js_sys::Reflect::set(&request,&js("file"),source.file())?;
    js_sys::Reflect::set(&request,&js("source_key"),&js(source.key()))?;
    if let Some(cancelled)=cancelled {js_sys::Reflect::set(&request,&js("cancelled"),&cancelled)?;}
    let wire=JsFuture::from(raster_worker::request(&request)?).await?;
    let _offer=received_offer(&wire);
    let presentation=js_sys::Reflect::get(&wire,&js("package"))?;
    if !presentation.is_undefined(){
        let presentation=serde_wasm_bindgen::from_value(presentation).map_err(js)?;
        let preview=js_sys::Reflect::get(&wire,&js("preview"))?;
        return Ok(Opened::Package {presentation,preview:if preview.is_null()||preview.is_undefined(){None}else{Some(preview.dyn_into()?)}});
    }
    let(metadata,buffers)=parts(&wire)?;
    let source=serde_wasm_bindgen::from_value(js_sys::Reflect::get(&wire,&js("source"))?).map_err(js)?;
    let document=unpack_capture(&metadata,buffers).await?.into_document().map_err(js)?;
    let fallback=js_sys::Reflect::get(&wire,&js("fallback"))?;
    let fallback=if fallback.is_undefined(){None}else{
        let presentation=serde_wasm_bindgen::from_value(fallback).map_err(js)?;
        let preview=js_sys::Reflect::get(&wire,&js("preview"))?;
        Some((presentation,if preview.is_null()||preview.is_undefined(){None}else{Some(preview.dyn_into()?)}))
    };
    Ok(Opened::Editable {document:layer_ui::ImportedDocument::new(document,source),fallback})
}
#[wasm_bindgen]
pub async fn raster_worker_read(options:&str,file:web_sys::Blob,source_key:Option<String>)->Result<JsValue,JsValue>{
    if options.len()>64*1024 {return Err(js("Oversized file read options"));}
    let options:OpenOptions=serde_json::from_str(options).map_err(js)?;
    let backing=blob::Backing::with_key(file,source_key)?;
    let mut photo_limits=layer_color::photo::DecodeLimits::from_memory_budget(photo_memory_budget());
    if let Some(remaining)=options.source_bytes{photo_limits.source_bytes=photo_limits.source_bytes.min(remaining);}
    let imported=layer_ui::read_import_backing(backing.source.clone(),options.intent,options.photo_policy,options.name,limits(options.dimension),photo_limits,&Default::default()).map_err(js)?;
    match imported {
        layer_ui::ImportOutcome::Editable(imported)=>{
            if let Err(reason)=hdr::admit_document(&imported.project){
                return match imported.preserve_unsupported(reason.as_string().unwrap_or_else(||format!("{reason:?}"))) {
                    Some(outcome)=>package_value(outcome),None=>Err(reason),
                };
            }
            let fallback=imported.preserve_unsupported("").map(layer_ui::PackageView::new).transpose().map_err(js)?;
            let wire=pack(imported.project.artwork).await?;js_sys::Reflect::set(&wire,&js("source"),&serialize(&imported.source)?)?;
            if let Some(view)=fallback {
                js_sys::Reflect::set(&wire,&js("fallback"),&serialize(&view.presentation())?)?;
                if let Some(preview)=view.preview(){js_sys::Reflect::set(&wire,&js("preview"),&preview_value(preview)?)?;}
            }
            Ok(wire.into_js())
        }
        layer_ui::ImportOutcome::Package(outcome)=>package_value(outcome),
    }
}
fn package_value(outcome:layer_core::package::codec::OpenOutcome)->Result<JsValue,JsValue> {
    let view=layer_ui::PackageView::new(outcome).map_err(js)?;
    web_sys::console::info_1(&js(view.reason()));
    let wire=js_sys::Object::new();js_sys::Reflect::set(&wire,&js("package"),&serialize(&view.presentation())?)?;
    if let Some(preview)=view.preview(){js_sys::Reflect::set(&wire,&js("preview"),&preview_value(preview)?)?;}
    Ok(wire.into())
}
fn preview_value(preview:&layer_core::package::preview::Preview)->Result<JsValue,JsValue> {
    let value=js_sys::Object::new();
    js_sys::Reflect::set(&value,&js("encoded"),&js_sys::Uint8Array::from(preview.encoded().as_ref()))?;
    js_sys::Reflect::set(&value,&js("pixels"),&js_sys::Uint8Array::from(preview.pixels().as_ref()))?;
    Ok(value.into())
}

#[wasm_bindgen]
pub async fn raster_worker_write(metadata:String,buffers:js_sys::Array,write:js_sys::Function)->Result<JsValue,JsValue>{
    struct Output {write:js_sys::Function,offset:u64}
    impl std::io::Write for Output {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize>{
            let count=bytes.len().min(BLOCK);
            let value=self.write.call2(&JsValue::NULL,&JsValue::from_f64(self.offset as f64),&js_sys::Uint8Array::from(&bytes[..count])).map_err(|e|std::io::Error::other(format!("{e:?}")))?;
            if value.as_f64()!=Some(count as f64){return Err(std::io::Error::other("Incomplete drawing write"));}self.offset+=count as u64;Ok(count)
        }
        fn flush(&mut self)->std::io::Result<()>{Ok(())}
    }
    let capture=unpack_capture(&metadata,buffers).await?;let cancelled=AtomicBool::new(false);
    let preview=if let Some(composition)=capture.artwork.compositions.get(capture.artwork.root) {
        match raster_worker::snapshot_gpu(composition.color).await {
            Ok(gpu)=>gpu.package_preview_async(&capture,&cancelled).await,
            Err(_)=>None,
        }
    } else {None};
    let mut output=layer_ui::FingerprintWriter::new(Output{write,offset:0});
    layer_core::package::codec::PreparedPackage::prepare_async(&capture,preview,&cancelled).await.map_err(js)?.write_async(&mut output,&cancelled).await.map_err(js)?;
    serialize(&output.finish())
}

pub(super) fn seed_memory_budget() {
    static SEEDED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    SEEDED.get_or_init(|| {
        let capacity=js_sys::Reflect::get(&js_sys::global(),&js("navigator")).ok()
            .and_then(|navigator|js_sys::Reflect::get(&navigator,&js("deviceMemory")).ok())
            .and_then(|value|value.as_f64()).filter(|value|value.is_finite()&&*value>0.)
            .map(|gib|(gib*(1024_u64.pow(3) as f64)).min(u64::MAX as f64) as u64);
        let used=wasm_bindgen::memory().dyn_into::<js_sys::WebAssembly::Memory>().ok()
            .map(|memory|js_sys::Uint8Array::new(&memory.buffer()).length() as u64);
        let ceiling=isize::MAX as u64;
        let headroom=ceiling.saturating_sub(used.unwrap_or(0));
        let default=capacity.map_or(2*1024_u64.pow(3),|bytes|bytes/4).min(headroom);
        layer_core::MemoryBudget::process().update_ram(layer_core::MemoryFacts {
            host_default_bytes:Some(default),process_limit_bytes:Some(ceiling),process_used_bytes:used,
            ..Default::default()
        },layer_core::MemoryBudget::now_ms());
    });
}
pub(super) fn photo_memory_budget()->layer_color::photo::PhotoMemoryBudget {
    seed_memory_budget();
    layer_color::photo::PhotoMemoryBudget::from_limit(layer_core::MemoryBudget::process().import_headroom_bytes())
}
