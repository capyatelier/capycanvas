//! Latest-point coalescing and color policy, shared by every host. One GPU
//! sample can be in flight; neither pointer events nor frames wait for it.
use layer_render::{CanvasRenderer, ColorSampleRequest, ColorSampleSource};

#[derive(Default)]
pub(crate) struct Eyedropper {
    pub layer: bool,
    pub contact: bool,
    generation: u64,
    last: Option<(ColorSampleSource, [u32; 2])>,
    queued: Option<ColorSampleRequest>,
    pending: bool,
}
impl Eyedropper {
    pub fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.contact = false;
        self.last = None;
        self.queued = None;
        // Still drain an accepted request, but never apply its stale result.
    }
    pub fn queue(&mut self, source: ColorSampleSource, position: [u32; 2]) {
        if self.last == Some((source, position)) {
            return;
        }
        self.last = Some((source, position));
        self.queued = Some(ColorSampleRequest {
            request_id: self.generation,
            source,
            position,
        });
    }
    pub fn busy(&self) -> bool {
        self.pending || self.queued.is_some()
    }
    pub fn poll<R: CanvasRenderer>(
        &mut self,
        renderer: &mut R,
    ) -> Result<Option<[f32; 4]>, String> {
        if !self.busy() {
            return Ok(None);
        }
        let mut color = None;
        if let Some(result) = renderer.take_color_sample() {
            self.pending = false;
            let sample = result.map_err(|e| e.to_string())?;
            if sample.request_id == self.generation && sample.rgba[3] > 0.0 {
                let [r, g, b, _] = sample.rgba;
                let encode = |v: f32| {
                    let v = v.clamp(0.0, 1.0);
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                };
                // Pick paint color, not the existing pixel's transparency.
                color = Some([encode(r), encode(g), encode(b), 1.0]);
            }
        }
        if !self.pending
            && let Some(request) = self.queued
            && renderer
                .request_color_sample(request)
                .map_err(|e| e.to_string())?
        {
            self.queued = None;
            self.pending = true;
        }
        Ok(color)
    }
}
