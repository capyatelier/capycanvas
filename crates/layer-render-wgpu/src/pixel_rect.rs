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
    pub fn to_rect(self) -> layer_core::Rect {
        layer_core::Rect {
            min: layer_core::Point { x: self.min_x as f32, y: self.min_y as f32 },
            max: layer_core::Point { x: self.max_x as f32, y: self.max_y as f32 },
        }
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct DocRect {
    pub min: [i64; 2],
    pub max: [i64; 2],
}
impl From<PixelRect> for DocRect {
    fn from(rect: PixelRect) -> Self {
        Self { min: [i64::from(rect.min_x()), i64::from(rect.min_y())], max: [i64::from(rect.max_x()), i64::from(rect.max_y())] }
    }
}
impl DocRect {
    pub fn expand(self, radius: u32) -> Self {
        if self.is_empty() { return self; }
        Self { min: self.min.map(|v| v.saturating_sub(i64::from(radius))), max: self.max.map(|v| v.saturating_add(i64::from(radius))) }
    }
    pub fn is_empty(self) -> bool { (0..2).any(|i| self.min[i] >= self.max[i]) }
    pub fn size(self) -> Result<[u32; 2], super::GpuRasterError> {
        if self.is_empty() { return Ok([0; 2]); }
        let side = |i: usize| self.max[i].checked_sub(self.min[i]).and_then(|v| u32::try_from(v).ok()).ok_or(super::GpuRasterError::SizeOverflow);
        Ok([side(0)?, side(1)?])
    }
    pub fn area(self) -> u64 {
        self.size().map_or(u64::MAX, |size| size.map(u64::from).into_iter().product())
    }
    pub fn local(self, rect: impl Into<Self>) -> PixelRect {
        let rect = rect.into();
        let min = std::array::from_fn::<_, 2, _>(|i| rect.min[i].max(self.min[i]));
        let max = std::array::from_fn::<_, 2, _>(|i| rect.max[i].min(self.max[i]));
        if (0..2).any(|i| min[i] >= max[i]) { return PixelRect::EMPTY; }
        let local = |value: i64, axis: usize| value.checked_sub(self.min[axis]).and_then(|n| u32::try_from(n).ok());
        let (Some(x), Some(y), Some(right), Some(bottom)) = (local(min[0], 0), local(min[1], 1), local(max[0], 0), local(max[1], 1)) else { return PixelRect::EMPTY; };
        PixelRect::new(x, y, right, bottom)
    }
    pub fn clamped(self, support: Self) -> Self {
        if self.is_empty() || support.is_empty() { return Self::default(); }
        Self {
            min: std::array::from_fn(|axis| self.min[axis].clamp(support.min[axis], support.max[axis] - 1)),
            max: std::array::from_fn(|axis| self.max[axis].clamp(support.min[axis] + 1, support.max[axis])),
        }
    }
    pub fn aligned(self, side: u32) -> Self {
        if self.is_empty() { return self; }
        let side = i64::from(side.max(1));
        Self { min: self.min.map(|n| n.div_euclid(side).saturating_mul(side)), max: self.max.map(|n| n.div_euclid(side).saturating_mul(side).saturating_add(i64::from(n.rem_euclid(side) != 0) * side)) }
    }
    pub fn intersect(self, other: Self) -> Self {
        let min = std::array::from_fn(|i| self.min[i].max(other.min[i]));
        let max = std::array::from_fn(|i| self.max[i].min(other.max[i]));
        if (0..2).any(|i| min[i] >= max[i]) { Self::default() } else { Self { min, max } }
    }
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() { return other; }
        if other.is_empty() { return self; }
        Self { min: std::array::from_fn(|i| self.min[i].min(other.min[i])), max: std::array::from_fn(|i| self.max[i].max(other.max[i])) }
    }
    pub fn from_rect(rect: layer_core::Rect) -> Self {
        if rect.is_empty() { return Self::default(); }
        Self { min: [rect.min.x.floor() as i64, rect.min.y.floor() as i64], max: [rect.max.x.ceil() as i64, rect.max.y.ceil() as i64] }
    }
    pub fn in_frame(self, extent: [u32; 2]) -> PixelRect {
        DocRect::from(PixelRect::full(extent)).local(self)
    }
    pub fn to_rect(self) -> layer_core::Rect {
        let lower = |value:i64| {let rounded=value as f32;if rounded as i128>i128::from(value) {rounded.next_down()} else {rounded}};
        let upper = |value:i64| {let rounded=value as f32;if (rounded as i128)<i128::from(value) {rounded.next_up()} else {rounded}};
        layer_core::Rect { min: layer_core::Point { x: lower(self.min[0]), y: lower(self.min[1]) }, max: layer_core::Point { x: upper(self.max[0]), y: upper(self.max[1]) } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_dependencies_have_checked_unsigned_allocation_coordinates() {
        let window = DocRect::from(PixelRect::new(0, 3, 17, 29)).expand(19);
        assert_eq!(window.min, [-19, -16]);
        assert_eq!(window.max, [36, 48]);
        assert_eq!(window.size().unwrap(), [55, 64]);
        assert_eq!(window.local(PixelRect::new(0, 3, 17, 29)), PixelRect::new(19, 19, 36, 45));
        assert_eq!(window.in_frame([17, 29]), PixelRect::full([17, 29]));
        assert_eq!(window.local(DocRect { min: [-90, -90], max: [-80, -80] }), PixelRect::EMPTY);
        assert!(DocRect { min: [i64::MIN, 0], max: [i64::MAX, 1] }.size().is_err());
        assert_eq!(DocRect { min: [-90; 2], max: [-80; 2] }.clamped(window), DocRect { min: window.min, max: window.min.map(|n| n + 1) });
        assert_eq!(DocRect { min: [90; 2], max: [99; 2] }.clamped(window), DocRect { min: window.max.map(|n| n - 1), max: window.max });
    }
    #[test]
    fn legacy_rect_projection_encloses_small_signed_bounds_beyond_float_precision() {
        for min in [[33_554_433,33_554_433],[-33_554_433,-33_554_433]] {
            let bounds=DocRect {min,max:min.map(|value|value+1)};
            let projected=bounds.to_rect();
            assert!(!projected.is_empty());
            assert!(f64::from(projected.min.x)<=bounds.min[0] as f64 && f64::from(projected.min.y)<=bounds.min[1] as f64);
            assert!(f64::from(projected.max.x)>=bounds.max[0] as f64 && f64::from(projected.max.y)>=bounds.max[1] as f64);
        }
    }
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
