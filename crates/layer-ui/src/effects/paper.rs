use super::*;
use crate::paper_library::PaperLibrary;
use layer_core::{TextureSource, paper};

pub(super) const TEXTURE: &str = "texture";
pub(super) const SCALE: &str = "scale";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperPickerView {
    pub tiles: Vec<PaperTileView>,
    pub import: Option<PropertyActionView>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperTileView {
    pub id: Option<Arc<str>>,
    pub label: Arc<str>,
    pub actions: Vec<PropertyActionView>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerPaper {
    pub id: Option<Arc<str>>,
    pub label: Arc<str>,
    pub editable: bool,
}

#[derive(Clone)]
pub(crate) enum PaperChoice {
    Texture(TextureSource),
    Library(String),
}
pub(crate) struct PaperOption {
    pub label: Arc<str>,
    pub choice: PaperChoice,
    pub id: Option<Arc<str>>,
    pub imported: bool,
}

pub(crate) fn paper_options(current: &TextureSource, papers: &PaperLibrary, l: &Localizer) -> (Vec<PaperOption>, usize) {
    let built_in = paper::built_in_papers().iter().map(|paper| PaperOption {
        label: resource_label(&paper.label, l), choice: PaperChoice::Texture(paper.texture.clone()), id: Some(paper.id.clone()), imported: false,
    });
    let imported = papers.entries.iter().flatten().map(|entry| PaperOption {
        label: entry.name.as_str().into(),
        choice: papers.image(&entry.id).map_or_else(|| PaperChoice::Library(entry.id.clone()), |image| PaperChoice::Texture(TextureSource::Image(image.clone()))),
        id: Some(entry.id.as_str().into()), imported: true,
    });
    let mut options: Vec<_> = built_in.chain(imported).collect();
    let current_entry = match current { TextureSource::Image(image) => papers.entry_of(image).map(|entry| entry.id.as_str()), _ => None };
    let selected = options.iter().position(|option| match &option.choice {
        _ if option.imported => option.id.as_deref() == current_entry,
        PaperChoice::Texture(texture) => texture == current,
        PaperChoice::Library(_) => false,
    }).unwrap_or_else(|| {
        options.insert(0, PaperOption { label: l.text(MessageId::RESOURCES_PAPER_OWN), choice: PaperChoice::Texture(current.clone()), id: None, imported: false });
        0
    });
    (options, selected)
}

pub(crate) fn layer_paper(effect: layer_core::EffectView, papers: &PaperLibrary, editable: bool, l: &Localizer) -> Option<LayerPaper> {
    if !paper::is_paper(effect.program) { return None; }
    let current = effect.values.iter().find_map(|value| match value { EffectValue::Texture(texture) => Some(texture), _ => None })?;
    let (mut options, selected) = paper_options(current, papers, l);
    let option = options.swap_remove(selected);
    Some(LayerPaper { id: option.id, label: option.label, editable })
}

pub(super) fn choice(application: &EffectApplication, papers: &PaperLibrary) -> Option<String> {
    match texture(application)? {
        TextureSource::Image(image) => papers.entry_of(image).map(|entry| entry.id.clone()),
        texture => paper::built_in_papers().iter().find(|paper| paper.texture == *texture).map(|paper| paper.id.to_string()),
    }
}

const CHOSEN_NS: u64 = 1_200_000_000;
const RELEASED_NS: u64 = 600_000_000;
const FADE_NS: u64 = 300_000_000;

#[derive(Default)]
pub(crate) struct PaperPreview {
    layer: Option<OccurrenceHandle>,
    restart: bool,
    remaining: u64,
    last_frame: Option<(u64, bool)>,
    shown: Option<layer_render::PaperOverlay>,
}
impl PaperPreview {
    pub fn show(&mut self, layer: OccurrenceHandle) {
        self.layer = Some(layer);
        self.restart = true;
    }
    pub fn invalidate(&mut self) {
        self.shown = None;
        self.last_frame = None;
    }
    pub fn active(&self) -> bool {
        self.layer.is_some() || self.restart
    }
    pub fn update<R: CanvasRenderer>(&mut self, document: &Document, renderer: &R, dragging: Option<OccurrenceHandle>, now: u64) -> Option<Option<layer_render::PaperOverlay>> {
        if let Some(layer) = dragging { self.layer = Some(layer); }
        let lit = self.layer.and_then(|layer| document.scene().effect(layer)).and_then(paper::light).is_some_and(|light| light.relief > 0.);
        let paper = self.layer.filter(|_| !lit).and_then(|layer| document.paper_surface(layer))
            .map(|surface| paper::PaperBinding { surface: Arc::new(surface), target_offset: [0; 2] });
        let ready = paper.as_ref().is_some_and(|paper| renderer.paper_overlay_ready(paper));
        let previous = self.last_frame.replace((now, ready));
        if dragging.is_some() {
            self.remaining = RELEASED_NS;
        } else if std::mem::take(&mut self.restart) {
            self.remaining = CHOSEN_NS;
        } else if ready && let Some((previous, true)) = previous {
            self.remaining = self.remaining.saturating_sub(now.saturating_sub(previous));
        }
        let opacity = (self.remaining as f32 / FADE_NS as f32).min(1.);
        let overlay = paper.filter(|_| opacity > 0.).map(|paper| layer_render::PaperOverlay { paper, opacity, moving: dragging.is_some() });
        if overlay.is_none() { self.layer = None; self.last_frame = None; }
        (overlay != self.shown).then(|| { self.shown = overlay.clone(); overlay })
    }
}

fn scale_control() -> NumericControl {
    NumericControl {
        kind: NumericKind::Slider,
        mapping: layer_core::NumericMapping::Log,
        scale: 100.,
        unit: "%".into(),
        slider_snap: crate::NumericSliderSnap::Percent,
        resolution: 0.0001,
        default_value: Some(1.),
        ..NumericControl::number(0.25, 4., 0.01, 0)
    }
}

fn texture(application: &EffectApplication) -> Option<&TextureSource> {
    application.values.iter().find_map(|value| match value { EffectValue::Texture(texture) => Some(texture), _ => None })
}

const ORDER: [&str; 6] = ["color", TEXTURE, SCALE, paper::RELIEF, paper::LIGHT, paper::THROUGH_PAINT];

pub(super) fn arrange(controls: &mut [PropertyControl]) {
    controls.sort_by_key(|control| ORDER.iter().position(|key| *key == control.key).unwrap_or(ORDER.len()));
    let flat = controls.iter().any(|control| control.key == paper::RELIEF && control.value == EffectValue::Number(0.));
    for control in controls {
        if control.key == paper::LIGHT && let PropertyKind::Number { numeric } = &mut control.kind {
            numeric.kind = NumericKind::Dial;
        }
        control.disabled = flat && matches!(control.key.as_str(), paper::LIGHT | paper::THROUGH_PAINT);
    }
}

pub(super) fn controls(token: u64, application: &EffectApplication, document: &Document, papers: &PaperLibrary, l: &Localizer) -> Vec<PropertyControl> {
    let (Some(current), Some(spatial)) = (texture(application), application.spatial) else { return Vec::new() };
    let parameter = application.program.parameters.iter().find(|p| p.key.as_ref() == TEXTURE).expect("paper texture parameter");
    let (options, selected) = paper_options(current, papers, l);
    let default_texture = paper::default_texture();
    let default = options.iter().position(|option| matches!(&option.choice, PaperChoice::Texture(texture) if *texture == default_texture)).unwrap_or(0) as u32;
    let scale = paper::scale(&spatial, paper::paper_ppi(document.composition().resolution));
    let action = |label: MessageId, icon: Option<&str>, action: EffectAction| PropertyActionView { label: l.text(label).to_string(), icon: icon.map(str::to_string), group: None, action };
    let library = papers.entries.is_some();
    let picker = PaperPickerView {
        tiles: options.iter().enumerate().map(|(index, option)| PaperTileView {
            id: option.id.clone(),
            label: option.label.clone(),
            actions: match &option.id {
                Some(id) if option.imported => vec![
                    action(MessageId::RESOURCES_PAPER_RENAME, None, EffectAction::RenamePaper { id: id.to_string(), name: option.label.to_string() }),
                    action(MessageId::COMMON_DELETE, None, EffectAction::DeletePaper { id: id.to_string() }),
                ],
                None if library && index == 0 && matches!(current, TextureSource::Image(_)) =>
                    vec![action(MessageId::RESOURCES_PAPER_ADD, None, EffectAction::AddPaper { layer: token })],
                _ => Vec::new(),
            },
        }).collect(),
        import: library.then(|| action(MessageId::RESOURCES_PAPER_IMPORT, Some("layer-folder-open-symbolic"), EffectAction::ImportPaper { layer: token })),
    };
    vec![
        PropertyControl {
            paper: Some(picker),
            ..PropertyControl::new(TEXTURE, &resource_label(&parameter.label, l), PropertyKind::Choice { options: options.into_iter().map(|option| option.label).collect() },
                EffectValue::Choice(selected as u32), EffectValue::Choice(default))
        },
        PropertyControl::new(SCALE, &l.text(MessageId::RESOURCES_PAPER_SCALE), PropertyKind::Number { numeric: scale_control() },
            EffectValue::Number(scale as f32), EffectValue::Number(1.)),
    ]
}

pub(super) enum PaperEdit {
    Applied(EffectApplication),
    Load(String),
}

pub(super) fn with_texture(application: &EffectApplication, texture: TextureSource) -> Result<EffectApplication, String> {
    let mut edited = application.clone();
    let mut draft = EffectInstance { program: edited.program.clone(), values: edited.values };
    draft.set(TEXTURE, EffectValue::Texture(texture)).map_err(str::to_string)?;
    edited.values = draft.values;
    Ok(edited)
}

pub(super) fn edited(application: &EffectApplication, document: &Document, retained: &[layer_core::Image], papers: &PaperLibrary, key: &str, value: &EffectValue, l: &Localizer) -> Option<Result<PaperEdit, String>> {
    if !paper::is_paper(&application.program) { return None; }
    let invalid = || l.text(MessageId::RESOURCES_ERROR_INVALID_LAYER_PROPERTY).to_string();
    let mut edited = application.clone();
    match (key, value) {
        (TEXTURE, EffectValue::Choice(index)) => {
            let (mut options, _) = paper_options(texture(application)?, papers, l);
            let index = *index as usize;
            if index >= options.len() { return Some(Err(invalid())); }
            return Some(match options.swap_remove(index).choice {
                PaperChoice::Texture(texture) => {
                    if let TextureSource::Image(image)=&texture && let Some(entry)=papers.entry_of(image)
                        && retained.iter().any(|retained|retained.id()==image.id() && !Arc::ptr_eq(retained.storage(),image.storage())) {
                        Ok(PaperEdit::Load(entry.id.clone()))
                    } else {with_texture(application,texture).map(PaperEdit::Applied)}
                },
                PaperChoice::Library(id) => Ok(PaperEdit::Load(id)),
            });
        }
        (SCALE, EffectValue::Number(scale)) => {
            let spatial = edited.spatial.as_mut()?;
            if !scale.is_finite() || *scale <= 0. { return Some(Err(invalid())); }
            *spatial = paper::with_scale(spatial, f64::from(*scale), paper::paper_ppi(document.composition().resolution));
        }
        _ => return None,
    }
    Some(Ok(PaperEdit::Applied(edited)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        let mut document = crate::NewDocumentOptions::default().project(&Localizer::shared(UiLanguage::English)).unwrap();
        let paper = layer_core::paper::paper_occurrence(document.scene(), None).unwrap();
        document.working.occurrence = Some(paper);
        document
    }

    #[test]
    fn paper_properties_offer_the_built_in_papers_and_scale_at_real_size() {
        let l = Localizer::shared(UiLanguage::English);
        let document = document();
        let view = properties(&document, Default::default(), &PaperLibrary::default(), None, &l);
        assert_eq!(view.title, "Paper");
        assert_eq!(view.controls.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["color", TEXTURE, SCALE, paper::RELIEF, paper::LIGHT, paper::THROUGH_PAINT]);
        assert_eq!(view.controls.iter().map(|c| c.label.as_str()).collect::<Vec<_>>()[3..], ["Texture visibility", "Light", "Shows through paint"]);
        assert_eq!(view.controls.iter().map(|c| c.disabled).collect::<Vec<_>>(), [false, false, false, false, true, true], "a flat paper has nothing to light");
        let PropertyKind::Number { numeric } = &view.controls[4].kind else { panic!("light is a number") };
        assert_eq!((numeric.kind, numeric.min, numeric.max), (NumericKind::Dial, 0., 360.));
        assert_eq!(view.controls[4].value, EffectValue::Number(225.));
        let PropertyKind::Choice { options } = &view.controls[1].kind else { panic!("texture is a choice") };
        assert_eq!(options.iter().map(|o| o.as_ref()).collect::<Vec<_>>(), ["Hot press", "Cold press", "Rough", "Extra rough", "Fine tooth", "Bristol", "Newsprint", "Ingres",
            "Pastel", "Sanded", "Coarse grit", "Washi", "Unryu", "Handmade", "Canvas", "Linen",
            "Canvas paper", "Rough linen", "Burlap", "Gesso panel", "Gesso across", "Gesso down", "Rough gesso", "Rough across",
            "Rough down", "Matte film", "Chalkboard", "Concrete", "Sand", "Asphalt", "Barn wood", "Dry mud",
            "Crumpled", "Papyrus", "Reeds across", "Reeds down", "Leaf", "Toilet"]);
        assert_eq!(view.controls[1].value, EffectValue::Choice(4));
        assert!(!view.controls[1].modified);
        let EffectValue::Number(scale) = view.controls[2].value else { panic!("scale is a number") };
        assert!((scale - 1.).abs() < 1e-6);
    }

    #[test]
    fn paper_edits_choose_a_texture_and_rescale_about_the_origin() {
        let l = Localizer::shared(UiLanguage::English);
        let document = document();
        let (_, application) = effect_application(&document, document.working.occurrence.unwrap()).unwrap();
        let papers = PaperLibrary::default();
        let applied = |edit: Option<Result<PaperEdit, String>>| match edit.unwrap().unwrap() { PaperEdit::Applied(application) => application, PaperEdit::Load(_) => panic!("built-in papers need no loading") };
        let rough = applied(edited(application, &document, &[], &papers, TEXTURE, &EffectValue::Choice(2), &l));
        assert_eq!(texture(&rough), Some(&paper::built_in_paper("rough").unwrap().texture));
        assert_eq!(rough.spatial, application.spatial);
        let doubled = applied(edited(&rough, &document, &[], &papers, SCALE, &EffectValue::Number(2.), &l));
        let spatial = doubled.spatial.unwrap();
        assert!((paper::scale(&spatial, [300.; 2]) - 2.).abs() < 1e-9);
        assert_eq!(spatial.mapping.0[4..], [0., 0.]);
        assert!(edited(application, &document, &[], &papers, TEXTURE, &EffectValue::Choice(paper::built_in_papers().len() as u32), &l).unwrap().is_err());
        assert!(edited(application, &document, &[], &papers, "color", &EffectValue::Choice(0), &l).is_none());
    }
}
