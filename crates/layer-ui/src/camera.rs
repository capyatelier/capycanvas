//! One camera/gesture implementation for native and browser event collectors.
//! Canvas coordinates are physical viewport pixels, independent of dock units.

use layer_engine::{PenPhase, ViewTransform};
use layer_render::ViewState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub revision: u64,
    pub viewport: [u32; 2],
    pub zoom: f32,
    pub rotation: f32,
    /// Document-axis reflections; affect presentation and input, never pixels.
    #[serde(default)]
    pub flipped: [bool; 2],
    pub translation: [f32; 2],
    /// Fitting bounds in physical full-window coordinates; updating these does
    /// not itself move the current view.
    pub work_area: [f32; 4],
}

impl Camera {
    pub fn new(document: [u32; 2], viewport: [u32; 2]) -> Self {
        let mut value = Self {
            revision: 0,
            viewport,
            zoom: 1.0,
            rotation: 0.0,
            flipped: [false; 2],
            translation: [0.0; 2],
            work_area: [0.0, 0.0, viewport[0] as f32, viewport[1] as f32],
        };
        value.fit(document);
        value
    }

    pub fn fit(&mut self, document: [u32; 2]) {
        let [_, _, width, height] = self.work_area;
        self.zoom = ((width * 0.9 / document[0].max(1) as f32)
            .min(height * 0.9 / document[1].max(1) as f32))
        .clamp(0.02, 16.0);
        self.rotation = 0.0;
        self.center_on([document[0] as f32 * 0.5, document[1] as f32 * 0.5]);
    }

    pub fn resize(&mut self, viewport: [u32; 2]) {
        if self.viewport == viewport {
            return;
        }
        if self.work_area == [0.0, 0.0, self.viewport[0] as f32, self.viewport[1] as f32] {
            self.work_area = [0.0, 0.0, viewport[0] as f32, viewport[1] as f32];
        }
        for (axis, size) in viewport.iter().enumerate() {
            self.translation[axis] += (*size as f32 - self.viewport[axis] as f32) * 0.5;
        }
        self.viewport = viewport;
        self.revision += 1;
    }

    /// Carries the document point at `from` to `to`, with anchored zoom/rotate.
    pub fn gesture(
        &mut self,
        from: [f32; 2],
        to: [f32; 2],
        scale: f32,
        rotation: f32,
    ) -> Result<(), String> {
        if !from
            .into_iter()
            .chain(to)
            .chain([scale, rotation])
            .all(f32::is_finite)
            || scale <= 0.0
        {
            return Err("Invalid camera gesture".into());
        }
        let zoom = (self.zoom * scale).clamp(0.02, 16.0);
        let scale = zoom / self.zoom;
        let (sin, cos) = rotation.sin_cos();
        let dx = self.translation[0] - from[0];
        let dy = self.translation[1] - from[1];
        let translation = [
            to[0] + scale * (cos * dx - sin * dy),
            to[1] + scale * (sin * dx + cos * dy),
        ];
        if !translation.into_iter().all(f32::is_finite) {
            return Err("Camera movement is too large".into());
        }
        self.translation = translation;
        self.zoom = zoom;
        self.rotation = (self.rotation + rotation + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        self.revision += 1;
        Ok(())
    }

    pub fn document_to_surface(&self) -> [f32; 6] {
        let (sin, cos) = self.rotation.sin_cos();
        let x = if self.flipped[0] {
            -self.zoom
        } else {
            self.zoom
        };
        let y = if self.flipped[1] {
            -self.zoom
        } else {
            self.zoom
        };
        [
            cos * x,
            sin * x,
            -sin * y,
            cos * y,
            self.translation[0],
            self.translation[1],
        ]
    }

    pub fn input_transform(&self) -> ViewTransform {
        let [a, b, c, d, x, y] = self.document_to_surface();
        let determinant = a * d - b * c;
        let [a, b, c, d] = [
            d / determinant,
            -b / determinant,
            -c / determinant,
            a / determinant,
        ];
        ViewTransform {
            revision: self.revision,
            surface_to_document: [a, b, c, d, -a * x - c * y, -b * x - d * y],
        }
    }

    pub fn work_area_center(&self) -> [f32; 2] {
        let [x, y, width, height] = self.work_area;
        [x + width * 0.5, y + height * 0.5]
    }

    pub fn center_on(&mut self, point: [f32; 2]) {
        let [a, b, c, d, _, _] = self.document_to_surface();
        let [x, y] = self.work_area_center();
        self.translation = [
            x - a * point[0] - c * point[1],
            y - b * point[0] - d * point[1],
        ];
        self.revision += 1;
    }

    pub fn flip(&mut self, horizontal: bool) {
        let [x, y] = self.work_area_center();
        let center = self.input_transform().map(layer_core::Point { x, y });
        self.flipped[usize::from(!horizontal)] ^= true;
        self.center_on([center.x, center.y]);
    }

    pub fn view(&self) -> ViewState {
        ViewState {
            width_px: self.viewport[0],
            height_px: self.viewport[1],
            document_to_surface: self.document_to_surface(),
            background_rgba_linear: [1.0; 4],
        }
    }
}

/// A single finger never paints or pans. Adding/removing fingers rebases the
/// two-touch gesture, so changing pointer identities cannot jump the canvas.
#[derive(Default)]
pub struct TouchGesture {
    points: BTreeMap<u64, [f32; 2]>,
}
impl TouchGesture {
    pub fn is_active(&self) -> bool {
        !self.points.is_empty()
    }
    pub fn clear(&mut self) {
        self.points.clear();
    }

    pub fn update(
        &mut self,
        camera: &mut Camera,
        id: u64,
        phase: PenPhase,
        point: [f32; 2],
        single_pan: bool,
    ) -> bool {
        if !point.into_iter().all(f32::is_finite) {
            return false;
        }
        let before = self.pair();
        let previous = self.points.get(&id).copied();
        match phase {
            PenPhase::Down => {
                self.points.insert(id, point);
            }
            PenPhase::Move => {
                if let Some(value) = self.points.get_mut(&id) {
                    *value = point;
                } else {
                    return false;
                }
            }
            PenPhase::Up | PenPhase::Cancel => {
                self.points.remove(&id);
            }
            PenPhase::Hover => return false,
        }
        if phase != PenPhase::Move {
            return false;
        }
        if single_pan
            && self.points.len() == 1
            && let Some(previous) = previous
        {
            return camera.gesture(previous, point, 1.0, 0.0).is_ok();
        }
        let (Some([a, b]), Some([c, d])) = (before, self.pair()) else {
            return false;
        };
        let before_length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let after_length = (d[0] - c[0]).hypot(d[1] - c[1]);
        if before_length < 4.0 || after_length < 4.0 {
            return false;
        }
        let center = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let rotation = (d[1] - c[1]).atan2(d[0] - c[0]) - (b[1] - a[1]).atan2(b[0] - a[0]);
        camera
            .gesture(
                center(a, b),
                center(c, d),
                after_length / before_length,
                rotation,
            )
            .is_ok()
    }

    fn pair(&self) -> Option<[[f32; 2]; 2]> {
        if self.points.len() != 2 {
            return None;
        }
        let mut values = self.points.values();
        Some([*values.next()?, *values.next()?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::Point;
    fn near(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001,
            "{a:?} != {b:?}"
        );
    }
    #[test]
    fn transform_roundtrip_and_anchored_gesture() {
        let mut camera = Camera::new([2048, 1536], [1000, 800]);
        let anchor = camera.input_transform().map(Point { x: 125.0, y: 220.0 });
        camera
            .gesture([125.0, 220.0], [200.0, 350.0], 1.7, 0.6)
            .unwrap();
        near(
            anchor,
            camera.input_transform().map(Point { x: 200.0, y: 350.0 }),
        );
        let forward = ViewTransform {
            revision: 0,
            surface_to_document: camera.document_to_surface(),
        };
        near(
            Point { x: 532.0, y: 407.0 },
            camera
                .input_transform()
                .map(forward.map(Point { x: 532.0, y: 407.0 })),
        );
        let before = camera.clone();
        assert!(camera.gesture([0.0; 2], [f32::NAN, 0.0], 1.0, 0.0).is_err());
        assert_eq!(camera, before);
    }
    #[test]
    fn reflected_views_preserve_center_and_input_roundtrip() {
        let mut camera = Camera::new([2048, 1536], [1200, 900]);
        camera.work_area = [220.0, 48.0, 650.0, 810.0];
        camera
            .gesture([300.0, 200.0], [280.0, 170.0], 2.0, 0.6)
            .unwrap();
        let [x, y] = camera.work_area_center();
        let center = camera.input_transform().map(Point { x, y });
        for horizontal in [true, false, true, false] {
            camera.flip(horizontal);
            near(center, camera.input_transform().map(Point { x, y }));
            let forward = ViewTransform {
                revision: 0,
                surface_to_document: camera.document_to_surface(),
            };
            for point in [
                Point { x: 0.0, y: 0.0 },
                Point {
                    x: 1372.0,
                    y: 561.0,
                },
            ] {
                near(point, camera.input_transform().map(forward.map(point)));
            }
            let anchor = camera.input_transform().map(Point { x: 125.0, y: 220.0 });
            let mut moved = camera.clone();
            moved
                .gesture([125.0, 220.0], [200.0, 350.0], 1.7, 0.6)
                .unwrap();
            near(
                anchor,
                moved.input_transform().map(Point { x: 200.0, y: 350.0 }),
            );
        }
        assert_eq!(camera.flipped, [false; 2]);
        camera.flip(true);
        camera.fit([2048, 1536]);
        assert_eq!(camera.flipped, [true, false]);
        near(
            Point {
                x: 1024.0,
                y: 768.0,
            },
            camera.input_transform().map(Point { x, y }),
        );
    }
    #[test]
    fn two_fingers_rotate_zoom_and_pan_without_lifecycle_jumps() {
        let mut camera = Camera::new([1000, 1000], [1000, 1000]);
        let mut touch = TouchGesture::default();
        let before = camera.clone();
        assert!(!touch.update(&mut camera, 1, PenPhase::Down, [100.0, 100.0], false));
        assert!(!touch.update(&mut camera, 1, PenPhase::Move, [200.0, 100.0], false));
        assert_eq!(camera, before);
        touch.update(&mut camera, 2, PenPhase::Down, [300.0, 100.0], false);
        let anchor = camera.input_transform().map(Point { x: 250.0, y: 100.0 });
        assert!(touch.update(&mut camera, 2, PenPhase::Move, [200.0, 300.0], false));
        assert!((camera.zoom - before.zoom * 2.0).abs() < 0.001);
        assert!((camera.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        near(
            anchor,
            camera.input_transform().map(Point { x: 200.0, y: 200.0 }),
        );
        let before = camera.clone();
        touch.update(&mut camera, 3, PenPhase::Down, [400.0, 300.0], false);
        touch.update(&mut camera, 2, PenPhase::Up, [200.0, 300.0], false);
        assert_eq!(camera, before);
        touch.clear();
        assert!(!touch.update(&mut camera, 3, PenPhase::Move, [300.0, 300.0], false));
    }
}
