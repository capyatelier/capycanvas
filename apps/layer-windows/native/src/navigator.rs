//! Native UI geometry only. The shared presenter samples the live GPU composition.
use layer_host::NativeHost;
use layer_render_wgpu::OverviewPlacement;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    bounds: [f32; 4],
    clip: [f32; 4],
    order: i32,
}
#[derive(Default)]
pub(crate) struct Navigator {
    slots: Vec<Slot>,
    placements: Vec<OverviewPlacement>,
}
impl Navigator {
    pub(crate) fn set(&mut self, json: &str) -> Result<bool, String> {
        let mut slots: Vec<Slot> =
            serde_json::from_str(json).map_err(|_| "Invalid Navigator geometry")?;
        if slots.len() > 32
            || slots.iter().any(|s| {
                !s.bounds.iter().chain(&s.clip).all(|v| v.is_finite())
                    || s.bounds[2] <= 0.
                    || s.bounds[3] <= 0.
                    || s.clip[2] <= 0.
                    || s.clip[3] <= 0.
            })
        {
            return Err("Invalid Navigator geometry".into());
        }
        slots.sort_by_key(|s| s.order);
        let changed = self.slots != slots;
        self.slots = slots;
        Ok(changed)
    }
    pub(crate) fn placements(&mut self, host: &NativeHost, scale: f32) -> &[OverviewPlacement] {
        self.placements.clear();
        if !host.startup.canvas_ready {
            return &self.placements;
        }
        let state = host.session.state();
        let document = host.session.engine().document();
        let fg = state.palette.text.linear();
        let bg = state.palette.panel.linear();
        for slot in &self.slots {
            let [x, y, w, h] = slot.bounds;
            let Some(g) = layer_ui::NavigatorGeometry::new(
                &state.camera,
                [document.width, document.height],
                [w, h],
            ) else {
                continue;
            };
            self.placements.push(OverviewPlacement {
                bounds: [
                    (x + g.image.x) * scale,
                    (y + g.image.y) * scale,
                    g.image.width * scale,
                    g.image.height * scale,
                ],
                clip: Some(slot.clip.map(|v| v * scale)),
                work_area: g.work_area.map(|[a, b]| [(x + a) * scale, (y + b) * scale]),
                outline_linear: [fg[0], fg[1], fg[2]],
                background_linear: [bg[0], bg[1], bg[2]],
                scale,
                opacity: 1.,
            });
        }
        &self.placements
    }
}
/// Shared image bounds for the transparent native UI cutout. No live host access.
///
/// # Safety
/// `output` must point to four writable, aligned floats for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_navigator_image(
    width: f32,
    height: f32,
    document_width: u32,
    document_height: u32,
    output: *mut f32,
) -> bool {
    if output.is_null() {
        return false;
    }
    let document = [document_width, document_height];
    let camera = layer_ui::Camera::new(document, document);
    let Some(g) = layer_ui::NavigatorGeometry::new(&camera, document, [width, height]) else {
        return false;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(
            [g.image.x, g.image.y, g.image.width, g.image.height].as_ptr(),
            output,
            4,
        );
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_validation_is_atomic_and_camera_updates_reuse_storage() {
        let mut navigator = Navigator::default();
        let slots = r#"[{"bounds":[20,30,120,90],"clip":[22,32,116,86],"order":1}]"#;
        assert!(navigator.set(slots).unwrap());
        assert!(!navigator.set(slots).unwrap());
        assert!(
            navigator
                .set(r#"[{"bounds":[20,30,-1,90],"clip":[22,32,116,86],"order":1}]"#)
                .is_err()
        );
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.startup.canvas_ready = true;
        let p = navigator.placements(&host, 2.)[0];
        assert_eq!(p.clip, Some([44., 64., 232., 172.]));
        let address = navigator.placements.as_ptr();
        host.dispatch(layer_ui::UiAction::Invoke {
            command: layer_ui::CommandId::RotateRight,
        })
        .unwrap();
        let rotated = navigator.placements(&host, 2.)[0];
        assert_eq!(rotated.bounds, p.bounds);
        assert_ne!(rotated.work_area, p.work_area);
        assert_eq!(navigator.placements.as_ptr(), address);
        let scaled = navigator.placements(&host, 1.)[0];
        assert_eq!(scaled.bounds, rotated.bounds.map(|v| v / 2.));
        assert!(navigator.set("[]").unwrap());
        assert!(navigator.placements(&host, 1.).is_empty());
    }
}
