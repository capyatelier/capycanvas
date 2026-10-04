use super::*;
use std::{cell::Cell, io::Cursor};

fn package(extra: &[(&str, &[u8])]) -> Vec<u8> { package_manifest(b"{}", extra) }
fn package_manifest(manifest: &[u8], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut inputs: Vec<_> = [("mimetype", MIMETYPE), ("manifest.json", manifest)].into_iter().chain(extra.iter().copied())
        .map(|(name, bytes)| (name, Cursor::new(bytes))).collect();
    let mut members: Vec<_> = inputs.iter_mut().map(|(name, input)| InputMember { name, length: input.get_ref().len() as u64,
        crc32: crc32fast::hash(input.get_ref()), input }).collect();
    let mut bytes = Vec::new(); write_archive(&mut bytes, &mut members, 16 * 1024 * 1024).unwrap(); bytes
}
fn read(bytes: &[u8]) -> DecodeResult<Directory> { Directory::read(&mut Cursor::new(bytes), 100_000, 16 * 1024 * 1024) }

#[test]
fn nonseekable_output_has_complete_local_headers_and_preserves_compressed_bytes() {
    let blob = crate::raster::TileBlob::encode(crate::color::DocumentColor::default().paint_descriptor(), &vec![93; 256*256*4]).unwrap();
    let compressed = blob.compressed().unwrap();
    let bytes = package(&[("data/tiles-1.bin", &compressed)]);
    let dir = read(&bytes).unwrap();
    assert_eq!(dir.members.len(), 3);
    let member = dir.member("data/tiles-1.bin").unwrap();
    assert_eq!(dir.read_member(&mut Cursor::new(&bytes), member, 1024*1024).unwrap(), &*compressed);
    let mut independent = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut restored = Vec::new(); independent.by_name("data/tiles-1.bin").unwrap().read_to_end(&mut restored).unwrap();
    assert_eq!(restored, &*compressed);
    for file in &dir.members { assert_eq!(crc32fast::hash(&bytes[file.offset as usize..(file.offset+file.length) as usize]), file.crc32); }
}

#[test]
fn ambiguous_local_and_central_headers_are_rejected() {
    let original = package(&[("data/a", b"pixels")]);
    let directory = u32_at(&original, original.len()-6) as usize;
    for (offset, value) in [(6, 8), (8, 8), (14, 255), (26, 9), (directory+8, 8), (directory+34, 1)] {
        let mut bytes = original.clone(); bytes[offset] = value;
        assert!(read(&bytes).is_err(), "accepted damaged header at {offset}");
    }
    for length in [0, 1, 21, 22, original.len()-1] { assert!(read(&original[..length]).is_err()); }
    let mut trailing = original.clone(); trailing.push(0); assert!(read(&trailing).is_err());
    let mut prefix = vec![0]; prefix.extend(original); assert!(read(&prefix).is_err());
}

#[test]
fn crc_failure_never_becomes_empty_payload_and_signature_exception_is_exact() {
    let mut bytes = package(&[("data/a", b"pixels")]);
    let dir = read(&bytes).unwrap(); let member = dir.member("data/a").unwrap();
    bytes[member.offset as usize] ^= 1;
    assert!(dir.read_member(&mut Cursor::new(bytes), member, 100).unwrap_err().contains("checksum"));
    assert!(verify(&Member { name:CREDENTIAL.into(), offset:0, length:1, compressed_length:None, crc32:0 }, 77).is_ok());
    assert!(verify(&Member { name:"META-INF/other.c2pa".into(), offset:0, length:1, compressed_length:None, crc32:0 }, 77).is_err());
}

#[test]
fn names_and_case_aliases_are_rejected_without_extraction() {
    for name in ["../paint", "/paint", "data//paint", "data/./paint", "data\\paint", "c:paint", "data/", "data/é"] { assert!(!name_valid(name)); }
    let mut a = b"".as_slice(); let mut b = b"".as_slice();
    let mut members = [InputMember { name:"data/paint", length:0, crc32:0, input:&mut a }, InputMember { name:"DATA/PAINT", length:0, crc32:0, input:&mut b }];
    assert!(write_archive(&mut Vec::new(), &mut members, 1000).is_err());
}

#[test]
fn zip64_thresholds_encode_only_saturated_fields_and_roundtrip_entry_count() {
    for length in [LIMIT32-1, LIMIT32, LIMIT32+1] {
        for offset in [LIMIT32-1, LIMIT32, LIMIT32+1] {
            let (local, central) = headers("data/a", length, None, 17, offset);
            let sizes = if length >= LIMIT32 { vec![length, length] } else { vec![] };
            extras(&local[36..], &sizes).unwrap();
            let mut values = sizes; if offset >= LIMIT32 { values.push(offset); }
            extras(&central[52..], &values).unwrap();
            assert_eq!(u32_at(&local, 18) as u64, length.min(LIMIT32));
            assert_eq!(u32_at(&central, 42) as u64, offset.min(LIMIT32));
        }
    }
    let extra: Vec<_> = (0..65_533).map(|n| (format!("data/{n}"), b"".as_slice())).collect();
    let borrowed: Vec<_> = extra.iter().map(|(n, b)| (n.as_str(), *b)).collect();
    let bytes = package(&borrowed);
    assert_eq!(u16_at(&bytes, bytes.len()-12), u16::MAX);
    assert_eq!(read(&bytes).unwrap().members.len(), 65_535);
}

#[test]
fn failed_and_cancelled_streams_do_not_publish_a_complete_directory() {
    let cancelled = Cell::new(false);
    struct CancelAfter<'a> { bytes: Vec<u8>, cancelled: &'a Cell<bool> }
    impl Write for CancelAfter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.bytes.extend(bytes); self.cancelled.set(true); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut output = crate::Cancellable { inner: CancelAfter { bytes: Vec::new(), cancelled: &cancelled }, cancelled: || cancelled.get() };
    let mut input = b"hello".as_slice();
    assert!(write_archive(&mut output, &mut [InputMember { name:"mimetype", length:5, crc32:crc32fast::hash(b"hello"), input:&mut input }], 1000).is_err());
    assert!(read(&output.inner.bytes).is_err());
    let mut input = b"short".as_slice();
    let mut bytes = Vec::new();
    assert!(write_archive(&mut bytes, &mut [InputMember { name:"mimetype", length:10, crc32:0, input:&mut input }], 1000).is_err());
    assert!(read(&bytes).is_err());
}

fn positions(bytes: &[u8]) -> Vec<(usize, usize)> {
    let directory = read(bytes).unwrap();
    let mut central = u32_at(bytes, bytes.len() - 6) as usize;
    directory.members.iter().map(|member| {
        let local = u32_at(bytes, central + 42) as usize;
        let pair = (local, central);
        central += 46 + member.name.len() + usize::from(u16_at(bytes, central + 30));
        pair
    }).collect()
}

#[test]
fn reader_rejects_aliases_links_reordered_members_and_competing_tails() {
    let original = package(&[("data/a", b"a"), ("data/b", b"b")]);
    let positions = positions(&original);
    for replacement in [b"data/a".as_slice(), b"DATA/A", b"../bad", b"data/\0"] {
        let mut bytes = original.clone();
        let (local, central) = positions[3];
        bytes[local + 30..local + 36].copy_from_slice(replacement);
        bytes[central + 46..central + 52].copy_from_slice(replacement);
        assert!(read(&bytes).is_err(), "accepted invalid member {replacement:?}");
    }
    for attributes in [0x10u32, 0o120777 << 16, 0o040755 << 16, 0o140600 << 16] {
        let mut bytes = original.clone();
        let central = positions[2].1;
        bytes[central + 38..central + 42].copy_from_slice(&attributes.to_le_bytes());
        assert!(read(&bytes).is_err(), "accepted non-file attributes {attributes:x}");
    }
    let mut reordered = original.clone();
    let (a, b) = (positions[2].1, positions[3].1);
    let second = reordered[b..b + 52].to_vec();
    reordered.copy_within(a..a + 52, b);
    reordered[a..a + 52].copy_from_slice(&second);
    assert!(read(&reordered).is_err());
    let mut alias = original.clone();
    alias[positions[3].1 + 42..positions[3].1 + 46].copy_from_slice(&(positions[2].0 as u32).to_le_bytes());
    assert!(read(&alias).is_err());
    let mut concatenated = original.clone(); concatenated.extend(&original);
    assert!(read(&concatenated).is_err());
}

#[test]
fn terminal_inventory_and_member_reads_respect_their_independent_bounds() {
    let bytes = package(&[("data/a", b"pixels")]);
    let directory_size = u32_at(&bytes, bytes.len() - 10) as u64;
    assert!(Directory::read(&mut Cursor::new(&bytes), 2, directory_size).is_err());
    assert!(Directory::read(&mut Cursor::new(&bytes), 3, directory_size - 1).is_err());
    let directory = Directory::read(&mut Cursor::new(&bytes), 3, directory_size).unwrap();
    let member = directory.member("data/a").unwrap();
    assert!(directory.read_member(&mut Cursor::new(&bytes), member, 5).is_err());
    assert_eq!(directory.read_member(&mut Cursor::new(&bytes), member, 6).unwrap(), b"pixels");
    for index in [8, 10, 12, 16] {
        let mut damaged = bytes.clone();
        let offset = damaged.len() - 22 + index;
        damaged[offset] ^= 1;
        assert!(read(&damaged).is_err(), "accepted inconsistent end record field {index}");
    }
}

#[test]
fn signature_crc_exception_is_checked_through_the_real_member_reader() {
    for name in [CREDENTIAL, "META-INF/other.c2pa", "META-INF/CONTENT_CREDENTIAL.C2PA"] {
        let mut bytes = package(&[(name, b"unverified signature")]);
        let (local, central) = positions(&bytes)[2];
        bytes[local + 14..local + 18].fill(0);
        bytes[central + 16..central + 20].fill(0);
        let directory = read(&bytes).unwrap();
        let result = directory.read_member(&mut Cursor::new(&bytes), directory.member(name).unwrap(), 100);
        assert_eq!(result.is_ok(), name == CREDENTIAL);
    }
    let mut bytes = package(&[(CREDENTIAL, b"unverified signature")]);
    let (local, _) = positions(&bytes)[2];
    bytes[local + 14..local + 18].fill(0);
    assert!(read(&bytes).is_err());
}

#[derive(Clone)]
struct SparseArchive {
    chunks: Vec<(u64, Vec<u8>)>,
    length: u64,
    position: u64,
    read_bytes: usize,
}
impl Read for SparseArchive {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.length.saturating_sub(self.position).min(output.len() as u64) as usize;
        output[..count].fill(0);
        let end = self.position + count as u64;
        for (offset, bytes) in &self.chunks {
            let first = self.position.max(*offset);
            let last = end.min(*offset + bytes.len() as u64);
            if first < last {
                output[(first - self.position) as usize..(last - self.position) as usize]
                    .copy_from_slice(&bytes[(first - offset) as usize..(last - offset) as usize]);
            }
        }
        self.position = end;
        self.read_bytes += count;
        Ok(count)
    }
}
impl Seek for SparseArchive {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let position = match from {
            SeekFrom::Start(position) => i128::from(position),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.length) + i128::from(delta),
        };
        self.position = u64::try_from(position).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        Ok(self.position)
    }
}
impl SparseArchive {
    fn replace(&mut self, offset: u64, replacement: &[u8]) {
        let (start, chunk) = self.chunks.iter_mut().find(|(start, bytes)| offset >= *start
            && offset - start + replacement.len() as u64 <= bytes.len() as u64).unwrap();
        let offset = (offset - *start) as usize;
        chunk[offset..offset + replacement.len()].copy_from_slice(replacement);
    }
}

fn sparse_zip64(length: u64) -> (SparseArchive, Vec<u64>, Vec<u64>, u64) {
    let mut chunks = Vec::new();
    let mut local_positions = Vec::new();
    let mut directory = Vec::new();
    let mut relative_central = Vec::new();
    let mut offset = 0;
    for (name, length, payload) in [("mimetype", MIMETYPE.len() as u64, MIMETYPE),
        ("manifest.json", 2, b"{}".as_slice()), ("data/tiles-1.bin", length, b"".as_slice()),
        ("data/tail", 2, b"ok".as_slice())] {
        local_positions.push(offset);
        relative_central.push(directory.len() as u64);
        let (mut local, central) = headers(name, length, None, crc32fast::hash(payload), offset);
        let data_offset = offset + local.len() as u64;
        local.extend(payload);
        chunks.push((offset, local));
        directory.extend(central);
        offset = data_offset + length;
    }
    let central_positions = relative_central.into_iter().map(|relative| offset + relative).collect();
    let end_start = offset + directory.len() as u64;
    let end = tail(4, directory.len() as u64, offset, true).unwrap();
    let length = end_start + end.len() as u64;
    chunks.push((offset, directory)); chunks.push((end_start, end));
    (SparseArchive { chunks, length, position: 0, read_bytes: 0 }, local_positions, central_positions, end_start)
}

#[test]
fn large_member_and_later_small_member_keep_u64_offsets_without_reading_payloads() {
    for length in [LIMIT32 - 1, LIMIT32, LIMIT32 + 1] {
        let (mut input, local, _, _) = sparse_zip64(length);
        assert!(local[3] > LIMIT32);
        let directory = Directory::read(&mut input, 4, 1024).unwrap();
        let large = directory.member("data/tiles-1.bin").unwrap();
        assert_eq!(large.length, length);
        assert_eq!(large.offset + large.length, local[3]);
        let small = directory.member("data/tail").unwrap();
        assert!(small.offset > LIMIT32);
        assert_eq!(directory.read_member(&mut input, small, 2).unwrap(), b"ok");
        assert!(directory.read_member(&mut input, large, 1024).is_err());
        assert!(input.read_bytes < 256 * 1024, "directory scanned {} bytes", input.read_bytes);
        let mut independent = zip::ZipArchive::new(input).unwrap();
        assert_eq!(independent.by_name("data/tiles-1.bin").unwrap().size(), length);
        let mut payload = Vec::new(); independent.by_name("data/tail").unwrap().read_to_end(&mut payload).unwrap();
        assert_eq!(payload, b"ok");
    }
}

#[test]
fn large_archive_rejects_conflicting_zip64_values_and_offset_overflow() {
    let (original, local, central, end) = sparse_zip64(LIMIT32);
    let large_extra = local[2] + 30 + "data/tiles-1.bin".len() as u64;
    let small_extra = central[3] + 46 + "data/tail".len() as u64;
    let mutations = [
        (end + 4, 45u64.to_le_bytes().to_vec()),
        (end + 14, 20u16.to_le_bytes().to_vec()),
        (end + 16, 1u32.to_le_bytes().to_vec()),
        (end + 24, 5u64.to_le_bytes().to_vec()),
        (end + 64, (end + 1).to_le_bytes().to_vec()),
        (end + 72, 2u32.to_le_bytes().to_vec()),
        (large_extra, 2u16.to_le_bytes().to_vec()),
        (large_extra + 4, (LIMIT32 + 1).to_le_bytes().to_vec()),
        (local[3] + 4, 20u16.to_le_bytes().to_vec()),
        (small_extra + 4, u64::MAX.to_le_bytes().to_vec()),
        (end + 48, (u64::MAX - 1).to_le_bytes().to_vec()),
    ];
    for (offset, replacement) in mutations {
        let mut input = original.clone(); input.replace(offset, &replacement);
        assert!(Directory::read(&mut input, 4, 1024).is_err(), "accepted ZIP64 mutation at {offset}");
        assert!(input.read_bytes < 256 * 1024, "invalid directory scanned {} bytes", input.read_bytes);
    }
}


#[test]
fn compressed_manifest_is_lossless_bounded_and_keeps_resource_ranges_seekable() {
    let manifest = serde_json::to_vec(&serde_json::json!({"resources": (0..1024).map(|index|
        serde_json::json!({"id": format!("{index:032x}"), "type": "capy.tile/1", "bytes": "65536", "data": {"width":256,"height":256}})
    ).collect::<Vec<_>>()})).unwrap();
    let payload = (0..8192).map(|value| (value * 37) as u8).collect::<Vec<_>>();
    let bytes = package_manifest(&manifest, &[("data/tiles-1.bin", &payload)]);
    let directory = read(&bytes).unwrap();
    let member = directory.member("manifest.json").unwrap();
    assert!(member.compressed_length.unwrap() < member.length / 4);
    assert!(directory.read_member(&mut Cursor::new(&bytes), member, manifest.len() - 1).is_err());
    assert_eq!(directory.read_member(&mut Cursor::new(&bytes), member, manifest.len()).unwrap(), manifest);
    let pack = directory.member("data/tiles-1.bin").unwrap();
    assert_eq!(pack.compressed_length, None);
    assert_eq!(&bytes[pack.offset as usize..(pack.offset + pack.length) as usize], payload);
    let mut independent = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut restored = Vec::new();
    independent.by_name("manifest.json").unwrap().read_to_end(&mut restored).unwrap();
    assert_eq!(restored, manifest);
    assert_eq!(directory.members[0].compressed_length, None);
}

fn encoded_manifest(encoded: &[u8], length: u64, crc: u32) -> Vec<u8> {
    let (mut bytes, mut directory) = headers("mimetype", MIMETYPE.len() as u64, None, crc32fast::hash(MIMETYPE), 0);
    bytes.extend(MIMETYPE);
    let (local, central) = headers("manifest.json", length, Some(encoded.len() as u64), crc, bytes.len() as u64);
    bytes.extend(local); bytes.extend(encoded); directory.extend(central);
    let end = tail(2, directory.len() as u64, bytes.len() as u64, false).unwrap();
    bytes.extend(directory); bytes.extend(end); bytes
}

#[test]
fn compressed_metadata_rejects_corruption_expansion_mismatch_and_extra_streams() {
    let raw = vec![b'x'; 8192];
    let archive = package_manifest(&raw, &[]);
    let directory = read(&archive).unwrap();
    let member = directory.member("manifest.json").unwrap();
    let encoded = archive[member.offset as usize..(member.offset + member.compressed_length.unwrap()) as usize].to_vec();
    let mut trailing = encoded.clone(); trailing.push(0);
    let mut joined = encoded.clone(); joined.extend_from_slice(&encoded);
    let mut corrupt = encoded.clone(); corrupt[0] ^= 0xff;
    for (payload, length, crc) in [
        (encoded.clone(), 8191, member.crc32), (encoded.clone(), 8193, member.crc32),
        (encoded.clone(), 8192, member.crc32 ^ 1),
        (encoded[..encoded.len()-1].to_vec(), 8192, member.crc32),
        (trailing, 8192, member.crc32), (joined, 8192, member.crc32), (corrupt, 8192, member.crc32),
    ] {
        let bytes = encoded_manifest(&payload, length, crc);
        let directory = read(&bytes).unwrap();
        assert!(directory.read_member(&mut Cursor::new(&bytes), directory.member("manifest.json").unwrap(), 8193).is_err());
    }
    let bytes = encoded_manifest(&encoded, u64::from(u32::MAX)-1, member.crc32);
    assert!(read(&bytes).is_err());
    let mut bounded_output = Vec::new();
    let mut input = raw.as_slice();
    assert!(write_archive(&mut bounded_output, &mut [InputMember {name:"manifest.json", length:8192, crc32:member.crc32, input:&mut input}], 8191).is_err());
    assert!(bounded_output.is_empty());
}

#[test]
fn member_name_budget_is_unsupported() {
    let mut bytes=Vec::new();let mut directory=Vec::new();
    for (name,data) in [("mimetype".to_string(),MIMETYPE),("manifest.json".into(),b"{}".as_slice()),("x".repeat(256),b"optional".as_slice())] {
        let (local,central)=headers(&name,data.len() as u64,None,crc32fast::hash(data),bytes.len() as u64);
        bytes.extend(local);bytes.extend(data);directory.extend(central);
    }
    let end=tail(3,directory.len() as u64,bytes.len() as u64,false).unwrap();
    bytes.extend(directory);bytes.extend(end);
    assert!(matches!(read(&bytes),Err(DecodeError::Unsupported(_))));
}
