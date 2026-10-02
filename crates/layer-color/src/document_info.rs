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
    pub fn describe(&self) -> Result<Vec<(String, String)>, String> {
        let mut rows = vec![
            (
                "Canvas size".into(),
                format!("{} × {} pixels", self.extent[0], self.extent[1]),
            ),
            ("Working color space".into(), self.color.space.name().into()),
            (
                "Bit depth".into(),
                self.color.depth.label().into(),
            ),
            ("Blending".into(), self.blend_space.label().into()),
            (
                "Resolution metadata".into(),
                self.resolution.map_or_else(
                    || "Not specified".into(),
                    |r| {
                        let [x, y] = r.pixels_per_inch();
                        format!("{x:.2} × {y:.2} pixels per inch")
                    },
                ),
            ),
        ];
        if self.color.depth.is_float() { rows.push(("HDR reference white".into(), format!("203 cd/m² · linear RGB · finite magnitude ≤ {}", self.color.depth.max_linear()))); }
        for source in &self.sources {
            let i = &source.interpretation;
            let profile = crate::profile_description(&i.profile)?;
            let channels = match i.channels {
                layer_core::color::source::SourceChannels::Rgb
                | layer_core::color::source::SourceChannels::Rgba => "RGB",
                layer_core::color::source::SourceChannels::Gray
                | layer_core::color::source::SourceChannels::GrayAlpha => "Grayscale",
                layer_core::color::source::SourceChannels::Cmyk => "CMYK",
            };
            let tag = if i.profile_assumed {
                "Profile assumed"
            } else {
                "Source profile"
            };
            let retained = if source.kind == SourceKind::Rasterized {
                "Rasterized in document coordinates."
            } else if matches!(i.profile, ColorProfile::Icc(_)) {
                "Original samples and embedded ICC retained."
            } else {
                "Original samples and color interpretation retained."
            };
            rows.push((
                source.name.clone(),
                format!(
                    "{} × {} px · {}-bit {channels}\n{tag}: {profile}\n{retained}",
                    source.extent[0],
                    source.extent[1],
                    i.depth.bits()
                ),
            ));
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn properties_show_how_layers_blend() {
        let mut document = Document::new("Properties", 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let rows = |document: &Document| DocumentInfo::capture(document).describe().unwrap();
        assert!(rows(&document).contains(&("Blending".into(), "Linear light".into())));
        document.blend_space = BlendSpace::Perceptual;
        let described = rows(&document);
        let position = |label: &str| described.iter().position(|(l, _)| l == label).unwrap();
        assert_eq!(described[position("Blending")].1, "Perceptual");
        assert_eq!(position("Blending"), position("Bit depth") + 1);
    }
}
