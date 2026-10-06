//! Transient notices: refusals and hints raised by canvas gestures. The core
//! authors the text and keeps any action; hosts show each new id once, hide it
//! after a short timeout or at the next canvas contact, and answer with
//! `UiAction::Notice`.
use super::*;
use layer_core::{DrawingRefusal, RetouchSource};
use layer_engine::StrokeRefusal;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Notice {
    /// Increases with every notice, so a repeated refusal is shown again.
    pub id: u64,
    pub text: String,
    pub actions: Vec<NoticeAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeActionId { CloneSource, UseReference, Flatten, AddMask, EditMask, NewPaintLayer, RasterizeLayer }

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoticeAction {
    pub id: NoticeActionId,
    pub label: String,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Clone)]
enum NoticeCopy {
    Drawing(DrawingRefusal), Stroke(StrokeRefusal), Message(MessageId), Reference { message: MessageId, name: std::sync::Arc<str> },
    Object { layer: OccurrenceHandle, name: std::sync::Arc<str> }, Flatten { hidden: usize },
}

impl NoticeCopy {
    fn text(&self, localization: &Localizer) -> String {
        match self {
            Self::Drawing(reason) => drawing_refusal_text(*reason, localization),
            Self::Stroke(reason) => stroke_refusal_text(*reason, localization),
            Self::Message(message) | Self::Reference { message, .. } => localization.text(*message),
            Self::Object { name, .. } => {
                let mut args = FluentArgs::new(); args.set("layer", name.as_ref());
                return localization.format(MessageId::OBJECTS_REFUSAL_PAINT, &args);
            }
            Self::Flatten { hidden } => {
                let mut args = FluentArgs::new(); args.set("count", *hidden);
                return localization.format(MessageId::COMMANDS_FLATTEN_DISCARDS_HIDDEN_LAYERS, &args);
            }
        }.to_string()
    }
    fn action_label(&self, id: NoticeActionId, localization: &Localizer) -> String {
        match (self, id) {
            (Self::Reference { name, .. }, _) => {
                let mut args = FluentArgs::new(); args.set("name", name.as_ref());
                localization.format(MessageId::RESOURCES_REFERENCE_USE_LAYER, &args)
            }
            (_, NoticeActionId::CloneSource) => CommandId::CloneSourceArm.localized_label(localization).to_string(),
            (_, NoticeActionId::UseReference) => CommandId::UseReferenceBelow.localized_label(localization).to_string(),
            (_, NoticeActionId::Flatten) => CommandId::FlattenImage.localized_label(localization).to_string(),
            (_, NoticeActionId::AddMask) => localization.text(MessageId::OBJECTS_ACTION_ADD_MASK).to_string(),
            (_, NoticeActionId::EditMask) => localization.text(MessageId::OBJECTS_ACTION_EDIT_MASK).to_string(),
            (_, NoticeActionId::NewPaintLayer) => localization.text(MessageId::OBJECTS_ACTION_NEW_PAINT_LAYER).to_string(),
            (_, NoticeActionId::RasterizeLayer) => CommandId::RasterizeLayer.localized_label(localization).to_string(),
        }
    }
}

#[derive(Clone)]
pub(super) enum NoticeRun { Dispatch(Box<UiAction>), RevealMask(OccurrenceHandle) }

#[derive(Default)]
pub(super) struct Notices {
    last_id: u64,
    copy: Option<NoticeCopy>,
    actions: Vec<(NoticeActionId, NoticeRun)>,
    owner: u64,
    epoch: u64,
    changed: bool,
    /// The mask whose editing session already explained dry coverage.
    dry_mask: Option<SourceTarget>,
}

impl Notices {
    pub(super) fn publishing(&self) -> bool {
        self.changed
    }
}

pub(super) fn drawing_refusal_text(refusal: DrawingRefusal, l: &Localizer) -> std::sync::Arc<str> {
    match refusal {
        DrawingRefusal::NoLayer => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_SELECT_A_LAYER_TO_DRAW_ON),
        DrawingRefusal::Locked => l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED),
        DrawingRefusal::BaseLocked => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THE_LAYER_BELOW_THIS_EFFECT_IS_LOCKED),
        DrawingRefusal::Group => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_A_GROUP_HAS_NO_PIXELS_OF_ITS_OWN_SELECT_A_LAYER_INSIDE_IT),
        DrawingRefusal::Fill => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_ADD_A_MASK_TO_PAINT_ON_THIS_FILL_LAYER),
        DrawingRefusal::SelectionLayer => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_A_SELECTION_LAYER_HOLDS_A_SELECTION_NOT_PAINT),
        DrawingRefusal::EffectWithoutBase => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_EFFECT_LAYER_HAS_NO_LAYER_BELOW_IT_TO_DRAW_ON),
        DrawingRefusal::Mask => l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST),
        DrawingRefusal::EffectMask => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_TOOL_DRAWS_ON_ARTWORK_NOT_ON_AN_EFFECT_LAYER_S_MASK),
        DrawingRefusal::Object => l.text(MessageId::OBJECTS_REFUSAL_LAYER),
    }
}

fn stroke_refusal_text(refusal: StrokeRefusal, l: &Localizer) -> std::sync::Arc<str> {
    match refusal {
        StrokeRefusal::Target(refusal) => drawing_refusal_text(refusal, l),
        StrokeRefusal::AlphaLocked => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_ALPHA_LOCK_KEEPS_THIS_LAYER_S_TRANSPARENCY_SO_ERASING_HAS_NO_EFFECT),
        StrokeRefusal::DryMask => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_MASKS_TAKE_DRY_COVERAGE_SO_THIS_BRUSH_PAINTS_WITHOUT_ITS_WET_OR_BLENDING_BEHAVIOR),
        StrokeRefusal::EmptySource(RetouchSource::Editing) => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_LAYER_IS_EMPTY_SO_THERE_S_NOTHING_TO_COPY),
        StrokeRefusal::EmptySource(RetouchSource::References) => {
            l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THIS_LAYER_IS_EMPTY_AND_THERE_S_NO_LAYER_BELOW_IT_TO_COPY_FROM)
        }
        StrokeRefusal::NoCloneSource => l.text(MessageId::COMMANDS_REFUSAL_NOTICES_CHOOSE_WHERE_TO_COPY_FROM_FIRST),
    }
}

pub(super) const NO_REFERENCE_BELOW: MessageId = MessageId::COMMANDS_REFUSAL_NOTICES_NO_VISIBLE_PHOTO_OR_PAINT_LAYER_BELOW;

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn refresh_notice_localization(&mut self) {
        if let (Some(copy), Some(notice)) = (self.notices.copy.clone(), &mut self.state.notice) {
            notice.text = copy.text(&self.state.localization);
            for action in &mut notice.actions { action.label = copy.action_label(action.id, &self.state.localization); }
            self.notices.changed = true;
        }
        if let Some(NoticeCopy::Object { layer, .. }) = self.notices.copy.clone() {
            let actions = self.object_refusal_actions(layer);
            if let Some(notice) = &mut self.state.notice { notice.actions = actions.into_iter().map(|(action, _)| action).collect(); }
        }
    }

    pub fn update_notice_text(&mut self, id: u64, text: String) -> bool {
        let Some(notice) = self.state.notice.as_mut().filter(|notice| notice.id == id) else { return false; };
        if notice.text != text {
            notice.text = text;
            self.notices.changed = true;
        }
        true
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.raise_notice(text.into(), Vec::new());
    }

    pub fn raise_message_notice(&mut self, message: MessageId) {
        self.raise_notice(self.localization().text(message).to_string(), Vec::new());
        self.notices.copy = Some(NoticeCopy::Message(message));
    }

    pub(super) fn raise_notice(&mut self, text: String, actions: Vec<(NoticeAction, NoticeRun)>) {
        let notices = &mut self.notices;
        notices.last_id += 1;
        notices.epoch = self.state.document_file.epoch;
        notices.owner = self.engine.document().owner;
        notices.changed = true;
        notices.actions = actions.iter().map(|(action, run)| (action.id, run.clone())).collect();
        notices.copy = None;
        self.state.notice = Some(Notice { id: notices.last_id, text, actions: actions.into_iter().map(|(action, _)| action).collect() });
    }

    fn notice_choice(&self, copy: &NoticeCopy, id: NoticeActionId, run: NoticeRun, reason: Option<std::sync::Arc<str>>) -> (NoticeAction, NoticeRun) {
        (NoticeAction { id, label: copy.action_label(id, self.localization()), enabled: reason.is_none(), reason: reason.map(|r| r.to_string()) }, run)
    }

    fn object_refusal_actions(&self, layer: OccurrenceHandle) -> Vec<(NoticeAction, NoticeRun)> {
        let doc = self.engine.document();
        let Some(occurrence) = doc.scene().occurrence(layer) else { return Vec::new(); };
        let copy = NoticeCopy::Object { layer, name: occurrence.name.clone() };
        let l = self.localization();
        let locked = doc.is_locked(layer).then(|| l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED));
        let parent_locked = doc.scene().parent(layer).filter(|parent| doc.is_locked(*parent)).map(|_| l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED));
        let id = occurrence_token(layer);
        let rasterize = self.command(CommandId::RasterizeLayer);
        vec![
            match &occurrence.mask {
                Some(_) => self.notice_choice(&copy, NoticeActionId::EditMask, NoticeRun::Dispatch(Box::new(UiAction::Layer { action: LayerAction::Select { id, mask: true } })), locked.clone()),
                None => self.notice_choice(&copy, NoticeActionId::AddMask, NoticeRun::RevealMask(layer), locked.clone()),
            },
            self.notice_choice(&copy, NoticeActionId::NewPaintLayer, NoticeRun::Dispatch(Box::new(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } })), parent_locked),
            self.notice_choice(&copy, NoticeActionId::RasterizeLayer, NoticeRun::Dispatch(Box::new(UiAction::Invoke { command: CommandId::RasterizeLayer })),
                (!rasterize.enabled).then(|| rasterize.disabled_reason.clone().unwrap_or_default())),
        ]
    }

    pub(super) fn notify_object_refusal(&mut self, layer: OccurrenceHandle) {
        let Some(name) = self.engine.document().scene().occurrence(layer).map(|o| o.name.clone()) else { return; };
        let copy = NoticeCopy::Object { layer, name };
        let actions = self.object_refusal_actions(layer);
        self.raise_notice(copy.text(self.localization()), actions);
        self.notices.copy = Some(copy);
    }

    fn reveal_layer_mask(&mut self, layer: OccurrenceHandle) -> Result<(), String> {
        let doc = self.engine.document();
        let occurrence = doc.scene().occurrence(layer).ok_or("Unknown layer")?.clone();
        if occurrence.mask.is_some() { return self.layer_action(LayerAction::Select { id: occurrence_token(layer), mask: true }); }
        if doc.is_locked(layer) { return Err(self.localization().text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED).to_string()); }
        let handle = doc.artwork.coverage.next_handle();
        let snapshot = layer_core::CoverageSnapshot::reveal_all(handle, doc.composition().size, [0; 2]);
        let coverage = layer_core::RecordChange::insert(&doc.artwork.coverage, snapshot.source);
        let mut owner = occurrence;
        owner.mask = Some(layer_core::authored::MaskUse { source: coverage.handle, ..snapshot.use_ });
        let mut working = doc.working.clone();
        working.occurrence = Some(layer);
        working.target = Some(SourceTarget::Coverage(coverage.handle));
        working.inspect_mask = None;
        let edit = layer_core::Edit::Batch(vec![layer_core::Edit::Coverage(coverage),
            layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&doc.artwork.occurrences, layer, Some(owner))?), layer_core::Edit::Working(working)]);
        self.layer_edit(edit)?;
        self.layer_interaction.tool = LayerCanvasTool::Paint;
        self.state.layer_tools.tool = LayerCanvasTool::Paint;
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn dismiss_notice(&mut self) {
        if self.state.notice.take().is_some() {
            self.notices.actions.clear();
            self.notices.copy = None;
            self.notices.changed = true;
        }
    }

    /// Identifies the current notice so a contact can clear it unless the
    /// contact raised a new one.
    pub(super) fn notice_id(&self) -> Option<u64> {
        self.state.notice.as_ref().map(|n| n.id)
    }

    pub(super) fn dismiss_notice_unless_raised(&mut self, before: Option<u64>) {
        if before.is_some() && self.notice_id() == before {
            self.dismiss_notice();
        }
    }

    /// HOST when the notice changed since the last publication. A notice
    /// never outlives the document activation that raised it.
    pub(super) fn notice_regions(&mut self) -> u32 {
        if self.notices.epoch != self.state.document_file.epoch {
            self.dismiss_notice();
            self.notices.epoch = self.state.document_file.epoch;
        }
        if std::mem::take(&mut self.notices.changed) { regions::HOST } else { 0 }
    }

    /// Keep notice ids increasing across the documents of one window.
    pub(super) fn inherit_notice_ids(&mut self, previous: &Self) {
        self.notices.last_id = self.notices.last_id.max(previous.notices.last_id);
    }

    pub(super) fn notice_action(&mut self, id: u64, accept: bool, action: Option<NoticeActionId>) -> Result<UiChange, String> {
        if self.notice_id() != Some(id) {
            return Err("This notice was already dismissed".into());
        }
        let enabled = |token: NoticeActionId| self.state.notice.as_ref().is_some_and(|n| n.actions.iter().any(|a| a.id == token && a.enabled));
        let chosen = if !accept { None } else {
            let token = action.or_else(|| self.state.notice.as_ref()?.actions.iter().find(|a| a.enabled).map(|a| a.id));
            if token.is_some_and(|token| !enabled(token)) { return Err("This action isn't available".into()); }
            token.and_then(|token| self.notices.actions.iter().find(|(id, _)| *id == token).map(|(_, run)| run.clone()))
        };
        let copy = self.notices.copy.clone();
        let current = self.notices.owner == self.engine.document().owner && self.notices.epoch == self.state.document_file.epoch;
        self.dismiss_notice();
        let Some(run) = chosen else { return Ok(self.changed(0, false)); };
        if let Some(NoticeCopy::Object { layer, .. }) = copy {
            let doc = self.engine.document();
            if !current || doc.working.occurrence != Some(layer) || doc.scene().object_layer(layer).is_none() {
                return Err(self.localization().text(MessageId::OBJECTS_NOTICE_STALE).to_string());
            }
        }
        match run {
            NoticeRun::Dispatch(action) => self.dispatch(*action),
            NoticeRun::RevealMask(layer) => {
                self.reveal_layer_mask(layer)?;
                self.refresh_document();
                Ok(self.changed(regions::DOCUMENT | regions::BRUSH | regions::COMMANDS, true))
            }
        }
    }

    /// At pen-down, explain a brush stroke that will not paint, or will paint
    /// differently than configured.
    pub(super) fn notify_stroke_refusal(&mut self, event: &PenEvent) {
        let refusal = self.engine.stroke_refusal(event);
        if refusal == Some(StrokeRefusal::DryMask) {
            let mask = self.engine.document().drawing_target();
            if self.notices.dry_mask == mask {
                return;
            }
            self.notices.dry_mask = mask;
        }
        match refusal {
            Some(StrokeRefusal::EmptySource(RetouchSource::References)) if self.reference_below().is_some() => self
                .offer_reference_below(MessageId::RESOURCES_REFERENCE_EMPTY_UNMARKED),
            Some(StrokeRefusal::Target(DrawingRefusal::Object)) => {
                if let Some(layer) = self.engine.document().working.occurrence { self.notify_object_refusal(layer); }
            }
            Some(refusal) => {
                let copy = NoticeCopy::Stroke(refusal);
                let actions = (refusal == StrokeRefusal::NoCloneSource).then(|| self.notice_choice(&copy, NoticeActionId::CloneSource,
                    NoticeRun::Dispatch(Box::new(UiAction::Invoke { command: CommandId::CloneSourceArm })), None)).into_iter().collect();
                self.raise_notice(stroke_refusal_text(refusal, self.localization()).to_string(), actions);
                self.notices.copy = Some(copy);
            }
            None => {}
        }
    }

    /// A mask-editing session ends when the brush target is no longer its mask.
    pub(super) fn end_dry_mask_session(&mut self, drawing_target: Option<SourceTarget>) {
        if self.notices.dry_mask != drawing_target {
            self.notices.dry_mask = None;
        }
    }

    /// Whether a pixel write on the active layer would land on its images.
    pub(super) fn image_content(&self) -> bool {
        self.engine.document().drawing_refusal() == Some(DrawingRefusal::Object)
    }

    /// Refuse a pixel write on image content with the image actions.
    pub(super) fn refuse_image_content(&mut self) -> bool {
        let image = self.image_content();
        if image { self.notify_drawing_refusal(); }
        image
    }

    /// Explain why a content tool (Move, Lasso Fill, Gradient, Figure, Fill)
    /// has nothing to act on.
    pub(super) fn notify_drawing_refusal(&mut self) {
        if self.engine.document().drawing_refusal() == Some(DrawingRefusal::Object)
            && let Some(layer) = self.engine.document().working.occurrence {
            return self.notify_object_refusal(layer);
        }
        if let Some(refusal) = self.engine.document().drawing_refusal() {
            self.notify(drawing_refusal_text(refusal, self.localization()).to_string());
            self.notices.copy = Some(NoticeCopy::Drawing(refusal));
        }
    }

    /// The nearest visible Paint layer, photo or painting, below the active
    /// layer.
    pub(super) fn reference_below(&self) -> Option<(OccurrenceHandle, &layer_core::Occurrence)> {
        let doc = self.engine.document();
        let scene = doc.scene();
        let index = scene.position(doc.working.occurrence?)?;
        scene.order()[index + 1..].iter().find_map(|handle| {
            let occurrence = scene.occurrence(*handle)?;
            (matches!(occurrence.kind(), LayerKind::Paint | LayerKind::Object) && doc.layer_is_visible(*handle)).then_some((*handle, occurrence))
        })
    }

    pub(super) fn use_reference_below_reason(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        match self.reference_below() {
            None => Some(l.text(MessageId::COMMANDS_REFUSAL_NOTICES_NO_VISIBLE_PHOTO_OR_PAINT_LAYER_BELOW)),
            Some((_, layer)) if layer.reference => {
                Some(l.text(MessageId::COMMANDS_REFUSAL_NOTICES_THE_LAYER_BELOW_IS_ALREADY_A_REFERENCE))
            }
            Some(_) => None,
        }
    }

    pub(super) fn use_reference_below(&mut self) -> Result<(), String> {
        refused(self.use_reference_below_reason())?;
        let id = self.reference_below().ok_or_else(|| self.localization().text(NO_REFERENCE_BELOW).to_string())?.0;
        let mut references = self.engine.document().scene().references();
        references.insert(id);
        self.set_references(references)
    }

    /// Wand and Fill sampling reference layers when none is marked.
    pub(super) fn notify_missing_reference(&mut self) {
        if self.reference_below().is_some() {
            self.offer_reference_below(MessageId::RESOURCES_REFERENCE_TOOL_UNMARKED);
        } else {
            self.raise_message_notice(MessageId::RESOURCES_REFERENCE_TOOL_MARK_FIRST);
        }
    }

    pub(super) fn offer_flatten(&mut self, hidden: usize) {
        let copy = NoticeCopy::Flatten { hidden };
        let action = self.notice_choice(&copy, NoticeActionId::Flatten, NoticeRun::Dispatch(Box::new(UiAction::Layer { action: LayerAction::Flatten })), None);
        self.raise_notice(copy.text(self.localization()), vec![action]);
        self.notices.copy = Some(copy);
    }

    fn offer_reference_below(&mut self, message: MessageId) {
        if let Some((_, layer)) = self.reference_below() {
            let copy = NoticeCopy::Reference { message, name: layer.name.clone() };
            let action = self.notice_choice(&copy, NoticeActionId::UseReference, NoticeRun::Dispatch(Box::new(UiAction::Invoke { command: CommandId::UseReferenceBelow })), None);
            self.raise_notice(copy.text(self.localization()), vec![action]);
            self.notices.copy = Some(copy);
        }
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::session;

    #[test]
    fn reference_offer_language_refresh_retains_literal_name_and_action() {
        let mut s = session(Platform::Gtk);
        let name = "日本語 { $name } 🎨";
        use layer_core::{Edit, authored::{Occurrence, OccurrenceContent, PaintSource, RecordChange}};
        let artwork=&s.engine.document().artwork;
        let paint=RecordChange::insert(&artwork.paint,PaintSource { color_mode: Default::default(),domain:s.engine.document().composition().size,raster:Default::default(),base:None,operations:Default::default()});
        let occurrence=RecordChange::insert(&artwork.occurrences,Occurrence::new(OccurrenceContent::Paint(paint.handle),name));
        let id=occurrence.handle;let root=s.engine.document().composition().result;
        let mut stack=artwork.stacks.get(root).unwrap().clone();stack.entries.insert(1,id);
        let membership=RecordChange::replace(&artwork.stacks,root,Some(stack)).unwrap();
        s.engine.apply_edit(Edit::Batch(vec![Edit::Paint(paint),Edit::Occurrence(occurrence),Edit::Stack(membership)])).unwrap();
        s.notify_missing_reference();
        let notice = s.state.notice.as_ref().unwrap().id;
        let document = s.engine.document().clone();
        let checkpoint = s.engine.checkpoint();
        for language in UiLanguage::ALL {
            s.set_localization(Localizer::shared(language));
            let current = s.state.notice.as_ref().unwrap();
            assert_eq!(current.id, notice);
            assert_eq!(current.text, s.localization().text(MessageId::RESOURCES_REFERENCE_TOOL_UNMARKED).as_ref());
            let mut args = FluentArgs::new(); args.set("name", name);
            assert_eq!(current.actions[0].label, s.localization().format(MessageId::RESOURCES_REFERENCE_USE_LAYER, &args));
            assert!(matches!(&s.notices.actions[..], [(NoticeActionId::UseReference, NoticeRun::Dispatch(action))] if matches!(**action, UiAction::Invoke { command: CommandId::UseReferenceBelow })));
            assert_eq!(s.engine.document(), &document);
            assert_eq!(s.engine.checkpoint(), checkpoint);
        }
        s.notice_action(notice, true, None).unwrap();
        assert!(s.engine.document().scene().occurrence(id).unwrap().reference);
    }

    #[test]
    fn notice_language_refresh_preserves_identity_action_and_dry_mask_lifetime() {
        let mut session = session(Platform::Gtk);
        let action = session.notice_choice(&NoticeCopy::Stroke(StrokeRefusal::NoCloneSource), NoticeActionId::CloneSource, NoticeRun::Dispatch(Box::new(UiAction::Invoke { command: CommandId::CloneSourceArm })), None);
        session.raise_notice(stroke_refusal_text(StrokeRefusal::NoCloneSource, session.localization()).to_string(), vec![action]);
        session.notices.copy = Some(NoticeCopy::Stroke(StrokeRefusal::NoCloneSource));
        session.notices.dry_mask = Some(SourceTarget::Coverage(layer_core::CoverageHandle::from_index(17)));
        let before = session.state.notice.clone().unwrap();
        let epoch = session.notices.epoch;
        assert!(session.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = session.state.notice.as_ref().unwrap();
        assert_eq!(after.id, before.id);
        assert_ne!(after.text, before.text);
        assert_ne!(after.actions, before.actions);
        assert_eq!(session.notices.epoch, epoch);
        assert_eq!(session.notices.dry_mask, Some(SourceTarget::Coverage(layer_core::CoverageHandle::from_index(17))));
        assert!(matches!(&session.notices.actions[..], [(NoticeActionId::CloneSource, NoticeRun::Dispatch(action))] if matches!(**action, UiAction::Invoke { command: CommandId::CloneSourceArm })));
        session.notify("literal { $name } 🖌");
        assert!(session.set_localization(Localizer::shared(UiLanguage::English)));
        assert_eq!(session.state.notice.as_ref().unwrap().text, "literal { $name } 🖌");
        let id = session.state.notice.as_ref().unwrap().id;
        assert!(!session.update_notice_text(id - 1, "stale result".into()));
        assert!(session.update_notice_text(id, "current result".into()));
        assert_eq!(session.state.notice.as_ref().unwrap().id, id);
        assert_eq!(session.state.notice.as_ref().unwrap().text, "current result");
        session.dismiss_notice();
        assert!(!session.update_notice_text(id, "late result".into()));
    }
}
