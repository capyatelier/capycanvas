//! Shared photo-to-master policy. Original samples and metadata are retained;
//! source file locations remain the host's separate, read-only import reference.
use layer_core::{
    Document, Project, ProjectLimits,
    color::{DocumentColor, SampleDepth, RgbSpace, source::SourceImage},
};
use std::sync::Arc;

/// An explicit source assumption changes its interpretation, never its samples.
/// Validate the transform before publishing this choice into a live document.
pub fn assume_source_profile(
    mut source: SourceImage,
    profile: layer_core::color::ColorProfile,
) -> Result<SourceImage, String> {
    source.interpretation.profile = profile;
    source.interpretation.profile_assumed = false;
    crate::WorkingDecoder::new(
        &source.interpretation,
        RgbSpace::ProPhoto,
        Default::default(),
    )?;
    source.validate()?;
    Ok(source)
}

pub fn photo_project(
    source: SourceImage,
    name: &str,
    depth: SampleDepth,
) -> Result<Project, String> {
    source.validate()?;
    if source.interpretation.depth.is_float() && !depth.is_float() { return Err("HDR placement requires an HDR document; export an SDR rendition for SDR placement".into()); }
    let space = crate::suggested_working_space(&source.interpretation.profile)?
        .unwrap_or(RgbSpace::ProPhoto);
    let mut document = Document::new("untitled", source.extent[0], source.extent[1]);
    document.resolution = source.resolution;
    document.color = DocumentColor { space, depth };
    let name: String = name.chars().filter(|c| !c.is_control()).take(128).collect();
    document.layers[0].name = if name.is_empty() {
        "Photo".into()
    } else {
        name.into()
    };
    document.layers[0].source = Some(Arc::new(source));
    document.layers[1].visible = false;
    let project = Project {
        document,
        assets: Default::default(),
    };
    project.validate(ProjectLimits::default())?;
    Ok(project)
}
