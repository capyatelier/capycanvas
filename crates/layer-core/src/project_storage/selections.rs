//! Selection words are lossless little-endian binary chunks, using the same
//! bounded compression/integrity transport as raster tiles. Placement stays in
//! the document; shared immutable masks are restored once, even with many layers.
use super::*;

const CHUNK_BYTES: usize = (TILE_SIZE * TILE_SIZE) as usize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PixelsRecord {
    extent: [u32; 2],
    bounds: [u32; 4],
    byte_coverage: bool,
    chunks: Vec<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rewrite(bytes: &[u8], legacy: bool, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
        let size = u64::from_le_bytes(bytes[12..20].try_into().unwrap()) as usize;
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&bytes[52..52 + size]).unwrap();
        edit(&mut manifest);
        let json = serde_json::to_vec(&manifest).unwrap();
        let mut output = if legacy { LEGACY_MAGIC } else { MAGIC }.to_vec();
        output.extend_from_slice(&(json.len() as u64).to_le_bytes());
        output.extend_from_slice(&Sha256::digest(&json));
        output.extend_from_slice(&json);
        output.extend_from_slice(&bytes[52 + size..]);
        output
    }

    #[test]
    fn photo_selection_roundtrips_shared_binary_coverage() {
        let extent = [9504, 6336];
        let mut document = Document::new("61 MP selection", extent[0], extent[1]);
        let pixels = Arc::new(
            SelectionPixels::bytes(
                extent,
                [0, 0, extent[0], extent[1]],
                vec![0xff7f3f01; (extent[0] / 4 * extent[1]) as usize],
            )
            .unwrap(),
        );
        let selection = Selection {
            affine: Affine([1., 0., 0., 1., 2., 3.]),
            inverted: true,
            ..Selection::pixels(pixels)
        };
        document.selection = Some(selection.clone());
        let id = document.allocate_layer_id();
        document
            .layers
            .insert(0, Layer::selection(id, "Saved", selection.clone()));
        let id = document.allocate_layer_id();
        let mut mask = LayerMask::reveal_all(id, Point::default());
        mask.initial = Some(selection.clone());
        document.layers[1].mask = Some(mask);
        let project = Project {
            document,
            assets: Default::default(),
        };
        let mut transferred = project.document.clone();
        let mut blocks = Vec::new();
        let index = SelectionIndex::detach(&mut transferred, |pixels, range| {
            let mut bytes = vec![0; CHUNK_BYTES];
            for (word, dest) in pixels.words()[range].iter().zip(bytes.chunks_exact_mut(4)) {
                dest.copy_from_slice(&word.to_le_bytes());
            }
            blocks.push(bytes); Ok(blocks.len() - 1)
        }).unwrap();
        assert!(serde_json::to_vec(&transferred).unwrap().len() < 4096);
        assert!(serde_json::to_vec(&index).unwrap().len() < 128 * 1024);
        assert!(index.attach(&mut transferred.clone(), ProjectLimits {
            raster_bytes: 1024, ..Default::default()
        }, |id| Ok(blocks[id].clone())).is_err());
        index.attach(&mut transferred, Default::default(), |id| Ok(std::mem::take(&mut blocks[id]))).unwrap();
        assert_eq!(transferred.selection, project.document.selection);
        let SelectionShape::Pixels(current) = &transferred.selection.as_ref().unwrap().shape else { panic!() };
        let SelectionShape::Pixels(saved) = &transferred.layers[0].selection.as_ref().unwrap().shape else { panic!() };
        assert!(Arc::ptr_eq(current, saved));
        let mut bytes = Vec::new();
        project.write(&mut bytes).unwrap();
        let metadata_bytes = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        assert!(
            metadata_bytes < 128 * 1024,
            "Mask pixels belong in binary payload: {metadata_bytes}"
        );
        let restored = Project::read(bytes.as_slice(), Default::default()).unwrap();
        assert_eq!(restored.document.selection, Some(selection));
        let current = match &restored.document.selection.as_ref().unwrap().shape {
            SelectionShape::Pixels(p) => p,
            _ => panic!(),
        };
        for selected in [
            restored.document.layers[0].selection.as_ref(),
            restored.document.layers[1]
                .mask
                .as_ref()
                .unwrap()
                .initial
                .as_ref(),
        ] {
            let SelectionShape::Pixels(other) = &selected.unwrap().shape else {
                panic!()
            };
            assert!(Arc::ptr_eq(current, other));
        }
        let limits = ProjectLimits {
            raster_bytes: current.words().len() as u64 * 4,
            ..Default::default()
        };
        restored.validate(limits).unwrap();
        assert!(
            Project::read(
                bytes.as_slice(),
                ProjectLimits {
                    raster_bytes: 1024,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .contains("Selection coverage")
        );
        for edit in [
            |v: &mut serde_json::Value| {
                v["selections"]["pixels"][0]["bounds"] = serde_json::json!([1, 0, 9504, 6336])
            },
            |v: &mut serde_json::Value| v["selections"]["pixels"][0]["chunks"][0] = 999999.into(),
            |v: &mut serde_json::Value| v["selections"]["bindings"][0]["pixels"] = 999.into(),
            |v: &mut serde_json::Value| {
                v["selections"]["bindings"][1] = v["selections"]["bindings"][0].clone()
            },
        ] {
            assert!(
                Project::read(rewrite(&bytes, false, edit).as_slice(), Default::default()).is_err()
            );
        }
        let last = bytes.len() - 1;
        bytes[last] ^= 0x80;
        assert!(Project::read(bytes.as_slice(), Default::default()).is_err());
    }

    #[test]
    fn version_six_inline_pixels_migrate_without_precision_loss() {
        for pixels in [
            SelectionPixels::new([9, 1], [0, 0, 9, 1], vec![0x43210432, 4]).unwrap(),
            SelectionPixels::bytes([5, 1], [0, 0, 5, 1], vec![0xff807f01, 128]).unwrap(),
        ] {
            let project = Project {
                document: Document::new("Legacy", 9, 1),
                assets: Default::default(),
            };
            let selection = Selection::pixels(Arc::new(pixels));
            let mut bytes = Vec::new();
            project.write(&mut bytes).unwrap();
            let legacy = rewrite(&bytes, true, |v| {
                v.as_object_mut().unwrap().remove("selections");
                v["document"]["selection"] = serde_json::to_value(&selection).unwrap();
            });
            let restored = Project::read(legacy.as_slice(), Default::default()).unwrap();
            assert_eq!(restored.document.selection, Some(selection));
            let mut upgraded = Vec::new();
            restored.write(&mut upgraded).unwrap();
            assert_eq!(&upgraded[..12], MAGIC);
            let reread = Project::read(upgraded.as_slice(), Default::default()).unwrap();
            assert_eq!(reread.document.selection, restored.document.selection);
        }
    }
}
impl PixelsRecord {
    fn words(&self) -> u64 {
        u64::from(self.extent[0].div_ceil(if self.byte_coverage { 4 } else { 8 }))
            * u64::from(self.extent[1])
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    target: Target,
    pixels: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Target {
    Current,
    Layer(LayerId),
    Mask(LayerId),
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionIndex {
    pixels: Vec<PixelsRecord>,
    bindings: Vec<Binding>,
}

fn selections(document: &mut Document) -> impl Iterator<Item = (Target, &mut Selection)> {
    document
        .selection
        .iter_mut()
        .map(|s| (Target::Current, s))
        .chain(document.layers.iter_mut().flat_map(|l| {
            let id = l.id;
            l.selection
                .iter_mut()
                .map(move |s| (Target::Layer(id), s))
                .chain(l.mask.iter_mut().flat_map(|m| {
                    let id = m.id;
                    m.initial.iter_mut().map(move |s| (Target::Mask(id), s))
                }))
        }))
}

impl SelectionIndex {
    /// Fixed-size little-endian word blocks, with zero padding in the last block.
    pub const CHUNK_BYTES: usize = CHUNK_BYTES;

    /// Private worker input uses raw blocks so compression stays off the editor
    /// thread. Archive input uses the same index and validation with LZ4 blocks.
    pub fn attach(
        &self, document: &mut Document, limits: ProjectLimits,
        read: impl FnMut(usize) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        self.check_version(document, false)?;
        let mut chunks = BTreeSet::new();
        self.validate_index(document, limits, |id| {
            if !chunks.insert(id) || chunks.len() > limits.tiles {
                return Err("Duplicate or excessive selection transfer blocks".into());
            }
            Ok(())
        })?;
        self.restore_with(document, read)
    }

    pub(super) fn check_version(
        &self,
        document: &mut Document,
        legacy: bool,
    ) -> Result<(), String> {
        if legacy {
            if !self.bindings.is_empty() || !self.pixels.is_empty() {
                return Err("Selection index requires project version 7".into());
            }
        } else if selections(document).any(|(_, s)| matches!(s.shape, SelectionShape::Pixels(_))) {
            return Err("Selection pixels must use binary storage".into());
        }
        Ok(())
    }
    pub(super) fn collect(
        document: &mut Document,
        blobs: &mut Vec<Arc<TileBlob>>,
        blob_ids: &mut BTreeMap<[u8; 32], usize>,
        tile_count: &mut usize,
    ) -> Result<Self, String> {
        let mut bytes = vec![0; CHUNK_BYTES];
        Self::detach(document, |pixels, range| {
            bytes.fill(0);
            for (word, destination) in pixels.words()[range].iter().zip(bytes.chunks_exact_mut(4)) {
                destination.copy_from_slice(&word.to_le_bytes());
            }
            let blob = Arc::new(TileBlob::encode(color::PixelDescriptor::COVERAGE8, &bytes)?);
            let id = *blob_ids.entry(blob.digest).or_insert_with(|| {
                let id = blobs.len(); blobs.push(blob); id
            });
            *tile_count += 1;
            Ok(id)
        })
    }

    /// Detach shared masks once; the host decides how to transport their blocks.
    pub fn detach(
        document: &mut Document,
        mut chunk: impl FnMut(&Arc<SelectionPixels>, std::ops::Range<usize>) -> Result<usize, String>,
    ) -> Result<Self, String> {
        let mut result = Self::default();
        let mut masks = BTreeMap::new();
        for (target, selection) in selections(document) {
            let SelectionShape::Pixels(pixels) = &selection.shape else {
                continue;
            };
            let identity = Arc::as_ptr(pixels) as usize;
            let index = if let Some(index) = masks.get(&identity) {
                *index
            } else {
                let mut chunks = Vec::new();
                for start in (0..pixels.words().len()).step_by(CHUNK_BYTES / 4) {
                    chunks.push(chunk(pixels, start..(start + CHUNK_BYTES / 4).min(pixels.words().len()))?);
                }
                let index = result.pixels.len();
                result.pixels.push(PixelsRecord {
                    extent: pixels.extent(),
                    bounds: pixels.bounds(),
                    byte_coverage: pixels.coverage_format() == 2,
                    chunks,
                });
                masks.insert(identity, index);
                index
            };
            result.bindings.push(Binding {
                target,
                pixels: index,
            });
            // Retain affine/inversion and the presence of a selection. No pixel
            // payload is serialized through Document's legacy JSON representation.
            selection.shape = SelectionShape::Contours(Arc::from([]));
        }
        Ok(result)
    }

    pub(super) fn validate(
        &self,
        document: &Document,
        blobs: &[BlobRecord],
        limits: ProjectLimits,
        referenced: &mut BTreeSet<usize>,
        tile_count: &mut usize,
    ) -> Result<(), String> {
        self.validate_index(document, limits, |id| {
            if blobs.get(id).is_none_or(|b| b.descriptor != color::PixelDescriptor::COVERAGE8) {
                return Err("Invalid selection chunk descriptor".into());
            }
            referenced.insert(id);
            *tile_count += 1;
            Ok(())
        })
    }

    fn validate_index(
        &self, document: &Document, limits: ProjectLimits,
        mut chunk: impl FnMut(usize) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.bindings.len() > document.layers.len() * 2 + 1
            || self.pixels.len() > self.bindings.len()
        {
            return Err("Oversized selection index".into());
        }
        let mut targets = BTreeSet::new();
        let mut used = BTreeSet::new();
        for binding in &self.bindings {
            let selection = match binding.target {
                Target::Current => document.selection.as_ref(),
                Target::Layer(id) => document.layer(id).and_then(|l| l.selection.as_ref()),
                Target::Mask(id) => document
                    .layers
                    .iter()
                    .filter_map(|l| l.mask.as_ref())
                    .find(|m| m.id == id)
                    .and_then(|m| m.initial.as_ref()),
            }
            .ok_or("Missing selection target")?;
            if !matches!(&selection.shape, SelectionShape::Contours(c) if c.is_empty())
                || !targets.insert(binding.target)
                || binding.pixels >= self.pixels.len()
            {
                return Err("Invalid selection reference".into());
            }
            used.insert(binding.pixels);
        }
        if used.len() != self.pixels.len() {
            return Err("Unused selection pixels".into());
        }
        let mut bytes = 0u64;
        for pixels in &self.pixels {
            let [w, h] = pixels.extent;
            let [x0, y0, x1, y1] = pixels.bounds;
            if w == 0
                || h == 0
                || w > limits.dimension
                || h > limits.dimension
                || x0 > x1
                || y0 > y1
                || x1 > w
                || y1 > h
                || (pixels.words() * 4).div_ceil(CHUNK_BYTES as u64) != pixels.chunks.len() as u64
            {
                return Err("Invalid selection chunk index".into());
            }
            bytes = bytes.saturating_add(pixels.words() * 4);
            if bytes > limits.raster_bytes {
                return Err("Selection coverage exceeds the project memory limit".into());
            }
            for id in &pixels.chunks {
                chunk(*id)?;
            }
        }
        Ok(())
    }

    pub(super) fn restore(
        &self,
        tiles: &[RasterTile],
        document: &mut Document,
    ) -> Result<(), String> {
        self.restore_with(document, |id| tiles[id].wait_backing()?.decode())
    }

    fn restore_with(
        &self, document: &mut Document,
        mut read: impl FnMut(usize) -> Result<Vec<u8>, String>,
    ) -> Result<(), String> {
        let mut masks = Vec::with_capacity(self.pixels.len());
        for record in &self.pixels {
            let count = record.words() as usize;
            let mut words = Vec::new();
            words
                .try_reserve_exact(count)
                .map_err(|_| "Selection allocation failed")?;
            for id in &record.chunks {
                let bytes = read(*id)?;
                if bytes.len() != CHUNK_BYTES { return Err("Invalid selection block length".into()); }
                let remaining = count - words.len();
                let used = bytes.len().min(remaining * 4);
                if bytes[used..].iter().any(|v| *v != 0) {
                    return Err("Nonzero selection chunk padding".into());
                }
                words.extend(
                    bytes[..used]
                        .chunks_exact(4)
                        .map(|b| u32::from_le_bytes(b.try_into().unwrap())),
                );
            }
            let pixels = if record.byte_coverage {
                SelectionPixels::bytes(record.extent, record.bounds, words)
            } else {
                SelectionPixels::new(record.extent, record.bounds, words)
            }
            .map_err(|e| e.to_string())?;
            masks.push(Arc::new(pixels));
        }
        let bindings: BTreeMap<_, _> = self.bindings.iter().map(|b| (b.target, b.pixels)).collect();
        for (target, selection) in selections(document) {
            if let Some(index) = bindings.get(&target) {
                selection.shape = SelectionShape::Pixels(masks[*index].clone());
            }
        }
        Ok(())
    }
}
