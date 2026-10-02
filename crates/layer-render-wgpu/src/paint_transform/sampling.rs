use super::*;
use layer_core::{Affine, Rect, TransformMap};

pub(crate) fn input_level(level: u32, preview: &layer_render::TransformPreview, placement: Affine, extent: [u32; 2]) -> u32 {
    let bounds = Rect::from_extent(extent);
    let bounds = preview.selection.as_ref().map_or(bounds, |s| bounds.intersect(s.bounds()));
    let rate = stretch(&preview.transform.map, placement, bounds).max(placement.magnification());
    selection_level(level, placement, preview.selection.as_ref(), extent)
        .min((level as f32 - rate.log2()).floor().clamp(0., 4.) as u32)
}

pub(crate) fn selection_level(level: u32, placement: Affine, selection: Option<&layer_core::Selection>, extent: [u32; 2]) -> u32 {
    local_level(level, placement).saturating_sub(u32::from(keeps_pixels(selection, PixelRect::full(extent))))
}

fn stretch(map: &TransformMap, placement: Affine, bounds: Rect) -> f32 {
    match map {
        TransformMap::Affine(affine) => affine.then(placement).magnification(),
        TransformMap::Projective(projective) => {
            let Some(map) = projective.then(layer_core::Projective::from_affine(placement)) else { return f32::INFINITY; };
            if let Some(affine) = map.as_affine() { return affine.magnification(); }
            let [a,b,c,d,e,f,g,h,i] = map.0.map(f64::from);
            let mut weight = f64::INFINITY;
            let mut numerator = [0f64; 4];
            for p in bounds.corners() {
                let [x,y] = [f64::from(p.x), f64::from(p.y)];
                weight = weight.min(g*x + h*y + i);
                let n = [(a*h-b*g)*y + a*i-c*g, (d*h-e*g)*y + d*i-f*g,
                    (b*g-a*h)*x + b*i-c*h, (e*g-d*h)*x + e*i-f*h];
                for (bound, value) in numerator.iter_mut().zip(n) { *bound = bound.max(value.abs()); }
            }
            if weight <= 0. { return f32::INFINITY; }
            let [a,b,c,d] = numerator.map(|n| (n / weight.powi(2)) as f32);
            Affine([a,b,c,d,0.,0.]).magnification()
        }
        TransformMap::Mesh(mesh) => {
            let Some(inverse) = mesh.frame.inverse() else { return f32::INFINITY; };
            let [a,b,c,d,_,_] = inverse.0.map(f32::abs);
            let [pa,pb,pc,pd,_,_] = placement.0;
            let width = usize::from(mesh.cells[0]) * 3 + 1;
            let mut largest = 0f32;
            for row in 0..usize::from(mesh.cells[1]) {
                for column in 0..usize::from(mesh.cells[0]) {
                    let mut du = [0f32; 2];
                    let mut dv = [0f32; 2];
                    let at = |x,y| mesh.net[(row * 3 + y) * width + column * 3 + x];
                    for y in 0..4 { for x in 0..4 {
                        let p = at(x,y);
                        for (axis, delta, bound) in [(0, (x<3).then(|| at(x+1,y)), &mut du), (1, (y<3).then(|| at(x,y+1)), &mut dv)] {
                            if let Some(q) = delta {
                                let k = 3. * f32::from(mesh.cells[axis]);
                                bound[0] = bound[0].max((pa*(q.x-p.x) + pc*(q.y-p.y)).abs()*k);
                                bound[1] = bound[1].max((pb*(q.x-p.x) + pd*(q.y-p.y)).abs()*k);
                            }
                        }
                    }}
                    largest = largest.max(Affine([
                        du[0]*a+dv[0]*b, du[1]*a+dv[1]*b,
                        du[0]*c+dv[0]*d, du[1]*c+dv[1]*d, 0.,0.,
                    ]).magnification());
                }
            }
            largest
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::Point;

    #[test]
    fn mapped_input_texels_do_not_exceed_the_display_footprint() {
        let extent = [512,256];
        let bounds = Rect { min: Point::default(), max: Point { x: 512., y: 256. } };
        let enlarged = Affine::around(Point { x: 256., y: 128. }, [3.,2.5], 0.3, Point::default());
        let perspective = layer_core::Projective::rect_to_quad(bounds,
            [[0.,0.],[512.,0.],[900.,700.],[-400.,900.]].map(|[x,y]| Point {x,y})).unwrap();
        let mesh = layer_core::MeshMap::from_affine(bounds, [3,3], enlarged).unwrap()
            .move_node(5, Point { x: 120., y: -40. }).unwrap();
        for map in [TransformMap::Affine(enlarged), TransformMap::Projective(perspective), TransformMap::Mesh(Arc::new(mesh))] {
            for placement in [Affine::IDENTITY, Affine([0.3,0.2,-0.2,0.3,8.,4.])] {
                let rate = stretch(&map, placement, bounds);
                let preview = layer_render::TransformPreview { transaction: 1, layer: LayerId(1), moving: true,
                    selection: None, transform: layer_core::ImageTransform { map: map.clone(), ..Default::default() } };
                let local = input_level(4, &preview, placement, extent);
                for y in 1..20 { for x in 1..20 {
                    let p = Point { x: x as f32 * 512./20., y: y as f32 * 256./20. };
                    let mapped = |p| map.map(p).map(|p| placement.map(p)).unwrap();
                    let origin = mapped(p);
                    for angle in 0..16 {
                        let angle = angle as f32 * std::f32::consts::TAU/16.;
                        let q = mapped(Point { x: p.x + angle.cos()*0.05, y: p.y + angle.sin()*0.05 });
                        let observed = (q.x-origin.x).hypot(q.y-origin.y)/0.05;
                        assert!(observed <= rate*1.01, "{observed} > {rate}");
                        assert!(local == 0 || observed*(1<<local) as f32 <= 16.*1.01);
                    }
                }}
            }
        }
    }
}
