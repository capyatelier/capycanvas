//! Half-open pixel bounds with one empty representation. Construction and
//! clipping preserve ordered coordinates; empty regions have zero area and
//! visit no pages. Fields are private so callers cannot fabricate a sentinel
//! that becomes an overflowing GPU scissor after page-local conversion.
use super::PAGE_SIZE;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PixelRect {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

impl PixelRect {
    pub const EMPTY: Self = Self {
        min_x: 0,
        min_y: 0,
        max_x: 0,
        max_y: 0,
    };
    pub fn new(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        if min_x >= max_x || min_y >= max_y {
            Self::EMPTY
        } else {
            Self {
                min_x,
                min_y,
                max_x,
                max_y,
            }
        }
    }
    pub fn full(extent: [u32; 2]) -> Self {
        Self::new(0, 0, extent[0], extent[1])
    }
    pub fn min_x(self) -> u32 {
        self.min_x
    }
    pub fn min_y(self) -> u32 {
        self.min_y
    }
    pub fn max_x(self) -> u32 {
        self.max_x
    }
    pub fn max_y(self) -> u32 {
        self.max_y
    }
    pub fn is_empty(self) -> bool {
        self.max_x == 0
    }
    pub fn width(self) -> u32 {
        self.max_x - self.min_x
    }
    pub fn height(self) -> u32 {
        self.max_y - self.min_y
    }
    pub fn area(self) -> u64 {
        self.width() as u64 * self.height() as u64
    }
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self::new(
            self.min_x.min(other.min_x),
            self.min_y.min(other.min_y),
            self.max_x.max(other.max_x),
            self.max_y.max(other.max_y),
        )
    }
    pub fn intersect(self, other: Self) -> Self {
        Self::new(
            self.min_x.max(other.min_x),
            self.min_y.max(other.min_y),
            self.max_x.min(other.max_x),
            self.max_y.min(other.max_y),
        )
    }
    pub fn expand(self, radius: u32, extent: [u32; 2]) -> Self {
        if self.is_empty() {
            return self;
        }
        Self::new(
            self.min_x.saturating_sub(radius),
            self.min_y.saturating_sub(radius),
            self.max_x.saturating_add(radius).min(extent[0]),
            self.max_y.saturating_add(radius).min(extent[1]),
        )
    }
    /// Clip to an image window and express the result in its texture coordinates.
    pub fn window_local(self, window: Self) -> Self {
        let region = self.intersect(window);
        if region.is_empty() {
            return Self::EMPTY;
        }
        Self::new(
            region.min_x - window.min_x,
            region.min_y - window.min_y,
            region.max_x - window.min_x,
            region.max_y - window.min_y,
        )
    }
    /// Non-overlapping top, bottom, left and right regions of self - other.
    pub fn subtract(self, other: Self) -> [Self; 4] {
        let overlap = self.intersect(other);
        if overlap.is_empty() {
            return [self, Self::EMPTY, Self::EMPTY, Self::EMPTY];
        }
        [
            Self::new(self.min_x, self.min_y, self.max_x, overlap.min_y),
            Self::new(self.min_x, overlap.max_y, self.max_x, self.max_y),
            Self::new(self.min_x, overlap.min_y, overlap.min_x, overlap.max_y),
            Self::new(overlap.max_x, overlap.min_y, self.max_x, overlap.max_y),
        ]
    }
    /// Clip and translate together. Even an unrelated page yields bounded,
    /// empty coordinates; callers never subtract an origin from a sentinel.
    pub fn page_local(self, coordinate: [u32; 2]) -> Self {
        let origin_x = coordinate[0] * PAGE_SIZE;
        let origin_y = coordinate[1] * PAGE_SIZE;
        Self::new(
            self.min_x.saturating_sub(origin_x).min(PAGE_SIZE),
            self.min_y.saturating_sub(origin_y).min(PAGE_SIZE),
            self.max_x.saturating_sub(origin_x).min(PAGE_SIZE),
            self.max_y.saturating_sub(origin_y).min(PAGE_SIZE),
        )
    }
}

pub(super) fn page_rect(coordinate: [u32; 2]) -> PixelRect {
    let min_x = coordinate[0] * PAGE_SIZE;
    let min_y = coordinate[1] * PAGE_SIZE;
    PixelRect::new(min_x, min_y, min_x + PAGE_SIZE, min_y + PAGE_SIZE)
}

pub(super) fn page_coordinates(rect: PixelRect) -> impl DoubleEndedIterator<Item = [u32; 2]> + Clone {
    let xs = rect.min_x / PAGE_SIZE..rect.max_x.div_ceil(PAGE_SIZE);
    let ys = rect.min_y / PAGE_SIZE..rect.max_y.div_ceil(PAGE_SIZE);
    ys.flat_map(move |y| xs.clone().map(move |x| [x, y]))
}

pub(super) fn pixel_rect(rect: layer_core::Rect, extent: [u32; 2]) -> PixelRect {
    if rect.is_empty() {
        return PixelRect::EMPTY;
    }
    PixelRect::new(
        rect.min.x.floor().max(0.).min(extent[0] as f32) as u32,
        rect.min.y.floor().max(0.).min(extent[1] as f32) as u32,
        rect.max.x.ceil().max(0.).min(extent[0] as f32) as u32,
        rect.max.y.ceil().max(0.).min(extent[1] as f32) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_partition_preserves_area_and_local_bounds() {
        let edges = [0, 1, 255, 256, 257, 511, 512, 1537];
        for x in edges {
            for y in edges {
                for right in edges {
                    for bottom in edges {
                        let rect = PixelRect::new(x, y, right, bottom);
                        let mut area = 0;
                        for page in page_coordinates(rect) {
                            let local = rect.page_local(page);
                            assert!(!local.is_empty());
                            assert!(local.max_x() <= PAGE_SIZE && local.max_y() <= PAGE_SIZE);
                            assert_eq!(local.area(), rect.intersect(page_rect(page)).area());
                            area += local.area();
                        }
                        assert_eq!(area, rect.area());
                        assert_eq!(rect.union(PixelRect::EMPTY), rect);
                        assert_eq!(rect.intersect(PixelRect::EMPTY), PixelRect::EMPTY);
                        for page in [[0, 0], [5, 0], [9, 9]] {
                            let local = rect.page_local(page);
                            assert!(local.max_x() <= PAGE_SIZE && local.max_y() <= PAGE_SIZE);
                            assert_eq!(local.area(), rect.intersect(page_rect(page)).area());
                        }
                    }
                }
            }
        }
    }
}
