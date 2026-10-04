use super::*;
use crate::authored::PortableId;
use std::{collections::BTreeMap, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}};

#[test]
fn duplicate_keys_and_reserved_reference_ambiguity_are_rejected_before_interpretation() {
    for bytes in [br#"{"a":1,"a":1}"#.as_slice(), br#"{"unknown":{"x":null,"x":false}}"#, br#"{"a":1,"\u0061":2}"#] {
        assert!(parse_json(bytes, 4096).unwrap_err().to_string().contains("Duplicate JSON key"));
    }
    for bytes in [br#"{"ref":"00000000000000000000000000000001","fallback":1}"#.as_slice(), br#"{"ref":18446744073709551615}"#] {
        let value = parse_json(bytes, 4096).unwrap();
        assert!(references(&value, 100).is_err());
    }
    assert!(parse_json(b"{} {}", 4096).is_err());
    assert!(parse_json(b"[0,1,2]", 6).is_err());
}

#[test]
fn local_slots_and_literal_ids_survive_visible_reference_remapping() {
    let first: PortableId = "00000000000000000000000000000001".parse().unwrap();
    let second: PortableId = "00000000000000000000000000000002".parse().unwrap();
    let mut value = parse_json(br#"{
        "inputs":{"matte":{"object":{"ref":"00000000000000000000000000000001"},"port":"coverage"}},
        "literal":"00000000000000000000000000000001",
        "bindings":{"ink":{"ref":"00000000000000000000000000000002"}},"seed":77
    }"#, 4096).unwrap();
    let refs = references(&value, 100).unwrap();
    assert_eq!(refs.len(), 2);
    assert!(refs.contains(&first) && refs.contains(&second));
    remap_references(&mut value, &BTreeMap::from([(first, second)]), 100).unwrap();
    assert_eq!(references(&value, 100).unwrap(), [second, second]);
    assert_eq!(value["literal"], first.to_string());
    assert_eq!(value["seed"], 77);
    assert_eq!(value["inputs"]["matte"]["port"], "coverage");
    let before = value.clone();
    assert!(remap_references(&mut value, &BTreeMap::from([(second, first)]), 1).is_err());
    assert_eq!(value, before);
}

#[test]
fn owned_ranges_share_bytes_and_retain_only_the_original_owner() {
    let bytes: Arc<[u8]> = vec![10, 20, 30, 40, 50].into();
    let weak = Arc::downgrade(&bytes);
    let pointer = bytes.as_ptr();
    let range = ByteRange::new(bytes.clone(), 1..5).unwrap();
    let nested = range.slice(1..3).unwrap();
    assert_eq!(&*nested, [30, 40]);
    assert_eq!(nested.as_ptr(), pointer.wrapping_add(2));
    assert_eq!(nested.retained_bytes(), 5);
    assert!(range.slice(0..5).is_err());
    assert!(ByteRange::new(bytes.clone(), 4..3).is_err());
    drop(bytes); drop(range);
    assert!(weak.upgrade().is_some());
    drop(nested);
    assert!(weak.upgrade().is_none());
}

struct Source { ready: AtomicBool, failure: Mutex<Option<String>>, requests: Mutex<Vec<(u64, usize)>>, bytes: Arc<[u8]> }
impl ByteSource for Source {
    fn byte_len(&self) -> u64 { u64::MAX }
    fn poll(&self, offset: u64, len: usize) -> Result<RangeState, String> {
        self.requests.lock().unwrap().push((offset, len));
        if let Some(failure) = self.failure.lock().unwrap().as_ref() { return Err(failure.clone()); }
        Ok(if self.ready.load(Ordering::Relaxed) {
            RangeState::Ready(ByteRange::new(self.bytes.clone(), 0..len)?)
        } else { RangeState::Pending })
    }
}
#[test]
fn package_owners_preserve_large_offsets_readiness_failures_and_snapshot_lifetimes() {
    let source = Arc::new(Source { ready: AtomicBool::new(false), failure: Mutex::new(None), requests: Mutex::new(Vec::new()), bytes: vec![1, 2, 3].into() });
    let backing = ImmutableBacking::new(source.clone()).unwrap();
    let snapshot = backing.clone();
    let other_open = ImmutableBacking::new(source.clone()).unwrap();
    assert_eq!(backing.identity(), snapshot.identity());
    assert_ne!(backing.identity(), other_open.identity());
    let offset = (1u64 << 54) + 1;
    assert!(matches!(backing.poll(offset, 3).unwrap(), RangeState::Pending));
    assert!(backing.poll(u64::MAX, 1).is_err());
    assert!(backing.poll(0, MAX_RANGE_BYTES + 1).is_err());
    assert_eq!(*source.requests.lock().unwrap(), [(offset, 3)]);
    source.ready.store(true, Ordering::Relaxed);
    let RangeState::Ready(bytes) = snapshot.poll(offset, 3).unwrap() else { panic!() };
    assert_eq!(&*bytes, [1, 2, 3]);
    *source.failure.lock().unwrap() = Some("Corrupt package block".into());
    drop(backing);
    assert_eq!(snapshot.poll(offset, 3).unwrap_err(), "Corrupt package block");
    *source.failure.lock().unwrap() = None;
    assert_eq!(snapshot.poll(offset, 3).unwrap_err(), "Corrupt package block");
    assert_eq!(snapshot.poll_original(offset, 3).unwrap_err(), "Corrupt package block");
    assert!(matches!(other_open.poll(offset, 3).unwrap(), RangeState::Ready(_)));
}

#[test]
fn bounded_request_cannot_retain_a_whole_large_archive() {
    let bytes: Arc<[u8]> = vec![0; MAX_RANGE_BYTES + 1].into();
    let backing = ImmutableBacking::new(Arc::new(bytes.clone())).unwrap();
    let RangeState::Ready(range) = backing.poll(0, 1).unwrap() else {panic!()};
    assert_eq!(&*range, &[0]);
    assert_eq!(range.retained_bytes(), 1);
    let unbounded=Source {ready:AtomicBool::new(true),failure:Mutex::new(None),requests:Mutex::new(Vec::new()),bytes};
    let backing=ImmutableBacking::new(Arc::new(unbounded)).unwrap();
    assert!(backing.poll(0,1).unwrap_err().contains("oversized buffer"));
    assert!(backing.poll_original(0,1).is_err());
}
