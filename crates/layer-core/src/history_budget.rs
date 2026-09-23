//! Shared accounting for admission and eviction. The current document owns its
//! backing independently; history is charged for additional retained ownership.
use super::*;
use std::collections::HashSet;

pub(super) const BYTE_BUDGET: usize = 512 * 1024 * 1024;
pub(super) const ENTRY_BUDGET: usize = 256;

#[derive(Default)]
pub(super) struct Accounting {
    roots: HashSet<u64>,
    tiles: HashSet<u64>,
    sources: color::source::SourceAccounting,
    selections: HashSet<usize>,
}
impl Accounting {
    pub fn new(document: &Document) -> Self {
        let mut result = Self::default();
        for selection in document.selection.iter().chain(document.layers.iter().filter_map(|l| l.selection.as_ref())) {
            result.charge_selection(selection);
        }
        for layer in &document.layers {
            if let Some(source) = &layer.source {
                result.sources.charge(source);
            }
            for revision in std::iter::once(&layer.raster).chain(layer.masks().map(|m| &m.raster)) {
                result.roots.insert(revision.identity());
                if let Some(Ok(data)) = revision.try_data() {
                    result
                        .tiles
                        .extend(data.tiles.values().map(|t| t.identity()));
                }
            }
        }
        result
    }

    pub fn charge(&mut self, entry: &HistoryEntry) -> usize {
        let mut bytes = entry.metadata_bytes;
        let mut selections = Vec::new();
        entry.edit.selection_roots(&mut selections);
        for selection in selections { bytes = bytes.saturating_add(self.charge_selection(selection)); }
        let mut sources = Vec::new();
        entry.edit.source_roots(&mut sources);
        for source in sources {
            bytes = bytes.saturating_add(self.sources.charge(source));
        }
        let mut roots = Vec::new();
        entry.edit.raster_roots(&mut roots);
        for revision in roots {
            if !self.roots.insert(revision.identity()) {
                continue;
            }
            match revision.try_data() {
                Some(Ok(data)) => {
                    bytes = bytes.saturating_add(data.tiles.len().saturating_mul(96));
                    for tile in data.tiles.values() {
                        if self.tiles.insert(tile.identity()) {
                            bytes = bytes.saturating_add(match tile.try_backing() {
                                Some(Ok(blob)) => blob.resident_bytes(),
                                _ => {
                                    raster::TileBlob::max_compressed_len(tile.descriptor())
                                        .unwrap_or(raster::MAX_COMPRESSED_TILE_BYTES)
                                }
                            });
                        }
                    }
                }
                // Pending producers reserve the permitted capture, never zero.
                None => bytes = bytes.saturating_add(revision.pending_bytes()),
                Some(Err(_)) => (),
            }
        }
        bytes
    }

    fn charge_selection(&mut self, selection: &Selection) -> usize {
        match &selection.shape {
            SelectionShape::Pixels(pixels) => {
                if self.selections.insert(pixels.words().as_ptr() as usize) {
                    pixels.words().len().saturating_mul(4)
                } else { 0 }
            }
            SelectionShape::Contours(paths) => paths.iter().map(|path| {
                if self.selections.insert(path.as_ptr() as usize) {
                    path.len().saturating_mul(std::mem::size_of::<Point>())
                } else { 0 }
            }).fold(0usize, usize::saturating_add),
        }
    }
}

#[cfg(test)]
mod tests;
