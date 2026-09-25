//! Versioned envelopes for small JSON indices with opaque binary payloads.
//! Bounds are checked before allocation; a digest covers the index and bytes.
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
const MAX_BLOCKS: usize = 65_536;

pub fn encode<T: Serialize>(
    magic: &[u8; 12],
    metadata: &T,
    blocks: &[impl AsRef<[u8]>],
    limit: usize,
) -> Result<Vec<u8>, String> {
    if blocks.len() > MAX_BLOCKS {
        return Err("Too many binary payloads".into());
    }
    let json = serde_json::to_vec(metadata).map_err(|e| e.to_string())?;
    let size = 52usize
        .checked_add(json.len())
        .and_then(|start| {
            blocks.iter().try_fold(start, |n, b| {
                n.checked_add(8)?.checked_add(b.as_ref().len())
            })
        })
        .filter(|n| *n <= limit)
        .ok_or("Binary payload exceeds its size limit")?;
    let mut out = Vec::new();
    out.try_reserve_exact(size).map_err(|e| e.to_string())?;
    out.extend_from_slice(magic);
    out.extend_from_slice(
        &u32::try_from(json.len())
            .map_err(|_| "Oversized metadata")?
            .to_le_bytes(),
    );
    out.extend_from_slice(
        &u32::try_from(blocks.len())
            .map_err(|_| "Too many payloads")?
            .to_le_bytes(),
    );
    out.extend_from_slice(&json);
    for block in blocks {
        let bytes = block.as_ref();
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    let digest = Sha256::digest(&out);
    out.extend_from_slice(&digest);
    Ok(out)
}

pub fn decode<'a, T: DeserializeOwned>(
    magic: &[u8; 12],
    bytes: &'a [u8],
    limit: usize,
) -> Result<(T, Vec<&'a [u8]>), String> {
    if bytes.len() > limit || bytes.len() < 52 || &bytes[..12] != magic {
        return Err("Unsupported or oversized binary payload".into());
    }
    let (body, digest) = bytes.split_at(bytes.len() - 32);
    if Sha256::digest(body).as_slice() != digest {
        return Err("Binary payload integrity check failed".into());
    }
    let size = u32::from_le_bytes(body[12..16].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(body[16..20].try_into().unwrap()) as usize;
    let mut rest = &body[20..];
    let json = take(&mut rest, size)?;
    if count > MAX_BLOCKS || count > rest.len() / 8 {
        return Err("Incomplete or oversized binary payload index".into());
    }
    let mut blocks = Vec::new();
    for _ in 0..count {
        let length = u64::from_le_bytes(take(&mut rest, 8)?.try_into().unwrap());
        blocks.push(take(
            &mut rest,
            usize::try_from(length).map_err(|_| "Oversized payload")?,
        )?);
    }
    if !rest.is_empty() {
        return Err("Unexpected binary payload data".into());
    }
    Ok((
        serde_json::from_slice(json).map_err(|e| e.to_string())?,
        blocks,
    ))
}

fn take<'a>(bytes: &mut &'a [u8], count: usize) -> Result<&'a [u8], String> {
    if count > bytes.len() {
        return Err("Incomplete binary payload".into());
    }
    let (head, tail) = bytes.split_at(count);
    *bytes = tail;
    Ok(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_envelope_rejects_corruption_truncation_and_trailing_bytes() {
        let magic = b"CAPYTEST\x01\0\0\0";
        let blocks = [vec![255; 1024], vec![0, 1, 2]];
        let encoded = encode(magic, &"test", &blocks, 2048).unwrap();
        let (metadata, restored): (String, _) = decode(magic, &encoded, 2048).unwrap();
        assert_eq!(metadata, "test");
        assert_eq!(restored, blocks);
        assert!(encode(magic, &"test", &blocks, 1024).is_err());
        for end in 0..encoded.len() {
            assert!(decode::<String>(magic, &encoded[..end], 2048).is_err());
        }
        for pos in [0, 12, 16, 20, 100, encoded.len() - 1] {
            let mut bad = encoded.clone();
            bad[pos] ^= 1;
            assert!(decode::<String>(magic, &bad, 2048).is_err());
        }
        // A valid digest does not make attacker-controlled lengths admissible.
        for (range, replacement) in [
            (12..16, u32::MAX.to_le_bytes().to_vec()),
            (16..20, u32::MAX.to_le_bytes().to_vec()),
            (26..34, u64::MAX.to_le_bytes().to_vec()),
        ] {
            let mut bad = encoded[..encoded.len() - 32].to_vec();
            bad[range].copy_from_slice(&replacement);
            let digest = Sha256::digest(&bad);
            bad.extend_from_slice(&digest);
            assert!(decode::<String>(magic, &bad, 2048).is_err());
        }
        let mut bad = encoded;
        bad.push(0);
        assert!(decode::<String>(magic, &bad, 2048).is_err());
    }
}
