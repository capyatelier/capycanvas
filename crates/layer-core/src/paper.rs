use crate::authored::{Affine64, Composition, EffectApplication, EffectSpatialReference, OccurrenceHandle, SceneView, SourceTarget};
use crate::{EffectProgram, EffectValue, EffectView, ImageResolution, ResourceLabel, TextureSource};
use std::sync::Arc;

pub const PAPER_PROGRAM: &str = "paper";
pub const RELIEF: &str = "relief";
pub const LIGHT: &str = "light";
pub const THROUGH_PAINT: &str = "through_paint";
const LIGHTING: [&str; 3] = [RELIEF, LIGHT, THROUGH_PAINT];
pub const DEFAULT_PAPER: &str = "drawing_paper";
pub const DEFAULT_PAPER_PPI: f64 = 300.;
const MINIMUM_IMAGE_PPI: f64 = 150.;
const MM_PER_INCH: f64 = 25.4;

pub fn paper_ppi(resolution: Option<ImageResolution>) -> [f64; 2] {
    let ppi = resolution.map_or([DEFAULT_PAPER_PPI; 2], ImageResolution::pixels_per_inch);
    if ppi.iter().all(|v| v.is_finite() && *v > 0.) { ppi } else { [DEFAULT_PAPER_PPI; 2] }
}

pub fn image_ppi(resolution: Option<ImageResolution>) -> [f64; 2] {
    let ppi = paper_ppi(resolution);
    if ppi.iter().all(|v| *v >= MINIMUM_IMAGE_PPI) { ppi } else { [DEFAULT_PAPER_PPI; 2] }
}

pub fn is_paper(program: &EffectProgram) -> bool {
    program.id.as_ref() == PAPER_PROGRAM && program.kind == crate::EffectKind::Generator
}

pub fn is_paper_layer(scene: SceneView<'_>, occurrence: OccurrenceHandle) -> bool {
    scene.effect(occurrence).is_some_and(|effect| is_paper(effect.program))
}

mod import;
pub use import::{CMYK, decode_png, encode_png, image_preview, paper_image};

pub const IMAGE_RELIEF_UM: f32 = 100.;
pub const PREVIEW_SIZE: usize = 96;
pub const PREVIEW_MM: f64 = 16.;
const PREVIEW_EXAGGERATION: f32 = 3.;
const BUILT_IN_PREVIEWS: &[u8] = include_bytes!("../../../assets/filters/paper_previews.gray");

pub struct BuiltInPaper {
    pub id: Arc<str>,
    pub label: ResourceLabel,
    pub texture: TextureSource,
    pub preview: Arc<[u8]>,
}

pub fn built_in_papers() -> &'static [BuiltInPaper] {
    static PAPERS: std::sync::OnceLock<Vec<BuiltInPaper>> = std::sync::OnceLock::new();
    PAPERS.get_or_init(|| crate::bundled_texture_catalog().filters().iter()
        .flat_map(|definition| definition.presets.iter().map(move |preset| (definition, preset)))
        .zip(BUILT_IN_PREVIEWS.as_chunks::<{ PREVIEW_SIZE * PREVIEW_SIZE }>().0)
        .map(|((definition, preset), preview)| BuiltInPaper {
            id: preset.id.clone(),
            label: preset.label.clone(),
            texture: TextureSource::Program(definition.preset(preset).expect("bundled paper")),
            preview: Arc::from(&preview[..]),
        })
        .collect())
}

pub fn preview(heights: &[f32]) -> Vec<u8> {
    let size = 2 * PREVIEW_SIZE;
    assert_eq!(heights.len(), size * size);
    let texel_um = (PREVIEW_MM * 1000. / size as f64) as f32;
    let at = |x: usize, y: usize| heights[y.min(size - 1) * size + x.min(size - 1)];
    let light = { let l = [-0.75f32, -0.45, 0.48]; let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt(); l.map(|v| v / n) };
    let shade = |x: usize, y: usize| {
        let scale = PREVIEW_EXAGGERATION / (2. * texel_um);
        let sx = (at(x + 1, y) - at(x.saturating_sub(1), y)) * scale;
        let sy = (at(x, y + 1) - at(x, y.saturating_sub(1))) * scale;
        let lit = (-sx * light[0] - sy * light[1] + light[2]) / (sx * sx + sy * sy + 1.).sqrt();
        (0.8 * lit / light[2]).clamp(0., 1.)
    };
    (0..PREVIEW_SIZE * PREVIEW_SIZE).map(|i| {
        let (x, y) = (2 * (i % PREVIEW_SIZE), 2 * (i / PREVIEW_SIZE));
        let sum = shade(x, y) + shade(x + 1, y) + shade(x, y + 1) + shade(x + 1, y + 1);
        (sum * 0.25 * 255.).round() as u8
    }).collect()
}

pub fn built_in_paper(id: &str) -> Option<&'static BuiltInPaper> {
    built_in_papers().iter().find(|paper| paper.id.as_ref() == id)
}

pub fn default_texture() -> TextureSource {
    built_in_paper(DEFAULT_PAPER).expect("bundled default paper").texture.clone()
}

pub fn reference(size: [u32; 2], ppi: [f64; 2]) -> EffectSpatialReference {
    let per_mm = ppi.map(|v| v / MM_PER_INCH);
    EffectSpatialReference {
        mapping: Affine64([per_mm[0], 0., 0., per_mm[1], 0., 0.]),
        extent: [f64::from(size[0]) / per_mm[0], f64::from(size[1]) / per_mm[1]],
    }
}

fn column_lengths(mapping: Affine64) -> [f64; 2] {
    let [a, b, c, d, _, _] = mapping.0;
    [a.hypot(b), c.hypot(d)]
}

pub fn scale(spatial: &EffectSpatialReference, ppi: [f64; 2]) -> f64 {
    let [x, y] = column_lengths(spatial.mapping);
    (x * y).sqrt() / ((ppi[0] * ppi[1]).sqrt() / MM_PER_INCH)
}

pub fn with_scale(spatial: &EffectSpatialReference, scale: f64, ppi: [f64; 2]) -> EffectSpatialReference {
    let [a, b, c, d, x, y] = spatial.mapping.0;
    let [first, second] = column_lengths(spatial.mapping);
    let per_mm = scale * (ppi[0] * ppi[1]).sqrt() / MM_PER_INCH;
    let size = [spatial.extent[0] * first, spatial.extent[1] * second];
    EffectSpatialReference {
        mapping: Affine64([a / first * per_mm, b / first * per_mm, c / second * per_mm, d / second * per_mm, x, y]),
        extent: size.map(|v| v / per_mm),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaperSurface {
    pub texture: TextureSource,
    pub mapping: Affine64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaperBinding {
    pub surface: Arc<PaperSurface>,
    pub target_offset: [i64; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperLight {
    pub relief: f32,
    pub angle: f32,
    pub through_paint: f32,
}
impl PaperLight {
    pub fn direction(self) -> [f32; 2] {
        let radians = self.angle.to_radians();
        [radians.cos(), radians.sin()]
    }
}

pub fn light(effect: EffectView<'_>) -> Option<PaperLight> {
    if !is_paper(effect.program) { return None; }
    let number = |key| match effect.value(key)? { EffectValue::Number(v) => Some(*v), _ => None };
    Some(PaperLight { relief: number(RELIEF)? / 100., angle: number(LIGHT)?, through_paint: number(THROUGH_PAINT)? / 100. })
}

pub fn lit_paper(scene: SceneView<'_>) -> Option<(OccurrenceHandle, PaperLight)> {
    let paper = *scene.children(None).last()?;
    let light = light(scene.effect(paper)?)?;
    (light.relief > 0. && scene.includes(paper) && scene.visible(paper)).then_some((paper, light))
}

pub fn separates(scene: SceneView<'_>, paper: OccurrenceHandle) -> bool {
    fn over(scene: SceneView<'_>, parent: Option<OccurrenceHandle>, paper: OccurrenceHandle) -> bool {
        scene.members(parent).filter(|h| *h != paper && scene.includes(*h) && scene.visible(*h)).all(|h| {
            let occurrence = scene.occurrence(h).unwrap();
            !scene.effect(h).is_some_and(|effect| effect.program.kind == crate::EffectKind::Adjustment)
                && scene.clipping_base(h) != Some(paper)
                && match occurrence.blend {
                    crate::LayerBlend::Normal => true,
                    crate::LayerBlend::PassThrough => over(scene, Some(h), paper),
                    _ => false,
                }
        })
    }
    scene.occurrence(paper).is_some_and(|o| o.blend == crate::LayerBlend::Normal && !o.mask.as_ref().is_some_and(|m| m.enabled))
        && scene.effect(paper).is_some_and(|effect| effect.constant_color().is_some()) && over(scene, None, paper)
}

fn splits(light: PaperLight) -> bool { light.relief > 0. && light.through_paint < 1. }

pub fn separate_paper(scene: SceneView<'_>) -> Option<(OccurrenceHandle, PaperLight)> {
    lit_paper(scene).filter(|(paper, light)| splits(*light) && separates(scene, *paper))
}

pub fn same_composite(a: &EffectApplication, b: &EffectApplication) -> bool {
    is_paper(&a.program) && a.program == b.program && a.spatial == b.spatial && a.values.len() == b.values.len()
        && a.program.parameters.iter().zip(a.values.iter().zip(&b.values))
            .all(|(parameter, (x, y))| x == y || LIGHTING.contains(&parameter.key.as_ref()))
        && light(EffectView::new(&a.program, &a.values)).map(splits) == light(EffectView::new(&b.program, &b.values)).map(splits)
}

pub fn surface(scene: SceneView<'_>, occurrence: OccurrenceHandle) -> Option<PaperSurface> {
    let application = scene.effect_application(occurrence)?;
    Some(PaperSurface { texture: texture(application)?.clone(), mapping: application.spatial?.mapping })
}

fn texture(application: &EffectApplication) -> Option<&TextureSource> {
    if !is_paper(&application.program) { return None; }
    application.values.iter().find_map(|value| match value { EffectValue::Texture(texture) => Some(texture), _ => None })
}

pub fn paper_occurrence(scene: SceneView<'_>, from: Option<OccurrenceHandle>) -> Option<OccurrenceHandle> {
    let mut current = from;
    loop {
        let parent = current.and_then(|h| scene.parent(h));
        let siblings = scene.children(parent);
        let start = current.and_then(|h| siblings.iter().position(|s| *s == h)).unwrap_or(0);
        if let Some(found) = siblings[start..].iter().find(|h| scene.effect_application(**h).is_some_and(|a| is_paper(&a.program))) {
            return Some(*found);
        }
        current = Some(parent?);
    }
}

impl crate::Document {
    pub fn bottom_paper(&self) -> Option<OccurrenceHandle> {
        self.scene().children(None).last().copied().filter(|h| is_paper_layer(self.scene(), *h))
    }
    pub(crate) fn validate_papers(&self, previous: &Self) -> Result<(), crate::DocumentError> {
        let scene = self.scene();
        let bottom = self.bottom_paper();
        if let Some(paper) = previous.bottom_paper()
            && scene.includes(paper) && is_paper_layer(scene, paper) && bottom != Some(paper) {
            return Err(crate::DocumentError::ProtectedOccurrence(paper));
        }
        let mask = |document: &Self, h| document.scene().occurrence(h).and_then(|o| o.mask.as_ref()).map(|mask| mask.source);
        for &h in scene.order().iter().filter(|h| is_paper_layer(scene, **h)) {
            let kept = previous.scene().position(h).is_some() && is_paper_layer(previous.scene(), h);
            if !kept && bottom != Some(h) { return Err(crate::DocumentError::ProtectedOccurrence(h)); }
            if mask(self, h).is_some() && (!kept || mask(self, h) != mask(previous, h)) { return Err("Paper can't have a mask".into()); }
        }
        Ok(())
    }
    pub fn paper(&self, target: Option<SourceTarget>) -> PaperSurface {
        let scene = self.scene();
        paper_occurrence(scene, target.and_then(|t| scene.source_owner(t))).and_then(|h| self.paper_surface(h)).unwrap_or_else(|| PaperSurface {
            texture: default_texture(),
            mapping: reference(self.composition().size, paper_ppi(self.composition().resolution)).mapping,
        })
    }
    pub fn paper_surface(&self, occurrence: OccurrenceHandle) -> Option<PaperSurface> {
        surface(self.scene(), occurrence)
    }
    pub fn nearest_paper_binding(&self, target: Option<SourceTarget>) -> Option<PaperBinding> {
        let paper = paper_occurrence(self.scene(), target.and_then(|target| self.target_owner(target)).or(self.working.occurrence))?;
        Some(PaperBinding { surface: Arc::new(self.paper_surface(paper)?), target_offset: target.map_or([0; 2], |target| self.target_offset(target)) })
    }
    pub fn paper_binding(&self, target: SourceTarget) -> PaperBinding {
        PaperBinding { surface: Arc::new(self.paper(Some(target))), target_offset: self.target_offset(target) }
    }
    pub fn set_paper_texture(&mut self, texture: TextureSource) {
        let Some(handle) = paper_occurrence(self.scene(), None).and_then(|h| self.scene().effect_handle(h)) else { return };
        for value in &mut self.artwork.effects.get_mut(handle).expect("listed paper").values {
            if let EffectValue::Texture(current) = value { *current = texture.clone(); }
        }
    }
    pub fn without_paper(mut self) -> Result<Self, crate::DocumentError> {
        let Some(paper) = self.bottom_paper() else { return Ok(self) };
        let effect = self.scene().effect_handle(paper);
        let stack = self.composition().result;
        self.artwork.stacks.get_mut(stack).expect("root stack").entries.retain(|entry| *entry != paper);
        self.artwork.occurrences.remove(paper);
        if let Some(effect) = effect { self.artwork.effects.remove(effect); }
        let working = self.working.clone();
        let mut document = Self::from_artwork(self.artwork)?;
        document.working = working;
        Ok(document)
    }
    pub fn set_paper_color(&mut self, color: crate::color::RgbColor) {
        let Some(handle) = paper_occurrence(self.scene(), None).and_then(|h| self.scene().effect_handle(h)) else { return };
        for value in &mut self.artwork.effects.get_mut(handle).expect("listed paper").values {
            if let EffectValue::Color(current) = value { *current = crate::color::RgbColor { rgba: [color.rgba[0], color.rgba[1], color.rgba[2], 1.], ..color }; }
        }
    }
    pub fn set_paper_relief(&mut self, percent: f32) {
        let Some(handle) = paper_occurrence(self.scene(), None).and_then(|h| self.scene().effect_handle(h)) else { return };
        let application = self.artwork.effects.get_mut(handle).expect("listed paper");
        let Some(index) = application.program.parameters.iter().position(|p| p.key.as_ref() == RELIEF) else { return };
        application.values[index] = EffectValue::Number(percent);
    }
    pub fn fit_papers_to_resolution(&mut self) {
        let ppi = paper_ppi(self.composition().resolution);
        let papers: Vec<_> = self.artwork.effects.iter().filter(|(_, _, a)| is_paper(&a.program)).map(|(h, _, _)| h).collect();
        for handle in papers {
            if let Some(spatial) = &mut self.artwork.effects.get_mut(handle).expect("listed paper").spatial {
                *spatial = with_scale(spatial, 1., ppi);
            }
        }
    }
}

impl EffectApplication {
    pub fn in_composition(program: Arc<EffectProgram>, values: Vec<EffectValue>, composition: &Composition) -> Self {
        let mut application = Self::new(program, values, composition.size);
        if is_paper(&application.program) {
            application.spatial = Some(reference(composition.size, paper_ppi(composition.resolution)));
        }
        application
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, DocumentNames, ResolutionUnit, authored::PortableId};
    fn document() -> Document {
        Document::new(PortableId::random(), 300, 200, DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
    }
    #[test]
    fn drawings_map_paper_at_their_resolution_and_image_files_below_print_count_as_print() {
        assert_eq!(paper_ppi(None), [300.; 2]);
        for ppi in [72, 96, 144, 150, 600] {
            assert_eq!(paper_ppi(Some(ImageResolution::ppi(ppi))), [f64::from(ppi); 2]);
        }
        let metric = ImageResolution { unit: ResolutionUnit::Centimetre, density: [[118, 1]; 2] };
        assert!((paper_ppi(Some(metric))[0] - 299.72).abs() < 1e-9);
        assert_eq!(image_ppi(None), [300.; 2]);
        assert_eq!(image_ppi(Some(ImageResolution::ppi(72))), [300.; 2]);
        assert_eq!(image_ppi(Some(ImageResolution::ppi(96))), [300.; 2]);
        assert_eq!(image_ppi(Some(ImageResolution::ppi(150))), [150.; 2]);
        assert_eq!(image_ppi(Some(ImageResolution::ppi(600))), [600.; 2]);
    }
    #[test]
    fn scale_reads_the_mapping_against_the_paper_resolution() {
        let spatial = reference([300, 200], [300.; 2]);
        assert!((scale(&spatial, [300.; 2]) - 1.).abs() < 1e-12);
        assert!((scale(&spatial, [600.; 2]) - 0.5).abs() < 1e-12);
        let resampled = EffectSpatialReference { mapping: Affine64([2., 0., 0., 2., 0., 0.]).compose(spatial.mapping), ..spatial };
        assert!((scale(&resampled, [300.; 2]) - 2.).abs() < 1e-12);
        let stretched = EffectSpatialReference { mapping: Affine64([4., 0., 0., 1., 0., 0.]).compose(spatial.mapping), ..spatial };
        assert!((scale(&stretched, [300.; 2]) - 2.).abs() < 1e-12);
        let uniform = with_scale(&stretched, 1.5, [300.; 2]);
        assert!((scale(&uniform, [300.; 2]) - 1.5).abs() < 1e-12);
        let [a, _, _, d, _, _] = uniform.mapping.0;
        assert!((a - d).abs() < 1e-12);
    }
    #[test]
    fn scale_keeps_quarter_turns_flips_and_origin() {
        let spatial = EffectSpatialReference { mapping: Affine64([0., -2., 3., 0., 7., 9.]), extent: [10., 20.] };
        let scaled = with_scale(&spatial, 1., [254.; 2]);
        assert_eq!(scaled.mapping.0, [0., -10., 10., 0., 7., 9.]);
        assert_eq!(scaled.extent, [2., 6.]);
    }
    #[test]
    fn new_drawings_start_on_the_default_paper_at_real_size() {
        let mut doc = document();
        let surface = doc.paper(doc.working.target);
        assert_eq!(surface.texture, default_texture());
        assert!((surface.mapping.0[0] - 300. / 25.4).abs() < 1e-12);
        doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().resolution = Some(ImageResolution::ppi(600));
        doc.fit_papers_to_resolution();
        assert!((doc.paper(doc.working.target).mapping.0[0] - 600. / 25.4).abs() < 1e-12);
    }
    #[test]
    fn lighting_edits_keep_the_composite_and_only_a_visible_raised_paper_is_lit() {
        use crate::{Edit, authored::RecordChange};
        let mut doc = document();
        let paper = doc.bottom_paper().unwrap();
        assert_eq!(light(doc.scene().effect(paper).unwrap()), Some(PaperLight { relief: 0., angle: 225., through_paint: 1. }));
        assert_eq!(lit_paper(doc.scene()), None);
        let handle = doc.scene().effect_handle(paper).unwrap();
        let edit = |doc: &Document, values: &[(&str, EffectValue)]| {
            let mut application = doc.artwork.effects.get(handle).unwrap().clone();
            let mut draft = crate::EffectInstance { program: application.program.clone(), values: application.values };
            for (key, value) in values { draft.set(key, value.clone()).unwrap(); }
            application.values = draft.values;
            Edit::Effect(RecordChange::replace(&doc.artwork.effects, handle, Some(application)).unwrap())
        };
        let lighting = edit(&doc, &[(RELIEF, EffectValue::Number(150.)), (LIGHT, EffectValue::Number(90.))]);
        assert!(!lighting.changes_image(&doc));
        doc.apply(lighting).unwrap();
        let through = edit(&doc, &[(THROUGH_PAINT, EffectValue::Number(40.))]);
        assert!(through.changes_image(&doc), "paint starts covering the texture, so the paper leaves the composite");
        doc.apply(through).unwrap();
        assert!(!edit(&doc, &[(THROUGH_PAINT, EffectValue::Number(60.)), (RELIEF, EffectValue::Number(80.))]).changes_image(&doc));
        assert!(edit(&doc, &[(RELIEF, EffectValue::Number(0.))]).changes_image(&doc), "a flat paper returns to the composite");
        assert!(edit(&doc, &[(THROUGH_PAINT, EffectValue::Number(100.))]).changes_image(&doc));
        let (lit, light) = lit_paper(doc.scene()).unwrap();
        assert_eq!((lit, light), (paper, PaperLight { relief: 1.5, angle: 90., through_paint: 0.4 }));
        let [x, y] = light.direction();
        assert!(x.abs() < 1e-6 && (y - 1.).abs() < 1e-6, "90° points down the y-down document");
        let recolor = edit(&doc, &[("color", EffectValue::Color(crate::color::RgbColor::BLACK))]);
        assert!(recolor.changes_image(&doc));
        let mut hidden = doc.scene().occurrence(paper).unwrap().clone();
        hidden.visible = false;
        doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, paper, Some(hidden)).unwrap())).unwrap();
        assert_eq!(lit_paper(doc.scene()), None);
    }
    #[test]
    fn paper_separates_from_paint_only_under_normal_unclipped_layers() {
        use crate::{Edit, LayerBlend, authored::RecordChange};
        let mut doc = document();
        let paper = doc.bottom_paper().unwrap();
        let ink = doc.scene().order()[0];
        assert!(separates(doc.scene(), paper));
        let mut multiply = doc.scene().occurrence(ink).unwrap().clone();
        multiply.blend = LayerBlend::Multiply;
        let edit = Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, ink, Some(multiply.clone())).unwrap());
        let mut multiplied = doc.clone();
        multiplied.apply(edit).unwrap();
        assert!(!separates(multiplied.scene(), paper), "Multiply reads the paper's color");
        multiply.visible = false;
        let hidden = Edit::Occurrence(RecordChange::replace(&multiplied.artwork.occurrences, ink, Some(multiply)).unwrap());
        multiplied.apply(hidden).unwrap();
        assert!(separates(multiplied.scene(), paper), "a hidden layer reads nothing");
        assert_eq!(separate_paper(doc.scene()), None, "a flat paper has no light to separate");
        let handle = doc.scene().effect_handle(paper).unwrap();
        let mut application = doc.artwork.effects.get(handle).unwrap().clone();
        let mut draft = crate::EffectInstance { program: application.program.clone(), values: application.values };
        draft.set(RELIEF, EffectValue::Number(100.)).unwrap();
        application.values = draft.values.clone();
        doc.apply(Edit::Effect(RecordChange::replace(&doc.artwork.effects, handle, Some(application.clone())).unwrap())).unwrap();
        assert_eq!(separate_paper(doc.scene()), None, "relief that fully shows through paint needs no separation");
        draft.set(THROUGH_PAINT, EffectValue::Number(30.)).unwrap();
        application.values = draft.values;
        doc.apply(Edit::Effect(RecordChange::replace(&doc.artwork.effects, handle, Some(application)).unwrap())).unwrap();
        assert_eq!(separate_paper(doc.scene()).map(|(h, light)| (h, light.through_paint)), Some((paper, 0.3)));
    }
    #[test]
    fn paper_order_is_protected_across_layer_edits_and_history() {
        use crate::{Edit, LayerBlend, authored::{RecordChange, Stack}};
        let mut doc = document();
        let paper = doc.bottom_paper().unwrap();
        let ink = doc.scene().order()[0];
        let original = doc.clone();
        assert!(doc.move_occurrence_edit(paper, 0).is_err());
        assert!(doc.group_layers_edit(&[ink, paper], LayerBlend::Normal, "Group").is_err());
        assert!(doc.validate_duplicate_layers(&[paper]).is_err());
        assert!(doc.duplicate_layers_edit(&[ink, paper]).is_err());
        let root = doc.composition().result;
        let reorder = Edit::Stack(RecordChange::replace(&doc.artwork.stacks, root, Some(Stack { entries: vec![paper, ink] })).unwrap());
        assert!(doc.apply(reorder).is_err());
        assert_eq!(doc.artwork, original.artwork);
        let undo = doc.apply(doc.delete_layers_edit(&[paper]).unwrap()).unwrap();
        assert_eq!(doc.bottom_paper(), None);
        let redo = doc.apply(undo).unwrap();
        assert_eq!(doc.artwork, original.artwork);
        doc.apply(redo).unwrap();
        assert_eq!(doc.bottom_paper(), None);
    }
    #[test]
    fn edits_never_give_paper_a_mask() {
        use crate::{CoverageSnapshot, Edit, authored::{MaskUse, RecordChange}, operation_test_support as f};
        let mut doc = f::document([32; 2], &["Ink"]);
        let masked = |doc: &Document, owner| {
            let snapshot = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), [32; 2], [0; 2]);
            let coverage = RecordChange::insert(&doc.artwork.coverage, snapshot.source);
            let mut occurrence = doc.scene().occurrence(owner).unwrap().clone();
            occurrence.mask = Some(MaskUse { source: coverage.handle, ..snapshot.use_ });
            Edit::Batch(vec![Edit::Coverage(coverage), Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, owner, Some(occurrence)).unwrap())])
        };
        let original = doc.artwork.clone();
        let paper = doc.bottom_paper().unwrap();
        assert_eq!(doc.apply(masked(&doc, paper)), Err(crate::DocumentError::InvalidLayerOperation("Paper can't have a mask")));
        assert_eq!(doc.artwork, original);
        doc.apply(masked(&doc, f::id(&doc, "Ink"))).unwrap();
    }
    #[test]
    fn unmasked_fills_draw_above_without_entering_groups_and_own_masks_take_priority() {
        use crate::{DrawingRefusal, LayerBlend, operation_test_support as f};
        for program in ["paper", "solid_color", "gradient_fill"] {
            let mut doc = f::document([32; 2], &["Top", "Nearest"]);
            let fill = doc.bottom_paper().unwrap();
            if program != "paper" { f::effect(&mut doc, "Paper", program); }
            let nearest = f::id(&doc, "Nearest");
            doc.apply(doc.select_occurrence_edit(fill).unwrap()).unwrap();
            assert_eq!(doc.drawing_target(), doc.scene().source_target(nearest), "{program}");
            assert_eq!(doc.working.occurrence, Some(fill));
            doc.artwork.occurrences.get_mut(nearest).unwrap().locked = true;
            assert_eq!(doc.try_drawing_target(), Err(DrawingRefusal::BaseLocked));
            doc.artwork.occurrences.get_mut(nearest).unwrap().locked = false;
            let undo = doc.apply(doc.group_layers_edit(&[nearest], LayerBlend::Normal, "Group").unwrap()).unwrap();
            assert_eq!(doc.try_drawing_target(), Err(DrawingRefusal::Group));
            doc.apply(undo).unwrap();
            assert_eq!(doc.drawing_target(), doc.scene().source_target(nearest));
            let undo = doc.apply(doc.delete_layers_edit(&[f::id(&doc, "Top"), nearest]).unwrap()).unwrap();
            assert_eq!(doc.try_drawing_target(), Err(DrawingRefusal::NoLayer));
            if program == "paper" { continue; }
            let coverage = f::add_mask(&mut doc, fill, [32; 2], [0; 2]);
            assert_eq!(doc.drawing_target(), Some(SourceTarget::Coverage(coverage)));
            doc.apply(undo).unwrap();
            assert_eq!(doc.drawing_target(), Some(SourceTarget::Coverage(coverage)));
        }
    }
    #[test]
    fn strokes_use_the_nearest_paper_at_or_below_their_layer() {
        let doc = document();
        let scene = doc.scene();
        let [ink, paper] = [scene.order()[0], scene.order()[1]];
        assert_eq!(paper_occurrence(scene, Some(ink)), Some(paper));
        assert_eq!(paper_occurrence(scene, Some(paper)), Some(paper));
        assert_eq!(paper_occurrence(scene, None), Some(paper));
        let target = doc.drawing_target().unwrap();
        let binding = doc.nearest_paper_binding(Some(target)).unwrap();
        assert_eq!(binding, doc.paper_binding(target));
        let mut changed = doc.clone();
        changed.set_paper_texture(built_in_paper("rough").unwrap().texture.clone());
        assert_ne!(changed.nearest_paper_binding(Some(target)).as_ref(), Some(&binding));
        assert_eq!(doc.nearest_paper_binding(Some(target)).as_ref(), Some(&binding));
        assert_eq!(changed.without_paper().unwrap().nearest_paper_binding(Some(target)), None);
    }
}
