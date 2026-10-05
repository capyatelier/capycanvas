//! Shared photo-to-master policy. Original samples and metadata are retained;
//! source file locations remain the host's separate, read-only import reference.
use layer_core::{
    BlendSpace, Document, DocumentNames, PhotoMetadata, ProjectLimits, authored::{PortableId, SourceTarget},
    color::{DocumentColor, SampleDepth, RgbSpace, source::SourceImage},
};
use std::sync::Arc;

/// An explicit source assumption changes its interpretation, never its samples.
/// Validate the transform before publishing this choice into a live document.
pub fn assume_source_profile(
    mut source: SourceImage,
    profile: layer_core::color::ColorProfile,
) -> Result<SourceImage, String> {
    source.interpretation =
        crate::repair_source_interpretation(source.interpretation, RgbSpace::ProPhoto, profile)?;
    source.validate()?;
    Ok(source)
}

pub fn photo_project(
    source: SourceImage,
    metadata: PhotoMetadata,
    names: DocumentNames,
    depth: SampleDepth,
) -> Result<Document, String> {
    source.validate()?;
    if source.interpretation.depth.is_float() && !depth.is_float() { return Err("HDR placement requires an HDR document; export an SDR rendition for SDR placement".into()); }
    let space = crate::suggested_working_space(&source.interpretation.profile)?
        .unwrap_or(RgbSpace::ProPhoto);
    let mut document = Document::new(PortableId::random(), source.extent[0], source.extent[1], names);
    let composition = document.artwork.compositions.get_mut(document.artwork.root).unwrap();
    composition.resolution = source.resolution;
    composition.color = DocumentColor { space, depth };
    composition.blend = BlendSpace::Perceptual.for_depth(depth);
    document.artwork.metadata = Arc::new(metadata);
    let SourceTarget::Paint(paint) = document.working.target.unwrap() else { unreachable!() };
    document.artwork.paint.get_mut(paint).unwrap().base = Some(layer_core::authored::PaintBase::new(Arc::new(source).into()));
    let paper = document.scene().order()[1];
    document.artwork.occurrences.get_mut(paper).unwrap().visible = false;
    document.validate(ProjectLimits::default())?;
    crate::validate_document_color(&document)?;
    Ok(document)
}
