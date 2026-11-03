use super::*;
use crate::package::store_io::{FileAccess,FileTransport};

pub struct MetadataStore {io:Arc<dyn FileTransport>,limit:u64}
impl MetadataStore {
    pub fn new(io:Arc<dyn FileTransport>,limit:u64)->Self {Self {io,limit}}
    pub async fn generation_id(&self)->Result<Option<PortableId>,String> {
        let Some(published)=head::read(&*self.io).await? else {return Ok(None);};
        self.generation(&published.head.current).await?;Ok(Some(published.head.current.id))
    }
    async fn generation(&self,generation:&Generation)->Result<Vec<u8>,String> {
        let file=self.io.open(&format!("generations/{}.json",generation.id),FileAccess::Read).await?;
        let bytes=io::read_file(&*file,self.limit).await?;
        if Generation::new(generation.id,&bytes)!=*generation {return Err("Saved session metadata checksum failed".into());}Ok(bytes)
    }
    async fn current(&self,head:&Head)->Result<(Generation,Vec<u8>),String> {
        Ok((head.current.clone(),self.generation(&head.current).await?))
    }
    pub async fn read(&self)->Result<Option<Vec<u8>>,String> {
        let Some(head)=head::read(&*self.io).await? else {return Ok(None);};
        self.current(&head.head).await.map(|(_,bytes)|Some(bytes))
    }
    pub async fn publish(&self,bytes:&[u8])->Result<(),String> {
        if bytes.len() as u64>self.limit {return Err("Saved session is too large".into());}
        let published=head::read(&*self.io).await?;
        let (previous,sequence)=match published {
            Some(published)=>{
                let (generation,current)=self.current(&published.head).await?;
                if current==bytes {return Ok(());}
                (Some(generation),published.sequence)
            },
            None=>(None,0),
        };
        let generation=PortableId::random();let cancelled=AtomicBool::new(false);
        io::create(&*self.io,&format!("generations/{generation}.json"),bytes,&cancelled).await?;
        let head=Head {current:Generation::new(generation,bytes),previous};
        head::publish(&*self.io,sequence,&head).await?;
        let retained=std::iter::once(head.current.id).chain(head.previous.map(|generation|generation.id)).collect();
        io::clean(&*self.io,"generations",&retained,".json").await
    }
}

#[cfg(all(test,not(target_arch="wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn metadata_publication_reuses_clean_bytes_and_never_resurrects_an_older_index() {
        let root=std::env::temp_dir().join(format!("capy-metadata-{}",PortableId::random()));
        let io=Arc::new(crate::package::store_io::NativeTransport::new(root.clone()));
        let store=MetadataStore::new(io.clone(),1024);
        pollster::block_on(async {
            assert!(store.read().await.unwrap().is_none());
            store.publish(b"first").await.unwrap();let first=head::read(&*io).await.unwrap().unwrap();
            store.publish(b"first").await.unwrap();assert_eq!(head::read(&*io).await.unwrap().unwrap().sequence,first.sequence);
            store.publish(b"second").await.unwrap();let second=head::read(&*io).await.unwrap().unwrap();
            let file=io.open(&format!("generations/{}.json",second.head.current.id),FileAccess::Write).await.unwrap();
            file.write_at(0,b"broken").await.unwrap();file.flush().await.unwrap();
            assert!(store.read().await.is_err());assert!(store.publish(b"third").await.is_err());
            let current=head::read(&*io).await.unwrap().unwrap();assert_eq!(current.sequence,second.sequence);
            assert_eq!(current.head.current.id,second.head.current.id);assert_eq!(current.head.previous.unwrap().id,first.head.current.id);
            assert!(store.publish(&[0;1025]).await.is_err());
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
