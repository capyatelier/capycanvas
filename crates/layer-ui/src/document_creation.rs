//! Portable creation choices and the saved New drawing setups. Display state
//! is never a document color default; precision and working RGB remain
//! independent.
use crate::{DEFAULT_DOCUMENT_EXTENT, MAX_NEW_DOCUMENT_DIMENSION, Localizer, MessageId, FluentArgs};
use std::sync::Arc;
use layer_core::{
    BlendSpace, Document,
    color::{DocumentColor, SampleDepth},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewDocumentError {
    CanvasSize,
    Resolution,
    CanvasUnavailable,
    PresetUnavailable,
    PresetLimit,
    PresetName,
    DuplicatePresetName,
}
impl NewDocumentError {
    pub fn message(self, localization: &Localizer) -> String {
        let id = match self {
            Self::CanvasSize => {
                let mut args = FluentArgs::new();
                args.set("maximum", MAX_NEW_DOCUMENT_DIMENSION);
                return localization.format(MessageId::DOCUMENTS_ERROR_CANVAS_SIZE, &args);
            }
            Self::Resolution => {
                let mut args = FluentArgs::new();
                args.set("maximum", MAX_NEW_DOCUMENT_RESOLUTION);
                return localization.format(MessageId::DOCUMENTS_ERROR_RESOLUTION, &args);
            }
            Self::CanvasUnavailable => MessageId::DOCUMENTS_ERROR_CANVAS_UNAVAILABLE,
            Self::PresetUnavailable => MessageId::DOCUMENTS_ERROR_PRESET_UNAVAILABLE,
            Self::PresetLimit => MessageId::DOCUMENTS_ERROR_PRESET_LIMIT,
            Self::PresetName => MessageId::DOCUMENTS_ERROR_PRESET_NAME,
            Self::DuplicatePresetName => MessageId::DOCUMENTS_ERROR_PRESET_DUPLICATE,
        };
        localization.text(id).to_string()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentBackground {
    #[default]
    White,
    Transparent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentOptions {
    pub extent: [u32; 2],
    pub color: DocumentColor,
    pub background: DocumentBackground,
    /// Settings from before this field read with the new-document default.
    #[serde(default = "perceptual")]
    pub blend_space: BlendSpace,
    #[serde(default = "print_resolution")]
    pub resolution: u32,
}
fn perceptual() -> BlendSpace {
    BlendSpace::Perceptual
}
fn print_resolution() -> u32 {
    layer_core::paper::DEFAULT_PAPER_PPI as u32
}
pub const MAX_NEW_DOCUMENT_RESOLUTION: u32 = 10_000;
impl Default for NewDocumentOptions {
    fn default() -> Self {
        Self {
            extent: DEFAULT_DOCUMENT_EXTENT,
            color: DocumentColor::default(),
            background: DocumentBackground::White,
            blend_space: BlendSpace::Perceptual,
            resolution: print_resolution(),
        }
    }
}
impl NewDocumentOptions {
    pub fn validate(self) -> Result<(), NewDocumentError> {
        if self
            .extent
            .iter()
            .any(|v| *v == 0 || *v > MAX_NEW_DOCUMENT_DIMENSION)
        {
            return Err(NewDocumentError::CanvasSize);
        }
        if !(1..=MAX_NEW_DOCUMENT_RESOLUTION).contains(&self.resolution) {
            return Err(NewDocumentError::Resolution);
        }
        Ok(())
    }
    pub fn project(self, localization: &Localizer) -> Result<Document, String> {
        self.project_on(layer_core::paper::default_texture(), localization)
    }
    pub fn project_on(self, paper: layer_core::TextureSource, localization: &Localizer) -> Result<Document, String> {
        self.validate().map_err(|error| error.message(localization))?;
        Ok(self.document_on(layer_core::authored::PortableId::random(), paper, localization))
    }
    pub(crate) fn document_on(self, id: layer_core::authored::PortableId, paper: layer_core::TextureSource, localization: &Localizer) -> Document {
        let mut document = Document::new(id, self.extent[0], self.extent[1], layer_core::DocumentNames {
            paint: localization.text(MessageId::DOCUMENTS_CURRENT_INK),
            paper: localization.text(MessageId::DOCUMENTS_PAPER),
        });
        let composition = document.artwork.compositions.get_mut(document.artwork.root).unwrap();
        composition.color = self.color;
        composition.blend = self.blend_space.for_depth(self.color.depth);
        composition.resolution = Some(layer_core::ImageResolution::ppi(self.resolution));
        document.fit_papers_to_resolution();
        document.set_paper_texture(paper);
        let paper = layer_core::paper::paper_occurrence(document.scene(), None).expect("new drawings have paper");
        document.artwork.occurrences.get_mut(paper).unwrap().visible = self.background == DocumentBackground::White;
        document
    }
}

pub(crate) fn depth_label(depth: SampleDepth, localization: &Localizer) -> Arc<str> {
    localization.text(match depth {
        SampleDepth::U8 => MessageId::DOCUMENTS_DEPTH_U8,
        SampleDepth::U16 => MessageId::DOCUMENTS_DEPTH_U16,
        SampleDepth::F16 => MessageId::DOCUMENTS_DEPTH_F16,
        SampleDepth::F32 => MessageId::DOCUMENTS_DEPTH_F32,
    })
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NewDocumentSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub setups: Vec<crate::SavedSetup>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<crate::PresetKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<crate::DrawingSetup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_colors: Option<Vec<layer_core::color::RgbColor>>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewDocumentAction {
    SaveSetup { name: String, group: crate::PresetGroup, setup: crate::DrawingSetup },
    RenameSetup { from: String, to: String, group: crate::PresetGroup },
    DeleteSetup { name: String },
    Created { setup: crate::DrawingSetup, preset: Option<crate::PresetKey> },
    PaperColors { colors: Vec<layer_core::color::RgbColor> },
}
impl NewDocumentSettings {
    pub fn apply(&mut self, action: NewDocumentAction) -> Result<(), NewDocumentError> {
        let mut next = self.clone();
        match action {
            NewDocumentAction::SaveSetup { name, group, setup } => {
                let name = name.trim().to_string();
                next.setups.retain(|saved| saved.name.to_lowercase() != name.to_lowercase());
                next.recent.retain(|key| !matches!(key, crate::PresetKey::Saved(saved) if saved.to_lowercase() == name.to_lowercase()));
                next.setups.push(crate::SavedSetup { name, group, setup });
            }
            NewDocumentAction::RenameSetup { from, to, group } => {
                let to = to.trim().to_string();
                let saved = next.setups.iter_mut().find(|saved| saved.name == from).ok_or(NewDocumentError::PresetUnavailable)?;
                saved.name.clone_from(&to);
                saved.group = group;
                for key in &mut next.recent {
                    if *key == crate::PresetKey::Saved(from.clone()) { *key = crate::PresetKey::Saved(to.clone()); }
                }
            }
            NewDocumentAction::DeleteSetup { name } => {
                if !next.setups.iter().any(|saved| saved.name == name) { return Err(NewDocumentError::PresetUnavailable); }
                next.setups.retain(|saved| saved.name != name);
                next.recent.retain(|key| *key != crate::PresetKey::Saved(name.clone()));
            }
            NewDocumentAction::Created { setup, preset } => {
                let key = preset.unwrap_or_else(|| {
                    next.last = Some(setup);
                    crate::PresetKey::Last
                });
                next.recent.retain(|recent| *recent != key);
                next.recent.insert(0, key);
                next.recent.truncate(crate::MAX_RECENT);
            }
            NewDocumentAction::PaperColors { colors } => {
                if colors.len() > crate::MAX_PAPER_COLORS || colors.iter().any(|color| color.validate().is_err()) { return Err(NewDocumentError::PresetLimit); }
                next.paper_colors = Some(colors);
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), NewDocumentError> {
        if self.setups.len() > 64 {
            return Err(NewDocumentError::PresetLimit);
        }
        let mut names = Vec::new();
        for saved in &self.setups {
            let name = saved.name.trim();
            if name != saved.name || name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
                return Err(NewDocumentError::PresetName);
            }
            if names.contains(&name.to_lowercase()) {
                return Err(NewDocumentError::DuplicatePresetName);
            }
            names.push(name.to_lowercase());
        }
        Ok(())
    }
    pub fn paper(&self) -> layer_core::TextureSource {
        self.paper.as_deref().and_then(layer_core::paper::built_in_paper)
            .map_or_else(layer_core::paper::default_texture, |paper| paper.texture.clone())
    }
}

/// The Blending field: its choices, the depths that blend only in linear
/// light, and why.
#[derive(Serialize)]
pub struct NewDocumentBlending {
    pub label: Arc<str>,
    pub choices: Vec<BlendingChoice>,
    pub linear_only: Vec<SampleDepth>,
    pub float_reason: Arc<str>,
}
#[derive(Serialize)]
pub struct BlendingChoice {
    pub id: BlendSpace,
    pub label: Arc<str>,
    pub description: Arc<str>,
}
fn blending_choice(id: BlendSpace, localization: &Localizer) -> BlendingChoice {
    let (label, description) = match id {
        BlendSpace::Perceptual => (MessageId::DOCUMENTS_BLENDING_PERCEPTUAL, MessageId::DOCUMENTS_BLENDING_PERCEPTUAL_DESCRIPTION),
        BlendSpace::Linear => (MessageId::DOCUMENTS_BLENDING_LINEAR, MessageId::DOCUMENTS_BLENDING_LINEAR_DESCRIPTION),
    };
    BlendingChoice { id, label: localization.text(label), description: localization.text(description) }
}
impl NewDocumentBlending {
    pub fn new(localization: &Localizer) -> Self {
        Self {
            label: localization.text(MessageId::DOCUMENTS_BLENDING),
            choices: BlendSpace::ALL
                .into_iter()
                .map(|id| blending_choice(id, localization))
                .collect(),
            linear_only: [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32]
                .into_iter()
                .filter(|depth| BlendSpace::unavailable_reason(*depth).is_some())
                .collect(),
            float_reason: localization.text(MessageId::DOCUMENTS_BLENDING_FLOAT_REASON),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::color::RgbSpace;
    use crate::session::test_support::reopen_capture as reopen;
    fn english() -> Arc<Localizer> { Localizer::shared(crate::UiLanguage::English) }
    #[test]
    fn saved_creation_names_keep_their_language_and_literal_source_names() {
        use crate::{UiSession, UiLanguage, Platform, LayerAction, UiAction, SelectionAction, CommandId};
        use crate::session::test_support::{Recorder, layer, invoke};
        use crate::session::occurrence_token;
        use layer_core::authored::{OccurrenceHandle, OccurrenceContent};
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut session = UiSession::blank_localized(Recorder { tiled_sources: true, ..Default::default() }, [256, 256], Platform::Gtk, japanese).unwrap();
        assert_eq!(session.engine().document().scene().occurrence(OccurrenceHandle::from_index(0)).unwrap().name.as_ref(), "現在のインク");
        assert_eq!(session.engine().document().scene().occurrence(OccurrenceHandle::from_index(1)).unwrap().name.as_ref(), "用紙");
        layer(&mut session, LayerAction::New { group: false, clipped: false });
        let paint = session.engine().document().working.occurrence.unwrap();
        assert_eq!(session.engine().document().scene().occurrence(paint).unwrap().name.as_ref(), "レイヤー 3");
        let literal = "  Layer 3 { $name }「漢字」🖌️\u{2066}literal\u{2069}  ";
        layer(&mut session, LayerAction::Rename { id: occurrence_token(paint), name: literal.into() });
        let original = session.engine().document().scene().occurrence(paint).unwrap().clone();
        layer(&mut session, LayerAction::Duplicate { id: occurrence_token(paint) });
        let copy = session.engine().document().working.occurrence.unwrap();
        assert_eq!(session.engine().document().scene().occurrence(copy).unwrap().name.as_ref(), format!("{literal}のコピー"));
        assert_eq!(session.engine().document().scene().occurrence(paint).unwrap(), &original);
        layer(&mut session, LayerAction::GroupSelected);
        let group_handle = session.engine().document().working.occurrence.unwrap();
        let group = session.engine().document().scene().occurrence(group_handle).unwrap().clone();
        assert_eq!(group.name.as_ref(), "グループ 5");
        invoke(&mut session, CommandId::Undo);
        assert!(session.engine().document().scene().occurrence(group_handle).is_none());
        assert_eq!(session.engine().document().scene().parent(copy), None);
        invoke(&mut session, CommandId::Redo);
        assert_eq!(session.engine().document().scene().occurrence(group_handle).unwrap(), &group);
        session.dispatch(UiAction::Selection { action: SelectionAction::NewLayer { parent: Some(occurrence_token(group_handle)), save_current: false } }).unwrap();
        let selection_handle = session.engine().document().scene().order().iter().copied().find(|handle| matches!(session.engine().document().scene().occurrence(*handle).unwrap().content, OccurrenceContent::Selection(_))).unwrap();
        let selection = session.engine().document().scene().occurrence(selection_handle).unwrap();
        assert_eq!(selection.name.as_ref(), "選択範囲 6");
        layer(&mut session, LayerAction::New { group: true, clipped: false });
        assert_eq!(session.engine().document().scene().occurrence(session.engine().document().working.occurrence.unwrap()).unwrap().name.as_ref(), "グループ 7");
        let imported_name = "  Current ink { $name }「写真」🖼️\u{2068}literal\u{2069}  ";
        let source = layer_core::color::source::rgba8_source([2, 2], |_, _| [200; 4]);
        session.import_layer_source(imported_name, Arc::unwrap_or_clone(source)).unwrap();
        let imported = session.engine().document().working.occurrence.unwrap();
        assert_eq!(session.engine().document().scene().occurrence(imported).unwrap().name.as_ref(), imported_name);
        let paint_id = session.engine().document().artwork.occurrences.id(paint).unwrap();
        let imported_id = session.engine().document().artwork.occurrences.id(imported).unwrap();
        let captured = session.capture_artwork().unwrap();
        let read = reopen(&captured);
        let paint = read.artwork.occurrences.resolve(paint_id).unwrap();
        let imported = read.artwork.occurrences.resolve(imported_id).unwrap();
        let reopened = UiSession::from_project_localized(Recorder { tiled_sources: true, ..Default::default() }, read, None, [256, 256], Platform::Gtk, english()).unwrap();
        assert_eq!(reopened.engine().document().artwork.id, captured.artwork.id);
        assert_eq!(reopened.engine().document().scene().occurrence(paint).unwrap().name.as_ref(), literal);
        assert_eq!(reopened.engine().document().scene().occurrence(imported).unwrap().name.as_ref(), imported_name);
    }

    #[test]
    fn new_documents_blend_perceptually_except_at_float() {
        let project = NewDocumentOptions::default().project(&english()).unwrap();
        assert_eq!(project.composition().blend, BlendSpace::Perceptual);
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            for blend_space in BlendSpace::ALL {
                let options = NewDocumentOptions { color: DocumentColor { depth, ..Default::default() }, blend_space, ..Default::default() };
                assert_eq!(options.project(&english()).unwrap().composition().blend, if depth.is_float() { BlendSpace::Linear } else { blend_space });
            }
        }
        for resolution in [1, MAX_NEW_DOCUMENT_RESOLUTION] {
            assert_eq!(NewDocumentOptions { resolution, ..Default::default() }.validate(), Ok(()));
        }
        assert_eq!(NewDocumentOptions { resolution: MAX_NEW_DOCUMENT_RESOLUTION + 1, ..Default::default() }.validate(), Err(NewDocumentError::Resolution));
    }
    #[test]
    fn new_drawings_start_on_the_remembered_paper_at_real_size() {
        let mut settings = NewDocumentSettings::default();
        let options = NewDocumentOptions { extent: [600, 400], resolution: 150, ..Default::default() };
        let surface = |settings: &NewDocumentSettings| options.project_on(settings.paper(), &english()).unwrap().paper(None);
        let drawing = surface(&settings);
        assert_eq!(drawing.texture, layer_core::paper::default_texture());
        assert!((drawing.mapping.0[0] - 150. / 25.4).abs() < 1e-12);
        settings.paper = Some("rough".into());
        assert_eq!(surface(&settings).texture, layer_core::paper::built_in_paper("rough").unwrap().texture);
        settings.paper = Some("deleted".into());
        assert_eq!(surface(&settings).texture, layer_core::paper::default_texture());
        assert_eq!(NewDocumentOptions { resolution: 0, ..options }.validate(), Err(NewDocumentError::Resolution));
        let older: NewDocumentOptions = serde_json::from_value({
            let mut value = serde_json::to_value(options).unwrap();
            value.as_object_mut().unwrap().remove("resolution");
            value
        }).unwrap();
        assert_eq!(older.resolution, 300);
    }
    #[test]
    fn creation_preserves_independent_depth_space_and_background() {
        for space in RgbSpace::ALL {
            for depth in [SampleDepth::U8, SampleDepth::U16] {
                for background in [DocumentBackground::White, DocumentBackground::Transparent] {
                    let options = NewDocumentOptions {
                        extent: [513, 257],
                        color: DocumentColor { space, depth },
                        background,
                        blend_space: BlendSpace::Perceptual,
                        resolution: 600,
                    };
                    let project = options.project(&english()).unwrap();
                    assert_eq!(project.composition().color, options.color);
                    assert_eq!(project.composition().resolution, Some(layer_core::ImageResolution::ppi(600)));
                    assert_eq!(
                        project.scene().order().iter().find_map(|handle| project.scene().effect(*handle)?.constant_color().map(|_| project.scene().occurrence(*handle).unwrap().visible)).unwrap(),
                        background == DocumentBackground::White
                    );
                    let captured = layer_core::Editor::new(project.clone()).capture(0, project.output().context.clone()).unwrap();
                    let restored = reopen(&captured);
                    assert_eq!(restored.artwork.id, project.artwork.id);
                    assert_eq!(restored.composition(), project.composition());
                    assert_eq!(restored.output(), project.output());
                }
            }
        }
    }
}
