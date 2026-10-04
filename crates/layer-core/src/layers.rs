use super::*;
use crate::authored::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawingRefusal {
    NoLayer,
    NonAffine,
    Locked,
    BaseLocked,
    Group,
    SelectionLayer,
    EffectWithoutBase,
    Fill,
    /// Content tools draw on artwork, and the layer's mask is being edited.
    Mask,
    EffectMask,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoverageSnapshot {
    pub target: CoverageHandle,
    pub source: CoverageSource,
    pub use_: MaskUse,
}
impl CoverageSnapshot {
    pub fn reveal_all(target: CoverageHandle, domain: [u32; 2], translation: Point) -> Self {
        Self {
            target,
            source: CoverageSource { domain, raster: Default::default(), initial: None, default_coverage: 1., operations: Arc::default() },
            use_: MaskUse { source: target, enabled: true, linked: true, inverted: false, translation, placement: Projective::IDENTITY },
        }
    }
    pub fn validate(&self) -> Result<(), DocumentError> {
        self.source.validate()?;
        self.use_.validate()?;
        if self.target != self.use_.source {
            return Err(DocumentError::InvalidLayerOperation("Coverage target differs from its use"));
        }
        Ok(())
    }
}
impl CoverageSource {
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.domain.contains(&0)
            || self.domain.iter().any(|v| *v > MAX_EXTENT)
            || !self.default_coverage.is_finite()
            || !(0. ..=1.).contains(&self.default_coverage)
        {
            return Err(DocumentError::InvalidLayerOperation("Invalid mask value"));
        }
        if let Some(initial) = &self.initial {
            initial.validate()?;
        }
        for op in self.operations.iter() {
            if !matches!(op.kind, RasterOperationKind::Transform(_)) {
                return Err(DocumentError::InvalidLayerOperation("Unsupported mask operation"));
            }
            op.validate()?;
        }
        Ok(())
    }
}
impl MaskUse {
    pub fn validate(&self) -> Result<(), DocumentError> {
        if !self.translation.x.is_finite() || !self.translation.y.is_finite() || self.placement.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation("Invalid mask value"));
        }
        Ok(())
    }
    pub fn geometry_in_parent(&self, owner: &Occurrence) -> ImageTransform {
        if self.linked {
            let pre = self
                .placement
                .then(Projective::from_affine(Affine::translation(Point {
                    x: self.translation.x - owner.translation.x,
                    y: self.translation.y - owner.translation.y,
                })))
                .unwrap_or(Projective([f32::NAN; 9]));
            ImageTransform {
                placement: owner
                    .placement
                    .post(Projective::from_affine(Affine::translation(owner.translation)))
                    .unwrap_or_else(|| LayerPlacement::from_projective(Projective([f32::NAN; 9]))),
                source_from_owner: Some(pre.inverse().unwrap_or(Projective([f32::NAN; 9]))),
                keep_source: false,
            }
        } else {
            ImageTransform {
                placement: LayerPlacement::from_projective(
                    self.placement
                        .then(Projective::from_affine(Affine::translation(self.translation)))
                        .unwrap_or(Projective([f32::NAN; 9])),
                ),
                ..Default::default()
            }
        }
    }
    pub fn set_linked(&mut self, linked: bool, owner: &Occurrence) -> Result<(), DocumentError> {
        if self.linked == linked {
            return Ok(());
        }
        let invalid = || DocumentError::InvalidLayerOperation("Apply the layer transform before changing mask linkage");
        if owner.placement.as_affine().is_none() || self.geometry_in_parent(owner).as_affine().is_none() {
            return Err(invalid());
        }
        let current = self.geometry_in_parent(owner).projective().ok_or_else(invalid)?;
        let mut next = self.clone();
        next.linked = linked;
        next.placement = Projective::IDENTITY;
        let rest = next.geometry_in_parent(owner).projective().and_then(Projective::inverse).ok_or_else(invalid)?;
        next.placement = current.then(rest).ok_or_else(invalid)?;
        next.validate()?;
        *self = next;
        Ok(())
    }
}

pub fn target_offset(scene: SceneView<'_>, target: SourceTarget) -> Point {
    scene.target_offset(target)
}
pub fn target_geometry(scene: SceneView<'_>, target: SourceTarget) -> ImageTransform {
    scene.target_geometry(target)
}
pub fn affine_edit_transform(scene: SceneView<'_>, target: SourceTarget) -> Option<Affine> {
    scene.target_geometry(target).as_affine()
}
pub fn isolated_scope(scene: SceneView<'_>, mut parent: Option<OccurrenceHandle>) -> Option<OccurrenceHandle> {
    while let Some(h) = parent {
        if scene.includes(h) && !scene.occurrence(h).is_some_and(|o| o.passes_through()) {
            break;
        }
        parent = scene.evaluation_parent(h);
    }
    parent
}
pub fn descends_from(scene: SceneView<'_>, handle: OccurrenceHandle, scope: Option<OccurrenceHandle>) -> bool {
    let Some(scope) = scope else {
        return true;
    };
    let mut parent = scene.parent(handle);
    while let Some(h) = parent {
        if h == scope {
            return true;
        }
        parent = scene.parent(h);
    }
    false
}
pub fn layer_is_visible(scene: SceneView<'_>, handle: OccurrenceHandle) -> bool {
    scene.visible(handle)
}
pub fn backdrop_layers(scene: SceneView<'_>, handle: OccurrenceHandle) -> Vec<OccurrenceHandle> {
    let mut levels = vec![(scene.evaluation_parent(handle), handle)];
    while let Some(&(Some(group), _)) = levels.last() {
        if !scene.occurrence(group).is_some_and(|o| o.passes_through()) {
            break;
        }
        levels.push((scene.evaluation_parent(group), group));
    }
    scene
        .order()
        .iter()
        .copied()
        .filter(|h| {
            if !scene.includes(*h) {
                return false;
            }
            let mut root = *h;
            loop {
                if let Some(&(_, own)) = levels.iter().find(|(p, _)| *p == scene.evaluation_parent(root)) {
                    return scene.position(root) > scene.position(own);
                }
                let Some(parent) = scene.evaluation_parent(root) else {
                    return false;
                };
                root = parent;
            }
        })
        .collect()
}
pub fn composite_input_scope(scene: SceneView<'_>, handle: OccurrenceHandle) -> Option<OccurrenceHandle> {
    if scene.effective_clipped(handle) { scene.evaluation_parent(handle) } else { isolated_scope(scene, scene.evaluation_parent(handle)) }
}
pub fn composite_input_layers(scene: SceneView<'_>, handle: OccurrenceHandle) -> Vec<OccurrenceHandle> {
    let Some(o) = scene.occurrence(handle) else {
        return Vec::new();
    };
    if o.kind() == LayerKind::Group {
        return scene
            .order()
            .iter()
            .copied()
            .filter(|h| {
                scene.includes(*h) && scene.occurrence(*h).is_some_and(|o| o.is_artwork()) && descends_from(scene, *h, Some(handle))
            })
            .collect();
    }
    if !scene.effect(handle).is_some_and(|e| e.program.kind == EffectKind::Adjustment) {
        return Vec::new();
    }
    if !scene.effective_clipped(handle) {
        let mut input = backdrop_layers(scene, handle);
        input.retain(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork()));
        return input;
    }
    let siblings = scene.members(scene.evaluation_parent(handle)).collect::<Vec<_>>();
    let Some(start) = siblings.iter().position(|h| *h == handle) else {
        return Vec::new();
    };
    let end = siblings[start + 1..]
        .iter()
        .position(|h| {
            scene.occurrence(*h).is_some_and(|o| o.is_artwork() && !scene.effective_clipped(*h))
        })
        .map_or(siblings.len(), |i| start + 1 + i + 1);
    let roots = &siblings[start + 1..end];
    scene
        .order()
        .iter()
        .copied()
        .filter(|h| {
            scene.includes(*h)
                && scene.occurrence(*h).is_some_and(|o| o.is_artwork())
                && (roots.contains(h) || roots.iter().any(|r| descends_from(scene, *h, Some(*r))))
        })
        .collect()
}
/// How a layer combines with the pixels below it. The discriminant is the
/// renderer's mode code and the flat `ALL` order; documents store the name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u32)]
pub enum LayerBlend {
    #[default]
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Add = 3,
    Overlay = 4,
    SoftLight = 5,
    Color = 6,
    Darken = 7,
    Lighten = 8,
    ColorBurn = 9,
    LinearBurn = 10,
    ColorDodge = 11,
    HardLight = 12,
    VividLight = 13,
    LinearLight = 14,
    PinLight = 15,
    HardMix = 16,
    Difference = 17,
    Exclusion = 18,
    Subtract = 19,
    Divide = 20,
    Hue = 21,
    Saturation = 22,
    Luminosity = 23,
    /// Groups only: the group's layers composite onto the layers below it, as
    /// if they were not grouped, and its opacity and mask fade between that
    /// result and what lies below.
    PassThrough = 24,
}
/// The operands a blend mode is defined on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendRange {
    /// Defined for extended float values.
    Unbounded,
    /// Defined on [0, 1]; operands are clamped, and float documents do not
    /// offer the mode.
    Unit,
}
/// The values a document's layers combine on. Layer pixels stay linear; in
/// Perceptual documents the composite holds the document's encoded values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BlendSpace {
    #[default]
    Linear,
    Perceptual,
}
impl BlendSpace {
    pub const ALL: [Self; 2] = [Self::Perceptual, Self::Linear];
    pub fn label(self) -> &'static str {
        match self {
            Self::Perceptual => "Perceptual",
            Self::Linear => "Linear light",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Perceptual => "Like Photoshop and Clip Studio Paint",
            Self::Linear => "Physically based",
        }
    }
    /// Why a document of this depth cannot blend perceptually.
    pub fn unavailable_reason(depth: color::SampleDepth) -> Option<&'static str> {
        depth.is_float().then_some("Float documents blend in linear light")
    }
    /// The space a document of `depth` uses when this one is chosen.
    pub fn for_depth(self, depth: color::SampleDepth) -> Self {
        if depth.is_float() { Self::Linear } else { self }
    }
    /// Premultiplied linear `rgba` as the composite of this space holds it,
    /// with the transfer curve of `rgb`.
    pub fn composite(self, rgb: color::RgbSpace, rgba: [f32; 4]) -> [f32; 4] {
        if self == Self::Linear || rgba[3] <= 0. {
            return rgba;
        }
        let a = rgba[3];
        let encode = |c: f32| (rgb.encode(f64::from(c / a)) * f64::from(a)) as f32;
        [encode(rgba[0]), encode(rgba[1]), encode(rgba[2]), a]
    }
}
impl LayerBlend {
    pub const ALL: [Self; 25] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Add,
        Self::Overlay,
        Self::SoftLight,
        Self::Color,
        Self::Darken,
        Self::Lighten,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::ColorDodge,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Luminosity,
        Self::PassThrough,
    ];
    /// Menu groups in Photoshop's order: Pass Through and Normal, darken,
    /// lighten, contrast, inversion and component modes.
    pub const MENU: [&'static [Self]; 6] = [
        &[Self::PassThrough, Self::Normal],
        &[Self::Darken, Self::Multiply, Self::ColorBurn, Self::LinearBurn],
        &[Self::Lighten, Self::Screen, Self::ColorDodge, Self::Add],
        &[Self::Overlay, Self::SoftLight, Self::HardLight, Self::VividLight, Self::LinearLight, Self::PinLight, Self::HardMix],
        &[Self::Difference, Self::Exclusion, Self::Subtract, Self::Divide],
        &[Self::Hue, Self::Saturation, Self::Color, Self::Luminosity],
    ];
    pub fn code(self) -> u32 {
        self as u32
    }
    pub fn from_code(code: u32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }
    pub fn range(self) -> BlendRange {
        match self {
            Self::Overlay
            | Self::SoftLight
            | Self::HardLight
            | Self::ColorBurn
            | Self::ColorDodge
            | Self::VividLight
            | Self::HardMix
            | Self::Exclusion => BlendRange::Unit,
            _ => BlendRange::Unbounded,
        }
    }
    /// Whether a layer of this kind, in a document of this depth, offers the
    /// mode in its menus.
    pub fn offered(self, kind: LayerKind, float: bool) -> bool {
        (!float || self.range() == BlendRange::Unbounded) && (self != Self::PassThrough || kind == LayerKind::Group)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Add => "Add",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::Color => "Color",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::ColorDodge => "Color Dodge",
            Self::HardLight => "Hard Light",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Subtract => "Subtract",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Luminosity => "Luminosity",
            Self::PassThrough => "Pass Through",
        }
    }
}
impl From<BrushBlendMode> for LayerBlend {
    fn from(mode: BrushBlendMode) -> Self {
        match mode {
            BrushBlendMode::Normal => Self::Normal,
            BrushBlendMode::Multiply => Self::Multiply,
            BrushBlendMode::Screen => Self::Screen,
            BrushBlendMode::Add => Self::Add,
            BrushBlendMode::Subtract => Self::Subtract,
            BrushBlendMode::Darken => Self::Darken,
            BrushBlendMode::Lighten => Self::Lighten,
            BrushBlendMode::Overlay => Self::Overlay,
        }
    }
}

/// Transient raster commands retain source coverage until their GPU submission.
/// Raster revisions own the resulting pixels, undo states, and recovery data.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterOperation {
    /// Figure/gradient coordinates to local pixels. Coverage is already local.
    pub placement: Affine,
    pub coverage: CoverageSnapshot,
    pub kind: RasterOperationKind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum RasterOperationKind {
    ApplyMask,
    /// Erases where the coverage is set: ApplyMask with complemented coverage.
    /// Alpha-locked content keeps its transparency, so nothing changes.
    Erase {
        alpha_locked: bool,
    },
    Transform(crate::ImageTransform),
    Figure(Figure),
    Fill {
        color: [f32; 4],
        alpha_locked: bool,
    },
    Gradient {
        start: Point,
        end: Point,
        colors: [[f32; 4]; 2],
        radial: bool,
        alpha_locked: bool,
    },
    Bake {
        scene: Arc<SceneSnapshot>,
        scope: SceneScope,
        offset: Point,
    },
    FrequencyDetail {
        scene: Arc<SceneSnapshot>,
        scope: SceneScope,
        offset: Point,
        low: PaintHandle,
    },
}
impl RasterOperation {
    /// Conservative affected area in layer coordinates. Inverted coverage and
    /// applying a mask can change pixels outside the selection's geometry.
    pub fn bounds(&self, extent: [u32; 2]) -> Rect {
        let mut bounds = match &self.kind {
            RasterOperationKind::Figure(figure) => self.placement.bounds(figure.bounds()),
            RasterOperationKind::Bake { scene, scope, offset } | RasterOperationKind::FrequencyDetail { scene, scope, offset, .. } => {
                crate::merge::bake_bounds(scene, scope, *offset, extent)
            }
            _ => Rect::from_extent(extent),
        };
        if self.kind != RasterOperationKind::ApplyMask
            && self.coverage.source.default_coverage == 0.0
            && !self.coverage.use_.inverted
            && self.coverage.source.raster.is_empty()
            && let Some(selection) = &self.coverage.source.initial
            && !selection.inverted
        {
            let selection = selection.translated(self.coverage.use_.translation).bounds();
            bounds.min.x = bounds.min.x.max(selection.min.x);
            bounds.min.y = bounds.min.y.max(selection.min.y);
            bounds.max.x = bounds.max.x.min(selection.max.x);
            bounds.max.y = bounds.max.y.min(selection.max.y);
        }
        if let RasterOperationKind::Transform(transform) = &self.kind { transform.affected_bounds(bounds) } else { bounds }
    }
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.placement.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation("Invalid paint operation placement"));
        }
        if !self.coverage.source.operations.is_empty() && self.kind != RasterOperationKind::ApplyMask {
            return Err(DocumentError::InvalidLayerOperation("Nested coverage operations"));
        }
        self.coverage.validate()?;
        if self.coverage.source.initial.as_ref().is_some_and(|s| s.affine.inverse().is_none()) {
            return Err(DocumentError::InvalidLayerOperation("Invalid selection transform"));
        }
        // Paint is straight linear RGB, like BrushSnapshot. Portable colors can
        // leave the document gamut and HDR can exceed reference white. Only
        // coverage is a unit interval; native storage owns quantization/range.
        let color_ok = |c: &[f32; 4]| c.iter().all(|v| v.is_finite()) && (0.0..=1.0).contains(&c[3]);
        let valid = match &self.kind {
            RasterOperationKind::ApplyMask | RasterOperationKind::Erase { .. } => self.placement == Affine::IDENTITY,
            RasterOperationKind::Bake { offset, .. } => self.placement == Affine::IDENTITY && offset.x.is_finite() && offset.y.is_finite(),
            RasterOperationKind::FrequencyDetail { offset, .. } => {
                self.placement == Affine::IDENTITY
                    && offset.x.is_finite()
                    && offset.y.is_finite()
                    && self.coverage.source.default_coverage == 1.
                    && self.coverage.source.initial.is_none()
                    && self.coverage.source.raster.is_empty()
            }
            RasterOperationKind::Transform(transform) => {
                self.placement == Affine::IDENTITY
                    && transform.validate().is_ok()
                    && self.coverage.source.raster.is_empty()
                    && self.coverage.use_.translation == Point::default()
                    && self.coverage.use_.enabled
                    && !self.coverage.use_.inverted
                    && if self.coverage.source.initial.is_some() {
                        self.coverage.source.default_coverage == 0.
                    } else {
                        self.coverage.source.default_coverage == 1.
                    }
            }
            RasterOperationKind::Figure(figure) => figure.valid(),
            RasterOperationKind::Fill { color, .. } => color_ok(color),
            RasterOperationKind::Gradient { start, end, colors, .. } => {
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let length2 = dx * dx + dy * dy;
                [start.x, start.y, end.x, end.y].iter().all(|v| v.is_finite())
                    && length2.is_finite()
                    && length2 >= 0.000001
                    && colors.iter().all(color_ok)
            }
        };
        if valid { Ok(()) } else { Err(DocumentError::InvalidLayerOperation("Invalid paint operation")) }
    }
}

impl Document {
    pub fn ordered_layers(&self) -> &[OccurrenceHandle] {
        self.scene().order()
    }
    pub fn layer_roots(&self, selected: &BTreeSet<OccurrenceHandle>) -> Vec<OccurrenceHandle> {
        let scene = self.scene();
        scene
            .order()
            .iter()
            .copied()
            .filter(|h| selected.contains(h))
            .filter(|h| {
                let mut parent = scene.parent(*h);
                while let Some(p) = parent {
                    if selected.contains(&p) {
                        return false;
                    }
                    parent = scene.parent(p);
                }
                true
            })
            .collect()
    }
    pub fn layer_subtrees(&self, roots: &[OccurrenceHandle]) -> BTreeSet<OccurrenceHandle> {
        let scene = self.scene();
        scene.order().iter().copied().filter(|h| roots.contains(h) || roots.iter().any(|r| descends_from(scene, *h, Some(*r)))).collect()
    }
    pub fn clipping_base(&self, id: OccurrenceHandle) -> Option<OccurrenceHandle> {
        let scene = self.scene();
        let siblings = scene.children(scene.parent(id));
        let index = siblings.iter().position(|h| *h == id)?;
        siblings[index + 1..]
            .iter()
            .copied()
            .find(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork() && !o.clipped))
            .filter(|h| scene.occurrence(*h).is_some_and(|o| o.kind() == LayerKind::Paint))
    }
    pub fn clipping_stack_top(&self, id: OccurrenceHandle) -> Option<OccurrenceHandle> {
        let scene = self.scene();
        scene.occurrence(id)?;
        let siblings = scene.children(scene.parent(id));
        let index = siblings.iter().position(|h| *h == id)?;
        Some(
            siblings[..index]
                .iter()
                .rev()
                .copied()
                .filter(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork()))
                .take_while(|h| scene.occurrence(*h).is_some_and(|o| o.clipped))
                .last()
                .unwrap_or(id),
        )
    }
    pub fn target_owner(&self, target: SourceTarget) -> Option<OccurrenceHandle> {
        self.scene().source_owner(target)
    }
    pub fn active_target(&self) -> Option<SourceTarget> {
        self.working.target
    }
    pub fn drawing_target(&self) -> Option<SourceTarget> {
        self.try_drawing_target().ok()
    }
    pub fn try_drawing_target(&self) -> Result<SourceTarget, DrawingRefusal> {
        let scene = self.scene();
        let mut id = self.working.occurrence.ok_or(DrawingRefusal::NoLayer)?;
        let mut occurrence = scene.occurrence(id).ok_or(DrawingRefusal::NoLayer)?;
        if self.is_locked(id) {
            return Err(DrawingRefusal::Locked);
        }
        if let Some(mask) = &occurrence.mask
            && (self.working.target == Some(SourceTarget::Coverage(mask.source)) || occurrence.kind() == LayerKind::Effect)
        {
            let target = SourceTarget::Coverage(mask.source);
            self.validate_content_write(target)?;
            return Ok(target);
        }
        while occurrence.kind() == LayerKind::Effect {
            if scene.effect(id).is_some_and(|e| e.program.kind == EffectKind::Generator) {
                return Err(DrawingRefusal::Fill);
            }
            id = if occurrence.clipped {
                self.clipping_base(id)
            } else {
                let siblings = scene.children(scene.parent(id));
                let index = siblings.iter().position(|h| *h == id).ok_or(DrawingRefusal::NoLayer)?;
                siblings[index + 1..].iter().copied().find(|h| scene.occurrence(*h).is_some_and(|o| o.is_artwork()))
            }
            .ok_or(DrawingRefusal::EffectWithoutBase)?;
            occurrence = scene.occurrence(id).ok_or(DrawingRefusal::NoLayer)?;
        }
        match occurrence.kind() {
            LayerKind::Paint if self.is_locked(id) => Err(DrawingRefusal::BaseLocked),
            LayerKind::Paint => {
                let target = scene.source_target(id).ok_or(DrawingRefusal::NoLayer)?;
                self.validate_content_write(target)?;
                Ok(target)
            }
            LayerKind::Group => Err(DrawingRefusal::Group),
            LayerKind::Selection => Err(DrawingRefusal::SelectionLayer),
            LayerKind::Effect => Err(DrawingRefusal::EffectWithoutBase),
        }
    }
    pub fn drawing_content(&self) -> Option<SourceTarget> {
        self.try_drawing_content().ok()
    }
    pub fn try_drawing_content(&self) -> Result<SourceTarget, DrawingRefusal> {
        match self.try_drawing_target()? {
            target @ SourceTarget::Paint(_) => Ok(target),
            _ if matches!(self.working.target, Some(SourceTarget::Coverage(_))) => Err(DrawingRefusal::Mask),
            _ => Err(DrawingRefusal::EffectMask),
        }
    }
    pub fn drawing_refusal(&self) -> Option<DrawingRefusal> {
        self.try_drawing_content().err()
    }
    pub fn target_operations(&self, target: SourceTarget) -> Option<&[RasterOperation]> {
        match target {
            SourceTarget::Paint(h) => self.artwork.paint.get(h).map(|s| s.operations.as_slice()),
            SourceTarget::Coverage(h) => self.artwork.coverage.get(h).map(|s| s.operations.as_slice()),
            _ => None,
        }
    }
    pub fn target_operations_mut(&mut self, target: SourceTarget) -> Option<&mut Vec<RasterOperation>> {
        match target {
            SourceTarget::Paint(h) => self.artwork.paint.get_mut(h).map(|s| Arc::make_mut(&mut s.operations)),
            SourceTarget::Coverage(h) => self.artwork.coverage.get_mut(h).map(|s| Arc::make_mut(&mut s.operations)),
            _ => None,
        }
    }
    pub fn layer_offset(&self, id: OccurrenceHandle) -> Point {
        let scene = self.scene();
        let mut offset = Point::default();
        let mut current = Some(id);
        while let Some(h) = current {
            let Some(o) = scene.occurrence(h) else {
                break;
            };
            offset.x += o.translation.x;
            offset.y += o.translation.y;
            current = scene.parent(h);
        }
        offset
    }
    pub fn target_offset(&self, target: SourceTarget) -> Point {
        self.scene().target_offset(target)
    }
    pub fn target_geometry(&self, target: SourceTarget) -> ImageTransform {
        self.scene().target_geometry(target)
    }
    pub fn affine_edit_transform(&self, target: SourceTarget) -> Option<Affine> {
        self.target_geometry(target).as_affine()
    }
    pub fn target_extent(&self, target: SourceTarget) -> [u32; 2] {
        self.scene().target_extent(target)
    }
    pub fn is_locked(&self, id: OccurrenceHandle) -> bool {
        let scene = self.scene();
        let mut current = Some(id);
        while let Some(h) = current {
            let Some(o) = scene.occurrence(h) else {
                return false;
            };
            if o.locked {
                return true;
            }
            current = scene.parent(h);
        }
        false
    }
    pub fn validate_content_write(&self, target: SourceTarget) -> Result<(), DrawingRefusal> {
        let owner = self.target_owner(target).ok_or(DrawingRefusal::NoLayer)?;
        if matches!(target, SourceTarget::Selection(_)) {
            return Err(DrawingRefusal::SelectionLayer);
        }
        if self.is_locked(owner) {
            return Err(DrawingRefusal::Locked);
        }
        self.affine_edit_transform(target).ok_or(DrawingRefusal::NonAffine).map(|_| ())
    }
    pub fn layer_is_visible(&self, id: OccurrenceHandle) -> bool {
        let scene = self.scene();
        let mut current = Some(id);
        while let Some(handle) = current {
            if !self.effective_visibility(handle) {
                return false;
            }
            current = scene.parent(handle);
        }
        true
    }
    pub fn reference_scope(&self) -> SceneScope {
        SceneScope::Members(self.reference_members(|_| true).into_iter().collect::<Vec<_>>().into())
    }
    pub fn references_below(&self, target: OccurrenceHandle) -> BTreeSet<OccurrenceHandle> {
        let scene = self.scene();
        let Some(index) = scene.position(target) else {
            return Default::default();
        };
        self.reference_members(|h| scene.position(h).is_some_and(|i| i > index))
    }
    fn reference_members(&self, keep: impl Fn(OccurrenceHandle) -> bool) -> BTreeSet<OccurrenceHandle> {
        let scene = self.scene();
        let mut members: BTreeSet<_> =
            scene.order().iter().copied().filter(|h| scene.occurrence(*h).is_some_and(|o| o.reference)).collect();
        loop {
            let before = members.len();
            members = self.layer_subtrees(&members.iter().copied().collect::<Vec<_>>());
            for h in scene.order().iter().copied().filter(|h| members.contains(h)).collect::<Vec<_>>() {
                let o = scene.occurrence(h).unwrap();
                let base = if o.clipped { self.clipping_base(h).unwrap_or(h) } else { h };
                members.insert(base);
                let siblings = scene.children(scene.parent(h));
                let index = siblings.iter().position(|h| *h == base).unwrap();
                members.extend(siblings[..index].iter().rev().copied().take_while(|h| scene.occurrence(*h).is_some_and(|o| o.clipped)));
                if scene.effect(h).is_some_and(|e| e.program.kind == EffectKind::Adjustment) && !o.clipped {
                    members.extend(backdrop_layers(scene, h));
                }
            }
            if before == members.len() {
                break;
            }
        }
        members.retain(|h| keep(*h));
        for h in members.iter().copied().collect::<Vec<_>>() {
            let mut parent = scene.parent(h);
            while let Some(h) = parent {
                members.insert(h);
                parent = scene.parent(h);
            }
        }
        members
    }
    pub fn retained_transform_targets(&self, roots: &[OccurrenceHandle]) -> Result<Vec<OccurrenceHandle>, DocumentError> {
        if roots.is_empty() {
            return Err(DocumentError::InvalidLayerOperation("Select artwork to transform"));
        }
        let scene = self.scene();
        for h in roots {
            scene.occurrence(*h).ok_or(DocumentError::MissingOccurrence(*h))?;
        }
        let roots = self.layer_roots(&roots.iter().copied().collect());
        let mut members = BTreeSet::new();
        for h in roots {
            let o = scene.occurrence(h).ok_or(DocumentError::MissingOccurrence(h))?;
            if !matches!(o.kind(), LayerKind::Paint | LayerKind::Group) {
                return Err(DocumentError::InvalidLayerOperation("Select paint layers or groups to transform"));
            }
            let subtree = self.layer_subtrees(&[h]);
            if !subtree.iter().any(|h| scene.occurrence(*h).is_some_and(|o| o.kind() == LayerKind::Paint)) {
                return Err(DocumentError::InvalidLayerOperation("This group has no paint layers"));
            }
            members.extend(subtree);
        }
        for h in &members {
            let o = scene.occurrence(*h).unwrap();
            if self.is_locked(*h) {
                return Err(DocumentError::ProtectedOccurrence(*h));
            }
            if scene.source_target(*h).and_then(|t| self.target_operations(t)).is_some_and(|ops| !ops.is_empty())
                || o.mask.as_ref().and_then(|m| self.target_operations(SourceTarget::Coverage(m.source))).is_some_and(|ops| !ops.is_empty())
            {
                return Err(DocumentError::InvalidLayerOperation("Wait for the current edit"));
            }
            if matches!(o.kind(), LayerKind::Selection)
                || scene.effect(*h).is_some_and(|e| e.program.kind == EffectKind::Generator)
            {
                return Err(DocumentError::InvalidLayerOperation("This selection contains content that cannot retain a transform"));
            }
        }
        Ok(scene.order().iter().copied().filter(|h| members.contains(h)).collect())
    }
    pub fn retained_transform_edit(&self, roots: &[OccurrenceHandle], delta: Projective) -> Result<Edit, DocumentError> {
        let targets = self.retained_transform_targets(roots)?;
        if delta == Projective::IDENTITY {
            return Ok(Edit::Batch(Vec::new()));
        }
        let invalid = || DocumentError::InvalidLayerOperation("Invalid retained transform");
        if delta.inverse().is_none() {
            return Err(invalid());
        }
        let scene = self.scene();
        let mut edits = Vec::new();
        for h in targets {
            let old = scene.occurrence(h).unwrap();
            let mut o = old.clone();
            if o.kind() == LayerKind::Paint {
                let to = Projective::from_affine(Affine::translation(self.layer_offset(h)));
                let local = to.then(delta).and_then(|m| m.then(to.inverse()?)).ok_or_else(invalid)?;
                o.placement = old.placement.post(local).ok_or_else(invalid)?;
                o.placement.validate_for(Rect::from_extent(scene.local_extent(h)))?;
            }
            if let Some(mask) = o.mask.as_mut() && !(mask.linked && old.kind() == LayerKind::Paint) {
                let desired = self
                    .target_geometry(SourceTarget::Coverage(mask.source))
                    .projective()
                    .and_then(|m| m.then(delta))
                    .ok_or_else(invalid)?;
                mask.placement = Projective::IDENTITY;
                let mut rest = mask.geometry_in_parent(old).projective().ok_or_else(invalid)?;
                let world = self.layer_offset(h);
                rest = rest
                    .then(Projective::from_affine(Affine::translation(Point {
                        x: world.x - old.translation.x,
                        y: world.y - old.translation.y,
                    })))
                    .ok_or_else(invalid)?;
                mask.placement = desired.then(rest.inverse().ok_or_else(invalid)?).ok_or_else(invalid)?;
                mask.validate()?;
            }
            if o != *old {
                edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, h, Some(o))?));
            }
        }
        Ok(Edit::Batch(edits))
    }
    pub fn move_target_edit(&self, delta: Point) -> Result<Edit, DocumentError> {
        let h = self.working.occurrence.ok_or(DocumentError::InvalidLayerOperation("Select artwork to move"))?;
        let mut o = self.scene().occurrence(h).ok_or(DocumentError::MissingOccurrence(h))?.clone();
        if self.is_locked(h) {
            return Err(DocumentError::ProtectedOccurrence(h));
        }
        let mask_target = matches!(self.working.target, Some(SourceTarget::Coverage(_)));
        let linked = o.mask.as_ref().is_some_and(|m| m.linked);
        if !mask_target || linked {
            o.translation.x += delta.x;
            o.translation.y += delta.y;
        }
        if let Some(mask) = o.mask.as_mut().filter(|_| mask_target || linked) {
            mask.translation.x += delta.x;
            mask.translation.y += delta.y;
        }
        Ok(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, h, Some(o))?))
    }
}

impl Document {
    pub fn select_occurrence_edit(&self, id: OccurrenceHandle) -> Result<Edit, DocumentError> {
        self.scene().occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        let mut working = self.working.clone();
        if let Some(previous) = working.occurrence.filter(|h| {
            self.scene().occurrence(*h).is_some_and(|o| matches!(o.content, OccurrenceContent::Selection(_)))
        }) {
            working.selection_visibility.insert(previous, false);
        }
        if matches!(self.scene().occurrence(id).unwrap().content, OccurrenceContent::Selection(_)) {
            working.selection_visibility.insert(id, true);
        }
        working.occurrence = Some(id);
        working.target = self.scene().source_target(id);
        working.inspect_mask = None;
        Ok(Edit::Working(working))
    }
    pub fn move_occurrence_edit(&self, id: OccurrenceHandle, to: usize) -> Result<Edit, DocumentError> {
        self.reparent_occurrence_edit(id, self.scene().parent(id), to)
    }

    pub fn can_delete_layers(&self, roots: &[OccurrenceHandle]) -> bool {
        self.deletable_layer_ids(roots).is_ok()
    }
    fn deletable_layer_ids(&self, roots: &[OccurrenceHandle]) -> Result<BTreeSet<OccurrenceHandle>, DocumentError> {
        if roots.is_empty() {
            return Err(DocumentError::InvalidLayerOperation("Select layers first"));
        }
        let scene = self.scene();
        for h in roots {
            scene.occurrence(*h).ok_or(DocumentError::MissingOccurrence(*h))?;
        }
        let ids = self.layer_subtrees(roots);
        for h in &ids {
            if self.is_locked(*h) {
                return Err(DocumentError::ProtectedOccurrence(*h));
            }
        }
        if scene.order().iter().any(|h| {
            scene.occurrence(*h).is_some_and(|o| o.clipped) && !ids.contains(h) && self.clipping_base(*h).is_some_and(|b| ids.contains(&b))
        }) {
            return Err(DocumentError::InvalidLayerOperation("Include the clipped layers above this base"));
        }
        Ok(ids)
    }
    pub fn delete_layers_edit(&self, roots: &[OccurrenceHandle]) -> Result<Edit, DocumentError> {
        let ids = self.deletable_layer_ids(roots)?;
        let mut edits = Vec::new();
        for (h, _, stack) in self.artwork.stacks.iter() {
            let mut stack = stack.clone();
            stack.entries.retain(|h| !ids.contains(h));
            if self.artwork.stacks.get(h) != Some(&stack) {
                edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, h, Some(stack))?));
            }
        }
        edits.extend(self.removal_edits(&ids)?);
        if self.working.occurrence.is_some_and(|h| ids.contains(&h)) {
            let mut working = self.working.clone();
            working.occurrence = self.scene().order().iter().copied().find(|h| !ids.contains(h));
            working.target = working.occurrence.and_then(|h| self.scene().source_target(h));
            working.inspect_mask = None;
            edits.push(Edit::Working(working));
        }
        Ok(Edit::Batch(edits))
    }
    pub fn group_layers_edit(
        &self,
        roots: &[OccurrenceHandle],
        blend: LayerBlend,
        name: impl Into<Arc<str>>,
    ) -> Result<Edit, DocumentError> {
        let scene = self.scene();
        let first = *roots.first().ok_or(DocumentError::InvalidLayerOperation("Select layers first"))?;
        let parent = scene.parent(first);
        let stack = scene.stack(first).ok_or(DocumentError::MissingOccurrence(first))?;
        let selected: BTreeSet<_> = roots.iter().copied().collect();
        for h in roots {
            scene.occurrence(*h).ok_or(DocumentError::MissingOccurrence(*h))?;
            if self.is_locked(*h) {
                return Err(DocumentError::ProtectedOccurrence(*h));
            }
            if scene.parent(*h) != parent {
                return Err(DocumentError::InvalidLayerOperation("Select layers in the same group"));
            }
        }
        let siblings = scene.children(parent);
        let positions: Vec<_> = siblings.iter().enumerate().filter(|(_, h)| selected.contains(h)).map(|(i, _)| i).collect();
        if positions.len() != roots.len() || positions.last().unwrap() - positions[0] + 1 != roots.len() {
            return Err(DocumentError::InvalidLayerOperation("Select neighboring layers to group"));
        }
        for h in siblings {
            if scene.occurrence(*h).is_some_and(|o| o.clipped)
                && self.clipping_base(*h).is_none_or(|b| selected.contains(h) != selected.contains(&b))
            {
                return Err(DocumentError::InvalidLayerOperation("Include the complete clipping stack"));
            }
        }
        let nested = RecordChange::insert(
            &self.artwork.stacks,
            Stack { entries: siblings.iter().copied().filter(|h| selected.contains(h)).collect() },
        );
        let mut group = Occurrence::new(OccurrenceContent::Stack(nested.handle), name);
        group.blend = blend;
        let occurrence = RecordChange::insert(&self.artwork.occurrences, group);
        let mut containing = self.artwork.stacks.get(stack).unwrap().clone();
        containing.entries.retain(|h| !selected.contains(h));
        containing.entries.insert(positions[0], occurrence.handle);
        Ok(Edit::Batch(vec![
            Edit::Stack(nested),
            Edit::Occurrence(occurrence),
            Edit::Stack(RecordChange::replace(&self.artwork.stacks, stack, Some(containing))?),
        ]))
    }
    pub fn ungroup_layer_edit(&self, id: OccurrenceHandle) -> Result<Edit, DocumentError> {
        let scene = self.scene();
        let group = scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        if self.is_locked(id) {
            return Err(DocumentError::ProtectedOccurrence(id));
        }
        if group.kind() != LayerKind::Group
            || group.opacity != 1.
            || group.mask.is_some()
            || !(group.passes_through() || group.blend == LayerBlend::Normal)
            || group.clipped
        {
            return Err(DocumentError::InvalidLayerOperation("Remove the group mask, blend and opacity effects before ungrouping"));
        }
        let children = scene.children(Some(id));
        let mut edits = Vec::new();
        for h in children {
            let child = scene.occurrence(*h).unwrap();
            if self.is_locked(*h) {
                return Err(DocumentError::ProtectedOccurrence(*h));
            }
            if !group.passes_through() && child.blend != LayerBlend::Normal {
                return Err(DocumentError::InvalidLayerOperation("Set child layers to Normal before ungrouping"));
            }
            let mut child = child.clone();
            child.translation.x += group.translation.x;
            child.translation.y += group.translation.y;
            if let Some(mask) = &mut child.mask {
                mask.translation.x += group.translation.x;
                mask.translation.y += group.translation.y;
            }
            child.visible &= group.visible;
            child.reference |= group.reference;
            edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, *h, Some(child))?));
        }
        let stack = scene.stack(id).unwrap();
        let mut containing = self.artwork.stacks.get(stack).unwrap().clone();
        let index = containing.entries.iter().position(|h| *h == id).unwrap();
        containing.entries.splice(index..index + 1, children.iter().copied());
        let OccurrenceContent::Stack(nested) = group.content else { unreachable!() };
        edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, nested, Some(Stack::default()))?));
        edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, stack, Some(containing))?));
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences, id, None)?));
        if self.working.occurrence == Some(id) {
            let mut working = self.working.clone();
            working.occurrence = children.first().copied();
            working.target = working.occurrence.and_then(|h| scene.source_target(h));
            working.inspect_mask = None;
            edits.push(Edit::Working(working));
        }
        Ok(Edit::Batch(edits))
    }
}

impl Document {
    pub(crate) fn removal_edits(&self, ids: &BTreeSet<OccurrenceHandle>) -> Result<Vec<Edit>, DocumentError> {
        let mut paints = BTreeSet::new();
        let mut coverage = BTreeSet::new();
        let mut stacks = BTreeSet::new();
        let mut effects = BTreeSet::new();
        let mut selections = BTreeSet::new();
        for h in ids {
            let o = self.artwork.occurrences.get(*h).ok_or(DocumentError::MissingOccurrence(*h))?;
            match o.content {
                OccurrenceContent::Paint(h) => {
                    paints.insert(h);
                }
                OccurrenceContent::Stack(h) => {
                    stacks.insert(h);
                }
                OccurrenceContent::Effect(h) => {
                    effects.insert(h);
                }
                OccurrenceContent::Selection(h) => {
                    selections.insert(h);
                }
            }
            if let Some(mask) = &o.mask {
                coverage.insert(mask.source);
            }
        }
        for (h, _, o) in self.artwork.occurrences.iter().filter(|(h, _, _)| !ids.contains(h)) {
            let _ = h;
            match o.content {
                OccurrenceContent::Paint(h) => {
                    paints.remove(&h);
                }
                OccurrenceContent::Stack(h) => {
                    stacks.remove(&h);
                }
                OccurrenceContent::Effect(h) => {
                    effects.remove(&h);
                }
                OccurrenceContent::Selection(h) => {
                    selections.remove(&h);
                }
            }
            if let Some(mask) = &o.mask {
                coverage.remove(&mask.source);
            }
        }
        let mut edits = Vec::new();
        for h in ids {
            edits.push(Edit::Occurrence(RecordChange::remove(&self.artwork.occurrences, *h)?));
        }
        for h in paints {
            edits.push(Edit::Paint(RecordChange::remove(&self.artwork.paint, h)?));
        }
        for h in coverage {
            edits.push(Edit::Coverage(RecordChange::remove(&self.artwork.coverage, h)?));
        }
        for h in stacks {
            edits.push(Edit::Stack(RecordChange::remove(&self.artwork.stacks, h)?));
        }
        for h in effects {
            edits.push(Edit::Effect(RecordChange::remove(&self.artwork.effects, h)?));
        }
        for h in selections {
            edits.push(Edit::SavedSelection(RecordChange::remove(&self.artwork.selections, h)?));
        }
        Ok(edits)
    }
}

#[cfg(test)]
mod organization_tests {
    use super::*;
    use crate::operation_test_support as fixture;
    use fixture::*;
    #[test]
    fn paint_operations_accept_extended_rgb_and_reject_invalid_coverage() {
        let operations = |color| {
            [
                RasterOperationKind::Fill { color, alpha_locked: false },
                RasterOperationKind::Gradient {
                    start: Point { x: 10., y: 20. },
                    end: Point { x: 80., y: 60. },
                    colors: [color; 2],
                    radial: false,
                    alpha_locked: false,
                },
                RasterOperationKind::Figure(crate::Figure {
                    shape: crate::FigureShape::Rectangle,
                    paint: crate::FigurePaint::Both,
                    start: Point { x: 10., y: 20. },
                    end: Point { x: 80., y: 60. },
                    width: 4.,
                    colors: [color; 2],
                    alpha_locked: false,
                    erase: false,
                }),
            ]
            .map(|kind| RasterOperation {
                placement: Affine::IDENTITY,
                coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(20), [100; 2], Point::default()),
                kind,
            })
        };
        // Portable P3 red is outside sRGB even in an ordinary SDR document.
        let p3 = color::RgbColor::new(color::RgbSpace::DisplayP3, [1., 0., 0., 0.8]).unwrap().linear_in(color::RgbSpace::Srgb).unwrap();
        assert!(p3[0] > 1. && p3[1] < 0.);
        for color in [p3, [8., -0.125, 2., 0.25], [-0.01, 1.01, 0., 0.], [1.; 4]] {
            for operation in operations(color) {
                operation.validate().unwrap();
            }
        }
        for channel in 0..4 {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut color = [0.5; 4];
                color[channel] = value;
                for operation in operations(color) {
                    assert!(operation.validate().is_err(), "channel {channel}: {value}");
                }
            }
        }
        for alpha in [-0.001, 1.001] {
            for operation in operations([0.5, 0.5, 0.5, alpha]) {
                assert!(operation.validate().is_err());
            }
        }
    }

    #[test]
    fn blend_codes_are_stable_and_every_mode_has_one_menu_place() {
        let shipped = ["Normal", "Multiply", "Screen", "Add", "Overlay", "SoftLight", "Color"];
        for (code, name) in shipped.into_iter().enumerate() {
            let blend = LayerBlend::from_code(code as u32).unwrap();
            assert_eq!(serde_json::to_value(blend).unwrap(), name);
            assert_eq!(serde_json::from_value::<LayerBlend>(name.into()).unwrap(), blend);
        }
        for (code, blend) in LayerBlend::ALL.into_iter().enumerate() {
            assert_eq!(blend.code(), code as u32);
            assert_eq!(LayerBlend::from_code(blend.code()), Some(blend));
        }
        assert_eq!(LayerBlend::from_code(LayerBlend::ALL.len() as u32), None);
        let menu: Vec<_> = LayerBlend::MENU.into_iter().flatten().copied().collect();
        assert_eq!(menu.len(), LayerBlend::ALL.len());
        assert!(LayerBlend::ALL.iter().all(|b| menu.contains(b)));
        assert_eq!(LayerBlend::MENU[0], [LayerBlend::PassThrough, LayerBlend::Normal]);
        let float: Vec<_> = menu.iter().filter(|b| b.offered(LayerKind::Paint, true)).map(|b| b.label()).collect();
        assert!(!float.contains(&"Overlay") && !float.contains(&"Hard Mix") && float.contains(&"Linear Light"));
        assert!(menu.iter().all(|b| b.offered(LayerKind::Group, false)));
        let paint: Vec<_> = menu.iter().filter(|b| b.offered(LayerKind::Paint, false)).collect();
        assert_eq!(paint.len(), menu.len() - 1);
        assert!(!paint.contains(&&LayerBlend::PassThrough));
        assert!(LayerBlend::PassThrough.offered(LayerKind::Group, true));
    }

    #[test]
    fn placement_composes_group_offsets_and_linked_masks() {
        let mut doc = document([2000, 1500], &["Group", "Ink"]);
        nest(&mut doc, "Group", &["Ink"]);
        occurrence_mut(&mut doc, "Group").translation = Point { x: 20., y: -30. };
        occurrence_mut(&mut doc, "Ink").translation = Point { x: 6., y: 9. };
        occurrence_mut(&mut doc, "Ink").placement = LayerPlacement::from_affine(Affine([0.5, 0., 0., 0.5, -100., 50.]));
        let ink = id(&doc, "Ink");
        let mask = add_mask(&mut doc, ink, [2000, 1500], Point { x: 10., y: 9. });
        let target = target(&doc, "Ink");
        let mask_target = SourceTarget::Coverage(mask);
        let p = Point { x: 100., y: 200. };
        assert_eq!(doc.affine_edit_transform(target).unwrap().map(p), Point { x: -24., y: 129. });
        assert_eq!(doc.affine_edit_transform(mask_target).unwrap().map(p), Point { x: -22., y: 129. });
        occurrence_mut(&mut doc, "Ink").mask.as_mut().unwrap().linked = false;
        assert_eq!(doc.affine_edit_transform(mask_target).unwrap().map(p), Point { x: 130., y: 179. });
        let mut invalid = occurrence(&doc, "Ink").clone();
        invalid.placement = LayerPlacement::from_affine(Affine([0.; 6]));
        assert!(doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, ink, Some(invalid)).unwrap())).is_err());
    }
    #[test]
    fn saved_selection_navigation_overrides_only_overlays_and_respects_hidden_groups() {
        let mut doc = document([64; 2], &["Group", "One", "Two", "Paint"]);
        saved(&mut doc, "One", Selection::full());
        saved(&mut doc, "Two", Selection::full());
        nest(&mut doc, "Group", &["One", "Two"]);
        occurrence_mut(&mut doc, "Group").visible = false;
        occurrence_mut(&mut doc, "One").visible = false;
        let one = id(&doc, "One");
        let two = id(&doc, "Two");
        let paint = id(&doc, "Paint");
        let authored = doc.artwork.clone();
        let edit = doc.select_occurrence_edit(one).unwrap();
        assert!(matches!(edit, Edit::Working(_)));
        doc.apply(edit).unwrap();
        assert!(doc.effective_visibility(one));
        assert!(!doc.layer_is_visible(one), "a hidden parent still hides the selected overlay");
        assert_eq!(doc.artwork, authored);

        occurrence_mut(&mut doc, "Group").visible = true;
        let authored = doc.artwork.clone();
        assert!(doc.layer_is_visible(one));
        assert!(!layer_is_visible(doc.scene(), one), "authored scene visibility ignores working overlays");
        doc.apply(doc.select_occurrence_edit(two).unwrap()).unwrap();
        assert!(!doc.layer_is_visible(one));
        assert!(doc.layer_is_visible(two));
        assert_eq!(doc.working.selection_visibility.get(&one), Some(&false));
        assert_eq!(doc.working.selection_visibility.get(&two), Some(&true));
        doc.apply(doc.select_occurrence_edit(two).unwrap()).unwrap();
        assert!(doc.layer_is_visible(two), "reselecting keeps the selected overlay shown");
        doc.apply(doc.select_occurrence_edit(paint).unwrap()).unwrap();
        assert!(!doc.layer_is_visible(two));
        assert!(doc.layer_is_visible(paint));
        assert!(!doc.working.selection_visibility.contains_key(&paint));
        assert_eq!(doc.artwork, authored);
    }

    #[test]
    fn pending_mask_operations_validate_before_submission() {
        let op = RasterOperation {
            placement: Affine::IDENTITY,
            coverage: CoverageSnapshot::reveal_all(CoverageHandle::from_index(20), [100; 2], Point::default()),
            kind: RasterOperationKind::Transform(ImageTransform::default()),
        };
        let mut mask = CoverageSnapshot::reveal_all(CoverageHandle::from_index(9), [100; 2], Point::default());
        mask.source.operations = Arc::new(vec![op.clone()]);
        assert!(mask.validate().is_ok());
        let mut snapshot = RasterOperation { placement: Affine::IDENTITY, coverage: mask.clone(), kind: RasterOperationKind::ApplyMask };
        assert!(snapshot.validate().is_ok());
        for mut operation in [op.clone(), snapshot.clone()] {
            operation.placement = Affine::translation(Point { x: 12., y: -7. });
            assert!(operation.validate().is_err());
        }
        snapshot.kind = op.kind;
        assert!(snapshot.validate().is_err());
        mask.source.operations = Arc::default();
        mask.source.default_coverage = f32::NAN;
        assert!(mask.validate().is_err());
        mask.source.default_coverage = 1.;
        mask.use_.translation.x = f32::INFINITY;
        assert!(mask.validate().is_err());
    }
    fn reference_names(doc: &Document) -> Vec<String> {
        let scope = doc.reference_scope();
        doc.scene()
            .order()
            .iter()
            .copied()
            .filter(|h| doc.scene().with_scope(&scope).visible(*h))
            .map(|h| doc.scene().occurrence(h).unwrap().name.to_string())
            .collect()
    }
    #[test]
    fn references_preserve_objects_and_ancestors_not_unrelated_siblings() {
        let mut doc = document([128; 2], &["Group", "Clip", "Line", "Unrelated", "Ink"]);
        let group = nest(&mut doc, "Group", &["Clip", "Line", "Unrelated"]);
        occurrence_mut(&mut doc, "Group").translation = Point { x: 5., y: 8. };
        occurrence_mut(&mut doc, "Clip").clipped = true;
        assert!(reference_names(&doc).is_empty());
        for name in ["Line", "Clip"] {
            occurrence_mut(&mut doc, name).reference = true;
            assert_eq!(reference_names(&doc), ["Group", "Clip", "Line"]);
            let snapshot = doc.snapshot();
            assert_eq!(snapshot.view().occurrence(group), doc.scene().occurrence(group));
            assert_eq!(snapshot.view().order(), doc.scene().order());
            occurrence_mut(&mut doc, name).reference = false;
        }
        occurrence_mut(&mut doc, "Group").reference = true;
        assert_eq!(reference_names(&doc), ["Group", "Clip", "Line", "Unrelated"]);
        occurrence_mut(&mut doc, "Group").visible = false;
        assert!(reference_names(&doc).is_empty());
    }
    #[test]
    fn clipping_stack_top_respects_siblings_and_hidden_members() {
        let mut doc = document([100; 2], &["Group", "Hidden clip", "Second", "Base", "Other root"]);
        nest(&mut doc, "Group", &["Hidden clip", "Second", "Base"]);
        for name in ["Hidden clip", "Second"] {
            occurrence_mut(&mut doc, name).clipped = true;
            occurrence_mut(&mut doc, name).visible = false;
        }
        assert_eq!(doc.clipping_stack_top(id(&doc, "Base")), Some(id(&doc, "Hidden clip")));
        assert_eq!(doc.clipping_stack_top(id(&doc, "Second")), Some(id(&doc, "Hidden clip")));
        assert_eq!(doc.clipping_stack_top(id(&doc, "Other root")), Some(id(&doc, "Other root")));
        assert_eq!(doc.clipping_stack_top(OccurrenceHandle::INVALID), None);
    }
    #[test]
    fn grouping_and_ungrouping_preserve_order_and_world_coordinates() {
        let mut doc = document([100; 2], &["Texture", "Ink"]);
        let texture = id(&doc, "Texture");
        let ink = id(&doc, "Ink");
        let mask = add_mask(&mut doc, texture, [100; 2], Point { x: 7., y: 9. });
        let roots = doc.layer_roots(&[ink, texture].into());
        let name = "  Group { $name }「グループ」🖌️\u{2068}literal\u{2069}  ";
        doc.apply(doc.group_layers_edit(&roots, LayerBlend::Normal, name).unwrap()).unwrap();
        let group = id(&doc, name);
        assert_eq!(occurrence(&doc, name).name.as_ref(), name);
        assert_eq!(doc.layer_roots(&[group, texture].into()), vec![group]);
        occurrence_mut(&mut doc, name).translation = Point { x: 20., y: -10. };
        occurrence_mut(&mut doc, name).reference = true;
        insert_paint(&mut doc, "Outside", 0, None);
        doc.apply(doc.move_occurrence_edit(texture, 0).unwrap()).unwrap();
        let offset = doc.layer_offset(texture);
        let mask_offset = doc.target_offset(SourceTarget::Coverage(mask));
        let before = doc.clone();
        let order: Vec<_> = doc.scene().order().iter().copied().filter(|h| *h != group).collect();
        let undo = doc.apply(doc.ungroup_layer_edit(group).unwrap()).unwrap();
        assert_eq!(doc.scene().order(), order);
        assert_eq!(doc.layer_offset(texture), offset);
        assert_eq!(doc.target_offset(SourceTarget::Coverage(mask)), mask_offset);
        assert_eq!(doc.scene().references(), [ink, texture].into());
        doc.apply(undo).unwrap();
        restored(&before, &doc);
        assert_eq!(doc.scene().references(), before.scene().references());
        occurrence_mut(&mut doc, name).opacity = 0.5;
        assert!(doc.ungroup_layer_edit(group).is_err());
    }
    #[test]
    fn bulk_edits_protect_clipping_stacks_and_locks() {
        let mut doc = document([100; 2], &["Shade", "Ink"]);
        occurrence_mut(&mut doc, "Shade").clipped = true;
        let base = id(&doc, "Ink");
        let shade = id(&doc, "Shade");
        assert!(doc.delete_layers_edit(&[base]).is_err());
        assert!(doc.group_layers_edit(&[base], LayerBlend::Normal, "Group").is_err());
        assert!(doc.delete_layers_edit(&[base, shade]).is_ok());
        insert_paint(&mut doc, "Other", 0, None);
        let before = doc.clone();
        let undo = doc.apply(doc.delete_layers_edit(&[base, shade]).unwrap()).unwrap();
        assert!(doc.scene().occurrence(base).is_none());
        doc.apply(undo).unwrap();
        restored(&before, &doc);
        occurrence_mut(&mut doc, "Shade").locked = true;
        assert!(doc.delete_layers_edit(&[base, shade]).is_err());
    }
    fn pass_through_document() -> Document {
        let mut doc = document([64; 2], &["Above", "Group", "Top", "Adjustment", "Inner", "Below", "Ink"]);
        nest(&mut doc, "Group", &["Top", "Adjustment", "Inner"]);
        occurrence_mut(&mut doc, "Group").blend = LayerBlend::PassThrough;
        for name in ["Top", "Inner"] {
            occurrence_mut(&mut doc, name).blend = LayerBlend::Multiply;
        }
        effect(&mut doc, "Adjustment", "exposure");
        doc
    }
    #[test]
    fn pass_through_is_for_groups_and_is_saved_by_a_new_name() {
        let mut doc = pass_through_document();
        assert_eq!(serde_json::to_value(LayerBlend::PassThrough).unwrap(), "PassThrough");
        assert_eq!(LayerBlend::PassThrough.label(), "Pass Through");
        let h = id(&doc, "Above");
        let mut paint = occurrence(&doc, "Above").clone();
        paint.blend = LayerBlend::PassThrough;
        assert_eq!(
            doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, h, Some(paint)).unwrap())).unwrap_err().to_string(),
            "Only groups can use Pass Through"
        );
        let group = occurrence(&doc, "Group");
        assert!(group.passes_through());
        let mut clipped = group.clone();
        clipped.clipped = true;
        assert!(!clipped.passes_through());
    }
    #[test]
    fn backdrops_widen_through_pass_through_groups_to_the_nearest_isolated_one() {
        let mut doc = pass_through_document();
        let names_for = |doc: &Document, handles: Vec<OccurrenceHandle>| {
            handles.into_iter().map(|h| doc.scene().occurrence(h).unwrap().name.to_string()).collect::<Vec<_>>()
        };
        let group = id(&doc, "Group");
        let adjustment = id(&doc, "Adjustment");
        assert_eq!(isolated_scope(doc.scene(), Some(group)), None);
        assert_eq!(names_for(&doc, backdrop_layers(doc.scene(), adjustment)), ["Inner", "Below", "Ink", "Paper"]);
        assert_eq!(names_for(&doc, backdrop_layers(doc.scene(), id(&doc, "Top"))), ["Adjustment", "Inner", "Below", "Ink", "Paper"]);
        assert_eq!(
            names_for(&doc, backdrop_layers(doc.scene(), id(&doc, "Above"))),
            ["Group", "Top", "Adjustment", "Inner", "Below", "Ink", "Paper"]
        );
        occurrence_mut(&mut doc, "Adjustment").reference = true;
        assert_eq!(reference_names(&doc), ["Group", "Adjustment", "Inner", "Below", "Ink", "Paper"]);
        occurrence_mut(&mut doc, "Group").blend = LayerBlend::Normal;
        assert_eq!(isolated_scope(doc.scene(), Some(group)), Some(group));
        assert_eq!(names_for(&doc, backdrop_layers(doc.scene(), adjustment)), ["Inner"]);
        assert_eq!(reference_names(&doc), ["Group", "Adjustment", "Inner"]);
        assert!(descends_from(doc.scene(), id(&doc, "Inner"), Some(group)));
        assert!(!descends_from(doc.scene(), id(&doc, "Below"), Some(group)));
    }
    #[test]
    fn pass_through_groups_ungroup_with_any_layers_at_full_opacity_without_a_mask() {
        let mut doc = pass_through_document();
        let group = id(&doc, "Group");
        let before = doc.clone();
        let undo = doc.apply(doc.ungroup_layer_edit(group).unwrap()).unwrap();
        assert!(doc.scene().order().iter().all(|h| doc.scene().parent(*h).is_none()));
        assert_eq!(occurrence(&doc, "Top").blend, LayerBlend::Multiply);
        doc.apply(undo).unwrap();
        restored(&before, &doc);
        for blend in [false, true] {
            let mut doc = before.clone();
            if blend {
                occurrence_mut(&mut doc, "Group").blend = LayerBlend::Normal;
            } else {
                occurrence_mut(&mut doc, "Group").opacity = 0.5;
            }
            assert!(doc.ungroup_layer_edit(group).is_err());
        }
        let mut doc = before.clone();
        add_mask(&mut doc, group, [64; 2], Point::default());
        assert!(doc.ungroup_layer_edit(group).is_err());
    }
    #[test]
    fn deletion_moves_exclusive_roots_into_inverse_and_preserves_unplaced_sources() {
        let mut doc = document([64; 2], &["Ink"]);
        let owner = id(&doc, "Ink");
        let SourceTarget::Paint(paint) = target(&doc, "Ink") else { panic!("paint") };
        let mask = add_mask(&mut doc, owner, [64; 2], Point::default());
        let unplaced = doc
            .artwork
            .paint
            .insert(
                PortableId::random(),
                PaintSource { domain: [64; 2], raster: Default::default(), original: None, operations: Arc::default() },
            )
            .unwrap();
        let before = doc.clone();
        let inverse = doc.apply(doc.delete_layers_edit(&[owner]).unwrap()).unwrap();
        assert!(doc.artwork.paint.get(paint).is_none());
        assert!(doc.artwork.coverage.get(mask).is_none());
        assert!(doc.artwork.paint.get(unplaced).is_some());
        doc.apply(inverse).unwrap();
        restored(&before, &doc);
        assert_eq!(doc.artwork.paint.id(paint), before.artwork.paint.id(paint));
        assert_eq!(doc.artwork.coverage.id(mask), before.artwork.coverage.id(mask));
        assert_eq!(doc.artwork.paint.get(paint), before.artwork.paint.get(paint));
        assert_eq!(doc.artwork.coverage.get(mask), before.artwork.coverage.get(mask));
    }
}
