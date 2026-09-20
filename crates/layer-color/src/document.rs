//! Atomic, worker-owned SDR interpretation and backing changes. Immutable source
//! originals and sRGB-defined effect parameters retain their own color meaning.
use crate::{OutputStatistics, WorkingDecoder, WorkingEncoder};
use layer_core::{Edit, Project, color::source::*, color::*, raster::*};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DocumentColorChange {
    Assign(RgbSpace),
    Convert {
        space: RgbSpace,
        options: ConversionOptions,
    },
    Depth {
        depth: SampleDepth,
        dither: OutputDither,
    },
}
impl DocumentColorChange {
    pub fn target(self, mut color: DocumentColor) -> DocumentColor {
        match self {
            Self::Assign(space) | Self::Convert { space, .. } => color.space = space,
            Self::Depth { depth, .. } => color.depth = depth,
        }
        color
    }
    fn encoding(self) -> OutputEncoding {
        match self {
            Self::Convert { options, .. } => OutputEncoding {
                conversion: options,
                ..Default::default()
            },
            Self::Depth { dither, .. } => OutputEncoding {
                dither,
                ..Default::default()
            },
            Self::Assign(_) => Default::default(),
        }
    }
}

pub struct PreparedDocumentColor {
    pub project: Project,
    pub statistics: OutputStatistics,
    /// New compressed backing and conservative index/cache allocation charge.
    /// Unchanged original/document ownership is separate from this job's limit.
    pub allocated_bytes: usize,
}
impl PreparedDocumentColor {
    pub fn edit(&self) -> Edit {
        Edit::SetColor {
            color: self.project.document.color,
            layers: self.project.document.layers.clone(),
        }
    }
}

struct Converter<'a> {
    old: DocumentColor,
    target: DocumentColor,
    change: DocumentColorChange,
    decoder: WorkingDecoder,
    encoder: WorkingEncoder,
    cancelled: &'a mut dyn FnMut() -> bool,
    bytes: usize,
    limit: usize,
    statistics: OutputStatistics,
    blobs: HashMap<(usize, [u32; 2]), Arc<TileBlob>>,
    roots: HashMap<u64, RasterRevision>,
    images: HashMap<usize, Arc<SourceImage>>,
}
impl Converter<'_> {
    fn check(&mut self) -> Result<(), String> {
        if (self.cancelled)() {
            Err("Document color change cancelled".into())
        } else {
            Ok(())
        }
    }
    fn charge(&mut self, bytes: usize) -> Result<(), String> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.limit {
            Err("Document color change exceeds the prepared-data memory limit".into())
        } else {
            Ok(())
        }
    }
    fn wait<T>(
        &mut self,
        mut ready: impl FnMut() -> Option<Result<T, String>>,
    ) -> Result<T, String> {
        // Browser transport resolves backing before handing this synchronous
        // worker a project. Never sleep or use native clocks in Wasm.
        #[cfg(target_arch = "wasm32")]
        {
            self.check()?;
            ready().unwrap_or_else(|| {
                Err("Resolve raster backing before browser color conversion".into())
            })
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                self.check()?;
                if let Some(result) = ready() {
                    return result;
                }
                if Instant::now() >= deadline {
                    return Err("Raster backing did not complete for the color change".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
    fn blob(
        &mut self,
        original: &Arc<TileBlob>,
        coordinate: [u32; 2],
    ) -> Result<Arc<TileBlob>, String> {
        self.check()?;
        let scalar = original.descriptor.channels == 1;
        if scalar && self.old.depth.coverage() == self.target.depth.coverage()
            || matches!(self.change, DocumentColorChange::Assign(_))
                && original.descriptor == self.target.paint_descriptor()
        {
            return Ok(original.clone());
        }
        let coordinate = if self.change.encoding().dither == OutputDither::None {
            [0, 0]
        } else {
            coordinate
        };
        let key = (Arc::as_ptr(original) as usize, coordinate);
        if let Some(blob) = self.blobs.get(&key) {
            return Ok(blob.clone());
        }
        let decoded = original.decode()?;
        let descriptor = if scalar {
            self.target.coverage_descriptor()
        } else {
            self.target.paint_descriptor()
        };
        let mut encoded = vec![
            0;
            descriptor
                .byte_len([TILE_SIZE; 2])
                .ok_or("Invalid target layout")?
        ];
        if scalar {
            for (old, new) in decoded
                .chunks_exact(self.old.depth.coverage().bytes())
                .zip(encoded.chunks_exact_mut(self.target.depth.coverage().bytes()))
            {
                match self.target.depth.coverage() {
                    SampleDepth::U16 => new.copy_from_slice(&(old[0] as u16 * 257).to_le_bytes()),
                    SampleDepth::F16 | SampleDepth::F32 => unreachable!("coverage is integer"),
                    SampleDepth::U8 => {
                        new[0] = ((u16::from_le_bytes([old[0], old[1]]) as u32 + 128) / 257) as u8
                    }
                }
            }
        } else {
            let mut linear = [[0.; 4]; TILE_SIZE as usize];
            let old_stride =
                TILE_SIZE as usize * self.old.paint_descriptor().bytes_per_pixel().unwrap();
            let new_stride = TILE_SIZE as usize * descriptor.bytes_per_pixel().unwrap();
            for (y, (old, new)) in decoded
                .chunks_exact(old_stride)
                .zip(encoded.chunks_exact_mut(new_stride))
                .enumerate()
            {
                self.check()?;
                self.decoder.decode_pixels(old, &mut linear)?;
                let origin = [
                    coordinate[0] * TILE_SIZE,
                    coordinate[1] * TILE_SIZE + y as u32,
                ];
                let stats = if original.descriptor.alpha == AlphaAssociation::PremultipliedLinear {
                    self.encoder
                        .encode_premultiplied(&linear, new, None, origin)?
                } else {
                    self.encoder.encode_straight(&linear, new, None, origin)?
                };
                self.statistics.clipped_channels += stats.clipped_channels;
            }
        }
        self.check()?;
        // This runs on a document worker, so favor compact retained backing.
        let blob = Arc::new(TileBlob::encode(descriptor, &encoded)?);
        self.charge(blob.resident_bytes().saturating_add(128))?;
        self.blobs.insert(key, blob.clone());
        Ok(blob)
    }
    fn root(
        &mut self,
        original: &RasterRevision,
        extent: [u32; 2],
        mask: bool,
    ) -> Result<RasterRevision, String> {
        self.check()?;
        let data = self.wait(|| original.try_data())?;
        data.validate_index(extent, mask, self.old)?;
        if let Some(root) = self.roots.get(&original.identity()) {
            return Ok(root.clone());
        }
        let mut tiles = BTreeMap::new();
        let mut changed = false;
        for (key, tile) in &data.tiles {
            let blob = self.wait(|| tile.try_backing())?;
            let converted = self.blob(&blob, key.coordinate)?;
            let replacement = if Arc::ptr_eq(&blob, &converted) {
                tile.clone()
            } else {
                changed = true;
                RasterTile::backed_shared(converted)
            };
            tiles.insert(*key, replacement);
        }
        let result = if changed {
            self.charge(tiles.len().saturating_mul(128).saturating_add(128))?;
            RasterRevision::backed(RasterData {
                tiles,
                watercolor: data.watercolor,
            })
        } else {
            original.clone()
        };
        self.charge(64)?;
        self.roots.insert(original.identity(), result.clone());
        Ok(result)
    }
    fn image(&mut self, original: &Arc<SourceImage>) -> Result<Arc<SourceImage>, String> {
        if original.is_original() {
            return Ok(original.clone());
        }
        self.check()?;
        let key = Arc::as_ptr(original) as usize;
        if let Some(image) = self.images.get(&key) {
            return Ok(image.clone());
        }
        let mut converted = original.as_ref().clone();
        converted.interpretation.depth = self.target.depth;
        converted.interpretation.profile = ColorProfile::Builtin(self.target.space);
        for (coordinate, tile) in &mut converted.tiles {
            *tile = self.blob(tile, *coordinate)?;
        }
        self.charge(
            converted
                .tiles
                .len()
                .saturating_mul(128)
                .saturating_add(256),
        )?;
        let converted = Arc::new(converted);
        self.images.insert(key, converted.clone());
        Ok(converted)
    }
}

/// Prepare complete immutable layers; never alter the live document or original
/// source samples. The caller admits total project/history ownership separately.
pub fn prepare_document_color(
    project: &Project,
    change: DocumentColorChange,
    max_bytes: usize,
    mut cancelled: impl FnMut() -> bool,
) -> Result<PreparedDocumentColor, String> {
    if cancelled() {
        return Err("Document color change cancelled".into());
    }
    let mut candidate = project.clone().pruned()?;
    let old = candidate.document.color;
    let target = change.target(old);
    if old.depth.is_float() && !target.depth.is_float() { return Err("Export an SDR rendition to reduce HDR range; the editable HDR master remains unchanged".into()); }
    change.encoding().validate(target.depth)?;
    if target == old {
        return Ok(PreparedDocumentColor {
            project: candidate,
            statistics: Default::default(),
            allocated_bytes: 0,
        });
    }
    // Effect colors retain their defining RGB space. GPU preparation derives
    // new document coordinates; assignment/conversion never rewrites definitions.
    if candidate.document.layers.iter().any(|l| {
        !l.pending_operations.is_empty()
            || l.mask
                .as_ref()
                .is_some_and(|m| !m.pending_operations.is_empty())
    }) {
        return Err("Finish the pending raster operation before changing document color".into());
    }
    let source = SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: old.depth,
        profile: ColorProfile::Builtin(old.space),
        profile_assumed: false,
    };
    // Assignment keeps numeric coordinates. Only an explicitly premultiplied
    // attachment needs canonicalization; that encoding stays in the old space.
    let destination = SourceInterpretation {
        depth: target.depth,
        profile: ColorProfile::Builtin(if matches!(change, DocumentColorChange::Assign(_)) {
            old.space
        } else {
            target.space
        }),
        ..source.clone()
    };
    let mut converter = Converter {
        old,
        target,
        change,
        decoder: WorkingDecoder::new(&source, old.space, Default::default())?,
        encoder: WorkingEncoder::new(old.space, &destination, change.encoding())?,
        cancelled: &mut cancelled,
        bytes: 0,
        limit: max_bytes,
        statistics: Default::default(),
        blobs: HashMap::new(),
        roots: HashMap::new(),
        images: HashMap::new(),
    };
    let mut layers = candidate.document.layers.clone();
    let extent = [candidate.document.width, candidate.document.height];
    for layer in &mut layers {
        layer.raster = converter.root(&layer.raster, extent, false)?;
        if let Some(mask) = &mut layer.mask {
            mask.raster = converter.root(&mask.raster, extent, true)?;
        }
        if let Some(source) = &mut layer.source {
            *source = converter.image(source)?;
        }
    }
    converter.check()?;
    candidate
        .document
        .apply(Edit::SetColor {
            color: target,
            layers,
        })
        .map_err(|e| e.to_string())?;
    candidate.validate(Default::default())?;
    Ok(PreparedDocumentColor {
        project: candidate,
        statistics: converter.statistics,
        allocated_bytes: converter.bytes,
    })
}

#[cfg(test)]
mod tests;
