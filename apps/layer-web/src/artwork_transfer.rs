use super::*;
use layer_core::{Artwork,ArtworkCapture,CaptureCheckpoint,ProjectLimits,package::{MAX_RANGE_BYTES,transfer::{PreparedTransfer,TransferDescriptor,TransferReceiver}}};
use std::sync::atomic::AtomicBool;
use wasm_bindgen_futures::JsFuture;

const BLOCK:usize=4*1024*1024;
fn limits(dimension:u32)->ProjectLimits {ProjectLimits {dimension,asset_bytes:256*1024*1024,raster_bytes:512*1024*1024,tiles:8192,..Default::default()}}

#[derive(Serialize,Deserialize)]
struct Envelope {transfer:TransferDescriptor,chunks:Vec<usize>}

pub(super) async fn wait_backing(artwork:&Artwork)->Result<(),JsValue>{wait_backing_cancellable(artwork,None).await}
pub(super) async fn wait_backing_cancellable(artwork:&Artwork,control:Option<&layer_render_wgpu::snapshot::CaptureControl>)->Result<(),JsValue>{
    let start=js_sys::Date::now();
    loop {
        if control.is_some_and(|c|c.is_cancelled()){return Err(js("Operation cancelled"));}
        let mut ready=true;
        for raster in artwork.paint.iter().map(|(_,_,p)|&p.raster).chain(artwork.coverage.iter().map(|(_,_,p)|&p.raster)) {
            match raster.try_data(){None=>ready=false,Some(Err(e))=>return Err(js(e)),Some(Ok(data))=>{
                for tile in data.tiles.values(){match tile.try_backing(){None=>ready=false,Some(Err(e))=>return Err(js(e)),Some(Ok(blob))=>ready &=blob.compressed_ready().map_err(js)?,}}
            }}
        }
        for (_,_,paint) in artwork.paint.iter(){if let Some(source)=&paint.original{for tile in source.tiles.values(){ready &=tile.compressed_ready().map_err(js)?;}}}
        if ready{return Ok(());}
        if js_sys::Date::now()-start>30_000.{return Err(js("Raster backing timed out"));}
        documents::yield_browser().await?;
    }
}

pub(super) async fn pack(artwork:Artwork)->Result<JsValue,JsValue>{pack_scene(artwork,None).await}
pub(super) async fn pack_scene(artwork:Artwork,selection:Option<layer_core::Selection>)->Result<JsValue,JsValue>{
    let checkpoint=CaptureCheckpoint {document:artwork.id,owner:0,session_generation:0,artwork_generation:0,working_generation:0,edit_checkpoint:0};
    pack_with_selection(artwork.capture(checkpoint).map_err(js)?,selection).await
}
async fn pack_capture(capture:ArtworkCapture)->Result<JsValue,JsValue>{pack_with_selection(capture,None).await}
async fn pack_with_selection(capture:ArtworkCapture,selection:Option<layer_core::Selection>)->Result<JsValue,JsValue>{
    wait_backing(&capture.artwork).await?;
    let transfer=PreparedTransfer::capture_with_selection(&capture,&selection,&AtomicBool::new(false)).map_err(js)?;
    let buffers=js_sys::Array::new();let mut chunks=Vec::new();
    for index in 0..transfer.payload_count(){
        let len=usize::try_from(transfer.payload_len(index).map_err(js)?).map_err(js)?;let mut count=0;
        for offset in (0..len).step_by(BLOCK){
            let bytes=transfer.read_chunk(index,offset as u64,(len-offset).min(BLOCK)).map_err(js)?;
            buffers.push(&js_sys::Uint8Array::from(bytes.as_slice()));count+=1;
            documents::yield_browser().await?;
        }
        chunks.push(count);
    }
    let metadata=serde_json::to_string(&Envelope{transfer:transfer.descriptor().clone(),chunks}).map_err(js)?;
    let result=js_sys::Object::new();js_sys::Reflect::set(&result,&js("metadata"),&js(metadata))?;js_sys::Reflect::set(&result,&js("buffers"),&buffers)?;Ok(result.into())
}
pub(super) async fn unpack(metadata:&str,buffers:js_sys::Array)->Result<Artwork,JsValue>{Ok(unpack_capture(metadata,buffers).await?.artwork.as_ref().clone())}
pub(super) async fn unpack_scene(metadata:&str,buffers:js_sys::Array)->Result<(Artwork,Option<layer_core::Selection>),JsValue>{let(capture,selection)=unpack_with_selection(metadata,buffers).await?;Ok((capture.artwork.as_ref().clone(),selection))}
async fn unpack_capture(metadata:&str,buffers:js_sys::Array)->Result<ArtworkCapture,JsValue>{Ok(unpack_with_selection(metadata,buffers).await?.0)}
async fn unpack_with_selection(metadata:&str,buffers:js_sys::Array)->Result<(ArtworkCapture,Option<layer_core::Selection>),JsValue>{
    if metadata.len()>64*1024*1024{return Err(js("Oversized artwork metadata"));}
    let envelope:Envelope=serde_json::from_str(metadata).map_err(js)?;
    let count=envelope.chunks.iter().try_fold(0usize,|sum,n|sum.checked_add(*n)).ok_or_else(||js("Transfer chunk count overflow"))?;
    if count!=buffers.length() as usize{return Err(js("Incomplete artwork transfer"));}
    let mut receiver=TransferReceiver::new(envelope.transfer,limits(ProjectLimits::default().dimension)).map_err(js)?;let mut index=0;let mut total=0usize;
    for (payload,count) in envelope.chunks.into_iter().enumerate() {
        for _ in 0..count {
            let buffer=buffers.get(index).dyn_into::<js_sys::Uint8Array>().map_err(|_|js("Missing artwork transfer chunk"))?;
            if buffer.length() as usize>MAX_RANGE_BYTES{return Err(js("Oversized artwork transfer chunk"));}
            total=total.checked_add(buffer.length() as usize).filter(|n|*n<=1024*1024*1024).ok_or_else(||js("Artwork transfer exceeds budget"))?;
            receiver.push_chunk(payload,&buffer.to_vec()).map_err(js)?;buffers.set(index,JsValue::UNDEFINED);index+=1;
            documents::yield_browser().await?;
        }
    }
    receiver.finish().map_err(js)?.adopt_verified_with_selection(limits(ProjectLimits::default().dimension),&AtomicBool::new(false)).map_err(js)
}
fn parts(wire:&JsValue)->Result<(String,js_sys::Array),JsValue>{Ok((js_sys::Reflect::get(wire,&js("metadata"))?.as_string().ok_or_else(||js("Missing artwork transfer metadata"))?,js_sys::Reflect::get(wire,&js("buffers"))?.dyn_into()?))}
pub(super) async fn save(capture:ArtworkCapture)->Result<JsValue,JsValue>{let wire=pack_capture(capture).await?;let (metadata,buffers)=parts(&wire)?;JsFuture::from(raster_worker::call("write",&metadata,&buffers)?).await}
#[derive(Serialize,Deserialize)]
pub(super) struct OpenOptions {pub dimension:u32,pub photo_policy:layer_ui::PhotoOpenPolicy,pub names:layer_core::DocumentNames,pub intent:layer_ui::ImportIntent,#[serde(default)]pub source_bytes:Option<usize>}
pub(super) type Preserved = (layer_ui::PackagePresentation,Option<js_sys::Uint8Array>);
pub(super) enum Opened {Editable {document:layer_ui::ImportedDocument,fallback:Option<Preserved>},Package {presentation:layer_ui::PackagePresentation,preview:Option<js_sys::Uint8Array>}}
pub(super) async fn open(bytes:js_sys::Uint8Array,options:OpenOptions)->Result<Opened,JsValue>{
    let buffers=js_sys::Array::new();buffers.push(&bytes);
    let wire=JsFuture::from(raster_worker::call("read",&serde_json::to_string(&options).map_err(js)?,&buffers)?).await?;
    let presentation=js_sys::Reflect::get(&wire,&js("package"))?;
    if !presentation.is_undefined(){
        let presentation=serde_wasm_bindgen::from_value(presentation).map_err(js)?;
        let preview=js_sys::Reflect::get(&wire,&js("preview"))?;
        return Ok(Opened::Package {presentation,preview:if preview.is_null()||preview.is_undefined(){None}else{Some(preview.dyn_into()?)}});
    }
    let(metadata,buffers)=parts(&wire)?;
    let source=serde_wasm_bindgen::from_value(js_sys::Reflect::get(&wire,&js("source"))?).map_err(js)?;
    let document=layer_core::Document::from_artwork(unpack(&metadata,buffers).await?).map_err(js)?;
    let fallback=js_sys::Reflect::get(&wire,&js("fallback"))?;
    let fallback=if fallback.is_undefined(){None}else{
        let presentation=serde_wasm_bindgen::from_value(fallback).map_err(js)?;
        let preview=js_sys::Reflect::get(&wire,&js("preview"))?;
        Some((presentation,if preview.is_null()||preview.is_undefined(){None}else{Some(preview.dyn_into()?)}))
    };
    Ok(Opened::Editable {document:layer_ui::ImportedDocument::new(document,source),fallback})
}
#[wasm_bindgen]
pub async fn raster_worker_read(options:&str,bytes:Vec<u8>)->Result<JsValue,JsValue>{
    let options:OpenOptions=serde_json::from_str(options).map_err(js)?;
    let mut photo_limits=layer_color::photo::DecodeLimits::from_memory_budget(photo_memory_budget());
    if let Some(remaining)=options.source_bytes{photo_limits.source_bytes=photo_limits.source_bytes.min(remaining);}
    let imported=layer_ui::read_import(std::io::Cursor::new(&bytes),options.intent,options.photo_policy,options.names,limits(options.dimension),photo_limits,&Default::default()).map_err(js)?;
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
                if let Some(preview)=view.preview(){js_sys::Reflect::set(&wire,&js("preview"),&js_sys::Uint8Array::from(preview.encoded().as_ref()))?;}
            }
            Ok(wire)
        }
        layer_ui::ImportOutcome::Package(outcome)=>package_value(outcome),
    }
}
fn package_value(outcome:layer_core::package::codec::OpenOutcome)->Result<JsValue,JsValue> {
    let view=layer_ui::PackageView::new(outcome).map_err(js)?;
    let wire=js_sys::Object::new();js_sys::Reflect::set(&wire,&js("package"),&serialize(&view.presentation())?)?;
    if let Some(preview)=view.preview(){js_sys::Reflect::set(&wire,&js("preview"),&js_sys::Uint8Array::from(preview.encoded().as_ref()))?;}
    Ok(wire.into())
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
    layer_core::package::codec::PreparedPackage::prepare(&capture,preview,&cancelled).map_err(js)?.write(&mut output,&cancelled).map_err(js)?;
    serialize(&output.finish())
}

pub(super) fn photo_memory_budget()->layer_color::photo::PhotoMemoryBudget {
    use layer_color::photo::PhotoMemoryBudget;
    let capacity=js_sys::Reflect::get(&js_sys::global(),&js("navigator")).ok().and_then(|n|js_sys::Reflect::get(&n,&js("deviceMemory")).ok()).and_then(|v|v.as_f64()).filter(|v|v.is_finite()&&*v>0.);
    let Some(gib)=capacity else{return PhotoMemoryBudget::current();};let capacity=(gib.min(8.)*(1024_u64.pow(3) as f64)) as u64;
    PhotoMemoryBudget{source_bytes:(capacity/32) as usize,decode_bytes:(capacity/8) as usize,encode_bytes:(capacity/16*3) as usize}
}
