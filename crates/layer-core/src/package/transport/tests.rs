use super::*;

#[test]
fn bounded_memory_crosses_chunks_without_retaining_the_archive() {
    let mut first = vec![7; MAX_RANGE_BYTES];
    first[MAX_RANGE_BYTES - 1] = 3;
    let backing = ImmutableBacking::new(Arc::new(ChunkedBytes::new(vec![first.into(), Arc::from([4, 5])]).unwrap())).unwrap();
    let RangeState::Ready(bytes) = backing.poll((MAX_RANGE_BYTES - 1) as u64, 3).unwrap() else { panic!() };
    assert_eq!(&*bytes, &[3, 4, 5]);
    assert_eq!(bytes.retained_bytes(), 3);
    let cancel = AtomicBool::new(false);
    let mut reader = BackingReader::new(&backing, &cancel);
    assert_eq!(reader.seek(SeekFrom::End(-2)).unwrap(), MAX_RANGE_BYTES as u64);
    let mut bytes = [0; 3];
    assert_eq!(reader.read(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes[..2], &[4, 5]);
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(reader.read(&mut bytes).unwrap_err().kind(), io::ErrorKind::Other);
    cancel.store(false, Ordering::Relaxed);
    reader.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(reader.read(&mut bytes).unwrap(), 3);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn private_spool_survives_the_provider_without_leaving_a_file() {
    let path = std::env::temp_dir().join(format!("capy-spool-test-{}", crate::authored::PortableId::random()));
    std::fs::create_dir(&path).unwrap();
    let cancelled = AtomicBool::new(false);
    let mut input = io::Cursor::new(vec![3; MAX_RANGE_BYTES + 1]);
    let backing = spool(&mut input, &path, (MAX_RANGE_BYTES + 1) as u64, &cancelled).unwrap();
    drop(input);
    let retained = backing.clone();
    drop(backing);
    let RangeState::Ready(bytes) = retained.poll(MAX_RANGE_BYTES as u64, 1).unwrap() else { panic!() };
    assert_eq!(&*bytes, &[3]);
    #[cfg(unix)]
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    drop(retained);
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    assert_eq!(&*bytes, &[3]);
    assert!(spool(&mut io::Cursor::new([1, 2]), &path, 1, &cancelled).is_err());
    cancelled.store(true, Ordering::Relaxed);
    assert!(spool(&mut io::Cursor::new([1]), &path, 1, &cancelled).is_err());
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
    std::fs::remove_dir(path).unwrap();
}
