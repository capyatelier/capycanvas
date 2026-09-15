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
}
impl Accounting {
    pub fn new(document: &Document) -> Self {
        let mut result = Self::default();
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
                                    tile.descriptor()
                                        .byte_len([raster::TILE_SIZE; 2])
                                        .unwrap_or(raster::MAX_TILE_BYTES)
                                        + 1024
                                }
                            });
                        }
                    }
                }
                // Pending producers reserve the permitted capture, never zero.
                None => bytes = bytes.saturating_add(raster::MAX_CAPTURE_BYTES as usize),
                Some(Err(_)) => (),
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests;
