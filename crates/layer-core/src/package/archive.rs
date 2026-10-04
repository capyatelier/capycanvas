use std::{collections::BTreeSet, io::{Read, Seek, SeekFrom, Write}};
use super::values::{DecodeError, DecodeResult};

pub const MIMETYPE: &[u8] = b"application/vnd.capycanvas";
const LIMIT32: u64 = u32::MAX as u64;
const LIMIT16: u64 = u16::MAX as u64;
const CREDENTIAL: &str = "META-INF/content_credential.c2pa";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member { pub name: String, pub offset: u64, pub length: u64, pub compressed_length: Option<u64>, pub crc32: u32 }
#[derive(Clone, Debug)]
pub struct Directory { pub members: Vec<Member>, pub length: u64 }

pub struct InputMember<'a> { pub name: &'a str, pub length: u64, pub crc32: u32, pub input: &'a mut dyn Read }

fn io(error: std::io::Error) -> String { format!("Package I/O failed: {error}") }
fn u16_at(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset+2].try_into().unwrap()) }
fn u32_at(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset+4].try_into().unwrap()) }
fn u64_at(bytes: &[u8], offset: usize) -> u64 { u64::from_le_bytes(bytes[offset..offset+8].try_into().unwrap()) }
fn at<const N: usize>(input: &mut (impl Read + Seek), offset: u64) -> Result<[u8; N], String> {
    input.seek(SeekFrom::Start(offset)).map_err(io)?;
    let mut bytes = [0; N]; input.read_exact(&mut bytes).map_err(io)?; Ok(bytes)
}
fn name_valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255 && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"/._-".contains(&c))
        && name.split('/').all(|part| !matches!(part, "" | "." | ".."))
}
fn add(a: u64, b: u64) -> Result<u64, String> { a.checked_add(b).ok_or_else(|| "Package offset overflow".into()) }
fn extras(bytes: &[u8], expected: &[u64]) -> Result<(), String> {
    if expected.is_empty() { return if bytes.is_empty() { Ok(()) } else { Err("Unexpected ZIP extra field".into()) }; }
    if bytes.len() != 4 + 8 * expected.len() || u16_at(bytes, 0) != 1 || usize::from(u16_at(bytes, 2)) != expected.len() * 8
        || expected.iter().enumerate().any(|(i, value)| u64_at(bytes, 4 + 8 * i) != *value) {
        return Err("Noncanonical ZIP64 extra field".into());
    }
    Ok(())
}

impl Directory {
    pub fn read(input: &mut (impl Read + Seek), max_entries: usize, max_metadata: u64) -> DecodeResult<Self> {
        let length = input.seek(SeekFrom::End(0)).map_err(io)?;
        let end_offset = length.checked_sub(22).ok_or("Incomplete ZIP end record")?;
        let end = at::<22>(input, end_offset)?;
        if u32_at(&end, 0) != 0x06054b50 || u16_at(&end, 4) != 0 || u16_at(&end, 6) != 0 || u16_at(&end, 20) != 0 {
            return Err("Invalid terminal ZIP directory".into());
        }
        let (mut count, mut size, mut offset) = (u16_at(&end, 10) as u64, u32_at(&end, 12) as u64, u32_at(&end, 16) as u64);
        if u16_at(&end, 8) as u64 != count { return Err("Multipart ZIP directory".into()); }
        let zip64 = count == LIMIT16 || size == LIMIT32 || offset == LIMIT32;
        let directory_end = if zip64 {
            let start = end_offset.checked_sub(76).ok_or("Incomplete ZIP64 end records")?;
            let tail = at::<76>(input, start)?;
            if u32_at(&tail, 0) != 0x06064b50 || u64_at(&tail, 4) != 44
                || u16_at(&tail, 14) != 45 || u32_at(&tail, 16) != 0 || u32_at(&tail, 20) != 0
                || u64_at(&tail, 24) != u64_at(&tail, 32) || u32_at(&tail, 56) != 0x07064b50
                || u32_at(&tail, 60) != 0 || u64_at(&tail, 64) != start || u32_at(&tail, 72) != 1 {
                return Err("Invalid ZIP64 end records".into());
            }
            let actual = [u64_at(&tail, 32), u64_at(&tail, 40), u64_at(&tail, 48)];
            for ((classic, value), limit) in [count, size, offset].into_iter().zip(actual).zip([LIMIT16, LIMIT32, LIMIT32]) {
                if classic != value.min(limit) { return Err("Conflicting ZIP64 directory".into()); }
            }
            [count, size, offset] = actual;
            start
        } else { end_offset };
        if count == 0 || add(offset, size)? != directory_end {
            return Err("Invalid ZIP directory".into());
        }
        if count > max_entries as u64 || size > max_metadata { return Err(DecodeError::Unsupported("ZIP directory exceeds admission".into())); }
        let mut archive = zip::ZipArchive::new(&mut *input).map_err(|e| format!("Invalid ZIP: {e}"))?;
        if archive.len() as u64 != count || archive.offset() != 0 { return Err("Ambiguous ZIP member inventory".into()); }
        let mut members = Vec::with_capacity(count as usize);
        let mut headers = Vec::with_capacity(count as usize);
        let mut names = BTreeSet::new();
        for index in 0..archive.len() {
            let file = archive.by_index_raw(index).map_err(|e| e.to_string())?;
            let name = std::str::from_utf8(file.name_raw()).map_err(|_| "Non-ASCII ZIP member")?;
            if name.len()>255 { return Err(DecodeError::Unsupported("ZIP member name exceeds admission".into())); }
            if name == "manifest.json" && file.size() > max_metadata { return Err(DecodeError::Unsupported("Package metadata exceeds limit".into())); }
            if !name_valid(name) || !names.insert(name.to_ascii_lowercase()) || file.encrypted()
                || match file.compression() {
                    zip::CompressionMethod::Stored => file.size() != file.compressed_size(),
                    zip::CompressionMethod::Deflated => name != "manifest.json" || file.compressed_size() >= file.size(),
                    _ => true,
                }
                || file.unix_mode().is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000) {
                return Err("Unsupported or ambiguous ZIP member".into());
            }
            headers.push((file.header_start(), file.central_header_start()));
            members.push(Member { name: name.into(), offset: file.data_start().ok_or("Missing ZIP data offset")?, length: file.size(), compressed_length: (file.compression() == zip::CompressionMethod::Deflated).then_some(file.compressed_size()), crc32: file.crc32() });
        }
        drop(archive);
        let (mut local, mut central) = (0, offset);
        for (member, (header, directory)) in members.iter().zip(headers) {
            if local != header || central != directory { return Err("Gapped or overlapping ZIP members".into()); }
            let l = at::<30>(input, local)?;
            let c = at::<46>(input, central)?;
            let encoded_length = member.compressed_length.unwrap_or(member.length);
            let method = if member.compressed_length.is_some() { 8 } else { 0 };
            let large = member.length >= LIMIT32 || encoded_length >= LIMIT32;
            let version = if large || header >= LIMIT32 { 45 } else { 20 };
            if u32_at(&l, 0) != 0x04034b50 || u32_at(&c, 0) != 0x02014b50
                || u16_at(&l, 4) != version || u16_at(&c, 6) != version
                || u16_at(&l, 6) != 0 || u16_at(&c, 8) != 0
                || u16_at(&l, 8) != method || u16_at(&c, 10) != method
                || l[10..14] != c[12..16] || u32_at(&l, 14) != member.crc32 || u32_at(&c, 16) != member.crc32
                || u32_at(&l, 18) as u64 != encoded_length.min(LIMIT32) || u32_at(&l, 22) as u64 != member.length.min(LIMIT32)
                || u32_at(&c, 20) as u64 != encoded_length.min(LIMIT32) || u32_at(&c, 24) as u64 != member.length.min(LIMIT32)
                || u32_at(&c, 42) as u64 != header.min(LIMIT32) || u16_at(&c, 32) != 0 || u16_at(&c, 34) != 0
                || usize::from(u16_at(&l, 26)) != member.name.len() || usize::from(u16_at(&c, 28)) != member.name.len() {
                return Err("Conflicting or noncanonical ZIP headers".into());
            }
            let mut expected = [member.length, encoded_length].into_iter().filter(|value| *value >= LIMIT32).collect::<Vec<_>>();
            for (base, fixed, name_size, extra_size, is_central) in [
                (local, 30, u16_at(&l, 26), u16_at(&l, 28), false),
                (central, 46, u16_at(&c, 28), u16_at(&c, 30), true),
            ] {
                input.seek(SeekFrom::Start(add(base, fixed)?)).map_err(io)?;
                let mut name = vec![0; name_size as usize]; input.read_exact(&mut name).map_err(io)?;
                if name != member.name.as_bytes() { return Err("Conflicting ZIP member names".into()); }
                let mut extra = vec![0; extra_size as usize]; input.read_exact(&mut extra).map_err(io)?;
                if is_central && header >= LIMIT32 { expected.push(header); }
                extras(&extra, &expected)?;
            }
            if member.offset != add(local, 30 + member.name.len() as u64 + u64::from(u16_at(&l, 28)))? { return Err("Conflicting ZIP data offset".into()); }
            local = add(member.offset, encoded_length)?;
            central = add(central, 46 + member.name.len() as u64 + u64::from(u16_at(&c, 30)))?;
        }
        if local != offset || central != directory_end { return Err("Conflicting ZIP member ranges".into()); }
        let directory = Self { members, length };
        let first = &directory.members[0];
        if first.name != "mimetype" || first.length != MIMETYPE.len() as u64
            || directory.read_member(input, first, MIMETYPE.len())? != MIMETYPE {
            return Err("Not a Capy Canvas package".into());
        }
        if !directory.members.iter().any(|m| m.name == "manifest.json") { return Err("Missing artwork manifest".into()); }
        Ok(directory)
    }
    pub fn member(&self, name: &str) -> Option<&Member> { self.members.iter().find(|m| m.name == name) }
    pub fn read_member(&self, input: &mut (impl Read + Seek), member: &Member, limit: usize) -> Result<Vec<u8>, String> {
        let encoded_length = member.compressed_length.unwrap_or(member.length);
        if member.length > limit as u64 || encoded_length > limit as u64 || !self.members.contains(member)
            || member.offset.checked_add(encoded_length).is_none_or(|end| end > self.length) { return Err("ZIP member exceeds read bound".into()); }
        input.seek(SeekFrom::Start(member.offset)).map_err(io)?;
        let mut bytes = vec![0; encoded_length as usize]; input.read_exact(&mut bytes).map_err(io)?;
        if member.compressed_length.is_some() {
            let mut decoded = vec![0; (member.length as usize).checked_add(1).ok_or("Manifest length overflow")?];
            let mut decoder = flate2::Decompress::new(false);
            let status = decoder.decompress(&bytes, &mut decoded, flate2::FlushDecompress::Finish).map_err(|e| e.to_string())?;
            if status != flate2::Status::StreamEnd || decoder.total_in() != encoded_length || decoder.total_out() != member.length {
                return Err("Invalid compressed manifest length or stream".into());
            }
            decoded.truncate(member.length as usize);
            bytes = decoded;
        }
        verify(member, crc32fast::hash(&bytes))?;
        Ok(bytes)
    }
}
fn verify(member: &Member, crc: u32) -> Result<(), String> {
    if member.name == CREDENTIAL && member.crc32 == 0 || member.crc32 == crc { Ok(()) }
    else { Err("ZIP member checksum failed".into()) }
}
fn word(out: &mut Vec<u8>, value: u16) { out.extend(value.to_le_bytes()); }
fn dword(out: &mut Vec<u8>, value: u32) { out.extend(value.to_le_bytes()); }
fn qword(out: &mut Vec<u8>, value: u64) { out.extend(value.to_le_bytes()); }
fn extra(values: &[u64]) -> Vec<u8> {
    let mut out = Vec::new();
    if !values.is_empty() { word(&mut out, 1); word(&mut out, (values.len() * 8) as u16); for value in values { qword(&mut out, *value); } }
    out
}
fn headers(name: &str, length: u64, compressed_length: Option<u64>, crc: u32, offset: u64) -> (Vec<u8>, Vec<u8>) {
    let encoded_length = compressed_length.unwrap_or(length);
    let sizes = [length, encoded_length].into_iter().filter(|value| *value >= LIMIT32).collect::<Vec<_>>();
    let local_extra = extra(&sizes);
    let mut all = sizes;
    if offset >= LIMIT32 { all.push(offset); }
    let central_extra = extra(&all);
    let version = if length >= LIMIT32 || encoded_length >= LIMIT32 || offset >= LIMIT32 { 45 } else { 20 };
    let mut common = Vec::new();
    for value in [version, 0, if compressed_length.is_some() { 8 } else { 0 }, 0, 0] { word(&mut common, value); }
    for value in [crc, encoded_length.min(LIMIT32) as u32, length.min(LIMIT32) as u32] { dword(&mut common, value); }
    word(&mut common, name.len() as u16);
    let mut local = Vec::new(); dword(&mut local, 0x04034b50); local.extend(&common); word(&mut local, local_extra.len() as u16);
    local.extend(name.as_bytes()); local.extend(local_extra);
    let mut central = Vec::new(); dword(&mut central, 0x02014b50); word(&mut central, version); central.extend(common);
    word(&mut central, central_extra.len() as u16);
    for value in [0, 0, 0] { word(&mut central, value); }
    dword(&mut central, 0); dword(&mut central, offset.min(LIMIT32) as u32);
    central.extend(name.as_bytes()); central.extend(central_extra);
    (local, central)
}
fn tail(count: u64, size: u64, offset: u64, large_member: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    if large_member || count >= LIMIT16 || size >= LIMIT32 || offset >= LIMIT32 {
        dword(&mut out, 0x06064b50); qword(&mut out, 44);
        word(&mut out, 45); word(&mut out, 45); dword(&mut out, 0); dword(&mut out, 0);
        for value in [count, count, size, offset] { qword(&mut out, value); }
        dword(&mut out, 0x07064b50); dword(&mut out, 0); qword(&mut out, add(offset, size)?); dword(&mut out, 1);
    }
    dword(&mut out, 0x06054b50); word(&mut out, 0); word(&mut out, 0);
    word(&mut out, count.min(LIMIT16) as u16); word(&mut out, count.min(LIMIT16) as u16);
    dword(&mut out, size.min(LIMIT32) as u32); dword(&mut out, offset.min(LIMIT32) as u32); word(&mut out, 0);
    Ok(out)
}

pub fn write_archive(output: &mut impl Write, members: &mut [InputMember<'_>], max_metadata: usize) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut directory = Vec::new();
    let mut offset = 0;
    let mut large = false;
    let mut buffer = vec![0; 64 * 1024];
    for member in members.iter_mut() {
        if !name_valid(member.name) || !names.insert(member.name.to_ascii_lowercase()) { return Err("Invalid or duplicate ZIP name".into()); }
        let manifest = if member.name == "manifest.json" {
            if member.length > max_metadata as u64 { return Err("Artwork metadata exceeds package limit".into()); }
            let mut raw = Vec::with_capacity(member.length as usize);
            let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            let mut remaining = member.length;
            while remaining != 0 {
                let count = remaining.min(buffer.len() as u64) as usize;
                member.input.read_exact(&mut buffer[..count]).map_err(io)?;
                raw.extend_from_slice(&buffer[..count]);
                encoder.write_all(&buffer[..count]).map_err(io)?;
                remaining -= count as u64;
            }
            if crc32fast::hash(&raw) != member.crc32 { return Err("ZIP member checksum failed".into()); }
            let encoded = encoder.finish().map_err(io)?;
            Some(if encoded.len() < raw.len() { (encoded, true) } else { (raw, false) })
        } else { None };
        let compressed_length = manifest.as_ref().and_then(|(bytes, compressed)| compressed.then_some(bytes.len() as u64));
        let encoded_length = compressed_length.unwrap_or(member.length);
        let mut encoded = std::io::Cursor::new(manifest.as_ref().map(|(bytes, _)| bytes.as_slice()).unwrap_or_default());
        let input: &mut dyn Read = if manifest.is_some() { &mut encoded } else { member.input };
        let (local, central) = headers(member.name, member.length, compressed_length, member.crc32, offset);
        if central.len() > max_metadata.saturating_sub(directory.len()) { return Err("ZIP directory exceeds memory bound".into()); }
        directory.extend(central);
        large |= member.length >= LIMIT32 || offset >= LIMIT32;
        output.write_all(&local).map_err(io)?;
        offset = add(add(offset, local.len() as u64)?, encoded_length)?;
        let mut remaining = encoded_length;
        let mut crc = crc32fast::Hasher::new();
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..count]).map_err(io)?;
            crc.update(&buffer[..count]); output.write_all(&buffer[..count]).map_err(io)?;
            remaining -= count as u64;
        }
        if compressed_length.is_none() && crc.finalize() != member.crc32 && !(member.name == CREDENTIAL && member.crc32 == 0) {
            return Err("ZIP member checksum failed".into());
        }
    }
    output.write_all(&directory).map_err(io)?;
    output.write_all(&tail(members.len() as u64, directory.len() as u64, offset, large)?).map_err(io)?;
    output.flush().map_err(io)
}

#[cfg(test)]
mod tests;
