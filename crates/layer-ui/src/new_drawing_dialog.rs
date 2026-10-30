//! The New drawing dialog, owned by the session while its New request waits.
//! Hosts open it with the screen they show it on, present
//! `NewDrawingDialogView`, keep their own color editor for the paper color and
//! send `NewDrawingAction`s. Create records the preset and leaves a
//! `NewDrawingOrder` for the host's project preparation.
use super::*;
use crate::paper_library::PaperMaterial;
use layer_core::BlendSpace;
use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
use std::sync::Arc;

const DEPTHS: [SampleDepth; 4] = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NewDrawingAction {
    Open { #[serde(default)] screen: Option<ScreenInfo>, untitled: u64 },
    Name { text: String },
    Width { value: f64 },
    Height { value: f64 },
    Resolution { value: f64 },
    Unit { unit: LengthUnit },
    Orientation { orientation: Orientation },
    Linked { linked: bool },
    Space { space: RgbSpace },
    Depth { depth: SampleDepth },
    Blending { blending: BlendSpace },
    Presets,
    Paper,
    PaperColor,
    CloseDrawer,
    /// Escape: leave preset naming, then the open drawer, then the dialog.
    Back,
    Tab { index: usize },
    Search { query: String },
    PickPreset { index: usize },
    UsePreset,
    SaveAs,
    RenamePreset { index: usize },
    DeletePreset { index: usize },
    PresetName { text: String },
    PresetGroup { group: PresetGroup },
    SavePreset,
    CancelSave,
    PickPaper { id: Option<String> },
    PaperRelief { value: f64 },
    UsePaper,
    PaperColorValue { color: RgbColor },
    Transparent { transparent: bool },
    AddPaperColor,
    RemovePaperColor { index: usize },
    UsePaperColor,
    Create,
    Cancel,
}

/// What Create chose, for the host to build into a drawing named `name`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewDrawingOrder {
    pub artwork: layer_core::authored::PortableId,
    pub setup: DrawingSetup,
    pub name: String,
    pub paper: PaperMaterial,
    pub limits: CanvasLimits,
}
impl NewDrawingOrder {
    pub fn project(&self, paper: layer_core::TextureSource, l: &Localizer) -> Result<Document, String> {
        self.setup.project(self.artwork, paper, self.limits, l)
    }
}

/// A sheet drawn at its proportions in its paper color and grain.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SheetView {
    pub extent: [u32; 2],
    pub color: RgbColor,
    pub texture: Option<Arc<str>>,
    pub shown: bool,
}
impl SheetView {
    fn new(setup: &DrawingSetup) -> Self {
        Self { extent: setup.extent(), color: setup.paper.color, texture: setup.paper.texture.clone(), shown: setup.paper.shown() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewDrawingChoice<T> {
    pub value: T,
    pub label: Arc<str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolutionChoice {
    pub value: u32,
    pub label: String,
    pub purpose: Arc<str>,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewDrawingCopy {
    pub title: Arc<str>,
    pub name: Arc<str>,
    pub size: Arc<str>,
    pub width: Arc<str>,
    pub height: Arc<str>,
    pub width_name: Arc<str>,
    pub height_name: Arc<str>,
    pub link: Arc<str>,
    pub unit: Arc<str>,
    pub portrait: Arc<str>,
    pub landscape: Arc<str>,
    pub resolution: Arc<str>,
    pub resolution_unit: Arc<str>,
    pub common_resolutions: Arc<str>,
    pub color: Arc<str>,
    pub space: Arc<str>,
    pub depth: Arc<str>,
    pub blending: Arc<str>,
    pub paper: Arc<str>,
    pub paper_color: Arc<str>,
    pub presets: Arc<str>,
    pub cancel: Arc<str>,
    pub create: Arc<str>,
    pub search: Arc<str>,
    pub select: Arc<str>,
    pub preset_name: Arc<str>,
    pub preset_category: Arc<str>,
    pub rename: Arc<str>,
    pub delete: Arc<str>,
    pub import: Arc<str>,
    pub relief: Arc<str>,
    pub relief_unit: Arc<str>,
    pub use_texture: Arc<str>,
    pub transparent: Arc<str>,
    pub sheet_mode: Arc<str>,
    pub paper_colors: Arc<str>,
    pub add_paper_color: Arc<str>,
    pub use_color: Arc<str>,
}
impl NewDrawingCopy {
    fn new(l: &Localizer) -> Self {
        let common = CommonCopy::new(l);
        let native = NativeCopy::new(l);
        let t = |id| l.text(id);
        use MessageId as M;
        Self {
            title: t(M::DOCUMENTS_NEW), name: t(M::DOCUMENTS_DRAWING_NAME), size: t(M::DOCUMENTS_SIZE_HEADING),
            width: t(M::DOCUMENTS_WIDTH_SHORT), height: t(M::DOCUMENTS_HEIGHT_SHORT),
            width_name: t(M::DOCUMENTS_WIDTH_NAME), height_name: t(M::DOCUMENTS_HEIGHT_NAME),
            link: t(M::DOCUMENTS_LINK_DIMENSIONS), unit: t(M::DOCUMENTS_UNIT_LABEL),
            portrait: t(M::DOCUMENTS_PORTRAIT), landscape: t(M::DOCUMENTS_LANDSCAPE),
            resolution: t(M::RESOURCES_SIZE_RESOLUTION), resolution_unit: "ppi".into(), common_resolutions: t(M::DOCUMENTS_COMMON_RESOLUTIONS),
            color: t(M::DOCUMENTS_COLOR_LABEL), space: t(M::DOCUMENTS_SPACE_LABEL), depth: t(M::DOCUMENTS_DEPTH_LABEL), blending: t(M::DOCUMENTS_BLENDING),
            paper: t(M::DOCUMENTS_PAPER), paper_color: t(M::DOCUMENTS_PAPER_COLOR), presets: t(M::DOCUMENTS_PRESETS),
            cancel: common.cancel.clone(), create: t(M::DOCUMENTS_CREATE),
            search: t(M::DOCUMENTS_SEARCH_PRESETS), select: t(M::DOCUMENTS_SELECT_PRESET),
            preset_name: t(M::DOCUMENTS_PRESET_NAME), preset_category: t(M::DOCUMENTS_PRESET_CATEGORY),
            rename: t(M::DOCUMENTS_RENAME_PRESET), delete: common.delete.clone(),
            import: t(M::RESOURCES_PAPER_IMPORT), relief: t(M::RESOURCES_PARAMETER_PAPER_TEXTURE_VISIBILITY), relief_unit: "%".into(),
            use_texture: native.paper.use_texture.clone(),
            transparent: t(M::DOCUMENTS_TRANSPARENT), sheet_mode: t(M::DOCUMENTS_SHEET_MODE),
            paper_colors: t(M::DOCUMENTS_PAPER_COLORS), add_paper_color: t(M::DOCUMENTS_ADD_PAPER_COLOR),
            use_color: t(M::NATIVE_COLOR_USE_COLOR),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetTabView {
    pub label: Arc<str>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetTileView {
    pub name: String,
    pub dimensions: String,
    pub details: String,
    pub sheet: SheetView,
    pub own: bool,
    pub enabled: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetSaveView {
    pub name: String,
    pub group: PresetGroup,
    pub groups: Vec<NewDrawingChoice<PresetGroup>>,
    pub confirm: Arc<str>,
    pub can_save: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetBrowserView {
    pub query: String,
    pub tabs: Vec<PresetTabView>,
    pub tabs_enabled: bool,
    pub tiles: Vec<PresetTileView>,
    pub empty: Option<String>,
    pub summary: PresetSummary,
    pub summary_sheet: SheetView,
    pub save_label: Arc<str>,
    pub can_select: bool,
    pub save: Option<PresetSaveView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperChoice {
    pub id: Option<Arc<str>>,
    pub label: Arc<str>,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperReliefView {
    pub value: f64,
    pub numeric: NumericControl,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperColorChoice {
    pub color: RgbColor,
    pub name: String,
    pub selected: bool,
}

/// The editor starts from `color`; `transparent` dims it until the color changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaperColorDrawerView {
    pub color: RgbColor,
    pub transparent: bool,
    pub palette: Vec<PaperColorChoice>,
    pub count: String,
    pub can_add: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewDrawingDrawerView {
    Presets(Box<PresetBrowserView>),
    Paper { tiles: Vec<PaperChoice>, relief: PaperReliefView },
    PaperColor(PaperColorDrawerView),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NewDrawingDialogView {
    pub copy: NewDrawingCopy,
    pub name: String,
    pub untitled: String,
    pub setup: DrawingSetup,
    pub sheet: SheetView,
    pub page: NewDrawingView,
    pub units: Vec<NewDrawingChoice<LengthUnit>>,
    pub values: [f64; 2],
    pub numeric: NumericControl,
    pub linked: bool,
    pub resolution_numeric: NumericControl,
    pub resolutions: Vec<ResolutionChoice>,
    pub spaces: Vec<NewDrawingChoice<RgbSpace>>,
    pub depths: Vec<NewDrawingChoice<SampleDepth>>,
    pub blendings: Vec<NewDrawingChoice<BlendSpace>>,
    pub blending_note: Option<Arc<str>>,
    pub paper_color_enabled: bool,
    pub can_create: bool,
    pub drawer: Option<NewDrawingDrawerView>,
}

struct PresetSave {
    name: String,
    group: PresetGroup,
    renaming: Option<String>,
}

struct Browser {
    tab: BrowserTab,
    query: String,
    picked: Option<PresetKey>,
    save: Option<PresetSave>,
}

struct PaperColorDraft {
    value: RgbColor,
    transparent: bool,
    from: Option<RgbColor>,
}

struct PaperDraft {
    picked: Option<Arc<str>>,
    relief: u32,
}

enum Drawer {
    Presets(Browser),
    Paper(PaperDraft),
    PaperColor(PaperColorDraft),
}

pub(super) struct NewDrawingDialog {
    draft: NewDrawingDraft,
    name: String,
    untitled: u64,
    screen: Option<ScreenInfo>,
    limits: CanvasLimits,
    drawer: Option<Drawer>,
}

fn opaque(color: RgbColor) -> RgbColor {
    RgbColor { rgba: [color.rgba[0], color.rgba[1], color.rgba[2], 1.], ..color }
}

fn paper_tiles(imported: &[NamedPaper], l: &Localizer) -> Vec<(Option<Arc<str>>, Arc<str>)> {
    let built_in = layer_core::paper::built_in_papers().iter().map(|paper| (Some(paper.id.clone()), texture_name(Some(&paper.id), imported, l)));
    let own = imported.iter().map(|paper| (Some(paper.id.clone()), paper.label.clone()));
    std::iter::once((None, l.text(MessageId::DOCUMENTS_NO_PAPER))).chain(built_in).chain(own).collect()
}

impl Browser {
    fn tab(&self, catalog: &PresetCatalog) -> BrowserTab {
        let tabs = catalog.tabs();
        if tabs.iter().any(|(tab, _)| *tab == self.tab) { self.tab } else { tabs.first().map_or(BrowserTab::Group(PresetGroup::Sketch), |(tab, _)| *tab) }
    }
    fn tiles(&self, catalog: &PresetCatalog) -> Vec<PresetView> { catalog.tiles(self.tab(catalog), &self.query) }
    fn picked(&self, catalog: &PresetCatalog) -> Option<PresetView> { self.picked.as_ref().and_then(|key| catalog.get(key)) }
}

impl NewDrawingDialog {
    fn new(settings: &NewDocumentSettings, screen: Option<ScreenInfo>, limits: CanvasLimits, untitled: u64) -> Self {
        Self { draft: NewDrawingDraft::new(settings.opening_setup(screen)), name: String::new(), untitled, screen, limits, drawer: None }
    }

    fn catalog<'a>(&self, settings: &'a NewDocumentSettings, imported: &'a [NamedPaper], l: &'a Localizer) -> PresetCatalog<'a> {
        PresetCatalog { settings, screen: self.screen, imported, localization: l }
    }

    fn browser(&mut self) -> Result<&mut Browser, String> {
        match &mut self.drawer { Some(Drawer::Presets(browser)) => Ok(browser), _ => Err("The presets are not open".into()) }
    }

    fn paper(&mut self) -> Result<&mut PaperDraft, String> {
        match &mut self.drawer { Some(Drawer::Paper(draft)) => Ok(draft), _ => Err("Paper is not open".into()) }
    }

    fn paper_color(&mut self) -> Result<&mut PaperColorDraft, String> {
        match &mut self.drawer { Some(Drawer::PaperColor(draft)) => Ok(draft), _ => Err("Paper color is not open".into()) }
    }

    fn begin_save(&mut self, catalog: &PresetCatalog, renaming: Option<String>) -> Result<(), String> {
        let name = renaming.clone().unwrap_or_else(|| self.name.trim().to_string());
        let browser = self.browser()?;
        let group = browser.picked(catalog).filter(|view| view.own || renaming.is_some()).map_or(PresetGroup::Yours, |view| view.group);
        browser.save = Some(PresetSave { name, group, renaming });
        Ok(())
    }

    fn own_tile(&mut self, catalog: &PresetCatalog, index: usize) -> Result<(PresetView, String), String> {
        let view = self.browser()?.tiles(catalog).into_iter().nth(index).filter(|view| view.own).ok_or("Choose one of your presets")?;
        let PresetKey::Saved(name) = view.key.clone() else { return Err("Choose one of your presets".into()) };
        Ok((view, name))
    }

    fn view(&self, catalog: &PresetCatalog) -> NewDrawingDialogView {
        let l = catalog.localization;
        let setup = &self.draft.setup;
        let blending = NewDocumentBlending::new(l);
        let float = setup.color.depth.is_float();
        NewDrawingDialogView {
            copy: NewDrawingCopy::new(l),
            name: self.name.clone(),
            untitled: crate::DocumentTabLabel::untitled(self.untitled, l),
            setup: setup.clone(),
            sheet: SheetView::new(setup),
            page: NewDrawingView::new(&self.draft, catalog, self.limits),
            units: LengthUnit::ALL.map(|unit| NewDrawingChoice { value: unit, label: unit.label(l) }).into(),
            values: [setup.width.get(), setup.height.get()],
            numeric: length_control(setup.unit, self.limits),
            linked: self.draft.linked,
            resolution_numeric: resolution_control(),
            resolutions: RESOLUTION_CHOICES.into_iter().map(|(value, purpose)| ResolutionChoice {
                value,
                label: resolution_text(value, l),
                purpose: l.text(purpose),
                enabled: setup.unit == LengthUnit::Px || DrawingSetup { resolution: value, ..setup.clone() }.validate(self.limits).is_ok(),
                selected: value == setup.resolution,
            }).collect(),
            spaces: RgbSpace::ALL.map(|space| NewDrawingChoice { value: space, label: space.name().into() }).into(),
            depths: DEPTHS.map(|depth| NewDrawingChoice { value: depth, label: crate::document_creation::depth_label(depth, l) }).into(),
            blendings: blending.choices.iter().map(|choice| NewDrawingChoice { value: choice.id, label: choice.label.clone() }).collect(),
            blending_note: float.then(|| blending.float_reason.clone()),
            paper_color_enabled: setup.paper.texture.is_some(),
            can_create: setup.validate(self.limits).is_ok(),
            drawer: self.drawer.as_ref().map(|drawer| self.drawer_view(drawer, catalog)),
        }
    }

    fn drawer_view(&self, drawer: &Drawer, catalog: &PresetCatalog) -> NewDrawingDrawerView {
        let l = catalog.localization;
        match drawer {
            Drawer::Presets(browser) => {
                let tab = browser.tab(catalog);
                let picked = browser.picked(catalog);
                let tiles = browser.tiles(catalog);
                let searching = !browser.query.trim().is_empty();
                let custom = l.text(MessageId::DOCUMENTS_CUSTOM_PRESET);
                let (name, setup) = picked.as_ref().map_or((custom.as_ref(), &self.draft.setup), |view| (view.name.as_str(), &view.setup));
                let save = browser.save.as_ref().map(|save| {
                    let check = catalog.settings.check_preset_name(&save.name, save.renaming.as_deref());
                    PresetSaveView {
                        name: save.name.clone(),
                        group: save.group,
                        groups: PresetGroup::ALL.map(|group| NewDrawingChoice { value: group, label: group.label(l) }).into(),
                        confirm: l.text(match check {
                            PresetNameCheck::Replace => MessageId::DOCUMENTS_REPLACE_PRESET,
                            PresetNameCheck::Rename => MessageId::DOCUMENTS_RENAME_CONFIRM,
                            PresetNameCheck::Save | PresetNameCheck::Invalid => MessageId::COMMON_SAVE,
                        }),
                        can_save: check != PresetNameCheck::Invalid,
                    }
                });
                NewDrawingDrawerView::Presets(Box::new(PresetBrowserView {
                    query: browser.query.clone(),
                    tabs: catalog.tabs().into_iter().map(|(candidate, label)| PresetTabView { label, selected: candidate == tab }).collect(),
                    tabs_enabled: !searching,
                    empty: (tiles.is_empty() && searching).then(|| catalog.no_match(&browser.query)),
                    tiles: tiles.into_iter().map(|view| PresetTileView {
                        selected: picked.as_ref().is_some_and(|picked| picked.key == view.key),
                        enabled: view.setup.validate(self.limits).is_ok(),
                        sheet: SheetView::new(&view.setup),
                        name: view.name, dimensions: view.dimensions, details: view.details, own: view.own,
                    }).collect(),
                    summary: catalog.summary(name, setup),
                    summary_sheet: SheetView::new(setup),
                    save_label: l.text(if picked.as_ref().is_some_and(|view| view.own) { MessageId::DOCUMENTS_RENAME_PRESET } else { MessageId::DOCUMENTS_SAVE_AS }),
                    can_select: picked.is_some(),
                    save,
                }))
            }
            Drawer::Paper(draft) => NewDrawingDrawerView::Paper {
                tiles: paper_tiles(catalog.imported, l).into_iter().map(|(id, label)| PaperChoice { selected: id == draft.picked, id, label }).collect(),
                relief: PaperReliefView { value: f64::from(draft.relief), numeric: relief_control(), enabled: draft.picked.is_some() },
            },
            Drawer::PaperColor(draft) => {
                let palette = catalog.settings.paper_palette();
                NewDrawingDrawerView::PaperColor(PaperColorDrawerView {
                    color: draft.value,
                    transparent: draft.transparent,
                    count: palette.len().to_string(),
                    can_add: !draft.transparent && palette.len() < MAX_PAPER_COLORS && !palette.contains(&opaque(draft.value)),
                    palette: palette.into_iter().map(|color| PaperColorChoice { name: paper_color_name(color, l), selected: !draft.transparent && color == draft.value, color }).collect(),
                })
            }
        }
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn new_drawing_view(&self) -> Option<&NewDrawingDialogView> { self.state.new_drawing.as_ref() }

    pub fn new_drawing_order(&self) -> Option<&NewDrawingOrder> { self.new_drawing_order.as_ref() }

    /// Names the drawing that `previous`'s order built.
    pub fn inherit_new_drawing_name(&mut self, previous: &Self) {
        if let Some(order) = previous.new_drawing_order.as_ref().filter(|order| order.artwork == self.engine.document().artwork.id) {
            self.name_new_drawing(&order.name);
        }
    }

    pub(super) fn new_drawing_imported(&mut self, id: &str) {
        if let Some(NewDrawingDialog { drawer: Some(Drawer::Paper(draft)), .. }) = &mut self.new_drawing { draft.picked = Some(id.into()); }
    }

    pub(super) fn close_new_drawing(&mut self) {
        self.new_drawing = None;
        self.state.new_drawing = None;
    }

    pub(super) fn refresh_new_drawing(&mut self) {
        let imported = self.imported_papers();
        let view = self.new_drawing.as_ref().map(|dialog| dialog.view(&dialog.catalog(&self.state.settings.new_document, &imported, &self.state.localization)));
        self.state.new_drawing = view;
    }

    pub(super) fn new_drawing_action(&mut self, action: NewDrawingAction) -> Result<(u32, bool), String> {
        let before = self.state.settings.new_document.clone();
        let result = match action {
            NewDrawingAction::Open { screen, untitled } => {
                if !self.state.requests.iter().any(|request| matches!(request.kind, HostRequestKind::Document { request: DocumentRequest::New })) {
                    return Err("Choose New drawing first".into());
                }
                self.new_drawing_order = None;
                self.new_drawing = Some(NewDrawingDialog::new(&self.state.settings.new_document, screen, self.canvas_limits(), untitled));
                Ok(())
            }
            NewDrawingAction::Create => self.create_new_drawing(),
            NewDrawingAction::Cancel => {
                self.new_drawing = None;
                Ok(())
            }
            NewDrawingAction::Back if self.new_drawing.as_ref().is_some_and(|dialog| dialog.drawer.is_none()) => {
                self.new_drawing = None;
                Ok(())
            }
            action => {
                let imported = self.imported_papers();
                let dialog = self.new_drawing.as_mut().ok_or("New drawing is not open")?;
                dialog.act(action, &mut self.state.settings.new_document, &imported, &self.state.localization)
            }
        };
        self.refresh_new_drawing();
        result?;
        let saved = self.state.settings.new_document != before;
        Ok((regions::HOST | if saved { regions::SETTINGS | regions::COMMANDS } else { 0 }, saved))
    }

    fn create_new_drawing(&mut self) -> Result<(), String> {
        let dialog = self.new_drawing.as_ref().ok_or("New drawing is not open")?;
        let limits = self.canvas_limits();
        let setup = dialog.draft.setup.clone();
        setup.validate(limits).map_err(|problem| problem.message(self.localization()))?;
        let imported = self.imported_papers();
        let preset = dialog.catalog(&self.state.settings.new_document, &imported, &self.state.localization).matching(&setup, true)
            .map(|view| view.key).filter(|key| *key != PresetKey::Last);
        let paper = setup.paper.texture.as_deref().map_or_else(|| PaperMaterial::Ready(layer_core::paper::default_texture()), |id| self.paper_material(id));
        let name = dialog.name.trim().into();
        self.state.settings.new_document.apply(NewDocumentAction::Created { setup: setup.clone(), preset }).map_err(|error| error.message(self.localization()))?;
        self.new_drawing_order = Some(NewDrawingOrder { artwork: layer_core::authored::PortableId::random(), setup, name, paper, limits });
        self.new_drawing = None;
        Ok(())
    }
}

impl NewDrawingDialog {
    fn act(&mut self, action: NewDrawingAction, settings: &mut NewDocumentSettings, imported: &[NamedPaper], l: &Localizer) -> Result<(), String> {
        let apply = |settings: &mut NewDocumentSettings, action| settings.apply(action).map_err(|error: NewDocumentError| error.message(l));
        let finite = |value: f64| if value.is_finite() { Ok(value) } else { Err(NewDocumentError::CanvasSize.message(l)) };
        match action {
            NewDrawingAction::Open { .. } | NewDrawingAction::Create | NewDrawingAction::Cancel => unreachable!(),
            NewDrawingAction::Name { text } => self.name = text,
            NewDrawingAction::Width { value } => self.draft.apply(NewDrawingEdit::Width(finite(value)?)),
            NewDrawingAction::Height { value } => self.draft.apply(NewDrawingEdit::Height(finite(value)?)),
            NewDrawingAction::Resolution { value } => self.draft.apply(NewDrawingEdit::Resolution(finite(value)?.round().clamp(1., f64::from(MAX_NEW_DOCUMENT_RESOLUTION)) as u32)),
            NewDrawingAction::Unit { unit } => self.draft.apply(NewDrawingEdit::Unit(unit)),
            NewDrawingAction::Orientation { orientation } => self.draft.apply(NewDrawingEdit::Orientation(orientation)),
            NewDrawingAction::Linked { linked } => self.draft.apply(NewDrawingEdit::Linked(linked)),
            NewDrawingAction::Space { space } => self.draft.apply(NewDrawingEdit::Space(space)),
            NewDrawingAction::Depth { depth } => self.draft.apply(NewDrawingEdit::Depth(depth)),
            NewDrawingAction::Blending { blending } => self.draft.apply(NewDrawingEdit::Blending(blending)),
            NewDrawingAction::Presets => {
                let catalog = self.catalog(settings, imported, l);
                let opening = catalog.default_tab();
                let current = catalog.matching(&self.draft.setup, true);
                let tab = current.as_ref().filter(|view| view.key != PresetKey::Last)
                    .map_or(opening, |view| if opening == BrowserTab::Recent { opening } else { BrowserTab::Group(view.group) });
                self.drawer = Some(Drawer::Presets(Browser { tab, query: String::new(), picked: current.map(|view| view.key), save: None }));
            }
            NewDrawingAction::Paper => {
                let paper = &self.draft.setup.paper;
                self.drawer = Some(Drawer::Paper(PaperDraft { picked: paper.texture.clone(), relief: paper.relief }));
            }
            NewDrawingAction::PaperColor => {
                let paper = &self.draft.setup.paper;
                if paper.texture.is_none() { return Err(l.text(MessageId::DOCUMENTS_NO_PAPER).to_string()); }
                self.drawer = Some(Drawer::PaperColor(PaperColorDraft { value: paper.color, transparent: paper.transparent, from: paper.transparent.then_some(paper.color) }));
            }
            NewDrawingAction::CloseDrawer => self.drawer = None,
            NewDrawingAction::Back => match &mut self.drawer {
                Some(Drawer::Presets(browser)) if browser.save.is_some() => browser.save = None,
                Some(Drawer::Presets(browser)) if !browser.query.is_empty() => browser.query.clear(),
                _ => self.drawer = None,
            },
            NewDrawingAction::Tab { index } => {
                let tab = self.catalog(settings, imported, l).tabs().get(index).map(|(tab, _)| *tab).ok_or("Choose a preset category")?;
                let browser = self.browser()?;
                (browser.tab, browser.query) = (tab, String::new());
            }
            NewDrawingAction::Search { query } => self.browser()?.query = query,
            NewDrawingAction::PickPreset { index } => {
                let catalog = self.catalog(settings, imported, l);
                let key = self.browser()?.tiles(&catalog).into_iter().nth(index).ok_or("Choose a preset")?.key;
                let browser = self.browser()?;
                if browser.picked.as_ref() != Some(&key) { browser.save = None; }
                browser.picked = Some(key);
            }
            NewDrawingAction::UsePreset => {
                let catalog = self.catalog(settings, imported, l);
                let setup = self.browser()?.picked(&catalog).ok_or("Choose a preset")?.setup;
                self.draft.apply(NewDrawingEdit::Setup(setup));
                self.drawer = None;
            }
            NewDrawingAction::SaveAs => {
                let catalog = self.catalog(settings, imported, l);
                let renaming = self.browser()?.picked(&catalog).filter(|view| view.own).map(|view| view.name);
                self.begin_save(&catalog, renaming)?;
            }
            NewDrawingAction::RenamePreset { index } => {
                let catalog = self.catalog(settings, imported, l);
                let (view, name) = self.own_tile(&catalog, index)?;
                self.browser()?.picked = Some(view.key);
                self.begin_save(&catalog, Some(name))?;
            }
            NewDrawingAction::DeletePreset { index } => {
                let (view, name) = self.own_tile(&self.catalog(settings, imported, l), index)?;
                apply(settings, NewDocumentAction::DeleteSetup { name })?;
                let browser = self.browser()?;
                if browser.picked.as_ref() == Some(&view.key) { browser.picked = None; }
                browser.save = None;
            }
            NewDrawingAction::PresetName { text } => self.browser()?.save.as_mut().ok_or("Name the preset first")?.name = text,
            NewDrawingAction::PresetGroup { group } => self.browser()?.save.as_mut().ok_or("Name the preset first")?.group = group,
            NewDrawingAction::SavePreset => {
                let catalog = self.catalog(settings, imported, l);
                let browser = self.browser()?;
                let save = browser.save.as_ref().ok_or("Name the preset first")?;
                if settings.check_preset_name(&save.name, save.renaming.as_deref()) == PresetNameCheck::Invalid {
                    return Err(NewDocumentError::PresetName.message(l));
                }
                let (name, group) = (save.name.trim().to_string(), save.group);
                let action = match save.renaming.clone() {
                    Some(from) => NewDocumentAction::RenameSetup { from, to: name.clone(), group },
                    None => NewDocumentAction::SaveSetup { name: name.clone(), group, setup: browser.picked(&catalog).map_or_else(|| self.draft.setup.clone(), |view| view.setup) },
                };
                apply(settings, action)?;
                let browser = self.browser()?;
                (browser.picked, browser.tab, browser.save, browser.query) = (Some(PresetKey::Saved(name)), BrowserTab::Group(group), None, String::new());
            }
            NewDrawingAction::CancelSave => self.browser()?.save = None,
            NewDrawingAction::PickPaper { id } => {
                let id = id.map(Arc::<str>::from);
                if !paper_tiles(imported, l).iter().any(|(tile, _)| *tile == id) { return Err(l.text(MessageId::RESOURCES_PAPER_OWN).to_string()); }
                self.paper()?.picked = id;
            }
            NewDrawingAction::PaperRelief { value } => {
                let control = relief_control();
                self.paper()?.relief = finite(value)?.round().clamp(control.min, control.max) as u32;
            }
            NewDrawingAction::UsePaper => {
                let Some(Drawer::Paper(PaperDraft { picked, relief })) = self.drawer.take() else { return Err("Paper is not open".into()) };
                self.draft.apply(NewDrawingEdit::Texture { texture: picked, relief });
            }
            NewDrawingAction::PaperColorValue { color } => {
                color.validate()?;
                let draft = self.paper_color()?;
                draft.value = color;
                if draft.transparent && draft.from != Some(color) { (draft.transparent, draft.from) = (false, None); }
            }
            NewDrawingAction::Transparent { transparent } => {
                let draft = self.paper_color()?;
                (draft.transparent, draft.from) = (transparent, transparent.then_some(draft.value));
            }
            NewDrawingAction::AddPaperColor => {
                let draft = self.paper_color()?;
                let color = opaque(draft.value);
                let mut colors = settings.paper_palette();
                if !draft.transparent && !colors.contains(&color) {
                    colors.push(color);
                    apply(settings, NewDocumentAction::PaperColors { colors })?;
                }
            }
            NewDrawingAction::RemovePaperColor { index } => {
                self.paper_color()?;
                let mut colors = settings.paper_palette();
                if index >= colors.len() { return Err(NewDocumentError::PresetUnavailable.message(l)); }
                colors.remove(index);
                apply(settings, NewDocumentAction::PaperColors { colors })?;
            }
            NewDrawingAction::UsePaperColor => {
                let draft = self.paper_color()?;
                let edit = NewDrawingEdit::PaperColor { color: draft.value, transparent: draft.transparent };
                self.draft.apply(edit);
                self.drawer = None;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::{session, invoke};

    fn open(s: &mut UiSession<crate::session::test_support::Recorder>) {
        invoke(s, CommandId::NewDocument);
        s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Open { screen: Some(ScreenInfo { extent: [2880, 1800], ppi: Some(220) }), untitled: 2 } }).unwrap();
    }
    fn act(s: &mut UiSession<impl CanvasRenderer>, action: NewDrawingAction) { s.dispatch(UiAction::NewDrawing { action }).unwrap(); }
    fn view(s: &UiSession<impl CanvasRenderer>) -> NewDrawingDialogView { s.state.new_drawing.clone().unwrap() }
    fn browser(s: &UiSession<impl CanvasRenderer>) -> PresetBrowserView {
        match view(s).drawer { Some(NewDrawingDrawerView::Presets(browser)) => *browser, other => panic!("{other:?}") }
    }

    #[test]
    fn the_dialog_opens_only_for_a_new_request_and_create_leaves_a_named_order() {
        let mut s = session(Platform::Gtk);
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Open { screen: None, untitled: 2 } }).is_err());
        open(&mut s);
        let opened = view(&s);
        assert_eq!((opened.untitled.as_str(), opened.page.preset.as_str()), ("Untitled 2", "Sketchbook A4"));
        assert_eq!(opened.page.orientation, Orientation::Landscape);
        assert_eq!(opened.resolutions.iter().filter(|choice| choice.selected).map(|choice| choice.value).collect::<Vec<_>>(), [300]);
        act(&mut s, NewDrawingAction::Name { text: "  Harbor study ".into() });
        act(&mut s, NewDrawingAction::Unit { unit: LengthUnit::Px });
        act(&mut s, NewDrawingAction::Width { value: 1200. });
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Width { value: f64::NAN } }).is_err());
        act(&mut s, NewDrawingAction::Create);
        assert!(s.state.new_drawing.is_none());
        let order = s.new_drawing_order().unwrap().clone();
        assert_eq!((order.name.as_str(), order.setup.extent()), ("Harbor study", [1200, 2480]));
        assert_eq!(s.state.settings.new_document.recent, [PresetKey::Last]);
        let project = order.project(order.paper.fallback(), &english()).unwrap();
        assert_eq!((project.artwork.id, project.composition().size), (order.artwork, [1200, 2480]));
        let request = s.state.requests.iter().find(|request| matches!(request.kind, HostRequestKind::Document { request: DocumentRequest::New })).unwrap().id;
        s.complete_document_request(request, Ok(true)).unwrap();
        let mut next = UiSession::from_project(crate::session::test_support::Recorder::default(), project, None, [64, 64], Platform::Gtk).unwrap();
        next.inherit_new_drawing_name(&s);
        assert_eq!(next.state.document_file.unsaved_name.as_deref(), Some("Harbor study"));
        let mut other = session(Platform::Gtk);
        other.inherit_new_drawing_name(&s);
        assert_eq!(other.state.document_file.unsaved_name, None);
    }

    #[test]
    fn untitled_names_follow_the_language_without_rewriting_the_artists_name() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            open(&mut s);
            act(&mut s, NewDrawingAction::Name { text: "Harbor {study}".into() });
            for language in [UiLanguage::Japanese, UiLanguage::German, UiLanguage::English] {
                s.set_localization(crate::Localizer::shared(language));
                let v = view(&s);
                assert_eq!(v.untitled, crate::DocumentTabLabel::untitled(2, s.localization()));
                assert_eq!(v.name, "Harbor {study}");
            }
        }
    }

    fn english() -> Arc<Localizer> { Localizer::shared(UiLanguage::English) }

    #[test]
    fn back_leaves_naming_then_the_drawer_then_the_dialog() {
        let mut s = session(Platform::Gtk);
        open(&mut s);
        act(&mut s, NewDrawingAction::Presets);
        act(&mut s, NewDrawingAction::SaveAs);
        assert!(browser(&s).save.is_some());
        act(&mut s, NewDrawingAction::Back);
        assert!(browser(&s).save.is_none());
        act(&mut s, NewDrawingAction::Back);
        assert!(view(&s).drawer.is_none());
        act(&mut s, NewDrawingAction::Back);
        assert!(s.state.new_drawing.is_none());
        assert!(s.new_drawing_order().is_none());
    }

    #[test]
    fn back_leaves_naming_then_search_then_the_drawer_on_every_platform() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            open(&mut s);
            act(&mut s, NewDrawingAction::Presets);
            act(&mut s, NewDrawingAction::Search { query: "chalk".into() });
            act(&mut s, NewDrawingAction::SaveAs);
            assert!(browser(&s).save.is_some());
            act(&mut s, NewDrawingAction::Back);
            assert!(browser(&s).save.is_none(), "{platform:?}: Back first leaves preset naming");
            assert_eq!(browser(&s).query, "chalk", "{platform:?}: leaving naming keeps the search");
            act(&mut s, NewDrawingAction::Back);
            assert!(matches!(view(&s).drawer, Some(NewDrawingDrawerView::Presets(_))), "{platform:?}: Back clears search before closing its drawer");
            assert_eq!(browser(&s).query, "");
            assert!(browser(&s).tabs_enabled);
            act(&mut s, NewDrawingAction::Back);
            assert!(view(&s).drawer.is_none());
            act(&mut s, NewDrawingAction::Back);
            assert!(s.state.new_drawing.is_none());
            assert!(s.new_drawing_order().is_none());
        }
    }

    #[test]
    fn the_browser_picks_saves_renames_and_deletes_presets() {
        let mut s = session(Platform::Gtk);
        open(&mut s);
        act(&mut s, NewDrawingAction::Presets);
        let opened = browser(&s);
        assert_eq!(opened.tabs.iter().find(|tab| tab.selected).map(|tab| tab.label.as_ref()), Some("Sketch"));
        assert_eq!((opened.summary.name.as_str(), opened.save_label.as_ref(), opened.can_select), ("Sketchbook A4", "Save As…", true));
        act(&mut s, NewDrawingAction::Search { query: "chalk".into() });
        assert!(!browser(&s).tabs_enabled);
        act(&mut s, NewDrawingAction::PickPreset { index: 0 });
        assert_eq!(browser(&s).summary.name, "Chalkboard");
        act(&mut s, NewDrawingAction::SaveAs);
        assert_eq!(browser(&s).save.unwrap().confirm.as_ref(), "Save");
        act(&mut s, NewDrawingAction::PresetName { text: "Studio wall".into() });
        act(&mut s, NewDrawingAction::SavePreset);
        let saved = browser(&s);
        assert_eq!(saved.tabs.iter().find(|tab| tab.selected).map(|tab| tab.label.as_ref()), Some("Your presets"));
        let index = saved.tiles.iter().position(|tile| tile.selected).unwrap();
        assert!(saved.tiles[index].own);
        assert_eq!(saved.save_label.as_ref(), "Rename…");
        act(&mut s, NewDrawingAction::RenamePreset { index });
        act(&mut s, NewDrawingAction::PresetName { text: "studio WALL".into() });
        assert_eq!(browser(&s).save.unwrap().confirm.as_ref(), "Rename");
        act(&mut s, NewDrawingAction::PresetGroup { group: PresetGroup::Pastel });
        act(&mut s, NewDrawingAction::SavePreset);
        assert_eq!(s.state.settings.new_document.setups[0].name, "studio WALL");
        assert_eq!(browser(&s).tabs.iter().find(|tab| tab.selected).map(|tab| tab.label.as_ref()), Some("Pastel"));
        act(&mut s, NewDrawingAction::UsePreset);
        assert_eq!(view(&s).page.preset, "studio WALL");
        act(&mut s, NewDrawingAction::Presets);
        let index = browser(&s).tiles.iter().position(|tile| tile.own).unwrap();
        act(&mut s, NewDrawingAction::DeletePreset { index });
        assert!(s.state.settings.new_document.setups.is_empty());
        assert!(!browser(&s).can_select);
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::RenamePreset { index: 0 } }).is_err());
    }

    #[test]
    fn paper_and_paper_color_drawers_choose_texture_color_and_transparency() {
        let mut s = session(Platform::Gtk);
        open(&mut s);
        let paper = |s: &UiSession<_>| match view(s).drawer { Some(NewDrawingDrawerView::Paper { tiles, relief }) => (tiles, relief), other => panic!("{other:?}") };
        act(&mut s, NewDrawingAction::Paper);
        let (tiles, relief) = paper(&s);
        assert_eq!((tiles[0].id.clone(), tiles.len()), (None, layer_core::paper::built_in_papers().len() + 1));
        assert!(tiles.iter().any(|tile| tile.selected && tile.id.as_deref() == Some("cold_press")));
        assert_eq!((relief.value, relief.enabled, relief.numeric.min, relief.numeric.max), (100., true, 0., 300.));
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::PickPaper { id: Some("missing".into()) } }).is_err());
        act(&mut s, NewDrawingAction::PaperRelief { value: 250. });
        act(&mut s, NewDrawingAction::CloseDrawer);
        assert_eq!(view(&s).setup.paper.relief, 100, "Cancel keeps the sheet's texture visibility");
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::PaperRelief { value: 40. } }).is_err());
        act(&mut s, NewDrawingAction::Paper);
        act(&mut s, NewDrawingAction::PickPaper { id: Some("cold_press".into()) });
        act(&mut s, NewDrawingAction::PaperRelief { value: 1000. });
        assert_eq!(paper(&s).1.value, 300.);
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::PaperRelief { value: f64::NAN } }).is_err());
        act(&mut s, NewDrawingAction::PaperRelief { value: 39.6 });
        act(&mut s, NewDrawingAction::UsePaper);
        assert_eq!((view(&s).setup.paper.texture.as_deref(), view(&s).setup.paper.relief), (Some("cold_press"), 40));
        assert_eq!(view(&s).page.relief.as_deref(), Some("40 %"));
        act(&mut s, NewDrawingAction::PaperColor);
        act(&mut s, NewDrawingAction::Transparent { transparent: true });
        let Some(NewDrawingDrawerView::PaperColor(drawer)) = view(&s).drawer else { panic!() };
        assert!(drawer.transparent && !drawer.can_add && drawer.palette.iter().all(|choice| !choice.selected));
        let kraft = s.state.settings.new_document.paper_palette()[5];
        act(&mut s, NewDrawingAction::PaperColorValue { color: kraft });
        let Some(NewDrawingDrawerView::PaperColor(drawer)) = view(&s).drawer else { panic!() };
        assert!(!drawer.transparent && drawer.palette[5].selected && !drawer.can_add);
        let rose = RgbColor { rgba: [0.9, 0.4, 0.5, 1.], ..kraft };
        act(&mut s, NewDrawingAction::PaperColorValue { color: rose });
        act(&mut s, NewDrawingAction::AddPaperColor);
        act(&mut s, NewDrawingAction::AddPaperColor);
        assert_eq!(s.state.settings.new_document.paper_palette().len(), PAPER_COLORS.len() + 1);
        act(&mut s, NewDrawingAction::RemovePaperColor { index: PAPER_COLORS.len() });
        assert_eq!(s.state.settings.new_document.paper_palette().len(), PAPER_COLORS.len());
        act(&mut s, NewDrawingAction::Transparent { transparent: true });
        act(&mut s, NewDrawingAction::UsePaperColor);
        let opened = view(&s);
        assert!(opened.setup.paper.transparent && !opened.sheet.shown);
        assert_eq!(opened.page.paper_color, "Transparent");
        act(&mut s, NewDrawingAction::Paper);
        act(&mut s, NewDrawingAction::PickPaper { id: None });
        assert!(!paper(&s).1.enabled, "No paper has no texture to show");
        act(&mut s, NewDrawingAction::UsePaper);
        assert!(!view(&s).paper_color_enabled);
        assert_eq!(view(&s).page.relief, None);
        assert!(s.dispatch(UiAction::NewDrawing { action: NewDrawingAction::PaperColor }).is_err());
    }

    #[test]
    fn language_changes_relabel_the_open_dialog_and_keep_its_draft() {
        let mut s = session(Platform::Gtk);
        open(&mut s);
        act(&mut s, NewDrawingAction::Resolution { value: 450. });
        act(&mut s, NewDrawingAction::Presets);
        let before = view(&s);
        assert!(s.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = view(&s);
        assert_ne!(after.copy.title, before.copy.title);
        assert_eq!((after.setup, after.values), (before.setup, before.values));
        assert!(matches!(after.drawer, Some(NewDrawingDrawerView::Presets(_))));
    }
}
