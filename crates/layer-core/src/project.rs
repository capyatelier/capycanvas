//! Native project transport. Hosts supply streams and perform atomic file I/O
//! off the drawing thread. No filenames, GPU handles or platform state are saved.
use crate::*;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectAssetFormat {
    R8Unorm,
    Rgba8Srgb,
}
impl ProjectAssetFormat {
    pub fn descriptor(self) -> color::PixelDescriptor {
        match self {
            Self::R8Unorm => color::PixelDescriptor::COVERAGE8,
            Self::Rgba8Srgb => color::PixelDescriptor::SRGB8_STRAIGHT,
        }
    }
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
impl ProjectAsset {
    /// Retain only tightly packed source rows, excluding host-buffer padding.
    /// This is an import operation, never a canvas raster or GPU readback.
    pub fn copy_rows(
        extent: [u32; 2],
        format: ProjectAssetFormat,
        stride: usize,
        bytes: &[u8],
    ) -> Result<Self, String> {
        let row = (extent[0] as usize)
            .checked_mul(format.channels() as usize)
            .ok_or("Project image size overflow")?;
        let size = stride
            .checked_mul(extent[1] as usize)
            .ok_or("Project image size overflow")?;
        if extent.contains(&0) || stride < row || bytes.len() < size {
            return Err("Incomplete project image".into());
        }
        let mut packed = Vec::new();
        packed
            .try_reserve_exact(row * extent[1] as usize)
            .map_err(|_| "Project image allocation failed")?;
        for source in bytes[..size].chunks_exact(stride) {
            packed.extend_from_slice(&source[..row]);
        }
        Ok(Self {
            extent,
            format,
            bytes: packed.into(),
        })
    }
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
    pub raster_bytes: u64,
    pub tiles: usize,
    pub dimension: u32,
    pub layers: usize,
}
impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            metadata_bytes: 64 * 1024 * 1024,
            asset_bytes: 512 * 1024 * 1024,
            raster_bytes: 1024 * 1024 * 1024,
            tiles: 16384,
            dimension: 32768,
            layers: 4096,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssetRecord {
    pub(super) id: AssetId,
    pub(super) extent: [u32; 2],
    pub(super) format: ProjectAssetFormat,
}
impl AssetRecord {
    pub(super) fn size(&self, limits: ProjectLimits) -> Result<u64, String> {
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

impl Project {
    /// Snapshot current reachable raster and source content. History is separate.
    pub fn snapshot(
        document: &Document,
        assets: &BTreeMap<AssetId, ProjectAsset>,
    ) -> Result<Self, String> {
        Self::snapshot_with(document, |id| assets.get(id).cloned())
    }

    /// Resolve only referenced immutable resources. Renderers can share their
    /// retained source bytes without copying every loaded texture or reading GPU
    /// canvas pixels. Supplied built-in textures are embedded exactly too.
    pub fn snapshot_with(
        document: &Document,
        mut source: impl FnMut(&AssetId) -> Option<ProjectAsset>,
    ) -> Result<Self, String> {
        let document = document.clone();
        let needed = asset_references(&document)?;
        let project = Self {
            document,
            assets: needed
                .keys()
                .filter_map(|id| source(id).map(|asset| (id.clone(), asset)))
                .collect(),
        };
        project.validate(ProjectLimits::default())?;
        Ok(project)
    }

    /// Run on the file worker after capturing an immutable session snapshot.
    /// Takes ownership so pruning does not clone the document again.
    pub fn pruned(mut self) -> Result<Self, String> {
        let needed = asset_references(&self.document)?;
        self.assets.retain(|id, _| needed.contains_key(id));
        self.validate(ProjectLimits::default())?;
        Ok(self)
    }

    pub fn validate(&self, limits: ProjectLimits) -> Result<(), String> {
        validate_document(&self.document, limits)?;
        let needed = asset_references(&self.document)?;
        for (id, format) in &needed {
            match self.assets.get(id) {
                Some(a) if a.format == *format => (),
                _ => {
                    return Err("A source image is missing or has the wrong format".into());
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

    /// Indexed lossless raster transport. Run on a file worker: pending captures
    /// are awaited here, never by the input owner.
    pub fn write(&self, output: impl Write) -> Result<(), String> {
        crate::project_storage::write(self, output)
    }

    pub fn read(input: impl Read, limits: ProjectLimits) -> Result<Self, String> {
        crate::project_storage::read(input, limits)
    }
}

pub(super) fn io_error(e: std::io::Error) -> String {
    format!("Project I/O failed: {e}")
}

pub(super) fn metadata(value: &impl Serialize, limit: u64) -> Result<Vec<u8>, String> {
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
pub(super) fn read_block(input: &mut impl Read, size: u64, limit: u64) -> Result<Vec<u8>, String> {
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

fn asset_references(document: &Document) -> Result<BTreeMap<AssetId, ProjectAssetFormat>, String> {
    let mut ids = BTreeMap::new();
    let mut add = |id: &AssetId, format| -> Result<(), String> {
        if ids
            .insert(id.clone(), format)
            .is_some_and(|previous| previous != format)
        {
            return Err("One project asset has incompatible uses".into());
        }
        Ok(())
    };
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

pub(super) fn validate_document(doc: &Document, limits: ProjectLimits) -> Result<(), String> {
    if doc.width == 0
        || doc.height == 0
        || doc.width > limits.dimension
        || doc.height > limits.dimension
        || doc.layers.is_empty()
        || doc.layers.len() > limits.layers
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
    let max_id = ids.iter().map(|id| id.0).max().unwrap_or(0);
    if doc.next_layer_id <= max_id
        || doc.next_layer_id == u64::MAX
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
    }
    Ok(())
}
