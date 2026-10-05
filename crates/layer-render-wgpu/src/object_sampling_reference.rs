fn stretch(j: [f64; 4]) -> [f64; 3] {
    let [a, b, c, d] = j;
    let [xx, xy, yy] = [a*a+b*b, a*c+b*d, c*c+d*d];
    let angle = 0.5*(2.*xy).atan2(xx-yy);
    let (sin, cos) = angle.sin_cos();
    let major = ((xx+yy+(xx-yy).hypot(2.*xy))*0.5).max(0.).sqrt().max(1.);
    let minor = ((xx+yy-(xx-yy).hypot(2.*xy))*0.5).max(0.).sqrt().max(1.);
    [major*cos*cos+minor*sin*sin, (major-minor)*sin*cos, major*sin*sin+minor*cos*cos]
}

pub(super) fn sample(q: [f64; 2], j: [f64; 4], nearest: bool, mut pixel: impl FnMut(i64, i64) -> [f64; 4]) -> [f64; 4] {
    if nearest { return pixel(q[0].floor() as i64, q[1].floor() as i64); }
    let [xx, xy, yy] = stretch(j);
    let determinant = xx*yy-xy*xy;
    let inverse = [yy/determinant, -xy/determinant, xx/determinant];
    let radius = [xx+xy.abs(), yy+xy.abs()];
    let low: [i64; 2] = std::array::from_fn(|axis| (q[axis]-radius[axis]-0.5).floor() as i64);
    let high: [i64; 2] = std::array::from_fn(|axis| (q[axis]+radius[axis]-0.5).ceil() as i64);
    let mut sum = [0.; 4];
    let mut normalization = 0.;
    for y in low[1]..=high[1] { for x in low[0]..=high[0] {
        let [dx, dy] = [x as f64+0.5-q[0], y as f64+0.5-q[1]];
        let [rx, ry] = [inverse[0]*dx+inverse[1]*dy, inverse[1]*dx+inverse[2]*dy];
        let weight = (1.-rx.abs()).max(0.)*(1.-ry.abs()).max(0.);
        normalization += weight;
        if weight > 0. { let value = pixel(x,y); for channel in 0..4 { sum[channel] += value[channel]*weight; } }
    }}
    sum.map(|value| value/normalization)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn permutations_and_nearest_preserve_signed_hdr_samples() {
        let pixel = |x: i64, y: i64| [x as f64-2., y as f64*4., -0.25, 0.5];
        for j in [[1.,0.,0.,1.], [0.,-1.,1.,0.], [-1.,0.,0.,1.]] {
            for y in -4..4 { for x in -4..4 {
                let q = [x as f64+0.5,y as f64+0.5];
                assert_eq!(sample(q,j,false,pixel),pixel(x,y));
            }}
        }
        assert_eq!(sample([3.7,2.8],[64.,0.,0.,64.],true,pixel),pixel(3,2));
    }
    #[test]
    fn strong_reductions_average_checkerboard_without_a_tap_cap() {
        for reduction in [8.,16.,32.,64.,19.7] {
            let actual = sample([100.5,100.5],[reduction,0.,0.,reduction],false,|x,y| {
                let v = ((x+y).rem_euclid(2)) as f64; [v,v,v,1.]
            });
            assert!((actual[0]-0.5).abs()<2e-5,"{reduction}: {actual:?}");
            assert_eq!(actual[3],1.);
        }
    }
    #[test]
    fn finite_image_keeps_the_transparent_lattice_in_its_denominator() {
        let value = sample([0.,0.],[64.,0.,0.,64.],false,|x,y| if x>=0 && y>=0 { [1.;4] } else { [0.;4] });
        for channel in value { assert!((channel-0.25).abs()<1e-12); }
    }
    #[test]
    fn mirrored_rotated_anisotropy_preserves_unminified_detail_and_total_weight() {
        for angle in [0.13f64,0.47,1.19] {
            let (s,c) = angle.sin_cos();
            for mirror in [-1.,1.] {
                let j = [64.*c*mirror,-s,64.*s*mirror,c];
                let value = sample([100.25,100.75],j,false,|_,_| [3.,-4.,0.25,1.]);
                for (actual, expected) in value.into_iter().zip([3.,-4.,0.25,1.]) { assert!((actual-expected).abs()<1e-11); }
                let other = sample([100.25,100.75],[-j[0],j[1],-j[2],j[3]],false,|x,y| {
                    let v = ((x*3+y*7).rem_euclid(11)) as f64/11.; [v;4]
                });
                let original = sample([100.25,100.75],j,false,|x,y| {
                    let v = ((x*3+y*7).rem_euclid(11)) as f64/11.; [v;4]
                });
                assert_eq!(other,original);
            }
        }
    }
    #[test]
    fn anisotropic_reduction_preserves_the_unminified_axis() {
        for y in [100i64,101] {
            let value=sample([100.5,y as f64+0.5],[64.,0.,0.,1.],false,|_,y|[(y.rem_euclid(2)) as f64;4]);
            assert_eq!(value,[y.rem_euclid(2) as f64;4]);
            assert!((value[0]-0.5).abs()>0.49);
        }
    }
    #[test]
    fn box_mip_and_capped_grid_are_not_the_tent_contract() {
        let tent = sample([24.,0.5],[32.,0.,0.,1.],false,|x,_| if x>=0 {[1.;4]} else {[0.;4]});
        assert!((tent[0]-0.96875).abs()<1e-12);
        let box_mip = 1.;
        assert!(box_mip-tent[0]>0.03);
        let tent = sample([100.5,100.5],[32.,0.,0.,32.],false,|x,y| [((x+y).rem_euclid(2)) as f64;4]);
        let capped = (0..16).flat_map(|y| (0..16).map(move |x| (((85+2*x)+(85+2*y))%2) as f64)).sum::<f64>()/256.;
        assert_eq!(capped,0.);
        assert!((tent[0]-capped-0.5).abs()<1e-12);
    }
}
