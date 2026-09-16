//! A flattened conversion is a separate native document, never a replacement edit.
use layer_core::{
    Document, ImageResolution, Project,
    color::{
        ColorProfile, DocumentColor,
        source::{SourceBuilder, SourceChannels, SourceInterpretation, SourceKind},
    },
};
use std::sync::Arc;

/// Consume the complete encoded composition in row order. The renderer/output
/// worker owns native composition, conversion, cancellation and row production.
pub fn flattened_document(
    extent: [u32; 2],
    color: DocumentColor,
    resolution: Option<ImageResolution>,
    actual: &SourceInterpretation,
    limit: usize,
    mut read: impl FnMut(u32, &mut [u8]) -> Result<(), String>,
) -> Result<Project, String> {
    if actual.channels != SourceChannels::Rgba
        || actual.depth != color.depth
        || actual.profile != ColorProfile::Builtin(color.space)
    {
        return Err("A flattened copy must use the selected document color mode".into());
    }
    let mut builder = SourceBuilder::new(extent, actual.clone(), limit)?;
    let mut row = vec![0; extent[0] as usize * actual.pixel_bytes()];
    for y in 0..extent[1] {
        read(y, &mut row)?;
        builder.push_row(&row)?;
    }
    let mut source = builder.finish()?;
    source.kind = SourceKind::Rasterized;
    source.resolution = resolution;
    let mut document = Document::new("Converted copy", extent[0], extent[1]);
    document.color = color;
    document.resolution = resolution;
    document.layers.truncate(1);
    document.layers[0].name = "Converted image".into();
    document.layers[0].source = Some(Arc::new(source));
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(Default::default())?;
    Ok(project)
}
