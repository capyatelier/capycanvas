//! Brush-asset geometry for a display-only cursor, never canvas rasterization.
//! Traced once from the original R8 asset, not from a GPU readback.

use std::collections::BTreeMap;

/// Closed contours in normalized tip coordinates [-1, 1].
pub type TipOutline = Vec<Vec<[f32; 2]>>;

/// Trace the 10% coverage silhouette, including holes. Clip to the same unit
/// disk as the brush shader. Collinear vertices are removed without changing
/// the contour. The source texture's texel resolution is retained.
pub fn mask_outline(width: u32, height: u32, stride: u32, pixels: &[u8]) -> TipOutline {
    if width == 0
        || height == 0
        || stride < width
        || width > i32::MAX as u32
        || height > i32::MAX as u32
        || (stride as usize)
            .checked_mul(height as usize)
            .is_none_or(|n| n > pixels.len())
    {
        return Vec::new();
    }
    let inside = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return false;
        }
        let u = (x as f32 + 0.5) * 2.0 / width as f32 - 1.0;
        let v = (y as f32 + 0.5) * 2.0 / height as f32 - 1.0;
        u * u + v * v < 1.0 && pixels[(y as u32 * stride + x as u32) as usize] >= 26
    };
    let mut edges: BTreeMap<[i32; 2], Vec<[i32; 2]>> = BTreeMap::new();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            if !inside(x, y) {
                continue;
            }
            for (neighbor, from, to) in [
                ([x, y - 1], [x, y], [x + 1, y]),
                ([x + 1, y], [x + 1, y], [x + 1, y + 1]),
                ([x, y + 1], [x + 1, y + 1], [x, y + 1]),
                ([x - 1, y], [x, y + 1], [x, y]),
            ] {
                if !inside(neighbor[0], neighbor[1]) {
                    edges.entry(from).or_default().push(to);
                }
            }
        }
    }
    let mut contours = Vec::new();
    while let Some((&start, _)) = edges.first_key_value() {
        let mut path = vec![start];
        let mut point = start;
        loop {
            let next = edges.get_mut(&point).unwrap().pop().unwrap();
            if edges[&point].is_empty() {
                edges.remove(&point);
            }
            if path.len() >= 2 {
                let a = path[path.len() - 2];
                if (point[0] - a[0]) * (next[1] - point[1])
                    == (point[1] - a[1]) * (next[0] - point[0])
                {
                    path.pop();
                }
            }
            path.push(next);
            point = next;
            if point == start {
                break;
            }
        }
        contours.push(
            path.into_iter()
                .map(|[x, y]| {
                    [
                        x as f32 * 2.0 / width as f32 - 1.0,
                        y as f32 * 2.0 / height as f32 - 1.0,
                    ]
                })
                .collect(),
        );
    }
    contours
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_contours_include_holes_and_remain_closed() {
        let mut pixels = vec![0; 64];
        for y in 2..6 {
            for x in 2..6 {
                pixels[y * 8 + x] = 255;
            }
        }
        pixels[3 * 8 + 3] = 0;
        let paths = mask_outline(8, 8, 8, &pixels);
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().all(|p| p.first() == p.last()));
        assert!(paths.iter().all(|p| p.len() <= 6));
        assert!(mask_outline(8, 8, 8, &[0; 64]).is_empty());
    }
}
