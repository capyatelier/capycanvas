//! One conservative tile plan for brush allocation, copies and evaluation.
use super::*;

#[derive(Clone)]
pub(super) struct BrushTile {
    pub coordinate: [u32; 2],
    pub local: PixelRect,
    pub dabs: std::ops::Range<u32>,
    // Empty for a contiguous range; otherwise only intersecting contacts.
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

/// Everything a fan span paints lies in the convex hull of its contact
/// rectangles at the start and end poses, widened by the bulge of a roll and
/// antialiasing. Each hull edge is a constraint `normal · p <= limit`.
struct FanRegion {
    bounds: layer_core::Rect,
    edges: Vec<([f64; 2], f64)>,
}

impl FanRegion {
    fn new(dab: Dab) -> Self {
        let turn = fan_turn(dab).abs();
        let end = [f64::from(dab.center.x), f64::from(dab.center.y)];
        let start = [end[0] - f64::from(dab.motion[0]), end[1] - f64::from(dab.motion[1])];
        let (width, depth) = (dab.previous[1].max(dab.radii[1]).max(0.5), dab.previous[0].max(dab.radii[0]).max(0.5));
        let margin = f64::from(turn * width.max(depth) * 0.5 + 2.);
        let mut corners = Vec::with_capacity(8);
        for (centre, normal, width, depth) in [
            (start, [dab.previous[2], dab.previous[3]], dab.previous[1], dab.previous[0]),
            (end, dab.rotation, dab.radii[1], dab.radii[0]),
        ] {
            let normal = [f64::from(normal[0]), f64::from(normal[1])];
            let fan = [-normal[1], normal[0]];
            let (w, d) = (f64::from(width.max(0.5)) + margin, f64::from(depth.max(0.5)) + margin);
            for (a, b) in [(1., 1.), (1., -1.), (-1., -1.), (-1., 1.)] {
                corners.push([
                    centre[0] + fan[0] * w * a + normal[0] * d * b,
                    centre[1] + fan[1] * w * a + normal[1] * d * b,
                ]);
            }
        }
        let hull = convex_hull(corners);
        let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &hull {
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        let edges = (0..hull.len())
            .map(|i| {
                let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
                let normal = [b[1] - a[1], a[0] - b[0]];
                (normal, normal[0] * a[0] + normal[1] * a[1])
            })
            .collect();
        Self {
            bounds: layer_core::Rect {
                min: layer_core::Point { x: min[0] as f32, y: min[1] as f32 },
                max: layer_core::Point { x: max[0] as f32, y: max[1] as f32 },
            },
            edges,
        }
    }

    /// The layer-space bounds of this region within one tile, if any.
    fn clip(&self, tile: layer_core::Rect, layer_to_brush: layer_core::Affine, brush_to_layer: layer_core::Affine) -> Option<layer_core::Rect> {
        let corners = [
            tile.min,
            layer_core::Point { x: tile.max.x, y: tile.min.y },
            tile.max,
            layer_core::Point { x: tile.min.x, y: tile.max.y },
        ];
        let mut polygon: Vec<[f64; 2]> = corners
            .iter()
            .map(|&p| {
                let p = layer_to_brush.map(p);
                [f64::from(p.x), f64::from(p.y)]
            })
            .collect();
        for plane in &self.edges {
            polygon = clip_polygon(&polygon, *plane);
        }
        polygon.iter().fold(None, |result: Option<layer_core::Rect>, p| {
            let q = brush_to_layer.map(layer_core::Point { x: p[0] as f32, y: p[1] as f32 });
            let rect = layer_core::Rect { min: q, max: q };
            Some(result.map_or(rect, |r| r.union(rect)))
        })
    }
}

/// A bristle fan span's roll between its two poses.
fn fan_turn(dab: Dab) -> f32 {
    (dab.previous[2] * dab.rotation[1] - dab.previous[3] * dab.rotation[0])
        .atan2(dab.previous[2] * dab.rotation[0] + dab.previous[3] * dab.rotation[1])
}

/// Counter-clockwise hull by Andrew's monotone chain.
fn convex_hull(mut points: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut hull: Vec<[f64; 2]> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let floor = hull.len();
        for index in 0..points.len() {
            let p = if pass == 0 { points[index] } else { points[points.len() - 1 - index] };
            while hull.len() >= floor + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0. {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

fn clip_polygon(polygon: &[[f64; 2]], (normal, limit): ([f64; 2], f64)) -> Vec<[f64; 2]> {
    let side = |p: [f64; 2]| normal[0] * p[0] + normal[1] * p[1] - limit;
    let mut output = Vec::with_capacity(polygon.len() + 1);
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let (sa, sb) = (side(a), side(b));
        if sa <= 0. {
            output.push(a);
        }
        if (sa <= 0.) != (sb <= 0.) {
            let t = sa / (sa - sb);
            output.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    output
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
    if !pointwise(&batch.style) || batch.style.rendering.edge_after_stroke {
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
    let fan = contact.is_some_and(|c| c.bristles.is_some());
    let inverse = batch.style.brush_to_layer.inverse();
    let mut tiles = std::collections::BTreeMap::<_, BrushTile>::new();
    for (index, dab) in dabs.iter().enumerate() {
        let radius = contact.filter(|_| !fan).map_or(0., |contact| contact_radius(*dab, contact));
        let hull = (contact.is_some() && !fan && dab.previous[0] > 0.)
            .then(|| ContactHull::new(*dab, radius));
        let region = fan.then(|| FanRegion::new(*dab));
        let footprint = if let Some(region) = &region {
            region.bounds
        } else if contact.is_some() {
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
            let mut clipped = bounds;
            if let Some(region) = &region
                && let Some(inverse) = inverse
            {
                let tile = page_rect(coordinate);
                let rect = layer_core::Rect {
                    min: layer_core::Point { x: tile.min_x() as f32, y: tile.min_y() as f32 },
                    max: layer_core::Point { x: tile.max_x() as f32, y: tile.max_y() as f32 },
                };
                let Some(area) = region.clip(rect, inverse, batch.style.brush_to_layer) else {
                    continue;
                };
                let area = layer_core::Rect {
                    min: layer_core::Point { x: area.min.x - 1., y: area.min.y - 1. },
                    max: layer_core::Point { x: area.max.x + 1., y: area.max.y + 1. },
                };
                clipped = pixel_rect(area, extent).intersect(bounds);
                if clipped.is_empty() {
                    continue;
                }
            }
            if let Some(hull) = &hull
                && let Some(inverse) = inverse
            {
                let tile = page_rect(coordinate);
                let rect = tile.to_rect();
                if !hull.touches(rect, inverse) {
                    continue;
                }
            }
            let local = clipped
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
            if tile.indices.is_empty() && tile.dabs.end < index {
                tile.indices.extend(tile.dabs.clone());
            }
            if !tile.indices.is_empty() { tile.indices.push(index); }
            tile.dabs.end = index + 1;
        }
    }
    tiles.into_values().collect()
}

/// A source tile's influence on the document composite. Placed display images
/// reduce by at most 256; include one such texel for the bilinear footprint.
/// Identity placement reads aligned pixels and needs no sampling halo.
pub(super) fn document_damage(
    scene: SceneView<'_>,
    id: SourceTarget,
    local: PixelRect,
    extent: [u32; 2],
) -> PixelRect {
    let transform = scene.target_geometry(id);
    if transform.is_identity() {
        return local.intersect(PixelRect::full(extent));
    }
    if local.is_empty() {
        return PixelRect::EMPTY;
    }
    let halo = PAGE_SIZE as f32;
    pixel_rect(
        transform.forward_bounds(layer_core::Rect {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revisited_tiles_keep_only_the_contacts_that_touch_them() {
        let dabs = [64., 832., 64.].map(|x| Dab {
            center: layer_core::Point { x, y: 64. }, radii: [20.; 2], rotation: [1., 0.],
            color_rgba_linear: [1.; 4], flow: 1., hardness: 0.5, texture_sign: [1.; 2],
            material: [0., 0., 1., 0.], motion: [0.; 2], previous: [20., 20., 1., 0.],
            contact: [1., 0., 2., 7.], previous_contact: [1., 0., 1., 7.],
        });
        for contact in [None, Some(layer_core::BrushContact::default()), Some(layer_core::BrushContact {
            bristles: Some(Default::default()), ..Default::default()
        })] {
            let mut style = crate::tests::test_style(BrushExecution::Dry);
            style.contact = contact;
            let mut batch = crate::test_support::dab_batch(SourceTarget::Paint(layer_core::PaintHandle::from_index(1)), style, PixelRect::full([1024, 256]).to_rect());
            batch.first_dab = 7;
            batch.dab_count = dabs.len() as u32;
            let tiles = plan(&batch, &dabs, [1024, 256]);
            assert_eq!(tiles.len(), 2);
            assert_eq!(tiles[0].coordinate, [0, 0]);
            assert_eq!(tiles[0].indices, [7, 9]);
            assert!(tiles[1].indices.is_empty());
            assert_eq!(tiles[1].dabs, 8..9);
        }
    }

    #[test]
    fn fan_tiles_keep_every_pixel_the_moving_contact_touches_and_skip_the_rest() {
        let touched = |dab: Dab, p: [f32; 2]| {
            let end = [dab.center.x, dab.center.y];
            let start = [end[0] - dab.motion[0], end[1] - dab.motion[1]];
            (0..=64).any(|step| {
                let t = step as f32 / 64.;
                let mix = |a: f32, b: f32| a + (b - a) * t;
                let normal = [mix(dab.previous[2], dab.rotation[0]), mix(dab.previous[3], dab.rotation[1])];
                let length = normal[0].hypot(normal[1]).max(0.0001);
                let normal = [normal[0] / length, normal[1] / length];
                let q = [p[0] - mix(start[0], end[0]), p[1] - mix(start[1], end[1])];
                let across = (q[1] * normal[0] - q[0] * normal[1]).abs();
                let along = (q[0] * normal[0] + q[1] * normal[1]).abs();
                across <= mix(dab.previous[1], dab.radii[1]).max(0.5) + 1.
                    && along <= mix(dab.previous[0], dab.radii[0]).max(0.5) + 1.
            })
        };
        let mut style = crate::tests::test_style(BrushExecution::Dry);
        style.contact = Some(layer_core::BrushContact {
            bristles: Some(Default::default()),
            ..Default::default()
        });
        let mut seed = 0x2545_f491_u32;
        let mut random = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32
        };
        let (mut kept, mut skipped) = (0, 0);
        for case in 0..60 {
            let angle = random() * std::f32::consts::TAU;
            let turn = if case % 3 == 0 { (random() - 0.5) * 0.6 } else { 0. };
            let travel = if case % 5 == 0 { 0. } else { random() * 400. };
            let heading = random() * std::f32::consts::TAU;
            let dab = Dab {
                center: layer_core::Point { x: 700. + random() * 200., y: 700. + random() * 200. },
                radii: [20. + random() * 120., 50. + random() * 450.],
                rotation: [(angle + turn).cos(), (angle + turn).sin()],
                motion: [heading.cos() * travel, heading.sin() * travel],
                color_rgba_linear: [1.; 4],
                flow: 1.,
                hardness: 1.,
                texture_sign: [1.; 2],
                material: [f32::from(u8::from(case % 3 == 0)), 0., 1., 0.],
                previous: [20. + random() * 120., 50. + random() * 450., angle.cos(), angle.sin()],
                contact: [1., 0., 2., 7.],
                previous_contact: [1., 0., if case % 4 == 0 { 0. } else { 1. }, 7.],
            };
            let batch = DabBatch {
                material_update: 0,
                stroke_id: StrokeId(1),
                target: SourceTarget::Paint(layer_core::PaintHandle::from_index(1)),
                kind: DabBatchKind::Persistent,
                stroke_start: false,
                stroke_end: false,
                first_dab: 0,
                dab_count: 1,
                style: style.clone(),
                damage: layer_core::Rect {
                    min: layer_core::Point { x: 0., y: 0. },
                    max: layer_core::Point { x: 1600., y: 1600. },
                },
            };
            let mut planned = vec![false; 1600 * 1600];
            for tile in plan(&batch, &[dab], [1600, 1600]) {
                let origin = [tile.coordinate[0] * PAGE_SIZE, tile.coordinate[1] * PAGE_SIZE];
                for y in origin[1] + tile.local.min_y()..origin[1] + tile.local.max_y() {
                    let row = y as usize * 1600;
                    planned[row + (origin[0] + tile.local.min_x()) as usize..row + (origin[0] + tile.local.max_x()) as usize]
                        .fill(true);
                }
            }
            let reach = dab.radii[0].max(dab.radii[1]).max(dab.previous[0]).max(dab.previous[1]) * 1.5 + 2.;
            let start = [dab.center.x - dab.motion[0], dab.center.y - dab.motion[1]];
            let near = |x: f32, y: f32| {
                x >= dab.center.x.min(start[0]) - reach && x <= dab.center.x.max(start[0]) + reach
                    && y >= dab.center.y.min(start[1]) - reach && y <= dab.center.y.max(start[1]) + reach
            };
            for y in (0..1600).step_by(3) {
                for x in (0..1600).step_by(3) {
                    let covered = planned[y as usize * 1600 + x as usize];
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    if near(p[0], p[1]) && touched(dab, p) {
                        assert!(covered, "case {case}: pixel {x},{y} is outside the planned tiles");
                        kept += 1;
                    } else if !covered {
                        skipped += 1;
                    }
                }
            }
        }
        assert!(kept > 10_000 && skipped > kept * 2, "kept {kept}, skipped {skipped}");
    }
}
