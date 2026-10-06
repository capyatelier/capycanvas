use crate::Point;

pub const MAX_OFFSET: i64 = 1 << 24;

pub fn checked_add(a: [i64; 2], b: [i64; 2]) -> Option<[i64; 2]> {
    Some([a[0].checked_add(b[0])?, a[1].checked_add(b[1])?])
}
pub fn checked_sub(a: [i64; 2], b: [i64; 2]) -> Option<[i64; 2]> {
    Some([a[0].checked_sub(b[0])?, a[1].checked_sub(b[1])?])
}
pub fn admitted(offset: [i64; 2]) -> bool {
    offset.iter().all(|value| value.unsigned_abs() <= MAX_OFFSET as u64)
}
pub fn point(offset: [i64; 2]) -> Point {
    Point { x: offset[0] as f32, y: offset[1] as f32 }
}
pub fn exact(point: Point) -> Option<[i64; 2]> {
    let values = [point.x, point.y];
    values.iter().all(|v| v.is_finite() && v.fract() == 0. && v.abs() <= MAX_OFFSET as f32).then(|| values.map(|v| v as i64))
}
pub fn rounded(point: Point) -> Option<[i64; 2]> {
    exact(Point { x: point.x.round(), y: point.y.round() })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offsets_round_halves_away_from_zero_and_refuse_fractions_or_overflow() {
        assert_eq!(rounded(Point { x: 2.5, y: -2.5 }), Some([3, -3]));
        assert_eq!(rounded(Point { x: 0.49, y: -0.49 }), Some([0, 0]));
        assert_eq!(exact(Point { x: 3., y: -256. }), Some([3, -256]));
        assert_eq!(exact(Point { x: 0.25, y: 0. }), None);
        assert_eq!(exact(Point { x: f32::NAN, y: 0. }), None);
        assert_eq!(rounded(Point { x: 2. * MAX_OFFSET as f32, y: 0. }), None);
        assert_eq!(checked_add([i64::MAX, 0], [1, 0]), None);
        assert!(admitted([MAX_OFFSET, -MAX_OFFSET]));
        assert!(!admitted([MAX_OFFSET + 1, 0]));
        assert_eq!(checked_sub([i64::MIN, 0], [1, 0]), None);
        assert_eq!(checked_sub([7, -3], [5, 4]), Some([2, -7]));
    }
}
