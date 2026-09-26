//! Deterministic 9504 x 6336 sRGB image with an empty paint layer for replays.
//! Usage: photo_fixture OUTPUT.capy [--source-layer]
use layer_core::{color::{ColorProfile, RgbSpace, SampleDepth, source::*}, Layer};
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
    let mut project = layer_color::photo_project(source.finish()?,
        "Synthetic 61 MP source", SampleDepth::U8)?;
    let ink = project.document.allocate_layer_id();
    project.document.layers.insert(0, Layer::paint(ink, "Benchmark ink"));
    if !std::env::args().any(|arg| arg == "--source-layer") {
        project.document.active_layer = ink;
    }
    project.write(BufWriter::new(File::create(output)?))?;
    println!("Created {width} x {height} synthetic image, hidden paper, active layer {:?}", project.document.active_layer);
    Ok(())
}
