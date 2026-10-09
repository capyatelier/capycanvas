//! Capture small document/source metadata on the owner; profile inspection runs
//! on the file worker. Raster sample payloads never enter this message.
use layer_core::{
    BlendSpace, Document, ImageResolution,
    color::{
        ColorProfile, DocumentColor,
        source::SourceInterpretation,
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
    policy: Option<layer_core::authored::PaintBasePolicy>,
    interpretation: SourceInterpretation,
}
impl DocumentInfo {
    pub fn capture(document: &Document) -> Self {
        let scene = document.scene();
        let composition = scene.composition();
        Self {
            extent: composition.size,
            color: composition.color,
            blend_space: composition.blend,
            resolution: composition.resolution,
            sources: scene.order().iter().flat_map(|handle| {
                let occurrence=scene.occurrence(*handle).unwrap();
                if let Some(base)=scene.paint_source(*handle).and_then(|paint|paint.base.as_ref()) {
                    return vec![SourceInfo {name:occurrence.name.to_string(),extent:base.image.extent,policy:Some(base.policy),interpretation:base.image.interpretation.clone()}];
                }
                scene.object_layer(*handle).into_iter().map(|object|SourceInfo {
                    name:occurrence.name.to_string(),extent:object.image.extent,policy:None,interpretation:object.image.interpretation.clone(),
                }).collect()
            }).collect(),
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
                policy: source.policy,
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
    /// The paint layer's use of its base, or none for a placed image.
    pub policy: Option<layer_core::authored::PaintBasePolicy>,
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
        let mut document = Document::new(layer_core::authored::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        assert_eq!(DocumentInfo::capture(&document).inspect().unwrap().blend_space, BlendSpace::Linear);
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend = BlendSpace::Perceptual;
        assert_eq!(DocumentInfo::capture(&document).inspect().unwrap().blend_space, BlendSpace::Perceptual);
    }
}
