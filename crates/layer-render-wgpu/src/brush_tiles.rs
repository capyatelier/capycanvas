//! One conservative tile plan for brush allocation, copies and evaluation.
use super::*;

#[derive(Clone)]
pub(super) struct BrushTile {
    pub coordinate: [u32; 2],
    pub local: PixelRect,
    pub dabs: std::ops::Range<u32>,
    // Dispersed stamps can leave large holes in the first-to-last range.
    // Keep their exact order for packing into the existing GPU dab buffer.
    pub indices: Vec<u32>,
}

fn contact_radius(dab: Dab, contact: &layer_core::BrushContact) -> f32 {
    let axes = [dab.radii[0], dab.radii[1], dab.previous[0], dab.previous[1]];
    let maximum = axes.into_iter().fold(0.005_f32, f32::max);
    let minimum = axes.into_iter().fold(f32::INFINITY, f32::min).max(0.005);
    let low_pressure = dab.contact[0].min(dab.previous_contact[0]).clamp(0., 1.);
    let high_pressure = dab.contact[0].max(dab.previous_contact[0]).clamp(0., 1.);
    // evolving_contact's outer edge is boundary + half an AA pixel; roughness
    // and pooling are the only terms that expand it. Interpolated/rotated
    // ellipses stay inside the largest endpoint radius. The shader also rejects
    // radius > 1.45, independently of the material parameters.
    let expansion = (1.0
        + 0.5 * (1.5 - 0.5 * low_pressure) * contact.edge_roughness
        + 0.12 * (0.3 + 0.7 * high_pressure) * contact.pooling
        + 0.5 * (1.0 / minimum).min(1.0))
    .min(1.45);
    maximum * expansion + 1.0
}

fn contact_bounds(dab: Dab, radius: f32) -> layer_core::Rect {
    if dab.previous[0] <= 0.0 {
        return dab.bounds();
    }
    // Each interpolated ellipse is enclosed by the maximum endpoint axes.
    // Bound their support over the entire rotation arc, including interior
    // extrema. Endpoint rectangles alone miss a nib rotating through an axis.
    let axes = [
        dab.radii[0].max(dab.previous[0]).max(0.005),
        dab.radii[1].max(dab.previous[1]).max(0.005),
    ];
    let start_angle = dab.previous[3].atan2(dab.previous[2]);
    let end_angle = dab.rotation[1].atan2(dab.rotation[0]);
    let delta = (end_angle - start_angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let (lo, hi) = (
        start_angle.min(start_angle + delta),
        start_angle.max(start_angle + delta),
    );
    let expansion = (radius - 1.) / axes[0].max(axes[1]);
    let half = [0., std::f32::consts::FRAC_PI_2].map(|offset| {
        let support = |angle: f32| {
            let (sin, cos) = (angle - offset).sin_cos();
            (axes[0] * cos).hypot(axes[1] * sin)
        };
        let peak = offset
            + if axes[0] >= axes[1] {
                0.
            } else {
                std::f32::consts::FRAC_PI_2
            };
        let next_peak = peak + ((lo - peak) / std::f32::consts::PI).ceil() * std::f32::consts::PI;
        let maximum = if next_peak <= hi + 0.00001 {
            axes[0].max(axes[1])
        } else {
            support(lo).max(support(hi))
        };
        maximum * expansion + 1.
    });
    let start = [dab.center.x - dab.motion[0], dab.center.y - dab.motion[1]];
    layer_core::Rect {
        min: layer_core::Point {
            x: dab.center.x.min(start[0]) - half[0],
            y: dab.center.y.min(start[1]) - half[1],
        },
        max: layer_core::Point {
            x: dab.center.x.max(start[0]) + half[0],
            y: dab.center.y.max(start[1]) + half[1],
        },
    }
}

/// An ellipse metric turns a steady swept nib into a unit-radius capsule.
/// For a changing pose, enlarge it by the maximum singular value of the
/// intermediate rotation in that metric. Endpoint axis maxima enclose every
/// interpolated size; this also preserves pressure and orientation extrema.
struct ContactHull {
    transform: layer_core::Affine,
    dab: Dab,
    radius: f32,
}
impl ContactHull {
    fn new(dab: Dab, radius: f32) -> Self {
        let axes = [
            dab.radii[0].max(dab.previous[0]).max(0.005),
            dab.radii[1].max(dab.previous[1]).max(0.005),
        ];
        let angle = dab.previous[3].atan2(dab.previous[2]);
        let turn = (dab.rotation[1].atan2(dab.rotation[0]) - angle + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let (sin, cos) = (angle + turn * 0.5).sin_cos();
        let transform = layer_core::Affine([
            cos / axes[0],
            -sin / axes[1],
            sin / axes[0],
            cos / axes[1],
            -(dab.center.x * cos + dab.center.y * sin) / axes[0],
            (dab.center.x * sin - dab.center.y * cos) / axes[1],
        ]);
        let k = 0.5 * (axes[0] / axes[1] - axes[1] / axes[0]).abs() * (turn * 0.5).sin().abs();
        let stretch = (1. + k * k).sqrt() + k;
        let expansion = (radius - 1.) / axes[0].max(axes[1]);
        Self {
            transform,
            dab: Dab {
                center: layer_core::Point::default(),
                motion: [
                    (dab.motion[0] * cos + dab.motion[1] * sin) / axes[0],
                    (-dab.motion[0] * sin + dab.motion[1] * cos) / axes[1],
                ],
                ..dab
            },
            radius: expansion * stretch + 1. / axes[0].min(axes[1]),
        }
    }
    fn touches(&self, tile: layer_core::Rect, layer_to_brush: layer_core::Affine) -> bool {
        let transform = layer_to_brush.then(self.transform);
        let corners = [
            tile.min,
            layer_core::Point {
                x: tile.max.x,
                y: tile.min.y,
            },
            tile.max,
            layer_core::Point {
                x: tile.min.x,
                y: tile.max.y,
            },
        ]
        .map(|p| {
            let p = transform.map(p);
            [f64::from(p.x), f64::from(p.y)]
        });
        capsule_touches_quad(
            self.dab.motion.map(f64::from),
            f64::from(self.radius),
            corners,
        )
    }
}

// Exact distance from a center segment to the inverse-mapped tile. Keeping
// the quadrilateral avoids adding another axis-aligned bounding box around a
// rotated thin nib; those empty corners otherwise cost paint and composition.
fn capsule_touches_quad(motion: [f64; 2], radius: f64, corners: [[f64; 2]; 4]) -> bool {
    let sub = |a: [f64; 2], b: [f64; 2]| [a[0] - b[0], a[1] - b[1]];
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    let start = motion.map(|v| -v);
    let winding = cross(sub(corners[1], corners[0]), sub(corners[2], corners[1])).signum();
    let (mut enter, mut exit) = (0.0_f64, 1.0_f64);
    for i in 0..4 {
        let edge = sub(corners[(i + 1) % 4], corners[i]);
        let offset = cross(edge, sub(start, corners[i])) * winding;
        let slope = cross(edge, motion) * winding;
        if slope == 0. {
            if offset < 0. {
                exit = -1.;
            }
        } else if slope > 0. {
            enter = enter.max(-offset / slope);
        } else {
            exit = exit.min(-offset / slope);
        }
    }
    if enter <= exit {
        return true;
    }
    let point_segment = |point: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        let d = sub(b, a);
        let p = sub(point, a);
        let length2 = d[0] * d[0] + d[1] * d[1];
        let t = if length2 > 0. {
            ((p[0] * d[0] + p[1] * d[1]) / length2).clamp(0., 1.)
        } else {
            0.
        };
        (p[0] - t * d[0]).powi(2) + (p[1] - t * d[1]).powi(2)
    };
    let mut distance = f64::INFINITY;
    for i in 0..4 {
        let a = corners[i];
        let b = corners[(i + 1) % 4];
        distance = distance
            .min(point_segment(start, a, b))
            .min(point_segment([0.; 2], a, b))
            .min(point_segment(a, start, [0.; 2]));
    }
    distance <= radius * radius
}

pub(super) fn plan(batch: &DabBatch, dabs: &[Dab], extent: [u32; 2]) -> Vec<BrushTile> {
    let damage = batch_pixel_rect(batch, extent);
    if dabs.is_empty() || damage.is_empty() {
        return Vec::new();
    }
    if batch.style.execution != BrushExecution::Dry || batch.style.rendering.edge_after_stroke {
        // Nonlocal materials retain their complete dependency sequence/region.
        return page_coordinates(damage)
            .map(|coordinate| BrushTile {
                coordinate,
                local: damage
                    .intersect(page_rect(coordinate))
                    .page_local(coordinate),
                dabs: batch.first_dab..batch.first_dab + batch.dab_count,
                indices: Vec::new(),
            })
            .collect();
    }
    let contact = batch.style.contact.as_ref();
    let inverse = batch.style.brush_to_layer.inverse();
    let mut tiles = std::collections::BTreeMap::<_, BrushTile>::new();
    for (index, dab) in dabs.iter().enumerate() {
        let radius = contact.map_or(0., |contact| contact_radius(*dab, contact));
        let hull =
            (contact.is_some() && dab.previous[0] > 0.).then(|| ContactHull::new(*dab, radius));
        let footprint = if contact.is_some() {
            contact_bounds(*dab, radius)
        } else {
            dab.bounds()
        };
        let bounds =
            pixel_rect(batch.style.brush_to_layer.bounds(footprint), extent).intersect(damage);
        if bounds.is_empty() {
            continue;
        }
        let index = batch.first_dab + index as u32;
        for coordinate in page_coordinates(bounds) {
            if let Some(hull) = &hull
                && let Some(inverse) = inverse
            {
                let tile = page_rect(coordinate);
                let rect = layer_core::Rect {
                    min: layer_core::Point {
                        x: tile.min_x() as f32,
                        y: tile.min_y() as f32,
                    },
                    max: layer_core::Point {
                        x: tile.max_x() as f32,
                        y: tile.max_y() as f32,
                    },
                };
                if !hull.touches(rect, inverse) {
                    continue;
                }
            }
            let local = bounds
                .intersect(page_rect(coordinate))
                .page_local(coordinate);
            // Match page_coordinates' row-major order without a second sort.
            let tile = tiles
                .entry([coordinate[1], coordinate[0]])
                .or_insert(BrushTile {
                    coordinate,
                    local,
                    dabs: index..index + 1,
                    indices: Vec::new(),
                });
            tile.local = tile.local.union(local);
            tile.dabs.end = index + 1;
            if contact.is_none() {
                tile.indices.push(index);
            }
        }
    }
    tiles.into_values().collect()
}

/// A source tile's influence on the document composite. Placed display images
/// reduce by at most 256; include one such texel for the bilinear footprint.
/// Identity placement reads aligned pixels and needs no sampling halo.
pub(super) fn document_damage(
    layers: &[Layer],
    id: LayerId,
    local: PixelRect,
    extent: [u32; 2],
) -> PixelRect {
    let transform = layer_core::target_transform(layers, id);
    if transform == layer_core::Affine::IDENTITY {
        return local.intersect(PixelRect::full(extent));
    }
    if local.is_empty() {
        return PixelRect::EMPTY;
    }
    let halo = PAGE_SIZE as f32;
    pixel_rect(
        transform.bounds(layer_core::Rect {
            min: layer_core::Point {
                x: local.min_x() as f32 - halo,
                y: local.min_y() as f32 - halo,
            },
            max: layer_core::Point {
                x: local.max_x() as f32 + halo,
                y: local.max_y() as f32 + halo,
            },
        }),
        extent,
    )
}
