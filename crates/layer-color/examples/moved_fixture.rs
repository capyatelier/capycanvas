use layer_core::{DocumentNames, Editor, EvaluationContext, SourceTarget, authored::*, color::SampleDepth};
use std::{fs::File, io::{BufReader, BufWriter, Write}, sync::atomic::AtomicBool};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let input = args.first().ok_or("PHOTO.jpg OUTPUT.capy X Y")?;
    let output = args.get(1).ok_or("OUTPUT.capy is required")?;
    let offset = [args.get(2).ok_or("X is required")?.parse()?, args.get(3).ok_or("Y is required")?.parse()?];
    let source = layer_color::photo::read_photo(BufReader::new(File::open(input)?), Default::default())?;
    let document = layer_color::photo_project(source, Default::default(), DocumentNames {paint:"Photo".into(),paper:"Paper".into()}, SampleDepth::U8)?;
    let SourceTarget::Paint(paint) = document.working.target.ok_or("Photo paint target is unavailable")? else { return Err("Photo source is not paint".into()); };
    let base = document.scene().source_owner(SourceTarget::Paint(paint)).ok_or("Photo layer is unavailable")?;
    let mut artwork = document.artwork.clone();
    let copy = artwork.paint.insert(PortableId::random(), artwork.paint.get(paint).ok_or("Photo source is unavailable")?.clone())?;
    let mut moved = Occurrence::new(OccurrenceContent::Paint(copy), "Moved");
    moved.offset = offset;
    moved.opacity = 0.35;
    let moved = artwork.occurrences.insert(PortableId::random(), moved)?;
    let root = artwork.compositions.get(artwork.root).ok_or("Root composition is unavailable")?.result;
    let entries = &mut artwork.stacks.get_mut(root).ok_or("Root stack is unavailable")?.entries;
    let at = entries.iter().position(|handle| *handle == base).ok_or("Photo is not a root layer")?;
    entries.insert(at, moved);
    let mut document = layer_core::Document::from_artwork(artwork)?;
    document.working.target = Some(SourceTarget::Paint(copy));
    document.working.occurrence = Some(moved);
    let capture = Editor::new(document).capture(0, EvaluationContext::default())?;
    let cancelled = AtomicBool::new(false);
    let prepared = layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancelled)?;
    let mut writer = BufWriter::new(File::create(output)?);
    prepared.write(&mut writer, &cancelled)?;
    writer.flush()?;
    println!("moved copy at {offset:?}");
    Ok(())
}
