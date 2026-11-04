use super::*;
use crate::package::{storage_budget::{self,StorageReservation},store_io::{FileAccess,StoreFile}};
use std::sync::atomic::AtomicUsize;

pub(super) const CHUNK_BYTES:u64=4*1024*1024;
pub(super) const DEAD_BYTES:u64=16*1024*1024;
pub(super) const DEAD_TRIGGER:u64=256*1024*1024;
pub(super) const IDLE_MS:u64=2000;

#[derive(Default)]
pub(super) struct Activity {pub last:AtomicU64,pub busy:AtomicUsize,pub background:AtomicBool,epoch:AtomicU64,copying:AtomicBool,next_chunk:AtomicU64}
pub struct CompactionActivity {pub(super) state:Arc<Activity>,pub(super) busy:bool}
impl Default for CompactionActivity {
    fn default()->Self {Self {state:Self::process(),busy:false}}
}
impl CompactionActivity {
    fn process()->Arc<Activity> {static STATE:OnceLock<Arc<Activity>>=OnceLock::new();STATE.get_or_init(Default::default).clone()}
    pub fn hold()->Self {let mut activity=Self::default();activity.set_busy(true);activity}
    pub fn revision(&self)->u64 {self.state.epoch.load(Ordering::Acquire)}
    pub fn interrupt() {Self::default().touch();}
    pub fn touch(&self) {self.state.last.store(crate::MemoryBudget::now_ms(),Ordering::Release);self.state.epoch.fetch_add(1,Ordering::Release);}
    pub fn set_busy(&mut self,busy:bool) {
        if busy {self.touch();}
        if std::mem::replace(&mut self.busy,busy)!=busy {
            if busy {self.state.busy.fetch_add(1,Ordering::AcqRel);}else {self.state.busy.fetch_sub(1,Ordering::AcqRel);self.touch();}
        }
    }
    pub fn set_background(background:bool) {let state=Self::process();state.background.store(background,Ordering::Release);state.last.store(crate::MemoryBudget::now_ms(),Ordering::Release);state.epoch.fetch_add(1,Ordering::Release);}
    pub fn idle()->bool {Self::idle_at(&Self::process(),crate::MemoryBudget::now_ms())}
    pub(super) fn idle_at(state:&Activity,now:u64)->bool {state.busy.load(Ordering::Acquire)==0 && !state.background.load(Ordering::Acquire) && now.saturating_sub(state.last.load(Ordering::Acquire))>=IDLE_MS}
}
impl Drop for CompactionActivity {fn drop(&mut self) {self.set_busy(false);}}
struct ChunkLease(Arc<Activity>);
impl Drop for ChunkLease {fn drop(&mut self) {self.0.copying.store(false,Ordering::Release);}}


#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum CompactionStep {Idle,Copied(u64),Published}
struct Record {range:Arc<StoreRange>,length:u64,crc:u32,target:u64,cold:bool}
pub(super) struct Copy {
    source:PortableId,target:PortableId,file:Arc<dyn StoreFile>,hot_output:Arc<dyn StoreFile>,input:Arc<dyn StoreFile>,_output:Arc<Pack>,extent:u64,
    records:Vec<Record>,next:usize,offset:u64,checksum:crc32fast::Hasher,reservation:StorageReservation,
}
#[derive(Clone,Copy)]
pub(super) struct Candidate {pub id:PortableId,pub live:u64,pub dead:u64}
impl Candidate {
    pub(super) fn qualifies(&self)->bool {self.live>0 && self.live<self.dead && self.dead>=DEAD_BYTES}
    pub(super) fn before(&self,other:&Self)->bool {u128::from(self.dead)*u128::from(other.live)>u128::from(other.dead)*u128::from(self.live)}
}
pub(super) fn triggered(total:u64,dead:u64,available:Option<u64>)->bool {
    dead>total/4 || dead>DEAD_TRIGGER || available.is_some_and(|bytes|bytes<PACK_BYTES+MAX_RANGE_BYTES as u64)
}

impl SessionStore {
    pub(super) fn seal_compaction_output(&mut self) {
        if self.compaction.as_ref().is_some_and(|copy|self.append.is_some_and(|(id,_)|id==copy.target)) {self.append=None;}
    }
    fn compaction_candidate(&mut self)->Option<(Candidate,BTreeMap<u64,(Arc<StoreRange>,u64,u32)>)> {
        let mut identities=BTreeSet::new();
        for head in self.head.iter().chain(self.compaction_fallback.iter()) {
            for generation in std::iter::once(&head.current).chain(head.previous.as_ref()) {
                let (known,roots)=self.generation_roots.get(&generation.id)?;
                if known!=generation || !roots.resources.is_empty() || roots.identities.iter().any(|id|!self.index.contains_key(id)) {return None;}
                identities.extend(&roots.identities);
            }
        }
        self.retain_index(&identities);
        let mut live=BTreeMap::<PortableId,BTreeMap<u64,(Arc<StoreRange>,u64,u32)>>::new();
        for entry in self.index.values() {
            let range=entry.location.range();
            live.entry(range.pack.id).or_default().entry(range.offset).or_insert((range,entry.location.length,entry.location.crc));
        }
        let mut total=0u64;let mut dead=0u64;let mut candidate:Option<Candidate>=None;
        let packs=self._owner.packs.lock().unwrap();
        for pack in packs.values().filter_map(Weak::upgrade) {
            let length=pack.extent.load(Ordering::Acquire);
            let used=live.get(&pack.id).map_or(0,|ranges|ranges.values().map(|(_,length,_)|(*length).max(1)).sum());
            if used>length {return None;}
            total=total.saturating_add(length);dead=dead.saturating_add(length-used);
            let current=Candidate {id:pack.id,live:used,dead:length-used};
            if self.append.is_none_or(|(append,_)|append!=pack.id) && current.qualifies() && candidate.is_none_or(|best|current.before(&best)) {candidate=Some(current);}
        }
        let available=self.storage_scope.as_ref().and_then(|scope|storage_budget::StorageBudget::process().available_bytes(scope));
        if !triggered(total,dead,available) {return None;}
        let candidate=candidate?;Some((candidate,live.remove(&candidate.id)?))
    }
    async fn index_compaction_generations(&mut self)->Result<(),String> {
        let generations=self.head.iter().chain(self.compaction_fallback.iter())
            .flat_map(|head|std::iter::once(&head.current).chain(head.previous.as_ref())).cloned().collect::<Vec<_>>();
        for generation in generations {
            if self.generation_roots.get(&generation.id).is_some_and(|(known,roots)|*known==generation
                && (!roots.resources.is_empty() || roots.identities.iter().all(|id|self.index.contains_key(id)))) {continue;}
            let metadata=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes).await?;
            let stored=StoredGeneration::parse(&metadata,ProjectLimits::default()).map_err(|error|error.message())?;
            let mut roots=GenerationRoots::default();
            if let Some(locations)=&stored.locations {
                self.index_generation_parts(&stored,&AtomicBool::new(false)).await.map_err(|error|error.message())?;
                roots.identities.extend(locations.keys());roots.packs.extend(stored.extents.keys());
            }else {
                roots.resources.extend(resource_index(&stored.metadata,ProjectLimits::default()).map_err(|error|error.message())?.records.into_iter().map(|record|record.id));
                roots.identities.extend(&roots.resources);
            }
            self.generation_roots.insert(generation.id,(generation,roots));
        }Ok(())
    }
    async fn begin_compaction(&mut self)->Result<Option<Copy>,String> {
        self.index_compaction_generations().await?;
        let Some((candidate,ranges))=self.compaction_candidate() else {return Ok(None);};
        self.refresh_head().await?;self.collect_garbage().await?;
        let reservation=storage_budget::reserve(&*self._owner.io,candidate.live).await?;
        let mut target=None;
        if let Some((id,_))=self.append {
            let file=self._owner.io.open_copy(&format!("packs/{id}.bin"),FileAccess::Write).await?;
            let length=file.len().await?;
            if length.checked_add(candidate.live).is_some_and(|end|end<=PACK_BYTES) {target=Some((id,length,file));}
        }
        let (target,start,file)=match target {
            Some(target)=>target,
            None=>{let id=PortableId::random();let file=self._owner.io.open_copy(&format!("packs/{id}.bin"),FileAccess::Create).await?;(id,0,file)},
        };
        let mut extent=start;let mut records=Vec::new();
        for (_, (range,length,crc)) in ranges {
            let cold=!range.source.cached(range.offset,length);
            records.push(Record {range,length,crc,target:extent,cold});extent+=length.max(1);
        }
        let input=self._owner.io.open_copy(&format!("packs/{}.bin",candidate.id),FileAccess::Read).await?;
        let hot_output=self._owner.io.open(&format!("packs/{target}.bin"),FileAccess::Write).await?;
        let output=self.pack(target,start).await?;
        Ok(Some(Copy {source:candidate.id,target,file,hot_output,input,_output:output,extent,records,next:0,offset:0,checksum:crc32fast::Hasher::new(),reservation}))
    }
    pub async fn compact_step(&mut self)->Result<CompactionStep,String> {
        let activity=CompactionActivity::default();let epoch=activity.revision();
        if !CompactionActivity::idle() {return Ok(CompactionStep::Idle);}
        let state=CompactionActivity::process();
        if state.copying.compare_exchange(false,true,Ordering::AcqRel,Ordering::Acquire).is_err() {return Ok(CompactionStep::Idle);}
        let permit=ChunkLease(state);let now=crate::MemoryBudget::now_ms();
        if now<permit.0.next_chunk.load(Ordering::Acquire) {return Ok(CompactionStep::Idle);}
        let result=self.compact_ready_step(&activity,epoch).await;
        if !matches!(result,Ok(CompactionStep::Idle)) {permit.0.next_chunk.store(crate::MemoryBudget::now_ms().saturating_add(IDLE_MS),Ordering::Release);}
        result
    }
    pub(super) async fn compact_ready_step(&mut self,activity:&CompactionActivity,epoch:u64)->Result<CompactionStep,String> {
        if activity.revision()!=epoch {return Ok(CompactionStep::Idle);}
        let released=self._owner.packs.lock().map_err(|_|"Recovery pack lock failed")?.values().any(|pack|pack.strong_count()==0);
        if released {self.collect_garbage().await?;}
        if let Some(refusal)=&self.compaction_refusal {
            let facts=self.storage_scope.as_ref().and_then(|scope|storage_budget::StorageBudget::process().facts(scope));
            if facts.as_ref().is_none_or(|facts|!refusal.allows(facts)) {return Ok(CompactionStep::Idle);}self.compaction_refusal=None;
        }
        let mut copy=match self.compaction.take() {Some(copy)=>copy,None=>match self.begin_compaction().await {
            Ok(Some(copy))=>copy,Ok(None)=>return Ok(CompactionStep::Idle),Err(error)=>{self.refuse_compaction(&error).await;return Err(error);},
        }};
        let result:Result<CompactionStep,String>=async {
            if activity.revision()!=epoch {return Ok(CompactionStep::Idle);}
            let mut copied=0;
            while copied<CHUNK_BYTES && let Some(record)=copy.records.get(copy.next) {
                let count=(record.length-copy.offset).min(CHUNK_BYTES-copied) as usize;
                let input=if record.cold {&copy.input}else {&record.range.pack.file};
                let bytes=if record.cold {input.read_cold_at(record.range.offset+copy.offset,count).await?}else {input.read_at(record.range.offset+copy.offset,count).await?};
                if bytes.len()!=count {return Err("Incomplete compaction read".into());}
                copy.checksum.update(&bytes);
                let file=if record.cold {&copy.file}else {&copy.hot_output};
                let written=if count==0 {file.write_at(record.target,&[0]).await}else {file.write_at(record.target+copy.offset,&bytes).await};
                copy.reservation.written((count as u64).max(1))?;written?;
                if record.cold {copy.input.discard_cold(record.range.offset+copy.offset,count as u64).await?;}
                copy.offset+=count as u64;copied+=(count as u64).max(1);
                if copy.offset==record.length {
                    if std::mem::replace(&mut copy.checksum,crc32fast::Hasher::new()).finalize()!=record.crc {return Err(RESOURCE_CHECKSUM_FAILED.into());}
                    copy.next+=1;copy.offset=0;
                }
                if activity.revision()!=epoch {break;}
            }
            if copied>0 {return Ok(CompactionStep::Copied(copied));}
            Ok(if self.publish_compaction(&copy,activity,epoch).await? {CompactionStep::Published}else {CompactionStep::Idle})
        }.await;
        match &result {
            Ok(CompactionStep::Published)=>{drop(copy);self.collect_garbage().await?;},
            Ok(_)=>self.compaction=Some(copy),
            Err(error)=>{
                if self.append.is_some_and(|(id,_)|id==copy.target) {self.append=None;}
                drop(copy);let cleanup=self.collect_garbage().await;self.refuse_compaction(error).await;
                if let Err(cleanup)=cleanup {return Err(format!("{error}; {cleanup}"));}
            },
        }
        result
    }
    async fn refuse_compaction(&mut self,error:&str) {
        if error==storage_budget::STORAGE_UNAVAILABLE {
            let epoch=storage_budget::StorageBudget::process().event_epoch();
            let facts=self._owner.io.storage_facts().await.ok();
            if let Some(facts)=&facts {self.storage_scope=Some(facts.scope.clone());storage_budget::StorageBudget::process().publish(facts.clone());}
            self.compaction_refusal=Some(storage_budget::StorageRefusal {facts,epoch});
        }
    }
    async fn publish_compaction(&mut self,copy:&Copy,activity:&CompactionActivity,epoch:u64)->Result<bool,String> {
        if activity.revision()!=epoch {return Ok(false);}
        copy.file.flush().await?;
        for record in &copy.records {if record.cold {copy.file.discard_cold(record.target,record.length).await?;}}
        self.refresh_head().await?;
        if activity.revision()!=epoch {return Ok(false);}
        let targets=copy.records.iter().map(|record|(record.range.offset,record.target)).collect::<BTreeMap<_,_>>();
        let mut generations=BTreeMap::new();let mut remapped=Vec::new();
        if let Some(head)=self.head.clone() {
            for generation in std::iter::once(head.current).chain(head.previous) {
                let bytes=self.generation_metadata(&generation,ProjectLimits::default().metadata_bytes).await?;
                let mut packed:PackedGeneration=serde_json::from_slice(&bytes).map_err(|error|error.to_string())?;
                if !packed.extents.contains_key(&copy.source) {generations.insert(generation.id,generation);continue;}
                for record in packed.session["resources"].as_array_mut().ok_or("Missing recovery resource inventory")? {
                    if record["location"]["pack"]==format!("packs/{}.bin",copy.source) {
                        let offset=super::super::manifest::decimal_u64(&record["location"]["offset"])?;
                        let offset=targets.get(&offset).ok_or("Compaction omitted a published resource")?;
                        record["location"]=serde_json::json!({"pack":format!("packs/{}.bin",copy.target),"offset":offset.to_string()});
                    }
                }
                packed.extents.remove(&copy.source);packed.extents.insert(copy.target,copy.extent);
                let bytes=serde_json::to_vec(&packed).map_err(|error|error.to_string())?;
                let replacement=Generation::new(PortableId::random(),&bytes);generations.insert(generation.id,replacement.clone());remapped.push((generation,replacement,bytes));
            }
        }
        if activity.revision()!=epoch {return Ok(false);}
        let target=self.pack(copy.target,copy.extent).await?;let source=target.source(&self.reads);
        let mut updates=Vec::new();
        for entry in self.index.values() {
            let range=entry.location.range();
            if range.pack.id==copy.source {
                let offset=*targets.get(&range.offset).ok_or("Compaction omitted a live resource")?;
                updates.push((entry.location.clone(),Arc::new(StoreRange {pack:target.clone(),source:source.clone(),offset})));
            }
        }
        let publish_head=self.head.is_some() && (!remapped.is_empty() || self.compaction_fallback.is_some());
        let size=remapped.iter().map(|(_,_,bytes)|bytes.len() as u64).sum::<u64>()+if publish_head {head::SLOT_BYTES as u64}else {0};
        let mut reservation=if size==0 {None}else {Some(storage_budget::reserve(&*self._owner.io,size).await?)};
        let mut created=Vec::new();
        for (_,generation,bytes) in &remapped {
            let name=format!("generations/{}.json",generation.id);
            let file=self._owner.io.open(&name,FileAccess::Create).await?;created.push(name);
            for (index,chunk) in bytes.chunks(CHUNK_BYTES as usize).enumerate() {
                if activity.revision()!=epoch {
                    drop(file);for name in created {storage_budget::remove(&*self._owner.io,&name).await?;}return Ok(false);
                }
                let result=file.write_at(index as u64*CHUNK_BYTES,chunk).await;
                reservation.as_mut().unwrap().written(chunk.len() as u64)?;result?;
            }
            file.flush().await?;
        }
        if activity.revision()!=epoch {
            for name in created {storage_budget::remove(&*self._owner.io,&name).await?;}return Ok(false);
        }
        if let Some(head)=self.head.clone() && publish_head {
            if self.compaction_fallback.is_none() {self.compaction_fallback=Some(head.clone());}
            for (old,new,_) in &remapped {if let Some(programs)=self.programs.get(&old.id).cloned() {self.programs.insert(new.id,programs);}}
            let head=Head {current:generations[&head.current.id].clone(),previous:head.previous.map(|previous|generations[&previous.id].clone())};
            let published=head::publish(&*self._owner.io,self.sequence,&head).await;
            reservation.as_mut().unwrap().written(head::SLOT_BYTES as u64)?;
            let sequence=published?;self.head=Some(head);self.sequence=sequence;self.compaction_fallback=None;
            if let Some(loaded)=&mut self.loaded {*loaded=generations[&loaded.id].clone();}
            if let Some(published)=&mut self.published_capture {
                if let Some(generation)=generations.get(&published.generation) {published.generation=generation.id;}else {self.published_capture=None;}
            }
            let head=self.head.as_ref().unwrap();
            self.programs.retain(|id,_|*id==head.current.id || head.previous.as_ref().is_some_and(|previous|previous.id==*id));
            self.generation_roots.clear();
        }
        for (location,range) in updates {*location.range.lock().unwrap()=range;}
        if self.append.is_none() || self.append.is_some_and(|(id,_)|id==copy.target) {self.append=Some((copy.target,copy.extent));}
        Ok(true)
    }
}
