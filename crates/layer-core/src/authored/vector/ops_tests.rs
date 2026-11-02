use super::*;

fn paint() -> Paint { Paint::color(crate::color::RgbColor::new(Default::default(), [0.1, 0.2, 0.3, 1.]).unwrap()) }
fn path(id: u64, points: &[[f64; 2]], closed: bool, width: f64, knots: Option<Vec<WidthKnot>>) -> PathItem {
    let anchors: Vec<Anchor> = points.iter().map(|p| Anchor::new(*p, [-2., 1.], [2., -1.], AnchorKind::SMOOTH)).collect();
    let mut stroke = Stroke::new(paint(), width);
    stroke.profile = knots.map(|k| Arc::new(WidthProfile::new(k).unwrap()));
    PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, closed)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)])))
}
fn tapered(id: u64, points: &[[f64; 2]]) -> PathItem {
    let last = points.len() as u32 - 2;
    path(id, points, false, 4., Some(vec![WidthKnot::new(0, 0., 0.2, 0.4), WidthKnot::new(last, 1., 1., 0.8)]))
}
fn profile(path: &PathItem) -> &WidthProfile { path.style.strokes().next().unwrap().profile.as_deref().unwrap() }
fn points(path: &PathItem) -> Vec<[f64; 2]> { path.geometry.subpaths().iter().flat_map(|s| s.anchors().map(Anchor::point)).collect() }
fn near(a: [f64; 2], b: [f64; 2]) -> bool { (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4 }

#[test]
fn filled_area_does_not_cancel_at_self_crossings() {
    let polygon = |id, points: &[[f64; 2]]| {
        let mut p = path(id, points, true, 2., None);
        p.geometry = Arc::new(PathGeometry::new(vec![Subpath::new(&points.iter().copied().map(Anchor::corner).collect::<Vec<_>>(), true)]).unwrap());
        p
    };
    let bow = polygon(1, &[[0., 0.], [100., 100.], [0., 100.], [100., 0.]]);
    let small = polygon(2, &[[40., 10.], [60., 10.], [60., 30.], [40., 30.]]);
    for fill_rule in [FillRule::NonZero, FillRule::EvenOdd] {
        let bow = PathItem { fill_rule, ..bow.clone() };
        assert_eq!(bow.area_around([50., 20.], 0.01), 5000.);
        assert!(small.area_around([50., 20.], 0.01) < bow.area_around([50., 20.], 0.01));
    }
    let concave = polygon(3, &[[0., 0.], [100., 0.], [100., 10.], [10., 10.], [10., 100.], [0., 100.]]);
    assert_eq!(concave.area_around([5., 50.], 0.01), 1900.);
}

#[test]
fn continuing_uses_the_chosen_end_even_when_the_new_tail_is_closer() {
    let original = path(1, &[[100., 100.], [300., 100.]], false, 4., None);
    let added = path(2, &[[305., 100.], [500., 300.], [100., 101.]], false, 4., None);
    let joined = original.joined_ends(&added, true, false, 6.).unwrap();
    assert_eq!(joined.open_ends(), Some([[100., 100.], [100., 101.]]));
    assert_eq!(joined.id, original.id);
}

#[test]
fn reversing_runs_anchors_handles_and_widths_backwards() {
    let line = tapered(1, &[[0., 0.], [10., 0.], [20., 5.], [30., 0.]]);
    let reversed = line.reversed().unwrap();
    assert_eq!(points(&reversed), points(&line).into_iter().rev().collect::<Vec<_>>());
    let (a, b) = (&line.geometry.subpaths()[0], &reversed.geometry.subpaths()[0]);
    assert_eq!((b.anchor(0).in_handle(), b.anchor(0).out_handle()), (a.anchor(3).out_handle(), a.anchor(3).in_handle()));
    for (segment, t) in [(0, 0.), (1, 0.3), (2, 1.)] {
        let [l, r] = profile(&line).at(segment, t);
        assert!(near(profile(&reversed).at(2 - segment, 1. - t), [r, l]), "sides swap along the reversed line");
    }
    assert!(near(b.segment(0).point(0.25), a.segment(2).point(0.75)));
    assert_eq!(reversed.reversed().unwrap().geometry, line.geometry);
    let square = path(2, &[[0., 0.], [10., 0.], [10., 10.], [0., 10.]], true, 2., None);
    let turned = square.reversed().unwrap();
    for i in 0..4 { assert!(near(turned.geometry.subpaths()[0].segment(i).point(0.4), square.geometry.subpaths()[0].segment([2, 1, 0, 3][i]).point(0.6)), "segment {i}"); }
}

#[test]
fn closing_adds_one_segment_that_eases_between_the_end_widths() {
    let line = tapered(1, &[[0., 0.], [10., 0.], [20., 5.]]);
    let closed = line.closed().unwrap();
    let subpath = &closed.geometry.subpaths()[0];
    assert!(subpath.closed && subpath.segment_count() == 3);
    assert!(near(profile(&closed).at(2, 0.), [1., 0.8]) && near(profile(&closed).at(2, 1.), [0.2, 0.4]));
    profile(&closed).validate_for(3).unwrap();
    assert!(!closed.can_close() && line.can_close());
}

#[test]
fn splitting_at_anchors_keeps_curves_and_widths_in_each_piece() {
    let line = tapered(1, &[[0., 0.], [10., 0.], [20., 5.], [30., 0.]]);
    let pieces = line.split_at(&[AnchorRef { subpath: 0, anchor: 1 }, AnchorRef { subpath: 0, anchor: 2 }].into()).unwrap();
    assert_eq!(pieces.iter().map(points).collect::<Vec<_>>(), [vec![[0., 0.], [10., 0.]], vec![[10., 0.], [20., 5.]], vec![[20., 5.], [30., 0.]]]);
    for (i, piece) in pieces.iter().enumerate() {
        assert!(near(piece.geometry.subpaths()[0].segment(0).point(0.5), line.geometry.subpaths()[0].segment(i).point(0.5)));
        assert!(near(profile(piece).at(0, 0.5), profile(&line).at(i as u32, 0.5)));
    }
    assert_eq!(line.split_at(&[AnchorRef { subpath: 0, anchor: 0 }].into()).unwrap(), vec![line.clone()], "an end is not a cut");
    let square = path(2, &[[0., 0.], [10., 0.], [10., 10.], [0., 10.]], true, 2., None);
    let [opened] = &square.split_at(&[AnchorRef { subpath: 0, anchor: 2 }].into()).unwrap()[..] else { panic!("one opened path") };
    assert_eq!(points(opened), [[10., 10.], [0., 10.], [0., 0.], [10., 0.], [10., 10.]]);
    assert!(!opened.geometry.subpaths()[0].closed);
}

#[test]
fn joining_meets_the_nearest_ends_and_bridges_a_gap() {
    let a = tapered(1, &[[0., 0.], [10., 0.]]);
    let b = path(2, &[[30., 0.], [10., 0.]], false, 8., None);
    let joined = a.joined(&b, 0.5).unwrap();
    assert_eq!(points(&joined), [[0., 0.], [10., 0.], [30., 0.]], "touching ends merge, the far path turns around");
    assert_eq!(joined.id, a.id);
    assert!(near(profile(&joined).at(1, 0.5), [2., 2.]), "the other path keeps its own width");
    let c = path(3, &[[50., 0.], [60., 0.]], false, 4., None);
    let bridged = a.joined(&c, 0.5).unwrap();
    assert_eq!(points(&bridged), [[0., 0.], [10., 0.], [50., 0.], [60., 0.]]);
    assert!(bridged.geometry.subpaths()[0].segment(1).is_line(), "a straight segment bridges the gap");
    assert!(near(profile(&bridged).at(1, 0.), [1., 0.8]) && near(profile(&bridged).at(1, 1.), [1., 1.]));
    assert!(a.joined(&path(4, &[[0., 0.], [5., 5.], [9., 0.]], true, 1., None), 0.5).is_err(), "closed paths do not join");
}

#[test]
fn compound_paths_combine_and_release_into_their_subpaths() {
    let outer = path(1, &[[0., 0.], [100., 0.], [100., 100.], [0., 100.]], true, 2., None);
    let inner = path(2, &[[30., 30.], [70., 30.], [70., 70.], [30., 70.]], true, 4., None);
    let compound = PathItem::compound(&[outer.clone(), inner.clone()]).unwrap();
    assert_eq!((compound.id, compound.fill_rule, compound.geometry.subpaths().len()), (outer.id, FillRule::EvenOdd, 2));
    assert!(near(profile(&compound).at(5, 0.5), [2., 2.]), "the inner outline keeps its width");
    let released = compound.released().unwrap();
    assert_eq!(released.iter().map(points).collect::<Vec<_>>(), [points(&outer), points(&inner)]);
}

#[test]
fn object_operations_replace_paths_in_place_with_fresh_identities() {
    let object = VectorObject::from_records(vec![
        Record::Path(tapered(1, &[[0., 0.], [10., 0.], [20., 0.]])),
        Record::Path(path(2, &[[20., 0.], [40., 0.]], false, 4., None)),
        Record::Path(path(3, &[[0., 50.], [10., 50.]], false, 4., None)),
    ]);
    let (joined, id) = object.join_paths(&[ItemId(1), ItemId(2)].into(), 0.5).unwrap().unwrap();
    assert_eq!((id, joined.path_count()), (ItemId(1), 2));
    assert_eq!(points(joined.record(joined.find(id).unwrap()).unwrap().path().unwrap()).len(), 4);
    let (closed, _) = object.join_paths(&[ItemId(3)].into(), 0.5).unwrap().unwrap();
    assert!(closed.paths().any(|(_, p)| p.id == ItemId(3) && p.geometry.subpaths()[0].closed), "Join closes one open path");
    let (split, selected) = object.split_paths(&[(ItemId(1), [AnchorRef { subpath: 0, anchor: 1 }].into())].into()).unwrap().unwrap();
    assert_eq!((split.path_count(), selected.len()), (4, 2));
    assert!(selected.contains(&ItemId(1)));
    split.validate().unwrap();
    let order: Vec<ItemId> = split.paths().map(|(_, p)| p.id).collect();
    assert_eq!(order[0], ItemId(1), "pieces keep the split path's place");
    let (compound, id) = object.compound_paths(&[ItemId(1), ItemId(3)].into()).unwrap().unwrap();
    assert_eq!((id, compound.path_count()), (ItemId(3), 2), "the compound takes the back-most path's style and place");
    let (released, ids) = compound.release_paths(&[id].into()).unwrap().unwrap();
    assert_eq!((released.path_count(), ids.len()), (3, 2));
    released.validate().unwrap();
    assert!(object.release_paths(&[ItemId(1)].into()).unwrap().is_none());
}

#[test]
fn path_commands_keep_single_anchor_dots() {
    let dot = path(1, &[[5., 5.]], false, 6., None);
    assert_eq!(points(&dot.reversed().unwrap()), [[5., 5.]], "a dot reverses to itself");
    assert!(!dot.can_close());
    assert_eq!(dot.split_at(&[AnchorRef { subpath: 0, anchor: 0 }].into()).unwrap(), vec![dot.clone()], "a dot has nothing to split");
    let line = path(2, &[[20., 5.], [40., 5.]], false, 6., None);
    let joined = dot.joined(&line, 0.5).unwrap();
    joined.validate().unwrap();
    assert_eq!(points(&joined), [[5., 5.], [20., 5.], [40., 5.]], "a dot joins a line across a gap");
    let compound = PathItem::compound(&[dot.clone(), line.clone()]).unwrap();
    compound.reversed().unwrap().validate().unwrap();
    assert_eq!(compound.released().unwrap().iter().map(points).collect::<Vec<_>>(), [points(&dot), points(&line)]);
}

#[test]
fn erasing_up_to_intersection_stops_at_locked_lines_and_passes_hidden_ones() {
    let corner = |id: u64, points: &[[f64; 2]]| PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&points.iter().map(|p| Anchor::corner(*p)).collect::<Vec<_>>(), false)]).unwrap()),
        Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(paint(), 4.))])));
    let mut locked = corner(2, &[[300., 100.], [300., 500.]]);
    locked.locked = true;
    let mut hidden = corner(3, &[[400., 100.], [400., 500.]]);
    hidden.hidden = true;
    let object = VectorObject::from_records(vec![Record::Path(corner(1, &[[100., 300.], [700., 300.]])), Record::Path(locked.clone()), Record::Path(hidden.clone()), Record::Path(corner(4, &[[500., 100.], [500., 500.]]))]);
    let erased = object.erase(&mut VectorIndex::default(), &Eraser { from: [450., 290.], to: [450., 310.], radius: 3. }, EraseMode::ToIntersection).unwrap();
    let mut spans: Vec<[f64; 2]> = erased.paths().map(|(_, p)| p.geometry.hull()).filter(|h| h[1] == h[3]).map(|h| [h[0], h[2]]).collect();
    spans.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert!(spans.len() == 2 && (spans[0][1] - 300.).abs() < 0.5 && (spans[1][0] - 500.).abs() < 0.5, "a locked line still stops the eraser and a hidden one does not: {spans:?}");
    for kept in [&locked, &hidden] { assert_eq!(erased.record(erased.find(kept.id).unwrap()).unwrap().path(), Some(kept)); }
}

#[test]
fn mirroring_a_path_keeps_each_side_width_on_its_mirrored_side() {
    let sided = path(1, &[[0., 0.], [40., 10.], [80., 0.]], false, 6., Some(vec![WidthKnot::new(0, 0., 0.2, 1.), WidthKnot::new(1, 1., 0.9, 0.4)]));
    let edge = |item: &PathItem, t: f64, left: bool| {
        let segment = item.geometry.subpaths()[0].segment(0);
        let (p, q) = (segment.point(t), segment.point(t + 1e-6));
        let stroke = item.style.strokes().next().unwrap();
        let [l, r] = stroke.profile.as_ref().unwrap().at(0, t);
        let offset = if left { l } else { -r } * stroke.width * 0.5 / (q[0] - p[0]).hypot(q[1] - p[1]);
        [p[0] + (q[1] - p[1]) * offset, p[1] - (q[0] - p[0]) * offset]
    };
    for affine in [Affine64([-1., 0., 0., 1., 100., 0.]), Affine64([2., 0., 0., -2., 0., 50.]), Affine64([0., 1., 1., 0., 0., 0.])] {
        let mirrored = sided.transformed(affine, true).unwrap();
        for t in [0.1, 0.5, 0.9] {
            for left in [true, false] {
                let ([x, y], [ex, ey]) = (affine.map(edge(&sided, t, left)), edge(&mirrored, t, !left));
                assert!((x - ex).abs() < 1e-3 && (y - ey).abs() < 1e-3, "{affine:?} at {t}: {:?} vs {:?}", [x, y], [ex, ey]);
            }
        }
    }
    let flip = Affine64([-1., 0., 0., 1., 0., 0.]);
    assert_eq!(sided.transformed(flip, true).unwrap().transformed(flip, true).unwrap().style, sided.style, "flipping twice restores the widths");
    let even = path(2, &[[0., 0.], [10., 0.]], false, 2., Some(vec![WidthKnot::new(0, 0., 0.5, 0.5), WidthKnot::new(0, 1., 1., 1.)]));
    assert!(Arc::ptr_eq(&even.transformed(flip, true).unwrap().style, &even.style), "even widths keep their shared style");
}

#[test]
fn joining_or_combining_onto_a_zero_width_outline_keeps_the_other_widths() {
    let hairless = path(1, &[[0., 0.], [10., 0.]], false, 0., None);
    let inked = path(2, &[[10., 0.], [30., 0.]], false, 4., None);
    let width = |path: &PathItem, segment: u32, t: f64| { let stroke = path.style.strokes().next().unwrap(); stroke.profile.as_ref().map_or([1.; 2], |p| p.at(segment, t)).map(|f| f * stroke.width) };
    let joined = hairless.joined(&inked, 0.5).unwrap();
    joined.validate().unwrap();
    assert!(near(width(&joined, 0, 0.5), [0., 0.]) && near(width(&joined, 1, 0.5), [4., 4.]), "the front stays bare and the joined path keeps its width: {:?}", joined.style);
    let compound = PathItem::compound(&[hairless.clone(), inked.clone()]).unwrap();
    compound.validate().unwrap();
    assert!(near(width(&compound, 0, 0.5), [0., 0.]) && near(width(&compound, 1, 0.5), [4., 4.]), "{:?}", compound.style);
    assert_eq!(inked.joined(&hairless, 0.5).unwrap().style.strokes().next().unwrap().width, 4., "a wider front path keeps its own width");
}

#[test]
fn replacing_paths_in_place_matches_a_full_replace_and_needs_no_revalidation() {
    let line = |id: u64| Record::Path(path(id, &[[id as f64, 0.], [id as f64 + 5., 9.]], false, 2., None));
    let chunk = |ids: std::ops::Range<u64>| Resource::new(Arc::new(VectorChunk::new(ids.map(line).collect())));
    let object = VectorObject::from_chunks(vec![chunk(1..11), chunk(11..21), chunk(21..81), chunk(81..87), chunk(87..91)]);
    object.validate().unwrap();
    let ids: BTreeSet<ItemId> = [12, 15, 40, 41, 88].map(ItemId).into();
    let affine = Affine64([1.5, 0.2, -0.1, 0.8, 30., -12.]);
    let transformed = object.transform_items(&ids, affine, true).unwrap();
    let replaced = object.replace(object.selected_paths(&ids).into_iter().map(|(p, path)| (p, Some(Record::Path(path.transformed(affine, true).unwrap())))).collect::<Vec<_>>());
    assert_eq!(transformed, replaced, "the same records as a full replace");
    let sizes = |o: &VectorObject| o.chunks().iter().map(|c| c.records().len()).collect::<Vec<_>>();
    assert_eq!((sizes(&transformed), sizes(&replaced)), (vec![20, 60, 10], vec![20, 60, 10]), "small edited chunks join their neighbours as a full replace joins them");
    assert_eq!(transformed.root.valid.get(), Some(&Ok(())), "an in-place edit of a valid object needs no validation pass");
    assert_eq!(replaced.root.valid.get(), None);
    assert!(ids.iter().all(|id| transformed.find(*id).is_some_and(|p| transformed.record(p).and_then(Record::id) == Some(*id))));
    let (position, first) = object.selected_paths(&ids)[0];
    assert!(object.with_paths([(position, PathItem { id: ItemId(10_000), ..first.clone() })]).is_err(), "a replacement keeps its identity");
    let broken = PathItem { style: Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(paint(), -1.))])), ..first.clone() };
    let invalid = object.with_paths([(position, broken)]).unwrap();
    assert_eq!(invalid.root.valid.get(), None);
    assert!(invalid.validate().is_err(), "an invalid replacement is still refused");
}

#[test]
fn erasing_along_a_frames_segments_matches_erasing_them_one_at_a_time() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; (seed >> 11) as f64 / (1u64 << 53) as f64 };
    let records: Vec<Record> = (1..=600).map(|id| {
        let [x, y] = [next() * 900., next() * 700.];
        Record::Path(path(id, &[[x, y], [x + 90. * next() - 45., y + 80.], [x + 60., y + 120. * next() - 60.]], id % 9 == 0, 3., None))
    }).collect();
    let object = VectorObject::from_records(records);
    let erasers: Vec<Eraser> = (0..12).map(|i| { let t = f64::from(i); Eraser { from: [100. + t * 60., 120. + t * 40.], to: [160. + t * 60., 160. + t * 40. + (t * 1.7).sin() * 30.], radius: 18. } }).collect();
    let shapes = |o: &VectorObject| o.paths().map(|(_, p)| (p.id, p.geometry.clone(), p.style.clone())).collect::<Vec<_>>();
    for mode in [EraseMode::Touched, EraseMode::Whole, EraseMode::ToIntersection] {
        let mut index = VectorIndex::default();
        let one_by_one = erasers.iter().fold(object.clone(), |current, eraser| current.erase(&mut index, eraser, mode).unwrap_or(current));
        let along = object.erase_along(&mut VectorIndex::default(), &erasers, mode).unwrap();
        assert_ne!(shapes(&along), shapes(&object), "{mode:?} erases something");
        assert_eq!(shapes(&along), shapes(&one_by_one), "{mode:?}");
        assert_eq!(along.next_item(), one_by_one.next_item());
        along.validate().unwrap();
    }
}

fn face_line(id: u64, points: &[[f64; 2]], closed: bool, width: f64) -> PathItem {
    PathItem::new(ItemId(id), Arc::new(PathGeometry::new(vec![Subpath::new(&points.iter().copied().map(Anchor::corner).collect::<Vec<_>>(), closed)]).unwrap()), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(paint(), width))])))
}
fn face_at(paths: &[PathItem], point: [f64; 2]) -> VectorFace {
    vector_face(&paths.iter().map(|path| FaceLine { path, offset: [0.; 2] }).collect::<Vec<_>>(), point).expect("a bounded face")
}
fn face_path(face: &VectorFace) -> PathItem {
    let mut path = PathItem::new(ItemId(100_000), Arc::new(face.geometry.clone()), Arc::new(Style::new(vec![PaintEntry::Fill(Fill { paint: paint(), unknown: None })])));
    path.fill_rule = if face.geometry.subpaths().len() > 1 { FillRule::EvenOdd } else { FillRule::NonZero };
    path
}
fn face_box(id: u64, lo: f64, hi: f64) -> PathItem {
    face_line(id, &[[lo, lo], [hi, lo], [hi, hi], [lo, hi]], true, 2.)
}

#[test]
fn vector_face_splits_crossings_and_prunes_overshoots_at_t_junctions() {
    let paths = vec![
        face_line(1, &[[-20., 0.], [120., 0.]], false, 2.),
        face_line(2, &[[100., -20.], [100., 120.]], false, 2.),
        face_line(3, &[[120., 100.], [-20., 100.]], false, 2.),
        face_line(4, &[[0., 120.], [0., -20.]], false, 2.),
        face_line(5, &[[50., 0.], [50., 100.]], false, 2.),
    ];
    let face = face_at(&paths, [25., 50.]);
    let filled = face_path(&face);
    assert_eq!(face.geometry.hull(), [0., 0., 50., 100.]);
    assert!(filled.encloses([25., 50.], 0.001));
    assert!(!filled.encloses([75., 50.], 0.001));
    assert_eq!(face.boundary, [0, 2, 3, 4].into());
}

#[test]
fn vector_face_uses_self_crossings_and_overlapping_shape_boundaries() {
    let bow = face_line(1, &[[0., 0.], [100., 100.], [0., 100.], [100., 0.]], true, 2.);
    let face = face_at(&[bow], [50., 20.]);
    assert!(face_path(&face).encloses([50., 20.], 0.001));
    assert!(!face_path(&face).encloses([50., 80.], 0.001));
    let face = face_at(&[face_box(1, 0., 100.), face_box(2, 50., 150.)], [75., 75.]);
    assert_eq!(face.geometry.hull(), [50., 50., 100., 100.]);
    assert_eq!(face.boundary, [0, 1].into());
}

#[test]
fn vector_face_joins_visible_endpoint_contacts_but_leaves_real_gaps_open() {
    let make = |gap| vec![
        face_line(1, &[[0., 0.], [100., 0.], [100., 100.], [0., 100.]], false, 6.),
        face_line(2, &[[0., 100.], [0., gap]], false, 6.),
    ];
    let face = face_at(&make(3.), [50., 50.]);
    assert!(face_path(&face).encloses([50., 50.], 0.001));
    let paths = make(8.);
    assert!(vector_face(&paths.iter().map(|path| FaceLine { path, offset: [0.; 2] }).collect::<Vec<_>>(), [50., 50.]).is_none());
}

#[test]
fn vector_face_pressure_changes_contact_reach_not_the_centre_line_boundary() {
    let mut square = face_box(1, 0., 100.);
    let mut stroke = square.style.strokes().next().unwrap().clone();
    stroke.width = 40.;
    stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot::new(0, 0., 0.1, 0.8), WidthKnot::new(3, 1., 1., 0.2)]).unwrap()));
    square.style = Arc::new(Style::new(vec![PaintEntry::Stroke(stroke)]));
    let face = face_at(&[square], [50., 50.]);
    assert_eq!(face.geometry.hull(), [0., 0., 100., 100.]);
}

#[test]
fn vector_face_keeps_only_outermost_holes_and_selects_the_innermost_face() {
    let paths = [face_box(1, 0., 200.), face_box(2, 50., 150.), face_box(3, 75., 125.)];
    let face = face_at(&paths, [25., 25.]);
    assert_eq!(face.geometry.subpaths().len(), 2);
    assert_eq!(face.boundary, [0, 1].into());
    let filled = face_path(&face);
    assert!(filled.encloses([25., 25.], 0.001));
    assert!(!filled.encloses([60., 60.], 0.001));
    assert!(!filled.encloses([100., 100.], 0.001));
    let inner = face_at(&paths, [100., 100.]);
    assert_eq!(inner.geometry.hull(), [75., 75., 125., 125.]);
    assert_eq!(inner.boundary, [2].into());
}

#[test]
fn vector_face_doubles_its_window_and_preserves_document_offsets() {
    let square = face_box(1, 0., 1200.);
    let face = vector_face(&[FaceLine { path: &square, offset: [2000., -300.] }], [2600., 300.]).unwrap();
    assert_eq!(face.geometry.hull(), [2000., -300., 3200., 900.]);
    assert_eq!(face.boundary, [0].into());
    let paths = [
        face_line(1, &[[0., 0.], [1200., 0.]], false, 6.),
        face_line(2, &[[1200., 3.], [1200., 100.]], false, 6.),
        face_line(3, &[[1200., 100.], [0., 100.]], false, 6.),
        face_line(4, &[[0., 100.], [0., 3.]], false, 6.),
    ];
    let face = face_at(&paths, [600., 50.]);
    assert_eq!(face.geometry.hull(), [0., 0., 1200., 100.]);
    assert_eq!(face.boundary, [0, 1, 2, 3].into());
    assert!(face_path(&face).encloses([600., 50.], 0.001));
}

#[test]
fn vector_face_busy_page() {
    let mut paths = vec![face_box(1, 0., 100.), face_box(2, 200., 1800.)];
    paths.extend((0..2000).map(|i| {
        let x = 300. + f64::from(i % 50) * 20.;
        let y = 300. + f64::from(i / 50) * 20.;
        face_line(i as u64 + 3, &[[x, y], [x + 10., y + 10.]], false, 2.)
    }));
    let lines: Vec<_> = paths.iter().map(|path| FaceLine { path, offset: [0.; 2] }).collect();
    for (name, point, expected) in [("small", [50., 50.], [0., 0., 100., 100.]), ("large", [1000., 1400.], [200., 200., 1800., 1800.])] {
        let start = std::time::Instant::now();
        let face = vector_face(&lines, point).unwrap();
        let elapsed = start.elapsed().as_micros();
        eprintln!("vector_face_busy_page {name} elapsed_us={elapsed} path_count={}", paths.len());
        assert_eq!(face.geometry.hull(), expected);
        assert!(face_path(&face).encloses(point, 0.001));
    }
}

#[test]
fn line_picking_and_intersection_erasing_ignore_fill_only_paths() {
    let line = face_line(1, &[[0., 50.], [100., 50.]], false, 2.);
    let face = face_at(&[face_box(2, 40., 60.)], [50., 50.]);
    let fill = face_path(&face);
    let object = VectorObject::from_records(vec![Record::Path(fill.clone()), Record::Path(line)]);
    let mut index = VectorIndex::default();
    assert_eq!(index.hit(&object, [50., 45.], 0.1), None);
    assert_eq!(index.hit(&object, [50., 50.], 0.1), Some(ItemId(1)));
    let erased = object.erase(&mut index, &Eraser { from: [10., 50.], to: [10., 50.], radius: 1. }, EraseMode::ToIntersection).unwrap();
    let remaining: Vec<_> = erased.paths().map(|(_, path)| path).collect();
    assert_eq!(remaining, [&fill]);
}

#[test]
fn vector_face_open_short_edge_arcs_do_not_make_their_own_holes() {
    let arc = (0..40).map(|i| { let angle = f64::from(i) * 0.08; [100. + 20. * angle.cos(), 100. + 20. * angle.sin()] }).collect::<Vec<_>>();
    let face = face_at(&[face_box(1, 0., 200.), face_line(2, &arc, false, 6.)], [20., 180.]);
    assert_eq!(face.geometry.subpaths().len(), 1);
    assert_eq!(face.boundary, [0].into());
}

#[test]
fn vector_face_one_stroke_circle_joins_its_nearby_ends() {
    let circle = (0..80).map(|i| { let angle = f64::from(i) / 79. * (std::f64::consts::TAU - 0.08); [100. + 25. * angle.cos(), 100. + 25. * angle.sin()] }).collect::<Vec<_>>();
    let face = face_at(&[face_line(1, &circle, false, 6.)], [100., 100.]);
    assert_eq!(face.geometry.subpaths().len(), 1);
    assert_eq!(face.boundary, [0].into());
    assert!(face_path(&face).encloses([100., 100.], 0.001));
}
