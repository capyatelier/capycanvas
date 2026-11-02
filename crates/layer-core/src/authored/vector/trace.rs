use crate::SelectionPixels;
use std::collections::HashMap;

pub struct ContourTrace {
    pixels: SelectionPixels,
    bounds: [i64; 4],
    cursor: [i64; 2],
    links: HashMap<u64, (u64, [f64; 2])>,
    starts: Vec<u64>,
    start: usize,
    following: Option<(u64, u64)>,
    contour: Vec<[f64; 2]>,
    contours: Vec<Vec<[f64; 2]>>,
    area: f64,
    marching: bool,
}
impl ContourTrace {
    pub fn new(pixels: SelectionPixels) -> Self {
        let [x0, y0, x1, y1] = pixels.bounds().map(i64::from);
        Self { pixels, bounds: [x0 - 1, y0 - 1, x1, y1], cursor: [x0 - 1, y0 - 1], links: HashMap::new(), starts: Vec::new(), start: 0,
            following: None, contour: Vec::new(), contours: Vec::new(), area: 0., marching: x0 < x1 && y0 < y1 }
    }
    fn coverage(&self, x: i64, y: i64) -> u8 {
        let [x0, y0, x1, y1] = self.pixels.bounds().map(i64::from);
        if x < x0 || y < y0 || x >= x1 || y >= y1 { return 0; }
        let count = self.pixels.pixels_per_word();
        let stride = self.pixels.extent()[0].div_ceil(count) as usize;
        let bits = 32 / count;
        ((self.pixels.words()[y as usize * stride + x as usize / count as usize] >> (x as u32 % count * bits)) & ((1 << bits) - 1)) as u8
    }
    fn uniform_run(&self, quota: usize) -> usize {
        let [x, y] = self.cursor;
        let [x0, y0, x1, y1] = self.pixels.bounds().map(i64::from);
        let count = i64::from(self.pixels.pixels_per_word());
        if x < x0 || x < 0 { return 0; }
        let end = (x / count + 1) * count;
        if end > x1 { return 0; }
        let run = (end - x - 1).min(self.bounds[2] - x).max(0) as usize;
        if run == 0 { return 0; }
        let stride = self.pixels.extent()[0].div_ceil(count as u32) as usize;
        let word = |y: i64| if y < y0 || y >= y1 { 0 } else { self.pixels.words()[y as usize * stride + (x / count) as usize] };
        let a = word(y);
        let full = if count == 4 { u32::MAX } else { 0x4444_4444 };
        if (a == 0 || a == full) && word(y + 1) == a { run.min(quota) } else { 0 }
    }
    fn cell(&mut self) {
        let [x, y] = self.cursor;
        let values = [self.coverage(x, y), self.coverage(x + 1, y), self.coverage(x + 1, y + 1), self.coverage(x, y + 1)];
        let threshold = if self.pixels.coverage_format() == 2 { 127.5 } else { 2. };
        let case = values.iter().enumerate().fold(0, |mask, (i, value)| mask | (usize::from(f64::from(*value) >= threshold) << i));
        let pairs: &[(usize, usize)] = match case {
            1 => &[(0, 3)], 2 => &[(1, 0)], 3 => &[(1, 3)], 4 => &[(2, 1)], 5 => &[(0, 3), (2, 1)],
            6 => &[(2, 0)], 7 => &[(2, 3)], 8 => &[(3, 2)], 9 => &[(0, 2)], 10 => &[(1, 0), (3, 2)],
            11 => &[(1, 2)], 12 => &[(3, 1)], 13 => &[(0, 1)], 14 => &[(3, 0)], _ => &[],
        };
        let stride = u64::from(self.pixels.extent()[0]) + 2;
        let edge = |side: usize| {
            let (a, b, px, py, axis) = match side {
                0 => (0, 1, x, y, 0), 1 => (1, 2, x + 1, y, 1), 2 => (3, 2, x, y + 1, 0), _ => (0, 3, x, y, 1),
            };
            let t = (threshold - f64::from(values[a])) / (f64::from(values[b]) - f64::from(values[a]));
            let point = [px as f64 + 0.5 + if axis == 0 { t } else { 0. }, py as f64 + 0.5 + if axis == 1 { t } else { 0. }];
            let key = (((py + 1) as u64 * stride + (px + 1) as u64) * 2) + axis;
            (key, point)
        };
        for &(from, to) in pairs {
            let (from, point) = edge(from);
            let (to, _) = edge(to);
            self.links.insert(from, (to, point));
            self.starts.push(from);
        }
    }
    pub fn advance(&mut self, quota: usize) -> Option<Vec<Vec<[f64; 2]>>> {
        let mut spent = 0;
        while spent < quota {
            if self.marching {
                let run = self.uniform_run(quota - spent);
                if run == 0 { self.cell(); }
                let run = run.max(1);
                spent += run;
                self.cursor[0] += run as i64;
                if self.cursor[0] >= self.bounds[2] { self.cursor[0] = self.bounds[0]; self.cursor[1] += 1; }
                self.marching = self.cursor[1] < self.bounds[3];
                continue;
            }
            spent += 1;
            let (first, current) = match self.following {
                Some(pair) => pair,
                None => {
                    let Some(&first) = self.starts.get(self.start) else { return Some(std::mem::take(&mut self.contours)); };
                    self.start += 1;
                    if !self.links.contains_key(&first) { continue; }
                    (first, first)
                }
            };
            let Some((next, point)) = self.links.remove(&current) else { self.following = None; self.contour.clear(); self.area = 0.; continue; };
            if let Some(previous) = self.contour.last() { self.area += previous[0] * point[1] - point[0] * previous[1]; }
            if self.contour.len() >= 2 {
                let a = self.contour[self.contour.len() - 2];
                let b = self.contour[self.contour.len() - 1];
                let u = [b[0] - a[0], b[1] - a[1]];
                let v = [point[0] - b[0], point[1] - b[1]];
                if u[0] * v[1] == u[1] * v[0] && u[0] * v[0] + u[1] * v[1] >= 0. { self.contour.pop(); }
            }
            self.contour.push(point);
            if next == first {
                let a = self.contour[0];
                self.area += point[0] * a[1] - a[0] * point[1];
                if self.area.abs() >= 8. { self.contours.push(std::mem::take(&mut self.contour)); } else { self.contour.clear(); }
                self.area = 0.; self.following = None;
            } else { self.following = Some((first, next)); }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mask(size: [u32; 2], byte: bool, sample: impl Fn(u32, u32) -> u8) -> SelectionPixels {
        let count = if byte { 4 } else { 8 };
        let stride = size[0].div_ceil(count) as usize;
        let mut words = vec![0; stride * size[1] as usize];
        for y in 0..size[1] { for x in 0..size[0] { words[y as usize * stride + (x / count) as usize] |= u32::from(sample(x, y)) << (x % count * (32 / count)); } }
        if byte { SelectionPixels::bytes(size, [0, 0, size[0], size[1]], words).unwrap() } else { SelectionPixels::new(size, [0, 0, size[0], size[1]], words).unwrap() }
    }
    fn trace(pixels: SelectionPixels, quota: usize) -> Vec<Vec<[f64; 2]>> {
        let mut job = ContourTrace::new(pixels);
        loop { if let Some(contours) = job.advance(quota) { return contours; } }
    }
    fn area(points: &[[f64; 2]]) -> f64 {
        points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f64>() * 0.5
    }
    #[test]
    fn contours_keep_outer_and_hole_interiors_on_the_left() {
        let pixels = mask([24, 24], false, |x, y| if (2..22).contains(&x) && (2..22).contains(&y) && !((8..16).contains(&x) && (8..16).contains(&y)) { 4 } else { 0 });
        let contours = trace(pixels, 17);
        assert_eq!(contours.len(), 2);
        assert!(contours.iter().any(|c| area(c) > 0.));
        assert!(contours.iter().any(|c| area(c) < 0.));
    }
    #[test]
    fn contours_keep_diagonal_saddles_four_connected_and_drop_tiny_specks() {
        let pixels = mask([12, 12], false, |x, y| if ((1..4).contains(&x) && (1..4).contains(&y)) || ((4..7).contains(&x) && (4..7).contains(&y)) { 4 } else { 0 });
        let contours = trace(pixels, 7);
        assert_eq!(contours.len(), 2);
        assert!(contours.iter().all(|c| area(c) > 0.));
        let speck = mask([8, 8], false, |x, y| if (2..5).contains(&x) && y == 3 { 4 } else { 0 });
        assert!(trace(speck, 1).is_empty());
    }
    #[test]
    fn contours_interpolate_byte_coverage_at_one_half() {
        let pixels = mask([10, 12], true, |x, y| if (2..10).contains(&y) { match x { 1 => 64, 2..=7 => 192, _ => 0 } } else { 0 });
        let contours = trace(pixels, 11);
        assert_eq!(contours.len(), 1);
        let expected = 1.5 + (127.5 - 64.) / 128.;
        assert!(contours[0].iter().any(|p| (p[0] - expected).abs() < 1e-12));
        let below = mask([8, 8], true, |_, _| 127);
        assert!(trace(below, 10).is_empty());
    }
    #[test]
    fn contour_quotas_preserve_every_point_and_border_component() {
        let pixels = mask([96, 96], true, |x, y| {
            let radius = (f64::from(x) + 0.5 - 48.).hypot(f64::from(y) + 0.5 - 48.);
            ((35.5 - radius).clamp(0., 1.) * 255.).round() as u8
        });
        let all = trace(pixels.clone(), usize::MAX);
        assert_eq!(all, trace(pixels.clone(), 1));
        assert_eq!(all, trace(pixels, 262_144));
        let border = mask([9, 9], false, |_, _| 4);
        let contour = trace(border, 13);
        assert_eq!(contour.len(), 1);
        assert!(contour[0].iter().any(|p| p[0] == 0.));
        assert!(contour[0].iter().any(|p| p[1] == 9.));
    }
}
