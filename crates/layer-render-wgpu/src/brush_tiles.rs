//! One conservative tile plan for brush allocation, copies and evaluation.
use super::*;

#[derive(Clone)]
pub(super) struct BrushTile {
    pub coordinate: [u32; 2],
    pub local: PixelRect,
    pub dabs: std::ops::Range<u32>,
}

pub(super) fn plan(batch: &DabBatch, dabs: &[Dab], extent: [u32; 2]) -> Vec<BrushTile> {
    let damage = batch_pixel_rect(batch, extent);
    if dabs.is_empty() || damage.is_empty() {
        return Vec::new();
    }
    if batch.style.contact.is_none() || batch.style.execution != BrushExecution::Dry {
        // Nonlocal materials retain their complete dependency sequence/region.
        return page_coordinates(damage).map(|coordinate| BrushTile {
            coordinate,
            local: damage.intersect(page_rect(coordinate)).page_local(coordinate),
            dabs: batch.first_dab..batch.first_dab + batch.dab_count,
        }).collect();
    }
    let mut tiles = std::collections::BTreeMap::<_, BrushTile>::new();
    for (index, dab) in dabs.iter().enumerate() {
        let bounds = pixel_rect(batch.style.brush_to_layer.bounds(dab.bounds()), extent)
            .intersect(damage);
        if bounds.is_empty() { continue; }
        let index = batch.first_dab + index as u32;
        for coordinate in page_coordinates(bounds) {
            let local = bounds.intersect(page_rect(coordinate)).page_local(coordinate);
            // Match page_coordinates' row-major order without a second sort.
            let tile = tiles.entry([coordinate[1], coordinate[0]]).or_insert(BrushTile {
                coordinate, local, dabs: index..index + 1,
            });
            tile.local = tile.local.union(local);
            tile.dabs.end = index + 1;
        }
    }
    tiles.into_values().collect()
}

/// A source tile's influence on the document composite. Placed display images
/// reduce by at most 256; include one such texel for the bilinear footprint.
/// Identity placement reads aligned pixels and needs no sampling halo.
pub(super) fn document_damage(layers: &[Layer], id: LayerId, local: PixelRect, extent: [u32; 2]) -> PixelRect {
    let transform = layer_core::target_transform(layers, id);
    if transform == layer_core::Affine::IDENTITY {
        return local.intersect(PixelRect::full(extent));
    }
    if local.is_empty() { return PixelRect::EMPTY; }
    let halo = PAGE_SIZE as f32;
    pixel_rect(transform.bounds(layer_core::Rect {
        min: layer_core::Point { x: local.min_x() as f32 - halo, y: local.min_y() as f32 - halo },
        max: layer_core::Point { x: local.max_x() as f32 + halo, y: local.max_y() as f32 + halo },
    }), extent)
}
