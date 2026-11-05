pub const GAP: i32 = 12;
pub const HEIGHT: i32 = 120;
pub const WIDTH: i32 = 160;
pub const RASTER_BOUNDS: [u32; 2] = [220, 160];

pub fn over_checker(pixel: [f32; 4], position: [u32; 2], checker: [f32; 2]) -> [f64; 3] {
    let cell = crate::TRANSPARENCY_CHECKER_CELL as u32;
    let checker = f64::from(checker[((position[0] / cell + position[1] / cell) % 2) as usize]);
    std::array::from_fn(|c| f64::from(pixel[c]) + checker * (1. - f64::from(pixel[3])))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_checker_preserves_linear_premultiplied_and_extended_values() {
        let checker = crate::TRANSPARENCY_CHECKER.map(|gray| gray.linear()[0]);
        for position in [[0,0],[5,0],[0,5],[5,5]] {
            let gray = f64::from(checker[((position[0]/5+position[1]/5)%2) as usize]);
            assert_eq!(over_checker([0.;4],position,checker),[gray;3]);
            assert_eq!(over_checker([2.,0.5,-0.1,1.],position,checker),[2.,0.5,f64::from(-0.1_f32)]);
            assert_eq!(over_checker([0.1,0.2,0.3,0.5],position,checker),[0.1_f32,0.2,0.3].map(|v|f64::from(v)+gray*0.5));
        }
    }
}
