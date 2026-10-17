use super::values::DecodeError;
use super::vector_records::*;
use crate::authored::vector::*;
use crate::color::RgbColor;
use std::sync::Arc;

fn color(r: f32) -> Paint { Paint::color(RgbColor::new(Default::default(), [r, 0.25, 0.5, 1.]).unwrap()) }
fn line(id: u64, points: &[[f64; 2]], width: f64) -> Record {
    let anchors: Vec<_> = points.iter().map(|p| Anchor { point: *p, in_handle: [-1., 0.5], out_handle: [1., -0.5], kind: AnchorKind::SMOOTH }).collect();
    let geometry = PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap();
    let mut stroke = Stroke::new(color(0.1), width);
    stroke.cap = Cap::Round;
    stroke.join = Join::Round;
    stroke.profile = Some(Arc::new(WidthProfile::new(vec![
        WidthKnot { segment: 0, t: 0., left: 0., right: 0. },
        WidthKnot { segment: 0, t: 0.5, left: 1., right: 0.5 },
        WidthKnot { segment: points.len().saturating_sub(2) as u32, t: 1., left: 0.2, right: 0.2 },
    ]).unwrap()));
    Record::Path(PathItem::new(ItemId(id), Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))
}
fn shape(id: u64) -> Record {
    let square = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]].map(Anchor::corner);
    let hole = [[3., 3.], [3., 7.], [7., 7.], [7., 3.]].map(Anchor::corner);
    let geometry = PathGeometry::new(vec![Subpath::new(&square, true), Subpath::new(&hole, true)]).unwrap();
    let mut stroke = Stroke::new(color(0.9), 2.);
    stroke.miter_limit = 10.;
    let mut path = PathItem::new(ItemId(id), Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: color(0.4), unknown: None }), PaintEntry::Stroke(stroke)])));
    path.fill_rule = FillRule::EvenOdd;
    path.name = Some("Badge".into());
    path.hidden = true;
    path.locked = true;
    Record::Path(path)
}
fn chunk(records: Vec<Record>) -> VectorChunk { VectorChunk::new(records) }
fn round_trip(records: Vec<Record>) -> (Vec<u8>, VectorChunk) {
    let original = chunk(records);
    let bytes = encode_chunk(&original);
    let decoded = decode_chunk(&bytes, original.records().len(), &mut 0).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(encode_chunk(&decoded), bytes);
    (bytes, decoded)
}
fn record_bytes(kind: u64, id: u64, properties: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut props = Vec::new();
    for (tag, value) in properties { put_varuint(&mut props, *tag); put_varuint(&mut props, value.len() as u64); props.extend_from_slice(value); }
    let mut out = Vec::new();
    put_varuint(&mut out, kind);
    if kind != 0 { out.extend_from_slice(&id.to_le_bytes()); put_varuint(&mut out, props.len() as u64); out.extend(props); }
    out
}
fn geometry_bytes() -> Vec<u8> {
    let Record::Path(path) = line(1, &[[0., 0.], [5., 5.]], 3.) else { unreachable!() };
    encode_geometry(&path.geometry)
}
fn is_unsupported(result: Result<VectorChunk, DecodeError>) -> bool { matches!(result, Err(DecodeError::Unsupported(_))) }
fn is_invalid(result: Result<VectorChunk, DecodeError>) -> bool { matches!(result, Err(DecodeError::Invalid(_))) }

#[test]
fn every_v0_property_round_trips_canonically() {
    let dot = Record::Path(PathItem::new(ItemId(3), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([4., 4.])], false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke { cap: Cap::Square, join: Join::Bevel, ..Stroke::new(color(0.), 6.) })]))));
    let empty = Record::Path(PathItem::new(ItemId(4), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([1., 2.]), Anchor::corner([3., 4.])], false)]).unwrap()), Arc::default()));
    let linear = Record::Path(PathItem::new(ItemId(5), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([1., 2.])], false)]).unwrap()),
        Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: Paint::color(RgbColor::from_linear(Default::default(), [0.123_456_7, 0.5, 0.25, 0.75]).unwrap()), unknown: None })]))));
    round_trip(vec![line(1, &[[0., 0.], [10., 4.], [20., 0.]], 4.), shape(2), dot, empty, linear]);
}

#[test]
fn defaults_are_omitted_and_framing_is_minimal() {
    let (bytes, _) = round_trip(vec![Record::Path(PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0., 0.])], false)]).unwrap()), Arc::default()))]);
    assert_eq!(bytes.len(), 1 + 8 + 1 + 2 + 3 + 49);
}

#[test]
fn long_paths_keep_runs_of_256_and_edit_one_run() {
    let anchors: Vec<_> = (0..1000).map(|i| Anchor::corner([f64::from(i), 0.])).collect();
    let subpath = Subpath::new(&anchors, false);
    assert_eq!(subpath.runs().iter().map(|r| r.len()).collect::<Vec<_>>(), vec![256, 256, 256, 232]);
    let edited = subpath.with_anchor(600, Anchor::corner([-1., -1.]));
    for (index, (a, b)) in subpath.runs().iter().zip(edited.runs()).enumerate() { assert_eq!(Arc::ptr_eq(a, b), index != 2); }
    assert_eq!(edited.anchor(600).point, [-1., -1.]);
    assert_eq!(edited.anchors().rev().nth(999).unwrap().point, [0., 0.]);
    round_trip(vec![Record::Path(PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![edited]).unwrap()), Arc::default()))]);
}

#[test]
fn unknown_ignorable_data_survives_and_required_data_is_unsupported() {
    let geometry = geometry_bytes();
    let keep = tag_word(900, IGNORABLE | COPY_SAFE, ValueClass::Data);
    let volatile = tag_word(901, IGNORABLE, ValueClass::Nested);
    let nested = record_bytes(kind_word(77) | IGNORABLE, 0, &[(tag_word(3, IGNORABLE, ValueClass::Items), 42u64.to_le_bytes().to_vec())]);
    let bytes = record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (keep, vec![1, 2, 3]), (volatile, nested.clone())]);
    let chunk = decode_chunk(&bytes, 1, &mut 0).unwrap();
    assert_eq!(encode_chunk(&chunk), bytes);
    let Record::Path(path) = &chunk.records()[0] else { panic!() };
    let moved = path.with_geometry(path.geometry.clone());
    assert_eq!(moved.unknown.as_ref().unwrap().iter().map(|p| p.tag).collect::<Vec<_>>(), vec![keep]);
    let renamed = PathItem { name: Some("Renamed".into()), ..path.clone() };
    assert_eq!(renamed.unknown, path.unknown);

    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (tag_word(902, 0, ValueClass::Data), vec![])]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (tag_word(902, IGNORABLE | 16, ValueClass::Data), vec![])]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (WIDTH, 1f64.to_le_bytes().to_vec())]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50), 3, &[]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50) | IGNORABLE | KIND_PARENT | 8, 3, &[]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50) | COPY_SAFE, 3, &[]), 1, &mut 0)));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (FILL_RULE, vec![7])]), 1, &mut 0)));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry), (FILL_RULE, vec![0, 0])]), 1, &mut 0)));
}

#[test]
fn unknown_parents_are_atomic_and_may_span_chunks() {
    let parent = kind_word(60) | IGNORABLE | COPY_SAFE | KIND_PARENT;
    let mut first = record_bytes(parent, 11, &[]);
    first.extend(encode_chunk(&chunk(vec![line(12, &[[0., 0.], [1., 1.]], 1.)])));
    let mut second = record_bytes(0, 0, &[]);
    second.extend(encode_chunk(&chunk(vec![line(13, &[[2., 2.], [3., 3.]], 1.)])));
    let chunks = decode_object(&[(&first, 2), (&second, 2)]).unwrap();
    assert!(matches!(chunks[0].records()[0], Record::Opaque(_)));
    assert!(matches!(chunks[0].records()[1], Record::Opaque(_)), "children of an unknown parent stay uninterpreted");
    assert!(matches!(chunks[1].records()[0], Record::End));
    assert!(matches!(chunks[1].records()[1], Record::Path(_)));
    assert_eq!(encode_chunk(&chunks[0]), first);
    assert_eq!(encode_chunk(&chunks[1]), second);
    assert!(matches!(decode_object(&[(&first, 2)]), Err(DecodeError::Invalid(_))));
    assert!(matches!(decode_object(&[(&second, 2)]), Err(DecodeError::Invalid(_))));
}

#[test]
fn malformed_framing_is_invalid() {
    let good = encode_chunk(&chunk(vec![line(1, &[[0., 0.], [3., 1.]], 2.)]));
    for cut in 1..good.len() { assert!(decode_chunk(&good[..cut], 1, &mut 0).is_err(), "truncated at {cut}"); }
    assert!(is_invalid(decode_chunk(&good, 2, &mut 0)));
    let mut padded = good.clone();
    padded.push(0);
    assert!(is_invalid(decode_chunk(&padded, 1, &mut 0)));
    let mut overlong = vec![PATH as u8 | 0x80, 0];
    overlong.extend_from_slice(&good[1..]);
    assert!(is_invalid(decode_chunk(&overlong, 1, &mut 0)));
    let geometry = geometry_bytes();
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(FILL_RULE, vec![1]), (GEOMETRY, geometry.clone())]), 1, &mut 0)));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (GEOMETRY, geometry.clone())]), 1, &mut 0)));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 0, &[(GEOMETRY, geometry.clone())]), 1, &mut 0)));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[]), 1, &mut 0)));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH | IGNORABLE, 8, &[(GEOMETRY, geometry.clone())]), 1, &mut 0)));
    let mut nan = geometry.clone();
    let at = nan.len() - 8;
    nan[at..].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, nan)]), 1, &mut 0)));
    let two = [encode_chunk(&chunk(vec![line(1, &[[0., 0.], [1., 1.]], 1.)])), encode_chunk(&chunk(vec![line(1, &[[0., 0.], [1., 1.]], 1.)]))];
    assert!(matches!(decode_object(&[(&two[0], 1), (&two[1], 1)]), Err(DecodeError::Invalid(_))));
}

#[test]
fn registry_numbers_are_unique_and_listed() {
    let listed: Vec<String> = include_str!("vector_numbers.tsv").lines().filter(|l| !l.starts_with('#')).map(str::to_owned).collect();
    let mut generated = Vec::new();
    for (space, table, shift) in [("kind", KINDS, 4), ("tag", TAGS, 5)] {
        for (index, number) in table.iter().enumerate() {
            assert_eq!(number.word >> shift, index as u64 + 1, "{space} numbers are dense and ordered");
            generated.push(format!("{space}\t{}\t{}\t{}", number.word >> shift, number.word & ((1 << shift) - 1), number.name));
        }
    }
    assert_eq!(generated, listed, "update vector_numbers.tsv by appending; never renumber");
}

#[test]
fn seeded_unknown_properties_survive_every_position() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state };
    for _ in 0..200 {
        let number = 1000 + next() % 1000;
        let flags = next() % 4;
        let class = [ValueClass::Data, ValueClass::Items, ValueClass::Nested][(next() % 3) as usize];
        let value = match class {
            ValueClass::Items => (0..next() % 3).flat_map(|_| next().to_le_bytes()).collect(),
            ValueClass::Nested => record_bytes(kind_word(500 + next() % 50) | IGNORABLE, next(), &[]),
            _ => (0..next() % 9).map(|_| next() as u8).collect(),
        };
        let tag = tag_word(number, flags, class);
        let bytes = record_bytes(PATH, 1 + next() % 1000, &[(GEOMETRY, geometry_bytes()), (tag, value)]);
        let decoded = decode_chunk(&bytes, 1, &mut 0);
        if flags & IGNORABLE == 0 { assert!(is_unsupported(decoded)); continue; }
        let chunk = decoded.unwrap();
        assert_eq!(encode_chunk(&chunk), bytes);
        let Record::Path(path) = &chunk.records()[0] else { panic!() };
        let kept = path.with_style(path.style.clone()).unknown.is_some();
        assert_eq!(kept, flags & COPY_SAFE != 0);
    }
}

#[test]
fn chunk_edits_rewrite_only_touched_chunks() {
    let object = VectorObject::from_records((1..=200).map(|i| line(i, &[[0., 0.], [i as f64, 1.]], 1.)).collect());
    assert!(object.chunks().iter().all(|c| c.records().len() <= CHUNK_RECORDS));
    assert_eq!(object.path_count(), 200);
    let target = object.find(ItemId(150)).unwrap();
    let edited = object.replace([(target, Some(line(150, &[[5., 5.], [6., 6.]], 3.)))]);
    for (index, (a, b)) in object.chunks().iter().zip(edited.chunks()).enumerate() { assert_eq!(a.same_owner(b), index != target.chunk as usize); }
    let removed = object.replace([(target, None)]);
    assert_eq!(removed.path_count(), 199);
    assert!(removed.find(ItemId(150)).is_none());
    let mut drawn = VectorObject::default();
    for i in 0..300 { drawn = drawn.insert_front(vec![line(1000 + i, &[[0., 0.], [1., 1.]], 1.)]); }
    assert_eq!(drawn.paths().next().unwrap().1.id, ItemId(1299));
    assert!(drawn.chunks().iter().all(|c| (1..=CHUNK_RECORDS).contains(&c.records().len())));
    let before = drawn.clone();
    let after = drawn.insert_front(vec![line(5000, &[[0., 0.], [1., 1.]], 1.)]);
    assert!(before.chunks()[1..].iter().zip(&after.chunks()[after.chunks().len() - (before.chunks().len() - 1)..]).all(|(a, b)| a.same_owner(b)));
}

fn document_with_paths(count: u64) -> (crate::Editor, crate::OccurrenceHandle) {
    let artwork = crate::Artwork::new([640, 480]).unwrap();
    let mut editor = crate::Editor::new(crate::Document::from_artwork(artwork).unwrap());
    let records = (1..=count).map(|i| if i % 7 == 0 { shape(i) } else { line(i, &[[i as f64, 3.], [i as f64 + 20., 40.], [i as f64 + 5., 90.]], 2. + (i % 5) as f64) }).collect();
    let (layer, edit) = editor.document().create_vector_layer_edit("Ink", VectorObject::from_records(records), None, 0).unwrap();
    editor.perform(edit).unwrap();
    (editor, layer)
}
fn save(editor: &crate::Editor) -> Vec<u8> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let capture = editor.capture(1, Default::default()).unwrap();
    let mut bytes = Vec::new();
    super::codec::PreparedPackage::prepare(&capture, None, &cancel).unwrap().write(&mut bytes, &cancel).unwrap();
    bytes
}
fn open(bytes: Vec<u8>) -> crate::Artwork {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let backing = super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();
    match super::codec::open(backing, Default::default(), &cancel).unwrap() {
        super::codec::OpenOutcome::Candidate { artwork, .. } => artwork,
        outcome => panic!("{outcome:?}"),
    }
}
fn vector(artwork: &crate::Artwork) -> &VectorObject { artwork.vectors.iter().next().unwrap().2 }

#[test]
fn vector_layers_round_trip_through_packages_and_keep_unchanged_chunks() {
    let (mut editor, layer) = document_with_paths(300);
    let original = editor.document().artwork.clone();
    let reopened = open(save(&editor));
    assert_eq!(vector(&reopened), vector(&original));
    let occurrence = reopened.occurrences.iter().find(|(_, _, o)| matches!(o.content, crate::OccurrenceContent::Vector(_))).unwrap().2;
    assert_eq!(occurrence.name.as_ref(), "Ink");
    let ids: Vec<_> = vector(&reopened).chunks().iter().map(|c| c.id()).collect();
    assert_eq!(ids, vector(&original).chunks().iter().map(|c| c.id()).collect::<Vec<_>>());

    let mut reloaded = crate::Editor::new(crate::Document::from_artwork(reopened).unwrap());
    let reloaded_layer = reloaded.document().scene().order().iter().copied().find(|h| reloaded.document().scene().vector_handle(*h).is_some()).unwrap();
    let object = reloaded.document().scene().vector_layer(reloaded_layer).unwrap().clone();
    let target = object.find(ItemId(150)).unwrap();
    let edited = object.replace([(target, Some(line(150, &[[1., 1.], [2., 2.]], 9.)))]);
    reloaded.perform(reloaded.document().vector_object_edit(reloaded_layer, edited).unwrap()).unwrap();
    let saved = open(save(&reloaded));
    let before = vector(&original);
    for (index, chunk) in vector(&saved).chunks().iter().enumerate() {
        assert_eq!(chunk.id() == before.chunks()[index].id(), index != target.chunk as usize);
    }
    assert!(reloaded.undo().unwrap());
    assert_eq!(reloaded.document().scene().vector_layer(reloaded_layer).unwrap(), before);

    editor.perform(editor.document().vector_object_edit(layer, VectorObject::default()).unwrap()).unwrap();
    assert_eq!(vector(&open(save(&editor))).record_count(), 0);
}

#[test]
fn vector_history_survives_sessions_and_worker_transfer() {
    use super::{session::PreparedSession, session_transfer::{PreparedSessionTransfer, SessionTransferReceiver}};
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let limits = crate::ProjectLimits::default();
    let (mut editor, layer) = document_with_paths(130);
    let first = editor.document().artwork.clone();
    let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![line(9000, &[[0., 0.], [50., 50.]], 4.)]);
    editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
    let drawn = editor.document().artwork.clone();
    editor.undo().unwrap();
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let session = PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
    let mut pack = Vec::new();
    std::io::Read::read_to_end(&mut session.resources().reader(&cancel), &mut pack).unwrap();
    let mut loaded = super::session::open_parts(session.metadata(), super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(), limits, &cancel).unwrap();
    assert_eq!(vector(&loaded.editor.document().artwork), vector(&first));
    loaded.editor.redo().unwrap();
    assert_eq!(vector(&loaded.editor.document().artwork), vector(&drawn));

    let transfer = PreparedSessionTransfer::capture(&capture, serde_json::json!({}), &cancel).unwrap();
    let descriptor = serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
    let mut receiver = SessionTransferReceiver::new(descriptor, limits).unwrap();
    for index in receiver.missing_payloads() {
        let length = transfer.payload_len(index).unwrap();
        let mut offset = 0;
        while offset < length {
            let count = (length - offset).min(super::MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index, &transfer.read_chunk(index, offset, count).unwrap()).unwrap();
            offset += count as u64;
        }
    }
    let mut adopted = receiver.finish().unwrap().adopt_verified(limits, &cancel).unwrap();
    assert_eq!(vector(&adopted.editor.document().artwork), vector(&first));
    adopted.editor.redo().unwrap();
    assert_eq!(vector(&adopted.editor.document().artwork), vector(&drawn));
}
