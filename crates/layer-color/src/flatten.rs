//! A flattened conversion is a separate native document, never a replacement edit.
use layer_core::{
    BlendSpace, Document, ImageResolution, authored::{Artwork,PortableId,PaintSource,Occurrence,OccurrenceContent,SourceTarget},
    color::{
        ColorProfile, DocumentColor,
        source::{SourceBuilder, SourceChannels, SourceInterpretation},
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
) -> Result<Document, String> {
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

    source.resolution = resolution;
    let mut artwork = Artwork::new(extent)?;
    let composition = artwork.compositions.get_mut(artwork.root).unwrap();
    composition.color = color;
    composition.blend = BlendSpace::Perceptual.for_depth(color.depth);
    composition.resolution = resolution;
    let stack = composition.result;
    let paint = artwork.paint.insert(PortableId::random(), PaintSource { color_mode:Default::default(),domain:extent, raster:Default::default(), base:Some(layer_core::authored::PaintBase {image:Arc::new(source).into(),offset:[0;2],policy:layer_core::authored::PaintBasePolicy::WorkingPixels}), operations:Default::default()})?;
    let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(OccurrenceContent::Paint(paint), "Converted image"))?;
    artwork.stacks.get_mut(stack).unwrap().entries.push(occurrence);
    let mut document = Document::from_artwork(artwork).map_err(|error| error.to_string())?;
    document.working.occurrence = Some(occurrence);
    document.working.target = Some(SourceTarget::Paint(paint));
    document.validate(Default::default())?;
    crate::validate_document_color(&document)?;
    Ok(document)
}
