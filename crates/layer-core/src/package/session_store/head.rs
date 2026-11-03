use super::{Head,parse_head};
use crate::package::store_io::{FileAccess,FileTransport};

const SLOT_BYTES:usize=2048;
const HEADER_BYTES:usize=24;
const MAGIC:&[u8;8]=b"CAPYSH01";
const NAMES:[&str;2]=["head.0","head.1"];
pub(super) struct PublishedHead {pub sequence:u64,pub head:Head}

fn checksum(bytes:&[u8])->u32 {
    let mut checksum=crc32fast::Hasher::new();checksum.update(&bytes[8..20]);checksum.update(&bytes[HEADER_BYTES..]);checksum.finalize()
}
fn decode(bytes:&[u8],slot:usize)->Result<PublishedHead,String> {
    if bytes.len()!=SLOT_BYTES || &bytes[..8]!=MAGIC {return Err("Invalid drawing head slot".into());}
    let sequence=u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let length=u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let expected=u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    if sequence==0 || sequence%2!=slot as u64 || length>SLOT_BYTES-HEADER_BYTES || checksum(bytes)!=expected {return Err("Drawing head slot checksum failed".into());}
    Ok(PublishedHead {sequence,head:parse_head(&bytes[HEADER_BYTES..HEADER_BYTES+length])?})
}
fn encode(sequence:u64,head:&Head)->Result<Vec<u8>,String> {
    let body=serde_json::to_vec(head).map_err(|error|error.to_string())?;
    if body.len()>SLOT_BYTES-HEADER_BYTES {return Err("Drawing head exceeds its slot".into());}
    let mut bytes=vec![0;SLOT_BYTES];bytes[..8].copy_from_slice(MAGIC);bytes[8..16].copy_from_slice(&sequence.to_le_bytes());
    bytes[16..20].copy_from_slice(&(body.len() as u32).to_le_bytes());bytes[HEADER_BYTES..HEADER_BYTES+body.len()].copy_from_slice(&body);
    let crc=checksum(&bytes);bytes[20..24].copy_from_slice(&crc.to_le_bytes());Ok(bytes)
}
pub(super) async fn read(io:&dyn FileTransport)->Result<Option<PublishedHead>,String> {
    let entries=io.list("").await?;let mut latest:Option<PublishedHead>=None;let mut failure=None;
    for (slot,name) in NAMES.into_iter().enumerate() {
        if !entries.iter().any(|entry|entry.name==name) {continue;}
        let result=async {
            let file=io.open(name,FileAccess::Read).await?;
            if file.len().await?!=SLOT_BYTES as u64 {return Err("Incomplete drawing head slot".into());}
            decode(&file.read_at(0,SLOT_BYTES).await?,slot)
        }.await;
        match result {
            Ok(head) if latest.as_ref().is_none_or(|latest|head.sequence>latest.sequence)=>latest=Some(head),
            Ok(_)=>{},Err(error)=>failure=Some(error),
        }
    }
    match (latest,failure) {(Some(head),_)=>Ok(Some(head)),(None,Some(error))=>Err(error),(None,None)=>Ok(None)}
}
pub(super) async fn publish(io:&dyn FileTransport,previous:u64,head:&Head)->Result<u64,String> {
    let sequence=previous.checked_add(1).ok_or("Drawing head sequence exhausted")?;
    let name=NAMES[(sequence%2) as usize];let bytes=encode(sequence,head)?;
    let existing=io.list("").await?.iter().any(|entry|entry.name==name);
    let file=io.open(name,if existing {FileAccess::Write}else {FileAccess::Create}).await?;
    let result=async {file.write_at(0,&bytes).await?;file.flush().await}.await;
    if let Err(error)=result {
        if !existing {
            let incomplete=match file.len().await {
                Ok(length) if length!=SLOT_BYTES as u64=>true,
                Ok(_)=>file.read_at(0,SLOT_BYTES).await.is_ok_and(|bytes|decode(&bytes,(sequence%2) as usize).is_err()),
                Err(_)=>false,
            };
            drop(file);
            if incomplete {io.remove(name).await.map_err(|cleanup|format!("{error}; {cleanup}"))?;}
        }
        return Err(error);
    }
    Ok(sequence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::session_store::Generation;
    #[test]
    fn a_torn_slot_preserves_the_other_sequenced_head() {
        let head=Head {current:Generation::new(crate::PortableId::random(),b"generation"),previous:None};
        let old=encode(1,&head).unwrap();let current=encode(2,&head).unwrap();
        assert_eq!(decode(&old,1).unwrap().sequence,1);assert_eq!(decode(&current,0).unwrap().sequence,2);
        assert!(decode(&current,1).is_err());
        for length in 0..SLOT_BYTES {assert!(decode(&current[..length],0).is_err());}
        for offset in [0,8,16,20,HEADER_BYTES,SLOT_BYTES-1] {
            let mut torn=current.clone();torn[offset]^=1;assert!(decode(&torn,0).is_err());
            assert!(decode(&old,1).unwrap().head==head);
        }
    }
}
