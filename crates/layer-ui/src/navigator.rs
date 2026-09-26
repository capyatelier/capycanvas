//! The preview is document-oriented. Only its work-area outline follows the
//! camera, including reflection and rotation. All dimensions here are UI units.
use crate::{Bounds, Camera};
use layer_core::Point;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct NavigatorGeometry {
    pub image: Bounds,
    pub work_area: [[f32; 2]; 4],
    pub overview_aspect: f32,
    scale: f32,
}
impl NavigatorGeometry {
    pub fn overview_aspect(document: [u32; 2]) -> f32 {
        (document[1] as f32 / document[0].max(1) as f32).clamp(0.25, 1.0)
    }
    pub fn new(camera: &Camera, document: [u32; 2], viewport: [f32; 2]) -> Option<Self> {
        if document.contains(&0) || !viewport.into_iter().all(|n| n.is_finite() && n > 8.0) {
            return None;
        }
        let scale = ((viewport[0] - 8.0) / document[0] as f32)
            .min((viewport[1] - 8.0) / document[1] as f32);
        let width = document[0] as f32 * scale;
        let height = document[1] as f32 * scale;
        let image = Bounds {
            x: (viewport[0] - width) * 0.5,
            y: (viewport[1] - height) * 0.5,
            width,
            height,
        };
        let [x, y, w, h] = camera.work_area;
        let work_area = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|[x, y]| {
            let point = camera.input_transform().map(Point { x, y });
            [image.x + point.x * scale, image.y + point.y * scale]
        });
        Some(Self {
            image,
            work_area,
            overview_aspect: Self::overview_aspect(document),
            scale,
        })
    }
    pub fn document_point(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        [
            (x - self.image.x) / self.scale,
            (y - self.image.y) / self.scale,
        ]
    }
    pub fn in_work_area(&self, camera: &Camera, point: [f32; 2]) -> bool {
        let [x, y] = self.document_point(point);
        let [a, b, c, d, tx, ty] = camera.document_to_surface();
        let [wx, wy, width, height] = camera.work_area;
        Bounds {
            x: wx,
            y: wy,
            width,
            height,
        }
        .contains(a * x + c * y + tx, b * x + d * y + ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_uses_work_area_not_full_window_and_preserves_aspect() {
        let mut camera = Camera::new([1000, 500], [1200, 800]);
        camera.work_area = [200.0, 50.0, 600.0, 600.0];
        camera.zoom = 1.0;
        camera.center_on([500.0, 250.0]);
        for flipped in [[false, false], [true, false], [false, true], [true, true]] {
            camera.flipped = flipped;
            camera.rotation = 0.7;
            camera.center_on([500.0, 250.0]);
            let g = NavigatorGeometry::new(&camera, [1000, 500], [208.0, 208.0]).unwrap();
            assert_eq!(
                g.image,
                Bounds {
                    x: 4.0,
                    y: 54.0,
                    width: 200.0,
                    height: 100.0
                }
            );
            let [a, b, c, d, x, y] = camera.document_to_surface();
            for (corner, expected) in g.work_area.into_iter().zip([
                [200.0, 50.0],
                [800.0, 50.0],
                [800.0, 650.0],
                [200.0, 650.0],
            ]) {
                let [px, py] = g.document_point(corner);
                assert!((a * px + c * py + x - expected[0]).abs() < 0.001);
                assert!((b * px + d * py + y - expected[1]).abs() < 0.001);
            }
            assert!(g.in_work_area(&camera, [104.0, 104.0]));
            assert_eq!(g.overview_aspect, 0.5);
        }
    }

    #[test]
    fn overview_aspect_follows_the_document_between_a_strip_and_a_square() {
        for (document, aspect) in [
            ([1920, 1080], 0.5625),
            ([1000, 1000], 1.0),
            ([1080, 1920], 1.0),
            ([8000, 1000], 0.25),
        ] {
            assert_eq!(NavigatorGeometry::overview_aspect(document), aspect);
        }
    }
}
