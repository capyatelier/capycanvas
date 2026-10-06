use super::*;
use layer_core::Rect;
#[cfg(test)]
use layer_core::{Affine, LayerPlacement};

pub(crate) fn input_level(level: u32, preview: &layer_render::TransformPreview, extent: [u32; 2]) -> u32 {
    let bounds = Rect::from_extent(extent);
    let bounds = preview.selection.as_ref().map_or(bounds, |s| bounds.intersect(s.bounds()));
    let rate = preview.transform.magnification(bounds);
    selection_level(level, preview.selection.as_ref(), extent)
        .min((level as f32 - rate.log2()).floor().clamp(0., 4.) as u32)
}

pub(crate) fn selection_level(level: u32, selection: Option<&layer_core::Selection>, extent: [u32; 2]) -> u32 {
    level.min(4).saturating_sub(u32::from(keeps_pixels(selection, PixelRect::full(extent))))
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
        for map in [LayerPlacement::from_affine(enlarged), LayerPlacement::from_projective(perspective), layer_core::LayerPlacement { mesh: Some(Arc::new(mesh)), ..Default::default() }] {
            let rate = map.magnification(bounds);
            let preview = layer_render::TransformPreview { transaction: 1, target: SourceTarget::Paint(layer_core::PaintHandle::from_index(1)), moving: true,
                selection: None, transform: layer_core::ImageTransform { placement: map.clone(), ..Default::default() } };
            let local = input_level(4, &preview, extent);
            for y in 1..20 { for x in 1..20 {
                let p = Point { x: x as f32 * 512./20., y: y as f32 * 256./20. };
                let mapped = |p| map.map(p).unwrap();
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
