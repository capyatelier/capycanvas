use super::*;
use layer_core::color::RgbSpace;
use layer_core::package::{codec::CapturedPreview, preview::Preview};

impl SnapshotGpu {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn package_preview(&self, capture: &ArtworkCapture, cancelled: &AtomicBool) -> Option<CapturedPreview> {
        pollster::block_on(self.package_preview_async(capture, cancelled))
    }

    pub async fn package_preview_async(&self, capture: &ArtworkCapture, cancelled: &AtomicBool) -> Option<CapturedPreview> {
        self.render_package_preview(capture, cancelled).await.ok()
    }

    async fn render_package_preview(&self, capture: &ArtworkCapture, cancelled: &AtomicBool) -> Result<CapturedPreview, String> {
        let check = || if cancelled.load(Ordering::Relaxed) { Err("Preview cancelled".to_string()) } else { Ok(()) };
        check()?;
        let artwork = &capture.artwork;
        let output = artwork.outputs.get(artwork.default_output).ok_or("Missing default output")?;
        let composition = artwork.compositions.get(artwork.root).ok_or("Missing composition")?;
        if output.composition != artwork.root || output.frame.is_some_and(|frame| frame != (composition.origin, composition.size))
            || output.scale[0] != output.scale[1] {
            return Err("Unsupported preview framing".into());
        }
        for raster in artwork.paint.iter().map(|(_,_,p)| &p.raster).chain(artwork.coverage.iter().map(|(_,_,p)| &p.raster)) {
            raster.wait_data_cancellable(cancelled)?;
        }
        let mut renderer = self.capture(capture.clone(), Default::default()).map_err(|e| e.to_string())?;
        renderer.planned_pixel_bytes = 128 * 1024 * 1024;
        let extent = renderer.extent;
        let working = renderer.color().space;
        let guide = if renderer.sdr_rendition.is_some() { Some(renderer.local_tone_guide_async().await?) } else { None };
        let mut reduction = layer_color::AreaPreview::new(extent, [1024; 2])?;
        let mut y = 0;
        while y < extent[1] {
            check()?;
            let (height, pixels) = renderer.read_band_async(y).await.map_err(|e| e.to_string())?;
            for row in pixels.chunks_exact(extent[0] as usize) { reduction.push(row)?; }
            y += height;
        }
        check()?;
        let (size, mut pixels) = reduction.finish()?;
        let space = if let (Some(rendition), Some(guide)) = (renderer.sdr_rendition, guide) {
            let mapper = rendition.mapper(working, RgbSpace::Srgb);
            for (index, pixel) in pixels.iter_mut().enumerate() {
                let position = [index as u32 % size[0], index as u32 / size[0]];
                *pixel = mapper.map_local_premultiplied(*pixel,
                    std::array::from_fn(|i| (position[i] as f32 + 0.5) * extent[i] as f32 / size[i] as f32), &guide);
            }
            RgbSpace::Srgb
        } else { working };
        let bytes = SnapshotPreview { extent: size, space, pixels }.srgb_bytes()?;
        let preview = Preview::from_rgba(size, bytes.into())?;
        check()?;
        Ok(CapturedPreview { checkpoint: capture.checkpoint, context: output.context.clone(), preview })
    }
}
