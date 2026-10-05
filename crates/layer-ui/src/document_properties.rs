use crate::{FluentArgs, Localizer, MessageId};
use layer_core::{BlendSpace, authored::PaintBasePolicy, color::{SampleDepth, source::SourceChannels}};
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct DocumentPropertiesView {
    pub title: Arc<str>,
    pub source_images: Arc<str>,
    pub done: Arc<str>,
    pub proof: Arc<str>,
    pub rows: Vec<(String, String)>,
    pub sources: Vec<(String, String)>,
}
pub fn document_properties(info: &layer_color::InspectedDocumentInfo, localizer: &Localizer) -> DocumentPropertiesView {
    let label = |id| localizer.text(id).to_string();
    let mut args = FluentArgs::new();
    args.set("width", info.extent[0]); args.set("height", info.extent[1]);
    let depth = match info.color.depth {
        SampleDepth::U8 => MessageId::COLOR_FEATURES_COLOR_DEPTH_8,
        SampleDepth::U16 => MessageId::COLOR_FEATURES_COLOR_DEPTH_16,
        SampleDepth::F16 => MessageId::COLOR_FEATURES_COLOR_DEPTH_FLOAT16,
        SampleDepth::F32 => MessageId::COLOR_FEATURES_COLOR_DEPTH_FLOAT32,
    };
    let blend = match info.blend_space { BlendSpace::Linear => MessageId::COMMAND_BLEND_LINEAR, BlendSpace::Perceptual => MessageId::COMMAND_BLEND_PERCEPTUAL };
    let resolution = info.resolution.map_or_else(|| label(MessageId::COLOR_PROPERTIES_UNSPECIFIED), |resolution| {
        let [x,y] = resolution.pixels_per_inch(); let mut args = FluentArgs::new();
        args.set("x", format!("{x:.2}")); args.set("y", format!("{y:.2}"));
        localizer.format(MessageId::COLOR_PROPERTIES_RESOLUTION_VALUE, &args)
    });
    let mut rows = vec![
        (label(MessageId::COLOR_PROPERTIES_CANVAS_SIZE), localizer.format(MessageId::COLOR_PROPERTIES_PIXELS, &args)),
        (label(MessageId::COLOR_PROPERTIES_WORKING_SPACE), info.color.space.name().into()),
        (label(MessageId::COLOR_PROPERTIES_BIT_DEPTH), label(depth)),
        (label(MessageId::COLOR_PROPERTIES_BLENDING), label(blend)),
        (label(MessageId::COLOR_PROPERTIES_RESOLUTION), resolution),
    ];
    if info.color.depth.is_float() {
        let mut args = FluentArgs::new(); args.set("maximum", info.color.depth.max_linear().to_string());
        rows.push((label(MessageId::COLOR_PROPERTIES_HDR_WHITE), localizer.format(MessageId::COLOR_PROPERTIES_HDR_VALUE, &args)));
    }
    let sources = info.sources.iter().map(|source| {
        let channels = match source.channels { SourceChannels::Rgb | SourceChannels::Rgba => "RGB".into(), SourceChannels::Cmyk => "CMYK".into(), SourceChannels::Gray | SourceChannels::GrayAlpha => label(MessageId::COLOR_FEATURES_PROFILE_GRAYSCALE) };
        let retained = if source.policy == PaintBasePolicy::WorkingPixels { MessageId::COLOR_PROPERTIES_RETAINED_RASTERIZED } else if source.embedded { MessageId::COLOR_PROPERTIES_RETAINED_ICC } else { MessageId::COLOR_PROPERTIES_RETAINED_ORIGINAL };
        let mut args = FluentArgs::new(); args.set("width", source.extent[0]); args.set("height", source.extent[1]);
        args.set("bits", source.bits); args.set("channels", channels); args.set("profile", crate::profile_library::profile_description_name(source.profile_description.clone(), localizer));
        args.set("assumed", if source.profile_assumed {"yes"} else {"no"}); args.set("retained", label(retained));
        (source.name.clone(), localizer.format(MessageId::COLOR_PROPERTIES_SOURCE, &args))
    }).collect();
    DocumentPropertiesView { title: localizer.text(MessageId::COLOR_PROPERTIES_TITLE), source_images: localizer.text(MessageId::COLOR_PROPERTIES_SOURCE_IMAGES), done: localizer.text(MessageId::COMMON_DONE), proof: localizer.text(MessageId::COLOR_FEATURES_PROOF_TITLE), rows, sources }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn properties_preserve_literals_and_all_retention_branches() {
        let context = Localizer::shared(crate::UiLanguage::English);
        let document = layer_core::Document::new(layer_core::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let mut info = layer_color::DocumentInfo::capture(&document).inspect().unwrap();
        for (policy, embedded, retained) in [(PaintBasePolicy::SourceProfile, true, "Original samples and embedded ICC retained."), (PaintBasePolicy::SourceProfile, false, "Original samples and color interpretation retained."), (PaintBasePolicy::WorkingPixels, true, "Rasterized in document coordinates.")] {
            info.sources = vec![layer_color::InspectedSourceInfo { name: "私の写真 · 내 사진".into(), extent: [10,20], policy, channels: SourceChannels::Gray, bits: 16, profile_description: Some("Embedded ICC profile".into()), profile_assumed: true, embedded }];
            let view = document_properties(&info, &context);
            assert_eq!(view.sources[0].0, "私の写真 · 내 사진");
            assert!(view.sources[0].1.contains("Profile assumed: Embedded ICC profile"));
            assert!(view.sources[0].1.contains(retained));
            info.sources[0].profile_description = None;
            info.sources[0].profile_assumed = false;
            assert!(document_properties(&info,&context).sources[0].1.contains("Source profile: Embedded ICC profile"));
        }
    }
    #[test]
    fn properties_show_blending_and_preserve_literal_source_names() {
        let context = Localizer::shared(crate::UiLanguage::English);
        let mut document = layer_core::Document::new(layer_core::PortableId::random(), 64, 48, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        let rows = |document: &layer_core::Document| document_properties(&layer_color::DocumentInfo::capture(document).inspect().unwrap(), &context).rows;
        assert!(rows(&document).contains(&("Blending".into(), "Linear Light Blending".into())));
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().blend = BlendSpace::Perceptual;
        let described = rows(&document);
        let position = |label: &str| described.iter().position(|(l,_)| l == label).unwrap();
        assert_eq!(described[position("Blending")].1, "Perceptual Blending");
        assert_eq!(position("Blending"), position("Bit depth") + 1);
    }
}
