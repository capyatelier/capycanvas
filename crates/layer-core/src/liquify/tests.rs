use super::*;
use crate::{authored::{PaintBase, SourceTarget}, raster::RasterRevision, Document, DocumentNames, Edit, Editor, PortableId};

#[derive(Debug)]
struct TestField;
impl Field for TestField {
    fn content_version(&self)->crate::raster::ContentVersion {crate::raster::ContentVersion::derive(0,&[])}
    fn tiles(&self) -> Vec<[u32; 2]> { vec![[0, 0]] }
    fn as_any(&self) -> &dyn Any { self }
    fn resources(&self, visit: &mut dyn FnMut(u64, usize)) { visit(71, 4096); }
}

fn document() -> Document {
    Document::new(PortableId::random(), 64, 64, DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
}

fn source(document: &Document) -> &PaintSource {
    let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!() };
    document.artwork.paint.get(handle).unwrap()
}

fn with_state(source: &PaintSource, locked: bool) -> (PaintSource, Arc<State>) {
    let original = Context::for_source(source, locked).original;
    let mut result = source.clone();
    result.raster = RasterRevision::pending();
    #[cfg_attr(target_arch="wasm32",expect(clippy::arc_with_non_send_sync,reason="Shared single-threaded Wasm ownership preserves the native Arc contract"))]
    let state = Arc::new(State::new(original, Arc::new(TestField), &result, locked));
    result.raster.set_liquify(state.clone());
    (result, state)
}

#[test]
fn continuation_shares_original_and_field_across_pixel_revisions() {
    let document = document();
    let (paint, state) = with_state(source(&document), false);
    let a = Context::for_source(&paint, false);
    let b = Context::for_source(&paint, false);
    assert_eq!(a, b);
    assert!(Arc::ptr_eq(&a.original, &state.original));
    assert!(Arc::ptr_eq(a.field.as_ref().unwrap(), &state.field));
    let mut later = paint.clone();
    later.raster = RasterRevision::pending();
    later.raster.set_liquify(state.clone());
    assert_eq!(a, Context::for_source(&later, false));
    let mut resources = Vec::new();
    a.field.unwrap().resources(&mut |id, bytes| resources.push((id, bytes)));
    assert_eq!(resources, [(71, 4096)]);
}

#[test]
fn source_and_alpha_lock_changes_start_from_current_pixels() {
    let document = document();
    for locked in [false, true] {
        let (paint, state) = with_state(source(&document), locked);
        assert!(state.compatible(&paint, locked));
        for changed in [
            { let mut changed = paint.clone(); changed.domain[0] += 1; changed },
            { let mut changed = paint.clone(); changed.color_mode = crate::color::LayerColorMode::Grayscale; changed },
            { let mut changed = paint.clone(); changed.base = Some(PaintBase::new(crate::color::source::rgba8_source([64; 2], |_, _| [1, 2, 3, 255]).into())); changed },
        ] {
            assert!(!state.compatible(&changed, locked));
            let fresh = Context::for_source(&changed, locked);
            assert!(fresh.field.is_none());
            assert_eq!(fresh.original.as_ref(), &changed);
            assert!(fresh.original.raster.liquify().is_none());
        }
        let fresh = Context::for_source(&paint, !locked);
        assert!(fresh.field.is_none());
        assert_eq!(fresh.original.raster.identity(), paint.raster.identity());
        assert!(fresh.original.raster.liquify().is_none());
    }
}

#[test]
fn removing_liquify_preserves_pixels_without_retaining_the_session() {
    let document = document();
    let (paint, state) = with_state(source(&document), false);
    let weak = Arc::downgrade(&state);
    let clean = paint.raster.without_liquify();
    assert_eq!(clean.identity(), paint.raster.identity());
    assert_eq!(clean, paint.raster);
    assert!(clean.liquify().is_none());
    drop(state);
    drop(paint);
    assert!(weak.upgrade().is_none());
}

#[test]
fn undo_redo_retain_the_exact_session_on_the_pixel_revision() {
    let mut editor = Editor::new(document());
    let target = editor.document().working.target.unwrap();
    let before = source(editor.document()).raster.clone();
    let (paint, state) = with_state(source(editor.document()), false);
    editor.perform(Edit::SetRaster { target, revision: paint.raster.clone() }).unwrap();
    assert!(editor.undo().unwrap());
    assert_eq!(source(editor.document()).raster.identity(), before.identity());
    assert!(source(editor.document()).raster.liquify().is_none());
    assert!(editor.redo().unwrap());
    let restored = source(editor.document());
    assert_eq!(restored.raster.identity(), paint.raster.identity());
    assert!(Arc::ptr_eq(&restored.raster.liquify().unwrap(), &state));
    assert_eq!(Context::for_source(restored, false), Context::for_source(&paint, false));
}

#[test]
fn context_retains_its_source_without_retaining_unrelated_layers() {
    let mut document = document();
    let image = crate::color::source::rgba8_source([64; 2], |_, _| [4, 5, 6, 255]);
    let weak = Arc::downgrade(&image);
    let mut unrelated = source(&document).clone();
    unrelated.base = Some(PaintBase::new(image.into()));
    document.artwork.paint.insert(PortableId::random(), unrelated).unwrap();
    let context = Context::for_source(source(&document), false);
    assert!(weak.upgrade().is_some());
    drop(document);
    assert!(weak.upgrade().is_none());
    assert_eq!(context.original.domain, [64; 2]);
}

#[derive(Debug)]
struct Resources(Vec<(u64, usize)>);
impl Field for Resources {
    fn content_version(&self)->crate::raster::ContentVersion {crate::raster::ContentVersion::derive(1,&[])}
    fn tiles(&self) -> Vec<[u32; 2]> { vec![[0, 0]] }
    fn as_any(&self) -> &dyn Any { self }
    fn resources(&self, visit: &mut dyn FnMut(u64, usize)) {
        for &(id, bytes) in &self.0 { visit(id, bytes); }
    }
}

fn revision_with_resources(source: &PaintSource, resources: Vec<(u64, usize)>) -> RasterRevision {
    let revision = RasterRevision::backed(Default::default());
    #[cfg_attr(target_arch="wasm32",expect(clippy::arc_with_non_send_sync,reason="Shared single-threaded Wasm ownership preserves the native Arc contract"))]
    revision.set_liquify(Arc::new(State::new(Context::for_source(source, false).original,
        Arc::new(Resources(resources)), source, false)));
    revision
}

#[test]
fn history_and_resident_accounting_count_shared_gpu_allocations_once() {
    let document = document();
    let target = document.working.target.unwrap();
    let a = revision_with_resources(source(&document), vec![(100, 4096), (101, 2048)]);
    let b = revision_with_resources(source(&document), vec![(100, 4096), (102, 1024)]);
    let a_entry = crate::HistoryEntry::new(Edit::SetRaster { target, revision: a.clone() }, 0);
    let b_entry = crate::HistoryEntry::new(Edit::SetRaster { target, revision: b.clone() }, 0);
    let mut accounting = crate::history_budget::Accounting::new(&document);
    assert_eq!(accounting.charge(&a_entry), a_entry.metadata_bytes + 6144);
    assert_eq!(accounting.charge(&b_entry), b_entry.metadata_bytes + 1024);
    assert_eq!(accounting.charge(&a_entry), a_entry.metadata_bytes);
    let mut editor = Editor::new(document);
    editor.perform(Edit::SetRaster { target, revision: a }).unwrap();
    editor.perform(Edit::SetRaster { target, revision: b }).unwrap();
    assert_eq!(editor.retained_tiles().resident_bytes(), 7168);
}

#[test]
fn budget_discards_historical_maps_before_pixel_undo_steps() {
    let mut editor = Editor::new(document());
    let target = editor.document().working.target.unwrap();
    let historical = revision_with_resources(source(editor.document()), vec![(200, 8192)]);
    editor.perform(Edit::SetRaster { target, revision: historical.clone() }).unwrap();
    let current = revision_with_resources(source(editor.document()), vec![(201, 2048)]);
    editor.perform(Edit::SetRaster { target, revision: current.clone() }).unwrap();
    editor.trim_history(4096);
    assert!(historical.liquify().is_none());
    assert!(current.liquify().is_some());
    assert!(editor.undo().unwrap());
    assert_eq!(source(editor.document()).raster.identity(), historical.identity());
    assert!(editor.undo().unwrap());
    assert!(editor.redo().unwrap());
    assert!(editor.redo().unwrap());
    assert_eq!(source(editor.document()).raster.identity(), current.identity());
}

#[test]
fn retained_tiles_keep_starting_pixels_after_the_live_source_replaces_them() {
    let mut document = document();
    let image = crate::color::source::rgba8_source([64; 2], |_, _| [9, 8, 7, 255]);
    let weak = Arc::downgrade(&image);
    let mut original = source(&document).clone();
    original.base = Some(PaintBase::new(image.into()));
    let SourceTarget::Paint(handle) = document.working.target.unwrap() else { panic!() };
    let output = document.artwork.paint.get_mut(handle).unwrap();
    output.raster = RasterRevision::backed(Default::default());
    #[cfg_attr(target_arch="wasm32",expect(clippy::arc_with_non_send_sync,reason="Shared single-threaded Wasm ownership preserves the native Arc contract"))]
    output.raster.set_liquify(Arc::new(State::new(Arc::new(original), Arc::new(TestField), output, false)));
    let editor = Editor::new(document);
    let retained = editor.retained_tiles();
    assert!(!retained.try_blobs().unwrap().unwrap().is_empty());
    assert!(retained.resident_bytes() > 4096);
    let revision = source(editor.document()).raster.clone();
    revision.drop_liquify();
    assert!(weak.upgrade().is_none());
    assert!(!retained.try_blobs().unwrap().unwrap().is_empty());
}
