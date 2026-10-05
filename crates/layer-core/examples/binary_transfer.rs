use layer_core::{package::codec::PreparedPackage, *};
use std::{
    io::Write,
    sync::{Arc, atomic::AtomicBool},
};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("Pass an output .capy path and an RGB ICC profile path")?;
    let profile_path = args.next().ok_or("Pass an RGB ICC profile path")?;
    let mut profile = std::fs::read(profile_path).map_err(|e| e.to_string())?;
    let size = 2 * 1024 * 1024;
    if profile.len() < 132 || profile.len() > size - 28 || &profile[36..40] != b"acsp" || &profile[16..20] != b"RGB " {
        return Err("Fixture requires a complete RGB ICC profile smaller than 2 MiB".into());
    }
    let integer = |at| u32::from_be_bytes(profile[at..at + 4].try_into().unwrap()) as usize;
    let count = integer(128);
    let table_end = count.checked_mul(12).and_then(|n| n.checked_add(132)).filter(|end| *end <= profile.len()).ok_or("Invalid ICC tag table")?;
    if integer(0) != profile.len() { return Err("ICC declared length must match the input file".into()); }
    for at in (132..table_end).step_by(12) {
        let offset = integer(at + 4);
        if offset < table_end || offset.checked_add(integer(at + 8)).is_none_or(|end| end > profile.len()) {
            return Err("Invalid ICC tag range".into());
        }
    }
    drop(profile.splice(table_end..table_end, [0; 12]));
    for at in (132..table_end).step_by(12) {
        let offset = u32::from_be_bytes(profile[at + 4..at + 8].try_into().unwrap()) + 12;
        profile[at + 4..at + 8].copy_from_slice(&offset.to_be_bytes());
    }
    let private_offset = profile.len().next_multiple_of(4);
    profile.resize(size, 0);
    profile[..4].copy_from_slice(&(size as u32).to_be_bytes());
    profile[84..100].fill(0);
    profile[128..132].copy_from_slice(&((count + 1) as u32).to_be_bytes());
    profile[table_end..table_end + 4].copy_from_slice(b"tst0");
    profile[table_end + 4..table_end + 8].copy_from_slice(&(private_offset as u32).to_be_bytes());
    profile[table_end + 8..table_end + 12].copy_from_slice(&((size - private_offset) as u32).to_be_bytes());
    profile[private_offset..private_offset + 4].copy_from_slice(b"data");
    profile[private_offset + 4..private_offset + 8].fill(0);
    profile[private_offset + 8..private_offset + 12].copy_from_slice(&1u32.to_be_bytes());
    for (i, byte) in profile[private_offset + 12..].iter_mut().enumerate() { *byte = i as u8; }
    std::fs::write(format!("{path}.icc"), &profile).map_err(|e| e.to_string())?;
    let [width, height] = [9504, 6336];
    let document = Document::new(PortableId::random(), width, height, DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    let ink = document.scene().children(None)[0];
    let SourceTarget::Paint(paint) = document.scene().source_target(ink).ok_or("Missing fixture paint")? else {
        return Err("Invalid fixture paint".into());
    };
    let words = (0..height)
        .flat_map(|y| (0..width / 4).map(move |x| u32::from_le_bytes(std::array::from_fn(|c| ((x * 4 + c as u32) / 37 + y / 19) as u8))))
        .collect::<Vec<_>>();
    let selection =
        Selection::pixels(Arc::new(SelectionPixels::bytes([width, height], [0, 0, width, height], words).map_err(|e| e.to_string())?));
    let mut artwork = document.artwork;
    let saved =
        artwork.selections.insert(PortableId::random(), SavedSelection { selection: selection.clone(),})?;
    let occurrence =
        artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Selection(saved), "Shared mask"))?;
    let stack = artwork.compositions.get(artwork.root).ok_or("Missing fixture composition")?.result;
    artwork.stacks.get_mut(stack).ok_or("Missing fixture stack")?.entries.insert(0, occurrence);
    let target = artwork.coverage.next_handle();
    let mut coverage = CoverageSnapshot::reveal_all(target, [width, height], Point::default());
    coverage.source.initial = Some(selection);
    artwork.coverage.insert(PortableId::random(), coverage.source)?;
    artwork.occurrences.get_mut(ink).ok_or("Missing fixture occurrence")?.mask = Some(coverage.use_);
    let proof = color::ProofRecipe::new(
        "RGB 2 MiB profile".into(),
        color::ColorProfile::Icc(profile.into()),
    );
    let mut source = color::source::SourceBuilder::new(
        [1, 1],
        color::source::SourceInterpretation {
            channels: color::source::SourceChannels::Rgba,
            depth: color::SampleDepth::U8,
            profile: proof.profile.clone(),
            profile_assumed: false,
        },
        1024 * 1024,
    )?;
    source.push_row(&[17, 33, 65, 255])?;
    artwork.paint.get_mut(paint).ok_or("Missing fixture source")?.base = Some(layer_core::PaintBase::new(Arc::new(source.finish()?).into()));
    artwork.outputs.get_mut(artwork.default_output).ok_or("Missing fixture output")?.proof = Some(proof);
    let capture = artwork.capture(CaptureCheckpoint {
        owner: document.owner,
        document: artwork.id,
        session_generation: 0,
        artwork_generation: 0,
        working_generation: 0,
        edit_checkpoint: 0,
    })?;
    let mut output = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    let cancelled = AtomicBool::new(false);
    PreparedPackage::prepare(&capture, None, &cancelled)?.write(&mut output, &cancelled)?;
    output.flush().map_err(|e| e.to_string())?;
    println!("Generated {} bytes in {:?}", output.metadata().map_err(|e| e.to_string())?.len(), start.elapsed());
    Ok(())
}
