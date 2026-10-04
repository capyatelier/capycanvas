use crate::*;
use std::sync::Arc;
pub fn document(size: [u32; 2], names: &[&str]) -> Document {
    let mut doc = Document::new(PortableId::random(), size[0], size[1], DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
    let root = doc.composition().result;
    let original = doc.scene().children(None)[0];
    doc.artwork.stacks.get_mut(root).unwrap().entries.remove(0);
    doc.artwork.occurrences.remove(original);
    for (index, name) in names.iter().enumerate() {
        insert_paint(&mut doc, *name, index, None);
    }
    doc.working.occurrence = doc.scene().children(None).first().copied();
    doc.working.target = doc.working.occurrence.and_then(|h| doc.scene().source_target(h));
    doc
}
pub fn refresh(doc: &mut Document) {
    doc.scene_index = Arc::new(SceneIndex::build(&doc.artwork).unwrap());
}
pub fn id(doc: &Document, name: &str) -> OccurrenceHandle {
    doc.scene().order().iter().copied().find(|h| doc.scene().occurrence(*h).unwrap().name.as_ref() == name).unwrap()
}
pub fn occurrence<'a>(doc: &'a Document, name: &str) -> &'a Occurrence {
    doc.artwork.occurrences.get(id(doc, name)).unwrap()
}
pub fn occurrence_mut<'a>(doc: &'a mut Document, name: &str) -> &'a mut Occurrence {
    let h = id(doc, name);
    doc.artwork.occurrences.get_mut(h).unwrap()
}
pub fn paint<'a>(doc: &'a Document, name: &str) -> &'a PaintSource {
    doc.scene().paint_source(id(doc, name)).unwrap()
}
pub fn paint_mut<'a>(doc: &'a mut Document, name: &str) -> &'a mut PaintSource {
    let OccurrenceContent::Paint(h) = occurrence(doc, name).content else { panic!("paint") };
    doc.artwork.paint.get_mut(h).unwrap()
}
pub fn target(doc: &Document, name: &str) -> SourceTarget {
    doc.scene().source_target(id(doc, name)).unwrap()
}
pub fn names(doc: &Document) -> Vec<String> {
    doc.scene().order().iter().map(|h| doc.scene().occurrence(*h).unwrap().name.to_string()).collect()
}
pub fn insert_paint(doc: &mut Document, name: impl Into<Arc<str>>, index: usize, parent: Option<OccurrenceHandle>) -> OccurrenceHandle {
    let p = doc
        .artwork
        .paint
        .insert(
            PortableId::random(),
            PaintSource { domain: doc.composition().size, raster: Default::default(), original: None, operations: Arc::default() },
        )
        .unwrap();
    let o = doc.artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(p), name)).unwrap();
    let stack = parent
        .map(|h| match doc.artwork.occurrences.get(h).unwrap().content {
            OccurrenceContent::Stack(s) => s,
            _ => panic!("group"),
        })
        .unwrap_or(doc.composition().result);
    doc.artwork.stacks.get_mut(stack).unwrap().entries.insert(index, o);
    refresh(doc);
    o
}
pub fn nest(doc: &mut Document, group: &str, children: &[&str]) -> OccurrenceHandle {
    let h = id(doc, group);
    let members: Vec<_> = children.iter().map(|name| id(doc, name)).collect();
    let s = doc.artwork.stacks.insert(PortableId::random(), Stack { entries: members.clone() }).unwrap();
    let stacks: Vec<_> = doc.artwork.stacks.iter().map(|(h, _, _)| h).filter(|h| *h != s).collect();
    for stack in stacks {
        doc.artwork.stacks.get_mut(stack).unwrap().entries.retain(|h| !members.contains(h));
    }
    doc.artwork.occurrences.get_mut(h).unwrap().content = OccurrenceContent::Stack(s);
    refresh(doc);
    if doc.working.occurrence == Some(h) { doc.working.target = doc.scene().source_target(h); }
    h
}
pub fn add_mask(doc: &mut Document, owner: OccurrenceHandle, domain: [u32; 2], translation: Point) -> CoverageHandle {
    let c = doc.artwork.coverage.next_handle();
    let snapshot = CoverageSnapshot::reveal_all(c, domain, translation);
    let h = doc.artwork.coverage.insert(PortableId::random(), snapshot.source).unwrap();
    doc.artwork.occurrences.get_mut(owner).unwrap().mask = Some(snapshot.use_);
    refresh(doc);
    h
}
pub fn effect(doc: &mut Document, name: &str, program: &str) {
    let h = id(doc, name);
    let program = bundled_effect_catalog().get(program).unwrap().program();
    let draft = EffectInstance::new(program.clone());
    let d = doc.artwork.definitions.insert(PortableId::random(), Definition { program, dimensions: Default::default() }).unwrap();
    let e = doc
        .artwork
        .effects
        .insert(PortableId::random(), EffectApplication { definition: d, values: draft.values, domain: doc.composition().size })
        .unwrap();
    doc.artwork.occurrences.get_mut(h).unwrap().content = OccurrenceContent::Effect(e);
    refresh(doc);
    if doc.working.occurrence == Some(h) { doc.working.target = doc.scene().source_target(h); }
}
pub fn saved(doc: &mut Document, name: &str, selection: Selection) {
    let h = id(doc, name);
    let s = doc.artwork.selections.insert(PortableId::random(), SavedSelection { selection, display: Default::default() }).unwrap();
    doc.artwork.occurrences.get_mut(h).unwrap().content = OccurrenceContent::Selection(s);
    refresh(doc);
    if doc.working.occurrence == Some(h) { doc.working.target = doc.scene().source_target(h); }
}
pub fn activate(doc: &mut Document, name: &str) {
    let h = id(doc, name);
    doc.working.occurrence = Some(h);
    doc.working.target = doc.scene().source_target(h);
}
pub fn restored(before: &Document, after: &Document) {
    assert_eq!(
        after.artwork.occurrences.iter().map(|(h, id, v)| (h, id, v)).collect::<Vec<_>>(),
        before.artwork.occurrences.iter().map(|(h, id, v)| (h, id, v)).collect::<Vec<_>>()
    );
    assert_eq!(after.scene().order(), before.scene().order());
    assert_eq!(after.working.occurrence, before.working.occurrence);
    assert_eq!(after.working.target, before.working.target);
}
pub fn encoded(doc: &Document) -> Vec<u8> {
    let capture = doc
        .artwork
        .capture(CaptureCheckpoint {
            document: doc.artwork.id,
            owner: doc.owner,
            session_generation: 0,
            artwork_generation: doc.revision,
            working_generation: doc.working.generation,
            edit_checkpoint: 0,
        })
        .unwrap();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let package = crate::package::codec::PreparedPackage::prepare(&capture, None, &cancelled).unwrap();
    let mut bytes = Vec::new();
    package.write(&mut bytes, &cancelled).unwrap();
    bytes
}
pub fn decoded(bytes: Vec<u8>) -> Document {
    let chunks=bytes.chunks(crate::package::MAX_RANGE_BYTES).map(Arc::<[u8]>::from).collect();
    let source=crate::package::ImmutableBacking::new(Arc::new(crate::package::transport::ChunkedBytes::new(chunks).unwrap())).unwrap();
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let crate::package::codec::OpenOutcome::Candidate { artwork, .. } =
        crate::package::codec::open(source, Default::default(), &cancelled).unwrap()
    else {
        panic!("editable artwork")
    };
    Document::from_artwork(artwork).unwrap()
}
pub fn roundtrip(doc: &Document) -> Document {
    decoded(encoded(doc))
}
