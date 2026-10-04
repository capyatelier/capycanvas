use super::*;
use color::{ColorProfile, RgbSpace, SampleDepth, source::*};
fn document(size: [u32; 2]) -> Document {
    Document::new(
        PortableId::random(),
        size[0],
        size[1],
        DocumentNames {
            paint: "Current ink".into(),
            paper: "Paper".into(),
        },
    )
}
fn selection_edit(document: &Document, selection: Option<Selection>) -> Edit {
    let mut working = document.working.clone();
    working.selection = selection;
    Edit::Working(working)
}
fn paint_handle(document: &Document) -> PaintHandle {
    let SourceTarget::Paint(h) = document.working.target.unwrap() else {
        panic!()
    };
    h
}
fn paint_change(document: &Document, h: PaintHandle, source: Option<Arc<SourceImage>>) -> Edit {
    let mut s = document.artwork.paint.get(h).unwrap().clone();
    s.original = source;
    Edit::Paint(RecordChange::replace(&document.artwork.paint, h, Some(s)).unwrap())
}
#[test]
fn selection_history_charges_shared_coverage_once_and_rejects_oversized_edits() {
    let mask = Selection::pixels(Arc::new(
        SelectionPixels::bytes([1024, 1], [0, 0, 1024, 1], vec![u32::MAX; 256]).unwrap(),
    ));
    let mut d = document([1024, 1]);
    d.working.selection = Some(mask.clone());
    let entry = HistoryEntry::new(selection_edit(&d, Some(mask.clone())), 0);
    assert!(
        entry.metadata_bytes < 1024,
        "coverage is not serialized into history metadata"
    );
    assert_eq!(Accounting::new(&d).charge(&entry), entry.metadata_bytes);
    let mut accounting = Accounting::default();
    assert_eq!(accounting.charge(&entry), entry.metadata_bytes + 1024);
    assert_eq!(accounting.charge(&entry), entry.metadata_bytes);
    let mut editor = Editor::new(document([1024, 1]));
    let before = editor.document.clone();
    let edit = selection_edit(editor.document(), Some(mask));
    assert!(editor.perform_with_history_budget(edit, 1023).is_err());
    assert_eq!(editor.document, before);
    assert!(!editor.can_undo());
}
#[test]
fn retained_selection_inventory_counts_binary_ownership_across_history_and_masks() {
    let extent = [9504, 6336];
    let bytes = extent[0] as usize * extent[1] as usize;
    let selection = Selection::pixels(Arc::new(
        SelectionPixels::bytes(
            extent,
            [0, 0, extent[0], extent[1]],
            vec![u32::MAX; bytes / 4],
        )
        .unwrap(),
    ));
    let mut d = document(extent);
    d.working.selection = Some(selection.clone());
    let h = d.working.occurrence.unwrap();
    let coverage = RecordChange::insert(
        &d.artwork.coverage,
        CoverageSource {
            domain: extent,
            raster: Default::default(),
            initial: Some(selection.clone()),
            default_coverage: 1.,
            operations: Default::default(),
        },
    );
    let coverage_handle = coverage.handle;
    let saved = RecordChange::insert(
        &d.artwork.selections,
        SavedSelection {
            selection: selection.clone(),
            display: Default::default(),
        },
    );
    let mut occurrence = d.artwork.occurrences.get(h).unwrap().clone();
    occurrence.mask = Some(MaskUse {
        source: coverage_handle,
        enabled: true,
        linked: true,
        inverted: false,
        translation: Point::default(),
        placement: Projective::IDENTITY,
    });
    d.apply(Edit::Batch(vec![
        Edit::Coverage(coverage),
        Edit::SavedSelection(saved),
        Edit::Occurrence(
            RecordChange::replace(&d.artwork.occurrences, h, Some(occurrence)).unwrap(),
        ),
    ]))
    .unwrap();
    let mut editor = Editor::new(d);
    let retained = editor.retained_tiles().metadata_bytes;
    assert!(
        (bytes..bytes + 32 * 1024).contains(&retained),
        "Shared 61 MP coverage is one 60 MB allocation: {retained}"
    );
    editor
        .perform(selection_edit(editor.document(), None))
        .unwrap();
    let retained = editor.retained_tiles().metadata_bytes;
    assert!((bytes..bytes + 32 * 1024).contains(&retained));
    let s = editor
        .document()
        .artwork
        .coverage
        .get(coverage_handle)
        .unwrap()
        .clone();
    let entry = HistoryEntry::new(
        Edit::Coverage(
            RecordChange::replace(
                &editor.document().artwork.coverage,
                coverage_handle,
                Some(s),
            )
            .unwrap(),
        ),
        0,
    );
    assert!(
        entry.metadata_bytes < 16 * 1024,
        "Initial mask pixels stay out of history metadata"
    );
    let mut accounting = Accounting::default();
    assert_eq!(accounting.charge(&entry), bytes + entry.metadata_bytes);
    assert_eq!(accounting.charge(&entry), entry.metadata_bytes);
}
#[test]
fn admitted_native_output_reservation_is_shared_until_tile_publication() {
    let d = document([256; 2]);
    let revision = raster::RasterRevision::pending();
    let h = paint_handle(&d);
    let mut s = d.artwork.paint.get(h).unwrap().clone();
    s.raster = revision.clone();
    let entry = HistoryEntry::new(
        Edit::Paint(RecordChange::replace(&d.artwork.paint, h, Some(s)).unwrap()),
        0,
    );
    let charge = || Accounting::new(&d).charge(&entry);
    let initial = charge();
    revision.clone().reserve_pending_bytes(480 * 1024 * 1024);
    assert_eq!(
        charge() - initial,
        480 * 1024 * 1024 - raster::MAX_CAPTURE_BYTES as usize
    );
    revision.reserve_pending_bytes(1);
    assert_eq!(
        charge() - initial,
        480 * 1024 * 1024 - raster::MAX_CAPTURE_BYTES as usize
    );
    revision.publish(Ok(raster::RasterData::default())).unwrap();
    assert_eq!(charge(), initial - raster::MAX_CAPTURE_BYTES as usize);
}
fn source() -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        [1, 1],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U8,
            profile: ColorProfile::Icc(vec![19; 16 * 1024].into()),
            profile_assumed: false,
        },
        256 * 256 * 4,
    )
    .unwrap();
    builder.push_row(&[31, 127, 200, 255]).unwrap();
    Arc::new(builder.finish().unwrap())
}
#[test]
fn over_budget_source_changes_reject_atomically_in_both_directions() {
    for remove in [false, true] {
        let mut d = document([256; 2]);
        let h = paint_handle(&d);
        let original = source();
        if remove {
            d.artwork.paint.get_mut(h).unwrap().original = Some(original.clone());
        }
        let mut editor = Editor::new(d);
        let occurrence = editor.document().working.occurrence.unwrap();
        let mut value = editor
            .document()
            .artwork
            .occurrences
            .get(occurrence)
            .unwrap()
            .clone();
        value.opacity = 0.7;
        editor
            .perform(Edit::Occurrence(
                RecordChange::replace(
                    &editor.document().artwork.occurrences,
                    occurrence,
                    Some(value),
                )
                .unwrap(),
            ))
            .unwrap();
        editor.undo().unwrap();
        let before = editor.document.clone();
        let checkpoint = editor.checkpoint();
        let next = editor.next_checkpoint;
        let edit = paint_change(editor.document(), h, (!remove).then_some(original));
        let error = editor
            .perform_with_history_budget(edit.clone(), 8192)
            .unwrap_err();
        assert!(error.to_string().contains("Undo/Redo memory limit"));
        assert_eq!(editor.document, before);
        assert_eq!(editor.checkpoint(), checkpoint);
        assert_eq!(editor.next_checkpoint, next);
        assert!(editor.can_redo());
        assert!(!editor.can_undo());
        editor.perform_with_history_budget(edit, 65536).unwrap();
        let after = editor.document.clone();
        assert!(editor.can_undo());
        assert!(!editor.can_redo());
        editor.undo().unwrap();
        assert_eq!(editor.document.artwork, before.artwork);
        editor.redo().unwrap();
        assert_eq!(editor.document.artwork, after.artwork);
    }
}
#[test]
fn reinterpretation_charges_shared_tiles_once_and_new_profile_ownership() {
    let mut d = document([256; 2]);
    let h = paint_handle(&d);
    let original = source();
    d.artwork.paint.get_mut(h).unwrap().original = Some(original.clone());
    let mut repaired = original.as_ref().clone();
    repaired.interpretation.profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    let mut editor = Editor::new(d);
    let edit = paint_change(editor.document(), h, Some(Arc::new(repaired)));
    assert!(
        editor
            .perform_with_history_budget(edit.clone(), 8192)
            .is_err()
    );
    editor.perform_with_history_budget(edit, 65536).unwrap();
    let repaired = editor
        .document()
        .artwork
        .paint
        .get(h)
        .unwrap()
        .original
        .as_ref()
        .unwrap();
    assert!(Arc::ptr_eq(
        &repaired.tiles[&[0, 0]],
        &original.tiles[&[0, 0]]
    ));
    editor.undo().unwrap();
    assert!(Arc::ptr_eq(
        editor
            .document()
            .artwork
            .paint
            .get(h)
            .unwrap()
            .original
            .as_ref()
            .unwrap(),
        &original
    ));
}
