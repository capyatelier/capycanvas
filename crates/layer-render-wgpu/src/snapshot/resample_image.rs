//! A canvas change that resamples an untouched photo keeps the photo's own
//! interpretation: the photo is rendered through the map, one tile at a time,
//! into float rows in its own primaries, and encoded at its own depth.
use super::*;
use layer_core::authored::{Affine64, Image, PaintBase, PortableId};
use layer_core::color::{ColorProfile, DocumentColor, RgbSpace, SampleDepth, source::{SourceBuilder, SourceImage}};
use layer_core::raster::TileBlob;

fn with_alpha(channels: SourceChannels) -> SourceChannels {
    match channels {
        SourceChannels::Gray => SourceChannels::GrayAlpha,
        SourceChannels::Rgb => SourceChannels::Rgba,
        other => other,
    }
}

/// Whether every alpha sample of an encoded row is fully opaque.
fn opaque(row: &[u8], interpretation: &SourceInterpretation) -> bool {
    let bytes = interpretation.depth.bytes();
    row.chunks_exact(interpretation.pixel_bytes()).all(|pixel| {
        let alpha = &pixel[pixel.len() - bytes..];
        match interpretation.depth {
            SampleDepth::U8 => alpha[0] == u8::MAX,
            SampleDepth::U16 => u16::from_le_bytes([alpha[0], alpha[1]]) == u16::MAX,
            SampleDepth::F16 => u16::from_le_bytes([alpha[0], alpha[1]]) >= 0x3c00,
            SampleDepth::F32 => f32::from_le_bytes([alpha[0], alpha[1], alpha[2], alpha[3]]) >= 1.,
        }
    })
}

/// The same samples without their last, alpha, channel.
fn without_alpha(image: SourceImage, channels: SourceChannels) -> Result<SourceImage, String> {
    let from = image.interpretation.pixel_bytes();
    let interpretation = SourceInterpretation { channels, ..image.interpretation.clone() };
    let to = interpretation.pixel_bytes();
    let descriptor = interpretation.descriptor();
    let tiles = image.tiles.iter().map(|(coordinate, tile)| {
        let bytes: Vec<u8> = tile.decode()?.chunks_exact(from).flat_map(|pixel| pixel[..to].to_vec()).collect();
        Ok((*coordinate, Arc::new(TileBlob::encode(descriptor, &bytes)?)))
    }).collect::<Result<_, String>>()?;
    Ok(SourceImage { interpretation, tiles, ..image })
}

impl SnapshotGpu {
    /// Run a canvas change's sample moves, resampling retained photos here.
    pub async fn remap(&self, plan: layer_core::RemapPlan, control: CaptureControl) -> Result<Vec<layer_core::RemapResult>, String> {
        let mut resampled = std::collections::BTreeMap::new();
        for (target, image, extent, to_image, interpolation) in plan.resamples() {
            let samples = self.resample_image(image.storage(), extent, to_image, interpolation, control.clone()).await?;
            resampled.insert(target, Image::new(Arc::new(samples)));
        }
        plan.run(&plan.scene.artwork, &resampled, control.cancellation_flag())
    }

    /// `source` resampled through `to_image` into an image of `extent`, in the
    /// source's own channels, depth and profile. RGB and gray gain alpha only
    /// where the result exposes transparency.
    pub async fn resample_image(&self, source: &Arc<SourceImage>, extent: [u32; 2], to_image: Affine64,
        interpolation: layer_core::Interpolation, control: CaptureControl,
    ) -> Result<SourceImage, String> {
        let space = match source.interpretation.profile { ColorProfile::Builtin(space) => space, ColorProfile::Icc(_) => RgbSpace::ProPhoto };
        let mut document = layer_core::Document::new(PortableId::random(), extent[0], extent[1], layer_core::DocumentNames { paint: "".into(), paper: "".into() });
        let root = document.artwork.root;
        document.artwork.compositions.get_mut(root).unwrap().color = DocumentColor { space, depth: SampleDepth::F32 };
        let Some(SourceTarget::Paint(paint)) = document.working.target else { return Err("Missing resampling layer".into()); };
        let layer = document.artwork.paint.get_mut(paint).unwrap();
        layer.domain = source.extent;
        layer.base = Some(PaintBase::new(Image::new(source.clone())));
        let stack = document.composition().result;
        document.artwork.stacks.get_mut(stack).unwrap().entries.truncate(1);
        let target = SourceTarget::Paint(paint);
        let placement = layer_core::LayerPlacement { interpolation, ..layer_core::LayerPlacement::from_affine(layer_core::Affine(to_image.0.map(|v| v as f32))) };
        let plan = layer_core::TransformPixelsPlan {
            target, scope: layer_core::TransformPixelsScope::Paint { linked_mask: false }, scene: document.snapshot(),
            output: layer_core::Edit::Batch(Vec::new()), map: placement.clone(), source: None,
            geometry: layer_core::ImageTransform { placement, ..Default::default() },
            origin: [0; 2], extent, paint: Some(paint), coverage: None,
        };
        let mut snapshot = SnapshotRenderer::construct(plan.scene.clone(), SceneScope::Raw(target), control.clone(), self).map_err(|e| e.to_string())?;
        snapshot.extent = extent;
        snapshot.set_raw_plan(plan.clone());
        let planes = snapshot.raw_planes.clone();
        snapshot.renderer.ensure_document_metadata(extent, snapshot.scene.view().with_scope(&snapshot.scope)).map_err(|e| e.to_string())?;
        let interpretation = SourceInterpretation { channels: with_alpha(source.interpretation.channels), ..source.interpretation.clone() };
        let encoder = layer_color::WorkingEncoder::new(space, &interpretation, Default::default())?;
        let mut builder = SourceBuilder::new(extent, interpretation.clone(), layer_core::ProjectLimits::default().asset_bytes as usize)?;
        let width = extent[0] as usize;
        let mut encoded = vec![0; width * interpretation.pixel_bytes()];
        let mut covered = true;
        for row in 0..extent[1].div_ceil(PAGE_SIZE) {
            let rows = PAGE_SIZE.min(extent[1] - row * PAGE_SIZE) as usize;
            let mut band = vec![[0f32; 4]; width * rows];
            for column in 0..extent[0].div_ceil(PAGE_SIZE) {
                control.check().map_err(|e| e.to_string())?;
                let coordinate = [column, row];
                let region = page_rect(coordinate).intersect(PixelRect::full(extent));
                let (tiles, capture) = snapshot.with_region_gpu([region.min_x(), region.min_y(), region.width(), region.height()], 8 * 1024 * 1024,
                    |r, packet, _, encoder| {
                        let mut scene = r.scene.take().unwrap_or_else(|| scene::Scene::new(r));
                        let result = scene.capture_raw_tile(r, packet, coordinate, &plan, &planes, encoder);
                        r.scene = Some(scene);
                        result
                    }).await.map_err(|e| e.to_string())?;
                #[cfg(not(target_arch = "wasm32"))]
                capture.finish()?;
                #[cfg(target_arch = "wasm32")]
                capture.finish_browser(self.encoder.as_ref().ok_or("Raster worker is unavailable")?).await?;
                let (_, tile) = tiles.into_iter().find(|(plane, _)| *plane == RasterPlane::Color).ok_or("Missing resampled pixels")?;
                let bytes = tile.try_backing().ok_or("Resampled pixels are not ready")??.decode()?;
                for y in 0..region.height() as usize {
                    for x in 0..region.width() as usize {
                        let at = (y * PAGE_SIZE as usize + x) * 16;
                        band[y * width + column as usize * PAGE_SIZE as usize + x] =
                            std::array::from_fn(|c| f32::from_le_bytes(bytes[at + 4 * c..at + 4 * c + 4].try_into().unwrap()));
                    }
                }
            }
            for (line, pixels) in band.chunks_exact(width).enumerate() {
                encoder.encode_straight(pixels, &mut encoded, None, [0, row * PAGE_SIZE + line as u32])?;
                covered &= opaque(&encoded, &interpretation);
                builder.push_row(&encoded)?;
            }
        }
        control.check().map_err(|e| e.to_string())?;
        let mut image = builder.finish()?;
        image.resolution = source.resolution;
        if covered && interpretation.channels != source.interpretation.channels {
            image = without_alpha(image, source.interpretation.channels)?;
        }
        image.validate()?;
        Ok(image)
    }
}
