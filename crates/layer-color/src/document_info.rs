//! Capture small document/source metadata on the owner; profile inspection runs
//! on the file worker. Raster sample payloads never enter this message.
use layer_core::{
    BlendSpace, Document, ImageResolution,
    color::{
        ColorProfile, DocumentColor,
        source::{SourceInterpretation, SourceKind},
    },
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct DocumentInfo {
    extent: [u32; 2],
    color: DocumentColor,
    #[serde(default)]
    blend_space: BlendSpace,
    resolution: Option<ImageResolution>,
    sources: Vec<SourceInfo>,
}
#[derive(Serialize, Deserialize)]
struct SourceInfo {
    name: String,
    extent: [u32; 2],
    kind: SourceKind,
    interpretation: SourceInterpretation,
}
impl DocumentInfo {
    pub fn capture(document: &Document) -> Self {
        Self {
            extent: [document.width, document.height],
            color: document.color,
            blend_space: document.blend_space,
            resolution: document.resolution,
            sources: document
                .layers
                .iter()
                .filter_map(|l| {
                    l.source.as_ref().map(|s| SourceInfo {
                        name: l.name.to_string(),
                        extent: s.extent,
                        kind: s.kind,
                        interpretation: s.interpretation.clone(),
                    })
                })
                .collect(),
        }
    }
    pub fn inspect(&self) -> Result<InspectedDocumentInfo, String> {
        Ok(InspectedDocumentInfo {
            extent: self.extent,
            color: self.color,
            blend_space: self.blend_space,
            resolution: self.resolution,
            sources: self.sources.iter().map(|source| Ok(InspectedSourceInfo {
                name: source.name.clone(),
                extent: source.extent,
                kind: source.kind,
                channels: source.interpretation.channels,
                bits: source.interpretation.depth.bits(),
                profile_description: crate::profile_description_optional(&source.interpretation.profile)?,
                profile_assumed: source.interpretation.profile_assumed,
                embedded: matches!(source.interpretation.profile, ColorProfile::Icc(_)),
            })).collect::<Result<Vec<_>, String>>()?,
        })
    }

}

#[derive(Serialize, Deserialize)]
pub struct InspectedDocumentInfo {
    pub extent: [u32; 2],
    pub color: DocumentColor,
    pub blend_space: BlendSpace,
    pub resolution: Option<ImageResolution>,
    pub sources: Vec<InspectedSourceInfo>,
}
#[derive(Serialize, Deserialize)]
pub struct InspectedSourceInfo {
    pub name: String,
    pub extent: [u32; 2],
    pub kind: SourceKind,
    pub channels: layer_core::color::source::SourceChannels,
    pub bits: u8,
    pub profile_description: Option<String>,
    pub profile_assumed: bool,
    pub embedded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn properties_show_how_layers_blend() {
        let mut document = Document::new("Properties", 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        assert_eq!(DocumentInfo::capture(&document).inspect().unwrap().blend_space, BlendSpace::Linear);
        document.blend_space = BlendSpace::Perceptual;
        assert_eq!(DocumentInfo::capture(&document).inspect().unwrap().blend_space, BlendSpace::Perceptual);
    }
}
