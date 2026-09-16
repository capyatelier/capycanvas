use super::*;
use color::{ColorProfile, IntegerDepth, RgbSpace, source::*};

#[test]
fn admitted_native_output_reservation_is_shared_until_tile_publication() {
    let document = Document::new("pending native output", 256, 256);
    let revision = raster::RasterRevision::pending();
    let mut layer = document.layers[0].clone();
    layer.raster = revision.clone();
    let entry = HistoryEntry::new(Edit::ReplaceLayer(Box::new(layer)), 0);
    let charge = || Accounting::new(&document).charge(&entry);
    let initial = charge();
    revision.clone().reserve_pending_bytes(480 * 1024 * 1024);
    assert_eq!(charge() - initial, 480 * 1024 * 1024 - raster::MAX_CAPTURE_BYTES as usize);
    revision.reserve_pending_bytes(1);
    assert_eq!(charge() - initial, 480 * 1024 * 1024 - raster::MAX_CAPTURE_BYTES as usize);
    revision.publish(Ok(raster::RasterData::default())).unwrap();
    assert_eq!(charge(), initial - raster::MAX_CAPTURE_BYTES as usize);
}

fn source() -> Arc<SourceImage> {
    let mut builder = SourceBuilder::new(
        [1, 1],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: IntegerDepth::U8,
            profile: ColorProfile::Icc(vec![19; 16 * 1024].into()),
            profile_assumed: false,
        },
        1024,
    )
    .unwrap();
    builder.push_row(&[31, 127, 200, 255]).unwrap();
    Arc::new(builder.finish().unwrap())
}

#[test]
fn over_budget_source_changes_reject_atomically_in_both_directions() {
    // Small explicit limits exercise the real admission path without allocating
    // a half-gigabyte image. ICC contents are immaterial to ownership accounting.
    for remove in [false, true] {
        let mut document = Document::new("admission", 256, 256);
        let original = source();
        if remove {
            document.layers[0].source = Some(original.clone());
        }
        let mut editor = Editor::new(document);
        editor
            .perform(Edit::SetLayerOpacity {
                id: LayerId(1),
                opacity: 0.7,
            })
            .unwrap();
        editor.undo().unwrap();
        let before = editor.document.clone();
        let checkpoint = editor.checkpoint();
        let next = editor.next_checkpoint;
        let mut layer = editor.document.layers[0].clone();
        layer.source = (!remove).then_some(original);
        let edit = Edit::ReplaceLayer(Box::new(layer));
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
        assert_eq!(editor.document.layers, before.layers);
        editor.redo().unwrap();
        assert_eq!(editor.document.layers, after.layers);
    }
}

#[test]
fn reinterpretation_charges_shared_tiles_once_and_new_profile_ownership() {
    let mut document = Document::new("source repair", 256, 256);
    let original = source();
    document.layers[0].source = Some(original.clone());
    let mut layer = document.layers[0].clone();
    let mut repaired = original.as_ref().clone();
    repaired.interpretation.profile = ColorProfile::Builtin(RgbSpace::DisplayP3);
    layer.source = Some(Arc::new(repaired));
    let mut editor = Editor::new(document);
    let edit = Edit::ReplaceLayer(Box::new(layer));
    // Removing a retained ICC still needs to keep those bytes for Undo.
    assert!(
        editor
            .perform_with_history_budget(edit.clone(), 8192)
            .is_err()
    );
    editor.perform_with_history_budget(edit, 65536).unwrap();
    let repaired = editor.document.layers[0].source.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &repaired.tiles[&[0, 0]],
        &original.tiles[&[0, 0]]
    ));
    editor.undo().unwrap();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].source.as_ref().unwrap(),
        &original
    ));
}
