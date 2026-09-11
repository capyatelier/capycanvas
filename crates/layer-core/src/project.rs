//! Native project transport. Hosts supply streams and perform atomic file I/O
//! off the drawing thread. No filenames, GPU handles or platform state are saved.
use crate::*;
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

const MAGIC: &[u8; 12] = b"CAPYPROJECT\x01";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectAssetFormat {
    R8Unorm,
    Rgba8Srgb,
}
impl ProjectAssetFormat {
    pub fn channels(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::Rgba8Srgb => 4,
        }
    }
}

/// Packed source pixels, never a CPU canvas raster. Arc storage lets a save
/// snapshot share immutable imported images and custom brush textures.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectAsset {
    pub extent: [u32; 2],
    pub format: ProjectAssetFormat,
    pub bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub document: Document,
    pub assets: BTreeMap<AssetId, ProjectAsset>,
}

/// Bounds apply to decoded data, not just compressed file size. Hosts may use
/// stricter limits for their memory budget. GPU device limits are checked later.
#[derive(Clone, Copy, Debug)]
pub struct ProjectLimits {
    pub metadata_bytes: u64,
    pub asset_bytes: u64,
    pub dimension: u32,
    pub layers: usize,
    pub strokes: usize,
    pub points: usize,
}
impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            metadata_bytes: 64 * 1024 * 1024,
            asset_bytes: 512 * 1024 * 1024,
            dimension: 32768,
            layers: 4096,
            strokes: 500_000,
            points: 8_000_000,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetRecord {
    id: AssetId,
    extent: [u32; 2],
    format: ProjectAssetFormat,
}
impl AssetRecord {
    fn size(&self, limits: ProjectLimits) -> Result<u64, String> {
        if self.extent.iter().any(|v| *v == 0 || *v > limits.dimension)
            || self.id.0.is_empty()
            || self.id.0.len() > 1024
        {
            return Err("Invalid project image".into());
        }
        u64::from(self.extent[0])
            .checked_mul(u64::from(self.extent[1]))
            .and_then(|n| n.checked_mul(u64::from(self.format.channels())))
            .filter(|n| *n <= limits.asset_bytes)
            .ok_or_else(|| "Project image exceeds the memory limit".into())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest<D = Document> {
    document: D,
    assets: Vec<AssetRecord>,
}

impl Project {
    /// Snapshot only current, reachable content. Deleted layers and obsolete
    /// strokes kept alive for undo must not leak into the saved project.
    pub fn snapshot(
        document: &Document,
        assets: &BTreeMap<AssetId, ProjectAsset>,
    ) -> Result<Self, String> {
        let mut document = document.clone();
        let (reachable, _) = history_references(&document, ProjectLimits::default())?;
        document.strokes.retain(|id, _| reachable.contains_key(id));
        let needed = asset_references(&document)?;
        let project = Self {
            document,
            assets: assets
                .iter()
                .filter(|(id, _)| needed.contains_key(*id))
                .map(|(id, a)| (id.clone(), a.clone()))
                .collect(),
        };
        project.validate(ProjectLimits::default())?;
        Ok(project)
    }

    pub fn validate(&self, limits: ProjectLimits) -> Result<(), String> {
        validate_document(&self.document, limits)?;
        let needed = asset_references(&self.document)?;
        for (id, format) in &needed {
            match self.assets.get(id) {
                Some(a) if a.format == *format => (),
                None if builtin_asset(id) && *format == ProjectAssetFormat::R8Unorm => (),
                _ => {
                    return Err(
                        "A source image or brush texture is missing or has the wrong format".into(),
                    );
                }
            }
        }
        let mut total = 0u64;
        for (id, asset) in &self.assets {
            if !needed.contains_key(id) {
                return Err("Project contains an unused asset".into());
            }
            let size = AssetRecord {
                id: id.clone(),
                extent: asset.extent,
                format: asset.format,
            }
            .size(limits)?;
            total = total
                .checked_add(size)
                .filter(|n| *n <= limits.asset_bytes)
                .ok_or("Project images exceed the memory limit")?;
            if size != asset.bytes.len() as u64 {
                return Err("Invalid project image size".into());
            }
        }
        Ok(())
    }

    /// Versioned gzip stream: metadata length, JSON, then packed image blocks
    /// in manifest order. No archive paths to extract, base64, or JSON byte arrays.
    pub fn write(&self, mut output: impl Write) -> Result<(), String> {
        let limits = ProjectLimits::default();
        self.validate(limits)?;
        let manifest = Manifest {
            document: &self.document,
            assets: self
                .assets
                .iter()
                .map(|(id, a)| AssetRecord {
                    id: id.clone(),
                    extent: a.extent,
                    format: a.format,
                })
                .collect(),
        };
        let json = metadata(&manifest, limits.metadata_bytes)?;
        output.write_all(MAGIC).map_err(io_error)?;
        let mut stream = GzEncoder::new(output, Compression::fast());
        stream
            .write_all(&(json.len() as u64).to_le_bytes())
            .map_err(io_error)?;
        stream.write_all(&json).map_err(io_error)?;
        for a in self.assets.values() {
            stream.write_all(&a.bytes).map_err(io_error)?;
        }
        stream.finish().map_err(io_error)?;
        Ok(())
    }

    pub fn read(mut input: impl Read, limits: ProjectLimits) -> Result<Self, String> {
        let mut magic = [0; MAGIC.len()];
        input.read_exact(&mut magic).map_err(io_error)?;
        if &magic != MAGIC {
            return Err("Unsupported Capy Canvas project version".into());
        }
        let mut stream = MultiGzDecoder::new(input);
        let mut size = [0; 8];
        stream.read_exact(&mut size).map_err(io_error)?;
        let json = read_block(&mut stream, u64::from_le_bytes(size), limits.metadata_bytes)?;
        let manifest: Manifest =
            serde_json::from_slice(&json).map_err(|e| format!("Invalid project metadata: {e}"))?;
        validate_document(&manifest.document, limits)?;
        let mut assets = BTreeMap::new();
        let mut remaining = limits.asset_bytes;
        for record in manifest.assets {
            if assets.contains_key(&record.id) {
                return Err("Duplicate project asset".into());
            }
            let size = record.size(limits)?;
            let bytes = read_block(&mut stream, size, remaining)?;
            remaining -= size;
            assets.insert(
                record.id,
                ProjectAsset {
                    extent: record.extent,
                    format: record.format,
                    bytes: bytes.into(),
                },
            );
        }
        let mut extra = [0];
        // Consume the gzip trailer too: truncated/corrupt saves are not success.
        if stream.read(&mut extra).map_err(io_error)? != 0 {
            return Err("Unexpected project data".into());
        }
        let project = Self {
            document: manifest.document,
            assets,
        };
        project.validate(limits)?;
        Ok(project)
    }
}

fn io_error(e: std::io::Error) -> String {
    format!("Project I/O failed: {e}")
}
fn metadata(value: &impl Serialize, limit: u64) -> Result<Vec<u8>, String> {
    struct Bounded {
        bytes: Vec<u8>,
        limit: u64,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() as u64 > self.limit.saturating_sub(self.bytes.len() as u64) {
                return Err(std::io::Error::other(
                    "Project metadata exceeds the memory limit",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut output, value).map_err(|e| e.to_string())?;
    Ok(output.bytes)
}
fn read_block(input: &mut impl Read, size: u64, limit: u64) -> Result<Vec<u8>, String> {
    if size > limit {
        return Err("Project data exceeds the memory limit".into());
    }
    let mut bytes = Vec::new();
    input.take(size).read_to_end(&mut bytes).map_err(io_error)?;
    if bytes.len() as u64 != size {
        return Err("The project is incomplete".into());
    }
    Ok(bytes)
}

fn builtin_asset(id: &AssetId) -> bool {
    [
        PENCIL_TEXTURE_ASSET,
        PAINTBRUSH_TEXTURE_ASSET,
        PAPER_GRAIN_TEXTURE_ASSET,
        BRISTLE_GRAIN_TEXTURE_ASSET,
        WATERCOLOR_TIP_TEXTURE_ASSET,
        WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET,
        WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
        WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET,
        WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
    ]
    .contains(&id.0.as_ref())
}

fn asset_references(document: &Document) -> Result<BTreeMap<AssetId, ProjectAssetFormat>, String> {
    let mut ids = BTreeMap::new();
    let mut add = |id: &AssetId, format| -> Result<(), String> {
        if ids
            .insert(id.clone(), format)
            .is_some_and(|previous| previous != format)
            || (builtin_asset(id) && format != ProjectAssetFormat::R8Unorm)
        {
            return Err("One project asset has incompatible uses".into());
        }
        Ok(())
    };
    for s in document.strokes() {
        for tip in std::iter::once(&s.brush.tip).chain(s.brush.dual.iter().map(|d| &d.tip)) {
            if let BrushTip::Mask(id) = tip {
                add(id, ProjectAssetFormat::R8Unorm)?;
            }
        }
        for grain in s
            .brush
            .grain
            .iter()
            .chain(s.brush.dual.iter().filter_map(|d| d.grain.as_ref()))
        {
            add(&grain.asset, ProjectAssetFormat::R8Unorm)?;
        }
        if let Some(t) = &s.brush.transport {
            add(&t.conductance, ProjectAssetFormat::R8Unorm)?;
        }
    }
    for l in &document.layers {
        if let Some(id) = &l.asset {
            add(id, ProjectAssetFormat::Rgba8Srgb)?;
        }
    }
    Ok(ids)
}

fn validate_selection(selection: &Selection, limits: ProjectLimits) -> Result<(), String> {
    if selection.affine.inverse().is_none() {
        return Err("Invalid selection transform".into());
    }
    match &selection.shape {
        SelectionShape::Contours(paths) => {
            if paths
                .iter()
                .any(|p| p.len() < 3 || p.iter().any(|v| !v.x.is_finite() || !v.y.is_finite()))
            {
                return Err("Invalid selection contour".into());
            }
        }
        SelectionShape::Pixels(p) => {
            p.validate().map_err(|e| e.to_string())?;
            if p.extent().iter().any(|v| *v > limits.dimension) {
                return Err("Selection exceeds the image limit".into());
            }
            // Bounds may be conservative, but must not omit nonzero coverage.
            // Check nonempty words, not each pixel, and reject row padding too.
            let stride = p.extent()[0].div_ceil(8) as usize;
            let [x0, y0, x1, y1] = p.bounds();
            for (i, &word) in p.words().iter().enumerate().filter(|(_, w)| **w != 0) {
                let y = (i / stride) as u32;
                let x = (i % stride) as u32 * 8;
                if y < y0
                    || y >= y1
                    || x + word.trailing_zeros() / 4 < x0
                    || x + 7 - word.leading_zeros() / 4 >= x1
                {
                    return Err("Selection bounds omit coverage".into());
                }
            }
        }
    }
    Ok(())
}

/// Include immutable mask histories retained by Apply mask, not only live masks.
fn history_references(
    document: &Document,
    limits: ProjectLimits,
) -> Result<(BTreeMap<StrokeId, LayerId>, u64), String> {
    fn history(
        ids: &mut BTreeMap<StrokeId, LayerId>,
        owner: LayerId,
        strokes: &[StrokeId],
        ops: &[LayerOperation],
        depth: usize,
        limits: ProjectLimits,
    ) -> Result<u64, String> {
        if depth > 8 {
            return Err("Project mask history is too deeply nested".into());
        }
        if owner.0 == 0 && !strokes.is_empty() {
            return Err("Invalid stroke target".into());
        }
        let mut max_id = owner.0;
        let mut seen = BTreeSet::new();
        for &id in strokes {
            if !seen.insert(id)
                || ids
                    .insert(id, owner)
                    .is_some_and(|previous| previous != owner)
            {
                return Err("Invalid stroke ownership".into());
            }
        }
        let mut after = 0;
        for op in ops {
            if op.after_stroke < after || op.after_stroke > strokes.len() {
                return Err("Invalid paint operation order".into());
            }
            after = op.after_stroke;
            let m = &op.coverage;
            if let Some(s) = &m.initial {
                validate_selection(s, limits)?;
            }
            max_id = max_id.max(history(
                ids,
                m.id,
                &m.strokes,
                &m.operations,
                depth + 1,
                limits,
            )?);
        }
        Ok(max_id)
    }
    let mut ids = BTreeMap::new();
    let mut max_id = 0;
    for l in &document.layers {
        max_id = max_id.max(history(
            &mut ids,
            l.id,
            &l.strokes,
            &l.operations,
            0,
            limits,
        )?);
        if let Some(m) = &l.mask {
            if let Some(s) = &m.initial {
                validate_selection(s, limits)?;
            }
            max_id = max_id.max(history(
                &mut ids,
                m.id,
                &m.strokes,
                &m.operations,
                0,
                limits,
            )?);
        }
    }
    Ok((ids, max_id))
}

fn validate_document(doc: &Document, limits: ProjectLimits) -> Result<(), String> {
    if doc.width == 0
        || doc.height == 0
        || doc.width > limits.dimension
        || doc.height > limits.dimension
        || doc.layers.is_empty()
        || doc.layers.len() > limits.layers
        || doc.strokes.len() > limits.strokes
        || doc.id.len() > 1024
        || doc.revision == u64::MAX
    {
        return Err("Invalid or oversized project document".into());
    }
    let mut ids = BTreeSet::new();
    for l in &doc.layers {
        if l.id.0 == 0 || !ids.insert(l.id) || l.name.len() > 4096 {
            return Err("Invalid project layer identity".into());
        }
    }
    for l in &doc.layers {
        if let Some(m) = &l.mask
            && (m.id.0 == 0 || !ids.insert(m.id))
        {
            return Err("Invalid project mask identity".into());
        }
    }
    let (referenced, max_id) = history_references(doc, limits)?;
    let mut points = 0usize;
    for (&id, &owner) in &referenced {
        let s = doc.stroke(id).ok_or("Project is missing a stroke")?;
        points = points
            .checked_add(s.points.len())
            .filter(|n| *n <= limits.points)
            .ok_or("Project has too many stroke points")?;
        if id.0 == 0
            || id != s.id
            || s.layer_id != owner
            || s.points.is_empty()
            || s.points.iter().any(|p| {
                ![
                    p.position.x,
                    p.position.y,
                    p.pressure,
                    p.tilt[0],
                    p.tilt[1],
                    p.twist,
                ]
                .into_iter()
                .all(f32::is_finite)
                    || !(0.0..=1.0).contains(&p.pressure)
            })
        {
            return Err("Invalid project stroke".into());
        }
        s.brush.validate().map_err(|e| e.to_string())?;
        if s.material_updates.first() == Some(&0)
            || s.material_updates.windows(2).any(|w| w[0] >= w[1])
            || s.material_updates
                .last()
                .is_some_and(|end| *end as usize != s.points.len())
        {
            return Err("Invalid stroke material updates".into());
        }
        if let Some(selection) = &s.selection {
            validate_selection(selection, limits)?;
        }
        // Bounds are part of incremental replay. Never trust forged/corrupt
        // bounds that could omit pixels even though the samples are valid.
        let mut bounds = Rect::EMPTY;
        let radius = s.brush.conservative_radius();
        for p in s.points.iter() {
            bounds.include_circle(p.position, radius);
        }
        if bounds != s.bounds
            || ![bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
                .into_iter()
                .all(f32::is_finite)
        {
            return Err("Invalid stroke bounds".into());
        }
    }
    if referenced.len() != doc.strokes.len() {
        return Err("Project contains unreachable strokes".into());
    }
    if doc.next_layer_id <= max_id
        || doc.next_layer_id == u64::MAX
        || doc.next_stroke_id <= doc.strokes.keys().last().map_or(0, |id| id.0)
        || doc.next_stroke_id == u64::MAX
    {
        return Err("Invalid project ID allocator".into());
    }
    if doc
        .layer(doc.active_layer)
        .is_none_or(|l| doc.active_mask && l.mask.is_none())
    {
        return Err("Invalid editing target".into());
    }
    if !doc.layers.iter().any(|l| l.kind == LayerKind::Paint) {
        return Err("Project has no paint layer".into());
    }
    if let Some(s) = &doc.selection {
        validate_selection(s, limits)?;
    }
    rulers::validate_rulers(&doc.rulers).map_err(|e| e.to_string())?;
    for id in &doc.reference_layers {
        if doc.layer(*id).is_none_or(|l| {
            !matches!(
                l.kind,
                LayerKind::Paint | LayerKind::ImportedImage | LayerKind::Group
            )
        }) {
            return Err("Invalid reference layer".into());
        }
    }
    for (i, l) in doc.layers.iter().enumerate() {
        let mut parent = l.properties.parent;
        for _ in 0..32 {
            parent = match parent {
                None => break,
                Some(id) => {
                    doc.layer(id)
                        .ok_or("Missing layer group")?
                        .properties
                        .parent
                }
            };
        }
        if parent.is_some() {
            return Err("Layer groups are too deeply nested or cyclic".into());
        }
        doc.validate_layer(l).map_err(|e| e.to_string())?;
        if l.kind == LayerKind::Background && i + 1 != doc.layers.len() {
            return Err("Paper must be the bottom layer".into());
        }
        if (matches!(l.kind, LayerKind::ImportedImage | LayerKind::AiSuggestion)
            && l.asset.is_none())
            || (!matches!(
                l.kind,
                LayerKind::Paint | LayerKind::ImportedImage | LayerKind::AiSuggestion
            ) && l.asset.is_some())
        {
            return Err("Invalid layer source image".into());
        }
        if l.kind != LayerKind::Paint && (!l.strokes.is_empty() || !l.operations.is_empty()) {
            return Err("Non-paint layer contains paint history".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(doc: &mut Document, layer: LayerId, preset: DefaultBrushPreset) {
        let id = doc.allocate_stroke_id();
        let s = Stroke::new(
            id,
            layer,
            StrokeTool::Brush,
            default_brush(preset),
            Arc::from([
                StrokePoint {
                    position: Point { x: 7.25, y: 12.75 },
                    pressure: 0.35,
                    tilt: [0.2, -0.3],
                    twist: 0.7,
                    elapsed_micros: 0,
                },
                StrokePoint {
                    position: Point { x: 42.5, y: 39.5 },
                    pressure: 0.95,
                    tilt: [0.4, -0.2],
                    twist: 0.9,
                    elapsed_micros: 8200,
                },
            ]),
        )
        .unwrap();
        doc.apply(Edit::InsertStroke(Box::new(s))).unwrap();
    }
    fn encode(manifest: &Manifest, payload: &[u8]) -> Vec<u8> {
        let json = serde_json::to_vec(manifest).unwrap();
        let mut out = MAGIC.to_vec();
        let mut gzip = GzEncoder::new(&mut out, Compression::fast());
        gzip.write_all(&(json.len() as u64).to_le_bytes()).unwrap();
        gzip.write_all(&json).unwrap();
        gzip.write_all(payload).unwrap();
        gzip.finish().unwrap();
        out
    }
    fn roundtrip(project: &Project) -> Project {
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        Project::read(bytes.as_slice(), ProjectLimits::default()).unwrap()
    }
    #[test]
    fn editable_project_roundtrips_brushes_masks_operations_and_runtime_filters() {
        let mut doc = Document::new("project-fixture", 64, 64);
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::Pencil,
            DefaultBrushPreset::WatercolorWash,
            DefaultBrushPreset::WetWatercolor,
            DefaultBrushPreset::LoadedOil,
            DefaultBrushPreset::NaturalBlender,
            DefaultBrushPreset::PaletteKnife,
            DefaultBrushPreset::DualTexture,
            DefaultBrushPreset::LiquifyTwirl,
        ] {
            stroke(&mut doc, LayerId(1), preset);
        }
        let mask_id = doc.allocate_layer_id();
        let selection = Selection::polygon(vec![
            Point::default(),
            Point { x: 50., y: 0. },
            Point { x: 50., y: 50. },
        ])
        .unwrap();
        let mut mask = LayerMask::reveal_all(mask_id, Point { x: 2., y: -3. });
        mask.initial = Some(selection.clone());
        mask.linked = false;
        doc.layers[0].mask = Some(mask);
        stroke(&mut doc, mask_id, DefaultBrushPreset::GPen);
        let applied = doc.layers[0].mask.take().unwrap();
        doc.layers[0].operations.push(LayerOperation {
            after_stroke: 3,
            coverage: applied,
            kind: LayerOperationKind::ApplyMask,
        });
        for kind in [
            LayerOperationKind::Fill {
                color: [0.2, 0.3, 0.4, 0.5],
                alpha_locked: true,
            },
            LayerOperationKind::Gradient {
                start: Point::default(),
                end: Point { x: 64., y: 12. },
                colors: [[0.1, 0.2, 0.3, 1.], [0.; 4]],
                radial: true,
                alpha_locked: false,
            },
            LayerOperationKind::Transform(ImageTransform {
                affine: Affine::translation(Point { x: 4., y: 7. }),
                interpolation: Interpolation::Linear,
            }),
        ] {
            doc.layers[0].operations.push(LayerOperation {
                after_stroke: 9,
                coverage: LayerMask::reveal_all(LayerId(0), Point::default()),
                kind,
            });
        }
        doc.layers[0].mask = Some(LayerMask::reveal_all(
            doc.allocate_layer_id(),
            Point::default(),
        ));
        doc.selection = Some(Selection::pixels(Arc::new(
            SelectionPixels::new([8, 2], [0, 0, 8, 2], Arc::from([0x12344321, 0x44444444]))
                .unwrap(),
        )));
        doc.reference_layers.insert(LayerId(1));
        doc.rulers.push(Ruler {
            id: 1,
            geometry: RulerGeometry::Parallel {
                start: Point::default(),
                end: Point { x: 20., y: 10. },
            },
        });
        doc.active_mask = true;
        for definition in bundled_effect_catalog().filters() {
            let mut layer = Layer::paint(doc.allocate_layer_id(), definition.label());
            layer.kind = LayerKind::Effect;
            layer.properties.clipped = true;
            layer.effect = Some(Arc::new(definition.preview().unwrap()));
            doc.layers.insert(0, layer);
        }
        let project = Project::snapshot(&doc, &BTreeMap::new()).unwrap();
        assert_eq!(roundtrip(&project), project);
        // Opening does not require the current runtime catalog: the exact WGSL
        // and declarative program used by each effect travel with the document.
        assert_eq!(
            project
                .document
                .layers
                .iter()
                .filter(|l| l.effect.is_some())
                .count(),
            40
        );
    }

    #[test]
    fn project_embeds_only_reachable_assets_and_paint() {
        let mut doc = Document::new("assets", 64, 64);
        let image: AssetId = "image-1".into();
        doc.layers[0].asset = Some(image.clone()); // Imported editable paint.
        let deleted = doc.allocate_layer_id();
        doc.apply(Edit::InsertLayer {
            index: 0,
            layer: Layer::paint(deleted, "Deleted content"),
        })
        .unwrap();
        stroke(&mut doc, deleted, DefaultBrushPreset::GPen);
        doc.apply(Edit::RemoveLayer { id: deleted }).unwrap();
        assert_eq!(doc.strokes().count(), 1); // Still held for document undo.
        let a = ProjectAsset {
            extent: [2, 2],
            format: ProjectAssetFormat::Rgba8Srgb,
            bytes: Arc::from([18, 118, 250, 128].repeat(4)),
        };
        let assets = BTreeMap::from([
            (image.clone(), a.clone()),
            (AssetId::from("unused-private-image"), a.clone()),
        ]);
        let project = Project::snapshot(&doc, &assets).unwrap();
        assert_eq!(project.document.strokes().count(), 0);
        assert_eq!(project.assets.len(), 1);
        assert!(Arc::ptr_eq(&project.assets[&image].bytes, &a.bytes));
        assert_eq!(roundtrip(&project), project);
        assert!(Project::snapshot(&doc, &BTreeMap::new()).is_err());
    }

    #[test]
    fn project_rejects_broken_histories_ids_and_selection_data() {
        let mut doc = Document::new("validation", 64, 64);
        stroke(&mut doc, LayerId(1), DefaultBrushPreset::GPen);
        let base = serde_json::to_value(&doc).unwrap();
        for path in [
            "missing_stroke",
            "bounds",
            "duplicate_layer",
            "mask_collision",
            "allocator",
            "group_cycle",
            "selection_words",
            "missing_edit_target",
            "paper_order",
            "operation_order",
        ] {
            let mut d = doc.clone();
            match path {
                "missing_stroke" => d.layers[0].strokes.push(StrokeId(99)),
                "bounds" => d.strokes.get_mut(&StrokeId(1)).unwrap().bounds.max.x = 0.,
                "duplicate_layer" => d.layers[1].id = LayerId(1),
                "mask_collision" => {
                    d.layers[0].mask = Some(LayerMask::reveal_all(LayerId(2), Point::default()))
                }
                "allocator" => d.next_stroke_id = 1,
                "group_cycle" => d.layers[0].properties.parent = Some(LayerId(1)),
                "selection_words" => {
                    let mut value = base.clone();
                    value["selection"] = serde_json::json!({"affine": [1.,0.,0.,1.,0.,0.], "inverted": false, "shape": {"Pixels": {"extent": [8,1], "bounds": [0,0,8,1], "words": [15]}}});
                    d = serde_json::from_value(value).unwrap();
                }
                "missing_edit_target" => d.active_layer = LayerId(99),
                "paper_order" => d.layers.swap(0, 1),
                "operation_order" => d.layers[0].operations.push(LayerOperation {
                    after_stroke: 2,
                    coverage: LayerMask::reveal_all(LayerId(0), Point::default()),
                    kind: LayerOperationKind::Fill {
                        color: [1.; 4],
                        alpha_locked: false,
                    },
                }),
                _ => unreachable!(),
            }
            let bytes = encode(
                &Manifest {
                    document: d,
                    assets: vec![],
                },
                &[],
            );
            assert!(
                Project::read(bytes.as_slice(), ProjectLimits::default()).is_err(),
                "accepted {path}"
            );
        }
    }

    #[test]
    fn project_stream_checks_limits_versions_truncation_and_checksums() {
        let project = Project::snapshot(&Document::new("blank", 64, 64), &BTreeMap::new()).unwrap();
        assert!(metadata(&project.document, 10).is_err());
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        for end in [0, 8, 12, bytes.len() / 2, bytes.len() - 1] {
            assert!(Project::read(&bytes[..end], ProjectLimits::default()).is_err());
        }
        let mut corrupt = bytes.clone();
        corrupt[11] = 99;
        assert!(Project::read(corrupt.as_slice(), ProjectLimits::default()).is_err());
        let end = corrupt.len();
        corrupt = bytes.clone();
        corrupt[end - 8] ^= 1;
        assert!(Project::read(corrupt.as_slice(), ProjectLimits::default()).is_err());
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    metadata_bytes: 10,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    dimension: 32,
                    ..Default::default()
                }
            )
            .is_err()
        );
        let extra = encode(
            &Manifest {
                document: project.document,
                assets: vec![],
            },
            &[1],
        );
        assert!(Project::read(extra.as_slice(), ProjectLimits::default()).is_err());
        struct FullDisk;
        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(
            Project::snapshot(&Document::new("blank", 64, 64), &BTreeMap::new())
                .unwrap()
                .write(FullDisk)
                .is_err()
        );
    }

    #[test]
    fn asset_roles_and_decoded_budgets_are_validated() {
        let mut doc = Document::new("asset validation", 64, 64);
        stroke(&mut doc, LayerId(1), DefaultBrushPreset::GPen);
        let id: AssetId = "custom-tip".into();
        doc.strokes.get_mut(&StrokeId(1)).unwrap().brush.tip = BrushTip::Mask(id.clone());
        let a = ProjectAsset {
            extent: [2, 2],
            format: ProjectAssetFormat::R8Unorm,
            bytes: Arc::from([0, 255, 255, 0]),
        };
        let mut project = Project::snapshot(&doc, &BTreeMap::from([(id.clone(), a)])).unwrap();
        assert_eq!(roundtrip(&project), project);
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    asset_bytes: 3,
                    ..Default::default()
                }
            )
            .is_err()
        );
        project.document.layers[0].asset = Some(id.clone());
        assert!(
            project.validate(ProjectLimits::default()).is_err(),
            "mask cannot also be an RGBA image"
        );
        project.document.layers[0].asset = None;
        project.assets.get_mut(&id).unwrap().format = ProjectAssetFormat::Rgba8Srgb;
        project.assets.get_mut(&id).unwrap().bytes = Arc::from([0; 16]);
        assert!(project.validate(ProjectLimits::default()).is_err());
        let records = || {
            vec![AssetRecord {
                id: id.clone(),
                extent: [2, 2],
                format: ProjectAssetFormat::R8Unorm,
            }]
        };
        let mut duplicate = records();
        duplicate.extend(records());
        let data = encode(
            &Manifest {
                document: doc.clone(),
                assets: duplicate,
            },
            &[0; 8],
        );
        assert!(Project::read(data.as_slice(), ProjectLimits::default()).is_err());
        let truncated = encode(
            &Manifest {
                document: doc,
                assets: records(),
            },
            &[0; 3],
        );
        assert!(Project::read(truncated.as_slice(), ProjectLimits::default()).is_err());
    }

    #[test]
    fn selection_bounds_history_allocators_and_group_depth_are_checked() {
        let mut doc = Document::new("bounds", 64, 64);
        let check = |doc: &Document| validate_document(doc, ProjectLimits::default());
        doc.selection = Some(Selection::pixels(Arc::new(
            SelectionPixels::new([8, 1], [1, 0, 8, 1], [4].to_vec()).unwrap(),
        )));
        assert!(check(&doc).is_err(), "coverage outside declared bounds");
        doc.selection = Some(Selection::pixels(Arc::new(
            SelectionPixels::new([1, 1], [0, 0, 1, 1], [0x40].to_vec()).unwrap(),
        )));
        assert!(check(&doc).is_err(), "nonzero coverage in row padding");
        doc.selection = None;
        doc.layers[0].operations.push(LayerOperation {
            after_stroke: 0,
            coverage: LayerMask::reveal_all(LayerId(8), Point::default()),
            kind: LayerOperationKind::ApplyMask,
        });
        assert!(
            check(&doc).is_err(),
            "retained mask IDs must not be reallocated"
        );
        doc.next_layer_id = 9;
        assert!(check(&doc).is_ok());
        let mut parent = None;
        for _ in 0..34 {
            let mut group = Layer::paint(doc.allocate_layer_id(), "Group");
            group.kind = LayerKind::Group;
            group.properties.parent = parent;
            parent = Some(group.id);
            doc.layers.insert(0, group);
        }
        assert!(check(&doc).is_err());
    }
}
