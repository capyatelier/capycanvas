//! Layer ownership and coverage, independent of UI widgets and GPU storage.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum LayerBlend {
    #[default]
    Normal,
    Multiply,
    Screen,
    Add,
    Overlay,
    SoftLight,
    Color,
}
impl LayerBlend {
    pub const ALL: [Self; 7] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Add,
        Self::Overlay,
        Self::SoftLight,
        Self::Color,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Add => "Add",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::Color => "Color",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerProperties {
    pub parent: Option<LayerId>,
    pub offset: Point,
    pub alpha_locked: bool,
    pub locked: bool,
    pub clipped: bool,
    pub blend: LayerBlend,
}

/// Polygon selection: even/odd interiors support holes and disjoint islands.
/// Coordinates remain geometry; rasterization and antialiasing are GPU work.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub contours: Arc<[Arc<[Point]>]>,
    pub inverted: bool,
}
impl Selection {
    pub fn polygon(points: Vec<Point>) -> Result<Self, DocumentError> {
        if points.len() < 3 || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(DocumentError::InvalidLayerOperation(
                "A selection needs a closed area",
            ));
        }
        Ok(Self {
            contours: vec![points.into()].into(),
            inverted: false,
        })
    }
    pub fn translated(&self, delta: Point) -> Self {
        Self {
            contours: self
                .contours
                .iter()
                .map(|path| {
                    path.iter()
                        .map(|p| Point {
                            x: p.x + delta.x,
                            y: p.y + delta.y,
                        })
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect::<Vec<_>>()
                .into(),
            inverted: self.inverted,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayerMask {
    /// Unique image identity, allocated from the document layer-ID allocator.
    pub id: LayerId,
    pub enabled: bool,
    pub linked: bool,
    pub offset: Point,
    pub initial: Option<Selection>,
    pub default_coverage: f32,
    pub inverted: bool,
    pub strokes: Arc<Vec<StrokeId>>,
    /// Inspection only; never participates in exported color.
    pub show_area: bool,
}

/// Ordered raster mutations retain their source coverage for deterministic undo
/// and device-loss replay. They are not live composition masks after baking.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerOperation {
    pub after_stroke: usize,
    pub coverage: LayerMask,
    pub kind: LayerOperationKind,
}
#[derive(Clone, Debug, PartialEq)]
pub enum LayerOperationKind {
    ApplyMask,
    Fill { color: [f32; 4], alpha_locked: bool },
}
impl LayerMask {
    pub fn reveal_all(id: LayerId, offset: Point) -> Self {
        Self {
            id,
            enabled: true,
            linked: true,
            offset,
            initial: None,
            default_coverage: 1.0,
            inverted: false,
            strokes: Arc::default(),
            show_area: false,
        }
    }
}

impl Document {
    /// Storage order defines sibling order; parents always precede their subtree
    /// in the UI, including after moving a group as a single item.
    pub fn ordered_layers(&self) -> Vec<&Layer> {
        fn visit<'a>(doc: &'a Document, parent: Option<LayerId>, out: &mut Vec<&'a Layer>) {
            for l in doc.layers.iter().filter(|l| l.properties.parent == parent) {
                out.push(l);
                if l.kind == LayerKind::Group {
                    visit(doc, Some(l.id), out);
                }
            }
        }
        let mut out = Vec::with_capacity(self.layers.len());
        visit(self, None, &mut out);
        out
    }
    pub fn target_owner(&self, target: LayerId) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|l| l.id == target || l.mask.as_ref().is_some_and(|m| m.id == target))
    }
    pub(crate) fn target_strokes_mut(&mut self, target: LayerId) -> Option<&mut Vec<StrokeId>> {
        for layer in &mut self.layers {
            if layer.id == target {
                return Some(&mut layer.strokes);
            }
            if let Some(mask) = &mut layer.mask
                && mask.id == target
            {
                return Some(Arc::make_mut(&mut mask.strokes));
            }
        }
        None
    }
    pub fn active_target(&self) -> LayerId {
        self.layer(self.active_layer)
            .and_then(|l| l.mask.as_ref())
            .filter(|_| self.active_mask)
            .map_or(self.active_layer, |m| m.id)
    }
    pub fn layer_offset(&self, id: LayerId) -> Point {
        let Some(owner) = self.target_owner(id) else {
            return Point::default();
        };
        let mut offset = if owner.id == id {
            owner.properties.offset
        } else {
            owner.mask.as_ref().unwrap().offset
        };
        let mut parent = owner.properties.parent;
        for _ in 0..self.layers.len() {
            let Some(layer) = parent.and_then(|id| self.layer(id)) else {
                break;
            };
            offset.x += layer.properties.offset.x;
            offset.y += layer.properties.offset.y;
            parent = layer.properties.parent;
        }
        offset
    }
    pub fn is_locked(&self, id: LayerId) -> bool {
        let mut target = self.target_owner(id);
        for _ in 0..self.layers.len() {
            let Some(layer) = target else {
                return false;
            };
            if layer.properties.locked {
                return true;
            }
            target = layer.properties.parent.and_then(|id| self.layer(id));
        }
        target.is_some()
    }
    pub fn validate_layer(&self, layer: &Layer) -> Result<(), DocumentError> {
        if !layer.opacity.is_finite()
            || !(0.0..=1.0).contains(&layer.opacity)
            || !layer.properties.offset.x.is_finite()
            || !layer.properties.offset.y.is_finite()
        {
            return Err(DocumentError::InvalidLayerOperation("Invalid layer value"));
        }
        let mut parent = layer.properties.parent;
        for _ in 0..=self.layers.len() {
            let Some(id) = parent else {
                return Ok(());
            };
            if id == layer.id {
                break;
            }
            let node = self.layer(id).ok_or(DocumentError::MissingLayer(id))?;
            if node.kind != LayerKind::Group {
                break;
            }
            parent = node.properties.parent;
        }
        Err(DocumentError::InvalidLayerOperation("Invalid layer group"))
    }
    pub fn move_target_edit(&self, delta: Point) -> Result<Edit, DocumentError> {
        let mut layer = self
            .layer(self.active_layer)
            .ok_or(DocumentError::MissingLayer(self.active_layer))?
            .clone();
        if self.is_locked(layer.id) {
            return Err(DocumentError::ProtectedLayer(layer.id));
        }
        let linked = layer.mask.as_ref().is_some_and(|m| m.linked);
        if !self.active_mask || linked {
            layer.properties.offset.x += delta.x;
            layer.properties.offset.y += delta.y;
        }
        if let Some(mask) = &mut layer.mask
            && (self.active_mask || linked)
        {
            mask.offset.x += delta.x;
            mask.offset.y += delta.y;
        }
        Ok(Edit::ReplaceLayer(Box::new(layer)))
    }
}
