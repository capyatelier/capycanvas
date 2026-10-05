//! Deterministic 9504 x 6336 sRGB image with an empty paint layer for replays.
//! Usage: photo_fixture OUTPUT.capy [--source-layer]
use layer_core::{color::{ColorProfile, RgbSpace, SampleDepth, source::*}, Edit, Editor, EvaluationContext, Occurrence, OccurrenceContent, PaintSource, RecordChange};
use std::{fs::File, io::BufWriter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("OUTPUT.capy is required")?;
    let [width, height] = [9504u32, 6336u32];
    let mut source = SourceBuilder::new([width, height], SourceInterpretation {
        channels: SourceChannels::Rgba, depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false,
    }, 256 * 1024 * 1024)?;
    for y in 0..height {
        let mut row = Vec::with_capacity(width as usize * 4);
        for x in 0..width {
            let noise = x.wrapping_mul(1664525).wrapping_add(y.wrapping_mul(1013904223));
            row.extend_from_slice(&[(x * 255 / (width - 1)) as u8,
                (y * 255 / (height - 1)) as u8, (noise >> 24) as u8, 255]);
        }
        source.push_row(&row)?;
    }
    let mut project = layer_color::photo_project(source.finish()?, Default::default(),
        layer_core::DocumentNames { paint: "Synthetic 61 MP source".into(), paper: "Paper".into() }, SampleDepth::U8)?;
    let source_layer=std::env::args().any(|arg|arg=="--source-layer");
    let paint=RecordChange::insert(&project.artwork.paint,PaintSource { color_mode: Default::default(),domain:[width,height],raster:Default::default(),original:None,operations:Default::default()});
    let occurrence=RecordChange::insert(&project.artwork.occurrences,Occurrence::new(OccurrenceContent::Paint(paint.handle),"Benchmark ink"));
    let mut working=project.working.clone();
    if !source_layer {working.occurrence=Some(occurrence.handle);working.target=Some(layer_core::SourceTarget::Paint(paint.handle));}
    let stack=project.composition().result;
    let mut entries=project.artwork.stacks.get(stack).unwrap().clone();
    entries.entries.insert(usize::from(source_layer),occurrence.handle);
    let membership=RecordChange::replace(&project.artwork.stacks,stack,Some(entries))?;
    project.apply(Edit::Batch(vec![Edit::Paint(paint),Edit::Occurrence(occurrence),Edit::Stack(membership),Edit::Working(working)]))?;
    let capture=Editor::new(project.clone()).capture(0,EvaluationContext::default())?;
    let cancelled=std::sync::atomic::AtomicBool::new(false);
    let prepared=layer_core::package::codec::PreparedPackage::prepare(&capture,None,&cancelled)?;
    prepared.write(&mut BufWriter::new(File::create(output)?),&cancelled)?;
    println!("Created {width} x {height} synthetic image, hidden paper, active layer {:?}", project.working.occurrence);
    Ok(())
}
