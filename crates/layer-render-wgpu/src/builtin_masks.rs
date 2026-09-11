use super::*;

pub(super) type Pixels = (u32, u32, Vec<u8>);
type Generate = fn() -> Result<Pixels, GpuRasterError>;

type PreparedPixels = std::sync::Mutex<Option<Result<Pixels, GpuRasterError>>>;
struct Mask {
    id: AssetId,
    pixels: Deferred<PreparedPixels>,
    priority: u8,
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
                    priority: u8::MAX,
                    uploaded: false,
                })
                .collect(),
        )
    }
    pub fn style(
        &mut self,
        compiler: &startup::Compiler,
        style: &layer_render::DabStyle,
        priority: u8,
    ) {
        let key = WgpuRasterizer::texture_set_key(style);
        for id in [
            key.primary,
            key.grain,
            key.dual,
            key.dual_grain,
            key.transport,
        ] {
            if let Some(mask) = self.0.iter_mut().find(|m| m.id == id) {
                mask.priority = mask.priority.min(priority);
                compiler.pipeline(&mask.pixels, priority);
            }
        }
    }
    pub fn remaining(&mut self, compiler: &startup::Compiler) {
        for mask in &mut self.0 {
            mask.priority = mask.priority.min(startup::OTHER);
            compiler.pipeline(&mask.pixels, startup::OTHER);
        }
    }
    pub fn ready_through(&self, priority: u8) -> bool {
        self.0.iter().all(|m| m.priority > priority || m.uploaded)
    }
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
pub(super) fn builtin_masks() -> [(&'static str, Generate); 10] {
    fn square(pixels: Vec<u8>) -> Result<Pixels, GpuRasterError> {
        Ok((PROCEDURAL_GRAIN_SIZE, PROCEDURAL_GRAIN_SIZE, pixels))
    }
    [
        (WHITE_MASK_ASSET, || Ok((1, 1, vec![255]))),
        (PENCIL_TEXTURE_ASSET, || {
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/pencil-grain.pgm"))
                .ok_or(GpuRasterError::InvalidImage)
        }),
        (PAINTBRUSH_TEXTURE_ASSET, || {
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/paint-bristles.pgm"))
                .ok_or(GpuRasterError::InvalidImage)
        }),
        (PAPER_GRAIN_TEXTURE_ASSET, || {
            square(procedural_paper_grain())
        }),
        (BRISTLE_GRAIN_TEXTURE_ASSET, || {
            square(procedural_bristle_grain())
        }),
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
#[test]
fn generated_pixels_do_not_signal_readiness_before_upload() {
    let mut masks = Masks(vec![Mask {
        id: AssetId::from("test:tip"),
        pixels: Deferred::new(|| std::sync::Mutex::new(Some(Ok((1, 1, vec![255]))))),
        priority: 3,
        uploaded: false,
    }]);
    assert!(masks.ready_through(2));
    masks.0[0].pixels.compile();
    assert!(!masks.ready_through(3));
    assert_eq!(masks.take_ready().unwrap().len(), 1);
    assert!(masks.ready_through(3));
    assert!(masks.take_ready().unwrap().is_empty());
}
