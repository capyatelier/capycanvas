//! One conservative tile plan for brush allocation, copies and evaluation.
use super::*;

#[derive(Clone)]
pub(super) struct BrushTile {
    pub coordinate: [u32; 2],
    pub local: PixelRect,
    pub dabs: std::ops::Range<u32>,
}

fn contact_bounds(dab: Dab, contact: &layer_core::BrushContact) -> layer_core::Rect {
    if dab.previous[0] <= 0.0 { return dab.bounds(); }
    let axes = [dab.radii[0], dab.radii[1], dab.previous[0], dab.previous[1]];
    let maximum = axes.into_iter().fold(0.005_f32, f32::max);
    let minimum = axes.into_iter().fold(f32::INFINITY, f32::min).max(0.005);
    // evolving_contact's outer edge is boundary + half an AA pixel; roughness
    // and pooling are the only terms that expand it. Interpolated/rotated
    // ellipses stay inside the largest endpoint radius. The shader also rejects
    // radius > 1.45, independently of the material parameters.
    let expansion = (1.0 + 0.75 * contact.edge_roughness + 0.12 * contact.pooling
        + 0.5 * (1.0 / minimum).min(1.0)).min(1.45);
    let radius = maximum * expansion + 1.0;
    let start = [dab.center.x - dab.motion[0], dab.center.y - dab.motion[1]];
    layer_core::Rect {
        min: layer_core::Point {
            x: dab.center.x.min(start[0]) - radius,
            y: dab.center.y.min(start[1]) - radius,
        },
        max: layer_core::Point {
            x: dab.center.x.max(start[0]) + radius,
            y: dab.center.y.max(start[1]) + radius,
        },
    }
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
    let contact = batch.style.contact.as_ref().unwrap();
    let mut tiles = std::collections::BTreeMap::<_, BrushTile>::new();
    for (index, dab) in dabs.iter().enumerate() {
        let bounds = pixel_rect(batch.style.brush_to_layer.bounds(contact_bounds(*dab, contact)), extent)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_bounds_enclose_interpolated_nibs_and_material_edges() {
        let mut dab = Dab {
            center: layer_core::Point { x: 2100., y: 1900. },
            radii: [1000., 17.], rotation: [1., 0.], motion: [390., -270.],
            color_rgba_linear: [1.; 4], flow: 1., hardness: 0.96,
            texture_sign: [1.; 2], material: [0.; 4],
            previous: [12., 850., 0., 1.], contact: [1.; 4], previous_contact: [0.; 4],
        };
        for axes in [[1000., 1000., 1000., 1000.], [1000., 17., 12., 850.], [0.002; 4]] {
            dab.radii = [axes[0], axes[1]];
            dab.previous[..2].copy_from_slice(&axes[2..]);
            for (roughness, pooling) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
                let contact = layer_core::BrushContact { edge_roughness: roughness, pooling, ..Default::default() };
                let bounds = contact_bounds(dab, &contact);
                for step in 0..=32 {
                    let t = step as f32 / 32.;
                    let axes = std::array::from_fn::<_, 2, _>(|i| (dab.previous[i] * (1. - t) + dab.radii[i] * t).max(0.005));
                    let angle = [t, 1. - t];
                    let length = angle[0].hypot(angle[1]);
                    let rotation = angle.map(|v| v / length);
                    let center = [dab.center.x - dab.motion[0] * (1. - t), dab.center.y - dab.motion[1] * (1. - t)];
                    for pressure in [0., 0.5, 1.] {
                        // The shader's outer smoothstep edge at maximal noise.
                        let boundary = 1. + roughness * 0.5 * (1.5 - pressure * 0.5)
                            + pooling * 0.12 * (0.3 + 0.7 * pressure);
                        let edge = (boundary + 0.5 * (1. / axes[0].min(axes[1])).min(1.)).min(1.45);
                        for direction in 0..64 {
                            let (sin, cos) = (direction as f32 * std::f32::consts::TAU / 64.).sin_cos();
                            let local = [axes[0] * edge * cos, axes[1] * edge * sin];
                            let point = [center[0] + local[0] * rotation[0] - local[1] * rotation[1],
                                center[1] + local[0] * rotation[1] + local[1] * rotation[0]];
                            assert!(point[0] >= bounds.min.x && point[0] <= bounds.max.x
                                && point[1] >= bounds.min.y && point[1] <= bounds.max.y);
                        }
                    }
                }
            }
        }
    }
}
