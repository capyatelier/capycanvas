use layer_core::package::{ByteRange,ByteSource,ImmutableBacking,RangeState,MAX_RANGE_BYTES};
use std::{cell::RefCell,collections::BTreeMap,marker::PhantomData,rc::Rc,sync::{Arc,atomic::{AtomicU64,Ordering}},thread::{self,ThreadId}};

struct Local {file:web_sys::Blob,reader:web_sys::FileReaderSync}
thread_local! {static SOURCES:RefCell<BTreeMap<u64,Local>>=Default::default();}
struct Source {id:u64,length:u64,thread:ThreadId,key:Option<String>}
impl ByteSource for Source {
    fn byte_len(&self)->u64 {self.length}
    fn transfer_key(&self)->Option<String> {self.key.clone()}
    fn resident_bytes(&self)->usize {0}
    fn poll(&self,offset:u64,length:usize)->Result<RangeState,String> {
        if self.thread!=thread::current().id() {return Err("File reader belongs to another worker".into());}
        let end=offset.checked_add(length as u64).filter(|end|*end<=self.length)
            .ok_or("Package range exceeds immutable backing")?;
        if length>MAX_RANGE_BYTES {return Err("Package read exceeds bounded range".into());}
        SOURCES.with(|sources| {
            let sources=sources.try_borrow().map_err(|_|"File reader is busy")?;
            let source=sources.get(&self.id).ok_or("File reader has expired")?;
            let slice=source.file.slice_with_f64_and_f64(offset as f64,end as f64).map_err(|error|format!("{error:?}"))?;
            let buffer=source.reader.read_as_array_buffer(&slice).map_err(|error|format!("{error:?}"))?;
            let bytes=js_sys::Uint8Array::new(&buffer);
            if bytes.length() as usize!=length {return Err("Incomplete package range".into());}
            Ok(RangeState::Ready(ByteRange::new(Arc::from(bytes.to_vec()),0..length).map_err(str::to_owned)?))
        })
    }
}
pub(super) struct Backing {pub source:ImmutableBacking,id:u64,_local:PhantomData<Rc<()>>}
impl Backing {
    pub fn new(file:web_sys::Blob)->Result<Self,wasm_bindgen::JsValue> {Self::with_key(file,None)}
    pub fn with_key(file:web_sys::Blob,key:Option<String>)->Result<Self,wasm_bindgen::JsValue> {
        if web_sys::window().is_some() {return Err(super::js("File reading requires its worker"));}
        let length=file.size();
        if !length.is_finite() || length<0. || length.fract()!=0. || length>9_007_199_254_740_991. {
            return Err(super::js("Invalid file length"));
        }
        static NEXT:AtomicU64=AtomicU64::new(1);
        let id=NEXT.try_update(Ordering::Relaxed,Ordering::Relaxed,|id|id.checked_add(1)).map_err(|_|super::js("File reader identities exhausted"))?;
        let reader=web_sys::FileReaderSync::new()?;
        let source=ImmutableBacking::new(Arc::new(Source {id,length:length as u64,thread:thread::current().id(),key})).map_err(super::js)?;
        SOURCES.with(|sources|sources.borrow_mut().insert(id,Local {file,reader}));
        Ok(Self {source,id,_local:PhantomData})
    }
}
impl Drop for Backing {
    fn drop(&mut self) {let _=SOURCES.try_with(|sources| {if let Ok(mut sources)=sources.try_borrow_mut() {sources.remove(&self.id);}});}
}
