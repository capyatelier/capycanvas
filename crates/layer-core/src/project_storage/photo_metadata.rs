//! Photo metadata blocks follow the source profiles as binary payloads with
//! independent digests, never as JSON byte arrays.
use super::*;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockRecord {
    offset: u64,
    size: u64,
    digest: [u8; 32],
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MetadataIndex {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    exif: Option<BlockRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    xmp: Option<BlockRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iptc: Option<BlockRecord>,
}
impl MetadataIndex {
    pub(super) fn is_empty(&self) -> bool {
        self.records().iter().all(|r| r.is_none())
    }
    fn records(&self) -> [&Option<BlockRecord>; 3] {
        [&self.exif, &self.xmp, &self.iptc]
    }
    pub(super) fn collect(metadata: &PhotoMetadata, offset: &mut u64) -> (Self, Vec<Arc<[u8]>>) {
        let mut blocks = Vec::new();
        let mut record = |block: &Option<crate::authored::Resource<[u8]>>| {
            block.as_ref().map(|bytes| {
                let record = BlockRecord {
                    offset: *offset,
                    size: bytes.len() as u64,
                    digest: Sha256::digest(bytes).into(),
                };
                *offset += record.size;
                blocks.push(bytes.storage().clone());
                record
            })
        };
        let index = Self {
            exif: record(&metadata.exif),
            xmp: record(&metadata.xmp),
            iptc: record(&metadata.iptc),
        };
        (index, blocks)
    }
    pub(super) fn validate(&self, offset: &mut u64) -> Result<(), String> {
        let mut total = 0u64;
        for record in self.records().into_iter().flatten() {
            total = total.saturating_add(record.size);
            if record.offset != *offset
                || record.size == 0
                || total > PhotoMetadata::MAX_BYTES as u64
            {
                return Err("Invalid photo metadata index".into());
            }
            *offset = offset
                .checked_add(record.size)
                .ok_or("Photo metadata index overflow")?;
        }
        Ok(())
    }
    pub(super) fn read(self, input: &mut impl Read, document: &mut Document) -> Result<(), String> {
        let mut block = |record: Option<BlockRecord>| {
            record
                .map(|record| {
                    let bytes = read_block(input, record.size, PhotoMetadata::MAX_BYTES as u64)?;
                    if <[u8; 32]>::from(Sha256::digest(&bytes)) != record.digest {
                        return Err("Photo metadata integrity check failed".to_string());
                    }
                    Ok(bytes.into())
                })
                .transpose()
        };
        document.metadata = PhotoMetadata {
            exif: block(self.exif)?,
            xmp: block(self.xmp)?,
            iptc: block(self.iptc)?,
        };
        Ok(())
    }
}
