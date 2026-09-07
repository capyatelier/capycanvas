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
            translation: [0.0; 2],
            work_area: [0.0, 0.0, viewport[0] as f32, viewport[1] as f32],
        };
        value.fit(document);
        value
    }

    pub fn fit(&mut self, document: [u32; 2]) {
        let [x, y, width, height] = self.work_area;
        self.zoom = ((width * 0.9 / document[0].max(1) as f32)
            .min(height * 0.9 / document[1].max(1) as f32))
        .clamp(0.02, 16.0);
        self.rotation = 0.0;
        self.translation = [
            x + (width - document[0] as f32 * self.zoom) * 0.5,
            y + (height - document[1] as f32 * self.zoom) * 0.5,
        ];
        self.revision += 1;
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
        [
            cos * self.zoom,
            sin * self.zoom,
            -sin * self.zoom,
            cos * self.zoom,
            self.translation[0],
            self.translation[1],
        ]
    }

    pub fn input_transform(&self) -> ViewTransform {
        let (sin, cos) = self.rotation.sin_cos();
        let a = cos / self.zoom;
        let b = -sin / self.zoom;
        let c = -b;
        let d = a;
        let [x, y] = self.translation;
        ViewTransform {
            revision: self.revision,
            surface_to_document: [a, b, c, d, -a * x - c * y, -b * x - d * y],
        }
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
    ) -> bool {
        if !point.into_iter().all(f32::is_finite) {
            return false;
        }
        let before = self.pair();
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
    fn two_fingers_rotate_zoom_and_pan_without_lifecycle_jumps() {
        let mut camera = Camera::new([1000, 1000], [1000, 1000]);
        let mut touch = TouchGesture::default();
        let before = camera.clone();
        assert!(!touch.update(&mut camera, 1, PenPhase::Down, [100.0, 100.0]));
        assert!(!touch.update(&mut camera, 1, PenPhase::Move, [200.0, 100.0]));
        assert_eq!(camera, before);
        touch.update(&mut camera, 2, PenPhase::Down, [300.0, 100.0]);
        let anchor = camera.input_transform().map(Point { x: 250.0, y: 100.0 });
        assert!(touch.update(&mut camera, 2, PenPhase::Move, [200.0, 300.0]));
        assert!((camera.zoom - before.zoom * 2.0).abs() < 0.001);
        assert!((camera.rotation - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        near(
            anchor,
            camera.input_transform().map(Point { x: 200.0, y: 200.0 }),
        );
        let before = camera.clone();
        touch.update(&mut camera, 3, PenPhase::Down, [400.0, 300.0]);
        touch.update(&mut camera, 2, PenPhase::Up, [200.0, 300.0]);
        assert_eq!(camera, before);
        touch.clear();
        assert!(!touch.update(&mut camera, 3, PenPhase::Move, [300.0, 300.0]));
    }
}
