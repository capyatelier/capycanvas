//! Portable creation choices and reusable presets. Display state is never a
//! document color default; precision and working RGB remain independent.
use crate::{DEFAULT_DOCUMENT_EXTENT, MAX_NEW_DOCUMENT_DIMENSION, Localizer, MessageId, FluentArgs};
use std::sync::Arc;
use layer_core::{
    BlendSpace, Document, Project,
    color::{DocumentColor, SampleDepth, RgbSpace},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewDocumentError {
    CanvasSize,
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
}
fn perceptual() -> BlendSpace {
    BlendSpace::Perceptual
}
impl Default for NewDocumentOptions {
    fn default() -> Self {
        Self {
            extent: DEFAULT_DOCUMENT_EXTENT,
            color: DocumentColor::default(),
            background: DocumentBackground::White,
            blend_space: BlendSpace::Perceptual,
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
        Ok(())
    }
    pub fn project(self, localization: &Localizer) -> Result<Project, String> {
        self.validate().map_err(|error| error.message(localization))?;
        let mut document = Document::new("untitled", self.extent[0], self.extent[1], layer_core::DocumentNames {
            paint: localization.text(MessageId::DOCUMENTS_CURRENT_INK),
            paper: localization.text(MessageId::DOCUMENTS_PAPER),
        });
        document.color = self.color;
        document.blend_space = self.blend_space.for_depth(self.color.depth);
        document.layers[1].visible = self.background == DocumentBackground::White;
        Ok(Project { document })
    }
    pub fn description(self, localization: &Localizer) -> String {
        let mut args = FluentArgs::new();
        args.set("width", self.extent[0]);
        args.set("height", self.extent[1]);
        args.set("space", self.color.space.name());
        let depth = depth_label(self.color.depth, localization);
        args.set("depth", depth.as_ref());
        let background = localization.text(if self.background == DocumentBackground::White {
            MessageId::DOCUMENTS_WHITE
        } else { MessageId::DOCUMENTS_TRANSPARENT });
        args.set("background", background.as_ref());
        localization.format(MessageId::DOCUMENTS_OPTIONS_DESCRIPTION, &args)
    }
}

fn depth_label(depth: SampleDepth, localization: &Localizer) -> Arc<str> {
    localization.text(match depth {
        SampleDepth::U8 => MessageId::DOCUMENTS_DEPTH_U8,
        SampleDepth::U16 => MessageId::DOCUMENTS_DEPTH_U16,
        SampleDepth::F16 => MessageId::DOCUMENTS_DEPTH_F16,
        SampleDepth::F32 => MessageId::DOCUMENTS_DEPTH_F32,
    })
}
#[derive(Serialize)]
pub struct NewDocumentAppearance {
    pub summary: String,
    pub blending: BlendSpace,
    pub blending_editable: bool,
    pub blending_help: Arc<str>,
    pub note: Option<Arc<str>>,
}
impl NewDocumentOptions {
    pub fn appearance(self, localization: &Localizer) -> NewDocumentAppearance {
        let blending = self.blend_space.for_depth(self.color.depth);
        let choice = blending_choice(blending, localization);
        let depth = depth_label(self.color.depth, localization);
        let mut args = FluentArgs::new();
        args.set("space", self.color.space.name());
        args.set("depth", depth.as_ref());
        args.set("blending", choice.label.as_ref());
        NewDocumentAppearance {
            summary: localization.format(MessageId::DOCUMENTS_COLOR_SUMMARY, &args),
            blending,
            blending_editable: !self.color.depth.is_float(),
            blending_help: if self.color.depth.is_float() { localization.text(MessageId::DOCUMENTS_BLENDING_FLOAT_REASON) } else { choice.description },
            note: (self.color.space == RgbSpace::ProPhoto && self.color.depth == SampleDepth::U8)
                .then(|| localization.text(MessageId::DOCUMENTS_PROPHOTO_NOTE)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentPreset {
    pub name: String,
    pub options: NewDocumentOptions,
}
impl NewDocumentPreset {
    pub fn builtins(localization: &Localizer) -> [NewDocumentPresetView; 4] {
        [
            (NewDocumentPresetId::Standard, MessageId::DOCUMENTS_STANDARD_PRESET, RgbSpace::Srgb, SampleDepth::U8),
            (NewDocumentPresetId::WideColor, MessageId::DOCUMENTS_WIDE_PRESET, RgbSpace::DisplayP3, SampleDepth::U8),
            (NewDocumentPresetId::PhotoEditing, MessageId::DOCUMENTS_PHOTO_PRESET, RgbSpace::ProPhoto, SampleDepth::U16),
            (NewDocumentPresetId::HdrDrawing, MessageId::DOCUMENTS_HDR_PRESET, RgbSpace::Srgb, SampleDepth::F16),
        ]
        .map(|(id, name, space, depth)| NewDocumentPresetView {
            id,
            remove: None,
            name: localization.text(name).to_string(),
            options: NewDocumentOptions {
                color: DocumentColor { space, depth },
                blend_space: BlendSpace::Perceptual.for_depth(depth),
                ..Default::default()
            },
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDocumentSettings {
    pub defaults: NewDocumentOptions,
    pub presets: Vec<NewDocumentPreset>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewDocumentAction {
    Remember { options: NewDocumentOptions, name: String, defaults: bool },
    Remove { index: usize },
}
impl NewDocumentSettings {
    pub fn apply(&mut self, action: NewDocumentAction) -> Result<(), NewDocumentError> {
        let mut next = self.clone();
        match action {
            NewDocumentAction::Remember { options, name, defaults } => {
                options.validate()?;
                if !name.trim().is_empty() {
                    next.presets.push(NewDocumentPreset { name: name.trim().into(), options });
                }
                if defaults { next.defaults = options; }
            }
            NewDocumentAction::Remove { index } => {
                if index >= next.presets.len() { return Err(NewDocumentError::PresetUnavailable); }
                next.presets.remove(index);
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), NewDocumentError> {
        self.defaults.validate()?;
        if self.presets.len() > 64 {
            return Err(NewDocumentError::PresetLimit);
        }
        let mut names = Vec::new();
        for preset in &self.presets {
            preset.options.validate()?;
            let name = preset.name.trim();
            if name != preset.name
                || name.is_empty()
                || name.chars().count() > 64
                || name.chars().any(char::is_control)
            {
                return Err(NewDocumentError::PresetName);
            }
            if names.contains(&name.to_lowercase()) {
                return Err(NewDocumentError::DuplicatePresetName);
            }
            names.push(name.to_lowercase());
        }
        Ok(())
    }
    pub fn save(&mut self, name: &str, options: NewDocumentOptions) -> Result<(), NewDocumentError> {
        let mut next = self.clone();
        next.presets.push(NewDocumentPreset {
            name: name.trim().into(),
            options,
        });
        next.validate()?;
        *self = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NewDocumentPresetId { Standard, WideColor, PhotoEditing, HdrDrawing, Saved { index: usize } }

#[derive(Clone, Debug, Serialize)]
pub struct NewDocumentPresetView {
    pub id: NewDocumentPresetId,
    pub remove: Option<NewDocumentAction>,
    pub name: String,
    pub options: NewDocumentOptions,
}

/// Native and browser creation sheets use the same independent options.
#[derive(Serialize)]
pub struct NewDocumentForm {
    pub options: NewDocumentOptions,
    pub text: NewDocumentText,
    pub selected: Option<NewDocumentPresetId>,
    pub backgrounds: Vec<(DocumentBackground, Arc<str>)>,
    pub depths: Vec<(SampleDepth, Arc<str>)>,
    pub presets: Vec<NewDocumentPresetView>,
    pub spaces: Vec<(RgbSpace, &'static str)>,
    pub blending: NewDocumentBlending,
}
#[derive(Serialize)]
pub struct NewDocumentText {
    pub new_title: Arc<str>,
    pub defaults_title: Arc<str>,
    pub width: Arc<str>,
    pub height: Arc<str>,
    pub preset: Arc<str>,
    pub custom: Arc<str>,
    pub background: Arc<str>,
    pub space: Arc<str>,
    pub depth: Arc<str>,
    pub color: Arc<str>,
    pub save_preset: Arc<str>,
    pub remove_preset: Arc<str>,
    pub remember: Arc<str>,
    pub use_defaults: Arc<str>,
    pub create: Arc<str>,
    pub cancel: Arc<str>,
    pub preset_name: Arc<str>,
    pub save_preset_title: Arc<str>,
    pub save: Arc<str>,
}
impl NewDocumentText {
    fn new(localization: &Localizer) -> Self {
        Self {
            new_title: localization.text(MessageId::DOCUMENTS_NEW),
            defaults_title: localization.text(MessageId::DOCUMENTS_DEFAULTS_TITLE),
            width: localization.text(MessageId::DOCUMENTS_WIDTH),
            height: localization.text(MessageId::DOCUMENTS_HEIGHT),
            preset: localization.text(MessageId::DOCUMENTS_PRESET_LABEL),
            custom: localization.text(MessageId::DOCUMENTS_CUSTOM_PRESET),
            background: localization.text(MessageId::DOCUMENTS_BACKGROUND_LABEL),
            space: localization.text(MessageId::DOCUMENTS_SPACE_LABEL),
            depth: localization.text(MessageId::DOCUMENTS_DEPTH_LABEL),
            color: localization.text(MessageId::DOCUMENTS_COLOR_LABEL),
            save_preset: localization.text(MessageId::DOCUMENTS_SAVE_PRESET),
            remove_preset: localization.text(MessageId::DOCUMENTS_REMOVE_PRESET),
            remember: localization.text(MessageId::DOCUMENTS_REMEMBER),
            use_defaults: localization.text(MessageId::DOCUMENTS_USE_DEFAULTS),
            create: localization.text(MessageId::DOCUMENTS_CREATE),
            cancel: localization.text(MessageId::COMMON_CANCEL),
            preset_name: localization.text(MessageId::DOCUMENTS_PRESET_NAME),
            save_preset_title: localization.text(MessageId::DOCUMENTS_SAVE_PRESET_TITLE),
            save: localization.text(MessageId::COMMON_SAVE),
        }
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
impl NewDocumentSettings {
    pub fn form(&self, localization: &Localizer) -> NewDocumentForm {
        let presets: Vec<_> = NewDocumentPreset::builtins(localization).into_iter().chain(self.presets.iter().enumerate().map(|(index, preset)| NewDocumentPresetView {
            id: NewDocumentPresetId::Saved { index }, remove: Some(NewDocumentAction::Remove { index }),
            name: preset.name.clone(), options: preset.options,
        })).collect();
        let selected = presets.iter().find(|preset| preset.options == self.defaults).map(|preset| preset.id);
        NewDocumentForm {
            options: self.defaults,
            text: NewDocumentText::new(localization), selected,
            backgrounds: vec![(DocumentBackground::White, localization.text(MessageId::DOCUMENTS_WHITE)),
                (DocumentBackground::Transparent, localization.text(MessageId::DOCUMENTS_TRANSPARENT))],
            depths: [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32]
                .into_iter().map(|depth| (depth, depth_label(depth, localization))).collect(),
            presets,
            spaces: RgbSpace::ALL.into_iter().map(|space| (space, space.name())).collect(),
            blending: NewDocumentBlending::new(localization),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn english() -> Arc<Localizer> { Localizer::shared(crate::UiLanguage::English) }
    #[test]
    fn saved_creation_names_keep_their_language_and_literal_source_names() {
        use crate::{UiSession, UiLanguage, Platform, LayerAction, UiAction, SelectionAction, CommandId};
        use crate::session::test_support::{Recorder, layer, invoke};
        use layer_core::LayerId;
        let japanese = Localizer::shared(UiLanguage::Japanese);
        let mut session = UiSession::blank_localized(Recorder { tiled_sources: true, ..Default::default() }, [256, 256], Platform::Gtk, japanese).unwrap();
        assert_eq!(session.engine().document().layers[0].name.as_ref(), "現在のインク");
        assert_eq!(session.engine().document().layers[1].name.as_ref(), "用紙");
        layer(&mut session, LayerAction::New { group: false, clipped: false });
        let paint = session.engine().document().active_layer;
        assert_eq!(session.engine().document().layer(paint).unwrap().name.as_ref(), "レイヤー 3");
        let literal = "  Layer 3 { $name }「漢字」🖌️\u{2066}literal\u{2069}  ";
        layer(&mut session, LayerAction::Rename { id: paint.0, name: literal.into() });
        let original = session.engine().document().layer(paint).unwrap().clone();
        layer(&mut session, LayerAction::Duplicate { id: paint.0 });
        let copy = session.engine().document().active_layer;
        assert_eq!(session.engine().document().layer(copy).unwrap().name.as_ref(), format!("{literal}のコピー"));
        assert_eq!(session.engine().document().layer(paint).unwrap(), &original);
        layer(&mut session, LayerAction::GroupSelected);
        let group = session.engine().document().layer(LayerId(5)).unwrap().clone();
        assert_eq!(group.name.as_ref(), "グループ 5");
        invoke(&mut session, CommandId::Undo);
        assert!(session.engine().document().layer(group.id).is_none());
        assert_eq!(session.engine().document().layer(copy).unwrap().properties.parent, None);
        invoke(&mut session, CommandId::Redo);
        assert_eq!(session.engine().document().layer(group.id).unwrap(), &group);
        session.dispatch(UiAction::Selection { action: SelectionAction::NewLayer { parent: Some(group.id.0), save_current: false } }).unwrap();
        let selection = session.engine().document().layers.iter().find(|layer| layer.id == LayerId(6)).unwrap();
        assert_eq!(selection.name.as_ref(), "選択範囲 6");
        layer(&mut session, LayerAction::New { group: true, clipped: false });
        assert_eq!(session.engine().document().layer(LayerId(7)).unwrap().name.as_ref(), "グループ 7");
        let imported_name = "  Current ink { $name }「写真」🖼️\u{2068}literal\u{2069}  ";
        let source = layer_core::color::source::rgba8_source([2, 2], |_, _| [200; 4]);
        session.import_layer_source(imported_name, Arc::unwrap_or_clone(source)).unwrap();
        let imported = session.engine().document().active_layer;
        assert_eq!(session.engine().document().layer(imported).unwrap().name.as_ref(), imported_name);
        let captured = session.capture_project_recovery().unwrap();
        let mut bytes = Vec::new();
        captured.write(&mut bytes).unwrap();
        let read = Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
        assert_eq!(read, captured);
        let reopened = UiSession::from_project_localized(Recorder { tiled_sources: true, ..Default::default() }, read, None, [256, 256], Platform::Gtk, english()).unwrap();
        assert_eq!(reopened.engine().document(), &captured.document);
        assert_eq!(reopened.engine().document().id.as_ref(), "untitled");
        assert_eq!(reopened.engine().document().layer(paint).unwrap().name.as_ref(), literal);
        assert_eq!(reopened.engine().document().layer(imported).unwrap().name.as_ref(), imported_name);
    }

    #[test]
    fn builtin_presets_have_identity_and_default_looking_user_names_stay_literal() {
        let hdr = NewDocumentSettings::default().form(&english()).presets.into_iter().find(|p| p.name == "HDR drawing").unwrap();
        assert_eq!(hdr.options.color, DocumentColor { space: RgbSpace::Srgb, depth: SampleDepth::F16 });
        let mut settings = NewDocumentSettings::default();
        settings.apply(NewDocumentAction::Remember { options: Default::default(), name: "HDR drawing".into(), defaults: false }).unwrap();
        let form = settings.form(&Localizer::shared(crate::UiLanguage::Japanese));
        assert_eq!(form.presets[3].id, NewDocumentPresetId::HdrDrawing);
        assert_eq!(form.presets[4].id, NewDocumentPresetId::Saved { index: 0 });
        assert_eq!(form.presets[4].name, "HDR drawing");
        assert_eq!(form.presets[3].remove, None);
        assert_eq!(form.presets[4].remove, Some(NewDocumentAction::Remove { index: 0 }));
        settings.defaults = NewDocumentOptions { extent: [513, 257], ..Default::default() };
        settings.apply(NewDocumentAction::Remember { options: settings.defaults, name: "{ $name }「絵」🖌️".into(), defaults: false }).unwrap();
        let form = settings.form(&english());
        assert_eq!(form.selected, Some(NewDocumentPresetId::Saved { index: 1 }));
        assert_eq!(form.presets[5].name, "{ $name }「絵」🖌️");
    }
    #[test]
    fn creation_appearance_projects_depth_policy_and_whole_color_summary() {
        let options = NewDocumentOptions { color: DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U8 }, ..Default::default() };
        let view = options.appearance(&english());
        assert!(view.note.is_some());
        assert!(view.blending_editable);
        assert_eq!(view.blending, BlendSpace::Perceptual);
        assert_eq!(view.summary, "ProPhoto RGB · 8-bit SDR · Perceptual");
        let view = NewDocumentOptions { color: DocumentColor { depth: SampleDepth::F32, ..options.color }, ..options }.appearance(&english());
        assert!(view.note.is_none());
        assert!(!view.blending_editable);
        assert_eq!(view.blending, BlendSpace::Linear);
        assert_eq!(view.blending_help.as_ref(), "Float documents blend in linear light");
    }

    #[test]
    fn preference_actions_validate_before_changing_defaults_or_presets() {
        let mut settings = NewDocumentSettings::default();
        let options = NewDocumentOptions { extent: [513, 257], ..Default::default() };
        settings.apply(NewDocumentAction::Remember { options, name: " Photo ".into(), defaults: true }).unwrap();
        let saved = settings.clone();
        assert!(settings.apply(NewDocumentAction::Remember { options: Default::default(), name: "photo".into(), defaults: true }).is_err());
        assert_eq!(settings, saved);
        assert!(settings.apply(NewDocumentAction::Remove { index: 1 }).is_err());
        assert_eq!(settings, saved);
        settings.apply(NewDocumentAction::Remove { index: 0 }).unwrap();
        assert!(settings.presets.is_empty());
        assert_eq!(settings.defaults, options);
    }
    #[test]
    fn new_documents_blend_perceptually_except_at_float_and_presets_keep_the_choice() {
        let project = NewDocumentOptions::default().project(&english()).unwrap();
        assert_eq!(project.document.blend_space, BlendSpace::Perceptual);
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            for blend_space in BlendSpace::ALL {
                let options = NewDocumentOptions { color: DocumentColor { depth, ..Default::default() }, blend_space, ..Default::default() };
                assert_eq!(options.project(&english()).unwrap().document.blend_space, if depth.is_float() { BlendSpace::Linear } else { blend_space });
            }
        }
        let options = NewDocumentOptions { blend_space: BlendSpace::Linear, ..Default::default() };
        let mut settings = NewDocumentSettings::default();
        settings.apply(NewDocumentAction::Remember { options, name: "Linear".into(), defaults: true }).unwrap();
        let read: NewDocumentSettings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!((read.defaults.blend_space, read.presets[0].options.blend_space), (BlendSpace::Linear, BlendSpace::Linear));
        let mut older = serde_json::to_value(&settings).unwrap();
        older["defaults"].as_object_mut().unwrap().remove("blend_space");
        older["presets"][0]["options"].as_object_mut().unwrap().remove("blend_space");
        let older: NewDocumentSettings = serde_json::from_value(older).unwrap();
        assert_eq!((older.defaults.blend_space, older.presets[0].options.blend_space), (BlendSpace::Perceptual, BlendSpace::Perceptual));
        let form = serde_json::to_value(NewDocumentSettings::default().form(&english())).unwrap();
        assert_eq!(form["blending"]["choices"][0]["description"], "Like Photoshop and Clip Studio Paint");
        assert_eq!(form["blending"]["float_reason"], "Float documents blend in linear light");
        assert_eq!(form["blending"]["linear_only"], serde_json::json!(["F16", "F32"]));
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
                    };
                    let project = options.project(&english()).unwrap();
                    assert_eq!(project.document.color, options.color);
                    assert_eq!(
                        project.document.layers[1].visible,
                        background == DocumentBackground::White
                    );
                    let mut bytes = Vec::new();
                    project.write(&mut bytes).unwrap();
                    assert_eq!(
                        Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap(),
                        project
                    );
                }
            }
        }
        assert_eq!(
            NewDocumentPreset::builtins(&english())[0].options,
            NewDocumentOptions::default()
        );
    }
    #[test]
    fn saved_presets_validate_atomically_and_roundtrip() {
        let mut settings = NewDocumentSettings::default();
        let options = NewDocumentPreset::builtins(&english())[2].options;
        settings.save(" My photo ", options).unwrap();
        settings.defaults = options;
        let before = settings.clone();
        assert!(settings.save("my PHOTO", options).is_err());
        assert!(settings.save("MY photo", options).is_err());
        assert!(
            settings
                .save(
                    "Bad size",
                    NewDocumentOptions {
                        extent: [0, 8193],
                        ..options
                    }
                )
                .is_err()
        );
        assert_eq!(settings, before);
        assert_eq!(
            serde_json::from_str::<NewDocumentSettings>(&serde_json::to_string(&settings).unwrap())
                .unwrap(),
            settings
        );
    }
}
