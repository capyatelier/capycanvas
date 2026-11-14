#[cfg(test)]
use super::archive::InputMember;
use super::{archive::{self, Directory}, manifest::{Manifest, ManifestLimits, ManifestRead},
    resources::{self, ResourceInventory, PreparedResources}, preview::{Preview, MAX_PREVIEW_BYTES, MAX_PREVIEW_DIMENSION}, ImmutableBacking};
use crate::authored::{Artwork, ArtworkCapture, CaptureCheckpoint, EvaluationContext, PortableId, Support};
use serde_json::{Value, json};
use std::{io::{Cursor, Write}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Debug)]
pub struct CapturedPreview { pub checkpoint: CaptureCheckpoint, pub context: EvaluationContext, pub preview: Preview }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewStatus { Included, Unavailable, Stale }
#[derive(Debug)]
pub struct PreparedPackage {
    pub checkpoint: CaptureCheckpoint,
    pub preview_status: PreviewStatus,
    manifest: Arc<[u8]>, resources: PreparedResources, preview: Option<Preview>,
}
#[derive(Clone, Debug)]
pub struct OutputInfo { pub id: PortableId, pub name: Arc<str> }
#[derive(Debug)]
pub enum OpenOutcome {
    Candidate { artwork: Artwork, source: ImmutableBacking, preview: Option<Preview>, memory: crate::DocumentMemory },
    Preserved { source: ImmutableBacking, outputs: Vec<OutputInfo>, preview: Option<Preview>, reason: String },
    RecoveredView { source: ImmutableBacking, preview: Preview, reason: String, cause: Option<PackageCause> },
    Failure { source: ImmutableBacking, reason: String, cause: Option<PackageCause> },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageCause { Unsupported, Damaged }
#[derive(serde::Serialize)]
struct ManifestWire<'a> {
    default_output: Value,
    document: PortableId,
    format: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<&'a Value>,
    objects: &'a [Value],
    outputs: &'a [Value],
    resources: ResourceRecords<'a>,
    root: Value,
    version: u32,
}
struct ResourceRecords<'a>(&'a [resources::PreparedResource]);
impl serde::Serialize for ResourceRecords<'_> {
    fn serialize<S:serde::Serializer>(&self, serializer:S) -> Result<S::Ok,S::Error> {
        serializer.collect_seq(self.0.iter().map(|entry|&entry.record))
    }
}
fn active(cancelled:&AtomicBool) -> Result<(),String> {
    if cancelled.load(Ordering::Relaxed) {Err("Package operation cancelled".into())} else {Ok(())}
}
fn metadata(artwork:&Artwork,resources:&mut ResourceInventory) -> Result<Option<Value>,String> {
    artwork.metadata.validate()?;
    let mut fields=serde_json::Map::new();
    for (kind,bytes) in ["exif","xmp","iptc"].into_iter().zip(artwork.metadata.blocks()) {
        if let Some(bytes)=bytes {fields.insert(kind.into(),resources.bytes("capy.photo-metadata/1",bytes,json!({"kind":kind}))?);}
    }
    Ok((!fields.is_empty()).then_some(Value::Object(fields)))
}
pub(crate) fn admit_metadata(artwork:&Artwork,limits:crate::ProjectLimits)->Result<(),String> {
    if artwork.paint.iter().any(|(_,_,p)|!p.operations.is_empty()) || artwork.coverage.iter().any(|(_,_,p)|!p.operations.is_empty()) {return Ok(());}
    for raster in artwork.paint.iter().map(|(_,_,p)|&p.raster).chain(artwork.coverage.iter().map(|(_,_,p)|&p.raster)) {
        let Some(data)=raster.try_data() else {return Ok(())};
        if data?.tiles.values().any(|tile|tile.try_backing().is_none()) {return Ok(());}
    }
    let (mut objects,mut inventory)=super::artwork_records::encode_with_inventory(artwork,&AtomicBool::new(false),ResourceInventory::for_transfer())?;
    let metadata=metadata(artwork,&mut inventory)?;
    let live=objects.iter().filter_map(|r|r["id"].as_str()?.parse().ok()).collect();
    let (extensions,opaque)=artwork.extensions.edited_retained(&live)?;
    objects.extend(extensions);
    let opaque=inventory.with_opaque(opaque)?;
    let resources:Vec<_>=inventory.entries.iter().map(|(id,entry)| {
        let mut data=entry.data.clone();
        if matches!(entry.payload,resources::Payload::Bytes(_)|resources::Payload::Code(_)) {data["decoded_bytes"]=u64::MAX.to_string().into();}
        json!({"id":id,"type":entry.kind,"data":data,"encoding":"capy.lz4-bytes/1",
            "location":{"pack":"data/tiles-1.bin","offset":u64::MAX.to_string()},"bytes":u64::MAX.to_string(),"crc32":"ffffffff"})
    }).chain(opaque.iter().map(|resource|resource.record(json!({"pack":"data/tiles-1.bin","offset":u64::MAX.to_string()})))).collect();
    let default=artwork.outputs.id(artwork.default_output).ok_or("Missing default output")?;
    if let Some(output)=objects.iter_mut().find(|record|record["id"]==json!(default)) {
        output["data"]["representation"]=json!({"member":"preview.png","size":[MAX_PREVIEW_DIMENSION,MAX_PREVIEW_DIMENSION],"color":"srgb"});
    }
    let mut value=json!({"default_output":resources::reference(default),"document":artwork.id,"format":"capy.canvas",
        "objects":objects,"outputs":artwork.outputs.iter().map(|(_,id,_)|resources::reference(id)).collect::<Vec<_>>(),
        "resources":resources,"root":resources::reference(artwork.compositions.id(artwork.root).ok_or("Missing root")?),"version":1});
    if let Some(metadata)=metadata {value["metadata"]=metadata;}
    let metadata_limit=limits.metadata_bytes.min(crate::ProjectLimits::default().metadata_bytes) as usize;
    if crate::json_len(&value)>metadata_limit {return Err("Artwork metadata exceeds package limit".into());}
    Manifest::admit(&value,ManifestLimits {metadata_bytes:metadata_limit,..Default::default()})?;
    Ok(())
}
impl PreparedPackage {
    pub fn prepare(capture:&ArtworkCapture,preview:Option<CapturedPreview>,cancelled:&AtomicBool)->Result<Self,String> {archive::ready(Self::prepare_using::<resources::ReadyResources>(capture,preview,cancelled))}
    pub async fn prepare_async(capture:&ArtworkCapture,preview:Option<CapturedPreview>,cancelled:&AtomicBool)->Result<Self,String> {Self::prepare_using::<resources::LeasedResources>(capture,preview,cancelled).await}
    async fn prepare_using<I:resources::ResourceInput>(capture:&ArtworkCapture, preview:Option<CapturedPreview>, cancelled:&AtomicBool) -> Result<Self,String> {
        active(cancelled)?;
        if capture.checkpoint.document!=capture.artwork.id {return Err("Capture belongs to a different drawing".into());}
        let artwork=&capture.artwork;
        if artwork.paint.iter().any(|(_,_,s)|!s.operations.is_empty())||artwork.coverage.iter().any(|(_,_,s)|!s.operations.is_empty()){return Err("Wait for the current edit before saving".into());}
        let shape=artwork.topology()?;
        let (mut objects,mut inventory)=super::artwork_records::encode(artwork,cancelled)?;
        crate::Document::from_artwork((**artwork).clone()).map_err(|e|e.to_string())?.admit(Default::default())?;
        for record in &objects {
            if super::registry::descriptor(record["type"].as_str().ok_or("Missing authored type")?).is_some() {
                super::registry::validate_context(record,super::registry::RecordContext::Portable)?;
            }
        }
        let metadata=metadata(artwork,&mut inventory)?;
        let root=artwork.compositions.id(artwork.root).ok_or("Missing authored root")?;
        let default=artwork.outputs.id(artwork.default_output).ok_or("Missing default output")?;
        let outputs:Vec<_>=artwork.outputs.iter().map(|(_,id,_)|resources::reference(id)).collect();
        let (preview,preview_status)=match preview {
            Some(preview) if preview.checkpoint==capture.checkpoint
                && artwork.outputs.get(artwork.default_output).is_some_and(|output|output.context==preview.context)=>(Some(preview.preview),PreviewStatus::Included),
            Some(_)=>(None,PreviewStatus::Stale),None=>(None,PreviewStatus::Unavailable),
        };
        if let Some(preview)=&preview {
            let output=objects.iter_mut().find(|record|record["id"]==json!(default)).ok_or("Missing captured output")?;
            output["data"]["representation"]=json!({"member":"preview.png","size":preview.size(),"color":"srgb"});
        }
        let live=objects.iter().map(|record|record["id"].as_str().ok_or("Missing object identity")?.parse().map_err(str::to_string)).collect::<Result<_,String>>()?;
        let (extensions,opaque)=artwork.extensions.edited_retained(&live)?;
        objects.extend(extensions);
        objects.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));
        if inventory.entries.keys().any(|id|*id==artwork.id || shape.objects.contains_key(id)) {return Err("Object and resource identities overlap".into());}
        let opaque=inventory.with_opaque(opaque)?;
        let mut resources=inventory.prepare_using::<I>(cancelled).await?;
        resources.append_opaque_using::<I>(opaque,cancelled).await?;
        let manifest=serde_json::to_vec(&ManifestWire {
            default_output:resources::reference(default),document:artwork.id,format:"capy.canvas",metadata:metadata.as_ref(),
            objects:&objects,outputs:&outputs,resources:ResourceRecords(&resources.entries),root:resources::reference(root),version:1,
        }).map_err(|e|e.to_string())?;
        if manifest.len()>crate::ProjectLimits::default().metadata_bytes as usize {return Err("Artwork metadata exceeds package limit".into());}
        Manifest::admit(&super::parse_json(&manifest,ManifestLimits::default().metadata_bytes)?,ManifestLimits::default())?;
        Ok(Self {checkpoint:capture.checkpoint,preview_status,manifest:manifest.into(),resources,preview})
    }
    pub fn manifest(&self) -> &[u8] {&self.manifest}
    pub fn resources(&self) -> &PreparedResources {&self.resources}
    pub fn write(&self,output:&mut impl Write,cancelled:&AtomicBool)->Result<(),String> {
        let mut pack=self.resources.reader(cancelled);let mut pack=archive::ReadyReader(&mut pack);
        archive::ready(self.write_using(output,cancelled,&mut pack))
    }
    pub async fn write_async(&self,output:&mut impl Write,cancelled:&AtomicBool)->Result<(),String> {
        let mut pack=self.resources.reader(cancelled);self.write_using(output,cancelled,&mut pack).await
    }
    async fn write_using(&self,output:&mut impl Write,cancelled:&AtomicBool,pack:&mut dyn archive::ReadCursor)->Result<(),String> {
        active(cancelled)?;
        let mut mime=Cursor::new(archive::MIMETYPE);let mut mime=archive::ReadyReader(&mut mime);
        let mut manifest=crate::Cancellable {inner:Cursor::new(&*self.manifest),cancelled:||cancelled.load(Ordering::Relaxed)};let mut manifest=archive::ReadyReader(&mut manifest);
        let mut preview=Cursor::new(self.preview.as_ref().map(|p|p.encoded().as_ref()).unwrap_or(&[]));let mut preview=archive::ReadyReader(&mut preview);
        let mut members=vec![archive::CursorMember {name:"mimetype",length:archive::MIMETYPE.len() as u64,crc32:crc32fast::hash(archive::MIMETYPE),input:&mut mime},
            archive::CursorMember {name:"manifest.json",length:self.manifest.len() as u64,crc32:crc32fast::hash(&self.manifest),input:&mut manifest}];
        if !self.resources.entries.is_empty() {members.push(archive::CursorMember {name:"data/tiles-1.bin",length:self.resources.length,crc32:self.resources.crc,input:pack});}
        if let Some(p)=&self.preview {members.push(archive::CursorMember {name:"preview.png",length:p.encoded().len() as u64,crc32:crc32fast::hash(p.encoded()),input:&mut preview});}
        let mut writer=crate::Cancellable {inner:output,cancelled:||cancelled.load(Ordering::Relaxed)};
        archive::write_archive_async(&mut writer,&mut members,usize::try_from(crate::ProjectLimits::default().metadata_bytes).map_err(|_|"Package metadata limit exceeds address space")?).await
    }
}

fn failed(source:ImmutableBacking,preview:Option<Preview>,error:super::values::DecodeError) -> OpenOutcome {
    let cause = match &error {
        super::values::DecodeError::Unsupported(_) => Some(PackageCause::Unsupported),
        super::values::DecodeError::Invalid(_) => Some(PackageCause::Damaged),
        super::values::DecodeError::Io(_) | super::values::DecodeError::MemoryBudgetExceeded | super::values::DecodeError::Uncovered => None,
    };
    let reason = error.to_string();
    match preview {Some(preview)=>OpenOutcome::RecoveredView {source,preview,reason,cause},None=>OpenOutcome::Failure {source,reason,cause}}
}
fn output_inventory(manifest:&Manifest) -> Vec<OutputInfo> {
    manifest.outputs.iter().map(|id|OutputInfo {id:*id,name:manifest.objects.get(id).and_then(|record|record["data"]["name"].as_str()).unwrap_or("").into()}).collect()
}
fn matching_preview(manifest:&Manifest,preview:Option<Preview>) -> Option<Preview> {
    let preview=preview?;
    let record=manifest.objects.get(&manifest.default_output?)?;
    let representation=&record["data"]["representation"];
    (representation==&json!({"member":"preview.png","size":preview.size(),"color":"srgb"})).then_some(preview)
}
fn limited(source:ImmutableBacking,value:Value,preview:Option<Preview>,reason:String)->OpenOutcome {
    let mut outputs=Vec::new();
    let mut matched=None;
    if let (Some(records),Some(ids))=(value["objects"].as_array(),value["outputs"].as_array()) {
        let mut wanted=ids.iter().take(ManifestLimits::default().graph.objects).filter_map(|id|resources::reference_id(id).ok()).collect::<std::collections::BTreeSet<_>>();
        for record in records {
            let Some(id)=record["id"].as_str().and_then(|id|id.parse().ok()) else {continue};
            if record["type"]=="capy.output/2" && wanted.remove(&id) {
                outputs.push(OutputInfo {id,name:record["data"]["name"].as_str().unwrap_or("").into()});
                if resources::reference_id(&value["default_output"]).ok()==Some(id)
                    && preview.as_ref().is_some_and(|p|record["data"]["representation"]==json!({"member":"preview.png","size":p.size(),"color":"srgb"})) {matched=preview.clone();}
            }
        }
    }
    OpenOutcome::Preserved {source,outputs,preview:matched,reason}
}
pub fn open(source:ImmutableBacking,limits:crate::ProjectLimits,cancelled:&AtomicBool) -> Result<OpenOutcome,String> {
    open_with_budget(source, limits, cancelled, crate::MemoryBudget::process())
}
pub fn open_with_budget(source:ImmutableBacking,limits:crate::ProjectLimits,cancelled:&AtomicBool,budget:crate::MemoryBudget) -> Result<OpenOutcome,String> {
    #[cfg(not(target_arch="wasm32"))]
    {pollster::block_on(open_async_with_budget(source,limits,cancelled,budget))}
    #[cfg(target_arch="wasm32")]
    {
        let mut future=std::pin::pin!(open_decoded(source,limits,cancelled,budget,false));
        match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
            std::task::Poll::Ready(result)=>result,std::task::Poll::Pending=>Err("Use asynchronous package metadata opening for pending sources".into()),
        }
    }
}
pub async fn open_async(source:ImmutableBacking,limits:crate::ProjectLimits,cancelled:&AtomicBool)->Result<OpenOutcome,String> {
    open_async_with_budget(source,limits,cancelled,crate::MemoryBudget::process()).await
}
pub async fn open_async_with_budget(source:ImmutableBacking,limits:crate::ProjectLimits,cancelled:&AtomicBool,budget:crate::MemoryBudget)->Result<OpenOutcome,String> {
    open_decoded(source,limits,cancelled,budget,true).await
}
async fn open_decoded(source:ImmutableBacking,limits:crate::ProjectLimits,cancelled:&AtomicBool,budget:crate::MemoryBudget,waits:bool)->Result<OpenOutcome,String> {
    active(cancelled)?;
    let directory=match Directory::read_async(&source,cancelled,262_144,limits.metadata_bytes).await {
        Ok(directory)=>directory,
        Err(super::values::DecodeError::Unsupported(reason))=>{active(cancelled)?;return Ok(OpenOutcome::Preserved {source,outputs:Vec::new(),preview:None,reason});},
        Err(error)=>{active(cancelled)?;return Ok(failed(source,None,error));}
    };
    let preview=if let Some(member)=directory.member("preview.png") {
        directory.read_member_async(&source,cancelled,member,MAX_PREVIEW_BYTES).await.ok().and_then(|bytes|Preview::decode(bytes.into()).ok())
    }else {None};
    let bytes=match directory.read_member_async(&source,cancelled,directory.member("manifest.json").unwrap(),limits.metadata_bytes.min(usize::MAX as u64) as usize).await {
        Ok(bytes)=>bytes,Err(reason)=>{active(cancelled)?;return Ok(failed(source,preview,reason));}
    };
    let manifest=match Manifest::parse(&bytes,&directory,ManifestLimits {metadata_bytes:limits.metadata_bytes.min(usize::MAX as u64) as usize,..Default::default()}) {
        Ok(ManifestRead::Known(manifest))=>manifest,
        Ok(ManifestRead::UnsupportedEnvelope(_))=>return Ok(OpenOutcome::Preserved {source,outputs:Vec::new(),preview,reason:"Unsupported package envelope".into()}),
        Ok(ManifestRead::Limited {value,reason})=>return Ok(limited(source,value,preview,reason)),
        Err(super::values::DecodeError::Unsupported(reason))=>return Ok(OpenOutcome::Preserved {source,outputs:Vec::new(),preview:None,reason}),
        Err(error)=>return Ok(failed(source,preview,error)),
    };
    let indexed=manifest.resources.values().filter_map(|resource|resource.range.map(|range|range.member)).collect::<std::collections::BTreeSet<_>>();
    if directory.members.iter().enumerate().any(|(index,member)|member.name.starts_with("data/") && !indexed.contains(&index)) {
        return Ok(failed(source,preview,"Unindexed data member".into()));
    }
    let mut resources=resources::ResourceReader::with_budget(&manifest,&source,cancelled,limits,budget.clone());resources.waits=waits;
    let decoded=match resources.prepare_metadata().await {Ok(())=>loop {
        match super::artwork_records::decode(&manifest,&mut resources) {Err(super::values::DecodeError::Uncovered)=>resources.covering().await,decoded=>break decoded}
    },Err(error)=>Err(error)};
    active(cancelled)?;
    match decoded {
        Ok(mut artwork)=>{
            if let Support::Preserved(reasons)=&manifest.support {return Ok(OpenOutcome::Preserved {source,outputs:output_inventory(&manifest),preview:matching_preview(&manifest,preview),reason:reasons.iter().copied().collect::<Vec<_>>().join("; ")});}
            let document=crate::Document::from_artwork(artwork.clone());
            match document {
                Err(reason)=>return Ok(failed(source,preview,reason.to_string().into())),
                Ok(document)=>if let Err(reason)=document.admit(limits) {if reason==crate::MEMORY_BUDGET_EXCEEDED{return Err(reason);}return Ok(OpenOutcome::Preserved {source,outputs:output_inventory(&manifest),preview:matching_preview(&manifest,preview),reason});},
            }
            artwork.extensions=match crate::authored::Extensions::load_async(&manifest,&source,cancelled).await {Ok(extensions)=>Arc::new(extensions),Err(reason)=>{active(cancelled)?;return Ok(failed(source,preview,reason));}};
            let memory=crate::DocumentMemory::new(budget,resources.admission.clone());
            Ok(OpenOutcome::Candidate {artwork,source,preview:matching_preview(&manifest,preview),memory})
        }
        Err(super::values::DecodeError::MemoryBudgetExceeded | super::values::DecodeError::Uncovered)=>Err(crate::MEMORY_BUDGET_EXCEEDED.into()),
        Err(super::values::DecodeError::Unsupported(reason))=>Ok(OpenOutcome::Preserved {source,outputs:output_inventory(&manifest),preview:matching_preview(&manifest,preview),reason}),
        Err(error)=>Ok(failed(source,preview,error)),
    }
}

pub fn copy_original(source:&ImmutableBacking,output:&mut impl Write,cancelled:&AtomicBool) -> Result<(),String> {
    let mut offset=0;
    active(cancelled)?;
    while offset<source.byte_len() {
        active(cancelled)?;
        let count=(source.byte_len()-offset).min(64*1024) as usize;
        let super::RangeState::Ready(bytes)=source.poll_original_on_worker(offset,count)? else {return Err("Package bytes are pending".into());};
        output.write_all(&bytes).map_err(|e|e.to_string())?;
        offset+=count as u64;
    }
    output.flush().map_err(|e|e.to_string())
}

#[cfg(test)]
mod tests;
