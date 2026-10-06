use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;
use super::session_recovery::DestinationFingerprint;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DestinationExpectation {
    pub location: DocumentLocation,
    pub fingerprint: Option<DestinationFingerprint>,
    pub required: bool,
}
impl DestinationExpectation {
    pub fn matches(&self, observed: Option<&DestinationFingerprint>) -> bool {
        match (&self.fingerprint, observed) {
            (Some(expected), Some(actual)) => expected == actual,
            (None, _) => !self.required,
            _ => false,
        }
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn can_reuse_path(&self,path:&std::path::Path)->bool {
        std::fs::OpenOptions::new().write(true).open(path).is_ok()
            && (!self.required || self.matches(DestinationFingerprint::observe_path(path,self.fingerprint.as_ref()).as_ref()))
    }
}
pub struct FingerprintWriter<W> {writer:W,hash:Sha256,bytes:u64}
impl<W:std::io::Write> FingerprintWriter<W> {
    pub fn new(writer:W)->Self {Self {writer,hash:Sha256::new(),bytes:0}}
    pub fn finish(self)->DestinationFingerprint {DestinationFingerprint {bytes:self.bytes,sha256:format!("{:x}",self.hash.finalize())}}
}
impl<W:std::io::Write> std::io::Write for FingerprintWriter<W> {
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {
        let length=self.writer.write(bytes)?;
        self.hash.update(&bytes[..length]);
        self.bytes=self.bytes.checked_add(length as u64).ok_or_else(||std::io::Error::other("Saved drawing is too large"))?;
        Ok(length)
    }
    fn flush(&mut self)->std::io::Result<()> {self.writer.flush()}
}
impl DestinationFingerprint {
    pub fn observe(&self,reader:impl Read)->Option<Self> {
        self.validate().ok()?;
        let observed=Self::read(reader.take(self.bytes.saturating_add(1))).ok()?;
        (observed.bytes==self.bytes).then_some(observed)
    }
    #[cfg(not(target_arch="wasm32"))]
    pub fn observe_path(path:&std::path::Path,expected:Option<&Self>)->Option<Self> {
        let expected=expected?;let file=std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len()!=expected.bytes {return None;}
        expected.observe(file)
    }
    pub fn read(mut reader: impl Read) -> Result<Self, String> {
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let length = match reader.read(&mut buffer) {
                Ok(length) => length,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.to_string()),
            };
            if length == 0 { break; }
            bytes = bytes.checked_add(length as u64).ok_or("Saved drawing is too large")?;
            hash.update(&buffer[..length]);
        }
        Ok(Self { bytes, sha256: format!("{:x}", hash.finalize()) })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_path(path: &std::path::Path) -> Result<Self, String> {
        Self::read(std::fs::File::open(path).map_err(|error| error.to_string())?)
    }
}
impl<R: CanvasRenderer> UiSession<R> {
    pub fn save_destination_expectation(&self) -> Option<DestinationExpectation> {
        self.state.document_file.location.as_ref().map(|location| DestinationExpectation {
            location: location.clone(),
            fingerprint: self.files.destination.clone(),
            required: self.files.check_destination,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(target_arch="wasm32"))]
    fn saved_destinations_require_write_access_and_unchanged_originals() {
        let root=std::env::temp_dir().join(format!("capy-save-access-{}",layer_core::PortableId::random()));
        std::fs::create_dir(&root).unwrap();
        let path=root.join("drawing.capy");
        std::fs::write(&path,b"original drawing").unwrap();
        let permissions=std::fs::metadata(&path).unwrap().permissions();
        let expected=DestinationExpectation {
            location:DocumentLocation {uri:path.to_string_lossy().into_owned(),name:"drawing.capy".into()},
            fingerprint:Some(DestinationFingerprint::read_path(&path).unwrap()),required:true,
        };
        assert!(expected.can_reuse_path(&path));
        let mut readonly=permissions.clone();readonly.set_readonly(true);
        std::fs::set_permissions(&path,readonly).unwrap();
        assert!(!expected.can_reuse_path(&path));
        assert!(!DestinationExpectation {required:false,..expected.clone()}.can_reuse_path(&path));
        assert_eq!(std::fs::read(&path).unwrap(),b"original drawing");
        std::fs::set_permissions(&path,permissions).unwrap();
        std::fs::write(&path,b"modified drawing").unwrap();
        assert!(!expected.can_reuse_path(&path));
        assert!(DestinationExpectation {required:false,..expected.clone()}.can_reuse_path(&path));
        std::fs::remove_file(&path).unwrap();
        assert!(!expected.can_reuse_path(&path));
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn observing_external_originals_bounds_reads_and_protects_unreadable_files() {
        use std::io::Read;
        let expected=DestinationFingerprint::read(&b"saved"[..]).unwrap();
        let mut growing=std::io::repeat(7);let mut read=0;
        struct Count<'a,R>(&'a mut R,&'a mut u64);impl<R:Read> Read for Count<'_,R> {fn read(&mut self,bytes:&mut [u8])->std::io::Result<usize>{let count=self.0.read(bytes)?;*self.1+=count as u64;Ok(count)}}
        let reader=Count(&mut growing,&mut read);
        assert!(expected.observe(reader).is_none());assert_eq!(read,expected.bytes+1);
        assert!(expected.observe(&b"short"[..]).is_some());assert!(expected.observe(&b"tiny"[..]).is_none());
        struct Denied;impl Read for Denied {fn read(&mut self,_:&mut [u8])->std::io::Result<usize>{Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))}}
        assert!(expected.observe(Denied).is_none());
    }
    #[test]
    fn fingerprint_writer_hashes_successful_short_writes() {
        struct Short(Vec<u8>);
        impl std::io::Write for Short {
            fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {let length=bytes.len().min(3);self.0.extend_from_slice(&bytes[..length]);Ok(length)}
            fn flush(&mut self)->std::io::Result<()> {Ok(())}
        }
        let mut output=Short(Vec::new());
        let mut writer=FingerprintWriter::new(&mut output);
        std::io::Write::write_all(&mut writer,b"saved pixels and metadata").unwrap();
        let fingerprint=writer.finish();
        assert_eq!(fingerprint,DestinationFingerprint::read(output.0.as_slice()).unwrap());
    }
    #[test]
    fn destination_fingerprints_detect_equal_length_external_changes() {
        let fingerprint = DestinationFingerprint::read(&b"artist's saved drawing"[..]).unwrap();
        let changed = DestinationFingerprint::read(&b"Artist's saved drawing"[..]).unwrap();
        assert_eq!(fingerprint.bytes, changed.bytes);
        let expected = DestinationExpectation {
            location: DocumentLocation { uri: "/drawing.capy".into(), name: "drawing.capy".into() },
            fingerprint: Some(fingerprint.clone()), required: true,
        };
        assert!(expected.matches(Some(&fingerprint)));
        assert!(!expected.matches(Some(&changed)));
        assert!(!expected.matches(None));
        assert!(!DestinationExpectation { fingerprint: None, ..expected.clone() }.matches(Some(&fingerprint)));
        assert!(DestinationExpectation { fingerprint: None, required: false, ..expected }.matches(None));
    }
}
