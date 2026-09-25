use layer_render_wgpu::BackdropRegion;
use serde::Deserialize;

const MAX_REGIONS: usize = 256;

#[derive(Default, Deserialize, PartialEq)]
struct Request {
    regions: Vec<[f32; 8]>,
    connections: Vec<layer_ui::DrawerConnection>,
}

#[derive(Default)]
pub(crate) struct Glass {
    request: Request,
    scaled: Vec<BackdropRegion>,
    scale: f32,
}

impl Glass {
    pub(crate) fn set(&mut self, json: &str) -> Result<bool, String> {
        let request: Request = serde_json::from_str(json).map_err(|_| "Invalid glass geometry")?;
        if request.regions.len() + request.connections.len() > MAX_REGIONS
            || request
                .regions
                .iter()
                .any(|r| !r.iter().all(|v| v.is_finite()) || r[2] <= 0. || r[3] <= 0.)
        {
            return Err("Invalid glass geometry".into());
        }
        let changed = self.request != request;
        if changed {
            self.request = request;
            self.scale = 0.;
        }
        Ok(changed)
    }
    pub(crate) fn regions(&mut self, scale: f32) -> &[BackdropRegion] {
        if self.scale != scale {
            self.scale = scale;
            let boxes = self.request.regions.iter().map(|b| {
                BackdropRegion::rounded(
                    [b[0], b[1], b[2], b[3]].map(|v| v * scale),
                    [b[4], b[5], b[6], b[7]].map(|v| v * scale),
                    BackdropRegion::CIRCULAR,
                )
            });
            let connections = self.request.connections.iter().flat_map(|c| c.glass()).map(|(bounds, radii)| {
                BackdropRegion {
                    bounds: bounds.map(|v| v * scale),
                    radii: radii.map(|v| v * scale),
                    shape: BackdropRegion::CIRCULAR,
                }
            });
            self.scaled = boxes.chain(connections).collect();
        }
        &self.scaled
    }
    pub(crate) fn count(&self) -> usize {
        self.request.regions.len() + self.request.connections.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_regions_scale_once_and_reject_invalid_geometry() {
        let mut glass = Glass::default();
        assert!(glass.set(r#"{"regions":[[10,20,100,50,8,8,8,8]],"connections":[]}"#).unwrap());
        assert!(!glass.set(r#"{"regions":[[10,20,100,50,8,8,8,8]],"connections":[]}"#).unwrap());
        let region = glass.regions(1.5)[0];
        assert_eq!(region.bounds, [15., 30., 150., 75.]);
        assert_eq!(region.radii, [12.; 4]);
        assert_eq!(glass.count(), 1);
        assert!(glass.set(r#"{"regions":[[0,0,0,10,0,0,0,0]],"connections":[]}"#).is_err());
        assert!(glass.set(r#"{"regions":[],"connections":[]}"#).unwrap());
        assert!(glass.regions(1.5).is_empty());
    }
}
