pub const GAP: i32 = 12;
pub const HEIGHT: i32 = 120;
pub const WIDTH: i32 = 160;
pub const RASTER_BOUNDS: [u32; 2] = [220, 160];

pub fn over_checker(pixel: [f32; 4], position: [u32; 2], checker: [f32; 2]) -> [f64; 3] {
    let cell = crate::TRANSPARENCY_CHECKER_CELL as u32;
    let checker = f64::from(checker[((position[0] / cell + position[1] / cell) % 2) as usize]);
    std::array::from_fn(|c| f64::from(pixel[c]) + checker * (1. - f64::from(pixel[3])))
}

pub fn master_preview(extent: [u32; 2], space: layer_core::color::RgbSpace, read: &mut impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>) -> Result<([u32; 2], Vec<[f32; 4]>), String> {
    let mut preview = layer_color::AreaPreview::new(extent, RASTER_BOUNDS)?;
    let mut row = vec![[0.; 4]; extent[0] as usize];
    for y in 0..extent[1] { read(y, &mut row)?; preview.push(&row)?; }
    let (extent, mut pixels) = preview.finish()?;
    let target = layer_core::color::RgbSpace::Srgb;
    let matrix = space.linear_transform(target);
    for pixel in &mut pixels {
        let rgb = layer_core::color::rgb::apply(matrix, [pixel[0],pixel[1],pixel[2]].map(f64::from));
        pixel[..3].copy_from_slice(&rgb.map(|value|value as f32));
    }
    Ok((extent,pixels))
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
    #[test]
    fn master_preview_reads_full_source_and_keeps_extended_linear_samples() {
        let mut rows = Vec::new();
        let (extent,pixels) = master_preview([1,161], layer_core::color::RgbSpace::Srgb, &mut |y,row| {
            rows.push(y);
            row.fill([2.,-0.25,0.5,if y==160 {0.5}else{1.}]);
            Ok(())
        }).unwrap();
        assert_eq!(rows,(0..161).collect::<Vec<_>>());
        assert_eq!(extent,[1,160]);
        assert!(pixels.iter().all(|pixel|pixel[0]>1.9 && pixel[1] < -0.2));
        assert!(pixels.iter().any(|pixel|pixel[3]<1.));
    }
}
