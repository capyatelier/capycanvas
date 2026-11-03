use super::*;
use layer_core::{PortableId,ProjectLimits,package::{ImmutableBacking,session_store::{ImportFailure,MetadataStore}},tile_io::{ReadCache,RangeReader,ReadCompletion}};
use layer_host::session_stores::{SessionStores,StoreResult};
use layer_ui::session_recovery::{SessionManifest,import::{ImportLedger,ImportRecord,SourceVersion,merge_window,block_copy}};
use std::{collections::BTreeMap,sync::Arc};
use wasm_bindgen_futures::spawn_local;

fn error(value:JsValue)->String {value.as_string().unwrap_or_else(||format!("{value:?}"))}
#[derive(Deserialize)]
struct Checkpoint {generation:u64,metadata:String,metadata_sha256:String}
impl Checkpoint {
    fn version(&self,key:&str)->Result<SourceVersion,String> {
        if self.metadata.len() as u64>ProjectLimits::for_import().metadata_bytes {return Err("Saved session is too large".into());}
        SourceVersion::capture(key.into(),self.generation,&self.metadata)
    }
    fn verified(&self,key:&str)->Result<(),ImportFailure> {
        let version=self.version(key).map_err(ImportFailure::Invalid)?;
        if version.sha256!=self.metadata_sha256 {return Err(ImportFailure::Invalid("Drawing checkpoint metadata is damaged".into()));}Ok(())
    }
}
#[derive(Deserialize)]
struct Snapshot {current:Checkpoint,previous:Option<Checkpoint>}
struct ReleasedReader {key:String,resource:String,_lease:Arc<dyn Send+Sync>}
impl RangeReader for ReleasedReader {
    fn read(&self,offset:u64,length:usize,crc:Option<u32>,completion:ReadCompletion) {
        let (key,resource,lease)=(self.key.clone(),self.resource.clone(),self._lease.clone());
        spawn_local(async move {
            let _lease=lease;
            let result=async {
                let value=store_transport::call("legacy-range",serde_json::json!({"key":key,"resource":resource,"offset":offset,"length":length}),None).await?;
                let buffers=js_sys::Reflect::get(&value,&js("buffers")).map_err(error)?.dyn_into::<js_sys::Array>().map_err(error)?;
                let bytes=buffers.get(0).dyn_into::<js_sys::Uint8Array>().map_err(error)?;
                if buffers.length()!=1 || bytes.length() as usize!=length {return Err("Incomplete saved session range".into());}
                let bytes:Arc<[u8]>=bytes.to_vec().into();layer_core::tile_io::verify_crc(&bytes,crc)?;Ok(bytes)
            }.await;
            completion.complete(result);
        });
    }
}
fn resources(key:&str,checkpoint:&Checkpoint,lease:Arc<dyn Send+Sync>)->Result<BTreeMap<PortableId,ImmutableBacking>,String> {
    let value=layer_core::package::parse_json(checkpoint.metadata.as_bytes(),ProjectLimits::for_import().metadata_bytes as usize)?;
    let mut result=BTreeMap::new();let mut aliases:BTreeMap<_,ImmutableBacking>=BTreeMap::new();
    for record in value["resources"].as_array().ok_or("Missing saved session resource inventory")? {
        let id:PortableId=record["id"].as_str().ok_or("Missing saved resource identity")?.parse().map_err(str::to_string)?;
        let location=&record["location"];
        let pack=location["pack"].as_str().ok_or("Missing saved resource pack")?.to_owned();
        let offset=layer_core::package::manifest::decimal_u64(&location["offset"])?;
        let length=layer_core::package::manifest::decimal_u64(&record["bytes"])?;
        let crc=record["crc32"].as_str().ok_or("Missing saved resource checksum")?.to_owned();
        let backing=match aliases.entry((pack,offset,length,crc)) {
            std::collections::btree_map::Entry::Occupied(entry)=>entry.get().clone(),
            std::collections::btree_map::Entry::Vacant(entry)=>{
                let source=ReadCache::process().source(length,Arc::new(ReleasedReader {key:key.into(),resource:id.to_string(),_lease:lease.clone()}));
                entry.insert(ImmutableBacking::new(Arc::new(source))?).clone()
            },
        };
        if result.insert(id,backing).is_some() {return Err("Duplicate saved resource identity".into());}
    }Ok(result)
}
async fn publish_ledger(store:&MetadataStore,ledger:&ImportLedger)->Result<(),String> {
    ledger.validate()?;store.publish(&serde_json::to_vec(ledger).map_err(|error|error.to_string())?).await
}
async fn copy(stores:&SessionStores,record:&ImportRecord,expected:Option<PortableId>,checkpoint:&Checkpoint,lease:Arc<dyn Send+Sync>)->Result<PortableId,ImportFailure> {
    checkpoint.verified(&record.source.key)?;
    let resources=resources(&record.source.key,checkpoint,lease).map_err(ImportFailure::Invalid)?;
    match stores.import_legacy(&record.drawing,expected,Arc::from(checkpoint.metadata.as_bytes()),resources).map_err(ImportFailure::Io)?.await.map_err(ImportFailure::Io)? {
        StoreResult::Imported(result)=>result,
        _=>Err(ImportFailure::Io("Invalid import completion".into())),
    }
}
enum Destination {Retired,Copy(Option<PortableId>)}
async fn destination_generation(record:&ImportRecord)->Result<Option<(Destination,Arc<dyn layer_core::package::store_io::FileTransport>)>,String> {
    let Some(io)=store_transport::transport(&format!("drawings/{}",record.drawing)).await? else {return Ok(None);};
    if layer_core::package::session_store::store_retired(&*io).await? {return Ok(Some((Destination::Retired,io)));}
    let generation=MetadataStore::new(io.clone(),ProjectLimits::for_import().metadata_bytes).generation_id().await.unwrap_or(None);
    Ok(Some((Destination::Copy(generation),io)))
}
async fn stage_drawing(record:&ImportRecord,source:&SessionManifest,roots:&mut std::collections::BTreeSet<String>)->Result<Option<(MetadataStore,SessionManifest)>,String> {
    let Some(io)=store_transport::transport(&format!("windows/{}",record.window)).await? else {return Ok(None);};
    let window=MetadataStore::new(io,1024*1024);
    let id=source.drawings.iter().find(|drawing|drawing.key==record.source.key).ok_or("Missing imported drawing membership")?.id;
    let bindings=[(id,record.drawing.clone())].into_iter().collect();
    let previous=SessionManifest::read_index(&window).await?.unwrap_or_default();
    let staged=merge_window(&previous,source,&bindings)?;staged.publish_index(&window).await?;
    roots.insert(record.drawing.clone());Ok(Some((window,staged)))
}
async fn import_drawing(stores:&SessionStores,index:&MetadataStore,ledger:&mut ImportLedger,roots:&mut std::collections::BTreeSet<String>,origin:&str,source_manifest:&SessionManifest,key:&str)->Result<Option<String>,String> {
    let Some(lease)=store_transport::legacy_lock("drawing",key).await? else {return Ok(None);};
    let value=store_transport::call("legacy-snapshot",serde_json::json!({"key":key}),None).await?;
    if value.is_undefined() || value.is_null() {return Err("The saved drawing is missing".into());}
    let snapshot:Snapshot=serde_wasm_bindgen::from_value(value.clone()).map_err(|error|error.to_string())?;
    let version=snapshot.current.version(key)?;
    let (current,_existing)=match ledger.records.get(key) {
        Some(previous)=>match destination_generation(previous).await? {
            Some((Destination::Copy(generation),lease))=>(generation,Some(lease)),
            Some((Destination::Retired,_)) if previous.source==version=>return Ok(None),
            Some((Destination::Retired,lease))=>(None,Some(lease)),None=>return Ok(None),
        },
        None=>(None,None),
    };
    if let Some(previous)=ledger.records.get(key) && previous.complete&&previous.source==version {
        if current.is_some() {
            if !roots.contains(&previous.drawing) {stage_drawing(previous,source_manifest,roots).await?;}
            return Ok(None);
        }
        ledger.retry_missing(key)?;
    }
    let window=ledger.window(origin)?;
    let Some(record)=ledger.plan(version,current,window)? else {return Ok(None);};
    let Some(_destination)=store_transport::transport(&format!("drawings/{}",record.drawing)).await? else {return Ok(None);};
    ledger.stage(record.clone());publish_ledger(index,ledger).await?;
    let Some((window,staged))=stage_drawing(&record,source_manifest,roots).await? else {return Ok(None);};
    let handles=js_sys::Reflect::get(&value,&js("handles")).map_err(error)?;store_transport::remember_handles(handles).await?;
    let mut baseline=record.baseline;
    if baseline.is_none() && let Some(previous)=&snapshot.previous {
        match copy(stores,&record,baseline,previous,lease.clone()).await {
            Ok(generation)=>{baseline=Some(generation);ledger.published(key,generation,false)?;publish_ledger(index,ledger).await?;},
            Err(ImportFailure::Invalid(_))=>{},
            Err(error)=>return Err(error.message()),
        }
    }
    match copy(stores,&record,baseline,&snapshot.current,lease).await {
        Ok(generation)=>{
            ledger.published(key,generation,true)?;publish_ledger(index,ledger).await?;
            let clean=staged.reconcile(staged.drawings.clone(),staged.active,source_manifest.clean_exit&&staged.blocked.is_empty())?;clean.publish_index(&window).await?;Ok(None)
        },
        Err(ImportFailure::Invalid(detail)) if baseline.is_some()=>{
            ledger.published(key,baseline.unwrap(),true)?;publish_ledger(index,ledger).await?;Ok(Some(detail))
        },
        Err(ImportFailure::Changed)=>Err(ImportFailure::Changed.message()),
        Err(error)=>{
            let failed=block_copy(&staged,&record.drawing)?;failed.publish_index(&window).await?;
            Err(error.message())
        },
    }
}
async fn import_sources(stores:&SessionStores,only:Option<&str>)->Result<Vec<String>,String> {
    let Some(io)=store_transport::transport("imports").await? else {return Ok(Vec::new());};
    let index=MetadataStore::new(io,ProjectLimits::for_import().metadata_bytes);
    let mut ledger:ImportLedger=match index.read().await? {Some(bytes)=>serde_json::from_slice(&bytes).map_err(|error|error.to_string())?,None=>Default::default()};ledger.validate()?;
    let names:Vec<String>=serde_wasm_bindgen::from_value(store_transport::call("legacy-windows",serde_json::Value::Null,None).await?).map_err(|error|error.to_string())?;
    let mut warnings=Vec::new();
    let mut roots=std::collections::BTreeSet::new();
    for key in store_transport::directories("windows").await? {
        let Some(io)=store_transport::transport(&format!("windows/{key}")).await? else {continue;};
        if let Ok(Some(manifest))=SessionManifest::read_index(&MetadataStore::new(io,1024*1024)).await {roots.extend(manifest.drawings.into_iter().map(|drawing|drawing.key));}
    }
    for origin in names {
        let Some(_lease)=store_transport::legacy_lock("window",&origin).await? else {continue;};
        let value=store_transport::call("legacy-manifest",serde_json::json!({"key":origin}),None).await?;
        if value.is_undefined() || value.is_null() {continue;}
        let source:SessionManifest=match serde_wasm_bindgen::from_value::<SessionManifest>(value).map_err(|error|error.to_string()).and_then(|source|source.validate().map(|()|source)) {
            Ok(source)=>source,Err(error)=>{warnings.push(error);continue;},
        };
        for drawing in source.drawings.iter().filter(|drawing|only.is_none_or(|only|drawing.key==only)) {
            match import_drawing(stores,&index,&mut ledger,&mut roots,&origin,&source,&drawing.key).await {
                Ok(Some(warning))|Err(warning)=>warnings.push(warning),Ok(None)=>{},
            }
        }
    }Ok(warnings)
}
pub(super) async fn import_all(stores:&SessionStores)->Result<Vec<String>,String> {import_sources(stores,None).await}
pub(super) async fn ensure_copy(stores:&SessionStores,key:&str)->Result<Option<String>,String> {
    let Some(io)=store_transport::transport("imports").await? else {return Ok(None);};
    let index=MetadataStore::new(io,ProjectLimits::for_import().metadata_bytes);
    let ledger:ImportLedger=match index.read().await? {Some(bytes)=>serde_json::from_slice(&bytes).map_err(|error|error.to_string())?,None=>return Ok(Some(key.into()))};ledger.validate()?;
    let pending=ledger.records.values().find(|record|record.drawing==key&&!record.complete).map(|record|record.source.key.clone());
    drop(index);
    if let Some(source)=pending {
        let warnings=import_sources(stores,Some(&source)).await?;
        if let Some(error)=warnings.first() {return Err(error.clone());}
        let Some(io)=store_transport::transport("imports").await? else {return Ok(None);};
        let index=MetadataStore::new(io,ProjectLimits::for_import().metadata_bytes);
        let ledger:ImportLedger=serde_json::from_slice(&index.read().await?.ok_or("Missing imported session index")?).map_err(|error|error.to_string())?;
        return Ok(ledger.records.get(&source).filter(|record|record.complete).map(|record|record.drawing.clone()));
    }
    Ok(Some(key.into()))
}
