use super::*;

pub(super) type Pixels = (u32, u32, Vec<u8>);
type Generate = fn() -> Result<Pixels, GpuRasterError>;

type PreparedPixels = std::sync::Mutex<Option<Result<Pixels, GpuRasterError>>>;
struct Mask {
    id: AssetId,
    pixels: Deferred<PreparedPixels>,
    requested: bool,
    uploaded: bool,
}
pub(super) struct Masks(Vec<Mask>);
impl Masks {
    pub fn new() -> Self {
        Self(
            builtin_masks()
                .into_iter()
                .filter(|(id, _)| *id != WHITE_MASK_ASSET)
                .map(|(id, generate)| Mask {
                    id: AssetId::from(id),
                    pixels: Deferred::new(move || std::sync::Mutex::new(Some(generate()))),
                    requested: false,
                    uploaded: false,
                })
                .collect(),
        )
    }
    pub fn style(
        &mut self,
        compiler: &compile::Scope,
        style: &layer_render::DabStyle,
        priority: startup::Class,
    ) {
        let key = WgpuRasterizer::texture_set_key(style);
        for id in [key.primary, key.grain, key.transport] {
            if let Some(mask) = self.0.iter_mut().find(|m| m.id == id) {
                mask.requested = true;
                compiler.require([&mask.pixels], priority);
            }
        }
    }
    pub fn generates(&self, id: &AssetId) -> bool {
        self.0.iter().any(|mask| mask.id == *id)
    }
    pub fn ready(&self) -> bool { self.0.iter().all(|m| !m.requested || m.uploaded) }
    pub fn take_ready(&mut self) -> Result<Vec<(AssetId, Pixels)>, GpuRasterError> {
        let mut ready = Vec::new();
        for mask in &mut self.0 {
            if !mask.uploaded
                && mask.pixels.ready()
                && let Some(result) = mask.pixels.compile().lock().unwrap().take()
            {
                ready.push((mask.id.clone(), result?));
                mask.uploaded = true;
            }
        }
        Ok(ready)
    }
}

// Recipes are cheap to enumerate. Procedural textures are generated only when
// selected by the startup dependency queue (or immediately by eager hosts).
pub(super) fn builtin_masks() -> [(&'static str, Generate); 8] {
    fn square(pixels: Vec<u8>) -> Result<Pixels, GpuRasterError> {
        Ok((PROCEDURAL_GRAIN_SIZE, PROCEDURAL_GRAIN_SIZE, pixels))
    }
    [
        (WHITE_MASK_ASSET, || Ok((1, 1, vec![255]))),
        (bristle_table::HAIRS_ASSET, || Ok(bristle_table::hairs())),
        (bristle_table::FIELD_ASSET, || Ok(bristle_table::field())),
        (WATERCOLOR_TIP_TEXTURE_ASSET, || {
            square(procedural_watercolor_tip())
        }),
        (WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::LongNarrow))
        }),
        (WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::LongBroad))
        }),
        (WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::ShortNarrow))
        }),
        (WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::ShortBroad))
        }),
    ]
}

#[cfg(test)]
#[path = "builtin_masks_tests.rs"]
mod tests;
