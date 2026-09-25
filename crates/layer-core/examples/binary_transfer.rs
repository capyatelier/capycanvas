//! Generated 61 MP coverage plus opaque ICC bytes for transport qualification.
//! No renderer/CMM adoption: the deliberately synthetic ICC is a byte fixture.
use layer_core::*;
use std::{io::Write, sync::Arc};

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("Pass an output .capy path")?;
    let [width, height] = [9504, 6336];
    let mut document = Document::new("Generated binary transport fixture", width, height);
    let words: Vec<u32> = (0..height)
        .flat_map(|y| {
            (0..width / 4).map(move |x| {
                u32::from_le_bytes(std::array::from_fn(|c| {
                    ((x * 4 + c as u32) / 37 + y / 19) as u8
                }))
            })
        })
        .collect();
    let selected = Selection::pixels(Arc::new(
        SelectionPixels::bytes([width, height], [0, 0, width, height], words)
            .map_err(|e| e.to_string())?,
    ));
    document.selection = Some(selected.clone());
    let id = document.allocate_layer_id();
    document
        .layers
        .insert(0, Layer::selection(id, "Shared mask", selected.clone()));
    let id = document.allocate_layer_id();
    let mut mask = LayerMask::reveal_all(id, Point::default());
    mask.initial = Some(selected);
    document.layers[1].mask = Some(mask);
    document.proof = Some(color::ProofRecipe::new(
        "Opaque 2 MiB profile".into(),
        color::ColorProfile::Icc(
            (0..2 * 1024 * 1024)
                .map(|n| n as u8)
                .collect::<Vec<_>>()
                .into(),
        ),
    ));
    let mut source = color::source::SourceBuilder::new(
        [1, 1],
        color::source::SourceInterpretation {
            channels: color::source::SourceChannels::Rgba,
            depth: color::SampleDepth::U8,
            profile: document.proof.as_ref().unwrap().profile.clone(),
            profile_assumed: false,
        },
        1024 * 1024,
    )?;
    source.push_row(&[17, 33, 65, 255])?;
    document.layers[1].source = Some(Arc::new(source.finish()?));
    let project = Project {
        document,
        assets: Default::default(),
    };
    let mut output = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    project.write(&mut output)?;
    output.flush().map_err(|e| e.to_string())?;
    println!(
        "Generated {} bytes in {:?}",
        output.metadata().map_err(|e| e.to_string())?.len(),
        start.elapsed()
    );
    Ok(())
}
