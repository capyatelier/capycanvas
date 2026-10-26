use super::values::DecodeError;
use super::vector_records::*;
use crate::authored::vector::*;
use crate::color::RgbColor;
use std::{collections::BTreeSet, sync::Arc};
use crate::authored::Affine64;

fn color(r: f32) -> Paint { Paint::color(RgbColor::new(Default::default(), [r, 0.25, 0.5, 1.]).unwrap()) }
fn line(id: u64, points: &[[f64; 2]], width: f64) -> Record {
    let anchors: Vec<_> = points.iter().map(|p| Anchor::new(*p, [-1., 0.5], [1., -0.5], AnchorKind::SMOOTH)).collect();
    let geometry = PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap();
    let mut stroke = Stroke::new(color(0.1), width);
    stroke.cap = Cap::Round;
    stroke.join = Join::Round;
    stroke.profile = Some(Arc::new(WidthProfile::new(vec![
        WidthKnot::new(0, 0., 0., 0.),
        WidthKnot::new(0, 0.5, 1., 0.5),
        WidthKnot::new(points.len().saturating_sub(2) as u32, 1., 0.2, 0.2),
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
    let decoded = decode_chunk(&bytes, original.records().len(), &mut Reading::default()).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(encode_chunk(&decoded), bytes);
    (bytes, decoded)
}
fn record_bytes(kind: u64, id: u64, properties: &[(u64, Vec<u8>)]) -> Vec<u8> {
    let mut props = Vec::new();
    for (tag, value) in properties { put_varuint(&mut props, *tag); put_varuint(&mut props, value.len() as u64); props.extend_from_slice(value); }
    let mut out = Vec::new();
    put_varuint(&mut out, kind);
    if kind != 0 { put_varuint(&mut out, id); put_varuint(&mut out, props.len() as u64); out.extend(props); }
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
    assert_eq!(bytes.len(), 1 + 1 + 1 + 2 + 3 + 3);
}

#[test]
fn long_paths_keep_runs_of_256_and_edit_one_run() {
    let anchors: Vec<_> = (0..1000).map(|i| Anchor::corner([f64::from(i), 0.])).collect();
    let subpath = Subpath::new(&anchors, false);
    assert_eq!(subpath.runs().iter().map(|r| r.len()).collect::<Vec<_>>(), vec![256, 256, 256, 232]);
    let edited = subpath.with_anchor(600, Anchor::corner([-1., -1.]));
    for (index, (a, b)) in subpath.runs().iter().zip(edited.runs()).enumerate() { assert_eq!(Arc::ptr_eq(a, b), index != 2); }
    assert_eq!(edited.anchor(600).point(), [-1., -1.]);
    assert_eq!(edited.anchors().rev().nth(999).unwrap().point(), [0., 0.]);
    round_trip(vec![Record::Path(PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![edited]).unwrap()), Arc::default()))]);
}

#[test]
fn unknown_ignorable_data_survives_and_required_data_is_unsupported() {
    let geometry = geometry_bytes();
    let keep = tag_word(900, IGNORABLE | COPY_SAFE, ValueClass::Data);
    let volatile = tag_word(901, IGNORABLE, ValueClass::Nested);
    let nested = record_bytes(kind_word(77) | IGNORABLE, 0, &[(tag_word(3, IGNORABLE, ValueClass::Items), vec![42])]);
    let bytes = record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (keep, vec![1, 2, 3]), (volatile, nested.clone())]);
    let chunk = decode_chunk(&bytes, 1, &mut Reading::default()).unwrap();
    assert_eq!(encode_chunk(&chunk), bytes);
    let Record::Path(path) = &chunk.records()[0] else { panic!() };
    let moved = path.with_geometry(path.geometry.clone());
    assert_eq!(moved.unknown.as_ref().unwrap().iter().map(|p| p.tag).collect::<Vec<_>>(), vec![keep]);
    let renamed = PathItem { name: Some("Renamed".into()), ..path.clone() };
    assert_eq!(renamed.unknown, path.unknown);

    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (tag_word(902, 0, ValueClass::Data), vec![])]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (tag_word(902, IGNORABLE | 16, ValueClass::Data), vec![])]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (WIDTH, 1f64.to_le_bytes().to_vec())]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50), 3, &[]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50) | IGNORABLE | KIND_PARENT | 8, 3, &[]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(kind_word(50) | COPY_SAFE, 3, &[]), 1, &mut Reading::default())));
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (FILL_RULE, vec![7])]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry), (FILL_RULE, vec![0, 0])]), 1, &mut Reading::default())));
}

#[test]
fn unknown_parents_are_atomic_and_may_span_chunks() {
    let parent = kind_word(60) | IGNORABLE | COPY_SAFE | KIND_PARENT;
    let mut first = record_bytes(parent, 11, &[]);
    first.extend(encode_chunk(&chunk(vec![line(12, &[[0., 0.], [1., 1.]], 1.)])));
    let mut second = record_bytes(0, 0, &[]);
    second.extend(encode_chunk(&chunk(vec![line(13, &[[2., 2.], [3., 3.]], 1.)])));
    let chunks = decode_object(&[(&first, 2), (&second, 2)], &mut Reading::default()).unwrap();
    assert!(matches!(chunks[0].records()[0], Record::Opaque(_)));
    assert!(matches!(chunks[0].records()[1], Record::Opaque(_)), "children of an unknown parent stay uninterpreted");
    assert!(matches!(chunks[1].records()[0], Record::End));
    assert!(matches!(chunks[1].records()[1], Record::Path(_)));
    assert_eq!(encode_chunk(&chunks[0]), first);
    assert_eq!(encode_chunk(&chunks[1]), second);
    assert!(matches!(decode_object(&[(&first, 2)], &mut Reading::default()), Err(DecodeError::Invalid(_))));
    assert!(matches!(decode_object(&[(&second, 2)], &mut Reading::default()), Err(DecodeError::Invalid(_))));
}

#[test]
fn extreme_geometry_integers_are_refused_without_overflow() {
    for value in [i64::MIN, i64::MAX, -(1 << 31), 1 << 31] {
        for header in [0, 1, 2, 3] {
            let mut geometry = vec![1, 1, 0, header];
            put_zigzag(&mut geometry, value);
            put_zigzag(&mut geometry, 0);
            geometry.extend([0; 4]);
            assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 1, &[(GEOMETRY, geometry)]), 1, &mut Reading::default())));
        }
    }
    for value in [i64::MIN, i64::MAX] {
        let mut geometry = vec![1, 1, 0, 1, 0, 0];
        put_zigzag(&mut geometry, value);
        put_zigzag(&mut geometry, 0);
        assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 1, &[(GEOMETRY, geometry)]), 1, &mut Reading::default())));
    }
}

#[test]
fn malformed_framing_is_invalid() {
    let good = encode_chunk(&chunk(vec![line(1, &[[0., 0.], [3., 1.]], 2.)]));
    for cut in 1..good.len() { assert!(decode_chunk(&good[..cut], 1, &mut Reading::default()).is_err(), "truncated at {cut}"); }
    assert!(is_invalid(decode_chunk(&good, 2, &mut Reading::default())));
    let mut padded = good.clone();
    padded.push(0);
    assert!(is_invalid(decode_chunk(&padded, 1, &mut Reading::default())));
    let mut overlong = vec![PATH as u8 | 0x80, 0];
    overlong.extend_from_slice(&good[1..]);
    assert!(is_invalid(decode_chunk(&overlong, 1, &mut Reading::default())));
    let geometry = geometry_bytes();
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(FILL_RULE, vec![1]), (GEOMETRY, geometry.clone())]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, geometry.clone()), (GEOMETRY, geometry.clone())]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 0, &[(GEOMETRY, geometry.clone())]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH, 8, &[]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&record_bytes(PATH | IGNORABLE, 8, &[(GEOMETRY, geometry.clone())]), 1, &mut Reading::default())));
    let mut far = vec![1, 1, 0, 0];
    put_zigzag(&mut far, 1 << 31);
    put_zigzag(&mut far, 0);
    assert!(is_unsupported(decode_chunk(&record_bytes(PATH, 8, &[(GEOMETRY, far)]), 1, &mut Reading::default())));
    let two = [encode_chunk(&chunk(vec![line(1, &[[0., 0.], [1., 1.]], 1.)])), encode_chunk(&chunk(vec![line(1, &[[0., 0.], [1., 1.]], 1.)]))];
    assert!(matches!(decode_object(&[(&two[0], 1), (&two[1], 1)], &mut Reading::default()), Err(DecodeError::Invalid(_))));
}

#[test]
fn seeded_byte_mutations_never_panic_and_decode_to_stable_records() {
    let records = vec![line(1, &[[0., 0.], [3., 1.], [9., -4.]], 2.), shape(2), Record::Path(live(3, ShapeKind::Rectangle { radii: [4.; 4] })), Record::Path(live(4, ShapeKind::Ellipse))];
    let good = encode_chunk(&chunk(records.clone()));
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state };
    let mut decoded = 0;
    for _ in 0..20_000 {
        let mut bytes = good.clone();
        for _ in 0..1 + next() % 4 {
            let at = (next() as usize) % bytes.len().max(1);
            match next() % 5 {
                0 => bytes[at] ^= 1 << (next() % 8),
                1 => bytes[at] = 0xff,
                2 => { let end = (at + 1 + (next() as usize) % 12).min(bytes.len()); bytes.drain(at..end); }
                3 => { let end = (at + 1 + (next() as usize) % 12).min(bytes.len()); let copy = bytes[at..end].to_vec(); bytes.splice(at..at, copy); }
                _ => bytes.splice(at..at, [0xff, 0xff, 0xff, 0xff, 0x0f]).for_each(drop),
            }
            if bytes.is_empty() { break; }
        }
        let Ok(chunk) = decode_chunk(&bytes, records.len(), &mut Reading::default()) else { continue };
        decoded += 1;
        let again = encode_chunk(&chunk);
        assert_eq!(decode_chunk(&again, chunk.records().len(), &mut Reading::default()).as_ref(), Ok(&chunk));
        assert_eq!(encode_chunk(&decode_chunk(&again, chunk.records().len(), &mut Reading::default()).unwrap()), again);
    }
    assert!(decoded > 0, "some mutations land in values that stay valid");
}

#[test]
fn registry_numbers_are_unique_and_listed() {
    let listed: Vec<String> = include_str!("vector_numbers.tsv").lines().filter(|l| !l.starts_with('#')).map(str::to_owned).collect();
    let mut generated = Vec::new();
    for (space, table, shift) in [("kind", KINDS, 4), ("tag", TAGS, 5)] {
        for (index, number) in table.iter().enumerate() {
            assert_eq!(number.word >> shift, index as u64 + 1, "{space} numbers are dense and ordered");
            generated.push(format!("{space}\t{}\t{}\t{}\t{}", number.word >> shift, number.word & ((1 << shift) - 1), number.name, number.since));
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
            ValueClass::Items => (0..next() % 3).fold(Vec::new(), |mut list, _| { put_varuint(&mut list, 1 + next() % 1000); list }),
            ValueClass::Nested => record_bytes(kind_word(500 + next() % 50) | IGNORABLE, next(), &[]),
            _ => (0..next() % 9).map(|_| next() as u8).collect(),
        };
        let tag = tag_word(number, flags, class);
        let bytes = record_bytes(PATH, 1 + next() % 1000, &[(GEOMETRY, geometry_bytes()), (tag, value)]);
        let decoded = decode_chunk(&bytes, 1, &mut Reading::default());
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
fn vector_edits_reject_identities_and_markers_that_break_the_object() {
    let (mut editor, layer) = document_with_paths(300);
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    assert!(object.chunks().len() > 3);
    let far = object.find(ItemId(290)).unwrap();
    assert!(far.chunk > 0);
    let rejected = |editor: &mut crate::Editor, next: VectorObject| {
        let edit = editor.document().vector_object_edit(layer, next).unwrap();
        assert!(editor.perform(edit).is_err());
    };
    rejected(&mut editor, object.insert_front(vec![line(290, &[[0., 0.], [1., 1.]], 1.)]));
    rejected(&mut editor, object.replace([(object.find(ItemId(2)).unwrap(), Some(line(290, &[[0., 0.], [1., 1.]], 1.)))]));
    rejected(&mut editor, object.insert_front(vec![Record::End]));
    let id = object.allocate_id();
    assert!(!object.contains(id) && object.contains(ItemId(290)));
    let drawn = object.insert_front(vec![line(id.0, &[[0., 0.], [1., 1.]], 1.)]);
    editor.perform(editor.document().vector_object_edit(layer, drawn).unwrap()).unwrap();
    assert_eq!(editor.document().scene().vector_layer(layer).unwrap().find(id), Some(Position { chunk: 0, index: 0 }));
}

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

#[test]
fn session_history_shares_unchanged_runs_of_vector_chunk_lists() {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let (mut editor, layer) = document_with_paths(128 * CHUNK_RECORDS as u64);
    let first = editor.document().artwork.clone();
    let strokes = 40;
    for i in 0..strokes {
        let object = editor.document().scene().vector_layer(layer).unwrap().insert_front(vec![line(100_000 + i, &[[0., 0.], [50., i as f64]], 4.)]);
        editor.perform(editor.document().vector_object_edit(layer, object).unwrap()).unwrap();
    }
    let chunks = vector(&editor.document().artwork).chunks().len();
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let session = super::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(session.metadata()).unwrap();
    let runs = metadata["index_chunks"].as_array().unwrap();
    let listed: usize = runs.iter().map(|run| run.as_array().unwrap().len()).sum();
    let max_run = runs.iter().map(|run| run.as_array().unwrap().len()).max().unwrap_or(0);
    assert!(max_run <= chunks / 2, "{max_run} references in one run for {chunks} chunks");
    assert!(listed <= strokes as usize * max_run + 2 * chunks, "{listed} chunk references for {chunks} chunks and {max_run}-reference runs");
    let mut pack = Vec::new();
    std::io::Read::read_to_end(&mut session.resources().reader(&cancel), &mut pack).unwrap();
    let mut loaded = super::session::open_parts(session.metadata(), super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(), crate::ProjectLimits::default(), &cancel).unwrap();
    assert_eq!(vector(&loaded.editor.document().artwork), vector(&editor.document().artwork));
    for _ in 0..strokes { loaded.editor.undo().unwrap(); }
    assert_eq!(vector(&loaded.editor.document().artwork), vector(&first));
}

fn plain_line(id: u64, from: [f64; 2], to: [f64; 2], width: f64) -> PathItem {
    let geometry = PathGeometry::new(vec![Subpath::new(&[Anchor::corner(from), Anchor::corner(to)], false)]).unwrap();
    PathItem::new(ItemId(id), Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(color(0.1), width))])))
}

#[test]
fn hits_find_the_frontmost_visible_unlocked_path_under_the_point() {
    let Record::Path(mut badge) = shape(3) else { unreachable!() };
    (badge.hidden, badge.locked) = (false, false);
    let mut locked = plain_line(4, [0., 50.], [100., 50.], 6.);
    locked.locked = true;
    let object = VectorObject::from_records(vec![Record::Path(plain_line(1, [0., 0.], [100., 0.], 4.)), Record::Path(plain_line(2, [50., -20.], [50., 20.], 2.)), Record::Path(badge), Record::Path(locked)]);
    let mut index = VectorIndex::default();
    assert_eq!(index.hit(&object, [50., 0.], 0.), Some(ItemId(1)), "the front path wins where paths overlap");
    assert_eq!(index.hit(&object, [50., 10.], 0.), Some(ItemId(2)));
    assert_eq!(index.hit(&object, [20., 1.9], 0.), Some(ItemId(1)));
    assert_eq!(index.hit(&object, [20., 2.5], 0.), None);
    assert_eq!(index.hit(&object, [20., 2.5], 1.), Some(ItemId(1)));
    assert_eq!(index.hit(&object, [1.5, 5.], 0.), Some(ItemId(3)), "filled area");
    assert_eq!(index.hit(&object, [5., 5.], 0.), None, "even-odd hole");
    assert_eq!(index.hit(&object, [20., 50.], 0.), None, "locked paths are not picked");
}

#[test]
fn transforming_items_rewrites_only_their_chunks_and_scales_stroke_widths() {
    let object = VectorObject::from_records((1..=300).map(|i| Record::Path(plain_line(i, [i as f64, 0.], [i as f64, 10.], 2.))).collect());
    let few: BTreeSet<ItemId> = [ItemId(5), ItemId(250)].into();
    let many: BTreeSet<ItemId> = (1..=300).filter(|i| i % 3 == 0).map(ItemId).collect();
    for ids in [&few, &many] {
        let expected: BTreeSet<_> = ids.iter().map(|id| object.find(*id).unwrap()).collect();
        let found: BTreeSet<_> = object.positions(ids).into_iter().flat_map(|(c, v)| v.into_iter().map(move |i| Position { chunk: c as u32, index: i as u32 })).collect();
        assert_eq!(found, expected);
    }
    let moved = object.transform_items(&few, Affine64([2., 0., 0., 2., 1000., 0.]), true).unwrap();
    moved.validate().unwrap();
    let touched: BTreeSet<_> = few.iter().map(|id| object.find(*id).unwrap().chunk as usize).collect();
    for (index, (a, b)) in object.chunks().iter().zip(moved.chunks()).enumerate() { assert_eq!(a.same_owner(b), !touched.contains(&index)); }
    let path = moved.record(moved.find(ItemId(5)).unwrap()).unwrap().path().unwrap();
    assert_eq!(path.geometry.subpaths()[0].anchor(1).point(), [1010., 20.]);
    assert_eq!(path.style.strokes().next().unwrap().width, 4.);
    assert_eq!(moved.items_bounds(&few), Some([1002., -8., 1508., 28.]));
}

#[test]
fn path_selections_keep_only_paths_that_still_exist() {
    let (mut editor, layer) = document_with_paths(200);
    let mut working = editor.document().working.clone();
    working.vector_items = Some(crate::VectorItemSelection { layer, items: Arc::new([ItemId(3), ItemId(150), ItemId(9999)].into()) });
    editor.perform(crate::Edit::Working(working)).unwrap();
    let selected = |editor: &crate::Editor| editor.document().working.vector_items.as_ref().map(|s| s.items.iter().copied().collect::<Vec<_>>());
    assert_eq!(selected(&editor), Some(vec![ItemId(3), ItemId(150)]), "missing paths drop out of the selection");
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let removed = object.replace([(object.find(ItemId(150)).unwrap(), None)]);
    editor.perform(editor.document().vector_object_edit(layer, removed).unwrap()).unwrap();
    assert_eq!(selected(&editor), Some(vec![ItemId(3)]));
    let object = editor.document().scene().vector_layer(layer).unwrap().clone();
    let removed = object.replace([(object.find(ItemId(3)).unwrap(), None)]);
    editor.perform(editor.document().vector_object_edit(layer, removed).unwrap()).unwrap();
    assert_eq!(selected(&editor), None, "an empty path selection clears");
}

fn live(id: u64, kind: ShapeKind) -> PathItem { rotated(id, kind, 0.3) }
fn rotated(id: u64, kind: ShapeKind, rotation: f64) -> PathItem {
    let shape = LiveShape { kind, center: [120., 80.], size: [90., 40.], rotation };
    let base = PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0.; 2])], false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: color(0.4), unknown: None })])));
    base.with_shape(shape).unwrap()
}
fn near(a: &PathGeometry, b: &PathGeometry) -> bool {
    a.anchor_count() == b.anchor_count() && a.subpaths().iter().zip(b.subpaths()).all(|(a, b)| a.anchors().zip(b.anchors()).all(|(a, b)|
        a.bits().iter().zip(b.bits()).all(|(x, y)| f64::from((x - y).abs()) <= 3. / GRID)))
}

#[test]
fn live_shapes_round_trip_and_detach_when_a_foreign_writer_changed_their_geometry() {
    let shapes = vec![Record::Path(live(1, ShapeKind::Rectangle { radii: [0., 6., 12., 3.] })), Record::Path(live(2, ShapeKind::Ellipse))];
    let (bytes, decoded) = round_trip(shapes.clone());
    for (record, original) in decoded.records().iter().zip(&shapes) {
        let (path, original) = (record.path().unwrap(), original.path().unwrap());
        assert_eq!(path.shape, original.shape);
        assert!(near(&path.geometry, &path.shape.as_ref().unwrap().geometry().unwrap()));
    }
    assert!(!bytes.is_empty());
    let Record::Path(edited) = &shapes[0] else { unreachable!() };
    let recipe = encode_shape(edited.shape.as_ref().unwrap(), fingerprint(edited));
    let moved = PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0., 0.]), Anchor::corner([5., 5.])], false)]).unwrap();
    let stale = record_bytes(PATH, 1, &[(GEOMETRY, encode_geometry(&moved)), (STYLE, encode_style(&edited.style)), (LIVE_SHAPE, recipe)]);
    let reopened = decode_chunk(&stale, 1, &mut Reading::default()).unwrap();
    let path = reopened.records()[0].path().unwrap();
    assert_eq!(path.shape, None, "a recipe whose fingerprint no longer matches detaches");
    assert_eq!(*path.geometry, moved, "and leaves the geometry unchanged");
}

#[test]
fn polygons_and_stars_keep_their_recipe_through_turns_flips_and_stretches() {
    let corners = |geometry: &PathGeometry| { let mut p: Vec<[f64; 2]> = geometry.subpaths()[0].anchors().map(|a| a.point()).collect(); p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1]))); p };
    let regridded = |a: Vec<[f64; 2]>, b: Vec<[f64; 2]>| a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| (0..2).all(|i| (a[i] - b[i]).abs() <= 3. / GRID));
    let polygon = LiveShape { kind: ShapeKind::Polygon { corners: 5 }, center: [50., 50.], size: [40., 40.], rotation: 0. };
    let star = LiveShape { kind: ShapeKind::Star { points: 5, inner: 0.5 }, ..polygon };
    for (shape, count) in [(polygon, 5), (star, 10)] {
        let geometry = shape.geometry().unwrap();
        assert_eq!(geometry.anchor_count(), count);
        assert!(geometry.subpaths()[0].closed);
        let top = geometry.subpaths()[0].anchor(0).point();
        assert!((top[0] - 50.).abs() < 1e-9 && (top[1] - 30.).abs() < 1e-9, "the first corner is at the top: {top:?}");
        for affine in [Affine64([1., 0., 0., -1., 0., 100.]), Affine64([-1., 0., 0., 1., 100., 0.]), Affine64([0., 1., -1., 0., 100., 0.]), Affine64([2., 0., 0., 1., 0., 0.])] {
            let moved = shape.transformed(affine).expect("a frame-aligned map keeps the recipe");
            assert!(regridded(corners(&moved.geometry().unwrap()), corners(&geometry.transformed(|p| affine.map(p)).unwrap())), "{affine:?}");
        }
    }
    let inner = |shape: &LiveShape| { let p = shape.geometry().unwrap().subpaths()[0].anchor(1).point(); (p[0] - 50.).hypot(p[1] - 50.) };
    assert!((inner(&star) - 10.).abs() <= 1. / GRID, "inner points sit at half the radius");
    for kind in [ShapeKind::Polygon { corners: 2 }, ShapeKind::Star { points: 101, inner: 0.5 }, ShapeKind::Star { points: 5, inner: 0. }] {
        assert!(!kind.valid() && LiveShape { kind, ..polygon }.geometry().is_err(), "{kind:?}");
    }
    let records: Vec<Record> = [polygon, star].into_iter().enumerate().map(|(i, shape)| {
        let Record::Path(path) = line(i as u64 + 1, &[[0., 0.], [1., 1.]], 2.) else { unreachable!() };
        Record::Path(path.with_shape(shape).unwrap())
    }).collect();
    let (bytes, decoded) = round_trip(records.clone());
    assert_eq!(decoded.records(), &records[..], "polygon and star recipes reopen");
    assert_eq!(encode_chunk(&decoded), bytes);
}

#[test]
fn transforms_keep_live_shapes_they_can_express_and_drop_the_rest() {
    let rounded = live(1, ShapeKind::Rectangle { radii: [4., 0., 8., 2.] });
    for affine in [Affine64([1., 0., 0., 1., 30., -12.]), Affine64([0.8, 0.6, -0.6, 0.8, 5., 9.]), Affine64([-1.5, 0., 0., 1.5, 400., 0.])] {
        let moved = rounded.transformed(affine, true).unwrap();
        let shape = moved.shape.as_ref().expect("similarities keep the recipe");
        assert!(near(&normalized(&moved.geometry), &normalized(&shape.geometry().unwrap())), "{affine:?}");
    }
    let stretched = rounded.transformed(Affine64([2., 0., 0., 1., 0., 0.]), true).unwrap();
    assert_eq!(stretched.shape, None, "uneven scale cannot keep rounded corners");
    let plain = rotated(2, ShapeKind::Rectangle { radii: [0.; 4] }, 0.).transformed(Affine64([2., 0., 0., 1., 0., 0.]), true).unwrap();
    assert!(plain.shape.is_some(), "a square-cornered rectangle stretches along its own axes");
    assert!(live(3, ShapeKind::Ellipse).transformed(Affine64([2., 0., 0., 1., 0., 0.]), true).unwrap().shape.is_none(), "stretching across a rotated frame shears it");
    assert!(rounded.with_style(Arc::new(Style::default())).shape.is_some(), "restyling keeps the recipe");
    assert!(rounded.with_geometry(rounded.geometry.clone()).shape.is_none(), "editing geometry detaches it");
}
fn normalized(geometry: &PathGeometry) -> PathGeometry {
    let mut points: Vec<[f64; 2]> = geometry.subpaths()[0].anchors().map(|a| a.point()).collect();
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    PathGeometry::new(vec![Subpath::new(&points.into_iter().map(Anchor::corner).collect::<Vec<_>>(), true)]).unwrap()
}

#[test]
fn vector_fixture_reopens_every_feature_and_saves_byte_for_byte() {
    let bytes = include_bytes!("codec/fixtures/vector-objects.capy").to_vec();
    let editor = crate::Editor::new(crate::Document::from_artwork(open(bytes.clone())).unwrap());
    let scene = editor.document().scene();
    let layer = |name: &str| scene.order().iter().find(|h| scene.occurrence(**h).unwrap().name.as_ref() == name).and_then(|h| scene.vector_layer(*h)).unwrap().clone();
    let shapes = layer("Shapes");
    let paths: Vec<PathItem> = shapes.paths().map(|(_, p)| p.clone()).collect();
    assert_eq!(paths.len(), 11);
    assert!(matches!(paths[0].shape.as_deref(), Some(LiveShape { kind: ShapeKind::Rectangle { .. }, .. })) && paths[0].name.as_deref() == Some("Badge"));
    assert!(matches!(paths[0].style.entries[..], [PaintEntry::Stroke(_), PaintEntry::Fill(_)]), "a stroke below a fill");
    assert!(matches!(paths[1].shape.as_deref(), Some(LiveShape { kind: ShapeKind::Ellipse, .. })));
    assert_eq!(paths[2].fill_rule, FillRule::EvenOdd);
    assert_eq!(paths[2].geometry.subpaths().len(), 2);
    let strokes: Vec<&Stroke> = paths.iter().flat_map(|p| p.style.strokes()).collect();
    for cap in [Cap::Butt, Cap::Round, Cap::Square] { assert!(strokes.iter().any(|s| s.cap == cap)); }
    for join in [Join::Miter, Join::Round, Join::Bevel] { assert!(strokes.iter().any(|s| s.join == join)); }
    assert!(strokes.iter().filter_map(|s| s.profile.as_ref()).any(|p| p.knots().iter().any(|k| k.left() != k.right())), "a one-sided width profile");
    assert!(paths.iter().any(|p| p.geometry.anchor_count() == 1), "a dot");
    assert!(paths.iter().any(|p| p.hidden) && paths.iter().any(|p| p.locked));
    let hatching = layer("Hatching");
    assert_eq!((hatching.path_count(), hatching.chunks().len()), (65, 2), "a full chunk and one more");
    assert_eq!(layer("Empty").path_count(), 0);
    let codes = layer("Codes");
    let colors: BTreeSet<(u8, bool)> = codes.paths().flat_map(|(_, p)| p.style.entries.iter()).filter_map(|e| match e {
        PaintEntry::Fill(Fill { paint: Paint::Color { color, .. }, .. }) => Some((color.space as u8, color.linear_rgb.is_some())), _ => None }).collect();
    assert_eq!(colors.len(), 8, "every color space in both forms");
    let anchors: Vec<Anchor> = codes.paths().flat_map(|(_, p)| p.geometry.subpaths()[0].to_vec()).collect();
    assert!(anchors.iter().any(|a| a.kind == AnchorKind::SYMMETRIC && a.in_handle() == a.out_handle().map(|v| -v) && a.out_handle() != [0.; 2]), "a symmetric anchor with an implied in-handle");
    assert!(anchors.iter().any(|a| a.kind == AnchorKind::SYMMETRIC && a.out_handle() == [0.; 2] && a.in_handle() != [0.; 2]), "a symmetric anchor with an explicit in-handle");
    assert!(codes.paths().any(|(_, p)| p.unknown.as_ref().is_some_and(|u| u.iter().any(|p| p.tag >> 2 & 3 == ValueClass::Items as u64))), "an Items-class unknown property");
    assert_eq!((codes.next_item(), codes.paths().map(|(_, p)| p.id.0).max()), (15, Some(10)), "the next item stays past deleted identities");
    let manifest = |bytes: &[u8]| -> serde_json::Value {
        let directory = super::archive::Directory::read(&mut std::io::Cursor::new(bytes), 16, 1 << 20).unwrap();
        serde_json::from_slice(&directory.read_member(&mut std::io::Cursor::new(bytes), directory.member("manifest.json").unwrap(), 1 << 20).unwrap()).unwrap()
    };
    let resources = manifest(&bytes)["resources"].as_array().unwrap().clone();
    assert!(resources.iter().any(|r| r["type"] == "capy.vector-items/1" && r["encoding"] == "capy.vector-records/1"), "a raw chunk");
    assert!(resources.iter().any(|r| r["type"] == "capy.vector-items/1" && r["encoding"] == "capy.lz4-bytes/1"), "a compressed chunk");
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let capture = editor.capture(0, Default::default()).unwrap();
    let mut saved = Vec::new();
    super::codec::PreparedPackage::prepare(&capture, None, &cancel).unwrap().write(&mut saved, &cancel).unwrap();
    assert!(saved == bytes, "a save without edits reproduces the fixture byte for byte");
}

#[test]
fn seeded_edit_scripts_keep_unknown_data_by_its_rules_and_never_lower_the_next_item() {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; state };
    let unknown = |number: u64, flags: u64| UnknownProperty { tag: tag_word(number, flags, ValueClass::Data), value: Arc::from(&number.to_le_bytes()[..]) };
    let decorated = |id: u64| {
        let Record::Path(mut path) = line(id, &[[id as f64, 0.], [id as f64 + 9., 20.]], 2.) else { unreachable!() };
        path.unknown = Some(Arc::from(vec![unknown(900, IGNORABLE | COPY_SAFE), unknown(901, IGNORABLE)]));
        if let PaintEntry::Stroke(stroke) = &mut Arc::make_mut(&mut path.style).entries[0] { stroke.unknown = Some(Arc::from(vec![unknown(902, IGNORABLE | COPY_SAFE)])); }
        Record::Path(path)
    };
    let mut object = VectorObject::from_records((1..=150).map(decorated).collect());
    let mut history = vec![object.clone()];
    let pick = |object: &VectorObject, at: u64| object.paths().nth(at as usize % object.path_count().max(1)).map(|(_, p)| p.id);
    for _ in 0..400 {
        let before = object.clone();
        let step = next() % 7;
        object = match step {
            0 => {
                let ids: BTreeSet<ItemId> = (0..1 + next() % 5).filter_map(|_| pick(&object, next())).collect();
                let moved = object.transform_items(&ids, Affine64([1., 0., 0., 1., (next() % 40) as f64, 3.]), true).unwrap();
                for id in &ids {
                    let path = moved.record(moved.find(*id).unwrap()).unwrap().path().unwrap();
                    assert!(path.unknown.iter().flat_map(|u| u.iter()).all(|p| p.tag & COPY_SAFE != 0), "moving drops data that is not copy-safe");
                    assert!(path.unknown.iter().flat_map(|u| u.iter()).any(|p| p.tag & COPY_SAFE != 0), "and keeps copy-safe data");
                }
                moved
            }
            1 | 2 => {
                let Some(id) = pick(&object, next()) else { continue };
                let at = object.find(id).unwrap();
                let path = object.record(at).unwrap().path().unwrap().clone();
                let edited = if step == 1 { PathItem { name: Some(format!("n{}", next() % 100).into()), ..path.clone() } } else { path.with_style(Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: color(0.7), unknown: None })]))) };
                let copy_safe: Vec<_> = path.unknown.iter().flat_map(|u| u.iter()).filter(|p| p.tag & COPY_SAFE != 0).cloned().collect();
                let kept: Vec<_> = edited.unknown.iter().flat_map(|u| u.iter()).cloned().collect();
                if step == 1 { assert_eq!(edited.unknown, path.unknown, "renaming keeps every property"); } else { assert_eq!(kept, copy_safe, "restyling keeps copy-safe properties"); }
                object.replace([(at, Some(Record::Path(edited)))])
            }
            3 => match pick(&object, next()) { Some(id) => object.replace([(object.find(id).unwrap(), None)]), None => continue },
            4 => {
                let id = object.allocate_id();
                assert!(object.paths().all(|(_, p)| p.id < id), "a new path follows every identity in its object");
                object.insert_front(vec![decorated(id.0)])
            }
            5 if history.len() > 1 => { history.pop(); history.last().unwrap().clone() }
            _ => continue,
        };
        if step != 5 { history.push(before.clone()); history.push(object.clone()); history.swap_remove(history.len() - 2); }
        if step != 5 { assert!(object.next_item() >= before.next_item(), "edits never lower the next item"); }
        object.validate().unwrap();
        let encoded: Vec<(Vec<u8>, usize)> = object.chunks().iter().map(|c| (encode_chunk(c), c.records().len())).collect();
        let decoded = decode_object(&encoded.iter().map(|(b, n)| (&b[..], *n)).collect::<Vec<_>>(), &mut Reading::default()).unwrap();
        assert!(decoded.iter().zip(object.chunks()).all(|(a, b)| a.records() == b.records()) && decoded.len() == object.chunks().len(), "every state round-trips exactly");
    }
}

#[test]
fn arranging_paths_moves_them_in_stacking_order_and_rewrites_only_their_chunks() {
    let object = VectorObject::from_records((1..=200).map(|i| Record::Path(plain_line(i, [i as f64, 0.], [i as f64, 9.], 1.))).collect());
    let order = |object: &VectorObject| object.paths().map(|(_, p)| p.id.0).collect::<Vec<_>>();
    let ids = |list: &[u64]| list.iter().copied().map(ItemId).collect::<BTreeSet<_>>();
    let same = |a: &VectorObject, b: &VectorObject| a.chunks().iter().zip(b.chunks()).filter(|(x, y)| Arc::ptr_eq(x.storage(), y.storage())).count();

    let front = object.arrange(&ids(&[150, 5]), Arrange::Front);
    assert_eq!(order(&front)[..3], [5, 150, 1], "selected paths keep their relative order at the front");
    assert_eq!(order(&front).len(), 200);
    assert!(front.arrange(&ids(&[150, 5]), Arrange::Front).same_root(&front), "already in front is unchanged");
    let back = object.arrange(&ids(&[150, 5]), Arrange::Back);
    assert_eq!(order(&back)[197..], [200, 5, 150]);
    assert!(back.arrange(&ids(&[150, 5]), Arrange::Back).same_root(&back));

    let forward = object.arrange(&ids(&[10, 11, 70]), Arrange::Forward);
    assert_eq!(order(&forward)[7..12], [8, 10, 11, 9, 12], "a run moves past one path");
    assert_eq!(order(&forward)[67..71], [68, 70, 69, 71]);
    assert!(same(&object, &forward) >= object.chunks().len() - 3, "only the touched chunks change");
    let backward = object.arrange(&ids(&[64, 65, 200]), Arrange::Backward);
    assert_eq!(order(&backward)[62..67], [63, 66, 64, 65, 67], "a run crossing a chunk boundary moves together");
    assert_eq!(order(&backward)[199], 200, "the last path cannot move further back");
    assert!(object.arrange(&ids(&[1]), Arrange::Forward).same_root(&object), "the front path cannot move further forward");

    let opaque = Record::Opaque(OpaqueRecord { kind: 999 << 4 | IGNORABLE, id: 900, properties: Arc::from([]) });
    let walled = VectorObject::from_records(vec![Record::Path(plain_line(1, [0., 0.], [1., 1.], 1.)), opaque, Record::Path(plain_line(2, [0., 0.], [1., 1.], 1.))]);
    assert!(walled.arrange(&ids(&[2]), Arrange::Forward).same_root(&walled), "unknown records are not stepped over");
    assert_eq!(order(&walled.arrange(&ids(&[2]), Arrange::Front)), [2, 1]);
    walled.arrange(&ids(&[2]), Arrange::Front).validate().unwrap();
}

fn ink(id: u64, anchors: Vec<Anchor>, closed: bool, profile: Option<WidthProfile>) -> PathItem {
    let mut stroke = Stroke::new(color(0.2), 8.);
    (stroke.cap, stroke.join, stroke.profile) = (Cap::Round, Join::Round, profile.map(Arc::new));
    PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, closed)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
}
fn samples(path: &PathItem) -> Vec<([f64; 2], [f64; 2])> {
    let profile = path.style.strokes().next().unwrap().profile.clone();
    let mut out = Vec::new();
    let mut base = 0u32;
    for subpath in path.geometry.subpaths() {
        for (i, segment) in subpath.segments().enumerate() {
            for k in 0..=200 { let t = f64::from(k) / 200.; out.push((segment.point(t), profile.as_ref().map_or([1.; 2], |p| p.at(base + i as u32, t)))); }
        }
        base += subpath.segment_count() as u32;
    }
    out
}

#[test]
fn erasing_cuts_visible_ink_into_pieces_that_keep_their_shape_and_widths() {
    let curve = vec![
        Anchor::new([0., 0.], [0.; 2], [30., -40.], AnchorKind::CORNER),
        Anchor::new([100., 0.], [-30., -30.], [30., 30.], AnchorKind::SYMMETRIC),
        Anchor::new([200., 40.], [-20., 0.], [0.; 2], AnchorKind::CORNER),
    ];
    let profile = WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.2), WidthKnot::new(0, 0.7, 1., 0.8), WidthKnot::new(1, 0.4, 0.5, 1.), WidthKnot::new(1, 1., 0.2, 0.1)]).unwrap();
    let path = ink(1, curve, false, Some(profile));
    let original = samples(&path);
    let eraser = Eraser { from: [100., -40.], to: [100., 40.], radius: 6. };
    let pieces = path.erased(&eraser, None).expect("the eraser crosses the path");
    assert_eq!(pieces.len(), 2, "one crossing leaves two pieces");
    for piece in &pieces {
        piece.validate().unwrap();
        for (point, width) in samples(piece) {
            let (nearest, expected) = original.iter().min_by(|a, b| { let d = |q: &[f64; 2]| (q[0] - point[0]).hypot(q[1] - point[1]); d(&a.0).total_cmp(&d(&b.0)) }).unwrap();
            assert!((nearest[0] - point[0]).hypot(nearest[1] - point[1]) < 0.5, "piece points lie on the original curve");
            assert!((width[0] - expected[0]).abs() < 0.02 && (width[1] - expected[1]).abs() < 0.02, "widths follow the original profile: {width:?} vs {expected:?}");
            assert!(eraser.from[0] - point[0] >= 6. - 1e-3 || point[0] - eraser.from[0] >= 6. - 1e-3, "nothing inside the eraser remains");
        }
    }
    assert!(path.erased(&Eraser { from: [400., 0.], to: [400., 10.], radius: 6. }, None).is_none(), "a miss leaves the path alone");
    assert_eq!(path.erased(&Eraser { from: [-10., 0.], to: [210., 20.], radius: 80. }, None), Some(Vec::new()), "covering everything removes the path");

    let square = ink(2, [[0., 0.], [50., 0.], [50., 50.], [0., 50.]].map(Anchor::corner).to_vec(), true, None);
    let opened = square.erased(&Eraser { from: [25., -5.], to: [25., 5.], radius: 3. }, None).unwrap();
    assert_eq!(opened.len(), 1, "cutting a closed path once opens it");
    let subpath = &opened[0].geometry.subpaths()[0];
    assert!(!subpath.closed);
    assert_eq!(subpath.len(), 6, "the open piece runs from the cut through every corner back to the cut");
    assert!((subpath.anchor(0).point()[0] - 32.).abs() < 1e-3 && (subpath.anchor(5).point()[0] - 18.).abs() < 1e-3);
    assert!(!opened[0].hit([25., 0.], 0.), "round caps leave the erased area clear");

    let compound = PathItem::new(ItemId(3), Arc::new(PathGeometry::new(vec![
        Subpath::new(&[Anchor::corner([0., 0.]), Anchor::corner([100., 0.])], false),
        Subpath::new(&[Anchor::corner([0., 50.]), Anchor::corner([100., 50.])], false),
    ]).unwrap()), square.style.clone());
    let cut = compound.erased(&Eraser { from: [50., -5.], to: [50., 5.], radius: 3. }, None).unwrap();
    assert_eq!(cut.len(), 1, "a compound path stays one path");
    assert_eq!(cut[0].geometry.subpaths().len(), 3);
}

#[test]
fn erasing_a_layer_splits_or_removes_only_touched_paths_with_fresh_ids() {
    let object = VectorObject::from_records((1..=100).map(|i| Record::Path(plain_line(i, [0., i as f64 * 10.], [200., i as f64 * 10.], 2.))).collect());
    let mut index = VectorIndex::default();
    let eraser = Eraser { from: [100., 5.], to: [100., 52.], radius: 4. };
    let split = object.erase(&mut index, &eraser, EraseMode::Touched).unwrap();
    split.validate().unwrap();
    assert_eq!(split.path_count(), 105, "five crossed lines split in two");
    let order: Vec<u64> = split.paths().map(|(_, p)| p.id.0).take(11).collect();
    assert_eq!(order[..2], [1, order[1]], "a split keeps the first piece's identity in place");
    assert!(order[1] > 100, "the second piece gets a new identity");
    let whole = object.erase(&mut index, &eraser, EraseMode::Whole).unwrap();
    assert_eq!(whole.path_count(), 95, "Whole Path removes every touched path");
    assert!(object.erase(&mut index, &Eraser { from: [505., 505.], to: [505., 505.], radius: 5. }, EraseMode::Touched).is_none());
}

#[test]
fn newer_vector_layers_open_from_their_fallback_locked_and_save_back_unchanged() {
    let image: crate::Image = crate::color::source::rgba8_source([32, 24], |x, y| [x as u8 * 8, y as u8 * 10, 90, 255]).into();
    let unknown = record_bytes(900 << 4, 7, &[(1 << 5, vec![1, 2, 3])]);
    let readable = encode_chunk(&chunk(vec![line(3, &[[0., 0.], [10., 10.]], 2.)]));
    let layer = |bytes: &[u8], records: u64| {
        let mut object = crate::ImageObject::new(image.clone());
        object.affine = crate::authored::Affine64([1., 0., 0., 1., 40., 30.]);
        object.vector = Some(crate::authored::PreservedVector(Arc::from([(serde_json::json!({"records": records}), crate::authored::Resource::new(Arc::<[u8]>::from(bytes)))]), Arc::default(), Some(records + 7)));
        let mut editor = crate::Editor::new(crate::Document::from_artwork(crate::Artwork::new([200, 150]).unwrap()).unwrap());
        let (_, edit) = editor.document().create_object_layer_edit("Newer", object, None, 0).unwrap();
        editor.perform(edit).unwrap();
        editor
    };
    let saved = save(&layer(&unknown, 1));
    let reopened = open(saved.clone());
    assert!(reopened.vectors.is_empty(), "this reader cannot read the paths");
    let (_, _, object) = reopened.objects.iter().next().expect("the layer shows its fallback image");
    assert_eq!((object.image.extent, object.affine.0[4]), ([32, 24], 40.));
    let preserved = object.vector.as_ref().expect("the unreadable paths are kept");
    assert_eq!(preserved.0.len(), 1);
    assert_eq!(&**preserved.0[0].1.storage(), &unknown[..], "item records keep their bytes");
    let document = crate::Document::from_artwork(reopened.clone()).unwrap();
    let occurrence = *document.scene().order().iter().find(|h| document.scene().object_layer(**h).is_some()).unwrap();
    assert!(document.is_locked(occurrence), "a layer from a newer version cannot be edited");
    let again = open(save(&crate::Editor::new(document)));
    assert_eq!(&**again.objects.iter().next().unwrap().2.vector.as_ref().unwrap().0[0].1.storage(), &unknown[..], "saving keeps the paths for the newer version");

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let editor = crate::Editor::new(crate::Document::from_artwork(open(saved)).unwrap());
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let session = super::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
    let mut pack = Vec::new();
    std::io::Read::read_to_end(&mut session.resources().reader(&cancel), &mut pack).unwrap();
    let loaded = super::session::open_parts(session.metadata(), super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(), crate::ProjectLimits::default(), &cancel).unwrap();
    let restored = loaded.editor.document().artwork.objects.iter().next().unwrap().2;
    assert_eq!(&**restored.vector.as_ref().unwrap().0[0].1.storage(), &unknown[..], "sessions keep the paths too");

    let current = open(save(&layer(&readable, 1)));
    assert!(current.objects.is_empty() && vector(&current).path_count() == 1, "a reader that knows every record opens the paths and drops the fallback");
}

#[test]
fn erasing_up_to_intersections_removes_the_touched_span_between_crossings() {
    let horizontal = plain_line(1, [0., 50.], [300., 50.], 2.);
    let left = plain_line(2, [100., 0.], [100., 100.], 2.);
    let right = plain_line(3, [200., 0.], [200., 100.], 2.);
    let object = VectorObject::from_records(vec![Record::Path(horizontal.clone()), Record::Path(left), Record::Path(right)]);
    let mut index = VectorIndex::default();
    let eraser = Eraser { from: [150., 45.], to: [150., 55.], radius: 3. };
    let erased = object.erase(&mut index, &eraser, EraseMode::ToIntersection).unwrap();
    erased.validate().unwrap();
    let pieces: Vec<[f64; 4]> = erased.paths().filter(|(_, p)| p.id == ItemId(1) || p.id.0 > 3).map(|(_, p)| p.geometry.hull()).collect();
    assert_eq!(pieces.len(), 2, "the line keeps both outer parts: {pieces:?}");
    assert!(pieces.iter().any(|h| (h[0] - 0.).abs() < 1e-3 && (h[2] - 100.).abs() < 1e-3), "{pieces:?}");
    assert!(pieces.iter().any(|h| (h[0] - 200.).abs() < 1e-3 && (h[2] - 300.).abs() < 1e-3), "{pieces:?}");
    assert_eq!(erased.paths().filter(|(_, p)| p.id == ItemId(2) || p.id == ItemId(3)).count(), 2, "the crossing lines stay");
    let cut = object.erase(&mut index, &eraser, EraseMode::Touched).unwrap();
    assert!(cut.paths().filter(|(_, p)| p.id == ItemId(1) || p.id.0 > 3).any(|(_, p)| p.geometry.hull()[2] > 140. && p.geometry.hull()[2] < 150.), "Touched Part cuts only under the eraser");
    let alone = VectorObject::from_records(vec![Record::Path(horizontal)]);
    let gone = alone.erase(&mut VectorIndex::default(), &eraser, EraseMode::ToIntersection).unwrap();
    assert_eq!(gone.path_count(), 0, "a line that crosses nothing goes entirely");
    let square = PathItem::new(ItemId(9), Arc::new(PathGeometry::new(vec![Subpath::new(&[[0., 0.], [40., 0.], [40., 40.], [0., 40.]].map(Anchor::corner), true)]).unwrap()), Arc::new(Style::default()));
    assert!(square.encloses([20., 20.], 0.1) && !square.encloses([60., 20.], 0.1), "a closed path encloses its inside");
    assert!(!plain_line(10, [0., 0.], [40., 40.], 2.).encloses([20., 10.], 0.1), "an open unfilled line encloses nothing");
}

#[test]
fn merging_a_plain_vector_layer_down_onto_another_keeps_paths() {
    let artwork = crate::Artwork::new([640, 480]).unwrap();
    let mut editor = crate::Editor::new(crate::Document::from_artwork(artwork).unwrap());
    let (lower, edit) = editor.document().create_vector_layer_edit("Lines", VectorObject::from_records(vec![Record::Path(plain_line(1, [10., 10.], [100., 10.], 2.))]), None, 0).unwrap();
    editor.perform(edit).unwrap();
    let (upper, edit) = editor.document().create_vector_layer_edit("More", VectorObject::from_records(vec![Record::Path(plain_line(1, [10., 50.], [100., 50.], 2.)), Record::Path(plain_line(2, [10., 90.], [100., 90.], 2.))]), None, 0).unwrap();
    editor.perform(edit).unwrap();
    let mut moved = editor.document().scene().occurrence(upper).unwrap().clone();
    moved.offset = [5, 7];
    editor.perform(crate::Edit::Occurrence(crate::RecordChange::replace(&editor.document().artwork.occurrences, upper, Some(moved)).unwrap())).unwrap();
    let mut working = editor.document().working.clone();
    working.occurrence = Some(upper);
    editor.perform(crate::Edit::Working(working)).unwrap();
    assert!(editor.document().merge_rasterizes(crate::MergeKind::Down).is_empty());
    assert_eq!(editor.document().merge_rasterizes(crate::MergeKind::Visible).len(), 2, "Merge Visible turns both into pixels");
    let edits = editor.document().vector_merge_down().expect("two plain vector layers merge as paths");
    editor.perform(crate::Edit::Batch(edits)).unwrap();
    let document = editor.document();
    assert!(document.scene().occurrence(upper).is_none() && document.working.occurrence == Some(lower));
    let merged = document.scene().vector_layer(lower).unwrap();
    merged.validate().unwrap();
    assert_eq!(merged.path_count(), 3);
    let fronts: Vec<[f64; 2]> = merged.paths().map(|(_, p)| p.geometry.subpaths()[0].anchor(0).point()).collect();
    assert_eq!(fronts, [[15., 57.], [15., 97.], [10., 10.]], "the upper paths sit in front, at their place on the canvas");
    editor.undo().unwrap();
    assert!(editor.document().scene().vector_layer(upper).is_some(), "one undo step restores both layers");
    let mut faded = editor.document().scene().occurrence(upper).unwrap().clone();
    faded.opacity = 0.5;
    editor.perform(crate::Edit::Occurrence(crate::RecordChange::replace(&editor.document().artwork.occurrences, upper, Some(faded)).unwrap())).unwrap();
    assert!(editor.document().vector_merge_down().is_none(), "a faded layer bakes into pixels");
    assert_eq!(editor.document().merge_rasterizes(crate::MergeKind::Down), [upper, lower]);
}

#[test]
fn colors_caps_and_joins_use_fixed_wire_codes() {
    let mut entries = Vec::new();
    for (code, space) in [(0u8, crate::color::RgbSpace::Srgb), (1, crate::color::RgbSpace::DisplayP3), (2, crate::color::RgbSpace::AdobeRgb), (3, crate::color::RgbSpace::ProPhoto)] {
        for (form, color) in [(0u8, RgbColor::new(space, [0.5, 0.25, 0.125, 1.]).unwrap()), (1, RgbColor::from_linear(space, [1.5, 0.25, 0.125, 1.]).unwrap())] {
            entries.push((code, form, PaintEntry::Fill(Fill { paint: Paint::color(color), unknown: None })));
        }
    }
    for (code, (cap, join)) in [(1u8, (Cap::Round, Join::Round)), (2, (Cap::Square, Join::Bevel))] {
        let mut stroke = Stroke::new(color(0.5), 2.);
        (stroke.cap, stroke.join) = (cap, join);
        let bytes = encode_chunk(&chunk(vec![Record::Path(PathItem::new(ItemId(9), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0.; 2])], false)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]))))]));
        for tag in [CAP, JOIN] {
            let mut property = Vec::new();
            put_varuint(&mut property, tag);
            property.extend([1, code]);
            assert!(bytes.windows(property.len()).any(|w| w == property));
        }
    }
    for (code, form, entry) in entries {
        let bytes = encode_chunk(&chunk(vec![Record::Path(PathItem::new(ItemId(9), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([0.; 2])], false)]).unwrap()), Arc::new(Style::new(vec![entry.clone()]))))]));
        let PaintEntry::Fill(Fill { paint: Paint::Color { color, .. }, .. }) = &entry else { unreachable!() };
        let form = if color.linear_rgb.is_some() { form } else { 0 };
        let mut property = Vec::new();
        put_varuint(&mut property, COLOR_VALUE);
        property.extend([18, code, form]);
        assert!(bytes.windows(property.len()).any(|w| w == property), "space {code} form {form}");
        let decoded = decode_chunk(&bytes, 1, &mut Reading::default()).unwrap();
        assert_eq!(decoded.records()[0].path().unwrap().style.entries[0], entry);
    }
}

#[test]
fn slot_values_name_existing_slots_and_stay_behind_when_paths_leave_their_object() {
    let slots = |value: Vec<u8>| record_bytes(PATH, 4, &[(GEOMETRY, geometry_bytes()), (tag_word(1000, IGNORABLE | COPY_SAFE, ValueClass::Slots), value)]);
    assert!(decode_chunk(&slots(vec![0, 1]), 1, &mut Reading::slots(2)).is_ok());
    assert!(is_invalid(decode_chunk(&slots(vec![0, 2]), 1, &mut Reading::slots(2))));
    assert!(is_invalid(decode_chunk(&slots(vec![0]), 1, &mut Reading::default())));
    assert!(is_invalid(decode_chunk(&slots(vec![0x80]), 1, &mut Reading::slots(2))));
    assert!(decode_chunk(&slots(Vec::new()), 1, &mut Reading::default()).is_ok());
    let nested = record_bytes(PATH, 4, &[(GEOMETRY, geometry_bytes()), (tag_word(1001, IGNORABLE, ValueClass::Nested), record_bytes(kind_word(60) | IGNORABLE, 0, &[(tag_word(3, IGNORABLE, ValueClass::Slots), vec![1])]))]);
    assert!(decode_chunk(&nested, 1, &mut Reading::slots(2)).is_ok() && is_invalid(decode_chunk(&nested, 1, &mut Reading::slots(1))));

    let artwork = open(include_bytes!("codec/fixtures/vector-future.capy").to_vec());
    let object = vector(&artwork).clone();
    let Record::Path(path) = &object.records().next().unwrap().1 else { panic!() };
    assert!(path.unknown.as_ref().is_some_and(|u| u[0].tag >> 2 & 3 == ValueClass::Slots as u64));
    let mut kept = BTreeSet::new();
    kept.insert(path.id);
    let (_, same) = object.copies(&kept, &object);
    let (_, other) = object.copies(&kept, &VectorObject::default());
    assert_eq!(same[0].path().unwrap().unknown, path.unknown);
    assert_eq!(other[0].path().unwrap().unknown, None, "slot references never point into another object's slots");
    let moved = object.transform_items(&kept, Affine64([1., 0., 0., 1., 5., 5.]), true).unwrap();
    assert!(Arc::ptr_eq(moved.slots(), object.slots()), "edits keep the object's slots");
}

#[test]
fn future_slots_open_editable_or_from_their_fallback_and_save_back_byte_for_byte() {
    let fixed = include_bytes!("codec/fixtures/vector-future.capy").to_vec();
    let artwork = open(fixed.clone());
    let object = vector(&artwork);
    assert_eq!(object.path_count(), 1);
    let [Slot::Image(texture), Slot::Resource(font)] = &object.slots()[..] else { panic!("an image and a resource slot") };
    assert_eq!(texture.extent, [16, 12]);
    assert_eq!((font.kind.as_ref(), font.length), ("example.font/1", 24));
    assert_eq!(&*font.read_chunk(0, 24, &std::sync::atomic::AtomicBool::new(false)).unwrap(), b"capy future font program");
    let (_, _, newer) = artwork.objects.iter().next().expect("the newer layer opens from its fallback");
    let preserved = newer.vector.as_ref().unwrap();
    assert!(matches!(&preserved.1[..], [Slot::Resource(a), Slot::Image(b)] if a.id == font.id && b.id() == texture.id()));
    assert!(super::artwork_records::slot_resources(&artwork).contains(&font.id));
    let document = crate::Document::from_artwork(artwork.clone()).unwrap();
    let newer_layer = *document.scene().order().iter().find(|h| document.scene().object_layer(**h).is_some()).unwrap();
    assert!(document.is_locked(newer_layer));
    let editor = crate::Editor::new(document);
    assert!(save(&editor) == fixed, "a save without edits reproduces the fixture byte for byte");

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let capture = editor.capture_session(editor.capture(1, Default::default()).unwrap()).unwrap();
    let session = super::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
    let mut pack = Vec::new();
    std::io::Read::read_to_end(&mut session.resources().reader(&cancel), &mut pack).unwrap();
    let loaded = super::session::open_parts(session.metadata(), super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(), crate::ProjectLimits::default(), &cancel).unwrap();
    let restored = &loaded.editor.document().artwork;
    assert!(matches!(&vector(restored).slots()[..], [Slot::Image(_), Slot::Resource(r)] if r.kind.as_ref() == "example.font/1"));
    assert!(restored.extensions.resources.is_empty());

    let transfer = super::transfer::PreparedTransfer::capture(&editor.capture(1, Default::default()).unwrap(), &cancel).unwrap();
    let descriptor = serde_json::from_slice(&serde_json::to_vec(transfer.descriptor()).unwrap()).unwrap();
    let mut receiver = super::transfer::TransferReceiver::new(descriptor, crate::ProjectLimits::default()).unwrap();
    for index in receiver.missing_payloads() {
        let length = transfer.payload_len(index).unwrap();
        let mut offset = 0;
        while offset < length {
            let count = (length - offset).min(super::MAX_RANGE_BYTES as u64) as usize;
            receiver.push_chunk(index, &transfer.read_chunk(index, offset, count).unwrap()).unwrap();
            offset += count as u64;
        }
    }
    let adopted = receiver.finish().unwrap().adopt_verified(crate::ProjectLimits::default(), &cancel).unwrap();
    assert!(matches!(&vector(&adopted.artwork).slots()[..], [Slot::Image(_), Slot::Resource(r)] if r.id == font.id));
    assert!(adopted.artwork.extensions.resources.is_empty(), "slot resources are not ancillary extensions");
    let mut edited = crate::Editor::new(crate::Document::from_artwork((*adopted.artwork).clone()).unwrap());
    let layer = *edited.document().scene().order().iter().find(|h| edited.document().scene().vector_layer(**h).is_some()).unwrap();
    let moved = vector(&edited.document().artwork).transformed(Affine64([1., 0., 0., 1., 3., 0.])).unwrap();
    edited.perform(edited.document().vector_object_edit(layer, moved).unwrap()).unwrap();
    let reopened = open(save(&edited));
    assert_eq!(vector(&reopened).slots().len(), 2);
    assert!(reopened.objects.iter().next().unwrap().2.vector.as_ref().is_some_and(|v| v.1.len() == 2));
}

#[test]
fn chunks_that_do_not_compress_are_stored_raw_and_reopen() {
    let mut state = 0x1234_5678_9abc_def1u64;
    let mut next = move || { state ^= state << 13; state ^= state >> 7; state ^= state << 17; (state >> 11) as f64 / (1u64 << 53) as f64 };
    let anchors: Vec<Anchor> = (0..2000).map(|_| Anchor::new([next() * 640., next() * 480.], [next() * 9. - 4.5, next() * 9. - 4.5], [next() * 9. - 4.5, next() * 9. - 4.5], AnchorKind::CORNER)).collect();
    let path = PathItem::new(ItemId(0x9f3a_5c21_77e4_0b6d), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)]).unwrap()),
        Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(Paint::color(RgbColor::BLACK), 3.))])));
    let (mut editor, layer) = document_with_paths(1);
    editor.perform(editor.document().vector_object_edit(layer, VectorObject::from_records(vec![Record::Path(path.clone())])).unwrap()).unwrap();
    let saved = save(&editor);
    let directory = super::archive::Directory::read(&mut std::io::Cursor::new(&saved), 16, 1 << 20).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&directory.read_member(&mut std::io::Cursor::new(&saved), directory.member("manifest.json").unwrap(), 1 << 20).unwrap()).unwrap();
    assert!(manifest["resources"].as_array().unwrap().iter().any(|r| r["type"] == "capy.vector-items/1" && r["encoding"] == "capy.vector-records/1"));
    let reopened = open(saved);
    assert_eq!(vector(&reopened).paths().next().unwrap().1, &path);
    let again = crate::Editor::new(crate::Document::from_artwork(reopened).unwrap());
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let capture = again.capture_session(again.capture(1, Default::default()).unwrap()).unwrap();
    let session = super::session::PreparedSession::prepare(&capture, serde_json::json!({}), &cancel).unwrap();
    let mut pack = Vec::new();
    std::io::Read::read_to_end(&mut session.resources().reader(&cancel), &mut pack).unwrap();
    let loaded = super::session::open_parts(session.metadata(), super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(pack))).unwrap(), crate::ProjectLimits::default(), &cancel).unwrap();
    assert_eq!(vector(&loaded.editor.document().artwork).paths().next().unwrap().1, &path);
}

#[test]
fn opening_charges_the_records_that_decoded_vector_bytes_become() {
    let (editor, _) = document_with_paths(3000);
    let saved = save(&editor);
    let directory = super::archive::Directory::read(&mut std::io::Cursor::new(&saved), 16, 1 << 24).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&directory.read_member(&mut std::io::Cursor::new(&saved), directory.member("manifest.json").unwrap(), 1 << 24).unwrap()).unwrap();
    let decoded: u64 = manifest["resources"].as_array().unwrap().iter().filter(|r| r["type"] == "capy.vector-items/1")
        .map(|r| r["data"]["decoded_bytes"].as_str().map_or_else(|| r["bytes"].as_str().unwrap(), |v| v).parse::<u64>().unwrap()).sum();
    let resident = crate::history_budget::Accounting::default().charge_vector(vector(&open(saved.clone()))) as u64;
    assert!(resident > decoded, "records hold more than their wire bytes: {resident} vs {decoded}");
    let outcome = |budget: u64| {
        let backing = super::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(saved.clone()))).unwrap();
        super::codec::open(backing, crate::ProjectLimits { raster_bytes: budget, asset_bytes: 0, ..Default::default() }, &std::sync::atomic::AtomicBool::new(false)).unwrap()
    };
    assert!(matches!(outcome(decoded + resident / 2), super::codec::OpenOutcome::Preserved { .. }));
    assert!(matches!(outcome(decoded + resident), super::codec::OpenOutcome::Candidate { .. }));
}
